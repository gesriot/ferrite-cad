// SPDX-License-Identifier: MIT
//! §31I: the Circle and annulus canvas through the production forms and real egui
//! events (no window, no GPU, no kernel). Every gesture starts with a
//! `PointerButton` press on a handle the previous frame painted.
#![allow(clippy::panic)]
use super::{
    analytic::{Handle, Profile, SIZE, Strings},
    tests::{click, frame, frame_running, text_at},
    *,
};

pub(super) fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    ctx
}

fn event(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

fn press(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
    frame(ctx, e, vec![egui::Event::PointerMoved(at)]);
    frame(ctx, e, vec![event(at, true)]);
}

fn move_to(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) -> egui::FullOutput {
    frame(ctx, e, vec![egui::Event::PointerMoved(at)])
}

fn release(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
    frame(ctx, e, vec![event(at, false)]);
}

/// Press, move and release in separate frames, as a hand does.
fn drag(ctx: &egui::Context, e: &mut Editor, from: egui::Pos2, to: egui::Pos2) {
    press(ctx, e, from);
    move_to(ctx, e, to);
    release(ctx, e, to);
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}

/// Whether the form shows its annular canvas (else its circle canvas).
fn annular(e: &Editor) -> bool {
    e.mode == Mode::Annulus
}

/// The strings the canvas reads and writes, as the form holds them.
pub(super) fn shown(e: &Editor) -> Strings {
    if annular(e) {
        Strings {
            centre: e.annulus.center.clone(),
            outer: e.annulus.outer_radius.clone(),
            bore: Some(e.annulus.inner_radius.clone()),
        }
    } else {
        Strings {
            centre: e.circle.center.clone(),
            outer: e.circle.radius.clone(),
            bore: None,
        }
    }
}

fn strings(centre: [&str; 2], outer: &str, bore: Option<&str>) -> Strings {
    Strings {
        centre: centre.map(str::to_owned),
        outer: outer.to_owned(),
        bore: bore.map(str::to_owned),
    }
}

fn profile(e: &Editor) -> Result<Profile, String> {
    let mut s = shown(e);
    Profile::read(&analytic::Fields {
        centre: &mut s.centre,
        outer: &mut s.outer,
        bore: s.bore.as_mut(),
    })
}

fn canvas(e: &Editor) -> &analytic::Canvas {
    if annular(e) {
        &e.annulus_canvas
    } else {
        &e.circle_canvas
    }
}

/// The depth of the shown profile's draft Undo and Redo.
pub(crate) fn depth(e: &Editor) -> (usize, usize) {
    if annular(e) {
        (e.annulus_undo.len(), e.annulus_redo.len())
    } else {
        (e.circle_undo.len(), e.circle_redo.len())
    }
}

/// The painted canvas: its background, the one rectangle of exactly its size.
pub(super) fn canvas_rect(out: &egui::FullOutput) -> egui::Rect {
    out.shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Rect(r) if (r.rect.size() - SIZE).length() < 0.01 => Some(r.rect),
            _ => None,
        })
        .expect("painted canvas")
}

/// Where the shown profile's `which` handle is painted now.
pub(super) fn handle(ctx: &egui::Context, e: &mut Editor, which: Handle) -> egui::Pos2 {
    let rect = canvas_rect(&frame(ctx, e, vec![]));
    let view = canvas(e).view.expect("a fitted view");
    let at = profile(e)
        .expect("a drawable profile")
        .handles()
        .into_iter()
        .find(|(h, _)| *h == which)
        .unwrap_or_else(|| panic!("no {which:?} handle"))
        .1;
    view.screen(rect, at)
}

fn scale(e: &Editor) -> f64 {
    f64::from(canvas(e).view.expect("a fitted view").scale)
}

/// The circles painted on the canvas: (centre, radius in points, colour).
fn circles(out: &egui::FullOutput) -> Vec<(egui::Pos2, f32, egui::Color32)> {
    let rect = canvas_rect(out);
    out.shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Circle(s)
                if c.clip_rect == rect
                    && s.stroke.width == 2.
                    && s.fill == egui::Color32::TRANSPARENT =>
            {
                Some((s.center, s.radius, s.stroke.color))
            }
            _ => None,
        })
        .collect()
}

fn painted(out: &egui::FullOutput, text: &str) -> bool {
    out.shapes.iter().any(|c| match &c.shape {
        egui::Shape::Text(t) => t.galley.text() == text,
        _ => false,
    })
}

/// The window's New → `profile` (Circle or Circle with hole), through its buttons.
pub(crate) fn open_new(ctx: &egui::Context, e: &mut Editor, profile: &str) {
    e.set_create(true);
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, "Create sketch + Extrude…"));
    for _ in 0..3 {
        frame(ctx, e, vec![]);
    }
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, profile));
    for _ in 0..2 {
        frame(ctx, e, vec![]);
    }
}

/// A saved Circle (`radii` of one) or annulus, read without a kernel, and its form
/// opened as the window opens it for an idle, clean session.
pub(crate) fn saved(radii: &[f64]) -> (tempfile::TempDir, egui::Context, Editor) {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("round.fcad");
    crate::sketch::tests::analytic_apply::write_round(&path, [12.5, -7.25], radii, 15.25, false);
    let reading = crate::sketch_facts::read(&path).expect("reading");
    let mut e = Editor::default();
    e.set_session(true, false, false);
    e.set_analytic_apply(true);
    let opened = if radii.len() == 1 {
        e.begin_circle_edit(&path, &reading, reading.circle_sketches[0].sketch)
    } else {
        e.begin_annulus_edit(&path, &reading, reading.annulus_sketches[0].sketch)
    };
    assert!(opened, "the form opens");
    let ctx = context();
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    (root, ctx, e)
}

/// Replaces the text of the box on the row labelled `label` by `value`, through the
/// widgets: the boxes start right of the widest label of the grid.
pub(crate) fn type_into(ctx: &egui::Context, e: &mut Editor, label: &str, value: &str) {
    let out = frame(ctx, e, vec![]);
    let column = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t)
                if matches!(
                    t.galley.text(),
                    "Center X"
                        | "Center Y"
                        | "Radius"
                        | "Outer radius"
                        | "Inner radius"
                        | "Blind height"
                ) =>
            {
                Some(t.visual_bounding_rect().right())
            }
            _ => None,
        })
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(column.is_finite(), "no number grid");
    let row = text_at(&out, label).y;
    click(ctx, e, egui::pos2(column + 40., row));
    let mut events = vec![
        egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        },
        key(egui::Key::Backspace),
    ];
    if !value.is_empty() {
        events.push(egui::Event::Text(value.into()));
    }
    frame(ctx, e, events);
}

fn press_button(ctx: &egui::Context, e: &mut Editor, label: &str) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, label));
}

