// SPDX-License-Identifier: MIT
//! §30K: the Chamfer Add form's document Add, through real egui widgets and the
//! same predicate (`can_add_chamfer`) the window handler uses. Kernel-free fixtures.
use super::*;
use crate::cuts::tests::add_session::{dirty, opened};
use crate::{
    Loads, can_add_chamfer, can_apply_chamfer_distance, creates, edits, exports, sessions,
};

/// The awkward free plate of §30G's gates with no edge feature yet: reversed
/// rowids/ordinals, every name `same`, moved capability rowids, an optional
/// capability and an unrelated table.
pub(crate) fn plate() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    crate::fillets::tests::session_apply::fixture(0)
}
fn ready(c: &creates::Creates, s: &sessions::Sessions) -> bool {
    can_add_chamfer(
        c,
        &Loads::default(),
        &exports::Exports::default(),
        &edits::Edits::default(),
        s,
    )
}
fn tell(c: &mut creates::Creates, s: &sessions::Sessions) {
    let apply = can_apply_chamfer_distance(
        c,
        &Loads::default(),
        &exports::Exports::default(),
        &edits::Edits::default(),
        s,
    );
    c.sketch.chamfers.set_session(true, apply, s.dirty());
    c.sketch.chamfers.set_add(ready(c, s));
}
fn settle(ctx: &egui::Context, e: &mut Editor) {
    for _ in 0..3 {
        frame(ctx, e, false);
    }
}
fn typed(e: &Editor) -> Typed {
    e.draft.as_ref().expect("draft").typed.clone()
}
fn history(e: &Editor) -> History<Typed> {
    e.draft.as_ref().expect("draft").history.clone()
}
/// The corner of the open draft stored at `at`, and the label its radio shows.
fn corner_at(e: &Editor, at: [f64; 2]) -> (usize, ChamferCorner, String) {
    let draft = e.draft.as_ref().expect("draft");
    let target = draft.choice.target.as_ref().expect("target");
    let i = target
        .corners
        .iter()
        .position(|c| c.corner_mm == at)
        .unwrap_or_else(|| panic!("no corner at {at:?}"));
    let corner = target.corners[i];
    (i, corner, describe_candidate(&corner, target))
}

/// The Add form's own widgets, as the window leaves them: a corner chosen by its
/// stored position, a distance typed and **Add chamfer** pressed without
/// confirming the draft. Shared by the native gates.
pub(crate) fn typed_add(
    path: &Path,
    reading: &ExtrudeEditSource,
    s: &sessions::Sessions,
    at: [f64; 2],
    distance_text: &str,
) -> (crate::sketch::Editor, EdgeChamferRequest) {
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .chamfers
            .begin(path, reading, reading.chamfer_bodies[0].body)
    );
    assert!(ready(&c, s), "own Add form must allow its Add in idle");
    tell(&mut c, s);
    let ctx = egui::Context::default();
    let e = &mut c.sketch.chamfers;
    settle(&ctx, e);
    let (_, corner, label) = corner_at(e, at);
    click(&ctx, e, &label);
    distance(&ctx, e, distance_text);
    click(&ctx, e, "Add chamfer");
    let request = e.take_add_request().expect("valid widget Add request");
    assert_eq!(request.chamfer.edge.joint, corner.joint);
    assert!(e.take_request().is_none(), "Add is not the copy");
    assert!(!history(e).can_undo(), "Add confirmed nothing");
    (c.sketch, request)
}
/// The Add form's whole draft (corner, distance text, request history, refusal).
pub(crate) fn add_draft_state(e: &crate::sketch::Editor) -> String {
    format!("{:?}", e.chamfers.draft)
}

