// SPDX-License-Identifier: MIT
//! §31D-2: the explicit "From object" gesture through the production canvas and
//! editor. Real egui pointer, key and text events go in; draft strings, history and
//! what was painted come out. No window and no graphics device, and nothing here
//! calls a private helper in place of the interface: every anchor below is reached by
//! hovering it, and every gesture starts with a press on it.
#![allow(clippy::panic)]
use super::{
    drag_tests::{button, move_to, press, vertex},
    guide_tests::{circle, gesture_is_over, guides, history, near, ray, ring, target, type_length},
    object_snap_tests::{
        L, ZIGZAG, at, choose, context, escape, hover, label_at, open, painted, painted_containing,
        pair, rect, saved, strings, switch,
    },
    tests::{click, frame, frame_running, replace_field, text_at},
    *,
};

/// The provisional anchor's ring, where it is drawn.
fn provisional(out: &egui::FullOutput) -> Option<egui::Pos2> {
    circle(out, ANCHOR_COLOUR, 11.)
}
/// How many confirmed-anchor rings are drawn: the guide anchor's, apart from the target.
fn rings(out: &egui::FullOutput) -> usize {
    out.shapes
        .iter()
        .filter(|c| matches!(&c.shape, egui::Shape::Circle(s) if s.stroke.color == GUIDE_COLOUR && (s.radius - 8.).abs() < 0.01))
        .count()
}
/// "From object" pressed through the real checkbox in the Guides row.
fn from_object(ctx: &egui::Context, e: &mut Editor) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, "From object"));
}
/// An open New drawing with Guides chosen and the mode on, the way a user does.
fn anchored(points: &[[&str; 2]], mode: &str) -> (egui::Context, Editor) {
    let (ctx, mut e) = open(points, false);
    guides(&ctx, &mut e, mode);
    from_object(&ctx, &mut e);
    assert!(e.canvas.anchors, "the checkbox took the click");
    (ctx, e)
}
/// A saved Sketch with Guides chosen and the mode on.
fn saved_anchored(points: &[[&str; 2]], mode: &str) -> (egui::Context, Editor) {
    let (ctx, mut e, _) = saved(points);
    guides(&ctx, &mut e, mode);
    from_object(&ctx, &mut e);
    assert!(e.canvas.anchors);
    (ctx, e)
}
fn drag_events(from: egui::Pos2, to: egui::Pos2) -> Vec<egui::Event> {
    let primary = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    vec![
        egui::Event::PointerMoved(from),
        primary(from, true),
        egui::Event::PointerMoved(to),
        primary(to, false),
    ]
}
fn anchor_of(e: &Editor) -> Option<String> {
    e.canvas
        .provisional
        .map(|c| c.source(e.draft.as_ref().map_or(0, |d| d.points.len())))
}
/// The standing words painted on the canvas, if there are any, on one line.
fn standing(out: &egui::FullOutput) -> Option<String> {
    out.shapes.iter().find_map(|c| match &c.shape {
        egui::Shape::Text(t) if t.galley.text().starts_with("From object: ") => {
            Some(t.galley.text().replace('\n', " "))
        }
        _ => None,
    })
}
/// Whether the standing words fit the canvas: no row is wider than it.
fn standing_fits(out: &egui::FullOutput, canvas: egui::Rect) -> bool {
    out.shapes.iter().all(|c| match &c.shape {
        egui::Shape::Text(t) if t.galley.text().starts_with("From object: ") => {
            t.visual_bounding_rect().width() < canvas.width() - 8.
        }
        _ => true,
    })
}
/// An open L: three vertices, no crossing; middles at (20, 0) and (40, 15).
const OPEN_L: [[&str; 2]; 3] = [["0", "0"], ["40", "0"], ["40", "30"]];

#[test]
fn the_mode_sits_in_the_guides_row_needs_guides_and_changes_only_editor_state() {
    let (ctx, mut e) = open(&ZIGZAG, false);
    let out = frame(&ctx, &mut e, vec![]);
    let (row, tick) = (text_at(&out, "Guides:"), text_at(&out, "From object"));
    assert!((tick.y - row.y).abs() < 3., "in the Guides row");
    assert!(tick.x > text_at(&out, "Length mm").x, "beside Length mm");
    assert!(
        !e.canvas.anchors && e.canvas.provisional.is_none(),
        "off for a new editor"
    );
    assert_eq!(standing(&out), None, "no line while it is off");
    // Unavailable while Guides are Off: the click is not taken.
    click(&ctx, &mut e, tick);
    assert!(!e.canvas.anchors);

    guides(&ctx, &mut e, "Orthogonal");
    let before = (e.draft.clone(), history(&e), e.canvas.selected);
    from_object(&ctx, &mut e);
    assert!(e.canvas.anchors);
    let out = frame(&ctx, &mut e, vec![]);
    let line = standing(&out).expect("the standing line");
    assert!(
        line.contains("hover an endpoint, midpoint or crossing")
            && line.contains("it adds vertex 5"),
        "{line}"
    );
    assert_eq!((e.draft.clone(), history(&e), e.canvas.selected), before);
    assert!(
        ctx.globally_used_rect().width() <= 700.,
        "the wider row keeps the form near its width: {:?}",
        ctx.globally_used_rect()
    );

    // A shown anchor, then Guides Off: the anchor is gone, the mode is remembered but
    // unavailable, and no string moved.
    let spot = at(&ctx, &mut e, [10., 10.]);
    let out = hover(&ctx, &mut e, spot);
    assert!(provisional(&out).is_some() && e.canvas.provisional.is_some());
    guides(&ctx, &mut e, "Off");
    let out = frame(&ctx, &mut e, vec![]);
    assert!(e.canvas.provisional.is_none() && provisional(&out).is_none());
    assert!(e.canvas.anchors, "the setting is the editor's, not cleared");
    assert!(standing(&out).is_some_and(|l| l.contains("choose Orthogonal or Polar 45° first")));
    assert_eq!((e.draft.clone(), history(&e), e.canvas.selected), before);
    // Back on: the mode is still on, and the anchor comes back only from the pointer.
    guides(&ctx, &mut e, "Polar 45°");
    assert!(e.canvas.anchors);
    // Switched off by the checkbox itself: anchor gone, nothing else changed.
    hover(&ctx, &mut e, spot);
    assert!(e.canvas.provisional.is_some());
    from_object(&ctx, &mut e);
    assert!(!e.canvas.anchors && e.canvas.provisional.is_none());
    assert_eq!((e.draft.clone(), history(&e)), (before.0, before.1));
}

