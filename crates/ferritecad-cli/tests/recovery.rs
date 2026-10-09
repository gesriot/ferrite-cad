// SPDX-License-Identifier: MIT
//! §30M: `list-recovery` and `extract-recovery`, through the library claim the
//! window recovers from. No kernel: the copies are made by the library here.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use ferritecad_document::Document;
use ferritecad_jobs::{
    CreateDocumentRequest, DocumentSession, HistoryLimits, NewDocument, PlateSize, RecordId,
    RecoveryStore, create_document, read_extrude_source,
};
use ferritecad_kernel::{OperationContext, mock::MockKernel};
use ferritecad_types::{CadError, ContentHash, DocumentId, Result};
use serde_json::Value;

#[path = "support/pipe.rs"]
mod pipe;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferritecad"));
    command.env_remove(ferritecad_jobs::RECOVERY_DIR_ENV);
    command
}

fn no_kernel() -> Result<MockKernel> {
    Err(CadError::unsupported("no kernel was expected here"))
}

/// An orphaned record holding a new sample plate, and its document.
fn orphan(root: &Path, sessions: &Path) -> (RecordId, DocumentSession) {
    let session = DocumentSession::create_document_in(
        sessions,
        HistoryLimits::default(),
        NewDocument::SamplePlate(PlateSize {
            width: 70.0,
            depth: 40.0,
            height: 13.0,
        }),
        no_kernel,
        &OperationContext::default(),
    )
    .expect("new");
    let store = RecoveryStore::open(root).expect("store");
    let mut record = store.create_record().expect("record");
    record
        .publish(1, &session.current(), "plate.fcad")
        .expect("copy");
    let id = record.id();
    drop(record);
    (id, session)
}

fn json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("one JSON document")
}

fn content_of(path: &Path) -> String {
    let document = Document::open_read_only(path).expect("document");
    let content = document.content_version().expect("content").to_string();
    document.close().expect("close");
    content
}

#[test]
fn list_and_extract_share_the_claim_keep_the_record_and_refuse_occupied_outputs() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let (id, session) = orphan(root.path(), sessions.path());
    let expected = session.current().version();

    let listed = cli()
        .args(["list-recovery", "--json", "--recovery-dir"])
        .arg(root.path())
        .output()
        .expect("list");
    assert_eq!(listed.status.code(), Some(0));
    let value = json(&listed);
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["operation"], "list-recovery");
    assert_eq!(value["ok"], true);
    let records = value["result"]["records"].as_array().expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["state"], "recoverable");
    assert_eq!(records[0]["record"], id.to_string());
    assert_eq!(records[0]["name"], "plate.fcad");
    assert_eq!(records[0]["document_id"], expected.document_id.to_string());
    assert_eq!(records[0]["content_version"], expected.content.to_string());
    assert_eq!(value["result"]["active"], 0);

    let output = user.path().join("recovered.fcad");
    let extracted = cli()
        .args(["extract-recovery", &id.to_string(), "--json", "--output"])
        .arg(&output)
        .arg("--recovery-dir")
        .arg(root.path())
        .output()
        .expect("extract");
    assert_eq!(extracted.status.code(), Some(0), "{extracted:?}");
    let value = json(&extracted);
    assert_eq!(value["operation"], "extract-recovery");
    assert_eq!(value["result"]["record"], id.to_string());
    assert_eq!(
        value["result"]["content_version"],
        expected.content.to_string()
    );
    assert_eq!(content_of(&output), expected.content.to_string());

    // Occupied: a structured refusal, the file as it was.
    let before = std::fs::read(&output).expect("bytes");
    let refused = cli()
        .args(["extract-recovery", &id.to_string(), "--json", "--output"])
        .arg(&output)
        .arg("--recovery-dir")
        .arg(root.path())
        .output()
        .expect("extract");
    assert_eq!(refused.status.code(), Some(2));
    let value = json(&refused);
    assert_eq!(value["ok"], false);
    assert!(
        value["error"]["message"]
            .as_str()
            .expect("message")
            .contains("already exists")
    );
    assert!(value["error"].get("recovery_refusal").is_none());
    assert_eq!(std::fs::read(&output).expect("bytes"), before);

    // Inside the recovery folder: refused, nothing written there.
    let inside = root.path().join("inside.fcad");
    let refused = cli()
        .args(["extract-recovery", &id.to_string(), "--json", "--output"])
        .arg(&inside)
        .arg("--recovery-dir")
        .arg(root.path())
        .output()
        .expect("extract");
    assert_eq!(refused.status.code(), Some(2));
    assert!(!inside.exists());

    // The record is still there: extraction never takes it over.
    let store = RecoveryStore::at(root.path()).expect("store");
    assert_eq!(store.list().expect("list").recoverable().count(), 1);

    // Text mode reads the same listing.
    let text = cli()
        .args(["list-recovery", "--recovery-dir"])
        .arg(root.path())
        .output()
        .expect("list");
    assert_eq!(text.status.code(), Some(0));
    let text = String::from_utf8(text.stdout).expect("text");
    assert!(text.contains(&format!("{id}  recoverable")), "{text}");
    assert!(text.contains("plate.fcad"), "{text}");
}

