// SPDX-License-Identifier: MIT
use super::cli::{cli_edit, export};
use super::constraint_widgets::{click_label, press_rule};
use super::diameter::{CENTER, compare_circles, move_centers};
use super::widgets::Ui;
use super::window::{click, enter, finish, open, publish, version};
use super::*;
use crate::sessions::tests::tabs::Drawn;
use ferritecad_document::{Document, Expression};
use ferritecad_jobs::{CreateDocumentRequest, NewDocument};
use ferritecad_kernel::OperationContext;
use ferritecad_ui::ModelEdit;

fn solved(sketch: ObjectId, source: &Sketch) -> (Sketch, Option<usize>) {
    match ferritecad_eval::solve_sketch(source, sketch).expect("solve circular profile") {
        Some((drawing, report)) => (drawing, Some(report.degrees_of_freedom())),
        None => (source.clone(), None),
    }
}

fn same_model(ui: &Path, peer: &Path, sketch: ObjectId, wanted: &Sketch, freedom: usize) {
    let window = Document::open_read_only(ui).expect("window revision");
    let cli = Document::open_read_only(peer).expect("CLI output");
    assert_eq!(window.meta().document_id, cli.meta().document_id);
    assert_eq!(
        window.dependencies().expect("links"),
        cli.dependencies().expect("links")
    );
    assert_eq!(
        crate::test_names::names(&window),
        crate::test_names::names(&cli)
    );
    assert_eq!(
        window.objects().expect("objects"),
        cli.objects().expect("objects")
    );
    for d in [&window, &cli] {
        let (id, drawing) = ferritecad_fixtures::sketch(d).expect("sketch");
        assert_eq!(id, sketch);
        assert_eq!(&drawing, wanted);
        assert_eq!(solved(id, &drawing).1, Some(freedom));
    }
    window.close().expect("close");
    cli.close().expect("close");
    for format in ["stl", "fbx"] {
        assert_eq!(export(ui, format), export(peer, format));
    }
}

