// SPDX-License-Identifier: MIT
//! §31K: the arithmetic of picking on the accepted drawing, and what makes a drawing the
//! one a form may show. Pure: curves are the app's own `Curve`s, so none of this is
//! geometry a rebuild made (the scene and native gates in `main.rs`, `constraints` and
//! `sessions` read real presentations).
#![allow(clippy::panic)]
use super::accepted::*;
use super::navigation::View;
use ferritecad_document::{Point2, Sketch, SketchCurve, SketchGeometry};
use ferritecad_types::{ObjectId, StableEntityId};

fn rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(100., 50.), SIZE)
}

/// Two points per mm, the document origin at the canvas's lower left corner.
fn view() -> View {
    View {
        minimum: [0., 0.],
        scale: 2.,
    }
}

fn at(p: [f64; 2]) -> egui::Pos2 {
    view().screen(rect(), p)
}

fn line(start: [f64; 2], end: [f64; 2]) -> Curve {
    Curve {
        id: StableEntityId::new(),
        shape: Shape::Line { start, end },
    }
}

fn circle(center: [f64; 2], radius: f64) -> Curve {
    Curve {
        id: StableEntityId::new(),
        shape: Shape::Circle { center, radius },
    }
}

fn pick_at(curves: &[Curve], target: Target, p: egui::Pos2) -> Option<Hit> {
    pick(curves, target, &view(), rect(), rect(), p)
}

/// A closed contour (10,10) → (60,10) → (60,40) → (10,40), as four Lines.
fn contour() -> Vec<Curve> {
    vec![
        line([10., 10.], [60., 10.]),
        line([60., 10.], [60., 40.]),
        line([60., 40.], [10., 40.]),
        line([10., 40.], [10., 10.]),
    ]
}

#[test]
fn the_nearest_part_within_nine_logical_points_wins_and_the_boundary_is_exact() {
    let curves = contour();
    let mid = at([35., 10.]);
    // Exactly the pick radius from the Line: taken. A hair farther: not.
    for target in [Target::Object, Target::PairA, Target::PairB] {
        let on = pick_at(&curves, target, mid + egui::vec2(0., -9.));
        assert_eq!(
            on,
            Some(Hit {
                curve: curves[0].id,
                part: Part::Line
            }),
            "{target:?}"
        );
        assert_eq!(pick_at(&curves, target, mid + egui::vec2(0., -9.001)), None);
        assert_eq!(
            pick_at(&curves, target, mid + egui::vec2(0., 8.999)).map(|h| h.curve),
            Some(curves[0].id)
        );
        assert_eq!(pick_at(&curves, target, mid + egui::vec2(0., 9.001)), None);
    }
    // Between two Lines the nearer one: 3 points from the bottom, 6 from the (left) side
    // 40 points away is out of reach of everything else.
    let near_corner = at([10., 10.]) + egui::vec2(6., -3.);
    assert_eq!(
        pick_at(&curves, Target::Object, near_corner).map(|h| h.curve),
        Some(curves[0].id)
    );
    let near_side = at([10., 10.]) + egui::vec2(3., -6.);
    assert_eq!(
        pick_at(&curves, Target::Object, near_side).map(|h| h.curve),
        Some(curves[3].id)
    );
    // Far from everything: nothing, and nothing is made up.
    assert_eq!(pick_at(&curves, Target::Object, at([35., 25.])), None);
}

#[test]
fn an_exact_tie_goes_to_the_stored_order_and_the_order_decides_not_the_ids() {
    let curves = contour();
    // The corner (60, 10) is the End of the first Line and the Start of the second.
    let corner = at([60., 10.]);
    let first = pick_at(&curves, Target::Object, corner).expect("a hit");
    assert_eq!(first.curve, curves[0].id, "stored first");
    let end = pick_at(&curves, Target::Endpoint, corner).expect("a hit");
    assert_eq!((end.curve, end.part), (curves[0].id, Part::End));
    // The same contour stored from its second Line: that Line now ties first, and at
    // this corner it is the one whose Start is there.
    let mut turned = curves.clone();
    turned.rotate_left(1);
    let second = pick_at(&turned, Target::Endpoint, corner).expect("a hit");
    assert_eq!((second.curve, second.part), (curves[1].id, Part::Start));
    assert_eq!(
        pick_at(&turned, Target::Object, corner).map(|h| h.curve),
        Some(curves[1].id)
    );
    // The two ends of one Line at one point: Start before End.
    let closed = [line([20., 20.], [20., 20.])];
    assert_eq!(
        pick_at(&closed, Target::Endpoint, at([20., 20.])),
        None,
        "a zero Line has no ends"
    );
}

