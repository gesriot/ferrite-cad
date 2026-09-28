// SPDX-License-Identifier: MIT
//! Rounding one vertical edge of a saved rectangular plate, in a new copy
//! (§28A).
//!
//! The same history model a Cut uses (ADR 0004): the new Fillet names the
//! feature whose result it rounds, the Body names its tip, and ownership is
//! derived. The supported source is the exact plate the Cut editors read
//! through [`crate::cut_edit::saved_history`], with no Cut, and a profile that
//! is literally an axis-aligned rectangle.
//!
//! The edge is chosen by what it means — the base Extrude and the corner of
//! its profile, named by the unordered pair of the two Line UUIDs that meet
//! there — never by an index or a position. The radius is judged by one
//! measured, conservative policy ([`MIN_RADIUS_MM`], [`MAX_RADIUS_FRACTION`]),
//! applied by discovery, preparation, the writer's re-derivation and the
//! evaluator alike. Nothing is clamped.
use ferritecad_types::{CadError, ObjectId, ProfileJoint, Result, StableEntityId};

use crate::cut_boundary::CutBoundary;
use crate::cut_edit::NewObject;
use crate::{
    Body, CapSide, Dependency, DependencyRole, Document, EntityKind, Fillet, FilletEdge,
    ObjectPayload, ObjectRecord, SelectionRule, SemanticRole, Sketch, SketchGeometry, TopologyRef,
};

/// The smallest radius this build rounds an edge to, in millimetres.
///
/// Measured on Open CASCADE 8.0.1: at 1e-7 mm the builder returns a "valid"
/// solid that removes no measurable material, and at 1e-5 mm the removed
/// volume is off by 1.4e-3 relative; from 0.01 mm it agrees with the
/// analytic value to 5e-10. 0.01 mm is 10⁵ times the kernel's linear
/// tolerance.
pub const MIN_RADIUS_MM: f64 = 0.01;

/// The largest radius, as a fraction of the shorter Line meeting at the
/// corner.
///
/// Open CASCADE stops at the shorter side itself (measured: done at 0.996 of
/// it, not done at 1.0). Half of it keeps at least half of each adjacent side
/// flat, well away from the degenerate limit, so neither neighbouring corner
/// and no other face can be reached. Chosen conservatively; never widened to
/// make an example pass.
pub const MAX_RADIUS_FRACTION: f64 = 0.5;

fn unsupported(message: impl Into<String>) -> CadError {
    CadError::unsupported(message)
}

/// One vertical edge of the plate a Fillet could round.
///
/// `joint` is the identity. The rest are labels a person can read — where the
/// corner is, how long its two sides are, how large a radius fits — and are
/// recomputed from the saved profile every time; none of them names the edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilletCorner {
    /// The feature that swept the edge: the plate's base Extrude.
    pub feature: ObjectId,
    pub joint: ProfileJoint,
    /// The corner of the profile the edge was swept from, in sketch mm.
    pub corner_mm: [f64; 2],
    /// The lengths of the two Lines meeting there, in the joint's canonical
    /// order.
    pub adjacent_lengths_mm: [f64; 2],
    /// The largest radius this build accepts here.
    pub max_radius_mm: f64,
}

/// The part of the radius policy that holds at every corner: a finite radius
/// of at least [`MIN_RADIUS_MM`]. The bound that depends on the corner is
/// [`FilletCorner::check_radius`].
pub fn check_radius_value(radius_mm: f64) -> Result<()> {
    if !radius_mm.is_finite() {
        return Err(CadError::input(format!(
            "a fillet radius must be a finite number of millimetres, found {radius_mm}"
        )));
    }
    if radius_mm < MIN_RADIUS_MM {
        return Err(CadError::input(format!(
            "a fillet radius must be at least {MIN_RADIUS_MM} mm, found {radius_mm} mm"
        )));
    }
    Ok(())
}

impl FilletCorner {
    /// The radius policy, with the numbers in the refusal.
    pub fn check_radius(&self, radius_mm: f64) -> Result<()> {
        check_radius_value(radius_mm)?;
        if radius_mm > self.max_radius_mm {
            return Err(CadError::input(format!(
                "a fillet of {radius_mm} mm at corner {} is too large: this build rounds up to \
                 {MAX_RADIUS_FRACTION} × the shorter adjacent side ({} mm), which is {} mm; \
                 nothing is clamped",
                self.joint,
                self.adjacent_lengths_mm[0].min(self.adjacent_lengths_mm[1]),
                self.max_radius_mm
            )));
        }
        Ok(())
    }
}

