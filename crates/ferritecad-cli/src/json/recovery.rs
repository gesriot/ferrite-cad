// SPDX-License-Identifier: MIT
//! JSON v1 for `list-recovery` and `extract-recovery` (§30M).

use std::process::ExitCode;

use ferritecad_jobs::{ExtractedRecovery, RecoveryEntry, RecoveryListing, RecoveryStore};
use ferritecad_types::{ContentHash, DocumentId};
use serde::Serialize;

use super::{Failure, Operation, Outcome, emit_outcome};
use crate::recovery::ExtractFailure;

#[derive(Serialize)]
pub struct RecoveryList {
    folder: String,
    records: Vec<Record>,
    /// Records some FerriteCAD process holds exclusively (a running window, a claim
    /// or a removal): never listed, claimed or removed.
    active: usize,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum Record {
    Recoverable {
        record: String,
        name: String,
        written_unix_ms: u64,
        document_id: DocumentId,
        content_version: ContentHash,
        bytes: u64,
    },
    Refused {
        record: String,
        reason: &'static str,
        message: String,
    },
}

impl RecoveryList {
    pub fn of(store: &RecoveryStore, listing: &RecoveryListing) -> Self {
        Self {
            // Checked to be UTF-8 before the listing was read.
            folder: store.root().to_string_lossy().into_owned(),
            records: listing
                .entries
                .iter()
                .map(|entry| match entry {
                    RecoveryEntry::Recoverable(summary) => Record::Recoverable {
                        record: summary.record.to_string(),
                        name: summary.name.clone(),
                        written_unix_ms: summary.written_unix_ms,
                        document_id: summary.document_id,
                        content_version: summary.content,
                        bytes: summary.bytes,
                    },
                    RecoveryEntry::Refused(refusal) => Record::Refused {
                        record: refusal.record.to_string(),
                        reason: refusal.kind.as_str(),
                        message: refusal.error.to_string(),
                    },
                })
                .collect(),
            active: listing.active,
        }
    }
}

#[derive(Serialize)]
struct Extracted {
    record: String,
    destination: String,
    document_id: DocumentId,
    content_version: ContentHash,
    name: String,
}

/// One envelope for an extraction. A refused record carries its reason as data
/// (`recovery_refusal`), so a script never reads it out of the sentence.
pub fn emit_recovery_extract(
    result: std::result::Result<ExtractedRecovery, ExtractFailure>,
) -> ExitCode {
    let (outcome, exit) = match result {
        Ok(extracted) => (
            Outcome::Success {
                ok: true,
                result: Extracted {
                    record: extracted.record.to_string(),
                    // Checked to be UTF-8 before anything was published.
                    destination: extracted.destination.to_string_lossy().into_owned(),
                    document_id: extracted.document_id,
                    content_version: extracted.content,
                    name: extracted.name,
                },
            },
            ExitCode::SUCCESS,
        ),
        Err(failure) => {
            let (error, refusal) = match failure {
                ExtractFailure::Refused(refusal) => {
                    let kind = refusal.kind.as_str();
                    (refusal.error, Some(kind))
                }
                ExtractFailure::Failed(error) => (error, None),
            };
            let _ = crate::report(&error);
            let mut failure = Failure::from(&error);
            failure.recovery_refusal = refusal;
            (
                Outcome::Failure {
                    ok: false,
                    error: Box::new(failure),
                },
                ExitCode::from(crate::EXIT_FAILED),
            )
        }
    };
    emit_outcome(Operation::ExtractRecovery, outcome, exit)
}
