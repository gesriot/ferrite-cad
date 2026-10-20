// SPDX-License-Identifier: MIT
//! §31L: which saved rules are dimensions, what a label says, where it stands and what a
//! pointer reaches. Pure: curves are the app's own `Curve`s and the stored Sketch is built
//! here, so none of this is geometry a rebuild made (the form, scene and native gates read
//! real drawings).
#![allow(clippy::panic)]
use super::accepted::{Curve, SIZE, Shape};
use super::dimension::*;
use super::navigation::View;
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, CircleConstraintKind, CircleRadiusMm,
    LineConstraintKind, LineLengthMm, Point2, Sketch, SketchConstraint, SketchConstraintEdits,
    SketchConstraintRule, SketchCurve, SketchGeometry, SketchPointRef, SketchPointSelector,
};
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

/// Six points a character and eleven high: the measure of a label's words.
fn measure(words: &str) -> egui::Vec2 {
    egui::vec2(6. * words.chars().count() as f32, 11.)
}

fn line(start: [f64; 2], end: [f64; 2]) -> Curve {
    Curve {
        id: StableEntityId::new(),
        shape: Shape::Line { start, end },
    }
}

fn point(at: SketchPointSelector, curve: StableEntityId) -> SketchPointRef {
    SketchPointRef::new(curve, at)
}

fn distance(
    curve: StableEntityId,
    ends: (SketchPointSelector, SketchPointSelector),
    mm: f64,
) -> SketchConstraint {
    SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Distance {
            a: point(ends.0, curve),
            b: point(ends.1, curve),
            distance: mm,
        },
    }
}

fn radius(curve: StableEntityId, mm: f64) -> SketchConstraint {
    SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Radius { curve, radius: mm },
    }
}

fn sketch(geometries: Vec<SketchGeometry>, constraints: Vec<SketchConstraint>) -> Sketch {
    Sketch {
        plane: ObjectId::new(),
        curves: geometries
            .into_iter()
            .map(|geometry| SketchCurve {
                id: StableEntityId::new(),
                construction: false,
                geometry,
            })
            .collect(),
        constraints,
    }
}

fn a_line(start: (f64, f64), end: (f64, f64)) -> SketchGeometry {
    SketchGeometry::Line {
        start: Point2 {
            x: start.0,
            y: start.1,
        },
        end: Point2 { x: end.0, y: end.1 },
    }
}

fn a_circle(at: (f64, f64), radius: f64) -> SketchGeometry {
    SketchGeometry::Circle {
        center: Point2 { x: at.0, y: at.1 },
        radius,
    }
}

const START_END: (SketchPointSelector, SketchPointSelector) =
    (SketchPointSelector::Start, SketchPointSelector::End);
const END_START: (SketchPointSelector, SketchPointSelector) =
    (SketchPointSelector::End, SketchPointSelector::Start);

fn dimension(curve: StableEntityId, kind: Kind, mm: f64) -> Dimension {
    Dimension {
        id: StableEntityId::new(),
        curve,
        kind,
        target_mm: mm,
    }
}

fn shown(dimension: Dimension, pending: Option<Pending>) -> Shown {
    Shown { dimension, pending }
}

