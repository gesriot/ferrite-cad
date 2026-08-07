// SPDX-License-Identifier: MIT
use std::collections::BTreeSet;

use ferritecad_document::{Dependency, Document};
use ferritecad_types::{ObjectId, Result};

use crate::dirty::DependentIndex;
use crate::plan::{RebuildPlan, plan_full_rebuild, plan_rebuild};

/// A document's graph, read once and then queried many times.
///
/// Reading it costs a decode of every object payload, so planning against a
/// live [`Document`] on each keystroke would decode the whole model on each
/// keystroke. This snapshot exists to make that cost explicit and payable once.
///
/// It is a snapshot, not a view: it does not observe later edits to the
/// document. Take a fresh one after the graph changes shape. That is the
/// intended usage — an edit invalidates the plan anyway.
#[derive(Debug, Clone)]
pub struct DocumentGraph {
    nodes: Vec<ObjectId>,
    dependencies: Vec<Dependency>,
}

impl DocumentGraph {
    /// Reads the objects and edges out of an open document.
    pub fn read(document: &Document) -> Result<Self> {
        Ok(Self {
            nodes: document.objects()?.into_iter().map(|o| o.id).collect(),
            dependencies: document.dependencies()?,
        })
    }

    /// Builds a graph from parts, for callers holding them already.
    pub fn new(nodes: Vec<ObjectId>, dependencies: Vec<Dependency>) -> Self {
        Self {
            nodes,
            dependencies,
        }
    }

    pub fn nodes(&self) -> &[ObjectId] {
        &self.nodes
    }

    pub fn dependencies(&self) -> &[Dependency] {
        &self.dependencies
    }

    /// The reverse index, for callers propagating staleness repeatedly.
    pub fn dependent_index(&self) -> Result<DependentIndex> {
        DependentIndex::build(&self.nodes, &self.dependencies)
    }

    /// Everything made stale by `changed`, including `changed` itself.
    pub fn dirty_set(&self, changed: &[ObjectId]) -> Result<BTreeSet<ObjectId>> {
        self.dependent_index()?.dirty_set(changed)
    }

    /// What to rebuild after `changed` were edited.
    pub fn plan(&self, changed: &[ObjectId]) -> Result<RebuildPlan> {
        plan_rebuild(&self.nodes, &self.dependencies, changed)
    }

    /// What to rebuild with no cache at all.
    pub fn plan_full(&self) -> Result<RebuildPlan> {
        plan_full_rebuild(&self.nodes, &self.dependencies)
    }
}
