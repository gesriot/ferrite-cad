// SPDX-License-Identifier: MIT
//! §31M: which saved rules are directed-angle dimensions, what a label says, where a pair's label
//! stands and what a pointer reaches among all the labels. Pure: curves are the app's own `Curve`s
//! and the stored Sketch is built here, so none of this is geometry a rebuild made (the form, scene
//! and native gates read real drawings).
#![allow(clippy::panic)]
use super::accepted::{Curve, SIZE, Shape};
use super::dimension::*;
use super::navigation::View;
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, DirectedAngle, DirectedLine, LineConstraintKind,
    LineEndpoint, LineLengthMm, LineRelation, Point2, Sketch, SketchConstraint,
    SketchConstraintEdits, SketchConstraintRule, SketchCurve, SketchGeometry, SketchPointRef,
    SketchPointSelector, SketchSegmentRef,
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

fn degrees(value: f64) -> DirectedAngle {
    DirectedAngle::from_degrees(value).expect("an angle in range")
}

fn vector(curve: StableEntityId, from: LineEndpoint) -> DirectedLine {
    DirectedLine::new(curve, from, from.other()).expect("two different ends")
}

fn segment(v: DirectedLine) -> SketchSegmentRef {
    SketchSegmentRef::new(
        SketchPointRef::new(v.curve(), v.from().selector()),
        SketchPointRef::new(v.curve(), v.to().selector()),
    )
}

