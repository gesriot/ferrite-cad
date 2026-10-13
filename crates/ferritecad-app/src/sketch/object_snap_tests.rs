// SPDX-License-Identifier: MIT
//! §31C: object snaps through the production canvas and editor. Real egui pointer
//! and key events go in; draft strings, history and what was painted come out. No
//! window and no graphics device, and nothing here calls a private helper in place
//! of the interface.
#![allow(clippy::panic)]
use super::{
    drag_tests::{button, canvas, key, move_to, press, vertex},
    tests::{click, frame, frame_running, replace_field, text_at},
    *,
};

/// The editor's own L, closed: 60 × 20 foot with a 20-wide column to y = 40.
const L: [[&str; 2]; 6] = [
    ["0", "0"],
    ["60", "0"],
    ["60", "20"],
    ["20", "20"],
    ["20", "40"],
    ["0", "40"],
];
/// An open zigzag with exactly one crossing, at (10, 10), between its first and
/// last edges; midpoints (15, 15), (30, 15) and (15, 7.5).
const ZIGZAG: [[&str; 2]; 4] = [["0", "0"], ["30", "30"], ["30", "0"], ["0", "15"]];

/// A context whose windows appear at once, so painted colours are the real ones
/// (a fading window paints everything translucent).
fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    ctx
}

/// A drawing for a new document, as typed, on a canvas that has laid itself out.
fn open(points: &[[&str; 2]], closed: bool) -> (egui::Context, Editor) {
    let ctx = context();
    let mut e = Editor::default();
    e.begin();
    let draft = e.draft.as_mut().expect("draft");
    draft.points = points.iter().map(|p| p.map(str::to_owned)).collect();
    draft.closed = closed;
    frame(&ctx, &mut e, vec![]);
    frame(&ctx, &mut e, vec![]);
    (ctx, e)
}

/// The same drawing as a saved Sketch being edited: the form that has Apply and
/// Save, with no document behind it.
fn saved(points: &[[&str; 2]]) -> (egui::Context, Editor, EditSketchRequest) {
    let (ctx, mut e) = open(points, true);
    let vertices: Vec<_> = Canvas::points(e.draft.as_ref().expect("draft"))
        .expect("points")
        .into_iter()
        .map(|start_mm| SketchVertex {
            curve_id: ferritecad_types::StableEntityId::new(),
            start_mm,
        })
        .collect();
    let id = ferritecad_types::ObjectId::new();
    let request = EditSketchRequest {
        source: PathBuf::from("accepted.fcad"),
        destination: PathBuf::new(),
        sketch: id,
        vertices: vertices.clone(),
        expected: ferritecad_document::DocumentVersion {
            document_id: ferritecad_types::DocumentId::new(),
            content: ferritecad_types::ContentHash::of_bytes(b"accepted snapshot"),
        },
    };
    let choice = SketchChoice {
        cut_history: None,
        fillets: Vec::new(),
        chamfer: None,
        sketch: id,
        name: None,
        vertices: Some(vertices),
        profile_use: Some(ferritecad_document::SketchProfileUse::BlindExtrude {
            feature: ferritecad_types::ObjectId::new(),
            height_mm: 10.,
        }),
        refusal: None,
    };
    e.editing = Some((request.clone(), choice));
    frame(&ctx, &mut e, vec![]);
    frame(&ctx, &mut e, vec![]); // the changed form's identity finishes its first layout
    (ctx, e, request)
}

