// SPDX-License-Identifier: MIT
//! Cutting one vertical edge of a saved rectangular plate away at equal
//! distances, in a new copy (§29A), and changing that distance in another.
//!
//! The same history model a Fillet uses (ADR 0004): the new Chamfer names the
//! feature whose result it cuts, the Body names its tip, and ownership is
//! derived. The class is the plate the Fillet reads through
//! [`crate::cut_edit::saved_plate_for_fillet`], with no Cut, no Fillet and no
//! other Chamfer, and a Sketch that carries no constraint or only the four
//! Coincident closure links — the Lines are then exactly the stored ones, so
//! there is no solver and no "starting guess".
//!
//! The edge is chosen by what it means — the base Extrude and the corner of its
//! profile, named by the unordered pair of the two Line UUIDs that meet there —
//! never by an index or a position. The distance is judged by one measured
//! policy ([`MIN_DISTANCE_MM`], [`MIN_FLAT_MM`], [`ChamferCorner::max_distance_mm`])
//! applied by discovery, preparation, the writer's re-derivation and the
//! evaluator alike. Nothing is clamped.
//!
//! The Chamfer's constants are its own. They were measured on Open CASCADE
//! 8.0.1 for a chamfer, and are not the Fillet's radius bounds: the operation
//! is analytic to 1.7e-16 relative at every distance it builds, so the lower
//! bound is about what a mesh and a person can still see, and the upper one is
//! about what is left of the two adjacent faces.
use ferritecad_types::{CadError, ContentHash, ObjectId, ProfileJoint, Result, StableEntityId};

use crate::cut_edit::NewObject;
use crate::fillet::{FilletCorner, corner_for, corners_of_lines};
use crate::{
    Body, Chamfer, Dependency, DependencyRole, Document, ObjectPayload, ObjectRecord, SemanticRole,
    Sketch, SketchConstraintRule, SweptEdge, TopologyRef,
};

fn unsupported(message: impl Into<String>) -> CadError {
    CadError::unsupported(message)
}

/// The smallest distance this build cuts an edge at, in millimetres.
///
/// Measured on Open CASCADE 8.0.1: a chamfer builds, valid and analytic to
/// 1.7e-16 relative, at every distance from 1e-7 mm up, and fails at 1e-9 mm.
/// 0.001 mm is 10⁴ times the kernel's linear tolerance, four orders above the
/// smallest distance the kernel builds, and leaves a flat 1.4 µm wide — hundreds
/// of ulps of an f32 STL coordinate at 30 mm, so the face is still a face in an
/// exported mesh. Chosen for that, never narrowed to make an example pass.
pub const MIN_DISTANCE_MM: f64 = 0.001;

/// How much of each adjacent face a chamfer must leave, in millimetres.
///
/// A chamfer of `d` leaves `len − d` of each of the two faces meeting at the
/// edge. Open CASCADE builds up to `d = len − 1e-3` and fails at `d = len`
/// (measured); a face of a few microns is a sliver nobody asked for. 0.01 mm
/// is the smallest feature this build keeps (the Fillet's flat is the same).
pub const MIN_FLAT_MM: f64 = 0.01;

/// The largest distance at a corner whose two adjacent Lines have these
/// lengths. The one expression discovery reports and every check applies, so
/// the largest distance offered is exactly one that is accepted, and the next
/// representable value above it is refused.
pub fn max_distance_of(adjacent_lengths_mm: [f64; 2]) -> f64 {
    adjacent_lengths_mm[0].min(adjacent_lengths_mm[1]) - MIN_FLAT_MM
}

/// The part of the policy that holds at every corner: a finite distance of at
/// least [`MIN_DISTANCE_MM`]. The bound that depends on the corner is
/// [`ChamferCorner::check_distance`].
pub fn check_distance_value(distance_mm: f64) -> Result<()> {
    if !distance_mm.is_finite() {
        return Err(CadError::input(format!(
            "a chamfer distance must be a finite number of millimetres, found {distance_mm}"
        )));
    }
    if distance_mm < MIN_DISTANCE_MM {
        return Err(CadError::input(format!(
            "a chamfer distance must be at least {MIN_DISTANCE_MM} mm, found {distance_mm} mm"
        )));
    }
    Ok(())
}

/// One vertical edge of the plate a Chamfer could cut.
///
/// `joint` is the identity. The rest are labels a person can read, recomputed
/// from the profile every time; none of them names the edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChamferCorner {
    /// The feature that swept the edge: the plate's base Extrude.
    pub feature: ObjectId,
    pub joint: ProfileJoint,
    /// The corner of the profile the edge was swept from, in sketch mm.
    pub corner_mm: [f64; 2],
    /// The lengths of the two Lines meeting there, in the joint's canonical
    /// order.
    pub adjacent_lengths_mm: [f64; 2],
    /// The largest distance this build accepts here; below
    /// [`MIN_DISTANCE_MM`] when the corner's sides leave no distance at all.
    pub max_distance_mm: f64,
}

impl ChamferCorner {
    fn of(corner: &FilletCorner) -> Self {
        Self {
            feature: corner.feature,
            joint: corner.joint,
            corner_mm: corner.corner_mm,
            adjacent_lengths_mm: corner.adjacent_lengths_mm,
            max_distance_mm: max_distance_of(corner.adjacent_lengths_mm),
        }
    }

    /// Whether any distance fits this corner.
    pub fn is_offerable(&self) -> bool {
        self.max_distance_mm >= MIN_DISTANCE_MM
    }

    /// The distance policy, with the numbers in the refusal.
    pub fn check_distance(&self, distance_mm: f64) -> Result<()> {
        check_distance_value(distance_mm)?;
        if distance_mm > self.max_distance_mm {
            return Err(CadError::input(format!(
                "a chamfer of {distance_mm} mm at corner {} is too large: this build chamfers up \
                 to the shorter adjacent side ({} mm) less the {MIN_FLAT_MM} mm it keeps of each \
                 face, which is {} mm; nothing is clamped",
                self.joint,
                self.adjacent_lengths_mm[0].min(self.adjacent_lengths_mm[1]),
                self.max_distance_mm
            )));
        }
        Ok(())
    }
}

/// §29A: a saved Chamfer, if the document holds one, and why every reader of a
/// plate then refuses: the one sentence each of them says.
pub(crate) fn refuse_chamfered(objects: &[ObjectRecord]) -> Result<()> {
    let chamfers: Vec<&ObjectRecord> = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Chamfer(_)))
        .collect();
    let Some(first) = chamfers.first() else {
        return Ok(());
    };
    if chamfers.len() > 1 {
        return Err(unsupported(format!(
            "this document holds {} Chamfers, beginning with {} (§29A); this build holds one \
             Chamfer on one plate, and only its distance (edit-chamfer-distance) can be edited",
            chamfers.len(),
            first.id
        )));
    }
    Err(unsupported(format!(
        "this Body ends in Chamfer {} (§29A); only its distance (edit-chamfer-distance), its plate's \
         height (edit-extrude, §29B), its base Sketch's coordinates (edit-sketch-copy, \
         §29C) and its base Sketch's Line constraints (edit-sketch-constraints-copy, §29D) \
         can be edited. A Fillet or a Cut after it, and a second Chamfer are not supported yet, and no editor changes a \
         chamfered plate without knowing its Chamfer",
        first.id
    )))
}

fn rule_name(rule: &SketchConstraintRule) -> &'static str {
    match rule {
        SketchConstraintRule::Coincident { .. } => "Coincident",
        SketchConstraintRule::Fixed { .. } => "Fixed",
        SketchConstraintRule::Distance { .. } => "Distance",
        SketchConstraintRule::Horizontal { .. } => "Horizontal",
        SketchConstraintRule::Vertical { .. } => "Vertical",
        SketchConstraintRule::EqualLength { .. } => "EqualLength",
        SketchConstraintRule::Perpendicular { .. } => "Perpendicular",
        SketchConstraintRule::Parallel { .. } => "Parallel",
        _ => "other",
    }
}

/// The plate's Sketch must be free or closure-only. Names the first
/// constraint that is neither.
fn require_free_profile(sketch: &Sketch) -> Result<()> {
    if sketch.constraints.is_empty() || crate::sketch_constraints::closure_links_only(sketch) {
        return Ok(());
    }
    let offending = sketch
        .constraints
        .iter()
        .find(|c| !matches!(c.rule, SketchConstraintRule::Coincident { .. }))
        .or_else(|| sketch.constraints.first());
    Err(unsupported(match offending {
        Some(c) => format!(
            "this slice chamfers a free or closure-only plate; its profile carries the {} \
             constraint {}, and a dimensioned plate is not chamfered yet",
            rule_name(&c.rule),
            c.id
        ),
        None => "this slice chamfers a free or closure-only plate".to_owned(),
    }))
}

/// §29D: the plate's Sketch under a *saved* Chamfer may carry the constraint
/// editor's managed Line family (free, closure-only, or the family itself);
/// anything else is refused naming the Chamfer and the constraint editor's reason.
fn require_managed_profile(sketch: &Sketch, chamfer: ObjectId) -> Result<()> {
    if sketch.constraints.is_empty() || crate::sketch_constraints::closure_links_only(sketch) {
        return Ok(());
    }
    crate::sketch_constraints::managed_lines(sketch).map_err(|e| {
        let guilty = crate::sketch_constraints::first_outside_managed_lines(sketch)
            .or_else(|| sketch.constraints.first())
            .map_or_else(String::new, |c| {
                format!(" (the {} constraint {})", rule_name(&c.rule), c.id)
            });
        unsupported(format!(
            "Chamfer {chamfer} sits on a plate whose Sketch carries constraints outside the \
             constraint editor's Line family{guilty}: {e}"
        ))
    })
}

/// A saved Body a Chamfer can be added to, exactly as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedChamferTarget {
    pub body: ObjectId,
    /// The base Extrude: both the feature consumed and the one whose edge is
    /// cut.
    pub base_feature: ObjectId,
    pub profile: ObjectId,
    pub profile_segments: Vec<StableEntityId>,
    pub height_mm: f64,
    /// The four candidates, in stored segment order.
    pub corners: Vec<ChamferCorner>,
}

impl SavedChamferTarget {
    pub fn corner_for(&self, edge: SweptEdge) -> Result<ChamferCorner> {
        let stored: Vec<FilletCorner> = self
            .corners
            .iter()
            .map(|c| FilletCorner {
                feature: c.feature,
                joint: c.joint,
                corner_mm: c.corner_mm,
                adjacent_lengths_mm: c.adjacent_lengths_mm,
                max_radius_mm: 0.0,
            })
            .collect();
        let found = corner_for(&stored, edge)?;
        self.corners
            .iter()
            .find(|c| c.joint == found.joint)
            .copied()
            .ok_or_else(|| CadError::input("the corner disappeared from the target"))
    }
}

