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
    /// The private directory this value last told [`shown_as`] about.
    registered: Option<PathBuf>,
    /// Workers of operations that belonged to a session which has since been
    /// replaced, cancelled and waiting to be joined. Their answers change nothing.
    retired: Vec<JoinHandle<()>>,
}

/// Which user's file each private working directory stands for, so that text
/// written for a person names the document they opened, not the file the window
/// reads it from. Only ever consulted to *show* a path; no code reads a document
/// through it.
static SHOWN_AS: Mutex<Vec<(PathBuf, PathBuf)>> = Mutex::new(Vec::new());

/// How to name `path` to the user: the document it is the working copy of, or the
/// path itself when it is not one of ours.
pub(crate) fn shown_as(path: &Path) -> String {
    let known = SHOWN_AS.lock().unwrap_or_else(|e| e.into_inner());
    known
        .iter()
        .find(|(private, _)| path.starts_with(private))
        .map_or_else(
            || path.display().to_string(),
            |(_, logical)| logical.display().to_string(),
        )
}

impl Drop for Sessions {
    fn drop(&mut self) {
        self.stop_all();
    }
}

impl Sessions {
    /// Joins the retired workers that have finished.
    fn reap(&mut self) {
        let (done, running): (Vec<_>, Vec<_>) = std::mem::take(&mut self.retired)
            .into_iter()
            .partition(JoinHandle::is_finished);
        self.retired = running;
        for worker in done {
            let _ = worker.join();
        }
    }

    /// Keeps [`shown_as`] in step with the session: called whenever the session or
    /// its logical path may have changed.
    fn register(&mut self) {
        let mut known = SHOWN_AS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = self.registered.take() {
            known.retain(|(private, _)| *private != old);
        }
        if let Some(session) = &self.session {
            let private = session.private_directory().to_path_buf();
            known.push((private.clone(), session.logical_path().to_path_buf()));
            self.registered = Some(private);
        }
    }

    /// Where a file dialog should start: the folder the user's document is in, never
    /// the working copy's.
    pub(crate) fn suggested_directory(&self) -> PathBuf {
        self.logical_path()
            .and_then(Path::parent)
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    }

    /// The directory that goes away with the session; nothing may be saved in it.
    pub(crate) fn private_directory(&self) -> Option<&Path> {
        self.session
            .as_ref()
            .map(DocumentSession::private_directory)
    }

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

    /// The file an export reads: the current accepted version, which holds unsaved
    /// changes. Never the user's file on disk, which holds only what was last saved
    /// (ADR 0005); a window that exported that would hand over a model other than
    /// the one on screen.
    pub(crate) fn export_source(&self) -> Option<Arc<ferritecad_jobs::Snapshot>> {
        self.session.as_ref().map(DocumentSession::current)
    }

    #[cfg(test)]
    pub(crate) fn session_saved_version(&self) -> Option<ferritecad_document::DocumentVersion> {
        self.session.as_ref().map(DocumentSession::saved_version)
    }

    #[cfg(test)]
    pub(crate) fn export_path(&self) -> Option<PathBuf> {
        self.export_source()
            .map(|snapshot| snapshot.path().to_path_buf())
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
        // Whatever was in flight belonged to the document being replaced: its
        // answer must not be applied to this one (a Save's checkpoint above all).
        // It is cancelled and its worker joined later, not here, so the window
        // is not made to wait.
        if let Some(mut operation) = self.operation.take() {
            operation.cancel.cancel();
            if let Some(worker) = operation.worker.take() {
                self.retired.push(worker);
            }
        }
        self.reap();
        self.session = Some(session);
        self.register();
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
        // A picture that was ready when the user pressed Cancel is not shown: the
        // answer to Cancel is "nothing changed", for Apply, Undo and Redo alike.
        // The staged version stays where it is and `finish_scene` drops it.
        if let Some(op) = &self.operation {
            op.cancel.check()?;
        }
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
                self.register();
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
        for worker in self.retired.drain(..) {
            let _ = worker.join();
        }
        self.after_save = None;
        self.session = None;
        self.register();
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

/// The vertex edit: the reused `edit-sketch-copy` operation on the worker that
/// owns the kernel session. `expected` is the version the form was opened from.
pub(crate) fn spawn_apply_sketch(
    ticket: StepTicket,
    sketch: ObjectId,
    vertices: Vec<ferritecad_document::SketchVertex>,
    expected: ferritecad_document::DocumentVersion,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ProducedStep>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            let context = OperationContext::default().with_cancel(cancel);
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ticket.edit_sketch_vertices(sketch, vertices, expected, &mut kernel, &context)
        },
        deliver,
        || Err(CadError::kernel("the edit worker stopped unexpectedly")),
    )
}

/// The constraint edit: the reused `edit-sketch-constraints-copy` operation on the
/// worker that owns the kernel session. `expected` is the version the form was
/// opened from.
pub(crate) fn spawn_apply_constraints(
    ticket: StepTicket,
    sketch: ObjectId,
    edits: ferritecad_document::SketchConstraintEdits,
    expected: ferritecad_document::DocumentVersion,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ProducedStep>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            let context = OperationContext::default().with_cancel(cancel);
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ticket.edit_sketch_constraints(sketch, edits, expected, &mut kernel, &context)
        },
        deliver,
        || Err(CadError::kernel("the edit worker stopped unexpectedly")),
    )
}

/// The circle edit: the reused `edit-circle` copy operation on the worker that
/// owns the kernel session. `expected` is the version the form was opened from.
pub(crate) fn spawn_apply_circle(
    ticket: StepTicket,
    sketch: ObjectId,
    edit: ferritecad_document::CircleEdit,
    expected: ferritecad_document::DocumentVersion,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ProducedStep>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            let context = OperationContext::default().with_cancel(cancel);
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ticket.edit_circle(sketch, edit, expected, &mut kernel, &context)
        },
        deliver,
        || Err(CadError::kernel("the edit worker stopped unexpectedly")),
    )
}

/// The annulus edit: the reused `edit-annular` copy operation on the worker that
/// owns the kernel session. `expected` is the version the form was opened from.
pub(crate) fn spawn_apply_annulus(
    ticket: StepTicket,
    sketch: ObjectId,
    edit: ferritecad_document::AnnulusEdit,
    expected: ferritecad_document::DocumentVersion,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ProducedStep>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            let context = OperationContext::default().with_cancel(cancel);
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ticket.edit_annulus(sketch, edit, expected, &mut kernel, &context)
        },
        deliver,
        || Err(CadError::kernel("the edit worker stopped unexpectedly")),
    )
}

/// The partial Revolve angle edit, through the existing copy job on the worker
/// owning the kernel. `expected` is the version the form was opened from.
pub(crate) fn spawn_apply_angle(
    ticket: StepTicket,
    feature: ObjectId,
    degrees: f64,
    expected: ferritecad_document::DocumentVersion,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ProducedStep>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            let context = OperationContext::default().with_cancel(cancel);
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ticket.edit_revolve_angle(feature, degrees, expected, &mut kernel, &context)
        },
        deliver,
        || Err(CadError::kernel("the edit worker stopped unexpectedly")),
    )
}

