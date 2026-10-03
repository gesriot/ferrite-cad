// SPDX-License-Identifier: MIT
//! Save and Save As: what the guard refuses, what it publishes, and the real race
//! boundaries. Files and state only; no window, no geometry.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use ferritecad_jobs::{
    Conflict, CreateDocumentRequest, DocumentSession, HistoryLimits, NewDocument, PlateSize,
    SaveFailureKind, SaveHooks, SaveKind, SaveTarget, create_document, read_extrude_source,
};
use ferritecad_kernel::{CancelToken, OperationContext, mock::MockKernel};
use ferritecad_types::ObjectId;

struct Fixture {
    root: tempfile::TempDir,
    sessions: tempfile::TempDir,
    file: PathBuf,
    feature: ObjectId,
}

fn make_plate(file: &Path, height: f64) {
    create_document(
        CreateDocumentRequest::new(
            file,
            NewDocument::SamplePlate(PlateSize {
                width: 70.0,
                depth: 40.0,
                height,
            }),
            "keep",
        ),
        &OperationContext::default(),
    )
    .expect("a plate");
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("directory");
    let file = root.path().join("plate.fcad");
    make_plate(&file, 13.0);
    let feature = read_extrude_source(&file).expect("reading").features[0].feature;
    Fixture {
        root,
        sessions: tempfile::tempdir().expect("sessions"),
        file,
        feature,
    }
}

fn open(f: &Fixture) -> DocumentSession {
    DocumentSession::open_in(f.sessions.path(), &f.file, HistoryLimits::default()).expect("session")
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

fn height_of(path: &Path) -> f64 {
    read_extrude_source(path).expect("reading").features[0]
        .distance_mm
        .expect("a height")
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

fn save(session: &DocumentSession) -> Result<ferritecad_jobs::Saved, ferritecad_jobs::SaveFailure> {
    session
        .begin_save(SaveTarget::InPlace)
        .run(&OperationContext::default())
}

/// Nothing but the user's file and what they put there may be in their directory.
fn only(f: &Fixture, expected: &[&str]) {
    let mut wanted: Vec<String> = expected.iter().map(|s| (*s).to_owned()).collect();
    wanted.sort();
    assert_eq!(names(f.root.path()), wanted, "scratch or lock files leaked");
}

#[test]
fn an_externally_modified_file_is_never_replaced_and_keeps_the_users_work() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);

    // Somebody else saves a different height to the same file.
    let rival = f.root.path().join("rival.fcad");
    std::fs::copy(&f.file, &rival).expect("copy");
    let mut rival_session =
        DocumentSession::open_in(f.sessions.path(), &rival, HistoryLimits::default())
            .expect("rival session");
    apply(&mut rival_session, f.feature, 31.0);
    let rival_saved = save(&rival_session).expect("rival saves");
    rival_session.record_saved(&rival_saved);
    std::fs::copy(&rival, &f.file).expect("their version lands on our file");
    let theirs = bytes(&f.file);

    let failure = save(&session).expect_err("must not overwrite");
    assert_eq!(
        failure.kind,
        SaveFailureKind::Conflict(Conflict::Modified),
        "{failure}"
    );
    assert_eq!(bytes(&f.file), theirs, "the file was touched");
    assert!(session.is_dirty(), "a refused Save moved the checkpoint");
    assert_eq!(height_of(session.current().path()), 22.0);
    std::fs::remove_file(&rival).expect("tidy");
    only(&f, &["plate.fcad"]);

    // The user's work survives: Save As keeps it.
    let kept = f.root.path().join("kept.fcad");
    let saved = session
        .begin_save(SaveTarget::As(kept.clone()))
        .run(&OperationContext::default())
        .expect("save as");
    session.record_saved(&saved);
    assert_eq!(height_of(&kept), 22.0);
    assert_eq!(session.logical_path(), kept);
    assert!(!session.is_dirty());
    assert_eq!(bytes(&f.file), theirs, "Save As touched the original");
}

