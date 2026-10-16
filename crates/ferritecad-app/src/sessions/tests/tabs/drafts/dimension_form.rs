// SPDX-License-Identifier: MIT
//! §31L: the saved dimensions of a constraints form go with the form: with its tab, its picks,
//! its typed text and its prepared request, through the window's own owners (`present`,
//! `Tabs`, `Forms`). A form that comes back made on another version is held, loses its
//! drawing and so its labels, and keeps its rows and its text.

use super::*;
use crate::sketch::accepted::{Curve, Drawing, Target};

/// The constraints form on the shown tab, opened as the window opens it: from the facts and
/// the drawing of the scene it is showing.
fn open_constraints(w: &mut Window) -> Vec<Curve> {
    let current = w.sessions.export_path().expect("shown");
    let facts = w
        .scene
        .edit_source
        .clone()
        .expect("the scene carries its facts");
    let sketch = facts.constraint_sketches[0].sketch;
    assert!(
        w.creates
            .sketch
            .constraints
            .begin_with(&current, &facts, &w.scene.sketches, sketch)
    );
    match w
        .creates
        .sketch
        .constraints
        .draft_canvas()
        .expect("a draft")
        .0
    {
        Drawing::Solved(curves) => curves.clone(),
        other => panic!("no drawing: {other:?}"),
    }
}

fn frame(w: &mut Window, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(988., 768.),
            )),
            events,
            ..Default::default()
        },
        |ui| w.creates.sketch.draw(ui, true, false, ""),
    );
    out.textures_delta.clear();
    out
}

fn centre(out: &egui::FullOutput, words: &str) -> egui::Pos2 {
    out.shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == words => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("not painted: {words}"))
}

fn painted(out: &egui::FullOutput, words: &str) -> bool {
    out.shapes
        .iter()
        .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == words))
}

fn click_at(w: &mut Window, ctx: &egui::Context, at: egui::Pos2) {
    frame(w, ctx, vec![egui::Event::PointerMoved(at)]);
    for pressed in [true, false] {
        frame(
            w,
            ctx,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }],
        );
    }
}

fn press(w: &mut Window, ctx: &egui::Context, words: &str) {
    let at = centre(&frame(w, ctx, vec![]), words);
    click_at(w, ctx, at);
}

fn settle(w: &mut Window, ctx: &egui::Context) {
    for _ in 0..4 {
        frame(w, ctx, vec![]);
    }
}

/// Text typed into the box that stands to the right of the painted `before`.
fn type_after(w: &mut Window, ctx: &egui::Context, before: &str, text: &str) {
    let out = frame(w, ctx, vec![]);
    let at = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == before => {
                let r = t.visual_bounding_rect();
                Some(egui::pos2(r.right() + 30., r.center().y))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("not painted: {before}"));
    click_at(w, ctx, at);
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    frame(
        w,
        ctx,
        vec![
            key(egui::Key::A, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, Default::default()),
            egui::Event::Text(text.into()),
        ],
    );
}

/// The form of the shown tab with a Distance of 60 mm on its first Line and 30 mm on its
/// second, and the curves and rules they are on.
fn open_dimensioned(
    w: &mut Window,
) -> (
    Vec<ferritecad_types::StableEntityId>,
    Vec<ferritecad_types::StableEntityId>,
) {
    let curves = open_constraints(w);
    let ids: Vec<_> = curves.iter().map(|c| c.id).collect();
    let rules = w
        .creates
        .sketch
        .constraints
        .persist_lengths_for_tests(&[(ids[0], 60.), (ids[1], 30.)]);
    (ids, rules)
}

/// What a form keeps for its tab: its dimensions, the selected Line, the target of its canvas,
/// the text typed for a replacement and the request prepared.
fn dimension_state(
    w: &Window,
) -> (
    Vec<(
        ferritecad_types::StableEntityId,
        ferritecad_types::StableEntityId,
    )>,
    Option<ferritecad_types::StableEntityId>,
    Target,
    String,
    ferritecad_document::SketchConstraintEdits,
) {
    let constraints = &w.creates.sketch.constraints;
    let (dimensions, typed) = constraints.draft_dimensions().expect("a draft");
    (
        dimensions.iter().map(|d| (d.id, d.curve)).collect(),
        constraints.draft_state().expect("draft").0,
        constraints.draft_canvas().expect("canvas").1.target(),
        typed,
        constraints.draft_edits().expect("draft"),
    )
}

