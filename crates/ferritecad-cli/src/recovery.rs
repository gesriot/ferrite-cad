// SPDX-License-Identifier: MIT
//! `list-recovery` and `extract-recovery` (§30M): the crash copies a window left,
//! read through the same library claim the window recovers from.
//!
//! Neither command takes a record over. Listing changes nothing; extraction holds
//! the record's lease only while it copies, publishes a new file by the shared
//! no-clobber publication, and leaves the record in place.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use ferritecad_jobs::{
    ExtractedRecovery, RecordId, RecoveryEntry, RecoveryListing, RecoveryRefusal, RecoveryStore,
    format_utc,
};
use ferritecad_types::Result;

use crate::json;

#[derive(Debug, Args)]
pub struct ListRecoveryArgs {
    /// The recovery folder. Default: FERRITECAD_RECOVERY_DIR, else the per-user one.
    #[arg(long)]
    recovery_dir: Option<PathBuf>,
    /// Emit one JSON v1 result. Argument errors remain clap text. Paths must be UTF-8.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub struct ExtractRecoveryArgs {
    /// The record's UUID, as list-recovery prints it.
    record: RecordId,
    /// New .fcad path. Existing files are refused; there is no --force.
    #[arg(short, long)]
    output: PathBuf,
    /// The recovery folder. Default: FERRITECAD_RECOVERY_DIR, else the per-user one.
    #[arg(long)]
    recovery_dir: Option<PathBuf>,
    /// Emit one JSON v1 result or execution error. Argument errors remain clap text.
    /// Paths must be UTF-8 in JSON mode.
    #[arg(long)]
    json: bool,
}

fn store(chosen: Option<&PathBuf>) -> Result<RecoveryStore> {
    match chosen {
        Some(root) => RecoveryStore::at(root),
        None => RecoveryStore::at(&ferritecad_jobs::default_recovery_root()?),
    }
}

/// Both presentations read one listing.
pub fn list_result(args: &ListRecoveryArgs) -> Result<(RecoveryStore, RecoveryListing)> {
    if args.json
        && let Some(root) = &args.recovery_dir
    {
        json::require_utf8_path(root)?;
    }
    let store = store(args.recovery_dir.as_ref())?;
    if args.json {
        json::require_utf8_path(store.root())?;
    }
    let listing = store.list()?;
    Ok((store, listing))
}

pub fn run_list(args: ListRecoveryArgs) -> Result<ExitCode> {
    if args.json {
        return Ok(json::emit(
            json::Operation::ListRecovery,
            list_result(&args).map(|(store, listing)| json::RecoveryList::of(&store, &listing)),
        ));
    }
    let (store, listing) = list_result(&args)?;
    if listing.entries.is_empty() {
        println!("no recovery copies in {}", store.root().display());
    }
    for entry in &listing.entries {
        match entry {
            RecoveryEntry::Recoverable(summary) => println!(
                "{}  recoverable  {}  {}",
                summary.record,
                format_utc(summary.written_unix_ms),
                summary.name
            ),
            RecoveryEntry::Refused(refusal) => println!(
                "{}  refused ({})  {}",
                refusal.record,
                refusal.kind.as_str(),
                refusal.error
            ),
        }
    }
    if listing.active > 0 {
        println!(
            "{} more held by FerriteCAD windows that are running",
            listing.active
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// What an extraction did, or why it did not.
pub enum ExtractFailure {
    Refused(RecoveryRefusal),
    Failed(ferritecad_types::CadError),
}

impl From<ferritecad_types::CadError> for ExtractFailure {
    fn from(error: ferritecad_types::CadError) -> Self {
        Self::Failed(error)
    }
}

pub fn extract_result(
    args: &ExtractRecoveryArgs,
) -> std::result::Result<ExtractedRecovery, ExtractFailure> {
    if args.json {
        // Before anything is published: an unrepresentable path must not turn a
        // finished extraction into an apparent refusal.
        json::require_utf8_path(&args.output)?;
        if let Some(root) = &args.recovery_dir {
            json::require_utf8_path(root)?;
        }
    }
    let store = store(args.recovery_dir.as_ref())?;
    let claim = store.claim(args.record).map_err(ExtractFailure::Refused)?;
    // The claim is let go when this returns; the record stays where it is.
    Ok(claim.extract_to(&args.output)?)
}

pub fn run_extract(args: ExtractRecoveryArgs) -> Result<ExitCode> {
    if args.json {
        return Ok(json::emit_recovery_extract(extract_result(&args)));
    }
    match extract_result(&args) {
        Ok(extracted) => {
            println!(
                "extracted {} to {} ({})",
                extracted.record,
                extracted.destination.display(),
                extracted.document_id
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(ExtractFailure::Refused(refusal)) => Err(refusal.into()),
        Err(ExtractFailure::Failed(error)) => Err(error),
    }
}
