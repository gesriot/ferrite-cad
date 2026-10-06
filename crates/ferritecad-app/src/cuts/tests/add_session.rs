// SPDX-License-Identifier: MIT
//! §30I: the Cut Add form's document Add, through real egui widgets and the same
//! predicate (`can_add_cut`) the window handler uses. Kernel-free fixtures.
use super::session_apply::{fixture, selected};
use super::*;
use crate::{Loads, can_add_cut, can_apply_cut, creates, edits, exports, sessions};

/// A disk inside the real concave outline and clear of the fixture's three tools.
pub(crate) const FIRST: (&str, &str, &str, &str) = ("19.25", "30.5", "2.125", "4.875");

fn ready(c: &creates::Creates, s: &sessions::Sessions) -> bool {
    can_add_cut(
        c,
        &Loads::default(),
        &exports::Exports::default(),
        &edits::Edits::default(),
        s,
    )
}
fn tell(c: &mut creates::Creates, s: &sessions::Sessions) {
    let edit = can_apply_cut(
        c,
        &Loads::default(),
        &exports::Exports::default(),
        &edits::Edits::default(),
        s,
    );
    c.sketch.cuts.set_session(true, edit, s.dirty());
    c.sketch.cuts.set_add(ready(c, s));
}
pub(crate) fn opened(path: &Path) -> sessions::Sessions {
    let mut s = sessions::Sessions::default();
    s.adopt(ferritecad_jobs::DocumentSession::open(path).expect("session"));
    s
}
fn settle(ctx: &egui::Context, e: &mut Editor) {
    for _ in 0..3 {
        frame(ctx, e, false);
    }
}
fn typed(e: &Editor) -> Numbers {
    e.draft.as_ref().expect("draft").typed.clone()
}

/// The Add form's own widgets, as the window leaves them: fields typed and
/// **Add cut** pressed without confirming the draft. Shared by native gates.
pub(crate) fn typed_add(
    path: &Path,
    reading: &ExtrudeEditSource,
    s: &sessions::Sessions,
    (x, y, radius, depth): (&str, &str, &str, Option<&str>),
) -> (crate::sketch::Editor, CircularCutRequest) {
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .cuts
            .begin(path, reading, reading.cut_bodies[0].body)
    );
    assert!(ready(&c, s), "own Add form must allow its Add in idle");
    tell(&mut c, s);
    let ctx = egui::Context::default();
    let e = &mut c.sketch.cuts;
    settle(&ctx, e);
    enter_field(&ctx, e, "Centre X (mm):", x);
    enter_field(&ctx, e, "Centre Y (mm):", y);
    enter_field(&ctx, e, "Radius (mm):", radius);
    match depth {
        Some(depth) => {
            click(&ctx, e, "Blind depth");
            enter_field(&ctx, e, "Depth (mm):", depth);
        }
        None => click(&ctx, e, "Through all"),
    }
    click(&ctx, e, "Add cut");
    let request = e.take_add_request().expect("valid widget Add request");
    (c.sketch, request)
}

/// Makes the session dirty with a real accepted structural step, no kernel.
pub(crate) fn dirty(s: &mut sessions::Sessions, id: ObjectId) {
    let (tx, rx) = std::sync::mpsc::channel();
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
}

