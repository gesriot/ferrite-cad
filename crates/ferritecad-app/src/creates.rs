// SPDX-License-Identifier: MIT
//! Making a new document from the window, away from the event loop.
//!
//! # The work is not here
//!
//! What a new document *is* — the display units, the sample plate's plane,
//! profile, extrusion and references, the transaction they go in through — is
//! [`ferritecad_jobs::create_document_with_kernel`], which is the same route the
//! shipped command takes. Nothing in this file writes a byte of SQLite, decides
//! what a plate is made of, or knows what an exit code is. What is here is
//! everything that is about a *window*: when the action is offered, what the form
//! holds, what happens while it runs, and what the user is shown afterwards.
//!
//! # A new document is a session before it is a file (§30L)
//!
//! Creating no longer asks for a file name. The worker makes the document inside
//! a *candidate* session's own private directory
//! ([`ferritecad_jobs::DocumentSession::create_document_in`]) and reads its
//! picture from there, exactly as Open does. The candidate becomes the window's
//! document only when that picture is accepted (`sessions::Bind::Open`, in the
//! window's `show`); until then the picture, the session, its history and every
//! draft are what they were, and a refused, cancelled, stale or unshowable
//! candidate is dropped with its directory. The document is **Untitled** until
//! its first Save, which asks where its file goes.
//!
//! The New form and the drawing drafts stay on screen until the new document is
//! accepted, so nothing a person typed is lost to a failure.
//!
//! # Nothing here blocks the loop
//!
//! Creating is filesystem and kernel work and runs on a thread of its own,
//! cancelled and joined before this process ends.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use ferritecad_jobs::{DocumentSession, NewDocument, PlateSize};
use ferritecad_kernel::{CancelToken, OperationContext};
use ferritecad_scene::LoadedScene;
use ferritecad_types::{ErrorKind, Result};
use ferritecad_ui::{NewChoice, NewContent, NewDocumentForm, ViewportInput};

/// What the window says while a document is being made.
const CREATING: &str = "Creating a new document…";
/// The result of the last creation. This stays visible after Save or Open, so it
/// must not claim the current document is still untitled or unsaved.
const CREATED: &str = "Created a new document.";
/// New was given up for an Open (§30T). Nothing was made.
const CREATE_STOPPED: &str = "New was stopped to open another file; nothing was made.";
/// A document that was not made, or not shown. Nothing on screen changed.
const CREATE_FAILED: &str = "Could not create the new document";
/// A creation the window gave up on. Nothing on screen changed.
const CREATE_CANCELLED: &str = "New document cancelled; nothing changed";

/// A new document the worker made and whose picture it read: what the window
/// accepts together, or drops together (its private directory with it).
pub(crate) struct Candidate {
    pub(crate) scene: LoadedScene,
    pub(crate) session: DocumentSession,
}

impl std::fmt::Debug for Candidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Candidate")
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

/// What the form says about a size that is not a number.
///
/// Names the field, because a person looking at three boxes needs to know
/// which one to fix. What counts as an acceptable size once it *is* a number —
/// how big, whether it may be zero — is the document's to say and is not
/// second-guessed here.
fn not_a_number(field: &str, typed: &str) -> String {
    format!("{field} is not a number: {typed:?}")
}

/// Which create request an answer belongs to.
///
/// Monotonic and never reused, on the same terms as a load and an export: two
/// creations are two requests, and the older one's answer is as unwelcome as
/// any other stale answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CreateGeneration(u64);

/// Which New a question is about: one opening of the form or of the drawing, from
/// its press to its end (its Cancel, a tab of its own, or a stop for an Open).
///
/// Monotonic and never reused, on the same terms as a creation's generation. An
/// idle form has no creation to number, so the opening itself is numbered: a
/// question asked over one New is never answered against the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NewGeneration(u64);

/// A file chosen with Open while New was not finished (§30T): nothing is thrown
/// away until the person says so. The only state the choice leaves behind.
#[derive(Debug)]
struct AskedOpen {
    path: PathBuf,
    over: NewGeneration,
}

/// A creation that has been started and not yet joined.
struct Creating {
    cancel: CancelToken,
    worker: JoinHandle<()>,
}

/// What the window says about the document it was last asked to make.
///
/// Entirely separate from what it says about opening one.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) enum CreateStatus {
    /// Nothing has been asked for. True until the first New.
    #[default]
    Idle,
    /// Running. Nothing on screen has changed yet.
    Running { generation: CreateGeneration },
    /// Made and waiting for its picture to be accepted.
    Made,
    /// Accepted: the new document is the window's, untitled.
    Shown,
    /// Given up on. Nothing on screen changed.
    Cancelled,
    /// New was stopped for an Open: its form, drawing and creation are gone.
    Stopped,
    /// Could not be made or shown. Nothing on screen changed.
    Failed { message: String },
}

impl CreateStatus {
    /// The line to put in front of the user. Names no private path: a candidate
    /// is shown as what it is, an untitled document.
    fn line(&self) -> String {
        match self {
            Self::Idle => String::new(),
            Self::Running { .. } | Self::Made => CREATING.to_owned(),
            Self::Shown => CREATED.to_owned(),
            Self::Cancelled => CREATE_CANCELLED.to_owned(),
            Self::Stopped => CREATE_STOPPED.to_owned(),
            Self::Failed { message } => format!("{CREATE_FAILED}: {message}"),
        }
    }
}

/// Every creation this window has started and not yet finished with, and the
/// form it is filling in.
///
/// The same shape as the exports beside it, and for the same reasons: one
/// request may be current, an older answer changes nothing, and no worker is
/// ever left running with nobody to wait for it.
#[derive(Default)]
pub(crate) struct Creates {
    issued: u64,
    /// How many times New has been opened (§30T): the generation a question is about.
    opened: u64,
    /// §30T: the file chosen while New was unfinished, until it is answered.
    asked: Option<AskedOpen>,
    cancel_requested: bool,
    /// The request whose answer may still reach the window.
    current: Option<CreateGeneration>,
    running: Vec<Creating>,
    status: CreateStatus,
    /// What the form on screen holds, while one is on screen.
    ///
    /// `None` is no form at all rather than an empty one: a window that kept a
    /// half-filled form invisibly would show it again the next time somebody
    /// pressed New, holding numbers they typed for a document they decided not
    /// to make.
    form: Option<NewDocumentForm>,
    pub(crate) sketch: crate::sketch::Editor,
}

impl Creates {
    pub(crate) fn forms(&mut self) -> (Option<&mut NewDocumentForm>, &mut crate::sketch::Editor) {
        (self.form.as_mut(), &mut self.sketch)
    }
    pub(crate) fn status(&self) -> &CreateStatus {
        &self.status
    }

    /// §30L: the window's one `can_create` answer, written each frame into every
    /// form that can make a new document, so the button and the handler agree.
    pub(crate) fn set_can_create(&mut self, can_create: bool) {
        if let Some(form) = self.form.as_mut() {
            form.can_create = can_create;
        }
        self.sketch.set_create(can_create);
    }

    /// The form on screen, to be drawn and typed into.
    #[cfg(test)]
    pub(crate) fn form(&mut self) -> Option<&mut NewDocumentForm> {
        self.form.as_mut()
    }

    /// How many creation workers are accounted for, running or not yet joined.
    #[cfg(test)]
    pub(crate) fn accounted(&self) -> usize {
        self.running.len()
    }

    /// Whether a creation is running, which is what makes New unavailable.
    pub(crate) fn running(&self) -> bool {
        matches!(self.status, CreateStatus::Running { .. })
    }

    pub(crate) fn busy(&self) -> bool {
        self.running() || self.form.is_some() || self.sketch.active()
    }

    /// §30P: New is under way — a creation running, the New form, or a drawing
    /// for a new document. None of it belongs to a tab, so it holds the window.
    pub(crate) fn making_new(&self) -> bool {
        self.running() || self.form.is_some() || self.sketch.drawing_new()
    }

    /// The saved Sketch form is the caller of Apply, not an operation blocking it.
    /// New and the other Sketch-family forms retain their existing exclusion.
    pub(crate) fn can_apply_sketch(&self) -> bool {
        !self.running() && self.form.is_none() && self.sketch.editing_saved_vertices()
    }

    /// The constraints form is the caller of its own Apply, like the Sketch form.
    pub(crate) fn can_apply_constraints(&self) -> bool {
        !self.running() && self.form.is_none() && self.sketch.constraints.active()
    }

    /// The saved Circle or annulus form is the caller of its own Apply.
    pub(crate) fn can_apply_analytic(&self) -> bool {
        !self.running() && self.form.is_none() && self.sketch.editing_analytic()
    }

    /// The angle form is the caller of its own Apply, not an operation blocking it.
    pub(crate) fn can_apply_angle(&self) -> bool {
        !self.running()
            && self.form.is_none()
            && self.sketch.editing_saved_angle()
            && !self.sketch.constraints.active()
            && !self.sketch.cuts.active()
            && !self.sketch.fillets.active()
            && !self.sketch.chamfers.active()
    }

