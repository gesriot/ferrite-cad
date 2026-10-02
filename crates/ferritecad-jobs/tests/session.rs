// SPDX-License-Identifier: MIT
//! The document session's state machine, with real documents and no window.
//!
//! No Open CASCADE: the copy operation under test is the real `edit-extrude`
//! (`StepTicket::edit_extrude_height`) run on the mock kernel, which is enough
//! for a plate and exercises the same prepare, cold rebuild, strict reference
//! check, version re-check and publication the command line uses. Geometry is
//! measured by the native tests; what is asserted here is state and files.

use std::path::{Path, PathBuf};

use ferritecad_jobs::{
    CreateDocumentRequest, DocumentSession, HistoryLimits, NewDocument, PlateSize, STALE_STEP,
    StepCommit, create_document, read_extrude_source,
};
use ferritecad_kernel::{OperationContext, mock::MockKernel};
use ferritecad_types::{CadError, ObjectId};

struct Fixture {
    root: tempfile::TempDir,
    file: PathBuf,
    feature: ObjectId,
    height: f64,
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("directory");
    let file = root.path().join("plate.fcad");
    create_document(
        CreateDocumentRequest::new(
            &file,
            NewDocument::SamplePlate(PlateSize {
                width: 83.0,
                depth: 47.0,
                height: 13.0,
            }),
            "keep",
        ),
        &OperationContext::default(),
    )
    .expect("a plate");
    let reading = read_extrude_source(&file).expect("reading");
    Fixture {
        feature: reading.features[0].feature,
        height: reading.features[0].distance_mm.expect("a height"),
        file,
        root,
    }
}

fn private_root() -> tempfile::TempDir {
    tempfile::tempdir().expect("a private root for sessions")
}

fn open(fixture: &Fixture, root: &Path, limits: HistoryLimits) -> DocumentSession {
    DocumentSession::open_in(root, &fixture.file, limits).expect("session")
}

fn height_of(path: &Path) -> f64 {
    read_extrude_source(path).expect("reading").features[0]
        .distance_mm
        .expect("a height")
}

fn apply(
    session: &mut DocumentSession,
    feature: ObjectId,
    millimetres: f64,
) -> Result<StepCommit, CadError> {
    let ticket = session.begin_step();
    let step = ticket.edit_extrude_height(
        feature,
        millimetres,
        &mut MockKernel::new(),
        &OperationContext::default(),
    )?;
    session.commit_step(step)
}

fn files(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .expect("directory")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("bytes")
}

#[test]
fn open_apply_undo_redo_save_is_one_state_machine() {
    let f = fixture();
    let sessions = private_root();
    let original = bytes(&f.file);
    let mut session = open(&f, sessions.path(), HistoryLimits::default());

    // Opened: clean, nothing to undo, named for the user's file.
    assert!(!session.is_dirty() && !session.can_undo() && !session.can_redo());
    assert_eq!(session.display_name(), "plate.fcad");
    assert_eq!(height_of(session.current().path()), f.height);

    // Apply: the accepted version moved, the user's file did not.
    assert_eq!(
        apply(&mut session, f.feature, 21.5).expect("apply"),
        StepCommit::Accepted
    );
    assert!(session.is_dirty() && session.can_undo() && !session.can_redo());
    assert_eq!(height_of(session.current().path()), 21.5);
    assert_eq!(bytes(&f.file), original, "Apply wrote the user's file");

    // Undo back to the checkpoint is clean, and Redo is dirty again.
    let undo = session.begin_undo().expect("one step back");
    assert_eq!(height_of(undo.path()), f.height);
    session.commit_move(undo).expect("undo");
    assert!(!session.is_dirty(), "the saved checkpoint is not dirty");
    assert!(!session.can_undo() && session.can_redo());
    let redo = session.begin_redo().expect("one step forward");
    session.commit_move(redo).expect("redo");
    assert!(session.is_dirty() && session.can_undo() && !session.can_redo());
    assert_eq!(
        bytes(&f.file),
        original,
        "Undo and Redo wrote the user's file"
    );

    // Save: the file now holds the accepted version, byte-for-model.
    let saved = session
        .begin_save(ferritecad_jobs::SaveTarget::InPlace)
        .run(&OperationContext::default())
        .expect("saved");
    session.record_saved(&saved);
    assert!(!session.is_dirty());
    assert_eq!(height_of(&f.file), 21.5);
    assert_eq!(session.logical_path(), f.file);

    // The saved checkpoint moved with it: Undo is now dirty, Redo clean again.
    let undo = session.begin_undo().expect("back");
    session.commit_move(undo).expect("undo");
    assert!(session.is_dirty());
    let redo = session.begin_redo().expect("forward");
    session.commit_move(redo).expect("redo");
    assert!(!session.is_dirty());
}

