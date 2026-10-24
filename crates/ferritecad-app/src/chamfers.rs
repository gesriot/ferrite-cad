// SPDX-License-Identifier: MIT
//! Draft fields over stored geometry; the shared transaction judges Apply.
use crate::feature_facts::{
    Corner as ChamferCorner, EdgeChamfer, EdgeChoice as ChamferChoice,
    EdgeTarget as SavedChamferTarget,
};
use crate::sketch_facts::Source as ExtrudeEditSource;
use ferritecad_document::{DocumentVersion, ExpressionSlotValue, SweptEdge};
use ferritecad_jobs::EditRequest;
use ferritecad_types::{CadError, ObjectId, Result};
use std::path::{Path, PathBuf};

/// What the form holds before any of it is a request: the chosen corner, by
/// its position in the catalogue's list, and the distance as text.
///
/// The position never leaves the form. What is sent is the corner the
/// catalogue listed at that position, by its producer and its two Lines.
#[derive(Debug, Clone, Default, PartialEq)]
struct Typed {
    corner: Option<usize>,
    distance: String,
}

/// The requests a confirmation accepted, oldest first, with the one the form is
/// at. The first entry is always the empty request (or the saved distance).
#[derive(Debug, Clone, PartialEq)]
struct History<T> {
    states: Vec<T>,
    at: usize,
}

impl<T: Clone + PartialEq> History<T> {
    fn new(initial: T) -> Self {
        Self {
            states: vec![initial],
            at: 0,
        }
    }
    fn current(&self) -> &T {
        &self.states[self.at]
    }
    /// A request accepted by Apply. One accepted again is not a new entry; one
    /// accepted after an Undo discards what Redo would have returned to.
    fn record(&mut self, state: T) {
        if *self.current() == state {
            return;
        }
        self.states.truncate(self.at + 1);
        self.states.push(state);
        self.at = self.states.len() - 1;
    }
    fn can_undo(&self) -> bool {
        self.at > 0
    }
    fn can_redo(&self) -> bool {
        self.at + 1 < self.states.len()
    }
    fn undo(&mut self) -> Option<&T> {
        self.can_undo().then(|| {
            self.at -= 1;
            self.current()
        })
    }
    fn redo(&mut self) -> Option<&T> {
        self.can_redo().then(|| {
            self.at += 1;
            self.current()
        })
    }
}

#[derive(Debug, Clone)]
struct Draft {
    source: PathBuf,
    version: DocumentVersion,
    choice: ChamferChoice,
    typed: Typed,
    history: History<Typed>,
    refusal: Option<String>,
}

impl Draft {
    fn corners(&self) -> &[ChamferCorner] {
        self.choice
            .target
            .as_ref()
            .map(|t| t.corners.as_slice())
            .unwrap_or_default()
    }

    /// The typed state as one request, judged by the document's own rule.
    fn chamfer(&self, typed: &Typed) -> Result<(ChamferCorner, EdgeChamfer)> {
        let corner = typed
            .corner
            .and_then(|i| self.corners().get(i))
            .ok_or_else(|| CadError::input("choose one vertical edge to chamfer"))?;
        let distance_mm = typed
            .distance
            .trim()
            .parse::<f64>()
            .map_err(|_| CadError::input("enter a finite chamfer distance in mm"))?;
        let chamfer = EdgeChamfer {
            edge: SweptEdge {
                feature: corner.feature,
                joint: corner.joint,
            },
            distance_mm,
        };
        let target = self.choice.target.as_ref().expect("form has target");
        target.addition(chamfer.feature()?, "Chamfer")?;
        Ok((*corner, chamfer))
    }

    /// The accepted request the form is at, when the text still says it.
    fn confirmed(&self) -> Option<(ChamferCorner, EdgeChamfer)> {
        (self.history.can_undo() && *self.history.current() == self.typed)
            .then(|| self.chamfer(&self.typed).ok())
            .flatten()
    }
}

/// A new distance for the saved Chamfer, as typed. The edge is the saved one
/// and is shown, never chosen: this form cannot retarget the Chamfer.
#[derive(Debug, Clone)]
struct DistanceDraft {
    source: PathBuf,
    version: DocumentVersion,
    choice: ExpressionSlotValue,
    typed: String,
    history: History<String>,
    refusal: Option<String>,
}

