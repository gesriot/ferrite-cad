// SPDX-License-Identifier: MIT
//! Tab-owned sketch, sweep and numeric forms, separate from the accepted scene.
use crate::sketch_facts::Source as ExtrudeEditSource;
use ferritecad_jobs::EditRequest;
use ferritecad_types::{CadError, Result};
use std::path::{Path, PathBuf};

pub(crate) mod accepted;
pub(crate) mod dimension;
pub(crate) mod grid;
pub(crate) mod guide;
pub(crate) mod ink;
pub(crate) mod marker;
mod navigation;
mod revolve_angle;

#[derive(Debug, Default)]
pub(crate) struct Editor {
    /// The viewport drawing form travels with this same tab-owned editor.
    pub(crate) viewport: crate::sketch_mode::Editor,
    /// Sweep drafts share saved-object ownership, tab parking and publication.
    pub(crate) sweeps: crate::sweeps::Editor,
    /// Text editing over the current accepted document, following Model selection.
    pub(crate) parameters: crate::parameters::Editor,
    pub(crate) constraints: crate::constraints::Editor,
    /// The circular cut form. Its own editor beside the constraint one: a cut
    /// is added to a body's history, not to a drawing, and it reads a different
    /// catalogue of the same pinned reading.
    pub(crate) cuts: crate::cuts::Editor,
    /// §28A: the single-edge fillet form, beside the cut one for the same
    /// reason: it adds to a Body's history and reads its own catalogue.
    pub(crate) fillets: crate::fillets::Editor,
    pub(crate) keep_solids: crate::keep_solids::Commands,
    /// §29A: the single-edge chamfer forms, beside the fillet one for the same
    /// reason: they add to a Body's history (or change its one number) and read
    /// their own catalogue.
    pub(crate) chamfers: crate::chamfers::Editor,
    /// Permission to open the saved angle form on the current session.
    can_begin_angle: bool,
    /// Unsaved changes withhold the retained numeric form's copy action.
    unsaved: bool,
    /// The saved partial-turn angle form and its local history.
    angle: revolve_angle::Draft,
    /// Publication is complete, but its picture has not yet been accepted.
    /// Keep one recovery draft without preventing the ordinary async Open.
    published_draft: Option<(PathBuf, Box<Editor>)>,
}

/// How many confirmed angle checkpoints the numeric form keeps.
const DRAFT_HISTORY: usize = 128;

fn push_bounded<T>(stack: &mut Vec<T>, value: T) {
    if stack.len() == DRAFT_HISTORY {
        stack.remove(0);
    }
    stack.push(value);
}

impl Editor {
    pub(crate) fn edge_and_piece_requests(&mut self) -> impl Iterator<Item = EditRequest> + use<> {
        [
            self.fillets.take_apply_request(),
            self.fillets.take_add_request(),
            self.chamfers.take_apply_request(),
            self.chamfers.take_add_request(),
            self.keep_solids.take_request(),
        ]
        .into_iter()
        .flatten()
    }
    pub(crate) fn only_editing_cut(&self) -> bool {
        self.cuts.editing_saved()
            && self.angle.editing_angle.is_none()
            && !self.constraints.active()
            && !self.fillets.active()
            && !self.chamfers.active()
    }
    /// §30I: the Add form of a Cut is the only form open.
    pub(crate) fn only_adding_cut(&self) -> bool {
        self.cuts.adding()
            && self.angle.editing_angle.is_none()
            && !self.constraints.active()
            && !self.fillets.active()
            && !self.chamfers.active()
    }
    /// §30J: the Add form of a Fillet is the only form open.
    pub(crate) fn only_adding_fillet(&self) -> bool {
        self.fillets.adding()
            && self.angle.editing_angle.is_none()
            && !self.constraints.active()
            && !self.cuts.active()
            && !self.chamfers.active()
    }
    pub(crate) fn only_editing_fillet_radius(&self) -> bool {
        self.fillets.editing_radius()
            && self.angle.editing_angle.is_none()
            && !self.constraints.active()
            && !self.cuts.active()
            && !self.chamfers.active()
    }
    /// §30K: the Add form of a Chamfer is the only form open.
    pub(crate) fn only_adding_chamfer(&self) -> bool {
        self.chamfers.adding()
            && self.angle.editing_angle.is_none()
            && !self.constraints.active()
            && !self.cuts.active()
            && !self.fillets.active()
    }
    pub(crate) fn only_editing_chamfer_distance(&self) -> bool {
        self.chamfers.editing_distance()
            && self.angle.editing_angle.is_none()
            && !self.constraints.active()
            && !self.cuts.active()
            && !self.fillets.active()
    }
    fn outcome_line(ui: &mut egui::Ui, outcome: &str) {
        if !outcome.is_empty() {
            ui.separator();
            ui.label(format!("Document: {outcome}"));
        }
    }
    pub(crate) fn active(&self) -> bool {
        self.angle.editing_angle.is_some()
            || self.constraints.active()
            || self.cuts.active()
            || self.fillets.active()
            || self.chamfers.active()
            || self.parameters.active()
            || self.viewport.active()
            || self.sweeps.active()
    }
    pub(crate) fn dismiss(&mut self) {
        // What the window said about the session is not the draft's: it is said
        // again every frame, but a draft that is replaced must not forget it for
        // the frame in between.
        let (begin, unsaved) = (self.can_begin_angle, self.unsaved);
        let angle = self.angle.can_apply_angle;
        let constraints = self.constraints.session();
        let cuts = self.cuts.session();
        let cuts_add = self.cuts.add_session();
        let fillets = self.fillets.session();
        let fillets_add = self.fillets.add_session();
        let chamfers = self.chamfers.session();
        let chamfers_add = self.chamfers.add_session();
        *self = Self::default();
        self.can_begin_angle = begin;
        self.angle.can_apply_angle = angle;
        self.unsaved = unsaved;
        self.constraints
            .set_session(constraints.0, constraints.1, constraints.2);
        self.cuts.set_session(cuts.0, cuts.1, cuts.2);
        self.cuts.set_add(cuts_add);
        self.fillets.set_session(fillets.0, fillets.1, fillets.2);
        self.fillets.set_add(fillets_add);
        self.chamfers
            .set_session(chamfers.0, chamfers.1, chamfers.2);
        self.chamfers.set_add(chamfers_add);
    }
    /// What the window tells the form each frame: whether Apply is possible and
    /// whether unsaved changes withhold the copy workflow.
    pub(crate) fn set_session(&mut self, can_begin: bool, unsaved: bool) {
        self.can_begin_angle = can_begin;
        self.unsaved = unsaved;
    }
    /// Saved-object forms describe the picture that was replaced and end here.
    /// Parameters reads each accepted version and keeps its unsubmitted text.
    pub(crate) fn finish_session_change(&mut self) {
        if self.saved_object_open() && !self.parameters.active() {
            self.dismiss();
        }
    }
    /// A form about a saved object of the open document is open: what a hidden
    /// tab keeps (§30P).
    pub(crate) fn saved_object_open(&self) -> bool {
        self.angle.editing_angle.is_some()
            || self.constraints.active()
            || self.cuts.active()
            || self.fillets.active()
            || self.chamfers.active()
            || self.parameters.active()
            || self.viewport.active()
            || self.sweeps.active()
    }
    /// A captured sketch-mode handle or constraint-canvas pan holds window transitions
    /// until the pointer is released or the gesture is cancelled.
    pub(crate) fn gesturing(&self) -> bool {
        self.viewport.gesturing() || self.constraints.panning()
    }
    pub(crate) fn take_cut_request(&mut self) -> Option<ferritecad_jobs::EditRequest> {
        self.cuts.take_request()
    }
    pub(crate) fn take_cut_edit_request(&mut self) -> Option<ferritecad_jobs::EditRequest> {
        self.cuts.take_edit_request()
    }
    pub(crate) fn take_fillet_request(&mut self) -> Option<ferritecad_jobs::EditRequest> {
        self.fillets.take_request()
    }
    pub(crate) fn take_fillet_radius_request(&mut self) -> Option<ferritecad_jobs::EditRequest> {
        self.fillets.take_radius_request()
    }
    pub(crate) fn take_chamfer_request(&mut self) -> Option<ferritecad_jobs::EditRequest> {
        self.chamfers.take_request()
    }
    pub(crate) fn take_chamfer_distance_request(&mut self) -> Option<ferritecad_jobs::EditRequest> {
        self.chamfers.take_distance_request()
    }
    pub(crate) fn draft_published(&mut self, path: &Path) {
        let mut saved = std::mem::take(self);
        saved.published_draft = None;
        self.published_draft = Some((path.to_path_buf(), Box::new(saved)));
    }
    /// Called only for a current load, after scene preparation/commit decides.
    pub(crate) fn draft_load_finished(&mut self, path: &Path, accepted: bool) {
        if accepted {
            self.published_draft = None;
        } else if self
            .published_draft
            .as_ref()
            .is_some_and(|(p, _)| p == path)
        {
            let (_, saved) = self.published_draft.take().expect("matching publication");
            *self = *saved;
        }
    }
    pub(crate) fn draw_choices(
        &mut self,
        ui: &mut egui::Ui,
        can_begin: bool,
        path: Option<&Path>,
        source: Option<&ExtrudeEditSource>,
        accepted: &accepted::Accepted,
    ) {
        if self.active() {
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("saved-sketch-actions")
            .max_height(180.)
            .show(ui, |ui| {
                self.draw_choice_rows(ui, can_begin, path, source, accepted);
            });
    }
    fn draw_choice_rows(
        &mut self,
        ui: &mut egui::Ui,
        can_begin: bool,
        path: Option<&Path>,
        source: Option<&ExtrudeEditSource>,
        accepted: &accepted::Accepted,
    ) {
        if !self.active() {
            self.constraints
                .choices(ui, can_begin, path, source, accepted);
            self.cuts.choices(ui, can_begin, path, source);
            self.fillets.choices(ui, can_begin, path, source);
            self.keep_solids.draw(ui, path, source);
            self.chamfers.choices(ui, can_begin, path, source);
        }
        if self.active() {
            return;
        }
        if let (Some(path), Some(source)) = (path, source) {
            for choice in source
                .expression_slots
                .iter()
                .filter(|s| s.slot == ferritecad_document::ExpressionSlot::RevolveAngle)
            {
                let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
                let response = ui.add_enabled(
                    (can_begin || self.can_begin_angle) && refusal.is_none(),
                    egui::Button::new(format!(
                        "Edit Revolve angle {} — {}…",
                        choice.name.as_deref().unwrap_or("Unnamed"),
                        choice.object
                    )),
                );
                if response.clicked() {
                    self.begin_angle_edit(path, source, choice.object);
                }
                if let Some(reason) = refusal {
                    response.on_hover_text(reason);
                }
            }
        }
    }
    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, running: bool, outcome: &str) {
        if self.sweeps.active() {
            self.sweeps.draw(ui, running, outcome);
            return;
        }
        if self.viewport.active() {
            self.viewport.draw(ui, running, outcome);
            return;
        }
        // §31J: a pan whose canvas is no longer drawn (another profile was chosen while
        // the middle button was held) would hold the window for good.
        let frame = ui.ctx().cumulative_frame_nr();
        self.constraints.forget_undrawn(frame);
        if self.constraints.active() {
            self.constraints.draw(ui, running);
            return;
        }
        if self.cuts.active() {
            self.cuts.set_outcome(outcome);
            self.cuts.draw(ui, running);
            return;
        }
        if self.fillets.active() {
            self.fillets.set_outcome(outcome);
            self.fillets.draw(ui, running);
            return;
        }
        if self.chamfers.active() {
            self.chamfers.set_outcome(outcome);
            self.chamfers.draw(ui, running);
            return;
        }
        // Parameters is drawn with the current accepted source by the workspace.
        if !self.active() || self.parameters.active() {
            return;
        }
        crate::form_window(ui, "Edit saved Revolve angle")
            .resizable(false)
            .default_width(540.)
            .show(ui.ctx(), |ui| self.draw_angle_draft(ui, running, outcome));
    }
}

