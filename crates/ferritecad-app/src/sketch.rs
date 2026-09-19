// SPDX-License-Identifier: MIT
//! A disposable drawing, separate from the accepted scene and persisted model.
//! No kernel, filesystem, IDs, or document mutation occurs while editing it.
use ferritecad_document::{
    AnnulusChoice, AnnulusEdit, CircleChoice, CircleEdit, ExtrudeEditSource, SketchChoice,
    SketchVertex,
};
use ferritecad_jobs::{
    AnnularExtrusion, CircleExtrusion, EditAnnulusRequest, EditCircleRequest, EditSketchRequest,
    NewDocument, PolygonExtrusion,
};
use ferritecad_types::{CadError, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
struct State {
    points: Vec<[String; 2]>,
    closed: bool,
    height: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            closed: false,
            height: "10".into(),
        }
    }
}

/// Which profile the open draft window is asking for.
///
/// A mode of one window rather than a second window: both produce the same
/// `NewDocument` through the same worker, and switching between them must not
/// throw away what was typed in the other.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Mode {
    #[default]
    Polygon,
    Circle,
    /// One circle with one concentric circular hole: a hollow part.
    Annulus,
}

/// The numbers a circle needs, as typed.
///
/// Text for the same reason the polygon's are: a field being filled in passes
/// through `-`, `1.` and empty, and what a number means is decided once, by
/// the document, when the person says they are finished.
#[derive(Debug, Clone, PartialEq)]
struct CircleState {
    center: [String; 2],
    radius: String,
    height: String,
}
impl Default for CircleState {
    fn default() -> Self {
        Self {
            center: ["0".into(), "0".into()],
            radius: "10".into(),
            height: "10".into(),
        }
    }
}

