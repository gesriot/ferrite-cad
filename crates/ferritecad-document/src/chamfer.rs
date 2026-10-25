// SPDX-License-Identifier: MIT
//! The evaluator's measured Chamfer policy and output naming contract.
use ferritecad_types::{CadError, ObjectId, ProfileJoint, Result};

use crate::fillet::{FilletCorner, corner_for, corners_of_lines};
use crate::{Chamfer, ObjectPayload, ObjectRecord, Sketch, SketchConstraintRule};

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
        SketchConstraintRule::DirectedAngle { .. } => "DirectedAngle",
        SketchConstraintRule::Collinear { .. } => "Collinear",
        _ => "other",
    }
}

/// The plate's Sketch must be free or closure-only. Names the first
/// constraint that is neither.
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
    corner
        .check_distance(chamfer.distance_mm.value())
        .map_err(|e| {
            CadError::input(format!(
                "Chamfer {chamfer_id} of {} mm does not fit the solved plate: {e}",
                chamfer.distance_mm.value()
            ))
        })?;
    Ok(corner)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::{
        Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression, Extrude,
        Point2, SketchCurve, SketchGeometry, SolidOperation, SweptEdge,
    };
    use ferritecad_types::StableEntityId;
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
    fn constrain_plate(d: &mut Document, length: f64) -> (ObjectId, Vec<crate::EditChange>) {
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
        let prepared =
            crate::test_changes::constraint_changes(d, profile, &edits).expect("prepared");
        (profile, prepared)
    }
    fn chamfer_at(d: &mut Document, body: ObjectId, at: usize, distance: f64) -> ObjectId {
        let objects = d.objects().expect("objects");
        let base = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
            .expect("base")
            .id;
        let corners = corners_of_lines(base, &sketch_of(d).curves).expect("corners");
        let id = ObjectId::new();
        d.write_changes(&[crate::EditChange::AddFeature {
            object: id,
            body,
            name: Some("Chamfer".into()),
            feature: crate::FeatureAddition::Chamfer {
                edge: SweptEdge {
                    feature: base,
                    joint: corners[at].joint,
                },
                distance_mm: Expression::constant(distance).expect("distance"),
            },
        }])
        .expect("write");
        id
    }

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
            distance_mm: crate::Expression::constant(2.5).expect("finite constant"),
        };
        let payload = ObjectPayload::Chamfer(chamfer.clone());
        assert_eq!(payload.type_name(), "feature.chamfer");

        let bytes = payload.to_storage_bytes().expect("encodes");
        assert_eq!(
            ObjectPayload::from_storage_bytes(&bytes).expect("decodes"),
            payload
        );
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            if !bad.is_finite() {
                assert!(crate::Expression::constant(bad).is_err());
                continue;
            }
            let mut c = chamfer.clone();
            c.distance_mm = crate::Expression::constant(bad).expect("finite constant");
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
        too_much.distance_mm =
            crate::Expression::constant(corner.max_distance_mm).expect("finite constant");
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
    #[test]
    fn the_evaluator_judges_a_constrained_plate_on_its_solved_lines() {
        let (_root, mut d, body) = plate(PLATE);
        let id = chamfer_at(&mut d, body, 1, 2.5);
        let (_, prepared) = constrain_plate(&mut d, 20.0);
        d.write_changes(&prepared).expect("written");
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
        edge.distance_mm = crate::Expression::constant(at).expect("finite constant");
        evaluable_chamfer(&objects, &edge, Some(&drawn([0.0, 0.0, 30.0, 12.0]))).expect("at");
        edge.distance_mm = crate::Expression::constant(at.next_up()).expect("finite constant");
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
    #[test]
    fn the_distance_policy_is_exact_at_its_bounds_and_the_same_everywhere() {
        let corners = corners_of_lines(ObjectId::new(), &sketch(&PLATE).curves).expect("corners");
        for corner in corners.iter().map(ChamferCorner::of) {
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
        let thin = sketch(&[[0., 0.], [30., 0.], [30., 0.0105], [0., 0.0105]]);
        let corners = corners_of_lines(ObjectId::new(), &thin.curves).expect("corners");
        let corners: Vec<_> = corners.iter().map(ChamferCorner::of).collect();
        assert!(corners.iter().all(|c| !c.is_offerable()));
        assert!(corners[0].check_distance(MIN_DISTANCE_MM).is_err());
    }
}
