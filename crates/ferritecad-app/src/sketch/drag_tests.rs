// SPDX-License-Identifier: MIT
//! Gesture tests feed actual egui pointer/key events without a window or GPU.
#![allow(clippy::panic)]
use super::{
    tests::{click, frame, replace_field, text_at},
    *,
};

fn setup() -> (egui::Context, Editor) {
    let ctx = egui::Context::default();
    let mut e = Editor::default();
    e.begin();
    e.draft.as_mut().expect("draft").points = [
        ["0", "0"],
        ["60", "0"],
        ["60", "20"],
        ["20", "20"],
        ["20", "40"],
        ["0", "40"],
    ]
    .map(|p| p.map(str::to_owned))
    .to_vec();
    e.draft.as_mut().expect("draft").closed = true;
    frame(&ctx, &mut e, vec![]);
    frame(&ctx, &mut e, vec![]);
    (ctx, e)
}
fn canvas(output: &egui::FullOutput) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Rect(r)
                if (r.rect.width() - 510.).abs() < 1. && (r.rect.height() - 250.).abs() < 1. =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .expect("painted canvas")
}
fn press(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
    frame(ctx, e, vec![egui::Event::PointerMoved(at)]);
    button(ctx, e, at, true);
}
fn button(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2, pressed: bool) {
    frame(
        ctx,
        e,
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }],
    );
}
fn move_to(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) -> egui::FullOutput {
    frame(ctx, e, vec![egui::Event::PointerMoved(at)])
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}
fn vertex(ctx: &egui::Context, e: &mut Editor, i: usize) -> egui::Pos2 {
    let rect = canvas(&frame(ctx, e, vec![]));
    e.canvas.screen(
        rect,
        Canvas::points(e.draft.as_ref().expect("draft")).expect("numbers")[i],
    )
}

#[test]
fn pointer_drag_is_one_precise_undo_step_and_selects_only_its_vertex() {
    let (ctx, mut e) = setup();
    // Strings and coordinates on every untouched axis must survive pixel input.
    e.draft.as_mut().expect("draft").points[0][0] = "0.0000000000000001".into();
    e.draft.as_mut().expect("draft").points[1][1] = "0.0000".into();
    let before = e.draft.clone();
    let at = vertex(&ctx, &mut e, 1) + egui::vec2(3., -2.); // retain grab offset, no jump
    click(&ctx, &mut e, at);
    assert_eq!(e.canvas.selected, Some(1));
    assert_eq!(e.draft, before, "click does not round or edit");
    assert!(e.undo.is_empty());
    press(&ctx, &mut e, at);
    for dx in [8., 24., 56., 80.] {
        move_to(&ctx, &mut e, at + egui::vec2(dx, 0.));
        assert!(
            e.undo.is_empty(),
            "preview must not create per-frame history"
        );
    }
    let preview = e.draft.clone();
    let p = &preview.as_ref().expect("draft").points;
    assert_eq!(p[1], ["80".to_owned(), "0.0000".to_owned()]);
    for i in [0, 2, 3, 4, 5] {
        assert_eq!(p[i], before.as_ref().expect("draft").points[i]);
    }
    button(&ctx, &mut e, at + egui::vec2(80., 0.), false);
    assert_eq!(e.draft, preview);
    assert_eq!(e.undo.len(), 1);
    assert!(e.gesture_finished());
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Undo draft"));
    assert_eq!(e.draft, before);
    assert_eq!(e.redo.len(), 1);
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Redo draft"));
    assert_eq!(e.draft, preview);
    // A second gesture on another vertex is a second checkpoint.
    let at = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(80., 0.));
    button(&ctx, &mut e, at + egui::vec2(80., 0.), false);
    assert_eq!(e.undo.len(), 2);
    assert_eq!(e.draft.as_ref().expect("draft").points[2][0], "80");
    replace_field(&ctx, &mut e, "80", "81.125");
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "81.125");
    let history = e.undo.len();
    let exact = e.draft.clone();
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Fit drawing"));
    assert_eq!(e.draft, exact);
    assert_eq!(e.undo.len(), history);
    let at = vertex(&ctx, &mut e, 1);
    click(&ctx, &mut e, at + egui::vec2(7., 0.));
    assert_eq!(e.canvas.selected, Some(1));
    assert_eq!(e.draft, exact);
    assert_eq!(e.undo.len(), history);
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Reset view"));
    assert_eq!(e.draft, exact);
    assert_eq!(e.undo.len(), history);
}

