// SPDX-License-Identifier: MIT
use super::cli::{cli_edit, export};
use super::constraint_widgets::{click_label, press_rule};
use super::widgets::Ui;
use super::window::{click, enter, finish, open, publish, version};
use super::*;
use crate::feature_tests::U_PROFILE;
use crate::sessions::tests::tabs::{Drawn, Window};
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, Document, ExtrudeEditSource, LineRelation,
    ObjectPayload, SketchConstraintEdits,
};
use ferritecad_kernel::OperationContext;
use ferritecad_ui::ModelEdit;
use std::path::Path;

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

fn pick_rims(w: &mut Window) {
    let draft = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    let midpoints = [0, 4].map(|i| {
        let SketchGeometry::Line { start, end } = draft.state.curves[i].geometry else {
            panic!("line");
        };
        point((start.x + end.x) / 2., (start.y + end.y) / 2.)
    });
    click(w, midpoints[0], false);
    click(w, midpoints[1], true);
}

/// Build the CLI edit using only the stored sketch and the public solver.
/// Removing one rule and adding another are separate preview solves in the window.
fn edit_from_display(
    sketch: ObjectId,
    saved: &Sketch,
    parallel: Option<&SketchConstraint>,
) -> Vec<EditChange> {
    let solve = |input: &Sketch| {
        ferritecad_eval::solve_sketch(input, sketch)
            .expect("solve U")
            .expect("U has constraints")
            .0
    };
    let mut shown = solve(saved);
    let old = shown.constraints.pop().expect("stored collinearity");

    let mut shown = solve(&shown);
    if let Some(rule) = parallel {
        shown.constraints.push(rule.clone());
        shown = solve(&shown);
    }
    let mut original_seed = saved.clone();
    original_seed.constraints = shown.constraints.clone();
    let prediction = solve(&original_seed);
    let mut changes = vec![EditChange::RemoveConstraint {
        sketch,
        constraint: old.id,
    }];
    for (predicted, target) in prediction.curves.iter().zip(&shown.curves) {
        assert_eq!(predicted.id, target.id);
        let (SketchGeometry::Line { start: a, end: b }, SketchGeometry::Line { start: c, end: d }) =
            (&predicted.geometry, &target.geometry)
        else {
            panic!("expected two U segments")
        };
        if [a.x - c.x, a.y - c.y, b.x - d.x, b.y - d.y]
            .iter()
            .any(|v| v.abs() > 1e-6)
        {
            changes.push(EditChange::SetCurveGeometry {
                sketch,
                curve: target.id,
                geometry: target.geometry.clone(),
            });
        }
    }
    if let Some(rule) = parallel {
        changes.push(EditChange::AddConstraint {
            sketch,
            constraint: rule.clone(),
        });
    }
    changes
}