#[test]
fn add_chamfer_reads_current_fields_request_history_stays_separate_and_copy_is_clean_only() {
    let (_root, path, reading) = plate();
    let mut s = opened(&path);
    let body = reading.chamfer_bodies[0].body;
    let base = reading.chamfer_bodies[0]
        .target
        .as_ref()
        .expect("target")
        .base_feature;
    let mut c = creates::Creates::default();
    assert!(!ready(&c, &s), "nothing to add before the form is open");
    assert!(c.sketch.chamfers.begin(&path, &reading, body));
    assert!(c.busy(), "New and Open still wait for this form");
    assert!(ready(&c, &s), "the Add form must not block its own Add");
    assert!(
        !can_apply_chamfer_distance(
            &c,
            &Loads::default(),
            &exports::Exports::default(),
            &edits::Edits::default(),
            &s
        ),
        "the distance edit's Apply never serves the Add form"
    );
    let ctx = egui::Context::default();
    // The window says "not ready": the button is there and asks for nothing.
    c.sketch.chamfers.set_add(false);
    let e = &mut c.sketch.chamfers;
    settle(&ctx, e);
    let out = frame(&ctx, e, false);
    for label in [
        "Add chamfer",
        "Confirm draft edge and distance",
        "Undo request",
        "Redo request",
        "Chamfer one vertical edge",
        "This document changes with Add chamfer",
    ] {
        assert!(painted(&out, label), "{label}");
    }
    assert!(!painted(&out, "Apply distance"), "no distance Apply here");
    assert!(!painted(&out, "Apply chamfer"), "renamed");
    assert!(!painted(&out, "new copy"), "the window title is not a copy");
    let (i, corner, label) = corner_at(e, [-4.5, 15.5]);
    click(&ctx, e, &label);
    distance(&ctx, e, "1.06250");
    click(&ctx, e, "Add chamfer");
    assert!(e.take_add_request().is_none(), "disabled asked");
    // The window says what it computes: one press, one request, nothing confirmed.
    tell(&mut c, &s);
    let e = &mut c.sketch.chamfers;
    click(&ctx, e, "Add chamfer");
    let request = e.take_add_request().expect("unconfirmed current fields");
    assert!(e.take_add_request().is_none(), "one press, one request");
    assert_eq!(request.body, body);
    assert_eq!(request.expected, reading.version);
    assert_eq!(request.source, path);
    assert_eq!(request.chamfer.edge.feature, base);
    assert_eq!(request.chamfer.edge.joint, corner.joint);
    assert_eq!(request.chamfer.distance_mm, 1.0625);
    assert!(!history(e).can_undo(), "Add confirmed nothing");
    assert!(e.take_request().is_none() && e.take_apply_request().is_none());
    // Confirm records the request in the form's own history only.
    let first = typed(e);
    click(&ctx, e, "Confirm draft edge and distance");
    assert!(e.take_add_request().is_none() && e.take_request().is_none());
    assert_eq!(history(e).states.len(), 2);
    let (_, _, other) = corner_at(e, [33., 3.25]);
    click(&ctx, e, &other);
    distance(&ctx, e, "2.5");
    click(&ctx, e, "Confirm draft edge and distance");
    let second = typed(e);
    assert_eq!(history(e).states.len(), 3);
    // Undo/Redo request move the fields, never the model or the window.
    click(&ctx, e, "Undo request");
    assert_eq!(typed(e), first);
    assert_eq!(typed(e).corner, Some(i));
    assert!(e.take_add_request().is_none());
    click(&ctx, e, "Redo request");
    assert_eq!(typed(e), second);
    click(&ctx, e, "Undo request");
    // Add after a request Undo reads the restored fields and keeps the Redo.
    click(&ctx, e, "Add chamfer");
    assert_eq!(
        e.take_add_request()
            .expect("restored fields")
            .chamfer
            .distance_mm,
        1.0625
    );
    assert!(history(e).can_redo(), "Add kept the request Redo");
    // A clean document keeps the copy workflow for the confirmed request.
    click(&ctx, e, "Save chamfer copy…");
    assert_eq!(
        e.take_request().expect("clean copy").chamfer.distance_mm,
        1.0625
    );
    // Dirty: copy disabled with words, Add still adds.
    dirty(&mut s, base);
    tell(&mut c, &s);
    let e = &mut c.sketch.chamfers;
    assert!(painted(&frame(&ctx, e, false), "unsaved changes"));
    click(&ctx, e, "Save chamfer copy…");
    assert!(e.take_request().is_none(), "a dirty copy asked");
    click(&ctx, e, "Add chamfer");
    assert!(
        e.take_add_request().is_some(),
        "Add works on a dirty document"
    );
}

