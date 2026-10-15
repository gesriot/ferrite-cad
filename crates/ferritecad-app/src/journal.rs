// SPDX-License-Identifier: MIT
//! §31H: the launch journal — what a window said, kept in a per-user folder for a
//! person who started it from Finder and had no terminal to read stderr in.
//!
//! The window route of `main` runs inside [`launch`]: the journal is opened, with
//! the panic hook, before the event loop is asked for, and how the route ended is
//! written after it. The application's diagnostics go through [`report`], which
//! prints them on stderr exactly as before and then writes them here. Nothing else
//! writes: no frame, pointer event, model, geometry, request or environment, and
//! nothing is ever sent anywhere. Policy, platforms and limits:
//! `docs/viewer-launch-journal.md`.
//!
//! # One instance, its own files
//!
//! Every process that opens a journal is an *instance* named by a fresh UUIDv7 (a
//! PID alone is reused, and is shared by two journals in one process). Its files
//! all start with `viewer-<UTC start>-<pid>-<uuid>`: a `.lock` it creates
//! exclusively and holds locked for its whole life, and segments `.<n>.log`, each
//! created exclusively and headed with the instance's facts. The operating system
//! lets go of the lock when the process ends however it ends, so a lock that can be
//! taken is a finished instance; one that cannot is running and is not touched or
//! waited for. No PID, age or host name is consulted.
//!
//! # Bounded
//!
//! A record is one line of at most [`RECORD_LIMIT`] bytes; a segment at most
//! [`SEGMENT_LIMIT`] bytes, newlines included, because a record is written only
//! when it fits whole. An instance keeps its first segment (the start and the
//! earliest errors) and its current one; a starting window removes the files of
//! all but the [`KEEP_FINISHED`] newest finished instances. Writes go straight to
//! the file under one mutex, with no queue; the panic hook only tries that mutex,
//! a bounded number of times. Any failure stops the journal for the process, said
//! once on stderr, and the window goes on.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ferritecad_types::{CadError, ObjectId, Result};

/// Names a folder other than the per-user default (tests, the window recipe).
pub(crate) const DIR_ENV: &str = "FERRITECAD_LOG_DIR";
/// The most bytes one segment holds, newlines included: 16 MiB.
pub(crate) const SEGMENT_LIMIT: u64 = 16 * 1024 * 1024;
/// The most bytes one record takes, its newline included: 16 KiB, far above any
/// message this application writes and a thousandth of a segment.
pub(crate) const RECORD_LIMIT: usize = 16 * 1024;
/// How many finished instances a starting window keeps the files of.
pub(crate) const KEEP_FINISHED: usize = 4;
/// What ends a record whose message was cut.
pub(crate) const TRUNCATED: &str = " [truncated]";
/// The first bytes of every segment, before `segment=<n>`.
pub(crate) const SEGMENT_HEADER: &str = "FERRITECAD-LAUNCH-JOURNAL 1 ";
/// The first bytes of every lock file, before the instance it names.
const LOCK_HEADER: &str = "FERRITECAD-LAUNCH-JOURNAL-LOCK 1 ";
const PREFIX: &str = "viewer-";
const LOCK_SUFFIX: &str = ".lock";
const LOG_SUFFIX: &str = ".log";
/// What of a record is the time, the kind and the message: the rest is room for
/// [`TRUNCATED`] and the newline.
const CONTENT_LIMIT: usize = RECORD_LIMIT - TRUNCATED.len() - 1;
/// A panic tries the journal this often, [`PANIC_WAIT`] apart, and then leaves it.
const PANIC_TRIES: u32 = 100;
const PANIC_WAIT: Duration = Duration::from_millis(1);
/// Fresh names an opening tries when a starting window locked its new lock file
/// first (it was judging the folder at that moment).
const OPEN_ATTEMPTS: usize = 3;

/// This process's journal, once the window route has opened one.
static JOURNAL: OnceLock<Mutex<Journal>> = OnceLock::new();

/// Runs the window route under the journal: opened (with the panic hook) before
/// `window` runs, so a window that cannot be made is written too, and how the
/// route ended written after it. `window`'s answer is returned as it was. A
/// journal that cannot be opened is said once on stderr; the window runs anyway.
pub(crate) fn launch(window: impl FnOnce() -> Result<()>) -> Result<()> {
    open_for_this_process();
    let outcome = window();
    if let Some(journal) = JOURNAL.get() {
        let line = match &outcome {
            Ok(()) => record("exit", format_args!("the event loop ended")),
            Err(error) => record("exit", format_args!("the window failed: {}", Causes(error))),
        };
        hold(journal).append(&line);
    }
    outcome
}