pub(crate) fn finish_edit(
    editor: &mut Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::EditedDocument>,
) -> Option<PathBuf> {
    if edits.accepts(generation)
        && let Ok(saved) = &result
    {
        editor.draft_published(&saved.destination);
    }
    edits.finish(generation, result)
}

/// Finish one Revolve angle edit at the application boundary (§27E), through
/// the same publication and accepted-picture recovery as the other numeric forms.
pub(crate) fn finish_angle_edit(
    editor: &mut Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::EditedDocument>,
) -> Option<PathBuf> {
    if edits.accepts(generation)
        && let Ok(saved) = &result
    {
        editor.draft_published(&saved.destination);
    }
    edits.finish(generation, result)
}

/// Coordinates beyond this are not drawn by the constraint canvas.
const DRAWABLE_MM: f64 = 1e6;

/// How near a press must be to a vertex or a handle to take it, in egui logical
/// points: independent of zoom and of the display's pixels per point (§25C).
const PICK_POINTS: f32 = 9.0;

/// The screen point of the document point `p` (mm) on a canvas `rect` whose
/// lower-left corner shows `minimum` at `scale` logical points per mm; +Y is up.
/// The transform of the retained constraint canvas.
fn canvas_point(rect: egui::Rect, minimum: [f64; 2], scale: f32, p: [f64; 2]) -> egui::Pos2 {
    rect.left_bottom()
        + egui::vec2(
            ((p[0] - minimum[0]) * f64::from(scale)) as f32,
            -((p[1] - minimum[1]) * f64::from(scale)) as f32,
        )
}

/// The document point (mm) under the screen position `pos` of that canvas.
fn canvas_mm(rect: egui::Rect, minimum: [f64; 2], scale: f32, pos: egui::Pos2) -> [f64; 2] {
    let origin = rect.left_bottom();
    [
        minimum[0] + f64::from(pos.x - origin.x) / f64::from(scale),
        minimum[1] + f64::from(origin.y - pos.y) / f64::from(scale),
    ]
}

/// A canvas's background and millimetre grid; returns the grid step in mm. At most
/// ~50 lines, even for very small or far-translated drawings.
fn paint_grid(painter: &egui::Painter, rect: egui::Rect, minimum: [f64; 2], scale: f32) -> f64 {
    painter.rect_filled(rect, 0., egui::Color32::from_gray(25));
    let grid = 10_f64.powf((40.0 / f64::from(scale)).log10().floor());
    for axis in 0..2 {
        for i in 0..55 {
            let value = (minimum[axis] / grid).ceil() * grid + f64::from(i) * grid;
            let mut p = minimum;
            p[axis] = value;
            let start = canvas_point(rect, minimum, scale, p);
            let end = if axis == 0 {
                egui::pos2(start.x, rect.top())
            } else {
                egui::pos2(rect.right(), start.y)
            };
            painter.line_segment([start, end], (1., egui::Color32::from_gray(48)));
        }
    }
    grid
}

/// Whether a press or a move at `pos` is on this canvas, as an input the form takes:
/// the form is enabled, the point is visible on the canvas and no other window covers it.
fn pointer_over(ui: &egui::Ui, visible: egui::Rect, pos: egui::Pos2) -> bool {
    ui.is_enabled()
        && ui.input(|i| i.focused)
        && visible.contains(pos)
        && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
}

pub(crate) fn number(s: &str) -> Result<f64> {
    s.trim()
        .parse()
        .map_err(|_| CadError::input(format!("Not a number: {s:?}")))
}

#[cfg(test)]
#[allow(clippy::panic)]
pub(crate) mod tests {
    use super::*;
    use crate::sketch_facts::SketchProfileUse;
    pub(crate) mod analytic_apply;
    pub(crate) mod angle_apply;
    /// §30Q: New over a tab's floating form, through real widgets.
    mod new_over_forms;
    /// §30T: the question asked when a file is chosen over an unfinished New.
    mod open_over_new;