#[test]
fn active_damaged_and_unknown_records_are_refused_as_data() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let (held, _) = orphan(root.path(), sessions.path());
    let (damaged, _) = orphan(root.path(), sessions.path());
    let copy = root.path().join(format!("r-{damaged}")).join("c1.fcad");
    let mut bytes = std::fs::read(&copy).expect("copy");
    let last = bytes.len() - 1;
    bytes[last] ^= 0x55;
    std::fs::write(&copy, bytes).expect("flip");

    let store = RecoveryStore::at(root.path()).expect("store");
    let claim = store.claim(held).expect("held by this process");
    let refusal = |id: String| {
        let output = cli()
            .args(["extract-recovery", &id, "--json", "--output"])
            .arg(user.path().join(format!("{id}.fcad")))
            .arg("--recovery-dir")
            .arg(root.path())
            .output()
            .expect("extract");
        assert_eq!(output.status.code(), Some(2));
        let value = json(&output);
        assert_eq!(value["ok"], false);
        value["error"]["recovery_refusal"]
            .as_str()
            .map(str::to_owned)
    };
    assert_eq!(refusal(held.to_string()).as_deref(), Some("active"));
    assert_eq!(refusal(damaged.to_string()).as_deref(), Some("mismatch"));
    assert_eq!(
        refusal("01a11bec-7115-7200-89eb-ad517d927abd".to_owned()).as_deref(),
        Some("not-found")
    );
    assert_eq!(std::fs::read_dir(user.path()).expect("user").count(), 0);

    let listed = cli()
        .args(["list-recovery", "--json", "--recovery-dir"])
        .arg(root.path())
        .output()
        .expect("list");
    let value = json(&listed);
    assert_eq!(value["result"]["active"], 1);
    let records = value["result"]["records"].as_array().expect("records");
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["state"], "refused");
    assert_eq!(records[0]["reason"], "mismatch");
    drop(claim);
}

#[test]
fn a_missing_folder_lists_nothing_and_is_not_created_and_the_variable_names_the_folder() {
    let parent = tempfile::tempdir().expect("parent");
    let missing = parent.path().join("never-made");
    let listed = cli()
        .args(["list-recovery", "--json", "--recovery-dir"])
        .arg(&missing)
        .output()
        .expect("list");
    assert_eq!(listed.status.code(), Some(0));
    assert_eq!(json(&listed)["result"]["records"], serde_json::json!([]));
    assert!(!missing.exists(), "listing created the folder");

    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let (id, _) = orphan(root.path(), sessions.path());
    let listed = cli()
        .env(ferritecad_jobs::RECOVERY_DIR_ENV, root.path())
        .args(["list-recovery", "--json"])
        .output()
        .expect("list");
    let value = json(&listed);
    assert_eq!(value["result"]["records"][0]["record"], id.to_string());
}

