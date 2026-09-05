// SPDX-License-Identifier: MIT
//! Writing the document on screen out as a file, away from the event loop.
//!
//! # The work is not here
//!
//! What an export *is* — the cold read of the document, the neutral scene, the
//! one call to the writer, the atomic publication — is
//! [`ferritecad_jobs::export_document_as_fbx`], which is the same route the
//! shipped command takes. Nothing in this file writes a byte of FBX, decides
//! what a file should contain, or knows what an exit code is. What is here is
//! everything that is about a *window*: when the action is offered, which
//! document it applies to, what happens while it runs, and what the user is
//! shown afterwards.
//!
//! # Which document is exported
//!
//! The one on screen, and never the one that was last asked for. Those are
//! different: asking to open a document replaces the path this application
//! remembers for its file dialog long before that document has been read, and
//! an Open that fails or is given up on never replaces the picture at all. So
//! the path an export reads is kept beside the picture and changes only when
//! [`crate::commit_scene`] accepts a new one — which is to say exactly when
//! what the user is looking at changes.
//!
//! # Cold, and honest about it
//!
//! The export re-reads the accepted `.fcad` from disk rather than writing out
//! what the viewer holds. That is what makes the file identical to the one the
//! command line produces, and it is worth saying what it does not promise:
//! this application has no unsaved model state to lose today, and an export
//! reads whatever is at that path at the moment it runs. A document replaced
//! on disk behind the viewer's back is exported as it now is, not as it is
//! drawn.
//!
//! # Nothing here blocks the loop
//!
//! An export of a real assembly is minutes of rebuilding and tessellation. It
//! runs on a thread that owns its own kernel session, reports back as one more
//! event, and is cancelled — and joined — before this process ends.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use ferritecad_exchange::Import;
use ferritecad_export::{ExportNodeId, ExportOmissionReport, ExportSource, FbxWriteReport};
use ferritecad_jobs::{
    Existing, FbxExport, FbxExportRequest, SOURCE_IS_DESTINATION, export_document_as_fbx,
    is_same_entry, path_entry_exists,
};
use ferritecad_kernel::{CancelToken, GeometryKernel, OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{ErrorKind, Result};
use ferritecad_ui::{
    ExportOutcome, OmittedDefinition, PublishedFile, ReplaceChoice, ViewportInput,
};

/// What the window says while an export is running.
const EXPORTING: &str = "Exporting…";
/// And after it has been asked to stop but has not stopped yet.
const CANCELLING: &str = "Cancelling…";
/// A published file that is the whole document.
///
/// Deliberately without a word of the vocabulary below it: somebody scanning a
/// window for what went wrong must not find anything here.
const EXPORTED: &str = "Exported";
/// A published file that is not the whole document.
///
/// Said in the status itself rather than only in the list underneath, because
/// the list can be scrolled past and this cannot.
const EXPORTED_WITH_OMISSIONS: &str = "Exported with missing geometry";
/// An export the user, or an Open, gave up on. No file was published.
const EXPORT_CANCELLED: &str = "Export cancelled";
/// An export that could not be done. Whatever was at the destination is still
/// there, and the model on screen is untouched.
const EXPORT_FAILED: &str = "Export failed";

/// What the window tells a person when a file appears at the destination
/// between their choosing it and the writer finishing.
///
/// The window's own words. The command line names the flag that would have
/// authorised the replacement; there is no such flag here, and printing one
/// would be telling somebody to type something into a window.
const APPEARED: &str = "it was created while the export was being written";

/// Which export request an answer belongs to.
///
/// Monotonic and never reused, on the same terms as a load: two exports of one
/// document to one destination are two requests, and the older one's answer is
/// as unwelcome as any other stale answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ExportGeneration(u64);

/// An export that has been started and not yet joined.
///
/// The worker is kept rather than dropped, for the reason a load's is: dropping
/// a `JoinHandle` detaches the thread, and a detached export owns an Open
/// CASCADE session nobody will ever wait for.
struct Exporting {
    cancel: CancelToken,
    worker: JoinHandle<()>,
}

/// What was written, counted the way the writer counted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WroteFile {
    bytes: u64,
    models: u32,
    geometries: u32,
    materials: u32,
}

impl WroteFile {
    fn of(report: &FbxWriteReport) -> Self {
        Self {
            bytes: report.bytes(),
            models: report.models(),
            geometries: report.geometries(),
            materials: report.materials(),
        }
    }

    fn shown<'a>(&self, destination: &'a str) -> PublishedFile<'a> {
        PublishedFile {
            destination,
            bytes: self.bytes,
            models: self.models,
            geometries: self.geometries,
            materials: self.materials,
        }
    }
}

/// One definition the published file has no triangles for, in words.
///
/// Written once, when the answer arrives, out of the typed record the writer
/// kept and out of nothing else. The scene is not read again — a second
/// opinion could disagree with the file that was published — and neither a
/// `Debug` rendering nor the text of an error becomes data here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OmittedWords {
    /// The definition's durable identity, qualified by where it came from: a
    /// source-local key means nothing without the source, because `#2583`
    /// occurs in most STEP files and names something different in each.
    definition: String,
    /// What the document recorded when the file was imported, whole.
    finding: String,
    /// This build's typed refusal, by its stable name. Not its message, which
    /// is written for a person and free to change.
    refusal: String,
    /// Every placement of it in the file, under the same key the writer put on
    /// that model. All of them, in the writer's order.
    placements: Vec<String>,
}

impl OmittedWords {
    fn of(report: &ExportOmissionReport) -> Self {
        Self {
            definition: identity(&report.source),
            finding: report.omission.finding.to_string(),
            refusal: report.omission.refusal.stable_name().to_owned(),
            placements: report.nodes.iter().map(node_key).collect(),
        }
    }

    fn shown(&self) -> OmittedDefinition<'_> {
        OmittedDefinition {
            definition: &self.definition,
            finding: &self.finding,
            refusal: &self.refusal,
            placements: &self.placements,
        }
    }
}

/// One definition's durable identity, qualified by where it came from.
fn identity(source: &ExportSource) -> String {
    match source {
        ExportSource::Body { object } => format!("body {object}"),
        ExportSource::Imported {
            source,
            definition_key,
        } => format!("imported source {source}  key {definition_key}"),
    }
}

/// One placement, named the way the published file names it.
///
/// `node/<n>` is not a number that means something only while this process
/// runs: it is exactly the `FerriteCADNodeKey` property the writer put on that
/// model, so a person holding this window can find the node in the file.
fn node_key(node: &ExportNodeId) -> String {
    format!("node/{}", node.index())
}

/// What the window says about the export it was last asked for.
///
/// Entirely separate from what it says about opening a document. An export
/// that failed is not a document that failed to open: the model is on screen,
/// it is correct, and only the file was not written.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) enum ExportStatus {
    /// Nothing has been asked for. True until the first export.
    #[default]
    Idle,
    /// Running. `giving_up` is a request that has been withdrawn and has not
    /// yet answered — one state rather than two, because the work in flight is
    /// the same work either way.
    Running {
        generation: ExportGeneration,
        destination: String,
        giving_up: bool,
    },
    /// A file was published. Whether it is the whole document is whether
    /// anything is missing from it, and nothing else.
    Wrote {
        destination: String,
        file: WroteFile,
        omissions: Vec<OmittedWords>,
    },
    /// Given up on. Nothing was published and nothing at the destination
    /// changed.
    Cancelled { destination: String },
    /// Could not be done. Whatever was at the destination is still there.
    Failed {
        destination: String,
        message: String,
    },
}

