// SPDX-License-Identifier: MIT
//! The last published crash copy of the accepted model (§30M, ADR 0005).
//!
//! A [`DocumentSession`] keeps its versions in a private directory that goes with
//! the process. This module keeps, beside that and outside the system temporary
//! directory, one copy of the session's *current accepted* version for as long as
//! it has unsaved changes, so that a person whose window ended without their
//! deciding can find it on the next start.
//!
//! # One owner
//!
//! [`RecoveryStore`] owns a per-user folder. Inside it every session that needed a
//! copy has one *record*: a directory `r-<uuid>` holding a `lease`, a `manifest`
//! and one `c<n>.fcad`. Nothing else in the folder, and nothing outside it, is ever
//! read as a record or removed.
//!
//! * [`RecoveryRecord`] is a live session's record, written by
//!   [`RecoveryRecord::publish`] and emptied or removed when the session's life ends.
//! * [`RecoveryClaim`] is a record whose owner is gone, validated and held.
//! * [`RecoveryRecorder`] is the one worker thread a window hands accepted
//!   versions to; the event loop never copies, hashes or `fsync`s anything. Each
//!   open document of a window (§30O: each tab) talks to it through its own lane
//!   ([`RecoveryRecorder::lane`]), so what one document does never ends another's
//!   record.
//!
//! # Ownership is a lock, not a guess
//!
//! The owner of a record holds an exclusive advisory lock on its `lease` for the
//! record's whole life, and the operating system lets go of it when the process
//! ends however it ends. A lease that can be locked (and carries the header) is
//! an orphan; one that cannot is held, and is neither offered nor touched. A claim
//! keeps holding the lock, so two processes never restore or extract one record at
//! once. No PID, host name or age is consulted.
//!
//! # Reading is not owning (§30S)
//!
//! A listing takes the lease **shared**, only while it verifies that one record.
//! A shared lock is refused exactly when somebody holds the lock exclusively, so a
//! listing knows nobody owns, claims or removes the record while it reads, and
//! nobody can start to until it lets go; two listings never exclude each other. A
//! claim or removal still takes the lock exclusively: it *waits, bounded and
//! cancellable, for readers* (they end by themselves) and never for a holder. What
//! an exclusive refusal means is then proven rather than guessed: a shared
//! try returning `WouldBlock` means an exclusive holder (a window, a claim or a removal — the primitive
//! cannot say which), a granted one means only readers.
//!
//! # What a crash can leave
//!
//! A publication copies to a partial name, syncs and verifies it, renames it into
//! place, then writes the manifest the same way, then removes the previous copy.
//! The manifest is what names a copy, so a process that dies at any point leaves
//! the previous complete copy or the new one; partial names are never read. What is
//! recovered is the last copy whose manifest was published. Whether that also
//! survives a power cut depends on the platform honouring `fsync`, and nothing here
//! claims it does.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ferritecad_document::Document;
use ferritecad_kernel::CancelToken;
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result};

use crate::publish::{Existing, Temporary, path_entry_exists};
use crate::session::{DocumentSession, Snapshot, UNTITLED, is_inside};

/// Names a recovery folder other than the per-user default (tests, a recipe).
pub const RECOVERY_DIR_ENV: &str = "FERRITECAD_RECOVERY_DIR";

/// The most records a folder holds before a new session gets none.
pub const MAX_RECORDS: usize = 32;

const RECORD_PREFIX: &str = "r-";
const LEASE: &str = "lease";
const MANIFEST: &str = "manifest";
const MANIFEST_PARTIAL: &str = ".manifest.partial";
const LEASE_HEADER: &[u8] = b"FERRITECAD-RECOVERY-LEASE 1\n";
const MANIFEST_MAGIC: &str = "FERRITECAD-RECOVERY ";
const MANIFEST_VERSION: u32 = 1;
/// A manifest is a dozen short lines; anything bigger is not one of ours.
const MANIFEST_LIMIT: u64 = 64 * 1024;
const NAME_LIMIT: usize = 200;
/// The most a claim or removal waits for readers of its record. A reader holds the
/// lock for one verification (a SQLite open and a BLAKE3 pass over one copy), which
/// ends by itself; this only bounds a reader that is stuck.
const READER_WAIT: Duration = Duration::from_secs(5);
/// How often that wait looks again. `File` has no timed lock.
const READER_POLL: Duration = Duration::from_millis(5);

/// Why a publication was not written: a newer one already was.
pub const STALE_RECOVERY: &str =
    "a newer recovery copy of this document was already requested; this older one was not written";

/// The per-user recovery folder: [`RECOVERY_DIR_ENV`] when set, otherwise the
/// platform's place for an application's own state. Never the system temporary
/// directory.
pub fn default_recovery_root() -> Result<PathBuf> {
    per_user_state_folder(RECOVERY_DIR_ENV, "recovery folder", "Recovery", "recovery")
}

/// A folder of FerriteCAD's own per-user state: `env` when it is set and not
/// empty, otherwise `~/Library/Application Support/FerriteCAD/<leaf>` (macOS),
/// `%LOCALAPPDATA%\FerriteCAD\<leaf>` (Windows), or
/// `$XDG_STATE_HOME/ferritecad/<unix_leaf>`, by default
/// `~/.local/state/ferritecad/<unix_leaf>` (Linux, other Unix). Never the system
/// temporary directory. Nothing is created; `what` names the folder in the refusal.
pub fn per_user_state_folder(
    env: &str,
    what: &str,
    leaf: &str,
    unix_leaf: &str,
) -> Result<PathBuf> {
    if let Some(chosen) = std::env::var_os(env).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(chosen));
    }
    let home = || {
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                CadError::input(format!(
                    "no home folder is known, so there is no {what}; set {env}"
                ))
            })
    };
    if cfg!(target_os = "macos") {
        return Ok(home()?
            .join("Library")
            .join("Application Support")
            .join("FerriteCAD")
            .join(leaf));
    }
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                CadError::input(format!(
                    "LOCALAPPDATA is not set, so there is no {what}; set {env}"
                ))
            })?;
        return Ok(PathBuf::from(local).join("FerriteCAD").join(leaf));
    }
    if let Some(state) = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Ok(state.join("ferritecad").join(unix_leaf));
    }
    Ok(home()?
        .join(".local")
        .join("state")
        .join("ferritecad")
        .join(unix_leaf))
}

/// A record's identity: the UUID in its directory name and in its manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RecordId(ObjectId);

impl RecordId {
    fn new() -> Self {
        Self(ObjectId::new())
    }

    fn directory_name(self) -> String {
        format!("{RECORD_PREFIX}{}", self.0)
    }

    fn from_directory_name(name: &str) -> Option<Self> {
        name.strip_prefix(RECORD_PREFIX)?.parse().ok().map(Self)
    }
}

impl std::fmt::Display for RecordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for RecordId {
    type Err = CadError;

    fn from_str(s: &str) -> Result<Self> {
        s.parse()
            .map(Self)
            .map_err(|e| CadError::input_because(format!("{s:?} is not a recovery record"), e))
    }
}

/// What a recoverable record holds, as its validated manifest says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverySummary {
    pub record: RecordId,
    /// What the document was called: a file name or [`UNTITLED`]. Never a path.
    pub name: String,
    /// When the copy was confirmed written (Unix time, milliseconds).
    pub written_unix_ms: u64,
    pub document_id: DocumentId,
    pub content: ContentHash,
    pub model: ContentHash,
    pub bytes: u64,
    pub sequence: u64,
}

/// Why one record cannot be used. Only that record is affected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RefusalKind {
    /// Some FerriteCAD process holds the lock exclusively: a running window, a
    /// claim or a removal. The lock cannot say which.
    Active,
    /// Only readers held it, and still did when the bounded wait ended. Try again.
    Busy,
    /// The caller stopped waiting.
    Cancelled,
    /// Written by a FerriteCAD that uses a manifest version this one does not read.
    UnknownVersion,
    /// Not a complete record: a missing, malformed or truncated part.
    Damaged,
    /// Complete, but the copy is not the one the manifest describes.
    Mismatch,
    /// This filesystem cannot say whether another process holds it.
    Unlockable,
    /// No such record.
    NotFound,
}

impl RefusalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Busy => "busy",
            Self::Cancelled => "cancelled",
            Self::UnknownVersion => "unknown-version",
            Self::Damaged => "damaged",
            Self::Mismatch => "mismatch",
            Self::Unlockable => "unlockable",
            Self::NotFound => "not-found",
        }
    }
}

/// A record that was not opened, and why.
#[derive(Debug)]
pub struct RecoveryRefusal {
    pub record: RecordId,
    pub kind: RefusalKind,
    pub error: CadError,
}

impl RecoveryRefusal {
    fn new(record: RecordId, kind: RefusalKind, message: impl Into<String>) -> Self {
        Self {
            record,
            kind,
            error: CadError::input(message),
        }
    }

    fn io(record: RecordId, error: CadError) -> Self {
        Self {
            record,
            kind: RefusalKind::Damaged,
            error,
        }
    }
}

impl std::fmt::Display for RecoveryRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl From<RecoveryRefusal> for CadError {
    fn from(refusal: RecoveryRefusal) -> Self {
        refusal.error
    }
}

/// One record found in the folder.
#[derive(Debug)]
pub enum RecoveryEntry {
    Recoverable(RecoverySummary),
    Refused(RecoveryRefusal),
}

/// What [`RecoveryStore::list`] found. Records held exclusively are only counted.
#[derive(Debug, Default)]
pub struct RecoveryListing {
    /// Newest recoverable first, then refusals.
    pub entries: Vec<RecoveryEntry>,
    /// Records some FerriteCAD process holds exclusively (a running window, a claim
    /// or a removal): never listed, claimed or removed here.
    pub active: usize,
    /// Orphaned records with nothing in them.
    pub empty: usize,
}

impl RecoveryListing {
    pub fn recoverable(&self) -> impl Iterator<Item = &RecoverySummary> {
        self.entries.iter().filter_map(|entry| match entry {
            RecoveryEntry::Recoverable(summary) => Some(summary),
            RecoveryEntry::Refused(_) => None,
        })
    }
}

/// A copy extracted to a file of the user's choosing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedRecovery {
    pub record: RecordId,
    pub destination: PathBuf,
    pub document_id: DocumentId,
    pub content: ContentHash,
    pub name: String,
}

/// What a publication did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Publication {
    /// A new copy was published.
    Written { sequence: u64, written_unix_ms: u64 },
    /// The record already holds exactly this model; nothing was written.
    Unchanged { sequence: u64, written_unix_ms: u64 },
}

