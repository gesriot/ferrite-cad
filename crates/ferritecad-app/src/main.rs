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

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::{sync::Arc, time::Instant};

use ferritecad_document::{CapSide, DOCUMENT_EXTENSION, SelectionRule, SemanticRole};
use ferritecad_kernel::{CancelToken, OperationContext, ProgressSink, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_scene::{
    CatalogueEntry, FaceMeaning, FaceNames, LoadedScene, SceneItem, Selection, snapshot_of,
};
use ferritecad_types::{CadError, Result};
use ferritecad_ui::{
    Activity, Chosen, FRAME_ALL_KEY, FRAME_KEY, HIDE_KEY, Hover, ISOLATE_KEY, PROJECTION_KEY,
    PointerButton, RowVisibility, SHOW_ALL_KEY, Selected, VIEWS, ViewportEvent, ViewportInput,
};
use ferritecad_viewport::{
    Camera, Marked, PickId, Projection, RenderSnapshot, SnapshotBuilder, StandardView, Visibility,
};
use ferritecad_viewport_gpu::{Hit, PreparedSnapshot, Renderer, WindowSurface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

fn main() -> Result<()> {
    let document = match document_argument(std::env::args_os().skip(1)) {
        Ok(document) => document,
        Err(error) => {
            // Printed rather than returned: an error out of `main` is shown in
            // its debug form, and a usage line is for a person to read.
            eprintln!("{error}");
            std::process::exit(2);
        }
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

/// The document to look at, from the command line.
///
/// One argument and no options: a viewer that guessed which of several files
/// was meant, or opened the current directory, would be doing something the
/// user did not ask for with a file they may not have meant.
fn document_argument(arguments: impl Iterator<Item = OsString>) -> Result<PathBuf> {
    const USAGE: &str = "usage: ferritecad-viewer <file.fcad>";

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
                    Err(error) => Status::Failed {
                        file,
                        message: error.to_string(),
                    },
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
/// Both halves of the pixel are read from one frame and decided together, by
/// the scene, which is the only place that knows what the document calls a
/// face and is therefore the only place that can decide.
///
/// A pixel that names nothing chooses nothing: clicking the background is how
/// a person unchooses, and a pick left over from a document that has since
/// been replaced names a definition of a picture nobody is looking at.
fn selection_at(hit: Hit, snapshot: &RenderSnapshot, faces: &FaceNames) -> Selection {
    Selection::at(hit.definition(), hit.face(), snapshot, faces)
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

/// Prepares both halves of a loaded picture without changing the current one.
///
/// Framing the camera is part of accepting a scene. It therefore has to be
/// staged alongside the GPU upload: if preparing the buffers fails after the
/// camera has already moved, the old model remains resident but may be framed
/// completely out of view. Returning both candidates lets the event-loop
/// thread commit them together after every fallible step has succeeded.
fn prepare_load<P>(
    current_input: &ViewportInput,
    loaded: Result<LoadedScene>,
    prepare: impl FnOnce(Arc<RenderSnapshot>) -> Result<P>,
) -> Result<(ViewportInput, P, Vec<CatalogueEntry>, FaceNames, Visibility)> {
    let mut input = current_input.clone();
    let loaded = loaded?;
    let snapshot = input.accept_load(Ok(loaded.snapshot))?;
    // Everything drawn, in the picture that arrived. Built here rather than
    // carried over: what was hidden was hidden in a picture nobody is looking
    // at any more, and a mask that outlived its picture would be a document
    // opening with parts already missing.
    let visibility = Visibility::new(&snapshot);
    let prepared = prepare(Arc::new(snapshot))?;
    Ok((input, prepared, loaded.catalogue, loaded.faces, visibility))
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

/// The GPU picture and both meanings of a choice made in it.
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
    /// Which definitions this window is drawing. Transient, bound to
    /// `prepared`, and reset with it: what is hidden is a state of looking at
    /// a document, not a fact about the document.
    visibility: Visibility,
    /// What is chosen: nothing, a definition, or one face of one. One state
    /// rather than a transient field beside a semantic one, so there is no
    /// arrangement in which they describe different things.
    selection: Selection,
    /// What the pointer is over, which is a question and not a decision. Also
    /// issued by `prepared`, also transient, and written down nowhere. Three
    /// states rather than one identity, because a list row can only name a
    /// definition and a pixel can name the face under it, and the two are
    /// different things to show.
    hovered: Marked,
}

impl<P> LiveScene<P> {
    /// A replacement picture begins with no choice made in it.
    fn new(
        prepared: P,
        catalogue: Vec<CatalogueEntry>,
        faces: FaceNames,
        visibility: Visibility,
    ) -> Self {
        Self {
            prepared,
            catalogue,
            faces,
            visibility,
            selection: Selection::Nothing,
            hovered: Marked::Nothing,
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
    next: Result<(ViewportInput, P, Vec<CatalogueEntry>, FaceNames, Visibility)>,
) -> Result<()> {
    let (framed, prepared, catalogue, faces, visibility) = next?;
    *scene = LiveScene::new(prepared, catalogue, faces, visibility);
    *camera = framed;
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
    hovered: &mut Marked,
    snapshot: &RenderSnapshot,
    input: &mut ViewportInput,
) -> bool {
    if !visibility.hide(selection.marked(), snapshot) {
        return false;
    }
    *selection = Selection::Nothing;
    *hovered = Marked::Nothing;
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
    hovered: &mut Marked,
    snapshot: &RenderSnapshot,
    input: &mut ViewportInput,
) -> bool {
    if !visibility.isolate(selection.marked(), snapshot) {
        return false;
    }
    *hovered = Marked::Nothing;
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
fn change_projection(input: &mut ViewportInput, hovered: &mut Marked, to: Projection) -> bool {
    if !input.set_projection(to) {
        return false;
    }
    *hovered = Marked::Nothing;
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
    hovered: &mut Marked,
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
    *hovered = Marked::Nothing;
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
    hovered: &mut Marked,
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
    *hovered = Marked::Nothing;
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
    hovered: &mut Marked,
    snapshot: &RenderSnapshot,
    requested: PickId,
    input: &mut ViewportInput,
) -> bool {
    if !visibility.show(Marked::Definition(requested), snapshot) {
        return false;
    }
    *hovered = Marked::Nothing;
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
fn show_all(visibility: &mut Visibility, hovered: &mut Marked, input: &mut ViewportInput) -> bool {
    if !visibility.show_all() {
        return false;
    }
    *hovered = Marked::Nothing;
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

/// Records what the pointer is over, and says whether anything changed.
///
/// Answered through the picture that is on screen, so a question about a
/// picture that has been replaced marks nothing – a definition of another
/// picture and a face of another picture alike. Returns whether the answer
/// differs from the one already showing: pointing at the same face again is
/// not a reason to draw the same picture twice.
///
/// Given the one field it may change and nothing else, so pointing at
/// something cannot choose it however this is called.
fn hover(hovered: &mut Marked, snapshot: &RenderSnapshot, answer: Marked) -> bool {
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
    faces: Vec<FaceWords>,
}

/// One durable face name, in the words a person reads.
///
/// Portable terms only. There is no field here for a face ordinal, a mesh
/// index, a handle or a session, because there is nothing true to put in one:
/// what names a face is the reference the document stores.
struct FaceWords {
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
    let faces = match selection {
        Selection::Face(face) if selection.owning_definition(snapshot).is_some() => {
            face.meanings().iter().map(face_words).collect()
        }
        _ => Vec::new(),
    };
    Words {
        identities: identities_of(catalogue),
        faces,
    }
}

/// One stored reference, said in the document's own terms.
fn face_words(meaning: &FaceMeaning) -> FaceWords {
    FaceWords {
        reference: meaning.reference.to_string(),
        owner: meaning.owner.to_string(),
        producer_feature: meaning.producer_feature.to_string(),
        expected_kind: meaning.expected_kind.as_str().to_owned(),
        role: describe_role(&meaning.output_role),
        rule: describe_rule(&meaning.selection),
    }
}

fn face_name(words: &FaceWords) -> ferritecad_ui::FaceName<'_> {
    ferritecad_ui::FaceName {
        reference: &words.reference,
        owner: &words.owner,
        producer_feature: &words.producer_feature,
        expected_kind: &words.expected_kind,
        role: &words.role,
        rule: &words.rule,
    }
}

/// What a stored role says, as a sentence.
///
/// The document's own vocabulary, spelled out. A role names what a face *is* –
/// the end cap of this extrusion, the side raised from that sketch segment –
/// and every part of that sentence is durable.
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
/// describe different things. A chosen face is described as a face; anything
/// else is described as the definition it is.
fn inspected<'a>(
    selection: &Selection,
    catalogue: &'a [CatalogueEntry],
    identities: &'a [String],
    faces: &'a [ferritecad_ui::FaceName<'a>],
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
                match live.draw(&self.input, activity) {
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
                let chosen = selection_at(hit, live.scene.prepared.snapshot(), &live.scene.faces);
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
            // names a definition and can say nothing about a face: a list of
            // definitions has no faces in it.
            HoverRequest::Row(row) => live
                .scene
                .prepared
                .snapshot()
                .pick_of(row)
                .map(Marked::Definition),
            HoverRequest::Pixel(x, y) => {
                // One offscreen frame, and only because the pointer moved. A
                // pixel is the only thing that knows which face it came from.
                match Self::hit_at(live, self.input.camera(), x, y) {
                    Ok(hit) => Some(Marked::Face(hit.face())),
                    Err(error) => {
                        eprintln!("ferritecad: {error}");
                        return;
                    }
                }
            }
            // Away from the model, over a panel, or in the middle of a
            // gesture: whatever was under the pointer is not any more.
            HoverRequest::Clear => Some(Marked::Nothing),
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

    /// Reads one pixel, and both answers about it.
    ///
    /// The definition and the face come from one pixel of one frame, so they
    /// cannot describe different triangles. A click asks only the first half
    /// through [`Self::pick_at`]: what a click means is unchanged by any of
    /// this.
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
            Marked::Nothing,
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
                Visibility::default(),
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
        let face_names: Vec<ferritecad_ui::FaceName<'_>> =
            words.faces.iter().map(face_name).collect();
        // One answer for both sides of the choice: the row a list marks and
        // the facts an inspector shows come from the same resolution.
        let (definitions, chosen_row) = scene.view(&words.identities, scene.prepared.snapshot());
        // What each row can offer about whether it is drawn, and which rows
        // are not being drawn: one answer, read from the same visibility the
        // renderer reads, so a row marked hidden and a definition missing from
        // the picture cannot be two different sets.
        let offers = rows_visibility(&scene.visibility, scene.prepared.snapshot());
        let described = inspected(
            &scene.selection,
            &scene.catalogue,
            &words.identities,
            &face_names,
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
            snapshot,
            catalogue: Vec::new(),
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
        // The same order the event loop uses: whether to show it, then
        // showing it, then saying so. Announcing first would let the window
        // call a document ready before the frame that put it there.
        let outcome = if loads.accepts(generation) {
            match prepare_load(input, result, |_| Ok(())) {
                Ok((updated, (), _, _, _)) => {
                    *input = updated;
                    Ok(())
                }
                Err(error) => Err(error),
            }
        } else {
            result.map(|_| ())
        };
        finish_answer(loads, input, generation, outcome)
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
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Marked::Nothing,
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
            Visibility::default(),
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
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Marked::Nothing,
        };
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);

        let arriving = a_body();
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);

        commit_scene(
            &mut scene,
            &mut camera,
            Ok((
                framed,
                (),
                vec![arriving.clone()],
                FaceNames::default(),
                Visibility::default(),
            )),
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
            visibility: Visibility::default(),
            selection: Selection::Definition(
                picture
                    .draws()
                    .first()
                    .expect("the picture draws something")
                    .pick,
            ),
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: scene.selection,
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Marked::Nothing,
        };

        // Pointing at the definition that is already chosen: a question about
        // what is under the pointer, and the choice is untouched by it.
        assert!(hover(
            &mut scene.hovered,
            &picture,
            Marked::Definition(chosen)
        ));
        assert_eq!(scene.hovered, Marked::Definition(chosen));
        assert_eq!(
            scene.selection,
            Selection::Definition(chosen),
            "pointing at something chose it"
        );

        // Asking the same thing again changes nothing, so nothing asks for a
        // frame that would draw the picture that is already on screen.
        assert!(
            !hover(&mut scene.hovered, &picture, Marked::Definition(chosen)),
            "the same question was treated as news"
        );

        // Away from the model: the question is answered with nothing, and the
        // choice survives it.
        assert!(hover(&mut scene.hovered, &picture, Marked::Nothing));
        assert_eq!(scene.hovered, Marked::Nothing);
        assert_eq!(scene.selection, Selection::Definition(chosen));

        // A question about a picture that has been replaced marks nothing in
        // this one, however plausible its number looks.
        assert!(!hover(
            &mut scene.hovered,
            &other,
            Marked::Definition(chosen)
        ));
        assert_eq!(scene.hovered, Marked::Nothing);
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
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Marked::Nothing,
        };
        assert!(hover(
            &mut scene.hovered,
            &picture,
            Marked::Definition(picture.pick_of(first).expect("a row"))
        ));
        assert!(hover(
            &mut scene.hovered,
            &picture,
            Marked::Definition(by_row)
        ));
        assert_eq!(scene.hovered, Marked::Definition(by_row));
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
                    Marked::Nothing,
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
            Visibility::new(picture),
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
            Visibility::new(&picture),
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
        scene.hovered = Marked::Definition(picture.pick_of(0).expect("drawn"));
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
        assert_eq!(scene.hovered, Marked::Nothing);
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
        assert_eq!(scene.hovered, Marked::Nothing);
        assert_eq!(input.camera().view_projection(), camera);
    }

    #[test]
    fn an_action_with_nothing_to_do_asks_for_nothing() {
        let picture = two_definitions();
        let mut scene = LiveScene::new(
            (),
            vec![a_body(), a_body()],
            FaceNames::default(),
            Visibility::new(&picture),
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
            Ok((
                framed,
                (),
                vec![a_body(), a_body()],
                FaceNames::default(),
                Visibility::new(&next),
            )),
        )
        .expect("a load that arrived commits");
        assert!(!scene.visibility.anything_hidden());
        assert!(scene.visibility.shows(0, &next));
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.hovered, Marked::Nothing);
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
    fn plate_and_more_bodies(extra: usize) -> (tempfile::TempDir, LoadedScene) {
        use ferritecad_document::{
            Body, Dependency, DependencyRole, EndCondition, Expression, Extrude, ObjectPayload,
            Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
        };
        use ferritecad_kernel::mock::MockKernel;
        use ferritecad_types::{ObjectId, StableEntityId};

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");

        let mut document = ferritecad_document::Document::open(&path).expect("opens");
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
                            id: StableEntityId::new(),
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
                        &ObjectPayload::Sketch(Sketch { plane, curves }),
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
            snapshot,
            &scene.faces,
        );
        let Selection::Face(before) = &chosen else {
            panic!("the plate's face is not named: {chosen:?}");
        };
        let meanings = before.meanings().to_vec();

        let mut visibility = Visibility::new(snapshot);
        let mut hovered = Marked::Nothing;
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
            Visibility::new(&picture),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);

        // A face selection, which is the one that could most easily be
        // downgraded by an operation that dealt only in definitions.
        let face = picture.face_of(1, 0).expect("numbered");
        scene.selection = Selection::at(
            picture.pick_of(1).expect("drawn"),
            face,
            &picture,
            &FaceNames::default(),
        );
        // With no durable names this falls back to the definition, so the face
        // case is stated with the transient mark the renderer is given.
        scene.hovered = Marked::Face(picture.face_of(0, 0).expect("numbered"));
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
        assert_eq!(scene.hovered, Marked::Nothing);
        assert_eq!(input.take_pick(), None);
        assert_eq!(input.take_hover(), Hover::Cleared);
        assert!(input.take_redraw(), "isolating owes a frame");
        assert_eq!(input.camera().view_projection(), camera);
    }

    #[test]
    fn isolating_a_chosen_face_keeps_it_chosen_as_that_face() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let mut visibility = Visibility::new(&scene.snapshot);
        let mut hovered = Marked::Nothing;
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
            Visibility::new(&picture),
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
            Visibility::new(&picture),
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
            Visibility::new(&picture),
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
            Visibility::new(&picture),
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
            Visibility::new(&picture),
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
        scene.hovered = Marked::Definition(picture.pick_of(1).expect("drawn"));
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
            Marked::Nothing,
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
            Visibility::new(&picture),
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
            Ok((
                framed,
                (),
                vec![a_body(), a_body(), a_body()],
                FaceNames::default(),
                Visibility::new(&next),
            )),
        )
        .expect("a load that arrived commits");
        assert!(!scene.visibility.anything_hidden());
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.hovered, Marked::Nothing);
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
            Visibility::new(&picture),
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
        scene.hovered = Marked::Definition(picture.pick_of(1).expect("drawn"));
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
        assert_eq!(scene.hovered, Marked::Nothing);
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
            snapshot,
            &scene.faces,
        );
        let Selection::Face(before) = &chosen else {
            panic!("the plate's face is not named: {chosen:?}");
        };
        let meanings = before.meanings().to_vec();

        let mut visibility = Visibility::new(snapshot);
        let mut hovered = Marked::Nothing;
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
            Visibility::new(&picture),
        );
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let before = scene.visibility.clone();

        // A no-op must preserve real transient state, not merely turn one
        // empty state into another. Record a mark, click and hover question
        // that still belong to this unchanged frame.
        scene.hovered = Marked::Definition(picture.pick_of(1).expect("drawn"));
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
            Visibility::new(&picture),
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
            Ok((
                framed,
                (),
                vec![a_body(), a_body(), a_body()],
                FaceNames::default(),
                Visibility::new(&next),
            )),
        )
        .expect("a load that arrived commits");
        assert!(!scene.visibility.anything_hidden());
        assert_eq!(scene.selection, Selection::Nothing);
        assert_eq!(scene.hovered, Marked::Nothing);
    }

    #[test]
    fn hiding_a_row_keeps_a_chosen_face_on_another_definition_exactly() {
        let (_directory, scene) = plate_and_a_second_body();
        let snapshot = &scene.snapshot;
        let face = snapshot.face_of(0, 0).expect("numbered");
        let mut chosen = Selection::at(
            snapshot.pick_of(0).expect("drawn"),
            face,
            snapshot,
            &scene.faces,
        );
        let Selection::Face(before) = &chosen else {
            panic!("the plate's face is not named: {chosen:?}");
        };
        let meanings = before.meanings().to_vec();

        let mut visibility = Visibility::new(snapshot);
        let mut hovered = Marked::Nothing;
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
            Visibility::new(&picture),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let chosen = scene.selection.clone();
        scene.hovered = Marked::Definition(picture.pick_of(2).expect("drawn"));
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

        assert_eq!(scene.hovered, Marked::Nothing);
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
            Visibility::new(&picture),
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
        scene.hovered = Marked::Definition(picture.pick_of(2).expect("drawn"));
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
            Visibility::new(&picture),
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
            Ok((
                framed,
                (),
                vec![a_body(), a_body(), a_body()],
                FaceNames::default(),
                Visibility::new(&next),
            )),
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
            let mut hovered = Marked::Nothing;
            if door >= 3 {
                // Show one and Show all need something already missing.
                assert!(hide_one(
                    &mut visibility,
                    &mut Selection::Nothing,
                    &mut Marked::Nothing,
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
            Visibility::new(&picture),
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
            Visibility::new(&picture),
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
        scene.hovered = Marked::Definition(picture.pick_of(2).expect("drawn"));
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

        assert_eq!(scene.hovered, Marked::Nothing);
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
            Visibility::new(&picture),
        );
        scene.selection = Selection::Definition(picture.pick_of(1).expect("drawn"));
        let chosen = scene.selection.clone();
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        let before = scene.visibility.clone();

        // Real transient state belonging to this unchanged frame, not the
        // absence of any, and a frame already owed.
        scene.hovered = Marked::Definition(picture.pick_of(2).expect("drawn"));
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
            Visibility::new(&picture),
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
            Ok((
                framed,
                (),
                vec![a_body(), a_body(), a_body()],
                FaceNames::default(),
                Visibility::new(&next),
            )),
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
            snapshot,
            &scene.faces,
        );
        let Selection::Face(before) = &chosen else {
            panic!("the plate's face is not named: {chosen:?}");
        };
        let meanings = before.meanings().to_vec();

        let mut visibility = Visibility::new(snapshot);
        let mut hovered = Marked::Nothing;
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
        hovered = Marked::Definition(snapshot.pick_of(0).expect("drawn"));
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
        assert_eq!(hovered, Marked::Nothing);
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
        let mut hovered = Marked::Definition(distant_scene().pick_of(0).expect("drawn"));
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
            Visibility::new(&picture),
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
            arriving.0.projection(),
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
                &mut Marked::Nothing,
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
            snapshot,
            &scene.faces,
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
        let mut hovered = Marked::Nothing;
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
            Visibility::new(&picture),
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
            Visibility::new(&picture),
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
            Visibility::new(&picture),
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
            Visibility::new(&picture),
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
                Marked::Nothing,
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
            Visibility::new(&snapshot),
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
                Marked::Nothing,
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
        let chosen = selection_at(hit, &snapshot, &scene.faces);
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
                Marked::Nothing,
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
        let face_names: Vec<ferritecad_ui::FaceName<'_>> =
            words.faces.iter().map(face_name).collect();
        let inspected = inspected(
            &chosen,
            &scene.catalogue,
            &words.identities,
            &face_names,
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
        let chosen = Selection::at(pick, face, &scene.snapshot, &scene.faces);
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
            Visibility::new(&scene.snapshot),
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
            Visibility::new(&snapshot),
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
                    Marked::Nothing,
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
                Marked::Nothing,
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
            Visibility::new(&scene.snapshot),
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
            Visibility::new(snapshot),
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
            Visibility::new(snapshot),
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
            Visibility::new(&scene.snapshot),
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
        let mut scene_with_face =
            LiveScene::new((), Vec::new(), FaceNames::default(), Visibility::default());
        scene_with_face.selection = chosen;
        assert!(
            frame_selection(&scene_with_face, &scene.snapshot, &mut looking_at_the_face)
                .expect("frames"),
            "framing a chosen face did nothing"
        );

        let mut looking_at_the_part = ViewportInput::new();
        looking_at_the_part.resize(800, 600);
        let mut scene_with_part =
            LiveScene::new((), Vec::new(), FaceNames::default(), Visibility::default());
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
            Visibility::default(),
        );
        live.selection = chosen.clone();
        live.hovered = Marked::Face(scene.snapshot.face_of(0, 1).expect("numbered"));
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);

        // A load that failed changes nothing at all, including which face is
        // chosen and what the pointer was over.
        commit_scene(&mut live, &mut camera, Err(CadError::input("no")))
            .expect_err("a failed load commits nothing");
        assert_eq!(live.selection, chosen);
        assert_eq!(
            live.hovered,
            Marked::Face(scene.snapshot.face_of(0, 1).expect("numbered"))
        );

        // A load that arrived replaces all of it at once.
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut live,
            &mut camera,
            Ok((
                framed,
                (),
                vec![a_body()],
                FaceNames::default(),
                Visibility::default(),
            )),
        )
        .expect("a load that arrived commits");
        assert_eq!(live.selection, Selection::Nothing);
        assert_eq!(live.hovered, Marked::Nothing);
    }

    #[test]
    fn a_face_of_the_replaced_picture_chooses_nothing_in_the_next_one() {
        let (_directory, scene, _) = plate_with_a_chosen_face();
        let stale = scene.snapshot.face_of(0, 0).expect("numbered");

        // Another picture entirely, with no durable face names at all.
        let picture = distant_scene();
        let pick = picture.pick_of(0).expect("drawn");
        assert_eq!(
            Selection::at(pick, stale, &picture, &FaceNames::default()),
            Selection::Definition(pick),
            "a face of the replaced picture attached itself to the new one"
        );
    }

    #[test]
    fn the_inspector_and_the_marked_pixels_describe_one_selection() {
        let (_directory, scene, chosen) = plate_with_a_chosen_face();
        let words = words_of(&chosen, &scene.catalogue, &scene.snapshot);
        let face_names: Vec<ferritecad_ui::FaceName<'_>> =
            words.faces.iter().map(face_name).collect();
        let described = inspected(
            &chosen,
            &scene.catalogue,
            &words.identities,
            &face_names,
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
        let face_names: Vec<ferritecad_ui::FaceName<'_>> =
            words.faces.iter().map(face_name).collect();
        let described = inspected(
            &definition,
            &scene.catalogue,
            &words.identities,
            &face_names,
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
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Marked::Nothing,
        };

        // The face a pixel of this picture would report.
        let face = FacePickId::from_raw(1, &picture);
        assert_eq!(picture.definition_of_face(face), Some(0));

        assert!(hover(&mut scene.hovered, &picture, Marked::Face(face)));
        assert_eq!(scene.hovered, Marked::Face(face));
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
            Marked::Definition(chosen)
        ));
        assert_eq!(scene.hovered, Marked::Definition(chosen));

        // A face of a picture that has been replaced marks nothing here,
        // however plausible its number looks: the other picture numbers its
        // faces from one as well.
        assert!(hover(
            &mut scene.hovered,
            &other,
            Marked::Face(FacePickId::from_raw(1, &picture))
        ));
        assert_eq!(scene.hovered, Marked::Nothing);
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
            visibility: Visibility::default(),
            selection: Selection::Definition(chosen),
            hovered: Marked::Definition(chosen),
        };
        let mut camera = ViewportInput::new();
        camera.resize(800, 600);

        // A load that failed keeps the scene it could not replace, including
        // what the pointer was over.
        commit_scene(&mut scene, &mut camera, Err(CadError::input("no")))
            .expect_err("a failed load commits nothing");
        assert_eq!(scene.hovered, Marked::Definition(chosen));
        assert_eq!(scene.selection, Selection::Definition(chosen));

        // A load that arrived replaces all of it: the question belonged to the
        // previous picture as much as the answer did.
        let mut framed = ViewportInput::new();
        framed.resize(640, 480);
        commit_scene(
            &mut scene,
            &mut camera,
            Ok((
                framed,
                (),
                vec![a_body()],
                FaceNames::default(),
                Visibility::default(),
            )),
        )
        .expect("a load that arrived commits");
        assert_eq!(scene.hovered, Marked::Nothing);
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
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Definition(picture.pick_of(1).expect("the picture has that row")),
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Definition(
                picture.pick_of(mesh).expect("the picture has that row"),
            ),
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Definition(
                picture.pick_of(chosen).expect("the picture has that row"),
            ),
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Definition(
                picture.pick_of(only).expect("the picture has that row"),
            ),
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Definition(picture.pick_of(0).expect("the picture has that row")),
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Nothing,
            hovered: Marked::Nothing,
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
            visibility: Visibility::default(),
            selection: Selection::Definition(
                picture
                    .draws()
                    .first()
                    .expect("the picture draws something")
                    .pick,
            ),
            hovered: Marked::Nothing,
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
                visibility: Visibility::default(),
                selection: Selection::Definition(draw.pick),
                hovered: Marked::Nothing,
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
}
