// SPDX-License-Identifier: MIT
//! Existing publication fixtures edited through real viewport handle gestures.
use super::dragging::{frame, movement, press, release};
use super::widgets::Ui;
use super::window::{enter, finish, open};
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window, mock_scene};
use crate::sketch::Editor as Forms;
use ferritecad_document::{Document, ObjectPayload};
use ferritecad_ui::ModelEdit;
use ferritecad_ui::ViewportInput;

pub(crate) fn stored(path: &Path) -> (ObjectId, ferritecad_document::Sketch) {
    ferritecad_fixtures::sketch(&Document::open_read_only(path).expect("fixture"))
        .expect("fixture sketch")
}

pub(crate) fn sketch_id(source: &Source) -> ObjectId {
    sketch_fact(source).0
}

pub(crate) fn sketch_fact(source: &Source) -> (ObjectId, &Sketch) {
    source
        .objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Sketch(sketch) => Some((o.id, sketch)),
            _ => None,
        })
        .expect("fixture sketch")
}

pub(crate) fn plate(path: &Path) -> (Window, EditRequest, Vec<EditChange>) {
    let (sketch, saved) = stored(path);
    assert!(saved.constraints.is_empty());
    assert_eq!(saved.curves.len(), 4);
    let expected = [
        ([36.5, 15.5], [36.5, 3.3]),
        ([36.5, 3.3], [-1.3, 3.3]),
        ([-1.3, 3.3], [-1.3, 15.5]),
        ([-1.3, 15.5], [36.5, 15.5]),
    ]
    .into_iter()
    .zip(&saved.curves)
    .map(|((start, end), curve)| EditChange::SetCurveGeometry {
        sketch,
        curve: curve.id,
        geometry: SketchGeometry::Line {
            start: point(start[0], start[1]),
            end: point(end[0], end[1]),
        },
    })
    .collect::<Vec<_>>();
    let mut w = open(path, Drawn::Native);
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    w.input
        .frame(([-15., -10., -1.], [50., 30., 1.]))
        .expect("frame the plate and its destinations");
    ui.press(&mut w, "Off", false);
    ui.press(&mut w, "0.1 mm", false);
    assert_eq!(
        w.creates.sketch.viewport.snapping.guides,
        crate::sketch::guide::Mode::Off
    );
    for (old, target) in [
        ([33., 15.5], [36.5, 15.5]),
        ([33., 3.25], [36.5, 3.3]),
        ([-4.5, 3.25], [-1.3, 3.3]),
        ([-4.5, 15.5], [-1.3, 15.5]),
    ] {
        for second in [false, true] {
            press(&mut w, point(old[0], old[1]));
            assert!(
                w.creates.sketch.viewport.gesturing(),
                "capture a saved endpoint"
            );
            movement(&mut w, point(target[0], target[1]));
            frame(&mut w);
            assert_eq!(
                w.creates.sketch.viewport.cursor,
                Some(point(target[0], target[1]))
            );
            if second {
                assert_eq!(
                    w.creates.sketch.viewport.snapped,
                    Some(point(target[0], target[1])),
                    "the second endpoint snaps onto the first"
                );
            }
            release(&mut w);
        }
    }
    let draft = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    assert_eq!(draft.undo.len(), 8, "eight separate free-endpoint gestures");
    assert_eq!(draft.state.constraints, saved.constraints);
    assert_eq!(
        draft.state.curves.iter().map(|c| c.id).collect::<Vec<_>>(),
        saved.curves.iter().map(|c| c.id).collect::<Vec<_>>()
    );
    let request = finish(&mut w, &mut ui);
    assert_eq!(
        request.changes, expected,
        "literal saved ends, identities and order"
    );
    (w, request, expected)
}

/// Open from the drawing produced by the same reading, without a retired catalogue.
pub(crate) fn begin(forms: &mut Forms, path: &Path, source: &Source, id: ObjectId) -> bool {
    if forms.active() {
        return false;
    }
    let input = ViewportInput::new();
    let scene = crate::prepare_load(&input, path, mock_scene(path), |_, _| Ok(()))
        .expect("accepted test picture");
    let accepted = scene.edit_source.as_ref().expect("source");
    assert_eq!(accepted.version, source.version);
    let apply = forms.viewport.can_apply;
    let opened = forms
        .viewport
        .begin_with(path, accepted, &scene.sketches, id);
    // Standalone form witnesses repeat the permission supplied by their owner frame.
    forms.viewport.set_apply(apply);
    opened
}

