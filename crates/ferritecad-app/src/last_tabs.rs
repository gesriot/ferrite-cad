// SPDX-License-Identifier: MIT
//! §30R: the saved files of the last window that went through Quit to the end, and
//! which one was shown — one small descriptor in a per-user folder.
//!
//! The descriptor names files, never their contents: no model, Undo, form, camera
//! or selection. What a later start reopens is each file as it is on disk then.
//!
//! # One whole list or the previous one
//!
//! A publication writes the new list to a temporary file of its own
//! (`.last-window.<uuid>.partial`, created exclusively, so it never opens or
//! removes another process's), syncs it and renames it over `last-window`. A
//! reader therefore sees the previous complete list or the new complete one. Two
//! windows that quit one after the other each publish their whole list; the last
//! rename stays. Whether a rename also survives a power cut depends on the
//! platform honouring `fsync`, and nothing here claims it does.
//!
//! # Paths are kept exactly
//!
//! A path is stored as the platform's own units in hexadecimal — bytes on Unix,
//! UTF-16 code units on Windows — so Unicode, spaces, a newline in a name or a name
//! that is not UTF-8 (or not valid UTF-16) comes back exactly as it was, and no
//! lossy conversion stands between the window's file and the one reopened. Only
//! absolute paths are stored. A path is only ever handed to Open: nothing here
//! runs, fetches, imports or migrates anything.

use std::fmt;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use ferritecad_types::ObjectId;

use crate::tabs::MAX_TABS;

/// Names a folder other than the per-user default (tests, the window recipe).
pub(crate) const TABS_DIR_ENV: &str = "FERRITECAD_TABS_DIR";
/// The descriptor's name inside the folder.
pub(crate) const DESCRIPTOR: &str = "last-window";
const PARTIAL_PREFIX: &str = ".last-window.";
const PARTIAL_SUFFIX: &str = ".partial";
const MAGIC: &str = "FERRITECAD-LAST-TABS ";
const VERSION: &str = "1";
#[cfg(unix)]
const PLATFORM: &str = "unix";
#[cfg(windows)]
const PLATFORM: &str = "windows";
/// The longest path stored, in hexadecimal digits of its native units: 64 KiB of
/// Unix bytes, or 32 Ki UTF-16 units (Windows' own limit is 32 767).
pub(crate) const MAX_PATH_HEX: usize = 128 * 1024;
/// The four header lines are at most this long together.
const HEADER_LIMIT: usize = 64;
/// How many times a reading starts again when another window's publication
/// replaced the list between looking at it and opening it.
const READ_ATTEMPTS: usize = 3;
/// The largest descriptor read: anything bigger is refused before it is read.
pub(crate) const MAX_BYTES: u64 =
    (HEADER_LIMIT + MAX_TABS * ("path ".len() + MAX_PATH_HEX + 1)) as u64;

/// The saved files of one window, in the tab row's order, and the index of the
/// one that was shown (none when the shown tab was Untitled).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LastTabs {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) active: Option<usize>,
}

/// Why the descriptor was not read or not published. Every case leaves the
/// descriptor on disk as it was.
#[derive(Debug)]
pub(crate) enum LastTabsError {
    /// There is no folder for it: no home folder (or `LOCALAPPDATA`) is known.
    Unavailable(String),
    /// Written by a FerriteCAD that knows another version of the format.
    UnknownVersion(String),
    /// Not a complete, well-formed descriptor of this platform.
    Damaged(String),
    /// Bigger than any descriptor can be; refused before it was read.
    TooLarge(u64),
    /// The folder or the descriptor is a link, or not a plain folder and file.
    Foreign(String),
    /// A list this format cannot hold (a relative or too long path, too many).
    Refused(String),
    Io {
        context: String,
        source: std::io::Error,
    },
}

impl LastTabsError {
    /// One word for the kind, as the window and the record name it.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Unavailable(_) => "unavailable",
            Self::UnknownVersion(_) => "unknown-version",
            Self::Damaged(_) => "damaged",
            Self::TooLarge(_) => "too-large",
            Self::Foreign(_) => "foreign",
            Self::Refused(_) => "refused",
            Self::Io { .. } => "io",
        }
    }

    fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

impl fmt::Display for LastTabsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message)
            | Self::UnknownVersion(message)
            | Self::Damaged(message)
            | Self::Foreign(message)
            | Self::Refused(message) => f.write_str(message),
            Self::TooLarge(bytes) => write!(
                f,
                "the list is {bytes} bytes, more than the {MAX_BYTES} a list can be"
            ),
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for LastTabsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

