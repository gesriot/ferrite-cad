// SPDX-License-Identifier: MIT
//! §31F: the directed angle between two saved Lines, in the document.
//!
//! No solver and no kernel: this is whether the document can hold what the
//! constraint means — its unit boundary, its slot, its layout and capability —
//! and refuse what it must. The solving is measured in the solver, evaluator
//! and command-line gates.
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_types::{ObjectId, StableEntityId, Transform};
use std::f64::consts::PI;

/// A slanted four-Line frame, as the shipped editor reads one.
fn fixture() -> (tempfile::TempDir, Document, ObjectId, Vec<StableEntityId>) {
    let root = tempfile::tempdir().expect("dir");
    let mut d = Document::create(root.path().join("source.fcad")).expect("document");
    let [plane, id, extrude, body] = std::array::from_fn(|_| ObjectId::new());
    let points = [[-20., -10.], [40., -8.], [42., 30.], [-20., 30.]];
    let curves: Vec<SketchCurve> = points
        .iter()
        .enumerate()
        .map(|(i, p)| SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(p[0], p[1]).expect("p"),
                end: Point2::new(points[(i + 1) % 4][0], points[(i + 1) % 4][1]).expect("p"),
            },
        })
        .collect();
    let curve_ids = curves.iter().map(|c| c.id).collect();
    d.write(|w| {
        for (oid, ordinal, payload) in [
            (
                plane,
                0,
                ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            ),
            (
                id,
                1,
                ObjectPayload::Sketch(Sketch {
                    plane,
                    curves,
                    constraints: vec![],
                }),
            ),
            (
                extrude,
                2,
                ObjectPayload::Extrude(Extrude {
                    profile: id,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(10.)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    previous: None,
                }),
            ),
            (
                body,
                3,
                ObjectPayload::Body(Body {
                    tip_feature: Some(extrude),
                }),
            ),
        ] {
            w.put_object(oid, None, ordinal, Some("Profile"), &payload)?;
        }
        for (dependent, dependency, role) in [
            (id, plane, DependencyRole::Plane),
            (extrude, id, DependencyRole::Profile),
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
    (root, d, id, curve_ids)
}
fn stored(d: &Document, id: ObjectId) -> Sketch {
    let ObjectPayload::Sketch(s) = d.object(id).expect("read").expect("sketch").payload else {
        panic!("Sketch")
    };
    s
}
fn deg(value: f64) -> DirectedAngle {
    DirectedAngle::from_degrees(value).expect("a canonical angle")
}
fn vector(curve: StableEntityId, from: LineEndpoint) -> DirectedLine {
    DirectedLine::new(curve, from, from.other()).expect("a vector")
}
fn angle(a: DirectedLine, b: DirectedLine, degrees: f64) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::DirectedAngle {
        a,
        b,
        angle: deg(degrees),
    })
}
fn edits(remove: Vec<StableEntityId>, add: Vec<AddSketchConstraint>) -> SketchConstraintEdits {
    SketchConstraintEdits { remove, add }
}
fn relation(a: StableEntityId, b: StableEntityId, relation: LineRelation) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::Relation { a, b, relation })
}
fn write(d: &mut Document, id: ObjectId, e: &SketchConstraintEdits) -> PreparedSketchConstraints {
    let prepared = prepare_sketch_constraints(d, id, e).expect("prepared");
    d.write_sketch_constraints(&prepared).expect("written");
    prepared
}
fn refusal(d: &Document, id: ObjectId, e: &SketchConstraintEdits) -> String {
    let error = prepare_sketch_constraints(d, id, e).expect_err("a structural refusal");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input, "{error}");
    error.to_string()
}

