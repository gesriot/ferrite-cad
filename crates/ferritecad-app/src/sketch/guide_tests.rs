// SPDX-License-Identifier: MIT
//! §31D-1: gesture guides and the temporary Length through the production canvas
//! and editor. Real egui pointer, key and text events go in; draft strings, history
//! and what was painted come out. No window and no graphics device, and nothing
//! here calls a private helper in place of the interface.
#![allow(clippy::panic)]
use super::{
    drag_tests::{button, key, move_to, press, vertex},
    object_snap_tests::{
        L, ZIGZAG, at, choose, context, hover, label_at, marker, open, painted, painted_containing,
        pair, rect, saved, strings, switch,
    },
    tests::{click, frame, frame_running, replace_field, text_at},
    *,
};

/// Every painted text equal to `text`, in paint order.
pub(super) fn texts(out: &egui::FullOutput, text: &str) -> Vec<egui::Pos2> {
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text() == text => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .collect()
}
/// A choice in the Guides row (its "Off" is not the grid step's).
pub(super) fn guides(ctx: &egui::Context, e: &mut Editor, label: &str) {
    let out = frame(ctx, e, vec![]);
    let row = text_at(&out, "Guides:").y;
    let spot = texts(&out, label)
        .into_iter()
        .find(|p| (p.y - row).abs() < 3.)
        .unwrap_or_else(|| panic!("{label} is in the Guides row"));
    click(ctx, e, spot);
}
/// Where the Length mm box is: just right of its label.
fn length_box(out: &egui::FullOutput) -> egui::Pos2 {
    let name = out
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Text(t) if t.galley.text() == "Length mm" => {
                Some(t.visual_bounding_rect())
            }
            _ => None,
        })
        .expect("Length mm is painted");
    egui::pos2(name.right() + 8. + 35., name.center().y)
}
/// Replaces the Length box's text through the real widget: click, select all, type.
pub(super) fn type_length(ctx: &egui::Context, e: &mut Editor, value: &str) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, length_box(&out));
    let mut events = vec![
        egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        },
        key(egui::Key::Backspace),
    ];
    if !value.is_empty() {
        events.push(egui::Event::Text(value.into()));
    }
    frame(ctx, e, events);
}
pub(super) fn circle(
    out: &egui::FullOutput,
    colour: egui::Color32,
    radius: f32,
) -> Option<egui::Pos2> {
    out.shapes.iter().find_map(|c| match &c.shape {
        egui::Shape::Circle(s) if s.stroke.color == colour && (s.radius - radius).abs() < 0.01 => {
            Some(s.center)
        }
        _ => None,
    })
}
/// The anchor ring and the target circle of a guide, apart from each other.
pub(super) fn ring(out: &egui::FullOutput) -> Option<egui::Pos2> {
    circle(out, GUIDE_COLOUR, 8.)
}
pub(super) fn target(out: &egui::FullOutput) -> Option<egui::Pos2> {
    circle(out, GUIDE_COLOUR, 4.)
}
/// How many dashes of the guide ray are painted in `colour`.
pub(super) fn ray(out: &egui::FullOutput, colour: egui::Color32) -> usize {
    out.shapes
        .iter()
        .filter(|c| matches!(&c.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == colour && stroke.width == 1.))
        .count()
}
/// Nothing of any guide is painted: no ring, ray, target or words.
pub(super) fn no_guide_shown(out: &egui::FullOutput) -> bool {
    ring(out).is_none()
        && target(out).is_none()
        && ray(out, GUIDE_COLOUR) == 0
        && ray(out, GUIDE_REFUSED) == 0
        && !painted_containing(out, "mm from vertex")
        && !painted_containing(out, "from its start")
        && !painted_containing(out, "Length refused")
}
/// A click at a document point (mm).
fn click_mm(ctx: &egui::Context, e: &mut Editor, mm: [f64; 2]) {
    let spot = at(ctx, e, mm);
    click(ctx, e, spot);
}
pub(super) fn near(a: egui::Pos2, b: egui::Pos2) -> bool {
    a.distance(b) < 0.01
}
pub(super) fn gesture_is_over(e: &Editor) -> bool {
    e.canvas.gesture.is_none() && !e.canvas.claimed_press
}
/// An open drawing with Guides chosen, the way a user does.
fn guided(points: &[[&str; 2]], closed: bool, mode: &str) -> (egui::Context, Editor) {
    let (ctx, mut e) = open(points, closed);
    guides(&ctx, &mut e, mode);
    (ctx, e)
}
pub(super) fn history(e: &Editor) -> (usize, usize) {
    (e.undo.len(), e.redo.len())
}

#[test]
fn the_guides_row_follows_the_snap_row_and_changes_only_editor_state() {
    let (ctx, mut e) = open(&ZIGZAG, false);
    let out = frame(&ctx, &mut e, vec![]);
    let (snap, row) = (text_at(&out, "Snap:"), text_at(&out, "Guides:"));
    assert!(row.y > snap.y + 5., "its own row, under the grid step's");
    for name in ["Orthogonal", "Polar 45°", "Length mm"] {
        assert!(painted(&out, name), "{name}");
    }
    assert_eq!(
        (e.canvas.guides, e.canvas.length.as_str()),
        (guide::Mode::Off, ""),
        "off and blank for a new editor, like the grid step"
    );
    assert!(painted_containing(&out, "guides: Off"));
    assert!(painted_containing(&out, "Guides are off"));
    assert!(no_guide_shown(&out), "Off draws no anchor");
    // While Off the box is inert: clicking it and typing changes nothing.
    type_length(&ctx, &mut e, "12");
    assert_eq!(e.canvas.length, "");

    let before = e.draft.clone();
    guides(&ctx, &mut e, "Orthogonal");
    assert_eq!(e.canvas.guides, guide::Mode::Orthogonal);
    assert_eq!(e.draft, before, "choosing moves nothing");
    assert_eq!(history(&e), (0, 0), "and is no Undo step");
    let out = frame(&ctx, &mut e, vec![]);
    assert!(painted_containing(&out, "guides: Orthogonal"));
    assert!(
        painted_containing(&out, "ringed anchor"),
        "the standing note"
    );
    type_length(&ctx, &mut e, "12.5");
    assert_eq!(e.canvas.length, "12.5");
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        painted_containing(&out, "length: 12.5 mm"),
        "in the caption"
    );
    assert!(
        painted_containing(&out, "Length 12.5 mm is used for every point or drag"),
        "the note says it stays until cleared"
    );
    assert_eq!(e.draft, before);
    assert_eq!(history(&e), (0, 0), "typing the length is not geometry");

    guides(&ctx, &mut e, "Polar 45°");
    assert_eq!(e.canvas.guides, guide::Mode::Polar);
    guides(&ctx, &mut e, "Off");
    assert_eq!(e.canvas.guides, guide::Mode::Off);
    assert_eq!(e.canvas.length, "12.5", "the text stays; it is only unused");
    let out = frame(&ctx, &mut e, vec![]);
    assert!(!painted_containing(&out, "length: 12.5"));
    assert_eq!(e.draft, before);
    assert_eq!(history(&e), (0, 0));
}

