// SPDX-License-Identifier: MIT
use super::*;

#[test]
fn a_held_sketch_keeps_its_lines_and_selection_until_cancel() {
    use crate::sketch_mode::tests::widgets::Ui;
    use crate::sketch_mode::tests::{curve_edits, window};
    use ferritecad_document::{EditChange, Point2, SketchCurve, SketchGeometry};

    fn painted(w: &mut Window) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            w.creates
                .sketch
                .viewport
                .paint(ui.ctx(), w.input.camera(), w.input.region());
        });
        out.textures_delta.clear();
        out.shapes
            .into_iter()
            .map(|c| c.shape)
            .filter(|s| matches!(s, egui::Shape::Path(_)))
            .collect()
    }

    let user = tempfile::tempdir().expect("user");
    let (a_file, b_file) = (user.path().join("a.fcad"), user.path().join("b.fcad"));
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    let feature = w.feature();

    let (sketch, saved) = curve_edits::stored(&a_file);
    w.input
        .frame(([-80., -80., -20.], [80., 80., 20.]))
        .expect("frame plate");
    window::enter(&mut w, sketch, ferritecad_ui::ModelEdit::Sketch);
    let SketchGeometry::Line { start, end } = saved.curves[1].geometry else {
        panic!("plate side");
    };
    window::click(
        &mut w,
        Point2 {
            x: (start.x + end.x) / 2.,
            y: (start.y + end.y) / 2.,
        },
        false,
    );
    let base = crate::sketch_mode::tests::window::version(&w);
    let rgb = ferritecad_viewport_gpu::SKETCH_SELECTED_COLOUR.map(|c| (255. * c) as u8);
    let selected =
        egui::epaint::PathStroke::new(1., egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
    let strokes = |w: &mut Window| {
        painted(w).iter().map(|shape| {
            matches!(shape, egui::Shape::Path(path) if path.stroke.color == selected.color)
        }).collect::<Vec<_>>()
    };
    let retained = curve_edits::curves(&w.creates.sketch);
    assert_eq!(retained, saved.curves);
    assert_eq!(strokes(&mut w), [false, true, false, false]);
    w.open(&b_file);
    let added = SketchCurve {
        id: ferritecad_types::StableEntityId::new(),
        construction: true,
        geometry: SketchGeometry::Line {
            start: Point2 { x: 4., y: 1. },
            end: Point2 { x: 4., y: 8. },
        },
    };

    // Behind the window's back A's session accepts another version.
    let hidden = w.tabs.hidden_sessions_mut(a).expect("A");
    let (tx, rx) = mpsc::channel();
    let generation = hidden
        .apply(
            hidden.export_source().expect("open").version(),
            vec![
                EditChange::constant(
                    feature,
                    ferritecad_document::ExpressionSlot::ExtrudeDistance,
                    20.0,
                )
                .expect("test value"),
                EditChange::AddCurve {
                    sketch,
                    curve: added.clone(),
                },
            ],
            move |_, reply| {
                tx.send(reply).expect("deliver");
            },
        )
        .expect("started");
    assert!(matches!(
        hidden.finish_apply(generation, wait(&rx)),
        Edited::Show(_)
    ));
    hidden.commit_staged().expect("accepted");
    assert!(hidden.finish_scene(generation, Ok(())));

    w.round(a);
    assert!(w.sessions.stale_draft(), "the form is held");

    assert_ne!(crate::sketch_mode::tests::window::version(&w), base);
    assert!(w.creates.sketch.viewport.active());
    assert_eq!(curve_edits::curves(&w.creates.sketch), retained);
    assert_eq!(strokes(&mut w), [false, true, false, false]);
    assert!(!crate::numeric::can_submit_sketch_or_sweep(
        &w.creates,
        &crate::Loads::default(),
        &crate::exports::Exports::default(),
        &w.edits,
        &w.sessions,
    ));
    let current = window::version(&w);
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Finish", false);
    assert!(w.creates.sketch.viewport.active());
    assert!(w.creates.sketch.viewport.take_request().is_none());
    assert_eq!(current, window::version(&w));
    ui.press(&mut w, "Cancel", false);
    assert!(!w.creates.sketch.viewport.active());
    w.sessions.release_stale_draft();
    assert!(!w.sessions.stale_draft());
    let mut expected = retained;
    expected.push(added);
    assert_eq!(
        curve_edits::stored(&w.sessions.export_path().expect("accepted"))
            .1
            .curves,
        expected
    );
    window::enter(&mut w, sketch, ferritecad_ui::ModelEdit::Sketch);
    assert_eq!(curve_edits::curves(&w.creates.sketch), expected);
    assert_eq!(strokes(&mut w), [false; 5]);
    assert_eq!(current, window::version(&w));
}