/// Change every occurrence of one saved coordinate using real handle gestures.
/// Both ends at a legacy polygon corner are moved explicitly: they have no Coincident rule.
pub(crate) fn request(
    path: &Path,
    source: &Source,
    id: ObjectId,
    from: &str,
    to: &str,
) -> (Forms, EditRequest) {
    let mut w = open(path, Drawn::Mock);
    // These copy/session witnesses address the original accepted file, not its private copy.
    assert!(begin(&mut w.creates.sketch, path, source, id));
    w.creates.sketch.viewport.look(&mut w.input);
    let mut ui = Ui::new(&mut w);
    let from: f64 = from.trim().parse().expect("fixture coordinate");
    let to: f64 = to.trim().parse().expect("fixture coordinate");
    let original = curves(&w.creates.sketch);
    let positions: Vec<_> = original
        .iter()
        .flat_map(|c| match c.geometry {
            SketchGeometry::Line { start, end } => [start, end],
            _ => panic!("line fixture"),
        })
        .collect();
    let min = positions
        .iter()
        .fold([f64::INFINITY; 2], |p, v| [p[0].min(v.x), p[1].min(v.y)]);
    let max = positions.iter().fold([f64::NEG_INFINITY; 2], |p, v| {
        [p[0].max(v.x), p[1].max(v.y)]
    });
    w.input
        .frame((
            [(min[0] - 10.) as f32, (min[1] - 10.) as f32, -1.],
            [(max[0] + 10.) as f32, (max[1] + 10.) as f32, 1.],
        ))
        .expect("frame all fixture handles");

    let points: Vec<Point2> = original
        .iter()
        .flat_map(|c| match c.geometry {
            SketchGeometry::Line { start, end } => [start, end],
            _ => panic!("line fixture"),
        })
        .filter(|p| p.x == from || p.y == from)
        .collect();
    ui.press(&mut w, "Off", false);
    ui.press(&mut w, "0.1 mm", false);
    for point in points {
        let target = Point2 {
            x: ((if point.x == from { to } else { point.x }) * 10.).round() / 10.,
            y: ((if point.y == from { to } else { point.y }) * 10.).round() / 10.,
        };
        press(&mut w, point);
        assert!(
            w.creates.sketch.viewport.gesturing(),
            "saved handle captured"
        );
        pointer(&mut w.creates.sketch, &mut w.input, target);
        frame(&mut w);
        release(&mut w);
    }
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.source, path);
    assert_eq!(request.expected, source.version);
    w.creates.sketch.viewport.clear_pointer();
    (std::mem::take(&mut w.creates.sketch), request)
}

pub(crate) fn curves(forms: &Forms) -> Vec<SketchCurve> {
    forms
        .viewport
        .draft
        .as_ref()
        .expect("draft")
        .state
        .curves
        .clone()
}

pub(crate) fn state(forms: &Forms) -> Option<(Vec<SketchCurve>, usize, usize)> {
    forms
        .viewport
        .draft
        .as_ref()
        .map(|d| (d.state.curves.clone(), d.undo.len(), d.redo.len()))
}

impl Forms {
    /// Open the fixture from an accepted picture of the same version.
    pub(crate) fn begin_test_drawing(
        &mut self,
        path: &Path,
        source: &Source,
        id: ObjectId,
    ) -> bool {
        begin(self, path, source, id)
    }
}

pub(crate) fn input(forms: &Forms) -> ViewportInput {
    let mut input = ViewportInput::new();
    input.set_region(ferritecad_viewport::PixelRect::sized(800, 600));
    let draft = forms.viewport.draft.as_ref().expect("draft");
    let plane = draft.planes[draft.plane].frame.expect("plane");
    let n = plane.normal();
    let y = plane.y_axis();
    input.look_along(
        [n.x as f32, n.y as f32, n.z as f32],
        [y.x as f32, y.y as f32, y.z as f32],
    );
    let curves = curves(forms);
    let points: Vec<_> = curves
        .iter()
        .flat_map(super::super::rules::handles)
        .map(|(_, p)| p)
        .collect();
    let lo = points
        .iter()
        .fold([f64::INFINITY; 2], |a, p| [a[0].min(p.x), a[1].min(p.y)]);
    let hi = points.iter().fold([f64::NEG_INFINITY; 2], |a, p| {
        [a[0].max(p.x), a[1].max(p.y)]
    });
    input
        .frame((
            [(lo[0] - 20.) as f32, (lo[1] - 20.) as f32, -1.],
            [(hi[0] + 20.) as f32, (hi[1] + 20.) as f32, 1.],
        ))
        .expect("all handles visible");
    input
}