#[test]
fn a_new_point_follows_a_shown_guide_from_the_last_vertex_with_the_anchor_drawn_apart() {
    // The last vertex's y is written `0.0`: an axis guide keeps that very text.
    let (ctx, mut e) = open(&[["0", "0"], ["20", "0.0"]], false);
    assert!(no_guide_shown(&frame(&ctx, &mut e, vec![])));
    guides(&ctx, &mut e, "Orthogonal");
    let anchor = at(&ctx, &mut e, [20., 0.]);
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        ring(&out).is_some_and(|p| near(p, anchor)),
        "the anchor is shown at once"
    );
    assert!(
        target(&out).is_none() && ray(&out, GUIDE_COLOUR) == 0,
        "no guide yet"
    );

    // East, 30.5 mm along and three points off the line.
    let pointer = at(&ctx, &mut e, [50.5, 0.75]);
    let before = e.draft.clone();
    let out = hover(&ctx, &mut e, pointer);
    assert!(
        ring(&out).is_some_and(|p| near(p, anchor)),
        "the anchor does not follow"
    );
    let shown = target(&out).expect("the point the guide gives");
    assert!(
        near(shown, at(&ctx, &mut e, [50.5, 0.])),
        "on the guide, not at the pointer"
    );
    assert!(ray(&out, GUIDE_COLOUR) > 3, "the ray is drawn");
    let words = label_at(&out, "0° · 30.5 mm from vertex 2").expect("the label");
    assert!(
        words.x > pointer.x && words.y > pointer.y,
        "beside the pointer"
    );
    assert_eq!(e.draft, before, "hovering edits nothing");
    assert_eq!(history(&e), (0, 0));

    click(&ctx, &mut e, pointer);
    assert_eq!(
        strings(&e, 2),
        pair("50.5", "0.0"),
        "the anchor's own text on the other axis"
    );
    assert_eq!(history(&e), (1, 0), "one point, one Undo");
    let added = e.draft.clone();
    e.undo();
    assert_eq!(e.draft, before);
    e.redo();
    assert_eq!(e.draft, added);

    // Now the new vertex is the anchor; north of it, 25.5 mm.
    let pointer = at(&ctx, &mut e, [50.75, 25.5]);
    let out = hover(&ctx, &mut e, pointer);
    assert!(ring(&out).is_some_and(|p| near(p, at(&ctx, &mut e, [50.5, 0.]))));
    assert!(painted(&out, "90° · 25.5 mm from vertex 3"));
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 3), pair("50.5", "25.5"));

    // Far from every guide the ordinary free point is placed, and nothing is forced.
    let free = at(&ctx, &mut e, [20., 15.]);
    let out = hover(&ctx, &mut e, free);
    assert!(target(&out).is_none() && ray(&out, GUIDE_COLOUR) == 0);
    assert!(ring(&out).is_some(), "the anchor stays, the guide does not");
    click(&ctx, &mut e, free);
    assert_eq!(strings(&e, 4), pair("20.000", "15.000"));
}

#[test]
fn polar_diagonals_and_the_first_point_and_what_the_canvas_does_not_show() {
    // Polar adds the diagonal: projected on the guide, so both offsets are equal.
    let (ctx, mut e) = guided(&[["0", "0"], ["20", "0"]], false, "Polar 45°");
    let pointer = at(&ctx, &mut e, [40., 20.125]);
    let out = hover(&ctx, &mut e, pointer);
    assert!(painted(&out, "45° · 28.373 mm from vertex 2"));
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 2), pair("40.0625", "20.0625"));
    // Orthogonal offers no diagonal.
    let (ctx, mut e) = guided(&[["0", "0"], ["20", "0"]], false, "Orthogonal");
    let pointer = at(&ctx, &mut e, [40., 20.125]);
    let out = hover(&ctx, &mut e, pointer);
    assert!(target(&out).is_none());
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 2), pair("40.000", "20.125"));

    // No anchor before the first point: Polar and a length change nothing there.
    let (ctx, mut e) = open(&[], false);
    guides(&ctx, &mut e, "Polar 45°");
    type_length(&ctx, &mut e, "5");
    let pointer = at(&ctx, &mut e, [10.25, 5.5]);
    assert!(no_guide_shown(&hover(&ctx, &mut e, pointer)));
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 0), pair("10.250", "5.500"));
    assert_eq!(history(&e), (1, 0));
    // Now there is one, and the length applies from it.
    let pointer = at(&ctx, &mut e, [50., 5.625]);
    click(&ctx, &mut e, pointer);
    assert_eq!(
        strings(&e, 1),
        pair("15.25", "5.500"),
        "5 mm east of the first point, the anchor's own y text"
    );

    // A closed drawing adds no point, so it shows no anchor for one.
    let (ctx, mut e) = guided(&[["0", "0"], ["20", "0"], ["20", "20"]], true, "Polar 45°");
    let pointer = at(&ctx, &mut e, [40., 20.]);
    let out = hover(&ctx, &mut e, pointer);
    assert!(no_guide_shown(&out));
    // An anchor the canvas does not show (x = 300 is far outside) offers no guide.
    let (ctx, mut e) = guided(&[["0", "0"], ["300", "0"]], false, "Orthogonal");
    let pointer = at(&ctx, &mut e, [50., 0.5]);
    assert!(no_guide_shown(&hover(&ctx, &mut e, pointer)));
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 2), pair("50.000", "0.500"));
    // A last vertex that is not a number is no anchor either.
    let (ctx, mut e) = guided(&[["0", "0"], ["x", "0"]], false, "Orthogonal");
    assert!(no_guide_shown(&frame(&ctx, &mut e, vec![])));

    // Every feature that draws a Line profile shares the canvas: a Revolve profile is
    // guided too, and its axis of revolution is still named.
    let (ctx, mut e) = open(&[["2", "0"], ["12", "0"]], false);
    e.draft.as_mut().expect("draft").feature = Feature::Revolve;
    guides(&ctx, &mut e, "Orthogonal");
    let pointer = at(&ctx, &mut e, [12.25, 10.5]);
    let out = hover(&ctx, &mut e, pointer);
    assert!(painted(&out, "90° · 10.5 mm from vertex 2"));
    assert!(painted_containing(&out, "axis (Y) at X = 0"));
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 2), pair("12", "10.5"));
}

