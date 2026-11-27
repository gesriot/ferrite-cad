// SPDX-License-Identifier: MIT
//! Saved round drawings use the same Finish gate as the window's handler.
//! Copy-specific guards stay on the surviving Revolve angle form.

use super::*;
use crate::feature_tests::write_round;
use crate::sketch_facts::Source as ExtrudeEditSource;
use crate::sketch_mode::tests::{curve_edits, round_edits};
use crate::{Loads, creates, edits, exports, numeric, sessions};
use ferritecad_types::ObjectId;

struct Window {
    _private: tempfile::TempDir,
    sessions: sessions::Sessions,
    creates: creates::Creates,
    loads: Loads,
    exports: exports::Exports,
    edits: edits::Edits,
}
impl Window {
    fn open(path: &Path) -> Self {
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = sessions::Sessions::default();
        sessions.adopt(
            crate::sessions::test_open::open_in(
                private.path(),
                path,
                ferritecad_jobs::HistoryLimits::default(),
            )
            .expect("session"),
        );
        Self {
            _private: private,
            sessions,
            creates: Default::default(),
            loads: Default::default(),
            exports: Default::default(),
            edits: Default::default(),
        }
    }
    fn ready(&self) -> bool {
        self.creates.sketch.viewport.active()
            && numeric::can_apply(
                &self.creates,
                &self.loads,
                &self.exports,
                &self.edits,
                &self.sessions,
            )
    }
    fn tell(&mut self) {
        let ready = self.ready();
        self.creates.sketch.viewport.set_apply(ready);
    }
    fn form(&mut self) -> &mut Editor {
        &mut self.creates.sketch
    }
    fn begin(&mut self, path: &Path, source: &ExtrudeEditSource) {
        assert!(
            self.form()
                .begin_test_drawing(path, source, curve_edits::sketch_id(source))
        );
        self.tell();
    }
}
fn document(root: &Path, annulus: bool) -> (PathBuf, ExtrudeEditSource) {
    let path = root.join(if annulus {
        "annulus.fcad"
    } else {
        "circle.fcad"
    });
    let radii = if annulus {
        vec![10.5, 4.75]
    } else {
        vec![10.5]
    };
    write_round(&path, [12.5, -7.25], &radii, 15.25, false);
    let source = crate::sketch_facts::read(&path).expect("reading");
    (path, source)
}
fn move_center(ctx: &egui::Context, e: &mut Editor) {
    curve_edits::move_coordinate(ctx, e, "12.5", "-3.5");
    curve_edits::move_coordinate(ctx, e, "-7.3", "4.3");
}

