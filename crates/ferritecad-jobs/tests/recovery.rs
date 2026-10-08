// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
//! §30M: the last published crash copy of the accepted model, without a window
//! and without a geometry kernel. Crashes are real: the test binary starts itself
//! as a child process, which makes and accepts versions, waits until the recorder
//! says the copy is written, and is then killed by its own PID.

use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ferritecad_document::{Document, DocumentVersion};
use ferritecad_jobs::{
    CreateDocumentRequest, DocumentSession, Ending, HistoryLimits, NewDocument, PlateSize,
    Publication, RecordId, RecoveryEntry, RecoveryHooks, RecoveryRecorder, RecoveryStatus,
    RecoveryStore, RefusalKind, STALE_RECOVERY, SaveFailureKind, SaveTarget, UNTITLED,
    create_document, read_extrude_source,
};
use ferritecad_kernel::{OperationContext, mock::MockKernel};
use ferritecad_types::{CadError, ContentHash, ObjectId, Result};

const CHILD: &str = "child_process_entry";
const MODE: &str = "FCAD_RECOVERY_CHILD_MODE";
const ROOT: &str = "FCAD_RECOVERY_CHILD_ROOT";
const WORK: &str = "FCAD_RECOVERY_CHILD_WORK";
const SAY: &str = "FCAD-CHILD ";

fn plate() -> NewDocument {
    NewDocument::SamplePlate(PlateSize {
        width: 70.0,
        depth: 40.0,
        height: 13.0,
    })
}

fn no_kernel() -> Result<MockKernel> {
    Err(CadError::unsupported("no kernel was expected here"))
}

fn make_plate(path: &Path) {
    create_document(
        CreateDocumentRequest::new(path, plate(), "keep"),
        &OperationContext::default(),
    )
    .expect("a plate");
}

fn feature_of(session: &DocumentSession) -> ObjectId {
    read_extrude_source(session.current().path())
        .expect("reading")
        .features[0]
        .feature
}

fn apply(session: &mut DocumentSession, millimetres: f64) {
    let feature = feature_of(session);
    let step = session
        .begin_step()
        .edit_extrude_height(
            feature,
            millimetres,
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("an edit");
    session.commit_step(step).expect("accepted");
}

fn undo(session: &mut DocumentSession) {
    session
        .commit_move(session.begin_undo().expect("undo"))
        .expect("shown");
}

fn redo(session: &mut DocumentSession) {
    session
        .commit_move(session.begin_redo().expect("redo"))
        .expect("shown");
}

fn version_of(path: &Path) -> (DocumentVersion, ContentHash) {
    let document = Document::open_read_only(path).expect("document");
    let version = DocumentVersion {
        document_id: document.meta().document_id,
        content: document.content_version().expect("content"),
    };
    let model = document.model_version().expect("model");
    document.close().expect("close");
    (version, model)
}

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("bytes")
}