impl Publication {
    pub fn written_unix_ms(self) -> u64 {
        match self {
            Self::Written {
                written_unix_ms, ..
            }
            | Self::Unchanged {
                written_unix_ms, ..
            } => written_unix_ms,
        }
    }
}

/// Points inside a publication where a test can stop the process, to exercise
/// each phase a crash can interrupt. Everything defaults to doing nothing.
#[doc(hidden)]
pub trait RecoveryHooks {
    /// The copy is complete and synced under its partial name.
    fn after_copy(&mut self) {}
    /// The copy has its final name; the manifest still names the previous one.
    fn after_copy_published(&mut self) {}
    /// The new manifest is written under its partial name, not yet renamed.
    fn after_manifest_written(&mut self) {}
    /// The manifest has been renamed; a test can inject failure of the final sync.
    fn sync_published_manifest(&mut self, directory: &Path) -> Result<()> {
        sync_directory(directory)
    }
}

struct NoHooks;
impl RecoveryHooks for NoHooks {}

// --- the folder ----------------------------------------------------------------

/// The recovery folder. See the module documentation.
#[derive(Debug, Clone)]
pub struct RecoveryStore {
    root: PathBuf,
    limit: usize,
}

impl RecoveryStore {
    /// Opens (creating it, private, if needed) the folder at `root`.
    pub fn open(root: &Path) -> Result<Self> {
        let root = std::path::absolute(root)
            .map_err(|e| CadError::io(format!("resolving {}", root.display()), e))?;
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt as _;
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder
        };
        #[cfg(not(unix))]
        let builder = {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            builder
        };
        builder.create(&root).map_err(|e| {
            CadError::io(
                format!("creating the recovery folder {}", root.display()),
                e,
            )
        })?;
        if !std::fs::metadata(&root).is_ok_and(|metadata| metadata.is_dir()) {
            return Err(CadError::input(format!(
                "{} is not a folder, so it cannot hold recovery copies",
                root.display()
            )));
        }
        Ok(Self {
            root,
            limit: MAX_RECORDS,
        })
    }

    /// The folder at `root`, without creating it: a folder that does not exist
    /// holds nothing (the command line's listing changes nothing).
    pub fn at(root: &Path) -> Result<Self> {
        let root = std::path::absolute(root)
            .map_err(|e| CadError::io(format!("resolving {}", root.display()), e))?;
        Ok(Self {
            root,
            limit: MAX_RECORDS,
        })
    }

    /// The folder at the default place ([`default_recovery_root`]).
    pub fn open_default() -> Result<Self> {
        Self::open(&default_recovery_root()?)
    }

    /// The same folder with another bound on its records (tests).
    #[doc(hidden)]
    pub fn with_record_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn record_directory(&self, record: RecordId) -> PathBuf {
        self.root.join(record.directory_name())
    }

    /// The record directories in the folder, by name. Anything else is not ours.
    fn record_ids(&self) -> Result<Vec<RecordId>> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(CadError::io(
                    format!("reading the recovery folder {}", self.root.display()),
                    error,
                ));
            }
        };
        let mut found = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| CadError::io("reading the recovery folder", e))?;
            if let Some(record) = entry
                .file_name()
                .to_str()
                .and_then(RecordId::from_directory_name)
            {
                found.push(record);
            }
        }
        found.sort();
        Ok(found)
    }

    /// What is in the folder. Reads and verifies every record nobody holds
    /// exclusively, each under a shared lock for the length of its own reading;
    /// changes nothing and keeps no lease. The answer is advisory: a claim verifies
    /// the record again under its own lock.
    pub fn list(&self) -> Result<RecoveryListing> {
        let mut listing = RecoveryListing::default();
        for record in self.record_ids()? {
            match self.inspect(record, Hold::Read) {
                Inspected::Verified(_, summary) => {
                    listing.entries.push(RecoveryEntry::Recoverable(summary));
                }
                Inspected::Empty(_) => listing.empty += 1,
                Inspected::Active => listing.active += 1,
                Inspected::NotOurs => {}
                Inspected::Refused(refusal) => {
                    listing.entries.push(RecoveryEntry::Refused(refusal));
                }
            }
        }
        listing.entries.sort_by(|a, b| match (a, b) {
            (RecoveryEntry::Recoverable(a), RecoveryEntry::Recoverable(b)) => b
                .written_unix_ms
                .cmp(&a.written_unix_ms)
                .then(a.record.cmp(&b.record)),
            (RecoveryEntry::Recoverable(_), RecoveryEntry::Refused(_)) => std::cmp::Ordering::Less,
            (RecoveryEntry::Refused(_), RecoveryEntry::Recoverable(_)) => {
                std::cmp::Ordering::Greater
            }
            (RecoveryEntry::Refused(a), RecoveryEntry::Refused(b)) => a.record.cmp(&b.record),
        });
        Ok(listing)
    }

    /// Takes an orphaned record and verifies it. The lease stays held until the
    /// claim is dropped, finished, or handed to a recovered session. Waits a
    /// bounded time for readers of the record, never for a holder.
    pub fn claim(&self, record: RecordId) -> std::result::Result<RecoveryClaim, RecoveryRefusal> {
        self.claim_cancellable(record, &CancelToken::new())
    }

    /// [`Self::claim`] whose wait for readers ends at once when `cancel` is
    /// cancelled, with [`RefusalKind::Cancelled`] and nothing taken.
    pub fn claim_cancellable(
        &self,
        record: RecordId,
        cancel: &CancelToken,
    ) -> std::result::Result<RecoveryClaim, RecoveryRefusal> {
        self.claim_within(record, cancel, READER_WAIT)
    }

    fn claim_within(
        &self,
        record: RecordId,
        cancel: &CancelToken,
        wait: Duration,
    ) -> std::result::Result<RecoveryClaim, RecoveryRefusal> {
        match self.inspect(record, Hold::Own(cancel, wait)) {
            Inspected::Verified(lease, summary) => Ok(RecoveryClaim {
                root: self.root.clone(),
                directory: self.record_directory(record),
                lease,
                summary,
            }),
            Inspected::Empty(_) => Err(RecoveryRefusal::new(
                record,
                RefusalKind::NotFound,
                format!("recovery record {record} holds no copy"),
            )),
            Inspected::Active => Err(active(record)),
            Inspected::NotOurs => Err(RecoveryRefusal::new(
                record,
                RefusalKind::NotFound,
                format!("there is no recovery record {record}"),
            )),
            Inspected::Refused(refusal) => Err(refusal),
        }
    }

    /// Removes an orphaned record, recoverable or not: the person asked to. Only
    /// the names a record is made of are removed; anything else stays. Waits a
    /// bounded time for readers of the record, never for a holder.
    pub fn delete(&self, record: RecordId) -> std::result::Result<(), RecoveryRefusal> {
        self.remove_within(record, &CancelToken::new(), READER_WAIT)
    }

    fn remove_within(
        &self,
        record: RecordId,
        cancel: &CancelToken,
        wait: Duration,
    ) -> std::result::Result<(), RecoveryRefusal> {
        let directory = self.record_directory(record);
        match own_lease(&directory, cancel, wait) {
            LeaseState::Held(lease) => {
                remove_record(&directory, lease).map_err(|e| RecoveryRefusal::io(record, e))
            }
            LeaseState::Active => Err(active(record)),
            LeaseState::Readers => Err(busy(record)),
            LeaseState::Cancelled => Err(cancelled(record)),
            LeaseState::Unlockable => Err(unlockable(record)),
            LeaseState::Missing | LeaseState::NotOurs => Err(RecoveryRefusal::new(
                record,
                RefusalKind::NotFound,
                format!("there is no recovery record {record} that FerriteCAD may remove"),
            )),
            LeaseState::Failed(error) => Err(RecoveryRefusal::io(record, error)),
        }
    }

    /// A new, empty record for a live session, its lease held. Orphaned records
    /// with nothing in them are removed first; at the limit no record is made and
    /// nothing else is removed.
    pub fn create_record(&self) -> Result<RecoveryRecord> {
        let mut kept = 0usize;
        for record in self.record_ids()? {
            let directory = self.record_directory(record);
            match empty_record_lease(&directory) {
                Some(LeaseState::Held(lease)) if !has_manifest(&directory) => {
                    // Nothing recoverable: a session that ended before its first
                    // copy, or a crash before its first manifest.
                    if remove_record(&directory, lease).is_err() {
                        kept += 1;
                    }
                }
                // Nothing at all left in it (a removal that raced a reader on a
                // platform that defers deletion): an empty folder of our name.
                Some(LeaseState::Missing) if std::fs::remove_dir(&directory).is_ok() => {}
                _ if std::fs::symlink_metadata(&directory).is_ok() => kept += 1,
                _ => {}
            }
        }
        if kept >= self.limit {
            return Err(CadError::input(format!(
                "the recovery folder already holds {kept} recovery copies, its limit; recover or delete some to protect this document"
            )));
        }
        // A folder another window is sweeping in the instant between our
        // directory and our lease is simply tried again under a new name.
        let mut last = None;
        for _ in 0..3 {
            let record = RecordId::new();
            let directory = self.record_directory(record);
            create_private_directory(&directory)?;
            match create_lease(&directory) {
                Ok(lease) => {
                    return Ok(RecoveryRecord {
                        record,
                        directory,
                        lease,
                        sequence: 0,
                        current: None,
                        confirmed: true,
                        last_order: 0,
                    });
                }
                Err(error) => {
                    let _ = std::fs::remove_file(directory.join(LEASE));
                    let _ = std::fs::remove_dir(&directory);
                    last = Some(error);
                }
            }
        }
        Err(last.unwrap_or_else(|| CadError::io("creating a recovery record", "no attempt")))
    }

    /// Looks at one record under the lock `hold` asks for, verifying it if it has a
    /// copy. The lock comes back with the answer; the caller keeps it (a claim) or
    /// lets it go (a listing).
    fn inspect(&self, record: RecordId, hold: Hold<'_>) -> Inspected {
        let directory = self.record_directory(record);
        match std::fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Inspected::Refused(RecoveryRefusal::new(
                    record,
                    RefusalKind::Damaged,
                    format!("recovery record {record} is not a folder; it was left as it is"),
                ));
            }
            Err(_) => return Inspected::NotOurs,
        }
        let lease = match match hold {
            Hold::Read => read_lease(&directory),
            Hold::Own(cancel, wait) => own_lease(&directory, cancel, wait),
        } {
            LeaseState::Held(lease) => lease,
            LeaseState::Active => return Inspected::Active,
            LeaseState::Readers => return Inspected::Refused(busy(record)),
            LeaseState::Cancelled => return Inspected::Refused(cancelled(record)),
            LeaseState::Unlockable => return Inspected::Refused(unlockable(record)),
            // Being made right now, or not ours: neither offered nor touched.
            LeaseState::Missing | LeaseState::NotOurs if !has_manifest(&directory) => {
                return Inspected::NotOurs;
            }
            LeaseState::Missing | LeaseState::NotOurs => {
                return Inspected::Refused(RecoveryRefusal::new(
                    record,
                    RefusalKind::Damaged,
                    format!(
                        "recovery record {record} has no FerriteCAD lease; it was left as it is"
                    ),
                ));
            }
            LeaseState::Failed(error) => {
                return Inspected::Refused(RecoveryRefusal::io(record, error));
            }
        };
        if !has_manifest(&directory) {
            return Inspected::Empty(lease);
        }
        match verify(record, &directory) {
            Ok(summary) => Inspected::Verified(lease, summary),
            Err(refusal) => Inspected::Refused(refusal),
        }
    }
}

