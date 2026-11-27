// SPDX-License-Identifier: MIT
use super::widgets::Ui;
use super::window::{apply, enter, finish, open};
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window};
use crate::sketch::{grid::Snap, guide::Mode};
use ferritecad_document::{Expression, SketchConstraintRule as Rule, SketchPointSelector as S};
use ferritecad_ui::{ModelEdit, PointerButton, ViewportEvent as E};

fn fixture(
    curves: Vec<SketchCurve>,
    rules: Vec<Rule>,
    drawn: Drawn,
) -> (fixtures::Drawing, Window, Ui) {
    let mut f = fixtures::Drawing::new(curves, false);
    let changes: Vec<_> = rules
        .into_iter()
        .map(|rule| EditChange::AddConstraint {
            sketch: f.sketch,
            constraint: SketchConstraint {
                id: StableEntityId::new(),
                rule,
            },
        })
        .collect();
    if !changes.is_empty() {
        f.doc.write_changes(&changes).expect("saved constraints");
    }
    let mut w = open(&f.path, drawn);
    enter(&mut w, f.sketch, ModelEdit::Sketch);
    let ui = Ui::new(&mut w);
    (f, w, ui)
}

fn d(w: &Window) -> &Draft {
    w.creates.sketch.viewport.draft.as_ref().expect("open form")
}
fn event(w: &mut Window, event: E) {
    let editable = input_editable(&w.creates, &w.edits, &w.sessions);
    w.creates
        .sketch
        .viewport
        .route(&mut w.input, event, false, editable, false);
}
pub(super) fn movement(w: &mut Window, target: Point2) -> Point2 {
    let frame = d(w).planes[d(w).plane].frame.expect("built plane");
    let pixel = geometry::screen(w.input.camera(), frame, target).expect("on screen");
    let window = w.input.region().window((pixel[0] as f32, pixel[1] as f32));
    event(
        w,
        E::PointerMoved {
            x: window.0,
            y: window.1,
        },
    );
    let (x, y) = w.input.region().local(window);
    geometry::point_on_plane(w.input.camera(), [f64::from(x), f64::from(y)], frame)
        .expect("actual pointer on plane")
}
pub(super) fn frame(w: &mut Window) {
    assert!(w.creates.sketch.viewport.take_hover(&mut w.input));
}
pub(super) fn press(w: &mut Window, target: Point2) {
    movement(w, target);
    event(w, E::PointerPressed(PointerButton::Primary));
}
pub(super) fn release(w: &mut Window) {
    event(w, E::PointerReleased(PointerButton::Primary));
    frame(w);
}
fn at(w: &Window, handle: SketchPointRef) -> Point2 {
    rules::position(&d(w).state.curves, handle).expect("named handle")
}
fn near(actual: Point2, expected: Point2) {
    assert!(
        (actual.x - expected.x).abs() <= 1e-6 && (actual.y - expected.y).abs() <= 1e-6,
        "{actual:?}, expected {expected:?}"
    );
}
fn counts() -> (usize, usize, usize) {
    (
        drag::STARTS.with(|v| v.get()),
        drag::SAMPLES.with(|v| v.get()),
        draft::SOLVES.with(|v| v.get()),
    )
}
fn reset() {
    drag::STARTS.with(|v| v.set(0));
    drag::SAMPLES.with(|v| v.set(0));
    draft::SOLVES.with(|v| v.set(0));
}
fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[
        (ferritecad_eval::solver_available(), "PlaneGCS"),
        (ferritecad_occt::is_available(), "OCCT"),
    ])
}
fn horizontal() -> (SketchCurve, SketchPointRef, SketchPointRef, Vec<Rule>) {
    let side = line([-10., 0.], [10., 0.]);
    let a = SketchPointRef::new(side.id, S::Start);
    let b = SketchPointRef::new(side.id, S::End);
    let fixed = Rule::Fixed {
        point: a,
        x: Expression::constant(-10.).expect("fixture value"),
        y: Expression::constant(0.).expect("fixture value"),
    };
    (side, a, b, vec![fixed, Rule::Horizontal { a, b }])
}

