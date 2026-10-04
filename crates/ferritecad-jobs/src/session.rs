// SPDX-License-Identifier: MIT
//! The open document: one owner of what is accepted, what is saved and what can
//! be undone.
//!
//! A [`DocumentSession`] is the library object a window is a client of (plan
//! §4.5, ADR 0005). It owns, for the document that is open, the logical path
//! (where Save writes and what the window calls the document), the saved
//! checkpoint (what is on disk as of the last Open or Save), the accepted
//! history (every version the user accepted, one of them current) and the
//! private directory those versions live in. Nothing here needs a window, a
//! renderer or a kernel: the kernel is handed in by whoever runs an edit.
//!
//! # Versions are private files
//!
//! Every reused job takes a path and opens SQLite itself, so a version is a file:
//! one immutable `.fcad` in a private directory under the system temporary
//! directory. The user's own file is never written by Apply, Undo or Redo; it is
//! written by [`SavePlan::run`] and by nothing else. The private path is an
//! implementation detail and is never part of anything a person reads.
//!
//! # Two phases, because showing can fail
//!
//! Producing a step is work (a cold rebuild); showing it can still fail (a device
//! may refuse the upload). So an edit first *produces* a [`ProducedStep`] from a
//! [`StepTicket`], and only [`DocumentSession::commit_step`], called when the
//! prepared scene is ready to replace the shown one, makes it current. A step that
//! is dropped, or whose ticket has gone stale, leaves the session exactly as it
//! was and removes its file.
//!
//! # Dirty is a comparison
//!
//! The session is dirty when the *model content* of the current version differs
//! from that of the saved checkpoint ([`Document::model_version`]: the complete
//! logical content with only the modified stamp set aside). Undoing back to the
//! checkpoint, redoing away from it, editing a value and editing it back, and a
//! no-op edit therefore need no counter and cannot leave a false mark.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferritecad_document::{Document, DocumentVersion};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use ferritecad_types::{CadError, ContentHash, ObjectId, Result};

use crate::edit::{
    EditAnnulusRequest, EditCircleRequest, EditCircularCutRequest, EditExtrudeRequest,
    EditRevolveAngleRequest, EditSketchConstraintsRequest, EditSketchRequest, edit_annulus_copy,
    edit_circle_copy, edit_circular_cut_copy, edit_extrude_copy, edit_revolve_angle_copy,
    edit_sketch_constraints_copy, edit_sketch_copy,
};
use crate::save::{SavePlan, SaveTarget, Saved};

/// How much accepted history a session keeps.
///
/// Both bounds apply, and the oldest versions go first. The current version is
/// never dropped, so a single version larger than the byte bound is kept alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryLimits {
    /// The most versions kept, the current one included.
    pub max_versions: usize,
    /// The most bytes kept in private files, the current version included.
    pub max_bytes: u64,
}

impl Default for HistoryLimits {
    fn default() -> Self {
        Self {
            max_versions: 64,
            max_bytes: 512 * 1024 * 1024,
        }
    }
}

/// The private directory a session's versions live in.
///
/// Created by the session (mode `0700` on Unix), shared by every version through
/// an `Arc`, and removed when the last of them has gone: a worker still holding a
/// version after its session was dropped keeps the directory alive until it ends.
#[derive(Debug)]
struct SessionDir {
    path: PathBuf,
}

impl SessionDir {
    fn create(root: &Path) -> Result<Arc<Self>> {
        let path = root.join(format!("ferritecad-session-{}", ObjectId::new()));
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt as _;
            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700);
            builder
        };
        #[cfg(not(unix))]
        let builder = std::fs::DirBuilder::new();
        builder
            .create(&path)
            .map_err(|e| CadError::io("reserving the document session's private directory", e))?;
        Ok(Arc::new(Self { path }))
    }
}

