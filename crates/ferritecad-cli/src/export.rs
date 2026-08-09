// SPDX-License-Identifier: MIT
//! Writing a document's geometry out as a file another program can open.
//!
//! The first path through this project a person can actually use: a stored
//! document goes in, a mesh file comes out, and nothing in between is a
//! prototype. That makes the file-handling worth more care than the arithmetic
//! — an export that half-writes over yesterday's part has done more damage
//! than one that simply fails.

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
    // Checked before any work: rebuilding a document and meshing it only to
    // refuse at the last step wastes the user's time and tells them nothing
    // they could not have been told immediately.
    if !args.force && args.output.exists() {
        return Err(CadError::input(format!(
            "{} already exists; pass --force to replace it",
            args.output.display()
        )));
    }

    let params = TessellationParams::new(args.linear_deflection, args.angular_deflection, false)?;
    let document = Document::open(&args.path)?;

    // Cold on purpose. An export is rare and must be right; consulting a cache
    // here would make the file depend on the state of a sidecar that exists
    // only to save time.
    let mut kernel = OcctKernel::new()?;
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())?;

    let chosen = choose(&document, args.solid.as_deref());
    let mesh = chosen.and_then(|(id, label)| {
        let shape = built
            .shape(id)
            .ok_or_else(|| CadError::input(format!("{label} produced no geometry to export")))?;
        let mesh = kernel.tessellate(shape, &params, &OperationContext::default())?;
        Ok((label, mesh))
    });

    // Whatever happened, the session gets its shapes back before we return.
    let (label, mesh) = match mesh {
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
    let written = write_atomically(&args.output, &bytes)?;

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

    let matched: Vec<&ObjectRecord> = bodies
        .iter()
        .filter(|object| object.name.as_deref() == Some(wanted) || object.id.to_string() == wanted)
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
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
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
    let mut temporary = directory.join(name);
    temporary.as_mut_os_string().push(".partial");

    let guard = Temporary(temporary);
    std::fs::write(&guard.0, bytes)
        .map_err(|e| CadError::io(format!("writing {}", guard.0.display()), e))?;

    // The last step, and the only one that touches the destination. Until it
    // runs, whatever was already there is untouched.
    std::fs::rename(&guard.0, path)
        .map_err(|e| CadError::io(format!("replacing {}", path.display()), e))?;

    // Renamed away; there is nothing left to clean up.
    std::mem::forget(guard);
    Ok(path.to_path_buf())
}

/// Removes a partial file when it goes out of scope.
struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        // A failure here is not worth reporting over whatever error is already
        // on its way out, and there is nothing useful to do about it.
        let _ = std::fs::remove_file(&self.0);
    }
}