/// The pointer's movement between two positions, as the canvas measures it (mm).
fn moved(e: &Editor, from: egui::Pos2, to: egui::Pos2) -> [f64; 2] {
    [
        (f64::from(to.x) - f64::from(from.x)) / scale(e),
        (f64::from(from.y) - f64::from(to.y)) / scale(e),
    ]
}

#[test]
fn a_new_circle_moves_its_centre_and_radius_by_real_events_one_undo_step_each() {
    let ctx = context();
    let mut e = Editor::default();
    open_new(&ctx, &mut e, "Circle");
    let start = shown(&e);
    assert_eq!(start, strings(["0", "0"], "10", None));
    let out = frame(&ctx, &mut e, vec![]);
    let rect = canvas_rect(&out);
    // First open: the whole circle, centred, with margins.
    let painted = circles(&out);
    assert_eq!(painted.len(), 1, "{painted:?}");
    assert!((painted[0].0 - rect.center()).length() < 0.01);
    assert!((f64::from(painted[0].1) - 10. * scale(&e)).abs() < 1e-3);
    assert!(rect.shrink(29.).contains_rect(egui::Rect::from_center_size(
        painted[0].0,
        egui::Vec2::splat(2. * painted[0].1)
    )));

    // The centre, pressed 3 points right of and 2 above it, dragged diagonally: it
    // moves by the pointer's movement, not to the pointer.
    let centre = handle(&ctx, &mut e, Handle::Centre);
    let grab = centre + egui::vec2(3., -2.);
    press(&ctx, &mut e, grab);
    assert!(e.gesturing() && canvas(&e).held() == Some(Handle::Centre));
    let to = grab + egui::vec2(37.5, -22.5);
    move_to(&ctx, &mut e, to);
    assert_eq!(depth(&e), (0, 0), "a preview is no step");
    release(&ctx, &mut e, to);
    assert!(!e.gesturing());
    let d = moved(&e, grab, to);
    assert!((d[0] * scale(&e) - 37.5).abs() < 1e-3 && (d[1] * scale(&e) - 22.5).abs() < 1e-3);
    assert_eq!(e.circle.center, [d[0].to_string(), d[1].to_string()]);
    let at = handle(&ctx, &mut e, Handle::Centre);
    assert!(
        ((at - to) - egui::vec2(-3., 2.)).length() < 1e-3,
        "the centre jumped under the pointer: {at:?} against {to:?}"
    );
    assert_eq!(
        (e.circle.radius.as_str(), e.circle.height.as_str()),
        ("10", "10")
    );
    assert_eq!(depth(&e), (1, 0));
    let centred = shown(&e);

    // The radius handle, dragged diagonally: the Euclidean distance from the centre,
    // never the X projection; the centre keeps its text.
    let r_at = handle(&ctx, &mut e, Handle::Outer);
    let r_to = r_at + egui::vec2(20., -15.);
    drag(&ctx, &mut e, r_at, r_to);
    let d = moved(&e, r_at, r_to);
    let radius = (10. + d[0]).hypot(d[1]);
    assert_eq!(e.circle.radius, radius.to_string());
    assert!((radius - (10. + d[0])).abs() > 0.1, "an X projection");
    assert_eq!(e.circle.center, centred.centre);
    assert_eq!(depth(&e), (2, 0));
    let resized = shown(&e);

    // One Undo per gesture, back to the exact strings; Redo returns them.
    press_button(&ctx, &mut e, "Undo draft");
    assert_eq!((shown(&e), depth(&e)), (centred.clone(), (1, 1)));
    press_button(&ctx, &mut e, "Undo draft");
    assert_eq!((shown(&e), depth(&e)), (start.clone(), (0, 2)));
    press_button(&ctx, &mut e, "Redo draft");
    press_button(&ctx, &mut e, "Redo draft");
    assert_eq!((shown(&e), depth(&e)), (resized.clone(), (2, 0)));

    // A press that does not move is no step and keeps the Redo.
    press_button(&ctx, &mut e, "Undo draft");
    let at = handle(&ctx, &mut e, Handle::Outer);
    press(&ctx, &mut e, at);
    release(&ctx, &mut e, at);
    assert_eq!((shown(&e), depth(&e)), (centred, (1, 1)));
    press_button(&ctx, &mut e, "Redo draft");

    // Create hands the job exactly the circle the canvas shows.
    press_button(&ctx, &mut e, CREATE_CIRCLE);
    let NewDocument::CircleExtrude(c) = e.take_request().expect("one request") else {
        panic!("not a circle");
    };
    assert_eq!(
        c,
        CircleExtrusion::new(
            [
                resized.centre[0].parse().expect("x"),
                resized.centre[1].parse().expect("y"),
            ],
            radius,
            10.,
        )
        .expect("valid")
    );
}