#[test]
fn an_end_is_start_or_end_by_the_stored_line_and_never_by_the_side_of_the_screen() {
    // Stored from right to left: its Start is on the right of the screen.
    let curves = [line([60., 25.], [10., 25.])];
    let (left, right) = (at([10., 25.]), at([60., 25.]));
    let hit = |p| pick_at(&curves, Target::Endpoint, p).expect("an end");
    assert_eq!((hit(left).part, hit(right).part), (Part::End, Part::Start));
    // The Line stored the other way round: the names swap with the stored ends.
    let forward = [line([10., 25.], [60., 25.])];
    let hit = |p| pick_at(&forward, Target::Endpoint, p).expect("an end");
    assert_eq!((hit(left).part, hit(right).part), (Part::Start, Part::End));
    // Away from both ends, along the Line, an Endpoint click takes nothing; Object takes it.
    let middle = at([35., 25.]);
    assert_eq!(pick_at(&curves, Target::Endpoint, middle), None);
    assert!(pick_at(&curves, Target::Object, middle).is_some());
}

#[test]
fn a_circle_is_one_curve_whose_ring_and_centre_are_told_apart() {
    let curves = [circle([35., 25.], 15.)];
    let id = curves[0].id;
    let (centre, on_ring) = (at([35., 25.]), at([50., 25.]));
    let part = |target, p| pick_at(&curves, target, p).map(|h| (h.curve, h.part));
    assert_eq!(part(Target::Object, on_ring), Some((id, Part::Ring)));
    assert_eq!(part(Target::Object, centre), Some((id, Part::Centre)));
    assert_eq!(
        part(Target::Object, centre + egui::vec2(5., 0.)),
        Some((id, Part::Centre))
    );
    assert_eq!(
        part(Target::PairA, on_ring + egui::vec2(8., 0.)),
        Some((id, Part::Ring))
    );
    assert_eq!(part(Target::PairB, on_ring + egui::vec2(9.001, 0.)), None);
    // The Endpoint target has the centre only; the circle itself is not an end.
    assert_eq!(part(Target::Endpoint, centre), Some((id, Part::Centre)));
    assert_eq!(part(Target::Endpoint, on_ring), None);
    // A tie between the ring and the centre of the same curve goes to the ring.
    let small = [circle([35., 25.], 5.)];
    let tie = at([35., 25.]) + egui::vec2(5., 0.);
    assert_eq!(
        pick_at(&small, Target::Object, tie).map(|h| h.part),
        Some(Part::Ring)
    );
}

#[test]
fn degenerate_and_non_numeric_shapes_are_never_picked() {
    let nan = f64::NAN;
    let curves = [
        line([20., 20.], [20., 20.]),
        circle([40., 20.], 0.),
        circle([40., 20.], -3.),
        line([nan, 20.], [30., 30.]),
        line([f64::INFINITY, 0.], [0., 0.]),
        circle([nan, nan], 4.),
        circle([30., 30.], nan),
    ];
    for target in [
        Target::Object,
        Target::Endpoint,
        Target::PairA,
        Target::PairB,
    ] {
        for p in [at([20., 20.]), at([40., 20.]), at([30., 30.]), at([0., 0.])] {
            assert_eq!(pick_at(&curves, target, p), None, "{target:?} {p:?}");
        }
    }
}

#[test]
fn a_part_outside_the_visible_canvas_cannot_be_picked() {
    // A Line whose End lies just beyond the visible part: the pointer inside, 6 points
    // away from it, takes the Line (its nearest point is visible) but not that End.
    let curves = [line([10., 25.], [47., 25.])];
    let visible = egui::Rect::from_min_max(rect().min, egui::pos2(100. + 90., rect().max.y));
    let end = at([47., 25.]);
    assert!(end.x > visible.right());
    let near = egui::pos2(visible.right() - 0.5, end.y);
    let got = |target, p| pick(&curves, target, &view(), rect(), visible, p);
    assert_eq!(got(Target::Endpoint, near), None);
    assert!(got(Target::Object, near).is_some());
    // The Line's nearest point itself beyond the visible edge: nothing.
    assert_eq!(
        got(Target::Object, egui::pos2(visible.right() + 3., end.y)),
        None
    );
}

