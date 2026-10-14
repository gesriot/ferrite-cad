// SPDX-License-Identifier: MIT
//! §31F: a directed angle between two saved Lines, through the form's real
//! widgets.
//!
//! A directed angle is two explicit vectors and a signed angle, added as one
//! step to the one request the draft is. These gates read the draft, the
//! request and the painted text, never a separate "angle" state; none of them
//! needs a kernel or a solver (the plate is the sample one, the stored angle is
//! written through the document API). The native comparison with the peer CLI
//! lives in `native_directed_angle_form_worker_and_cli_publish_the_same_geometry`.

use super::*;
use ferritecad_document::{DirectedAngle, DirectedLine, ObjectPayload};

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

fn opened_form(path: &Path, source: &ExtrudeEditSource) -> (Editor, egui::Context) {
    let mut e = Editor::default();
    assert!(e.begin(path, source, source.constraint_sketches[0].sketch));
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    (e, ctx)
}

/// Lines `a` and `b` (1-based, as the list numbers them) picked as the pair.
fn pair(ctx: &egui::Context, e: &mut Editor, a: usize, b: usize) {
    click(ctx, e, &format!("Segment {a}"));
    click(ctx, e, "Pair Line A");
    click(ctx, e, &format!("Segment {b}"));
    click(ctx, e, "Pair Line B");
}

