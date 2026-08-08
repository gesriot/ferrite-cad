// SPDX-License-Identifier: MIT
//! Rebuilding a whole document from nothing, one feature at a time.
//!
//! Cold means no cache is consulted and none is written: every feature is
//! recomputed from the document. That is the slowest path and the one that
//! defines correctness — a cached rebuild is only allowed to agree with this
//! one faster.
//!
//! Sequential on purpose. [`RebuildPlan`](crate::RebuildPlan) already reports
//! levels that could run concurrently, but a kernel session is not shareable
//! and there is no scheduler yet; running the plan in order is the honest
//! implementation of what exists today.

use std::collections::BTreeMap;

use ferritecad_document::TopologyRef;
use ferritecad_document::{Document, ObjectPayload, ObjectRecord};
use ferritecad_kernel::{
    GeometryKernel, History, OperationContext, Profile, ShapeHandle, SketchPlane, SubShapeHandle,
};
use ferritecad_topology::TopologyMap;
use ferritecad_types::{CadError, ObjectId, Result};

use crate::convert::{extrude_request, plane_from_datum, profile_from_sketch};
use crate::document_graph::DocumentGraph;

/// The faces closing each end of an extrusion.
///
/// Reported apart from history because they are generated from no input: the
/// sweep creates them, so "generated from segment S" cannot name them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtrudeCaps {
    pub start: Vec<SubShapeHandle>,
    pub end: Vec<SubShapeHandle>,
}

/// What a cold rebuild produced.
///
/// # Handles are session-owned
///
/// The shapes named here live in the kernel session that built them. Call
/// [`ColdRebuild::release_all`] when finished; dropping this value instead
/// leaves the kernel holding the geometry until the session ends. A failed
/// rebuild needs no such care — it releases everything it made before
/// returning the error.
///
/// This value is deliberately not cloneable. A copy could release the shapes
/// while the original continued handing out handles that were no longer live.
#[derive(Debug, Default)]
pub struct ColdRebuild {
    shapes: BTreeMap<ObjectId, ShapeHandle>,
    histories: BTreeMap<ObjectId, History>,
    caps: BTreeMap<ObjectId, ExtrudeCaps>,
    profiles: BTreeMap<ObjectId, Profile>,
    topology: TopologyMap,
    order: Vec<ObjectId>,
    owned: Vec<ShapeHandle>,
}

impl ColdRebuild {
    /// The shape a feature or body resolved to.
    pub fn shape(&self, object: ObjectId) -> Option<ShapeHandle> {
        self.shapes.get(&object).copied()
    }

    /// What a feature did to its inputs.
    pub fn history(&self, object: ObjectId) -> Option<&History> {
        self.histories.get(&object)
    }

    /// The cap faces of an extrusion.
    pub fn caps(&self, object: ObjectId) -> Option<&ExtrudeCaps> {
        self.caps.get(&object)
    }

    /// The profile a sketch converted to.
    pub fn profile(&self, object: ObjectId) -> Option<&Profile> {
        self.profiles.get(&object)
    }

    /// What each feature's output is called, for this session only.
    ///
    /// Read-only: the mapping is built while the geometry is, and nothing
    /// outside may add to it. It holds kernel handles, which implement no
    /// serialisation, so it cannot reach a document even by accident.
    pub fn topology(&self) -> &TopologyMap {
        &self.topology
    }

    /// Resolves a reference the document stores against this rebuild.
    ///
    /// Fails rather than approximating; see
    /// [`ferritecad_topology::resolve`] for what each failure means.
    pub fn resolve(&self, reference: &TopologyRef) -> Result<Vec<SubShapeHandle>> {
        ferritecad_topology::resolve(&self.topology, reference)
    }

    /// The objects that were evaluated, in the order they were evaluated.
    pub fn order(&self) -> &[ObjectId] {
        &self.order
    }

    /// How many shapes this rebuild created and still holds.
    pub fn shape_count(&self) -> usize {
        self.owned.len()
    }

    /// Hands every shape back to the kernel.
    pub fn release_all(self, kernel: &mut dyn GeometryKernel) {
        // Later features may share storage with their inputs in a real kernel,
        // so unwind ownership in the opposite order from construction.
        for shape in self.owned.into_iter().rev() {
            kernel.release(shape);
        }
    }
}

