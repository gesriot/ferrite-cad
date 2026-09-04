// SPDX-License-Identifier: MIT
//! Putting a finished file where the user asked for it, and not before.
//!
//! Everything that produces a file — the STL export, the STEP import and the
//! FBX export both interfaces offer — builds it under a scratch name beside
//! the destination and publishes it in one step. A reader of the destination
//! sees the version that was there before or the complete new one, never a
//! prefix of it, and a run that fails leaves the destination exactly as it
//! found it.
//!
//! Shared rather than written twice. The care here is in the details — where
//! the scratch file lives, what makes its name safe, which system call
//! publishes it — and a second copy of those details is a second place for them
//! to drift apart. It lives here rather than inside the command line because a
//! window publishes a file on exactly these terms and cannot depend on a
//! binary crate to do it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use ferritecad_types::{CadError, ObjectId, Result};

/// What to do about anything already at a destination.
///
/// Not a `bool`. The two interfaces that publish files offer replacement in
/// different words — one a flag, the other a button somebody pressed — and the
/// sentence a person is shown when something is already there has to be the
/// one their own interface can act on. Carrying it here keeps one publication
/// while keeping the command line's vocabulary out of a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Existing<'a> {
    /// Refuse. The publication is an atomic no-clobber, and `advice` finishes
    /// the sentence the caller's user reads when it refuses.
    Keep { advice: &'a str },
    /// Replace it whole. The caller has already decided that it may.
    Replace,
}

/// A scratch file in a private directory beside its destination.
///
/// The directory is created atomically before the file is handed to its
/// producer. That matters for SQLite: unlike the STL writer, it cannot be
/// given an already-open `create_new` file, so reserving only the file name
/// would leave a check/use window in which a symlink could be put there.
#[derive(Debug)]
pub struct Temporary {
    directory: PathBuf,
    path: PathBuf,
}

impl Temporary {
    /// Reserves a unique private scratch directory beside `destination`.
    ///
    /// Beside it rather than in the system temporary directory, because a
    /// rename is only atomic within one filesystem and `/tmp` is often another
    /// one. The name is unique rather than a fixed `<output>.partial`: a
    /// concurrent run of the same command would otherwise truncate this one's
    /// scratch file, and a fixed name is one anything else can be waiting at.
    ///
    /// The file itself is not created here. What creates it decides how — the
    /// export opens it with `create_new`, the import hands the name to SQLite.
    /// Its parent, however, already belongs to this guard, so neither path has
    /// a check/use gap at the file name.
    pub fn beside(destination: &Path) -> Result<Self> {
        let parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        destination.file_name().ok_or_else(|| {
            CadError::input(format!("{} is not a file name", destination.display()))
        })?;

        // Do not include the destination's full name: a legal name close to a
        // filesystem's component limit must not become illegal merely because
        // a UUID was appended to it.
        let mut name = OsString::from(".ferritecad-");
        name.push(ObjectId::new().to_string());
        name.push(".partial");
        let directory = parent.join(name);

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
            .create(&directory)
            .map_err(|e| CadError::io(format!("reserving {}", directory.display()), e))?;

        Ok(Self {
            path: directory.join("payload"),
            directory,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Publishes the scratch file as `destination`.
    ///
    /// The only step that touches the destination. For [`Existing::Keep`] a
    /// hard link is an atomic no-clobber publish on the filesystems the product
    /// platforms support: a destination that appeared while the work was going
    /// on makes this fail rather than being overwritten. For
    /// [`Existing::Replace`] the caller has explicitly authorised replacement,
    /// so a rename gives the usual atomic old-or-new view.
    pub fn publish(self, destination: &Path, existing: Existing<'_>) -> Result<()> {
        let published = match existing {
            Existing::Replace => std::fs::rename(&self.path, destination)
                .map_err(|e| CadError::io(format!("replacing {}", destination.display()), e)),
            Existing::Keep { advice } => {
                std::fs::hard_link(&self.path, destination).map_err(|error| {
                    if error.kind() == std::io::ErrorKind::AlreadyExists {
                        CadError::input(format!(
                            "{} already exists; {advice}",
                            destination.display()
                        ))
                    } else {
                        CadError::io(
                            format!("publishing {} without replacing it", destination.display()),
                            error,
                        )
                    }
                })
            }
        };

        // Drop removes the scratch name after a hard-link publication and the
        // now-empty private directory after either kind of publication.
        published
    }

    fn clean(&self) {
        let _ = std::fs::remove_file(&self.path);
        // SQLite normally removes these itself. Naming them explicitly keeps
        // a failed open or close from turning a private scratch directory into
        // a permanent one, without using recursive deletion on a path whose
        // contents this code did not create knowingly.
        for suffix in ["-journal", "-wal", "-shm"] {
            let mut sidecar = self.path.as_os_str().to_owned();
            sidecar.push(OsStr::new(suffix));
            let _ = std::fs::remove_file(PathBuf::from(sidecar));
        }
        let _ = std::fs::remove_dir(&self.directory);
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        // A failure here is not worth reporting over whatever error is already
        // on its way out, and there is nothing useful to do about it.
        self.clean();
    }
}

/// Whether a directory entry exists, including a dangling symlink.
///
/// [`Path::exists`] follows symlinks and suppresses metadata errors. Neither
/// behaviour is safe for deciding whether a command may replace something.
pub fn path_entry_exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CadError::io(
            format!("checking whether {} exists", path.display()),
            error,
        )),
    }
}