fn event(forms: &mut Forms, input: &mut ViewportInput, event: ferritecad_ui::ViewportEvent) {
    forms.viewport.route(input, event, false, true, false);
}

fn pointer(forms: &mut Forms, input: &mut ViewportInput, p: Point2) -> egui::Pos2 {
    let draft = forms.viewport.draft.as_ref().expect("draft");
    let pixel = geometry::screen(
        input.camera(),
        draft.planes[draft.plane].frame.expect("plane"),
        p,
    )
    .expect("screen");
    let (mut x, mut y) = input.region().window((pixel[0] as f32, pixel[1] as f32));
    // Aim at the positive side of the same zero node in physical pixel precision.
    // A negative projection residue otherwise rounds to the distinct stored -0.0.
    if p.x == 0. {
        x = x.next_up();
    }
    if p.y == 0. {
        y = y.next_down();
    }
    event(
        forms,
        input,
        ferritecad_ui::ViewportEvent::PointerMoved { x, y },
    );
    egui::pos2(x, y)
}

/// A press really captures an endpoint, so transition tests hold the same owner as the window.
pub(crate) fn hold(forms: &mut Forms) -> (egui::Context, egui::Pos2) {
    let mut input = input(forms);
    let point = curves(forms)
        .iter()
        .flat_map(super::super::rules::handles)
        .next()
        .expect("handle")
        .1;
    let at = pointer(forms, &mut input, point);
    event(
        forms,
        &mut input,
        ferritecad_ui::ViewportEvent::PointerPressed(ferritecad_ui::PointerButton::Primary),
    );
    assert!(forms.viewport.gesturing());
    (egui::Context::default(), at)
}

pub(crate) fn release_held(forms: &mut Forms) {
    let mut input = input(forms);
    event(
        forms,
        &mut input,
        ferritecad_ui::ViewportEvent::PointerReleased(ferritecad_ui::PointerButton::Primary),
    );
    assert!(forms.viewport.take_hover(&mut input));
    assert!(!forms.viewport.gesturing());
}

pub(crate) fn move_coordinate(ctx: &egui::Context, forms: &mut Forms, from: &str, to: &str) {
    let from: f64 = from.trim().parse().expect("fixture coordinate");
    let to: f64 = to.trim().parse().expect("fixture coordinate");
    let mut input = input(forms);
    if forms.viewport.snapping.grid != crate::sketch::grid::Snap::Tenth {
        let label = forms.viewport.snapping.grid.label();
        crate::sketch::tests::press_with(ctx, forms, label);
        crate::sketch::tests::press_with(ctx, forms, "0.1 mm");
    }
    assert_eq!(
        forms.viewport.snapping.guides,
        crate::sketch::guide::Mode::Off
    );
    let positions: Vec<_> = curves(forms)
        .iter()
        .flat_map(super::super::rules::handles)
        .map(|(_, p)| p)
        .filter(|p| p.x == from || p.y == from)
        .collect();
    assert!(
        !positions.is_empty(),
        "the fixture has the requested saved coordinate"
    );
    for p in positions {
        let x = ((if p.x == from { to } else { p.x }) * 10.).round() / 10.;
        let y = ((if p.y == from { to } else { p.y }) * 10.).round() / 10.;
        pointer(forms, &mut input, p);
        event(
            forms,
            &mut input,
            ferritecad_ui::ViewportEvent::PointerPressed(ferritecad_ui::PointerButton::Primary),
        );
        assert!(forms.viewport.gesturing());
        pointer(forms, &mut input, point(x, y));
        assert!(forms.viewport.take_hover(&mut input));
        event(
            forms,
            &mut input,
            ferritecad_ui::ViewportEvent::PointerReleased(ferritecad_ui::PointerButton::Primary),
        );
        assert!(forms.viewport.take_hover(&mut input));
    }
}

impl Forms {
    pub(crate) fn set_test_session(&mut self, begin: bool, apply: bool, unsaved: bool) {
        self.set_session(begin, unsaved);
        self.viewport.set_apply(apply);
    }
}