// No production test-only query API; inspect the draft and the two bounded states.
impl Editor {
    fn gesture_finished(&self) -> bool {
        self.canvas.gesture.is_none() && !self.canvas.claimed_press
    }
}

#[test]
fn pointer_escape_focus_and_outside_release_have_explicit_outcomes() {
    for event in [
        key(egui::Key::Escape),
        egui::Event::WindowFocused(false),
        egui::Event::PointerGone,
    ] {
        let (ctx, mut e) = setup();
        let before = e.draft.clone();
        let at = vertex(&ctx, &mut e, 1);
        press(&ctx, &mut e, at);
        move_to(&ctx, &mut e, at + egui::vec2(24., 5.));
        assert_ne!(e.draft, before);
        frame(&ctx, &mut e, vec![event]);
        assert_eq!(e.draft, before);
        assert!(e.undo.is_empty());
        button(&ctx, &mut e, at + egui::vec2(24., 5.), false);
        assert_eq!(e.draft, before);
        assert!(e.undo.is_empty());
        assert!(e.gesture_finished());
        // A fresh press after cancel really edits, with one new history entry.
        press(&ctx, &mut e, at);
        move_to(&ctx, &mut e, at + egui::vec2(40., 0.));
        button(&ctx, &mut e, at + egui::vec2(40., 0.), false);
        assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "70");
        assert_eq!(e.undo.len(), 1);
    }
    let (ctx, mut e) = setup();
    let before = e.draft.clone();
    let at = vertex(&ctx, &mut e, 1);
    let rect = canvas(&frame(&ctx, &mut e, vec![]));
    let outside = egui::pos2(rect.right() + 24., at.y);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, outside);
    button(&ctx, &mut e, outside, false);
    assert_eq!(
        e.draft.as_ref().expect("draft").points[1][0],
        (60. + f64::from(outside.x - at.x) / 4.).to_string()
    );
    assert_eq!(e.undo.len(), 1);
    assert!(e.gesture_finished());
    e.undo();
    assert_eq!(e.draft, before);
    // Moving away and back is a no-op, including exact strings and redo.
    let redo = e.redo.clone();
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(40., 0.));
    move_to(&ctx, &mut e, at);
    button(&ctx, &mut e, at, false);
    assert_eq!(e.draft, before);
    assert!(e.undo.is_empty());
    assert_eq!(e.redo, redo);
}

#[test]
fn pointer_hit_ties_free_add_and_invalid_preview_use_existing_draft_policy() {
    let (ctx, mut e) = setup();
    e.draft.as_mut().expect("draft").closed = false;
    // Two nearby screen-space candidates: nearest, then stored order on a tie.
    e.draft.as_mut().expect("draft").points[2] = ["62".into(), "0".into()];
    let at = vertex(&ctx, &mut e, 1);
    let before = e.draft.clone();
    click(&ctx, &mut e, at + egui::vec2(4., 0.));
    assert_eq!(e.canvas.selected, Some(1));
    click(&ctx, &mut e, at + egui::vec2(5., 0.));
    assert_eq!(e.canvas.selected, Some(2));
    assert_eq!(e.draft, before);
    assert!(e.undo.is_empty());
    let rect = canvas(&frame(&ctx, &mut e, vec![]));
    let free = e.canvas.screen(rect, [90., 30.]);
    click(&ctx, &mut e, free);
    assert_eq!(e.draft.as_ref().expect("draft").points.len(), 7);
    assert_eq!(
        e.draft.as_ref().expect("draft").points[6],
        ["90.000".to_owned(), "30.000".to_owned()]
    );
    assert_eq!(e.undo.len(), 1);
    let (ctx, mut e) = setup();
    let at = vertex(&ctx, &mut e, 1);
    let other = vertex(&ctx, &mut e, 0);
    press(&ctx, &mut e, at);
    let output = move_to(&ctx, &mut e, other);
    assert!(e.content().is_err());
    assert!(output.shapes.iter().any(
        |c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text().contains("repeated vertices"))
    ));
    button(&ctx, &mut e, other, false);
    assert!(e.content().is_err());
    assert_eq!(e.undo.len(), 1);
    assert!(e.pending.is_none());
    let out = frame(&ctx, &mut e, vec![]);
    assert!(!out.shapes.iter().any(
        |c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text()=="Create in new file…")
    ));
    e.undo();
    assert!(e.content().is_ok());
}