/// The four corners of a profile that is an axis-aligned rectangle of four
/// Lines, in stored segment order: corner `j` is where Line `j - 1` ends and
/// Line `j` starts.
///
/// Refuses anything else with its reason. Shared by discovery, preparation and
/// the evaluator, so all three agree about which edges exist.
pub fn rectangle_corners(feature: ObjectId, sketch: &Sketch) -> Result<Vec<FilletCorner>> {
    if !sketch.constraints.is_empty() {
        return Err(unsupported(
            "this slice rounds an edge of an unconstrained profile; the profile carries \
             constraints",
        ));
    }
    corners_of_lines(feature, &sketch.curves)
}

/// [`rectangle_corners`] of any set of Lines, whatever produced them: the
/// stored ones, or the ones a rebuild solved and built the plate from
/// (§28E). The geometry alone; whether the profile may carry constraints is
/// the caller's structural question.
pub(crate) fn corners_of_lines(
    feature: ObjectId,
    curves: &[crate::SketchCurve],
) -> Result<Vec<FilletCorner>> {
    let lines: Vec<(StableEntityId, [f64; 2], [f64; 2])> = curves
        .iter()
        .map(|c| match c.geometry {
            SketchGeometry::Line { start, end } if !c.construction => {
                Ok((c.id, [start.x, start.y], [end.x, end.y]))
            }
            _ => Err(unsupported(
                "this slice rounds an edge of a profile of four Lines, and this profile holds \
                 something else",
            )),
        })
        .collect::<Result<_>>()?;
    let boundary = CutBoundary::read(curves, 1.0)?;
    if lines.len() != 4 || boundary.rectangle_mm().is_none() {
        return Err(unsupported(
            "this slice rounds a vertical edge of an axis-aligned rectangular plate, and this \
             profile is not an axis-aligned rectangle",
        ));
    }
    let length =
        |(_, a, b): &(StableEntityId, [f64; 2], [f64; 2])| (b[0] - a[0]).hypot(b[1] - a[1]);
    (0..lines.len())
        .map(|j| {
            let before = &lines[(j + lines.len() - 1) % lines.len()];
            let after = &lines[j];
            let joint = ProfileJoint::new(before.0, after.0)?;
            let [first, _] = joint.segments();
            let lengths = if first == before.0 {
                [length(before), length(after)]
            } else {
                [length(after), length(before)]
            };
            Ok(FilletCorner {
                feature,
                joint,
                corner_mm: after.1,
                adjacent_lengths_mm: lengths,
                max_radius_mm: MAX_RADIUS_FRACTION * lengths[0].min(lengths[1]),
            })
        })
        .collect()
}

/// §28D/§28E: each Line of `candidate` runs along the same axis in the same
/// direction as in `saved`, both given as starts in the same stored order.
///
/// Both profiles have already passed the shared rectangle reader. The
/// dominant component of a Line identifies its side; a component within the
/// reader's tolerance on the other axis does not invent a diagonal direction.
/// The coordinates are read as they are: nothing is snapped.
pub(crate) fn keeps_every_side(
    ids: &[StableEntityId],
    saved: &[[f64; 2]],
    candidate: &[[f64; 2]],
) -> Result<()> {
    let sign = |v: f64| (v > 0.) as i8 - (v < 0.) as i8;
    let side = |dx: f64, dy: f64| {
        if dx.abs() >= dy.abs() {
            (sign(dx), 0)
        } else {
            (0, sign(dy))
        }
    };
    let n = ids.len();
    if saved.len() != n || candidate.len() != n {
        return Err(CadError::input(
            "the rounded plate keeps its four Lines in their stored order",
        ));
    }
    for i in 0..n {
        let (a, b) = (saved[i], saved[(i + 1) % n]);
        let (p, q) = (candidate[i], candidate[(i + 1) % n]);
        let was = side(b[0] - a[0], b[1] - a[1]);
        let now = side(q[0] - p[0], q[1] - p[1]);
        if was != now {
            return Err(CadError::input(format!(
                "Line {} of the rounded plate must keep its side: it ran {} and would run {}; \
                 the Fillet's corner is where two particular sides meet",
                ids[i],
                direction(was),
                direction(now)
            )));
        }
    }
    Ok(())
}

fn direction((x, y): (i8, i8)) -> &'static str {
    match (x, y) {
        (1, 0) => "+X",
        (-1, 0) => "-X",
        (0, 1) => "+Y",
        (0, -1) => "-Y",
        _ => "nowhere",
    }
}

/// The corner a stated edge means, or why it means none.
pub fn corner_for(corners: &[FilletCorner], edge: FilletEdge) -> Result<FilletCorner> {
    let Some(first) = corners.first() else {
        return Err(unsupported("this profile has no corner to round"));
    };
    if edge.feature != first.feature {
        return Err(CadError::input(format!(
            "the edge names feature {}, and this slice rounds an edge of the base Extrude {}",
            edge.feature, first.feature
        )));
    }
    corners
        .iter()
        .find(|c| c.joint == edge.joint)
        .copied()
        .ok_or_else(|| {
            CadError::input(format!(
                "{} is not a corner of this profile: the two Lines must be adjacent Lines of \
                 the base Extrude's profile",
                edge.joint
            ))
        })
}