#[test]
fn editing_back_to_the_saved_value_is_clean_and_a_no_op_is_not_a_step() {
    let f = fixture();
    let sessions = private_root();
    let mut session = open(&f, sessions.path(), HistoryLimits::default());

    // A different value, then the original value again: the stamp differs from
    // the checkpoint's, the model does not.
    apply(&mut session, f.feature, 30.0).expect("apply");
    assert!(session.is_dirty());
    apply(&mut session, f.feature, f.height).expect("apply back");
    assert!(
        !session.is_dirty(),
        "editing back to the saved value left a false dirty mark"
    );
    assert_eq!(session.undo_depth(), 2, "both edits are accepted steps");

    // The same value applied again rewrites only the stamp: no step.
    let before = (session.history_len(), session.generation());
    let ticket = session.begin_step();
    let step = ticket
        .edit_extrude_height(
            f.feature,
            f.height,
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("a no-op still produces a file");
    assert!(!session.changes_model(&step), "the model is the same");
    assert_eq!(
        session.commit_step(step).expect("commit"),
        StepCommit::NoChange
    );
    assert_eq!(
        (session.history_len(), session.generation()),
        before,
        "a no-op became a history entry"
    );
    assert!(!session.is_dirty());
}

#[test]
fn a_new_step_after_undo_drops_redo_only_after_it_succeeded() {
    let f = fixture();
    let sessions = private_root();
    let mut session = open(&f, sessions.path(), HistoryLimits::default());
    apply(&mut session, f.feature, 20.0).expect("first");
    apply(&mut session, f.feature, 25.0).expect("second");
    let undo = session.begin_undo().expect("back");
    session.commit_move(undo).expect("undo");
    assert!(session.can_redo());

    // A failed edit (the feature is not there) must not cost the redo history.
    let missing = ObjectId::new();
    assert!(apply(&mut session, missing, 40.0).is_err());
    assert!(
        session.can_redo(),
        "a failed Apply discarded the redo history"
    );
    assert_eq!(height_of(session.current().path()), 20.0);

    // A successful one does.
    apply(&mut session, f.feature, 40.0).expect("branch");
    assert!(!session.can_redo(), "the branch kept its redo history");
    assert_eq!(session.undo_depth(), 2);
    assert_eq!(height_of(session.current().path()), 40.0);
}

#[test]
fn history_is_bounded_oldest_first_and_removes_its_files() {
    let f = fixture();
    let sessions = private_root();
    let limits = HistoryLimits {
        max_versions: 3,
        max_bytes: u64::MAX,
    };
    let mut session = open(&f, sessions.path(), limits);
    for height in [20.0, 21.0, 22.0, 23.0, 24.0] {
        apply(&mut session, f.feature, height).expect("apply");
    }
    assert_eq!(session.history_len(), 3);
    assert_eq!(session.undo_depth(), 2, "the current version is the newest");
    assert_eq!(height_of(session.current().path()), 24.0);
    // Only what the history holds is on disk.
    let names = files(session.private_directory());
    assert_eq!(names.len(), 3, "{names:?}");

    // The saved checkpoint (the opened file) fell out of the history; dirty is
    // still right because it compares models, not entries.
    assert!(session.is_dirty());
    let mut back = 0;
    while let Some(undo) = session.begin_undo() {
        session.commit_move(undo).expect("undo");
        back += 1;
    }
    assert_eq!(back, 2, "Undo stops at the oldest kept version");
    assert_eq!(height_of(session.current().path()), 22.0);
    assert!(
        session.is_dirty(),
        "the oldest kept version is not the saved one"
    );

    // A byte bound keeps at least the current version.
    let mut tight = open(
        &f,
        sessions.path(),
        HistoryLimits {
            max_versions: 64,
            max_bytes: 1,
        },
    );
    apply(&mut tight, f.feature, 20.0).expect("apply");
    apply(&mut tight, f.feature, 21.0).expect("apply");
    assert_eq!(tight.history_len(), 1, "only the current version fits");
    assert!(!tight.can_undo());
    assert_eq!(height_of(tight.current().path()), 21.0);
}

#[test]
fn a_failed_stale_or_dropped_step_changes_nothing_and_leaves_no_file() {
    let f = fixture();
    let sessions = private_root();
    let mut session = open(&f, sessions.path(), HistoryLimits::default());
    let before_files = files(session.private_directory());
    let generation = session.generation();

    // A step that fails to produce leaves nothing.
    assert!(apply(&mut session, ObjectId::new(), 20.0).is_err());
    assert_eq!(files(session.private_directory()), before_files);
    assert_eq!(session.generation(), generation);
    assert!(!session.is_dirty() && !session.can_undo());

    // A step produced and then dropped (its scene could not be prepared) too.
    let ticket = session.begin_step();
    let step = ticket
        .edit_extrude_height(
            f.feature,
            22.0,
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("produced");
    assert_eq!(
        files(session.private_directory()).len(),
        before_files.len() + 1
    );
    drop(step);
    assert_eq!(files(session.private_directory()), before_files);
    assert!(!session.is_dirty());

    // A step made before the session moved on is stale and is not applied.
    let stale = session
        .begin_step()
        .edit_extrude_height(
            f.feature,
            23.0,
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("produced");
    apply(&mut session, f.feature, 24.0).expect("the session moves on");
    let error = session.commit_step(stale).expect_err("stale");
    assert_eq!(error.to_string(), format!("invalid input: {STALE_STEP}"));
    assert_eq!(height_of(session.current().path()), 24.0);
    assert_eq!(session.undo_depth(), 1);
    assert_eq!(
        files(session.private_directory()).len(),
        2,
        "v0 and the accepted step"
    );

    // So is an Undo asked for before the session moved on.
    let undo = session.begin_undo().expect("back");
    apply_after_undo_request(&mut session, f.feature);
    assert!(session.commit_move(undo).is_err());
}

fn apply_after_undo_request(session: &mut DocumentSession, feature: ObjectId) {
    apply(session, feature, 26.0).expect("apply");
}

#[test]
fn the_session_removes_everything_it_made_and_a_holder_keeps_its_version() {
    let f = fixture();
    let sessions = private_root();
    let private;
    let held;
    {
        let mut session = open(&f, sessions.path(), HistoryLimits::default());
        apply(&mut session, f.feature, 20.0).expect("apply");
        private = session.private_directory().to_path_buf();
        assert!(private.starts_with(sessions.path()));
        assert!(
            !private.to_string_lossy().contains("plate"),
            "the private directory does not carry the user's name"
        );
        held = session.current();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&private)
                .expect("stat")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700, "the session directory is private");
        }
    }
    // The session is gone; a reader (an export in flight) still has its version.
    assert_eq!(height_of(held.path()), 20.0);
    drop(held);
    assert!(
        !private.exists(),
        "the private directory outlived its last version"
    );
    assert_eq!(files(sessions.path()), Vec::<String>::new());
    // And the user's directory holds exactly the user's file.
    assert_eq!(files(f.root.path()), vec!["plate.fcad".to_owned()]);
}

#[test]
fn opening_a_document_that_cannot_be_read_creates_no_session_directory() {
    let sessions = private_root();
    let root = tempfile::tempdir().expect("directory");
    let not_a_document = root.path().join("not-a-document.fcad");
    std::fs::write(&not_a_document, b"this is not SQLite").expect("write");
    assert!(
        DocumentSession::open_in(sessions.path(), &not_a_document, HistoryLimits::default())
            .is_err()
    );
    assert!(
        DocumentSession::open_in(
            sessions.path(),
            &root.path().join("absent.fcad"),
            HistoryLimits::default()
        )
        .is_err()
    );
    assert_eq!(files(sessions.path()), Vec::<String>::new());
}
