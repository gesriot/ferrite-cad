// SPDX-License-Identifier: MIT
//! CLI-only projection of completed STEP import facts. No reading or geometry.

use std::path::PathBuf;
use std::process::ExitCode;

use ferritecad_exchange::{Diagnostic, Severity, Stage};
use ferritecad_jobs::{PublishedStepImport, StepImportOutcome, StepReadFacts};
use ferritecad_types::{ContentHash, DocumentId, ErrorKind, ImportedSourceId, ObjectId, Result};
use serde::Serialize;

use super::{Failure, Operation, Outcome, emit, emit_outcome, emit_with_exit};

#[derive(Serialize)]
struct ImportedStep {
    destination: PathBuf,
    document_id: DocumentId,
    object_id: ObjectId,
    source_id: ImportedSourceId,
    name: String,
    source_name: Option<String>,
    #[serde(flatten)]
    read: ReadFacts,
    step_schema: String,
    source_unit: String,
    definitions: u64,
    placements: u64,
}

#[derive(Serialize)]
struct ReadFacts {
    source_byte_len: u64,
    source_hash: ContentHash,
    importer: Importer,
    diagnostics: Vec<Finding>,
}

#[derive(Serialize)]
struct Importer {
    id: String,
    version: String,
    build: String,
}

#[derive(Serialize)]
struct Finding {
    stage: &'static str,
    severity: &'static str,
    entity: String,
    message: String,
}

// Flattened only into a reader-rejection error. Other errors keep their exact
// existing fields, and cannot accidentally acquire publication identities.
#[derive(Serialize)]
pub(super) struct ReaderRejection {
    code: RejectionCode,
    step_read: ReadFacts,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum RejectionCode {
    ReaderRejected,
}

impl From<&Diagnostic> for Finding {
    fn from(finding: &Diagnostic) -> Self {
        Self {
            stage: match finding.stage {
                Stage::Load => "load",
                Stage::Transfer => "transfer",
                Stage::Identity => "identity",
                Stage::Validation => "validation",
                _ => "unknown",
            },
            severity: match finding.severity {
                Severity::Warning => "warning",
                Severity::Fail => "fail",
                _ => "unknown",
            },
            entity: finding.entity.clone(),
            message: finding.message.clone(),
        }
    }
}

impl From<StepReadFacts> for ReadFacts {
    fn from(read: StepReadFacts) -> Self {
        Self {
            source_byte_len: read.source_byte_len,
            source_hash: read.source_hash,
            importer: Importer {
                id: read.imported_by.id,
                version: read.imported_by.version,
                build: read.imported_by.build,
            },
            diagnostics: read.diagnostics.iter().map(Finding::from).collect(),
        }
    }
}

impl From<PublishedStepImport> for ImportedStep {
    fn from(published: PublishedStepImport) -> Self {
        Self {
            destination: published.destination,
            document_id: published.document_id,
            object_id: published.object_id,
            source_id: published.source,
            name: published.name,
            source_name: published.source_name,
            read: published.read.into(),
            step_schema: published.scene.schema,
            source_unit: published.scene.source_unit,
            definitions: published.scene.definitions.len() as u64,
            placements: published.scene.instances.len() as u64,
        }
    }
}

pub fn emit_import(result: Result<StepImportOutcome>) -> ExitCode {
    match result {
        Ok(StepImportOutcome::Published(published)) => {
            let exit = if published.read.diagnostics.is_empty() {
                0
            } else {
                crate::EXIT_NOTICED
            };
            emit_with_exit(
                Operation::ImportStep,
                Ok((ImportedStep::from(*published), exit)),
            )
        }
        Ok(StepImportOutcome::Rejected(read)) => emit_outcome::<ImportedStep>(
            Operation::ImportStep,
            Outcome::Failure {
                ok: false,
                error: Failure {
                    kind: ErrorKind::Input.as_str(),
                    message: "STEP reader rejected the source; nothing was published".into(),
                    causes: Vec::new(),
                    rejection: Some(ReaderRejection {
                        code: RejectionCode::ReaderRejected,
                        step_read: read.into(),
                    }),
                },
            },
            ExitCode::from(crate::EXIT_REJECTED),
        ),
        Err(error) => emit::<ImportedStep>(Operation::ImportStep, Err(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_document::ImporterIdentity;
    use ferritecad_exchange::PersistedScene;

    #[test]
    fn step_dto_keeps_null_and_exact_ordered_diagnostics_without_domain_serialization() {
        let strings = "零\"\n\\";
        let read = StepReadFacts {
            source_byte_len: 3,
            source_hash: ContentHash::of_bytes(b"abc"),
            imported_by: ImporterIdentity {
                id: "reader".into(),
                version: "1".into(),
                build: String::new(),
            },
            diagnostics: [
                Stage::Validation,
                Stage::Load,
                Stage::Identity,
                Stage::Transfer,
            ]
            .into_iter()
            .map(|stage| Diagnostic {
                stage,
                severity: Severity::Fail,
                entity: strings.into(),
                message: strings.into(),
            })
            .chain(std::iter::once(Diagnostic {
                stage: Stage::Load,
                severity: Severity::Warning,
                entity: String::new(),
                message: String::new(),
            }))
            .collect(),
        };
        let published = PublishedStepImport {
            destination: "output.fcad".into(),
            document_id: DocumentId::new(),
            object_id: ObjectId::new(),
            source: ImportedSourceId::new(),
            name: strings.into(),
            source_name: None,
            read,
            scene: PersistedScene {
                schema: String::new(),
                source_unit: String::new(),
                definitions: Vec::new(),
                instances: Vec::new(),
            },
        };
        let value = serde_json::to_value(ImportedStep::from(published)).expect("explicit DTO");
        assert_eq!(value.get("source_name"), Some(&serde_json::Value::Null));
        assert_eq!(value["name"], strings);
        assert_eq!(value["step_schema"], "");
        assert_eq!(value["source_unit"], "");
        assert_eq!(value["importer"]["build"], "");
        assert_eq!(value["definitions"], 0);
        assert_eq!(value["placements"], 0);
        for (i, stage) in ["validation", "load", "identity", "transfer"]
            .iter()
            .enumerate()
        {
            assert_eq!(value["diagnostics"][i]["stage"], *stage);
            assert_eq!(value["diagnostics"][i]["severity"], "fail");
            assert_eq!(value["diagnostics"][i]["entity"], strings);
            assert_eq!(value["diagnostics"][i]["message"], strings);
        }
        assert_eq!(value["diagnostics"][4]["severity"], "warning");
        assert_eq!(value["diagnostics"][4]["entity"], "");
        assert_eq!(value["diagnostics"][4]["message"], "");
    }
}
