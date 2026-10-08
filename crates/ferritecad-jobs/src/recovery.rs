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
//!   versions to; the event loop never copies, hashes or `fsync`s anything.
//!
//! # Ownership is a lock, not a guess
//!
//! The owner of a record holds an exclusive advisory lock on its `lease` for the
//! record's whole life, and the operating system lets go of it when the process
//! ends however it ends. A lease that can be locked (and carries the header) is
//! an orphan; one that cannot belongs to a live window and is neither offered nor
//! touched. A claim keeps holding the lock, so two processes never restore or
//! extract one record at once. No PID, host name or age is consulted.
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
use std::time::{SystemTime, UNIX_EPOCH};

use ferritecad_document::Document;
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

/// Why a publication was not written: a newer one already was.
pub const STALE_RECOVERY: &str =
    "a newer recovery copy of this document was already requested; this older one was not written";

/// The per-user recovery folder: [`RECOVERY_DIR_ENV`] when set, otherwise the
/// platform's place for an application's own state. Never the system temporary
/// directory.
pub fn default_recovery_root() -> Result<PathBuf> {
    if let Some(chosen) = std::env::var_os(RECOVERY_DIR_ENV).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(chosen));
    }
    let home = || {
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                CadError::input(format!(
                    "no home folder is known, so there is no recovery folder; set {RECOVERY_DIR_ENV}"
                ))
            })
    };
    if cfg!(target_os = "macos") {
        return Ok(home()?
            .join("Library")
            .join("Application Support")
            .join("FerriteCAD")
            .join("Recovery"));
    }
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                CadError::input(format!(
                    "LOCALAPPDATA is not set, so there is no recovery folder; set {RECOVERY_DIR_ENV}"
                ))
            })?;
        return Ok(PathBuf::from(local).join("FerriteCAD").join("Recovery"));
    }
    if let Some(state) = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Ok(state.join("ferritecad").join("recovery"));
    }
    Ok(home()?
        .join(".local")
        .join("state")
        .join("ferritecad")
        .join("recovery"))
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
pub enum RefusalKind {
    /// A running FerriteCAD holds it.
    Active,
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

/// What [`RecoveryStore::list`] found. Records held by running windows are only
/// counted.
#[derive(Debug, Default)]
pub struct RecoveryListing {
    /// Newest recoverable first, then refusals.
    pub entries: Vec<RecoveryEntry>,
    /// Records a running FerriteCAD holds.
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

