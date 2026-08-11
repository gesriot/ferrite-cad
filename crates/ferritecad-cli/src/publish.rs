// SPDX-License-Identifier: MIT
//! Putting a finished file where the user asked for it, and not before.
//!
//! Both commands that produce a file — the STL export and the STEP import —
//! build it under a scratch name beside the destination and publish it in one
//! step. A reader of the destination sees the version that was there before or
//! the complete new one, never a prefix of it, and a command that fails leaves
//! the destination exactly as it found it.
//!
//! Shared rather than written twice. The care here is in the details — where
//! the scratch file lives, what makes its name safe, which system call
//! publishes it — and a second copy of those details is a second place for them
//! to drift apart.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ferritecad_types::{CadError, ObjectId, Result};

/// A scratch file beside its destination, removed on every path out.
#[derive(Debug)]
pub(crate) struct Temporary(Option<PathBuf>);

impl Temporary {
    /// Reserves a unique scratch name beside `destination`.
    ///
    /// Beside it rather than in the system temporary directory, because a
    /// rename is only atomic within one filesystem and `/tmp` is often another
    /// one. The name is unique rather than a fixed `<output>.partial`: a
    /// concurrent run of the same command would otherwise truncate this one's
    /// scratch file, and a fixed name is one anything else can be waiting at.
    ///
    /// The file itself is not created here. What creates it decides how — the
    /// export opens it with `create_new`, the import hands the name to SQLite —
    /// and the guard removes whatever ended up there.
    pub(crate) fn beside(destination: &Path) -> Result<Self> {
        let directory = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let name = destination.file_name().ok_or_else(|| {
            CadError::input(format!("{} is not a file name", destination.display()))
        })?;
        let mut scratch = OsString::from(".");
        scratch.push(name);
        scratch.push(".");
        scratch.push(ObjectId::new().to_string());
        scratch.push(".partial");

        let path = directory.join(scratch);
        // A fresh UUIDv7 does not collide, so anything already at this name is
        // something else entirely — including a dangling symlink, which is why
        // this asks for the link's own metadata rather than following it.
        if path_entry_exists(&path)? {
            return Err(CadError::io(
                format!("reserving {}", path.display()),
                std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "something is already at a name that should have been unused",
                ),
            ));
        }
        Ok(Self(Some(path)))
    }

    pub(crate) fn path(&self) -> &Path {
        self.0.as_deref().expect("a live guard always has a path")
    }

    /// Publishes the scratch file as `destination`.
    ///
    /// The only step that touches the destination. Without `--force`, a hard
    /// link is an atomic no-clobber publish on the filesystems the product
    /// platforms support: a destination that appeared while the work was going
    /// on makes this fail rather than being overwritten. With `--force` the
    /// caller explicitly authorised replacement, so a rename gives the usual
    /// atomic old-or-new view.
    pub(crate) fn publish(mut self, destination: &Path, force: bool) -> Result<()> {
        let scratch = self.0.take().expect("a live guard always has a path");

        let published = if force {
            std::fs::rename(&scratch, destination)
                .map_err(|e| CadError::io(format!("replacing {}", destination.display()), e))
        } else {
            std::fs::hard_link(&scratch, destination).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    CadError::input(format!(
                        "{} already exists; pass --force to replace it",
                        destination.display()
                    ))
                } else {
                    CadError::io(
                        format!("publishing {} without replacing it", destination.display()),
                        error,
                    )
                }
            })
        };

        // After a rename the scratch name is already gone. After hard-link
        // publication it is a second name for the published file, and only that
        // name is removed. A failure leaves nothing behind either way.
        let _ = std::fs::remove_file(&scratch);
        published
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        // A failure here is not worth reporting over whatever error is already
        // on its way out, and there is nothing useful to do about it.
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Whether a directory entry exists, including a dangling symlink.
///
/// [`Path::exists`] follows symlinks and suppresses metadata errors. Neither
/// behaviour is safe for deciding whether a command may replace something.
pub(crate) fn path_entry_exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CadError::io(
            format!("checking whether {} exists", path.display()),
            error,
        )),
    }
}

/// Refuses a destination that is the source, which no flag makes acceptable.
pub(crate) fn refuse_source_as_destination(
    source: &Path,
    destination: &Path,
    what: &str,
) -> Result<()> {
    if !path_entry_exists(destination)? {
        return Ok(());
    }

    let resolved_source = std::fs::canonicalize(source)
        .map_err(|e| CadError::io(format!("resolving {}", source.display()), e))?;
    let resolved_destination = std::fs::canonicalize(destination)
        .map_err(|e| CadError::io(format!("resolving {}", destination.display()), e))?;
    if resolved_source == resolved_destination {
        return Err(CadError::input(what.to_owned()));
    }
    Ok(())
}