pub(super) fn move_saved_l(ctx: &egui::Context, e: &mut Editor) {
    let original = e.edit_request().expect("original");
    for i in [1, 2] {
        let before = e.draft.clone();
        let history = e.undo.len();
        let at = vertex(ctx, e, i);
        let dx = 20. * e.canvas.scale; // Fit gives 4.75 points/mm for this exact L.
        press(ctx, e, at);
        for fraction in [0.25, 0.5, 0.75, 1.] {
            move_to(ctx, e, at + egui::vec2(dx * fraction, 0.));
        }
        button(ctx, e, at + egui::vec2(dx, 0.), false);
        assert_eq!(e.draft.as_ref().expect("draft").points[i][0], "80");
        assert_eq!(e.undo.len(), history + 1);
        for j in 0..6 {
            if j != i {
                assert_eq!(
                    e.draft.as_ref().expect("draft").points[j],
                    before.as_ref().expect("draft").points[j]
                );
            }
        }
    }
    let modified = e.edit_request().expect("edited");
    assert_eq!(
        original
            .vertices
            .iter()
            .map(|v| v.curve_id)
            .collect::<Vec<_>>(),
        modified
            .vertices
            .iter()
            .map(|v| v.curve_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(original.expected, modified.expected);
    let at = vertex(ctx, e, 1);
    let zero = vertex(ctx, e, 0);
    let before = e.draft.clone();
    let history = e.undo.len();
    press(ctx, e, at);
    move_to(ctx, e, zero);
    assert!(
        e.edit_request().is_err(),
        "saved preview uses common policy"
    );
    let out = frame(ctx, e, vec![]);
    assert!(
        !out.shapes.iter().any(
            |c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text()=="Save edited copy…")
        )
    );
    frame(ctx, e, vec![key(egui::Key::Escape)]);
    button(ctx, e, zero, false);
    assert_eq!(e.draft, before);
    assert_eq!(e.undo.len(), history);
    assert!(e.edit_request().is_ok());
}

pub(super) fn attach_source_claim(source: &std::path::Path) {
    let db = rusqlite::Connection::open(source).expect("SQL");
    let bytes = b"attached Sketch source must remain owned after pointer editing";
    db.execute("INSERT INTO imported_sources(id,format,bytes,content_hash,byte_len,created_at) VALUES(zeroblob(16),'future.sketch-source',?1,?2,?3,'saved')",
        rusqlite::params![bytes.as_slice(),ferritecad_types::ContentHash::of_bytes(bytes).as_bytes().as_slice(),bytes.len()]).expect("source bytes");
    db.execute_batch("INSERT INTO imported_source_refs SELECT id,zeroblob(16) FROM objects WHERE kind='sketch'; CREATE TABLE extension(value BLOB); INSERT INTO extension(rowid,value) VALUES(73,X'00FF'); UPDATE capabilities SET rowid=rowid+100; INSERT INTO capabilities(rowid,name,required) VALUES(999,'future.optional',0);").expect("claims and other SQL");
}
fn sql_facts(
    path: &std::path::Path,
) -> std::collections::BTreeMap<String, Vec<Vec<rusqlite::types::Value>>> {
    use rusqlite::{Connection, OpenFlags};
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("SQL");
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .expect("tables")
        .query_map([], |r| r.get(0))
        .expect("rows")
        .collect::<rusqlite::Result<_>>()
        .expect("names");
    names
        .into_iter()
        .map(|name| {
            let quoted = name.replace('"', "\"\"");
            let mut query = db
                .prepare(&format!("SELECT rowid,* FROM \"{quoted}\" ORDER BY rowid"))
                .or_else(|_| db.prepare(&format!("SELECT * FROM \"{quoted}\"")))
                .expect("table");
            let n = query.column_count();
            let rows = query
                .query_map([], |r| {
                    (0..n)
                        .map(|i| r.get(i))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .expect("rows")
                .collect::<rusqlite::Result<_>>()
                .expect("cells");
            (name, rows)
        })
        .collect()
}
pub(super) fn verify_publications(
    source: &std::path::Path,
    ui: &std::path::Path,
    cli: &std::path::Path,
) {
    use crate::creates::tests::ferritecad;
    use ferritecad_document::Document;
    use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
    let a = sql_facts(ui);
    assert_eq!(a, sql_facts(cli), "all SQL cells, including rowids");
    for (table, rows) in sql_facts(source) {
        if table != "objects" {
            assert_eq!(a[&table], rows, "preserve {table}");
        }
    }
    let d = Document::open_read_only(ui).expect("UI copy");
    let original = Document::open_read_only(source).expect("source");
    let choices = ExtrudeEditSource::read(&d).expect("catalog");
    assert_eq!(
        choices.sketches[0]
            .vertices
            .as_ref()
            .expect("vertices")
            .iter()
            .map(|v| v.start_mm)
            .collect::<Vec<_>>(),
        [
            [0., 0.],
            [80., 0.],
            [80., 20.],
            [20., 20.],
            [20., 40.],
            [0., 40.]
        ]
    );
    let selected = choices.sketches[0].sketch;
    for old in original.objects().expect("objects") {
        let new = d.object(old.id).expect("object").expect("same ID");
        if old.id != selected {
            assert_eq!(old, new);
        } else {
            assert_eq!(
                (old.id, old.name, old.parent, old.ordinal),
                (new.id, new.name, new.parent, new.ordinal)
            );
        }
    }
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&d, &mut kernel, &context).expect("cold rebuild");
    let refs = d.topology_refs().expect("refs");
    assert_eq!(refs.len(), 3);
    for r in refs {
        let faces = built.resolve(&r).expect("preserved ref");
        assert_eq!(faces.len(), 1);
        let face = faces[0];
        let mesh = kernel
            .tessellate(face.shape(), &TessellationParams::default(), &context)
            .expect("face mesh");
        let range = mesh
            .faces
            .iter()
            .find(|m| m.face == face)
            .expect("actual face");
        let points: Vec<_> = mesh.indices
            [range.first_index as usize..(range.first_index + range.index_count) as usize]
            .iter()
            .map(|&i| &mesh.positions[3 * i as usize..3 * i as usize + 3])
            .collect();
        use ferritecad_document::{CapSide, SemanticRole};
        match r.output_role {
            SemanticRole::ExtrudeCap {
                side: CapSide::Start,
            } => assert!(points.iter().all(|p| p[2] == 0.)),
            SemanticRole::ExtrudeCap { side: CapSide::End } => {
                assert!(points.iter().all(|p| p[2] == 10.))
            }
            SemanticRole::ExtrudeSide { profile_segment } => {
                assert_eq!(
                    profile_segment,
                    choices.sketches[0].vertices.as_ref().expect("vertices")[0].curve_id
                );
                assert!(points.iter().all(|p| p[1] == 0.));
                assert!(points.iter().any(|p| p[0] == 80.));
            }
            _ => panic!("unexpected reference"),
        }
    }
    built.release_all(&mut kernel);
    d.close().expect("close");
    original.close().expect("close");
    let mut artifacts = Vec::new();
    for path in [ui, cli] {
        for command in ["validate", "rebuild"] {
            let mut cmd = std::process::Command::new(ferritecad());
            cmd.arg(command).arg(path);
            if command == "rebuild" {
                cmd.arg("--cold");
            }
            let output = cmd.output().expect("process");
            assert!(output.status.success(), "{output:?}");
        }
        let mut files = Vec::new();
        for (op, suffix) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
            let out = path.with_extension(suffix);
            let p = std::process::Command::new(ferritecad())
                .arg(op)
                .arg(path)
                .arg("-o")
                .arg(&out)
                .arg("--json")
                .output()
                .expect("export");
            assert!(p.status.success(), "{p:?}");
            files.push(std::fs::read(&out).expect("artifact"));
        }
        artifacts.push(files);
    }
    assert_eq!(artifacts[0], artifacts[1]);
    let bytes = &artifacts[0][0];
    let triangles = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + triangles * 50);
    assert_eq!(triangles, 20);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut volume6 = 0.;
    for i in 0..triangles {
        let v: [f64; 9] = std::array::from_fn(|j| {
            let at = 84 + 50 * i + 12 + 4 * j;
            f64::from(f32::from_le_bytes(
                bytes[at..at + 4].try_into().expect("f32"),
            ))
        });
        for p in v.chunks_exact(3) {
            for j in 0..3 {
                lo[j] = lo[j].min(p[j]);
                hi[j] = hi[j].max(p[j]);
            }
        }
        volume6 += v[0] * (v[4] * v[8] - v[5] * v[7])
            + v[1] * (v[5] * v[6] - v[3] * v[8])
            + v[2] * (v[3] * v[7] - v[4] * v[6]);
    }
    assert_eq!(lo, [0., 0., 0.]);
    assert_eq!(hi, [80., 40., 10.]);
    // Same 1 ppm tolerance as §25B; planar integer vertices are exact in f32.
    assert!((volume6.abs() / 6. - 20000.).abs() < 0.02);
    if let Some(dir) = std::env::var_os("FCAD_SKETCH_DRAG_ARTIFACT_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifact directory");
        for (from, name) in [
            (ui.to_path_buf(), "drag.fcad"),
            (ui.with_extension("stl"), "drag.stl"),
            (ui.with_extension("fbx"), "drag.fbx"),
        ] {
            std::fs::copy(from, dir.join(name)).expect("retain artifacts for independent reader");
        }
    }
}