fn saved_target(
    document: &Document,
    objects: &[ObjectRecord],
    body: ObjectId,
) -> Result<SavedChamferTarget> {
    // Named first, so a Fillet or a Chamfer already on the plate is refused by
    // its own sentence rather than reported as a shape mismatch, and a plate
    // with a dimension by that dimension, whatever the constraint editor would
    // have made of it.
    for object in objects {
        if let ObjectPayload::Sketch(sketch) = &object.payload {
            require_free_profile(sketch)?;
        }
    }
    let history = crate::cut_edit::saved_plate_for_fillet(document, objects)?;
    let target = history.target_for_fillet(body)?;
    let profile = objects
        .iter()
        .find(|o| o.id == target.profile)
        .ok_or_else(|| unsupported("the part's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(unsupported("the part's profile is not a Sketch"));
    };
    require_free_profile(sketch)?;
    let corners = corners_of_lines(target.base_feature, &sketch.curves)?
        .iter()
        .map(ChamferCorner::of)
        .collect();
    Ok(SavedChamferTarget {
        body,
        base_feature: target.base_feature,
        profile: target.profile,
        profile_segments: target.profile_segments,
        height_mm: target.height_mm,
        corners,
    })
}

/// A Body row of discovery: a target, or the reason it is not one.
#[derive(Debug, Clone, PartialEq)]
pub struct ChamferChoice {
    pub body: ObjectId,
    pub name: Option<String>,
    pub target: Option<SavedChamferTarget>,
    pub refusal: Option<String>,
}

impl ChamferChoice {
    /// The same check preparation applies, with no SQLite or kernel work.
    pub fn validate(&self, chamfer: &EdgeChamfer) -> Result<ChamferCorner> {
        let target = self
            .target
            .as_ref()
            .ok_or_else(|| unsupported(self.refusal.clone().unwrap_or_default()))?;
        let corner = target.corner_for(chamfer.edge)?;
        corner.check_distance(chamfer.distance_mm)?;
        Ok(corner)
    }
}

/// What one chamfer asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeChamfer {
    pub edge: SweptEdge,
    pub distance_mm: f64,
}

/// One row per Body, each a target or its reason, from one snapshot.
pub fn chamfer_choices(document: &Document, objects: &[ObjectRecord]) -> Vec<ChamferChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .map(|o| match saved_target(document, objects, o.id) {
            Ok(target) => ChamferChoice {
                body: o.id,
                name: o.name.clone(),
                target: Some(target),
                refusal: None,
            },
            Err(e) => ChamferChoice {
                body: o.id,
                name: o.name.clone(),
                target: None,
                refusal: Some(e.to_string()),
            },
        })
        .collect()
}

/// A checked Chamfer creation: the new feature, the Body with its new tip, the
/// dependency changes and the names the finished part needs.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEdgeChamfer {
    pub(crate) body: ObjectRecord,
    pub(crate) feature: NewObject,
    pub(crate) added_dependencies: Vec<Dependency>,
    pub(crate) removed_dependencies: Vec<Dependency>,
    pub(crate) references: Vec<TopologyRef>,
    pub(crate) previous: ObjectId,
    pub(crate) corner: ChamferCorner,
}

impl PreparedEdgeChamfer {
    pub fn body(&self) -> &ObjectRecord {
        &self.body
    }
    pub fn feature(&self) -> &NewObject {
        &self.feature
    }
    pub fn previous(&self) -> ObjectId {
        self.previous
    }
    pub fn references(&self) -> &[TopologyRef] {
        &self.references
    }
    /// The corner being cut, with its labels, read from the stored Lines.
    pub fn corner(&self) -> ChamferCorner {
        self.corner
    }
    pub fn distance_mm(&self) -> f64 {
        match &self.feature.payload {
            ObjectPayload::Chamfer(c) => c.distance_mm,
            _ => f64::NAN,
        }
    }
}

/// What the finished part is called after one chamfer: the new planar face
/// under the edge it replaced (a role of its own), and every face the plate
/// had, as the chamfer leaves it, qualified by the plate's own Extrude.
pub(crate) fn chamfer_references(
    feature: ObjectId,
    base: ObjectId,
    joint: ProfileJoint,
    profile_segments: &[StableEntityId],
) -> Vec<TopologyRef> {
    crate::fillet::edge_operation_references(
        feature,
        base,
        profile_segments,
        SemanticRole::EdgeChamferFace {
            edge_feature: base,
            joint,
        },
    )
}

/// Prepares one chamfer against the saved document.
pub fn prepare_edge_chamfer(
    document: &Document,
    body: ObjectId,
    chamfer: &EdgeChamfer,
) -> Result<PreparedEdgeChamfer> {
    let objects = document.objects()?;
    let record = objects
        .iter()
        .find(|o| o.id == body)
        .cloned()
        .ok_or_else(|| CadError::input("selected Body UUID does not exist"))?;
    if !matches!(record.payload, ObjectPayload::Body(_)) {
        return Err(CadError::input("the selected object is not a Body"));
    }
    let target = saved_target(document, &objects, body)?;
    let corner = target.corner_for(chamfer.edge)?;
    corner.check_distance(chamfer.distance_mm)?;
    let previous = target.base_feature;

    let ordinal = objects
        .iter()
        .map(|o| o.ordinal)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| {
            CadError::input("a new object ordinal cannot fit after the saved objects")
        })?;
    let feature_id = ObjectId::new();
    let feature = NewObject {
        id: feature_id,
        ordinal,
        name: "Chamfer".to_owned(),
        payload: ObjectPayload::Chamfer(Chamfer {
            previous,
            edge: SweptEdge {
                feature: target.base_feature,
                joint: corner.joint,
            },
            distance_mm: chamfer.distance_mm,
        }),
    };
    let mut moved = record.clone();
    moved.payload = ObjectPayload::Body(Body {
        tip_feature: Some(feature_id),
    });
    let added_dependencies = vec![
        Dependency {
            dependent: feature_id,
            dependency: previous,
            role: DependencyRole::Predecessor,
        },
        Dependency {
            dependent: body,
            dependency: feature_id,
            role: DependencyRole::BodyTip,
        },
    ];
    let removed_dependencies = vec![Dependency {
        dependent: body,
        dependency: previous,
        role: DependencyRole::BodyTip,
    }];
    let references = chamfer_references(
        feature_id,
        target.base_feature,
        corner.joint,
        &target.profile_segments,
    );
    Ok(PreparedEdgeChamfer {
        body: moved,
        feature,
        added_dependencies,
        removed_dependencies,
        references,
        previous,
        corner,
    })
}

/// Re-derives a prepared chamfer from the document it claims to be against:
/// the writer's guard. Everything but the minted identities is derived again
/// from the numbers the prepared payload carries, and the whole is compared.
pub(crate) fn rederive(document: &Document, prepared: &PreparedEdgeChamfer) -> Result<()> {
    let ObjectPayload::Chamfer(chamfer) = &prepared.feature.payload else {
        return Err(CadError::input("a prepared chamfer carries a Chamfer"));
    };
    let stated = EdgeChamfer {
        edge: chamfer.edge,
        distance_mm: chamfer.distance_mm,
    };
    let mut checked = prepare_edge_chamfer(document, prepared.body.id, &stated)?;
    let fresh = checked.feature.id;
    let swap = |id: ObjectId| {
        if id == fresh { prepared.feature.id } else { id }
    };
    checked.feature.id = prepared.feature.id;
    if let ObjectPayload::Body(b) = &mut checked.body.payload {
        b.tip_feature = b.tip_feature.map(swap);
    }
    for dependency in checked
        .added_dependencies
        .iter_mut()
        .chain(&mut checked.removed_dependencies)
    {
        dependency.dependent = swap(dependency.dependent);
        dependency.dependency = swap(dependency.dependency);
    }
    if checked.references.len() != prepared.references.len() {
        return Err(CadError::input(
            "the prepared chamfer names a different number of faces than the document would",
        ));
    }
    for (mine, theirs) in checked.references.iter_mut().zip(&prepared.references) {
        mine.id = theirs.id;
        mine.owner = swap(mine.owner);
        mine.producer_feature = swap(mine.producer_feature);
    }
    if checked != *prepared {
        return Err(CadError::input(
            "the prepared chamfer does not describe the document it is being written to",
        ));
    }
    Ok(())
}

/// A Chamfer exactly as saved: the plate under it, its edge and its distance,
/// read through the one reader the discovery, the edit and the writer share.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedChamfer {
    pub feature: ObjectId,
    pub body: ObjectId,
    pub name: Option<String>,
    /// The base Extrude it consumes, and the one whose edge it cuts.
    pub base_feature: ObjectId,
    pub profile: ObjectId,
    pub height_mm: f64,
    pub edge: SweptEdge,
    pub distance_mm: f64,
    /// Its corner on the stored Lines, with the labels.
    pub corner: ChamferCorner,
    /// §29D: the base Sketch carries user constraints (not only the closure
    /// links). Its Lines are then the solver's starting approximation: `corner`
    /// is a stored fact, and the distance's upper bound is the rebuild's to judge
    /// on the solved Lines, never this reading's on the stored ones.
    pub constrained: bool,
}

impl SavedChamfer {
    /// §29C: this Chamfer on a candidate drawing of its plate: the rectangle
    /// read again by the creation reader, the saved joint still one of its
    /// corners by the two Line UUIDs alone, and the saved distance inside the
    /// policy on the NEW adjacent sides. Nothing is clamped.
    pub fn corner_on(&self, curves: &[crate::SketchCurve]) -> Result<ChamferCorner> {
        let corners = corners_of_lines(self.base_feature, curves)?;
        let corner = ChamferCorner::of(&corner_for(&corners, self.edge)?);
        corner.check_distance(self.distance_mm).map_err(|e| {
            CadError::input(format!(
                "Chamfer {} of {} mm does not fit the new plate: {e}",
                self.feature, self.distance_mm
            ))
        })?;
        Ok(corner)
    }

    /// The distance policy at this Chamfer's own corner. On a constrained base
    /// only the numbers that do not depend on the solved plate are checked here;
    /// the upper bound is the evaluator's, on the solved sides (§29D).
    pub fn check_distance(&self, distance_mm: f64) -> Result<()> {
        if self.constrained {
            return check_distance_value(distance_mm);
        }
        self.corner.check_distance(distance_mm)
    }
}