#[test]
fn a_replaced_missing_or_foreign_file_is_a_typed_conflict_not_an_overwrite() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let original = bytes(&f.file);

    // A different document under the same name.
    let other = f.root.path().join("other.fcad");
    make_plate(&other, 5.0);
    std::fs::rename(&other, &f.file).expect("replace");
    let replaced = bytes(&f.file);
    let failure = save(&session).expect_err("replaced");
    assert_eq!(failure.kind, SaveFailureKind::Conflict(Conflict::Replaced));
    assert_eq!(bytes(&f.file), replaced);

    // Not a document at all.
    std::fs::write(&f.file, b"not a document").expect("write");
    let failure = save(&session).expect_err("foreign");
    assert_eq!(failure.kind, SaveFailureKind::Conflict(Conflict::Replaced));
    assert_eq!(bytes(&f.file), b"not a document");

    // Gone.
    std::fs::remove_file(&f.file).expect("remove");
    let failure = save(&session).expect_err("missing");
    assert_eq!(failure.kind, SaveFailureKind::Conflict(Conflict::Missing));
    assert!(!f.file.exists(), "Save recreated a file the user deleted");
    assert!(session.is_dirty());
    only(&f, &[]);
    let _ = original;
}

#[test]
fn save_as_refuses_an_occupied_path_and_succeeds_on_a_free_one() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let untouched = bytes(&f.file);

    // Its own path is occupied.
    let failure = session
        .begin_save(SaveTarget::As(f.file.clone()))
        .run(&OperationContext::default())
        .expect_err("occupied");
    assert_eq!(failure.kind, SaveFailureKind::Occupied);
    // So is somebody else's.
    let theirs = f.root.path().join("theirs.fcad");
    std::fs::write(&theirs, b"theirs").expect("write");
    let failure = session
        .begin_save(SaveTarget::As(theirs.clone()))
        .run(&OperationContext::default())
        .expect_err("occupied");
    assert_eq!(failure.kind, SaveFailureKind::Occupied);
    assert_eq!(bytes(&theirs), b"theirs");
    assert_eq!(bytes(&f.file), untouched);
    assert!(session.is_dirty(), "a refused Save As moved the checkpoint");
    assert_eq!(
        session.logical_path(),
        f.file,
        "a refused Save As moved the path"
    );
    only(&f, &["plate.fcad", "theirs.fcad"]);

    // A dangling symlink is an entry too.
    #[cfg(unix)]
    {
        let dangling = f.root.path().join("dangling.fcad");
        std::os::unix::fs::symlink(f.root.path().join("nowhere"), &dangling).expect("link");
        let failure = session
            .begin_save(SaveTarget::As(dangling.clone()))
            .run(&OperationContext::default())
            .expect_err("occupied");
        assert_eq!(failure.kind, SaveFailureKind::Occupied);
        std::fs::remove_file(&dangling).expect("tidy");
    }

    let free = f.root.path().join("free.fcad");
    let saved = session
        .begin_save(SaveTarget::As(free.clone()))
        .run(&OperationContext::default())
        .expect("saves");
    assert_eq!(saved.kind, SaveKind::As);
    session.record_saved(&saved);
    assert_eq!(height_of(&free), 22.0);
    assert_eq!(session.logical_path(), free);
    assert!(!session.is_dirty());
    assert_eq!(bytes(&f.file), untouched, "Save As changed the original");
    // The next Save goes to the new name.
    apply(&mut session, f.feature, 23.0);
    let saved = save(&session).expect("saves in place");
    session.record_saved(&saved);
    assert_eq!(height_of(&free), 23.0);
    assert_eq!(bytes(&f.file), untouched);
    only(&f, &["plate.fcad", "theirs.fcad", "free.fcad"]);
}

