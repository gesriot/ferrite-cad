// SPDX-License-Identifier: MIT
//! §30J: the Fillet Add form's document Add, through real egui widgets and the
//! same predicate (`can_add_fillet`) the window handler uses. Kernel-free fixtures.
use super::session_apply::{fixture, selected};
use super::*;
use crate::cuts::tests::add_session::{dirty, opened};
use crate::{Loads, can_add_fillet, can_apply_fillet_radius, creates, edits, exports, sessions};

fn ready(c: &creates::Creates, s: &sessions::Sessions) -> bool {
    can_add_fillet(
        c,
        &Loads::default(),
        &exports::Exports::default(),
        &edits::Edits::default(),
        s,
    )
}
fn tell(c: &mut creates::Creates, s: &sessions::Sessions) {
    let radius = can_apply_fillet_radius(
        c,
        &Loads::default(),
        &exports::Exports::default(),
        &edits::Edits::default(),
        s,
    );
    c.sketch.fillets.set_session(true, radius, s.dirty());
    c.sketch.fillets.set_add(ready(c, s));
}
fn settle(ctx: &egui::Context, e: &mut Editor) {
    for _ in 0..3 {
        frame(ctx, e, false);
    }
}
fn typed(e: &Editor) -> Typed {
    e.draft.as_ref().expect("draft").typed.clone()
}
/// The corner of the open draft stored at `at`, and the label its radio shows.
fn corner_at(e: &Editor, at: [f64; 2]) -> (usize, FilletCorner, String) {
    let draft = e.draft.as_ref().expect("draft");
    let target = draft.choice.target.as_ref().expect("target");
    let i = target
        .corners
        .iter()
        .position(|c| c.corner_mm == at)
        .unwrap_or_else(|| panic!("no free corner at {at:?}"));
    let corner = target.corners[i];
    (i, corner, describe_candidate(&corner, target))
}

/// The Add form's own widgets, as the window leaves them: a corner chosen by its
/// stored position, a radius typed and **Add fillet** pressed without confirming
/// the draft. Shared by the native gates.
pub(crate) fn typed_add(
    path: &Path,
    reading: &ExtrudeEditSource,
    s: &sessions::Sessions,
    at: [f64; 2],
    radius_text: &str,
) -> (crate::sketch::Editor, EdgeFilletRequest) {
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .fillets
            .begin(path, reading, reading.fillet_bodies[0].body)
    );
    assert!(ready(&c, s), "own Add form must allow its Add in idle");
    tell(&mut c, s);
    let ctx = egui::Context::default();
    let e = &mut c.sketch.fillets;
    settle(&ctx, e);
    let (_, corner, label) = corner_at(e, at);
    click(&ctx, e, &label);
    radius(&ctx, e, radius_text);
    click(&ctx, e, "Add fillet");
    let request = e.take_add_request().expect("valid widget Add request");
    assert_eq!(request.fillet.edge.joint, corner.joint);
    assert!(e.take_request().is_none(), "Add is not the copy");
    (c.sketch, request)
}

