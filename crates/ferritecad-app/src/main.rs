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

use ferritecad_document::DOCUMENT_EXTENSION;
use ferritecad_kernel::{CancelToken, OperationContext, ProgressSink, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_scene::{LoadedScene, SceneItem, snapshot_of};
use ferritecad_types::{CadError, Result};
use ferritecad_ui::{Activity, Chosen, PointerButton, VIEWS, ViewportEvent, ViewportInput};
use ferritecad_viewport::{PickId, RenderSnapshot, SnapshotBuilder, StandardView};
use ferritecad_viewport_gpu::{PreparedSnapshot, Renderer, WindowSurface};
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

/// What a pick means for what is chosen.
///
/// A pick that names nothing in the picture it was read from chooses nothing:
/// clicking the background is how a person unchooses, and a pick left over
/// from a document that has since been replaced names a definition of a
/// picture nobody is looking at. Both arrive here as the same answer, because
/// both mean the same thing on screen.
fn selection_from(pick: PickId, snapshot: &RenderSnapshot) -> PickId {
    match snapshot.definition(pick) {
        Some(_) => pick,
        None => PickId::NOTHING,
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
) -> Result<(ViewportInput, P, Vec<SceneItem>)> {
    let mut input = current_input.clone();
    let loaded = loaded?;
    let snapshot = input.accept_load(Ok(loaded.snapshot))?;
    let prepared = prepare(Arc::new(snapshot))?;
    Ok((input, prepared, loaded.catalogue))
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
    prepared: PreparedSnapshot,
    /// What each mesh of `prepared` is, in terms a document could store.
    /// Replaced with the picture it describes, so the two cannot disagree.
    catalogue: Vec<SceneItem>,
    egui: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

struct App {
    live: Option<Live>,
    input: ViewportInput,
    proxy: EventLoopProxy<AppEvent>,
    frames: FrameScheduler,
    document: PathBuf,
    loads: Loads,
    /// What is chosen, for as long as the picture it was read from is the one
    /// on screen. Never written down: see `ferritecad_scene::SceneItem` for
    /// the half of a click that can be.
    selection: PickId,
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
                let selection = live.chosen_name(self.selection);
                let activity = Activity {
                    line: &line,
                    progress: self.loads.status().fraction(),
                    selection: selection.as_deref(),
                };
                match live.draw(&self.input, activity, self.selection) {
                    // A button pressed during this frame reaches the camera
                    // the same way a keystroke does, through the reducer.
                    Ok(chosen) => {
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
                        // Asked after the frame that was clicked has been
                        // published, and only when somebody clicked: answering
                        // means drawing the model again offscreen to read one
                        // pixel of it.
                        if let Some((x, y)) = self.input.take_pick() {
                            self.choose_at(x, y);
                        }
                    }
                    Err(error) => {
                        eprintln!("ferritecad: {error}");
                        event_loop.exit();
                    }
                }
            }
            other => {
                for event in translate(&other) {
                    let claimed = match event {
                        ViewportEvent::Wheel { .. } => {
                            response.consumed || live.egui.egui_wants_pointer_input()
                        }
                        ViewportEvent::PointerPressed(_) => {
                            response.consumed || live.egui.egui_wants_pointer_input()
                        }
                        // A move is claimed only while no gesture is running;
                        // the reducer keeps a drag that began in the viewport.
                        _ => response.consumed,
                    };
                    self.input.handle(event, claimed);
                }
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
            selection: PickId::NOTHING,
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

        match live
            .renderer
            .render(&live.prepared, self.input.camera(), PickId::NOTHING)
        {
            Ok(frame) => {
                let chosen = selection_from(frame.pick_at(x, y), frame.snapshot());
                if chosen != self.selection {
                    self.selection = chosen;
                    self.input.request_redraw();
                }
            }
            // A failed pick chooses nothing and changes nothing. The model is
            // still on screen and still correct; the click simply went
            // unanswered.
            Err(error) => eprintln!("ferritecad: {error}"),
        }
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

        let (input, prepared, catalogue) = prepare_load(&self.input, loaded, |snapshot| {
            live.renderer.prepare(snapshot)
        })?;

        // Every operation above can fail; neither assignment can. No event
        // observes the application between these two lines, so camera and
        // resident geometry become current together – and only after both is
        // the document a thing the window may call ready.
        self.input = input;
        live.prepared = prepared;
        live.catalogue = catalogue;
        // The selection is deliberately not cleared here. A pick is bound to
        // the picture that issued it, and both places that read one – the
        // renderer and the catalogue – answer "nothing" for a pick this
        // picture does not recognise. Clearing as well would be a third copy
        // of that rule, and the one nobody could test.
        Ok(())
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
            prepared,
            catalogue: Vec::new(),
            egui,
            egui_state,
            egui_renderer,
        })
    }
}