#[test]
fn a_new_annulus_changes_each_radius_alone_and_every_profile_keeps_its_draft() {
    let ctx = context();
    let mut e = Editor::default();
    open_new(&ctx, &mut e, "Circle with hole");
    assert_eq!(shown(&e), strings(["0", "0"], "10", Some("4")));
    let out = frame(&ctx, &mut e, vec![]);
    let colours: Vec<_> = circles(&out).iter().map(|c| c.2).collect();
    assert_eq!(colours.len(), 2, "the bounding circle and the bore");
    assert_ne!(colours[0], colours[1], "the two roles look alike");

    // Outer: only the outer radius.
    let at = handle(&ctx, &mut e, Handle::Outer);
    let to = at + egui::vec2(12.5, 7.5);
    drag(&ctx, &mut e, at, to);
    let d = moved(&e, at, to);
    let outer = (10. + d[0]).hypot(d[1]).to_string();
    assert_eq!(shown(&e), strings(["0", "0"], &outer, Some("4")));
    // Bore: only the inner radius, measured from the same centre, north handle.
    let at = handle(&ctx, &mut e, Handle::Bore);
    let to = at + egui::vec2(-6.25, -10.);
    drag(&ctx, &mut e, at, to);
    let d = moved(&e, at, to);
    let bore = d[0].hypot(4. + d[1]).to_string();
    assert_eq!(shown(&e), strings(["0", "0"], &outer, Some(&bore)));
    // Centre: both radii keep their text.
    let at = handle(&ctx, &mut e, Handle::Centre);
    let to = at + egui::vec2(-20., 12.5);
    drag(&ctx, &mut e, at, to);
    let d = moved(&e, at, to);
    let annulus = strings([&d[0].to_string(), &d[1].to_string()], &outer, Some(&bore));
    assert_eq!(shown(&e), annulus);
    assert_eq!(depth(&e), (3, 0));
    assert_eq!(e.annulus.height, "10");

    // The other profiles keep their drafts, histories and views across switches.
    press_button(&ctx, &mut e, "Line polygon");
    assert!(e.draft.as_ref().expect("the polygon").points.is_empty());
    press_button(&ctx, &mut e, "Circle");
    assert_eq!(
        (shown(&e), depth(&e)),
        (strings(["0", "0"], "10", None), (0, 0))
    );
    let at = handle(&ctx, &mut e, Handle::Outer);
    drag(&ctx, &mut e, at, at + egui::vec2(10., 0.));
    let circle = shown(&e);
    assert_eq!(depth(&e), (1, 0));
    let circle_view = e.circle_canvas.view;
    press_button(&ctx, &mut e, "Circle with hole");
    assert_eq!((shown(&e), depth(&e)), (annulus.clone(), (3, 0)));
    press_button(&ctx, &mut e, "Undo draft");
    assert_eq!(shown(&e).centre, ["0", "0"], "Undo is the annulus's own");
    assert_eq!(depth(&e), (2, 1));
    press_button(&ctx, &mut e, "Circle");
    assert_eq!((shown(&e), depth(&e)), (circle, (1, 0)));
    assert_eq!(e.circle_canvas.view, circle_view);
    press_button(&ctx, &mut e, "Circle with hole");
    press_button(&ctx, &mut e, "Redo draft");
    assert_eq!(shown(&e), annulus);

    // Create gets the shown annulus; Cancel ends every draft and every canvas.
    press_button(&ctx, &mut e, CREATE_ANNULUS);
    let NewDocument::AnnularExtrude(a) = e.take_request().expect("one request") else {
        panic!("not an annulus");
    };
    let n = |s: &str| s.parse::<f64>().expect("number");
    assert_eq!(
        a,
        AnnularExtrusion::new(
            [n(&annulus.centre[0]), n(&annulus.centre[1])],
            n(&outer),
            n(&bore),
            10.
        )
        .expect("valid")
    );
    press_button(&ctx, &mut e, "Cancel draft");
    assert!(!e.active() && e.annulus_canvas.view.is_none() && e.circle_canvas.view.is_none());
}

#[test]
fn saved_forms_take_a_gesture_as_one_confirmed_step_and_apply_exactly_its_numbers() {
    // Circle: the gesture is the confirmed draft; nothing has to be confirmed again.
    let (_root, ctx, mut e) = saved(&[10.5]);
    let stored = shown(&e);
    assert_eq!(stored, strings(["12.5", "-7.25"], "10.5", None));
    let (curve, sketch) = {
        let (request, _) = e.editing_circle.as_ref().expect("form");
        (
            crate::transaction_tests::circle(&request).curve_id,
            crate::transaction_tests::sketch(&request),
        )
    };
    let at = handle(&ctx, &mut e, Handle::Centre);
    let to = at + egui::vec2(-31.25, 18.75);
    drag(&ctx, &mut e, at, to);
    let d = moved(&e, at, to);
    let centre = [(12.5 + d[0]).to_string(), (-7.25 + d[1]).to_string()];
    assert_eq!(e.circle.center, centre);
    assert_eq!(depth(&e), (1, 0));
    assert_eq!(
        e.circle_applied.as_ref(),
        Some(&e.circle),
        "a second Confirm is needed"
    );
    press_button(&ctx, &mut e, "Confirm draft numbers");
    assert_eq!(depth(&e), (1, 0), "Confirm made another step");
    press_button(&ctx, &mut e, "Apply circle");
    let request = e.take_apply_circle_request().expect("Apply asked");
    assert_eq!(
        (
            crate::transaction_tests::sketch(&request),
            crate::transaction_tests::circle(&request).curve_id,
            crate::transaction_tests::circle(&request).radius_mm
        ),
        (sketch, curve, 10.5)
    );
    assert_eq!(
        crate::transaction_tests::circle(&request).center_mm,
        [12.5 + d[0], -7.25 + d[1]],
        "the request is the canvas's numbers"
    );
    press_button(&ctx, &mut e, "Save edited circle copy…");
    let copy = e.take_circle_edit_request().expect("copy asked");
    assert_eq!(copy.changes, request.changes);
    press_button(&ctx, &mut e, "Undo draft");
    assert_eq!((shown(&e), depth(&e)), (stored, (0, 1)));
    press_button(&ctx, &mut e, "Redo draft");
    assert_eq!(e.circle.center, centre);

    // Annulus: the bore handle writes the bore's radius under the bore's UUID; the
    // outer radius and its UUID are untouched.
    let (_root, ctx, mut e) = saved(&[10.5, 4.75]);
    let (outer_id, inner_id) = {
        let (request, _) = e.editing_annulus.as_ref().expect("form");
        (
            crate::transaction_tests::annulus(&request).outer_curve_id,
            crate::transaction_tests::annulus(&request).inner_curve_id,
        )
    };
    let at = handle(&ctx, &mut e, Handle::Bore);
    let to = at + egui::vec2(9.5, -12.25);
    drag(&ctx, &mut e, at, to);
    let d = moved(&e, at, to);
    let bore = d[0].hypot(4.75 + d[1]);
    assert_eq!(
        shown(&e),
        strings(["12.5", "-7.25"], "10.5", Some(&bore.to_string()))
    );
    press_button(&ctx, &mut e, "Apply annulus");
    let request = e.take_apply_annulus_request().expect("Apply asked");
    assert_eq!(
        (
            crate::transaction_tests::annulus(&request).outer_curve_id,
            crate::transaction_tests::annulus(&request).inner_curve_id,
            crate::transaction_tests::annulus(&request).outer_radius_mm,
            crate::transaction_tests::annulus(&request).inner_radius_mm,
            crate::transaction_tests::circle(&request).center_mm,
        ),
        (outer_id, inner_id, 10.5, bore, [12.5, -7.25])
    );
    // And the outer handle writes only the outer radius.
    let at = handle(&ctx, &mut e, Handle::Outer);
    let to = at + egui::vec2(15., 5.);
    drag(&ctx, &mut e, at, to);
    let d = moved(&e, at, to);
    assert_eq!(
        shown(&e),
        strings(
            ["12.5", "-7.25"],
            &(10.5 + d[0]).hypot(d[1]).to_string(),
            Some(&bore.to_string())
        )
    );
    assert_eq!(depth(&e), (2, 0));
}

