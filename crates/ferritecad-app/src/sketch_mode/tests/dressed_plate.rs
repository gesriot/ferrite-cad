// SPDX-License-Identifier: MIT
//! Pin and dimension the base sketch while retaining its named downstream edges.
use super::circular_rules::same_model;
use super::cli::cli_edit;
use super::constraint_widgets::press_rule;
use super::curve_edits::stored;
use super::dragging::{frame, movement, press, release};
use super::widgets::Ui;
use super::window::{click, enter, finish, open, publish, version};
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window};
use ferritecad_document::{Document, Expression, SketchConstraintRule as Rule};
use ferritecad_kernel::OperationContext;
use ferritecad_ui::ModelEdit;

fn ends(sketch: &Sketch, i: usize) -> (Point2, Point2) {
    let SketchGeometry::Line { start, end } = sketch.curves[i].geometry else {
        panic!("a side of the plate")
    };
    (start, end)
}

fn solve(sketch: ObjectId, drawing: &mut Sketch) -> usize {
    let (next, report) = ferritecad_eval::solve_sketch(drawing, sketch)
        .expect("consistent plate constraints")
        .expect("constrained sketch");
    *drawing = next;
    report.degrees_of_freedom()
}

// Only the new UUID is shared with the peer. Its rule and coordinates come from
// the original document, the widget action and the public sketch solver.
fn add(w: &mut Window, ui: &mut Ui, sketch: ObjectId, peer: &mut Sketch, button: &str, rule: Rule) {
    press_rule(ui, w, button);
    let draft = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    let new = draft.state.constraints.last().expect("new rule");
    assert_eq!(new.rule, rule, "{button}");
    peer.constraints.push(SketchConstraint { id: new.id, rule });
    solve(sketch, peer);
}

fn check_body(path: &Path, sketch: ObjectId, pin_y: f64, expected: f64) {
    let d = Document::open_read_only(path).expect("accepted document");
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = ferritecad_eval::rebuild_cold(&d, &mut k, &OperationContext::default())
        .expect("cold rebuild");
    let curves = built
        .sketch_presentation(sketch)
        .expect("solved rectangle")
        .curves();
    let SketchGeometry::Line { start, .. } = curves[1].geometry() else {
        panic!("pinned side")
    };
    assert!((start.x - 36.5).abs() < 1e-7 && (start.y - pin_y).abs() < 1e-7);
    let body = d
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("Body");
    let (_, volume) = k
        .shape_stats(built.shape(body.id).expect("built"))
        .expect("volume");
    assert!(
        (volume - expected).abs() < 1e-9 * expected,
        "{volume} vs {expected}"
    );
    assert_eq!(
        built
            .solve_report(sketch)
            .expect("solve")
            .degrees_of_freedom(),
        0
    );
    crate::test_names::assert_names(&built, &crate::test_names::names(&d));
    built.release_all(&mut k);
    d.close().expect("close");
}

