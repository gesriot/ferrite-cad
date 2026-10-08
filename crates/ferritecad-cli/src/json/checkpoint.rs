// SPDX-License-Identifier: MIT
//! JSON v1 for the §30N checkpoint commands. Additive: four new `operation`
//! values in the existing envelope; nothing else in v1 changes.

use std::path::PathBuf;

use ferritecad_document::CheckpointEntry;
use ferritecad_jobs::{CheckpointCopy, CheckpointListing};
use ferritecad_types::{CheckpointId, ContentHash, DocumentId};
use serde::Serialize;

#[derive(Serialize)]
pub struct CheckpointList {
    document_id: DocumentId,
    content_version: ContentHash,
    checkpoints: Vec<Checkpoint>,
}

#[derive(Serialize)]
struct Checkpoint {
    checkpoint_id: CheckpointId,
    name: String,
    /// SQLite UTC time it was made, `YYYY-MM-DDTHH:MM:SS.SSSZ`.
    created_at: String,
    bytes: u64,
    /// Whether this checkpoint holds the model the source holds now.
    holds_current_model: bool,
}

impl Checkpoint {
    fn of(entry: &CheckpointEntry, current: ContentHash) -> Self {
        Self {
            checkpoint_id: entry.id,
            name: entry.name.clone(),
            created_at: entry.created_at.clone(),
            bytes: entry.bytes,
            holds_current_model: current == entry.model,
        }
    }
}

impl From<CheckpointListing> for CheckpointList {
    fn from(listing: CheckpointListing) -> Self {
        Self {
            document_id: listing.document_id,
            content_version: listing.content_version,
            checkpoints: listing
                .entries
                .iter()
                .map(|entry| Checkpoint::of(entry, listing.model))
                .collect(),
        }
    }
}

/// What `create-checkpoint`, `delete-checkpoint` and `extract-checkpoint`
/// published, and which checkpoint it was about.
#[derive(Serialize)]
pub struct CheckpointCopied {
    destination: PathBuf,
    document_id: DocumentId,
    /// The published file's complete content version.
    content_version: ContentHash,
    checkpoint: CheckpointFacts,
}

#[derive(Serialize)]
struct CheckpointFacts {
    checkpoint_id: CheckpointId,
    name: String,
    created_at: String,
    bytes: u64,
}

impl From<CheckpointCopy> for CheckpointCopied {
    fn from(copy: CheckpointCopy) -> Self {
        Self {
            destination: copy.destination,
            document_id: copy.document_id,
            content_version: copy.content_version,
            checkpoint: CheckpointFacts {
                checkpoint_id: copy.checkpoint.id,
                name: copy.checkpoint.name,
                created_at: copy.checkpoint.created_at,
                bytes: copy.checkpoint.bytes,
            },
        }
    }
}