fn names(directory: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(directory)
        .expect("directory")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

/// Waits until the recorder says the newest request is written.
fn wait_written(recorder: &RecoveryRecorder) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match recorder.status() {
            RecoveryStatus::Written { unix_ms } if recorder.settled() => return unix_ms,
            RecoveryStatus::Failed(message) => panic!("the copy failed: {message}"),
            _ if Instant::now() > deadline => panic!("the copy was never written"),
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn wait_status(recorder: &RecoveryRecorder, wanted: impl Fn(&RecoveryStatus) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !(recorder.settled() && wanted(&recorder.status())) {
        assert!(Instant::now() < deadline, "status {:?}", recorder.status());
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn record_dirs(root: &Path) -> Vec<String> {
    names(root)
        .into_iter()
        .filter(|name| name.starts_with("r-"))
        .collect()
}

// --- the child ---------------------------------------------------------------------

/// Says something the parent waits for, then waits to be killed.
fn say_and_hang(line: &str) -> ! {
    println!("{SAY}{line}");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

/// Stops inside a publication, at one phase, for the parent to kill.
struct StopAt(&'static str);
impl RecoveryHooks for StopAt {
    fn after_copy(&mut self) {
        if self.0 == "after-copy" {
            say_and_hang("MIDWAY");
        }
    }
    fn after_copy_published(&mut self) {
        if self.0 == "after-copy-published" {
            say_and_hang("MIDWAY");
        }
    }
    fn after_manifest_written(&mut self) {
        if self.0 == "after-manifest-written" {
            say_and_hang("MIDWAY");
        }
    }
}

/// The child process. Ignored, so it runs only when a parent asks for it by name.
#[test]
#[ignore = "run by the crash tests as a child process"]
fn child_process_entry() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let root = PathBuf::from(std::env::var_os(ROOT).expect("root"));
    let work = PathBuf::from(std::env::var_os(WORK).expect("work"));
    let store = RecoveryStore::open(&root).expect("store");
    let sessions = work.join("sessions");
    std::fs::create_dir_all(&sessions).expect("sessions");
    let witness = work.join("witness.fcad");

    // One recorder for the child's whole life, as a window has.
    let mut recorder = RecoveryRecorder::start(store.clone(), || {});
    let accepted = |mut session: DocumentSession, mut recorder: RecoveryRecorder| -> ! {
        recorder.observe(&mut session);
        wait_written(&recorder);
        let current = session.current();
        let source = Document::open_read_only(current.path()).expect("current");
        source.snapshot_to(&witness).expect("witness");
        source.close().expect("close");
        // Held, never finished: the process dies with the recorder running.
        std::mem::forget(recorder);
        std::mem::forget(session);
        say_and_hang(&format!("READY {}", current.version().content));
    };

    match mode.as_str() {
        "named-dirty" => {
            let mut session = DocumentSession::open_in(
                &sessions,
                &work.join("plate.fcad"),
                HistoryLimits::default(),
            )
            .expect("open");
            recorder.observe(&mut session);
            apply(&mut session, 22.0);
            accepted(session, recorder);
        }
        "untitled-empty" => {
            let session = DocumentSession::create_document_in(
                &sessions,
                HistoryLimits::default(),
                NewDocument::Empty,
                no_kernel,
                &OperationContext::default(),
            )
            .expect("new");
            accepted(session, recorder);
        }
        "undo-redo" => {
            let mut session = DocumentSession::create_document_in(
                &sessions,
                HistoryLimits::default(),
                plate(),
                no_kernel,
                &OperationContext::default(),
            )
            .expect("new");
            // Every accepted version is observed, as the window does.
            recorder.observe(&mut session);
            apply(&mut session, 22.0);
            recorder.observe(&mut session);
            apply(&mut session, 31.0);
            recorder.observe(&mut session);
            undo(&mut session);
            recorder.observe(&mut session);
            redo(&mut session);
            recorder.observe(&mut session);
            undo(&mut session);
            accepted(session, recorder);
        }
        "lease" => {
            let session = DocumentSession::create_document_in(
                &sessions,
                HistoryLimits::default(),
                plate(),
                no_kernel,
                &OperationContext::default(),
            )
            .expect("new");
            accepted(session, recorder);
        }
        phase @ ("after-copy" | "after-copy-published" | "after-manifest-written") => {
            let mut session = DocumentSession::create_document_in(
                &sessions,
                HistoryLimits::default(),
                plate(),
                no_kernel,
                &OperationContext::default(),
            )
            .expect("new");
            let first = session.current();
            let mut record = store.create_record().expect("record");
            record.publish(1, &first, "plate").expect("first copy");
            let source = Document::open_read_only(first.path()).expect("current");
            source.snapshot_to(&witness).expect("witness");
            source.close().expect("close");
            apply(&mut session, 22.0);
            drop(recorder);
            let phase: &'static str = match phase {
                "after-copy" => "after-copy",
                "after-copy-published" => "after-copy-published",
                _ => "after-manifest-written",
            };
            let _ = record.publish_with(2, &session.current(), "plate", &mut StopAt(phase));
            unreachable!("the hook hangs");
        }
        other => panic!("unknown child mode {other}"),
    }
}

/// A child process in `mode`, and what it said before it hung.
struct Crashed {
    child: Child,
    said: String,
}

fn start_child(mode: &str, root: &Path, work: &Path) -> Crashed {
    let mut child = Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            CHILD,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(MODE, mode)
        .env(ROOT, root)
        .env(WORK, work)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("a child process");
    let stdout = child.stdout.take().expect("stdout");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            // libtest may have started the line with the test's name.
            if let Some(at) = line.find(SAY) {
                let _ = sender.send(line[at + SAY.len()..].to_owned());
                break;
            }
        }
    });
    let said = receiver
        .recv_timeout(Duration::from_secs(120))
        .unwrap_or_else(|_| {
            let _ = child.kill();
            panic!("the {mode} child said nothing")
        });
    Crashed { child, said }
}

impl Crashed {
    /// Kills this child, and only this child, by its own PID.
    fn kill(mut self) -> String {
        self.child.kill().expect("killing our own child");
        let status = self.child.wait().expect("reaped");
        assert!(!status.success(), "the child was killed, not finished");
        self.said
    }
}

fn recoverable(store: &RecoveryStore) -> Vec<ferritecad_jobs::RecoverySummary> {
    store.list().expect("list").recoverable().cloned().collect()
}

// --- the tests ------------------------------------------------------------------------

#[test]
fn a_killed_window_leaves_its_last_published_copy_for_named_untitled_and_undo_redo_sessions() {
    for mode in ["named-dirty", "untitled-empty", "undo-redo"] {
        let root = tempfile::tempdir().expect("root");
        let work = tempfile::tempdir().expect("work");
        let user = tempfile::tempdir().expect("user");
        let original = work.path().join("plate.fcad");
        make_plate(&original);
        let original_bytes = bytes(&original);

        let said = start_child(mode, root.path(), work.path()).kill();
        let content: ContentHash = said
            .strip_prefix("READY ")
            .expect("ready")
            .parse()
            .expect("hash");
        let witness = work.path().join("witness.fcad");
        let (accepted, accepted_model) = version_of(&witness);
        assert_eq!(accepted.content, content, "{mode}");

        // Found by another process, once, as the version accepted before the kill.
        let store = RecoveryStore::open(root.path()).expect("store");
        let found = recoverable(&store);
        assert_eq!(found.len(), 1, "{mode}: {found:?}");
        assert_eq!(found[0].document_id, accepted.document_id, "{mode}");
        assert_eq!(found[0].content, accepted.content, "{mode}");
        assert_eq!(found[0].model, accepted_model, "{mode}");
        let expected_name = match mode {
            "named-dirty" => "plate.fcad",
            _ => UNTITLED,
        };
        assert_eq!(found[0].name, expected_name, "{mode}");

        let sessions = tempfile::tempdir().expect("sessions");
        let claim = store.claim(found[0].record).expect("claim");
        let mut recovered =
            DocumentSession::recover_in(sessions.path(), HistoryLimits::default(), claim)
                .expect("recovered");
        // Every SQL cell, row identity and reference: the complete content version.
        assert_eq!(recovered.current().version(), accepted, "{mode}");
        assert_eq!(recovered.current().model(), accepted_model, "{mode}");
        assert!(recovered.is_untitled() && recovered.is_recovered() && recovered.is_dirty());
        assert_eq!(recovered.logical_path(), None);
        assert_eq!(recovered.saved_version(), None);
        assert_eq!(
            recovered.display_name(),
            format!("{expected_name} (recovered)")
        );
        assert!(!recovered.can_undo(), "the history is not recovered");

        // No right to save in place over the file it came from.
        let refused = recovered
            .begin_save(SaveTarget::InPlace)
            .run(&OperationContext::default())
            .expect_err("no file");
        assert_eq!(refused.kind, SaveFailureKind::Failed);
        // An occupied Save As is refused.
        let refused = recovered
            .begin_save(SaveTarget::As(original.clone()))
            .run(&OperationContext::default())
            .expect_err("occupied");
        assert_eq!(refused.kind, SaveFailureKind::Occupied);
        assert_eq!(bytes(&original), original_bytes, "{mode}: source untouched");
        let saved_path = user.path().join("recovered.fcad");
        let saved = recovered
            .begin_save(SaveTarget::As(saved_path.clone()))
            .run(&OperationContext::default())
            .expect("save as");
        recovered.record_saved(&saved);
        assert!(!recovered.is_dirty());
        assert_eq!(version_of(&saved_path), (accepted, accepted_model));
        // The claim is still held by the session; dropping it lets the record go.
        assert!(recovered.take_recovery_claim().is_some());
        assert_eq!(bytes(&original), original_bytes, "{mode}: source untouched");
    }
}

#[test]
fn a_live_lease_is_never_offered_claimed_or_deleted_and_death_releases_it() {
    let root = tempfile::tempdir().expect("root");
    let work = tempfile::tempdir().expect("work");
    let child = start_child("lease", root.path(), work.path());
    let store = RecoveryStore::open(root.path()).expect("store");
    let listing = store.list().expect("list");
    assert_eq!(listing.active, 1);
    assert_eq!(listing.entries.len(), 0, "{:?}", listing.entries);
    let record: RecordId = record_dirs(root.path())[0]
        .strip_prefix("r-")
        .expect("named")
        .parse()
        .expect("id");
    assert_eq!(
        store.claim(record).map(|_| ()).expect_err("refused").kind,
        RefusalKind::Active
    );
    assert_eq!(
        store.delete(record).expect_err("refused").kind,
        RefusalKind::Active
    );
    assert_eq!(record_dirs(root.path()).len(), 1);

    child.kill();
    let claim = store.claim(record).expect("an orphan now");
    // Held: nobody else, in this process or another, can take it meanwhile.
    assert_eq!(
        store.claim(record).map(|_| ()).expect_err("refused").kind,
        RefusalKind::Active
    );
    assert_eq!(store.list().expect("list").active, 1);
    drop(claim);
    assert_eq!(recoverable(&store).len(), 1);
}

#[test]
fn a_crash_in_every_phase_of_a_publication_leaves_the_previous_whole_copy() {
    for phase in [
        "after-copy",
        "after-copy-published",
        "after-manifest-written",
    ] {
        let root = tempfile::tempdir().expect("root");
        let work = tempfile::tempdir().expect("work");
        let said = start_child(phase, root.path(), work.path()).kill();
        assert_eq!(said, "MIDWAY", "{phase}");
        let (first, first_model) = version_of(&work.path().join("witness.fcad"));

        let store = RecoveryStore::open(root.path()).expect("store");
        let found = recoverable(&store);
        assert_eq!(found.len(), 1, "{phase}");
        assert_eq!(
            found[0].content, first.content,
            "{phase}: the previous copy"
        );
        assert_eq!(found[0].sequence, 1, "{phase}");
        let directory = root.path().join(format!("r-{}", found[0].record));
        let left = names(&directory);
        assert!(
            left.iter()
                .any(|name| name.ends_with(".partial") || name == "c2.fcad"),
            "{phase}: the interrupted publication left its partial names: {left:?}"
        );

        let sessions = tempfile::tempdir().expect("sessions");
        let claim = store.claim(found[0].record).expect("claim");
        let mut recovered =
            DocumentSession::recover_in(sessions.path(), HistoryLimits::default(), claim)
                .expect("recovered");
        assert_eq!(recovered.current().version(), first, "{phase}");
        assert_eq!(recovered.current().model(), first_model, "{phase}");
        // Adopted as the recovered session's own record: the partial names go.
        let record = recovered
            .take_recovery_claim()
            .expect("claim")
            .into_record()
            .expect("record");
        assert_eq!(
            names(&directory),
            ["c1.fcad", "lease", "manifest"],
            "{phase}"
        );
        record.retire().expect("retired");
        assert!(record_dirs(root.path()).is_empty());
    }
}

fn new_plate_session(root: &Path) -> DocumentSession {
    DocumentSession::create_document_in(
        root,
        HistoryLimits::default(),
        plate(),
        no_kernel,
        &OperationContext::default(),
    )
    .expect("new")
}

#[test]
fn an_older_request_never_replaces_a_newer_copy() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let store = RecoveryStore::open(root.path()).expect("store");
    let mut session = new_plate_session(sessions.path());
    let first = session.current();
    apply(&mut session, 22.0);
    let second = session.current();

    let mut record = store.create_record().expect("record");
    assert!(matches!(
        record.publish(5, &second, "plate"),
        Ok(Publication::Written { sequence: 1, .. })
    ));
    let late = record.publish(4, &first, "plate").expect_err("stale");
    assert!(late.to_string().contains(STALE_RECOVERY), "{late}");
    let late = record.clear(5).expect_err("stale");
    assert!(late.to_string().contains(STALE_RECOVERY), "{late}");
    // The same model again writes nothing.
    assert!(matches!(
        record.publish(6, &second, "plate"),
        Ok(Publication::Unchanged { sequence: 1, .. })
    ));
    drop(record);
    let found = recoverable(&store);
    assert_eq!(found[0].content, second.version().content);
}

