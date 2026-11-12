// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{
    EndCondition, Expression, FeatureAddition, ObjectPayload, SolidOperation,
};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use std::sync::Arc;

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
}

fn cold(path: &std::path::Path) -> ferritecad_scene::LoadedScene {
    ferritecad_scene::snapshot_of(
        path,
        &mut OcctKernel::new().expect("OCCT"),
        OcctKernel::import_step,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("cold reading")
}

fn renderer() -> Option<ferritecad_viewport_gpu::Renderer> {
    match ferritecad_viewport_gpu::Renderer::new() {
        Ok(renderer) => Some(renderer),
        Err(error) if error.kind() == ferritecad_types::ErrorKind::Unsupported => {
            ferritecad_fixtures::native_dependencies(&[(false, "GPU")]);
            None
        }
        Err(error) => panic!("GPU: {error}"),
    }
}

fn draw(renderer: &mut ferritecad_viewport_gpu::Renderer, scene: &ferritecad_scene::LoadedScene) {
    let snapshot = Arc::new(scene.snapshot.clone());
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("upload");
    let mut camera = ferritecad_viewport::Camera::new();
    camera.resize(160, 160);
    camera
        .frame(snapshot.bounds().expect("solid bounds"))
        .expect("frame");
    let frame = renderer
        .render(
            &prepared,
            &camera,
            crate::Marked::Nothing,
            crate::Hovered::Nothing,
            &crate::Visibility::new(&snapshot),
        )
        .expect("render accepted geometry");
    let pick = snapshot.pick_of(0).expect("body");
    assert!((0..frame.height()).any(|y| (0..frame.width()).any(|x| frame.pick_at(x, y) == pick)));
}

#[test]
fn native_parameter_widgets_apply_values_and_undo_redo_exact_versions_and_cold_scenes() {
    if !native() {
        return;
    }
    let Some(mut renderer) = renderer() else {
        return;
    };
    let f = fixture();
    let mut w = open(&f, Drawn::Native);
    let original_file =
        std::fs::read(w.sessions.export_path().expect("working file")).expect("bytes");
    let original = cold(&w.sessions.export_path().expect("working file"));
    let mut ui = widgets::Ui::new(&mut w);
    ui.type_field(&mut w, "Parameter expression", "20 mm");
    let asked = ui
        .press(&mut w, "Apply parameter", true)
        .parameter
        .expect("real Apply");
    assert_eq!(
        asked.change,
        EditChange::SetParameter {
            object: f.stock,
            expression: "20 mm".into()
        }
    );
    let mut changed = None;
    assert!(apply_observing(&mut w, asked, |scene| {
        assert!((scene.snapshot.bounds().expect("bounds").1[2] - 23.).abs() < 1e-5);
        draw(&mut renderer, scene);
        changed = Some(scene.clone());
    }));
    let changed = changed.expect("candidate inspected before publication");
    let changed_file =
        std::fs::read(w.sessions.export_path().expect("working file")).expect("bytes");
    let source = w.scene.edit_source.as_ref().expect("current source");
    assert_eq!(
        parameter(source, f.total)
            .expect("total")
            .expression
            .value(),
        23.
    );
    assert!(w.sessions.dirty());
    assert!(ui.press(&mut w, "Close", true).parameter.is_none());
    assert!(!w.creates.sketch.parameters.active());
    for (undo, expected, bytes) in [
        (true, &original, &original_file),
        (false, &changed, &changed_file),
    ] {
        let (address, path) = w.sessions.begin_move(undo).expect("history step");
        accept_observing(&mut w, address, path, |scene| {
            assert_eq!(scene, expected, "Undo/Redo restores the same scene");
            draw(&mut renderer, scene);
        });
        assert_eq!(
            &std::fs::read(w.sessions.export_path().expect("working")).expect("bytes"),
            bytes
        );
        assert_eq!(w.sessions.dirty(), !undo);
    }
    w.creates
        .sketch
        .parameters
        .begin(w.scene.edit_source.as_ref().expect("source"));
    select(&mut w, f.feature);
    ui.type_field(&mut w, "23 mm", "stock + 3 mm");
    let asked = ui
        .press(&mut w, "Apply value", true)
        .parameter
        .expect("Values Apply");
    assert_eq!(
        asked.change,
        EditChange::SetExpressionText {
            object: f.feature,
            slot: ExpressionSlot::ExtrudeDistance,
            source: "stock + 3 mm".into(),
        }
    );
    assert!(apply_observing(&mut w, asked, |scene| draw(
        &mut renderer,
        scene
    )));
    let source = w.scene.edit_source.as_ref().expect("facts");
    assert_eq!(
        source
            .expression_slot(f.feature, ExpressionSlot::ExtrudeDistance)
            .expect("slot")
            .refusal
            .as_deref(),
        Some(crate::numeric::EXPRESSION_REFUSAL)
    );
    let asked = request(
        &w,
        Field::Slot(f.feature, ExpressionSlot::ExtrudeDistance),
        EditChange::SetExpressionText {
            object: f.feature,
            slot: ExpressionSlot::ExtrudeDistance,
            source: "24 mm".into(),
        },
    );
    assert!(apply(&mut w, asked));
    assert!(
        w.scene
            .edit_source
            .as_ref()
            .expect("facts")
            .expression_slot(f.feature, ExpressionSlot::ExtrudeDistance)
            .expect("slot")
            .refusal
            .is_none()
    );
}

#[test]
fn native_parameter_owner_rebuilds_only_its_body_and_preserves_the_other_body() {
    if !native() {
        return;
    }
    let f = fixture();
    let mut document = Document::open(&f.path).expect("document");
    let ObjectPayload::Extrude(extrude) = document
        .object(f.feature)
        .expect("object")
        .expect("feature")
        .payload
    else {
        panic!("extrusion")
    };
    let other = ObjectId::new();
    let other_body = ObjectId::new();
    document
        .write_changes(&[EditChange::AddBody {
            object: other_body,
            name: Some("B".into()),
            feature_object: other,
            feature_name: Some("Independent extrusion".into()),
            feature: FeatureAddition::Extrude {
                profile: extrude.profile,
                regions: extrude.regions,
                reversed: false,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(5.).expect("literal"),
                },
                operation: SolidOperation::NewBody,
            },
        }])
        .expect("independent body");
    document.close().expect("close");
    let mut w = Window::new(Drawn::Native, None);
    let (scene, tab, probe) =
        sessions::test_open::native_counted(w.private.path(), &f.path).expect("counted owner");
    let other_index = scene
        .catalogue
        .iter()
        .position(
            |entry| matches!(entry.item, ferritecad_scene::SceneItem::Body(id) if id == other_body),
        )
        .expect("Body B");
    let original_b = scene.snapshot.meshes()[other_index].clone();
    let path = tab.current().path().to_path_buf();
    w.present(
        &path,
        Ok(scene),
        sessions::Bind::Open(Box::new(tab)),
        Ok(()),
    )
    .expect("present");
    w.creates
        .sketch
        .parameters
        .begin(w.scene.edit_source.as_ref().expect("source"));
    select(&mut w, f.stock);
    let before = probe.lock().expect("counts").calls;
    let asked = request(
        &w,
        Field::Text(f.stock),
        EditChange::SetParameter {
            object: f.stock,
            expression: "21 mm".into(),
        },
    );
    assert!(apply_observing(&mut w, asked, |scene| {
        let index = scene.catalogue.iter().position(|entry|
            matches!(entry.item, ferritecad_scene::SceneItem::Body(id) if id == other_body))
            .expect("Body B survives");
        assert_eq!(scene.snapshot.meshes()[index], original_b);
        assert_eq!(scene.catalogue.len(), 2);
        let counts = probe.lock().expect("counts").calls;
        assert_eq!(
            (
                counts.extrude_count - before.extrude_count,
                counts.boolean - before.boolean,
                counts.tessellate - before.tessellate
            ),
            (1, 0, 1)
        );
        assert_eq!(counts.revolve_count, before.revolve_count);
        assert_eq!(counts.fillet, before.fillet);
        assert_eq!(counts.chamfer, before.chamfer);
        eprintln!(
            "PARAMETER_OWNER extrusion=1 boolean=0 tessellation=1; independent Body B reused"
        );
    }));
    let source = w.scene.edit_source.as_ref().expect("source");
    assert_eq!(
        source
            .expression_slot(other, ExpressionSlot::ExtrudeDistance)
            .expect("unchanged B")
            .expression
            .value(),
        5.
    );
}

