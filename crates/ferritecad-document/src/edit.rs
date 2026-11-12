// SPDX-License-Identifier: MIT
//! Stored editing facts carried alongside the accepted picture.
use crate::{Access, Document, ObjectRecord};
use ferritecad_types::{ContentHash, DocumentId, ObjectId, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentVersion {
    pub document_id: DocumentId,
    pub content: ContentHash,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudeEditSource {
    pub objects: Vec<ObjectRecord>,
    /// Saved parameter dependency rows, with their stated input slots.
    pub parameter_consumers: std::result::Result<Vec<ParameterConsumer>, String>,
    /// Every object in the accepted rebuild's failed and skipped lists, with its tree status.
    /// These include expression and geometry failures, not just failed parameter values.
    pub unbuilt_statuses: Vec<(ObjectId, String)>,
    pub model_tree: crate::ModelTree,
    pub version: DocumentVersion,
    pub expression_slots: Vec<crate::ExpressionSlotValue>,
    pub revolves: Vec<crate::RevolveChoice>,
    pub refusal: Option<String>,
}

/// One stated use of a parameter, backed by a saved Parameter dependency row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterConsumer {
    /// The parameter being read.
    pub parameter: ObjectId,
    /// The object containing the expression.
    pub object: ObjectId,
    /// The input field; `None` denotes another parameter's expression.
    pub slot: Option<crate::ExpressionSlot>,
}

impl ExtrudeEditSource {
    pub fn read(document: &Document) -> Result<Self> {
        let objects = document.objects()?;
        Ok(Self {
            parameter_consumers: document
                .dependencies()
                .and_then(|edges| crate::parameters::consumers(&objects, &edges))
                .map_err(|error| error.to_string()),
            unbuilt_statuses: Vec::new(),
            model_tree: crate::ModelTree::from_dependencies(&objects, document.dependencies()),
            objects: objects.clone(),
            version: DocumentVersion {
                document_id: document.meta().document_id,
                content: document.content_version()?,
            },
            expression_slots: document.expression_slots()?,
            revolves: crate::revolve_choices(&objects),
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
