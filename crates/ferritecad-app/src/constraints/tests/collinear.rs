// SPDX-License-Identifier: MIT
//! §31G: two saved Lines on one infinite line, through the form's real widgets.
//!
//! A collinearity is a pair relation, added as one step to the one request the
//! draft is. These gates read the draft, the request and the painted text,
//! never a separate "collinear" state; the first two need neither a kernel nor
//! a solver (the plate is the sample one, the stored collinearity is written
//! through the document API). The native comparison with the peer CLI is
//! `native_collinear_form_worker_and_cli_publish_the_same_geometry`.

use super::angle::{opened_form, pair};
use super::*;
use ferritecad_document::ObjectPayload;

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

/// A U with two disjoint, parallel rims at different heights: the right one
/// (Line 1) at y 22 and the left one (Line 5) at y 20. Neither touches the
/// other and no horizontal or vertical is stored, so only a collinearity can
/// bring them onto one line.
pub(crate) const U_PROFILE: [[f64; 2]; 8] = [
    [50., 22.],
    [35., 22.],
    [35., 8.],
    [15., 8.],
    [15., 20.],
    [0., 20.],
    [0., 0.],
    [50., 0.],
];

/// The signed distance of `p` from the line through `a` and `b`, and the sine of
/// the angle between the two Lines, from solved points.
fn measure(vectors: &[[[f64; 2]; 2]], first: usize, second: usize) -> ([f64; 4], f64, [f64; 2]) {
    let [a0, a1] = vectors[first];
    let [b0, b1] = vectors[second];
    let (da, db) = (
        [a1[0] - a0[0], a1[1] - a0[1]],
        [b1[0] - b0[0], b1[1] - b0[1]],
    );
    let (la, lb) = (da[0].hypot(da[1]), db[0].hypot(db[1]));
    let side = |p: [f64; 2], from: [f64; 2], d: [f64; 2], l: f64| {
        (d[0] * (p[1] - from[1]) - d[1] * (p[0] - from[0])) / l
    };
    (
        [
            side(a0, b0, db, lb),
            side(a1, b0, db, lb),
            side(b0, a0, da, la),
            side(b1, a0, da, la),
        ],
        (da[0] * db[1] - da[1] * db[0]) / (la * lb),
        [la, lb],
    )
}