fn round_rules(annular: bool) {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_eval::solver_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("round.fcad");
    let template = if annular {
        NewDocument::AnnularExtrude(
            ferritecad_document::AnnularExtrusion::new([12., -7.], 10., 4., 15.).expect("ring"),
        )
    } else {
        NewDocument::CircleExtrude(
            ferritecad_document::CircleExtrusion::new([12., -7.], 10., 15.).expect("cylinder"),
        )
    };
    ferritecad_jobs::create_document_with_kernel(
        CreateDocumentRequest::new(&path, template, "test"),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
    .expect("source");
    let before = std::fs::read(&path).expect("source");
    let doc = Document::open_read_only(&path).expect("source");
    let (sketch, saved) = ferritecad_fixtures::sketch(&doc).expect("circle profile");
    doc.close().expect("close");
    let mut w = open(&path, Drawn::Native);
    let start_version = version(&w);
    w.input
        .frame(([-30., -30., -20.], [33., 32., 20.]))
        .expect("fit");
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    move_centers(&mut w, &mut ui, saved.curves.len());
    let center = point(CENTER[0], CENTER[1]);
    let at = |i: usize| SketchPointRef::new(saved.curves[i].id, SketchPointSelector::Center);
    let literal = |value| Expression::constant(value).expect("constant");
    let mut expected = Vec::new();
    if annular {
        click(&mut w, center, false);
        click(&mut w, center, true);
        press_rule(&mut ui, &mut w, "Coincident");
        expected.push(SketchConstraintRule::Coincident { a: at(0), b: at(1) });
    }
    for (curve, radius) in saved.curves.iter().zip([6.75, 2.125]) {
        let SketchGeometry::Circle { radius: old, .. } = curve.geometry else {
            panic!("circle")
        };
        click(&mut w, point(center.x, center.y + old), false);
        press_rule(&mut ui, &mut w, "Radius");
        ui.type_value(&mut w, &old.to_string(), &radius.to_string());
        ui.press(&mut w, "Set", false);
        expected.push(SketchConstraintRule::Radius {
            curve: curve.id,
            radius: literal(radius),
        });
    }
    click(&mut w, center, false);
    press_rule(&mut ui, &mut w, "Fix");
    expected.push(SketchConstraintRule::Fixed {
        point: at(0),
        x: literal(CENTER[0]),
        y: literal(CENTER[1]),
    });
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes.len(), expected.len());
    // Supply the new UUIDs from this transaction to both publishers. Derive each rule
    // and the two geometry seeds independently from the input document.
    let rules: Vec<_> = expected
        .into_iter()
        .zip(&request.changes)
        .map(|(rule, change)| {
            let EditChange::AddConstraint { constraint, .. } = change else {
                panic!("added rule")
            };
            assert_eq!(constraint.rule, rule);
            SketchConstraint {
                id: constraint.id,
                rule,
            }
        })
        .collect();
    assert_eq!(solved(sketch, &saved).1, None);
    let mut preview = saved.clone();
    for c in &mut preview.curves {
        let SketchGeometry::Circle {
            center: position, ..
        } = &mut c.geometry
        else {
            panic!("circle")
        };
        *position = center;
    }
    for rule in &rules {
        preview.constraints.push(rule.clone());
        preview = solved(sketch, &preview).0;
    }
    let persisted = Sketch {
        constraints: rules.clone(),
        ..saved.clone()
    };
    let (reference, dof) = solved(sketch, &persisted);
    assert_eq!(dof, Some(0));
    compare_circles(&reference, &preview);
    let changes: Vec<_> = rules
        .iter()
        .map(|c| EditChange::AddConstraint {
            sketch,
            constraint: c.clone(),
        })
        .collect();
    let peer = root.path().join("added.fcad");
    cli_edit(&path, &peer, start_version, &changes);
    publish(&mut w, request);
    assert!(!w.creates.sketch.viewport.active());
    let added = w.sessions.export_path().expect("accepted");
    same_model(&added, &peer, sketch, &persisted, 0);
    assert_eq!(std::fs::read(&path).expect("source"), before);
    if !annular {
        return;
    }
    let before_remove = version(&w);
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    // Select the topmost pin first when its label overlaps the centre relation.
    let concentric = rules[0].id;
    let pin = rules.last().expect("pin").id;
    let mut shown = solved(sketch, &persisted).0;
    for id in [pin, concentric] {
        click_label(&mut w, &mut ui, id);
        ui.press(&mut w, "Delete", true);
        shown.constraints.retain(|c| c.id != id);
        shown = solved(sketch, &shown).0;
    }
    let request = finish(&mut w, &mut ui);
    let mut removal: Vec<_> = [concentric, pin]
        .map(|constraint| EditChange::RemoveConstraint { sketch, constraint })
        .into();
    let only_rules = root.path().join("just-remove.fcad");
    cli_edit(&added, &only_rules, before_remove, &removal);
    let free = Sketch {
        constraints: rules[1..3].to_vec(),
        ..saved.clone()
    };
    let control = solved(sketch, &free);
    assert_eq!(control.1, Some(4));
    for c in &control.0.curves {
        let SketchGeometry::Circle { center, .. } = c.geometry else {
            panic!("circle")
        };
        assert_eq!(center, point(12., -7.));
    }
    assert_eq!(shown.constraints, free.constraints);
    for c in &shown.curves {
        removal.push(EditChange::SetCurveGeometry {
            sketch,
            curve: c.id,
            geometry: c.geometry.clone(),
        });
    }
    assert_eq!(request.changes, removal);
    let full = root.path().join("removed-from-display.fcad");
    cli_edit(&added, &full, before_remove, &removal);
    publish(&mut w, request);
    let window = w.sessions.export_path().expect("freed ring");
    same_model(&window, &full, sketch, &shown, 4);
    let doc = Document::open_read_only(&only_rules).expect("CLI removal");
    let (_, stored) = ferritecad_fixtures::sketch(&doc).expect("freed ring");
    assert_eq!(stored, free);
    doc.close().expect("close");
    assert_eq!(std::fs::read(&path).expect("source"), before);
}
#[test]
fn native_viewport_radius_and_fix_match_the_cli_cylinder() {
    round_rules(false);
}
#[test]
fn native_viewport_adds_and_removes_annulus_rules_like_the_complete_cli_edit() {
    round_rules(true);
}
