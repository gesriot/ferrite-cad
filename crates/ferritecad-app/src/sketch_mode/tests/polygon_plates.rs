// SPDX-License-Identifier: MIT
use super::circular_rules::same_model;
use super::cli::{cli_edit, export};
use super::constraint_widgets::click_label;
use super::curve_edits::stored;
use super::dragging::{frame, movement, press, release};
use super::polygon_rules::{add, close, distance, ends, orient, solve};
use super::widgets::Ui;
use super::window::{click, enter, finish, open, publish, version};
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window};
use crate::sketch::drag_tests::{attach_source_claim, sql_facts};
use ferritecad_document::{Expression, SketchConstraintRule as Rule, SketchSegmentRef};
use ferritecad_ui::ModelEdit;

fn plate(path: &Path, points: [[f64; 2]; 4]) {
    let input = path.with_extension("json");
    let shape = serde_json::json!({"request_version":1,"points_mm":points,"height_mm":10.});
    std::fs::write(&input, shape.to_string()).expect("template");
    let result = std::process::Command::new(crate::creates::tests::ferritecad())
        .arg("create-sketch-extrude")
        .arg(&input)
        .arg("-o")
        .arg(path)
        .arg("--json")
        .output()
        .expect("create polygon");
    assert!(result.status.success(), "{result:?}");
    attach_source_claim(path);
}

fn pair(
    w: &mut Window,
    ui: &mut Ui,
    sketch: ObjectId,
    peer: &mut Sketch,
    selection: ([usize; 2], &str),
    make: impl FnOnce(SketchSegmentRef, SketchSegmentRef) -> Rule,
) {
    let (sides, label) = selection;
    let segment = |i: usize| {
        let id = peer.curves[i].id;
        SketchSegmentRef::new(
            SketchPointRef::new(id, SketchPointSelector::Start),
            SketchPointRef::new(id, SketchPointSelector::End),
        )
    };
    let rule = make(segment(sides[0]), segment(sides[1]));
    for (n, i) in sides.into_iter().enumerate() {
        let (p, q) = ends(peer, i);
        click(w, point((p.x + q.x) / 2., (p.y + q.y) / 2.), n == 1);
    }
    add(w, ui, sketch, peer, label, rule);
}

fn edits_for_display(
    sketch: ObjectId,
    saved: &Sketch,
    shown: &Sketch,
) -> (Sketch, Vec<EditChange>, Vec<EditChange>) {
    let mut expected = Sketch {
        constraints: shown.constraints.clone(),
        ..saved.clone()
    };
    let mut rules: Vec<_> = saved
        .constraints
        .iter()
        .filter(|c| !shown.constraints.iter().any(|r| r.id == c.id))
        .map(|c| EditChange::RemoveConstraint {
            sketch,
            constraint: c.id,
        })
        .collect();
    let additions: Vec<_> = shown
        .constraints
        .iter()
        .filter(|c| !saved.constraints.iter().any(|r| r.id == c.id))
        .map(|c| EditChange::AddConstraint {
            sketch,
            constraint: c.clone(),
        })
        .collect();
    let mut prediction = expected.clone();
    solve(sketch, &mut prediction);
    let mut full = rules.clone();
    for i in 0..saved.curves.len() {
        let (a, b) = ends(&prediction, i);
        let (c, d) = ends(shown, i);
        if [a.x - c.x, a.y - c.y, b.x - d.x, b.y - d.y]
            .iter()
            .any(|delta| delta.abs() > 1e-6)
        {
            let curve = &shown.curves[i];
            expected.curves[i].geometry = curve.geometry.clone();
            full.push(EditChange::SetCurveGeometry {
                sketch,
                curve: curve.id,
                geometry: curve.geometry.clone(),
            });
        }
    }
    full.extend(additions.iter().cloned());
    rules.extend(additions);
    (expected, rules, full)
}