#[test]
fn a_read_only_file_is_refused_by_the_session_not_by_the_account_running_it() {
    // The refusal is the session's own metadata check, so it holds for an account
    // that could have written the file anyway (root).
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let original = std::fs::metadata(&f.file).expect("stat").permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&f.file, readonly).expect("read-only");
    let before = bytes(&f.file);
    let failure = save(&session).expect_err("read-only");
    assert_eq!(failure.kind, SaveFailureKind::Unwritable, "{failure}");
    assert_eq!(bytes(&f.file), before);
    assert!(session.is_dirty());
    std::fs::set_permissions(&f.file, original).expect("writable again");
    only(&f, &["plate.fcad"]);
}

#[cfg(unix)]
#[test]
fn a_symlink_is_saved_through_and_a_hard_linked_file_is_refused() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);

    // Hard link: replacing the entry would detach the other name.
    let other_name = f.root.path().join("other-name.fcad");
    std::fs::hard_link(&f.file, &other_name).expect("hard link");
    let failure = save(&session).expect_err("hard-linked");
    assert_eq!(failure.kind, SaveFailureKind::Unwritable, "{failure}");
    assert_eq!(height_of(&f.file), 13.0);
    std::fs::remove_file(&other_name).expect("tidy");

    // Symlink: the path the user chose stays a link and its target is replaced.
    let real = f.root.path().join("real.fcad");
    std::fs::rename(&f.file, &real).expect("move the file");
    std::os::unix::fs::symlink(&real, &f.file).expect("link");
    let saved = save(&session).expect("saves through the link");
    session.record_saved(&saved);
    assert!(
        std::fs::symlink_metadata(&f.file)
            .expect("stat")
            .file_type()
            .is_symlink(),
        "Save replaced the link"
    );
    assert_eq!(height_of(&real), 22.0);
    assert_eq!(session.logical_path(), f.file);
    only(&f, &["plate.fcad", "real.fcad"]);
}

/// Mutates the file at one of the save's boundaries.
struct Interfere<F: FnMut()> {
    after_copy: Option<F>,
    inside_lock: Option<F>,
}

impl<F: FnMut()> SaveHooks for Interfere<F> {
    fn after_copy(&mut self) {
        if let Some(action) = self.after_copy.as_mut() {
            action();
        }
    }
    fn inside_lock(&mut self) {
        if let Some(action) = self.inside_lock.as_mut() {
            action();
        }
    }
}

#[test]
fn a_change_between_the_first_compare_and_the_lock_is_caught_by_the_second_compare() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let file = f.file.clone();
    let mut interfere = Interfere {
        after_copy: Some(|| {
            // An external writer lands after the first compare and the copy.
            let other = file.with_file_name("late.fcad");
            // Same document id is what a real external edit keeps; changing the
            // height through the document API keeps it.
            std::fs::copy(&file, &other).expect("copy");
            let mut document = ferritecad_document::Document::open(&other).expect("open");
            let reading = read_extrude_source(&other).expect("reading");
            let prepared = ferritecad_document::prepare_extrude_height(
                &document,
                reading.features[0].feature,
                17.0,
            )
            .expect("prepared");
            document.write_extrude_height(&prepared).expect("written");
            document.close().expect("closed");
            std::fs::rename(&other, &file).expect("lands");
        }),
        inside_lock: None,
    };
    let failure = session
        .begin_save(SaveTarget::InPlace)
        .run_with(&OperationContext::default(), &mut interfere)
        .expect_err("the second compare must catch it");
    assert_eq!(failure.kind, SaveFailureKind::Conflict(Conflict::Modified));
    assert_eq!(height_of(&f.file), 17.0, "their write was overwritten");
    assert!(session.is_dirty());
    only(&f, &["plate.fcad"]);
}