/// How a record is looked at.
#[derive(Clone, Copy)]
enum Hold<'a> {
    /// A listing: a shared lock, so that nothing owns the record meanwhile.
    Read,
    /// A claim or removal: the exclusive lock, waiting this long for readers and
    /// no longer once the token is cancelled.
    Own(&'a CancelToken, Duration),
}

enum Inspected {
    /// Verified under the lock that is handed back.
    Verified(Lease, RecoverySummary),
    Empty(#[allow(dead_code)] Lease),
    Active,
    NotOurs,
    Refused(RecoveryRefusal),
}

fn active(record: RecordId) -> RecoveryRefusal {
    RecoveryRefusal::new(
        record,
        RefusalKind::Active,
        format!(
            "recovery record {record} is held by another FerriteCAD process (a running window, or a recovery or removal in progress); it was left as it is"
        ),
    )
}

fn busy(record: RecordId) -> RecoveryRefusal {
    RecoveryRefusal::new(
        record,
        RefusalKind::Busy,
        format!(
            "recovery record {record} is still being read by another FerriteCAD process; try again in a moment, it was left as it is"
        ),
    )
}

fn cancelled(record: RecordId) -> RecoveryRefusal {
    RecoveryRefusal {
        record,
        kind: RefusalKind::Cancelled,
        error: CadError::Cancelled,
    }
}

fn unlockable(record: RecordId) -> RecoveryRefusal {
    RecoveryRefusal::new(
        record,
        RefusalKind::Unlockable,
        format!(
            "this filesystem cannot lock recovery record {record}, so whether another FerriteCAD uses it cannot be told; it was left as it is"
        ),
    )
}

fn has_manifest(directory: &Path) -> bool {
    std::fs::symlink_metadata(directory.join(MANIFEST)).is_ok()
}

/// Cleanup has no business locking a published record: even a short probe would
/// make a concurrent Recover mistake it for a live owner. A manifest of any kind
/// means keep the record. When none is visible, take the lease, then the caller
/// checks again under that lease before removing anything (publication may race
/// the first check). Ownership and claim locks themselves are unchanged.
fn empty_record_lease(directory: &Path) -> Option<LeaseState> {
    (!has_manifest(directory)).then(|| take_lease(directory))
}

fn create_private_directory(path: &Path) -> Result<()> {
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
        .create(path)
        .map_err(|e| CadError::io("creating a recovery record", e))
}

// --- the lease -------------------------------------------------------------------

/// A held lease: the open, locked `lease` file of one record. Exclusive for an
/// owner, a claim or a removal; shared while a listing reads (never kept past it).
#[derive(Debug)]
struct Lease {
    file: File,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // A concurrent Unix spawn can briefly inherit this open file description
        // before exec closes its descriptor. Closing only our handle would leave
        // the lock held until that unrelated child closes its copy. The lease
        // ends here, so release it explicitly; File still closes on any error.
        let _ = self.file.unlock();
    }
}

enum LeaseState {
    Held(Lease),
    /// The lock is held exclusively (the shared try returned `WouldBlock`).
    Active,
    /// An exclusive try was refused, but a shared one was granted: only readers.
    Readers,
    /// The caller stopped waiting for readers.
    Cancelled,
    Unlockable,
    Missing,
    NotOurs,
    Failed(CadError),
}

fn create_lease(directory: &Path) -> Result<Lease> {
    let path = directory.join(LEASE);
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| CadError::io("creating a recovery lease", e))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Err(CadError::io(
                "locking a new recovery lease",
                "another process locked it first",
            ));
        }
        Err(std::fs::TryLockError::Error(error)) => {
            return Err(CadError::io(
                "locking a new recovery lease (this filesystem must support file locks)",
                error,
            ));
        }
    }
    file.write_all(LEASE_HEADER)
        .and_then(|()| file.sync_all())
        .map_err(|e| CadError::io("writing a recovery lease", e))?;
    Ok(Lease { file })
}

/// One try at the exclusive lease: ownership, claim, removal, empty-orphan cleanup.
fn take_lease(directory: &Path) -> LeaseState {
    lock_lease(directory, false)
}

/// One try at the shared lease: reading a record. Refused only by an exclusive
/// holder, never by another reader.
fn read_lease(directory: &Path) -> LeaseState {
    lock_lease(directory, true)
}

/// The exclusive lease, waiting for readers and for nobody else: an exclusive
/// holder is answered at once ([`LeaseState::Active`]); readers are polled for at
/// most `wait` ([`LeaseState::Readers`] when they stay) and not at all once
/// `cancel` is cancelled ([`LeaseState::Cancelled`]).
fn own_lease(directory: &Path, cancel: &CancelToken, wait: Duration) -> LeaseState {
    let deadline = Instant::now() + wait;
    loop {
        if cancel.is_cancelled() {
            return LeaseState::Cancelled;
        }
        match take_lease(directory) {
            LeaseState::Readers if Instant::now() < deadline => std::thread::sleep(READER_POLL),
            other => return other,
        }
    }
}

/// Whether an exclusive try was refused by readers alone. A shared lock is granted
/// exactly when nobody holds the lock exclusively; it is let go again at once.
fn only_readers(file: &File) -> LeaseState {
    shared_probe(file.try_lock_shared(), || file.unlock())
}

fn shared_probe(
    probe: std::result::Result<(), std::fs::TryLockError>,
    release: impl FnOnce() -> std::io::Result<()>,
) -> LeaseState {
    match probe {
        Ok(()) => match release() {
            Ok(()) => LeaseState::Readers,
            Err(error) => {
                LeaseState::Failed(CadError::io("releasing a recovery reader probe", error))
            }
        },
        Err(std::fs::TryLockError::WouldBlock) => LeaseState::Active,
        Err(std::fs::TryLockError::Error(error))
            if error.kind() == std::io::ErrorKind::Unsupported =>
        {
            LeaseState::Unlockable
        }
        Err(std::fs::TryLockError::Error(error)) => {
            LeaseState::Failed(CadError::io("probing a recovery lease", error))
        }
    }
}

fn lock_lease(directory: &Path, shared: bool) -> LeaseState {
    // All callers, including deletion and empty-orphan cleanup, must reject a
    // linked directory before opening any of its children.
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return LeaseState::NotOurs,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return LeaseState::Missing,
        Err(error) => {
            return LeaseState::Failed(CadError::io("reading a recovery directory", error));
        }
    }
    let path = directory.join(LEASE);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return LeaseState::NotOurs,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return LeaseState::Missing,
        Err(error) => return LeaseState::Failed(CadError::io("reading a recovery lease", error)),
    }
    // A reader opens the lease read-only: looking writes nothing.
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(!shared)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return LeaseState::Missing,
        Err(error) => return LeaseState::Failed(CadError::io("opening a recovery lease", error)),
    };
    match if shared {
        file.try_lock_shared()
    } else {
        file.try_lock()
    } {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return if shared {
                LeaseState::Active
            } else {
                only_readers(&file)
            };
        }
        Err(std::fs::TryLockError::Error(error))
            if error.kind() == std::io::ErrorKind::Unsupported =>
        {
            return LeaseState::Unlockable;
        }
        Err(std::fs::TryLockError::Error(error)) => {
            return LeaseState::Failed(CadError::io("locking a recovery lease", error));
        }
    }
    let mut found = vec![0u8; LEASE_HEADER.len()];
    if file.read_exact(&mut found).is_err() || found != LEASE_HEADER {
        return LeaseState::NotOurs;
    }
    LeaseState::Held(Lease { file })
}

/// Removes the names a record is made of, the lease last, then the directory if
/// nothing else is in it.
fn remove_record(directory: &Path, lease: Lease) -> Result<()> {
    remove_contents(directory)?;
    // Removed while still held, so nobody can take a lease of a record that is
    // going away; the directory goes once the handle is closed.
    let _ = std::fs::remove_file(directory.join(LEASE));
    drop(lease);
    // On Windows a name removed while another process (a scanner, another
    // window listing the folder) has it open is gone only when that handle is
    // closed, a moment later; the empty folder is asked for a few times.
    let mut attempt = 0;
    loop {
        match std::fs::remove_dir(directory) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) if cfg!(windows) && attempt < 20 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(error) => {
                return Err(CadError::io(
                    "removing a recovery record (files that are not FerriteCAD's were left in it)",
                    error,
                ));
            }
        }
    }
}

/// Removes the manifest first (from then on nothing is recoverable from the
/// record), then every copy and partial name. Nothing else.
fn remove_contents(directory: &Path) -> Result<()> {
    match std::fs::remove_file(directory.join(MANIFEST)) {
        Ok(()) => sync_directory(directory)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(CadError::io("removing a recovery manifest", error)),
    }
    let entries =
        std::fs::read_dir(directory).map_err(|e| CadError::io("reading a recovery record", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| CadError::io("reading a recovery record", e))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if is_copy_name(name) || is_partial_name(name) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

fn copy_name(sequence: u64) -> String {
    format!("c{sequence}.fcad")
}

fn is_copy_name(name: &str) -> bool {
    name.strip_prefix('c')
        .and_then(|rest| rest.strip_suffix(".fcad"))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

fn is_partial_name(name: &str) -> bool {
    name == MANIFEST_PARTIAL
        || name
            .strip_prefix('.')
            .and_then(|rest| rest.strip_suffix(".partial"))
            .is_some_and(is_copy_name)
}

fn sync_file(path: &Path) -> Result<()> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| CadError::io("syncing a recovery copy to disk", e))
}

/// Makes a rename or removal in `directory` durable as far as the platform does.
/// Windows has no directory sync; its renames are what they are.
fn sync_directory(directory: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(directory)
            .and_then(|handle| handle.sync_all())
            .map_err(|e| CadError::io("syncing the recovery folder", e))
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
        Ok(())
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// A time a person reads: `2026-10-08 07:12:33 UTC`. No time zone database is
/// consulted, so it is always UTC and says so.
pub fn format_utc(unix_ms: u64) -> String {
    let seconds = unix_ms / 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let of_day = seconds % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

/// One line of text a person reads: no control characters, bounded, never empty.
fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(NAME_LIMIT)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        UNTITLED.to_owned()
    } else {
        trimmed.to_owned()
    }
}

// --- the manifest ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct Manifest {
    record: RecordId,
    sequence: u64,
    file: String,
    bytes: u64,
    blake3: ContentHash,
    document: DocumentId,
    content: ContentHash,
    model: ContentHash,
    written_unix_ms: u64,
    name: String,
}