#[test]
fn a_new_point_is_added_from_a_shown_endpoint_midpoint_or_crossing_reached_by_real_events() {
    // What the old canvas does, mode off: a press on an end selects and drags that end, and a
    // click elsewhere adds a point. Neither can start a gesture from another point.
    let (ctx, mut e) = open(&OPEN_L, false);
    guides(&ctx, &mut e, "Orthogonal");
    let first = vertex(&ctx, &mut e, 0);
    press(&ctx, &mut e, first);
    assert_eq!(
        e.canvas.selected,
        Some(0),
        "the press selects the end it is on"
    );
    assert!(
        e.canvas
            .gesture
            .as_ref()
            .is_some_and(|g| g.anchored.is_none()),
        "and drags it"
    );
    button(&ctx, &mut e, first, false);
    assert_eq!(
        (history(&e), e.draft.as_ref().expect("draft").points.len()),
        ((0, 0), 3)
    );

    // Mode on. The pointer rests 3 points from vertex 1's centre: a provisional anchor
    // is shown, nothing else moves.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    let v0 = vertex(&ctx, &mut e, 0);
    let near_v0 = v0 + egui::vec2(2., -2.);
    let before = (e.draft.clone(), e.canvas.selected, history(&e));
    let out = hover(&ctx, &mut e, near_v0);
    assert!(
        provisional(&out).is_some_and(|p| near(p, v0)),
        "the ring is on the endpoint"
    );
    assert!(
        label_at(&out, "From Endpoint · vertex 1 at (0, 0) mm").is_some(),
        "named"
    );
    assert_eq!(anchor_of(&e).as_deref(), Some("vertex 1"));
    assert!(
        standing(&out)
            .is_some_and(|l| l.contains("press to start") && l.contains("it adds vertex 4"))
    );
    assert_eq!(
        (e.draft.clone(), e.canvas.selected, history(&e)),
        before,
        "hover changes nothing"
    );
    assert!(
        rings(&out) == 0,
        "and only one ring is shown: the provisional one"
    );

    // The press confirms it: no selection, no drag of the vertex under the pointer.
    press(&ctx, &mut e, near_v0);
    assert_eq!(e.canvas.selected, None, "the anchor is not selected");
    assert!(
        e.canvas
            .gesture
            .as_ref()
            .is_some_and(|g| g.anchored.is_some())
    );
    assert!(e.canvas.claimed_press && e.gesturing());
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        provisional(&out).is_none(),
        "confirmed: the provisional ring is gone"
    );
    assert!(
        ring(&out).is_some_and(|p| near(p, v0)),
        "the confirmed ring, on the anchor"
    );
    assert!(standing(&out).is_some_and(|l| l.contains("fixed until release")));

    // North, 22.5 mm along and two points off the line: a point 22.5 mm from the anchor
    // (not from the last vertex, which is (40, 30)) is added to the draft as a preview.
    let pointer = at(&ctx, &mut e, [0.5, 22.5]);
    let out = move_to(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 3), pair("0", "22.5"));
    assert!(
        ring(&out).is_some_and(|p| near(p, v0)),
        "the anchor stays behind"
    );
    assert!(target(&out).is_some_and(|p| near(p, at(&ctx, &mut e, [0., 22.5]))));
    assert!(
        label_at(
            &out,
            "90° · 22.5 mm from Endpoint · vertex 1 · adds vertex 4"
        )
        .is_some()
    );
    assert_eq!(history(&e), (0, 0), "a preview is no history");
    button(&ctx, &mut e, pointer, false);
    assert!(gesture_is_over(&e));
    assert_eq!(history(&e), (1, 0), "one gesture, one Undo");
    let added = e.draft.clone();
    assert_eq!(e.canvas.selected, None);
    e.undo();
    assert_eq!(e.draft, before.0);
    e.redo();
    assert_eq!(e.draft, added);

    // A press that does not move adds nothing, and no duplicate of the anchor.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    let v0 = vertex(&ctx, &mut e, 0);
    let before = e.draft.clone();
    hover(&ctx, &mut e, v0);
    press(&ctx, &mut e, v0);
    button(&ctx, &mut e, v0, false);
    assert_eq!(
        (e.draft.clone(), history(&e), e.canvas.selected),
        (before, (0, 0), None)
    );
    assert!(gesture_is_over(&e));

    // The crossing of a zigzag, then the middle of an edge: kinds and words.
    let (ctx, mut e) = anchored(&ZIGZAG, "Orthogonal");
    let crossing = at(&ctx, &mut e, [10., 10.]);
    let out = hover(&ctx, &mut e, crossing + egui::vec2(3., 1.));
    assert!(provisional(&out).is_some_and(|p| near(p, crossing)));
    assert!(label_at(&out, "From Intersection · edges 1>2 × 3>4 at (10, 10) mm").is_some());
    press(&ctx, &mut e, crossing + egui::vec2(3., 1.));
    let east = at(&ctx, &mut e, [25., 10.25]);
    let out = move_to(&ctx, &mut e, east);
    assert!(
        label_at(
            &out,
            "0° · 15 mm from Intersection · edges 1>2 × 3>4 · adds vertex 5"
        )
        .is_some()
    );
    button(&ctx, &mut e, east, false);
    assert_eq!(strings(&e, 4), pair("25", "10"));
    assert_eq!(history(&e), (1, 0));

    let (ctx, mut e) = anchored(&ZIGZAG, "Orthogonal");
    let middle = at(&ctx, &mut e, [30., 15.]);
    let out = hover(&ctx, &mut e, middle);
    assert!(provisional(&out).is_some_and(|p| near(p, middle)));
    assert!(label_at(&out, "From Midpoint · edge 2>3 at (30, 15) mm").is_some());
    press(&ctx, &mut e, middle);
    let west = at(&ctx, &mut e, [20., 15.25]);
    let out = move_to(&ctx, &mut e, west);
    assert!(
        label_at(
            &out,
            "180° · 10 mm from Midpoint · edge 2>3 · adds vertex 5"
        )
        .is_some()
    );
    button(&ctx, &mut e, west, false);
    assert_eq!(strings(&e, 4), pair("20", "15"));
    assert_eq!(history(&e), (1, 0));

    // Only finite crossings: the first edge's extension meets the last edge at (12, 0),
    // beyond its end (10, 0); what is offered there is the middle (12, -1) of that edge.
    let extension = [
        ["0", "0"],
        ["10", "0"],
        ["10", "3"],
        ["12", "3"],
        ["12", "-5"],
    ];
    let (ctx, mut e) = anchored(&extension, "Orthogonal");
    let spot = at(&ctx, &mut e, [12., 0.]);
    let out = hover(&ctx, &mut e, spot);
    assert_eq!(
        anchor_of(&e).as_deref(),
        Some("edge 4>5"),
        "a middle, not an intersection"
    );
    assert!(label_at(&out, "From Midpoint · edge 4>5 at (12, -1) mm").is_some());

    // Far from every object point there is no anchor, and a click is the old click.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    let free = at(&ctx, &mut e, [90., 40.]);
    let out = hover(&ctx, &mut e, free);
    assert!(provisional(&out).is_none() && e.canvas.provisional.is_none());
    click(&ctx, &mut e, free);
    assert_eq!(strings(&e, 3), pair("90.000", "40.000"));
    assert_eq!(history(&e), (1, 0));
}