#[test]
fn native_dragged_sketch_worker_and_cli_preserve_identity_and_geometry() {
    super::tests::saved_sketch_and_cli(true);
}

#[test]
fn coalesced_pointer_events_keep_press_target_and_history_stays_bounded() {
    let (ctx, mut e) = setup();
    let before = e.draft.clone().expect("draft");
    e.undo = vec![before.clone(); 128];
    let at = vertex(&ctx, &mut e, 1);
    let to = at + egui::vec2(20., -20.);
    frame(
        &ctx,
        &mut e,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerMoved(to),
        ],
    );
    button(&ctx, &mut e, to, false);
    assert_eq!(
        e.draft.as_ref().expect("draft").points[1],
        ["65".to_owned(), "5".to_owned()]
    );
    for i in [0, 2, 3, 4, 5] {
        assert_eq!(e.draft.as_ref().expect("draft").points[i], before.points[i]);
    }
    assert_eq!(e.undo.len(), 128);
    e.undo();
    assert_eq!(e.draft, Some(before));
    let mut idle = frame(&ctx, &mut e, vec![]);
    for _ in 0..30 {
        idle = frame(&ctx, &mut e, vec![]);
    }
    assert!(
        idle.viewport_output[&egui::ViewportId::ROOT].repaint_delay > std::time::Duration::ZERO,
        "settled canvas must not request a perpetual idle repaint"
    );
}