impl Manifest {
    fn render(&self) -> String {
        format!(
            "{MANIFEST_MAGIC}{MANIFEST_VERSION}\nrecord {}\nsequence {}\nfile {}\nbytes {}\nblake3 {}\ndocument {}\ncontent {}\nmodel {}\nwritten-unix-ms {}\nname {}\n",
            self.record,
            self.sequence,
            self.file,
            self.bytes,
            self.blake3,
            self.document,
            self.content,
            self.model,
            self.written_unix_ms,
            self.name
        )
    }

    fn parse(record: RecordId, text: &str) -> std::result::Result<Self, RecoveryRefusal> {
        let damaged = |why: &str| {
            RecoveryRefusal::new(
                record,
                RefusalKind::Damaged,
                format!("recovery record {record} has a damaged manifest: {why}"),
            )
        };
        let mut lines = text.split('\n');
        let first = lines.next().unwrap_or_default();
        let Some(version) = first.strip_prefix(MANIFEST_MAGIC) else {
            return Err(damaged(
                "it does not start with the FerriteCAD recovery header",
            ));
        };
        if version != MANIFEST_VERSION.to_string() {
            return Err(RecoveryRefusal::new(
                record,
                RefusalKind::UnknownVersion,
                format!(
                    "recovery record {record} was written in recovery format {version:?}, which this FerriteCAD does not read; it was left as it is"
                ),
            ));
        }
        let mut field = |key: &str| -> std::result::Result<String, RecoveryRefusal> {
            let line = lines.next().ok_or_else(|| damaged("it ends early"))?;
            line.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(' '))
                .map(str::to_owned)
                .ok_or_else(|| damaged(&format!("expected {key}")))
        };
        fn number(value: &str) -> Option<u64> {
            (!value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
                .then(|| value.parse().ok())
                .flatten()
        }
        let named = field("record")?;
        let sequence = field("sequence")?;
        let file = field("file")?;
        let bytes = field("bytes")?;
        let blake3 = field("blake3")?;
        let document = field("document")?;
        let content = field("content")?;
        let model = field("model")?;
        let written = field("written-unix-ms")?;
        let name = field("name")?;
        if lines.next() != Some("") || lines.next().is_some() {
            return Err(damaged("it does not end where a manifest ends"));
        }
        let manifest = Self {
            record: named
                .parse()
                .map_err(|_| damaged("its record is not a UUID"))?,
            sequence: number(&sequence).ok_or_else(|| damaged("its sequence is not a number"))?,
            file,
            bytes: number(&bytes).ok_or_else(|| damaged("its length is not a number"))?,
            blake3: blake3
                .parse()
                .map_err(|_| damaged("its BLAKE3 is malformed"))?,
            document: document
                .parse()
                .map_err(|_| damaged("its document id is not a UUID"))?,
            content: content
                .parse()
                .map_err(|_| damaged("its content version is malformed"))?,
            model: model
                .parse()
                .map_err(|_| damaged("its model version is malformed"))?,
            written_unix_ms: number(&written).ok_or_else(|| damaged("its time is not a number"))?,
            name,
        };
        if manifest.record != record {
            return Err(damaged("it names another record"));
        }
        if manifest.file != copy_name(manifest.sequence) {
            return Err(damaged("its copy is not one this record may name"));
        }
        if manifest.name != clean_name(&manifest.name) {
            return Err(damaged("its name is not one line of text"));
        }
        Ok(manifest)
    }
}

/// Reads and verifies a record's manifest and copy. Nothing is followed outside
/// the record and nothing is written.
fn verify(
    record: RecordId,
    directory: &Path,
) -> std::result::Result<RecoverySummary, RecoveryRefusal> {
    #[cfg(test)]
    reading::reached(record);
    let damaged = |why: String| RecoveryRefusal::new(record, RefusalKind::Damaged, why);
    let manifest_path = directory.join(MANIFEST);
    let metadata = std::fs::symlink_metadata(&manifest_path)
        .map_err(|e| RecoveryRefusal::io(record, CadError::io("reading a recovery manifest", e)))?;
    if !metadata.is_file() || metadata.len() > MANIFEST_LIMIT {
        return Err(damaged(format!(
            "recovery record {record} has a manifest that is not a small plain file"
        )));
    }
    let bytes = std::fs::read(&manifest_path)
        .map_err(|e| RecoveryRefusal::io(record, CadError::io("reading a recovery manifest", e)))?;
    let text = String::from_utf8(bytes).map_err(|_| {
        damaged(format!(
            "recovery record {record} has a manifest that is not text"
        ))
    })?;
    let manifest = Manifest::parse(record, &text)?;

    let copy = directory.join(&manifest.file);
    let mismatch = |why: &str| {
        RecoveryRefusal::new(
            record,
            RefusalKind::Mismatch,
            format!("recovery record {record}: {why}; it was left as it is"),
        )
    };
    match std::fs::symlink_metadata(&copy) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return Err(damaged(format!(
                "recovery record {record} names a copy that is not a plain file"
            )));
        }
        Err(_) => {
            return Err(damaged(format!(
                "recovery record {record} names a copy that is missing"
            )));
        }
    }
    let contents = File::open(&copy)
        .map_err(|e| RecoveryRefusal::io(record, CadError::io("reading a recovery copy", e)))?;
    let length = contents
        .metadata()
        .map_err(|e| RecoveryRefusal::io(record, CadError::io("measuring a recovery copy", e)))?
        .len();
    if length != manifest.bytes {
        return Err(mismatch(
            "the copy is not the length its manifest says (incomplete)",
        ));
    }
    let hash = ContentHash::of_reader(contents)
        .map_err(|e| RecoveryRefusal::io(record, CadError::io("hashing a recovery copy", e)))?;
    if hash != manifest.blake3 {
        return Err(mismatch(
            "the copy's bytes are not the ones its manifest names",
        ));
    }
    let document = Document::open_read_only(&copy).map_err(|e| {
        RecoveryRefusal::new(
            record,
            RefusalKind::Damaged,
            format!("recovery record {record} holds a copy that does not open as a document: {e}"),
        )
    })?;
    let id = document.meta().document_id;
    let content = document.content_version();
    let model = document.model_version();
    let _ = document.close();
    let (content, model) = match (content, model) {
        (Ok(content), Ok(model)) => (content, model),
        _ => {
            return Err(damaged(format!(
                "recovery record {record} holds a copy whose content cannot be read"
            )));
        }
    };
    if id != manifest.document {
        return Err(mismatch(
            "the copy is a different document than its manifest names",
        ));
    }
    if content != manifest.content || model != manifest.model {
        return Err(mismatch(
            "the copy's content is not the version its manifest names",
        ));
    }
    Ok(RecoverySummary {
        record,
        name: manifest.name,
        written_unix_ms: manifest.written_unix_ms,
        document_id: manifest.document,
        content: manifest.content,
        model: manifest.model,
        bytes: manifest.bytes,
        sequence: manifest.sequence,
    })
}

// --- a live record -------------------------------------------------------------------

/// The record of one live session: its lease is held for as long as this exists.
///
/// Dropped without [`Self::retire`], the record stays in the folder as an orphan
/// — exactly what a crash leaves — and the next start offers it.
#[derive(Debug)]
pub struct RecoveryRecord {
    record: RecordId,
    directory: PathBuf,
    lease: Lease,
    sequence: u64,
    /// What the published manifest says, when there is one.
    current: Option<Manifest>,
    /// A renamed manifest remains authoritative even if its directory sync fails.
    /// That copy must be kept, but an identical retry must not claim durability.
    confirmed: bool,
    /// The highest request number this record has acted on.
    last_order: u64,
}

impl RecoveryRecord {
    pub fn id(&self) -> RecordId {
        self.record
    }

    /// When the published copy was written, if there is one.
    pub fn written_unix_ms(&self) -> Option<u64> {
        self.current
            .as_ref()
            .map(|manifest| manifest.written_unix_ms)
    }

    /// Publishes `snapshot` as this record's copy. `order` is the caller's request
    /// number: a request no newer than one already acted on is refused with
    /// [`STALE_RECOVERY`] and writes nothing.
    pub fn publish(&mut self, order: u64, snapshot: &Snapshot, name: &str) -> Result<Publication> {
        self.publish_with(order, snapshot, name, &mut NoHooks)
    }

