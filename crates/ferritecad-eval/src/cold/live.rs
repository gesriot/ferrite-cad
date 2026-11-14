// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_kernel::KernelIdentity;
use ferritecad_types::CanonicalHasher;
use std::collections::BTreeSet;

pub(super) enum Source<'a> {
    Cold,
    Sidecar(&'a mut CacheStore),
    Live(Option<&'a RebuildResult>, &'a KernelIdentity),
}

impl Source<'_> {
    pub(super) fn identity<'a>(
        &'a self,
        kernel: &'a (impl GeometryKernel + ?Sized),
    ) -> &'a KernelIdentity {
        match self {
            Self::Live(_, identity) => identity,
            _ => kernel.identity(),
        }
    }

    pub(super) fn feature(
        &mut self,
        kernel: &mut (impl GeometryKernel + ?Sized),
        context: &OperationContext,
        key: ContentHash,
        owner: ObjectId,
        state: &mut RebuildResult,
        events: &mut Vec<CacheEvent>,
    ) -> Result<bool> {
        match self {
            Self::Sidecar(cache) => restore(kernel, cache, context, key, owner, state, events),
            Self::Live(Some(old), _) if old.keys.get(&owner) == Some(&key) => {
                let names = old.topology.features[&owner].clone();
                state.topology.insert_feature(owner, names)?;
                state.shapes.insert(owner, old.shapes[&owner]);
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

pub(super) fn node_key(
    object: &ObjectRecord,
    graph: &DocumentGraph,
    state: &RebuildResult,
    identity: &KernelIdentity,
    context: &OperationContext,
) -> Result<ContentHash> {
    let mut h = CanonicalHasher::new("eval.live.node");
    identity.feed(&mut h);
    context.tolerance().feed(&mut h);
    h.bytes(&object.id.to_bytes())
        .hash(&object.evaluated_payload_hash()?);
    for edge in graph
        .dependencies()
        .iter()
        .filter(|e| e.dependent == object.id)
    {
        h.str(edge.role.as_str())
            .bytes(&edge.dependency.to_bytes())
            .hash(state.keys.get(&edge.dependency).ok_or_else(|| {
                CadError::input(format!(
                    "object {} has an unavailable input {}",
                    object.id, edge.dependency
                ))
            })?);
    }
    Ok(h.finish())
}

impl RebuildResult {
    pub(super) fn reuse_node(&mut self, id: ObjectId, old: &Self) {
        if let Some(shape) = old.shape(id) {
            self.shapes.insert(id, shape);
        }
        if let Some(plane) = old.plane(id) {
            self.planes.insert(id, plane);
        }
        if let Some(profile) = old.profile(id) {
            self.profiles.insert(id, profile.clone());
        }
        if let Some(reason) = old.profile_failures.get(&id) {
            self.profile_failures
                .insert(id, std::sync::Arc::clone(reason));
        }
        if let Some(presentation) = old.sketch_presentation(id) {
            self.presentations.insert(id, presentation.clone());
        }
        if let Some(report) = old.solve_report(id) {
            self.solve_reports.insert(id, report.clone());
        }
    }

    pub fn result_key(&self, id: ObjectId) -> Option<ContentHash> {
        self.keys.get(&id).copied()
    }
}

/// Evaluations in one kernel session. Only newly built shapes belong to the
/// pending evaluation until commit transfers its borrowed survivors.
#[derive(Debug, Default)]
pub struct LiveModel {
    identity: Option<KernelIdentity>,
    accepted: Option<RebuildResult>,
    candidate: Option<RebuildResult>,
}

impl LiveModel {
    pub fn prepare(
        &mut self,
        document: &Document,
        kernel: &mut (impl GeometryKernel + ?Sized),
        context: &OperationContext,
        baseline: Option<&BTreeSet<ObjectId>>,
    ) -> Result<&RebuildResult> {
        if self.candidate.is_some() {
            return Err(CadError::input(
                "finish the pending revision before preparing another",
            ));
        }
        let identity = self
            .identity
            .get_or_insert_with(|| kernel.identity().clone());
        let source = Source::Live(self.accepted.as_ref(), identity);
        let (built, _) = rebuild(document, kernel, source, context)?;
        let built = match baseline {
            Some(unbuilt) => built.require_no_new_failures(unbuilt, kernel)?,
            None => built,
        };
        self.candidate = Some(built);
        Ok(self.prepared())
    }

    /// The candidate being staged for display in its owning kernel session.
    pub fn prepared(&self) -> &RebuildResult {
        self.candidate.as_ref().expect("prepared revision")
    }

    pub fn accept(&mut self, kernel: &mut (impl GeometryKernel + ?Sized)) {
        let mut next = self.candidate.take().expect("prepared revision");
        if let Some(mut old) = self.accepted.take() {
            let needed: BTreeSet<_> = next.shapes.values().copied().collect();
            let (shared, obsolete): (Vec<_>, Vec<_>) =
                old.owned.into_iter().partition(|h| needed.contains(h));
            old.owned = obsolete;
            old.release_all(kernel);
            next.owned.splice(..0, shared);
        }
        self.accepted = Some(next);
    }

    pub fn discard(&mut self, kernel: &mut (impl GeometryKernel + ?Sized)) {
        if let Some(next) = self.candidate.take() {
            next.release_all(kernel);
        }
    }

    pub fn release(&mut self, kernel: &mut (impl GeometryKernel + ?Sized)) {
        self.discard(kernel);
        if let Some(old) = self.accepted.take() {
            old.release_all(kernel);
        }
    }
}
