// SPDX-License-Identifier: MIT
//! §31K: the drawing a constraints form was opened with, and the target and view of its
//! canvas, are part of the form: they go with its tab and its picks, through the window's own
//! owners (`present`, `Tabs`, `Forms`). A form that comes back made on another version is held
//! and loses its drawing; it is never shown a drawing of a version it was not made on.

use super::*;
use crate::sketch::accepted::{Curve, Drawing, Target, Unavailable};

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
    drawn(w)
}

fn drawn(w: &Window) -> Vec<Curve> {
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

#[test]
fn the_drawing_the_target_and_the_picks_of_a_form_follow_its_tab() {
    let user = tempfile::tempdir().expect("user");
    let (a_file, b_file) = (user.path().join("a.fcad"), user.path().join("b.fcad"));
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    let in_a = open_constraints(&mut w);
    let a_ids = w.creates.sketch.constraints.draft_state().expect("draft");
    assert_eq!(a_ids, (None, None, None, 0, 0));
    w.creates
        .sketch
        .constraints
        .draft_canvas_mut()
        .expect("canvas")
        .set_target(Target::PairB);

    // Another document in another tab: its form is handed its own scene's drawing, curves
    // of its own UUIDs, and A's form waits with its own.
    w.open(&b_file);
    assert!(!w.creates.sketch.constraints.active(), "A's form is parked");
    let in_b = open_constraints(&mut w);
    assert!(
        in_a.iter().all(|x| in_b.iter().all(|y| x.id != y.id)),
        "two documents, no curve in common"
    );
    assert_eq!(in_b.len(), 4);
    assert_eq!(
        w.creates
            .sketch
            .constraints
            .draft_canvas()
            .expect("draft")
            .1
            .target(),
        Target::Object,
        "B's canvas starts as a new canvas does"
    );

    // Back to A: its drawing, its target, untouched; B's is parked in turn.
    w.round(a);
    assert_eq!(drawn(&w), in_a);
    assert_eq!(
        w.creates
            .sketch
            .constraints
            .draft_canvas()
            .expect("draft")
            .1
            .target(),
        Target::PairB
    );
    assert!(!w.sessions.stale_draft());

    // A pan of the canvas holds the window as a dragged vertex does, and a release ends it.
    assert!(w.can_leave());
    let ctx = egui::Context::default();
    let frame = |w: &mut Window, events: Vec<egui::Event>| {
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
    };
    for _ in 0..3 {
        frame(&mut w, vec![]);
    }
    let from = w
        .creates
        .sketch
        .constraints
        .draft_canvas()
        .and_then(|(_, canvas)| canvas.rect())
        .expect("the canvas")
        .center();
    frame(&mut w, vec![egui::Event::PointerMoved(from)]);
    frame(
        &mut w,
        vec![egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Middle,
            pressed: true,
            modifiers: Default::default(),
        }],
    );
    frame(
        &mut w,
        vec![egui::Event::PointerMoved(from + egui::vec2(9., 4.))],
    );
    assert!(!w.can_leave(), "a held pan holds the window");
    frame(
        &mut w,
        vec![egui::Event::PointerButton {
            pos: from + egui::vec2(9., 4.),
            button: egui::PointerButton::Middle,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(w.can_leave(), "a released pan does not");
    assert!(!w.sessions.dirty(), "a view is not a change");
    println!("\nFCAD_31K_TAB_EXECUTED");
}

#[test]
fn a_form_held_on_another_version_loses_its_drawing_and_keeps_its_picks() {
    let user = tempfile::tempdir().expect("user");
    let (a_file, b_file) = (user.path().join("a.fcad"), user.path().join("b.fcad"));
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    let feature = w.feature();
    let in_a = open_constraints(&mut w);
    // Two picks made through the lists, as a person makes them.
    w.creates.sketch.constraints.pick_for_tests(in_a[1].id);
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
    let (drawing, _) = w
        .creates
        .sketch
        .constraints
        .draft_canvas()
        .expect("the draft is kept");
    assert_eq!(drawing, &Drawing::Unavailable(Unavailable::OtherVersion));
    assert_eq!(
        w.creates.sketch.constraints.draft_state().expect("draft").0,
        Some(in_a[1].id),
        "the picks of a held form are kept"
    );
    // Cancel ends it, and a form opened afresh on the version now shown is handed that
    // version's own drawing.
    w.creates.sketch.constraints.dismiss();
    w.sessions.release_stale_draft();
    let again = open_constraints(&mut w);
    assert_eq!(again.len(), 4);
    println!("\nFCAD_31K_TAB_STALE_EXECUTED");
}

#[test]
fn a_form_given_back_by_a_cancelled_close_or_an_aborted_quit_keeps_its_own_drawing() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    plate(&a_file, 12.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let in_a = open_constraints(&mut w);
    w.creates.sketch.constraints.pick_for_tests(in_a[2].id);
    w.creates
        .sketch
        .constraints
        .draft_canvas_mut()
        .expect("canvas")
        .set_target(Target::Endpoint);
    let kept = |w: &Window| {
        let (drawing, canvas) = w
            .creates
            .sketch
            .constraints
            .draft_canvas()
            .expect("a draft");
        (
            drawing.clone(),
            canvas.target(),
            w.creates.sketch.constraints.draft_state().expect("draft").0,
        )
    };
    let before = kept(&w);
    assert_eq!(before.2, Some(in_a[2].id));

    // Close asked, confirmed and answered Cancel: the original form comes back.
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    assert!(
        !w.creates.sketch.constraints.active(),
        "the form is set aside"
    );
    assert!(w.back_from_form_close(id));
    assert_eq!(kept(&w), before, "a cancelled Close");

    // Quit asked and aborted: the same.
    assert!(w.begin_quit());
    w.park_quit_form();
    assert!(!w.creates.sketch.constraints.active());
    w.stop_quit();
    assert_eq!(kept(&w), before, "an aborted Quit");
    assert!(!w.sessions.stale_draft());
    println!("\nFCAD_31K_TAB_GIVEN_BACK_EXECUTED");
}