#[test]
fn fixed_end_stays_and_free_end_slides_on_its_horizontal_through_finish() {
    if !native() {
        return;
    }
    let (side, a, b, rules) = horizontal();
    let (f, mut w, mut ui) = fixture(vec![side], rules, Drawn::Native);
    reset();
    press(&mut w, point(-10., 0.));
    movement(&mut w, point(-14., 5.));
    release(&mut w);
    near(at(&w, a), point(-10., 0.));
    assert_eq!(d(&w).message, "");
    assert!(d(&w).undo.is_empty());
    assert_eq!(counts(), (1, 1, 0));
    press(&mut w, point(10., 0.));
    for target in [point(14., 3.), point(8., -2.), point(15., 1.)] {
        let actual = movement(&mut w, target);
        frame(&mut w);
        near(at(&w, b), point(actual.x, 0.));
        near(at(&w, a), point(-10., 0.));
    }
    release(&mut w);
    assert_eq!(d(&w).undo.len(), 1);
    let shown = at(&w, b);
    let req = finish(&mut w, &mut ui);
    assert!(apply(&mut w, req));
    let source = w.scene.edit_source.as_ref().expect("accepted");
    let accepted = w
        .scene
        .sketches
        .presentation(source, f.sketch)
        .expect("drawing");
    let SketchGeometry::Line { end, .. } = accepted.curves()[0].geometry() else {
        unreachable!()
    };
    near(*end, shown);
}

#[test]
fn a_dragged_rectangle_keeps_its_corners_and_one_history_step_through_finish() {
    if !native() {
        return;
    }
    let corners = [[-10., -6.], [10., -6.], [10., 6.], [-10., 6.]];
    let sides: Vec<_> = (0..4)
        .map(|i| line(corners[i], corners[(i + 1) % 4]))
        .collect();
    let mut constraints = Vec::new();
    for i in 0..4 {
        let a = SketchPointRef::new(sides[i].id, S::Start);
        let b = SketchPointRef::new(sides[i].id, S::End);
        constraints.push(if i % 2 == 0 {
            Rule::Horizontal { a, b }
        } else {
            Rule::Vertical { a, b }
        });
        constraints.push(Rule::Coincident {
            a: b,
            b: SketchPointRef::new(sides[(i + 1) % 4].id, S::Start),
        });
    }
    let (f, mut w, mut ui) = fixture(sides, constraints, Drawn::Native);
    let before = d(&w).state.clone();
    reset();
    press(&mut w, point(-10., -6.));
    movement(&mut w, point(-13., -8.));
    movement(&mut w, point(-14., -10.));
    let expected = movement(&mut w, point(-15., -11.));
    assert_eq!(counts(), (0, 0, 0));
    frame(&mut w);
    frame(&mut w);
    release(&mut w);
    assert_eq!(counts(), (1, 1, 0));
    let shown = d(&w).state.clone();
    assert_eq!(shown.selected, before.selected);
    assert_eq!(shown.report, before.report);
    for i in 0..4 {
        let SketchGeometry::Line { start, end } = shown.curves[i].geometry else {
            unreachable!()
        };
        let SketchGeometry::Line { start: next, .. } = shown.curves[(i + 1) % 4].geometry else {
            unreachable!()
        };
        near(end, next);
        if i == 0 {
            near(start, expected);
        }
        if i % 2 == 0 {
            assert!((start.y - end.y).abs() < 1e-6);
        } else {
            assert!((start.x - end.x).abs() < 1e-6);
        }
    }
    assert_eq!(d(&w).undo.len(), 1);
    ui.press(&mut w, "Undo", true);
    assert_eq!(before, d(&w).state);
    ui.press(&mut w, "Redo", true);
    assert_eq!(shown, d(&w).state);
    assert_eq!(counts(), (1, 1, 0));
    let req = finish(&mut w, &mut ui);
    assert!(apply(&mut w, req));
    let source = w.scene.edit_source.as_ref().expect("accepted");
    let resolved = w
        .scene
        .sketches
        .presentation(source, f.sketch)
        .expect("accepted sketch");
    for (c, got) in shown.curves.iter().zip(resolved.curves()) {
        assert_eq!(c.id, got.id());
        assert!(super::super::prediction::agrees(
            &c.geometry,
            got.geometry()
        ));
    }
}

#[test]
fn an_unconstrained_handle_moves_alone_and_does_not_snap_to_itself() {
    let side = line([0., 0.], [10., 0.]);
    let separate = line([10., 0.], [10., 10.]);
    let start = SketchPointRef::new(side.id, S::Start);
    let end = SketchPointRef::new(side.id, S::End);
    let other = SketchPointRef::new(separate.id, S::Start);
    let (_f, mut w, _ui) = fixture(vec![side, separate], vec![], Drawn::Mock);
    reset();
    press(&mut w, point(10., 0.));
    let first = movement(&mut w, point(16., 4.));
    frame(&mut w);
    near(at(&w, end), first);
    near(at(&w, other), point(10., 0.));
    release(&mut w);
    // Move farther than click slop but within the old position's snap aperture.
    press(&mut w, point(0., 0.));
    let (x, y) = w.input.pointer_position().expect("fixture value");
    event(&mut w, E::PointerMoved { x: x + 6., y });
    release(&mut w);
    assert!(at(&w, start).x > 0.1);
    assert_eq!(counts(), (0, 0, 0));
    assert_eq!(d(&w).undo.len(), 2);
}