#[test]
fn saved_winding_and_crossing_previews_refuse_through_shared_policy() {
    let (ctx, mut e) = setup();
    e.draft.as_mut().expect("draft").points = [["0", "0"], ["60", "0"], ["0", "40"]]
        .map(|p| p.map(str::to_owned))
        .to_vec();
    let vertices: Vec<_> = Canvas::points(e.draft.as_ref().expect("draft"))
        .expect("points")
        .into_iter()
        .map(|start_mm| SketchVertex {
            curve_id: ferritecad_types::StableEntityId::new(),
            start_mm,
        })
        .collect();
    let id = ferritecad_types::ObjectId::new();
    let choice = SketchChoice {
        sketch: id,
        name: None,
        vertices: Some(vertices.clone()),
        height_mm: Some(10.),
        refusal: None,
    };
    e.editing = Some((
        EditSketchRequest {
            source: PathBuf::from("accepted.fcad"),
            destination: PathBuf::new(),
            sketch: id,
            vertices,
            expected: ferritecad_document::DocumentVersion {
                document_id: ferritecad_types::DocumentId::new(),
                content: ferritecad_types::ContentHash::of_bytes(b"accepted snapshot"),
            },
        },
        choice,
    ));
    frame(&ctx, &mut e, vec![]);
    frame(&ctx, &mut e, vec![]); // changed editor window identity: finish its first layout
    let at = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(0., 320.));
    let error = e
        .edit_request()
        .expect_err("winding must not change")
        .to_string();
    assert!(error.contains("winding"));
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        out.shapes
            .iter()
            .any(|c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text()==error))
    );
    button(&ctx, &mut e, at + egui::vec2(0., 320.), false);
    assert_eq!(e.undo.len(), 1);
    assert!(e.edit_request().is_err());
    e.undo();
    assert!(e.edit_request().is_ok());
    let (ctx, mut e) = setup();
    let at = vertex(&ctx, &mut e, 1);
    press(&ctx, &mut e, at);
    let out = move_to(&ctx, &mut e, at + egui::vec2(-200., -120.));
    let error = e.content().expect_err("crossing preview").to_string();
    assert!(error.contains("intersect"));
    assert!(
        out.shapes
            .iter()
            .any(|c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text()==error))
    );
}

