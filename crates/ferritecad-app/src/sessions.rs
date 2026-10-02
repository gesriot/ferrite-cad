// SPDX-License-Identifier: MIT
//! The window's side of the open document session (ADR 0005).
//!
//! The session itself — what is accepted, what is saved, what can be undone, how a
//! file is replaced — is `ferritecad_jobs::DocumentSession`, which needs no window
//! and is tested without one. This module is the part a window adds: which
//! operation is in flight, which worker answers are still wanted, what to do after
//! a Save the user asked for on the way to something else, and the sentences a
//! person reads. It holds no second copy of any fact the session owns.
//!
//! # One operation at a time
//!
//! Apply, Undo, Redo, Save and Save As are one operation each, and only one runs.
//! An Apply has two phases that share a generation: the edit (a worker, which makes
//! a version) and the scene (a worker, which builds the picture of it). The version
//! becomes current only when the picture is prepared, in the same statement that
//! replaces the shown one ([`Sessions::commit_staged`], called from the window's
//! `show`). Anything that goes wrong before that — a failed edit, a stale answer,
//! a cancellation, a picture that cannot be uploaded — drops the produced version
//! and changes nothing else.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::JoinHandle;

use ferritecad_jobs::{
    DocumentSession, Move, ProducedStep, SaveFailure, SaveFailureKind, SavePlan, SaveTarget, Saved,
    StepCommit, StepTicket,
};
use ferritecad_kernel::{CancelToken, OperationContext, TessellationParams};
use ferritecad_scene::LoadedScene;
use ferritecad_types::{CadError, ErrorKind, ObjectId, Result};

use crate::PRODUCT_NAME;

/// What the user asked for on the way to which a Save was offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Continuation {
    /// Choose another document.
    Open,
    /// Make a new document.
    New,
    /// Close the window.
    Quit,
}

/// The answer to "this document has unsaved changes".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

/// What the decision to replace the document leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Replace {
    /// Nothing is lost by going on.
    Go,
    /// The user chose Discard: going on is permitted for this one request, and the
    /// unsaved changes stay in the session until the replacement is accepted.
    Discarded,
    /// The user chose Save: go on when it has succeeded.
    AfterSave,
    /// The user chose Cancel (or the question could not be asked): stay.
    Stay,
}

/// Which operation is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Apply,
    Undo,
    Redo,
    Save,
    SaveAs,
}

enum Staged {
    Step(ProducedStep),
    Move(Move),
}

/// What an arriving picture is bound to, decided at the moment it replaces the
/// shown one.
pub(crate) enum Bind {
    /// An Open: the session the picture was read from becomes the session, and the
    /// previous one (with its private files) is dropped.
    Open(Box<DocumentSession>),
    /// An Apply, Undo or Redo: the staged version becomes current.
    Staged,
}

struct Operation {
    generation: u64,
    kind: Kind,
    cancel: CancelToken,
    worker: Option<JoinHandle<()>>,
    staged: Option<Staged>,
}

/// What the edit's answer means for the window.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Edited {
    /// Not the answer to the operation in flight: nothing at all happens.
    Ignore,
    /// The edit failed or was cancelled; the session, the picture and the draft are
    /// as they were.
    Failed,
    /// The edit succeeded and the model is the model that was already there.
    NoChange,
    /// Build the picture of this version; it becomes current when it is shown.
    Show(PathBuf),
}

/// What a finished Save means for the window.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SaveReport {
    /// Published: the title and the toolbar change.
    pub(crate) published: bool,
    /// What the user was on their way to, when the Save succeeded.
    pub(crate) continuation: Option<Continuation>,
}

#[derive(Default)]
pub(crate) struct Sessions {
    session: Option<DocumentSession>,
    operation: Option<Operation>,
    issued: u64,
    /// What the user was on their way to when they pressed Save.
    after_save: Option<Continuation>,
    /// The last thing that happened to the document, in a sentence. Not state:
    /// nothing is read back from it.
    pub(crate) status: String,
}

impl Sessions {
    pub(crate) fn has_session(&self) -> bool {
        self.session.is_some()
    }

    pub(crate) fn dirty(&self) -> bool {
        self.session.as_ref().is_some_and(DocumentSession::is_dirty)
    }

    pub(crate) fn busy(&self) -> bool {
        self.operation.is_some()
    }