#[test]
fn a_saved_vertex_moves_from_another_anchor_without_switching_the_selection() {
    let (ctx, mut e) = saved_anchored(&L, "Orthogonal");
    let original = e.draft.clone();
    // No vertex is selected: the form says so, nothing is offered, and the press is the
    // old press that selects the vertex it is on.
    let out = frame(&ctx, &mut e, vec![]);
    assert!(standing(&out).is_some_and(|l| l.contains("select the vertex to move first")));
    let v4 = vertex(&ctx, &mut e, 4);
    assert!(provisional(&hover(&ctx, &mut e, v4)).is_none());
    let v2 = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, v2);
    assert_eq!(e.canvas.selected, Some(2));
    assert!(
        e.canvas
            .gesture
            .as_ref()
            .is_some_and(|g| g.anchored.is_none())
    );
    button(&ctx, &mut e, v2, false);
    assert_eq!((history(&e), e.draft.clone()), ((0, 0), original));
    let first = e.draft.clone();

    // The selected vertex is not an anchor, and neither is anything on its position:
    // hovering it shows no ring, a press on it is the ordinary drag.
    let out = hover(&ctx, &mut e, v2);
    assert!(provisional(&out).is_none());
    assert!(standing(&out).is_some_and(|l| l.contains("it moves vertex 3")));
    press(&ctx, &mut e, v2);
    assert!(
        e.canvas
            .gesture
            .as_ref()
            .is_some_and(|g| g.anchored.is_none())
    );
    button(&ctx, &mut e, v2, false);
    assert_eq!(e.draft, first);

    // Another vertex is: shown, named, and the press keeps the selection on vertex 3.
    type_length(&ctx, &mut e, "15");
    let anchor = vertex(&ctx, &mut e, 4);
    let at_anchor = anchor + egui::vec2(2., -2.);
    let out = hover(&ctx, &mut e, at_anchor);
    assert!(provisional(&out).is_some_and(|p| near(p, anchor)));
    assert!(standing(&out).is_some_and(
        |l| l.contains("Endpoint · vertex 5 at (20, 40) mm") && l.contains("it moves vertex 3")
    ));
    press(&ctx, &mut e, at_anchor);
    assert_eq!(e.canvas.selected, Some(2), "not switched to the anchor");
    // East of it: 82 points along, one off the line. Length 15 mm from the anchor puts
    // vertex 3 at (35, 40), not 15 mm from where vertex 3 was.
    let pointer = at(&ctx, &mut e, [40.5, 40.25]);
    let out = move_to(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 2), pair("35", "40"));
    assert_eq!(
        strings(&e, 4),
        pair("20", "40"),
        "the anchor's own vertex is untouched"
    );
    assert!(ring(&out).is_some_and(|p| near(p, anchor)));
    assert!(
        label_at(
            &out,
            "0° · Length 15 mm from Endpoint · vertex 5 · moves vertex 3"
        )
        .is_some()
    );
    button(&ctx, &mut e, pointer, false);
    assert_eq!(history(&e), (1, 0));
    assert_eq!(e.canvas.selected, Some(2));
    let moved = e.draft.clone();
    let request = e.edit_request().expect("a valid saved edit");
    assert_eq!(
        crate::transaction_tests::vertices_in(
            &request,
            e.editing
                .as_ref()
                .expect("saved")
                .1
                .stored
                .as_ref()
                .expect("sketch")
        )[2]
        .start_mm,
        [35., 40.]
    );
    e.undo();
    assert_eq!(e.draft, first);
    e.redo();
    assert_eq!(e.draft, moved);
    assert_eq!(e.canvas.length, "15", "the field is left as it was typed");

    // The middle of an edge that meets the moved vertex is shown where it is, and is
    // kept there: it does not travel with the vertex being moved.
    let (ctx, mut e) = saved_anchored(&L, "Orthogonal");
    let v2 = vertex(&ctx, &mut e, 2);
    click(&ctx, &mut e, v2);
    assert_eq!(e.canvas.selected, Some(2));
    let middle = at(&ctx, &mut e, [40., 20.]);
    let out = hover(&ctx, &mut e, middle);
    assert!(provisional(&out).is_some_and(|p| near(p, middle)));
    assert_eq!(anchor_of(&e).as_deref(), Some("edge 3>4"));
    // A press on it that does not move changes no string, no step and no selection.
    let unchanged = e.draft.clone();
    press(&ctx, &mut e, middle);
    button(&ctx, &mut e, middle, false);
    assert_eq!(
        (e.draft.clone(), history(&e), e.canvas.selected),
        (unchanged, (0, 0), Some(2))
    );
    hover(&ctx, &mut e, middle);
    press(&ctx, &mut e, middle);
    let north = at(&ctx, &mut e, [40.25, 35.]);
    let out = move_to(&ctx, &mut e, north);
    assert_eq!(strings(&e, 2), pair("40", "35"));
    assert!(
        ring(&out).is_some_and(|p| near(p, middle)),
        "the press-time middle, not the live one at (30, 27.5)"
    );
    button(&ctx, &mut e, north, false);
    assert_eq!(history(&e), (1, 0));

    // A vertex stacked on the moving one is the same place: not offered either.
    let stacked = [
        ["0", "0"],
        ["60", "0"],
        ["60", "20"],
        ["60", "20"],
        ["20", "20"],
        ["0", "40"],
    ];
    let (ctx, mut e) = saved_anchored(&stacked, "Orthogonal");
    let v2 = vertex(&ctx, &mut e, 2);
    click(&ctx, &mut e, v2);
    assert_eq!(e.canvas.selected, Some(2));
    let out = hover(&ctx, &mut e, v2);
    assert!(provisional(&out).is_none() && e.canvas.provisional.is_none());
}

#[test]
fn a_held_anchor_is_released_at_twice_the_aperture_and_replaced_only_by_a_clearly_nearer_one() {
    // P = (20, 20) and Q = (20, 14), six millimetres (24 points) apart; no edge between.
    let drawing = [["20", "20"], ["50", "45"], ["60", "5"], ["20", "14"]];
    let (ctx, mut e) = anchored(&drawing, "Orthogonal");
    let (p, q) = (at(&ctx, &mut e, [20., 20.]), at(&ctx, &mut e, [20., 14.]));
    let on = |ctx: &egui::Context, e: &mut Editor, y: f64| {
        let spot = at(ctx, e, [20., y]);
        let out = hover(ctx, e, spot);
        (provisional(&out), anchor_of(e))
    };
    let held = |seen: (Option<egui::Pos2>, Option<String>),
                expected: Option<(egui::Pos2, &str)>| {
        match expected {
            Some((ring, source)) => {
                assert!(seen.0.is_some_and(|r| near(r, ring)), "{seen:?}");
                assert_eq!(seen.1.as_deref(), Some(source));
            }
            None => assert_eq!(seen, (None, None)),
        }
    };
    // Acquired inside 12 points (11.5), kept out to 24 (23.5) and given up beyond (24.5).
    held(on(&ctx, &mut e, 22.875), Some((p, "vertex 1")));
    held(on(&ctx, &mut e, 23.5), Some((p, "vertex 1"))); // 14 points: held, not acquired
    held(on(&ctx, &mut e, 25.875), Some((p, "vertex 1"))); // 23.5
    held(on(&ctx, &mut e, 26.125), None); // 24.5
    held(on(&ctx, &mut e, 23.375), None); // 13.5: the acquisition distance applies afresh
    // Replacement: P is held, Q gets nearer. Within a point of each other nothing changes
    // (0.5 either way); more than a point nearer replaces.
    held(on(&ctx, &mut e, 18.), Some((p, "vertex 1"))); // 8 from P, 16 from Q
    held(on(&ctx, &mut e, 17.), Some((p, "vertex 1"))); // 12 and 12
    held(on(&ctx, &mut e, 16.9375), Some((p, "vertex 1"))); // 12.25 and 11.75
    held(on(&ctx, &mut e, 16.75), Some((q, "vertex 4"))); // 13 and 11
    held(on(&ctx, &mut e, 17.0625), Some((q, "vertex 4"))); // Q is 12.25 away, P 11.75
    held(on(&ctx, &mut e, 17.3), Some((p, "vertex 1"))); // 2.4 points nearer
    // A fresh pointer between them takes the §31C choice: the nearer.
    frame(&ctx, &mut e, vec![egui::Event::PointerGone]);
    assert!(e.canvas.provisional.is_none(), "PointerGone clears it");
    held(on(&ctx, &mut e, 16.75), Some((q, "vertex 4")));
    // Leaving the canvas clears it too (a move outside the drawing area).
    let outside = rect(&ctx, &mut e).right_top() + egui::vec2(40., -40.);
    hover(&ctx, &mut e, outside);
    assert!(e.canvas.provisional.is_none());
    // The hover never wrote a string, a selection or a step.
    assert_eq!((history(&e), e.canvas.selected), ((0, 0), None));
}

