// SPDX-License-Identifier: MIT
//! One STEP reading, owned geometry, and a closed document published atomically.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ferritecad_document::{Document, ImporterIdentity, StepImportRequest};
use ferritecad_exchange::{Diagnostic, Import, PersistedScene, StoredScene};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use ferritecad_types::{CadError, ContentHash, DocumentId, ImportedSourceId, ObjectId, Result};

use crate::{Existing, Temporary, path_entry_exists, refuse_source_as_destination};

pub const STEP_SOURCE_IS_DESTINATION: &str =
    "the STEP file cannot also be the document written from it";

/// Interface-independent input. `Existing::Keep` carries the caller's advice.
#[derive(Debug, Clone, Copy)]
pub struct ImportStepRequest<'a> {
    pub source: &'a Path,
    pub destination: &'a Path,
    pub name: Option<&'a str>,
    pub existing: Existing<'a>,
}

/// Facts from this reading, including diagnostics in reader order.
/// Silence describes the reader, not the correctness of the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReadFacts {
    pub source_byte_len: u64,
    pub source_hash: ContentHash,
    pub imported_by: ImporterIdentity,
    pub diagnostics: Vec<Diagnostic>,
}

/// A published document. The scene is the exact persisted projection returned
/// by storage, including its newly minted occurrence identities, never another
/// call to `Scene::persist`. No geometry handles escape the job.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishedStepImport {
    pub destination: PathBuf,
    pub document_id: DocumentId,
    pub object_id: ObjectId,
    pub name: String,
    pub source: ImportedSourceId,
    /// Basename only, with the text client's established lossy OS-string rule.
    pub source_name: Option<String>,
    pub read: StepReadFacts,
    /// STEP schema/unit, definitions, local placements, colours and identities.
    pub scene: PersistedScene,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepImportOutcome {
    Published(Box<PublishedStepImport>),
    /// No document, object, destination or live geometry was produced.
    Rejected(StepReadFacts),
}

/// Read and import on the calling thread, then store and publish once.
///
/// The factory runs only after source/alias protection, no-clobber and the one
/// source read. The importer must return handles belonging to that kernel;
/// ownership transfers to this job immediately on `Import::Imported`. On an
/// importer error or `Rejected`, the callback retains responsibility for any
/// intermediate geometry, as the existing native adapter already does.
///
/// Progress boundaries: 0 before preflight/read; .1 after bytes; .6 after import;
/// .7 after scratch creation; .8 after storage; .9 after SQLite close and shape
/// release, before publication; 1 after publication. Cancellation is checked at
/// every pre-publication boundary. It cannot interrupt a blocking native read.
/// A cancellation requested at 1 does not revoke the published result.
pub fn import_step_document<K: GeometryKernel>(
    request: &ImportStepRequest<'_>,
    open_kernel: impl FnOnce() -> Result<K>,
    read_step: impl FnOnce(&mut K, &[u8]) -> Result<Import>,
    context: &OperationContext,
) -> Result<StepImportOutcome> {
    boundary(context, 0.0)?;
    refuse_source_as_destination(
        request.source,
        request.destination,
        STEP_SOURCE_IS_DESTINATION,
    )?;
    if let Existing::Keep { advice } = request.existing
        && path_entry_exists(request.destination)?
    {
        return Err(CadError::input(format!(
            "{} already exists; {advice}",
            request.destination.display()
        )));
    }
    let bytes = std::fs::read(request.source)
        .map_err(|e| CadError::io(format!("reading {}", request.source.display()), e))?;
    boundary(context, 0.1)?;
    let mut kernel = open_kernel()?;
    let outcome = read_step(&mut kernel, &bytes)?;
    // Establish ownership before any fallible work, cancellation or callback.
    let owned = OwnedImport {
        kernel: &mut kernel,
        outcome,
    };
    boundary(context, 0.6)?;
    if let Import::Rejected { diagnostics } = &owned.outcome {
        return Ok(StepImportOutcome::Rejected(StepReadFacts {
            source_byte_len: bytes.len() as u64,
            source_hash: ContentHash::of_bytes(&bytes),
            imported_by: ImporterIdentity::of(owned.kernel.identity()),
            diagnostics: diagnostics.clone(),
        }));
    }

    let object_id = ObjectId::new();
    let name = request
        .name
        .map(str::to_owned)
        .or_else(|| {
            request
                .source
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Imported".to_owned());
    let source_name = request
        .source
        .file_name()
        .map(|s| s.to_string_lossy().into_owned());
    // Declaration order also closes SQLite before Temporary cleanup on errors.
    let temporary = Temporary::beside(request.destination)?;
    let mut document = Document::create(temporary.path())?;
    boundary(context, 0.7)?;
    let stored = document.store_step_import(StepImportRequest {
        object: object_id,
        name: Some(&name),
        source: &bytes,
        source_name: source_name.as_deref(),
        import: &owned.outcome,
        importer: owned.kernel.identity(),
    })?;
    boundary(context, 0.8)?;
    let StoredScene::V3(scene) = stored.scene else {
        return Err(CadError::unsupported(
            "STEP import storage did not return occurrence identities",
        ));
    };
    let published = Box::new(PublishedStepImport {
        destination: request.destination.to_owned(),
        document_id: document.meta().document_id,
        object_id,
        name,
        source: stored.source,
        source_name: stored.source_name,
        read: StepReadFacts {
            source_byte_len: stored.source_byte_len,
            source_hash: stored.source_hash,
            imported_by: stored.imported_by,
            diagnostics: stored.diagnostics_at_import,
        },
        scene,
    });
    document.close()?;
    drop(owned);
    boundary(context, 0.9)?;
    // A long import leaves time for either path to become a source alias.
    refuse_source_as_destination(
        request.source,
        request.destination,
        STEP_SOURCE_IS_DESTINATION,
    )?;
    context.check_cancelled()?;
    temporary.publish(request.destination, request.existing)?;
    context.progress().report(1.0);
    Ok(StepImportOutcome::Published(published))
}

fn boundary(context: &OperationContext, fraction: f64) -> Result<()> {
    context.progress().report(fraction);
    context.check_cancelled()
}

struct OwnedImport<'a, K: GeometryKernel> {
    kernel: &'a mut K,
    outcome: Import,
}

impl<K: GeometryKernel> Drop for OwnedImport<'_, K> {
    fn drop(&mut self) {
        if let Some(scene) = self.outcome.scene() {
            // One handle may be shared by definitions in a callback. Ownership
            // is per handle, not per occurrence or definition.
            for shape in scene.shapes().collect::<BTreeSet<_>>() {
                self.kernel.release(shape);
            }
        }
    }
}

#[cfg(test)]
mod tests;