    /// What is in the folder. Reads and verifies every orphaned record; changes
    /// nothing and keeps no lease.
    pub fn list(&self) -> Result<RecoveryListing> {
        let mut listing = RecoveryListing::default();
        for record in self.record_ids()? {
            match self.inspect(record) {
                Inspected::Claimed(claim) => {
                    listing
                        .entries
                        .push(RecoveryEntry::Recoverable(claim.summary.clone()));
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
    /// claim is dropped, finished, or handed to a recovered session.
    pub fn claim(&self, record: RecordId) -> std::result::Result<RecoveryClaim, RecoveryRefusal> {
        match self.inspect(record) {
            Inspected::Claimed(claim) => Ok(claim),
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
    /// the names a record is made of are removed; anything else stays.
    pub fn delete(&self, record: RecordId) -> std::result::Result<(), RecoveryRefusal> {
        let directory = self.record_directory(record);
        match take_lease(&directory) {
            LeaseState::Held(lease) => {
                remove_record(&directory, lease).map_err(|e| RecoveryRefusal::io(record, e))
            }
            LeaseState::Active => Err(active(record)),
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
            match take_lease(&directory) {
                LeaseState::Held(lease) if !has_manifest(&directory) => {
                    // Nothing recoverable: a session that ended before its first
                    // copy, or a crash before its first manifest.
                    if remove_record(&directory, lease).is_err() {
                        kept += 1;
                    }
                }
                // Nothing at all left in it (a removal that raced a reader on a
                // platform that defers deletion): an empty folder of our name.
                LeaseState::Missing if std::fs::remove_dir(&directory).is_ok() => {}
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

    fn inspect(&self, record: RecordId) -> Inspected {
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
        let lease = match take_lease(&directory) {
            LeaseState::Held(lease) => lease,
            LeaseState::Active => return Inspected::Active,
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
            Ok(summary) => Inspected::Claimed(RecoveryClaim {
                root: self.root.clone(),
                directory,
                lease,
                summary,
            }),
            Err(refusal) => Inspected::Refused(refusal),
        }
    }
}

enum Inspected {
    Claimed(RecoveryClaim),
    Empty(#[allow(dead_code)] Lease),
    Active,
    NotOurs,
    Refused(RecoveryRefusal),
}

fn active(record: RecordId) -> RecoveryRefusal {
    RecoveryRefusal::new(
        record,
        RefusalKind::Active,
        format!("recovery record {record} belongs to a FerriteCAD window that is still running"),
    )
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

/// A held lease: the open, locked `lease` file of one record.
#[derive(Debug)]
struct Lease {
    /// Never read: holding the open, locked file *is* the lease.
    #[allow(dead_code)]
    file: File,
}

enum LeaseState {
    Held(Lease),
    Active,
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

fn take_lease(directory: &Path) -> LeaseState {
    let path = directory.join(LEASE);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return LeaseState::NotOurs,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return LeaseState::Missing,
        Err(error) => return LeaseState::Failed(CadError::io("reading a recovery lease", error)),
    }
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return LeaseState::Missing,
        Err(error) => return LeaseState::Failed(CadError::io("opening a recovery lease", error)),
    };
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return LeaseState::Active,
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
    let contents = std::fs::read(&copy)
        .map_err(|e| RecoveryRefusal::io(record, CadError::io("reading a recovery copy", e)))?;
    if contents.len() as u64 != manifest.bytes {
        return Err(mismatch(
            "the copy is not the length its manifest says (incomplete)",
        ));
    }
    if ContentHash::of_bytes(&contents) != manifest.blake3 {
        return Err(mismatch(
            "the copy's bytes are not the ones its manifest names",
        ));
    }
    drop(contents);
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
            let contents = std::fs::read(&partial)
                .map_err(|e| CadError::io("reading back a recovery copy", e))?;
            let manifest = Manifest {
                record: self.record,
                sequence,
                file: file.clone(),
                bytes: contents.len() as u64,
                blake3: ContentHash::of_bytes(&contents),
                document: version.document_id,
                content: version.content,
                model: snapshot.model(),
                written_unix_ms: now_unix_ms(),
                name,
            };
            drop(contents);
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
            sync_directory(&self.directory)?;
            Ok(manifest)
        })();
        let manifest = match outcome {
            Ok(manifest) => manifest,
            Err(error) => {
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

enum Request {
    Adopt {
        key: ObjectId,
        claim: RecoveryClaim,
    },
    Checkpoint {
        key: ObjectId,
        order: u64,
        snapshot: Arc<Snapshot>,
        name: String,
    },
    Clear {
        key: ObjectId,
        order: u64,
    },
    End {
        key: ObjectId,
        ending: Ending,
    },
}

impl Request {
    fn key(&self) -> ObjectId {
        match self {
            Self::Adopt { key, .. }
            | Self::Checkpoint { key, .. }
            | Self::Clear { key, .. }
            | Self::End { key, .. } => *key,
        }
    }

    fn order(&self) -> Option<u64> {
        match self {
            Self::Checkpoint { order, .. } | Self::Clear { order, .. } => Some(*order),
            Self::Adopt { .. } | Self::End { .. } => None,
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

/// The one worker that writes crash copies for a window. See the module docs.
///
/// [`Self::observe`] is called whenever the session has a new accepted version,
/// has been saved, or has been replaced; it never waits for the disk.
pub struct RecoveryRecorder {
    sender: Option<Sender<Request>>,
    worker: Option<JoinHandle<()>>,
    shared: Arc<Mutex<Shared>>,
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
        let shared = Arc::new(Mutex::new(Shared {
            key: None,
            requested: 0,
            done: 0,
            status: RecoveryStatus::Off,
        }));
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::spawn(move || run(&store, &receiver, &worker_shared, &notify));
        Self {
            sender: Some(sender),
            worker: Some(worker),
            shared,
            order: 0,
            tracked: None,
        }
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
            self.send(Request::Adopt { key, claim });
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
            });
        } else {
            self.send(Request::Clear { key, order });
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

    /// Finishes every request already made and stops the worker. Records still
    /// tracked end as `ending` says.
    pub fn finish(mut self, ending: Ending) {
        self.end(ending);
        self.stop();
    }

    fn stop(&mut self) {
        self.sender = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
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

fn run(
    store: &RecoveryStore,
    receiver: &Receiver<Request>,
    shared: &Mutex<Shared>,
    notify: &dyn Fn(),
) {
    let mut records: HashMap<ObjectId, RecoveryRecord> = HashMap::new();
    let mut ended: HashSet<ObjectId> = HashSet::new();
    while let Ok(first) = receiver.recv() {
        let mut batch = vec![first];
        batch.extend(receiver.try_iter());
        // Only the newest Checkpoint or Clear of each session is worth doing.
        let mut newest: HashMap<ObjectId, u64> = HashMap::new();
        for request in &batch {
            if let Some(order) = request.order() {
                let entry = newest.entry(request.key()).or_insert(order);
                *entry = (*entry).max(order);
            }
        }
        for request in batch {
            let key = request.key();
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
                Request::Adopt { claim, .. } => match claim.into_record() {
                    Ok(record) => {
                        if let Some(old) = records.insert(key, record) {
                            drop(old);
                        }
                        None
                    }
                    Err(error) => Some((0, Err(error))),
                },
                Request::Checkpoint {
                    order,
                    snapshot,
                    name,
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
                    Some((order, result.map(|p| Some(p.written_unix_ms()))))
                }
                Request::Clear { order, .. } => {
                    let result = records
                        .get_mut(&key)
                        .map_or(Ok(()), |record| record.clear(order));
                    Some((order, result.map(|()| None)))
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
            };
            if let Some((order, result)) = outcome
                && let Ok(mut shared) = shared.lock()
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