#[test]
fn save_discard_cancel_and_failed_saves_end_the_record_as_the_person_decided() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let store = RecoveryStore::open(root.path()).expect("store");

    // A clean Open has nothing to keep.
    let named = user.path().join("plate.fcad");
    make_plate(&named);
    let mut session =
        DocumentSession::open_in(sessions.path(), &named, HistoryLimits::default()).expect("open");
    let mut recorder = RecoveryRecorder::start(store.clone(), || {});
    recorder.observe(&mut session);
    wait_status(&recorder, |status| *status == RecoveryStatus::Off);
    apply(&mut session, 22.0);
    recorder.observe(&mut session);
    wait_written(&recorder);
    assert_eq!(record_dirs(root.path()).len(), 1);
    // Undo back to what is saved: the copy goes, the record stays held.
    undo(&mut session);
    recorder.observe(&mut session);
    wait_status(&recorder, |status| *status == RecoveryStatus::Off);
    let directory = root.path().join(&record_dirs(root.path())[0]);
    assert_eq!(names(&directory), ["lease"]);
    redo(&mut session);
    recorder.observe(&mut session);
    wait_written(&recorder);

    // A failed Save keeps it.
    let taken = user.path().join("taken.fcad");
    std::fs::write(&taken, b"theirs").expect("occupied");
    let refused = session
        .begin_save(SaveTarget::As(taken.clone()))
        .run(&OperationContext::default())
        .expect_err("occupied");
    assert_eq!(refused.kind, SaveFailureKind::Occupied);
    recorder.observe(&mut session);
    wait_written(&recorder);
    assert!(directory.join("manifest").exists());

    // A published Save empties it.
    let saved = session
        .begin_save(SaveTarget::InPlace)
        .run(&OperationContext::default())
        .expect("saved");
    session.record_saved(&saved);
    recorder.observe(&mut session);
    wait_status(&recorder, |status| *status == RecoveryStatus::Off);
    assert_eq!(names(&directory), ["lease"]);

    // Discard: a dirty document replaced by another one that was accepted.
    apply(&mut session, 31.0);
    recorder.observe(&mut session);
    wait_written(&recorder);
    let mut replacement = new_plate_session(sessions.path());
    recorder.observe(&mut replacement);
    wait_written(&recorder);
    recorder.finish(Ending::Retire);
    assert!(
        record_dirs(root.path()).is_empty(),
        "Discard and Quit retire every record"
    );

    // Cancel (or an exit nobody chose) keeps it for the next start.
    let mut kept = new_plate_session(sessions.path());
    let mut recorder = RecoveryRecorder::start(store.clone(), || {});
    recorder.observe(&mut kept);
    wait_written(&recorder);
    drop(recorder);
    let found = recoverable(&store);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].content, kept.current().version().content);
}