/// §29A: the saved Chamfer of this document, if it holds one: `Ok(None)` for
/// a document with none, and a typed refusal for a second Chamfer or for a
/// Chamfer outside the exact class — never a partial reading.
pub fn saved_chamfer(
    document: &Document,
    objects: &[ObjectRecord],
) -> Result<Option<SavedChamfer>> {
    let chamfers: Vec<&ObjectRecord> = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Chamfer(_)))
        .collect();
    let Some(record) = chamfers.first() else {
        return Ok(None);
    };
    if chamfers.len() > 1 {
        return Err(refuse_chamfered(objects).expect_err("two Chamfers are refused"));
    }
    saved_chamfer_for_edit(document, objects, record).map(Some)
}

/// The same reading for one selected feature.
pub(crate) fn saved_chamfer_for_edit(
    document: &Document,
    objects: &[ObjectRecord],
    record: &ObjectRecord,
) -> Result<SavedChamfer> {
    let ObjectPayload::Chamfer(chamfer) = &record.payload else {
        return Err(unsupported(format!(
            "object {} is {}, not a Chamfer",
            record.id,
            record.payload.type_name()
        )));
    };
    if objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Chamfer(_)))
        .count()
        > 1
    {
        return Err(refuse_chamfered(objects).expect_err("two Chamfers are refused"));
    }
    // Named first, as the creation reader does: a constraint outside the
    // constraint editor's managed Line family by its own UUID (§29D: the managed
    // family itself is the plate's to carry), and any other feature over the
    // plate or the Chamfer by its UUID, whatever the general history reader would
    // have made of them.
    for object in objects {
        if let ObjectPayload::Sketch(sketch) = &object.payload {
            require_managed_profile(sketch, record.id)?;
        }
    }
    if let Some(other) = objects.iter().find(|o| {
        o.id != record.id
            && o.id != chamfer.previous
            && (o
                .payload
                .previous_feature()
                .is_some_and(|p| p == record.id || p == chamfer.previous)
                || o.payload.kind().is_some_and(|k| k.is_feature()))
    }) {
        return Err(unsupported(format!(
            "feature {} ({}) is part of this document beside the plate and the Chamfer {}; this \
             build holds one Chamfer on one plate with no other feature",
            other.id,
            other.payload.type_name(),
            record.id
        )));
    }
    let history = crate::cut_edit::saved_history_under_chamfer(document, objects, record)?;
    if !history.cuts.is_empty() || history.target.base_feature != chamfer.previous {
        return Err(unsupported(
            "this build edits a Chamfer directly on the plate's base Extrude, with no Cut",
        ));
    }
    if chamfer.edge.feature != chamfer.previous {
        return Err(unsupported(
            "this build edits a Chamfer of an edge of the feature it consumes",
        ));
    }
    let profile = objects
        .iter()
        .find(|o| o.id == history.target.profile)
        .ok_or_else(|| unsupported("the part's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(unsupported("the part's profile is not a Sketch"));
    };
    require_managed_profile(sketch, record.id)?;
    let constrained =
        !sketch.constraints.is_empty() && !crate::sketch_constraints::closure_links_only(sketch);
    let corners: Vec<ChamferCorner> =
        corners_of_lines(history.target.base_feature, &sketch.curves)?
            .iter()
            .map(ChamferCorner::of)
            .collect();
    let corner = corners
        .iter()
        .find(|c| c.joint == chamfer.edge.joint)
        .copied()
        .ok_or_else(|| {
            unsupported(format!(
                "{} is not a corner of the plate this Chamfer cuts",
                chamfer.edge.joint
            ))
        })?;
    // The names it owns must be exactly the ones a Chamfer of its numbers is
    // given: no more, no fewer.
    let owner = record.id;
    let stored: Vec<TopologyRef> = document
        .topology_refs()?
        .into_iter()
        .filter(|r| r.owner == owner)
        .collect();
    let wanted = chamfer_references(
        owner,
        history.target.base_feature,
        chamfer.edge.joint,
        &history.target.profile_segments,
    );
    let mut remaining: Vec<&TopologyRef> = stored.iter().collect();
    for want in &wanted {
        let at = remaining
            .iter()
            .position(|r| crate::cut_edit::same_meaning(r, want))
            .ok_or_else(|| {
                unsupported(format!(
                    "the saved Chamfer {owner} does not name the faces this build gives a Chamfer"
                ))
            })?;
        remaining.remove(at);
    }
    if !remaining.is_empty() {
        return Err(unsupported(format!(
            "the saved Chamfer {owner} names more faces than a Chamfer of its numbers gives"
        )));
    }
    Ok(SavedChamfer {
        feature: owner,
        body: history.target.body,
        name: record.name.clone(),
        base_feature: history.target.base_feature,
        profile: history.target.profile,
        height_mm: history.target.height_mm,
        edge: chamfer.edge,
        distance_mm: chamfer.distance_mm,
        corner,
        constrained,
    })
}

/// One row per saved Chamfer, editable or with its reason, from one snapshot.
/// Kernel-free.
#[derive(Debug, Clone, PartialEq)]
pub struct ChamferDistanceChoice {
    pub feature: ObjectId,
    pub name: Option<String>,
    pub stored: Chamfer,
    pub saved: Option<SavedChamfer>,
    pub refusal: Option<String>,
}

pub fn chamfer_distance_choices(
    document: &Document,
    objects: &[ObjectRecord],
) -> Vec<ChamferDistanceChoice> {
    objects
        .iter()
        .filter_map(|o| match &o.payload {
            ObjectPayload::Chamfer(stored) => Some((o, stored.clone())),
            _ => None,
        })
        .map(|(o, stored)| {
            let saved = saved_chamfer_for_edit(document, objects, o).map_err(|e| e.to_string());
            ChamferDistanceChoice {
                feature: o.id,
                name: o.name.clone(),
                stored,
                refusal: saved.as_ref().err().cloned(),
                saved: saved.ok(),
            }
        })
        .collect()
}

/// A checked distance edit: the selected Chamfer row with only its distance
/// replaced, the saved facts it was read with, and the complete version of the
/// document it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedChamferDistance {
    pub(crate) feature: ObjectRecord,
    pub(crate) saved: SavedChamfer,
    pub(crate) source_version: ContentHash,
}

impl PreparedChamferDistance {
    pub fn feature(&self) -> &ObjectRecord {
        &self.feature
    }
    /// The Chamfer as saved, before the edit.
    pub fn saved(&self) -> &SavedChamfer {
        &self.saved
    }
    pub fn distance_mm(&self) -> f64 {
        match &self.feature.payload {
            ObjectPayload::Chamfer(c) => c.distance_mm,
            _ => f64::NAN,
        }
    }
    /// The Line UUIDs of the cut corner, in canonical order.
    pub fn joint(&self) -> [StableEntityId; 2] {
        self.saved.edge.joint.segments()
    }
}

pub fn prepare_chamfer_distance(
    document: &Document,
    feature: ObjectId,
    distance_mm: f64,
) -> Result<PreparedChamferDistance> {
    let objects = document.objects()?;
    let mut record = objects
        .iter()
        .find(|o| o.id == feature)
        .cloned()
        .ok_or_else(|| {
            CadError::input(format!("feature {feature} does not exist in this document"))
        })?;
    let saved = saved_chamfer_for_edit(document, &objects, &record)?;
    saved.check_distance(distance_mm)?;
    let ObjectPayload::Chamfer(chamfer) = &mut record.payload else {
        unreachable!("checked Chamfer")
    };
    chamfer.distance_mm = distance_mm;
    Ok(PreparedChamferDistance {
        feature: record,
        saved,
        source_version: document.content_version()?,
    })
}

/// The writer's check, against the snapshot the write consumes: the same
/// document version, and the same prepared value derived again from it.
pub(crate) fn rederive_distance(
    document: &Document,
    prepared: &PreparedChamferDistance,
) -> Result<()> {
    if document.content_version()? != prepared.source_version {
        return Err(CadError::input(
            "document changed after Chamfer distance preparation",
        ));
    }
    let ObjectPayload::Chamfer(chamfer) = &prepared.feature.payload else {
        return Err(CadError::input(
            "prepared Chamfer distance edit must carry a Chamfer",
        ));
    };
    let checked = prepare_chamfer_distance(document, prepared.feature.id, chamfer.distance_mm)?;
    if checked != *prepared {
        return Err(CadError::input(
            "prepared Chamfer distance edit does not describe the current document",
        ));
    }
    Ok(())
}

