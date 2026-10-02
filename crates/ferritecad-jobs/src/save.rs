// SPDX-License-Identifier: MIT
//! Putting the accepted version where the user's file is: Save and Save As.
//!
//! Not a copy job. The copy operations (`edit-extrude` and the rest) refuse to
//! write over their source and refuse any occupied destination, and those refusals
//! are the protection the command line relies on; they are unchanged. Save is a
//! different operation with its own, narrower permission: it may replace *one*
//! file, the session's logical path, and only if that file still holds exactly the
//! version the session last read from it or wrote to it. Save As may write one new
//! file and refuses an occupied one.
//!
//! # What is guaranteed, and what is not
//!
//! Replacing a file is atomic (a rename: a reader sees the old file or the new
//! one). Comparing the file's version and replacing it are two operations, and no
//! portable filesystem call joins them. So:
//!
//! * A file changed, replaced or deleted before the save is a typed
//!   [`SaveFailureKind::Conflict`], found by comparing the document id and the
//!   complete content version, never by modification time.
//! * Cooperating FerriteCAD savers are serialised: an advisory lock on a sidecar
//!   beside the file is held across the second compare and the rename. A second
//!   saver gets [`SaveFailureKind::Busy`] while the first holds it and a
//!   `Conflict` once the first has published. A filesystem without advisory
//!   locks degrades to the compare alone.
//! * A writer that ignores the lock can still change the file in the instant
//!   between the compare under the lock and the rename. That window is the cost
//!   of one rename, it is exercised by a test rather than denied, and nothing here
//!   claims to close it.
//! * Everything before the rename leaves the user's file exactly as it was;
//!   after the rename the result is [`Saved`], whatever else arrives later
//!   (a cancellation, a failed delivery). Existing exit-7 semantics are the
//!   caller's to keep: a published save is never reported as "nothing happened".

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ferritecad_document::{Document, DocumentVersion};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, ContentHash, ErrorKind};

use crate::session::Snapshot;
use crate::{Existing, Temporary, path_entry_exists};

/// Where a save goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveTarget {
    /// The session's logical path, replaced if it still holds the saved version.
    InPlace,
    /// A new file. An occupied path is refused.
    As(PathBuf),
}

/// What kind of save succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    InPlace,
    As,
}

/// A file is in place: this version, at this path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub path: PathBuf,
    pub kind: SaveKind,
    pub disk: DocumentVersion,
    pub model: ContentHash,
}

/// Why the file the session saved to is not the one it expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// Same document, different content: edited outside FerriteCAD.
    Modified,
    /// A different document, or not a document at all.
    Replaced,
    /// Gone.
    Missing,
}

/// What kind of failure a save was. In every case nothing was published and the
/// user's file is as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFailureKind {
    /// The file is not the version the session saved or read last (see
    /// [`Conflict`]). Save As keeps the user's work.
    Conflict(Conflict),
    /// Another saver holds the file right now. Try again.
    Busy,
    /// Save As onto a path that already exists.
    Occupied,
    /// The file is read-only, or has other hard links that a replacement would
    /// silently detach.
    Unwritable,
    /// Cancelled before the file was replaced.
    Cancelled,
    /// Anything else: an I/O failure, a copy that could not be made.
    Failed,
}

/// A save that published nothing, and why.
#[derive(Debug)]
pub struct SaveFailure {
    pub kind: SaveFailureKind,
    pub error: CadError,
}

impl SaveFailure {
    fn new(kind: SaveFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            error: CadError::input(message),
        }
    }

    fn failed(error: CadError) -> Self {
        let kind = if error.kind() == ErrorKind::Cancellation {
            SaveFailureKind::Cancelled
        } else {
            SaveFailureKind::Failed
        };
        Self { kind, error }
    }
}

impl std::fmt::Display for SaveFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

/// Points inside a save where a test can act, to exercise the real race
/// boundaries. Everything defaults to doing nothing.
#[doc(hidden)]
pub trait SaveHooks {
    /// After the private copy was made, before the lock is taken.
    fn after_copy(&mut self) {}
    /// Holding the lock, after the second compare, immediately before the rename.
    fn inside_lock(&mut self) {}
    /// After the file was replaced or linked: a cancellation arriving here is late.
    fn after_publish(&mut self) {}
}

struct NoHooks;
impl SaveHooks for NoHooks {}

/// What a worker needs to save. `Send`; owns no session state.
#[derive(Debug)]
pub struct SavePlan {
    source: Arc<Snapshot>,
    logical: PathBuf,
    expected: DocumentVersion,
    target: SaveTarget,
    generation: u64,
}

impl SavePlan {
    pub(crate) fn new(
        source: Arc<Snapshot>,
        logical: PathBuf,
        expected: DocumentVersion,
        target: SaveTarget,
        generation: u64,
    ) -> Self {
        Self {
            source,
            logical,
            expected,
            target,
            generation,
        }
    }