#[test]
fn a_discarded_document_is_not_brought_back_by_a_copy_still_in_flight() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let store = RecoveryStore::open(root.path()).expect("store");
    for _ in 0..20 {
        let mut recorder = RecoveryRecorder::start(store.clone(), || {});
        let mut discarded = new_plate_session(sessions.path());
        apply(&mut discarded, 22.0);
        // Its copy is requested and, without waiting, the document is replaced.
        recorder.observe(&mut discarded);
        let mut next = DocumentSession::create_document_in(
            sessions.path(),
            HistoryLimits::default(),
            NewDocument::Empty,
            no_kernel,
            &OperationContext::default(),
        )
        .expect("new");
        recorder.observe(&mut next);
        wait_written(&recorder);
        // Keep the next one, so what remains is exactly what is still wanted.
        drop(recorder);
        let found = recoverable(&store);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].content, next.current().version().content);
        let claim = store.claim(found[0].record).expect("claim");
        claim.discard().expect("cleaned");
    }
    assert!(record_dirs(root.path()).is_empty());
}

#[test]
fn extraction_publishes_a_new_file_and_keeps_the_record_and_the_source() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let store = RecoveryStore::open(root.path()).expect("store");
    let named = user.path().join("plate.fcad");
    make_plate(&named);
    let mut session =
        DocumentSession::open_in(sessions.path(), &named, HistoryLimits::default()).expect("open");
    apply(&mut session, 22.0);
    let accepted = version_of(session.current().path());
    let mut record = store.create_record().expect("record");
    record
        .publish(1, &session.current(), &session.recovery_name())
        .expect("copy");
    drop(record);

    // The source is changed outside FerriteCAD after the crash.
    std::fs::remove_file(&named).expect("remove");
    create_document(
        CreateDocumentRequest::new(&named, NewDocument::Empty, "keep"),
        &OperationContext::default(),
    )
    .expect("someone else's document");
    let source_bytes = bytes(&named);

    let found = recoverable(&store);
    let claim = store.claim(found[0].record).expect("claim");
    // Occupied, inside the recovery folder: refused, nothing written.
    let refused = claim.extract_to(&named).expect_err("occupied");
    assert!(refused.to_string().contains("already exists"), "{refused}");
    let inside = root.path().join("out.fcad");
    let refused = claim.extract_to(&inside).expect_err("inside");
    assert!(refused.to_string().contains("recovery folder"), "{refused}");
    assert!(!inside.exists());
    let output = user.path().join("extracted.fcad");
    let extracted = claim.extract_to(&output).expect("extracted");
    assert_eq!(extracted.content, accepted.0.content);
    assert_eq!(version_of(&output), accepted);
    assert_eq!(bytes(&named), source_bytes, "the source is never written");
    drop(claim);
    assert_eq!(recoverable(&store).len(), 1, "extraction keeps the record");
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write");
}