/// A saved Body a Fillet can be added to, exactly as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedFilletTarget {
    pub body: ObjectId,
    /// The base Extrude: both the feature consumed and the one whose edge is
    /// rounded.
    pub base_feature: ObjectId,
    pub profile: ObjectId,
    pub profile_segments: Vec<StableEntityId>,
    pub height_mm: f64,
    /// The four candidates, in stored segment order.
    pub corners: Vec<FilletCorner>,
}

/// A Body row of discovery: a target, or the reason it is not one.
#[derive(Debug, Clone, PartialEq)]
pub struct FilletChoice {
    pub body: ObjectId,
    pub name: Option<String>,
    pub target: Option<SavedFilletTarget>,
    pub refusal: Option<String>,
}

impl FilletChoice {
    /// The same check preparation applies, with no SQLite or kernel work.
    pub fn validate(&self, fillet: &EdgeFillet) -> Result<FilletCorner> {
        let target = self
            .target
            .as_ref()
            .ok_or_else(|| unsupported(self.refusal.clone().unwrap_or_default()))?;
        let corner = corner_for(&target.corners, fillet.edge)?;
        corner.check_radius(fillet.radius_mm)?;
        Ok(corner)
    }
}

/// What one fillet asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeFillet {
    pub edge: FilletEdge,
    pub radius_mm: f64,
}

/// Why a document that already holds a Fillet is not a target for anything
/// this build edits. One sentence, used by every editor that reads the plate.
pub(crate) fn refuse_filleted(objects: &[ObjectRecord]) -> Result<()> {
    if let Some(fillet) = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
    {
        return Err(unsupported(format!(
            "this Body ends in Fillet {} (§28A); only its radius (edit-fillet-radius, §28B), \
             the rounded plate's height (edit-extrude, §28C), its base Sketch's coordinates \
             (edit-sketch-copy, §28D) and that Sketch's Line constraints \
             (edit-sketch-constraints-copy, §28E) can be edited. Editing the rest of a \
             filleted part, and adding a second Fillet or a Cut after one, are not supported \
             yet",
            fillet.id
        )));
    }
    Ok(())
}

/// Why an editor refuses a filleted part whose Fillet is outside the frame
/// the radius, height and Sketch edits read: [`refuse_filleted`]'s sentence
/// naming the Fillet, and the frame's own reason.
pub(crate) fn filleted_outside_frame(objects: &[ObjectRecord], reason: &CadError) -> CadError {
    match refuse_filleted(objects) {
        Err(e) => unsupported(format!(
            "{e}. This Fillet is outside the frame those edits read: {reason}"
        )),
        Ok(()) => unsupported(reason.to_string()),
    }
}

fn saved_target(
    document: &Document,
    objects: &[ObjectRecord],
    body: ObjectId,
) -> Result<SavedFilletTarget> {
    refuse_filleted(objects)?;
    let history = crate::cut_edit::saved_history(document, objects)?;
    let target = history.target_for_fillet(body)?;
    let profile = objects
        .iter()
        .find(|o| o.id == target.profile)
        .ok_or_else(|| unsupported("the part's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(unsupported("the part's profile is not a Sketch"));
    };
    let corners = rectangle_corners(target.base_feature, sketch)?;
    Ok(SavedFilletTarget {
        body,
        base_feature: target.base_feature,
        profile: target.profile,
        profile_segments: target.profile_segments,
        height_mm: target.height_mm,
        corners,
    })
}

/// One row per Body, each a target or its reason, from one snapshot.
pub fn fillet_choices(document: &Document, objects: &[ObjectRecord]) -> Vec<FilletChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .map(|o| {
            let target = saved_target(document, objects, o.id).map_err(|e| e.to_string());
            FilletChoice {
                body: o.id,
                name: o.name.clone(),
                refusal: target.as_ref().err().cloned(),
                target: target.ok(),
            }
        })
        .collect()
}

/// Everything one accepted fillet adds to a copy, prepared before anything is
/// written. Identifiers are minted once, after every check has passed.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEdgeFillet {
    pub(crate) body: ObjectRecord,
    pub(crate) feature: NewObject,
    pub(crate) added_dependencies: Vec<Dependency>,
    pub(crate) removed_dependencies: Vec<Dependency>,
    pub(crate) references: Vec<TopologyRef>,
    pub(crate) previous: ObjectId,
    pub(crate) corner: FilletCorner,
}

impl PreparedEdgeFillet {
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
    /// The corner being rounded, with its labels.
    pub fn corner(&self) -> FilletCorner {
        self.corner
    }
    pub fn radius_mm(&self) -> f64 {
        match &self.feature.payload {
            ObjectPayload::Fillet(f) => f.radius_mm,
            _ => f64::NAN,
        }
    }
}