fn constrain_plate(path: &Path, root: &Path, removed_area: f64) {
    let source = std::fs::read(path).expect("source");
    let (sketch, saved) = stored(path);
    assert!(saved.constraints.is_empty());
    assert!(
        ferritecad_eval::solve_sketch(&saved, sketch)
            .expect("source")
            .is_none()
    );
    let mut peer = saved.clone();
    let mut w = open(path, Drawn::Native);
    let base = version(&w);
    enter(&mut w, sketch, ModelEdit::Sketch);
    w.input
        .frame(([-15., -10., -1.], [50., 30., 1.]))
        .expect("fit plate");
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Off", false);
    ui.press(&mut w, "0.1 mm", false);
    let corner = point(36.5, 1.3);
    let old = ends(&saved, 1).0;
    assert_eq!(old, point(33., 3.25));
    // No closure links exist yet. Move both saved handles to the same grid node.
    for _ in 0..2 {
        press(&mut w, old);
        assert!(w.creates.sketch.viewport.gesturing());
        movement(&mut w, corner);
        frame(&mut w);
        release(&mut w);
    }
    let (start, _) = ends(&peer, 0);
    peer.curves[0].geometry = SketchGeometry::Line { start, end: corner };
    let (_, end) = ends(&peer, 1);
    peer.curves[1].geometry = SketchGeometry::Line { start: corner, end };
    let handle = |i: usize, at| SketchPointRef::new(saved.curves[i].id, at);
    let start = |i| handle(i, SketchPointSelector::Start);
    let end = |i| handle(i, SketchPointSelector::End);
    let literal = |v| Expression::constant(v).expect("finite millimetres");
    // The first click takes the preceding side's End. Extend the selection, then
    // toggle that first handle off to pin the second side's stored Start.
    for shift in [false, true, true] {
        click(&mut w, corner, shift);
    }
    add(
        &mut w,
        &mut ui,
        sketch,
        &mut peer,
        "Fix",
        Rule::Fixed {
            point: start(1),
            x: literal(corner.x),
            y: literal(corner.y),
        },
    );

    for i in 0..4 {
        let next = (i + 1) % 4;
        let (a, b) = if next == 0 {
            (start(0), end(i))
        } else {
            (end(i), start(next))
        };
        let joint = ends(&peer, i).1;
        click(&mut w, joint, false);
        click(&mut w, joint, true);
        add(
            &mut w,
            &mut ui,
            sketch,
            &mut peer,
            "Coincident",
            Rule::Coincident { a, b },
        );
    }
    let pick_line = |w: &mut Window, drawing: &Sketch, i| {
        let (p, q) = ends(drawing, i);
        click(w, point((p.x + q.x) / 2., (p.y + q.y) / 2.), false);
    };
    for i in 0..4 {
        pick_line(&mut w, &peer, i);
        let (a, b) = (start(i), end(i));
        let (button, rule) = if i % 2 == 0 {
            ("Vertical", Rule::Vertical { a, b })
        } else {
            ("Horizontal", Rule::Horizontal { a, b })
        };
        add(&mut w, &mut ui, sketch, &mut peer, button, rule);
    }
    for (i, value) in [(1, 41.), (0, 14.25)] {
        let (p, q) = ends(&peer, i);
        let (a, b) = (start(i), end(i));
        pick_line(&mut w, &peer, i);
        let measured = (p.x - q.x).hypot(p.y - q.y);
        add(
            &mut w,
            &mut ui,
            sketch,
            &mut peer,
            "Distance",
            Rule::Distance {
                a,
                b,
                distance: literal(measured),
            },
        );
        ui.replace_dimension_text(&mut w, &value.to_string());
        ui.press(&mut w, "Set", false);
        peer.constraints.last_mut().expect("distance").rule = Rule::Distance {
            a,
            b,
            distance: literal(value),
        };
        solve(sketch, &mut peer);
    }
    let expected = Sketch {
        constraints: peer.constraints.clone(),
        ..saved.clone()
    };
    let mut from_storage = expected.clone();
    assert_eq!(solve(sketch, &mut from_storage), 0);
    assert_eq!(expected.constraints.len(), 11);
    for (a, b) in from_storage.curves.iter().zip(&peer.curves) {
        for (p, q) in rules::handles(a).iter().zip(rules::handles(b)) {
            assert_eq!(p.0, q.0);
            assert!((p.1.x - q.1.x).abs() < 1e-6 && (p.1.y - q.1.y).abs() < 1e-6);
        }
    }
    // Zero freedom: the rules alone also predict every displayed coordinate.
    let changes: Vec<_> = expected
        .constraints
        .iter()
        .map(|c| EditChange::AddConstraint {
            sketch,
            constraint: c.clone(),
        })
        .collect();
    let cli = root.join("constrained.fcad");
    cli_edit(path, &cli, base, &changes);
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, changes);
    publish(&mut w, request);
    let window = w.sessions.export_path().expect("published plate");
    same_model(&window, &cli, sketch, &expected, 0);
    let volume = (41. * 14.25 - removed_area) * 6.75;
    for output in [&window, &cli] {
        check_body(output, sketch, 1.3, volume);
    }
    // The older, off-grid pin remains expressible as a versioned CLI rule edit.
    let mut off_grid = changes;
    let EditChange::AddConstraint { constraint, .. } = &mut off_grid[0] else {
        panic!("fixed corner")
    };
    constraint.rule = Rule::Fixed {
        point: start(1),
        x: literal(36.5),
        y: literal(1.25),
    };
    let control = root.join("off-grid.fcad");
    cli_edit(path, &control, base, &off_grid);
    assert_eq!(stored(&control).1.curves, saved.curves);
    check_body(&control, sketch, 1.25, volume);
    assert_eq!(std::fs::read(path).expect("source"), source);
}

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_eval::solver_available(), "PLANEGCS"),
    ])
}

#[test]
fn native_viewport_constraints_keep_the_fillet_on_the_solved_plate() {
    if native() {
        let (root, path, _) = crate::fillets::tests::rounded(2.375);
        constrain_plate(
            &path,
            root.path(),
            (1. - std::f64::consts::PI / 4.) * 2.375_f64.powi(2),
        );
    }
}

#[test]
fn native_viewport_constraints_keep_the_chamfer_on_the_solved_plate() {
    if native() {
        let (root, path, _) = crate::chamfers::tests::chamfered(2.375);
        constrain_plate(&path, root.path(), 2.375_f64.powi(2) / 2.);
    }
}

#[test]
fn native_viewport_constraints_keep_both_fillets_on_the_solved_plate() {
    if native() {
        let (root, path, _) = crate::fillets::tests::rounded_twice(2.375, 3.0625);
        let corner = 1. - std::f64::consts::PI / 4.;
        constrain_plate(
            &path,
            root.path(),
            corner * (2.375_f64.powi(2) + 3.0625_f64.powi(2)),
        );
    }
}