#[test]
fn usage_stays_text_and_a_lost_report_after_extraction_exits_7_and_keeps_the_file() {
    let usage = cli()
        .args(["extract-recovery", "--json"])
        .output()
        .expect("usage");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty(), "usage stays clap text");
    let usage = cli()
        .args([
            "extract-recovery",
            "not-a-uuid",
            "--json",
            "--output",
            "x.fcad",
        ])
        .output()
        .expect("usage");
    assert_eq!(usage.status.code(), Some(2));
    assert!(
        usage.stdout.is_empty(),
        "a malformed record is a usage error"
    );

    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let (id, session) = orphan(root.path(), sessions.path());
    let output = user.path().join("delivered-or-not.fcad");
    let status = cli()
        .args(["extract-recovery", &id.to_string(), "--json", "--output"])
        .arg(&output)
        .arg("--recovery-dir")
        .arg(root.path())
        .stdout(pipe::closed_pipe())
        .stderr(pipe::closed_pipe())
        .status()
        .expect("pipes");
    assert_eq!(status.code(), Some(7));
    assert_eq!(
        content_of(&output),
        session.current().version().content.to_string(),
        "the extraction stands"
    );
    let store = RecoveryStore::at(root.path()).expect("store");
    assert_eq!(store.list().expect("list").recoverable().count(), 1);
}

// --- §30S: reading is not owning -------------------------------------------------------

/// What an orphaned copy of a real, edited document must come back as.
struct Dirty {
    record: RecordId,
    source: PathBuf,
    source_bytes: Vec<u8>,
    document: DocumentId,
    content: ContentHash,
    model: ContentHash,
}

/// A real source file, opened, edited (its extrusion height) and accepted; the
/// copy published and its owner gone, as a crash leaves it. The source is on disk
/// exactly as it was made.
fn dirty_copy_of_a_source(root: &Path, user: &Path, sessions: &Path) -> Dirty {
    let source = user.join("plate.fcad");
    create_document(
        CreateDocumentRequest::new(
            &source,
            NewDocument::SamplePlate(PlateSize {
                width: 70.0,
                depth: 40.0,
                height: 13.0,
            }),
            "keep",
        ),
        &OperationContext::default(),
    )
    .expect("a source");
    let source_bytes = std::fs::read(&source).expect("source bytes");
    let mut session =
        DocumentSession::open_in(sessions, &source, HistoryLimits::default()).expect("opened");
    let feature = read_extrude_source(session.current().path())
        .expect("reading")
        .features[0]
        .feature;
    let step = session
        .begin_step()
        .edit_extrude_height(
            feature,
            22.0,
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("an edit");
    session.commit_step(step).expect("accepted");
    let store = RecoveryStore::open(root).expect("store");
    let mut record = store.create_record().expect("record");
    record
        .publish(1, &session.current(), "plate.fcad")
        .expect("copy");
    let id = record.id();
    drop(record);
    let version = session.current().version();
    Dirty {
        record: id,
        source,
        source_bytes,
        document: version.document_id,
        content: version.content,
        model: session.current().model(),
    }
}

/// The record's files, by name: bytes and modification time.
fn record_files(root: &Path, record: RecordId) -> Vec<(String, Vec<u8>, std::time::SystemTime)> {
    let mut files: Vec<_> = std::fs::read_dir(root.join(format!("r-{record}")))
        .expect("the record")
        .map(|entry| {
            let entry = entry.expect("entry");
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path()).expect("bytes"),
                entry.metadata().and_then(|m| m.modified()).expect("mtime"),
            )
        })
        .collect();
    files.sort();
    files
}

/// Listing processes that are killed and reaped if a failing assertion leaves them.
struct Listings(Vec<std::process::Child>);

impl Drop for Listings {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Every identity of the extracted file is the recorded one: its document id, its
/// content version (every SQL cell, row id and reference) and its model version.
fn assert_recorded(path: &Path, dirty: &Dirty) {
    let document = Document::open_read_only(path).expect("the extracted document");
    assert_eq!(document.meta().document_id, dirty.document);
    assert_eq!(document.content_version().expect("content"), dirty.content);
    assert_eq!(document.model_version().expect("model"), dirty.model);
    document.close().expect("close");
}

/// A reader that is not this slice's code: any process that holds the lease shared,
/// as a listing does while it verifies.
fn foreign_reader(root: &Path, record: RecordId) -> std::fs::File {
    let file = std::fs::File::open(root.join(format!("r-{record}")).join("lease")).expect("lease");
    file.try_lock_shared().expect("nobody holds it exclusively");
    file
}

fn extract(root: &Path, dirty: &Dirty, output: &Path) -> std::process::Output {
    cli()
        .args([
            "extract-recovery",
            &dirty.record.to_string(),
            "--json",
            "--output",
        ])
        .arg(output)
        .arg("--recovery-dir")
        .arg(root)
        .output()
        .expect("extract")
}

#[test]
fn an_extraction_waits_for_a_reader_and_says_busy_only_when_the_reader_stays() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let dirty = dirty_copy_of_a_source(root.path(), user.path(), sessions.path());
    let before = record_files(root.path(), dirty.record);