impl Live {
    /// What is chosen, in the words the document or the file used for it.
    ///
    /// The catalogue is indexed the way the picture is, so this is a lookup
    /// rather than a search, and it answers `None` for a pick this picture
    /// does not recognise – which is the same answer as nothing chosen.
    fn chosen_name(&self, selection: PickId) -> Option<String> {
        let definition = self.prepared.snapshot().definition(selection)?;
        match self.catalogue.get(definition)? {
            SceneItem::Body(object) => Some(format!("Body {object}")),
            SceneItem::Imported(reference) => Some(reference.definition_key().to_owned()),
            // A kind of thing this build does not know how to name. Saying
            // nothing is better than inventing a name for it.
            _ => None,
        }
    }

    /// One frame: the model, then the interface, then publication.
    ///
    /// One texture, acquired once. The order is not a convention here – the
    /// seam enforces it, because the model's pass is what clears the target
    /// and the type only offers a view to draw into afterwards.
    fn draw(
        &mut self,
        input: &ViewportInput,
        activity: Activity<'_>,
        selected: PickId,
    ) -> Result<Chosen> {
        let Some(frame) = self.surface.begin(&mut self.renderer)? else {
            // No area, nobody watching, or the compositor was busy. None of
            // those is an error.
            return Ok(Chosen::default());
        };
        let frame = frame.draw_scene(&self.prepared, input.camera(), selected)?;

        let raw_input = self.egui_state.take_egui_input(&self.window);
        let mut chosen = Chosen::default();
        let mut output = self.egui.run_ui(raw_input, |ui| {
            // The panel returns what was asked for and applies nothing. What a
            // request means to the camera is the reducer's, and having one
            // place for that is what stops a button and a keystroke drifting
            // apart.
            chosen = ferritecad_ui::toolbar(ui, activity);
        });
        self.egui_state
            .handle_platform_output(&self.window, output.platform_output);

        let (width, height) = frame.size();
        let descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: self.egui.pixels_per_point(),
        };
        let primitives = self
            .egui
            .tessellate(output.shapes, self.egui.pixels_per_point());
        let mut textures = std::mem::take(&mut output.textures_delta);
        upload_textures(&mut textures, |id, delta| {
            self.egui_renderer
                .update_texture(frame.device(), frame.queue(), id, delta);
        });

        let mut encoder = frame
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ferritecad interface"),
            });
        self.egui_renderer.update_buffers(
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
            self.egui_renderer
                .render(&mut pass, &primitives, &descriptor);
        }
        frame.queue().submit(Some(encoder.finish()));

        free_textures(&mut textures, |id| self.egui_renderer.free_texture(id));
        frame.present();
        Ok(chosen)
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
        WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
            named_view(&event.logical_key)
                .map(|view| vec![ViewportEvent::Look(view)])
                .unwrap_or_default()
        }
        WindowEvent::Focused(false) => vec![ViewportEvent::GestureCancelled],
        _ => Vec::new(),
    }
}

fn button_of(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Middle => Some(PointerButton::Middle),
        MouseButton::Right => Some(PointerButton::Secondary),
        _ => None,
    }
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
mod tests {
    use std::time::Duration;

    use super::*;

    /// A picture with nothing catalogued, for tests that only look at where
    /// the camera ends up.
    fn loaded(snapshot: RenderSnapshot) -> LoadedScene {
        LoadedScene {
            snapshot,
            catalogue: Vec::new(),
        }
    }

    fn distant_scene() -> RenderSnapshot {
        scene_at(900.0)
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
                Ok((updated, (), _)) => {
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
        assert_eq!(selection_from(something, &snapshot), something);

        // The background is nothing, and that is how a person unchooses.
        assert_eq!(
            selection_from(PickId::NOTHING, &snapshot),
            PickId::NOTHING,
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
        assert_eq!(selection_from(chosen, &before), chosen);
        assert_eq!(
            selection_from(chosen, &after),
            PickId::NOTHING,
            "a choice made in the previous document was applied to this one"
        );
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
    fn focus_loss_translates_to_gesture_cancellation() {
        assert_eq!(
            translate(&WindowEvent::Focused(false)),
            vec![ViewportEvent::GestureCancelled]
        );
        assert!(translate(&WindowEvent::Focused(true)).is_empty());
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