    pub(crate) fn logical_path(&self) -> Option<&Path> {
        self.session.as_ref().map(DocumentSession::logical_path)
    }

    pub(crate) fn name(&self) -> Option<String> {
        self.session.as_ref().map(DocumentSession::display_name)
    }

    pub(crate) fn title(&self) -> String {
        match (self.name(), self.dirty()) {
            (None, _) => PRODUCT_NAME.to_owned(),
            (Some(name), false) => format!("{name} — {PRODUCT_NAME}"),
            (Some(name), true) => format!("*{name} — {PRODUCT_NAME}"),
        }
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.busy() && self.session.as_ref().is_some_and(DocumentSession::can_undo)
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.busy() && self.session.as_ref().is_some_and(DocumentSession::can_redo)
    }

    pub(crate) fn can_save(&self) -> bool {
        !self.busy() && self.dirty()
    }

    pub(crate) fn can_save_as(&self) -> bool {
        !self.busy() && self.has_session()
    }

    /// The session of the document that was just accepted replaces the old one,
    /// which is dropped here and takes its private files with it.
    pub(crate) fn adopt(&mut self, session: DocumentSession) {
        self.session = Some(session);
        self.after_save = None;
        self.status.clear();
    }

    // --- Apply -----------------------------------------------------------

    /// Starts the edit. `spawn` is handed the ticket and the generation to label its
    /// answer with.
    pub(crate) fn begin_apply(
        &mut self,
        spawn: impl FnOnce(StepTicket, u64, &CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        if self.busy() {
            return None;
        }
        let session = self.session.as_mut()?;
        let ticket = session.begin_step();
        self.issued += 1;
        let generation = self.issued;
        let cancel = CancelToken::new();
        let worker = spawn(ticket, generation, &cancel);
        self.operation = Some(Operation {
            generation,
            kind: Kind::Apply,
            cancel,
            worker: Some(worker),
            staged: None,
        });
        self.status = "Applying the change…".to_owned();
        Some(generation)
    }

    /// The answer of the edit worker.
    pub(crate) fn finish_apply(&mut self, generation: u64, result: Result<ProducedStep>) -> Edited {
        let Some(operation) = self.operation.as_mut() else {
            return Edited::Ignore;
        };
        if operation.generation != generation || operation.kind != Kind::Apply {
            return Edited::Ignore;
        }
        if let Some(worker) = operation.worker.take() {
            let _ = worker.join();
        }
        let cancelled = operation.cancel.is_cancelled();
        let step = match result {
            Ok(step) if !cancelled => step,
            Ok(_) => {
                self.operation = None;
                self.status = "Cancelled; nothing was changed.".to_owned();
                return Edited::Failed;
            }
            Err(error) => {
                self.operation = None;
                self.status = if error.kind() == ErrorKind::Cancellation {
                    "Cancelled; nothing was changed.".to_owned()
                } else {
                    format!("Could not apply the change: {error}")
                };
                return Edited::Failed;
            }
        };
        let session = self.session.as_ref().expect("an operation has a session");
        if !session.changes_model(&step) {
            self.operation = None;
            self.status = "No change: the model is already like that.".to_owned();
            return Edited::NoChange;
        }
        let path = step.path().to_path_buf();
        self.operation.as_mut().expect("in flight").staged = Some(Staged::Step(step));
        Edited::Show(path)
    }

    // --- Undo and Redo ---------------------------------------------------

    /// Asks the session for one step back or forward. The version to show is
    /// returned; nothing is current until it has been shown.
    pub(crate) fn begin_move(&mut self, undo: bool) -> Option<(u64, PathBuf)> {
        if self.busy() {
            return None;
        }
        let session = self.session.as_ref()?;
        let request = if undo {
            session.begin_undo()?
        } else {
            session.begin_redo()?
        };
        let path = request.path().to_path_buf();
        self.issued += 1;
        let generation = self.issued;
        self.operation = Some(Operation {
            generation,
            kind: if undo { Kind::Undo } else { Kind::Redo },
            cancel: CancelToken::new(),
            worker: None,
            staged: Some(Staged::Move(request)),
        });
        self.status = if undo { "Undoing…" } else { "Redoing…" }.to_owned();
        Some((generation, path))
    }

    // --- the scene phase ---------------------------------------------------

    /// Records the worker that is building the picture of the staged version, and
    /// the token that stops it.
    pub(crate) fn attach_scene(&mut self, generation: u64, worker: JoinHandle<()>) -> bool {
        match self.operation.as_mut() {
            Some(op) if op.generation == generation && op.staged.is_some() => {
                op.worker = Some(worker);
                true
            }
            _ => {
                let _ = worker.join();
                false
            }
        }
    }

    pub(crate) fn scene_token(&self, generation: u64) -> Option<CancelToken> {
        self.operation
            .as_ref()
            .filter(|op| op.generation == generation)
            .map(|op| op.cancel.clone())
    }

    /// Where the picture being waited for was read from.
    pub(crate) fn staged_path(&self, generation: u64) -> Option<PathBuf> {
        let op = self
            .operation
            .as_ref()
            .filter(|op| op.generation == generation)?;
        match op.staged.as_ref()? {
            Staged::Step(step) => Some(step.path().to_path_buf()),
            Staged::Move(request) => Some(request.path().to_path_buf()),
        }
    }

    /// Makes the arriving picture's session binding current. Called between
    /// preparing the picture and replacing the shown one: it can fail, and then the
    /// picture is not shown either.
    pub(crate) fn bind(&mut self, bind: Bind) -> Result<()> {
        match bind {
            Bind::Open(session) => {
                self.adopt(*session);
                Ok(())
            }
            Bind::Staged => self.commit_staged(),
        }
    }

    /// Makes the staged version current. Called by the window between preparing the
    /// picture and replacing the shown one, so a version is current exactly when its
    /// picture is on screen.
    pub(crate) fn commit_staged(&mut self) -> Result<()> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| CadError::input("there is no open document"))?;
        let staged = self
            .operation
            .as_mut()
            .and_then(|op| op.staged.take())
            .ok_or_else(|| CadError::input("no change is waiting to be shown"))?;
        match staged {
            Staged::Step(step) => match session.commit_step(step)? {
                StepCommit::Accepted | StepCommit::NoChange => Ok(()),
            },
            Staged::Move(request) => session.commit_move(request),
        }
    }

    /// The scene phase is over. `shown` is whether the picture replaced the old one.
    pub(crate) fn finish_scene(&mut self, generation: u64, shown: Result<()>) -> bool {
        let Some(operation) = self.operation.take_if(|op| op.generation == generation) else {
            return false;
        };
        if let Some(worker) = operation.worker {
            let _ = worker.join();
        }
        self.status = match (&shown, operation.kind) {
            (Ok(()), Kind::Apply) => "Applied. Undo is available; Save writes the file.".to_owned(),
            (Ok(()), Kind::Undo) => "Undone.".to_owned(),
            (Ok(()), Kind::Redo) => "Redone.".to_owned(),
            (Err(error), _) if error.kind() == ErrorKind::Cancellation => {
                "Cancelled; nothing was changed.".to_owned()
            }
            (Err(error), _) => format!("Could not show the change; nothing was changed: {error}"),
            (Ok(()), _) => String::new(),
        };
        // A staged version that was never shown is dropped here, removing its file.
        drop(operation.staged);
        true
    }

    // --- Save --------------------------------------------------------------

    pub(crate) fn begin_save(
        &mut self,
        target: SaveTarget,
        after: Option<Continuation>,
        spawn: impl FnOnce(SavePlan, u64, &CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        if self.busy() {
            return None;
        }
        let session = self.session.as_ref()?;
        let kind = match target {
            SaveTarget::InPlace => Kind::Save,
            SaveTarget::As(_) => Kind::SaveAs,
        };
        let plan = session.begin_save(target);
        self.issued += 1;
        let generation = self.issued;
        let cancel = CancelToken::new();
        let worker = spawn(plan, generation, &cancel);
        self.operation = Some(Operation {
            generation,
            kind,
            cancel,
            worker: Some(worker),
            staged: None,
        });
        self.after_save = after;
        self.status = "Saving…".to_owned();
        Some(generation)
    }

    /// The answer of the save worker. A published file moves the checkpoint (and,
    /// for Save As, the logical path); a failure moves nothing.
    pub(crate) fn finish_save(
        &mut self,
        generation: u64,
        result: std::result::Result<Saved, SaveFailure>,
    ) -> Option<SaveReport> {
        let operation = self.operation.take_if(|op| {
            op.generation == generation && matches!(op.kind, Kind::Save | Kind::SaveAs)
        })?;
        if let Some(worker) = operation.worker {
            let _ = worker.join();
        }
        let continuation = self.after_save.take();
        match result {
            Ok(saved) => {
                let session = self.session.as_mut().expect("a save has a session");
                session.record_saved(&saved);
                self.status = format!("Saved {}.", session.display_name());
                Some(SaveReport {
                    published: true,
                    continuation,
                })
            }
            Err(failure) => {
                self.status = describe_failure(&failure);
                Some(SaveReport {
                    published: false,
                    continuation: None,
                })
            }
        }
    }

    // --- stopping ----------------------------------------------------------

    /// Asks the operation in flight to stop. Returns whether there was one.
    pub(crate) fn cancel(&mut self) -> bool {
        match &self.operation {
            Some(op) => {
                op.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Stops and joins everything; the session and its private files go with it.
    pub(crate) fn stop_all(&mut self) {
        if let Some(mut operation) = self.operation.take() {
            operation.cancel.cancel();
            if let Some(worker) = operation.worker.take() {
                let _ = worker.join();
            }
        }
        self.after_save = None;
        self.session = None;
    }

    /// What replacing the document asks of the user.
    pub(crate) fn replacing(&self, choice: Option<UnsavedChoice>) -> Replace {
        if !self.dirty() {
            return Replace::Go;
        }
        match choice {
            Some(UnsavedChoice::Discard) => Replace::Discarded,
            Some(UnsavedChoice::Save) => Replace::AfterSave,
            Some(UnsavedChoice::Cancel) | None => Replace::Stay,
        }
    }
}

/// The sentence a person reads for a save that did not publish.
fn describe_failure(failure: &SaveFailure) -> String {
    match failure.kind {
        SaveFailureKind::Cancelled => "Save cancelled; the file was not changed.".to_owned(),
        _ => format!("Not saved: {}", failure.error),
    }
}

// --- workers -----------------------------------------------------------------

fn spawn<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
    deliver: impl FnOnce(T) + Send + 'static,
    stopped: impl FnOnce() -> T + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
            .unwrap_or_else(|_| stopped());
        deliver(result);
    })
}

