// SPDX-License-Identifier: MIT
//! §31F: a directed angle between two saved Lines, through the form's real
//! widgets.
//!
//! A directed angle is two explicit vectors and a signed angle, added as one
//! step to the one request the draft is. These gates read the draft, the
//! request and the painted text, never a separate "angle" state; none of them
//! needs a kernel or a solver (the plate is the sample one, the stored angle is
//! written through the document API).

use super::*;
use ferritecad_document::{DirectedAngle, DirectedLine};

fn vector(curve: StableEntityId, from: LineEndpoint) -> DirectedLine {
    DirectedLine::new(curve, from, from.other()).expect("a vector")
}

fn angle_of(a: DirectedLine, b: DirectedLine, degrees: f64) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::DirectedAngle {
        a,
        b,
        angle: DirectedAngle::from_degrees(degrees).expect("an angle"),
    })
}

pub(super) fn opened_form(path: &Path, source: &ExtrudeEditSource) -> (Editor, egui::Context) {
    let mut e = Editor::default();
    assert!(e.begin(path, source, source.constraint_sketches[0].sketch));
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    (e, ctx)
}

/// Lines `a` and `b` (1-based, as the list numbers them) picked as the pair.
pub(super) fn pair(ctx: &egui::Context, e: &mut Editor, a: usize, b: usize) {
    click(ctx, e, &format!("Segment {a}"));
    click(ctx, e, "Pair Line A");
    click(ctx, e, &format!("Segment {b}"));
    click(ctx, e, "Pair Line B");
}

const ANGLE_BOX: &str = "A>B (°):";

/// The right edge of the widest painted text for which `pick` holds.
fn right_edge(out: &egui::FullOutput, pick: impl Fn(&str) -> bool) -> f32 {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if pick(t.galley.text()) => Some(t.visual_bounding_rect().right()),
            _ => None,
        })
        .fold(f32::MIN, f32::max)
}

/// A pair picked and an angle typed through the widgets, optionally added. For
/// the window's tab gate: the form as a person leaves it.
pub(crate) fn typed_angle(
    path: &Path,
    source: &ExtrudeEditSource,
    text: &str,
    add: bool,
) -> Editor {
    let (mut e, ctx) = opened_form(path, source);
    pair(&ctx, &mut e, 1, 2);
    click(&ctx, &mut e, "B: start>end");
    enter_field(&ctx, &mut e, ANGLE_BOX, text, false);
    if add {
        click(&ctx, &mut e, "Add directed angle");
    }
    e
}

