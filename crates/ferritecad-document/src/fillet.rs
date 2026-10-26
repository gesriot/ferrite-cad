// SPDX-License-Identifier: MIT
//! The evaluator's measured Fillet policy and output naming contract.
use ferritecad_types::{CadError, ObjectId, ProfileJoint, Result, StableEntityId};

use crate::cut_boundary::CutBoundary;
use crate::{Fillet, FilletEdge, ObjectPayload, ObjectRecord, Sketch, SketchGeometry};

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

/// The evaluator accepts one Fillet per vertical corner of the rectangle.
pub const MAX_PLATE_FILLETS: usize = 4;

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

/// §28G/§28L: whether two Fillets at `first` and `second` of one plate leave the
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
            "corner {} is already rounded; another Fillet rounds another corner",
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

/// §28L: [`check_pair`] where one of the two is the saved Fillet `beside`
/// (its feature and corner): a refusal names that Fillet and its joint as well
/// as the shared Line, so a person can tell which of up to three neighbours
/// of one corner is the one in the way. The kind and the numbers are the
/// predicate's own.
pub(crate) fn check_pair_beside(
    beside: (ObjectId, ProfileJoint),
    first: &FilletCorner,
    first_radius_mm: f64,
    second: &FilletCorner,
    second_radius_mm: f64,
) -> Result<()> {
    check_pair(first, first_radius_mm, second, second_radius_mm).map_err(|e| {
        let message = format!(
            "beside Fillet {} at corner {}: {}",
            beside.0,
            beside.1,
            bare(&e)
        );
        match e.kind() {
            ferritecad_types::ErrorKind::Input => CadError::input(message),
            _ => unsupported(message),
        }
    })
}

