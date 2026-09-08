// SPDX-License-Identifier: MIT
//! Explicit CLI v1 projection of the published writer report, not the scene.

use std::path::PathBuf;

use ferritecad_exchange::{Severity, Stage};
use ferritecad_export::{ExportOmissionReport, ExportSource};
use ferritecad_jobs::FbxExport;
use ferritecad_types::{ImportedSourceId, ObjectId};
use serde::Serialize;

#[derive(Serialize)]
pub struct ExportedFbx {
    destination: PathBuf,
    bytes: u64,
    models: u32,
    geometries: u32,
    materials: u32,
    complete: bool,
    omissions: Vec<Omission>,
}

impl From<&FbxExport> for ExportedFbx {
    fn from(exported: &FbxExport) -> Self {
        let report = exported.report();
        Self {
            destination: exported.destination().to_path_buf(),
            bytes: report.bytes(),
            models: report.models(),
            geometries: report.geometries(),
            materials: report.materials(),
            complete: report.is_complete(),
            omissions: report.omissions().iter().map(Omission::from).collect(),
        }
    }
}

#[derive(Serialize)]
struct Omission {
    source: Source,
    finding: Finding,
    refusal: &'static str,
    placements: Vec<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Source {
    Body {
        body_id: ObjectId,
    },
    Imported {
        source_id: ImportedSourceId,
        definition_key: String,
    },
}

#[derive(Serialize)]
struct Finding {
    stage: &'static str,
    severity: &'static str,
    entity: String,
    message: String,
}

impl From<&ExportOmissionReport> for Omission {
    fn from(report: &ExportOmissionReport) -> Self {
        let source = match &report.source {
            ExportSource::Body { object } => Source::Body { body_id: *object },
            ExportSource::Imported {
                source,
                definition_key,
            } => Source::Imported {
                source_id: *source,
                definition_key: definition_key.clone(),
            },
        };
        let finding = &report.omission.finding;
        Self {
            source,
            finding: Finding {
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
            },
            refusal: report.omission.refusal.stable_name(),
            // These are file-local FerriteCADNodeKey values. The report does
            // not carry durable occurrence IDs; do not manufacture any.
            placements: report
                .nodes
                .iter()
                .map(|node| format!("node/{}", node.index()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_exchange::Diagnostic;
    use ferritecad_export::{
        ExportDefinitionIdentity, ExportGeometry, ExportOccurrence, ExportOmission,
        ExportProvenance, ExportSceneBuilder, ExportTransform,
    };
    use ferritecad_kernel::TessellationRefusal;

    #[test]
    fn omissions_keep_source_domains_finding_fields_and_every_file_local_placement() {
        let sources = [ImportedSourceId::new(), ImportedSourceId::new()];
        let body = ObjectId::new();
        let key = "key/Я \"same\"\nkey";
        let message = "diagnostic \"Я\"\n\\tail";
        let mut builder = ExportSceneBuilder::new();
        for imported in [Some(sources[0]), Some(sources[1]), None] {
            let (source, identity) = if let Some(source) = imported {
                (
                    ExportSource::Imported {
                        source,
                        definition_key: key.into(),
                    },
                    ExportDefinitionIdentity::Source {
                        source,
                        definition_key: key.into(),
                    },
                )
            } else {
                (
                    ExportSource::Body { object: body },
                    ExportDefinitionIdentity::Object(body),
                )
            };
            let definition = builder
                .definition(
                    source,
                    identity,
                    Some(message.into()),
                    ExportProvenance::default(),
                    ExportGeometry::Omitted(ExportOmission::new(
                        Diagnostic {
                            stage: Stage::Validation,
                            severity: Severity::Warning,
                            entity: key.into(),
                            message: message.into(),
                        },
                        TessellationRefusal::IncompleteFace,
                    )),
                )
                .expect("definition");
            for _ in 0..2 {
                builder
                    .node(
                        None,
                        definition,
                        ExportTransform::IDENTITY,
                        Some(message.into()),
                        None,
                        ExportOccurrence::Unrecorded,
                    )
                    .expect("legacy unrecorded placement");
            }
        }
        let scene = builder.finish().expect("scene");
        let report = ferritecad_export::write_fbx_ascii_7400(&scene, &mut Vec::new())
            .expect("writer report");
        let rows: Vec<_> = report.omissions().iter().map(Omission::from).collect();
        let value = serde_json::to_value(rows).expect("normal serializer");
        assert_eq!(value.as_array().expect("array").len(), 3);
        for i in 0..3 {
            let row = &value[i];
            if i < 2 {
                assert_eq!(row["source"]["kind"], "imported");
                assert_eq!(row["source"]["source_id"], sources[i].to_string());
                assert_eq!(row["source"]["definition_key"], key);
                assert!(row["source"].get("body_id").is_none());
            } else {
                assert_eq!(row["source"]["kind"], "body");
                assert_eq!(row["source"]["body_id"], body.to_string());
                assert!(row["source"].get("source_id").is_none());
            }
            assert_eq!(
                row["finding"],
                serde_json::json!({"stage":"validation", "severity":"warning", "entity":key, "message":message})
            );
            assert_eq!(row["refusal"], "IncompleteFace");
            assert_eq!(
                row["placements"],
                serde_json::json!([format!("node/{}", i * 2), format!("node/{}", i * 2 + 1)])
            );
            assert!(
                row.get("occurrence_id").is_none(),
                "do not invent recorded identity"
            );
        }
        let mut entry = report.omissions()[0].clone();
        for (stage, wire) in [
            (Stage::Load, "load"),
            (Stage::Transfer, "transfer"),
            (Stage::Identity, "identity"),
            (Stage::Validation, "validation"),
        ] {
            entry.omission.finding.stage = stage;
            entry.omission.finding.severity = Severity::Fail;
            entry.omission.finding.entity.clear();
            let row = serde_json::to_value(Omission::from(&entry)).expect("finding");
            assert_eq!(row["finding"]["stage"], wire);
            assert_eq!(row["finding"]["severity"], "fail");
            assert_eq!(row["finding"]["entity"], "");
        }
    }
}