/// The edit: the reused `edit-extrude` copy operation on the worker that owns the
/// kernel session.
pub(crate) fn spawn_apply(
    ticket: StepTicket,
    feature: ObjectId,
    distance_mm: f64,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ProducedStep>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            let context = OperationContext::default().with_cancel(cancel);
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ticket.edit_extrude_height(feature, distance_mm, &mut kernel, &context)
        },
        deliver,
        || Err(CadError::kernel("the edit worker stopped unexpectedly")),
    )
}

/// The picture of one private version, read cold exactly as Open reads a file.
pub(crate) fn spawn_scene(
    path: PathBuf,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<LoadedScene>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            let context = OperationContext::default().with_cancel(cancel);
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ferritecad_scene::snapshot_of(
                &path,
                &mut kernel,
                |kernel, source| kernel.import_step(source),
                &TessellationParams::default(),
                &context,
            )
        },
        deliver,
        || Err(CadError::kernel("the scene worker stopped unexpectedly")),
    )
}

pub(crate) fn spawn_save(
    plan: SavePlan,
    cancel: CancelToken,
    deliver: impl FnOnce(std::result::Result<Saved, SaveFailure>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || plan.run(&OperationContext::default().with_cancel(cancel)),
        deliver,
        || {
            Err(SaveFailure {
                kind: SaveFailureKind::Failed,
                error: CadError::io("saving", "the save worker stopped unexpectedly"),
            })
        },
    )
}

