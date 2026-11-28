// SPDX-License-Identifier: MIT
//! Drive the viewport and independently advance the literal polygon edit with the public solver.
use super::constraint_widgets::press_rule;
use super::widgets::Ui;
use super::window::click;
use super::*;
use crate::sessions::tests::tabs::Window;
use ferritecad_document::{Expression, SketchConstraintRule as Rule};

pub(super) fn ends(sketch: &Sketch, i: usize) -> (Point2, Point2) {
    let SketchGeometry::Line { start, end } = sketch.curves[i].geometry else {
        panic!("a side of the plate")
    };
    (start, end)
}

pub(super) fn solve(sketch: ObjectId, drawing: &mut Sketch) -> usize {
    let (next, report) = ferritecad_eval::solve_sketch(drawing, sketch)
        .expect("consistent plate constraints")
        .expect("constrained sketch");
    *drawing = next;
    report.degrees_of_freedom()
}

// Only the new UUID is shared with the peer. Its rule and coordinates come from
// the original document, the widget action and the public sketch solver.
pub(super) fn add(
    w: &mut Window,
    ui: &mut Ui,
    sketch: ObjectId,
    peer: &mut Sketch,
    button: &str,
    rule: Rule,
) {
    press_rule(ui, w, button);
    let draft = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    let new = draft.state.constraints.last().expect("new rule");
    assert_eq!(new.rule, rule, "{button}");
    peer.constraints.push(SketchConstraint { id: new.id, rule });
    solve(sketch, peer);
}

fn handles(drawing: &Sketch, i: usize) -> (SketchPointRef, SketchPointRef) {
    let id = drawing.curves[i].id;
    (
        SketchPointRef::new(id, SketchPointSelector::Start),
        SketchPointRef::new(id, SketchPointSelector::End),
    )
}

fn pick_line(w: &mut Window, drawing: &Sketch, i: usize) {
    let (p, q) = ends(drawing, i);
    click(w, point((p.x + q.x) / 2., (p.y + q.y) / 2.), false);
}

pub(super) fn close(w: &mut Window, ui: &mut Ui, sketch: ObjectId, peer: &mut Sketch) {
    for i in 0..peer.curves.len() {
        let next = (i + 1) % peer.curves.len();
        let end = handles(peer, i).1;
        let start = handles(peer, next).0;
        let (a, b) = if next == 0 {
            (start, end)
        } else {
            (end, start)
        };
        let joint = ends(peer, i).1;
        click(w, joint, false);
        click(w, joint, true);
        add(w, ui, sketch, peer, "Coincident", Rule::Coincident { a, b });
    }
}

pub(super) fn orient(
    w: &mut Window,
    ui: &mut Ui,
    sketch: ObjectId,
    peer: &mut Sketch,
    i: usize,
    vertical: bool,
) {
    pick_line(w, peer, i);
    let (a, b) = handles(peer, i);
    let (button, rule) = if vertical {
        ("Vertical", Rule::Vertical { a, b })
    } else {
        ("Horizontal", Rule::Horizontal { a, b })
    };
    add(w, ui, sketch, peer, button, rule);
}

pub(super) fn distance(
    w: &mut Window,
    ui: &mut Ui,
    sketch: ObjectId,
    peer: &mut Sketch,
    i: usize,
    mm: f64,
) {
    let (p, q) = ends(peer, i);
    let (a, b) = handles(peer, i);
    pick_line(w, peer, i);
    let measured = (p.x - q.x).hypot(p.y - q.y);
    let rule = |v| Rule::Distance {
        a,
        b,
        distance: Expression::constant(v).expect("distance"),
    };
    add(w, ui, sketch, peer, "Distance", rule(measured));
    if measured != mm {
        ui.replace_dimension_text(w, &mm.to_string());
        ui.press(w, "Set", false);
        peer.constraints.last_mut().expect("distance").rule = rule(mm);
        solve(sketch, peer);
    }
}
