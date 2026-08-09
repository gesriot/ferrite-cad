// SPDX-License-Identifier: MIT
//! Writing a document's geometry out as a file another program can open.
//!
//! The first path through this project a person can actually use: a stored
//! document goes in, a mesh file comes out, and nothing in between is a
//! prototype. That makes the file-handling worth more care than the arithmetic
//! — an export that half-writes over yesterday's part has done more damage
//! than one that simply fails.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ferritecad_document::{Document, ObjectPayload, ObjectRecord};
use ferritecad_eval::rebuild_cold;
use ferritecad_export::binary_stl;
use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{CadError, ObjectId, Result};

use crate::ExportStlArgs;

pub fn export_stl(args: ExportStlArgs) -> Result<ExitCode> {
    // This check takes precedence over the ordinary no-clobber message: the
    // source is not a destination `--force` can ever make acceptable.
    refuse_source_as_output(&args.path, &args.output)?;

    // Checked before any work: rebuilding a document and meshing it only to
    // refuse at the last step wastes the user's time and tells them nothing
    // they could not have been told immediately.
    if !args.force && path_entry_exists(&args.output)? {
        return Err(CadError::input(format!(
            "{} already exists; pass --force to replace it",
            args.output.display()
        )));
    }
    let params = TessellationParams::new(args.linear_deflection, args.angular_deflection, false)?;
    let document = Document::open(&args.path)?;
    let (chosen_id, label) = choose(&document, args.solid.as_deref())?;

    // Cold on purpose. An export is rare and must be right; consulting a cache
    // here would make the file depend on the state of a sidecar that exists
    // only to save time.
    let mut kernel = OcctKernel::new()?;
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())?;

    let mesh = (|| {
        let shape = built
            .shape(chosen_id)
            .ok_or_else(|| CadError::input(format!("{label} produced no geometry to export")))?;
        let mesh = kernel.tessellate(shape, &params, &OperationContext::default())?;
        Ok(mesh)
    })();

    // Whatever happened, the session gets its shapes back before we return.
    let mesh = match mesh {
        Ok(found) => found,
        Err(error) => {
            built.release_all(&mut kernel);
            return Err(error);
        }
    };
    let triangles = mesh.triangle_count();
    let bytes = binary_stl(&mesh);
    built.release_all(&mut kernel);

    let bytes = bytes?;
    let written = write_atomically(&args.output, &bytes, args.force)?;

    println!(
        "wrote {} ({} triangles, {} bytes) from {label}",
        written.display(),
        triangles,
        bytes.len()
    );
    println!(
        "  deflection: {} mm linear, {} rad angular",
        params.linear_deflection(),
        params.angular_deflection()
    );
    Ok(ExitCode::SUCCESS)
}

/// Which solid to export, and what to call it in messages.
///
/// With one body in the document the choice is obvious and is made. With
/// several it is not made: picking the first would be picking whichever
/// happened to sort first, and the user would have no way of knowing that a
/// different part was meant.
fn choose(document: &Document, wanted: Option<&str>) -> Result<(ObjectId, String)> {
    let bodies: Vec<ObjectRecord> = document
        .objects()?
        .into_iter()
        .filter(|object| matches!(object.payload, ObjectPayload::Body(_)))
        .collect();

    if bodies.is_empty() {
        return Err(CadError::input(format!(
            "{} contains no bodies to export",
            document.path().display()
        )));
    }

    let Some(wanted) = wanted else {
        if bodies.len() == 1 {
            return Ok(describe(&bodies[0]));
        }
        return Err(CadError::input(format!(
            "{} contains {} bodies; name one with --solid:\n{}",
            document.path().display(),
            bodies.len(),
            list(&bodies)
        )));
    };

    // A canonical UUID is an identifier first, even if another body's name
    // happens to contain the same text. Otherwise the very identifier offered
    // as the escape hatch for duplicate names could itself become ambiguous.
    if let Ok(id) = wanted.parse::<ObjectId>() {
        return bodies
            .iter()
            .find(|object| object.id == id)
            .map(describe)
            .ok_or_else(|| {
                CadError::input(format!(
                    "no body with identifier {wanted} in {}; this document holds:\n{}",
                    document.path().display(),
                    list(&bodies)
                ))
            });
    }

    let matched: Vec<&ObjectRecord> = bodies
        .iter()
        .filter(|object| object.name.as_deref() == Some(wanted))
        .collect();

    match matched.as_slice() {
        [one] => Ok(describe(one)),
        [] => Err(CadError::input(format!(
            "no body called {wanted} in {}; this document holds:\n{}",
            document.path().display(),
            list(&bodies)
        ))),
        several => Err(CadError::input(format!(
            "{} bodies are called {wanted}; name one by its identifier instead:\n{}",
            several.len(),
            list(&bodies)
        ))),
    }
}