/// Native, once: the rims of a U picked in the form and Collinear added, saved
/// by the window's worker; then the same request through the peer CLI. The two
/// files must carry the same rule, the same identities and SQL, solve to the
/// same line (distances measured from the reopened file's solved points) and
/// export the same bytes; a Parallel on the same pair is the negative control
/// that leaves the rims 2 mm apart.
#[test]
fn native_collinear_form_worker_and_cli_publish_the_same_geometry() {
    use crate::creates::tests::ferritecad;
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return;
    }
    // The U carries data this editor knows nothing about (an attached source
    // claim, an unknown table and capability rows): it has to come through.
    let root = tempfile::tempdir().expect("dir");
    let source_path = root.path().join("u.fcad");
    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            &source_path,
            ferritecad_jobs::NewDocument::SketchExtrude(
                ferritecad_document::PolygonExtrusion::new(U_PROFILE.to_vec(), 10.).expect("a U"),
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
        let source = crate::sketch_facts::Source::read(&d).expect("snapshot");
        d.close().expect("close");
        source
    };
    let choice = &source.constraint_sketches[0];
    let sketch = choice.sketch;
    let saved = choice.stored.clone().expect("stored");
    let before = std::fs::read(&source_path).expect("source bytes");

    // The window: pair the rims (Lines 1 and 5) and add the collinearity.
    let (mut e, ctx) = opened_form(&source_path, &source);
    pair(&ctx, &mut e, 1, 5);
    click(&ctx, &mut e, "Add Collinear");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    click(&ctx, &mut e, "Save constraints copy…");
    let request = e.take_request().expect("widget request");
    let expected = collinear_of(saved.curves[0].id, saved.curves[4].id);
    assert_eq!(
        crate::transaction_tests::constraints(&request).add,
        vec![expected]
    );
    let ui = root.path().join("ui.fcad");
    let work = |request: EditRequest, to: &Path| {
        let mut request = request;
        request.destination = to.to_path_buf();
        let mut state = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start(request, move |r, g, c| {
                crate::edits::spawn_edit(
                    r.source,
                    r.expected,
                    r.changes,
                    r.destination,
                    c,
                    move |result| tx.send((g, result)).expect("reply"),
                )
            })
            .expect("worker");
        let (g, result) = rx
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("worker response");
        let dof = result.as_ref().ok().map(|r| {
            crate::transaction_tests::solve(r)
                .as_ref()
                .map(|s| s.degrees_of_freedom())
        });
        let mut idle = Editor::default();
        (finish_edit(&mut idle, &mut state, g, result), dof)
    };
    let (made, dof) = work(request.clone(), &ui);
    assert_eq!(made, Some(ui.clone()), "an accepted copy");
    // 16 vertex coordinates, minus the two a collinearity of disjoint Lines takes.
    assert_eq!(dof, Some(Some(14)));

    // The peer: the same spelling, as request v1, and a Parallel for contrast.
    let run = |args: &[&std::ffi::OsStr]| {
        let out = std::process::Command::new(ferritecad())
            .args(args)
            .output()
            .expect("peer CLI");
        assert!(out.status.success(), "{out:?}");
        out
    };
    let request_file = root.path().join("request.json");
    let peer = |from: &Path,
                version: &ferritecad_document::DocumentVersion,
                edits: &SketchConstraintEdits,
                to: &Path| {
        std::fs::write(
            &request_file,
            super::session_apply::peer_request(from, sketch, edits),
        )
        .expect("request");
        run(&[
            "edit".as_ref(),
            from.as_os_str(),
            "--expect-version".as_ref(),
            version.content.to_string().as_ref(),
            "--request".as_ref(),
            request_file.as_os_str(),
            "-o".as_ref(),
            to.as_os_str(),
        ]);
    };
    let cli = root.path().join("cli.fcad");
    peer(
        &source_path,
        &source.version,
        &crate::transaction_tests::constraints(&request),
        &cli,
    );
    let control = root.path().join("parallel.fcad");
    peer(
        &source_path,
        &source.version,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![AddSketchConstraint::Line(AddLineConstraint::Relation {
                a: saved.curves[0].id,
                b: saved.curves[4].id,
                relation: LineRelation::Parallel,
            })],
        },
        &control,
    );
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

    let rules = |s: &Sketch| {
        s.constraints
            .iter()
            .map(|c| c.rule.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(rules(&ui_sketch), rules(&cli_sketch));
    // Eight closure joints and the collinearity, last.
    assert_eq!(ui_sketch.constraints.len(), 9);
    for (x, y) in ui_sketch.constraints.iter().zip(&cli_sketch.constraints) {
        assert_ne!(x.id, y.id, "each publication minted its own UUID");
    }
    let SketchConstraintRule::Collinear { a, b } = ui_sketch.constraints[8].rule else {
        panic!("the collinearity comes last")
    };
    assert_eq!(
        (a.from.curve, a.to.curve, b.from.curve, b.to.curve),
        (
            saved.curves[0].id,
            saved.curves[0].id,
            saved.curves[4].id,
            saved.curves[4].id
        ),
        "the two whole Lines, as asked"
    );
    // Every SQL cell is literally the other file's, but the Sketch row's payload
    // and hash (where the new UUIDs live) and the modified timestamp; and the
    // source's rows, but for the Sketch and the new capability, are all kept.
    type Rows = Vec<Vec<rusqlite::types::Value>>;
    let (ta, tb, ts) = (
        crate::fillets::tests::tables(&ui),
        crate::fillets::tests::tables(&cli),
        crate::fillets::tests::tables(&source_path),
    );
    assert_eq!(ta.keys().collect::<Vec<_>>(), tb.keys().collect::<Vec<_>>());
    let selected = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
    let mut cells = 0;
    for (table, (columns, rows)) in &ta {
        assert_eq!(rows.len(), tb[table].1.len(), "{table}");
        for (x, y) in rows.iter().zip(&tb[table].1) {
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
    for (table, (_, rows)) in &ts {
        match table.as_str() {
            "meta" => {}

            "objects" => {
                let rest = |rows: &Rows| {
                    rows.iter()
                        .filter(|r| !r.contains(&selected))
                        .cloned()
                        .collect::<Vec<_>>()
                };
                assert_eq!(rest(&ta[table].1), rest(rows), "another object changed");
            }
            _ => assert_eq!(ta[table].1, *rows, "{table} was not preserved"),
        }
    }

    // Reopen and rebuild cold, and measure the solved Lines of each file: the
    // signed distance of each end of each rim from the line through the other,
    // the sine of the angle between them, and their lengths.
    let solved = |path: &Path| {
        let doc = Document::open_read_only(path).expect("reopen");
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let built = ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("cold rebuild after reopen");
        let report = built.solve_report(sketch).expect("solve report");
        let dof = (report.degrees_of_freedom(), report.redundant().len());
        let presentation = built.sketch_presentation(sketch).expect("presentation");
        let ends: Vec<[[f64; 2]; 2]> = presentation
            .curves()
            .iter()
            .map(|c| {
                let SketchGeometry::Line { start, end } = c.geometry() else {
                    panic!("a Line")
                };
                [[start.x, start.y], [end.x, end.y]]
            })
            .collect();
        for r in doc.topology_refs().expect("refs") {
            assert_eq!(built.resolve(&r).expect("resolves").len(), 1);
        }
        built.release_all(&mut kernel);
        doc.close().expect("close");
        (measure(&ends, 0, 4), dof)
    };
    for path in [&ui, &cli] {
        let ((distances, sine, lengths), (dof, redundant)) = solved(path);
        assert_eq!((dof, redundant), (14, 0));
        for d in distances {
            assert!(d.abs() < 1e-6, "an end is {d} mm off the other rim's line");
        }
        assert!(sine.abs() < 1e-9, "the rims still turn: sine {sine}");
        assert!(
            lengths.iter().all(|l| *l > 5.),
            "a rim collapsed: {lengths:?}"
        );
    }
    // The negative control: a Parallel is held as parallel and the rims stay
    // 2 mm apart, which is exactly what a collinearity must not allow.
    let ((distances, sine, _), (dof, _)) = solved(&control);
    assert_eq!(dof, 15);
    assert!(sine.abs() < 1e-9);
    assert!(
        distances.iter().all(|d| (d.abs() - 2.).abs() < 1e-3),
        "a Parallel moved the rims onto one line: {distances:?}"
    );

    // The exported bytes of the two files are the same bytes, and are not the
    // control's.
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
    let (control_stl, _) = bytes(&control);
    assert_eq!(
        std::fs::read(&ui_stl).expect("STL"),
        std::fs::read(&cli_stl).expect("STL"),
        "the STL of the window's copy is the command line's"
    );
    assert_eq!(
        std::fs::read(&ui_fbx).expect("FBX"),
        std::fs::read(&cli_fbx).expect("FBX")
    );
    assert_ne!(
        std::fs::read(&ui_stl).expect("STL"),
        std::fs::read(&control_stl).expect("STL"),
        "the collinear solid is not the Parallel's"
    );
    if let Some(dir) = std::env::var_os("FCAD_COLLINEAR_ARTIFACT_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifact directory");
        std::fs::copy(&ui_fbx, dir.join("collinear-ui.fbx")).expect("FBX for the pinned reader");
    }

    // Replace, then remove, from the window's own copy: the form's Remove row
    // and a Parallel on the same pair (one request, one worker), then a Remove
    // alone of that Parallel. Each answer is set against the peer CLI given the
    // same request on the same file.
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
        let source = crate::sketch_facts::Source::read(&d).expect("snapshot");
        d.close().expect("close");
        source
    };
    let from_ui = reading(&ui);
    let old_ids = ids_of(&ui);
    let stored_collinear = old_ids[8];
    let (mut e, ctx) = opened_form(&ui, &from_ui);
    click_remove_on_row(&ctx, &mut e, &format!("Collinear · {stored_collinear}"));
    pair(&ctx, &mut e, 1, 5);
    click(&ctx, &mut e, "Add Parallel");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    click(&ctx, &mut e, "Save constraints copy…");
    let replace = e.take_request().expect("widget request");
    assert_eq!(
        crate::transaction_tests::constraints(&replace).remove,
        vec![stored_collinear]
    );
    let (ui2, cli2) = (root.path().join("ui2.fcad"), root.path().join("cli2.fcad"));
    let (made, dof) = work(replace.clone(), &ui2);
    assert_eq!((made, dof), (Some(ui2.clone()), Some(Some(15))));
    peer(
        &ui,
        &from_ui.version,
        &crate::transaction_tests::constraints(&replace),
        &cli2,
    );
    let (new_ui, new_cli) = (ids_of(&ui2), ids_of(&cli2));
    assert_eq!(new_ui[..8], old_ids[..8], "the joints keep their UUIDs");
    assert_eq!(new_ui[..8], new_cli[..8]);
    assert!(!new_ui.contains(&stored_collinear) && !new_cli.contains(&stored_collinear));
    assert_eq!(
        sketch_of(&ui2).0.constraints[8].rule,
        sketch_of(&cli2).0.constraints[8].rule
    );

    for path in [&ui2, &cli2] {
        let ((distances, sine, _), (dof, _)) = solved(path);
        assert_eq!(dof, 15);
        assert!(sine.abs() < 1e-9);
        assert!(
            distances.iter().all(|d| (d.abs() - 2.).abs() < 1e-3),
            "{distances:?}"
        );
    }
    let from_ui2 = reading(&ui2);
    let (mut e, ctx) = opened_form(&ui2, &from_ui2);
    click_remove_on_row(&ctx, &mut e, &format!("Parallel · {}", new_ui[8]));
    click(&ctx, &mut e, "Save constraints copy…");
    let remove = e.take_request().expect("widget request");
    assert_eq!(
        (
            crate::transaction_tests::constraints(&remove)
                .remove
                .clone(),
            crate::transaction_tests::constraints(&remove).add.len()
        ),
        (vec![new_ui[8]], 0)
    );
    let (ui3, cli3) = (root.path().join("ui3.fcad"), root.path().join("cli3.fcad"));
    let (made, dof) = work(remove.clone(), &ui3);
    assert_eq!((made, dof), (Some(ui3.clone()), Some(Some(16))));
    peer(
        &ui2,
        &from_ui2.version,
        &crate::transaction_tests::constraints(&remove),
        &cli3,
    );
    for path in [&ui3, &cli3] {
        assert_eq!(ids_of(path), new_ui[..8].to_vec(), "only the joints remain");
        assert_eq!(solved(path).1.0, 16);
    }
}