/// Replay the literal polygon through free endpoint gestures, retaining saved order and IDs.
/// Each endpoint is its own history step; no Coincident rule is silently introduced.
pub(crate) fn polygon(path: &Path, points: &[[f64; 2]]) -> (Window, EditRequest, Vec<EditChange>) {
    let (sketch, saved) = stored(path);
    assert!(saved.constraints.is_empty());
    assert_eq!(saved.curves.len(), points.len());
    let expected = saved
        .curves
        .iter()
        .enumerate()
        .filter_map(|(i, curve)| {
            let start = points[i];
            let end = points[(i + 1) % points.len()];
            let geometry = SketchGeometry::Line {
                start: point(start[0], start[1]),
                end: point(end[0], end[1]),
            };
            (geometry != curve.geometry).then_some(EditChange::SetCurveGeometry {
                sketch,
                curve: curve.id,
                geometry,
            })
        })
        .collect::<Vec<_>>();
    let drawn = if ferritecad_occt::is_available() {
        Drawn::Native
    } else {
        Drawn::Mock
    };
    let mut w = open(path, drawn);
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    let mut lo = [-10.0_f64; 2];
    let mut hi = [20.0_f64; 2];
    for p in saved
        .curves
        .iter()
        .flat_map(|c| match c.geometry {
            SketchGeometry::Line { start, end } => vec![start, end],
            _ => panic!("polygon fixture"),
        })
        .chain(points.iter().map(|p| point(p[0], p[1])))
    {
        lo[0] = lo[0].min(p.x - 10.);
        lo[1] = lo[1].min(p.y - 10.);
        hi[0] = hi[0].max(p.x + 10.);
        hi[1] = hi[1].max(p.y + 10.);
    }
    w.input
        .frame((
            [lo[0] as f32, lo[1] as f32, -1.],
            [hi[0] as f32, hi[1] as f32, 1.],
        ))
        .expect("frame");
    ui.press(&mut w, "Off", false);
    ui.press(&mut w, "0.1 mm", false);
    assert_eq!(
        w.creates.sketch.viewport.snapping.guides,
        crate::sketch::guide::Mode::Off
    );
    for (curve, p) in saved.curves.iter().zip(points) {
        let SketchGeometry::Line { start, .. } = curve.geometry else {
            panic!("line")
        };
        let target = point(p[0], p[1]);
        if start == target {
            continue;
        }
        for _ in 0..2 {
            let before = curves(&w.creates.sketch);
            let depth = state(&w.creates.sketch).expect("draft").1;
            press(&mut w, start);
            assert!(w.creates.sketch.viewport.gesturing());
            pointer(&mut w.creates.sketch, &mut w.input, target);
            frame(&mut w);
            assert_eq!(
                state(&w.creates.sketch).expect("draft").1,
                depth,
                "pointer frames do not append history"
            );
            release(&mut w);
            let moved = curves(&w.creates.sketch);
            assert_ne!(moved, before);
            assert_eq!(state(&w.creates.sketch).expect("draft").1, depth + 1);
            ui.press(&mut w, "Undo", true);
            assert_eq!(curves(&w.creates.sketch), before);
            ui.press(&mut w, "Redo", true);
            assert_eq!(curves(&w.creates.sketch), moved);
            // A cancelled follow-up gesture leaves the committed drawing and history intact.
            press(&mut w, target);
            movement(&mut w, point(target.x + 3., target.y + 2.));
            frame(&mut w);
            ui.key(&mut w, egui::Key::Escape);
            release(&mut w);
            assert_eq!(curves(&w.creates.sketch), moved);
            assert_eq!(state(&w.creates.sketch).expect("draft").1, depth + 1);
        }
    }
    assert_eq!(
        w.creates
            .sketch
            .viewport
            .draft
            .as_ref()
            .expect("draft")
            .state
            .constraints,
        saved.constraints
    );
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, expected);
    (w, request, expected)
}

pub(crate) fn first_handle(forms: &Forms) -> Point2 {
    curves(forms)
        .iter()
        .flat_map(super::super::rules::handles)
        .next()
        .expect("fixture handle")
        .1
}

pub(crate) fn screen(forms: &Forms, input: &ViewportInput, p: Point2) -> (f32, f32) {
    let draft = forms.viewport.draft.as_ref().expect("draft");
    let p = super::super::geometry::screen(
        input.camera(),
        draft.planes[draft.plane].frame.expect("plane"),
        p,
    )
    .expect("projected point");
    input.region().window((p[0] as f32, p[1] as f32))
}