fn rect(ctx: &egui::Context, e: &mut Editor) -> egui::Rect {
    canvas(&frame(ctx, e, vec![]))
}
/// Where a document point (mm) is on the screen now.
fn at(ctx: &egui::Context, e: &mut Editor, mm: [f64; 2]) -> egui::Pos2 {
    let rect = rect(ctx, e);
    e.canvas.screen(rect, mm)
}
fn switch(ctx: &egui::Context, e: &mut Editor) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, "Object snap"));
}
fn choose(ctx: &egui::Context, e: &mut Editor, label: &str) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, label));
}
/// The pointer rests at `pos`; the frame that shows what it would do.
fn hover(ctx: &egui::Context, e: &mut Editor, pos: egui::Pos2) -> egui::FullOutput {
    frame(ctx, e, vec![egui::Event::PointerMoved(pos)]);
    frame(ctx, e, vec![])
}
fn label_at(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    out.shapes.iter().find_map(|c| match &c.shape {
        egui::Shape::Text(t) if t.galley.text() == text => Some(t.visual_bounding_rect().center()),
        _ => None,
    })
}
fn painted(out: &egui::FullOutput, text: &str) -> bool {
    label_at(out, text).is_some()
}
fn painted_containing(out: &egui::FullOutput, text: &str) -> bool {
    out.shapes
        .iter()
        .any(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text().contains(text)))
}
/// Where the marker is: the middle of whatever was drawn in the marker's colour.
fn marker(out: &egui::FullOutput) -> Option<egui::Pos2> {
    let centres: Vec<_> = out
        .shapes
        .iter()
        .filter(|c| match &c.shape {
            egui::Shape::Rect(r) => r.stroke.color == SNAP_MARKER,
            egui::Shape::LineSegment { stroke, .. } => stroke.color == SNAP_MARKER,
            egui::Shape::Path(p) => p.stroke.color == egui::epaint::ColorMode::Solid(SNAP_MARKER),
            _ => false,
        })
        .map(|c| c.shape.visual_bounding_rect().center())
        .collect();
    (!centres.is_empty()).then(|| {
        let n = centres.len() as f32;
        egui::pos2(
            centres.iter().map(|c| c.x).sum::<f32>() / n,
            centres.iter().map(|c| c.y).sum::<f32>() / n,
        )
    })
}
fn strings(e: &Editor, i: usize) -> [String; 2] {
    e.draft.as_ref().expect("draft").points[i].clone()
}
fn pair(x: &str, y: &str) -> [String; 2] {
    [x.to_owned(), y.to_owned()]
}
fn escape(ctx: &egui::Context, e: &mut Editor) {
    frame(ctx, e, vec![key(egui::Key::Escape)]);
}

#[test]
fn the_switch_sits_beside_the_grid_step_and_changes_only_editor_state() {
    let (ctx, mut e) = open(&ZIGZAG, false);
    let out = frame(&ctx, &mut e, vec![]);
    let (grid, last, objects) = (
        text_at(&out, "1 mm"),
        text_at(&out, "10 mm"),
        text_at(&out, "Object snap"),
    );
    assert!(
        (grid.y - objects.y).abs() < 3. && objects.x > last.x,
        "in the grid step's row"
    );
    assert!(!e.canvas.object_snap, "off for a new editor, like the grid");
    assert!(painted_containing(&out, "object snap: off"));
    let before = e.draft.clone();
    switch(&ctx, &mut e);
    assert!(e.canvas.object_snap);
    assert_eq!(e.draft, before, "toggling moves nothing");
    assert!(
        e.undo.is_empty() && e.redo.is_empty(),
        "and is no Undo step"
    );
    assert!(painted_containing(
        &frame(&ctx, &mut e, vec![]),
        "object snap: on"
    ));
    choose(&ctx, &mut e, "5 mm");
    assert_eq!((e.canvas.snap, e.canvas.object_snap), (Snap::Five, true));
    switch(&ctx, &mut e);
    assert_eq!((e.canvas.snap, e.canvas.object_snap), (Snap::Five, false));
    assert_eq!(e.draft, before);
    assert!(e.undo.is_empty() && e.redo.is_empty());
    println!("\nFCAD_31C_SWITCH_EXECUTED");
}

#[test]
fn new_points_take_crossing_midpoint_and_endpoint_with_a_marker_and_a_name() {
    // (target in mm, pointer offset in points, kind, the text it places)
    for (target, offset, name, placed) in [
        (
            [10., 10.],
            egui::vec2(3., -2.),
            "Intersection",
            ["10", "10"],
        ),
        ([30., 15.], egui::vec2(-2., 3.), "Midpoint", ["30", "15"]),
        // Ten and a half points from a vertex: outside its 9-point grab, inside
        // the aperture, so a click places a point on that end.
        ([30., 30.], egui::vec2(10.5, 0.), "Endpoint", ["30", "30"]),
    ] {
        let (ctx, mut e) = open(&ZIGZAG, false);
        switch(&ctx, &mut e);
        let snapped = at(&ctx, &mut e, target);
        let pointer = snapped + offset;
        let before = e.draft.clone();
        let out = hover(&ctx, &mut e, pointer);
        let label = label_at(&out, name).unwrap_or_else(|| panic!("{name} is named"));
        assert!(
            label.x > pointer.x && label.y > pointer.y && label.distance(pointer) < 90.,
            "{name} beside the pointer"
        );
        let found = marker(&out).unwrap_or_else(|| panic!("{name} has a marker"));
        assert!(
            found.distance(snapped) < 0.01,
            "{name}: marker on the point"
        );
        assert_eq!(e.draft, before, "{name}: hovering edits nothing");
        assert!(
            e.undo.is_empty() && e.redo.is_empty(),
            "{name}: or the history"
        );
        click(&ctx, &mut e, pointer);
        let draft = e.draft.clone().expect("draft");
        assert_eq!(draft.points.len(), ZIGZAG.len() + 1, "{name}");
        assert_eq!(
            draft.points[ZIGZAG.len()],
            placed,
            "{name}: exact, not the pointer"
        );
        assert_eq!(
            e.undo.len(),
            1,
            "{name}: the coordinate edit is the one step"
        );
        e.undo();
        assert_eq!(e.draft, before);
        e.redo();
        assert_eq!(e.draft.as_ref(), Some(&draft));
    }
    println!("\nFCAD_31C_NEW_POINTS_EXECUTED");
}

