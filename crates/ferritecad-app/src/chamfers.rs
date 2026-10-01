// SPDX-License-Identifier: MIT
//! A disposable single-edge chamfer request over stored facts; no kernel, no
//! file reads (§29A).
//!
//! The form offers only what the document says can be cut: the four vertical
//! edges of one saved rectangular plate, each named by the base Extrude and the
//! two Lines that meet there. It lists them rather than letting a pointer pick
//! geometry, because this slice has no picking. The distance is in millimetres
//! **along each of the two faces** the edge joins — not the width of the
//! slanted flat, which is that times √2 — and the form says so.
//!
//! The whole request (the corner and the distance) is one history entry:
//! **Undo** and **Redo** step through the requests a confirmed **Apply**
//! accepted, restoring both at once. The Fillet's form has no such history and
//! this one does not borrow its description.
use ferritecad_document::{
    ChamferChoice, ChamferCorner, ChamferDistanceChoice, DocumentVersion, EdgeChamfer,
    ExtrudeEditSource, SavedChamfer, SavedChamferTarget, SweptEdge,
};
use ferritecad_jobs::{EdgeChamferRequest, EditChamferDistanceRequest};
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

/// The requests a confirmed Apply accepted, oldest first, with the one the
/// form is at. The first entry is always the empty request.
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
        let corner = self.choice.validate(&chamfer)?;
        Ok((corner, chamfer))
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
    choice: ChamferDistanceChoice,
    typed: String,
    history: History<String>,
    refusal: Option<String>,
}

impl DistanceDraft {
    fn saved(&self) -> Option<&SavedChamfer> {
        self.choice.saved.as_ref()
    }

    /// The typed distance, judged by the document's own rule.
    fn distance(&self, typed: &str) -> Result<f64> {
        let distance_mm = typed
            .trim()
            .parse::<f64>()
            .map_err(|_| CadError::input("enter a finite chamfer distance in mm"))?;
        self.saved()
            .ok_or_else(|| CadError::unsupported("this Chamfer cannot be edited"))?
            .check_distance(distance_mm)?;
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
    pending: Option<EdgeChamferRequest>,
    distance: Option<DistanceDraft>,
    pending_distance: Option<EditChamferDistanceRequest>,
}

impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some() || self.distance.is_some()
    }
    pub(crate) fn dismiss(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn take_request(&mut self) -> Option<EdgeChamferRequest> {
        self.pending.take()
    }
    pub(crate) fn take_distance_request(&mut self) -> Option<EditChamferDistanceRequest> {
        self.pending_distance.take()
    }

    /// Begin changing the distance of the saved Chamfer. The form opens on the
    /// stored distance.
    fn begin_distance(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        feature: ObjectId,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .chamfer_features
            .iter()
            .find(|c| c.feature == feature && c.refusal.is_none() && c.saved.is_some())
        else {
            return false;
        };
        let typed = choice.stored.distance_mm.to_string();
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

    fn begin(&mut self, path: &Path, source: &ExtrudeEditSource, body: ObjectId) -> bool {
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
            let response = ui.add_enabled(
                can_begin && refusal.is_none(),
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
        for choice in &source.chamfer_features {
            let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
            let response = ui.add_enabled(
                can_begin && refusal.is_none(),
                egui::Button::new(format!(
                    "Edit Chamfer distance {} — {} (d{} mm)…",
                    choice.name.as_deref().unwrap_or("Unnamed chamfer"),
                    choice.feature,
                    choice.stored.distance_mm
                )),
            );
            if response.clicked() {
                self.begin_distance(path, source, choice.feature);
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
        let Some(saved) = draft.saved().cloned() else {
            return;
        };
        let mut cancel = false;
        egui::Window::new("Edit Chamfer distance — new copy")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(
                    "Changes only the distance. The Chamfer keeps its edge, its UUID and every \
                     name; the edge cannot be changed here.",
                );
                ui.small(format!(
                    "Chamfer {} · Body {} · base Extrude {} · {}",
                    saved.feature,
                    saved.body,
                    saved.base_feature,
                    draft.source.display()
                ));
                ui.small(format!("Edge: {}", describe(&saved.corner)));
                ui.small(format!(
                    "Saved distance {} mm along each face; from {} mm to {} mm here. The \
                     slanted flat is {} × √2 wide.",
                    saved.distance_mm,
                    ferritecad_document::MIN_DISTANCE_MM,
                    saved.corner.max_distance_mm,
                    saved.distance_mm
                ));
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
                    if ui.button("Apply distance").clicked() {
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
                                "Ready: distance {} mm -> {distance_mm} mm at ({}, {})",
                                saved.distance_mm,
                                saved.corner.corner_mm[0],
                                saved.corner.corner_mm[1]
                            ));
                            if ui.button("Save distance copy…").clicked() {
                                self.pending_distance = Some(EditChamferDistanceRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,
                                    feature: saved.feature,
                                    distance_mm,
                                    destination: PathBuf::new(),
                                });
                            }
                        }
                        None => {
                            ui.small("Apply a distance before saving.");
                        }
                    }
                });
                if running {
                    ui.label("Saving… Draft retained until publication. Cancel job in toolbar.");
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
        egui::Window::new("Chamfer one vertical edge — new copy")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
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
                    draft.source.display()
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
                    if ui.button("Apply chamfer").clicked() {
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
                            if ui.button("Save chamfer copy…").clicked() {
                                self.pending = Some(EdgeChamferRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,
                                    body: target.body,
                                    chamfer,
                                    destination: PathBuf::new(),
                                });
                            }
                        }
                        None => {
                            ui.small("Apply an edge and a distance before saving.");
                        }
                    }
                });
                if running {
                    ui.label("Saving… Draft retained until publication. Cancel job in toolbar.");
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
    if corner.is_offerable() {
        format!(
            "Corner ({}, {}) — Lines {a} | {b}; sides {} × {} mm; d ≤ {} mm",
            corner.corner_mm[0],
            corner.corner_mm[1],
            corner.adjacent_lengths_mm[0],
            corner.adjacent_lengths_mm[1],
            corner.max_distance_mm
        )
    } else {
        format!(
            "Corner ({}, {}) — Lines {a} | {b}; sides {} × {} mm; too small to chamfer (it \
             leaves no distance)",
            corner.corner_mm[0],
            corner.corner_mm[1],
            corner.adjacent_lengths_mm[0],
            corner.adjacent_lengths_mm[1]
        )
    }
}

fn describe_candidate(corner: &ChamferCorner, _target: &SavedChamferTarget) -> String {
    describe(corner)
}

/// The same two steps every published copy takes: the draft is handed to the
/// shared retention so an Open that is later refused can give it back.
pub(crate) fn finish_chamfer(
    editor: &mut crate::sketch::Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::AddedEdgeChamfer>,
) -> Option<PathBuf> {
    if edits.accepts(generation) {
        match &result {
            Ok(saved) => editor.draft_published(&saved.destination),
            // The refusal is shown in the form it came from as well as in the
            // status line behind it; the draft is kept.
            Err(error) => {
                if let Some(draft) = editor.chamfers.draft.as_mut() {
                    draft.refusal = Some(format!("Could not save: {error}"));
                }
            }
        }
    }
    edits.finish_chamfer(generation, result)
}

/// The distance edit's completion, through exactly the same two steps.
pub(crate) fn finish_chamfer_distance(
    editor: &mut crate::sketch::Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::EditedChamferDistance>,
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
    edits.finish_chamfer_distance(generation, result)
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
