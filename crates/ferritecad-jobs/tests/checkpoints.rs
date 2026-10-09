// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
//! §30N: named checkpoints through the shared library, without a window and
//! without a geometry kernel (the mock kernel stands in for model changes):
//! the session's steps, the real committed schema v3 file, the copy operations'
//! guards and faults, and the crash copy carrying the catalog.

use std::path::{Path, PathBuf};

use ferritecad_document::{CheckpointEntry, Document, DocumentVersion};
use ferritecad_jobs::{
    CreateCheckpointRequest, CreateDocumentRequest, DeleteCheckpointRequest, DocumentSession,
    ExtractCheckpointRequest, HistoryLimits, NewDocument, PlateSize, ProducedStep, RecoveryStore,
    SaveTarget, StepCommit, create_checkpoint_copy, create_document, delete_checkpoint_copy,
    extract_checkpoint, list_checkpoints, read_extrude_source,
};
use ferritecad_kernel::{CancelToken, OperationContext, ProgressSink, mock::MockKernel};
use ferritecad_types::{CheckpointId, ErrorKind};

fn plate_v3() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../ferritecad-fixtures/plate/plate.fcad")
}

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("bytes")
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

fn listed(session: &DocumentSession) -> Vec<CheckpointEntry> {
    session
        .current()
        .checkpoints()
        .expect("a readable list")
        .to_vec()
}

fn context() -> OperationContext {
    OperationContext::default()
}

fn produce_create(session: &mut DocumentSession, name: &str) -> ProducedStep {
    session
        .begin_step()
        .create_checkpoint(name, &context())
        .expect("a checkpoint step")
}

fn create(session: &mut DocumentSession, name: &str) -> CheckpointEntry {
    let before = listed(session);
    let step = produce_create(session, name);
    assert!(step.keeps_picture(), "a checkpoint draws what was drawn");
    assert_eq!(
        session.commit_step(step).expect("accepted"),
        StepCommit::Accepted
    );
    let after = listed(session);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        &after[..before.len()],
        before.as_slice(),
        "earlier rows untouched"
    );
    after.last().expect("the new one").clone()
}