#[test]
fn typed_unconfirmed_text_comes_back_exactly_with_the_first_undo_after_a_drag() {
    // Saved: "12.50" and " 10.5" are typed and not confirmed, then the radius is
    // dragged. One Undo gives back exactly those strings; a second the stored ones.
    let (_root, ctx, mut e) = saved(&[10.5]);
    let stored = shown(&e);
    type_into(&ctx, &mut e, "Center X", "12.50");
    type_into(&ctx, &mut e, "Radius", " 10.5");
    let typed = strings(["12.50", "-7.25"], " 10.5", None);
    assert_eq!((shown(&e), depth(&e)), (typed.clone(), (0, 0)));
    // A press that does not move records nothing, not even the typed text.
    let at = handle(&ctx, &mut e, Handle::Outer);
    press(&ctx, &mut e, at);
    release(&ctx, &mut e, at);
    assert_eq!((shown(&e), depth(&e)), (typed.clone(), (0, 0)));
    let to = at + egui::vec2(25., 0.);
    drag(&ctx, &mut e, at, to);
    let dragged = shown(&e);
    assert_eq!(
        dragged.centre, typed.centre,
        "the centre keeps its spelling"
    );
    assert_ne!(dragged.outer, typed.outer);
    assert_eq!(depth(&e), (2, 0));
    assert_eq!(e.circle_applied.as_ref(), Some(&e.circle));
    press_button(&ctx, &mut e, "Undo draft");
    assert_eq!((shown(&e), depth(&e)), (typed.clone(), (1, 1)));
    press_button(&ctx, &mut e, "Undo draft");
    assert_eq!((shown(&e), depth(&e)), (stored, (0, 2)));
    press_button(&ctx, &mut e, "Redo draft");
    assert_eq!(shown(&e), typed);
    press_button(&ctx, &mut e, "Redo draft");
    assert_eq!(shown(&e), dragged);
    // Confirm keeps its meaning: it rewrites the spelling, and the same numbers in
    // another spelling are no step, even beside a spelling a gesture kept.
    type_into(&ctx, &mut e, "Center Y", "-7.250");
    press_button(&ctx, &mut e, "Confirm draft numbers");
    assert_eq!(e.circle.center, ["12.5", "-7.25"]);
    assert_eq!(depth(&e), (2, 0), "an equivalent spelling is no step");
    type_into(&ctx, &mut e, "Center Y", "-8");
    press_button(&ctx, &mut e, "Confirm draft numbers");
    assert_eq!(depth(&e), (3, 0), "a different number is one step");

    // New: the opening numbers are the first confirmed state.
    let ctx = context();
    let mut e = Editor::default();
    open_new(&ctx, &mut e, "Circle with hole");
    type_into(&ctx, &mut e, "Inner radius", "4.0");
    let at = handle(&ctx, &mut e, Handle::Centre);
    drag(&ctx, &mut e, at, at + egui::vec2(10., 10.));
    assert_eq!(depth(&e), (2, 0));
    press_button(&ctx, &mut e, "Undo draft");
    assert_eq!(shown(&e), strings(["0", "0"], "10", Some("4.0")));
    press_button(&ctx, &mut e, "Undo draft");
    assert_eq!(shown(&e), strings(["0", "0"], "10", Some("4")));
}

#[test]
fn gestures_in_one_frame_keep_their_order_and_a_no_op_keeps_the_redo() {
    let (_root, ctx, mut e) = saved(&[10.5, 4.75]);
    let stored = shown(&e);
    let c = handle(&ctx, &mut e, Handle::Centre);
    let o = handle(&ctx, &mut e, Handle::Outer);
    let s = scale(&e);
    // A whole gesture in one frame is one step.
    let shift = egui::vec2(16., -8.);
    frame(
        &ctx,
        &mut e,
        vec![
            egui::Event::PointerMoved(c),
            event(c, true),
            egui::Event::PointerMoved(c + shift),
            event(c + shift, false),
        ],
    );
    let first = shown(&e);
    assert_ne!(first.centre, stored.centre);
    assert_eq!((first.outer.as_str(), depth(&e)), ("10.5", (1, 0)));
    // Two gestures in one frame are two steps in their order: the centre again, then
    // the outer handle where the first left it.
    let o = o + shift;
    let c = c + shift;
    frame(
        &ctx,
        &mut e,
        vec![
            event(c, true),
            egui::Event::PointerMoved(c + shift),
            event(c + shift, false),
            event(o + shift, true),
            egui::Event::PointerMoved(o + shift + egui::vec2(12., 0.)),
            event(o + shift + egui::vec2(12., 0.), false),
        ],
    );
    assert_eq!(depth(&e), (3, 0));
    let last = shown(&e);
    let d = moved(&e, o + shift, o + shift + egui::vec2(12., 0.));
    assert!((d[0] * s - 12.).abs() < 1e-3);
    assert_eq!(
        last.outer,
        (10.5 + d[0]).hypot(d[1]).to_string(),
        "the second gesture held the outer handle"
    );
    press_button(&ctx, &mut e, "Undo draft");
    let second = shown(&e);
    assert_eq!(second.outer, "10.5");
    assert_ne!(second.centre, first.centre);
    press_button(&ctx, &mut e, "Undo draft");
    assert_eq!(shown(&e), first);
    // A press and release in one frame, and a cancelled one, keep the Redo.
    let c = handle(&ctx, &mut e, Handle::Centre);
    frame(&ctx, &mut e, vec![event(c, true), event(c, false)]);
    frame(
        &ctx,
        &mut e,
        vec![
            event(c, true),
            egui::Event::PointerMoved(c + shift),
            key(egui::Key::Escape),
            event(c + shift, false),
        ],
    );
    assert_eq!((shown(&e), depth(&e)), (first, (1, 2)));
    press_button(&ctx, &mut e, "Redo draft");
    press_button(&ctx, &mut e, "Redo draft");
    assert_eq!(shown(&e), last);

    // A completed gesture and the next gesture's preview can arrive together.
    // Only the released gesture is confirmed: cancelling the held one must not
    // leave its preview as the history's accepted draft.
    for radii in [&[10.5][..], &[10.5, 4.75][..]] {
        let (_root, ctx, mut e) = saved(radii);
        let original = shown(&e);
        let c = handle(&ctx, &mut e, Handle::Centre);
        let o = handle(&ctx, &mut e, Handle::Outer);
        let shift = egui::vec2(12., -6.);
        frame(
            &ctx,
            &mut e,
            vec![
                event(c, true),
                egui::Event::PointerMoved(c + shift),
                event(c + shift, false),
                event(o + shift, true),
                egui::Event::PointerMoved(o + shift + egui::vec2(18., 0.)),
            ],
        );
        assert!(e.gesturing());
        assert_eq!(depth(&e), (1, 0));
        frame(&ctx, &mut e, vec![key(egui::Key::Escape)]);
        release(&ctx, &mut e, o + shift + egui::vec2(18., 0.));
        assert_eq!(shown(&e).outer, original.outer);
        if annular(&e) {
            assert_eq!(e.annulus_applied.as_ref(), Some(&e.annulus));
        } else {
            assert_eq!(e.circle_applied.as_ref(), Some(&e.circle));
        }
        let at = handle(&ctx, &mut e, Handle::Outer);
        drag(&ctx, &mut e, at, at + egui::vec2(9., 0.));
        assert_eq!(depth(&e), (2, 0), "a cancelled preview is no extra step");
        press_button(&ctx, &mut e, "Undo draft");
        press_button(&ctx, &mut e, "Undo draft");
        assert_eq!(shown(&e), original);
    }
}