#[test]
fn two_cooperating_savers_are_serialised_and_the_second_never_overwrites() {
    let f = fixture();
    let mut first = open(&f);
    let mut second = open(&f);
    apply(&mut first, f.feature, 22.0);
    apply(&mut second, f.feature, 33.0);

    let busy_seen = Cell::new(false);
    let mut interfere = Interfere {
        after_copy: None,
        // The first saver holds the lock here; the second saver runs entirely
        // inside that window.
        inside_lock: Some(|| {
            let failure = second
                .begin_save(SaveTarget::InPlace)
                .run(&OperationContext::default())
                .expect_err("the lock is held");
            assert_eq!(failure.kind, SaveFailureKind::Busy, "{failure}");
            busy_seen.set(true);
        }),
    };
    let saved = first
        .begin_save(SaveTarget::InPlace)
        .run_with(&OperationContext::default(), &mut interfere)
        .expect("the first saver wins");
    assert!(busy_seen.get(), "the hook did not run inside the lock");
    first.record_saved(&saved);
    assert_eq!(height_of(&f.file), 22.0);

    // Once the first has published, the second finds the file is not what it read.
    let failure = save(&second).expect_err("the second must not overwrite");
    assert_eq!(failure.kind, SaveFailureKind::Conflict(Conflict::Modified));
    assert_eq!(height_of(&f.file), 22.0);
    assert!(second.is_dirty());
    // And no lock file is left behind.
    only(&f, &["plate.fcad"]);
}

#[test]
fn a_writer_that_ignores_the_lock_can_win_the_instant_before_the_rename() {
    // The documented limit, not a promise: the compare under the lock and the
    // rename are two operations, and an external writer that does not take the
    // lock can land between them. This pins what happens so nobody mistakes it
    // for protection.
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let file = f.file.clone();
    let mut interfere = Interfere {
        after_copy: None,
        inside_lock: Some(|| {
            let other = file.with_file_name("sneaky.fcad");
            std::fs::copy(&file, &other).expect("copy");
            let mut document = ferritecad_document::Document::open(&other).expect("open");
            let reading = read_extrude_source(&other).expect("reading");
            let prepared = ferritecad_document::prepare_extrude_height(
                &document,
                reading.features[0].feature,
                99.0,
            )
            .expect("prepared");
            document.write_extrude_height(&prepared).expect("written");
            document.close().expect("closed");
            std::fs::rename(&other, &file).expect("lands");
        }),
    };
    let saved = session
        .begin_save(SaveTarget::InPlace)
        .run_with(&OperationContext::default(), &mut interfere)
        .expect("the window is open, the save goes through");
    session.record_saved(&saved);
    assert_eq!(height_of(&f.file), 22.0, "the external write was replaced");
    only(&f, &["plate.fcad"]);
}

#[test]
fn cancellation_is_honoured_until_the_rename_and_late_cancellation_is_a_success() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let before = bytes(&f.file);

    // Cancelled before anything: nothing is touched.
    let token = CancelToken::new();
    token.cancel();
    let failure = session
        .begin_save(SaveTarget::InPlace)
        .run(&OperationContext::default().with_cancel(token))
        .expect_err("cancelled");
    assert_eq!(failure.kind, SaveFailureKind::Cancelled);
    assert_eq!(bytes(&f.file), before);

    // Cancelled while holding the lock, just before the rename: still nothing.
    let token = CancelToken::new();
    let canceller = token.clone();
    let mut interfere = Interfere {
        after_copy: None,
        inside_lock: Some(move || canceller.cancel()),
    };
    let failure = session
        .begin_save(SaveTarget::InPlace)
        .run_with(
            &OperationContext::default().with_cancel(token),
            &mut interfere,
        )
        .expect_err("cancelled at the last moment");
    assert_eq!(failure.kind, SaveFailureKind::Cancelled);
    assert_eq!(bytes(&f.file), before, "a cancelled save changed the file");
    assert!(session.is_dirty());
    only(&f, &["plate.fcad"]);

    // Cancelled after the file was replaced: the answer is the truth, a success.
    struct Late(CancelToken);
    impl SaveHooks for Late {
        fn after_publish(&mut self) {
            self.0.cancel();
        }
    }
    let token = CancelToken::new();
    let saved = session
        .begin_save(SaveTarget::InPlace)
        .run_with(
            &OperationContext::default().with_cancel(token.clone()),
            &mut Late(token),
        )
        .expect("a late cancellation does not undo a published save");
    session.record_saved(&saved);
    assert_eq!(height_of(&f.file), 22.0);
    assert!(!session.is_dirty());
    only(&f, &["plate.fcad"]);
}

