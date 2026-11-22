// SPDX-License-Identifier: MIT
//! Drag document points through the same translation used by a rebuild.
use ferritecad_document::{
    Point2, Sketch, SketchConstraint, SketchConstraintRule as Rule, SketchCurve, SketchGeometry,
    SketchPointRef, SketchPointSelector,
};
use ferritecad_eval::{SketchDrag, solve_sketch};
use ferritecad_types::{ObjectId, StableEntityId};
use std::time::Instant;

fn point(x: f64, y: f64) -> Point2 {
    Point2::new(x, y).expect("finite fixture coordinates")
}

fn rectangles(count: usize) -> Sketch {
    let mut s = Sketch {
        plane: ObjectId::new(),
        curves: vec![],
        constraints: vec![],
    };
    for i in 0..count {
        let x = 20. * i as f64;
        let corners = [
            point(x, 0.),
            point(x + 10., 0.),
            point(x + 10., 5.),
            point(x, 5.),
        ];
        let sides = corners.map(|_| StableEntityId::new());
        for side in 0..4 {
            let id = sides[side];
            let next = (side + 1) % 4;
            s.curves.push(SketchCurve {
                id,
                construction: false,
                geometry: SketchGeometry::Line {
                    start: corners[side],
                    end: corners[next],
                },
            });
            let a = SketchPointRef::new(id, SketchPointSelector::Start);
            let b = SketchPointRef::new(id, SketchPointSelector::End);
            let next_start = SketchPointRef::new(sides[next], SketchPointSelector::Start);
            let alignment = if side % 2 == 0 {
                Rule::Horizontal { a, b }
            } else {
                Rule::Vertical { a, b }
            };
            for rule in [
                alignment,
                Rule::Coincident {
                    a: b,
                    b: next_start,
                },
            ] {
                s.constraints.push(SketchConstraint {
                    id: StableEntityId::new(),
                    rule,
                });
            }
        }
    }
    s
}

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(ferritecad_eval::solver_available(), "PlaneGCS")])
}

fn ends(curve: &SketchCurve) -> [Point2; 2] {
    let SketchGeometry::Line { start, end } = curve.geometry else {
        unreachable!()
    };
    [start, end]
}

fn near(a: Point2, b: Point2, limit: f64) {
    assert!(
        (a.x - b.x).abs() <= limit && (a.y - b.y).abs() <= limit,
        "{a:?} and {b:?}"
    );
}

#[test]
fn dragging_a_rectangle_corner_keeps_every_side_connected_and_stable_on_rebuild() {
    if !native() {
        return;
    }
    let s = rectangles(1);
    let id = ObjectId::new();
    let p = SketchPointRef::new(s.curves[0].id, SketchPointSelector::Start);
    let mut drag = SketchDrag::begin(s, id, p).expect("start the gesture");
    for target in [point(2., 3.), point(-4., -1.)] {
        let shown = drag.move_to(target).expect("solved sample");
        near(ends(&shown.curves[0])[0], target, 1e-6);
        let (rebuilt, _) = solve_sketch(&shown, id)
            .expect("solve shown drawing")
            .expect("eight rules");
        for side in 0..4 {
            let [a, b] = ends(&shown.curves[side]);
            let (v, w) = if side % 2 == 0 {
                (a.y, b.y)
            } else {
                (a.x, b.x)
            };
            assert!((v - w).abs() <= 1e-9);
            near(b, ends(&shown.curves[(side + 1) % 4])[0], 1e-9);
            for (before, after) in [a, b].into_iter().zip(ends(&rebuilt.curves[side])) {
                near(before, after, 1e-9);
            }
        }
    }
}

#[test]
fn dragging_without_the_solver_reports_the_same_missing_component_as_rebuild() {
    if ferritecad_eval::solver_available() {
        return;
    }
    let s = rectangles(1);
    let id = ObjectId::new();
    let p = SketchPointRef::new(s.curves[0].id, SketchPointSelector::Start);
    let expected = solve_sketch(&s, id).expect_err("missing solver");
    let actual = SketchDrag::begin(s, id, p).expect_err("missing solver");
    assert_eq!(actual.to_string(), expected.to_string());
}

#[test]
fn measure_one_hundred_drag_samples_on_twelve_rectangles() {
    if !native() {
        return;
    }
    let s = rectangles(12);
    let p = SketchPointRef::new(s.curves[0].id, SketchPointSelector::Start);
    let mut drag = SketchDrag::begin(s, ObjectId::new(), p).expect("start the gesture");
    let mut samples = Vec::new();
    for step in 1..=100 {
        let target = point(f64::from(step) / 100., -f64::from(step) / 50.);
        let start = Instant::now();
        let shown = drag.move_to(target).expect("solved sample");
        samples.push(start.elapsed());
        near(ends(&shown.curves[0])[0], target, 1e-6);
    }
    samples.sort_unstable();
    let median = (samples[49] + samples[50]) / 2;
    eprintln!(
        "100 samples on 48 lines: median {median:?}, maximum {:?}",
        samples[99]
    );
}

#[test]
fn dragging_a_fixed_document_point_keeps_its_coordinates() {
    if !native() {
        return;
    }
    let mut s = rectangles(1);
    let id = ObjectId::new();
    let p = SketchPointRef::new(s.curves[0].id, SketchPointSelector::Start);
    s.constraints.push(SketchConstraint {
        id: StableEntityId::new(),
        rule: Rule::Fixed {
            point: p,
            x: ferritecad_document::Expression::constant(0.).expect("x"),
            y: ferritecad_document::Expression::constant(0.).expect("y"),
        },
    });
    let mut drag = SketchDrag::begin(s, id, p).expect("fixed corner accepts a soft target");
    let shown = drag.move_to(point(14., 3.)).expect("solved fixed corner");
    near(ends(&shown.curves[0])[0], point(0., 0.), 1e-6);
}
