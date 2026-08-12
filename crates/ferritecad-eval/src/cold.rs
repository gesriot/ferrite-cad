// SPDX-License-Identifier: MIT
//! Rebuilding a whole document, one feature at a time.
//!
//! Two entry points share one loop. [`rebuild_cold`] consults no cache and
//! writes none: every feature is recomputed from the document, which is the
//! slowest path and the one that defines correctness. [`rebuild_cached`] may
//! restore a feature's geometry and names from a sidecar instead of computing
//! them, and is only allowed to agree with the cold path faster.
//!
//! The cold path's guarantee is structural rather than a promise: it has no
//! parameter through which a cache could reach it.
//!
//! # Both paths produce the same thing
//!
//! [`RebuildResult`] is the single return type, and it reports nothing that
//! only a fresh computation could supply. An archive carries geometry and
//! names, not the history of the operation that made them, so a result that
//! exposed raw history would have to answer differently after a hit — and a
//! caller able to tell a warm rebuild from a cold one is a caller whose
//! correctness depends on which one it got.
//!
//! Sequential on purpose. [`RebuildPlan`](crate::RebuildPlan) already reports
//! levels that could run concurrently, but a kernel session is not shareable
//! and there is no scheduler yet; running the plan in order is the honest
//! implementation of what exists today.

use std::collections::BTreeMap;

use ferritecad_document::TopologyRef;
use ferritecad_document::{CacheStore, Document, ObjectPayload, ObjectRecord};
use ferritecad_kernel::{
    ExtrudeRequest, GeometryKernel, OperationContext, Profile, ShapeHandle, SketchPlane,
    SubShapeHandle,
};
use ferritecad_topology::{TopologyMap, archive_feature, restore_feature};
use ferritecad_types::{CadError, ObjectId, Result};

use crate::cache::{load_extrude_archive, store_extrude_archive};
use crate::convert::{extrude_request, plane_from_datum, profile_from_sketch};
use crate::document_graph::DocumentGraph;

/// What a rebuild produced.
///
/// # Handles are session-owned
///
/// The shapes named here live in the kernel session that built them. Call
/// [`RebuildResult::release_all`] when finished; dropping this value instead
/// leaves the kernel holding the geometry until the session ends. A failed
/// rebuild needs no such care — it releases everything it made before
/// returning the error.
///
/// This value is deliberately not cloneable. A copy could release the shapes
/// while the original continued handing out handles that were no longer live.
///
/// # What a face is called lives in one place
///
/// [`topology`][Self::topology] is the only account of what this rebuild
/// produced. Raw [`History`][ferritecad_kernel::History] and cap handles are
/// used while building that account and then dropped: they describe an
/// operation, and a feature restored from a cache had no operation. Anything a
/// caller needs about a face it must ask for by name.
#[derive(Debug, Default)]
pub struct RebuildResult {
    shapes: BTreeMap<ObjectId, ShapeHandle>,
    profiles: BTreeMap<ObjectId, Profile>,
    topology: TopologyMap,
    order: Vec<ObjectId>,
    owned: Vec<ShapeHandle>,
    imports: Vec<ObjectId>,
}