#[test]
fn a_typed_length_places_the_exact_point_and_one_that_cannot_be_used_places_nothing() {
    let (ctx, mut e) = guided(&[["0", "0"], ["10", "0"]], false, "Polar 45°");
    type_length(&ctx, &mut e, "7.5");
    // North, 30 mm along at the pointer; the length is 7.5.
    let pointer = at(&ctx, &mut e, [10.25, 30.]);
    let out = hover(&ctx, &mut e, pointer);
    assert!(painted(&out, "90° · Length 7.5 mm from vertex 2"));
    assert!(
        target(&out).is_some_and(|p| near(p, at(&ctx, &mut e, [10., 7.5]))),
        "the length, not the pointer"
    );
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 2), pair("10", "7.5"));
    assert_eq!(e.canvas.length, "7.5", "the field is not consumed");
    // 10 mm at 45° from (10, 7.5), to a picometre and never an f64 artefact.
    type_length(&ctx, &mut e, "10");
    click_mm(&ctx, &mut e, [24., 21.5625]);
    assert_eq!(strings(&e, 3), pair("17.071067812", "14.571067812"));
    // The length is added to the anchor in decimal: 0.1 + 0.2 reads 0.3.
    let (ctx, mut e) = guided(&[["0", "0"], ["0.1", "0"]], false, "Orthogonal");
    type_length(&ctx, &mut e, "0.2");
    click_mm(&ctx, &mut e, [20., 0.]);
    assert_eq!(strings(&e, 2), pair("0.3", "0"));

    // Anything that is not a positive finite number stays as typed, is shown as a
    // refusal, and places nothing; it never becomes zero or a free distance.
    let (ctx, mut e) = guided(&[["0", "0"], ["10", "0"]], false, "Polar 45°");
    for text in [
        "0", "-3", "NaN", "inf", "abc", "1e999", "12 mm", "1,5", "-0",
    ] {
        type_length(&ctx, &mut e, text);
        assert_eq!(e.canvas.length, text, "the text stays exactly as typed");
        let before = e.draft.clone();
        let pointer = at(&ctx, &mut e, [10.25, 30.]);
        let out = hover(&ctx, &mut e, pointer);
        assert!(
            painted(&out, "90° · Length refused · no point added"),
            "{text}"
        );
        assert!(
            ray(&out, GUIDE_REFUSED) > 3 && target(&out).is_none(),
            "{text}"
        );
        assert!(
            painted_containing(&out, "is refused: use a positive number"),
            "{text}: the standing note says it too"
        );
        assert!(
            painted_containing(&out, "length: refused"),
            "{text}: and the caption"
        );
        click(&ctx, &mut e, pointer);
        assert_eq!(e.draft, before, "{text}: nothing was placed");
        assert_eq!(history(&e), (0, 0), "{text}");
        assert_eq!(e.canvas.length, text);
    }
    // Off a guide the refused length is not in use: the free point is placed.
    let free = at(&ctx, &mut e, [20., 15.]);
    click(&ctx, &mut e, free);
    assert_eq!(strings(&e, 2), pair("20.000", "15.000"));
    // Clearing it brings the free distance back.
    type_length(&ctx, &mut e, "");
    click_mm(&ctx, &mut e, [20.25, 40.]);
    assert_eq!(
        strings(&e, 3),
        pair("20.000", "40"),
        "free distance, the anchor's text"
    );
}

#[test]
fn an_object_point_beats_a_guide_and_only_a_matching_length_keeps_it() {
    // Anchor C (30, 20); the south guide passes the middle (30, 10) of the edge B–C
    // and the end B (30, 0).
    let drawing = [["0", "0"], ["30", "0"], ["30", "20"]];
    let (ctx, mut e) = guided(&drawing, false, "Orthogonal");
    switch(&ctx, &mut e);
    let pointer = at(&ctx, &mut e, [30.25, 10.25]);
    // No length: the object wins, takes its own exact point and the guide is not
    // the accepted source.
    let out = hover(&ctx, &mut e, pointer);
    assert!(marker(&out).is_some() && painted(&out, "Midpoint"));
    assert!(target(&out).is_none() && ray(&out, GUIDE_COLOUR) == 0);
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 3), pair("30", "10"));
    e.undo();

    // A refused length on this guide must also refuse a nearby object hit.
    type_length(&ctx, &mut e, "abc");
    let before = e.draft.clone();
    let before_history = history(&e);
    let out = hover(&ctx, &mut e, pointer);
    assert!(marker(&out).is_none() && target(&out).is_none());
    assert!(painted(&out, "270° · Length refused · no point added"));
    click(&ctx, &mut e, pointer);
    assert_eq!(
        e.draft, before,
        "a refused length cannot place an object point"
    );
    assert_eq!(history(&e), before_history, "including the existing Redo");

    // A length that puts the point exactly on that middle keeps the object.
    type_length(&ctx, &mut e, "10");
    let out = hover(&ctx, &mut e, pointer);
    assert!(marker(&out).is_some() && painted(&out, "Midpoint"));
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 3), pair("30", "10"));
    e.undo();

    // A length that does not: the length is kept and the conflict shown. The
    // object is not drawn as accepted, and the point written is the point drawn.
    type_length(&ctx, &mut e, "7");
    let out = hover(&ctx, &mut e, pointer);
    assert!(
        marker(&out).is_none(),
        "no object marker for a point not taken"
    );
    assert!(painted(
        &out,
        "270° · Length 7 mm from vertex 3 · Midpoint is not at this length"
    ));
    let drawn = target(&out).expect("the length's point");
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 3), pair("30", "13"));
    assert!(
        near(drawn, at(&ctx, &mut e, [30., 13.])),
        "drawn where it was written"
    );
    e.undo();

    // The grid never rounds a typed length either (13 is not on the 5 mm grid).
    choose(&ctx, &mut e, "5 mm");
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 3), pair("30", "13"));
    // Object snap off: the same typed length, the same point.
    e.undo();
    switch(&ctx, &mut e);
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 3), pair("30", "13"));
}