/// Says one diagnostic of the application: on stderr, the same bytes
/// `eprintln!` printed, then in the journal when this process has one.
///
/// Stderr comes first, so a journal that cannot be written never costs the
/// message. Unlike `eprintln!`, a stderr that cannot be written is not a panic.
pub(crate) fn report(message: fmt::Arguments<'_>) {
    let _ = writeln!(std::io::stderr(), "{message}");
    if let Some(journal) = JOURNAL.get() {
        let line = record("error", message);
        hold(journal).append(&line);
    }
}

fn open_for_this_process() {
    match default_folder().and_then(|folder| Journal::open(&folder)) {
        Ok(journal) => {
            if JOURNAL.set(Mutex::new(journal)).is_ok() {
                install_panic_hook();
            }
        }
        // Said once, on stderr only; the window goes on without a journal.
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "ferritecad: no launch journal: {error}");
        }
    }
}

/// The journal for an ordinary caller, who waits for one record at most. Nothing
/// panics while holding it, and a poisoned lock still guards a whole journal.
fn hold(journal: &Mutex<Journal>) -> MutexGuard<'_, Journal> {
    journal.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Writes a panic before the hook that was there runs, which then runs whatever
/// happened here; unwinding or abort is that hook's and the runtime's, unchanged.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(journal) = JOURNAL.get() {
            let payload = info.payload();
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("Box<dyn Any>");
            let line = match info.location() {
                Some(location) => record("panic", format_args!("at {location}: {message}")),
                None => record("panic", format_args!("{message}")),
            };
            append_if_free(journal, &line);
        }
        previous(info);
    }));
}

/// Appends `line` unless the journal stays busy for [`PANIC_TRIES`] tries
/// [`PANIC_WAIT`] apart (about a tenth of a second): a panic never waits for the
/// writer, which may be the panicking thread itself. Whether it was written.
fn append_if_free(journal: &Mutex<Journal>, line: &str) -> bool {
    for _ in 0..PANIC_TRIES {
        match journal.try_lock() {
            Ok(mut journal) => {
                journal.append(line);
                return true;
            }
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                poisoned.into_inner().append(line);
                return true;
            }
            Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(PANIC_WAIT),
        }
    }
    false
}

/// [`DIR_ENV`] when it is set and not empty; otherwise `~/Library/Logs/FerriteCAD`
/// on macOS (the per-user folder macOS keeps application logs in),
/// and FerriteCAD's per-user state folder elsewhere (`%LOCALAPPDATA%\FerriteCAD\Logs`,
/// `$XDG_STATE_HOME/ferritecad/logs`). Never the checkout, the bundle, a model's
/// folder or the system temporary directory. Nothing is created here.
fn default_folder() -> Result<PathBuf> {
    if cfg!(target_os = "macos") && std::env::var_os(DIR_ENV).is_none_or(|v| v.is_empty()) {
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                CadError::input(format!(
                    "no home folder is known, so there is no folder for the launch journal; \
                     set {DIR_ENV}"
                ))
            })?;
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join("FerriteCAD"));
    }
    ferritecad_jobs::per_user_state_folder(DIR_ENV, "folder for the launch journal", "Logs", "logs")
}

/// One instance's journal: its held lock and its current segment.
#[derive(Debug)]
pub(crate) struct Journal {
    folder: PathBuf,
    /// `viewer-<stamp>-<pid>-<uuid>`: how every file of this instance starts.
    stem: String,
    id: ObjectId,
    /// What every segment header says after its number.
    facts: String,
    /// Locked for the life of the instance: the proof that it is running.
    _lock: File,
    current: File,
    number: u32,
    /// Bytes in the current segment.
    len: u64,
    /// A write, a rotation or a removal failed: nothing more is written.
    stopped: bool,
}

impl Journal {
    /// A new instance in `folder` (made when it is missing; a link or a file in its
    /// place is refused), with its lock held and its first segment headed; then the
    /// files of old finished instances are removed (see [`sweep`]).
    pub(crate) fn open(folder: &Path) -> Result<Self> {
        let folder = std::path::absolute(folder)
            .map_err(|error| failed(format!("resolving {}", folder.display()), error))?;
        make_folder(&folder)?;
        for _ in 0..OPEN_ATTEMPTS {
            if let Some(journal) = Self::start(&folder)? {
                sweep(&folder, journal.id);
                return Ok(journal);
            }
        }
        Err(CadError::input(format!(
            "another process kept locking this window's new lock file in {} first",
            folder.display()
        )))
    }