    #[doc(hidden)]
    pub fn publish_with(
        &mut self,
        order: u64,
        snapshot: &Snapshot,
        name: &str,
        hooks: &mut dyn RecoveryHooks,
    ) -> Result<Publication> {
        if order <= self.last_order {
            return Err(CadError::input(STALE_RECOVERY));
        }
        self.last_order = order;
        let name = clean_name(name);
        let version = snapshot.version();
        if let Some(current) = &self.current
            && self.confirmed
            && current.document == version.document_id
            && current.content == version.content
            && current.model == snapshot.model()
        {
            return Ok(Publication::Unchanged {
                sequence: current.sequence,
                written_unix_ms: current.written_unix_ms,
            });
        }
        let sequence = self.sequence + 1;
        let file = copy_name(sequence);
        let partial = self.directory.join(format!(".{file}.partial"));
        let _ = std::fs::remove_file(&partial);
        let mut published = None;
        let outcome = (|| -> Result<Manifest> {
            let source = Document::open_read_only(snapshot.path())?;
            source.snapshot_to(&partial)?;
            source.close()?;
            sync_file(&partial)?;
            hooks.after_copy();
            // What was written is checked as a reader will check it.
            let written = Document::open_read_only(&partial)?;
            let found = (
                written.meta().document_id,
                written.content_version()?,
                written.model_version()?,
            );
            written.close()?;
            if found != (version.document_id, version.content, snapshot.model()) {
                return Err(CadError::io(
                    "verifying a recovery copy",
                    "the copy does not hold the version it was made from",
                ));
            }
            let contents = File::open(&partial)
                .map_err(|e| CadError::io("reading back a recovery copy", e))?;
            let bytes = contents
                .metadata()
                .map_err(|e| CadError::io("measuring a recovery copy", e))?
                .len();
            let blake3 = ContentHash::of_reader(contents)
                .map_err(|e| CadError::io("hashing a recovery copy", e))?;
            let manifest = Manifest {
                record: self.record,
                sequence,
                file: file.clone(),
                bytes,
                blake3,
                document: version.document_id,
                content: version.content,
                model: snapshot.model(),
                written_unix_ms: now_unix_ms(),
                name,
            };
            std::fs::rename(&partial, self.directory.join(&file))
                .map_err(|e| CadError::io("publishing a recovery copy", e))?;
            sync_directory(&self.directory)?;
            hooks.after_copy_published();
            let manifest_partial = self.directory.join(MANIFEST_PARTIAL);
            let mut handle = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&manifest_partial)
                .map_err(|e| CadError::io("writing a recovery manifest", e))?;
            handle
                .write_all(manifest.render().as_bytes())
                .and_then(|()| handle.sync_all())
                .map_err(|e| CadError::io("writing a recovery manifest", e))?;
            drop(handle);
            hooks.after_manifest_written();
            std::fs::rename(&manifest_partial, self.directory.join(MANIFEST))
                .map_err(|e| CadError::io("publishing a recovery manifest", e))?;
            published = Some(manifest.clone());
            hooks.sync_published_manifest(&self.directory)?;
            Ok(manifest)
        })();
        let manifest = match outcome {
            Ok(manifest) => manifest,
            Err(error) => {
                if let Some(manifest) = published {
                    // The manifest already names this file. Deleting it here
                    // would turn a sync failure into loss of the recovery copy.
                    self.sequence = sequence;
                    self.current = Some(manifest);
                    self.confirmed = false;
                    return Err(CadError::io("syncing the published recovery copy", error));
                }
                let _ = std::fs::remove_file(&partial);
                let _ = std::fs::remove_file(self.directory.join(MANIFEST_PARTIAL));
                // A copy renamed into place whose manifest never followed is not
                // named by anything; it is not left lying about.
                if self
                    .current
                    .as_ref()
                    .is_none_or(|current| current.file != file)
                {
                    let _ = std::fs::remove_file(self.directory.join(&file));
                }
                // Said as what it is: the crash copy, not the document or a Save.
                return Err(CadError::io("writing the recovery copy", error));
            }
        };
        self.sequence = sequence;
        self.confirmed = true;
        let previous = self.current.replace(manifest.clone());
        if let Some(previous) = previous
            && previous.file != manifest.file
        {
            let _ = std::fs::remove_file(self.directory.join(previous.file));
        }
        Ok(Publication::Written {
            sequence,
            written_unix_ms: manifest.written_unix_ms,
        })
    }

    /// Nothing is unsaved any more: the copy goes (the manifest first), the record
    /// and its lease stay for the session's next change.
    pub fn clear(&mut self, order: u64) -> Result<()> {
        if order <= self.last_order {
            return Err(CadError::input(STALE_RECOVERY));
        }
        self.last_order = order;
        remove_contents(&self.directory)?;
        self.current = None;
        Ok(())
    }

    /// The session's life is over: the record is removed.
    pub fn retire(self) -> Result<()> {
        remove_record(&self.directory, self.lease)
    }
}

// --- a claimed orphan ------------------------------------------------------------------

/// An orphaned record, verified and held under its lease.
#[derive(Debug)]
pub struct RecoveryClaim {
    root: PathBuf,
    directory: PathBuf,
    lease: Lease,
    summary: RecoverySummary,
}

impl RecoveryClaim {
    pub fn summary(&self) -> &RecoverySummary {
        &self.summary
    }

    fn copy(&self) -> PathBuf {
        self.directory.join(copy_name(self.summary.sequence))
    }

    /// Copies the recorded model to `destination` (absent, in storage the caller
    /// owns) and checks the copy is the recorded version. The record is unchanged.
    pub fn restore_to(&self, destination: &Path) -> Result<()> {
        let source = Document::open_read_only(self.copy())?;
        // The pinned reading is checked once more: the record is held, but the
        // bytes on disk are not trusted twice without looking.
        if source.meta().document_id != self.summary.document_id
            || source.content_version()? != self.summary.content
        {
            let _ = source.close();
            return Err(CadError::input(format!(
                "recovery record {} changed after it was checked; it was not restored",
                self.summary.record
            )));
        }
        source.snapshot_to(destination)?;
        source.close()?;
        let restored = Document::open_read_only(destination)?;
        let found = (
            restored.meta().document_id,
            restored.content_version()?,
            restored.model_version()?,
        );
        restored.close()?;
        if found
            != (
                self.summary.document_id,
                self.summary.content,
                self.summary.model,
            )
        {
            return Err(CadError::io(
                "restoring a recovery copy",
                "the restored file does not hold the recorded version",
            ));
        }
        Ok(())
    }

    /// Writes the recorded model, with every identity as recorded, to a new file
    /// at `destination` by the shared no-clobber publication. The record stays.
    pub fn extract_to(&self, destination: &Path) -> Result<ExtractedRecovery> {
        let destination = std::path::absolute(destination)
            .map_err(|e| CadError::io(format!("resolving {}", destination.display()), e))?;
        if is_inside(&self.root, &destination) {
            return Err(CadError::input(format!(
                "{} is inside the recovery folder; choose a folder of your own",
                destination.display()
            )));
        }
        let advice = "choose a different name";
        if path_entry_exists(&destination)? {
            return Err(CadError::input(format!(
                "{} already exists; {advice}",
                destination.display()
            )));
        }
        let scratch = Temporary::beside(&destination)?;
        self.restore_to(scratch.path())?;
        scratch.publish(&destination, Existing::Keep { advice })?;
        Ok(ExtractedRecovery {
            record: self.summary.record,
            destination,
            document_id: self.summary.document_id,
            content: self.summary.content,
            name: self.summary.name.clone(),
        })
    }

    /// The record becomes the record of the session restored from it: its copy
    /// already is that session's current model, so nothing is written or removed.
    pub fn into_record(self) -> Result<RecoveryRecord> {
        let text = std::fs::read_to_string(self.directory.join(MANIFEST))
            .map_err(|e| CadError::io("reading a recovery manifest", e))?;
        let manifest = Manifest::parse(self.summary.record, &text).map_err(CadError::from)?;
        // Whatever a crash left half-written belongs to this record and goes now.
        if let Ok(entries) = std::fs::read_dir(&self.directory) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if let Some(name) = name.to_str()
                    && (is_partial_name(name) || (is_copy_name(name) && name != manifest.file))
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Ok(RecoveryRecord {
            record: self.summary.record,
            directory: self.directory,
            lease: self.lease,
            sequence: manifest.sequence,
            current: Some(manifest),
            confirmed: true,
            last_order: 0,
        })
    }

    /// Removes the record (the person chose to).
    pub fn discard(self) -> Result<()> {
        remove_record(&self.directory, self.lease)
    }
}

// --- the recorder ----------------------------------------------------------------------

/// What the window may say about the open document's crash copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryStatus {
    /// Nothing is unsaved, so there is nothing to keep.
    Off,
    /// A copy of the newest accepted version is being written.
    Writing,
    /// The newest accepted version's copy was published at this time.
    Written { unix_ms: u64 },
    /// It could not be written. The model and Save are unaffected.
    Failed(String),
}

/// How a session's record ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The person decided (Save, Discard, Quit): the record goes.
    Retire,
    /// Nobody decided (the window is ending on its own): the record stays to be
    /// recovered next time.
    Keep,
}

/// The status a lane shows, written by the worker for that lane only.
type Lane = Arc<Mutex<Shared>>;

enum Request {
    Adopt {
        key: ObjectId,
        claim: RecoveryClaim,
        lane: Lane,
    },
    Checkpoint {
        key: ObjectId,
        order: u64,
        snapshot: Arc<Snapshot>,
        name: String,
        lane: Lane,
    },
    Clear {
        key: ObjectId,
        order: u64,
        lane: Lane,
    },
    End {
        key: ObjectId,
        ending: Ending,
    },
    /// The last lane has gone: finish what came before and end the worker.
    Stop,
}

impl Request {
    fn key(&self) -> Option<ObjectId> {
        match self {
            Self::Adopt { key, .. }
            | Self::Checkpoint { key, .. }
            | Self::Clear { key, .. }
            | Self::End { key, .. } => Some(*key),
            Self::Stop => None,
        }
    }

    fn order(&self) -> Option<u64> {
        match self {
            Self::Checkpoint { order, .. } | Self::Clear { order, .. } => Some(*order),
            Self::Adopt { .. } | Self::End { .. } | Self::Stop => None,
        }
    }
}

#[derive(Debug)]
struct Shared {
    key: Option<ObjectId>,
    requested: u64,
    /// The newest request of the tracked session the worker has finished.
    done: u64,
    status: RecoveryStatus,
}

/// The worker thread, shared by every lane of one window and stopped (after the
/// requests already sent) when the last lane is dropped.
struct Worker {
    sender: Sender<Request>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.sender.send(Request::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One lane of the one worker that writes crash copies for a window. See the
/// module docs.
///
/// [`Self::observe`] is called whenever the lane's session has a new accepted
/// version, has been saved, or has been replaced; it never waits for the disk. A
/// lane tracks one session at a time; [`Self::lane`] makes another lane of the
/// same worker for another open document (§30O), with its own session, order and
/// status.
pub struct RecoveryRecorder {
    sender: Option<Sender<Request>>,
    worker: Option<Arc<Worker>>,
    shared: Lane,
    order: u64,
    tracked: Option<ObjectId>,
}

impl std::fmt::Debug for RecoveryRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryRecorder")
            .field("order", &self.order)
            .field("tracked", &self.tracked)
            .finish_non_exhaustive()
    }
}

impl RecoveryRecorder {
    /// Starts the worker. `notify` is called (on the worker) after each request it
    /// finished, so a window can wake and read [`Self::status`].
    pub fn start(store: RecoveryStore, notify: impl Fn() + Send + 'static) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || run(&store, &receiver, &notify));
        let worker = Arc::new(Worker {
            sender: sender.clone(),
            thread: Some(thread),
        });
        Self::on(sender, worker)
    }

    fn on(sender: Sender<Request>, worker: Arc<Worker>) -> Self {
        Self {
            sender: Some(sender),
            worker: Some(worker),
            shared: Arc::new(Mutex::new(Shared {
                key: None,
                requested: 0,
                done: 0,
                status: RecoveryStatus::Off,
            })),
            order: 0,
            tracked: None,
        }
    }