#[test]
fn escape_lost_focus_and_a_vanished_pointer_restore_the_exact_strings_and_history() {
    let (_root, ctx, mut e) = saved(&[10.5, 4.75]);
    // A history in both directions, and typed text that is not confirmed.
    let at = handle(&ctx, &mut e, Handle::Centre);
    drag(&ctx, &mut e, at, at + egui::vec2(10., 0.));
    press_button(&ctx, &mut e, "Undo draft");
    type_into(&ctx, &mut e, "Outer radius", "10.50");
    let before = (shown(&e), depth(&e), e.annulus_applied.clone());
    assert_eq!(before.1, (0, 1));
    for (which, cancel) in [
        (Handle::Outer, key(egui::Key::Escape)),
        (Handle::Bore, egui::Event::PointerGone),
        (Handle::Centre, egui::Event::WindowFocused(false)),
    ] {
        let at = handle(&ctx, &mut e, which);
        press(&ctx, &mut e, at);
        move_to(&ctx, &mut e, at + egui::vec2(14., -9.));
        assert_ne!(shown(&e), before.0, "{which:?} previewed nothing");
        frame(&ctx, &mut e, vec![cancel]);
        assert!(!e.gesturing(), "{which:?} still held");
        // Focus comes back for the next case; the late release does nothing.
        frame(&ctx, &mut e, vec![egui::Event::WindowFocused(true)]);
        release(&ctx, &mut e, at + egui::vec2(14., -9.));
        assert_eq!(
            (shown(&e), depth(&e), e.annulus_applied.clone()),
            before,
            "{which:?}"
        );
    }
    // A job that starts while a handle is held makes the form unavailable: the drag
    // ends as if cancelled.
    let at = handle(&ctx, &mut e, Handle::Bore);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(-8., 5.));
    assert!(e.gesturing() && shown(&e) != before.0);
    frame_running(&ctx, &mut e, vec![], true);
    assert!(!e.gesturing(), "a running job left the handle held");
    release(&ctx, &mut e, at + egui::vec2(-8., 5.));
    assert_eq!(
        (shown(&e), depth(&e), e.annulus_applied.clone()),
        before,
        "a running job"
    );
    // A release outside the canvas (and outside the form) ends the gesture there.
    let at = handle(&ctx, &mut e, Handle::Outer);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, egui::pos2(990., 890.));
    release(&ctx, &mut e, egui::pos2(990., 890.));
    assert!(!e.gesturing());
    assert_eq!(depth(&e), (2, 0), "the typed text, then the gesture");
}

#[test]
fn busy_queued_covered_and_outside_presses_start_nothing_and_held_fields_take_no_text() {
    let (_root, ctx, mut e) = saved(&[10.5]);
    let stored = shown(&e);
    let c = handle(&ctx, &mut e, Handle::Centre);
    // A job running: the form is disabled, a press takes nothing.
    frame_running(&ctx, &mut e, vec![egui::Event::PointerMoved(c)], true);
    frame_running(&ctx, &mut e, vec![event(c, true)], true);
    frame_running(
        &ctx,
        &mut e,
        vec![egui::Event::PointerMoved(c + egui::vec2(20., 0.))],
        true,
    );
    frame_running(
        &ctx,
        &mut e,
        vec![event(c + egui::vec2(20., 0.), false)],
        true,
    );
    assert!(!e.gesturing());
    assert_eq!((shown(&e), depth(&e)), (stored.clone(), (0, 0)));

    // A request asked for and not yet taken: no gesture may change the draft under it.
    type_into(&ctx, &mut e, "Radius", "11");
    press_button(&ctx, &mut e, "Apply circle");
    drag(&ctx, &mut e, c, c + egui::vec2(20., 0.));
    assert_eq!(
        shown(&e).centre,
        stored.centre,
        "a queued Apply was changed"
    );
    let asked = e.take_apply_circle_request().expect("asked");
    assert_eq!(crate::transaction_tests::circle(&asked).radius_mm, 11.);
    drag(&ctx, &mut e, c, c + egui::vec2(20., 0.));
    assert_ne!(
        shown(&e).centre,
        stored.centre,
        "taken, the canvas works again"
    );
    press_button(&ctx, &mut e, "Undo draft");

    // A press outside the form, or beside the canvas, dragged over a handle, takes
    // nothing. (The press beside it may move the floating form: positions are read
    // again.)
    let before = (shown(&e), depth(&e));
    for outside in [
        egui::pos2(990., 890.),
        egui::pos2(canvas_rect(&frame(&ctx, &mut e, vec![])).left() - 3., c.y),
    ] {
        let c = handle(&ctx, &mut e, Handle::Centre);
        press(&ctx, &mut e, outside);
        move_to(&ctx, &mut e, c);
        assert!(!e.gesturing(), "a press at {outside:?}");
        move_to(&ctx, &mut e, c + egui::vec2(20., 0.));
        release(&ctx, &mut e, c + egui::vec2(20., 0.));
        assert_eq!((shown(&e), depth(&e)), before);
    }
    let c = handle(&ctx, &mut e, Handle::Centre);

    // Another window over the canvas takes the press.
    let cover = egui::Rect::from_center_size(c, egui::vec2(60., 60.));
    let covered = |ctx: &egui::Context, e: &mut Editor, events| {
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                e.draw(ui, true, false, "");
                egui::Area::new(egui::Id::new("over the canvas"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(cover.min)
                    .show(ui.ctx(), |ui| {
                        ui.allocate_exact_size(cover.size(), egui::Sense::click_and_drag())
                    });
            },
        );
    };
    for _ in 0..2 {
        covered(&ctx, &mut e, vec![]);
    }
    covered(&ctx, &mut e, vec![egui::Event::PointerMoved(c)]);
    covered(&ctx, &mut e, vec![event(c, true)]);
    covered(
        &ctx,
        &mut e,
        vec![egui::Event::PointerMoved(c + egui::vec2(20., 0.))],
    );
    assert!(!e.gesturing(), "a press through another window");
    covered(&ctx, &mut e, vec![event(c + egui::vec2(20., 0.), false)]);
    assert_eq!((shown(&e), depth(&e)), before);

    // A box that kept the keyboard focus takes no text while a handle is held.
    let out = frame(&ctx, &mut e, vec![]);
    let field = text_at(&out, "11");
    click(&ctx, &mut e, field);
    frame(
        &ctx,
        &mut e,
        vec![key(egui::Key::End), egui::Event::Text("2".into())],
    );
    assert_eq!(e.circle.radius, "112", "the box has the focus");
    assert!(ctx.memory(|m| m.focused()).is_some());
    let at = handle(&ctx, &mut e, Handle::Centre);
    // egui gives a box's focus up only when a click elsewhere is complete; a press
    // and a drag on the canvas are none. Only the box being inactive while a handle
    // is held keeps the text out (and lets the focus go).
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(10., 0.));
    frame(&ctx, &mut e, vec![egui::Event::Text("9".into())]);
    assert_eq!(e.circle.radius, "112", "a held gesture's form took text");
    release(&ctx, &mut e, at + egui::vec2(10., 0.));

    // A move onto the floating canvas and a press can arrive before egui's next
    // pass. The window must use the latest OS position over the painted areas,
    // rather than starting a 3D orbit behind either analytic form, at either DPI.
    for (radii, pixels_per_point) in [
        (&[10.5][..], 1.),
        (&[10.5, 4.75][..], 1.),
        (&[10.5][..], 2.),
        (&[10.5, 4.75][..], 2.),
    ] {
        use winit::{
            dpi::PhysicalPosition,
            event::{DeviceId, ElementState, MouseButton, WindowEvent},
        };
        let (_root, ctx, mut e) = saved(radii);
        ctx.set_pixels_per_point(pixels_per_point);
        for _ in 0..2 {
            frame(&ctx, &mut e, vec![]);
        }
        let at = handle(&ctx, &mut e, Handle::Centre);
        let outside = egui::pos2(10., 880.);
        frame(&ctx, &mut e, vec![egui::Event::PointerMoved(outside)]);
        assert!(
            !ctx.egui_wants_pointer_input(),
            "{radii:?} at {pixels_per_point} pixels/point"
        );
        let mut input = ferritecad_ui::ViewportInput::new();
        input.resize(
            (1000. * pixels_per_point) as u32,
            (900. * pixels_per_point) as u32,
        );
        input.frame(([-10.; 3], [10.; 3])).expect("frame");
        let camera = *input.camera();
        let motion = |p: egui::Pos2| WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(
                f64::from(p.x * pixels_per_point),
                f64::from(p.y * pixels_per_point),
            ),
        };
        let button = |state| WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button: MouseButton::Left,
        };
        for event in [
            motion(at),
            button(ElementState::Pressed),
            motion(at + egui::vec2(24., 12.)),
            button(ElementState::Released),
        ] {
            let claimed = crate::window_pointer_claimed(&ctx, &input, &event);
            crate::apply_viewport_input(&mut input, &event, false, claimed);
        }
        assert_eq!(
            *input.camera(),
            camera,
            "the canvas drag rotated the 3D view"
        );
        assert!(
            input.take_pick().is_none(),
            "the canvas selected the model behind it"
        );
        // A drag that starts in the unobstructed viewport still belongs to the
        // viewport when it crosses the form.
        for event in [
            motion(outside),
            button(ElementState::Pressed),
            motion(at),
            button(ElementState::Released),
        ] {
            let claimed = crate::window_pointer_claimed(&ctx, &input, &event);
            crate::apply_viewport_input(&mut input, &event, false, claimed);
        }
        assert_ne!(
            *input.camera(),
            camera,
            "the form swallowed a viewport drag"
        );
    }
}

