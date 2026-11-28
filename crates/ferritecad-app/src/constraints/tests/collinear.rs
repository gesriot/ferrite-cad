// SPDX-License-Identifier: MIT
//! §31G: two saved Lines on one infinite line, through the form's real widgets.
//!
//! A collinearity is a pair relation, added as one step to the one request the
//! draft is. These gates read the draft, the request and the painted text,
//! never a separate "collinear" state; the first two need neither a kernel nor
//! a solver (the plate is the sample one, the stored collinearity is written
//! through the document API). The native comparison with the peer CLI is
//! in the sketch-mode Collinear test.

use super::angle::{opened_form, pair};
use super::*;

fn collinear_of(a: StableEntityId, b: StableEntityId) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::Relation {
        a,
        b,
        relation: LineRelation::Collinear,
    })
}

/// A pair picked and a collinearity added through the widgets, as the form is
/// left: for the window's session and tab gates.
pub(crate) fn collinear_apply(
    path: &Path,
    source: &ExtrudeEditSource,
    (a, b): (usize, usize),
) -> (Editor, EditRequest) {
    let (mut e, ctx) = opened_form(path, source);
    e.set_session(true, true, true);
    pair(&ctx, &mut e, a, b);
    click(&ctx, &mut e, "Add Collinear");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    click(&ctx, &mut e, "Apply constraints");
    let request = e.take_apply_request().expect("Apply constraints asked");
    (e, request)
}

/// A pair picked and, optionally, a collinearity added through the widgets:
/// the form as a person leaves it, for the window's tab gate.
pub(crate) fn typed_collinear(
    path: &Path,
    source: &ExtrudeEditSource,
    (a, b): (usize, usize),
    add: bool,
) -> Editor {
    let (mut e, ctx) = opened_form(path, source);
    pair(&ctx, &mut e, a, b);
    if add {
        click(&ctx, &mut e, "Add Collinear");
    }
    e
}

/// The pair, the button, the slot and the steps, through the widgets: one
/// accepted add is one step; the vector directions of the angle row are read by
/// nothing here; a second answer on the pair is refused in words.
#[test]
fn a_collinear_is_one_step_on_a_picked_pair_and_reads_neither_vector_direction() {
    let (_root, path, source) = fixture();
    let sketch = source.constraint_sketches[0].sketch;
    let stored = source.constraint_sketches[0]
        .stored
        .clone()
        .expect("stored Sketch");
    let curve = |i: usize| stored.curves[i].id;
    let (mut e, ctx) = opened_form(&path, &source);

    let out = frame(&ctx, &mut e, vec![]);
    assert!(painted(&out, "Add Collinear"), "the button is not painted");
    // Nothing is picked: nothing is added.
    click(&ctx, &mut e, "Add Collinear");
    assert!(e.draft.as_ref().expect("draft").edits.add.is_empty());
    // Two opposite sides, each vector turned the way it is not stored: the
    // request carries the two Lines and no direction at all.
    pair(&ctx, &mut e, 1, 3);
    click(&ctx, &mut e, "A: start>end");
    click(&ctx, &mut e, "B: start>end");
    click(&ctx, &mut e, "Add Collinear");
    let first = collinear_of(curve(0), curve(2));
    let draft = e.draft.as_ref().expect("draft");
    assert_eq!(draft.refusal, None, "{:?}", draft.refusal);
    assert_eq!(draft.edits.add, vec![first]);
    assert_eq!(draft.history.undo.len(), 1, "one add, one step");
    assert_eq!(
        (draft.vector_a_from, draft.vector_b_from),
        (LineEndpoint::End, LineEndpoint::End),
        "adding did not touch the vector directions"
    );
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        painted(
            &out,
            &format!("Collinear · Lines {} and {}", curve(0), curve(2))
        ),
        "the pending line must name the relation and both Lines"
    );
    // The same request whichever way the vectors are turned back.
    click(&ctx, &mut e, "A: end>start");
    click(&ctx, &mut e, "B: end>start");

    pair(&ctx, &mut e, 1, 3);
    // An equal length on the same pair is another property: its own slot.
    click(&ctx, &mut e, "Add Equal length");
    assert_eq!(e.draft.as_ref().expect("d").refusal, None);
    assert_eq!(e.draft.as_ref().expect("d").history.undo.len(), 2);
    // Another pair, and the relation of the first pair kept, with the other
    // spelling of the pair: neither lead matters.
    pair(&ctx, &mut e, 4, 2);
    click(&ctx, &mut e, "Add Collinear");
    let second = collinear_of(curve(3), curve(1));
    let draft = e.draft.as_ref().expect("draft");
    assert_eq!(draft.refusal, None, "{:?}", draft.refusal);
    assert_eq!(draft.history.undo.len(), 3);
    assert_eq!(draft.edits.add.len(), 3);
    assert_eq!((draft.edits.add[0], draft.edits.add[2]), (first, second));

    // Undo and Redo move the draft and ask no job for anything.
    click(&ctx, &mut e, "Undo");
    click(&ctx, &mut e, "Undo");
    assert_eq!(e.draft.as_ref().expect("draft").edits.add, vec![first]);
    assert!(e.take_request().is_none() && e.take_apply_request().is_none());
    click(&ctx, &mut e, "Redo");
    click(&ctx, &mut e, "Redo");
    assert_eq!(e.draft.as_ref().expect("draft").edits.add.len(), 3);
    assert!(e.take_request().is_none() && e.take_apply_request().is_none());

    // A running job freezes the button.
    let frozen = history_state(&e);
    pair(&ctx, &mut e, 1, 2);
    click_running(&ctx, &mut e, "Add Collinear", true);
    assert_eq!(history_state(&e), frozen);

    // Save a copy and Apply hand over the one request the draft is.
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
    assert_eq!(crate::transaction_tests::constraints(&request).add.len(), 3);
    assert_eq!(
        (crate::transaction_tests::sketch(&request), request.expected),
        (sketch, source.version)
    );
    assert!(e.take_request().is_none(), "one press, one request");
    let (pair_now, history) = (
        e.draft.as_ref().map(|d| (d.pair_a, d.pair_b)),
        history_state(&e),
    );
    // A cancelled, refused or stale reply keeps the draft, the pair and the
    // history, as the form's other families do.
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
            ferritecad_types::CadError::constraint("a collinearity the solver could not satisfy")
        };
        let stale = Err(ferritecad_types::CadError::input("stale"));
        assert!(finish_edit(&mut e, &mut state, generation + 1, stale).is_none());
        let g = rx.recv().expect("done");
        assert!(finish_edit(&mut e, &mut state, g, Err(error)).is_none());
        let draft = e.draft.as_ref().expect("retained");
        assert_eq!(Some((draft.pair_a, draft.pair_b)), pair_now);
        assert_eq!(history_state(&e), history);
    }
    click(&ctx, &mut e, "Cancel constraints draft");
    assert!(!e.active());
}

