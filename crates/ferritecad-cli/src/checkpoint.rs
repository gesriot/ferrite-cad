// SPDX-License-Identifier: MIT
//! `list-checkpoints`, `create-checkpoint`, `delete-checkpoint` and
//! `extract-checkpoint` (§30N): the checkpoints stored inside a document, through
//! the same library operations the window uses.
//!
//! The source is never written. Create and delete publish a *copy* of the
//! source with the change, and require the version they were asked about;
//! extraction publishes one checkpoint's model alone, which is how the command
//! line restores one. Every output is no-clobber. No command needs a kernel.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use ferritecad_document::DocumentVersion;
use ferritecad_jobs::{
    CheckpointCopy, CheckpointListing, CreateCheckpointRequest, DeleteCheckpointRequest,
    ExtractCheckpointRequest, list_checkpoints,
};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{CheckpointId, ContentHash, Result};

use crate::json;

#[derive(Debug, Args)]
pub struct ListCheckpointsArgs {
    /// Existing .fcad, read without migration or modification.
    source: PathBuf,
    /// Emit one JSON v1 result. Argument errors remain clap text. Paths must be UTF-8.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub struct CreateCheckpointArgs {
    /// Existing .fcad, read without migration or modification.
    source: PathBuf,
    /// The checkpoint's name: 1-80 characters after trimming, no control
    /// characters. Names need not be unique; the printed UUID is the identity.
    #[arg(long)]
    name: String,
    /// Required complete content version of SOURCE (inspect or list-checkpoints
    /// print it); a source that changed is refused.
    #[arg(long)]
    expect_version: ContentHash,
    /// New .fcad path: SOURCE plus the checkpoint. Existing files are refused.
    #[arg(short, long)]
    output: PathBuf,
    /// Emit one JSON v1 result or execution error. Argument errors remain clap text.
    /// Source and output paths must be UTF-8 in JSON mode.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub struct DeleteCheckpointArgs {
    /// Existing .fcad, read without migration or modification.
    source: PathBuf,
    /// The checkpoint's UUID, as list-checkpoints prints it (never a name).
    #[arg(long)]
    checkpoint: CheckpointId,
    /// Required complete content version of SOURCE; a source that changed is refused.
    #[arg(long)]
    expect_version: ContentHash,
    /// New .fcad path: SOURCE without the checkpoint. Existing files are refused.
    #[arg(short, long)]
    output: PathBuf,
    /// Emit one JSON v1 result or execution error. Argument errors remain clap text.
    /// Source and output paths must be UTF-8 in JSON mode.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub struct ExtractCheckpointArgs {
    /// Existing .fcad, read without migration or modification.
    source: PathBuf,
    /// The checkpoint's UUID, as list-checkpoints prints it (never a name).
    #[arg(long)]
    checkpoint: CheckpointId,
    /// New .fcad path holding that checkpoint's model, every identity kept.
    /// Existing files are refused; there is no --force.
    #[arg(short, long)]
    output: PathBuf,
    /// Optional complete content version of SOURCE; refuses a source that changed.
    #[arg(long)]
    expect_version: Option<ContentHash>,
    /// Emit one JSON v1 result or execution error. Argument errors remain clap text.
    /// Source and output paths must be UTF-8 in JSON mode.
    #[arg(long)]
    json: bool,
}

pub fn list_result(args: &ListCheckpointsArgs) -> Result<CheckpointListing> {
    if args.json {
        json::require_utf8_path(&args.source)?;
    }
    list_checkpoints(&args.source)
}