impl ExportStatus {
    /// What a finished export says, in the words the window keeps.
    ///
    /// The one conversion from the writer's own record into anything a person
    /// reads. Built when the answer arrives and never again: the scene is not
    /// read a second time, because a second opinion about what is missing can
    /// disagree with the file that was published.
    fn of(destination: String, report: &FbxWriteReport) -> Self {
        Self::Wrote {
            destination,
            file: WroteFile::of(report),
            // Every one of them, in the order the writer kept them.
            omissions: report.omissions().iter().map(OmittedWords::of).collect(),
        }
    }

    /// The line to put in front of the user.
    fn line(&self) -> String {
        match self {
            Self::Idle => String::new(),
            Self::Running {
                destination,
                giving_up: false,
                ..
            } => format!("{EXPORTING} {destination}"),
            Self::Running {
                destination,
                giving_up: true,
                ..
            } => format!("{CANCELLING} {destination}"),
            // The one place the difference between a whole export and a
            // partial one is decided, and it is decided by whether anything is
            // missing rather than by anything a caller passed in.
            Self::Wrote {
                destination,
                omissions,
                ..
            } if omissions.is_empty() => format!("{EXPORTED} {destination}"),
            Self::Wrote { destination, .. } => {
                format!("{EXPORTED_WITH_OMISSIONS}: {destination}")
            }
            Self::Cancelled { destination } => format!("{EXPORT_CANCELLED}: {destination}"),
            Self::Failed {
                destination,
                message,
            } => format!("{EXPORT_FAILED}: {destination}: {message}"),
        }
    }

    /// Every omission, in the words a panel may use.
    ///
    /// Built per frame and borrowed from what the answer wrote down, on the
    /// same terms as a failed Open's constraints: the panel is handed finished
    /// text and the frame owns it.
    fn omissions(&self) -> Vec<OmittedDefinition<'_>> {
        match self {
            Self::Wrote { omissions, .. } => omissions.iter().map(OmittedWords::shown).collect(),
            _ => Vec::new(),
        }
    }

    /// What the section shows this frame, borrowed from this frame's words.
    fn shown<'a>(
        &'a self,
        line: &'a str,
        omissions: &'a [OmittedDefinition<'a>],
    ) -> Option<ExportOutcome<'a>> {
        if matches!(self, Self::Idle) {
            return None;
        }
        Some(ExportOutcome {
            line,
            file: match self {
                Self::Wrote {
                    destination, file, ..
                } => Some(file.shown(destination)),
                _ => None,
            },
            omissions,
        })
    }
}

/// Every export this window has started and not yet finished with.
///
/// The same shape as the loads beside it, and for the same reasons: one
/// request may be current, an older answer changes nothing, and no worker is
/// ever left running with nobody to wait for it.
#[derive(Default)]
pub(crate) struct Exports {
    issued: u64,
    /// The request whose answer may still reach the screen.
    current: Option<ExportGeneration>,
    running: Vec<Exporting>,
    status: ExportStatus,
    /// A destination the user chose that something is already at, waiting for
    /// them to say whether it may be replaced.
    ///
    /// Nothing is written while this is set. It is one path rather than a
    /// list: a second choice replaces the first, because the question is about
    /// the file the user just named.
    pending: Option<PathBuf>,
}

impl Exports {
    /// What the window should be saying about exporting.
    pub(crate) fn status(&self) -> &ExportStatus {
        &self.status
    }

    /// The destination waiting to be confirmed, if one is.
    pub(crate) fn pending(&self) -> Option<&Path> {
        self.pending.as_deref()
    }

    /// Whether an export is running, which is what makes stopping one
    /// something a window can offer.
    pub(crate) fn running(&self) -> bool {
        matches!(
            self.status,
            ExportStatus::Running {
                giving_up: false,
                ..
            }
        )
    }

    /// Asks whether a file already at `destination` may be replaced.
    ///
    /// Nothing is started and nothing on screen about a previous export
    /// changes: this is a question, and a question that is never answered must
    /// leave the window exactly as it was.
    fn ask(&mut self, destination: PathBuf) {
        self.pending = Some(destination);
    }

    /// Takes back the question without answering it.
    fn dismiss(&mut self) -> bool {
        self.pending.take().is_some()
    }

    /// Replaces the current request with a refusal that already happened.
    ///
    /// A terminal refusal is a new answer, not merely a line of text. Any
    /// older worker whose line it replaces must lose both the authority to
    /// publish and the right to answer; otherwise the window says there is no
    /// export to cancel while that worker can still put a file in place.
    fn refuse(&mut self, destination: String, message: String) {
        self.pending = None;
        for exporting in &self.running {
            exporting.cancel.cancel();
        }
        self.current = None;
        self.status = ExportStatus::Failed {
            destination,
            message,
        };
    }

    /// Starts an export, abandoning whatever was already running.
    ///
    /// `spawn` is handed the destination, whether a replacement was
    /// authorised, the generation to label its answer with and the token that
    /// stops it. Starting and recording are one operation, so there is no
    /// arrangement of calls in which a running worker is untracked.
    fn start(
        &mut self,
        destination: &Path,
        replace: bool,
        spawn: impl FnOnce(&Path, bool, ExportGeneration, &CancelToken) -> JoinHandle<()>,
    ) -> ExportGeneration {
        for exporting in &self.running {
            exporting.cancel.cancel();
        }
        // A question about some other file is over: this is the export the
        // user is asking for now.
        self.pending = None;

        self.issued += 1;
        let generation = ExportGeneration(self.issued);
        let cancel = CancelToken::new();
        let worker = spawn(destination, replace, generation, &cancel);
        self.running.push(Exporting { cancel, worker });
        self.current = Some(generation);
        self.status = ExportStatus::Running {
            generation,
            destination: destination.display().to_string(),
            giving_up: false,
        };
        generation
    }

    /// Whether this answer is still the one that was asked for.
    fn accepts(&self, generation: ExportGeneration) -> bool {
        self.current == Some(generation)
    }

    /// Asks the export in flight to stop, and says so.
    ///
    /// The request stays current: it is still the export the window is
    /// describing, and it is its own answer — a cancellation — that will say
    /// so. A pending question about replacing a file goes too, because it was
    /// about the document that is being left behind.
    ///
    /// Returns whether the line changed.
    fn cancel_current(&mut self) -> bool {
        let asked = self.dismiss();
        for exporting in &self.running {
            exporting.cancel.cancel();
        }
        match &mut self.status {
            ExportStatus::Running { giving_up, .. } if !*giving_up => {
                *giving_up = true;
                true
            }
            _ => asked,
        }
    }

