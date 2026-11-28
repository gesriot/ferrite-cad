// SPDX-License-Identifier: MIT
//! Set saved dimensions on the accepted polygon and independently solve the CLI reference.
use super::cli::cli_edit;
use super::constraint_widgets::click_label;
use super::curve_edits::stored;
use super::polygon_rules::{geometry_for_display, solve};
use super::widgets::{Ui, text};
use super::window::{enter, finish, open, version};
use super::*;
use crate::sessions::tests::tabs::Drawn;
use crate::sketch_facts::Source;
use ferritecad_document::{Expression, ExpressionSlot};
use ferritecad_ui::ModelEdit;
use std::path::Path;

pub(crate) fn dimension_id(sketch: &Sketch, value: f64) -> StableEntityId {
    let ids: Vec<_> = sketch
        .constraints
        .iter()
        .filter(|c| draft::dimension(c).is_some_and(|(_, n)| (n.value() - value).abs() < 1e-12))
        .map(|c| c.id)
        .collect();
    let [id] = ids.as_slice() else {
        panic!("expected one dimension at {value}: {ids:?}")
    };
    *id
}

pub(crate) fn set(
    path: &Path,
    source: &Source,
    sketch: ObjectId,
    id: StableEntityId,
    typed: &str,
) -> (crate::sketch::Editor, EditRequest, Vec<EditChange>) {
    let (stored_id, original) = stored(path);
    assert_eq!(sketch, stored_id);
    let mut shown = original.clone();
    solve(sketch, &mut shown);
    let rule = shown
        .constraints
        .iter_mut()
        .find(|c| c.id == id)
        .expect("saved dimension");
    let value: f64 = typed.parse().expect("finite fixture value");
    let (slot, constant) = match &mut rule.rule {
        SketchConstraintRule::Distance { distance, .. } => {
            *distance = Expression::constant(value).expect("distance");
            (ExpressionSlot::Distance { constraint: id }, value)
        }
        SketchConstraintRule::DirectedAngle { angle, .. } => {
            let radians = ferritecad_document::DirectedAngle::from_degrees(value)
                .expect("angle")
                .radians();
            *angle = Expression::constant(radians).expect("angle");
            (ExpressionSlot::DirectedAngle { constraint: id }, radians)
        }
        _ => panic!("expected a line dimension"),
    };
    let freedom = solve(sketch, &mut shown);
    let mut predicted = Sketch {
        constraints: shown.constraints.clone(),
        ..original
    };
    let mut changes = geometry_for_display(sketch, &mut predicted, &shown);
    let set = EditChange::constant(sketch, slot, constant).expect("new value");
    let root = tempfile::tempdir().expect("rules-only control");
    let only = root.path().join("value.fcad");
    cli_edit(path, &only, source.version, std::slice::from_ref(&set));
    let mut other = stored(&only).1;
    assert_eq!(solve(sketch, &mut other), freedom);
    changes.push(set);

    let mut w = open(path, Drawn::Native);
    assert_eq!(version(&w), source.version);
    enter(&mut w, sketch, ModelEdit::Sketch);
    w.input
        .frame(([-80., -65., -1.], [90., 75., 1.]))
        .expect("show all dimensions");
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, id);
    ui.replace_dimension_text(&mut w, typed);
    ui.press(&mut w, "Set", false);
    let state = &w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("draft")
        .state;
    assert_eq!(state.curves, shown.curves);
    assert_eq!(state.constraints, shown.constraints);
    assert_eq!(
        state
            .report
            .as_ref()
            .expect("constraint report")
            .degrees_of_freedom(),
        freedom
    );
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, changes);
    (w.creates.sketch, request, changes)
}

type Line = ([f64; 2], [f64; 2]);

fn cold_polygon(path: &Path) -> (Sketch, Vec<Line>, Vec<String>) {
    let (id, saved) = stored(path);
    let mut w = open(path, Drawn::Native);
    let source = w.scene.edit_source.as_ref().expect("cold source");
    let presentation = w
        .scene
        .sketches
        .presentation(source, id)
        .expect("cold drawing");
    let lines: Vec<_> = presentation
        .curves()
        .iter()
        .map(|c| {
            let SketchGeometry::Line { start, end } = c.geometry() else {
                panic!("polygon side")
            };
            ([start.x, start.y], [end.x, end.y])
        })
        .collect();
    assert_eq!(lines.len(), 4);
    for i in 0..4 {
        let (end, next) = (lines[i].1, lines[(i + 1) % 4].0);
        assert!((end[0] - next[0]).abs() < 1e-6 && (end[1] - next[1]).abs() < 1e-6);
    }
    enter(&mut w, id, ModelEdit::Sketch);
    w.input
        .frame(([-80., -65., -1.], [90., 75., 1.]))
        .expect("show all dimensions");
    let ui = Ui::new(&mut w);
    let words = text(&ui.paint(
        &mut w.creates.sketch.viewport,
        w.input.camera(),
        w.input.region(),
    ));
    (saved, lines, words)
}