#[test]
fn add_chamfer_busy_guard_proves_idle_request_then_excludes_other_work_and_forms() {
    let (_root, path, reading) = plate();
    let mut s = opened(&path);
    let (sketch, _) = typed_add(&path, &reading, &s, [-4.5, 15.5], "1.0625");
    let mut c = creates::Creates::default();
    c.sketch = sketch;
    let ctx = egui::Context::default();
    settle(&ctx, &mut c.sketch.chamfers);
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
        let available = can_add_chamfer(&c, &loads, &ex, &ed, &s);
        assert!(!available, "busy mode {mode}");
        c.sketch.chamfers.set_add(available);
        click(&ctx, &mut c.sketch.chamfers, "Add chamfer");
        assert!(
            c.sketch.chamfers.take_add_request().is_none(),
            "mode {mode}"
        );
        if let Some(g) = generation {
            s.cancel();
            s.finish_apply(g, Err(CadError::Cancelled));
        }
    }
    // While the worker runs the form is disabled: a press starts nothing.
    let (sketch, _) = typed_add(&path, &reading, &s, [-4.5, 15.5], "1.0625");
    let mut c = creates::Creates::default();
    c.sketch = sketch;
    tell(&mut c, &s);
    let e = &mut c.sketch.chamfers;
    for _ in 0..3 {
        frame(&ctx, e, true);
    }
    let at = find(&frame(&ctx, e, true), "Add chamfer").expect("Add chamfer painted while running");
    press(&ctx, e, at, true);
    assert!(e.take_add_request().is_none(), "a running form asked");
    // Another editor's form, no session, a closed form: never Add.
    let base = reading.chamfer_bodies[0]
        .target
        .as_ref()
        .expect("target")
        .profile;
    let mut c = creates::Creates::default();
    assert!(c.sketch.begin_edit(&path, &reading, base));
    assert!(!ready(&c, &s));
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .chamfers
            .begin(&path, &reading, reading.chamfer_bodies[0].body)
    );
    assert!(!ready(&c, &sessions::Sessions::default()), "no session");
    c.sketch.chamfers.dismiss();
    assert!(!ready(&c, &s), "a closed form");
}

