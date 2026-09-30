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
//! §28F: the plate's Sketch may carry the constraint editor's managed Line
//! family. Its stored Lines are then the solver's starting guess: they name
//! the candidate joints, and the part itself is judged by
//! [`evaluable_fillet`] on the solved plate when the copy is rebuilt.
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

/// §28G: whether two Fillets at `first` and `second` of one plate leave the
/// Line they share a flat of at least [`MIN_RADIUS_MM`].
///
/// Measured on Open CASCADE 8.0.1: two arcs that meet (`r1 + r2` equal to the
/// shared side) are not built at all, and any flat from 1e-7 mm up builds a
/// valid solid of the analytic volume. The minimum is the smallest feature
/// this build rounds: a strip narrower than the smallest radius counts as the
/// arcs touching. Opposite corners share no Line and answer `Ok`. The same
/// corner twice is refused here too. Nothing is clamped.
///
/// The rule is stated as a bound on the second radius, [`pair_bound`], the
/// one expression discovery reports: `r2 ≤ L − r1 − MIN_RADIUS_MM`, so the
/// largest radius discovery offers is one this check accepts.
pub fn check_pair(
    first: &FilletCorner,
    first_radius_mm: f64,
    second: &FilletCorner,
    second_radius_mm: f64,
) -> Result<()> {
    if first.joint == second.joint {
        return Err(CadError::input(format!(
            "corner {} is already rounded; a second Fillet rounds another corner",
            second.joint
        )));
    }
    let Some((shared, length)) = shared_side(first, second) else {
        return Ok(());
    };
    if second_radius_mm > pair_bound(length, first_radius_mm) {
        let flat = length - first_radius_mm - second_radius_mm;
        return Err(CadError::input(format!(
            "fillets of {first_radius_mm} mm and {second_radius_mm} mm at the two ends of Line \
             {shared} ({length} mm) would leave {flat} mm of it flat; this build keeps at least \
             {MIN_RADIUS_MM} mm between two arcs on one side and does not build arcs that touch \
             or overlap; nothing is clamped"
        )));
    }
    Ok(())
}

/// §28G: the largest second radius beside a first one of `first_radius_mm`
/// on a shared Line of `length_mm`.
pub fn pair_bound(length_mm: f64, first_radius_mm: f64) -> f64 {
    length_mm - first_radius_mm - MIN_RADIUS_MM
}

/// §28H: the largest first radius beside a second one of
/// `second_radius_mm` on a shared Line of `length_mm`, by the same predicate
/// [`check_pair`] applies in history order (`r2 ≤ pair_bound(L, r1)`).
///
/// Not `pair_bound(L, r2)`: the rule is not symmetric in floating point, and
/// an offered maximum the check would then refuse (by an ulp) is what §28G
/// measured once already. Bisect the ordered positive-float bit patterns,
/// so the search takes at most 63 steps even when the answer is near zero.
/// Returns NaN for non-finite/negative inputs or when no nonnegative first
/// radius fits. The corner and minimum-radius checks remain the caller's.
pub fn pair_bound_of_first(length_mm: f64, second_radius_mm: f64) -> f64 {
    if !length_mm.is_finite()
        || !second_radius_mm.is_finite()
        || length_mm <= 0.0
        || second_radius_mm < 0.0
    {
        return f64::NAN;
    }
    let fits = |first: f64| second_radius_mm <= pair_bound(length_mm, first);
    if !fits(0.0) {
        return f64::NAN;
    }
    // Zero fits, while L cannot: its remainder is -MIN_RADIUS_MM.
    // Nonnegative finite f64 bit patterns have the same order as their values.
    let (mut low, mut high) = (0_u64, length_mm.to_bits());
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if fits(f64::from_bits(mid)) {
            low = mid;
        } else {
            high = mid;
        }
    }
    f64::from_bits(low)
}