#[test]
fn a_long_gesture_keeps_the_pressed_anchor_even_past_other_points_and_twice_the_aperture() {
    let (ctx, mut e) = anchored(&[["0", "0"], ["40", "0"]], "Orthogonal");
    let v0 = vertex(&ctx, &mut e, 0);
    hover(&ctx, &mut e, v0);
    press(&ctx, &mut e, v0);
    // 95 mm east along y = 0: 380 points, far past 24, over the other vertex (40, 0)
    // on the way, and the anchor is where it was.
    for x in [20., 39.75, 40., 40.25, 70., 95.] {
        let pointer = at(&ctx, &mut e, [x, 0.25]);
        let out = move_to(&ctx, &mut e, pointer);
        assert_eq!(rings(&out), 1, "x = {x}");
        assert!(ring(&out).is_some_and(|p| near(p, v0)), "x = {x}");
        assert!(
            e.canvas.provisional.is_none(),
            "x = {x}: no other anchor is acquired"
        );
        assert_eq!(anchor_of(&e), None);
        assert_eq!(strings(&e, 2), pair(&x.to_string(), "0"), "x = {x}");
    }
    let out = frame(&ctx, &mut e, vec![]);
    assert!(label_at(&out, "0° · 95 mm from Endpoint · vertex 1 · adds vertex 3").is_some());
    let release = at(&ctx, &mut e, [95., 0.25]);
    button(&ctx, &mut e, release, false);
    assert_eq!(history(&e), (1, 0));
    // Released, it is gone: the next hover is a fresh acquisition.
    assert!(e.canvas.gesture.is_none() && !e.canvas.claimed_press);
}

/// Hovers `from` (mm), presses it, moves to `to` (mm) and returns that move's frame; the
/// gesture is still held.
fn begin(ctx: &egui::Context, e: &mut Editor, from: [f64; 2], to: [f64; 2]) -> egui::FullOutput {
    let spot = at(ctx, e, from);
    hover(ctx, e, spot);
    press(ctx, e, spot);
    let pointer = at(ctx, e, to);
    move_to(ctx, e, pointer)
}
/// The same, released where it moved to.
fn gesture(ctx: &egui::Context, e: &mut Editor, from: [f64; 2], to: [f64; 2]) -> egui::FullOutput {
    let out = begin(ctx, e, from, to);
    let pointer = at(ctx, e, to);
    button(ctx, e, pointer, false);
    out
}
fn marker_seen(out: &egui::FullOutput) -> bool {
    super::object_snap_tests::marker(out).is_some()
}

#[test]
fn an_object_point_beats_a_guide_from_the_anchor_and_a_length_decides_what_it_keeps() {
    // The anchor is vertex 1 (30, 20), not the last vertex. The south guide from it passes
    // the middle (30, 10) of its edge and the end (30, 0) of vertex 2.
    let drawing = [["30", "20"], ["30", "0"], ["0", "0"]];
    let (ctx, mut e) = anchored(&drawing, "Orthogonal");
    switch(&ctx, &mut e);
    // No length: the object wins and writes its own exact point. The anchor itself is no
    // target, the ring stays where it was, and nothing says "guide".
    let out = gesture(&ctx, &mut e, [30., 20.], [30.25, 10.25]);
    assert!(marker_seen(&out) && painted(&out, "Midpoint"));
    assert!(target(&out).is_none() && ray(&out, GUIDE_COLOUR) == 0);
    assert_eq!(strings(&e, 3), pair("30", "10"));
    e.undo();

    // A refused length on this guide also refuses the nearby object: the text stays, the
    // label says so, and nothing is placed.
    type_length(&ctx, &mut e, "abc");
    let (before, steps) = (e.draft.clone(), history(&e));
    let out = begin(&ctx, &mut e, [30., 20.], [30.25, 10.25]);
    assert!(!marker_seen(&out) && target(&out).is_none());
    assert!(painted(&out, "270° · Length refused · no point added"));
    assert_eq!(e.draft, before, "a preview of a refusal is no point");
    let pointer = at(&ctx, &mut e, [30.25, 10.25]);
    button(&ctx, &mut e, pointer, false);
    assert_eq!(
        (e.draft.clone(), history(&e)),
        (before, steps),
        "and no step, Redo kept"
    );
    assert_eq!(e.canvas.length, "abc", "the text stays as typed");

    // A length that puts the point exactly on the middle keeps the object ...
    type_length(&ctx, &mut e, "10");
    let out = gesture(&ctx, &mut e, [30., 20.], [30.25, 10.25]);
    assert!(marker_seen(&out) && painted(&out, "Midpoint"));
    assert_eq!(strings(&e, 3), pair("30", "10"));
    e.undo();
    // ... one that does not is kept as the length, and the conflict is named. The point
    // written is the point drawn.
    type_length(&ctx, &mut e, "7");
    let out = begin(&ctx, &mut e, [30., 20.], [30.25, 10.25]);
    assert!(!marker_seen(&out), "no object marker for a point not taken");
    assert!(painted(
        &out,
        "270° · Length 7 mm from Endpoint · vertex 1 · adds vertex 4 · Midpoint is not at this length"
    ));
    let drawn = target(&out).expect("the length's point");
    let pointer = at(&ctx, &mut e, [30.25, 10.25]);
    button(&ctx, &mut e, pointer, false);
    assert_eq!(strings(&e, 3), pair("30", "13"));
    assert!(
        near(drawn, at(&ctx, &mut e, [30., 13.])),
        "drawn where it was written"
    );
    e.undo();
    // The grid never rounds a typed length (13 is not on the 5 mm grid), and Object snap
    // off changes nothing about it.
    choose(&ctx, &mut e, "5 mm");
    gesture(&ctx, &mut e, [30., 20.], [30.25, 10.25]);
    assert_eq!(strings(&e, 3), pair("30", "13"));
    e.undo();
    switch(&ctx, &mut e);
    gesture(&ctx, &mut e, [30., 20.], [30.25, 10.25]);
    assert_eq!(strings(&e, 3), pair("30", "13"));
    e.undo();

    // No length, Object snap off, grid 5 mm: a guided point is not rounded (8.25 stays),
    // and off every guide the grid rules the free point, as for a click.
    type_length(&ctx, &mut e, "");
    gesture(&ctx, &mut e, [30., 20.], [30.25, 8.25]);
    assert_eq!(strings(&e, 3), pair("30", "8.25"));
    e.undo();
    gesture(&ctx, &mut e, [30., 20.], [50.3125, 3.0625]);
    assert_eq!(strings(&e, 3), pair("50", "5"));
    e.undo();
    // Nearer the anchor than a guide can be held (12 points, minimum reach 16): no
    // direction, no length, no ray: the free point.
    type_length(&ctx, &mut e, "9");
    let out = begin(&ctx, &mut e, [30., 20.], [33., 20.5]);
    assert!(target(&out).is_none() && ray(&out, GUIDE_COLOUR) == 0);
    let pointer = at(&ctx, &mut e, [33., 20.5]);
    button(&ctx, &mut e, pointer, false);
    assert_eq!(
        strings(&e, 3),
        pair("35", "20"),
        "grid 5 mm, the length is unused here"
    );
    e.undo();
    // A tiny positive length is a length, shown in full, not read as zero.
    type_length(&ctx, &mut e, "0.0004");
    let out = gesture(&ctx, &mut e, [30., 20.], [60., 20.25]);
    assert!(painted_containing(
        &out,
        "0° · Length 0.0004 mm from Endpoint · vertex 1 · adds vertex 4"
    ));
    assert_eq!(strings(&e, 3), pair("30.0004", "20"));
}