#[test]
fn a_failed_save_leaves_the_session_and_the_directory_exactly_as_they_were() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    // Save As into a directory that does not exist: an I/O failure before anything.
    let nowhere = f.root.path().join("no-such-directory").join("x.fcad");
    let failure = session
        .begin_save(SaveTarget::As(nowhere))
        .run(&OperationContext::default())
        .expect_err("no directory");
    assert_eq!(failure.kind, SaveFailureKind::Failed);
    assert!(session.is_dirty());
    assert_eq!(session.logical_path(), f.file);
    only(&f, &["plate.fcad"]);
}

const LOCK_NAME: &str = ".plate.fcad.ferritecad-save-lock";

#[cfg(unix)]
#[test]
fn two_names_for_one_file_meet_at_one_lock() {
    let f = fixture();
    let alias = f.root.path().join("alias.fcad");
    std::os::unix::fs::symlink(&f.file, &alias).expect("link");
    let mut first = open(&f);
    let mut second = DocumentSession::open_in(f.sessions.path(), &alias, HistoryLimits::default())
        .expect("a second window on the link");
    apply(&mut first, f.feature, 22.0);
    apply(&mut second, f.feature, 31.0);

    let busy_seen = Cell::new(false);
    let mut interfere = Interfere {
        after_copy: None,
        inside_lock: Some(|| {
            let failure = second
                .begin_save(SaveTarget::InPlace)
                .run(&OperationContext::default())
                .expect_err("the other name for the same file must find the lock held");
            assert_eq!(failure.kind, SaveFailureKind::Busy, "{failure}");
            busy_seen.set(true);
        }),
    };
    let saved = first
        .begin_save(SaveTarget::InPlace)
        .run_with(&OperationContext::default(), &mut interfere)
        .expect("the first saver publishes");
    assert!(busy_seen.get());
    first.record_saved(&saved);
    assert_eq!(height_of(&f.file), 22.0);

    let failure = save(&second).expect_err("and then finds the file changed");
    assert_eq!(failure.kind, SaveFailureKind::Conflict(Conflict::Modified));
    assert_eq!(height_of(&f.file), 22.0);
    assert!(
        std::fs::symlink_metadata(&alias)
            .expect("alias")
            .file_type()
            .is_symlink()
    );
    only(&f, &["alias.fcad", "plate.fcad"]);
}

#[test]
fn a_file_with_the_lock_name_that_is_not_ours_is_never_changed_or_removed() {
    for content in [
        &b"unrelated user data"[..],
        &b""[..],
        &b"FERRITECAD-SAVE-LOCK 2\n"[..],
    ] {
        let f = fixture();
        let mut session = open(&f);
        apply(&mut session, f.feature, 22.0);
        let foreign = f.root.path().join(LOCK_NAME);
        std::fs::write(&foreign, content).expect("a user's file");
        let before = bytes(&f.file);
        let failure = save(&session).expect_err("Save must not go past somebody's file");
        assert_eq!(failure.kind, SaveFailureKind::Failed, "{failure}");
        assert_eq!(bytes(&foreign), content, "the user's file was changed");
        assert_eq!(bytes(&f.file), before);
        assert!(session.is_dirty());
        only(&f, &["plate.fcad", LOCK_NAME]);
    }
}

#[cfg(unix)]
#[test]
fn a_link_with_the_lock_name_is_neither_followed_nor_removed() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let precious = f.root.path().join("precious.txt");
    std::fs::write(&precious, b"keep me").expect("data");
    let foreign = f.root.path().join(LOCK_NAME);
    std::os::unix::fs::symlink(&precious, &foreign).expect("link");
    let failure = save(&session).expect_err("a link is not our lock");
    assert_eq!(failure.kind, SaveFailureKind::Failed, "{failure}");
    assert_eq!(bytes(&precious), b"keep me");
    assert!(
        std::fs::symlink_metadata(&foreign)
            .expect("still there")
            .file_type()
            .is_symlink()
    );
    // A dangling link is not created through either.
    std::fs::remove_file(&foreign).expect("remove");
    std::os::unix::fs::symlink(f.root.path().join("nowhere"), &foreign).expect("link");
    let failure = save(&session).expect_err("a dangling link is not our lock");
    assert_eq!(failure.kind, SaveFailureKind::Failed, "{failure}");
    assert!(!f.root.path().join("nowhere").exists());
}

