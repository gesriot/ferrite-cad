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
pub(super) const L: [[&str; 2]; 6] = [
    ["0", "0"],
    ["60", "0"],
    ["60", "20"],
    ["20", "20"],
    ["20", "40"],
    ["0", "40"],
];
/// An open zigzag with exactly one crossing, at (10, 10), between its first and
/// last edges; midpoints (15, 15), (30, 15) and (15, 7.5).
pub(super) const ZIGZAG: [[&str; 2]; 4] = [["0", "0"], ["30", "30"], ["30", "0"], ["0", "15"]];

/// A context whose windows appear at once, so painted colours are the real ones
/// (a fading window paints everything translucent).
pub(super) fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    ctx
}

/// The same drawing as a saved Sketch being edited: the form that has Apply and
/// Save, with no document behind it.
pub(super) fn saved(points: &[[&str; 2]]) -> (egui::Context, Editor, EditRequest) {
    let ctx = context();
    let mut e = Editor::default();
    e.draft = Some(State {
        points: points.iter().map(|p| p.map(str::to_owned)).collect(),
        height: "10".into(),
        feature: Feature::Extrude,
        angle: String::new(),
    });
    let vertices: Vec<_> = Canvas::points(e.draft.as_ref().expect("draft"))
        .expect("points")
        .into_iter()
        .map(|start_mm| SketchVertex {
            curve_id: ferritecad_types::StableEntityId::new(),
            start_mm,
        })
        .collect();
    let id = ferritecad_types::ObjectId::new();
    let stored = ferritecad_document::Sketch {
        plane: ferritecad_types::ObjectId::new(),
        constraints: vec![],
        curves: vertices
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let end = vertices[(i + 1) % vertices.len()].start_mm;
                ferritecad_document::SketchCurve {
                    id: v.curve_id,
                    construction: false,
                    geometry: ferritecad_document::SketchGeometry::Line {
                        start: ferritecad_document::Point2::new(v.start_mm[0], v.start_mm[1])
                            .expect("start"),
                        end: ferritecad_document::Point2::new(end[0], end[1]).expect("end"),
                    },
                }
            })
            .collect(),
    };
    let request = EditRequest {
        changes: stored
            .curves
            .iter()
            .map(|c| ferritecad_document::EditChange::SetCurveGeometry {
                sketch: id,
                curve: c.id,
                geometry: c.geometry.clone(),
            })
            .collect(),
        source: PathBuf::from("accepted.fcad"),
        destination: PathBuf::new(),
        expected: ferritecad_document::DocumentVersion {
            document_id: ferritecad_types::DocumentId::new(),
            content: ferritecad_types::ContentHash::of_bytes(b"accepted snapshot"),
        },
    };
    let choice = SketchChoice {
        details: vec![],
        stored: Some(stored),
        sketch: id,
        name: None,
        vertices: Some(vertices),
        profile_use: Some(crate::sketch_facts::SketchProfileUse::BlindExtrude {
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

pub(super) fn rect(ctx: &egui::Context, e: &mut Editor) -> egui::Rect {
    canvas(&frame(ctx, e, vec![]))
}
/// Where a document point (mm) is on the screen now.
pub(super) fn at(ctx: &egui::Context, e: &mut Editor, mm: [f64; 2]) -> egui::Pos2 {
    let rect = rect(ctx, e);
    e.canvas.screen(rect, mm)
}
pub(super) fn switch(ctx: &egui::Context, e: &mut Editor) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, "Object snap"));
}
pub(super) fn choose(ctx: &egui::Context, e: &mut Editor, label: &str) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, label));
}
/// The pointer rests at `pos`; the frame that shows what it would do.
pub(super) fn hover(ctx: &egui::Context, e: &mut Editor, pos: egui::Pos2) -> egui::FullOutput {
    frame(ctx, e, vec![egui::Event::PointerMoved(pos)]);
    frame(ctx, e, vec![])
}
pub(super) fn label_at(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    out.shapes.iter().find_map(|c| match &c.shape {
        egui::Shape::Text(t) if t.galley.text() == text => Some(t.visual_bounding_rect().center()),
        _ => None,
    })
}
pub(super) fn painted(out: &egui::FullOutput, text: &str) -> bool {
    label_at(out, text).is_some()
}
pub(super) fn painted_containing(out: &egui::FullOutput, text: &str) -> bool {
    out.shapes
        .iter()
        .any(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text().contains(text)))
}
/// Where the marker is: the middle of whatever was drawn in the marker's colour.
pub(super) fn marker(out: &egui::FullOutput) -> Option<egui::Pos2> {
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
pub(super) fn strings(e: &Editor, i: usize) -> [String; 2] {
    e.draft.as_ref().expect("draft").points[i].clone()
}
pub(super) fn pair(x: &str, y: &str) -> [String; 2] {
    [x.to_owned(), y.to_owned()]
}
pub(super) fn escape(ctx: &egui::Context, e: &mut Editor) {
    frame(ctx, e, vec![key(egui::Key::Escape)]);
}

#[test]
fn off_busy_and_covered_canvases_offer_no_snap() {
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
    let (ctx, mut e, _) = saved(&ZIGZAG);
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
                e.draw(ui, false, "");
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
    let ids = |r: &EditRequest| {
        crate::transaction_tests::vertices_in(
            r,
            e.editing
                .as_ref()
                .expect("saved")
                .1
                .stored
                .as_ref()
                .expect("sketch"),
        )
        .iter()
        .map(|v| v.curve_id)
        .collect::<Vec<_>>()
    };
    assert_eq!(ids(&snapped), ids(&original));
    assert_eq!(
        crate::transaction_tests::vertices_in(
            &snapped,
            e.editing
                .as_ref()
                .expect("saved")
                .1
                .stored
                .as_ref()
                .expect("sketch")
        )
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
        crate::transaction_tests::vertices_in(
            &by_hand.edit_request().expect("typed"),
            by_hand
                .editing
                .as_ref()
                .expect("saved")
                .1
                .stored
                .as_ref()
                .expect("sketch"),
        )
    };
    assert_eq!(
        typed.iter().map(|v| v.start_mm).collect::<Vec<_>>(),
        crate::transaction_tests::vertices_in(
            &snapped,
            e.editing
                .as_ref()
                .expect("saved")
                .1
                .stored
                .as_ref()
                .expect("sketch")
        )
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
    let literal = [[0, 0], [60, 0], [40, 20], [20, 20], [20, 40], [0, 40]];
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
}