fn angle_rule(a: DirectedLine, b: DirectedLine, angle: DirectedAngle) -> SketchConstraint {
    SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::DirectedAngle {
            a: segment(a),
            b: segment(b),
            angle: angle.radians(),
        },
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

fn square() -> Sketch {
    sketch(
        vec![
            a_line((0., 0.), (10., 0.)),
            a_line((10., 0.), (10., 10.)),
            a_line((10., 10.), (0., 10.)),
            a_line((0., 10.), (0., 0.)),
        ],
        Vec::new(),
    )
}

fn turn(a: DirectedLine, b: DirectedLine, value: f64) -> Turn {
    Turn {
        id: StableEntityId::new(),
        a,
        b,
        angle: degrees(value),
    }
}

fn shown(turn: Turn, pending: Option<TurnPending>) -> ShownTurn {
    ShownTurn { turn, pending }
}

fn line(start: [f64; 2], end: [f64; 2]) -> Curve {
    Curve {
        id: StableEntityId::new(),
        shape: Shape::Line { start, end },
    }
}

#[test]
fn saved_angles_are_the_directed_angle_rules_between_two_lines_with_their_exact_vectors_in_rule_order()
 {
    let mut s = square();
    let ids: Vec<_> = s.curves.iter().map(|c| c.id).collect();
    let start = |i: usize| vector(ids[i], LineEndpoint::Start);
    let end = |i: usize| vector(ids[i], LineEndpoint::End);
    // Rule order is not curve order: the angle of Lines 3 and 4 is stored before that of 1 and 2.
    let late = angle_rule(end(2), start(3), degrees(-80.));
    let early = angle_rule(start(0), end(1), degrees(110.));
    let half_turn = angle_rule(start(1), start(3), degrees(-180.));
    let zero = angle_rule(end(0), end(2), degrees(0.));
    // A Distance and a Parallel are not angles, and neither is a rule that is not a canonical
    // angle between two different Lines of this Sketch.
    let length = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Distance {
            a: SketchPointRef::new(ids[0], SketchPointSelector::Start),
            b: SketchPointRef::new(ids[0], SketchPointSelector::End),
            distance: 10.,
        },
    };
    let parallel = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Parallel {
            a: segment(start(0)),
            b: segment(start(2)),
        },
    };
    let mut broken = Vec::new();
    // Both vectors on one Line; a vector between two curves; a vector that is not a Line's ends;
    // a Line the Sketch does not hold; radians outside (-pi, pi]; NaN.
    broken.push(angle_rule(start(0), end(0), degrees(30.)));
    let mut across = angle_rule(start(0), start(1), degrees(30.));
    if let SketchConstraintRule::DirectedAngle { a, .. } = &mut across.rule {
        a.to = SketchPointRef::new(ids[1], SketchPointSelector::End);
    }
    broken.push(across);
    let mut centre = angle_rule(start(0), start(1), degrees(30.));
    if let SketchConstraintRule::DirectedAngle { b, .. } = &mut centre.rule {
        b.from = SketchPointRef::new(ids[1], SketchPointSelector::Center);
    }
    broken.push(centre);
    broken.push(angle_rule(
        start(0),
        vector(StableEntityId::new(), LineEndpoint::Start),
        degrees(30.),
    ));
    for radians in [4.0, -std::f64::consts::PI, f64::NAN, f64::INFINITY] {
        let mut bad = angle_rule(start(0), start(1), degrees(30.));
        if let SketchConstraintRule::DirectedAngle { angle, .. } = &mut bad.rule {
            *angle = radians;
        }
        broken.push(bad);
    }
    s.constraints = [vec![length, late, parallel, early, half_turn, zero], broken].concat();

    let found = saved_turns(&s);
    assert_eq!(
        found,
        vec![
            Turn {
                id: late.id,
                a: end(2),
                b: start(3),
                angle: degrees(-80.)
            },
            Turn {
                id: early.id,
                a: start(0),
                b: end(1),
                angle: degrees(110.)
            },
            Turn {
                id: half_turn.id,
                a: start(1),
                b: start(3),
                angle: degrees(180.)
            },
            Turn {
                id: zero.id,
                a: end(0),
                b: end(2),
                angle: degrees(0.)
            },
        ],
        "the order of the rules, with each vector as the rule holds it; -180 is the stored +180; \
         nothing invented for a rule that is not an angle between two different Lines"
    );
    // The rule's own UUID is the identity, and a vector keeps the end it starts at.
    assert_eq!(found[0].a.from(), LineEndpoint::End);
    assert_eq!(found[0].b.from(), LineEndpoint::Start);
    assert_eq!(found[0].name(), "Directed angle -80°");
    assert_eq!(found[2].name(), "Directed angle 180°");
    assert_eq!(found[3].name(), "Directed angle 0°");

    // The family of lengths and radii does not see an angle, and an angle does not see them.
    s.constraints.push(SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Distance {
            a: SketchPointRef::new(ids[1], SketchPointSelector::End),
            b: SketchPointRef::new(ids[1], SketchPointSelector::Start),
            distance: 25.,
        },
    });
    assert_eq!(saved(&s).len(), 2, "the lengths of Lines 1 and 2 only");
    assert!(
        saved(&s)
            .iter()
            .all(|d| d.kind == Kind::Length && (d.target_mm == 10. || d.target_mm == 25.))
    );
    assert_eq!(saved_turns(&s), found);

    // A Circle, an annulus and a mixed profile have no pair of Lines to turn.
    let circle = SketchGeometry::Circle {
        center: Point2 { x: 0., y: 0. },
        radius: 3.,
    };
    let mut mixed = sketch(vec![a_line((0., 0.), (5., 0.)), circle.clone()], Vec::new());
    let (l, c) = (mixed.curves[0].id, mixed.curves[1].id);
    mixed.constraints = vec![angle_rule(
        vector(l, LineEndpoint::Start),
        vector(c, LineEndpoint::Start),
        degrees(30.),
    )];
    assert!(saved_turns(&mixed).is_empty(), "a mixed profile");
    let mut round = sketch(vec![circle], Vec::new());
    round.constraints = Vec::new();
    assert!(saved_turns(&round).is_empty(), "a Circle");
    assert!(saved_turns(&sketch(Vec::new(), Vec::new())).is_empty());
    println!("\nFCAD_31M_SAVED_EXECUTED");
}