fn stored(curves: Vec<(StableEntityId, SketchGeometry)>) -> Sketch {
    Sketch {
        plane: ObjectId::new(),
        curves: curves
            .into_iter()
            .map(|(id, geometry)| SketchCurve {
                id,
                construction: false,
                geometry,
            })
            .collect(),
        constraints: Vec::new(),
    }
}

fn p(x: f64, y: f64) -> Point2 {
    Point2 { x, y }
}

fn a_line(
    id: StableEntityId,
    from: (f64, f64),
    to: (f64, f64),
) -> (StableEntityId, SketchGeometry) {
    (
        id,
        SketchGeometry::Line {
            start: p(from.0, from.1),
            end: p(to.0, to.1),
        },
    )
}

fn a_circle(id: StableEntityId, at: (f64, f64), radius: f64) -> (StableEntityId, SketchGeometry) {
    (
        id,
        SketchGeometry::Circle {
            center: p(at.0, at.1),
            radius,
        },
    )
}

#[test]
fn a_drawing_is_shown_only_for_the_curves_the_form_stores_in_the_order_it_stores_them() {
    let ids: Vec<_> = (0..4).map(|_| StableEntityId::new()).collect();
    let square = |ids: &[StableEntityId]| {
        vec![
            a_line(ids[0], (0., 0.), (10., 0.)),
            a_line(ids[1], (10., 0.), (10., 10.)),
            a_line(ids[2], (10., 10.), (0., 10.)),
            a_line(ids[3], (0., 10.), (0., 0.)),
        ]
    };
    let store = stored(square(&ids));
    // The solved drawing moved every point: still the stored curves, so it is shown, with
    // the solved numbers and nothing of the stored ones.
    let solved = vec![
        a_line(ids[0], (0., 0.), (12.5, 0.)),
        a_line(ids[1], (12.5, 0.), (12.5, 8.)),
        a_line(ids[2], (12.5, 8.), (0., 8.)),
        a_line(ids[3], (0., 8.), (0., 0.)),
    ];
    let Drawing::Solved(curves) = Drawing::of(&store, solved) else {
        panic!("the solved drawing of the stored curves is shown")
    };
    assert_eq!(
        curves.iter().map(|c| c.id).collect::<Vec<_>>(),
        ids,
        "stored order and identity"
    );
    assert_eq!(
        curves[0].shape,
        Shape::Line {
            start: [0., 0.],
            end: [12.5, 0.]
        }
    );
    let differs = |presented: Vec<(StableEntityId, SketchGeometry)>| Drawing::of(&store, presented);
    let wrong = Drawing::Unavailable(Unavailable::Differs);
    // Another curve, a different order, a missing curve, an extra curve, another kind.
    let mut other = square(&ids);
    other[2].0 = StableEntityId::new();
    assert_eq!(differs(other), wrong);
    let mut swapped = square(&ids);
    swapped.swap(0, 1);
    assert_eq!(differs(swapped), wrong);
    assert_eq!(differs(square(&ids)[..3].to_vec()), wrong);
    let mut longer = square(&ids);
    longer.push(a_line(StableEntityId::new(), (0., 0.), (1., 1.)));
    assert_eq!(differs(longer), wrong);
    let mut kind = square(&ids);
    kind[3] = a_circle(ids[3], (0., 0.), 3.);
    assert_eq!(differs(kind), wrong);
}