#[test]
fn unknown_damaged_mismatched_and_foreign_records_are_refused_alone() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let store = RecoveryStore::open(root.path()).expect("store");
    let session = new_plate_session(sessions.path());
    let make = || {
        let mut record = store.create_record().expect("record");
        record
            .publish(1, &session.current(), "plate.fcad")
            .expect("copy");
        let id = record.id();
        drop(record);
        (id, root.path().join(format!("r-{id}")))
    };
    let (good, _) = make();
    let (newer, dir) = make();
    let manifest = std::fs::read_to_string(dir.join("manifest")).expect("manifest");
    write(
        &dir.join("manifest"),
        &manifest.replace("FERRITECAD-RECOVERY 1", "FERRITECAD-RECOVERY 2"),
    );
    let (truncated, dir) = make();
    let copy = std::fs::read(dir.join("c1.fcad")).expect("copy");
    std::fs::write(dir.join("c1.fcad"), &copy[..copy.len() / 2]).expect("truncate");
    let (flipped, dir) = make();
    let mut copy = std::fs::read(dir.join("c1.fcad")).expect("copy");
    let last = copy.len() - 1;
    copy[last] ^= 0x55;
    std::fs::write(dir.join("c1.fcad"), &copy).expect("flip");
    let (elsewhere, dir) = make();
    let manifest = std::fs::read_to_string(dir.join("manifest")).expect("manifest");
    write(
        &dir.join("manifest"),
        &manifest.replace("file c1.fcad", "file ../escape.fcad"),
    );
    let (missing, dir) = make();
    std::fs::remove_file(dir.join("c1.fcad")).expect("remove");
    // Someone else's things in the folder: never read as records, never touched.
    std::fs::create_dir(root.path().join("notes")).expect("foreign dir");
    write(&root.path().join("r-not-a-uuid"), "foreign file");
    let foreign = root.path().join(format!("r-{}", ObjectId::new()));
    std::fs::create_dir(&foreign).expect("foreign record-like dir");
    write(&foreign.join("lease"), "not a FerriteCAD lease");
    write(&foreign.join("manifest"), "FERRITECAD-RECOVERY 1\n");

    let listing = store.list().expect("list");
    let kinds: std::collections::BTreeMap<String, &str> = listing
        .entries
        .iter()
        .map(|entry| match entry {
            RecoveryEntry::Recoverable(summary) => (summary.record.to_string(), "recoverable"),
            RecoveryEntry::Refused(refusal) => (refusal.record.to_string(), refusal.kind.as_str()),
        })
        .collect();
    let expect = |id: RecordId, kind: &str| {
        assert_eq!(kinds.get(&id.to_string()).copied(), Some(kind), "{kinds:?}");
    };
    expect(good, "recoverable");
    expect(newer, "unknown-version");
    expect(truncated, "mismatch");
    expect(flipped, "mismatch");
    expect(elsewhere, "damaged");
    expect(missing, "damaged");
    assert_eq!(kinds.len(), 7, "{kinds:?}");
    assert_eq!(
        store.claim(newer).map(|_| ()).expect_err("refused").kind,
        RefusalKind::UnknownVersion
    );
    // The good one is untouched by its neighbours.
    let claim = store.claim(good).expect("good");
    let restored = sessions.path().join("restored.fcad");
    claim.restore_to(&restored).expect("restored");
    assert_eq!(version_of(&restored).0, session.current().version());
    drop(claim);
    // A damaged record of ours can be deleted on request; foreign things cannot.
    store.delete(truncated).expect("deleted");
    assert!(!root.path().join(format!("r-{truncated}")).exists());
    let foreign_id: RecordId = foreign
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("r-"))
        .expect("name")
        .parse()
        .expect("id");
    assert_eq!(
        store.delete(foreign_id).expect_err("refused").kind,
        RefusalKind::NotFound
    );
    assert_eq!(names(&foreign), ["lease", "manifest"]);
    assert!(root.path().join("notes").is_dir());
    assert!(root.path().join("r-not-a-uuid").is_file());
    assert!(!root.path().join("escape.fcad").exists());
}

