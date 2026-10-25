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
//! An Apply has two phases that share a generation: a worker makes the version
//! and its picture, then the window prepares that picture for display. The version
//! becomes current only when the picture is prepared, in the same statement that
//! replaces the shown one ([`Sessions::commit_staged`], called from the window's
//! `show`). Anything that goes wrong before that — a failed edit, a stale answer,
//! a cancellation, a picture that cannot be uploaded — drops the produced version
//! and changes nothing else.
//!
//! # One value per tab (§30O)
//!
//! A window has one `Sessions` per open document tab, each with its own [`TabId`].
//! Every answer a worker sends back is labelled with an [`Address`] — the tab and
//! the generation — so an answer meant for another tab, for a tab that was closed or
//! for an older operation of this tab changes nothing and releases nothing.

mod presentation;
#[cfg(test)]
pub(crate) mod test_open;

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
use ferritecad_types::{CadError, ErrorKind, Result};

use crate::PRODUCT_NAME;
use crate::tabs::TabId;

/// Which operation of which tab a worker's answer belongs to (§30O). Monotonic
/// generations within a tab, and tab ids that are never reused, so no two
/// operations of one window ever share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Address {
    pub(crate) tab: TabId,
    pub(crate) generation: u64,
}

/// Tests name "a later generation of the same tab" as `address + n`.
#[cfg(test)]
impl std::ops::Add<u64> for Address {
    type Output = Self;
    fn add(self, later: u64) -> Self {
        Self {
            generation: self.generation + later,
            ..self
        }
    }
}

/// What the user asked for on the way to which a Save was offered. Since §30O
/// Open, New and Recover add a tab and ask nothing; only closing does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Continuation {
    /// Close the window (the Quit pass goes on to the next unsaved tab).
    Quit(crate::tabs::QuitId),
    /// Close this tab.
    Close,
    /// §30U: continue only this confirmed form Close attempt.
    CloseForm(crate::tabs::FormCloseId),
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
    /// §30N: a checkpoint was added, restored or deleted.
    Checkpoint(CheckpointKind),
}

/// §30N: which change to the checkpoints, for the sentences a person reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckpointKind {
    Create,
    Restore,
    Delete,
}

/// §30N: what a checkpoint worker is asked to do. Ids, never names or rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckpointAction {
    Create(String),
    Restore(ferritecad_types::CheckpointId),
    Delete(ferritecad_types::CheckpointId),
}

impl CheckpointAction {
    fn kind(&self) -> CheckpointKind {
        match self {
            Self::Create(_) => CheckpointKind::Create,
            Self::Restore(_) => CheckpointKind::Restore,
            Self::Delete(_) => CheckpointKind::Delete,
        }
    }
}

/// §30N: a checkpoint worker's answer. `facts` is the new version's kernel-free
/// edit facts, read on the worker exactly when the step keeps the picture (only
/// the catalog changed): the picture stays, and what the forms read follows the
/// new version without a rebuild.
#[derive(Debug)]
pub(crate) struct CheckpointStep {
    pub(crate) step: ProducedStep,
    pub(crate) facts: Option<crate::sketch_facts::Source>,
}

enum Staged {
    Step(ProducedStep),
    Move(Move),
}

/// What an arriving picture is bound to, decided at the moment it replaces the
/// shown one (`Tabs::bind`).
pub(crate) enum Bind {
    /// An Open, New or Recover: the session the picture was read from becomes a
    /// new tab, shown; the tab that was shown stays open, hidden (§30O).
    Open(Box<DocumentSession>),
    /// An Apply, Undo or Redo: the staged version becomes current.
    Staged,
    /// §30O: the hidden tab this switch generation was showing becomes the shown
    /// one; the tab that was shown is hidden.
    Switch(u64),
}

struct Operation {
    generation: u64,
    kind: Kind,
    cancel: CancelToken,
    worker: Option<JoinHandle<()>>,
    staged: Option<Staged>,
    /// §30N: the edit facts of a staged version that keeps the shown picture.
    kept: Option<crate::sketch_facts::Source>,
    ready_scene: Option<LoadedScene>,
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
    /// §30N: this version draws what is shown already (only its checkpoints
    /// differ): accept it with the shown picture ([`Sessions::commit_kept`]).
    Keep(PathBuf),
}

/// What a finished Save means for the window.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SaveReport {
    /// Published: the title and the toolbar change.
    pub(crate) published: bool,
    /// What the user was on their way to, when the Save succeeded.
    pub(crate) continuation: Option<Continuation>,
}

/// What else holds the operation slot: work whose worker belongs to another owner.
/// Cancellation releases the slot at once and makes the eventual answer stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reserved {
    /// Recover (§30M): the recoveries' worker.
    Recover(u64),
    /// Showing another tab (§30O): the tab list's worker builds its picture.
    Switch(u64),
}

#[derive(Default)]
pub(crate) struct Sessions {
    /// Which tab this is (§30O). Made with the value, never reused.
    tab: TabId,
    session: Option<DocumentSession>,
    operation: Option<Operation>,
    reserved: Option<Reserved>,
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
    /// §30M: the worker that keeps the crash copy of the current accepted
    /// version. Told at the one place a version is accepted, saved or replaced;
    /// `None` when there is no recovery folder (and in tests that do not ask).
    recorder: Option<ferritecad_jobs::RecoveryRecorder>,
    /// The person chose how the window ends (Quit after the guard): the record
    /// goes. Without that decision a dirty document's record stays.
    exit_decided: bool,
    /// §30P: the form that came back with this tab was made on another accepted
    /// version than the one shown. Its Apply and Add are refused until it is
    /// closed; its text stays to be read and cancelled.
    stale_draft: bool,
}

/// Which user's file each private working directory stands for, so that text
/// written for a person names the document they opened, not the file the window
/// reads it from. Only ever consulted to *show* a path; no code reads a document
/// through it.
static SHOWN_AS: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