impl Drop for SessionDir {
    fn drop(&mut self) {
        // Every file in here was made by this session or by a worker working for
        // it, and the directory is private to it, so removing the whole thing is
        // not deleting anything that is not ours. A failure has nowhere useful to
        // go.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// One accepted version of the document: an immutable private file and the two
/// facts about it a session needs.
#[derive(Debug)]
pub struct Snapshot {
    path: PathBuf,
    /// Document id and complete content version, exactly as a copy job pins them.
    disk: DocumentVersion,
    /// The model content: what dirty is compared on.
    model: ContentHash,
    bytes: u64,
    _directory: Arc<SessionDir>,
}

impl Snapshot {
    /// Reads the facts of a finished private file.
    fn adopt(path: PathBuf, directory: Arc<SessionDir>) -> Result<Arc<Self>> {
        let document = Document::open_read_only(&path)?;
        let disk = DocumentVersion {
            document_id: document.meta().document_id,
            content: document.content_version()?,
        };
        let model = document.model_version()?;
        document.close()?;
        let bytes = std::fs::metadata(&path)
            .map_err(|e| CadError::io(format!("measuring {}", path.display()), e))?
            .len();
        Ok(Arc::new(Self {
            path,
            disk,
            model,
            bytes,
            _directory: directory,
        }))
    }

    /// The private file. For jobs that read a path (exports, scene loads, edits);
    /// never for anything a person reads.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The version a copy job is given as the one it expects to read.
    pub fn version(&self) -> DocumentVersion {
        self.disk
    }

    pub fn model(&self) -> ContentHash {
        self.model
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        for suffix in ["-journal", "-wal", "-shm"] {
            let mut sidecar = self.path.as_os_str().to_owned();
            sidecar.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(sidecar));
        }
    }
}

/// What the last successful Open or Save put on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    pub(crate) disk: DocumentVersion,
    pub(crate) model: ContentHash,
}

/// Why a step could not be made current.
///
/// Both are the same fact — the document moved on while the step was being made
/// — and both leave the session untouched.
pub const STALE_STEP: &str =
    "the document changed while this edit was running; the edit was not applied";

/// The open document. See the module documentation.
#[derive(Debug)]
pub struct DocumentSession {
    directory: Arc<SessionDir>,
    logical: PathBuf,
    saved: Checkpoint,
    history: Vec<Arc<Snapshot>>,
    current: usize,
    next_file: u64,
    generation: u64,
    limits: HistoryLimits,
}

/// What [`DocumentSession::commit_step`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepCommit {
    /// The step is the current version, and one Undo goes back to what was.
    Accepted,
    /// The step's model is the model that was already current (only the stamp
    /// differed): nothing was added to the history.
    NoChange,
}

impl DocumentSession {
    /// Opens `source` read-only into a new session in the system temporary
    /// directory. The file is read once; the model the window shows is read from
    /// the session's own copy of it, so what is shown and what is saved are one
    /// reading.
    pub fn open(source: &Path) -> Result<Self> {
        Self::open_in(&std::env::temp_dir(), source, HistoryLimits::default())
    }

    /// As [`Self::open`], with the private directory's parent and the history
    /// limits chosen by the caller.
    pub fn open_in(root: &Path, source: &Path, limits: HistoryLimits) -> Result<Self> {
        let logical = std::path::absolute(source)
            .map_err(|e| CadError::io(format!("resolving {}", source.display()), e))?;
        let document = Document::open_read_only(&logical)?;
        let directory = SessionDir::create(root)?;
        let first = directory.path.join("v0.fcad");
        // Pinned by the reading that made the versions below, so the copy and the
        // two versions describe one state of the file.
        let disk = DocumentVersion {
            document_id: document.meta().document_id,
            content: document.content_version()?,
        };
        let model = document.model_version()?;
        document.snapshot_to(&first)?;
        document.close()?;
        let snapshot = Snapshot::adopt(first, Arc::clone(&directory))?;
        if snapshot.disk != disk || snapshot.model != model {
            return Err(CadError::io(
                "copying the document into its session",
                "the copy does not have the version of the file it was made from",
            ));
        }
        Ok(Self {
            directory,
            logical,
            saved: Checkpoint { disk, model },
            history: vec![snapshot],
            current: 0,
            next_file: 1,
            generation: 0,
            limits,
        })
    }

    /// Where Save writes, as the user named it.
    pub fn logical_path(&self) -> &Path {
        &self.logical
    }