#[test]
fn add_chamfer_dirty_discovery_refusals_keep_draft_status_dismissal_and_closed_classes() {
    let (_root, path, reading) = plate();
    let original = std::fs::read(&path).expect("source");
    let mut s = opened(&path);
    let base = reading.chamfer_bodies[0]
        .target
        .as_ref()
        .expect("target")
        .base_feature;
    dirty(&mut s, base);
    let current = s.export_path().expect("current");
    let reading = ferritecad_jobs::read_extrude_source(&current).expect("reading");
    let choice = &reading.chamfer_bodies[0];
    let label = format!(
        "Chamfer edge of {} — {}…",
        choice.name.as_deref().unwrap_or("Unnamed body"),
        choice.body
    );
    // Opens on a dirty document only when the window says the session is idle;
    // the form is on the accepted snapshot, not the user's file.
    for (session_idle, opens) in [(false, false), (true, true)] {
        let mut c = creates::Creates::default();
        c.sketch.chamfers.set_session(session_idle, false, true);
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
                        .chamfers
                        .choices(ui, false, Some(&current), Some(&reading))
                },
            );
            out.textures_delta.clear();
            out
        };
        draw(&mut c, vec![]);
        let at = find(&draw(&mut c, vec![]), &label).expect("Add entry painted");
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
        assert_eq!(
            c.sketch.chamfers.adding(),
            opens,
            "session_idle={session_idle}"
        );
        if opens {
            assert_eq!(
                c.sketch.chamfers.draft.as_ref().expect("draft").source,
                current,
                "the form reads the accepted snapshot"
            );
        }
    }
    let mut c = creates::Creates::default();
    assert!(c.sketch.chamfers.begin(&current, &reading, choice.body));
    tell(&mut c, &s);
    let ctx = egui::Context::default();
    let e = &mut c.sketch.chamfers;
    settle(&ctx, e);
    // No corner, not a number, below 0.001 mm, beyond the shorter side less
    // 0.01 mm (12.25 − 0.01): the draft stays, nothing is clamped or asked.
    click(&ctx, e, "Add chamfer");
    assert!(e.take_add_request().is_none(), "no corner");
    let (i, _, at) = corner_at(e, [-4.5, 3.25]);
    click(&ctx, e, &at);
    for (text, why) in [
        ("banana", "finite"),
        ("0.0005", "at least 0.001 mm"),
        ("12.2401", "too large"),
    ] {
        distance(&ctx, e, text);
        let out = frame(&ctx, e, false);
        click(&ctx, e, "Add chamfer");
        assert!(e.take_add_request().is_none(), "{text}");
        assert_eq!(typed(e).distance, text);
        assert_eq!(typed(e).corner, Some(i));
        let shown = e
            .draft
            .as_ref()
            .expect("draft")
            .chamfer(&typed(e))
            .expect_err("refused")
            .to_string();
        assert!(shown.contains(why), "{text}: {shown}");
        assert!(painted(&out, &shown), "the reason is shown in the form");
    }
    distance(&ctx, e, "12.24");
    click(&ctx, e, "Add chamfer");
    assert_eq!(
        e.take_add_request()
            .expect("the bound itself")
            .chamfer
            .distance_mm,
        12.24
    );
    // The document's line is drawn inside the open form, where it can be read.
    e.set_outcome("Could not apply the change: this slice chamfers a free plate");
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
        c.sketch.chamfers.add_session(),
        "the window's word survives the draft"
    );
    assert_eq!(std::fs::read(&path).expect("source"), original);

    // A plate with a Chamfer offers no second one.
    let (_rootc, pathc, chamfered) = chamfered(1.5);
    assert!(
        chamfered.chamfer_bodies[0].target.is_none(),
        "a second Chamfer is offered"
    );
    assert!(!creates::Creates::default().sketch.chamfers.begin(
        &pathc,
        &chamfered,
        chamfered.chamfer_bodies[0].body
    ));
    // A constrained plate is not a creation target: the domain's reason, unchanged.
    let (_rootk, pathk, plain) = plate();
    let profile = plain.chamfer_bodies[0]
        .target
        .as_ref()
        .expect("target")
        .profile;
    let mut d = Document::open(&pathk).expect("document");
    let ferritecad_document::ObjectPayload::Sketch(sketch) =
        d.object(profile).expect("object").expect("sketch").payload
    else {
        panic!("Sketch");
    };
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &d,
        profile,
        &ferritecad_document::SketchConstraintEdits {
            remove: vec![],
            add: vec![ferritecad_document::AddSketchConstraint::Line(
                ferritecad_document::AddLineConstraint::Line {
                    curve: sketch.curves[0].id,
                    kind: ferritecad_document::LineConstraintKind::Vertical,
                },
            )],
        },
    )
    .expect("constraint");
    d.write_sketch_constraints(&prepared).expect("write");
    d.close().expect("close");
    let constrained = ferritecad_jobs::read_extrude_source(&pathk).expect("reading");
    let refusal = constrained.chamfer_bodies[0]
        .refusal
        .as_deref()
        .expect("a constrained plate is refused");
    assert!(refusal.contains("free or closure-only plate"), "{refusal}");
    assert!(!creates::Creates::default().sketch.chamfers.begin(
        &pathk,
        &constrained,
        constrained.chamfer_bodies[0].body
    ));
}
