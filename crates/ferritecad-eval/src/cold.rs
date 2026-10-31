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

use ferritecad_document::{CacheStore, Document, EndCondition, ObjectPayload, ObjectRecord};
use ferritecad_kernel::{
    BooleanRequest, ChamferRequest, FilletRequest, GeometryKernel, OperationContext, Profile,
    ProgressSink, ShapeHandle, SketchPlane,
};
use ferritecad_topology::{ModelNaming, archive_feature, restore_feature};
use ferritecad_types::{CadError, ContentHash, ObjectId, Result};

use crate::{
    DegradedBody, FailedFeature, RebuildRefusal, RebuildReport, RebuildStatus, SkippedFeature,
};

use crate::cache::{
    boolean_archive_key, edge_archive_key, extrude_archive_key, load_feature_archive,
    revolve_archive_key, store_feature_archive, through_all_profile_key,
};
use crate::convert::{extrude_request, profile_from_sketch, revolve_request, through_all_request};
use crate::document_graph::DocumentGraph;
use crate::presentation::SketchPresentation;
use crate::solve::SketchSolveReport;

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
/// produced. Complete operation provenance is used while building that
/// account and then dropped: it describes an operation, and a feature restored
/// from a cache had no operation. Anything a
/// caller needs about a face it must ask for by name.
///
/// # What a solve found out is kept, and only for the sketch that was solved
///
/// A sketch that carries constraints is solved once, wherever this rebuild
/// reaches it, and [`SketchSolveReport`] is the other half of that one answer.
/// It is filed under the sketch's own [`ObjectId`], so a sketch two features
/// read is still one sketch with one report, and nothing that was not solved —
/// an unconstrained sketch, an extrude, a body, a datum, an import — has one
/// at all.
///
/// # What each sketch looked like is kept for every sketch
///
/// [`SketchPresentation`] is the third side of the same one evaluation, and
/// unlike a report every sketch this rebuild reached has one: an unconstrained
/// drawing is still a drawing, and asking no solver about it is not the same
/// as having nothing to show. Filed under the sketch's own [`ObjectId`] for
/// the same reason a report is, so two extrudes of one sketch are two
/// extrusions of one drawing.
#[derive(Debug, Default)]
pub struct RebuildResult {
    statuses: BTreeMap<ObjectId, RebuildStatus>,
    degraded_bodies: Vec<DegradedBody>,
    shapes: BTreeMap<ObjectId, ShapeHandle>,
    profiles: BTreeMap<ObjectId, Profile>,
    planes: BTreeMap<ObjectId, SketchPlane>,
    solve_reports: BTreeMap<ObjectId, SketchSolveReport>,
    presentations: BTreeMap<ObjectId, SketchPresentation>,
    topology: ModelNaming,
    /// What each feature's result is addressed by in a cache.
    ///
    /// Kept because a boolean is keyed by its inputs' keys rather than by a
    /// request of its own: a cut's entry has to move when the solid it cuts
    /// moves, and the only honest name for "that solid" is the key the feature
    /// that built it was stored under.
    keys: BTreeMap<ObjectId, ferritecad_types::ContentHash>,
    order: Vec<ObjectId>,
    owned: Vec<ShapeHandle>,
    imports: Vec<ObjectId>,
}