impl DistanceDraft {
    /// The typed distance, judged by the document's own rule.
    fn distance(&self, typed: &str) -> Result<f64> {
        let distance_mm = typed
            .trim()
            .parse::<f64>()
            .map_err(|_| CadError::input("enter a finite chamfer distance in mm"))?;
        self.choice.validate(distance_mm)?;
        Ok(distance_mm)
    }

    fn confirmed(&self) -> Option<f64> {
        (self.history.can_undo() && *self.history.current() == self.typed)
            .then(|| self.distance(&self.typed).ok())
            .flatten()
    }
}

#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<Draft>,
    pending: Option<EditRequest>,
    distance: Option<DistanceDraft>,
    pending_distance: Option<EditRequest>,
    /// §30H: the distance the user asked to apply to the open document.
    pending_apply: Option<EditRequest>,
    /// §30K: the new Chamfer the Add form asked to add to the open document.
    pending_add: Option<EditRequest>,
    /// Whether the window's `can_add_chamfer` allows the Add form's own Add now.
    can_add: bool,
    /// What the window says each frame: whether an existing Chamfer's form may
    /// open on the accepted (possibly unsaved) version, whether its Apply may
    /// start, and whether unsaved changes withhold the copy workflows.
    can_begin_distance: bool,
    can_apply: bool,
    unsaved: bool,
    outcome: String,
}

impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some() || self.distance.is_some()
    }
    pub(crate) fn dismiss(&mut self) {
        let availability = self.session();
        let add = self.can_add;
        *self = Self::default();
        self.set_session(availability.0, availability.1, availability.2);
        self.can_add = add;
    }
    pub(crate) fn editing_distance(&self) -> bool {
        self.distance.is_some() && self.draft.is_none()
    }
    /// §30K: the Add form is open (not the distance edit of a saved Chamfer).
    pub(crate) fn adding(&self) -> bool {
        self.draft.is_some() && self.distance.is_none()
    }
    /// What the window's `can_add_chamfer` says, each frame.
    pub(crate) fn set_add(&mut self, can_add: bool) {
        self.can_add = can_add;
    }
    pub(crate) fn add_session(&self) -> bool {
        self.can_add
    }
    /// §30K: the new Chamfer the user asked to add to the open document.
    pub(crate) fn take_add_request(&mut self) -> Option<EditRequest> {
        self.pending_add.take()
    }
    pub(crate) fn set_session(&mut self, can_begin: bool, can_apply: bool, unsaved: bool) {
        self.can_begin_distance = can_begin;
        self.can_apply = can_apply;
        self.unsaved = unsaved;
    }
    pub(crate) fn session(&self) -> (bool, bool, bool) {
        (self.can_begin_distance, self.can_apply, self.unsaved)
    }
    pub(crate) fn set_outcome(&mut self, outcome: &str) {
        self.outcome = outcome.to_owned();
    }
    pub(crate) fn take_apply_request(&mut self) -> Option<EditRequest> {
        self.pending_apply.take()
    }
    pub(crate) fn take_request(&mut self) -> Option<EditRequest> {
        self.pending.take()
    }
    pub(crate) fn take_distance_request(&mut self) -> Option<EditRequest> {
        self.pending_distance.take()
    }

    /// Begin changing the distance of the saved Chamfer. The form opens on the
    /// stored distance.
    pub(crate) fn begin_distance(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        feature: ObjectId,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .expression_slot(
                feature,
                ferritecad_document::ExpressionSlot::ChamferDistance,
            )
            .filter(|c| c.refusal.is_none())
        else {
            return false;
        };
        let typed = choice.expression.value().to_string();
        self.distance = Some(DistanceDraft {
            source: path.to_path_buf(),
            version: source.version,
            history: History::new(typed.clone()),
            typed,
            choice: choice.clone(),
            refusal: None,
        });
        true
    }

    pub(crate) fn begin(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        body: ObjectId,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .chamfer_bodies
            .iter()
            .find(|c| c.body == body && c.refusal.is_none() && c.target.is_some())
        else {
            return false;
        };
        self.draft = Some(Draft {
            source: path.to_path_buf(),
            version: source.version,
            choice: choice.clone(),
            typed: Typed::default(),
            history: History::new(Typed::default()),
            refusal: None,
        });
        true
    }

    /// One button per Body the document says can be chamfered, with the reason
    /// on the ones it says cannot, and one per saved Chamfer.
    pub(crate) fn choices(
        &mut self,
        ui: &mut egui::Ui,
        can_begin: bool,
        path: Option<&Path>,
        source: Option<&ExtrudeEditSource>,
    ) {
        if self.active() {
            return;
        }
        let (Some(path), Some(source)) = (path, source) else {
            return;
        };
        for choice in &source.chamfer_bodies {
            let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
            // §30K: the Add form adds into the open document, so it may be opened
            // with unsaved changes whenever the session is idle.
            let response = ui.add_enabled(
                (can_begin || self.can_begin_distance) && refusal.is_none(),
                egui::Button::new(format!(
                    "Chamfer edge of {} — {}…",
                    choice.name.as_deref().unwrap_or("Unnamed body"),
                    choice.body
                )),
            );
            if response.clicked() {
                self.begin(path, source, choice.body);
            }
            if let Some(reason) = refusal {
                response.on_hover_text(reason);
            }
        }
        for choice in source
            .expression_slots
            .iter()
            .filter(|s| s.slot == ferritecad_document::ExpressionSlot::ChamferDistance)
        {
            let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
            let response = ui.add_enabled(
                (can_begin || self.can_begin_distance) && refusal.is_none(),
                egui::Button::new(format!(
                    "Edit Chamfer distance {} — {} (d{} mm)…",
                    choice.name.as_deref().unwrap_or("Unnamed chamfer"),
                    choice.object,
                    choice.expression.value()
                )),
            );
            if response.clicked() {
                self.begin_distance(path, source, choice.object);
            }
            if let Some(reason) = refusal {
                response.on_hover_text(reason);
            }
        }
    }

    fn draw_distance(&mut self, ui: &mut egui::Ui, running: bool) {
        let Some(draft) = &mut self.distance else {
            return;
        };
        let saved = draft.choice.clone();
        let mut cancel = false;
        crate::form_window(ui, "Edit Chamfer distance")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                if !self.outcome.is_empty() {
                    ui.label(format!("Document: {}", self.outcome));
                }
                ui.label(
                    "Changes only the distance. The Chamfer keeps its edge, its UUID and every \
                     name; the edge cannot be changed here.",
                );
                ui.small(format!(
                    "Chamfer {} · {}",
                    saved.object,
                    crate::sessions::shown_as(&draft.source)
                ));
                ui.small(format!("Saved value {} mm", saved.expression.value()));
                ui.add_enabled_ui(!running, |ui| {
                    if ui.button("Cancel distance draft").clicked() {
                        cancel = true;
                    }
                    ui.horizontal(|ui| {
                        ui.label("New distance (mm):");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.typed)
                                .id_salt("chamfer-new-distance")
                                .char_limit(32)
                                .desired_width(110.),
                        );
                    });
                    // §30H: the current text, judged by the document's own rule,
                    // applied to the open document without a prior confirmation.
                    let valid = draft.distance(&draft.typed);
                    let differs = valid.as_ref().is_ok_and(|d| *d != saved.expression.value());
                    if ui
                        .add_enabled(
                            self.can_apply && differs,
                            egui::Button::new("Apply distance"),
                        )
                        .clicked()
                        && let Ok(distance_mm) = valid.as_ref()
                    {
                        self.pending_apply = Some(EditRequest {
                            source: draft.source.clone(),
                            expected: draft.version,

                            changes: vec![
                                ferritecad_document::EditChange::constant(
                                    saved.object,
                                    ferritecad_document::ExpressionSlot::ChamferDistance,
                                    *distance_mm,
                                )
                                .expect("validated expression"),
                            ],
                            destination: PathBuf::new(),
                        });
                    }
                    match &valid {
                        Err(error) => {
                            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                        }
                        Ok(_) if !differs => {
                            ui.small("This is the stored distance: there is nothing to apply.");
                        }
                        Ok(_) => {}
                    }
                    if ui.button("Confirm draft number").clicked() {
                        match draft.distance(&draft.typed) {
                            Ok(_) => {
                                draft.history.record(draft.typed.clone());
                                draft.refusal = None;
                            }
                            Err(error) => draft.refusal = Some(error.to_string()),
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                draft.history.can_undo(),
                                egui::Button::new("Undo request"),
                            )
                            .clicked()
                            && let Some(state) = draft.history.undo()
                        {
                            draft.typed = state.clone();
                            draft.refusal = None;
                        }
                        if ui
                            .add_enabled(
                                draft.history.can_redo(),
                                egui::Button::new("Redo request"),
                            )
                            .clicked()
                            && let Some(state) = draft.history.redo()
                        {
                            draft.typed = state.clone();
                            draft.refusal = None;
                        }
                    });
                    if let Some(refusal) = &draft.refusal {
                        egui::ScrollArea::vertical()
                            .id_salt("chamfer-distance-refusal")
                            .max_height(72.)
                            .show(ui, |ui| {
                                ui.colored_label(ui.visuals().error_fg_color, refusal);
                            });
                    }
                    match draft.confirmed() {
                        Some(distance_mm) => {
                            ui.small(format!(
                                "Ready: {} mm -> {distance_mm} mm",
                                saved.expression.value()
                            ));
                            if ui
                                .add_enabled(
                                    !self.unsaved,
                                    egui::Button::new("Save distance copy…"),
                                )
                                .clicked()
                            {
                                self.pending_distance = Some(EditRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,

                                    changes: vec![
                                        ferritecad_document::EditChange::constant(
                                            saved.object,
                                            ferritecad_document::ExpressionSlot::ChamferDistance,
                                            distance_mm,
                                        )
                                        .expect("validated expression"),
                                    ],
                                    destination: PathBuf::new(),
                                });
                            }
                        }
                        None => {
                            ui.small("Confirm the draft number before saving a copy.");
                        }
                    }
                });
                ui.small("Apply changes this document; the file on disk changes only on Save.");
                if self.unsaved {
                    ui.small(
                        "Saving a copy is unavailable while the document has unsaved changes: \
                         Save or Undo them first.",
                    );
                }
                if running {
                    ui.label("Working… Draft retained until acceptance. Cancel job in toolbar.");
                }
            });
        if cancel {
            self.dismiss();
        }
    }

    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, running: bool) {
        if self.distance.is_some() {
            self.draw_distance(ui, running);
            return;
        }
        let Some(draft) = &mut self.draft else {
            return;
        };
        let Some(target) = draft.choice.target.clone() else {
            return;
        };
        let mut cancel = false;
        crate::form_window(ui, "Chamfer one vertical edge")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                if !self.outcome.is_empty() {
                    ui.label(format!("Document: {}", self.outcome));
                }
                ui.label(
                    "Cuts one vertical edge of the plate away at one equal distance along both \
                     of its faces. The edge is named by the base Extrude and the two Lines that \
                     meet at its corner.",
                );
                ui.small(format!(
                    "Body {} · base Extrude {} · profile {} · {} mm tall · {}",
                    target.body,
                    target.base_feature,
                    target.profile,
                    target.height_mm,
                    crate::sessions::shown_as(&draft.source)
                ));
                ui.small(format!(
                    "Distance in mm, measured along each face (the slanted flat is that × √2 \
                     wide): from {} mm to the shorter adjacent side less {} mm.",
                    ferritecad_document::MIN_DISTANCE_MM,
                    ferritecad_document::MIN_FLAT_MM
                ));
                ui.add_enabled_ui(!running, |ui| {
                    if ui.button("Cancel chamfer draft").clicked() {
                        cancel = true;
                    }
                    ui.label("Edge:");
                    // Bounded, so Save and Cancel stay in reach.
                    egui::ScrollArea::vertical()
                        .id_salt("chamfer-edges")
                        .max_height(160.)
                        .show(ui, |ui| {
                            for (i, corner) in target.corners.iter().enumerate() {
                                ui.radio_value(
                                    &mut draft.typed.corner,
                                    Some(i),
                                    describe_candidate(corner, &target),
                                );
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label("Distance (mm):");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.typed.distance)
                                .id_salt("chamfer-distance")
                                .char_limit(32)
                                .desired_width(110.),
                        );
                    });
                    // §30K: into the open document, no file name; the current corner
                    // and valid distance, whether or not the draft was confirmed.
                    let valid = draft.chamfer(&draft.typed);
                    if ui
                        .add_enabled(
                            self.can_add && valid.is_ok(),
                            egui::Button::new("Add chamfer"),
                        )
                        .clicked()
                    {
                        let (_, chamfer) = valid.as_ref().copied().expect("validated draft");
                        self.pending_add = Some(EditRequest {
                            source: draft.source.clone(),
                            expected: draft.version,
                            changes: target
                                .addition(chamfer.feature().expect("validated number"), "Chamfer")
                                .expect("validated draft"),
                            destination: PathBuf::new(),
                        });
                    }
                    if let Err(error) = &valid {
                        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                    }
                    if ui.button("Confirm draft edge and distance").clicked() {
                        match draft.chamfer(&draft.typed) {
                            Ok(_) => {
                                draft.history.record(draft.typed.clone());
                                draft.refusal = None;
                            }
                            Err(error) => draft.refusal = Some(error.to_string()),
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                draft.history.can_undo(),
                                egui::Button::new("Undo request"),
                            )
                            .clicked()
                            && let Some(state) = draft.history.undo()
                        {
                            draft.typed = state.clone();
                            draft.refusal = None;
                        }
                        if ui
                            .add_enabled(
                                draft.history.can_redo(),
                                egui::Button::new("Redo request"),
                            )
                            .clicked()
                            && let Some(state) = draft.history.redo()
                        {
                            draft.typed = state.clone();
                            draft.refusal = None;
                        }
                    });
                    if let Some(refusal) = &draft.refusal {
                        egui::ScrollArea::vertical()
                            .id_salt("chamfer-refusal")
                            .max_height(72.)
                            .show(ui, |ui| {
                                ui.colored_label(ui.visuals().error_fg_color, refusal);
                            });
                    }
                    match draft.confirmed() {
                        Some((corner, chamfer)) => {
                            ui.small(format!(
                                "Ready: chamfer the edge at ({}, {}) with d{} mm",
                                corner.corner_mm[0], corner.corner_mm[1], chamfer.distance_mm
                            ));
                            if ui
                                .add_enabled(!self.unsaved, egui::Button::new("Save chamfer copy…"))
                                .clicked()
                            {
                                self.pending = Some(EditRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,
                                    changes: target
                                        .addition(
                                            chamfer.feature().expect("validated number"),
                                            "Chamfer",
                                        )
                                        .expect("validated draft"),
                                    destination: PathBuf::new(),
                                });
                            }
                        }
                        None => {
                            ui.small("Confirm the draft edge and distance before saving a copy.");
                        }
                    }
                });
                if self.unsaved {
                    ui.small(
                        "Saving a copy is unavailable while the document has unsaved changes: \
                         Save or Undo them first.",
                    );
                }
                ui.small(
                    "This document changes with Add chamfer; the file on disk changes only on \
                     Save.",
                );
                if running {
                    ui.label(
                        "Working… Draft retained until publication or acceptance. Cancel job in \
                         toolbar.",
                    );
                }
            });
        if cancel {
            self.dismiss();
        }
    }
}