    /// Notes what a generation answered, and joins whatever has ended.
    ///
    /// The outcome is reported for every answer, current or not, and this
    /// decides what it is worth. An answer to a request that has been replaced
    /// changes nothing at all: not the line, not the file rows, not the list
    /// of what is missing.
    ///
    /// Returns whether the line changed.
    fn answered(&mut self, generation: ExportGeneration, outcome: Result<FbxExport>) -> bool {
        let changed = match &self.status {
            ExportStatus::Running {
                generation: waiting,
                destination,
                ..
            } if *waiting == generation => {
                let destination = destination.clone();
                self.status = match outcome {
                    Ok(exported) => ExportStatus::of(destination, exported.report()),
                    // Giving up is not a failure, and a window that reported
                    // it as one would be complaining about something the user
                    // asked for.
                    Err(error) if error.kind() == ErrorKind::Cancellation => {
                        ExportStatus::Cancelled { destination }
                    }
                    Err(error) => ExportStatus::Failed {
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
        changed
    }

    /// Stops every export and waits for all of them.
    ///
    /// The one place that blocks, and the last thing that happens: a worker
    /// owns a kernel session, and leaving it to be cut short by process exit
    /// would make the one path that releases geometry the one nobody takes.
    pub(crate) fn stop_all(&mut self) {
        self.current = None;
        self.pending = None;
        for exporting in &self.running {
            exporting.cancel.cancel();
        }
        // Cancelled first, all of them, and only then waited for.
        for exporting in self.running.drain(..) {
            let _ = exporting.worker.join();
        }
    }
}

/// What a chosen destination means, before any work begins.
///
/// The whole decision, made from two paths and the filesystem, with no window
/// anywhere near it. What a real save dialog returns is a `Some` or a `None`,
/// and that is exactly what this is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExportRequest {
    /// Nothing at all: no document has been accepted, or the dialog was
    /// closed. Exactly nothing happens — no worker, no status, no file.
    Nothing,
    /// The document itself was chosen as its own output.
    RefusedSource,
    /// Something is already there. The user is asked before anything is
    /// written.
    Confirm(PathBuf),
    /// Nothing is there. The export may start.
    Start(PathBuf),
}

/// What to do about a destination the user chose.
///
/// `source` is the document on screen, which is `None` before anything has
/// been accepted; a direct call with no document starts nothing, so a window
/// whose button was somehow pressed too early cannot export a file nobody is
/// looking at.
pub(crate) fn requested(source: Option<&Path>, chosen: Option<PathBuf>) -> Result<ExportRequest> {
    let (Some(source), Some(destination)) = (source, chosen) else {
        return Ok(ExportRequest::Nothing);
    };

    // First, and whatever anybody confirms afterwards: a document that is its
    // own destination is destroyed by being exported to.
    if is_same_entry(source, &destination)? {
        return Ok(ExportRequest::RefusedSource);
    }
    if path_entry_exists(&destination)? {
        return Ok(ExportRequest::Confirm(destination));
    }
    Ok(ExportRequest::Start(destination))
}

/// Acts on what the user chose, and makes any visible change once.
///
/// Returns the generation when an export really started. A closed dialog, a
/// destination that needs confirming and a document that is its own output all
/// start nothing; only the last of the three says anything on screen.
pub(crate) fn begin_export(
    exports: &mut Exports,
    input: &mut ViewportInput,
    source: Option<&Path>,
    chosen: Option<PathBuf>,
    spawn: impl FnOnce(&Path, bool, ExportGeneration, &CancelToken) -> JoinHandle<()>,
) -> Option<ExportGeneration> {
    match requested(source, chosen) {
        Ok(ExportRequest::Nothing) => None,
        Ok(ExportRequest::RefusedSource) => {
            exports.refuse(
                source.map(display).unwrap_or_default(),
                SOURCE_IS_DESTINATION.to_owned(),
            );
            input.request_redraw();
            None
        }
        Ok(ExportRequest::Confirm(destination)) => {
            exports.ask(destination);
            input.request_redraw();
            None
        }
        // Nothing was there when the user chose, so this publishes without
        // replacing: a file that arrives in the meantime is refused by the
        // publication rather than overwritten by it.
        Ok(ExportRequest::Start(destination)) => {
            let generation = exports.start(&destination, false, spawn);
            input.request_redraw();
            Some(generation)
        }
        Err(error) => {
            exports.refuse(String::new(), error.to_string());
            input.request_redraw();
            None
        }
    }
}

/// Acts on the answer to `Replace existing file?`.
///
/// The path is the one that was asked about and no other: it is taken from the
/// question rather than from anything the window has since been told, so an
/// export cannot end up replacing a file nobody was asked about. Refusing the
/// document as its own destination happens again here, because a confirmation
/// is not a way to authorise that.
pub(crate) fn confirm_export(
    exports: &mut Exports,
    input: &mut ViewportInput,
    source: Option<&Path>,
    choice: ReplaceChoice,
    spawn: impl FnOnce(&Path, bool, ExportGeneration, &CancelToken) -> JoinHandle<()>,
) -> Option<ExportGeneration> {
    match choice {
        // The usual answer on any given frame.
        ReplaceChoice::Waiting => None,
        // The file stays exactly as it is, and so does everything else.
        ReplaceChoice::Cancel => {
            if exports.dismiss() {
                input.request_redraw();
            }
            None
        }
        ReplaceChoice::Replace => {
            let destination = exports.pending.take()?;
            let source = source?;
            match is_same_entry(source, &destination) {
                Ok(false) => {
                    let generation = exports.start(&destination, true, spawn);
                    input.request_redraw();
                    Some(generation)
                }
                Ok(true) => {
                    exports.refuse(display(source), SOURCE_IS_DESTINATION.to_owned());
                    input.request_redraw();
                    None
                }
                Err(error) => {
                    exports.refuse(display(&destination), error.to_string());
                    input.request_redraw();
                    None
                }
            }
        }
    }
}

/// Stops the export in flight, whoever asked for it to stop.
///
/// Nothing on screen changes: the model is the model, and the file that was
/// being written never reaches its destination.
pub(crate) fn cancel_export(exports: &mut Exports, input: &mut ViewportInput) -> bool {
    let changed = exports.cancel_current();
    if changed {
        input.request_redraw();
    }
    changed
}

/// Finishes an answer at the application boundary.
///
/// [`Exports::answered`] is the one generation check, and its answer controls
/// the only thing visible outside that state machine: a redraw.
pub(crate) fn finish_export(
    exports: &mut Exports,
    input: &mut ViewportInput,
    generation: ExportGeneration,
    outcome: Result<FbxExport>,
) -> bool {
    let changed = exports.answered(generation, outcome);
    if changed {
        input.request_redraw();
    }
    changed
}

/// What the export section shows this frame, or nothing.
///
/// Three borrows that must outlive the frame, so they are made by the caller
/// and handed in, exactly as a failed Open's are.
pub(crate) fn shown<'a>(
    status: &'a ExportStatus,
    line: &'a str,
    omissions: &'a [OmittedDefinition<'a>],
) -> Option<ExportOutcome<'a>> {
    status.shown(line, omissions)
}

/// The line and the omissions this frame borrows from.
pub(crate) fn words(status: &ExportStatus) -> (String, Vec<OmittedDefinition<'_>>) {
    (status.line(), status.omissions())
}

/// A path as a person reads it: whole, because where a file went is the
/// question a window about exporting has to answer.
fn display(path: &Path) -> String {
    path.display().to_string()
}

/// Runs an export away from the event loop and delivers the answer back to it.
///
/// Both halves are arguments so that this can be shown to return while the
/// export is still running, which is the whole property: the window stays
/// alive while a document is written out.
pub(crate) fn spawn_export(
    export: impl FnOnce() -> Result<FbxExport> + Send + 'static,
    deliver: impl FnOnce(Result<FbxExport>) + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || deliver(export()))
}

/// The whole of the work, on the thread that owns the kernel session.
///
/// The one call to the shared job, and the only place this application does
/// any of it. The kernel is made and dropped inside this call: an Open CASCADE
/// session belongs to the thread that opened it, and ending it with the thread
/// means an abandoned export cannot outlive the shapes it was holding.
pub(crate) fn run_export(
    document: &Path,
    destination: &Path,
    replace: bool,
    context: &OperationContext,
) -> Result<FbxExport> {
    let mut kernel = OcctKernel::new()?;
    export_into(
        &mut kernel,
        // How this kernel re-reads a STEP file the document stores. The bytes
        // are in the document, so the file it was imported from need not
        // exist any more.
        |kernel, source| kernel.import_step(source),
        document,
        destination,
        replace,
        context,
    )
}

/// The one call to the shared job, whichever kernel is doing the work.
///
/// Split from [`run_export`] by exactly one thing: which session the shapes
/// belong to. That is what lets every gate below drive the real route —
/// document, scene, writer, publication — on a machine with no Open CASCADE,
/// while leaving one call to the job in the shipped path.
fn export_into<K>(
    kernel: &mut K,
    read_step: impl FnMut(&mut K, &[u8]) -> Result<Import>,
    document: &Path,
    destination: &Path,
    replace: bool,
    context: &OperationContext,
) -> Result<FbxExport>
where
    K: GeometryKernel + ?Sized,
{
    export_document_as_fbx(
        FbxExportRequest::new(document, destination, existing(replace)),
        kernel,
        read_step,
        &TessellationParams::default(),
        context,
    )
}

