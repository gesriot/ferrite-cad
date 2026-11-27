// SPDX-License-Identifier: MIT
use super::constraint_widgets::click_label;
use super::widgets::Ui;
use super::window::{apply, enter, open, version};
use super::*;
use crate::sessions::tests::tabs::Drawn;
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, DirectedAngle, DirectedLine, Document, DocumentVersion,
    ExpressionSlot, LineEndpoint, SketchConstraintEdits,
};
use ferritecad_kernel::OperationContext;
use ferritecad_ui::ModelEdit;
use std::path::Path;

fn cli_edit(from: &Path, to: &Path, version: DocumentVersion, changes: &[EditChange]) {
    let request = to.with_extension("json");
    std::fs::write(
        &request,
        ferritecad_fixtures::edit_request_json(changes).to_string(),
    )
    .expect("write CLI request");
    let output = std::process::Command::new(crate::creates::tests::ferritecad())
        .arg("edit")
        .arg(from)
        .arg("--expect-version")
        .arg(version.content.to_string())
        .arg("--request")
        .arg(request)
        .arg("-o")
        .arg(to)
        .output()
        .expect("CLI edit");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn native_set_of_a_saved_reversed_angle_matches_the_independent_cli_edit() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_eval::solver_available(), "PLANEGCS"),
        (ferritecad_occt::is_available(), "OCCT"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let bare = root.path().join("dart.fcad");
    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            &bare,
            ferritecad_jobs::NewDocument::SketchExtrude(
                ferritecad_document::PolygonExtrusion::new(
                    vec![[0., 0.], [40., 0.], [10., 10.], [0., 40.]],
                    10.,
                )
                .expect("dart"),
            ),
            "test",
        ),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
    .expect("create dart");
    crate::sketch::drag_tests::attach_source_claim(&bare);
    let doc = Document::open_read_only(&bare).expect("document");
    let (sketch, saved) = ferritecad_fixtures::sketch(&doc).expect("source sketch");
    let direction = |index: usize, at: LineEndpoint| {
        DirectedLine::new(saved.curves[index].id, at, at.other()).expect("directed line")
    };
    let seed = ferritecad_fixtures::constraint_changes(
        &doc,
        sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![AddSketchConstraint::Line(
                AddLineConstraint::DirectedAngle {
                    a: direction(1, LineEndpoint::Start),
                    b: direction(2, LineEndpoint::End),
                    angle: DirectedAngle::from_degrees(110.).expect("angle"),
                },
            )],
        },
    )
    .expect("closure joints and reversed angle");
    let source = root.path().join("saved.fcad");
    cli_edit(
        &bare,
        &source,
        ferritecad_document::ExtrudeEditSource::read(&doc)
            .expect("facts")
            .version,
        &seed,
    );
    doc.close().expect("close");
    let before = std::fs::read(&source).expect("saved source");
    let doc = Document::open_read_only(&source).expect("seeded source");
    let (_, saved) = ferritecad_fixtures::sketch(&doc).expect("saved sketch");
    assert_eq!(saved.constraints.len(), 5);
    let id = saved.constraints[4].id;
    let v = ferritecad_document::ExtrudeEditSource::read(&doc)
        .expect("source version")
        .version;
    doc.close().expect("close");
    let to = DirectedAngle::from_degrees(130.).expect("new angle");
    let edit = EditChange::constant(
        sketch,
        ExpressionSlot::DirectedAngle { constraint: id },
        to.radians(),
    )
    .expect("edit");
    // Independent witness for the preview branch: start from the solved 110° drawing.
    let (mut preview_seed, _) = ferritecad_eval::solve_sketch(&saved, sketch)
        .expect("saved solve")
        .expect("has constraints");
    let SketchConstraintRule::DirectedAngle { angle, .. } = &mut preview_seed.constraints[4].rule
    else {
        panic!("angle");
    };
    *angle = ferritecad_document::Expression::constant(to.radians()).expect("literal");
    let (preview, report) = ferritecad_eval::solve_sketch(&preview_seed, sketch)
        .expect("updated solve")
        .expect("has constraints");
    assert_eq!(report.degrees_of_freedom(), 7);
    let mut changes: Vec<_> = preview
        .curves
        .iter()
        .map(|c| EditChange::SetCurveGeometry {
            sketch,
            curve: c.id,
            geometry: c.geometry.clone(),
        })
        .collect();
    assert_eq!(changes.len(), 4);
    changes.push(edit.clone());
    let corrected = root.path().join("full-edit.fcad");
    cli_edit(&source, &corrected, v, &changes);
    let peer = root.path().join("peer.fcad");
    cli_edit(&source, &peer, v, std::slice::from_ref(&edit));
    let old_peer = root.path().join("replace-rule.fcad");
    let doc = Document::open_read_only(&source).expect("saved document");
    let replace = ferritecad_fixtures::constraint_changes(
        &doc,
        sketch,
        &SketchConstraintEdits {
            remove: vec![id],
            add: vec![AddSketchConstraint::Line(
                AddLineConstraint::DirectedAngle {
                    a: direction(1, LineEndpoint::Start),
                    b: direction(2, LineEndpoint::End),
                    angle: to,
                },
            )],
        },
    )
    .expect("old replacement");
    doc.close().expect("close saved source");
    cli_edit(&source, &old_peer, v, &replace);
    assert_corner(&peer, sketch, -50., 7);
    assert_eq!(export(&peer, "stl"), export(&old_peer, "stl"));

    let mut w = open(&source, Drawn::Native);
    assert_eq!(version(&w), v);
    assert!(!w.sessions.can_undo());
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, id);
    ui.type_value(&mut w, "110", "130");
    ui.press(&mut w, "Set", false);
    let draft = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    let SketchConstraintRule::DirectedAngle { a, b, angle } = &draft.state.constraints[4].rule
    else {
        panic!("saved directed angle");
    };
    let SketchConstraintRule::DirectedAngle {
        a: saved_a,
        b: saved_b,
        ..
    } = &saved.constraints[4].rule
    else {
        panic!("saved directed angle");
    };
    assert_eq!((a, b, angle.value()), (saved_a, saved_b, to.radians()));
    assert_eq!(draft.state.constraints[4].id, id);
    ui.press(&mut w, "Finish", false);
    let form = &mut w.creates.sketch.viewport;
    let ask = form.take_request().unwrap_or_else(|| {
        panic!(
            "Finish refused: {:?}",
            form.draft.as_ref().map(|d| &d.message)
        )
    });
    assert_eq!(ask.expected, v);
    assert_eq!(ask.changes.last(), Some(&edit));
    assert!(form.take_request().is_none());
    assert_eq!(ask.changes, changes);
    assert!(apply(&mut w, ask));
    assert_ne!(v, version(&w));
    assert_eq!(std::fs::read(&source).expect("source bytes"), before);
    let actual = w.sessions.export_path().expect("accepted revision");
    let (_, result) = super::curve_edits::stored(&actual);
    let (_, full_peer) = super::curve_edits::stored(&corrected);
    assert_eq!(result, full_peer);
    assert_eq!(&result.constraints[..4], &saved.constraints[..4]);
    assert_eq!(result.constraints[4].id, id);
    let SketchConstraintRule::DirectedAngle { a, b, angle } = &result.constraints[4].rule else {
        panic!("stored directed angle");
    };
    assert_eq!((a, b, angle.value()), (saved_a, saved_b, to.radians()));
    assert_database(&source, &actual, &corrected, sketch);
    assert_corner(&source, sketch, -70., 7);
    let ui_volume = assert_corner(&actual, sketch, -50., 7);
    let cli_volume = assert_corner(&corrected, sketch, -50., 7);
    for format in ["stl", "fbx"] {
        let ui = export(&actual, format);
        let reference = export(&corrected, format);
        assert!(
            ui == reference,
            "{format} differs: window {} bytes, {ui_volume} mm³; CLI {} bytes, {cli_volume} mm³",
            ui.len(),
            reference.len()
        );
        if format == "fbx"
            && let Some(dir) = std::env::var_os("FCAD_DIRECTED_ANGLE_ARTIFACT_DIR")
        {
            std::fs::create_dir_all(&dir).expect("artifact directory");
            std::fs::write(Path::new(&dir).join("angle-ui.fbx"), ui).expect("pinned reader FBX");
        }
    }
    assert_ne!(export(&actual, "stl"), export(&source, "stl"));
    let accepted = version(&w);
    w.step(true);
    assert_eq!(version(&w), v);
    assert!(!w.sessions.can_undo());
    w.step(false);
    assert_eq!(version(&w), accepted);
    assert!(!w.sessions.can_redo());

    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, id);
    ui.press(&mut w, "Delete", true);
    let request = super::window::finish(&mut w, &mut ui);
    let remove = EditChange::RemoveConstraint {
        sketch,
        constraint: id,
    };
    assert_eq!(request.changes.as_slice(), std::slice::from_ref(&remove));
    let removed_cli = root.path().join("removed.fcad");
    cli_edit(&actual, &removed_cli, accepted, &[remove]);
    assert!(apply(&mut w, request));
    let removed = w.sessions.export_path().expect("accepted removal");
    for file in [&removed, &removed_cli] {
        assert_eq!(
            super::curve_edits::stored(file).1.constraints,
            result.constraints[..4]
        );
        assert_corner(file, sketch, -50., 8);
    }
    assert_database(&actual, &removed, &removed_cli, sketch);
    assert_eq!(std::fs::read(source).expect("original source"), before);
}