#[test]
fn an_object_point_beats_the_grid_is_never_rounded_and_typed_text_stays_exact() {
    // Grid 5 mm would give 15; the middle of the 33 mm edge is 16.5.
    let (ctx, mut e) = open(&[["0", "0"], ["33", "0"]], false);
    switch(&ctx, &mut e);
    choose(&ctx, &mut e, "5 mm");
    let middle = at(&ctx, &mut e, [16.5, 0.]) + egui::vec2(1., -1.);
    click(&ctx, &mut e, middle);
    assert_eq!(
        strings(&e, 2),
        pair("16.5", "0"),
        "the object, not the grid"
    );
    // Away from every object the grid still decides.
    let away = at(&ctx, &mut e, [60., 30.]) + egui::vec2(1., -1.);
    let out = hover(&ctx, &mut e, away);
    assert!(marker(&out).is_none() && !painted(&out, "Midpoint"));
    click(&ctx, &mut e, away);
    assert_eq!(strings(&e, 3), pair("60", "30"));

    // The same in a drag. Grid 5 mm would drop vertex 2 on (15, 20); the middle of
    // its edge to vertex 3 is (16.5, 21). Away from every object the grid rounds.
    let (ctx, mut e) = open(&[["0", "0"], ["33", "0"], ["33", "21"], ["0", "21"]], true);
    switch(&ctx, &mut e);
    choose(&ctx, &mut e, "5 mm");
    let from = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, from);
    let near = at(&ctx, &mut e, [16.75, 21.25]);
    move_to(&ctx, &mut e, near);
    assert_eq!(strings(&e, 2), pair("16.5", "21"));
    let free = at(&ctx, &mut e, [60.25, 30.25]);
    move_to(&ctx, &mut e, free);
    assert_eq!(strings(&e, 2), pair("60", "30"));
    button(&ctx, &mut e, free, false);
    assert_eq!(e.undo.len(), 1);

    // Not three decimals either: the midpoint of 0.1 and 50.7 is not 25.400.
    let (ctx, mut e) = open(&[["0.1", "0"], ["50.7", "0"]], false);
    switch(&ctx, &mut e);
    let middle = at(&ctx, &mut e, [25.4, 0.]) + egui::vec2(1., 1.);
    click(&ctx, &mut e, middle);
    let placed = strings(&e, 2);
    assert_eq!(placed[0], ((0.1_f64 + 50.7) / 2.0).to_string());
    assert_eq!(placed[0], "25.400000000000002");
    assert_ne!(placed[0], format!("{:.3}", 25.4));
    assert_eq!(placed[1], "0");

    // Typed X/Y never snap, however near a snap point the pointer last rested.
    let (ctx, mut e) = open(&[["0.50", "1"], ["33", "0"]], false);
    switch(&ctx, &mut e);
    let middle = at(&ctx, &mut e, [16.75, 0.5]) + egui::vec2(1., 0.);
    assert!(
        marker(&hover(&ctx, &mut e, middle)).is_some(),
        "a snap is offered"
    );
    replace_field(&ctx, &mut e, "0.50", "16.500");
    assert_eq!(strings(&e, 0), pair("16.500", "1"), "typed text is exact");
    e.next = [" 7.5 ".into(), "1e1".into()];
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Add point"));
    assert_eq!(strings(&e, 2), pair(" 7.5 ", "1e1"));
    println!("\nFCAD_31C_GRID_AND_TEXT_EXECUTED");
}