#[test]
fn native_values_edit_the_stored_revolve_angle_from_an_angle_parameter() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("sector.fcad");
    let revolve = crate::constraints::tests::revolve::write_sector(
        &path,
        crate::constraints::tests::revolve::SECTOR_P.as_slice(),
        45.,
    );
    let mut document = Document::open(&path).expect("document");
    document
        .write_changes(&[EditChange::AddParameter {
            object: ObjectId::new(),
            name: "turn".into(),
            dimension: Dimension::Angle,
            expression: "30 deg".into(),
        }])
        .expect("angle");
    document.close().expect("close");
    let mut w = Window::new(Drawn::Native, None);
    w.open(&path);
    w.creates
        .sketch
        .parameters
        .begin(w.scene.edit_source.as_ref().expect("source"));
    select(&mut w, revolve);
    let asked = request(
        &w,
        Field::Slot(revolve, ExpressionSlot::RevolveAngle),
        EditChange::SetExpressionText {
            object: revolve,
            slot: ExpressionSlot::RevolveAngle,
            source: "turn".into(),
        },
    );
    assert!(apply(&mut w, asked));
    let source = w.scene.edit_source.as_ref().expect("source");
    let slot = source
        .expression_slot(revolve, ExpressionSlot::RevolveAngle)
        .expect("angle");
    assert_eq!(words::value(slot.expression.value(), slot.dimension), "30°");
    assert_eq!(slot.expression.source, "turn");
}