fn assert_corner(path: &Path, sketch: ObjectId, degrees: f64, dof: usize) -> f64 {
    let d = Document::open_read_only(path).expect("reader");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let cold =
        ferritecad_eval::rebuild_cold(&d, &mut kernel, &OperationContext::default()).expect("cold");
    let solved = cold.sketch_presentation(sketch).expect("drawing");
    let vector = |i: usize| {
        let SketchGeometry::Line { start, end } = solved.curves()[i].geometry() else {
            panic!("expected line");
        };
        [end.x - start.x, end.y - start.y]
    };
    let (a, b) = (vector(1), vector(2));
    let measured =
        DirectedAngle::from_radians((a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]))
            .expect("corner")
            .degrees();
    assert!((measured - degrees).abs() < 1e-6);
    let r = cold.solve_report(sketch).expect("report");
    assert_eq!(r.degrees_of_freedom(), dof);
    assert!(r.redundant().is_empty());
    crate::test_names::assert_names(&cold, &crate::test_names::names(&d));
    let extrude = d
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Extrude(_)))
        .expect("dart extrusion");
    let solid = cold.shape(extrude.id).expect("rebuilt dart");
    assert!(kernel.is_valid(solid).expect("validity"));
    let volume = kernel.shape_stats(solid).expect("volume").1;
    cold.release_all(&mut kernel);
    d.close().expect("close");
    volume
}