#[test]
fn a_drag_lands_on_the_frozen_incident_midpoint_never_on_itself_and_is_one_undo() {
    let (ctx, mut e) = open(&L, true);
    switch(&ctx, &mut e);
    let before = e.draft.clone();
    // Vertex 2 (60, 20) to the middle of its own edge to vertex 3: (40, 20).
    let from = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, from);
    let near = at(&ctx, &mut e, [40.375, 20.25]);
    let out = move_to(&ctx, &mut e, near);
    assert_eq!(strings(&e, 2), pair("40", "20"));
    assert!(painted(&out, "Midpoint"));
    let found = marker(&out).expect("marker");
    assert!(found.distance(at(&ctx, &mut e, [40., 20.])) < 0.01);
    assert!(e.undo.is_empty(), "a preview is no history");
    button(&ctx, &mut e, near, false);
    assert!(e.canvas.gesture.is_none() && !e.canvas.claimed_press);
    assert_eq!(strings(&e, 2), pair("40", "20"));
    assert_eq!(e.undo.len(), 1, "one gesture, one Undo");
    assert!(e.content().is_ok(), "this snap makes a valid chamfered L");
    let moved = e.draft.clone();
    choose(&ctx, &mut e, "Undo draft");
    assert_eq!(e.draft, before);
    choose(&ctx, &mut e, "Redo draft");
    assert_eq!(e.draft, moved);

    // The press-time midpoint of an incident edge stays where the press found it.
    // Vertex 1's edge to vertex 0 has its middle at (30, 0); with vertex 1 at
    // (30.5, 0.75) the edge's *moving* middle is (15.25, 0.4) and is not offered.
    let from = vertex(&ctx, &mut e, 1);
    press(&ctx, &mut e, from);
    let near = at(&ctx, &mut e, [30.5, 0.75]);
    move_to(&ctx, &mut e, near);
    assert_eq!(strings(&e, 1), pair("30", "0"), "the frozen middle");
    let moving = at(&ctx, &mut e, [16., 1.]);
    let out = move_to(&ctx, &mut e, moving);
    assert_eq!(
        strings(&e, 1),
        pair("16", "1"),
        "no middle follows the draft"
    );
    assert!(!painted(&out, "Midpoint") && marker(&out).is_none());
    escape(&ctx, &mut e);
    assert_eq!(e.draft, moved);

    // The vertex is not offered to itself: a 2-point nudge moves it.
    let from = vertex(&ctx, &mut e, 0);
    press(&ctx, &mut e, from);
    move_to(&ctx, &mut e, from + egui::vec2(2., 0.));
    assert_eq!(strings(&e, 0), pair("0.5", "0"));
    escape(&ctx, &mut e);
    // Nor is a foreign vertex stacked on it at the press.
    e.draft
        .as_mut()
        .expect("draft")
        .points
        .push(pair("60", "0"));
    let stacked = e.draft.clone();
    let from = vertex(&ctx, &mut e, 1);
    press(&ctx, &mut e, from);
    move_to(&ctx, &mut e, from + egui::vec2(2., 0.));
    assert_eq!(e.canvas.selected, Some(1));
    assert_eq!(strings(&e, 1), pair("60.5", "0"));
    escape(&ctx, &mut e);
    assert_eq!(e.draft, stacked);

    // The other kinds in a drag, on an open drawing: vertex 3 (0, 15) onto the
    // crossing (10, 10) of its own edge with the first; vertex 0 onto vertex 2.
    let (ctx, mut e) = open(&ZIGZAG, false);
    switch(&ctx, &mut e);
    for (vertex_index, target, name, placed) in [
        (3, [10.25, 10.5], "Intersection", pair("10", "10")),
        (0, [30.5, 0.5], "Endpoint", pair("30", "0")),
    ] {
        let from = vertex(&ctx, &mut e, vertex_index);
        press(&ctx, &mut e, from);
        let near = at(&ctx, &mut e, target);
        let out = move_to(&ctx, &mut e, near);
        assert_eq!(strings(&e, vertex_index), placed, "{name}");
        assert!(painted(&out, name), "{name} is named");
        let spot = at(
            &ctx,
            &mut e,
            [
                placed[0].parse().expect("number"),
                placed[1].parse().expect("number"),
            ],
        );
        assert!(marker(&out).expect(name).distance(spot) < 0.01);
        escape(&ctx, &mut e);
    }
    println!("\nFCAD_31C_DRAG_EXECUTED");
}