#[test]
fn table_scroll_and_numeric_escape_do_not_start_canvas_gestures() {
    let (ctx, mut e) = setup();
    e.draft.as_mut().expect("draft").points = (0..20)
        .map(|i| [(i * 10).to_string(), (i * 2).to_string()])
        .collect();
    let before = e.draft.clone();
    let out = frame(&ctx, &mut e, vec![]);
    let at = text_at(&out, "2  X mm");
    let visible = |out: &egui::FullOutput, label: &str| {
        out.shapes.iter().any(|c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text()==label && c.clip_rect.contains_rect(t.visual_bounding_rect())))
    };
    assert!(visible(&out, "1  X mm"));
    frame(
        &ctx,
        &mut e,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0., -140.),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            },
        ],
    );
    let mut out = frame(&ctx, &mut e, vec![]);
    for _ in 0..12 {
        out = frame(&ctx, &mut e, vec![]);
    }
    assert!(!visible(&out, "1  X mm"), "table scrolled");
    assert_eq!(e.draft, before);
    assert!(e.undo.is_empty());
    assert!(e.gesture_finished());
    let (ctx, mut e) = setup();
    replace_field(&ctx, &mut e, "60", "61.125");
    let before = e.draft.clone();
    let history = e.undo.len();
    frame(&ctx, &mut e, vec![key(egui::Key::Escape)]);
    assert_eq!(e.draft, before);
    assert_eq!(e.undo.len(), history);
    assert!(e.gesture_finished());
    assert!(e.canvas.selected.is_none());
}