#[test]
fn a_refused_length_moves_no_saved_vertex_even_beside_an_object_point() {
    let (ctx, mut e) = saved_anchored(&L, "Orthogonal");
    let v2 = vertex(&ctx, &mut e, 2);
    click(&ctx, &mut e, v2);
    assert_eq!(e.canvas.selected, Some(2));
    switch(&ctx, &mut e);
    type_length(&ctx, &mut e, "-5");
    let before = e.draft.clone();
    // Anchor vertex 5 (20, 40); west, 10.25 mm along, beside the middle (10, 40) of the edge
    // to vertex 6, which an object snap alone would take.
    let out = begin(&ctx, &mut e, [20., 40.], [10., 40.25]);
    assert!(!marker_seen(&out), "no object bypasses the refusal");
    assert!(painted(&out, "180° · Length refused · vertex 3 not moved"));
    assert_eq!(e.draft, before, "the vertex keeps its strings");
    let pointer = at(&ctx, &mut e, [10., 40.25]);
    button(&ctx, &mut e, pointer, false);
    assert_eq!((e.draft.clone(), history(&e)), (before, (0, 0)));
    assert_eq!(e.canvas.length, "-5");
    // Cleared, the same gesture lands on the object point and is one step.
    type_length(&ctx, &mut e, "");
    gesture(&ctx, &mut e, [20., 40.], [10., 40.25]);
    assert_eq!(strings(&e, 2), pair("10", "40"));
    assert_eq!(history(&e), (1, 0));
}

#[test]
fn escape_lost_focus_and_a_vanished_pointer_restore_the_strings_and_a_release_outside_ends_it() {
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    // Give Redo something to lose: a gesture, Undo, then the real ones.
    gesture(&ctx, &mut e, [0., 0.], [0.25, 22.5]);
    assert_eq!(strings(&e, 3), pair("0", "22.5"));
    e.undo();
    assert_eq!(history(&e), (0, 1));
    let rest = e.draft.clone();
    for cancel in ["Escape", "WindowFocused", "PointerGone"] {
        let out = begin(&ctx, &mut e, [0., 0.], [0.25, 22.5]);
        assert_eq!(
            strings(&e, 3),
            pair("0", "22.5"),
            "{cancel}: the gesture is live"
        );
        assert!(ring(&out).is_some());
        match cancel {
            "Escape" => escape(&ctx, &mut e),
            "WindowFocused" => {
                frame(&ctx, &mut e, vec![egui::Event::WindowFocused(false)]);
            }
            _ => {
                frame(&ctx, &mut e, vec![egui::Event::PointerGone]);
            }
        }
        assert_eq!(e.draft, rest, "{cancel}: the press-time strings are back");
        assert_eq!(history(&e), (0, 1), "{cancel}: Undo and Redo as they were");
        assert!(e.canvas.gesture.is_none(), "{cancel}");
        let out = frame(&ctx, &mut e, vec![]);
        assert!(
            target(&out).is_none() && ray(&out, GUIDE_COLOUR) == 0,
            "{cancel}: no guide left"
        );
        assert!(
            ring(&out).is_none_or(|p| !near(p, at(&ctx, &mut e, [0., 0.]))),
            "{cancel}: the confirmed ring is gone"
        );
        // The next release is not a ghost of the cancelled gesture.
        let pointer = at(&ctx, &mut e, [0.25, 22.5]);
        button(&ctx, &mut e, pointer, false);
        assert!(gesture_is_over(&e), "{cancel}");
        assert_eq!(
            (e.draft.clone(), history(&e)),
            (rest.clone(), (0, 1)),
            "{cancel}"
        );
        frame(&ctx, &mut e, vec![egui::Event::WindowFocused(true)]);
    }
    // Released outside the canvas the gesture still ends, once, on its guide.
    let spot = at(&ctx, &mut e, [0., 0.]);
    hover(&ctx, &mut e, spot);
    press(&ctx, &mut e, spot);
    let canvas = rect(&ctx, &mut e);
    let outside = egui::pos2(spot.x + 1., canvas.top() - 60.);
    move_to(&ctx, &mut e, outside);
    button(&ctx, &mut e, outside, false);
    assert!(gesture_is_over(&e));
    assert_eq!(history(&e), (1, 0), "a new step ends the old Redo");
    let y: f64 = strings(&e, 3)[1].parse().expect("a number");
    assert!(
        y > 55. && strings(&e, 3)[0] == "0",
        "north of the canvas, on the guide: {y}"
    );
    // A saved vertex: the same three ways back.
    let (ctx, mut e) = saved_anchored(&L, "Orthogonal");
    let v2 = vertex(&ctx, &mut e, 2);
    click(&ctx, &mut e, v2);
    let rest = e.draft.clone();
    for cancel in ["Escape", "PointerGone"] {
        begin(&ctx, &mut e, [20., 40.], [40.5, 40.25]);
        assert_ne!(e.draft, rest, "{cancel}: the vertex is live");
        if cancel == "Escape" {
            escape(&ctx, &mut e);
        } else {
            frame(&ctx, &mut e, vec![egui::Event::PointerGone]);
        }
        assert_eq!(
            (e.draft.clone(), history(&e), e.canvas.selected),
            (rest.clone(), (0, 0), Some(2))
        );
        // Escape cancels the preview while the physical button is still held.
        // Release it before the next hover/press gesture.
        let released = at(&ctx, &mut e, [40.5, 40.25]);
        button(&ctx, &mut e, released, false);
        assert_eq!((e.draft.clone(), history(&e)), (rest.clone(), (0, 0)));
    }
}