    #[test]
    fn floating_forms_in_two_tabs_do_not_share_keyboard_focus() {
        let (_root, path, reading) = angle_apply::fixture();
        let id = ferritecad_fixtures::expression_slot(
            &reading,
            ferritecad_document::ExpressionSlot::RevolveAngle,
        )
        .object;
        let mut a = Editor::default();
        let mut b = Editor::default();
        for editor in [&mut a, &mut b] {
            assert!(editor.begin_angle_edit(&path, &reading, id));
        }
        let ctx = egui::Context::default();
        let frame = |editor: &mut Editor, tab, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(988., 768.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    crate::in_tab_scope(ui, "tab-forms", tab, |ui| {
                        editor.draw(ui, false, "");
                    });
                },
            );
            output.textures_delta.clear();
            output
        };
        frame(&mut a, 1, vec![]);
        let out = frame(&mut a, 1, vec![]);
        let field = text_at(&out, "137.5");
        for pressed in [true, false] {
            frame(
                &mut a,
                1,
                vec![
                    egui::Event::PointerMoved(field),
                    egui::Event::PointerButton {
                        pos: field,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        frame(&mut a, 1, vec![egui::Event::Text("5".into())]);
        let typed_a = a.angle.angle_edit.clone();
        assert_ne!(typed_a, "137.5", "A received keyboard input");
        frame(&mut b, 2, vec![]);
        frame(&mut b, 2, vec![egui::Event::Text("9".into())]);
        frame(&mut b, 2, vec![egui::Event::Text("9".into())]);
        assert_eq!(
            b.angle.angle_edit, "137.5",
            "A's focus edited B's floating form"
        );
        assert_eq!(a.angle.angle_edit, typed_a);
    }

    /// Move the matching sketch corner with real pointer input, or replace the
    /// retained angle form's field through its text widget.
    pub(crate) fn replace_one_in_form(ctx: &egui::Context, e: &mut Editor, from: &str, to: &str) {
        if e.viewport.active() {
            crate::sketch_mode::tests::curve_edits::one_coordinate(e, from, to);
            return;
        }
        frame(ctx, e, vec![]);
        frame(ctx, e, vec![]);
        replace_field(ctx, e, from, to);
        frame(ctx, e, vec![]);
    }

    pub(crate) fn replace_in_form(ctx: &egui::Context, e: &mut Editor, from: &str, to: &str) {
        crate::sketch_mode::tests::curve_edits::move_coordinate(ctx, e, from, to);
    }

    /// Whether the workspace paints the sketch form's Finish action.
    pub(crate) fn offers_apply(ctx: &egui::Context, e: &mut Editor) -> bool {
        let out = frame(ctx, e, vec![]);
        out.shapes
            .iter()
            .any(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text() == "Finish"))
    }

    /// §30P: one press of `label` on a form the window holds, through the widgets.
    pub(crate) fn press(e: &mut Editor, label: &str) {
        press_with(&egui::Context::default(), e, label);
    }
    pub(crate) fn press_with(ctx: &egui::Context, e: &mut Editor, label: &str) {
        frame(ctx, e, vec![]);
        let out = frame(ctx, e, vec![]);
        click(ctx, e, text_at(&out, label));
    }

    /// §30P: the vertex draft as typed, and the depth of its Undo and Redo.
    pub(crate) fn sketch_draft(
        e: &Editor,
    ) -> Option<(Vec<ferritecad_document::SketchCurve>, usize, usize)> {
        crate::sketch_mode::tests::curve_edits::state(e)
    }

    pub(crate) fn typed_apply(
        path: &Path,
        reading: &ExtrudeEditSource,
        sketch: ferritecad_types::ObjectId,
        from: &str,
        to: &str,
    ) -> (Editor, EditRequest) {
        crate::sketch_mode::tests::curve_edits::request(path, reading, sketch, from, to)
    }

    #[test]
    fn apply_vertices_is_offered_when_it_would_work_and_keeps_the_draft() {
        use crate::sketch_mode::tests::curve_edits;
        let (_root, path, reading) = crate::fillets::tests::plate();
        let id = curve_edits::sketch_id(&reading);
        let ctx = egui::Context::default();
        let mut e = Editor::default();
        assert!(e.begin_test_drawing(&path, &reading, id));
        replace_in_form(&ctx, &mut e, "33", "41.3");
        let kept = sketch_draft(&e);
        e.viewport.set_apply(false);
        press_with(&ctx, &mut e, "Finish");
        assert!(
            e.viewport.take_request().is_none(),
            "a disabled button asked"
        );
        assert_eq!(sketch_draft(&e), kept);
        e.viewport.set_apply(true);
        press_with(&ctx, &mut e, "Finish");
        let request = e.viewport.take_request().expect("one request");
        assert_eq!(request.source, path);
        assert_eq!(request.expected, reading.version);
        assert_eq!(crate::transaction_tests::sketch(&request), id);
        assert!(
            crate::transaction_tests::vertices(&request)
                .iter()
                .any(|v| { v.start_mm == [41.3, 15.5] })
        );
        assert!(
            e.viewport.take_request().is_none(),
            "one press, one request"
        );
        assert_eq!(sketch_draft(&e), kept, "Finish discarded the draft");
        assert!(e.viewport.active());
    }

    /// The owner and its Finish button share the same availability predicate.
    #[test]
    fn the_open_sketch_form_can_apply_through_the_windows_busy_guard() {
        use crate::{Loads, creates, edits, exports, numeric, sessions};
        let (_root, path, reading) = crate::fillets::tests::plate();
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = sessions::Sessions::default();
        sessions.adopt(
            crate::sessions::test_open::open_in(
                private.path(),
                &path,
                ferritecad_jobs::HistoryLimits::default(),
            )
            .expect("session"),
        );
        let mut creates = creates::Creates::default();
        let mut loads = Loads::default();
        let exports = exports::Exports::default();
        let edits = edits::Edits::default();
        let id = crate::sketch_mode::tests::curve_edits::sketch_id(&reading);
        assert!(creates.sketch.begin_test_drawing(&path, &reading, id));
        assert!(creates.busy(), "New and Open still wait for this form");
        let ready = numeric::can_apply(&creates, &loads, &exports, &edits, &sessions);
        creates.sketch.viewport.set_apply(ready);
        let ctx = egui::Context::default();
        replace_in_form(&ctx, &mut creates.sketch, "33", "41.3");
        press_with(&ctx, &mut creates.sketch, "Finish");
        let request = creates
            .sketch
            .viewport
            .take_request()
            .expect("the open form allows Finish");
        assert_eq!(request.expected, reading.version);
        assert!(
            crate::transaction_tests::vertices(&request)
                .iter()
                .any(|v| { v.start_mm == [41.3, 15.5] })
        );
        assert!(numeric::can_apply(
            &creates, &loads, &exports, &edits, &sessions
        ));
        loads
            .open(
                Some(&path),
                std::sync::Arc::new(crate::ProgressRelay::default()),
                |_, _| std::thread::spawn(|| {}),
            )
            .expect("load");
        assert!(!numeric::can_apply(
            &creates, &loads, &exports, &edits, &sessions
        ));
        loads.stop_all();
        let loads = Loads::default();
        sessions
            .begin_apply(|_, _, _| std::thread::spawn(|| {}))
            .expect("operation");
        assert!(!numeric::can_apply(
            &creates, &loads, &exports, &edits, &sessions
        ));
        sessions.stop_all();
        creates.sketch.dismiss();
        assert!(!creates.sketch.viewport.active());
    }

    #[test]
    fn with_unsaved_changes_the_copy_workflow_is_withheld_but_apply_is_not() {
        let (root, path, reading) = angle_apply::fixture();
        let id = ferritecad_fixtures::expression_slot(
            &reading,
            ferritecad_document::ExpressionSlot::RevolveAngle,
        )
        .object;
        let ctx = egui::Context::default();
        let mut e = Editor::default();
        e.set_session(true, true);
        e.set_angle_apply(true);
        assert!(e.begin_angle_edit(&path, &reading, id));
        frame(&ctx, &mut e, vec![]);
        replace_field(&ctx, &mut e, "137.5", "41.25");
        let out = frame(&ctx, &mut e, vec![]);
        assert!(
            out.shapes.iter().any(|c| matches!(
                &c.shape,
                egui::Shape::Text(t) if t.galley.text().contains("unsaved changes")
            )),
            "the withheld copy workflow was not explained"
        );
        click(&ctx, &mut e, text_at(&out, "Save edited Revolve copy…"));
        assert!(
            e.take_angle_edit_request().is_none(),
            "the copy workflow was reachable with unsaved changes"
        );
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Apply angle"));
        assert!(
            e.take_apply_angle_request().is_some(),
            "Apply works while dirty"
        );
        drop(root);
    }

    #[test]
    fn a_new_accepted_version_ends_the_forms_about_the_old_one_and_only_those() {
        let (root, path, reading) = angle_apply::fixture();
        let id = ferritecad_fixtures::expression_slot(
            &reading,
            ferritecad_document::ExpressionSlot::RevolveAngle,
        )
        .object;
        let mut e = Editor::default();
        assert!(e.begin_angle_edit(&path, &reading, id));
        assert!(e.active());
        e.finish_session_change();
        assert!(!e.active(), "a form about the replaced version survived");
        assert!(e.take_apply_angle_request().is_none());
        let mut creates = crate::creates::Creates::default();
        let mut input = ferritecad_ui::ViewportInput::new();
        assert!(crate::ask_new(
            &mut creates,
            &crate::Loads::default(),
            &crate::exports::Exports::default(),
            &mut input,
        ));
        creates.form().expect("open New").width = "32.50".into();
        creates.sketch.finish_session_change();
        assert_eq!(creates.form().expect("New is still open").width, "32.50");
        drop(root);
    }

    pub(super) fn frame(
        ctx: &egui::Context,
        e: &mut Editor,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        frame_running(ctx, e, events, false)
    }

    #[test]
    fn a_dirty_document_offers_new_without_the_retired_creation_section() {
        use crate::sessions::tests::tabs::{Drawn, Window};
        use ferritecad_ui::{NewChoice, PointerButton, ViewportEvent};
        let (_root, path, _) = crate::fillets::tests::plate();
        let mut w = Window::new(Drawn::Mock, None);
        w.open(&path);
        w.apply(18.);
        let current = w.sessions.export_path().expect("dirty model");
        let reading = crate::sketch_facts::read(&current).expect("read");
        let id = crate::sketch_mode::tests::curve_edits::sketch_id(&reading);
        assert!(w.creates.sketch.begin_test_drawing(&current, &reading, id));
        w.creates
            .sketch
            .set_test_session(true, true, w.sessions.dirty());
        let out = frame(&egui::Context::default(), &mut w.creates.sketch, vec![]);
        assert!(!painted(&out, "Create sketch + Extrude…"));
        w.input
            .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        assert!(!w.begin_new(), "New must wait for the gesture");
        w.input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
        assert!(w.begin_new());
        assert!(w.creates.form().is_some());
        assert!(!w.creates.running(), "opening New does not start creation");
        assert!(w.answer_new(NewChoice::Cancel).is_none());
        assert!(w.sessions.dirty() && w.creates.sketch.unsaved);
        assert!(w.creates.sketch.viewport.take_request().is_none());
    }
    pub(super) fn frame_running(
        ctx: &egui::Context,
        e: &mut Editor,
        events: Vec<egui::Event>,
        running: bool,
    ) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                if e.viewport.active() {
                    e.viewport.draw(ui, running, "");
                } else {
                    e.draw(ui, running, "");
                }
            },
        );
        output.textures_delta.clear();
        output
    }
    pub(super) fn text_at(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    assert!(
                        c.clip_rect
                            .expand(1.)
                            .contains_rect(t.visual_bounding_rect()),
                        "clipped {label}"
                    );
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("button {label} was not painted"))
    }
    pub(super) fn click(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
        frame(ctx, e, vec![egui::Event::PointerMoved(at)]);
        for pressed in [true, false] {
            frame(
                ctx,
                e,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
            );
        }
    }
    pub(super) fn replace_field(ctx: &egui::Context, e: &mut Editor, label: &str, value: &str) {
        let out = frame(ctx, e, vec![]);
        click(ctx, e, text_at(&out, label));
        frame(
            ctx,
            e,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Text(value.into()),
            ],
        );
    }
    /// Clicks into the box on `label`'s row and replaces what it holds.
    fn replace_box_on_row(
        ctx: &egui::Context,
        e: &mut Editor,
        out: &egui::FullOutput,
        column: f32,
        label: &str,
        value: &str,
    ) {
        let row = text_at(out, label).y;
        click(ctx, e, egui::pos2(column + 40., row));
        frame(
            ctx,
            e,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Text(value.into()),
            ],
        );
    }

    /// A real source document with one saved analytic circle, and the accepted
    /// reading of it that a form is allowed to take its facts from.
    fn saved_circle(root: &Path) -> (PathBuf, crate::sketch_facts::Source) {
        use crate::creates::tests::ferritecad;
        let source = root.join("original.fcad");
        let input = root.join("create.json");
        std::fs::write(
            &input,
            r#"{"schema_version":1,"center_mm":[12.0,-7.0],"radius_mm":10.0,"height_mm":15.0}"#,
        )
        .expect("input");
        let p = std::process::Command::new(ferritecad())
            .arg("create-circle-extrude")
            .arg(&input)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(p.status.success(), "{p:?}");
        let loaded = {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                &source,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &ferritecad_kernel::OperationContext::default(),
            )
            .expect("accepted scene")
        };
        (
            source,
            crate::sketch_facts::Source::from(loaded.edit_source.expect("accepted edit facts")),
        )
    }

    /// §27B through the real widgets: a saved Revolve profile opens in the
    /// ordinary coordinate editor with its axis and no height, a radius on the
    /// axis is refused and undone, and the shared worker and the peer CLI
    /// publish the same copy with the same identities.
    #[test]
    fn native_revolve_profile_edit_widgets_worker_and_cli_publish_one_copy() {
        use crate::creates::tests::{ferritecad, read_semantics};
        use ferritecad_document::Document;
        use ferritecad_kernel::OperationContext;
        if !ferritecad_occt::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("bushing.fcad");
        let input = root.path().join("create.json");
        std::fs::write(
            &input,
            concat!(
                r#"{"request_version":1,"points_mm":[[4.5,0.25],[10.5,0.25],[10.5,15.25],"#,
                r#"[4.5,15.25]],"axis":"sketch_y","angle":"full_turn"}"#
            ),
        )
        .expect("request");
        let p = std::process::Command::new(ferritecad())
            .arg("create-sketch-revolve")
            .arg(&input)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(p.status.success(), "{p:?}");
        let bytes = std::fs::read(&source).expect("source");
        let loaded = {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                &source,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("accepted scene")
        };
        let reading =
            crate::sketch_facts::Source::from(loaded.edit_source.expect("accepted edit facts"));
        let literal = [[3.5, 2.5], [8.5, 2.5], [8.5, 12.5], [3.5, 12.5]];
        let (mut window, request, literal_changes) =
            crate::sketch_mode::tests::curve_edits::polygon(&source, &literal);
        assert_eq!(request.expected, reading.version);
        // Crossing the axis still refuses in the evaluator; the draft is a real gesture result.
        let (refused_window, refused, _) = crate::sketch_mode::tests::curve_edits::polygon(
            &source,
            &[[-0.5, 2.5], [8.5, 2.5], [8.5, 12.5], [3.5, 12.5]],
        );
        crate::transaction_tests::assert_refused(&refused, &["Revolve"]);
        drop(refused_window);
        assert!(crate::sketch_mode::tests::window::apply(
            &mut window,
            request
        ));
        let destination = root.path().join("ui.fcad");
        assert!(
            window
                .save(ferritecad_jobs::SaveTarget::As(destination))
                .published
        );

        let edit = root.path().join("edit.json");

        std::fs::write(
            &edit,
            ferritecad_fixtures::edit_request_json(&literal_changes).to_string(),
        )
        .expect("edit request");
        let cli = root.path().join("cli.fcad");
        let p = std::process::Command::new(ferritecad())
            .arg("edit")
            .arg(&source)
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&edit)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(p.status.success(), "{p:?}");
        let ui = root.path().join("ui.fcad");
        assert_eq!(read_semantics(&ui), read_semantics(&cli));
        let a = Document::open_read_only(&ui).expect("UI");
        let b = Document::open_read_only(&cli).expect("CLI");
        crate::transaction_tests::assert_meta(a.meta(), b.meta());
        assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
        assert_eq!(crate::test_names::names(&a), crate::test_names::names(&b));
        let original = Document::open_read_only(&source).expect("source");
        assert_eq!(
            crate::test_names::names(&a),
            crate::test_names::names(&original),
            "no new names"
        );
        original.close().expect("close");
        // The kernel's own volume: π·(8.5² − 3.5²)·10.
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let context = OperationContext::default();
        let built = ferritecad_eval::rebuild_cold(&a, &mut kernel, &context).expect("cold");
        let body = a
            .objects()
            .expect("objects")
            .iter()
            .find(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Body(_)))
            .map(|o| o.id)
            .expect("Body");
        let body = built.shape(body).expect("one solid");
        let (_, volume) = kernel.shape_stats(body).expect("stats");
        let expected = std::f64::consts::PI * (8.5f64 * 8.5 - 3.5 * 3.5) * 10.;
        assert!((volume - expected).abs() < 1e-6 * expected, "{volume}");
        built.release_all(&mut kernel);
        a.close().expect("close");
        b.close().expect("close");
        let mut exported = Vec::new();
        for path in [&ui, &cli] {
            for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
                let out = path.with_extension(extension);
                let p = std::process::Command::new(ferritecad())
                    .arg(op)
                    .arg(path)
                    .arg("-o")
                    .arg(&out)
                    .output()
                    .expect("export");
                assert!(p.status.success(), "{p:?}");
                exported.push(std::fs::read(&out).expect("bytes"));
            }
        }
        assert_eq!(exported[0], exported[2], "STL bytes");
        assert_eq!(
            exported[1], exported[3],
            "FBX bytes: same stored identities"
        );
        assert_eq!(std::fs::read(&source).expect("source"), bytes);
    }

    /// Scrolls the saved-object actions until `label` is fully visible, then
    /// clicks it through real pointer input.
    pub(super) fn click_saved_action(
        ctx: &egui::Context,
        e: &mut Editor,
        path: &Path,
        source: &ExtrudeEditSource,
        label: &str,
    ) {
        let mut at = None;
        for step in 0..40 {
            let events = if step == 0 {
                Vec::new()
            } else {
                vec![
                    egui::Event::PointerMoved(egui::pos2(400., 200.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        phase: egui::TouchPhase::Move,
                        delta: egui::vec2(0., -60.),
                        modifiers: Default::default(),
                    },
                ]
            };
            let mut out = document_frame(ctx, e, path, source, events);
            for _ in 0..10 {
                out = document_frame(ctx, e, path, source, vec![]);
            }
            at = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if t.galley.text() == label
                        && s.clip_rect.contains_rect(t.visual_bounding_rect()) =>
                {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            });
            if at.is_some() {
                break;
            }
        }
        let at = at.unwrap_or_else(|| panic!("{label} must be reachable"));
        document_frame(ctx, e, path, source, vec![egui::Event::PointerMoved(at)]);
        for pressed in [true, false] {
            document_frame(
                ctx,
                e,
                path,
                source,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
            );
        }
    }

    /// §27E through the real widgets: a saved sector's angle opens from its
    /// own action while its Sketch stays refused, a full turn is refused in
    /// the form, Apply/Undo/Redo move through the bounded history, and the
    /// shared worker and the peer CLI publish the same copy.
    #[test]
    fn native_revolve_angle_edit_widgets_worker_and_cli_publish_one_copy() {
        use crate::creates::tests::{ferritecad, read_semantics};
        use ferritecad_document::{Document, ObjectPayload, RevolveExtent};
        use ferritecad_kernel::OperationContext;
        if !ferritecad_occt::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("sector.fcad");
        let input = root.path().join("create.json");
        std::fs::write(
            &input,
            concat!(
                r#"{"request_version":2,"points_mm":[[4.25,1.5],[10.75,1.5],[10.75,6.5],"#,
                r#"[7.5,6.5],[7.5,16.25],[4.25,16.25]],"axis":"sketch_y","#,
                r#""extent":{"kind":"angle","degrees":137.5}}"#
            ),
        )
        .expect("request");
        let p = std::process::Command::new(ferritecad())
            .arg("create-sketch-revolve")
            .arg(&input)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(p.status.success(), "{p:?}");
        let bytes = std::fs::read(&source).expect("source");
        let loaded = {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                &source,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("accepted scene")
        };
        let reading =
            crate::sketch_facts::Source::from(loaded.edit_source.expect("accepted edit facts"));
        let angle = reading
            .expression_slots
            .iter()
            .find(|s| s.slot == ferritecad_document::ExpressionSlot::RevolveAngle)
            .expect("angle slot");
        assert_eq!(angle.refusal, None);
        assert_eq!(
            ferritecad_types::display_degrees(angle.expression.value()),
            137.5
        );
        let feature = angle.object;
        // §27F: the sector's coordinates are a separate, available edit.
        let sketch = &reading.constraint_sketches[0];
        assert!(crate::sketch_mode::tests::curve_edits::begin(
            &mut Editor::default(),
            &source,
            &reading,
            sketch.sketch,
        ));
        assert!(matches!(
            sketch.profile_use,
            Some(SketchProfileUse::PartialRevolve { radians: angle, .. }) if angle.radians().to_degrees() == 137.5
        ));

        let mut e = Editor::default();
        let ctx = egui::Context::default();
        click_saved_action(
            &ctx,
            &mut e,
            &source,
            &reading,
            &format!("Edit Revolve angle Revolve1 — {feature}…"),
        );
        assert!(
            e.angle.editing_angle.is_some(),
            "the row opened the angle form"
        );
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        let out = frame(&ctx, &mut e, vec![]);
        let texts: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        for wanted in [
            "Edit saved Revolve angle",
            "Saved partial Revolve · angle only · right-handed about the sketch +Y axis",
            "Saved angle 137.5°",
            "137.5",
            "° (0.01–359.99, a full turn is not an angle)",
        ] {
            assert!(texts.iter().any(|t| t == wanted), "{wanted} in {texts:?}");
        }
        assert!(e.angle.angle_undo.is_empty() && e.angle.angle_redo.is_empty());

        // A full turn is refused by the one angle policy, in the form.
        replace_field(&ctx, &mut e, "137.5", "360");
        let refusal = e.angle_edit_request().expect_err("full turn").to_string();
        assert!(refusal.contains("state a full turn"), "{refusal}");
        let out = frame(&ctx, &mut e, vec![]);
        assert!(out.shapes.iter().any(|c| matches!(&c.shape,
            egui::Shape::Text(t) if t.galley.text().contains("state a full turn"))));
        assert!(
            !out.shapes.iter().any(|c| matches!(&c.shape,
            egui::Shape::Text(t) if t.galley.text() == "Save edited Revolve copy…")),
            "no Save for a refused angle"
        );
        for bad in ["0", "-90", "359.995", "400", "NaN", "inf", "ninety"] {
            e.angle.angle_edit = bad.into();
            assert!(e.angle_edit_request().is_err(), "{bad} accepted");
        }
        e.angle.angle_edit = "360".into();

        // Through 180°, applied, undone and redone through the real buttons.
        replace_field(&ctx, &mut e, "360", "220");
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Confirm draft numbers"));
        assert_eq!(e.angle.angle_applied.as_deref(), Some("220"));
        assert_eq!(e.angle.angle_undo, ["137.5"]);
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Undo draft"));
        assert_eq!(e.angle.angle_edit, "137.5");
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Redo draft"));
        assert_eq!(e.angle.angle_edit, "220");
        // Applying the same number again adds no checkpoint.
        e.apply_angle().expect("same angle");
        assert_eq!(e.angle.angle_undo, ["137.5"]);
        assert!(e.angle.angle_redo.is_empty());

        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Save edited Revolve copy…"));
        let request = e.take_angle_edit_request().expect("submit");
        assert_eq!(request.changes[0].object(), feature);
        assert_eq!(
            request.changes[0]
                .expression()
                .expect("numeric form contains an expression change")
                .value(),
            220.0_f64.to_radians()
        );
        assert_eq!(request.expected, reading.version);
        let kept = (e.angle.angle_edit.clone(), e.angle.angle_undo.clone());

        let mut edits = crate::edits::Edits::default();
        for occupied in [true, false] {
            let mut request = request.clone();
            request.destination =
                root.path()
                    .join(if occupied { "occupied.fcad" } else { "ui.fcad" });
            if occupied {
                std::fs::write(&request.destination, b"keep").expect("sentinel");
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let generation = edits
                .start(request.clone(), move |r, g, c| {
                    crate::edits::spawn_edit(
                        r.source,
                        r.expected,
                        r.changes,
                        r.destination,
                        c,
                        move |result| tx.send((g, result)).expect("reply"),
                    )
                })
                .expect("worker");
            assert!(
                edits
                    .start(request, |_, _, _| panic!("duplicate worker"))
                    .is_none(),
                "a running job takes no second request"
            );
            assert!(
                finish_angle_edit(
                    &mut e,
                    &mut edits,
                    generation + 1,
                    Err(CadError::input("stale response"))
                )
                .is_none(),
                "a reply for another request changes nothing"
            );
            assert_eq!(
                (e.angle.angle_edit.clone(), e.angle.angle_undo.clone()),
                kept
            );
            let (g, result) = rx.recv().expect("completed");
            let path = finish_angle_edit(&mut e, &mut edits, g, result);
            if occupied {
                assert!(path.is_none(), "a taken destination publishes nothing");
                assert!(e.angle.editing_angle.is_some(), "and keeps the draft");
                assert_eq!(
                    std::fs::read(root.path().join("occupied.fcad")).expect("sentinel"),
                    b"keep"
                );
            } else {
                let path = path.expect("published");
                assert!(!e.active(), "published draft must not block async Open");
                e.draft_load_finished(Path::new("another.fcad"), false);
                assert!(!e.active(), "an unrelated load failure cannot restore it");
                e.draft_load_finished(&path, false);
                assert!(e.active(), "a failed Open restores the edited draft");
                assert_eq!(
                    (e.angle.angle_edit.clone(), e.angle.angle_undo.clone()),
                    kept
                );
                assert_eq!(e.angle.angle_undo.len(), 1, "history survives failed Open");
                e.draft_published(&path);
                e.draft_load_finished(&path, true);
                assert!(!e.active());
                assert!(e.published_draft.is_none());
            }
        }

        // The peer CLI, the same request, the same source.
        let edit = root.path().join("angle.json");
        std::fs::write(&edit, format!(r#"{{"request_version":1,"changes":[{{"kind":"set_expression","object":"{feature}","slot":{{"kind":"revolve_angle"}},"value":220,"unit":"deg"}}]}}"#)).expect("request");
        let cli = root.path().join("cli.fcad");
        let p = std::process::Command::new(ferritecad())
            .arg("edit")
            .arg(&source)
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&edit)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(p.status.success(), "{p:?}");
        let ui = root.path().join("ui.fcad");
        assert_eq!(read_semantics(&ui), read_semantics(&cli));
        let a = Document::open_read_only(&ui).expect("UI");
        let b = Document::open_read_only(&cli).expect("CLI");
        assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
        assert_eq!(crate::test_names::names(&a), crate::test_names::names(&b));
        let original = Document::open_read_only(&source).expect("source");
        assert_eq!(
            crate::test_names::names(&a),
            crate::test_names::names(&original),
            "no new names"
        );
        original.close().expect("close");
        let revolve = a
            .objects()
            .expect("objects")
            .into_iter()
            .find_map(|o| match o.payload {
                ObjectPayload::Revolve(r) => Some(r),
                _ => None,
            })
            .expect("Revolve");
        assert!(matches!(revolve.extent,
            RevolveExtent::Partial { angle } if angle.value().to_degrees() == 220.0));
        a.close().expect("close");
        b.close().expect("close");
        let mut exported = Vec::new();
        for path in [&ui, &cli] {
            for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
                let out = path.with_extension(extension);
                let p = std::process::Command::new(ferritecad())
                    .arg(op)
                    .arg(path)
                    .arg("-o")
                    .arg(&out)
                    .output()
                    .expect("export");
                assert!(p.status.success(), "{p:?}");
                exported.push(std::fs::read(&out).expect("bytes"));
            }
        }
        assert_eq!(exported[0], exported[2], "STL bytes");
        assert_eq!(
            exported[1], exported[3],
            "FBX bytes: same stored identities"
        );
        assert_eq!(std::fs::read(&source).expect("source"), bytes);
    }

    #[test]
    fn native_circle_edit_worker_and_cli_publish_equivalent_copies() {
        use crate::creates::tests::{ferritecad, read_semantics};
        use ferritecad_document::Document;
        if !ferritecad_occt::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let (source, reading) = saved_circle(root.path());
        let bytes = std::fs::read(&source).expect("source");
        let modified = std::fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("mtime");
        let id = crate::sketch_mode::tests::curve_edits::sketch_id(&reading);

        let (mut window, request) = crate::sketch_mode::tests::round_edits::edit(
            &source,
            id,
            [-4., 4.],
            &[6.75],
            false,
            || {},
        );
        assert!(crate::sketch_mode::tests::window::apply(
            &mut window,
            request.clone()
        ));
        assert!(
            window
                .save(ferritecad_jobs::SaveTarget::As(root.path().join("ui.fcad")))
                .published
        );

        // The peer CLI applies the same request to the same source.
        let input = root.path().join("edit.json");
        std::fs::write(
            &input,
            ferritecad_fixtures::edit_request_json(&request.changes).to_string(),
        )
        .expect("request");
        let cli = root.path().join("cli.fcad");
        let p = std::process::Command::new(ferritecad())
            .arg("edit")
            .arg(&source)
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(p.status.success(), "{p:?}");

        // One source, one request: the two copies are the same document, with
        // the same identities. Only the instant each was written may differ.
        let ui = root.path().join("ui.fcad");
        assert_eq!(read_semantics(&ui), read_semantics(&cli));
        let a = Document::open_read_only(&ui).expect("UI");
        let b = Document::open_read_only(&cli).expect("CLI");
        assert_eq!(a.meta().document_id, b.meta().document_id);
        assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
        assert_eq!(
            a.dependencies().expect("deps"),
            b.dependencies().expect("deps")
        );
        assert_eq!(crate::test_names::names(&a), crate::test_names::names(&b));
        a.close().expect("close");
        b.close().expect("close");

        let mut meshes = Vec::new();
        let mut fbxs = Vec::new();
        for (path, name) in [(&ui, "circle-edit-ui"), (&cli, "circle-edit-cli")] {
            for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
                let out = path.with_extension(extension);
                let p = std::process::Command::new(ferritecad())
                    .arg(op)
                    .arg(path)
                    .arg("-o")
                    .arg(&out)
                    .output()
                    .expect("export");
                assert!(p.status.success(), "{p:?}");
                if extension == "stl" {
                    meshes.push(std::fs::read(&out).expect("STL"));
                } else {
                    fbxs.push(std::fs::read(&out).expect("FBX"));
                    if let Some(dir) = std::env::var_os("FCAD_CIRCLE_ARTIFACTS") {
                        // Read by the pinned ufbx reader in the same CI job.
                        let dir = std::path::Path::new(&dir);
                        std::fs::create_dir_all(dir).expect("artifact directory");
                        std::fs::copy(&out, dir.join(format!("{name}.fbx"))).expect("artifact");
                    }
                }
            }
        }
        assert_eq!(meshes[0], meshes[1], "one geometry, two copies");
        assert_eq!(fbxs[0], fbxs[1], "same stored identities, same FBX");
        assert_eq!(std::fs::read(&source).expect("source"), bytes);
        assert_eq!(
            std::fs::metadata(&source)
                .expect("metadata")
                .modified()
                .expect("mtime"),
            modified
        );
    }

    /// A real §25L source and the accepted reading a form may read facts from.
    fn saved_annulus(root: &Path) -> (PathBuf, crate::sketch_facts::Source) {
        use crate::creates::tests::ferritecad;
        let source = root.join("original.fcad");
        let input = root.join("create.json");
        std::fs::write(
            &input,
            concat!(
                r#"{"schema_version":1,"center_mm":[12.0,-7.0],"#,
                r#""outer_radius_mm":10.0,"inner_radius_mm":4.0,"height_mm":15.0}"#
            ),
        )
        .expect("input");
        let p = std::process::Command::new(ferritecad())
            .arg("create-annular-extrude")
            .arg(&input)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(p.status.success(), "{p:?}");
        let loaded = {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                &source,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &ferritecad_kernel::OperationContext::default(),
            )
            .expect("accepted scene")
        };
        (
            source,
            crate::sketch_facts::Source::from(loaded.edit_source.expect("accepted edit facts")),
        )
    }

    #[test]
    fn native_annulus_edit_worker_and_cli_publish_equivalent_copies() {
        use crate::creates::tests::{ferritecad, read_semantics};
        use ferritecad_document::Document;
        if !ferritecad_occt::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let (source, reading) = saved_annulus(root.path());
        let bytes = std::fs::read(&source).expect("source");
        let modified = std::fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("mtime");
        let id = crate::sketch_mode::tests::curve_edits::sketch_id(&reading);

        let (mut window, request) = crate::sketch_mode::tests::round_edits::edit(
            &source,
            id,
            [-4., 4.],
            &[6.75, 2.125],
            false,
            || {},
        );
        assert!(crate::sketch_mode::tests::window::apply(
            &mut window,
            request.clone()
        ));
        assert!(
            window
                .save(ferritecad_jobs::SaveTarget::As(root.path().join("ui.fcad")))
                .published
        );

        // The peer CLI applies the same request to the same source.
        let input = root.path().join("edit.json");
        std::fs::write(
            &input,
            ferritecad_fixtures::edit_request_json(&request.changes).to_string(),
        )
        .expect("request");
        let cli = root.path().join("cli.fcad");
        let p = std::process::Command::new(ferritecad())
            .arg("edit")
            .arg(&source)
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(p.status.success(), "{p:?}");

        // One source, one request: the two copies are the same document, with
        // the same identities. Only the instant each was written may differ.
        let ui = root.path().join("ui.fcad");
        assert_eq!(read_semantics(&ui), read_semantics(&cli));
        let a = Document::open_read_only(&ui).expect("UI");
        let b = Document::open_read_only(&cli).expect("CLI");
        assert_eq!(a.meta().document_id, b.meta().document_id);
        assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
        assert_eq!(
            a.dependencies().expect("deps"),
            b.dependencies().expect("deps")
        );
        assert_eq!(crate::test_names::names(&a), crate::test_names::names(&b));
        a.close().expect("close");
        b.close().expect("close");

        let mut meshes = Vec::new();
        let mut fbxs = Vec::new();
        for (path, name) in [(&ui, "annulus-edit-ui"), (&cli, "annulus-edit-cli")] {
            for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
                let out = path.with_extension(extension);
                let p = std::process::Command::new(ferritecad())
                    .arg(op)
                    .arg(path)
                    .arg("-o")
                    .arg(&out)
                    .output()
                    .expect("export");
                assert!(p.status.success(), "{p:?}");
                if extension == "stl" {
                    meshes.push(std::fs::read(&out).expect("STL"));
                } else {
                    fbxs.push(std::fs::read(&out).expect("FBX"));
                    if let Some(dir) = std::env::var_os("FCAD_ANNULUS_EDIT_ARTIFACTS") {
                        // Read by the pinned ufbx reader in the same CI job.
                        let dir = std::path::Path::new(&dir);
                        std::fs::create_dir_all(dir).expect("artifact directory");
                        std::fs::copy(&out, dir.join(format!("{name}.fbx"))).expect("artifact");
                    }
                }
            }
        }
        assert_eq!(meshes[0], meshes[1], "one geometry, two copies");
        assert_eq!(fbxs[0], fbxs[1], "same stored identities, same FBX");
        assert_eq!(std::fs::read(&source).expect("source"), bytes);
        assert_eq!(
            std::fs::metadata(&source)
                .expect("metadata")
                .modified()
                .expect("mtime"),
            modified
        );
    }

    fn document_frame(
        ctx: &egui::Context,
        e: &mut Editor,
        path: &Path,
        source: &ExtrudeEditSource,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(988., 768.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ferritecad_ui::toolbar(
                    ui,
                    ferritecad_ui::Activity {
                        can_open: true,
                        can_export: true,
                        ..Default::default()
                    },
                );
                e.draw_choices(
                    ui,
                    true,
                    Some(path),
                    Some(source),
                    &accepted::Accepted::default(),
                );
                e.draw(ui, false, "");
                crate::edits::Edits::default().draw(ui, true, source.unavailable_reason());
            },
        );
        out.textures_delta.clear();
        out
    }
    #[test]
    fn native_saved_sketch_draft_and_cli_preserve_same_model() {
        saved_sketch_and_cli(false);
    }
    pub(super) fn saved_sketch_and_cli(drag: bool) {
        saved_sketch_and_cli_history(drag, 0);
    }
    #[test]
    fn native_cut_base_widgets_worker_cli_preserve_draft() {
        saved_sketch_and_cli_history(false, 4);
        saved_sketch_and_cli_history(false, 16);
    }
    fn saved_sketch_and_cli_history(drag: bool, cuts: usize) {
        use crate::creates::tests::{ferritecad, read_semantics};
        use ferritecad_document::Document;
        use ferritecad_kernel::OperationContext;
        if !ferritecad_occt::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("original.fcad");
        let input = root.path().join("L.json");
        let json = if cuts == 0 {
            r#"{"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10}"#
        } else {
            r#"{"request_version":1,"points_mm":[[0,0],[60,0],[60,40],[0,40]],"height_mm":10}"#
        };
        std::fs::write(&input, json).expect("input");
        let p = std::process::Command::new(ferritecad())
            .arg("create-sketch-extrude")
            .arg(&input)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(p.status.success(), "{p:?}");
        if cuts > 0 {
            let mut d = Document::open(&source).expect("doc");
            let body = crate::sketch_facts::Source::read(&d)
                .expect("catalog")
                .cut_bodies[0]
                .body;
            for i in 0..cuts {
                let slot = (i * 7) % 16;
                let tool = crate::feature_facts::CircularCut {
                    center_mm: [
                        if i + 1 == cuts {
                            55.
                        } else {
                            6. + (slot % 4) as f64 * 14.
                        },
                        5. + (slot / 4) as f64 * 9.,
                    ],
                    radius_mm: 1. + (i % 3) as f64 * 0.1,
                    extent: crate::feature_facts::CutExtent::Blind {
                        depth_mm: if i % 2 == 0 { 10. } else { 3. + (i % 4) as f64 },
                    },
                };
                let p = ferritecad_fixtures::feature_changes::circular_tool(
                    &d,
                    body,
                    tool.center_mm,
                    tool.radius_mm,
                    tool.extent.end().expect("end"),
                )
                .expect("tool")
                .changes;
                d.write_changes(&p).expect("cut");
            }
            d.close().expect("close");
        }
        if drag {
            super::drag_tests::attach_source_claim(&source);
        }
        let bytes = std::fs::read(&source).expect("source");
        let modified = std::fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("mtime");
        let loaded = {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                &source,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("accepted scene")
        };
        let reading =
            crate::sketch_facts::Source::from(loaded.edit_source.expect("accepted edit facts"));
        if cuts > 0 {
            let (probe, request, literal_changes) = crate::sketch_mode::tests::curve_edits::polygon(
                &source,
                &[[0., 0.], [55.5, 0.], [55.5, 40.], [0., 40.]],
            );
            assert_eq!(
                request.changes, literal_changes,
                "the rebuild judges the Cut"
            );
            assert_eq!(std::fs::read(&source).expect("source"), bytes);
            drop(probe);
        }
        let literal: &[[f64; 2]] = if cuts == 0 {
            &[
                [0., 0.],
                [80., 0.],
                [80., 20.],
                [20., 20.],
                [20., 40.],
                [0., 40.],
            ]
        } else {
            &[[0., 0.], [80., 0.], [80., 40.], [0., 40.]]
        };
        let (mut window, request, literal_changes) =
            crate::sketch_mode::tests::curve_edits::polygon(&source, literal);
        assert_eq!(request.expected, reading.version);
        assert!(crate::sketch_mode::tests::window::apply(
            &mut window,
            request
        ));
        let destination = root.path().join("ui.fcad");
        assert!(
            window
                .save(ferritecad_jobs::SaveTarget::As(destination))
                .published
        );
        // IDs and finite numeric coordinates only; process JSON assertions live
        // in the CLI suite, avoiding an application dependency just for a test.

        std::fs::write(
            &input,
            ferritecad_fixtures::edit_request_json(&literal_changes).to_string(),
        )
        .expect("request");
        let cli = root.path().join("cli.fcad");
        let p = std::process::Command::new(ferritecad())
            .arg("edit")
            .arg(&source)
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(p.status.success(), "{p:?}");
        let ui = root.path().join("ui.fcad");
        if cuts == 0 {
            assert_eq!(
                read_semantics(&ui),
                read_semantics(&cli),
                "same source: all identities and semantic relationships identical"
            );
        } // Cut names may repeat; the complete UUID-keyed objects/refs follow.
        let a = Document::open_read_only(&ui).expect("UI");
        let b = Document::open_read_only(&cli).expect("CLI");
        crate::transaction_tests::assert_meta(a.meta(), b.meta());
        assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
        assert_eq!(
            a.dependencies().expect("deps"),
            b.dependencies().expect("deps")
        );
        assert_eq!(crate::test_names::names(&a), crate::test_names::names(&b));
        if drag {
            super::drag_tests::verify_publications(&source, &ui, &cli);
        }
        if cuts > 0 {
            for format in ["stl", "fbx"] {
                let mut exports = Vec::new();
                for model in [&ui, &cli] {
                    let output = model.with_extension(format);
                    let p = std::process::Command::new(ferritecad())
                        .arg(format!("export-{format}"))
                        .arg(model)
                        .arg("-o")
                        .arg(&output)
                        .arg("--json")
                        .output()
                        .expect("export");
                    assert!(p.status.success(), "{p:?}");
                    exports.push(std::fs::read(output).expect("bytes"));
                }
                assert_eq!(exports[0], exports[1], "worker and fresh CLI {format}");
            }
        }
        assert_eq!(std::fs::read(&source).expect("source"), bytes);
        assert_eq!(
            std::fs::metadata(&source)
                .expect("metadata")
                .modified()
                .expect("mtime"),
            modified
        );
    }

    fn painted(out: &egui::FullOutput, text: &str) -> bool {
        out.shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains(text)))
    }

    /// §28J, native: the widgets' request through the app's worker and the
    /// same edit through the shipped `edit` publish one document
    /// under two Fillets: every SQL cell equal with no identifier mapped, and
    /// byte-identical STL and FBX. Copy failure recovery is covered by the retained angle form.
    #[test]
    fn native_two_fillet_base_sketch_worker_and_cli_publish_the_same_part() {
        use crate::creates::tests::ferritecad;
        if !ferritecad_occt::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        let (root, path, reading) = crate::fillets::tests::rounded_twice(2.375, 3.0625);
        let before = std::fs::read(&path).expect("source");
        let (mut window, request, literal_changes) =
            crate::sketch_mode::tests::curve_edits::plate(&path);
        assert!(crate::sketch_mode::tests::window::apply(
            &mut window,
            request
        ));
        let ui = root.path().join("worker.fcad");
        assert!(
            window
                .save(ferritecad_jobs::SaveTarget::As(ui.clone()))
                .published
        );

        let input = root.path().join("request.json");

        std::fs::write(
            &input,
            ferritecad_fixtures::edit_request_json(&literal_changes).to_string(),
        )
        .expect("request");
        let peer = root.path().join("peer.fcad");
        let out = std::process::Command::new(ferritecad())
            .arg("edit")
            .arg(&path)
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&peer)
            .arg("--json")
            .output()
            .expect("peer");
        assert!(out.status.success(), "{out:?}");
        crate::fillets::tests::same_radius_publication(&path, &ui, &peer);
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }

    /// §28D, native: the widgets' request through the app's worker and the
    /// same edit through the shipped `edit` publish one document:
    /// every SQL cell equal with no identifier mapped (nothing is minted), and
    /// byte-identical STL and FBX. Copy failure recovery is covered by the retained angle form.
    #[test]
    fn native_fillet_base_sketch_widgets_worker_and_cli_publish_the_same_part() {
        use crate::creates::tests::ferritecad;
        if !ferritecad_occt::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        let (root, path, reading) = crate::fillets::tests::rounded(2.375);
        let before = std::fs::read(&path).expect("source");
        let (mut window, request, literal_changes) =
            crate::sketch_mode::tests::curve_edits::plate(&path);
        assert!(crate::sketch_mode::tests::window::apply(
            &mut window,
            request
        ));
        let ui = root.path().join("worker.fcad");
        assert!(
            window
                .save(ferritecad_jobs::SaveTarget::As(ui.clone()))
                .published
        );

        let input = root.path().join("request.json");

        std::fs::write(
            &input,
            ferritecad_fixtures::edit_request_json(&literal_changes).to_string(),
        )
        .expect("request");
        let peer = root.path().join("peer.fcad");
        let out = std::process::Command::new(ferritecad())
            .arg("edit")
            .arg(&path)
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&peer)
            .arg("--json")
            .output()
            .expect("peer");
        assert!(out.status.success(), "{out:?}");
        let (left, right) = (
            crate::fillets::tests::tables(&ui),
            crate::fillets::tests::tables(&peer),
        );
        assert_eq!(
            left.keys().collect::<Vec<_>>(),
            right.keys().collect::<Vec<_>>()
        );
        for (table, (columns, rows)) in &left {
            let (theirs_columns, theirs) = &right[table];
            assert_eq!(columns, theirs_columns);
            assert_eq!(rows.len(), theirs.len(), "{table} rows");
            for (l, r) in rows.iter().zip(theirs) {
                for (i, column) in columns.iter().enumerate() {
                    if table == "meta" && column == "modified_at" {
                        continue;
                    }
                    assert_eq!(l[i], r[i], "{table}.{column} differs");
                }
            }
        }
        for format in ["stl", "fbx"] {
            let mut exports = Vec::new();
            for model in [&ui, &peer] {
                let output = model.with_extension(format);
                let p = std::process::Command::new(ferritecad())
                    .arg(format!("export-{format}"))
                    .arg(model)
                    .arg("-o")
                    .arg(&output)
                    .arg("--json")
                    .output()
                    .expect("export");
                assert!(p.status.success(), "{p:?}");
                exports.push(std::fs::read(output).expect("bytes"));
            }
            assert_eq!(exports[0], exports[1], "worker and CLI {format}");
        }
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }

    /// §29C, native: the widgets' request through the app's worker and the same
    /// edit through the shipped `edit` publish one document: every
    /// SQL cell equal with no identifier mapped, and byte-identical STL and FBX.
    /// Copy failure recovery is covered by the retained angle form.
    #[test]
    fn native_chamfer_base_sketch_widgets_worker_and_cli_publish_the_same_part() {
        use crate::creates::tests::ferritecad;
        if !ferritecad_occt::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        let (root, path, reading) = crate::chamfers::tests::chamfered(2.375);
        let before = std::fs::read(&path).expect("source");
        let (mut window, request, literal_changes) =
            crate::sketch_mode::tests::curve_edits::plate(&path);
        assert!(crate::sketch_mode::tests::window::apply(
            &mut window,
            request
        ));
        let ui = root.path().join("worker.fcad");
        assert!(
            window
                .save(ferritecad_jobs::SaveTarget::As(ui.clone()))
                .published
        );

        let input = root.path().join("request.json");

        std::fs::write(
            &input,
            ferritecad_fixtures::edit_request_json(&literal_changes).to_string(),
        )
        .expect("request");
        let peer = root.path().join("peer.fcad");
        let out = std::process::Command::new(ferritecad())
            .arg("edit")
            .arg(&path)
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&peer)
            .arg("--json")
            .output()
            .expect("peer");
        assert!(out.status.success(), "{out:?}");
        let (left, right) = (
            crate::fillets::tests::tables(&ui),
            crate::fillets::tests::tables(&peer),
        );
        assert_eq!(
            left.keys().collect::<Vec<_>>(),
            right.keys().collect::<Vec<_>>()
        );
        for (table, (columns, rows)) in &left {
            let (theirs_columns, theirs) = &right[table];
            assert_eq!(columns, theirs_columns);
            assert_eq!(rows.len(), theirs.len(), "{table} rows");
            for (l, r) in rows.iter().zip(theirs) {
                for (i, column) in columns.iter().enumerate() {
                    if table == "meta" && column == "modified_at" {
                        continue;
                    }
                    assert_eq!(l[i], r[i], "{table}.{column} differs");
                }
            }
        }
        for format in ["stl", "fbx"] {
            let mut exports = Vec::new();
            for model in [&ui, &peer] {
                let output = model.with_extension(format);
                let p = std::process::Command::new(ferritecad())
                    .arg(format!("export-{format}"))
                    .arg(model)
                    .arg("-o")
                    .arg(&output)
                    .arg("--json")
                    .output()
                    .expect("export");
                assert!(p.status.success(), "{p:?}");
                exports.push(std::fs::read(output).expect("bytes"));
            }
            assert_eq!(exports[0], exports[1], "worker and CLI {format}");
        }
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }

    pub(crate) fn click_label(ctx: &egui::Context, e: &mut Editor, label: &str) {
        let out = frame(ctx, e, vec![]);
        click(ctx, e, text_at(&out, label));
    }
}

#[cfg(test)]
#[path = "sketch/accepted_tests.rs"]
pub(crate) mod accepted_tests;
#[cfg(test)]
#[path = "sketch/analytic_tests.rs"]
pub(crate) mod analytic_tests;
#[cfg(test)]
#[path = "sketch/dimension_tests.rs"]
pub(crate) mod dimension_tests;
#[cfg(test)]
#[path = "sketch/drag_tests.rs"]
pub(crate) mod drag_tests;
#[cfg(test)]
#[path = "sketch/guide_tests.rs"]
pub(crate) mod guide_tests;
#[cfg(test)]
#[path = "sketch/marker_tests.rs"]
pub(crate) mod marker_tests;
#[cfg(test)]
#[path = "sketch/navigation_tests.rs"]
pub(crate) mod navigation_tests;
#[cfg(test)]
#[path = "sketch/object_snap_tests.rs"]
pub(crate) mod object_snap_tests;
#[cfg(test)]
#[path = "sketch/turn_label_tests.rs"]
pub(crate) mod turn_label_tests;