#[test]
fn native_collinear_viewport_and_cli_publish_the_same_geometry() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_eval::solver_available(), "PLANEGCS"),
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
    let doc = Document::open_read_only(&source_path).expect("source");
    let (sketch, saved) = ferritecad_fixtures::sketch(&doc).expect("sketch");
    let before = std::fs::read(&source_path).expect("original bytes");
    let relation = |kind: LineRelation| {
        AddSketchConstraint::Line(AddLineConstraint::Relation {
            a: saved.curves[0].id,
            b: saved.curves[4].id,
            relation: kind,
        })
    };
    let peer =
        |from: &Path, to: &Path, remove: Vec<StableEntityId>, add: Vec<AddSketchConstraint>| {
            let source = Document::open_read_only(from).expect("peer source");
            let changes = ferritecad_fixtures::constraint_changes(
                &source,
                sketch,
                &SketchConstraintEdits { remove, add },
            )
            .expect("peer changes");
            let v = ExtrudeEditSource::read(&source).expect("version").version;
            source.close().expect("close");
            cli_edit(from, to, v, &changes);
        };
    doc.close().expect("close");
    let mut w = open(&source_path, Drawn::Native);
    w.input
        .frame(([-10., -10., -20.], [60., 32., 20.]))
        .expect("U in view");
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut controls = Ui::new(&mut w);
    // End of each line, then start of its neighbour, including the closing pair.
    for i in 0..8 {
        let [x, y] = U_PROFILE[(i + 1) % 8];
        click(&mut w, point(x, y), false);
        click(&mut w, point(x, y), true);
        if i == 7 {
            // The first line's start is picked before the last line's end at this corner.
            click(&mut w, point(x, y), true);
            click(&mut w, point(x, y), true);
        }
        press_rule(&mut controls, &mut w, "Coincident");
    }
    pick_rims(&mut w);
    press_rule(&mut controls, &mut w, "Collinear");
    let request = finish(&mut w, &mut controls);
    assert_eq!(request.changes.len(), 9);
    assert!(
        request
            .changes
            .iter()
            .all(|c| matches!(c, EditChange::AddConstraint { .. }))
    );
    publish(&mut w, request);
    let ui = w.sessions.export_path().expect("accepted collinearity");
    let cli = root.path().join("collinear.fcad");
    peer(
        &source_path,
        &cli,
        vec![],
        vec![relation(LineRelation::Collinear)],
    );
    let control = root.path().join("parallel.fcad");
    peer(
        &source_path,
        &control,
        vec![],
        vec![relation(LineRelation::Parallel)],
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
            crate::test_names::names(&d),
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
        crate::test_names::assert_names(&built, &crate::test_names::names(&doc));
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

    let ui_stl = export(&ui, "stl");
    assert_eq!(ui_stl, export(&cli, "stl"));
    assert_ne!(ui_stl, export(&control, "stl"));
    let ui_fbx = export(&ui, "fbx");
    assert_eq!(ui_fbx, export(&cli, "fbx"));
    if let Some(dir) = std::env::var_os("FCAD_COLLINEAR_ARTIFACT_DIR") {
        std::fs::create_dir_all(&dir).expect("reader directory");
        std::fs::write(Path::new(&dir).join("collinear-ui.fbx"), ui_fbx).expect("reader input");
    }

    let old_ids: Vec<_> = ui_sketch.constraints.iter().map(|c| c.id).collect();
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut controls = Ui::new(&mut w);
    click_label(&mut w, &mut controls, old_ids[8]);
    controls.press(&mut w, "Delete", true);
    pick_rims(&mut w);
    press_rule(&mut controls, &mut w, "Parallel");
    let replacement = finish(&mut w, &mut controls);
    assert_eq!(version(&w), replacement.expected);
    assert_eq!(
        replacement
            .changes
            .iter()
            .filter(|change| matches!(change, EditChange::SetCurveGeometry { .. }))
            .count(),
        6
    );
    publish(&mut w, replacement);
    let ui2 = w.sessions.export_path().expect("accepted replacement");
    let cli2 = root.path().join("replacement.fcad");
    peer(
        &ui,
        &cli2,
        vec![old_ids[8]],
        vec![relation(LineRelation::Parallel)],
    );
    let (ui_rules, cli_rules) = (
        sketch_of(&ui2).0.constraints,
        sketch_of(&cli2).0.constraints,
    );
    assert_eq!(ui_rules[..8], ui_sketch.constraints[..8]);
    assert_eq!(ui_rules[..8], cli_rules[..8]);
    assert!(
        ui_rules
            .iter()
            .chain(&cli_rules)
            .all(|c| c.id != old_ids[8])
    );
    assert_eq!(ui_rules[8].rule, cli_rules[8].rule);
    let full_cli = root.path().join("parallel-from-shown.fcad");
    let doc = Document::open_read_only(&ui).expect("accepted source");
    let v = ExtrudeEditSource::read(&doc).expect("version").version;
    doc.close().expect("close");
    let full = edit_from_display(sketch, &ui_sketch, Some(&cli_rules[8]));
    assert_eq!(full.len(), 8, "remove, six geometry writes and add");
    cli_edit(&ui, &full_cli, v, &full);
    let (full_sketch, _) = sketch_of(&full_cli);
    assert_eq!(full_sketch.curves, sketch_of(&ui2).0.curves);
    assert_eq!(full_sketch.constraints[..8], ui_rules[..8]);
    assert_eq!(full_sketch.constraints[8].rule, ui_rules[8].rule);
    let measured = solved(&ui2);
    assert_eq!(measured, solved(&full_cli));
    let ((distances, sine, _), freedom) = measured;
    assert_eq!(freedom, (15, 0));
    assert!(distances.iter().all(|d| d.abs() < 1e-6));
    assert!(sine.abs() < 1e-9);
    for format in ["stl", "fbx"] {
        assert_eq!(export(&ui2, format), export(&full_cli, format));
    }
    let ((distances, sine, _), (dof, redundant)) = solved(&cli2);
    assert_eq!((dof, redundant), (15, 0));
    assert!(sine.abs() < 1e-9);
    assert!(
        distances.iter().all(|d| (d.abs() - 2.).abs() < 1e-3),
        "{distances:?}"
    );
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut controls = Ui::new(&mut w);
    click_label(&mut w, &mut controls, ui_rules[8].id);
    controls.press(&mut w, "Delete", true);
    let remove = finish(&mut w, &mut controls);
    assert_eq!(
        remove.changes,
        [EditChange::RemoveConstraint {
            sketch,
            constraint: ui_rules[8].id
        }]
    );
    publish(&mut w, remove);
    let ui3 = w.sessions.export_path().expect("accepted removal");
    let cli3 = root.path().join("removed.fcad");
    let (saved, _) = sketch_of(&ui2);
    let full = edit_from_display(sketch, &saved, None);
    assert_eq!(
        full,
        [EditChange::RemoveConstraint {
            sketch,
            constraint: ui_rules[8].id
        }]
    );
    let doc = Document::open_read_only(&ui2).expect("replacement");
    let v = ExtrudeEditSource::read(&doc).expect("version").version;
    doc.close().expect("close");
    cli_edit(&ui2, &cli3, v, &full);
    for format in ["stl", "fbx"] {
        assert_eq!(export(&ui3, format), export(&cli3, format));
    }
    for path in [&ui3, &cli3] {
        assert_eq!(sketch_of(path).0.constraints, ui_rules[..8]);
        assert_eq!(solved(path).1.0, 16);
    }
}
