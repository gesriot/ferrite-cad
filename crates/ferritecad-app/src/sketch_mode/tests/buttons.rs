// SPDX-License-Identifier: MIT
use super::super::rules::{Button as B, Selection as S};
use super::*;
use ferritecad_document::{
    Expression, SketchConstraintRule as R, SketchPointSelector as P, SketchSegmentRef,
};

#[test]
fn each_button_requires_its_exact_selection_and_a_solver() {
    let first = line([0., 0.], [3., 4.]);
    let second = line([5., 7.], [9., 4.]);
    let circle = curve(SketchGeometry::Circle {
        center: point(0., 0.),
        radius: 3.,
    });
    let curves = [first.clone(), second.clone(), circle.clone()];
    let a = SketchPointRef::new(first.id, P::Start);
    let b = SketchPointRef::new(first.id, P::End);
    let segment = |id| {
        SketchSegmentRef::new(
            SketchPointRef::new(id, P::Start),
            SketchPointRef::new(id, P::End),
        )
    };
    let one = S::Curve(first.id);
    let two = S::Curve(second.id);
    let round = S::Curve(circle.id);
    let pa = S::Point(a);
    let pb = S::Point(b);
    let rule = S::Constraint(StableEntityId::new());
    let cases: &[&[S]] = &[
        &[],
        &[one],
        &[one, two],
        &[pa],
        &[pa, pb],
        &[round],
        &[rule],
        &[one, pa],
        &[pa, pb, one],
        &[round, two],
    ];
    let expr = |v| Expression::constant(v).expect("fixture operation");
    for s in cases {
        for button in B::ALL {
            assert_eq!(
                button.rule(&curves, s, false),
                Err("This build has no sketch solver, so constraints cannot be added.".into())
            );
            let pair = (*s == [pa, pb]) || (*s == [one]);
            let lines = *s == [one, two];
            let (sa, sb) = (segment(first.id), segment(second.id));
            let expected = match button {
                B::Coincident if *s == [pa, pb] => Ok(R::Coincident { a, b }),
                B::Horizontal if pair => Ok(R::Horizontal { a, b }),
                B::Vertical if pair => Ok(R::Vertical { a, b }),
                B::Distance if pair => Ok(R::Distance {
                    a,
                    b,
                    distance: expr(5.),
                }),
                B::Fix if *s == [pa] => Ok(R::Fixed {
                    point: a,
                    x: expr(0.),
                    y: expr(0.),
                }),
                B::Parallel if lines => Ok(R::Parallel { a: sa, b: sb }),
                B::Perpendicular if lines => Ok(R::Perpendicular { a: sa, b: sb }),
                B::Collinear if lines => Ok(R::Collinear { a: sa, b: sb }),
                B::Equal if lines => Ok(R::EqualLength { a: sa, b: sb }),
                B::Angle if lines => Ok(R::DirectedAngle {
                    a: sa,
                    b: sb,
                    angle: expr(-std::f64::consts::FRAC_PI_2),
                }),
                B::Radius | B::Diameter if *s == [round] => Ok(R::Radius {
                    curve: circle.id,
                    radius: expr(3.),
                }),
                B::Coincident => Err("Select two points."),
                B::Horizontal | B::Vertical | B::Distance => Err("Select two points or one line."),
                B::Fix => Err("Select one point."),
                B::Parallel | B::Perpendicular | B::Collinear | B::Equal | B::Angle => {
                    Err("Select two lines.")
                }
                B::Radius | B::Diameter => Err("Select one circle."),
            };
            assert_eq!(
                button.rule(&curves, s, true),
                expected.map_err(str::to_owned),
                "{button:?} {s:?}"
            );
        }
    }
}
#[test]
fn opposite_lines_measure_positive_pi_even_when_the_first_line_is_selected_last() {
    let a = line([4., 2.], [1., 2.]);
    let b = line([1., 3.], [4., 3.]);
    for pair in [[a.id, b.id], [b.id, a.id]] {
        let constraint = B::Angle
            .rule(&[a.clone(), b.clone()], &pair.map(S::Curve), true)
            .expect("fixture operation");
        let R::DirectedAngle {
            angle,
            a: sa,
            b: sb,
        } = constraint
        else {
            unreachable!()
        };
        assert_eq!(angle.value(), std::f64::consts::PI);
        assert_eq!([sa.from.curve, sb.from.curve], pair);
    }
}

#[test]
fn line_end_and_circle_centre_take_precedence_over_projected_curves() {
    let plane = SketchPlane::world_xy();
    let camera = camera(plane, Projection::Orthographic);
    let side = line([0., 0.], [10., 0.]);
    let circle = curve(SketchGeometry::Circle {
        center: point(5., 0.),
        radius: 5.,
    });
    for (at, want) in [
        (
            point(0., 0.),
            S::Point(SketchPointRef::new(side.id, P::Start)),
        ),
        (
            point(10., 0.),
            S::Point(SketchPointRef::new(side.id, P::End)),
        ),
        (
            point(5., 0.),
            S::Point(SketchPointRef::new(circle.id, P::Center)),
        ),
        (point(5., 5.), S::Curve(circle.id)),
    ] {
        let at = geometry::screen(&camera, plane, at).expect("fixture operation");
        assert_eq!(
            geometry::selection(
                &[side.clone(), circle.clone()],
                &camera,
                plane,
                at,
                &[],
                false,
                &[]
            ),
            Some(want)
        );
    }
}