/// The two explicit vectors, the signed angle and the pair slot, through the
/// widgets: one accepted add is one step, and text that is no angle stays in
/// the box and changes nothing.
#[test]
fn a_directed_angle_is_two_explicit_vectors_and_a_signed_angle_in_one_step_through_the_real_widgets()
 {
    let (_root, path, source) = fixture();
    let sketch = source.constraint_sketches[0].sketch;
    let stored = source.constraint_sketches[0]
        .stored
        .clone()
        .expect("stored Sketch");
    let curve = |i: usize| stored.curves[i].id;
    let (mut e, ctx) = opened_form(&path, &source);

    // The rows are there, say which way each vector runs and which way the angle
    // turns. They were first held to the width of the form's widest paragraph;
    // §31G puts `Add Collinear` on the same relation row, which cannot fit in
    // that width, so what is held now is that the row stays one row (no
    // taller form) and that the whole form stays inside a bounded width.
    let out = frame(&ctx, &mut e, vec![]);
    for label in [
        "A: start>end",
        "B: start>end",
        ANGLE_BOX,
        "CCW +",
        "Add directed angle",
        "Add Parallel",
        "Add Perpendicular",
        "Add Collinear",
        "Add Equal length",
    ] {
        assert!(painted(&out, label), "{label} is not painted");
    }
    let new_row = |t: &str| {
        t.starts_with("A: ")
            || t.starts_with("B: ")
            || matches!(t, ANGLE_BOX | "Add directed angle")
    };
    let (newest, oldest) = (right_edge(&out, new_row), right_edge(&out, |t| !new_row(t)));
    println!("Directed angle rows right edge {newest} pt, rest of the form {oldest} pt");
    assert!(
        newest.max(oldest) <= 700.,
        "the form reaches {newest}/{oldest} pt, past the 700 pt this row was budgeted"
    );
    let top_of = |label: &str| {
        out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                Some(t.visual_bounding_rect().top())
            }
            _ => None,
        })
    };
    for label in [
        "Add Equal length",
        "Add Parallel",
        "Add Collinear",
        ANGLE_BOX,
    ] {
        assert_eq!(
            top_of(label),
            top_of("Add Perpendicular"),
            "{label} left the relation row, which would make the form taller"
        );
    }

    // Nothing is picked: nothing is added.
    click(&ctx, &mut e, "Add directed angle");
    assert!(e.draft.as_ref().expect("draft").edits.add.is_empty());
    // The pair is the two Lines picked, and each vector says which way it runs.
    pair(&ctx, &mut e, 1, 2);
    let out = frame(&ctx, &mut e, vec![]);
    assert!(painted(&out, "A: start>end") && painted(&out, "B: start>end"));
    // A turned vector is shown turned, and turned back is shown as it was.
    click(&ctx, &mut e, "A: start>end");
    let out = frame(&ctx, &mut e, vec![]);
    assert!(painted(&out, "A: end>start") && painted(&out, "B: start>end"));
    assert!(!painted(&out, "A: start>end"));
    click(&ctx, &mut e, "A: end>start");
    assert!(painted(&frame(&ctx, &mut e, vec![]), "A: start>end"));
    assert!(
        e.draft.as_ref().expect("draft").history.undo.is_empty(),
        "choosing a direction is not a step"
    );

    // Text that is no angle: refused in words, kept exactly as typed, no step.
    for bad in [
        "",
        "-",
        "banana",
        "30,5",
        "NaN",
        "inf",
        "-inf",
        "1e999",
        "181",
        "-181",
        "180.0000001",
        "270",
        "360",
        "720",
        "−30",
    ] {
        enter_field(&ctx, &mut e, ANGLE_BOX, bad, false);
        click(&ctx, &mut e, "Add directed angle");
        let draft = e.draft.as_ref().expect("draft");
        assert!(draft.edits.add.is_empty(), "angle {bad:?} was accepted");
        assert!(draft.refusal.is_some(), "angle {bad:?} said nothing");
        assert!(draft.history.undo.is_empty() && draft.history.redo.is_empty());
        assert_eq!(draft.angle_deg, bad, "the text is the person's");
        assert_eq!(
            (draft.pair_a, draft.pair_b),
            (Some(curve(0)), Some(curve(1)))
        );
    }
    assert!(
        e.draft
            .as_ref()
            .expect("draft")
            .refusal
            .as_deref()
            .is_some_and(|r| r.contains("-180 to 180")),
        "a refusal says the range: {:?}",
        e.draft.as_ref().map(|d| &d.refusal)
    );

    // Typing is not a checkpoint; the add is exactly one.
    enter_field(&ctx, &mut e, ANGLE_BOX, "30", false);
    assert!(history_state(&e).1.is_empty(), "typing made a history step");
    click(&ctx, &mut e, "Add directed angle");
    let first = angle_of(
        vector(curve(0), LineEndpoint::Start),
        vector(curve(1), LineEndpoint::Start),
        30.,
    );
    let draft = e.draft.as_ref().expect("draft");
    assert_eq!(draft.refusal, None);
    assert_eq!(draft.edits.add, vec![first]);
    assert_eq!(draft.history.undo.len(), 1, "one add, one step");
    assert_eq!(draft.angle_deg, "30", "the typed text is not rewritten");
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        painted(
            &out,
            &format!(
                "Directed angle 30° · A: Line {} start>end · B: Line {} start>end",
                curve(0),
                curve(1)
            )
        ),
        "the pending line must say the angle and both directions"
    );

    // Another pair, the second vector turned, and the opposite spelt -180: the
    // stored value is +180, the direction is as said.
    pair(&ctx, &mut e, 3, 4);
    click(&ctx, &mut e, "B: start>end");
    enter_field(&ctx, &mut e, ANGLE_BOX, "-180", false);
    click(&ctx, &mut e, "Add directed angle");
    let second = angle_of(
        vector(curve(2), LineEndpoint::Start),
        vector(curve(3), LineEndpoint::End),
        180.,
    );
    let draft = e.draft.as_ref().expect("draft");
    assert_eq!(draft.refusal, None, "{:?}", draft.refusal);
    assert_eq!(draft.edits.add, vec![first, second]);
    assert_eq!(draft.history.undo.len(), 2);
    assert!(painted(
        &frame(&ctx, &mut e, vec![]),
        &format!(
            "Directed angle 180° · A: Line {} start>end · B: Line {} end>start",
            curve(2),
            curve(3)
        )
    ));

    // Undo and Redo move the draft and ask no job for anything.
    click(&ctx, &mut e, "Undo");
    assert_eq!(e.draft.as_ref().expect("draft").edits.add, vec![first]);
    assert!(e.take_request().is_none() && e.take_apply_request().is_none());
    click(&ctx, &mut e, "Redo");
    assert_eq!(
        e.draft.as_ref().expect("draft").edits.add,
        vec![first, second]
    );
    assert!(e.take_request().is_none() && e.take_apply_request().is_none());

    // A running job freezes the box, even one that already holds the focus, and
    // the button.
    let frozen = history_state(&e);
    enter_field(&ctx, &mut e, ANGLE_BOX, "20", false);
    let typed = e.draft.as_ref().expect("draft").angle_deg.clone();
    frame_running(&ctx, &mut e, vec![egui::Event::Text("7".into())], true);
    assert_eq!(
        e.draft.as_ref().expect("draft").angle_deg,
        typed,
        "a focused box kept taking text while a job ran"
    );
    click_running(&ctx, &mut e, "Add directed angle", true);
    assert_eq!(history_state(&e), frozen);

    // Save a copy and Apply hand over the one request the draft is, and a
    // cancelled, refused or stale reply keeps the draft, the pair and the text.
    e.set_session(true, true, true);
    click(&ctx, &mut e, "Apply constraints");
    let applied = e.take_apply_request().expect("Apply asked");
    e.set_session(true, true, false);
    click(&ctx, &mut e, "Save constraints copy…");
    let request = e.take_request().expect("Save asked");
    assert_eq!(
        crate::transaction_tests::constraints(&request),
        crate::transaction_tests::constraints(&applied)
    );
    assert_eq!(
        crate::transaction_tests::constraints(&request).add,
        vec![first, second]
    );
    assert_eq!(
        (crate::transaction_tests::sketch(&request), request.expected),
        (sketch, source.version)
    );
    assert!(e.take_request().is_none(), "one press, one request");
    let (text, history) = (
        e.draft.as_ref().expect("draft").angle_deg.clone(),
        history_state(&e),
    );
    let mut state = crate::edits::Edits::default();
    for cancelled in [false, true] {
        let (tx, rx) = std::sync::mpsc::channel();
        let generation = state
            .start(request.clone(), move |_, g, c| {
                std::thread::spawn(move || {
                    if cancelled {
                        c.cancel();
                    }
                    tx.send(g).expect("reply")
                })
            })
            .expect("start");
        let error = if cancelled {
            ferritecad_types::CadError::Cancelled
        } else {
            ferritecad_types::CadError::constraint("a directed angle the solver could not satisfy")
        };
        let stale = Err(ferritecad_types::CadError::input("stale"));
        assert!(finish_edit(&mut e, &mut state, generation + 1, stale).is_none());
        let g = rx.recv().expect("done");
        assert!(finish_edit(&mut e, &mut state, g, Err(error)).is_none());
        let draft = e.draft.as_ref().expect("retained");
        assert_eq!(draft.angle_deg, text);
        assert_eq!(
            (draft.pair_a, draft.pair_b),
            (Some(curve(2)), Some(curve(3)))
        );
        assert_eq!(history_state(&e), history);
    }
    click(&ctx, &mut e, "Cancel constraints draft");
    assert!(!e.active());
}