fn compare_sql(source: &Path, result: &Path, peer: &Path, sketch: ObjectId) {
    let before = sql_facts(source);
    let after = sql_facts(result);
    assert_eq!(after, sql_facts(peer), "SQL rows including rowids");
    assert!(before.keys().eq(after.keys()));
    let id = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
    for (name, rows) in before {
        if name == "objects" {
            assert_eq!(rows.len(), after[&name].len());
            for (old, new) in rows.iter().zip(&after[&name]) {
                for (i, cell) in old.iter().enumerate() {
                    if old[1] != id || ![6, 7].contains(&i) {
                        assert_eq!(cell, &new[i], "{name} column {i}");
                    }
                }
            }
        } else {
            assert_eq!(rows, after[&name], "{name}");
        }
    }
}

fn solid(path: &Path, artifact: Option<&str>) -> ([f64; 3], [f64; 3], f64) {
    let bytes = export(path, "stl");
    if let (Some(name), Some(dir)) = (
        artifact,
        std::env::var_os("FERRITECAD_CONSTRAINT_ARTIFACTS"),
    ) {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).expect("artifact directory");
        std::fs::copy(path, dir.join(format!("{name}.fcad"))).expect("document");
        std::fs::write(dir.join(format!("{name}.stl")), &bytes).expect("STL");
        std::fs::write(dir.join(format!("{name}.fbx")), export(path, "fbx")).expect("FBX");
    }
    let (faces, volume) = crate::feature_tests::read_stl(&bytes);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in faces.into_iter().flatten() {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    (lo, hi, volume.abs())
}

#[test]
fn native_viewport_line_relations_and_cli_orient_the_same_solved_body() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_eval::solver_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("plate");
    let src = root.path().join("source.fcad");
    plate(&src, [[-20., -10.], [40., -8.], [42., 30.], [-20., 30.]]);
    let original = std::fs::read(&src).expect("source");
    let (sketch, saved) = stored(&src);
    let mut shown = saved.clone();
    let mut w = open(&src, Drawn::Native);
    enter(&mut w, sketch, ModelEdit::Sketch);
    w.input
        .frame(([-27., -18., -10.], [51., 38., 10.]))
        .expect("fit plate");
    let mut ui = Ui::new(&mut w);
    assert!(shown.constraints.is_empty());
    close(&mut w, &mut ui, sketch, &mut shown);
    for sides in [[0, 2], [1, 3]] {
        pair(
            &mut w,
            &mut ui,
            sketch,
            &mut shown,
            (sides, "Parallel"),
            |a, b| Rule::Parallel { a, b },
        );
    }
    pair(
        &mut w,
        &mut ui,
        sketch,
        &mut shown,
        ([0, 1], "Perpendicular"),
        |a, b| Rule::Perpendicular { a, b },
    );
    distance(&mut w, &mut ui, sketch, &mut shown, 0, 60.);
    distance(&mut w, &mut ui, sketch, &mut shown, 1, 30.);
    ui.press(&mut w, "Undo", false);
    ui.press(&mut w, "Redo", false);
    let (expected, rules, full) = edits_for_display(sketch, &saved, &shown);
    assert_eq!(expected.constraints.len(), 9);
    let peer = root.path().join("full.fcad");
    let control = root.path().join("rules.fcad");
    cli_edit(&src, &peer, version(&w), &full);
    cli_edit(&src, &control, version(&w), &rules);
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, full);
    publish(&mut w, request);
    let result = w.sessions.export_path().expect("published revision");
    same_model(&result, &peer, sketch, &expected, 3);
    compare_sql(&src, &result, &peer, sketch);
    for (path, artifact) in [
        (&result, Some("relation-ui")),
        (&peer, Some("relation-cli")),
        (&control, None),
    ] {
        let (_, drawing) = stored(path);
        let report = crate::transaction_tests::solve_path(path).expect("cold solve report");
        assert_eq!(report.degrees_of_freedom(), 3);
        assert!(report.redundant().is_empty());
        if path == &control {
            assert_eq!(drawing.curves, saved.curves);
            assert_eq!(drawing.constraints, expected.constraints);
        }
        let (lo, hi, volume) = solid(path, artifact);
        assert!((volume - 18000.).abs() < 0.02, "{volume}");
        assert!((hi[2] - lo[2] - 10.).abs() < 1e-4);
        assert!(
            hi[0] - lo[0] > 60.05 && hi[1] - lo[1] > 30.05,
            "{lo:?} {hi:?}"
        );
    }
    assert_eq!(std::fs::read(&src).expect("source"), original);
}