/// How to name `path` to the user: the document it is the working copy of, or the
/// path itself when it is not one of ours.
pub(crate) fn shown_as(path: &Path) -> String {
    let known = SHOWN_AS.lock().unwrap_or_else(|e| e.into_inner());
    known
        .iter()
        .find(|(private, _)| path.starts_with(private))
        .map_or_else(|| path.display().to_string(), |(_, shown)| shown.clone())
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

    /// §30M: crash copies of this window's accepted versions are kept by `recorder`.
    pub(crate) fn keep_recovery(&mut self, recorder: ferritecad_jobs::RecoveryRecorder) {
        self.recorder = Some(recorder);
        self.record();
    }

    /// The one place the recorder is told about the session: after a version was
    /// accepted with its picture, after a save was published, after the session
    /// was replaced. Never for a produced, stale or unshown version.
    fn record(&mut self) {
        if let (Some(recorder), Some(session)) = (self.recorder.as_mut(), self.session.as_mut()) {
            recorder.observe(session);
        }
    }

    /// What the window says about the crash copy of the open document, when there
    /// is something to say. Never says written while it is being written or failed.
    pub(crate) fn recovery_line(&self) -> Option<String> {
        let status = self.recorder.as_ref()?.status();
        match status {
            ferritecad_jobs::RecoveryStatus::Off => None,
            ferritecad_jobs::RecoveryStatus::Writing => {
                Some("Recovery copy: writing the newest change…".to_owned())
            }
            ferritecad_jobs::RecoveryStatus::Written { unix_ms } => Some(format!(
                "Recovery copy written {}.",
                ferritecad_jobs::format_utc(unix_ms)
            )),
            ferritecad_jobs::RecoveryStatus::Failed(message) => Some(format!(
                "Recovery copy could not be written: {message}. The document is unchanged and Save still works."
            )),
        }
    }

    #[cfg(test)]
    pub(crate) fn recovery_status(&self) -> Option<ferritecad_jobs::RecoveryStatus> {
        self.recorder
            .as_ref()
            .map(ferritecad_jobs::RecoveryRecorder::status)
    }

    #[cfg(test)]
    pub(crate) fn recovery_settled(&self) -> bool {
        self.recorder
            .as_ref()
            .is_none_or(ferritecad_jobs::RecoveryRecorder::settled)
    }

    /// The person chose to end the window (Quit after the guard): its record goes.
    pub(crate) fn decide_exit(&mut self) {
        self.exit_decided = true;
    }

    /// Whether the open document is a recovered copy that has not been saved.
    pub(crate) fn recovered(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(DocumentSession::is_recovered)
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
            // An untitled document is named by its name, never by its private file.
            let shown = session
                .logical_path()
                .map_or_else(|| session.display_name(), |path| path.display().to_string());
            known.push((private.clone(), shown));
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
        self.operation.is_some() || self.reserved.is_some()
    }

    /// Which tab this is.
    pub(crate) fn tab(&self) -> TabId {
        self.tab
    }

    fn address(&self, generation: u64) -> Address {
        Address {
            tab: self.tab,
            generation,
        }
    }

    /// The operation in flight, when `address` is its own: this tab's, this
    /// generation's. Anything else is an answer nobody here is waiting for.
    fn addressed(&self, address: Address) -> Option<&Operation> {
        self.operation
            .as_ref()
            .filter(|op| address.tab == self.tab && op.generation == address.generation)
    }

    pub(crate) fn hold_recovery(&mut self, generation: u64) {
        self.reserved = Some(Reserved::Recover(generation));
    }

    pub(crate) fn finish_recovery(&mut self, generation: u64) -> bool {
        self.reserved
            .take_if(|held| *held == Reserved::Recover(generation))
            .is_some()
    }

    /// §30O: another tab's picture is being prepared to replace this one's. Holds
    /// the one operation slot until it is shown, refused or cancelled.
    pub(crate) fn hold_switch(&mut self, generation: u64) -> bool {
        if self.busy() {
            return false;
        }
        self.reserved = Some(Reserved::Switch(generation));
        true
    }

    /// Releases the slot held for switch `generation`; false when it is no longer
    /// held for it (cancelled, or another one).
    pub(crate) fn finish_switch(&mut self, generation: u64) -> bool {
        self.reserved
            .take_if(|held| *held == Reserved::Switch(generation))
            .is_some()
    }

    /// §30P: whether a form's Apply or Add may start: a document, nothing in
    /// flight, and the form is about the version shown. The one answer every
    /// form's button and handler share.
    pub(crate) fn takes_form_apply(&self) -> bool {
        self.has_session() && !self.busy() && !self.stale_draft
    }

    /// §30P: the shown tab's form came back made on another version than the
    /// current one. Nothing is re-read or re-bound; Apply is refused in words.
    pub(crate) fn hold_stale_draft(&mut self) {
        self.stale_draft = true;
        self.status = "The open form was made on another version of this document; it \
                       cannot be applied. Cancel it and open it again."
            .to_owned();
    }

    #[cfg(test)]
    pub(crate) fn stale_draft(&self) -> bool {
        self.stale_draft
    }

    /// §30P: the shown tab has no form open any more: nothing is held.
    pub(crate) fn release_stale_draft(&mut self) {
        self.stale_draft = false;
    }

    pub(crate) fn logical_path(&self) -> Option<&Path> {
        self.session
            .as_ref()
            .and_then(DocumentSession::logical_path)
    }

    pub(crate) fn name(&self) -> Option<String> {
        self.session.as_ref().map(DocumentSession::display_name)
    }

    /// Whether the open document has never been saved: Save asks where (§30L).
    pub(crate) fn untitled(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(DocumentSession::is_untitled)
    }

    /// The name and folder a file dialog suggests for this document: its own
    /// file, or **Untitled** in the folder dialogs start in. Never the private
    /// file the model is read from.
    pub(crate) fn suggestion(&self) -> Option<PathBuf> {
        let session = self.session.as_ref()?;
        // A recovered copy is suggested as `<stem> (recovered)`, so its dialogs
        // offer `plate (recovered).fcad`, never `plate.fcad (recovered).fcad`.
        let name = if session.is_recovered() {
            let base = session.recovery_name();
            let stem = Path::new(&base)
                .file_stem()
                .map_or_else(|| base.clone(), |stem| stem.to_string_lossy().into_owned());
            format!("{stem} (recovered)")
        } else {
            session.display_name()
        };
        Some(
            session
                .logical_path()
                .map_or_else(|| self.suggested_directory().join(&name), Path::to_path_buf),
        )
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
        self.session
            .as_ref()
            .and_then(DocumentSession::saved_version)
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
        self.reserved = None;
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
        self.record();
    }

    // --- Apply -----------------------------------------------------------

    /// Starts the edit. `spawn` is handed the ticket and the generation to label its
    /// answer with.
    pub(crate) fn begin_apply(
        &mut self,
        spawn: impl FnOnce(StepTicket, Address, &CancelToken) -> JoinHandle<()>,
    ) -> Option<Address> {
        if self.busy() {
            return None;
        }
        let session = self.session.as_mut()?;
        let ticket = session.begin_step();
        self.issued += 1;
        let generation = self.issued;
        let cancel = CancelToken::new();
        let worker = spawn(ticket, self.address(generation), &cancel);
        self.operation = Some(Operation {
            generation,
            kind: Kind::Apply,
            cancel,
            worker: Some(worker),
            staged: None,
            kept: None,
            ready_scene: None,
        });
        self.status = "Applying the change…".to_owned();
        Some(self.address(generation))
    }

    /// The answer of the edit worker.
    pub(crate) fn finish_apply(
        &mut self,
        address: Address,
        result: Result<ProducedStep>,
    ) -> Edited {
        if self
            .addressed(address)
            .is_none_or(|operation| operation.kind != Kind::Apply)
        {
            return Edited::Ignore;
        }
        self.finish_step(result)
    }

    /// The shared end of an edit or checkpoint worker's answer, once it is known
    /// to belong to the operation in flight.
    fn finish_step(&mut self, result: Result<ProducedStep>) -> Edited {
        let operation = self.operation.as_mut().expect("the answer's operation");
        if let Some(worker) = operation.worker.take() {
            let _ = worker.join();
        }
        let cancelled = operation.cancel.is_cancelled();
        let kind = operation.kind;
        let mut step = match result {
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
                    match kind {
                        Kind::Checkpoint(CheckpointKind::Create) => {
                            format!("The checkpoint was not created: {error}")
                        }
                        Kind::Checkpoint(CheckpointKind::Restore) => {
                            format!("The checkpoint was not restored: {error}")
                        }
                        Kind::Checkpoint(CheckpointKind::Delete) => {
                            format!("The checkpoint was not deleted: {error}")
                        }
                        _ => format!("Could not apply the change: {error}"),
                    }
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
        self.operation.as_mut().expect("in flight").ready_scene = step.take_scene();
        self.operation.as_mut().expect("in flight").staged = Some(Staged::Step(step));
        Edited::Show(path)
    }

    /// Delivers an Apply picture directly; history navigation still starts a scene worker.
    pub(crate) fn stage_picture(
        &mut self,
        address: Address,
        path: PathBuf,
        deliver: impl FnOnce(Result<LoadedScene>) + Send + 'static,
        spawn_scene: impl FnOnce(
            PathBuf,
            CancelToken,
            Box<dyn FnOnce(Result<LoadedScene>) + Send>,
        ) -> JoinHandle<()>,
    ) {
        let Some(cancel) = self.scene_token(address) else {
            return;
        };
        if let Some(scene) = self.operation.as_mut().and_then(|op| op.ready_scene.take()) {
            deliver(Ok(scene));
        } else {
            let worker = spawn_scene(path, cancel, Box::new(deliver));
            self.attach_scene(address, worker);
        }
    }

    pub(crate) fn remember_staged_rebuild(
        &self,
        facts: &ferritecad_scene::RebuildFacts,
    ) -> Result<()> {
        match self.operation.as_ref().and_then(|op| op.staged.as_ref()) {
            Some(Staged::Step(step)) => step.snapshot().remember_rebuild(facts),
            Some(Staged::Move(request)) => request.snapshot().remember_rebuild(facts),
            None => Ok(()),
        }
    }

    // --- checkpoints (§30N) ----------------------------------------------

    /// Starts a checkpoint change in the one operation slot Apply, Undo, Save,
    /// Open and Recover share.
    pub(crate) fn begin_checkpoint(
        &mut self,
        action: CheckpointAction,
        spawn: impl FnOnce(StepTicket, CheckpointAction, Address, &CancelToken) -> JoinHandle<()>,
    ) -> Option<Address> {
        if self.busy() {
            return None;
        }
        let session = self.session.as_mut()?;
        let ticket = session.begin_step();
        let kind = action.kind();
        self.issued += 1;
        let generation = self.issued;
        let cancel = CancelToken::new();
        let worker = spawn(ticket, action, self.address(generation), &cancel);
        self.operation = Some(Operation {
            generation,
            kind: Kind::Checkpoint(kind),
            cancel,
            worker: Some(worker),
            staged: None,
            kept: None,
            ready_scene: None,
        });
        self.status = match kind {
            CheckpointKind::Create => "Creating the checkpoint…",
            CheckpointKind::Restore => "Restoring the checkpoint…",
            CheckpointKind::Delete => "Deleting the checkpoint…",
        }
        .to_owned();
        Some(self.address(generation))
    }

    /// The answer of a checkpoint worker: as an Apply's, except that a version
    /// which only changes the checkpoints is accepted with the picture shown.
    pub(crate) fn finish_checkpoint(
        &mut self,
        address: Address,
        result: Result<CheckpointStep>,
    ) -> Edited {
        if self
            .addressed(address)
            .is_none_or(|operation| !matches!(operation.kind, Kind::Checkpoint(_)))
        {
            return Edited::Ignore;
        }
        let (step, facts) = match result {
            Ok(CheckpointStep { step, facts }) => (Ok(step), facts),
            Err(error) => (Err(error), None),
        };
        match self.finish_step(step) {
            Edited::Show(path) if facts.is_some() => {
                let operation = self.operation.as_mut().expect("in flight");
                operation.kept = facts;
                Edited::Keep(path)
            }
            other => other,
        }
    }

    /// Makes a staged version that keeps the picture current, and hands back its
    /// edit facts for the picture to carry. The statement that also re-points
    /// the shown picture at the new version; see [`Self::commit_staged`].
    pub(crate) fn commit_kept(&mut self) -> Result<crate::sketch_facts::Source> {
        let facts = self
            .operation
            .as_mut()
            .and_then(|op| op.kept.take())
            .ok_or_else(|| CadError::input("no change is waiting to be shown"))?;
        self.commit_staged()?;
        Ok(facts)
    }

    // --- Undo and Redo ---------------------------------------------------

    /// Asks the session for one step back or forward. The version to show is
    /// returned; nothing is current until it has been shown.
    pub(crate) fn begin_move(&mut self, undo: bool) -> Option<(Address, PathBuf)> {
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
            kept: None,
            ready_scene: None,
        });
        self.status = if undo { "Undoing…" } else { "Redoing…" }.to_owned();
        Some((self.address(generation), path))
    }

    // --- the scene phase ---------------------------------------------------

    /// Records the worker that is building the picture of the staged version, and
    /// the token that stops it.
    pub(crate) fn attach_scene(&mut self, address: Address, worker: JoinHandle<()>) -> bool {
        if self
            .addressed(address)
            .is_some_and(|op| op.staged.is_some())
            && let Some(op) = self.operation.as_mut()
        {
            op.worker = Some(worker);
            return true;
        }
        let _ = worker.join();
        false
    }

    pub(crate) fn scene_token(&self, address: Address) -> Option<CancelToken> {
        self.addressed(address).map(|op| op.cancel.clone())
    }

    /// Where the picture being waited for was read from.
    pub(crate) fn staged_path(&self, address: Address) -> Option<PathBuf> {
        let op = self.addressed(address)?;
        match op.staged.as_ref()? {
            Staged::Step(step) => Some(step.path().to_path_buf()),
            Staged::Move(request) => Some(request.path().to_path_buf()),
        }
    }

    /// Metadata-only Undo/Redo uses the same picture as Create/Delete. Reading
    /// the new form facts still happens on a worker, without opening a kernel.
    pub(crate) fn staged_keeps_picture(&self, address: Address) -> bool {
        let Some(op) = self.addressed(address) else {
            return false;
        };
        match op.staged.as_ref() {
            Some(Staged::Step(step)) => step.keeps_picture(),
            Some(Staged::Move(request)) => self
                .export_source()
                .is_some_and(|current| current.drawn_model() == request.drawn_model()),
            None => false,
        }
    }

    pub(crate) fn finish_kept_facts(
        &mut self,
        address: Address,
        result: Result<crate::sketch_facts::Source>,
    ) -> Edited {
        let Some(path) = self.staged_path(address) else {
            return Edited::Ignore;
        };
        if !self.staged_keeps_picture(address) {
            return Edited::Ignore;
        }
        match result {
            Ok(facts) => {
                self.operation.as_mut().expect("staged").kept = Some(facts);
                Edited::Keep(path)
            }
            Err(error) => {
                self.finish_scene(address, Err(error));
                Edited::Failed
            }
        }
    }

    /// One controller's part of a bind, for tests of this controller alone: a
    /// staged version is committed, an opened session adopted into this value. In
    /// the window `Tabs::bind` decides which controller (a new tab) gets it.
    #[cfg(test)]
    pub(crate) fn bind(&mut self, bind: Bind) -> Result<()> {
        match bind {
            Bind::Staged => self.commit_staged(),
            Bind::Open(session) => {
                self.adopt(*session);
                Ok(())
            }
            Bind::Switch(_) => Err(CadError::input("another tab is bound by the tab list")),
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
                StepCommit::Accepted | StepCommit::NoChange => {}
            },
            Staged::Move(request) => session.commit_move(request)?,
        }
        self.record();
        Ok(())
    }

    /// The scene phase is over. `shown` is whether the picture replaced the old one.
    pub(crate) fn finish_scene(&mut self, address: Address, shown: Result<()>) -> bool {
        if self.addressed(address).is_none() {
            return false;
        }
        let Some(operation) = self.operation.take() else {
            return false;
        };
        if let Some(worker) = operation.worker {
            let _ = worker.join();
        }
        self.status = match (&shown, operation.kind) {
            (Ok(()), Kind::Apply) => "Applied. Undo is available; Save writes the file.".to_owned(),
            (Ok(()), Kind::Checkpoint(CheckpointKind::Create)) => {
                "Checkpoint created. It is kept in the document when you save; Undo removes it."
                    .to_owned()
            }
            (Ok(()), Kind::Checkpoint(CheckpointKind::Restore)) => {
                "Checkpoint restored as the working model. Undo returns to the previous one; \
                 Save writes the file."
                    .to_owned()
            }
            (Ok(()), Kind::Checkpoint(CheckpointKind::Delete)) => {
                "Checkpoint deleted from the working document. Undo brings it back; Save writes \
                 the file."
                    .to_owned()
            }
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
        spawn: impl FnOnce(SavePlan, Address, &CancelToken) -> JoinHandle<()>,
    ) -> Option<Address> {
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
        let worker = spawn(plan, self.address(generation), &cancel);
        self.operation = Some(Operation {
            generation,
            kind,
            cancel,
            worker: Some(worker),
            staged: None,
            kept: None,
            ready_scene: None,
        });
        self.after_save = after;
        self.status = "Saving…".to_owned();
        Some(self.address(generation))
    }

    /// The answer of the save worker. A published file moves the checkpoint (and,
    /// for Save As, the logical path); a failure moves nothing.
    pub(crate) fn finish_save(
        &mut self,
        address: Address,
        result: std::result::Result<Saved, SaveFailure>,
    ) -> Option<SaveReport> {
        if !self
            .addressed(address)
            .is_some_and(|op| matches!(op.kind, Kind::Save | Kind::SaveAs))
        {
            return None;
        }
        let operation = self.operation.take()?;
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
                self.record();
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
        match self.reserved.take() {
            Some(Reserved::Recover(_)) => {
                self.status = "Recovery cancelled; the copy is kept.".to_owned();
                return true;
            }
            Some(Reserved::Switch(_)) => {
                self.status = "Switching tabs cancelled; this document stays.".to_owned();
                return true;
            }
            None => {}
        }
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
        self.reserved = None;
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
        // A dirty document whose window ends without the person deciding keeps its
        // crash copy for the next start; one they decided about loses it.
        if let Some(recorder) = self.recorder.take() {
            let ending = if self.exit_decided || !self.dirty() {
                ferritecad_jobs::Ending::Retire
            } else {
                ferritecad_jobs::Ending::Keep
            };
            recorder.finish(ending);
        }
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

/// Apply one expression transaction on the worker that owns the kernel session.
pub(crate) fn spawn_apply(
    expected: ferritecad_document::DocumentVersion,
    ticket: StepTicket,
    changes: Vec<ferritecad_document::EditChange>,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ProducedStep>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            let context = OperationContext::default().with_cancel(cancel);
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ticket.edit(
                changes,
                expected,
                &mut kernel,
                |kernel, source| kernel.import_step(source),
                &context,
            )
        },
        deliver,
        || Err(CadError::kernel("the edit worker stopped unexpectedly")),
    )
}