#[test]
fn a_lock_left_by_a_dead_saver_is_taken_over_and_removed() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    std::fs::write(f.root.path().join(LOCK_NAME), b"FERRITECAD-SAVE-LOCK 1\n").expect("stale");
    let saved = save(&session).expect("a stale lock of ours does not block Save");
    session.record_saved(&saved);
    assert_eq!(height_of(&f.file), 22.0);
    only(&f, &["plate.fcad"]);
}

#[test]
fn a_name_that_was_replaced_while_held_is_left_to_whoever_put_it_there() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let foreign = f.root.path().join(LOCK_NAME);
    let mut interfere = Interfere {
        after_copy: None,
        inside_lock: Some(|| {
            std::fs::remove_file(&foreign).expect("remove ours");
            std::fs::write(&foreign, b"somebody else's").expect("theirs");
        }),
    };
    session
        .begin_save(SaveTarget::InPlace)
        .run_with(&OperationContext::default(), &mut interfere)
        .expect("saves");
    assert_eq!(bytes(&foreign), b"somebody else's");
}

#[test]
fn a_file_made_read_only_after_the_copy_is_refused_under_the_lock() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let file = f.file.clone();
    let original = std::fs::metadata(&file).expect("stat").permissions();
    let mut interfere = Interfere {
        after_copy: Some(|| {
            let mut readonly = original.clone();
            readonly.set_readonly(true);
            std::fs::set_permissions(&file, readonly).expect("read-only");
        }),
        inside_lock: None,
    };
    let before = bytes(&f.file);
    let failure = session
        .begin_save(SaveTarget::InPlace)
        .run_with(&OperationContext::default(), &mut interfere)
        .expect_err("read-only by now");
    assert_eq!(failure.kind, SaveFailureKind::Unwritable, "{failure}");
    assert_eq!(bytes(&f.file), before);
    std::fs::set_permissions(&f.file, original).expect("writable again");
    only(&f, &["plate.fcad"]);
}

#[cfg(unix)]
#[test]
fn a_hard_link_made_after_the_copy_is_refused_under_the_lock() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let file = f.file.clone();
    let twin = f.root.path().join("twin.fcad");
    let mut interfere = Interfere {
        after_copy: Some(|| std::fs::hard_link(&file, &twin).expect("link")),
        inside_lock: None,
    };
    let before = bytes(&f.file);
    let failure = session
        .begin_save(SaveTarget::InPlace)
        .run_with(&OperationContext::default(), &mut interfere)
        .expect_err("hard-linked by now");
    assert_eq!(failure.kind, SaveFailureKind::Unwritable, "{failure}");
    assert_eq!(bytes(&f.file), before);
    assert_eq!(bytes(&twin), before);
    only(&f, &["plate.fcad", "twin.fcad"]);
}

#[cfg(unix)]
#[test]
fn a_link_retargeted_after_the_copy_is_not_saved_through() {
    let f = fixture();
    let other = f.root.path().join("other.fcad");
    make_plate(&other, 50.0);
    let alias = f.root.path().join("alias.fcad");
    std::os::unix::fs::symlink(&f.file, &alias).expect("link");
    let mut session = DocumentSession::open_in(f.sessions.path(), &alias, HistoryLimits::default())
        .expect("a window on the link");
    apply(&mut session, f.feature, 22.0);
    let mut interfere = Interfere {
        after_copy: Some(|| {
            std::fs::remove_file(&alias).expect("unlink");
            std::os::unix::fs::symlink(&other, &alias).expect("retarget");
        }),
        inside_lock: None,
    };
    let (plate_before, other_before) = (bytes(&f.file), bytes(&other));
    let failure = session
        .begin_save(SaveTarget::InPlace)
        .run_with(&OperationContext::default(), &mut interfere)
        .expect_err("the link now names another file");
    assert!(
        matches!(failure.kind, SaveFailureKind::Conflict(_)),
        "{failure}"
    );
    assert_eq!(bytes(&f.file), plate_before);
    assert_eq!(bytes(&other), other_before);
    assert!(session.is_dirty());
    only(&f, &["alias.fcad", "other.fcad", "plate.fcad"]);
}