#[test]
fn a_handle_click_and_idle_hover_select_without_starting_a_solver() {
    let (side, _, end, rules) = horizontal();
    let rules = if ferritecad_eval::solver_available() {
        rules
    } else {
        Vec::new()
    };
    let (_f, mut w, _ui) = fixture(vec![side], rules, Drawn::Mock);
    reset();
    movement(&mut w, point(10., 0.));
    frame(&mut w);
    press(&mut w, point(10., 0.));
    release(&mut w);
    assert_eq!(d(&w).state.selected, vec![Selection::Point(end)]);
    assert_eq!(counts(), (0, 0, 0));
    assert!(d(&w).undo.is_empty());
}

#[test]
fn escape_and_focus_loss_restore_the_press_without_touching_history() {
    let side = line([0., 0.], [10., 0.]);
    let (_f, mut w, mut ui) = fixture(vec![side], vec![], Drawn::Mock);
    let before = d(&w).state.clone();
    for escape in [true, false] {
        press(&mut w, point(10., 0.));
        movement(&mut w, point(15., 4.));
        frame(&mut w);
        assert_ne!(d(&w).state, before);
        if escape {
            ui.key(&mut w, egui::Key::Escape);
        } else {
            event(&mut w, E::GestureCancelled);
        }
        assert_eq!(d(&w).state, before);
        release(&mut w);
        assert!(d(&w).undo.is_empty());
    }
}

fn new_line_for_transition() -> (tempfile::TempDir, Window, Ui) {
    let root = tempfile::tempdir().expect("documents");
    let file = root.path().join("empty.fcad");
    ferritecad_document::Document::create(&file)
        .expect("Empty")
        .close()
        .expect("close");
    let mut w = open(&file, Drawn::Mock);
    let mut ui = Ui::new(&mut w);
    window::start_new(&mut w, &mut ui);
    ui.press(&mut w, "Line", false);
    window::click(&mut w, point(-10., 0.), false);
    window::click(&mut w, point(10., 0.), false);
    ui.press(&mut w, "Select", false);
    (root, w, ui)
}

fn choose_file(w: &mut Window, file: &std::path::Path) -> Option<std::path::PathBuf> {
    crate::choose_open(
        &mut w.creates,
        &w.tabs,
        &mut w.sessions,
        &w.edits,
        &mut w.input,
        &crate::restores::Restores::default(),
        file.into(),
    )
}

fn quit(w: &mut Window) -> bool {
    crate::begin_window_quit(
        &mut w.tabs,
        &mut w.sessions,
        &w.creates,
        &crate::Loads::default(),
        &crate::exports::Exports::default(),
        &w.edits,
        &w.input,
    )
}

#[test]
fn open_waits_for_the_viewport_sketch_handle() {
    let (root, mut w, mut ui) = new_line_for_transition();
    let other = root.path().join("other.fcad");
    ferritecad_document::Document::create(&other)
        .expect("other")
        .close()
        .expect("close");
    for escape in [true, false] {
        press(&mut w, point(10., 0.));
        assert!(w.creates.sketch.viewport.gesturing());
        assert!(!w.input.is_dragging());
        assert_eq!(choose_file(&mut w, &other), None);
        movement(&mut w, point(14., 3.));
        frame(&mut w);
        assert_eq!(choose_file(&mut w, &other), None);
        assert!(w.creates.asking().is_none());
        if escape {
            ui.key(&mut w, egui::Key::Escape);
        } else {
            event(&mut w, E::GestureCancelled);
        }
        assert!(!w.creates.sketch.viewport.gesturing());
        assert_eq!(choose_file(&mut w, &other).as_ref(), Some(&other));
    }
    press(&mut w, point(10., 0.));
    movement(&mut w, point(15., 3.));
    event(&mut w, E::PointerReleased(PointerButton::Primary));
    assert!(!w.creates.sketch.viewport.gesturing());
    frame(&mut w);
    assert_eq!(choose_file(&mut w, &other), Some(other));
}