#[test]
fn the_dimensions_the_text_and_the_prepared_replacement_of_a_form_follow_its_tab() {
    let user = tempfile::tempdir().expect("user");
    let (a_file, b_file) = (user.path().join("a.fcad"), user.path().join("b.fcad"));
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    let ctx = egui::Context::default();
    w.open(&a_file);
    let a = w.sessions.tab();
    let (a_curves, a_rules) = open_dimensioned(&mut w);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    // In A: the Dimension target, the label of 60 mm, a replacement by 70 mm.
    press(&mut w, &ctx, "Dimension");
    let at = centre(&frame(&mut w, &ctx, vec![]), "60 mm");
    click_at(&mut w, &ctx, at);
    settle(&mut w, &ctx);
    type_after(&mut w, &ctx, "New length", "70");
    press(&mut w, &ctx, "Replace dimension");
    let in_a = dimension_state(&w);
    assert_eq!(
        in_a.0,
        vec![(a_rules[0], a_curves[0]), (a_rules[1], a_curves[1])]
    );
    assert_eq!(in_a.1, Some(a_curves[0]));
    assert_eq!(in_a.2, Target::Dimension);
    assert_eq!(in_a.3, "70");
    assert_eq!(in_a.4.remove, vec![a_rules[0]]);
    assert_eq!(in_a.4.add.len(), 1);
    assert!(painted(&frame(&mut w, &ctx, vec![]), "60 → 70 mm"));

    // Another document in another tab: its form has its own curves, rules, text and request,
    // and not a trace of A's.
    w.open(&b_file);
    assert!(!w.creates.sketch.constraints.active(), "A's form is parked");
    let (b_curves, b_rules) = open_dimensioned(&mut w);
    assert!(
        a_curves.iter().all(|x| !b_curves.contains(x))
            && a_rules.iter().all(|x| !b_rules.contains(x)),
        "two documents, no curve and no rule in common"
    );
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    let out = frame(&mut w, &ctx, vec![]);
    assert!(painted(&out, "60 mm") && painted(&out, "30 mm") && !painted(&out, "60 → 70 mm"));
    let in_b = dimension_state(&w);
    assert_eq!(
        in_b.0,
        vec![(b_rules[0], b_curves[0]), (b_rules[1], b_curves[1])]
    );
    assert_eq!(
        (in_b.1, in_b.2, in_b.3.as_str(), in_b.4),
        (None, Target::Object, "", Default::default())
    );
    // B's own replacement, of the other dimension.
    press(&mut w, &ctx, "Dimension");
    let at = centre(&frame(&mut w, &ctx, vec![]), "30 mm");
    click_at(&mut w, &ctx, at);
    settle(&mut w, &ctx);
    type_after(&mut w, &ctx, "New length", "33");
    press(&mut w, &ctx, "Replace dimension");
    let in_b = dimension_state(&w);
    assert_eq!(in_b.4.remove, vec![b_rules[1]]);

    // Back to A: its dimensions, text and request exactly, and the label that says so.
    w.round(a);
    assert_eq!(dimension_state(&w), in_a);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    let out = frame(&mut w, &ctx, vec![]);
    assert!(painted(&out, "60 → 70 mm") && painted(&out, "30 mm"));
    assert!(!painted(&out, "30 → 33 mm"));
    assert!(!w.sessions.dirty(), "a prepared replacement is no change");
    println!("\nFCAD_31L_TAB_EXECUTED");
}