/// §30N: a checkpoint change on a worker of its own. No kernel: the library
/// copies and checks files; a restored model is drawn afterwards by
/// [`spawn_scene`] like any Apply's.
pub(crate) fn spawn_checkpoint(
    ticket: StepTicket,
    action: CheckpointAction,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<CheckpointStep>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            let context = OperationContext::default().with_cancel(cancel);
            let step = match &action {
                CheckpointAction::Create(name) => ticket.create_checkpoint(name, &context)?,
                CheckpointAction::Restore(id) => ticket.restore_checkpoint(*id, &context)?,
                CheckpointAction::Delete(id) => ticket.delete_checkpoint(*id, &context)?,
            };
            let facts = if step.keeps_picture() {
                Some(crate::sketch_facts::read(step.path())?)
            } else {
                None
            };
            Ok(CheckpointStep { step, facts })
        },
        deliver,
        || {
            Err(CadError::io(
                "checkpoint",
                "the checkpoint worker stopped unexpectedly",
            ))
        },
    )
}

/// Form facts for a private version that retains the current picture.
pub(crate) fn spawn_kept_facts(
    path: PathBuf,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<crate::sketch_facts::Source>) + Send + 'static,
) -> JoinHandle<()> {
    spawn(
        move || {
            cancel.check()?;
            let facts = crate::sketch_facts::read(&path)?;
            cancel.check()?;
            Ok(facts)
        },
        deliver,
        || {
            Err(CadError::io(
                "reading checkpoint version",
                "worker stopped unexpectedly",
            ))
        },
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

/// What a creation does on its worker (§30L): the new document is made in a
/// candidate session's own private directory by the shared creation route, and
/// its picture is read from that private file exactly as Open reads one. Nothing
/// is written anywhere else. A failure at any point drops the candidate and its
/// directory; the window's session is not touched until the picture is accepted.
pub(crate) fn create_for_view(
    root: &Path,
    content: ferritecad_jobs::NewDocument,
    context: &OperationContext,
) -> Result<(LoadedScene, DocumentSession)> {
    let session = DocumentSession::create_document_in(
        root,
        ferritecad_jobs::HistoryLimits::default(),
        content,
        ferritecad_occt::OcctKernel::new,
        context,
    )?;
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    let scene = session.current().load_scene(
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        context,
    )?;
    Ok((scene, session))
}

/// What a Recover does on its worker (§30M): the record is claimed and checked,
/// restored into a new untitled session's private directory, and its picture is
/// read from there exactly as Open reads one. A failure at any point drops the
/// session and the claim with it; the record stays as it was.
pub(crate) fn recover_for_view(
    root: &Path,
    store: &ferritecad_jobs::RecoveryStore,
    record: ferritecad_jobs::RecordId,
    context: &OperationContext,
) -> Result<(LoadedScene, DocumentSession)> {
    // Waits, off the event loop, only for a reader of the record, and stops when the
    // window cancels (§30S).
    let claim = store
        .claim_cancellable(record, context.cancel())
        .map_err(CadError::from)?;
    let session =
        DocumentSession::recover_in(root, ferritecad_jobs::HistoryLimits::default(), claim)?;
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    let scene = session.current().load_scene(
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        context,
    )?;
    Ok((scene, session))
}

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
    let scene = session.current().load_scene(
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
    use ferritecad_types::ObjectId;
    mod add_chamfer;
    mod add_cut;
    mod add_fillet;
    mod analytic;
    mod angle;
    mod chamfer;
    mod checkpoints;
    mod cut;
    mod fillet;
    mod new_document;
    mod recovery;
    mod tabs;
    use ferritecad_document::Document;
    use ferritecad_jobs::{
        CreateDocumentRequest, HistoryLimits, NewDocument, PlateSize, create_document,
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
        let feature = ferritecad_fixtures::expression_slot(
            &crate::sketch_facts::read(&file).expect("reading"),
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
        )
        .object;
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
            crate::sessions::test_open::open_in(
                f.private.path(),
                &f.file,
                HistoryLimits::default(),
            )
            .expect("session"),
        );
        sessions
    }

    /// The edit as the window runs it, on a thread, with the mock kernel (the same
    /// reused `edit with extrude_distance` operation; geometry is measured by the native tests).
    fn apply(sessions: &mut Sessions, feature: ObjectId, millimetres: f64) -> (Address, Edited) {
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, _| {
                std::thread::spawn(move || {
                    let result = {
                        let ticket = ticket;
                        let expected = ticket.source().version();
                        ticket.edit(
                            vec![
                                ferritecad_document::EditChange::constant(
                                    feature,
                                    ferritecad_document::ExpressionSlot::ExtrudeDistance,
                                    millimetres,
                                )
                                .expect("test value"),
                            ],
                            expected,
                            &mut MockKernel::new(),
                            |_, _| {
                                Err(ferritecad_types::CadError::unsupported(
                                    "no STEP imports in this model",
                                ))
                            },
                            &OperationContext::default(),
                        )
                    };
                    tx.send(result).expect("deliver");
                })
            })
            .expect("started");
        let result = rx.recv().expect("answer");
        (generation, sessions.finish_apply(generation, result))
    }

    #[test]
    fn apply_delivers_the_step_picture_without_spawning_a_scene_worker() {
        let fixture = fixture();
        let mut sessions = open(&fixture);
        let (address, edited) = apply(&mut sessions, fixture.feature, 15.0);
        let Edited::Show(path) = edited else {
            panic!("Apply must stage a picture")
        };
        let (tx, rx) = mpsc::channel();
        sessions.stage_picture(
            address,
            path,
            move |scene| {
                tx.send(scene).expect("deliver");
            },
            |_, _, _| panic!("Apply must not spawn a scene worker"),
        );
        let scene = rx.recv().expect("picture").expect("prepared picture");
        assert!(!scene.catalogue.is_empty());
        assert!(scene.rebuild_facts.is_some());
        sessions.bind(Bind::Staged).expect("accept picture");
        assert!(sessions.finish_scene(address, Ok(())));
        assert!(sessions.can_undo());
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
        ferritecad_fixtures::expression_slot(
            &crate::sketch_facts::read(path).expect("reading"),
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
        )
        .expression
        .value()
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
            sessions.finish_apply(
                Address {
                    tab: sessions.tab(),
                    generation: 77
                },
                Err(CadError::input("stale"))
            ),
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
                    tx.send({
                        let ticket = ticket;
                        let expected = ticket.source().version();
                        ticket.edit(
                            vec![
                                ferritecad_document::EditChange::constant(
                                    feature,
                                    ferritecad_document::ExpressionSlot::ExtrudeDistance,
                                    26.0,
                                )
                                .expect("test value"),
                            ],
                            expected,
                            &mut MockKernel::new(),
                            |_, _| {
                                Err(ferritecad_types::CadError::unsupported(
                                    "no STEP imports in this model",
                                ))
                            },
                            &OperationContext::default(),
                        )
                    })
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
        let prepared = ferritecad_document::EditChange::constant(
            f.feature,
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
            99.0,
        )
        .expect("prepared");
        document.write_changes(&[prepared]).expect("written");
        document.close().expect("closed");
        std::fs::rename(&theirs, &f.file).expect("lands");
        let report = save(
            &mut sessions,
            SaveTarget::InPlace,
            Some(Continuation::Close),
        )
        .expect("answered");
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
            Some(Continuation::Close),
        )
        .expect("answered");
        assert_eq!(
            report,
            SaveReport {
                published: true,
                continuation: Some(Continuation::Close)
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
        ferritecad_jobs::edit_copy(
            &ferritecad_jobs::EditRequest {
                source: accepted.clone(),
                expected: version_of(&accepted),

                changes: vec![
                    ferritecad_document::EditChange::constant(
                        f.feature,
                        ferritecad_document::ExpressionSlot::ExtrudeDistance,
                        41.0,
                    )
                    .expect("validated expression"),
                ],
                destination: copy.clone(),
            },
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("the copy");
        assert_eq!(height_of(&copy), 41.0);
        sessions.adopt(
            crate::sessions::test_open::open_in(f.private.path(), &copy, HistoryLimits::default())
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
            crate::sessions::test_open::open_in(f.private.path(), &other, HistoryLimits::default())
                .expect("the other document");
        let before = second.saved_version().expect("opened from a file");
        sessions.adopt(std::mem::replace(
            &mut second,
            crate::sessions::test_open::open_in(f.private.path(), &other, HistoryLimits::default())
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
        let opened =
            crate::sessions::test_open::open_in(f.private.path(), &other, HistoryLimits::default())
                .expect("another document");
        // Time the handoff, not the independent document's disk work.
        let started = std::time::Instant::now();
        sessions.adopt(opened);
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
    fn vertex_request(path: &Path, by: f64) -> ferritecad_jobs::EditRequest {
        let reading = crate::sketch_facts::read(path).expect("reading");
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
        ferritecad_jobs::EditRequest {
            changes: crate::transaction_tests::vertex_changes(
                path,
                choice.sketch,
                &(vertices
                    .into_iter()
                    .map(|mut v| {
                        if v.start_mm[0] == right {
                            v.start_mm[0] += by;
                        }
                        v
                    })
                    .collect::<Vec<_>>()),
            ),
            source: path.to_path_buf(),
            expected: reading.version,
            destination: PathBuf::new(),
        }
    }

    fn width_of(path: &Path) -> f64 {
        let reading = crate::sketch_facts::read(path).expect("reading");
        let vertices = reading.sketches[0].vertices.clone().expect("vertices");
        let xs = vertices.iter().map(|v| v.start_mm[0]);
        xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
    }

    /// Apply vertices as the window runs it, on a thread, with the mock kernel.
    fn apply_vertices(
        sessions: &mut Sessions,
        request: ferritecad_jobs::EditRequest,
    ) -> (Address, Edited) {
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, _| {
                std::thread::spawn(move || {
                    tx.send(ticket.edit(
                        request.changes,
                        request.expected,
                        &mut MockKernel::new(),
                        |_, _| {
                            Err(ferritecad_types::CadError::unsupported(
                                "no STEP imports in this model",
                            ))
                        },
                        &OperationContext::default(),
                    ))
                    .expect("deliver");
                })
            })
            .expect("started");
        let result = rx.recv().expect("answer");
        (generation, sessions.finish_apply(generation, result))
    }

    fn accept_vertices(sessions: &mut Sessions, request: ferritecad_jobs::EditRequest) {
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
        let prepared = ferritecad_document::EditChange::constant(
            f.feature,
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
            99.0,
        )
        .expect("p");
        document.write_changes(&[prepared]).expect("written");
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

        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        false
    }

    std::thread_local! {
        /// Peer CLI processes started on this test's thread (§30J: a refused
        /// window comparison must start none).
        static CLI_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    fn cli_runs() -> usize {
        CLI_RUNS.with(|n| n.get())
    }
    fn cli(arguments: &[&std::ffi::OsStr]) -> std::process::Output {
        CLI_RUNS.with(|n| n.set(n.get() + 1));
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
    fn apply_native(sessions: &mut Sessions, feature: ObjectId, millimetres: f64) -> LoadedScene {
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply(
                    ticket.source().version(),
                    ticket,
                    vec![
                        ferritecad_document::EditChange::constant(
                            feature,
                            ferritecad_document::ExpressionSlot::ExtrudeDistance,
                            millimetres,
                        )
                        .expect("height"),
                    ],
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
        sessions.stage_picture(
            generation,
            path,
            move |scene| tx.send(scene).expect("deliver"),
            |_, _, _| panic!("Apply must not spawn a scene worker"),
        );
        let scene = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("scene")
            .expect("the picture of the new version");
        assert!(!scene.catalogue.is_empty());
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
        scene
    }

    /// The Apply worker reads a stored STEP part with its kernel, as Open does,
    /// so a height edit beside an imported part still shows both.
    #[test]
    fn an_apply_beside_an_imported_step_part_shows_both() {
        use ferritecad_kernel::GeometryKernel as _;
        if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
            return;
        }
        let root = tempfile::tempdir().expect("root");
        let path = root.path().join("mixed.fcad");
        ferritecad_fixtures::write_plate(&path).expect("plate");
        let step = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/step/canonical/01-single-part.step"
        ))
        .expect("STEP corpus");
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let import = kernel.import_step(&step).expect("import");
        let mut document = Document::open(&path).expect("writable");
        document
            .store_step_import(ferritecad_document::StepImportRequest {
                object: ObjectId::new(),
                name: Some("Part"),
                source: &step,
                source_name: Some("part.step"),
                import: &import,
                importer: kernel.identity(),
            })
            .expect("stored");
        document.close().expect("closed");
        if let ferritecad_exchange::Import::Imported { scene, .. } = &import {
            for shape in scene.shapes().collect::<Vec<_>>() {
                kernel.release(shape);
            }
        }
        let feature = ferritecad_fixtures::expression_slot(
            &crate::sketch_facts::read(&path).expect("reading"),
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
        )
        .object;
        let private = tempfile::tempdir().expect("private");
        let mut sessions = Sessions::default();
        sessions.adopt(
            crate::sessions::test_open::open_in(private.path(), &path, HistoryLimits::default())
                .expect("session"),
        );
        let scene = apply_native(&mut sessions, feature, 12.0);
        assert!(
            scene
                .catalogue
                .iter()
                .any(|entry| entry.source_file.as_deref() == Some("part.step")),
            "{:?}",
            scene.catalogue
        );
        assert!(sessions.can_undo());
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
        let version = crate::sketch_facts::read(source).expect("reading").version;
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = Sessions::default();
        sessions.adopt(
            crate::sessions::test_open::open_in(private.path(), source, HistoryLimits::default())
                .expect("session"),
        );
        let name = source
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        assert_eq!(sessions.title(), format!("{name} — {PRODUCT_NAME}"));

        // The same change through the command line, which is what the window must
        // reach: the old `edit with extrude_distance`, unchanged.
        let peer = root.join("cli-edit.fcad");
        {
            let request = crate::edits::tests::expression_request(
                &feature.to_string(),
                "extrude_distance",
                &height.to_string(),
                "mm",
            );
            cli(&[
                "edit".as_ref(),
                source.as_os_str(),
                "--expect-version".as_ref(),
                version.content.to_string().as_ref(),
                "--request".as_ref(),
                request.path().as_os_str(),
                "--output".as_ref(),
                peer.as_os_str(),
            ])
        };
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
        let refs = crate::test_names::names(&saved).len();
        assert!(refs > 0, "the plate carries saved names");
        let built = ferritecad_eval::rebuild_cold(
            &saved,
            &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
            &OperationContext::default(),
        )
        .expect("cold rebuild");
        crate::test_names::assert_names(&built, &crate::test_names::names(&saved));
        // And the file holds exactly what is shown.
        let (stl, fbx) = peer_bytes(source, root, "saved");
        assert_eq!((stl, fbx), (peer_stl, peer_fbx));
        let _ = sessions;
    }

    fn apply_sketch_native(sessions: &mut Sessions, request: ferritecad_jobs::EditRequest) {
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply(
                    request.expected,
                    ticket,
                    request.changes.clone(),
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

    fn sketch_peer(source: &Path, request: &ferritecad_jobs::EditRequest, root: &Path, out: &Path) {
        let file = root.join("vertices.json");
        std::fs::write(
            &file,
            ferritecad_fixtures::edit_request_json(&request.changes).to_string(),
        )
        .expect("request");
        let version = crate::sketch_facts::read(source).expect("reading").version;
        cli(&[
            "edit".as_ref(),
            source.as_os_str(),
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
        vertex_gate(root, source, height, None, |path, reading, sketch| {
            let (form, request) =
                crate::sketch::tests::typed_apply(path, reading, sketch, from, to);
            assert!(
                crate::transaction_tests::vertices(&request)
                    .iter()
                    .any(|v| v.start_mm.iter().any(|n| n.to_string() == to)),
                "the typed number did not reach the request"
            );
            (form, request)
        });
    }

    /// The same gate for any way of making the request: `make` is given the accepted
    /// version's file, its reading and the Sketch, and returns the form that applied
    /// and what it asked for. `artifacts` names a directory (an environment variable)
    /// and a file stem that keep the unsaved export for the independent readers.
    fn vertex_gate(
        root: &Path,
        source: &Path,
        height: f64,
        artifacts: Option<(&str, &str)>,
        make: impl FnOnce(
            &Path,
            &crate::sketch_facts::Source,
            ObjectId,
        ) -> (crate::sketch::Editor, ferritecad_jobs::EditRequest),
    ) {
        let original = std::fs::read(source).expect("source");
        let opened = crate::sketch_facts::read(source).expect("reading");
        let feature = ferritecad_fixtures::expression_slot(
            &opened,
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
        )
        .object;
        let sketch = opened
            .sketches
            .iter()
            .find(|s| s.refusal.is_none())
            .expect("an editable Sketch")
            .sketch;
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = Sessions::default();
        sessions.adopt(
            crate::sessions::test_open::open_in(private.path(), source, HistoryLimits::default())
                .expect("session"),
        );
        let alias = sessions.logical_path().expect("logical").to_path_buf();

        // The window: height, then the vertices through the form's own widgets.
        apply_native(&mut sessions, feature, height);
        let after_height = sessions.export_path().expect("accepted");
        let reading = crate::sketch_facts::read(&after_height).expect("reading");
        let (mut form, request) = make(&after_height, &reading, sketch);
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
        {
            let request = crate::edits::tests::expression_request(
                &feature.to_string(),
                "extrude_distance",
                &height.to_string(),
                "mm",
            );
            cli(&[
                "edit".as_ref(),
                source.as_os_str(),
                "--expect-version".as_ref(),
                opened.version.content.to_string().as_ref(),
                "--request".as_ref(),
                request.path().as_os_str(),
                "--output".as_ref(),
                peer1.as_os_str(),
            ])
        };
        sketch_peer(&peer1, &request, root, &peer2);
        let (original_stl, original_fbx) = peer_bytes(source, root, "v-original");
        let (h_stl, h_fbx) = peer_bytes(&peer1, root, "v-height");
        let (v_stl, v_fbx) = peer_bytes(&peer2, root, "v-vertices");
        assert_ne!(h_stl, v_stl, "moving the vertices must change the part");

        // Export while unsaved is the working model.
        let working = sessions.export_path().expect("accepted");
        let (stl, fbx) = export_bytes(&working, &alias, root, "v-unsaved");
        assert_eq!((stl, fbx), (v_stl.clone(), v_fbx.clone()), "unsaved export");
        if let Some((variable, stem)) = artifacts
            && let Some(dir) = std::env::var_os(variable)
        {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).expect("artifact directory");
            std::fs::write(dir.join(format!("{stem}.stl")), &v_stl).expect("artifact");
            std::fs::write(dir.join(format!("{stem}.fbx")), &v_fbx).expect("artifact");
        }

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
        crate::test_names::assert_names(&built, &crate::test_names::names(&saved));
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

    /// §31D-2: the plate's vertex 2 moved with the mouse from another point as the anchor,
    /// at a typed Length, then Apply, Undo, Redo, an unsaved export, Save and a cold
    /// reopen, each against the command line given the coordinates the mouse made. The
    /// marker is printed after the last assertion, so a skip cannot print it.
    #[test]
    fn native_a_vertex_moved_from_another_anchor_is_applied_undone_exported_and_saved_like_the_command_line()
     {
        if !native() {
            return;
        }
        let (root, path, _) = crate::fillets::tests::plate();
        vertex_gate(
            root.path(),
            &path,
            9.5,
            Some(("FCAD_SKETCH_ANCHOR_ARTIFACT_DIR", "anchor")),
            |after, reading, sketch| {
                let (form, request) =
                    crate::sketch::anchor_tests::anchored_apply(after, reading, sketch, "5");
                assert_eq!(
                    crate::transaction_tests::vertices(&request)[0].start_mm,
                    [33., 15.5],
                    "the anchor's vertex"
                );
                assert_eq!(
                    crate::transaction_tests::vertices(&request)[1].start_mm,
                    [33., 10.5],
                    "5 mm south of it"
                );
                assert_eq!(
                    crate::transaction_tests::vertices(&request)[2].start_mm,
                    [-4.5, 3.25]
                );
                assert_eq!(
                    crate::transaction_tests::vertices(&request)[3].start_mm,
                    [-4.5, 15.5]
                );
                (form, request)
            },
        );
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
    fn native_a_refused_vertex_edit_changes_nothing_and_constrained_lines_remain_drawable() {
        if !native() {
            return;
        }
        // A rectangle under a Chamfer: a plate the Chamfer no longer fits is refused.
        let (root, path, _) = crate::chamfers::tests::chamfered(2.375);
        let opened = crate::sketch_facts::read(&path).expect("reading");
        let sketch = opened
            .sketches
            .iter()
            .find(|s| s.refusal.is_none())
            .expect("an editable Sketch")
            .sketch;
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = Sessions::default();
        sessions.adopt(
            crate::sessions::test_open::open_in(private.path(), &path, HistoryLimits::default())
                .expect("session"),
        );
        apply_native(
            &mut sessions,
            ferritecad_fixtures::expression_slot(
                &opened,
                ferritecad_document::ExpressionSlot::ExtrudeDistance,
            )
            .object,
            9.5,
        );
        let accepted = sessions.export_path().expect("accepted");
        let reading = crate::sketch_facts::read(&accepted).expect("reading");

        // The form accepts the payload and the session rebuild refuses the Chamfer.
        let mut form = crate::sketch::Editor::default();
        form.set_session(true, true, true);
        assert!(form.begin_edit(&accepted, &reading, sketch));
        let ctx = egui::Context::default();
        crate::sketch::tests::replace_in_form(&ctx, &mut form, "33", "-4.4");
        assert!(crate::sketch::tests::offers_apply(&ctx, &mut form));
        crate::sketch::tests::click_label(&ctx, &mut form, "Apply vertices");
        let forged = form.take_apply_request().expect("form transaction");
        assert!(form.active());
        let before = (sessions.dirty(), sessions.can_undo(), sessions.can_redo());
        let files = private_files(&sessions);
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply(
                    forged.expected,
                    ticket,
                    forged.changes.clone(),
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

        // Constraints do not hide drawable saved Lines; the rebuild solves them.
        if ferritecad_sketch_solver::is_available() {
            let (root, constrained, _) = constrained_chamfer_plate();
            let reading = crate::sketch_facts::read(&constrained).expect("reading");
            let mut form = crate::sketch::Editor::default();
            form.set_session(true, true, false);
            let editable = reading
                .sketches
                .iter()
                .any(|choice| form.begin_edit(&constrained, &reading, choice.sketch));
            assert!(editable, "a dimensioned plate must keep its vertex form");
            assert!(form.active());
            drop(root);
        }
    }

    #[test]
    fn native_an_ordinary_plate_is_applied_undone_exported_and_saved_like_the_command_line() {
        if !native() {
            return;
        }
        let (root, path, source) = crate::fillets::tests::plate();
        let feature = ferritecad_fixtures::expression_slot(
            &source,
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
        )
        .object;
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
        use ferritecad_document::{
            AddLineConstraint, LineConstraintKind, LineEndpoint, LineLengthMm,
            SketchConstraintEdits, SketchCoordinateMm,
        };
        let mut additions: Vec<_> = (0..curves.len())
            .map(|i| AddLineConstraint::Line {
                curve: curves[i].id,
                kind: if flat(i) {
                    LineConstraintKind::Horizontal
                } else {
                    LineConstraintKind::Vertical
                },
            })
            .collect();
        let first = start(0);
        additions.push(AddLineConstraint::Line {
            curve: curves[0].id,
            kind: LineConstraintKind::Fixed {
                at: LineEndpoint::Start,
                x: SketchCoordinateMm::new(first.x).expect("x"),
                y: SketchCoordinateMm::new(first.y).expect("y"),
            },
        });
        for (i, length) in [
            (
                (0..curves.len()).find(|i| flat(*i)).expect("horizontal"),
                41.125,
            ),
            (
                (0..curves.len()).find(|i| !flat(*i)).expect("vertical"),
                10.5,
            ),
        ] {
            additions.push(AddLineConstraint::Line {
                curve: curves[i].id,
                kind: LineConstraintKind::Distance(LineLengthMm::new(length).expect("length")),
            });
        }
        let changes = crate::transaction_tests::constraint_changes(
            &path,
            choice.sketch,
            &SketchConstraintEdits {
                remove: vec![],
                add: additions.into_iter().map(Into::into).collect(),
            },
        );
        let request = root.path().join("constraints.json");
        std::fs::write(
            &request,
            ferritecad_fixtures::edit_request_json(&changes).to_string(),
        )
        .expect("request");
        let constrained = root.path().join("constrained.fcad");
        cli(&[
            "edit".as_ref(),
            path.as_os_str(),
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
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let (root, constrained, sketch) = constrained_chamfer_plate();
        let reading = crate::sketch_facts::read(&constrained).expect("reading");
        assert_eq!(
            reading.unavailable_reason(),
            None,
            "the height editor accepts it"
        );
        let feature = ferritecad_fixtures::expression_slot(
            &reading,
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
        )
        .object;
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

    fn apply_constraints_native(sessions: &mut Sessions, request: &ferritecad_jobs::EditRequest) {
        let (tx, rx) = mpsc::channel();
        let (changes, expected) = (request.changes.clone(), request.expected);
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply(expected, ticket, changes, cancel.clone(), move |result| {
                    tx.send(result).expect("deliver")
                })
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
        request: &ferritecad_jobs::EditRequest,
    ) -> String {
        let before = (sessions.dirty(), sessions.can_undo(), sessions.can_redo());
        let shown = sessions.export_path().expect("accepted");
        let files = private_files(sessions);
        let (tx, rx) = mpsc::channel();
        let (changes, expected) = (request.changes.clone(), request.expected);
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply(expected, ticket, changes, cancel.clone(), move |result| {
                    tx.send(result).expect("deliver")
                })
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
        request: &ferritecad_jobs::EditRequest,
        root: &Path,
        out: &Path,
    ) {
        let file = root.join("constraints-request.json");
        std::fs::write(
            &file,
            ferritecad_fixtures::edit_request_json(&request.changes).to_string(),
        )
        .expect("request");
        let version = crate::sketch_facts::read(source).expect("reading").version;
        cli(&[
            "edit".as_ref(),
            source.as_os_str(),
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
        crate::sketch_facts::read(path)
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
            &crate::sketch_facts::Source,
            ObjectId,
        ) -> (crate::constraints::Editor, ferritecad_jobs::EditRequest),
    ) {
        native_constraint_gate_with(root, source, height, None, draft);
    }

    /// A name under which a saved model's exports go to the pinned reader, and a reader of
    /// the saved STL of the caller's own.
    type SavedReader<'a> = (&'a str, &'a dyn Fn(&Path, &[u8]));

    /// The same, with a name under which the saved model's exports go to the pinned reader
    /// (`FCAD_ANALYTIC_ARTIFACTS`, as the Circle and annulus gates') and a reader of the saved
    /// STL of the caller's own.
    fn native_constraint_gate_with(
        root: &Path,
        source: &Path,
        height: f64,
        readers: Option<SavedReader<'_>>,
        draft: impl FnOnce(
            &Path,
            &crate::sketch_facts::Source,
            ObjectId,
        ) -> (crate::constraints::Editor, ferritecad_jobs::EditRequest),
    ) {
        let original = std::fs::read(source).expect("source");
        let opened = crate::sketch_facts::read(source).expect("reading");
        let feature = ferritecad_fixtures::expression_slot(
            &opened,
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
        )
        .object;
        let sketch = opened.constraint_sketches[0].sketch;
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = Sessions::default();
        sessions.adopt(
            crate::sessions::test_open::open_in(private.path(), source, HistoryLimits::default())
                .expect("session"),
        );
        let alias = sessions.logical_path().expect("logical").to_path_buf();

        apply_native(&mut sessions, feature, height);
        let after_height = sessions.export_path().expect("accepted");
        let reading = crate::sketch_facts::read(&after_height).expect("reading");
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
        for removed in &crate::transaction_tests::constraints(&request).remove {
            assert!(!new_ids.contains(removed), "{removed} was not removed");
        }

        // The command line, doing the same two things.
        let peer1 = root.join("peer-height.fcad");
        let peer2 = root.join("peer-constraints.fcad");
        {
            let request = crate::edits::tests::expression_request(
                &feature.to_string(),
                "extrude_distance",
                &height.to_string(),
                "mm",
            );
            cli(&[
                "edit".as_ref(),
                source.as_os_str(),
                "--expect-version".as_ref(),
                opened.version.content.to_string().as_ref(),
                "--request".as_ref(),
                request.path().as_os_str(),
                "--output".as_ref(),
                peer1.as_os_str(),
            ])
        };
        constraint_peer(&peer1, &request, root, &peer2);
        let (original_stl, original_fbx) = peer_bytes(source, root, "c-original");
        let (h_stl, h_fbx) = peer_bytes(&peer1, root, "c-height");
        let (c_stl, c_fbx) = peer_bytes(&peer2, root, "c-constraints");
        assert_eq!(
            same_model_with_explicit_new_ids(&applied, &peer2, sketch, &old_ids, "Apply").len(),
            crate::transaction_tests::constraints(&request).add.len(),
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
        let references = crate::test_names::names(&saved);
        assert!(!references.is_empty(), "the plate carries saved names");
        crate::test_names::assert_names(&built, &references);
        let (stl, fbx) = peer_bytes(source, root, "c-saved");
        if let Some((name, check)) = readers {
            check(source, &stl);
            if let Some(dir) = std::env::var_os("FCAD_ANALYTIC_ARTIFACTS") {
                let dir = PathBuf::from(dir);
                std::fs::create_dir_all(&dir).expect("artifact directory");
                std::fs::write(dir.join(format!("{name}.stl")), &stl).expect("STL for the readers");
                std::fs::write(dir.join(format!("{name}.fbx")), &fbx)
                    .expect("FBX for the pinned reader");
            }
        }
        assert_eq!((stl, fbx), (c_stl, c_fbx));

        // A refused edit past the form changes nothing, and does not cut off the
        // Redo; a branch that succeeds does.
        move_native(&mut sessions, true);
        assert!(sessions.can_redo());
        let mut forged = request.clone();
        forged.expected = crate::sketch_facts::read(&sessions.export_path().expect("accepted"))
            .expect("reading")
            .version;
        forged
            .changes
            .push(ferritecad_document::EditChange::RemoveConstraint {
                sketch: crate::transaction_tests::sketch(&forged),
                constraint: ferritecad_types::StableEntityId::new(),
            });
        refused_constraints_native(&mut sessions, &forged);
        assert!(sessions.can_redo(), "a refusal cut off the Redo");
        apply_native(&mut sessions, feature, height + 2.0);
        assert!(!sessions.can_redo());
        assert_eq!(
            same_model_with_explicit_new_ids(source, &peer2, sketch, &old_ids, "branch").len(),
            crate::transaction_tests::constraints(&request).add.len(),
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
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
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
        sketch
            .constraints
            .iter()
            .find_map(|c| match c.rule.clone() {
                ferritecad_document::SketchConstraintRule::Distance { a, b, distance }
                    if a.curve == curve && b.curve == curve =>
                {
                    Some(distance.value())
                }
                _ => None,
            })
    }

    /// §31F: a directed angle typed into the form, Applied into an unsaved
    /// session after a height step, exported before Save, undone, redone and
    /// saved, each against the command line doing the same to the same dart.
    #[test]
    fn native_directed_angle_of_a_dart_is_applied_like_the_command_line() {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("dart.fcad");
        ferritecad_jobs::create_document_with_kernel(
            ferritecad_jobs::CreateDocumentRequest::new(
                &path,
                ferritecad_jobs::NewDocument::SketchExtrude(
                    ferritecad_document::PolygonExtrusion::new(
                        vec![[0., 0.], [40., 0.], [10., 10.], [0., 40.]],
                        10.,
                    )
                    .expect("a dart"),
                ),
                "test",
            ),
            ferritecad_occt::OcctKernel::new,
            &OperationContext::default(),
        )
        .expect("source");
        // A profile that already holds its closure joints (what removing the last
        // constraint leaves), so that the angle is the one thing the edit adds.
        {
            use ferritecad_document::{
                AddLineConstraint, AddSketchConstraint, LineConstraintKind, SketchConstraintEdits,
            };
            let mut document = Document::open(&path).expect("document");
            let choice = crate::sketch_facts::Source::read(&document)
                .expect("catalog")
                .constraint_sketches[0]
                .clone();
            let curve = choice.stored.as_ref().expect("stored").curves[0].id;
            let prepared = ferritecad_fixtures::constraint_changes(
                &document,
                choice.sketch,
                &SketchConstraintEdits {
                    remove: vec![],
                    add: vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve,
                        kind: LineConstraintKind::Horizontal,
                    })],
                },
            )
            .expect("prepare");
            let horizontal = ferritecad_fixtures::added_constraints(&prepared)
                .last()
                .expect("the Horizontal")
                .id;
            document.write_changes(&prepared).expect("joints");
            let prepared = ferritecad_fixtures::constraint_changes(
                &document,
                choice.sketch,
                &SketchConstraintEdits {
                    remove: vec![horizontal],
                    add: vec![],
                },
            )
            .expect("prepare");
            document.write_changes(&prepared).expect("removed");
            document.close().expect("close");
        }
        native_constraint_gate(root.path(), &path, 9.5, |path, source, _| {
            // Lines 2 and 3, the second vector turned: 110 is -70 from Line 2.
            crate::constraints::tests::angle::typed_angle_apply(path, source, (2, 3), true, "110")
        });
    }

    /// §31G: a collinearity of a U's two rims typed into the form, Applied into
    /// an unsaved session after a height step, exported before Save, undone,
    /// redone and saved, each against the command line doing the same to the
    /// same profile.
    #[test]
    fn native_collinear_of_a_u_is_applied_like_the_command_line() {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("u.fcad");
        ferritecad_jobs::create_document_with_kernel(
            ferritecad_jobs::CreateDocumentRequest::new(
                &path,
                ferritecad_jobs::NewDocument::SketchExtrude(
                    ferritecad_document::PolygonExtrusion::new(
                        crate::constraints::tests::collinear::U_PROFILE.to_vec(),
                        10.,
                    )
                    .expect("a U"),
                ),
                "test",
            ),
            ferritecad_occt::OcctKernel::new,
            &OperationContext::default(),
        )
        .expect("source");
        // A profile that already holds its closure joints (what removing the last
        // constraint leaves), so that the collinearity is the one thing the edit
        // adds.
        {
            use ferritecad_document::{
                AddLineConstraint, AddSketchConstraint, LineConstraintKind, SketchConstraintEdits,
            };
            let mut document = Document::open(&path).expect("document");
            let choice = crate::sketch_facts::Source::read(&document)
                .expect("catalog")
                .constraint_sketches[0]
                .clone();
            let curve = choice.stored.as_ref().expect("stored").curves[0].id;
            let prepared = ferritecad_fixtures::constraint_changes(
                &document,
                choice.sketch,
                &SketchConstraintEdits {
                    remove: vec![],
                    add: vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve,
                        kind: LineConstraintKind::Horizontal,
                    })],
                },
            )
            .expect("prepare");
            let horizontal = ferritecad_fixtures::added_constraints(&prepared)
                .last()
                .expect("the Horizontal")
                .id;
            document.write_changes(&prepared).expect("joints");
            let prepared = ferritecad_fixtures::constraint_changes(
                &document,
                choice.sketch,
                &SketchConstraintEdits {
                    remove: vec![horizontal],
                    add: vec![],
                },
            )
            .expect("prepare");
            document.write_changes(&prepared).expect("removed");
            document.close().expect("close");
        }
        native_constraint_gate(root.path(), &path, 9.5, |path, source, _| {
            // The rims are Lines 1 and 5.
            crate::constraints::tests::collinear::collinear_apply(path, source, (1, 5))
        });
    }

    #[test]
    fn native_radius_and_fixed_centre_of_a_circle_are_applied_like_the_command_line() {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
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

    /// §31K: a constrained rectangle whose Sketch stores a rough guess, its rules chosen on
    /// the canvas of the accepted solved drawing (Pair A and B, the Line, the End), Applied
    /// into an unsaved session after a height step, exported before Save, undone, redone
    /// and saved, each against the command line given the same UUIDs; the saved model is
    /// read back by the drawing of its own rebuild and by an STL reader that is not the writer.
    #[test]
    fn native_rules_chosen_on_the_canvas_of_a_constrained_rectangle_are_applied_like_the_command_line()
     {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("rectangle.fcad");
        crate::constraints::tests::canvas::write_guessed_rectangle(&path);
        native_constraint_gate_with(
            root.path(),
            &path,
            9.5,
            Some(("constraint-canvas-rectangle", &|saved, stl| {
                crate::constraints::tests::canvas::saved_rectangle_is_what_the_rules_ask(
                    saved, stl, 9.5,
                )
            })),
            |path, source, sketch| {
                crate::constraints::tests::canvas::native_rectangle(path, source, sketch)
            },
        );
    }

    /// §31K: a cylinder whose Sketch stores the circle (4.5, 0) r 1.2 and keeps a Radius and
    /// a Fixed centre that solve it at (0, 0) r 10; the ring and the centre picked on the
    /// canvas of that solved circle replace both rules, through the same session route.
    #[test]
    fn native_a_radius_and_a_fixed_centre_chosen_on_the_canvas_of_a_solved_circle_are_applied_like_the_command_line()
     {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("cylinder.fcad");
        crate::constraints::tests::canvas::write_guessed_circle(&path);
        native_constraint_gate_with(
            root.path(),
            &path,
            9.5,
            Some(("constraint-canvas-circle", &|saved, stl| {
                crate::constraints::tests::canvas::saved_circle_is_what_the_rules_ask(
                    saved, stl, 9.5,
                )
            })),
            |path, source, sketch| {
                crate::constraints::tests::canvas::native_circle(path, source, sketch)
            },
        );
    }

    /// §31L: a rectangle whose Sketch stores a rough guess and keeps rules that solve it into
    /// 60 × 30 mm. The label of the saved 60 mm length, chosen on the canvas of the accepted
    /// solved drawing under the Dimension target, replaces that rule by 70 mm; the request is
    /// applied into an unsaved session after a height step, exported before Save, undone, redone
    /// and saved, each against the command line given the same request, and the saved model is
    /// read back by the form of its cold rebuild and by an STL reader that is not the writer.
    #[test]
    fn native_a_saved_length_chosen_by_its_label_and_replaced_is_applied_like_the_command_line() {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("rectangle.fcad");
        crate::constraints::tests::dimension::write_dimensioned_rectangle(&path);
        let seen = std::cell::Cell::new(None);
        native_constraint_gate_with(
            root.path(),
            &path,
            9.5,
            Some(("dimension-labels-rectangle", &|saved, stl| {
                crate::constraints::tests::dimension::saved_dimensioned_rectangle(
                    saved,
                    stl,
                    9.5,
                    seen.get().expect("the form ran first"),
                )
            })),
            |path, source, sketch| {
                crate::constraints::tests::dimension::native_dimension_rectangle(
                    path, source, sketch, &seen,
                )
            },
        );
    }

    /// §31L: a cylinder whose Sketch stores the circle (4.5, 0) r 1.2 and keeps a Radius and a
    /// Fixed centre that solve it at (0, 0) r 10; the label of the Radius, chosen under the
    /// Dimension target, replaces it by a diameter of 17 mm (the Radius 8.5), through the same
    /// session route.
    #[test]
    fn native_a_saved_radius_chosen_by_its_label_and_replaced_by_a_diameter_is_applied_like_the_command_line()
     {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("cylinder.fcad");
        crate::constraints::tests::canvas::write_guessed_circle(&path);
        let seen = std::cell::Cell::new(None);
        native_constraint_gate_with(
            root.path(),
            &path,
            9.5,
            Some(("dimension-labels-circle", &|saved, stl| {
                crate::constraints::tests::dimension::saved_dimensioned_circle(
                    saved,
                    stl,
                    9.5,
                    seen.get().expect("the form ran first"),
                )
            })),
            |path, source, sketch| {
                crate::constraints::tests::dimension::native_dimension_circle(
                    path, source, sketch, &seen,
                )
            },
        );
    }

    /// §31M: a quadrilateral whose Sketch stores a rough guess and keeps rules that solve it into
    /// another shape (an anchor, three lengths and two saved directed angles, the second with its
    /// first vector reversed). The label of the saved -100° angle, chosen on the canvas of the
    /// accepted solved drawing under the Dimension target, replaces that rule by -130°, keeping both
    /// exact vectors; the request is applied into an unsaved session after a height step, exported
    /// before Save, undone, redone and saved, each against the command line given the same request,
    /// and the saved model is read back by the form of its cold rebuild, by the sign of the turn
    /// between the very vectors the rule names, and by an STL reader that is not the writer.
    #[test]
    fn native_a_saved_angle_chosen_by_its_label_and_replaced_is_applied_like_the_command_line() {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("quad.fcad");
        crate::constraints::tests::turn_label::write_angled_quad(&path);
        let pristine = root.path().join("pristine.fcad");
        std::fs::copy(&path, &pristine).expect("a copy of the source");
        let seen = std::cell::Cell::new(None);
        native_constraint_gate_with(
            root.path(),
            &path,
            9.5,
            Some(("angle-dimension-quad", &|saved, stl| {
                crate::constraints::tests::turn_label::saved_angled_quad(
                    saved,
                    stl,
                    9.5,
                    seen.get().expect("the form ran first"),
                )
            })),
            |path, source, sketch| {
                crate::constraints::tests::turn_label::native_angle_quad(
                    path, source, sketch, &seen,
                )
            },
        );
        // The reader of the saved result is not a rubber stamp: given the pristine source and its
        // own export in place of the result, it must fail (the angle is still -100°).
        let (source_stl, _) = peer_bytes(&pristine, root.path(), "angle-pristine");
        let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::constraints::tests::turn_label::saved_angled_quad(
                &pristine,
                &source_stl,
                9.5,
                seen.get().expect("the form ran first"),
            );
        }));
        assert!(
            verdict.is_err(),
            "the source was accepted in place of the result"
        );
    }

    /// §31N: a rectangle whose Sketch stores a rough guess and keeps rules that solve it into another
    /// shape (a pin, a horizontal, a vertical, two lengths, and an EqualLength and a Parallel of Lines 1
    /// and 3 that stand on one place). The marker of the EqualLength, chosen on the canvas of the accepted
    /// solved drawing under the Rule target, is removed by the one explicit action; the request is
    /// applied into an unsaved session after a height step, exported before Save, undone, redone and
    /// saved, each against the command line given the same request, and the saved model is read back
    /// by the solver's own degrees of freedom, by the form of its cold rebuild and by an STL reader
    /// that is not the writer.
    #[test]
    fn native_a_saved_equal_length_chosen_by_its_marker_and_removed_is_applied_like_the_command_line()
     {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("rectangle.fcad");
        crate::constraints::tests::marker::write_marked_rectangle(&path);
        let pristine = root.path().join("pristine.fcad");
        std::fs::copy(&path, &pristine).expect("a copy of the source");
        let seen = std::cell::Cell::new(None);
        native_constraint_gate_with(
            root.path(),
            &path,
            9.5,
            Some(("constraint-markers-rectangle", &|saved, stl| {
                crate::constraints::tests::marker::saved_unequal_rectangle(
                    saved,
                    stl,
                    9.5,
                    seen.get().expect("the form ran first"),
                )
            })),
            |path, source, sketch| {
                crate::constraints::tests::marker::native_marked_rectangle(
                    path, source, sketch, &seen,
                )
            },
        );
        // The reader of the saved result is not a rubber stamp: given the pristine source and its own
        // export in place of the result, it must fail (Line 3 is still 60 mm and nothing is free).
        let (source_stl, _) = peer_bytes(&pristine, root.path(), "markers-pristine");
        let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::constraints::tests::marker::saved_unequal_rectangle(
                &pristine,
                &source_stl,
                9.5,
                seen.get().expect("the form ran first"),
            );
        }));
        assert!(
            verdict.is_err(),
            "the source was accepted in place of the result"
        );
    }

    /// §31N: a cylinder whose Sketch stores the circle (4.5, 0) r 1.2 and keeps a Radius and a Fixed
    /// centre that solve it at (0, 0) r 10. The marker of the Fixed centre, chosen under the Rule target,
    /// is removed through the same session route; the centre is then where the Sketch stored it.
    #[test]
    fn native_a_saved_fixed_centre_chosen_by_its_marker_and_removed_is_applied_like_the_command_line()
     {
        if !native() {
            return;
        }
        if !ferritecad_sketch_solver::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("cylinder.fcad");
        crate::constraints::tests::canvas::write_guessed_circle(&path);
        let pristine = root.path().join("pristine.fcad");
        std::fs::copy(&path, &pristine).expect("a copy of the source");
        let seen = std::cell::Cell::new(None);
        native_constraint_gate_with(
            root.path(),
            &path,
            9.5,
            Some(("constraint-markers-circle", &|saved, stl| {
                crate::constraints::tests::marker::saved_unpinned_circle(
                    saved,
                    stl,
                    9.5,
                    seen.get().expect("the form ran first"),
                )
            })),
            |path, source, sketch| {
                crate::constraints::tests::marker::native_marked_circle(path, source, sketch, &seen)
            },
        );
        let (source_stl, _) = peer_bytes(&pristine, root.path(), "centre-pristine");
        let verdict = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::constraints::tests::marker::saved_unpinned_circle(
                &pristine,
                &source_stl,
                9.5,
                seen.get().expect("the form ran first"),
            );
        }));
        assert!(
            verdict.is_err(),
            "the source was accepted in place of the result"
        );
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
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        let (root, constrained, sketch) = constrained_chamfer_plate();
        let opened = crate::sketch_facts::read(&constrained).expect("reading");
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = Sessions::default();
        sessions.adopt(
            crate::sessions::test_open::open_in(
                private.path(),
                &constrained,
                HistoryLimits::default(),
            )
            .expect("session"),
        );
        apply_native(
            &mut sessions,
            ferritecad_fixtures::expression_slot(
                &opened,
                ferritecad_document::ExpressionSlot::ExtrudeDistance,
            )
            .object,
            9.5,
        );
        move_native(&mut sessions, true);
        assert!(sessions.can_redo());
        move_native(&mut sessions, false);
        let accepted = sessions.export_path().expect("accepted");
        let reading = crate::sketch_facts::read(&accepted).expect("reading");
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
        let reading = crate::sketch_facts::read(&accepted).expect("reading");
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
        let reading = crate::sketch_facts::read(&accepted).expect("reading");
        let (form, offered) = crate::constraints::tests::session_apply::replace_length_offers_apply(
            &accepted,
            &reading,
            sketch,
            width + 1,
            "1",
        );
        assert!(form.active(), "the draft was erased");
        let mut forged = ferritecad_jobs::EditRequest {
            changes: crate::transaction_tests::constraint_changes(
                &(accepted.clone()),
                sketch,
                &(form.draft_edits().expect("a draft")),
            ),
            source: accepted.clone(),
            expected: reading.version,
            destination: PathBuf::new(),
        };
        if offered {
            // The form leaves the judgement to the document: the worker refuses.
            refused_constraints_native(&mut sessions, &forged);
        } else {
            forged.changes = crate::transaction_tests::constraint_changes(
                &forged.source,
                sketch,
                &form.draft_edits().expect("a draft"),
            );
            refused_constraints_native(&mut sessions, &forged);
        }
        assert!(form.active());
        drop(root);
    }

    /// With a kernel and no solver, an Apply of constraints is refused whole and
    /// the document stays clean; nothing is accepted, nothing is left behind.
    #[test]
    fn without_the_solver_a_constraints_apply_is_refused_and_nothing_changes() {
        if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
            || ferritecad_sketch_solver::is_available()
        {
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
            crate::sessions::test_open::open_in(private.path(), &path, HistoryLimits::default())
                .expect("session"),
        );
        let request = ferritecad_jobs::EditRequest {
            changes: crate::transaction_tests::constraint_changes(
                &(path.clone()),
                sketch,
                &(ferritecad_document::SketchConstraintEdits {
                    remove: Vec::new(),
                    add: vec![ferritecad_document::AddSketchConstraint::Line(
                        ferritecad_document::AddLineConstraint::Line {
                            curve: curves[0].id,
                            kind: ferritecad_document::LineConstraintKind::Horizontal,
                        },
                    )],
                }),
            ),
            source: path.clone(),
            expected: source.version,
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
            crate::sessions::test_open::open_in(private.path(), &path, HistoryLimits::default())
                .expect("a document opens into a session without a kernel"),
        );
        let before = private_files(&sessions);
        let feature = ferritecad_fixtures::expression_slot(
            &source,
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
        )
        .object;
        let (tx, rx) = mpsc::channel();
        let generation = sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply(
                    ticket.source().version(),
                    ticket,
                    vec![
                        ferritecad_document::EditChange::constant(
                            feature,
                            ferritecad_document::ExpressionSlot::ExtrudeDistance,
                            30.0,
                        )
                        .expect("height"),
                    ],
                    cancel.clone(),
                    move |result| tx.send(result).expect("deliver"),
                )
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

    /// Missing solver support refuses the initial picture, before an Apply can
    /// acquire baseline facts. No session or private version survives the refusal.
    #[test]
    fn without_the_solver_a_constrained_plate_cannot_supply_an_apply_baseline() {
        if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
            || ferritecad_sketch_solver::is_available()
        {
            return;
        }
        let (root, path, _) = crate::chamfers::tests::chamfered(2.375);
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
            let prepared = ferritecad_fixtures::constraint_changes(&document, sketch.0, &edits)
                .expect("prepared without a solver");
            document.write_changes(&prepared).expect("written");
            document.close().expect("closed");
        }
        let original = std::fs::read(&path).expect("bytes");
        let private = tempfile::tempdir().expect("private");
        let error = open_for_view(private.path(), &path, &OperationContext::default())
            .expect_err("no baseline picture without the solver");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert!(error.to_string().contains("planegcs"));
        assert_eq!(
            std::fs::read_dir(private.path())
                .expect("private files")
                .count(),
            0
        );
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
                Some(Continuation::Close),
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
