// SPDX-License-Identifier: MIT
use std::collections::BTreeSet;

use ferritecad_document::Document;
use ferritecad_types::{ContentHash, ObjectId, Result, StableEntityId};

use crate::RebuildResult;

/// Kernel-free facts of one immutable model, including references with multiple results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildFacts {
    pub model: ContentHash,
    pub resolved: BTreeSet<StableEntityId>,
    pub unbuilt: BTreeSet<ObjectId>,
}

impl RebuildFacts {
    pub fn of(document: &Document, built: &RebuildResult) -> Result<Self> {
        Ok(Self {
            model: document.model_without_checkpoints()?,
            resolved: document
                .topology_refs()?
                .iter()
                .filter_map(|reference| {
                    built
                        .resolve(reference)
                        .ok()
                        .filter(|found| !found.is_empty())
                        .map(|_| reference.id)
                })
                .collect(),
            unbuilt: built.report().unbuilt(),
        })
    }
}