#[test]
fn crowded_handles_at_a_small_scale_follow_the_pick_rule_through_real_presses() {
    // The view fitted to 10.5 mm; then 0.5 and 0.25 mm typed: three handles within
    // a few points of each other, the view as it was.
    let (_root, ctx, mut e) = saved(&[10.5, 4.75]);
    let view = e.annulus_canvas.view;
    type_into(&ctx, &mut e, "Outer radius", "0.5");
    type_into(&ctx, &mut e, "Inner radius", "0.25");
    assert_eq!(e.annulus_canvas.view, view, "typing moved the view");
    let c = handle(&ctx, &mut e, Handle::Centre);
    let o = handle(&ctx, &mut e, Handle::Outer);
    let b = handle(&ctx, &mut e, Handle::Bore);
    assert!(
        (o - c).length() < 9. && (b - c).length() < 9.,
        "{c:?} {o:?} {b:?}"
    );
    let held_at = |ctx: &egui::Context, e: &mut Editor, at: egui::Pos2| {
        press(ctx, e, at);
        let held = e.annulus_canvas.held();
        release(ctx, e, at);
        held
    };
    assert_eq!(held_at(&ctx, &mut e, c), Some(Handle::Centre));
    assert_eq!(held_at(&ctx, &mut e, o), Some(Handle::Outer));
    assert_eq!(held_at(&ctx, &mut e, b), Some(Handle::Bore));
    // Nearer to one of two crowded handles: that one.
    assert_eq!(
        held_at(&ctx, &mut e, c + (o - c) * 0.25),
        Some(Handle::Centre)
    );
    assert_eq!(
        held_at(&ctx, &mut e, c + (o - c) * 0.75),
        Some(Handle::Outer)
    );
    assert_eq!(
        held_at(&ctx, &mut e, c + (b - c) * 0.75),
        Some(Handle::Bore)
    );
    // Within the radius of nothing: no gesture.
    assert_eq!(held_at(&ctx, &mut e, c + egui::vec2(-12., 0.)), None);
    assert_eq!(depth(&e), (0, 0), "presses without movement made steps");
    // The handle taken at the press is kept over the others to the release.
    let typed = shown(&e);
    press(&ctx, &mut e, o);
    for over in [c, b, c + egui::vec2(-30., 0.)] {
        move_to(&ctx, &mut e, over);
        assert_eq!(e.annulus_canvas.held(), Some(Handle::Outer));
    }
    release(&ctx, &mut e, c + egui::vec2(-30., 0.));
    let after = shown(&e);
    assert_eq!(
        (&after.centre, &after.bore),
        (&typed.centre, &typed.bore),
        "another handle moved"
    );
    let d = moved(&e, o, c + egui::vec2(-30., 0.));
    assert_eq!(after.outer, (0.5 + d[0]).hypot(d[1]).to_string());
    // So small that the three handles are one point on the screen: the exact tie goes
    // to the centre, whatever is pressed, and the gesture moves the whole profile.
    type_into(&ctx, &mut e, "Outer radius", "0.000000001");
    type_into(&ctx, &mut e, "Inner radius", "0.0000000005");
    let c = handle(&ctx, &mut e, Handle::Centre);
    assert_eq!(handle(&ctx, &mut e, Handle::Outer), c);
    assert_eq!(handle(&ctx, &mut e, Handle::Bore), c);
    assert_eq!(held_at(&ctx, &mut e, c), Some(Handle::Centre));
    let tiny = shown(&e);
    drag(&ctx, &mut e, c, c + egui::vec2(10., 0.));
    let moved_tiny = shown(&e);
    assert_ne!(moved_tiny.centre[0], tiny.centre[0]);
    assert_eq!(
        (&moved_tiny.outer, &moved_tiny.bore),
        (&tiny.outer, &tiny.bore)
    );
}