fn describe(object: &ObjectRecord) -> (ObjectId, String) {
    match &object.name {
        Some(name) => (object.id, format!("{name} ({})", object.id)),
        None => (object.id, object.id.to_string()),
    }
}

fn list(bodies: &[ObjectRecord]) -> String {
    bodies
        .iter()
        .map(|object| match &object.name {
            Some(name) => format!("  {name}  {}", object.id),
            None => format!("  (unnamed)  {}", object.id),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Writes `bytes` to `path` through a temporary file beside it.
///
/// Beside it rather than in the system temporary directory, because a rename
/// is only atomic within one filesystem and `/tmp` is often another one. The
/// reader of this file either sees the version that was there before or the
/// complete new one, never a prefix of it.
///
/// The temporary file is removed on every path out, including the ones taken
/// by `?`, which is what the guard below is for.
fn write_atomically(path: &Path, bytes: &[u8], force: bool) -> Result<PathBuf> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let directory = match directory {
        Some(parent) => parent.to_path_buf(),
        None => PathBuf::from("."),
    };

    let name = path
        .file_name()
        .ok_or_else(|| CadError::input(format!("{} is not a file name", path.display())))?;
    // The name is unique and, more importantly, opened with `create_new`.
    // A fixed `<output>.partial` path would let a concurrent export truncate
    // this one's scratch file, and would follow an attacker's symlink.
    let mut temporary_name = OsString::from(".");
    temporary_name.push(name);
    temporary_name.push(".");
    temporary_name.push(ObjectId::new().to_string());
    temporary_name.push(".partial");
    let temporary = directory.join(temporary_name);

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| CadError::io(format!("creating {}", temporary.display()), e))?;
    let guard = Temporary(Some(temporary));
    file.write_all(bytes)
        .map_err(|e| CadError::io(format!("writing {}", guard.path().display()), e))?;
    file.sync_all()
        .map_err(|e| CadError::io(format!("syncing {}", guard.path().display()), e))?;
    drop(file);

    // The last step, and the only one that touches the destination. Without
    // --force, a hard link is an atomic no-clobber publish on the filesystems
    // supported by the product platforms: a destination created during the
    // rebuild makes this fail rather than being overwritten. With --force the
    // caller explicitly authorised replacement, so rename provides the usual
    // atomic old-or-new view.
    if force {
        std::fs::rename(guard.path(), path)
            .map_err(|e| CadError::io(format!("replacing {}", path.display()), e))?;
    } else {
        std::fs::hard_link(guard.path(), path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                CadError::input(format!(
                    "{} already exists; pass --force to replace it",
                    path.display()
                ))
            } else {
                CadError::io(
                    format!("publishing {} without replacing it", path.display()),
                    error,
                )
            }
        })?;
    }

    guard.finish();
    Ok(path.to_path_buf())
}

/// Whether a directory entry exists, including a dangling symlink.
///
/// [`Path::exists`] follows symlinks and suppresses metadata errors. Neither
/// behaviour is safe for deciding whether an export may replace something.
fn path_entry_exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CadError::io(
            format!("checking whether {} exists", path.display()),
            error,
        )),
    }
}

/// Refuses the one `--force` target that must never be replaced: the source.
fn refuse_source_as_output(source: &Path, output: &Path) -> Result<()> {
    if !path_entry_exists(output)? {
        return Ok(());
    }

    let source = std::fs::canonicalize(source)
        .map_err(|e| CadError::io(format!("resolving {}", source.display()), e))?;
    let output = std::fs::canonicalize(output)
        .map_err(|e| CadError::io(format!("resolving {}", output.display()), e))?;
    if source == output {
        return Err(CadError::input(
            "the native document cannot also be the STL output",
        ));
    }
    Ok(())
}

/// Removes a partial file when it goes out of scope.
struct Temporary(Option<PathBuf>);

impl Temporary {
    fn path(&self) -> &Path {
        self.0.as_deref().expect("a live guard always has a path")
    }

    fn finish(mut self) {
        if let Some(path) = self.0.take() {
            // After rename this is already absent. After hard-link publication
            // this removes only the temporary name; the destination remains.
            let _ = std::fs::remove_file(path);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_clobber_is_checked_again_at_publish_time() {
        let directory = tempfile::tempdir().expect("temp dir");
        let output = directory.path().join("part.stl");
        std::fs::write(&output, b"arrived during the rebuild").expect("writes");

        let error = write_atomically(&output, b"new export", false)
            .expect_err("publishing without force must not replace anything");

        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
        assert_eq!(
            std::fs::read(&output).expect("the other file remains"),
            b"arrived during the rebuild"
        );
        let leftovers: Vec<_> = std::fs::read_dir(directory.path())
            .expect("lists")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.to_string_lossy().contains(".partial"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files remain: {leftovers:?}"
        );
    }
}