impl RebuildResult {
    pub fn status(&self, id: ObjectId) -> Option<&RebuildStatus> {
        self.statuses.get(&id)
    }
    pub fn all_built(&self) -> bool {
        self.statuses
            .values()
            .all(|s| matches!(s, RebuildStatus::Built))
    }
    pub fn report(&self) -> RebuildReport {
        let mut report = RebuildReport {
            degraded_bodies: self.degraded_bodies.clone(),
            ..RebuildReport::default()
        };
        for id in &self.order {
            match self.statuses.get(id) {
                Some(RebuildStatus::Failed { error }) => report.failed.push(FailedFeature {
                    id: *id,
                    kind: error.kind().as_str().into(),
                    message: error.to_string(),
                }),
                Some(RebuildStatus::Skipped { cause }) => report.skipped.push(SkippedFeature {
                    id: *id,
                    cause: *cause,
                }),
                _ => {}
            }
        }
        report
    }
    /// Reject a candidate only when it introduces an unbuilt object.
    pub fn require_no_new_failures(
        mut self,
        baseline: &std::collections::BTreeSet<ObjectId>,
        kernel: &mut (impl GeometryKernel + ?Sized),
    ) -> Result<Self> {
        let mut report = self.report();
        report
            .failed
            .retain(|failure| !baseline.contains(&failure.id));
        report
            .skipped
            .retain(|skipped| !baseline.contains(&skipped.id));
        if report.all_built() {
            return Ok(self);
        }
        let errors = std::mem::take(&mut self.statuses)
            .into_iter()
            .filter_map(|(id, status)| match status {
                RebuildStatus::Failed { error } if !baseline.contains(&id) => Some((id, error)),
                _ => None,
            })
            .collect();
        self.release_all(kernel);
        let refusal = RebuildRefusal { report, errors };
        let message = format!("edit would leave features unbuilt:\n{refusal}");
        let kind = refusal
            .errors
            .values()
            .next()
            .map(CadError::kind)
            .unwrap_or(ferritecad_types::ErrorKind::Topology);
        Err(CadError::Context {
            kind,
            message,
            source: Box::new(refusal),
        })
    }

    /// The shape a feature or body resolved to.
    pub fn shape(&self, object: ObjectId) -> Option<ShapeHandle> {
        self.shapes.get(&object).copied()
    }

    /// The computed frame of a successfully evaluated datum.
    pub fn plane(&self, object: ObjectId) -> Option<SketchPlane> {
        self.planes.get(&object).copied()
    }

    /// The profile a sketch converted to.
    pub fn profile(&self, object: ObjectId) -> Option<&Profile> {
        self.profiles.get(&object)
    }

    /// What the solve of one sketch found out, if that sketch was solved.
    ///
    /// `None` for everything else, including a sketch that carries no
    /// constraints: nothing asked a solver about it, so there is nothing this
    /// could honestly say.
    pub fn solve_report(&self, sketch: ObjectId) -> Option<&SketchSolveReport> {
        self.solve_reports.get(&sketch)
    }

    /// What one sketch of this document looked like, at the coordinates this
    /// rebuild built its profile from.
    ///
    /// `None` for everything that is not a sketch. Present for every sketch,
    /// constrained or not: what a drawing looks like is not a thing only a
    /// solver could have said.
    pub fn sketch_presentation(&self, sketch: ObjectId) -> Option<&SketchPresentation> {
        self.presentations.get(&sketch)
    }

    /// Every sketch this rebuild solved, in the order it evaluated them.
    ///
    /// The order is [`order`][Self::order]'s, which is the plan's, which is a
    /// function of the document alone. Not the map's own iteration order:
    /// identifiers sort by when they were minted, which is close enough to
    /// document order to look right in a test and is not the same fact.
    pub fn solve_reports(&self) -> impl Iterator<Item = (ObjectId, &SketchSolveReport)> {
        self.order
            .iter()
            .filter_map(|id| Some((*id, self.solve_reports.get(id)?)))
    }