#[test]
fn a_label_states_the_signed_target_the_order_and_the_unit_and_never_reads_as_another_value() {
    let (a, b) = (
        vector(StableEntityId::new(), LineEndpoint::Start),
        vector(StableEntityId::new(), LineEndpoint::End),
    );
    let text = |value: f64| shown(turn(a, b, value), None).text();
    // The words are the order, the signed number and the unit; the sign is the rule's.
    assert_eq!(text(80.), "A>B 80°");
    assert_eq!(text(-80.), "A>B -80°");
    assert_eq!(text(0.), "A>B 0°");
    assert_eq!(text(180.), "A>B 180°");
    assert_eq!(text(-180.), "A>B 180°", "-180 is stored as +180");
    assert_eq!(text(12.5), "A>B 12.5°");
    // A prepared replacement stands beside the target and never replaces it.
    let seventy = turn(a, b, -70.);
    assert_eq!(
        shown(seventy, Some(TurnPending::Replaced(degrees(110.)))).text(),
        "A>B -70 -> 110°"
    );
    assert_eq!(
        shown(seventy, Some(TurnPending::Replaced(degrees(-180.)))).text(),
        "A>B -70 -> 180°"
    );
    assert_eq!(
        shown(seventy, Some(TurnPending::Removed)).text(),
        "A>B -70° -> removed"
    );
    // Long numbers are shortened with a mark that digits are missing.
    assert_eq!(text(12.345_678_912), "A>B 12.3457…°");
    assert_eq!(text(-33.333_333_333_3), "A>B -33.3333…°");
    assert_eq!(text(0.000_04), "A>B 0.00004°");
    // A value that four decimals would make read as zero, half a turn or another side of it is
    // written with all its digits (scientific when it is tiny): never `0…` for 1e-10 degrees, never
    // `180…` for 179.9999999.
    for (value, words) in [
        (1e-10, "A>B 1e-10°"),
        (-1e-10, "A>B -1e-10°"),
        (0.000_012_345_6, "A>B 1.23456e-5°"),
        (179.999_999_9, "A>B 179.9999999°"),
        (-179.999_999_9, "A>B -179.9999999°"),
        (179.999_999_999_999_97, "A>B 179.99999999999997°"),
        (-179.999_999_999_999_97, "A>B -179.99999999999997°"),
    ] {
        assert_eq!(text(value), words, "{value}");
    }
    for words in [
        text(1e-10),
        text(-1e-10),
        text(179.999_999_9),
        text(-179.999_999_9),
    ] {
        assert!(!words.contains('…'), "{words}");
        assert!(
            words != text(0.) && words != text(180.),
            "{words} reads as another value"
        );
    }
    // Two values that differ only at the ninth decimal are two different words.
    assert_ne!(text(179.999_999_9), text(179.999_999_8));

    // What the pending request says of a saved angle is read by its rule's UUID and its two exact
    // vectors: the rule removed with an addition on the same vectors is a replacement; the removal
    // alone is a removal; another answer about the pair, the swapped or reversed vectors and
    // another pair's angle are not this angle's replacement.
    let add = |a, b, value| {
        AddSketchConstraint::Line(AddLineConstraint::DirectedAngle {
            a,
            b,
            angle: degrees(value),
        })
    };
    assert_eq!(
        pending_turn(&SketchConstraintEdits::default(), &seventy),
        None
    );
    let removed = SketchConstraintEdits {
        remove: vec![seventy.id],
        add: Vec::new(),
    };
    assert_eq!(pending_turn(&removed, &seventy), Some(TurnPending::Removed));
    let replaced = SketchConstraintEdits {
        remove: vec![seventy.id],
        add: vec![add(b, a, 70.), add(seventy.a, seventy.b, 110.)],
    };
    assert_eq!(
        pending_turn(&replaced, &seventy),
        Some(TurnPending::Replaced(degrees(110.))),
        "the swapped vectors are not this rule's replacement"
    );
    let turned = vector(a.curve(), LineEndpoint::End);
    for other in [
        add(turned, b, 110.),
        add(a, vector(b.curve(), LineEndpoint::Start), 110.),
        add(vector(StableEntityId::new(), LineEndpoint::Start), b, 110.),
        AddSketchConstraint::Line(AddLineConstraint::Relation {
            a: a.curve(),
            b: b.curve(),
            relation: LineRelation::Parallel,
        }),
        AddSketchConstraint::Line(AddLineConstraint::Line {
            curve: a.curve(),
            kind: LineConstraintKind::Distance(LineLengthMm::new(5.).expect("length")),
        }),
    ] {
        assert!(!seventy.is_replaced_by(&other), "{other:?}");
        assert_eq!(
            pending_turn(
                &SketchConstraintEdits {
                    remove: vec![seventy.id],
                    add: vec![other]
                },
                &seventy
            ),
            Some(TurnPending::Removed)
        );
    }
    // An addition without the removal and another rule's removal say nothing of this one.
    assert_eq!(
        pending_turn(
            &SketchConstraintEdits {
                remove: Vec::new(),
                add: vec![add(a, b, 110.)]
            },
            &turn(a, b, -70.)
        ),
        None
    );
    assert_eq!(
        pending_turn(
            &SketchConstraintEdits {
                remove: vec![StableEntityId::new()],
                add: vec![add(a, b, 110.)]
            },
            &seventy
        ),
        None
    );
    assert!(seventy.is_replaced_by(&seventy.replaced_by(degrees(5.))));
    println!("\nFCAD_31M_TEXT_EXECUTED");
}