#[test]
fn quit_waits_for_the_viewport_sketch_handle() {
    let (_root, mut w, _ui) = new_line_for_transition();
    press(&mut w, point(10., 0.));
    assert!(!quit(&mut w));
    let moved = movement(&mut w, point(14., 3.));
    frame(&mut w);
    let SketchGeometry::Line { end, .. } = d(&w).state.curves[0].geometry else {
        panic!("line");
    };
    near(end, moved);
    assert!(!quit(&mut w));
    assert!(!w.tabs.quitting());
    release(&mut w);
    assert!(quit(&mut w));
    assert!(w.tabs.quitting());
}

#[test]
fn empty_space_and_curve_bodies_keep_the_orbit_gesture() {
    let (_f, mut w, _ui) = fixture(vec![line([-10., 0.], [10., 0.])], vec![], Drawn::Mock);
    let original = d(&w).state.clone();
    for hit in [point(0., 0.), point(8., 8.)] {
        let before = *w.input.camera();
        press(&mut w, hit);
        assert!(!w.creates.sketch.viewport.gesturing());
        assert!(w.input.is_dragging(), "the view owns this press");
        movement(&mut w, point(15., 6.));
        release(&mut w);
        assert_ne!(before, *w.input.camera());
        assert_eq!(d(&w).state, original);
    }
}

#[test]
fn constrained_drag_in_a_build_without_the_solver_keeps_the_drawing() {
    if ferritecad_eval::solver_available() {
        return;
    }
    // Start with a built drawing, then exercise the no-solver form with its saved rules.
    let (side, _a, _b, constraints) = horizontal();
    let (_f, mut w, _ui) = fixture(vec![side], vec![], Drawn::Mock);
    let draft = w
        .creates
        .sketch
        .viewport
        .draft
        .as_mut()
        .expect("fixture value");
    draft.state.constraints = constraints
        .into_iter()
        .map(|rule| SketchConstraint {
            id: StableEntityId::new(),
            rule,
        })
        .collect();
    let before = d(&w).state.clone();
    press(&mut w, point(10., 0.));
    movement(&mut w, point(14., 3.));
    release(&mut w);
    assert_eq!(d(&w).state, before);
    assert!(d(&w).undo.is_empty());
    assert_eq!(
        d(&w).message,
        "This build has no sketch solver, so constrained drawings cannot be dragged."
    );
}

#[test]
fn a_line_uses_the_grid_then_its_latest_vertex_and_an_exact_object_snap() {
    let target = point(10.3, 4.1);
    let (_f, mut w, mut ui) = fixture(
        vec![curve(SketchGeometry::Point { at: target })],
        vec![],
        Drawn::Mock,
    );
    ui.press(&mut w, "Line", false);
    ui.press(&mut w, "Off", false);
    ui.press(&mut w, "5 mm", false);
    ui.press(&mut w, "Off", true);
    ui.press(&mut w, "Orthogonal", false);
    assert_eq!(w.creates.sketch.viewport.snapping.grid, Snap::Five);
    assert_eq!(w.creates.sketch.viewport.snapping.guides, Mode::Orthogonal);
    for p in [point(-13.1, -4.1), point(-3.1, -4.7), target] {
        window::click(&mut w, p, false);
    }
    let curves = &d(&w).state.curves;
    let SketchGeometry::Line { start, end } = curves[1].geometry else {
        unreachable!()
    };
    assert_eq!(start, point(-15., -5.));
    assert_eq!(end.y, -5.);
    assert!((end.x + 3.1).abs() < 1e-4);
    let SketchGeometry::Line { start, end: to } = curves[2].geometry else {
        unreachable!()
    };
    assert_eq!(start, end);
    assert_eq!(to, target);
    assert!(d(&w).state.constraints.is_empty());
}

#[test]
fn consecutive_drags_released_between_frames_keep_both_steps_in_order() {
    let side = line([-10., 0.], [10., 0.]);
    let a = SketchPointRef::new(side.id, S::Start);
    let b = SketchPointRef::new(side.id, S::End);
    let (_f, mut w, _ui) = fixture(vec![side], vec![], Drawn::Mock);
    press(&mut w, point(-10., 0.));
    let target_a = movement(&mut w, point(-15., 3.));
    event(&mut w, E::PointerReleased(PointerButton::Primary));
    press(&mut w, point(10., 0.));
    let target_b = movement(&mut w, point(16., 4.));
    event(&mut w, E::PointerReleased(PointerButton::Primary));
    frame(&mut w);
    frame(&mut w);
    near(at(&w, a), target_a);
    near(at(&w, b), target_b);
    assert_eq!(d(&w).undo.len(), 2);
    w.creates.sketch.viewport.history(true);
    near(at(&w, a), target_a);
    near(at(&w, b), point(10., 0.));
}

