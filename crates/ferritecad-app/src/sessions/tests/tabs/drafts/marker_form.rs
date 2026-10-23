// SPDX-License-Identifier: MIT
//! §31N: the saved rule markers of a constraints form go with the form: with its tab, its
//! selection (the rule, never a curve or an angle), its typed text and its prepared request, through
//! the window's own owners (`present`, `Tabs`, `Forms`). A form that comes back made on another
//! version is held, loses its drawing and so its markers, and keeps its rows, its selection and its
//! prepared removal. Two copies of one document hold the same curve and rule UUIDs and still keep
//! separate forms.

use super::dimension_form::{centre, click_at, frame, open_constraints, painted, press, settle};
use super::*;
use crate::sketch::accepted::Target;
use crate::sketch::marker::Kind;
use ferritecad_document::{
    SketchConstraint, SketchConstraintRule, SketchPointRef, SketchPointSelector as At,
    SketchSegmentRef,
};
use ferritecad_types::StableEntityId;

/// The UUIDs of the three rules the forms of this file hold; copies of a document carry the same.
#[derive(Debug, Clone, Copy)]
struct Ids {
    fixed: StableEntityId,
    horizontal: StableEntityId,
    parallel: StableEntityId,
}

impl Ids {
    fn new() -> Self {
        Self {
            fixed: StableEntityId::new(),
            horizontal: StableEntityId::new(),
            parallel: StableEntityId::new(),
        }
    }
    fn stored(&self) -> [StableEntityId; 3] {
        [self.fixed, self.horizontal, self.parallel]
    }
}

/// The form of the shown tab with three saved rules of its Lines: Line 1's End fixed, Line 1
/// Horizontal (its points stored End then Start) and Lines 3 and 1 Parallel (the second segment
/// stored End to Start); and the curves they are on. Synthetic: the catalogue of the form is
/// amended, as in `dimension_form` and `turn_form`.
fn open_marked(w: &mut Window, rules: Ids) -> Vec<StableEntityId> {
    let curves = open_constraints(w);
    let ids: Vec<_> = curves.iter().map(|c| c.id).collect();
    let point = |i: usize, at: At| SketchPointRef::new(ids[i], at);
    let rule = |id, rule| SketchConstraint { id, rule };
    w.creates.sketch.constraints.persist_rules_for_tests(&[
        rule(
            rules.fixed,
            SketchConstraintRule::Fixed {
                point: point(0, At::End),
                x: ferritecad_document::Expression::constant(1.).expect("finite constant"),
                y: ferritecad_document::Expression::constant(2.).expect("finite constant"),
            },
        ),
        rule(
            rules.horizontal,
            SketchConstraintRule::Horizontal {
                a: point(0, At::End),
                b: point(0, At::Start),
            },
        ),
        rule(
            rules.parallel,
            SketchConstraintRule::Parallel {
                a: SketchSegmentRef::new(point(2, At::Start), point(2, At::End)),
                b: SketchSegmentRef::new(point(0, At::End), point(0, At::Start)),
            },
        ),
    ]);
    ids
}

type Held = (
    Vec<(StableEntityId, Kind)>,
    Option<StableEntityId>,
    Option<StableEntityId>,
    Target,
    ferritecad_document::SketchConstraintEdits,
);

/// What a form keeps for its tab: its rules, the selected rule, the selected Line (none while a
/// rule is), the target of its canvas and the request prepared.
fn mark_state(w: &Window) -> Held {
    let constraints = &w.creates.sketch.constraints;
    let (marks, selected) = constraints.draft_marks().expect("a draft");
    (
        marks.iter().map(|m| (m.id, m.kind)).collect(),
        selected,
        constraints.draft_state().expect("draft").0,
        constraints.draft_canvas().expect("canvas").1.target(),
        constraints.draft_edits().expect("draft"),
    )
}

