// SPDX-License-Identifier: MIT
//! One native Body, one cold read, one binary STL and one atomic publication.

use std::io::Write;
use std::path::{Path, PathBuf};

use ferritecad_document::{Document, ObjectPayload, ObjectRecord};
use ferritecad_eval::rebuild_cold;
use ferritecad_export::binary_stl;
use ferritecad_kernel::{GeometryKernel, OperationContext, ProgressSink, TessellationParams};
use ferritecad_types::{CadError, ObjectId, Result};

use crate::{Existing, Temporary, path_entry_exists, refuse_source_as_destination};

pub const STL_SOURCE_IS_DESTINATION: &str = "the native document cannot also be the STL output";

/// Names and IDs are distinct: a UI supplies Id, never a row or a displayed name.
#[derive(Debug, Clone, Copy)]
pub enum BodySelection<'a> {
    /// Accept the only Body. The caller supplies its own instructions if ambiguous.
    Only {
        advice: &'a str,
    },
    Id(ObjectId),
    /// CLI spelling: a parseable UUID has priority over an identical body name.
    NameOrId(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StlBody {
    pub id: ObjectId,
    pub name: Option<String>,
}

impl StlBody {
    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => format!("{name} ({})", self.id),
            None => self.id.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StlExportRequest<'a> {
    pub document: &'a Path,
    pub destination: &'a Path,
    pub body: BodySelection<'a>,
    pub params: TessellationParams,
    pub existing: Existing<'a>,
}

/// Facts about a completed publication, never a started worker or a GPU mesh.
#[derive(Debug)]
pub struct StlExport {
    pub destination: PathBuf,
    pub body: StlBody,
    pub triangles: usize,
    pub bytes: usize,
}

/// The caller's worker creates and owns the kernel session. Its factory is
/// called only after read-only open and body selection, preserving CLI refusals
/// even on a build without a native kernel. No source is reopened for geometry.
pub fn export_document_as_stl<K: GeometryKernel>(
    request: StlExportRequest<'_>,
    open_kernel: impl FnOnce() -> Result<K>,
    context: &OperationContext,
) -> Result<StlExport> {
    context.check_cancelled()?;
    refuse_source_as_destination(
        request.document,
        request.destination,
        STL_SOURCE_IS_DESTINATION,
    )?;
    if let Existing::Keep { advice } = request.existing
        && path_entry_exists(request.destination)?
    {
        return Err(CadError::input(format!(
            "{} already exists; {advice}",
            request.destination.display()
        )));
    }
    let document = Document::open_read_only(request.document)?;
    let body = choose(&document, request.body)?;
    context.check_cancelled()?;
    let mut kernel = open_kernel()?;
    // The outer 0.95/1.0 boundaries mean scratch closed / published. A rebuild's
    // own completion cannot be mistaken for publication by a progress consumer.
    let progress = context.progress().clone();
    let building = context
        .clone()
        .with_progress(ProgressSink::new(move |fraction| {
            progress.report(fraction * 0.8);
        }));
    let built = rebuild_cold(&document, &mut kernel, &building)?;
    let serialized: Result<_> = (|| {
        let shape = built.shape(body.id).ok_or_else(|| {
            CadError::input(format!("{} produced no geometry to export", body.label()))
        })?;
        let mesh = kernel.tessellate(shape, &request.params, &building)?;
        context.check_cancelled()?;
        let triangles = mesh.triangle_count();
        let bytes = binary_stl(&mesh)?;
        Ok((triangles, bytes))
    })();
    // Includes tessellation, serialization and cancellation failures. A failed
    // rebuild releases its partial results itself; after success we own them.
    built.release_all(&mut kernel);
    let (triangles, bytes) = serialized?;
    document.close()?;
    write_and_publish(&request, &bytes, context)?;
    Ok(StlExport {
        destination: request.destination.to_path_buf(),
        body,
        triangles,
        bytes: bytes.len(),
    })
}

fn write_and_publish(
    request: &StlExportRequest<'_>,
    bytes: &[u8],
    context: &OperationContext,
) -> Result<()> {
    context.check_cancelled()?;
    let temporary = Temporary::beside(request.destination)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.path())
        .map_err(|e| CadError::io(format!("creating {}", temporary.path().display()), e))?;
    for chunk in bytes.chunks(1 << 20) {
        context.check_cancelled()?;
        file.write_all(chunk)
            .map_err(|e| CadError::io(format!("writing {}", temporary.path().display()), e))?;
    }
    file.sync_all()
        .map_err(|e| CadError::io(format!("syncing {}", temporary.path().display()), e))?;
    drop(file);
    context.progress().report(0.95);
    // A destination may have become an alias since the initial request, even
    // if replacement was confirmed. Cancellation is checked at the last seam.
    refuse_source_as_destination(
        request.document,
        request.destination,
        STL_SOURCE_IS_DESTINATION,
    )?;
    context.check_cancelled()?;
    temporary.publish(request.destination, request.existing)?;
    context.progress().report(1.0);
    // Publication completed: a late cancellation cannot retract that fact.
    Ok(())
}

/// Which solid to export, and what to call it in messages.
///
/// With one body in the document the choice is obvious and is made. With
/// several it is not made: picking the first would be picking whichever
/// happened to sort first, and the user would have no way of knowing that a
/// different part was meant.
fn choose(document: &Document, selection: BodySelection<'_>) -> Result<StlBody> {
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

    let wanted = match selection {
        BodySelection::Only { advice } => {
            if bodies.len() == 1 {
                return Ok(describe(&bodies[0]));
            }
            return Err(CadError::input(format!(
                "{} contains {} bodies; {advice}:\n{}",
                document.path().display(),
                bodies.len(),
                list(&bodies)
            )));
        }
        BodySelection::Id(id) => {
            return bodies
                .iter()
                .find(|object| object.id == id)
                .map(describe)
                .ok_or_else(|| {
                    CadError::input(format!(
                        "no body with identifier {id} in {}; this document holds:\n{}",
                        document.path().display(),
                        list(&bodies)
                    ))
                });
        }
        BodySelection::NameOrId(wanted) => wanted,
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

fn describe(object: &ObjectRecord) -> StlBody {
    StlBody {
        id: object.id,
        name: object.name.clone(),
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

#[cfg(test)]
mod tests;