    /// The generation of the session this plan was made at.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn target(&self) -> &SaveTarget {
        &self.target
    }

    /// Does the save. The caller owns the thread; `context` can cancel it up to
    /// the instant before the file is replaced or linked.
    pub fn run(self, context: &OperationContext) -> Result<Saved, SaveFailure> {
        self.run_with(context, &mut NoHooks)
    }

    #[doc(hidden)]
    pub fn run_with(
        self,
        context: &OperationContext,
        hooks: &mut dyn SaveHooks,
    ) -> Result<Saved, SaveFailure> {
        context.check_cancelled().map_err(SaveFailure::failed)?;
        match self.target.clone() {
            SaveTarget::InPlace => self.in_place(context, hooks),
            SaveTarget::As(destination) => self.as_new(&destination, context, hooks),
        }
    }

    fn saved(&self, path: PathBuf, kind: SaveKind) -> Saved {
        Saved {
            path,
            kind,
            disk: self.source.version(),
            model: self.source.model(),
        }
    }

    fn copy_to(&self, destination: &Path) -> Result<(), SaveFailure> {
        let document = Document::open_read_only(self.source.path()).map_err(SaveFailure::failed)?;
        document
            .snapshot_to(destination)
            .map_err(SaveFailure::failed)?;
        document.close().map_err(SaveFailure::failed)
    }

    fn name(&self) -> String {
        self.logical
            .file_name()
            .unwrap_or(self.logical.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    fn in_place(
        &self,
        context: &OperationContext,
        hooks: &mut dyn SaveHooks,
    ) -> Result<Saved, SaveFailure> {
        // The path resolved: saving through a symbolic link replaces the file it
        // points at and leaves the link as it is.
        let target = match std::fs::canonicalize(&self.logical) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(self.conflict(Conflict::Missing));
            }
            Err(error) => {
                return Err(SaveFailure::failed(CadError::io(
                    format!("resolving {}", self.logical.display()),
                    error,
                )));
            }
        };
        let metadata = std::fs::metadata(&target).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                self.conflict(Conflict::Missing)
            } else {
                SaveFailure::failed(CadError::io(format!("reading {}", target.display()), error))
            }
        })?;
        if !metadata.is_file() {
            return Err(self.conflict(Conflict::Replaced));
        }
        if metadata.permissions().readonly() {
            return Err(SaveFailure::new(
                SaveFailureKind::Unwritable,
                format!(
                    "{} is read-only; Save was not done. Use Save As to keep your changes under another name.",
                    self.name()
                ),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            if metadata.nlink() > 1 {
                return Err(SaveFailure::new(
                    SaveFailureKind::Unwritable,
                    format!(
                        "{} has other hard links, and replacing it would detach them from your changes; Save was not done. Use Save As.",
                        self.name()
                    ),
                ));
            }
        }
        // Cheap and early: a file that is already not ours is found before the
        // copy is made.
        self.compare(&target)?;

        let scratch = Temporary::beside(&target).map_err(SaveFailure::failed)?;
        self.copy_to(scratch.path())?;
        std::fs::set_permissions(scratch.path(), metadata.permissions())
            .map_err(|e| SaveFailure::failed(CadError::io("copying the file's permissions", e)))?;
        hooks.after_copy();
        context.check_cancelled().map_err(SaveFailure::failed)?;

        let lock = SaveLock::acquire(&target, &self.name())?;
        // Again, now that no cooperating saver can be between its own compare and
        // its own rename.
        self.compare(&target)?;
        hooks.inside_lock();
        // The last place a cancellation is honoured. Past this line the file is
        // replaced, and the answer is a success whatever arrives next.
        context.check_cancelled().map_err(SaveFailure::failed)?;
        scratch
            .publish(&target, Existing::Replace)
            .map_err(SaveFailure::failed)?;
        drop(lock);
        hooks.after_publish();
        Ok(self.saved(self.logical.clone(), SaveKind::InPlace))
    }

    fn as_new(
        &self,
        destination: &Path,
        context: &OperationContext,
        hooks: &mut dyn SaveHooks,
    ) -> Result<Saved, SaveFailure> {
        let destination = std::path::absolute(destination).map_err(|e| {
            SaveFailure::failed(CadError::io(
                format!("resolving {}", destination.display()),
                e,
            ))
        })?;
        let occupied = || {
            SaveFailure::new(
                SaveFailureKind::Occupied,
                format!(
                    "{} already exists; Save As does not replace a file. Choose a different name.",
                    destination.display()
                ),
            )
        };
        if path_entry_exists(&destination).map_err(SaveFailure::failed)? {
            return Err(occupied());
        }
        let scratch = Temporary::beside(&destination).map_err(SaveFailure::failed)?;
        self.copy_to(scratch.path())?;
        hooks.after_copy();
        context.check_cancelled().map_err(SaveFailure::failed)?;
        scratch
            .publish(
                &destination,
                Existing::Keep {
                    advice: "choose a different name",
                },
            )
            .map_err(|error| {
                // The only `input` failure the no-clobber publication has is the
                // destination appearing while the copy was being made.
                if error.kind() == ErrorKind::Input {
                    occupied()
                } else {
                    SaveFailure::failed(error)
                }
            })?;
        hooks.after_publish();
        Ok(self.saved(destination, SaveKind::As))
    }

    /// Whether the file at `target` still holds the version the session expects.
    fn compare(&self, target: &Path) -> Result<(), SaveFailure> {
        let document = match Document::open_read_only(target) {
            Ok(document) => document,
            Err(error) => {
                // Why it did not open decides what it is. A file that is gone is
                // Missing; one that cannot even be read is an I/O failure; one that
                // can be read but is not a document this build opens has been
                // replaced by something else.
                return Err(match std::fs::File::open(target) {
                    Err(open) if open.kind() == std::io::ErrorKind::NotFound => {
                        self.conflict(Conflict::Missing)
                    }
                    Err(open) => SaveFailure::failed(CadError::io(
                        format!("reading {}", target.display()),
                        open,
                    )),
                    Ok(_) if error.kind() == ErrorKind::Cancellation => SaveFailure::failed(error),
                    Ok(_) => self.conflict(Conflict::Replaced),
                });
            }
        };
        let found = DocumentVersion {
            document_id: document.meta().document_id,
            content: document.content_version().map_err(SaveFailure::failed)?,
        };
        // Closed before anything replaces it: a replaced file must not have a
        // handle open on it.
        document.close().map_err(SaveFailure::failed)?;
        if found.document_id != self.expected.document_id {
            return Err(self.conflict(Conflict::Replaced));
        }
        if found.content != self.expected.content {
            return Err(self.conflict(Conflict::Modified));
        }
        Ok(())
    }

    fn conflict(&self, conflict: Conflict) -> SaveFailure {
        let name = self.name();
        let what = match conflict {
            Conflict::Modified => {
                format!("{name} was changed outside FerriteCAD after it was opened or last saved")
            }
            Conflict::Replaced => {
                format!("{name} was replaced by a different file after it was opened or last saved")
            }
            Conflict::Missing => format!("{name} no longer exists where it was opened"),
        };
        SaveFailure::new(
            SaveFailureKind::Conflict(conflict),
            format!(
                "{what}; Save was not done and the file was not touched. Use Save As to keep your changes under another name."
            ),
        )
    }
}