    /// The name the window shows.
    pub fn display_name(&self) -> String {
        self.logical
            .file_name()
            .unwrap_or(self.logical.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    /// The current version. Held (it is an `Arc`) by whoever reads it for longer
    /// than one call, so the history cannot remove the file underneath them.
    pub fn current(&self) -> Arc<Snapshot> {
        Arc::clone(&self.history[self.current])
    }

    /// Whether the current model differs from what was last saved or opened.
    pub fn is_dirty(&self) -> bool {
        self.history[self.current].model != self.saved.model
    }

    pub fn can_undo(&self) -> bool {
        self.current > 0
    }

    pub fn can_redo(&self) -> bool {
        self.current + 1 < self.history.len()
    }

    pub fn undo_depth(&self) -> usize {
        self.current
    }

    pub fn redo_depth(&self) -> usize {
        self.history.len() - 1 - self.current
    }

    /// Bumped by every change of the current version; a ticket made before it
    /// moved is stale.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The private bytes the history holds.
    pub fn history_bytes(&self) -> u64 {
        self.history.iter().map(|snapshot| snapshot.bytes).sum()
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    #[doc(hidden)]
    pub fn private_directory(&self) -> &Path {
        &self.directory.path
    }

    /// Whether `path` is, or would be, inside this session's private directory.
    /// The directory is removed with the session, so nothing the user is meant to
    /// keep may be written there.
    pub fn owns(&self, path: &Path) -> bool {
        is_inside(&self.directory.path, path)
    }

    // --- steps -----------------------------------------------------------

    /// What an edit needs to run on a worker: the current version to read, where
    /// to write the next one, and the generation it must still be current at.
    pub fn begin_step(&mut self) -> StepTicket {
        let number = self.next_file;
        self.next_file += 1;
        StepTicket {
            source: self.current(),
            destination: self.directory.path.join(format!("v{number}.fcad")),
            generation: self.generation,
            directory: Arc::clone(&self.directory),
        }
    }

    /// Whether committing this step would change the model.
    ///
    /// Asked before the step's scene is built, so a no-op Apply costs neither a
    /// scene load nor a history entry.
    pub fn changes_model(&self, step: &ProducedStep) -> bool {
        step.snapshot.model != self.history[self.current].model
    }

    /// Makes a produced step current. Called at the moment its prepared scene
    /// replaces the shown one, so the two cannot disagree.
    ///
    /// A new step after Undo discards the redo history, and only here, after
    /// success. The oldest versions go when the limits are exceeded.
    ///
    /// # Errors
    ///
    /// [`STALE_STEP`] when the document moved on since the ticket was made. The
    /// session is unchanged and the step's file is removed.
    pub fn commit_step(&mut self, step: ProducedStep) -> Result<StepCommit> {
        let current = &self.history[self.current];
        if step.generation != self.generation || !Arc::ptr_eq(&step.base, current) {
            return Err(CadError::input(STALE_STEP));
        }
        if step.snapshot.model == current.model {
            return Ok(StepCommit::NoChange);
        }
        self.history.truncate(self.current + 1);
        self.history.push(step.snapshot);
        self.current += 1;
        self.generation += 1;
        self.enforce_limits();
        Ok(StepCommit::Accepted)
    }

    fn enforce_limits(&mut self) {
        while self.current > 0
            && (self.history.len() > self.limits.max_versions.max(1)
                || self.history_bytes() > self.limits.max_bytes)
        {
            self.history.remove(0);
            self.current -= 1;
        }
    }

    // --- Undo and Redo ----------------------------------------------------

    /// The version one step back, if there is one. Nothing changes until
    /// [`Self::commit_move`].
    pub fn begin_undo(&self) -> Option<Move> {
        self.begin_move(self.current.checked_sub(1)?)
    }

    pub fn begin_redo(&self) -> Option<Move> {
        let index = self.current + 1;
        (index < self.history.len()).then(|| self.begin_move(index))?
    }

    fn begin_move(&self, index: usize) -> Option<Move> {
        Some(Move {
            generation: self.generation,
            index,
            target: Arc::clone(self.history.get(index)?),
        })
    }

    /// Makes the target of a move current, when its scene is ready to be shown.
    pub fn commit_move(&mut self, request: Move) -> Result<()> {
        let still_there = self
            .history
            .get(request.index)
            .is_some_and(|snapshot| Arc::ptr_eq(snapshot, &request.target));
        if request.generation != self.generation || !still_there {
            return Err(CadError::input(STALE_STEP));
        }
        self.current = request.index;
        self.generation += 1;
        Ok(())
    }

    // --- saving ----------------------------------------------------------

    /// What a save needs to run on a worker. Saving does not change the session;
    /// [`Self::record_saved`] does, once the file is in place.
    pub fn begin_save(&self, target: SaveTarget) -> SavePlan {
        SavePlan::new(
            self.current(),
            self.directory.path.clone(),
            self.logical.clone(),
            self.saved.disk,
            target,
            self.generation,
        )
    }

    /// Notes that exactly `saved` is now on disk at `saved.path`: the saved
    /// checkpoint moves to it and, for Save As, so does the logical path.
    ///
    /// Not conditional on the generation. The file *is* what it is: if the model
    /// moved on after the save began, the session is dirty against the new
    /// checkpoint, which is the truth.
    pub fn record_saved(&mut self, saved: &Saved) {
        self.saved = Checkpoint {
            disk: saved.disk,
            model: saved.model,
        };
        if saved.kind == crate::save::SaveKind::As {
            self.logical = saved.path.clone();
        }
    }

    /// The version last written to or read from the logical path.
    pub fn saved_version(&self) -> DocumentVersion {
        self.saved.disk
    }
}

/// What a worker needs to make the next version. `Send`, owns no session state.
#[derive(Debug)]
pub struct StepTicket {
    source: Arc<Snapshot>,
    destination: PathBuf,
    generation: u64,
    directory: Arc<SessionDir>,
}

impl StepTicket {
    /// The version the edit reads.
    pub fn source(&self) -> &Snapshot {
        &self.source
    }

    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Changes the blind height of one Extrude: the reused `edit-extrude` copy
    /// operation, reading the current version and writing the next, so the
    /// result is what the command line's `edit-extrude` makes from the same
    /// document. The caller owns the kernel and its thread.
    pub fn edit_extrude_height<K: GeometryKernel + ?Sized>(
        self,
        feature: ObjectId,
        distance_mm: f64,
        kernel: &mut K,
        context: &OperationContext,
    ) -> Result<ProducedStep> {
        self.run(|source, expected, destination| {
            edit_extrude_copy(
                &EditExtrudeRequest {
                    source: source.to_path_buf(),
                    expected,
                    feature,
                    distance_mm,
                    destination: destination.to_path_buf(),
                },
                kernel,
                context,
            )
            .map(|_| ())
        })
    }

    /// Moves the vertices of one saved Line Sketch: the reused `edit-sketch-copy`
    /// operation, reading the current version and writing the next, so the result
    /// is what the command line makes from the same document.
    ///
    /// `expected` is the version the form that asked for this was opened from. If
    /// the session has moved on since (an Apply, an Undo, a Redo), the request is
    /// about a picture that is no longer the document and is refused, so an old
    /// form can never be applied to a newer version.
    pub fn edit_sketch_vertices<K: GeometryKernel + ?Sized>(
        self,
        sketch: ObjectId,
        vertices: Vec<ferritecad_document::SketchVertex>,
        expected: DocumentVersion,
        kernel: &mut K,
        context: &OperationContext,
    ) -> Result<ProducedStep> {
        if expected != self.source.version() {
            return Err(CadError::input(
                "the document changed after this form was opened; open it again to edit the current version",
            ));
        }
        self.run(|source, expected, destination| {
            edit_sketch_copy(
                &EditSketchRequest {
                    source: source.to_path_buf(),
                    expected,
                    sketch,
                    vertices,
                    destination: destination.to_path_buf(),
                },
                kernel,
                context,
            )
            .map(|_| ())
        })
    }

    /// Adds and removes constraints of one saved Sketch: the reused
    /// `edit-sketch-constraints-copy` operation, reading the current version and
    /// writing the next, so the result is what the command line makes from the same
    /// document (the solver decides the shape; stored coordinates stay the
    /// solver's starting approximation).
    ///
    /// `expected` is the version the form that asked for this was opened on; a
    /// request about any other version is refused, as for the vertex edit. An
    /// edit that leaves the model as it was is not a step (`changes_model`); a
    /// replaced constraint is a different UUID and so is a step even when the
    /// solved drawing is the same.
    pub fn edit_sketch_constraints<K: GeometryKernel + ?Sized>(
        self,
        sketch: ObjectId,
        edits: ferritecad_document::SketchConstraintEdits,
        expected: DocumentVersion,
        kernel: &mut K,
        context: &OperationContext,
    ) -> Result<ProducedStep> {
        if expected != self.source.version() {
            return Err(CadError::input(
                "the document changed after this form was opened; open it again to edit the current version",
            ));
        }
        self.run(|source, expected, destination| {
            edit_sketch_constraints_copy(
                &EditSketchConstraintsRequest {
                    source: source.to_path_buf(),
                    expected,
                    sketch,
                    edits,
                    destination: destination.to_path_buf(),
                },
                kernel,
                context,
            )
            .map(|_| ())
        })
    }

    /// Moves and resizes one saved analytic circle: the reused `edit-circle`
    /// copy operation, reading the current version and writing the next, so the
    /// result is what the command line's `edit-circle` makes from the same
    /// document. The height is not part of the request.
    ///
    /// `expected` is the version the form that asked for this was opened on; a
    /// request about any other version is refused, as for the other forms. A
    /// request that leaves the model as it was is not a step (`changes_model`).
    pub fn edit_circle<K: GeometryKernel + ?Sized>(
        self,
        sketch: ObjectId,
        edit: ferritecad_document::CircleEdit,
        expected: DocumentVersion,
        kernel: &mut K,
        context: &OperationContext,
    ) -> Result<ProducedStep> {
        self.check_form_version(expected)?;
        self.run(|source, expected, destination| {
            edit_circle_copy(
                &EditCircleRequest {
                    source: source.to_path_buf(),
                    expected,
                    sketch,
                    edit,
                    destination: destination.to_path_buf(),
                },
                kernel,
                context,
            )
            .map(|_| ())
        })
    }

    /// Moves and resizes the two circles of one saved annular profile: the reused
    /// `edit-annular` copy operation, each circle named by its own UUID in the
    /// role it already holds. Version rule and no-op rule as for the circle.
    pub fn edit_annulus<K: GeometryKernel + ?Sized>(
        self,
        sketch: ObjectId,
        edit: ferritecad_document::AnnulusEdit,
        expected: DocumentVersion,
        kernel: &mut K,
        context: &OperationContext,
    ) -> Result<ProducedStep> {
        self.check_form_version(expected)?;
        self.run(|source, expected, destination| {
            edit_annulus_copy(
                &EditAnnulusRequest {
                    source: source.to_path_buf(),
                    expected,
                    sketch,
                    edit,
                    destination: destination.to_path_buf(),
                },
                kernel,
                context,
            )
            .map(|_| ())
        })
    }

    /// Changes one existing Cut by its saved feature/tool identities, using the
    /// current accepted snapshot and the CLI's existing copy operation. New floor
    /// refs belong to the produced version; Undo/Redo never regenerates them.
    pub fn edit_circular_cut<K: GeometryKernel + ?Sized>(
        self,
        cut: ObjectId,
        edit: ferritecad_document::CircularCutEdit,
        expected: DocumentVersion,
        kernel: &mut K,
        context: &OperationContext,
    ) -> Result<ProducedStep> {
        self.check_form_version(expected)?;
        self.run(|source, expected, destination| {
            edit_circular_cut_copy(
                &EditCircularCutRequest {
                    source: source.to_path_buf(),
                    expected,
                    cut,
                    edit,
                    destination: destination.to_path_buf(),
                },
                kernel,
                context,
            )
            .map(|_| ())
        })
    }

    /// Changes the angle of one supported saved partial Revolve using the same
    /// copy job as the CLI. The form must name the current version; model-content
    /// comparison decides no-op, preserving Redo and releasing the private file.
    pub fn edit_revolve_angle<K: GeometryKernel + ?Sized>(
        self,
        feature: ObjectId,
        degrees: f64,
        expected: DocumentVersion,
        kernel: &mut K,
        context: &OperationContext,
    ) -> Result<ProducedStep> {
        self.check_form_version(expected)?;
        self.run(|source, expected, destination| {
            edit_revolve_angle_copy(
                &EditRevolveAngleRequest {
                    source: source.to_path_buf(),
                    expected,
                    feature,
                    degrees,
                    destination: destination.to_path_buf(),
                },
                kernel,
                context,
            )
            .map(|_| ())
        })
    }

    fn check_form_version(&self, expected: DocumentVersion) -> Result<()> {
        if expected != self.source.version() {
            return Err(CadError::input(
                "the document changed after this form was opened; open it again to edit the current version",
            ));
        }
        Ok(())
    }

    /// Runs any copy operation that reads `source` (checking it is still the
    /// `expected` version) and publishes a new file at `destination`.
    ///
    /// The seam both the height edit and the headless state-machine tests use.
    /// What `produce` writes is adopted only if it is a document of the same
    /// identity; anything else is removed and refused.
    pub fn run(
        self,
        produce: impl FnOnce(&Path, DocumentVersion, &Path) -> Result<()>,
    ) -> Result<ProducedStep> {
        let outcome = produce(self.source.path(), self.source.version(), &self.destination);
        match outcome {
            Ok(()) => {}
            Err(error) => {
                let _ = std::fs::remove_file(&self.destination);
                return Err(error);
            }
        }
        let snapshot = Snapshot::adopt(self.destination.clone(), Arc::clone(&self.directory))
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&self.destination);
            })?;
        if snapshot.disk.document_id != self.source.disk.document_id {
            return Err(CadError::input(
                "the edit produced a different document; it was not applied",
            ));
        }
        Ok(ProducedStep {
            generation: self.generation,
            base: self.source,
            snapshot,
        })
    }
}