#[test]
fn the_grid_step_is_not_applied_to_a_guided_point_but_still_rules_a_free_one() {
    let (ctx, mut e) = guided(&[["0", "0"], ["20", "0"]], false, "Orthogonal");
    choose(&ctx, &mut e, "5 mm");
    assert!(painted_containing(
        &frame(&ctx, &mut e, vec![]),
        "not applied to guided points"
    ));
    // On the guide: 33.25 stays 33.25 (the grid would say 35).
    click_mm(&ctx, &mut e, [33.25, 0.25]);
    assert_eq!(strings(&e, 2), pair("33.25", "0"));
    // Off every guide: the grid rounds as before.
    click_mm(&ctx, &mut e, [60.3125, 23.0625]);
    assert_eq!(strings(&e, 3), pair("60", "25"));
}

#[test]
fn a_saved_vertex_drag_measures_from_where_it_was_pressed_and_keeps_that_anchor() {
    let (ctx, mut e, _) = saved(&L);
    guides(&ctx, &mut e, "Orthogonal");
    type_length(&ctx, &mut e, "15");
    let before = e.draft.clone();
    // Pressed three points off the vertex centre (the grab offset is kept).
    let centre = vertex(&ctx, &mut e, 2);
    let from = centre + egui::vec2(2., -2.);
    press(&ctx, &mut e, from);
    // West, 72 points (18 mm) along and one point off the line.
    let pointer = from + egui::vec2(-72., 1.);
    let out = move_to(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 2), pair("45", "20"), "15 mm west of (60, 20)");
    assert!(
        ring(&out).is_some_and(|p| near(p, centre)),
        "the anchor is the press-time vertex"
    );
    assert!(target(&out).is_some_and(|p| near(p, at(&ctx, &mut e, [45., 20.]))));
    let words = label_at(&out, "180° · Length 15 mm · vertex 3 moved from its start")
        .expect("a movement from the start, not an edge length");
    assert!(words.x > pointer.x);
    button(&ctx, &mut e, pointer, false);
    assert!(gesture_is_over(&e));
    assert_eq!(history(&e), (1, 0), "one drag, one Undo");
    let dragged = e.draft.clone();
    assert!(
        no_guide_shown(&frame(&ctx, &mut e, vec![])),
        "release clears the gesture's guide"
    );
    e.undo();
    assert_eq!(e.draft, before);
    e.redo();
    assert_eq!(e.draft, dragged);
    assert_eq!(e.canvas.length, "15", "the field is left as it was typed");
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
        [45., 20.]
    );

    // Without a length the pointer chooses the distance, however far: 65 mm west
    // is 260 points, far beyond twice the aperture, and the guide still holds.
    type_length(&ctx, &mut e, "");
    choose(&ctx, &mut e, "Restore saved vertices");
    let centre = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, centre);
    let far = at(&ctx, &mut e, [-5., 20.25]);
    let out = move_to(&ctx, &mut e, far);
    assert_eq!(strings(&e, 2), pair("-5", "20"));
    assert!(
        ring(&out).is_some_and(|p| near(p, centre)),
        "the anchor stayed behind"
    );
    assert!(painted(&out, "180° · vertex 3 moved 65 mm from its start"));
    // Off the guide by more than the aperture the vertex is simply free.
    let off = at(&ctx, &mut e, [-5., 24.]);
    let out = move_to(&ctx, &mut e, off);
    assert_eq!(strings(&e, 2), pair("-5", "24"));
    assert!(target(&out).is_none() && ray(&out, GUIDE_COLOUR) == 0);
    // Back on it, the vertex returns to the anchor's own y.
    move_to(&ctx, &mut e, far);
    assert_eq!(strings(&e, 2), pair("-5", "20"));
    button(&ctx, &mut e, far, false);
    assert_eq!(
        history(&e),
        (3, 0),
        "the drag, Restore saved vertices and this drag"
    );

    // A press that did not move has no direction: no change, no step, no ring left.
    let (ctx, mut e, _) = saved(&L);
    guides(&ctx, &mut e, "Polar 45°");
    type_length(&ctx, &mut e, "15");
    let before = e.draft.clone();
    let at_vertex = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, at_vertex);
    let out = move_to(&ctx, &mut e, at_vertex + egui::vec2(3., 0.));
    assert!(
        target(&out).is_none(),
        "a vector shorter than the minimum reach is no direction"
    );
    assert_eq!(
        strings(&e, 2).map(|s| s.parse::<f64>().is_ok()),
        [true, true]
    );
    move_to(&ctx, &mut e, at_vertex);
    button(&ctx, &mut e, at_vertex, false);
    assert_eq!(e.draft, before);
    assert_eq!(history(&e), (0, 0));
}