/// An advisory lock on a sidecar beside the file being replaced.
///
/// Held across the second compare and the rename, so two cooperating savers cannot
/// both pass their compare and both rename. Never waited for: a saver that finds
/// it held is told so ([`SaveFailureKind::Busy`]) and the person decides.
struct SaveLock {
    path: PathBuf,
    file: Option<File>,
}

impl SaveLock {
    fn acquire(target: &Path, name: &str) -> Result<Self, SaveFailure> {
        let parent = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let path = parent.join(format!(".{name}.ferritecad-save-lock"));
        for _ in 0..4 {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map_err(|e| {
                    SaveFailure::failed(CadError::io(format!("locking {}", path.display()), e))
                })?;
            match file.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(SaveFailure::new(
                        SaveFailureKind::Busy,
                        format!(
                            "{name} is being saved by another FerriteCAD window or process right now; try again in a moment."
                        ),
                    ));
                }
                Err(std::fs::TryLockError::Error(error))
                    if error.kind() == std::io::ErrorKind::Unsupported =>
                {
                    // No advisory locks here: the compare alone, as documented.
                    return Ok(Self { path, file: None });
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(SaveFailure::failed(CadError::io(
                        format!("locking {}", path.display()),
                        error,
                    )));
                }
            }
            // The saver before us removes the name when it finishes. A lock taken
            // on a file that no longer has the name is a lock on nothing.
            let same =
                same_file::Handle::from_file(file.try_clone().map_err(|e| {
                    SaveFailure::failed(CadError::io("duplicating a lock handle", e))
                })?)
                .and_then(|held| same_file::Handle::from_path(&path).map(|named| held == named));
            if matches!(same, Ok(true)) {
                return Ok(Self {
                    path,
                    file: Some(file),
                });
            }
        }
        Err(SaveFailure::new(
            SaveFailureKind::Busy,
            format!(
                "{name} is being saved by another FerriteCAD window or process right now; try again in a moment."
            ),
        ))
    }
}

impl Drop for SaveLock {
    fn drop(&mut self) {
        // The name goes while the lock is still held, so nobody can lock a stale
        // name; the lock itself goes when the handle closes.
        if self.file.is_some() {
            let _ = std::fs::remove_file(&self.path);
        }
        self.file = None;
    }
}
