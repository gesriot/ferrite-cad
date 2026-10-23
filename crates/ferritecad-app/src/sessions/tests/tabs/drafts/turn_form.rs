// SPDX-License-Identifier: MIT
//! §31M: the saved directed angles of a constraints form go with the form: with its tab, its
//! selection (the rule of an angle, never a curve), its typed text and its prepared request,
//! through the window's own owners (`present`, `Tabs`, `Forms`). A form that comes back made on
//! another version is held, loses its drawing and so its labels, and keeps its rows, its selection
//! and its text.

use super::dimension_form::{
    centre, click_at, frame, open_constraints, painted, press, settle, type_after,
};
use super::*;
use crate::sketch::accepted::Target;
use ferritecad_document::{DirectedAngle, DirectedLine, LineEndpoint};
use ferritecad_types::StableEntityId;

/// The form of the shown tab with two saved angles of its Lines (R0: Line 1 start>end to Line 2
/// start>end, -70°; R1: Line 2 end>start to Line 3 start>end, 110°), and the curves and rules they
/// are on. Synthetic: the catalogue of the form is amended, as in `dimension_form`.
fn open_angled(w: &mut Window) -> (Vec<StableEntityId>, Vec<StableEntityId>) {
    let curves = open_constraints(w);
    let ids: Vec<_> = curves.iter().map(|c| c.id).collect();
    let v = |i: usize, from: LineEndpoint| {
        DirectedLine::new(ids[i], from, from.other()).expect("two different ends")
    };
    let angle = |degrees| DirectedAngle::from_degrees(degrees).expect("in range");
    let rules = w.creates.sketch.constraints.persist_angles_for_tests(&[
        (
            v(0, LineEndpoint::Start),
            v(1, LineEndpoint::Start),
            angle(-70.),
        ),
        (
            v(1, LineEndpoint::End),
            v(2, LineEndpoint::Start),
            angle(110.),
        ),
    ]);
    (ids, rules)
}

type Held = (
    Vec<(StableEntityId, DirectedLine, DirectedLine, f64)>,
    Option<StableEntityId>,
    Option<StableEntityId>,
    Target,
    String,
    ferritecad_document::SketchConstraintEdits,
);

/// What a form keeps for its tab: its angles, the selected angle, the selected Line (none while an
/// angle is), the target of its canvas, the text typed for a replacement and the request prepared.
fn turn_state(w: &Window) -> Held {
    let constraints = &w.creates.sketch.constraints;
    let (turns, selected, typed) = constraints.draft_turns().expect("a draft");
    (
        turns
            .iter()
            .map(|t| (t.id, t.a, t.b, t.angle.degrees()))
            .collect(),
        selected,
        constraints.draft_state().expect("draft").0,
        constraints.draft_canvas().expect("canvas").1.target(),
        typed,
        constraints.draft_edits().expect("draft"),
    )
}

#[test]
fn the_angles_the_selection_the_text_and_the_prepared_replacement_of_a_form_follow_its_tab() {
    let user = tempfile::tempdir().expect("user");
    let (a_file, b_file) = (user.path().join("a.fcad"), user.path().join("b.fcad"));
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    let ctx = egui::Context::default();
    w.open(&a_file);
    let a = w.sessions.tab();
    let (a_curves, a_rules) = open_angled(&mut w);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    // In A: the Dimension target, the label of -70°, a replacement by 110°.
    press(&mut w, &ctx, "Dimension");
    let at = centre(&frame(&mut w, &ctx, vec![]), "A>B -70°");
    click_at(&mut w, &ctx, at);
    settle(&mut w, &ctx);
    type_after(&mut w, &ctx, "New angle", "110");
    press(&mut w, &ctx, "Replace dimension");
    let in_a = turn_state(&w);
    assert_eq!(
        in_a.0.iter().map(|t| t.0).collect::<Vec<_>>(),
        a_rules,
        "the form reads its own rules by their UUIDs"
    );
    assert_eq!(
        (in_a.1, in_a.2, in_a.3, in_a.4.as_str()),
        (Some(a_rules[0]), None, Target::Dimension, "110")
    );
    assert_eq!(
        in_a.5.remove,
        vec![a_rules[0]],
        "{:?}",
        w.creates.sketch.constraints.draft_inputs()
    );
    assert_eq!(in_a.5.add.len(), 1);
    assert!(painted(&frame(&mut w, &ctx, vec![]), "A>B -70 -> 110°"));

    // Another document in another tab: its form has its own curves, rules, selection, text and
    // request, and not a trace of A's.
    w.open(&b_file);
    assert!(!w.creates.sketch.constraints.active(), "A's form is parked");
    let (b_curves, b_rules) = open_angled(&mut w);
    assert!(
        a_curves.iter().all(|x| !b_curves.contains(x))
            && a_rules.iter().all(|x| !b_rules.contains(x)),
        "two documents, no curve and no rule in common"
    );
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    let out = frame(&mut w, &ctx, vec![]);
    assert!(
        painted(&out, "A>B -70°") && painted(&out, "A>B 110°") && !painted(&out, "A>B -70 -> 110°")
    );
    let in_b = turn_state(&w);
    assert_eq!(in_b.0.iter().map(|t| t.0).collect::<Vec<_>>(), b_rules);
    assert_eq!(
        (in_b.1, in_b.2, in_b.3, in_b.4.as_str(), in_b.5),
        (None, None, Target::Object, "", Default::default())
    );
    // B's own replacement, of the other angle.
    press(&mut w, &ctx, "Dimension");
    let at = centre(&frame(&mut w, &ctx, vec![]), "A>B 110°");
    click_at(&mut w, &ctx, at);
    settle(&mut w, &ctx);
    type_after(&mut w, &ctx, "New angle", "70");
    press(&mut w, &ctx, "Replace dimension");
    let in_b = turn_state(&w);
    assert_eq!(
        (in_b.1, in_b.5.remove),
        (Some(b_rules[1]), vec![b_rules[1]])
    );

    // Back to A: its angles, selection, text and request exactly, and the label that says so.
    w.round(a);
    assert_eq!(turn_state(&w), in_a);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    let out = frame(&mut w, &ctx, vec![]);
    assert!(painted(&out, "A>B -70 -> 110°") && painted(&out, "A>B 110°"));
    assert!(!painted(&out, "A>B 110 -> 70°"));
    assert!(!w.sessions.dirty(), "a prepared replacement is no change");
}