#[test]
fn a_new_contour_drag_is_a_guided_one_step_and_escape_focus_and_pointer_loss_restore_it() {
    let (ctx, mut e) = open(&L, true);
    guides(&ctx, &mut e, "Orthogonal");
    // Give Redo something to lose: a drag, Undo, then the real test drags.
    let v2 = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, v2);
    let west = at(&ctx, &mut e, [42., 20.25]);
    move_to(&ctx, &mut e, west);
    button(&ctx, &mut e, west, false);
    assert_eq!(strings(&e, 2), pair("42", "20"));
    e.undo();
    assert_eq!(history(&e), (0, 1));
    let rest = e.draft.clone();

    for cancel in ["Escape", "WindowFocused", "PointerGone"] {
        let v2 = vertex(&ctx, &mut e, 2);
        press(&ctx, &mut e, v2);
        let west = at(&ctx, &mut e, [42., 20.25]);
        let out = move_to(&ctx, &mut e, west);
        assert_eq!(
            strings(&e, 2),
            pair("42", "20"),
            "{cancel}: the guide is live"
        );
        assert!(ring(&out).is_some());
        match cancel {
            "Escape" => frame(&ctx, &mut e, vec![key(egui::Key::Escape)]),
            "WindowFocused" => frame(&ctx, &mut e, vec![egui::Event::WindowFocused(false)]),
            _ => frame(&ctx, &mut e, vec![egui::Event::PointerGone]),
        };
        assert_eq!(e.draft, rest, "{cancel}: the press-time strings are back");
        assert_eq!(history(&e), (0, 1), "{cancel}: Undo and Redo as they were");
        assert!(e.canvas.gesture.is_none(), "{cancel}");
        assert!(
            no_guide_shown(&frame(&ctx, &mut e, vec![])),
            "{cancel}: no guide left"
        );
        // The next release is not a ghost of the cancelled gesture.
        button(&ctx, &mut e, west, false);
        assert!(gesture_is_over(&e), "{cancel}");
        assert_eq!(e.draft, rest, "{cancel}");
        assert_eq!(history(&e), (0, 1), "{cancel}");
        // Focus was lost: give it back for the next round.
        frame(&ctx, &mut e, vec![egui::Event::WindowFocused(true)]);
    }

    // Released outside the canvas the gesture still ends, once, on its guide.
    let v2 = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, v2);
    let canvas = rect(&ctx, &mut e);
    let outside = egui::pos2(canvas.right() + 60., v2.y + 1.);
    move_to(&ctx, &mut e, outside);
    button(&ctx, &mut e, outside, false);
    assert!(gesture_is_over(&e));
    assert_eq!(history(&e), (1, 0), "a new step ends the old Redo");
    let x: f64 = strings(&e, 2)[0].parse().expect("a number");
    assert!(
        x > 118.75 && strings(&e, 2)[1] == "20",
        "east of the canvas, on the guide: {x}"
    );
}

#[test]
fn packed_events_keep_their_order_and_the_settings_belong_to_the_press() {
    let drag = |from: egui::Pos2, to: egui::Pos2| {
        [
            egui::Event::PointerMoved(from),
            egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerMoved(to),
            egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]
    };
    // Two whole gestures in one frame: two checkpoints, each with its own guide.
    let (ctx, mut e) = open(&L, true);
    guides(&ctx, &mut e, "Orthogonal");
    let (v2, v4) = (vertex(&ctx, &mut e, 2), vertex(&ctx, &mut e, 4));
    let (to2, to4) = (
        at(&ctx, &mut e, [42., 20.25]),
        at(&ctx, &mut e, [20.25, 47.]),
    );
    let mut events = Vec::new();
    events.extend(drag(v2, to2));
    events.extend(drag(v4, to4));
    frame(&ctx, &mut e, events);
    assert_eq!(strings(&e, 2), pair("42", "20"));
    assert_eq!(strings(&e, 4), pair("20", "47"));
    assert_eq!(e.undo.len(), 2);
    assert!(gesture_is_over(&e));

    // A drag, then Guides turned Off in the same frame: the drag keeps the setting
    // it was pressed under; the next gesture is free.
    let (ctx, mut e) = open(&L, true);
    guides(&ctx, &mut e, "Orthogonal");
    let out = frame(&ctx, &mut e, vec![]);
    let row = text_at(&out, "Guides:").y;
    let off = texts(&out, "Off")
        .into_iter()
        .find(|p| (p.y - row).abs() < 3.)
        .expect("Off in the Guides row");
    let (v2, to2) = (vertex(&ctx, &mut e, 2), at(&ctx, &mut e, [42., 20.25]));
    let mut events = Vec::new();
    events.extend(drag(v2, to2));
    events.extend(drag(off, off));
    frame(&ctx, &mut e, events);
    assert_eq!(
        e.canvas.guides,
        guide::Mode::Off,
        "the later click was accepted"
    );
    assert_eq!(
        strings(&e, 2),
        pair("42", "20"),
        "and cannot rewrite the drag"
    );
    assert_eq!(e.undo.len(), 1);
    let v1 = vertex(&ctx, &mut e, 1);
    press(&ctx, &mut e, v1);
    let near_spot = at(&ctx, &mut e, [30.5, 0.75]);
    move_to(&ctx, &mut e, near_spot);
    assert_eq!(strings(&e, 1), pair("30.5", "0.75"), "now free");
    button(&ctx, &mut e, near_spot, false);

    // Held down, the box takes no text: the press owns the length it read.
    let (ctx, mut e) = open(&L, true);
    guides(&ctx, &mut e, "Orthogonal");
    type_length(&ctx, &mut e, "15");
    let v2 = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, v2);
    frame(&ctx, &mut e, vec![egui::Event::Text("9".into())]);
    assert_eq!(e.canvas.length, "15", "and the box takes no text");
    let to = at(&ctx, &mut e, [42., 20.25]);
    move_to(&ctx, &mut e, to);
    assert_eq!(
        strings(&e, 2),
        pair("45", "20"),
        "the length of the press: 15 mm"
    );
    button(&ctx, &mut e, to, false);
    assert_eq!(e.undo.len(), 1);
}

