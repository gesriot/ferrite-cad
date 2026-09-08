// SPDX-License-Identifier: MIT
//! The stored facts on which a bounded extrusion edit can be confirmed.

use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result};

use crate::{Access, DependencyRole, Document, EndCondition, ObjectPayload, ObjectRecord};

/// Identity and complete content version of one consistent document reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentVersion {
    pub document_id: DocumentId,
    pub content: ContentHash,
}

/// One explicitly addressable native extrusion, including why it is unavailable.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudeChoice {
    pub feature: ObjectId,
    pub name: Option<String>,
    pub distance_mm: Option<f64>,
    pub refusal: Option<String>,
}

/// Facts carried alongside the accepted picture, never re-read by a form.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudeEditSource {
    pub version: DocumentVersion,
    pub features: Vec<ExtrudeChoice>,
    pub refusal: Option<String>,
}

impl ExtrudeEditSource {
    pub fn read(document: &Document) -> Result<Self> {
        let refusal = match document.copy_access()? {
            Access::ReadOnly { reason } => Some(reason),
            _ => None,
        };
        let features = document
            .objects()?
            .iter()
            .filter_map(|object| {
                let ObjectPayload::Extrude(extrude) = &object.payload else {
                    return None;
                };
                let distance_mm = match &extrude.end_condition {
                    EndCondition::Blind { distance } | EndCondition::Symmetric { distance } => {
                        Some(distance.value())
                    }
                    _ => None,
                };
                Some(ExtrudeChoice {
                    feature: object.id,
                    name: object.name.clone(),
                    distance_mm,
                    refusal: editable_extrude(document, object)
                        .err()
                        .map(|e| e.to_string()),
                })
            })
            .collect();
        Ok(Self {
            version: DocumentVersion {
                document_id: document.meta().document_id,
                content: document.content_version()?,
            },
            features,
            refusal,
        })
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        self.refusal.as_deref().or_else(|| {
            if self.features.is_empty() {
                Some("This document has no native extrusions.")
            } else if self.features.iter().all(|f| f.refusal.is_some()) {
                Some("No supported extrusion: a constant Blind distance is required.")
            } else {
                None
            }
        })
    }
}

/// Only a matching numeric source and stored value is a literal. No evaluator
/// is implied: expressions currently store source text and their last value.
/// A parameter edge is refused even if the text happens to look numeric.
pub fn editable_extrude(document: &Document, object: &ObjectRecord) -> Result<f64> {
    let ObjectPayload::Extrude(extrude) = &object.payload else {
        return Err(CadError::unsupported(format!(
            "object {} is {}, not a native extrusion",
            object.id,
            object.payload.type_name()
        )));
    };
    let EndCondition::Blind { distance } = &extrude.end_condition else {
        return Err(CadError::unsupported(
            "only Blind extrusions can be edited; Symmetric and ThroughAll are unsupported",
        ));
    };
    let literal = distance.source.trim().parse::<f64>().ok();
    if !literal.is_some_and(|v| v.is_finite() && v == distance.value())
        || document
            .dependencies()?
            .iter()
            .any(|dep| dep.dependent == object.id && dep.role == DependencyRole::Parameter)
    {
        return Err(CadError::unsupported(
            "extrusion distance is a formula or parameter dependency; only a numeric literal can be edited",
        ));
    }
    Ok(distance.value())
}