    /// What each feature's output is called, for this session only.
    ///
    /// Read-only: the mapping is built while the geometry is, and nothing
    /// outside may add to it. It holds kernel handles, which implement no
    /// serialisation, so it cannot reach a document even by accident.
    pub fn topology(&self) -> &ModelNaming {
        &self.topology
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

    /// Release geometry while retaining the original per-object errors.
    pub fn into_statuses(
        mut self,
        kernel: &mut (impl GeometryKernel + ?Sized),
    ) -> BTreeMap<ObjectId, RebuildStatus> {
        let statuses = std::mem::take(&mut self.statuses);
        self.release_all(kernel);
        statuses
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
/// Reports local failures and continues independent branches. Structural errors
/// and cancellation release all shapes and fail the operation.
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

    state.topology.prepare(&objects)?;
    let total = plan.order().len();
    for (position, id) in plan.order().iter().enumerate() {
        // Checked between features rather than only inside the kernel: a
        // document of many cheap features must still stop promptly.
        context.check_cancelled()?;

        // What the kernel says about this one feature, on the scale of the
        // whole document. See `within`.
        let scoped = within(context, position, total);

        let object = objects.get(id).ok_or_else(|| {
            CadError::input(format!(
                "the plan names {id}, which the document does not hold"
            ))
        })?;

        state.order.push(*id);
        let selection = match &object.payload {
            ObjectPayload::Extrude(e) => Some((e.profile, &e.regions)),
            ObjectPayload::Revolve(r) => Some((r.profile, &r.regions)),
            ObjectPayload::KeepSolids(_) => None,
            _ => None,
        };
        if let Some((sketch_id, selected)) = selection
            && let Some(ObjectPayload::Sketch(sketch)) = objects.get(&sketch_id).map(|o| &o.payload)
            && let Some(curve) = selected.iter().find(|id| {
                !sketch
                    .curves
                    .iter()
                    .any(|c| c.id == **id && !c.construction)
            })
        {
            state.statuses.insert(
                *id,
                RebuildStatus::Failed {
                    error: CadError::input(format!(
                        "region curve {curve} is missing or is construction geometry"
                    )),
                },
            );
            continue;
        }
        let cause = graph
            .dependencies()
            .iter()
            .filter(|d| d.dependent == *id)
            .find_map(|d| {
                state
                    .status(d.dependency)
                    .and_then(|s| s.root_failure(d.dependency))
            });
        if let Some(cause) = cause {
            state.statuses.insert(*id, RebuildStatus::Skipped { cause });
            if let ObjectPayload::Body(body) = &object.payload {
                let mut current = body.tip_feature;
                let mut fallback = None;
                while let Some(feature) = current {
                    if matches!(state.status(feature), Some(RebuildStatus::Built))
                        && state.shape(feature).is_some()
                    {
                        fallback = Some(feature);
                        break;
                    }
                    current = objects
                        .get(&feature)
                        .and_then(|o| o.payload.previous_feature());
                }
                if let Some(shape) = fallback.and_then(|f| state.shape(f)) {
                    state.shapes.insert(*id, shape);
                }
                state.degraded_bodies.push(DegradedBody {
                    id: *id,
                    rolled_back_to: fallback,
                    cause,
                });
            }
            continue;
        }
        let owned_before = state.owned.len();
        let outcome = (|| -> Result<()> {
            match &object.payload {
                ObjectPayload::DatumPlane(datum) => {
                    let plane = crate::datum::evaluate(
                        *id,
                        datum,
                        &state.planes,
                        &state.topology,
                        kernel,
                        &scoped,
                    )?;
                    state.planes.insert(*id, plane);
                }

                ObjectPayload::Sketch(sketch) => {
                    let plane = state.planes.get(&sketch.plane).copied().ok_or_else(|| {
                        CadError::input(format!(
                            "sketch {id} is placed on {}, which is not a datum plane",
                            sketch.plane
                        ))
                    })?;
                    // Converted here rather than when the extrude asks for it, so a
                    // malformed sketch is reported against the sketch.
                    let evaluated = profile_from_sketch(sketch, *id, plane)?;
                    state.profiles.insert(*id, evaluated.profile);
                    // Filed for every sketch, however it was arrived at. The
                    // profile above and this were built from one set of
                    // coordinates, so nothing downstream has to choose between
                    // them or ask for them again.
                    state.presentations.insert(*id, evaluated.presentation);
                    // Filed only when there was a solve to report. Absent is the
                    // honest answer for a sketch nobody constrained, and an empty
                    // report would read as one that was solved and found rigid.
                    if let Some(report) = evaluated.report {
                        state.solve_reports.insert(*id, report);
                    }
                }

                ObjectPayload::Extrude(_) | ObjectPayload::Revolve(_) => {
                    let (profile_id, operation, through_all) = match &object.payload {
                        ObjectPayload::Extrude(e) => (
                            e.profile,
                            e.operation,
                            matches!(e.end_condition, EndCondition::ThroughAll),
                        ),
                        ObjectPayload::Revolve(r) => (r.profile, r.operation, false),
                        _ => unreachable!("sweep branch"),
                    };
                    let previous = object.payload.previous_feature();
                    let profile = state.profiles.get(&profile_id).ok_or_else(|| {
                        CadError::input(format!(
                            "feature {id} reads {profile_id}, which produced no profile"
                        ))
                    })?;
                    let selected = match &object.payload {
                        ObjectPayload::Extrude(e) => &e.regions,
                        ObjectPayload::Revolve(r) => &r.regions,
                        _ => unreachable!("sweep branch"),
                    };
                    let profile = profile.select(selected)?;
                    state.topology.select_profile(*id, &profile)?;
                    let target = previous
                        .map(|p| {
                            state.shapes.get(&p).copied().ok_or_else(|| {
                                CadError::input(format!(
                                    "feature {id} modifies {p}, which produced no result"
                                ))
                            })
                        })
                        .transpose()?;
                    let request = match &object.payload {
                        ObjectPayload::Extrude(e) if through_all => SweepRequest::ThroughAll {
                            profile,
                            reversed: e.reversed,
                            target: target.ok_or_else(|| {
                                CadError::input("ThroughAll has no target to go through")
                            })?,
                        },
                        ObjectPayload::Extrude(e) if e.end_condition.support().is_some() => {
                            let (result, reference) = match &e.end_condition {
                                EndCondition::UpToFace { result, face, .. } => (*result, face),
                                EndCondition::UpToVertex { result, vertex } => (*result, vertex),
                                _ => unreachable!("supported end condition"),
                            };
                            let kind = match &e.end_condition {
                                EndCondition::UpToFace { .. } => {
                                    ferritecad_document::EntityKind::Face
                                }
                                _ => ferritecad_document::EntityKind::Vertex,
                            };
                            let entities =
                                state.topology.resolve(kind, *id, result, &[*reference])?;
                            let end = match &e.end_condition {
                                EndCondition::UpToFace { offset_mm, .. } => SurfaceEnd::Face {
                                    faces: entities,
                                    offset: offset_mm.value(),
                                },
                                _ => SurfaceEnd::Vertex(entities[0]),
                            };
                            SweepRequest::UpTo {
                                profile,
                                reversed: e.reversed,
                                end,
                                support_key: state.keys[&result],
                                payload_key: e.cache_key(scoped.tolerance()),
                            }
                        }
                        ObjectPayload::Extrude(e) => {
                            SweepRequest::Extrude(extrude_request(e, profile, None)?)
                        }
                        ObjectPayload::Revolve(r) => SweepRequest::Revolve(revolve_request(
                            r,
                            profile,
                            &state.presentations[&profile_id],
                        )?),
                        _ => unreachable!("sweep branch"),
                    };
                    let tool_key = request.key(kernel.identity(), &scoped);
                    let key = if let Some(previous) = previous {
                        boolean_archive_key(
                            kernel.identity(),
                            boolean_kind(operation)?,
                            previous,
                            &state.keys[&previous],
                            &tool_key,
                            through_all,
                            &scoped,
                        )
                    } else {
                        tool_key
                    };
                    let key = state.topology.cache_key(*id, key);
                    let restored = match cache.as_deref_mut() {
                        Some(cache) => restore(kernel, cache, &scoped, key, *id, state, events)?,
                        None => false,
                    };
                    if !restored {
                        let tool = request.build(kernel, &scoped)?;
                        state.owned.push(tool.shape);
                        context.check_cancelled()?;
                        let tool_names =
                            state
                                .topology
                                .name_operation(*id, tool.shape, &[], kernel)?;
                        let (shape, names) = match (previous, target) {
                            (Some(previous), Some(target)) => {
                                let request = BooleanRequest::new(
                                    boolean_kind(operation)?,
                                    target,
                                    tool.shape,
                                )?;
                                let result = kernel.boolean(&request, &scoped)?;
                                state.owned.push(result.shape);
                                context.check_cancelled()?;
                                let previous_names =
                                    state.topology.feature(previous).ok_or_else(|| {
                                        CadError::topology(format!(
                                            "feature {id} modifies {previous}, which named nothing"
                                        ))
                                    })?;
                                let names = state.topology.name_operation(
                                    *id,
                                    result.shape,
                                    &[previous_names, &tool_names],
                                    kernel,
                                )?;
                                (result.shape, names)
                            }
                            _ => (tool.shape, tool_names),
                        };
                        state.topology.insert_feature(*id, names)?;
                        state.shapes.insert(*id, shape);
                        if let Some(cache) = cache.as_deref_mut() {
                            store(kernel, cache, key, *id, state, events);
                        }
                    }
                    state.keys.insert(*id, key);
                }

                ObjectPayload::KeepSolids(keep) => {
                    let witnesses = match &keep.selection {
                        ferritecad_document::KeepSelection::All => Vec::new(),
                        ferritecad_document::KeepSelection::ContainingFaces(faces) => {
                            // Report every unresolved reference before testing native membership.
                            state.topology.resolve(
                                ferritecad_document::EntityKind::Face,
                                *id,
                                keep.previous,
                                faces,
                            )?;
                            faces
                                .iter()
                                .map(|face| {
                                    Ok(ferritecad_kernel::SolidWitness {
                                        reference: face.id,
                                        faces: state.topology.resolve(
                                            ferritecad_document::EntityKind::Face,
                                            *id,
                                            keep.previous,
                                            &[*face],
                                        )?,
                                    })
                                })
                                .collect::<Result<Vec<_>>>()?
                        }
                    };
                    let key = crate::cache::keep_archive_key(
                        kernel.identity(),
                        &state.keys[&keep.previous],
                        keep.cache_key(),
                    );
                    let key = state.topology.cache_key(*id, key);
                    let restored = match cache.as_deref_mut() {
                        Some(cache) => restore(kernel, cache, &scoped, key, *id, state, events)?,
                        None => false,
                    };
                    if !restored {
                        let previous = &state.topology.features[&keep.previous];
                        let target = state.shapes[&keep.previous];
                        let result =
                            kernel
                                .keep_solids(target, &witnesses, &scoped)
                                .map_err(|error| CadError::Context {
                                    kind: error.kind(),
                                    message: format!("KeepSolids {id}: {error}"),
                                    source: Box::new(error),
                                })?;
                        state.owned.push(result.shape);
                        let names = state.topology.name_operation(
                            *id,
                            result.shape,
                            &[previous],
                            kernel,
                        )?;
                        state.topology.insert_feature(*id, names)?;
                        state.shapes.insert(*id, result.shape);
                        if let Some(cache) = cache.as_deref_mut() {
                            store(kernel, cache, key, *id, state, events);
                        }
                    }
                    state.keys.insert(*id, key);
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
                }

                ObjectPayload::Fillet(_) | ObjectPayload::Chamfer(_) => {
                    let (previous, references, amount, chamfer, payload_key) = match &object.payload
                    {
                        ObjectPayload::Fillet(f) => (
                            f.previous,
                            &f.edges,
                            f.radius_mm.value(),
                            false,
                            f.cache_key(scoped.tolerance()),
                        ),
                        ObjectPayload::Chamfer(c) => (
                            c.previous,
                            &c.edges,
                            c.distance_mm.value(),
                            true,
                            c.cache_key(scoped.tolerance()),
                        ),
                        _ => unreachable!("edge operation arm"),
                    };
                    let target_key = state.keys.get(&previous).ok_or_else(|| {
                        CadError::topology(format!(
                            "feature {id} consumes an unavailable result {previous}"
                        ))
                    })?;
                    let edges = state.topology.resolve(
                        ferritecad_document::EntityKind::Edge,
                        *id,
                        previous,
                        references,
                    )?;
                    let key = edge_archive_key(kernel.identity(), target_key, payload_key);
                    let key = state.topology.cache_key(*id, key);
                    let restored = match cache.as_deref_mut() {
                        Some(cache) => restore(kernel, cache, &scoped, key, *id, state, events)?,
                        None => false,
                    };
                    if !restored {
                        let previous_names = &state.topology.features[&previous];
                        let target = previous_names.shape().ok_or_else(|| {
                            CadError::topology(format!("feature {previous} has no shape"))
                        })?;
                        let result = if chamfer {
                            kernel.chamfer_edges(
                                &ChamferRequest::new(target, &edges, amount)?,
                                &scoped,
                            )?
                        } else {
                            kernel.fillet_edges(
                                &FilletRequest::new(target, &edges, amount)?,
                                &scoped,
                            )?
                        };
                        state.owned.push(result.shape);
                        context.check_cancelled()?;
                        let names = state.topology.name_operation(
                            *id,
                            result.shape,
                            &[previous_names],
                            kernel,
                        )?;
                        state.topology.insert_feature(*id, names)?;
                        state.shapes.insert(*id, result.shape);
                        if let Some(cache) = cache.as_deref_mut() {
                            store(kernel, cache, key, *id, state, events);
                        }
                    }
                    state.keys.insert(*id, key);
                }

                other => {
                    return Err(CadError::unsupported(format!(
                        "object {id} has payload {}, which the cold evaluator does not implement",
                        other.type_name()
                    )));
                }
            }

            Ok(())
        })();
        match outcome {
            Ok(()) => {
                state.statuses.insert(*id, RebuildStatus::Built);
            }
            Err(error) if error.kind() == ferritecad_types::ErrorKind::Cancellation => {
                return Err(error);
            }
            Err(error) => {
                for shape in state.owned.drain(owned_before..).rev() {
                    kernel.release(shape);
                }
                state.shapes.remove(id);
                state.profiles.remove(id);
                state.keys.remove(id);
                state.topology.remove_feature(*id);
                state.statuses.insert(*id, RebuildStatus::Failed { error });
            }
        }
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
    key: ferritecad_types::ContentHash,
    id: ObjectId,
    state: &mut RebuildResult,
    events: &mut Vec<CacheEvent>,
) -> Result<bool> {
    let archived = match load_feature_archive(cache, kernel.identity(), key, id) {
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
        if error.kind() == ferritecad_types::ErrorKind::Cancellation {
            return Err(error);
        }
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
    key: ferritecad_types::ContentHash,
    id: ObjectId,
    state: &RebuildResult,
    events: &mut Vec<CacheEvent>,
) {
    let identity = kernel.identity().clone();
    let stored = archive_feature(kernel, &state.topology, id)
        .and_then(|archived| store_feature_archive(cache, &identity, key, &archived));

    if let Err(error) = stored {
        events.push(CacheEvent::new(
            id,
            CacheOutcome::WriteFailed,
            Some(error.to_string()),
        ));
    }
}

/// One feature's slice of the whole rebuild's progress.
///
/// A kernel reports how far it is through the operation it was given; someone
/// watching a rebuild wants to know how far it is through the document. This
/// wraps the caller's sink so a kernel's `0.0` and `1.0` for feature `done` of
/// `total` arrive as the fractions of the document they actually are.
///
/// Wrapping rather than forwarding is what keeps one meaning on one channel: a
/// sink that saw the kernel's numbers unchanged would watch the count sweep to
/// completion once per feature, with no way to tell that from being finished.
///
/// Only what a kernel reports moves the count. A plan item that asks no
/// kernel anything – a datum, a sketch, a stored import – passes in silence,
/// because a progress report has always meant that geometry was worked on and
/// broadening it here would let a callback cancel a rebuild earlier than the
/// work it was watching.
///
/// Everything else about the context travels unchanged, the cancel token
/// included: it is the same token, so a progress callback can still stop the
/// rebuild it is watching.
fn within(context: &OperationContext, done: usize, total: usize) -> OperationContext {
    let outer = context.progress().clone();
    let (done, total) = (done as f64, total.max(1) as f64);

    OperationContext::new(context.tolerance())
        .with_cancel(context.cancel().clone())
        .with_progress(ProgressSink::new(move |fraction| {
            outer.report((done + fraction) / total);
        }))
}

fn ensure_rebuildable(document: &Document) -> Result<()> {
    let report = document.validate()?;
    let structural: Vec<_> = report
        .errors()
        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
        .collect();
    if !structural.is_empty() {
        return Err(CadError::input(format!(
            "document validation failed before cold rebuild: {}",
            structural.join("; ")
        )));
    }
    // A missing solver limits this build, as an unsupported capability does.
    // As a sketch's own failure it would be in every baseline, so an edit
    // nothing could check would pass as one that adds no failure.
    if let Err(unavailable) = ferritecad_sketch_solver::availability() {
        for object in document.objects()? {
            if let ObjectPayload::Sketch(sketch) = &object.payload
                && !sketch.constraints.is_empty()
            {
                return Err(CadError::unsupported(format!(
                    "sketch {} carries {} constraint(s) and {unavailable}",
                    object.id,
                    sketch.constraints.len()
                )));
            }
        }
    }
    Ok(())
}

fn boolean_kind(
    operation: ferritecad_document::SolidOperation,
) -> Result<ferritecad_kernel::BooleanKind> {
    use ferritecad_document::SolidOperation as S;
    use ferritecad_kernel::BooleanKind as B;
    match operation {
        S::Add => Ok(B::Fuse),
        S::Cut => Ok(B::Cut),
        S::Intersect => Ok(B::Common),
        _ => Err(CadError::input(
            "a boolean feature must specify Add, Cut or Intersect",
        )),
    }
}

enum SurfaceEnd {
    Face {
        faces: Vec<ferritecad_kernel::SubShapeHandle>,
        offset: f64,
    },
    Vertex(ferritecad_kernel::SubShapeHandle),
}

enum SweepRequest {
    UpTo {
        profile: Profile,
        reversed: bool,
        end: SurfaceEnd,
        support_key: ContentHash,
        payload_key: ContentHash,
    },
    ThroughAll {
        profile: Profile,
        reversed: bool,
        target: ShapeHandle,
    },
    Extrude(ferritecad_kernel::ExtrudeRequest),
    Revolve(ferritecad_kernel::RevolveRequest),
}
impl SweepRequest {
    fn key(
        &self,
        kernel: &ferritecad_kernel::KernelIdentity,
        context: &OperationContext,
    ) -> ContentHash {
        match self {
            Self::UpTo {
                profile,
                support_key,
                payload_key,
                ..
            } => {
                let mut h = ferritecad_types::CanonicalHasher::new("eval.extrude.up-to");
                h.algorithm_version(1);
                kernel.feed(&mut h);
                context.tolerance().feed(&mut h);
                profile.feed(&mut h);
                h.hash(support_key).hash(payload_key);
                h.finish()
            }
            Self::ThroughAll {
                profile, reversed, ..
            } => through_all_profile_key(profile, *reversed),
            Self::Extrude(r) => extrude_archive_key(kernel, r, context),
            Self::Revolve(r) => revolve_archive_key(kernel, r, context),
        }
    }
    fn build(
        &self,
        kernel: &mut (impl GeometryKernel + ?Sized),
        context: &OperationContext,
    ) -> Result<ferritecad_kernel::OperationResult> {
        match self {
            Self::UpTo {
                profile,
                reversed,
                end,
                ..
            } => match end {
                SurfaceEnd::Face { faces, offset } => {
                    kernel.extrude_to_face(profile, *reversed, faces, *offset, context)
                }
                SurfaceEnd::Vertex(vertex) => {
                    let extent = crate::convert::vertex_extent(
                        *profile.plane(),
                        *reversed,
                        kernel.vertex_point(*vertex)?,
                    )?;
                    kernel.extrude(
                        &ferritecad_kernel::ExtrudeRequest::new(profile.clone(), extent, *reversed),
                        context,
                    )
                }
            },
            Self::ThroughAll {
                profile,
                reversed,
                target,
            } => {
                let n = profile.plane().normal();
                let sign = if *reversed { -1. } else { 1. };
                let direction = ferritecad_types::Vec3::new(n.x * sign, n.y * sign, n.z * sign)?;
                let bounds = kernel.projected_bounds(*target, direction)?;
                let request = through_all_request(profile.clone(), *reversed, bounds)?;
                kernel.extrude(&request, context)
            }
            Self::Extrude(r) => kernel.extrude(r, context),
            Self::Revolve(r) => kernel.revolve(r, context),
        }
    }
}