#[test]
fn add_fillet_reads_current_selection_confirm_is_draft_only_and_copy_is_clean_only() {
    let (_root, path, reading) = fixture(1);
    let mut s = opened(&path);
    let body = reading.fillet_bodies[0].body;
    let base = selected(&reading, 0).base_feature;
    let mut c = creates::Creates::default();
    assert!(!ready(&c, &s), "nothing to add before the form is open");
    assert!(c.sketch.fillets.begin(&path, &reading, body));
    assert!(c.busy(), "New and Open still wait for this form");
    assert!(ready(&c, &s), "the Add form must not block its own Add");
    assert!(
        !can_apply_fillet_radius(
            &c,
            &Loads::default(),
            &exports::Exports::default(),
            &edits::Edits::default(),
            &s
        ),
        "the radius edit's Apply never serves the Add form"
    );
    let ctx = egui::Context::default();
    // The window says "not ready": the button is there and asks for nothing.
    c.sketch.fillets.set_add(false);
    let e = &mut c.sketch.fillets;
    settle(&ctx, e);
    let out = frame(&ctx, e, false);
    for label in [
        "Add fillet",
        "Confirm draft edge and radius",
        "Fillet one vertical edge",
        "This document changes with Add fillet",
    ] {
        assert!(painted(&out, label), "{label}");
    }
    assert!(
        !painted(&out, "Apply radius"),
        "no radius Apply on the Add form"
    );
    assert!(!painted(&out, "new copy"), "the window title is not a copy");
    let (i, corner, label) = corner_at(e, [-4.5, 15.5]);
    click(&ctx, e, &label);
    radius(&ctx, e, "3.06250");
    click(&ctx, e, "Add fillet");
    assert!(e.take_add_request().is_none(), "disabled asked");
    // The window says what it computes: one press, one request, nothing confirmed.
    tell(&mut c, &s);
    let e = &mut c.sketch.fillets;
    click(&ctx, e, "Add fillet");
    let request = e.take_add_request().expect("unconfirmed current selection");
    assert!(e.take_add_request().is_none(), "one press, one request");
    assert_eq!(request.body, body);
    assert_eq!(request.expected, reading.version);
    assert_eq!(request.source, path);
    assert_eq!(request.fillet.edge.feature, base);
    assert_eq!(request.fillet.edge.joint, corner.joint);
    assert_eq!(request.fillet.radius_mm, 3.0625);
    assert!(
        e.draft.as_ref().expect("draft").applied.is_none(),
        "Add did not confirm the draft"
    );
    assert!(e.take_request().is_none() && e.take_apply_request().is_none());
    assert_eq!(typed(e).corner, Some(i));
    assert_eq!(typed(e).radius, "3.06250");
    // Confirm records the draft for the copy workflow only: it asks the window
    // for nothing, and only then is the copy offered.
    assert!(!painted(&frame(&ctx, e, false), "Save fillet copy…"));
    click(&ctx, e, "Confirm draft edge and radius");
    assert!(e.take_add_request().is_none() && e.take_request().is_none());
    assert_eq!(
        e.draft.as_ref().expect("draft").applied.as_ref(),
        Some(&typed(e))
    );
    // A clean document keeps the copy workflow.
    click(&ctx, e, "Save fillet copy…");
    assert_eq!(
        e.take_request().expect("clean copy").fillet.radius_mm,
        3.0625
    );
    // Dirty: copy disabled with words, Add still adds.
    dirty(&mut s, base);
    tell(&mut c, &s);
    let e = &mut c.sketch.fillets;
    assert!(painted(&frame(&ctx, e, false), "unsaved changes"));
    click(&ctx, e, "Save fillet copy…");
    assert!(e.take_request().is_none(), "a dirty copy asked");
    click(&ctx, e, "Add fillet");
    assert!(
        e.take_add_request().is_some(),
        "Add works on a dirty document"
    );
}

#[test]
fn add_fillet_busy_guard_proves_idle_request_then_excludes_other_work_and_forms() {
    let (_root, path, reading) = fixture(1);
    let mut s = opened(&path);
    let (sketch, _) = typed_add(&path, &reading, &s, [-4.5, 15.5], "3.0625");
    let mut c = creates::Creates::default();
    c.sketch = sketch;
    let ctx = egui::Context::default();
    settle(&ctx, &mut c.sketch.fillets);
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
        let available = can_add_fillet(&c, &loads, &ex, &ed, &s);
        assert!(!available, "busy mode {mode}");
        c.sketch.fillets.set_add(available);
        click(&ctx, &mut c.sketch.fillets, "Add fillet");
        assert!(c.sketch.fillets.take_add_request().is_none(), "mode {mode}");
        if let Some(g) = generation {
            s.cancel();
            s.finish_apply(g, Err(CadError::Cancelled));
        }
    }
    // While the worker runs the form is disabled: a press starts nothing.
    let (sketch, _) = typed_add(&path, &reading, &s, [-4.5, 15.5], "3.0625");
    let mut c = creates::Creates::default();
    c.sketch = sketch;
    tell(&mut c, &s);
    let e = &mut c.sketch.fillets;
    for _ in 0..3 {
        frame(&ctx, e, true);
    }
    let at = find(&frame(&ctx, e, true), "Add fillet").expect("Add fillet painted while running");
    press(&ctx, e, at, true);
    assert!(e.take_add_request().is_none(), "a running form asked");
    // The radius form, another editor's form, no session, a closed form: never Add.
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .fillets
            .begin_radius(&path, &reading, selected(&reading, 0).feature)
    );
    assert!(!ready(&c, &s), "the radius form is not the Add form");
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .begin_edit(&path, &reading, selected(&reading, 0).profile)
    );
    assert!(!ready(&c, &s));
    let mut c = creates::Creates::default();
    assert!(
        c.sketch
            .fillets
            .begin(&path, &reading, reading.fillet_bodies[0].body)
    );
    assert!(!ready(&c, &sessions::Sessions::default()), "no session");
    c.sketch.fillets.dismiss();
    assert!(!ready(&c, &s), "a closed form");
}

