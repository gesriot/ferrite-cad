// SPDX-License-Identifier: MIT
//! Select a saved rule on the rebuilt drawing and read the resulting document and mesh.
use super::cli::cli_edit;
use super::constraint_widgets::click_label;
use super::curve_edits::stored;
use super::polygon_rules::solve;
use super::saved_radius::check_rim;
use super::widgets::{Ui, text};
use super::window::{enter, finish, open, version};
use super::*;
use crate::sessions::tests::tabs::Drawn;
use crate::sketch_facts::Source;
use ferritecad_ui::ModelEdit;
use std::{cell::Cell, path::Path};

type Removal = (StableEntityId, StableEntityId, usize);

pub(crate) fn unpin_circle(
    path: &Path,
    source: &Source,
    id: ObjectId,
    seen: &Cell<Option<Removal>>,
) -> (crate::sketch::Editor, EditRequest, Vec<EditChange>) {
    let (sketch, original) = stored(path);
    assert_eq!(id, sketch);
    let circle = |x, r| SketchGeometry::Circle {
        center: point(x, 0.),
        radius: r,
    };
    assert_eq!(original.curves[0].geometry, circle(4.5, 1.2));
    let mut shown = original;
    let freedom = solve(id, &mut shown);
    assert_eq!(freedom, 0);
    assert_eq!(shown.curves[0].geometry, circle(0., 10.));
    let pin = shown
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::Fixed { .. }))
        .expect("Fixed")
        .id;
    let radius = shown
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::Radius { .. }))
        .expect("Radius")
        .id;
    seen.set(Some((pin, radius, freedom)));
    let mut w = open(path, Drawn::Native);
    assert_eq!(version(&w), source.version);
    enter(&mut w, id, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    let editor = &mut w.creates.sketch.viewport;
    let words = text(&ui.paint(editor, w.input.camera(), w.input.region()));
    assert!(words.iter().any(|s| s == "Fix"));
    assert!(words.iter().any(|s| s == "R 10 mm"));
    click_label(&mut w, &mut ui, pin);
    ui.press(&mut w, "Delete", true);
    shown.constraints.retain(|c| c.id != pin);
    assert_eq!(solve(id, &mut shown), freedom + 2);
    let draft = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    assert_eq!(draft.state.curves, shown.curves);
    assert_eq!(draft.state.constraints, shown.constraints);
    let mut changes = vec![EditChange::RemoveConstraint {
        sketch: id,
        constraint: pin,
    }];
    let root = tempfile::tempdir().expect("rules-only CLI control");
    let only = root.path().join("free.fcad");
    cli_edit(path, &only, source.version, &changes);
    let mut control = stored(&only).1;
    assert_eq!(solve(id, &mut control), freedom + 2);
    assert_eq!(control.curves[0].geometry, circle(4.5, 10.));
    changes.push(EditChange::SetCurveGeometry {
        sketch: id,
        curve: shown.curves[0].id,
        geometry: shown.curves[0].geometry.clone(),
    });
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, changes);
    (w.creates.sketch, request, changes)
}

pub(crate) fn saved_unpinned_circle(saved: &Path, stl: &[u8], height: f64, was: Removal) {
    let (pin, radius_id, previous_dof) = was;
    let (id, mut drawing) = stored(saved);
    assert_eq!(solve(id, &mut drawing), previous_dof + 2);
    assert!(!drawing.constraints.iter().any(|c| c.id == pin));
    assert_eq!(drawing.constraints.len(), 1);
    assert_eq!(drawing.constraints[0].id, radius_id);
    let mut w = open(saved, Drawn::Native);
    let source = w.scene.edit_source.as_ref().expect("saved source");
    let presentation = w
        .scene
        .sketches
        .presentation(source, id)
        .expect("cold drawing");
    let [curve] = presentation.curves() else {
        panic!("one saved curve")
    };
    let SketchGeometry::Circle { center, radius } = *curve.geometry() else {
        panic!("circle")
    };
    assert!(center.x.abs() < 1e-6 && center.y.abs() < 1e-6 && (radius - 10.).abs() < 1e-6);
    enter(&mut w, id, ModelEdit::Sketch);
    let ui = Ui::new(&mut w);
    let editor = &mut w.creates.sketch.viewport;
    let words = text(&ui.paint(editor, w.input.camera(), w.input.region()));
    assert_eq!(
        words
            .iter()
            .filter(|s| s.starts_with("R "))
            .collect::<Vec<_>>(),
        ["R 10 mm"]
    );
    assert!(words.iter().all(|s| s != "Fix"));
    assert_eq!(
        editor.labels.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [radius_id]
    );
    check_rim(stl, height, center, radius);
}