type Outcome<T> = std::result::Result<T, LastTabsError>;

/// The folder that holds the descriptor.
#[derive(Debug, Clone)]
pub(crate) struct Folder {
    root: PathBuf,
}

impl Folder {
    /// [`TABS_DIR_ENV`] when set, otherwise FerriteCAD's per-user state folder
    /// (`Tabs`, or `tabs` on Linux), beside the recovery folder. Never the system
    /// temporary directory. Nothing is created.
    pub(crate) fn default_place() -> Outcome<Self> {
        let root = ferritecad_jobs::per_user_state_folder(
            TABS_DIR_ENV,
            "folder for the list of open files",
            "Tabs",
            "tabs",
        )
        .map_err(|error| LastTabsError::Unavailable(error.to_string()))?;
        Self::at(&root)
    }

    /// The folder at `root`. Nothing is created.
    pub(crate) fn at(root: &Path) -> Outcome<Self> {
        let root = std::path::absolute(root)
            .map_err(|error| LastTabsError::io(format!("resolving {}", root.display()), error))?;
        Ok(Self { root })
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn descriptor(&self) -> PathBuf {
        self.root.join(DESCRIPTOR)
    }

    /// The last published list. `None` when there is none (no folder, no
    /// descriptor). Reads only: no folder is made, nothing is written or removed.
    pub(crate) fn read(&self) -> Outcome<Option<LastTabs>> {
        if !plain(&self.root, true)? {
            return Ok(None);
        }
        let path = self.descriptor();
        for _ in 0..READ_ATTEMPTS {
            let Some(listed) = metadata(&path)? else {
                return Ok(None);
            };
            if listed.file_type().is_symlink() || !listed.is_file() {
                return Err(LastTabsError::Foreign(format!(
                    "{} is a link or not a plain file; it was not followed",
                    path.display()
                )));
            }
            if listed.len() > MAX_BYTES {
                return Err(LastTabsError::TooLarge(listed.len()));
            }
            let file = File::open(&path)
                .map_err(|error| LastTabsError::io(format!("opening {}", path.display()), error))?;
            let opened = file
                .metadata()
                .map_err(|error| LastTabsError::io(format!("reading {}", path.display()), error))?;
            // What the name holds now, looked at after opening: the file opened
            // must be that plain file, never one reached through a link put in its
            // place. Another window's publication may have replaced the list since
            // it was looked at; that is a newer whole list, and reading starts again.
            let now = metadata(&path)?;
            if !now.is_some_and(|now| {
                !now.file_type().is_symlink() && now.is_file() && same_entry(&now, &opened)
            }) {
                continue;
            }
            if opened.len() > MAX_BYTES {
                return Err(LastTabsError::TooLarge(opened.len()));
            }
            let mut bytes = Vec::new();
            file.take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| LastTabsError::io(format!("reading {}", path.display()), error))?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err(LastTabsError::TooLarge(bytes.len() as u64));
            }
            return parse(&bytes).map(Some);
        }
        Err(LastTabsError::Foreign(format!(
            "{} kept changing while it was being opened; it was not read",
            path.display()
        )))
    }

    /// Replaces the descriptor with `set`, whole. A list the format cannot hold is
    /// refused before anything is written; any failure leaves the previous
    /// descriptor as it was and removes this publication's own temporary file.
    pub(crate) fn publish(&self, set: &LastTabs) -> Outcome<()> {
        let bytes = serialize(set)?;
        self.make_folder()?;
        // Only replace a descriptor this build understands. A refusal at startup
        // must not turn into a destructive overwrite at Quit (including a list
        // from a newer build, foreign text, or a damaged/incomplete list).
        self.read()?;
        let destination = self.descriptor();
        let partial = self.root.join(format!(
            "{PARTIAL_PREFIX}{}{PARTIAL_SUFFIX}",
            ObjectId::new()
        ));
        replace_with_new(&partial, &destination, &bytes)?;
        // The list is in place. A folder that cannot be synced only weakens what a
        // power cut might keep, which is not claimed; it is said, not hidden.
        if let Err(error) = sync_folder(&self.root) {
            eprintln!(
                "ferritecad: the list of open files was replaced, but its folder could not \
                 be synced: {error}"
            );
        }
        Ok(())
    }

    /// The folder, made `0700` where the platform has modes when it is missing.
    /// An existing link or file in its place is refused, never followed.
    fn make_folder(&self) -> Outcome<()> {
        if plain(&self.root, false)? {
            return Ok(());
        }
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&self.root).map_err(|error| {
            LastTabsError::io(format!("making the folder {}", self.root.display()), error)
        })?;
        if plain(&self.root, false)? {
            Ok(())
        } else {
            Err(LastTabsError::Foreign(format!(
                "{} went away as it was made",
                self.root.display()
            )))
        }
    }
}