#[test]
fn packed_events_keep_their_order_and_the_settings_belong_to_the_press() {
    // Hover, press, move and release in ONE frame: the anchor is found by the move before
    // the press, in event order.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    let (v0, to) = (vertex(&ctx, &mut e, 0), at(&ctx, &mut e, [0.25, 22.5]));
    frame(&ctx, &mut e, drag_events(v0, to));
    assert_eq!(strings(&e, 3), pair("0", "22.5"));
    assert_eq!(history(&e), (1, 0));
    assert!(gesture_is_over(&e));

    // Two whole gestures in one frame: the second is anchored on the vertex the first
    // added, and each is its own step.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    let (v0, north) = (vertex(&ctx, &mut e, 0), at(&ctx, &mut e, [0., 20.]));
    let east = at(&ctx, &mut e, [15., 20.25]);
    let mut events = drag_events(v0, north);
    events.extend(drag_events(north, east));
    frame(&ctx, &mut e, events);
    assert_eq!(strings(&e, 3), pair("0", "20"));
    assert_eq!(
        strings(&e, 4),
        pair("15", "20"),
        "15 mm east of the first gesture's point"
    );
    assert_eq!(history(&e), (2, 0));
    e.undo();
    assert_eq!(e.draft.as_ref().expect("draft").points.len(), 4);
    e.undo();
    assert_eq!(e.draft.as_ref().expect("draft").points.len(), 3);

    // The mode turned off in the frame after the gesture: the gesture keeps what it was
    // pressed under; the next press is the old press.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    let out = frame(&ctx, &mut e, vec![]);
    let tick = text_at(&out, "From object");
    let (v0, to) = (vertex(&ctx, &mut e, 0), at(&ctx, &mut e, [0.25, 22.5]));
    let mut events = drag_events(v0, to);
    events.extend(drag_events(tick, tick));
    frame(&ctx, &mut e, events);
    assert!(!e.canvas.anchors, "the later click was accepted");
    assert_eq!(
        strings(&e, 3),
        pair("0", "22.5"),
        "and cannot rewrite the gesture"
    );
    assert_eq!(history(&e), (1, 0));
    let v1 = vertex(&ctx, &mut e, 1);
    press(&ctx, &mut e, v1);
    assert_eq!(
        e.canvas.selected,
        Some(1),
        "now the old press selects the vertex"
    );
    assert!(
        e.canvas
            .gesture
            .as_ref()
            .is_some_and(|g| g.anchored.is_none())
    );
    button(&ctx, &mut e, v1, false);

    // A key typed in the very frame of the press is read before the press: the box holds
    // "15" when the anchored gesture reads its length.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    type_length(&ctx, &mut e, "1");
    let (v0, to) = (vertex(&ctx, &mut e, 0), at(&ctx, &mut e, [0.25, 40.]));
    let mut events = vec![egui::Event::Text("5".into())];
    events.extend(drag_events(v0, to));
    frame(&ctx, &mut e, events);
    assert_eq!(e.canvas.length, "15");
    assert_eq!(strings(&e, 3), pair("0", "15"));

    // Held down, the Length box takes no text: the press owns the length it read.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    type_length(&ctx, &mut e, "15");
    begin(&ctx, &mut e, [0., 0.], [0.25, 40.]);
    frame(&ctx, &mut e, vec![egui::Event::Text("9".into())]);
    assert_eq!(e.canvas.length, "15", "and the box takes no text");
    assert_eq!(
        strings(&e, 3),
        pair("0", "15"),
        "the length of the press: 15 mm"
    );
    let release = at(&ctx, &mut e, [0.25, 40.]);
    button(&ctx, &mut e, release, false);
    assert_eq!(history(&e), (1, 0));
}

#[test]
fn off_busy_covered_and_unreadable_canvases_offer_no_anchor_and_typed_text_stays_exact() {
    // Mode off: the pointer on an end shows no anchor and no line.
    let (ctx, mut e) = open(&OPEN_L, false);
    guides(&ctx, &mut e, "Orthogonal");
    let v0 = vertex(&ctx, &mut e, 0);
    let out = hover(&ctx, &mut e, v0);
    assert!(provisional(&out).is_none() && standing(&out).is_none());

    // Busy: the whole form is disabled. The anchor shown a moment ago is gone, nothing is
    // grabbed, and the Length box takes no text.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    type_length(&ctx, &mut e, "15");
    let v0 = vertex(&ctx, &mut e, 0);
    assert!(
        provisional(&hover(&ctx, &mut e, v0)).is_some(),
        "idle: it is shown"
    );
    let before = e.draft.clone();
    frame_running(&ctx, &mut e, vec![], true);
    let out = frame_running(&ctx, &mut e, vec![egui::Event::PointerMoved(v0)], true);
    assert!(provisional(&out).is_none() && e.canvas.provisional.is_none());
    let north = at(&ctx, &mut e, [0.25, 22.5]);
    let mut events = vec![egui::Event::PointerMoved(v0)];
    for (pos, pressed) in [(v0, true), (north, false)] {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    frame_running(&ctx, &mut e, events, true);
    frame_running(&ctx, &mut e, vec![egui::Event::Text("7".into())], true);
    assert_eq!(e.draft, before);
    assert!(e.canvas.gesture.is_none() && e.undo.is_empty());
    assert_eq!(
        e.canvas.length, "15",
        "a job holds the form: no text reaches the box"
    );

    // Covered by another window: no anchor, no press through it.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    let v0 = vertex(&ctx, &mut e, 0);
    let before = e.draft.clone();
    let mut covered = |events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                e.draw(ui, true, false, "");
                egui::Area::new(egui::Id::new("cover-canvas-anchors"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(v0 - egui::vec2(60., 60.))
                    .show(ui.ctx(), |ui| {
                        ui.allocate_exact_size(
                            egui::vec2(120., 120.),
                            egui::Sense::click_and_drag(),
                        );
                    });
            },
        )
    };
    covered(vec![]);
    covered(vec![]);
    covered(vec![egui::Event::PointerMoved(v0)]);
    let out = covered(vec![]);
    assert!(provisional(&out).is_none());
    for pressed in [true, false] {
        covered(vec![egui::Event::PointerButton {
            pos: v0,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }]);
    }
    assert_eq!(e.draft, before, "nothing through the window");
    assert!(e.canvas.gesture.is_none());

    // Geometry that cannot be read offers nothing: a vertex that is not a number.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    let v0 = vertex(&ctx, &mut e, 0);
    assert!(provisional(&hover(&ctx, &mut e, v0)).is_some());
    replace_field(&ctx, &mut e, "40", "x");
    assert_eq!(strings(&e, 1), pair("x", "0"));
    let out = hover(&ctx, &mut e, v0);
    assert!(provisional(&out).is_none() && e.canvas.provisional.is_none());
    press(&ctx, &mut e, v0);
    assert!(
        e.canvas.gesture.is_none(),
        "no gesture over geometry that cannot be read"
    );
    button(&ctx, &mut e, v0, false);
    assert_eq!(
        strings(&e, 3),
        pair("0.000", "0.000"),
        "the press is the old click, as before"
    );

    // Closing the form, and the document changing under it, end the anchor: a saved form
    // goes with its document; a drawing for a new document stays, with its mode, without
    // the anchor.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    let v0 = vertex(&ctx, &mut e, 0);
    assert!(provisional(&hover(&ctx, &mut e, v0)).is_some());
    e.finish_session_change();
    assert!(e.drawing_new() && e.canvas.anchors && e.canvas.provisional.is_none());
    assert!(
        provisional(&hover(&ctx, &mut e, v0)).is_some(),
        "the pointer finds it again"
    );
    choose(&ctx, &mut e, "Cancel draft");
    assert!(!e.active() && e.canvas.provisional.is_none() && !e.canvas.anchors);
    let (ctx, mut e, _) = saved(&L);
    guides(&ctx, &mut e, "Orthogonal");
    from_object(&ctx, &mut e);
    let (v2, v4) = (vertex(&ctx, &mut e, 2), vertex(&ctx, &mut e, 4));
    click(&ctx, &mut e, v2);
    assert!(provisional(&hover(&ctx, &mut e, v4)).is_some());
    e.finish_session_change();
    assert!(
        !e.active() && e.canvas.provisional.is_none(),
        "the saved form ended with its document"
    );

    // Typed X/Y and Next X/Y bypass every snap and keep their text, with the mode on and an
    // anchor shown.
    let (ctx, mut e) = anchored(&[["0.50", "1"], ["33", "0"]], "Polar 45°");
    let v0 = vertex(&ctx, &mut e, 0);
    assert!(provisional(&hover(&ctx, &mut e, v0)).is_some());
    replace_field(&ctx, &mut e, "0.50", "16.500");
    assert_eq!(strings(&e, 0), pair("16.500", "1"), "typed text is exact");
    assert!(
        e.canvas.provisional.is_none(),
        "the pointer is on the box, not on the canvas"
    );
    e.next = [" 7.5 ".into(), "1e1".into()];
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Add point"));
    assert_eq!(strings(&e, 2), pair(" 7.5 ", "1e1"));
    // A reset must survive an idle redraw at the old pointer position. This is
    // the next frame after a document change or after parking/showing a tab form.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    let v0 = vertex(&ctx, &mut e, 0);
    assert!(provisional(&hover(&ctx, &mut e, v0)).is_some());
    e.finish_session_change();
    for _ in 0..2 {
        let out = frame(&ctx, &mut e, vec![]);
        assert!(
            provisional(&out).is_none(),
            "an idle redraw resurrected a cleared anchor"
        );
    }
    assert!(provisional(&hover(&ctx, &mut e, v0)).is_some());
    e.forget_anchor();
    assert!(provisional(&frame(&ctx, &mut e, vec![])).is_none());
    assert!(provisional(&hover(&ctx, &mut e, v0)).is_some());
    escape(&ctx, &mut e);
    assert!(
        provisional(&frame(&ctx, &mut e, vec![])).is_none(),
        "Escape ends a provisional anchor too"
    );
    assert!(provisional(&hover(&ctx, &mut e, v0)).is_some());
    let out = frame(&ctx, &mut e, vec![egui::Event::WindowFocused(false)]);
    assert!(
        provisional(&out).is_none(),
        "focus loss reacquired the anchor in the same frame"
    );
    assert!(provisional(&frame(&ctx, &mut e, vec![])).is_none());
}