#[test]
fn save_as_never_writes_into_the_sessions_own_disposable_directory() {
    let f = fixture();
    let mut session = open(&f);
    apply(&mut session, f.feature, 22.0);
    let private = session.private_directory().to_path_buf();
    assert!(session.owns(&private.join("mine.fcad")));
    assert!(session.owns(&private.join("nested").join("deeper").join("mine.fcad")));
    assert!(!session.owns(&f.file));
    let attempt = private.join("mine.fcad");
    let before = names(&private);
    let failure = session
        .begin_save(SaveTarget::As(attempt.clone()))
        .run(&OperationContext::default())
        .expect_err("the working folder is not a place to keep a file");
    assert_eq!(failure.kind, SaveFailureKind::Failed, "{failure}");
    assert!(!attempt.exists());
    assert_eq!(names(&private), before);
    assert!(session.is_dirty());

    // By what the path names, not how it is spelled.
    #[cfg(unix)]
    {
        let door = f.root.path().join("door");
        std::os::unix::fs::symlink(&private, &door).expect("link");
        assert!(session.owns(&door.join("mine.fcad")));
        let failure = session
            .begin_save(SaveTarget::As(door.join("mine.fcad")))
            .run(&OperationContext::default())
            .expect_err("a link into it is still inside it");
        assert_eq!(failure.kind, SaveFailureKind::Failed, "{failure}");
        assert_eq!(names(&private), before);
    }
    // Next to the user's file it is fine.
    let ok = f.root.path().join("kept.fcad");
    session
        .begin_save(SaveTarget::As(ok.clone()))
        .run(&OperationContext::default())
        .expect("a place of the user's own");
    assert_eq!(height_of(&ok), 22.0);
}

#[test]
fn the_working_folder_guard_uses_filesystem_identity_for_case_aliases() {
    let f = fixture();
    let session = open(&f);
    let private = session.private_directory();
    let alias = private.with_file_name(
        private
            .file_name()
            .expect("directory name")
            .to_string_lossy()
            .to_uppercase(),
    );
    let attempt = alias.join("must-not-disappear.fcad");
    if same_file::is_same_file(private, &alias).unwrap_or(false) {
        let before = names(private);
        assert!(
            session.owns(&attempt),
            "a differently cased name is the same working folder"
        );
        let failure = session
            .begin_save(SaveTarget::As(attempt.clone()))
            .run(&OperationContext::default())
            .expect_err("no publication into the working folder");
        assert_eq!(failure.kind, SaveFailureKind::Failed);
        assert!(!attempt.exists());
        assert_eq!(names(private), before);
    } else {
        // A case-sensitive filesystem treats this as an unrelated sibling.
        assert!(!session.owns(&attempt));
    }
}

#[cfg(unix)]
#[test]
fn an_outward_leaf_symlink_still_occupies_the_working_folder() {
    let f = fixture();
    let session = open(&f);
    let outside = f.root.path().join("outside.fbx");
    std::fs::write(&outside, b"keep outside").expect("outside file");
    let entry = session.private_directory().join("output.fbx");
    std::os::unix::fs::symlink(&outside, &entry).expect("outward link");
    // A force export replaces the directory entry, not the symlink's target.
    // Such an export would disappear with this session despite resolving outside.
    assert!(
        session.owns(&entry),
        "publication would replace an entry inside the disposable folder"
    );
    assert_eq!(bytes(&outside), b"keep outside");
}