    // A reader lets go after a moment: the extraction is delayed, not refused, and
    // takes the record with every identity.
    let reader = foreign_reader(root.path(), dirty.record);
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(reader);
    });
    let output = user.path().join("waited.fcad");
    let waited = extract(root.path(), &dirty, &output);
    assert_eq!(waited.status.code(), Some(0), "{waited:?}");
    assert_eq!(
        json(&waited)["result"]["content_version"],
        dirty.content.to_string()
    );
    assert_recorded(&output, &dirty);
    releaser.join().expect("releaser");

    // A reader that stays past the bound: `busy`, as data, with the exit code of every
    // refusal; no file; and the record exactly as it was.
    let reader = foreign_reader(root.path(), dirty.record);
    let stuck = user.path().join("never.fcad");
    let busy = extract(root.path(), &dirty, &stuck);
    assert_eq!(busy.status.code(), Some(2), "{busy:?}");
    let value = json(&busy);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["recovery_refusal"], "busy");
    assert!(!stuck.exists());
    drop(reader);

    assert_eq!(
        record_files(root.path(), dirty.record),
        before,
        "the record changed"
    );
    assert_eq!(
        std::fs::read(&dirty.source).expect("source"),
        dirty.source_bytes
    );
    // And nobody holds it any more.
    let again = user.path().join("again.fcad");
    assert_eq!(extract(root.path(), &dirty, &again).status.code(), Some(0));
    assert_recorded(&again, &dirty);
}

/// Real processes, overlapping as they happen to: listings, in numbers, around
/// every extraction. A listing that overlaps the extraction itself may find the
/// record held and say so; it may never say anything else, and no extraction may be
/// refused for a listing's sake.
#[test]
fn listings_in_other_processes_never_stop_an_extraction_or_change_anything() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let dirty = dirty_copy_of_a_source(root.path(), user.path(), sessions.path());
    let before = record_files(root.path(), dirty.record);

    for round in 0..6 {
        let mut listings = Listings(
            (0..8)
                .map(|_| {
                    cli()
                        .args(["list-recovery", "--json", "--recovery-dir"])
                        .arg(root.path())
                        .stdout(Stdio::piped())
                        .spawn()
                        .expect("a listing process")
                })
                .collect(),
        );
        let output = user.path().join(format!("extracted-{round}.fcad"));
        let extracted = extract(root.path(), &dirty, &output);
        assert_eq!(
            extracted.status.code(),
            Some(0),
            "round {round}: {extracted:?}"
        );
        assert_recorded(&output, &dirty);
        for listing in std::mem::take(&mut listings.0) {
            let listing = listing.wait_with_output().expect("listing result");
            assert_eq!(listing.status.code(), Some(0), "{listing:?}");
            let value = json(&listing);
            let records = value["result"]["records"].as_array().expect("records");
            let shape = (records.len(), value["result"]["active"].as_u64());
            // Either it overlapped the extraction itself and found the record held,
            // or it found the one recoverable record whole.
            assert!(
                matches!(shape, (0, Some(1)) | (1, Some(0))),
                "{shape:?}: {value}"
            );
            if let Some(record) = records.first() {
                assert_eq!(record["state"], "recoverable", "{value}");
                assert_eq!(record["record"], dirty.record.to_string());
                assert_eq!(record["document_id"], dirty.document.to_string());
                assert_eq!(record["content_version"], dirty.content.to_string());
            }
        }
    }
    assert_eq!(
        record_files(root.path(), dirty.record),
        before,
        "the record changed"
    );
    assert_eq!(
        std::fs::read(&dirty.source).expect("source"),
        dirty.source_bytes
    );
}