/// The numbers a circle with one concentric hole needs, as typed.
///
/// Its own state rather than a circle's with a field added, for the same
/// reason the circle's is its own: switching profile must not throw away what
/// was typed in the other, and a shared radius would make "Radius" mean two
/// different things depending on which form was last open.
#[derive(Debug, Clone, PartialEq)]
struct AnnulusState {
    center: [String; 2],
    outer_radius: String,
    inner_radius: String,
    height: String,
}
impl Default for AnnulusState {
    fn default() -> Self {
        Self {
            center: ["0".into(), "0".into()],
            outer_radius: "10".into(),
            inner_radius: "4".into(),
            height: "10".into(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Editor {
    pub(crate) constraints: crate::constraints::Editor,
    /// The circular cut form. Its own editor beside the constraint one: a cut
    /// is added to a body's history, not to a drawing, and it reads a different
    /// catalogue of the same pinned reading.
    pub(crate) cuts: crate::cuts::Editor,
    draft: Option<State>,
    undo: Vec<State>,
    redo: Vec<State>,
    next: [String; 2],
    pending: Option<NewDocument>,
    canvas: Canvas,
    editing: Option<(EditSketchRequest, SketchChoice)>,
    pending_edit: Option<EditSketchRequest>,
    /// Which profile the open window is asking for, and the circle's numbers.
    /// Both outlive a trip through the other mode; only Cancel clears them.
    mode: Mode,
    circle: CircleState,
    /// The annular form's numbers. Outlives a trip through either other mode,
    /// exactly as the circle's do; only Cancel clears them.
    annulus: AnnulusState,
    /// The saved circle being edited, with the request it was read from.
    editing_circle: Option<(EditCircleRequest, CircleChoice)>,
    pending_circle_edit: Option<EditCircleRequest>,
    /// The circle draft's own bounded history, under the same policy and the
    /// same bound as the polygon draft's beside it.
    circle_undo: Vec<CircleState>,
    circle_redo: Vec<CircleState>,
    circle_applied: Option<CircleState>,
    /// The saved pair of circles being edited, with the request it was read
    /// from, and that draft's own bounded history under the same policy.
    editing_annulus: Option<(EditAnnulusRequest, AnnulusChoice)>,
    pending_annulus_edit: Option<EditAnnulusRequest>,
    annulus_undo: Vec<AnnulusState>,
    annulus_redo: Vec<AnnulusState>,
    annulus_applied: Option<AnnulusState>,
    /// Publication is complete, but its picture has not yet been accepted.
    /// Keep one recovery draft without preventing the ordinary async Open.
    published_draft: Option<(PathBuf, Box<Editor>)>,
}
/// How many draft checkpoints either editor keeps. One bound, one policy.
const DRAFT_HISTORY: usize = 128;
fn push_bounded<T>(stack: &mut Vec<T>, value: T) {
    if stack.len() == DRAFT_HISTORY {
        stack.remove(0);
    }
    stack.push(value);
}

impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some()
            || self.editing_circle.is_some()
            || self.editing_annulus.is_some()
            || self.constraints.active()
            || self.cuts.active()
    }
    pub(crate) fn dismiss(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn take_request(&mut self) -> Option<NewDocument> {
        self.pending.take()
    }
    pub(crate) fn take_edit_request(&mut self) -> Option<EditSketchRequest> {
        self.pending_edit.take()
    }
    pub(crate) fn take_circle_edit_request(&mut self) -> Option<EditCircleRequest> {
        self.pending_circle_edit.take()
    }
    pub(crate) fn take_annulus_edit_request(&mut self) -> Option<EditAnnulusRequest> {
        self.pending_annulus_edit.take()
    }
    pub(crate) fn take_cut_request(&mut self) -> Option<ferritecad_jobs::CircularCutRequest> {
        self.cuts.take_request()
    }
    pub(crate) fn take_cut_edit_request(
        &mut self,
    ) -> Option<ferritecad_jobs::EditCircularCutRequest> {
        self.cuts.take_edit_request()
    }
    /// Begin editing one saved pair of concentric circles of the accepted scene.
    ///
    /// The path and the version come from the reading that was accepted, so a
    /// later Open that has not been accepted cannot retarget this draft. The
    /// form opens on the bounding circle's stored centre; applying an edit puts
    /// both circles at exactly one centre, which the two may not be already.
    pub(crate) fn begin_annulus_edit(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        id: ferritecad_types::ObjectId,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .annulus_sketches
            .iter()
            .find(|c| c.sketch == id && c.refusal.is_none())
        else {
            return false;
        };
        let Some(saved) = &choice.annulus else {
            return false;
        };
        self.dismiss();
        self.mode = Mode::Annulus;
        self.annulus = AnnulusState {
            center: saved.center_mm.map(|n| n.to_string()),
            outer_radius: saved.outer_radius_mm.to_string(),
            inner_radius: saved.inner_radius_mm.to_string(),
            height: saved.height_mm.to_string(),
        };
        self.annulus_applied = Some(self.annulus.clone());
        self.editing_annulus = Some((
            EditAnnulusRequest {
                source: path.to_path_buf(),
                expected: source.version,
                sketch: id,
                edit: AnnulusEdit {
                    outer_curve_id: saved.outer_curve_id,
                    inner_curve_id: saved.inner_curve_id,
                    center_mm: saved.center_mm,
                    outer_radius_mm: saved.outer_radius_mm,
                    inner_radius_mm: saved.inner_radius_mm,
                },
                destination: PathBuf::new(),
            },
            choice.clone(),
        ));
        true
    }
    /// What the annular edit form is asking for, or why it is not an edit yet.
    fn annulus_edit_request(&self) -> Result<EditAnnulusRequest> {
        let (basis, choice) = self
            .editing_annulus
            .as_ref()
            .ok_or_else(|| CadError::input("no saved annulus draft"))?;
        let mut request = basis.clone();
        request.edit = AnnulusEdit {
            // The saved identities in their saved roles, never ones read back
            // out of a text box.
            outer_curve_id: basis.edit.outer_curve_id,
            inner_curve_id: basis.edit.inner_curve_id,
            center_mm: [
                number(&self.annulus.center[0])?,
                number(&self.annulus.center[1])?,
            ],
            outer_radius_mm: number(&self.annulus.outer_radius)?,
            inner_radius_mm: number(&self.annulus.inner_radius)?,
        };
        choice.validate_annulus(&request.edit)?;
        Ok(request)
    }
    fn apply_annulus(&mut self) -> Result<()> {
        let request = self.annulus_edit_request()?;
        self.annulus.center = request.edit.center_mm.map(|n| n.to_string());
        self.annulus.outer_radius = request.edit.outer_radius_mm.to_string();
        self.annulus.inner_radius = request.edit.inner_radius_mm.to_string();
        let before = self
            .annulus_applied
            .replace(self.annulus.clone())
            .ok_or_else(|| CadError::input("no applied annulus draft"))?;
        if self.annulus != before {
            push_bounded(&mut self.annulus_undo, before);
            self.annulus_redo.clear();
        }
        Ok(())
    }
    /// Begin editing one saved analytic circle of the accepted scene.
    ///
    /// The path and the version come from the reading that was accepted, so a
    /// later Open that has not been accepted cannot retarget this draft.
    pub(crate) fn begin_circle_edit(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        id: ferritecad_types::ObjectId,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .circle_sketches
            .iter()
            .find(|c| c.sketch == id && c.refusal.is_none())
        else {
            return false;
        };
        let Some(saved) = &choice.circle else {
            return false;
        };
        self.dismiss();
        self.mode = Mode::Circle;
        self.circle = CircleState {
            center: saved.center_mm.map(|n| n.to_string()),
            radius: saved.radius_mm.to_string(),
            height: saved.height_mm.to_string(),
        };
        self.circle_applied = Some(self.circle.clone());
        self.editing_circle = Some((
            EditCircleRequest {
                source: path.to_path_buf(),
                expected: source.version,
                sketch: id,
                edit: CircleEdit {
                    curve_id: saved.curve_id,
                    center_mm: saved.center_mm,
                    radius_mm: saved.radius_mm,
                },
                destination: PathBuf::new(),
            },
            choice.clone(),
        ));
        true
    }
    /// What the circle edit form is asking for, or why it is not an edit yet.
    fn circle_edit_request(&self) -> Result<EditCircleRequest> {
        let (basis, choice) = self
            .editing_circle
            .as_ref()
            .ok_or_else(|| CadError::input("no saved Circle draft"))?;
        let mut request = basis.clone();
        request.edit = CircleEdit {
            // The saved identity, never one read back out of a text box.
            curve_id: basis.edit.curve_id,
            center_mm: [
                number(&self.circle.center[0])?,
                number(&self.circle.center[1])?,
            ],
            radius_mm: number(&self.circle.radius)?,
        };
        choice.validate_circle(&request.edit)?;
        Ok(request)
    }
    fn apply_circle(&mut self) -> Result<()> {
        let request = self.circle_edit_request()?;
        self.circle.center = request.edit.center_mm.map(|n| n.to_string());
        self.circle.radius = request.edit.radius_mm.to_string();
        let before = self
            .circle_applied
            .replace(self.circle.clone())
            .ok_or_else(|| CadError::input("no applied Circle draft"))?;
        if self.circle != before {
            push_bounded(&mut self.circle_undo, before);
            self.circle_redo.clear();
        }
        Ok(())
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
    pub(crate) fn begin_edit(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        id: ferritecad_types::ObjectId,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .sketches
            .iter()
            .find(|s| s.sketch == id && s.refusal.is_none())
        else {
            return false;
        };
        let (Some(vertices), Some(height)) = (&choice.vertices, choice.height_mm) else {
            return false;
        };
        self.dismiss();
        self.draft = Some(State {
            points: vertices
                .iter()
                .map(|v| v.start_mm.map(|n| n.to_string()))
                .collect(),
            closed: true,
            height: height.to_string(),
        });
        self.canvas
            .fit(&vertices.iter().map(|v| v.start_mm).collect::<Vec<_>>());
        self.editing = Some((
            EditSketchRequest {
                source: path.to_path_buf(),
                expected: source.version,
                sketch: id,
                vertices: vertices.clone(),
                destination: PathBuf::new(),
            },
            choice.clone(),
        ));
        true
    }
    pub(crate) fn draw_choices(
        &mut self,
        ui: &mut egui::Ui,
        can_begin: bool,
        path: Option<&Path>,
        source: Option<&ExtrudeEditSource>,
    ) {
        if self.active() {
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("saved-sketch-actions")
            .max_height(180.)
            .show(ui, |ui| self.draw_choice_rows(ui, can_begin, path, source));
    }
    fn draw_choice_rows(
        &mut self,
        ui: &mut egui::Ui,
        can_begin: bool,
        path: Option<&Path>,
        source: Option<&ExtrudeEditSource>,
    ) {
        if !self.active() {
            self.constraints.choices(ui, can_begin, path, source);
            self.cuts.choices(ui, can_begin, path, source);
        }
        if self.active() {
            return;
        }
        if let (Some(path), Some(source)) = (path, source) {
            for choice in &source.sketches {
                let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
                let response = ui.add_enabled(
                    can_begin && refusal.is_none(),
                    egui::Button::new(format!(
                        "Edit Sketch {} — {}…",
                        choice.name.as_deref().unwrap_or("Unnamed"),
                        choice.sketch
                    )),
                );
                if response.clicked() {
                    self.begin_edit(path, source, choice.sketch);
                }
                if let Some(reason) = refusal {
                    response.on_hover_text(reason);
                }
            }
            for choice in &source.circle_sketches {
                let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
                let response = ui.add_enabled(
                    can_begin && refusal.is_none(),
                    egui::Button::new(format!(
                        "Edit circle {} — {}…",
                        choice.name.as_deref().unwrap_or("Unnamed"),
                        choice.sketch
                    )),
                );
                if response.clicked() {
                    self.begin_circle_edit(path, source, choice.sketch);
                }
                if let Some(reason) = refusal {
                    response.on_hover_text(reason);
                }
            }
            for choice in &source.annulus_sketches {
                let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
                let response = ui.add_enabled(
                    can_begin && refusal.is_none(),
                    egui::Button::new(format!(
                        "Edit annulus {} — {}…",
                        choice.name.as_deref().unwrap_or("Unnamed"),
                        choice.sketch
                    )),
                );
                if response.clicked() {
                    self.begin_annulus_edit(path, source, choice.sketch);
                }
                if let Some(reason) = refusal {
                    response.on_hover_text(reason);
                }
            }
        }
    }
    fn edit_request(&self) -> Result<EditSketchRequest> {
        let (basis, choice) = self
            .editing
            .as_ref()
            .ok_or_else(|| CadError::input("no saved Sketch draft"))?;
        let draft = self
            .draft
            .as_ref()
            .ok_or_else(|| CadError::input("no draft"))?;
        if draft.points.len() != basis.vertices.len() {
            return Err(CadError::input("segment count cannot change"));
        }
        let mut request = basis.clone();
        request.vertices = basis
            .vertices
            .iter()
            .zip(&draft.points)
            .map(|(v, p)| {
                Ok(SketchVertex {
                    curve_id: v.curve_id,
                    start_mm: [number(&p[0])?, number(&p[1])?],
                })
            })
            .collect::<Result<_>>()?;
        choice.validate_coordinates(&request.vertices)?;
        Ok(request)
    }
    fn begin(&mut self) {
        self.dismiss();
        self.draft = Some(State::default());
        self.next = ["0".into(), "0".into()];
    }
    /// What the circle form is asking for, or why it is not a circle yet.
    ///
    /// Parses here and decides nothing else: whether a number that parses is
    /// an acceptable size belongs to the document, which refuses what it will
    /// not store and says why.
    fn circle_content(&self) -> Result<NewDocument> {
        Ok(NewDocument::CircleExtrude(CircleExtrusion::new(
            [
                number(&self.circle.center[0])?,
                number(&self.circle.center[1])?,
            ],
            number(&self.circle.radius)?,
            number(&self.circle.height)?,
        )?))
    }
    /// What the annular form is asking for, or why it is not one yet.
    ///
    /// Parses here and decides nothing else, exactly as the circle form does:
    /// whether two radii that parse are a boundary and a hole — concentric, one
    /// inside the other, with a wall thick enough to be a wall — belongs to the
    /// document, which refuses what it will not store and says why.
    fn annulus_content(&self) -> Result<NewDocument> {
        Ok(NewDocument::AnnularExtrude(AnnularExtrusion::new(
            [
                number(&self.annulus.center[0])?,
                number(&self.annulus.center[1])?,
            ],
            number(&self.annulus.outer_radius)?,
            number(&self.annulus.inner_radius)?,
            number(&self.annulus.height)?,
        )?))
    }
    fn record(&mut self, before: State) {
        if self.draft.as_ref() != Some(&before) {
            self.push_undo(before);
        }
    }
    fn push_undo(&mut self, before: State) {
        push_bounded(&mut self.undo, before);
        self.redo.clear();
    }
    fn undo(&mut self) {
        if self.editing_annulus.is_some() {
            if let Some(previous) = self.annulus_undo.pop() {
                push_bounded(
                    &mut self.annulus_redo,
                    self.annulus_applied
                        .replace(previous.clone())
                        .expect("applied annulus"),
                );
                self.annulus = previous;
            }
            return;
        }
        if self.editing_circle.is_some() {
            if let Some(previous) = self.circle_undo.pop() {
                push_bounded(
                    &mut self.circle_redo,
                    self.circle_applied
                        .replace(previous.clone())
                        .expect("applied circle"),
                );
                self.circle = previous;
            }
            return;
        }
        if let Some(previous) = self.undo.pop()
            && let Some(current) = self.draft.replace(previous)
        {
            self.redo.push(current);
        }
    }
    fn redo(&mut self) {
        if self.editing_annulus.is_some() {
            if let Some(next) = self.annulus_redo.pop() {
                push_bounded(
                    &mut self.annulus_undo,
                    self.annulus_applied
                        .replace(next.clone())
                        .expect("applied annulus"),
                );
                self.annulus = next;
            }
            return;
        }
        if self.editing_circle.is_some() {
            if let Some(next) = self.circle_redo.pop() {
                push_bounded(
                    &mut self.circle_undo,
                    self.circle_applied
                        .replace(next.clone())
                        .expect("applied circle"),
                );
                self.circle = next;
            }
            return;
        }
        if let Some(next) = self.redo.pop()
            && let Some(current) = self.draft.replace(next)
        {
            self.undo.push(current);
        }
    }
    fn content(&self) -> Result<NewDocument> {
        let draft = self
            .draft
            .as_ref()
            .ok_or_else(|| CadError::input("no sketch draft"))?;
        if !draft.closed {
            return Err(CadError::input(
                "Close the contour explicitly before creating.",
            ));
        }
        let points = draft
            .points
            .iter()
            .map(|p| Ok([number(&p[0])?, number(&p[1])?]))
            .collect::<Result<_>>()?;
        Ok(NewDocument::SketchExtrude(PolygonExtrusion::new(
            points,
            number(&draft.height)?,
        )?))
    }
    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, can_begin: bool, running: bool) {
        if self.constraints.active() {
            self.constraints.draw(ui, running);
            return;
        }
        if self.cuts.active() {
            self.cuts.draw(ui, running);
            return;
        }
        if !self.active() {
            if ui
                .add_enabled(can_begin, egui::Button::new("Create sketch + Extrude…"))
                .clicked()
            {
                self.begin();
            }
            return;
        }
        egui::Window::new(if self.editing_annulus.is_some() {
            "Edit saved annulus — new copy"
        } else if self.editing_circle.is_some() {
            "Edit saved Circle — new copy"
        } else if self.editing.is_some() {
            "Edit saved Sketch — new copy"
        } else {
            "Sketch + Extrude — new document"
        })
        .resizable(false)
        .default_width(540.)
        .show(ui.ctx(), |ui| self.draw_draft(ui, running));
    }

    /// The circle half of the same window.
    ///
    /// Four numbers and one action. The polygon draft beside it is untouched
    /// while this is on screen, so switching back finds the points that were
    /// already there.
    fn draw_circle(&mut self, ui: &mut egui::Ui) {
        ui.label("XY · mm · one analytic circle · Blind · NewBody");
        ui.label("The circle is stored as a centre and a radius, and stays one in the solid.");
        let [x, y] = &mut self.circle.center;
        let fields: [(&str, &mut String); 4] = [
            ("Center X", x),
            ("Center Y", y),
            ("Radius", &mut self.circle.radius),
            ("Blind height", &mut self.circle.height),
        ];
        egui::Grid::new("ferritecad circle numbers")
            .num_columns(3)
            .show(ui, |ui| {
                for (label, value) in fields {
                    ui.label(label);
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .char_limit(64)
                            .desired_width(120.),
                    );
                    ui.label("mm");
                    ui.end_row();
                }
            });
        match self.circle_content() {
            Ok(content) => {
                if ui.button("Save circle extrusion…").clicked() {
                    self.pending = Some(content);
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        }
    }

    /// The annular half of the same window.
    ///
    /// Five numbers and one action. Both other drafts beside it are untouched
    /// while this is on screen, so switching back finds what was already there.
    fn draw_annulus(&mut self, ui: &mut egui::Ui) {
        ui.label("XY · mm · two concentric analytic circles · Blind · NewBody");
        ui.label(
            "The hole is a loop of the same profile, so it is in the solid and in every export.",
        );
        let [x, y] = &mut self.annulus.center;
        let fields: [(&str, &mut String); 5] = [
            ("Center X", x),
            ("Center Y", y),
            ("Outer radius", &mut self.annulus.outer_radius),
            ("Inner radius", &mut self.annulus.inner_radius),
            ("Blind height", &mut self.annulus.height),
        ];
        egui::Grid::new("ferritecad annulus numbers")
            .num_columns(3)
            .show(ui, |ui| {
                for (label, value) in fields {
                    ui.label(label);
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .char_limit(64)
                            .desired_width(120.),
                    );
                    ui.label("mm");
                    ui.end_row();
                }
            });
        match self.annulus_content() {
            Ok(content) => {
                if ui.button("Save annular extrusion…").clicked() {
                    self.pending = Some(content);
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        }
    }

    /// The saved-circle half of the same window.
    ///
    /// Shows what is stored, by identity, and offers the two numbers this edit
    /// may change. The height is shown because it decides what they mean and
    /// is deliberately not editable here: `Edit extrusion` owns it.
    fn draw_circle_edit(&mut self, ui: &mut egui::Ui) {
        let Some((request, choice)) = &self.editing_circle else {
            return;
        };
        ui.label("XY · mm · saved analytic circle · centre and radius only");
        ui.small(format!(
            "Sketch {} · {}",
            request.sketch,
            request.source.display()
        ));
        if let Some(saved) = &choice.circle {
            ui.small(format!(
                "Circle {} · saved centre ({}, {}) mm · radius {} mm · height {} mm",
                saved.curve_id,
                saved.center_mm[0],
                saved.center_mm[1],
                saved.radius_mm,
                saved.height_mm
            ));
        }
        let height = self.circle.height.clone();
        let [x, y] = &mut self.circle.center;
        let fields: [(&str, &mut String); 3] = [
            ("Center X", x),
            ("Center Y", y),
            ("Radius", &mut self.circle.radius),
        ];
        egui::Grid::new("ferritecad saved circle numbers")
            .num_columns(3)
            .show(ui, |ui| {
                for (label, value) in fields {
                    ui.label(label);
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .char_limit(64)
                            .desired_width(120.),
                    );
                    ui.label("mm");
                    ui.end_row();
                }
                ui.label("Blind height");
                ui.label(&height);
                ui.label("mm (retained)");
                ui.end_row();
            });
        match self.circle_edit_request() {
            Ok(request) => {
                let pending = self.circle_applied.as_ref() != Some(&self.circle);
                if ui
                    .add_enabled(pending, egui::Button::new("Apply circle change"))
                    .clicked()
                {
                    self.apply_circle().expect("validated circle draft");
                }
                let applied = self.circle_applied.as_ref() == Some(&self.circle);
                if ui
                    .add_enabled(applied, egui::Button::new("Save edited circle copy…"))
                    .clicked()
                {
                    self.pending_circle_edit = Some(request);
                }
                if !applied {
                    ui.small("Apply the numbers before saving the copy.");
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        }
    }

    /// The saved-annulus half of the same window.
    ///
    /// Shows what is stored, by identity, and offers the three numbers this edit
    /// may change. The height is shown because it decides what they mean and is
    /// deliberately not editable here: `Edit extrusion` owns it.
    fn draw_annulus_edit(&mut self, ui: &mut egui::Ui) {
        let Some((request, choice)) = &self.editing_annulus else {
            return;
        };
        ui.label("XY · mm · saved annular profile · one centre and both radii");
        ui.small(format!(
            "Sketch {} · {}",
            request.sketch,
            request.source.display()
        ));
        if let Some(saved) = &choice.annulus {
            ui.small(format!(
                "Boundary {} · saved centre ({}, {}) mm · radius {} mm",
                saved.outer_curve_id, saved.center_mm[0], saved.center_mm[1], saved.outer_radius_mm
            ));
            ui.small(format!(
                "Bore {} · saved centre ({}, {}) mm · radius {} mm · height {} mm",
                saved.inner_curve_id,
                saved.inner_center_mm[0],
                saved.inner_center_mm[1],
                saved.inner_radius_mm,
                saved.height_mm
            ));
        }
        let height = self.annulus.height.clone();
        let [x, y] = &mut self.annulus.center;
        let fields: [(&str, &mut String); 4] = [
            ("Center X", x),
            ("Center Y", y),
            ("Outer radius", &mut self.annulus.outer_radius),
            ("Inner radius", &mut self.annulus.inner_radius),
        ];
        egui::Grid::new("ferritecad saved annulus numbers")
            .num_columns(3)
            .show(ui, |ui| {
                for (label, value) in fields {
                    ui.label(label);
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .char_limit(64)
                            .desired_width(120.),
                    );
                    ui.label("mm");
                    ui.end_row();
                }
                ui.label("Blind height");
                ui.label(&height);
                ui.label("mm (retained)");
                ui.end_row();
            });
        match self.annulus_edit_request() {
            Ok(request) => {
                let pending = self.annulus_applied.as_ref() != Some(&self.annulus);
                if ui
                    .add_enabled(pending, egui::Button::new("Apply annulus change"))
                    .clicked()
                {
                    self.apply_annulus().expect("validated annulus draft");
                }
                let applied = self.annulus_applied.as_ref() == Some(&self.annulus);
                if ui
                    .add_enabled(applied, egui::Button::new("Save edited annulus copy…"))
                    .clicked()
                {
                    self.pending_annulus_edit = Some(request);
                }
                if !applied {
                    ui.small("Apply the numbers before saving the copy.");
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        }
    }

    fn draw_draft(&mut self, ui: &mut egui::Ui, running: bool) {
        if self.editing_annulus.is_some() {
            ui.add_enabled_ui(!running, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.annulus_undo.is_empty(),
                            egui::Button::new("Undo draft"),
                        )
                        .clicked()
                    {
                        self.undo();
                    }
                    if ui
                        .add_enabled(
                            !self.annulus_redo.is_empty(),
                            egui::Button::new("Redo draft"),
                        )
                        .clicked()
                    {
                        self.redo();
                    }
                    if ui.button("Cancel draft").clicked() {
                        self.dismiss();
                    }
                });
            });
            // Cancel took the draft down; there is nothing left to draw.
            if self.editing_annulus.is_none() {
                return;
            }
            ui.add_enabled_ui(!running, |ui| self.draw_annulus_edit(ui));
            if running {
                ui.label("Saving… Draft retained until publication. Cancel job in toolbar.");
            }
            ui.small("Undo/redo changes only this draft; history ends at publication.");
            return;
        }
        if self.editing_circle.is_some() {
            ui.add_enabled_ui(!running, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.circle_undo.is_empty(),
                            egui::Button::new("Undo draft"),
                        )
                        .clicked()
                    {
                        self.undo();
                    }
                    if ui
                        .add_enabled(
                            !self.circle_redo.is_empty(),
                            egui::Button::new("Redo draft"),
                        )
                        .clicked()
                    {
                        self.redo();
                    }
                    if ui.button("Cancel draft").clicked() {
                        self.dismiss();
                    }
                });
            });
            // Cancel took the draft down; there is nothing left to draw.
            if self.editing_circle.is_none() {
                return;
            }
            ui.add_enabled_ui(!running, |ui| self.draw_circle_edit(ui));
            if running {
                ui.label("Saving… Draft retained until publication. Cancel job in toolbar.");
            }
            ui.small("Undo/redo changes only this draft; history ends at publication.");
            return;
        }
        // Editing a saved Sketch is not creating one, so it offers no choice
        // of profile: the document already decided what it holds.
        if self.editing.is_none() {
            ui.add_enabled_ui(!running, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Profile:");
                    ui.selectable_value(&mut self.mode, Mode::Polygon, "Line polygon");
                    ui.selectable_value(&mut self.mode, Mode::Circle, "Circle");
                    ui.selectable_value(&mut self.mode, Mode::Annulus, "Circle with hole");
                });
            });
            if matches!(self.mode, Mode::Circle | Mode::Annulus) {
                let annular = self.mode == Mode::Annulus;
                ui.add_enabled_ui(!running, |ui| {
                    if annular {
                        self.draw_annulus(ui);
                    } else {
                        self.draw_circle(ui);
                    }
                    if ui.button("Cancel draft").clicked() {
                        self.dismiss();
                    }
                });
                if running {
                    ui.label("Saving… Draft retained until publication. Cancel job in toolbar.");
                }
                return;
            }
        }
        ui.label("XY · mm · Line polygon · Blind · NewBody");
        ui.label(if self.editing.is_some() {
            "Edit exact coordinates. Curve IDs, order, closure and height are retained."
        } else {
            "Click to add vertices, or enter exact coordinates. Last edge closes to vertex 1."
        });
        if let Some((request, choice)) = &self.editing {
            if let Some(history) = &choice.cut_history {
                ui.label(format!(
                    "Base of {} circular Cuts. Tools stay at their saved XY coordinates.",
                    history.tools.len()
                ));
                ui.small("Keep an axis-aligned rectangle with clearance from every tool. Each vertex moves only when you edit it.");
            }
            ui.small(format!(
                "Sketch {} · {}",
                request.sketch,
                request.source.display()
            ));
        }
        ui.add_enabled_ui(!running, |ui| self.edit(ui));
        if running {
            ui.label("Saving… Draft retained until publication. Cancel job in toolbar.");
        }
        ui.small("Undo/redo changes only this draft; history ends at publication.");
    }

    fn edit(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(self.canvas.gesture.is_none(), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo draft"))
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo draft"))
                    .clicked()
                {
                    self.redo();
                }
                if ui.button("Cancel draft").clicked() {
                    self.dismiss();
                }
            })
        });
        let Some(before) = self.draft.clone() else {
            return;
        };
        let draft = self.draft.as_mut().expect("present");
        let canvas_edit = self.canvas.draw(ui, draft);
        // A pointer gesture owns its checkpoint. Ordinary numeric/button edits
        // keep their existing per-change history and cannot modify a live drag.
        ui.add_enabled_ui(matches!(canvas_edit, CanvasEdit::Ordinary), |ui| {
            ui.add_enabled_ui(self.editing.is_none(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Next X");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.next[0])
                            .char_limit(64)
                            .desired_width(75.),
                    );
                    ui.label("Y");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.next[1])
                            .char_limit(64)
                            .desired_width(75.),
                    );
                    if ui
                        .add_enabled(
                            !draft.closed && draft.points.len() < PolygonExtrusion::MAX_POINTS,
                            egui::Button::new("Add point"),
                        )
                        .clicked()
                    {
                        draft.points.push(self.next.clone());
                    }
                    if ui
                        .add_enabled(!draft.closed, egui::Button::new("Close contour"))
                        .clicked()
                    {
                        draft.closed = true;
                    }
                });
            });
            egui::ScrollArea::vertical()
                .max_height(160.)
                .show(ui, |ui| {
                    let mut remove = None;
                    for (i, p) in draft.points.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            let label = egui::RichText::new(format!("{}  X mm", i + 1));
                            ui.label(if self.canvas.selected == Some(i) {
                                label.strong().color(egui::Color32::LIGHT_YELLOW)
                            } else {
                                label
                            });
                            ui.add(
                                egui::TextEdit::singleline(&mut p[0])
                                    .char_limit(64)
                                    .desired_width(125.),
                            );
                            ui.label("Y mm");
                            ui.add(
                                egui::TextEdit::singleline(&mut p[1])
                                    .char_limit(64)
                                    .desired_width(125.),
                            );
                            if ui
                                .add_enabled(self.editing.is_none(), egui::Button::new("Remove"))
                                .clicked()
                            {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        draft.points.remove(i);
                        self.canvas.selected = None;
                    }
                });
            ui.horizontal(|ui| {
                ui.label("Blind height mm");
                ui.add_enabled(
                    self.editing.is_none(),
                    egui::TextEdit::singleline(&mut draft.height)
                        .char_limit(64)
                        .desired_width(100.),
                );
            });
        });
        match canvas_edit {
            CanvasEdit::Ordinary => self.record(before),
            CanvasEdit::Gesture => {}
            CanvasEdit::Finished(checkpoints) => {
                for before in checkpoints {
                    self.push_undo(before);
                }
            }
        }
        if self.editing.is_some() {
            match self.edit_request() {
                Ok(request) => {
                    if ui
                        .add_enabled(
                            self.canvas.gesture.is_none(),
                            egui::Button::new("Save edited copy…"),
                        )
                        .clicked()
                    {
                        self.pending_edit = Some(request);
                    }
                }
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                }
            }
            return;
        }
        match self.content() {
            Ok(content) => {
                if ui
                    .add_enabled(
                        self.canvas.gesture.is_none(),
                        egui::Button::new("Create in new file…"),
                    )
                    .clicked()
                {
                    self.pending = Some(content);
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        }
    }
}

