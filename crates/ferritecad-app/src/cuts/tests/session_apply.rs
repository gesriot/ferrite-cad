// SPDX-License-Identifier: MIT
//! §30F: real egui widgets and the same predicate the window handler uses.
use super::*;
use crate::{Loads, can_apply_cut, creates, edits, exports, sessions};

pub(crate) const OUTLINE: [[f64; 2]; 6] = [
    [-2.5, -1.25],
    [84., -1.25],
    [84., 28.],
    [54., 28.],
    [54., 55.],
    [-2.5, 55.],
];
/// Structural fixtures require no native geometry. Native gates independently
/// cold rebuild them and compare real workers with the shipped CLI.
pub(crate) fn fixture(count: usize) -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("cuts.fcad");
    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            &path,
            ferritecad_jobs::NewDocument::SketchExtrude(
                ferritecad_document::PolygonExtrusion::new(OUTLINE.to_vec(), 12.75)
                    .expect("outline"),
            ),
            "test",
        ),
        || Ok(ferritecad_kernel::mock::MockKernel::new()),
        &OperationContext::default(),
    )
    .expect("structural base");
    let mut d = Document::open(&path).expect("document");
    let body = ExtrudeEditSource::read(&d).expect("reading").cut_bodies[0].body;
    for i in 0..count {
        let slot = i * 7 % 16;
        let cut = CircularCut {
            center_mm: [
                7.125 + (slot % 4) as f64 * 12.,
                6.375 + (slot / 4) as f64 * 12.,
            ],
            radius_mm: 1.375 + (i % 3) as f64 * 0.25,
            extent: if i % 2 == 0 {
                CutExtent::ThroughAll
            } else {
                CutExtent::Blind {
                    depth_mm: 4.375 + (i % 3) as f64,
                }
            },
        };
        let prepared = ferritecad_document::prepare_circular_cut(&d, body, &cut).expect("cut");
        d.write_circular_cut(&prepared).expect("write");
    }
    d.close().expect("close");
    let db = rusqlite::Connection::open(&path).expect("fixture SQL");
    db.execute_batch(
        "UPDATE objects SET rowid=-rowid, ordinal=100-ordinal, name='same';
        UPDATE capabilities SET rowid=rowid+100;
        INSERT INTO capabilities(rowid,name,required) VALUES(900,'optional.extension',0);
        CREATE TABLE extra_data(id INTEGER PRIMARY KEY,bytes BLOB);
        INSERT INTO extra_data VALUES(91,x'010203');",
    )
    .expect("unordered extended fixture");
    drop(db);
    let reading = ferritecad_jobs::read_extrude_source(&path).expect("reading");
    (root, path, reading)
}
pub(crate) fn selected(
    reading: &ExtrudeEditSource,
    index: usize,
) -> ferritecad_document::SavedCircularCut {
    let any = reading
        .cut_features
        .iter()
        .find_map(|c| c.saved.as_ref())
        .expect("Cut");
    let id = any.tools[index].feature;
    reading
        .cut_features
        .iter()
        .find_map(|c| c.saved.as_ref().filter(|s| s.feature == id))
        .expect("selected UUID")
        .clone()
}
fn ready(c: &creates::Creates, s: &sessions::Sessions) -> bool {
    can_apply_cut(
        c,
        &Loads::default(),
        &exports::Exports::default(),
        &edits::Edits::default(),
        s,
    )
}
fn tell(c: &mut creates::Creates, s: &sessions::Sessions) {
    c.sketch.cuts.set_session(true, ready(c, s), s.dirty());
}
fn opened(path: &Path) -> sessions::Sessions {
    let mut s = sessions::Sessions::default();
    s.adopt(ferritecad_jobs::DocumentSession::open(path).expect("session"));
    s
}
/// Shared by native gates: the request must come from real widgets with the
/// production predicate, with no manual can_apply override.
pub(crate) fn typed_cut(
    path: &Path,
    reading: &ExtrudeEditSource,
    s: &sessions::Sessions,
    index: usize,
    x: &str,
    radius: &str,
    depth: Option<&str>,
) -> (crate::sketch::Editor, EditCircularCutRequest) {
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .cuts
            .begin_edit(path, reading, selected(reading, index).feature)
    );
    assert!(
        ready(&c, s),
        "own form must allow a changed request in idle"
    );
    tell(&mut c, s);
    let ctx = egui::Context::default();
    let e = &mut c.sketch.cuts;
    for _ in 0..3 {
        frame(&ctx, e, false);
    }
    enter_field(&ctx, e, "Centre X (mm):", x);
    enter_field(&ctx, e, "Radius (mm):", radius);
    if let Some(depth) = depth {
        click(&ctx, e, "Blind depth");
        enter_field(&ctx, e, "Depth (mm):", depth);
    }
    click(&ctx, e, "Apply cut");
    let request = e
        .take_apply_request()
        .expect("changed valid widget request");
    (c.sketch, request)
}
pub(crate) fn draft_state(e: &crate::sketch::Editor) -> String {
    format!("{:?}", e.cuts.draft)
}
#[test]
fn cut_apply_current_fields_draft_history_noop_and_add_exclusion() {
    let (_root, path, reading) = fixture(3);
    let s = opened(&path);
    let saved = selected(&reading, 0);
    let mut c = creates::Creates::default();
    assert!(!ready(&c, &s));
    assert!(c.sketch.cuts.begin_edit(&path, &reading, saved.feature));
    tell(&mut c, &s);
    let ctx = egui::Context::default();
    let e = &mut c.sketch.cuts;
    for _ in 0..3 {
        frame(&ctx, e, false);
    }
    enter_field(&ctx, e, "Centre X (mm):", "7.500");
    click(&ctx, e, "Apply cut");
    let r = e.take_apply_request().expect("unconfirmed fields");
    assert_eq!(r.edit.center_mm[0], 7.5);
    assert_eq!(r.cut, saved.feature);
    assert_eq!(r.edit.tool_curve, saved.tool_curve);
    assert_eq!(r.expected, reading.version);
    assert_eq!(r.edit.extent, CutExtent::ThroughAll);
    assert!(e.draft.as_ref().expect("draft").history.undo.is_empty());
    click(&ctx, e, "Confirm draft numbers");
    assert!(e.take_apply_request().is_none());
    click(&ctx, e, "Undo");
    enter_field(&ctx, e, "Centre X (mm):", "7.12500");
    // Unused text is deliberately different, but the same ThroughAll intent.
    e.draft.as_mut().expect("draft").typed.depth = "unused junk".into();
    click(&ctx, e, "Apply cut");
    assert!(e.take_apply_request().is_none());
    assert_eq!(e.draft.as_ref().expect("draft").history.redo.len(), 1);
    assert!(painted(&frame(&ctx, e, false), "nothing to apply"));
    click(&ctx, e, "Redo");
    assert_eq!(e.draft.as_ref().expect("draft").typed.center_x, "7.500");
    click(&ctx, e, "Save cut copy…");
    assert!(
        e.take_edit_request().is_some(),
        "clean copy remains available"
    );
    e.dismiss();
    assert!(e.begin(&path, &reading, reading.cut_bodies[0].body));
    assert!(!ready(&c, &s), "Add has no document Apply");
    let out = frame(&ctx, &mut c.sketch.cuts, false);
    assert!(!painted(&out, "Confirm draft numbers"));
    click(&ctx, &mut c.sketch.cuts, "Apply cut");
    assert!(c.sketch.cuts.take_apply_request().is_none());
}
#[test]
fn cut_apply_busy_guard_proves_idle_request_then_excludes_other_work_and_forms() {
    let (_root, path, reading) = fixture(3);
    let mut s = opened(&path);
    let (form, _) = typed_cut(&path, &reading, &s, 0, "7.5", "1.625", None);
    let mut c = creates::Creates::default();
    c.sketch = form;
    // The helper already submitted a changed valid request while idle.
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut c.sketch.cuts, false);
    }
    let mut loads = Loads::default();
    loads
        .open(
            Some(&path),
            std::sync::Arc::new(crate::ProgressRelay::default()),
            |_, _| std::thread::spawn(|| {}),
        )
        .expect("load");
    let mut ex = exports::Exports::default();
    for mode in 0..5 {
        loads.stop_all();
        loads = Loads::default();
        ex.stop_all();
        ex = exports::Exports::default();
        let mut ed = edits::Edits::default();
        let mut generation = None;
        match mode {
            0 => {
                loads
                    .open(
                        Some(&path),
                        std::sync::Arc::new(crate::ProgressRelay::default()),
                        |_, _| std::thread::spawn(|| {}),
                    )
                    .expect("load");
            }
            1 => {
                generation = s.begin_apply(|_, _, _| std::thread::spawn(|| {}));
            }
            2 => {
                exports::begin_export(
                    &mut ex,
                    &mut crate::ViewportInput::default(),
                    Some(&path),
                    Some(path.with_extension("fbx")),
                    |_, _, _, _| std::thread::spawn(|| {}),
                )
                .expect("export");
            }
            3 => {
                assert!(ed.begin(&path, &reading));
            }
            _ => {
                let form = std::mem::take(&mut c.sketch);
                assert!(creates::open_form(
                    &mut c,
                    &mut crate::ViewportInput::default()
                ));
                c.sketch = form;
            }
        }
        let available = can_apply_cut(&c, &loads, &ex, &ed, &s);
        assert!(!available, "busy mode {mode}");
        c.sketch.cuts.set_session(true, available, s.dirty());
        click(&ctx, &mut c.sketch.cuts, "Apply cut");
        assert!(c.sketch.cuts.take_apply_request().is_none());
        if let Some(g) = generation {
            s.cancel();
            s.finish_apply(g, Err(CadError::Cancelled));
        }
    }
    c = creates::Creates::default();
    c.sketch.cuts.dismiss();
    assert!(
        c.sketch
            .begin_edit(&path, &reading, selected(&reading, 0).profile_sketch)
    );
    assert!(!ready(&c, &s));
    c.sketch.dismiss();
    assert!(
        c.sketch
            .cuts
            .begin_edit(&path, &reading, selected(&reading, 0).feature)
    );
    assert!(!ready(&c, &sessions::Sessions::default()));
}
#[test]
fn cut_apply_dirty_discovery_copy_refusals_status_and_scene_dismissal() {
    let (_root, path, reading) = fixture(3);
    let original = std::fs::read(&path).expect("source");
    let mut s = opened(&path);
    // A real accepted structural change makes the session dirty without OCCT.
    let (tx, rx) = std::sync::mpsc::channel();
    let id = selected(&reading, 0).base_feature;
    let g = s
        .begin_apply(|t, _, _| {
            std::thread::spawn(move || {
                tx.send(t.run(|src, _, dst| {
                    std::fs::copy(src, dst).expect("copy");
                    let mut d = Document::open(dst)?;
                    let o = d.object(id)?.expect("base");
                    d.write(|w| {
                        w.put_object(id, o.parent, o.ordinal, Some("Dirty base"), &o.payload)
                    })?;
                    d.close()
                }))
                .expect("deliver");
            })
        })
        .expect("step");
    assert!(matches!(
        s.finish_apply(g, rx.recv().expect("result")),
        sessions::Edited::Show(_)
    ));
    s.bind(sessions::Bind::Staged).expect("bind");
    s.finish_scene(g, Ok(()));
    assert!(s.dirty());
    let current = s.export_path().expect("current");
    let reading = ferritecad_jobs::read_extrude_source(&current).expect("reading");
    let saved = selected(&reading, 0);
    let mut c = creates::Creates::default();
    tell(&mut c, &s);
    let ctx = egui::Context::default();
    let label = format!(
        "Edit cut {} — {}…",
        reading
            .cut_features
            .iter()
            .find(|f| f.feature == saved.feature)
            .expect("choice")
            .name
            .as_deref()
            .unwrap_or("Unnamed cut"),
        saved.feature
    );
    let mut output = None;
    for _ in 0..3 {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                ..Default::default()
            },
            |ui| {
                c.sketch
                    .cuts
                    .choices(ui, false, Some(&current), Some(&reading))
            },
        );
        out.textures_delta.clear();
        output = Some(out);
    }
    let out = output.expect("frame");
    let at = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .expect("dirty edit button");
    for pressed in [true, false] {
        let mut out = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            |ui| {
                c.sketch
                    .cuts
                    .choices(ui, false, Some(&current), Some(&reading))
            },
        );
        out.textures_delta.clear();
    }
    assert!(
        c.sketch.cuts.editing_saved(),
        "dirty discovery must open Edit"
    );
    assert!(ready(&c, &s));
    tell(&mut c, &s);
    let e = &mut c.sketch.cuts;
    for _ in 0..3 {
        frame(&ctx, e, false);
    }
    for (x, radius) in [
        ("70", "1.375"),
        ("7.125", "-1"),
        ("7.125", "NaN"),
        ("31.125", "1.625"),
    ] {
        fill(&ctx, e, x, "42.375", radius, "");
        click(&ctx, e, "Apply cut");
        assert!(e.take_apply_request().is_none());
        assert_eq!(e.draft.as_ref().expect("draft").typed.center_x, x);
    }
    fill(&ctx, e, "7.5", "6.375", "1.625", "");
    click(&ctx, e, "Confirm draft numbers");
    assert!(painted(&frame(&ctx, e, false), "unsaved changes"));
    click(&ctx, e, "Save cut copy…");
    assert!(e.take_edit_request().is_none());
    click(&ctx, e, "Apply cut");
    assert!(e.take_apply_request().is_some());
    e.set_outcome("Could not apply: protected floor UUID remains named");
    let out = frame(&ctx, e, false);
    assert!(painted(&out, "Document: Could not apply"));
    let text = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text().starts_with("Document:") => {
                Some((s.clip_rect, t.visual_bounding_rect()))
            }
            _ => None,
        })
        .expect("status");
    assert!(text.0.contains(text.1.center()), "status inside form");
    c.sketch.finish_session_change();
    assert!(!c.sketch.active());
    assert_eq!(std::fs::read(path).expect("source"), original);
}