/// What a confirmed replacement means at the moment of publication.
///
/// Without one the publication is an atomic no-clobber: the user chose a name
/// nothing was at, and a file that appeared in between is somebody else's and
/// is not replaced.
fn existing(replace: bool) -> Existing<'static> {
    if replace {
        Existing::Replace
    } else {
        Existing::Keep { advice: APPEARED }
    }
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use ferritecad_export::{
        ExportGeometry, ExportOccurrence, ExportOmission, ExportProvenance, ExportScene,
        ExportSceneBuilder, ExportTransform,
    };
    use ferritecad_kernel::TessellationRefusal;
    use ferritecad_kernel::mock::MockKernel;
    use ferritecad_types::{CadError, ImportedSourceId};

    /// A worker that does nothing until it is let go, or cancelled.
    ///
    /// Real threads and a real token: what these gates are about is what
    /// happens while an export is still going on, and a stand-in that finished
    /// immediately would have no such moment.
    fn held_worker(cancel: &CancelToken) -> (JoinHandle<()>, mpsc::Sender<()>, Arc<AtomicBool>) {
        let (release, held) = mpsc::channel::<()>();
        let finished = Arc::new(AtomicBool::new(false));
        let ended = Arc::clone(&finished);
        let cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            // A deadline as well as a token. A gate that waited for ever for a
            // cancellation that a mutation removed would hang rather than
            // fail, and a gate that hangs has measured nothing.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !cancel.is_cancelled() && std::time::Instant::now() < deadline {
                match held.recv_timeout(Duration::from_millis(5)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            // A real export does not stop the instant it is asked to: it
            // notices at the next unit of work and then has a session to end.
            // Waiting here is what lets a gate tell a worker that was waited
            // for from one that was merely dropped.
            std::thread::sleep(Duration::from_millis(150));
            ended.store(true, Ordering::SeqCst);
        });
        (worker, release, finished)
    }

    /// What every export a gate starts was asked to do, and how to stop it.
    #[derive(Default)]
    struct Started {
        asked: Vec<(PathBuf, bool)>,
        cancels: Vec<CancelToken>,
        holds: Vec<mpsc::Sender<()>>,
        finished: Vec<Arc<AtomicBool>>,
    }

    impl Started {
        /// Records one start and hands back a worker that will not finish on
        /// its own.
        fn held(
            &mut self,
            destination: &Path,
            replace: bool,
            cancel: &CancelToken,
        ) -> JoinHandle<()> {
            self.asked.push((destination.to_path_buf(), replace));
            self.cancels.push(cancel.clone());
            let (worker, release, finished) = held_worker(cancel);
            self.holds.push(release);
            self.finished.push(finished);
            worker
        }
    }

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

    /// A private copy of the committed plate, which must never be opened in
    /// place.
    fn plate(directory: &Path) -> PathBuf {
        let target = directory.join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &target).expect("copies the fixture");
        target
    }

    /// The plate holds no imports, so nothing may ask this for STEP bytes.
    fn no_imports<K: ?Sized>(_: &mut K, _: &[u8]) -> Result<Import> {
        Err(CadError::unsupported("the plate holds no stored imports"))
    }

    /// The production route, with a kernel that needs no Open CASCADE.
    ///
    /// Everything a real export does happens here: the document is read from
    /// disk, rebuilt cold, turned into the neutral scene, handed to the writer
    /// once and published atomically. Only the session is different.
    fn export_now(document: &Path, destination: &Path, replace: bool) -> Result<FbxExport> {
        export_with(document, destination, replace, &OperationContext::default())
    }

    fn export_with(
        document: &Path,
        destination: &Path,
        replace: bool,
        context: &OperationContext,
    ) -> Result<FbxExport> {
        let mut kernel = MockKernel::new();
        export_into(
            &mut kernel,
            no_imports,
            document,
            destination,
            replace,
            context,
        )
    }

    /// Drives one export the way the event loop does, with the real work
    /// behind it and no window anywhere.
    fn run_to_completion(
        exports: &mut Exports,
        input: &mut ViewportInput,
        document: &Path,
        chosen: Option<PathBuf>,
    ) -> Option<ExportGeneration> {
        let (answers, answered) = mpsc::channel();
        let document = document.to_path_buf();
        let source = document.clone();
        let generation = begin_export(
            exports,
            input,
            Some(&source),
            chosen,
            |destination, replace, generation, cancel| {
                let destination = destination.to_path_buf();
                let context = OperationContext::default().with_cancel(cancel.clone());
                spawn_export(
                    move || export_with(&document, &destination, replace, &context),
                    move |result| {
                        let _ = answers.send((generation, result));
                    },
                )
            },
        )?;
        let (answered_generation, result) = answered.recv().expect("the export answered");
        finish_export(exports, input, answered_generation, result);
        Some(generation)
    }

    fn omission(key: &str) -> ExportGeometry {
        ExportGeometry::Omitted(ExportOmission::new(
            ferritecad_exchange::Diagnostic {
                stage: ferritecad_exchange::Stage::Validation,
                severity: ferritecad_exchange::Severity::Warning,
                entity: key.to_owned(),
                message: "the solid is not valid".to_owned(),
            },
            TessellationRefusal::IncompleteFace,
        ))
    }

    /// A scene of `keys` omitted definitions, each placed twice.
    fn partial_scene(keys: &[&str]) -> ExportScene {
        let mut builder = ExportSceneBuilder::new();
        let source = ImportedSourceId::new();
        for key in keys {
            let definition = builder
                .definition(
                    ExportSource::Imported {
                        source,
                        definition_key: (*key).to_owned(),
                    },
                    Some((*key).to_owned()),
                    ExportProvenance::default(),
                    omission(key),
                )
                .expect("a definition");
            for _ in 0..2 {
                builder
                    .node(
                        None,
                        definition,
                        ExportTransform::IDENTITY,
                        Some((*key).to_owned()),
                        None,
                        ExportOccurrence::Unrecorded,
                    )
                    .expect("a placement");
            }
        }
        builder.finish().expect("a scene")
    }

    /// What a window holds after a partial export of that scene.
    ///
    /// Through the production conversion and a real write, so what is gated
    /// below is what a window would really be showing. The bytes go into
    /// memory rather than onto a disk: where a file is published is the job's
    /// business and is gated where the job lives.
    fn partial_status(destination: &str, scene: &ExportScene) -> ExportStatus {
        let report = ferritecad_export::write_fbx_ascii_7400(scene, &mut Vec::new())
            .expect("a scene with omissions is still writable");
        assert!(!report.is_complete(), "the gate's scene exports completely");
        ExportStatus::of(destination.to_owned(), &report)
    }

    // ------------------------------------------------ what starts an export

    /// With nothing accepted there is nothing to export, and a direct call
    /// starts nothing rather than exporting whatever path is lying around.
    #[test]
    fn an_export_with_no_document_on_screen_starts_nothing() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let destination = directory.path().join("part.fbx");

        assert_eq!(
            requested(None, Some(destination.clone())).expect("decides"),
            ExportRequest::Nothing
        );

        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();
        let generation = begin_export(
            &mut exports,
            &mut input,
            None,
            Some(destination.clone()),
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        );

        assert!(generation.is_none(), "an export of no document started");
        assert!(started.asked.is_empty(), "a worker was started");
        assert_eq!(*exports.status(), ExportStatus::Idle);
        assert!(!destination.exists());
        assert!(
            !input.take_redraw(),
            "nothing happened and a frame was owed"
        );
    }

    /// A dialog the user closed is an answer, and it does exactly nothing.
    #[test]
    fn a_closed_dialog_is_an_exact_no_op() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());

        assert_eq!(
            requested(Some(&document), None).expect("decides"),
            ExportRequest::Nothing
        );

        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();
        let generation = begin_export(
            &mut exports,
            &mut input,
            Some(&document),
            None,
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        );

        assert!(generation.is_none(), "a closed dialog started an export");
        assert!(started.asked.is_empty(), "a closed dialog started a worker");
        assert_eq!(*exports.status(), ExportStatus::Idle);
        assert!(exports.pending().is_none());
        assert!(
            !input.take_redraw(),
            "a closed dialog asked for a frame it has nothing to draw"
        );
        // And nothing appeared beside the document.
        let entries: Vec<_> = std::fs::read_dir(directory.path())
            .expect("lists")
            .map(|entry| entry.expect("an entry").path())
            .collect();
        assert_eq!(entries, vec![document]);
    }

    /// The document is not a destination anything can authorise.
    #[test]
    fn the_document_itself_is_refused_as_its_own_output() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());

        assert_eq!(
            requested(Some(&document), Some(document.clone())).expect("decides"),
            ExportRequest::RefusedSource
        );

        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();
        begin_export(
            &mut exports,
            &mut input,
            Some(&document),
            Some(document.clone()),
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        );

        assert!(started.asked.is_empty(), "the document was exported over");
        assert!(
            exports.status().line().contains(SOURCE_IS_DESTINATION),
            "{}",
            exports.status().line()
        );
        // And the document is still a document.
        assert_eq!(
            std::fs::read(&document).expect("reads it"),
            std::fs::read(ferritecad_fixtures::plate_source()).expect("reads the fixture")
        );
    }

    /// And a confirmation is not a way round it either.
    #[test]
    fn confirming_does_not_make_the_document_its_own_output() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let before = std::fs::read(&document).expect("reads it");

        let mut exports = Exports::default();
        let mut input = input();
        // A question asked about the document itself, however it got there.
        exports.ask(document.clone());

        let mut started = Started::default();
        let generation = confirm_export(
            &mut exports,
            &mut input,
            Some(&document),
            ReplaceChoice::Replace,
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        );

        assert!(generation.is_none(), "a confirmation exported the document");
        assert!(started.asked.is_empty());
        assert!(exports.status().line().contains(SOURCE_IS_DESTINATION));
        assert_eq!(std::fs::read(&document).expect("reads it"), before);
    }

    // ------------------------------------------------- replacing a file

    /// A destination something is already at is a question, not a start.
    #[test]
    fn an_existing_destination_is_asked_about_before_anything_is_written() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let destination = directory.path().join("part.fbx");
        std::fs::write(&destination, b"somebody else's file").expect("writes");

        assert_eq!(
            requested(Some(&document), Some(destination.clone())).expect("decides"),
            ExportRequest::Confirm(destination.clone())
        );

        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();
        let generation = begin_export(
            &mut exports,
            &mut input,
            Some(&document),
            Some(destination.clone()),
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        );

        assert!(generation.is_none(), "an unconfirmed export started");
        assert!(started.asked.is_empty(), "a worker ran before the question");
        assert_eq!(exports.pending(), Some(destination.as_path()));
        assert_eq!(*exports.status(), ExportStatus::Idle);
        assert_eq!(
            std::fs::read(&destination).expect("reads it"),
            b"somebody else's file",
            "the file was replaced before anybody agreed to it"
        );
    }

    /// Saying no leaves the file, and the window, exactly as they were.
    #[test]
    fn cancelling_the_confirmation_keeps_the_destination() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let destination = directory.path().join("part.fbx");
        std::fs::write(&destination, b"somebody else's file").expect("writes");

        let mut exports = Exports::default();
        let mut input = input();
        exports.ask(destination.clone());

        let mut started = Started::default();
        let generation = confirm_export(
            &mut exports,
            &mut input,
            Some(&document),
            ReplaceChoice::Cancel,
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        );

        assert!(generation.is_none());
        assert!(started.asked.is_empty(), "saying no started an export");
        assert!(exports.pending().is_none(), "the question is still up");
        assert_eq!(*exports.status(), ExportStatus::Idle);
        assert_eq!(
            std::fs::read(&destination).expect("reads it"),
            b"somebody else's file"
        );
    }

    /// And an unanswered question writes nothing however long it is left.
    #[test]
    fn a_question_nobody_answers_writes_nothing() {
        let mut exports = Exports::default();
        let mut input = input();
        exports.ask(PathBuf::from("part.fbx"));

        let mut started = Started::default();
        for _ in 0..3 {
            let generation = confirm_export(
                &mut exports,
                &mut input,
                Some(Path::new("document.fcad")),
                ReplaceChoice::Waiting,
                |destination, replace, _, cancel| started.held(destination, replace, cancel),
            );
            assert!(generation.is_none());
        }
        assert!(started.asked.is_empty());
        assert_eq!(exports.pending(), Some(Path::new("part.fbx")));
    }

    /// Saying yes replaces that file, and does it whole.
    #[test]
    fn confirming_replaces_the_whole_file() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let destination = directory.path().join("part.fbx");
        // Longer than the export will be, so a partial overwrite would leave a
        // tail behind and be visible as one.
        std::fs::write(&destination, vec![b'x'; 1 << 20]).expect("writes");

        let mut exports = Exports::default();
        let mut input = input();
        exports.ask(destination.clone());

        let (answers, answered) = mpsc::channel();
        let reading = document.clone();
        let generation = confirm_export(
            &mut exports,
            &mut input,
            Some(&document),
            ReplaceChoice::Replace,
            |destination, replace, generation, cancel| {
                assert!(replace, "a confirmed replacement was published no-clobber");
                let destination = destination.to_path_buf();
                let context = OperationContext::default().with_cancel(cancel.clone());
                spawn_export(
                    move || export_with(&reading, &destination, replace, &context),
                    move |result| {
                        let _ = answers.send((generation, result));
                    },
                )
            },
        )
        .expect("a confirmed export starts");

        let (answered_generation, result) = answered.recv().expect("the export answered");
        assert_eq!(answered_generation, generation);
        finish_export(&mut exports, &mut input, generation, result);

        let published = std::fs::read(&destination).expect("reads the replacement");
        assert!(published.starts_with(b"; FBX 7.4.0 project file"));
        assert!(published.len() < (1 << 20), "the old file's tail survived");
        assert!(matches!(exports.status(), ExportStatus::Wrote { .. }));
    }

    /// A confirmation is about the file it asked about, and no other.
    #[test]
    fn a_confirmation_applies_to_the_file_it_named() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let first = directory.path().join("first.fbx");
        let second = directory.path().join("second.fbx");
        std::fs::write(&first, b"the first file").expect("writes");
        std::fs::write(&second, b"the second file").expect("writes");

        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();

        // Two dialogs, one after the other. The second question is the one on
        // screen, so it is the one the button answers.
        for destination in [&first, &second] {
            begin_export(
                &mut exports,
                &mut input,
                Some(&document),
                Some(destination.clone()),
                |destination, replace, _, cancel| started.held(destination, replace, cancel),
            );
        }
        assert_eq!(exports.pending(), Some(second.as_path()));

        confirm_export(
            &mut exports,
            &mut input,
            Some(&document),
            ReplaceChoice::Replace,
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        )
        .expect("a confirmed export starts");

        assert_eq!(
            started.asked,
            vec![(second.clone(), true)],
            "the confirmation replaced a file it never asked about"
        );
        assert_eq!(
            std::fs::read(&first).expect("reads it"),
            b"the first file",
            "the file the user was not asked about was replaced"
        );
        exports.stop_all();
    }

    /// And a question left over from the document being replaced goes with it.
    #[test]
    fn a_question_about_the_last_document_does_not_survive_a_new_open() {
        let mut exports = Exports::default();
        let mut input = input();
        exports.ask(PathBuf::from("was-about-the-old-document.fbx"));

        // What the window does when an Open begins.
        cancel_export(&mut exports, &mut input);
        assert!(exports.pending().is_none(), "a stale question is still up");

        let mut started = Started::default();
        let generation = confirm_export(
            &mut exports,
            &mut input,
            Some(Path::new("the-new-document.fcad")),
            ReplaceChoice::Replace,
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        );
        assert!(
            generation.is_none(),
            "a stale question exported the new document over the old destination"
        );
        assert!(started.asked.is_empty());
    }

    // ------------------------------------------------------ the worker

    /// Starting an export returns while the export is still running.
    #[test]
    fn the_export_does_not_happen_in_the_event_loop() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let destination = directory.path().join("part.fbx");

        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();
        let generation = begin_export(
            &mut exports,
            &mut input,
            Some(&document),
            Some(destination.clone()),
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        )
        .expect("a destination was chosen, so an export started");

        // This line is reached with that worker still alive and the file not
        // yet written, which is the whole property.
        assert!(!started.cancels[0].is_cancelled());
        assert!(
            !destination.exists(),
            "the file was written before returning"
        );
        assert!(exports.accepts(generation));
        assert!(exports.running());
        assert!(
            exports.status().line().starts_with(EXPORTING),
            "{}",
            exports.status().line()
        );

        exports.stop_all();
        assert!(started.cancels[0].is_cancelled());
    }

    /// The work happens on the worker, and the call to start it returns.
    ///
    /// Not a claim about the export in particular: what is gated is that this
    /// is how work is started at all, because a window whose export ran in the
    /// call that started it would freeze for exactly as long as the export
    /// took, however carefully everything else was arranged.
    #[test]
    fn the_work_happens_on_the_worker_and_not_in_the_call() {
        let (running, started) = mpsc::channel();
        let (release, held) = mpsc::channel::<()>();
        let (answers, answered) = mpsc::channel();

        let worker = spawn_export(
            move || {
                let _ = running.send(());
                // Waits until this gate lets it go, which it does only after
                // the call that started it has returned — and gives up on its
                // own if that never happens. A gate that waited for ever would
                // hang rather than fail when the work really did run in the
                // call, and a gate that hangs measures nothing.
                let _ = held.recv_timeout(Duration::from_secs(2));
                Err(ferritecad_types::CadError::Cancelled)
            },
            move |result| {
                let _ = answers.send(result);
            },
        );

        started
            .recv_timeout(Duration::from_secs(5))
            .expect("the work never started");
        assert!(
            answered.try_recv().is_err(),
            "the export finished inside the call that started it"
        );

        let _ = release.send(());
        let result = answered
            .recv_timeout(Duration::from_secs(5))
            .expect("the worker never answered");
        assert_eq!(
            result.expect_err("this one was told to give up").kind(),
            ErrorKind::Cancellation
        );
        worker.join().expect("the worker ends");
    }

    /// The window says what a window can act on, and never what a flag would.
    ///
    /// The publication is shared with the command line, and the sentence a
    /// person reads when something is already at the destination is not: there
    /// is no flag to pass here, and printing one would be telling somebody
    /// looking at a window to type something.
    #[test]
    fn the_window_speaks_for_itself() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let destination = directory.path().join("part.fbx");
        // Chosen when nothing was there, and something is there by the time
        // the writer finishes. This is the race, and the window has to explain
        // it in its own words.
        std::fs::write(&destination, b"arrived while the export ran").expect("writes");

        let error = export_now(&document, &destination, false)
            .expect_err("a file that appeared must not be replaced");
        let said = error.to_string();
        assert!(said.contains(APPEARED), "{said}");
        assert!(
            !said.contains("--force") && !said.contains("pass "),
            "the window printed a command line's vocabulary: {said}"
        );
        assert_eq!(
            std::fs::read(&destination).expect("the other file remains"),
            b"arrived while the export ran"
        );
    }

    /// The whole route, windowless: a real document in, a real FBX out.
    #[test]
    fn an_export_writes_the_file_the_user_chose() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let destination = directory.path().join("plate.fbx");

        let mut exports = Exports::default();
        let mut input = input();
        run_to_completion(
            &mut exports,
            &mut input,
            &document,
            Some(destination.clone()),
        )
        .expect("an export started");

        let bytes = std::fs::read(&destination).expect("the export published a file");
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).into_owned();
        assert!(head.contains("FBXVersion: 7400"), "{head}");

        match exports.status() {
            ExportStatus::Wrote {
                destination: shown,
                file,
                omissions,
            } => {
                assert_eq!(shown, &destination.display().to_string());
                assert_eq!(file.bytes, bytes.len() as u64);
                assert!(file.models > 0 && file.geometries > 0);
                assert!(omissions.is_empty(), "the plate exported incompletely");
            }
            other => panic!("an export that wrote a file reported {other:?}"),
        }
        // A complete export says so without a word of the vocabulary a person
        // would search a log for.
        let line = exports.status().line();
        assert!(line.starts_with(EXPORTED), "{line}");
        assert!(!line.contains(EXPORTED_WITH_OMISSIONS), "{line}");
        assert!(!line.contains("missing"), "{line}");
        assert!(!line.contains("failed"), "{line}");
        // And no scratch space was left beside it.
        assert!(
            !std::fs::read_dir(directory.path())
                .expect("lists")
                .filter_map(std::result::Result::ok)
                .any(|entry| entry.path().to_string_lossy().contains(".partial"))
        );
    }

    /// Two exports of one document produce one file: the same one.
    #[test]
    fn the_window_and_the_shared_job_write_the_same_bytes() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let through_window = directory.path().join("window.fbx");
        let through_job = directory.path().join("job.fbx");

        let mut exports = Exports::default();
        let mut input = input();
        run_to_completion(
            &mut exports,
            &mut input,
            &document,
            Some(through_window.clone()),
        )
        .expect("an export started");
        export_now(&document, &through_job, false).expect("the job writes");

        assert_eq!(
            std::fs::read(&through_window).expect("reads one"),
            std::fs::read(&through_job).expect("reads the other"),
            "the window wrote something other than what the job writes"
        );
    }

    /// A new request abandons the one before it, and the abandoned answer
    /// changes nothing when it arrives.
    #[test]
    fn a_new_export_cancels_the_old_one_whose_answer_then_counts_for_nothing() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let first = directory.path().join("first.fbx");
        let second = directory.path().join("second.fbx");

        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();

        let older = begin_export(
            &mut exports,
            &mut input,
            Some(&document),
            Some(first.clone()),
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        )
        .expect("an export started");
        let newer = begin_export(
            &mut exports,
            &mut input,
            Some(&document),
            Some(second.clone()),
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        )
        .expect("a second export started");

        assert!(older < newer, "generations are not monotonic");
        assert!(
            started.cancels[0].is_cancelled(),
            "the abandoned export was left running"
        );
        assert_eq!(
            exports.running.len(),
            2,
            "an abandoned worker was dropped, which detaches its thread"
        );
        assert!(!exports.accepts(older));

        // The abandoned export answers last, as the slow one usually does.
        let _ = input.take_redraw();
        let stale = export_now(&document, &first, false);
        let changed = finish_export(&mut exports, &mut input, older, stale);
        assert!(!changed, "a stale answer changed the line");
        assert!(!input.take_redraw(), "a stale answer asked for a frame");
        match exports.status() {
            ExportStatus::Running { destination, .. } => {
                assert_eq!(destination, &second.display().to_string());
            }
            other => panic!("a stale answer replaced the current status with {other:?}"),
        }

        exports.stop_all();
    }

    /// A request that is refused still replaces the export the user was
    /// asking for before it. Otherwise the line and Cancel button describe a
    /// finished failure while the old worker remains able to publish a file
    /// nobody can see how to stop.
    #[test]
    fn a_refused_new_export_stops_the_old_one_it_replaced() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let first = directory.path().join("first.fbx");

        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();

        let older = begin_export(
            &mut exports,
            &mut input,
            Some(&document),
            Some(first),
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        )
        .expect("an export started");

        let refused = begin_export(
            &mut exports,
            &mut input,
            Some(&document),
            Some(document.clone()),
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        );

        assert!(refused.is_none(), "the document was exported over itself");
        assert!(
            started.cancels[0].is_cancelled(),
            "the export hidden by the refusal was left able to publish"
        );
        assert!(
            !exports.accepts(older),
            "the hidden export still had an answer the window would accept"
        );
        assert!(
            !exports.running(),
            "Cancel was offered for a refused request"
        );
        assert!(exports.status().line().contains(SOURCE_IS_DESTINATION));
        assert_eq!(
            exports.running.len(),
            1,
            "the superseded worker was dropped instead of being joined later"
        );

        let refusal = exports.status().line();
        let _ = input.take_redraw();
        assert!(
            !finish_export(&mut exports, &mut input, older, Err(CadError::Cancelled)),
            "the answer from the hidden export replaced the refusal"
        );
        assert_eq!(exports.status().line(), refusal);
        assert!(!input.take_redraw(), "the stale answer asked for a frame");

        exports.stop_all();
    }

    /// Giving up says so, publishes nothing, and is not called a failure.
    #[test]
    fn an_export_given_up_on_publishes_nothing_and_is_not_a_failure() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());
        let destination = directory.path().join("part.fbx");

        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();
        let generation = begin_export(
            &mut exports,
            &mut input,
            Some(&document),
            Some(destination.clone()),
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        )
        .expect("an export started");

        assert!(cancel_export(&mut exports, &mut input));
        assert!(started.cancels[0].is_cancelled());
        assert!(
            exports.status().line().starts_with(CANCELLING),
            "{}",
            exports.status().line()
        );
        // Still the current request: it is its own answer that ends it.
        assert!(exports.accepts(generation));

        finish_export(
            &mut exports,
            &mut input,
            generation,
            Err(ferritecad_types::CadError::Cancelled),
        );
        assert_eq!(
            *exports.status(),
            ExportStatus::Cancelled {
                destination: destination.display().to_string()
            }
        );
        assert!(exports.status().line().starts_with(EXPORT_CANCELLED));
        assert!(!exports.status().line().contains(EXPORT_FAILED));
        assert!(!destination.exists(), "a cancelled export published a file");
    }

    /// Shutting down stops every export and waits for all of them.
    #[test]
    fn shutting_down_cancels_and_joins_every_export() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let document = plate(directory.path());

        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();
        for name in ["a.fbx", "b.fbx", "c.fbx"] {
            begin_export(
                &mut exports,
                &mut input,
                Some(&document),
                Some(directory.path().join(name)),
                |destination, replace, _, cancel| started.held(destination, replace, cancel),
            );
        }
        assert_eq!(exports.running.len(), 3);

        exports.stop_all();

        assert!(
            started.cancels.iter().all(CancelToken::is_cancelled),
            "an export was left running after the window closed"
        );
        assert!(
            exports.running.is_empty(),
            "a worker was left unjoined, which detaches a kernel session"
        );
        // Every one of them had really ended by the time this returned. A
        // shutdown that dropped the handles instead of joining them would come
        // back while the sessions were still open.
        assert!(
            started
                .finished
                .iter()
                .all(|ended| ended.load(Ordering::SeqCst)),
            "shutting down returned while an export worker was still running"
        );
        assert!(exports.pending().is_none());
    }

    // ------------------------------------------------- what is reported

    /// A failure is a failure, names the destination, and publishes nothing.
    #[test]
    fn a_failed_export_is_reported_as_one() {
        let mut exports = Exports::default();
        let mut input = input();
        let mut started = Started::default();
        let generation = begin_export(
            &mut exports,
            &mut input,
            Some(Path::new("document.fcad")),
            Some(PathBuf::from("part.fbx")),
            |destination, replace, _, cancel| started.held(destination, replace, cancel),
        )
        .expect("an export started");

        finish_export(
            &mut exports,
            &mut input,
            generation,
            Err(ferritecad_types::CadError::io(
                "creating part.fbx",
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            )),
        );

        let line = exports.status().line();
        assert!(line.starts_with(EXPORT_FAILED), "{line}");
        assert!(line.contains("part.fbx"), "{line}");
        assert!(!line.contains(EXPORT_CANCELLED), "{line}");
        assert!(!line.contains(EXPORTED_WITH_OMISSIONS), "{line}");
        exports.stop_all();
    }

    /// A partial export says it is partial, and lists every omission whole.
    #[test]
    fn a_partial_export_says_so_and_reports_every_omission() {
        let keys = ["#11", "#22", "#33"];
        let status = partial_status("/models/part.fbx", &partial_scene(&keys));

        let line = status.line();
        assert!(line.starts_with(EXPORTED_WITH_OMISSIONS), "{line}");
        assert!(line.contains("/models/part.fbx"), "{line}");

        let omissions = status.omissions();
        assert_eq!(
            omissions.len(),
            keys.len(),
            "the report stopped short of the omissions there were"
        );

        let rows: Vec<(&'static str, String)> = omissions
            .iter()
            .flat_map(ferritecad_ui::OmittedDefinition::rows)
            .collect();
        let said = |value: &str| rows.iter().any(|(_, written)| written.contains(value));

        for key in keys {
            assert!(said(key), "the omission of {key} is not reported: {rows:?}");
        }
        // Every placement of every one of them, under the key the file uses.
        for node in 0..keys.len() * 2 {
            assert!(
                said(&format!("node/{node}")),
                "placement node/{node} was dropped: {rows:?}"
            );
        }
        // The source qualifies the key, the persisted finding survives whole,
        // and the refusal is the typed one by its stable name.
        assert!(said("imported source"), "{rows:?}");
        assert!(said("the solid is not valid"), "{rows:?}");
        assert!(said("warning") && said("validating"), "{rows:?}");
        assert!(said("IncompleteFace"), "{rows:?}");
        // And nothing here is a rendering of a debugging aid.
        assert!(
            !said("Diagnostic {") && !said("IncompleteFace,") && !said("ExportSource::"),
            "a Debug rendering became data: {rows:?}"
        );
        assert!(
            !said("one or more faces have no usable triangles"),
            "the refusal's message became data: {rows:?}"
        );
    }

    /// Two files may both call a definition `#31`, and they are not one thing.
    #[test]
    fn two_sources_with_one_local_key_stay_two_omissions() {
        let mut builder = ExportSceneBuilder::new();
        let first = ImportedSourceId::new();
        let second = ImportedSourceId::new();
        for source in [first, second] {
            let definition = builder
                .definition(
                    ExportSource::Imported {
                        source,
                        definition_key: "step.product_definition#31".to_owned(),
                    },
                    Some("part".to_owned()),
                    ExportProvenance::default(),
                    omission("step.product_definition#31"),
                )
                .expect("a definition");
            builder
                .node(
                    None,
                    definition,
                    ExportTransform::IDENTITY,
                    Some("part".to_owned()),
                    None,
                    ExportOccurrence::Unrecorded,
                )
                .expect("a placement");
        }
        let scene = builder.finish().expect("a scene");
        let status = partial_status("part.fbx", &scene);
        let omissions = status.omissions();

        assert_eq!(omissions.len(), 2);
        assert!(omissions[0].definition.contains(&first.to_string()));
        assert!(omissions[1].definition.contains(&second.to_string()));
        assert_ne!(omissions[0].definition, omissions[1].definition);
    }

    /// A whole export is never dressed up as a partial one.
    #[test]
    fn a_complete_export_carries_no_omissions_and_no_warning_words() {
        let status = ExportStatus::Wrote {
            destination: "part.fbx".to_owned(),
            file: WroteFile {
                bytes: 12,
                models: 1,
                geometries: 1,
                materials: 1,
            },
            omissions: Vec::new(),
        };
        assert_eq!(status.line(), format!("{EXPORTED} part.fbx"));
        assert!(status.omissions().is_empty());

        let line = status.line();
        let omissions = status.omissions();
        let shown = shown(&status, &line, &omissions).expect("a finished export is shown");
        assert!(shown.omissions.is_empty());
        assert_eq!(shown.file.expect("a published file is described").bytes, 12);
    }

    /// Nothing at all is shown before the first export.
    #[test]
    fn a_window_that_has_exported_nothing_says_nothing_about_exporting() {
        let status = ExportStatus::Idle;
        let line = status.line();
        let omissions = status.omissions();
        assert!(shown(&status, &line, &omissions).is_none());
    }

    // ------------------------------------------ the real assembly, end to end

    /// The complex AP203 assembly, from a STEP file to a published FBX.
    ///
    /// The whole route the window takes, with the session it really opens:
    /// the assembly is imported, the external STEP is deleted, and what is
    /// exported comes from the bytes the document stores and from nothing
    /// else. What is checked is what a person would see — a published file,
    /// and a window saying which definitions are not in it.
    ///
    /// Needs Open CASCADE. The command line has its own gate on the same
    /// assembly; this one is about the window, and the two do not share a
    /// process, a status or a line of display code.
    #[test]
    fn the_complex_assembly_exports_from_the_window_and_says_what_is_missing() {
        if !ferritecad_occt::is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return;
        }
        let directory = tempfile::tempdir().expect("a temporary directory");
        let step = directory.path().join("assembly.stp");
        std::fs::copy(complex_assembly(), &step).expect("copies the fixture");
        let document = directory.path().join("assembly.fcad");
        import_into(&step, &document);
        // Gone before anything is exported: a document carries its own source
        // bytes, so the file it was imported from need not exist.
        std::fs::remove_file(&step).expect("removes the external STEP");

        let destination = directory.path().join("assembly.fbx");
        let mut exports = Exports::default();
        let mut input = input();
        let (answers, answered) = mpsc::channel();
        let reading = document.clone();
        let generation = begin_export(
            &mut exports,
            &mut input,
            Some(&document),
            Some(destination.clone()),
            |destination, replace, generation, cancel| {
                let destination = destination.to_path_buf();
                let context = OperationContext::default().with_cancel(cancel.clone());
                spawn_export(
                    // The production worker, session and all.
                    move || run_export(&reading, &destination, replace, &context),
                    move |result| {
                        let _ = answers.send((generation, result));
                    },
                )
            },
        )
        .expect("an export started");
        let (answered_generation, result) = answered.recv().expect("the export answered");
        assert_eq!(answered_generation, generation);
        finish_export(&mut exports, &mut input, generation, result);

        let written = std::fs::metadata(&destination)
            .expect("the window published a file")
            .len();
        assert!(
            written > 100_000_000,
            "the published file is {written} bytes"
        );

        let (line, omissions) = words(exports.status());
        assert!(line.starts_with(EXPORTED_WITH_OMISSIONS), "{line}");
        assert!(line.contains("assembly.fbx"), "{line}");
        assert!(
            !omissions.is_empty(),
            "a partial export reported nothing missing"
        );

        let rows: Vec<(&'static str, String)> = omissions
            .iter()
            .flat_map(ferritecad_ui::OmittedDefinition::rows)
            .collect();
        let said = |value: &str| rows.iter().any(|(_, written)| written.contains(value));
        // The definition this build cannot give triangles to, by the identity
        // the source gave it and qualified by the source it came from.
        assert!(said(OMITTED_KEY), "{rows:?}");
        assert!(said("imported source"), "{rows:?}");
        // The typed refusal by its stable name, and every placement of it.
        assert!(said("IncompleteFace"), "{rows:?}");
        assert!(
            rows.iter()
                .any(|(label, value)| *label == "Placement" && value.starts_with("node/")),
            "{rows:?}"
        );
        // And what the window shows about the file itself is the writer's own
        // count of it rather than anything read back from disk.
        match exports.status() {
            ExportStatus::Wrote { file, .. } => {
                assert_eq!(file.bytes, written);
                assert!(file.models > file.geometries, "{file:?}");
            }
            other => panic!("a published export reported {other:?}"),
        }
    }

    /// The committed AP203 assembly, which is somebody else's file.
    fn complex_assembly() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/step/interoperability/c3d-ap203-complex-assembly.stp")
    }

    /// The definition this build's kernel cannot turn into triangles.
    const OMITTED_KEY: &str = "step.product_definition#2583";

    /// Puts a STEP file into a document, source bytes and all.
    ///
    /// Test setup rather than a route: what is being gated is the export, and
    /// an export needs a document that was imported. Every shape the session
    /// built is given back before it ends.
    fn import_into(step: &Path, document: &Path) {
        use ferritecad_kernel::GeometryKernel as _;

        let source = std::fs::read(step).expect("reads the fixture");
        let mut kernel = OcctKernel::new().expect("opens a session");
        let import = kernel.import_step(&source).expect("reads the assembly");
        {
            let mut created =
                ferritecad_document::Document::create(document).expect("creates the document");
            created
                .store_step_import(ferritecad_document::StepImportRequest {
                    object: ferritecad_types::ObjectId::new(),
                    name: Some("assembly"),
                    source: &source,
                    source_name: Some("assembly.stp"),
                    import: &import,
                    importer: kernel.identity(),
                })
                .expect("stores the import");
            created.close().expect("closes the document");
        }
        if let Some(scene) = import.scene() {
            for shape in scene.shapes() {
                kernel.release(shape);
            }
        }
    }

    /// A window is handed borrowed text, and the panel draws it.
    #[test]
    fn the_panel_draws_what_a_finished_export_says() {
        let keys = ["#11", "#22"];
        let status = partial_status("part.fbx", &partial_scene(&keys));
        let line = status.line();
        let omissions = status.omissions();
        let outcome = shown(&status, &line, &omissions).expect("a finished export is shown");

        let context = egui::Context::default();
        // A frame has to have run for the fonts to be loaded before anything
        // can be laid out.
        let mut warm = context.run_ui(egui::RawInput::default(), |_| {});
        warm.textures_delta.clear();

        let mut drawn = context.run_ui(egui::RawInput::default(), |ui| {
            ferritecad_ui::export_panel(ui, Some(outcome));
        });
        drawn.textures_delta.clear();
        let mut empty = context.run_ui(egui::RawInput::default(), |ui| {
            ferritecad_ui::export_panel(ui, None);
        });
        empty.textures_delta.clear();

        let vertices = |output: egui::FullOutput| {
            context
                .tessellate(output.shapes, 1.0)
                .iter()
                .map(|clipped| match &clipped.primitive {
                    egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                    egui::epaint::Primitive::Callback(_) => 0,
                })
                .sum::<usize>()
        };
        let with_report = vertices(drawn);
        assert_eq!(
            vertices(empty),
            0,
            "a window that has exported nothing drew a section about exporting"
        );
        assert!(with_report > 0, "the export panel drew nothing at all");
    }
}