    /// Another lane of the same worker, tracking nothing yet (§30O: one per tab).
    /// Its session, order and status are its own; observing or ending a session
    /// through it never ends a session another lane tracks.
    pub fn lane(&self) -> Self {
        let (Some(sender), Some(worker)) = (&self.sender, &self.worker) else {
            unreachable!("a lane is stopped only by finish or drop, which consume it")
        };
        Self::on(sender.clone(), Arc::clone(worker))
    }

    fn send(&self, request: Request) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(request);
        }
    }

    /// The window's session is `session` now. A different session than the one
    /// tracked so far ends the old one's record (`Retire`: it was replaced by a
    /// document the person accepted). A dirty session's current version is copied,
    /// a clean one's record is emptied.
    pub fn observe(&mut self, session: &mut DocumentSession) {
        let key = session.id();
        if let Some(old) = self.tracked.filter(|old| *old != key) {
            self.send(Request::End {
                key: old,
                ending: Ending::Retire,
            });
        }
        self.tracked = Some(key);
        if let Some(claim) = session.take_recovery_claim() {
            self.send(Request::Adopt {
                key,
                claim,
                lane: Arc::clone(&self.shared),
            });
        }
        self.order += 1;
        let order = self.order;
        let dirty = session.is_dirty();
        if let Ok(mut shared) = self.shared.lock() {
            shared.key = Some(key);
            shared.requested = order;
            shared.status = if dirty {
                RecoveryStatus::Writing
            } else {
                RecoveryStatus::Off
            };
        }
        if dirty {
            self.send(Request::Checkpoint {
                key,
                order,
                snapshot: session.current(),
                name: session.recovery_name(),
                lane: Arc::clone(&self.shared),
            });
        } else {
            self.send(Request::Clear {
                key,
                order,
                lane: Arc::clone(&self.shared),
            });
        }
    }

    /// The window has no session any more.
    pub fn end(&mut self, ending: Ending) {
        if let Some(key) = self.tracked.take() {
            self.send(Request::End { key, ending });
        }
        if let Ok(mut shared) = self.shared.lock() {
            shared.key = None;
            shared.status = RecoveryStatus::Off;
        }
    }

    pub fn status(&self) -> RecoveryStatus {
        self.shared
            .lock()
            .map_or(RecoveryStatus::Off, |shared| shared.status.clone())
    }

    /// Whether the worker has finished the newest request about the tracked
    /// session (written, emptied, or failed).
    pub fn settled(&self) -> bool {
        self.shared
            .lock()
            .is_ok_and(|shared| shared.key.is_none() || shared.done >= shared.requested)
    }

    /// Ends this lane's session as `ending` says. When it is the last lane of the
    /// worker, also finishes every request already made and stops the worker.
    pub fn finish(mut self, ending: Ending) {
        self.end(ending);
        self.stop();
    }

    fn stop(&mut self) {
        self.sender = None;
        // The last lane joins the worker after everything already sent.
        self.worker = None;
    }
}

impl Drop for RecoveryRecorder {
    /// Nobody decided: whatever is tracked is kept for the next start.
    fn drop(&mut self) {
        if self.sender.is_some() {
            self.end(Ending::Keep);
        }
        self.stop();
    }
}

fn run(store: &RecoveryStore, receiver: &Receiver<Request>, notify: &dyn Fn()) {
    let mut records: HashMap<ObjectId, RecoveryRecord> = HashMap::new();
    let mut ended: HashSet<ObjectId> = HashSet::new();
    'requests: while let Ok(first) = receiver.recv() {
        let mut batch = vec![first];
        batch.extend(receiver.try_iter());
        // Only the newest Checkpoint or Clear of each session is worth doing.
        let mut newest: HashMap<ObjectId, u64> = HashMap::new();
        for request in &batch {
            if let (Some(key), Some(order)) = (request.key(), request.order()) {
                let entry = newest.entry(key).or_insert(order);
                *entry = (*entry).max(order);
            }
        }
        for request in batch {
            let Some(key) = request.key() else {
                // Stop: everything sent before it has been done.
                break 'requests;
            };
            if let Some(order) = request.order()
                && newest.get(&key).is_some_and(|newest| *newest != order)
            {
                continue;
            }
            if ended.contains(&key) {
                // A late request about a session whose record already ended cannot
                // bring it back; a claim arriving for one is let go, kept.
                continue;
            }
            let outcome = match request {
                Request::Adopt { claim, lane, .. } => match claim.into_record() {
                    Ok(record) => {
                        if let Some(old) = records.insert(key, record) {
                            drop(old);
                        }
                        None
                    }
                    Err(error) => Some((lane, 0, Err(error))),
                },
                Request::Checkpoint {
                    order,
                    snapshot,
                    name,
                    lane,
                    ..
                } => {
                    let record = match records.remove(&key) {
                        Some(record) => Ok(record),
                        None => store.create_record(),
                    };
                    let result = record.and_then(|mut record| {
                        let published = record.publish(order, &snapshot, &name);
                        records.insert(key, record);
                        published
                    });
                    Some((lane, order, result.map(|p| Some(p.written_unix_ms()))))
                }
                Request::Clear { order, lane, .. } => {
                    let result = records
                        .get_mut(&key)
                        .map_or(Ok(()), |record| record.clear(order));
                    Some((lane, order, result.map(|()| None)))
                }
                Request::End { ending, .. } => {
                    ended.insert(key);
                    if let Some(record) = records.remove(&key) {
                        match ending {
                            Ending::Retire => {
                                let _ = record.retire();
                            }
                            Ending::Keep => drop(record),
                        }
                    }
                    None
                }
                Request::Stop => break 'requests,
            };
            if let Some((lane, order, result)) = outcome
                && let Ok(mut shared) = lane.lock()
                && shared.key == Some(key)
                && (order == 0 || shared.requested == order)
            {
                shared.done = shared.done.max(order);
                shared.status = match result {
                    Ok(Some(unix_ms)) => RecoveryStatus::Written { unix_ms },
                    Ok(None) => RecoveryStatus::Off,
                    Err(error) => RecoveryStatus::Failed(error.to_string()),
                };
            }
            notify();
        }
    }
    // The window is gone without saying how its records end: they stay.
    drop(records);
}

// --- test-only: holding a reader inside a verification -----------------------------

/// Lets this crate's own tests hold a thread inside the verification of a record,
/// after it took whatever protection it takes and before it reads anything. It is
/// compiled into no build but the unit tests, so nothing outside can reach it.
#[cfg(test)]
mod reading {
    use std::cell::RefCell;

    use super::RecordId;

    type Hook = Box<dyn Fn(RecordId)>;

    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    pub(super) fn on_this_thread(hook: impl Fn(RecordId) + 'static) {
        HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }

    pub(super) fn reached(record: RecordId) {
        HOOK.with(|slot| {
            if let Some(hook) = &*slot.borrow() {
                hook(record);
            }
        });
    }
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;

    use ferritecad_kernel::{CancelToken, OperationContext, mock::MockKernel};

    use super::*;
    use crate::create::{NewDocument, PlateSize};
    use crate::session::HistoryLimits;

    /// A folder holding one orphaned record with a published copy of a new plate.
    fn orphan() -> (tempfile::TempDir, RecoveryStore, RecordId) {
        let root = tempfile::tempdir().expect("recovery folder");
        let scratch = tempfile::tempdir().expect("scratch");
        let session = DocumentSession::create_document_in(
            scratch.path(),
            HistoryLimits::default(),
            NewDocument::SamplePlate(PlateSize {
                width: 70.0,
                depth: 40.0,
                height: 13.0,
            }),
            || -> Result<MockKernel> { Err(CadError::unsupported("no kernel was expected")) },
            &OperationContext::default(),
        )
        .expect("a plate");
        let store = RecoveryStore::open(root.path()).expect("store");
        let mut record = store.create_record().expect("record");
        record
            .publish(1, &session.current(), "plate.fcad")
            .expect("a copy");
        let id = record.id();
        drop(record);
        (root, store, id)
    }

    /// Every file of a record: bytes and modification time, by name. The lease's
    /// bytes are not read (a locked file cannot be read on Windows); its length and
    /// modification time are.
    fn bytes_and_times(
        store: &RecoveryStore,
        record: RecordId,
    ) -> BTreeMap<String, (Vec<u8>, u64, std::time::SystemTime)> {
        std::fs::read_dir(store.record_directory(record))
            .expect("the record")
            .map(|entry| {
                let entry = entry.expect("entry");
                let name = entry.file_name().to_string_lossy().into_owned();
                let metadata = entry.metadata().expect("metadata");
                let bytes = if name == LEASE {
                    Vec::new()
                } else {
                    std::fs::read(entry.path()).expect("bytes")
                };
                (
                    name,
                    (bytes, metadata.len(), metadata.modified().expect("mtime")),
                )
            })
            .collect()
    }

    /// A listing on a thread of its own, held inside the verification of the
    /// record until [`Self::finish`].
    struct HeldListing {
        release: mpsc::Sender<()>,
        thread: std::thread::JoinHandle<Result<RecoveryListing>>,
    }

    impl HeldListing {
        fn start(store: &RecoveryStore) -> Self {
            let (reading_tx, reading) = mpsc::channel();
            let (release, release_rx) = mpsc::channel::<()>();
            let store = store.clone();
            let thread = std::thread::spawn(move || {
                reading::on_this_thread(move |_| {
                    let _ = reading_tx.send(());
                    // Released by the test; the bound only keeps a failed test from
                    // hanging (a dropped sender ends the wait at once).
                    let _ = release_rx.recv_timeout(Duration::from_secs(60));
                });
                store.list()
            });
            reading
                .recv_timeout(Duration::from_secs(60))
                .expect("the listing reached the verification of the record");
            Self { release, thread }
        }

        fn finish(self) -> RecoveryListing {
            self.release.send(()).expect("the listing is waiting");
            self.thread
                .join()
                .expect("listing thread")
                .expect("the listing")
        }
    }

