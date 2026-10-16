// SPDX-License-Identifier: MIT
//! §31N: which saved rules are markers, what a marker says, where it stands and what a pointer reaches
//! among them. Pure: curves are the app's own `Curve`s and the stored Sketch is built here, so none of
//! this is geometry a rebuild made (the form, tab and native gates read real drawings).
#![allow(clippy::panic)]
use super::accepted::{Curve, SIZE, Shape};
use super::dimension::{self, band};
use super::marker::*;
use super::navigation::View;
use ferritecad_document::{
    Point2, Sketch, SketchConstraint, SketchConstraintEdits, SketchConstraintRule, SketchCurve,
    SketchGeometry, SketchPointRef, SketchPointSelector as At, SketchSegmentRef,
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

/// Six points a character and eleven high: the measure of a marker's words.
fn measure(words: &str) -> egui::Vec2 {
    egui::vec2(6. * words.chars().count() as f32, 11.)
}

fn at(curve: StableEntityId, at: At) -> SketchPointRef {
    SketchPointRef::new(curve, at)
}

fn seg(curve: StableEntityId, from: At, to: At) -> SketchSegmentRef {
    SketchSegmentRef::new(at(curve, from), at(curve, to))
}

fn rule(rule: SketchConstraintRule) -> SketchConstraint {
    SketchConstraint {
        id: StableEntityId::new(),
        rule,
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

fn a_circle(x: f64, y: f64, radius: f64) -> SketchGeometry {
    SketchGeometry::Circle {
        center: Point2 { x, y },
        radius,
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

fn square(constraints: impl FnOnce(&[StableEntityId]) -> Vec<SketchConstraint>) -> Sketch {
    let mut s = sketch(
        vec![
            a_line((0., 0.), (10., 0.)),
            a_line((10., 0.), (10., 10.)),
            a_line((10., 10.), (0., 10.)),
            a_line((0., 10.), (0., 0.)),
        ],
        Vec::new(),
    );
    let ids: Vec<_> = s.curves.iter().map(|c| c.id).collect();
    s.constraints = constraints(&ids);
    s
}

fn line(id: StableEntityId, start: [f64; 2], end: [f64; 2]) -> Curve {
    Curve {
        id,
        shape: Shape::Line { start, end },
    }
}

fn mark(id: StableEntityId, kind: Kind, refs: Refs) -> Mark {
    Mark { id, kind, refs }
}

#[test]
fn saved_markers_are_the_seven_families_with_their_exact_references_in_rule_order() {
    // Every family once, in an order that is neither the curves' nor the kinds'. Three of them are
    // written reversed: a Horizontal's two points, an EqualLength's second segment and a Parallel's
    // first. None of them may be rewritten, re-ordered or turned into a curve of its own.
    let s = square(|ids| {
        vec![
            rule(SketchConstraintRule::Fixed {
                point: at(ids[0], At::End),
                x: 1.,
                y: 2.,
            }),
            rule(SketchConstraintRule::Horizontal {
                a: at(ids[0], At::End),
                b: at(ids[0], At::Start),
            }),
            rule(SketchConstraintRule::Vertical {
                a: at(ids[1], At::Start),
                b: at(ids[1], At::End),
            }),
            rule(SketchConstraintRule::EqualLength {
                a: seg(ids[0], At::Start, At::End),
                b: seg(ids[2], At::End, At::Start),
            }),
            rule(SketchConstraintRule::Parallel {
                a: seg(ids[1], At::End, At::Start),
                b: seg(ids[3], At::Start, At::End),
            }),
            rule(SketchConstraintRule::Perpendicular {
                a: seg(ids[0], At::Start, At::End),
                b: seg(ids[1], At::Start, At::End),
            }),
            rule(SketchConstraintRule::Collinear {
                a: seg(ids[2], At::Start, At::End),
                b: seg(ids[3], At::Start, At::End),
            }),
        ]
    });
    let ids: Vec<_> = s.curves.iter().map(|c| c.id).collect();
    let got = saved(&s);
    let want = [
        (Kind::Fixed, Refs::Point(at(ids[0], At::End))),
        (
            Kind::Horizontal,
            Refs::Line(at(ids[0], At::End), at(ids[0], At::Start)),
        ),
        (
            Kind::Vertical,
            Refs::Line(at(ids[1], At::Start), at(ids[1], At::End)),
        ),
        (
            Kind::EqualLength,
            Refs::Pair(
                seg(ids[0], At::Start, At::End),
                seg(ids[2], At::End, At::Start),
            ),
        ),
        (
            Kind::Parallel,
            Refs::Pair(
                seg(ids[1], At::End, At::Start),
                seg(ids[3], At::Start, At::End),
            ),
        ),
        (
            Kind::Perpendicular,
            Refs::Pair(
                seg(ids[0], At::Start, At::End),
                seg(ids[1], At::Start, At::End),
            ),
        ),
        (
            Kind::Collinear,
            Refs::Pair(
                seg(ids[2], At::Start, At::End),
                seg(ids[3], At::Start, At::End),
            ),
        ),
    ];
    assert_eq!(
        got,
        s.constraints
            .iter()
            .zip(want)
            .map(|(c, (kind, refs))| mark(c.id, kind, refs))
            .collect::<Vec<_>>(),
        "a marker is its rule: the UUID, the kind and the references as stored, in rule order"
    );

    // What is not a marker keeps its row and has none: the dimensions (they have labels), the
    // closure, and every rule that does not name whole Lines of this Sketch.
    let foreign = StableEntityId::new();
    let none = square(|ids| {
        vec![
            rule(SketchConstraintRule::Distance {
                a: at(ids[0], At::Start),
                b: at(ids[0], At::End),
                distance: 10.,
            }),
            rule(SketchConstraintRule::Coincident {
                a: at(ids[0], At::End),
                b: at(ids[1], At::Start),
            }),
            rule(SketchConstraintRule::Horizontal {
                a: at(ids[0], At::Start),
                b: at(ids[1], At::End),
            }),
            rule(SketchConstraintRule::Horizontal {
                a: at(ids[0], At::Start),
                b: at(ids[0], At::Start),
            }),
            rule(SketchConstraintRule::Vertical {
                a: at(foreign, At::Start),
                b: at(foreign, At::End),
            }),
            rule(SketchConstraintRule::EqualLength {
                a: seg(ids[0], At::Start, At::End),
                b: seg(ids[0], At::End, At::Start),
            }),
            rule(SketchConstraintRule::Parallel {
                a: SketchSegmentRef::new(at(ids[0], At::Start), at(ids[1], At::End)),
                b: seg(ids[2], At::Start, At::End),
            }),
            rule(SketchConstraintRule::Perpendicular {
                a: seg(ids[0], At::Start, At::End),
                b: seg(foreign, At::Start, At::End),
            }),
            rule(SketchConstraintRule::Fixed {
                point: at(ids[0], At::Center),
                x: 0.,
                y: 0.,
            }),
            rule(SketchConstraintRule::Fixed {
                point: at(foreign, At::Start),
                x: 0.,
                y: 0.,
            }),
            rule(SketchConstraintRule::Fixed {
                point: at(ids[1], At::Start),
                x: f64::NAN,
                y: 0.,
            }),
            rule(SketchConstraintRule::Radius {
                curve: ids[0],
                radius: 5.,
            }),
        ]
    });
    assert_eq!(saved(&none), Vec::new());

    // A single Circle: its Fixed centre is a marker, its Radius is a label, and nothing else is.
    let mut circle = sketch(vec![a_circle(1., 2., 3.)], Vec::new());
    let c = circle.curves[0].id;
    circle.constraints = vec![
        rule(SketchConstraintRule::Radius {
            curve: c,
            radius: 3.,
        }),
        rule(SketchConstraintRule::Fixed {
            point: at(c, At::Center),
            x: 0.,
            y: 0.,
        }),
        rule(SketchConstraintRule::Fixed {
            point: at(c, At::Start),
            x: 0.,
            y: 0.,
        }),
    ];
    assert_eq!(
        saved(&circle),
        vec![mark(
            circle.constraints[1].id,
            Kind::Fixed,
            Refs::Point(at(c, At::Center))
        )]
    );
    // An annulus and a mixed profile have no markers: those canvases are not covered.
    let mut annulus = sketch(vec![a_circle(0., 0., 5.), a_circle(0., 0., 2.)], Vec::new());
    let first = annulus.curves[0].id;
    annulus.constraints = vec![rule(SketchConstraintRule::Fixed {
        point: at(first, At::Center),
        x: 0.,
        y: 0.,
    })];
    assert_eq!(saved(&annulus), Vec::new());
    let mut mixed = sketch(
        vec![a_line((0., 0.), (1., 0.)), a_circle(0., 0., 5.)],
        Vec::new(),
    );
    let (l, k) = (mixed.curves[0].id, mixed.curves[1].id);
    mixed.constraints = vec![
        rule(SketchConstraintRule::Horizontal {
            a: at(l, At::Start),
            b: at(l, At::End),
        }),
        rule(SketchConstraintRule::Fixed {
            point: at(k, At::Center),
            x: 0.,
            y: 0.,
        }),
    ];
    assert_eq!(saved(&mixed), Vec::new());

    // The references enumerate in the order the rule names them, and a mark knows its curves.
    let m = got[3];
    assert_eq!(
        m.refs.points(),
        vec![
            at(ids[0], At::Start),
            at(ids[0], At::End),
            at(ids[2], At::End),
            at(ids[2], At::Start)
        ]
    );
    assert!(m.names(ids[0]) && m.names(ids[2]) && !m.names(ids[1]) && !m.names(ids[3]));
    println!("\nFCAD_31N_SAVED_EXECUTED");
}

#[test]
fn a_marker_says_its_kind_in_ascii_and_a_prepared_removal_beside_it() {
    let s = square(|ids| {
        vec![
            rule(SketchConstraintRule::Horizontal {
                a: at(ids[0], At::Start),
                b: at(ids[0], At::End),
            }),
            rule(SketchConstraintRule::Parallel {
                a: seg(ids[1], At::Start, At::End),
                b: seg(ids[3], At::Start, At::End),
            }),
        ]
    });
    let marks = saved(&s);
    let none = SketchConstraintEdits::default();
    let gone = SketchConstraintEdits {
        remove: vec![marks[0].id],
        add: Vec::new(),
    };
    assert_eq!(Shown::of(&none, &marks[0]).text(), "H");
    assert_eq!(Shown::of(&gone, &marks[0]).text(), "H -> removed");
    assert_eq!(Shown::of(&gone, &marks[1]).text(), "Par");
    assert!(Shown::of(&gone, &marks[0]).removed && !Shown::of(&gone, &marks[1]).removed);
    // A removal of another rule, or an addition, says nothing about this one.
    let other = SketchConstraintEdits {
        remove: vec![StableEntityId::new()],
        add: Vec::new(),
    };
    assert!(!Shown::of(&other, &marks[0]).removed);

    // Every kind has a name for the list and the hover and a short ASCII word for the canvas, and
    // the canvas font has a glyph for each of them (a missing one is a box in a window).
    let ctx = egui::Context::default();
    let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
    let kinds = [
        (Kind::Horizontal, "Horizontal", "H"),
        (Kind::Vertical, "Vertical", "V"),
        (Kind::Fixed, "Fixed", "Fix"),
        (Kind::EqualLength, "Equal length", "Eq"),
        (Kind::Parallel, "Parallel", "Par"),
        (Kind::Perpendicular, "Perpendicular", "Perp"),
        (Kind::Collinear, "Collinear", "Col"),
    ];
    for (kind, name, short) in kinds {
        assert_eq!((kind.name(), kind.short()), (name, short));
        for words in [short.to_string(), format!("{short} -> removed")] {
            assert!(words.is_ascii(), "{words}");
            assert!(
                ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(11.), &words)),
                "the canvas font has no glyph for {words:?}"
            );
        }
    }
    let shorts: std::collections::BTreeSet<_> = kinds.iter().map(|k| k.2).collect();
    assert_eq!(shorts.len(), 7, "no two kinds share a marker");
    println!("\nFCAD_31N_TEXT_EXECUTED");
}

#[test]
fn a_marker_stands_where_its_references_are_and_is_absent_when_they_cannot_be_drawn() {
    let canvas = rect();
    let view = view();
    let size = measure("Perp") + dimension::PAD * 2.;
    let s = |p: [f64; 2]| view.screen(canvas, p);
    let ids: Vec<_> = (0..4).map(|_| StableEntityId::new()).collect();
    // A box 40 × 20 mm well inside the band; Line 1 runs left to right, Line 2 bottom to top.
    let curves = vec![
        line(ids[0], [20., 22.], [60., 22.]),
        line(ids[1], [60., 22.], [60., 42.]),
        line(ids[2], [60., 42.], [20., 42.]),
        line(ids[3], [20., 42.], [20., 22.]),
    ];
    let put = |m: &Mark| place(m, &curves, &view, canvas, size);
    let horizontal = mark(
        StableEntityId::new(),
        Kind::Horizontal,
        Refs::Line(at(ids[0], At::Start), at(ids[0], At::End)),
    );
    let vertical = mark(
        StableEntityId::new(),
        Kind::Vertical,
        Refs::Line(at(ids[1], At::End), at(ids[1], At::Start)),
    );

    // A Horizontal stands on the lower side of its Line, the Vertical on the left: the side
    // opposite to the one the Line's length label takes (the upper one, the right one).
    let middle = s([40., 22.]);
    let below = put(&horizontal).expect("placed");
    assert!(
        below.top() >= middle.y && (below.center().x - middle.x).abs() < 0.01,
        "{below:?} for the middle {middle:?}"
    );
    let length = dimension::place(&curves[0].shape, &view, canvas, size).expect("the length label");
    assert!(
        length.bottom() <= middle.y && !length.intersects(below),
        "the length label {length:?} and the marker {below:?} share no space"
    );
    let middle = s([60., 32.]);
    let left = put(&vertical).expect("placed");
    assert!(
        left.right() <= middle.x && (left.center().y - middle.y).abs() < 0.01,
        "{left:?} for the middle {middle:?}"
    );
    let length = dimension::place(&curves[1].shape, &view, canvas, size).expect("the length label");
    assert!(length.left() >= middle.x && !length.intersects(left));

    // A Fixed Line end stands just beyond that end, away from the Line, whichever end it is and
    // whichever way the Line was written.
    let fixed_end = mark(
        StableEntityId::new(),
        Kind::Fixed,
        Refs::Point(at(ids[0], At::End)),
    );
    let beyond = put(&fixed_end).expect("placed");
    assert!(
        (beyond.left() - (s([60., 22.]).x + 3.)).abs() < 0.01
            && (beyond.center().y - s([60., 22.]).y).abs() < 0.01,
        "{beyond:?}"
    );
    let fixed_start = mark(
        StableEntityId::new(),
        Kind::Fixed,
        Refs::Point(at(ids[0], At::Start)),
    );
    let before = put(&fixed_start).expect("placed");
    assert!(
        (before.right() - (s([20., 22.]).x - 3.)).abs() < 0.01,
        "{before:?}"
    );

    // A pair stands at the middle of the middles of its two Lines, in either order of the segments.
    let pair = |a: SketchSegmentRef, b: SketchSegmentRef| {
        mark(StableEntityId::new(), Kind::Parallel, Refs::Pair(a, b))
    };
    let across = pair(
        seg(ids[0], At::Start, At::End),
        seg(ids[2], At::End, At::Start),
    );
    let flipped = pair(
        seg(ids[2], At::Start, At::End),
        seg(ids[0], At::End, At::Start),
    );
    let (a, b) = (
        put(&across).expect("placed"),
        put(&flipped).expect("placed"),
    );
    let want = s([40., 32.]);
    assert!(a.center().distance(want) < 0.01, "{a:?} {want:?}");
    assert_eq!(a, b, "the order of the segments moves nothing");

    // A circle's Fixed centre stands to the lower right of the centre, not on it.
    let circle = StableEntityId::new();
    let round = vec![Curve {
        id: circle,
        shape: Shape::Circle {
            center: [40., 32.],
            radius: 8.,
        },
    }];
    let centre = mark(
        StableEntityId::new(),
        Kind::Fixed,
        Refs::Point(at(circle, At::Center)),
    );
    let beside = place(&centre, &round, &view, canvas, size).expect("placed");
    let c = s([40., 32.]);
    assert!(
        beside.center().x > c.x && beside.center().y > c.y && !beside.contains(c),
        "{beside:?} for the centre {c:?}"
    );
    // A Line's marker never stands on a Circle, nor a Circle's on a Line.
    let wrong = mark(
        StableEntityId::new(),
        Kind::Fixed,
        Refs::Point(at(circle, At::Start)),
    );
    assert_eq!(place(&wrong, &round, &view, canvas, size), None);

    // Absent, not clamped: a curve that is not in the drawing or cannot be drawn, and an anchor off
    // the canvas.
    let unknown = mark(
        StableEntityId::new(),
        Kind::Horizontal,
        Refs::Line(
            at(StableEntityId::new(), At::Start),
            at(StableEntityId::new(), At::End),
        ),
    );
    assert_eq!(put(&unknown), None);
    let dead = vec![
        line(ids[0], [20., 22.], [20., 22.]),
        line(ids[1], [20., 22.], [f64::NAN, 3.]),
        line(ids[2], [60., 42.], [20., 42.]),
    ];
    for m in [&horizontal, &fixed_end, &fixed_start, &across] {
        assert_eq!(
            place(m, &dead, &view, canvas, size),
            None,
            "{:?}: a Line that cannot be drawn has no marker",
            m.kind
        );
    }
    assert_eq!(
        place(
            &horizontal,
            &curves,
            &View {
                minimum: [500., 0.],
                scale: 2.
            },
            canvas,
            size
        ),
        None
    );

    // Every box is whole inside the band the canvas's edge words leave, also when its anchor is
    // near an edge: it is slid in, never clipped.
    let high = vec![line(ids[0], [20., 52.], [60., 52.])];
    let slid = place(&horizontal, &high, &view, canvas, size).expect("placed");
    assert!(band(canvas).contains_rect(slid), "{slid:?}");
    let low = vec![line(ids[0], [20., 1.], [60., 1.])];
    let slid = place(&horizontal, &low, &view, canvas, size).expect("placed");
    assert!(band(canvas).contains_rect(slid), "{slid:?}");
    println!("\nFCAD_31N_PLACE_EXECUTED");
}

#[test]
fn markers_overlap_in_saved_order_and_a_marker_is_exactly_its_box() {
    let canvas = rect();
    let ids: Vec<_> = (0..4).map(|_| StableEntityId::new()).collect();
    // Two pair rules on the same two Lines (an EqualLength and a Parallel) stand on one and the same
    // place; a third on the other two Lines stands there as well (the opposite sides of a square
    // have one common middle of middles).
    let curves = vec![
        line(ids[0], [10., 10.], [50., 10.]),
        line(ids[1], [50., 10.], [50., 30.]),
        line(ids[2], [50., 30.], [10., 30.]),
        line(ids[3], [10., 30.], [10., 10.]),
    ];
    let first = mark(
        StableEntityId::new(),
        Kind::EqualLength,
        Refs::Pair(
            seg(ids[0], At::Start, At::End),
            seg(ids[2], At::Start, At::End),
        ),
    );
    let second = mark(
        StableEntityId::new(),
        Kind::Parallel,
        Refs::Pair(
            seg(ids[0], At::Start, At::End),
            seg(ids[2], At::Start, At::End),
        ),
    );
    let third = mark(
        StableEntityId::new(),
        Kind::Collinear,
        Refs::Pair(
            seg(ids[1], At::Start, At::End),
            seg(ids[3], At::Start, At::End),
        ),
    );
    // The order of the rules is neither ascending nor descending by UUID: sort the three by UUID
    // and put them in the order middle, last, first.
    let mut by_uuid = [first, second, third];
    by_uuid.sort_by_key(|m| m.id);
    let stored = [by_uuid[1], by_uuid[2], by_uuid[0]];
    let edits = SketchConstraintEdits::default();
    let shown: Vec<_> = stored.iter().map(|m| Shown::of(&edits, m)).collect();
    let placed = placed(&shown, &curves, &view(), canvas, &measure);
    assert_eq!(placed.len(), 3);
    let d = |p: &Placed| p.shown.mark.id;
    assert_eq!(
        placed.iter().map(d).collect::<Vec<_>>(),
        stored.iter().map(|m| m.id).collect::<Vec<_>>(),
        "saved order"
    );
    // A point in the common box reaches the rule stored first, never the smallest or the largest
    // UUID; the persisted rows reach the others.
    let visible = canvas;
    let centre = placed[0].rect.center();
    assert!(placed.iter().all(|p| p.rect.contains(centre)));
    assert_eq!(
        d(reach(&placed, visible, centre).expect("hit")),
        stored[0].id
    );
    assert!(stored[0].id != by_uuid[0].id && stored[0].id != by_uuid[2].id);

    // A marker is exactly its box: the edges are in, a hair outside is not, and a point off the
    // visible part of the canvas reaches nothing even inside the box.
    let one = [placed[0]];
    let r = one[0].rect;
    for corner in [r.min, r.max, r.left_bottom(), r.right_top()] {
        assert!(reach(&one, visible, corner).is_some(), "{corner:?}");
    }
    for out in [
        r.min - egui::vec2(0.01, 0.),
        r.min - egui::vec2(0., 0.01),
        r.max + egui::vec2(0.01, 0.),
        r.max + egui::vec2(0., 0.01),
    ] {
        assert!(reach(&one, visible, out).is_none(), "{out:?}");
    }
    let clipped = egui::Rect::from_min_max(visible.min, egui::pos2(r.center().x, visible.max.y));
    assert!(reach(&one, clipped, r.right_center()).is_none());
    assert!(reach(&one, clipped, r.left_center()).is_some());
    println!("\nFCAD_31N_HIT_EXECUTED");
}

#[test]
fn a_marker_carries_its_references_whole_and_reaches_nothing_when_nothing_stands() {
    // The marker a click reaches knows its rule, its kind and the references as stored; the
    // reversed segment of a pair is the one it was written with.
    let canvas = rect();
    let ids: Vec<_> = (0..2).map(|_| StableEntityId::new()).collect();
    let curves = vec![
        line(ids[0], [10., 10.], [50., 10.]),
        line(ids[1], [10., 30.], [50., 30.]),
    ];
    let reversed = mark(
        StableEntityId::new(),
        Kind::Perpendicular,
        Refs::Pair(
            seg(ids[0], At::End, At::Start),
            seg(ids[1], At::Start, At::End),
        ),
    );
    let edits = SketchConstraintEdits::default();
    let shown = [Shown::of(&edits, &reversed)];
    let placed = placed(&shown, &curves, &view(), canvas, &measure);
    let hit = reach(&placed, canvas, placed[0].rect.center()).expect("reached");
    assert_eq!(hit.shown.mark, reversed);
    assert_eq!(
        hit.shown.mark.refs.points()[0],
        at(ids[0], At::End),
        "the first point of the first segment is the End it was written with"
    );
    // Nothing stands: nothing is reached, anywhere.
    assert!(reach(&[], canvas, canvas.center()).is_none());
    // The hover words name the kind, the short rule and what the rule names as it stores it.
    let name = |id: StableEntityId| {
        if id == ids[0] {
            "Line A".to_owned()
        } else {
            "Line B".to_owned()
        }
    };
    assert_eq!(
        reversed.refs_words(&name),
        "Line A end>start · Line B start>end"
    );
    let fixed = mark(
        StableEntityId::new(),
        Kind::Fixed,
        Refs::Point(at(ids[1], At::End)),
    );
    assert_eq!(fixed.refs_words(&name), "Line B end");
    let horizontal = mark(
        StableEntityId::new(),
        Kind::Horizontal,
        Refs::Line(at(ids[0], At::End), at(ids[0], At::Start)),
    );
    assert_eq!(horizontal.refs_words(&name), "Line A");
    println!("\nFCAD_31N_REFS_EXECUTED");
}