#[test]
fn saved_dimensions_are_the_whole_line_lengths_and_the_one_circle_radius_in_curve_order() {
    let mut square = sketch(
        vec![
            a_line((0., 0.), (10., 0.)),
            a_line((10., 0.), (10., 10.)),
            a_line((10., 10.), (0., 10.)),
            a_line((0., 10.), (0., 0.)),
        ],
        Vec::new(),
    );
    let ids: Vec<_> = square.curves.iter().map(|c| c.id).collect();
    // A rule per Line in the opposite order of the curves, one of them stored End to Start.
    let late = distance(ids[3], START_END, 42.5);
    let reversed = distance(ids[1], END_START, 30.);
    let first = distance(ids[0], START_END, 60.);
    // A Distance between two different Lines' ends, one that is not Start and End of a Line,
    // a Horizontal and a Distance whose number is not a length: none is a dimension.
    let across = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Distance {
            a: point(SketchPointSelector::Start, ids[0]),
            b: point(SketchPointSelector::End, ids[2]),
            distance: 5.,
        },
    };
    let same_end = distance(
        ids[2],
        (SketchPointSelector::Start, SketchPointSelector::Start),
        7.,
    );
    let horizontal = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Horizontal {
            a: point(SketchPointSelector::Start, ids[0]),
            b: point(SketchPointSelector::End, ids[0]),
        },
    };
    square.constraints = vec![late, across, reversed, same_end, horizontal, first];
    let found = saved(&square);
    assert_eq!(
        found,
        vec![
            Dimension {
                id: first.id,
                curve: ids[0],
                kind: Kind::Length,
                target_mm: 60.
            },
            Dimension {
                id: reversed.id,
                curve: ids[1],
                kind: Kind::Length,
                target_mm: 30.
            },
            Dimension {
                id: late.id,
                curve: ids[3],
                kind: Kind::Length,
                target_mm: 42.5
            },
        ],
        "the order of the curves, not of the rules or of their UUIDs"
    );
    assert_eq!(found[0].name(), "Line length 60 mm");
    assert_eq!(found[2].name(), "Line length 42.5 mm");

    // A number that is not a length is no dimension, however the document came by it.
    for bad in [0., -3., f64::NAN, f64::INFINITY] {
        let mut broken = square.clone();
        broken.constraints = vec![distance(ids[0], START_END, bad)];
        assert!(saved(&broken).is_empty(), "{bad}");
    }

    // One Circle: its Radius, by the rule's own UUID.
    let mut round = sketch(vec![a_circle((4.5, 0.), 1.2)], Vec::new());
    let circle_id = round.curves[0].id;
    let rule = radius(circle_id, 10.);
    round.constraints = vec![
        SketchConstraint {
            id: StableEntityId::new(),
            rule: SketchConstraintRule::Fixed {
                point: point(SketchPointSelector::Center, circle_id),
                x: 0.,
                y: 0.,
            },
        },
        rule,
    ];
    assert_eq!(
        saved(&round),
        vec![Dimension {
            id: rule.id,
            curve: circle_id,
            kind: Kind::Radius,
            target_mm: 10.
        }]
    );
    assert_eq!(saved(&round)[0].name(), "Radius 10 mm");
    for bad in [0., -1., f64::NAN] {
        round.constraints = vec![radius(circle_id, bad)];
        assert!(saved(&round).is_empty(), "{bad}");
    }

    // An annulus and a circle among Lines have no dimension on this canvas; a Radius on a
    // Line polygon and a Line length on a Circle are not dimensions either.
    let mut two = sketch(
        vec![a_circle((0., 0.), 10.), a_circle((0., 0.), 4.)],
        Vec::new(),
    );
    two.constraints = vec![radius(two.curves[0].id, 10.), radius(two.curves[1].id, 4.)];
    assert!(saved(&two).is_empty(), "an annulus");
    let mut mixed = sketch(
        vec![a_line((0., 0.), (5., 0.)), a_circle((0., 0.), 3.)],
        Vec::new(),
    );
    mixed.constraints = vec![
        distance(mixed.curves[0].id, START_END, 5.),
        radius(mixed.curves[1].id, 3.),
    ];
    assert!(saved(&mixed).is_empty(), "a mixed profile");
    square.constraints = vec![radius(ids[0], 5.)];
    assert!(saved(&square).is_empty());
    round.constraints = vec![distance(circle_id, START_END, 5.)];
    assert!(saved(&round).is_empty());
}