    /// The existing Cut form may Apply itself; every other form excludes it.
    pub(crate) fn can_apply_cut(&self) -> bool {
        !self.running() && self.form.is_none() && self.sketch.only_editing_cut()
    }

    /// §30I: the Cut Add form may Add itself; every other form excludes it.
    pub(crate) fn can_add_cut(&self) -> bool {
        !self.running() && self.form.is_none() && self.sketch.only_adding_cut()
    }

    /// §30J: the Fillet Add form may Add itself; every other form excludes it.
    pub(crate) fn can_add_fillet(&self) -> bool {
        !self.running() && self.form.is_none() && self.sketch.only_adding_fillet()
    }

    pub(crate) fn can_apply_fillet_radius(&self) -> bool {
        !self.running() && self.form.is_none() && self.sketch.only_editing_fillet_radius()
    }

    /// §30K: the Chamfer Add form may Add itself; every other form excludes it.
    pub(crate) fn can_add_chamfer(&self) -> bool {
        !self.running() && self.form.is_none() && self.sketch.only_adding_chamfer()
    }

    pub(crate) fn can_apply_chamfer_distance(&self) -> bool {
        !self.running() && self.form.is_none() && self.sketch.only_editing_chamfer_distance()
    }

    pub(crate) fn can_cancel(&self) -> bool {
        self.running() && !self.cancel_requested
    }

    /// Keep waiting for the real outcome: the file may already be published.
    pub(crate) fn cancel(&mut self, input: &mut ViewportInput) {
        if !self.can_cancel() {
            return;
        }
        self.cancel_requested = true;
        for creating in &self.running {
            creating.cancel.cancel();
        }
        input.request_redraw();
    }

    /// Puts a form on screen, filled in with what both interfaces offer.
    ///
    /// Always a fresh one: the defaults come from the shared operation, so the
    /// window and the command line cannot suggest different plates.
    fn ask(&mut self) {
        self.opened += 1;
        self.form = Some(NewDocumentForm {
            content: NewContent::Empty,
            width: millimetres(PlateSize::DEFAULT.width),
            depth: millimetres(PlateSize::DEFAULT.depth),
            height: millimetres(PlateSize::DEFAULT.height),
            refusal: None,
            can_create: false,
        });
    }

    /// Opens the drawing for a new document: another opening of New (§30T).
    pub(crate) fn begin_drawing(&mut self) {
        self.opened += 1;
        self.sketch.begin();
    }

    // --- Open while New is not finished (§30T) --------------------------------

    /// Whether the New numbered `over` is the one in progress now.
    fn in_progress(&self, over: NewGeneration) -> bool {
        self.making_new() && over.0 == self.opened
    }

    /// What giving New up throws away, as a clause for the question.
    fn losing(&self) -> &'static str {
        if self.running() {
            "the new document being made will be thrown away, and nothing of it is kept"
        } else if self.sketch.drawing_new() {
            "the sketch you drew for the new document will be discarded"
        } else {
            "the choices and sizes typed in the New form will be discarded"
        }
    }

    /// A file was chosen with Open while New is unfinished: remembers the question
    /// (a newer choice replaces an older one) and throws nothing away. `false`, and
    /// nothing remembered, when no New is in progress.
    pub(crate) fn ask_open(&mut self, path: PathBuf, input: &mut ViewportInput) -> bool {
        if !self.making_new() {
            return false;
        }
        self.asked = Some(AskedOpen {
            path,
            over: NewGeneration(self.opened),
        });
        input.request_redraw();
        true
    }

    /// The question to put to the person: the file, and what discarding costs. Only
    /// for the New in progress; a question about a New that has ended is not shown.
    pub(crate) fn asking(&self) -> Option<(&Path, &'static str)> {
        let asked = self.asked.as_ref()?;
        self.in_progress(asked.over)
            .then(|| (asked.path.as_path(), self.losing()))
    }

    /// *Back to New*: the question is withdrawn and nothing else changes.
    pub(crate) fn keep_new(&mut self, input: &mut ViewportInput) {
        if self.asked.take().is_some() {
            input.request_redraw();
        }
    }

    /// *Discard New and open*: the one moment New's draft may go. The New the
    /// question was asked over, still in progress, ends: its form, its drawing and
    /// its creation. The worker is told to stop and stays accounted until it ends,
    /// and its answer, whenever it comes, is no longer anyone's. Returns the file
    /// the question named. A question about a New that has ended is no decision
    /// about the one now in progress: nothing is stopped, and [`Self::new_ended`]
    /// hands the file back.
    pub(crate) fn stop_new(&mut self, input: &mut ViewportInput) -> Option<PathBuf> {
        if !self
            .asked
            .as_ref()
            .is_some_and(|a| self.in_progress(a.over))
        {
            return None;
        }
        let asked = self.asked.take()?;
        self.form = None;
        self.sketch.dismiss();
        for creating in &self.running {
            creating.cancel.cancel();
        }
        self.current = None;
        self.cancel_requested = false;
        self.status = CreateStatus::Stopped;
        input.request_redraw();
        Some(asked.path)
    }

    /// The New a question was asked over has ended some other way (its Cancel, or a
    /// tab of its own): returns the file to open by the ordinary Open. `None` while
    /// that New is still in progress, or without a question.
    pub(crate) fn new_ended(&mut self) -> Option<PathBuf> {
        let over = self.asked.as_ref()?.over;
        if self.in_progress(over) {
            return None;
        }
        self.asked.take().map(|asked| asked.path)
    }

    /// Takes the form away without making anything.
    fn dismiss(&mut self) -> bool {
        self.form.take().is_some()
    }

    /// Starts a creation, abandoning whatever was already running.
    ///
    /// `spawn` is handed what to put in the document, the generation to label
    /// its answer with and the token that stops it. Starting and recording are
    /// one operation, so there is no arrangement of calls in which a running
    /// worker is untracked. The form stays: it is taken down only when the new
    /// document is accepted, so a failure leaves what was typed in it.
    fn start(
        &mut self,
        content: NewDocument,
        spawn: impl FnOnce(NewDocument, CreateGeneration, &CancelToken) -> JoinHandle<()>,
    ) -> CreateGeneration {
        for creating in &self.running {
            creating.cancel.cancel();
        }
        self.cancel_requested = false;
        self.issued += 1;
        let generation = CreateGeneration(self.issued);
        let cancel = CancelToken::new();
        let worker = spawn(content, generation, &cancel);
        self.running.push(Creating { cancel, worker });
        self.current = Some(generation);
        self.status = CreateStatus::Running { generation };
        generation
    }

    /// Whether this answer is still the one that was asked for.
    fn accepts(&self, generation: CreateGeneration) -> bool {
        self.current == Some(generation)
    }

    /// Notes what a generation answered, and joins whatever has ended.
    ///
    /// The outcome is reported for every answer, current or not, and this
    /// decides what it is worth. An answer to a request that has been replaced,
    /// or one that arrives after Cancel, changes nothing on screen: its
    /// candidate is dropped here, and its private directory with it.
    ///
    /// Returns the candidate to show, when there is one and it is still wanted.
    fn answered(
        &mut self,
        generation: CreateGeneration,
        outcome: Result<Candidate>,
    ) -> (bool, Option<Candidate>) {
        let mut show = None;
        let changed = match &self.status {
            CreateStatus::Running {
                generation: waiting,
            } if *waiting == generation && self.accepts(generation) => {
                self.status = match outcome {
                    Ok(candidate) if !self.cancel_requested => {
                        // The drawing draft is kept aside until the picture is
                        // accepted, and comes back if it is not.
                        self.sketch
                            .draft_published(candidate.session.current().path());
                        show = Some(candidate);
                        CreateStatus::Made
                    }
                    // Cancel was pressed: the answer is "nothing changed".
                    Ok(_) => CreateStatus::Cancelled,
                    // Giving up is not a failure, and a window that reported
                    // it as one would be complaining about something it was
                    // asked to do.
                    Err(error) if error.kind() == ErrorKind::Cancellation => {
                        CreateStatus::Cancelled
                    }
                    Err(error) => CreateStatus::Failed {
                        message: error.to_string(),
                    },
                };
                true
            }
            _ => false,
        };

        if self.accepts(generation) {
            self.current = None;
        }
        // Only the threads that have already finished, which join at once.
        let mut index = 0;
        while index < self.running.len() {
            if self.running[index].worker.is_finished() {
                let done = self.running.swap_remove(index);
                let _ = done.worker.join();
            } else {
                index += 1;
            }
        }
        (changed, show)
    }

    /// What became of the candidate `answered` handed out: shown (the form is
    /// done with), or not (the form stays as it was typed).
    fn shown(&mut self, outcome: std::result::Result<(), String>) {
        if self.status != CreateStatus::Made {
            return;
        }
        self.status = match outcome {
            Ok(()) => {
                self.form = None;
                CreateStatus::Shown
            }
            Err(message) => CreateStatus::Failed { message },
        };
    }

    /// Stops every creation and waits for all of them.
    ///
    /// The one place that blocks, and the last thing that happens. A worker
    /// that was cut short by process exit would leave its scratch directory
    /// beside somebody's documents.
    pub(crate) fn stop_all(&mut self) {
        self.current = None;
        self.form = None;
        self.asked = None;
        for creating in &self.running {
            creating.cancel.cancel();
        }
        // Cancelled first, all of them, and only then waited for.
        for creating in self.running.drain(..) {
            let _ = creating.worker.join();
        }
    }
}