#[test]
fn a_write_failure_is_reported_keeps_the_previous_copy_and_is_never_written_status() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let store = RecoveryStore::open(root.path()).expect("store");
    let mut session = new_plate_session(sessions.path());
    let mut record = store.create_record().expect("record");
    record
        .publish(1, &session.current(), "plate")
        .expect("first");
    let first = session.current().version();
    apply(&mut session, 22.0);
    // The name the next copy is written under is taken by something that is not
    // a file: the copy cannot be made, whoever runs the test.
    let directory = root.path().join(format!("r-{}", record.id()));
    std::fs::create_dir(directory.join(".c2.fcad.partial")).expect("blocker");
    let failed = record.publish(2, &session.current(), "plate");
    assert!(failed.is_err());
    std::fs::remove_dir(directory.join(".c2.fcad.partial")).expect("unblock");
    drop(record);
    let found = recoverable(&store);
    assert_eq!(found[0].content, first.content, "the previous copy stands");

    // Through the recorder: the status says it failed, never that it is written,
    // and the model is untouched.
    let full = RecoveryStore::open(root.path())
        .expect("store")
        .with_record_limit(1);
    let mut recorder = RecoveryRecorder::start(full, || {});
    let before = session.current().version();
    recorder.observe(&mut session);
    wait_status(&recorder, |status| {
        matches!(status, RecoveryStatus::Failed(_))
    });
    let RecoveryStatus::Failed(message) = recorder.status() else {
        unreachable!()
    };
    assert!(message.contains("limit"), "{message}");
    assert_eq!(session.current().version(), before);
    drop(recorder);
    assert_eq!(
        recoverable(&store).len(),
        1,
        "nothing was removed for the limit"
    );
}