    /// One try at a fresh instance. `None` when another process locked its new lock
    /// file before it could, which is then removed again.
    fn start(folder: &Path) -> Result<Option<Self>> {
        let id = ObjectId::new();
        let started = now_ms();
        let pid = std::process::id();
        let stem = format!("{PREFIX}{}-{pid}-{id}", name_stamp(started));
        let lock_path = folder.join(format!("{stem}{LOCK_SUFFIX}"));
        let mut lock = create_new(&lock_path)
            .map_err(|error| failed(format!("creating {}", lock_path.display()), error))?;
        let held = match lock.try_lock() {
            Ok(()) => lock
                .write_all(lock_header(pid, id).as_bytes())
                .map_err(|error| failed(format!("writing {}", lock_path.display()), error)),
            Err(std::fs::TryLockError::WouldBlock) => {
                abandon(lock, &lock_path);
                return Ok(None);
            }
            Err(std::fs::TryLockError::Error(error)) => Err(failed(
                format!(
                    "locking {} (the folder must support file locks)",
                    lock_path.display()
                ),
                error,
            )),
        };
        if let Err(error) = held {
            abandon(lock, &lock_path);
            return Err(error);
        }
        let facts = format!(
            "ferritecad-viewer {} os={} arch={} pid={pid} instance={id} started={}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            ferritecad_jobs::format_utc(started)
        );
        match new_segment(folder, &stem, 1, &facts) {
            Ok((current, len)) => Ok(Some(Self {
                folder: folder.to_path_buf(),
                stem,
                id,
                facts,
                _lock: lock,
                current,
                number: 1,
                len,
                stopped: false,
            })),
            Err(error) => {
                abandon(lock, &lock_path);
                Err(error)
            }
        }
    }

    /// Writes one record (see [`record`]) whole: into the current segment when it
    /// fits within [`SEGMENT_LIMIT`], otherwise into the next one. A line longer
    /// than [`RECORD_LIMIT`] is not a record and is dropped. After any failure
    /// nothing more is written, so the files never grow past their bounds.
    pub(crate) fn append(&mut self, line: &str) {
        if self.stopped || line.len() > RECORD_LIMIT {
            return;
        }
        let bytes = line.len() as u64;
        if self.len + bytes > SEGMENT_LIMIT
            && let Err(error) = self.rotate()
        {
            return self.stop(&error);
        }
        match self.current.write_all(line.as_bytes()) {
            Ok(()) => self.len += bytes,
            Err(error) => {
                let error = failed(
                    format!("writing {}", self.segment(self.number).display()),
                    error,
                );
                self.stop(&error);
            }
        }
    }

    /// The next segment is created and headed first; then the full one is removed,
    /// unless it is the first, which the instance keeps.
    fn rotate(&mut self) -> Result<()> {
        let next = self
            .number
            .checked_add(1)
            .ok_or_else(|| CadError::input("the launch journal has no further segment number"))?;
        let (file, len) = new_segment(&self.folder, &self.stem, next, &self.facts)?;
        let full = std::mem::replace(&mut self.number, next);
        // Closed before it is removed: Windows keeps an open file's name.
        drop(std::mem::replace(&mut self.current, file));
        self.len = len;
        if full != 1 {
            let path = self.segment(full);
            std::fs::remove_file(&path)
                .map_err(|error| failed(format!("removing {}", path.display()), error))?;
        }
        Ok(())
    }

    /// Nothing more is written by this instance. Said once, on stderr only: the
    /// journal is what failed, so it cannot be told there.
    fn stop(&mut self, error: &CadError) {
        self.stopped = true;
        let _ = writeln!(
            std::io::stderr(),
            "ferritecad: the launch journal stopped: {error}"
        );
    }

    fn segment(&self, number: u32) -> PathBuf {
        segment_path(&self.folder, &self.stem, number)
    }
}

/// One record: `<UTC time> <kind> <message>\n`, at most [`RECORD_LIMIT`] bytes.
///
/// Control characters other than tab are escaped (`\n`, `\u{1b}`), so a record is
/// one line. A longer message is cut at a character — never inside a UTF-8
/// sequence or an escape — and ends with [`TRUNCATED`]. Formatting stops at the
/// limit, so the work and the memory are bounded whatever `message` holds.
pub(crate) fn record(kind: &str, message: fmt::Arguments<'_>) -> String {
    let mut line = Line::default();
    // An error from either is the limit being reached, which `full` records.
    let _ = write!(line, "{} {kind} ", ferritecad_jobs::format_utc(now_ms()));
    let _ = line.write_fmt(message);
    let Line { mut text, full } = line;
    if full {
        text.push_str(TRUNCATED);
    }
    text.push('\n');
    text
}