pub(crate) fn finish_edit(
    editor: &mut Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::EditedSketch>,
) -> Option<PathBuf> {
    if edits.accepts(generation)
        && let Ok(saved) = &result
    {
        editor.draft_published(&saved.destination);
    }
    edits.finish_sketch(generation, result)
}

/// Finish one circle edit at the application boundary.
///
/// The draft survives everything except a publication: a refusal, a stale
/// version and a reply for a request that is no longer current all leave the
/// numbers on screen to try again with.
pub(crate) fn finish_circle_edit(
    editor: &mut Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::EditedCircle>,
) -> Option<PathBuf> {
    if edits.accepts(generation)
        && let Ok(saved) = &result
    {
        editor.draft_published(&saved.destination);
    }
    edits.finish_circle(generation, result)
}

/// Finish one annulus edit at the application boundary.
///
/// The same two steps the circle edit takes, through the same shared draft
/// mechanism: a published copy hands its draft to `draft_published` so an Open
/// that is later refused can give it back, and the generation check stays the
/// worker state's.
pub(crate) fn finish_annulus_edit(
    editor: &mut Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::EditedAnnulus>,
) -> Option<PathBuf> {
    if edits.accepts(generation)
        && let Ok(saved) = &result
    {
        editor.draft_published(&saved.destination);
    }
    edits.finish_annulus(generation, result)
}

/// Drawing coordinates are a view of numbers, never a source of modelling rules.
#[derive(Debug)]
struct Canvas {
    minimum: [f64; 2],
    scale: f32,
    selected: Option<usize>,
    gesture: Option<VertexDrag>,
    claimed_press: bool,
    snap: Snap,
}

/// Editor-only step. Not model data, not a preference, not persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Snap {
    #[default]
    Off,
    Tenth,
    One,
    Five,
    Ten,
}

#[derive(Debug)]
struct VertexDrag {
    vertex: usize,
    press: egui::Pos2,
    scale: f64,
    start: [f64; 2],
    before: State,
    snap: Snap,
}