#[cfg(unix)]
#[test]
fn a_folder_without_write_permission_fails_the_copy_and_loses_nothing() {
    use std::os::unix::fs::PermissionsExt as _;
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let store = RecoveryStore::open(root.path()).expect("store");
    let mut session = new_plate_session(sessions.path());
    let mut record = store.create_record().expect("record");
    record
        .publish(1, &session.current(), "plate")
        .expect("first");
    let first = session.current().version();
    apply(&mut session, 22.0);
    let directory = root.path().join(format!("r-{}", record.id()));
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500)).expect("chmod");
    let probe = directory.join("probe");
    if std::fs::write(&probe, b"x").is_ok() {
        let _ = std::fs::remove_file(&probe);
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .expect("chmod");
        println!("skipped: this process ignores directory permissions (it runs as root)");
        return;
    }
    let failed = record.publish(2, &session.current(), "plate");
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let error = failed.expect_err("a read-only record cannot be written");
    assert!(error.to_string().contains("recovery"), "{error}");
    drop(record);
    let found = recoverable(&store);
    assert_eq!(found[0].content, first.content);
    println!("\nFCAD_30M_PERMISSION_GATE_EXECUTED");
}

#[test]
fn the_limit_never_deletes_recoverable_work_and_empty_orphans_are_swept() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let store = RecoveryStore::open(root.path())
        .expect("store")
        .with_record_limit(2);
    let session = new_plate_session(sessions.path());
    for _ in 0..2 {
        let mut record = store.create_record().expect("record");
        record
            .publish(1, &session.current(), "plate")
            .expect("copy");
    }
    // Both are recoverable orphans now; a third is refused and neither goes.
    drop(store.create_record().expect_err("at the limit"));
    let empty = RecoveryStore::open(root.path()).expect("store");
    assert_eq!(record_dirs(root.path()).len(), 2);
    let listing = empty.list().expect("list");
    assert_eq!(listing.recoverable().count(), 2);

    let unlimited = RecoveryStore::open(root.path()).expect("store");
    let blank = unlimited.create_record().expect("record");
    drop(blank);
    assert_eq!(unlimited.list().expect("list").empty, 1);
    let next = unlimited.create_record().expect("record");
    assert_eq!(
        unlimited.list().expect("list").empty,
        0,
        "the empty orphan was swept"
    );
    assert_eq!(unlimited.list().expect("list").recoverable().count(), 2);
    drop(next);
}