#[test]
fn add_fillet_dirty_discovery_refusals_keep_draft_status_dismissal_and_full_plate() {
    let (_root, path, reading) = fixture(1);
    let original = std::fs::read(&path).expect("source");
    let mut s = opened(&path);
    dirty(&mut s, selected(&reading, 0).base_feature);
    let current = s.export_path().expect("current");
    let reading = ferritecad_jobs::read_extrude_source(&current).expect("reading");
    let choice = &reading.fillet_bodies[0];
    let label = format!(
        "Fillet edge of {} — {}…",
        choice.name.as_deref().unwrap_or("Unnamed body"),
        choice.body
    );
    // Opens on a dirty document only when the window says the session is idle;
    // the form is on the accepted snapshot, not the user's file.
    for (session_idle, opens) in [(false, false), (true, true)] {
        let mut c = creates::Creates::default();
        c.sketch.fillets.set_session(session_idle, false, true);
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
                        .fillets
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
            c.sketch.fillets.adding(),
            opens,
            "session_idle={session_idle}"
        );
        if opens {
            assert_eq!(
                c.sketch.fillets.draft.as_ref().expect("draft").source,
                current,
                "the form reads the accepted snapshot"
            );
        }
    }
    let mut c = creates::Creates::default();
    assert!(c.sketch.fillets.begin(&current, &reading, choice.body));
    tell(&mut c, &s);
    let ctx = egui::Context::default();
    let e = &mut c.sketch.fillets;
    settle(&ctx, e);
    // The rounded corner is not offered again.
    let rounded = selected(&reading, 0).corner.joint;
    assert!(
        e.draft
            .as_ref()
            .expect("draft")
            .corners()
            .iter()
            .all(|c| c.joint != rounded),
        "a rounded corner is offered"
    );
    // No corner, not a number, below the minimum, above half the shorter side:
    // the draft stays and nothing is asked.
    click(&ctx, e, "Add fillet");
    assert!(e.take_add_request().is_none(), "no corner");
    let (i, _, beside) = corner_at(e, [-4.5, 3.25]);
    click(&ctx, e, &beside);
    for (text, why) in [
        ("banana", "finite"),
        ("0.005", "at least"),
        ("6.2", "too large"),
    ] {
        radius(&ctx, e, text);
        let out = frame(&ctx, e, false);
        click(&ctx, e, "Add fillet");
        assert!(e.take_add_request().is_none(), "{text}");
        assert_eq!(typed(e).radius, text);
        assert_eq!(typed(e).corner, Some(i));
        let shown = e
            .draft
            .as_ref()
            .expect("draft")
            .fillet(&typed(e))
            .expect_err("refused")
            .to_string();
        assert!(shown.contains(why), "{text}: {shown}");
        assert!(painted(&out, &shown), "the reason is shown in the form");
    }
    // The document's line is drawn inside the open form, where it can be read.
    e.set_outcome("Could not apply the change: the corner is already rounded");
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
        c.sketch.fillets.add_session(),
        "the window's word survives the draft"
    );
    assert_eq!(std::fs::read(&path).expect("source"), original);

    // With four Fillets the plate offers no Add.
    let (_root4, path4, full) = fixture(4);
    assert!(
        full.fillet_bodies[0].refusal.is_some() || full.fillet_bodies[0].target.is_none(),
        "a fifth Fillet is offered"
    );
    assert!(!creates::Creates::default().sketch.fillets.begin(
        &path4,
        &full,
        full.fillet_bodies[0].body
    ));
}