/// A stored angle is listed with its directions, comes off by its exact UUID,
/// and a new one in the same request replaces it.
#[test]
fn a_stored_angle_is_replaced_by_another_in_one_exact_remove_and_add() {
    let (_root, path, plain) = fixture();
    let c = plain.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .curves
        .iter()
        .map(|c| c.id)
        .collect::<Vec<_>>();
    // The stored angle is written through the document, turned against the
    // stored order, as an earlier editor session would have left it.
    let mut document = Document::open(&path).expect("doc");
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        plain.constraint_sketches[0].sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![angle_of(
                vector(c[0], LineEndpoint::Start),
                vector(c[1], LineEndpoint::End),
                -150.,
            )],
        },
    )
    .expect("prepare");
    let stored_id = ferritecad_fixtures::added_constraints(&prepared)
        .last()
        .expect("the angle")
        .id;
    document.write_changes(&prepared).expect("write");
    let source = crate::sketch_facts::Source::read(&document).expect("catalog");
    document.close().expect("close");
    let (mut e, ctx) = opened_form(&path, &source);

    let row = format!("Directed angle -150° · {stored_id}");
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        painted(&out, &row),
        "the stored angle is listed with its value"
    );
    scroll_to_row(&ctx, &mut e, &row);
    assert!(painted(
        &frame(&ctx, &mut e, vec![]),
        &format!(
            "A Line {} start>end > B Line {} end>start",
            short(c[0]),
            short(c[1])
        )
    ));
    click_remove_on_row(&ctx, &mut e, &row);
    assert_eq!(
        e.draft.as_ref().expect("draft").edits.remove,
        vec![stored_id]
    );

    // The same pair, now said the other way round: free only because the stored
    // one is on its way out in this very request.
    pair(&ctx, &mut e, 2, 1);
    enter_field(&ctx, &mut e, ANGLE_BOX, "150", false);
    click(&ctx, &mut e, "Add directed angle");
    let draft = e.draft.as_ref().expect("draft");
    assert_eq!(draft.refusal, None, "{:?}", draft.refusal);
    assert_eq!(draft.history.undo.len(), 2, "the removal and the add");
    assert_eq!(
        draft.edits.add,
        vec![angle_of(
            vector(c[1], LineEndpoint::Start),
            vector(c[0], LineEndpoint::Start),
            150.
        )]
    );
    e.set_session(true, true, false);
    click(&ctx, &mut e, "Save constraints copy…");
    let request = e.take_request().expect("Save asked");
    assert_eq!(
        crate::transaction_tests::constraints(&request).remove,
        vec![stored_id]
    );
    assert_eq!(crate::transaction_tests::constraints(&request).add.len(), 1);
    // The form also permits an additional rule; the solver decides its effect.
    click(&ctx, &mut e, "Undo");
    click(&ctx, &mut e, "Undo");
    click(&ctx, &mut e, "Add directed angle");
    assert!(e.draft.as_ref().expect("draft").refusal.is_none());
}

/// A directed angle between Lines `a` and `b` (1-based, as the list numbers
/// them), the second vector turned when asked, added through the widgets and
/// Applied: the form as the window's session gate takes it.
pub(crate) fn typed_angle_apply(
    path: &Path,
    source: &ExtrudeEditSource,
    (a, b): (usize, usize),
    turn_b: bool,
    text: &str,
) -> (Editor, EditRequest) {
    let (mut e, ctx) = opened_form(path, source);
    e.set_session(true, true, true);
    pair(&ctx, &mut e, a, b);
    if turn_b {
        click(&ctx, &mut e, "B: start>end");
    }
    enter_field(&ctx, &mut e, ANGLE_BOX, text, false);
    click(&ctx, &mut e, "Add directed angle");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    click(&ctx, &mut e, "Apply constraints");
    let request = e.take_apply_request().expect("Apply constraints asked");
    (e, request)
}
