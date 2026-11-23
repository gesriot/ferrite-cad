// SPDX-License-Identifier: MIT
//! §31D-2: the explicit "From object" gesture through the production canvas and
//! editor. Real egui pointer, key and text events go in; draft strings, history and
//! what was painted come out. No window and no graphics device, and nothing here
//! calls a private helper in place of the interface: every anchor below is reached by
//! hovering it, and every gesture starts with a press on it.
#![allow(clippy::panic)]
use super::{
    drag_tests::{button, move_to, press, vertex},
    guide_tests::{circle, guides, history, near, ring, type_length},
    object_snap_tests::{
        L, at, choose, context, escape, hover, label_at, painted, pair, rect, saved, strings,
        switch,
    },
    tests::{click, frame, text_at},
    *,
};

/// The provisional anchor's ring, where it is drawn.
fn provisional(out: &egui::FullOutput) -> Option<egui::Pos2> {
    circle(out, ANCHOR_COLOUR, 11.)
}
/// "From object" pressed through the real checkbox in the Guides row.
fn from_object(ctx: &egui::Context, e: &mut Editor) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, "From object"));
}
/// A saved Sketch with Guides chosen and the mode on.
fn saved_anchored(points: &[[&str; 2]], mode: &str) -> (egui::Context, Editor) {
    let (ctx, mut e, _) = saved(points);
    guides(&ctx, &mut e, mode);
    from_object(&ctx, &mut e);
    assert!(e.canvas.anchors);
    (ctx, e)
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
fn off_busy_covered_and_unreadable_canvases_offer_no_anchor_and_typed_text_stays_exact() {
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
}

#[test]
fn a_result_that_breaks_the_contour_stays_a_draft_and_restore_keeps_the_mode() {
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
