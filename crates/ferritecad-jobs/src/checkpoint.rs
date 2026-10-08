// SPDX-License-Identifier: MIT
//! §30N: named checkpoints, the same operations for the window and the command
//! line.
//!
//! Every write here reads a source document and publishes a *new* file: the
//! source with one checkpoint more or less, the source's catalog around a
//! checkpoint's model (Restore), or a checkpoint's model alone (extraction).
//! The pattern is the edit copies': pinned read-only source, optional or
//! required expected version, the work in a private scratch file beside the
//! destination, the version re-checked after the work, no-clobber publication
//! last. Nothing here needs a kernel; showing a restored model does.
//!
//! The window runs these on its private versions through [`crate::StepTicket`];
//! the command line runs them on a user's file and writes a copy.

use std::path::{Path, PathBuf};

use ferritecad_document::{Access, CheckpointEntry, Document, DocumentVersion};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CadError, CheckpointId, ContentHash, DocumentId, Result};

use crate::publish::{Existing, Temporary, path_entry_exists, refuse_source_as_destination};

/// What a document's checkpoint list is, read from one pinned reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointListing {
    pub document_id: DocumentId,
    /// The source's complete content version: what `--expect-version` names.
    pub content_version: ContentHash,
    /// The source's model with the catalog set aside: equal to an entry's
    /// `model` exactly when that checkpoint holds the model the file holds now.
    pub model: ContentHash,
    pub entries: Vec<CheckpointEntry>,
}

/// Lists the checkpoints of `source` without writing anything, migrating
/// nothing and reading no image. Schema v3 documents have none.
pub fn list_checkpoints(source: &Path) -> Result<CheckpointListing> {
    let document = Document::open_read_only(source)?;
    let listing = CheckpointListing {
        document_id: document.meta().document_id,
        content_version: document.content_version()?,
        model: document.model_without_checkpoints()?,
        entries: document.checkpoints()?,
    };
    document.close()?;
    Ok(listing)
}

/// Adds a checkpoint named `name` of the source's current model, in a new file.
#[derive(Debug, Clone)]
pub struct CreateCheckpointRequest {
    pub source: PathBuf,
    pub expected: DocumentVersion,
    pub name: String,
    pub destination: PathBuf,
}

/// Removes one checkpoint, in a new file. The model is not touched.
#[derive(Debug, Clone)]
pub struct DeleteCheckpointRequest {
    pub source: PathBuf,
    pub expected: DocumentVersion,
    pub checkpoint: CheckpointId,
    pub destination: PathBuf,
}

/// Writes one checkpoint's model, alone, to a new file.
#[derive(Debug, Clone)]
pub struct ExtractCheckpointRequest {
    pub source: PathBuf,
    /// Optional: the checkpoint's UUID and stored hash already pin what is
    /// written; this pins the file it is read from as well.
    pub expected: Option<DocumentVersion>,
    pub checkpoint: CheckpointId,
    pub destination: PathBuf,
}

/// What a published checkpoint change or extraction is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointCopy {
    pub destination: PathBuf,
    pub document_id: DocumentId,
    /// The published file's complete content version.
    pub content_version: ContentHash,
    /// The checkpoint created, deleted, restored or extracted.
    pub checkpoint: CheckpointEntry,
}

const TAKEN: &str = "choose a different file name";