fn pinned(equal: bool) {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_eval::solver_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("plate");
    let source = root.path().join("source.fcad");
    plate(
        &source,
        [[-40., -20.], [40., -20.], [40., 20.], [-40., 20.]],
    );
    let bytes = std::fs::read(&source).expect("source");
    let (sketch, saved) = stored(&source);
    let mut shown = saved.clone();
    assert!(shown.constraints.is_empty());
    let mut w = open(&source, Drawn::Native);
    enter(&mut w, sketch, ModelEdit::Sketch);
    w.input
        .frame(([-51., -32., -10.], [87., 68., 10.]))
        .expect("fit both positions");
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Off", false);
    ui.press(&mut w, "1 mm", false);
    let corner = point(10., -5.);
    let old = ends(&shown, 0).0;
    for _ in 0..2 {
        press(&mut w, old);
        assert!(w.creates.sketch.viewport.gesturing());
        movement(&mut w, corner);
        frame(&mut w);
        release(&mut w);
    }
    let (_, end) = ends(&shown, 0);
    shown.curves[0].geometry = SketchGeometry::Line { start: corner, end };
    let (start, _) = ends(&shown, 3);
    shown.curves[3].geometry = SketchGeometry::Line { start, end: corner };
    click(&mut w, corner, false);
    let pin = Rule::Fixed {
        point: SketchPointRef::new(saved.curves[0].id, SketchPointSelector::Start),
        x: Expression::constant(10.).expect("X"),
        y: Expression::constant(-5.).expect("Y"),
    };
    add(&mut w, &mut ui, sketch, &mut shown, "Fix", pin);
    let pin_id = shown.constraints[0].id;
    close(&mut w, &mut ui, sketch, &mut shown);
    for i in 0..4 {
        orient(&mut w, &mut ui, sketch, &mut shown, i, i % 2 == 1);
    }
    distance(&mut w, &mut ui, sketch, &mut shown, 0, 60.);
    if equal {
        pair(
            &mut w,
            &mut ui,
            sketch,
            &mut shown,
            ([0, 1], "Equal"),
            |a, b| Rule::EqualLength { a, b },
        );
    } else {
        distance(&mut w, &mut ui, sketch, &mut shown, 1, 30.);
    }
    ui.press(&mut w, "Undo", false);
    ui.press(&mut w, "Redo", false);
    let (expected, rules, full) = edits_for_display(sketch, &saved, &shown);
    assert_eq!(expected.constraints.len(), 11);
    let peer = root.path().join("peer.fcad");
    let control = root.path().join("rules.fcad");
    cli_edit(&source, &peer, version(&w), &full);
    cli_edit(&source, &control, version(&w), &rules);
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, full);
    publish(&mut w, request);
    let result = w.sessions.export_path().expect("published revision");
    same_model(&result, &peer, sketch, &expected, 0);
    compare_sql(&source, &result, &peer, sketch);
    let names = if equal {
        ["equal-ui", "equal-cli"]
    } else {
        ["ui", "cli"]
    };
    let height = if equal { 60. } else { 30. };
    for (path, artifact) in [
        (&result, Some(names[0])),
        (&peer, Some(names[1])),
        (&control, None),
    ] {
        let (_, drawing) = stored(path);
        assert_eq!(drawing.constraints, expected.constraints);
        if path == &control {
            assert_eq!(drawing.curves, saved.curves);
        }
        let report = crate::transaction_tests::solve_path(path).expect("cold solve report");
        assert_eq!(report.degrees_of_freedom(), 0);
        assert!(report.redundant().is_empty());
        let (lo, hi, volume) = solid(path, artifact);
        for (i, mm) in [60., height, 10.].into_iter().enumerate() {
            assert!((hi[i] - lo[i] - mm).abs() < 1e-4, "{lo:?} {hi:?}");
        }
        let tolerance = if equal { 0.04 } else { 0.02 };
        assert!((volume - 600. * height).abs() < tolerance, "{volume}");
        for (i, v) in [10., -5.].into_iter().enumerate() {
            assert!(
                (lo[i] - v).abs() < 1e-4 || (hi[i] - v).abs() < 1e-4,
                "{lo:?} {hi:?}"
            );
        }
    }
    if !equal {
        enter(&mut w, sketch, ModelEdit::Sketch);
        let mut ui = Ui::new(&mut w);
        click_label(&mut w, &mut ui, pin_id);
        ui.press(&mut w, "Delete", true);
        let mut freed = expected.clone();
        assert_eq!(solve(sketch, &mut freed), 0);
        freed.constraints.retain(|c| c.id != pin_id);
        assert_eq!(solve(sketch, &mut freed), 2);
        let (unfixed, rules, full) = edits_for_display(sketch, &expected, &freed);
        assert_eq!(
            rules,
            [EditChange::RemoveConstraint {
                sketch,
                constraint: pin_id
            }]
        );
        let other = root.path().join("unfixed.fcad");
        let only_rule = root.path().join("unfixed-rule.fcad");
        cli_edit(&result, &other, version(&w), &full);
        cli_edit(&result, &only_rule, version(&w), &rules);
        assert_eq!(stored(&only_rule).1.constraints, unfixed.constraints);
        let request = finish(&mut w, &mut ui);
        assert_eq!(request.changes, full);
        publish(&mut w, request);
        same_model(
            &w.sessions.export_path().expect("pin removed"),
            &other,
            sketch,
            &unfixed,
            2,
        );
    }
    assert_eq!(std::fs::read(&source).expect("source"), bytes);
}