    /// The violation of §30S, deterministically: while one listing is inside the
    /// verification of an orphaned record, another listing and a claim must see
    /// an orphan and a reader, never "a window that is still running".
    #[test]
    fn a_listing_in_progress_neither_hides_the_record_nor_looks_like_its_owner() {
        let (_root, store, record) = orphan();
        let before = bytes_and_times(&store, record);
        let held = HeldListing::start(&store);
        let mut violations = Vec::new();

        let other = store.list().expect("another listing");
        if other.recoverable().count() != 1 || other.active != 0 {
            violations.push(format!(
                "a second listing found recoverable={} active={}",
                other.recoverable().count(),
                other.active
            ));
        }
        let claim = store.claim_within(record, &CancelToken::new(), Duration::from_millis(50));
        match &claim {
            Err(refusal) if refusal.kind.as_str() == "busy" => {}
            Err(refusal) => violations.push(format!(
                "a claim beside a reader was refused as {}: {refusal}",
                refusal.kind.as_str()
            )),
            Ok(_) => violations.push("a claim was granted beside a reader".to_owned()),
        }
        drop(claim);
        assert!(violations.is_empty(), "{violations:#?}");

        let listing = held.finish();
        assert_eq!(listing.recoverable().count(), 1, "{listing:?}");
        assert_eq!(listing.active, 0);
        assert!(store.claim(record).is_ok(), "released: an orphan again");
        assert_eq!(bytes_and_times(&store, record), before, "listing wrote");
    }

    #[test]
    fn cleanup_never_holds_a_published_records_lease() {
        let root = tempfile::tempdir().expect("record directory");
        drop(create_lease(root.path()).expect("lease"));
        // Cleanup must retain even damaged/unknown manifests without examining
        // or locking them; verification belongs to the explicit reader/claim.
        std::fs::write(root.path().join(MANIFEST), b"published").expect("manifest");
        let sweep = empty_record_lease(root.path());
        let claimant = take_lease(root.path());
        assert!(
            matches!(claimant, LeaseState::Held(_)),
            "cleanup must not make an orphan look like a live window"
        );
        assert!(sweep.is_none());
        assert!(matches!(take_lease(root.path()), LeaseState::Active));
        drop(claimant);
        drop(sweep);
        // A genuinely empty orphan still needs an exclusive lease for cleanup.
        std::fs::remove_file(root.path().join(MANIFEST)).expect("remove manifest");
        let empty = empty_record_lease(root.path());
        assert!(matches!(empty, Some(LeaseState::Held(_))));
        assert!(matches!(take_lease(root.path()), LeaseState::Active));
    }

    // A concurrently spawned child can hold a duplicated Unix descriptor until
    // exec closes it. Keep that descriptor alive deterministically: ending our
    // lease must release the lock without waiting for an unrelated child.
    #[cfg(unix)]
    #[test]
    fn dropping_a_lease_unlocks_before_a_duplicated_descriptor_closes() {
        let root = tempfile::tempdir().expect("record directory");
        let lease = create_lease(root.path()).expect("lease");
        let duplicate = lease.file.try_clone().expect("duplicated descriptor");
        assert!(matches!(take_lease(root.path()), LeaseState::Active));
        drop(lease);
        let next = take_lease(root.path());
        assert!(
            matches!(next, LeaseState::Held(_)),
            "a dropped lease must not remain active through a duplicate"
        );
        // Closing the older descriptor must not release the new owner's lock.
        drop(duplicate);
        assert!(matches!(take_lease(root.path()), LeaseState::Active));
        drop(next);
        assert!(matches!(take_lease(root.path()), LeaseState::Held(_)));
    }