pub(crate) fn saved_rectangle(
    saved: &Path,
    stl: &[u8],
    height: f64,
    was: (StableEntityId, StableEntityId),
) {
    let (drawing, lines, words) = cold_polygon(saved);
    let length = |i: usize| (lines[i].0[0] - lines[i].1[0]).hypot(lines[i].0[1] - lines[i].1[1]);
    assert!((length(0) - 70.).abs() < 1e-6 && (length(1) - 30.).abs() < 1e-6);
    assert!((lines[0].0[1] - lines[0].1[1]).abs() < 1e-6);
    assert!((lines[1].0[0] - lines[1].1[0]).abs() < 1e-6);
    let mut dimensions: Vec<_> = words
        .iter()
        .filter(|w| w.ends_with(" mm"))
        .map(String::as_str)
        .collect();
    dimensions.sort_unstable();
    assert_eq!(dimensions, ["30 mm", "70 mm"]);
    assert_eq!(
        (dimension_id(&drawing, 70.), dimension_id(&drawing, 30.)),
        was
    );
    // The STL: a prism 70 × 30 × height.
    let (faces, volume) = crate::feature_tests::read_stl(stl);
    let expected = 70. * 30. * height;
    assert!(
        (volume - expected).abs() <= 1e-4 * expected,
        "the exported solid has volume {volume}, the saved drawing {expected}"
    );
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in faces.iter().flatten() {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    for (got, want) in [
        (hi[0] - lo[0], 70.),
        (hi[1] - lo[1], 30.),
        (hi[2] - lo[2], height),
    ] {
        assert!((got - want).abs() < 1e-3, "extent {got}, not {want}");
    }
}

fn check_prism(lines: &[Line], stl: &[u8], height: f64) {
    let (faces, volume) = crate::feature_tests::read_stl(stl);
    let area = lines
        .iter()
        .map(|(p, q)| p[0] * q[1] - p[1] * q[0])
        .sum::<f64>()
        .abs()
        / 2.;
    assert!(area > 0.);
    let expected = area * height;
    assert!(
        (volume - expected).abs() <= expected * 1e-4,
        "{volume} vs {expected}"
    );
    let xy = |x: f64, y: f64| ((x * 1e3).round() as i64, (y * 1e3).round() as i64);
    let mut base: Vec<_> = faces
        .iter()
        .flatten()
        .filter(|p| p[2].abs() < 1e-6)
        .map(|p| xy(p[0], p[1]))
        .collect();
    base.sort_unstable();
    base.dedup();
    let mut corners: Vec<_> = lines.iter().map(|(p, _)| xy(p[0], p[1])).collect();
    corners.sort_unstable();
    assert_eq!(base, corners);
    let top = faces
        .iter()
        .flatten()
        .map(|p| p[2])
        .fold(f64::MIN, f64::max);
    assert!((top - height).abs() < 1e-6);
}

pub(crate) fn saved_angled_quad(
    path: &Path,
    stl: &[u8],
    height: f64,
    was: (StableEntityId, StableEntityId),
) {
    let (drawing, lines, words) = cold_polygon(path);
    let mut labels: Vec<_> = words
        .iter()
        .map(String::as_str)
        .filter(|w| w.ends_with(" mm") || w.starts_with("A>B "))
        .collect();
    labels.sort_unstable();
    assert_eq!(labels, ["30 mm", "40 mm", "50 mm", "A>B -130°", "A>B 100°"]);
    for (i, expected) in [50., 30., 40.].into_iter().enumerate() {
        let (p, q) = lines[i];
        assert!(((q[0] - p[0]).hypot(q[1] - p[1]) - expected).abs() < 1e-6);
    }
    assert!(lines[0].0.iter().all(|n| n.abs() < 1e-6));
    assert!((lines[0].0[1] - lines[0].1[1]).abs() < 1e-6);
    for (id, degrees, first, second, reverse) in
        [(was.0, -130_f64, 1, 2, true), (was.1, 100., 0, 1, false)]
    {
        assert_eq!(dimension_id(&drawing, degrees.to_radians()), id);
        let c = drawing
            .constraints
            .iter()
            .find(|c| c.id == id)
            .expect("saved angle");
        let SketchConstraintRule::DirectedAngle { a, b, .. } = c.rule else {
            panic!("angle")
        };
        let vector = |i: usize, flipped: bool| {
            let id = drawing.curves[i].id;
            let (mut p, mut q) = (SketchPointSelector::Start, SketchPointSelector::End);
            if flipped {
                std::mem::swap(&mut p, &mut q);
            }
            (SketchPointRef::new(id, p), SketchPointRef::new(id, q))
        };
        assert_eq!((a.from, a.to), vector(first, reverse));
        assert_eq!((b.from, b.to), vector(second, false));
        let (p, q) = lines[first];
        let (s, t) = lines[second];
        let sign = if reverse { -1. } else { 1. };
        let (x, y) = ((q[0] - p[0]) * sign, (q[1] - p[1]) * sign);
        let (u, v) = (t[0] - s[0], t[1] - s[1]);
        let turn = (x * v - y * u).atan2(x * u + y * v).to_degrees();
        assert!((turn - degrees).abs() < 1e-6, "{turn} vs {degrees}");
    }
    check_prism(&lines, stl, height);
}

pub(crate) fn unequal_rectangle(
    path: &Path,
    source: &Source,
    sketch: ObjectId,
    seen: &std::cell::Cell<Option<(StableEntityId, StableEntityId, usize)>>,
) -> (crate::sketch::Editor, EditRequest, Vec<EditChange>) {
    let (id, original) = stored(path);
    assert_eq!(id, sketch);
    let mut shown = original.clone();
    let freedom = solve(id, &mut shown);
    assert_eq!(freedom, 0);
    let equal = shown
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::EqualLength { .. }))
        .expect("EqualLength")
        .id;
    let parallel = shown
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::Parallel { .. }))
        .expect("Parallel")
        .id;
    seen.set(Some((equal, parallel, freedom)));
    let mut w = open(path, Drawn::Native);
    enter(&mut w, sketch, ModelEdit::Sketch);
    w.input
        .frame(([-20., -20., -1.], [80., 50., 1.]))
        .expect("show both markers");
    let mut ui = Ui::new(&mut w);
    let words = text(&ui.paint(
        &mut w.creates.sketch.viewport,
        w.input.camera(),
        w.input.region(),
    ));
    for label in ["Eq", "Fix", "H", "Par", "V", "60 mm", "30 mm"] {
        assert!(words.iter().any(|s| s == label), "{words:?}");
    }
    click_label(&mut w, &mut ui, parallel);
    click_label(&mut w, &mut ui, equal);
    ui.press(&mut w, "Delete", true);
    shown.constraints.retain(|c| c.id != equal);
    assert_eq!(solve(id, &mut shown), freedom + 1);
    let draft = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    assert_eq!(draft.state.curves, shown.curves);
    assert_eq!(draft.state.constraints, shown.constraints);

    let mut changes = vec![EditChange::RemoveConstraint {
        sketch,
        constraint: equal,
    }];
    let root = tempfile::tempdir().expect("control directory");
    let control = root.path().join("rules-only.fcad");
    cli_edit(path, &control, source.version, &changes);
    let mut free = stored(&control).1;
    assert_eq!(solve(id, &mut free), freedom + 1);
    let (a, b) = super::polygon_rules::ends(&free, 2);
    assert!(((a.x - b.x).hypot(a.y - b.y) - 65.).abs() < 1e-3);
    let mut expected = Sketch {
        constraints: shown.constraints.clone(),
        ..original
    };
    changes.extend(geometry_for_display(sketch, &mut expected, &shown));
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, changes);
    (w.creates.sketch, request, changes)
}

pub(crate) fn saved_unequal_rectangle(
    path: &Path,
    stl: &[u8],
    height: f64,
    was: (StableEntityId, StableEntityId, usize),
) {
    let (mut drawing, lines, words) = cold_polygon(path);
    let (id, _) = stored(path);
    assert_eq!(solve(id, &mut drawing), was.2 + 1);
    assert!(drawing.constraints.iter().all(|c| c.id != was.0));
    assert!(
        drawing
            .constraints
            .iter()
            .any(|c| c.id == was.1 && matches!(c.rule, SketchConstraintRule::Parallel { .. }))
    );
    assert!(words.iter().all(|s| s != "Eq"));
    for label in ["Fix", "H", "Par", "V", "60 mm", "30 mm"] {
        assert!(words.iter().any(|s| s == label), "{words:?}");
    }
    for ((p, _), [x, y]) in lines
        .iter()
        .zip([[0., 0.], [60., 0.], [60., 30.], [0., 30.]])
    {
        assert!((p[0] - x).abs() < 1e-6 && (p[1] - y).abs() < 1e-6, "{p:?}");
    }
    check_prism(&lines, stl, height);
}