type Segment = [[f64; 2]; 2];

/// The box of a label of `words` for the two Lines in the view and canvas of these tests.
fn stand(a: Segment, b: Segment, words: &str) -> Option<egui::Rect> {
    place_pair(
        &Shape::Line {
            start: a[0],
            end: a[1],
        },
        &Shape::Line {
            start: b[0],
            end: b[1],
        },
        &view(),
        rect(),
        measure(words) + PAD * 2.,
    )
}

#[test]
fn the_label_of_a_pair_stands_midway_between_its_lines_and_is_finite_for_every_relative_position() {
    let words = "A>B 90°";
    let size = measure(words) + PAD * 2.;
    let screen = |p: [f64; 2]| view().screen(rect(), p);
    // Two Lines at a right angle: centred on the middle of the segment joining their middles.
    let (a, b) = ([[10., 10.], [50., 10.]], [[50., 10.], [50., 40.]]);
    let at = stand(a, b, words).expect("on the canvas");
    assert_eq!(at.size(), size);
    let (ma, mb) = (screen([30., 10.]), screen([50., 25.]));
    assert_eq!(at.center(), ma + (mb - ma) * 0.5);
    // Swapping A and B, or running a Line the other way, moves nothing: the place is the pair's.
    assert_eq!(stand(b, a, words), Some(at));
    assert_eq!(stand([a[1], a[0]], b, words), Some(at));
    assert_eq!(stand(a, [b[1], b[0]], words), Some(at));

    // 0° and 180°: parallel Lines side by side, run the same way and opposite ways; collinear
    // Lines end to end; a Line over itself; Lines crossing at their middles. Every one has a place
    // and it is the middle of the middles, no intersection is looked for or invented.
    let cases: [(Segment, Segment); 6] = [
        ([[10., 10.], [60., 10.]], [[10., 40.], [60., 40.]]),
        ([[10., 10.], [60., 10.]], [[60., 40.], [10., 40.]]),
        ([[10., 25.], [30., 25.]], [[40., 25.], [60., 25.]]),
        ([[10., 25.], [60., 25.]], [[10., 25.], [60., 25.]]),
        ([[10., 25.], [60., 25.]], [[35., 10.], [35., 40.]]),
        ([[10., 25.], [60., 25.]], [[60., 25.], [10., 25.]]),
    ];
    for (a, b) in cases {
        let at = stand(a, b, words).unwrap_or_else(|| panic!("{a:?} {b:?}"));
        let mid = |l: Segment| {
            let (p, q) = (screen(l[0]), screen(l[1]));
            p + (q - p) * 0.5
        };
        let want = mid(a) + (mid(b) - mid(a)) * 0.5;
        assert!(
            at.is_finite() && (at.center() - want).length() < 1e-3,
            "{a:?} {b:?}: {at:?}"
        );
    }
    // Nearly parallel: the lines meet 1e9 mm away; the label is where the Lines are.
    let near = stand(
        [[10., 20.], [60., 20.]],
        [[10., 20.], [60., 20. + 5e-8]],
        words,
    )
    .expect("on the canvas");
    assert!(near.is_finite() && rect().contains_rect(near));
    assert!((near.center().y - screen([35., 20.]).y).abs() < 1e-3);

    // Inside the band the words on the canvas edges leave: slid, not clamped away.
    let low = stand([[0.2, 0.], [0.4, 0.]], [[0.2, 0.], [0.4, 0.]], words).expect("on the canvas");
    assert!(low.bottom() <= rect().bottom() - 24. && low.left() >= rect().left() + 3.);
    // Absent, not clamped, when the point is off the canvas or a Line cannot be drawn.
    assert_eq!(
        stand(
            [[200., 10.], [220., 10.]],
            [[200., 20.], [220., 20.]],
            words
        ),
        None
    );
    assert_eq!(stand(a, [[50., 10.], [50., 10.]], words), None, "a point");
    assert_eq!(stand([[f64::NAN, 1.], [2., 3.]], b, words), None);
    let ring = Shape::Circle {
        center: [30., 25.],
        radius: 5.,
    };
    let l = Shape::Line {
        start: a[0],
        end: a[1],
    };
    assert_eq!(
        place_pair(&l, &ring, &view(), rect(), size),
        None,
        "a pair is two Lines"
    );
    // One Line's middle off the canvas but the middle of the middles on it: still a label.
    assert!(stand([[100., 25.], [200., 25.]], [[10., 25.], [30., 25.]], words).is_some());

    // The labels of a list stand in the order of the list and skip a pair whose Lines the
    // drawing does not hold.
    let (l0, l1, l2) = (
        line([10., 10.], [50., 10.]),
        line([50., 10.], [50., 40.]),
        line([50., 40.], [10., 40.]),
    );
    let curves = [l0.clone(), l1.clone(), l2.clone()];
    let start = |c: &Curve| vector(c.id, LineEndpoint::Start);
    let list = [
        shown(turn(start(&l0), start(&l1), 90.), None),
        shown(turn(start(&l1), start(&l2), 90.), None),
        shown(
            turn(
                start(&l1),
                vector(StableEntityId::new(), LineEndpoint::Start),
                90.,
            ),
            None,
        ),
    ];
    let placed = placed_turns(&list, &curves, &view(), rect(), &measure);
    assert_eq!(placed.len(), 2);
    assert_eq!(
        placed.iter().map(|p| p.shown.turn.id).collect::<Vec<_>>(),
        vec![list[0].turn.id, list[1].turn.id]
    );
    assert!(
        placed
            .iter()
            .all(|p| p.rect.size() == measure("A>B 90°") + PAD * 2.)
    );
    println!("\nFCAD_31M_PLACE_EXECUTED");
}