/// Rebuilds every object in `document` from scratch.
///
/// Fails on the first feature that cannot be built, having released whatever it
/// had already made. A half-built rebuild is worse than none: it would leave
/// the caller holding shapes it cannot name and a model it cannot trust.
pub fn rebuild_cold(
    document: &Document,
    kernel: &mut dyn GeometryKernel,
    context: &OperationContext,
) -> Result<ColdRebuild> {
    // Do this before reading or validating the document. Besides avoiding
    // needless work, it makes cancellation observable for an empty document,
    // whose evaluation loop has no feature boundary at which to check it.
    context.check_cancelled()?;

    let mut state = ColdRebuild::default();

    match run(document, kernel, context, &mut state) {
        Ok(()) => Ok(state),
        Err(error) => {
            state.release_all(kernel);
            Err(error)
        }
    }
}

fn run(
    document: &Document,
    kernel: &mut dyn GeometryKernel,
    context: &OperationContext,
    state: &mut ColdRebuild,
) -> Result<()> {
    ensure_rebuildable(document)?;

    let graph = DocumentGraph::read(document)?;
    let plan = graph.plan_full()?;

    let objects: BTreeMap<ObjectId, ObjectRecord> = document
        .objects()?
        .into_iter()
        .map(|object| (object.id, object))
        .collect();

    let mut planes: BTreeMap<ObjectId, SketchPlane> = BTreeMap::new();

    for id in plan.order() {
        // Checked between features rather than only inside the kernel: a
        // document of many cheap features must still stop promptly.
        context.check_cancelled()?;

        let object = objects.get(id).ok_or_else(|| {
            CadError::input(format!(
                "the plan names {id}, which the document does not hold"
            ))
        })?;

        match &object.payload {
            ObjectPayload::DatumPlane(datum) => {
                planes.insert(*id, plane_from_datum(datum)?);
            }

            ObjectPayload::Sketch(sketch) => {
                let plane = planes.get(&sketch.plane).copied().ok_or_else(|| {
                    CadError::input(format!(
                        "sketch {id} is placed on {}, which is not a datum plane",
                        sketch.plane
                    ))
                })?;
                // Converted here rather than when the extrude asks for it, so a
                // malformed sketch is reported against the sketch.
                state
                    .profiles
                    .insert(*id, profile_from_sketch(sketch, plane)?);
            }

            ObjectPayload::Extrude(feature) => {
                let profile = state
                    .profiles
                    .get(&feature.profile)
                    .cloned()
                    .ok_or_else(|| {
                        CadError::input(format!(
                            "extrude {id} reads {}, which produced no profile",
                            feature.profile
                        ))
                    })?;

                let request = extrude_request(feature, profile)?;
                let result = kernel.extrude(&request, context)?;

                // Register ownership before checking again: a kernel can
                // finish the operation and then invoke a progress callback
                // that cancels the rebuild. The resulting shape still belongs
                // to us and must participate in error cleanup.
                state.owned.push(result.shape);
                context.check_cancelled()?;

                // Named while the result is still whole. The correspondence
                // between a segment and the face it raised lives across the
                // history and the caps, and reassembling it from the separate
                // fields below would be inventing it rather than recording it.
                state
                    .topology
                    .record_extrude(*id, request.profile(), &result)?;

                state.shapes.insert(*id, result.shape);
                state.histories.insert(*id, result.history);
                state.caps.insert(
                    *id,
                    ExtrudeCaps {
                        start: result.start_cap,
                        end: result.end_cap,
                    },
                );
            }

            ObjectPayload::Body(body) => {
                // A body in this slice is a name for its tip feature's result.
                // It creates nothing, so it owns nothing to release.
                if let Some(tip) = body.tip_feature {
                    let shape = state.shapes.get(&tip).copied().ok_or_else(|| {
                        CadError::input(format!(
                            "body {id} names tip feature {tip}, which produced no shape"
                        ))
                    })?;
                    state.shapes.insert(*id, shape);
                }
            }

            // Parameters carry no geometry. Expressions arrive with their own
            // stage; evaluating one here would be inventing a semantics.
            ObjectPayload::Parameter(_) => {}

            ObjectPayload::Unknown(unknown) => {
                return Err(CadError::unsupported(format!(
                    "object {id} has type {:?}, which this build preserves but cannot rebuild",
                    unknown.type_name
                )));
            }

            other => {
                return Err(CadError::unsupported(format!(
                    "object {id} has payload {}, which the cold evaluator does not implement",
                    other.type_name()
                )));
            }
        }

        state.order.push(*id);
    }

    // Covers a cancellation concurrent with the final non-kernel feature.
    context.check_cancelled()?;
    Ok(())
}

fn ensure_rebuildable(document: &Document) -> Result<()> {
    let report = document.validate()?;
    if report.is_ok() {
        return Ok(());
    }

    let errors = report
        .errors()
        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
        .collect::<Vec<_>>()
        .join("; ");
    Err(CadError::input(format!(
        "document validation failed before cold rebuild: {errors}"
    )))
}