fn export(path: &Path, format: &str) -> Vec<u8> {
    let out = tempfile::tempdir().expect("export directory");
    let file = out.path().join(format!("dart.{format}"));
    let result = std::process::Command::new(crate::creates::tests::ferritecad())
        .arg(format!("export-{format}"))
        .arg(path)
        .arg("-o")
        .arg(&file)
        .output()
        .expect("CLI export");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::read(file).expect("exported bytes")
}

fn assert_database(source: &Path, window: &Path, cli: &Path, sketch: ObjectId) {
    let ui = crate::fillets::tests::tables(window);
    let peer = crate::fillets::tests::tables(cli);
    let original = crate::fillets::tests::tables(source);
    assert!(
        ui.values()
            .flat_map(|(_, rows)| rows)
            .map(Vec::len)
            .sum::<usize>()
            > 50
    );
    assert!(ui.keys().eq(peer.keys()) && ui.keys().eq(original.keys()));
    let selected = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
    for (table, (columns, rows)) in &ui {
        assert_eq!(columns, &peer[table].0);
        assert_eq!(rows.len(), peer[table].1.len(), "{table}");
        assert_eq!(rows.len(), original[table].1.len(), "{table}");
        for ((row, peer_row), source_row) in rows.iter().zip(&peer[table].1).zip(&original[table].1)
        {
            for (i, column) in columns.iter().enumerate() {
                if table == "meta" && column == "modified_at" {
                    continue;
                }
                assert_eq!(row[i], peer_row[i], "{table}.{column} against CLI");
                if table != "objects"
                    || !row.contains(&selected)
                    || !matches!(column.as_str(), "payload" | "payload_hash")
                {
                    assert_eq!(row[i], source_row[i], "{table}.{column} against source");
                }
            }
        }
    }
}