#[test]
fn labels_of_curves_and_pairs_overlap_in_saved_order_and_a_label_is_exactly_its_box() {
    let visible = rect();
    let (a, b) = (line([10., 10.], [50., 10.]), line([50., 10.], [50., 40.]));
    let shown_size = |curve: &Curve| Shown {
        dimension: Dimension {
            id: StableEntityId::new(),
            curve: curve.id,
            kind: Kind::Length,
            target_mm: 40.,
        },
        pending: None,
    };
    let start = |c: &Curve| vector(c.id, LineEndpoint::Start);
    // Three angle labels over one place; their UUIDs sorted the other way round than the rules.
    let mut turns: Vec<_> = (0..3)
        .map(|_| shown(turn(start(&a), start(&b), 90.), None))
        .collect();
    turns.sort_by_key(|x| std::cmp::Reverse(x.turn.id.to_string()));
    let curves = [a.clone(), b.clone()];
    let sizes = placed(&[shown_size(&a)], &curves, &view(), visible, &measure);
    let pairs = placed_turns(&turns, &curves, &view(), visible, &measure);
    assert_eq!((sizes.len(), pairs.len()), (1, 3));
    let (size_box, pair_box) = (sizes[0].rect, pairs[0].rect);
    assert!(pairs.iter().all(|p| p.rect == pair_box), "one place");
    let id_of = |r: Option<Reached>| match r {
        Some(Reached::Turn(t)) => Some(t.shown.turn.id),
        _ => None,
    };
    // A point on the angle labels only reaches the first of them in the order of the rules, never
    // the one a UUID or a hash would prefer.
    let apart = egui::pos2(pair_box.right() - 1., pair_box.center().y);
    assert!(!size_box.contains(apart));
    assert_eq!(
        id_of(reach(&sizes, &pairs, visible, apart)),
        Some(turns[0].turn.id)
    );
    // A label of a curve is drawn above a label of a pair: where they overlap, the curve's is
    // reached; the part of the pair's box outside it still reaches the angle.
    let overlap = size_box.intersect(pair_box);
    assert!(
        overlap.is_positive(),
        "the fixture overlaps: {size_box:?} {pair_box:?}"
    );
    assert!(matches!(
        reach(&sizes, &pairs, visible, overlap.center()),
        Some(Reached::Size(_))
    ));
    // Exactly the drawn box, edges included, nothing a hair outside; off the visible canvas nothing.
    let lone = placed_turns(&turns[..1], &curves, &view(), visible, &measure);
    let r = lone[0].rect;
    for inside in [r.min, r.max, r.center(), r.left_bottom(), r.right_top()] {
        assert!(
            matches!(reach(&[], &lone, visible, inside), Some(Reached::Turn(_))),
            "{inside:?}"
        );
    }
    for outside in [
        r.min - egui::vec2(0.01, 0.),
        r.max + egui::vec2(0., 0.01),
        egui::pos2(r.center().x, r.top() - 0.01),
        egui::pos2(r.right() + 0.01, r.center().y),
    ] {
        assert!(reach(&[], &lone, visible, outside).is_none(), "{outside:?}");
    }
    let clipped = egui::Rect::from_min_max(visible.min, egui::pos2(r.center().x, visible.max.y));
    assert!(
        reach(&[], &lone, clipped, r.max).is_none(),
        "outside the visible canvas"
    );
    assert!(reach(&[], &[], visible, r.center()).is_none());
    println!("\nFCAD_31M_HIT_EXECUTED");
}