#[test]
fn the_open_forms_apply_through_the_windows_own_predicate_and_keep_their_drafts() {
    let root = tempfile::tempdir().expect("root");
    for annulus in [false, true] {
        let (path, source) = document(root.path(), annulus);
        let mut w = Window::open(&path);
        assert!(!w.ready());
        w.begin(&path, &source);
        assert!(w.creates.busy(), "New and Open wait for the drawing");
        assert!(w.ready(), "the open form must not block its own Finish");
        let ctx = egui::Context::default();
        move_center(&ctx, w.form());
        let draft = curve_edits::state(w.form());
        w.form().viewport.set_apply(false);
        press_with(&ctx, w.form(), "Finish");
        assert!(w.form().viewport.take_request().is_none());
        assert_eq!(curve_edits::state(w.form()), draft);
        w.tell();
        press_with(&ctx, w.form(), "Finish");
        let request = w.form().viewport.take_request().expect("Finish");
        assert_eq!(request.source, path);
        assert_eq!(request.expected, source.version);
        let (sketch, saved) = curve_edits::stored(&path);
        let expected = saved
            .curves
            .iter()
            .map(|curve| {
                let ferritecad_document::SketchGeometry::Circle { radius, .. } = curve.geometry
                else {
                    panic!("round fixture")
                };
                ferritecad_document::EditChange::SetCurveGeometry {
                    sketch,
                    curve: curve.id,
                    geometry: ferritecad_document::SketchGeometry::Circle {
                        center: ferritecad_document::Point2 { x: -3.5, y: 4.3 },
                        radius,
                    },
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(request.changes, expected);
        assert!(
            w.form().viewport.take_request().is_none(),
            "one press, one request"
        );
        assert_eq!(
            curve_edits::state(w.form()),
            draft,
            "Finish keeps the pending draft"
        );
        assert!(w.ready());
    }
}

#[test]
fn nothing_else_running_or_open_lets_the_command_start() {
    let root = tempfile::tempdir().expect("root");
    let (path, source) = document(root.path(), false);
    let mut w = Window::open(&path);
    w.begin(&path, &source);
    w.loads
        .open(
            Some(&path),
            std::sync::Arc::new(crate::ProgressRelay::default()),
            |_, _| std::thread::spawn(|| {}),
        )
        .expect("load");
    assert!(!w.ready(), "Open blocks Finish");
    w.loads.stop_all();
    w.loads = Loads::default();
    assert!(w.ready());
    w.sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("operation");
    assert!(!w.ready(), "session work blocks Finish");
    w.sessions.stop_all();
    let mut w = Window::open(&path);
    w.begin(&path, &source);
    assert!(w.creates.busy());
    w.form().dismiss();
    assert!(!w.ready());
    w.begin(&path, &source);
    w.sessions = sessions::Sessions::default();
    assert!(!w.ready(), "no session, no Finish");
}

#[test]
fn the_stored_numbers_offer_no_apply_and_a_draft_round_trip_keeps_its_redo() {
    // A literal field and its no-op Apply belong to the surviving angle form.
    // Finish of an unchanged drawing instead closes that drawing by design.
    let (_root, path, source) = angle_apply::fixture();
    let id = ferritecad_fixtures::expression_slot(
        &source,
        ferritecad_document::ExpressionSlot::RevolveAngle,
    )
    .object;
    let mut e = Editor::default();
    e.set_session(true, false);
    e.set_angle_apply(true);
    assert!(e.begin_angle_edit(&path, &source, id));
    let ctx = egui::Context::default();
    frame(&ctx, &mut e, vec![]);
    let original = e.angle.angle_edit.clone();
    press_with(&ctx, &mut e, "Apply angle");
    assert!(e.take_apply_angle_request().is_none());
    replace_one_in_form(&ctx, &mut e, &original, "212.25");
    press_with(&ctx, &mut e, "Confirm draft numbers");
    press_with(&ctx, &mut e, "Undo draft");
    assert_eq!(e.angle.angle_edit, original);
    let redo = e.angle.angle_redo.clone();
    assert_eq!(redo.len(), 1);
    press_with(&ctx, &mut e, "Apply angle");
    assert!(e.take_apply_angle_request().is_none());
    assert_eq!(e.angle.angle_redo, redo);
    replace_one_in_form(&ctx, &mut e, &original, "137.50");
    press_with(&ctx, &mut e, "Apply angle");
    assert!(e.take_apply_angle_request().is_none());
}

#[test]
fn with_unsaved_changes_save_copy_is_withheld_with_words_and_apply_is_not() {
    let (_root, path, source) = angle_apply::fixture();
    let id = ferritecad_fixtures::expression_slot(
        &source,
        ferritecad_document::ExpressionSlot::RevolveAngle,
    )
    .object;
    let mut e = Editor::default();
    e.set_session(true, true);
    e.set_angle_apply(true);
    assert!(e.begin_angle_edit(&path, &source, id));
    let ctx = egui::Context::default();
    replace_one_in_form(&ctx, &mut e, "137.5", "212.25");
    assert!(painted(&frame(&ctx, &mut e, vec![]), "unsaved changes"));
    press_with(&ctx, &mut e, "Save edited Revolve copy…");
    assert!(e.take_angle_edit_request().is_none());
    press_with(&ctx, &mut e, "Apply angle");
    assert!(e.take_apply_angle_request().is_some());
    e.set_session(true, false);
    press_with(&ctx, &mut e, "Confirm draft numbers");
    press_with(&ctx, &mut e, "Save edited Revolve copy…");
    assert!(e.take_angle_edit_request().is_some());
}

#[test]
fn a_new_accepted_version_ends_the_drafts_and_keeps_what_the_window_said() {
    let root = tempfile::tempdir().expect("root");
    for annulus in [false, true] {
        let (path, source) = document(root.path(), annulus);
        let mut w = Window::open(&path);
        w.begin(&path, &source);
        let ctx = egui::Context::default();
        move_center(&ctx, w.form());
        w.form().finish_session_change();
        assert!(!w.form().active(), "draft outlived its accepted scene");
        assert!(w.form().viewport.take_request().is_none());
        w.begin(&path, &source);
        move_center(&ctx, w.form());
        press_with(&ctx, w.form(), "Finish");
        assert!(w.form().viewport.take_request().is_some());
    }
}

#[test]
fn the_forms_open_on_a_document_with_unsaved_changes_only_when_the_window_says_so() {
    use crate::sessions::tests::tabs::{Drawn, Window as Tabs};
    use ferritecad_ui::ModelEdit;
    let root = tempfile::tempdir().expect("root");
    for annulus in [false, true] {
        let (path, _) = document(root.path(), annulus);
        for idle in [false, true] {
            let mut w = Tabs::new(Drawn::Mock, None);
            w.open(&path);
            w.apply(19.);
            assert!(w.sessions.dirty());
            if !idle {
                w.sessions
                    .begin_apply(|_, _, _| std::thread::spawn(|| {}))
                    .expect("busy");
            }
            let source = w.scene.edit_source.as_ref().expect("source");
            let navigation = w.tabs.model_navigation(w.sessions.tab());
            let row = source
                .model_tree
                .rows
                .iter()
                .find(|r| r.id.0 == curve_edits::sketch_id(source))
                .expect("row")
                .id;
            crate::commands::select_row(source, navigation, row);
            let action = ferritecad_ui::ModelAction {
                tab: w.sessions.tab().key(),
                epoch: navigation.epoch,
                row,
                edit: crate::commands::Command::Edit(ModelEdit::Sketch),
            };
            let answer = crate::commands::launch(
                action,
                source,
                &w.scene.sketches,
                &mut w.creates,
                &mut w.edits,
                &w.sessions,
                &w.tabs,
                &Loads::default(),
                &exports::Exports::default(),
                &w.input,
                false,
            );
            assert_eq!(answer.is_ok(), idle);
            assert_eq!(w.creates.sketch.viewport.active(), idle);
            w.sessions.stop_all();
        }
    }
}

#[test]
fn the_documents_last_outcome_is_readable_inside_the_open_form() {
    let root = tempfile::tempdir().expect("root");
    for annulus in [false, true] {
        let (path, source) = document(root.path(), annulus);
        let mut e = Editor::default();
        assert!(e.begin_test_drawing(&path, &source, curve_edits::sketch_id(&source)));
        let ctx = egui::Context::default();
        let line = "Could not apply the change: the radius is too large";
        let mut draw = |outcome| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000., 900.),
                    )),
                    ..Default::default()
                },
                |ui| e.viewport.draw(ui, false, outcome),
            );
            output.textures_delta.clear();
            output
        };
        for _ in 0..3 {
            draw("");
        }
        let out = draw(line);
        assert!(painted(&out, line));
        let at = text_at(&out, line);
        let clip = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "Sketch drawing" => Some(s.clip_rect),
                _ => None,
            })
            .expect("drawing heading");
        assert!(clip.contains(at));
        assert!(!painted(&draw(""), line));
    }
}