/// Whether `root` is a plain folder (false: nothing is there); a link or any
/// other entry is refused. `reading` words the refusal for a read.
fn plain(root: &Path, reading: bool) -> Outcome<bool> {
    match metadata(root)? {
        None => Ok(false),
        Some(found) if !found.file_type().is_symlink() && found.is_dir() => Ok(true),
        Some(_) => Err(LastTabsError::Foreign(format!(
            "{} is a link or not a folder; it was {}",
            root.display(),
            if reading {
                "not followed"
            } else {
                "left as it is"
            }
        ))),
    }
}

/// The entry at `path` itself (a link is not followed); `None` when there is none.
fn metadata(path: &Path) -> Outcome<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(found) => Ok(Some(found)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(LastTabsError::io(
            format!("looking at {}", path.display()),
            error,
        )),
    }
}

/// Whether the file opened is the entry the name holds.
#[cfg(unix)]
fn same_entry(listed: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    listed.dev() == opened.dev() && listed.ino() == opened.ino() && opened.is_file()
}

/// Windows has no stable file identity in `std`: the entry the name holds is
/// checked to be a plain file (not a link) after opening, and the file opened to
/// be one.
#[cfg(windows)]
fn same_entry(_listed: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    opened.is_file()
}

/// Publishes an exclusively created file; cleans up only after owning it.
fn replace_with_new(partial: &Path, destination: &Path, bytes: &[u8]) -> Outcome<()> {
    write_new(partial, bytes)?;
    if let Err(error) = std::fs::rename(partial, destination) {
        let error = LastTabsError::io(format!("replacing {}", destination.display()), error);
        return Err(with_cleanup(error, partial));
    }
    Ok(())
}

/// Creates `path` (never an existing file), writes `bytes` and syncs them.
fn write_new(path: &Path, bytes: &[u8]) -> Outcome<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .map_err(|error| LastTabsError::io(format!("creating {}", path.display()), error))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    // Close before removing on Windows; create_new succeeded, so this file is ours.
    drop(file);
    written.map_err(|error| {
        with_cleanup(
            LastTabsError::io(format!("writing {}", path.display()), error),
            path,
        )
    })
}

/// Removes this publication's own temporary file after `error`; a removal that
/// fails is added to the error, and nothing else is touched.
fn with_cleanup(error: LastTabsError, partial: &Path) -> LastTabsError {
    match std::fs::remove_file(partial) {
        Ok(()) => error,
        Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => error,
        Err(cleanup) => LastTabsError::io(
            format!("{error}; its temporary file {} remains", partial.display()),
            cleanup,
        ),
    }
}

#[cfg(unix)]
fn sync_folder(root: &Path) -> std::io::Result<()> {
    File::open(root)?.sync_all()
}

/// A folder cannot be opened for syncing through `std` on Windows; its rename
/// is the platform's own.
#[cfg(windows)]
fn sync_folder(_root: &Path) -> std::io::Result<()> {
    Ok(())
}

// --- the format ------------------------------------------------------------------
//
// FERRITECAD-LAST-TABS 1
// platform unix|windows
// count <n>            0 ..= MAX_TABS
// active <i>|none      i < n
// path <hex>           exactly n lines, lowercase hexadecimal native units
//
// Every line ends with a newline and nothing follows the last one.

/// A path's native units in lowercase hexadecimal.
#[cfg(unix)]
fn encode(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(windows)]
fn encode(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt as _;
    path.as_os_str()
        .encode_wide()
        .map(|unit| format!("{unit:04x}"))
        .collect()
}

/// The digits of one native unit.
#[cfg(unix)]
const UNIT_DIGITS: usize = 2;
#[cfg(windows)]
const UNIT_DIGITS: usize = 4;