/// Choose the marker that says `words` (the Rule target first) and prepare its removal.
fn remove_marker(w: &mut Window, ctx: &egui::Context, words: &str) {
    press(w, ctx, "Rule");
    let at = centre(&frame(w, ctx, vec![]), words);
    click_at(w, ctx, at);
    settle(w, ctx);
    press(w, ctx, "Remove constraint");
}

#[test]
fn the_rules_the_selection_and_the_prepared_removal_of_a_form_follow_its_tab() {
    let user = tempfile::tempdir().expect("user");
    let (a_file, b_file, c_file) = (
        user.path().join("a.fcad"),
        user.path().join("b.fcad"),
        user.path().join("c.fcad"),
    );
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    std::fs::copy(&a_file, &c_file).expect("a copy of A");
    let (a_rules, b_rules) = (Ids::new(), Ids::new());
    let mut w = Window::new(Drawn::Mock, None);
    let ctx = egui::Context::default();
    w.open(&a_file);
    let a = w.sessions.tab();
    let a_curves = open_marked(&mut w, a_rules);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    // In A: the Rule target, the marker of the Horizontal, its removal prepared.
    remove_marker(&mut w, &ctx, "H");
    let in_a = mark_state(&w);
    assert_eq!(
        in_a.0,
        vec![
            (a_rules.fixed, Kind::Fixed),
            (a_rules.horizontal, Kind::Horizontal),
            (a_rules.parallel, Kind::Parallel)
        ],
        "the form reads its own rules by their UUIDs, in the order they are stored"
    );
    assert_eq!(
        (in_a.1, in_a.2, in_a.3),
        (Some(a_rules.horizontal), None, Target::Rule)
    );
    assert_eq!(in_a.4.remove, vec![a_rules.horizontal]);
    assert!(in_a.4.add.is_empty());
    let out = frame(&mut w, &ctx, vec![]);
    assert!(painted(&out, "H -> removed") && painted(&out, "Par") && painted(&out, "Fix"));

    // Another document in another tab: its form has its own curves, rules, selection and request,
    // and not a trace of A's.
    w.open(&b_file);
    assert!(!w.creates.sketch.constraints.active(), "A's form is parked");
    let b_curves = open_marked(&mut w, b_rules);
    assert!(
        a_curves.iter().all(|x| !b_curves.contains(x))
            && a_rules
                .stored()
                .iter()
                .all(|x| !b_rules.stored().contains(x)),
        "two documents, no curve and no rule in common"
    );
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    let out = frame(&mut w, &ctx, vec![]);
    assert!(painted(&out, "H") && painted(&out, "Par") && !painted(&out, "H -> removed"));
    let in_b = mark_state(&w);
    assert_eq!(
        in_b.0.iter().map(|m| m.0).collect::<Vec<_>>(),
        b_rules.stored().to_vec()
    );
    assert_eq!(
        (in_b.1, in_b.2, in_b.3, in_b.4),
        (None, None, Target::Object, Default::default())
    );
    // B's own removal, of the other rule.
    remove_marker(&mut w, &ctx, "Par");
    let in_b = mark_state(&w);
    assert_eq!(
        (in_b.1, in_b.4.remove),
        (Some(b_rules.parallel), vec![b_rules.parallel])
    );

    // Back to A: its rules, selection and request exactly, and the marker that says so.
    w.round(a);
    assert_eq!(mark_state(&w), in_a);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    let out = frame(&mut w, &ctx, vec![]);
    assert!(painted(&out, "H -> removed") && painted(&out, "Par"));
    assert!(!painted(&out, "Par -> removed"));
    assert!(!w.sessions.dirty(), "a prepared removal is no change");

    // A copy of A in a third tab holds the very same curve and rule UUIDs and none of A's choices.
    w.open(&c_file);
    assert!(!w.creates.sketch.constraints.active(), "A's form is parked");
    let c_curves = open_marked(&mut w, a_rules);
    assert_eq!(c_curves, a_curves, "a copy has the very same curves");
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    let in_c = mark_state(&w);
    assert_eq!(in_c.0, in_a.0, "and the very same rules");
    assert_eq!(
        (in_c.1, in_c.2, in_c.3, in_c.4),
        (None, None, Target::Object, Default::default()),
        "the copy's form is its own"
    );
    remove_marker(&mut w, &ctx, "Fix");
    let in_c = mark_state(&w);
    assert_eq!(in_c.4.remove, vec![a_rules.fixed]);
    w.round(a);
    assert_eq!(mark_state(&w), in_a, "the copy's removal is not A's");
}