#[test]
fn only_a_line_polygon_or_one_circle_is_a_profile_the_canvas_covers() {
    let (a, b, c) = (
        StableEntityId::new(),
        StableEntityId::new(),
        StableEntityId::new(),
    );
    // One circle: shown.
    let one = vec![a_circle(a, (12., -7.), 6.75)];
    assert!(matches!(
        Drawing::of(&stored(one.clone()), one),
        Drawing::Solved(_)
    ));
    // Two circles (an annulus), an arc, a point, a circle among Lines: not this canvas.
    let profile = Drawing::Unavailable(Unavailable::Profile);
    let two = vec![a_circle(a, (0., 0.), 10.), a_circle(b, (0., 0.), 4.)];
    assert_eq!(Drawing::of(&stored(two.clone()), two), profile);
    let arc = vec![(
        a,
        SketchGeometry::Arc {
            center: p(0., 0.),
            radius: 5.,
            start_angle: 0.,
            end_angle: 1.,
        },
    )];
    assert_eq!(Drawing::of(&stored(arc.clone()), arc), profile);
    let point = vec![(a, SketchGeometry::Point { at: p(1., 1.) })];
    assert_eq!(Drawing::of(&stored(point.clone()), point), profile);
    let mixed = vec![
        a_line(a, (0., 0.), (5., 0.)),
        a_circle(b, (0., 0.), 3.),
        a_line(c, (5., 0.), (0., 5.)),
    ];
    assert_eq!(Drawing::of(&stored(mixed.clone()), mixed), profile);
    let nothing: Vec<(StableEntityId, SketchGeometry)> = Vec::new();
    assert_eq!(Drawing::of(&stored(nothing.clone()), nothing), profile);
}

#[test]
fn a_coordinate_that_cannot_be_drawn_keeps_the_whole_drawing_away() {
    let (a, b) = (StableEntityId::new(), StableEntityId::new());
    for bad in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1.000_000_1e6,
        -2e9,
    ] {
        let polygon = vec![
            a_line(a, (0., 0.), (5., 0.)),
            a_line(b, (5., 0.), (bad, 5.)),
        ];
        assert_eq!(
            Drawing::of(&stored(polygon.clone()), polygon),
            Drawing::Unavailable(Unavailable::NotDrawable),
            "{bad}"
        );
        let round = vec![a_circle(a, (0., bad), 5.)];
        assert_eq!(
            Drawing::of(&stored(round.clone()), round),
            Drawing::Unavailable(Unavailable::NotDrawable),
            "{bad}"
        );
    }
    // The drawable bound itself is drawable; a zero Line is drawn as nothing and kept.
    let edge = vec![
        a_line(a, (0., 0.), (1e6, 0.)),
        a_line(b, (3., 3.), (3., 3.)),
    ];
    assert!(matches!(
        Drawing::of(&stored(edge.clone()), edge),
        Drawing::Solved(_)
    ));
}

#[test]
fn fit_shows_every_live_curve_inside_the_canvas_with_finite_numbers() {
    let inside = |v: &View, curves: &[Curve]| {
        let limit = rect().shrink2(egui::vec2(13.9, 21.9));
        curves
            .iter()
            .filter(|c| {
                c.shape
                    != Shape::Line {
                        start: [500., 500.],
                        end: [500., 500.],
                    }
            })
            .all(|c| match c.shape {
                Shape::Line { start, end } => {
                    limit.contains(v.screen(rect(), start)) && limit.contains(v.screen(rect(), end))
                }
                Shape::Circle { center, radius } => {
                    let c = v.screen(rect(), center);
                    let r = (radius * f64::from(v.scale)) as f32;
                    limit.contains(c + egui::vec2(r, 0.)) && limit.contains(c - egui::vec2(r, 0.))
                }
            })
    };
    for curves in [
        contour(),
        vec![circle([12., -7.], 6.75)],
        vec![line([0., 0.], [1e-4, 0.]), line([1e-4, 0.], [1e-4, 1e-4])],
        vec![line([-9e5, -9e5], [9e5, 9e5])],
        // A Line of zero length beside a live one is no part of the box.
        vec![line([500., 500.], [500., 500.]), line([0., 0.], [5., 5.])],
    ] {
        let v = fitted(&curves).expect("a view");
        assert!(v.scale.is_finite() && v.scale > 0.);
        assert!(v.minimum.iter().all(|m| m.is_finite()));
        assert!(inside(&v, &curves), "{curves:?} in {v:?}");
    }
    // Nothing live, nothing to fit.
    assert!(fitted(&[]).is_none());
    assert!(fitted(&[line([1., 1.], [1., 1.]), circle([0., 0.], 0.)]).is_none());
}