#[test]
fn a_snap_that_breaks_the_contour_stays_a_draft_and_cancelling_leaves_history_alone() {
    let (ctx, mut e) = open(&L, true);
    switch(&ctx, &mut e);
    let before = e.draft.clone();
    // Vertex 1 onto the middle of the far column's edge: edges now cross.
    let from = vertex(&ctx, &mut e, 1);
    press(&ctx, &mut e, from);
    let near = at(&ctx, &mut e, [20.25, 30.5]);
    move_to(&ctx, &mut e, near);
    assert_eq!(strings(&e, 1), pair("20", "30"));
    let error = e.content().expect_err("a crossing contour").to_string();
    assert!(error.contains("intersect"), "{error}");
    assert!(
        painted(&frame(&ctx, &mut e, vec![]), &error),
        "the reason is shown"
    );
    button(&ctx, &mut e, near, false);
    assert_eq!(strings(&e, 1), pair("20", "30"), "the draft stays, visible");
    assert_eq!(e.undo.len(), 1);
    assert!(e.content().is_err(), "and is refused by the usual check");
    choose(&ctx, &mut e, "Undo draft");
    assert_eq!(e.draft, before);
    assert_eq!((e.undo.len(), e.redo.len()), (0, 1));
    let history = (e.undo.len(), e.redo.len());

    // Escape, lost focus and a vanished pointer each restore the press-time
    // strings and leave Undo and Redo exactly as they were.
    for cancel in [
        key(egui::Key::Escape),
        egui::Event::WindowFocused(false),
        egui::Event::PointerGone,
    ] {
        let from = vertex(&ctx, &mut e, 1);
        press(&ctx, &mut e, from);
        let target = at(&ctx, &mut e, [20.25, 30.5]);
        move_to(&ctx, &mut e, target);
        assert_eq!(strings(&e, 1), pair("20", "30"));
        let out = frame(&ctx, &mut e, vec![cancel]);
        assert_eq!(e.draft, before);
        assert!(e.canvas.gesture.is_none());
        assert!(marker(&out).is_none() && !painted(&out, "Midpoint"));
        assert_eq!((e.undo.len(), e.redo.len()), history);
        button(&ctx, &mut e, from, false);
        assert_eq!(e.draft, before, "a late release changes nothing");
    }

    // A release outside the canvas completes the gesture at that place, as for
    // the grid: one step, no candidate there.
    let from = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, from);
    let target = at(&ctx, &mut e, [40.25, 20.5]);
    move_to(&ctx, &mut e, target);
    let outside = rect(&ctx, &mut e).right_center() + egui::vec2(60., 0.);
    button(&ctx, &mut e, outside, false);
    assert!(e.canvas.gesture.is_none());
    assert_eq!(e.undo.len(), 1);
    assert_eq!(e.redo.len(), 0, "a new step ends the old Redo");
    let x: f64 = strings(&e, 2)[0].parse().expect("a number");
    assert!(x > 118.75, "released outside, at {x}");
    println!("\nFCAD_31C_CANCEL_AND_INVALID_EXECUTED");
}

#[test]
fn packed_events_keep_order_and_a_switch_in_the_frame_changes_only_the_next_gesture() {
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
    // Two whole gestures and a press/release in one frame: two checkpoints.
    let (ctx, mut e) = open(&L, true);
    switch(&ctx, &mut e);
    let (v2, v4) = (vertex(&ctx, &mut e, 2), vertex(&ctx, &mut e, 4));
    let (to2, to4) = (
        at(&ctx, &mut e, [40.375, 20.25]),
        at(&ctx, &mut e, [10.25, 40.5]),
    );
    let mut events = Vec::new();
    events.extend(drag(v2, to2));
    events.extend(drag(v4, to4));
    frame(&ctx, &mut e, events);
    assert_eq!(strings(&e, 2), pair("40", "20"));
    assert_eq!(strings(&e, 4), pair("10", "40"));
    assert_eq!(e.undo.len(), 2);
    assert!(e.canvas.gesture.is_none() && !e.canvas.claimed_press);

    // A drag, then the switch turned off, in one frame: the drag keeps the
    // setting it was pressed under; the next gesture does not snap.
    let (ctx, mut e) = open(&L, true);
    switch(&ctx, &mut e);
    let switch_at = text_at(&frame(&ctx, &mut e, vec![]), "Object snap");
    let (v2, to2) = (vertex(&ctx, &mut e, 2), at(&ctx, &mut e, [40.375, 20.25]));
    let mut events = Vec::new();
    events.extend(drag(v2, to2));
    events.extend(drag(switch_at, switch_at));
    frame(&ctx, &mut e, events);
    assert!(!e.canvas.object_snap, "the later click was accepted");
    assert_eq!(
        strings(&e, 2),
        pair("40", "20"),
        "and cannot rewrite the drag"
    );
    assert_eq!(e.undo.len(), 1);
    let v1 = vertex(&ctx, &mut e, 1);
    press(&ctx, &mut e, v1);
    let near = at(&ctx, &mut e, [30.5, 0.75]);
    move_to(&ctx, &mut e, near);
    assert_eq!(strings(&e, 1), pair("30.5", "0.75"), "now free");
    button(&ctx, &mut e, near, false);

    // Held down, the switch is not available: the press owns the setting.
    let (ctx, mut e) = open(&L, true);
    switch(&ctx, &mut e);
    let v2 = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, v2);
    let switch_at = text_at(&frame(&ctx, &mut e, vec![]), "Object snap");
    click(&ctx, &mut e, switch_at);
    assert!(e.canvas.object_snap, "a held gesture keeps its settings");
    println!("\nFCAD_31C_PACKED_EXECUTED");
}

