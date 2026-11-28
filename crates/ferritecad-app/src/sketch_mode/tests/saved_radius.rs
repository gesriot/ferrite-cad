// SPDX-License-Identifier: MIT
//! Edit the saved radius of the dimension-label cylinder through its displayed diameter.
use super::constraint_widgets::click_label;
use super::curve_edits::stored;
use super::polygon_rules::solve;
use super::widgets::{Ui, text};
use super::window::{enter, finish, open, version};
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window};
use ferritecad_document::{Expression, ExpressionSlot};
use ferritecad_ui::ModelEdit;
use std::path::Path;

fn field(w: &Window) -> &str {
    &w.creates
        .sketch
        .viewport
        .value_field
        .as_ref()
        .expect("Value")
        .2
}

pub(crate) fn set_diameter(
    path: &Path,
    source: &crate::sketch_facts::Source,
    sketch: ObjectId,
    seen: &std::cell::Cell<Option<(StableEntityId, StableEntityId)>>,
) -> (crate::sketch::Editor, EditRequest, Vec<EditChange>) {
    let (id, stored) = stored(path);
    assert_eq!(id, sketch);
    let mut drawing = stored.clone();
    assert_eq!(solve(id, &mut drawing), 0);
    let solved_circle = SketchGeometry::Circle {
        center: point(0., 0.),
        radius: 10.,
    };
    assert_eq!(drawing.curves[0].geometry, solved_circle);
    assert_eq!(
        stored.curves[0].geometry,
        SketchGeometry::Circle {
            center: point(4.5, 0.),
            radius: 1.2
        }
    );
    let (radius_id, radius) = drawing
        .constraints
        .iter_mut()
        .find_map(|c| {
            if let SketchConstraintRule::Radius { radius, .. } = &mut c.rule {
                Some((c.id, radius))
            } else {
                None
            }
        })
        .expect("saved radius");
    *radius = Expression::constant(8.5).expect("half of 17 mm");
    let slot = ExpressionSlot::Radius {
        constraint: radius_id,
    };
    let changes = vec![EditChange::constant(id, slot, 8.5).expect("radius")];
    assert_eq!(solve(id, &mut drawing), 0);
    // The complete CLI edit can start at the stored coordinates without geometry writes.
    let mut prediction = Sketch {
        constraints: drawing.constraints.clone(),
        ..stored
    };
    assert_eq!(solve(id, &mut prediction), 0);
    assert_eq!(prediction.curves, drawing.curves);
    let fixed = drawing
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::Fixed { .. }))
        .expect("fixed centre")
        .id;
    seen.set(Some((radius_id, fixed)));

    let mut w = open(path, Drawn::Native);
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, radius_id);
    ui.settle(&mut w);
    assert_eq!(field(&w), "10");
    ui.replace_dimension_text(&mut w, "123");
    ui.press(&mut w, "Diameter (Ø)", false);
    ui.settle(&mut w);
    assert_eq!(field(&w), "20");
    ui.press(&mut w, "Undo", true);
    ui.settle(&mut w);
    assert_eq!(field(&w), "10");
    ui.press(&mut w, "Redo", true);
    ui.settle(&mut w);
    assert_eq!(field(&w), "20");
    ui.press(&mut w, "Finish", false);
    assert!(!w.creates.sketch.viewport.active());
    assert!(w.creates.sketch.viewport.take_request().is_none());
    assert_eq!(version(&w), source.version);
    assert!(!w.sessions.can_undo());

    enter(&mut w, sketch, ModelEdit::Sketch);
    click_label(&mut w, &mut ui, radius_id);
    let labels = text(&ui.paint(
        &mut w.creates.sketch.viewport,
        w.input.camera(),
        w.input.region(),
    ));
    assert!(labels.iter().any(|s| s == "R 10 mm"), "{labels:?}");
    ui.settle(&mut w);
    assert_eq!(field(&w), "10");
    ui.press(&mut w, "Diameter (Ø)", false);
    ui.replace_dimension_text(&mut w, "17");
    ui.press(&mut w, "Set", false);
    let d = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    let SketchGeometry::Circle { radius, .. } = d.state.curves[0].geometry else {
        panic!("circle")
    };
    assert_eq!(radius * 2., 17., "displayed cylinder diameter in mm");
    assert_eq!(d.state.curves, drawing.curves);
    assert_eq!(d.state.constraints, drawing.constraints);
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, changes);
    (w.creates.sketch, request, changes)
}
