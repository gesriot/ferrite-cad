// SPDX-License-Identifier: MIT
//! A window showing a model, and nothing else yet.
//!
//! Everything this binary does was decided somewhere it could be tested. What
//! a drag means is `ferritecad-ui`'s and is settled without an event loop; what
//! a frame is, and in what order it is composed, is `ferritecad-viewport-gpu`'s
//! and is settled without a window. What is left here is the wiring, and the
//! aim is that reading it should be enough to see that it is only wiring.
//!
//! # One frame per reason to draw one
//!
//! The event loop waits. It does not redraw on a timer, and it does not redraw
//! once per event: a drag delivers a pointer position for every sample the
//! hardware took, and drawing each one would render the same model at every
//! intermediate place the cursor passed through. Instead every reason to
//! redraw sets a flag. Taking that flag enters a one-frame scheduler, and only
//! the first reason in a batch calls `Window::request_redraw`; the slot opens
//! again when `RedrawRequested` begins. Delayed `egui` repaint requests become
//! a `ControlFlow::WaitUntil`, so waiting still serves animations without
//! turning the loop into a poller.
//!
//! # The document is read somewhere else
//!
//! Reading a `.fcad` file means rebuilding its features and tessellating the
//! result, which is seconds of work on a model of any size. Doing that between
//! two events would freeze the window for exactly that long, so it happens on
//! its own thread and comes back as one more event. The window opens on an
//! empty scene and gains the model when the model is ready.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::{sync::Arc, time::Instant};

use ferritecad_document::{
    CapSide, DOCUMENT_EXTENSION, SelectionRule, SemanticRole, SketchConstraintRule,
    SketchSegmentRef,
};
use ferritecad_kernel::{CancelToken, OperationContext, ProgressSink, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_scene::{
    CatalogueEntry, EdgeNames, FaceMeaning, FaceNames, LoadedScene, SceneItem, Selection,
    SketchPresentation, SketchSolveFacts, VertexNames, snapshot_of,
};
use ferritecad_types::{CadError, Result};
use ferritecad_ui::{
    Activity, Chosen, FRAME_ALL_KEY, FRAME_KEY, HIDE_KEY, Hover, ISOLATE_KEY, PROJECTION_KEY,
    PointerButton, RowVisibility, SHOW_ALL_KEY, Selected, SolvedSketch, VIEWS, ViewportEvent,
    ViewportInput,
};
use ferritecad_viewport::{
    Camera, EdgePickId, FacePickId, Hovered, Marked, PickId, Projection, RenderSnapshot,
    SnapshotBuilder, StandardView, Visibility,
};
use ferritecad_viewport_gpu::{Hit, PreparedSnapshot, Renderer, WindowSurface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

fn main() -> Result<()> {
    let request = match request(std::env::args_os().skip(1)) {
        Ok(request) => request,
        Err(error) => {
            // Printed rather than returned: an error out of `main` is shown in
            // its debug form, and a usage line is for a person to read.
            eprintln!("{error}");
            std::process::exit(EXIT_USAGE);
        }
    };
    let document = match request {
        // Answered here and gone, before this binary does anything else. No
        // event loop, no window, no surface, no document and no Open CASCADE
        // session: which solver a build has must be answerable on a machine
        // with no display and no GPU, by somebody who is not opening a model.
        Request::SolverInfo => std::process::exit(solver_info()),
        Request::Document(document) => document,
    };
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .map_err(|error| {
            ferritecad_types::CadError::rendering_because("opening a window", error)
        })?;
    // Wait rather than poll: a viewport with nothing happening in it should
    // cost nothing. Every path that changes what is on screen asks for a
    // redraw explicitly.
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App::new(event_loop.create_proxy(), document);
    event_loop
        .run_app(&mut app)
        .map_err(|error| ferritecad_types::CadError::rendering_because("running the window", error))
}

/// Exit code for a command line this viewer cannot act on.
const EXIT_USAGE: i32 = 2;

/// Exit code for `--solver-info` on a build that has no sketch solver.
///
/// Distinct from both of the others, and stable. Nothing went wrong with the
/// command, so this is not a failure; and there is no solver, so it is not
/// success either. A script has to be able to tell "this build can solve a
/// sketch" from "this build cannot" from "you typed it wrong" without reading
/// prose, and none of the three may be described as one of the others.
const EXIT_NO_SOLVER: i32 = 3;

/// What the command line asked this binary to do.
///
/// Two commands, not a command with options: `--solver-info` answers a
/// question about the build and opens nothing, and a document opens a window.
/// Nothing takes both.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// Open this document in a window.
    Document(PathBuf),
    /// Say which sketch solver this build has, and exit.
    SolverInfo,
}

/// The name of the diagnostic command, spelled once.
const SOLVER_INFO: &str = "--solver-info";

const USAGE: &str = "usage: ferritecad-viewer <file.fcad>, or ferritecad-viewer --solver-info";

/// Which of the two commands was typed.
///
/// `--solver-info` is recognised only as the whole command and only first: it
/// is not an option that modifies opening a document, and a line that names
/// both a document and this flag has asked for two different things.
fn request(arguments: impl Iterator<Item = OsString>) -> Result<Request> {
    let mut arguments = arguments.peekable();
    if arguments.peek().map(OsString::as_os_str) == Some(OsStr::new(SOLVER_INFO)) {
        arguments.next();
        if arguments.next().is_some() {
            return Err(CadError::input(format!(
                "{USAGE}; {SOLVER_INFO} takes no arguments"
            )));
        }
        return Ok(Request::SolverInfo);
    }
    document_argument(arguments).map(Request::Document)
}

/// Says which sketch solver this build has, and answers with an exit code.
///
/// Routing and formatting, and deliberately no arithmetic of its own. Whether
/// there is a solver, and what the library behind it is, are
/// `ferritecad-sketch-solver`'s answers; a provenance string spelled out here
/// would go on saying what it said when it was written, beside a library built
/// from anything at all.
fn solver_info() -> i32 {
    match ferritecad_sketch_solver::provenance() {
        Ok(provenance) => {
            println!("sketch solver: available");
            println!("provenance: {provenance}");
            0
        }
        // The typed refusal, forwarded and not paraphrased. A sentence written
        // here would be free to go on saying what it said when it was written,
        // after the crate that owns the answer had changed its mind.
        Err(unavailable) => {
            println!("sketch solver: unavailable");
            println!("{unavailable}");
            EXIT_NO_SOLVER
        }
    }
}

/// The document to look at, from the command line.
///
/// One argument and no options: a viewer that guessed which of several files
/// was meant, or opened the current directory, would be doing something the
/// user did not ask for with a file they may not have meant.
fn document_argument(arguments: impl Iterator<Item = OsString>) -> Result<PathBuf> {
    let mut arguments = arguments;
    let Some(path) = arguments.next() else {
        return Err(CadError::input(USAGE));
    };
    if arguments.next().is_some() {
        return Err(CadError::input(format!("{USAGE}; one document at a time")));
    }
    // There are no options, so anything that looks like one is a question
    // about how to use this rather than a file to open. Answering it by
    // failing to find a document called `--help` would answer nothing.
    if path.as_encoded_bytes().first() == Some(&b'-') {
        return Err(CadError::input(USAGE));
    }
    Ok(PathBuf::from(path))
}

/// A wake-up requested from outside winit's event-loop thread.
#[derive(Debug)]
enum AppEvent {
    RepaintAt(Instant),
    /// A document has finished loading, or has finished failing to.
    ///
    /// Boxed because a scene is the largest thing this application moves, and
    /// every other event would otherwise be that size too.
    Loaded {
        generation: LoadGeneration,
        result: Box<Result<LoadedScene>>,
    },
    /// A load has something new to say about how far along it is.
    ///
    /// Carries no number: the number is in the relay, and by the time this
    /// arrives it may already have been overtaken by a newer one.
    Progress {
        generation: LoadGeneration,
    },
}

/// The latest progress of one load, and whether the loop has been told.
///
/// A reading reports as often as it has something to say; the event loop is
/// woken once and reads the newest value when it gets there. An event per
/// report would put the whole of a long load's chatter in the queue ahead of
/// the user's next click, which is the opposite of what reporting progress is
/// for.
#[derive(Debug, Default)]
struct ProgressRelay {
    /// The newest fraction, as the bits of an `f64`.
    latest: AtomicU64,
    waiting: AtomicBool,
}

impl ProgressRelay {
    /// Records a report, returning whether the loop must be woken for it.
    fn record(&self, fraction: f64) -> bool {
        self.latest.store(fraction.to_bits(), Ordering::SeqCst);
        !self.waiting.swap(true, Ordering::SeqCst)
    }

    /// Takes the newest value, opening the way for the next wake-up.
    ///
    /// Cleared before it is read, so a report that lands in between is either
    /// read here or sends its own wake-up: what must not happen is that the
    /// last thing a load said is never seen.
    fn take(&self) -> f64 {
        self.waiting.store(false, Ordering::SeqCst);
        f64::from_bits(self.latest.load(Ordering::SeqCst))
    }
}

/// Which request an answer belongs to.
///
/// Monotonic and never reused. Identifying a load by the document it reads
/// would not do: opening the same file twice would produce two answers that
/// cannot be told apart, and the older one would be as welcome as the newer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct LoadGeneration(u64);

/// A load that has been started and not yet joined.
///
/// Which generation it belongs to is not kept here: what a running load is
/// for is to be stopped and waited for, and both are the same for all of them.
/// Only the answer carries a generation, because only the answer is a thing
/// that can arrive too late.
struct Loading {
    cancel: CancelToken,
    worker: JoinHandle<()>,
}

/// Every load this application has started and not yet finished with.
///
/// # Nothing here waits
///
/// Opening a second document while the first is still being read cancels the
/// first and carries on. Cancelling is a request: a kernel inside a rebuild
/// notices at the next feature, and that moment is one the window would
/// otherwise spend frozen with nothing on screen changing.
///
/// The abandoned worker is kept rather than dropped. Dropping a `JoinHandle`
/// detaches the thread, and a detached worker owns a kernel session nobody
/// will ever wait for; these are joined when they end, and all of them are
/// joined before the process does.
#[derive(Default)]
struct Loads {
    issued: u64,
    /// The request whose answer may still reach the screen.
    current: Option<LoadGeneration>,
    running: Vec<Loading>,
    status: Status,
    /// The document currently drawn, if any reading ever finished.
    shown: Option<String>,
    /// Where the reading in flight reports how far it has got.
    progress: Option<(LoadGeneration, Arc<ProgressRelay>)>,
    /// What the last attempt to open a document failed over, when it failed
    /// over something with parts.
    ///
    /// About that attempt and nothing else. Written, replaced and cleared by
    /// the one place that decides what an answer is worth, so a diagnostic
    /// cannot outlive the question it answered.
    conflict: Option<OpenConflict>,
}

/// What the window says about the document it is showing.
///
/// One sentence, and it is about the request the user last made rather than
/// about whatever finished most recently. A viewer that reported the state of
/// an abandoned reading would be describing a document nobody asked for.
#[derive(Debug, Default, Clone, PartialEq)]
enum Status {
    /// Nothing has been asked for. True only before the first request.
    #[default]
    Idle,
    /// Being read. Whatever was on screen before is still on screen.
    Loading {
        generation: LoadGeneration,
        file: String,
        /// How much of it is done, as far as anyone has said.
        fraction: f32,
    },
    /// On screen, which is a thing only the frame that replaced it can say.
    Ready { file: String },
    /// Could not be read. The previous model stays, and this says why.
    Failed { file: String, message: String },
}

/// Why one attempt to open a document failed, in the words a person reads.
///
/// Owned by the application rather than by the picture. It is not a fact about
/// the model on screen: the model on screen came from another file, which
/// still opens, and this describes the one that did not. Filing it beside what
/// that model's own solves found out would make a window say that the drawing
/// in front of the user contradicts itself when it does not.
///
/// Every string is made once, when the refusal arrives, from the durable facts
/// the refusal carried: the document's identifier for the sketch, the
/// document's identifier for each blamed constraint, and the same sentence
/// [`describe_constraint`] writes for a repeated constraint of that rule. No
/// solver is asked anything and no document is opened: by the time one of
/// these exists the rebuild that produced it is over, and the file may since
/// have been deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenConflict {
    /// The document that was being opened, named as the attempt named it.
    file: String,
    /// The sketch whose constraints disagree, as the document stores it.
    sketch: String,
    /// The blamed constraints, in the order the refusal carried them.
    constraints: Vec<ConstraintWords>,
}

impl OpenConflict {
    /// The diagnostic behind a refusal, when the refusal is a conflict.
    ///
    /// The type is asked, never the message: a message is a sentence written
    /// for a person and free to be rewritten, and a window that read one back
    /// as data would break the next time it was. A refusal that is not a
    /// conflict answers `None`, and the window shows the one line it has
    /// always shown.
    fn of(error: &CadError, file: &str) -> Option<Self> {
        let conflict = ferritecad_scene::SketchConflict::of(error)?;
        Some(Self {
            file: file.to_owned(),
            sketch: conflict.sketch().to_string(),
            // In the order the conflict carried them, which is the order the
            // document stores them. Nothing is sorted, folded or dropped: two
            // constraints that say the same thing under two identifiers are
            // two constraints, and a person looking for either has to find it.
            constraints: conflict
                .constraints()
                .iter()
                .map(|constraint| ConstraintWords::of(constraint.id(), constraint.rule()))
                .collect(),
        })
    }

    /// Each blamed constraint, borrowed, in the order it arrived in.
    fn rules(&self) -> Vec<ferritecad_ui::ConflictingRule<'_>> {
        self.constraints
            .iter()
            .map(|constraint| ferritecad_ui::ConflictingRule {
                identifier: &constraint.identifier,
                says: &constraint.says,
            })
            .collect()
    }

    /// What the section shows this frame, borrowed from this frame's words.
    ///
    /// The rules arrive alongside rather than inside, on the same terms as a
    /// solved sketch's repeated constraints: a panel is given borrowed text
    /// and the owner of that text is the frame.
    fn shown<'a>(
        &'a self,
        rules: &'a [ferritecad_ui::ConflictingRule<'a>],
    ) -> ferritecad_ui::OpenFailure<'a> {
        ferritecad_ui::OpenFailure {
            file: &self.file,
            sketch: &self.sketch,
            constraints: rules,
        }
    }
}

impl Status {
    /// How far the load in flight has got, if one is in flight.
    ///
    /// `None` is not "nothing is happening at nought per cent": it is nothing
    /// happening at all, which is what tells the toolbar whether there is
    /// anything to offer to cancel.
    fn fraction(&self) -> Option<f32> {
        match self {
            Self::Loading { fraction, .. } => Some(*fraction),
            _ => None,
        }
    }

    /// The line to put in front of the user.
    fn line(&self) -> String {
        match self {
            Self::Idle => "No document".to_owned(),
            Self::Loading { file, fraction, .. } => {
                format!("Opening {file}… {:.0}%", fraction * 100.0)
            }
            Self::Ready { file } => file.clone(),
            Self::Failed { file, message } => format!("{file}: {message}"),
        }
    }
}

/// What to call a document in one line.
///
/// The file's own name rather than the path it was found at: a status line is
/// a few centimetres wide, and the part that identifies a document to the
/// person who opened it is the end of the path rather than the beginning.
fn short_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

impl Loads {
    /// Opens what the user chose, if they chose anything.
    ///
    /// `None` is a dialog they closed, and it does exactly nothing: no
    /// generation, no worker, and above all no change to what the window says
    /// about the document already on screen.
    ///
    /// Otherwise whatever was being read is abandoned and a new request takes
    /// its place. `spawn` is handed the generation to label its answer with
    /// and the token that stops it; starting and recording are one operation,
    /// so there is no arrangement of calls in which a running worker is
    /// untracked.
    fn open(
        &mut self,
        chosen: Option<&Path>,
        progress: Arc<ProgressRelay>,
        spawn: impl FnOnce(LoadGeneration, &CancelToken) -> JoinHandle<()>,
    ) -> Option<LoadGeneration> {
        let path = chosen?;

        for loading in &self.running {
            loading.cancel.cancel();
        }

        self.issued += 1;
        let generation = LoadGeneration(self.issued);
        let cancel = CancelToken::new();
        let worker = spawn(generation, &cancel);
        self.running.push(Loading { cancel, worker });
        self.current = Some(generation);
        self.progress = Some((generation, progress));
        // Asking for a document is the end of the last attempt's account of
        // itself. Leaving it up beside `Opening…` would read as this file
        // having already failed.
        //
        // The only place one is taken down, and that is why it is the only
        // state an account can be in when an answer arrives: nothing else can
        // reach `Status::Loading`, so a document that opens, a document that
        // fails for some other reason and a reading given up on all find that
        // this has already happened.
        self.conflict = None;
        self.status = Status::Loading {
            generation,
            file: short_name(path),
            fraction: 0.0,
        };
        Some(generation)
    }

    /// Whether this answer is still the one that was asked for.
    fn accepts(&self, generation: LoadGeneration) -> bool {
        self.current == Some(generation)
    }

    /// What the window should be saying.
    fn status(&self) -> &Status {
        &self.status
    }

    /// What the last attempt to open a document failed over, if it did.
    fn conflict(&self) -> Option<&OpenConflict> {
        self.conflict.as_ref()
    }

    /// The newest thing the load in flight has said about itself.
    ///
    /// Reported for every generation and worth something for one, exactly as
    /// an answer is. Returns whether the line changed, and a change of less
    /// than half a per cent is not one: a frame for a difference nobody can
    /// see is a frame spent for nothing.
    fn progressed(&mut self, generation: LoadGeneration, fraction: f64) -> bool {
        let Status::Loading {
            generation: waiting,
            fraction: shown,
            ..
        } = &mut self.status
        else {
            return false;
        };
        if *waiting != generation {
            return false;
        }

        let now = (fraction as f32).clamp(0.0, 1.0);
        if (now - *shown).abs() < 0.005 {
            return false;
        }
        *shown = now;
        true
    }

    /// Where to read the progress of the load in flight.
    fn relay(&self, generation: LoadGeneration) -> Option<&Arc<ProgressRelay>> {
        self.progress
            .as_ref()
            .filter(|(current, _)| *current == generation)
            .map(|(_, relay)| relay)
    }

    /// Stops the reading in flight and goes back to describing what is drawn.
    ///
    /// Nothing on screen changes: the model the user was looking at is the one
    /// they keep. The abandoned reading will still answer, and by then it is
    /// no longer current, so its answer is discarded like any other stale one.
    ///
    /// Returns whether the line changed.
    fn cancel_current(&mut self) -> bool {
        if self.current.take().is_none() {
            return false;
        }
        for loading in &self.running {
            loading.cancel.cancel();
        }
        self.progress = None;
        // Nothing is cleared here beyond the reading itself. There can be no
        // account of a failure to clear: asking for this document took the
        // last one's down, and a reading that has not answered has not failed.

        // Back to naming the document that is actually drawn. Saying anything
        // about the one that was abandoned would describe a model the window
        // is not showing.
        self.status = match &self.shown {
            Some(file) => Status::Ready { file: file.clone() },
            None => Status::Idle,
        };
        true
    }

    /// Notes what a generation answered, and joins whatever has ended.
    ///
    /// Returns whether the line changed, because a line that changed is a
    /// reason to draw a frame and nothing else here is.
    ///
    /// The outcome is reported for every answer, current or not, and this
    /// decides what it is worth. Asking callers to filter would put the rule
    /// in two places, and the place that forgot it would be the one that
    /// announced a document the user is no longer opening.
    fn answered(&mut self, generation: LoadGeneration, outcome: Result<()>) -> bool {
        let changed = match &self.status {
            Status::Loading {
                generation: waiting,
                file,
                ..
            } if *waiting == generation => {
                let file = file.clone();
                self.status = match outcome {
                    Ok(()) => {
                        // What the window is showing from now on, which is what
                        // it goes back to saying if the next reading is
                        // abandoned.
                        self.shown = Some(file.clone());
                        Status::Ready { file }
                    }
                    Err(error) => {
                        // The one place an account of a failure is written,
                        // and it is written for the attempt that just failed,
                        // named by the file that attempt asked for. A refusal
                        // with no parts writes none, and there is never an
                        // older one underneath it: asking for this document
                        // took that one down.
                        self.conflict = OpenConflict::of(&error, &file);
                        Status::Failed {
                            file,
                            message: error.to_string(),
                        }
                    }
                };
                true
            }
            _ => false,
        };

        if self.current == Some(generation) {
            self.current = None;
        }
        // Only the threads that have already finished, which join at once.
        // Waiting here for one that is still reading would be the freeze this
        // whole arrangement exists to avoid.
        let mut index = 0;
        while index < self.running.len() {
            if self.running[index].worker.is_finished() {
                let done = self.running.swap_remove(index);
                let _ = done.worker.join();
            } else {
                index += 1;
            }
        }
        changed
    }

    /// Stops every load and waits for all of them.
    ///
    /// The one place that blocks, and the last thing that happens: a worker
    /// owns a kernel session, and leaving it to be cut short by process exit
    /// would make the one path that releases geometry the one nobody takes.
    fn stop_all(&mut self) {
        self.current = None;
        for loading in &self.running {
            loading.cancel.cancel();
        }
        // Cancelled first, all of them, and only then waited for. Cancelling
        // one and waiting for it before asking the next to stop would add up
        // every reading's remaining work.
        for loading in self.running.drain(..) {
            let _ = loading.worker.join();
        }
    }
}

/// Starts a chosen load and makes its new status visible.
///
/// `Loads` owns the state transition; this application boundary owns the fact
/// that a visible transition is a reason to draw. Keeping the two in one
/// operation prevents a caller from starting real work while leaving the old
/// status on screen until that work happens to finish.
fn begin_load(
    loads: &mut Loads,
    input: &mut ViewportInput,
    chosen: Option<&Path>,
    progress: Arc<ProgressRelay>,
    spawn: impl FnOnce(LoadGeneration, &CancelToken) -> JoinHandle<()>,
) -> Option<LoadGeneration> {
    let generation = loads.open(chosen, progress, spawn);
    if generation.is_some() {
        input.request_redraw();
    }
    generation
}

/// Takes what a load has said about itself since the last time it was asked.
///
/// The wake-up carries no number, so the number is read here: by the time the
/// loop gets to it, several more reports may have arrived and only the newest
/// is worth anything. Reading also opens the way for the next wake-up, which
/// is what keeps one load's chatter to one event at a time.
fn advance_load(loads: &mut Loads, input: &mut ViewportInput, generation: LoadGeneration) -> bool {
    let Some(fraction) = loads.relay(generation).map(|relay| relay.take()) else {
        return false;
    };
    let changed = loads.progressed(generation, fraction);
    if changed {
        input.request_redraw();
    }
    changed
}

/// Abandons the reading in flight at the user's request.
///
/// The model on screen is not touched, and the line goes back to naming it.
fn cancel_load(loads: &mut Loads, input: &mut ViewportInput) -> bool {
    let changed = loads.cancel_current();
    if changed {
        input.request_redraw();
    }
    changed
}

/// What accepting or discarding an answer did at the application boundary.
#[derive(Debug, PartialEq, Eq)]
struct AnswerEffect {
    status_changed: bool,
    /// Present only for the current request's failure.
    error: Option<String>,
}

/// Finishes an answer after its picture has either been committed or refused.
///
/// `Loads::answered` is the one generation check. Its answer controls both
/// things visible outside that state machine: a redraw and an error in the
/// diagnostic stream. A stale error must not bypass it merely because stderr
/// and the toolbar are two different places to report something.
fn finish_answer(
    loads: &mut Loads,
    input: &mut ViewportInput,
    generation: LoadGeneration,
    outcome: Result<()>,
) -> AnswerEffect {
    let error = outcome.as_ref().err().map(ToString::to_string);
    let status_changed = loads.answered(generation, outcome);
    if status_changed {
        input.request_redraw();
    }
    AnswerEffect {
        status_changed,
        error: if status_changed { error } else { None },
    }
}

/// What a click on one pixel chooses.
///
/// All four identities of the pixel are read from one frame and decided
/// together, by the scene, which is the only place that knows what the
/// document calls a face, edge or vertex and is therefore the only place that
/// can decide.
///
/// A pixel that names nothing chooses nothing: clicking the background is how
/// a person unchooses, and a pick left over from a document that has since
/// been replaced names a definition of a picture nobody is looking at.
fn selection_at(
    hit: Hit,
    snapshot: &RenderSnapshot,
    faces: &FaceNames,
    edges: &EdgeNames,
    vertices: &VertexNames,
) -> Selection {
    // All four answers about one pixel, from one frame, decided together.
    Selection::at(
        hit.definition(),
        hit.face(),
        hit.edge(),
        hit.vertex(),
        snapshot,
        faces,
        edges,
        vertices,
    )
}

/// Chooses the definition named by a list row and draws that change once.
///
/// A row is deliberately not an identity. The snapshot that supplied the
/// list must turn it into one, and a row outside that snapshot changes
/// nothing. Repeating the current choice is not a visible change either.
fn select_definition_row(
    selection: &mut Selection,
    snapshot: &RenderSnapshot,
    input: &mut ViewportInput,
    row: usize,
) {
    let Some(pick) = snapshot.pick_of(row) else {
        return;
    };
    // A row names a definition and can name nothing else. A list of
    // definitions holds no faces, so pressing one cannot choose the face that
    // happened to be under the pointer a moment ago.
    let chosen = Selection::definition(pick, snapshot);
    if chosen != *selection {
        *selection = chosen;
        input.request_redraw();
    }
}

/// Runs a load away from the event loop and delivers the answer back to it.
///
/// Both halves are arguments so that this can be shown to return while the
/// load is still running, which is the whole property: the window stays alive
/// while a document is read.
fn spawn_load(
    load: impl FnOnce() -> Result<LoadedScene> + Send + 'static,
    deliver: impl FnOnce(Result<LoadedScene>) + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || deliver(load()))
}

/// Everything a prepared load hands to the event loop, in one name.
///
/// Nine parts of one arrival that must become current together. A named
/// structure rather than the tuple this was: what a fifth, sixth or ninth
/// element of a tuple means is something a reader has to count out, and the
/// last two of them – what the solve of each sketch found out, and where each
/// drawing ended up – are a different kind of fact from the rest and would be
/// the easiest to misread.
///
/// Private, and nothing here leaves this file: these are the terms in which
/// one document replaces another, not an interface anything else uses.
#[derive(Debug)]
struct PreparedLoad<P> {
    /// The camera that frames the arriving picture, staged rather than
    /// applied: a camera that moved before the upload succeeded would frame a
    /// model that is still the old one.
    framed: ViewportInput,
    prepared: P,
    catalogue: Vec<CatalogueEntry>,
    faces: FaceNames,
    edges: EdgeNames,
    vertices: VertexNames,
    visibility: Visibility,
    /// What the solve of each constrained sketch of the arriving document
    /// found out, in document order.
    ///
    /// Carried beside the picture rather than derived from it, because it
    /// cannot be derived from it: a sketch is not drawn here, and the one
    /// rebuild that could have said this is over. Committed with the picture
    /// so that no frame can show what one document's solve found out beside
    /// another document's model.
    sketch_solves: Vec<SketchSolveFacts>,
    /// Every sketch of the arriving document, at the coordinates its profile
    /// was built from, in document order.
    ///
    /// Beside the picture rather than in it, and committed with it for the
    /// same reason: a frame showing one document's model over another
    /// document's drawings would be two files at once.
    sketch_presentations: Vec<SketchPresentation>,
}

/// Prepares both halves of a loaded picture without changing the current one.
///
/// Framing the camera is part of accepting a scene. It therefore has to be
/// staged alongside the GPU upload: if preparing the buffers fails after the
/// camera has already moved, the old model remains resident but may be framed
/// completely out of view. Returning every candidate lets the event-loop
/// thread commit them together after every fallible step has succeeded.
fn prepare_load<P>(
    current_input: &ViewportInput,
    loaded: Result<LoadedScene>,
    prepare: impl FnOnce(Arc<RenderSnapshot>) -> Result<P>,
) -> Result<PreparedLoad<P>> {
    let mut input = current_input.clone();
    let loaded = loaded?;
    let snapshot = input.accept_load(Ok(loaded.snapshot))?;
    // Everything drawn, in the picture that arrived. Built here rather than
    // carried over: what was hidden was hidden in a picture nobody is looking
    // at any more, and a mask that outlived its picture would be a document
    // opening with parts already missing.
    let visibility = Visibility::new(&snapshot);
    let prepared = prepare(Arc::new(snapshot))?;
    Ok(PreparedLoad {
        framed: input,
        prepared,
        catalogue: loaded.catalogue,
        faces: loaded.faces,
        edges: loaded.edges,
        vertices: loaded.vertices,
        visibility,
        // Carried, not rebuilt. This is what the one rebuild behind the
        // picture found out, and the only other way to have it would be to
        // solve every sketch of the document a second time.
        sketch_solves: loaded.sketch_solves,
        // Carried on the same terms, and it could not be rebuilt here at all:
        // the drawings are not in the picture, and the rebuild that knew where
        // they ended up is over.
        sketch_presentations: loaded.sketch_presentations,
    })
}

/// Applies every texture upload and consumes it from egui's command set.
fn upload_textures(
    textures: &mut egui::TexturesDelta,
    mut upload: impl FnMut(egui::TextureId, &egui::epaint::ImageDelta),
) {
    for (id, deltas) in textures.set.drain() {
        for delta in deltas {
            upload(id, &delta);
        }
    }
}

/// Frees every retired texture and consumes it from egui's command set.
fn free_textures(textures: &mut egui::TexturesDelta, mut free: impl FnMut(&egui::TextureId)) {
    for id in textures.free.drain() {
        free(&id);
    }
}

/// The one outstanding frame and the earliest future reason to draw another.
///
/// `ViewportInput` coalesces reasons at the application boundary. This second
/// gate is deliberately at the OS boundary: calling `Window::request_redraw`
/// twenty times and relying on a platform to merge them is not the same
/// contract as asking the platform once.
#[derive(Debug, Default)]
struct FrameScheduler {
    queued: bool,
    deadline: Option<Instant>,
}

impl FrameScheduler {
    /// Records an immediate frame, returning whether winit must be asked.
    fn request_now(&mut self) -> bool {
        // An immediate frame supersedes a timer. The frame will run egui again,
        // which will request a new delay if the animation still needs one.
        self.deadline = None;
        !std::mem::replace(&mut self.queued, true)
    }

    /// Records the earliest future frame, returning a changed deadline.
    fn request_at(&mut self, deadline: Instant, now: Instant) -> Option<Instant> {
        if deadline <= now {
            return None;
        }
        // A frame already on the OS queue is earlier than any future one. Its
        // egui pass will ask again if the delayed repaint remains necessary.
        if self.queued {
            return None;
        }
        if self.deadline.is_some_and(|current| current <= deadline) {
            return None;
        }
        self.deadline = Some(deadline);
        Some(deadline)
    }

    /// Promotes a timer that has elapsed into an immediate reason to draw.
    fn take_due(&mut self, now: Instant) -> bool {
        if self.deadline.is_none_or(|deadline| deadline > now) {
            return false;
        }
        self.deadline = None;
        true
    }

    /// The queued frame has begun and no longer occupies the one-frame slot.
    fn frame_started(&mut self) {
        self.queued = false;
    }
}

/// What exists only once a window does.
struct Live {
    window: Arc<Window>,
    renderer: Renderer,
    surface: WindowSurface,
    scene: LiveScene<PreparedSnapshot>,
    egui: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

/// The GPU picture and the transient state that belongs to it.
///
/// These values describe one snapshot and are replaced as one. In particular,
/// selection cannot survive Open merely because the next document happens to
/// produce byte-identical geometry: the same raw pick beside a different
/// catalogue could otherwise silently name another document object.
struct LiveScene<P> {
    prepared: P,
    /// What each mesh of `prepared` is: an identity a document could store,
    /// and the facts a person needs to recognise it.
    catalogue: Vec<CatalogueEntry>,
    /// What the document durably calls each face of `prepared`, which is what
    /// makes a face selectable as a face rather than as the part it is on.
    faces: FaceNames,
    /// What the document durably calls the topological edges of `prepared`.
    /// Beside `faces` and replaced with it, for the same reason: what a name
    /// means belongs to the picture it was read against.
    edges: EdgeNames,
    /// And what it durably calls the topological vertices. Carried on the same
    /// terms as the two above: a name read against one picture says nothing
    /// about the next, so it is replaced whole rather than kept.
    ///
    /// What makes a corner selectable as a corner rather than as the edge,
    /// face or part beneath it.
    vertices: VertexNames,
    /// Which definitions this window is drawing. Transient, bound to
    /// `prepared`, and reset with it: what is hidden is a state of looking at
    /// a document, not a fact about the document.
    visibility: Visibility,
    /// What is chosen: nothing, a definition, or one face of one. One state
    /// rather than a transient field beside a semantic one, so there is no
    /// arrangement in which they describe different things.
    selection: Selection,
    /// What the pointer is over, which is a question and not a decision. Also
    /// issued by `prepared`, also transient, and written down nowhere. Five
    /// states rather than one identity, because a list row can name only a
    /// definition while a pixel can name a definition, face, edge or vertex,
    /// and those are different things to show.
    hovered: Hovered,
    /// What the solve of each constrained sketch of the document behind
    /// `prepared` found out, in document order.
    ///
    /// Held here and replaced with the picture, though it is no part of one: a
    /// window that kept the last document's account of its drawings beside
    /// this document's model would be describing a file nobody has open. It
    /// names no pick, no mesh and no definition, so nothing resolves through
    /// it and nothing about the picture depends on it.
    sketch_solves: Vec<SketchSolveFacts>,
    /// Every sketch of the document behind `prepared`, at the coordinates its
    /// profile was built from, in document order.
    ///
    /// Held here and replaced with the picture, on the same terms as the
    /// account of the solves above: a drawing belongs to the document it was
    /// read from, and keeping the last one beside this one's model would put
    /// two files on screen. It names no pick, no mesh and no definition, so
    /// nothing resolves through it and nothing about the picture depends on
    /// it.
    ///
    /// Nothing draws it yet, which is why the compiler is told so by name:
    /// this slice carries the drawings to the window and stops, and what puts
    /// them on screen arrives next. It is here so that whatever does can draw
    /// the sketch the rebuild actually built from, rather than asking a solver
    /// again during a frame or drawing coordinates the file has outgrown.
    #[allow(dead_code, reason = "drawn by the slice after this one")]
    sketch_presentations: Vec<SketchPresentation>,
}

impl<P> LiveScene<P> {
    /// A replacement picture begins with no choice made in it.
    ///
    /// The parameters are the parts of one arrival that must become current
    /// together, and they are listed rather than grouped on purpose: the group
    /// already exists, it is [`PreparedLoad`], and a second structure holding
    /// the same values would be a second place for one of them to be forgotten.
    #[allow(
        clippy::too_many_arguments,
        reason = "one document replacing another is one statement; see PreparedLoad"
    )]
    fn new(
        prepared: P,
        catalogue: Vec<CatalogueEntry>,
        faces: FaceNames,
        edges: EdgeNames,
        vertices: VertexNames,
        visibility: Visibility,
        sketch_solves: Vec<SketchSolveFacts>,
        sketch_presentations: Vec<SketchPresentation>,
    ) -> Self {
        Self {
            prepared,
            catalogue,
            faces,
            edges,
            vertices,
            visibility,
            selection: Selection::Nothing,
            hovered: Hovered::Nothing,
            sketch_solves,
            sketch_presentations,
        }
    }

    /// What the interface shows this frame: every definition, and which one
    /// of them is chosen.
    ///
    /// One answer for both halves. A list marking one row while an inspector
    /// describes another would be two accounts of one choice, and the way to
    /// have one account is for one resolution to produce both.
    fn view<'a>(
        &'a self,
        identities: &'a [String],
        snapshot: &RenderSnapshot,
    ) -> (Vec<Selected<'a>>, Option<usize>) {
        (
            self.rows(identities),
            self.chosen(snapshot).map(|(row, _)| row),
        )
    }

    /// What a list of definitions shows, in the order the picture packs them.
    ///
    /// One row per definition and not one per placement: a definition drawn
    /// four times is one definition, and a list with a row each would offer
    /// four ways to choose the same thing while marking only one of them as
    /// chosen. The identities are passed in because a row borrows one and they
    /// must outlive the frame.
    fn rows<'a>(&'a self, identities: &'a [String]) -> Vec<Selected<'a>> {
        self.catalogue
            .iter()
            .zip(identities)
            .map(|(entry, identity)| describe(entry, identity))
            .collect()
    }

    /// What is chosen: where it sits in this picture, and what it is.
    ///
    /// Two lookups and no search: the pick names a definition of this snapshot
    /// or of no snapshot, and the catalogue is indexed the way the snapshot
    /// is. Nothing falls back to a name, because names repeat.
    ///
    /// Both halves come from the same resolution, so the row a list shows as
    /// chosen and the facts an inspector shows about it cannot disagree.
    fn chosen<'a>(&'a self, snapshot: &RenderSnapshot) -> Option<(usize, &'a CatalogueEntry)> {
        let definition = self.selection.owning_definition(snapshot)?;
        Some((definition, self.catalogue.get(definition)?))
    }
}

/// Puts a finished load on screen, or leaves everything exactly as it was.
///
/// One statement for both outcomes, so there is no arrangement in which half
/// of a document arrives. A load that failed or was given up on changes
/// nothing at all: the model is still the model, the camera is still framing
/// it, and what was chosen in it is still chosen. A load that succeeded
/// replaces the picture, what its parts are, the choice made in it and the
/// camera together – the choice cleared, because a choice belongs to the
/// picture that issued it and the next document may draw the same geometry
/// while meaning something else by it.
fn commit_scene<P>(
    scene: &mut LiveScene<P>,
    camera: &mut ViewportInput,
    next: Result<PreparedLoad<P>>,
) -> Result<()> {
    let next = next?;
    *scene = LiveScene::new(
        next.prepared,
        next.catalogue,
        next.faces,
        next.edges,
        next.vertices,
        next.visibility,
        next.sketch_solves,
        next.sketch_presentations,
    );
    *camera = next.framed;
    Ok(())
}

/// Where everything drawn as the chosen definition is.
///
/// Asked of the picture that issued the choice, so a choice belonging to a
/// replaced picture, a definition that draws nothing and nothing chosen at all
/// are one answer: there is nowhere to go.
fn selection_bounds<P>(
    scene: &LiveScene<P>,
    snapshot: &RenderSnapshot,
) -> Option<([f32; 3], [f32; 3])> {
    scene.selection.bounds(snapshot)
}

/// Whether Hide selected would remove any geometry from this picture.
///
/// A definition with no triangles may still have a catalogue row and be
/// chosen from it. It is already nowhere on screen, so hiding it must not
/// enable Show all or claim a change that no frame can show.
fn can_hide_selection<P>(scene: &LiveScene<P>, snapshot: &RenderSnapshot) -> bool {
    scene
        .visibility
        .can_hide(scene.selection.marked(), snapshot)
}

/// Stops drawing what is chosen, and forgets everything that pointed at it.
///
/// Hiding is per definition: a chosen face hides the part it is on, because
/// this is what a definition being hidden means and a part with one face
/// missing is a different part.
///
/// The choice goes with it, and so does what the pointer was over and any
/// question or click still in flight. Leaving a selection on something no
/// longer drawn would leave an inspector describing a part nobody can see, and
/// a click already recorded would be answered against the frame that is about
/// to be replaced.
///
/// Returns whether anything happened, which is what an unavailable action must
/// not claim. The camera is not an argument here: hiding is not a way to move.
fn hide_selected(
    visibility: &mut Visibility,
    selection: &mut Selection,
    hovered: &mut Hovered,
    snapshot: &RenderSnapshot,
    input: &mut ViewportInput,
) -> bool {
    if !visibility.hide(selection.marked(), snapshot) {
        return false;
    }
    *selection = Selection::Nothing;
    *hovered = Hovered::Nothing;
    input.forget_pending();
    true
}

/// Whether Isolate selected would remove any geometry from this picture.
///
/// The same resolution Hide uses, asked the other way round: something chosen
/// and still drawn, with something else still drawn beside it.
fn can_isolate_selection<P>(scene: &LiveScene<P>, snapshot: &RenderSnapshot) -> bool {
    scene
        .visibility
        .can_isolate(scene.selection.marked(), snapshot)
}

/// Stops drawing everything except what is chosen, and changes nothing else.
///
/// The choice is kept exactly as it was, which is the whole point: this is the
/// operation for looking at the thing you have already chosen. It is given the
/// selection immutably, so it cannot alter one however it is called.
///
/// What the pointer was over goes, along with any click or question still in
/// flight: those are about the frame being replaced, and answering them
/// afterwards would answer them against a picture with different parts in it.
/// The camera is not an argument: isolating is not a way to move.
fn isolate_selected(
    visibility: &mut Visibility,
    selection: &Selection,
    hovered: &mut Hovered,
    snapshot: &RenderSnapshot,
    input: &mut ViewportInput,
) -> bool {
    if !visibility.isolate(selection.marked(), snapshot) {
        return false;
    }
    *hovered = Hovered::Nothing;
    input.forget_pending();
    true
}

/// Draws the model through the other projection, and forgets the old frame.
///
/// Neither what is chosen nor what is drawn is an argument: switching between
/// what an eye sees and what a drawing shows is a change of view, and a view
/// cannot decide what is selected or which parts are on screen. The camera
/// keeps what it was looking at, the viewing direction and apparent size.
///
/// Every pixel means something different afterwards, so what the pointer was
/// over, and any click, question or gesture in flight, are forgotten: they
/// describe a frame that is being replaced.
fn change_projection(input: &mut ViewportInput, hovered: &mut Hovered, to: Projection) -> bool {
    if !input.set_projection(to) {
        return false;
    }
    *hovered = Hovered::Nothing;
    input.forget_pending();
    true
}

/// The projection that is not the one in use.
fn other_projection(current: Projection) -> Projection {
    match current {
        Projection::Perspective => Projection::Orthographic,
        Projection::Orthographic => Projection::Perspective,
    }
}

/// Takes back the last change to what is drawn, and forgets the old frame.
///
/// Visibility only: it restores what was on screen, not what was chosen. A
/// selection that is still drawn survives exactly; one whose definition has
/// gone back off screen is cleared, for the same reason hiding it clears it.
/// There is no old selection here to put back, and no argument through which
/// one could arrive.
///
/// The camera is not an argument either. What the pointer was over, and any
/// click, question or gesture in flight, belong to the frame being replaced.
fn undo_visibility(
    visibility: &mut Visibility,
    selection: &mut Selection,
    hovered: &mut Hovered,
    snapshot: &RenderSnapshot,
    input: &mut ViewportInput,
) -> bool {
    if !visibility.undo(snapshot) {
        return false;
    }
    if let Some(definition) = selection.owning_definition(snapshot)
        && !visibility.shows(definition, snapshot)
    {
        *selection = Selection::Nothing;
    }
    *hovered = Hovered::Nothing;
    input.forget_pending();
    true
}

/// What each row of this list can offer about whether it is drawn.
///
/// One entry per row, in the order the picture packs them, and one offer per
/// row: a definition is either drawn or it is not. A row that draws nothing
/// wherever it is offers neither, because both would change no pixel.
///
/// The rule is here rather than in the panel because only this side knows what
/// the picture draws, and it is the same pair of questions the rest of the
/// application asks – nothing here decides visibility a second time.
fn rows_visibility(visibility: &Visibility, snapshot: &RenderSnapshot) -> Vec<RowVisibility> {
    (0..snapshot.meshes().len())
        .map(|definition| {
            let Some(pick) = snapshot.pick_of(definition) else {
                return RowVisibility::Neither;
            };
            let mark = Marked::Definition(pick);
            if visibility.can_hide(mark, snapshot) {
                RowVisibility::Hide(pick)
            } else if visibility.can_show(mark, snapshot) {
                RowVisibility::Show(pick)
            } else {
                RowVisibility::Neither
            }
        })
        .collect()
}

/// Stops drawing one definition, named by its row, and forgets the old frame.
///
/// The way to remove a distraction without giving up what is being looked at:
/// everything else keeps its state, and a choice on another definition is
/// untouched.
///
/// The choice does go when it is the thing being removed. Geometry nobody can
/// see cannot remain chosen: an inspector would be describing something that
/// is not on screen, and Frame selected would have nowhere to go.
///
/// Both the request and the choice are resolved through the picture, so a
/// definition index never leaves this function and never becomes an identity
/// anywhere.
///
/// What the pointer was over, and any click, question or gesture in flight,
/// are forgotten: geometry leaving changes what some pixels mean.
fn hide_one(
    visibility: &mut Visibility,
    selection: &mut Selection,
    hovered: &mut Hovered,
    snapshot: &RenderSnapshot,
    requested: PickId,
    input: &mut ViewportInput,
) -> bool {
    let requested_definition = snapshot.definition(requested);
    if !visibility.hide(Marked::Definition(requested), snapshot) {
        return false;
    }
    if selection.owning_definition(snapshot) == requested_definition {
        *selection = Selection::Nothing;
    }
    *hovered = Hovered::Nothing;
    input.forget_pending();
    true
}

/// Draws one hidden definition again, and forgets the old frame.
///
/// The way back from hiding one thing too many, without giving up the rest of
/// the view: everything else that was hidden stays hidden.
///
/// What is chosen is not an argument, so this cannot alter a selection however
/// it is called - a definition returning to the screen is not a decision about
/// what the user is working on. The camera is not an argument either.
///
/// What the pointer was over, and any click, question or gesture in flight,
/// are forgotten: returning geometry changes what some pixels mean, and a
/// click recorded while the part was absent must not be answered against a
/// frame in which it is present.
fn show_one(
    visibility: &mut Visibility,
    hovered: &mut Hovered,
    snapshot: &RenderSnapshot,
    requested: PickId,
    input: &mut ViewportInput,
) -> bool {
    if !visibility.show(Marked::Definition(requested), snapshot) {
        return false;
    }
    *hovered = Hovered::Nothing;
    input.forget_pending();
    true
}

/// Draws every definition again, keeps the choice and forgets the old frame.
///
/// Deliberately not a way to choose anything: what was hidden was unchosen
/// when it was hidden, and putting it back on screen is not the same as
/// deciding it is what the user is working on. Pointing and interaction in
/// flight are different: they describe pixels of the frame before hidden
/// geometry returned, so answering them afterwards could name something that
/// was absent when the question was recorded.
fn show_all(visibility: &mut Visibility, hovered: &mut Hovered, input: &mut ViewportInput) -> bool {
    if !visibility.show_all() {
        return false;
    }
    *hovered = Hovered::Nothing;
    input.forget_pending();
    true
}

/// Shows what is chosen, and changes nothing else.
///
/// The scene is borrowed immutably, so this cannot move the camera *and*
/// alter the choice: showing something is not choosing it, and a user who
/// pressed this to see their selection would not expect to lose it. Returns
/// whether anything happened, which is what an unavailable action must not
/// claim.
fn frame_selection<P>(
    scene: &LiveScene<P>,
    snapshot: &RenderSnapshot,
    camera: &mut ViewportInput,
) -> Result<bool> {
    camera.frame_extent(selection_bounds(scene, snapshot))
}

/// Shows the whole picture, and changes nothing else.
///
/// Takes the picture and nothing else: what is chosen is not an input to
/// showing everything, and a function that cannot see a selection cannot
/// disturb one. The extent is the snapshot's own, computed once when it was
/// packed – recomputing it here from the catalogue, the picks or the order the
/// definitions happen to be in would be a second answer to a settled question.
fn frame_scene(
    visibility: &Visibility,
    snapshot: &RenderSnapshot,
    camera: &mut ViewportInput,
) -> Result<bool> {
    camera.frame_extent(visibility.bounds(snapshot))
}

/// What one pixel of the model is a question about.
///
/// Most specific first, and from one hit rather than separate reads: a vertex
/// is the corner a person aimed at, an edge is the line beneath it, a face is
/// the surface behind that, and a definition is what is left when the picture
/// cannot say which subshape. Every arm comes from the same pixel of the same
/// frame, so the answer cannot describe one thing and be resolved against
/// another.
///
/// The edge arm is already coherent: [`Hit::edge`] gives nothing where the
/// edge target and the definition target disagree, which is exactly the outer
/// silhouette, where a line lands on a pixel the fill did not reach. So the
/// silhouette answers with the face or with nothing, never with an edge whose
/// definition is not there.
fn hovered_at(hit: Hit) -> Hovered {
    // The corner first, and only the coherent one. `Hit::vertex` is where the
    // aperture is checked against the definition, the face and the edge under
    // the same sample; the raw target deliberately reaches past the surface
    // and must never be read here.
    if hit.vertex() != ferritecad_viewport::VertexPickId::NOTHING {
        return Hovered::Vertex(hit.vertex());
    }
    if hit.edge() != EdgePickId::NOTHING {
        return Hovered::Edge(hit.edge());
    }
    if hit.face() != FacePickId::NOTHING {
        return Hovered::Face(hit.face());
    }
    if hit.definition() != PickId::NOTHING {
        return Hovered::Definition(hit.definition());
    }
    Hovered::Nothing
}

/// Records what the pointer is over, and says whether anything changed.
///
/// Answered through the picture that is on screen, so a question about a
/// picture that has been replaced marks nothing, whether it names a
/// definition, face, edge or vertex. Returns whether the answer differs from
/// the one already showing: pointing at the same thing again is not a reason
/// to draw the same picture twice.
///
/// Given the one field it may change and nothing else, so pointing at
/// something cannot choose it however this is called.
fn hover(hovered: &mut Hovered, snapshot: &RenderSnapshot, answer: Hovered) -> bool {
    let answer = answer.known_to(snapshot);
    if answer == *hovered {
        return false;
    }
    *hovered = answer;
    true
}

/// What the app must do with one hover question after the interface was drawn.
///
/// `EventResponse::consumed` is not enough for pointer motion: egui reports an
/// idle pointer over a panel as not consumed because no widget is actively
/// using it. The completed egui pass knows whether the pointer is over an
/// interface area, so that fact is an input here. A row is allowed to answer
/// directly; every other interface area blocks the model behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum HoverRequest {
    Unchanged,
    Clear,
    Row(usize),
    Pixel(f32, f32),
}

fn hover_request(row: Option<usize>, interface_has_pointer: bool, question: Hover) -> HoverRequest {
    if let Some(row) = row {
        return HoverRequest::Row(row);
    }
    if interface_has_pointer {
        return HoverRequest::Clear;
    }
    match question {
        Hover::Unchanged => HoverRequest::Unchanged,
        Hover::Cleared => HoverRequest::Clear,
        Hover::At(x, y) => HoverRequest::Pixel(x, y),
    }
}

/// The text one frame of the interface borrows.
///
/// Owned here because a panel borrows what it shows and cannot outlive the
/// frame, and built from the selection and the catalogue alone: every string
/// in it is a durable identifier or a display fact the loader sanitised.
struct Words {
    identities: Vec<String>,
    faces: Vec<TopologyWords>,
    /// The same six terms for a chosen edge. One type, three lists: what is
    /// chosen is one thing, so only one of these is ever non-empty.
    edges: Vec<TopologyWords>,
    /// And for a chosen topological vertex, on the same terms.
    vertices: Vec<TopologyWords>,
}

/// One durable name, in the words a person reads.
///
/// Portable terms only. There is no field here for an ordinal, a mesh index, a
/// handle, a session or a transient identity, because there is nothing true to
/// put in one: what names a face, an edge or a corner is the reference the
/// document stores. One type for all three, because the document stores the
/// same six terms about any of them.
struct TopologyWords {
    reference: String,
    owner: String,
    producer_feature: String,
    expected_kind: String,
    role: String,
    rule: String,
}

/// The identifier each row is described by.
fn identities_of(catalogue: &[CatalogueEntry]) -> Vec<String> {
    catalogue.iter().map(identity_of).collect()
}

/// What the interface borrows this frame.
fn words_of(
    selection: &Selection,
    catalogue: &[CatalogueEntry],
    snapshot: &RenderSnapshot,
) -> Words {
    let known = selection.owning_definition(snapshot).is_some();
    let faces = match selection {
        Selection::Face(face) if known => face.meanings().iter().map(topology_words).collect(),
        _ => Vec::new(),
    };
    let edges = match selection {
        Selection::Edge(edge) if known => edge.meanings().iter().map(topology_words).collect(),
        _ => Vec::new(),
    };
    let vertices = match selection {
        Selection::Vertex(vertex) if known => {
            vertex.meanings().iter().map(topology_words).collect()
        }
        _ => Vec::new(),
    };
    Words {
        identities: identities_of(catalogue),
        faces,
        edges,
        vertices,
    }
}

/// One stored reference, said in the document's own terms.
fn topology_words(meaning: &FaceMeaning) -> TopologyWords {
    TopologyWords {
        reference: meaning.reference.to_string(),
        owner: meaning.owner.to_string(),
        producer_feature: meaning.producer_feature.to_string(),
        expected_kind: meaning.expected_kind.as_str().to_owned(),
        role: describe_role(&meaning.output_role),
        rule: describe_rule(&meaning.selection),
    }
}

fn topology_name(words: &TopologyWords) -> ferritecad_ui::TopologyName<'_> {
    ferritecad_ui::TopologyName {
        reference: &words.reference,
        owner: &words.owner,
        producer_feature: &words.producer_feature,
        expected_kind: &words.expected_kind,
        role: &words.role,
        rule: &words.rule,
    }
}

/// What one solved sketch is called, and what its solve found out, in words.
///
/// Owned here for the same reason [`Words`] is: a panel borrows what it shows
/// and cannot outlive the frame. Every string is either a display name the
/// loader already had or a durable identifier the document stores; there is no
/// solver number here, and nothing in this type was obtained by asking
/// anything. The solve is long over by the time one of these exists.
struct SolveWords {
    name: Option<String>,
    object: String,
    degrees_of_freedom: usize,
    redundant: Vec<ConstraintWords>,
}

/// One constraint, in the words a person will read.
///
/// The identifier exactly as the document stores it, and one sentence saying
/// what the constraint is. Both are made here, from the durable rule whoever
/// had the document open carried out of it, and neither is a value formatted
/// for a programmer: the window has no `{:?}` to fall back on because there is
/// nothing here that could be missing.
///
/// One type for both places a constraint is named – a repeated one beside the
/// sketch it repeats in, and a conflicting one beside the Open that failed
/// over it – because they are the same two halves of the same thing. Two
/// types would be two vocabularies, and the first time either was changed a
/// window would say one rule two ways.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConstraintWords {
    identifier: String,
    says: String,
}

impl ConstraintWords {
    /// What one stored constraint is called and what it says.
    ///
    /// The one road from a durable rule to a sentence, taken by everything
    /// that shows a constraint. A second one would be a second vocabulary for
    /// the eight families the document can store.
    fn of(id: ferritecad_types::StableEntityId, rule: &SketchConstraintRule) -> Self {
        Self {
            identifier: id.to_string(),
            says: describe_constraint(rule),
        }
    }
}

impl SolveWords {
    /// Each repeated constraint, borrowed, in the order it arrived in.
    fn repeated(&self) -> Vec<ferritecad_ui::RedundantExplanation<'_>> {
        self.redundant
            .iter()
            .map(|constraint| ferritecad_ui::RedundantExplanation {
                identifier: &constraint.identifier,
                says: &constraint.says,
            })
            .collect()
    }
}

/// What the window has to say about the sketches behind the picture.
///
/// A translation and nothing more: one entry per fact, in the order the facts
/// arrived, which is the order the document stores its sketches. No solver is
/// asked anything, no document is opened, and nothing is sorted, deduplicated
/// or dropped – a sketch the loader accounted for has a row here or the window
/// is not saying what it was told.
fn solve_words_of(solves: &[SketchSolveFacts]) -> Vec<SolveWords> {
    solves
        .iter()
        .map(|facts| SolveWords {
            name: facts.name().map(str::to_owned),
            object: facts.sketch().to_string(),
            degrees_of_freedom: facts.report().degrees_of_freedom(),
            redundant: facts
                .redundant()
                .iter()
                .map(|constraint| ConstraintWords::of(constraint.id(), constraint.rule()))
                .collect(),
        })
        .collect()
}

/// The rows the section shows this frame, borrowed from this frame's words.
///
/// The repeated constraints arrive alongside rather than inside `words`
/// because a panel is given borrowed text and the owner of that text is the
/// frame. Both lists are built from one slice in one order, so the row a
/// person reads and the identifiers under it belong to the same sketch.
fn solved_sketches<'a>(
    words: &'a [SolveWords],
    repeated: &'a [Vec<ferritecad_ui::RedundantExplanation<'a>>],
) -> Vec<SolvedSketch<'a>> {
    words
        .iter()
        .zip(repeated)
        .map(|(sketch, repeated)| SolvedSketch {
            name: sketch.name.as_deref(),
            object: &sketch.object,
            degrees_of_freedom: sketch.degrees_of_freedom,
            redundant: repeated,
        })
        .collect()
}

/// What one repeated constraint says, as a sentence.
///
/// The document's own vocabulary, spelled out: which kind of relationship it
/// is, which parts of the drawing it holds together, and the size it asks for
/// when it asks for one. Every part of that is durable – a curve by the
/// identifier the document stores it under, a point of that curve by which
/// point of it it is – so the sentence still means the same thing after the
/// sketch has been reordered, edited or opened by another build.
///
/// The order the rule names its parts in is the order it is read out in.
/// `Coincident` says the same thing either way round, but a document that
/// stored one order and a window that printed the other would be a window
/// making its own decisions about what is written down, and the families that
/// do care about order sit in the same match arm as the ones that do not.
///
/// Every family the document can store is written out. `SketchConstraintRule`
/// is `#[non_exhaustive]`, so one more arm is unavoidable, and it says in
/// words that this build has no words rather than printing the value: an
/// explanation that could degrade into a `{:?}` is not an explanation, and the
/// one thing worse than "no words for it" is a line of punctuation offered as
/// though it were a sentence. Adding a family means adding an arm above.
fn describe_constraint(rule: &SketchConstraintRule) -> String {
    match *rule {
        SketchConstraintRule::Coincident { a, b } => {
            format!("Coincident: {a} and {b} are the same point")
        }
        SketchConstraintRule::Fixed { point, x, y } => {
            format!("Fixed: {point} is pinned at x {} mm, y {} mm", mm(x), mm(y))
        }
        SketchConstraintRule::Distance { a, b, distance } => {
            format!("Distance: {a} and {b} are {} mm apart", mm(distance))
        }
        SketchConstraintRule::Horizontal { a, b } => {
            format!("Horizontal: {a} and {b} share a y coordinate")
        }
        SketchConstraintRule::Vertical { a, b } => {
            format!("Vertical: {a} and {b} share an x coordinate")
        }
        SketchConstraintRule::EqualLength { a, b } => {
            format!("Equal length: {} is as long as {}", segment(a), segment(b))
        }
        SketchConstraintRule::Perpendicular { a, b } => {
            format!(
                "Perpendicular: {} meets {} at a right angle",
                segment(a),
                segment(b)
            )
        }
        SketchConstraintRule::Parallel { a, b } => {
            format!(
                "Parallel: {} runs in the same direction as {}",
                segment(a),
                segment(b)
            )
        }
        _ => "A kind of relationship this build has no words for".to_owned(),
    }
}

/// One straight run between two named points, read out end to end.
///
/// From and to in the order the document stores them. A segment is not a
/// curve and has no identifier of its own, so its two ends are the whole of
/// what there is to say about it.
fn segment(segment: SketchSegmentRef) -> String {
    format!("{} to {}", segment.from, segment.to)
}

/// A length the way a drawing states one.
///
/// Millimetres, which is the only unit this document has, printed as the
/// number that was stored rather than rounded to a house style: a size a
/// person typed is a size they should be able to recognise.
fn mm(value: f64) -> String {
    value.to_string()
}

/// What a stored role says, as a sentence.
///
/// The document's own vocabulary, spelled out. A role names what a subshape
/// *is* – the end cap of this extrusion, the side raised from that sketch
/// segment, or the edge where two such things meet – and every part of that
/// sentence is durable.
fn describe_role(role: &SemanticRole) -> String {
    match role {
        SemanticRole::ExtrudeCap { side } => match side {
            CapSide::Start => "Extrusion cap, start".to_owned(),
            CapSide::End => "Extrusion cap, end".to_owned(),
            other => format!("Extrusion cap, {other:?}"),
        },
        SemanticRole::ExtrudeSide { profile_segment } => {
            format!("Side raised from profile segment {profile_segment}")
        }
        SemanticRole::ExtrudeCapEdge {
            side,
            profile_segment,
        } => match side {
            // Which end and which segment, because either alone names four
            // edges of a plate rather than one. `CapSide` is non-exhaustive,
            // and a side this build has no words for is said the way the cap
            // faces beside it say one.
            CapSide::Start => format!("Start cap edge of profile segment {profile_segment}"),
            CapSide::End => format!("End cap edge of profile segment {profile_segment}"),
            other => format!("{other:?} cap edge of profile segment {profile_segment}"),
        },
        SemanticRole::ExtrudeCapVertex { side, joint } => {
            // Both segments and which end, because neither alone names one
            // point: the pair says which corner of the profile, and the side
            // says which of that corner's two ends. In the joint's own
            // canonical order, which is what makes the same corner read the
            // same way however the profile was walked.
            let [one, another] = joint.segments();
            match side {
                // `CapSide` is non-exhaustive, and a side this build has no
                // words for is said the way the cap faces and cap edges beside
                // it say one: the side alone, never the whole role.
                CapSide::Start => {
                    format!("Start cap vertex at the joint of profile segments {one} and {another}")
                }
                CapSide::End => {
                    format!("End cap vertex at the joint of profile segments {one} and {another}")
                }
                unknown => format!(
                    "{unknown:?} cap vertex at the joint of profile segments {one} and {another}"
                ),
            }
        }
        SemanticRole::ExtrudeSweepEdge { joint } => {
            // Both segments, in the order the joint keeps them, which is the
            // canonical one. Naming one of the two would describe a corner by
            // half of what it is, and the other half is what tells it from the
            // corner next to it.
            let [one, other] = joint.segments();
            format!("Sweep edge at the joint of profile segments {one} and {other}")
        }
        SemanticRole::SketchSegment { segment } => format!("Sketch segment {segment}"),
        SemanticRole::FilletFace { source_edge } => format!("Fillet of edge {source_edge}"),
        other => format!("{other:?}"),
    }
}

/// What a stored selection rule says.
fn describe_rule(rule: &SelectionRule) -> String {
    match rule {
        SelectionRule::Exact => "Exactly this one".to_owned(),
        SelectionRule::AllDerivedFrom { ancestor } => {
            format!("Everything derived from {ancestor}")
        }
        other => format!("{other:?}"),
    }
}

/// What the read-only inspector shows about what is chosen.
///
/// One resolution for both halves of the interface: the row a list marks and
/// the facts shown beside it come from the same selection, so they cannot
/// describe different things. A chosen face, edge or vertex is described as
/// that exact kind; a definition is described as the definition it is.
fn inspected<'a>(
    selection: &Selection,
    catalogue: &'a [CatalogueEntry],
    identities: &'a [String],
    faces: &'a [ferritecad_ui::TopologyName<'a>],
    edges: &'a [ferritecad_ui::TopologyName<'a>],
    vertices: &'a [ferritecad_ui::TopologyName<'a>],
    snapshot: &RenderSnapshot,
) -> Option<Selected<'a>> {
    let definition = selection.owning_definition(snapshot)?;
    let entry = catalogue.get(definition)?;
    let described = describe(entry, identities.get(definition)?);
    match (selection, described) {
        (Selection::Face(_), Selected::Body { name, object }) => Some(Selected::Face {
            name,
            object,
            names: faces,
        }),
        // An edge, on the same terms and for the same reason: only a native
        // body has durable edge names, so an edge of an imported definition is
        // never chosen as an edge and never reaches here.
        (Selection::Edge(_), Selected::Body { name, object }) => Some(Selected::Edge {
            name,
            object,
            names: edges,
        }),
        // A corner, on the same terms and for the same reason: only a native
        // body has durable corner names, so a corner of an imported definition
        // is never chosen as a corner and never reaches here.
        (Selection::Vertex(_), Selected::Body { name, object }) => Some(Selected::Vertex {
            name,
            object,
            names: vertices,
        }),
        // An imported definition has no durable face names, so a face of one
        // is never chosen as a face and never reaches here.
        (_, described) => Some(described),
    }
}

/// What the interface should say about a chosen definition.
///
/// The conversion lives here because this is where both halves are known: the
/// scene's identity types on one side, and a panel that must not learn what a
/// document is on the other. `Selected` offers no role for transient state;
/// the strings passed here come only from the durable identity and the display
/// facts sanitised while the scene catalogue was built.
fn describe<'a>(entry: &'a CatalogueEntry, identity: &'a str) -> Selected<'a> {
    match &entry.item {
        SceneItem::Body(_) => Selected::Body {
            name: entry.name.as_deref(),
            object: identity,
        },
        SceneItem::Imported(reference) => Selected::Imported {
            name: entry.name.as_deref(),
            source_file: entry.source_file.as_deref(),
            source: identity,
            definition_key: reference.definition_key(),
            solids: entry.solids,
        },
    }
}

/// The identifier the document stores for whatever this entry names.
///
/// One string, because only one applies: a body is named by its object and an
/// imported definition by the source its key belongs to. Formatting is the
/// caller's business because the borrow has to outlive the frame.
fn identity_of(entry: &CatalogueEntry) -> String {
    match &entry.item {
        SceneItem::Body(object) => object.to_string(),
        SceneItem::Imported(reference) => reference.source().to_string(),
    }
}

struct App {
    live: Option<Live>,
    input: ViewportInput,
    proxy: EventLoopProxy<AppEvent>,
    frames: FrameScheduler,
    document: PathBuf,
    loads: Loads,
}

impl ApplicationHandler<AppEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        // An unrelated OS event may wake the loop at or just after the
        // deadline, so check the clock for every batch rather than only for a
        // particular StartCause variant.
        if self.frames.take_due(Instant::now()) {
            self.request_frame_now(event_loop);
        }
    }

    /// Everything that needs a display is built here and not before.
    ///
    /// `resumed` is the only place a window may be created, and on some
    /// platforms it is called again after the application returns from the
    /// background. Building a second window on the second call is the usual
    /// way that goes wrong, so this returns early when one already exists.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.live.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(live) => {
                self.live = Some(live);
                // The window is up and empty; reading the document starts now
                // and finishes whenever it finishes.
                self.open(self.document.clone());
                // Construction, resize and framing all owe the first picture.
                // Do not depend on a platform happening to send another event
                // after `resumed` before the window first becomes visible.
                if self.input.take_redraw() {
                    self.request_frame_now(event_loop);
                }
            }
            Err(error) => {
                eprintln!("ferritecad: {error}");
                event_loop.exit();
            }
        }
    }

    /// The last thing the loop does, whichever way it came to an end.
    ///
    /// Every exit passes through here – the close button, a surface that could
    /// not be reconfigured, a frame that could not be drawn – so this is the
    /// one place a load in flight can be stopped without listing the ways a
    /// window can end.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.loads.stop_all();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::RepaintAt(deadline) => self.request_frame_at(event_loop, deadline),
            AppEvent::Progress { generation } => {
                advance_load(&mut self.loads, &mut self.input, generation);
                if self.input.take_redraw() {
                    self.request_frame_now(event_loop);
                }
            }
            AppEvent::Loaded { generation, result } => {
                // An answer to a question the user has since replaced is not
                // shown and is not announced: "this document could not be
                // opened", about a document they are no longer opening, is a
                // complaint about the wrong file arriving after they moved on.
                // Which of the two this is belongs to `Loads`, so the outcome
                // is reported the same way whatever it turns out to be.
                let outcome = if self.loads.accepts(generation) {
                    self.show(*result)
                } else {
                    (*result).map(|_| ())
                };
                let effect = finish_answer(&mut self.loads, &mut self.input, generation, outcome);
                if let Some(error) = effect.error {
                    eprintln!("ferritecad: {error}");
                }
                if self.input.take_redraw() {
                    self.request_frame_now(event_loop);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(live) = self.live.as_mut() else {
            return;
        };

        // The interface gets first refusal on every event, and says whether it
        // wanted it. What that answer means is the reducer's business.
        let response = live.egui_state.on_window_event(&live.window, &event);
        if response.repaint {
            self.input.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                // One size, applied to both. The reducer holds the camera and
                // hands back what the surface must be configured with, so the
                // two cannot be given different numbers.
                let (width, height) = self.input.resize(size.width, size.height);
                if let Err(error) = live.surface.resize(&live.renderer, width, height) {
                    eprintln!("ferritecad: {error}");
                    event_loop.exit();
                    return;
                }
            }
            WindowEvent::RedrawRequested => {
                self.frames.frame_started();
                let line = self.loads.status().line();
                let activity = Activity {
                    line: &line,
                    progress: self.loads.status().fraction(),
                    can_frame_selection: selection_bounds(
                        &live.scene,
                        live.scene.prepared.snapshot(),
                    )
                    .is_some(),
                    can_frame_scene: live
                        .scene
                        .visibility
                        .bounds(live.scene.prepared.snapshot())
                        .is_some(),
                    // Exactly when there is something chosen that is still
                    // being drawn. Nothing hidden can be chosen, so this is
                    // the whole of the condition.
                    can_hide: can_hide_selection(&live.scene, live.scene.prepared.snapshot()),
                    can_show_all: live.scene.visibility.anything_hidden(),
                    can_isolate: can_isolate_selection(&live.scene, live.scene.prepared.snapshot()),
                    can_undo_visibility: live
                        .scene
                        .visibility
                        .can_undo(live.scene.prepared.snapshot()),
                    orthographic: self.input.projection() == Projection::Orthographic,
                };
                // The words of the last failed attempt, borrowed for this
                // frame from the application's own account of it. Nothing is
                // computed here: these were written when the refusal arrived.
                let rules = self
                    .loads
                    .conflict()
                    .map(OpenConflict::rules)
                    .unwrap_or_default();
                let failure = self.loads.conflict().map(|conflict| conflict.shown(&rules));
                match live.draw(&self.input, activity, failure) {
                    // A button pressed during this frame reaches the camera
                    // the same way a keystroke does, through the reducer.
                    Ok((chosen, pointed_row, interface_has_pointer)) => {
                        if let Some(view) = chosen.view {
                            self.input.handle(ViewportEvent::Look(view), false);
                        }
                        // Asked for after the frame was published, never
                        // during it: a modal dialog runs its own event loop,
                        // and opening one while this application held an
                        // acquired surface texture would keep that texture for
                        // as long as the user browsed.
                        if chosen.open {
                            self.ask_for_a_document();
                        }
                        // Asked for after the frame as well: this is the frame
                        // whose button was pressed, and the line it drew is
                        // about to be replaced by the one naming what stays.
                        if chosen.cancel {
                            cancel_load(&mut self.loads, &mut self.input);
                        }
                        // A definition picked out of the list. The row is a
                        // position in the list that was just drawn; what it
                        // means is whatever the picture says sits there, and
                        // the picture is the only thing that can say.
                        if let Some(row) = chosen.definition {
                            self.choose_definition(row);
                        }
                        // A hidden definition asked back on screen, named by
                        // the picture that drew the list rather than by where
                        // its row happened to sit.
                        match chosen.row_visibility {
                            Some(RowVisibility::Show(requested)) => {
                                self.show_one_definition(requested);
                            }
                            Some(RowVisibility::Hide(requested)) => {
                                self.hide_one_definition(requested);
                            }
                            Some(RowVisibility::Neither) | None => {}
                        }
                        // The button and the key are two ways of asking for
                        // the same thing, and they ask the same function.
                        if chosen.frame {
                            self.frame_selection();
                        }
                        if chosen.frame_all {
                            self.frame_whole_scene();
                        }
                        // The button and the key ask the same function, as
                        // framing does.
                        if chosen.hide {
                            self.hide_chosen();
                        }
                        if chosen.isolate {
                            self.isolate_chosen();
                        }
                        if chosen.show_all {
                            self.show_everything();
                        }
                        if chosen.undo_visibility {
                            self.undo_last_visibility();
                        }
                        // The button and the key ask the same function, as
                        // every other pair here does.
                        if chosen.projection {
                            self.swap_projection();
                        }
                        // Asked after the frame that was clicked has been
                        // published, and only when somebody clicked: answering
                        // means drawing the model again offscreen to read one
                        // pixel of it.
                        if let Some((x, y)) = self.input.take_pick() {
                            self.choose_at(x, y);
                        }
                        // What the pointer is over. A row of the list answers
                        // for itself through the picture, and anywhere else it
                        // is the picture that is asked.
                        self.point_at(pointed_row, interface_has_pointer);
                    }
                    Err(error) => {
                        eprintln!("ferritecad: {error}");
                        event_loop.exit();
                    }
                }
            }
            // Asked for by name rather than translated into a camera event:
            // where to go depends on what the picture says is chosen, and the
            // reducer is handed the answer rather than asked to find it.
            WindowEvent::KeyboardInput { ref event, .. }
                if event.state == ElementState::Pressed
                    && requested(&event.logical_key, response.consumed).is_some() =>
            {
                // Which action a key asks for is decided by `requested`, where
                // it can be exercised without a window; this arm only carries
                // out the answer.
                match requested(&event.logical_key, response.consumed) {
                    Some(Requested::FrameSelection) => self.frame_selection(),
                    Some(Requested::FrameScene) => self.frame_whole_scene(),
                    Some(Requested::Hide) => self.hide_chosen(),
                    Some(Requested::Isolate) => self.isolate_chosen(),
                    Some(Requested::ShowAll) => self.show_everything(),
                    Some(Requested::Projection) => self.swap_projection(),
                    None => {}
                }
            }

            other => {
                // A double tap is the one gesture that has to know what is on
                // screen before it can say where to look, so it takes the
                // route that can read the picture. Every other gesture is
                // camera state alone and takes the route that cannot.
                if let Err(error) = magnify_gesture(
                    &live.scene,
                    live.scene.prepared.snapshot(),
                    &mut self.input,
                    &other,
                    response.consumed,
                    live.egui.egui_wants_pointer_input(),
                ) {
                    eprintln!("ferritecad: {error}");
                }
                apply_viewport_input(
                    &mut self.input,
                    &other,
                    response.consumed,
                    live.egui.egui_wants_pointer_input(),
                );
            }
        }

        // The reducer collapses semantic reasons; the scheduler below
        // collapses the actual request made to the window.
        if self.input.take_redraw() {
            self.request_frame_now(event_loop);
        }
    }
}

impl App {
    fn new(proxy: EventLoopProxy<AppEvent>, document: PathBuf) -> Self {
        Self {
            live: None,
            input: ViewportInput::new(),
            proxy,
            frames: FrameScheduler::default(),
            document,
            loads: Loads::default(),
        }
    }

    /// Asks the system for a document, and opens it if one was chosen.
    ///
    /// Blocking on purpose. A file dialog is modal: while it is up, the user
    /// is choosing a file and not turning a model, and every toolkit runs the
    /// window's events for the duration. Reading the document it names is the
    /// part that must not block, and that has its own thread.
    ///
    /// A cancelled dialog is an answer, not a failure, and leaves the document
    /// already on screen exactly as it was.
    fn ask_for_a_document(&mut self) {
        // A toolbar cannot be drawn without a live window, so reaching this
        // without one would be a wiring error. Do not silently turn that into
        // an unowned top-level dialog: its parent is what keeps it in front of
        // this viewer and gives the XDG portal a non-empty window identifier.
        let Some(live) = &self.live else {
            return;
        };
        let chosen = rfd::FileDialog::new()
            .set_title("Open a document")
            .add_filter("FerriteCAD document", &[DOCUMENT_EXTENSION])
            .set_directory(
                self.document
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )
            .set_parent(live.window.as_ref())
            .pick_file();

        if let Some(path) = chosen {
            self.open(path);
        }
    }

    /// Starts reading a document on a thread of its own.
    ///
    /// Whatever was being read is abandoned. The reading that replaces it is
    /// the only one whose answer can reach the screen, however the two
    /// readings finish relative to each other.
    fn open(&mut self, path: PathBuf) {
        self.document = path;
        let path = self.document.clone();
        let proxy = self.proxy.clone();
        let waking = self.proxy.clone();
        let chosen = self.document.clone();

        // One relay per load: the worker writes the newest fraction into it,
        // the event loop reads it when it gets there, and the two of them wake
        // each other no more than once per frame's worth of chatter.
        let relay = Arc::new(ProgressRelay::default());
        let reports = Arc::clone(&relay);

        begin_load(
            &mut self.loads,
            &mut self.input,
            Some(&chosen),
            relay,
            |generation, cancel| {
                let context = OperationContext::default()
                    .with_cancel(cancel.clone())
                    .with_progress(ProgressSink::new(move |fraction| {
                        // Woken only when nothing is outstanding. A reading
                        // that reports a thousand times while the loop is busy
                        // leaves one event behind it, holding the newest
                        // number rather than the first.
                        if reports.record(fraction) {
                            let _ = waking.send_event(AppEvent::Progress { generation });
                        }
                    }));
                spawn_load(
                    move || {
                        // The kernel is made and dropped inside the worker. An Open
                        // CASCADE session belongs to the thread that opened it, and
                        // ending it with the thread means an abandoned load cannot
                        // outlive the shapes it was holding.
                        let mut kernel = OcctKernel::new()?;
                        snapshot_of(
                            &path,
                            &mut kernel,
                            // How this kernel re-reads a STEP file the document
                            // stores. Handed over as a function so one session
                            // builds both the rebuilt bodies and the imported ones.
                            |kernel, source| kernel.import_step(source),
                            &TessellationParams::default(),
                            &context,
                        )
                    },
                    move |result| {
                        // A closed event loop is an ordinary end state, and there
                        // is nowhere useful to report a failed wake-up after it.
                        let _ = proxy.send_event(AppEvent::Loaded {
                            generation,
                            result: Box::new(result),
                        });
                    },
                )
            },
        );
    }

    /// Answers "what is under this point" and chooses it.
    ///
    /// One offscreen frame at the camera the window is already using, because
    /// identities are not written on the path a window takes: paying for them
    /// on every frame would be paying continuously for something wanted when
    /// somebody clicks.
    fn choose_at(&mut self, x: f32, y: f32) {
        let Some(live) = self.live.as_mut() else {
            return;
        };

        // Rounded rather than truncated, and refused outside the picture: a
        // pointer position is a float in window coordinates and the frame is a
        // grid of pixels.
        let (Ok(x), Ok(y)) = (
            u32::try_from(x.round() as i64),
            u32::try_from(y.round() as i64),
        ) else {
            return;
        };

        match Self::hit_at(live, self.input.camera(), x as f32, y as f32) {
            Ok(hit) => {
                let chosen = selection_at(
                    hit,
                    live.scene.prepared.snapshot(),
                    &live.scene.faces,
                    &live.scene.edges,
                    &live.scene.vertices,
                );
                if chosen != live.scene.selection {
                    live.scene.selection = chosen;
                    self.input.request_redraw();
                }
            }
            // A failed pick chooses nothing and changes nothing. The model is
            // still on screen and still correct; the click simply went
            // unanswered.
            Err(error) => eprintln!("ferritecad: {error}"),
        }
    }

    /// Shows what is chosen, if the picture can say where it is.
    ///
    /// The one operation behind both the button and the key. Where the camera
    /// should go is the reducer's decision and is made in one place; what is
    /// on screen is the picture's business and is answered by the picture.
    fn frame_selection(&mut self) {
        let Some(live) = self.live.as_ref() else {
            return;
        };
        // Nothing to show means nothing moved and no frame owed, which is not
        // a failure and is not reported as one.
        if let Err(error) =
            frame_selection(&live.scene, live.scene.prepared.snapshot(), &mut self.input)
        {
            eprintln!("ferritecad: {error}");
        }
    }

    /// Stops drawing what is chosen, if anything chosen is being drawn.
    ///
    /// The rule lives in `hide_selected`, where it can be exercised without a
    /// window; this is the wiring that hands it the picture. An action with
    /// nothing to do asks for nothing, which is what makes a disabled button
    /// and an unavailable key agree.
    fn hide_chosen(&mut self) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let scene = &mut live.scene;
        hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            scene.prepared.snapshot(),
            &mut self.input,
        );
    }

    /// Stops drawing everything except what is chosen.
    fn isolate_chosen(&mut self) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let scene = &mut live.scene;
        isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            scene.prepared.snapshot(),
            &mut self.input,
        );
    }

    /// Draws one hidden definition again.
    fn show_one_definition(&mut self, requested: PickId) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let scene = &mut live.scene;
        show_one(
            &mut scene.visibility,
            &mut scene.hovered,
            scene.prepared.snapshot(),
            requested,
            &mut self.input,
        );
    }

    /// Stops drawing one definition, named by its row.
    fn hide_one_definition(&mut self, requested: PickId) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let scene = &mut live.scene;
        hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            scene.prepared.snapshot(),
            requested,
            &mut self.input,
        );
    }

    /// Draws the model through the other projection.
    fn swap_projection(&mut self) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let to = other_projection(self.input.projection());
        change_projection(&mut self.input, &mut live.scene.hovered, to);
    }

    /// Takes back the last change to what is drawn.
    fn undo_last_visibility(&mut self) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let scene = &mut live.scene;
        undo_visibility(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            scene.prepared.snapshot(),
            &mut self.input,
        );
    }

    /// Draws every definition again.
    fn show_everything(&mut self) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        show_all(
            &mut live.scene.visibility,
            &mut live.scene.hovered,
            &mut self.input,
        );
    }

    /// Shows the whole model, wherever the camera had wandered to.
    ///
    /// The one operation behind both the button and the key.
    fn frame_whole_scene(&mut self) {
        let Some(live) = self.live.as_ref() else {
            return;
        };
        if let Err(error) = frame_scene(
            &live.scene.visibility,
            live.scene.prepared.snapshot(),
            &mut self.input,
        ) {
            eprintln!("ferritecad: {error}");
        }
    }

    /// Records what the pointer is over, from whichever side asked.
    ///
    /// A row of the list answers for itself: it knows which definition it
    /// draws, and asks the picture for that definition's identity rather than
    /// asking what pixel is under a panel. Anywhere else the picture is asked
    /// where the pointer is, once, and only when the pointer moved.
    ///
    /// Nothing here touches the selection. Pointing at something is a question
    /// about it, and a viewer that chose whatever the pointer crossed would
    /// make the choice worthless.
    fn point_at(&mut self, row: Option<usize>, interface_has_pointer: bool) {
        let question = self.input.take_hover();
        let Some(live) = self.live.as_mut() else {
            return;
        };

        let answer = match hover_request(row, interface_has_pointer, question) {
            // The list said which one, which needs no pixel read at all. A row
            // names a definition and can say nothing about a face, edge or
            // vertex: a list of definitions has none of them in it.
            HoverRequest::Row(row) => live
                .scene
                .prepared
                .snapshot()
                .pick_of(row)
                .map(Hovered::Definition),
            HoverRequest::Pixel(x, y) => {
                // One offscreen frame, and only because the pointer moved. A
                // pixel is the only thing that knows which face, edge or
                // vertex it came from.
                match Self::hit_at(live, self.input.camera(), x, y) {
                    Ok(hit) => Some(hovered_at(hit)),
                    Err(error) => {
                        eprintln!("ferritecad: {error}");
                        return;
                    }
                }
            }
            // Away from the model, over a panel, or in the middle of a
            // gesture: whatever was under the pointer is not any more.
            HoverRequest::Clear => Some(Hovered::Nothing),
            // Nothing moved, so nothing changed.
            HoverRequest::Unchanged => None,
        };

        let Some(answer) = answer else {
            return;
        };
        if hover(
            &mut live.scene.hovered,
            live.scene.prepared.snapshot(),
            answer,
        ) {
            self.input.request_redraw();
        }
    }

    /// Reads one pixel and all four answers about it.
    ///
    /// The definition, face, edge and vertex come from one pixel of one frame,
    /// so they cannot describe different geometry. The click and hover
    /// decisions consume the same coherent answers with their own precedence
    /// and lifetime rules.
    fn hit_at(live: &mut Live, camera: &Camera, x: f32, y: f32) -> Result<Hit> {
        let (Ok(x), Ok(y)) = (
            u32::try_from(x.round() as i64),
            u32::try_from(y.round() as i64),
        ) else {
            return Ok(Hit::NOTHING);
        };
        let frame = live.renderer.render(
            &live.scene.prepared,
            camera,
            Marked::Nothing,
            Hovered::Nothing,
            &live.scene.visibility,
        )?;
        Ok(frame.hit_at(x, y))
    }

    /// Chooses the definition a list row names, if the picture has one there.
    ///
    /// The row is turned into an identity by the snapshot that drew the list,
    /// so a position can never become a name for anything: a row of a picture
    /// that has since been replaced resolves to nothing, exactly as a click
    /// made in that picture would.
    fn choose_definition(&mut self, row: usize) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        select_definition_row(
            &mut live.scene.selection,
            live.scene.prepared.snapshot(),
            &mut self.input,
            row,
        );
    }

    /// Puts a finished load on screen, or leaves the screen alone.
    ///
    /// What a failure means to the camera is the reducer's rule and is tested
    /// there. What is left here is that a failure is reported and nothing else
    /// happens: the model already on screen stays on screen, because a viewer
    /// that went blank would lose the drawing the user was reading while they
    /// work out what went wrong.
    fn show(&mut self, loaded: Result<LoadedScene>) -> Result<()> {
        let Some(live) = self.live.as_mut() else {
            // No window to show it in, which means the loop is already on its
            // way out. The outcome is still the outcome; nothing here changes
            // state that can no longer be observed.
            return loaded.map(|_| ());
        };

        // Everything that can fail happens inside `prepare_load`; committing
        // cannot. No event observes the application in between, so the picture,
        // what its parts are, the choice made in it and the camera all become
        // current together – and only then is the document a thing the window
        // may call ready.
        let next = prepare_load(&self.input, loaded, |snapshot| {
            live.renderer.prepare(snapshot)
        });
        commit_scene(&mut live.scene, &mut self.input, next)
    }

    fn request_frame_now(&mut self, event_loop: &ActiveEventLoop) {
        // An immediate request cancels a previously installed WaitUntil.
        event_loop.set_control_flow(ControlFlow::Wait);
        if self.frames.request_now()
            && let Some(live) = &self.live
        {
            live.window.request_redraw();
        }
    }

    fn request_frame_at(&mut self, event_loop: &ActiveEventLoop, deadline: Instant) {
        let now = Instant::now();
        if deadline <= now {
            self.request_frame_now(event_loop);
        } else if let Some(deadline) = self.frames.request_at(deadline, now) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        }
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<Live> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("FerriteCAD")
                        .with_inner_size(winit::dpi::LogicalSize::new(1024.0, 768.0)),
                )
                .map_err(|error| {
                    ferritecad_types::CadError::rendering_because("creating the window", error)
                })?,
        );

        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|error| {
                ferritecad_types::CadError::rendering_because(
                    "creating a surface for the window",
                    error,
                )
            })?;

        // The adapter is chosen for this surface, not for the machine: see
        // Renderer::for_surface.
        let mut renderer = Renderer::for_surface(&instance, &surface)?;
        let size = window.inner_size();
        let (width, height) = self.input.resize(size.width, size.height);
        let window_surface = WindowSurface::new(&renderer, surface, width, height)?;

        // Empty until the document arrives. The window is worth opening before
        // then: it is how the user learns that the file is being read rather
        // than that nothing happened.
        let prepared = renderer.prepare(Arc::new(SnapshotBuilder::new().build()))?;

        let egui = egui::Context::default();
        let repaint_proxy = self.proxy.clone();
        egui.set_request_repaint_callback(move |request| {
            if request.viewport_id != egui::ViewportId::ROOT {
                return;
            }
            let Some(deadline) = Instant::now().checked_add(request.delay) else {
                return;
            };
            // Closing the event loop is an ordinary end state. There is no
            // useful place to report a failed wake-up after that point.
            let _ = repaint_proxy.send_event(AppEvent::RepaintAt(deadline));
        });
        let egui_state = egui_winit::State::new(
            egui.clone(),
            egui.viewport_id(),
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            renderer.device(),
            window_surface.format(),
            egui_wgpu::RendererOptions {
                // The interface draws over a frame whose depth buffer belongs
                // to the model's pass and is already finished with.
                depth_stencil_format: None,
                msaa_samples: 1,
                ..Default::default()
            },
        );

        Ok(Live {
            window,
            renderer,
            surface: window_surface,
            scene: LiveScene::new(
                prepared,
                Vec::new(),
                FaceNames::default(),
                EdgeNames::default(),
                VertexNames::default(),
                Visibility::default(),
                Vec::new(),
                Vec::new(),
            ),
            egui,
            egui_state,
            egui_renderer,
        })
    }
}

impl Live {
    /// One frame: the model, then the interface, then publication.
    ///
    /// One texture, acquired once. The order is not a convention here – the
    /// seam enforces it, because the model's pass is what clears the target
    /// and the type only offers a view to draw into afterwards.
    fn draw(
        &mut self,
        input: &ViewportInput,
        activity: Activity<'_>,
        failure: Option<ferritecad_ui::OpenFailure<'_>>,
    ) -> Result<(Chosen, Option<usize>, bool)> {
        // Taken apart so the picture can be read while the surface is being
        // drawn into: these are different fields, and only the compiler needs
        // telling. It also means the list below describes the catalogue itself
        // rather than a copy of it made once a frame.
        let Self {
            window,
            renderer,
            surface,
            scene,
            egui,
            egui_state,
            egui_renderer,
        } = self;

        // What each definition is, and what the document calls the chosen
        // face if one is chosen, in the words a panel is allowed to use. Owned
        // because `Selected` borrows them and they must outlive the frame.
        let words = words_of(
            &scene.selection,
            &scene.catalogue,
            scene.prepared.snapshot(),
        );
        let face_names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.faces.iter().map(topology_name).collect();
        let edge_names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.edges.iter().map(topology_name).collect();
        let vertex_names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.vertices.iter().map(topology_name).collect();
        // One answer for both sides of the choice: the row a list marks and
        // the facts an inspector shows come from the same resolution.
        let (definitions, chosen_row) = scene.view(&words.identities, scene.prepared.snapshot());
        // What each row can offer about whether it is drawn, and which rows
        // are not being drawn: one answer, read from the same visibility the
        // renderer reads, so a row marked hidden and a definition missing from
        // the picture cannot be two different sets.
        let offers = rows_visibility(&scene.visibility, scene.prepared.snapshot());
        // What the solve of each constrained sketch of this document found
        // out, in the words a panel may use. Built from what the load carried
        // and from nothing else: no solver is asked, and the document is not
        // opened again.
        let solve_words = solve_words_of(&scene.sketch_solves);
        let repeated: Vec<Vec<ferritecad_ui::RedundantExplanation<'_>>> =
            solve_words.iter().map(SolveWords::repeated).collect();
        let solves = solved_sketches(&solve_words, &repeated);
        let described = inspected(
            &scene.selection,
            &scene.catalogue,
            &words.identities,
            &face_names,
            &edge_names,
            &vertex_names,
            scene.prepared.snapshot(),
        );

        let Some(frame) = surface.begin(renderer)? else {
            // No area, nobody watching, or the compositor was busy. None of
            // those is an error.
            return Ok((Chosen::default(), None, false));
        };
        let frame = frame.draw_scene(
            &scene.prepared,
            input.camera(),
            scene.selection.marked(),
            scene.hovered,
            &scene.visibility,
        )?;

        let raw_input = egui_state.take_egui_input(window);
        let mut chosen = Chosen::default();
        let mut pointed_row = None;
        let mut output = egui.run_ui(raw_input, |ui| {
            // The panel returns what was asked for and applies nothing. What a
            // request means to the camera is the reducer's, and having one
            // place for that is what stops a button and a keystroke drifting
            // apart.
            chosen = ferritecad_ui::toolbar(ui, activity);
            ui.separator();
            // Why the last attempt to open a document failed, when there is
            // more to it than the line the toolbar already shows. Beside the
            // status line because that is what it belongs to, and above
            // everything that describes the model because it describes another
            // document: the picture below is the one that did open.
            ferritecad_ui::open_failure_panel(ui, failure);
            // Every definition in the picture, whether or not any of it is on
            // screen: a part hidden behind another, too small to hit or out of
            // shot is reachable here and nowhere else.
            let rows = ferritecad_ui::definitions_panel(ui, &definitions, chosen_row, &offers);
            chosen.definition = rows.pressed;
            chosen.row_visibility = rows.visibility;
            pointed_row = rows.hovered;
            ui.separator();
            // Read-only, and the only place the choice is described. What it
            // is allowed to say is decided by `Selected`, which cannot name
            // anything that means something only to this frame.
            ferritecad_ui::selection_inspector(ui, described);
            ui.separator();
            // What the rebuild behind this picture found out about the
            // drawings it was built from. Not an inspector and not part of the
            // choice: a sketch is not drawn in the viewport and cannot be
            // picked, so this is a third kind of thing the window knows and it
            // is shown as one.
            ferritecad_ui::sketch_solves_panel(ui, &solves);
        });
        // Asked after the pass, when egui knows the areas it just laid out.
        // `EventResponse::consumed` deliberately means something narrower for
        // CursorMoved and is false for an idle pointer over a toolbar.
        let interface_has_pointer = egui.egui_wants_pointer_input();
        egui_state.handle_platform_output(window, output.platform_output);

        let (width, height) = frame.size();
        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: egui.pixels_per_point(),
        };
        let primitives = egui.tessellate(output.shapes, egui.pixels_per_point());
        let mut textures = std::mem::take(&mut output.textures_delta);
        upload_textures(&mut textures, |id, delta| {
            egui_renderer.update_texture(frame.device(), frame.queue(), id, delta);
        });

        let mut encoder = frame
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ferritecad interface"),
            });
        egui_renderer.update_buffers(
            frame.device(),
            frame.queue(),
            &mut encoder,
            &primitives,
            &descriptor,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ferritecad interface pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: frame.view(),
                        depth_slice: None,
                        resolve_target: None,
                        // Load, never clear: the model is already in there.
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            egui_renderer.render(&mut pass, &primitives, &descriptor);
        }
        frame.queue().submit(Some(encoder.finish()));

        free_textures(&mut textures, |id| egui_renderer.free_texture(id));
        frame.present();
        Ok((chosen, pointed_row, interface_has_pointer))
    }
}

/// Turns one window event into what this application means by it.
///
/// Small and obvious on purpose: everything with a decision in it is on the
/// other side of this function, where it can be tested.
fn translate(event: &WindowEvent) -> Vec<ViewportEvent> {
    match event {
        WindowEvent::CursorMoved { position, .. } => vec![ViewportEvent::PointerMoved {
            x: position.x as f32,
            y: position.y as f32,
        }],
        WindowEvent::MouseInput { state, button, .. } => {
            let Some(button) = button_of(*button) else {
                return Vec::new();
            };
            vec![match state {
                ElementState::Pressed => ViewportEvent::PointerPressed(button),
                ElementState::Released => ViewportEvent::PointerReleased(button),
            }]
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let amount = match delta {
                MouseScrollDelta::LineDelta(_, lines) => *lines,
                // A trackpad reports pixels, which are a much finer unit than
                // a wheel's notches; scaled so both feel like the same gesture.
                MouseScrollDelta::PixelDelta(position) => position.y as f32 / 40.0,
            };
            vec![ViewportEvent::Wheel { delta: amount }]
        }
        // Two fingers on a trackpad, which macOS reports as a magnification
        // delta rather than as a number of notches. The phase is deliberately
        // not translated into a semantic event of its own: it describes this
        // gesture's lifetime, not focus loss. In particular, a zero-delta
        // start, end or cancellation must not drop the pointer or end an
        // unrelated mouse drag.
        WindowEvent::PinchGesture { delta, .. } => {
            vec![ViewportEvent::Pinch {
                delta: *delta as f32,
            }]
        }
        // Two fingers turning, which macOS reports in degrees and counts
        // positive counterclockwise. The camera works in radians, so the one
        // place the two units meet is here. Like a pinch, the phase describes
        // this gesture's lifetime and is not translated into anything of its
        // own: a turn of nothing must not drop the pointer or end a mouse drag.
        WindowEvent::RotationGesture { delta, .. } => {
            vec![ViewportEvent::Roll {
                radians: delta.to_radians(),
            }]
        }
        WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
            named_view(&event.logical_key)
                .map(|view| vec![ViewportEvent::Look(view)])
                .unwrap_or_default()
        }
        WindowEvent::Focused(false) => vec![ViewportEvent::GestureCancelled],
        // The pointer is somewhere else entirely, so nothing is under it. A
        // highlight left behind would claim it still was.
        WindowEvent::CursorLeft { .. } => vec![ViewportEvent::PointerLeft],
        _ => Vec::new(),
    }
}

/// Whether the interface, rather than the model, is what an event was for.
///
/// A free function so the rule can be stated as a test: a window is needed to
/// produce the two answers, but not to decide what they mean together.
///
/// Anything the interface actually consumed belongs to it. Beyond that, the
/// events that point at something - a press, a move, a wheel, a pinch - also
/// belong to it whenever it wants the pointer at all, because they would
/// otherwise reach the model through the panel drawn over it. A pinch is one
/// of those: two fingers over a list are a gesture about the list.
fn claimed_by_interface(event: &ViewportEvent, consumed: bool, wants_pointer: bool) -> bool {
    match event {
        ViewportEvent::Wheel { .. }
        | ViewportEvent::Pinch { .. }
        | ViewportEvent::Roll { .. }
        | ViewportEvent::PointerPressed(_)
        | ViewportEvent::PointerMoved { .. } => pointer_gesture_claimed(consumed, wants_pointer),
        // A move is claimed only while no gesture is running; the reducer
        // keeps a drag that began in the viewport.
        _ => consumed,
    }
}

/// Translates one window event and gives only viewport state to its answers.
///
/// Selection and visibility are deliberately absent: camera gestures cannot
/// change either through this semantic route. Keeping the small piece of
/// window wiring free also lets tests drive the same path without a window.
fn apply_viewport_input(
    input: &mut ViewportInput,
    event: &WindowEvent,
    consumed: bool,
    wants_pointer: bool,
) {
    for event in translate(event) {
        let claimed = claimed_by_interface(&event, consumed, wants_pointer);
        input.handle(event, claimed);
    }
}

/// What a double tap on a trackpad asks to be looked at.
///
/// What is chosen, if anything chosen is drawn; otherwise everything that is
/// still on screen. A double tap carries no position and no geometry, so it
/// cannot mean "this part here": it means "the thing I am working on", and
/// what the user is working on is what they chose. With nothing chosen it can
/// only mean the picture, and the picture is what is visible in it rather
/// than everything the file happens to contain.
///
/// Both answers are the extents the rest of the application already frames
/// with. Neither is recomputed here.
fn magnified_bounds<P>(
    scene: &LiveScene<P>,
    snapshot: &RenderSnapshot,
) -> Option<([f32; 3], [f32; 3])> {
    selection_bounds(scene, snapshot).or_else(|| scene.visibility.bounds(snapshot))
}

/// One level of smart magnification, and the way back from it.
///
/// The semantic route a window takes for a double tap, callable without one.
/// The picture and what is chosen in it arrive by shared reference: this
/// route reads them to decide where to look and cannot change either.
///
/// A gesture the interface wanted is inert, exactly as a wheel, a pinch or a
/// turn over a panel is. Any other event is not this gesture and is left to
/// the camera route beside this one.
fn magnify_gesture<P>(
    scene: &LiveScene<P>,
    snapshot: &RenderSnapshot,
    camera: &mut ViewportInput,
    event: &WindowEvent,
    consumed: bool,
    wants_pointer: bool,
) -> Result<bool> {
    if !matches!(event, WindowEvent::DoubleTapGesture { .. })
        || pointer_gesture_claimed(consumed, wants_pointer)
    {
        return Ok(false);
    }
    camera.magnify(magnified_bounds(scene, snapshot))
}

/// Whether a gesture aimed at whatever is under the pointer belongs to the
/// interface rather than to the model.
///
/// One statement, so the double tap cannot drift from the wheel, the pinch and
/// the turn: a panel drawn over the viewport wants the pointer, and a gesture
/// made over it is about the panel.
fn pointer_gesture_claimed(consumed: bool, wants_pointer: bool) -> bool {
    consumed || wants_pointer
}

fn button_of(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Middle => Some(PointerButton::Middle),
        MouseButton::Right => Some(PointerButton::Secondary),
        _ => None,
    }
}

/// What one keystroke asks the window to do.
///
/// Named rather than translated into a camera event: where to go depends on
/// what the picture says is chosen, and the reducer is handed the answer
/// rather than asked to find it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Requested {
    FrameSelection,
    FrameScene,
    Hide,
    Isolate,
    ShowAll,
    Projection,
}

/// Which action an unclaimed key asks for, if any.
///
/// One place, so every one of these shortcuts obeys the same rule about what
/// the interface has already claimed, and so that rule can be exercised
/// without opening a window. Each key is read from the constant its button
/// prints.
fn requested(key: &Key, claimed_by_ui: bool) -> Option<Requested> {
    for (shortcut, action) in [
        (FRAME_KEY, Requested::FrameSelection),
        (FRAME_ALL_KEY, Requested::FrameScene),
        (HIDE_KEY, Requested::Hide),
        (ISOLATE_KEY, Requested::Isolate),
        (SHOW_ALL_KEY, Requested::ShowAll),
        (PROJECTION_KEY, Requested::Projection),
    ] {
        if wants(key, claimed_by_ui, shortcut) {
            return Some(action);
        }
    }
    None
}

/// Whether this unclaimed key is the one printed on a particular button.
///
/// `shortcut` is read from the same constant the panel prints, for the same
/// reason the view keys are: a shortcut that drifts from its label is a
/// shortcut nobody can trust. The interface has first refusal, just as it does
/// for view keys: a focused text control that accepted an `F` did not also ask
/// to move the model camera. Case is ignored because a keyboard reports what
/// was typed and the button prints one of the two.
///
/// One function for every such button, so a fourth action cannot arrive with a
/// fourth almost-identical rule about what counts as claimed.
fn wants(key: &Key, claimed_by_ui: bool, shortcut: &str) -> bool {
    if claimed_by_ui {
        return false;
    }
    let Key::Character(text) = key else {
        return false;
    };
    text.eq_ignore_ascii_case(shortcut)
}

/// The number keys a drawing office would expect.
fn named_view(key: &Key) -> Option<StandardView> {
    let Key::Character(text) = key else {
        return match key {
            Key::Named(NamedKey::Home) => Some(StandardView::Isometric),
            _ => None,
        };
    };
    VIEWS
        .iter()
        .find_map(|(view, _, shortcut)| (*shortcut == text.as_str()).then_some(*view))
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use std::time::Duration;

    use ferritecad_viewport::PickId;

    use super::*;

    /// A picture with nothing catalogued, for tests that only look at where
    /// the camera ends up.
    /// One catalogue entry naming a body nobody else names.
    fn a_body() -> CatalogueEntry {
        CatalogueEntry {
            item: SceneItem::Body(ferritecad_types::ObjectId::new()),
            name: Some("Plate".to_owned()),
            source_file: None,
            solids: None,
        }
    }

    fn loaded(snapshot: RenderSnapshot) -> LoadedScene {
        LoadedScene {
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            snapshot,
            catalogue: Vec::new(),
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        }
    }

    fn distant_scene() -> RenderSnapshot {
        scene_at(900.0)
    }

    /// The triangle `scene_at` packs, on its own.
    fn distant_scene_mesh() -> ferritecad_kernel::Mesh {
        use ferritecad_kernel::{Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeKind};

        let mut mesh = Mesh::default();
        mesh.positions
            .extend_from_slice(&[0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0]);
        mesh.normals
            .extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
        mesh.indices.extend_from_slice(&[0, 1, 2]);
        mesh.faces.push(MeshFaceRange {
            face: ferritecad_kernel::SubShapeHandle::new(
                ShapeHandle::new(SessionId::new(), 1),
                SubShapeKind::Face,
                0,
            ),
            first_index: 0,
            index_count: 3,
        });
        mesh
    }

    /// One triangle, somewhere nobody else is.
    ///
    /// Two documents have to be told apart by where the camera ends up, so the
    /// only thing that varies is the place.
    fn scene_at(x: f32) -> RenderSnapshot {
        use ferritecad_kernel::{
            Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeHandle, SubShapeKind,
        };
        use ferritecad_types::Transform;

        let mut mesh = Mesh::default();
        mesh.positions.extend_from_slice(&[
            x,
            900.0,
            900.0,
            x + 10.0,
            900.0,
            900.0,
            x,
            910.0,
            900.0,
        ]);
        mesh.normals
            .extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
        mesh.indices.extend_from_slice(&[0, 1, 2]);
        mesh.faces.push(MeshFaceRange {
            face: SubShapeHandle::new(ShapeHandle::new(SessionId::new(), 1), SubShapeKind::Face, 0),
            first_index: 0,
            index_count: 3,
        });

        let mut builder = SnapshotBuilder::new();
        let definition = builder.add_mesh(&mesh).expect("the mesh is valid");
        builder
            .place(definition, None, &Transform::IDENTITY, [0.5, 0.5, 0.5])
            .expect("places it");
        builder.build()
    }

    /// A relay for a load whose progress the test does not look at.
    fn relay() -> Arc<ProgressRelay> {
        Arc::new(ProgressRelay::default())
    }

    /// A worker that does nothing until it is let go, or cancelled.
    ///
    /// Real threads and a real token: what these tests are about is what
    /// happens while a reading is still going on, and a stand-in that finished
    /// immediately would have no such moment.
    fn held_worker(cancel: &CancelToken) -> (JoinHandle<()>, std::sync::mpsc::Sender<()>) {
        let (release, held) = std::sync::mpsc::channel::<()>();
        let cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            while !cancel.is_cancelled() {
                match held.recv_timeout(Duration::from_millis(5)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });
        (worker, release)
    }

    #[test]
    fn every_request_gets_its_own_generation() {
        let mut loads = Loads::default();
        let mut holds = Vec::new();

        let mut generations = Vec::new();
        for _ in 0..3 {
            generations.push(
                loads
                    .open(Some(Path::new("a.fcad")), relay(), |_, cancel| {
                        let (worker, release) = held_worker(cancel);
                        holds.push(release);
                        worker
                    })
                    .expect("a document was named, so a load started"),
            );
        }

        // Monotonic, so a late answer can be recognised as late rather than as
        // coincidentally equal to something started afterwards.
        assert!(generations[0] < generations[1] && generations[1] < generations[2]);
        assert!(loads.accepts(generations[2]));
        assert!(
            !loads.accepts(generations[0]),
            "an abandoned load is current"
        );

        loads.stop_all();
    }

    #[test]
    fn opening_a_second_document_stops_the_first_without_waiting_for_it() {
        let mut loads = Loads::default();
        let mut first = None;
        let mut holds = Vec::new();

        loads.open(Some(Path::new("a.fcad")), relay(), |_, cancel| {
            first = Some(cancel.clone());
            let (worker, release) = held_worker(cancel);
            holds.push(release);
            worker
        });
        let first = first.expect("the first load was spawned");
        assert!(!first.is_cancelled());

        // The second request returns while the first reading is still going:
        // this line is reached with that worker alive, which is the property.
        loads.open(Some(Path::new("a.fcad")), relay(), |_, cancel| {
            let (worker, release) = held_worker(cancel);
            holds.push(release);
            worker
        });
        assert!(
            first.is_cancelled(),
            "the abandoned reading was left running"
        );
        assert_eq!(
            loads.running.len(),
            2,
            "an abandoned worker was dropped, which detaches its thread"
        );

        loads.stop_all();
        assert!(loads.running.is_empty(), "a worker was left unjoined");
    }

    /// Applies an answer the way the event loop does, minus the GPU.
    ///
    /// What `App::show` adds is the upload; what it decides is here. Returns
    /// what the event loop uses to decide whether to draw: a line that changed
    /// is a reason to draw a frame, and on the path where a load failed it is
    /// the only one.
    fn deliver(
        loads: &mut Loads,
        input: &mut ViewportInput,
        generation: LoadGeneration,
        result: Result<LoadedScene>,
    ) -> AnswerEffect {
        deliver_into(&mut empty_scene(), loads, input, generation, result)
    }

    /// The same, into a picture that is already on screen.
    ///
    /// What `App::show` does, minus the graphics device: prepare, commit and
    /// then say so, in that order. A picture is committed by the same
    /// statement the window uses, so a gate about what a failed Open leaves
    /// behind is a gate about the arrangement the window actually has.
    fn deliver_into(
        scene: &mut LiveScene<()>,
        loads: &mut Loads,
        input: &mut ViewportInput,
        generation: LoadGeneration,
        result: Result<LoadedScene>,
    ) -> AnswerEffect {
        // The same order the event loop uses: whether to show it, then
        // showing it, then saying so. Announcing first would let the window
        // call a document ready before the frame that put it there.
        let outcome = if loads.accepts(generation) {
            let next = prepare_load(input, result, |_| Ok(()));
            commit_scene(scene, input, next)
        } else {
            result.map(|_| ())
        };
        finish_answer(loads, input, generation, outcome)
    }

    /// A window with nothing in it yet.
    fn empty_scene() -> LiveScene<()> {
        LiveScene::new(
            (),
            Vec::new(),
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::default(),
            Vec::new(),
            Vec::new(),
        )
    }

    #[test]
    fn the_answer_to_the_older_request_never_reaches_the_screen() {
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut holds = Vec::new();
        let mut spawn = |cancel: &CancelToken| {
            let (worker, release) = held_worker(cancel);
            holds.push(release);
            worker
        };

        // Two documents opened in quick succession, and the first one answers
        // last: the slow reading is exactly the one a user gives up waiting
        // for, so this order is the usual one rather than the unlucky one.
        let a = loads
            .open(Some(Path::new("a.fcad")), relay(), |_, cancel| {
                spawn(cancel)
            })
            .expect("a load started");
        let b = loads
            .open(Some(Path::new("b.fcad")), relay(), |_, cancel| {
                spawn(cancel)
            })
            .expect("a load started");

        deliver(&mut loads, &mut input, b, Ok(loaded(scene_at(0.0))));
        let showing_b = *input.camera();
        deliver(&mut loads, &mut input, a, Ok(loaded(scene_at(5000.0))));

        assert_eq!(
            *input.camera(),
            showing_b,
            "the abandoned document replaced the one the user asked for"
        );

        loads.stop_all();
    }

    #[test]
    fn a_failure_from_an_abandoned_request_changes_nothing_either() {
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut holds = Vec::new();
        let mut spawn = |cancel: &CancelToken| {
            let (worker, release) = held_worker(cancel);
            holds.push(release);
            worker
        };

        let a = loads
            .open(Some(Path::new("a.fcad")), relay(), |_, cancel| {
                spawn(cancel)
            })
            .expect("a load started");
        let b = loads
            .open(Some(Path::new("b.fcad")), relay(), |_, cancel| {
                spawn(cancel)
            })
            .expect("a load started");

        deliver(&mut loads, &mut input, b, Ok(loaded(scene_at(0.0))));
        let showing_b = *input.camera();
        // B's own arrival owed a frame; take it, so what is measured below is
        // what the discarded answer asked for and not what B did.
        assert!(input.take_redraw());

        // A document nobody is waiting for failed. What is on screen is not
        // that document, so nothing about it changes.
        deliver(
            &mut loads,
            &mut input,
            a,
            Err(CadError::input("no such document")),
        );
        assert_eq!(*input.camera(), showing_b);
        assert!(
            !input.take_redraw(),
            "an answer that was discarded still asked for a frame"
        );

        loads.stop_all();
    }

    #[test]
    fn the_line_says_what_is_being_opened_and_then_that_it_is_open() {
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert_eq!(loads.status().line(), "No document");

        let a = loads
            .open(Some(Path::new("/models/bracket.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a document was named");

        // While it is being read the previous picture is still the picture,
        // and the line says which document the window is waiting for. The path
        // it was found at is not the name of it.
        assert_eq!(
            *loads.status(),
            Status::Loading {
                generation: a,
                file: "bracket.fcad".to_owned(),
                fraction: 0.0,
            }
        );
        assert_eq!(loads.status().line(), "Opening bracket.fcad… 0%");

        assert!(
            deliver(&mut loads, &mut input, a, Ok(loaded(scene_at(0.0)))).status_changed,
            "the line changed and the window was not asked to draw it"
        );
        assert_eq!(
            *loads.status(),
            Status::Ready {
                file: "bracket.fcad".to_owned()
            }
        );
        assert_eq!(loads.status().line(), "bracket.fcad");

        loads.stop_all();
    }

    #[test]
    fn a_document_that_could_not_be_read_says_why_and_keeps_the_picture() {
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        let a = loads
            .open(Some(Path::new("good.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a document was named");
        deliver(&mut loads, &mut input, a, Ok(loaded(scene_at(0.0))));
        let showing = *input.camera();

        let b = loads
            .open(Some(Path::new("broken.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a document was named");
        let effect = deliver(
            &mut loads,
            &mut input,
            b,
            Err(CadError::input("this is not a document")),
        );
        assert!(
            effect.status_changed,
            "a failure changed the line and asked for no frame to show it"
        );
        assert_eq!(
            effect.error.as_deref(),
            Some("invalid input: this is not a document"),
            "the current failure was hidden from the diagnostic stream"
        );

        // The failure is about the document that failed, and the model the
        // user was reading is still on screen underneath the message.
        assert_eq!(
            *loads.status(),
            Status::Failed {
                file: "broken.fcad".to_owned(),
                message: "invalid input: this is not a document".to_owned(),
            }
        );
        assert!(loads.status().line().contains("broken.fcad"));
        assert!(loads.status().line().contains("not a document"));
        assert_eq!(*input.camera(), showing, "the picture changed as well");

        loads.stop_all();
    }

    #[test]
    fn the_answer_to_the_older_request_changes_neither_picture_nor_line() {
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        let a = loads
            .open(Some(Path::new("a.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a document was named");
        let b = loads
            .open(Some(Path::new("b.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a document was named");

        // The reading of A finishes after B was asked for. Both halves of the
        // window must ignore it: the picture, and the sentence under it.
        let waiting_for_b = loads.status().clone();
        assert!(
            !deliver(&mut loads, &mut input, a, Ok(loaded(scene_at(5000.0)))).status_changed,
            "an abandoned answer asked for a frame"
        );
        assert_eq!(*loads.status(), waiting_for_b);
        assert_eq!(loads.status().line(), "Opening b.fcad… 0%");

        deliver(&mut loads, &mut input, b, Ok(loaded(scene_at(0.0))));
        let showing_b = *input.camera();
        assert_eq!(
            *loads.status(),
            Status::Ready {
                file: "b.fcad".to_owned()
            }
        );

        // And a failure that belonged to A, arriving even later, says nothing
        // about B – which is the document on screen.
        let stale = deliver(
            &mut loads,
            &mut input,
            a,
            Err(CadError::input("A was unreadable")),
        );
        assert!(
            !stale.status_changed,
            "an abandoned failure asked for a frame"
        );
        assert_eq!(
            stale.error, None,
            "an abandoned failure escaped to the diagnostic stream"
        );
        assert_eq!(
            *loads.status(),
            Status::Ready {
                file: "b.fcad".to_owned()
            },
            "an abandoned document's failure was reported as this one's"
        );
        assert_eq!(*input.camera(), showing_b);

        loads.stop_all();
    }

    #[test]
    fn a_reading_that_reports_often_wakes_the_loop_once() {
        let relay = ProgressRelay::default();

        // The first report is the one that has to wake anybody. Everything
        // said while that wake-up is still outstanding is caught up with when
        // the loop reads, which is what stops a long load from filling the
        // queue ahead of the user's next click.
        assert!(relay.record(0.1), "the first report woke nobody");
        for step in 2..=500 {
            assert!(
                !relay.record(f64::from(step) / 1000.0),
                "report {step} asked for a second wake-up"
            );
        }

        // And what the loop reads is the newest thing said, not the oldest.
        assert!(
            (relay.take() - 0.5).abs() < 1e-9,
            "the loop read stale news"
        );
        assert!(
            relay.record(0.6),
            "reading did not open the way for the next"
        );
        assert!((relay.take() - 0.6).abs() < 1e-9);
    }

    #[test]
    fn the_last_thing_a_load_said_is_never_lost() {
        let relay = ProgressRelay::default();
        relay.record(0.4);

        // A report that lands between the loop clearing the flag and reading
        // the value is read here; one that lands after is a fresh wake-up.
        // Neither order may leave the newest number unseen, which at the end
        // of a load is the difference between 99% and finished.
        relay.take();
        assert!(relay.record(1.0));
        assert!((relay.take() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn progress_moves_the_line_and_asks_for_a_frame() {
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        let a = loads
            .open(Some(Path::new("part.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a document was named");
        input.take_redraw();

        let relay = Arc::clone(loads.relay(a).expect("the load has somewhere to report"));
        relay.record(0.5);
        assert!(advance_load(&mut loads, &mut input, a));
        assert_eq!(loads.status().line(), "Opening part.fcad… 50%");
        assert!(
            input.take_redraw(),
            "the line changed and no frame followed"
        );

        // A difference nobody could see is not worth a frame.
        relay.record(0.502);
        assert!(!advance_load(&mut loads, &mut input, a));
        assert!(!input.take_redraw());

        loads.stop_all();
    }

    #[test]
    fn progress_from_an_abandoned_reading_is_ignored() {
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        let a = loads
            .open(Some(Path::new("a.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a document was named");
        let stale = Arc::clone(loads.relay(a).expect("the load has somewhere to report"));

        let b = loads
            .open(Some(Path::new("b.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a document was named");
        input.take_redraw();

        // A reading of A that is still going and still reporting. Its numbers
        // describe a document nobody is waiting for, and showing them under
        // B's name would be describing the wrong file.
        stale.record(0.9);
        assert!(!advance_load(&mut loads, &mut input, a));
        assert!(!loads.progressed(a, 0.9));
        assert_eq!(loads.status().line(), "Opening b.fcad… 0%");
        assert!(!input.take_redraw());

        assert!(loads.progressed(b, 0.25), "the current reading was ignored");
        assert_eq!(loads.status().line(), "Opening b.fcad… 25%");

        loads.stop_all();
    }

    #[test]
    fn giving_up_on_a_reading_keeps_what_is_on_screen() {
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        // A document on screen, then a second one asked for and given up on.
        let a = loads
            .open(Some(Path::new("first.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a document was named");
        deliver(&mut loads, &mut input, a, Ok(loaded(scene_at(0.0))));
        let showing = *input.camera();

        let b = loads
            .open(Some(Path::new("second.fcad")), relay(), |_, cancel| {
                let cancel = cancel.clone();
                std::thread::spawn(move || {
                    while !cancel.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                })
            })
            .expect("a document was named");
        input.take_redraw();

        assert!(cancel_load(&mut loads, &mut input));
        assert!(
            input.take_redraw(),
            "the line changed and no frame followed"
        );

        // The line names the model that is drawn, which is the first one: the
        // second was never on screen and saying anything about it would
        // describe something nobody can see.
        assert_eq!(
            *loads.status(),
            Status::Ready {
                file: "first.fcad".to_owned()
            }
        );
        assert_eq!(*input.camera(), showing);

        // And the reading that was given up on still answers eventually. By
        // then it is as stale as any abandoned load, and is discarded.
        assert!(
            !deliver(&mut loads, &mut input, b, Ok(loaded(scene_at(5000.0)))).status_changed,
            "an abandoned reading's answer changed the line"
        );
        assert_eq!(*input.camera(), showing, "an abandoned reading took over");

        // Nothing left to give up on, so asking again changes nothing.
        assert!(!cancel_load(&mut loads, &mut input));

        loads.stop_all();
    }

    #[test]
    fn giving_up_on_the_first_reading_leaves_an_empty_window() {
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();

        loads
            .open(Some(Path::new("only.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a document was named");

        // Nothing has ever been drawn, so there is nothing to go back to
        // describing. Claiming the abandoned document was ready would be the
        // one thing worse than saying nothing.
        assert!(cancel_load(&mut loads, &mut input));
        assert_eq!(*loads.status(), Status::Idle);
        assert_eq!(loads.status().line(), "No document");

        loads.stop_all();
    }

    #[test]
    fn clicking_the_background_chooses_nothing() {
        let snapshot = distant_scene();

        // Something in the picture is something.
        let something = snapshot
            .draws()
            .first()
            .expect("the picture draws something")
            .pick;
        assert_eq!(
            Selection::definition(something, &snapshot),
            Selection::Definition(something)
        );

        // The background is nothing, and that is how a person unchooses.
        assert_eq!(
            Selection::definition(PickId::NOTHING, &snapshot),
            Selection::Nothing,
            "clicking away from the model kept the old choice"
        );
    }

    #[test]
    fn a_pick_from_the_old_picture_cannot_choose_in_the_new_one() {
        let before = distant_scene();
        let after = scene_at(50.0);

        // The same number means a different definition in a different picture,
        // and nothing about the number itself says which one it came from.
        // Answering with the picture on screen is what stops a click made
        // before Open from landing on whatever now occupies that index.
        let chosen = before
            .draws()
            .first()
            .expect("the picture draws something")
            .pick;
        assert_eq!(
            Selection::definition(chosen, &before),
            Selection::Definition(chosen)
        );
        assert_eq!(
            Selection::definition(chosen, &after),
            Selection::Nothing,
            "a choice made in the previous document was applied to this one"
        );
    }

    #[test]
    fn replacing_even_an_identical_picture_clears_its_transient_choice() {
        let picture = distant_scene();
        let chosen = picture
            .draws()
            .first()
            .expect("the picture draws something")
            .pick;
        let old = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        assert_eq!(
            old.selection,
            Selection::Definition(chosen),
            "the gate began with no choice"
        );

        // A second document can produce byte-identical geometry and therefore
        // the same deterministic snapshot identity while its catalogue names
        // another object. Replacing all three pieces through the constructor
        // is what prevents the old raw definition number from retargeting.
        let replacement = LiveScene::new(
            (),
            vec![a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::default(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(replacement.selection, Selection::Nothing);
    }

    #[test]
    fn a_load_that_failed_leaves_the_picture_and_the_choice_alone() {
        let picture = distant_scene();
        let chosen = picture
            .draws()
            .first()
            .expect("the picture draws something")
            .pick;
        let mine = a_body();
        let mut scene = LiveScene {
            prepared: (),
            catalogue: vec![mine.clone()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            // A real question about the picture that is still on screen, so
            // "nothing changed" is a statement with content.
            hovered: Hovered::Definition(chosen),
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);
        let framing = *camera.camera();

        // A document that could not be read changes nothing that is on screen,
        // and that includes what the user had chosen in it. Going blank, or
        // quietly unchoosing, would both lose work while they read the message.
        let error = commit_scene(
            &mut scene,
            &mut camera,
            Err(CadError::input("this is not a document")),
        )
        .expect_err("a failed load must not commit a picture");
        assert!(error.to_string().contains("not a document"));
        assert_eq!(scene.selection, Selection::Definition(chosen));
        assert_eq!(
            scene.hovered,
            Hovered::Definition(chosen),
            "a failed load forgot what the pointer was over"
        );
        assert_eq!(scene.catalogue, vec![mine]);
        assert_eq!(*camera.camera(), framing);
    }

    #[test]
    fn a_load_that_arrived_replaces_the_picture_and_unchooses() {
        let picture = distant_scene();
        let chosen = picture
            .draws()
            .first()
            .expect("the picture draws something")
            .pick;
        let mut scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);

        let arriving = a_body();
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);

        commit_scene(
            &mut scene,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: vec![arriving.clone()],
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::default(),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");

        // All four together: the picture, what its parts are, the choice made
        // in the old one, and the camera that frames the new one.
        assert_eq!(scene.catalogue, vec![arriving]);
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(camera.camera().width(), 640);
    }

    #[test]
    fn what_is_chosen_is_resolved_through_this_picture_and_no_other() {
        let picture = distant_scene();
        let elsewhere = scene_at(50.0);
        let mine = a_body();
        let scene = LiveScene {
            prepared: (),
            catalogue: vec![mine.clone()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(
                picture
                    .draws()
                    .first()
                    .expect("the picture draws something")
                    .pick,
            ),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };

        // Two lookups and no search: this snapshot names the definition, this
        // catalogue says what it is. Nothing falls back to a name, because
        // names repeat.
        assert_eq!(scene.chosen(&picture), Some((0, &mine)));

        // The same raw number in another picture is another definition, and
        // this one declines to answer for it.
        assert_eq!(
            scene.chosen(&elsewhere),
            None,
            "a choice made in another picture was answered from this catalogue"
        );

        // A catalogue that does not run that far answers nothing rather than
        // whatever is at the end of it.
        let short = LiveScene {
            prepared: (),
            catalogue: Vec::new(),
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: scene.selection,
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        assert_eq!(short.chosen(&picture), None);
    }

    #[test]
    fn pointing_at_something_asks_about_it_and_chooses_nothing() {
        let picture = distant_scene();
        let other = scene_at(400.0);
        let chosen = picture
            .draws()
            .first()
            .expect("the picture draws something")
            .pick;
        let mut scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };

        // Pointing at the definition that is already chosen: a question about
        // what is under the pointer, and the choice is untouched by it.
        assert!(hover(
            &mut scene.hovered,
            &picture,
            Hovered::Definition(chosen)
        ));
        assert_eq!(scene.hovered, Hovered::Definition(chosen));
        assert_eq!(
            scene.selection,
            Selection::Definition(chosen),
            "pointing at something chose it"
        );

        // Asking the same thing again changes nothing, so nothing asks for a
        // frame that would draw the picture that is already on screen.
        assert!(
            !hover(&mut scene.hovered, &picture, Hovered::Definition(chosen)),
            "the same question was treated as news"
        );

        // Away from the model: the question is answered with nothing, and the
        // choice survives it.
        assert!(hover(&mut scene.hovered, &picture, Hovered::Nothing));
        assert_eq!(scene.hovered, Hovered::Nothing);
        assert_eq!(scene.selection, Selection::Definition(chosen));

        // A question about a picture that has been replaced marks nothing in
        // this one, however plausible its number looks.
        assert!(!hover(
            &mut scene.hovered,
            &other,
            Hovered::Definition(chosen)
        ));
        assert_eq!(scene.hovered, Hovered::Nothing);
    }

    #[test]
    fn the_interface_blocks_the_model_beneath_it_after_layout() {
        // This is the fact the completed egui pass reports. For an idle
        // CursorMoved, egui-winit's EventResponse::consumed is false even over
        // a toolbar, so using only that earlier answer would return Pixel and
        // perform an offscreen pick through the panel.
        assert_eq!(
            hover_request(None, true, Hover::At(40.0, 50.0)),
            HoverRequest::Clear
        );

        // A definition row is the one interface area that answers the
        // question itself, without consulting the pixel under the panel.
        assert_eq!(
            hover_request(Some(3), true, Hover::At(40.0, 50.0)),
            HoverRequest::Row(3)
        );

        // The same physical point over the viewport remains a pixel question.
        assert_eq!(
            hover_request(None, false, Hover::At(40.0, 50.0)),
            HoverRequest::Pixel(40.0, 50.0)
        );
    }

    #[test]
    fn pointing_at_a_row_and_pointing_at_its_geometry_are_one_answer() {
        let mut builder = SnapshotBuilder::new();
        let first = builder
            .add_mesh(&distant_scene_mesh())
            .expect("the mesh is valid");
        let second = builder
            .add_mesh(&distant_scene_mesh())
            .expect("the mesh is valid");
        builder
            .place(
                first,
                None,
                &ferritecad_types::Transform::IDENTITY,
                [0.5, 0.5, 0.5],
            )
            .expect("places it");
        builder
            .place(
                second,
                None,
                &ferritecad_types::Transform::IDENTITY,
                [0.5, 0.5, 0.5],
            )
            .expect("places it");
        let picture = builder.build();

        // What a row of the list says, and what a pixel of the model says,
        // are the same identity: the list asks the picture rather than asking
        // what is under a panel.
        let by_row = picture.pick_of(second).expect("the picture has that row");
        let by_pixel = picture
            .draws()
            .iter()
            .find(|draw| picture.definition(draw.pick) == Some(second))
            .expect("that definition is drawn")
            .pick;
        assert_eq!(by_row, by_pixel);

        // Moving from one to the other is a change of question and nothing
        // else: what is chosen, and what an inspector would describe, are
        // decided elsewhere and stay where they were.
        let entries = vec![a_body(), a_body()];
        let mut scene = LiveScene {
            prepared: (),
            catalogue: entries.clone(),
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        assert!(hover(
            &mut scene.hovered,
            &picture,
            Hovered::Definition(picture.pick_of(first).expect("a row"))
        ));
        assert!(hover(
            &mut scene.hovered,
            &picture,
            Hovered::Definition(by_row)
        ));
        assert_eq!(scene.hovered, Hovered::Definition(by_row));
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(
            scene.chosen(&picture),
            None,
            "a question filled the inspector"
        );
    }

    /// The committed plate, loaded exactly as the viewer loads a document.
    fn plate_scene() -> (tempfile::TempDir, LoadedScene) {
        use ferritecad_kernel::mock::MockKernel;

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");
        let scene = snapshot_of(
            &path,
            &mut MockKernel::new(),
            |_: &mut MockKernel, _: &[u8]| Err(CadError::unsupported("the plate holds no imports")),
            &ferritecad_kernel::TessellationParams::default(),
            &ferritecad_kernel::OperationContext::default(),
        )
        .expect("the committed plate loads");
        (directory, scene)
    }

    /// The committed plate through the kernel that ships, so its topological
    /// edges are named rather than absent.
    ///
    /// The mock kernel reports no edge association at all, which is the honest
    /// thing for it to say and useless for this gate: what is being measured
    /// is that a real edge, named by Open CASCADE, reaches a pixel.
    fn native_plate_scene() -> Option<(tempfile::TempDir, LoadedScene)> {
        if !ferritecad_occt::is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return None;
        }
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");
        let mut kernel = OcctKernel::new().expect("opens a session");
        let scene = snapshot_of(
            &path,
            &mut kernel,
            |kernel: &mut OcctKernel, bytes: &[u8]| kernel.import_step(bytes),
            &ferritecad_kernel::TessellationParams::default(),
            &ferritecad_kernel::OperationContext::default(),
        )
        .expect("the committed plate loads through Open CASCADE");
        Some((directory, scene))
    }

    /// A renderer, or a reason this machine cannot run a pixel gate.
    macro_rules! renderer_or_skip {
        () => {
            match Renderer::new() {
                Ok(renderer) => renderer,
                Err(reason) if reason.kind() == ferritecad_types::ErrorKind::Unsupported => {
                    eprintln!("skipped: {reason}");
                    return;
                }
                Err(reason) => panic!("a renderer failed after adapter discovery: {reason}"),
            }
        };
    }

    #[test]
    fn what_a_pixel_is_a_question_about_is_the_most_particular_thing_it_is() {
        let Some((_directory, scene)) = native_plate_scene() else {
            return;
        };
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        input
            .frame(picture.bounds().expect("somewhere"))
            .expect("frames");
        let frame = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &Visibility::new(&picture),
            )
            .expect("draws");

        // Every pixel of a real picture, sorted by what it actually is. All
        // four kinds occur here, and each must answer with the most particular
        // thing true of it rather than with the first thing looked for.
        let (mut on_corners, mut on_edges, mut on_faces, mut on_nothing) = (0u32, 0u32, 0u32, 0u32);
        for y in 0..frame.height() {
            for x in 0..frame.width() {
                let hit = frame.hit_at(x, y);
                let answer = hovered_at(hit);
                if hit.vertex() != ferritecad_viewport::VertexPickId::NOTHING {
                    assert_eq!(answer, Hovered::Vertex(hit.vertex()), "at {x},{y}");
                    on_corners += 1;
                } else if hit.edge() != EdgePickId::NOTHING {
                    assert_eq!(answer, Hovered::Edge(hit.edge()), "at {x},{y}");
                    on_edges += 1;
                } else if hit.face() != FacePickId::NOTHING {
                    assert_eq!(answer, Hovered::Face(hit.face()), "at {x},{y}");
                    on_faces += 1;
                } else if hit.definition() != PickId::NOTHING {
                    assert_eq!(answer, Hovered::Definition(hit.definition()), "at {x},{y}");
                } else {
                    assert_eq!(answer, Hovered::Nothing, "at {x},{y}");
                    on_nothing += 1;
                }
            }
        }
        assert!(
            on_corners > 0 && on_edges > 0 && on_faces > 0 && on_nothing > 0,
            "all four kinds of pixel occur: {on_corners} on corners, {on_edges} on \
             edges, {on_faces} on surfaces, {on_nothing} on nothing"
        );

        // A pixel that is on an edge is also on a face: the ordering is what
        // decides between them, not the absence of the other answer.
        let (x, y, _) = an_edge_pixel(&frame).expect("the plate draws its edges");
        assert_ne!(frame.hit_at(x, y).face(), FacePickId::NOTHING);
        assert_ne!(frame.hit_at(x, y).definition(), PickId::NOTHING);

        // And the same is true one step further in: a pixel on a corner is on
        // an edge and a face too, so the corner wins by precedence rather than
        // because nothing else was there.
        let on_a_corner = (0..frame.height())
            .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
            .find(|(x, y)| {
                frame.hit_at(*x, *y).vertex() != ferritecad_viewport::VertexPickId::NOTHING
            })
            .expect("the plate draws its corners");
        let hit = frame.hit_at(on_a_corner.0, on_a_corner.1);
        assert_ne!(hit.face(), FacePickId::NOTHING);
        assert_ne!(hit.definition(), PickId::NOTHING);
    }

    #[test]
    fn a_question_about_an_edge_leaves_the_choice_and_the_click_alone() {
        let Some((_directory, scene)) = native_plate_scene() else {
            return;
        };
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        input
            .frame(picture.bounds().expect("somewhere"))
            .expect("frames");
        let visibility = Visibility::new(&picture);

        let plain = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");
        let (x, y, edge) = an_edge_pixel(&plain).expect("the plate draws its edges");

        // A choice already made, and a question about an edge of it.
        let mut scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::new(&picture),
            selection: Selection::Nothing,
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let before = scene.selection.clone();
        assert!(hover(
            &mut scene.hovered,
            &picture,
            hovered_at(plain.hit_at(x, y))
        ));
        assert_eq!(scene.hovered, Hovered::Edge(edge));
        assert_eq!(
            scene.selection, before,
            "pointing at an edge changed what was chosen"
        );
        // Asking the same thing again is not a reason to draw again.
        assert!(!hover(
            &mut scene.hovered,
            &picture,
            hovered_at(plain.hit_at(x, y))
        ));

        // And a click on that very pixel finds what it found before: the mark
        // is drawn over the picture and changes no answer about it.
        let marked = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                scene.hovered,
                &visibility,
            )
            .expect("draws");
        assert_eq!(
            marked.pick_at(x, y),
            plain.pick_at(x, y),
            "the marked edge changed which definition the pixel is"
        );
        assert_eq!(
            marked.hit_at(x, y).face(),
            plain.hit_at(x, y).face(),
            "the marked edge changed which face the pixel is"
        );
        assert_ne!(
            plain.hit_at(x, y).face(),
            FacePickId::NOTHING,
            "a click on a line still lands on the face under it"
        );
    }

    #[test]
    fn a_row_answers_with_its_definition_and_never_with_an_edge() {
        let Some((_directory, scene)) = native_plate_scene() else {
            return;
        };
        let picture = scene.snapshot;
        assert!(picture.edge_count() > 0, "this picture has edges to offer");

        let mut hovered = Hovered::Nothing;
        // What a row asks, exactly as `point_at` asks it.
        let answer = picture.pick_of(0).map(Hovered::Definition).expect("a row");
        assert!(hover(&mut hovered, &picture, answer));
        assert!(
            matches!(hovered, Hovered::Definition(_)),
            "a list of definitions answered with something else: {hovered:?}"
        );
        // A row names a definition and can say nothing about a face or an
        // edge: there are none in a list of definitions.
        assert_eq!(hovered.definition(&picture), Some(0));
    }

    #[test]
    fn a_picture_whose_kernel_named_no_edges_still_answers_about_faces() {
        // The mock kernel reports no edge association at all, which is the
        // honest thing for it to say. Every pixel of the model must then fall
        // back to the face under it, and none may invent an edge.
        let (_directory, scene) = plate_scene();
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot);
        assert_eq!(
            picture.edge_count(),
            0,
            "the mock kernel names no topological edges"
        );
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(320, 320);
        input
            .frame(picture.bounds().expect("somewhere"))
            .expect("frames");
        let frame = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &Visibility::new(&picture),
            )
            .expect("draws");

        let mut on_faces = 0u32;
        for y in 0..frame.height() {
            for x in 0..frame.width() {
                let answer = hovered_at(frame.hit_at(x, y));
                assert!(
                    !matches!(answer, Hovered::Edge(_)),
                    "a picture with no edge association answered with one at {x},{y}"
                );
                if matches!(answer, Hovered::Face(_)) {
                    on_faces += 1;
                }
            }
        }
        assert!(on_faces > 0, "the model is drawn and its faces answer");
    }

    /// The committed plate through the real kernel, with an exact durable name
    /// written for every cap-boundary edge.
    fn native_plate_with_named_edges() -> Option<(tempfile::TempDir, LoadedScene)> {
        use ferritecad_document::{CapSide, Document, EntityKind, SelectionRule};

        if !ferritecad_occt::is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return None;
        }
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");

        let mut document = Document::open(&path).expect("opens the plate");
        let stored = document.topology_refs().expect("reads");
        let sides: Vec<(ferritecad_types::ObjectId, ferritecad_types::StableEntityId)> = stored
            .iter()
            .filter_map(|reference| match &reference.output_role {
                SemanticRole::ExtrudeSide { profile_segment } => {
                    Some((reference.producer_feature, *profile_segment))
                }
                _ => None,
            })
            .collect();
        assert!(!sides.is_empty(), "the fixture names its swept faces");
        let owner = stored[0].owner;
        document
            .write(|w| {
                for (producer, segment) in &sides {
                    for side in [CapSide::Start, CapSide::End] {
                        w.put_topology_ref(&ferritecad_document::TopologyRef {
                            id: ferritecad_types::StableEntityId::new(),
                            owner,
                            producer_feature: *producer,
                            expected_kind: EntityKind::Edge,
                            output_role: SemanticRole::ExtrudeCapEdge {
                                side,
                                profile_segment: *segment,
                            },
                            selection: SelectionRule::Exact,
                            fallback_signature: None,
                        })?;
                    }
                }
                Ok(())
            })
            .expect("stores the cap edge references");
        drop(document);

        let mut kernel = OcctKernel::new().expect("opens a session");
        let scene = snapshot_of(
            &path,
            &mut kernel,
            |kernel: &mut OcctKernel, bytes: &[u8]| kernel.import_step(bytes),
            &ferritecad_kernel::TessellationParams::default(),
            &ferritecad_kernel::OperationContext::default(),
        )
        .expect("the plate loads through Open CASCADE");
        Some((directory, scene))
    }

    /// The committed plate through the real kernel, with an exact durable name
    /// for every corner where a profile joint reaches a cap.
    ///
    /// The corners come from `ProfileLoop::joints`, the same adjacency the
    /// kernel and the topology map already share.
    fn native_plate_with_named_cap_vertices() -> Option<(tempfile::TempDir, LoadedScene)> {
        native_plate_with_named_cap_vertices_beside(0)
    }

    /// The same plate with `neighbours` further bodies beside it, so an
    /// operation that acts on one definition has another to leave alone.
    fn native_plate_with_named_cap_vertices_beside(
        neighbours: usize,
    ) -> Option<(tempfile::TempDir, LoadedScene)> {
        use ferritecad_document::{CapSide, Document, EntityKind, ObjectPayload, SelectionRule};

        if !ferritecad_occt::is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return None;
        }
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");
        if neighbours > 0 {
            add_extra_bodies(&path, neighbours);
        }

        let mut document = Document::open(&path).expect("opens the plate");
        let objects = document.objects().expect("reads objects");
        let sketch = objects
            .iter()
            .find_map(|object| match &object.payload {
                ObjectPayload::Sketch(sketch) => Some(sketch.clone()),
                _ => None,
            })
            .expect("the fixture has a sketch");
        let datum = objects
            .iter()
            .find_map(|object| match &object.payload {
                ObjectPayload::DatumPlane(datum) => Some(datum.clone()),
                _ => None,
            })
            .expect("the fixture has a datum plane");
        let plane = ferritecad_eval::plane_from_datum(&datum).expect("reads the plane");
        let profile =
            ferritecad_eval::profile_from_sketch(&sketch, ferritecad_types::ObjectId::new(), plane)
                .expect("builds a profile")
                .profile;

        let stored = document.topology_refs().expect("reads");
        let producer = stored
            .iter()
            .find_map(|reference| match &reference.output_role {
                SemanticRole::ExtrudeSide { .. } => Some(reference.producer_feature),
                _ => None,
            })
            .expect("the fixture names its swept faces");
        let owner = stored[0].owner;
        document
            .write(|w| {
                for joint in profile.outer().joints() {
                    for side in [CapSide::Start, CapSide::End] {
                        w.put_topology_ref(&ferritecad_document::TopologyRef {
                            id: ferritecad_types::StableEntityId::new(),
                            owner,
                            producer_feature: producer,
                            expected_kind: EntityKind::Vertex,
                            output_role: SemanticRole::ExtrudeCapVertex { side, joint },
                            selection: SelectionRule::Exact,
                            fallback_signature: None,
                        })?;
                    }
                }
                Ok(())
            })
            .expect("stores the cap vertex references");
        drop(document);

        let mut kernel = OcctKernel::new().expect("opens a session");
        let scene = snapshot_of(
            &path,
            &mut kernel,
            |kernel: &mut OcctKernel, bytes: &[u8]| kernel.import_step(bytes),
            &ferritecad_kernel::TessellationParams::default(),
            &ferritecad_kernel::OperationContext::default(),
        )
        .expect("the plate loads through Open CASCADE");
        Some((directory, scene))
    }

    #[test]
    fn what_a_load_hands_over_includes_the_names_of_its_corners() {
        let Some((_directory, loaded)) = native_plate_with_named_cap_vertices() else {
            return;
        };
        let picture = loaded.snapshot.clone();
        // Everything the document names, before the load is prepared.
        let before: Vec<(u32, usize)> = (0..picture.vertex_count())
            .filter_map(|ordinal| picture.vertex_of(0, ordinal))
            .map(|vertex| (vertex.to_raw(), loaded.vertices.of(vertex, &picture).len()))
            .collect();
        assert_eq!(
            before.iter().filter(|(_, count)| *count > 0).count(),
            8,
            "the document names all eight corners of the plate"
        );

        let mut input = ViewportInput::new();
        input.resize(800, 600);
        // The route a real Open takes: everything fallible first, and the
        // parts handed over together afterwards.
        let vertices = prepare_load(&input, Ok(loaded), |_| Ok(()))
            .expect("the load is prepared")
            .vertices;

        let after: Vec<(u32, usize)> = (0..picture.vertex_count())
            .filter_map(|ordinal| picture.vertex_of(0, ordinal))
            .map(|vertex| (vertex.to_raw(), vertices.of(vertex, &picture).len()))
            .collect();
        assert_eq!(
            after, before,
            "preparing a load dropped what the document calls its corners"
        );
    }

    /// A copy of the committed plate with an exact name for every edge that
    /// runs along the sweep.
    ///
    /// The corners come from `ProfileLoop::joints`, which is the one piece of
    /// adjacency arithmetic in the workspace. Pairing the sketch curves here
    /// would be a second one, free to disagree with the first.
    fn native_plate_with_named_sweep_edges() -> Option<(tempfile::TempDir, LoadedScene)> {
        use ferritecad_document::{Document, EntityKind, ObjectPayload, SelectionRule};

        if !ferritecad_occt::is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return None;
        }
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");

        let mut document = Document::open(&path).expect("opens the plate");
        let objects = document.objects().expect("reads objects");
        let sketch = objects
            .iter()
            .find_map(|object| match &object.payload {
                ObjectPayload::Sketch(sketch) => Some(sketch.clone()),
                _ => None,
            })
            .expect("the fixture has a sketch");
        let datum = objects
            .iter()
            .find_map(|object| match &object.payload {
                ObjectPayload::DatumPlane(datum) => Some(datum.clone()),
                _ => None,
            })
            .expect("the fixture has a datum plane");
        let plane = ferritecad_eval::plane_from_datum(&datum).expect("reads the plane");
        let profile =
            ferritecad_eval::profile_from_sketch(&sketch, ferritecad_types::ObjectId::new(), plane)
                .expect("builds")
                .profile;

        let stored = document.topology_refs().expect("reads");
        let producer = stored
            .iter()
            .find_map(|reference| match &reference.output_role {
                SemanticRole::ExtrudeSide { .. } => Some(reference.producer_feature),
                _ => None,
            })
            .expect("the fixture names its swept faces");
        let owner = stored[0].owner;

        let joints: Vec<ferritecad_types::ProfileJoint> = profile.outer().joints().collect();
        assert_eq!(joints.len(), 4, "the plate has four corners");
        document
            .write(|w| {
                for joint in &joints {
                    w.put_topology_ref(&ferritecad_document::TopologyRef {
                        id: ferritecad_types::StableEntityId::new(),
                        owner,
                        producer_feature: producer,
                        expected_kind: EntityKind::Edge,
                        output_role: SemanticRole::ExtrudeSweepEdge { joint: *joint },
                        selection: SelectionRule::Exact,
                        fallback_signature: None,
                    })?;
                }
                Ok(())
            })
            .expect("stores the sweep edge references");
        drop(document);

        let mut kernel = OcctKernel::new().expect("opens a session");
        let scene = snapshot_of(
            &path,
            &mut kernel,
            |kernel: &mut OcctKernel, bytes: &[u8]| kernel.import_step(bytes),
            &ferritecad_kernel::TessellationParams::default(),
            &ferritecad_kernel::OperationContext::default(),
        )
        .expect("the plate loads through Open CASCADE");
        Some((directory, scene))
    }

    /// Every stored sweep-edge meaning of one edge, as the inspector says it.
    fn sweep_meanings(selected: &ferritecad_scene::SelectedEdge) -> Vec<String> {
        selected
            .meanings()
            .iter()
            .filter(|meaning| matches!(meaning.output_role, SemanticRole::ExtrudeSweepEdge { .. }))
            .map(|meaning| describe_role(&meaning.output_role))
            .collect()
    }

    /// One edge together with a face it actually bounds.
    ///
    /// Neither identity is inferred from the other's ordinal. The picture's
    /// own partition is the relationship a click on their shared pixel has to
    /// satisfy, so a fixture reordered during packing cannot quietly turn an
    /// edge gate into a test of an unrelated face.
    fn edge_with_bounding_face(
        picture: &RenderSnapshot,
        edge: EdgePickId,
    ) -> (PickId, ferritecad_viewport::FacePickId, EdgePickId) {
        let definition_index = picture
            .definition_of_edge(edge)
            .expect("the picture issued the edge");
        let definition = picture.pick_of(definition_index).expect("drawn");
        let face = (0..picture.face_count())
            .filter_map(|ordinal| picture.face_of(definition_index, ordinal))
            .find(|face| picture.edge_bounds_face(edge, *face))
            .expect("a drawn edge bounds a face of its definition");
        (definition, face, edge)
    }

    /// One named sweep edge together with a face it actually bounds.
    fn named_sweep_edge(
        scene: &LoadedScene,
    ) -> (PickId, ferritecad_viewport::FacePickId, EdgePickId) {
        let picture = &scene.snapshot;
        let edge = (0..picture.meshes().len())
            .find_map(|definition| {
                (0..picture.edge_count())
                    .filter_map(|ordinal| picture.edge_of(definition, ordinal))
                    .find(|edge| {
                        scene.edges.of(*edge, picture).iter().any(|meaning| {
                            matches!(meaning.output_role, SemanticRole::ExtrudeSweepEdge { .. })
                        })
                    })
            })
            .expect("the document names an edge along the sweep");
        edge_with_bounding_face(picture, edge)
    }

    /// One named cap edge together with a face it actually bounds.
    fn named_cap_edge(
        scene: &LoadedScene,
    ) -> (PickId, ferritecad_viewport::FacePickId, EdgePickId) {
        let picture = &scene.snapshot;
        let edge = (0..picture.meshes().len())
            .find_map(|definition| {
                (0..picture.edge_count())
                    .filter_map(|ordinal| picture.edge_of(definition, ordinal))
                    .find(|edge| {
                        scene.edges.of(*edge, picture).iter().any(|meaning| {
                            matches!(meaning.output_role, SemanticRole::ExtrudeCapEdge { .. })
                        })
                    })
            })
            .expect("the document names an edge at a cap");
        edge_with_bounding_face(picture, edge)
    }

    #[test]
    fn clicking_an_edge_along_the_sweep_chooses_it_and_says_what_it_is() {
        let Some((_directory, scene)) = native_plate_with_named_sweep_edges() else {
            return;
        };
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        input
            .frame(picture.bounds().expect("somewhere"))
            .expect("frames");
        // Off the axis, for the reason the cap-edge gate turns: which edges
        // have a coherent pixel depends on the view, and a line on the outer
        // silhouette is refused by `Hit` because the edge target and the
        // definition target disagree there.
        input.handle(ViewportEvent::PointerMoved { x: 200.0, y: 200.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::PointerMoved { x: 260.0, y: 150.0 }, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        let visibility = Visibility::new(&picture);

        let plain = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");

        // A pixel that is on an edge the document names as a sweep edge.
        let (x, y, edge) = (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let hit = plain.hit_at(x, y);
                let edge = hit.edge();
                if edge == EdgePickId::NOTHING {
                    return None;
                }
                scene
                    .edges
                    .of(edge, &picture)
                    .iter()
                    .any(|meaning| {
                        matches!(meaning.output_role, SemanticRole::ExtrudeSweepEdge { .. })
                    })
                    .then_some((x, y, edge))
            })
            .expect("the plate draws an edge the document names along the sweep");

        // The click chooses that edge, not the face under it and not the part.
        let chosen = selection_at(
            plain.hit_at(x, y),
            &picture,
            &scene.faces,
            &scene.edges,
            &scene.vertices,
        );
        let Selection::Edge(selected) = &chosen else {
            panic!("clicking an edge along the sweep chose {chosen:?} instead of the edge");
        };
        assert_eq!(selected.edge(), edge);
        assert_eq!(chosen.marked(), Marked::Edge(edge));

        // The next frame marks it and nothing else, and leaves what the
        // picture answers about identity alone.
        let marked = renderer
            .render(
                &prepared,
                input.camera(),
                chosen.marked(),
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");
        assert_ne!(
            marked.colour_at(x, y),
            plain.colour_at(x, y),
            "the chosen edge was not drawn as chosen"
        );
        let mut changed = 0usize;
        for probe_y in 0..plain.height() {
            for probe_x in 0..plain.width() {
                if plain.colour_at(probe_x, probe_y) != marked.colour_at(probe_x, probe_y) {
                    changed += 1;
                    let belongs_to_edge = (-1i64..=1).any(|dy| {
                        (-1i64..=1).any(|dx| {
                            let neighbour_x = i64::from(probe_x) + dx;
                            let neighbour_y = i64::from(probe_y) + dy;
                            neighbour_x >= 0
                                && neighbour_y >= 0
                                && plain.edge_at(neighbour_x as u32, neighbour_y as u32) == edge
                        })
                    });
                    assert!(
                        belongs_to_edge,
                        "the chosen edge changed an unrelated pixel at {probe_x},{probe_y}"
                    );
                }
                assert_eq!(
                    plain.pick_at(probe_x, probe_y),
                    marked.pick_at(probe_x, probe_y),
                    "at {probe_x},{probe_y}"
                );
                assert_eq!(
                    plain.hit_at(probe_x, probe_y).edge(),
                    marked.hit_at(probe_x, probe_y).edge(),
                    "at {probe_x},{probe_y}"
                );
            }
        }
        assert!(changed > 0, "choosing the edge changed no pixels");

        // Asking about an edge that is already chosen cannot replace the
        // decision with the hover style anywhere in the frame.
        let selected_and_asked = renderer
            .render(
                &prepared,
                input.camera(),
                chosen.marked(),
                Hovered::Edge(edge),
                &visibility,
            )
            .expect("draws");
        for probe_y in 0..plain.height() {
            for probe_x in 0..plain.width() {
                assert_eq!(
                    selected_and_asked.colour_at(probe_x, probe_y),
                    marked.colour_at(probe_x, probe_y),
                    "hover over the chosen edge won at {probe_x},{probe_y}"
                );
            }
        }

        // And the inspector says what it is in the document's own terms.
        let said = sweep_meanings(selected);
        assert!(
            !said.is_empty(),
            "the inspector says nothing about the chosen sweep edge"
        );
        for sentence in &said {
            assert!(
                sentence.starts_with("Sweep edge at the joint of profile segments "),
                "the inspector does not describe a sweep edge: {sentence}"
            );
            for forbidden in [
                "ExtrudeSweepEdge",
                "ProfileJoint",
                "EdgePickId",
                "{",
                "}",
                "joint:",
                "raw",
            ] {
                assert!(
                    !sentence.contains(forbidden),
                    "the inspector printed {forbidden}: {sentence}"
                );
            }
        }
    }

    #[test]
    fn pointing_at_a_corner_of_the_committed_plate_asks_about_that_corner() {
        // The whole route: the committed plate, the real loader and Open
        // CASCADE's own vertex association, a picture, a real frame, an orbit,
        // a coherent corner sample, the application's answer, a second real
        // frame, and its pixels.
        let Some((_directory, scene)) = native_plate_with_named_edges() else {
            return;
        };
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        input
            .frame(picture.bounds().expect("somewhere"))
            .expect("frames");
        input.handle(ViewportEvent::PointerMoved { x: 200.0, y: 200.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::PointerMoved { x: 260.0, y: 150.0 }, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        let visibility = Visibility::new(&picture);
        let plain = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");

        // A pixel where the hit itself is coherent about a corner. The raw
        // aperture is deliberately not used here: it reaches past the surface
        // and only the hit checks that everything agrees.
        let (x, y, corner) = (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let vertex = plain.hit_at(x, y).vertex();
                (vertex != ferritecad_viewport::VertexPickId::NOTHING).then_some((x, y, vertex))
            })
            .expect("the plate draws a corner the picture is coherent about");

        // The application must ask about that corner rather than the edge or
        // face beneath it.
        let answer = hovered_at(plain.hit_at(x, y));
        assert_eq!(
            answer,
            Hovered::Vertex(corner),
            "pointing at a corner asked {answer:?} instead"
        );

        // And the next frame must show it, differently from the plain picture.
        let marked = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                answer,
                &visibility,
            )
            .expect("draws");
        let changed = (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .filter(|(x, y)| plain.colour_at(*x, *y) != marked.colour_at(*x, *y))
            .count();
        assert!(
            changed > 0,
            "asking about a corner changed no pixel of the picture"
        );
    }

    #[test]
    fn a_pixel_of_a_corner_of_the_committed_plate_says_which_corner_it_is() {
        // The whole route: the committed plate, the real loader and Open
        // CASCADE's own vertex association, a picture, prepared geometry, a
        // real offscreen frame, and a pixel at a corner's own projection.
        let Some((_directory, scene)) = native_plate_with_named_edges() else {
            return;
        };
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot);
        assert_eq!(
            picture.vertex_count(),
            8,
            "the plate's corners reach the picture"
        );
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        input
            .frame(picture.bounds().expect("somewhere"))
            .expect("frames");
        // Off the axis, so the corners are not all on the silhouette.
        input.handle(ViewportEvent::PointerMoved { x: 200.0, y: 200.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::PointerMoved { x: 260.0, y: 150.0 }, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        let visibility = Visibility::new(&picture);
        let frame = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");

        // Somewhere in the picture a corner must be answerable.
        let found = (0..frame.height())
            .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let corner = frame.vertex_at(x, y);
                (corner != ferritecad_viewport::VertexPickId::NOTHING).then_some((x, y, corner))
            });
        let Some((x, y, corner)) = found else {
            panic!("no pixel of the drawn plate says which corner it is on");
        };
        assert_eq!(
            picture.definition_of_vertex(corner),
            picture.definition(picture.pick_of(0).expect("drawn")),
            "the corner at {x},{y} belongs to another definition"
        );
    }

    /// A pixel of the committed plate that the frame is coherent about, whose
    /// corner the document names exactly.
    ///
    /// Coherence comes from `Hit`, never from the raw vertex target: that one
    /// reaches past the surface on purpose. The name comes from the picture's
    /// own `VertexNames`, so a corner nothing names is not offered here.
    fn named_corner_pixel(
        frame: &ferritecad_viewport_gpu::Frame,
        scene: &LoadedScene,
        picture: &RenderSnapshot,
    ) -> Option<(u32, u32, ferritecad_viewport::VertexPickId)> {
        (0..frame.height())
            .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let corner = frame.hit_at(x, y).vertex();
                (corner != ferritecad_viewport::VertexPickId::NOTHING
                    && !scene.vertices.of(corner, picture).is_empty())
                .then_some((x, y, corner))
            })
    }

    #[test]
    fn clicking_a_named_corner_of_the_committed_plate_chooses_that_corner() {
        // The whole route: the committed plate copied into a document this
        // test owns, eight exact `ExtrudeCapVertex` references written into
        // it, the real Open CASCADE loader, a `RenderSnapshot`, an orbit to a
        // usable view, a coherent pixel of a named corner, the hit, the
        // application's own selection decision, a second real frame, and the
        // inspector.
        let Some((_directory, scene)) = native_plate_with_named_cap_vertices() else {
            return;
        };
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot.clone());
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        input
            .frame(picture.bounds().expect("somewhere"))
            .expect("frames");
        // Off the axis, so the corners are not all on the silhouette.
        input.handle(ViewportEvent::PointerMoved { x: 200.0, y: 200.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::PointerMoved { x: 260.0, y: 150.0 }, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        let visibility = Visibility::new(&picture);
        let plain = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");

        let (x, y, corner) = named_corner_pixel(&plain, &scene, &picture).expect(
            "the plate draws a corner the picture is coherent about and the document names",
        );

        // The click must choose that corner, not the edge, face or part
        // beneath it.
        let chosen = selection_at(
            plain.hit_at(x, y),
            &picture,
            &scene.faces,
            &scene.edges,
            &scene.vertices,
        );
        let ferritecad_scene::Selection::Vertex(selected) = &chosen else {
            panic!("clicking a named corner at {x},{y} chose {chosen:?}");
        };
        assert_eq!(selected.vertex(), corner);
        assert_eq!(selected.definition(), plain.hit_at(x, y).definition());
        assert_eq!(selected.meanings(), scene.vertices.of(corner, &picture));
        assert_eq!(
            chosen.marked(),
            ferritecad_viewport::Marked::Vertex(corner),
            "what the renderer is told to mark is not the chosen corner"
        );

        // And the second frame must show it, differently from the plain
        // picture.
        let marked = renderer
            .render(
                &prepared,
                input.camera(),
                chosen.marked(),
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");
        let changed = (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .filter(|(x, y)| plain.colour_at(*x, *y) != marked.colour_at(*x, *y))
            .count();
        assert!(changed > 0, "choosing a corner changed no pixel");

        // And the inspector must describe it as a corner, in the document's
        // own words.
        let identities = identities_of(&scene.catalogue);
        let words = words_of(&chosen, &scene.catalogue, &picture);
        let vertices: Vec<ferritecad_ui::TopologyName<'_>> =
            words.vertices.iter().map(topology_name).collect();
        let described = inspected(
            &chosen,
            &scene.catalogue,
            &identities,
            &[],
            &[],
            &vertices,
            &picture,
        )
        .expect("the inspector describes what is chosen");
        let rows = described.rows();
        assert_eq!(
            rows.iter()
                .find(|(label, _)| *label == "Kind")
                .map(|(_, value)| value.as_str()),
            Some("Vertex"),
            "the inspector calls a chosen corner something else: {rows:?}"
        );
        assert!(
            rows.iter().any(|(label, value)| *label == "Role"
                && (value.starts_with("Start cap vertex at the joint of profile segments ")
                    || value.starts_with("End cap vertex at the joint of profile segments "))),
            "the inspector does not say what the corner is: {rows:?}"
        );
    }

    #[test]
    fn clicking_a_named_edge_of_the_committed_plate_chooses_that_edge() {
        let Some((_directory, scene)) = native_plate_with_named_edges() else {
            return;
        };
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        input
            .frame(picture.bounds().expect("somewhere"))
            .expect("frames");
        // Turned off the axis so a cap edge lies over the body rather than on
        // its silhouette. Which edges have a coherent pixel depends on the
        // view: a line on the outer silhouette is refused by `Hit`, because
        // the edge target and the definition target disagree there.
        input.handle(ViewportEvent::PointerMoved { x: 200.0, y: 200.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::PointerMoved { x: 260.0, y: 150.0 }, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        let visibility = Visibility::new(&picture);

        let plain = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");

        // A pixel that is on a topological edge the document names.
        let (x, y, edge) = (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let hit = plain.hit_at(x, y);
                let edge = hit.edge();
                (edge != EdgePickId::NOTHING && !scene.edges.of(edge, &picture).is_empty())
                    .then_some((x, y, edge))
            })
            .expect("the plate draws an edge the document names");

        // The click must choose that edge, and not the face beneath it.
        let chosen = selection_at(
            plain.hit_at(x, y),
            &picture,
            &scene.faces,
            &scene.edges,
            &scene.vertices,
        );
        let Selection::Edge(selected) = &chosen else {
            panic!("clicking a named edge chose {chosen:?} instead of the edge");
        };
        assert_eq!(selected.edge(), edge);
        assert_eq!(chosen.marked(), Marked::Edge(edge));
        assert!(!selected.meanings().is_empty(), "and it carries its names");

        // And the next frame must show it, differently from the plain picture.
        let marked = renderer
            .render(
                &prepared,
                input.camera(),
                chosen.marked(),
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");
        assert_ne!(
            marked.colour_at(x, y),
            plain.colour_at(x, y),
            "the chosen edge was not drawn as chosen"
        );

        // The inspector describes it in the document's own words and in no
        // others.
        let words = words_of(&chosen, &scene.catalogue, &picture);
        assert!(
            !words.faces.is_empty() || !words.edges.is_empty(),
            "the inspector says nothing about the chosen edge"
        );
    }

    /// Every stored cap-edge meaning of one edge, as the inspector says it.
    fn cap_meanings(selected: &ferritecad_scene::SelectedEdge) -> Vec<String> {
        selected
            .meanings()
            .iter()
            .filter(|meaning| matches!(meaning.output_role, SemanticRole::ExtrudeCapEdge { .. }))
            .map(|meaning| describe_role(&meaning.output_role))
            .collect()
    }

    #[test]
    fn clicking_a_cap_edge_says_which_cap_and_which_segment_it_is() {
        let Some((_directory, scene)) = native_plate_with_named_edges() else {
            return;
        };
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        input
            .frame(picture.bounds().expect("somewhere"))
            .expect("frames");
        // Off the axis, so a cap edge lies over the body rather than on its
        // silhouette, where `Hit` refuses it because the edge target and the
        // definition target disagree.
        input.handle(ViewportEvent::PointerMoved { x: 200.0, y: 200.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(ViewportEvent::PointerMoved { x: 260.0, y: 150.0 }, false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        let visibility = Visibility::new(&picture);

        let plain = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");

        // A pixel on an edge the document names as a cap edge.
        let (x, y, edge) = (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let hit = plain.hit_at(x, y);
                let edge = hit.edge();
                if edge == EdgePickId::NOTHING {
                    return None;
                }
                scene
                    .edges
                    .of(edge, &picture)
                    .iter()
                    .any(|meaning| {
                        matches!(meaning.output_role, SemanticRole::ExtrudeCapEdge { .. })
                    })
                    .then_some((x, y, edge))
            })
            .expect("the plate draws an edge the document names at a cap");

        // The click still chooses that edge, not the face under it.
        let chosen = selection_at(
            plain.hit_at(x, y),
            &picture,
            &scene.faces,
            &scene.edges,
            &scene.vertices,
        );
        let Selection::Edge(selected) = &chosen else {
            panic!("clicking a named cap edge chose {chosen:?} instead of the edge");
        };
        assert_eq!(selected.edge(), edge);
        assert_eq!(chosen.marked(), Marked::Edge(edge));

        // The next frame marks that edge and nothing else. Checked over every
        // pixel, not only the one clicked, and with the one-pixel reach the
        // identity target is already documented to have: where two edges meet
        // at a shared vertex or cross in projection, a sample reports whichever
        // was drawn last, so a marked pixel may sit one across from the sample
        // that names it.
        let marked = renderer
            .render(
                &prepared,
                input.camera(),
                chosen.marked(),
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");
        assert_ne!(
            marked.colour_at(x, y),
            plain.colour_at(x, y),
            "the chosen edge was not drawn as chosen"
        );

        let mut changed = 0;
        for probe_y in 0..plain.height() {
            for probe_x in 0..plain.width() {
                // Nothing the picture answers about identity moved.
                assert_eq!(
                    plain.pick_at(probe_x, probe_y),
                    marked.pick_at(probe_x, probe_y),
                    "at {probe_x},{probe_y}"
                );
                assert_eq!(
                    plain.hit_at(probe_x, probe_y).face(),
                    marked.hit_at(probe_x, probe_y).face(),
                    "at {probe_x},{probe_y}"
                );
                assert_eq!(
                    plain.hit_at(probe_x, probe_y).edge(),
                    marked.hit_at(probe_x, probe_y).edge(),
                    "at {probe_x},{probe_y}"
                );

                if plain.colour_at(probe_x, probe_y) == marked.colour_at(probe_x, probe_y) {
                    continue;
                }
                changed += 1;
                let near = (probe_x.saturating_sub(1)..=probe_x + 1)
                    .flat_map(|nx| {
                        (probe_y.saturating_sub(1)..=probe_y + 1).map(move |ny| (nx, ny))
                    })
                    .any(|(nx, ny)| {
                        nx < plain.width()
                            && ny < plain.height()
                            && plain.hit_at(nx, ny).edge() == edge
                    });
                assert!(
                    near,
                    "the pixel at {probe_x},{probe_y} changed and is not on the chosen edge"
                );
            }
        }
        assert!(changed > 0, "the chosen edge was drawn no differently");

        // The inspector says which cap and which segment, in the document's
        // own words. Go through the whole handoff that supplies the panel: a
        // direct call to `describe_role` would not catch a dropped name, a
        // reordered name, or an edge presented as some other kind.
        let words = words_of(&chosen, &scene.catalogue, &picture);
        assert!(words.faces.is_empty(), "an edge was described as a face");
        let names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.edges.iter().map(topology_name).collect();
        let described = inspected(
            &chosen,
            &scene.catalogue,
            &words.identities,
            &[],
            &names,
            &[],
            &picture,
        )
        .expect("the chosen cap edge reaches the inspector");
        let rows = described.rows();
        assert_eq!(
            rows.first().map(|(label, value)| (*label, value.as_str())),
            Some(("Kind", "Edge"))
        );
        let said: Vec<&str> = rows
            .iter()
            .filter(|(label, _)| *label == "Role")
            .map(|(_, value)| value.as_str())
            .collect();
        let expected = cap_meanings(selected);
        assert_eq!(
            said,
            expected.iter().map(String::as_str).collect::<Vec<_>>(),
            "the inspector dropped or reordered the chosen edge's names"
        );
        assert!(
            !said.is_empty(),
            "the inspector says nothing about the chosen cap edge"
        );
        for sentence in &said {
            assert!(
                sentence.starts_with("Start cap edge of profile segment ")
                    || sentence.starts_with("End cap edge of profile segment "),
                "the inspector does not describe a cap edge: {sentence}"
            );
            for forbidden in [
                "ExtrudeCapEdge",
                "CapSide",
                "StableEntityId(",
                "EdgePickId",
                "{",
                "}",
                "side:",
                "profile_segment:",
            ] {
                assert!(
                    !sentence.contains(forbidden),
                    "the inspector printed {forbidden}: {sentence}"
                );
            }
        }
        for (_, value) in &rows {
            for forbidden in [
                "EdgePickId",
                "FacePickId",
                "SubShapeHandle",
                "ShapeHandle",
                "SessionId",
                "session#",
                "shape#",
                "edge#",
                "face#",
                ".fcad",
            ] {
                assert!(
                    !value.contains(forbidden),
                    "the inspector printed {forbidden}: {value}"
                );
            }
        }
    }

    #[test]
    fn the_two_caps_of_one_segment_are_said_differently_and_exactly() {
        let Some((_directory, scene)) = native_plate_with_named_edges() else {
            return;
        };
        let picture = scene.snapshot;

        // Every stored cap-edge name of the plate, by side and segment.
        let mut by_side: std::collections::BTreeMap<
            (bool, ferritecad_types::StableEntityId),
            String,
        > = std::collections::BTreeMap::new();
        for ordinal in 0..picture.edge_count() {
            let edge = picture.edge_of(0, ordinal).expect("numbered");
            for meaning in scene.edges.of(edge, &picture) {
                if let SemanticRole::ExtrudeCapEdge {
                    side,
                    profile_segment,
                } = &meaning.output_role
                {
                    let starts = matches!(side, CapSide::Start);
                    by_side.insert(
                        (starts, *profile_segment),
                        describe_role(&meaning.output_role),
                    );
                }
            }
        }
        assert_eq!(by_side.len(), 8, "four segments, two caps");

        let segments: std::collections::BTreeSet<ferritecad_types::StableEntityId> =
            by_side.keys().map(|(_, segment)| *segment).collect();
        assert_eq!(segments.len(), 4);

        for segment in &segments {
            let start = by_side
                .get(&(true, *segment))
                .expect("the start cap of this segment is named");
            let end = by_side
                .get(&(false, *segment))
                .expect("the end cap of this segment is named");

            // Exactly these sentences, and the two ends are not the same one.
            assert_eq!(
                start,
                &format!("Start cap edge of profile segment {segment}")
            );
            assert_eq!(end, &format!("End cap edge of profile segment {segment}"));
            assert_ne!(start, end, "both ends of one segment read alike");

            // The identifier is present whole, and only in its own sentence.
            let text = segment.to_string();
            assert!(start.contains(&text) && end.contains(&text));
            for (other, sentence) in &by_side {
                if other.1 != *segment {
                    assert!(
                        !sentence.contains(&text),
                        "segment {segment} appears in another segment's sentence: {sentence}"
                    );
                }
            }
        }

        // And the wording settled in the previous slice is untouched.
        let joint = ferritecad_types::ProfileJoint::new(
            *segments.iter().next().expect("four segments"),
            *segments.iter().nth(1).expect("four segments"),
        )
        .expect("two different segments");
        let [one, other] = joint.segments();
        assert_eq!(
            describe_role(&SemanticRole::ExtrudeSweepEdge { joint }),
            format!("Sweep edge at the joint of profile segments {one} and {other}")
        );
    }

    #[test]
    fn several_stored_cap_edge_names_of_one_edge_are_said_one_each_in_order() {
        use ferritecad_document::{Document, EntityKind, SelectionRule};

        if !ferritecad_occt::is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return;
        }
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");

        // One cap edge named three times over, which a document may hold: two
        // objects can both name the same edge.
        let mut document = Document::open(&path).expect("opens the plate");
        let stored = document.topology_refs().expect("reads");
        let (producer, segment) = stored
            .iter()
            .find_map(|reference| match &reference.output_role {
                SemanticRole::ExtrudeSide { profile_segment } => {
                    Some((reference.producer_feature, *profile_segment))
                }
                _ => None,
            })
            .expect("the fixture names its swept faces");
        let owner = stored[0].owner;
        let mut written = Vec::new();
        for _ in 0..3 {
            written.push(ferritecad_document::TopologyRef {
                id: ferritecad_types::StableEntityId::new(),
                owner,
                producer_feature: producer,
                expected_kind: EntityKind::Edge,
                output_role: SemanticRole::ExtrudeCapEdge {
                    side: CapSide::Start,
                    profile_segment: segment,
                },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            });
        }
        document
            .write(|w| {
                for reference in &written {
                    w.put_topology_ref(reference)?;
                }
                Ok(())
            })
            .expect("stores the cap edge references");
        drop(document);

        let mut kernel = OcctKernel::new().expect("opens a session");
        let scene = snapshot_of(
            &path,
            &mut kernel,
            |kernel: &mut OcctKernel, bytes: &[u8]| kernel.import_step(bytes),
            &ferritecad_kernel::TessellationParams::default(),
            &ferritecad_kernel::OperationContext::default(),
        )
        .expect("the plate loads through Open CASCADE");
        let picture = scene.snapshot;

        let edge = (0..picture.edge_count())
            .filter_map(|ordinal| picture.edge_of(0, ordinal))
            .find(|edge| scene.edges.of(*edge, &picture).len() == 3)
            .expect("one edge carries the three stored names");

        // Select through a face the edge actually bounds, and hand the result
        // through the same conversion the live inspector uses.
        let (definition, face, edge) = edge_with_bounding_face(&picture, edge);
        let chosen = Selection::at(
            definition,
            face,
            edge,
            ferritecad_viewport::VertexPickId::NOTHING,
            &picture,
            &scene.faces,
            &scene.edges,
            &ferritecad_scene::VertexNames::default(),
        );
        let Selection::Edge(selected) = &chosen else {
            panic!("the triply named cap edge was not selected as an edge: {chosen:?}");
        };
        let words = words_of(&chosen, &scene.catalogue, &picture);
        let names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.edges.iter().map(topology_name).collect();
        let described = inspected(
            &chosen,
            &scene.catalogue,
            &words.identities,
            &[],
            &names,
            &[],
            &picture,
        )
        .expect("the triply named edge reaches the inspector");
        let rows = described.rows();
        assert_eq!(
            rows.first().map(|(label, value)| (*label, value.as_str())),
            Some(("Kind", "Edge"))
        );

        // One sentence per stored name, all alike because all three say the
        // same thing, and in the order the document keeps them.
        let said: Vec<&str> = rows
            .iter()
            .filter(|(label, _)| *label == "Role")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(said.len(), 3, "three names, three sentences");
        assert_eq!(
            said,
            cap_meanings(selected)
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            "the inspector dropped or reordered the cap-edge sentences"
        );
        for sentence in &said {
            assert_eq!(
                *sentence,
                format!("Start cap edge of profile segment {segment}")
            );
        }

        let order: Vec<ferritecad_types::StableEntityId> = Document::open(&path)
            .expect("reopens")
            .topology_refs()
            .expect("reads")
            .iter()
            .filter(|entry| written.iter().any(|w| w.id == entry.id))
            .map(|entry| entry.id)
            .collect();
        let arrived: Vec<String> = rows
            .iter()
            .filter(|(label, _)| *label == "Reference")
            .map(|(_, value)| value.clone())
            .collect();
        assert_eq!(
            arrived,
            order.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "the inspector names are not in the document's order"
        );

        // The named edge is a cap edge in the geometry too, asked through the
        // faces it bounds rather than through any coincidence of numbering.
        let definition_index = picture
            .definition_of_edge(edge)
            .expect("the picture issued the edge");
        let side_face = (0..picture.face_count())
            .filter_map(|ordinal| picture.face_of(definition_index, ordinal))
            .find(|face| {
                scene.faces.of(*face, &picture).iter().any(|meaning| {
                    matches!(
                        meaning.output_role,
                        SemanticRole::ExtrudeSide { profile_segment } if profile_segment == segment
                    )
                })
            })
            .expect("the picture has the face raised from that segment");
        assert!(
            picture.edge_bounds_face(edge, side_face),
            "a cap edge of this segment must bound the face raised from it"
        );
        assert!(
            (0..picture.face_count())
                .filter_map(|ordinal| picture.face_of(definition_index, ordinal))
                .filter(|face| {
                    scene.faces.of(*face, &picture).iter().any(|meaning| {
                        matches!(meaning.output_role, SemanticRole::ExtrudeCap { .. })
                    })
                })
                .any(|face| picture.edge_bounds_face(edge, face)),
            "a cap edge must bound a cap face"
        );
    }

    #[test]
    fn a_chosen_sweep_edge_uses_the_edge_semantics_that_already_existed() {
        let Some((_directory, scene)) = native_plate_with_named_sweep_edges() else {
            return;
        };
        let (definition, face, edge) = named_sweep_edge(&scene);
        let picture = scene.snapshot;

        let chosen = Selection::at(
            definition,
            face,
            edge,
            ferritecad_viewport::VertexPickId::NOTHING,
            &picture,
            &scene.faces,
            &scene.edges,
            &ferritecad_scene::VertexNames::default(),
        );
        let Selection::Edge(selected) = &chosen else {
            panic!("a named sweep edge was not chosen as an edge: {chosen:?}");
        };

        // Everything below is the generic edge behaviour, asked of a sweep
        // edge. Nothing here is a branch on the role: if one existed, these
        // would be the answers it changed.
        assert_eq!(chosen.bounds(&picture), picture.bounds_of_edge(edge));
        assert_ne!(chosen.bounds(&picture), picture.bounds_of(definition));
        assert_ne!(chosen.bounds(&picture), picture.bounds_of_face(face));

        let mut visibility = Visibility::new(&picture);
        assert!(visibility.can_hide(chosen.marked(), &picture));
        assert!(visibility.hide(chosen.marked(), &picture));
        assert!(!visibility.shows(0, &picture));

        // A list row still chooses the definition and never the edge.
        let by_row = Selection::definition(definition, &picture);
        assert!(matches!(by_row, Selection::Definition(_)), "{by_row:?}");

        // A pixel with no edge follows the existing face-selection rule. It
        // cannot retain the edge merely because the preceding pixel had one.
        assert_eq!(chosen.marked(), Marked::Edge(edge));
        let without_edge = Selection::at(
            definition,
            face,
            EdgePickId::NOTHING,
            ferritecad_viewport::VertexPickId::NOTHING,
            &picture,
            &scene.faces,
            &scene.edges,
            &ferritecad_scene::VertexNames::default(),
        );
        assert!(
            matches!(without_edge, Selection::Face(_)),
            "a named face under no edge produced {without_edge:?}"
        );
        assert_ne!(without_edge.marked(), chosen.marked());

        // The inspector says every stored name, in the document's order, and
        // describes each in portable terms.
        let words = words_of(&chosen, &scene.catalogue, &picture);
        assert!(words.faces.is_empty(), "an edge was described as a face");
        assert_eq!(
            words.edges.len(),
            scene.edges.of(edge, &picture).len(),
            "the inspector dropped a stored name"
        );
        let expected: Vec<String> = scene
            .edges
            .of(edge, &picture)
            .iter()
            .map(|meaning| describe_role(&meaning.output_role))
            .collect();
        let said: Vec<String> = words.edges.iter().map(|words| words.role.clone()).collect();
        assert_eq!(said, expected, "the inspector reordered the stored names");
        assert_eq!(said, sweep_meanings(selected));
        for sentence in &said {
            assert!(sentence.starts_with("Sweep edge at the joint of profile segments "));
        }
    }

    /// One corner of a loaded plate the document names exactly, with a face it
    /// touches and an edge it ends.
    ///
    /// Every part read out of the picture's own packed partitions, so nothing
    /// here depends on an ordinal lining up with anything.
    fn named_cap_vertex(
        scene: &LoadedScene,
    ) -> (
        PickId,
        FacePickId,
        EdgePickId,
        ferritecad_viewport::VertexPickId,
    ) {
        let picture = &scene.snapshot;
        let definition = picture.pick_of(0).expect("the plate is drawn");
        let vertex = (0..picture.vertex_count())
            .filter_map(|ordinal| picture.vertex_of(0, ordinal))
            .find(|vertex| !scene.vertices.of(*vertex, picture).is_empty())
            .expect("the document names a corner of the plate");
        let face = (0..picture.face_count())
            .filter_map(|ordinal| picture.face_of(0, ordinal))
            .find(|face| picture.vertex_touches_face(vertex, *face))
            .expect("the corner touches a face of the picture");
        let edge = (0..picture.edge_count())
            .filter_map(|ordinal| picture.edge_of(0, ordinal))
            .find(|edge| picture.vertex_ends_edge(vertex, *edge))
            .expect("the corner ends an edge of the picture");
        (definition, face, edge, vertex)
    }

    #[test]
    fn a_chosen_corner_is_framed_hidden_and_described_as_its_own_thing() {
        let Some((_directory, scene)) = native_plate_with_named_cap_vertices() else {
            return;
        };
        let (definition, face, edge, vertex) = named_cap_vertex(&scene);
        let picture = scene.snapshot.clone();

        let chosen = Selection::at(
            definition,
            face,
            edge,
            vertex,
            &picture,
            &scene.faces,
            &scene.edges,
            &scene.vertices,
        );
        assert!(matches!(chosen, Selection::Vertex(_)), "{chosen:?}");

        // Framed on the corner itself, not on the edge, the face or the part.
        let where_it_is = chosen
            .bounds(&picture)
            .expect("a chosen corner is somewhere in the picture");
        assert_eq!(Some(where_it_is), picture.bounds_of_vertex(vertex));
        assert_ne!(Some(where_it_is), picture.bounds_of(definition));
        assert_ne!(Some(where_it_is), picture.bounds_of_face(face));
        assert_ne!(Some(where_it_is), picture.bounds_of_edge(edge));

        // And the camera really uses it: framing what is chosen is framing
        // that extent and nothing wider.
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        let mut scene_state = LiveScene::new(
            (),
            scene.catalogue.clone(),
            scene.faces.clone(),
            scene.edges.clone(),
            scene.vertices.clone(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene_state.selection = chosen.clone();
        assert!(
            frame_selection(&scene_state, &picture, &mut input).expect("frames"),
            "framing a chosen corner moved nothing"
        );
        let on_the_corner = *input.camera();
        let mut wider = ViewportInput::new();
        wider.resize(480, 480);
        wider
            .frame_extent(picture.bounds_of(definition))
            .expect("frames the part");
        assert_ne!(
            format!("{:?}", on_the_corner.view_projection()),
            format!("{:?}", wider.camera().view_projection()),
            "framing a corner framed the whole part"
        );

        // Hiding and isolating act on the part the corner belongs to.
        let mut visibility = Visibility::new(&picture);
        assert!(visibility.can_hide(chosen.marked(), &picture));
        assert!(visibility.hide(chosen.marked(), &picture));
        assert!(!visibility.shows(0, &picture));

        // A list row still chooses the definition and never the corner.
        let by_row = Selection::definition(definition, &picture);
        assert!(matches!(by_row, Selection::Definition(_)), "{by_row:?}");
    }

    #[test]
    fn the_inspector_names_a_chosen_corner_by_its_cap_and_both_of_its_segments() {
        let Some((_directory, scene)) = native_plate_with_named_cap_vertices() else {
            return;
        };
        let (definition, face, edge, vertex) = named_cap_vertex(&scene);
        let picture = scene.snapshot.clone();
        let chosen = Selection::at(
            definition,
            face,
            edge,
            vertex,
            &picture,
            &scene.faces,
            &scene.edges,
            &scene.vertices,
        );
        assert!(matches!(chosen, Selection::Vertex(_)), "{chosen:?}");

        // Every stored name, in the document's own order, one sentence each.
        let words = words_of(&chosen, &scene.catalogue, &picture);
        assert!(words.faces.is_empty(), "a corner was described as a face");
        assert!(words.edges.is_empty(), "a corner was described as an edge");
        let stored = scene.vertices.of(vertex, &picture);
        assert_eq!(
            words.vertices.len(),
            stored.len(),
            "the inspector dropped a stored name"
        );
        let expected: Vec<String> = stored
            .iter()
            .map(|meaning| describe_role(&meaning.output_role))
            .collect();
        let said: Vec<String> = words
            .vertices
            .iter()
            .map(|words| words.role.clone())
            .collect();
        assert_eq!(said, expected, "the inspector reordered the stored names");

        let names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.vertices.iter().map(topology_name).collect();
        let described = inspected(
            &chosen,
            &scene.catalogue,
            &words.identities,
            &[],
            &[],
            &names,
            &picture,
        )
        .expect("a corner of a native body is described");
        let rows = described.rows();
        assert_eq!(
            rows.first().map(|(key, value)| (*key, value.as_str())),
            Some(("Kind", "Vertex"))
        );

        // The wording is exact: which cap, and both segments of the joint in
        // the order the joint keeps them, which is the canonical one.
        let roles: Vec<&str> = rows
            .iter()
            .filter(|(key, _)| *key == "Role")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(roles.len(), stored.len(), "one sentence per stored name");
        for (sentence, meaning) in roles.iter().zip(stored) {
            let ferritecad_document::SemanticRole::ExtrudeCapVertex { side, joint } =
                &meaning.output_role
            else {
                panic!("a corner of the plate is named by something else: {meaning:?}");
            };
            let [one, another] = joint.segments();
            let wanted = match side {
                CapSide::Start => {
                    format!("Start cap vertex at the joint of profile segments {one} and {another}")
                }
                CapSide::End => {
                    format!("End cap vertex at the joint of profile segments {one} and {another}")
                }
                other => panic!("this fixture stores no {other:?} cap"),
            };
            assert_eq!(*sentence, wanted);
        }

        // The document's own identifiers reach the rows that carry them.
        for meaning in stored {
            for (key, value) in [
                ("Reference", meaning.reference.to_string()),
                ("Owner", meaning.owner.to_string()),
                ("Feature", meaning.producer_feature.to_string()),
                ("Entity", meaning.expected_kind.as_str().to_owned()),
                ("Rule", "Exactly this one".to_owned()),
            ] {
                assert!(
                    rows.iter().any(|(k, v)| *k == key && *v == value),
                    "the inspector lost {key} = {value}: {rows:?}"
                );
            }
        }

        // And nothing transient or kernel-side reaches any row.
        for (_, value) in &rows {
            for forbidden in [
                "session#",
                "shape#",
                "face#",
                "edge#",
                "vertex#",
                "VertexPickId",
                "EdgePickId",
                "FacePickId",
                "PickId",
                "SubShapeHandle",
                "ShapeHandle",
                "SessionId",
                "ProfileJoint",
                "ExtrudeCapVertex",
                "StableEntityId(",
                ".fcad",
            ] {
                assert!(
                    !value.contains(forbidden),
                    "the inspector printed {forbidden}: {value}"
                );
            }
        }
    }

    #[test]
    fn several_stored_corner_names_of_one_corner_are_said_one_each_in_order() {
        use ferritecad_document::{Document, EntityKind, ObjectPayload, SelectionRule};

        if !ferritecad_occt::is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return;
        }
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");

        // One cap corner named three times over, which a document may hold:
        // two objects can both name the same corner.
        let mut document = Document::open(&path).expect("opens the plate");
        let objects = document.objects().expect("reads objects");
        let sketch = objects
            .iter()
            .find_map(|object| match &object.payload {
                ObjectPayload::Sketch(sketch) => Some(sketch.clone()),
                _ => None,
            })
            .expect("the fixture has a sketch");
        let datum = objects
            .iter()
            .find_map(|object| match &object.payload {
                ObjectPayload::DatumPlane(datum) => Some(datum.clone()),
                _ => None,
            })
            .expect("the fixture has a datum plane");
        let plane = ferritecad_eval::plane_from_datum(&datum).expect("reads the plane");
        let profile =
            ferritecad_eval::profile_from_sketch(&sketch, ferritecad_types::ObjectId::new(), plane)
                .expect("builds a profile")
                .profile;
        let joint = profile
            .outer()
            .joints()
            .next()
            .expect("the plate has corners");

        let stored = document.topology_refs().expect("reads");
        let producer = stored
            .iter()
            .find_map(|reference| match &reference.output_role {
                SemanticRole::ExtrudeSide { .. } => Some(reference.producer_feature),
                _ => None,
            })
            .expect("the fixture names its swept faces");
        let owner = stored[0].owner;
        let mut written = Vec::new();
        for _ in 0..3 {
            written.push(ferritecad_document::TopologyRef {
                id: ferritecad_types::StableEntityId::new(),
                owner,
                producer_feature: producer,
                expected_kind: EntityKind::Vertex,
                output_role: SemanticRole::ExtrudeCapVertex {
                    side: CapSide::Start,
                    joint,
                },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            });
        }
        document
            .write(|w| {
                for reference in &written {
                    w.put_topology_ref(reference)?;
                }
                Ok(())
            })
            .expect("stores the cap vertex references");
        drop(document);

        let mut kernel = OcctKernel::new().expect("opens a session");
        let scene = snapshot_of(
            &path,
            &mut kernel,
            |kernel: &mut OcctKernel, bytes: &[u8]| kernel.import_step(bytes),
            &ferritecad_kernel::TessellationParams::default(),
            &ferritecad_kernel::OperationContext::default(),
        )
        .expect("the plate loads through Open CASCADE");
        let picture = scene.snapshot.clone();

        let vertex = (0..picture.vertex_count())
            .filter_map(|ordinal| picture.vertex_of(0, ordinal))
            .find(|vertex| scene.vertices.of(*vertex, &picture).len() == 3)
            .expect("one corner carries the three stored names");
        let definition = picture.pick_of(0).expect("drawn");
        let face = (0..picture.face_count())
            .filter_map(|ordinal| picture.face_of(0, ordinal))
            .find(|face| picture.vertex_touches_face(vertex, *face))
            .expect("the corner touches a face of the picture");
        let chosen = Selection::at(
            definition,
            face,
            EdgePickId::NOTHING,
            vertex,
            &picture,
            &scene.faces,
            &scene.edges,
            &scene.vertices,
        );
        let Selection::Vertex(selected) = &chosen else {
            panic!("the triply named corner was not selected as a corner: {chosen:?}");
        };
        assert_eq!(selected.meanings().len(), 3);

        // The order the document stores its references in, read from the
        // document rather than assumed.
        let reopened = Document::open_read_only(&path).expect("reopens");
        let order: Vec<ferritecad_types::StableEntityId> = reopened
            .topology_refs()
            .expect("reads")
            .into_iter()
            .map(|reference| reference.id)
            .collect();

        let words = words_of(&chosen, &scene.catalogue, &picture);
        let names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.vertices.iter().map(topology_name).collect();
        let described = inspected(
            &chosen,
            &scene.catalogue,
            &words.identities,
            &[],
            &[],
            &names,
            &picture,
        )
        .expect("the triply named corner reaches the inspector");
        let rows = described.rows();

        // Three sentences, one per stored name, in the order the document
        // stores them.
        let references: Vec<&str> = rows
            .iter()
            .filter(|(key, _)| *key == "Reference")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(references.len(), 3, "the inspector dropped a stored name");
        let positions: Vec<usize> = references
            .iter()
            .map(|shown| {
                order
                    .iter()
                    .position(|id| id.to_string() == *shown)
                    .expect("every name shown is stored")
            })
            .collect();
        let mut sorted = positions.clone();
        sorted.sort_unstable();
        assert_eq!(
            positions, sorted,
            "the inspector reordered the stored names: {positions:?}"
        );
        let expected: Vec<String> = selected
            .meanings()
            .iter()
            .map(|meaning| meaning.reference.to_string())
            .collect();
        assert_eq!(
            references, expected,
            "the inspector reordered what it was given"
        );

        let roles: Vec<&str> = rows
            .iter()
            .filter(|(key, _)| *key == "Role")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(roles.len(), 3, "one sentence per stored name");
        let [one, another] = joint.segments();
        for sentence in &roles {
            assert_eq!(
                *sentence,
                format!("Start cap vertex at the joint of profile segments {one} and {another}")
            );
        }
    }

    #[test]
    fn a_failed_open_keeps_a_chosen_corner_and_a_successful_one_forgets_it() {
        let Some((_directory, loaded)) = native_plate_with_named_cap_vertices() else {
            return;
        };
        let (definition, face, edge, vertex) = named_cap_vertex(&loaded);
        let picture = loaded.snapshot.clone();
        let chosen = Selection::at(
            definition,
            face,
            edge,
            vertex,
            &picture,
            &loaded.faces,
            &loaded.edges,
            &loaded.vertices,
        );
        assert!(matches!(chosen, Selection::Vertex(_)), "{chosen:?}");

        let mut scene = LiveScene {
            prepared: (),
            catalogue: loaded.catalogue.clone(),
            faces: loaded.faces.clone(),
            edges: loaded.edges.clone(),
            vertices: loaded.vertices.clone(),
            visibility: Visibility::new(&picture),
            selection: chosen.clone(),
            hovered: Hovered::Vertex(vertex),
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);

        // A load that failed changes nothing, including the corner names it
        // would need to describe what is chosen.
        commit_scene(
            &mut scene,
            &mut camera,
            Err(CadError::input("this is not a document")),
        )
        .expect_err("a failed load is reported");
        assert_eq!(scene.selection, chosen, "a failed open lost the choice");
        assert_eq!(scene.hovered, Hovered::Vertex(vertex));
        assert!(
            !scene.vertices.of(vertex, &picture).is_empty(),
            "a failed open lost the corner names"
        );

        // A load that arrived replaces all of it, choice included.
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut scene,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: loaded.catalogue.clone(),
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::default(),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.hovered, Hovered::Nothing);
        assert!(scene.vertices.of(vertex, &picture).is_empty());
    }

    #[test]
    fn hiding_a_chosen_corner_forgets_it_and_isolating_keeps_it() {
        let Some((_directory, loaded)) = native_plate_with_named_cap_vertices_beside(1) else {
            return;
        };
        let (definition, face, edge, vertex) = named_cap_vertex(&loaded);
        let picture = loaded.snapshot.clone();
        let chosen = Selection::at(
            definition,
            face,
            edge,
            vertex,
            &picture,
            &loaded.faces,
            &loaded.edges,
            &loaded.vertices,
        );
        assert!(matches!(chosen, Selection::Vertex(_)), "{chosen:?}");
        let owner = chosen
            .owning_definition(&picture)
            .expect("the corner belongs to a definition of this picture");

        // Isolate keeps the corner chosen exactly as that corner, and takes
        // the other body off screen.
        let mut scene = LiveScene {
            prepared: (),
            catalogue: loaded.catalogue.clone(),
            faces: loaded.faces.clone(),
            edges: loaded.edges.clone(),
            vertices: loaded.vertices.clone(),
            visibility: Visibility::new(&picture),
            selection: chosen.clone(),
            hovered: Hovered::Vertex(vertex),
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        let camera_before = format!("{:?}", input.camera().view_projection());
        assert!(can_isolate_selection(&scene, &picture));
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input,
        ));
        assert_eq!(scene.selection, chosen, "isolating changed the choice");
        assert!(scene.visibility.shows(owner, &picture));
        for other in 0..picture.meshes().len() {
            if other != owner {
                assert!(
                    !scene.visibility.shows(other, &picture),
                    "isolating left definition {other} on screen"
                );
            }
        }
        assert_eq!(
            camera_before,
            format!("{:?}", input.camera().view_projection()),
            "isolating moved the camera"
        );

        // Undo puts the other body back and leaves the choice alone.
        assert!(undo_visibility(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input,
        ));
        for definition in 0..picture.meshes().len() {
            assert!(scene.visibility.shows(definition, &picture));
        }
        assert_eq!(scene.selection, chosen, "undo changed the choice");

        // Hide takes the corner's own definition off screen and forgets the
        // choice with it.
        assert!(can_hide_selection(&scene, &picture));
        assert!(hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input,
        ));
        assert!(!scene.visibility.shows(owner, &picture));
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.hovered, Hovered::Nothing);
        assert_eq!(
            camera_before,
            format!("{:?}", input.camera().view_projection()),
            "hiding moved the camera"
        );

        // Undoing the hide draws the definition again. The choice does not
        // come back: it was forgotten when what it was on stopped being drawn.
        assert!(undo_visibility(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input,
        ));
        assert!(scene.visibility.shows(owner, &picture));
        assert_eq!(scene.selection, Selection::Nothing);

        // A corner of a picture that is not this one changes nothing at all.
        let Some((_other_directory, elsewhere)) = native_plate_with_named_cap_vertices() else {
            return;
        };
        let foreign = ferritecad_viewport::Marked::Vertex(
            elsewhere.snapshot.vertex_of(0, 0).expect("numbered"),
        );
        let mut untouched = Visibility::new(&picture);
        assert!(!untouched.can_hide(foreign, &picture));
        assert!(!untouched.hide(foreign, &picture));
        assert!(!untouched.isolate(foreign, &picture));
        for definition in 0..picture.meshes().len() {
            assert!(untouched.shows(definition, &picture));
        }
    }

    #[test]
    fn a_chosen_edge_is_framed_hidden_and_described_as_its_own_thing() {
        let Some((_directory, scene)) = native_plate_with_named_edges() else {
            return;
        };
        let (definition, face, edge) = named_cap_edge(&scene);
        let picture = scene.snapshot;

        let chosen = Selection::at(
            definition,
            face,
            edge,
            ferritecad_viewport::VertexPickId::NOTHING,
            &picture,
            &scene.faces,
            &scene.edges,
            &ferritecad_scene::VertexNames::default(),
        );
        assert!(matches!(chosen, Selection::Edge(_)), "{chosen:?}");

        // Framed on the edge itself, not on the face or the part.
        assert_eq!(chosen.bounds(&picture), picture.bounds_of_edge(edge));
        assert_ne!(chosen.bounds(&picture), picture.bounds_of(definition));
        assert_ne!(chosen.bounds(&picture), picture.bounds_of_face(face));

        // Hiding and isolating act on the part the edge belongs to.
        let mut visibility = Visibility::new(&picture);
        assert!(visibility.can_hide(chosen.marked(), &picture));
        assert!(visibility.can_isolate(chosen.marked(), &picture) || picture.meshes().len() == 1);
        assert!(visibility.hide(chosen.marked(), &picture));
        assert!(!visibility.shows(0, &picture));

        // A list row still chooses the definition and never the edge.
        let by_row = Selection::definition(definition, &picture);
        assert!(matches!(by_row, Selection::Definition(_)), "{by_row:?}");

        // The inspector says only durable words, and says every name.
        let words = words_of(&chosen, &scene.catalogue, &picture);
        assert!(words.faces.is_empty(), "an edge was described as a face");
        assert_eq!(
            words.edges.len(),
            scene.edges.of(edge, &picture).len(),
            "the inspector dropped a stored name"
        );
        let names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.edges.iter().map(topology_name).collect();
        let described = inspected(
            &chosen,
            &scene.catalogue,
            &words.identities,
            &[],
            &names,
            &[],
            &picture,
        )
        .expect("an edge of a native body is described");
        let rows = described.rows();
        assert_eq!(
            rows.first().map(|(k, v)| (*k, v.as_str())),
            Some(("Kind", "Edge"))
        );
        for (_, value) in &rows {
            for forbidden in [
                "session#",
                "shape#",
                "face#",
                "edge#",
                "EdgePickId",
                "FacePickId",
                "SubShapeHandle",
            ] {
                assert!(
                    !value.contains(forbidden),
                    "the inspector printed {forbidden}: {value}"
                );
            }
        }
        // Every stored term reaches the panel.
        for label in ["Reference", "Owner", "Feature", "Entity", "Role", "Rule"] {
            assert!(
                rows.iter().any(|(key, _)| *key == label),
                "the inspector never said {label}"
            );
        }
    }

    #[test]
    fn the_inspector_names_a_sweep_edge_by_both_of_its_profile_segments() {
        let Some((_directory, scene)) = native_plate_with_named_sweep_edges() else {
            return;
        };
        let (definition, face, edge) = named_sweep_edge(&scene);
        let picture = scene.snapshot;
        let chosen = Selection::at(
            definition,
            face,
            edge,
            ferritecad_viewport::VertexPickId::NOTHING,
            &picture,
            &scene.faces,
            &scene.edges,
            &ferritecad_scene::VertexNames::default(),
        );

        // The two segments the stored name is actually made of.
        let joints: Vec<[ferritecad_types::StableEntityId; 2]> = scene
            .edges
            .of(edge, &picture)
            .iter()
            .filter_map(|meaning| match &meaning.output_role {
                SemanticRole::ExtrudeSweepEdge { joint } => Some(joint.segments()),
                _ => None,
            })
            .collect();
        assert!(!joints.is_empty(), "the chosen edge has a sweep name");

        let words = words_of(&chosen, &scene.catalogue, &picture);
        let names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.edges.iter().map(topology_name).collect();
        let described = inspected(
            &chosen,
            &scene.catalogue,
            &words.identities,
            &[],
            &names,
            &[],
            &picture,
        )
        .expect("an edge of a native body is described");
        let rows = described.rows();

        assert_eq!(
            rows.first().map(|(k, v)| (*k, v.as_str())),
            Some(("Kind", "Edge"))
        );

        // Both durable identifiers are shown, and the sentence is the stable
        // one rather than whatever Debug would print today.
        let roles: Vec<&str> = rows
            .iter()
            .filter(|(key, _)| *key == "Role")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(roles.len(), joints.len(), "one sentence per stored name");
        for (sentence, [one, other]) in roles.iter().zip(&joints) {
            assert_eq!(
                *sentence,
                format!("Sweep edge at the joint of profile segments {one} and {other}")
            );
        }

        // And nothing transient or kernel-side reaches any row.
        for (_, value) in &rows {
            for forbidden in [
                "session#",
                "shape#",
                "face#",
                "edge#",
                "EdgePickId",
                "FacePickId",
                "SubShapeHandle",
                "ShapeHandle",
                "SessionId",
                "ProfileJoint",
                "ExtrudeSweepEdge",
                "StableEntityId(",
                ".fcad",
            ] {
                assert!(
                    !value.contains(forbidden),
                    "the inspector printed {forbidden}: {value}"
                );
            }
        }
    }

    #[test]
    fn a_failed_open_keeps_a_chosen_edge_and_a_successful_one_forgets_it() {
        let Some((_directory, loaded)) = native_plate_with_named_edges() else {
            return;
        };
        let picture = loaded.snapshot;
        let definition = picture.pick_of(0).expect("drawn");
        let face = picture.face_of(0, 0).expect("numbered");
        let edge = (0..picture.edge_count())
            .filter_map(|ordinal| picture.edge_of(0, ordinal))
            .find(|edge| !loaded.edges.of(*edge, &picture).is_empty())
            .expect("named");
        let chosen = Selection::at(
            definition,
            face,
            edge,
            ferritecad_viewport::VertexPickId::NOTHING,
            &picture,
            &loaded.faces,
            &loaded.edges,
            &ferritecad_scene::VertexNames::default(),
        );

        let mut scene = LiveScene {
            prepared: (),
            catalogue: loaded.catalogue.clone(),
            faces: loaded.faces.clone(),
            edges: loaded.edges.clone(),
            vertices: loaded.vertices.clone(),
            visibility: Visibility::new(&picture),
            selection: chosen.clone(),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);

        // A load that failed changes nothing, including the edge names it
        // would need to describe what is chosen.
        commit_scene(
            &mut scene,
            &mut camera,
            Err(CadError::input("this is not a document")),
        )
        .expect_err("a failed load is reported");
        assert_eq!(scene.selection, chosen, "a failed open lost the choice");
        assert!(
            !scene.edges.of(edge, &picture).is_empty(),
            "a failed open lost the edge names"
        );

        // A load that arrived replaces all of it, choice included.
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut scene,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: loaded.catalogue.clone(),
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::default(),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");
        assert_eq!(scene.selection, Selection::Nothing);
        assert!(scene.edges.of(edge, &picture).is_empty());
    }

    #[test]
    fn a_drag_and_a_claimed_event_never_ask_about_a_corner_that_is_really_there() {
        let Some((_directory, scene)) = native_plate_with_named_edges() else {
            return;
        };
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        input
            .frame(picture.bounds().expect("somewhere"))
            .expect("frames");
        let frame = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &Visibility::new(&picture),
            )
            .expect("draws");

        // A pixel where a corner really is: the question this test is about is
        // a live one, so refusing to ask it is a decision rather than an
        // absence of anything to ask.
        let (x, y) = (0..frame.height())
            .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
            .find(|(x, y)| {
                frame.hit_at(*x, *y).vertex() != ferritecad_viewport::VertexPickId::NOTHING
            })
            .expect("the plate draws a corner");
        let corner = hovered_at(frame.hit_at(x, y));
        assert!(
            matches!(corner, Hovered::Vertex(_)),
            "this pixel must really be a corner, got {corner:?}"
        );

        // Standing still over it asks about the corner.
        input.handle(
            ViewportEvent::PointerMoved {
                x: x as f32,
                y: y as f32,
            },
            false,
        );
        let idle = input.take_hover();
        assert_eq!(idle, Hover::At(x as f32, y as f32));
        assert_eq!(
            hover_request(None, false, idle),
            HoverRequest::Pixel(x as f32, y as f32)
        );

        // A gesture under way asks nothing, at that very pixel. The reducer
        // reports `Cleared` while a drag runs, and the app turns that into
        // no question rather than into a question about whatever it crossed.
        input.handle(ViewportEvent::PointerPressed(PointerButton::Middle), false);
        assert_eq!(input.take_hover(), Hover::Cleared);
        input.handle(
            ViewportEvent::PointerMoved {
                x: x as f32,
                y: y as f32,
            },
            false,
        );
        let dragging = input.take_hover();
        assert_eq!(dragging, Hover::Cleared, "a drag asked about the corner");
        assert_eq!(hover_request(None, false, dragging), HoverRequest::Clear);
        input.handle(ViewportEvent::PointerReleased(PointerButton::Middle), false);

        // And an event the interface claimed asks nothing either, however
        // certainly a corner is under the pointer.
        input.handle(
            ViewportEvent::PointerMoved {
                x: x as f32,
                y: y as f32,
            },
            true,
        );
        let claimed = input.take_hover();
        assert_eq!(
            claimed,
            Hover::Cleared,
            "a claimed move asked about the corner"
        );
        assert_eq!(
            hover_request(None, true, claimed),
            HoverRequest::Clear,
            "a claimed event reached the model at a corner"
        );

        // A list row still names a definition and can say nothing else.
        assert_eq!(
            hover_request(Some(0), false, Hover::Cleared),
            HoverRequest::Row(0)
        );
        let by_row = picture.pick_of(0).map(Hovered::Definition);
        assert!(
            matches!(by_row, Some(Hovered::Definition(_))),
            "a row must answer with a definition and never a corner"
        );
    }

    #[test]
    fn a_failed_open_keeps_a_corner_question_and_a_successful_one_forgets_it() {
        let Some((_directory, loaded)) = native_plate_with_named_edges() else {
            return;
        };
        let picture = loaded.snapshot;

        // A real identity from the picture, not a fabricated raw value.
        let corner = (0..picture.vertex_count())
            .filter_map(|ordinal| picture.vertex_of(0, ordinal))
            .next()
            .expect("the plate's corners reach the picture");
        let asked = Hovered::Vertex(corner);
        assert_eq!(
            asked.known_to(&picture),
            asked,
            "the question must be a live one before either load is tried"
        );

        let mut scene = LiveScene {
            prepared: (),
            catalogue: loaded.catalogue.clone(),
            faces: loaded.faces.clone(),
            edges: loaded.edges.clone(),
            vertices: loaded.vertices.clone(),
            visibility: Visibility::new(&picture),
            selection: Selection::Nothing,
            hovered: asked,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);

        // A load that failed changes nothing, the question included: the old
        // picture is still on screen, so what the pointer was over is still
        // true of it.
        commit_scene(
            &mut scene,
            &mut camera,
            Err(CadError::input("this is not a document")),
        )
        .expect_err("a failed load is reported");
        assert_eq!(
            scene.hovered, asked,
            "a failed open lost the corner the pointer was over"
        );

        // A load that arrived replaces the picture, so a question about the old
        // one is forgotten rather than carried onto geometry it never named.
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut scene,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: loaded.catalogue.clone(),
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::default(),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");
        assert_eq!(
            scene.hovered,
            Hovered::Nothing,
            "a successful open kept a question about the picture it replaced"
        );
    }

    #[test]
    fn what_a_load_hands_over_includes_the_names_of_its_edges() {
        let Some((_directory, loaded)) = native_plate_with_named_edges() else {
            return;
        };
        let picture = loaded.snapshot.clone();
        let edge = (0..picture.edge_count())
            .filter_map(|ordinal| picture.edge_of(0, ordinal))
            .find(|edge| !loaded.edges.of(*edge, &picture).is_empty())
            .expect("the document names an edge of the plate");
        let expected = loaded.edges.of(edge, &picture).len();

        let mut input = ViewportInput::new();
        input.resize(800, 600);
        // The route a real Open takes: everything fallible first, and the
        // parts handed over together afterwards.
        let edges = prepare_load(&input, Ok(loaded), |_| Ok(()))
            .expect("the load is prepared")
            .edges;

        assert_eq!(
            edges.of(edge, &picture).len(),
            expected,
            "preparing a load dropped what the document calls its edges"
        );
    }

    /// A pixel of the picture that sits on a topological edge, and the edge.
    ///
    /// Chosen through `Hit`, so the pixel is one where the edge and the
    /// definition under it agree: the outer silhouette, where a line lands on
    /// a pixel the fill did not reach, is deliberately not a candidate.
    fn an_edge_pixel(
        frame: &ferritecad_viewport_gpu::Frame,
    ) -> Option<(u32, u32, ferritecad_viewport::EdgePickId)> {
        (0..frame.height())
            .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let hit = frame.hit_at(x, y);
                // On an edge and not on a corner. A corner is the more
                // particular answer, so a pixel that is both is a question
                // about the corner and would say so.
                (hit.edge() != ferritecad_viewport::EdgePickId::NOTHING
                    && hit.vertex() == ferritecad_viewport::VertexPickId::NOTHING)
                    .then_some((x, y, hit.edge()))
            })
    }

    #[test]
    fn pointing_at_a_topological_edge_of_the_committed_plate_marks_that_edge() {
        let Some((_directory, scene)) = native_plate_scene() else {
            return;
        };
        let mut renderer = renderer_or_skip!();
        let picture = std::sync::Arc::new(scene.snapshot);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&picture))
            .expect("uploads");
        let mut input = ViewportInput::new();
        input.resize(480, 480);
        input
            .frame(picture.bounds().expect("the plate is somewhere"))
            .expect("frames");
        let visibility = Visibility::new(&picture);

        let plain = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");

        assert!(
            picture.edge_count() > 0,
            "the kernel that ships names the plate's edges"
        );
        let (x, y, edge) = an_edge_pixel(&plain).expect("the plate draws its edges");

        // The pointer is over that pixel. What the app decides it is over must
        // be the edge, and not the face the edge happens to lie on.
        let hit = plain.hit_at(x, y);
        assert_eq!(
            hovered_at(hit),
            Hovered::Edge(edge),
            "the pointer over an exact topological edge did not mark the edge"
        );

        // And the next frame must show it: those pixels change, and the inside
        // of the face does not.
        let mut hovered = Hovered::Nothing;
        assert!(hover(&mut hovered, &picture, hovered_at(hit)));
        let marked = renderer
            .render(
                &prepared,
                input.camera(),
                Marked::Nothing,
                hovered,
                &visibility,
            )
            .expect("draws");
        assert_ne!(
            marked.colour_at(x, y),
            plain.colour_at(x, y),
            "the edge under the pointer was not drawn any differently"
        );
    }

    /// A big plate with a small one directly behind it.
    ///
    /// From the camera `Camera::frame` chooses, the front one covers the rear
    /// one completely: every pixel of the model is the front plate, and there
    /// is no angle in this gate's remit that would show what is behind it.
    fn occluding_pair() -> (std::sync::Arc<RenderSnapshot>, Camera) {
        use ferritecad_kernel::{Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeKind};

        // A square in the XZ plane at `y`, facing the eye that framing puts on
        // the negative side of Y.
        let plate = |half: f32, y: f32, shape: u64| {
            let handle = ShapeHandle::new(SessionId::new(), shape);
            Mesh {
                topological_vertices: None,
                positions: vec![
                    -half, y, -half, half, y, -half, half, y, half, -half, y, half,
                ],
                normals: vec![
                    0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
                ],
                indices: vec![0, 1, 2, 0, 2, 3],
                faces: vec![MeshFaceRange {
                    face: ferritecad_kernel::SubShapeHandle::new(handle, SubShapeKind::Face, 0),
                    first_index: 0,
                    index_count: 6,
                }],
                edges: None,
            }
        };

        let mut builder = SnapshotBuilder::new();
        let front = builder.add_mesh(&plate(20.0, 0.0, 1)).expect("packs");
        let rear = builder.add_mesh(&plate(4.0, 9.0, 2)).expect("packs");
        builder
            .place(
                front,
                None,
                &ferritecad_types::Transform::IDENTITY,
                [0.8, 0.2, 0.2],
            )
            .expect("places");
        builder
            .place(
                rear,
                None,
                &ferritecad_types::Transform::IDENTITY,
                [0.2, 0.4, 0.9],
            )
            .expect("places");
        let snapshot = std::sync::Arc::new(builder.build());

        let mut camera = Camera::new();
        camera.resize(128, 128);
        camera
            .frame(snapshot.bounds().expect("the pair has an extent"))
            .expect("frames");
        (snapshot, camera)
    }

    /// A marker plate well to the right of a plate at the middle.
    ///
    /// Asymmetric on purpose: a picture that is the same after a quarter turn
    /// cannot say which way it turned. Both lie in the plane a front view
    /// targets, so where they are on screen is decided by the camera alone.
    fn marker_beside_a_middle(width: u32, height: u32) -> (std::sync::Arc<RenderSnapshot>, Camera) {
        use ferritecad_kernel::{Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeKind};

        let plate = |half: f32, shape: u64| {
            let handle = ShapeHandle::new(SessionId::new(), shape);
            Mesh {
                topological_vertices: None,
                positions: vec![
                    -half, 0.0, -half, half, 0.0, -half, half, 0.0, half, -half, 0.0, half,
                ],
                normals: vec![
                    0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
                ],
                indices: vec![0, 1, 2, 0, 2, 3],
                faces: vec![MeshFaceRange {
                    face: ferritecad_kernel::SubShapeHandle::new(handle, SubShapeKind::Face, 0),
                    first_index: 0,
                    index_count: 6,
                }],
                edges: None,
            }
        };

        let mut builder = SnapshotBuilder::new();
        let marker = builder.add_mesh(&plate(3.0, 1)).expect("packs");
        let middle = builder.add_mesh(&plate(6.0, 2)).expect("packs");
        for (definition, x, colour) in [
            (marker, 24.0, [0.9, 0.2, 0.2]),
            (middle, 0.0, [0.2, 0.4, 0.9]),
        ] {
            builder
                .place(
                    definition,
                    None,
                    &ferritecad_types::Transform::from_translation(
                        ferritecad_types::Vec3::new(x, 0.0, 0.0).expect("finite"),
                    )
                    .expect("finite"),
                    colour,
                )
                .expect("places");
        }
        let snapshot = std::sync::Arc::new(builder.build());

        let mut camera = Camera::new();
        camera.resize(width, height);
        camera
            .frame(snapshot.bounds().expect("the plates have an extent"))
            .expect("frames");
        (snapshot, camera)
    }

    /// The middle of everything one definition drew, in pixels.
    fn centre_of_pick(
        frame: &ferritecad_viewport_gpu::Frame,
        pick: ferritecad_viewport::PickId,
    ) -> Option<(f32, f32)> {
        let mut count = 0.0f32;
        let (mut x, mut y) = (0.0f32, 0.0f32);
        for py in 0..frame.height() {
            for px in 0..frame.width() {
                if frame.pick_at(px, py) == pick {
                    // A pixel covers a unit square, so its middle is half on.
                    x += px as f32 + 0.5;
                    y += py as f32 + 0.5;
                    count += 1.0;
                }
            }
        }
        (count > 0.0).then(|| (x / count, y / count))
    }

    #[test]
    fn two_fingers_turning_counterclockwise_turn_the_model_counterclockwise() {
        let mut renderer = renderer_or_skip!();
        let (snapshot, camera) = marker_beside_a_middle(256, 256);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&snapshot))
            .expect("uploads");
        let everything = ferritecad_viewport::Visibility::default();
        let marker = snapshot.pick_of(0).expect("drawn");

        let mut input = ViewportInput::new();
        input.resize(256, 256);
        input
            .frame(snapshot.bounds().expect("an extent"))
            .expect("frames");
        assert_eq!(
            *input.camera(),
            camera,
            "the gate framed two different views"
        );

        let draw = |renderer: &mut Renderer, camera: &Camera| {
            renderer
                .render(
                    &prepared,
                    camera,
                    Marked::Nothing,
                    Hovered::Nothing,
                    &everything,
                )
                .expect("draws")
        };

        let before = draw(&mut renderer, input.camera());
        let (was_x, was_y) = centre_of_pick(&before, marker).expect("the marker is on screen");
        assert!(
            was_x - 128.0 > 30.0 && (was_y - 128.0).abs() < 10.0,
            "the marker did not start to the right of the middle: ({was_x}, {was_y})"
        );

        // A quarter turn counterclockwise, as a trackpad reports it: degrees,
        // positive for counterclockwise.
        apply_viewport_input(
            &mut input,
            &WindowEvent::RotationGesture {
                device_id: winit::event::DeviceId::dummy(),
                delta: 90.0,
                phase: winit::event::TouchPhase::Moved,
            },
            false,
            false,
        );

        let after = draw(&mut renderer, input.camera());
        let (now_x, now_y) = centre_of_pick(&after, marker).expect("the marker left the view");
        // Counterclockwise on screen: what was to the right is now above, and
        // screen rows grow downwards.
        assert!(
            (now_x - 128.0).abs() < 10.0 && 128.0 - now_y > 30.0,
            "a counterclockwise turn moved the marker from ({was_x}, {was_y}) to ({now_x}, {now_y})"
        );
    }

    /// A picture of two definitions, each placed twice.
    fn two_definitions() -> RenderSnapshot {
        let mut builder = SnapshotBuilder::new();
        let first = builder.add_mesh(&distant_scene_mesh()).expect("packs");
        let second = builder.add_mesh(&distant_scene_mesh()).expect("packs");
        for definition in [first, second] {
            for x in [0.0, 40.0] {
                builder
                    .place(
                        definition,
                        None,
                        &ferritecad_types::Transform::from_translation(
                            ferritecad_types::Vec3::new(x + definition as f64 * 100.0, 0.0, 0.0)
                                .expect("finite"),
                        )
                        .expect("finite"),
                        [0.5, 0.5, 0.5],
                    )
                    .expect("places");
            }
        }
        builder.build()
    }

    /// A scene with that picture, and something chosen in it.
    fn live_with(picture: &RenderSnapshot, chosen: usize) -> LiveScene<()> {
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection =
            Selection::Definition(picture.pick_of(chosen).expect("the picture has that row"));
        scene
    }

    #[test]
    fn a_chosen_definition_that_draws_nothing_cannot_offer_hide() {
        let mut builder = SnapshotBuilder::new();
        let empty = builder
            .add_mesh(&ferritecad_kernel::Mesh::default())
            .expect("packs");
        builder
            .place(
                empty,
                None,
                &ferritecad_types::Transform::IDENTITY,
                [1.0, 1.0, 1.0],
            )
            .expect("places");
        let picture = builder.build();
        let mut scene = LiveScene::new(
            (),
            vec![a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection =
            Selection::Definition(picture.pick_of(empty).expect("the definition has a row"));
        let before = scene.selection.clone();
        let mut input = ViewportInput::new();
        let _ = input.take_redraw();

        assert_eq!(selection_bounds(&scene, &picture), None);
        assert!(
            !can_hide_selection(&scene, &picture),
            "the toolbar offered Hide for a definition with no pixels"
        );
        assert!(!hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input,
        ));
        assert_eq!(scene.selection, before, "an unavailable action unchose it");
        assert!(!scene.visibility.anything_hidden());
        assert!(!input.take_redraw());
    }

    #[test]
    fn hiding_what_is_chosen_forgets_it_and_everything_pointing_at_it() {
        let picture = two_definitions();
        let mut scene = live_with(&picture, 0);
        scene.hovered = Hovered::Definition(picture.pick_of(0).expect("drawn"));
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let camera = input.camera().view_projection();
        // A click and a question already in flight, about the frame on screen.
        let click = |input: &mut ViewportInput| {
            input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
            input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
            input.handle(
                ViewportEvent::PointerReleased(PointerButton::Primary),
                false,
            );
        };
        let mut proof = input.clone();
        click(&mut proof);
        assert!(
            proof.take_pick().is_some(),
            "the gate needs a click that would be answered"
        );
        click(&mut input);
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);

        assert!(hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));

        // What was chosen is not chosen, what was pointed at is not pointed
        // at, and nothing in flight can bring either back.
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.hovered, Hovered::Nothing);
        assert_eq!(
            input.take_pick(),
            None,
            "a click survived hiding its target"
        );
        assert_eq!(input.take_hover(), Hover::Cleared);
        assert!(input.take_redraw(), "hiding something owes a frame");
        assert!(!scene.visibility.shows(0, &picture));
        assert!(scene.visibility.shows(1, &picture));

        // And the camera did not move for any of it.
        assert_eq!(input.camera().view_projection(), camera);
    }

    #[test]
    fn showing_everything_puts_it_back_without_choosing_anything() {
        let picture = two_definitions();
        let mut scene = live_with(&picture, 0);
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        let camera = input.camera().view_projection();
        let _ = input.take_redraw();

        assert!(show_all(
            &mut scene.visibility,
            &mut scene.hovered,
            &mut input
        ));
        assert!(scene.visibility.shows(0, &picture));
        assert!(!scene.visibility.anything_hidden());
        assert!(input.take_redraw(), "showing everything owes a frame");

        // Putting something back on screen is not deciding that it is what the
        // user is working on.
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.hovered, Hovered::Nothing);
        assert_eq!(input.camera().view_projection(), camera);
    }

    #[test]
    fn an_action_with_nothing_to_do_asks_for_nothing() {
        let picture = two_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let _ = input.take_redraw();

        // Nothing chosen, so there is nothing to hide; nothing hidden, so
        // there is nothing to show. Neither owes a frame.
        assert!(!hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        assert!(!show_all(
            &mut scene.visibility,
            &mut scene.hovered,
            &mut input
        ));
        assert!(
            !input.take_redraw(),
            "an action that did nothing asked for a frame"
        );

        // And repeating an action that has already happened is the same.
        scene.selection = Selection::Definition(picture.pick_of(0).expect("drawn"));
        assert!(hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        let _ = input.take_redraw();
        assert!(!hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        assert!(show_all(
            &mut scene.visibility,
            &mut scene.hovered,
            &mut input
        ));
        let _ = input.take_redraw();
        assert!(!show_all(
            &mut scene.visibility,
            &mut scene.hovered,
            &mut input
        ));
        assert!(!input.take_redraw());
    }

    #[test]
    fn framing_follows_what_is_still_drawn() {
        let picture = two_definitions();
        let mut scene = live_with(&picture, 0);
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        // Everything, then everything that is left.
        let mut all = input.clone();
        frame_scene(&scene.visibility, &picture, &mut all).expect("frames");
        assert!(hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        let mut visible = input.clone();
        frame_scene(&scene.visibility, &picture, &mut visible).expect("frames");
        assert_ne!(
            all.camera().view_projection(),
            visible.camera().view_projection(),
            "framing everything and framing what is left put the camera in one place"
        );

        // What is chosen was unchosen by hiding it, so there is nowhere to go.
        assert_eq!(selection_bounds(&scene, &picture), None);
        assert!(
            !frame_selection(&scene, &picture, &mut input)
                .expect("having nowhere to go is not a failure")
        );

        // With everything hidden there is no model to frame at all.
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        assert!(hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        assert_eq!(scene.visibility.bounds(&picture), None);
        assert!(!frame_scene(&scene.visibility, &picture, &mut input).expect("nowhere to go"));
    }

    #[test]
    fn a_successful_open_shows_everything_again_and_a_failed_one_changes_nothing() {
        let picture = two_definitions();
        let mut scene = live_with(&picture, 0);
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        let hidden = scene.visibility.clone();

        // A load that failed leaves the picture, what is hidden in it, and
        // what is chosen exactly as they were.
        let mut camera = ViewportInput::new();
        commit_scene(&mut scene, &mut camera, Err(CadError::input("no")))
            .expect_err("a failed load commits nothing");
        assert_eq!(scene.visibility, hidden);
        assert!(!scene.visibility.shows(0, &picture));

        // A load that arrived replaces all of it at once: a document does not
        // open with parts already missing.
        let next = two_definitions();
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut scene,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: vec![a_body(), a_body()],
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::new(&next),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");
        assert!(!scene.visibility.anything_hidden());
        assert!(scene.visibility.shows(0, &next));
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.hovered, Hovered::Nothing);
    }

    #[test]
    fn the_keys_that_hide_and_show_are_the_ones_the_panel_prints() {
        // Read from the same constants the buttons print.
        assert!(wants(&Key::Character(HIDE_KEY.into()), false, HIDE_KEY));
        assert!(wants(
            &Key::Character(HIDE_KEY.to_lowercase().into()),
            false,
            HIDE_KEY
        ));
        assert!(wants(
            &Key::Character(SHOW_ALL_KEY.into()),
            false,
            SHOW_ALL_KEY
        ));

        // Distinct from each other and from everything else bound here: two
        // actions on one key is a shortcut whose meaning depends on state
        // nobody can see.
        let bound = [FRAME_KEY, FRAME_ALL_KEY, HIDE_KEY, SHOW_ALL_KEY];
        for (first, one) in bound.iter().enumerate() {
            for other in bound.iter().skip(first + 1) {
                assert_ne!(one, other, "two actions share one key");
            }
            assert!(
                VIEWS.iter().all(|(_, _, view)| view != one),
                "{one} is also a view key"
            );
        }
        assert!(named_view(&Key::Character(HIDE_KEY.into())).is_none());
        assert!(named_view(&Key::Character(SHOW_ALL_KEY.into())).is_none());
        assert!(!wants(&Key::Named(NamedKey::Home), false, HIDE_KEY));
        assert!(!wants(&Key::Named(NamedKey::Home), false, SHOW_ALL_KEY));

        // And the interface has first refusal for both, exactly as it does for
        // framing: a focused text control that accepted an H did not also ask
        // to hide a part of the model.
        assert!(!wants(&Key::Character(HIDE_KEY.into()), true, HIDE_KEY));
        assert!(!wants(
            &Key::Character(SHOW_ALL_KEY.into()),
            true,
            SHOW_ALL_KEY
        ));
    }

    /// Three plates side by side, the middle one boxed in by its neighbours.
    ///
    /// Each is placed twice, so "the selected definition stays" and "the
    /// others go" are both claims about more than one placement.
    fn three_definitions() -> RenderSnapshot {
        let mut builder = SnapshotBuilder::new();
        let mut picks = Vec::new();
        for _ in 0..3 {
            picks.push(builder.add_mesh(&distant_scene_mesh()).expect("packs"));
        }
        for definition in &picks {
            for x in [0.0, 40.0] {
                builder
                    .place(
                        *definition,
                        None,
                        &ferritecad_types::Transform::from_translation(
                            ferritecad_types::Vec3::new(x + *definition as f64 * 100.0, 0.0, 0.0)
                                .expect("finite"),
                        )
                        .expect("finite"),
                        [0.5, 0.5, 0.5],
                    )
                    .expect("places");
            }
        }
        builder.build()
    }

    /// The committed plate with a second, unnamed body beside it.
    ///
    /// The plate brings durable face names, so a face can really be chosen;
    /// the second body is what makes isolating something that would change the
    /// picture. Written into a copy, never into the checkout.
    fn plate_and_a_second_body() -> (tempfile::TempDir, LoadedScene) {
        plate_and_more_bodies(1)
    }

    /// The committed plate with `extra` unnamed bodies beside it.
    ///
    /// The plate brings durable face names, so a face can really be chosen;
    /// the others are what make hiding and undoing something that changes the
    /// picture. Written into a copy, never into the checkout.
    /// Writes `extra` further extruded bodies into a copy of the plate.
    ///
    /// Separate from the loaders below because both a mock load and a real
    /// Open CASCADE one want the same document, and writing it twice would be
    /// two documents that drift.
    fn add_extra_bodies(path: &std::path::Path, extra: usize) {
        use ferritecad_document::{
            Body, Dependency, DependencyRole, EndCondition, Expression, Extrude, ObjectPayload,
            Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
        };
        use ferritecad_types::ObjectId;

        let mut document = ferritecad_document::Document::open(path).expect("opens");
        let plane = document
            .objects()
            .expect("reads objects")
            .into_iter()
            .find(|object| matches!(object.payload, ObjectPayload::DatumPlane(_)))
            .expect("the plate is sketched on a plane")
            .id;
        document
            .write(|w| {
                for extra in 0..extra {
                    let (sketch, extrude, body) =
                        (ObjectId::new(), ObjectId::new(), ObjectId::new());
                    let left = 100.0 + extra as f64 * 50.0;
                    let corners = [
                        (left, 0.0),
                        (left + 40.0, 0.0),
                        (left + 40.0, 30.0),
                        (left, 30.0),
                    ];
                    let ordinal = 100 + extra as i64 * 3;
                    let mut curves = Vec::new();
                    for index in 0..corners.len() {
                        let (sx, sy) = corners[index];
                        let (ex, ey) = corners[(index + 1) % corners.len()];
                        curves.push(SketchCurve {
                            id: ferritecad_types::StableEntityId::new(),
                            construction: false,
                            geometry: SketchGeometry::Line {
                                start: Point2::new(sx, sy)?,
                                end: Point2::new(ex, ey)?,
                            },
                        });
                    }
                    w.put_object(
                        sketch,
                        None,
                        ordinal,
                        None,
                        &ObjectPayload::Sketch(Sketch {
                            plane,
                            curves,
                            constraints: Vec::new(),
                        }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: sketch,
                        dependency: plane,
                        role: DependencyRole::Plane,
                    })?;
                    w.put_object(
                        extrude,
                        None,
                        ordinal + 1,
                        None,
                        &ObjectPayload::Extrude(Extrude {
                            profile: sketch,
                            end_condition: EndCondition::Blind {
                                distance: Expression::constant(4.0)?,
                            },
                            reversed: false,
                            operation: SolidOperation::NewBody,
                            target_body: None,
                        }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: extrude,
                        dependency: sketch,
                        role: DependencyRole::Profile,
                    })?;
                    w.put_object(
                        body,
                        None,
                        ordinal + 2,
                        Some("Bracket"),
                        &ObjectPayload::Body(Body {
                            tip_feature: Some(extrude),
                        }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: body,
                        dependency: extrude,
                        role: DependencyRole::BodyTip,
                    })?;
                }
                Ok(())
            })
            .expect("writes the extra bodies");
        drop(document);
    }

    fn plate_and_more_bodies(extra: usize) -> (tempfile::TempDir, LoadedScene) {
        use ferritecad_kernel::mock::MockKernel;

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");

        add_extra_bodies(&path, extra);

        let scene = snapshot_of(
            &path,
            &mut MockKernel::new(),
            |_: &mut MockKernel, _: &[u8]| {
                Err(CadError::unsupported("this document holds no imports"))
            },
            &ferritecad_kernel::TessellationParams::default(),
            &ferritecad_kernel::OperationContext::default(),
        )
        .expect("the document loads");
        assert_eq!(
            scene.snapshot.meshes().len(),
            extra + 1,
            "the plate and its neighbours were written"
        );
        (directory, scene)
    }

    #[test]
    fn isolating_keeps_a_chosen_face_exactly_as_that_face() {
        let (_directory, scene) = plate_and_a_second_body();
        let snapshot = &scene.snapshot;

        // A face the document names, chosen as that face, with another body
        // beside it so isolating would change the picture.
        let face = snapshot.face_of(0, 0).expect("numbered");
        let chosen = Selection::at(
            snapshot.pick_of(0).expect("drawn"),
            face,
            EdgePickId::NOTHING,
            ferritecad_viewport::VertexPickId::NOTHING,
            snapshot,
            &scene.faces,
            &EdgeNames::default(),
            &ferritecad_scene::VertexNames::default(),
        );
        let Selection::Face(before) = &chosen else {
            panic!("the plate's face is not named: {chosen:?}");
        };
        let meanings = before.meanings().to_vec();

        let mut visibility = Visibility::new(snapshot);
        let mut hovered = Hovered::Nothing;
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(visibility.can_isolate(chosen.marked(), snapshot));
        assert!(isolate_selected(
            &mut visibility,
            &chosen,
            &mut hovered,
            snapshot,
            &mut input
        ));

        // The part the face is on is what stays; the other body goes.
        assert!(visibility.shows(0, snapshot));
        assert!(!visibility.shows(1, snapshot));

        // And the choice is still that face, with what the document calls it.
        let Selection::Face(after) = &chosen else {
            panic!("the choice stopped being a face");
        };
        assert_eq!(after.face(), face);
        assert_eq!(after.meanings(), meanings.as_slice());
        assert_eq!(chosen.marked(), Marked::Face(face));
    }

    #[test]
    fn isolating_keeps_the_choice_exactly_and_forgets_everything_pointing_elsewhere() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        // A face selection, which is the one that could most easily be
        // downgraded by an operation that dealt only in definitions.
        let face = picture.face_of(1, 0).expect("numbered");
        scene.selection = Selection::at(
            picture.pick_of(1).expect("drawn"),
            face,
            EdgePickId::NOTHING,
            ferritecad_viewport::VertexPickId::NOTHING,
            &picture,
            &FaceNames::default(),
            &EdgeNames::default(),
            &ferritecad_scene::VertexNames::default(),
        );
        // With no durable names this falls back to the definition, so the face
        // case is stated with the transient mark the renderer is given.
        scene.hovered = Hovered::Face(picture.face_of(0, 0).expect("numbered"));
        let chosen = scene.selection.clone();
        let camera = input.camera().view_projection();

        // A click and a question already in flight, about the frame on screen.
        input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);

        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));

        // The choice is exactly what it was: this is the operation for looking
        // at what you have already chosen.
        assert_eq!(scene.selection, chosen);
        // What pointed elsewhere is gone, and nothing in flight can answer
        // against the frame being replaced.
        assert_eq!(scene.hovered, Hovered::Nothing);
        assert_eq!(input.take_pick(), None);
        assert_eq!(input.take_hover(), Hover::Cleared);
        assert!(input.take_redraw(), "isolating owes a frame");
        assert_eq!(input.camera().view_projection(), camera);
    }

    #[test]
    fn isolating_a_chosen_face_keeps_it_chosen_as_that_face() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let mut visibility = Visibility::new(&scene.snapshot);
        let mut hovered = Hovered::Nothing;
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        // Construction and resizing both owe a frame of their own; what this
        // gate is about is whether the action adds one.
        let _ = input.take_redraw();

        // The plate is the only definition the fixture draws, so there is
        // nothing to isolate away and the action is not offered.
        assert!(!visibility.can_isolate(chosen.marked(), &scene.snapshot));
        assert!(!isolate_selected(
            &mut visibility,
            &chosen,
            &mut hovered,
            &scene.snapshot,
            &mut input
        ));
        assert!(
            !input.take_redraw(),
            "an unavailable action asked for a frame"
        );

        // The face selection survives the attempt untouched, including what
        // the document calls it.
        let Selection::Face(face) = &chosen else {
            panic!("the fixture chose no face");
        };
        assert!(!face.meanings().is_empty());
        assert_eq!(chosen.marked(), Marked::Face(face.face()));
    }

    #[test]
    fn isolate_is_offered_exactly_when_something_else_is_still_drawn() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );

        // Nothing chosen: nothing to isolate to.
        assert!(!can_isolate_selection(&scene, &picture));

        // Three drawn, one chosen: two others to remove.
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        assert!(can_isolate_selection(&scene, &picture));

        // Two drawn: one other to remove.
        assert!(scene.visibility.hide(
            Marked::Definition(picture.pick_of(0).expect("drawn")),
            &picture
        ));
        assert!(can_isolate_selection(&scene, &picture));

        // One drawn: the chosen one is alone already.
        assert!(scene.visibility.hide(
            Marked::Definition(picture.pick_of(2).expect("drawn")),
            &picture
        ));
        assert!(!can_isolate_selection(&scene, &picture));

        // And a choice that is not drawn at all offers neither action.
        scene.selection = Selection::Definition(picture.pick_of(0).expect("drawn"));
        assert!(!can_isolate_selection(&scene, &picture));
        assert!(!can_hide_selection(&scene, &picture));
    }

    #[test]
    fn a_definition_that_draws_nothing_does_not_make_isolate_available() {
        let mut builder = SnapshotBuilder::new();
        let drawn = builder.add_mesh(&distant_scene_mesh()).expect("packs");
        let empty = builder
            .add_mesh(&ferritecad_kernel::Mesh::default())
            .expect("packs");
        for definition in [drawn, empty] {
            builder
                .place(
                    definition,
                    None,
                    &ferritecad_types::Transform::IDENTITY,
                    [1.0, 1.0, 1.0],
                )
                .expect("places");
        }
        let picture = builder.build();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(drawn).expect("has a row"));
        let mut input = ViewportInput::new();
        let _ = input.take_redraw();

        // The empty one is already nowhere, so the drawn one is alone on
        // screen and there is nothing to isolate away.
        assert!(!can_isolate_selection(&scene, &picture));
        assert!(!isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        assert!(
            !scene.visibility.anything_hidden(),
            "an empty definition was marked hidden by isolating"
        );
        assert!(!input.take_redraw());
    }

    #[test]
    fn what_isolating_leaves_is_what_both_framings_find() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));

        // One thing left, so showing what is chosen and showing everything are
        // the same journey.
        assert_eq!(
            selection_bounds(&scene, &picture),
            scene.visibility.bounds(&picture)
        );
        let mut by_selection = input.clone();
        let mut by_scene = input.clone();
        assert!(frame_selection(&scene, &picture, &mut by_selection).expect("frames"));
        assert!(frame_scene(&scene.visibility, &picture, &mut by_scene).expect("frames"));
        assert_eq!(
            by_selection.camera().view_projection(),
            by_scene.camera().view_projection()
        );
    }

    #[test]
    fn showing_everything_after_isolating_keeps_the_choice_and_hiding_still_clears_it() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let chosen = scene.selection.clone();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));

        // Show all is the way back, and it is not a way to unchoose.
        assert!(show_all(
            &mut scene.visibility,
            &mut scene.hovered,
            &mut input
        ));
        for definition in 0..3 {
            assert!(scene.visibility.shows(definition, &picture));
        }
        assert_eq!(scene.selection, chosen, "showing everything unchose it");

        // And hiding what is chosen still removes it and unchooses it, exactly
        // as it did before this operation existed.
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        assert!(hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.visibility.bounds(&picture), None);
    }

    #[test]
    fn showing_everything_forgets_questions_about_the_isolated_frame() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let chosen = scene.selection.clone();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        let _ = input.take_redraw();

        // A mark, click and question all belong to the isolated frame. Show
        // all will put geometry under pixels where none existed when these
        // were recorded, so none may be answered against the next frame.
        scene.hovered = Hovered::Definition(picture.pick_of(1).expect("drawn"));
        input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);
        let mut proof = input.clone();
        assert!(
            proof.take_pick().is_some(),
            "the gate needs a pending click"
        );
        assert!(
            matches!(proof.take_hover(), Hover::At { .. }),
            "the gate needs a pending hover question"
        );

        assert!(show_all(
            &mut scene.visibility,
            &mut scene.hovered,
            &mut input
        ));
        assert_eq!(scene.selection, chosen, "Show all changed the choice");
        assert_eq!(
            scene.hovered,
            Hovered::Nothing,
            "a hover from the isolated frame survived Show all"
        );
        assert_eq!(
            input.take_pick(),
            None,
            "a click from the isolated frame survived Show all"
        );
        assert_eq!(input.take_hover(), Hover::Cleared);
        assert!(input.take_redraw(), "Show all owes the replacement frame");

        // A gesture begun against the isolated frame ends too. If it
        // survived, the next move would pan the newly complete picture even
        // though the press began while that picture did not exist.
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        let _ = input.take_redraw();
        input.handle(ViewportEvent::PointerMoved { x: 20.0, y: 20.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Middle), false);
        let camera = input.camera().view_projection();
        assert!(show_all(
            &mut scene.visibility,
            &mut scene.hovered,
            &mut input
        ));
        input.handle(ViewportEvent::PointerMoved { x: 80.0, y: 80.0 }, false);
        assert_eq!(
            input.camera().view_projection(),
            camera,
            "a gesture from the isolated frame survived Show all"
        );
    }

    #[test]
    fn a_successful_open_forgets_an_isolation_and_a_failed_one_keeps_it() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        let isolated = scene.visibility.clone();
        let chosen = scene.selection.clone();

        let mut camera = ViewportInput::new();
        commit_scene(&mut scene, &mut camera, Err(CadError::input("no")))
            .expect_err("a failed load commits nothing");
        assert_eq!(scene.visibility, isolated);
        assert_eq!(scene.selection, chosen);

        let next = three_definitions();
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut scene,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: vec![a_body(), a_body(), a_body()],
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::new(&next),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");
        assert!(!scene.visibility.anything_hidden());
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.hovered, Hovered::Nothing);
    }

    #[test]
    fn every_shortcut_reaches_its_own_action_and_yields_to_the_interface() {
        // Each key, the action it asks for, and nothing else.
        for (shortcut, action) in [
            (FRAME_KEY, Requested::FrameSelection),
            (FRAME_ALL_KEY, Requested::FrameScene),
            (HIDE_KEY, Requested::Hide),
            (ISOLATE_KEY, Requested::Isolate),
            (SHOW_ALL_KEY, Requested::ShowAll),
        ] {
            assert_eq!(
                requested(&Key::Character(shortcut.into()), false),
                Some(action),
                "{shortcut} does not reach {action:?}"
            );
            assert_eq!(
                requested(&Key::Character(shortcut.to_lowercase().into()), false),
                Some(action),
                "{shortcut} is case-sensitive"
            );
            // The interface has first refusal for every one of them: a focused
            // text control that accepted the letter did not also ask to change
            // what the model shows.
            assert_eq!(
                requested(&Key::Character(shortcut.into()), true),
                None,
                "{shortcut} fired although the interface had claimed it"
            );
        }

        // And nothing else asks for any of them.
        for (_, _, view) in VIEWS {
            assert_eq!(requested(&Key::Character((*view).into()), false), None);
        }
        assert_eq!(requested(&Key::Named(NamedKey::Home), false), None);
        assert_eq!(requested(&Key::Character("g".into()), false), None);
    }

    #[test]
    fn the_key_that_isolates_is_the_one_the_panel_prints() {
        assert!(wants(
            &Key::Character(ISOLATE_KEY.into()),
            false,
            ISOLATE_KEY
        ));
        assert!(wants(
            &Key::Character(ISOLATE_KEY.to_lowercase().into()),
            false,
            ISOLATE_KEY
        ));

        // Distinct from every other action bound here, and from every view.
        let bound = [
            FRAME_KEY,
            FRAME_ALL_KEY,
            HIDE_KEY,
            SHOW_ALL_KEY,
            ISOLATE_KEY,
        ];
        for (first, one) in bound.iter().enumerate() {
            for other in bound.iter().skip(first + 1) {
                assert_ne!(one, other, "two actions share one key");
            }
        }
        assert!(VIEWS.iter().all(|(_, _, view)| *view != ISOLATE_KEY));
        assert!(named_view(&Key::Character(ISOLATE_KEY.into())).is_none());
        assert!(!wants(&Key::Named(NamedKey::Home), false, ISOLATE_KEY));

        // And the interface has first refusal, as it does for the others.
        assert!(!wants(
            &Key::Character(ISOLATE_KEY.into()),
            true,
            ISOLATE_KEY
        ));
    }

    #[test]
    fn showing_one_definition_keeps_the_choice_and_forgets_the_old_frame() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let chosen = scene.selection.clone();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        let camera = input.camera().view_projection();

        // A pointer question, a click and a gesture, all recorded while the
        // definition about to return was absent.
        scene.hovered = Hovered::Definition(picture.pick_of(1).expect("drawn"));
        input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);
        input.handle(
            ViewportEvent::PointerPressed(PointerButton::Secondary),
            false,
        );
        assert!(input.is_dragging(), "the gate needs a gesture under way");
        let _ = input.take_redraw();

        assert!(show_one(
            &mut scene.visibility,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut input
        ));

        // Returning geometry changes what some pixels mean, so nothing
        // recorded against the frame before it may be answered afterwards.
        assert_eq!(scene.hovered, Hovered::Nothing);
        assert_eq!(input.take_pick(), None, "a click survived the change");
        assert_eq!(input.take_hover(), Hover::Cleared);
        assert!(!input.is_dragging(), "a gesture survived the change");
        assert!(input.take_redraw(), "showing a definition owes a frame");

        // And what is chosen, and where the camera is, are not this
        // operation's business.
        assert_eq!(scene.selection, chosen);
        assert_eq!(input.camera().view_projection(), camera);
    }

    #[test]
    fn showing_one_definition_keeps_a_chosen_face_exactly() {
        let (_directory, scene) = plate_and_a_second_body();
        let snapshot = &scene.snapshot;
        let face = snapshot.face_of(0, 0).expect("numbered");
        let chosen = Selection::at(
            snapshot.pick_of(0).expect("drawn"),
            face,
            EdgePickId::NOTHING,
            ferritecad_viewport::VertexPickId::NOTHING,
            snapshot,
            &scene.faces,
            &EdgeNames::default(),
            &ferritecad_scene::VertexNames::default(),
        );
        let Selection::Face(before) = &chosen else {
            panic!("the plate's face is not named: {chosen:?}");
        };
        let meanings = before.meanings().to_vec();

        let mut visibility = Visibility::new(snapshot);
        let mut hovered = Hovered::Nothing;
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(isolate_selected(
            &mut visibility,
            &chosen,
            &mut hovered,
            snapshot,
            &mut input
        ));

        assert!(show_one(
            &mut visibility,
            &mut hovered,
            snapshot,
            snapshot.pick_of(1).expect("drawn"),
            &mut input
        ));

        // Both drawn again, and the choice is still that face, with what the
        // document calls it.
        assert!(visibility.shows(0, snapshot) && visibility.shows(1, snapshot));
        let Selection::Face(after) = &chosen else {
            panic!("the choice stopped being a face");
        };
        assert_eq!(after.face(), face);
        assert_eq!(after.meanings(), meanings.as_slice());
    }

    #[test]
    fn asking_a_definition_back_that_is_already_there_changes_nothing() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let before = scene.visibility.clone();

        // A no-op must preserve real transient state, not merely turn one
        // empty state into another. Record a mark, click and hover question
        // that still belong to this unchanged frame.
        scene.hovered = Hovered::Definition(picture.pick_of(1).expect("drawn"));
        let hovered = scene.hovered;
        input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);
        let camera = input.camera().view_projection();
        let _ = input.take_redraw();

        // Already drawn, and from another picture entirely: neither is a
        // change, and neither owes a frame.
        let elsewhere = three_definitions();
        for requested in [
            picture.pick_of(0).expect("drawn"),
            PickId::NOTHING,
            elsewhere.pick_of(0).expect("drawn"),
        ] {
            assert!(!show_one(
                &mut scene.visibility,
                &mut scene.hovered,
                &picture,
                requested,
                &mut input
            ));
        }
        assert_eq!(scene.visibility, before);
        assert_eq!(scene.hovered, hovered, "a no-op cleared the current hover");
        assert_eq!(
            input.take_pick(),
            Some((4.0, 4.0)),
            "a no-op cleared a pending click"
        );
        assert_eq!(
            input.take_hover(),
            Hover::At(9.0, 9.0),
            "a no-op cleared a pending hover question"
        );
        assert_eq!(
            input.camera().view_projection(),
            camera,
            "a no-op moved the camera"
        );
        assert!(
            !input.take_redraw(),
            "an action that did nothing asked for a frame"
        );

        // A gesture belongs to the same unchanged frame too. Use a separate
        // reducer because beginning a gesture deliberately clears a hover
        // question, and this gate needs to prove both states independently.
        let mut gesture = ViewportInput::new();
        gesture.resize(800, 600);
        gesture.handle(ViewportEvent::PointerMoved { x: 20.0, y: 20.0 }, false);
        gesture.handle(
            ViewportEvent::PointerPressed(PointerButton::Secondary),
            false,
        );
        let gesture_camera = gesture.camera().view_projection();
        let _ = gesture.take_redraw();
        assert!(!show_one(
            &mut scene.visibility,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut gesture
        ));
        assert!(gesture.is_dragging(), "a no-op cancelled an active gesture");
        assert_eq!(gesture.camera().view_projection(), gesture_camera);
        assert!(!gesture.take_redraw(), "a no-op asked for a frame");
    }

    #[test]
    fn which_rows_offer_a_way_back_is_exactly_which_are_hidden_and_drawn() {
        let mut builder = SnapshotBuilder::new();
        let drawn = builder.add_mesh(&distant_scene_mesh()).expect("packs");
        let other = builder.add_mesh(&distant_scene_mesh()).expect("packs");
        let empty = builder
            .add_mesh(&ferritecad_kernel::Mesh::default())
            .expect("packs");
        for definition in [drawn, other, empty] {
            builder
                .place(
                    definition,
                    None,
                    &ferritecad_types::Transform::from_translation(
                        ferritecad_types::Vec3::new(definition as f64 * 40.0, 0.0, 0.0)
                            .expect("finite"),
                    )
                    .expect("finite"),
                    [1.0, 1.0, 1.0],
                )
                .expect("places");
        }
        let picture = builder.build();
        let mut visibility = Visibility::new(&picture);

        // Nothing hidden: every drawn row offers to go, and the row that
        // draws nothing wherever it is offers neither.
        assert_eq!(
            rows_visibility(&visibility, &picture),
            vec![
                RowVisibility::Hide(picture.pick_of(drawn).expect("drawn")),
                RowVisibility::Hide(picture.pick_of(other).expect("drawn")),
                RowVisibility::Neither,
            ]
        );

        assert!(visibility.hide(
            Marked::Definition(picture.pick_of(other).expect("drawn")),
            &picture
        ));
        assert_eq!(
            rows_visibility(&visibility, &picture),
            vec![
                RowVisibility::Hide(picture.pick_of(drawn).expect("drawn")),
                RowVisibility::Show(picture.pick_of(other).expect("drawn")),
                RowVisibility::Neither,
            ],
            "the way back is offered for the hidden row and the way out for the drawn one"
        );
    }

    #[test]
    fn a_successful_open_forgets_a_partly_shown_scene_and_a_failed_one_keeps_it() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        assert!(show_one(
            &mut scene.visibility,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut input
        ));
        let partly = scene.visibility.clone();
        let chosen = scene.selection.clone();

        let mut camera = ViewportInput::new();
        commit_scene(&mut scene, &mut camera, Err(CadError::input("no")))
            .expect_err("a failed load commits nothing");
        assert_eq!(scene.visibility, partly);
        assert_eq!(scene.selection, chosen);

        let next = three_definitions();
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut scene,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: vec![a_body(), a_body(), a_body()],
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::new(&next),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");
        assert!(!scene.visibility.anything_hidden());
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.hovered, Hovered::Nothing);
    }

    #[test]
    fn hiding_a_row_keeps_a_chosen_face_on_another_definition_exactly() {
        let (_directory, scene) = plate_and_a_second_body();
        let snapshot = &scene.snapshot;
        let face = snapshot.face_of(0, 0).expect("numbered");
        let mut chosen = Selection::at(
            snapshot.pick_of(0).expect("drawn"),
            face,
            EdgePickId::NOTHING,
            ferritecad_viewport::VertexPickId::NOTHING,
            snapshot,
            &scene.faces,
            &EdgeNames::default(),
            &ferritecad_scene::VertexNames::default(),
        );
        let Selection::Face(before) = &chosen else {
            panic!("the plate's face is not named: {chosen:?}");
        };
        let meanings = before.meanings().to_vec();

        let mut visibility = Visibility::new(snapshot);
        let mut hovered = Hovered::Nothing;
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        // The other body goes; the chosen face is on the plate.
        assert!(hide_one(
            &mut visibility,
            &mut chosen,
            &mut hovered,
            snapshot,
            snapshot.pick_of(1).expect("drawn"),
            &mut input
        ));
        assert!(visibility.shows(0, snapshot) && !visibility.shows(1, snapshot));

        let Selection::Face(after) = &chosen else {
            panic!("the choice stopped being a face: {chosen:?}");
        };
        assert_eq!(after.face(), face);
        assert_eq!(
            after.meanings(),
            meanings.as_slice(),
            "what the document calls the face was lost"
        );

        // And hiding the plate itself, which the face is on, does unchoose it:
        // a face nobody can see cannot stay chosen.
        assert!(hide_one(
            &mut visibility,
            &mut chosen,
            &mut hovered,
            snapshot,
            snapshot.pick_of(0).expect("drawn"),
            &mut input
        ));
        assert_eq!(chosen, Selection::Nothing);
    }

    #[test]
    fn hiding_a_row_forgets_the_frame_it_was_asked_from() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let chosen = scene.selection.clone();
        scene.hovered = Hovered::Definition(picture.pick_of(2).expect("drawn"));
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let camera = input.camera().view_projection();

        // A click and a question recorded against the frame that still had
        // the definition about to go.
        input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);
        let _ = input.take_redraw();

        assert!(hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut input
        ));

        assert_eq!(scene.hovered, Hovered::Nothing);
        assert_eq!(input.take_pick(), None, "a click survived the change");
        assert_eq!(input.take_hover(), Hover::Cleared);
        assert!(input.take_redraw(), "hiding a row owes a frame");
        assert_eq!(
            scene.selection, chosen,
            "hiding a neighbour changed the choice"
        );
        assert_eq!(input.camera().view_projection(), camera);

        // A gesture belongs to that frame too, and is cancelled with it. A
        // separate reducer, because beginning a gesture clears a hover
        // question and this proves the two independently.
        let mut gesture = ViewportInput::new();
        gesture.resize(800, 600);
        gesture.handle(ViewportEvent::PointerMoved { x: 20.0, y: 20.0 }, false);
        gesture.handle(
            ViewportEvent::PointerPressed(PointerButton::Secondary),
            false,
        );
        assert!(gesture.is_dragging());
        assert!(hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            picture.pick_of(2).expect("drawn"),
            &mut gesture
        ));
        assert!(!gesture.is_dragging(), "a gesture survived the change");
    }

    #[test]
    fn a_row_hide_that_does_nothing_leaves_everything_as_it_was() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let chosen = scene.selection.clone();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut input
        ));
        let before = scene.visibility.clone();

        // Real transient state belonging to this unchanged frame, not the
        // absence of any.
        scene.hovered = Hovered::Definition(picture.pick_of(2).expect("drawn"));
        let hovered = scene.hovered;
        input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);
        let camera = input.camera().view_projection();
        let _ = input.take_redraw();

        // Already hidden, nothing at all, and a definition of a genuinely
        // different picture: one with the same geometry would be the same
        // picture by content, and its identities would rightly resolve here.
        let elsewhere = two_definitions();
        assert_ne!(
            elsewhere.pick_of(1).expect("drawn"),
            picture.pick_of(1).expect("drawn"),
            "the gate needs a pick that really belongs to another picture"
        );
        for requested in [
            picture.pick_of(0).expect("drawn"),
            PickId::NOTHING,
            elsewhere.pick_of(1).expect("drawn"),
        ] {
            assert!(
                !hide_one(
                    &mut scene.visibility,
                    &mut scene.selection,
                    &mut scene.hovered,
                    &picture,
                    requested,
                    &mut input
                ),
                "{requested:?} claimed to change something"
            );
        }

        assert_eq!(scene.visibility, before);
        assert_eq!(scene.selection, chosen, "a no-op unchose something");
        assert_eq!(scene.hovered, hovered, "a no-op cleared the current hover");
        assert_eq!(
            input.take_pick(),
            Some((4.0, 4.0)),
            "a no-op cleared a pending click"
        );
        assert_eq!(
            input.take_hover(),
            Hover::At(9.0, 9.0),
            "a no-op cleared a pending hover question"
        );
        assert_eq!(
            input.camera().view_projection(),
            camera,
            "a no-op moved the camera"
        );
        assert!(!input.take_redraw(), "a no-op asked for a frame");

        // And a gesture survives a no-op too.
        let mut gesture = ViewportInput::new();
        gesture.resize(800, 600);
        gesture.handle(ViewportEvent::PointerMoved { x: 20.0, y: 20.0 }, false);
        gesture.handle(
            ViewportEvent::PointerPressed(PointerButton::Secondary),
            false,
        );
        let _ = gesture.take_redraw();
        assert!(!hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut gesture
        ));
        assert!(gesture.is_dragging(), "a no-op cancelled an active gesture");
        assert!(!gesture.take_redraw());
    }

    #[test]
    fn a_row_hide_survives_a_failed_open_and_not_a_successful_one() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut input
        ));
        let mask = scene.visibility.clone();
        let chosen = scene.selection.clone();

        let mut camera = ViewportInput::new();
        commit_scene(&mut scene, &mut camera, Err(CadError::input("no")))
            .expect_err("a failed load commits nothing");
        assert_eq!(scene.visibility, mask);
        assert_eq!(scene.selection, chosen);

        let next = three_definitions();
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut scene,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: vec![a_body(), a_body(), a_body()],
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::new(&next),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");
        assert!(!scene.visibility.anything_hidden());
        assert_eq!(scene.selection, Selection::Nothing);
    }

    #[test]
    fn all_five_ways_of_changing_what_is_drawn_can_be_taken_back() {
        let picture = three_definitions();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let chosen = Selection::Definition(picture.pick_of(1).expect("drawn"));

        // Each entry point in turn, from a fresh arrangement, and each taken
        // back to exactly what it replaced. One rule, five doors into it.
        for door in 0..5 {
            let mut visibility = Visibility::new(&picture);
            let mut selection = chosen.clone();
            let mut hovered = Hovered::Nothing;
            if door >= 3 {
                // Show one and Show all need something already missing.
                assert!(hide_one(
                    &mut visibility,
                    &mut Selection::Nothing,
                    &mut Hovered::Nothing,
                    &picture,
                    picture.pick_of(0).expect("drawn"),
                    &mut input
                ));
            }
            let before = visibility.hidden_in(&picture).to_vec();

            let happened = match door {
                0 => hide_selected(
                    &mut visibility,
                    &mut selection,
                    &mut hovered,
                    &picture,
                    &mut input,
                ),
                1 => hide_one(
                    &mut visibility,
                    &mut selection,
                    &mut hovered,
                    &picture,
                    picture.pick_of(2).expect("drawn"),
                    &mut input,
                ),
                2 => isolate_selected(
                    &mut visibility,
                    &selection,
                    &mut hovered,
                    &picture,
                    &mut input,
                ),
                3 => show_one(
                    &mut visibility,
                    &mut hovered,
                    &picture,
                    picture.pick_of(0).expect("drawn"),
                    &mut input,
                ),
                _ => show_all(&mut visibility, &mut hovered, &mut input),
            };
            assert!(
                happened,
                "door {door} did nothing, so the gate proves nothing"
            );
            assert_ne!(visibility.hidden_in(&picture), before.as_slice());

            assert!(
                undo_visibility(
                    &mut visibility,
                    &mut selection,
                    &mut hovered,
                    &picture,
                    &mut input
                ),
                "door {door} left nothing to take back"
            );
            assert_eq!(
                visibility.hidden_in(&picture),
                before.as_slice(),
                "door {door} was not taken back to what it replaced"
            );
        }
    }

    #[test]
    fn taking_back_a_change_does_not_put_back_what_it_unchose() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        // Hiding what is chosen unchooses it. Taking the change back puts the
        // geometry on screen; it does not decide that it is what the user is
        // working on again.
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        assert!(hide_selected(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        assert_eq!(scene.selection, Selection::Nothing);

        assert!(undo_visibility(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        assert!(scene.visibility.shows(1, &picture));
        assert_eq!(
            scene.selection,
            Selection::Nothing,
            "taking back a change resurrected a choice it had cleared"
        );

        // And a choice made after the change is cleared if taking the change
        // back puts its definition away again. Everything is drawn at this
        // point, so the arrangement is built from there.
        assert!(hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut input
        ));
        assert!(show_one(
            &mut scene.visibility,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut input
        ));
        scene.selection = Selection::Definition(picture.pick_of(0).expect("drawn"));
        assert!(undo_visibility(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        assert!(!scene.visibility.shows(0, &picture));
        assert_eq!(
            scene.selection,
            Selection::Nothing,
            "a choice on geometry that went back off screen stayed chosen"
        );
    }

    #[test]
    fn taking_back_a_change_forgets_the_frame_and_leaves_the_camera() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut input
        ));
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let chosen = scene.selection.clone();
        scene.hovered = Hovered::Definition(picture.pick_of(2).expect("drawn"));
        let camera = input.camera().view_projection();

        input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);
        let _ = input.take_redraw();

        assert!(undo_visibility(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));

        assert_eq!(scene.hovered, Hovered::Nothing);
        assert_eq!(input.take_pick(), None, "a click survived the change");
        assert_eq!(input.take_hover(), Hover::Cleared);
        assert!(input.take_redraw(), "taking a change back owes a frame");
        assert_eq!(
            scene.selection, chosen,
            "a choice still drawn was disturbed"
        );
        assert_eq!(input.camera().view_projection(), camera);

        // A gesture belongs to that frame too. A separate reducer, because
        // beginning a gesture clears a hover question.
        let mut gesture = ViewportInput::new();
        gesture.resize(800, 600);
        gesture.handle(ViewportEvent::PointerMoved { x: 20.0, y: 20.0 }, false);
        gesture.handle(
            ViewportEvent::PointerPressed(PointerButton::Secondary),
            false,
        );
        assert!(gesture.is_dragging());
        assert!(hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            picture.pick_of(2).expect("drawn"),
            &mut gesture
        ));
        assert!(undo_visibility(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut gesture
        ));
        assert!(!gesture.is_dragging(), "a gesture survived the change");
    }

    #[test]
    fn an_undo_with_nothing_to_take_back_changes_absolutely_nothing() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let chosen = scene.selection.clone();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let before = scene.visibility.clone();

        // Real transient state belonging to this unchanged frame, not the
        // absence of any, and a frame already owed.
        scene.hovered = Hovered::Definition(picture.pick_of(2).expect("drawn"));
        let hovered = scene.hovered;
        input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);
        let camera = input.camera().view_projection();

        assert!(!scene.visibility.can_undo(&picture));
        assert!(!undo_visibility(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));

        assert_eq!(scene.visibility, before);
        assert_eq!(scene.selection, chosen);
        assert_eq!(scene.hovered, hovered, "a no-op cleared the current hover");
        assert_eq!(
            input.take_pick(),
            Some((4.0, 4.0)),
            "a no-op cleared a pending click"
        );
        assert_eq!(
            input.take_hover(),
            Hover::At(9.0, 9.0),
            "a no-op cleared a pending hover question"
        );
        assert_eq!(
            input.camera().view_projection(),
            camera,
            "a no-op moved the camera"
        );
        assert!(
            input.take_redraw(),
            "a no-op threw away a frame that was already owed"
        );

        // And a gesture survives it too.
        let mut gesture = ViewportInput::new();
        gesture.resize(800, 600);
        gesture.handle(ViewportEvent::PointerMoved { x: 20.0, y: 20.0 }, false);
        gesture.handle(
            ViewportEvent::PointerPressed(PointerButton::Secondary),
            false,
        );
        let _ = gesture.take_redraw();
        assert!(!undo_visibility(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            &mut gesture
        ));
        assert!(gesture.is_dragging(), "a no-op cancelled an active gesture");
        assert!(!gesture.take_redraw(), "a no-op asked for a frame");
    }

    #[test]
    fn a_document_opens_with_nothing_to_take_back_and_a_failed_open_keeps_it() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        assert!(hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            picture.pick_of(0).expect("drawn"),
            &mut input
        ));
        let mask = scene.visibility.clone();
        let chosen = scene.selection.clone();
        assert!(scene.visibility.can_undo(&picture));

        let mut camera = ViewportInput::new();
        commit_scene(&mut scene, &mut camera, Err(CadError::input("no")))
            .expect_err("a failed load commits nothing");
        assert_eq!(
            scene.visibility, mask,
            "a failed load disturbed the mask or its record"
        );
        assert_eq!(scene.selection, chosen);
        assert!(
            scene.visibility.can_undo(&picture),
            "a failed load threw away what could be taken back"
        );

        let next = three_definitions();
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut scene,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: vec![a_body(), a_body(), a_body()],
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::new(&next),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");
        assert!(!scene.visibility.anything_hidden());
        assert!(
            !scene.visibility.can_undo(&next),
            "a document opened with a change from the last one to take back"
        );
        assert_eq!(scene.selection, Selection::Nothing);
    }

    #[test]
    fn swapping_projection_keeps_the_choice_the_parts_and_the_view() {
        // The committed plate, so the choice is a face the document really
        // names, beside a second body so there is a visibility mask worth
        // preserving. Native bodies enter this loader once each; repeated
        // placements are the GPU gates' business.
        let (_directory, scene) = plate_and_a_second_body();
        let snapshot = &scene.snapshot;
        let face = snapshot.face_of(0, 0).expect("numbered");
        let mut chosen = Selection::at(
            snapshot.pick_of(0).expect("drawn"),
            face,
            EdgePickId::NOTHING,
            ferritecad_viewport::VertexPickId::NOTHING,
            snapshot,
            &scene.faces,
            &EdgeNames::default(),
            &ferritecad_scene::VertexNames::default(),
        );
        let Selection::Face(before) = &chosen else {
            panic!("the plate's face is not named: {chosen:?}");
        };
        let meanings = before.meanings().to_vec();

        let mut visibility = Visibility::new(snapshot);
        let mut hovered = Hovered::Nothing;
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(snapshot.bounds().expect("an extent"))
            .expect("frames");

        // An arrangement with something hidden and something to take back.
        assert!(hide_one(
            &mut visibility,
            &mut chosen,
            &mut hovered,
            snapshot,
            snapshot.pick_of(1).expect("drawn"),
            &mut input
        ));
        let mask = visibility.clone();
        assert!(visibility.can_undo(snapshot));

        // Real pointing state belonging to the frame about to be replaced.
        hovered = Hovered::Definition(snapshot.pick_of(0).expect("drawn"));
        input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);
        let _ = input.take_redraw();
        let (target, eye) = (input.camera().target(), input.camera().eye());

        assert!(change_projection(
            &mut input,
            &mut hovered,
            Projection::Orthographic
        ));

        // What is chosen is untouched, down to the face and what the document
        // calls it.
        let Selection::Face(after) = &chosen else {
            panic!("changing projection unchose the face: {chosen:?}");
        };
        assert_eq!(after.face(), face);
        assert_eq!(after.meanings(), meanings.as_slice());

        // So is what is drawn, and what could be put back.
        assert_eq!(visibility, mask, "changing projection disturbed the mask");
        assert!(
            visibility.can_undo(snapshot),
            "changing projection threw away what could be taken back"
        );

        // What was being looked at, and from where, are kept; what was
        // pointing at the old frame is not.
        assert_eq!(input.camera().target(), target);
        assert_eq!(input.camera().eye(), eye);
        assert_eq!(input.projection(), Projection::Orthographic);
        assert_eq!(hovered, Hovered::Nothing);
        assert_eq!(input.take_pick(), None, "a click survived the change");
        assert_eq!(input.take_hover(), Hover::Cleared);
        assert!(input.take_redraw(), "changing projection owes a frame");

        // A gesture belongs to that frame too. A separate reducer, because
        // beginning one clears a hover question.
        let mut gesture = ViewportInput::new();
        gesture.resize(800, 600);
        gesture.handle(ViewportEvent::PointerMoved { x: 20.0, y: 20.0 }, false);
        gesture.handle(
            ViewportEvent::PointerPressed(PointerButton::Secondary),
            false,
        );
        assert!(gesture.is_dragging());
        assert!(change_projection(
            &mut gesture,
            &mut hovered,
            Projection::Orthographic
        ));
        assert!(!gesture.is_dragging(), "a gesture survived the change");
    }

    #[test]
    fn asking_for_the_projection_already_in_use_changes_nothing() {
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(([-10.0, -10.0, -10.0], [10.0, 10.0, 10.0]))
            .expect("frames");
        let mut hovered = Hovered::Definition(distant_scene().pick_of(0).expect("drawn"));
        let recorded = hovered;

        // Real transient state belonging to this unchanged frame.
        input.handle(ViewportEvent::PointerMoved { x: 4.0, y: 4.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        input.handle(ViewportEvent::PointerMoved { x: 9.0, y: 9.0 }, false);
        let camera = input.camera().view_projection();
        let _ = input.take_redraw();

        assert!(!change_projection(
            &mut input,
            &mut hovered,
            Projection::Perspective
        ));

        assert_eq!(hovered, recorded, "a no-op cleared the current hover");
        assert_eq!(
            input.take_pick(),
            Some((4.0, 4.0)),
            "a no-op cleared a pending click"
        );
        assert_eq!(
            input.take_hover(),
            Hover::At(9.0, 9.0),
            "a no-op cleared a pending hover question"
        );
        assert_eq!(input.camera().view_projection(), camera);
        assert!(!input.take_redraw(), "a no-op asked for a frame");
    }

    #[test]
    fn a_document_opens_as_an_eye_sees_it_and_a_failed_open_keeps_the_drawing() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(picture.bounds().expect("an extent"))
            .expect("frames");
        assert!(change_projection(
            &mut input,
            &mut scene.hovered,
            Projection::Orthographic
        ));

        // A load that failed leaves the projection where the user put it.
        let mut camera = input.clone();
        commit_scene(&mut scene, &mut camera, Err(CadError::input("no")))
            .expect_err("a failed load commits nothing");
        assert_eq!(
            camera.projection(),
            Projection::Orthographic,
            "a failed load changed how the model is drawn"
        );

        // A load that arrived starts the way a new camera starts, because a
        // document is opened to be understood before it is measured.
        let next = three_definitions();
        let arriving = prepare_load(&input, Ok(loaded(next.clone())), |_| Ok(()))
            .expect("the picture is accepted");
        assert_eq!(
            arriving.framed.projection(),
            Projection::Perspective,
            "a document opened in the projection the last one was left in"
        );
    }

    #[test]
    fn the_key_that_changes_projection_is_the_one_the_panel_prints() {
        assert!(wants(
            &Key::Character(PROJECTION_KEY.into()),
            false,
            PROJECTION_KEY
        ));
        assert_eq!(
            requested(&Key::Character(PROJECTION_KEY.into()), false),
            Some(Requested::Projection),
            "the printed key does not reach the projection"
        );
        assert_eq!(
            requested(&Key::Character(PROJECTION_KEY.to_lowercase().into()), false),
            Some(Requested::Projection)
        );

        // Distinct from every other action bound here, and from every view.
        let bound = [
            FRAME_KEY,
            FRAME_ALL_KEY,
            HIDE_KEY,
            SHOW_ALL_KEY,
            ISOLATE_KEY,
            PROJECTION_KEY,
        ];
        for (first, one) in bound.iter().enumerate() {
            for other in bound.iter().skip(first + 1) {
                assert_ne!(one, other, "two actions share one key");
            }
        }
        assert!(VIEWS.iter().all(|(_, _, view)| *view != PROJECTION_KEY));
        assert!(named_view(&Key::Character(PROJECTION_KEY.into())).is_none());

        // And the interface has first refusal, as it does for the others.
        assert_eq!(
            requested(&Key::Character(PROJECTION_KEY.into()), true),
            None,
            "the projection key fired although the interface had claimed it"
        );
    }

    #[test]
    fn the_other_projection_is_the_one_that_is_not_in_use() {
        assert_eq!(
            other_projection(Projection::Perspective),
            Projection::Orthographic
        );
        assert_eq!(
            other_projection(Projection::Orthographic),
            Projection::Perspective
        );
    }

    #[test]
    fn undo_restores_the_arrangement_one_accidental_hide_destroyed() {
        // Four definitions: the plate, whose faces the document names, and
        // three others. Native bodies currently enter this loader once each;
        // repeated placements are exercised by the GPU gate built from
        // `three_plates`, while this gate owns the real named face and panel.
        let (_directory, scene) = plate_and_more_bodies(3);
        let snapshot = &scene.snapshot;
        assert_eq!(snapshot.meshes().len(), 4);
        assert_eq!(
            snapshot.draws().len(),
            4,
            "the native loader unexpectedly made occurrences"
        );
        for definition in 0..4 {
            assert_eq!(
                snapshot
                    .draws()
                    .iter()
                    .filter(|item| item.mesh == definition)
                    .count(),
                1,
                "native definition {definition} is not placed exactly once"
            );
            assert!(
                snapshot
                    .pick_of(definition)
                    .is_some_and(|pick| snapshot.bounds_of(pick).is_some()),
                "definition {definition} draws nothing"
            );
        }

        let mut visibility = Visibility::new(snapshot);
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        // An arrangement worth keeping: two of the four already out of the
        // way, chosen one at a time.
        for definition in [2, 3] {
            assert!(hide_one(
                &mut visibility,
                &mut Selection::Nothing,
                &mut Hovered::Nothing,
                snapshot,
                snapshot.pick_of(definition).expect("drawn"),
                &mut input
            ));
        }
        let arrangement = visibility.clone();
        let before = visibility.bounds(snapshot);

        // A face of the plate, chosen as that face, with what the document
        // calls it.
        let face = snapshot.face_of(0, 0).expect("numbered");
        let mut chosen = Selection::at(
            snapshot.pick_of(0).expect("drawn"),
            face,
            EdgePickId::NOTHING,
            ferritecad_viewport::VertexPickId::NOTHING,
            snapshot,
            &scene.faces,
            &EdgeNames::default(),
            &ferritecad_scene::VertexNames::default(),
        );
        let Selection::Face(named) = &chosen else {
            panic!("the plate's face is not named: {chosen:?}");
        };
        let meanings = named.meanings().to_vec();
        let camera = input.camera().view_projection();

        // The accident: Hide pressed on a different row, through the panel
        // that draws it.
        let context = egui::Context::default();
        let identities = identities_of(&scene.catalogue);
        let rows: Vec<Selected<'_>> = scene
            .catalogue
            .iter()
            .zip(&identities)
            .map(|(entry, identity)| describe(entry, identity))
            .collect();
        let offers = rows_visibility(&visibility, snapshot);
        let asked = press_row_control(&context, &rows, None, &offers, 1);
        let requested = match asked.visibility {
            Some(RowVisibility::Hide(pick)) => pick,
            other => panic!("the drawn row offers no way out: {other:?}"),
        };
        let mut hovered = Hovered::Nothing;
        assert!(hide_one(
            &mut visibility,
            &mut chosen,
            &mut hovered,
            snapshot,
            requested,
            &mut input
        ));
        assert_ne!(
            visibility.hidden_in(snapshot),
            arrangement.hidden_in(snapshot),
            "the accident changed nothing"
        );

        // Show all is not the way back: it would return both distractions.
        let mut everything = visibility.clone();
        assert!(everything.show_all());
        assert_ne!(
            everything.hidden_in(snapshot),
            arrangement.hidden_in(snapshot),
            "showing everything is not what was there before"
        );

        // Undo is.
        assert!(undo_visibility(
            &mut visibility,
            &mut chosen,
            &mut hovered,
            snapshot,
            &mut input
        ));

        // Exactly the arrangement that was there, definition by definition,
        // and the same extent it had. Compared by what is drawn rather than by
        // the whole state: the record of how it got there is history, and
        // history is not part of the picture.
        assert_eq!(
            visibility.hidden_in(snapshot),
            arrangement.hidden_in(snapshot)
        );
        for definition in 0..4 {
            assert_eq!(
                visibility.shows(definition, snapshot),
                arrangement.shows(definition, snapshot),
                "definition {definition} came back differently"
            );
        }
        assert_eq!(visibility.bounds(snapshot), before);

        // The chosen face is still chosen, and still that face.
        let Selection::Face(after) = &chosen else {
            panic!("undoing a visibility change unchose the face: {chosen:?}");
        };
        assert_eq!(after.face(), face);
        assert_eq!(after.meanings(), meanings.as_slice());
        assert_eq!(input.camera().view_projection(), camera);
    }

    #[test]
    fn one_visible_neighbour_goes_from_its_row_without_disturbing_the_choice() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        // The middle one, chosen from the list.
        select_definition_row(&mut scene.selection, &picture, &mut input, 1);
        let chosen = scene.selection.clone();
        assert_eq!(
            chosen,
            Selection::Definition(picture.pick_of(1).expect("drawn"))
        );
        let camera = input.camera().view_projection();

        // The defect: the only way to remove a neighbour is to choose it
        // first, and choosing it is exactly what destroys the choice.
        let mut by_choosing = scene.selection.clone();
        let mut elsewhere = ViewportInput::new();
        select_definition_row(&mut by_choosing, &picture, &mut elsewhere, 0);
        assert_ne!(
            by_choosing, chosen,
            "reaching a neighbour through the selection is what loses the choice"
        );

        // One row's Hide control, pressed through the panel that draws it.
        let context = egui::Context::default();
        let identities = identities_of(&scene.catalogue);
        let (definitions, chosen_row) = scene.view(&identities, &picture);
        let offers = rows_visibility(&scene.visibility, &picture);
        assert_eq!(
            offers,
            vec![
                RowVisibility::Hide(picture.pick_of(0).expect("drawn")),
                RowVisibility::Hide(picture.pick_of(1).expect("drawn")),
                RowVisibility::Hide(picture.pick_of(2).expect("drawn")),
            ],
            "every drawn row offers to be taken off screen"
        );
        let asked = press_row_control(&context, &definitions, chosen_row, &offers, 0);
        let requested = match asked.visibility {
            Some(RowVisibility::Hide(pick)) => pick,
            other => panic!("the visible row offers no way to hide it: {other:?}"),
        };
        assert_eq!(requested, picture.pick_of(0).expect("drawn"));
        assert_eq!(asked.pressed, None, "pressing Hide also chose the row");
        assert_eq!(asked.hovered, None, "pressing Hide also pointed at the row");

        assert!(hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            requested,
            &mut input
        ));

        // The one asked for is gone, in every placement; the others are as
        // they were; and what was chosen is still chosen.
        assert!(!scene.visibility.shows(0, &picture));
        assert!(scene.visibility.shows(1, &picture));
        assert!(
            scene.visibility.shows(2, &picture),
            "hiding one row hid another"
        );
        assert_eq!(
            scene.visibility.bounds(&picture),
            {
                let mut without = Visibility::new(&picture);
                assert!(without.hide(
                    Marked::Definition(picture.pick_of(0).expect("drawn")),
                    &picture
                ));
                without.bounds(&picture)
            },
            "what is left is the two definitions still drawn"
        );
        assert_eq!(
            scene.selection, chosen,
            "hiding a neighbour changed the choice"
        );
        assert_eq!(input.camera().view_projection(), camera);
    }

    #[test]
    fn hiding_the_row_that_is_chosen_unchooses_it() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));

        assert!(hide_one(
            &mut scene.visibility,
            &mut scene.selection,
            &mut scene.hovered,
            &picture,
            picture.pick_of(1).expect("drawn"),
            &mut input
        ));

        // Geometry nobody can see cannot remain chosen: an inspector would be
        // describing something that is not on screen.
        assert_eq!(scene.selection, Selection::Nothing);
        assert!(!scene.visibility.shows(1, &picture));
    }

    /// Runs the list once, pressing the visibility control of one row.
    fn press_row_control(
        context: &egui::Context,
        definitions: &[Selected<'_>],
        chosen: Option<usize>,
        offers: &[RowVisibility],
        row: usize,
    ) -> ferritecad_ui::Rows {
        // Found by pressing across the list rather than by assuming where the
        // control sits: a gate that guessed would pass while pressing empty
        // space.
        let wanted = offers[row];
        let mut found = None;
        'search: for step_y in 0..30 {
            for step_x in 0..40 {
                let at = egui::Pos2::new(step_x as f32 * 8.0, step_y as f32 * 5.0);
                if press_rows_at(context, at, definitions, chosen, offers).visibility
                    == Some(wanted)
                {
                    found = Some(at);
                    break 'search;
                }
            }
        }
        let at = found.expect("no point in the list asks that of the row");
        press_rows_at(context, at, definitions, chosen, offers)
    }

    /// One press of the list at one place, with a neutral frame before it.
    fn press_rows_at(
        context: &egui::Context,
        at: egui::Pos2,
        definitions: &[Selected<'_>],
        chosen: Option<usize>,
        offers: &[RowVisibility],
    ) -> ferritecad_ui::Rows {
        let away = egui::Pos2::new(4000.0, 4000.0);
        let mut output = context.run_ui(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(away)],
                ..Default::default()
            },
            |ui| {
                let _ = ferritecad_ui::definitions_panel(ui, definitions, chosen, offers);
            },
        );
        output.textures_delta.clear();

        let mut rows = ferritecad_ui::Rows::default();
        let mut output = context.run_ui(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            },
            |ui| {
                rows = ferritecad_ui::definitions_panel(ui, definitions, chosen, offers);
            },
        );
        output.textures_delta.clear();
        rows
    }

    #[test]
    fn one_hidden_neighbour_comes_back_from_its_row_without_the_other() {
        let picture = three_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        // The middle one, chosen from the list and then left alone on screen.
        select_definition_row(&mut scene.selection, &picture, &mut input, 1);
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));
        let chosen = scene.selection.clone();
        let camera = input.camera().view_projection();
        assert!(!scene.visibility.shows(0, &picture));
        assert!(!scene.visibility.shows(2, &picture));

        // The defect: the only way back is everything at once. Show all would
        // return the distraction along with the part being looked for.
        let mut everything = scene.visibility.clone();
        assert!(everything.show_all());
        assert!(
            everything.shows(0, &picture) && everything.shows(2, &picture),
            "Show all is all or nothing"
        );

        // One row's Show control, pressed through the panel that draws it.
        let context = egui::Context::default();
        let identities = identities_of(&scene.catalogue);
        let (definitions, chosen_row) = scene.view(&identities, &picture);
        let offered = rows_visibility(&scene.visibility, &picture);
        assert_eq!(
            offered,
            vec![
                RowVisibility::Show(picture.pick_of(0).expect("drawn")),
                RowVisibility::Hide(picture.pick_of(1).expect("drawn")),
                RowVisibility::Show(picture.pick_of(2).expect("drawn")),
            ],
            "the hidden rows offer a way back and the drawn one a way out"
        );
        let asked = press_row_control(&context, &definitions, chosen_row, &offered, 0);
        let requested = match asked.visibility {
            Some(RowVisibility::Show(pick)) => pick,
            other => panic!("the hidden row offers no way back: {other:?}"),
        };
        assert_eq!(requested, picture.pick_of(0).expect("drawn"));
        assert_eq!(asked.pressed, None, "pressing Show also chose the row");
        assert_eq!(asked.hovered, None, "pressing Show also pointed at the row");

        assert!(show_one(
            &mut scene.visibility,
            &mut scene.hovered,
            &picture,
            requested,
            &mut input
        ));

        // The one asked for is back, in every placement; the other is not; and
        // what was chosen is untouched.
        assert!(scene.visibility.shows(0, &picture));
        assert!(scene.visibility.shows(1, &picture));
        assert!(
            !scene.visibility.shows(2, &picture),
            "showing one row brought back another"
        );
        assert_eq!(
            scene.selection, chosen,
            "showing a row changed what is chosen"
        );
        assert_eq!(input.camera().view_projection(), camera);
    }

    #[test]
    fn nothing_but_isolate_leaves_one_definition_chosen_and_alone() {
        let picture = three_definitions();
        let target = picture.pick_of(1).expect("the middle definition has a row");
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body(), a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&picture),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        // Chosen the way a person chooses something they cannot see: from the
        // list, because the middle definition is surrounded by its neighbours.
        select_definition_row(&mut scene.selection, &picture, &mut input, 1);
        assert_eq!(scene.selection, Selection::Definition(target));
        let chosen = scene.selection.clone();
        let camera = input.camera().view_projection();

        // The defect. Hiding what is chosen removes the very thing that was
        // to be looked at, and there is no other single operation that leaves
        // it chosen and alone.
        let mut by_hiding = scene.visibility.clone();
        let mut selection = scene.selection.clone();
        let mut hovered = scene.hovered;
        assert!(hide_selected(
            &mut by_hiding,
            &mut selection,
            &mut hovered,
            &picture,
            &mut input.clone()
        ));
        assert!(
            !by_hiding.shows(1, &picture),
            "Hide selected removes the target itself"
        );
        assert_eq!(
            selection,
            Selection::Nothing,
            "and unchooses it, so the choice must be made again"
        );

        // One operation, and afterwards exactly the chosen definition is drawn
        // and still chosen.
        assert!(isolate_selected(
            &mut scene.visibility,
            &scene.selection,
            &mut scene.hovered,
            &picture,
            &mut input
        ));

        assert_eq!(scene.selection, chosen, "isolating changed what was chosen");
        assert!(scene.visibility.shows(1, &picture));
        assert!(!scene.visibility.shows(0, &picture));
        assert!(!scene.visibility.shows(2, &picture));
        assert_eq!(
            scene.visibility.bounds(&picture),
            picture.bounds_of(target),
            "what is left is exactly the chosen definition, in both its places"
        );
        assert_eq!(input.camera().view_projection(), camera);
    }

    #[test]
    fn hiding_the_front_definition_is_the_only_way_to_reach_the_one_behind_it() {
        let mut renderer = renderer_or_skip!();
        let (snapshot, camera) = occluding_pair();
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&snapshot))
            .expect("uploads");

        let plain = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &Visibility::new(&snapshot),
            )
            .expect("draws");

        // The defect, stated as pixels: the rear definition is drawn, is part
        // of the model, and cannot be reached. Every pixel of the model is the
        // front one.
        let front = snapshot.pick_of(0).expect("drawn");
        let rear = snapshot.pick_of(1).expect("drawn");
        let model: Vec<(u32, u32)> = (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .filter(|(x, y)| plain.pick_at(*x, *y) != PickId::NOTHING)
            .collect();
        assert!(model.len() > 400, "the pair is drawn");
        assert!(
            model.iter().all(|(x, y)| plain.pick_at(*x, *y) == front),
            "the gate needs the front definition to cover the rear one completely"
        );

        // Choosing the front one, and hiding what is chosen.
        let mut scene = LiveScene::new(
            (),
            Vec::new(),
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&snapshot),
            Vec::new(),
            Vec::new(),
        );
        scene.selection = Selection::Definition(front);
        let mut input = ViewportInput::new();
        input.resize(128, 128);
        let before = camera.view_projection();

        assert!(
            hide_selected(
                &mut scene.visibility,
                &mut scene.selection,
                &mut scene.hovered,
                &snapshot,
                &mut input
            ),
            "hiding a chosen, visible definition must be something that happens"
        );

        let revealed = renderer
            .render(
                &prepared,
                &camera,
                scene.selection.marked(),
                scene.hovered,
                &scene.visibility,
            )
            .expect("draws");

        // What was behind is now there, and is exactly itself: its own
        // definition, and its own face.
        let seen: Vec<(u32, u32)> = (0..revealed.height())
            .flat_map(|y| (0..revealed.width()).map(move |x| (x, y)))
            .filter(|(x, y)| revealed.pick_at(*x, *y) != PickId::NOTHING)
            .collect();
        assert!(!seen.is_empty(), "hiding the front revealed nothing");
        for (x, y) in &seen {
            assert_eq!(revealed.pick_at(*x, *y), rear);
            assert_eq!(
                revealed.hit_at(*x, *y).face(),
                snapshot.face_of(1, 0).expect("numbered"),
                "the revealed definition must carry its own face identity"
            );
        }
        // And where only the front used to be, there is now nothing at all -
        // not a stale pick of something no longer drawn.
        for (x, y) in &model {
            if revealed.pick_at(*x, *y) == PickId::NOTHING {
                assert_eq!(revealed.hit_at(*x, *y).definition(), PickId::NOTHING);
            }
        }

        // Nothing moved the camera to achieve it.
        assert_eq!(camera.view_projection(), before);
        assert_eq!(
            input.camera().view_projection(),
            ViewportInput::new().camera().view_projection()
        );
    }

    #[test]
    fn clicking_a_named_face_of_the_plate_selects_that_face_and_says_what_it_is() {
        let mut renderer = renderer_or_skip!();
        let (_directory, scene) = plate_scene();
        let snapshot = std::sync::Arc::new(scene.snapshot.clone());
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&snapshot))
            .expect("uploads");

        let mut camera = Camera::new();
        camera.resize(160, 160);
        camera
            .frame(snapshot.bounds().expect("the plate has an extent"))
            .expect("frames the plate");
        // Looked at from a corner, so more than one face of the plate is on
        // screen: a gate that saw only the face it clicked could not tell
        // "this face" from "this body".
        camera.orbit(0.7, 0.6);
        let plain = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &Visibility::new(&snapshot),
            )
            .expect("draws");

        // A pixel of the plate's surface, and what the frame says is under it.
        // Where the plate draws the boundary of a face, the pixel is ink taken
        // to the end of the range rather than the shaded material, and a
        // boundary belongs to the face on either side of it: which face a
        // pixel of ink is marked for is a question about linework, gated where
        // linework is gated, and not about which face a click chose.
        let drawn: Vec<(u32, u32)> = (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .filter(|(x, y)| {
                plain.pick_at(*x, *y) != PickId::NOTHING
                    && plain
                        .colour_at(*x, *y)
                        .is_some_and(|pixel| pixel[0..3] != [0, 0, 0] && pixel[0..3] != [255; 3])
            })
            .collect();
        assert!(drawn.len() > 200, "the plate is drawn");
        let hit = plain.hit_at(drawn[0].0, drawn[0].1);

        // The defect: this face is named by the document, so clicking it must
        // choose the face and not merely the body it is part of.
        let chosen = selection_at(hit, &snapshot, &scene.faces, &scene.edges, &scene.vertices);
        let Selection::Face(face) = &chosen else {
            panic!("clicking a named face of the plate chose {chosen:?}");
        };
        assert_eq!(face.face(), hit.face());
        assert!(!face.meanings().is_empty());

        // What is marked is that face, in the placements of its definition,
        // and not the rest of the definition.
        let selected = renderer
            .render(
                &prepared,
                &camera,
                chosen.marked(),
                Hovered::Nothing,
                &Visibility::new(&snapshot),
            )
            .expect("draws");
        let mut same_face = 0usize;
        let mut same_definition = 0usize;
        for (x, y) in &drawn {
            let changed = selected.colour_at(*x, *y) != plain.colour_at(*x, *y);
            if plain.hit_at(*x, *y).face() == hit.face() {
                assert!(changed, "the chosen face was left alone at {x},{y}");
                same_face += 1;
            } else {
                assert!(!changed, "choosing one face changed another at {x},{y}");
                same_definition += 1;
            }
        }
        assert!(
            same_face > 20 && same_definition > 20,
            "the gate must see both the chosen face and the rest of the body"
        );

        // And what the inspector says about it is the document's own words.
        let words = words_of(&chosen, &scene.catalogue, &snapshot);
        let face_names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.faces.iter().map(topology_name).collect();
        let edge_names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.edges.iter().map(topology_name).collect();
        let inspected = inspected(
            &chosen,
            &scene.catalogue,
            &words.identities,
            &face_names,
            &edge_names,
            &[],
            &snapshot,
        )
        .expect("a chosen face is described");
        let rows = inspected.rows();
        let shown = format!("{rows:?}");
        assert!(rows.iter().any(|(label, _)| *label == "Role"));
        assert!(rows.iter().any(|(label, _)| *label == "Reference"));
        for forbidden in ["pick", "session", "handle", "mesh", "ordinal", "index"] {
            assert!(
                !shown.to_lowercase().contains(forbidden),
                "the inspector said {forbidden}: {shown}"
            );
        }
    }

    /// The plate, with one of its named faces chosen.
    fn plate_with_a_chosen_face() -> (tempfile::TempDir, LoadedScene, Selection) {
        let (directory, scene) = plate_scene();
        let pick = scene.snapshot.pick_of(0).expect("the plate is drawn");
        let face = scene.snapshot.face_of(0, 0).expect("numbered");
        let chosen = Selection::at(
            pick,
            face,
            EdgePickId::NOTHING,
            ferritecad_viewport::VertexPickId::NOTHING,
            &scene.snapshot,
            &scene.faces,
            &EdgeNames::default(),
            &ferritecad_scene::VertexNames::default(),
        );
        assert!(
            matches!(chosen, Selection::Face(_)),
            "the fixture must begin with a face chosen"
        );
        (directory, scene, chosen)
    }

    fn double_tap() -> WindowEvent {
        WindowEvent::DoubleTapGesture {
            device_id: winit::event::DeviceId::dummy(),
        }
    }

    #[test]
    fn a_double_tap_magnifies_what_is_chosen_and_taps_back_to_where_it_was() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let mut live = LiveScene::new(
            (),
            vec![a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&scene.snapshot),
            Vec::new(),
            Vec::new(),
        );
        live.selection = chosen.clone();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        // Looking at the whole picture from a long way off, which is what a
        // smart magnification has to improve on.
        input
            .frame(scene.snapshot.bounds().expect("an extent"))
            .expect("frames");
        input.handle(ViewportEvent::Wheel { delta: -6.0 }, false);
        let away = *input.camera();
        let _ = input.take_redraw();

        magnify_gesture(
            &live,
            &scene.snapshot,
            &mut input,
            &double_tap(),
            false,
            false,
        )
        .expect("a double tap over a chosen face frames it");

        assert_ne!(
            *input.camera(),
            away,
            "a double tap did not magnify anything"
        );
        let closer = *input.camera();
        assert!(
            closer.world_per_pixel() < away.world_per_pixel(),
            "a double tap did not come closer: {} against {}",
            closer.world_per_pixel(),
            away.world_per_pixel()
        );

        // And the way back is the same gesture again, exactly.
        magnify_gesture(
            &live,
            &scene.snapshot,
            &mut input,
            &double_tap(),
            false,
            false,
        )
        .expect("a second double tap goes back");
        assert_eq!(
            *input.camera(),
            away,
            "a second double tap did not restore the view"
        );
    }

    #[test]
    fn magnifying_and_going_back_draw_the_very_same_pixels_again() {
        let mut renderer = renderer_or_skip!();
        let (snapshot, camera) = marker_beside_a_middle(200, 200);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&snapshot))
            .expect("uploads");
        let uploaded = renderer.geometry_uploads();
        let marker = snapshot.pick_of(0).expect("drawn");
        let marker_face = snapshot.face_of(0, 0).expect("numbered");

        let mut live = LiveScene::new(
            (),
            Vec::new(),
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&snapshot),
            Vec::new(),
            Vec::new(),
        );
        live.selection = Selection::Definition(marker);

        let mut input = ViewportInput::new();
        input.resize(200, 200);
        input
            .frame(snapshot.bounds().expect("an extent"))
            .expect("frames");
        assert_eq!(
            *input.camera(),
            camera,
            "the gate framed two different views"
        );

        let draw = |renderer: &mut Renderer, camera: &Camera| {
            renderer
                .render(
                    &prepared,
                    camera,
                    Marked::Nothing,
                    Hovered::Nothing,
                    &live.visibility,
                )
                .expect("draws")
        };

        let away = draw(&mut renderer, input.camera());
        let marker_pixels: Vec<(u32, u32)> = (0..away.height())
            .flat_map(|y| (0..away.width()).map(move |x| (x, y)))
            .filter(|(x, y)| away.pick_at(*x, *y) == marker)
            .collect();
        assert!(!marker_pixels.is_empty(), "the marker was never drawn");

        // The first tap: the chosen part fills the view.
        assert!(
            magnify_gesture(&live, &snapshot, &mut input, &double_tap(), false, false)
                .expect("frames"),
            "a double tap on a chosen part did nothing"
        );
        let closer = draw(&mut renderer, input.camera());
        assert_ne!(
            away.colour(),
            closer.colour(),
            "magnifying drew the same picture"
        );
        let now: Vec<(u32, u32)> = (0..closer.height())
            .flat_map(|y| (0..closer.width()).map(move |x| (x, y)))
            .filter(|(x, y)| closer.pick_at(*x, *y) == marker)
            .collect();
        assert!(
            now.len() > marker_pixels.len() * 3,
            "the chosen part did not fill the view: {} pixels became {}",
            marker_pixels.len(),
            now.len()
        );

        // It is the same part and the same face, and everything that is not a
        // part is still nobody.
        for (x, y) in &now {
            assert_eq!(closer.pick_at(*x, *y), marker);
            assert_eq!(closer.hit_at(*x, *y).face(), marker_face);
        }
        for (x, y) in (0..closer.height())
            .flat_map(|y| (0..closer.width()).map(move |x| (x, y)))
            .filter(|(x, y)| closer.pick_at(*x, *y) == PickId::NOTHING)
        {
            assert_eq!(
                closer.hit_at(x, y).definition(),
                PickId::NOTHING,
                "the backdrop became something to click at ({x}, {y})"
            );
            assert_eq!(
                closer.hit_at(x, y).face(),
                ferritecad_viewport::FacePickId::NOTHING
            );
        }

        // Drawing it again changes nothing about it.
        let again = draw(&mut renderer, input.camera());
        assert_eq!(
            closer.colour(),
            again.colour(),
            "the same camera drew two different pictures"
        );

        // The second tap: exactly the picture that was there before, to the
        // byte, which is the strongest thing "restored exactly" can mean.
        assert!(
            magnify_gesture(&live, &snapshot, &mut input, &double_tap(), false, false)
                .expect("goes back"),
            "a second double tap did nothing"
        );
        let back = draw(&mut renderer, input.camera());
        assert_eq!(
            away.colour(),
            back.colour(),
            "going back did not draw the picture it left"
        );
        for (x, y) in (0..back.height()).flat_map(|y| (0..back.width()).map(move |x| (x, y))) {
            assert_eq!(
                away.pick_at(x, y),
                back.pick_at(x, y),
                "going back changed what ({x}, {y}) belongs to"
            );
            assert_eq!(away.hit_at(x, y).face(), back.hit_at(x, y).face());
        }

        // None of it was a change to the model.
        assert_eq!(
            renderer.geometry_uploads(),
            uploaded,
            "magnifying uploaded geometry"
        );
    }

    /// Every pixel where the picture changes from one face to another.
    ///
    /// Read from the face target rather than from the colour, so the gate
    /// knows where a boundary is without having to recognise one by eye.
    fn face_boundary_pixels(frame: &ferritecad_viewport_gpu::Frame) -> Vec<(u32, u32)> {
        let face = |x: u32, y: u32| frame.hit_at(x, y).face();
        let mut boundary = Vec::new();
        for y in 1..frame.height() - 1 {
            for x in 1..frame.width() - 1 {
                let mine = face(x, y);
                if mine == ferritecad_viewport::FacePickId::NOTHING {
                    continue;
                }
                let neighbours = [
                    face(x - 1, y),
                    face(x + 1, y),
                    face(x, y - 1),
                    face(x, y + 1),
                ];
                if neighbours.iter().any(|other| {
                    *other != mine && *other != ferritecad_viewport::FacePickId::NOTHING
                }) {
                    boundary.push((x, y));
                }
            }
        }
        boundary
    }

    fn luminance(colour: [u8; 4]) -> f32 {
        0.2126 * f32::from(colour[0])
            + 0.7152 * f32::from(colour[1])
            + 0.0722 * f32::from(colour[2])
    }

    #[test]
    fn where_one_face_of_the_plate_ends_and_the_next_begins_is_drawn() {
        let mut renderer = renderer_or_skip!();
        let (_directory, scene) = plate_scene();
        let snapshot = std::sync::Arc::new(scene.snapshot);
        let prepared = renderer
            .prepare(std::sync::Arc::clone(&snapshot))
            .expect("uploads");
        let mut camera = Camera::new();
        camera.resize(240, 240);
        camera
            .frame(snapshot.bounds().expect("the plate has an extent"))
            .expect("frames");
        // A corner view, so several faces of the solid are on screen at once.
        camera.orbit(0.7, 0.55);
        let frame = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &Visibility::default(),
            )
            .expect("draws");

        let boundary = face_boundary_pixels(&frame);
        assert!(
            boundary.len() > 20,
            "the gate found no face boundary to look at: {} pixels",
            boundary.len()
        );

        // What the model is drawn in, away from any boundary.
        let fill: Vec<f32> = (0..frame.height())
            .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
            .filter(|(x, y)| {
                frame.hit_at(*x, *y).face() != ferritecad_viewport::FacePickId::NOTHING
                    && !boundary.contains(&(*x, *y))
            })
            .filter_map(|(x, y)| frame.colour_at(x, y).map(luminance))
            .collect();
        assert!(!fill.is_empty(), "the plate drew nothing");
        let dimmest_fill = fill.iter().copied().fold(f32::INFINITY, f32::min);

        // A boundary that is drawn is drawn in ink: darker than anything the
        // shaded surface produces, rather than merely a change of shade
        // between two lit faces.
        let inked = boundary
            .iter()
            .filter_map(|(x, y)| frame.colour_at(*x, *y).map(luminance))
            .filter(|value| *value + 12.0 < dimmest_fill)
            .count();
        assert!(
            inked * 4 > boundary.len(),
            "only {inked} of {} boundary pixels are drawn as a line; the dimmest fill is \
             {dimmest_fill}",
            boundary.len()
        );
    }

    #[test]
    fn linework_is_not_a_part_and_appears_in_nothing_that_lists_parts() {
        let (_directory, scene) = plate_scene();
        let snapshot = &scene.snapshot;

        // The plate has boundaries to draw, so this gate is about a picture
        // that actually has linework in it.
        assert!(
            snapshot.meshes().iter().any(|mesh| mesh.line_count() > 0),
            "the committed plate packs no linework"
        );

        // What a picture is measured as, and what a list of parts holds, are
        // both about the model. Lines have no extent of their own and no row.
        let from_the_model = snapshot
            .meshes()
            .iter()
            .zip(snapshot.draws())
            .filter(|(mesh, _)| mesh.triangle_count() > 0)
            .count();
        assert!(from_the_model > 0, "the plate draws no triangles");
        assert_eq!(
            scene.catalogue.len(),
            snapshot.meshes().len(),
            "the list of parts counted something other than the definitions"
        );
        // The extent is the model's, and the model's alone: a line indexes the
        // same vertices the triangles do, so no line can reach outside what
        // the fill already covers and none can enlarge what framing sees.
        for mesh in snapshot.meshes() {
            let vertices = mesh.vertex_count() as u32;
            for index in mesh.line_indices() {
                assert!(
                    *index < vertices,
                    "a line names vertex {index} of {vertices}, so it is not the model's"
                );
            }
        }
    }

    #[test]
    fn a_double_tap_is_never_a_camera_event_as_well() {
        // The two routes below the window arm are siblings, and only one of
        // them may act on a double tap.
        assert!(translate(&double_tap()).is_empty());
    }

    #[test]
    fn a_double_tap_the_interface_wanted_changes_nothing_at_all() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let mut live = LiveScene::new(
            (),
            vec![a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&scene.snapshot),
            Vec::new(),
            Vec::new(),
        );
        live.selection = chosen;
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(scene.snapshot.bounds().expect("an extent"))
            .expect("frames");
        input.handle(ViewportEvent::PointerMoved { x: 120.0, y: 80.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        let before = *input.camera();
        let _ = input.take_redraw();

        for (consumed, wants_pointer) in [(true, false), (false, true), (true, true)] {
            assert!(
                !magnify_gesture(
                    &live,
                    &scene.snapshot,
                    &mut input,
                    &double_tap(),
                    consumed,
                    wants_pointer,
                )
                .expect("a claimed gesture is not a failure"),
                "consumed {consumed}, wanted {wants_pointer}: the interface's gesture reached \
                 the model"
            );
            assert_eq!(
                *input.camera(),
                before,
                "consumed {consumed}, wanted {wants_pointer}: the camera moved"
            );
            assert!(
                !input.take_redraw(),
                "consumed {consumed}, wanted {wants_pointer}: a frame was owed"
            );
        }

        // Including the way back: a gesture the panel took cannot have started
        // a magnification to undo.
        assert_eq!(
            input.take_pick(),
            Some((120.0, 80.0)),
            "a claimed double tap forgot a waiting click"
        );
        assert!(
            magnify_gesture(
                &live,
                &scene.snapshot,
                &mut input,
                &double_tap(),
                false,
                false,
            )
            .expect("frames"),
            "the unclaimed gesture had nothing left to do"
        );
        assert_ne!(
            *input.camera(),
            before,
            "the first real double tap went back rather than magnifying"
        );
    }

    #[test]
    fn a_double_tap_looks_at_what_is_chosen_and_otherwise_at_what_is_on_screen() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let snapshot = &scene.snapshot;
        let mut live = LiveScene::new(
            (),
            vec![a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(snapshot),
            Vec::new(),
            Vec::new(),
        );

        // Nothing chosen: everything still on screen.
        assert_eq!(live.selection, Selection::Nothing);
        assert_eq!(
            magnified_bounds(&live, snapshot),
            live.visibility.bounds(snapshot),
            "with nothing chosen a double tap did not look at the visible picture"
        );

        // A chosen face is the face, not the part it is on and not the scene.
        live.selection = chosen.clone();
        let face_extent = magnified_bounds(&live, snapshot).expect("the face has an extent");
        assert_eq!(
            Some(face_extent),
            chosen.bounds(snapshot),
            "a chosen face was not what the double tap looked at"
        );
        assert_ne!(
            Some(face_extent),
            live.visibility.bounds(snapshot),
            "the gate cannot tell a chosen face from the whole picture"
        );

        // A chosen definition is the part.
        let pick = snapshot.pick_of(0).expect("the plate is drawn");
        live.selection = Selection::Definition(pick);
        assert_eq!(
            magnified_bounds(&live, snapshot),
            snapshot.bounds_of(pick),
            "a chosen part was not what the double tap looked at"
        );

        // What is hidden is not part of the picture to look at. Hiding the
        // only part leaves nothing at all, which is a complete no-op rather
        // than a view of the origin.
        live.selection = Selection::Nothing;
        assert!(live.visibility.hide(Marked::Definition(pick), snapshot));
        assert_eq!(
            magnified_bounds(&live, snapshot),
            None,
            "a hidden part was still counted as something to look at"
        );

        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(snapshot.bounds().expect("an extent"))
            .expect("frames");
        let before = *input.camera();
        let _ = input.take_redraw();
        assert!(
            !magnify_gesture(&live, snapshot, &mut input, &double_tap(), false, false)
                .expect("nothing to look at is not a failure"),
            "a double tap with nothing on screen did something"
        );
        assert_eq!(*input.camera(), before, "the camera moved towards nothing");
        assert!(!input.take_redraw(), "nothing to draw asked to be drawn");
    }

    #[test]
    fn magnifying_and_going_back_leave_the_chosen_face_and_the_hidden_parts_alone() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let snapshot = &scene.snapshot;
        let mut live = LiveScene::new(
            (),
            vec![a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(snapshot),
            Vec::new(),
            Vec::new(),
        );
        live.selection = chosen.clone();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(snapshot.bounds().expect("an extent"))
            .expect("frames");
        input.handle(ViewportEvent::Wheel { delta: -5.0 }, false);
        let away = *input.camera();
        let mask = live.visibility.clone();

        let Selection::Face(ref face) = live.selection else {
            panic!("the gate must begin with a face chosen");
        };
        let (was_face, was_meanings) = (face.face(), face.meanings().to_vec());

        for direction in ["magnifying", "going back"] {
            assert!(
                magnify_gesture(&live, snapshot, &mut input, &double_tap(), false, false)
                    .expect("frames or goes back"),
                "{direction} did nothing"
            );
            let Selection::Face(ref face) = live.selection else {
                panic!("{direction} stopped a face being chosen");
            };
            assert_eq!(face.face(), was_face, "{direction} changed which face");
            assert_eq!(
                face.meanings(),
                was_meanings.as_slice(),
                "{direction} changed the durable names the face resolves to"
            );
            assert_eq!(live.selection, chosen, "{direction} changed the selection");
            assert_eq!(live.visibility, mask, "{direction} changed what is drawn");
        }
        assert_eq!(
            *input.camera(),
            away,
            "going back did not restore the view exactly"
        );
    }

    #[test]
    fn the_rotation_input_path_receives_neither_selection_nor_visibility() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let mut live = LiveScene::new(
            (),
            vec![a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::new(&scene.snapshot),
            Vec::new(),
            Vec::new(),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(scene.snapshot.bounds().expect("an extent"))
            .expect("frames");

        // This is the semantic path the window calls, whose signature has no
        // scene state. It proves the ownership boundary below the window arm;
        // the arm itself cannot be exercised without opening a window.
        let turn = |input: &mut ViewportInput| {
            let before = *input.camera();
            apply_viewport_input(
                input,
                &WindowEvent::RotationGesture {
                    device_id: winit::event::DeviceId::dummy(),
                    delta: 30.0,
                    phase: winit::event::TouchPhase::Moved,
                },
                false,
                false,
            );
            assert_ne!(*input.camera(), before, "the gate turned nothing");
            assert_eq!(input.camera().target(), before.target(), "the view moved");
            assert_eq!(
                input.camera().distance(),
                before.distance(),
                "the view came closer"
            );
        };

        // A real face of a real definition, chosen the way a click chooses it.
        live.selection = chosen.clone();
        assert!(
            matches!(live.selection, Selection::Face(_)),
            "the gate must begin with a face chosen"
        );
        turn(&mut input);
        assert_eq!(
            live.selection, chosen,
            "a turn changed which face is chosen"
        );

        // The committed plate is one definition, so a mask that hides
        // something and a face chosen on something still visible cannot both
        // exist in this scene: hiding the chosen thing unchooses it. The two
        // are therefore gated in the two states the application can actually
        // reach, rather than in one it cannot.
        assert!(hide_selected(
            &mut live.visibility,
            &mut live.selection,
            &mut live.hovered,
            &scene.snapshot,
            &mut input,
        ));
        assert!(
            !live.visibility.shows(0, &scene.snapshot),
            "the gate must go on with something actually hidden"
        );
        let mask = live.visibility.clone();
        let _ = input.take_redraw();

        turn(&mut input);

        assert_eq!(live.visibility, mask, "a turn changed what is drawn");
        assert_eq!(
            live.selection,
            Selection::Nothing,
            "a turn chose something nobody clicked"
        );

        // And what undoing would put back is still there to be put back,
        // which is the half of a mask that comparing masks cannot see.
        let mut undone = live.visibility.clone();
        assert!(
            undone.undo(&scene.snapshot),
            "a turn discarded the checkpoint an undo needs"
        );
        assert_ne!(undone, mask, "the checkpoint put nothing back");
    }

    #[test]
    fn two_fingers_turning_are_measured_in_degrees_and_the_camera_in_radians() {
        use winit::event::TouchPhase;

        let turn = |delta: f32, phase| {
            translate(&WindowEvent::RotationGesture {
                device_id: winit::event::DeviceId::dummy(),
                delta,
                phase,
            })
        };

        // winit counts counterclockwise as positive and reports degrees; the
        // camera turns the same way and works in radians, so the conversion
        // happens exactly here and exactly once.
        assert_eq!(
            turn(90.0, TouchPhase::Moved),
            vec![ViewportEvent::Roll {
                radians: std::f32::consts::FRAC_PI_2
            }]
        );
        assert_eq!(
            turn(-45.0, TouchPhase::Moved),
            vec![ViewportEvent::Roll {
                radians: -std::f32::consts::FRAC_PI_4
            }]
        );

        // Every phase carries its own delta and none of them is focus loss.
        for phase in [
            TouchPhase::Started,
            TouchPhase::Moved,
            TouchPhase::Ended,
            TouchPhase::Cancelled,
        ] {
            assert_eq!(
                turn(12.0, phase),
                vec![ViewportEvent::Roll {
                    radians: 12.0f32.to_radians()
                }],
                "{phase:?} lost its delta"
            );
            assert_eq!(
                turn(0.0, phase),
                vec![ViewportEvent::Roll { radians: 0.0 }],
                "{phase:?} did not translate as a turn of nothing"
            );
        }
    }

    #[test]
    fn a_non_moving_rotation_phase_is_not_a_cancelled_mouse_gesture() {
        use winit::event::TouchPhase;

        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(([-5.0, -5.0, -5.0], [5.0, 5.0, 5.0]))
            .expect("frames");
        input.handle(ViewportEvent::PointerMoved { x: 300.0, y: 200.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        let before = *input.camera();
        let _ = input.take_redraw();

        for phase in [
            TouchPhase::Started,
            TouchPhase::Ended,
            TouchPhase::Cancelled,
        ] {
            for event in translate(&WindowEvent::RotationGesture {
                device_id: winit::event::DeviceId::dummy(),
                delta: 0.0,
                phase,
            }) {
                input.handle(event, false);
            }
        }

        assert_eq!(*input.camera(), before, "a rotation phase moved the camera");
        assert!(input.is_dragging(), "a rotation phase ended a mouse drag");
        assert!(!input.take_redraw(), "a rotation phase asked for a frame");

        // And the drag still turns the model, which is what "the pointer was
        // not discarded" actually means.
        input.handle(ViewportEvent::PointerMoved { x: 360.0, y: 240.0 }, false);
        assert_ne!(
            input.camera().eye(),
            before.eye(),
            "the drag stopped turning the model after a rotation phase"
        );
    }

    #[test]
    fn a_row_chooses_the_definition_and_never_the_face_under_the_pointer() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let mut selection = chosen;
        let mut input = ViewportInput::new();
        let _ = input.take_redraw();

        select_definition_row(&mut selection, &scene.snapshot, &mut input, 0);

        // The same part, chosen as a part. A list of definitions holds no
        // faces, so pressing a row cannot mean "that face".
        assert_eq!(
            selection,
            Selection::Definition(scene.snapshot.pick_of(0).expect("drawn"))
        );
        assert!(input.take_redraw(), "the changed highlight was not drawn");
    }

    #[test]
    fn framing_uses_the_face_when_a_face_is_chosen_and_the_part_when_it_is_not() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let definition = Selection::Definition(scene.snapshot.pick_of(0).expect("drawn"));

        let face_bounds = chosen.bounds(&scene.snapshot).expect("the face is drawn");
        let part_bounds = definition
            .bounds(&scene.snapshot)
            .expect("the part is drawn");
        assert_ne!(
            face_bounds, part_bounds,
            "a chosen face must not be framed as the whole part"
        );

        // And the camera actually goes to the smaller of the two.
        let mut looking_at_the_face = ViewportInput::new();
        looking_at_the_face.resize(800, 600);
        let mut scene_with_face = LiveScene::new(
            (),
            Vec::new(),
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::default(),
            Vec::new(),
            Vec::new(),
        );
        scene_with_face.selection = chosen;
        assert!(
            frame_selection(&scene_with_face, &scene.snapshot, &mut looking_at_the_face)
                .expect("frames"),
            "framing a chosen face did nothing"
        );

        let mut looking_at_the_part = ViewportInput::new();
        looking_at_the_part.resize(800, 600);
        let mut scene_with_part = LiveScene::new(
            (),
            Vec::new(),
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::default(),
            Vec::new(),
            Vec::new(),
        );
        scene_with_part.selection = definition;
        assert!(
            frame_selection(&scene_with_part, &scene.snapshot, &mut looking_at_the_part)
                .expect("frames")
        );
        assert_ne!(
            looking_at_the_face.camera().view_projection(),
            looking_at_the_part.camera().view_projection(),
            "framing a face and framing its part put the camera in one place"
        );
    }

    #[test]
    fn a_chosen_face_survives_a_failed_open_and_not_a_successful_one() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let mut live = LiveScene::new(
            (),
            vec![a_body()],
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::default(),
            Vec::new(),
            Vec::new(),
        );
        live.selection = chosen.clone();
        live.hovered = Hovered::Face(scene.snapshot.face_of(0, 1).expect("numbered"));
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);

        // A load that failed changes nothing at all, including which face is
        // chosen and what the pointer was over.
        commit_scene(&mut live, &mut camera, Err(CadError::input("no")))
            .expect_err("a failed load commits nothing");
        assert_eq!(live.selection, chosen);
        assert_eq!(
            live.hovered,
            Hovered::Face(scene.snapshot.face_of(0, 1).expect("numbered"))
        );

        // A load that arrived replaces all of it at once.
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut live,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: vec![a_body()],
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::default(),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");
        assert_eq!(live.selection, Selection::Nothing);
        assert_eq!(live.hovered, Hovered::Nothing);
    }

    #[test]
    fn a_face_of_the_replaced_picture_chooses_nothing_in_the_next_one() {
        let (_directory, scene, _) = plate_with_a_chosen_face();
        let stale = scene.snapshot.face_of(0, 0).expect("numbered");

        // Another picture entirely, with no durable face names at all.
        let picture = distant_scene();
        let pick = picture.pick_of(0).expect("drawn");
        assert_eq!(
            Selection::at(
                pick,
                stale,
                EdgePickId::NOTHING,
                ferritecad_viewport::VertexPickId::NOTHING,
                &picture,
                &FaceNames::default(),
                &EdgeNames::default(),
                &ferritecad_scene::VertexNames::default(),
            ),
            Selection::Definition(pick),
            "a face of the replaced picture attached itself to the new one"
        );
    }

    #[test]
    fn the_inspector_and_the_marked_pixels_describe_one_selection() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let words = words_of(&chosen, &scene.catalogue, &scene.snapshot);
        let face_names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.faces.iter().map(topology_name).collect();
        let edge_names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.edges.iter().map(topology_name).collect();
        let described = inspected(
            &chosen,
            &scene.catalogue,
            &words.identities,
            &face_names,
            &edge_names,
            &[],
            &scene.snapshot,
        )
        .expect("a chosen face is described");

        // The inspector describes a face, and the renderer is told to mark a
        // face: one selection, two views of it.
        assert!(matches!(described, Selected::Face { .. }));
        let Selection::Face(face) = &chosen else {
            panic!("the fixture chose no face");
        };
        assert_eq!(chosen.marked(), Marked::Face(face.face()));

        // Choosing the part instead moves both views together.
        let definition = Selection::Definition(scene.snapshot.pick_of(0).expect("drawn"));
        let words = words_of(&definition, &scene.catalogue, &scene.snapshot);
        let face_names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.faces.iter().map(topology_name).collect();
        let edge_names: Vec<ferritecad_ui::TopologyName<'_>> =
            words.edges.iter().map(topology_name).collect();
        let described = inspected(
            &definition,
            &scene.catalogue,
            &words.identities,
            &face_names,
            &edge_names,
            &[],
            &scene.snapshot,
        )
        .expect("a chosen definition is described");
        assert!(matches!(described, Selected::Body { .. }));
        assert!(matches!(definition.marked(), Marked::Definition(_)));
    }

    #[test]
    fn pointing_at_a_face_asks_about_that_face_and_chooses_nothing() {
        use ferritecad_viewport::FacePickId;

        let picture = distant_scene();
        let other = scene_at(400.0);
        let chosen = picture
            .draws()
            .first()
            .expect("the picture draws something")
            .pick;
        let mut scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };

        // The face a pixel of this picture would report.
        let face = FacePickId::from_raw(1, &picture);
        assert_eq!(picture.definition_of_face(face), Some(0));

        assert!(hover(&mut scene.hovered, &picture, Hovered::Face(face)));
        assert_eq!(scene.hovered, Hovered::Face(face));
        assert_eq!(
            scene.selection,
            Selection::Definition(chosen),
            "pointing at a face chose it"
        );

        // A face and the definition it belongs to are different answers, and
        // moving between them is news even though the part is the same.
        assert!(hover(
            &mut scene.hovered,
            &picture,
            Hovered::Definition(chosen)
        ));
        assert_eq!(scene.hovered, Hovered::Definition(chosen));

        // A face of a picture that has been replaced marks nothing here,
        // however plausible its number looks: the other picture numbers its
        // faces from one as well.
        assert!(hover(
            &mut scene.hovered,
            &other,
            Hovered::Face(FacePickId::from_raw(1, &picture))
        ));
        assert_eq!(scene.hovered, Hovered::Nothing);
        assert_eq!(scene.selection, Selection::Definition(chosen));
    }

    #[test]
    fn a_replacement_forgets_what_was_under_the_pointer() {
        let picture = distant_scene();
        let chosen = picture
            .draws()
            .first()
            .expect("the picture draws something")
            .pick;
        let mut scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Hovered::Definition(chosen),
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);

        // A load that failed keeps the scene it could not replace, including
        // what the pointer was over.
        commit_scene(&mut scene, &mut camera, Err(CadError::input("no")))
            .expect_err("a failed load commits nothing");
        assert_eq!(scene.hovered, Hovered::Definition(chosen));
        assert_eq!(scene.selection, Selection::Definition(chosen));

        // A load that arrived replaces all of it: the question belonged to the
        // previous picture as much as the answer did.
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut scene,
            &mut camera,
            Ok(PreparedLoad {
                framed,
                prepared: (),
                catalogue: vec![a_body()],
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::default(),
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            }),
        )
        .expect("a load that arrived commits");
        assert_eq!(scene.hovered, Hovered::Nothing);
        assert_eq!(scene.selection, Selection::Nothing);
    }

    #[test]
    fn a_row_and_a_click_choose_the_same_thing_and_show_it_the_same_way() {
        let mut builder = SnapshotBuilder::new();
        let first = builder
            .add_mesh(&distant_scene_mesh())
            .expect("the mesh is valid");
        let second = builder
            .add_mesh(&distant_scene_mesh())
            .expect("the mesh is valid");
        let at = |x: f64| {
            ferritecad_types::Transform::from_translation(
                ferritecad_types::Vec3::new(x, 0.0, 0.0).expect("finite"),
            )
            .expect("finite")
        };
        builder
            .place(first, None, &at(0.0), [0.5, 0.5, 0.5])
            .expect("places it");
        builder
            .place(second, None, &at(40.0), [0.5, 0.5, 0.5])
            .expect("places it");
        let picture = builder.build();

        let entries = vec![a_body(), a_body()];
        let mut scene = LiveScene {
            prepared: (),
            catalogue: entries.clone(),
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };

        // Choosing from a list: the row becomes an identity by asking the
        // picture, and nothing constructs one out of a number.
        let pick = picture.pick_of(second).expect("the picture has that row");
        scene.selection = Selection::Definition(pick);
        assert_eq!(scene.chosen(&picture), Some((second, &entries[second])));

        // Clicking the same definition in the viewport answers identically, so
        // the list, the highlight and the inspector cannot disagree about what
        // is chosen or which row is marked.
        let clicked = picture
            .draws()
            .iter()
            .find(|draw| picture.definition(draw.pick) == Some(second))
            .expect("that definition is drawn")
            .pick;
        assert_eq!(clicked, pick, "a row and a click named different things");
    }

    #[test]
    fn choosing_a_definition_row_changes_the_choice_and_draws_once() {
        let mut builder = SnapshotBuilder::new();
        for _ in 0..2 {
            let mesh = builder
                .add_mesh(&distant_scene_mesh())
                .expect("the mesh is valid");
            builder
                .place(
                    mesh,
                    None,
                    &ferritecad_types::Transform::IDENTITY,
                    [0.5, 0.5, 0.5],
                )
                .expect("places it");
        }
        let picture = builder.build();
        let mut selection = Selection::Nothing;
        let mut input = ViewportInput::new();
        let _ = input.take_redraw();

        select_definition_row(&mut selection, &picture, &mut input, 1);
        assert_eq!(selection.owning_definition(&picture), Some(1));
        assert!(input.take_redraw(), "the changed highlight was not drawn");
        assert!(
            !input.take_redraw(),
            "one row choice asked for more than one frame"
        );

        // Repeating the choice and pressing a row this picture does not have
        // are both no-ops, including at the redraw boundary.
        select_definition_row(&mut selection, &picture, &mut input, 1);
        select_definition_row(&mut selection, &picture, &mut input, 2);
        assert_eq!(selection.owning_definition(&picture), Some(1));
        assert!(!input.take_redraw());
    }

    #[test]
    fn a_definition_placed_many_times_is_one_row() {
        // What the scene crate hands over for a pattern: one entry, whatever
        // the picture does with it.
        let entry = CatalogueEntry {
            item: SceneItem::Imported(
                ferritecad_document::ImportedDefinitionRef::new(
                    ferritecad_types::ImportedSourceId::new(),
                    "step.product_definition#39",
                )
                .expect("a key names something"),
            ),
            name: Some("Bolt".to_owned()),
            source_file: Some("pattern.step".to_owned()),
            solids: Some(1),
        };
        let scene = LiveScene {
            prepared: (),
            catalogue: vec![entry],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };

        let identities = identities_of(&scene.catalogue);
        let rows = scene.rows(&identities);
        assert_eq!(
            rows.len(),
            scene.catalogue.len(),
            "a list of definitions is not a list of definitions"
        );
        assert_eq!(rows.len(), 1);

        // Two entries that describe themselves identically are still two rows:
        // what makes them two is their identity, and the list does not look at
        // what they are called to decide how many there are.
        let twins = LiveScene {
            prepared: (),
            catalogue: vec![a_body(), a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let identities = identities_of(&twins.catalogue);
        assert_eq!(twins.rows(&identities).len(), 2);
    }

    #[test]
    fn the_marked_row_and_the_described_definition_are_the_same_one() {
        let mut builder = SnapshotBuilder::new();
        for _ in 0..2 {
            let mesh = builder
                .add_mesh(&distant_scene_mesh())
                .expect("the mesh is valid");
            builder
                .place(
                    mesh,
                    None,
                    &ferritecad_types::Transform::IDENTITY,
                    [0.5, 0.5, 0.5],
                )
                .expect("places it");
        }
        let picture = builder.build();

        let entries = vec![a_body(), a_body()];
        let mut scene = LiveScene {
            prepared: (),
            catalogue: entries.clone(),
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(picture.pick_of(1).expect("the picture has that row")),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };

        let identities = identities_of(&scene.catalogue);
        let (rows, marked) = scene.view(&identities, &picture);
        assert_eq!(rows.len(), 2);
        assert_eq!(marked, Some(1), "the list marks no row for what is chosen");

        // The row the list marks is the definition the inspector describes:
        // both come from one resolution, so a click that highlights the model
        // cannot leave the list and the inspector saying different things.
        let described = scene.chosen(&picture).expect("something is chosen");
        assert_eq!(described.0, marked.expect("a row is marked"));
        assert_eq!(described.1, &entries[1]);
        assert_eq!(rows[described.0], describe(&entries[1], &identities[1]));

        // And nothing chosen marks nothing, which is what the background does.
        scene.selection = Selection::Nothing;
        let (rows, marked) = scene.view(&identities, &picture);
        assert_eq!(marked, None, "an empty choice still marked a row");
        assert_eq!(rows.len(), 2, "the list emptied along with the choice");
    }

    #[test]
    fn showing_what_is_chosen_shows_all_of_it_and_keeps_it_chosen() {
        let mut builder = SnapshotBuilder::new();
        let mesh = builder
            .add_mesh(&distant_scene_mesh())
            .expect("the mesh is valid");
        let at = |x: f64| {
            ferritecad_types::Transform::from_translation(
                ferritecad_types::Vec3::new(x, 0.0, 0.0).expect("finite"),
            )
            .expect("finite")
        };
        // Two placements, far apart, of the one definition.
        builder
            .place(mesh, None, &at(0.0), [0.5, 0.5, 0.5])
            .expect("places it");
        builder
            .place(mesh, None, &at(600.0), [0.5, 0.5, 0.5])
            .expect("places it");
        let picture = builder.build();

        let scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(
                picture.pick_of(mesh).expect("the picture has that row"),
            ),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);
        camera.take_redraw();

        assert!(
            frame_selection(&scene, &picture, &mut camera).expect("a box can be framed"),
            "there was somewhere to go and nothing happened"
        );
        assert!(
            camera.take_redraw(),
            "the camera moved and no frame followed"
        );
        assert!(!camera.take_redraw(), "one framing asked for two frames");

        // Both placements are in view: the camera looks at the middle of what
        // is chosen, which is between them rather than at either.
        let target = camera.camera().target();
        assert!(
            target[0] > 250.0 && target[0] < 350.0,
            "only one placement was framed: {target:?}"
        );

        // And what was chosen is still chosen. Showing something is not
        // choosing it, and the borrow above is what makes that structural.
        assert_eq!(
            scene.selection,
            Selection::Definition(picture.pick_of(mesh).expect("the picture has that row"))
        );
    }

    #[test]
    fn the_backdrop_is_not_part_of_the_document() {
        // What a viewer draws behind a model is not in the model. The grid has
        // no entry in the catalogue, adds no definition to choose from, and
        // does not move where a camera goes: it is drawn from the camera, so
        // it cannot be in the extent the camera is framing.
        let mut builder = SnapshotBuilder::new();
        let mesh = builder
            .add_mesh(&distant_scene_mesh())
            .expect("the mesh is valid");
        builder
            .place(
                mesh,
                None,
                &ferritecad_types::Transform::IDENTITY,
                [0.5, 0.5, 0.5],
            )
            .expect("places it");
        let picture = builder.build();

        let scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let identities = identities_of(&scene.catalogue);
        let (rows, marked) = scene.view(&identities, &picture);

        // One row for the one body, and nothing else offered.
        assert_eq!(rows.len(), 1);
        assert_eq!(marked, None);
        assert_eq!(picture.meshes().len(), 1, "something else was packed");
        assert_eq!(picture.draws().len(), 1);

        // The extent is the model's own triangle and nothing around it: a
        // grid that reached into the bounds would make Frame all frame the
        // backdrop instead of the model.
        let (min, max) = picture.bounds().expect("the model has extent");
        assert_eq!(min, [0.0, 0.0, 0.0]);
        assert_eq!(max, [10.0, 10.0, 0.0]);
    }

    #[test]
    fn showing_everything_shows_the_neighbour_that_showing_a_choice_leaves_out() {
        let mut builder = SnapshotBuilder::new();
        let chosen = builder
            .add_mesh(&distant_scene_mesh())
            .expect("the mesh is valid");
        let neighbour = builder
            .add_mesh(&distant_scene_mesh())
            .expect("the mesh is valid");
        let at = |x: f64| {
            ferritecad_types::Transform::from_translation(
                ferritecad_types::Vec3::new(x, 0.0, 0.0).expect("finite"),
            )
            .expect("finite")
        };
        builder
            .place(chosen, None, &at(0.0), [0.5, 0.5, 0.5])
            .expect("places it");
        builder
            .place(neighbour, None, &at(900.0), [0.5, 0.5, 0.5])
            .expect("places it");
        let picture = builder.build();

        let scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body(), a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(
                picture.pick_of(chosen).expect("the picture has that row"),
            ),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };

        let mut showing_choice = ViewportInput::new();
        showing_choice.resize(800, 600);
        assert!(
            frame_selection(&scene, &picture, &mut showing_choice).expect("a box can be framed")
        );

        let mut showing_all = ViewportInput::new();
        showing_all.resize(800, 600);
        assert!(
            frame_scene(&Visibility::new(&picture), &picture, &mut showing_all)
                .expect("a picture can be framed")
        );

        // The two answer different questions. What is chosen sits at the
        // origin, so showing it looks there; the neighbour is 900 away, so
        // showing everything looks between them.
        let choice = showing_choice.camera().target();
        let everything = showing_all.camera().target();
        assert!(choice[0] < 100.0, "showing a choice drifted: {choice:?}");
        assert!(
            everything[0] > 350.0,
            "showing everything left the neighbour out: {everything:?}"
        );

        // And showing everything leaves the choice exactly as it was: it is a
        // camera action, not a way of unchoosing.
        assert_eq!(
            scene.selection,
            Selection::Definition(picture.pick_of(chosen).expect("the picture has that row"))
        );
        assert_eq!(scene.chosen(&picture).map(|(row, _)| row), Some(chosen));
    }

    #[test]
    fn showing_a_choice_and_showing_everything_share_their_arithmetic() {
        // One definition, placed once: what is chosen covers exactly what the
        // picture covers, so both actions are being asked the same question.
        let mut builder = SnapshotBuilder::new();
        let only = builder
            .add_mesh(&distant_scene_mesh())
            .expect("the mesh is valid");
        builder
            .place(
                only,
                None,
                &ferritecad_types::Transform::IDENTITY,
                [0.5, 0.5, 0.5],
            )
            .expect("places it");
        let picture = builder.build();

        let scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(
                picture.pick_of(only).expect("the picture has that row"),
            ),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        assert_eq!(selection_bounds(&scene, &picture), picture.bounds());

        let mut by_choice = ViewportInput::new();
        by_choice.resize(800, 600);
        frame_selection(&scene, &picture, &mut by_choice).expect("frames");

        let mut by_everything = ViewportInput::new();
        by_everything.resize(800, 600);
        frame_scene(&Visibility::new(&picture), &picture, &mut by_everything).expect("frames");

        // The same answer, to the last float. Two implementations of where a
        // camera goes would agree only by accident, and stop agreeing the
        // first time either was touched.
        assert_eq!(by_choice.camera(), by_everything.camera());
    }

    #[test]
    fn an_empty_picture_is_nowhere_to_go_however_often_it_is_asked() {
        let empty = SnapshotBuilder::new().build();
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);
        camera.take_redraw();
        let before = *camera.camera();

        for _ in 0..3 {
            assert!(
                !frame_scene(&Visibility::new(&empty), &empty, &mut camera)
                    .expect("having nowhere to go is not a failure"),
                "an empty picture was framed"
            );
        }
        assert_eq!(*camera.camera(), before);
        assert!(
            !camera.take_redraw(),
            "an action that did nothing asked for a frame"
        );
    }

    #[test]
    fn there_is_nowhere_to_go_for_a_choice_this_picture_does_not_know() {
        let picture = distant_scene();
        let elsewhere = scene_at(400.0);
        let scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(picture.pick_of(0).expect("the picture has that row")),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);
        camera.take_redraw();
        let before = *camera.camera();

        // The choice was made in another picture. Framing it here would move
        // the camera to the extent of whatever occupies that number now.
        assert!(
            !frame_selection(&scene, &elsewhere, &mut camera)
                .expect("having nowhere to go is not a failure")
        );
        assert_eq!(*camera.camera(), before);
        assert!(
            !camera.take_redraw(),
            "an action that did nothing asked for a frame"
        );

        // Nothing chosen is the same answer, however often it is asked.
        let empty = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        for _ in 0..3 {
            assert!(!frame_selection(&empty, &picture, &mut camera).expect("no failure"));
        }
        assert!(!camera.take_redraw());
        assert_eq!(selection_bounds(&empty, &picture), None);
    }

    #[test]
    fn a_row_of_a_replaced_picture_chooses_nothing() {
        let picture = distant_scene();
        let replacement = scene_at(50.0);

        // A list is drawn from one picture and pressed against whatever is
        // current. The row is only a position, so the picture is asked, and a
        // picture that has since been replaced answers for its own rows.
        let row = 0;
        assert!(picture.pick_of(row).is_some());
        let stale = picture.pick_of(row).expect("the old picture had that row");
        assert_eq!(
            replacement.definition(stale),
            None,
            "a row of the previous picture chose something in this one"
        );
    }

    #[test]
    fn clicking_the_background_clears_the_inspector_as_well_as_the_highlight() {
        let picture = distant_scene();
        let mut scene = LiveScene {
            prepared: (),
            catalogue: vec![a_body()],
            faces: FaceNames::default(),
            edges: EdgeNames::default(),
            vertices: VertexNames::default(),
            visibility: Visibility::default(),
            selection: Selection::Definition(
                picture
                    .draws()
                    .first()
                    .expect("the picture draws something")
                    .pick,
            ),
            hovered: Hovered::Nothing,
            sketch_solves: Vec::new(),
            sketch_presentations: Vec::new(),
        };
        assert!(scene.chosen(&picture).is_some());

        // One value answers both halves, so the highlight and the inspector
        // cannot disagree about whether anything is chosen.
        scene.selection = Selection::definition(PickId::NOTHING, &picture);
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.chosen(&picture), None);
    }

    #[test]
    fn four_placements_of_one_definition_are_one_choice() {
        // A definition drawn four times, as an imported pattern is.
        let mut builder = SnapshotBuilder::new();
        let mesh = builder
            .add_mesh(&distant_scene_mesh())
            .expect("the mesh is valid");
        for x in 0..4 {
            builder
                .place(
                    mesh,
                    None,
                    &ferritecad_types::Transform::from_translation(
                        ferritecad_types::Vec3::new(f64::from(x) * 20.0, 0.0, 0.0).expect("finite"),
                    )
                    .expect("finite"),
                    [0.5, 0.5, 0.5],
                )
                .expect("places it");
        }
        let picture = builder.build();
        assert_eq!(picture.draws().len(), 4);

        let entry = CatalogueEntry {
            item: SceneItem::Imported(
                ferritecad_document::ImportedDefinitionRef::new(
                    ferritecad_types::ImportedSourceId::new(),
                    "step.product_definition#39",
                )
                .expect("a key names something"),
            ),
            name: Some("Bolt".to_owned()),
            source_file: Some("04-instance-colours.step".to_owned()),
            solids: Some(1),
        };

        // Clicking any of the four gives the same answer, because a pick names
        // the definition and never the placement.
        for draw in picture.draws() {
            let scene = LiveScene {
                prepared: (),
                catalogue: vec![entry.clone()],
                faces: FaceNames::default(),
                edges: EdgeNames::default(),
                vertices: VertexNames::default(),
                visibility: Visibility::default(),
                selection: Selection::Definition(draw.pick),
                hovered: Hovered::Nothing,
                sketch_solves: Vec::new(),
                sketch_presentations: Vec::new(),
            };
            assert_eq!(scene.chosen(&picture), Some((0, &entry)));
        }
    }

    #[test]
    fn closing_the_dialog_without_choosing_changes_nothing() {
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        assert!(
            input.take_redraw(),
            "the initial frame was not accounted for"
        );
        let a = begin_load(
            &mut loads,
            &mut input,
            Some(Path::new("open.fcad")),
            relay(),
            |_, _| std::thread::spawn(|| {}),
        )
        .expect("a document was named");
        assert!(
            input.take_redraw(),
            "opening changed the line to Loading but requested no frame"
        );
        let waiting = loads.status().clone();

        // A dialog the user closed. No generation, no worker, and above all no
        // change to what the window says about the document already there.
        // The spawn is never called, and if it ever were, the load it started
        // would be recorded and counted below.
        let mut spawned = false;
        let started = begin_load(&mut loads, &mut input, None, relay(), |_, _| {
            spawned = true;
            std::thread::spawn(|| {})
        });
        assert_eq!(started, None);
        assert!(!spawned, "a cancelled dialog started a load");
        assert!(
            !input.take_redraw(),
            "a cancelled dialog asked to redraw an unchanged line"
        );
        assert_eq!(*loads.status(), waiting);
        assert!(loads.accepts(a), "the request in flight was abandoned");
        assert_eq!(loads.running.len(), 1, "a worker appeared from nowhere");

        loads.stop_all();
    }

    #[test]
    fn nothing_is_still_running_when_the_loop_ends() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let mut loads = Loads::default();
        let mut tokens = Vec::new();
        // Counted by the workers themselves, after they have noticed. Each one
        // takes its time about finishing, the way a kernel between two
        // features does, so a `stop_all` that only forgot them rather than
        // waiting for them would reach the assertion below first.
        let tidied = Arc::new(AtomicUsize::new(0));

        for _ in 0..3 {
            let tidied = Arc::clone(&tidied);
            loads.open(Some(Path::new("a.fcad")), relay(), |_, cancel| {
                tokens.push(cancel.clone());
                let cancel = cancel.clone();
                std::thread::spawn(move || {
                    while !cancel.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    std::thread::sleep(Duration::from_millis(150));
                    tidied.fetch_add(1, Ordering::SeqCst);
                })
            });
        }

        // Every worker holds a kernel session. Exiting without waiting would
        // end the process while three of those sessions still held geometry,
        // and the session is what releases it.
        loads.stop_all();
        assert_eq!(
            tidied.load(Ordering::SeqCst),
            3,
            "the loop stopped waiting before its workers had finished"
        );
        assert!(loads.running.is_empty());
        assert!(tokens.iter().all(CancelToken::is_cancelled));
        assert!(!loads.accepts(LoadGeneration(3)), "a load is still current");
    }

    #[test]
    fn a_worker_that_has_finished_is_joined_without_waiting_for_the_others() {
        let mut loads = Loads::default();

        // One that ends by itself, one that will not end until it is told to.
        let done = loads
            .open(Some(Path::new("a.fcad")), relay(), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a load started");
        let mut release = None;
        loads.open(Some(Path::new("a.fcad")), relay(), |_, cancel| {
            let (worker, sender) = held_worker(cancel);
            release = Some(sender);
            worker
        });

        // The held reading is let go after a while by somebody else. Without
        // that, an implementation that waited for it would hang this test
        // instead of failing it, and a hung test is a timeout rather than an
        // answer.
        let release = release.expect("the held worker was spawned");
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            let _ = release.send(());
        });

        // Reaping happens on the event-loop thread, so it may only ever join
        // threads that have already ended. Waiting here for the reading still
        // in flight would be the freeze this whole design exists to avoid.
        let started = Instant::now();
        while loads.running.len() > 1 {
            loads.answered(done, Ok(()));
            std::thread::yield_now();
        }
        let waited = started.elapsed();

        assert_eq!(loads.running.len(), 1, "the reading in flight was joined");
        assert!(
            waited < Duration::from_millis(500),
            "reaping waited {waited:?} for a reading that had not finished"
        );

        loads.stop_all();
    }

    #[test]
    fn many_immediate_reasons_make_one_window_request() {
        let mut frames = FrameScheduler::default();

        assert!(frames.request_now(), "the first reason requested nothing");
        for _ in 0..21 {
            assert!(
                !frames.request_now(),
                "a second reason made a second OS request"
            );
        }

        frames.frame_started();
        assert!(
            frames.request_now(),
            "starting a frame did not open the slot for the next one"
        );
    }

    #[test]
    fn the_earliest_delayed_repaint_is_the_only_timer() {
        let mut frames = FrameScheduler::default();
        let now = Instant::now();
        let late = now + Duration::from_secs(2);
        let early = now + Duration::from_secs(1);

        assert_eq!(frames.request_at(late, now), Some(late));
        assert_eq!(frames.request_at(late + Duration::from_secs(1), now), None);
        assert_eq!(frames.request_at(early, now), Some(early));
        assert!(!frames.take_due(now + Duration::from_millis(999)));
        assert!(frames.take_due(early));
        assert!(!frames.take_due(late));
    }

    #[test]
    fn an_immediate_frame_supersedes_a_delayed_one() {
        let mut frames = FrameScheduler::default();
        let now = Instant::now();
        let later = now + Duration::from_secs(1);

        assert_eq!(frames.request_at(later, now), Some(later));
        assert!(frames.request_now());
        assert!(
            !frames.take_due(later),
            "the cancelled timer requested an extra frame"
        );
        assert_eq!(
            frames.request_at(later + Duration::from_secs(1), later),
            None,
            "a timer was installed while an earlier frame was queued"
        );
    }

    #[test]
    fn the_two_commands_are_told_apart() {
        assert_eq!(
            request([SOLVER_INFO.into()].into_iter()).expect("the diagnostic is a command"),
            Request::SolverInfo
        );
        assert_eq!(
            request(["part.fcad".into()].into_iter()).expect("a document is still a document"),
            Request::Document(PathBuf::from("part.fcad"))
        );

        // Not an option that modifies opening a document. A line naming both a
        // document and this flag has asked for two different things, and doing
        // either of them would be doing half of what was asked.
        for line in [
            [SOLVER_INFO, "part.fcad"],
            [SOLVER_INFO, SOLVER_INFO],
            ["part.fcad", SOLVER_INFO],
        ] {
            let refused = request(line.iter().map(OsString::from))
                .expect_err("two requests on one line are not one request");
            assert!(refused.to_string().contains("usage"), "{line:?}: {refused}");
        }

        // And everything the document route refused, it still refuses. The
        // flag is recognised as itself and not as a prefix or a spelling.
        assert!(request(std::iter::empty()).is_err());
        for flag in [
            "--help",
            "-h",
            "--version",
            "--solver-Info",
            "--solver-info=1",
        ] {
            assert!(
                request([OsString::from(flag)].into_iter()).is_err(),
                "{flag} was taken for a command"
            );
        }
    }

    #[test]
    fn one_document_is_asked_for_and_one_is_accepted() {
        let path = document_argument(["part.fcad".into()].into_iter()).expect("one path is enough");
        assert_eq!(path, PathBuf::from("part.fcad"));

        let missing = document_argument(std::iter::empty()).expect_err("no document was named");
        assert!(missing.to_string().contains("usage"), "{missing}");

        // Two files is a request this viewer cannot honour, and opening the
        // first silently would be honouring half of it.
        let extra = document_argument(["a.fcad".into(), "b.fcad".into()].into_iter())
            .expect_err("two documents must not be taken as one");
        assert!(
            extra.to_string().contains("one document at a time"),
            "{extra}"
        );

        // A question about usage, answered as one rather than as a document
        // called `--help` that could not be found.
        for flag in ["--help", "-h", "--version"] {
            let asked = document_argument([flag.into()].into_iter())
                .expect_err("an option is not a document");
            assert!(asked.to_string().contains("usage"), "{flag}: {asked}");
        }
    }

    #[test]
    fn the_event_loop_keeps_running_while_a_document_is_read() {
        // The load is held open until this thread has already carried on, so
        // the test proves the ordering rather than guessing at a duration: a
        // loader called on the event-loop thread could not reach the assertion
        // below at all.
        let (release, held) = std::sync::mpsc::channel::<()>();
        let (finished, answers) = std::sync::mpsc::channel();

        let worker = spawn_load(
            move || {
                // Waited for with a deadline rather than for ever: a load run
                // on the calling thread must fail this test, not hang it.
                let _ = held.recv_timeout(Duration::from_secs(5));
                Ok(loaded(SnapshotBuilder::new().build()))
            },
            move |loaded| finished.send(loaded).expect("the test is still waiting"),
        );

        assert!(
            answers.try_recv().is_err(),
            "the load answered before it was allowed to run, so it ran here"
        );
        release.send(()).expect("the worker is still waiting");

        let loaded = answers
            .recv_timeout(Duration::from_secs(30))
            .expect("the answer never came back");
        assert!(loaded.is_ok());
        worker.join().expect("the worker finished cleanly");
    }

    #[test]
    fn a_load_that_failed_is_delivered_rather_than_swallowed() {
        let (finished, answers) = std::sync::mpsc::channel();
        let worker = spawn_load(
            || Err(CadError::input("no such document")),
            move |loaded| finished.send(loaded).expect("the test is still waiting"),
        );

        let error = answers
            .recv_timeout(Duration::from_secs(30))
            .expect("the answer never came back")
            .expect_err("this load failed");
        assert!(error.to_string().contains("no such document"));
        worker.join().expect("the worker finished cleanly");
    }

    #[test]
    fn a_scene_the_gpu_refused_moves_neither_the_picture_nor_the_camera() {
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(([-5.0, -5.0, -5.0], [5.0, 5.0, 5.0]))
            .expect("frames the current picture");
        input.take_redraw();
        let before = *input.camera();

        let error = prepare_load::<()>(&input, Ok(loaded(distant_scene())), |_| {
            Err(CadError::rendering("the device refused the upload"))
        })
        .expect_err("a failed upload must not become current");

        assert!(error.to_string().contains("refused the upload"), "{error}");
        assert_eq!(
            *input.camera(),
            before,
            "the camera moved to geometry that never reached the GPU"
        );
        assert!(
            !input.take_redraw(),
            "an upload that changed no picture still requested a frame"
        );
    }

    #[test]
    fn every_texture_command_is_consumed_after_it_is_applied() {
        let context = egui::Context::default();
        let mut output = context.run_ui(Default::default(), |ui| {
            ui.label("this allocates the font atlas");
        });
        let mut textures = std::mem::take(&mut output.textures_delta);
        assert!(
            !textures.set.is_empty(),
            "the gate produced no texture upload"
        );

        let retired = egui::TextureId::Managed(u64::MAX);
        textures.free(retired);
        let mut uploads = 0;
        let mut frees = Vec::new();
        upload_textures(&mut textures, |_, _| uploads += 1);
        free_textures(&mut textures, |id| frees.push(*id));

        assert!(uploads > 0, "the atlas was silently dropped");
        assert_eq!(frees, vec![retired], "a retired texture was kept alive");
        assert!(
            textures.is_empty(),
            "applied texture commands remained marked as pending"
        );
    }

    #[test]
    fn the_pointer_leaving_the_window_is_the_pointer_being_over_nothing() {
        assert_eq!(
            translate(&WindowEvent::CursorLeft {
                device_id: winit::event::DeviceId::dummy(),
            }),
            vec![ViewportEvent::PointerLeft]
        );
    }

    fn pinch(delta: f64, phase: winit::event::TouchPhase) -> WindowEvent {
        WindowEvent::PinchGesture {
            device_id: winit::event::DeviceId::dummy(),
            delta,
            phase,
        }
    }

    #[test]
    fn a_trackpad_pinch_reaches_the_reducer_as_a_zoom() {
        use winit::event::TouchPhase;

        // winit reports magnification as positive, which is the direction a
        // zoom towards the model is asked for in, and the magnification delta
        // is carried across unchanged.
        assert_eq!(
            translate(&pinch(0.25, TouchPhase::Moved)),
            vec![ViewportEvent::Pinch { delta: 0.25 }]
        );
        assert_eq!(
            translate(&pinch(-0.25, TouchPhase::Moved)),
            vec![ViewportEvent::Pinch { delta: -0.25 }]
        );
    }

    #[test]
    fn a_non_moving_pinch_phase_is_not_a_cancelled_mouse_gesture() {
        use winit::event::TouchPhase;

        // A phase with no magnification is not a camera operation and, more
        // importantly, is not focus loss: translating the lifetime of a pinch
        // into a cancellation would drop the pointer and end a mouse drag that
        // was in progress.
        for phase in [
            TouchPhase::Started,
            TouchPhase::Ended,
            TouchPhase::Cancelled,
        ] {
            assert_eq!(
                translate(&pinch(0.0, phase)),
                vec![ViewportEvent::Pinch { delta: 0.0 }],
                "{phase:?} did not translate as a pinch of nothing"
            );
        }

        // And the reducer treats that as nothing happening, with a drag and a
        // pointer that belong to a different device left alone.
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        input
            .frame(([-5.0, -5.0, -5.0], [5.0, 5.0, 5.0]))
            .expect("frames");
        input.handle(ViewportEvent::PointerMoved { x: 300.0, y: 200.0 }, false);
        input.handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        let before = *input.camera();
        let _ = input.take_redraw();

        for phase in [
            TouchPhase::Started,
            TouchPhase::Ended,
            TouchPhase::Cancelled,
        ] {
            for event in translate(&pinch(0.0, phase)) {
                input.handle(event, false);
            }
        }

        assert_eq!(*input.camera(), before, "a pinch phase moved the camera");
        assert!(input.is_dragging(), "a pinch phase ended a mouse drag");
        assert!(!input.take_redraw(), "a pinch phase asked for a frame");

        // The drag still works afterwards, which is what "the pointer was not
        // discarded" actually means.
        input.handle(ViewportEvent::PointerMoved { x: 360.0, y: 240.0 }, false);
        assert_ne!(
            input.camera().eye(),
            before.eye(),
            "the drag stopped turning the model after a pinch phase"
        );
    }

    #[test]
    fn two_fingers_over_a_panel_belong_to_the_panel() {
        let moved = ViewportEvent::PointerMoved { x: 1.0, y: 2.0 };
        for event in [
            ViewportEvent::Pinch { delta: 0.3 },
            ViewportEvent::Wheel { delta: 1.0 },
            ViewportEvent::Roll { radians: 0.3 },
        ] {
            assert!(
                claimed_by_interface(&event, true, false),
                "{event:?} was consumed by the interface and reached the model"
            );
            assert!(
                claimed_by_interface(&event, false, true),
                "{event:?} happened while the interface wanted the pointer"
            );
            assert!(
                !claimed_by_interface(&event, false, false),
                "{event:?} never reached the model"
            );
        }
        // And the rule is not "everything is the interface's": a release ends
        // a gesture the viewport started, wherever the pointer has got to.
        assert!(!claimed_by_interface(
            &ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
            true
        ));
        assert!(claimed_by_interface(&moved, false, true));
    }

    #[test]
    fn focus_loss_translates_to_gesture_cancellation() {
        assert_eq!(
            translate(&WindowEvent::Focused(false)),
            vec![ViewportEvent::GestureCancelled]
        );
        assert!(translate(&WindowEvent::Focused(true)).is_empty());
    }

    #[test]
    fn the_key_and_the_button_ask_for_the_same_thing() {
        // Both routes end in `App::frame_selection`, so what is left to check
        // is that the key the window listens for is the key the panel prints.
        assert!(wants(&Key::Character(FRAME_KEY.into()), false, FRAME_KEY));
        assert!(
            wants(
                &Key::Character(FRAME_KEY.to_lowercase().into()),
                false,
                FRAME_KEY
            ),
            "the button prints one case and a keyboard reports the other"
        );

        // And nothing else is that key, including the view shortcuts beside it.
        for (_, _, key) in VIEWS {
            assert!(!wants(&Key::Character((*key).into()), false, FRAME_KEY));
        }
        assert!(!wants(&Key::Named(NamedKey::Home), false, FRAME_KEY));
        assert!(!wants(&Key::Character("g".into()), false, FRAME_KEY));
    }

    #[test]
    fn the_whole_model_key_is_its_own_and_yields_to_the_interface() {
        assert!(wants(
            &Key::Character(FRAME_ALL_KEY.into()),
            false,
            FRAME_ALL_KEY
        ));
        assert!(
            wants(
                &Key::Character(FRAME_ALL_KEY.to_lowercase().into()),
                false,
                FRAME_ALL_KEY
            ),
            "the button prints one case and a keyboard reports the other"
        );

        // A key the interface took is not a camera command: a focused text
        // control that accepted an `A` did not ask to reframe the model.
        assert!(
            !wants(&Key::Character(FRAME_ALL_KEY.into()), true, FRAME_ALL_KEY),
            "a key the interface claimed still moved the model camera"
        );

        // Nothing else is that key, and it is not the other framing key.
        assert!(!wants(
            &Key::Character(FRAME_KEY.into()),
            false,
            FRAME_ALL_KEY
        ));
        assert!(!wants(
            &Key::Character(FRAME_ALL_KEY.into()),
            false,
            FRAME_KEY
        ));
        for (_, _, key) in VIEWS {
            assert!(!wants(&Key::Character((*key).into()), false, FRAME_ALL_KEY));
        }
        // Home still means the isometric view and nothing else.
        assert!(!wants(&Key::Named(NamedKey::Home), false, FRAME_ALL_KEY));
        assert_eq!(
            named_view(&Key::Named(NamedKey::Home)),
            Some(StandardView::Isometric)
        );
    }

    #[test]
    fn the_frame_key_does_not_bypass_the_interface_that_claimed_it() {
        assert!(wants(&Key::Character(FRAME_KEY.into()), false, FRAME_KEY));
        assert!(
            !wants(&Key::Character(FRAME_KEY.into()), true, FRAME_KEY),
            "a key the interface claimed still moved the model camera"
        );
    }

    #[test]
    fn every_shortcut_printed_on_the_panel_reaches_that_view() {
        for (view, name, shortcut) in VIEWS {
            let key = Key::Character((*shortcut).into());
            assert_eq!(
                named_view(&key),
                Some(*view),
                "the {name} button prints {shortcut}, but that key selects something else"
            );
        }
    }

    // -----------------------------------------------------------------------
    // What a solve found out, from the document that was solved to the screen
    // -----------------------------------------------------------------------
    //
    // A picture of a solid says nothing about the drawing behind it. These
    // gates are about the third thing a load carries beside the picture and
    // what a click means: what the one rebuild that drew it found out about
    // each constrained sketch, and whether the window can still say it.
    //
    // Every document here is written to a real file and read back through the
    // real loader, and every solve is a real planegcs solve. Nothing in this
    // section may ask a solver anything of its own: if these facts could be
    // obtained by asking again, carrying them would be pointless.

    /// The corners of the plate every constrained document here draws.
    const SOLVE_CORNERS: [(f64, f64); 4] = [(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)];
    /// The width the sizing constraint asks for.
    const SOLVE_WIDTH: f64 = 60.0;

    /// Whether a real solver is here, and whether its absence is allowed.
    ///
    /// Under `FERRITECAD_REQUIRE_PLANEGCS=1` a missing library is a failure
    /// rather than a reason to return early: a run whose job is to show that
    /// a solved document reaches the window cannot pass by never solving one.
    fn solver_ready() -> bool {
        if ferritecad_sketch_solver::is_available() {
            return true;
        }
        assert!(
            !ferritecad_sketch_solver::is_required(),
            "FERRITECAD_REQUIRE_PLANEGCS=1 was set, so no gate may skip: a window that has \
             never been handed a real solve has not been shown to carry one"
        );
        eprintln!("skipped: this build has no sketch solver");
        false
    }

    macro_rules! solver_or_skip {
        () => {
            if !solver_ready() {
                return;
            }
        };
    }

    /// One straight side of the plate.
    fn solve_line(
        id: ferritecad_types::StableEntityId,
        start: (f64, f64),
        end: (f64, f64),
    ) -> ferritecad_document::SketchCurve {
        ferritecad_document::SketchCurve {
            id,
            construction: false,
            geometry: ferritecad_document::SketchGeometry::Line {
                start: ferritecad_document::Point2::new(start.0, start.1).expect("finite"),
                end: ferritecad_document::Point2::new(end.0, end.1).expect("finite"),
            },
        }
    }

    /// Four sides, each with an identity of its own.
    fn solve_curves() -> Vec<ferritecad_document::SketchCurve> {
        (0..4)
            .map(|index| {
                solve_line(
                    ferritecad_types::StableEntityId::new(),
                    SOLVE_CORNERS[index],
                    SOLVE_CORNERS[(index + 1) % SOLVE_CORNERS.len()],
                )
            })
            .collect()
    }

    /// Nine rules that close and square the plate without sizing it, which
    /// leaves it free in two directions.
    fn solve_frame(
        edges: &[ferritecad_types::StableEntityId],
    ) -> Vec<ferritecad_document::SketchConstraintRule> {
        use ferritecad_document::SketchConstraintRule as Rule;
        use ferritecad_document::SketchPointSelector::{End, Start};
        let at = ferritecad_document::SketchPointRef::new;
        vec![
            Rule::Coincident {
                a: at(edges[0], End),
                b: at(edges[1], Start),
            },
            Rule::Coincident {
                a: at(edges[1], End),
                b: at(edges[2], Start),
            },
            Rule::Coincident {
                a: at(edges[2], End),
                b: at(edges[3], Start),
            },
            Rule::Coincident {
                a: at(edges[3], End),
                b: at(edges[0], Start),
            },
            Rule::Fixed {
                point: at(edges[0], Start),
                x: 0.0,
                y: 0.0,
            },
            Rule::Horizontal {
                a: at(edges[0], Start),
                b: at(edges[0], End),
            },
            Rule::Vertical {
                a: at(edges[1], Start),
                b: at(edges[1], End),
            },
            Rule::Horizontal {
                a: at(edges[2], Start),
                b: at(edges[2], End),
            },
            Rule::Vertical {
                a: at(edges[3], Start),
                b: at(edges[3], End),
            },
        ]
    }

    /// The rule that says how wide the plate is.
    fn solve_width(
        edges: &[ferritecad_types::StableEntityId],
    ) -> ferritecad_document::SketchConstraintRule {
        ferritecad_document::SketchConstraintRule::Distance {
            a: ferritecad_document::SketchPointRef::new(
                edges[0],
                ferritecad_document::SketchPointSelector::Start,
            ),
            b: ferritecad_document::SketchPointRef::new(
                edges[0],
                ferritecad_document::SketchPointSelector::End,
            ),
            distance: SOLVE_WIDTH,
        }
    }

    /// Each rule given the identity the document stores it under.
    fn solve_named(
        rules: Vec<ferritecad_document::SketchConstraintRule>,
    ) -> Vec<ferritecad_document::SketchConstraint> {
        rules
            .into_iter()
            .map(|rule| ferritecad_document::SketchConstraint {
                id: ferritecad_types::StableEntityId::new(),
                rule,
            })
            .collect()
    }

    /// One sketch, one extrusion and one body per entry, written to a file.
    ///
    /// Returns the identity of each sketch, in the order the document stores
    /// them, which is what a gate about order compares against.
    fn write_sketches(
        path: &Path,
        sketches: Vec<(
            Option<&str>,
            Vec<ferritecad_document::SketchCurve>,
            Vec<ferritecad_document::SketchConstraint>,
        )>,
    ) -> Vec<ferritecad_types::ObjectId> {
        use ferritecad_document::{
            Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression,
            Extrude, ObjectPayload, Sketch, SolidOperation,
        };
        use ferritecad_types::{ObjectId, Transform};

        let mut document = Document::create(path).expect("creates");
        let plane = ObjectId::new();
        let ids: Vec<ObjectId> = sketches.iter().map(|_| ObjectId::new()).collect();
        document
            .write(|w| {
                w.put_object(
                    plane,
                    None,
                    0,
                    Some("XY"),
                    &ObjectPayload::DatumPlane(DatumPlane {
                        placement: Transform::IDENTITY,
                    }),
                )?;
                let mut order = 1i64;
                for (index, (name, curves, constraints)) in sketches.iter().enumerate() {
                    let sketch = ids[index];
                    let extrude = ObjectId::new();
                    let body = ObjectId::new();
                    w.put_object(
                        sketch,
                        None,
                        order,
                        *name,
                        &ObjectPayload::Sketch(Sketch {
                            plane,
                            curves: curves.clone(),
                            constraints: constraints.clone(),
                        }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: sketch,
                        dependency: plane,
                        role: DependencyRole::Plane,
                    })?;
                    w.put_object(
                        extrude,
                        None,
                        order + 1,
                        Some("Extrude"),
                        &ObjectPayload::Extrude(Extrude {
                            profile: sketch,
                            end_condition: EndCondition::Blind {
                                distance: Expression::constant(10.0)?,
                            },
                            reversed: false,
                            operation: SolidOperation::NewBody,
                            target_body: None,
                        }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: extrude,
                        dependency: sketch,
                        role: DependencyRole::Profile,
                    })?;
                    w.put_object(
                        body,
                        None,
                        order + 2,
                        Some("Plate"),
                        &ObjectPayload::Body(Body {
                            tip_feature: Some(extrude),
                        }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: body,
                        dependency: extrude,
                        role: DependencyRole::BodyTip,
                    })?;
                    order += 3;
                }
                Ok(())
            })
            .expect("populates");
        document.close().expect("closes");
        ids
    }

    /// The document the loader is given, read exactly as the viewer reads one.
    fn load_document(path: &Path) -> LoadedScene {
        use ferritecad_kernel::mock::MockKernel;

        snapshot_of(
            path,
            &mut MockKernel::new(),
            |_: &mut MockKernel, _: &[u8]| {
                Err(CadError::unsupported("these documents hold no imports"))
            },
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("the document loads")
    }

    /// A document with one sketch that is square, closed and free in one
    /// direction, and that says its width twice.
    ///
    /// The repetition is deliberate: it gives the solve something durable to
    /// report beyond a number, and the identifier it reports is one this test
    /// wrote and can therefore recognise.
    fn one_solved_sketch(
        directory: &tempfile::TempDir,
        name: Option<&str>,
    ) -> (
        LoadedScene,
        ferritecad_types::ObjectId,
        ferritecad_types::StableEntityId,
    ) {
        let path = directory.path().join("plate.fcad");
        let curves = solve_curves();
        let edges: Vec<ferritecad_types::StableEntityId> =
            curves.iter().map(|curve| curve.id).collect();
        let mut rules = solve_frame(&edges);
        rules.push(solve_width(&edges));
        rules.push(solve_width(&edges));
        let constraints = solve_named(rules);
        let repeated = constraints[10].id;
        let sketches = write_sketches(&path, vec![(name, curves, constraints)]);
        (load_document(&path), sketches[0], repeated)
    }

    /// A live scene holding one document's picture and its account of it.
    fn live_scene_of(loaded: LoadedScene) -> (LiveScene<()>, ViewportInput) {
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);
        let mut scene = empty_scene();
        let prepared = prepare_load(&camera, Ok(loaded), |_| Ok(()));
        commit_scene(&mut scene, &mut camera, prepared).expect("the document commits");
        (scene, camera)
    }

    /// Every word the section actually drew, in the order it drew them.
    ///
    /// Read out of the shapes egui produced rather than out of the values that
    /// went in: this is a gate about what a person reads.
    fn section_words(solves: &[SketchSolveFacts]) -> Vec<String> {
        let words = solve_words_of(solves);
        let repeated: Vec<Vec<ferritecad_ui::RedundantExplanation<'_>>> =
            words.iter().map(SolveWords::repeated).collect();
        let sketches = solved_sketches(&words, &repeated);
        laid_out(|ui| ferritecad_ui::sketch_solves_panel(ui, &sketches))
    }

    /// Every word one pass of egui actually drew, in the order it drew them.
    ///
    /// Read out of the shapes egui produced rather than out of the values that
    /// went in: these are gates about what a person reads, and a section that
    /// laid nothing out would satisfy any assertion made against its input.
    fn laid_out(section: impl FnMut(&mut egui::Ui)) -> Vec<String> {
        let context = egui::Context::default();
        // A frame first, so the fonts are loaded: with none loaded every line
        // comes back empty and any assertion about absence would pass.
        let mut warm = context.run_ui(egui::RawInput::default(), |_| {});
        warm.textures_delta.clear();
        let mut output = context.run_ui(egui::RawInput::default(), section);
        output.textures_delta.clear();

        fn collect(shape: &egui::Shape, into: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => into.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, into);
                    }
                }
                _ => {}
            }
        }
        let mut words = Vec::new();
        for clipped in output.shapes {
            collect(&clipped.shape, &mut words);
        }
        words
    }

    /// Every word the account of a failed Open drew, exactly as the window
    /// builds it: the application's own record, borrowed for one frame.
    fn failure_words(loads: &Loads) -> Vec<String> {
        let rules = loads
            .conflict()
            .map(OpenConflict::rules)
            .unwrap_or_default();
        let failure = loads.conflict().map(|conflict| conflict.shown(&rules));
        laid_out(|ui| ferritecad_ui::open_failure_panel(ui, failure))
    }

    /// Everything that account drew, as one string to search.
    fn failure_page(loads: &Loads) -> String {
        failure_words(loads).join("\n")
    }

    /// Everything the section drew, as one string to search.
    fn section_page(solves: &[SketchSolveFacts]) -> String {
        section_words(solves).join("\n")
    }

    /// Two curve identities of the shape a document actually stores.
    ///
    /// Made rather than written down, so nothing here can quietly depend on
    /// the text of a particular identifier.
    fn two_curves() -> (
        ferritecad_types::StableEntityId,
        ferritecad_types::StableEntityId,
    ) {
        (
            ferritecad_types::StableEntityId::new(),
            ferritecad_types::StableEntityId::new(),
        )
    }

    /// The eight families the document stores, each with the one sentence the
    /// window is to say for it.
    ///
    /// One list, read by every gate that checks those words, wherever the
    /// window says them. Two lists would be two vocabularies, and the gate
    /// that was not updated would be the one certifying the wrong one.
    fn eight_families(
        first: ferritecad_types::StableEntityId,
        second: ferritecad_types::StableEntityId,
    ) -> Vec<(ferritecad_document::SketchConstraintRule, String)> {
        use ferritecad_document::SketchConstraintRule as Rule;
        use ferritecad_document::SketchPointSelector::{At, End, Start};
        use ferritecad_document::{SketchPointRef, SketchSegmentRef};

        let point = SketchPointRef::new;
        let segment = SketchSegmentRef::new;
        // What kind of relationship it is, which parts of the drawing it holds
        // together, and the size it asks for when it asks for one.
        let families = vec![
            (
                Rule::Coincident {
                    a: point(first, End),
                    b: point(second, Start),
                },
                format!("Coincident: {first}.end and {second}.start are the same point"),
            ),
            (
                Rule::Fixed {
                    point: point(first, At),
                    x: -4.5,
                    y: 12.0,
                },
                format!("Fixed: {first}.at is pinned at x -4.5 mm, y 12 mm"),
            ),
            (
                Rule::Distance {
                    a: point(first, Start),
                    b: point(first, End),
                    distance: 12.7,
                },
                format!("Distance: {first}.start and {first}.end are 12.7 mm apart"),
            ),
            (
                Rule::Horizontal {
                    a: point(first, Start),
                    b: point(second, End),
                },
                format!("Horizontal: {first}.start and {second}.end share a y coordinate"),
            ),
            (
                Rule::Vertical {
                    a: point(first, Start),
                    b: point(second, End),
                },
                format!("Vertical: {first}.start and {second}.end share an x coordinate"),
            ),
            (
                Rule::EqualLength {
                    a: segment(point(first, Start), point(first, End)),
                    b: segment(point(second, Start), point(second, End)),
                },
                format!(
                    "Equal length: {first}.start to {first}.end is as long as {second}.start to \
                     {second}.end"
                ),
            ),
            (
                Rule::Perpendicular {
                    a: segment(point(first, Start), point(first, End)),
                    b: segment(point(second, Start), point(second, End)),
                },
                format!(
                    "Perpendicular: {first}.start to {first}.end meets {second}.start to \
                     {second}.end at a right angle"
                ),
            ),
            (
                Rule::Parallel {
                    a: segment(point(first, Start), point(first, End)),
                    b: segment(point(second, Start), point(second, End)),
                },
                format!(
                    "Parallel: {first}.start to {first}.end runs in the same direction as \
                     {second}.start to {second}.end"
                ),
            ),
        ];
        assert_eq!(families.len(), 8, "the document stores eight families");
        families
    }

    #[test]
    fn every_family_the_document_can_store_is_said_as_a_finished_sentence() {
        let (first, second) = two_curves();
        let cases = eight_families(first, second);

        let mut said: Vec<String> = Vec::new();
        for (rule, expected) in cases {
            let sentence = describe_constraint(&rule);
            assert_eq!(sentence, expected);
            said.push(sentence);
        }
        // No two families are described the same way, so a match arm answering
        // for its neighbour cannot pass by saying something plausible.
        let mut distinct = said.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            said.len(),
            "two families are described identically: {said:?}"
        );
        // And nothing in any of them is a shape a programmer prints.
        for sentence in &said {
            for forbidden in [
                "SketchConstraintRule",
                "SketchPointRef",
                "SketchSegmentRef",
                "ConstraintId",
                "PointId",
                "StableEntityId",
                "{",
                "}",
                "(",
                "[",
                "Some",
                "planegcs",
                "index",
                "ordinal",
            ] {
                assert!(
                    !sentence.contains(forbidden),
                    "a description printed {forbidden:?} instead of saying something: {sentence}"
                );
            }
        }
    }

    #[test]
    fn a_description_reads_the_rule_out_in_the_order_the_document_stores_it() {
        use ferritecad_document::SketchConstraintRule as Rule;
        use ferritecad_document::SketchPointSelector::{End, Start};
        use ferritecad_document::{SketchPointRef, SketchSegmentRef};

        let (first, second) = two_curves();
        let point = SketchPointRef::new;

        // Coincident says the same thing either way round. The document does
        // not, and neither may the window: the order stored is the order read
        // out, or the window has started editing what it was given.
        let forwards = describe_constraint(&Rule::Coincident {
            a: point(first, End),
            b: point(second, Start),
        });
        let backwards = describe_constraint(&Rule::Coincident {
            a: point(second, Start),
            b: point(first, End),
        });
        assert_ne!(
            forwards, backwards,
            "the two arguments of a constraint were reordered into a house style"
        );
        assert!(forwards.find(&format!("{first}.end")) < forwards.find(&format!("{second}.start")));

        // The two ends of one segment, likewise: start then end, because that
        // is what is written down.
        let run = describe_constraint(&Rule::Parallel {
            a: SketchSegmentRef::new(point(first, Start), point(first, End)),
            b: SketchSegmentRef::new(point(second, End), point(second, Start)),
        });
        assert!(
            run.contains(&format!("{first}.start to {first}.end"))
                && run.contains(&format!("{second}.end to {second}.start")),
            "a segment was read out from whichever end looked tidier: {run}"
        );
    }

    #[test]
    fn a_size_is_said_exactly_as_it_was_stored_and_with_its_unit() {
        use ferritecad_document::SketchConstraintRule as Rule;
        use ferritecad_document::SketchPointRef;
        use ferritecad_document::SketchPointSelector::{At, End, Start};

        let (first, _) = two_curves();
        let point = SketchPointRef::new;
        for (distance, expected) in [(60.0, "60 mm"), (12.5, "12.5 mm"), (0.125, "0.125 mm")] {
            let said = describe_constraint(&Rule::Distance {
                a: point(first, Start),
                b: point(first, End),
                distance,
            });
            assert!(
                said.contains(expected),
                "a size of {distance} was not said as {expected}: {said}"
            );
        }
        // The other family that carries numbers, and both of its numbers.
        let pinned = describe_constraint(&Rule::Fixed {
            point: point(first, At),
            x: 3.25,
            y: -7.0,
        });
        assert!(
            pinned.contains("x 3.25 mm") && pinned.contains("y -7 mm"),
            "a pinned point was not said at the place it is pinned to: {pinned}"
        );
    }

    #[test]
    fn a_repeated_constraint_reaches_the_window_explained_rather_than_merely_named() {
        solver_or_skip!();

        // The defect this closes: the window named the repeated constraint by
        // its identifier and stopped there, so a person reading their own
        // drawing was told that something repeats without being told what.
        //
        // A whole document, a real solve and the real section: what is checked
        // is the words that were laid out, not the values that went in.
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        let curves = solve_curves();
        let edges: Vec<ferritecad_types::StableEntityId> =
            curves.iter().map(|curve| curve.id).collect();
        let mut rules = solve_frame(&edges);
        rules.push(solve_width(&edges));
        rules.push(solve_width(&edges));
        let constraints = solve_named(rules);
        let repeated = constraints[10].id;
        write_sketches(&path, vec![(Some("Profile"), curves, constraints)]);

        let (scene, _camera) = live_scene_of(load_document(&path));
        let page = section_page(&scene.sketch_solves);

        assert!(
            page.contains(&repeated.to_string()),
            "the repeated constraint the document stores was not named:\n{page}"
        );
        assert!(
            page.contains("Distance"),
            "the window says something repeats without saying what kind of thing it is:\n{page}"
        );
        assert!(
            page.contains(&format!("{}.start", edges[0])),
            "the window does not say which points the repeated constraint is about:\n{page}"
        );
        assert!(
            page.contains(&format!("{}.end", edges[0])),
            "the window does not say which points the repeated constraint is about:\n{page}"
        );
        assert!(
            page.contains("60 mm"),
            "the window does not say the size the repeated constraint asks for:\n{page}"
        );
    }

    #[test]
    fn a_constrained_document_reaches_the_window_and_says_what_its_solve_found_out() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let before = ferritecad_sketch_solver::native_solves();
        let (loaded, sketch, repeated) = one_solved_sketch(&directory, Some("Profile"));
        let picture = loaded.snapshot.clone();
        let solved = ferritecad_sketch_solver::native_solves() - before;
        assert_eq!(
            solved, 1,
            "the document holds one constrained sketch and the load solved it {solved} times"
        );

        let (scene, _camera) = live_scene_of(loaded);

        // The facts survived the handoff, whole and in the document's words.
        assert_eq!(
            scene.sketch_solves.len(),
            1,
            "the window was handed a picture without the account that came with it"
        );
        assert_eq!(scene.sketch_solves[0].sketch(), sketch);
        assert_eq!(scene.sketch_solves[0].name(), Some("Profile"));
        assert_eq!(scene.sketch_solves[0].report().degrees_of_freedom(), 1);
        assert_eq!(scene.sketch_solves[0].report().redundant(), [repeated]);

        // And they reached a screen, in a person's words rather than a
        // programmer's.
        let page = section_page(&scene.sketch_solves);
        assert!(
            page.contains("Sketch solves"),
            "the section did not name itself:\n{page}"
        );
        assert!(
            page.contains("Profile"),
            "the sketch was not named:\n{page}"
        );
        assert!(
            page.contains(&sketch.to_string()),
            "the identifier the document stores for the sketch did not reach the screen:\n{page}"
        );
        assert!(
            page.contains("Under-constrained"),
            "a sketch with a degree of freedom left was not said to have one:\n{page}"
        );
        assert!(
            section_words(&scene.sketch_solves).iter().any(|w| w == "1"),
            "the exact number of degrees of freedom was not shown:\n{page}"
        );
        assert!(
            page.contains(&repeated.to_string()),
            "the repeated constraint the document stores was not named:\n{page}"
        );

        // And no solver was asked anything to put them there.
        assert_eq!(
            ferritecad_sketch_solver::native_solves() - before,
            1,
            "showing what a solve found out solved the sketch again"
        );

        // What is drawn is what the document draws, and no part of it was
        // decided by what the solve found out: a picture that arrived with an
        // account of its sketches is drawn exactly as one that arrived without
        // one.
        assert!(
            !scene.visibility.anything_hidden(),
            "a document arrived with parts already missing"
        );
        assert!(
            scene.visibility.shows(0, &picture),
            "what a solve found out decided what is on screen"
        );
    }

    #[test]
    fn opening_a_document_replaces_what_the_last_solve_said_with_this_one() {
        solver_or_skip!();

        let first = tempfile::tempdir().expect("a temporary directory is available");
        let second = tempfile::tempdir().expect("a temporary directory is available");
        let (old, old_sketch, _) = one_solved_sketch(&first, Some("Old"));
        let (mut scene, mut camera) = live_scene_of(old);
        assert_eq!(
            scene.sketch_solves.len(),
            1,
            "the first document's account never reached the window"
        );
        assert_eq!(scene.sketch_solves[0].sketch(), old_sketch);

        let (new, new_sketch, new_repeated) = one_solved_sketch(&second, Some("New"));
        let prepared = prepare_load(&camera, Ok(new), |_| Ok(()));
        commit_scene(&mut scene, &mut camera, prepared).expect("the second document commits");

        // One operation, both halves: the picture is the new one and so is
        // everything said about it. A window that kept the old account beside
        // the new model would be describing a file nobody has open.
        assert_eq!(scene.sketch_solves.len(), 1);
        assert_eq!(scene.sketch_solves[0].sketch(), new_sketch);
        assert_eq!(scene.sketch_solves[0].name(), Some("New"));
        assert_eq!(scene.sketch_solves[0].report().redundant(), [new_repeated]);
        let page = section_page(&scene.sketch_solves);
        assert!(
            !page.contains(&old_sketch.to_string()),
            "the window is still showing the last document's sketch:\n{page}"
        );
        assert!(
            !page.contains("Old"),
            "the window is still showing the last document's sketch by name:\n{page}"
        );
    }

    #[test]
    fn a_document_with_nothing_to_solve_takes_away_what_the_last_one_said() {
        solver_or_skip!();

        let first = tempfile::tempdir().expect("a temporary directory is available");
        let second = tempfile::tempdir().expect("a temporary directory is available");
        let (old, old_sketch, _) = one_solved_sketch(&first, Some("Profile"));
        let (mut scene, mut camera) = live_scene_of(old);
        assert_eq!(scene.sketch_solves.len(), 1);

        // A drawing with no constraints on it: nothing solved it, so there is
        // nothing to report about it.
        let path = second.path().join("plain.fcad");
        write_sketches(&path, vec![(Some("Plain"), solve_curves(), Vec::new())]);
        let prepared = prepare_load(&camera, Ok(load_document(&path)), |_| Ok(()));
        commit_scene(&mut scene, &mut camera, prepared).expect("the plain document commits");

        assert!(
            scene.sketch_solves.is_empty(),
            "a document with nothing solved in it left the last document's account on screen: \
             {:?}",
            scene.sketch_solves
        );
        let page = section_page(&scene.sketch_solves);
        assert!(
            !page.contains(&old_sketch.to_string()),
            "the last document's sketch is still on screen:\n{page}"
        );
        assert!(
            page.contains("No solved constrained sketches"),
            "a document with no constrained sketch left the section blank:\n{page}"
        );
    }

    #[test]
    fn a_load_that_failed_leaves_what_the_last_solve_said_alone() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, sketch, repeated) = one_solved_sketch(&directory, Some("Profile"));
        let (mut scene, mut camera) = live_scene_of(loaded);
        let kept = scene.sketch_solves.clone();
        assert_eq!(
            kept.len(),
            1,
            "the document's account never reached the window, so there is nothing here to keep"
        );

        let refused = prepare_load(
            &camera,
            Err(CadError::input("this is not a document")),
            |_| Ok(()),
        );
        let error = commit_scene(&mut scene, &mut camera, refused)
            .expect_err("a failed load must not commit");
        assert!(error.to_string().contains("not a document"));

        // The whole of it, and not merely a list of the same length: a
        // document that could not be read leaves the drawing a person was
        // reading exactly as it was.
        assert_eq!(scene.sketch_solves, kept);
        assert_eq!(scene.sketch_solves[0].sketch(), sketch);
        assert_eq!(scene.sketch_solves[0].report().redundant(), [repeated]);
    }

    #[test]
    fn a_load_that_was_given_up_on_leaves_what_the_last_solve_said_alone() {
        solver_or_skip!();

        let first = tempfile::tempdir().expect("a temporary directory is available");
        let second = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, sketch, _) = one_solved_sketch(&first, Some("Profile"));
        let (mut scene, mut camera) = live_scene_of(loaded);
        let kept = scene.sketch_solves.clone();
        assert_eq!(
            kept.len(),
            1,
            "the document's account never reached the window, so there is nothing here to keep"
        );

        // A second document that was read and then abandoned: the answer
        // arrives, and the window has already stopped waiting for it. Applying
        // it is what `Loads::accepts` refuses, so the account never reaches
        // the scene and the previous one is untouched.
        let mut loads = Loads::default();
        let mut holds = Vec::new();
        let stale = loads
            .open(Some(Path::new("a.fcad")), relay(), |_, cancel| {
                let (worker, release) = held_worker(cancel);
                holds.push(release);
                worker
            })
            .expect("a document was named");
        loads.open(Some(Path::new("b.fcad")), relay(), |_, cancel| {
            let (worker, release) = held_worker(cancel);
            holds.push(release);
            worker
        });
        assert!(
            !loads.accepts(stale),
            "the abandoned request is still current"
        );

        let (abandoned, other, _) = one_solved_sketch(&second, Some("Abandoned"));
        if loads.accepts(stale) {
            let prepared = prepare_load(&camera, Ok(abandoned), |_| Ok(()));
            commit_scene(&mut scene, &mut camera, prepared).expect("commits");
        }
        loads.stop_all();

        assert_eq!(scene.sketch_solves, kept);
        assert_eq!(scene.sketch_solves[0].sketch(), sketch);
        let page = section_page(&scene.sketch_solves);
        assert!(
            !page.contains(&other.to_string()),
            "a document the window stopped waiting for reached the screen:\n{page}"
        );
    }

    #[test]
    fn a_picture_that_could_not_be_prepared_leaves_what_the_last_solve_said_alone() {
        solver_or_skip!();

        let first = tempfile::tempdir().expect("a temporary directory is available");
        let second = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, sketch, _) = one_solved_sketch(&first, Some("Profile"));
        let (mut scene, mut camera) = live_scene_of(loaded);
        let kept = scene.sketch_solves.clone();
        assert_eq!(
            kept.len(),
            1,
            "the document's account never reached the window, so there is nothing here to keep"
        );
        let framing = *camera.camera();

        // The document read, solved and accepted, and then the graphics device
        // refused the upload. Nothing of it becomes current: not the picture,
        // not the camera, and not what its solve found out.
        let (arriving, other, _) = one_solved_sketch(&second, Some("Refused"));
        let prepared = prepare_load(&camera, Ok(arriving), |_| {
            Err(CadError::rendering("the device refused the buffers"))
        });
        let error = commit_scene(&mut scene, &mut camera, prepared)
            .expect_err("an upload that failed must not commit");
        assert!(error.to_string().contains("refused the buffers"));

        assert_eq!(scene.sketch_solves, kept);
        assert_eq!(scene.sketch_solves[0].sketch(), sketch);
        assert_eq!(*camera.camera(), framing);
        let page = section_page(&scene.sketch_solves);
        assert!(
            !page.contains(&other.to_string()),
            "a document whose picture was refused reached the screen anyway:\n{page}"
        );
    }

    // -----------------------------------------------------------------
    // The drawings behind the picture, across one document replacing another
    // -----------------------------------------------------------------

    /// Which sketches a window is holding drawings of, in its own order.
    fn drawings<P>(scene: &LiveScene<P>) -> Vec<ferritecad_types::ObjectId> {
        scene
            .sketch_presentations
            .iter()
            .map(|drawing| drawing.sketch())
            .collect()
    }

    /// A document of `count` unconstrained sketches, loaded as the viewer
    /// loads one.
    ///
    /// Unconstrained on purpose: what a drawing looks like is not a thing only
    /// a solver could have said, so these gates about one document replacing
    /// another hold in a build that never linked planegcs.
    fn plain_document(
        directory: &tempfile::TempDir,
        file: &str,
        count: usize,
    ) -> (LoadedScene, Vec<ferritecad_types::ObjectId>) {
        let path = directory.path().join(file);
        let sketches = write_sketches(
            &path,
            (0..count)
                .map(|_| (Some("Plain"), solve_curves(), Vec::new()))
                .collect(),
        );
        (load_document(&path), sketches)
    }

    #[test]
    fn a_document_that_opens_replaces_the_drawings_with_its_own() {
        let first = tempfile::tempdir().expect("a temporary directory is available");
        let second = tempfile::tempdir().expect("a temporary directory is available");
        let (old, was) = plain_document(&first, "old.fcad", 1);
        let (mut scene, mut camera) = live_scene_of(old);
        assert_eq!(
            drawings(&scene),
            was,
            "the first document's drawing never reached the window, so there is nothing here to \
             replace"
        );

        let (arriving, now) = plain_document(&second, "new.fcad", 2);
        let prepared = prepare_load(&camera, Ok(arriving), |_| Ok(()));
        commit_scene(&mut scene, &mut camera, prepared).expect("the second document commits");

        // Replaced whole, with the picture. A window holding the last
        // document's drawings beside this document's model would be two files
        // at once, and the drawings are the half nobody would notice.
        assert_eq!(drawings(&scene), now);
        assert!(
            !drawings(&scene).contains(&was[0]),
            "the window is still holding the drawing of a document nobody has open"
        );
    }

    #[test]
    fn a_load_that_failed_leaves_the_drawings_alone() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, sketches) = plain_document(&directory, "plain.fcad", 1);
        let (mut scene, mut camera) = live_scene_of(loaded);
        let kept = scene.sketch_presentations.clone();
        assert_eq!(
            drawings(&scene),
            sketches,
            "the document's drawing never reached the window, so there is nothing here to keep"
        );

        let refused = prepare_load(
            &camera,
            Err(CadError::input("this is not a document")),
            |_| Ok(()),
        );
        let error = commit_scene(&mut scene, &mut camera, refused)
            .expect_err("a failed load must not commit");
        assert!(error.to_string().contains("not a document"));

        // The whole of it, and not merely a list of the same length.
        assert_eq!(scene.sketch_presentations, kept);
    }

    #[test]
    fn a_load_that_was_given_up_on_leaves_the_drawings_alone() {
        let first = tempfile::tempdir().expect("a temporary directory is available");
        let second = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, _) = plain_document(&first, "plain.fcad", 1);
        let (mut scene, mut camera) = live_scene_of(loaded);
        let kept = scene.sketch_presentations.clone();
        assert_eq!(kept.len(), 1, "the gate begins with a drawing to keep");

        // A second document read and then abandoned: the answer arrives, and
        // the window has already stopped waiting for it.
        let mut loads = Loads::default();
        let mut holds = Vec::new();
        let stale = loads
            .open(Some(Path::new("a.fcad")), relay(), |_, cancel| {
                let (worker, release) = held_worker(cancel);
                holds.push(release);
                worker
            })
            .expect("a document was named");
        loads.open(Some(Path::new("b.fcad")), relay(), |_, cancel| {
            let (worker, release) = held_worker(cancel);
            holds.push(release);
            worker
        });
        assert!(
            !loads.accepts(stale),
            "the abandoned request is still current"
        );

        let (abandoned, other) = plain_document(&second, "abandoned.fcad", 1);
        if loads.accepts(stale) {
            let prepared = prepare_load(&camera, Ok(abandoned), |_| Ok(()));
            commit_scene(&mut scene, &mut camera, prepared).expect("commits");
        }
        loads.stop_all();

        assert_eq!(scene.sketch_presentations, kept);
        assert!(
            !drawings(&scene).contains(&other[0]),
            "a document the window stopped waiting for reached the screen"
        );
    }

    #[test]
    fn a_picture_that_could_not_be_prepared_leaves_the_drawings_alone() {
        let first = tempfile::tempdir().expect("a temporary directory is available");
        let second = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, _) = plain_document(&first, "plain.fcad", 1);
        let (mut scene, mut camera) = live_scene_of(loaded);
        let kept = scene.sketch_presentations.clone();
        assert_eq!(kept.len(), 1, "the gate begins with a drawing to keep");
        let framing = *camera.camera();

        // The document read and accepted, and then the graphics device refused
        // the upload. Nothing of it becomes current: not the picture, not the
        // camera, and not the drawings behind it.
        let (arriving, other) = plain_document(&second, "refused.fcad", 1);
        let prepared = prepare_load(&camera, Ok(arriving), |_| {
            Err(CadError::rendering("the device refused the buffers"))
        });
        let error = commit_scene(&mut scene, &mut camera, prepared)
            .expect_err("an upload that failed must not commit");
        assert!(error.to_string().contains("refused the buffers"));

        assert_eq!(scene.sketch_presentations, kept);
        assert!(!drawings(&scene).contains(&other[0]));
        assert_eq!(*camera.camera(), framing);
    }

    #[test]
    fn a_window_left_alone_neither_solves_again_nor_loses_the_drawings() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, sketch, _) = one_solved_sketch(&directory, Some("Profile"));
        let (scene, _camera) = live_scene_of(loaded);
        assert_eq!(drawings(&scene), vec![sketch]);
        let kept = scene.sketch_presentations.clone();

        // Twenty frames of the section, which is what a window on screen does
        // while nobody touches it. The drawings are carried so that whatever
        // comes to draw them need not ask again; a window that asked anyway
        // would have bought a solve per frame.
        let before = ferritecad_sketch_solver::native_solves();
        for _ in 0..20 {
            let page = section_page(&scene.sketch_solves);
            assert!(page.contains("Sketch solves"));
        }

        assert_eq!(
            ferritecad_sketch_solver::native_solves(),
            before,
            "a window that was left alone asked a solver something"
        );
        assert_eq!(
            scene.sketch_presentations, kept,
            "drawing a frame changed the drawings the window is holding"
        );
    }

    #[test]
    fn two_documents_that_draw_one_picture_can_say_different_things_about_it() {
        solver_or_skip!();

        // The same drawing, already at the size it is told to be, said once
        // and said twice. The pixels are the same and the accounts are not, so
        // a window that read its facts off the picture would show one thing
        // for both.
        let once = tempfile::tempdir().expect("a temporary directory is available");
        let twice = tempfile::tempdir().expect("a temporary directory is available");
        let settled = [
            (0.0, 0.0),
            (SOLVE_WIDTH, 0.0),
            (SOLVE_WIDTH, 30.0),
            (0.0, 30.0),
        ];
        let curves: Vec<ferritecad_document::SketchCurve> = (0..4)
            .map(|index| {
                solve_line(
                    ferritecad_types::StableEntityId::new(),
                    settled[index],
                    settled[(index + 1) % settled.len()],
                )
            })
            .collect();
        let edges: Vec<ferritecad_types::StableEntityId> =
            curves.iter().map(|curve| curve.id).collect();
        let mut sized = solve_frame(&edges);
        sized.push(solve_width(&edges));
        let mut repeated = sized.clone();
        repeated.push(solve_width(&edges));

        let once_path = once.path().join("plate.fcad");
        let twice_path = twice.path().join("plate.fcad");
        write_sketches(
            &once_path,
            vec![(Some("Profile"), curves.clone(), solve_named(sized))],
        );
        write_sketches(
            &twice_path,
            vec![(Some("Profile"), curves, solve_named(repeated))],
        );

        let first = load_document(&once_path);
        let second = load_document(&twice_path);
        assert_eq!(
            first.snapshot, second.snapshot,
            "the two documents were supposed to draw the same picture"
        );

        let (one, _) = live_scene_of(first);
        let (two, _) = live_scene_of(second);
        assert_eq!(
            (one.sketch_solves.len(), two.sketch_solves.len()),
            (1, 1),
            "one of the two documents arrived without its account"
        );
        assert!(one.sketch_solves[0].report().redundant().is_empty());
        assert_eq!(two.sketch_solves[0].report().redundant().len(), 1);
        assert_ne!(
            one.sketch_solves, two.sketch_solves,
            "one picture was allowed to decide what both documents say"
        );
        assert!(section_page(&one.sketch_solves).contains("None"));
        let said = section_page(&two.sketch_solves);
        assert!(said.contains(&two.sketch_solves[0].report().redundant()[0].to_string()));
        // The explanation is the other half of the same statement: two
        // documents that draw one picture are told apart by what is said about
        // them, and one of them says something the picture cannot show.
        assert!(
            said.contains("Distance") && said.contains("60 mm"),
            "the repeated constraint went unexplained, so the two documents differ only by an \
             identifier:\n{said}"
        );
        assert!(
            !section_page(&one.sketch_solves).contains("Distance"),
            "a document that repeats nothing was given an explanation anyway"
        );
    }

    #[test]
    fn every_sketch_keeps_its_own_name_and_its_own_account() {
        solver_or_skip!();

        // Two sketches whose accounts cannot be swapped without showing: one
        // free in two directions and repeating nothing, one free in one and
        // repeating the size it was given twice. The second is left unnamed,
        // so a section that dropped what nobody named would lose it.
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("two.fcad");
        let loose = solve_curves();
        let loose_edges: Vec<ferritecad_types::StableEntityId> =
            loose.iter().map(|curve| curve.id).collect();
        let tight = solve_curves();
        let tight_edges: Vec<ferritecad_types::StableEntityId> =
            tight.iter().map(|curve| curve.id).collect();
        let mut tight_rules = solve_frame(&tight_edges);
        tight_rules.push(solve_width(&tight_edges));
        tight_rules.push(solve_width(&tight_edges));
        tight_rules.push(ferritecad_document::SketchConstraintRule::Horizontal {
            a: ferritecad_document::SketchPointRef::new(
                tight_edges[2],
                ferritecad_document::SketchPointSelector::Start,
            ),
            b: ferritecad_document::SketchPointRef::new(
                tight_edges[2],
                ferritecad_document::SketchPointSelector::End,
            ),
        });
        let tight_constraints = solve_named(tight_rules);
        let first_repeated = tight_constraints[10].id;
        let second_repeated = tight_constraints[11].id;

        let sketches = write_sketches(
            &path,
            vec![
                (Some("Loose"), loose, solve_named(solve_frame(&loose_edges))),
                (None, tight, tight_constraints),
            ],
        );
        let (scene, _camera) = live_scene_of(load_document(&path));

        assert_eq!(scene.sketch_solves.len(), 2);
        assert_eq!(
            scene
                .sketch_solves
                .iter()
                .map(SketchSolveFacts::sketch)
                .collect::<Vec<_>>(),
            sketches,
            "the accounts arrived in an order that is not the document's"
        );
        assert_eq!(scene.sketch_solves[0].name(), Some("Loose"));
        assert_eq!(scene.sketch_solves[0].report().degrees_of_freedom(), 2);
        assert!(scene.sketch_solves[0].report().redundant().is_empty());
        assert_eq!(scene.sketch_solves[1].name(), None);
        assert_eq!(scene.sketch_solves[1].report().degrees_of_freedom(), 1);
        assert_eq!(
            scene.sketch_solves[1].report().redundant(),
            [first_repeated, second_repeated],
            "the repeated constraints are not the ones the document stores, or not in its order"
        );

        // And on screen, where the whole of one sketch's account has to sit
        // between its own name and the next sketch's. Positions in the words
        // actually laid out, because that is the order a person reads them in
        // and the only thing that says whose row is whose: two entries of the
        // right shape in the wrong order read as two correct entries.
        let drawn = section_words(&scene.sketch_solves);
        let page = drawn.join("\n");
        let at = |needle: &str| drawn.iter().position(|word| word == needle);
        let named = at("Loose");
        let unnamed = at("Unnamed sketch");
        let loose_row = at(&sketches[0].to_string());
        let tight_row = at(&sketches[1].to_string());
        // Two and one, where the numbers most easily put here by mistake –
        // nought, and how many constraints each sketch repeats – are nought
        // and two.
        let loose_freedom = at("2");
        let tight_freedom = at("1");
        let repeats_nothing = at("None");
        let first_at = at(&first_repeated.to_string());
        let second_at = at(&second_repeated.to_string());
        for (what, found) in [
            ("the first sketch's name", named),
            ("the second sketch, which nobody named", unnamed),
            ("the first sketch's identifier", loose_row),
            ("the second sketch's identifier", tight_row),
            ("two degrees of freedom", loose_freedom),
            ("one degree of freedom", tight_freedom),
            ("the line saying that nothing repeats", repeats_nothing),
            ("the first repeated constraint", first_at),
            ("the second repeated constraint", second_at),
        ] {
            assert!(found.is_some(), "{what} never reached the screen:\n{page}");
        }
        assert!(
            named < loose_row
                && loose_row < loose_freedom
                && loose_freedom < repeats_nothing
                && repeats_nothing < unnamed
                && unnamed < tight_row
                && tight_row < tight_freedom
                && tight_freedom < first_at
                && first_at < second_at,
            "a row of the section does not belong to the sketch it is under: {drawn:?}"
        );
        assert_eq!(
            drawn.iter().filter(|word| *word == "2").count(),
            1,
            "two degrees of freedom were reported for more than one of these sketches: {drawn:?}"
        );
        assert_eq!(
            drawn.iter().filter(|word| *word == "1").count(),
            1,
            "one degree of freedom was reported for more than one of these sketches: {drawn:?}"
        );
        assert_eq!(
            drawn.iter().filter(|word| *word == "None").count(),
            1,
            "exactly one of these two sketches repeats nothing: {drawn:?}"
        );
        assert!(
            !page.contains("Fully constrained"),
            "neither of these sketches is settled, and one was said to be:\n{page}"
        );
    }

    #[test]
    fn several_repeated_constraints_are_explained_one_by_one_in_the_documents_order() {
        solver_or_skip!();

        // One sketch repeating two different things: the width it was already
        // given, and a side already said to be level. Two families, so an
        // entry described from its neighbour's rule reads wrongly rather than
        // merely differently, and two entries, so the document's order is
        // something a section can get wrong.
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        let curves = solve_curves();
        let edges: Vec<ferritecad_types::StableEntityId> =
            curves.iter().map(|curve| curve.id).collect();
        let mut rules = solve_frame(&edges);
        rules.push(solve_width(&edges));
        rules.push(solve_width(&edges));
        rules.push(ferritecad_document::SketchConstraintRule::Horizontal {
            a: ferritecad_document::SketchPointRef::new(
                edges[2],
                ferritecad_document::SketchPointSelector::Start,
            ),
            b: ferritecad_document::SketchPointRef::new(
                edges[2],
                ferritecad_document::SketchPointSelector::End,
            ),
        });
        let constraints = solve_named(rules);
        let sized = constraints[10].id;
        let levelled = constraints[11].id;
        write_sketches(&path, vec![(Some("Profile"), curves, constraints)]);

        let (scene, _camera) = live_scene_of(load_document(&path));
        let drawn = section_words(&scene.sketch_solves);
        let page = drawn.join("\n");
        let at = |needle: &str| drawn.iter().position(|word| word == needle);

        let sized_says = format!(
            "Distance: {}.start and {}.end are 60 mm apart",
            edges[0], edges[0]
        );
        let levelled_says = format!(
            "Horizontal: {}.start and {}.end share a y coordinate",
            edges[2], edges[2]
        );
        for (what, found) in [
            ("the first repeated constraint", at(&sized.to_string())),
            ("what the first one says", at(&sized_says)),
            ("the second repeated constraint", at(&levelled.to_string())),
            ("what the second one says", at(&levelled_says)),
        ] {
            assert!(found.is_some(), "{what} never reached the screen:\n{page}");
        }
        assert!(
            at(&sized.to_string()) < at(&sized_says)
                && at(&sized_says) < at(&levelled.to_string())
                && at(&levelled.to_string()) < at(&levelled_says),
            "the explanations are not under the identifiers they belong to, or not in the \
             document's order: {drawn:?}"
        );
        assert!(
            !page.contains("None"),
            "a sketch repeating two constraints also said it repeats none:\n{page}"
        );
    }

    #[test]
    fn no_sketch_is_given_the_explanation_of_its_neighbour() {
        solver_or_skip!();

        // Two sketches in one document, each with curves of its own and each
        // repeating something of its own: the first the width it was given
        // twice, the second a side already said to be level. Nothing in the
        // first's account may name a curve of the second, and the join that
        // could get this wrong is the one that looks a report's identifiers up
        // in whatever sketch is to hand.
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("two.fcad");

        let sized = solve_curves();
        let sized_edges: Vec<ferritecad_types::StableEntityId> =
            sized.iter().map(|curve| curve.id).collect();
        let mut sized_rules = solve_frame(&sized_edges);
        sized_rules.push(solve_width(&sized_edges));
        sized_rules.push(solve_width(&sized_edges));

        let levelled = solve_curves();
        let levelled_edges: Vec<ferritecad_types::StableEntityId> =
            levelled.iter().map(|curve| curve.id).collect();
        let mut levelled_rules = solve_frame(&levelled_edges);
        levelled_rules.push(ferritecad_document::SketchConstraintRule::Horizontal {
            a: ferritecad_document::SketchPointRef::new(
                levelled_edges[2],
                ferritecad_document::SketchPointSelector::Start,
            ),
            b: ferritecad_document::SketchPointRef::new(
                levelled_edges[2],
                ferritecad_document::SketchPointSelector::End,
            ),
        });

        write_sketches(
            &path,
            vec![
                (Some("Sized"), sized, solve_named(sized_rules)),
                (Some("Levelled"), levelled, solve_named(levelled_rules)),
            ],
        );
        let (scene, _camera) = live_scene_of(load_document(&path));
        assert_eq!(scene.sketch_solves.len(), 2);

        let words = solve_words_of(&scene.sketch_solves);
        assert_eq!(
            (words[0].redundant.len(), words[1].redundant.len()),
            (1, 1),
            "each of these sketches repeats exactly one thing"
        );
        // Each account speaks only of its own drawing. Checked against every
        // curve of the other sketch, because naming one of them is exactly
        // what describing a neighbour's constraint would produce.
        for (mine, theirs) in [(&words[0], &levelled_edges), (&words[1], &sized_edges)] {
            for constraint in &mine.redundant {
                for curve in theirs {
                    assert!(
                        !constraint.says.contains(&curve.to_string()),
                        "a sketch was described using a curve of the sketch beside it: {}",
                        constraint.says
                    );
                }
            }
        }
        assert!(
            words[0].redundant[0].says.contains("Distance")
                && words[1].redundant[0].says.contains("Horizontal"),
            "the two sketches were given each other's families: {:?} and {:?}",
            words[0].redundant[0].says,
            words[1].redundant[0].says
        );
    }

    #[test]
    fn the_section_says_nothing_a_solve_kept_to_itself() {
        solver_or_skip!();

        // The whole section, laid out from a real document, read for words
        // that mean something only inside one call to one library, and for the
        // punctuation a derived Debug would leave behind.
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, _, _) = one_solved_sketch(&directory, None);
        let (scene, _camera) = live_scene_of(loaded);
        let page = section_page(&scene.sketch_solves);

        assert!(
            page.contains("Unnamed sketch"),
            "a sketch nobody named vanished from the section instead of being said to be \
             unnamed:\n{page}"
        );
        assert!(
            page.contains("Distance"),
            "the repeated constraint of an unnamed sketch went unexplained:\n{page}"
        );
        for forbidden in [
            "ConstraintId",
            "PointId",
            "SketchSolveReport",
            "SketchSolveFacts",
            "RedundantConstraint",
            "SketchConstraintRule",
            "SketchPointRef",
            "StableEntityId",
            "ObjectId",
            "degrees_of_freedom",
            "redundant",
            "planegcs",
            "session",
            "ordinal",
            "index",
            "{",
            "}",
            "[",
            "Some(",
        ] {
            assert!(
                !page.contains(forbidden),
                "the section printed {forbidden:?}, which means something only to one solve:\
                 \n{page}"
            );
        }
    }

    #[test]
    fn showing_what_a_solve_found_out_asks_no_solver_and_uploads_nothing() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, _, _) = one_solved_sketch(&directory, Some("Profile"));

        let mut camera = ViewportInput::new();
        camera.resize(800, 600);
        let mut uploads = 0;
        let mut scene = LiveScene::new(
            (),
            Vec::new(),
            FaceNames::default(),
            EdgeNames::default(),
            VertexNames::default(),
            Visibility::default(),
            Vec::new(),
            Vec::new(),
        );
        let prepared = prepare_load(&camera, Ok(loaded), |_| {
            uploads += 1;
            Ok(())
        });
        commit_scene(&mut scene, &mut camera, prepared).expect("the document commits");
        assert_eq!(uploads, 1, "one arrival, one upload");

        // Twenty frames of the section, which is what a window on screen does
        // while nobody touches it.
        let before = ferritecad_sketch_solver::native_solves();
        for _ in 0..20 {
            let page = section_page(&scene.sketch_solves);
            assert!(page.contains("Under-constrained"));
            // Including the explanation, which is what a second solve would
            // most plausibly be asked for: it is written from what the first
            // one already said and from the document that was already read.
            assert!(
                page.contains("Distance") && page.contains("60 mm"),
                "the explanation was not on screen this frame:\n{page}"
            );
        }
        assert_eq!(
            ferritecad_sketch_solver::native_solves(),
            before,
            "drawing the section asked a solver something"
        );
        assert_eq!(
            uploads, 1,
            "drawing the section put geometry on the graphics device"
        );
    }

    // -----------------------------------------------------------------
    // A document that contradicts itself, at the application boundary
    // -----------------------------------------------------------------

    /// Writes a plate told to be two different widths at once.
    ///
    /// Returns the file, the sketch the conflict belongs to, and the
    /// constraints the file holds for it, read back out of the file.
    fn impossible_document(
        directory: &tempfile::TempDir,
    ) -> (
        std::path::PathBuf,
        ferritecad_types::ObjectId,
        Vec<ferritecad_document::SketchConstraint>,
    ) {
        contradicted_document(directory, "impossible.fcad", 1)
    }

    /// The same, with `agreeing` copies of the stored width before the one
    /// that disagrees with them.
    ///
    /// More than one copy is how the same rule comes to be stored under
    /// several identifiers, which is the case where naming a constraint by
    /// what it says rather than by what it is called would lose one.
    fn contradicted_document(
        directory: &tempfile::TempDir,
        file: &str,
        agreeing: usize,
    ) -> (
        std::path::PathBuf,
        ferritecad_types::ObjectId,
        Vec<ferritecad_document::SketchConstraint>,
    ) {
        use ferritecad_document::{Document, ObjectPayload};

        let path = directory.path().join(file);
        let curves = solve_curves();
        let edges: Vec<ferritecad_types::StableEntityId> =
            curves.iter().map(|curve| curve.id).collect();
        let mut rules = solve_frame(&edges);
        for _ in 0..agreeing {
            rules.push(solve_width(&edges));
        }
        rules.push(ferritecad_document::SketchConstraintRule::Distance {
            a: ferritecad_document::SketchPointRef::new(
                edges[0],
                ferritecad_document::SketchPointSelector::Start,
            ),
            b: ferritecad_document::SketchPointRef::new(
                edges[0],
                ferritecad_document::SketchPointSelector::End,
            ),
            distance: SOLVE_WIDTH + 15.0,
        });
        let sketches = write_sketches(&path, vec![(Some("Profile"), curves, solve_named(rules))]);

        let document = Document::open_read_only(&path).expect("reopens");
        let record = document
            .objects()
            .expect("objects")
            .into_iter()
            .find(|record| record.id == sketches[0])
            .expect("the document holds that sketch");
        let ObjectPayload::Sketch(stored) = record.payload else {
            panic!("that object is not a sketch");
        };
        (path, sketches[0], stored.constraints)
    }

    /// Loads the way the viewer does, and expects the load to be refused.
    fn refused_document(path: &Path) -> CadError {
        use ferritecad_kernel::mock::MockKernel;

        let mut kernel = MockKernel::new();
        let error = snapshot_of(
            path,
            &mut kernel,
            |_: &mut MockKernel, _: &[u8]| {
                Err(CadError::unsupported("these documents hold no imports"))
            },
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect_err("a plate that is two widths at once has no picture");
        assert_eq!(
            kernel.live_shape_count(),
            0,
            "a refused load left shapes behind"
        );
        error
    }

    #[test]
    fn a_conflicting_document_reaches_the_boundary_as_facts_and_not_as_a_sentence() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, sketch, stored) = impossible_document(&directory);

        let mut camera = ViewportInput::new();
        camera.resize(800, 600);
        let mut uploads = 0;
        // The route a real Open takes, minus the graphics device: the document
        // is read, rebuilt, refused, and the refusal is handed to the same
        // `prepare_load` that would otherwise have staged a picture.
        let prepared = prepare_load(&camera, Err(refused_document(&path)), |_| {
            uploads += 1;
            Ok(())
        });
        let error = prepared.expect_err("a document that cannot be built has no picture to stage");

        let conflict = ferritecad_scene::SketchConflict::of(&error)
            .expect("the boundary was handed a sentence and nothing it could act on");
        assert_eq!(
            conflict.sketch(),
            sketch,
            "the conflict is about some other object than the sketch that holds it"
        );
        assert_eq!(
            conflict
                .constraints()
                .iter()
                .map(|constraint| (constraint.id(), *constraint.rule()))
                .collect::<Vec<_>>(),
            vec![
                (stored[9].id, stored[9].rule),
                (stored[10].id, stored[10].rule),
            ],
            "the boundary was not told which two sizes disagree, in the document's order"
        );

        assert_eq!(uploads, 0, "a refused document reached the graphics device");
    }

    #[test]
    fn a_refused_open_replaces_neither_the_picture_nor_what_it_came_with() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");

        // A document that loads, shown first, so there is something to lose.
        let (loaded, sketch, repeated) = one_solved_sketch(&directory, Some("Profile"));
        let (mut scene, mut camera) = live_scene_of(loaded);
        assert_eq!(scene.sketch_solves.len(), 1);
        let catalogue = scene.catalogue.clone();

        let (path, _, _) = impossible_document(&directory);
        let refused = prepare_load(&camera, Err(refused_document(&path)), |_| Ok(()));
        let error = commit_scene(&mut scene, &mut camera, refused)
            .expect_err("a refused document was committed");

        // The refusal still carries its facts after the commit refused it, and
        // it is still the same refusal.
        assert!(
            ferritecad_scene::SketchConflict::of(&error).is_some(),
            "the facts were lost between the boundary and the commit"
        );

        // And the window is showing exactly what it was showing.
        assert_eq!(scene.sketch_solves.len(), 1, "the account was replaced");
        assert_eq!(scene.sketch_solves[0].sketch(), sketch);
        assert_eq!(scene.sketch_solves[0].report().redundant(), [repeated]);
        assert_eq!(
            scene
                .sketch_solves
                .first()
                .map(|facts| facts.redundant().len()),
            Some(1),
            "the explanation that came with the picture was replaced"
        );
        assert_eq!(
            scene.catalogue, catalogue,
            "a refused document changed the catalogue of the picture on screen"
        );
    }

    #[test]
    fn a_failure_that_is_not_a_conflict_stays_what_it_is() {
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);
        let error = prepare_load(
            &camera,
            Err(CadError::input("no such document")),
            |_| Ok(()),
        )
        .expect_err("a document that is not there has no picture");

        assert!(
            ferritecad_scene::SketchConflict::of(&error).is_none(),
            "a document that could not be read was reported as a drawing that contradicts \
             itself: {error}"
        );
    }

    #[test]
    fn a_conflict_costs_one_native_system_and_one_reading_of_the_document() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, _, stored) = impossible_document(&directory);

        let sessions = ferritecad_sketch_solver::native_sessions();
        let solves = ferritecad_sketch_solver::native_solves();
        let live = ferritecad_sketch_solver::native_live_sessions();

        let error = refused_document(&path);
        let conflict =
            ferritecad_scene::SketchConflict::of(&error).expect("a conflict carries its facts");

        assert_eq!(
            ferritecad_sketch_solver::native_sessions() - sessions,
            1,
            "one refused document built more than one native system"
        );
        assert_eq!(
            ferritecad_sketch_solver::native_solves(),
            solves,
            "a sketch refused before it was solved was solved anyway"
        );
        assert_eq!(
            ferritecad_sketch_solver::native_live_sessions(),
            live,
            "a refused load left a native system alive"
        );

        // The file is gone and the facts are whole: they travelled with the
        // refusal rather than being fetched when somebody asked for them.
        std::fs::remove_file(&path).expect("removes");
        assert_eq!(
            conflict
                .constraints()
                .iter()
                .map(|constraint| constraint.id())
                .collect::<Vec<_>>(),
            vec![stored[9].id, stored[10].id]
        );
    }

    #[test]
    fn a_conflict_at_the_boundary_says_nothing_that_lasts_one_solve() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, _, _) = impossible_document(&directory);
        let error = refused_document(&path);
        let conflict =
            ferritecad_scene::SketchConflict::of(&error).expect("a conflict carries its facts");

        let printed = format!("{conflict:?} {conflict} {error}");
        for forbidden in [
            "PointId",
            "ConstraintId",
            "SolverError",
            "Outcome",
            "session",
            "Session",
            "ordinal",
            "equation",
        ] {
            assert!(
                !printed.contains(forbidden),
                "a conflict published {forbidden}, which means nothing outside one \
                 solve:\n{printed}"
            );
        }
    }

    #[test]
    fn showing_what_a_solve_found_out_changes_nothing_a_person_chose() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, _, _) = one_solved_sketch(&directory, Some("Profile"));
        let picture = loaded.snapshot.clone();
        let chosen = picture
            .draws()
            .first()
            .expect("the picture draws something")
            .pick;
        let (mut scene, camera) = live_scene_of(loaded);

        // A choice, a question and a hidden definition, all made in this
        // picture, and a camera pointed somewhere in particular.
        scene.selection = Selection::Definition(chosen);
        scene.hovered = Hovered::Definition(chosen);
        scene.visibility = Visibility::new(&picture);
        let visibility = scene.visibility.clone();
        let framing = *camera.camera();

        for _ in 0..5 {
            let page = section_page(&scene.sketch_solves);
            assert!(page.contains("Sketch solves"));
            assert!(
                page.contains("Distance"),
                "the explanation stopped being drawn"
            );
        }

        assert_eq!(scene.selection, Selection::Definition(chosen));
        assert_eq!(scene.hovered, Hovered::Definition(chosen));
        assert_eq!(scene.visibility, visibility);
        assert_eq!(*camera.camera(), framing);
    }

    // -----------------------------------------------------------------
    // What the window says about an Open that failed
    // -----------------------------------------------------------------

    /// Starts a real request for a document, without doing the reading.
    ///
    /// The generation, the status and the cancellation are the window's own;
    /// what the worker would have produced is handed to `deliver_into` by the
    /// gate, which is where the real load actually happens.
    fn start_open(loads: &mut Loads, path: &Path) -> LoadGeneration {
        loads
            .open(Some(path), relay(), |_, _| std::thread::spawn(|| {}))
            .expect("a load started")
    }

    /// The sentence a gate expects for one stored size.
    ///
    /// Written out here from the two points and the number the file holds,
    /// rather than obtained from the code that writes it.
    fn distance_says(constraint: &ferritecad_document::SketchConstraint) -> String {
        let ferritecad_document::SketchConstraintRule::Distance { a, b, distance } =
            constraint.rule
        else {
            panic!("this fixture stores its widths as distances");
        };
        format!("Distance: {a} and {b} are {distance} mm apart")
    }

    #[test]
    fn a_conflicting_document_says_in_the_window_which_constraints_disagree() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, sketch, stored) = impossible_document(&directory);

        // The whole route a real Open takes: a request, a document read and
        // refused by the real solver, and the answer delivered to the
        // generation that asked for it.
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let generation = start_open(&mut loads, &path);
        let effect = deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            generation,
            Err(refused_document(&path)),
        );
        assert!(effect.status_changed, "the failure was not accepted");

        assert_eq!(
            failure_words(&loads),
            vec![
                "Open failed".to_owned(),
                "Problem".to_owned(),
                "Constraint conflict".to_owned(),
                "File".to_owned(),
                "impossible.fcad".to_owned(),
                "Sketch".to_owned(),
                sketch.to_string(),
                "Constraint".to_owned(),
                stored[9].id.to_string(),
                "Says".to_owned(),
                distance_says(&stored[9]),
                "Constraint".to_owned(),
                stored[10].id.to_string(),
                "Says".to_owned(),
                distance_says(&stored[10]),
            ],
            "the window did not say which stored constraints disagree, on which file"
        );

        // The two sizes themselves, read back off the page: a size a person
        // typed is a size they have to be able to recognise.
        let page = failure_page(&loads);
        assert!(
            page.contains("are 60 mm apart"),
            "the stored size was lost:\n{page}"
        );
        assert!(
            page.contains("are 75 mm apart"),
            "the stated size was lost:\n{page}"
        );
    }

    #[test]
    fn every_family_is_said_in_a_failed_open_exactly_as_it_is_said_when_it_repeats() {
        let (first, second) = two_curves();
        let families = eight_families(first, second);

        // One conflict blaming one constraint of every family the document
        // can store, worded by the one road from a rule to a sentence.
        let sketch = ferritecad_types::ObjectId::new();
        let identifiers: Vec<ferritecad_types::StableEntityId> = families
            .iter()
            .map(|_| ferritecad_types::StableEntityId::new())
            .collect();
        let conflict = OpenConflict {
            file: "impossible.fcad".to_owned(),
            sketch: sketch.to_string(),
            constraints: identifiers
                .iter()
                .zip(&families)
                .map(|(id, (rule, _))| ConstraintWords::of(*id, rule))
                .collect(),
        };

        let rules = conflict.rules();
        let words =
            laid_out(|ui| ferritecad_ui::open_failure_panel(ui, Some(conflict.shown(&rules))));

        let mut expected = vec![
            "Open failed".to_owned(),
            "Problem".to_owned(),
            "Constraint conflict".to_owned(),
            "File".to_owned(),
            "impossible.fcad".to_owned(),
            "Sketch".to_owned(),
            sketch.to_string(),
        ];
        for (id, (_, says)) in identifiers.iter().zip(&families) {
            expected.push("Constraint".to_owned());
            expected.push(id.to_string());
            expected.push("Says".to_owned());
            expected.push(says.clone());
        }
        assert_eq!(
            words, expected,
            "a conflicting constraint is worded differently from a repeated one of the same rule"
        );

        // No two families read the same, so an arm answering for its
        // neighbour cannot pass by saying something plausible.
        let said: std::collections::BTreeSet<&String> =
            families.iter().map(|(_, says)| says).collect();
        assert_eq!(
            said.len(),
            families.len(),
            "two families are said the same way"
        );
    }

    #[test]
    fn a_conflicting_size_is_said_as_it_was_stored() {
        let (first, _) = two_curves();
        let point = ferritecad_document::SketchPointRef::new;
        use ferritecad_document::SketchPointSelector::{End, Start};

        // A whole number and a fraction, both as stored: a house style that
        // rounded either would be the window deciding what a drawing says.
        for (distance, expected) in [(60.0_f64, "60"), (12.7, "12.7"), (0.5, "0.5")] {
            let id = ferritecad_types::StableEntityId::new();
            let rule = ferritecad_document::SketchConstraintRule::Distance {
                a: point(first, Start),
                b: point(first, End),
                distance,
            };
            let conflict = OpenConflict {
                file: "impossible.fcad".to_owned(),
                sketch: ferritecad_types::ObjectId::new().to_string(),
                constraints: vec![ConstraintWords::of(id, &rule)],
            };
            let rules = conflict.rules();
            let page =
                laid_out(|ui| ferritecad_ui::open_failure_panel(ui, Some(conflict.shown(&rules))))
                    .join("\n");
            assert!(
                page.contains(&format!(
                    "Distance: {first}.start and {first}.end are {expected} mm apart"
                )),
                "a stored size, its unit or the order of its two points changed:\n{page}"
            );
        }
    }

    #[test]
    fn several_conflicting_constraints_stay_in_the_order_the_document_stores_them() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        // Three copies of one width and one that contradicts them: the same
        // rule under three identifiers, which is three constraints.
        let (path, sketch, stored) = contradicted_document(&directory, "repeated.fcad", 3);

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let generation = start_open(&mut loads, &path);
        deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            generation,
            Err(refused_document(&path)),
        );

        let mut expected = vec![
            "Open failed".to_owned(),
            "Problem".to_owned(),
            "Constraint conflict".to_owned(),
            "File".to_owned(),
            "repeated.fcad".to_owned(),
            "Sketch".to_owned(),
            sketch.to_string(),
        ];
        for constraint in &stored[9..13] {
            expected.push("Constraint".to_owned());
            expected.push(constraint.id.to_string());
            expected.push("Says".to_owned());
            expected.push(distance_says(constraint));
        }
        assert_eq!(
            failure_words(&loads),
            expected,
            "constraints were lost, merged or reordered on the way to the window"
        );

        // The three that agree say the same thing under three identifiers,
        // and are three rows: a section that named them by what they say
        // would show one.
        let says = distance_says(&stored[9]);
        assert_eq!(
            failure_words(&loads)
                .iter()
                .filter(|word| **word == says)
                .count(),
            3,
            "identical rules stored under different identifiers were folded together"
        );
    }

    #[test]
    fn a_failed_open_names_the_file_it_failed_on_and_not_the_one_on_screen() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, shown_sketch, _) = one_solved_sketch(&directory, Some("Profile"));
        let (path, conflicting_sketch, _) = impossible_document(&directory);

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();

        // A document that opens, first, so there is another file and another
        // sketch for the account to be confused with.
        let first = start_open(&mut loads, &directory.path().join("plate.fcad"));
        deliver_into(&mut scene, &mut loads, &mut input, first, Ok(loaded));

        let second = start_open(&mut loads, &path);
        deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            second,
            Err(refused_document(&path)),
        );

        let page = failure_page(&loads);
        assert!(
            page.contains("impossible.fcad"),
            "the attempted file is not named:\n{page}"
        );
        assert!(
            page.contains(&conflicting_sketch.to_string()),
            "the sketch that holds the conflict is not named:\n{page}"
        );
        assert!(
            !page.contains("plate.fcad"),
            "the account of a failed Open named the document on screen:\n{page}"
        );
        assert!(
            !page.contains(&shown_sketch.to_string()),
            "the account of a failed Open named a sketch of the document on screen:\n{page}"
        );
    }

    #[test]
    fn a_failed_open_leaves_the_picture_and_everything_chosen_in_it_alone() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, shown_sketch, repeated) = one_solved_sketch(&directory, Some("Profile"));
        let picture = loaded.snapshot.clone();
        let chosen = picture
            .draws()
            .first()
            .expect("the picture draws something")
            .pick;

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let first = start_open(&mut loads, &directory.path().join("plate.fcad"));
        deliver_into(&mut scene, &mut loads, &mut input, first, Ok(loaded));

        // A choice, a question, a hidden definition and a camera pointed
        // somewhere in particular, all made in the picture that is up.
        scene.selection = Selection::Definition(chosen);
        scene.hovered = Hovered::Definition(chosen);
        scene.visibility = Visibility::new(&picture);
        let catalogue = scene.catalogue.clone();
        let visibility = scene.visibility.clone();
        let framing = *input.camera();
        let solves = section_page(&scene.sketch_solves);

        let (path, _, _) = impossible_document(&directory);
        let second = start_open(&mut loads, &path);
        deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            second,
            Err(refused_document(&path)),
        );

        assert!(
            failure_page(&loads).contains("Constraint conflict"),
            "the failure was not reported at all"
        );
        assert_eq!(
            scene.selection,
            Selection::Definition(chosen),
            "the choice was cleared"
        );
        assert_eq!(
            scene.hovered,
            Hovered::Definition(chosen),
            "the question was cleared"
        );
        assert_eq!(scene.visibility, visibility, "what is drawn changed");
        assert_eq!(*input.camera(), framing, "the camera moved");
        assert_eq!(
            scene.catalogue, catalogue,
            "the parts of the picture changed"
        );
        assert_eq!(
            scene.sketch_solves.len(),
            1,
            "the account of the drawing was replaced"
        );
        assert_eq!(scene.sketch_solves[0].sketch(), shown_sketch);
        assert_eq!(scene.sketch_solves[0].report().redundant(), [repeated]);
        assert_eq!(
            section_page(&scene.sketch_solves),
            solves,
            "what this document's own solve found out changed"
        );
    }

    #[test]
    fn the_conflict_is_not_filed_with_what_the_documents_own_solves_found_out() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, _, _) = one_solved_sketch(&directory, Some("Profile"));

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let first = start_open(&mut loads, &directory.path().join("plate.fcad"));
        deliver_into(&mut scene, &mut loads, &mut input, first, Ok(loaded));

        let (path, conflicting_sketch, stored) = impossible_document(&directory);
        let second = start_open(&mut loads, &path);
        deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            second,
            Err(refused_document(&path)),
        );

        // The section that describes the model on screen says nothing about
        // the file that did not open: these are facts about two documents,
        // and one window showing them as one page would be describing a
        // drawing that does not contradict itself as one that does.
        let solves = section_page(&scene.sketch_solves);
        for foreign in [
            "Open failed",
            "Constraint conflict",
            "impossible.fcad",
            &conflicting_sketch.to_string(),
            &stored[9].id.to_string(),
            &stored[10].id.to_string(),
        ] {
            assert!(
                !solves.contains(foreign),
                "what a document's own solve found out included {foreign:?}, which belongs to \
                 another document's failed Open:\n{solves}"
            );
        }
    }

    #[test]
    fn a_stale_failure_after_a_newer_document_opened_says_nothing_anywhere() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, _, _) = one_solved_sketch(&directory, Some("Profile"));
        let (path, _, _) = impossible_document(&directory);

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();

        // The slow reading is the one a user gives up waiting for, so this
        // order is the usual one rather than the unlucky one.
        let stale = start_open(&mut loads, &path);
        let current = start_open(&mut loads, &directory.path().join("plate.fcad"));
        deliver_into(&mut scene, &mut loads, &mut input, current, Ok(loaded));

        let status = loads.status().clone();
        let solves = section_page(&scene.sketch_solves);
        let _ = input.take_redraw();

        let effect = deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            stale,
            Err(refused_document(&path)),
        );

        assert!(
            !effect.status_changed,
            "an abandoned reading changed the line"
        );
        assert_eq!(effect.error, None, "an abandoned reading was announced");
        assert!(
            !input.take_redraw(),
            "an abandoned reading asked for a frame"
        );
        assert_eq!(
            *loads.status(),
            status,
            "an abandoned reading changed the line"
        );
        assert!(
            failure_words(&loads).is_empty(),
            "a document nobody is opening was reported as having failed to open"
        );
        assert_eq!(
            scene.sketch_solves.len(),
            1,
            "an abandoned reading changed the picture"
        );
        assert_eq!(section_page(&scene.sketch_solves), solves);
    }

    #[test]
    fn a_stale_failure_while_another_document_is_being_read_says_nothing_anywhere() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, _, _) = impossible_document(&directory);

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();

        let stale = start_open(&mut loads, &path);
        start_open(&mut loads, &directory.path().join("plate.fcad"));
        let waiting = loads.status().clone();
        let _ = input.take_redraw();

        let effect = deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            stale,
            Err(refused_document(&path)),
        );

        assert!(!effect.status_changed);
        assert_eq!(effect.error, None, "an abandoned reading was announced");
        assert!(
            !input.take_redraw(),
            "an abandoned reading asked for a frame"
        );
        assert_eq!(
            *loads.status(),
            waiting,
            "an abandoned reading interrupted the one that replaced it"
        );
        assert!(
            failure_words(&loads).is_empty(),
            "a reading that was replaced put its own failure in front of the one in flight"
        );
    }

    #[test]
    fn asking_for_another_document_takes_down_the_last_attempts_account() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, _, _) = impossible_document(&directory);

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let failed = start_open(&mut loads, &path);
        deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            failed,
            Err(refused_document(&path)),
        );
        assert!(
            !failure_words(&loads).is_empty(),
            "the failure was not reported"
        );

        start_open(&mut loads, &directory.path().join("plate.fcad"));

        assert!(
            failure_words(&loads).is_empty(),
            "the last attempt's constraints were still on screen beside Opening…"
        );
        assert!(
            matches!(loads.status(), Status::Loading { .. }),
            "asking for a document did not say it was being read"
        );
    }

    #[test]
    fn giving_up_on_a_reading_leaves_no_account_of_it() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, _, _) = impossible_document(&directory);

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let failed = start_open(&mut loads, &path);
        deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            failed,
            Err(refused_document(&path)),
        );

        // A second attempt, given up on before it answered.
        start_open(&mut loads, &directory.path().join("plate.fcad"));
        cancel_load(&mut loads, &mut input);

        assert_eq!(
            *loads.status(),
            Status::Idle,
            "the line describes a reading nobody made"
        );
        assert!(
            failure_words(&loads).is_empty(),
            "an attempt that was given up on left an account of a failure behind"
        );
    }

    #[test]
    fn a_document_that_opens_takes_away_the_account_of_the_one_that_did_not() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (loaded, shown_sketch, _) = one_solved_sketch(&directory, Some("Profile"));
        let (path, _, _) = impossible_document(&directory);

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let failed = start_open(&mut loads, &path);
        deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            failed,
            Err(refused_document(&path)),
        );
        assert!(
            !failure_words(&loads).is_empty(),
            "the failure was not reported"
        );

        let opened = start_open(&mut loads, &directory.path().join("plate.fcad"));
        deliver_into(&mut scene, &mut loads, &mut input, opened, Ok(loaded));

        assert!(
            failure_words(&loads).is_empty(),
            "a document that opened left the last failure's constraints on screen"
        );
        assert_eq!(
            *loads.status(),
            Status::Ready {
                file: "plate.fcad".to_owned()
            }
        );
        assert_eq!(
            scene.sketch_solves.first().map(SketchSolveFacts::sketch),
            Some(shown_sketch),
            "the picture and its account did not arrive together"
        );
    }

    #[test]
    fn a_failure_with_nothing_to_explain_takes_away_the_last_ones_constraints() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, _, stored) = impossible_document(&directory);

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let failed = start_open(&mut loads, &path);
        deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            failed,
            Err(refused_document(&path)),
        );
        assert!(
            !failure_words(&loads).is_empty(),
            "the failure was not reported"
        );

        let missing = start_open(&mut loads, Path::new("gone.fcad"));
        deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            missing,
            Err(CadError::input("no such document")),
        );

        assert!(
            failure_words(&loads).is_empty(),
            "a document that could not be read was reported as a drawing that contradicts \
             itself, with the last one's constraints"
        );
        assert!(
            !failure_page(&loads).contains(&stored[9].id.to_string()),
            "the constraints of an earlier failure outlived it"
        );
        assert_eq!(
            loads.status().line(),
            "gone.fcad: invalid input: no such document",
            "the ordinary line about the ordinary failure changed"
        );
    }

    #[test]
    fn a_conflict_wrapped_in_another_failure_is_not_reported_as_one() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, _, _) = impossible_document(&directory);

        let refusal = refused_document(&path);
        let conflict = ferritecad_scene::SketchConflict::of(&refusal)
            .expect("the direct refusal carries its conflict")
            .clone();

        // Two shapes of the same mistake: a generic failure with the
        // constraint refusal below it, and a generic failure carrying the
        // conflict itself as its cause. What a failure is is what its own
        // variant says, however far down a conflict happens to lie.
        for wrapped in [
            CadError::input_because("this document could not be read", refusal),
            CadError::input_because("this document could not be read", conflict.clone()),
            CadError::kernel_because("this shape could not be built", conflict.clone()),
            CadError::io("this file could not be opened", conflict),
        ] {
            let mut loads = Loads::default();
            let mut input = ViewportInput::new();
            input.resize(800, 600);
            let mut scene = empty_scene();
            let generation = start_open(&mut loads, &path);
            deliver_into(&mut scene, &mut loads, &mut input, generation, Err(wrapped));

            assert!(
                failure_words(&loads).is_empty(),
                "a failure that merely has a conflict below it was reported as a conflict: {}",
                loads.status().line()
            );
        }
    }

    #[test]
    fn a_failed_open_with_no_picture_behind_it_still_reads() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, sketch, stored) = impossible_document(&directory);

        // Nothing has ever been opened: the first thing this window shows is
        // the account of what went wrong.
        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let generation = start_open(&mut loads, &path);
        deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            generation,
            Err(refused_document(&path)),
        );

        let page = failure_page(&loads);
        for expected in [
            "Open failed",
            "Constraint conflict",
            "impossible.fcad",
            &sketch.to_string(),
            &stored[9].id.to_string(),
            &distance_says(&stored[9]),
        ] {
            assert!(
                page.contains(expected),
                "a window with no picture lost {expected:?}:\n{page}"
            );
        }
    }

    #[test]
    fn one_accepted_failure_asks_for_one_frame_and_laying_it_out_again_changes_nothing() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, _, _) = impossible_document(&directory);

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let generation = start_open(&mut loads, &path);
        let _ = input.take_redraw();

        let error = refused_document(&path);
        let solves = ferritecad_sketch_solver::native_solves();
        let effect = deliver_into(&mut scene, &mut loads, &mut input, generation, Err(error));

        assert!(effect.status_changed, "the failure was not accepted");
        assert!(
            input.take_redraw(),
            "an accepted failure asked for no frame"
        );
        assert!(
            !input.take_redraw(),
            "an accepted failure asked for a second frame"
        );

        let first = failure_words(&loads);
        let recorded = loads.conflict().cloned();
        for _ in 0..20 {
            assert_eq!(
                failure_words(&loads),
                first,
                "the section changed while nothing did"
            );
        }
        assert_eq!(
            ferritecad_sketch_solver::native_solves(),
            solves,
            "drawing the account of a failure asked a solver something"
        );
        assert_eq!(
            loads.conflict().cloned(),
            recorded,
            "laying the section out changed it"
        );
        assert!(
            !input.take_redraw(),
            "laying the section out asked for a frame"
        );
    }

    #[test]
    fn the_diagnostic_stream_gets_the_current_failure_only_and_none_of_the_words() {
        solver_or_skip!();

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let (path, _, stored) = impossible_document(&directory);

        let mut loads = Loads::default();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let mut scene = empty_scene();
        let generation = start_open(&mut loads, &path);
        let effect = deliver_into(
            &mut scene,
            &mut loads,
            &mut input,
            generation,
            Err(refused_document(&path)),
        );

        let announced = effect.error.expect("the current failure was not announced");
        // The line the toolbar shows is the same one sentence it has always
        // been. What the section says is the section's, and a status line that
        // grew it would be a second place the wording lives.
        let line = loads.status().line();
        assert!(
            line.starts_with("impossible.fcad: "),
            "the line stopped being one sentence about one file: {line}"
        );
        // The sentence a person reads on screen is written for a screen. A
        // log line that grew it would be a second place the wording lives.
        for presentation in [
            "Open failed",
            "Constraint conflict",
            "Says",
            &distance_says(&stored[9]),
        ] {
            assert!(
                !announced.contains(presentation),
                "the diagnostic stream was given {presentation:?}, which is written for a \
                 window:\n{announced}"
            );
            assert!(
                !line.contains(presentation),
                "the status line was given {presentation:?}, which belongs to the section:\n{line}"
            );
        }
    }
}