#[test]
fn a_result_that_breaks_the_contour_stays_a_draft_and_restore_keeps_the_mode() {
    // New: 20 mm south of the anchor (20, 20) lands on the existing vertex (20, 0).
    let (ctx, mut e) = anchored(&[["20", "20"], ["20", "0"], ["0", "0"]], "Orthogonal");
    type_length(&ctx, &mut e, "20");
    gesture(&ctx, &mut e, [20., 20.], [20.25, 5.]);
    assert_eq!(strings(&e, 3), pair("20", "0"), "placed, and drawn");
    e.draft.as_mut().expect("draft").closed = true;
    let Err(refusal) = e.content() else {
        panic!("a repeated vertex is refused");
    };
    let refusal = refusal.to_string();
    assert!(
        painted_containing(&frame(&ctx, &mut e, vec![]), &refusal),
        "{refusal}"
    );

    // Saved: vertex 3 pulled to the anchor (20, 40) is two vertices in one place; the
    // draft shows it and the shared policy refuses it. Restore returns the numbers and
    // leaves the editor's settings alone.
    let (ctx, mut e) = saved_anchored(&L, "Orthogonal");
    let original = e.draft.clone();
    let v2 = vertex(&ctx, &mut e, 2);
    click(&ctx, &mut e, v2);
    type_length(&ctx, &mut e, "25");
    gesture(&ctx, &mut e, [20., 20.], [20.25, 50.]);
    assert_eq!(strings(&e, 2), pair("20", "45"));
    assert_eq!(history(&e), (1, 0));
    choose(&ctx, &mut e, "Restore saved vertices");
    assert_eq!(e.draft, original);
    assert_eq!(
        (e.canvas.anchors, e.canvas.guides, e.canvas.length.as_str()),
        (true, guide::Mode::Orthogonal, "25"),
        "restoring touches the vertices only"
    );
}

#[test]
fn one_and_two_pixels_per_point_and_other_canvas_scales_hold_the_same_anchor() {
    // The same gesture at 1x and 2x: only logical points are compared.
    let run = |pixels_per_point: f32| {
        let ctx = context();
        ctx.set_pixels_per_point(pixels_per_point);
        let mut e = Editor::default();
        e.begin();
        let draft = e.draft.as_mut().expect("draft");
        draft.points = OPEN_L.iter().map(|p| p.map(str::to_owned)).collect();
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        guides(&ctx, &mut e, "Orthogonal");
        from_object(&ctx, &mut e);
        let from = vertex(&ctx, &mut e, 0);
        let shown = hover(&ctx, &mut e, from + egui::vec2(2., -2.));
        let ring_at = provisional(&shown).map(|p| p - from);
        press(&ctx, &mut e, from + egui::vec2(2., -2.));
        let to = at(&ctx, &mut e, [0.25, 22.5]);
        let out = move_to(&ctx, &mut e, to);
        let seen = (strings(&e, 3), ring(&out), target(&out).map(|p| p - from));
        let label = label_at(
            &out,
            "90° · 22.5 mm from Endpoint · vertex 1 · adds vertex 4",
        )
        .map(|p| p - from);
        button(&ctx, &mut e, to, false);
        (seen, ring_at, label, e.draft.clone(), e.undo.len(), from)
    };
    let (one, two) = (run(1.), run(2.));
    assert_eq!((&one.0.0, &one.3, one.4), (&two.0.0, &two.3, two.4));
    assert_eq!(one.0.0, pair("0", "22.5"));
    for run in [&one, &two] {
        assert!(
            run.0.1.is_some_and(|p| near(p, run.5)),
            "the anchor is the pressed vertex"
        );
    }
    let (r1, r2) = (one.1.expect("ring"), two.1.expect("ring"));
    assert!(r1.length() < 0.01 && r2.length() < 0.01, "{r1:?} {r2:?}");
    let (t1, t2) = (one.0.2.expect("target"), two.0.2.expect("target"));
    assert!((t1 - t2).length() < 0.01, "target {t1:?} against {t2:?}");
    let (a, b) = (one.2.expect("label"), two.2.expect("label"));
    assert!((a - b).length() < 1., "label {a:?} against {b:?}");

    // The aperture is 12 points at every canvas scale: 11 acquire, 14 do not, whether a
    // millimetre is 4 points or well under 2.
    let (default_view, far) = (
        [["0", "0"], ["40", "0"], ["100", "50"]],
        [["0", "0"], ["40", "0"], ["400", "300"]],
    );
    let mut scales = Vec::new();
    for fit in [false, true] {
        let drawing = if fit { &far } else { &default_view };
        let (ctx, mut e) = open(drawing, false);
        guides(&ctx, &mut e, "Orthogonal");
        from_object(&ctx, &mut e);
        if fit {
            choose(&ctx, &mut e, "Fit drawing");
        }
        let scale = e.canvas.scale;
        scales.push(scale);
        let last = drawing[2].map(|v| v.parse::<f64>().expect("number"));
        let anchor = at(&ctx, &mut e, last);
        for (away, held) in [(14.0_f32, false), (11.0, true)] {
            let out = hover(&ctx, &mut e, anchor + egui::vec2(-away, 0.));
            assert_eq!(
                provisional(&out).is_some(),
                held,
                "{away} points away at {scale} points per mm"
            );
        }
        frame(&ctx, &mut e, vec![egui::Event::PointerGone]);
    }
    assert!(scales[0] > 3. * scales[1], "the scales differ: {scales:?}");
}