/// The Chamfer's class and distance policy, asked by the evaluator at every
/// cold and cached rebuild.
///
/// The **structure** is read from the saved objects: a forward Blind NewBody
/// Extrude with no other feature over it, whose profile is an axis-aligned
/// rectangle of four Lines with the saved joint at one of its corners, free or
/// closure-only. The **geometry** is `built`: the Lines the rebuild actually
/// built the plate from. On those, the same Lines in stored order must still be
/// an axis-aligned rectangle, have the saved joint as a corner, and leave room
/// for the saved distance. The one predicate every discovery and edit that
/// proposes a distance asks too.
pub fn evaluable_chamfer(
    objects: &[ObjectRecord],
    chamfer: &Chamfer,
    built: Option<&[crate::SketchCurve]>,
) -> Result<ChamferCorner> {
    let base = objects
        .iter()
        .find(|o| o.id == chamfer.previous)
        .ok_or_else(|| CadError::input("the Chamfer's predecessor is missing"))?;
    let ObjectPayload::Extrude(extrude) = &base.payload else {
        return Err(unsupported(format!(
            "this build chamfers an edge of an extruded plate, and the Chamfer's predecessor {} \
             is {}",
            base.id,
            base.payload.type_name()
        )));
    };
    if chamfer.edge.feature != base.id {
        return Err(unsupported(
            "this build chamfers an edge of the feature a Chamfer consumes",
        ));
    }
    if extrude.operation != crate::SolidOperation::NewBody
        || extrude.previous.is_some()
        || extrude.reversed
        || !matches!(extrude.end_condition, crate::EndCondition::Blind { .. })
    {
        return Err(unsupported(
            "this build chamfers an edge of a forward Blind NewBody extrusion",
        ));
    }
    // One Chamfer over the plate and nothing else: another feature over the
    // same result is a branch, and a Chamfer under a Fillet or a Cut is outside
    // the class.
    let over: Vec<&ObjectRecord> = objects
        .iter()
        .filter(|o| o.payload.previous_feature() == Some(base.id))
        .collect();
    if let Some(other) = over
        .iter()
        .find(|o| !matches!(&o.payload, ObjectPayload::Chamfer(_)))
    {
        return Err(unsupported(format!(
            "feature {} also consumes the plate a Chamfer cuts; this build holds one Chamfer on \
             a plate with nothing else over it",
            other.id
        )));
    }
    if let [first, second, ..] = over.as_slice() {
        return Err(unsupported(format!(
            "Chamfers {} and {} both cut this plate; this build holds one Chamfer on one plate",
            first.id, second.id
        )));
    }
    let profile = objects
        .iter()
        .find(|o| o.id == extrude.profile)
        .ok_or_else(|| CadError::input("the base Extrude's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(unsupported("the base Extrude's profile is not a Sketch"));
    };
    // §29D: the plate's Sketch may carry the managed Line family, whose Lines
    // the solver draws; the structure is asked of the stored ones and the
    // geometry, below, of the ones the rebuild built.
    // The saved row this payload is, by its meaning: the predecessor and the
    // edge it cuts, so a caller that asks about another distance still names it.
    let owner = objects
        .iter()
        .find(|o| {
            matches!(&o.payload, ObjectPayload::Chamfer(c)
                if c.previous == chamfer.previous && c.edge == chamfer.edge)
        })
        .map_or(chamfer.previous, |o| o.id);
    let chamfer_id = owner.to_string();
    require_managed_profile(sketch, owner)?;
    let stored = corners_of_lines(base.id, &sketch.curves)?;
    corner_for(&stored, chamfer.edge)?;
    let Some(built) = built else {
        return Err(CadError::input(format!(
            "the plate the Chamfer cuts was built from no profile {}",
            extrude.profile
        )));
    };
    // The Lines the rebuild built: the same Lines in stored order, a rectangle,
    // every Line on its side, the saved joint still a corner, room for the
    // saved distance. Each refusal names the Chamfer.
    let ids: Vec<_> = sketch.curves.iter().map(|c| c.id).collect();
    if built.iter().map(|c| c.id).collect::<Vec<_>>() != ids {
        return Err(CadError::input(format!(
            "Chamfer {chamfer_id}: the plate is not drawn by the same four Lines in their \
             stored order"
        )));
    }
    let corners = corners_of_lines(base.id, built).map_err(|e| {
        CadError::input(format!(
            "Chamfer {chamfer_id}: the solved plate is no longer an axis-aligned rectangle ({e})"
        ))
    })?;
    let start = |c: &crate::SketchCurve| match c.geometry {
        crate::SketchGeometry::Line { start, .. } => [start.x, start.y],
        _ => [f64::NAN; 2],
    };
    crate::fillet::keeps_every_side(
        &ids,
        &sketch.curves.iter().map(start).collect::<Vec<_>>(),
        &built.iter().map(start).collect::<Vec<_>>(),
    )
    .map_err(|e| {
        CadError::input(format!(
            "Chamfer {chamfer_id} keeps its corner only while every Line keeps its side: {e}"
        ))
    })?;
    let found = corner_for(&corners, chamfer.edge).map_err(|e| {
        CadError::input(format!(
            "Chamfer {chamfer_id}: its corner is not a corner of the solved plate ({e})"
        ))
    })?;
    let corner = ChamferCorner::of(&found);
    corner.check_distance(chamfer.distance_mm).map_err(|e| {
        CadError::input(format!(
            "Chamfer {chamfer_id} of {} mm does not fit the solved plate: {e}",
            chamfer.distance_mm
        ))
    })?;
    Ok(corner)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::{
        DatumPlane, EndCondition, Expression, Extrude, Point2, SketchCurve, SketchGeometry,
        SolidOperation,
    };
    use ferritecad_types::Transform;

    const PLATE: [[f64; 2]; 4] = [[-4.5, 3.25], [33., 3.25], [33., 15.5], [-4.5, 15.5]];

    fn sketch(corners: &[[f64; 2]]) -> Sketch {
        let n = corners.len();
        Sketch {
            plane: ObjectId::new(),
            curves: (0..n)
                .map(|i| crate::SketchCurve {
                    id: StableEntityId::new(),
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: Point2::new(corners[i][0], corners[i][1]).expect("point"),
                        end: Point2::new(corners[(i + 1) % n][0], corners[(i + 1) % n][1])
                            .expect("point"),
                    },
                })
                .collect(),
            constraints: Vec::new(),
        }
    }

    /// One plate written straight into a document, as a creator would.
    fn plate(corners: [[f64; 2]; 4]) -> (tempfile::TempDir, Document, ObjectId) {
        let root = tempfile::tempdir().expect("dir");
        let mut d = Document::create(root.path().join("plate.fcad")).expect("document");
        let [plane, profile, extrude, body] = std::array::from_fn(|_| ObjectId::new());
        let mut drawn = sketch(&corners);
        drawn.plane = plane;
        d.write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("XY"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            )?;
            w.put_object(
                profile,
                None,
                1,
                Some("Profile"),
                &ObjectPayload::Sketch(drawn),
            )?;
            w.put_object(
                extrude,
                None,
                2,
                Some("Extrude1"),
                &ObjectPayload::Extrude(Extrude {
                    profile,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(6.75)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    target_body: None,
                    previous: None,
                }),
            )?;
            w.put_object(
                body,
                None,
                3,
                Some("Body"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(extrude),
                }),
            )?;
            for (dependent, dependency, role) in [
                (profile, plane, DependencyRole::Plane),
                (extrude, profile, DependencyRole::Profile),
                (body, extrude, DependencyRole::BodyTip),
            ] {
                w.add_dependency(Dependency {
                    dependent,
                    dependency,
                    role,
                })?;
            }
            Ok(())
        })
        .expect("fixture");
        (root, d, body)
    }

    fn target_of(d: &Document, body: ObjectId) -> SavedChamferTarget {
        let objects = d.objects().expect("objects");
        saved_target(d, &objects, body).expect("a plate to chamfer")
    }

    /// Writes one Chamfer through the same prepare/write the CLI uses.
    fn chamfer_at(d: &mut Document, body: ObjectId, at: usize, distance_mm: f64) -> ObjectId {
        let t = target_of(d, body);
        let prepared = prepare_edge_chamfer(
            d,
            body,
            &EdgeChamfer {
                edge: SweptEdge {
                    feature: t.base_feature,
                    joint: t.corners[at].joint,
                },
                distance_mm,
            },
        )
        .expect("a chamfer");
        d.write_edge_chamfer(&prepared).expect("written");
        prepared.feature().id
    }

    /// §29A: the stored payload round-trips under its own kind and capability,
    /// refuses a distance no chamfer has and a field this reader would drop,
    /// and an edge of a feature the Chamfer does not consume.
    #[test]
    fn the_chamfer_payload_round_trips_and_refuses_what_it_cannot_mean() {
        let [a, b] = [(); 2].map(|_| StableEntityId::new());
        let base = ObjectId::new();
        let chamfer = Chamfer {
            previous: base,
            edge: SweptEdge {
                feature: base,
                joint: ProfileJoint::new(a, b).expect("joint"),
            },
            distance_mm: 2.5,
        };
        let payload = ObjectPayload::Chamfer(chamfer.clone());
        assert_eq!(payload.type_name(), "feature.chamfer");
        assert_eq!(payload.schema_version(), 1);
        assert_eq!(
            payload.required_capabilities(),
            [
                "core.part.v1",
                "feature.predecessor.v1",
                "feature.chamfer.v1"
            ]
        );
        let bytes = payload.to_storage_bytes().expect("encodes");
        assert_eq!(
            ObjectPayload::from_storage_bytes(&bytes).expect("decodes"),
            payload
        );
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut c = chamfer.clone();
            c.distance_mm = bad;
            assert!(
                ObjectPayload::Chamfer(c).to_storage_bytes().is_err(),
                "{bad}"
            );
        }
        let mut elsewhere = chamfer.clone();
        elsewhere.edge.feature = ObjectId::new();
        assert!(
            ObjectPayload::Chamfer(elsewhere)
                .to_storage_bytes()
                .is_err(),
            "an edge of another feature"
        );
        // A key this reader does not know is refused, not dropped.
        let mut value: ciborium::Value =
            ciborium::from_reader(&bytes[..]).expect("an envelope is cbor");
        let _ = &mut value;
        assert!(
            crate::ObjectKind::parse("feature.chamfer").is_some_and(|k| k.is_feature()),
            "it is a feature of this build"
        );
    }

    /// §29A: the policy is one expression: the largest distance offered is
    /// accepted, the next representable value above it is refused, and
    /// nothing below the minimum or non-finite is.
    #[test]
    fn the_distance_policy_is_exact_at_its_bounds_and_the_same_everywhere() {
        let (_root, d, body) = plate(PLATE);
        let t = target_of(&d, body);
        for corner in &t.corners {
            let shorter = corner.adjacent_lengths_mm[0].min(corner.adjacent_lengths_mm[1]);
            assert_eq!(corner.max_distance_mm, shorter - MIN_FLAT_MM);
            assert_eq!(
                corner.max_distance_mm,
                max_distance_of(corner.adjacent_lengths_mm)
            );
            assert!(corner.is_offerable());
            corner
                .check_distance(corner.max_distance_mm)
                .expect("the offered maximum is accepted");
            let next = corner.max_distance_mm.next_up();
            assert!(next > corner.max_distance_mm);
            let refusal = corner
                .check_distance(next)
                .expect_err("the next float")
                .to_string();
            assert!(refusal.contains("too large") && refusal.contains("nothing is clamped"));
            corner.check_distance(MIN_DISTANCE_MM).expect("the minimum");
            assert!(corner.check_distance(MIN_DISTANCE_MM.next_down()).is_err());
            for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300] {
                assert!(corner.check_distance(bad).is_err(), "{bad}");
            }
        }
        // A corner whose sides leave no distance at all is listed and refused.
        let (_r, thin, body) = plate([[0., 0.], [30., 0.], [30., 0.0105], [0., 0.0105]]);
        let t = target_of(&thin, body);
        assert!(t.corners.iter().all(|c| !c.is_offerable()));
        assert!(t.corners[0].check_distance(MIN_DISTANCE_MM).is_err());
    }

    /// §29A: a Chamfer is created at each of the four corners, on a translated
    /// fractional plate in either winding, names exactly the seven faces and
    /// reads back through the one reader with its distance and corner.
    #[test]
    fn a_chamfer_is_written_at_every_corner_in_both_windings_and_reads_back() {
        let ccw = PLATE;
        let cw = [ccw[3], ccw[2], ccw[1], ccw[0]];
        for corners in [ccw, cw] {
            for at in 0..4 {
                let (_root, mut d, body) = plate(corners);
                let before = d.content_version().expect("version");
                let t = target_of(&d, body);
                let corner = t.corners[at];
                let id = chamfer_at(&mut d, body, at, 2.375);
                assert_ne!(d.content_version().expect("version"), before);
                let objects = d.objects().expect("objects");
                assert!(d.validate().expect("validates").is_ok());
                let saved = saved_chamfer(&d, &objects)
                    .expect("reads")
                    .expect("a Chamfer");
                assert_eq!(saved.feature, id);
                assert_eq!(saved.distance_mm, 2.375);
                assert_eq!(saved.edge.joint, corner.joint);
                assert_eq!(saved.corner.corner_mm, corner.corner_mm);
                assert_eq!(saved.height_mm, 6.75);
                // Seven names, one in its own role, the rest the plate's.
                let refs: Vec<_> = d
                    .topology_refs()
                    .expect("refs")
                    .into_iter()
                    .filter(|r| r.owner == id)
                    .collect();
                assert_eq!(refs.len(), 7);
                assert_eq!(
                    refs.iter()
                        .filter(|r| matches!(r.output_role, SemanticRole::EdgeChamferFace { .. }))
                        .count(),
                    1
                );
                assert!(refs.iter().all(|r| r.producer_feature == id));
                let tip = match &objects.iter().find(|o| o.id == body).expect("body").payload {
                    ObjectPayload::Body(b) => b.tip_feature,
                    _ => None,
                };
                assert_eq!(tip, Some(id), "the Body ends in the Chamfer");
            }
        }
    }

    /// §29A: only the Chamfer's distance changes, in its own row, and its
    /// references keep their UUIDs; a stale or forged preparation is refused.
    #[test]
    fn the_distance_edit_changes_one_number_and_the_writer_refuses_forgery() {
        let (_root, mut d, body) = plate(PLATE);
        let id = chamfer_at(&mut d, body, 1, 2.375);
        let refs_before = d.topology_refs().expect("refs");
        let objects_before = d.objects().expect("objects");

        let prepared = prepare_chamfer_distance(&d, id, 4.5).expect("a new distance");
        assert_eq!(prepared.distance_mm(), 4.5);
        assert_eq!(prepared.saved().distance_mm, 2.375);
        d.write_chamfer_distance(&prepared).expect("written");
        let objects = d.objects().expect("objects");
        assert_eq!(d.topology_refs().expect("refs"), refs_before);
        for (was, now) in objects_before.iter().zip(&objects) {
            assert_eq!(was.id, now.id);
            if was.id == id {
                assert_ne!(was.payload, now.payload);
            } else {
                assert_eq!(was.payload, now.payload, "{}", was.id);
            }
        }
        // Stale: the document moved on after preparation.
        let stale = prepare_chamfer_distance(&d, id, 3.0).expect("prepared");
        let again = prepare_chamfer_distance(&d, id, 5.0).expect("prepared");
        d.write_chamfer_distance(&again).expect("written");
        assert!(d.write_chamfer_distance(&stale).is_err());
        // Forged: a distance the policy refuses, and another feature's row.
        let mut forged = prepare_chamfer_distance(&d, id, 3.0).expect("prepared");
        if let ObjectPayload::Chamfer(c) = &mut forged.feature.payload {
            c.distance_mm = 1.0e6;
        }
        let before = d.content_version().expect("version");
        assert!(d.write_chamfer_distance(&forged).is_err());
        assert_eq!(
            d.content_version().expect("version"),
            before,
            "nothing written"
        );
        let mut other = prepare_chamfer_distance(&d, id, 3.0).expect("prepared");
        other.feature.id = body;
        assert!(d.write_chamfer_distance(&other).is_err());
        // The numbers the policy refuses are refused before any preparation.
        let max = target_of_saved(&d, id).corner.max_distance_mm;
        prepare_chamfer_distance(&d, id, max).expect("the maximum");
        assert!(prepare_chamfer_distance(&d, id, max.next_up()).is_err());
        assert!(prepare_chamfer_distance(&d, id, f64::NAN).is_err());
    }

    fn target_of_saved(d: &Document, feature: ObjectId) -> SavedChamfer {
        let objects = d.objects().expect("objects");
        let record = objects.iter().find(|o| o.id == feature).expect("feature");
        saved_chamfer_for_edit(d, &objects, record).expect("saved")
    }

    /// §29A: the creation writer re-derives everything: a forged corner, a
    /// distance past the bound, a changed tip and a second Chamfer are refused
    /// and write nothing.
    #[test]
    fn the_creation_writer_rederives_the_chamfer_and_refuses_forgery() {
        let (_root, mut d, body) = plate(PLATE);
        let t = target_of(&d, body);
        let edge = |at: usize| SweptEdge {
            feature: t.base_feature,
            joint: t.corners[at].joint,
        };
        let good = EdgeChamfer {
            edge: edge(0),
            distance_mm: 2.0,
        };
        let prepared = prepare_edge_chamfer(&d, body, &good).expect("prepared");
        let before = d.content_version().expect("version");
        // A distance past the corner's bound.
        let mut forged = prepared.clone();
        if let ObjectPayload::Chamfer(c) = &mut forged.feature.payload {
            c.distance_mm = 1.0e6;
        }
        assert!(d.write_edge_chamfer(&forged).is_err());
        // Another corner than the one the names were minted for.
        let mut elsewhere = prepared.clone();
        if let ObjectPayload::Chamfer(c) = &mut elsewhere.feature.payload {
            c.edge = edge(1);
        }
        assert!(d.write_edge_chamfer(&elsewhere).is_err());
        // A name that is not the plate's.
        let mut renamed = prepared.clone();
        renamed.references[0].output_role = SemanticRole::EdgeFilletFace {
            edge_feature: t.base_feature,
            joint: t.corners[0].joint,
        };
        assert!(d.write_edge_chamfer(&renamed).is_err());
        // A dropped name.
        let mut short = prepared.clone();
        short.references.pop();
        assert!(d.write_edge_chamfer(&short).is_err());
        assert_eq!(
            d.content_version().expect("version"),
            before,
            "nothing written"
        );
        d.write_edge_chamfer(&prepared).expect("the honest one");
        // The same preparation again: the tip moved.
        assert!(d.write_edge_chamfer(&prepared).is_err());
        // A second Chamfer, and a Chamfer on a Body that ends in one.
        let second = prepare_edge_chamfer(
            &d,
            body,
            &EdgeChamfer {
                edge: edge(2),
                distance_mm: 1.0,
            },
        )
        .expect_err("a second Chamfer")
        .to_string();
        assert!(second.contains("Chamfer"), "{second}");
    }

    /// §29A: every reader of the plate refuses a chamfered Body by naming the
    /// Chamfer, and discovery says so: none silently drops, ignores or
    /// rebuilds it.
    #[test]
    fn every_other_editor_refuses_a_chamfered_body_by_name() {
        let (_root, mut d, body) = plate(PLATE);
        let id = chamfer_at(&mut d, body, 3, 2.0);
        let objects = d.objects().expect("objects");
        let base = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
            .expect("base")
            .id;
        let profile = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("sketch")
            .id;
        let named = |what: &str, e: ferritecad_types::CadError| {
            let text = e.to_string();
            assert!(
                text.contains(&id.to_string()) && text.contains("Chamfer"),
                "{what}: {text}"
            );
        };
        // §29B: the plate's height is the one edit besides the distance; asked
        // about the Chamfer itself, the extrusion editor names it.
        let height = crate::prepare_extrude_height(&d, id, 9.0)
            .expect_err("height")
            .to_string();
        assert!(height.contains(&id.to_string()), "{height}");
        named(
            "a Fillet",
            crate::prepare_edge_fillet(
                &d,
                body,
                &crate::EdgeFillet {
                    edge: crate::FilletEdge {
                        feature: base,
                        joint: target_of_saved(&d, id).edge.joint,
                    },
                    radius_mm: 1.0,
                },
            )
            .expect_err("a Fillet"),
        );
        named(
            "a Cut",
            crate::prepare_circular_cut(
                &d,
                body,
                &crate::CircularCut {
                    center_mm: [10.0, 8.0],
                    radius_mm: 1.0,
                    extent: crate::CutExtent::Blind { depth_mm: 1.0 },
                },
            )
            .expect_err("a Cut"),
        );
        // §29D: the constraint editor reads the plate through the same reader;
        // an empty request is still no request, but a Horizontal addition on the
        // plate's first Line is accepted with the Chamfer as its context.
        let line = sketch_of(&d).curves[0].id;
        let edit = crate::SketchConstraintEdits {
            remove: Vec::new(),
            add: vec![crate::AddSketchConstraint::Line(
                crate::AddLineConstraint::Line {
                    curve: line,
                    kind: crate::LineConstraintKind::Horizontal,
                },
            )],
        };
        let prepared = crate::prepare_sketch_constraints(&d, profile, &edit)
            .expect("the constraint editor accepts the chamfered plate");
        assert_eq!(prepared.chamfer().map(|c| c.feature), Some(id));
        assert!(
            crate::prepare_sketch_constraints(
                &d,
                profile,
                &crate::SketchConstraintEdits::default()
            )
            .is_err(),
            "an empty request is still no request"
        );
        let fillet_radius = crate::prepare_fillet_radius(&d, id, 1.0).expect_err("not a Fillet");
        assert!(fillet_radius.to_string().contains("Fillet"));
        // Discovery reports the saved Chamfer; §29B: the extrusion editor is
        // offered the base Extrude under it, with the Chamfer as its context.
        let source = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        assert!(source.filleted.is_none(), "{:?}", source.filleted);
        assert_eq!(
            source.features[0].chamfer.as_ref().map(|c| c.feature),
            Some(id)
        );
        assert_eq!(source.chamfer_features.len(), 1);
        assert!(source.chamfer_features[0].refusal.is_none());
        assert!(source.chamfer_bodies[0].target.is_none());
        assert!(
            source.chamfer_bodies[0]
                .refusal
                .as_deref()
                .is_some_and(|r| r.contains("Chamfer"))
        );
    }

    /// §29B: the plate's height changes under the Chamfer through the one
    /// extrusion editor: one row, nothing else, the Chamfer and every name
    /// as they were; the writer re-derives and refuses stale and forged edits.
    #[test]
    fn the_height_of_a_chamfered_plate_changes_one_row_and_the_writer_refuses_forgery() {
        let (_root, mut d, body) = plate(PLATE);
        let id = chamfer_at(&mut d, body, 2, 2.375);
        let base = target_of_base(&d);
        let source = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        assert!(source.filleted.is_none() && source.unavailable_reason().is_none());
        let choice = &source.features[0];
        assert_eq!(choice.feature, base);
        assert!(choice.refusal.is_none(), "{:?}", choice.refusal);
        assert_eq!(
            choice.chamfer.as_ref().map(|c| (c.feature, c.distance_mm)),
            Some((id, 2.375))
        );
        assert!(choice.cut_history.is_none() && choice.fillets.is_empty());

        let objects = d.objects().expect("objects");
        let refs = d.topology_refs().expect("refs");
        let deps = d.dependencies().expect("dependencies");
        for height in [9.5, 0.4, 6.75] {
            let prepared = crate::prepare_extrude_height(&d, base, height).expect("prepared");
            assert_eq!(prepared.chamfer().map(|c| c.feature), Some(id));
            assert!(prepared.added_references().is_empty());
            d.write_extrude_height(&prepared).expect("written");
            let now = d.objects().expect("objects");
            for (was, is) in objects.iter().zip(&now) {
                assert_eq!(was.id, is.id);
                if was.id == base {
                    let ObjectPayload::Extrude(e) = &is.payload else {
                        panic!("not an Extrude")
                    };
                    assert!(matches!(&e.end_condition, EndCondition::Blind { distance }
                        if distance.value() == height));
                } else {
                    assert_eq!(was.payload, is.payload, "{} moved", was.id);
                }
            }
            assert_eq!(d.topology_refs().expect("refs"), refs, "no name moved");
            assert_eq!(d.dependencies().expect("dependencies"), deps);
            assert!(d.validate().expect("validates").is_ok());
            let saved = saved_chamfer(&d, &now).expect("reads").expect("a Chamfer");
            assert_eq!((saved.distance_mm, saved.height_mm), (2.375, height));
        }
        // Stale: the document moved on after preparation.
        let stale = crate::prepare_extrude_height(&d, base, 3.0).expect("prepared");
        let again = crate::prepare_extrude_height(&d, base, 5.0).expect("prepared");
        d.write_extrude_height(&again).expect("written");
        assert!(d.write_extrude_height(&stale).is_err());
        // Forged: another feature's row, the Chamfer taken out, the Chamfer
        // changed, a joint that is not the saved one. Nothing is written.
        let before = d.content_version().expect("version");
        let mut other = crate::prepare_extrude_height(&d, base, 3.0).expect("prepared");
        other.feature.id = id;
        assert!(d.write_extrude_height(&other).is_err());
        let mut without = crate::prepare_extrude_height(&d, base, 3.0).expect("prepared");
        without.chamfer = None;
        assert!(d.write_extrude_height(&without).is_err());
        let mut moved = crate::prepare_extrude_height(&d, base, 3.0).expect("prepared");
        if let Some(c) = moved.chamfer.as_mut() {
            c.distance_mm = 1.0;
        }
        assert!(d.write_extrude_height(&moved).is_err());
        let mut rejoined = crate::prepare_extrude_height(&d, base, 3.0).expect("prepared");
        if let Some(c) = rejoined.chamfer.as_mut() {
            let lines = &sketch_of(&d).curves;
            c.edge.joint = ProfileJoint::new(lines[0].id, lines[1].id).expect("joint");
            assert_ne!(c.edge.joint, target_of_saved(&d, id).edge.joint);
        }
        assert!(d.write_extrude_height(&rejoined).is_err());
        assert_eq!(
            d.content_version().expect("version"),
            before,
            "nothing written"
        );
        // The numbers the shared rule refuses are refused before preparation.
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(
                crate::prepare_extrude_height(&d, base, bad).is_err(),
                "{bad}"
            );
        }
        // The distance edit still works on the edited plate, and the Chamfer
        // keeps its bound: it depends on the adjacent sides, not on the height.
        let max = target_of_saved(&d, id).corner.max_distance_mm;
        assert_eq!(max, 12.25 - MIN_FLAT_MM);
        let edit = prepare_chamfer_distance(&d, id, max).expect("distance after the height");
        d.write_chamfer_distance(&edit).expect("written");
    }

    /// §29B: a Chamfer outside the class is refused by the extrusion editor
    /// with the guilty feature's UUID, and nothing is read as some other history.
    #[test]
    fn the_height_edit_refuses_a_chamfer_outside_the_class_naming_the_feature() {
        // A dimension: the constraint's own UUID.
        let (_r, mut d, body) = plate(PLATE);
        chamfer_at(&mut d, body, 0, 2.0);
        let base = target_of_base(&d);
        let objects = d.objects().expect("objects");
        let record = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("sketch")
            .clone();
        let ObjectPayload::Sketch(mut sketch) = record.payload.clone() else {
            unreachable!()
        };
        let pin = crate::SketchConstraint {
            id: StableEntityId::new(),
            rule: SketchConstraintRule::Distance {
                a: crate::SketchPointRef::new(
                    sketch.curves[0].id,
                    crate::SketchPointSelector::Start,
                ),
                b: crate::SketchPointRef::new(sketch.curves[1].id, crate::SketchPointSelector::End),
                distance: 37.5,
            },
        };
        sketch.constraints.push(pin);
        d.write(|w| {
            w.put_object(
                record.id,
                record.parent,
                record.ordinal,
                record.name.as_deref(),
                &ObjectPayload::Sketch(sketch),
            )
        })
        .expect("constrained");
        let why = crate::prepare_extrude_height(&d, base, 9.0)
            .expect_err("a dimension")
            .to_string();
        assert!(
            why.contains(&pin.id.to_string()) && why.contains("Distance"),
            "{why}"
        );
        let source = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        assert!(
            source
                .filleted
                .as_deref()
                .is_some_and(|r| r.contains(&pin.id.to_string()))
        );
        assert!(source.unavailable_reason().is_some());
        assert!(source.features[0].chamfer.is_none() && source.features[0].refusal.is_some());

        // Another feature over the Chamfer: a Fillet, and a Cut.
        for kind in ["Fillet", "Cut"] {
            let (_r, mut d, body) = plate(PLATE);
            let id = chamfer_at(&mut d, body, 1, 2.0);
            let base = target_of_base(&d);
            let intruder = ObjectId::new();
            let payload = if kind == "Fillet" {
                ObjectPayload::Fillet(crate::Fillet {
                    previous: id,
                    edge: crate::FilletEdge {
                        feature: base,
                        joint: target_of_saved(&d, id).edge.joint,
                    },
                    radius_mm: 1.0,
                })
            } else {
                ObjectPayload::Extrude(Extrude {
                    profile: target_of_saved(&d, id).profile,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(1.0).expect("distance"),
                    },
                    reversed: false,
                    operation: SolidOperation::Cut,
                    target_body: None,
                    previous: Some(id),
                })
            };
            d.write(|w| w.put_object(intruder, None, 9, Some("Intruder"), &payload))
                .expect("written");
            let why = crate::prepare_extrude_height(&d, base, 9.0)
                .expect_err(kind)
                .to_string();
            assert!(why.contains(&intruder.to_string()), "{kind}: {why}");
            let source = crate::ExtrudeEditSource::read(&d).expect("catalogue");
            assert!(source.unavailable_reason().is_some(), "{kind}");
            assert!(
                source.features.iter().all(|f| f.chamfer.is_none()),
                "{kind}"
            );
        }

        // A second Chamfer: named by the first, both refused.
        let (_r, mut d, body) = plate(PLATE);
        let id = chamfer_at(&mut d, body, 1, 2.0);
        let base = target_of_base(&d);
        let second = ObjectId::new();
        let row = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| o.id == id)
            .expect("chamfer");
        d.write(|w| w.put_object(second, None, 9, Some("Second"), &row.payload))
            .expect("written");
        let why = crate::prepare_extrude_height(&d, base, 9.0)
            .expect_err("two")
            .to_string();
        assert!(
            why.contains(&id.to_string()) && why.contains("2 Chamfers"),
            "{why}"
        );

        // Not the base: only the base Extrude under the Chamfer is edited.
        let (_r, mut d, body) = plate(PLATE);
        let id = chamfer_at(&mut d, body, 1, 2.0);
        let other = ObjectId::new();
        let profile = target_of_saved(&d, id).profile;
        d.write(|w| {
            w.put_object(
                other,
                None,
                9,
                Some("Other"),
                &ObjectPayload::Extrude(Extrude {
                    profile,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(3.0).expect("distance"),
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    target_body: None,
                    previous: None,
                }),
            )
        })
        .expect("written");
        let why = crate::prepare_extrude_height(&d, other, 9.0)
            .expect_err("not the base")
            .to_string();
        assert!(why.contains(&other.to_string()), "{why}");
    }

    /// §29A: free and closure-only plates are accepted; any other constraint
    /// is refused with its kind and UUID.
    #[test]
    fn a_dimensioned_plate_is_refused_with_the_constraint_named() {
        let (_root, mut d, body) = plate(PLATE);
        let objects = d.objects().expect("objects");
        let record = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("sketch")
            .clone();
        let ObjectPayload::Sketch(mut sketch) = record.payload.clone() else {
            unreachable!()
        };
        let first = sketch.curves[0].id;
        let second = sketch.curves[1].id;
        let pin = crate::SketchConstraint {
            id: StableEntityId::new(),
            rule: SketchConstraintRule::Distance {
                a: crate::SketchPointRef::new(first, crate::SketchPointSelector::Start),
                b: crate::SketchPointRef::new(second, crate::SketchPointSelector::End),
                distance: 37.5,
            },
        };
        sketch.constraints.push(pin);
        d.write(|w| {
            w.put_object(
                record.id,
                record.parent,
                record.ordinal,
                record.name.as_deref(),
                &ObjectPayload::Sketch(sketch),
            )
        })
        .expect("constrained");
        let refusal = prepare_edge_chamfer(
            &d,
            body,
            &EdgeChamfer {
                edge: SweptEdge {
                    feature: target_of_base(&d),
                    joint: ProfileJoint::new(first, second).expect("joint"),
                },
                distance_mm: 1.0,
            },
        )
        .expect_err("a dimension")
        .to_string();
        assert!(
            refusal.contains("Distance") && refusal.contains(&pin.id.to_string()),
            "{refusal}"
        );
    }

    fn sketch_of(d: &Document) -> Sketch {
        d.objects()
            .expect("objects")
            .into_iter()
            .find_map(|o| match o.payload {
                ObjectPayload::Sketch(s) => Some(s),
                _ => None,
            })
            .expect("sketch")
    }

    fn target_of_base(d: &Document) -> ObjectId {
        d.objects()
            .expect("objects")
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
            .expect("base")
            .id
    }

    /// §29A: the evaluator's predicate, on the Lines the rebuild built: the
    /// same bound, the saved joint, a branch or a second feature refused.
    #[test]
    fn the_evaluator_judges_the_chamfer_on_the_built_lines() {
        let (_root, mut d, body) = plate(PLATE);
        let id = chamfer_at(&mut d, body, 0, 2.5);
        let objects = d.objects().expect("objects");
        let chamfer = match &objects
            .iter()
            .find(|o| o.id == id)
            .expect("chamfer")
            .payload
        {
            ObjectPayload::Chamfer(c) => c.clone(),
            _ => unreachable!(),
        };
        let built_of = |objects: &[ObjectRecord]| -> Vec<SketchCurve> {
            objects
                .iter()
                .find_map(|o| match &o.payload {
                    ObjectPayload::Sketch(s) => Some(s.curves.clone()),
                    _ => None,
                })
                .expect("sketch")
        };
        let built = built_of(&objects);
        let corner = evaluable_chamfer(&objects, &chamfer, Some(&built)).expect("accepted");
        assert_eq!(corner.joint, chamfer.edge.joint);
        // The Lines as built are shorter than the saved distance allows.
        let mut thin = built.clone();
        for curve in &mut thin {
            if let SketchGeometry::Line { start, end } = &mut curve.geometry {
                start.y *= 0.2;
                end.y *= 0.2;
                start.x *= 0.05;
                end.x *= 0.05;
            }
        }
        let mut too_much = chamfer.clone();
        too_much.distance_mm = corner.max_distance_mm;
        evaluable_chamfer(&objects, &too_much, Some(&built)).expect("at the bound");
        assert!(evaluable_chamfer(&objects, &too_much, Some(&thin)).is_err());
        assert!(evaluable_chamfer(&objects, &chamfer, None).is_err());
        // The built Lines keep the saved corner or the Chamfer refuses.
        let mut other_joint = chamfer.clone();
        other_joint.edge.joint =
            ProfileJoint::new(StableEntityId::new(), StableEntityId::new()).expect("joint");
        assert!(evaluable_chamfer(&objects, &other_joint, Some(&built)).is_err());
        let mut foreign = chamfer.clone();
        foreign.previous = body;
        assert!(evaluable_chamfer(&objects, &foreign, Some(&built)).is_err());
    }

    /// The saved Line starts of the plate's Sketch, each sent to the same
    /// corner of `rect` (`[x0, y0, width, depth]`), in saved order.
    fn vertices_in(d: &Document, rect: [f64; 4]) -> Vec<crate::SketchVertex> {
        let lines = sketch_of(d).curves;
        let starts: Vec<(StableEntityId, Point2)> = lines
            .iter()
            .map(|c| match c.geometry {
                SketchGeometry::Line { start, .. } => (c.id, start),
                _ => panic!("a Line"),
            })
            .collect();
        let lx = starts.iter().map(|s| s.1.x).fold(f64::MAX, f64::min);
        let ly = starts.iter().map(|s| s.1.y).fold(f64::MAX, f64::min);
        starts
            .into_iter()
            .map(|(id, p)| crate::SketchVertex {
                curve_id: id,
                start_mm: [
                    if p.x == lx {
                        rect[0]
                    } else {
                        rect[0] + rect[2]
                    },
                    if p.y == ly {
                        rect[1]
                    } else {
                        rect[1] + rect[3]
                    },
                ],
            })
            .collect()
    }

    /// §29C: the coordinates of the chamfered plate's Sketch are edited under
    /// one rule: every Line keeps its side, the saved corner is found again by
    /// its two Line UUIDs, and the saved distance is checked, exactly and
    /// without being reduced, on the NEW adjacent sides.
    #[test]
    fn a_chamfered_plates_sketch_keeps_its_corner_and_distance_exactly() {
        let (_root, mut d, body) = plate(PLATE);
        let distance = 2.5;
        let id = chamfer_at(&mut d, body, 1, distance);
        let profile = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("Sketch")
            .id;
        let objects = d.objects().expect("objects");
        let choice = crate::sketch_choices(&d, &objects)
            .into_iter()
            .find(|c| c.sketch == profile)
            .expect("the Sketch is listed");
        assert!(choice.refusal.is_none(), "{:?}", choice.refusal);
        assert_eq!(choice.chamfer.as_ref().map(|c| c.feature), Some(id));
        assert!(choice.fillets.is_empty() && choice.cut_history.is_none());

        // The exact bound of the policy's own expression.
        let fits = |side: f64| side - MIN_FLAT_MM >= distance;
        let mut bound = distance + MIN_FLAT_MM;
        while !fits(bound) {
            bound = bound.next_up();
        }
        while fits(bound.next_down()) {
            bound = bound.next_down();
        }
        let (kept_before, refs, deps) = (
            d.objects().expect("objects"),
            d.topology_refs().expect("refs"),
            d.dependencies().expect("dependencies"),
        );
        let mut version = d.content_version().expect("version");
        let refusal = |d: &Document, rect: [f64; 4]| {
            crate::replace_sketch_coordinates(d, profile, &vertices_in(d, rect))
                .expect_err("refused")
                .to_string()
        };
        for (what, rect) in [
            ("depth", [0.0, 0.0, 40.0, bound.next_down()]),
            ("width", [0.0, 0.0, bound.next_down(), 40.0]),
        ] {
            let text = refusal(&d, rect);
            assert!(
                text.contains(&id.to_string()) && text.contains("does not fit"),
                "{what}: {text}"
            );
        }
        // Not a plate any more, mirrored, flipped, degenerate.
        assert!(!refusal(&d, [30.0, 0.0, -20.0, 10.0]).is_empty());
        // Turned half a turn keeps the winding but would move the Chamfer to
        // the opposite corner of the part: refused by the side rule, naming it.
        let half = refusal(&d, [30.0, 20.0, -20.0, -10.0]);
        assert!(
            half.contains(&id.to_string()) && half.contains("side"),
            "{half}"
        );
        assert!(!refusal(&d, [0.0, 0.0, 40.0, 0.0]).is_empty());
        let mut crooked = vertices_in(&d, [0.0, 0.0, 40.0, 20.0]);
        crooked[2].start_mm[0] += 1.0;
        crooked[2].start_mm[1] += 0.5;
        assert!(crate::replace_sketch_coordinates(&d, profile, &crooked).is_err());
        let mut reordered = vertices_in(&d, [0.0, 0.0, 40.0, 20.0]);
        reordered.swap(0, 1);
        assert!(crate::replace_sketch_coordinates(&d, profile, &reordered).is_err());
        assert_eq!(
            d.content_version().expect("version"),
            version,
            "nothing written"
        );

        // The bound itself is accepted, the distance is kept and only the
        // Sketch's row moves.
        for rect in [
            [0.0, 0.0, 40.0, bound],
            [0.0, 0.0, bound, 40.0],
            [7.5, -3.25, 52.0, 30.5],
        ] {
            d.write_sketch_coordinates(profile, &vertices_in(&d, rect))
                .expect("accepted");
            assert_ne!(d.content_version().expect("version"), version);
            version = d.content_version().expect("version");
            let now = d.objects().expect("objects");
            for (was, is) in kept_before.iter().zip(&now) {
                assert_eq!(was.id, is.id);
                if was.id != profile {
                    assert_eq!(was.payload, is.payload, "{} moved", was.id);
                }
            }
            assert_eq!(d.topology_refs().expect("refs"), refs);
            assert_eq!(d.dependencies().expect("dependencies"), deps);
            assert!(d.validate().expect("validates").is_ok());
            let saved = saved_chamfer(&d, &now).expect("reads").expect("a Chamfer");
            assert_eq!(saved.distance_mm, distance, "never reduced");
            assert_eq!(saved.edge.joint, target_of_saved(&d, id).edge.joint);
        }
    }

    /// §29C: the writer re-derives a prepared coordinate payload inside its
    /// transaction: a forged payload (the corner moved to a non-plate) and a
    /// stale one are refused and write nothing.
    #[test]
    fn the_coordinate_writer_rederives_under_a_chamfer_and_refuses_forgery() {
        let (_root, mut d, body) = plate(PLATE);
        chamfer_at(&mut d, body, 3, 2.0);
        let profile = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("Sketch")
            .id;
        let before = d.content_version().expect("version");
        let prepared = crate::replace_sketch_coordinates(
            &d,
            profile,
            &vertices_in(&d, [1.0, 2.0, 50.0, 25.0]),
        )
        .expect("prepared");
        let mut forged = prepared.clone();
        if let ObjectPayload::Sketch(s) = &mut forged.payload
            && let SketchGeometry::Line { start, .. } = &mut s.curves[2].geometry
        {
            start.x += 3.0;
        }
        assert!(d.write_sketch_geometry(&forged).is_err());
        let mut small = prepared.clone();
        if let ObjectPayload::Sketch(s) = &mut small.payload {
            for curve in &mut s.curves {
                if let SketchGeometry::Line { start, end } = &mut curve.geometry {
                    start.y *= 0.05;
                    end.y *= 0.05;
                }
            }
        }
        assert!(d.write_sketch_geometry(&small).is_err());
        assert_eq!(d.content_version().expect("version"), before);
        // Stale: the document moved on after preparation.
        let other = crate::replace_sketch_coordinates(
            &d,
            profile,
            &vertices_in(&d, [0.0, 0.0, 30.0, 20.0]),
        )
        .expect("prepared");
        d.write_sketch_geometry(&prepared).expect("written");
        assert!(d.write_sketch_geometry(&other).is_err());
        assert!(d.validate().expect("validates").is_ok());
    }

    /// Constraints written by the shipped preparation and writer: Horizontal on
    /// the first Line and a length on the second (both managed).
    fn constrain_plate(
        d: &mut Document,
        length: f64,
    ) -> (ObjectId, crate::PreparedSketchConstraints) {
        let lines = sketch_of(d).curves;
        let profile = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("Sketch")
            .id;
        let edits = crate::SketchConstraintEdits {
            remove: Vec::new(),
            add: vec![
                crate::AddSketchConstraint::Line(crate::AddLineConstraint::Line {
                    curve: lines[0].id,
                    kind: crate::LineConstraintKind::Horizontal,
                }),
                crate::AddSketchConstraint::Line(crate::AddLineConstraint::Line {
                    curve: lines[1].id,
                    kind: crate::LineConstraintKind::Distance(
                        crate::LineLengthMm::new(length).expect("a length"),
                    ),
                }),
            ],
        };
        let prepared = crate::prepare_sketch_constraints(d, profile, &edits).expect("prepared");
        (profile, prepared)
    }

    /// §29D: the saved Chamfer is read through the one reader on a constrained
    /// plate, with its corner stored and its distance's upper bound deferred: a
    /// stored dimension is no evidence of the solved plate in either direction.
    #[test]
    fn a_constrained_plate_is_read_with_stored_facts_and_a_deferred_bound() {
        let (_root, mut d, body) = plate(PLATE);
        let id = chamfer_at(&mut d, body, 1, 2.0);
        let free = target_of_saved(&d, id);
        assert!(!free.constrained);
        assert!(
            free.check_distance(100.0).is_err(),
            "the stored bound, free"
        );
        let (_, prepared) = constrain_plate(&mut d, 20.0);
        d.write_sketch_constraints(&prepared).expect("written");
        let saved = target_of_saved(&d, id);
        assert!(saved.constrained);
        assert_eq!(
            saved.corner, free.corner,
            "the stored corner is read as stored"
        );
        saved
            .check_distance(100.0)
            .expect("no stored bound is applied");
        assert!(saved.check_distance(0.0005).is_err(), "the minimum stays");
        assert!(saved.check_distance(f64::NAN).is_err(), "finiteness stays");
        // The distance edit prepares a distance only the solved plate can judge.
        let edit = prepare_chamfer_distance(&d, id, 100.0).expect("deferred");
        assert!(edit.saved().constrained);
        // The height edit reads the same plate; the coordinate editor refuses
        // it, naming the Chamfer and the constraint editor.
        crate::prepare_extrude_height(&d, target_of_base(&d), 9.0).expect("height");
        let lines = sketch_of(&d).curves;
        let vertices: Vec<crate::SketchVertex> = lines
            .iter()
            .map(|c| match c.geometry {
                SketchGeometry::Line { start, .. } => crate::SketchVertex {
                    curve_id: c.id,
                    start_mm: [start.x, start.y],
                },
                _ => unreachable!(),
            })
            .collect();
        let profile = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("Sketch")
            .id;
        let why = crate::replace_sketch_coordinates(&d, profile, &vertices)
            .expect_err("a constrained plate")
            .to_string();
        assert!(
            why.contains(&id.to_string()) && why.contains("constraint editor"),
            "{why}"
        );
        // Removing every user constraint leaves the closure links, and the
        // coordinate editor accepts the plate again.
        let user: Vec<_> = sketch_of(&d)
            .constraints
            .iter()
            .filter(|c| !matches!(c.rule, SketchConstraintRule::Coincident { .. }))
            .map(|c| c.id)
            .collect();
        assert!(!user.is_empty());
        let clear = crate::prepare_sketch_constraints(
            &d,
            profile,
            &crate::SketchConstraintEdits {
                remove: user,
                add: Vec::new(),
            },
        )
        .expect("prepared");
        d.write_sketch_constraints(&clear).expect("written");
        assert!(!target_of_saved(&d, id).constrained);
        crate::replace_sketch_coordinates(&d, profile, &vertices).expect("closure-only again");
    }

    /// §29D: the evaluator's predicate on the Lines the rebuild built, with the
    /// stored ones only as the starting guess: a solved plate larger or smaller
    /// than stored, a turned side, a slanted plate and the exact bound.
    #[test]
    fn the_evaluator_judges_a_constrained_plate_on_its_solved_lines() {
        let (_root, mut d, body) = plate(PLATE);
        let id = chamfer_at(&mut d, body, 1, 2.5);
        let (_, prepared) = constrain_plate(&mut d, 20.0);
        d.write_sketch_constraints(&prepared).expect("written");
        let objects = d.objects().expect("objects");
        let chamfer = match &objects
            .iter()
            .find(|o| o.id == id)
            .expect("chamfer")
            .payload
        {
            ObjectPayload::Chamfer(c) => c.clone(),
            _ => unreachable!(),
        };
        let stored = sketch_of(&d).curves;
        let drawn = |rect: [f64; 4]| -> Vec<SketchCurve> {
            let [x0, y0, w, dep] = rect;
            let p = [[x0, y0], [x0 + w, y0], [x0 + w, y0 + dep], [x0, y0 + dep]];
            // The plate is drawn like the saved one: same Lines, same order.
            let n = stored.len();
            let starts: Vec<[f64; 2]> = stored
                .iter()
                .map(|c| match c.geometry {
                    SketchGeometry::Line { start, .. } => {
                        let lx = stored
                            .iter()
                            .map(|c| match c.geometry {
                                SketchGeometry::Line { start, .. } => start.x,
                                _ => 0.0,
                            })
                            .fold(f64::MAX, f64::min);
                        let ly = stored
                            .iter()
                            .map(|c| match c.geometry {
                                SketchGeometry::Line { start, .. } => start.y,
                                _ => 0.0,
                            })
                            .fold(f64::MAX, f64::min);
                        [
                            if start.x == lx { p[0][0] } else { p[1][0] },
                            if start.y == ly { p[0][1] } else { p[2][1] },
                        ]
                    }
                    _ => unreachable!(),
                })
                .collect();
            (0..n)
                .map(|i| SketchCurve {
                    id: stored[i].id,
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: Point2::new(starts[i][0], starts[i][1]).expect("point"),
                        end: Point2::new(starts[(i + 1) % n][0], starts[(i + 1) % n][1])
                            .expect("point"),
                    },
                })
                .collect()
        };
        let ask = |lines: &[SketchCurve]| evaluable_chamfer(&objects, &chamfer, Some(lines));
        // Larger than stored, and moved: accepted, the corner followed.
        ask(&drawn([10.0, -5.0, 60.0, 30.0])).expect("a larger solved plate");
        // Smaller than the stored plate but with room: accepted.
        ask(&drawn([0.0, 0.0, 9.0, 4.0])).expect("a smaller solved plate with room");
        // The exact bound on the solved sides, from the evaluator's own number.
        let corner = ask(&drawn([0.0, 0.0, 30.0, 12.0])).expect("fits");
        let at = corner.max_distance_mm;
        assert_eq!(at, 12.0 - MIN_FLAT_MM);
        let mut edge = chamfer.clone();
        edge.distance_mm = at;
        evaluable_chamfer(&objects, &edge, Some(&drawn([0.0, 0.0, 30.0, 12.0]))).expect("at");
        edge.distance_mm = at.next_up();
        let why = evaluable_chamfer(&objects, &edge, Some(&drawn([0.0, 0.0, 30.0, 12.0])))
            .expect_err("the next float")
            .to_string();
        assert!(
            why.contains(&id.to_string()) && why.contains("does not fit the solved plate"),
            "{why}"
        );
        // Too short for the saved distance on the solved sides, though the
        // stored ones are long.
        let why = ask(&drawn([0.0, 0.0, 30.0, 2.0])).expect_err("a thin solved plate");
        assert!(
            why.to_string().contains("does not fit the solved plate"),
            "{why}"
        );
        // A turned side: the plate is a rectangle, but a Line runs the other way.
        let mut turned = drawn([0.0, 0.0, 30.0, 12.0]);
        turned.reverse();
        let why = ask(&turned)
            .expect_err("a different Line order")
            .to_string();
        assert!(why.contains("same four Lines"), "{why}");
        let mut flipped = drawn([30.0, 12.0, -30.0, -12.0]);
        for c in &mut flipped {
            c.construction = false;
        }
        let why = ask(&flipped)
            .expect_err("every Line the other way")
            .to_string();
        assert!(
            why.contains(&id.to_string())
                && why.contains("keeps its corner only while every Line keeps its side"),
            "{why}"
        );
        // Slanted: not an axis-aligned rectangle.
        let mut slanted = drawn([0.0, 0.0, 30.0, 12.0]);
        if let SketchGeometry::Line { start, .. } = &mut slanted[0].geometry {
            start.y += 0.5;
        }
        let why = ask(&slanted).expect_err("a slanted plate").to_string();
        assert!(why.contains(&id.to_string()), "{why}");
        assert!(evaluable_chamfer(&objects, &chamfer, None).is_err());
    }

    /// §29D: the writer re-derives the constraint edit with the Chamfer's
    /// context inside its transaction: a forged context and a stale prepared
    /// edit (the distance edited after preparation) are refused, nothing written.
    #[test]
    fn the_constraint_writer_rederives_under_a_chamfer_and_refuses_forgery() {
        let (_root, mut d, body) = plate(PLATE);
        let id = chamfer_at(&mut d, body, 1, 2.0);
        let (_, prepared) = constrain_plate(&mut d, 20.0);
        let before = d.content_version().expect("version");
        let mut forged = prepared.clone();
        if let Some(c) = forged.chamfer.as_mut() {
            c.distance_mm = 9.0;
        }
        assert!(
            d.write_sketch_constraints(&forged).is_err(),
            "a forged context"
        );
        let mut without = prepared.clone();
        without.chamfer = None;
        assert!(d.write_sketch_constraints(&without).is_err(), "no context");
        let mut moved = prepared.clone();
        if let Some(c) = moved.chamfer.as_mut() {
            let lines = &sketch_of(&d).curves;
            c.edge.joint = ProfileJoint::new(lines[0].id, lines[2].id).expect("joint");
        }
        assert!(
            d.write_sketch_constraints(&moved).is_err(),
            "another corner"
        );
        assert_eq!(
            d.content_version().expect("version"),
            before,
            "nothing written"
        );
        // Stale: the Chamfer's distance moves after preparation.
        let edit = prepare_chamfer_distance(&d, id, 3.0).expect("prepared");
        d.write_chamfer_distance(&edit).expect("written");
        assert!(
            d.write_sketch_constraints(&prepared).is_err(),
            "a stale edit"
        );
        // The honest, fresh one is written and carries the Chamfer unchanged.
        let (_, fresh) = constrain_plate(&mut d, 20.0);
        d.write_sketch_constraints(&fresh).expect("written");
        let saved = target_of_saved(&d, id);
        assert!(saved.constrained);
        assert_eq!(saved.distance_mm, 3.0);
        assert!(d.validate().expect("validates").is_ok());
    }
}
