// SPDX-License-Identifier: MIT
use super::*;
use crate::sketch_mode::tests::{cli::cli_edit, curve_edits, polygon_rules, window};
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, EditChange, Expression, ExpressionSlot,
    LineConstraintKind, LineLengthMm, Sketch, SketchConstraintEdits, SketchConstraintRule,
};
use ferritecad_types::StableEntityId;

fn side(drawing: &Sketch, mm: f64) -> StableEntityId {
    let curve = drawing.curves[1].id;
    drawing
        .constraints
        .iter()
        .find(|c| {
            matches!(&c.rule,
                SketchConstraintRule::Distance { a, distance, .. }
                    if a.curve == curve && distance.value() == mm
            )
        })
        .expect("sector side dimension")
        .id
}

/// Compare a saved window document with the full CLI edit and two independent controls.
pub(super) fn compare(source: &Path, window: &Path, root: &Path) -> PathBuf {
    let (id, stored) = curve_edits::stored(source);
    let width = side(&stored, 12.75);
    let before: Vec<_> = stored.constraints.iter().map(|c| c.id).collect();
    assert_eq!(before.len(), 11);
    let version = crate::sketch_facts::read(source).expect("source").version;
    let set = EditChange::constant(id, ExpressionSlot::Distance { constraint: width }, 13.75)
        .expect("side dimension");
    let mut preview = stored.clone();
    assert_eq!(polygon_rules::solve(id, &mut preview), 0);
    let rule = preview
        .constraints
        .iter_mut()
        .find(|c| c.id == width)
        .expect("same side");
    let SketchConstraintRule::Distance { distance, .. } = &mut rule.rule else {
        panic!("side must be a distance");
    };
    *distance = Expression::constant(13.75).expect("height");
    let freedom = polygon_rules::solve(id, &mut preview);
    assert_eq!(freedom, 0);
    let mut expected = Sketch {
        constraints: preview.constraints.clone(),
        ..stored.clone()
    };
    let mut full = polygon_rules::geometry_for_display(id, &mut expected, &preview);
    full.push(set.clone());
    let turn = |tag: &str, changes: &[EditChange]| {
        let step = root.join(format!("{tag}-side.fcad"));
        cli_edit(source, &step, version, changes);
        let reading = crate::sketch_facts::read(&step).expect("side edited");
        let feature =
            ferritecad_fixtures::expression_slot(&reading, ExpressionSlot::RevolveAngle).object;
        let edit = EditChange::constant(
            feature,
            ExpressionSlot::RevolveAngle,
            212.25_f64.to_radians(),
        )
        .expect("turn");
        let turned = root.join(format!("{tag}-turned.fcad"));
        cli_edit(&step, &turned, reading.version, &[edit]);
        turned
    };
    let full_cli = turn("full", &full);
    let observed = curve_edits::stored(window).1;
    assert_eq!(observed, expected);
    assert_eq!(observed.curves, stored.curves, "solver baked coordinates");
    assert!(
        observed
            .constraints
            .iter()
            .map(|c| c.id)
            .eq(before.iter().copied())
    );
    assert_eq!(
        cells(window),
        cells(&full_cli),
        "GUI constraint/angle cells"
    );
    assert!(same_model_with_explicit_new_ids(window, &full_cli, id, &before, "Set").is_empty());
    let exports = peer_bytes(window, root, "window-set");
    assert_eq!(exports, peer_bytes(&full_cli, root, "full-set"));

    let value_only = turn("value-only", &[set]);
    assert_eq!(cells(window), cells(&value_only), "fully constrained side");
    assert_eq!(exports, peer_bytes(&value_only, root, "value-set"));
    let replacement = crate::transaction_tests::constraint_changes(
        source,
        id,
        &SketchConstraintEdits {
            remove: vec![width],
            add: vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                curve: stored.curves[1].id,
                kind: LineConstraintKind::Distance(LineLengthMm::new(13.75).expect("height")),
            })],
        },
    );
    let replaced = turn("replacement", &replacement);
    let other = curve_edits::stored(&replaced).1;
    let fresh = side(&other, 13.75);
    assert!(!before.contains(&fresh));
    assert_eq!(other.constraints.len(), 11);
    assert!(
        other
            .constraints
            .iter()
            .map(|c| c.id)
            .filter(|&i| i != fresh)
            .eq(before.iter().copied().filter(|&i| i != width))
    );
    assert_eq!(exports, peer_bytes(&replaced, root, "replacement"));
    let refusal = std::panic::catch_unwind(|| {
        same_model_with_explicit_new_ids(
            window,
            &replaced,
            id,
            &before,
            "GUI constraint/angle cells",
        );
    })
    .expect_err("replacing a retained UUID must fail");
    let message = refusal
        .downcast_ref::<String>()
        .expect("identity diagnostic");
    assert!(
        message
            .lines()
            .next()
            .is_some_and(|line| line.ends_with("GUI constraint/angle cells: an old UUID changed")),
        "{message}"
    );
    full_cli
}

/// Exercise the same constrained-sector comparison in automated native runs.
pub(super) fn through_widgets() {
    let root = tempfile::tempdir().expect("sector");
    let root = root.path();
    let initial = create(root, false);
    let source = root.join("constrained.fcad");
    let setup = constraint_request(&initial, 4.75, 12.75);
    cli_edit(&initial, &source, setup.expected, &setup.changes);
    let work = root.join("work.fcad");
    std::fs::copy(&source, &work).expect("copy");
    let (sketch, drawing) = curve_edits::stored(&source);
    let mut w = window::open(&work, crate::sessions::tests::tabs::Drawn::Native);
    let set = curve_edits::set_dimension(&mut w, sketch, side(&drawing, 12.75), "13.75");
    assert!(window::apply(&mut w, set));
    let current = w.sessions.export_path().expect("after Set");
    let facts = crate::sketch_facts::read(&current).expect("angle");
    let (_, apply_angle) = typed_angle(&current, &facts, &w.sessions, "212.25");
    assert!(window::apply(&mut w, apply_angle));
    let active = w.sessions.export_path().expect("before Save");
    let exports = export_bytes(&active, &work, root, "unsaved-sector");
    assert_eq!(
        std::fs::read(&source).expect("source"),
        std::fs::read(&work).expect("disk")
    );
    saved(&mut w.sessions, SaveTarget::InPlace);
    let full_cli = compare(&source, &work, root);
    assert_eq!(exports, peer_bytes(&full_cli, root, "checked-sector"));
    cold(&work, 2.75, 7.5, 13.75, 212.25);
    mesh(&exports.0, 2.75, 7.5, -3.25, 13.75, 212.25);
}