/// An error and every source under it, `: ` between them: the reason a window
/// could not be made is often only in the source.
#[derive(Debug)]
struct Causes<'a>(&'a CadError);

impl fmt::Display for Causes<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut source = std::error::Error::source(self.0);
        while let Some(cause) = source {
            write!(f, ": {cause}")?;
            source = cause.source();
        }
        Ok(())
    }
}

/// A record being formatted, refusing everything once it has reached its room.
#[derive(Debug, Default)]
struct Line {
    text: String,
    full: bool,
}

impl fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.full {
            return Err(fmt::Error);
        }
        for c in s.chars() {
            let escaped = c.is_control() && c != '\t';
            let width = if escaped {
                c.escape_default().len()
            } else {
                c.len_utf8()
            };
            if self.text.len() + width > CONTENT_LIMIT {
                self.full = true;
                return Err(fmt::Error);
            }
            if escaped {
                self.text.extend(c.escape_default());
            } else {
                self.text.push(c);
            }
        }
        Ok(())
    }
}

/// What a name in the folder is, when it is exactly one of this journal's.
#[derive(Debug)]
struct Named {
    id: ObjectId,
    pid: u32,
    stem: String,
    part: Part,
}

#[derive(Debug)]
enum Part {
    Lock,
    Segment(u32),
}

/// `viewer-YYYYMMDDTHHMMSSZ-<pid>-<uuidv7>.lock` or `….<n>.log`, with canonical
/// numbers and the UUID as this journal writes it; anything else is `None`.
fn parse(name: &str) -> Option<Named> {
    let (stem, part) = match name.strip_suffix(LOCK_SUFFIX) {
        Some(stem) => (stem, Part::Lock),
        None => {
            let (stem, number) = name.strip_suffix(LOG_SUFFIX)?.rsplit_once('.')?;
            (stem, Part::Segment(canonical(number)?))
        }
    };
    let (stamp, rest) = stem.strip_prefix(PREFIX)?.split_once('-')?;
    let (pid, uuid) = rest.split_once('-')?;
    let stamped = stamp.len() == 16
        && stamp.bytes().enumerate().all(|(at, byte)| match at {
            8 => byte == b'T',
            15 => byte == b'Z',
            _ => byte.is_ascii_digit(),
        });
    let pid = canonical(pid)?;
    let id = ObjectId::from_str(uuid).ok()?;
    (stamped && id.to_string() == uuid).then(|| Named {
        id,
        pid,
        stem: stem.to_owned(),
        part,
    })
}

/// A positive decimal number exactly as this journal writes it: no sign, no
/// leading zero, within `u32`.
fn canonical(text: &str) -> Option<u32> {
    let value: u32 = text.parse().ok()?;
    (value > 0 && value.to_string() == text).then_some(value)
}

/// An instance found in the folder: its name and what of it is there.
#[derive(Debug)]
struct Found {
    stem: String,
    pid: u32,
    lock: bool,
    segments: Vec<u32>,
    /// Two stems with one UUID: not something this journal wrote.
    clash: bool,
}

/// Removes the files of finished instances other than the [`KEEP_FINISHED`]
/// newest, once, when an instance has started. Never `own`'s, never a running
/// instance's (its lock is only tried, never waited for), never a name this journal
/// did not write. A failure leaves the files as they are and is deliberately not
/// said or retried: the next window looks again.
fn sweep(folder: &Path, own: ObjectId) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut found: BTreeMap<ObjectId, Found> = BTreeMap::new();
    for entry in entries.flatten() {
        let Some(named) = entry.file_name().to_str().and_then(parse) else {
            continue;
        };
        if named.id == own {
            continue;
        }
        let instance = found.entry(named.id).or_insert_with(|| Found {
            stem: named.stem.clone(),
            pid: named.pid,
            lock: false,
            segments: Vec::new(),
            clash: false,
        });
        instance.clash |= instance.stem != named.stem;
        match named.part {
            Part::Lock => instance.lock = true,
            Part::Segment(number) => instance.segments.push(number),
        }
    }
    let mut kept = 0;
    // Newest first: a UUIDv7 orders by creation time.
    for (id, instance) in found.iter().rev() {
        if !instance.lock || instance.clash {
            continue;
        }
        let Some(held) = finished(folder, *id, instance) else {
            continue;
        };
        if !instance.segments.is_empty() {
            kept += 1;
            if kept <= KEEP_FINISHED {
                continue;
            }
        }
        let mut removed = true;
        for &number in &instance.segments {
            let path = segment_path(folder, &instance.stem, number);
            let result = headed(&path, number).and_then(|ours| {
                if ours {
                    std::fs::remove_file(&path)
                } else {
                    Ok(())
                }
            });
            if let Err(error) = result {
                // Another sweeper may already have removed a stale directory
                // entry. Every other refusal must leave the lock for a retry.
                removed &= error.kind() == std::io::ErrorKind::NotFound;
            }
        }
        // Last, and only after successful cleanup: without this lock, leftover
        // segments could never be recognised as a finished instance again.
        if removed {
            let _ = std::fs::remove_file(folder.join(format!("{}{LOCK_SUFFIX}", instance.stem)));
        }
        drop(held);
    }
}