#[test]
fn add_cut_reads_current_fields_draft_history_stays_separate_and_copy_is_clean_only() {
    let (_root, path, reading) = fixture(3);
    let mut s = opened(&path);
    let body = reading.cut_bodies[0].body;
    let mut c = creates::Creates::default();
    assert!(!ready(&c, &s), "nothing to add before the form is open");
    assert!(c.sketch.cuts.begin(&path, &reading, body));
    assert!(c.busy(), "New and Open still wait for this form");
    assert!(ready(&c, &s), "the Add form must not block its own Add");
    assert!(
        !can_apply_cut(
            &c,
            &Loads::default(),
            &exports::Exports::default(),
            &edits::Edits::default(),
            &s
        ),
        "the edit's Apply cut never serves the Add form"
    );
    let ctx = egui::Context::default();
    // The window says "not ready": the button is there and asks for nothing.
    c.sketch.cuts.set_add(false);
    let e = &mut c.sketch.cuts;
    settle(&ctx, e);
    let out = frame(&ctx, e, false);
    for label in [
        "Add cut",
        "Confirm draft numbers",
        "Undo",
        "Redo",
        "Add circular cut",
    ] {
        assert!(painted(&out, label), "{label}");
    }
    assert!(!painted(&out, "Apply cut"), "no edit Apply on the Add form");
    assert!(!painted(&out, "new copy"), "the window title is not a copy");
    let (x, y, r, depth) = FIRST;
    enter_field(&ctx, e, "Centre X (mm):", x);
    enter_field(&ctx, e, "Centre Y (mm):", y);
    enter_field(&ctx, e, "Radius (mm):", r);
    enter_field(&ctx, e, "Depth (mm):", depth);
    click(&ctx, e, "Add cut");
    assert!(e.take_add_request().is_none(), "disabled asked");
    // The window says what it computes: one press, one request, fields unconfirmed.
    tell(&mut c, &s);
    let e = &mut c.sketch.cuts;
    click(&ctx, e, "Add cut");
    let request = e.take_add_request().expect("unconfirmed current fields");
    assert!(e.take_add_request().is_none(), "one press, one request");
    assert_eq!(request.body, body);
    assert_eq!(request.expected, reading.version);
    assert_eq!(request.source, path);
    assert_eq!(request.cut.center_mm, [19.25, 30.5]);
    assert_eq!(request.cut.radius_mm, 2.125);
    assert_eq!(request.cut.extent, CutExtent::Blind { depth_mm: 4.875 });
    assert!(
        e.draft.as_ref().expect("draft").history.undo.is_empty(),
        "Add did not confirm the draft"
    );
    assert!(e.take_apply_request().is_none() && e.take_request().is_none());
    // Through all is read as intent; unused depth text cannot leak in.
    click(&ctx, e, "Through all");
    click(&ctx, e, "Add cut");
    assert_eq!(
        e.take_add_request().expect("through").cut.extent,
        CutExtent::ThroughAll
    );
    click(&ctx, e, "Blind depth");
    // Confirm draft numbers is the draft's only: it asks the window for nothing.
    click(&ctx, e, "Confirm draft numbers");
    assert!(e.take_add_request().is_none() && e.take_request().is_none());
    assert_eq!(e.draft.as_ref().expect("draft").history.undo.len(), 1);
    click(&ctx, e, "Undo");
    assert_eq!(
        typed(e),
        Numbers::default(),
        "draft Undo restores the empty draft"
    );
    assert_eq!(e.draft.as_ref().expect("draft").history.redo.len(), 1);
    click(&ctx, e, "Add cut");
    assert!(e.take_add_request().is_none(), "empty fields are not a Cut");
    assert_eq!(
        e.draft.as_ref().expect("draft").history.redo.len(),
        1,
        "the empty press lost the draft's Redo"
    );
    click(&ctx, e, "Redo");
    assert_eq!(typed(e).radius, "2.125");
    // A clean document keeps the copy workflow.
    click(&ctx, e, "Save cut copy…");
    assert!(e.take_request().is_some(), "clean copy remains available");
    // Dirty: copy disabled with words, Add still adds.
    dirty(&mut s, selected(&reading, 0).base_feature);
    tell(&mut c, &s);
    let e = &mut c.sketch.cuts;
    assert!(painted(&frame(&ctx, e, false), "unsaved changes"));
    click(&ctx, e, "Save cut copy…");
    assert!(e.take_request().is_none(), "a dirty copy asked");
    click(&ctx, e, "Add cut");
    assert!(
        e.take_add_request().is_some(),
        "Add works on a dirty document"
    );
}

#[test]
fn add_cut_busy_guard_proves_idle_request_then_excludes_other_work_and_forms() {
    let (_root, path, reading) = fixture(3);
    let mut s = opened(&path);
    let (sketch, _) = typed_add(
        &path,
        &reading,
        &s,
        ("19.25", "30.5", "2.125", Some("4.875")),
    );
    let mut c = creates::Creates::default();
    c.sketch = sketch;
    let ctx = egui::Context::default();
    settle(&ctx, &mut c.sketch.cuts);
    let mut ex = exports::Exports::default();
    let mut loads = Loads::default();
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
            1 => generation = s.begin_apply(|_, _, _| std::thread::spawn(|| {})),
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
            3 => assert!(ed.begin(&path, &reading)),
            _ => {
                let form = std::mem::take(&mut c.sketch);
                assert!(creates::open_form(
                    &mut c,
                    &mut crate::ViewportInput::default()
                ));
                c.sketch = form;
            }
        }
        let available = can_add_cut(&c, &loads, &ex, &ed, &s);
        assert!(!available, "busy mode {mode}");
        c.sketch.cuts.set_add(available);
        click(&ctx, &mut c.sketch.cuts, "Add cut");
        assert!(c.sketch.cuts.take_add_request().is_none(), "mode {mode}");
        if let Some(g) = generation {
            s.cancel();
            s.finish_apply(g, Err(CadError::Cancelled));
        }
    }
    // While the worker runs the form is disabled: a press starts nothing.
    c = creates::Creates::default();
    assert!(
        c.sketch
            .cuts
            .begin(&path, &reading, reading.cut_bodies[0].body)
    );
    tell(&mut c, &s);
    let e = &mut c.sketch.cuts;
    for _ in 0..3 {
        frame(&ctx, e, true);
    }
    let out = frame(&ctx, e, true);
    let at = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Add cut" => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .expect("Add cut painted while running");
    run(&ctx, e, vec![egui::Event::PointerMoved(at)], true);
    for pressed in [true, false] {
        run(
            &ctx,
            e,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }],
            true,
        );
    }
    assert!(e.take_add_request().is_none(), "a running form asked");
    // An edit form, another editor's form, no session: never Add.
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .cuts
            .begin_edit(&path, &reading, selected(&reading, 0).feature)
    );
    assert!(!ready(&c, &s), "the edit form is not the Add form");
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .begin_edit(&path, &reading, selected(&reading, 0).profile_sketch)
    );
    assert!(!ready(&c, &s));
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .cuts
            .begin(&path, &reading, reading.cut_bodies[0].body)
    );
    assert!(!ready(&c, &sessions::Sessions::default()), "no session");
    c.sketch.cuts.dismiss();
    assert!(!ready(&c, &s), "a closed form");
}