/// A slot a worker fills with the session it opened, and the event reads out.
pub(crate) type Opened = Arc<Mutex<Option<DocumentSession>>>;

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use ferritecad_jobs::{
        CreateDocumentRequest, HistoryLimits, NewDocument, PlateSize, create_document,
        read_extrude_source,
    };
    use ferritecad_kernel::mock::MockKernel;
    use std::sync::mpsc;

    struct Fixture {
        root: tempfile::TempDir,
        private: tempfile::TempDir,
        file: PathBuf,
        feature: ObjectId,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().expect("directory");
        let file = root.path().join("plate.fcad");
        create_document(
            CreateDocumentRequest::new(
                &file,
                NewDocument::SamplePlate(PlateSize {
                    width: 80.0,
                    depth: 40.0,
                    height: 12.0,
                }),
                "keep",
            ),
            &OperationContext::default(),
        )
        .expect("a plate");
        let feature = read_extrude_source(&file).expect("reading").features[0].feature;
        Fixture {
            root,
            private: tempfile::tempdir().expect("private root"),
            file,
            feature,
        }
    }

    fn open(f: &Fixture) -> Sessions {
        let mut sessions = Sessions::default();
        sessions.adopt(
            DocumentSession::open_in(f.private.path(), &f.file, HistoryLimits::default())
                .expect("session"),
        );
        sessions
    }

    /// The edit as the window runs it, on a thread, with the mock kernel (the same
    /// reused `edit-extrude` operation; geometry is measured by the native tests).
    fn apply(sessions: &mut Sessions, feature: ObjectId, millimetres: f64) -> (u64, Edited) {
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, _| {
                std::thread::spawn(move || {
                    let result = ticket.edit_extrude_height(
                        feature,
                        millimetres,
                        &mut MockKernel::new(),
                        &OperationContext::default(),
                    );
                    tx.send(result).expect("deliver");
                })
            })
            .expect("started");
        let result = rx.recv().expect("answer");
        (generation, sessions.finish_apply(generation, result))
    }

    fn private_files(sessions: &Sessions) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(
            sessions
                .session
                .as_ref()
                .expect("session")
                .private_directory(),
        )
        .expect("directory")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
        names.sort();
        names
    }

    fn height_of(path: &Path) -> f64 {
        read_extrude_source(path).expect("reading").features[0]
            .distance_mm
            .expect("height")
    }

    #[test]
    fn a_version_is_current_only_when_its_picture_is_shown() {
        let f = fixture();
        let mut sessions = open(&f);
        assert!(!sessions.dirty() && !sessions.busy());
        assert_eq!(sessions.title(), "plate.fcad — FerriteCAD");

        let (generation, edited) = apply(&mut sessions, f.feature, 25.0);
        let Edited::Show(path) = edited else {
            panic!("a changed model is shown: {edited:?}");
        };
        assert_eq!(height_of(&path), 25.0);
        // Staged, not accepted: nothing observable has moved yet.
        assert!(
            sessions.busy(),
            "the operation is not over until it is shown"
        );
        assert!(
            !sessions.dirty(),
            "a version nobody has seen is not accepted"
        );
        assert!(!sessions.can_undo());
        assert_eq!(sessions.title(), "plate.fcad — FerriteCAD");
        assert_eq!(sessions.staged_path(generation), Some(path));

        // The picture is ready: bind (the window's show) makes it current...
        sessions
            .bind(Bind::Staged)
            .expect("committed with the scene");
        assert!(sessions.dirty());
        assert_eq!(sessions.title(), "*plate.fcad — FerriteCAD");
        // ...and finishing the scene closes the operation.
        assert!(sessions.finish_scene(generation, Ok(())));
        assert!(!sessions.busy() && sessions.can_undo() && !sessions.can_redo());
        assert!(sessions.status.starts_with("Applied"));
        assert_eq!(height_of(&f.file), 12.0, "the user's file was not written");
    }

    #[test]
    fn a_failed_stale_or_cancelled_apply_changes_nothing() {
        let f = fixture();
        let mut sessions = open(&f);
        let before = private_files(&sessions);

        // The edit fails: nothing staged, nothing dirty, nothing left behind.
        let (_, edited) = apply(&mut sessions, ObjectId::new(), 25.0);
        assert_eq!(edited, Edited::Failed);
        assert!(!sessions.busy() && !sessions.dirty());
        assert!(sessions.status.starts_with("Could not apply the change"));
        assert_eq!(private_files(&sessions), before);

        // A reply that is not the operation in flight does nothing.
        assert_eq!(
            sessions.finish_apply(77, Err(CadError::input("stale"))),
            Edited::Ignore
        );
        let (generation, _) = apply(&mut sessions, f.feature, 30.0);
        sessions.stop_all();
        assert_eq!(
            sessions.finish_apply(generation, Err(CadError::input("late"))),
            Edited::Ignore
        );
    }

    #[test]
    fn a_cancelled_edit_that_still_finished_is_dropped_and_leaves_no_file() {
        let f = fixture();
        let mut sessions = open(&f);
        let before = private_files(&sessions);
        let (tx, rx) = mpsc::channel();
        let feature = f.feature;
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                let cancel = cancel.clone();
                std::thread::spawn(move || {
                    // The user cancels; the worker finishes anyway (it was past
                    // its last checkpoint).
                    cancel.cancel();
                    tx.send(ticket.edit_extrude_height(
                        feature,
                        26.0,
                        &mut MockKernel::new(),
                        &OperationContext::default(),
                    ))
                    .expect("deliver");
                })
            })
            .expect("started");
        let result = rx.recv().expect("answer");
        assert_eq!(sessions.finish_apply(generation, result), Edited::Failed);
        assert!(!sessions.dirty() && !sessions.busy());
        assert_eq!(sessions.status, "Cancelled; nothing was changed.");
        assert_eq!(
            private_files(&sessions),
            before,
            "the produced file was kept"
        );
    }

    #[test]
    fn a_picture_that_cannot_be_shown_drops_the_version_and_keeps_the_last_good_state() {
        let f = fixture();
        let mut sessions = open(&f);
        let before = private_files(&sessions);
        let (generation, edited) = apply(&mut sessions, f.feature, 25.0);
        assert!(matches!(edited, Edited::Show(_)));
        assert_eq!(private_files(&sessions).len(), before.len() + 1);

        // The window could not prepare the picture: bind is never reached.
        assert!(sessions.finish_scene(
            generation,
            Err(CadError::rendering("the device refused the upload"))
        ));
        assert!(!sessions.busy() && !sessions.dirty() && !sessions.can_undo());
        assert!(sessions.status.starts_with("Could not show the change"));
        assert_eq!(private_files(&sessions), before, "the staged file was kept");
        // The session still works.
        let (_, again) = apply(&mut sessions, f.feature, 25.0);
        assert!(matches!(again, Edited::Show(_)));
    }

    #[test]
    fn a_no_op_apply_is_reported_and_adds_nothing() {
        let f = fixture();
        let mut sessions = open(&f);
        let before = private_files(&sessions);
        let (_, edited) = apply(&mut sessions, f.feature, 12.0);
        assert_eq!(edited, Edited::NoChange);
        assert!(!sessions.busy() && !sessions.dirty() && !sessions.can_undo());
        assert!(sessions.status.starts_with("No change"));
        assert_eq!(private_files(&sessions), before);
    }

    fn accept(sessions: &mut Sessions, feature: ObjectId, millimetres: f64) {
        let (generation, edited) = apply(sessions, feature, millimetres);
        assert!(matches!(edited, Edited::Show(_)), "{edited:?}");
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
    }

    #[test]
    fn undo_and_redo_move_only_when_their_picture_is_shown() {
        let f = fixture();
        let mut sessions = open(&f);
        accept(&mut sessions, f.feature, 25.0);
        assert!(sessions.dirty());

        let (generation, path) = sessions.begin_move(true).expect("one step back");
        assert_eq!(height_of(&path), 12.0);
        assert!(sessions.dirty(), "Undo has not been shown yet");
        assert!(!sessions.can_undo() && !sessions.can_redo(), "busy");
        // The picture could not be shown: nothing moved.
        assert!(sessions.finish_scene(generation, Err(CadError::rendering("no"))));
        assert!(sessions.dirty() && sessions.can_undo());

        let (generation, _) = sessions.begin_move(true).expect("again");
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
        assert!(!sessions.dirty(), "back at the saved checkpoint");
        assert!(sessions.can_redo() && !sessions.can_undo());
        assert_eq!(sessions.status, "Undone.");

        let (generation, path) = sessions.begin_move(false).expect("forward");
        assert_eq!(height_of(&path), 25.0);
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
        assert!(sessions.dirty());
        assert!(
            sessions.begin_move(false).is_none(),
            "nothing further forward"
        );
    }

    fn save(
        sessions: &mut Sessions,
        target: SaveTarget,
        after: Option<Continuation>,
    ) -> Option<SaveReport> {
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_save(target, after, |plan, generation, _| {
                std::thread::spawn(move || {
                    tx.send(plan.run(&OperationContext::default()))
                        .expect("deliver");
                    let _ = generation;
                })
            })
            .expect("started");
        sessions.finish_save(generation, rx.recv().expect("answer"))
    }

    #[test]
    fn save_moves_the_checkpoint_only_when_the_file_was_published() {
        let f = fixture();
        let mut sessions = open(&f);
        accept(&mut sessions, f.feature, 25.0);
        assert!(sessions.can_save());

        // Somebody else changed the file: refused, the document is still dirty and
        // the continuation the user was on their way to is dropped.
        let theirs = f.root.path().join("theirs.fcad");
        std::fs::copy(&f.file, &theirs).expect("copy");
        let mut document = ferritecad_document::Document::open(&theirs).expect("open");
        let prepared = ferritecad_document::prepare_extrude_height(&document, f.feature, 99.0)
            .expect("prepared");
        document.write_extrude_height(&prepared).expect("written");
        document.close().expect("closed");
        std::fs::rename(&theirs, &f.file).expect("lands");
        let report =
            save(&mut sessions, SaveTarget::InPlace, Some(Continuation::Open)).expect("answered");
        assert_eq!(
            report,
            SaveReport {
                published: false,
                continuation: None
            }
        );
        assert!(sessions.dirty(), "a refused Save moved the checkpoint");
        assert!(
            sessions.status.starts_with("Not saved:"),
            "{}",
            sessions.status
        );
        assert_eq!(height_of(&f.file), 99.0, "their file was overwritten");

        // Save As keeps the work and moves the document to its new name.
        let kept = f.root.path().join("kept.fcad");
        let report = save(
            &mut sessions,
            SaveTarget::As(kept.clone()),
            Some(Continuation::Quit),
        )
        .expect("answered");
        assert_eq!(
            report,
            SaveReport {
                published: true,
                continuation: Some(Continuation::Quit)
            }
        );
        assert!(!sessions.dirty());
        assert_eq!(sessions.title(), "kept.fcad — FerriteCAD");
        assert_eq!(height_of(&kept), 25.0);
        assert_eq!(sessions.logical_path(), Some(kept.as_path()));
    }

    #[test]
    fn replacing_the_document_asks_only_when_something_would_be_lost() {
        let f = fixture();
        let mut sessions = open(&f);
        for choice in [
            None,
            Some(UnsavedChoice::Cancel),
            Some(UnsavedChoice::Discard),
        ] {
            assert_eq!(sessions.replacing(choice), Replace::Go, "{choice:?}: clean");
        }
        accept(&mut sessions, f.feature, 25.0);
        assert_eq!(sessions.replacing(None), Replace::Stay, "could not ask");
        assert_eq!(
            sessions.replacing(Some(UnsavedChoice::Cancel)),
            Replace::Stay
        );
        assert_eq!(
            sessions.replacing(Some(UnsavedChoice::Discard)),
            Replace::Discarded
        );
        assert_eq!(
            sessions.replacing(Some(UnsavedChoice::Save)),
            Replace::AfterSave
        );
        // Discard is a permission, not an action: the changes are still there until
        // the replacement is accepted, so a cancelled file dialog loses nothing.
        assert!(sessions.dirty() && sessions.can_undo());
    }

    #[test]
    fn the_name_is_the_users_never_the_private_file() {
        let f = fixture();
        let mut sessions = open(&f);
        accept(&mut sessions, f.feature, 25.0);
        let shown = [sessions.title(), sessions.status.clone()].join(" ");
        assert!(shown.contains("plate.fcad"));
        assert!(!shown.contains("ferritecad-session"), "{shown}");
        assert!(!shown.contains("v1.fcad"), "{shown}");
        assert_eq!(sessions.logical_path(), Some(f.file.as_path()));
    }

    #[test]
    fn stopping_joins_the_worker_and_removes_every_private_file() {
        let f = fixture();
        let mut sessions = open(&f);
        let private = sessions
            .session
            .as_ref()
            .expect("session")
            .private_directory()
            .to_path_buf();
        let (_, edited) = apply(&mut sessions, f.feature, 25.0);
        assert!(matches!(edited, Edited::Show(_)));
        assert!(private.exists());
        sessions.stop_all();
        assert!(
            !private.exists(),
            "the private directory outlived the window"
        );
        assert!(!sessions.has_session() && !sessions.busy());
        assert_eq!(height_of(&f.file), 12.0);
    }
}