/// §28G: the largest second radius beside a first one of `first_radius_mm`
/// on a shared Line of `length_mm`.
pub fn pair_bound(length_mm: f64, first_radius_mm: f64) -> f64 {
    length_mm - first_radius_mm - MIN_RADIUS_MM
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
pub(crate) fn chain_below(
    objects: &[ObjectRecord],
    previous: ObjectId,
) -> Result<(ObjectId, Vec<&ObjectRecord>)> {
    let mut earlier = Vec::new();
    let mut cursor = previous;
    loop {
        let record = objects
            .iter()
            .find(|o| o.id == cursor)
            .ok_or_else(|| CadError::input("the Fillet's predecessor is missing"))?;
        match &record.payload {
            ObjectPayload::Extrude(_) => {
                earlier.reverse();
                return Ok((record.id, earlier));
            }
            ObjectPayload::Fillet(f) => {
                // The Fillet being judged is the last: at most three below it.
                if earlier.len() + 1 >= MAX_PLATE_FILLETS {
                    return Err(unsupported(format!(
                        "this build rounds at most {MAX_PLATE_FILLETS} corners of one plate, one \
                         Fillet each, and the history below Fillet {} is longer",
                        record.id
                    )));
                }
                earlier.push(record);
                cursor = f.previous;
            }
            ObjectPayload::Chamfer(_) => {
                return Err(unsupported(format!(
                    "this build adds nothing after a Chamfer (§29A), and the predecessor \
                     {cursor} is one"
                )));
            }
            _ => {
                return Err(unsupported(format!(
                    "this build rounds an edge of an extruded plate, or a corner of the plate an \
                     earlier Fillet rounded, and the predecessor {cursor} is neither an Extrude \
                     nor a Fillet of that plate"
                )));
            }
        }
    }
}

/// §28L: no feature of the chain has a second Fillet over it. A branch rounds
/// the same result twice and is no plate this build edits or rebuilds.
pub(crate) fn refuse_branch(
    objects: &[ObjectRecord],
    base: ObjectId,
    earlier: &[&ObjectRecord],
) -> Result<()> {
    for below in std::iter::once(base).chain(earlier.iter().map(|f| f.id)) {
        let over: Vec<ObjectId> = objects
            .iter()
            .filter(|o| matches!(&o.payload, ObjectPayload::Fillet(f) if f.previous == below))
            .map(|o| o.id)
            .collect();
        if over.len() > 1 {
            return Err(unsupported(format!(
                "Fillets {} and {} both round the result of {below}; this build rounds the \
                 corners of one plate one after the other, in one chain, never in a branch",
                over[0], over[1]
            )));
        }
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
/// §28G/§28L: a Fillet whose `previous` is another Fillet is checked the same
/// way at its own corner **and at every earlier Fillet's**, on the same built
/// Lines: the chain below it must be a chain of Fillets over the base Extrude
/// (at most four in all, one per corner, no branch), every joint must differ,
/// each radius must fit its own corner, and the new radius must pass
/// [`check_pair`] against **every** earlier Fillet whose corner shares a Line
/// with its own — in the order of the history, whatever Fillets lie between.
/// `built` is the base Extrude's. This is the one check on the plate as it is
/// built; every discovery and edit that proposes a radius asks the same
/// predicate.
pub fn evaluable_fillet(
    objects: &[ObjectRecord],
    fillet: &Fillet,
    built: Option<&[crate::SketchCurve]>,
) -> Result<FilletCorner> {
    if fillet.edge.feature == fillet.previous {
        refuse_branch(objects, fillet.previous, &[])?;
        return plate_corner(objects, fillet, built);
    }
    let (base, earlier) = chain_below(objects, fillet.previous)?;
    if earlier.is_empty() {
        return Err(unsupported(
            "this build rounds an edge of the feature a Fillet consumes, or a corner of the \
             plate earlier Fillets rounded, and this Fillet names neither",
        ));
    }
    refuse_branch(objects, base, &earlier)?;
    let rounded: Vec<&Fillet> = earlier
        .iter()
        .filter_map(|o| match &o.payload {
            ObjectPayload::Fillet(f) => Some(f),
            _ => None,
        })
        .collect();
    // Every Fillet of the chain rounds an edge of the one base Extrude.
    if fillet.edge.feature != base
        || rounded.iter().any(|f| f.edge.feature != base)
        || rounded.first().is_some_and(|f| f.previous != base)
    {
        return Err(unsupported(format!(
            "this build rounds a corner of the plate Fillet {} rounded, and this Fillet \
             names an edge of another feature",
            earlier[0].id
        )));
    }
    if let Some((j, _)) = rounded
        .iter()
        .enumerate()
        .find(|(_, f)| f.edge.joint == fillet.edge.joint)
    {
        return Err(CadError::input(format!(
            "corner {} is already rounded by Fillet {}; a Fillet rounds another corner",
            fillet.edge.joint, earlier[j].id
        )));
    }
    let plain = |f: &Fillet| Fillet {
        previous: base,
        edge: f.edge,
        radius_mm: f.radius_mm.clone(),
    };
    let corners: Vec<FilletCorner> = rounded
        .iter()
        .map(|f| plate_corner(objects, &plain(f), built))
        .collect::<Result<_>>()?;
    let own = plate_corner(objects, &plain(fillet), built)?;
    for (k, (f, corner)) in rounded.iter().zip(&corners).enumerate() {
        check_pair_beside(
            (earlier[k].id, f.edge.joint),
            corner,
            f.radius_mm.value(),
            &own,
            fillet.radius_mm.value(),
        )
        .map_err(|e| CadError::input(format!("as the plate is built, {}", bare(&e))))?;
    }
    Ok(own)
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
        corner.check_radius(fillet.radius_mm.value())?;
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
    corner.check_radius(fillet.radius_mm.value()).map_err(|e| {
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
    use crate::{Body, Dependency, DependencyRole, Document};
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
                    regions: Vec::new(),
                    profile,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(6.75)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
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

    /// The stored payload round-trips and refuses a radius no fillet has and a
    /// field this reader would drop.
    #[test]
    fn the_fillet_payload_round_trips_and_refuses_what_it_cannot_mean() {
        let [a, b] = [(); 2].map(|_| StableEntityId::new());
        let fillet = Fillet {
            previous: ObjectId::new(),
            edge: FilletEdge {
                feature: ObjectId::new(),
                joint: ProfileJoint::new(b, a).expect("joint"),
            },
            radius_mm: crate::Expression::constant(2.375).expect("finite constant"),
        };
        let payload = ObjectPayload::Fillet(fillet.clone());
        let bytes = payload.to_storage_bytes().expect("encode");
        assert_eq!(
            ObjectPayload::from_storage_bytes(&bytes).expect("decode"),
            payload
        );
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            if !radius.is_finite() {
                assert!(crate::Expression::constant(radius).is_err());
                continue;
            }
            let mut bad = fillet.clone();
            bad.radius_mm = crate::Expression::constant(radius).expect("finite constant");
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

    mod policy {
        use super::*;
        use ferritecad_types::ErrorKind;
        const PLATE: [[f64; 2]; 4] = [[-4.5, 3.25], [33., 3.25], [33., 15.5], [-4.5, 15.5]];
        const FOUR: [(usize, f64); 4] = [(1, 2.), (3, 2.), (0, 2.), (2, 2.)];
        #[test]
        fn the_validator_refuses_a_fillet_without_its_predecessor_edge_or_target() {
            let (_root, mut d, body) = plate(PLATE);
            let id = add(&mut d, body, 0, 1.0);
            let mut fillet = stored_fillet(&d, id);
            let base = fillet.previous;
            let profile = d
                .objects()
                .expect("objects")
                .into_iter()
                .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
                .expect("profile")
                .id;
            let codes = |d: &Document| {
                d.validate()
                    .expect("validate")
                    .errors()
                    .map(|e| e.code)
                    .collect::<Vec<_>>()
            };
            assert!(codes(&d).is_empty());
            d.write(|w| {
                w.remove_dependency(Dependency {
                    dependent: id,
                    dependency: base,
                    role: DependencyRole::Predecessor,
                })
            })
            .expect("drop edge");
            assert!(codes(&d).contains(&"reference.missing-edge"));
            fillet.edge.feature = profile;
            fillet.previous = id;
            d.write(|w| w.update_payload(id, &ObjectPayload::Fillet(fillet)))
                .expect("forge");
            let found = codes(&d);
            assert!(found.contains(&"feature.self-predecessor"), "{found:?}");
            assert!(found.contains(&"reference.missing-target"), "{found:?}");
        }

        #[test]
        fn the_validator_refuses_an_edge_outside_the_fillets_history() {
            let (_root, mut d, body) = plate(PLATE);
            let first = add(&mut d, body, 0, 1.0);
            let second = add(&mut d, body, 1, 1.5);
            assert!(d.validate().expect("validate").is_ok());
            let mut outside = stored_fillet(&d, second);
            outside.edge.feature = second;
            d.write(|w| w.update_payload(second, &ObjectPayload::Fillet(outside)))
                .expect("forge");
            let report = d.validate().expect("validate");
            assert!(
                report
                    .errors()
                    .any(|e| e.code == "fillet.edge-outside-history"),
                "{report:?}"
            );
            assert!(d.object(first).expect("first").is_some());
        }

        fn add(d: &mut Document, body: ObjectId, at: usize, r: f64) -> ObjectId {
            let objects = d.objects().expect("objects");
            let base = objects
                .iter()
                .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
                .expect("base")
                .id;
            let ObjectPayload::Sketch(sketch) = &objects
                .iter()
                .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
                .expect("sketch")
                .payload
            else {
                panic!("Sketch")
            };
            let corner = corners_of_lines(base, &sketch.curves).expect("corners")[at];
            let id = ObjectId::new();
            d.write_changes(&[crate::EditChange::AddFeature {
                object: id,
                body,
                name: Some("Fillet".into()),
                feature: crate::FeatureAddition::Fillet {
                    edge: FilletEdge {
                        feature: base,
                        joint: corner.joint,
                    },
                    radius_mm: crate::Expression::constant(r).expect("radius"),
                },
            }])
            .expect("write");
            id
        }
        fn rounded(
            points: [[f64; 2]; 4],
            spec: &[(usize, f64)],
        ) -> (tempfile::TempDir, Document, ObjectId, Vec<ObjectId>) {
            let (root, mut d, body) = plate(points);
            let ids = spec
                .iter()
                .map(|&(at, r)| add(&mut d, body, at, r))
                .collect();
            (root, d, body, ids)
        }
        fn filleted(r: f64) -> (tempfile::TempDir, Document, ObjectId) {
            let (root, d, _, ids) = rounded(PLATE, &[(1, r)]);
            (root, d, ids[0])
        }
        fn sequential(
            r1: f64,
            at: usize,
            r2: f64,
        ) -> (tempfile::TempDir, Document, ObjectId, ObjectId) {
            let (root, d, _, ids) = rounded(PLATE, &[(1, r1), (at, r2)]);
            (root, d, ids[0], ids[1])
        }

        fn base_of(d: &Document, fillet: ObjectId) -> ObjectId {
            match &d.object(fillet).expect("read").expect("Fillet").payload {
                ObjectPayload::Fillet(f) => f.previous,
                _ => panic!("a Fillet"),
            }
        }
        fn stored_fillet(d: &Document, fillet: ObjectId) -> Fillet {
            match &d.object(fillet).expect("read").expect("Fillet").payload {
                ObjectPayload::Fillet(f) => f.clone(),
                _ => panic!("a Fillet"),
            }
        }
        fn plate_lines(d: &Document) -> (ObjectId, Vec<StableEntityId>) {
            let objects = d.objects().expect("objects");
            let sketch = objects
                .iter()
                .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
                .expect("Sketch");
            let ObjectPayload::Sketch(s) = &sketch.payload else {
                unreachable!()
            };
            (sketch.id, s.curves.iter().map(|c| c.id).collect())
        }
        fn line_rule(
            curve: StableEntityId,
            kind: crate::LineConstraintKind,
        ) -> crate::AddSketchConstraint {
            crate::AddSketchConstraint::Line(crate::AddLineConstraint::Line { curve, kind })
        }
        fn constrained(d: &mut Document) -> Vec<crate::EditChange> {
            let (sketch, lines) = plate_lines(d);
            let edits = crate::SketchConstraintEdits {
                remove: Vec::new(),
                add: vec![line_rule(lines[0], crate::LineConstraintKind::Horizontal)],
            };
            let prepared =
                crate::test_changes::constraint_changes(d, sketch, &edits).expect("prepared");
            d.write_changes(&prepared).expect("written");
            prepared
        }
        fn built(lines: &[StableEntityId], at: [[f64; 2]; 4]) -> Vec<SketchCurve> {
            (0..4)
                .map(|i| SketchCurve {
                    id: lines[i],
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: Point2::new(at[i][0], at[i][1]).expect("point"),
                        end: Point2::new(at[(i + 1) % 4][0], at[(i + 1) % 4][1]).expect("point"),
                    },
                })
                .collect()
        }
        fn rect(cw: bool, (x0, y0): (f64, f64), (x1, y1): (f64, f64)) -> [[f64; 2]; 4] {
            if cw {
                [[x0, y1], [x1, y1], [x1, y0], [x0, y0]]
            } else {
                [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
            }
        }
        #[test]
        fn the_evaluator_judges_the_fillet_on_the_built_lines() {
            let (_root, mut d, fillet) = filleted(2.375);
            let (_, lines) = plate_lines(&d);
            let payload = stored_fillet(&d, fillet);
            let objects = d.objects().expect("objects");
            let saved = [PLATE[0], PLATE[1], PLATE[2], PLATE[3]];
            // Unconstrained: exactly the stored Lines, as before.
            crate::evaluable_fillet(&objects, &payload, Some(&built(&lines, saved)))
                .expect("the stored plate");
            assert!(crate::evaluable_fillet(&objects, &payload, None).is_err());
            drop(objects);

            constrained(&mut d);
            let objects = d.objects().expect("objects");
            let judge = |at: [[f64; 2]; 4]| {
                crate::evaluable_fillet(&objects, &payload, Some(&built(&lines, at)))
            };
            let rect = |x0: f64, y0: f64, w: f64, h: f64| {
                [[x0, y0], [x0 + w, y0], [x0 + w, y0 + h], [x0, y0 + h]]
            };
            // Solved larger or smaller than stored: the corner follows its two
            // Lines, and the bound is the solved one, exactly at 2 r.
            let corner = judge(rect(1., 2., 30., 4.75)).expect("exactly 2 r deep");
            assert_eq!(corner.corner_mm, [31., 2.]);
            assert_eq!(corner.adjacent_lengths_mm, [30., 4.75]);
            let e = judge(rect(1., 2., 30., 4.749_999)).expect_err("just under 2 r");
            assert_eq!(e.kind(), ErrorKind::Input);
            assert!(e.to_string().contains("too short"), "{e}");
            // The shared rectangle reader's tolerance, 1e-7 mm, on both sides.
            let mut near = rect(1., 2., 30., 10.);
            near[1][1] += 0.9e-7;
            judge(near).expect("within the rectangle reader's tolerance");
            let mut off = rect(1., 2., 30., 10.);
            off[1][1] += 1.1e-7;
            let e = judge(off).expect_err("beyond it");
            assert_eq!(e.kind(), ErrorKind::Unsupported);
            assert!(e.to_string().contains("rectangle"), "{e}");
            // A Line reversed: the first Line runs -X; still a rectangle.
            let e = judge([[31., 2.], [26., 2.], [26., 12.], [31., 12.]]).expect_err("reversed");
            assert_eq!(e.kind(), ErrorKind::Input);
            assert!(e.to_string().contains("side"), "{e}");
            // The same four Lines in another order.
            let mut shuffled = built(&lines, rect(1., 2., 30., 10.));
            shuffled.rotate_left(1);
            let e = crate::evaluable_fillet(&objects, &payload, Some(&shuffled))
                .expect_err("reordered");
            assert!(e.to_string().contains("same four Lines"), "{e}");
        }
        #[test]
        fn the_evaluator_judges_both_fillets_and_the_pair_on_the_built_lines() {
            // Adjacent, r1 = r2 = 3: each alone fits 6 mm; together they need more.
            let (_a, mut d, first, second) = sequential(3.0, 2, 3.0);
            constrained(&mut d);
            let (_, lines) = plate_lines(&d);
            let objects = d.objects().expect("objects");
            let (one, two) = (stored_fillet(&d, first), stored_fillet(&d, second));
            let judge = |which: &Fillet, at: [[f64; 2]; 4]| {
                crate::evaluable_fillet(&objects, which, Some(&built(&lines, at)))
            };
            let plate = |depth: f64| rect(false, (0., 0.), (20., depth));
            // The part is whatever the solver made: 6 mm deep holds each radius.
            judge(&one, plate(6.0)).expect("Fillet 1 alone fits 2 r");
            let e = judge(&one, plate(5.99)).expect_err("under 2 r1");
            assert_eq!(e.kind(), ErrorKind::Input);
            assert!(e.to_string().contains(&one.edge.joint.to_string()), "{e}");
            let e = judge(&two, plate(5.99)).expect_err("under 2 r2");
            assert!(
                e.to_string().contains("too short") || e.to_string().contains("too large"),
                "{e}"
            );
            // The flat between the two arcs: Fillet 2 is the one that completes
            // the pair, and the shared Line's UUID is what the reason names.
            let e = judge(&two, plate(6.005)).expect_err("no flat left");
            assert_eq!(e.kind(), ErrorKind::Input);
            assert!(e.to_string().contains("flat"), "{e}");
            let shared = crate::fillet::shared_side(
                &crate::corner_for(
                    &crate::fillet::corners_of_lines(base_of(&d, first), &built(&lines, plate(9.)))
                        .expect("corners"),
                    one.edge,
                )
                .expect("corner"),
                &crate::corner_for(
                    &crate::fillet::corners_of_lines(base_of(&d, first), &built(&lines, plate(9.)))
                        .expect("corners"),
                    two.edge,
                )
                .expect("corner"),
            )
            .expect("adjacent corners share a Line")
            .0;
            assert!(e.to_string().contains(&shared.to_string()), "{e}");
            let mut least = 6.01_f64;
            while 3.0 > crate::fillet::pair_bound(least, 3.0) {
                least = least.next_up();
            }
            judge(&two, plate(least)).expect("the least depth that leaves the flat");
            assert!(
                judge(&two, plate(least.next_down())).is_err(),
                "one float below"
            );
            // A Line that changes side is refused, naming the side.
            let e = judge(&two, [[20., 0.], [0., 0.], [0., 9.], [20., 9.]]).expect_err("reversed");
            assert!(e.to_string().contains("side"), "{e}");

            // Opposite corners share no Line: only each radius bounds them.
            let (_o, mut d, _, second) = sequential(3.0, 3, 3.0);
            constrained(&mut d);
            let (_, lines) = plate_lines(&d);
            let objects = d.objects().expect("objects");
            let two = stored_fillet(&d, second);
            let judge = |at: [[f64; 2]; 4]| {
                crate::evaluable_fillet(&objects, &two, Some(&built(&lines, at)))
            };
            judge(plate(6.0)).expect("no shared Line, no flat owed");
            assert!(judge(plate(5.99)).is_err());

            // Fillet 2's own radius, on its own corner: r1 = 2 fits 10.9 mm and
            // r2 = 5.5 does not, and no pair rule is involved (opposite corners).
            let (_w, mut d, _, second) = sequential(2.0, 3, 5.5);
            constrained(&mut d);
            let (_, lines) = plate_lines(&d);
            let objects = d.objects().expect("objects");
            let two = stored_fillet(&d, second);
            let judge = |depth: f64| {
                crate::evaluable_fillet(&objects, &two, Some(&built(&lines, plate(depth))))
            };
            judge(11.0).expect("exactly 2 r2");
            let e = judge(10.9).expect_err("under 2 r2, over 2 r1");
            assert_eq!(e.kind(), ErrorKind::Input);
            assert!(e.to_string().contains(&two.edge.joint.to_string()), "{e}");
        }
        #[test]
        fn the_evaluator_judges_four_fillets_and_every_pair_on_the_built_lines() {
            let (_root, mut d, _body, ids) = rounded(PLATE, &FOUR);
            constrained(&mut d);
            let (_, lines) = plate_lines(&d);
            let objects = d.objects().expect("objects");
            let judge = |k: usize, depth: f64| {
                crate::evaluable_fillet(
                    &objects,
                    &stored_fillet(&d, ids[k]),
                    Some(&built(&lines, rect(false, (0., 0.), (20., depth)))),
                )
            };
            let mut least = 4.01_f64;
            while 2.0 > crate::fillet::pair_bound(least, 2.0) {
                least = least.next_up();
            }
            while 2.0 <= crate::fillet::pair_bound(least.next_down(), 2.0) {
                least = least.next_down();
            }
            for k in 0..4 {
                judge(k, least).unwrap_or_else(|e| panic!("Fillet {} at {least}: {e}", k + 1));
            }
            let below = least.next_down();
            judge(0, below).expect("the first owes no earlier neighbour");
            judge(1, below).expect("opposite the first");
            let third = judge(2, below).expect_err("the third and the second share a short Line");
            assert_eq!(third.kind(), ErrorKind::Input);
            assert!(third.to_string().contains(&ids[1].to_string()), "{third}");
            assert!(third.to_string().contains(&lines[3].to_string()), "{third}");
            let fourth = judge(3, below).expect_err("the fourth closes the pair with the first");
            assert_eq!(fourth.kind(), ErrorKind::Input);
            assert!(fourth.to_string().contains(&ids[0].to_string()), "{fourth}");
            assert!(
                fourth.to_string().contains(&lines[1].to_string()),
                "{fourth}"
            );
            assert!(
                !fourth.to_string().contains(&ids[1].to_string()),
                "not the opposite one: {fourth}"
            );
            // Each radius judged at its own corner: 3.9 mm deep holds no 2 mm radius.
            for k in 0..4 {
                let e = judge(k, 3.9).expect_err("under 2 r");
                assert_eq!(e.kind(), ErrorKind::Input);
            }
        }
    }
}