/// Preview frames do not enter history. Each changed release supplies one
/// checkpoint, even when several gestures arrive in a frame. Cancellation
/// restores only the active gesture without changing undo or redo.
enum CanvasEdit {
    Ordinary,
    Gesture,
    Finished(Vec<State>),
}
impl Snap {
    const CHOICES: [(Self, &'static str); 5] = [
        (Self::Off, "Off"),
        (Self::Tenth, "0.1 mm"),
        (Self::One, "1 mm"),
        (Self::Five, "5 mm"),
        (Self::Ten, "10 mm"),
    ];
    fn label(self) -> &'static str {
        Snap::CHOICES
            .iter()
            .find(|(value, _)| *value == self)
            .map(|(_, label)| *label)
            .expect("every Snap value has a label")
    }
    fn apply(self, value: f64) -> String {
        let snapped = match self {
            Self::Off => return value.to_string(),
            // Multiply/divide by ten instead of by the inexact binary 0.1:
            // decimal halves round away from zero, and 3/10 displays as "0.3".
            Self::Tenth => (value * 10.0).round() / 10.0,
            Self::One => value.round(),
            Self::Five => (value / 5.0).round() * 5.0,
            Self::Ten => (value / 10.0).round() * 10.0,
        };
        if !snapped.is_finite() {
            // Keep invalid input invalid for the shared polygon policy. A
            // float-to-integer cast would turn NaN into zero or saturate it.
            value.to_string()
        } else if snapped == 0.0 {
            "0".into()
        } else {
            snapped.to_string()
        }
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            minimum: [-8.75, -7.5],
            scale: 4.0,
            selected: None,
            gesture: None,
            claimed_press: false,
            snap: Snap::Off,
        }
    }
}
impl Canvas {
    fn fit(&mut self, points: &[[f64; 2]]) {
        if points.is_empty() {
            return;
        }
        let mut lo = points[0];
        let mut hi = points[0];
        for p in points {
            for j in 0..2 {
                lo[j] = lo[j].min(p[j]);
                hi[j] = hi[j].max(p[j]);
            }
        }
        let width = (hi[0] - lo[0]).max(1e-5);
        let height = (hi[1] - lo[1]).max(1e-5);
        self.scale = (450.0 / width).min(190.0 / height) as f32;
        self.minimum = [
            lo[0] - 30.0 / f64::from(self.scale),
            lo[1] - 30.0 / f64::from(self.scale),
        ];
    }
    fn points(draft: &State) -> Option<Vec<[f64; 2]>> {
        draft
            .points
            .iter()
            .map(|p| {
                let [x, y] = [number(&p[0]).ok()?, number(&p[1]).ok()?];
                (x.is_finite() && y.is_finite() && x.abs() <= 1e6 && y.abs() <= 1e6)
                    .then_some([x, y])
            })
            .collect()
    }
    fn screen(&self, rect: egui::Rect, p: [f64; 2]) -> egui::Pos2 {
        rect.left_bottom()
            + egui::vec2(
                ((p[0] - self.minimum[0]) * f64::from(self.scale)) as f32,
                -((p[1] - self.minimum[1]) * f64::from(self.scale)) as f32,
            )
    }
    fn hit(
        &self,
        rect: egui::Rect,
        visible: egui::Rect,
        points: &[[f64; 2]],
        at: egui::Pos2,
    ) -> Option<usize> {
        const RADIUS: f32 = 9.0; // egui screen points, independent of zoom/DPI
        points
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let screen = self.screen(rect, *p);
                let distance = screen.distance_sq(at);
                (visible.contains(screen) && distance <= RADIUS * RADIUS).then_some((i, distance))
            })
            .min_by(|(i, a), (j, b)| a.total_cmp(b).then(i.cmp(j)))
            .map(|(i, _)| i)
    }
    fn cancel(&mut self, draft: &mut State) {
        if let Some(drag) = self.gesture.take() {
            *draft = drag.before;
        }
    }
    fn preview(&self, draft: &mut State, at: egui::Pos2) {
        if let Some(drag) = &self.gesture {
            // Preserve the grab offset and original strings on untouched axes.
            let delta = [
                f64::from(at.x) - f64::from(drag.press.x),
                f64::from(drag.press.y) - f64::from(at.y),
            ];
            for (axis, delta) in delta.into_iter().enumerate() {
                draft.points[drag.vertex][axis] = if delta == 0.0 {
                    drag.before.points[drag.vertex][axis].clone()
                } else {
                    drag.snap.apply(drag.start[axis] + delta / drag.scale)
                };
            }
        }
    }
    fn draw(&mut self, ui: &mut egui::Ui, draft: &mut State) -> CanvasEdit {
        let points = Self::points(draft);
        let events = ui.input(|i| i.events.clone());
        let mut snap_changes = Vec::new();
        ui.add_enabled_ui(self.gesture.is_none(), |ui| {
            ui.horizontal(|ui| {
                if ui.button("Fit drawing").clicked()
                    && let Some(p) = &points
                {
                    self.fit(p);
                }
                if ui.button("Reset view").clicked() {
                    self.minimum = Self::default().minimum;
                    self.scale = Self::default().scale;
                }
                ui.label("+X right · +Y up · coordinates in mm");
            });
            ui.horizontal(|ui| {
                ui.label("Snap:");
                for (value, label) in Snap::CHOICES {
                    let response = ui.selectable_label(self.snap == value, label);
                    if response.clicked() {
                        // egui evaluates the control before we replay canvas
                        // events. Defer its accepted click to the actual release
                        // so a later Snap choice cannot rewrite an earlier drag.
                        let event = events
                            .iter()
                            .rposition(|event| {
                                matches!(event,
                                    egui::Event::PointerButton {
                                        pos, button: egui::PointerButton::Primary,
                                        pressed: false, ..
                                    } if response.rect.contains(*pos)
                                )
                            })
                            .or_else(|| {
                                events.iter().rposition(|event| {
                                    matches!(
                                        event,
                                        egui::Event::Key {
                                            key: egui::Key::Space | egui::Key::Enter,
                                            pressed: true,
                                            ..
                                        }
                                    )
                                })
                            })
                            .unwrap_or(events.len());
                        snap_changes.push((event, value));
                        ui.ctx().request_repaint();
                    }
                }
            });
        });
        let (response, painter) =
            ui.allocate_painter(egui::vec2(510., 250.), egui::Sense::click_and_drag());
        let rect = response.rect;
        let mut change = if self.claimed_press {
            CanvasEdit::Gesture
        } else {
            CanvasEdit::Ordinary
        };
        let pointer = ui.input(|i| i.pointer.clone());
        let mut checkpoints = Vec::new();
        // Aggregate pointer state loses ordering when one frame contains the
        // release of an old gesture and the press (or whole gesture) of another.
        // Own gestures at their actual press and process every event in order.
        for (index, event) in events.iter().enumerate() {
            for &(_, value) in snap_changes.iter().filter(|(at, _)| *at == index) {
                self.snap = value;
            }
            match *event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } => {
                    self.cancel(draft);
                    if ui.is_enabled()
                        && rect.intersect(ui.clip_rect()).contains(pos)
                        && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
                        && let Some(points) = Self::points(draft)
                        && let Some(vertex) =
                            self.hit(rect, rect.intersect(ui.clip_rect()), &points, pos)
                    {
                        self.selected = Some(vertex);
                        self.claimed_press = true;
                        change = CanvasEdit::Gesture;
                        self.gesture = Some(VertexDrag {
                            vertex,
                            press: pos,
                            scale: f64::from(self.scale),
                            start: points[vertex],
                            before: draft.clone(),
                            snap: self.snap,
                        });
                    }
                }
                egui::Event::PointerMoved(pos) => self.preview(draft, pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    self.preview(draft, pos);
                    if let Some(drag) = self.gesture.take()
                        && *draft != drag.before
                    {
                        if checkpoints.len() == 128 {
                            checkpoints.remove(0);
                        }
                        checkpoints.push(drag.before);
                    }
                    self.claimed_press = false;
                }
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } if self.gesture.is_some() => {
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                    self.cancel(draft);
                }
                egui::Event::WindowFocused(false) | egui::Event::PointerGone => {
                    self.cancel(draft);
                }
                _ => {}
            }
        }
        // Accessibility/programmatic activation can have no pointer/key event.
        for &(_, value) in snap_changes.iter().filter(|(at, _)| *at == events.len()) {
            self.snap = value;
        }
        // A lost release must not leave a preview or a permanently owned drag.
        if !ui.input(|i| i.focused) || !pointer.primary_down() {
            self.cancel(draft);
        }
        if !checkpoints.is_empty() {
            change = CanvasEdit::Finished(checkpoints);
        }
        let points = Self::points(draft);
        let hover = response.hover_pos().and_then(|pos| {
            points
                .as_ref()
                .and_then(|points| self.hit(rect, rect.intersect(ui.clip_rect()), points, pos))
        });
        if matches!(change, CanvasEdit::Ordinary)
            && response.clicked_by(egui::PointerButton::Primary)
        {
            if let Some(vertex) = hover {
                self.selected = Some(vertex);
            } else if !draft.closed
                && draft.points.len() < PolygonExtrusion::MAX_POINTS
                && let Some(pos) = response.interact_pointer_pos()
            {
                let offset = to_document(pos, rect.left_bottom(), self.scale);
                let at = [self.minimum[0] + offset[0], self.minimum[1] + offset[1]];
                draft.points.push(match self.snap {
                    Snap::Off => [format!("{:.3}", at[0]), format!("{:.3}", at[1])],
                    snap => [snap.apply(at[0]), snap.apply(at[1])],
                });
            }
        }
        if !pointer.primary_down() {
            self.claimed_press = false;
        }
        let screen = |p| self.screen(rect, p);
        painter.rect_filled(rect, 0., egui::Color32::from_gray(25));
        // At most ~50 lines, even for very small or far-translated drawings.
        let grid = 10_f64.powf((40.0 / f64::from(self.scale)).log10().floor());
        for axis in 0..2 {
            for i in 0..55 {
                let value = (self.minimum[axis] / grid).ceil() * grid + f64::from(i) * grid;
                let mut p = self.minimum;
                p[axis] = value;
                let start = screen(p);
                let end = if axis == 0 {
                    egui::pos2(start.x, rect.top())
                } else {
                    egui::pos2(rect.right(), start.y)
                };
                painter.line_segment([start, end], (1., egui::Color32::from_gray(48)));
            }
        }
        painter.text(
            rect.left_top() + egui::vec2(5., 5.),
            egui::Align2::LEFT_TOP,
            format!("grid: {grid} mm · snap: {}", self.snap.label()),
            egui::FontId::proportional(12.),
            egui::Color32::WHITE,
        );
        if let Some(points) = Self::points(draft) {
            for pair in points.windows(2) {
                painter.line_segment(
                    [screen(pair[0]), screen(pair[1])],
                    (2., egui::Color32::LIGHT_BLUE),
                );
            }
            if draft.closed && points.len() > 1 {
                painter.line_segment(
                    [screen(points[points.len() - 1]), screen(points[0])],
                    (2., egui::Color32::LIGHT_BLUE),
                );
            }
            for (i, p) in points.iter().enumerate() {
                let p = screen(*p);
                let selected = self.selected == Some(i);
                if hover == Some(i) {
                    painter.circle_stroke(p, 9., (1.5, egui::Color32::LIGHT_BLUE));
                }
                painter.circle_filled(
                    p,
                    if selected { 5. } else { 3. },
                    if selected {
                        egui::Color32::LIGHT_YELLOW
                    } else {
                        egui::Color32::WHITE
                    },
                );
                painter.text(
                    p + egui::vec2(4., -4.),
                    egui::Align2::LEFT_BOTTOM,
                    (i + 1).to_string(),
                    egui::FontId::proportional(12.),
                    egui::Color32::WHITE,
                );
            }
        }
        ui.label(
            self.selected
                .and_then(|i| draft.points.get(i).map(|p| (i, p)))
                .map(|(i, p)| format!("Vertex {} · X {} mm · Y {} mm", i + 1, p[0], p[1]))
                .unwrap_or_else(|| "Select/drag a vertex · Escape cancels the gesture".into()),
        );
        change
    }
}
fn number(s: &str) -> Result<f64> {
    s.trim()
        .parse()
        .map_err(|_| CadError::input(format!("Not a number: {s:?}")))
}
fn to_document(pos: egui::Pos2, origin: egui::Pos2, scale: f32) -> [f64; 2] {
    [
        f64::from((pos.x - origin.x) / scale),
        f64::from((origin.y - pos.y) / scale),
    ]
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    #[test]
    fn draft_undo_redo_closure_and_shared_validity() {
        let mut e = Editor::default();
        e.begin();
        let initial = e.draft.clone().expect("draft");
        e.draft.as_mut().expect("draft").points = vec![
            ["0".into(), "0".into()],
            ["60".into(), "0".into()],
            ["0".into(), "40".into()],
        ];
        e.record(initial.clone());
        assert!(e.content().is_err());
        let open = e.draft.clone().expect("draft");
        e.draft.as_mut().expect("draft").closed = true;
        e.record(open.clone());
        assert!(e.content().is_ok());
        e.undo();
        assert_eq!(e.draft, Some(open));
        e.redo();
        assert!(e.content().is_ok());
        e.undo();
        e.undo();
        assert_eq!(e.draft, Some(initial));
        e.redo();
        e.redo();
        e.draft.as_mut().expect("draft").points[2] = ["60".into(), "0".into()];
        assert!(e.content().is_err());
        e.dismiss();
        assert!(!e.active());
        assert_eq!(
            to_document(egui::pos2(90., 60.), egui::pos2(10., 100.), 4.),
            [20., 10.]
        );
    }

    pub(super) fn frame(
        ctx: &egui::Context,
        e: &mut Editor,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        frame_running(ctx, e, events, false)
    }
    fn frame_running(
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
            |ui| e.draw(ui, true, running),
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
    /// Real egui widgets and pointer input, without winit/GPU or modal panels.
    fn draw_l_through_widgets(e: &mut Editor) -> NewDocument {
        let ctx = egui::Context::default();
        let out = frame(&ctx, e, vec![]);
        let at = text_at(&out, "Create sketch + Extrude…");
        click(&ctx, e, at);
        frame(&ctx, e, vec![]); // let the new window choose its initial position
        for [x, y] in [
            [0., 0.],
            [60., 0.],
            [60., 20.],
            [20., 20.],
            [20., 40.],
            [0., 40.],
        ] {
            let out = frame(&ctx, e, vec![]);
            let rect = out
                .shapes
                .iter()
                .find_map(|c| match &c.shape {
                    egui::Shape::Rect(r)
                        if (r.rect.width() - 510.).abs() < 1.
                            && (r.rect.height() - 250.).abs() < 1. =>
                    {
                        Some(r.rect)
                    }
                    _ => None,
                })
                .expect("visible drawing canvas");
            click(
                &ctx,
                e,
                rect.left_bottom() + egui::vec2(35. + 4. * x, -30. - 4. * y),
            );
        }
        assert!(!e.draft.as_ref().expect("draft").closed);
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Close contour"));
        assert!(e.draft.as_ref().expect("draft").closed);
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Undo draft"));
        assert!(!e.draft.as_ref().expect("draft").closed);
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Redo draft"));
        assert!(e.draft.as_ref().expect("draft").closed);
        replace_field(&ctx, e, "10", "12");
        assert_eq!(e.draft.as_ref().expect("draft").height, "12");
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Undo draft"));
        assert_eq!(e.draft.as_ref().expect("draft").height, "10");
        replace_field(&ctx, e, "60.000", "NaN");
        assert!(e.content().is_err());
        assert!(e.take_request().is_none());
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Undo draft"));
        assert!(e.content().is_ok());
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Create in new file…"));
        e.take_request().expect("real button submitted one request")
    }
    #[test]
    fn draft_widgets_submit_exact_mouse_coordinates_with_explicit_closure() {
        let mut e = Editor::default();
        let NewDocument::SketchExtrude(p) = draw_l_through_widgets(&mut e) else {
            panic!("wrong request")
        };
        assert_eq!(
            p.points().iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
            [
                [0., 0.],
                [60., 0.],
                [60., 20.],
                [20., 20.],
                [20., 40.],
                [0., 40.]
            ]
        );
        assert_eq!(p.height_mm(), 10.);
        assert!(e.take_request().is_none());
    }
    /// Opens the window, switches it to Circle and fills the four numbers in
    /// through real widgets, leaving whatever the polygon half held alone.
    fn draw_circle_through_widgets(
        e: &mut Editor,
        center: [&str; 2],
        radius: &str,
        height: &str,
    ) -> egui::Context {
        let ctx = egui::Context::default();
        if !e.active() {
            let out = frame(&ctx, e, vec![]);
            click(&ctx, e, text_at(&out, "Create sketch + Extrude…"));
        }
        // A context that has not drawn this window yet has to lay it out
        // before anything inside it can be found and pressed.
        for _ in 0..3 {
            frame(&ctx, e, vec![]);
        }
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Circle"));
        for (label, value) in [
            ("Center X", center[0]),
            ("Center Y", center[1]),
            ("Radius", radius),
            ("Blind height", height),
        ] {
            let out = frame(&ctx, e, vec![]);
            type_into_grid_row(&ctx, e, &out, label, value);
        }
        ctx
    }

    /// Types into the box on one row of the circle grid.
    ///
    /// The grid's first column is as wide as its widest label, so every box
    /// starts at the same x. Anchoring on that label rather than on each row's
    /// own puts the click inside the box for short labels as well as long
    /// ones.
    fn type_into_grid_row(
        ctx: &egui::Context,
        e: &mut Editor,
        out: &egui::FullOutput,
        label: &str,
        value: &str,
    ) {
        let column = out
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == "Blind height" => {
                    Some(t.visual_bounding_rect().right())
                }
                _ => None,
            })
            .expect("the widest label sets the column");
        replace_box_on_row(ctx, e, out, column, label, value);
    }

    /// Types into the box on one row of the annular grid.
    ///
    /// The column is taken from the widest of *this* grid's own labels rather
    /// than from one label chosen in advance: this form has five of them and
    /// which is widest is a property of the font, not something to assert.
    fn type_into_annulus_row(
        ctx: &egui::Context,
        e: &mut Editor,
        out: &egui::FullOutput,
        label: &str,
        value: &str,
    ) {
        let column = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t)
                    if matches!(
                        t.galley.text(),
                        "Center X" | "Center Y" | "Outer radius" | "Inner radius" | "Blind height"
                    ) =>
                {
                    Some(t.visual_bounding_rect().right())
                }
                _ => None,
            })
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(column.is_finite(), "the annular grid drew no labels");
        replace_box_on_row(ctx, e, out, column, label, value);
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

    /// Opens the window, switches it to the annular profile and fills its five
    /// numbers in through real widgets, leaving the other two drafts alone.
    fn draw_annulus_through_widgets(
        e: &mut Editor,
        center: [&str; 2],
        outer: &str,
        inner: &str,
        height: &str,
    ) -> egui::Context {
        let ctx = egui::Context::default();
        if !e.active() {
            let out = frame(&ctx, e, vec![]);
            click(&ctx, e, text_at(&out, "Create sketch + Extrude…"));
        }
        for _ in 0..3 {
            frame(&ctx, e, vec![]);
        }
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Circle with hole"));
        for (label, value) in [
            ("Center X", center[0]),
            ("Center Y", center[1]),
            ("Outer radius", outer),
            ("Inner radius", inner),
            ("Blind height", height),
        ] {
            let out = frame(&ctx, e, vec![]);
            type_into_annulus_row(&ctx, e, &out, label, value);
        }
        ctx
    }

    #[test]
    fn circle_widgets_submit_exact_numbers_and_keep_the_polygon_draft() {
        let mut e = Editor::default();
        let ctx = egui::Context::default();
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Create sketch + Extrude…"));
        frame(&ctx, &mut e, vec![]);
        // A polygon half-drawn before anyone asked for a circle.
        let points = vec![
            ["0".to_owned(), "0".to_owned()],
            ["60".to_owned(), "0".to_owned()],
            ["0".to_owned(), "40".to_owned()],
        ];
        e.draft.as_mut().expect("draft").points = points.clone();

        let ctx = draw_circle_through_widgets(&mut e, ["12", "-7"], "10", "15");
        assert_eq!(
            e.draft.as_ref().expect("draft").points,
            points,
            "switching mode threw away the polygon draft"
        );
        assert_eq!(e.circle.center, ["12".to_owned(), "-7".to_owned()]);
        assert_eq!(e.circle.radius, "10");
        assert_eq!(e.circle.height, "15");

        // A refusal is shown in the form and submits nothing.
        for bad in ["0", "-4", "banana", ""] {
            let out = frame(&ctx, &mut e, vec![]);
            type_into_grid_row(&ctx, &mut e, &out, "Radius", bad);
            let out = frame(&ctx, &mut e, vec![]);
            assert!(e.circle_content().is_err(), "radius {bad:?} was accepted");
            assert!(
                !out.shapes.iter().any(|c| matches!(&c.shape,
                    egui::Shape::Text(t) if t.galley.text() == "Save circle extrusion…")),
                "radius {bad:?} still offered Save"
            );
            assert!(e.take_request().is_none());
        }

        let ctx = draw_circle_through_widgets(&mut e, ["12", "-7"], "10", "15");
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Save circle extrusion…"));
        let NewDocument::CircleExtrude(c) = e.take_request().expect("one request") else {
            panic!("the circle form asked for something else")
        };
        assert_eq!((c.center().x, c.center().y), (12., -7.));
        assert_eq!((c.radius_mm(), c.height_mm()), (10., 15.));
        assert!(e.take_request().is_none(), "one press, one request");

        // While the submitted request is saving, only the toolbar can cancel
        // the job. Draft actions must not discard the recovery state or change
        // which form will reappear if the worker refuses the publication.
        let before_circle = e.circle.clone();
        for label in ["Cancel draft", "Line polygon", "Save circle extrusion…"] {
            let out = frame_running(&ctx, &mut e, vec![], true);
            let at = text_at(&out, label);
            frame_running(&ctx, &mut e, vec![egui::Event::PointerMoved(at)], true);
            for pressed in [true, false] {
                frame_running(
                    &ctx,
                    &mut e,
                    vec![egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    }],
                    true,
                );
            }
            assert!(e.active(), "{label} discarded a saving circle draft");
            assert_eq!(e.mode, Mode::Circle, "{label} changed a saving profile");
            assert_eq!(e.circle, before_circle);
            assert_eq!(e.draft.as_ref().expect("draft").points, points);
            assert!(e.take_request().is_none(), "{label} submitted twice");
        }

        // Back to the polygon: the points are still there, and so is its own
        // action. Neither draft was ever the other's.
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Line polygon"));
        assert_eq!(e.draft.as_ref().expect("draft").points, points);
        let out = frame(&ctx, &mut e, vec![]);
        text_at(&out, "Close contour");
        // Cancel ends both, and leaves nothing pending.
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Cancel draft"));
        assert!(!e.active());
        assert!(e.take_request().is_none());
    }

    #[test]
    fn annulus_widgets_submit_exact_numbers_and_keep_the_other_drafts() {
        let mut e = Editor::default();
        let ctx = egui::Context::default();
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Create sketch + Extrude…"));
        frame(&ctx, &mut e, vec![]);
        // A polygon half-drawn, and a circle typed, before anyone asked for a
        // hole. Neither may be disturbed by the third form.
        let points = vec![
            ["0".to_owned(), "0".to_owned()],
            ["60".to_owned(), "0".to_owned()],
            ["0".to_owned(), "40".to_owned()],
        ];
        e.draft.as_mut().expect("draft").points = points.clone();
        draw_circle_through_widgets(&mut e, ["1", "2"], "3", "4");
        let circle = e.circle.clone();

        let ctx = draw_annulus_through_widgets(&mut e, ["12", "-7"], "10", "4", "15");
        assert_eq!(e.mode, Mode::Annulus);
        assert_eq!(e.draft.as_ref().expect("draft").points, points);
        assert_eq!(
            e.circle, circle,
            "the circle draft is not the annulus draft"
        );
        assert_eq!(e.annulus.center, ["12".to_owned(), "-7".to_owned()]);
        assert_eq!(e.annulus.outer_radius, "10");
        assert_eq!(e.annulus.inner_radius, "4");
        assert_eq!(e.annulus.height, "15");

        // Every refusal is shown in the form and submits nothing: a hole as
        // big as its boundary, a hole bigger than it, a wall thinner than the
        // policy allows, and a number that is not one.
        for (field, bad) in [
            ("Inner radius", "10"),
            ("Inner radius", "12"),
            ("Inner radius", "9.9999"),
            ("Inner radius", "0"),
            ("Inner radius", "-4"),
            ("Inner radius", "banana"),
            ("Outer radius", "0"),
            ("Blind height", "0"),
        ] {
            let out = frame(&ctx, &mut e, vec![]);
            type_into_annulus_row(&ctx, &mut e, &out, field, bad);
            let out = frame(&ctx, &mut e, vec![]);
            assert!(e.annulus_content().is_err(), "{field} {bad:?} was accepted");
            assert!(
                !out.shapes.iter().any(|c| matches!(&c.shape,
                    egui::Shape::Text(t) if t.galley.text() == "Save annular extrusion…")),
                "{field} {bad:?} still offered Save"
            );
            assert!(e.take_request().is_none());
            // Put the form back to something that submits before the next one.
            let out = frame(&ctx, &mut e, vec![]);
            type_into_annulus_row(
                &ctx,
                &mut e,
                &out,
                field,
                match field {
                    "Inner radius" => "4",
                    "Outer radius" => "10",
                    _ => "15",
                },
            );
        }

        // An empty box cannot be reached by selecting and replacing, so it is
        // emptied directly. The form has to refuse it like any other
        // non-number rather than reading it as a zero.
        e.annulus.inner_radius.clear();
        assert!(e.annulus_content().is_err(), "an empty radius was accepted");
        let out = frame(&ctx, &mut e, vec![]);
        assert!(
            !out.shapes.iter().any(|c| matches!(&c.shape,
                egui::Shape::Text(t) if t.galley.text() == "Save annular extrusion…")),
            "an empty radius still offered Save"
        );
        assert!(e.take_request().is_none());

        let ctx = draw_annulus_through_widgets(&mut e, ["12", "-7"], "10", "4", "15");
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Save annular extrusion…"));
        let NewDocument::AnnularExtrude(a) = e.take_request().expect("one request") else {
            panic!("the annular form asked for something else")
        };
        assert_eq!((a.center().x, a.center().y), (12., -7.));
        assert_eq!(a.outer_radius_mm(), 10.);
        assert_eq!(a.inner_radius_mm(), 4.);
        assert_eq!(a.height_mm(), 15.);
        assert!(e.take_request().is_none(), "one press, one request");

        // While that request is saving, only the toolbar can cancel the job.
        let before = e.annulus.clone();
        for label in ["Cancel draft", "Circle", "Save annular extrusion…"] {
            let out = frame_running(&ctx, &mut e, vec![], true);
            let at = text_at(&out, label);
            frame_running(&ctx, &mut e, vec![egui::Event::PointerMoved(at)], true);
            for pressed in [true, false] {
                frame_running(
                    &ctx,
                    &mut e,
                    vec![egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    }],
                    true,
                );
            }
            assert!(e.active(), "{label} discarded a saving annular draft");
            assert_eq!(e.mode, Mode::Annulus, "{label} changed a saving profile");
            assert_eq!(e.annulus, before);
            assert_eq!(e.circle, circle);
            assert_eq!(e.draft.as_ref().expect("draft").points, points);
            assert!(e.take_request().is_none(), "{label} submitted twice");
        }

        // Back through the other two: both are still exactly as they were.
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Circle"));
        assert_eq!(e.circle, circle);
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Line polygon"));
        assert_eq!(e.draft.as_ref().expect("draft").points, points);
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Circle with hole"));
        assert_eq!(e.annulus, before);
        // Cancel ends all three, and leaves nothing pending.
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Cancel draft"));
        assert!(!e.active());
        assert!(e.take_request().is_none());
    }

    /// The window's worker and the shipped command publish the same part.
    #[test]
    fn native_annulus_draft_and_cli_publish_equivalent_models() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for annulus UI worker");
            return;
        }
        use crate::creates::{
            self,
            tests::{ferritecad, read_semantics},
        };
        use std::sync::mpsc;
        let d = tempfile::tempdir().expect("dir");
        let ui = d.path().join("annulus-ui.fcad");
        let cli = d.path().join("annulus-cli.fcad");
        let input = d.path().join("annulus-request.json");
        let mut creates = creates::Creates::default();
        draw_annulus_through_widgets(&mut creates.sketch, ["12", "-7"], "10", "4", "15");
        let content = creates
            .sketch
            .annulus_content()
            .expect("the widgets describe an annulus");
        let before = creates.sketch.annulus.clone();
        let mut view = ferritecad_ui::ViewportInput::new();
        let loads = crate::Loads::default();
        let exports = crate::exports::Exports::default();

        // A cancelled save dialog keeps the numbers that were typed.
        assert!(
            crate::start_new(
                &mut creates,
                &loads,
                &exports,
                &mut view,
                content.clone(),
                None,
                |_, _, _, _| panic!("no worker on cancel")
            )
            .is_none()
        );
        assert_eq!(creates.sketch.annulus, before);
        // So does a destination that is already taken.
        let busy = d.path().join("occupied.fcad");
        std::fs::write(&busy, b"keep").expect("busy");
        let (_, open) = creates::tests::run_to_completion(
            &mut creates,
            &mut view,
            content.clone(),
            Some(busy.clone()),
        );
        assert!(open.is_none());
        assert_eq!(creates.sketch.annulus, before);
        assert_eq!(std::fs::read(busy).expect("busy"), b"keep");

        let (tx, rx) = mpsc::channel();
        let spawn = move |path: &std::path::Path,
                          content,
                          generation,
                          cancel: &ferritecad_kernel::CancelToken| {
            let path = path.to_path_buf();
            let ctx = ferritecad_kernel::OperationContext::default().with_cancel(cancel.clone());
            creates::spawn_create(
                move || creates::run_create(&path, content, &ctx),
                move |result| tx.send((generation, result)).expect("reply"),
            )
        };
        crate::start_new(
            &mut creates,
            &loads,
            &exports,
            &mut view,
            content.clone(),
            Some(ui.clone()),
            spawn,
        )
        .expect("worker");
        let (generation, result) = rx.recv().expect("worker result");
        assert_eq!(
            creates::finish_create(&mut creates, &mut view, generation, result),
            Some(ui.clone())
        );
        assert!(!creates.sketch.active());
        // Publication must retain the draft until the async Open is accepted.
        creates
            .sketch
            .draft_load_finished(Path::new("unrelated.fcad"), false);
        assert!(!creates.sketch.active());
        creates.sketch.draft_load_finished(&ui, false);
        assert!(
            creates.sketch.active(),
            "failed Open must restore the published draft"
        );
        assert_eq!(creates.sketch.annulus, before);
        assert_eq!(creates.sketch.annulus_content().expect("restored"), content);
        assert!(
            creates.sketch.take_request().is_none(),
            "restoring must not resubmit"
        );
        creates.sketch.draft_published(&ui);
        creates.sketch.draft_load_finished(&ui, true);
        assert!(!creates.sketch.active());
        assert!(creates.sketch.published_draft.is_none());
        // A reply for a request that is no longer current changes nothing.
        assert!(
            creates::finish_create(
                &mut creates,
                &mut view,
                generation,
                Err(ferritecad_types::CadError::kernel("stale")),
            )
            .is_none()
        );
        creates.stop_all();

        std::fs::write(
            &input,
            concat!(
                r#"{"schema_version":1,"center_mm":[12.0,-7.0],"#,
                r#""outer_radius_mm":10.0,"inner_radius_mm":4.0,"height_mm":15.0}"#
            ),
        )
        .expect("request");
        let run = std::process::Command::new(ferritecad())
            .arg("create-annular-extrude")
            .arg(input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(run.status.success(), "{run:?}");

        // The same model, said in two independently minted sets of UUIDs.
        assert_eq!(read_semantics(&ui).0, read_semantics(&cli).0);
        assert_ne!(read_semantics(&ui).1, read_semantics(&cli).1);
        let mut bytes = Vec::new();
        for path in [&ui, &cli] {
            for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
                let out = path.with_extension(extension);
                let result = std::process::Command::new(ferritecad())
                    .arg(op)
                    .arg(path)
                    .arg("-o")
                    .arg(&out)
                    .output()
                    .expect("export");
                assert!(result.status.success(), "{result:?}");
                if extension == "stl" {
                    bytes.push(std::fs::read(&out).expect("STL"));
                } else if let Some(dir) = std::env::var_os("FCAD_ANNULUS_ARTIFACTS") {
                    // Read by the pinned ufbx reader in the same CI job. FBX
                    // identity properties carry each document's own UUIDs, so
                    // these two files are read rather than compared.
                    let dir = std::path::Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("artifact directory");
                    std::fs::copy(&out, dir.join(out.file_name().expect("name")))
                        .expect("artifact");
                }
            }
        }
        assert_eq!(bytes[0], bytes[1], "one geometry, two documents");
    }

    /// A real source document with one saved analytic circle, and the accepted
    /// reading of it that a form is allowed to take its facts from.
    fn saved_circle(root: &Path) -> (PathBuf, ferritecad_document::ExtrudeEditSource) {
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
        (source, loaded.edit_source.expect("accepted edit facts"))
    }

    #[test]
    fn circle_edit_widgets_change_only_the_two_numbers_and_keep_the_draft() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for the accepted scene this form reads");
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let (source, reading) = saved_circle(root.path());
        let saved = reading.circle_sketches[0]
            .circle
            .clone()
            .expect("a supported circle");
        let id = reading.circle_sketches[0].sketch;

        let mut e = Editor::default();
        assert!(
            !e.begin_circle_edit(&source, &reading, ferritecad_types::ObjectId::new()),
            "a Sketch this document does not have"
        );
        assert!(!e.active());
        assert!(e.begin_circle_edit(&source, &reading, id));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        // The form opens on what is stored, and says which circle it is.
        assert_eq!(e.circle.center, ["12".to_owned(), "-7".to_owned()]);
        assert_eq!(e.circle.radius, "10");
        assert_eq!(e.circle.height, "15");
        let out = frame(&ctx, &mut e, vec![]);
        assert!(
            out.shapes.iter().any(|c| matches!(&c.shape,
                egui::Shape::Text(t) if t.galley.text().contains(&saved.curve_id.to_string()))),
            "the saved circle is named by its own UUID"
        );
        assert!(
            out.shapes.iter().any(|c| matches!(&c.shape,
                egui::Shape::Text(t) if t.galley.text().contains(&id.to_string()))),
            "so is the Sketch that holds it"
        );

        // All three fields can change over many frames before one Apply.
        let out = frame(&ctx, &mut e, vec![]);
        type_into_grid_row(&ctx, &mut e, &out, "Center X", "-3.5");
        let out = frame(&ctx, &mut e, vec![]);
        type_into_grid_row(&ctx, &mut e, &out, "Center Y", "4.25");
        let out = frame(&ctx, &mut e, vec![]);
        type_into_grid_row(&ctx, &mut e, &out, "Radius", "6.75");
        assert_eq!(e.circle.center, ["-3.5".to_owned(), "4.25".to_owned()]);
        assert_eq!(e.circle.radius, "6.75");
        assert_eq!(
            e.circle.height, "15",
            "the height is not this form's to change"
        );

        assert!(e.circle_undo.is_empty(), "typing is not an Apply");
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Save edited circle copy…"));
        assert!(
            e.take_circle_edit_request().is_none(),
            "unapplied numbers cannot publish"
        );
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Apply circle change"));
        assert_eq!(e.circle_undo.len(), 1, "one Apply, one history step");
        // Undo and redo walk the whole confirmed change.
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Undo draft"));
        assert_eq!(e.circle.radius, "10");
        assert_eq!(e.circle.center, ["12", "-7"]);
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Redo draft"));
        assert_eq!(e.circle.radius, "6.75");

        // A number the document will not store is refused in the form, and
        // offers nothing to save.
        for bad in ["0", "-2", "banana", ""] {
            let out = frame(&ctx, &mut e, vec![]);
            type_into_grid_row(&ctx, &mut e, &out, "Radius", bad);
            let out = frame(&ctx, &mut e, vec![]);
            assert!(e.circle_edit_request().is_err(), "radius {bad:?}");
            assert!(
                !out.shapes.iter().any(|c| matches!(&c.shape,
                    egui::Shape::Text(t) if t.galley.text() == "Save edited circle copy…")),
                "radius {bad:?} still offered Save"
            );
            assert!(e.take_circle_edit_request().is_none());
            assert!(e.active(), "a refusal keeps the draft");
        }
        let out = frame(&ctx, &mut e, vec![]);
        type_into_grid_row(&ctx, &mut e, &out, "Radius", "6.75");

        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Save edited circle copy…"));
        let request = e.take_circle_edit_request().expect("submit");
        assert!(
            e.take_circle_edit_request().is_none(),
            "one press, one request"
        );
        assert_eq!(request.source, source, "the accepted scene's own path");
        assert_eq!(request.expected, reading.version);
        assert_eq!(request.sketch, id);
        assert_eq!(
            request.edit.curve_id, saved.curve_id,
            "the saved identity, not a number read back out of a box"
        );
        assert_eq!(request.edit.center_mm, [-3.5, 4.25]);
        assert_eq!(request.edit.radius_mm, 6.75);
        // A dialog that answers nothing leaves the same draft to try again.
        assert!(e.active());
        assert_eq!(e.circle.radius, "6.75");

        // While a job is running the form is disabled: pressing Save again
        // makes no second request, typing changes nothing, and the draft and
        // its history stay exactly as they were.
        let kept = e.circle.clone();
        let history = (e.circle_undo.clone(), e.circle_redo.clone());
        frame_running(&ctx, &mut e, vec![], true);
        let out = frame_running(&ctx, &mut e, vec![], true);
        let save = text_at(&out, "Save edited circle copy…");
        for pressed in [true, false] {
            frame_running(
                &ctx,
                &mut e,
                vec![egui::Event::PointerButton {
                    pos: save,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
                true,
            );
        }
        assert!(
            e.take_circle_edit_request().is_none(),
            "a running job takes no second request"
        );
        assert_eq!(e.circle, kept, "and loses nothing that was typed");
        assert_eq!((e.circle_undo.clone(), e.circle_redo.clone()), history);
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Cancel draft"));
        assert!(!e.active(), "Cancel ends the draft");
        assert!(e.take_circle_edit_request().is_none());
    }

    #[test]
    fn native_circle_edit_worker_and_cli_publish_equivalent_copies() {
        use crate::creates::tests::{ferritecad, read_semantics};
        use ferritecad_document::Document;
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for the saved Circle worker");
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let (source, reading) = saved_circle(root.path());
        let bytes = std::fs::read(&source).expect("source");
        let modified = std::fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("mtime");
        let id = reading.circle_sketches[0].sketch;
        let curve = reading.circle_sketches[0]
            .circle
            .as_ref()
            .expect("supported")
            .curve_id;

        let mut e = Editor::default();
        assert!(e.begin_circle_edit(&source, &reading, id));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        for (label, value) in [
            ("Center X", "-3.5"),
            ("Center Y", "4.25"),
            ("Radius", "6.75"),
        ] {
            let out = frame(&ctx, &mut e, vec![]);
            type_into_grid_row(&ctx, &mut e, &out, label, value);
        }
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Apply circle change"));
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Save edited circle copy…"));
        let request = e.take_circle_edit_request().expect("submit");
        let kept = e.circle.clone();

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
                .start_circle(request.clone(), move |r, g, c| {
                    crate::edits::spawn_circle_edit(r, c, move |result| {
                        tx.send((g, result)).expect("reply")
                    })
                })
                .expect("worker");
            assert!(
                edits
                    .start_circle(request, |_, _, _| panic!("duplicate worker"))
                    .is_none(),
                "a running job takes no second request"
            );
            // A reply for a request that is no longer current changes nothing.
            assert!(
                finish_circle_edit(
                    &mut e,
                    &mut edits,
                    generation + 1,
                    Err(CadError::input("stale response"))
                )
                .is_none()
            );
            assert_eq!(e.circle, kept);
            let (g, result) = rx.recv().expect("completed");
            let path = finish_circle_edit(&mut e, &mut edits, g, result);
            if occupied {
                assert!(path.is_none(), "a taken destination publishes nothing");
                assert_eq!(e.circle, kept, "and keeps the draft to retry with");
                assert_eq!(
                    std::fs::read(root.path().join("occupied.fcad")).expect("sentinel"),
                    b"keep"
                );
            } else {
                assert_eq!(path, Some(root.path().join("ui.fcad")));
                assert!(!e.active(), "published draft must not block async Open");
                e.draft_load_finished(Path::new("another.fcad"), false);
                assert!(!e.active(), "an unrelated load failure cannot restore it");
                e.draft_load_finished(&root.path().join("ui.fcad"), false);
                assert!(e.active(), "failed preparation restores the draft");
                assert_eq!(e.circle, kept);
                assert_eq!(e.circle_undo.len(), 1, "history survives failed Open");
                e.draft_published(&root.path().join("ui.fcad"));
                e.draft_load_finished(&root.path().join("ui.fcad"), true);
                assert!(!e.active());
                assert!(e.published_draft.is_none());
            }
        }

        // The peer CLI applies the same request to the same source.
        let input = root.path().join("edit.json");
        std::fs::write(
            &input,
            format!(
                r#"{{"request_version":1,"curve_id":"{curve}","center_mm":[{},{}],"radius_mm":{}}}"#,
                request.edit.center_mm[0], request.edit.center_mm[1], request.edit.radius_mm
            ),
        )
        .expect("request");
        let cli = root.path().join("cli.fcad");
        let p = std::process::Command::new(ferritecad())
            .arg("edit-circle")
            .arg(&source)
            .arg("--sketch")
            .arg(id.to_string())
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
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
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
    fn saved_annulus(root: &Path) -> (PathBuf, ferritecad_document::ExtrudeEditSource) {
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
        (source, loaded.edit_source.expect("accepted edit facts"))
    }

    #[test]
    fn annulus_edit_widgets_change_only_the_three_numbers_and_keep_the_draft() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for the accepted scene this form reads");
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let (source, reading) = saved_annulus(root.path());
        let saved = reading.annulus_sketches[0]
            .annulus
            .clone()
            .expect("a supported annulus");
        let id = reading.annulus_sketches[0].sketch;

        let mut e = Editor::default();
        // A Sketch this document does not have cannot be edited.
        assert!(!e.begin_annulus_edit(&source, &reading, ferritecad_types::ObjectId::new()));
        assert!(!e.active());
        assert!(e.begin_annulus_edit(&source, &reading, id));
        // And a second begin on a live draft is refused rather than retargeting.
        assert!(!e.begin_annulus_edit(&source, &reading, id));

        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        // The form opens on what is stored, and says which two circles it is.
        assert_eq!(e.annulus.center, ["12".to_owned(), "-7".to_owned()]);
        assert_eq!(e.annulus.outer_radius, "10");
        assert_eq!(e.annulus.inner_radius, "4");
        assert_eq!(e.annulus.height, "15");
        let out = frame(&ctx, &mut e, vec![]);
        let drawn: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        for needle in [
            saved.outer_curve_id.to_string(),
            saved.inner_curve_id.to_string(),
            id.to_string(),
        ] {
            assert!(
                drawn.iter().any(|line| line.contains(&needle)),
                "the form does not name {needle}: {drawn:?}"
            );
        }
        assert!(
            drawn.iter().any(|l| l.contains("mm (retained)")),
            "the height is shown as retained rather than editable"
        );

        for (label, value) in [
            ("Center X", "-3.5"),
            ("Center Y", "4.25"),
            ("Outer radius", "6.75"),
            ("Inner radius", "2.125"),
        ] {
            let out = frame(&ctx, &mut e, vec![]);
            type_into_annulus_row(&ctx, &mut e, &out, label, value);
        }
        assert_eq!(e.annulus.center, ["-3.5".to_owned(), "4.25".to_owned()]);
        assert_eq!(e.annulus.outer_radius, "6.75");
        assert_eq!(e.annulus.inner_radius, "2.125");
        assert_eq!(
            e.annulus.height, "15",
            "the height is not this form's to change"
        );
        assert!(e.annulus_undo.is_empty(), "typing is not an Apply");

        // Save is unavailable until the numbers are applied, and submits nothing.
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Save edited annulus copy…"));
        assert!(
            e.take_annulus_edit_request().is_none(),
            "an unapplied draft submitted a request"
        );

        // One Apply is one history step for all three numbers together.
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Apply annulus change"));
        assert_eq!(e.annulus_undo.len(), 1, "one Apply, one history step");
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Undo draft"));
        assert_eq!(e.annulus.outer_radius, "10");
        assert_eq!(e.annulus.inner_radius, "4");
        assert_eq!(e.annulus.center, ["12", "-7"]);
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Redo draft"));
        assert_eq!(e.annulus.outer_radius, "6.75");
        assert_eq!(e.annulus.inner_radius, "2.125");
        assert!(
            e.take_annulus_edit_request().is_none(),
            "undo and redo ask no job for anything"
        );

        // Every refusal is shown in the form and offers no Save.
        for (field, bad) in [
            ("Inner radius", "10"),
            ("Inner radius", "12"),
            ("Inner radius", "9.9999"),
            ("Inner radius", "0"),
            ("Inner radius", "-4"),
            ("Inner radius", "banana"),
            ("Outer radius", "0"),
            ("Outer radius", "2e6"),
            ("Center X", "banana"),
        ] {
            let out = frame(&ctx, &mut e, vec![]);
            type_into_annulus_row(&ctx, &mut e, &out, field, bad);
            let out = frame(&ctx, &mut e, vec![]);
            assert!(
                e.annulus_edit_request().is_err(),
                "{field} {bad:?} was accepted"
            );
            assert!(
                !out.shapes.iter().any(|c| matches!(&c.shape,
                    egui::Shape::Text(t) if t.galley.text() == "Save edited annulus copy…")),
                "{field} {bad:?} still offered Save"
            );
            assert!(e.take_annulus_edit_request().is_none());
            let out = frame(&ctx, &mut e, vec![]);
            type_into_annulus_row(
                &ctx,
                &mut e,
                &out,
                field,
                match field {
                    "Inner radius" => "2.125",
                    "Outer radius" => "6.75",
                    _ => "-3.5",
                },
            );
        }

        // Back to a request, and the identities come from the reading rather
        // than from anything typed.
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Apply annulus change"));
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Save edited annulus copy…"));
        let request = e.take_annulus_edit_request().expect("submit");
        assert!(
            e.take_annulus_edit_request().is_none(),
            "one press, one request"
        );
        assert_eq!(request.sketch, id);
        assert_eq!(request.source, source);
        assert_eq!(request.expected, reading.version);
        assert_eq!(request.edit.outer_curve_id, saved.outer_curve_id);
        assert_eq!(request.edit.inner_curve_id, saved.inner_curve_id);
        assert_eq!(request.edit.center_mm, [-3.5, 4.25]);
        assert_eq!(request.edit.outer_radius_mm, 6.75);
        assert_eq!(request.edit.inner_radius_mm, 2.125);

        // While the request is saving, the form is disabled and no draft action
        // discards it.
        let kept = e.annulus.clone();
        let history = (e.annulus_undo.clone(), e.annulus_redo.clone());
        for label in ["Cancel draft", "Save edited annulus copy…", "Undo draft"] {
            let out = frame_running(&ctx, &mut e, vec![], true);
            let at = text_at(&out, label);
            frame_running(&ctx, &mut e, vec![egui::Event::PointerMoved(at)], true);
            for pressed in [true, false] {
                frame_running(
                    &ctx,
                    &mut e,
                    vec![egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    }],
                    true,
                );
            }
            assert!(e.active(), "{label} discarded a saving annulus draft");
            assert_eq!(e.annulus, kept, "{label}");
            assert_eq!(
                (e.annulus_undo.clone(), e.annulus_redo.clone()),
                history,
                "{label} changed the history"
            );
            assert!(e.take_annulus_edit_request().is_none(), "{label} submitted");
        }

        // Cancel ends the draft and leaves nothing pending.
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Cancel draft"));
        assert!(!e.active());
        assert!(e.take_annulus_edit_request().is_none());
    }

    #[test]
    fn native_annulus_edit_worker_and_cli_publish_equivalent_copies() {
        use crate::creates::tests::{ferritecad, read_semantics};
        use ferritecad_document::Document;
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for the saved annulus worker");
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let (source, reading) = saved_annulus(root.path());
        let bytes = std::fs::read(&source).expect("source");
        let modified = std::fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("mtime");
        let id = reading.annulus_sketches[0].sketch;
        let saved = reading.annulus_sketches[0]
            .annulus
            .clone()
            .expect("supported");

        let mut e = Editor::default();
        assert!(e.begin_annulus_edit(&source, &reading, id));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        for (label, value) in [
            ("Center X", "-3.5"),
            ("Center Y", "4.25"),
            ("Outer radius", "6.75"),
            ("Inner radius", "2.125"),
        ] {
            let out = frame(&ctx, &mut e, vec![]);
            type_into_annulus_row(&ctx, &mut e, &out, label, value);
        }
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Apply annulus change"));
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Save edited annulus copy…"));
        let request = e.take_annulus_edit_request().expect("submit");
        let kept = e.annulus.clone();

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
                .start_annulus(request.clone(), move |r, g, c| {
                    crate::edits::spawn_annulus_edit(r, c, move |result| {
                        tx.send((g, result)).expect("reply")
                    })
                })
                .expect("worker");
            assert!(
                edits
                    .start_annulus(request, |_, _, _| panic!("duplicate worker"))
                    .is_none(),
                "a running job takes no second request"
            );
            // A reply for a request that is no longer current changes nothing.
            assert!(
                finish_annulus_edit(
                    &mut e,
                    &mut edits,
                    generation + 1,
                    Err(CadError::input("stale response"))
                )
                .is_none()
            );
            assert_eq!(e.annulus, kept);
            let (g, result) = rx.recv().expect("completed");
            let path = finish_annulus_edit(&mut e, &mut edits, g, result);
            if occupied {
                assert!(path.is_none(), "a taken destination publishes nothing");
                assert_eq!(e.annulus, kept, "and keeps the draft to retry with");
                assert_eq!(
                    std::fs::read(root.path().join("occupied.fcad")).expect("sentinel"),
                    b"keep"
                );
            } else {
                assert_eq!(path, Some(root.path().join("ui.fcad")));
                assert!(!e.active(), "published draft must not block async Open");
                e.draft_load_finished(Path::new("another.fcad"), false);
                assert!(!e.active(), "an unrelated load failure cannot restore it");
                e.draft_load_finished(&root.path().join("ui.fcad"), false);
                assert!(e.active(), "failed preparation restores the draft");
                assert_eq!(e.annulus, kept);
                assert_eq!(e.annulus_undo.len(), 1, "history survives failed Open");
                e.draft_published(&root.path().join("ui.fcad"));
                e.draft_load_finished(&root.path().join("ui.fcad"), true);
                assert!(!e.active());
                assert!(e.published_draft.is_none());
            }
        }

        // The peer CLI applies the same request to the same source.
        let input = root.path().join("edit.json");
        std::fs::write(
            &input,
            format!(
                concat!(
                    r#"{{"request_version":1,"outer_curve_id":"{}","inner_curve_id":"{}","#,
                    r#""center_mm":[{},{}],"outer_radius_mm":{},"inner_radius_mm":{}}}"#
                ),
                saved.outer_curve_id,
                saved.inner_curve_id,
                request.edit.center_mm[0],
                request.edit.center_mm[1],
                request.edit.outer_radius_mm,
                request.edit.inner_radius_mm
            ),
        )
        .expect("request");
        let cli = root.path().join("cli.fcad");
        let p = std::process::Command::new(ferritecad())
            .arg("edit-annular")
            .arg(&source)
            .arg("--sketch")
            .arg(id.to_string())
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
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
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

    #[test]
    fn native_circle_draft_and_cli_publish_equivalent_models() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for circle UI worker");
            return;
        }
        use crate::creates::{
            self,
            tests::{ferritecad, read_semantics},
        };
        use std::sync::mpsc;
        let d = tempfile::tempdir().expect("dir");
        let ui = d.path().join("circle-ui.fcad");
        let cli = d.path().join("circle-cli.fcad");
        let input = d.path().join("circle-request.json");
        let mut creates = creates::Creates::default();
        draw_circle_through_widgets(&mut creates.sketch, ["12", "-7"], "10", "15");
        let content = creates
            .sketch
            .circle_content()
            .expect("the widgets describe a circle");
        let before = creates.sketch.circle.clone();
        let mut view = ferritecad_ui::ViewportInput::new();
        let loads = crate::Loads::default();
        let exports = crate::exports::Exports::default();

        // A cancelled save dialog keeps the numbers that were typed.
        assert!(
            crate::start_new(
                &mut creates,
                &loads,
                &exports,
                &mut view,
                content.clone(),
                None,
                |_, _, _, _| panic!("no worker on cancel")
            )
            .is_none()
        );
        assert_eq!(creates.sketch.circle, before);
        // So does a destination that is already taken.
        let busy = d.path().join("occupied.fcad");
        std::fs::write(&busy, b"keep").expect("busy");
        let (_, open) = creates::tests::run_to_completion(
            &mut creates,
            &mut view,
            content.clone(),
            Some(busy.clone()),
        );
        assert!(open.is_none());
        assert_eq!(creates.sketch.circle, before);
        assert_eq!(std::fs::read(busy).expect("busy"), b"keep");

        let (tx, rx) = mpsc::channel();
        let spawn = move |path: &std::path::Path,
                          content,
                          generation,
                          cancel: &ferritecad_kernel::CancelToken| {
            let path = path.to_path_buf();
            let ctx = ferritecad_kernel::OperationContext::default().with_cancel(cancel.clone());
            creates::spawn_create(
                move || creates::run_create(&path, content, &ctx),
                move |result| tx.send((generation, result)).expect("reply"),
            )
        };
        crate::start_new(
            &mut creates,
            &loads,
            &exports,
            &mut view,
            content.clone(),
            Some(ui.clone()),
            spawn,
        )
        .expect("worker");
        let (generation, result) = rx.recv().expect("worker result");
        assert_eq!(
            creates::finish_create(&mut creates, &mut view, generation, result),
            Some(ui.clone())
        );
        assert!(!creates.sketch.active());
        // A reply for a request that is no longer current changes nothing.
        assert!(
            creates::finish_create(
                &mut creates,
                &mut view,
                generation,
                Err(ferritecad_types::CadError::kernel("stale")),
            )
            .is_none()
        );
        creates.stop_all();

        std::fs::write(
            &input,
            r#"{"schema_version":1,"center_mm":[12.0,-7.0],"radius_mm":10.0,"height_mm":15.0}"#,
        )
        .expect("request");
        let run = std::process::Command::new(ferritecad())
            .arg("create-circle-extrude")
            .arg(input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(run.status.success(), "{run:?}");

        // The same model, said in two independently minted sets of UUIDs.
        assert_eq!(read_semantics(&ui).0, read_semantics(&cli).0);
        assert_ne!(read_semantics(&ui).1, read_semantics(&cli).1);
        let mut bytes = Vec::new();
        for path in [&ui, &cli] {
            for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
                let out = path.with_extension(extension);
                let result = std::process::Command::new(ferritecad())
                    .arg(op)
                    .arg(path)
                    .arg("-o")
                    .arg(&out)
                    .output()
                    .expect("export");
                assert!(result.status.success(), "{result:?}");
                if extension == "stl" {
                    bytes.push(std::fs::read(&out).expect("STL"));
                } else if let Some(dir) = std::env::var_os("FCAD_CIRCLE_ARTIFACTS") {
                    // Read by the pinned ufbx reader in the same CI job. FBX
                    // identity properties carry each document's own UUIDs, so
                    // these two files are read rather than compared.
                    let dir = std::path::Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("artifact directory");
                    std::fs::copy(&out, dir.join(out.file_name().expect("name")))
                        .expect("artifact");
                }
            }
        }
        assert_eq!(bytes[0], bytes[1], "one geometry, two documents");
    }

    #[test]
    fn native_sketch_draft_and_cli_publish_equivalent_models() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for sketch UI worker");
            return;
        }
        use crate::creates::{
            self,
            tests::{ferritecad, read_semantics},
        };
        use std::sync::mpsc;
        let d = tempfile::tempdir().expect("dir");
        let ui = d.path().join("ui.fcad");
        let cli = d.path().join("cli.fcad");
        let input = d.path().join("request.json");
        let mut creates = creates::Creates::default();
        let content = draw_l_through_widgets(&mut creates.sketch);
        let before = creates.sketch.draft.clone();
        let mut view = ferritecad_ui::ViewportInput::new();
        let loads = crate::Loads::default();
        let exports = crate::exports::Exports::default();
        // Cancelled/failed save dialogs both provide no destination; the exact
        // post-dialog UI route must leave the draft and accepted scene alone.
        assert!(
            crate::start_new(
                &mut creates,
                &loads,
                &exports,
                &mut view,
                content.clone(),
                None,
                |_, _, _, _| panic!("no worker on cancel")
            )
            .is_none()
        );
        assert_eq!(creates.sketch.draft, before);
        let busy = d.path().join("occupied.fcad");
        std::fs::write(&busy, b"keep").expect("busy");
        let (_, open) = creates::tests::run_to_completion(
            &mut creates,
            &mut view,
            content.clone(),
            Some(busy.clone()),
        );
        assert!(open.is_none());
        assert_eq!(creates.sketch.draft, before);
        assert_eq!(std::fs::read(busy).expect("busy"), b"keep");
        let (tx, rx) = mpsc::channel();
        let spawn = move |path: &std::path::Path,
                          content,
                          generation,
                          cancel: &ferritecad_kernel::CancelToken| {
            let path = path.to_path_buf();
            let ctx = ferritecad_kernel::OperationContext::default().with_cancel(cancel.clone());
            creates::spawn_create(
                move || creates::run_create(&path, content, &ctx),
                move |result| tx.send((generation, result)).expect("reply"),
            )
        };
        crate::start_new(
            &mut creates,
            &loads,
            &exports,
            &mut view,
            content.clone(),
            Some(ui.clone()),
            spawn,
        )
        .expect("worker");
        assert!(
            crate::start_new(
                &mut creates,
                &loads,
                &exports,
                &mut view,
                content,
                Some(ui.clone()),
                |_, _, _, _| panic!("duplicate worker")
            )
            .is_none()
        );
        let (generation, result) = rx.recv().expect("worker result");
        assert_eq!(
            creates::finish_create(&mut creates, &mut view, generation, result),
            Some(ui.clone())
        );
        assert!(!creates.sketch.active());
        creates.stop_all();
        std::fs::write(&input,r#"{"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10}"#).expect("request");
        let run = std::process::Command::new(ferritecad())
            .arg("create-sketch-extrude")
            .arg(input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(run.status.success(), "{run:?}");
        assert_eq!(read_semantics(&ui).0, read_semantics(&cli).0);
        assert_ne!(read_semantics(&ui).1, read_semantics(&cli).1);
        // Independent UUIDs must not leak into the geometric comparison.
        let mut bytes = Vec::new();
        for path in [&ui, &cli] {
            let out = path.with_extension("stl");
            let result = std::process::Command::new(ferritecad())
                .arg("export-stl")
                .arg(path)
                .arg("-o")
                .arg(&out)
                .output()
                .expect("export");
            assert!(result.status.success(), "{result:?}");
            bytes.push(std::fs::read(out).expect("STL"));
        }
        assert_eq!(bytes[0], bytes[1]);
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
                e.draw_choices(ui, true, Some(path), Some(source));
                e.draw(ui, true, false);
                crate::edits::Edits::default().draw(ui, true, source.unavailable_reason());
            },
        );
        out.textures_delta.clear();
        out
    }
    fn open_base_from_full_document_ui(
        ctx: &egui::Context,
        e: &mut Editor,
        path: &Path,
        source: &ExtrudeEditSource,
        id: ferritecad_types::ObjectId,
    ) {
        let row = source
            .sketches
            .iter()
            .find(|s| s.sketch == id)
            .expect("base");
        let label = format!(
            "Edit Sketch {} — {}…",
            row.name.as_deref().unwrap_or("Unnamed"),
            id
        );
        let mut at = None;
        for step in 0..100 {
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
            for _ in 0..30 {
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
        let at = at.expect("16-Cut base action must be reachable in full viewport");
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
        assert!(e.active(), "base action starts coordinate editor");
        for _ in 0..3 {
            document_frame(ctx, e, path, source, vec![]);
        }
        let out = document_frame(ctx, e, path, source, vec![]);
        for label in [
            "Undo draft",
            "Redo draft",
            "Cancel draft",
            "Save edited copy…",
            "Blind height mm",
        ] {
            let at = text_at(&out, label);
            assert!(
                at.x >= 0. && at.x < 988. && at.y >= 0. && at.y < 768.,
                "off-screen {label}"
            );
        }
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
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for saved Sketch worker");
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
            let body = ExtrudeEditSource::read(&d).expect("catalog").cut_bodies[0].body;
            for i in 0..cuts {
                let slot = (i * 7) % 16;
                let p = ferritecad_document::prepare_circular_cut(
                    &d,
                    body,
                    &ferritecad_document::CircularCut {
                        center_mm: [
                            if i + 1 == cuts {
                                55.
                            } else {
                                6. + (slot % 4) as f64 * 14.
                            },
                            5. + (slot / 4) as f64 * 9.,
                        ],
                        radius_mm: 1. + (i % 3) as f64 * 0.1,
                        depth_mm: if i % 2 == 0 { 10. } else { 3. + (i % 4) as f64 },
                    },
                )
                .expect("prepare");
                d.write_circular_cut(&p).expect("cut");
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
        let reading = loaded.edit_source.expect("accepted edit facts");
        let id = reading
            .sketches
            .iter()
            .find(|s| s.refusal.is_none())
            .expect("editable base")
            .sketch;
        let mut e = Editor::default();
        assert!(!e.begin_edit(&source, &reading, ferritecad_types::ObjectId::new()));
        assert!(!e.active());
        let ctx = egui::Context::default();
        if cuts == 16 {
            open_base_from_full_document_ui(&ctx, &mut e, &source, &reading, id);
        } else {
            assert!(e.begin_edit(&source, &reading, id));
        }
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        // Two distinct saved start vertices have the same X; replace each real
        // field once, then undo/redo the second through the existing widgets.
        if drag {
            super::drag_tests::move_saved_l(&ctx, &mut e);
        } else {
            replace_field(&ctx, &mut e, "60", "80");
            replace_field(&ctx, &mut e, "60", "80");
        }
        assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
        assert_eq!(e.draft.as_ref().expect("draft").points[2][0], "80");
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Undo draft"));
        assert_eq!(e.draft.as_ref().expect("draft").points[2][0], "60");
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Redo draft"));
        if cuts > 0 {
            let valid = e.draft.clone().expect("draft");
            e.draft.as_mut().expect("draft").points[1][0] = "55.5".into();
            e.draft.as_mut().expect("draft").points[2][0] = "55.5".into();
            e.record(valid.clone());
            let far = e
                .editing
                .as_ref()
                .expect("edit")
                .1
                .cut_history
                .as_ref()
                .expect("history")
                .tools
                .last()
                .expect("far")
                .feature;
            let out = frame(&ctx, &mut e, vec![]);
            assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains(&far.to_string()))), "offending Cut is visible");
            assert_eq!(
                e.draft.as_ref().expect("retained invalid draft").points[1][0],
                "55.5"
            );
            assert!(e.edit_request().is_err());
            e.undo();
            assert_eq!(e.draft, Some(valid));
        }
        let kept = e.draft.clone();
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Remove"));
        assert_eq!(e.draft, kept, "disabled segment removal");
        let out = frame(&ctx, &mut e, vec![]);
        let canvas = out
            .shapes
            .iter()
            .find_map(|c| {
                if let egui::Shape::Rect(r) = &c.shape {
                    ((r.rect.width() - 510.).abs() < 1. && (r.rect.height() - 250.).abs() < 1.)
                        .then_some(r.rect)
                } else {
                    None
                }
            })
            .expect("canvas");
        click(&ctx, &mut e, canvas.center());
        assert_eq!(e.draft, kept, "closed canvas cannot add");
        let out = frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, text_at(&out, "Save edited copy…"));
        let request = e.take_edit_request().expect("submit");
        assert!(e.take_edit_request().is_none());
        assert_eq!(request.source, source);
        assert_eq!(request.expected, reading.version);
        // No dialog result starts nothing; the same owned draft remains retryable.
        assert_eq!(e.draft, kept);
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
                .start_sketch(request.clone(), move |r, g, c| {
                    crate::edits::spawn_sketch_edit(r, c, move |result| {
                        tx.send((g, result)).expect("reply")
                    })
                })
                .expect("worker");
            assert!(
                edits
                    .start_sketch(request, |_, _, _| panic!("duplicate worker"))
                    .is_none()
            );
            assert!(
                finish_edit(
                    &mut e,
                    &mut edits,
                    generation + 1,
                    Err(CadError::input("stale response"))
                )
                .is_none()
            );
            assert_eq!(e.draft, kept);
            let (g, result) = rx.recv().expect("completed");
            let path = finish_edit(&mut e, &mut edits, g, result);
            if occupied {
                assert!(path.is_none());
                assert_eq!(e.draft, kept);
                assert_eq!(
                    std::fs::read(root.path().join("occupied.fcad")).expect("sentinel"),
                    b"keep"
                );
            } else {
                assert_eq!(path, Some(root.path().join("ui.fcad")));
                assert!(!e.active());
                if cuts > 0 {
                    let path = path.expect("published path");
                    e.draft_load_finished(&path, false);
                    assert_eq!(e.draft, kept, "failed async Open restores draft");
                    e.draft_published(&path);
                    e.draft_load_finished(&path, true);
                    assert!(!e.active());
                }
            }
        }
        // IDs and finite numeric coordinates only; process JSON assertions live
        // in the CLI suite, avoiding an application dependency just for a test.
        let vertices = request
            .vertices
            .iter()
            .map(|v| {
                format!(
                    r#"{{"curve_id":"{}","start_mm":[{},{}]}}"#,
                    v.curve_id, v.start_mm[0], v.start_mm[1]
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            &input,
            format!(r#"{{"request_version":1,"vertices":[{vertices}]}}"#),
        )
        .expect("request");
        let cli = root.path().join("cli.fcad");
        let p = std::process::Command::new(ferritecad())
            .arg("edit-sketch-copy")
            .arg(&source)
            .arg("--sketch")
            .arg(id.to_string())
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
        assert_eq!(a.meta(), b.meta());
        assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
        assert_eq!(
            a.dependencies().expect("deps"),
            b.dependencies().expect("deps")
        );
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
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
}

#[cfg(test)]
#[path = "sketch/drag_tests.rs"]
pub(crate) mod drag_tests;