#[test]
fn one_and_two_pixels_per_point_give_the_same_strings_and_pick_radius() {
    let run = |pixels_per_point: f32| {
        let (_root, ctx, mut e) = saved(&[10.5, 4.75]);
        ctx.set_pixels_per_point(pixels_per_point);
        for _ in 0..2 {
            frame(&ctx, &mut e, vec![]);
        }
        let c = handle(&ctx, &mut e, Handle::Centre);
        let rect = canvas_rect(&frame(&ctx, &mut e, vec![]));
        let mut taken = Vec::new();
        for away in [8.5_f32, 9.5] {
            press(&ctx, &mut e, c + egui::vec2(-away, 0.));
            taken.push(e.annulus_canvas.held());
            release(&ctx, &mut e, c + egui::vec2(-away, 0.));
        }
        let b = handle(&ctx, &mut e, Handle::Bore);
        drag(&ctx, &mut e, b, b + egui::vec2(-6.5, -10.25));
        (shown(&e), taken, c - rect.min, e.annulus_canvas.view)
    };
    let (one, two) = (run(1.), run(2.));
    assert_eq!(one.0, two.0, "the same gesture made other strings");
    assert_eq!(one.1, [Some(Handle::Centre), None]);
    assert_eq!(one.1, two.1);
    assert!((one.2 - two.2).length() < 1e-3);
    assert_eq!(one.3, two.3, "the view depends on pixels");
}

#[test]
fn a_refused_profile_stays_a_visible_draft_and_text_that_cannot_be_drawn_draws_nothing() {
    let (_root, ctx, mut e) = saved(&[10.5, 4.75]);
    let view = e.annulus_canvas.view;
    // The bore dragged past the boundary: drawn as it is, in its role, refused by the
    // form's own policy, kept as the draft.
    let b = handle(&ctx, &mut e, Handle::Bore);
    let up = egui::vec2(0., -10. * scale(&e) as f32);
    drag(&ctx, &mut e, b, b + up);
    let s = shown(&e);
    let bore: f64 = s.bore.as_deref().expect("bore").parse().expect("number");
    assert!(bore > 10.5, "{bore}");
    let out = frame(&ctx, &mut e, vec![]);
    let painted_circles = circles(&out);
    assert_eq!(painted_circles.len(), 2);
    let (outer_ring, bore_ring) = (painted_circles[0], painted_circles[1]);
    assert!(
        bore_ring.1 > outer_ring.1,
        "the bore is drawn larger, as it is"
    );
    assert_ne!(
        bore_ring.2, outer_ring.2,
        "the roles were swapped or merged"
    );
    assert!(painted(
        &out,
        "invalid input: inner radius must be smaller than the outer radius; a hole is inside \
         its boundary"
    ));
    assert!(!painted(&out, "Apply annulus") && !painted(&out, "Confirm draft numbers"));
    assert!(e.editing_analytic() && depth(&e) == (1, 0));
    assert_eq!(s.outer, "10.5", "nothing was clamped or exchanged");

    // A radius that is not positive: no circle, no handle, its text kept and refused.
    type_into(&ctx, &mut e, "Outer radius", "-2");
    let out = frame(&ctx, &mut e, vec![]);
    assert_eq!(circles(&out).len(), 1, "only the bore is a circle");
    assert!(profile(&e).expect("drawable").handles().len() == 2);
    assert!(painted(
        &out,
        "invalid input: circle radius must be finite and positive in mm"
    ));

    // Text that cannot be drawn: no profile at all, the field and the reason named, the
    // view kept, no gesture.
    let c = handle(&ctx, &mut e, Handle::Centre);
    for (typed, words) in [
        ("-", "Not drawn: Outer radius: \"-\" is not a number"),
        ("", "Not drawn: Outer radius: empty"),
        (
            "1e999",
            "Not drawn: Outer radius: \"1e999\" is not a finite number",
        ),
        (
            "NaN",
            "Not drawn: Outer radius: \"NaN\" is not a finite number",
        ),
    ] {
        type_into(&ctx, &mut e, "Outer radius", typed);
        let out = frame(&ctx, &mut e, vec![]);
        assert!(circles(&out).is_empty(), "{typed:?} drew a circle");
        assert!(painted(&out, words), "{typed:?}");
        assert_eq!(e.annulus_canvas.view, view, "{typed:?} moved the view");
        let before = (shown(&e), depth(&e));
        drag(&ctx, &mut e, c, c + egui::vec2(15., 0.));
        assert_eq!((shown(&e), depth(&e)), before, "{typed:?} took a gesture");
    }
    // Undo drops the unconfirmed text, as it always did, and goes back one step;
    // Redo brings back the refused gesture exactly.
    press_button(&ctx, &mut e, "Undo draft");
    assert_eq!(shown(&e), strings(["12.5", "-7.25"], "10.5", Some("4.75")));
    press_button(&ctx, &mut e, "Redo draft");
    assert_eq!(shown(&e), s);
}

#[test]
fn the_view_fits_on_opening_and_on_fit_only_and_the_form_stays_within_768_points() {
    let (_root, ctx, mut e) = saved(&[10.5, 4.75]);
    let rect = canvas_rect(&frame(&ctx, &mut e, vec![]));
    let fitted = e.annulus_canvas.view;
    let c = handle(&ctx, &mut e, Handle::Centre);
    assert!((c - rect.center()).length() < 0.01, "first open is centred");
    // Typing and a gesture never move the view.
    type_into(&ctx, &mut e, "Center X", "20");
    let o = handle(&ctx, &mut e, Handle::Outer);
    drag(&ctx, &mut e, o, o + egui::vec2(8., 0.));
    assert_eq!(depth(&e), (2, 0), "the typed centre, then the gesture");
    assert_eq!(e.annulus_canvas.view, fitted);
    // Fit profile fits again, about the new centre.
    press_button(&ctx, &mut e, "Fit profile");
    assert_ne!(e.annulus_canvas.view, fitted);
    let c = handle(&ctx, &mut e, Handle::Centre);
    let rect = canvas_rect(&frame(&ctx, &mut e, vec![]));
    assert!((c - rect.center()).length() < 0.01);
    assert_eq!(rect.size(), SIZE);

    // The longest saved form (annulus, unsaved notes, a document line) on a
    // 988 x 768 screen: everything is inside it.
    e.set_session(true, false, true);
    let ctx = context();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(988., 768.));
    let tall = |ctx: &egui::Context, e: &mut Editor| {
        let out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                e.draw(
                    ui,
                    true,
                    false,
                    "Could not apply the change: the document changed after this form was opened",
                )
            },
        );
        let mut out = out;
        out.textures_delta.clear();
        (out, ctx.globally_used_rect())
    };
    for _ in 0..3 {
        tall(&ctx, &mut e);
    }
    let (out, used) = tall(&ctx, &mut e);
    assert!(used.max.y <= 768., "{used:?}");
    for label in ["Apply annulus", "Undo draft", "Fit profile"] {
        let at = text_at(&out, label);
        assert!(screen.contains(at), "{label} at {at:?}");
    }
    eprintln!("the form ends at {} of 768 points", used.max.y);
}

