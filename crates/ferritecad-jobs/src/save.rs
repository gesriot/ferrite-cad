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
//!   `Conflict` once the first has published. The sidecar is named after the
//!   resolved file, so every name that reaches it meets one lock, and it is only
//!   ever created, taken over or removed if it is ours (see `SaveLock`). The
//!   readability, link-count and resolution checks are repeated under the lock. A filesystem without advisory
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
    private: PathBuf,
    logical: PathBuf,
    expected: DocumentVersion,
    target: SaveTarget,
    generation: u64,
}

impl SavePlan {
    pub(crate) fn new(
        source: Arc<Snapshot>,
        private: PathBuf,
        logical: PathBuf,
        expected: DocumentVersion,
        target: SaveTarget,
        generation: u64,
    ) -> Self {
        Self {
            source,
            private,
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

    /// The file the logical path resolves to right now: saving through a symbolic
    /// link replaces the file it points at and leaves the link as it is.
    fn resolve(&self) -> Result<PathBuf, SaveFailure> {
        match std::fs::canonicalize(&self.logical) {
            Ok(path) => Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(self.conflict(Conflict::Missing))
            }
            Err(error) => Err(SaveFailure::failed(CadError::io(
                format!("resolving {}", self.logical.display()),
                error,
            ))),
        }
    }

    /// Whether `target` is a file a replacement may take the place of, and its
    /// metadata. Asked early (before the copy is made) and asked again under the
    /// lock: a file made read-only, hard-linked or turned into something else in
    /// between is found by the second asking, not published over.
    fn replaceable(&self, target: &Path) -> Result<std::fs::Metadata, SaveFailure> {
        let metadata = std::fs::metadata(target).map_err(|error| {
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
        Ok(metadata)
    }

    fn in_place(
        &self,
        context: &OperationContext,
        hooks: &mut dyn SaveHooks,
    ) -> Result<Saved, SaveFailure> {
        let target = self.resolve()?;
        let metadata = self.replaceable(&target)?;
        // Cheap and early: a file that is already not ours is found before the
        // copy is made.
        self.compare(&target)?;

        let scratch = Temporary::beside(&target).map_err(SaveFailure::failed)?;
        self.copy_to(scratch.path())?;
        std::fs::set_permissions(scratch.path(), metadata.permissions())
            .map_err(|e| SaveFailure::failed(CadError::io("copying the file's permissions", e)))?;
        hooks.after_copy();
        context.check_cancelled().map_err(SaveFailure::failed)?;

        // One lock per file, not per name: it is keyed by the resolved target, so
        // a window on `plate.fcad` and a window on a link to it meet at one lock.
        let lock = SaveLock::acquire(&target, &self.name())?;
        // Everything checked before the copy is checked again, now that no
        // cooperating saver can be between its own checks and its own rename: the
        // path still resolves to this file, which is still a plain, writable,
        // singly-linked file, and still holds the expected version.
        if self.resolve()? != target {
            return Err(self.conflict(Conflict::Replaced));
        }
        let late = self.replaceable(&target)?;
        self.compare(&target)?;
        if late.permissions() != metadata.permissions() {
            std::fs::set_permissions(scratch.path(), late.permissions()).map_err(|e| {
                SaveFailure::failed(CadError::io("copying the file's permissions", e))
            })?;
        }
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
        // The session's private directory goes with the session: a document saved
        // there would be deleted when the window moves on.
        if crate::session::is_inside(&self.private, &destination) {
            return Err(SaveFailure::new(
                SaveFailureKind::Failed,
                format!(
                    "{} is inside FerriteCAD's temporary working folder, which is deleted when the document is closed; choose a folder of your own.",
                    destination.display()
                ),
            ));
        }
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

/// The first line of every lock file this code makes, and the only proof that a
/// file with that name is ours to touch.
const LOCK_HEADER: &[u8] = b"FERRITECAD-SAVE-LOCK 1\n";

/// An advisory lock on a sidecar beside the file being replaced.
///
/// Held across the second compare and the rename, so two cooperating savers cannot
/// both pass their compare and both rename. Never waited for: a saver that finds
/// it held is told so ([`SaveFailureKind::Busy`]) and the person decides.
///
/// The sidecar is named after the *resolved* target, so every path that reaches
/// the same file reaches the same lock.
///
/// Ownership: a lock file is created exclusively and starts with [`LOCK_HEADER`].
/// A file of that name that does not start with it is somebody's data: it is never
/// written to, locked for good, or removed, and the save is refused. A file that
/// does is a lock of ours left by a saver that died; it is taken over. When the
/// save is done the name is removed only if it still names the very file held.
struct SaveLock {
    path: PathBuf,
    file: File,
    // Fresh creation proves ownership even before its header is written. An
    // existing file becomes ours only after its header has been verified.
    remove_on_drop: bool,
}

impl SaveLock {
    fn acquire(target: &Path, name: &str) -> Result<Self, SaveFailure> {
        Self::acquire_with(target, name, |_, _| {})
    }

    fn acquire_with(
        target: &Path,
        name: &str,
        mut after_open: impl FnMut(&Path, bool),
    ) -> Result<Self, SaveFailure> {
        use std::io::{Read as _, Write as _};

        let parent = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let sidecar = target.file_name().unwrap_or(target.as_os_str());
        let mut file_name = std::ffi::OsString::from(".");
        file_name.push(sidecar);
        file_name.push(".ferritecad-save-lock");
        let path = parent.join(file_name);
        let busy = || {
            SaveFailure::new(
                SaveFailureKind::Busy,
                format!(
                    "{name} is being saved by another FerriteCAD window or process right now; try again in a moment."
                ),
            )
        };
        let io = |what: &str, e: std::io::Error| {
            SaveFailure::failed(CadError::io(format!("{what} {}", path.display()), e))
        };
        for _ in 0..4 {
            let (file, fresh) = match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => (file, true),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Whatever is there, it is not followed if it is a link and
                    // not touched if it is not ours.
                    let kind = std::fs::symlink_metadata(&path).map_err(|e| io("inspecting", e))?;
                    if !kind.is_file() {
                        return Err(unrelated(&path));
                    }
                    let file = std::fs::File::open(&path).map_err(|e| io("opening", e))?;
                    (file, false)
                }
                Err(error) => return Err(io("creating", error)),
            };
            let mut lock = Self {
                path: path.clone(),
                file,
                remove_on_drop: fresh,
            };
            after_open(&path, fresh);
            match lock.file.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => return Err(busy()),
                // No advisory locks here: the compare alone, as documented.
                Err(std::fs::TryLockError::Error(error))
                    if error.kind() == std::io::ErrorKind::Unsupported => {}
                Err(std::fs::TryLockError::Error(error)) => return Err(io("locking", error)),
            }
            if fresh {
                lock.file
                    .write_all(LOCK_HEADER)
                    .and_then(|()| lock.file.flush())
                    .map_err(|error| io("writing", error))?;
            } else {
                let mut found = vec![0u8; LOCK_HEADER.len()];
                let read = lock.file.read_exact(&mut found);
                if read.is_err() || found != LOCK_HEADER {
                    return Err(unrelated(&path));
                }
                lock.remove_on_drop = true;
            }
            // The saver before us removes the name when it finishes. A lock taken
            // on a file that no longer has the name is a lock on nothing.
            let held = same_file::Handle::from_file(
                lock.file
                    .try_clone()
                    .map_err(|e| io("duplicating a handle to", e))?,
            );
            let named = same_file::Handle::from_path(&path);
            if matches!((held, named), (Ok(held), Ok(named)) if held == named) {
                return Ok(lock);
            }
        }
        Err(busy())
    }
}

fn unrelated(path: &Path) -> SaveFailure {
    SaveFailure::new(
        SaveFailureKind::Failed,
        format!(
            "{} is not a FerriteCAD lock file, so Save will not touch it or go past it. Move it away, or use Save As.",
            path.display()
        ),
    )
}

impl Drop for SaveLock {
    fn drop(&mut self) {
        if !self.remove_on_drop
            || !std::fs::symlink_metadata(&self.path).is_ok_and(|metadata| metadata.is_file())
        {
            return;
        }
        // The name goes while the lock is still held, so nobody can lock a stale
        // name, and only if it still names the file this lock holds: a name that
        // was replaced by something else belongs to whoever put it there.
        let still_ours = self
            .file
            .try_clone()
            .and_then(same_file::Handle::from_file)
            .and_then(|held| same_file::Handle::from_path(&self.path).map(|named| held == named));
        if matches!(still_ours, Ok(true)) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_first_lock_does_not_leave_an_empty_unrecoverable_sidecar() {
        let root = tempfile::tempdir().expect("directory");
        let target = root.path().join("model.fcad");
        std::fs::write(&target, b"source").expect("file");
        let lock_path = root.path().join(".model.fcad.ferritecad-save-lock");
        let mut contender = None;
        let failed = SaveLock::acquire_with(&target, "model.fcad", |path, fresh| {
            assert!(fresh);
            // Another saver opened the newly created, still-empty name and
            // reached its advisory lock first. It has not read the header yet.
            let file = File::open(path).expect("contender");
            file.try_lock().expect("first to lock");
            contender = Some(file);
        });
        assert!(matches!(
            failed,
            Err(SaveFailure {
                kind: SaveFailureKind::Busy,
                ..
            })
        ));
        drop(contender);
        assert!(
            !lock_path.exists(),
            "failed creator left an empty unowned file that blocks every later Save"
        );
        drop(SaveLock::acquire(&target, "model.fcad").expect("retry works"));
        assert!(!lock_path.exists());
        assert_eq!(std::fs::read(target).expect("source"), b"source");
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_keeps_a_replacement_symlink_even_if_it_targets_the_held_inode() {
        let root = tempfile::tempdir().expect("directory");
        let target = root.path().join("model.fcad");
        let lock = SaveLock::acquire(&target, "model.fcad").expect("lock");
        let path = lock.path.clone();
        let moved = root.path().join("moved-lock");
        std::fs::rename(&path, &moved).expect("move held inode");
        std::os::unix::fs::symlink(&moved, &path).expect("someone else's directory entry");
        drop(lock);
        assert!(
            std::fs::symlink_metadata(&path)
                .expect("replacement is preserved")
                .is_symlink()
        );
        assert_eq!(std::fs::read(moved).expect("held file"), LOCK_HEADER);
    }
}