/// A size as the form first shows it.
///
/// `60`, not `60.0`: the field is what somebody is about to type over, and a
/// window that put a decimal point in front of them would be suggesting one
/// matters.
fn millimetres(value: f64) -> String {
    format!("{value}")
}

/// Puts the form on screen, if there is a window to put it in.
///
/// Returns whether anything changed, which is what makes it a reason to draw.
pub(crate) fn open_form(creates: &mut Creates, input: &mut ViewportInput) -> bool {
    // Pressing New while one is being made would start a second document and
    // leave the window to decide which of the two to show. The toolbar already
    // refuses to offer it; this is the same rule where it can be exercised.
    if creates.busy() {
        return false;
    }
    creates.ask();
    input.request_redraw();
    true
}

/// Acts on the answer to the form.
///
/// Returns what a new document should contain, when the person has said they
/// are finished and every size is a number. Everything else — a frame in which
/// nothing was pressed, a form taken back, a size that is not a number — starts
/// nothing, and the last of the three says so inside the form, where the boxes
/// that need fixing are.
///
/// The form is deliberately not taken down here. It stays until the new
/// document is on screen, so a question about unsaved changes that is answered
/// Cancel, or a creation that fails, leaves the sizes that were typed.
pub(crate) fn answer_form(
    creates: &mut Creates,
    input: &mut ViewportInput,
    choice: NewChoice,
) -> Option<NewDocument> {
    match choice {
        // The usual answer on any given frame.
        NewChoice::Waiting => None,
        NewChoice::Cancel => {
            if creates.dismiss() {
                input.request_redraw();
            }
            None
        }
        NewChoice::Create => {
            let form = creates.form.as_mut()?;
            match content_of(form) {
                Ok(content) => {
                    form.refusal = None;
                    Some(content)
                }
                Err(refusal) => {
                    form.refusal = Some(refusal);
                    input.request_redraw();
                    None
                }
            }
        }
    }
}

/// What the form is asking for, or which box is not a number.
///
/// The one place text becomes a size, and it decides nothing else. Whether a
/// number that parses is an acceptable size — how big, whether it may be zero
/// or negative — belongs to the document, which refuses what it will not store
/// and says why; a second set of rules here would be free to disagree with the
/// one that actually applies.
fn content_of(form: &NewDocumentForm) -> std::result::Result<NewDocument, String> {
    match form.content {
        NewContent::Empty => Ok(NewDocument::Empty),
        NewContent::SamplePlate => Ok(NewDocument::SamplePlate(PlateSize {
            width: number("Width", &form.width)?,
            depth: number("Depth", &form.depth)?,
            height: number("Height", &form.height)?,
        })),
    }
}

fn number(field: &str, typed: &str) -> std::result::Result<f64, String> {
    typed
        .trim()
        .parse::<f64>()
        .map_err(|_| not_a_number(field, typed))
}

/// Starts making a new document, and makes the visible change once.
///
/// Returns the generation when a creation really started; one already running
/// starts nothing. No dialog and no destination: the document is made in a
/// candidate session's private directory and named when it is first saved.
pub(crate) fn begin_create(
    creates: &mut Creates,
    input: &mut ViewportInput,
    content: NewDocument,
    spawn: impl FnOnce(NewDocument, CreateGeneration, &CancelToken) -> JoinHandle<()>,
) -> Option<CreateGeneration> {
    if creates.running() {
        return None;
    }
    let generation = creates.start(content, spawn);
    input.request_redraw();
    Some(generation)
}

/// Finishes an answer at the application boundary.
///
/// [`Creates::answered`] is the one generation check, and its answer controls
/// both things visible outside that state machine: a redraw, and the candidate
/// the window goes on to show.
pub(crate) fn finish_create(
    creates: &mut Creates,
    input: &mut ViewportInput,
    generation: CreateGeneration,
    outcome: Result<Candidate>,
) -> Option<Candidate> {
    let (changed, show) = creates.answered(generation, outcome);
    if changed {
        input.request_redraw();
    }
    show
}

/// Records whether the candidate [`finish_create`] handed out was accepted.
pub(crate) fn finish_shown(
    creates: &mut Creates,
    input: &mut ViewportInput,
    outcome: std::result::Result<(), String>,
) {
    creates.shown(outcome);
    input.request_redraw();
}

/// The line this frame borrows from, and nothing when there is nothing to say.
pub(crate) fn words(status: &CreateStatus) -> String {
    status.line()
}

/// What the section shows this frame, or nothing.
pub(crate) fn shown<'a>(status: &CreateStatus, line: &'a str) -> Option<&'a str> {
    if matches!(status, CreateStatus::Idle) {
        None
    } else {
        Some(line)
    }
}

/// Runs a creation away from the event loop and delivers the answer back to it.
///
/// Both halves are arguments so that this can be shown to return while the
/// creation is still running, which is the whole property: the window stays
/// alive while a document is made.
pub(crate) fn spawn_create(
    create: impl FnOnce() -> Result<Candidate> + Send + 'static,
    deliver: impl FnOnce(Result<Candidate>) + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || deliver(create()))
}