/// The Line two corners share, with its length, if they are adjacent.
pub fn shared_side(first: &FilletCorner, second: &FilletCorner) -> Option<(StableEntityId, f64)> {
    let theirs = second.joint.segments();
    first
        .joint
        .segments()
        .into_iter()
        .zip(first.adjacent_lengths_mm)
        .find(|(segment, _)| theirs.contains(segment))
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
    /// The four candidates, in stored segment order, read from the stored
    /// Lines.
    pub corners: Vec<FilletCorner>,
    /// §28F: whether the profile carries the constraint editor's managed Line
    /// family. Then the stored Lines are the solver's starting guess: the
    /// candidates' joints are structural facts, their positions, sides and
    /// `max_radius_mm` are the guess's, and the part is judged on the solved
    /// plate when the copy is rebuilt.
    pub constrained: bool,
    /// §28G: the feature whose result the new Fillet rounds — the base
    /// Extrude for a plain plate, the one saved Fillet otherwise.
    pub previous_feature: ObjectId,
    /// §28G: the Fillets the plate already has, at most one. Its corner is
    /// not among `corners`.
    pub fillets: Vec<ExistingFillet>,
}

/// §28G: a Fillet a plate already carries, as a new Fillet's neighbour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExistingFillet {
    pub feature: ObjectId,
    pub edge: FilletEdge,
    pub radius_mm: f64,
    /// Its corner on the stored Lines.
    pub corner: FilletCorner,
}

impl SavedFilletTarget {
    /// §28G: the saved Fillet sharing a Line with `corner`, with that Line and
    /// its stored length, if there is one.
    pub fn adjacent_fillet(
        &self,
        corner: &FilletCorner,
    ) -> Option<(&ExistingFillet, StableEntityId, f64)> {
        self.fillets
            .iter()
            .find_map(|f| shared_side(&f.corner, corner).map(|(line, length)| (f, line, length)))
    }

    /// The largest radius this target can promise at `corner` without a
    /// solve: §28A's bound, and for an adjacent corner also the pair policy.
    /// `None` for a constrained plate, whose sides only the solve knows.
    pub fn max_radius_mm(&self, corner: &FilletCorner) -> Option<f64> {
        if self.constrained {
            return None;
        }
        let pair = self
            .adjacent_fillet(corner)
            .map_or(f64::INFINITY, |(f, _, length)| {
                pair_bound(length, f.radius_mm)
            });
        Some(corner.max_radius_mm.min(pair))
    }

    /// The radius policy this target can judge without a solve: the whole of
    /// it for an unconstrained plate, only its value part for a constrained
    /// one, whose sides only the solve knows.
    pub fn check_radius(&self, corner: &FilletCorner, radius_mm: f64) -> Result<()> {
        if self.constrained {
            return check_radius_value(radius_mm);
        }
        corner.check_radius(radius_mm)?;
        for existing in &self.fillets {
            check_pair(&existing.corner, existing.radius_mm, corner, radius_mm)?;
        }
        Ok(())
    }

