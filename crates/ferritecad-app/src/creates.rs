// SPDX-License-Identifier: MIT
//! Making a new document from the window, away from the event loop.
//!
//! # The work is not here
//!
//! What a new document *is* — the display units, the sample plate's plane,
//! profile, extrusion and references, the transaction they go in through, the
//! scratch file, the atomic publication — is
//! [`ferritecad_jobs::create_document`], which is the same route the shipped
//! command takes. Nothing in this file writes a byte of SQLite, decides what a
//! plate is made of, or knows what an exit code is. What is here is everything
//! that is about a *window*: when the action is offered, what the form holds,
//! what happens while it runs, and what the user is shown afterwards.
//!
//! # A created document is not yet the document on screen
//!
//! Creating publishes a file and stops. What the window shows afterwards is the
//! result of opening that file the ordinary way, on the thread that opens every
//! other document, and the two are reported apart: a file that was written and
//! then could not be shown is two facts, and a window that had one place to say
//! them would have to drop one. Until that Open is accepted, the picture, the
//! choice made in it and the document an export reads are all exactly what they
//! were.
//!
//! A polygon draft is owned separately by `sketch::Editor`. It is disposable
//! input, not the accepted document. Save cancellation and job failures retain
//! it; a published result ends its history before the ordinary async Open.
//!
//! # Nothing here blocks the loop, and nothing here replaces a file
//!
//! Writing a document is short, but it is filesystem work and it runs on a
//! thread of its own, cancelled and joined before this process ends. And the
//! shared operation has no way to overwrite anything: a destination that is
//! taken is refused in the window's own words, which say to pick another name
//! rather than offering a replacement that could not happen.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use ferritecad_jobs::{CreateDocumentRequest, CreatedDocument, NewDocument, PlateSize};
use ferritecad_kernel::{CancelToken, OperationContext};
use ferritecad_types::{ErrorKind, Result};
use ferritecad_ui::{NewChoice, NewContent, NewDocumentForm, ViewportInput};

/// What the window tells a person about a destination that is already taken.
///
/// The window's own words. The command line finishes the same sentence with
/// what `create` would have done to the file; there is no flag and no button
/// here that authorises a replacement, so this says the one thing that does
/// work — and says it instead of offering a replacement the shared operation
/// would refuse anyway.
pub(crate) const CHOOSE_ANOTHER_NAME: &str = "choose a different file name";

/// What the window says while a document is being made.
const CREATING: &str = "Creating…";
/// A document that was written. Whether it can be shown is a separate answer
/// and is reported separately.
const CREATED: &str = "Created";
/// A document that was not made. Nothing was written and nothing at the
/// destination changed.
const CREATE_FAILED: &str = "Could not create";
/// A creation the window gave up on. No file was published.
const CREATE_CANCELLED: &str = "New document cancelled";

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

/// A creation that has been started and not yet joined.
struct Creating {
    cancel: CancelToken,
    worker: JoinHandle<()>,
}

/// What the window says about the document it was last asked to make.
///
/// Entirely separate from what it says about opening one. A creation that
/// succeeded says so even when the document it produced then failed to open,
/// because those are two things that happened and a person needs both.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) enum CreateStatus {
    /// Nothing has been asked for. True until the first New.
    #[default]
    Idle,
    /// Running. Nothing is at the destination yet.
    Running {
        generation: CreateGeneration,
        destination: String,
    },
    /// A document was published at the destination.
    Made { destination: String },
    /// Given up on. Nothing was published.
    Cancelled { destination: String },
    /// Could not be made. Whatever was at the destination is still there.
    Failed {
        destination: String,
        message: String,
    },
}