#[test]
fn degrees_are_the_input_radians_are_the_stored_value_and_the_range_is_one_full_circle() {
    // The values a person types come back as typed, and are stored in radians.
    for typed in [30.0, -30.0, 90.0, 0.5, -179.5, 179.999999, 12.345678, 1e-7] {
        let angle = deg(typed);
        assert_eq!(
            angle.degrees(),
            typed,
            "{typed}° did not come back as typed"
        );
        assert!(
            (angle.radians() - typed.to_radians()).abs() < 1e-15,
            "{typed}° is stored as {} rad",
            angle.radians()
        );
        assert_eq!(
            DirectedAngle::from_radians(angle.radians()).expect("canonical"),
            angle
        );
    }
    // Every thousandth of a degree in the range, in both directions.
    for k in -180_000..=180_000 {
        let typed = f64::from(k) / 1000.;
        let angle = deg(typed);
        assert_eq!(angle.degrees(), if typed == -180. { 180. } else { typed });
        assert!(angle.radians() > -PI && angle.radians() <= PI, "{typed}");
    }
    // The opposite is +180 only; -0 is 0; -180 is the same direction as 180.
    assert_eq!(deg(180.).radians(), PI);
    assert_eq!(deg(-180.).radians(), PI);
    assert_eq!(deg(-180.), deg(180.));
    assert_eq!(deg(-0.).radians().to_bits(), 0f64.to_bits());
    assert_eq!(deg(0.).radians().to_bits(), 0f64.to_bits());
    assert_eq!(deg(-0.), deg(0.));
    // The neighbours of the boundary are not rounded into it.
    let below = f64::from_bits(180f64.to_bits() - 1);
    assert!(deg(below).radians() < PI && deg(-below).radians() > -PI);
    // Reporting must not round a distinct persisted angle to zero or to the
    // opposite direction. In particular, JSON must stay in (-180, 180].
    for typed in [1e-10, -1e-10, 12.345678901234, below, -below] {
        let angle = deg(typed);
        let reported = angle.degrees();
        assert!(reported > -180. && reported <= 180., "{reported}");
        assert_eq!(reported.signum(), typed.signum());
        assert!(
            (reported - typed).abs() <= typed.abs() * 4. * f64::EPSILON,
            "{typed} was reported as {reported}"
        );
    }
    // A nonzero request too small to survive degrees -> radians must not
    // silently become the different constraint 0° (nor a second signed zero).
    for tiny in [f64::from_bits(1), -f64::from_bits(1)] {
        assert!(DirectedAngle::from_degrees(tiny).is_err());
    }
    // One full circle, refused outside it, never wrapped.
    for bad in [
        180.000000000001,
        -180.000000000001,
        181.,
        -181.,
        270.,
        -270.,
        360.,
        -360.,
        720.,
        f64::from_bits(180f64.to_bits() + 1),
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
    ] {
        assert!(
            DirectedAngle::from_degrees(bad).is_err(),
            "{bad}° was taken"
        );
    }
    // A stored value must already be canonical.
    for bad in [-PI, -4., 3.2, 2. * PI, f64::NAN, f64::INFINITY] {
        assert!(DirectedAngle::from_radians(bad).is_err(), "{bad} rad");
    }
    assert_eq!(
        DirectedAngle::from_radians(-0.)
            .expect("-0 is 0")
            .radians()
            .to_bits(),
        0f64.to_bits()
    );
    // A vector runs between the two different ends, and is never a default.
    let id = StableEntityId::new();
    assert!(DirectedLine::new(id, LineEndpoint::Start, LineEndpoint::Start).is_err());
    assert!(DirectedLine::new(id, LineEndpoint::End, LineEndpoint::End).is_err());
    let v = DirectedLine::new(id, LineEndpoint::End, LineEndpoint::Start).expect("reversed");
    assert_eq!((v.from(), v.to()), (LineEndpoint::End, LineEndpoint::Start));
}

#[test]
fn every_way_of_saying_one_relation_occupies_the_slot_and_only_an_exact_replace_frees_it() {
    let (_root, mut d, id, c) = fixture();
    let forward = |i: usize| vector(c[i], LineEndpoint::Start);
    let back = |i: usize| vector(c[i], LineEndpoint::End);
    let first = write(
        &mut d,
        id,
        &edits(vec![], vec![angle(forward(0), forward(1), 30.)]),
    );
    let angle_id = first.added.last().expect("the angle").id;
    let kept = stored(&d, id);
    // The same relation said every other way, plus the neighbours of the slot.
    for (name, add) in [
        ("again", angle(forward(0), forward(1), 30.)),
        ("another value", angle(forward(0), forward(1), 45.)),
        ("swapped, negated", angle(forward(1), forward(0), -30.)),
        ("A turned, +180", angle(back(0), forward(1), -150.)),
        ("B turned, +180", angle(forward(0), back(1), -150.)),
        ("both turned", angle(back(0), back(1), 30.)),
        ("parallel", relation(c[0], c[1], LineRelation::Parallel)),
        (
            "parallel swapped",
            relation(c[1], c[0], LineRelation::Parallel),
        ),
        (
            "perpendicular",
            relation(c[0], c[1], LineRelation::Perpendicular),
        ),
    ] {
        let message = refusal(&d, id, &edits(vec![], vec![add]));
        assert!(
            message.contains("directed angle"),
            "{name}: the refusal does not name the angle: {message}"
        );
        assert_eq!(
            stored(&d, id),
            kept,
            "{name}: a refusal changed the document"
        );
    }
    // Two different Lines, both of this Sketch.
    for (name, add) in [
        ("one Line twice", angle(forward(2), back(2), 30.)),
        (
            "a foreign Line",
            angle(
                forward(2),
                vector(StableEntityId::new(), LineEndpoint::Start),
                30.,
            ),
        ),
    ] {
        refusal(&d, id, &edits(vec![], vec![add]));
        assert_eq!(stored(&d, id), kept, "{name}");
    }
    // The relation of a *different* pair is another slot, and so is an equal
    // length on the same pair: a different property of the same two Lines.
    write(
        &mut d,
        id,
        &edits(
            vec![],
            vec![
                angle(forward(2), forward(3), 90.),
                AddSketchConstraint::Line(AddLineConstraint::EqualLength { a: c[0], b: c[1] }),
            ],
        ),
    );
    // Exact removal plus a new angle, said another way, in one request.
    let replaced = write(
        &mut d,
        id,
        &edits(vec![angle_id], vec![angle(back(1), forward(0), 150.)]),
    );
    let after = stored(&d, id);
    assert!(!after.constraints.iter().any(|k| k.id == angle_id));
    for k in kept.constraints.iter().filter(|k| k.id != angle_id) {
        assert!(after.constraints.contains(k), "{k:?} lost its UUID or rule");
    }
    assert_eq!(replaced.removed, vec![angle_id]);
    assert_eq!(
        after
            .constraints
            .iter()
            .filter(|k| matches!(k.rule, SketchConstraintRule::DirectedAngle { .. }))
            .count(),
        2
    );
    // Removing the last angles gives back the layout every older build writes.
    let ids: Vec<_> = after
        .constraints
        .iter()
        .filter(|k| matches!(k.rule, SketchConstraintRule::DirectedAngle { .. }))
        .map(|k| k.id)
        .collect();
    write(&mut d, id, &edits(ids, vec![]));

    assert!(d.validate().expect("validate").is_ok());
}