#[test]
fn a_rule_form_held_on_another_version_loses_its_markers_and_keeps_its_rows_selection_and_removal()
{
    let user = tempfile::tempdir().expect("user");
    let (a_file, b_file) = (user.path().join("a.fcad"), user.path().join("b.fcad"));
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let rules = Ids::new();
    let mut w = Window::new(Drawn::Mock, None);
    let ctx = egui::Context::default();
    w.open(&a_file);
    let a = w.sessions.tab();
    let feature = w.feature();
    open_marked(&mut w, rules);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    press(&mut w, &ctx, "Rule");
    let at = centre(&frame(&mut w, &ctx, vec![]), "Par");
    click_at(&mut w, &ctx, at);
    settle(&mut w, &ctx);
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
    // No drawing, no markers and no Rule target; the rows and the selection are the form's own, and
    // the removal can still be prepared (Apply stays refused).
    let out = frame(&mut w, &ctx, vec![]);
    assert!(
        !painted(&out, "Par")
            && !painted(&out, "H")
            && !painted(&out, "Fix")
            && !painted(&out, "Rule")
    );
    let held = mark_state(&w);
    assert_eq!(
        held.0.iter().map(|m| m.0).collect::<Vec<_>>(),
        rules.stored().to_vec()
    );
    assert_eq!((held.1, held.2), (Some(rules.parallel), None));
    press(&mut w, &ctx, "Remove constraint");
    let prepared = w
        .creates
        .sketch
        .constraints
        .draft_edits()
        .expect("the held form's request");
    assert_eq!(prepared.remove, vec![rules.parallel]);
}

#[test]
fn a_rule_form_given_back_by_a_cancelled_close_or_an_aborted_quit_keeps_all_of_it() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    plate(&a_file, 12.0);
    let rules = Ids::new();
    let mut w = Window::new(Drawn::Mock, None);
    let ctx = egui::Context::default();
    w.open(&a_file);
    open_marked(&mut w, rules);
    for _ in 0..3 {
        frame(&mut w, &ctx, vec![]);
    }
    remove_marker(&mut w, &ctx, "H");
    let before = mark_state(&w);
    assert_eq!(before.1, Some(rules.horizontal));
    assert_eq!(before.4.remove, vec![rules.horizontal]);

    // Close asked, confirmed and answered Cancel: the original form comes back.
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    assert!(
        !w.creates.sketch.constraints.active(),
        "the form is set aside"
    );
    assert!(w.back_from_form_close(id));
    assert_eq!(mark_state(&w), before, "a cancelled Close");
    for _ in 0..4 {
        frame(&mut w, &ctx, vec![]);
    }
    assert!(painted(&frame(&mut w, &ctx, vec![]), "H -> removed"));

    // Quit asked and aborted: the same.
    assert!(w.begin_quit());
    w.park_quit_form();
    assert!(!w.creates.sketch.constraints.active());
    w.stop_quit();
    assert_eq!(mark_state(&w), before, "an aborted Quit");
    for _ in 0..4 {
        frame(&mut w, &ctx, vec![]);
    }
    assert!(painted(&frame(&mut w, &ctx, vec![]), "H -> removed"));
    assert!(!w.sessions.stale_draft() && !w.sessions.dirty());
}