/// The existing circular Cut edit on the kernel-owning worker. No Add route.
pub(crate) fn spawn_apply_cut(
    ticket: StepTicket,
    cut: ObjectId,
    edit: ferritecad_document::CircularCutEdit,
    expected: ferritecad_document::DocumentVersion,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ProducedStep>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            let context = OperationContext::default().with_cancel(cancel);
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ticket.edit_circular_cut(cut, edit, expected, &mut kernel, &context)
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

/// What an Open does on its worker: the file is read once, into a new session's
/// private copy, and the picture is read from that copy — so what is shown and what
/// Save writes are one reading, and the file on disk is not read a second time.
///
/// A failure at any point drops the session, and with it every private file.
pub(crate) fn open_for_view(
    root: &Path,
    path: &Path,
    context: &OperationContext,
) -> Result<(LoadedScene, DocumentSession)> {
    let session = DocumentSession::open_in(root, path, ferritecad_jobs::HistoryLimits::default())?;
    // The kernel is made and dropped inside the worker: an Open CASCADE session
    // belongs to the thread that opened it.
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    let scene = ferritecad_scene::snapshot_of(
        session.current().path(),
        &mut kernel,
        // How this kernel re-reads a STEP file the document stores.
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        context,
    )?;
    Ok((scene, session))
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    mod analytic;
    mod angle;
    mod cut;
    use ferritecad_document::Document;
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
    fn a_picture_that_was_ready_when_cancel_was_pressed_is_not_accepted() {
        // Apply, Undo and Redo each: the scene is built, the user cancels, the
        // answer arrives. The binding must refuse, and everything stays as it was.
        let f = fixture();
        let mut sessions = open(&f);
        accept(&mut sessions, f.feature, 25.0);
        accept(&mut sessions, f.feature, 30.0);
        enum Step {
            Apply,
            Undo,
            Redo,
        }
        for step in [Step::Apply, Step::Undo, Step::Redo] {
            if matches!(step, Step::Redo) {
                // Something to redo: go back first, for real.
                let (generation, _) = sessions.begin_move(true).expect("undo");
                sessions.bind(Bind::Staged).expect("bind");
                assert!(sessions.finish_scene(generation, Ok(())));
            }
            let (dirty, undo, redo) = (sessions.dirty(), sessions.can_undo(), sessions.can_redo());
            let source = sessions.export_path();
            let files = private_files(&sessions);
            let generation = match step {
                Step::Apply => {
                    let (generation, edited) = apply(&mut sessions, f.feature, 41.0);
                    assert!(matches!(edited, Edited::Show(_)), "{edited:?}");
                    generation
                }
                Step::Undo => sessions.begin_move(true).expect("undo").0,
                Step::Redo => sessions.begin_move(false).expect("redo").0,
            };
            assert!(sessions.cancel());
            let refused = sessions.bind(Bind::Staged).expect_err("cancelled");
            assert_eq!(refused.kind(), ErrorKind::Cancellation);
            // The window reports the refusal as a picture that was not shown.
            assert!(sessions.finish_scene(generation, Err(refused)));
            assert_eq!(sessions.status, "Cancelled; nothing was changed.");
            assert!(!sessions.busy());
            assert_eq!(
                (sessions.dirty(), sessions.can_undo(), sessions.can_redo()),
                (dirty, undo, redo)
            );
            assert_eq!(sessions.export_path(), source, "the current version moved");
            assert_eq!(private_files(&sessions), files, "a staged file was kept");
            assert_eq!(height_of(&f.file), 12.0);
        }
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

    fn version_of(path: &Path) -> ferritecad_document::DocumentVersion {
        let document = Document::open_read_only(path).expect("document");
        let version = ferritecad_document::DocumentVersion {
            document_id: document.meta().document_id,
            content: document.content_version().expect("content"),
        };
        document.close().expect("close");
        version
    }

    #[test]
    fn an_old_editor_gets_the_users_folder_and_name_and_its_copy_outlives_the_old_session() {
        let f = fixture();
        let mut sessions = open(&f);
        // Clean, at a checkpoint that is not the first: Save As, edit, Undo.
        accept(&mut sessions, f.feature, 25.0);
        let renamed = f.root.path().join("renamed.fcad");
        let report = save(&mut sessions, SaveTarget::As(renamed.clone()), None).expect("report");
        assert!(report.published);
        accept(&mut sessions, f.feature, 30.0);
        let (generation, _) = sessions.begin_move(true).expect("undo");
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
        assert!(!sessions.dirty(), "back at the saved checkpoint");

        // What the old editors are handed is the accepted snapshot, which is private...
        let accepted = sessions.export_path().expect("accepted snapshot");
        let private = sessions.private_directory().expect("private").to_path_buf();
        assert!(accepted.starts_with(&private));
        assert_ne!(Some(accepted.as_path()), sessions.logical_path());
        // ...and what the person is shown and offered is theirs.
        assert_eq!(shown_as(&accepted), renamed.display().to_string());
        assert_eq!(sessions.suggested_directory(), f.root.path());
        assert!(!sessions.suggested_directory().starts_with(&private));

        // A dialog answer inside the working folder is refused, for everything that
        // writes something to keep, and nothing is written there.
        let mut dialogs = crate::dialogs::Dialogs::default();
        let mut input = ferritecad_ui::ViewportInput::new();
        let inside = private.join("copy.fcad");
        for action in [
            crate::dialogs::Action::Edit,
            crate::dialogs::Action::SaveAs,
            crate::dialogs::Action::ExportFbx,
            crate::dialogs::Action::ExportStl,
            crate::dialogs::Action::New,
        ] {
            assert_eq!(
                dialogs.receive(
                    action,
                    crate::dialogs::Outcome::Selected(inside.clone()),
                    &mut input,
                    Some(&private)
                ),
                None
            );
            assert!(
                dialogs
                    .failure()
                    .expect("said so")
                    .contains("temporary working folder")
            );
        }
        assert!(!inside.exists());
        // Opening reads, it does not keep: not refused.
        assert_eq!(
            dialogs.receive(
                crate::dialogs::Action::Open,
                crate::dialogs::Outcome::Selected(inside.clone()),
                &mut input,
                Some(&private)
            ),
            Some(inside)
        );

        // The folder the dialog offers is accepted as it stands: the copy is made
        // from the accepted snapshot, opened as the new document, and the old
        // session is dropped by that.
        let copy = sessions.suggested_directory().join("copy.fcad");
        assert_eq!(
            dialogs.receive(
                crate::dialogs::Action::Edit,
                crate::dialogs::Outcome::Selected(copy.clone()),
                &mut input,
                Some(&private)
            ),
            Some(copy.clone())
        );
        ferritecad_jobs::edit_extrude_copy(
            &ferritecad_jobs::EditExtrudeRequest {
                source: accepted.clone(),
                expected: version_of(&accepted),
                feature: f.feature,
                distance_mm: 41.0,
                destination: copy.clone(),
            },
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("the copy");
        assert_eq!(height_of(&copy), 41.0);
        sessions.adopt(
            DocumentSession::open_in(f.private.path(), &copy, HistoryLimits::default())
                .expect("the copy opens"),
        );
        assert!(!private.exists(), "the old session's folder is gone");
        assert!(
            copy.is_file(),
            "the published copy was removed with the old session"
        );
        assert_eq!(height_of(&copy), 41.0);
        assert_eq!(sessions.logical_path(), Some(copy.as_path()));
        assert_eq!(shown_as(&accepted), accepted.display().to_string());
        assert_eq!(height_of(&renamed), 25.0);
    }

    #[test]
    fn a_save_still_running_when_another_document_is_adopted_never_moves_its_checkpoint() {
        let f = fixture();
        let mut sessions = open(&f);
        accept(&mut sessions, f.feature, 25.0);
        // A Save of the old document is in flight and does not finish until told to.
        let (release, wait) = mpsc::channel::<()>();
        let (answer, answered) = mpsc::channel();
        let generation = sessions
            .begin_save(SaveTarget::InPlace, None, |plan, _, _| {
                std::thread::spawn(move || {
                    wait.recv().expect("released");
                    answer
                        .send(plan.run(&OperationContext::default()))
                        .expect("deliver");
                })
            })
            .expect("started");

        // Meanwhile the Open of another document is accepted.
        let other = f.root.path().join("other.fcad");
        std::fs::copy(&f.file, &other).expect("copy");
        let mut second =
            DocumentSession::open_in(f.private.path(), &other, HistoryLimits::default())
                .expect("the other document");
        let before = second.saved_version();
        sessions.adopt(std::mem::replace(
            &mut second,
            DocumentSession::open_in(f.private.path(), &other, HistoryLimits::default())
                .expect("spare"),
        ));
        drop(second);
        assert_eq!(sessions.logical_path(), Some(other.as_path()));
        assert!(!sessions.dirty());

        // The old Save finishes and publishes the old document, as the user asked...
        release.send(()).expect("go");
        let result = answered.recv().expect("answer");
        assert!(
            result.is_ok(),
            "the old document's Save was the user's to ask for"
        );
        // ...and its answer is not applied to the document now open.
        assert_eq!(sessions.finish_save(generation, result), None);
        assert!(!sessions.dirty(), "the new document's checkpoint moved");
        assert_eq!(sessions.logical_path(), Some(other.as_path()));
        assert_eq!(sessions.session_saved_version(), Some(before));
        assert!(!sessions.busy());
        assert!(sessions.status.is_empty(), "{:?}", sessions.status);
        // And the new document can still be edited and saved on its own terms.
        accept(&mut sessions, f.feature, 31.0);
        assert!(sessions.dirty());
        let report = save(&mut sessions, SaveTarget::InPlace, None).expect("report");
        assert!(report.published);
        assert!(!sessions.dirty());
        assert_eq!(height_of(&other), 31.0);
        assert_eq!(height_of(&f.file), 25.0);
    }

    #[test]
    fn an_export_keeps_its_working_copy_alive_without_blocking_the_window() {
        let f = fixture();
        let mut sessions = open(&f);
        accept(&mut sessions, f.feature, 25.0);
        let lease = sessions.export_source().expect("accepted snapshot");
        let working = lease.path().to_path_buf();
        let private = working.parent().expect("folder").to_path_buf();

        // A running export holds its own reference...
        let (release, wait) = mpsc::channel::<()>();
        let (read, saw) = mpsc::channel();
        let mine = Arc::clone(&lease);
        let worker = std::thread::spawn(move || {
            wait.recv().expect("released");
            // Read after the window has moved on.
            let document = Document::open_read_only(mine.path()).expect("still there");
            read.send(document.meta().document_id).expect("deliver");
            document.close().expect("close");
        });
        // ...and so does a question waiting to be answered.
        let mut exports = crate::exports::Exports::default();
        let mut input = ferritecad_ui::ViewportInput::new();
        let occupied = f.root.path().join("occupied.fbx");
        std::fs::write(&occupied, b"theirs").expect("a file");
        exports.hold(Some(Arc::clone(&lease)));
        let alias = f.file.clone();
        assert!(
            crate::exports::begin_export(
                &mut exports,
                &mut input,
                Some(&alias),
                Some(occupied.clone()),
                |_, _, _, _| unreachable!("a question is asked first"),
            )
            .is_none()
        );
        assert_eq!(exports.pending(), Some(occupied.as_path()));
        drop(lease);

        // The window opens another document: the old session is dropped, at once.
        let other = f.root.path().join("other.fcad");
        std::fs::copy(&f.file, &other).expect("copy");
        let started = std::time::Instant::now();
        sessions.adopt(
            DocumentSession::open_in(f.private.path(), &other, HistoryLimits::default())
                .expect("another document"),
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert!(
            working.is_file(),
            "the export's working copy was deleted under it"
        );
        assert!(exports.held().is_some_and(|held| held.path() == working));

        release.send(()).expect("go");
        saw.recv().expect("the export read its snapshot");
        worker.join().expect("worker");
        assert!(working.is_file(), "the question still holds it");
        // Leaving the document abandons the question; with the last holder gone the
        // folder goes too.
        crate::exports::leave_document(&mut exports, &mut input);
        assert!(exports.pending().is_none() && exports.held().is_none());
        assert!(!working.exists());
        assert!(!private.exists());
        assert_eq!(std::fs::read(&occupied).expect("untouched"), b"theirs");
    }

    #[test]
    fn a_public_export_into_the_working_folder_is_refused_before_anything_is_written() {
        let f = fixture();
        let mut sessions = open(&f);
        accept(&mut sessions, f.feature, 25.0);
        let lease = sessions.export_source().expect("accepted snapshot");
        let private = lease.path().parent().expect("folder").to_path_buf();
        let before = std::fs::read_dir(&private).expect("dir").count();
        let mut exports = crate::exports::Exports::default();
        let mut input = ferritecad_ui::ViewportInput::new();
        for inside in [
            private.join("out.fbx"),
            private.join("deeper").join("out.stl"),
        ] {
            assert!(crate::exports::inside_working_copy(&lease, &inside));
            crate::exports::refuse_working_folder(&mut exports, &mut input, &inside);
            assert!(matches!(
                exports.status(),
                crate::exports::ExportStatus::Failed { message, .. }
                    if message.contains("temporary working folder")
            ));
        }
        assert!(!crate::exports::inside_working_copy(
            &lease,
            &f.root.path().join("out.fbx")
        ));
        assert_eq!(std::fs::read_dir(&private).expect("dir").count(), before);
        // The STL path refuses the same way, on the intent it was asked with.
        let intent = crate::exports::StlIntent {
            document: lease.path().to_path_buf(),
            alias: f.file.clone(),
            body: ObjectId::new(),
            params: TessellationParams::default(),
            lease: Some(Arc::clone(&lease)),
        };
        let started = crate::exports::begin_stl_export(
            &mut exports,
            &mut input,
            &intent,
            Some(private.join("out.stl")),
            |_, _, _, _| unreachable!("refused before any worker"),
        );
        assert!(started.is_none());
        assert!(!private.join("out.stl").exists());
    }

    /// What the saved Sketch form asks for when the right-hand vertices of the
    /// plate it was opened on move `by` millimetres.
    fn vertex_request(path: &Path, by: f64) -> ferritecad_jobs::EditSketchRequest {
        let reading = read_extrude_source(path).expect("reading");
        let choice = reading
            .sketches
            .iter()
            .find(|s| s.refusal.is_none())
            .expect("an editable Sketch");
        let vertices = choice.vertices.clone().expect("vertices");
        let right = vertices
            .iter()
            .map(|v| v.start_mm[0])
            .fold(f64::MIN, f64::max);
        ferritecad_jobs::EditSketchRequest {
            source: path.to_path_buf(),
            expected: reading.version,
            sketch: choice.sketch,
            vertices: vertices
                .into_iter()
                .map(|mut v| {
                    if v.start_mm[0] == right {
                        v.start_mm[0] += by;
                    }
                    v
                })
                .collect(),
            destination: PathBuf::new(),
        }
    }

    fn width_of(path: &Path) -> f64 {
        let reading = read_extrude_source(path).expect("reading");
        let vertices = reading.sketches[0].vertices.clone().expect("vertices");
        let xs = vertices.iter().map(|v| v.start_mm[0]);
        xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
    }

    /// Apply vertices as the window runs it, on a thread, with the mock kernel.
    fn apply_vertices(
        sessions: &mut Sessions,
        request: ferritecad_jobs::EditSketchRequest,
    ) -> (u64, Edited) {
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, _| {
                std::thread::spawn(move || {
                    tx.send(ticket.edit_sketch_vertices(
                        request.sketch,
                        request.vertices,
                        request.expected,
                        &mut MockKernel::new(),
                        &OperationContext::default(),
                    ))
                    .expect("deliver");
                })
            })
            .expect("started");
        let result = rx.recv().expect("answer");
        (generation, sessions.finish_apply(generation, result))
    }

    fn accept_vertices(sessions: &mut Sessions, request: ferritecad_jobs::EditSketchRequest) {
        let (generation, edited) = apply_vertices(sessions, request);
        assert!(matches!(edited, Edited::Show(_)), "{edited:?}");
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
    }

    #[test]
    fn a_vertex_apply_is_one_document_step_and_a_stale_or_unchanged_form_is_not() {
        let f = fixture();
        let mut sessions = open(&f);
        let on_disk = std::fs::read(&f.file).expect("file");
        let width = width_of(&f.file);

        accept(&mut sessions, f.feature, 25.0);
        let after_height = sessions.export_path().expect("accepted");
        // A form opened now, and one opened on a version that is about to be replaced.
        let request = vertex_request(&after_height, 5.25);
        let stale = request.clone();
        accept_vertices(&mut sessions, request);
        assert!(sessions.dirty());
        assert_eq!(
            width_of(&sessions.export_path().expect("accepted")),
            width + 5.25
        );
        assert_eq!(
            std::fs::read(&f.file).expect("file"),
            on_disk,
            "Apply wrote the file"
        );
        assert!(sessions.status.starts_with("Applied"));

        // The form that was opened before is about a picture that is gone: refused,
        // and nothing moved, nothing was left behind.
        let files = private_files(&sessions);
        let (_, edited) = apply_vertices(&mut sessions, stale);
        assert_eq!(edited, Edited::Failed);
        assert!(
            sessions.status.contains("changed after this form"),
            "{}",
            sessions.status
        );
        assert_eq!(private_files(&sessions), files);
        assert!(sessions.dirty() && sessions.can_undo() && !sessions.can_redo());
        assert!(!sessions.busy());

        // Moving the vertices to where they are is not a step, and Redo survives it.
        move_back(&mut sessions);
        assert!(sessions.can_redo());
        let current = sessions.export_path().expect("accepted");
        let (_, edited) = apply_vertices(&mut sessions, vertex_request(&current, 0.0));
        assert_eq!(edited, Edited::NoChange);
        assert!(sessions.can_redo(), "a no-op dropped the Redo");

        // A new edit from here is a new branch: the old future is gone.
        accept_vertices(&mut sessions, vertex_request(&current, 1.5));
        assert!(!sessions.can_redo());
        assert_eq!(
            width_of(&sessions.export_path().expect("accepted")),
            width + 1.5
        );

        // The user's file changed under us before Save: nothing is overwritten.
        let theirs = f.root.path().join("theirs.fcad");
        std::fs::copy(&f.file, &theirs).expect("copy");
        let mut document = Document::open(&theirs).expect("open");
        let prepared =
            ferritecad_document::prepare_extrude_height(&document, f.feature, 99.0).expect("p");
        document.write_extrude_height(&prepared).expect("written");
        document.close().expect("closed");
        std::fs::rename(&theirs, &f.file).expect("lands");
        let report = save(&mut sessions, SaveTarget::InPlace, None).expect("answered");
        assert!(!report.published);
        assert!(sessions.dirty(), "a refused Save moved the checkpoint");
        assert_eq!(height_of(&f.file), 99.0);
    }

    fn move_back(sessions: &mut Sessions) {
        let (generation, _) = sessions.begin_move(true).expect("a step back");
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
    }

    #[test]
    fn a_vertex_apply_cancelled_after_it_was_computed_is_not_accepted() {
        let f = fixture();
        let mut sessions = open(&f);
        let width = width_of(&f.file);
        let request = vertex_request(&f.file, 5.25);
        let before = private_files(&sessions);
        let (generation, edited) = apply_vertices(&mut sessions, request);
        assert!(matches!(edited, Edited::Show(_)));
        // The picture is ready, the user presses Cancel, then the answer arrives.
        assert!(sessions.cancel());
        let refused = sessions.bind(Bind::Staged).expect_err("cancelled");
        assert_eq!(refused.kind(), ErrorKind::Cancellation);
        assert!(sessions.finish_scene(generation, Err(refused)));
        assert!(!sessions.dirty() && !sessions.can_undo() && !sessions.busy());
        assert_eq!(private_files(&sessions), before, "the staged file was kept");
        assert_eq!(width_of(&sessions.export_path().expect("accepted")), width);
        // And a picture that cannot be shown (the device refuses it) is the same.
        let (generation, edited) = apply_vertices(&mut sessions, vertex_request(&f.file, 5.25));
        assert!(matches!(edited, Edited::Show(_)));
        assert!(sessions.finish_scene(generation, Err(CadError::rendering("no device"))));
        assert!(!sessions.dirty() && !sessions.can_undo());
        assert_eq!(private_files(&sessions), before);
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

    // ---- the real route: OCCT, the real worker functions and the peer CLI ----

    fn native() -> bool {
        if ferritecad_occt::is_available() {
            return true;
        }
        assert_ne!(
            std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(),
            Ok("1"),
            "this build is required to have Open CASCADE"
        );
        eprintln!("skipped: the session gates need Open CASCADE");
        false
    }

    fn cli(arguments: &[&std::ffi::OsStr]) -> std::process::Output {
        let output = std::process::Command::new(crate::creates::tests::ferritecad())
            .args(arguments)
            .output()
            .expect("the peer command line");
        assert!(output.status.success(), "{output:?}");
        output
    }

    /// Every cell of every table, with the one stamp every writer refreshes set
    /// aside: what two documents have to agree on to be the same model.
    fn cells(path: &Path) -> std::collections::BTreeMap<String, Vec<String>> {
        cells_with_sketch_payload(path, None)
    }

    // Only the selected payload and its derived hash may differ after an explicit
    // mapping of newly created constraint UUIDs. Keep every other SQL cell.
    fn cells_with_sketch_payload(
        path: &Path,
        normalized: Option<(ObjectId, &[u8])>,
    ) -> std::collections::BTreeMap<String, Vec<String>> {
        let db =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .expect("open");
        let names: Vec<String> = db
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .expect("tables")
            .query_map([], |row| row.get(0))
            .expect("names")
            .collect::<std::result::Result<_, _>>()
            .expect("names");
        let mut all = std::collections::BTreeMap::new();
        for table in names {
            let mut statement = db
                .prepare(&format!("SELECT * FROM \"{table}\""))
                .expect("select");
            let columns: Vec<String> = statement
                .column_names()
                .into_iter()
                .map(str::to_owned)
                .collect();
            let mut rows: Vec<String> = statement
                .query_map([], |row| {
                    let mut line = String::new();
                    for (index, column) in columns.iter().enumerate() {
                        if table == "meta" && column == "modified_at" {
                            continue;
                        }
                        if table == "objects"
                            && let Some((id, payload)) = normalized
                            && row.get_ref(0)?.as_blob()? == id.to_bytes()
                        {
                            if column == "payload_hash" {
                                let hash = ferritecad_types::ContentHash::of_bytes(payload);
                                line.push_str(&format!(
                                    "{column}={:?};",
                                    rusqlite::types::ValueRef::Blob(hash.as_bytes())
                                ));
                                continue;
                            }
                            if column == "payload" {
                                line.push_str(&format!(
                                    "{column}={:?};",
                                    rusqlite::types::ValueRef::Blob(payload)
                                ));
                                continue;
                            }
                        }
                        line.push_str(&format!("{column}={:?};", row.get_ref(index)?));
                    }
                    Ok(line)
                })
                .expect("rows")
                .collect::<std::result::Result<_, _>>()
                .expect("rows");
            rows.sort();
            all.insert(table, rows);
        }
        all
    }

    /// The window's own Apply, on the real workers: the edit on the kernel's thread,
    /// then the picture of the new version, then the version becoming current.
    fn apply_native(sessions: &mut Sessions, feature: ObjectId, millimetres: f64) {
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply(
                    ticket,
                    feature,
                    millimetres,
                    cancel.clone(),
                    move |result| tx.send(result).expect("deliver"),
                )
            })
            .expect("started");
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("edit");
        let Edited::Show(path) = sessions.finish_apply(generation, result) else {
            panic!("the edit was not shown: {}", sessions.status);
        };
        let (tx, rx) = mpsc::channel();
        let token = sessions.scene_token(generation).expect("token");
        let worker = spawn_scene(path, token, move |scene| tx.send(scene).expect("deliver"));
        assert!(sessions.attach_scene(generation, worker));
        let scene = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("scene");
        assert!(
            !scene
                .expect("the picture of the new version")
                .catalogue
                .is_empty()
        );
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
    }

    fn move_native(sessions: &mut Sessions, undo: bool) {
        let (generation, path) = sessions.begin_move(undo).expect("a step");
        let (tx, rx) = mpsc::channel();
        let token = sessions.scene_token(generation).expect("token");
        let worker = spawn_scene(path, token, move |scene| tx.send(scene).expect("deliver"));
        assert!(sessions.attach_scene(generation, worker));
        rx.recv_timeout(std::time::Duration::from_secs(120))
            .expect("scene")
            .expect("the picture");
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
    }

    /// What the window exports, on the real export workers, from `document`; and
    /// what the command line exports from `peer`.
    fn export_bytes(document: &Path, alias: &Path, root: &Path, tag: &str) -> (Vec<u8>, Vec<u8>) {
        let fbx = root.join(format!("{tag}.fbx"));
        crate::exports::run_export(document, &fbx, false, &OperationContext::default())
            .expect("the window's FBX");
        let reading = Document::open_read_only(document).expect("document");
        let body = ferritecad_jobs::stl_bodies(&reading).expect("bodies")[0].id;
        reading.close().expect("close");
        let stl = root.join(format!("{tag}.stl"));
        crate::exports::run_stl_export(
            &crate::exports::StlIntent {
                document: document.to_path_buf(),
                alias: alias.to_path_buf(),
                body,
                params: TessellationParams::default(),
                lease: None,
            },
            &stl,
            false,
            &OperationContext::default(),
        )
        .expect("the window's STL");
        (
            std::fs::read(stl).expect("stl"),
            std::fs::read(fbx).expect("fbx"),
        )
    }

    fn peer_bytes(document: &Path, root: &Path, tag: &str) -> (Vec<u8>, Vec<u8>) {
        let stl = root.join(format!("{tag}-peer.stl"));
        let fbx = root.join(format!("{tag}-peer.fbx"));
        cli(&[
            "export-stl".as_ref(),
            document.as_os_str(),
            "-o".as_ref(),
            stl.as_os_str(),
        ]);
        cli(&[
            "export-fbx".as_ref(),
            document.as_os_str(),
            "-o".as_ref(),
            fbx.as_os_str(),
        ]);
        (
            std::fs::read(stl).expect("stl"),
            std::fs::read(fbx).expect("fbx"),
        )
    }

    /// Open, Apply, Undo, Redo, export while unsaved and Save on `source`, each
    /// against the command line doing the same to the same document.
    fn native_gate(root: &Path, source: &Path, feature: ObjectId, height: f64) {
        let original = std::fs::read(source).expect("source");
        let version = ferritecad_jobs::read_extrude_source(source)
            .expect("reading")
            .version;
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = Sessions::default();
        sessions.adopt(
            DocumentSession::open_in(private.path(), source, HistoryLimits::default())
                .expect("session"),
        );
        let name = source
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        assert_eq!(sessions.title(), format!("{name} — {PRODUCT_NAME}"));

        // The same change through the command line, which is what the window must
        // reach: the old `edit-extrude`, unchanged.
        let peer = root.join("cli-edit.fcad");
        cli(&[
            "edit-extrude".as_ref(),
            source.as_os_str(),
            "--feature".as_ref(),
            feature.to_string().as_ref(),
            "--expect-version".as_ref(),
            version.content.to_string().as_ref(),
            "--distance-mm".as_ref(),
            height.to_string().as_ref(),
            "-o".as_ref(),
            peer.as_os_str(),
        ]);
        let (original_stl, original_fbx) = peer_bytes(source, root, "original");
        let (peer_stl, peer_fbx) = peer_bytes(&peer, root, "edited");
        assert_ne!(original_stl, peer_stl, "the edit must change the part");

        // Apply: accepted into the session, the file on disk untouched.
        apply_native(&mut sessions, feature, height);
        assert!(sessions.dirty());
        assert_eq!(sessions.title(), format!("*{name} — {PRODUCT_NAME}"));
        assert_eq!(
            std::fs::read(source).expect("source"),
            original,
            "Apply wrote the file"
        );

        // Export while unsaved: the working model, not the file on disk, and the
        // same bytes the command line exports from its copy.
        let alias = sessions.logical_path().expect("logical").to_path_buf();
        let working = sessions.export_path().expect("an open document exports");
        assert_ne!(Some(working.as_path()), sessions.logical_path());
        let (stl, fbx) = export_bytes(&working, &alias, root, "unsaved");
        assert_eq!(stl, peer_stl, "the unsaved STL is not the edited model");
        assert_eq!(fbx, peer_fbx, "the unsaved FBX is not the edited model");
        assert_ne!(stl, original_stl, "the export read the old file");

        // Undo is the saved model again, and exports as the original does; Redo is
        // the edit again.
        move_native(&mut sessions, true);
        assert!(!sessions.dirty());
        let working = sessions.export_path().expect("an open document exports");
        let (stl, fbx) = export_bytes(&working, &alias, root, "undone");
        assert_eq!(
            (stl, fbx),
            (original_stl, original_fbx),
            "Undo did not restore the model"
        );
        move_native(&mut sessions, false);
        assert!(sessions.dirty());
        assert_eq!(
            std::fs::read(source).expect("source"),
            original,
            "Undo/Redo wrote the file"
        );

        // Save: the file is now the command line's copy, cell for cell, and every
        // saved name still resolves after a cold reopen.
        let report = {
            let (tx, rx) = mpsc::channel();
            let generation = sessions
                .begin_save(SaveTarget::InPlace, None, |plan, _, cancel| {
                    spawn_save(plan, cancel.clone(), move |result| {
                        tx.send(result).expect("deliver")
                    })
                })
                .expect("started");
            sessions
                .finish_save(generation, rx.recv().expect("answer"))
                .expect("answered")
        };
        assert!(report.published && !sessions.dirty());
        assert_eq!(
            cells(source),
            cells(&peer),
            "Save is not the command line's copy"
        );
        let saved = Document::open_read_only(source).expect("saved");
        let refs = saved.topology_refs().expect("refs").len();
        assert!(refs > 0, "the plate carries saved names");
        let built = ferritecad_eval::rebuild_cold(
            &saved,
            &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
            &OperationContext::default(),
        )
        .expect("cold rebuild");
        for reference in saved.topology_refs().expect("refs") {
            assert!(
                built
                    .resolve(&reference)
                    .is_ok_and(|found| !found.is_empty()),
                "{} did not resolve after Save",
                reference.id
            );
        }
        // And the file holds exactly what is shown.
        let (stl, fbx) = peer_bytes(source, root, "saved");
        assert_eq!((stl, fbx), (peer_stl, peer_fbx));
        let _ = sessions;
    }

    fn apply_sketch_native(sessions: &mut Sessions, request: ferritecad_jobs::EditSketchRequest) {
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply_sketch(
                    ticket,
                    request.sketch,
                    request.vertices,
                    request.expected,
                    cancel.clone(),
                    move |result| tx.send(result).expect("deliver"),
                )
            })
            .expect("started");
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("edit");
        let Edited::Show(path) = sessions.finish_apply(generation, result) else {
            panic!("the edit was not shown: {}", sessions.status);
        };
        let (tx, rx) = mpsc::channel();
        let token = sessions.scene_token(generation).expect("token");
        let worker = spawn_scene(path, token, move |scene| tx.send(scene).expect("deliver"));
        assert!(sessions.attach_scene(generation, worker));
        rx.recv_timeout(std::time::Duration::from_secs(120))
            .expect("scene")
            .expect("the picture of the new version");
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
    }

    fn sketch_peer(
        source: &Path,
        request: &ferritecad_jobs::EditSketchRequest,
        root: &Path,
        out: &Path,
    ) {
        let body = request
            .vertices
            .iter()
            .map(|v| {
                format!(
                    r#"{{"curve_id":"{}","start_mm":[{},{}]}}"#,
                    v.curve_id, v.start_mm[0], v.start_mm[1]
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let file = root.join("vertices.json");
        std::fs::write(
            &file,
            format!(r#"{{"request_version":1,"vertices":[{body}]}}"#),
        )
        .expect("request");
        let version = ferritecad_jobs::read_extrude_source(source)
            .expect("reading")
            .version;
        cli(&[
            "edit-sketch-copy".as_ref(),
            source.as_os_str(),
            "--sketch".as_ref(),
            request.sketch.to_string().as_ref(),
            "--expect-version".as_ref(),
            version.content.to_string().as_ref(),
            "--request".as_ref(),
            file.as_os_str(),
            "-o".as_ref(),
            out.as_os_str(),
        ]);
    }

    /// Height, then vertices, Undo, Redo, an unsaved export, Save and a cold reopen
    /// on `source`, each against the command line doing the same to the same
    /// document; then a new branch after Undo.
    fn native_vertex_gate(root: &Path, source: &Path, from: &str, to: &str, height: f64) {
        let original = std::fs::read(source).expect("source");
        let opened = ferritecad_jobs::read_extrude_source(source).expect("reading");
        let feature = opened.features[0].feature;
        let sketch = opened
            .sketches
            .iter()
            .find(|s| s.refusal.is_none())
            .expect("an editable Sketch")
            .sketch;
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = Sessions::default();
        sessions.adopt(
            DocumentSession::open_in(private.path(), source, HistoryLimits::default())
                .expect("session"),
        );
        let alias = sessions.logical_path().expect("logical").to_path_buf();

        // The window: height, then the vertices through the form's own widgets.
        apply_native(&mut sessions, feature, height);
        let after_height = sessions.export_path().expect("accepted");
        let reading = ferritecad_jobs::read_extrude_source(&after_height).expect("reading");
        let (mut form, request) =
            crate::sketch::tests::typed_apply(&after_height, &reading, sketch, from, to);
        assert!(
            request
                .vertices
                .iter()
                .any(|v| v.start_mm.iter().any(|n| n.to_string() == to)),
            "the typed number did not reach the request"
        );
        apply_sketch_native(&mut sessions, request.clone());
        form.finish_session_change();
        assert!(!form.active(), "the form outlived the picture it described");
        assert!(sessions.dirty());
        assert_eq!(
            std::fs::read(source).expect("source"),
            original,
            "Apply wrote the file"
        );

        // The command line, doing the same two things.
        let peer1 = root.join("peer-height.fcad");
        let peer2 = root.join("peer-vertices.fcad");
        cli(&[
            "edit-extrude".as_ref(),
            source.as_os_str(),
            "--feature".as_ref(),
            feature.to_string().as_ref(),
            "--expect-version".as_ref(),
            opened.version.content.to_string().as_ref(),
            "--distance-mm".as_ref(),
            height.to_string().as_ref(),
            "-o".as_ref(),
            peer1.as_os_str(),
        ]);
        sketch_peer(&peer1, &request, root, &peer2);
        let (original_stl, original_fbx) = peer_bytes(source, root, "v-original");
        let (h_stl, h_fbx) = peer_bytes(&peer1, root, "v-height");
        let (v_stl, v_fbx) = peer_bytes(&peer2, root, "v-vertices");
        assert_ne!(h_stl, v_stl, "moving the vertices must change the part");

        // Export while unsaved is the working model.
        let working = sessions.export_path().expect("accepted");
        let (stl, fbx) = export_bytes(&working, &alias, root, "v-unsaved");
        assert_eq!((stl, fbx), (v_stl.clone(), v_fbx.clone()), "unsaved export");

        // Undo is the height step, Undo again the file; Redo comes back.
        move_native(&mut sessions, true);
        let (stl, fbx) = export_bytes(
            &sessions.export_path().expect("accepted"),
            &alias,
            root,
            "v-undo1",
        );
        assert_eq!(
            (stl, fbx),
            (h_stl, h_fbx),
            "Undo did not give the height step"
        );
        move_native(&mut sessions, true);
        assert!(!sessions.dirty());
        let (stl, fbx) = export_bytes(
            &sessions.export_path().expect("accepted"),
            &alias,
            root,
            "v-undo2",
        );
        assert_eq!(
            (stl, fbx),
            (original_stl, original_fbx),
            "Undo did not restore"
        );
        move_native(&mut sessions, false);
        move_native(&mut sessions, false);
        assert!(sessions.dirty() && !sessions.can_redo());
        assert_eq!(std::fs::read(source).expect("source"), original);

        // Save is the command line's second copy, cell for cell, and every saved
        // name resolves on a cold rebuild of the file.
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_save(SaveTarget::InPlace, None, |plan, _, cancel| {
                spawn_save(plan, cancel.clone(), move |result| {
                    tx.send(result).expect("deliver")
                })
            })
            .expect("started");
        let report = sessions
            .finish_save(generation, rx.recv().expect("answer"))
            .expect("answered");
        assert!(report.published && !sessions.dirty());
        assert_eq!(
            cells(source),
            cells(&peer2),
            "Save is not the command line's copy"
        );
        let saved = Document::open_read_only(source).expect("saved");
        let built = ferritecad_eval::rebuild_cold(
            &saved,
            &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
            &OperationContext::default(),
        )
        .expect("cold rebuild");
        for reference in saved.topology_refs().expect("refs") {
            assert!(
                built
                    .resolve(&reference)
                    .is_ok_and(|found| !found.is_empty()),
                "{} did not resolve after Save",
                reference.id
            );
        }
        let (stl, fbx) = peer_bytes(source, root, "v-saved");
        assert_eq!((stl, fbx), (v_stl, v_fbx));

        // A new branch after Undo drops the Redo it replaces; the file is as saved.
        move_native(&mut sessions, true);
        assert!(sessions.can_redo());
        apply_native(&mut sessions, feature, height + 2.0);
        assert!(!sessions.can_redo());
        assert_eq!(cells(source), cells(&peer2), "the branch wrote the file");
    }

    #[test]
    fn native_vertices_of_an_ordinary_offset_polygon_are_applied_like_the_command_line() {
        if !native() {
            return;
        }
        let (root, path, _) = crate::fillets::tests::plate();
        native_vertex_gate(root.path(), &path, "33", "41.25", 9.5);
    }

    #[test]
    fn native_vertices_of_a_rectangle_under_a_chamfer_keep_the_chamfer_like_the_command_line() {
        if !native() {
            return;
        }
        let (root, path, _) = crate::chamfers::tests::chamfered(2.375);
        native_vertex_gate(root.path(), &path, "33", "41.25", 9.5);
    }

    /// What the document refuses it refuses whole: the form keeps its draft, the
    /// session keeps its version, history and checkpoint, and no file is left.
    #[test]
    fn native_a_refused_vertex_edit_changes_nothing_and_a_constrained_plate_offers_none() {
        if !native() {
            return;
        }
        // A rectangle under a Chamfer: a plate the Chamfer no longer fits is refused.
        let (root, path, _) = crate::chamfers::tests::chamfered(2.375);
        let opened = ferritecad_jobs::read_extrude_source(&path).expect("reading");
        let sketch = opened
            .sketches
            .iter()
            .find(|s| s.refusal.is_none())
            .expect("an editable Sketch")
            .sketch;
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = Sessions::default();
        sessions.adopt(
            DocumentSession::open_in(private.path(), &path, HistoryLimits::default())
                .expect("session"),
        );
        apply_native(&mut sessions, opened.features[0].feature, 9.5);
        let accepted = sessions.export_path().expect("accepted");
        let reading = ferritecad_jobs::read_extrude_source(&accepted).expect("reading");

        // Through the form: the shortened plate is refused where it is typed, and the
        // typed number stays; nothing is asked of the session.
        let mut form = crate::sketch::Editor::default();
        form.set_session(true, true, true);
        assert!(form.begin_edit(&accepted, &reading, sketch));
        let ctx = egui::Context::default();
        crate::sketch::tests::replace_in_form(&ctx, &mut form, "33", "-4.4");
        assert!(
            !crate::sketch::tests::offers_apply(&ctx, &mut form),
            "Apply was offered for a draft the Chamfer does not fit"
        );
        assert!(form.take_apply_request().is_none());
        assert!(form.active(), "the refused draft was discarded");

        // Past the form (a request the form would not make): the worker refuses it.
        let mut forged = ferritecad_jobs::EditSketchRequest {
            source: accepted.clone(),
            expected: reading.version,
            sketch,
            vertices: reading.sketches[0].vertices.clone().expect("vertices"),
            destination: PathBuf::new(),
        };
        for vertex in &mut forged.vertices {
            if vertex.start_mm[0] == 33. {
                vertex.start_mm[0] = -4.4;
            }
        }
        let before = (sessions.dirty(), sessions.can_undo(), sessions.can_redo());
        let files = private_files(&sessions);
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply_sketch(
                    ticket,
                    forged.sketch,
                    forged.vertices,
                    forged.expected,
                    cancel.clone(),
                    move |result| tx.send(result).expect("deliver"),
                )
            })
            .expect("started");
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("edit");
        assert_eq!(sessions.finish_apply(generation, result), Edited::Failed);
        assert!(sessions.status.starts_with("Could not apply the change"));
        assert_eq!(
            (sessions.dirty(), sessions.can_undo(), sessions.can_redo()),
            before
        );
        assert_eq!(
            private_files(&sessions),
            files,
            "a refused edit left a file"
        );
        assert_eq!(sessions.export_path().expect("accepted"), accepted);
        drop(root);

        // A dimensioned plate offers no vertex form at all (its Lines are the
        // solver's); the height still applies and the constraints stay.
        if ferritecad_sketch_solver::is_available() {
            let (root, constrained, _) = constrained_chamfer_plate();
            let reading = ferritecad_jobs::read_extrude_source(&constrained).expect("reading");
            let mut form = crate::sketch::Editor::default();
            form.set_session(true, true, false);
            let editable = reading
                .sketches
                .iter()
                .any(|choice| form.begin_edit(&constrained, &reading, choice.sketch));
            assert!(!editable, "a dimensioned plate offered a vertex form");
            assert!(!form.active());
            drop(root);
        }
    }

    #[test]
    fn native_an_ordinary_plate_is_applied_undone_exported_and_saved_like_the_command_line() {
        if !native() {
            return;
        }
        let (root, path, source) = crate::fillets::tests::plate();
        let feature = source.features[0].feature;
        // A file whose name is not ASCII: the title and the dialogs keep it as it is.
        assert!(
            path.file_name()
                .expect("name")
                .to_string_lossy()
                .contains("плита")
        );
        native_gate(root.path(), &path, feature, 21.5);
    }

    /// The free chamfered plate, dimensioned through the command line: every Line
    /// horizontal or vertical, one corner pinned, a width and a depth. Returns the
    /// directory, the constrained file, its Sketch and the Sketch's own row text.
    fn constrained_chamfer_plate() -> (tempfile::TempDir, PathBuf, ObjectId) {
        let (root, path, source) = crate::chamfers::tests::chamfered(2.375);
        let choice = source.constraint_sketches[0].clone();
        let curves = choice.stored.expect("stored").curves;
        let start = |index: usize| match curves[index % curves.len()].geometry {
            ferritecad_document::SketchGeometry::Line { start, .. } => start,
            _ => panic!("a Line"),
        };
        let flat = |i: usize| (start(i).y - start(i + 1).y).abs() < 1e-9;
        let mut additions: Vec<String> = (0..curves.len())
            .map(|i| {
                format!(
                    r#"{{"rule":"{}","curve_id":"{}"}}"#,
                    if flat(i) { "horizontal" } else { "vertical" },
                    curves[i].id
                )
            })
            .collect();
        let first = start(0);
        additions.push(format!(
            r#"{{"rule":"fixed","curve_id":"{}","at":"start","x_mm":{},"y_mm":{}}}"#,
            curves[0].id, first.x, first.y
        ));
        let across = (0..curves.len()).find(|i| flat(*i)).expect("a horizontal");
        let up = (0..curves.len()).find(|i| !flat(*i)).expect("a vertical");
        additions.push(format!(
            r#"{{"rule":"distance","curve_id":"{}","distance_mm":41.125}}"#,
            curves[across].id
        ));
        additions.push(format!(
            r#"{{"rule":"distance","curve_id":"{}","distance_mm":10.5}}"#,
            curves[up].id
        ));
        let request = root.path().join("constraints.json");
        std::fs::write(
            &request,
            format!(
                r#"{{"request_version":1,"remove":[],"add":[{}]}}"#,
                additions.join(",")
            ),
        )
        .expect("request");
        let constrained = root.path().join("constrained.fcad");
        cli(&[
            "edit-sketch-constraints-copy".as_ref(),
            path.as_os_str(),
            "--sketch".as_ref(),
            choice.sketch.to_string().as_ref(),
            "--expect-version".as_ref(),
            source.version.content.to_string().as_ref(),
            "--request".as_ref(),
            request.as_os_str(),
            "-o".as_ref(),
            constrained.as_os_str(),
        ]);
        (root, constrained, choice.sketch)
    }

    #[test]
    fn native_a_chamfered_plate_with_constraints_keeps_them_byte_for_byte() {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            assert_ne!(
                std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
                Ok("1")
            );
            eprintln!("skipped: constraints need PlaneGCS");
            return;
        }
        let (root, constrained, sketch) = constrained_chamfer_plate();
        let reading = ferritecad_jobs::read_extrude_source(&constrained).expect("reading");
        assert_eq!(
            reading.unavailable_reason(),
            None,
            "the height editor accepts it"
        );
        let feature = reading.features[0].feature;
        let sketch_before = cells(&constrained)["objects"]
            .iter()
            .filter(|row| row.contains(&sketch.to_string()) || row.contains("Sketch"))
            .cloned()
            .collect::<Vec<_>>();
        native_gate(root.path(), &constrained, feature, 9.5);
        // The constraints are the Sketch's own row, which a height edit never writes.
        let sketch_after = cells(&constrained)["objects"]
            .iter()
            .filter(|row| row.contains(&sketch.to_string()) || row.contains("Sketch"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            sketch_before, sketch_after,
            "the constraints changed with the height"
        );
    }

    fn apply_constraints_native(
        sessions: &mut Sessions,
        request: &ferritecad_jobs::EditSketchConstraintsRequest,
    ) {
        let (tx, rx) = mpsc::channel();
        let (sketch, edits, expected) = (request.sketch, request.edits.clone(), request.expected);
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply_constraints(
                    ticket,
                    sketch,
                    edits,
                    expected,
                    cancel.clone(),
                    move |result| tx.send(result).expect("deliver"),
                )
            })
            .expect("started");
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("edit");
        let Edited::Show(path) = sessions.finish_apply(generation, result) else {
            panic!("the edit was not shown: {}", sessions.status);
        };
        let (tx, rx) = mpsc::channel();
        let token = sessions.scene_token(generation).expect("token");
        let worker = spawn_scene(path, token, move |scene| tx.send(scene).expect("deliver"));
        assert!(sessions.attach_scene(generation, worker));
        rx.recv_timeout(std::time::Duration::from_secs(120))
            .expect("scene")
            .expect("the picture of the new version");
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
    }

    /// A worker that is refused: the session keeps its version, history and
    /// checkpoint, and no file is left.
    fn refused_constraints_native(
        sessions: &mut Sessions,
        request: &ferritecad_jobs::EditSketchConstraintsRequest,
    ) -> String {
        let before = (sessions.dirty(), sessions.can_undo(), sessions.can_redo());
        let shown = sessions.export_path().expect("accepted");
        let files = private_files(sessions);
        let (tx, rx) = mpsc::channel();
        let (sketch, edits, expected) = (request.sketch, request.edits.clone(), request.expected);
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply_constraints(
                    ticket,
                    sketch,
                    edits,
                    expected,
                    cancel.clone(),
                    move |result| tx.send(result).expect("deliver"),
                )
            })
            .expect("started");
        let result = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("edit");
        assert_eq!(sessions.finish_apply(generation, result), Edited::Failed);
        assert!(sessions.status.starts_with("Could not apply the change"));
        assert_eq!(
            (sessions.dirty(), sessions.can_undo(), sessions.can_redo()),
            before,
            "a refused edit moved the session"
        );
        assert_eq!(private_files(sessions), files, "a refused edit left a file");
        assert_eq!(sessions.export_path().expect("accepted"), shown);
        sessions.status.clone()
    }

    fn constraint_peer(
        source: &Path,
        request: &ferritecad_jobs::EditSketchConstraintsRequest,
        root: &Path,
        out: &Path,
    ) {
        let file = root.join("constraints-request.json");
        std::fs::write(
            &file,
            crate::constraints::tests::session_apply::peer_request(&request.edits),
        )
        .expect("request");
        let version = ferritecad_jobs::read_extrude_source(source)
            .expect("reading")
            .version;
        cli(&[
            "edit-sketch-constraints-copy".as_ref(),
            source.as_os_str(),
            "--sketch".as_ref(),
            request.sketch.to_string().as_ref(),
            "--expect-version".as_ref(),
            version.content.to_string().as_ref(),
            "--request".as_ref(),
            file.as_os_str(),
            "-o".as_ref(),
            out.as_os_str(),
        ]);
    }

    /// Every stored constraint of the document's Sketch, in stored order.
    fn stored_constraints(path: &Path) -> Vec<ferritecad_document::SketchConstraint> {
        ferritecad_jobs::read_extrude_source(path)
            .expect("reading")
            .constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored")
            .constraints
            .clone()
    }

    /// Every SQL cell agrees except the stamp, after substituting only newly
    /// added constraint UUIDs in the payload and recomputing its hash. Rules agree with
    /// exactly the new ones (and nothing else) carrying different UUIDs: each new
    /// UUID of `ours` is paired with the one at the same place in `theirs`.
    fn same_model_with_explicit_new_ids(
        ours: &Path,
        theirs: &Path,
        sketch: ObjectId,
        before: &[ferritecad_types::StableEntityId],
        why: &str,
    ) -> Vec<(
        ferritecad_types::StableEntityId,
        ferritecad_types::StableEntityId,
    )> {
        let (a, b) = (stored_constraints(ours), stored_constraints(theirs));
        assert_eq!(a.len(), b.len(), "{why}: the number of constraints");
        let mut pairs = Vec::new();
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.rule, y.rule, "{why}: a rule differs");
            if before.contains(&x.id) {
                assert_eq!(x.id, y.id, "{why}: an old UUID changed");
            } else {
                assert!(!before.contains(&y.id), "{why}: a new UUID is an old one");
                pairs.push((x.id, y.id));
            }
        }
        let payload = |path: &Path| {
            // Decode the object and validate its raw stored hash before
            // normalizing newly added UUIDs. object() alone only decodes it.
            Document::open_read_only(path)
                .expect("document")
                .object(sketch)
                .expect("object")
                .expect("Sketch");
            let db = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .expect("database");
            let (payload, hash) = db
                .query_row(
                    "SELECT payload, payload_hash FROM objects WHERE id = ?1",
                    [sketch.to_bytes().as_slice()],
                    |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .expect("payload and hash");
            assert_eq!(
                ferritecad_types::ContentHash::of_bytes(&payload)
                    .as_bytes()
                    .as_slice(),
                hash,
                "{why}: the raw stored Sketch payload hash differs",
            );
            payload
        };
        let mut ours_payload = payload(ours);
        let theirs_payload = payload(theirs);
        for (ours_id, theirs_id) in &pairs {
            let encoded: Vec<_> = std::iter::once(0x50).chain(ours_id.to_bytes()).collect();
            let positions: Vec<_> = ours_payload
                .windows(encoded.len())
                .enumerate()
                .filter_map(|(i, part)| (part == encoded).then_some(i))
                .collect();
            assert_eq!(
                positions.len(),
                1,
                "{why}: new constraint UUID must occur exactly once"
            );
            let i = positions[0] + 1;
            ours_payload[i..i + 16].copy_from_slice(&theirs_id.to_bytes());
        }
        assert_eq!(
            cells_with_sketch_payload(ours, Some((sketch, &ours_payload))),
            cells_with_sketch_payload(theirs, Some((sketch, &theirs_payload))),
            "{why}: a SQL cell or another Sketch field differs",
        );
        pairs
    }

    #[test]
    fn constraint_comparison_keeps_the_sketch_row_metadata() {
        let (root, source, reading) = crate::fillets::tests::plate();
        let sketch = reading.constraint_sketches[0].sketch;
        let other = root.path().join("renamed-sketch.fcad");
        std::fs::copy(&source, &other).expect("copy");
        let old: Vec<_> = stored_constraints(&source).iter().map(|c| c.id).collect();
        assert!(same_model_with_explicit_new_ids(&source, &other, sketch, &old, "same").is_empty());
        let db = rusqlite::Connection::open(&other).expect("copy database");
        assert_eq!(
            db.execute(
                "UPDATE objects SET name = 'unexpected rename' WHERE hex(id) = upper(?1)",
                [sketch.to_string().replace('-', "")],
            )
            .expect("rename"),
            1
        );
        drop(db);
        assert!(
            std::panic::catch_unwind(|| {
                same_model_with_explicit_new_ids(&source, &other, sketch, &old, "renamed")
            })
            .is_err(),
            "the comparison discarded the Sketch's metadata"
        );
    }

    /// Height, then the constraint form's own widgets, then Undo and Redo, an
    /// unsaved export, Save and a cold reopen on `source`, each against the
    /// command line doing the same to the same document; then a branch after Undo.
    fn native_constraint_gate(
        root: &Path,
        source: &Path,
        height: f64,
        draft: impl FnOnce(
            &Path,
            &ferritecad_document::ExtrudeEditSource,
            ObjectId,
        ) -> (
            crate::constraints::Editor,
            ferritecad_jobs::EditSketchConstraintsRequest,
        ),
    ) {
        let original = std::fs::read(source).expect("source");
        let opened = ferritecad_jobs::read_extrude_source(source).expect("reading");
        let feature = opened.features[0].feature;
        let sketch = opened.constraint_sketches[0].sketch;
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = Sessions::default();
        sessions.adopt(
            DocumentSession::open_in(private.path(), source, HistoryLimits::default())
                .expect("session"),
        );
        let alias = sessions.logical_path().expect("logical").to_path_buf();

        apply_native(&mut sessions, feature, height);
        let after_height = sessions.export_path().expect("accepted");
        let reading = ferritecad_jobs::read_extrude_source(&after_height).expect("reading");
        let old_ids: Vec<_> = stored_constraints(&after_height)
            .iter()
            .map(|c| c.id)
            .collect();
        let (form, request) = draft(&after_height, &reading, sketch);
        assert_eq!(request.expected, reading.version, "the form's version");
        apply_constraints_native(&mut sessions, &request);
        let mut window_form = crate::sketch::Editor::default();
        window_form.constraints = form;
        assert!(window_form.active());
        window_form.finish_session_change();
        assert!(
            !window_form.active(),
            "the form outlived the picture it described"
        );
        assert!(sessions.dirty());
        assert_eq!(
            std::fs::read(source).expect("source"),
            original,
            "Apply wrote the file"
        );
        let applied = sessions.export_path().expect("accepted");
        let new_ids: Vec<_> = stored_constraints(&applied).iter().map(|c| c.id).collect();
        assert_ne!(new_ids, old_ids, "the constraints did not change");
        for removed in &request.edits.remove {
            assert!(!new_ids.contains(removed), "{removed} was not removed");
        }

        // The command line, doing the same two things.
        let peer1 = root.join("peer-height.fcad");
        let peer2 = root.join("peer-constraints.fcad");
        cli(&[
            "edit-extrude".as_ref(),
            source.as_os_str(),
            "--feature".as_ref(),
            feature.to_string().as_ref(),
            "--expect-version".as_ref(),
            opened.version.content.to_string().as_ref(),
            "--distance-mm".as_ref(),
            height.to_string().as_ref(),
            "-o".as_ref(),
            peer1.as_os_str(),
        ]);
        constraint_peer(&peer1, &request, root, &peer2);
        let (original_stl, original_fbx) = peer_bytes(source, root, "c-original");
        let (h_stl, h_fbx) = peer_bytes(&peer1, root, "c-height");
        let (c_stl, c_fbx) = peer_bytes(&peer2, root, "c-constraints");
        assert_eq!(
            same_model_with_explicit_new_ids(&applied, &peer2, sketch, &old_ids, "Apply").len(),
            request.edits.add.len(),
            "every added constraint pairs with the command line's"
        );

        // Export while unsaved is the working model.
        let (stl, fbx) = export_bytes(&applied, &alias, root, "c-unsaved");
        assert_eq!((stl, fbx), (c_stl.clone(), c_fbx.clone()), "unsaved export");

        // Undo is the height step, Undo again the file; Redo comes back with the
        // very same constraint UUIDs.
        move_native(&mut sessions, true);
        let shown = sessions.export_path().expect("accepted");
        let (stl, fbx) = export_bytes(&shown, &alias, root, "c-undo1");
        assert_eq!((stl, fbx), (h_stl.clone(), h_fbx.clone()), "Undo: height");
        let ids: Vec<_> = stored_constraints(&shown).iter().map(|c| c.id).collect();
        assert_eq!(ids, old_ids, "Undo changed the constraint UUIDs");
        move_native(&mut sessions, true);
        assert!(!sessions.dirty());
        let (stl, fbx) = export_bytes(
            &sessions.export_path().expect("accepted"),
            &alias,
            root,
            "c-undo2",
        );
        assert_eq!(
            (stl, fbx),
            (original_stl.clone(), original_fbx.clone()),
            "Undo did not restore"
        );
        move_native(&mut sessions, false);
        move_native(&mut sessions, false);
        assert!(sessions.dirty() && !sessions.can_redo());
        let ids: Vec<_> = stored_constraints(&sessions.export_path().expect("accepted"))
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, new_ids, "Redo made new constraint UUIDs");
        assert_eq!(std::fs::read(source).expect("source"), original);

        // Save is the command line's second copy, and every saved name resolves
        // on a cold rebuild of the file.
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_save(SaveTarget::InPlace, None, |plan, _, cancel| {
                spawn_save(plan, cancel.clone(), move |result| {
                    tx.send(result).expect("deliver")
                })
            })
            .expect("started");
        let report = sessions
            .finish_save(generation, rx.recv().expect("answer"))
            .expect("answered");
        assert!(report.published && !sessions.dirty());
        same_model_with_explicit_new_ids(source, &peer2, sketch, &old_ids, "Save");
        let saved = Document::open_read_only(source).expect("saved");
        let built = ferritecad_eval::rebuild_cold(
            &saved,
            &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
            &OperationContext::default(),
        )
        .expect("cold rebuild");
        let references = saved.topology_refs().expect("refs");
        assert!(!references.is_empty(), "the plate carries saved names");
        for reference in references {
            assert!(
                built
                    .resolve(&reference)
                    .is_ok_and(|found| !found.is_empty()),
                "{} did not resolve after Save",
                reference.id
            );
        }
        let (stl, fbx) = peer_bytes(source, root, "c-saved");
        assert_eq!((stl, fbx), (c_stl, c_fbx));

        // A refused edit past the form changes nothing, and does not cut off the
        // Redo; a branch that succeeds does.
        move_native(&mut sessions, true);
        assert!(sessions.can_redo());
        let mut forged = request.clone();
        forged.expected =
            ferritecad_jobs::read_extrude_source(&sessions.export_path().expect("accepted"))
                .expect("reading")
                .version;
        forged.edits.remove = vec![ferritecad_types::StableEntityId::new()];
        refused_constraints_native(&mut sessions, &forged);
        assert!(sessions.can_redo(), "a refusal cut off the Redo");
        apply_native(&mut sessions, feature, height + 2.0);
        assert!(!sessions.can_redo());
        assert_eq!(
            same_model_with_explicit_new_ids(source, &peer2, sketch, &old_ids, "branch").len(),
            request.edits.add.len(),
            "the branch wrote the file"
        );
    }

    #[test]
    fn native_replace_length_of_a_dimensioned_plate_under_a_chamfer_is_applied_like_the_command_line()
     {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            assert_ne!(
                std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
                Ok("1")
            );
            eprintln!("skipped: constraints need PlaneGCS");
            return;
        }
        let (root, constrained, _) = constrained_chamfer_plate();
        native_constraint_gate(root.path(), &constrained, 9.5, |path, source, sketch| {
            let stored = source.constraint_sketches[0]
                .stored
                .as_ref()
                .expect("stored");
            let across = stored
                .curves
                .iter()
                .position(|c| {
                    stored_length_of(stored, c.id).is_some_and(|mm| (mm - 41.125).abs() < 1e-9)
                })
                .expect("the width Line");
            crate::constraints::tests::session_apply::typed_replace_length(
                path,
                source,
                sketch,
                across + 1,
                "38.5",
            )
        });
    }

    fn stored_length_of(
        sketch: &ferritecad_document::Sketch,
        curve: ferritecad_types::StableEntityId,
    ) -> Option<f64> {
        sketch.constraints.iter().find_map(|c| match c.rule {
            ferritecad_document::SketchConstraintRule::Distance { a, b, distance }
                if a.curve == curve && b.curve == curve =>
            {
                Some(distance)
            }
            _ => None,
        })
    }

    #[test]
    fn native_radius_and_fixed_centre_of_a_circle_are_applied_like_the_command_line() {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            assert_ne!(
                std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
                Ok("1")
            );
            eprintln!("skipped: constraints need PlaneGCS");
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("cylinder.fcad");
        ferritecad_jobs::create_document_with_kernel(
            ferritecad_jobs::CreateDocumentRequest::new(
                &path,
                ferritecad_jobs::NewDocument::CircleExtrude(
                    ferritecad_document::CircleExtrusion::new([12., -7.], 10., 15.)
                        .expect("a circle"),
                ),
                "test",
            ),
            ferritecad_occt::OcctKernel::new,
            &OperationContext::default(),
        )
        .expect("source");
        native_constraint_gate(root.path(), &path, 9.5, |path, source, sketch| {
            crate::constraints::tests::session_apply::typed_circle(
                path,
                source,
                sketch,
                "6.75",
                ("-3.5", "4.25"),
            )
        });
    }

    /// What the solver or the Chamfer refuses is refused whole: the session keeps
    /// its version, history and checkpoint, no file is left, and the form keeps its
    /// draft because nothing accepted a new version.
    #[test]
    fn native_a_solver_conflict_or_a_chamfer_that_no_longer_fits_changes_nothing() {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            assert_ne!(
                std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
                Ok("1")
            );
            eprintln!("skipped: constraints need PlaneGCS");
            return;
        }
        let (root, constrained, sketch) = constrained_chamfer_plate();
        let opened = ferritecad_jobs::read_extrude_source(&constrained).expect("reading");
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = Sessions::default();
        sessions.adopt(
            DocumentSession::open_in(private.path(), &constrained, HistoryLimits::default())
                .expect("session"),
        );
        apply_native(&mut sessions, opened.features[0].feature, 9.5);
        move_native(&mut sessions, true);
        assert!(sessions.can_redo());
        move_native(&mut sessions, false);
        let accepted = sessions.export_path().expect("accepted");
        let reading = ferritecad_jobs::read_extrude_source(&accepted).expect("reading");
        let stored = reading.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored");
        let width = stored
            .curves
            .iter()
            .position(|c| {
                stored_length_of(stored, c.id).is_some_and(|mm| (mm - 41.125).abs() < 1e-9)
            })
            .expect("the width Line");
        let height_line = stored
            .curves
            .iter()
            .position(|c| stored_length_of(stored, c.id).is_some_and(|mm| (mm - 10.5).abs() < 1e-9))
            .expect("the depth Line");
        let accepted_bytes = std::fs::read(&accepted).expect("accepted bytes");

        // The solver: a horizontal Line parallel to a vertical one. The form cannot
        // know; the worker says so, and the session did not move. (A session with
        // a Redo to lose does not lose it.)
        move_native(&mut sessions, true);
        let accepted = sessions.export_path().expect("accepted");
        let reading = ferritecad_jobs::read_extrude_source(&accepted).expect("reading");
        let (form, request) = crate::constraints::tests::session_apply::typed_parallel(
            &accepted,
            &reading,
            sketch,
            width + 1,
            height_line + 1,
        );
        assert!(sessions.can_redo());
        let said = refused_constraints_native(&mut sessions, &request);
        assert!(!said.is_empty());
        assert!(form.active(), "the refused draft was discarded");
        assert!(sessions.can_redo(), "a refusal cut off the Redo");
        move_native(&mut sessions, false);
        assert_eq!(
            std::fs::read(sessions.export_path().expect("accepted")).expect("bytes"),
            accepted_bytes,
            "a refused edit changed an accepted version"
        );

        // The Chamfer: a width that leaves no room for it is refused where it is
        // typed (the form offers no Apply), and, past the form, by the worker.
        let accepted = sessions.export_path().expect("accepted");
        let reading = ferritecad_jobs::read_extrude_source(&accepted).expect("reading");
        let (form, offered) = crate::constraints::tests::session_apply::replace_length_offers_apply(
            &accepted,
            &reading,
            sketch,
            width + 1,
            "1",
        );
        assert!(form.active(), "the draft was erased");
        let mut forged = ferritecad_jobs::EditSketchConstraintsRequest {
            source: accepted.clone(),
            expected: reading.version,
            sketch,
            edits: form.draft_edits().expect("a draft"),
            destination: PathBuf::new(),
        };
        if offered {
            // The form leaves the judgement to the document: the worker refuses.
            refused_constraints_native(&mut sessions, &forged);
        } else {
            forged.edits = form.draft_edits().expect("a draft");
            refused_constraints_native(&mut sessions, &forged);
        }
        assert!(form.active());
        drop(root);
    }

    /// With a kernel and no solver, an Apply of constraints is refused whole and
    /// the document stays clean; nothing is accepted, nothing is left behind.
    #[test]
    fn without_the_solver_a_constraints_apply_is_refused_and_nothing_changes() {
        if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
            return;
        }
        let (root, path, source) = crate::chamfers::tests::chamfered(2.375);
        let sketch = source.constraint_sketches[0].sketch;
        let curves = source.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored")
            .curves
            .clone();
        let original = std::fs::read(&path).expect("bytes");
        let private = tempfile::tempdir().expect("private");
        let mut sessions = Sessions::default();
        sessions.adopt(
            DocumentSession::open_in(private.path(), &path, HistoryLimits::default())
                .expect("session"),
        );
        let request = ferritecad_jobs::EditSketchConstraintsRequest {
            source: path.clone(),
            expected: source.version,
            sketch,
            edits: ferritecad_document::SketchConstraintEdits {
                remove: Vec::new(),
                add: vec![ferritecad_document::AddSketchConstraint::Line(
                    ferritecad_document::AddLineConstraint::Line {
                        curve: curves[0].id,
                        kind: ferritecad_document::LineConstraintKind::Horizontal,
                    },
                )],
            },
            destination: PathBuf::new(),
        };
        let said = refused_constraints_native(&mut sessions, &request);
        assert!(said.starts_with("Could not apply the change"), "{said}");
        assert!(!sessions.dirty() && !sessions.can_undo());
        assert_eq!(std::fs::read(&path).expect("bytes"), original);
        drop(root);
    }

    /// A build with no Open CASCADE cannot apply anything, and says so without
    /// leaving the document dirty, a file behind or a half-staged step.
    #[test]
    fn without_a_kernel_an_apply_is_refused_and_the_document_stays_clean() {
        if ferritecad_occt::is_available() {
            return;
        }
        let (root, path, source) = crate::fillets::tests::plate();
        let mut sessions = Sessions::default();
        let private = tempfile::tempdir().expect("private");
        sessions.adopt(
            DocumentSession::open_in(private.path(), &path, HistoryLimits::default())
                .expect("a document opens into a session without a kernel"),
        );
        let before = private_files(&sessions);
        let feature = source.features[0].feature;
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply(ticket, feature, 30.0, cancel.clone(), move |result| {
                    tx.send(result).expect("deliver")
                })
            })
            .expect("started");
        let result = rx.recv().expect("answer");
        assert_eq!(
            result.as_ref().expect_err("no kernel").kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(sessions.finish_apply(generation, result), Edited::Failed);
        assert!(!sessions.dirty() && !sessions.busy() && !sessions.can_undo());
        assert!(sessions.status.starts_with("Could not apply the change"));
        assert_eq!(private_files(&sessions), before);
        assert_eq!(std::fs::read_dir(root.path()).expect("dir").count(), 1);
        // Save has nothing to write and Save As still works on the accepted model.
        assert!(!sessions.can_save() && sessions.can_save_as());
    }

    /// With a kernel and no sketch solver a constrained plate cannot be rebuilt, so
    /// an Apply to it is refused the same way: typed, nothing accepted, nothing
    /// published. (The plate's constraints are written by the document's own
    /// preparation, which asks no solver.)
    #[test]
    fn without_the_solver_an_apply_to_a_constrained_plate_is_refused_and_nothing_changes() {
        if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
            return;
        }
        let (root, path, source) = crate::chamfers::tests::chamfered(2.375);
        {
            let mut document = Document::open(&path).expect("writable");
            let sketch = document
                .objects()
                .expect("objects")
                .into_iter()
                .find_map(|o| match o.payload {
                    ferritecad_document::ObjectPayload::Sketch(s) => Some((o.id, s)),
                    _ => None,
                })
                .expect("Sketch");
            let edits = ferritecad_document::SketchConstraintEdits {
                remove: Vec::new(),
                add: vec![ferritecad_document::AddSketchConstraint::Line(
                    ferritecad_document::AddLineConstraint::Line {
                        curve: sketch.1.curves[0].id,
                        kind: ferritecad_document::LineConstraintKind::Vertical,
                    },
                )],
            };
            let prepared =
                ferritecad_document::prepare_sketch_constraints(&document, sketch.0, &edits)
                    .expect("prepared without a solver");
            document
                .write_sketch_constraints(&prepared)
                .expect("written");
            document.close().expect("closed");
        }
        let original = std::fs::read(&path).expect("bytes");
        let feature = source.features[0].feature;
        let mut sessions = Sessions::default();
        let private = tempfile::tempdir().expect("private");
        sessions.adopt(
            DocumentSession::open_in(private.path(), &path, HistoryLimits::default())
                .expect("session"),
        );
        let before = private_files(&sessions);
        let (_, edited) = {
            let (tx, rx) = mpsc::channel();
            let generation = sessions
                .begin_apply(|ticket, _, cancel| {
                    spawn_apply(ticket, feature, 9.5, cancel.clone(), move |result| {
                        tx.send(result).expect("deliver")
                    })
                })
                .expect("started");
            let result = rx.recv().expect("answer");
            let kind = result.as_ref().err().map(CadError::kind);
            assert_eq!(kind, Some(ErrorKind::Unsupported), "{result:?}");
            (generation, sessions.finish_apply(generation, result))
        };
        assert_eq!(edited, Edited::Failed);
        assert!(!sessions.dirty() && !sessions.can_undo());
        assert_eq!(private_files(&sessions), before);
        assert_eq!(std::fs::read(&path).expect("bytes"), original);
        assert_eq!(std::fs::read_dir(root.path()).expect("dir").count(), 1);
    }

    /// Open reads the file once into a session and draws from that copy; a file that
    /// cannot be opened leaves no session directory behind.
    #[test]
    fn native_open_draws_from_the_sessions_own_copy_and_a_failed_open_leaves_nothing() {
        if !native() {
            return;
        }
        let (root, path, _) = crate::fillets::tests::plate();
        let private = tempfile::tempdir().expect("private root");
        let (scene, session) =
            open_for_view(private.path(), &path, &OperationContext::default()).expect("opens");
        assert!(!scene.catalogue.is_empty());
        assert_ne!(
            session.current().path(),
            path,
            "the picture was read from the file"
        );
        assert!(session.current().path().starts_with(private.path()));
        assert!(!session.is_dirty());
        // The same picture as reading the file directly (the model is the model).
        let direct = ferritecad_scene::snapshot_of(
            &path,
            &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
            |kernel, source| kernel.import_step(source),
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("direct");
        assert_eq!(scene.catalogue.len(), direct.catalogue.len());
        drop(session);
        assert_eq!(std::fs::read_dir(private.path()).expect("dir").count(), 0);

        let broken = root.path().join("broken.fcad");
        std::fs::write(&broken, b"not a document").expect("write");
        assert!(open_for_view(private.path(), &broken, &OperationContext::default()).is_err());
        assert!(
            open_for_view(
                private.path(),
                &root.path().join("absent.fcad"),
                &OperationContext::default()
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_dir(private.path()).expect("dir").count(),
            0,
            "a failed Open left a private directory"
        );
    }

    #[test]
    fn a_cancelled_save_changes_nothing_and_says_so() {
        let f = fixture();
        let mut sessions = open(&f);
        accept(&mut sessions, f.feature, 25.0);
        let before = std::fs::read(&f.file).expect("bytes");
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_save(
                SaveTarget::InPlace,
                Some(Continuation::Quit),
                |plan, _, cancel| {
                    // Asked to stop before the file is replaced.
                    cancel.cancel();
                    let cancel = cancel.clone();
                    std::thread::spawn(move || {
                        tx.send(plan.run(&OperationContext::default().with_cancel(cancel)))
                            .expect("deliver");
                    })
                },
            )
            .expect("started");
        let report = sessions
            .finish_save(generation, rx.recv().expect("answer"))
            .expect("answered");
        assert_eq!(
            report,
            SaveReport {
                published: false,
                continuation: None
            },
            "a cancelled Save must not carry on to closing the window"
        );
        assert_eq!(sessions.status, "Save cancelled; the file was not changed.");
        assert!(sessions.dirty());
        assert_eq!(std::fs::read(&f.file).expect("bytes"), before);
    }
}