/// What the finished part is called after one fillet: the new face under the
/// edge it replaced, and every face the plate had, as the fillet leaves it,
/// qualified by the plate's own Extrude.
pub(crate) fn fillet_references(
    feature: ObjectId,
    base: ObjectId,
    joint: ProfileJoint,
    profile_segments: &[StableEntityId],
) -> Vec<TopologyRef> {
    let mut references = vec![TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Face,
        output_role: SemanticRole::EdgeFilletFace {
            edge_feature: base,
            joint,
        },
        selection: SelectionRule::Exact,
        fallback_signature: None,
    }];
    for side in [CapSide::Start, CapSide::End] {
        references.push(TopologyRef {
            id: StableEntityId::new(),
            owner: feature,
            producer_feature: feature,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::OriginCap {
                origin_feature: base,
                side,
            },
            selection: SelectionRule::Exact,
            fallback_signature: None,
        });
    }
    for segment in profile_segments {
        references.push(TopologyRef {
            id: StableEntityId::new(),
            owner: feature,
            producer_feature: feature,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::OriginSide {
                origin_feature: base,
                profile_segment: *segment,
            },
            selection: SelectionRule::AllDerivedFrom { ancestor: *segment },
            fallback_signature: None,
        });
    }
    references
}

/// Prepares one fillet against the saved document.
pub fn prepare_edge_fillet(
    document: &Document,
    body: ObjectId,
    fillet: &EdgeFillet,
) -> Result<PreparedEdgeFillet> {
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
    let corner = corner_for(&target.corners, fillet.edge)?;
    corner.check_radius(fillet.radius_mm)?;

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
        name: "Fillet".to_owned(),
        payload: ObjectPayload::Fillet(Fillet {
            previous: target.base_feature,
            edge: FilletEdge {
                feature: target.base_feature,
                joint: corner.joint,
            },
            radius_mm: fillet.radius_mm,
        }),
    };
    let mut moved = record.clone();
    moved.payload = ObjectPayload::Body(Body {
        tip_feature: Some(feature_id),
    });
    let added_dependencies = vec![
        Dependency {
            dependent: feature_id,
            dependency: target.base_feature,
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
        dependency: target.base_feature,
        role: DependencyRole::BodyTip,
    }];
    let references = fillet_references(
        feature_id,
        target.base_feature,
        corner.joint,
        &target.profile_segments,
    );
    Ok(PreparedEdgeFillet {
        body: moved,
        feature,
        added_dependencies,
        removed_dependencies,
        references,
        previous: target.base_feature,
        corner,
    })
}

/// Re-derives a prepared fillet from the document it claims to be against:
/// the writer's guard. Everything but the minted identities is derived again
/// from the numbers the prepared payload carries, and the whole is compared.
pub(crate) fn rederive(document: &Document, prepared: &PreparedEdgeFillet) -> Result<()> {
    let ObjectPayload::Fillet(fillet) = &prepared.feature.payload else {
        return Err(CadError::input("a prepared fillet carries a Fillet"));
    };
    let stated = EdgeFillet {
        edge: fillet.edge,
        radius_mm: fillet.radius_mm,
    };
    let mut checked = prepare_edge_fillet(document, prepared.body.id, &stated)?;
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
            "the prepared fillet names a different number of faces than the document would",
        ));
    }
    for (mine, theirs) in checked.references.iter_mut().zip(&prepared.references) {
        mine.id = theirs.id;
        mine.owner = swap(mine.owner);
        mine.producer_feature = swap(mine.producer_feature);
    }
    if checked != *prepared {
        return Err(CadError::input(
            "the prepared fillet does not describe the document it is being written to",
        ));
    }
    Ok(())
}

