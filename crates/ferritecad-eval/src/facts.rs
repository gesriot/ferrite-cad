// SPDX-License-Identifier: MIT
use std::collections::BTreeSet;

use ferritecad_document::Document;
use ferritecad_types::{ContentHash, ObjectId, Result};

use crate::RebuildResult;

/// Kernel-free facts of one immutable model, including unbuilt features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildFacts {
    pub model: ContentHash,
    pub unbuilt: BTreeSet<ObjectId>,
}

impl RebuildFacts {
    pub fn of(document: &Document, built: &RebuildResult) -> Result<Self> {
        Ok(Self {
            model: document.model_without_checkpoints()?,
            unbuilt: built.report().unbuilt(),
        })
    }
}