/// §31I: the centre handle of the circle or annular canvas the form shows, pressed,
/// moved and still held, as `drag_tests::hold_vertex` holds a vertex.
pub(crate) fn hold_handle(e: &mut Editor) -> (egui::Context, egui::Pos2) {
    let ctx = context();
    for _ in 0..3 {
        frame(&ctx, e, vec![]);
    }
    let at = handle(&ctx, e, Handle::Centre);
    press(&ctx, e, at);
    let to = at + egui::vec2(6., 0.);
    move_to(&ctx, e, to);
    assert!(e.gesturing(), "the handle is not held");
    (ctx, to)
}

pub(crate) fn release_handle(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
    release(ctx, e, at);
    assert!(!e.gesturing());
}

/// §31I: the open New window switched to `profile` by its own button.
pub(crate) fn choose_profile(e: &mut Editor, profile: &str) {
    let ctx = context();
    for _ in 0..3 {
        frame(&ctx, e, vec![]);
    }
    press_button(&ctx, e, profile);
}

/// §31I: the saved Circle form, on a kernel-free file written in `root`.
pub(crate) fn begin_saved_circle(e: &mut Editor, root: &std::path::Path) -> bool {
    let path = root.join("round.fcad");
    crate::sketch::tests::analytic_apply::write_round(&path, [12.5, -7.25], &[10.5], 15.25, false);
    let reading = crate::sketch_facts::read(&path).expect("reading");
    e.begin_circle_edit(&path, &reading, reading.circle_sketches[0].sketch)
}

/// §31I: a handle (`"centre"`, `"outer"`, `"bore"`) of the canvas the form shows,
/// dragged by `by` points with real events in a context of its own.
pub(crate) fn drag_role(e: &mut Editor, role: &str, by: egui::Vec2) {
    let which = match role {
        "centre" => Handle::Centre,
        "outer" => Handle::Outer,
        "bore" => Handle::Bore,
        _ => panic!("no handle {role}"),
    };
    let ctx = context();
    for _ in 0..3 {
        frame(&ctx, e, vec![]);
    }
    let at = handle(&ctx, e, which);
    drag(&ctx, e, at, at + by);
}

/// §31I: what a tab keeps of its canvas: the strings, the draft history's depth and
/// the view (lower-left corner, points per mm).
pub(crate) type CanvasFacts = (Vec<String>, (usize, usize), Option<([f64; 2], f32)>);

pub(crate) fn canvas_facts(e: &Editor) -> CanvasFacts {
    let s = shown(e);
    let mut text = s.centre.to_vec();
    text.push(s.outer);
    text.extend(s.bore);
    (text, depth(e), canvas(e).view.map(|v| (v.minimum, v.scale)))
}

/// §31I: **Undo draft** or **Fit profile** (any button) pressed on the form.
pub(crate) fn press_on_form(e: &mut Editor, label: &str) {
    let ctx = context();
    for _ in 0..3 {
        frame(&ctx, e, vec![]);
    }
    press_button(&ctx, e, label);
}

/// §31I: the saved Circle form on `path`, its centre and radius dragged by real
/// events (fractional, diagonal), then **Apply circle**: the form and its request,
/// whose numbers are exactly the strings the canvas wrote.
pub(crate) fn dragged_circle_apply(
    path: &std::path::Path,
    reading: &ExtrudeEditSource,
    sketch: ferritecad_types::ObjectId,
) -> (Editor, EditRequest) {
    let mut e = Editor::default();
    e.set_session(true, false, true);
    e.set_analytic_apply(true);
    assert!(e.begin_circle_edit(path, reading, sketch));
    drag_role(&mut e, "centre", egui::vec2(-31.25, 18.75));
    drag_role(&mut e, "outer", egui::vec2(-12.5, -9.75));
    let typed = shown(&e);
    press_on_form(&mut e, "Apply circle");
    let request = e.take_apply_circle_request().expect("Apply circle asked");
    let n = |s: &str| s.parse::<f64>().expect("a number");
    assert_eq!(
        (
            crate::transaction_tests::circle(&request).center_mm,
            crate::transaction_tests::circle(&request).radius_mm
        ),
        ([n(&typed.centre[0]), n(&typed.centre[1])], n(&typed.outer)),
        "the request is not what the canvas wrote"
    );
    (e, request)
}

/// §31I: the same for a saved annulus: its centre, outer and bore radius dragged.
pub(crate) fn dragged_annulus_apply(
    path: &std::path::Path,
    reading: &ExtrudeEditSource,
    sketch: ferritecad_types::ObjectId,
) -> (Editor, EditRequest) {
    let mut e = Editor::default();
    e.set_session(true, false, true);
    e.set_analytic_apply(true);
    assert!(e.begin_annulus_edit(path, reading, sketch));
    drag_role(&mut e, "centre", egui::vec2(-31.25, 18.75));
    drag_role(&mut e, "outer", egui::vec2(-12.5, -9.75));
    drag_role(&mut e, "bore", egui::vec2(-7.25, 4.5));
    let typed = shown(&e);
    press_on_form(&mut e, "Apply annulus");
    let request = e.take_apply_annulus_request().expect("Apply annulus asked");
    let n = |s: &str| s.parse::<f64>().expect("a number");
    assert_eq!(
        (
            crate::transaction_tests::annulus(&request).center_mm,
            crate::transaction_tests::annulus(&request).outer_radius_mm,
            crate::transaction_tests::annulus(&request).inner_radius_mm
        ),
        (
            [n(&typed.centre[0]), n(&typed.centre[1])],
            n(&typed.outer),
            n(typed.bore.as_deref().expect("bore"))
        ),
        "the request is not what the canvas wrote"
    );
    (e, request)
}

/// §31I: the form drawn a few frames, as a window shows it: its canvas has fitted.
pub(crate) fn settle_form(e: &mut Editor) {
    let ctx = context();
    for _ in 0..3 {
        frame(&ctx, e, vec![]);
    }
}