/// The lock file of a finished instance, locked now: a plain file whose lock is
/// free and whose header names this instance. `None` for a running instance (its
/// lock is held) and for anything that cannot be judged.
fn finished(folder: &Path, id: ObjectId, instance: &Found) -> Option<File> {
    let path = folder.join(format!("{}{LOCK_SUFFIX}", instance.stem));
    if !plain_file(&path) {
        return None;
    }
    let file = File::open(&path).ok()?;
    file.try_lock().ok()?;
    let expected = lock_header(instance.pid, id);
    let mut head = Vec::new();
    (&file)
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut head)
        .ok()?;
    (head == expected.as_bytes()).then_some(file)
}

/// Whether `path` is a plain file that starts as segment `number` of this journal.
fn headed(path: &Path, number: u32) -> std::io::Result<bool> {
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Ok(false);
    }
    let expected = format!("{SEGMENT_HEADER}segment={number} ");
    let mut head = Vec::new();
    File::open(path)?
        .take(expected.len() as u64)
        .read_to_end(&mut head)?;
    Ok(head == expected.as_bytes())
}

/// Whether `path` itself is a plain file (a link is not followed).
fn plain_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|found| found.is_file())
}

fn lock_header(pid: u32, id: ObjectId) -> String {
    format!("{LOCK_HEADER}pid={pid} instance={id}\n")
}

fn segment_path(folder: &Path, stem: &str, number: u32) -> PathBuf {
    folder.join(format!("{stem}.{number}{LOG_SUFFIX}"))
}

/// Segment `number`, created exclusively (never an existing file, nor a link in its
/// place) and headed. With its length.
fn new_segment(folder: &Path, stem: &str, number: u32, facts: &str) -> Result<(File, u64)> {
    let path = segment_path(folder, stem, number);
    let mut file =
        create_new(&path).map_err(|error| failed(format!("creating {}", path.display()), error))?;
    let header = format!("{SEGMENT_HEADER}segment={number} {facts}\n");
    match file.write_all(header.as_bytes()) {
        Ok(()) => Ok((file, header.len() as u64)),
        Err(error) => {
            abandon(file, &path);
            Err(failed(format!("writing {}", path.display()), error))
        }
    }
}

/// A file of this instance, made exclusively: never an existing file, nor a link
/// in its place. Readable by its owner only, where the platform has modes.
fn create_new(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// Closes and removes a file this instance created and does not keep. A removal
/// that fails leaves it, empty or without its header, which no sweep removes.
fn abandon(file: File, path: &Path) {
    drop(file);
    let _ = std::fs::remove_file(path);
}

/// `folder`, made (`0700` where the platform has modes) when it is missing. A link
/// or anything but a folder in its place is refused, never followed.
fn make_folder(folder: &Path) -> Result<()> {
    if plain_folder(folder)? {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(folder)
        .map_err(|error| failed(format!("making the folder {}", folder.display()), error))?;
    if plain_folder(folder)? {
        Ok(())
    } else {
        Err(CadError::input(format!(
            "{} went away as it was made",
            folder.display()
        )))
    }
}

/// Whether `folder` is a plain folder (false: nothing is there).
fn plain_folder(folder: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(folder) {
        Ok(found) if found.is_dir() => Ok(true),
        Ok(_) => Err(CadError::input(format!(
            "{} is a link or not a folder; it was not used",
            folder.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(failed(format!("looking at {}", folder.display()), error)),
    }
}

/// An I/O failure whose words carry the operating system's reason as well.
fn failed(context: String, error: std::io::Error) -> CadError {
    CadError::io(format!("{context}: {error}"), error)
}

/// `YYYYMMDDTHHMMSSZ` for a file name, from the same UTC reading the records use.
fn name_stamp(unix_ms: u64) -> String {
    let digits: String = ferritecad_jobs::format_utc(unix_ms)
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    let (date, time) = digits.split_at(8);
    format!("{date}T{time}Z")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
#[path = "journal/tests.rs"]
mod tests;