/// The Fillet's class and radius policy, asked by the evaluator at every
/// cold and cached rebuild.
///
/// Two separate facts (§28E). The **structure** is read from the saved
/// objects: a forward Blind NewBody Extrude, whose stored profile is an
/// axis-aligned rectangle of four Lines with the saved joint at one of its
/// corners, and which carries no constraints or only the constraint editor's
/// managed Line family. The **geometry** is `built`: the Lines the rebuild
/// actually built the predecessor from — the stored ones for an
/// unconstrained profile, the solved ones otherwise. On those, the same Lines
/// in stored order must still be an axis-aligned rectangle, keep every side,
/// have the saved joint as a corner, and leave room for the saved radius. The
/// stored numbers of a constrained profile are its solver's starting guess and
/// prove nothing about the part.
pub fn evaluable_fillet(
    objects: &[ObjectRecord],
    fillet: &Fillet,
    built: Option<&[crate::SketchCurve]>,
) -> Result<FilletCorner> {
    if fillet.edge.feature != fillet.previous {
        return Err(unsupported(
            "this build rounds an edge of the feature a Fillet consumes, and this Fillet names \
             an edge of another feature",
        ));
    }
    let base = objects
        .iter()
        .find(|o| o.id == fillet.previous)
        .ok_or_else(|| CadError::input("the Fillet's predecessor is missing"))?;
    let ObjectPayload::Extrude(extrude) = &base.payload else {
        return Err(unsupported(
            "this build rounds an edge of an extruded plate, and the Fillet's predecessor is not \
             an Extrude",
        ));
    };
    if extrude.operation != crate::SolidOperation::NewBody
        || extrude.previous.is_some()
        || extrude.reversed
        || !matches!(extrude.end_condition, crate::EndCondition::Blind { .. })
    {
        return Err(unsupported(
            "this build rounds an edge of a forward Blind NewBody extrusion",
        ));
    }
    let profile = objects
        .iter()
        .find(|o| o.id == extrude.profile)
        .ok_or_else(|| CadError::input("the base Extrude's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(unsupported("the base Extrude's profile is not a Sketch"));
    };
    if !sketch.constraints.is_empty() {
        crate::sketch_constraints::managed_lines(sketch).map_err(|e| {
            unsupported(format!(
                "the rounded plate's constraints are outside what this build edits: {e}"
            ))
        })?;
    }
    let stored = corners_of_lines(base.id, &sketch.curves)?;
    corner_for(&stored, fillet.edge)?;
    let Some(built) = built else {
        return Err(CadError::input(format!(
            "the plate the Fillet rounds was built from no profile {}",
            extrude.profile
        )));
    };
    if sketch.constraints.is_empty() {
        // Built from the stored Lines themselves: exactly §28A's check.
        let corners = corners_of_lines(base.id, built)?;
        let corner = corner_for(&corners, fillet.edge)?;
        corner.check_radius(fillet.radius_mm)?;
        return Ok(corner);
    }
    solved_corner(sketch, built, base.id, fillet).map_err(|e| {
        let message = format!(
            "as its constraints solve it, the rounded plate {}",
            bare(&e)
        );
        match e.kind() {
            ferritecad_types::ErrorKind::Input => CadError::input(message),
            _ => unsupported(message),
        }
    })
}

/// A refusal's sentence without the kind its display starts with, for a
/// refusal that is quoted inside another one.
fn bare(e: &CadError) -> String {
    let text = e.to_string();
    ["invalid input: ", "unsupported: "]
        .iter()
        .find_map(|p| text.strip_prefix(p))
        .map_or(text.clone(), str::to_owned)
}

/// The Fillet's policy on the solved Lines of a constrained plate.
fn solved_corner(
    stored: &Sketch,
    built: &[crate::SketchCurve],
    feature: ObjectId,
    fillet: &Fillet,
) -> Result<FilletCorner> {
    let ids: Vec<_> = stored.curves.iter().map(|c| c.id).collect();
    if built.iter().map(|c| c.id).collect::<Vec<_>>() != ids {
        return Err(unsupported(
            "is not drawn by the same four Lines in their stored order",
        ));
    }
    let corners = corners_of_lines(feature, built).map_err(|e| {
        unsupported(format!(
            "is no longer an axis-aligned rectangle ({})",
            bare(&e)
        ))
    })?;
    let start = |c: &crate::SketchCurve| match c.geometry {
        SketchGeometry::Line { start, .. } => [start.x, start.y],
        _ => [f64::NAN; 2],
    };
    keeps_every_side(
        &ids,
        &stored.curves.iter().map(start).collect::<Vec<_>>(),
        &built.iter().map(start).collect::<Vec<_>>(),
    )
    .map_err(|e| CadError::input(format!("moves a Line off its side: {}", bare(&e))))?;
    let corner = corner_for(&corners, fillet.edge)
        .map_err(|e| CadError::input(format!("no longer has the rounded corner: {}", bare(&e))))?;
    corner.check_radius(fillet.radius_mm).map_err(|e| {
        CadError::input(format!(
            "has sides of {} and {} mm at the rounded corner, too short for the saved radius: {}",
            corner.adjacent_lengths_mm[0],
            corner.adjacent_lengths_mm[1],
            bare(&e)
        ))
    })?;
    Ok(corner)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::{
        DatumPlane, EndCondition, Expression, Extrude, Point2, SketchCurve, SolidOperation,
    };
    use ferritecad_types::{ErrorKind, Transform};

    const PLATE: [[f64; 2]; 4] = [[-4.5, 3.25], [33., 3.25], [33., 15.5], [-4.5, 15.5]];

    fn sketch(corners: &[[f64; 2]]) -> Sketch {
        let n = corners.len();
        Sketch {
            plane: ObjectId::new(),
            curves: (0..n)
                .map(|i| SketchCurve {
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

    /// One plate written straight into a document, exactly as a creator would.
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

    /// The stored payload round-trips, requires its capability, and refuses
    /// a radius no fillet has and a field this reader would drop.
    #[test]
    fn the_fillet_payload_round_trips_and_refuses_what_it_cannot_mean() {
        let [a, b] = [(); 2].map(|_| StableEntityId::new());
        let fillet = Fillet {
            previous: ObjectId::new(),
            edge: FilletEdge {
                feature: ObjectId::new(),
                joint: ProfileJoint::new(b, a).expect("joint"),
            },
            radius_mm: 2.375,
        };
        let payload = ObjectPayload::Fillet(fillet.clone());
        let bytes = payload.to_storage_bytes().expect("encode");
        assert_eq!(
            ObjectPayload::from_storage_bytes(&bytes).expect("decode"),
            payload
        );
        let capabilities = payload.required_capabilities();
        for needed in [
            crate::CORE_CAPABILITY,
            crate::FEATURE_PREDECESSOR_CAPABILITY,
            crate::FEATURE_FILLET_CAPABILITY,
        ] {
            assert!(capabilities.iter().any(|c| c == needed), "{needed}");
        }
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut bad = fillet.clone();
            bad.radius_mm = radius;
            assert!(
                ObjectPayload::Fillet(bad).to_storage_bytes().is_err(),
                "{radius}"
            );
        }
        // A stored joint must already be canonical: the same bytes with the
        // two Line UUIDs swapped are refused, not quietly re-sorted.
        let [first, second] = fillet.edge.joint.segments().map(|id| id.to_bytes());
        let at = |needle: &[u8]| {
            bytes
                .windows(needle.len())
                .position(|w| w == needle)
                .expect("the UUID is stored")
        };
        let (i, j) = (at(&first), at(&second));
        let mut swapped = bytes.clone();
        swapped[i..i + 16].copy_from_slice(&second);
        swapped[j..j + 16].copy_from_slice(&first);
        assert!(ObjectPayload::from_storage_bytes(&swapped).is_err());
    }

    /// Either winding and any starting segment give the same four corners,
    /// each named by the two Lines that really meet there.
    #[test]
    fn corners_are_the_four_joints_whatever_the_winding_or_starting_segment() {
        let feature = ObjectId::new();
        let mut wanted: Vec<[f64; 2]> = PLATE.to_vec();
        wanted.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        let reversed: Vec<[f64; 2]> = PLATE.iter().rev().copied().collect();
        for start in 0..4 {
            for winding in [PLATE.to_vec(), reversed.clone()] {
                let corners: Vec<[f64; 2]> = (0..4).map(|i| winding[(start + i) % 4]).collect();
                let drawn = sketch(&corners);
                let found = rectangle_corners(feature, &drawn).expect("a rectangle");
                assert_eq!(found.len(), 4);
                let mut at: Vec<[f64; 2]> = found.iter().map(|c| c.corner_mm).collect();
                at.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
                assert_eq!(at, wanted);
                for corner in &found {
                    assert_eq!(corner.feature, feature);
                    let [a, b] = corner.joint.segments();
                    assert!(a < b, "canonical");
                    // Both Lines touch the corner, and each length is its own.
                    for (id, length) in [a, b].into_iter().zip(corner.adjacent_lengths_mm) {
                        let curve = drawn.curves.iter().find(|c| c.id == id).expect("a Line");
                        let SketchGeometry::Line { start, end } = curve.geometry else {
                            panic!("a Line");
                        };
                        let ends = [[start.x, start.y], [end.x, end.y]];
                        assert!(ends.contains(&corner.corner_mm));
                        assert_eq!(length, (end.x - start.x).hypot(end.y - start.y));
                    }
                    let mut lengths = corner.adjacent_lengths_mm;
                    lengths.sort_by(f64::total_cmp);
                    assert_eq!(lengths, [12.25, 37.5]);
                    assert_eq!(corner.max_radius_mm, 6.125);
                }
            }
        }
    }

    #[test]
    fn anything_but_an_unconstrained_axis_aligned_rectangle_of_lines_is_refused() {
        let feature = ObjectId::new();
        let trapezoid = sketch(&[[0., 0.], [10., 0.], [8., 5.], [2., 5.]]);
        let rotated = sketch(&[[0., 0.], [3., 4.], [-1., 7.], [-4., 3.]]);
        let pentagon = sketch(&[[0., 0.], [10., 0.], [10., 5.], [5., 8.], [0., 5.]]);
        for (why, drawn) in [
            ("a trapezoid", trapezoid),
            ("a rotated rectangle", rotated),
            ("a pentagon", pentagon),
        ] {
            assert_eq!(
                rectangle_corners(feature, &drawn).expect_err(why).kind(),
                ErrorKind::Unsupported,
                "{why}"
            );
        }
        let mut construction = sketch(&PLATE);
        construction.curves[0].construction = true;
        assert!(rectangle_corners(feature, &construction).is_err());
        let mut circle = sketch(&PLATE);
        circle.curves = vec![SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Circle {
                center: Point2::new(0., 0.).expect("point"),
                radius: 5.,
            },
        }];
        assert!(rectangle_corners(feature, &circle).is_err());
        let mut constrained = sketch(&PLATE);
        let line = constrained.curves[0].id;
        constrained.constraints = vec![crate::SketchConstraint {
            id: StableEntityId::new(),
            rule: crate::SketchConstraintRule::Horizontal {
                a: crate::SketchPointRef::new(line, crate::SketchPointSelector::Start),
                b: crate::SketchPointRef::new(line, crate::SketchPointSelector::End),
            },
        }];
        assert!(
            rectangle_corners(feature, &constrained)
                .expect_err("constrained")
                .to_string()
                .contains("constraints")
        );
    }

    /// The radius class, at and just past both of its ends, with nothing
    /// clamped; and an edge is named by its producer and its joint only.
    #[test]
    fn the_radius_policy_and_the_edge_meaning_refuse_with_their_reasons() {
        let feature = ObjectId::new();
        let drawn = sketch(&PLATE);
        let corners = rectangle_corners(feature, &drawn).expect("a rectangle");
        let corner = corners[1];
        for ok in [MIN_RADIUS_MM, 1.0, 6.125] {
            corner.check_radius(ok).expect("inside the class");
        }
        for refused in [
            0.0,
            -1.0,
            MIN_RADIUS_MM * 0.999,
            6.125 * (1. + 1e-12),
            12.25,
            f64::NAN,
            f64::INFINITY,
        ] {
            let error = corner.check_radius(refused).expect_err("outside");
            assert_eq!(error.kind(), ErrorKind::Input, "{refused}");
        }
        assert!(
            corner
                .check_radius(7.0)
                .expect_err("too large")
                .to_string()
                .contains("nothing is clamped")
        );

        let [a, b] = corner.joint.segments();
        let either = |one, other| FilletEdge {
            feature,
            joint: ProfileJoint::new(one, other).expect("joint"),
        };
        assert_eq!(corner_for(&corners, either(a, b)).expect("same"), corner);
        assert_eq!(corner_for(&corners, either(b, a)).expect("same"), corner);
        // Another feature, two Lines that do not meet, and a Line this profile
        // does not have.
        let foreign = FilletEdge {
            feature: ObjectId::new(),
            joint: corner.joint,
        };
        assert!(corner_for(&corners, foreign).is_err());
        let ids: Vec<_> = drawn.curves.iter().map(|c| c.id).collect();
        assert!(corner_for(&corners, either(ids[0], ids[2])).is_err());
        assert!(corner_for(&corners, either(ids[0], StableEntityId::new())).is_err());
    }

    /// Preparation, the writer and the validator: one new Fillet, the Body's
    /// tip moved to it, the predecessor recorded, seven new names, and nothing
    /// else; a forged preparation refused; and a second fillet refused.
    #[test]
    fn one_fillet_is_written_as_the_new_tip_and_a_forged_one_is_refused() {
        let (_root, mut d, body) = plate(PLATE);
        let choices = fillet_choices(&d, &d.objects().expect("objects"));
        assert_eq!(choices.len(), 1);
        let target = choices[0].target.clone().expect("a target");
        let corner = target.corners[2];
        let fillet = EdgeFillet {
            edge: FilletEdge {
                feature: target.base_feature,
                joint: corner.joint,
            },
            radius_mm: 2.375,
        };
        assert_eq!(choices[0].validate(&fillet).expect("valid"), corner);
        let honest = prepare_edge_fillet(&d, body, &fillet).expect("prepared");
        assert_eq!(honest.references().len(), 7);
        assert_eq!(honest.previous(), target.base_feature);

        // Forged preparations: another radius, another corner, a tip left in
        // place, a reference claimed for the base, and a reference dropped.
        let mut forged = Vec::new();
        let mut p = honest.clone();
        if let ObjectPayload::Fillet(f) = &mut p.feature.payload {
            f.radius_mm = 6.2;
        }
        forged.push(p);
        let mut p = honest.clone();
        if let ObjectPayload::Fillet(f) = &mut p.feature.payload {
            f.edge.joint = target.corners[0].joint;
        }
        forged.push(p);
        let mut p = honest.clone();
        if let ObjectPayload::Body(b) = &mut p.body.payload {
            b.tip_feature = Some(target.base_feature);
        }
        forged.push(p);
        let mut p = honest.clone();
        p.references[0].producer_feature = target.base_feature;
        forged.push(p);
        let mut p = honest.clone();
        p.references.pop();
        forged.push(p);
        for p in &forged {
            assert!(d.write_edge_fillet(p).is_err());
        }
        assert_eq!(d.objects().expect("objects").len(), 4, "a refusal wrote");

        d.write_edge_fillet(&honest).expect("the honest plan");
        let objects = d.objects().expect("objects");
        assert_eq!(objects.len(), 5);
        let saved = objects
            .iter()
            .find(|o| o.id == honest.feature().id)
            .expect("the Fillet");
        assert_eq!(saved.name.as_deref(), Some("Fillet"));
        let ObjectPayload::Fillet(stored) = &saved.payload else {
            panic!("a Fillet");
        };
        assert_eq!(stored.previous, target.base_feature);
        assert_eq!(stored.edge, fillet.edge);
        assert_eq!(stored.radius_mm, 2.375);
        let tip = objects.iter().find(|o| o.id == body).expect("body");
        assert!(matches!(
            tip.payload,
            ObjectPayload::Body(Body { tip_feature: Some(t) }) if t == saved.id
        ));
        let dependencies = d.dependencies().expect("deps");
        assert!(dependencies.contains(&Dependency {
            dependent: saved.id,
            dependency: target.base_feature,
            role: DependencyRole::Predecessor,
        }));
        assert!(dependencies.contains(&Dependency {
            dependent: body,
            dependency: saved.id,
            role: DependencyRole::BodyTip,
        }));
        assert!(!dependencies.contains(&Dependency {
            dependent: body,
            dependency: target.base_feature,
            role: DependencyRole::BodyTip,
        }));
        let refs = d.topology_refs().expect("refs");
        assert_eq!(refs.len(), 7);
        assert!(
            refs.iter()
                .all(|r| r.owner == saved.id && r.producer_feature == saved.id)
        );
        assert!(d.validate().expect("validate").is_ok());

        // A second fillet, and every editor of the plate but its height
        // (§28C), refuse by name.
        let again = fillet_choices(&d, &d.objects().expect("objects"));
        assert!(again[0].target.is_none());
        assert!(
            again[0]
                .refusal
                .as_deref()
                .expect("a reason")
                .contains(&saved.id.to_string())
        );
        assert!(prepare_edge_fillet(&d, body, &fillet).is_err());
        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        assert_eq!(reading.unavailable_reason(), None, "the plate's height");
        let base = reading.features.iter().find(|f| f.fillet.is_some());
        assert!(base.is_some_and(
            |f| f.refusal.is_none() && f.fillet.as_ref().is_some_and(|r| r.feature == saved.id)
        ));
        assert!(reading.cut_bodies.iter().all(|c| c.refusal.is_some()));
        // Its base Sketch is editable (§28D), with the Fillet as context, and
        // so are its Line constraints (§28E).
        assert!(reading.sketches.iter().all(|s| {
            s.refusal.is_none() && s.fillet.as_ref().is_some_and(|r| r.feature == saved.id)
        }));
        assert!(reading.constraint_sketches.iter().all(|s| {
            s.refusal.is_none() && s.fillet.as_ref().is_some_and(|r| r.feature == saved.id)
        }));
    }

    /// The validator holds a Fillet to the same history rules an Extrude is.
    #[test]
    fn the_validator_refuses_a_fillet_without_its_predecessor_edge_or_target() {
        let (_root, mut d, body) = plate(PLATE);
        let target = fillet_choices(&d, &d.objects().expect("objects"))[0]
            .target
            .clone()
            .expect("a target");
        let fillet = EdgeFillet {
            edge: FilletEdge {
                feature: target.base_feature,
                joint: target.corners[0].joint,
            },
            radius_mm: 1.0,
        };
        let prepared = prepare_edge_fillet(&d, body, &fillet).expect("prepared");
        d.write_edge_fillet(&prepared).expect("written");
        let id = prepared.feature().id;
        let codes = |d: &Document| -> Vec<&'static str> {
            d.validate()
                .expect("validate")
                .errors()
                .map(|e| e.code)
                .collect()
        };
        assert!(codes(&d).is_empty());
        d.write(|w| {
            w.remove_dependency(Dependency {
                dependent: id,
                dependency: target.base_feature,
                role: DependencyRole::Predecessor,
            })
        })
        .expect("drop the edge");
        assert!(codes(&d).contains(&"reference.missing-edge"));
        let mut edge_elsewhere = match &prepared.feature().payload {
            ObjectPayload::Fillet(f) => f.clone(),
            _ => panic!("a Fillet"),
        };
        edge_elsewhere.edge.feature = target.profile;
        edge_elsewhere.previous = id;
        d.write(|w| {
            w.put_object(
                id,
                None,
                4,
                Some("Fillet"),
                &ObjectPayload::Fillet(edge_elsewhere),
            )
            .map(|_| ())
        })
        .expect("forge");
        let found = codes(&d);
        assert!(found.contains(&"feature.self-predecessor"), "{found:?}");
        assert!(found.contains(&"reference.missing-target"), "{found:?}");
    }
}