impl RebuildResult {
    /// The shape a feature or body resolved to.
    pub fn shape(&self, object: ObjectId) -> Option<ShapeHandle> {
        self.shapes.get(&object).copied()
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

    /// The imported objects this rebuild found and did not build.
    ///
    /// Their geometry is stored rather than derived, so there is nothing here
    /// to recompute. Reported so that a caller which needs the whole document
    /// – an export, a viewport – can tell that this result is not all of it,
    /// instead of inferring completeness from the absence of an error.
    pub fn imports(&self) -> &[ObjectId] {
        &self.imports
    }

    /// How many shapes this rebuild created and still holds.
    pub fn shape_count(&self) -> usize {
        self.owned.len()
    }

    /// Hands every shape back to the kernel.
    pub fn release_all(self, kernel: &mut (impl GeometryKernel + ?Sized)) {
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
///
/// Reads and writes no cache, and cannot be made to: there is no parameter
/// through which one could be supplied.
pub fn rebuild_cold<K: GeometryKernel + ?Sized>(
    document: &Document,
    kernel: &mut K,
    context: &OperationContext,
) -> Result<RebuildResult> {
    rebuild(document, kernel, None, context).map(|(result, _)| result)
}

/// Rebuilds `document`, restoring from `cache` whatever it usefully holds.
///
/// Produces exactly what [`rebuild_cold`] would, alongside a per-feature
/// account of what the cache did. A cache that is empty, stale or damaged
/// costs time and nothing else: every failure to use an entry falls back to
/// computing the feature, and a failure to store one leaves a rebuild that
/// already succeeded successful.
pub fn rebuild_cached<K: GeometryKernel + ?Sized>(
    document: &Document,
    kernel: &mut K,
    cache: &mut CacheStore,
    context: &OperationContext,
) -> Result<(RebuildResult, Vec<CacheEvent>)> {
    rebuild(document, kernel, Some(cache), context)
}

fn rebuild<K: GeometryKernel + ?Sized>(
    document: &Document,
    kernel: &mut K,
    cache: Option<&mut CacheStore>,
    context: &OperationContext,
) -> Result<(RebuildResult, Vec<CacheEvent>)> {
    // Do this before reading or validating the document. Besides avoiding
    // needless work, it makes cancellation observable for an empty document,
    // whose evaluation loop has no feature boundary at which to check it.
    context.check_cancelled()?;

    let mut state = RebuildResult::default();
    let mut events = Vec::new();

    match run(document, kernel, cache, context, &mut state, &mut events) {
        Ok(()) => Ok((state, events)),
        Err(error) => {
            state.release_all(kernel);
            Err(error)
        }
    }
}

fn run<K: GeometryKernel + ?Sized>(
    document: &Document,
    kernel: &mut K,
    mut cache: Option<&mut CacheStore>,
    context: &OperationContext,
    state: &mut RebuildResult,
    events: &mut Vec<CacheEvent>,
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

                let restored = match cache.as_deref_mut() {
                    Some(cache) => restore(kernel, cache, context, *id, &request, state, events)?,
                    None => false,
                };

                if !restored {
                    let result = kernel.extrude(&request, context)?;

                    // Register ownership before checking again: a kernel can
                    // finish the operation and then invoke a progress callback
                    // that cancels the rebuild. The resulting shape still
                    // belongs to us and must participate in error cleanup.
                    state.owned.push(result.shape);
                    context.check_cancelled()?;

                    // Named while the result is still whole. The correspondence
                    // between a segment and the face it raised lives across the
                    // history and the caps, and reassembling it from those
                    // parts afterwards would be inventing it rather than
                    // recording it.
                    state
                        .topology
                        .record_extrude(*id, request.profile(), &result)?;
                    state.shapes.insert(*id, result.shape);

                    if let Some(cache) = cache.as_deref_mut() {
                        store(kernel, cache, context, *id, &request, state, events);
                    }
                }
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

            // An imported STEP object holds geometry that was never built from
            // features. It comes from bytes the document stores, and reading
            // them again needs an importer rather than a geometry kernel –
            // which is why this stops here rather than being wired through:
            // nothing in the shipped graph points from a kernel adapter back
            // at the document, and rebuilding through this crate would.
            //
            // Named rather than skipped. A document that holds imported
            // geometry and rebuilds "completely" would be telling a caller
            // that this result covers everything in it, and it does not.
            ObjectPayload::ImportedStep(_) => {
                state.imports.push(*id);
                continue;
            }

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

/// What the cache did about one feature.
///
/// A feature can produce more than one of these: a miss is followed by a
/// failed write when the rebuilt result could not be stored.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CacheEvent {
    pub feature: ObjectId,
    pub outcome: CacheOutcome,
    /// Why, when the outcome is a refusal or a failure.
    pub detail: Option<String>,
}

/// The four things that can happen to one feature's cache entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum CacheOutcome {
    /// The stored archive was used, and the kernel did not build this feature.
    Hit,
    /// Nothing usable was stored.
    ///
    /// Includes damage the sidecar detected itself: [`CacheStore`] verifies
    /// its own content hashes and reports a corrupt payload as absent, which
    /// is indistinguishable from never having been written and needs the same
    /// response.
    Miss,
    /// Intact bytes that are not a usable archive.
    ///
    /// Narrower than a miss on purpose: something was stored, it survived
    /// storage, and it was still refused — by the codec, or by a kernel that
    /// could not decode the B-Rep inside it. That is a defect worth surfacing,
    /// where a miss is ordinary.
    Rejected,
    /// The feature was rebuilt and its archive could not be kept.
    ///
    /// The rebuild is unaffected; the next run will simply be cold again.
    WriteFailed,
}

impl CacheEvent {
    fn new(feature: ObjectId, outcome: CacheOutcome, detail: Option<String>) -> Self {
        Self {
            feature,
            outcome,
            detail,
        }
    }
}

/// Tries to restore one feature instead of computing it.
///
/// `Ok(true)` means the shape is built, named and owned by `state`. `Ok(false)`
/// means the caller must extrude, and says why in `events`. An error is a
/// genuine failure of the rebuild — cancellation, or a kernel that restored a
/// shape it will not then describe — never a disappointing cache.
fn restore<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    cache: &CacheStore,
    context: &OperationContext,
    id: ObjectId,
    request: &ExtrudeRequest,
    state: &mut RebuildResult,
    events: &mut Vec<CacheEvent>,
) -> Result<bool> {
    let archived = match load_extrude_archive(cache, kernel.identity(), request, context, id) {
        Ok(Some(archived)) => archived,
        Ok(None) => {
            events.push(CacheEvent::new(id, CacheOutcome::Miss, None));
            return Ok(false);
        }
        Err(error) => {
            events.push(CacheEvent::new(
                id,
                CacheOutcome::Rejected,
                Some(error.to_string()),
            ));
            return Ok(false);
        }
    };

    // A blob this kernel will not decode is still only a cache problem. The
    // map is left untouched by a failure here, so the fresh build below
    // records this feature over nothing rather than over a half-restore.
    if let Err(error) = restore_feature(kernel, &archived, &mut state.topology) {
        events.push(CacheEvent::new(
            id,
            CacheOutcome::Rejected,
            Some(error.to_string()),
        ));
        return Ok(false);
    }

    let shape = state
        .topology
        .feature(id)
        .and_then(|names| names.shape())
        .ok_or_else(|| {
            CadError::kernel(format!(
                "restoring feature {id} produced names without a shape to hang them on"
            ))
        })?;

    // Ownership first, and before the cancellation check: the shape exists in
    // the session from the moment it was decoded, and anything that unwinds
    // from here must be able to hand it back.
    state.owned.push(shape);
    state.shapes.insert(id, shape);
    events.push(CacheEvent::new(id, CacheOutcome::Hit, None));

    context.check_cancelled()?;
    Ok(true)
}

/// Keeps a freshly built feature for next time, or reports why it could not.
///
/// Deliberately infallible. The geometry is already correct and already in
/// hand; turning a storage problem into a failed rebuild would trade a slow
/// next run for no result at all.
fn store<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    cache: &mut CacheStore,
    context: &OperationContext,
    id: ObjectId,
    request: &ExtrudeRequest,
    state: &RebuildResult,
    events: &mut Vec<CacheEvent>,
) {
    let identity = kernel.identity().clone();
    let stored = archive_feature(kernel, &state.topology, id)
        .and_then(|archived| store_extrude_archive(cache, &identity, request, context, &archived));

    if let Err(error) = stored {
        events.push(CacheEvent::new(
            id,
            CacheOutcome::WriteFailed,
            Some(error.to_string()),
        ));
    }
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