pub fn run_list(args: ListCheckpointsArgs) -> Result<ExitCode> {
    if args.json {
        return Ok(json::emit(
            json::Operation::ListCheckpoints,
            list_result(&args).map(json::CheckpointList::from),
        ));
    }
    let listing = list_result(&args)?;
    println!(
        "document {}  version {}",
        listing.document_id, listing.content_version
    );
    if listing.entries.is_empty() {
        println!("no checkpoints");
    }
    for entry in &listing.entries {
        let current = if entry.model == listing.model {
            "  (the model as it is now)"
        } else {
            ""
        };
        println!(
            "{}  {}  {} bytes  {}{current}",
            entry.id, entry.created_at, entry.bytes, entry.name
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// The version a write is asked about: the document SOURCE is now, with the
/// content the caller says it has.
fn expected(source: &std::path::Path, content: ContentHash) -> Result<DocumentVersion> {
    let listing = list_checkpoints(source)?;
    Ok(DocumentVersion {
        document_id: listing.document_id,
        content,
    })
}

fn require_utf8(json: bool, source: &std::path::Path, output: &std::path::Path) -> Result<()> {
    if json {
        // Before anything is published: an unrepresentable output must not turn
        // a published copy into an apparent refusal.
        json::require_utf8_path(source)?;
        json::require_utf8_path(output)?;
    }
    Ok(())
}

pub fn create_result(args: &CreateCheckpointArgs) -> Result<CheckpointCopy> {
    require_utf8(args.json, &args.source, &args.output)?;
    ferritecad_jobs::create_checkpoint_copy(
        &CreateCheckpointRequest {
            source: args.source.clone(),
            expected: expected(&args.source, args.expect_version)?,
            name: args.name.clone(),
            destination: args.output.clone(),
        },
        &OperationContext::default(),
    )
}

pub fn delete_result(args: &DeleteCheckpointArgs) -> Result<CheckpointCopy> {
    require_utf8(args.json, &args.source, &args.output)?;
    ferritecad_jobs::delete_checkpoint_copy(
        &DeleteCheckpointRequest {
            source: args.source.clone(),
            expected: expected(&args.source, args.expect_version)?,
            checkpoint: args.checkpoint,
            destination: args.output.clone(),
        },
        &OperationContext::default(),
    )
}

pub fn extract_result(args: &ExtractCheckpointArgs) -> Result<CheckpointCopy> {
    require_utf8(args.json, &args.source, &args.output)?;
    let expected = match args.expect_version {
        Some(content) => Some(expected(&args.source, content)?),
        None => None,
    };
    ferritecad_jobs::extract_checkpoint(
        &ExtractCheckpointRequest {
            source: args.source.clone(),
            expected,
            checkpoint: args.checkpoint,
            destination: args.output.clone(),
        },
        &OperationContext::default(),
    )
}

pub fn run_create(args: CreateCheckpointArgs) -> Result<ExitCode> {
    if args.json {
        return Ok(json::emit(
            json::Operation::CreateCheckpoint,
            create_result(&args).map(json::CheckpointCopied::from),
        ));
    }
    let copy = create_result(&args)?;
    println!(
        "created checkpoint {} \"{}\" in {} ({})",
        copy.checkpoint.id,
        copy.checkpoint.name,
        copy.destination.display(),
        copy.document_id
    );
    Ok(ExitCode::SUCCESS)
}

pub fn run_delete(args: DeleteCheckpointArgs) -> Result<ExitCode> {
    if args.json {
        return Ok(json::emit(
            json::Operation::DeleteCheckpoint,
            delete_result(&args).map(json::CheckpointCopied::from),
        ));
    }
    let copy = delete_result(&args)?;
    println!(
        "deleted checkpoint {} \"{}\" in {} ({})",
        copy.checkpoint.id,
        copy.checkpoint.name,
        copy.destination.display(),
        copy.document_id
    );
    Ok(ExitCode::SUCCESS)
}

pub fn run_extract(args: ExtractCheckpointArgs) -> Result<ExitCode> {
    if args.json {
        return Ok(json::emit(
            json::Operation::ExtractCheckpoint,
            extract_result(&args).map(json::CheckpointCopied::from),
        ));
    }
    let copy = extract_result(&args)?;
    println!(
        "extracted checkpoint {} \"{}\" to {} ({})",
        copy.checkpoint.id,
        copy.checkpoint.name,
        copy.destination.display(),
        copy.document_id
    );
    Ok(ExitCode::SUCCESS)
}