    #[test]
    fn utc_dates_are_the_civil_calendar() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(format_utc(951_782_400_000), "2000-02-29 00:00:00 UTC");
        assert_eq!(format_utc(1_791_444_753_000), "2026-10-08 07:32:33 UTC");
    }

    #[test]
    fn names_are_one_bounded_line() {
        assert_eq!(clean_name("plate.fcad"), "plate.fcad");
        assert_eq!(clean_name("a\nb\tc"), "a b c");
        assert_eq!(clean_name("  \n "), UNTITLED);
        assert_eq!(clean_name(&"x".repeat(500)).chars().count(), NAME_LIMIT);
    }

    #[test]
    fn copy_and_partial_names_are_exactly_the_records_own() {
        assert!(is_copy_name("c1.fcad") && is_copy_name("c42.fcad"));
        assert!(
            !is_copy_name("c.fcad") && !is_copy_name("c1.fcad.bak") && !is_copy_name("x1.fcad")
        );
        assert!(is_partial_name(".c3.fcad.partial") && is_partial_name(MANIFEST_PARTIAL));
        assert!(!is_partial_name("notes.partial") && !is_partial_name(".cX.fcad.partial"));
    }

    #[test]
    fn a_manifest_reads_back_and_refuses_every_deviation() {
        let record = RecordId::new();
        let manifest = Manifest {
            record,
            sequence: 3,
            file: copy_name(3),
            bytes: 4096,
            blake3: ContentHash::of_bytes(b"x"),
            document: DocumentId::new(),
            content: ContentHash::of_bytes(b"c"),
            model: ContentHash::of_bytes(b"m"),
            written_unix_ms: 1,
            name: "plate.fcad".to_owned(),
        };
        let text = manifest.render();
        assert_eq!(Manifest::parse(record, &text).expect("reads"), manifest);
        let kind = |text: &str| {
            Manifest::parse(record, text)
                .map(|_| ())
                .expect_err("refused")
                .kind
        };
        assert_eq!(
            kind(&text.replace("RECOVERY 1", "RECOVERY 2")),
            RefusalKind::UnknownVersion
        );
        assert_eq!(
            kind(&text.replace("file c3", "file ../c3")),
            RefusalKind::Damaged
        );
        assert_eq!(
            kind(&text.replace("file c3", "file c4")),
            RefusalKind::Damaged
        );
        assert_eq!(kind(&format!("{text}extra\n")), RefusalKind::Damaged);
        assert_eq!(kind(&text[..text.len() - 1]), RefusalKind::Damaged);
        assert_eq!(
            kind(&text.replace(&record.to_string(), &RecordId::new().to_string())),
            RefusalKind::Damaged
        );
        assert_eq!(kind("not a manifest"), RefusalKind::Damaged);
    }

    /// A claim or removal waits for readers alone, within its bound, and stops when
    /// the caller does; a holder is answered at once; nobody releases what is not
    /// theirs.
    #[test]
    fn a_claim_waits_for_readers_alone_within_its_bound_and_stops_when_cancelled() {
        let (_root, store, record) = orphan();
        let held = HeldListing::start(&store);

        let started = Instant::now();
        let refused = store
            .claim_within(record, &CancelToken::new(), Duration::from_millis(100))
            .map(|_| ())
            .expect_err("a reader is still reading");
        assert_eq!(refused.kind, RefusalKind::Busy, "{refused}");
        assert!(
            started.elapsed() >= Duration::from_millis(100),
            "a reader is waited for, to the end of the bound"
        );
        let refused = store
            .remove_within(record, &CancelToken::new(), Duration::from_millis(100))
            .expect_err("a reader is still reading");
        assert_eq!(refused.kind, RefusalKind::Busy, "{refused}");

        // Cancelled mid-wait: over long before a 30 s bound, nothing taken.
        let cancel = CancelToken::new();
        let stopper = {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                cancel.cancel();
            })
        };
        let started = Instant::now();
        let refused = store
            .claim_within(record, &cancel, Duration::from_secs(30))
            .map(|_| ())
            .expect_err("cancelled");
        assert_eq!(refused.kind, RefusalKind::Cancelled, "{refused}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "cancel is prompt"
        );
        stopper.join().expect("stopper");

        // The reader finished well: the record is still there, whole and claimable.
        assert_eq!(held.finish().recoverable().count(), 1);
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let refused = store
            .claim_within(record, &cancelled, Duration::from_secs(30))
            .map(|_| ())
            .expect_err("cancelled before it began");
        assert_eq!(refused.kind, RefusalKind::Cancelled, "takes nothing");
        let claim = store
            .claim(record)
            .expect("the cancelled attempts took nothing");

        // A holder is not waited for, whatever the bound, and the refusals release
        // nothing of the holder's.
        let started = Instant::now();
        let refused = store
            .claim_within(record, &CancelToken::new(), Duration::from_secs(30))
            .map(|_| ())
            .expect_err("held");
        assert_eq!(refused.kind, RefusalKind::Active, "{refused}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no wait for a holder"
        );
        let refused = store
            .remove_within(record, &CancelToken::new(), Duration::from_secs(30))
            .expect_err("held");
        assert_eq!(refused.kind, RefusalKind::Active, "{refused}");
        assert_eq!(store.list().expect("list").active, 1);
        assert!(matches!(
            take_lease(&store.record_directory(record)),
            LeaseState::Active
        ));
        drop(claim);
        assert!(store.claim(record).is_ok());
    }

    /// A listing is advice: what it showed is verified again by the claim, under the
    /// claim's own lock.
    #[test]
    fn a_stale_listing_is_verified_again_by_the_claim() {
        let (_root, store, record) = orphan();
        assert_eq!(store.list().expect("list").recoverable().count(), 1);
        let copy = store.record_directory(record).join(copy_name(1));
        let mut bytes = std::fs::read(&copy).expect("copy");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x55;
        std::fs::write(&copy, bytes).expect("a bit flipped after the listing");
        let refused = store.claim(record).map(|_| ()).expect_err("verified again");
        assert_eq!(refused.kind, RefusalKind::Mismatch, "{refused}");
        // Looking at it again says the same, and takes nothing.
        let listing = store.list().expect("list");
        assert!(
            matches!(&listing.entries[..], [RecoveryEntry::Refused(r)] if r.kind == RefusalKind::Mismatch)
        );
        assert!(matches!(
            take_lease(&store.record_directory(record)),
            LeaseState::Held(_)
        ));
    }

    /// Readers share the lock; an exclusive holder refuses them; a reader is told
    /// from a holder; and ending a read releases it through a duplicated descriptor
    /// (a concurrent spawn can briefly hold one), as PR #96 did for the owner.
    #[test]
    fn readers_share_and_are_told_from_holders_and_unlock_through_a_duplicate() {
        // A failed probe is not evidence that another process owns the record.
        // Feed the OS-result boundary without needing a broken filesystem, and
        // require that a probe which took no lock releases nothing.
        let failed = shared_probe(
            Err(std::fs::TryLockError::Error(std::io::Error::other(
                "probe transport failed",
            ))),
            || panic!("a failed probe must not unlock"),
        );
        assert!(
            matches!(failed, LeaseState::Failed(CadError::Io { ref source, .. }) if source.to_string() == "probe transport failed"),
            "an OS failure must not claim that another process owns the record"
        );
        assert!(matches!(
            shared_probe(
                Err(std::fs::TryLockError::Error(
                    std::io::ErrorKind::Unsupported.into()
                )),
                || panic!("an unsupported probe must not unlock"),
            ),
            LeaseState::Unlockable
        ));
        assert!(matches!(
            shared_probe(Err(std::fs::TryLockError::WouldBlock), || panic!(
                "a refused probe must not unlock"
            )),
            LeaseState::Active
        ));
        assert!(matches!(
            shared_probe(Ok(()), || Err(std::io::Error::other("unlock failed"))),
            LeaseState::Failed(_)
        ));
        let root = tempfile::tempdir().expect("record directory");
        drop(create_lease(root.path()).expect("lease"));
        let first = read_lease(root.path());
        let second = read_lease(root.path());
        assert!(matches!(
            (&first, &second),
            (LeaseState::Held(_), LeaseState::Held(_))
        ));
        assert!(matches!(take_lease(root.path()), LeaseState::Readers));
        drop(second);
        assert!(matches!(take_lease(root.path()), LeaseState::Readers));
        #[cfg(unix)]
        {
            let LeaseState::Held(read) = first else {
                panic!("a reader holds the lease");
            };
            let duplicate = read.file.try_clone().expect("duplicated descriptor");
            drop(read);
            let owner = take_lease(root.path());
            assert!(
                matches!(owner, LeaseState::Held(_)),
                "a dropped read must not remain a reader through a duplicate"
            );
            assert!(matches!(read_lease(root.path()), LeaseState::Active));
            drop(duplicate);
            assert!(matches!(take_lease(root.path()), LeaseState::Active));
            drop(owner);
        }
        #[cfg(not(unix))]
        drop(first);
        assert!(matches!(take_lease(root.path()), LeaseState::Held(_)));
    }

    /// While an owner (an adopted claim) publishes copy after copy, listings never
    /// see its changing files: only an orphan's whole copy of a version that was
    /// really published, or nothing but a count.
    #[test]
    fn listings_racing_an_owner_never_see_a_record_change_under_them() {
        let (root, store, record) = orphan();
        let scratch = tempfile::tempdir().expect("scratch");
        let versions: Vec<_> = (0..8)
            .map(|n| {
                DocumentSession::create_document_in(
                    scratch.path(),
                    HistoryLimits::default(),
                    NewDocument::SamplePlate(PlateSize {
                        width: 70.0,
                        depth: 40.0,
                        height: 14.0 + f64::from(n),
                    }),
                    || -> Result<MockKernel> { Err(CadError::unsupported("no kernel")) },
                    &OperationContext::default(),
                )
                .expect("a plate")
            })
            .collect();
        let mut known: Vec<ContentHash> = store
            .list()
            .expect("list")
            .recoverable()
            .map(|summary| summary.content)
            .collect();
        known.extend(versions.iter().map(|v| v.current().version().content));

        let mut owner = store
            .claim(record)
            .expect("claimed")
            .into_record()
            .expect("adopted");
        let done = std::sync::Arc::new(AtomicBool::new(false));
        let readers: Vec<_> = (0..3)
            .map(|_| {
                let (store, done, known) = (store.clone(), done.clone(), known.clone());
                std::thread::spawn(move || {
                    let (mut listings, mut wrong) = (0usize, Vec::new());
                    while !done.load(Ordering::SeqCst) || listings == 0 {
                        for entry in store.list().expect("a listing").entries {
                            match entry {
                                RecoveryEntry::Recoverable(summary)
                                    if known.contains(&summary.content) => {}
                                other => wrong.push(format!("{other:?}")),
                            }
                        }
                        listings += 1;
                    }
                    (listings, wrong)
                })
            })
            .collect();

        for (order, version) in versions.iter().enumerate() {
            owner
                .publish(order as u64 + 1, &version.current(), "plate.fcad")
                .expect("published beside the readers");
        }
        drop(owner);
        done.store(true, Ordering::SeqCst);
        for reader in readers {
            let (listings, wrong) = reader.join().expect("reader");
            assert!(listings > 0 && wrong.is_empty(), "{wrong:#?}");
        }
        let last = versions[7].current().version().content;
        let found: Vec<_> = store.list().expect("list").recoverable().cloned().collect();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].content, last,
            "the newest whole copy, once it is an orphan"
        );
        drop(root);
    }

    // --- two independent processes ---------------------------------------------------

    const ROLE: &str = "FCAD_30S_ROLE";
    const FOLDER: &str = "FCAD_30S_FOLDER";
    const RECORD: &str = "FCAD_30S_RECORD";
    const SAY: &str = "FCAD-30S-CHILD ";

    /// The child process: this test binary, started by name, doing one role against
    /// a folder and saying what happened. Ignored, so it runs only when asked.
    #[test]
    #[ignore = "run as a child process by the §30S process tests"]
    fn child_process_30s() {
        use std::io::{BufRead as _, Write as _};
        let Ok(role) = std::env::var(ROLE) else {
            return;
        };
        let folder = std::env::var_os(FOLDER).expect("a folder");
        let store = RecoveryStore::at(Path::new(&folder)).expect("store");
        let record = || -> RecordId {
            std::env::var(RECORD)
                .expect("a record")
                .parse()
                .expect("a record id")
        };
        let say = |line: &str| {
            println!("{SAY}{line}");
            let _ = std::io::stdout().flush();
        };
        // The parent lets the child go by writing a line (or by dying).
        let until_released = || {
            let _ = std::io::stdin().lock().read_line(&mut String::new());
        };
        match role.as_str() {
            "reader" => {
                reading::on_this_thread(move |_| {
                    say("READING");
                    until_released();
                });
                let listing = store.list().expect("the listing");
                say(&format!(
                    "LISTED recoverable={} active={}",
                    listing.recoverable().count(),
                    listing.active
                ));
            }
            "claimer" => match store.claim(record()) {
                Ok(claim) => {
                    say("CLAIMED");
                    until_released();
                    drop(claim);
                }
                Err(refusal) => say(&format!("REFUSED {}", refusal.kind.as_str())),
            },
            "deleter" => match store.delete(record()) {
                Ok(()) => say("DELETED"),
                Err(refusal) => say(&format!("REFUSED {}", refusal.kind.as_str())),
            },
            other => panic!("unknown role {other}"),
        }
    }

    /// A child process in one role. Killed and reaped when dropped unfinished, so a
    /// failing test leaves none behind.
    struct Child {
        process: std::process::Child,
        said: mpsc::Receiver<String>,
        stdin: Option<std::process::ChildStdin>,
    }

    impl Child {
        fn start(role: &str, folder: &Path, record: Option<RecordId>) -> Self {
            use std::io::BufRead as _;
            use std::process::{Command, Stdio};
            let mut command = Command::new(std::env::current_exe().expect("this test binary"));
            command
                .args([
                    "recovery::tests::child_process_30s",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(ROLE, role)
                .env(FOLDER, folder)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            if let Some(record) = record {
                command.env(RECORD, record.to_string());
            }
            let mut process = command.spawn().expect("a child process");
            let stdout = process.stdout.take().expect("stdout");
            let stdin = process.stdin.take();
            let (sender, said) = mpsc::channel();
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    // libtest may have started the line with the test's name.
                    if let Some(at) = line.find(SAY)
                        && sender.send(line[at + SAY.len()..].to_owned()).is_err()
                    {
                        break;
                    }
                }
            });
            Self {
                process,
                said,
                stdin,
            }
        }

        /// The child's next sentence; the child is killed if it says nothing.
        fn says(&self) -> String {
            self.said
                .recv_timeout(Duration::from_secs(60))
                .expect("the child said what it was started to say")
        }

        /// Lets a waiting child go on, expects its last sentence, and waits for it
        /// to end well.
        fn release_and_finish_after(mut self, last: &str) {
            drop(self.stdin.take());
            assert_eq!(self.says(), last);
            self.release_and_finish();
        }

        /// Lets a waiting child go on, and waits for it to end well.
        fn release_and_finish(mut self) {
            drop(self.stdin.take());
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            loop {
                if let Some(status) = self.process.try_wait().expect("waiting") {
                    assert!(status.success(), "the child ended with {status}");
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the child never ended"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for Child {
        fn drop(&mut self) {
            if self.process.try_wait().ok().flatten().is_none() {
                let _ = self.process.kill();
                let _ = self.process.wait();
            }
        }
    }

    /// The violation across processes: a second process reading an orphan must not
    /// hide it from a third, nor make it look like a window to a claim; and two
    /// claims, a claim and a removal still exclude each other.
    #[test]
    fn processes_that_read_claim_and_remove_an_orphan_exclude_only_the_owners() {
        let (root, store, record) = orphan();
        let before = bytes_and_times(&store, record);
        let mut violations = Vec::new();

        // One process is reading the record; this one lists and claims meanwhile.
        let reader = Child::start("reader", root.path(), None);
        assert_eq!(reader.says(), "READING");
        let beside = store.list().expect("a listing beside a reading process");
        if beside.recoverable().count() != 1 || beside.active != 0 {
            violations.push(format!(
                "a listing beside a reading process found recoverable={} active={}",
                beside.recoverable().count(),
                beside.active
            ));
        }
        match store.claim_within(record, &CancelToken::new(), Duration::from_millis(50)) {
            Err(refusal) if refusal.kind.as_str() == "busy" => {}
            Err(refusal) => violations.push(format!(
                "a claim beside a reading process was refused as {}",
                refusal.kind.as_str()
            )),
            Ok(_) => violations.push("a claim was granted beside a reader".to_owned()),
        }
        assert!(violations.is_empty(), "{violations:#?}");
        reader.release_and_finish_after("LISTED recoverable=1 active=0");

        // Two claims: one wins in this process; another process and a removal are
        // refused as held; letting go frees it for exactly one more.
        let claim = store
            .claim(record)
            .expect("an orphan once the reader is gone");
        let loser = Child::start("claimer", root.path(), Some(record));
        assert_eq!(loser.says(), "REFUSED active");
        loser.release_and_finish();
        let deleter = Child::start("deleter", root.path(), Some(record));
        assert_eq!(deleter.says(), "REFUSED active");
        deleter.release_and_finish();
        assert_eq!(
            bytes_and_times(&store, record),
            before,
            "the record changed"
        );
        drop(claim);
        let winner = Child::start("claimer", root.path(), Some(record));
        assert_eq!(winner.says(), "CLAIMED");
        assert_eq!(
            store.claim(record).map(|_| ()).expect_err("held").kind,
            RefusalKind::Active
        );
        assert_eq!(store.list().expect("list").active, 1);
        winner.release_and_finish();
        assert_eq!(
            bytes_and_times(&store, record),
            before,
            "the record changed"
        );

        // Removal is exclusive too, and final.
        let deleter = Child::start("deleter", root.path(), Some(record));
        assert_eq!(deleter.says(), "DELETED");
        deleter.release_and_finish();
        assert_eq!(
            store.claim(record).map(|_| ()).expect_err("gone").kind,
            RefusalKind::NotFound
        );
    }
}