/// A version an edit made and nobody has accepted yet. Dropping it removes its
/// file and changes nothing else.
#[derive(Debug)]
pub struct ProducedStep {
    generation: u64,
    base: Arc<Snapshot>,
    snapshot: Arc<Snapshot>,
}

impl ProducedStep {
    /// The new version's private file, for the scene load that shows it.
    pub fn path(&self) -> &Path {
        self.snapshot.path()
    }

    pub fn version(&self) -> DocumentVersion {
        self.snapshot.version()
    }
}

/// An Undo or Redo that has been asked for and not yet shown.
#[derive(Debug)]
pub struct Move {
    generation: u64,
    index: usize,
    target: Arc<Snapshot>,
}

impl Move {
    /// The version to show, for the scene load that shows it.
    pub fn path(&self) -> &Path {
        self.target.path()
    }
}

/// `path` with every symbolic link and `..` in its existing part resolved, and
/// the part that does not exist yet appended as written.
fn resolved(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut tail = Vec::new();
    let mut head = absolute.as_path();
    loop {
        if let Ok(real) = std::fs::canonicalize(head) {
            return tail.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match (head.parent(), head.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_owned());
                head = parent;
            }
            _ => return absolute,
        }
    }
}

/// Whether `path` is `directory` or lies under it, by what each really names
/// (links followed), whether or not `path` exists yet.
pub fn is_inside(directory: &Path, path: &Path) -> bool {
    let directory = resolved(directory);
    let inside = |candidate: PathBuf| {
        candidate.starts_with(&directory)
            || candidate
                .ancestors()
                .any(|ancestor| same_file::is_same_file(&directory, ancestor).unwrap_or(false))
    };
    // Canonical spelling alone is not filesystem identity: APFS and Windows
    // can resolve differently cased names to the same directory. Also check the
    // entry's parent, since replacing an outward leaf symlink writes *here*,
    // even when following that symlink resolves outside the disposable folder.
    inside(resolved(path)) || path.parent().is_some_and(|parent| inside(resolved(parent)))
}