#[test]
fn many_quick_changes_end_with_the_newest_accepted_version_written() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let store = RecoveryStore::open(root.path()).expect("store");
    let mut session = new_plate_session(sessions.path());
    let mut recorder = RecoveryRecorder::start(store.clone(), || {});
    for height in [15.0, 16.0, 17.0, 18.0, 19.0, 20.0] {
        apply(&mut session, height);
        recorder.observe(&mut session);
    }
    undo(&mut session);
    recorder.observe(&mut session);
    let written = wait_written(&recorder);
    drop(recorder);
    let found = recoverable(&store);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].content, session.current().version().content);
    assert_eq!(found[0].written_unix_ms, written);
}

#[test]
fn a_recovered_session_adopts_its_record_without_writing_or_duplicating_it() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let store = RecoveryStore::open(root.path()).expect("store");
    let first = new_plate_session(sessions.path());
    let mut record = store.create_record().expect("record");
    record
        .publish(1, &first.current(), "plate.fcad")
        .expect("copy");
    let id = record.id();
    drop(record);
    let directory = root.path().join(format!("r-{id}"));
    let copy_bytes = bytes(&directory.join("c1.fcad"));

    let claim = store.claim(id).expect("claim");
    let written_before = claim.summary().written_unix_ms;
    let mut recovered =
        DocumentSession::recover_in(sessions.path(), HistoryLimits::default(), claim)
            .expect("recovered");
    let mut recorder = RecoveryRecorder::start(store.clone(), || {});
    recorder.observe(&mut recovered);
    let written = wait_written(&recorder);
    assert_eq!(written, written_before, "already written: nothing new");
    assert_eq!(record_dirs(root.path()).len(), 1, "no duplicate");
    assert_eq!(bytes(&directory.join("c1.fcad")), copy_bytes);
    // An edit of the recovered model is copied into the same record.
    apply(&mut recovered, 22.0);
    recorder.observe(&mut recovered);
    wait_written(&recorder);
    assert_eq!(record_dirs(root.path()), [format!("r-{id}")]);
    assert_eq!(names(&directory), ["c2.fcad", "lease", "manifest"]);
    recorder.finish(Ending::Retire);
    assert!(record_dirs(root.path()).is_empty());
}