fn apply(session: &mut DocumentSession, millimetres: f64) {
    let feature = read_extrude_source(session.current().path())
        .expect("reading")
        .features[0]
        .feature;
    let step = session
        .begin_step()
        .edit_extrude_height(feature, millimetres, &mut MockKernel::new(), &context())
        .expect("an edit");
    assert!(!step.keeps_picture(), "a height changes what is drawn");
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

fn height(path: &Path) -> f64 {
    let source = read_extrude_source(path).expect("reading");
    source.features[0].distance_mm.expect("a constant height")
}

#[test]
fn an_old_file_gets_checkpoints_in_the_session_and_on_disk_only_through_save() {
    let dir = tempfile::tempdir().expect("dir");
    let work = tempfile::tempdir().expect("work");
    let user = dir.path().join("plate.fcad");
    std::fs::copy(plate_v3(), &user).expect("the committed schema v3 plate");
    let original = bytes(&user);

    // Listing and opening read the old file as it is.
    let listing = list_checkpoints(&user).expect("lists");
    assert!(listing.entries.is_empty());
    let mut session =
        DocumentSession::open_in(work.path(), &user, HistoryLimits::default()).expect("opens");
    assert!(listed(&session).is_empty() && !session.is_dirty());
    assert_eq!(bytes(&user), original, "open and list wrote nothing");

    // A checkpoint of the unsaved model is a step: dirty, undoable, redoable.
    let a = create(&mut session, "As opened");
    assert!(
        session.is_dirty(),
        "a new checkpoint is not lost to a clean mark"
    );
    assert_eq!(a.model, session.current().drawn_model());
    let with_a = session.current();
    undo(&mut session);
    assert!(listed(&session).is_empty() && !session.is_dirty());
    redo(&mut session);
    assert!(std::sync::Arc::ptr_eq(&session.current(), &with_a));
    assert_eq!(
        listed(&session),
        vec![a.clone()],
        "the same UUID, no new job"
    );
    assert_eq!(bytes(&user), original, "nothing but Save writes the file");

    // Save publishes it through the guarded in-place save; the file is v4 now.
    let saved = session
        .begin_save(SaveTarget::InPlace)
        .run(&context())
        .expect("saved");
    session.record_saved(&saved);
    assert!(!session.is_dirty());
    let reopened = list_checkpoints(&user).expect("lists");
    assert_eq!(reopened.entries, vec![a]);
    assert_eq!(reopened.model, reopened.entries[0].model);
}

#[test]
fn restore_is_one_step_that_keeps_the_catalog_and_undo_redo_move_through_it() {
    let dir = tempfile::tempdir().expect("dir");
    let work = tempfile::tempdir().expect("work");
    let user = dir.path().join("plate.fcad");
    create_document(
        CreateDocumentRequest::new(
            &user,
            NewDocument::SamplePlate(PlateSize {
                width: 60.0,
                depth: 40.0,
                height: 10.0,
            }),
            "keep",
        ),
        &context(),
    )
    .expect("a plate");
    let mut session =
        DocumentSession::open_in(work.path(), &user, HistoryLimits::default()).expect("opens");
    let a = create(&mut session, "Ten");
    let at_a = session.current();
    apply(&mut session, 17.0);
    apply(&mut session, 23.0);
    let b = create(&mut session, "Twenty-three");
    let before_restore = session.current();
    let user_bytes = bytes(&user);

    // Restore A: the model is A's again; the catalog is the current one.
    let step = session
        .begin_step()
        .restore_checkpoint(a.id, &context())
        .expect("restores");
    assert!(!step.keeps_picture(), "a restored model has to be drawn");
    assert_eq!(
        session.commit_step(step).expect("one step"),
        StepCommit::Accepted
    );
    let restored = session.current();
    assert_eq!(restored.drawn_model(), a.model);
    assert_eq!(restored.drawn_model(), at_a.drawn_model());
    assert_eq!(height(restored.path()), 10.0);
    assert_eq!(listed(&session), vec![a.clone(), b.clone()]);
    assert_eq!(bytes(&user), user_bytes, "Restore never writes the file");
    assert!(session.can_undo() && !session.can_redo());

    undo(&mut session);
    assert!(std::sync::Arc::ptr_eq(&session.current(), &before_restore));
    assert_eq!(height(session.current().path()), 23.0);
    redo(&mut session);
    assert!(std::sync::Arc::ptr_eq(&session.current(), &restored));

    // Restoring the model already current is no change; restoring B again is.
    let again = session
        .begin_step()
        .restore_checkpoint(a.id, &context())
        .expect("restores");
    assert!(again.keeps_picture());
    assert_eq!(
        session.commit_step(again).expect("no-op"),
        StepCommit::NoChange
    );
    assert!(std::sync::Arc::ptr_eq(&session.current(), &restored));

    // Delete is a step of its own; Undo brings the checkpoint back.
    let step = session
        .begin_step()
        .delete_checkpoint(a.id, &context())
        .expect("deletes");
    assert!(step.keeps_picture());
    session.commit_step(step).expect("accepted");
    assert_eq!(listed(&session), vec![b.clone()]);
    assert_eq!(session.current().drawn_model(), a.model, "the model stays");
    undo(&mut session);
    assert_eq!(listed(&session), vec![a.clone(), b.clone()]);
    redo(&mut session);
    assert_eq!(listed(&session), vec![b]);

    // A checkpoint that is not there is refused and changes nothing.
    let current = session.current();
    let refused = session
        .begin_step()
        .restore_checkpoint(a.id, &context())
        .expect_err("deleted");
    assert!(refused.to_string().contains("no checkpoint"), "{refused}");
    assert!(std::sync::Arc::ptr_eq(&session.current(), &current));

    // A step made before another one was accepted is stale.
    let late = produce_create(&mut session, "late");
    apply(&mut session, 12.0);
    let stale = session.commit_step(late).expect_err("stale");
    assert!(stale.to_string().contains(ferritecad_jobs::STALE_STEP));
}

#[test]
fn an_empty_untitled_document_keeps_checkpoints_through_save_as_and_reopen() {
    let work = tempfile::tempdir().expect("work");
    let dir = tempfile::tempdir().expect("dir");
    let mut session = DocumentSession::create_document_in(
        work.path(),
        HistoryLimits::default(),
        NewDocument::Empty,
        || -> ferritecad_types::Result<MockKernel> { panic!("Empty needs no kernel") },
        &context(),
    )
    .expect("Empty");
    let a = create(&mut session, "Nothing yet");
    let path = dir.path().join("empty.fcad");
    let saved = session
        .begin_save(SaveTarget::As(path.clone()))
        .run(&context())
        .expect("saved as");
    session.record_saved(&saved);
    drop(session);
    let reopened =
        DocumentSession::open_in(work.path(), &path, HistoryLimits::default()).expect("reopens");
    assert_eq!(listed(&reopened), vec![a.clone()]);
    let out = dir.path().join("extracted.fcad");
    let copy = extract_checkpoint(
        &ExtractCheckpointRequest {
            source: path.clone(),
            expected: None,
            checkpoint: a.id,
            destination: out.clone(),
        },
        &context(),
    )
    .expect("extracts");
    assert_eq!(copy.checkpoint, a);
    let extracted = Document::open_read_only(&out).expect("a document");
    assert!(
        extracted.objects().expect("objects").is_empty(),
        "Empty is Empty"
    );
    assert_eq!(
        extracted.model_without_checkpoints().expect("hash"),
        a.model
    );
    extracted.close().expect("close");
}

fn copy_fixture(dir: &Path) -> (PathBuf, CheckpointEntry) {
    let source = dir.join("source.fcad");
    std::fs::copy(plate_v3(), &source).expect("copies");
    let with = dir.join("with.fcad");
    let created = create_checkpoint_copy(
        &CreateCheckpointRequest {
            source: source.clone(),
            expected: version_of(&source),
            name: "First".to_owned(),
            destination: with.clone(),
        },
        &context(),
    )
    .expect("a copy with a checkpoint");
    (with, created.checkpoint)
}

#[test]
fn copies_need_the_expected_version_refuse_occupied_aliased_and_racing_destinations() {
    let dir = tempfile::tempdir().expect("dir");
    let (source, a) = copy_fixture(dir.path());
    let original = bytes(&source);
    let expected = version_of(&source);
    let request = |destination: PathBuf| DeleteCheckpointRequest {
        source: source.clone(),
        expected,
        checkpoint: a.id,
        destination,
    };

    let mut stale = request(dir.path().join("stale.fcad"));
    stale.expected.content = version_of(&plate_v3()).content;
    let refused = delete_checkpoint_copy(&stale, &context()).expect_err("stale version");
    assert!(refused.to_string().contains("has changed"), "{refused}");
    assert!(!stale.destination.exists());

    let occupied = dir.path().join("occupied.fcad");
    std::fs::write(&occupied, b"someone else's").expect("occupant");
    let refused =
        delete_checkpoint_copy(&request(occupied.clone()), &context()).expect_err("no clobber");
    assert!(refused.to_string().contains("already exists"), "{refused}");
    assert_eq!(bytes(&occupied), b"someone else's");

    let refused = delete_checkpoint_copy(&request(source.clone()), &context())
        .expect_err("the source itself");
    assert_eq!(refused.kind(), ErrorKind::Input);
    #[cfg(unix)]
    {
        let alias = dir.path().join("alias.fcad");
        std::os::unix::fs::symlink(&source, &alias).expect("symlink");
        delete_checkpoint_copy(&request(alias), &context()).expect_err("an alias of the source");
        let dangling = dir.path().join("dangling.fcad");
        std::os::unix::fs::symlink(dir.path().join("nowhere.fcad"), &dangling).expect("symlink");
        delete_checkpoint_copy(&request(dangling.clone()), &context())
            .expect_err("a dangling link is an entry");
        assert!(
            !dir.path().join("nowhere.fcad").exists(),
            "never written through"
        );
        let linked_dir = dir.path().join("linked");
        std::os::unix::fs::symlink(dir.path(), &linked_dir).expect("symlink");
        delete_checkpoint_copy(&request(linked_dir.join("source.fcad")), &context())
            .expect_err("the source reached through a linked folder");
    }
    assert_eq!(bytes(&source), original);

    // Faults at the last moment before publication: cancellation, an occupant
    // that appears, and a source that changes. None publishes; the source keeps
    // its bytes unless the test itself changed them.
    for event in ["cancel", "occupied", "source changed"] {
        let destination = dir
            .path()
            .join(format!("race-{}.fcad", event.replace(' ', "-")));
        let cancel = CancelToken::new();
        let stop = cancel.clone();
        let target = destination.clone();
        let changed = dir.path().join("changed.fcad");
        let kept = changed.clone();
        let watched = source.clone();
        let context = OperationContext::default()
            .with_cancel(cancel)
            .with_progress(ProgressSink::new(move |fraction| {
                if fraction == 0.95 {
                    match event {
                        "cancel" => stop.cancel(),
                        "occupied" => std::fs::write(&target, b"racer").expect("racer"),
                        _ => {
                            std::fs::copy(&watched, &kept).expect("keep");
                            let mut d = Document::open(&watched).expect("writer");
                            let object = d.objects().expect("objects").remove(0);
                            d.write(|w| {
                                w.put_object(
                                    object.id,
                                    object.parent,
                                    object.ordinal,
                                    Some("changed meanwhile"),
                                    &object.payload,
                                )
                            })
                            .expect("changes");
                            d.close().expect("close");
                        }
                    }
                }
            }));
        let result = delete_checkpoint_copy(&request(destination.clone()), &context);
        assert!(result.is_err(), "{event}");
        if event == "occupied" {
            assert_eq!(bytes(&destination), b"racer", "the racer's file is kept");
        } else {
            assert!(!destination.exists(), "{event}: nothing published");
        }
        if event == "source changed" {
            std::fs::copy(&changed, &source).expect("restore the fixture");
        }
        assert_eq!(bytes(&source), original, "{event}");
    }
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .expect("dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".ferritecad-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no scratch left behind: {leftovers:?}"
    );

    // The real thing, with the right version, works and is a v4 copy without A.
    let out = dir.path().join("without.fcad");
    let deleted = delete_checkpoint_copy(&request(out.clone()), &context()).expect("deletes");
    assert_eq!(deleted.checkpoint, a);
    assert!(list_checkpoints(&out).expect("lists").entries.is_empty());
    assert_eq!(deleted.content_version, version_of(&out).content);

    // Names and limits are refused before anything is written.
    for bad in ["", "\t", &"n".repeat(81)] {
        let destination = dir.path().join("named.fcad");
        create_checkpoint_copy(
            &CreateCheckpointRequest {
                source: source.clone(),
                expected,
                name: bad.to_owned(),
                destination: destination.clone(),
            },
            &context(),
        )
        .expect_err("a bad name");
        assert!(!destination.exists());
    }
    let missing = extract_checkpoint(
        &ExtractCheckpointRequest {
            source: plate_v3(),
            expected: None,
            checkpoint: CheckpointId::new(),
            destination: dir.path().join("missing.fcad"),
        },
        &context(),
    )
    .expect_err("a v3 file has no checkpoints");
    assert!(missing.to_string().contains("no checkpoint"), "{missing}");
}

#[test]
fn a_crash_copy_carries_the_checkpoints_of_the_accepted_version() {
    let root = tempfile::tempdir().expect("root");
    let work = tempfile::tempdir().expect("work");
    let store = RecoveryStore::open(root.path()).expect("store");
    let mut session = DocumentSession::create_document_in(
        work.path(),
        HistoryLimits::default(),
        NewDocument::SamplePlate(PlateSize {
            width: 50.0,
            depth: 30.0,
            height: 8.0,
        }),
        || -> ferritecad_types::Result<MockKernel> { panic!("no kernel") },
        &context(),
    )
    .expect("new");
    let a = create(&mut session, "Before");
    apply(&mut session, 15.0);
    let mut record = store.create_record().expect("record");
    record
        .publish(1, &session.current(), "plate.fcad")
        .expect("copy");
    let id = record.id();
    drop(record);

    let claim = store.claim(id).expect("claim");
    let recovered = DocumentSession::recover_in(work.path(), HistoryLimits::default(), claim)
        .expect("recovered");
    assert_eq!(listed(&recovered), vec![a.clone()]);
    assert_eq!(
        recovered.current().drawn_model(),
        session.current().drawn_model()
    );
    let mut recovered = recovered;
    let step = recovered
        .begin_step()
        .restore_checkpoint(a.id, &context())
        .expect("a recovered checkpoint restores");
    recovered.commit_step(step).expect("accepted");
    assert_eq!(height(recovered.current().path()), 8.0);
}
