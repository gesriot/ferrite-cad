// SPDX-License-Identifier: MIT
use super::cli::{cli_edit, export};
use super::constraint_widgets::click_label;
use super::curve_edits::stored;
use super::polygon_rules::solve;
use super::widgets::Ui;
use super::window::{enter, open, publish, version};
use super::*;
use crate::sessions::tests::tabs::Drawn;
use ferritecad_document::ExpressionSlot;
use ferritecad_ui::ModelEdit;

fn maximum_coordinate_difference(actual: &Sketch, shown: &Sketch) -> f64 {
    assert_eq!(actual.curves.len(), shown.curves.len());
    let mut error: f64 = 0.;
    for (a, b) in actual.curves.iter().zip(&shown.curves) {
        let coordinates = |c: &SketchCurve| match c.geometry {
            SketchGeometry::Line { start, end } => [start.x, start.y, end.x, end.y],
            _ => panic!("line fixture"),
        };
        for (x, y) in coordinates(a).into_iter().zip(coordinates(b)) {
            error = error.max((x - y).abs());
        }
    }
    error
}

#[test]
fn removing_horizontal_finishes_the_marked_plate_as_displayed() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_eval::solver_available(), "PLANEGCS"),
        (ferritecad_occt::is_available(), "OCCT"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("fixture directory");
    let initial = root.path().join("marked.fcad");
    crate::feature_tests::write_marked_rectangle(&initial);
    let facts = crate::sketch_facts::read(&initial).expect("source");
    let height = ferritecad_fixtures::expression_slot(&facts, ExpressionSlot::ExtrudeDistance);
    let set =
        EditChange::constant(height.object, ExpressionSlot::ExtrudeDistance, 9.5).expect("height");
    let path = root.path().join("height.fcad");
    cli_edit(&initial, &path, facts.version, &[set]);
    let (id, original) = stored(&path);
    let horizontal = original
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::Horizontal { .. }))
        .expect("saved Horizontal")
        .id;

    let mut shown = original;
    assert_eq!(solve(id, &mut shown), 0);
    shown.constraints.retain(|c| c.id != horizontal);
    assert_eq!(solve(id, &mut shown), 1);
    let mut w = open(&path, Drawn::Native);
    let base = version(&w);
    enter(&mut w, id, ModelEdit::Sketch);
    w.input
        .frame(([-20., -20., -1.], [80., 50., 1.]))
        .expect("fit");
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, horizontal);
    ui.press(&mut w, "Delete", true);
    let draft = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft");
    assert_eq!(draft.state.curves, shown.curves);
    assert_eq!(draft.state.constraints, shown.constraints);
    let mut changes = vec![EditChange::RemoveConstraint {
        sketch: id,
        constraint: horizontal,
    }];
    let only = root.path().join("rules-only.fcad");
    cli_edit(&path, &only, base, &changes);
    let mut control = stored(&only).1;
    assert_eq!(solve(id, &mut control), 1);
    let error = maximum_coordinate_difference(&control, &shown);
    assert!(
        (14.2..14.3).contains(&error),
        "rules-only offset {error} mm"
    );
    assert_eq!(shown.curves.len(), 4);
    changes.extend(shown.curves.iter().map(|c| EditChange::SetCurveGeometry {
        sketch: id,
        curve: c.id,
        geometry: c.geometry.clone(),
    }));
    let peer = root.path().join("full.fcad");
    cli_edit(&path, &peer, base, &changes);
    ui.press(&mut w, "Finish", false);
    let editor = &mut w.creates.sketch.viewport;
    let request = editor.take_request().unwrap_or_else(|| {
        panic!(
            "Finish: {}",
            editor.draft.as_ref().expect("open draft").message
        )
    });
    assert_eq!(request.changes, changes);
    publish(&mut w, request);
    let result = w.sessions.export_path().expect("accepted");
    let (accepted_id, mut accepted) = stored(&result);
    assert_eq!(accepted_id, id);
    assert_eq!(accepted, shown);
    assert_eq!(stored(&peer), (id, shown.clone()));
    assert_eq!(solve(id, &mut accepted), 1);
    let offset = maximum_coordinate_difference(&accepted, &shown);
    assert!(offset <= 1e-6, "accepted drawing moved {offset} mm");
    for kind in ["stl", "fbx"] {
        assert_eq!(export(&result, kind), export(&peer, kind), "{kind}");
    }
}