#[test]
fn a_label_states_the_target_and_beside_it_only_what_is_prepared_in_its_place() {
    let line_id = StableEntityId::new();
    let length = dimension(line_id, Kind::Length, 60.);
    let round = dimension(line_id, Kind::Radius, 10.);
    assert_eq!(shown(length, None).text(), "60 mm");
    assert_eq!(shown(round, None).text(), "R 10 mm");
    assert_eq!(
        shown(length, Some(Pending::Replaced(70.))).text(),
        "60 → 70 mm"
    );
    assert_eq!(
        shown(round, Some(Pending::Replaced(8.5))).text(),
        "R 10 → 8.5 mm"
    );
    assert_eq!(
        shown(length, Some(Pending::Removed)).text(),
        "60 mm → removed"
    );
    // The target is the saved number whatever is pending; the pending number never replaces it.
    assert!(
        shown(length, Some(Pending::Replaced(70.)))
            .text()
            .starts_with("60 ")
    );
    // Long numbers are shortened with a mark that digits are missing, never rounded silently.
    assert_eq!(
        shown(dimension(line_id, Kind::Length, 12.345_678_9), None).text(),
        "12.3457… mm"
    );
    assert_eq!(
        shown(dimension(line_id, Kind::Length, 0.1 + 0.2), None).text(),
        "0.3… mm"
    );
    assert_eq!(
        shown(dimension(line_id, Kind::Length, 1e21), None).text(),
        "1.000e21 mm"
    );
    assert_eq!(
        shown(dimension(line_id, Kind::Length, 1e-9), None).text(),
        "1.000e-9 mm"
    );
    assert_eq!(
        shown(dimension(line_id, Kind::Length, 123_456_789.), None).text(),
        "123456789 mm"
    );

    // What the pending request says of a dimension is read by the UUID of its rule.
    let other = StableEntityId::new();
    let add = |curve, mm| {
        AddSketchConstraint::Line(AddLineConstraint::Line {
            curve,
            kind: LineConstraintKind::Distance(LineLengthMm::new(mm).expect("length")),
        })
    };
    let nothing = SketchConstraintEdits::default();
    assert_eq!(pending(&nothing, &length), None);
    let removed = SketchConstraintEdits {
        remove: vec![length.id],
        add: Vec::new(),
    };
    assert_eq!(pending(&removed, &length), Some(Pending::Removed));
    let replaced = SketchConstraintEdits {
        remove: vec![length.id],
        add: vec![add(other, 5.), add(line_id, 70.)],
    };
    assert_eq!(pending(&replaced, &length), Some(Pending::Replaced(70.)));
    // Another dimension's removal, another Line's addition, an addition without the removal and
    // a radius are not this dimension's replacement.
    assert_eq!(
        pending(
            &SketchConstraintEdits {
                remove: vec![other],
                add: vec![add(line_id, 70.)]
            },
            &length
        ),
        None
    );
    assert_eq!(
        pending(
            &SketchConstraintEdits {
                remove: vec![length.id],
                add: vec![add(other, 70.)]
            },
            &length
        ),
        Some(Pending::Removed)
    );
    assert_eq!(
        pending(
            &SketchConstraintEdits {
                remove: Vec::new(),
                add: vec![add(line_id, 70.)]
            },
            &length
        ),
        None
    );
    let radius_add = AddSketchConstraint::Circle {
        curve: line_id,
        kind: CircleConstraintKind::Radius(CircleRadiusMm::new(8.5).expect("radius")),
    };
    assert_eq!(
        pending(
            &SketchConstraintEdits {
                remove: vec![length.id],
                add: vec![radius_add]
            },
            &length
        ),
        Some(Pending::Removed),
        "a radius is not a length's replacement"
    );
    assert_eq!(
        pending(
            &SketchConstraintEdits {
                remove: vec![round.id],
                add: vec![radius_add]
            },
            &round
        ),
        Some(Pending::Replaced(8.5))
    );
}

/// The box of the label of `words` for `shape` in the view and canvas of these tests.
fn stand(shape: Shape, words: &str) -> Option<egui::Rect> {
    place(&shape, &view(), rect(), measure(words) + PAD * 2.)
}