#[test]
fn while_a_worker_is_busy_the_forms_start_no_second_request() {
    let root = tempfile::tempdir().expect("root");
    for annulus in [false, true] {
        let (path, source) = document(root.path(), annulus);
        let mut w = Window::open(&path);
        w.begin(&path, &source);
        let ctx = egui::Context::default();
        move_center(&ctx, w.form());
        let state = curve_edits::state(w.form());
        let out = frame_running(&ctx, w.form(), vec![], true);
        let at = text_at(&out, "Finish");
        frame_running(&ctx, w.form(), vec![egui::Event::PointerMoved(at)], true);
        for pressed in [true, false] {
            frame_running(
                &ctx,
                w.form(),
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
                true,
            );
        }
        assert!(w.form().viewport.take_request().is_none());
        assert_eq!(curve_edits::state(w.form()), state);
    }
}

/// A round draft and its real Finish request for ownership tests on an existing session.
pub(crate) fn typed_circle(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    center: [&str; 2],
    radius: &str,
) -> (Editor, EditRequest) {
    round_edits::detached(
        path,
        source,
        sketch,
        center.map(|n| (n.trim().parse::<f64>().expect("coordinate") * 10.).round() / 10.),
        &[radius.trim().parse().expect("radius")],
    )
}
pub(crate) fn typed_annulus(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    center: [&str; 2],
    outer: &str,
    inner: &str,
) -> (Editor, EditRequest) {
    let (_, saved) = curve_edits::stored(path);
    let outer = outer.trim().parse::<f64>().expect("outer radius");
    let inner = inner.trim().parse::<f64>().expect("inner radius");
    let radii = saved
        .curves
        .iter()
        .map(|c| match c.geometry {
            ferritecad_document::SketchGeometry::Circle { radius, .. } if radius > 5. => outer,
            ferritecad_document::SketchGeometry::Circle { .. } => inner,
            _ => panic!("round fixture"),
        })
        .collect::<Vec<_>>();
    round_edits::detached(
        path,
        source,
        sketch,
        center.map(|n| (n.trim().parse::<f64>().expect("coordinate") * 10.).round() / 10.),
        &radii,
    )
}
