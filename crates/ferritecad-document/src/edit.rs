// SPDX-License-Identifier: MIT
//! Stored editing facts carried alongside the accepted picture.
use crate::{Access, DependencyRole, Document, EndCondition, ObjectPayload, ObjectRecord};
use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentVersion {
    pub document_id: DocumentId,
    pub content: ContentHash,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudeEditSource {
    pub objects: Vec<ObjectRecord>,
    pub model_tree: crate::ModelTree,
    pub version: DocumentVersion,
    pub expression_slots: Vec<crate::ExpressionSlotValue>,
    pub revolves: Vec<crate::RevolveChoice>,
    pub refusal: Option<String>,
}

impl ExtrudeEditSource {
    pub fn read(document: &Document) -> Result<Self> {
        let objects = document.objects()?;
        Ok(Self {
            model_tree: crate::ModelTree::from_dependencies(&objects, document.dependencies()),
            objects: objects.clone(),
            version: DocumentVersion {
                document_id: document.meta().document_id,
                content: document.content_version()?,
            },
            expression_slots: document.expression_slots()?,
            revolves: crate::revolve_choices(document, &objects),
            refusal: match document.copy_access()? {
                Access::ReadOnly { reason } => Some(reason),
                _ => None,
            },
        })
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        self.refusal.as_deref().or_else(|| {
            (!self
                .expression_slots
                .iter()
                .any(|s| s.slot == crate::ExpressionSlot::ExtrudeDistance))
            .then_some("This document has no extrusion distance.")
        })
    }

    pub fn expression_slot(
        &self,
        object: ObjectId,
        slot: crate::ExpressionSlot,
    ) -> Option<&crate::ExpressionSlotValue> {
        self.expression_slots
            .iter()
            .find(|s| s.object == object && s.slot == slot)
    }
}

/// Only a matching numeric source and stored value is a literal. No evaluator
/// is implied: expressions currently store source text and their last value.
/// A parameter edge is refused even if the text happens to look numeric.
pub fn editable_extrude(document: &Document, object: &ObjectRecord) -> Result<f64> {
    let distance = blind_literal_distance(object)?;
    // One selected feature needs only the original scan, not a new index.
    let parameterized = document
        .dependencies()?
        .iter()
        .any(|dep| dep.dependent == object.id && dep.role == DependencyRole::Parameter);
    without_parameter_dependency(distance, parameterized)
}

fn blind_literal_distance(object: &ObjectRecord) -> Result<f64> {
    let ObjectPayload::Extrude(extrude) = &object.payload else {
        return Err(CadError::unsupported(format!(
            "object {} is {}, not a native extrusion",
            object.id,
            object.payload.type_name()
        )));
    };
    if extrude.operation != crate::SolidOperation::NewBody || extrude.previous.is_some() {
        return Err(CadError::unsupported(
            "only NewBody extrusion distances can be edited; editing a Cut is not implemented",
        ));
    }
    let EndCondition::Blind { distance } = &extrude.end_condition else {
        return Err(CadError::unsupported(
            "only Blind extrusions can be edited; Symmetric and ThroughAll are unsupported",
        ));
    };
    let literal = distance.source.trim().parse::<f64>().ok();
    if !literal.is_some_and(|v| v.is_finite() && v == distance.value()) {
        return Err(formula_refusal());
    }
    Ok(distance.value())
}

fn without_parameter_dependency(distance: f64, parameterized: bool) -> Result<f64> {
    if parameterized {
        Err(formula_refusal())
    } else {
        Ok(distance)
    }
}

fn formula_refusal() -> CadError {
    CadError::unsupported(
        "extrusion distance is a formula or parameter dependency; only a numeric literal can be edited",
    )
}