#[test]
fn returning_the_handle_to_its_start_creates_no_history_step() {
    let side = line([-10., 0.], [10., 0.]);
    let end = SketchPointRef::new(side.id, S::End);
    let (_f, mut w, _ui) = fixture(vec![side], vec![], Drawn::Mock);
    w.creates.sketch.viewport.snapping.grid = Snap::One;
    let before = d(&w).state.clone();
    let camera = *w.input.camera();
    press(&mut w, point(10., 0.));
    movement(&mut w, point(15., 4.));
    frame(&mut w);
    near(at(&w, end), point(15., 4.));
    movement(&mut w, point(10., 0.));
    release(&mut w);
    assert_eq!(d(&w).state, before);
    assert!(d(&w).undo.is_empty());
    assert_eq!(camera, *w.input.camera());
}

#[test]
fn a_contradictory_drawing_reports_the_same_named_rules_at_drag_start_as_on_rebuild() {
    if !native() {
        return;
    }
    let (side, a, _b, rules) = horizontal();
    let (_f, mut w, _ui) = fixture(vec![side], rules, Drawn::Native);
    let draft = w
        .creates
        .sketch
        .viewport
        .draft
        .as_mut()
        .expect("open drawing");
    draft.state.constraints.push(SketchConstraint {
        id: StableEntityId::new(),
        rule: Rule::Fixed {
            point: a,
            x: Expression::constant(20.).expect("x"),
            y: Expression::constant(0.).expect("y"),
        },
    });
    let stated = Sketch {
        plane: draft.planes[draft.plane].id,
        curves: draft.state.curves.clone(),
        constraints: draft.state.constraints.clone(),
    };
    let failure =
        ferritecad_eval::solve_sketch(&stated, draft.sketch).expect_err("conflicting fixes");
    let facts = ferritecad_eval::SketchConflict::of(&failure).expect("named conflicting rules");
    assert!(!facts.constraints().is_empty());
    let expected = draft::solve_message(&failure);
    let before = draft.state.clone();
    reset();
    press(&mut w, point(10., 0.));
    movement(&mut w, point(15., 4.));
    release(&mut w);
    assert_eq!(d(&w).message, expected);
    assert_eq!(d(&w).state, before);
    assert!(d(&w).undo.is_empty());
    assert_eq!(counts(), (1, 0, 0));
}

#[test]
fn a_coincident_corner_follows_pointer_steps_shorter_than_the_snap_aperture() {
    if !native() {
        return;
    }
    let first = line([-10., -6.], [10., -6.]);
    let last = line([-10., 6.], [-10., -6.]);
    let corner = SketchPointRef::new(first.id, S::Start);
    let partner = SketchPointRef::new(last.id, S::End);
    let rules = vec![Rule::Coincident {
        a: partner,
        b: corner,
    }];
    let (_f, mut w, _ui) = fixture(vec![first, last], rules, Drawn::Native);
    press(&mut w, point(-10., -6.));
    let (x, y) = w.input.pointer_position().expect("pressed pointer");
    // Each step passes the click slop but stays inside the partner's snap aperture.
    for step in 1..=3u8 {
        let x = x - 6. * f32::from(step);
        event(&mut w, E::PointerMoved { x, y });
        frame(&mut w);
        let plane = d(&w).planes[d(&w).plane].frame.expect("built plane");
        let (lx, ly) = w.input.region().local((x, y));
        let pointer =
            geometry::point_on_plane(w.input.camera(), [f64::from(lx), f64::from(ly)], plane)
                .expect("pointer on plane");
        near(at(&w, corner), pointer);
        near(at(&w, partner), pointer);
    }
    release(&mut w);
    assert_eq!(d(&w).undo.len(), 1);
}

#[test]
fn delete_during_a_drag_keeps_the_dragged_curve_and_one_history_step() {
    let dot = curve(SketchGeometry::Point { at: point(4., 2.) });
    let handle = SketchPointRef::new(dot.id, S::At);
    let (_f, mut w, mut ui) = fixture(vec![dot], vec![], Drawn::Mock);
    press(&mut w, point(4., 2.));
    release(&mut w);
    assert_eq!(d(&w).state.selected, vec![Selection::Point(handle)]);
    press(&mut w, point(4., 2.));
    movement(&mut w, point(9., 5.));
    frame(&mut w);
    ui.key(&mut w, egui::Key::Delete);
    let target = movement(&mut w, point(12., 6.));
    frame(&mut w);
    release(&mut w);
    near(at(&w, handle), target);
    assert_eq!(d(&w).undo.len(), 1);
}