#[test]
fn an_angle_typed_back_from_its_display_is_the_saved_angle_for_the_probes_and_the_gap_is_told() {
    let (a, b) = (
        vector(StableEntityId::new(), LineEndpoint::Start),
        vector(StableEntityId::new(), LineEndpoint::Start),
    );
    // Near zero and near half a turn, both signs: the displayed degrees, typed, are exactly the
    // saved angle (no tolerance, the bits of the radians).
    let mut probes = vec![0., 180., -180.];
    for exponent in -12..=-3 {
        let step = 10f64.powi(exponent);
        probes.extend([step, -step, 1.5 * step, -3.7 * step]);
    }
    for exponent in -13..=-1 {
        let step = 10f64.powi(exponent);
        probes.extend([
            180. - step,
            -180. + step,
            180. - 3. * step,
            -180. + 2.5 * step,
        ]);
    }
    probes.extend([
        30.,
        -30.,
        70.,
        -70.,
        110.,
        -110.,
        89.999_999_999_999_99,
        0.1,
        -0.1,
    ]);
    for probe in probes {
        let saved = turn(a, b, probe);
        let typed = editable_degrees(saved.angle);
        let back = DirectedAngle::from_degrees(typed.to_string().parse().expect("a number"))
            .expect("in range");
        assert!(
            saved.typeable()
                && back == saved.angle
                && back.radians().to_bits() == saved.angle.radians().to_bits(),
            "{probe} shown as {typed} reads back as {back:?}"
        );
        assert!(typed.abs() <= 180. && typed > -180., "{typed}");
    }
    // Every angle created from these degree inputs must have an exact editable spelling, even
    // when the inverse conversion alone rounds to a neighbouring input.
    let mut gap = Vec::new();
    for k in 1..400 {
        let step = f64::from(k) * 1e-10;
        for value in [step, -step, 180. - step, -180. + step] {
            let saved = turn(a, b, value);
            let typed =
                DirectedAngle::from_degrees(editable_degrees(saved.angle)).expect("in range");
            assert_eq!(typed == saved.angle, saved.typeable(), "{value}");
            if !saved.typeable() {
                let apart = typed
                    .radians()
                    .to_bits()
                    .abs_diff(saved.angle.radians().to_bits());
                assert_eq!(apart, 1, "{value}: one unit in the last place, not more");
                assert!((typed.radians() - saved.angle.radians()).abs() < 1e-15);
                gap.push(value);
            }
        }
    }
    eprintln!(
        "FCAD_31M_ROUNDTRIP_GAP measured={} first={:?}",
        gap.len(),
        &gap[..gap.len().min(6)]
    );
    assert!(
        gap.is_empty(),
        "a displayed saved angle must type back exactly: {gap:?}"
    );
    // Arbitrary stored radians can lie outside the image of the degree converter: the UI must
    // still report that case honestly, without treating a nearby angle as equal.
    let mut unreachable = turn(a, b, 10.);
    unreachable.angle = DirectedAngle::from_radians(0.174_532_925_199_432_98).expect("canonical");
    assert!(!unreachable.typeable());
    println!("\nFCAD_31M_ROUNDTRIP_EXECUTED");
}