const ANGLE_BOX: &str = "A→B (°):";

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
    click(&ctx, &mut e, "B: start→end");
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
    // turns, and widen nothing: text that was already in the form reaches at
    // least as far right as the new rows do.
    let out = frame(&ctx, &mut e, vec![]);
    for label in [
        "A: start→end",
        "B: start→end",
        ANGLE_BOX,
        "CCW +",
        "Add directed angle",
        "Add Parallel",
        "Add Perpendicular",
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
        newest <= oldest,
        "the angle rows ({newest}) reach past the rest of the form ({oldest})"
    );

    // Nothing is picked: nothing is added.
    click(&ctx, &mut e, "Add directed angle");
    assert!(e.draft.as_ref().expect("draft").edits.add.is_empty());
    // The pair is the two Lines picked, and each vector says which way it runs.
    pair(&ctx, &mut e, 1, 2);
    let out = frame(&ctx, &mut e, vec![]);
    assert!(painted(&out, "A: start→end") && painted(&out, "B: start→end"));
    // A turned vector is shown turned, and turned back is shown as it was.
    click(&ctx, &mut e, "A: start→end");
    let out = frame(&ctx, &mut e, vec![]);
    assert!(painted(&out, "A: end→start") && painted(&out, "B: start→end"));
    assert!(!painted(&out, "A: start→end"));
    click(&ctx, &mut e, "A: end→start");
    assert!(painted(&frame(&ctx, &mut e, vec![]), "A: start→end"));
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
                "Directed angle 30° · A: Line {} start→end · B: Line {} start→end",
                curve(0),
                curve(1)
            )
        ),
        "the pending line must say the angle and both directions"
    );

    // One pair has one relative-orientation slot, whichever way it is said.
    let kept = history_state(&e);
    enter_field(&ctx, &mut e, ANGLE_BOX, "-150", false);
    click(&ctx, &mut e, "A: start→end");
    click(&ctx, &mut e, "Add directed angle");
    assert!(
        e.draft
            .as_ref()
            .expect("d")
            .refusal
            .as_deref()
            .is_some_and(|r| r.contains("directed angle")),
        "the same relation with A turned is a second answer to one question"
    );
    assert_eq!(history_state(&e), kept);
    click(&ctx, &mut e, "A: end→start");
    click(&ctx, &mut e, "Add Parallel");
    assert!(
        e.draft
            .as_ref()
            .expect("d")
            .refusal
            .as_deref()
            .is_some_and(|r| r.contains("directed angle")),
        "a Parallel on an angled pair is refused for the slot"
    );
    assert_eq!(history_state(&e), kept);

    // Another pair, the second vector turned, and the opposite spelt -180: the
    // stored value is +180, the direction is as said.
    pair(&ctx, &mut e, 3, 4);
    click(&ctx, &mut e, "B: start→end");
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
            "Directed angle 180° · A: Line {} start→end · B: Line {} end→start",
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
    assert_eq!(request.edits, applied.edits);
    assert_eq!(request.edits.add, vec![first, second]);
    assert_eq!((request.sketch, request.expected), (sketch, source.version));
    assert!(e.take_request().is_none(), "one press, one request");
    let (text, history) = (
        e.draft.as_ref().expect("draft").angle_deg.clone(),
        history_state(&e),
    );
    let mut state = crate::edits::Edits::default();
    for cancelled in [false, true] {
        let (tx, rx) = std::sync::mpsc::channel();
        let generation = state
            .start_constraints(request.clone(), move |_, g, c| {
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
    println!("\nFCAD_31F_WIDGETS_EXECUTED");
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
    let prepared = ferritecad_document::prepare_sketch_constraints(
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
    let stored_id = prepared.added.last().expect("the angle").id;
    document.write_sketch_constraints(&prepared).expect("write");
    let source = ExtrudeEditSource::read(&document).expect("catalog");
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
            "A Line {} start→end → B Line {} end→start",
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
    assert_eq!(request.edits.remove, vec![stored_id]);
    assert_eq!(request.edits.add.len(), 1);
    // And without the removal the same add is refused for the slot.
    click(&ctx, &mut e, "Undo");
    click(&ctx, &mut e, "Undo");
    enter_field(&ctx, &mut e, ANGLE_BOX, "150", false);
    click(&ctx, &mut e, "Add directed angle");
    assert!(
        e.draft
            .as_ref()
            .expect("d")
            .refusal
            .as_deref()
            .is_some_and(|r| r.contains("directed angle"))
    );
    println!("\nFCAD_31F_REPLACE_EXECUTED");
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
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut e, ctx) = opened_form(path, source);
    e.set_session(true, true, true);
    pair(&ctx, &mut e, a, b);
    if turn_b {
        click(&ctx, &mut e, "B: start→end");
    }
    enter_field(&ctx, &mut e, ANGLE_BOX, text, false);
    click(&ctx, &mut e, "Add directed angle");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    click(&ctx, &mut e, "Apply constraints");
    let request = e.take_apply_request().expect("Apply constraints asked");
    (e, request)
}

/// Native, once: a dart's two Lines picked, the second vector turned and
/// `110` typed into the form's box, saved by the window's worker; then the same
/// spelling given to the peer CLI. The two files must carry the same rule, the
/// same identities and SQL, solve to the same reflex corner (`-70°`, measured
/// from the reopened file) and export the same bytes.
#[test]
fn native_directed_angle_form_worker_and_cli_publish_the_same_geometry() {
    use crate::creates::tests::ferritecad;
    // Each missing piece is excused only where that piece is not required.
    for (present, required, what) in [
        (
            ferritecad_occt::is_available(),
            "FERRITECAD_REQUIRE_OCCT",
            "OCCT",
        ),
        (
            ferritecad_sketch_solver::is_available(),
            "FERRITECAD_REQUIRE_PLANEGCS",
            "planegcs",
        ),
    ] {
        if !present {
            assert_ne!(std::env::var(required).as_deref(), Ok("1"));
            eprintln!("skipped: the directed angle gate needs {what}");
            return;
        }
    }
    // The dart carries data this editor knows nothing about (an attached source
    // claim, an unknown table and capability rows): it has to come through.
    let root = tempfile::tempdir().expect("dir");
    let source_path = root.path().join("dart.fcad");
    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            &source_path,
            ferritecad_jobs::NewDocument::SketchExtrude(
                ferritecad_document::PolygonExtrusion::new(
                    vec![[0., 0.], [40., 0.], [10., 10.], [0., 40.]],
                    10.,
                )
                .expect("a dart"),
            ),
            "test",
        ),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
    .expect("source");
    crate::sketch::drag_tests::attach_source_claim(&source_path);
    let source = {
        let d = Document::open_read_only(&source_path).expect("doc");
        let source = ExtrudeEditSource::read(&d).expect("snapshot");
        d.close().expect("close");
        source
    };
    let choice = &source.constraint_sketches[0];
    let sketch = choice.sketch;
    let saved = choice.stored.clone().expect("stored");
    let before = std::fs::read(&source_path).expect("source bytes");

    // The window: pair Lines 2 and 3, the second vector turned, 110 typed.
    let (mut e, ctx) = opened_form(&source_path, &source);
    pair(&ctx, &mut e, 2, 3);
    click(&ctx, &mut e, "B: start→end");
    enter_field(&ctx, &mut e, ANGLE_BOX, "110", false);
    click(&ctx, &mut e, "Add directed angle");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    click(&ctx, &mut e, "Save constraints copy…");
    let mut request = e.take_request().expect("widget request");
    let expected = angle_of(
        vector(saved.curves[1].id, LineEndpoint::Start),
        vector(saved.curves[2].id, LineEndpoint::End),
        110.,
    );
    assert_eq!(request.edits.add, vec![expected]);
    let ui = root.path().join("ui.fcad");
    request.destination = ui.clone();
    let mut state = crate::edits::Edits::default();
    let (tx, rx) = std::sync::mpsc::channel();
    state
        .start_constraints(request.clone(), move |r, g, c| {
            crate::edits::spawn_constraint_edit(r, c, move |result| {
                tx.send((g, result)).expect("reply")
            })
        })
        .expect("worker");
    let (g, result) = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("worker response");
    assert_eq!(
        result
            .as_ref()
            .ok()
            .map(|r| r.solve.as_ref().expect("solve").degrees_of_freedom()),
        Some(7)
    );
    assert_eq!(finish_edit(&mut e, &mut state, g, result), Some(ui.clone()));
    assert!(!e.active(), "an accepted copy ends the form");

    // The peer: the same spelling, as request v1.
    let cli = root.path().join("cli.fcad");
    let request_file = root.path().join("request.json");
    std::fs::write(
        &request_file,
        super::session_apply::peer_request(&SketchConstraintEdits {
            remove: vec![],
            add: vec![expected],
        }),
    )
    .expect("request");
    let run = |args: &[&std::ffi::OsStr]| {
        let out = std::process::Command::new(ferritecad())
            .args(args)
            .output()
            .expect("peer CLI");
        assert!(out.status.success(), "{out:?}");
        out
    };
    run(&[
        "edit-sketch-constraints-copy".as_ref(),
        source_path.as_os_str(),
        "--sketch".as_ref(),
        sketch.to_string().as_ref(),
        "--expect-version".as_ref(),
        source.version.content.to_string().as_ref(),
        "--request".as_ref(),
        request_file.as_os_str(),
        "-o".as_ref(),
        cli.as_os_str(),
    ]);
    assert_eq!(std::fs::read(&source_path).expect("source"), before);

    // The same rule, the same identities; only the freshly minted UUIDs differ.
    let sketch_of = |path: &Path| {
        let d = Document::open_read_only(path).expect("copy");
        let objects = d.objects().expect("objects");
        let row = objects.iter().find(|o| o.id == sketch).expect("sketch");
        let ObjectPayload::Sketch(s) = row.payload.clone() else {
            panic!("a Sketch")
        };
        let others: Vec<_> = objects.iter().filter(|o| o.id != sketch).cloned().collect();
        let facts = (
            d.meta().document_id,
            others,
            d.dependencies().expect("dependencies"),
            d.topology_refs().expect("refs"),
        );
        d.close().expect("close");
        (s, facts)
    };
    let (ui_sketch, ui_facts) = sketch_of(&ui);
    let (cli_sketch, cli_facts) = sketch_of(&cli);
    let (source_sketch, source_facts) = sketch_of(&source_path);
    assert_eq!(
        ui_facts, cli_facts,
        "objects, links and refs of the two copies"
    );
    assert_eq!(
        ui_facts.1, source_facts.1,
        "every other object is the source's"
    );
    assert_eq!(
        ui_facts.3, source_facts.3,
        "the stored references are the source's"
    );
    assert_eq!(ui_sketch.curves, source_sketch.curves, "the guess was kept");
    assert_eq!(cli_sketch.curves, source_sketch.curves);
    assert_eq!(ui_sketch.schema_version(), 4);
    let rules = |s: &Sketch| s.constraints.iter().map(|c| c.rule).collect::<Vec<_>>();
    assert_eq!(rules(&ui_sketch), rules(&cli_sketch), "the same five rules");
    assert_eq!(ui_sketch.constraints.len(), 5);
    for (x, y) in ui_sketch.constraints.iter().zip(&cli_sketch.constraints) {
        assert_ne!(x.id, y.id, "each publication minted its own UUID");
    }
    let SketchConstraintRule::DirectedAngle { a, b, angle } = ui_sketch.constraints[4].rule else {
        panic!("the angle comes last")
    };
    assert_eq!(
        (a.from.at, a.to.at, b.from.at, b.to.at),
        (
            ferritecad_document::SketchPointSelector::Start,
            ferritecad_document::SketchPointSelector::End,
            ferritecad_document::SketchPointSelector::End,
            ferritecad_document::SketchPointSelector::Start,
        ),
        "both directions as said"
    );
    assert_eq!(angle, 110f64.to_radians());

    // Every SQL cell is literally the other file's, but the Sketch row's payload
    // and hash (where the new UUIDs live) and the modified timestamp; and the
    // source's rows, but for the Sketch and the new capability, are all kept.
    type Rows = Vec<Vec<rusqlite::types::Value>>;
    let (a, b, s) = (
        crate::fillets::tests::tables(&ui),
        crate::fillets::tests::tables(&cli),
        crate::fillets::tests::tables(&source_path),
    );
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    let selected = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
    let mut cells = 0;
    for (table, (columns, rows)) in &a {
        assert_eq!(rows.len(), b[table].1.len(), "{table}");
        for (x, y) in rows.iter().zip(&b[table].1) {
            for (i, cell) in x.iter().enumerate() {
                cells += 1;
                if cell == &y[i] {
                    continue;
                }
                let column = columns[i].as_str();
                assert!(
                    match table.as_str() {
                        "objects" =>
                            x.contains(&selected) && matches!(column, "payload" | "payload_hash"),
                        "meta" => column == "modified_at",
                        _ => false,
                    },
                    "{table}.{column} differs between the two copies"
                );
            }
        }
    }
    assert!(cells > 50, "{cells} cells is not a comparison");
    for (table, (_, rows)) in &s {
        match table.as_str() {
            "meta" => {}
            "capabilities" => {
                for row in rows {
                    assert!(
                        a[table].1.iter().any(|m| m[1..] == row[1..]),
                        "capability row {row:?} lost"
                    );
                }
                let angle = a[table]
                    .1
                    .iter()
                    .filter(|m| {
                        m.contains(&rusqlite::types::Value::Text(
                            "sketch.constraints.angle.v1".into(),
                        ))
                    })
                    .count();
                assert_eq!(angle, 1, "the angle capability is declared once");
            }
            "objects" => {
                let rest = |rows: &Rows| {
                    rows.iter()
                        .filter(|r| !r.contains(&selected))
                        .cloned()
                        .collect::<Vec<_>>()
                };
                assert_eq!(rest(&a[table].1), rest(rows), "another object changed");
            }
            _ => assert_eq!(a[table].1, *rows, "{table} was not preserved"),
        }
    }

    // Reopen and rebuild cold: the solved corner is the one that was asked for,
    // measured from the solved points of each file.
    let corner = |path: &Path| {
        let doc = Document::open_read_only(path).expect("reopen");
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let built = ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("cold rebuild after reopen");
        let report = built.solve_report(sketch).expect("solve report");
        assert!(report.redundant().is_empty());
        let dof = report.degrees_of_freedom();
        let presentation = built.sketch_presentation(sketch).expect("presentation");
        let vectors: Vec<[f64; 2]> = presentation
            .curves()
            .iter()
            .map(|c| {
                let SketchGeometry::Line { start, end } = c.geometry() else {
                    panic!("a Line")
                };
                [end.x - start.x, end.y - start.y]
            })
            .collect();
        for r in doc.topology_refs().expect("refs") {
            assert_eq!(built.resolve(&r).expect("resolves").len(), 1);
        }
        built.release_all(&mut kernel);
        doc.close().expect("close");
        let (u, v) = (vectors[1], vectors[2]);
        (
            (u[0] * v[1] - u[1] * v[0])
                .atan2(u[0] * v[0] + u[1] * v[1])
                .to_degrees(),
            dof,
        )
    };
    for path in [&ui, &cli] {
        let (measured, dof) = corner(path);
        assert_eq!(dof, 7);
        assert!(
            (measured + 70.).abs() < 1e-6,
            "the reflex corner measures {measured}°"
        );
    }

    // The exported bytes of the two files are the same bytes.
    let bytes = |path: &Path| {
        let (stl, fbx) = (path.with_extension("stl"), path.with_extension("fbx"));
        run(&[
            "export-stl".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            stl.as_os_str(),
        ]);
        run(&[
            "export-fbx".as_ref(),
            path.as_os_str(),
            "-o".as_ref(),
            fbx.as_os_str(),
        ]);
        (stl, fbx)
    };
    let (ui_stl, ui_fbx) = bytes(&ui);
    let (cli_stl, cli_fbx) = bytes(&cli);
    assert_eq!(
        std::fs::read(&ui_stl).expect("STL"),
        std::fs::read(&cli_stl).expect("STL"),
        "the STL of the window's copy is the command line's"
    );
    assert_eq!(
        std::fs::read(&ui_fbx).expect("FBX"),
        std::fs::read(&cli_fbx).expect("FBX")
    );
    if let Some(dir) = std::env::var_os("FCAD_DIRECTED_ANGLE_ARTIFACT_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifact directory");
        std::fs::copy(&ui_fbx, dir.join("angle-ui.fbx")).expect("FBX for the pinned reader");
    }

    // Replace, then remove, from the window's own copy: the form's Remove row and
    // a new Add (one request, one worker), then the Remove alone. Each answer is
    // set against the peer CLI given the same request on the same file.
    let work = |request: EditSketchConstraintsRequest, to: &Path| {
        let mut request = request;
        request.destination = to.to_path_buf();
        let mut state = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start_constraints(request, move |r, g, c| {
                crate::edits::spawn_constraint_edit(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (g, result) = rx
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("worker response");
        let dof = result
            .as_ref()
            .ok()
            .map(|r| r.solve.as_ref().map(|s| s.degrees_of_freedom()));
        let mut idle = Editor::default();
        (finish_edit(&mut idle, &mut state, g, result), dof)
    };
    let peer = |from: &Path,
                version: &ferritecad_document::DocumentVersion,
                edits: &SketchConstraintEdits,
                to: &Path| {
        std::fs::write(&request_file, super::session_apply::peer_request(edits)).expect("request");
        run(&[
            "edit-sketch-constraints-copy".as_ref(),
            from.as_os_str(),
            "--sketch".as_ref(),
            sketch.to_string().as_ref(),
            "--expect-version".as_ref(),
            version.content.to_string().as_ref(),
            "--request".as_ref(),
            request_file.as_os_str(),
            "-o".as_ref(),
            to.as_os_str(),
        ]);
    };
    let ids_of = |path: &Path| {
        sketch_of(path)
            .0
            .constraints
            .iter()
            .map(|c| c.id)
            .collect::<Vec<_>>()
    };
    let reading = |path: &Path| {
        let d = Document::open_read_only(path).expect("doc");
        let source = ExtrudeEditSource::read(&d).expect("snapshot");
        d.close().expect("close");
        source
    };
    let from_ui = reading(&ui);
    let old_ids = ids_of(&ui);
    let stored_angle = old_ids[4];
    let (mut e, ctx) = opened_form(&ui, &from_ui);
    click_remove_on_row(
        &ctx,
        &mut e,
        &format!("Directed angle 110° · {stored_angle}"),
    );
    pair(&ctx, &mut e, 2, 3);
    click(&ctx, &mut e, "B: start→end");
    enter_field(&ctx, &mut e, ANGLE_BOX, "130", false);
    click(&ctx, &mut e, "Add directed angle");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    click(&ctx, &mut e, "Save constraints copy…");
    let replace = e.take_request().expect("widget request");
    assert_eq!(replace.edits.remove, vec![stored_angle]);
    let (ui2, cli2) = (root.path().join("ui2.fcad"), root.path().join("cli2.fcad"));
    let (made, dof) = work(replace.clone(), &ui2);
    assert_eq!((made, dof), (Some(ui2.clone()), Some(Some(7))));
    peer(&ui, &from_ui.version, &replace.edits, &cli2);
    let (new_ui, new_cli) = (ids_of(&ui2), ids_of(&cli2));
    assert_eq!(new_ui[..4], old_ids[..4], "the joints keep their UUIDs");
    assert_eq!(new_ui[..4], new_cli[..4]);
    assert!(!new_ui.contains(&stored_angle) && !new_cli.contains(&stored_angle));
    assert_ne!(
        new_ui[4], new_cli[4],
        "each publication minted its own UUID"
    );
    assert_eq!(
        sketch_of(&ui2).0.constraints[4].rule,
        sketch_of(&cli2).0.constraints[4].rule
    );
    for path in [&ui2, &cli2] {
        let (measured, dof) = corner(path);
        assert_eq!(dof, 7);
        // 130° from Line 2 to Line 3 reversed is -50° from Line 2 to Line 3.
        assert!((measured + 50.).abs() < 1e-6, "measures {measured}°");
    }
    run(&[
        "export-stl".as_ref(),
        ui2.as_os_str(),
        "-o".as_ref(),
        ui2.with_extension("stl").as_os_str(),
    ]);
    run(&[
        "export-stl".as_ref(),
        cli2.as_os_str(),
        "-o".as_ref(),
        cli2.with_extension("stl").as_os_str(),
    ]);
    assert_eq!(
        std::fs::read(ui2.with_extension("stl")).expect("STL"),
        std::fs::read(cli2.with_extension("stl")).expect("STL")
    );
    assert_ne!(
        std::fs::read(ui2.with_extension("stl")).expect("STL"),
        std::fs::read(&ui_stl).expect("STL"),
        "the replaced angle is another solid"
    );

    let from_ui2 = reading(&ui2);
    let (mut e, ctx) = opened_form(&ui2, &from_ui2);
    click_remove_on_row(
        &ctx,
        &mut e,
        &format!("Directed angle 130° · {}", new_ui[4]),
    );
    click(&ctx, &mut e, "Save constraints copy…");
    let remove = e.take_request().expect("widget request");
    assert_eq!(
        (remove.edits.remove.clone(), remove.edits.add.len()),
        (vec![new_ui[4]], 0)
    );
    let (ui3, cli3) = (root.path().join("ui3.fcad"), root.path().join("cli3.fcad"));
    let (made, dof) = work(remove.clone(), &ui3);
    assert_eq!((made, dof), (Some(ui3.clone()), Some(Some(8))));
    peer(&ui2, &from_ui2.version, &remove.edits, &cli3);
    for path in [&ui3, &cli3] {
        assert_eq!(ids_of(path), new_ui[..4].to_vec(), "only the joints remain");
        assert_eq!(
            sketch_of(path).0.schema_version(),
            2,
            "the layout every older build writes"
        );
        assert_eq!(corner(path).1, 8);
    }
    assert_eq!(
        crate::fillets::tests::tables(&ui3)["objects"].1.len(),
        crate::fillets::tests::tables(&cli3)["objects"].1.len()
    );
    println!("\nFCAD_31F_NATIVE_EXECUTED turn=-70 constraints=5 dof=7");
}