#[test]
fn a_dimension_form_held_on_another_version_loses_its_labels_and_keeps_its_rows_and_text() {
    let user = tempfile::tempdir().expect("user");
    let (a_file, b_file) = (user.path().join("a.fcad"), user.path().join("b.fcad"));
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    let ctx = egui::Context::default();
    w.open(&a_file);
    let a = w.sessions.tab();
    let feature = w.feature();
    let (curves, rules) = open_dimensioned(&mut w);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    press(&mut w, &ctx, "Dimension");
    let at = centre(&frame(&mut w, &ctx, vec![]), "30 mm");
    click_at(&mut w, &ctx, at);
    settle(&mut w, &ctx);
    type_after(&mut w, &ctx, "New length", "33");
    w.open(&b_file);

    // Behind the window's back A's session accepts another version.
    let hidden = w.tabs.hidden_sessions_mut(a).expect("A");
    let (tx, rx) = mpsc::channel();
    let generation = hidden
        .begin_apply(|ticket, _, _| {
            std::thread::spawn(move || {
                tx.send(ticket.edit_extrude_height(
                    feature,
                    20.0,
                    &mut MockKernel::new(),
                    &OperationContext::default(),
                ))
                .expect("edit");
            })
        })
        .expect("started");
    assert!(matches!(
        hidden.finish_apply(generation, wait(&rx)),
        Edited::Show(_)
    ));
    hidden.commit_staged().expect("accepted");
    assert!(hidden.finish_scene(generation, Ok(())));

    w.round(a);
    assert!(w.sessions.stale_draft(), "the form is held");
    for _ in 0..4 {
        frame(&mut w, &ctx, vec![]);
    }
    // No drawing, no labels and no Dimension target; the rows, the selection and the text are
    // the form's own, and the replacement can still be prepared (Apply stays refused).
    let out = frame(&mut w, &ctx, vec![]);
    assert!(!painted(&out, "60 mm") && !painted(&out, "30 mm") && !painted(&out, "Dimension"));
    let held = dimension_state(&w);
    assert_eq!(held.0, vec![(rules[0], curves[0]), (rules[1], curves[1])]);
    assert_eq!((held.1, held.3.as_str()), (Some(curves[1]), "33"));
    press(&mut w, &ctx, "Replace dimension");
    let prepared = w
        .creates
        .sketch
        .constraints
        .draft_edits()
        .expect("the held form's request");
    assert_eq!(prepared.remove, vec![rules[1]]);
    println!("\nFCAD_31L_TAB_STALE_EXECUTED");
}

#[test]
fn a_dimension_form_given_back_by_a_cancelled_close_or_an_aborted_quit_keeps_all_of_it() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    plate(&a_file, 12.0);
    let mut w = Window::new(Drawn::Mock, None);
    let ctx = egui::Context::default();
    w.open(&a_file);
    let (curves, rules) = open_dimensioned(&mut w);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    press(&mut w, &ctx, "Dimension");
    let at = centre(&frame(&mut w, &ctx, vec![]), "60 mm");
    click_at(&mut w, &ctx, at);
    settle(&mut w, &ctx);
    type_after(&mut w, &ctx, "New length", "70");
    press(&mut w, &ctx, "Replace dimension");
    type_after(&mut w, &ctx, "New length", "58.25");
    let before = dimension_state(&w);
    assert_eq!(before.1, Some(curves[0]));
    assert_eq!(before.4.remove, vec![rules[0]]);

    // Close asked, confirmed and answered Cancel: the original form comes back.
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    assert!(
        !w.creates.sketch.constraints.active(),
        "the form is set aside"
    );
    assert!(w.back_from_form_close(id));
    assert_eq!(dimension_state(&w), before, "a cancelled Close");
    for _ in 0..4 {
        frame(&mut w, &ctx, vec![]);
    }
    assert!(painted(&frame(&mut w, &ctx, vec![]), "60 → 70 mm"));

    // Quit asked and aborted: the same.
    assert!(w.begin_quit());
    w.park_quit_form();
    assert!(!w.creates.sketch.constraints.active());
    w.stop_quit();
    assert_eq!(dimension_state(&w), before, "an aborted Quit");
    for _ in 0..4 {
        frame(&mut w, &ctx, vec![]);
    }
    assert!(painted(&frame(&mut w, &ctx, vec![]), "60 → 70 mm"));
    assert!(!w.sessions.stale_draft() && !w.sessions.dirty());
    println!("\nFCAD_31L_TAB_GIVEN_BACK_EXECUTED");
}