/// The whole of the work, and the only place this application does any of it:
/// the shared creation route into a candidate session under `root`, and the
/// picture of it. The kernel is opened for a drawn profile's check and for the
/// picture; Empty and the sample plate are created without one.
pub(crate) fn run_create(
    root: &Path,
    content: NewDocument,
    context: &OperationContext,
) -> Result<Candidate> {
    let (scene, session) = crate::sessions::create_for_view(root, content, context)?;
    Ok(Candidate { scene, session })
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
pub(crate) mod tests {
    use super::*;

    use std::path::PathBuf;
    use std::sync::mpsc;

    use ferritecad_document::{
        CapSide, DependencyRole, Document, EndCondition, ObjectPayload, SelectionRule,
        SemanticRole, SketchGeometry,
    };
    use ferritecad_types::ObjectId;

    /// A reducer with a size, and with the frame that owed itself taken.
    ///
    /// Sizing a viewport is a reason to draw. Taking it here is what lets a
    /// gate say "nothing happened, so no frame was owed" and mean it.
    fn input() -> ViewportInput {
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let _ = input.take_redraw();
        input
    }

    /// Every entry in a directory, sorted, as text.
    fn entries(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(directory)
            .expect("lists the directory")
            .map(|entry| {
                entry
                    .expect("reads an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    /// The candidate a test worker makes: the production `run_create` where this
    /// build has Open CASCADE, and otherwise the same production document route
    /// with its picture read through the mock kernel, so the window's state
    /// machine is exercised on every build. Production itself refuses a picture
    /// without Open CASCADE (see `stub_creation_is_refused_at_the_picture`).
    pub(crate) fn test_candidate(
        root: &Path,
        content: NewDocument,
        context: &OperationContext,
    ) -> Result<Candidate> {
        if ferritecad_occt::is_available() {
            return run_create(root, content, context);
        }
        let session = DocumentSession::create_document_in(
            root,
            ferritecad_jobs::HistoryLimits::default(),
            content,
            ferritecad_occt::OcctKernel::new,
            context,
        )?;
        let scene = ferritecad_scene::snapshot_of(
            session.current().path(),
            &mut ferritecad_kernel::mock::MockKernel::new(),
            |_, _| {
                Err(ferritecad_types::CadError::unsupported(
                    "no STEP in this gate",
                ))
            },
            &ferritecad_kernel::TessellationParams::default(),
            context,
        )?;
        Ok(Candidate { scene, session })
    }

    /// Drives one creation the way the event loop does, with the real work
    /// behind it and no window anywhere: the same [`crate::start_new`] the frame
    /// calls, a worker making a candidate under `root`, and the answer delivered
    /// back through [`finish_create`]. What is left out is the winit event and the
    /// picture's upload, which the caller decides with [`finish_shown`].
    pub(crate) fn run_to_completion(
        creates: &mut Creates,
        input: &mut ViewportInput,
        root: &Path,
        content: NewDocument,
    ) -> (Option<CreateGeneration>, Option<Candidate>) {
        let (answers, answered) = mpsc::channel();
        let root = root.to_path_buf();
        let Some(generation) = crate::start_new(
            creates,
            &crate::Loads::default(),
            &crate::exports::Exports::default(),
            input,
            content,
            move |content, generation, cancel| {
                let context = OperationContext::default().with_cancel(cancel.clone());
                spawn_create(
                    move || test_candidate(&root, content, &context),
                    move |result| {
                        let _ = answers.send((generation, result));
                    },
                )
            },
        ) else {
            return (None, None);
        };
        let (answered_generation, result) = answered.recv().expect("the creation answered");
        let candidate = finish_create(creates, input, answered_generation, result);
        (Some(generation), candidate)
    }

    /// A creation whose worker is held at a barrier (§30T), started through the
    /// window's own [`crate::start_new`]. `made_first`: the worker has already made
    /// its candidate, and so its folder, when it waits.
    pub(crate) struct Held {
        pub(crate) generation: CreateGeneration,
        reached: mpsc::Receiver<()>,
        release: mpsc::Sender<()>,
        answered: mpsc::Receiver<(CreateGeneration, Result<Candidate>)>,
    }

    impl Held {
        /// Lets the worker go on and returns its answer undelivered, as the event
        /// loop holds it between the worker's end and the `Created` handler.
        pub(crate) fn finish(self) -> (CreateGeneration, Result<Candidate>) {
            let _ = self.release.send(());
            self.answered.recv().expect("the creation answered")
        }

        /// Blocks until the worker stands at its barrier.
        pub(crate) fn at_barrier(&self) {
            self.reached.recv().expect("the worker reached its barrier");
        }
    }

    pub(crate) fn hold_creation(
        creates: &mut Creates,
        input: &mut ViewportInput,
        root: &Path,
        content: NewDocument,
        made_first: bool,
    ) -> Held {
        let (ready, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel::<()>();
        let (answers, answered) = mpsc::channel();
        let root = root.to_path_buf();
        let generation = crate::start_new(
            creates,
            &crate::Loads::default(),
            &crate::exports::Exports::default(),
            input,
            content,
            move |content, generation, cancel| {
                let context = OperationContext::default().with_cancel(cancel.clone());
                spawn_create(
                    move || {
                        if made_first {
                            let made = test_candidate(&root, content, &context);
                            let _ = ready.send(());
                            let _ = resume.recv();
                            made
                        } else {
                            let _ = ready.send(());
                            let _ = resume.recv();
                            test_candidate(&root, content, &context)
                        }
                    },
                    move |result| {
                        let _ = answers.send((generation, result));
                    },
                )
            },
        )
        .expect("the creation started");
        Held {
            generation,
            reached,
            release,
            answered,
        }
    }

    /// The first Save of an untitled session: the production Save As.
    pub(crate) fn save_first(session: &mut DocumentSession, destination: &Path) {
        let saved = session
            .begin_save(ferritecad_jobs::SaveTarget::As(destination.to_path_buf()))
            .run(&OperationContext::default())
            .expect("the first save is published");
        session.record_saved(&saved);
    }

    /// §30L: a drawing form's request through the window's route: the worker
    /// makes a candidate, the draft is held aside while its picture is prepared,
    /// a refused picture gives the draft back without resubmitting it
    /// (`restored` inspects it then), and an accepted one retires it. The
    /// accepted document is then saved for the first time at `destination`.
    pub(crate) fn drawn_through_the_window(
        creates: &mut Creates,
        view: &mut ViewportInput,
        content: NewDocument,
        destination: &Path,
        restored: impl FnOnce(&crate::sketch::Editor),
    ) {
        let sessions = tempfile::tempdir().expect("sessions");
        let (_, candidate) = run_to_completion(creates, view, sessions.path(), content.clone());
        let mut candidate = candidate.expect("a candidate");
        // Another request while this one is held is not a second worker.
        let shown = candidate.session.current().path().to_path_buf();
        assert!(!creates.sketch.active(), "the draft is held aside");
        creates
            .sketch
            .draft_load_finished(Path::new("unrelated.fcad"), false);
        assert!(!creates.sketch.active());
        creates.sketch.draft_load_finished(&shown, false);
        assert!(
            creates.sketch.active(),
            "a refused picture restores the draft"
        );
        assert!(creates.sketch.take_request().is_none(), "no resubmission");
        restored(&creates.sketch);
        creates.sketch.draft_published(&shown);
        creates.sketch.draft_load_finished(&shown, true);
        assert!(!creates.sketch.active());
        finish_shown(creates, view, Ok(()));
        assert!(candidate.session.is_untitled());
        save_first(&mut candidate.session, destination);
    }

    /// The whole of the window's route, from the button to the file, as one
    /// call: press New, fill the form in, let the candidate be made and its
    /// picture accepted, then Save it for the first time at `destination`.
    fn create_through_the_window(
        destination: &Path,
        content: NewContent,
        sizes: [&str; 3],
    ) -> (Creates, Option<PathBuf>) {
        let mut creates = Creates::default();
        let mut input = input();
        let sessions = tempfile::tempdir().expect("sessions");

        assert!(crate::ask_new(
            &mut creates,
            &crate::Loads::default(),
            &crate::exports::Exports::default(),
            &mut input
        ));
        let form = creates.form().expect("the form is on screen");
        form.content = content;
        form.width = sizes[0].to_owned();
        form.depth = sizes[1].to_owned();
        form.height = sizes[2].to_owned();

        let asked =
            answer_form(&mut creates, &mut input, NewChoice::Create).expect("the form is complete");
        let (_, candidate) = run_to_completion(&mut creates, &mut input, sessions.path(), asked);
        let Some(mut candidate) = candidate else {
            return (creates, None);
        };
        finish_shown(&mut creates, &mut input, Ok(()));
        save_first(&mut candidate.session, destination);
        (creates, Some(destination.to_path_buf()))
    }

    // ------------------------------------------------ what starts a creation

    /// Nothing is on screen and nothing is made until somebody asks.
    #[test]
    fn nothing_is_asked_before_new_is_pressed() {
        let mut creates = Creates::default();
        let mut input = input();

        assert!(creates.form().is_none());
        assert_eq!(creates.status(), &CreateStatus::Idle);
        assert!(answer_form(&mut creates, &mut input, NewChoice::Create).is_none());
        assert!(answer_form(&mut creates, &mut input, NewChoice::Waiting).is_none());
        assert!(!input.take_redraw());
    }

    /// The form offers exactly what the command line offers.
    #[test]
    fn the_form_offers_the_same_plate_the_command_line_does() {
        let mut creates = Creates::default();
        let mut input = input();

        assert!(open_form(&mut creates, &mut input));
        assert!(input.take_redraw(), "a form appearing owes a frame");

        let form = creates.form().expect("the form is on screen");
        // Empty by default: a new document is the smaller promise, and the
        // plate is a template somebody chooses rather than one they opt out of.
        assert_eq!(form.content, NewContent::Empty);
        assert_eq!(form.width, "60");
        assert_eq!(form.depth, "40");
        assert_eq!(form.height, "10");
        assert_eq!(form.refusal, None);
    }

    /// A form taken back makes nothing and says nothing.
    #[test]
    fn a_form_taken_back_makes_nothing() {
        let mut creates = Creates::default();
        let mut input = input();

        open_form(&mut creates, &mut input);
        let _ = input.take_redraw();
        assert!(answer_form(&mut creates, &mut input, NewChoice::Cancel).is_none());

        assert!(creates.form().is_none());
        assert_eq!(creates.status(), &CreateStatus::Idle);
        assert!(input.take_redraw(), "a form going away owes a frame");
    }

    /// A size that is not a number is said where the boxes are, and nothing
    /// is started.
    #[test]
    fn a_size_that_is_not_a_number_is_reported_in_the_form() {
        let mut creates = Creates::default();
        let mut input = input();

        open_form(&mut creates, &mut input);
        let form = creates.form().expect("the form is on screen");
        form.content = NewContent::SamplePlate;
        form.depth = "forty".to_owned();
        let _ = input.take_redraw();

        assert!(answer_form(&mut creates, &mut input, NewChoice::Create).is_none());

        let form = creates.form().expect("the form is still on screen");
        let refusal = form.refusal.clone().expect("the form says what is wrong");
        assert!(refusal.contains("Depth"), "{refusal}");
        assert!(refusal.contains("forty"), "{refusal}");
        // The status line is about creating documents, and none was asked for.
        assert_eq!(creates.status(), &CreateStatus::Idle);
    }

    /// §30L: the sizes stay on screen until the new document is accepted; a
    /// picture that could not be shown leaves them, and only acceptance takes the
    /// form down. No file is written anywhere a person keeps files.
    #[test]
    fn the_form_stays_until_the_new_document_is_accepted() {
        let sessions = tempfile::tempdir().expect("sessions");
        let mut creates = Creates::default();
        let mut input = input();

        open_form(&mut creates, &mut input);
        let form = creates.form().expect("the form is on screen");
        form.content = NewContent::SamplePlate;
        form.width = "125".to_owned();
        let content =
            answer_form(&mut creates, &mut input, NewChoice::Create).expect("the form is complete");

        let (generation, candidate) =
            run_to_completion(&mut creates, &mut input, sessions.path(), content.clone());
        assert!(generation.is_some());
        let candidate = candidate.expect("a candidate");
        assert_eq!(creates.status(), &CreateStatus::Made);
        assert_eq!(creates.form().expect("still typed").width, "125");
        // The picture could not be uploaded: nothing is accepted, the form stays.
        drop(candidate);
        finish_shown(&mut creates, &mut input, Err("no device".to_owned()));
        assert!(
            matches!(creates.status(), CreateStatus::Failed { message } if message == "no device")
        );
        assert_eq!(creates.form().expect("kept").width, "125");
        assert!(
            entries(sessions.path()).is_empty(),
            "the dropped candidate left files"
        );

        let (_, candidate) = run_to_completion(&mut creates, &mut input, sessions.path(), content);
        let candidate = candidate.expect("a candidate");
        finish_shown(&mut creates, &mut input, Ok(()));
        assert_eq!(creates.status(), &CreateStatus::Shown);
        assert!(creates.form().is_none(), "accepted: the form is done with");
        let line = words(creates.status());
        assert_eq!(line, "Created a new document.");
        assert!(
            !line.contains(&candidate.session.private_directory().display().to_string()),
            "{line}"
        );
    }

    // ------------------------------------------------------ what it produces

    /// The window's route makes an untitled session with no file, holding the
    /// plate the form asked for.
    #[test]
    fn the_window_route_makes_an_untitled_session_and_writes_no_file() {
        let sessions = tempfile::tempdir().expect("sessions");
        let mut creates = Creates::default();
        let mut input = input();

        let (_, candidate) = run_to_completion(
            &mut creates,
            &mut input,
            sessions.path(),
            NewDocument::SamplePlate(PlateSize {
                width: 80.0,
                depth: 50.0,
                height: 12.0,
            }),
        );
        let candidate = candidate.expect("a candidate");
        let session = &candidate.session;
        assert_eq!(session.logical_path(), None);
        assert_eq!(session.saved_version(), None);
        assert_eq!(session.display_name(), "Untitled");
        assert!(session.is_dirty());
        assert!(session.owns(session.current().path()));
        let document =
            Document::open_read_only(session.current().path()).expect("opens what was made");
        assert_eq!(document.objects().expect("reads objects").len(), 4);
        assert_eq!(document.topology_refs().expect("reads refs").len(), 3);
        document.close().expect("closes");
        assert_eq!(entries(sessions.path()).len(), 1, "one private folder");
    }

    /// And an empty document, which is a different thing from an empty window:
    /// it has a session, and it is unsaved.
    #[test]
    fn an_empty_document_is_a_document() {
        let sessions = tempfile::tempdir().expect("sessions");
        let mut creates = Creates::default();
        let mut input = input();

        let (_, candidate) = run_to_completion(
            &mut creates,
            &mut input,
            sessions.path(),
            NewDocument::Empty,
        );
        let candidate = candidate.expect("a candidate");
        assert!(candidate.session.is_untitled() && candidate.session.is_dirty());
        let document =
            Document::open_read_only(candidate.session.current().path()).expect("opens it");
        assert!(document.objects().expect("reads objects").is_empty());
        document.close().expect("closes");
    }

    // ------------------------------------------------------- what it refuses

    /// The first Save refuses a name that is taken, in its own words, and the
    /// untitled document stays exactly as it was.
    #[test]
    fn a_taken_name_is_refused_by_the_first_save_without_a_replacement() {
        let sessions = tempfile::tempdir().expect("sessions");
        let dir = tempfile::tempdir().expect("user");
        let destination = dir.path().join("plate.fcad");
        std::fs::write(&destination, b"somebody else's file").expect("writes the file");
        let mut creates = Creates::default();
        let mut input = input();
        let (_, candidate) = run_to_completion(
            &mut creates,
            &mut input,
            sessions.path(),
            NewDocument::Empty,
        );
        let candidate = candidate.expect("a candidate");
        let failure = candidate
            .session
            .begin_save(ferritecad_jobs::SaveTarget::As(destination.clone()))
            .run(&OperationContext::default())
            .expect_err("occupied");
        assert_eq!(failure.kind, ferritecad_jobs::SaveFailureKind::Occupied);
        let message = failure.to_string();
        assert!(!message.contains("--force"), "{message}");
        assert_eq!(
            std::fs::read(&destination).expect("reads it"),
            b"somebody else's file"
        );
        assert!(candidate.session.is_untitled());
        assert_eq!(entries(dir.path()), vec!["plate.fcad".to_owned()]);
    }

    /// A size the document will not store leaves nothing behind, and the
    /// window says so without pretending a document is there.
    ///
    /// `NaN` is a number as far as parsing goes, so it reaches the document
    /// and is refused by the same rule the command line meets.
    #[test]
    fn a_size_the_document_refuses_leaves_nothing_behind() {
        let sessions = tempfile::tempdir().expect("sessions");
        let mut creates = Creates::default();
        let mut input = input();
        let (_, candidate) = run_to_completion(
            &mut creates,
            &mut input,
            sessions.path(),
            NewDocument::SamplePlate(PlateSize {
                width: 60.0,
                depth: 40.0,
                height: f64::NAN,
            }),
        );
        assert!(candidate.is_none());
        let CreateStatus::Failed { message } = creates.status() else {
            panic!("the window does not say it failed: {:?}", creates.status());
        };
        assert!(message.contains("must be finite"), "{message}");
        assert!(
            entries(sessions.path()).is_empty(),
            "a refused creation left {:?}",
            entries(sessions.path())
        );
    }

    /// An answer to a creation that has been replaced changes nothing at all,
    /// and its candidate is dropped with its private folder.
    #[test]
    fn a_stale_answer_is_not_acted_on() {
        let sessions = tempfile::tempdir().expect("sessions");
        let mut creates = Creates::default();
        let mut input = input();

        let (stale, first) = run_to_completion(
            &mut creates,
            &mut input,
            sessions.path(),
            NewDocument::Empty,
        );
        let stale = stale.expect("the first creation started");
        drop(first);
        let (_, second) = run_to_completion(
            &mut creates,
            &mut input,
            sessions.path(),
            NewDocument::Empty,
        );
        let second = second.expect("the second candidate");
        finish_shown(&mut creates, &mut input, Ok(()));
        let _ = input.take_redraw();

        // The first request's answer, arriving after the second replaced it.
        let late = finish_create(
            &mut creates,
            &mut input,
            stale,
            test_candidate(
                sessions.path(),
                NewDocument::Empty,
                &OperationContext::default(),
            ),
        );
        assert!(late.is_none(), "a stale answer asked to be shown");
        assert_eq!(creates.status(), &CreateStatus::Shown);
        assert!(!input.take_redraw(), "a stale answer owed a frame");
        assert_eq!(
            entries(sessions.path()).len(),
            1,
            "only the accepted candidate's folder remains"
        );
        drop(second);
    }

    /// New is unavailable exactly while a document is being made.
    #[test]
    fn a_creation_in_flight_is_the_only_thing_that_withdraws_new() {
        let sessions = tempfile::tempdir().expect("sessions");
        let root = sessions.path().to_path_buf();
        let mut creates = Creates::default();
        let mut input = input();

        assert!(!creates.running());
        let (answers, answered) = mpsc::channel();
        begin_create(
            &mut creates,
            &mut input,
            NewDocument::Empty,
            move |content, generation, cancel| {
                let context = OperationContext::default().with_cancel(cancel.clone());
                spawn_create(
                    move || test_candidate(&root, content, &context),
                    move |result| {
                        let _ = answers.send((generation, result));
                    },
                )
            },
        )
        .expect("the creation started");

        assert!(
            creates.running(),
            "New stays available while one is running"
        );
        // And pressing it anyway does nothing, so the rule holds wherever the
        // press came from rather than only in a disabled button.
        assert!(!open_form(&mut creates, &mut input));
        assert!(creates.form().is_none());

        let (generation, result) = answered.recv().expect("the creation answered");
        drop(finish_create(&mut creates, &mut input, generation, result));
        assert!(!creates.running());
        creates.stop_all();
    }

    /// §30L stub: without Open CASCADE the production route still makes the
    /// kernel-free document, but no picture can be read, so nothing is accepted
    /// and the candidate's folder is gone.
    #[test]
    fn stub_creation_is_refused_at_the_picture() {
        if ferritecad_occt::is_available() {
            eprintln!("skipped: requires stub");
            return;
        }
        let sessions = tempfile::tempdir().expect("sessions");
        let error = run_create(
            sessions.path(),
            NewDocument::Empty,
            &OperationContext::default(),
        )
        .expect_err("no picture without a kernel");
        assert_eq!(error.kind(), ErrorKind::Unsupported, "{error}");
        assert!(entries(sessions.path()).is_empty());
        // The document itself needs no kernel, as before.
        let session = DocumentSession::create_document_in(
            sessions.path(),
            ferritecad_jobs::HistoryLimits::default(),
            NewDocument::SamplePlate(PlateSize::DEFAULT),
            ferritecad_occt::OcctKernel::new,
            &OperationContext::default(),
        )
        .expect("kernel-free creation");
        assert!(session.is_untitled());
    }

    // ------------------------------------------------------------ two clients

    /// The `ferritecad` this test was built alongside.
    pub(crate) fn ferritecad() -> PathBuf {
        // `current_exe` is target/<profile>/deps/<test>; the binary is two up.
        let mut path = std::env::current_exe().expect("the test knows where it is");
        path.pop();
        path.pop();
        path.push(format!("ferritecad{}", std::env::consts::EXE_SUFFIX));
        path
    }

    /// One plate, made by the real command-line process.
    ///
    /// Not a library call dressed up as one: this starts the shipped binary
    /// with the arguments a person would type. A gate that called the shared
    /// operation twice would go on passing while either client stopped
    /// reaching it.
    fn create_through_the_command_line(destination: &Path, size: [&str; 3]) {
        let binary = ferritecad();
        assert!(
            binary.is_file(),
            "{} is not built; this gate compares the two clients and needs both, so build the \
             workspace (cargo build --workspace --all-targets) before running it",
            binary.display()
        );
        let output = std::process::Command::new(&binary)
            .arg("create")
            .arg(destination)
            .arg("--sample")
            .arg("--size")
            .args(size)
            .output()
            .expect("the command runs");
        assert!(
            output.status.success(),
            "the command line refused: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Everything about a document that two independent creations must agree
    /// on, with every identity replaced by what it is linked to.
    ///
    /// Identities are not deleted: they are resolved. An object is named by
    /// what the document calls it, a sketch segment by its place in the
    /// profile, and every reference to either is rewritten the same way — so a
    /// reference that pointed at the wrong segment, an edge that pointed at
    /// the wrong object, or a role attached to the wrong feature all change
    /// this value. Two graphs with their identity fields simply removed would
    /// compare equal however their links were rearranged.
    #[derive(Debug, PartialEq)]
    pub(crate) struct Semantics {
        /// Display units, which are metadata a person chose rather than model.
        units: (String, String),
        payloads: Vec<String>,
        /// Each object as `(name, ordinal, payload in resolved terms)`.
        objects: Vec<(String, i64, String)>,
        /// Each edge as `(dependent name, dependency name, role)`.
        dependencies: Vec<(String, String, String)>,
        /// Each stored reference in resolved terms.
        references: Vec<String>,
    }

    /// The identities themselves, which two independent creations must *not*
    /// share.
    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct Identities {
        document: String,
        objects: Vec<String>,
        segments: Vec<String>,
        references: Vec<String>,
    }

    pub(crate) fn read_semantics(path: &Path) -> (Semantics, Identities) {
        let document = Document::open(path).expect("opens the document");
        let objects = document.objects().expect("reads objects");

        // What each object is called, which is what a reference to it is
        // resolved to below. The plate names all four of its objects, and two
        // objects sharing a name would make this ambiguous rather than wrong —
        // so that is refused here rather than papered over.
        let mut named: Vec<(ObjectId, String)> = objects
            .iter()
            .map(|object| {
                (
                    object.id,
                    object
                        .name
                        .clone()
                        .unwrap_or_else(|| "(unnamed)".to_owned()),
                )
            })
            .collect();
        named.sort_by(|a, b| a.1.cmp(&b.1));
        let mut distinct: Vec<&String> = named.iter().map(|(_, name)| name).collect();
        distinct.dedup();
        assert_eq!(distinct.len(), named.len(), "two objects share a name");
        let name_of = |id: ObjectId| -> String {
            named
                .iter()
                .find(|(other, _)| *other == id)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| format!("(not in this document: {id})"))
        };

        // Where each sketch segment sits in the profile that holds it. A
        // reference naming a segment is resolved to that, so a reference moved
        // to another segment changes what is compared.
        let mut segment_at = Vec::new();
        for object in &objects {
            if let ObjectPayload::Sketch(sketch) = &object.payload {
                for (index, curve) in sketch.curves.iter().enumerate() {
                    segment_at.push((curve.id, format!("{}/segment {index}", name_of(object.id))));
                }
            }
        }
        let segment_of = |id: ferritecad_types::StableEntityId| -> String {
            segment_at
                .iter()
                .find(|(other, _)| *other == id)
                .map(|(_, place)| place.clone())
                .unwrap_or_else(|| format!("(no such segment: {id})"))
        };

        let mut described: Vec<(String, i64, String)> = objects
            .iter()
            .map(|object| {
                let payload = match &object.payload {
                    ObjectPayload::DatumPlane(plane) => {
                        format!("datum plane at {:?}", plane.placement)
                    }
                    ObjectPayload::Sketch(sketch) => {
                        let geometry: Vec<String> = sketch
                            .curves
                            .iter()
                            .map(|curve| match curve.geometry {
                                SketchGeometry::Line { start, end } => format!(
                                    "line ({}, {}) -> ({}, {})",
                                    start.x, start.y, end.x, end.y
                                ),
                                ref other => format!("{other:?}"),
                            })
                            .collect();
                        format!(
                            "sketch on {} with {} constraint(s): {}",
                            name_of(sketch.plane),
                            sketch.constraints.len(),
                            geometry.join(", ")
                        )
                    }
                    ObjectPayload::Extrude(extrude) => {
                        let end = match &extrude.end_condition {
                            EndCondition::Blind { distance } => {
                                format!("blind {} mm", distance.value())
                            }
                            other => format!("{other:?}"),
                        };
                        format!(
                            "extrude of {} {end} reversed={} operation={:?} target={:?}",
                            name_of(extrude.profile),
                            extrude.reversed,
                            extrude.operation,
                            extrude.target_body.map(name_of),
                        )
                    }
                    ObjectPayload::Body(body) => {
                        format!("body tipped by {:?}", body.tip_feature.map(name_of))
                    }
                    ObjectPayload::Revolve(revolve) => format!(
                        "revolve of {} about {:?} by {:?} operation={:?}",
                        name_of(revolve.profile),
                        revolve.axis,
                        revolve.extent,
                        revolve.operation,
                    ),
                    other => format!("{other:?}"),
                };
                (
                    object
                        .name
                        .clone()
                        .unwrap_or_else(|| "(unnamed)".to_owned()),
                    object.ordinal,
                    payload,
                )
            })
            .collect();
        described.sort();

        let mut dependencies: Vec<(String, String, String)> = document
            .dependencies()
            .expect("reads dependencies")
            .iter()
            .map(|edge| {
                (
                    name_of(edge.dependent),
                    name_of(edge.dependency),
                    match edge.role {
                        DependencyRole::Plane => "plane".to_owned(),
                        DependencyRole::Profile => "profile".to_owned(),
                        DependencyRole::BodyTip => "body tip".to_owned(),
                        ref other => format!("{other:?}"),
                    },
                )
            })
            .collect();
        dependencies.sort();

        let stored = document.topology_refs().expect("reads references");
        let mut references: Vec<String> = stored
            .iter()
            .map(|reference| {
                let role = match reference.output_role {
                    SemanticRole::ExtrudeCap { side } => format!(
                        "cap {}",
                        match side {
                            CapSide::Start => "start".to_owned(),
                            CapSide::End => "end".to_owned(),
                            // A cap this build does not know is still a fact
                            // about the document, and two clients must agree
                            // about it rather than have it silently dropped.
                            ref other => format!("{other:?}"),
                        }
                    ),
                    SemanticRole::ExtrudeSide { profile_segment } => {
                        format!("side of {}", segment_of(profile_segment))
                    }
                    SemanticRole::RevolveFace { profile_segment } => {
                        format!("face turned from {}", segment_of(profile_segment))
                    }
                    ref other => format!("{other:?}"),
                };
                let selection = match reference.selection {
                    SelectionRule::Exact => "exact".to_owned(),
                    SelectionRule::AllDerivedFrom { ancestor } => {
                        format!("all derived from {}", segment_of(ancestor))
                    }
                    ref other => format!("{other:?}"),
                };
                format!(
                    "{} on {} from {} expecting {:?} by {selection} fallback={:?}",
                    role,
                    name_of(reference.owner),
                    name_of(reference.producer_feature),
                    reference.expected_kind,
                    reference.fallback_signature,
                )
            })
            .collect();
        references.sort();

        let mut payloads: Vec<String> = objects
            .iter()
            .map(|object| {
                let mut full =
                    format!("{:?}/{:?}/{:?}", object.name, object.parent, object.payload);
                for (id, name) in &named {
                    full = full.replace(&id.to_string(), &format!("object:{name}"));
                }
                for (id, place) in &segment_at {
                    full = full.replace(&id.to_string(), &format!("segment:{place}"));
                }
                full
            })
            .collect();
        payloads.sort();
        let meta = document.meta();
        let semantics = Semantics {
            payloads,
            units: (
                meta.display_length_unit.symbol().to_owned(),
                meta.display_angle_unit.symbol().to_owned(),
            ),
            objects: described,
            dependencies,
            references,
        };

        let mut object_ids: Vec<String> =
            objects.iter().map(|object| object.id.to_string()).collect();
        object_ids.sort();
        let mut segment_ids: Vec<String> =
            segment_at.iter().map(|(id, _)| id.to_string()).collect();
        segment_ids.sort();
        let mut reference_ids: Vec<String> = stored
            .iter()
            .map(|reference| reference.id.to_string())
            .collect();
        reference_ids.sort();
        let identities = Identities {
            document: meta.document_id.to_string(),
            objects: object_ids,
            segments: segment_ids,
            references: reference_ids,
        };

        document.close().expect("closes");
        (semantics, identities)
    }

    /// The two clients write the same plate, and two different documents.
    ///
    /// One is the shipped command run as a process; the other is the window's
    /// own route, driven the way the event loop drives it. What must agree is
    /// everything the document means — the objects, their sizes, their
    /// dependencies, the roles of the stored references and which profile
    /// segment each one is about — and what must differ is every identity,
    /// because two documents are two documents.
    #[test]
    fn both_clients_write_the_same_plate() {
        let dir = tempfile::tempdir().expect("temp dir");
        let from_command_line = dir.path().join("cli.fcad");
        let from_window = dir.path().join("ui.fcad");

        create_through_the_command_line(&from_command_line, ["80", "50", "12"]);
        let (_, open) =
            create_through_the_window(&from_window, NewContent::SamplePlate, ["80", "50", "12"]);
        assert_eq!(open.as_deref(), Some(from_window.as_path()));

        let (command_line, command_line_ids) = read_semantics(&from_command_line);
        let (window, window_ids) = read_semantics(&from_window);

        assert_eq!(command_line, window);
        // And the plate really is the one that was asked for, so an agreement
        // between two empty readings cannot pass for one.
        assert_eq!(command_line.objects.len(), 4);
        assert_eq!(command_line.dependencies.len(), 3);
        assert_eq!(command_line.references.len(), 3);
        assert!(
            command_line
                .objects
                .iter()
                .any(|(_, _, payload)| payload.contains("(80, 50)")),
            "{:?}",
            command_line.objects
        );
        assert!(
            command_line
                .objects
                .iter()
                .any(|(_, _, payload)| payload.contains("blind 12 mm")),
            "{:?}",
            command_line.objects
        );

        // Every identity differs. Two documents made from one description are
        // two documents, and the comparison above resolved links rather than
        // discarding them — which is only worth anything if the raw values
        // were never equal to begin with.
        assert_ne!(command_line_ids.document, window_ids.document);
        for (mine, theirs) in [
            (&command_line_ids.objects, &window_ids.objects),
            (&command_line_ids.segments, &window_ids.segments),
            (&command_line_ids.references, &window_ids.references),
        ] {
            assert_eq!(mine.len(), theirs.len());
            assert!(!mine.is_empty());
            for identity in mine {
                assert!(
                    !theirs.contains(identity),
                    "the two documents share the identity {identity}"
                );
            }
        }
    }

    /// And an empty document, made by both clients, is the same empty
    /// document.
    #[test]
    fn both_clients_write_the_same_empty_document() {
        let dir = tempfile::tempdir().expect("temp dir");
        let from_command_line = dir.path().join("cli.fcad");
        let from_window = dir.path().join("ui.fcad");

        let binary = ferritecad();
        assert!(binary.is_file(), "{} is not built", binary.display());
        let output = std::process::Command::new(&binary)
            .arg("create")
            .arg(&from_command_line)
            .output()
            .expect("the command runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );

        create_through_the_window(&from_window, NewContent::Empty, ["60", "40", "10"]);

        let (command_line, command_line_ids) = read_semantics(&from_command_line);
        let (window, window_ids) = read_semantics(&from_window);

        assert_eq!(command_line, window);
        assert!(command_line.objects.is_empty());
        assert_eq!(command_line.units, ("mm".to_owned(), "deg".to_owned()));
        assert_ne!(command_line_ids.document, window_ids.document);
    }
    #[test]
    fn cancel_waits_for_the_worker_and_accepts_nothing_it_made() {
        for late in [false, true] {
            let sessions = tempfile::tempdir().expect("sessions");
            let root = sessions.path().to_path_buf();
            let mut creates = Creates::default();
            let mut input = input();
            let (ready, reached) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            let (answers, answered) = mpsc::channel();
            begin_create(
                &mut creates,
                &mut input,
                NewDocument::Empty,
                move |content, generation, cancel| {
                    let context = OperationContext::default().with_cancel(cancel.clone());
                    spawn_create(
                        move || {
                            if late {
                                let result = test_candidate(&root, content, &context);
                                ready.send(()).expect("ready");
                                resume.recv().expect("release");
                                result
                            } else {
                                ready.send(()).expect("ready");
                                resume.recv().expect("release");
                                test_candidate(&root, content, &context)
                            }
                        },
                        move |result| {
                            answers.send((generation, result)).expect("answer");
                        },
                    )
                },
            )
            .expect("started");
            reached.recv().expect("worker reached barrier");
            creates.cancel(&mut input);
            assert!(creates.running(), "the outcome is not known yet");
            assert!(!creates.can_cancel());
            release.send(()).expect("resume");
            let (generation, result) = answered.recv().expect("answer");
            assert!(finish_create(&mut creates, &mut input, generation, result).is_none());
            assert_eq!(creates.status(), &CreateStatus::Cancelled);
            assert!(
                entries(sessions.path()).is_empty(),
                "a cancelled candidate left its folder (late={late})"
            );
            creates.stop_all();
            assert!(creates.running.is_empty());
        }
    }

    #[test]
    fn shutdown_cancels_and_joins_creation_with_its_scratch_removed() {
        let sessions = tempfile::tempdir().expect("sessions");
        let root = sessions.path().to_path_buf();
        let mut creates = Creates::default();
        let mut input = input();
        let (send, reached) = mpsc::channel();
        let (answer, answered) = mpsc::channel();
        begin_create(
            &mut creates,
            &mut input,
            NewDocument::Empty,
            move |content, _, cancel| {
                let waiting = cancel.clone();
                let context = OperationContext::default()
                    .with_cancel(cancel.clone())
                    .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                        if fraction == 0.9 {
                            send.send(()).expect("scratch ready");
                            let deadline =
                                std::time::Instant::now() + std::time::Duration::from_secs(5);
                            while !waiting.is_cancelled() {
                                assert!(
                                    std::time::Instant::now() < deadline,
                                    "shutdown never cancelled creation"
                                );
                                std::thread::sleep(std::time::Duration::from_millis(1));
                            }
                        }
                    }));
                spawn_create(
                    move || test_candidate(&root, content, &context),
                    move |result| {
                        answer.send(result.map(|_| ())).expect("answer");
                    },
                )
            },
        )
        .expect("started");
        reached.recv().expect("scratch is built");
        creates.stop_all();
        assert!(creates.running.is_empty(), "shutdown detached a worker");
        assert_eq!(
            answered
                .try_recv()
                .expect("joined worker answered")
                .expect_err("cancelled")
                .kind(),
            ErrorKind::Cancellation
        );
        assert!(entries(sessions.path()).is_empty());
    }

    // ------------------------------------------- Open while New is unfinished

    /// A question belongs to the New it was asked over: a newer choice replaces an
    /// older one, a New that ended (its Cancel) no longer puts it, and a later New —
    /// even one that began before the question was settled — is never discarded by it.
    #[test]
    fn a_question_is_bound_to_the_new_it_was_asked_over() {
        let mut creates = Creates::default();
        let mut input = input();
        let b = PathBuf::from("b.fcad");
        let c = PathBuf::from("c.fcad");

        assert!(
            !creates.ask_open(b.clone(), &mut input),
            "no New, no question"
        );
        assert!(creates.asking().is_none() && creates.new_ended().is_none());
        assert!(creates.stop_new(&mut input).is_none());

        assert!(open_form(&mut creates, &mut input));
        {
            let form = creates.form().expect("New's form");
            form.content = NewContent::SamplePlate;
            form.width = "1e999x".to_owned();
        }
        assert!(creates.ask_open(b, &mut input));
        assert!(
            creates.ask_open(c.clone(), &mut input),
            "a newer choice replaces it"
        );
        let (asked, losing) = creates.asking().expect("the question is put");
        assert_eq!(asked, c);
        assert!(losing.contains("typed in the New form"), "{losing}");
        assert!(creates.new_ended().is_none(), "New is still in progress");
        // Back to New: withdrawn, nothing else.
        creates.keep_new(&mut input);
        assert!(creates.asking().is_none());
        assert_eq!(creates.form().expect("kept").width, "1e999x");

        // New's own Cancel ends this New: the question is no longer put, and the
        // file it named is handed back for the ordinary Open.
        assert!(creates.ask_open(c.clone(), &mut input));
        assert!(answer_form(&mut creates, &mut input, NewChoice::Cancel).is_none());
        assert!(
            creates.asking().is_none(),
            "a question about a New that ended"
        );
        assert_eq!(creates.new_ended(), Some(c.clone()));
        assert!(creates.new_ended().is_none(), "answered once");

        // A question whose New ended without anyone settling it must not discard the
        // next New: a stop finds nothing to stop, and the file is handed back for the
        // ordinary Open to decide again.
        assert!(open_form(&mut creates, &mut input));
        assert!(creates.ask_open(c.clone(), &mut input));
        assert!(answer_form(&mut creates, &mut input, NewChoice::Cancel).is_none());
        assert!(open_form(&mut creates, &mut input));
        {
            let form = creates.form().expect("New #2's form");
            form.content = NewContent::SamplePlate;
            form.depth = "-".to_owned();
        }
        assert_eq!(creates.stop_new(&mut input), None);
        let form = creates.form().expect("New #2 survives a stale decision");
        assert_eq!(form.depth, "-");
        assert_ne!(creates.status(), &CreateStatus::Stopped);
        assert_eq!(
            creates.new_ended(),
            Some(c),
            "handed back, to be asked again"
        );
        assert!(creates.new_ended().is_none());
        println!("\nFCAD_30T_QUESTION_GENERATION_EXECUTED");
    }

    /// Discarding New is the one moment its draft goes: the form, the drawing and a
    /// creation under way end together; the worker stays accounted until it ends and
    /// its scratch folder is removed only by whoever owns the candidate — never by
    /// the decision. Three timings of the same creation: still working, candidate
    /// already made and held, and its answer already queued for the event loop.
    #[test]
    fn stopping_new_ends_its_draft_and_leaves_the_worker_to_end_on_its_own() {
        for timing in ["working", "made", "queued"] {
            let sessions = tempfile::tempdir().expect("sessions");
            let mut creates = Creates::default();
            let mut input = input();
            assert!(open_form(&mut creates, &mut input));
            creates.form().expect("form").width = "1e999x".to_owned();
            let held = hold_creation(
                &mut creates,
                &mut input,
                sessions.path(),
                NewDocument::Empty,
                timing != "working",
            );
            held.at_barrier();
            let (held, queued) = if timing == "queued" {
                (None, Some(held.finish()))
            } else {
                (Some(held), None)
            };
            assert!(creates.running() && creates.making_new());
            assert!(creates.ask_open(PathBuf::from("b.fcad"), &mut input));
            let (_, losing) = creates.asking().expect("question");
            assert!(losing.contains("being made"), "{losing}");

            assert_eq!(
                creates.stop_new(&mut input),
                Some(PathBuf::from("b.fcad")),
                "{timing}"
            );
            assert_eq!(creates.status(), &CreateStatus::Stopped, "{timing}");
            assert!(creates.form().is_none(), "the draft ended");
            assert!(!creates.making_new() && !creates.running() && !creates.busy());
            assert!(!creates.can_cancel(), "nothing left to cancel");
            assert!(creates.asking().is_none());
            // Still accounted until it ends: stop does not join, and removes nothing.
            if timing != "queued" {
                assert_eq!(creates.running.len(), 1, "{timing}");
            }
            if timing != "working" {
                assert_eq!(entries(sessions.path()).len(), 1, "{timing}: scratch kept");
            }
            let _ = input.take_redraw();

            let (generation, result) = match (held, queued) {
                (Some(held), None) => held.finish(),
                (None, Some(answer)) => answer,
                _ => unreachable!("one of the two"),
            };
            assert!(
                finish_create(&mut creates, &mut input, generation, result).is_none(),
                "{timing}: an abandoned creation was shown"
            );
            assert_eq!(creates.status(), &CreateStatus::Stopped, "{timing}");
            assert!(!input.take_redraw(), "{timing}: a late answer owed a frame");
            assert!(
                entries(sessions.path()).is_empty(),
                "{timing}: scratch left"
            );
            creates.stop_all();
            assert!(creates.running.is_empty());
        }
        println!("\nFCAD_30T_STOP_NEW_EXECUTED");
    }

    /// The abandoned creation's answer cannot touch a newer one: New again, Create
    /// again, then the old answer — the newer generation stays the one awaited, and
    /// only its candidate is accepted.
    #[test]
    fn an_abandoned_creation_never_answers_for_a_newer_one() {
        let sessions = tempfile::tempdir().expect("sessions");
        let mut creates = Creates::default();
        let mut input = input();
        assert!(open_form(&mut creates, &mut input));
        let old = hold_creation(
            &mut creates,
            &mut input,
            sessions.path(),
            NewDocument::Empty,
            true,
        );
        old.at_barrier();
        assert!(creates.ask_open(PathBuf::from("b.fcad"), &mut input));
        creates.stop_new(&mut input).expect("stopped");

        assert!(open_form(&mut creates, &mut input));
        let new = hold_creation(
            &mut creates,
            &mut input,
            sessions.path(),
            NewDocument::Empty,
            true,
        );
        new.at_barrier();
        assert_ne!(old.generation, new.generation);
        let newer = new.generation;

        // The old answer arrives while the newer creation is awaited.
        let (generation, result) = old.finish();
        let _ = input.take_redraw();
        assert!(finish_create(&mut creates, &mut input, generation, result).is_none());
        assert_eq!(
            creates.status(),
            &CreateStatus::Running { generation: newer },
            "the older answer changed the newer creation's status"
        );
        assert!(creates.running() && creates.accepts(newer));
        assert!(!input.take_redraw());
        assert_eq!(
            entries(sessions.path()).len(),
            1,
            "only the newer candidate"
        );

        let (generation, result) = new.finish();
        let candidate = finish_create(&mut creates, &mut input, generation, result)
            .expect("the newer creation is accepted");
        finish_shown(&mut creates, &mut input, Ok(()));
        assert_eq!(creates.status(), &CreateStatus::Shown);
        drop(candidate);
        creates.stop_all();
        println!("\nFCAD_30T_ABANDONED_NEVER_ANSWERS_EXECUTED");
    }

    #[test]
    fn semantic_comparison_preserves_the_reference_to_its_specific_segment() {
        let dir = tempfile::tempdir().expect("directory");
        let path = dir.path().join("plate.fcad");
        create_through_the_window(&path, NewContent::SamplePlate, ["80", "50", "12"]);
        let original = read_semantics(&path).0;
        let mut doc = Document::open(&path).expect("document");
        let segments = doc
            .objects()
            .expect("objects")
            .into_iter()
            .find_map(|object| {
                if let ObjectPayload::Sketch(sketch) = object.payload {
                    Some(sketch.curves)
                } else {
                    None
                }
            })
            .expect("sketch");
        let mut reference = doc
            .topology_refs()
            .expect("refs")
            .into_iter()
            .find(|reference| matches!(reference.output_role, SemanticRole::ExtrudeSide { .. }))
            .expect("side ref");
        reference.output_role = SemanticRole::ExtrudeSide {
            profile_segment: segments[1].id,
        };
        reference.selection = SelectionRule::AllDerivedFrom {
            ancestor: segments[1].id,
        };
        doc.write(|writer| writer.put_topology_ref(&reference))
            .expect("change relationship");
        doc.close().expect("close");
        assert_ne!(
            original,
            read_semantics(&path).0,
            "a different valid segment is a different graph"
        );
    }

    #[test]
    fn both_clients_reopen_rebuild_resolve_and_export_the_saved_plate() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: this build has no Open CASCADE (create geometry gate)");
            return;
        }
        let dir = tempfile::tempdir().expect("directory");
        let cli = dir.path().join("cli.fcad");
        let ui = dir.path().join("ui.fcad");
        create_through_the_command_line(&cli, ["83", "47", "13"]);
        create_through_the_window(&ui, NewContent::SamplePlate, ["83", "47", "13"]);
        let run = |args: &[&std::ffi::OsStr]| {
            let output = std::process::Command::new(ferritecad())
                .args(args)
                .output()
                .expect("CLI");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output
        };
        let mut stls = Vec::new();
        let mut fbxs = Vec::new();
        for path in [&cli, &ui] {
            let saved = std::fs::read(path).expect("document bytes");
            let rebuilt = run(&["rebuild".as_ref(), path.as_os_str(), "--cold".as_ref()]);
            let report = String::from_utf8_lossy(&rebuilt.stdout);
            assert!(
                report.contains("3 of 3 stored references resolved"),
                "{report}"
            );
            assert!(
                report.contains("4 objects evaluated, 1 shape built"),
                "{report}"
            );
            let stl = path.with_extension("stl");
            run(&[
                "export-stl".as_ref(),
                path.as_os_str(),
                "--output".as_ref(),
                stl.as_os_str(),
            ]);
            stls.push(std::fs::read(stl).expect("STL"));
            let fbx = path.with_extension("fbx");
            run(&[
                "export-fbx".as_ref(),
                path.as_os_str(),
                "--output".as_ref(),
                fbx.as_os_str(),
            ]);
            let from_window = path.with_extension("window.fbx");
            // The actual UI export worker: one saved document must yield identical bytes.
            crate::exports::run_export(path, &from_window, false, &OperationContext::default())
                .expect("UI FBX");
            let bytes = std::fs::read(&fbx).expect("FBX");
            assert_eq!(bytes, std::fs::read(from_window).expect("UI FBX bytes"));
            fbxs.push(String::from_utf8(bytes).expect("ASCII FBX"));
            assert_eq!(saved, std::fs::read(path).expect("source unchanged"));
        }
        assert_eq!(stls[0], stls[1], "independent plates have equal triangles");
        assert_ne!(
            fbxs[0], fbxs[1],
            "independent documents have distinct identities"
        );
        // Canonicalize the native document's object identities consistently everywhere
        // in the FBX, retaining connections, transforms, hierarchy and geometry verbatim.
        for (path, fbx) in [&cli, &ui].into_iter().zip(&mut fbxs) {
            let doc = Document::open(path).expect("document");
            for object in doc.objects().expect("objects") {
                *fbx = fbx.replace(
                    &object.id.to_string(),
                    object.name.as_deref().expect("named object"),
                );
            }
            doc.close().expect("close");
        }
        assert_eq!(
            fbxs[0], fbxs[1],
            "geometry and hierarchy agree after identity mapping"
        );
    }
}