#[test]
fn the_form_keeps_apply_and_save_in_reach_with_the_mode_on_at_768_points() {
    let (_root, path, reading) = crate::fillets::tests::rounded_n(&[
        ([33., 3.25], 2.375),
        ([-4.5, 15.5], 3.0625),
        ([-4.5, 3.25], 1.5),
        ([33., 15.5], 6.0),
    ]);
    let choice = reading.sketches[0].clone();
    let mut e = Editor::default();
    assert!(e.begin_edit(&path, &reading, choice.sketch));
    let ctx = context();
    frame(&ctx, &mut e, vec![]);
    frame(&ctx, &mut e, vec![]);
    let measure = |ctx: &egui::Context, out: &egui::FullOutput| {
        let (save, apply) = (
            text_at(out, "Save edited copy…"),
            text_at(out, "Apply vertices"),
        );
        (save.y.max(apply.y), ctx.globally_used_rect())
    };
    let off = measure(&ctx, &frame(&ctx, &mut e, vec![]));
    guides(&ctx, &mut e, "Orthogonal");
    let guided = measure(&ctx, &frame(&ctx, &mut e, vec![]));
    from_object(&ctx, &mut e);
    // The longest standing line: a selected vertex and a shown anchor with its words.
    let first = vertex(&ctx, &mut e, 0);
    click(&ctx, &mut e, first);
    assert_eq!(e.canvas.selected, Some(0));
    let other = vertex(&ctx, &mut e, 1);
    let out = hover(&ctx, &mut e, other);
    assert!(provisional(&out).is_some(), "an anchor is shown");
    let line = standing(&out).expect("the standing line");
    assert!(line.contains("press to start"), "{line}");
    assert!(
        standing_fits(&out, rect(&ctx, &mut e)),
        "the words fit the canvas: {line}"
    );
    let on = measure(&ctx, &out);
    assert!(on.0 < 768. && on.1.max.y <= 768., "{on:?}");
    assert!(on.1.width() <= 700., "{:?}", on.1);
    // The mode costs the form no height: the words are on the canvas.
    assert_eq!(
        on.1.max.y, guided.1.max.y,
        "off {off:?}, guided {guided:?}, on {on:?}"
    );
}

#[test]
fn a_new_contour_closed_from_an_anchor_hands_the_create_job_exactly_the_polygon_it_shows() {
    // The New path adds a vertex where the saved path moves one; what leaves the form is
    // still the typed polygon, checked here before the shared create job.
    let (ctx, mut e) = anchored(&OPEN_L, "Orthogonal");
    gesture(&ctx, &mut e, [0., 0.], [0.25, 30.]);
    assert_eq!(strings(&e, 3), pair("0", "30"));
    choose(&ctx, &mut e, "Close contour");
    e.set_create(true);
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, CREATE_DRAWN));
    let Some(NewDocument::SketchExtrude(made)) = e.take_request() else {
        panic!("a polygon extrusion was asked for");
    };
    let literal = PolygonExtrusion::new(vec![[0., 0.], [40., 0.], [40., 30.], [0., 30.]], 10.)
        .expect("a rectangle");
    assert_eq!(made, literal, "the numbers, not the pixels");
}

/// §31D-2, for the tests of the owners around the editor: whether the mode is on and
/// whether an anchor is live.
pub(crate) fn anchor_state(e: &Editor) -> (bool, bool) {
    (e.canvas.anchors, e.canvas.provisional.is_some())
}

/// A saved Sketch form with the mode on and the pointer resting on vertex 1 while vertex
/// 2 is selected, through the widgets. Whether the anchor is live.
pub(crate) fn rest_on_an_anchor(e: &mut Editor) -> bool {
    let ctx = context();
    frame(&ctx, e, vec![]);
    frame(&ctx, e, vec![]);
    choose(&ctx, e, "Fit drawing");
    if !e.canvas.anchors {
        guides(&ctx, e, "Orthogonal");
        from_object(&ctx, e);
    }
    let moving = vertex(&ctx, e, 1);
    click(&ctx, e, moving);
    let anchor = vertex(&ctx, e, 0);
    hover(&ctx, e, anchor);
    anchor_state(e).1
}

/// §31D-2: through the widgets, a saved Sketch's vertex 2 moved from vertex 1 as the anchor:
/// Fit, Orthogonal, the Length box, From object, a click that selects vertex 2, then a press
/// on the shown anchor dragged south and released; then Apply vertices. The form and the
/// request it asked for.
pub(crate) fn anchored_apply(
    path: &Path,
    reading: &ExtrudeEditSource,
    sketch: ferritecad_types::ObjectId,
    length: &str,
) -> (Editor, EditRequest) {
    let mut e = Editor::default();
    e.set_session(true, true, false);
    assert!(e.begin_edit(path, reading, sketch));
    let ctx = context();
    frame(&ctx, &mut e, vec![]);
    frame(&ctx, &mut e, vec![]);
    choose(&ctx, &mut e, "Fit drawing");
    guides(&ctx, &mut e, "Orthogonal");
    type_length(&ctx, &mut e, length);
    from_object(&ctx, &mut e);
    let moving = vertex(&ctx, &mut e, 1);
    click(&ctx, &mut e, moving);
    assert_eq!(e.canvas.selected, Some(1));
    let anchor = vertex(&ctx, &mut e, 0);
    let spot = anchor + egui::vec2(2., -2.);
    let out = hover(&ctx, &mut e, spot);
    assert!(
        provisional(&out).is_some_and(|p| near(p, anchor)),
        "the anchor is shown"
    );
    press(&ctx, &mut e, spot);
    // South of the anchor, 90 points along its guide and three off the line.
    let south = anchor + egui::vec2(3., 90.);
    move_to(&ctx, &mut e, south);
    button(&ctx, &mut e, south, false);
    assert_eq!(
        e.canvas.selected,
        Some(1),
        "the selection stayed on the moved vertex"
    );
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Apply vertices"));
    let request = e.take_apply_request().expect("Apply vertices asked");
    (e, request)
}