#[test]
fn native_solver_values_edit_a_distance_constraint_and_publish_its_new_solution() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("dimension.fcad");
    crate::constraints::tests::dimension::write_dimensioned_rectangle(&path);
    let mut document = Document::open(&path).expect("document");
    document
        .write_changes(&[EditChange::AddParameter {
            object: ObjectId::new(),
            name: "span".into(),
            dimension: Dimension::Length,
            expression: "75 mm".into(),
        }])
        .expect("parameter");
    document.close().expect("close");
    let mut w = Window::new(Drawn::Native, None);
    w.open(&path);
    let source = w.scene.edit_source.as_ref().expect("source");
    let slot = source
        .expression_slots
        .iter()
        .find(|s| matches!(s.slot, ExpressionSlot::Distance { .. }) && s.expression.value() == 60.)
        .expect("60 mm Distance");
    let (sketch, slot) = (slot.object, slot.slot);
    let ExpressionSlot::Distance { constraint } = slot else {
        panic!("Distance")
    };
    let words = words::slot(source, sketch, slot);
    assert!(words.contains("Distance") && words.contains(&constraint.to_string()));
    w.creates.sketch.parameters.begin(source);
    select(&mut w, sketch);
    let asked = request(
        &w,
        Field::Slot(sketch, slot),
        EditChange::SetExpressionText {
            object: sketch,
            slot,
            source: "span".into(),
        },
    );
    assert!(apply_observing(&mut w, asked, |scene| {
        assert!(scene.rebuild.all_built());
        let (min, max) = scene.snapshot.bounds().expect("solved extrusion");
        assert!((max[0] - min[0] - 75.).abs() < 1e-4);
        assert!((max[1] - min[1] - 30.).abs() < 1e-4);
        assert!(
            scene
                .sketch_solves
                .iter()
                .any(|facts| facts.sketch() == sketch)
        );
    }));
    assert_eq!(
        w.scene
            .edit_source
            .as_ref()
            .expect("source")
            .expression_slot(sketch, slot)
            .expect("same constraint UUID")
            .expression
            .value(),
        75.
    );
}