/// The shared shape of every checkpoint write: see the module documentation.
/// `write` gets the pinned source and the scratch path to fill, and reports
/// which checkpoint it was about. Progress marks 0.1 (read), 0.9 (written) and
/// 0.95 (about to re-check and publish) let a test change the world at a known
/// point.
fn checkpoint_copy(
    source_path: &Path,
    expected: Option<DocumentVersion>,
    destination: &Path,
    editable: bool,
    context: &OperationContext,
    write: impl FnOnce(&Document, &Path) -> Result<CheckpointEntry>,
) -> Result<CheckpointCopy> {
    context.check_cancelled()?;
    refuse_source_as_destination(
        source_path,
        destination,
        "source and output must be different files",
    )?;
    if path_entry_exists(destination)? {
        return Err(CadError::input(format!(
            "{} already exists; {TAKEN}",
            destination.display()
        )));
    }
    let source = Document::open_read_only(source_path)?;
    if let Some(expected) = expected {
        require_version(&source, expected)?;
    }
    if editable && let Access::ReadOnly { reason } = source.copy_access()? {
        return Err(CadError::unsupported(format!(
            "document cannot be edited: {reason}"
        )));
    }
    let document_id = source.meta().document_id;
    let pinned = source.content_version()?;
    context.progress().report(0.1);
    let temporary = Temporary::beside(destination)?;
    let checkpoint = write(&source, temporary.path())?;
    source.close()?;
    let content_version = {
        let written = Document::open_read_only(temporary.path())?;
        if written.meta().document_id != document_id {
            return Err(CadError::input(
                "the result is another document; nothing was written",
            ));
        }
        let version = written.content_version()?;
        written.close()?;
        version
    };
    context.progress().report(0.9);
    context.check_cancelled()?;
    context.progress().report(0.95);
    // Re-open the path, not the old handle: a source replaced while this ran is
    // stale too, whether or not an expected version was given.
    let current = Document::open_read_only(source_path)?;
    require_version(
        &current,
        DocumentVersion {
            document_id,
            content: pinned,
        },
    )?;
    refuse_source_as_destination(
        source_path,
        destination,
        "source and output must be different files",
    )?;
    context.check_cancelled()?;
    temporary.publish(destination, Existing::Keep { advice: TAKEN })?;
    drop(current);
    context.progress().report(1.0);
    Ok(CheckpointCopy {
        destination: destination.to_path_buf(),
        document_id,
        content_version,
        checkpoint,
    })
}

fn require_version(document: &Document, expected: DocumentVersion) -> Result<()> {
    if document.meta().document_id != expected.document_id
        || document.content_version()? != expected.content
    {
        return Err(CadError::input(
            "source has changed since it was read; read it again and repeat the request",
        ));
    }
    Ok(())
}

/// Two more private scratch names beside `destination`, for the copy an image is
/// made from and for the image.
fn image_scratch(destination: &Path) -> Result<(Temporary, Temporary)> {
    Ok((
        Temporary::beside(destination)?,
        Temporary::beside(destination)?,
    ))
}

/// A new file: the source with a checkpoint of its current model added. The
/// copy is migrated to schema v4 when the source is older; the source is not.
pub fn create_checkpoint_copy(
    request: &CreateCheckpointRequest,
    context: &OperationContext,
) -> Result<CheckpointCopy> {
    let name = ferritecad_document::checkpoint_name(&request.name)?;
    checkpoint_copy(
        &request.source,
        Some(request.expected),
        &request.destination,
        true,
        context,
        |source, scratch| {
            source.snapshot_to(scratch)?;
            let mut copy = Document::open(scratch)?;
            let (working, image) = image_scratch(&request.destination)?;
            copy.write_checkpoint_image(working.path(), image.path())?;
            context.check_cancelled()?;
            let entry = copy.add_checkpoint(&name, image.path())?;
            copy.close()?;
            Ok(entry)
        },
    )
}

/// A new file: the source without one checkpoint.
pub fn delete_checkpoint_copy(
    request: &DeleteCheckpointRequest,
    context: &OperationContext,
) -> Result<CheckpointCopy> {
    checkpoint_copy(
        &request.source,
        Some(request.expected),
        &request.destination,
        true,
        context,
        |source, scratch| {
            source.snapshot_to(scratch)?;
            let mut copy = Document::open(scratch)?;
            let entry = copy.remove_checkpoint(request.checkpoint)?;
            copy.close()?;
            Ok(entry)
        },
    )
}

/// A new file holding one checkpoint's model alone (its catalog is empty), with
/// every identity the model had. What the command line offers for Restore.
pub fn extract_checkpoint(
    request: &ExtractCheckpointRequest,
    context: &OperationContext,
) -> Result<CheckpointCopy> {
    checkpoint_copy(
        &request.source,
        request.expected,
        &request.destination,
        false,
        context,
        |source, scratch| source.extract_checkpoint_image(request.checkpoint, scratch),
    )
}

/// A new file: one checkpoint's model with the source's catalog, row for row.
/// What the window's Restore makes, as one more version of the open document.
pub(crate) fn restore_checkpoint_copy(
    source_path: &Path,
    expected: DocumentVersion,
    checkpoint: CheckpointId,
    destination: &Path,
    context: &OperationContext,
) -> Result<CheckpointCopy> {
    checkpoint_copy(
        source_path,
        Some(expected),
        destination,
        true,
        context,
        |source, scratch| {
            let entry = source.extract_checkpoint_image(checkpoint, scratch)?;
            let mut restored = Document::open(scratch)?;
            restored.replace_checkpoints_from(source)?;
            restored.close()?;
            Ok(entry)
        },
    )
}