#[test]
fn a_label_stands_beside_its_curve_whichever_way_it_is_stored_and_inside_the_canvas() {
    let words = "60 mm";
    let size = measure(words) + PAD * 2.;
    let line_shape = |start, end| Shape::Line { start, end };
    // A horizontal Line at 30 mm (102 points down from the top of the canvas' 162): the label
    // stands on its upper side, centred on the middle, a gap above it.
    let ends = ([10., 30.], [60., 30.]);
    let above = stand(line_shape(ends.0, ends.1), words).expect("on the canvas");
    assert_eq!(above.size(), size);
    assert_eq!(above.center().x, 170.);
    assert_eq!(
        above.bottom(),
        102. - 3.,
        "a gap of three points above the Line"
    );
    // Stored the other way round: the same box.
    assert_eq!(stand(line_shape(ends.1, ends.0), words), Some(above));
    // A vertical Line: to the right of it, whichever way it runs.
    let up = stand(line_shape([60., 10.], [60., 40.]), words).expect("on the canvas");
    let down = stand(line_shape([60., 40.], [60., 10.]), words).expect("on the canvas");
    assert_eq!(up, down);
    assert_eq!(up.left(), 220. + 3.);
    assert_eq!(up.center().y, 112.);
    // A slanted Line: the upper side, in both directions.
    let slanted = stand(line_shape([10., 10.], [60., 40.]), words).expect("on the canvas");
    assert_eq!(
        stand(line_shape([60., 40.], [10., 10.]), words),
        Some(slanted)
    );
    let middle = view().screen(rect(), [35., 25.]);
    assert!(slanted.center().y < middle.y && slanted.center().x < middle.x);
    // A Circle: outside its ring, toward the upper right.
    let ring = stand(
        Shape::Circle {
            center: [35., 25.],
            radius: 10.,
        },
        "R 10 mm",
    )
    .expect("on the canvas");
    let centre = view().screen(rect(), [35., 25.]);
    assert!(ring.left() > centre.x && ring.bottom() < centre.y);
    let corner = ring.left_bottom() - centre;
    assert!(
        corner.length() > 20.,
        "the box is outside the ring of 20 points: {corner:?}"
    );

    // A label never covers the words on the canvas's edges: it slides into the band they leave.
    let band = egui::Rect::from_min_max(
        rect().min + egui::vec2(3., 20.),
        rect().max - egui::vec2(3., 24.),
    );
    let high = stand(line_shape([10., 55.], [60., 55.]), words).expect("on the canvas");
    assert_eq!(high.top(), band.top(), "slid down from the top words");
    let low = stand(line_shape([10., 0.], [60., 0.]), words).expect("on the canvas");
    assert!(band.contains_rect(low), "{low:?} in {band:?}");
    let left = stand(line_shape([0., 30.], [2., 30.]), words).expect("on the canvas");
    assert!(band.contains_rect(left), "{left:?} in {band:?}");
    let right = stand(line_shape([126., 30.], [133., 30.]), words).expect("on the canvas");
    assert!(band.contains_rect(right), "{right:?} in {band:?}");
    // A box larger than the band is aligned to its start.
    let wide = place(
        &line_shape(ends.0, ends.1),
        &view(),
        rect(),
        egui::vec2(400., 15.),
    )
    .expect("on the canvas");
    assert_eq!(wide.left(), band.left());

    // Wherever the view is, a label that is placed is finite, inside the band and the size it
    // was asked for.
    for scale in [1e-5_f32, 0.01, 0.5, 2., 37., 1e3, 1e6, 1e8] {
        for dx in [-1e9_f64, -400., -3., 0., 1., 60., 5e3] {
            for dy in [-1e9_f64, -50., 0., 12., 90., 1e4] {
                let there = View {
                    minimum: [dx, dy],
                    scale,
                };
                for shape in [
                    line_shape([10., 30.], [60., 30.]),
                    line_shape([0., 0.], [0., 1e5]),
                    Shape::Circle {
                        center: [35., 25.],
                        radius: 10.,
                    },
                ] {
                    if let Some(found) = place(&shape, &there, rect(), size) {
                        assert!(
                            found.is_finite() && band.contains_rect(found) && found.size() == size,
                            "{found:?} for {shape:?} in {there:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_label_is_none_when_its_curve_cannot_be_drawn_or_its_anchor_is_off_the_canvas() {
    let words = "60 mm";
    let line_shape = |start, end| Shape::Line { start, end };
    // The anchor is the middle of a Line and the point of the ring toward the upper right: off
    // the canvas, no label (the list is the way to it), and never one clamped onto the edge.
    assert!(stand(line_shape([-500., 30.], [-400., 30.]), words).is_none());
    assert!(stand(line_shape([10., 900.], [60., 900.]), words).is_none());
    assert!(
        stand(
            Shape::Circle {
                center: [35., 25.],
                radius: 1e5
            },
            "R 1e5 mm"
        )
        .is_none()
    );
    // A Line that is long but whose middle is on the canvas has its label.
    assert!(stand(line_shape([-4000., 30.], [4070., 30.]), words).is_some());
    // Shapes a pointer cannot be near have no label: zero Line, non-positive and non-finite
    // numbers.
    for shape in [
        line_shape([5., 5.], [5., 5.]),
        line_shape([f64::NAN, 5.], [9., 5.]),
        line_shape([0., 5.], [f64::INFINITY, 5.]),
        Shape::Circle {
            center: [0., 0.],
            radius: 0.,
        },
        Shape::Circle {
            center: [0., 0.],
            radius: -3.,
        },
        Shape::Circle {
            center: [f64::NAN, 0.],
            radius: 3.,
        },
        Shape::Circle {
            center: [35., 25.],
            radius: f64::MAX,
        },
    ] {
        assert!(stand(shape, words).is_none(), "{shape:?}");
    }
    assert!(
        place(
            &line_shape([10., 30.], [60., 30.]),
            &view(),
            rect(),
            egui::vec2(f32::NAN, 10.)
        )
        .is_none()
    );
    // A Line whose two ends are one point on the screen (far out) still has a label, above it.
    let tiny = View {
        minimum: [0., 0.],
        scale: 1e-5,
    };
    let dot = place(
        &line_shape([0., 0.], [1., 1.]),
        &tiny,
        rect(),
        egui::vec2(38., 15.),
    )
    .expect("the origin is the canvas's corner");
    assert!(dot.is_finite());
}

#[test]
fn labels_that_overlap_are_reached_in_stored_order_and_a_label_is_exactly_its_box() {
    // Two parallel Lines four points apart (two points per mm): their labels overlap.
    let a = line([10., 30.], [60., 30.]);
    let b = line([10., 32.], [60., 32.]);
    let curves = [a.clone(), b.clone()];
    // The UUIDs of the two rules, in either order against the order of the curves: the order
    // of the curves decides, not the UUIDs and not the hash of anything.
    let (low, high) = {
        let (x, y) = (StableEntityId::new(), StableEntityId::new());
        if x < y { (x, y) } else { (y, x) }
    };
    for (first, second) in [(low, high), (high, low)] {
        let one = Dimension {
            id: first,
            curve: a.id,
            kind: Kind::Length,
            target_mm: 50.,
        };
        let two = Dimension {
            id: second,
            curve: b.id,
            kind: Kind::Length,
            target_mm: 51.,
        };
        let all = [shown(one, None), shown(two, None)];
        let there = placed(&all, &curves, &view(), rect(), &measure);
        assert_eq!(there.len(), 2);
        assert!(
            there[0].rect.intersects(there[1].rect),
            "the fixture overlaps"
        );
        let both = there[0].rect.intersect(there[1].rect).center();
        let hit = pick(&there, rect(), both).expect("a label");
        assert_eq!(
            hit.shown.dimension.id, first,
            "the first stored curve's label wins in the overlap"
        );
        // Outside the overlap each box is reached by its own label.
        let only_two = egui::pos2(there[1].rect.center().x, there[1].rect.top() + 1.);
        assert!(!there[0].rect.contains(only_two));
        assert_eq!(
            pick(&there, rect(), only_two).map(|p| p.shown.dimension.id),
            Some(second)
        );
        // The same labels listed the other way round: now the other wins. The list decides.
        let swapped = [all[1], all[0]];
        let there = placed(&swapped, &curves, &view(), rect(), &measure);
        let hit = pick(&there, rect(), both).expect("a label");
        assert_eq!(hit.shown.dimension.id, second);
    }

    // A label is exactly its box: the edges are in, a hair outside is not, and the 9-point
    // radius of the curves plays no part.
    let one = dimension(a.id, Kind::Length, 50.);
    let there = placed(&[shown(one, None)], &curves, &view(), rect(), &measure);
    let r = there[0].rect;
    for inside in [r.min, r.max, r.center(), r.left_bottom(), r.right_top()] {
        assert!(pick(&there, rect(), inside).is_some(), "{inside:?}");
    }
    for outside in [
        r.min - egui::vec2(0.001, 0.),
        r.min - egui::vec2(0., 0.001),
        r.max + egui::vec2(0.001, 0.),
        r.max + egui::vec2(0., 0.001),
        r.center() + egui::vec2(0., r.height()),
        view().screen(rect(), [35., 30.]),
    ] {
        assert!(pick(&there, rect(), outside).is_none(), "{outside:?}");
    }
    // The point must be on the visible canvas as well: a box half under another layer's clip
    // is reached only where it shows.
    let clip = egui::Rect::from_min_max(r.min, egui::pos2(r.center().x, r.max.y));
    assert!(pick(&there, clip, r.center() - egui::vec2(1., 0.)).is_some());
    assert!(pick(&there, clip, r.center() + egui::vec2(1., 0.)).is_none());
    // A dimension whose curve is not in the drawing has no box and no label.
    let lost = dimension(StableEntityId::new(), Kind::Length, 1.);
    assert!(placed(&[shown(lost, None)], &curves, &view(), rect(), &measure).is_empty());
}