/// Whether two paths name the same directory entry.
///
/// False when nothing is at `destination`: a name that does not exist yet is
/// not the file being read, whatever it is spelled like. Resolved rather than
/// compared as text, because the same file has any number of spellings and a
/// comparison of strings would accept most of them.
pub fn is_same_entry(source: &Path, destination: &Path) -> Result<bool> {
    if !path_entry_exists(destination)? {
        return Ok(false);
    }

    let resolved_source = std::fs::canonicalize(source)
        .map_err(|e| CadError::io(format!("resolving {}", source.display()), e))?;
    let resolved_destination = std::fs::canonicalize(destination)
        .map_err(|e| CadError::io(format!("resolving {}", destination.display()), e))?;
    Ok(resolved_source == resolved_destination)
}

/// Refuses a destination that is the source, which nothing makes acceptable.
///
/// Not a flag, and not a button either: a run that reads a file and writes the
/// same file has already destroyed the thing it was reading from by the time
/// anybody could be asked about it.
pub fn refuse_source_as_destination(source: &Path, destination: &Path, what: &str) -> Result<()> {
    if is_same_entry(source, destination)? {
        return Err(CadError::input(what.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a caller of this module would put in front of its own user. The
    /// wording is the interface's; this module only places it.
    const ADVICE: &str = "pass --force to replace it";

    #[test]
    fn the_scratch_namespace_is_reserved_before_the_file_is_created() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("part.fcad");
        let scratch = Temporary::beside(&destination).expect("reserves scratch space");

        assert!(scratch.directory.is_dir());
        assert!(!scratch.path().exists());
        assert_eq!(scratch.path().parent(), Some(scratch.directory.as_path()));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&scratch.directory)
                .expect("stats the directory")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        }

        drop(scratch);
        assert!(
            std::fs::read_dir(root.path())
                .expect("lists the parent")
                .next()
                .is_none(),
            "the guard left its private directory behind"
        );
    }

    #[test]
    fn a_long_destination_name_does_not_make_the_scratch_component_longer() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("x".repeat(240));
        let scratch = Temporary::beside(&destination).expect("reserves scratch space");

        assert!(scratch.directory.file_name().expect("has a name").len() < 80);
    }

    /// A destination that does not exist is nobody's source.
    #[test]
    fn a_name_nothing_is_at_is_not_the_file_being_read() {
        let root = tempfile::tempdir().expect("temporary directory");
        let source = root.path().join("part.fcad");
        std::fs::write(&source, b"a document").expect("writes the source");

        assert!(!is_same_entry(&source, &root.path().join("part.fbx")).expect("asks"));
        assert!(is_same_entry(&source, &source).expect("asks"));
        // The same file under another spelling is the same file.
        let sideways = root.path().join(".").join("part.fcad");
        assert!(is_same_entry(&source, &sideways).expect("asks"));
    }

    #[test]
    fn publication_removes_the_private_namespace_not_the_finished_file() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("part.stl");
        let scratch = Temporary::beside(&destination).expect("reserves scratch space");
        std::fs::write(scratch.path(), b"complete file").expect("writes the scratch file");

        scratch
            .publish(&destination, Existing::Keep { advice: ADVICE })
            .expect("publishes without replacing");

        assert_eq!(
            std::fs::read(&destination).expect("reads the published file"),
            b"complete file"
        );
        let entries: Vec<_> = std::fs::read_dir(root.path())
            .expect("lists the parent")
            .map(|entry| entry.expect("reads an entry").path())
            .collect();
        assert_eq!(entries, vec![destination]);
    }
}