/// A stored collinearity is listed with both Lines, comes off by its exact
/// UUID, and a Parallel on the same pair in the same request replaces it.
#[test]
fn a_stored_collinear_is_replaced_by_a_parallel_in_one_exact_remove_and_add() {
    let (_root, path, plain) = fixture();
    let c = plain.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .curves
        .iter()
        .map(|c| c.id)
        .collect::<Vec<_>>();
    // Written through the document, as an earlier editor session would have
    // left it.
    let mut document = Document::open(&path).expect("doc");
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        plain.constraint_sketches[0].sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![collinear_of(c[0], c[2])],
        },
    )
    .expect("prepare");
    let stored_id = ferritecad_fixtures::added_constraints(&prepared)
        .last()
        .expect("the collinear")
        .id;
    document.write_changes(&prepared).expect("write");
    let source = crate::sketch_facts::Source::read(&document).expect("catalog");
    document.close().expect("close");
    let (mut e, ctx) = opened_form(&path, &source);

    let row = format!("Collinear · {stored_id}");
    assert!(
        painted(&frame(&ctx, &mut e, vec![]), &row),
        "the stored collinearity is listed by its own name"
    );
    scroll_to_row(&ctx, &mut e, &row);
    assert!(painted(
        &frame(&ctx, &mut e, vec![]),
        &format!("Lines {} and {}", short(c[0]), short(c[2]))
    ));
    click_remove_on_row(&ctx, &mut e, &row);
    assert_eq!(
        e.draft.as_ref().expect("draft").edits.remove,
        vec![stored_id]
    );

    // Free only because the stored one is on its way out in this very request.
    pair(&ctx, &mut e, 3, 1);
    click(&ctx, &mut e, "Add Parallel");
    let draft = e.draft.as_ref().expect("draft");
    assert_eq!(draft.refusal, None, "{:?}", draft.refusal);
    assert_eq!(draft.history.undo.len(), 2, "the removal and the add");
    assert_eq!(
        draft.edits.add,
        vec![AddSketchConstraint::Line(AddLineConstraint::Relation {
            a: c[2],
            b: c[0],
            relation: LineRelation::Parallel,
        })]
    );
    e.set_session(true, true, false);
    click(&ctx, &mut e, "Save constraints copy…");
    let request = e.take_request().expect("Save asked");
    assert_eq!(
        crate::transaction_tests::constraints(&request).remove,
        vec![stored_id]
    );
    assert_eq!(crate::transaction_tests::constraints(&request).add.len(), 1);
}