/// One corner as the form lists it: where, which Lines, and the limit.
fn describe(corner: &ChamferCorner) -> String {
    let [a, b] = corner.joint.segments();
    format!(
        "Corner ({}, {}) — Lines {a} | {b}; stored sides {} × {} mm",
        corner.corner_mm[0],
        corner.corner_mm[1],
        corner.adjacent_lengths_mm[0],
        corner.adjacent_lengths_mm[1]
    )
}

fn describe_candidate(corner: &ChamferCorner, _target: &SavedChamferTarget) -> String {
    describe(corner)
}

/// The distance edit's completion, through exactly the same two steps.
pub(crate) fn finish_chamfer_distance(
    editor: &mut crate::sketch::Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::EditedDocument>,
) -> Option<PathBuf> {
    if edits.accepts(generation) {
        match &result {
            Ok(saved) => editor.draft_published(&saved.destination),
            Err(error) => {
                if let Some(draft) = editor.chamfers.distance.as_mut() {
                    draft.refusal = Some(format!("Could not save: {error}"));
                }
            }
        }
    }
    edits.finish(generation, result)
}

#[cfg(test)]
mod history_tests {
    use super::*;

    /// Undo and Redo step through the accepted requests, an accepted request
    /// after an Undo discards the Redo tail, and the first state is the empty
    /// request that nothing precedes.
    #[test]
    fn the_request_history_steps_back_and_forward_and_forgets_a_discarded_future() {
        let mut h = History::new(0u32);
        assert!(!h.can_undo() && !h.can_redo());
        h.record(1);
        h.record(1);
        h.record(2);
        assert_eq!(h.states, [0, 1, 2], "the same request twice is one entry");
        assert_eq!(h.undo(), Some(&1));
        assert_eq!(h.undo(), Some(&0));
        assert_eq!(h.undo(), None);
        assert_eq!(h.redo(), Some(&1));
        h.record(7);
        assert_eq!(h.states, [0, 1, 7], "a new request replaces the future");
        assert!(!h.can_redo());
        assert_eq!(h.redo(), None);
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
pub(crate) mod tests {
    pub(crate) mod add_session;
    pub(crate) mod session_apply;
    use super::*;
    use ferritecad_document::Document;
    use ferritecad_types::StableEntityId;

    fn run(
        ctx: &egui::Context,
        e: &mut Editor,
        events: Vec<egui::Event>,
        running: bool,
    ) -> egui::FullOutput {
        let mut o = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(988., 768.),
                )),
                events,
                ..Default::default()
            },
            |ui| e.draw(ui, running),
        );
        o.textures_delta.clear();
        o
    }
    fn frame(ctx: &egui::Context, e: &mut Editor, running: bool) -> egui::FullOutput {
        run(ctx, e, Vec::new(), running)
    }
    fn press(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2, running: bool) {
        run(ctx, e, vec![egui::Event::PointerMoved(at)], running);
        for pressed in [true, false] {
            run(
                ctx,
                e,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
                running,
            );
        }
    }
    fn find(out: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text().starts_with(label) => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
    }
    fn click_while(ctx: &egui::Context, e: &mut Editor, label: &str, running: bool) {
        let out = frame(ctx, e, running);
        let at = find(&out, label).unwrap_or_else(|| panic!("not painted: {label}"));
        press(ctx, e, at, running);
    }
    fn click(ctx: &egui::Context, e: &mut Editor, label: &str) {
        click_while(ctx, e, label, false);
    }
    fn painted(out: &egui::FullOutput, label: &str) -> bool {
        out.shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains(label)))
    }
    fn enter(ctx: &egui::Context, e: &mut Editor, field: &str, value: &str) {
        let out = frame(ctx, e, false);
        let label = find(&out, field).unwrap_or_else(|| panic!("label {field}"));
        press(ctx, e, egui::pos2(label.x + 90., label.y), false);
        run(
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
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                },
                egui::Event::Text(value.into()),
            ],
            false,
        );
    }
    fn distance(ctx: &egui::Context, e: &mut Editor, value: &str) {
        enter(ctx, e, "Distance (mm):", value);
    }
    fn new_distance(ctx: &egui::Context, e: &mut Editor, value: &str) {
        enter(ctx, e, "New distance (mm):", value);
    }
    /// [`crate::fillets::tests::plate`] with one Chamfer at its corner
    /// (33, 3.25), written by the shipped preparation and writer, no kernel.
    pub(crate) fn chamfered(distance_mm: f64) -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let (root, path, source) = crate::fillets::tests::plate();
        let target = source.chamfer_bodies[0].target.clone().expect("a target");
        let corner = target
            .corners
            .iter()
            .find(|c| c.corner_mm == [33., 3.25])
            .expect("that corner");
        let mut d = Document::open(&path).expect("writable");
        let changes = target
            .addition(
                (EdgeChamfer {
                    edge: SweptEdge {
                        feature: corner.feature,
                        joint: corner.joint,
                    },
                    distance_mm,
                })
                .feature()
                .expect("number"),
                "Chamfer",
            )
            .expect("changes");
        d.write_changes(&changes).expect("written");
        let source = crate::sketch_facts::Source::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }
}