pub(crate) fn move_point(forms: &mut Forms, from: Point2, to: Point2) {
    let ctx = egui::Context::default();
    let mut input = input(forms);
    let a = screen(forms, &input, from);
    let b = screen(forms, &input, to);
    if (a.0 - b.0).hypot(a.1 - b.1) < 24. {
        // Resolve nearby targets beyond the object-snap aperture before dragging.
        input
            .frame((
                [
                    (from.x.min(to.x) - 2.) as f32,
                    (from.y.min(to.y) - 2.) as f32,
                    -1.,
                ],
                [
                    (from.x.max(to.x) + 2.) as f32,
                    (from.y.max(to.y) + 2.) as f32,
                    1.,
                ],
            ))
            .expect("close-up of the intended grid node");
    }
    set_grid_with(&ctx, forms, "0.1 mm");
    set_guides_with(&ctx, forms, "Off");
    pointer(forms, &mut input, from);
    event(
        forms,
        &mut input,
        ferritecad_ui::ViewportEvent::PointerPressed(ferritecad_ui::PointerButton::Primary),
    );
    assert!(forms.gesturing());
    pointer(forms, &mut input, to);
    forms.viewport.take_hover(&mut input);
    event(
        forms,
        &mut input,
        ferritecad_ui::ViewportEvent::PointerReleased(ferritecad_ui::PointerButton::Primary),
    );
    forms.viewport.take_hover(&mut input);
}

pub(crate) fn hover_first(forms: &mut Forms) -> bool {
    let mut input = input(forms);
    pointer(forms, &mut input, first_handle(forms));
    forms.viewport.take_hover(&mut input);
    transient(forms)
}

pub(crate) fn transient(forms: &Forms) -> bool {
    forms.viewport.cursor.is_some()
        || forms.viewport.hovered.is_some()
        || forms.viewport.snapped.is_some()
}

pub(crate) fn settings(forms: &Forms) -> (&'static str, &'static str) {
    (
        forms.viewport.snapping.grid.label(),
        forms.viewport.snapping.guides.label(),
    )
}

pub(crate) fn one_coordinate(forms: &mut Forms, from: &str, to: &str) {
    let from: f64 = from.trim().parse().expect("saved coordinate");
    let to: f64 = to.trim().parse().expect("grid coordinate");
    let points = curves(forms)
        .iter()
        .flat_map(super::super::rules::handles)
        .map(|(_, p)| p)
        .collect::<Vec<_>>();
    let original = *points
        .iter()
        .find(|p| p.x == from || p.y == from)
        .expect("fixture corner");
    let target = point(
        ((if original.x == from { to } else { original.x }) * 10.).round() / 10.,
        ((if original.y == from { to } else { original.y }) * 10.).round() / 10.,
    );
    for _ in points.iter().filter(|p| **p == original) {
        move_point(forms, original, target);
    }
}

pub(crate) fn detached_polygon(
    path: &Path,
    source: &Source,
    points: &[[f64; 2]],
) -> (Forms, EditRequest) {
    let (mut w, mut request, _) = polygon(path, points);
    assert_eq!(request.expected, source.version);
    let mut forms = std::mem::take(&mut w.creates.sketch);
    forms.viewport.source = path.to_path_buf();
    forms.viewport.clear_pointer();
    request.source = path.to_path_buf();
    (forms, request)
}

fn set_grid_with(ctx: &egui::Context, forms: &mut Forms, label: &str) {
    let current = forms.viewport.snapping.grid.label();
    if current != label {
        crate::sketch::tests::press_with(ctx, forms, current);
        crate::sketch::tests::press_with(ctx, forms, label);
    }
}
fn set_guides_with(ctx: &egui::Context, forms: &mut Forms, label: &str) {
    let current = forms.viewport.snapping.guides.label();
    if current != label {
        crate::sketch::tests::press_with(ctx, forms, current);
        crate::sketch::tests::press_with(ctx, forms, label);
    }
}
pub(crate) fn set_grid(forms: &mut Forms, label: &str) {
    set_grid_with(&egui::Context::default(), forms, label);
}
pub(crate) fn set_guides(forms: &mut Forms, label: &str) {
    set_guides_with(&egui::Context::default(), forms, label);
}

/// Set an existing visible dimension through the viewport field and finish the draft.
pub(crate) fn set_dimension(
    w: &mut Window,
    id: ObjectId,
    dimension: StableEntityId,
    text: &str,
) -> EditRequest {
    enter(w, id, ModelEdit::Sketch);
    let mut ui = Ui::new(w);
    super::constraint_widgets::click_label(w, &mut ui, dimension);
    ui.replace_dimension_text(w, text);
    ui.press(w, "Set", false);
    finish(w, &mut ui)
}
