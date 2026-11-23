// SPDX-License-Identifier: MIT
//! §31D-1: gesture guides and the temporary Length through the production canvas
//! and editor. Real egui pointer, key and text events go in; draft strings, history
//! and what was painted come out. No window and no graphics device, and nothing
//! here calls a private helper in place of the interface.
#![allow(clippy::panic)]
use super::{
    drag_tests::{button, key, move_to, press, vertex},
    object_snap_tests::{
        L, at, choose, context, label_at, painted, painted_containing, pair, saved, strings,
    },
    tests::{click, frame, frame_running, text_at},
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
pub(super) fn near(a: egui::Pos2, b: egui::Pos2) -> bool {
    a.distance(b) < 0.01
}
pub(super) fn gesture_is_over(e: &Editor) -> bool {
    e.canvas.gesture.is_none() && !e.canvas.claimed_press
}
pub(super) fn history(e: &Editor) -> (usize, usize) {
    (e.undo.len(), e.redo.len())
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
fn off_busy_and_covered_canvases_offer_no_guide() {
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

    // Covered by another window: no hover, no grab, no new point through it.
    let (ctx, mut e, _) = saved(&L);
    guides(&ctx, &mut e, "Orthogonal");
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
                e.draw(ui, false, "");
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
fn typed_text_stays_exact_and_restore_saved_vertices_keeps_the_editor_settings() {
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