#[test]
fn off_busy_and_covered_canvases_offer_no_snap() {
    // Off: the old behaviour, to the three decimals of the pointer.
    let (ctx, mut e) = open(&ZIGZAG, false);
    let pointer = at(&ctx, &mut e, [10., 10.]) + egui::vec2(3., -2.);
    let out = hover(&ctx, &mut e, pointer);
    assert!(marker(&out).is_none() && !painted(&out, "Intersection"));
    click(&ctx, &mut e, pointer);
    assert_eq!(strings(&e, 4), pair("10.750", "10.500"));

    // Busy: the whole form is disabled; nothing is offered, grabbed or switched.
    let (ctx, mut e, _) = saved(&L);
    switch(&ctx, &mut e);
    let before = e.draft.clone();
    let (v2, near) = (vertex(&ctx, &mut e, 2), at(&ctx, &mut e, [40.375, 20.25]));
    frame_running(&ctx, &mut e, vec![], true);
    let out = frame_running(&ctx, &mut e, vec![egui::Event::PointerMoved(near)], true);
    assert!(marker(&out).is_none() && !painted(&out, "Midpoint"));
    let mut events = vec![egui::Event::PointerMoved(v2)];
    for (pos, pressed) in [(v2, true), (near, false)] {
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
    let switch_at = text_at(&frame_running(&ctx, &mut e, vec![], true), "Object snap");
    for pressed in [true, false] {
        frame_running(
            &ctx,
            &mut e,
            vec![
                egui::Event::PointerMoved(switch_at),
                egui::Event::PointerButton {
                    pos: switch_at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            true,
        );
    }
    assert!(e.canvas.object_snap, "a busy form takes no switch");

    // Covered by another window: no hover, no grab, no new point through it.
    let (ctx, mut e) = open(&ZIGZAG, false);
    switch(&ctx, &mut e);
    let spot = at(&ctx, &mut e, [10., 10.]);
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
                egui::Area::new(egui::Id::new("cover-canvas"))
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
    assert!(marker(&out).is_none() && !painted(&out, "Intersection"));
    for pressed in [true, false] {
        covered(vec![egui::Event::PointerButton {
            pos: spot,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }]);
    }
    assert_eq!(e.draft, before, "no point through the window");
    println!("\nFCAD_31C_BLOCKED_EXECUTED");
}

#[test]
fn one_and_two_pixels_per_point_and_other_canvas_scales_pick_the_same_point() {
    // The same gesture and the same hover, at 1x and 2x: only logical points are
    // compared, so what is picked and shown is identical.
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
        switch(&ctx, &mut e);
        let from = vertex(&ctx, &mut e, 2);
        press(&ctx, &mut e, from);
        let target = at(&ctx, &mut e, [40.375, 20.25]);
        let out = move_to(&ctx, &mut e, target);
        let seen = (strings(&e, 2), marker(&out).map(|p| p - from));
        let label = label_at(&out, "Midpoint").map(|p| p - from);
        button(&ctx, &mut e, from, false);
        (seen, label, e.draft.clone(), e.undo.len())
    };
    let (one, two) = (run(1.), run(2.));
    assert_eq!((&one.0, &one.2, one.3), (&two.0, &two.2, two.3));
    assert_eq!(one.0.0, pair("40", "20"));
    assert!(one.0.1.is_some());
    // Text metrics differ by a fraction of a point between the two scales.
    let (a, b) = (one.1.expect("label"), two.1.expect("label"));
    assert!((a - b).length() < 1., "label {a:?} against {b:?}");

    // The aperture is 12 points at every canvas scale: 11 hit, 13 do not, whether
    // a millimetre is 4 points or about one.
    // One crossing, at (30, 15), inside the default view; the last point is far
    // beyond it, so Fit shrinks the scale to a fifth.
    let far = [
        ["0", "0"],
        ["90", "45"],
        ["90", "0"],
        ["0", "22.5"],
        ["400", "300"],
    ];
    let (ctx, mut e) = open(&far, false);
    switch(&ctx, &mut e);
    let mut scales = vec![e.canvas.scale];
    for fitted in [false, true] {
        if fitted {
            choose(&ctx, &mut e, "Fit drawing");
            scales.push(e.canvas.scale);
        }
        let crossing = at(&ctx, &mut e, [30., 15.]);
        for (points, hit) in [(11.0_f32, true), (13.0, false)] {
            let out = hover(&ctx, &mut e, crossing + egui::vec2(0., points));
            assert_eq!(
                painted(&out, "Intersection"),
                hit,
                "{points} points at scale {}",
                e.canvas.scale
            );
        }
    }
    assert!(scales[0] > 3. * scales[1], "scales differ: {scales:?}");
    println!("\nFCAD_31C_SCALE_PIXELS_EXECUTED");
}

#[test]
fn only_what_the_canvas_shows_is_offered() {
    // The middle of this edge is at x = 120, just past the canvas's right edge
    // (118.75): in the aperture of the pointer, but not on the screen.
    let (ctx, mut e) = open(&[["100", "10"], ["140", "10"]], false);
    switch(&ctx, &mut e);
    let edge = rect(&ctx, &mut e);
    let pointer = egui::pos2(edge.right() - 2., at(&ctx, &mut e, [120., 10.]).y);
    let out = hover(&ctx, &mut e, pointer);
    assert!(marker(&out).is_none() && !painted(&out, "Midpoint"));
    let (ctx, mut e) = open(&[["100", "10"], ["136", "10"]], false);
    switch(&ctx, &mut e);
    let edge = rect(&ctx, &mut e);
    let pointer = egui::pos2(edge.right() - 2., at(&ctx, &mut e, [118., 10.]).y);
    assert!(painted(&hover(&ctx, &mut e, pointer), "Midpoint"));
    println!("\nFCAD_31C_VISIBLE_EXECUTED");
}

#[test]
fn saved_vertices_drag_to_a_midpoint_and_ask_for_the_request_typed_numbers_make() {
    let (ctx, mut e, original) = saved(&L);
    switch(&ctx, &mut e);
    let from = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, from);
    let near = at(&ctx, &mut e, [40.375, 20.25]);
    move_to(&ctx, &mut e, near);
    button(&ctx, &mut e, near, false);
    assert_eq!(e.undo.len(), 1);
    let snapped = e.edit_request().expect("a valid saved edit");
    // The very vertices the user would have typed: same IDs, same order, the
    // dragged one at exactly (40, 20).
    let ids = |r: &EditSketchRequest| r.vertices.iter().map(|v| v.curve_id).collect::<Vec<_>>();
    assert_eq!(ids(&snapped), ids(&original));
    assert_eq!(
        snapped
            .vertices
            .iter()
            .map(|v| v.start_mm)
            .collect::<Vec<_>>(),
        [
            [0., 0.],
            [60., 0.],
            [40., 20.],
            [20., 20.],
            [20., 40.],
            [0., 40.]
        ]
    );
    let moved = e.draft.clone();
    let typed = {
        let (ctx, mut by_hand, _) = saved(&L);
        replace_field(&ctx, &mut by_hand, "60", "60"); // keeps 60 as typed
        by_hand.draft.as_mut().expect("draft").points[2] = pair("40", "20");
        by_hand.edit_request().expect("typed").vertices
    };
    assert_eq!(
        typed.iter().map(|v| v.start_mm).collect::<Vec<_>>(),
        snapped
            .vertices
            .iter()
            .map(|v| v.start_mm)
            .collect::<Vec<_>>()
    );
    // The same Restore saved vertices as for any other edit.
    choose(&ctx, &mut e, "Restore saved vertices");
    assert_eq!(
        e.draft.as_ref().map(|d| d.points.clone()),
        Some(L.iter().map(|p| p.map(str::to_owned)).collect())
    );
    assert!(e.canvas.object_snap, "restoring leaves the editor setting");
    assert_ne!(e.draft, moved);
    println!("\nFCAD_31C_SAVED_EXECUTED");
}

/// Whether the editor's Object snap is on (for tests of the owners around it).
pub(crate) fn object_snap_on(e: &Editor) -> bool {
    e.canvas.object_snap
}

/// §31C: through the widgets, Fit and the switch, then vertex 1 dragged onto the
/// middle of its edge to vertex 2. Returns the strings it ended with and the
/// middle's own number, which must be the same.
pub(crate) fn drag_vertex_one_to_its_edge_middle(e: &mut Editor) -> ([String; 2], [String; 2]) {
    let ctx = context();
    frame(&ctx, e, vec![]);
    frame(&ctx, e, vec![]);
    choose(&ctx, e, "Fit drawing");
    if !e.canvas.object_snap {
        switch(&ctx, e);
    }
    let p = Canvas::points(e.draft.as_ref().expect("a draft")).expect("numbers");
    let middle = [(p[1][0] + p[2][0]) / 2., (p[1][1] + p[2][1]) / 2.];
    let from = vertex(&ctx, e, 1);
    press(&ctx, e, from);
    let to = at(&ctx, e, [middle[0] + 0.1, middle[1] + 0.1]);
    move_to(&ctx, e, to);
    button(&ctx, e, to, false);
    (
        strings(e, 1),
        [
            object_snap::coordinate(middle[0]),
            object_snap::coordinate(middle[1]),
        ],
    )
}

/// Native, once: the editor's L drawn by the peer CLI, vertex 2 dragged with the
/// mouse onto the press-time middle of its own edge through Object snap, the copy
/// saved by the window's worker; then the same numbers typed explicitly to the
/// peer CLI. The two files must agree on every cell and identity, rebuild cold
/// and export the same bytes, which an independent reader measures.
#[test]
fn native_object_snap_drag_worker_and_cli_agree_on_ids_sql_and_geometry() {
    use super::drag_tests::{Published, attach_source_claim, sql_facts, verify_published};
    use crate::creates::tests::{ferritecad, read_semantics};
    use ferritecad_document::Document;
    use ferritecad_kernel::OperationContext;
    use rusqlite::types::Value;
    if !ferritecad_occt::is_available() {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: no OCCT for the object-snap native gate");
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
    let reading = loaded.edit_source.expect("accepted edit facts");
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

    // The mouse: switch on, vertex 2 (60, 20) onto (40, 20), pixels a little off.
    let saved_text = e.draft.clone();
    switch(&ctx, &mut e);
    let from = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, from);
    let near = at(&ctx, &mut e, [40.375, 20.25]);
    move_to(&ctx, &mut e, near);
    button(&ctx, &mut e, near, false);
    assert_eq!(strings(&e, 2), pair("40", "20"));
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
        .start_sketch(request.clone(), move |r, g, c| {
            crate::edits::spawn_sketch_edit(r, c, move |result| {
                tx.send((g, result)).expect("reply")
            })
        })
        .expect("worker");
    let (g, result) = rx.recv().expect("completed");
    let ui = finish_edit(&mut e, &mut edits, g, result).expect("published");
    assert_eq!(ui, root.path().join("ui.fcad"));

    // The numbers, typed: the same IDs in the same order, literal coordinates.
    let literal = [[0, 0], [60, 0], [40, 20], [20, 20], [20, 40], [0, 40]];
    let vertices = request
        .vertices
        .iter()
        .zip(literal)
        .map(|(v, [x, y])| format!(r#"{{"curve_id":"{}","start_mm":[{x},{y}]}}"#, v.curve_id))
        .collect::<Vec<_>>()
        .join(",");
    std::fs::write(
        &input,
        format!(r#"{{"request_version":1,"vertices":[{vertices}]}}"#),
    )
    .expect("request");
    let cli = root.path().join("cli.fcad");
    let p = std::process::Command::new(ferritecad())
        .arg("edit-sketch-copy")
        .arg(&source)
        .arg("--sketch")
        .arg(id.to_string())
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
    assert_eq!(a.meta(), b.meta());
    assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
    assert_eq!(
        a.dependencies().expect("deps"),
        b.dependencies().expect("deps")
    );
    assert_eq!(
        a.topology_refs().expect("refs"),
        b.topology_refs().expect("refs")
    );
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
    // objects: rowid, id, kind, schema_version, parent_id, ordinal, name, payload,
    // payload_hash. The kind cell is at column 2 of the same row.
    let kind = Value::Text("sketch".into());
    assert_eq!(changed.len(), 2, "{changed:?}");
    assert!(
        changed
            .iter()
            .all(|c| c.0 == "objects" && c.1 == changed[0].1 && c.3 == kind)
    );
    assert_eq!(changed.iter().map(|c| c.2).collect::<Vec<_>>(), [7, 8]);

    verify_published(
        &source,
        &ui,
        &cli,
        &Published {
            vertices: [
                [0., 0.],
                [60., 0.],
                [40., 20.],
                [20., 20.],
                [20., 40.],
                [0., 40.],
            ],
            side_x: 60.,
            high: [60., 40., 10.],
            volume_mm3: 14000.,
            artifacts: ("FCAD_SKETCH_SNAP_ARTIFACT_DIR", "snap"),
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
    println!("\nFCAD_31C_NATIVE_EXECUTED sql_cells=2 volume=14000 triangles=20");
}