#[test]
fn native_viewport_fixed_corner_and_cli_pin_the_same_solved_body() {
    pinned(false);
}

#[test]
fn native_viewport_equal_length_and_cli_tie_the_same_solved_body() {
    pinned(true);
}

fn horizontal_plate(dimensioned: bool) {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_eval::solver_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("plate");
    let source = root.path().join("source.fcad");
    let points = if dimensioned {
        [[-40., -20.], [40., -20.], [40., 20.], [-40., 20.]]
    } else {
        [[-20., -10.], [40., -8.], [42., 30.], [-20., 30.]]
    };
    plate(&source, points);
    let original = std::fs::read(&source).expect("source");
    let (sketch, saved) = stored(&source);
    let mut w = open(&source, Drawn::Native);
    enter(&mut w, sketch, ModelEdit::Sketch);
    w.input
        .frame(([-51., -35., -10.], [55., 45., 10.]))
        .expect("fit");
    let mut ui = Ui::new(&mut w);
    let mut shown = saved.clone();
    assert!(shown.constraints.is_empty());
    close(&mut w, &mut ui, sketch, &mut shown);
    orient(&mut w, &mut ui, sketch, &mut shown, 0, false);
    let horizontal = shown.constraints.last().expect("horizontal").id;
    if dimensioned {
        for i in 1..4 {
            orient(&mut w, &mut ui, sketch, &mut shown, i, i % 2 == 1);
        }
        distance(&mut w, &mut ui, sketch, &mut shown, 0, 60.);
        distance(&mut w, &mut ui, sketch, &mut shown, 1, 30.);
    }
    let (expected, rules, full) = edits_for_display(sketch, &saved, &shown);
    assert_eq!(expected.constraints.len(), if dimensioned { 10 } else { 5 });
    let freedom = if dimensioned { 2 } else { 7 };
    let control = root.path().join("rules.fcad");
    let cli = root.path().join("full.fcad");
    cli_edit(&source, &cli, version(&w), &full);
    cli_edit(&source, &control, version(&w), &rules);
    assert_eq!(
        stored(&control).1,
        Sketch {
            constraints: shown.constraints.clone(),
            ..saved
        }
    );
    let undo_state = w
        .creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open")
        .undo
        .last()
        .expect("last edit")
        .clone();
    ui.press(&mut w, "Undo", true);
    assert_eq!(
        w.creates
            .sketch
            .viewport
            .draft
            .as_ref()
            .expect("open")
            .state,
        undo_state
    );
    ui.press(&mut w, "Redo", true);
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, full);
    publish(&mut w, request);
    let first = w.sessions.export_path().expect("accepted");
    let first_bytes = std::fs::read(&first).expect("version");
    same_model(&first, &cli, sketch, &expected, freedom);
    compare_sql(&source, &first, &cli, sketch);
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    let current = w.creates.sketch.viewport.draft.as_ref().expect("open");
    assert!(current.undo.is_empty() && current.redo.is_empty());
    click_label(&mut w, &mut ui, horizontal);
    ui.press(&mut w, "Delete", true);
    let mut shown = expected.clone();
    assert_eq!(solve(sketch, &mut shown), freedom);
    shown.constraints.retain(|c| c.id != horizontal);
    let released = solve(sketch, &mut shown);
    let (removed, removed_rules, full) = edits_for_display(sketch, &expected, &shown);
    assert_eq!(
        removed_rules,
        [EditChange::RemoveConstraint {
            sketch,
            constraint: horizontal
        }]
    );
    let peer = root.path().join("remove-horizontal.fcad");
    cli_edit(&first, &peer, version(&w), &full);
    let remove = finish(&mut w, &mut ui);
    assert_eq!(remove.changes, full);
    publish(&mut w, remove);
    same_model(
        &w.sessions.export_path().expect("horizontal removed"),
        &peer,
        sketch,
        &removed,
        released,
    );
    if dimensioned {
        w.step(true);
        enter(&mut w, sketch, ModelEdit::Sketch);
        let mut ui = Ui::new(&mut w);
        let mut shown = expected.clone();
        solve(sketch, &mut shown);
        let width = shown
            .constraints
            .iter_mut()
            .find(|c| matches!(&c.rule,Rule::Distance{distance,..} if distance.value()==60.))
            .expect("60 mm width");
        let id = width.id;
        let Rule::Distance { distance, .. } = &mut width.rule else {
            unreachable!()
        };
        *distance = Expression::constant(55.).expect("width");
        solve(sketch, &mut shown);
        click_label(&mut w, &mut ui, id);
        ui.replace_dimension_text(&mut w, "55");
        ui.press(&mut w, "Set", false);
        ui.press(&mut w, "Undo", true);
        assert_eq!(
            w.creates
                .sketch
                .viewport
                .draft
                .as_ref()
                .expect("open")
                .state
                .constraints,
            expected.constraints
        );
        ui.press(&mut w, "Redo", true);
        let (edited, other_rules, mut full) = edits_for_display(sketch, &expected, &shown);
        assert!(other_rules.is_empty());
        let change = EditChange::constant(
            sketch,
            ferritecad_document::ExpressionSlot::Distance { constraint: id },
            55.,
        )
        .expect("Set");
        let rule_peer = root.path().join("set-only.fcad");
        cli_edit(
            &first,
            &rule_peer,
            version(&w),
            std::slice::from_ref(&change),
        );
        full.push(change);
        let other = root.path().join("full-resized.fcad");
        cli_edit(&first, &other, version(&w), &full);
        let req = finish(&mut w, &mut ui);
        assert_eq!(req.changes, full);
        publish(&mut w, req);
        let updated = w.sessions.export_path().expect("width 55");
        same_model(&updated, &other, sketch, &edited, 2);
        compare_sql(&first, &updated, &other, sketch);
        assert!(
            edited
                .constraints
                .iter()
                .map(|c| c.id)
                .eq(expected.constraints.iter().map(|c| c.id))
        );
        for (path, artifact) in [
            (&updated, Some("ui-replaced")),
            (&other, Some("cli-replaced")),
            (&rule_peer, None),
        ] {
            let (lo, hi, volume) = solid(path, artifact);
            for (i, mm) in [55., 30., 10.].into_iter().enumerate() {
                assert!((hi[i] - lo[i] - mm).abs() < 1e-4, "{lo:?} {hi:?}");
            }
            assert!((volume - 16500.).abs() < 0.02, "{volume}");
        }
    }
    assert_eq!(std::fs::read(&source).expect("source"), original);
    assert_eq!(
        std::fs::read(&first).expect("constrained source"),
        first_bytes
    );
}

#[test]
fn native_viewport_horizontal_and_cli_preserve_the_solved_body() {
    horizontal_plate(false);
}
#[test]
fn native_viewport_line_length_and_cli_preserve_and_resize_the_solved_body() {
    horizontal_plate(true);
}