    /// The corner a stated edge means on this target: one of the corners
    /// still sharp. The one a saved Fillet rounded is refused by name.
    pub fn corner_for(&self, edge: FilletEdge) -> Result<FilletCorner> {
        if let Some(existing) = self.fillets.iter().find(|f| f.edge == edge) {
            return Err(CadError::input(format!(
                "corner {} is already rounded by Fillet {}; a second Fillet rounds another \
                 corner",
                edge.joint, existing.feature
            )));
        }
        corner_for(&self.corners, edge)
    }
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
        let corner = target.corner_for(fillet.edge)?;
        target.check_radius(&corner, fillet.radius_mm)?;
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
    let fillets: Vec<&ObjectRecord> = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
        .collect();
    // The last one: the Fillet no other Fillet rounds the result of.
    let Some(tip) = fillets.iter().find(|f| {
        !fillets
            .iter()
            .any(|o| matches!(&o.payload, ObjectPayload::Fillet(x) if x.previous == f.id))
    }) else {
        return Ok(());
    };
    if fillets.len() > 1 {
        return Err(unsupported(format!(
            "this Body ends in Fillet {} after {} Fillets in all (§28G); only the radius of \
             either Fillet (edit-fillet-radius, §28H) and the plate's height (edit-extrude, \
             §28I) can be edited. Editing the Sketch or constraints of a history with two \
             Fillets, and adding a third Fillet or a Cut, are not supported yet",
            tip.id,
            fillets.len()
        )));
    }
    Err(unsupported(format!(
        "this Body ends in Fillet {} (§28A); only its radius (edit-fillet-radius, §28B), the \
         rounded plate's height (edit-extrude, §28C), its base Sketch's coordinates \
         (edit-sketch-copy, §28D) and that Sketch's Line constraints \
         (edit-sketch-constraints-copy, §28E) can be edited, and a second Fillet added on \
         another corner (fillet-edge-copy, §28G). Editing the rest of a filleted part, and \
         adding a Cut after a Fillet, are not supported yet",
        tip.id
    )))
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
    let fillets = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
        .count();
    match fillets {
        0 => {}
        1 => return second_fillet_target(document, objects, body),
        _ => {
            return Err(unsupported(format!(
                "this plate already carries {fillets} Fillets; this build adds a second Fillet \
                 to a plate with one, and a third Fillet is not supported"
            )));
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
    // The reader has already admitted only the managed Line family; the
    // stored Lines still have to be the rectangle whose joints are offered.
    let corners = corners_of_lines(target.base_feature, &sketch.curves)?;
    Ok(SavedFilletTarget {
        body,
        base_feature: target.base_feature,
        profile: target.profile,
        profile_segments: target.profile_segments,
        height_mm: target.height_mm,
        corners,
        constrained: !sketch.constraints.is_empty(),
        previous_feature: target.base_feature,
        fillets: Vec::new(),
    })
}

/// §28G: the plate under its one saved Fillet, as a target for a second.
///
/// The whole §28B frame (`fillet_over_plate`: the exact history, the Body
/// tip, the dependencies and the Fillet's seven names) is what admits it. The
/// candidates are the three corners still sharp; the new Fillet rounds the
/// saved one's result, at a corner named by the base Extrude.
fn second_fillet_target(
    document: &Document,
    objects: &[ObjectRecord],
    body: ObjectId,
) -> Result<SavedFilletTarget> {
    let saved = crate::fillet_radius::fillet_over_plate(document, objects)?
        .ok_or_else(|| unsupported("the plate's Fillet is missing"))?;
    if saved.body != body {
        return Err(CadError::input(format!(
            "Body {body} is not the rounded plate's Body {}",
            saved.body
        )));
    }
    let profile = objects
        .iter()
        .find(|o| o.id == saved.profile)
        .ok_or_else(|| unsupported("the part's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(unsupported("the part's profile is not a Sketch"));
    };
    let all = corners_of_lines(saved.previous, &sketch.curves)?;
    let corners = all
        .iter()
        .filter(|c| c.joint != saved.edge.joint)
        .copied()
        .collect();
    Ok(SavedFilletTarget {
        body,
        base_feature: saved.previous,
        profile: saved.profile,
        profile_segments: sketch.curves.iter().map(|c| c.id).collect(),
        height_mm: saved.height_mm,
        corners,
        constrained: saved.constrained,
        previous_feature: saved.feature,
        fillets: vec![ExistingFillet {
            feature: saved.feature,
            edge: saved.edge,
            radius_mm: saved.radius_mm,
            corner: saved.corner,
        }],
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
    pub(crate) constrained: bool,
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
    /// The corner being rounded, with its labels, read from the stored
    /// Lines.
    pub fn corner(&self) -> FilletCorner {
        self.corner
    }
    /// §28F: whether the plate's Sketch carries constraints, so that
    /// [`Self::corner`] is the stored guess's and the part's corner is the
    /// solved one.
    pub fn profile_constrained(&self) -> bool {
        self.constrained
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

/// §28G: the name a later Fillet gives an earlier Fillet's face.
pub(crate) fn origin_fillet_reference(feature: ObjectId, existing: &ExistingFillet) -> TopologyRef {
    TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Face,
        output_role: SemanticRole::OriginFilletFace {
            origin_feature: existing.feature,
            edge_feature: existing.edge.feature,
            joint: existing.edge.joint,
        },
        selection: SelectionRule::Exact,
        fallback_signature: None,
    }
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
    let corner = target.corner_for(fillet.edge)?;
    target.check_radius(&corner, fillet.radius_mm)?;
    let previous = target.previous_feature;

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
            previous,
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
    let mut references = fillet_references(
        feature_id,
        target.base_feature,
        corner.joint,
        &target.profile_segments,
    );
    // §28G: the saved Fillet's own face, as the finished part has it.
    for existing in &target.fillets {
        references.push(origin_fillet_reference(feature_id, existing));
    }
    Ok(PreparedEdgeFillet {
        body: moved,
        feature,
        added_dependencies,
        removed_dependencies,
        references,
        previous,
        corner,
        constrained: target.constrained,
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
///
/// §28G: a Fillet whose `previous` is another Fillet is checked the same way
/// at both corners, on the same built Lines: the earlier Fillet must round the
/// base Extrude itself and be the only other Fillet, the two corners must
/// differ, each radius must fit its own corner, and two adjacent corners must
/// pass [`check_pair`]. `built` is the base Extrude's.
pub fn evaluable_fillet(
    objects: &[ObjectRecord],
    fillet: &Fillet,
    built: Option<&[crate::SketchCurve]>,
) -> Result<FilletCorner> {
    if fillet.edge.feature == fillet.previous {
        return plate_corner(objects, fillet, built);
    }
    let earlier = objects
        .iter()
        .find(|o| o.id == fillet.previous)
        .ok_or_else(|| CadError::input("the Fillet's predecessor is missing"))?;
    let ObjectPayload::Fillet(first) = &earlier.payload else {
        return Err(unsupported(
            "this build rounds an edge of the feature a Fillet consumes, or a second corner of \
             the plate an earlier Fillet rounded, and this Fillet names neither",
        ));
    };
    if first.previous != first.edge.feature || first.edge.feature != fillet.edge.feature {
        return Err(unsupported(format!(
            "this build rounds a second corner of the plate Fillet {} rounded, and this Fillet \
             names an edge of another feature",
            earlier.id
        )));
    }
    let fillets = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
        .count();
    if fillets != 2 {
        return Err(unsupported(format!(
            "this build rounds at most two corners of one plate, and this document holds \
             {fillets} Fillets"
        )));
    }
    if first.edge.joint == fillet.edge.joint {
        return Err(CadError::input(format!(
            "corner {} is already rounded by Fillet {}; a second Fillet rounds another corner",
            fillet.edge.joint, earlier.id
        )));
    }
    let first_corner = plate_corner(objects, first, built)?;
    let plain = Fillet {
        previous: fillet.edge.feature,
        edge: fillet.edge,
        radius_mm: fillet.radius_mm,
    };
    let second_corner = plate_corner(objects, &plain, built)?;
    check_pair(
        &first_corner,
        first.radius_mm,
        &second_corner,
        fillet.radius_mm,
    )
    .map_err(|e| CadError::input(format!("as the plate is built, {}", bare(&e))))?;
    Ok(second_corner)
}

/// One Fillet's corner and radius on the plate it rounds: §28A–F's check.
fn plate_corner(
    objects: &[ObjectRecord],
    fillet: &Fillet,
    built: Option<&[crate::SketchCurve]>,
) -> Result<FilletCorner> {
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

        // §28G: a second Fillet is offered on the three other corners, and
        // the same corner again is refused by name; every editor of the plate
        // but its height (§28C) refuses by name.
        let again = fillet_choices(&d, &d.objects().expect("objects"));
        let second = again[0].target.as_ref().expect("a second target");
        assert_eq!(second.previous_feature, saved.id);
        assert_eq!(second.corners.len(), 3);
        assert!(second.corners.iter().all(|c| c.joint != fillet.edge.joint));
        let twice = prepare_edge_fillet(&d, body, &fillet).expect_err("the same corner");
        assert_eq!(twice.kind(), ErrorKind::Input);
        assert!(twice.to_string().contains(&saved.id.to_string()), "{twice}");
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

    /// A first Fillet written through the shipped preparation and writer.
    fn first_fillet(d: &mut Document, body: ObjectId, at: usize, r: f64) -> (ObjectId, FilletEdge) {
        let target = fillet_choices(d, &d.objects().expect("objects"))[0]
            .target
            .clone()
            .expect("a target");
        let edge = FilletEdge {
            feature: target.base_feature,
            joint: target.corners[at].joint,
        };
        let prepared =
            prepare_edge_fillet(d, body, &EdgeFillet { edge, radius_mm: r }).expect("first");
        d.write_edge_fillet(&prepared).expect("written");
        (prepared.feature().id, edge)
    }

    /// §28G: a second Fillet rounds the first one's result at another corner,
    /// named by the base; the writer re-derives it; the pair policy is one
    /// rule for discovery, preparation and the evaluator; a third Fillet and
    /// every editor of a two-Fillet history refuse by name.
    #[test]
    fn a_second_fillet_is_written_on_the_first_and_a_third_is_refused() {
        let (_root, mut d, body) = plate(PLATE);
        // 6.125 mm is half the 12.25 mm side: the largest first radius.
        let (first, first_edge) = first_fillet(&mut d, body, 2, 6.125);
        let choice = fillet_choices(&d, &d.objects().expect("objects"))[0].clone();
        assert_eq!(choice.refusal, None);
        let target = choice.target.clone().expect("a second target");
        assert_eq!(target.previous_feature, first);
        assert_eq!(target.fillets.len(), 1);
        assert_eq!(target.fillets[0].feature, first);
        assert_eq!(target.fillets[0].edge, first_edge);
        assert_eq!(target.fillets[0].radius_mm, 6.125);
        assert_eq!(target.corners.len(), 3);
        let adjacent = *target
            .corners
            .iter()
            .find(|c| {
                target
                    .adjacent_fillet(c)
                    .is_some_and(|(_, _, l)| l == 12.25)
            })
            .expect("the corner across the short side");
        let long = *target
            .corners
            .iter()
            .find(|c| target.adjacent_fillet(c).is_some_and(|(_, _, l)| l == 37.5))
            .expect("the corner across the long side");
        let opposite = *target
            .corners
            .iter()
            .find(|c| target.adjacent_fillet(c).is_none())
            .expect("the opposite corner");
        let pair_max = target.max_radius_mm(&adjacent).expect("unconstrained");
        assert!((pair_max - (12.25 - 6.125 - MIN_RADIUS_MM)).abs() < 1e-12);
        assert_eq!(target.max_radius_mm(&long), Some(6.125));
        assert_eq!(target.max_radius_mm(&opposite), Some(6.125));
        let ask = |corner: &FilletCorner, radius_mm| EdgeFillet {
            edge: FilletEdge {
                feature: target.base_feature,
                joint: corner.joint,
            },
            radius_mm,
        };
        // Touching arcs, and a flat narrower than the smallest radius.
        for r in [6.125, 6.12] {
            let e = choice.validate(&ask(&adjacent, r)).expect_err("touching");
            assert_eq!(e.kind(), ErrorKind::Input, "{r}");
            assert!(e.to_string().contains("flat"), "{e}");
        }
        assert_eq!(
            choice.validate(&ask(&adjacent, 6.1)).expect("valid"),
            adjacent
        );
        assert_eq!(
            choice.validate(&ask(&opposite, 6.125)).expect("valid"),
            opposite
        );
        assert!(
            choice.validate(&ask(&opposite, 6.2)).is_err(),
            "§28A's bound"
        );

        let honest = prepare_edge_fillet(&d, body, &ask(&adjacent, 6.1)).expect("prepared");
        assert_eq!(honest.previous(), first);
        assert_eq!(honest.references().len(), 8);
        let ObjectPayload::Fillet(stored) = &honest.feature().payload else {
            panic!("a Fillet");
        };
        assert_eq!(stored.previous, first);
        assert_eq!(
            stored.edge.feature, target.base_feature,
            "never the producer"
        );
        assert_eq!(stored.schema_version(), 2);
        assert!(
            honest
                .feature()
                .payload
                .required_capabilities()
                .contains(&crate::FEATURE_FILLET_SEQUENTIAL_CAPABILITY.to_owned())
        );
        assert_eq!(
            honest.references()[7].output_role,
            SemanticRole::OriginFilletFace {
                origin_feature: first,
                edge_feature: target.base_feature,
                joint: first_edge.joint,
            }
        );

        // Forged: the first Fillet's face dropped, the plate as the result
        // rounded, the first Fillet as the edge's producer, the tip left.
        let mut forged = Vec::new();
        let mut p = honest.clone();
        p.references.pop();
        forged.push(p);
        let mut p = honest.clone();
        if let ObjectPayload::Fillet(f) = &mut p.feature.payload {
            f.previous = target.base_feature;
        }
        forged.push(p);
        let mut p = honest.clone();
        if let ObjectPayload::Fillet(f) = &mut p.feature.payload {
            f.edge.feature = first;
        }
        forged.push(p);
        let mut p = honest.clone();
        p.removed_dependencies.clear();
        forged.push(p);
        for p in &forged {
            assert!(d.write_edge_fillet(p).is_err());
        }
        assert_eq!(d.objects().expect("objects").len(), 5, "a refusal wrote");

        d.write_edge_fillet(&honest).expect("the honest plan");
        let second = honest.feature().id;
        assert_eq!(d.objects().expect("objects").len(), 6);
        let dependencies = d.dependencies().expect("deps");
        for (dependent, dependency, role, present) in [
            (second, first, DependencyRole::Predecessor, true),
            (body, second, DependencyRole::BodyTip, true),
            (body, first, DependencyRole::BodyTip, false),
        ] {
            let found = dependencies.contains(&Dependency {
                dependent,
                dependency,
                role,
            });
            assert_eq!(found, present, "{dependent} → {dependency}");
        }
        assert_eq!(d.topology_refs().expect("refs").len(), 15);
        assert!(d.validate().expect("validate").is_ok());

        // The evaluator's rule, on the Lines the plate is built from.
        let objects = d.objects().expect("objects");
        let drawn = objects
            .iter()
            .find_map(|o| match &o.payload {
                ObjectPayload::Sketch(s) => Some(s.curves.clone()),
                _ => None,
            })
            .expect("the Sketch");
        assert_eq!(
            evaluable_fillet(&objects, stored, Some(&drawn)).expect("evaluable"),
            adjacent
        );
        let mut wide = stored.clone();
        wide.radius_mm = 6.12;
        let e = evaluable_fillet(&objects, &wide, Some(&drawn)).expect_err("touching");
        assert_eq!(e.kind(), ErrorKind::Input);
        let mut same = stored.clone();
        same.edge.joint = first_edge.joint;
        let e = evaluable_fillet(&objects, &same, Some(&drawn)).expect_err("the same corner");
        assert!(e.to_string().contains("already rounded"), "{e}");
        let mut producer = stored.clone();
        producer.edge.feature = first;
        let e = evaluable_fillet(&objects, &producer, Some(&drawn)).expect_err("the producer");
        assert_eq!(e.kind(), ErrorKind::Unsupported);

        // A third Fillet, and every editor of a history with two.
        let third = fillet_choices(&d, &d.objects().expect("objects"))[0].clone();
        assert!(third.target.is_none());
        assert!(
            third
                .refusal
                .as_deref()
                .is_some_and(|r| r.contains("third"))
        );
        let e = prepare_edge_fillet(&d, body, &ask(&opposite, 1.0)).expect_err("a third");
        assert_eq!(e.kind(), ErrorKind::Unsupported);
        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        // §28H: the radius of either Fillet is editable; §28I: and the base
        // height, with both Fillets as context; the Sketch editors are not.
        assert!(reading.fillet_features.iter().all(|c| c.refusal.is_none()));
        assert_eq!(reading.fillet_features.len(), 2);
        let (base,) = match reading.features.as_slice() {
            [base] => (base,),
            other => panic!("one Extrude: {other:?}"),
        };
        assert!(base.refusal.is_none(), "{:?}", base.refusal);
        let (one, two) = (
            base.fillet.as_ref().expect("Fillet 1"),
            base.second_fillet.as_ref().expect("Fillet 2"),
        );
        assert_eq!(one.previous, base.feature);
        assert_eq!(
            two.previous, one.feature,
            "Fillet 2 rounds Fillet 1's result"
        );
        assert_eq!((one.history_index, two.history_index), (1, 2));
        assert!(reading.sketches.iter().all(|s| s.refusal.is_some()));
        assert!(
            reading
                .constraint_sketches
                .iter()
                .all(|s| s.refusal.is_some())
        );
        assert!(reading.cut_bodies.iter().all(|c| c.refusal.is_some()));
    }

    /// §28G: a Fillet's edge comes from the result it rounds or one of that
    /// result's predecessors, never from outside its history.
    #[test]
    fn the_validator_refuses_an_edge_outside_the_fillets_history() {
        let (_root, mut d, body) = plate(PLATE);
        let (first, _) = first_fillet(&mut d, body, 0, 1.0);
        let target = fillet_choices(&d, &d.objects().expect("objects"))[0]
            .target
            .clone()
            .expect("a second target");
        let prepared = prepare_edge_fillet(
            &d,
            body,
            &EdgeFillet {
                edge: FilletEdge {
                    feature: target.base_feature,
                    joint: target.corners[1].joint,
                },
                radius_mm: 1.5,
            },
        )
        .expect("prepared");
        d.write_edge_fillet(&prepared).expect("written");
        let second = prepared.feature();
        assert!(d.validate().expect("validate").is_ok());
        let ObjectPayload::Fillet(mut outside) = second.payload.clone() else {
            panic!("a Fillet");
        };
        // Its own result is not in the history it rounds.
        outside.edge.feature = second.id;
        d.write(|w| {
            w.put_object(
                second.id,
                None,
                second.ordinal,
                Some("Fillet"),
                &ObjectPayload::Fillet(outside),
            )
            .map(|_| ())
        })
        .expect("forge");
        let found: Vec<_> = d
            .validate()
            .expect("validate")
            .errors()
            .map(|e| e.code)
            .collect();
        assert!(found.contains(&"fillet.edge-outside-history"), "{found:?}");
        assert!(d.objects().expect("objects").iter().any(|o| o.id == first));
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

    /// §28F: a plate whose Sketch carries the managed Line family is a
    /// target. Its candidates are the stored joints; discovery and
    /// preparation judge only the value part of the radius, which the solved
    /// plate completes when the copy is rebuilt; and the writer re-derives
    /// the preparation against the document it writes to, so one prepared
    /// under the constrained policy is refused once that policy no longer
    /// holds there.
    #[test]
    fn a_dimensioned_plate_is_a_target_judged_by_value_until_it_is_solved() {
        let (_root, mut d, body) = plate(PLATE);
        let objects = d.objects().expect("objects");
        let (sketch_id, drawn) = objects
            .iter()
            .find_map(|o| match &o.payload {
                ObjectPayload::Sketch(s) => Some((o.id, s.clone())),
                _ => None,
            })
            .expect("the Sketch");
        let edits = crate::SketchConstraintEdits {
            remove: Vec::new(),
            add: vec![crate::AddSketchConstraint::Line(
                crate::AddLineConstraint::Line {
                    curve: drawn.curves[0].id,
                    kind: crate::LineConstraintKind::Horizontal,
                },
            )],
        };
        let prepared = crate::prepare_sketch_constraints(&d, sketch_id, &edits).expect("prepared");
        d.write_sketch_constraints(&prepared).expect("written");

        let choice = fillet_choices(&d, &d.objects().expect("objects"))[0].clone();
        assert_eq!(choice.refusal, None);
        let target = choice.target.clone().expect("a target");
        assert!(target.constrained);
        assert_eq!(
            target.corners,
            corners_of_lines(target.base_feature, &drawn.curves).expect("stored corners"),
            "the candidates are the stored joints"
        );
        let corner = target.corners[1];
        assert_eq!(corner.max_radius_mm, 6.125, "the stored guess's bound");
        let ask = |radius_mm| EdgeFillet {
            edge: FilletEdge {
                feature: target.base_feature,
                joint: corner.joint,
            },
            radius_mm,
        };
        // Beyond the stored bound: the solved plate decides.
        assert_eq!(choice.validate(&ask(7.0)).expect("value only"), corner);
        for (radius, why) in [(0.005, "at least"), (f64::NAN, "finite")] {
            let e = choice.validate(&ask(radius)).expect_err(why);
            assert_eq!(e.kind(), ErrorKind::Input);
            assert!(e.to_string().contains(why), "{e}");
        }
        let prepared = prepare_edge_fillet(&d, body, &ask(7.0)).expect("prepared");
        assert!(prepared.profile_constrained());
        assert_eq!(prepared.corner(), corner);

        // The same plate made unconstrained under the preparation: the
        // writer re-derives it with the stored bound and refuses, writing
        // nothing.
        let ordinal = objects
            .iter()
            .find(|o| o.id == sketch_id)
            .expect("row")
            .ordinal;
        d.write(|w| {
            w.put_object(
                sketch_id,
                None,
                ordinal,
                Some("Profile"),
                &ObjectPayload::Sketch(drawn.clone()),
            )
            .map(|_| ())
        })
        .expect("unconstrained again");
        let count = d.objects().expect("objects").len();
        let e = d
            .write_edge_fillet(&prepared)
            .expect_err("stale preparation");
        assert_eq!(e.kind(), ErrorKind::Input);
        assert!(e.to_string().contains("too large"), "{e}");
        assert_eq!(d.objects().expect("objects").len(), count);
    }
}