#[test]
fn single_frame_drag_uses_press_and_release_even_after_later_hover() {
    for outside in [false, true] {
        let (ctx, mut e) = setup();
        let before = e.draft.clone().expect("draft");
        let at = vertex(&ctx, &mut e, 1);
        let dx = if outside { 400. } else { 80. };
        let to = at + egui::vec2(dx, 0.);
        frame(&ctx, &mut e, vec![egui::Event::PointerMoved(at)]);
        frame(
            &ctx,
            &mut e,
            vec![
                egui::Event::PointerButton {
                    pos: at,
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
                egui::Event::PointerMoved(at + egui::vec2(0., 30.)),
            ],
        );
        assert_eq!(
            e.draft.as_ref().expect("draft").points[1],
            [(60. + f64::from(dx) / 4.).to_string(), "0".into()]
        );
        for i in [0, 2, 3, 4, 5] {
            assert_eq!(e.draft.as_ref().expect("draft").points[i], before.points[i]);
        }
        assert_eq!(e.canvas.selected, Some(1));
        assert_eq!(e.undo.len(), 1);
        assert!(e.gesture_finished());
        e.undo();
        assert_eq!(e.draft, Some(before));
    }
}

#[test]
fn release_then_next_press_in_one_frame_keeps_the_next_drag_active() {
    let (ctx, mut e) = setup();
    let first = vertex(&ctx, &mut e, 1);
    let second = vertex(&ctx, &mut e, 2);
    let event = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    press(&ctx, &mut e, first);
    frame(
        &ctx,
        &mut e,
        vec![
            egui::Event::PointerMoved(first + egui::vec2(80., 0.)),
            event(first + egui::vec2(80., 0.), false),
            egui::Event::PointerMoved(second),
            event(second, true),
        ],
    );
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
    assert!(e.canvas.gesture.is_some(), "next press starts its own drag");
    move_to(&ctx, &mut e, second + egui::vec2(80., 0.));
    button(&ctx, &mut e, second + egui::vec2(80., 0.), false);
    assert_eq!(e.draft.as_ref().expect("draft").points[2][0], "80");
    assert_eq!(e.undo.len(), 2);
    e.undo();
    assert_eq!(e.draft.as_ref().expect("draft").points[2][0], "60");
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
}

#[test]
fn several_gestures_in_one_frame_keep_changed_checkpoints_in_order() {
    let (ctx, mut e) = setup();
    let original = e.draft.clone();
    let at = vertex(&ctx, &mut e, 1);
    let moved = at + egui::vec2(80., 0.);
    let mut events = Vec::new();
    // A no-op, then A -> B -> A. Comparing only the frame's final draft
    // would discard both changed gestures (or record the no-op incorrectly).
    for (press, release) in [(at, at), (at, moved), (moved, at)] {
        events.extend([
            egui::Event::PointerMoved(press),
            egui::Event::PointerButton {
                pos: press,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerMoved(release),
            egui::Event::PointerButton {
                pos: release,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]);
    }
    frame(&ctx, &mut e, events);
    assert_eq!(e.draft, original);
    assert_eq!(e.undo.len(), 2);
    assert!(e.gesture_finished());
    e.undo();
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
    e.undo();
    assert_eq!(e.draft, original);
    assert!(e.undo.is_empty());
    e.redo();
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
    e.redo();
    assert_eq!(e.draft, original);
}

#[test]
fn cancellation_before_release_in_one_frame_does_not_publish_a_gesture() {
    for cancel in [
        key(egui::Key::Escape),
        egui::Event::PointerGone,
        egui::Event::WindowFocused(false),
    ] {
        let (ctx, mut e) = setup();
        let original = e.draft.clone();
        let at = vertex(&ctx, &mut e, 1);
        press(&ctx, &mut e, at);
        let moved = at + egui::vec2(80., 0.);
        frame(
            &ctx,
            &mut e,
            vec![
                egui::Event::PointerMoved(moved),
                cancel,
                egui::Event::PointerButton {
                    pos: moved,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
        );
        assert_eq!(e.draft, original);
        assert!(e.undo.is_empty());
        assert!(e.gesture_finished());
    }
}

#[test]
fn completed_pointer_gesture_does_not_edit_through_an_overlapping_window() {
    let (ctx, mut e) = setup();
    let before = e.draft.clone();
    let at = vertex(&ctx, &mut e, 1);
    let to = at + egui::vec2(80., 0.);
    let mut draw = |events| {
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
                e.draw(ui, true, false);
                egui::Area::new(egui::Id::new("cover-canvas"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(at - egui::vec2(20., 20.))
                    .show(ui.ctx(), |ui| {
                        ui.allocate_exact_size(
                            egui::vec2(150., 60.),
                            egui::Sense::click_and_drag(),
                        );
                    });
            },
        )
    };
    let _ = draw(vec![]);
    let _ = draw(vec![]);
    let _ = draw(vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton {
            pos: at,
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
    ]);
    assert_eq!(e.draft, before);
    assert_eq!(e.canvas.selected, None);
    assert!(e.undo.is_empty());
}