#[test]
fn add_cut_dirty_discovery_refusals_keep_text_status_dismissal_and_unsupported_bodies() {
    let (_root, path, reading) = fixture(3);
    let original = std::fs::read(&path).expect("source");
    let mut s = opened(&path);
    dirty(&mut s, selected(&reading, 0).base_feature);
    let current = s.export_path().expect("current");
    let reading = ferritecad_jobs::read_extrude_source(&current).expect("reading");
    let choice = &reading.cut_bodies[0];
    let label = format!(
        "Cut circle into {} — {}…",
        choice.name.as_deref().unwrap_or("Unnamed body"),
        choice.body
    );
    // Opens on a dirty document only when the window says the session is idle.
    for (session_idle, opens) in [(false, false), (true, true)] {
        let mut c = creates::Creates::default();
        c.sketch.cuts.set_session(session_idle, false, true);
        let ctx = egui::Context::default();
        let draw = |c: &mut creates::Creates, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000., 900.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    c.sketch
                        .cuts
                        .choices(ui, false, Some(&current), Some(&reading))
                },
            );
            out.textures_delta.clear();
            out
        };
        draw(&mut c, vec![]);
        let out = draw(&mut c, vec![]);
        let at = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("Add entry painted");
        draw(&mut c, vec![egui::Event::PointerMoved(at)]);
        for pressed in [true, false] {
            draw(
                &mut c,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
            );
        }
        assert_eq!(c.sketch.cuts.adding(), opens, "session_idle={session_idle}");
    }
    let mut c = creates::Creates::default();
    assert!(c.sketch.cuts.begin(&current, &reading, choice.body));
    tell(&mut c, &s);
    let ctx = egui::Context::default();
    let e = &mut c.sketch.cuts;
    settle(&ctx, e);
    // Inside the box but outside the concave outline; on an existing disk; a
    // negative radius; not a number: the form keeps the text and asks nothing.
    for (x, y, radius) in [
        ("70", "42", "1.375"),
        ("31.125", "42.375", "1.625"),
        ("19.25", "30.5", "-1"),
        ("19.25", "30.5", "NaN"),
    ] {
        fill(&ctx, e, x, y, radius, "4.875");
        click(&ctx, e, "Add cut");
        assert!(e.take_add_request().is_none(), "({x}, {y}) r{radius}");
        assert_eq!(typed(e).center_x, x);
        assert_eq!(typed(e).radius, radius);
    }
    // The document's line is drawn inside the open form, where it can be read.
    e.set_outcome("Could not apply the change: the tool touches Cut 0190");
    let out = frame(&ctx, e, false);
    let text = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text().starts_with("Document: Could not apply") => {
                Some((s.clip_rect, t.visual_bounding_rect()))
            }
            _ => None,
        })
        .expect("status");
    assert!(text.0.contains(text.1.center()), "status inside form");
    c.sketch.finish_session_change();
    assert!(
        !c.sketch.active(),
        "the draft outlived the picture it described"
    );
    assert!(
        c.sketch.cuts.add_session(),
        "the window's word survives the draft"
    );
    assert_eq!(std::fs::read(&path).expect("source"), original);

    // At 16 Cuts the Body offers no Add; a constrained base offers none either.
    let (_root16, path16, full) = fixture(16);
    assert!(
        full.cut_bodies[0].refusal.is_some(),
        "the 17th Cut is offered"
    );
    assert!(!creates::Creates::default().sketch.cuts.begin(
        &path16,
        &full,
        full.cut_bodies[0].body
    ));
    let (_rootc, pathc, plain) = fixture(0);
    let base = plain
        .sketches
        .iter()
        .find(|c| c.vertices.is_some())
        .expect("base");
    let first = &base.vertices.as_ref().expect("vertices")[0];
    let mut d = Document::open(&pathc).expect("document");
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &d,
        base.sketch,
        &ferritecad_document::SketchConstraintEdits {
            remove: vec![],
            add: vec![ferritecad_document::AddSketchConstraint::Line(
                ferritecad_document::AddLineConstraint::Line {
                    curve: first.curve_id,
                    kind: ferritecad_document::LineConstraintKind::Horizontal,
                },
            )],
        },
    )
    .expect("constraint");
    d.write_sketch_constraints(&prepared).expect("write");
    d.close().expect("close");
    let constrained = ferritecad_jobs::read_extrude_source(&pathc).expect("reading");
    assert!(
        constrained.cut_bodies[0].refusal.is_some(),
        "a constrained base is not a Cut base"
    );
    assert!(!creates::Creates::default().sketch.cuts.begin(
        &pathc,
        &constrained,
        constrained.cut_bodies[0].body
    ));
}