impl CreateStatus {
    /// The line to put in front of the user.
    fn line(&self) -> String {
        match self {
            Self::Idle => String::new(),
            Self::Running { destination, .. } => format!("{CREATING} {destination}"),
            Self::Made { destination } => format!("{CREATED} {destination}"),
            Self::Cancelled { destination } => format!("{CREATE_CANCELLED}: {destination}"),
            Self::Failed {
                destination,
                message,
            } => format!("{CREATE_FAILED} {destination}: {message}"),
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

    /// The form on screen, to be drawn and typed into.
    #[cfg(test)]
    pub(crate) fn form(&mut self) -> Option<&mut NewDocumentForm> {
        self.form.as_mut()
    }

    /// Whether a creation is running, which is what makes New unavailable.
    pub(crate) fn running(&self) -> bool {
        matches!(self.status, CreateStatus::Running { .. })
    }

    pub(crate) fn busy(&self) -> bool {
        self.running() || self.form.is_some() || self.sketch.active()
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
        self.form = Some(NewDocumentForm {
            content: NewContent::Empty,
            width: millimetres(PlateSize::DEFAULT.width),
            depth: millimetres(PlateSize::DEFAULT.depth),
            height: millimetres(PlateSize::DEFAULT.height),
            refusal: None,
        });
    }

    /// Takes the form away without making anything.
    fn dismiss(&mut self) -> bool {
        self.form.take().is_some()
    }

    /// Starts a creation, abandoning whatever was already running.
    ///
    /// `spawn` is handed the destination, what to put in the document, the
    /// generation to label its answer with and the token that stops it.
    /// Starting and recording are one operation, so there is no arrangement of
    /// calls in which a running worker is untracked.
    fn start(
        &mut self,
        destination: &Path,
        content: NewDocument,
        spawn: impl FnOnce(&Path, NewDocument, CreateGeneration, &CancelToken) -> JoinHandle<()>,
    ) -> CreateGeneration {
        for creating in &self.running {
            creating.cancel.cancel();
        }
        // The form asked a question that has now been answered.
        self.form = None;

        self.cancel_requested = false;
        self.issued += 1;
        let generation = CreateGeneration(self.issued);
        let cancel = CancelToken::new();
        let worker = spawn(destination, content, generation, &cancel);
        self.running.push(Creating { cancel, worker });
        self.current = Some(generation);
        self.status = CreateStatus::Running {
            generation,
            destination: destination.display().to_string(),
        };
        generation
    }

    /// Whether this answer is still the one that was asked for.
    fn accepts(&self, generation: CreateGeneration) -> bool {
        self.current == Some(generation)
    }

    /// Notes what a generation answered, and joins whatever has ended.
    ///
    /// The outcome is reported for every answer, current or not, and this
    /// decides what it is worth. An answer to a request that has been replaced
    /// changes nothing at all, and above all does not send the window off to
    /// open a document nobody asked for.
    ///
    /// Returns the document to open, when there is one and it is still wanted.
    fn answered(
        &mut self,
        generation: CreateGeneration,
        outcome: Result<CreatedDocument>,
    ) -> (bool, Option<PathBuf>) {
        let mut open = None;
        let changed = match &self.status {
            CreateStatus::Running {
                generation: waiting,
                destination,
            } if *waiting == generation && self.accepts(generation) => {
                let destination = destination.clone();
                self.status = match outcome {
                    Ok(created) => {
                        self.sketch.dismiss();
                        if !self.cancel_requested {
                            open = Some(created.destination().to_path_buf());
                        }
                        CreateStatus::Made { destination }
                    }
                    // Giving up is not a failure, and a window that reported
                    // it as one would be complaining about something it was
                    // asked to do.
                    Err(error) if error.kind() == ErrorKind::Cancellation => {
                        CreateStatus::Cancelled { destination }
                    }
                    Err(error) => CreateStatus::Failed {
                        destination,
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
        (changed, open)
    }

    /// Stops every creation and waits for all of them.
    ///
    /// The one place that blocks, and the last thing that happens. A worker
    /// that was cut short by process exit would leave its scratch directory
    /// beside somebody's documents.
    pub(crate) fn stop_all(&mut self) {
        self.current = None;
        self.form = None;
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
/// The form is deliberately not taken down here. The next thing that happens is
/// a system dialog, and a person who closes that dialog has not thrown away the
/// sizes they typed.
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

/// Acts on a chosen destination, and makes any visible change once.
///
/// Returns the generation when a creation really started. A closed dialog does
/// nothing at all: no worker, no status, no file, and the form stays exactly as
/// it was so the sizes are still there to try again with.
pub(crate) fn begin_create(
    creates: &mut Creates,
    input: &mut ViewportInput,
    content: NewDocument,
    chosen: Option<PathBuf>,
    spawn: impl FnOnce(&Path, NewDocument, CreateGeneration, &CancelToken) -> JoinHandle<()>,
) -> Option<CreateGeneration> {
    if creates.running() {
        return None;
    }
    let destination = chosen?;
    let generation = creates.start(&destination, content, spawn);
    input.request_redraw();
    Some(generation)
}

/// Finishes an answer at the application boundary.
///
/// [`Creates::answered`] is the one generation check, and its answer controls
/// both things visible outside that state machine: a redraw, and the document
/// the window goes on to open.
pub(crate) fn finish_create(
    creates: &mut Creates,
    input: &mut ViewportInput,
    generation: CreateGeneration,
    outcome: Result<CreatedDocument>,
) -> Option<PathBuf> {
    let (changed, open) = creates.answered(generation, outcome);
    if changed {
        input.request_redraw();
    }
    open
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
/// alive while a document is written.
pub(crate) fn spawn_create(
    create: impl FnOnce() -> Result<CreatedDocument> + Send + 'static,
    deliver: impl FnOnce(Result<CreatedDocument>) + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || deliver(create()))
}

/// The whole of the work, and the only place this application does any of it.
///
/// One shared operation. The factory opens a worker-owned kernel only for a
/// polygon; empty/sample documents retain their kernel-free creation behavior.
pub(crate) fn run_create(
    destination: &Path,
    content: NewDocument,
    context: &OperationContext,
) -> Result<CreatedDocument> {
    ferritecad_jobs::create_document_with_kernel(
        CreateDocumentRequest::new(destination, content, CHOOSE_ANOTHER_NAME),
        ferritecad_occt::OcctKernel::new,
        context,
    )
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
pub(crate) mod tests {
    use super::*;

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

    /// Drives one creation the way the event loop does, with the real work
    /// behind it and no window anywhere.
    ///
    /// This is the window's own route and not a second one: the same
    /// [`begin_create`] the frame calls, handed the same [`run_create`] the
    /// spawner in `main` hands it, and the answer delivered back through
    /// [`finish_create`] exactly as the event loop delivers it. What is left
    /// out is the winit event and the system dialog, which is what "without
    /// reproducing clicks" means.
    pub(crate) fn run_to_completion(
        creates: &mut Creates,
        input: &mut ViewportInput,
        content: NewDocument,
        chosen: Option<PathBuf>,
    ) -> (Option<CreateGeneration>, Option<PathBuf>) {
        let (answers, answered) = mpsc::channel();
        let Some(generation) = crate::start_new(
            creates,
            &crate::Loads::default(),
            &crate::exports::Exports::default(),
            input,
            content,
            chosen,
            |destination, content, generation, cancel| {
                let destination = destination.to_path_buf();
                let context = OperationContext::default().with_cancel(cancel.clone());
                spawn_create(
                    move || run_create(&destination, content, &context),
                    move |result| {
                        let _ = answers.send((generation, result));
                    },
                )
            },
        ) else {
            return (None, None);
        };
        let (answered_generation, result) = answered.recv().expect("the creation answered");
        let open = finish_create(creates, input, answered_generation, result);
        (Some(generation), open)
    }

    /// The whole of the window's route, from the button to the file, as one
    /// call: press New, fill the form in, choose a path, let it finish.
    fn create_through_the_window(
        destination: &Path,
        content: NewContent,
        sizes: [&str; 3],
    ) -> (Creates, Option<PathBuf>) {
        let mut creates = Creates::default();
        let mut input = input();

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
        let (_, open) = run_to_completion(
            &mut creates,
            &mut input,
            asked,
            Some(destination.to_path_buf()),
        );
        (creates, open)
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

    /// A closed save dialog makes nothing, and keeps the sizes that were
    /// typed so the person can try again.
    #[test]
    fn a_closed_dialog_makes_nothing_and_keeps_the_form() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut creates = Creates::default();
        let mut input = input();

        open_form(&mut creates, &mut input);
        let form = creates.form().expect("the form is on screen");
        form.content = NewContent::SamplePlate;
        form.width = "125".to_owned();
        let content =
            answer_form(&mut creates, &mut input, NewChoice::Create).expect("the form is complete");
        let _ = input.take_redraw();

        let (generation, open) = run_to_completion(&mut creates, &mut input, content, None);

        assert_eq!(generation, None);
        assert_eq!(open, None);
        assert_eq!(creates.status(), &CreateStatus::Idle);
        assert_eq!(
            creates.form().expect("the form is still there").width,
            "125"
        );
        assert!(!input.take_redraw(), "a closed dialog owed a frame");
        assert!(entries(dir.path()).is_empty());
    }

    // ------------------------------------------------------ what it produces

    /// The window's route publishes a document and hands it back to be opened.
    #[test]
    fn the_window_route_publishes_the_document_and_asks_for_it_to_be_opened() {
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("plate.fcad");

        let (creates, open) =
            create_through_the_window(&destination, NewContent::SamplePlate, ["80", "50", "12"]);

        assert_eq!(open.as_deref(), Some(destination.as_path()));
        let CreateStatus::Made { destination: said } = creates.status() else {
            panic!(
                "the window does not say it made one: {:?}",
                creates.status()
            );
        };
        assert_eq!(said, &destination.display().to_string());
        // The form asked its question and has been answered.
        assert_eq!(entries(dir.path()), vec!["plate.fcad".to_owned()]);

        let document = Document::open(&destination).expect("opens what the window made");
        assert_eq!(document.objects().expect("reads objects").len(), 4);
        assert_eq!(document.topology_refs().expect("reads refs").len(), 3);
        document.close().expect("closes");
    }

    /// And an empty document, which is a different thing from an empty window.
    #[test]
    fn an_empty_document_is_a_document() {
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("empty.fcad");

        let (creates, open) =
            create_through_the_window(&destination, NewContent::Empty, ["60", "40", "10"]);

        assert_eq!(open.as_deref(), Some(destination.as_path()));
        assert!(matches!(creates.status(), CreateStatus::Made { .. }));

        let document = Document::open(&destination).expect("opens it");
        assert!(document.objects().expect("reads objects").is_empty());
        document.close().expect("closes");
    }

    // ------------------------------------------------------- what it refuses

    /// A destination that is taken is refused in the window's own words, and
    /// the file that is there is not touched.
    #[test]
    fn a_taken_destination_is_refused_without_offering_a_replacement() {
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("plate.fcad");
        std::fs::write(&destination, b"somebody else's file").expect("writes the file");

        let (creates, open) =
            create_through_the_window(&destination, NewContent::SamplePlate, ["80", "50", "12"]);

        assert_eq!(
            open, None,
            "a refused creation sent a document to be opened"
        );
        let CreateStatus::Failed { message, .. } = creates.status() else {
            panic!("the window does not say it failed: {:?}", creates.status());
        };
        assert!(message.contains(CHOOSE_ANOTHER_NAME), "{message}");
        // The window has no flag to offer and must not print one.
        assert!(!message.contains("--force"), "{message}");
        assert!(!message.contains("Replace"), "{message}");
        assert_eq!(
            std::fs::read(&destination).expect("reads it"),
            b"somebody else's file"
        );
        assert_eq!(entries(dir.path()), vec!["plate.fcad".to_owned()]);
    }

    /// A size the document will not store leaves nothing behind, and the
    /// window says so without pretending a file is there.
    ///
    /// `NaN` is a number as far as parsing goes, so it reaches the document
    /// and is refused by the same rule the command line meets.
    #[test]
    fn a_size_the_document_refuses_leaves_nothing_at_the_destination() {
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("bad.fcad");

        let (creates, open) =
            create_through_the_window(&destination, NewContent::SamplePlate, ["60", "40", "NaN"]);

        assert_eq!(open, None);
        let CreateStatus::Failed { message, .. } = creates.status() else {
            panic!("the window does not say it failed: {:?}", creates.status());
        };
        assert!(message.contains("must be finite"), "{message}");
        assert!(!destination.exists(), "a refused creation left a document");
        assert!(
            entries(dir.path()).is_empty(),
            "a refused creation left {:?}",
            entries(dir.path())
        );
    }

    /// An answer to a creation that has been replaced changes nothing at all,
    /// and above all does not send the window off to open a document.
    #[test]
    fn a_stale_answer_is_not_acted_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = dir.path().join("first.fcad");
        let second = dir.path().join("second.fcad");
        let mut creates = Creates::default();
        let mut input = input();

        let (stale, _) = run_to_completion(
            &mut creates,
            &mut input,
            NewDocument::Empty,
            Some(first.clone()),
        );
        let stale = stale.expect("the first creation started");
        let (_, open) = run_to_completion(
            &mut creates,
            &mut input,
            NewDocument::Empty,
            Some(second.clone()),
        );
        assert_eq!(open.as_deref(), Some(second.as_path()));
        let _ = input.take_redraw();

        // The first request's answer, arriving after the second replaced it.
        let late = finish_create(
            &mut creates,
            &mut input,
            stale,
            run_create(
                &dir.path().join("late.fcad"),
                NewDocument::Empty,
                &OperationContext::default(),
            ),
        );

        assert_eq!(
            late, None,
            "a stale answer asked for a document to be opened"
        );
        let CreateStatus::Made { destination } = creates.status() else {
            panic!("{:?}", creates.status());
        };
        assert_eq!(destination, &second.display().to_string());
        assert!(!input.take_redraw(), "a stale answer owed a frame");
    }

    /// New is unavailable exactly while a document is being made.
    #[test]
    fn a_creation_in_flight_is_the_only_thing_that_withdraws_new() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut creates = Creates::default();
        let mut input = input();

        assert!(!creates.running());
        let (answers, answered) = mpsc::channel();
        let destination = dir.path().join("plate.fcad");
        begin_create(
            &mut creates,
            &mut input,
            NewDocument::Empty,
            Some(destination.clone()),
            |destination, content, generation, cancel| {
                let destination = destination.to_path_buf();
                let context = OperationContext::default().with_cancel(cancel.clone());
                spawn_create(
                    move || run_create(&destination, content, &context),
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
        finish_create(&mut creates, &mut input, generation, result);
        assert!(!creates.running());
        creates.stop_all();
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
    fn cancel_waits_for_the_worker_and_never_claims_a_published_file_is_absent() {
        for late in [false, true] {
            let dir = tempfile::tempdir().expect("directory");
            let path = dir.path().join("result.fcad");
            let mut creates = Creates::default();
            let mut input = input();
            let (ready, reached) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            let (answers, answered) = mpsc::channel();
            begin_create(
                &mut creates,
                &mut input,
                NewDocument::Empty,
                Some(path.clone()),
                move |path, content, generation, cancel| {
                    let path = path.to_path_buf();
                    let context = OperationContext::default().with_cancel(cancel.clone());
                    spawn_create(
                        move || {
                            if late {
                                let result = run_create(&path, content, &context);
                                ready.send(()).expect("ready");
                                resume.recv().expect("release");
                                result
                            } else {
                                ready.send(()).expect("ready");
                                resume.recv().expect("release");
                                run_create(&path, content, &context)
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
            assert_eq!(
                finish_create(&mut creates, &mut input, generation, result),
                None
            );
            assert_eq!(path.exists(), late);
            if late {
                assert!(matches!(creates.status(), CreateStatus::Made { .. }));
            } else {
                assert!(matches!(creates.status(), CreateStatus::Cancelled { .. }));
                assert!(entries(dir.path()).is_empty());
            }
            creates.stop_all();
            assert!(creates.running.is_empty());
        }
    }

    #[test]
    fn shutdown_cancels_and_joins_creation_with_its_scratch_removed() {
        let dir = tempfile::tempdir().expect("directory");
        let path = dir.path().join("closing.fcad");
        let mut creates = Creates::default();
        let mut input = input();
        let (send, reached) = mpsc::channel();
        let (answer, answered) = mpsc::channel();
        begin_create(
            &mut creates,
            &mut input,
            NewDocument::Empty,
            Some(path),
            |path, content, _, cancel| {
                let path = path.to_path_buf();
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
                    move || run_create(&path, content, &context),
                    move |result| {
                        answer.send(result).expect("answer");
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
        assert!(entries(dir.path()).is_empty());
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
