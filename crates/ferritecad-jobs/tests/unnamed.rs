// SPDX-License-Identifier: MIT
//! §30L: a new document is a session before it is a file. No path, no checkpoint,
//! unsaved until its first Save As is published; files and state only, no window.

use std::path::{Path, PathBuf};

use ferritecad_document::{Document, DocumentVersion};
use ferritecad_jobs::{
    Conflict, CreateDocumentRequest, DocumentSession, HistoryLimits, NewDocument, PlateSize,
    SaveFailureKind, SaveHooks, SaveKind, SaveTarget, UNTITLED, create_document,
    read_extrude_source,
};
use ferritecad_kernel::{CancelToken, OperationContext, mock::MockKernel};
use ferritecad_types::{CadError, ErrorKind, ObjectId, Result};

fn names(directory: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(directory)
        .expect("directory")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

/// A kernel factory that must not be called: Empty and the sample plate are
/// stored models with nothing to build.
fn no_kernel() -> Result<MockKernel> {
    Err(CadError::unsupported("no kernel was expected here"))
}

fn plate() -> NewDocument {
    NewDocument::SamplePlate(PlateSize {
        width: 70.0,
        depth: 40.0,
        height: 13.0,
    })
}

fn create(root: &Path, content: NewDocument) -> DocumentSession {
    DocumentSession::create_document_in(
        root,
        HistoryLimits::default(),
        content,
        no_kernel,
        &OperationContext::default(),
    )
    .expect("a new document")
}

fn version_of(path: &Path) -> DocumentVersion {
    let document = Document::open_read_only(path).expect("document");
    let version = DocumentVersion {
        document_id: document.meta().document_id,
        content: document.content_version().expect("content"),
    };
    document.close().expect("close");
    version
}

fn feature_of(session: &DocumentSession) -> ObjectId {
    read_extrude_source(session.current().path())
        .expect("reading")
        .features[0]
        .feature
}

fn apply(session: &mut DocumentSession, feature: ObjectId, millimetres: f64) {
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

fn save_as(
    session: &DocumentSession,
    path: &Path,
) -> std::result::Result<ferritecad_jobs::Saved, ferritecad_jobs::SaveFailure> {
    session
        .begin_save(SaveTarget::As(path.to_path_buf()))
        .run(&OperationContext::default())
}

/// What a session is, as far as Save and the window are concerned.
fn state(session: &DocumentSession) -> (Option<PathBuf>, Option<DocumentVersion>, bool, usize) {
    (
        session.logical_path().map(Path::to_path_buf),
        session.saved_version(),
        session.is_dirty(),
        session.history_len(),
    )
}

#[test]
fn a_new_empty_document_has_no_file_no_checkpoint_and_is_unsaved_until_published() {
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let session = create(sessions.path(), NewDocument::Empty);

    // Absent, not stood in for: no path, no checkpoint, the name a person reads.
    assert_eq!(session.logical_path(), None);
    assert!(session.is_untitled());
    assert_eq!(session.saved_version(), None);
    assert_eq!(session.display_name(), UNTITLED);
    assert!(session.is_dirty(), "an empty new document is still unsaved");
    assert!(!session.can_undo() && !session.can_redo());
    assert!(session.owns(session.current().path()));
    assert!(
        names(user.path()).is_empty(),
        "nothing was written for the user"
    );

    // There is no file to save in place: refused, in words without a private path.
    let before = state(&session);
    let refused = session
        .begin_save(SaveTarget::InPlace)
        .run(&OperationContext::default())
        .expect_err("no file yet");
    assert_eq!(refused.kind, SaveFailureKind::Failed);
    let words = refused.to_string();
    assert!(words.contains(UNTITLED), "{words}");
    assert!(
        !words.contains(&session.private_directory().display().to_string()),
        "{words}"
    );
    assert_eq!(state(&session), before);

    // An occupied destination is refused and nothing moves.
    let taken = user.path().join("taken.fcad");
    std::fs::write(&taken, b"theirs").expect("occupied");
    let refused = save_as(&session, &taken).expect_err("occupied");
    assert_eq!(refused.kind, SaveFailureKind::Occupied);
    assert_eq!(std::fs::read(&taken).expect("theirs"), b"theirs");
    assert_eq!(state(&session), before);

    // Never into the working folder, nor through a link to it.
    let private = session.private_directory().to_path_buf();
    let inside = names(&private);
    let refused = save_as(&session, &private.join("mine.fcad")).expect_err("private");
    assert_eq!(refused.kind, SaveFailureKind::Failed);
    #[cfg(unix)]
    {
        let door = user.path().join("door");
        std::os::unix::fs::symlink(&private, &door).expect("link");
        let refused = save_as(&session, &door.join("mine.fcad")).expect_err("alias");
        assert_eq!(refused.kind, SaveFailureKind::Failed);
        std::fs::remove_file(&door).expect("unlink");
    }
    assert_eq!(names(&private), inside);
    assert_eq!(state(&session), before);

    // The first Save: exactly the accepted version, under the chosen name.
    let mut session = session;
    let current = session.current().version();
    let first = user.path().join("first.fcad");
    let saved = save_as(&session, &first).expect("published");
    assert_eq!(saved.kind, SaveKind::As);
    session.record_saved(&saved);
    assert_eq!(session.logical_path(), Some(first.as_path()));
    assert_eq!(session.display_name(), "first.fcad");
    assert_eq!(session.saved_version(), Some(current));
    assert_eq!(version_of(&first), current, "the same document and content");
    assert!(!session.is_dirty());
    assert_eq!(session.history_len(), 1, "the history is kept");
    assert_eq!(names(user.path()), ["first.fcad", "taken.fcad"]);
}

#[test]
fn edits_undo_and_redo_before_the_first_save_never_make_it_saved() {
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let mut session = create(sessions.path(), plate());
    let created = session.current().version();
    let feature = feature_of(&session);

    apply(&mut session, feature, 22.0);
    assert!(session.is_dirty());
    let edited = session.current().version();
    assert_eq!(edited.document_id, created.document_id, "one document");

    // Back to the very version that was created: still never saved.
    session
        .commit_move(session.begin_undo().expect("undo"))
        .expect("shown");
    assert_eq!(session.current().version(), created);
    assert!(
        session.is_dirty(),
        "Undo to the created version is not a save"
    );
    session
        .commit_move(session.begin_redo().expect("redo"))
        .expect("shown");
    assert_eq!(session.current().version(), edited);

    // After the first Save the ordinary comparison applies, history and all.
    let file = user.path().join("plate.fcad");
    let saved = save_as(&session, &file).expect("published");
    session.record_saved(&saved);
    assert!(!session.is_dirty());
    assert_eq!(
        read_extrude_source(&file).expect("saved").features[0].feature,
        feature
    );
    session
        .commit_move(session.begin_undo().expect("undo"))
        .expect("shown");
    assert!(
        session.is_dirty(),
        "the created version is not what was saved"
    );
    session
        .commit_move(session.begin_redo().expect("redo"))
        .expect("shown");
    assert!(
        !session.is_dirty(),
        "Redo back to the saved version is clean"
    );

    // A no-op is still not a step.
    let generation = session.generation();
    let step = session
        .begin_step()
        .edit_extrude_height(
            feature,
            22.0,
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("an edit");
    assert!(!session.changes_model(&step));
    drop(step);
    assert_eq!(session.generation(), generation);

    // Later saves are in place, under the version guard.
    apply(&mut session, feature, 31.5);
    let saved = session
        .begin_save(SaveTarget::InPlace)
        .run(&OperationContext::default())
        .expect("in place");
    assert_eq!(saved.kind, SaveKind::InPlace);
    session.record_saved(&saved);
    assert!(!session.is_dirty());
    apply(&mut session, feature, 40.0);
    // Somebody else replaces the file: the guard refuses, as for an opened file.
    std::fs::remove_file(&file).expect("remove");
    create_document(
        CreateDocumentRequest::new(&file, plate(), "keep"),
        &OperationContext::default(),
    )
    .expect("another document there");
    let refused = session
        .begin_save(SaveTarget::InPlace)
        .run(&OperationContext::default())
        .expect_err("replaced");
    assert_eq!(refused.kind, SaveFailureKind::Conflict(Conflict::Replaced));
    assert!(session.is_dirty());
}

#[test]
fn a_failed_or_cancelled_creation_leaves_no_session_and_no_file() {
    let sessions = tempfile::tempdir().expect("sessions");

    // The produce step failing: nothing remains.
    let error = DocumentSession::create_in(sessions.path(), HistoryLimits::default(), |path| {
        std::fs::write(path, b"half").expect("partial");
        Err(CadError::input("refused"))
    })
    .expect_err("refused");
    assert_eq!(error.kind(), ErrorKind::Input);
    assert!(
        names(sessions.path()).is_empty(),
        "the candidate's folder stayed"
    );

    // Cancelled before it published its first version.
    let token = CancelToken::new();
    token.cancel();
    let error = DocumentSession::create_document_in(
        sessions.path(),
        HistoryLimits::default(),
        plate(),
        no_kernel,
        &OperationContext::default().with_cancel(token),
    )
    .expect_err("cancelled");
    assert_eq!(error.kind(), ErrorKind::Cancellation);
    assert!(names(sessions.path()).is_empty());

    // A drawn profile needs its kernel; without one it is refused, and nothing stays.
    let drawn = NewDocument::SketchExtrude(
        ferritecad_jobs::PolygonExtrusion::new(
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]],
            3.0,
        )
        .expect("a polygon"),
    );
    let error = DocumentSession::create_document_in(
        sessions.path(),
        HistoryLimits::default(),
        drawn,
        no_kernel,
        &OperationContext::default(),
    )
    .expect_err("no kernel for a drawn profile");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(names(sessions.path()).is_empty());

    // A session dropped before it was ever saved takes its folder with it.
    let session = create(sessions.path(), NewDocument::Empty);
    assert_eq!(names(sessions.path()).len(), 1);
    drop(session);
    assert!(names(sessions.path()).is_empty());
}

#[test]
fn the_first_save_is_published_or_not_and_a_late_cancellation_does_not_undo_it() {
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let mut session = create(sessions.path(), plate());
    let file = user.path().join("plate.fcad");

    // Cancelled before publication: nothing at the destination, still untitled.
    let token = CancelToken::new();
    token.cancel();
    let refused = session
        .begin_save(SaveTarget::As(file.clone()))
        .run(&OperationContext::default().with_cancel(token))
        .expect_err("cancelled");
    assert_eq!(refused.kind, SaveFailureKind::Cancelled);
    assert!(!file.exists());
    assert!(session.is_untitled() && session.is_dirty());

    // Cancelled after the file was linked: a success, and recorded as one.
    struct Late(CancelToken);
    impl SaveHooks for Late {
        fn after_publish(&mut self) {
            self.0.cancel();
        }
    }
    let token = CancelToken::new();
    let saved = session
        .begin_save(SaveTarget::As(file.clone()))
        .run_with(
            &OperationContext::default().with_cancel(token.clone()),
            &mut Late(token),
        )
        .expect("a late cancellation does not undo a published save");
    session.record_saved(&saved);
    assert_eq!(session.logical_path(), Some(file.as_path()));
    assert!(!session.is_dirty());
    assert_eq!(names(user.path()), ["plate.fcad"]);
}