/// The path `hex` names; `None` for anything that is not a well-formed one.
fn decode(hex: &str) -> Option<PathBuf> {
    if hex.is_empty()
        || !hex.len().is_multiple_of(UNIT_DIGITS)
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let units = (0..hex.len())
        .step_by(UNIT_DIGITS)
        .map(|at| u16::from_str_radix(&hex[at..at + UNIT_DIGITS], 16).ok())
        .collect::<Option<Vec<u16>>>()?;
    if units.contains(&0) {
        return None;
    }
    #[cfg(unix)]
    let native = {
        use std::os::unix::ffi::OsStringExt as _;
        let bytes = units
            .into_iter()
            .map(|unit| u8::try_from(unit).ok())
            .collect::<Option<Vec<u8>>>()?;
        std::ffi::OsString::from_vec(bytes)
    };
    #[cfg(windows)]
    let native = {
        use std::os::windows::ffi::OsStringExt as _;
        std::ffi::OsString::from_wide(&units)
    };
    let path = PathBuf::from(native);
    path.is_absolute().then_some(path)
}

fn serialize(set: &LastTabs) -> Outcome<Vec<u8>> {
    if set.paths.len() > MAX_TABS {
        return Err(LastTabsError::Refused(format!(
            "{} files are more than the {MAX_TABS} a window keeps open",
            set.paths.len()
        )));
    }
    let active = match set.active {
        None => "none".to_owned(),
        Some(index) if index < set.paths.len() => index.to_string(),
        Some(index) => {
            return Err(LastTabsError::Refused(format!(
                "the shown file {index} is not one of the {} listed",
                set.paths.len()
            )));
        }
    };
    let mut text = format!(
        "{MAGIC}{VERSION}\nplatform {PLATFORM}\ncount {}\nactive {active}\n",
        set.paths.len()
    );
    for path in &set.paths {
        let hex = encode(path);
        if !path.is_absolute() || hex.is_empty() || hex.len() > MAX_PATH_HEX {
            return Err(LastTabsError::Refused(format!(
                "{} cannot be kept in the list (it is not absolute, or longer than {} \
                 units)",
                path.display(),
                MAX_PATH_HEX / UNIT_DIGITS
            )));
        }
        text.push_str("path ");
        text.push_str(&hex);
        text.push('\n');
    }
    Ok(text.into_bytes())
}

/// A decimal written the one way this format writes it: digits, no sign, no
/// leading zero.
fn number(text: &str) -> Option<usize> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    canonical.then(|| text.parse().ok()).flatten()
}

fn parse(bytes: &[u8]) -> Outcome<LastTabs> {
    let damaged = |why: &str| LastTabsError::Damaged(format!("the list is damaged: {why}"));
    let text = std::str::from_utf8(bytes).map_err(|_| damaged("it is not text"))?;
    let first = text.split('\n').next().unwrap_or_default();
    let Some(version) = first.strip_prefix(MAGIC) else {
        return Err(if MAGIC.starts_with(text) {
            damaged("it is cut short")
        } else {
            damaged("it is not a FerriteCAD list of open files")
        });
    };
    if version != VERSION && text.len() > first.len() {
        return Err(match number(version) {
            Some(_) => LastTabsError::UnknownVersion(format!(
                "the list was written in version {version} of its format; this FerriteCAD \
                 reads version {VERSION}"
            )),
            None => damaged("its version is not a number"),
        });
    }
    let Some(body) = text.strip_suffix('\n') else {
        return Err(damaged("it is cut short (its last line is incomplete)"));
    };
    let mut lines = body.split('\n').skip(1);
    let mut field = |name: &str| {
        let line = lines.next().ok_or_else(|| damaged("it is cut short"))?;
        line.strip_prefix(name)
            .and_then(|rest| rest.strip_prefix(' '))
            .ok_or_else(|| damaged(&format!("`{name}` was expected")))
    };
    let platform = field("platform")?;
    if platform != PLATFORM {
        return Err(damaged(&format!(
            "it was written on {platform}, and this is {PLATFORM}"
        )));
    }
    let count = number(field("count")?)
        .filter(|count| *count <= MAX_TABS)
        .ok_or_else(|| damaged(&format!("its count is not 0 to {MAX_TABS}")))?;
    let active = match field("active")? {
        "none" => None,
        index => Some(
            number(index)
                .filter(|index| *index < count)
                .ok_or_else(|| damaged("its shown file is not one of those listed"))?,
        ),
    };
    let mut paths = Vec::with_capacity(count);
    for _ in 0..count {
        let hex = field("path")?;
        if hex.len() > MAX_PATH_HEX {
            return Err(damaged("a path is longer than any the list keeps"));
        }
        paths.push(decode(hex).ok_or_else(|| damaged("a path is not a well-formed absolute one"))?);
    }
    if lines.next().is_some() {
        return Err(damaged("lines follow the last path"));
    }
    Ok(LastTabs { paths, active })
}

#[cfg(test)]
#[path = "last_tabs/tests.rs"]
mod tests;