#[test]
fn an_angle_form_held_on_another_version_loses_its_labels_and_keeps_its_rows_selection_and_text() {
    let user = tempfile::tempdir().expect("user");
    let (a_file, b_file) = (user.path().join("a.fcad"), user.path().join("b.fcad"));
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    let ctx = egui::Context::default();
    w.open(&a_file);
    let a = w.sessions.tab();
    let feature = w.feature();
    let (_curves, rules) = open_angled(&mut w);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    press(&mut w, &ctx, "Dimension");
    let at = centre(&frame(&mut w, &ctx, vec![]), "A>B 110°");
    click_at(&mut w, &ctx, at);
    settle(&mut w, &ctx);
    type_after(&mut w, &ctx, "New angle", "33");
    w.open(&b_file);

    // Behind the window's back A's session accepts another version.
    let hidden = w.tabs.hidden_sessions_mut(a).expect("A");
    let (tx, rx) = mpsc::channel();
    let generation = hidden
        .begin_apply(|ticket, _, _| {
            std::thread::spawn(move || {
                tx.send({
                    let ticket = ticket;
                    let expected = ticket.source().version();
                    ticket.edit(
                        vec![
                            ferritecad_document::ExpressionChange::constant(
                                feature,
                                ferritecad_document::ExpressionSlot::ExtrudeDistance,
                                20.0,
                            )
                            .expect("test value"),
                        ],
                        expected,
                        &mut MockKernel::new(),
                        |_, _| {
                            Err(ferritecad_types::CadError::unsupported(
                                "no STEP imports in this model",
                            ))
                        },
                        &OperationContext::default(),
                    )
                })
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
    // No drawing, no labels and no Dimension target; the rows, the selection and the text are the
    // form's own, and the replacement can still be prepared (Apply stays refused).
    let out = frame(&mut w, &ctx, vec![]);
    assert!(
        !painted(&out, "A>B -70°") && !painted(&out, "A>B 110°") && !painted(&out, "Dimension")
    );
    let held = turn_state(&w);
    assert_eq!(held.0.iter().map(|t| t.0).collect::<Vec<_>>(), rules);
    assert_eq!(
        (held.1, held.2, held.4.as_str()),
        (Some(rules[1]), None, "33")
    );
    press(&mut w, &ctx, "Replace dimension");
    let prepared = w
        .creates
        .sketch
        .constraints
        .draft_edits()
        .expect("the held form's request");
    assert_eq!(prepared.remove, vec![rules[1]]);
}

#[test]
fn an_angle_form_given_back_by_a_cancelled_close_or_an_aborted_quit_keeps_all_of_it() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    plate(&a_file, 12.0);
    let mut w = Window::new(Drawn::Mock, None);
    let ctx = egui::Context::default();
    w.open(&a_file);
    let (_curves, rules) = open_angled(&mut w);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    press(&mut w, &ctx, "Dimension");
    let at = centre(&frame(&mut w, &ctx, vec![]), "A>B -70°");
    click_at(&mut w, &ctx, at);
    settle(&mut w, &ctx);
    type_after(&mut w, &ctx, "New angle", "110");
    press(&mut w, &ctx, "Replace dimension");
    type_after(&mut w, &ctx, "New angle", "58.25");
    let before = turn_state(&w);
    assert_eq!((before.1, before.2), (Some(rules[0]), None));
    assert_eq!(before.5.remove, vec![rules[0]]);

    // Close asked, confirmed and answered Cancel: the original form comes back.
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    assert!(
        !w.creates.sketch.constraints.active(),
        "the form is set aside"
    );
    assert!(w.back_from_form_close(id));
    assert_eq!(turn_state(&w), before, "a cancelled Close");
    for _ in 0..4 {
        frame(&mut w, &ctx, vec![]);
    }
    assert!(painted(&frame(&mut w, &ctx, vec![]), "A>B -70 -> 110°"));

    // Quit asked and aborted: the same.
    assert!(w.begin_quit());
    w.park_quit_form();
    assert!(!w.creates.sketch.constraints.active());
    w.stop_quit();
    assert_eq!(turn_state(&w), before, "an aborted Quit");
    for _ in 0..4 {
        frame(&mut w, &ctx, vec![]);
    }
    assert!(painted(&frame(&mut w, &ctx, vec![]), "A>B -70 -> 110°"));
    assert!(!w.sessions.stale_draft() && !w.sessions.dirty());
}