#[test]
fn native_failed_parameter_consumers_show_exact_tree_statuses_and_repair_through_widgets() {
    if !native() {
        return;
    }
    let f = fixture();
    let original = cold(&f.path);
    let original_tree = original
        .edit_source
        .as_ref()
        .expect("original source")
        .model_tree
        .clone();
    let document = Document::open_read_only(&f.path).expect("document");
    let ObjectPayload::Parameter(mut stored) = document
        .object(f.stock)
        .expect("object")
        .expect("stock")
        .payload
    else {
        panic!("parameter")
    };
    document.close().expect("close");
    stored.expression = Expression::new("1 mm / 0", 10.).expect("stored stale value");
    let bytes = ferritecad_document::Envelope::encode("parameter", &stored)
        .expect("envelope")
        .to_bytes()
        .expect("bytes");
    let db = rusqlite::Connection::open(&f.path).expect("SQL");
    db.execute(
        "UPDATE objects SET payload=?1,payload_hash=?2 WHERE id=?3",
        rusqlite::params![
            &bytes,
            ferritecad_types::ContentHash::of_bytes(&bytes)
                .as_bytes()
                .as_slice(),
            f.stock.to_bytes().as_slice(),
        ],
    )
    .expect("repairable saved failure");
    drop(db);
    let mut w = open(&f, Drawn::Native);
    let source = w
        .scene
        .edit_source
        .as_ref()
        .expect("accepted failure facts");
    let stock = format!(
        concat!(
            "Failed: invalid input: object {} parameter 'stock' expression: ",
            "invalid input: division by zero"
        ),
        f.stock
    );
    let total = "Failed: invalid input: depends on parameter stock, which failed";
    let feature = "Failed: invalid input: depends on parameter total, which failed";
    let body = source
        .objects
        .iter()
        .find(|object| matches!(object.payload, ObjectPayload::Body(_)))
        .expect("body")
        .id;
    for (id, expected) in [
        (f.stock, stock.as_str()),
        (f.total, total),
        (f.feature, feature),
        (body, "Rolled back to no built feature because of same"),
    ] {
        assert_eq!(
            source
                .unbuilt_statuses
                .iter()
                .find(|(object, _)| *object == id)
                .expect("report status")
                .1,
            expected
        );
        for row in source.model_tree.rows.iter().filter(|row| row.id.0 == id) {
            let saved = original_tree
                .rows
                .iter()
                .find(|saved| saved.id == row.id)
                .expect("row");
            assert_eq!(row.name, format!("⚠ {}", saved.name));
            assert_eq!(row.note, expected);
        }
    }
    let mut ui = widgets::Ui::new(&mut w);
    let output = ui.settle(&mut w);
    let labels = widgets::form_text(&output);
    assert_eq!(
        labels.iter().filter(|label| **label == stock).count(),
        1,
        "only the table shows the selected failed parameter's complete tree sentence"
    );
    assert_eq!(labels.iter().filter(|label| **label == total).count(), 1);
    assert_eq!(
        labels
            .iter()
            .filter(|label| *label == "10 mm" || *label == "13 mm")
            .count(),
        0
    );
    select(&mut w, f.feature);
    let output = ui.settle(&mut w);
    let values = widgets::values_text(&output);
    assert_eq!(values.iter().filter(|label| *label == feature).count(), 1);
    assert_eq!(values.iter().filter(|label| *label == "total").count(), 1);
    assert_eq!(values.iter().filter(|label| *label == "13 mm").count(), 0);

    select(&mut w, f.stock);
    ui.type_field(&mut w, "Parameter expression", "10 mm");
    assert_eq!(w.scene.edit_source.as_ref().expect("source").refusal, None);
    let asked = ui
        .press(&mut w, "Apply parameter", true)
        .parameter
        .expect("repair Apply");
    let accepted = apply_observing(&mut w, asked, |scene| {
        assert_eq!(
            scene.edit_source.as_ref().expect("source").model_tree,
            original_tree
        );
    });
    assert!(accepted, "repair refusal: {}", w.sessions.status);
    let labels = widgets::form_text(&ui.settle(&mut w));
    assert_eq!(
        labels
            .iter()
            .find(|label| *label == "10 mm")
            .map(String::as_str),
        Some("10 mm")
    );
    assert_eq!(
        labels
            .iter()
            .find(|label| *label == "13 mm")
            .map(String::as_str),
        Some("13 mm")
    );
    assert_eq!(
        w.scene
            .edit_source
            .as_ref()
            .expect("source")
            .unbuilt_statuses,
        vec![]
    );
}