#[test]
fn off_busy_and_covered_canvases_offer_no_guide() {
    // Off: the old behaviour, to the three decimals of the pointer.
    let (ctx, mut e) = open(&[["0", "0"], ["20", "0"]], false);
    let pointer = at(&ctx, &mut e, [50.5, 0.75]);
    assert!(no_guide_shown(&hover(&ctx, &mut e, pointer)));
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 2), pair("50.500", "0.750"));

    // Busy: the whole form is disabled; nothing is offered, grabbed or chosen.
    let (ctx, mut e, _) = saved(&L);
    guides(&ctx, &mut e, "Orthogonal");
    let before = e.draft.clone();
    let (v2, west) = (vertex(&ctx, &mut e, 2), at(&ctx, &mut e, [42., 20.25]));
    frame_running(&ctx, &mut e, vec![], true);
    let out = frame_running(&ctx, &mut e, vec![egui::Event::PointerMoved(west)], true);
    assert!(target(&out).is_none() && ray(&out, GUIDE_COLOUR) == 0);
    let mut events = vec![egui::Event::PointerMoved(v2)];
    for (pos, pressed) in [(v2, true), (west, false)] {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    frame_running(&ctx, &mut e, events, true);
    assert_eq!(e.draft, before);
    assert!(e.canvas.gesture.is_none() && e.undo.is_empty());
    let out = frame_running(&ctx, &mut e, vec![], true);
    let row = text_at(&out, "Guides:").y;
    let polar = texts(&out, "Polar 45°")
        .into_iter()
        .find(|p| (p.y - row).abs() < 3.)
        .expect("Polar");
    for pressed in [true, false] {
        frame_running(
            &ctx,
            &mut e,
            vec![
                egui::Event::PointerMoved(polar),
                egui::Event::PointerButton {
                    pos: polar,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            true,
        );
    }
    assert_eq!(
        e.canvas.guides,
        guide::Mode::Orthogonal,
        "a busy form takes no choice"
    );

    // Busy, on an open drawing where a click would add a guided point: the same
    // pointer shows the guide when the form is idle, and nothing while a job runs.
    let (ctx, mut e) = guided(&[["0", "0"], ["20", "0"]], false, "Orthogonal");
    let spot = at(&ctx, &mut e, [50.5, 0.]);
    assert!(
        target(&hover(&ctx, &mut e, spot)).is_some(),
        "idle: the guide is offered"
    );
    let before = e.draft.clone();
    frame_running(&ctx, &mut e, vec![], true);
    frame_running(&ctx, &mut e, vec![egui::Event::PointerMoved(spot)], true);
    let out = frame_running(&ctx, &mut e, vec![], true);
    assert!(target(&out).is_none() && ray(&out, GUIDE_COLOUR) == 0);
    assert!(!painted_containing(&out, "mm from vertex"));
    for pressed in [true, false] {
        frame_running(
            &ctx,
            &mut e,
            vec![egui::Event::PointerButton {
                pos: spot,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }],
            true,
        );
    }
    assert_eq!(e.draft, before, "no point while a job holds the form");
    assert_eq!(history(&e), (0, 0));

    // Covered by another window: no hover, no grab, no new point through it.
    let (ctx, mut e) = guided(&[["0", "0"], ["20", "0"]], false, "Orthogonal");
    let spot = at(&ctx, &mut e, [50.5, 0.]);
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
                egui::Area::new(egui::Id::new("cover-canvas-guides"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(spot - egui::vec2(60., 60.))
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
    covered(vec![egui::Event::PointerMoved(spot)]);
    let out = covered(vec![]);
    assert!(target(&out).is_none() && ray(&out, GUIDE_COLOUR) == 0);
    for pressed in [true, false] {
        covered(vec![egui::Event::PointerButton {
            pos: spot,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }]);
    }
    assert_eq!(e.draft, before, "no point through the window");
}

#[test]
fn one_and_two_pixels_per_point_and_other_canvas_scales_hold_the_same_guide() {
    // The same gesture at 1x and 2x: only logical points are compared.
    let run = |pixels_per_point: f32| {
        let ctx = context();
        ctx.set_pixels_per_point(pixels_per_point);
        let mut e = Editor::default();
        e.begin();
        let draft = e.draft.as_mut().expect("draft");
        draft.points = L.iter().map(|p| p.map(str::to_owned)).collect();
        draft.closed = true;
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        guides(&ctx, &mut e, "Polar 45°");
        let from = vertex(&ctx, &mut e, 2);
        press(&ctx, &mut e, from);
        let to = at(&ctx, &mut e, [42., 20.25]);
        let out = move_to(&ctx, &mut e, to);
        let seen = (strings(&e, 2), ring(&out), target(&out).map(|p| p - from));
        let label = label_at(&out, "180° · vertex 3 moved 18 mm from its start").map(|p| p - from);
        button(&ctx, &mut e, to, false);
        (seen, label, e.draft.clone(), e.undo.len(), from)
    };
    let (one, two) = (run(1.), run(2.));
    assert_eq!((&one.0.0, &one.2, one.3), (&two.0.0, &two.2, two.3));
    assert_eq!(one.0.0, pair("42", "20"));
    for run in [&one, &two] {
        assert!(
            run.0.1.is_some_and(|p| near(p, run.4)),
            "the anchor is the press-time vertex"
        );
    }
    let (t1, t2) = (one.0.2.expect("target"), two.0.2.expect("target"));
    assert!((t1 - t2).length() < 0.01, "target {t1:?} against {t2:?}");
    let (a, b) = (one.1.expect("label"), two.1.expect("label"));
    assert!((a - b).length() < 1., "label {a:?} against {b:?}");

    // The aperture is 8 points across the guide at every canvas scale: 7 hold it, 9
    // do not, whether a millimetre is 4 points or well under 2.
    let (default_view, far) = (
        [["0", "0"], ["40", "0"], ["100", "50"]],
        [["0", "0"], ["40", "0"], ["400", "300"]],
    );
    let mut scales = Vec::new();
    for fit in [false, true] {
        let drawing = if fit { &far } else { &default_view };
        let (ctx, mut e) = open(drawing, false);
        guides(&ctx, &mut e, "Orthogonal");
        if fit {
            choose(&ctx, &mut e, "Fit drawing");
        }
        let scale = e.canvas.scale;
        scales.push(scale);
        let last = drawing[2].map(|v| v.parse::<f64>().expect("number"));
        let anchor = at(&ctx, &mut e, last);
        // Along the west guide from the anchor, 100 points out.
        for (across, held) in [(7.0_f32, true), (9.0, false)] {
            let out = hover(&ctx, &mut e, anchor + egui::vec2(-100., across));
            assert_eq!(
                ray(&out, GUIDE_COLOUR) > 3,
                held,
                "{across} points across at {scale} points per mm"
            );
        }
    }
    assert!(scales[0] > 3. * scales[1], "the scales differ: {scales:?}");
}

#[test]
fn typed_text_stays_exact_and_restore_saved_vertices_keeps_the_editor_settings() {
    // Typed vertex and Next X/Y strings bypass guides and keep their exact text.
    let (ctx, mut e) = guided(&[["0.50", "1"], ["33", "0"]], false, "Polar 45°");
    type_length(&ctx, &mut e, "4");
    let pointer = at(&ctx, &mut e, [60., 0.25]);
    assert!(
        target(&hover(&ctx, &mut e, pointer)).is_some(),
        "a guide is offered"
    );
    replace_field(&ctx, &mut e, "0.50", "16.500");
    assert_eq!(strings(&e, 0), pair("16.500", "1"), "typed text is exact");
    e.next = [" 7.5 ".into(), "1e1".into()];
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Add point"));
    assert_eq!(strings(&e, 2), pair(" 7.5 ", "1e1"));

    // A guided saved drag is restored like any other edit; the guide settings are
    // the editor's, and stay.
    let (ctx, mut e, _) = saved(&L);
    guides(&ctx, &mut e, "Orthogonal");
    type_length(&ctx, &mut e, "15");
    let v2 = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, v2);
    let west = at(&ctx, &mut e, [42., 20.25]);
    move_to(&ctx, &mut e, west);
    button(&ctx, &mut e, west, false);
    assert_eq!(strings(&e, 2), pair("45", "20"));
    choose(&ctx, &mut e, "Restore saved vertices");
    assert_eq!(
        e.draft.as_ref().map(|d| d.points.clone()),
        Some(L.iter().map(|p| p.map(str::to_owned)).collect())
    );
    assert_eq!(
        (e.canvas.guides, e.canvas.length.as_str()),
        (guide::Mode::Orthogonal, "15"),
        "restoring touches the vertices only"
    );
}

#[test]
fn a_guided_result_that_breaks_the_contour_stays_a_visible_draft_and_is_refused_as_before() {
    // New: 20 mm south of (20, 20) lands on the existing vertex (20, 0).
    let (ctx, mut e) = guided(
        &[["0", "0"], ["20", "0"], ["20", "20"]],
        false,
        "Orthogonal",
    );
    type_length(&ctx, &mut e, "20");
    click_mm(&ctx, &mut e, [20.25, -5.]);
    assert_eq!(strings(&e, 3), pair("20", "0"), "placed, and drawn");
    assert_eq!(e.draft.as_ref().map(|d| d.points.len()), Some(4));
    e.draft.as_mut().expect("draft").closed = true;
    let Err(refusal) = e.content() else {
        panic!("a repeated vertex is refused");
    };
    let refusal = refusal.to_string();
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        painted_containing(&out, &refusal),
        "the existing refusal is shown: {refusal}"
    );

    // Saved: vertex 4 (20, 20) 25 mm south crosses the foot of the L.
    let (ctx, mut e, _) = saved(&L);
    guides(&ctx, &mut e, "Orthogonal");
    type_length(&ctx, &mut e, "25");
    let v3 = vertex(&ctx, &mut e, 3);
    press(&ctx, &mut e, v3);
    let south = at(&ctx, &mut e, [20.25, 10.]);
    move_to(&ctx, &mut e, south);
    button(&ctx, &mut e, south, false);
    assert_eq!(strings(&e, 3), pair("20", "-5"));
    assert_eq!(history(&e), (1, 0));
    assert!(
        e.edit_request().is_ok(),
        "the rebuild decides whether the crossing builds"
    );
}

/// The editor's Guides choice and Length text (for the tests of the owners around
/// the editor).
pub(crate) fn guide_state(e: &Editor) -> (&'static str, String) {
    (e.canvas.guides.label(), e.canvas.length.clone())
}

/// §31D-1: through the widgets, Fit, the Guides row and the Length box, then vertex
/// 1 dragged west along its guide. Returns the strings it ended with: the press-time
/// position moved by `length` mm, the other coordinate as the anchor's own text.
pub(crate) fn drag_vertex_one_west_by_a_typed_length(e: &mut Editor, length: &str) -> [String; 2] {
    let ctx = context();
    frame(&ctx, e, vec![]);
    frame(&ctx, e, vec![]);
    choose(&ctx, e, "Fit drawing");
    if e.canvas.guides == guide::Mode::Off {
        guides(&ctx, e, "Orthogonal");
        type_length(&ctx, e, length);
    }
    let from = vertex(&ctx, e, 1);
    press(&ctx, e, from);
    let to = from + egui::vec2(-90., 0.5);
    move_to(&ctx, e, to);
    button(&ctx, e, to, false);
    strings(e, 1)
}

/// Native, once: the editor's L drawn by the peer CLI, vertex 2 dragged with the
/// mouse along a shown guide at a typed length through the Guides row and Length
/// box, the copy saved by the window's worker; then the same numbers typed
/// explicitly to the peer CLI. The two files must agree on every cell and identity,
/// rebuild cold and export the same bytes, which an independent reader measures.
#[test]
fn native_guided_length_drag_worker_and_cli_agree_on_ids_sql_and_geometry() {
    use super::drag_tests::{Published, attach_source_claim, sql_facts, verify_published};
    use crate::creates::tests::{ferritecad, read_semantics};
    use ferritecad_document::Document;
    use ferritecad_kernel::OperationContext;
    use rusqlite::types::Value;
    if !ferritecad_occt::is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("original.fcad");
    let input = root.path().join("L.json");
    std::fs::write(
        &input,
        r#"{"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10}"#,
    )
    .expect("input");
    let p = std::process::Command::new(ferritecad())
        .arg("create-sketch-extrude")
        .arg(&input)
        .arg("-o")
        .arg(&source)
        .arg("--json")
        .output()
        .expect("create");
    assert!(p.status.success(), "{p:?}");
    attach_source_claim(&source);
    let bytes = std::fs::read(&source).expect("source");
    let modified = std::fs::metadata(&source)
        .expect("metadata")
        .modified()
        .expect("mtime");
    let loaded = {
        let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
        ferritecad_scene::snapshot_of(
            &source,
            &mut k,
            |k, b| k.import_step(b),
            &Default::default(),
            &OperationContext::default(),
        )
        .expect("accepted scene")
    };
    let reading =
        crate::sketch_facts::Source::from(loaded.edit_source.expect("accepted edit facts"));
    let id = reading
        .sketches
        .iter()
        .find(|s| s.refusal.is_none())
        .expect("editable base")
        .sketch;
    let mut e = Editor::default();
    let ctx = context();
    assert!(e.begin_edit(&source, &reading, id));
    frame(&ctx, &mut e, vec![]);
    frame(&ctx, &mut e, vec![]);

    // The mouse: Orthogonal, Length 15, vertex 2 (60, 20) pulled west along its guide
    // with pixels a little off the line; the length makes the point exact.
    let saved_text = e.draft.clone();
    guides(&ctx, &mut e, "Orthogonal");
    type_length(&ctx, &mut e, "15");
    let from = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, from);
    let near_spot = at(&ctx, &mut e, [42.375, 20.25]);
    let out = move_to(&ctx, &mut e, near_spot);
    assert!(ring(&out).is_some_and(|p| near(p, from)) && target(&out).is_some());
    button(&ctx, &mut e, near_spot, false);
    assert_eq!(strings(&e, 2), pair("45", "20"));
    assert_eq!(e.undo.len(), 1);
    let moved = e.draft.clone();
    choose(&ctx, &mut e, "Undo draft");
    assert_eq!(e.draft, saved_text);
    choose(&ctx, &mut e, "Redo draft");
    assert_eq!(e.draft, moved);
    choose(&ctx, &mut e, "Save edited copy…");
    let request = e.take_edit_request().expect("submit");
    assert_eq!(request.source, source);
    assert_eq!(request.expected, reading.version);
    let mut edits = crate::edits::Edits::default();
    let mut request = request;
    request.destination = root.path().join("ui.fcad");
    let (tx, rx) = std::sync::mpsc::channel();
    edits
        .start(request.clone(), move |r, g, c| {
            crate::edits::spawn_edit(
                r.source,
                r.expected,
                r.changes,
                r.destination,
                c,
                move |result| tx.send((g, result)).expect("reply"),
            )
        })
        .expect("worker");
    let (g, result) = rx.recv().expect("completed");
    let ui = finish_edit(&mut e, &mut edits, g, result).expect("published");
    assert_eq!(ui, root.path().join("ui.fcad"));

    // The numbers, typed: the same IDs in the same order, literal coordinates.
    let literal = [[0, 0], [60, 0], [45, 20], [20, 20], [20, 40], [0, 40]];
    assert_eq!(
        crate::transaction_tests::vertices(&request)
            .iter()
            .map(|v| v.start_mm)
            .collect::<Vec<_>>(),
        literal.map(|p| p.map(f64::from)).to_vec()
    );

    std::fs::write(
        &input,
        ferritecad_fixtures::edit_request_json(&request.changes).to_string(),
    )
    .expect("request");
    let cli = root.path().join("cli.fcad");
    let p = std::process::Command::new(ferritecad())
        .arg("edit")
        .arg(&source)
        .arg("--expect-version")
        .arg(reading.version.content.to_string())
        .arg("--request")
        .arg(&input)
        .arg("-o")
        .arg(&cli)
        .arg("--json")
        .output()
        .expect("peer CLI");
    assert!(p.status.success(), "{p:?}");
    assert_eq!(
        read_semantics(&ui),
        read_semantics(&cli),
        "same source: all identities and semantic relationships identical"
    );
    let (a, b) = (
        Document::open_read_only(&ui).expect("UI"),
        Document::open_read_only(&cli).expect("CLI"),
    );
    crate::transaction_tests::assert_meta(a.meta(), b.meta());
    assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
    assert_eq!(
        a.dependencies().expect("deps"),
        b.dependencies().expect("deps")
    );
    assert_eq!(crate::test_names::names(&a), crate::test_names::names(&b));
    drop((a, b));

    // The exact SQL allowlist: against the source, only the sketch object's payload
    // and its hash differ, in one row of one table; no table appears or goes.
    let (before, after) = (sql_facts(&source), sql_facts(&ui));
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    let mut changed = Vec::new();
    for (table, old_rows) in &before {
        let new_rows = &after[table];
        assert_eq!(old_rows.len(), new_rows.len(), "{table}");
        for (row, (old, new)) in old_rows.iter().zip(new_rows).enumerate() {
            for (column, (o, n)) in old.iter().zip(new).enumerate() {
                if o != n {
                    changed.push((table.clone(), row, column, old[2].clone()));
                }
            }
        }
    }
    // objects: rowid, id, kind, parent_id, ordinal, name, payload,
    // payload_hash. The kind cell is at column 2 of the same row.
    let kind = Value::Text("sketch".into());
    assert_eq!(changed.len(), 2, "{changed:?}");
    assert!(
        changed
            .iter()
            .all(|c| c.0 == "objects" && c.1 == changed[0].1 && c.3 == kind)
    );
    assert_eq!(changed.iter().map(|c| c.2).collect::<Vec<_>>(), [6, 7]);

    verify_published(
        &source,
        &ui,
        &cli,
        &Published {
            vertices: [
                [0., 0.],
                [60., 0.],
                [45., 20.],
                [20., 20.],
                [20., 40.],
                [0., 40.],
            ],
            side_x: 60.,
            high: [60., 40., 10.],
            volume_mm3: 14500.,
            artifacts: ("FCAD_SKETCH_GUIDE_ARTIFACT_DIR", "guide"),
        },
    );
    assert_eq!(std::fs::read(&source).expect("source"), bytes);
    assert_eq!(
        std::fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        modified
    );
}
