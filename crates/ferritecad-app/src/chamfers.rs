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
//! **Undo request** and **Redo request** step through the requests a confirmed
//! **Confirm draft edge and distance** (formerly *Apply chamfer*) accepted,
//! restoring both at once. §30K: **Add chamfer** reads the current fields and adds
//! the Chamfer to the open document; that request history is never the document's. The Fillet's form has no
//! such history and this one does not borrow its description.
//!
//! §30H: the distance form of an existing Chamfer also applies its current text
//! to the open document (**Apply distance**), through the session, while dirty.
//! Its **Confirm draft number** keeps the form's own history of confirmed
//! requests for the copy workflow; that history is not the document's Undo/Redo.
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
    /// §30H: the distance the user asked to apply to the open document.
    pending_apply: Option<EditChamferDistanceRequest>,
    /// §30K: the new Chamfer the Add form asked to add to the open document.
    pending_add: Option<EdgeChamferRequest>,
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
    pub(crate) fn take_add_request(&mut self) -> Option<EdgeChamferRequest> {
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
    pub(crate) fn take_apply_request(&mut self) -> Option<EditChamferDistanceRequest> {
        self.pending_apply.take()
    }
    pub(crate) fn take_request(&mut self) -> Option<EdgeChamferRequest> {
        self.pending.take()
    }
    pub(crate) fn take_distance_request(&mut self) -> Option<EditChamferDistanceRequest> {
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
        for choice in &source.chamfer_features {
            let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
            let response = ui.add_enabled(
                (can_begin || self.can_begin_distance) && refusal.is_none(),
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
                    "Chamfer {} · Body {} · base Extrude {} · {}",
                    saved.feature,
                    saved.body,
                    saved.base_feature,
                    crate::sessions::shown_as(&draft.source)
                ));
                if saved.constrained {
                    let corner = &saved.corner;
                    let [a, b] = corner.joint.segments();
                    ui.small(format!(
                        "Edge: Corner ({}, {}) — Lines {a} | {b}; stored sides {} × {} mm",
                        corner.corner_mm[0],
                        corner.corner_mm[1],
                        corner.adjacent_lengths_mm[0],
                        corner.adjacent_lengths_mm[1]
                    ));
                    // §29D: the stored sides are the solver's starting guess; the
                    // largest distance is the solved plate's, judged by the
                    // rebuild when the change is built, so none is shown here.
                    ui.small(format!(
                        "Saved distance {} mm along each face, at least {} mm. This plate's \
                         Sketch carries constraints: the largest distance is judged on the \
                         solved plate when the change is built. The slanted flat is {} × √2 \
                         wide.",
                        saved.distance_mm,
                        ferritecad_document::MIN_DISTANCE_MM,
                        saved.distance_mm
                    ));
                } else {
                    ui.small(format!("Edge: {}", describe(&saved.corner)));
                    ui.small(format!(
                        "Saved distance {} mm along each face; from {} mm to {} mm here. The \
                         slanted flat is {} × √2 wide.",
                        saved.distance_mm,
                        ferritecad_document::MIN_DISTANCE_MM,
                        saved.corner.max_distance_mm,
                        saved.distance_mm
                    ));
                }
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
                    let differs = valid.as_ref().is_ok_and(|d| *d != saved.distance_mm);
                    if ui
                        .add_enabled(
                            self.can_apply && differs,
                            egui::Button::new("Apply distance"),
                        )
                        .clicked()
                        && let Ok(distance_mm) = valid.as_ref()
                    {
                        self.pending_apply = Some(EditChamferDistanceRequest {
                            source: draft.source.clone(),
                            expected: draft.version,
                            feature: saved.feature,
                            distance_mm: *distance_mm,
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
                                "Ready: distance {} mm -> {distance_mm} mm at ({}, {})",
                                saved.distance_mm,
                                saved.corner.corner_mm[0],
                                saved.corner.corner_mm[1]
                            ));
                            if ui
                                .add_enabled(
                                    !self.unsaved,
                                    egui::Button::new("Save distance copy…"),
                                )
                                .clicked()
                            {
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
                        self.pending_add = Some(EdgeChamferRequest {
                            source: draft.source.clone(),
                            expected: draft.version,
                            body: target.body,
                            chamfer,
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
    /// Opens the form through its own button, as a person would.
    fn begin_by(e: &mut Editor, path: &Path, source: &ExtrudeEditSource, button: &str) {
        let ctx = egui::Context::default();
        let layout = |e: &mut Editor, events| {
            let mut o = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(988., 768.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| e.choices(ui, true, Some(path), Some(source)),
            );
            o.textures_delta.clear();
            o
        };
        let out = layout(e, Vec::new());
        let at = find(&out, button).unwrap_or_else(|| panic!("button {button}"));
        layout(e, vec![egui::Event::PointerMoved(at)]);
        for pressed in [true, false] {
            layout(
                e,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
            );
        }
        assert!(e.active(), "the button opened the form");
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
        let prepared = ferritecad_document::prepare_edge_chamfer(
            &d,
            source.chamfer_bodies[0].body,
            &EdgeChamfer {
                edge: SweptEdge {
                    feature: corner.feature,
                    joint: corner.joint,
                },
                distance_mm,
            },
        )
        .expect("prepared");
        d.write_edge_chamfer(&prepared).expect("written");
        let source = ExtrudeEditSource::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }

    fn published_stub() -> ferritecad_jobs::AddedEdgeChamfer {
        let corner = ferritecad_document::ChamferCorner {
            feature: ObjectId::new(),
            joint: ferritecad_types::ProfileJoint::new(
                StableEntityId::new(),
                StableEntityId::new(),
            )
            .expect("joint"),
            corner_mm: [0., 0.],
            adjacent_lengths_mm: [1., 1.],
            max_distance_mm: 0.5,
        };
        ferritecad_jobs::AddedEdgeChamfer {
            destination: PathBuf::from("stale.fcad"),
            document_id: ferritecad_types::DocumentId::new(),
            body: ObjectId::new(),
            feature: ObjectId::new(),
            previous: ObjectId::new(),
            corner,
            distance_mm: 0.25,
            references: Vec::new(),
        }
    }

    /// The form lists the four corners the document names, refuses what the
    /// document would, steps through the whole request with Undo and Redo, keeps
    /// Save and Cancel on the screen, and keeps its draft through a cancelled
    /// Save, a worker refusal and a stale reply. No kernel is involved.
    #[test]
    fn chamfer_widgets_list_corners_refuse_like_the_document_and_keep_the_draft() {
        let (_root, path, source) = crate::fillets::tests::plate();
        let choice = source.chamfer_bodies[0].clone();
        let target = choice.target.clone().expect("a target");
        let mut e = Editor::default();
        begin_by(&mut e, &path, &source, "Chamfer edge of Plate");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        let out = frame(&ctx, &mut e, false);
        for corner in &target.corners {
            let [a, b] = corner.joint.segments();
            assert!(painted(
                &out,
                &format!(
                    "Corner ({}, {}) — Lines {a} | {b}",
                    corner.corner_mm[0], corner.corner_mm[1]
                )
            ));
        }
        assert!(painted(&out, &target.base_feature.to_string()));
        assert!(painted(&out, "measured along each face"));
        assert!(painted(&out, "× √2"));
        assert!(!painted(&out, "Save chamfer copy…"), "nothing applied yet");
        // Undo and Redo exist and are inert while the history is empty.
        assert!(painted(&out, "Undo request") && painted(&out, "Redo request"));
        let before = e.draft.as_ref().expect("draft").typed.clone();
        click(&ctx, &mut e, "Undo request");
        click(&ctx, &mut e, "Redo request");
        assert_eq!(e.draft.as_ref().expect("draft").typed, before);

        // Nothing chosen, then text that is no distance, then distances the
        // document refuses: each refused, none recorded.
        click(&ctx, &mut e, "Confirm draft edge and distance");
        assert!(
            e.draft
                .as_ref()
                .expect("draft")
                .refusal
                .as_deref()
                .expect("refused")
                .contains("choose one vertical edge")
        );
        click(&ctx, &mut e, "Corner (33, 3.25)");
        let chosen = target
            .corners
            .iter()
            .position(|c| c.corner_mm == [33., 3.25])
            .expect("that corner");
        assert_eq!(e.draft.as_ref().expect("draft").typed.corner, Some(chosen));
        let max = target.corners[chosen].max_distance_mm;
        let past = max.next_up().to_string();
        for (text, why) in [
            ("banana", "finite"),
            ("0", "at least"),
            ("0.0009", "at least"),
            (past.as_str(), "too large"),
            ("12.25", "too large"),
            ("inf", "finite"),
        ] {
            distance(&ctx, &mut e, text);
            click(&ctx, &mut e, "Confirm draft edge and distance");
            let draft = e.draft.as_ref().expect("draft");
            assert!(!draft.history.can_undo(), "{text} was recorded");
            assert!(
                draft.refusal.as_deref().expect("a refusal").contains(why),
                "{text}: {:?}",
                draft.refusal
            );
        }
        // The exact maximum is accepted and the next float was not.
        distance(&ctx, &mut e, &max.to_string());
        click(&ctx, &mut e, "Confirm draft edge and distance");
        assert!(e.draft.as_ref().expect("draft").history.can_undo());
        distance(&ctx, &mut e, "2.375");
        click(&ctx, &mut e, "Confirm draft edge and distance");
        let out = frame(&ctx, &mut e, false);
        assert!(painted(
            &out,
            "Ready: chamfer the edge at (33, 3.25) with d2.375 mm"
        ));
        assert!(painted(&out, "Save chamfer copy…"));
        // An unapplied change un-readies the request.
        distance(&ctx, &mut e, "3");
        assert!(!painted(&frame(&ctx, &mut e, false), "Save chamfer copy…"));
        distance(&ctx, &mut e, "2.375");
        assert!(painted(&frame(&ctx, &mut e, false), "Save chamfer copy…"));

        // Whole-request Undo and Redo: the corner and the distance come back
        // together, a new request after an Undo drops the Redo tail.
        click(&ctx, &mut e, "Corner (-4.5, 15.5)");
        distance(&ctx, &mut e, "1.5");
        click(&ctx, &mut e, "Confirm draft edge and distance");
        let third = e.draft.as_ref().expect("draft").typed.clone();
        assert_eq!(third.distance, "1.5");
        click(&ctx, &mut e, "Undo request");
        let second = e.draft.as_ref().expect("draft").typed.clone();
        assert_eq!(
            (second.corner, second.distance.as_str()),
            (Some(chosen), "2.375")
        );
        assert!(painted(
            &frame(&ctx, &mut e, false),
            "Ready: chamfer the edge at (33, 3.25) with d2.375 mm"
        ));
        click(&ctx, &mut e, "Undo request");
        assert_eq!(
            e.draft.as_ref().expect("draft").typed.distance,
            max.to_string()
        );
        click(&ctx, &mut e, "Undo request");
        assert_eq!(e.draft.as_ref().expect("draft").typed, Typed::default());
        assert!(!painted(&frame(&ctx, &mut e, false), "Save chamfer copy…"));
        click(&ctx, &mut e, "Redo request");
        click(&ctx, &mut e, "Redo request");
        assert_eq!(e.draft.as_ref().expect("draft").typed, second);
        click(&ctx, &mut e, "Redo request");
        assert_eq!(e.draft.as_ref().expect("draft").typed, third);
        click(&ctx, &mut e, "Undo request");
        distance(&ctx, &mut e, "2.375");
        click(&ctx, &mut e, "Confirm draft edge and distance");
        assert_eq!(
            e.draft.as_ref().expect("draft").history.states.len(),
            4,
            "an unchanged request is not a new entry"
        );
        click(&ctx, &mut e, "Redo request");
        assert_eq!(
            e.draft.as_ref().expect("draft").typed,
            third,
            "the future was not discarded by a request that changed nothing"
        );
        click(&ctx, &mut e, "Undo request");
        assert_eq!(e.draft.as_ref().expect("draft").typed, second);

        // Save and Cancel are on the screen, whatever the form holds.
        let out = frame(&ctx, &mut e, false);
        for label in ["Save chamfer copy…", "Cancel chamfer draft"] {
            let at = find(&out, label).unwrap_or_else(|| panic!("{label} is not painted"));
            assert!(
                at.y > 0. && at.y < 768. && at.x > 0. && at.x < 988.,
                "{label} at {at:?}"
            );
        }

        click(&ctx, &mut e, "Save chamfer copy…");
        let request = e.take_request().expect("the widgets' request");
        assert!(e.take_request().is_none(), "one press, one request");
        assert_eq!(request.source, path);
        assert_eq!(request.expected, source.version);
        assert_eq!(request.body, choice.body);
        assert_eq!(request.chamfer.edge.feature, target.base_feature);
        assert_eq!(request.chamfer.edge.joint, target.corners[chosen].joint);
        assert_eq!(request.chamfer.distance_mm, 2.375);
        let typed = e.draft.as_ref().expect("draft").typed.clone();

        // The Save dialog was cancelled: nothing ran, and the draft is as it was.
        assert!(e.active());
        assert_eq!(e.draft.as_ref().expect("draft").typed, typed);
        // While a job runs the form is inert, and says the draft is kept.
        let out = frame(&ctx, &mut e, true);
        assert!(painted(
            &out,
            "Draft retained until publication or acceptance"
        ));
        click_while(&ctx, &mut e, "Cancel chamfer draft", true);
        assert!(e.active(), "Cancel is disabled while saving");

        // A worker refusal and a stale reply both leave the draft in place.
        let mut editor = crate::sketch::Editor::default();
        editor.chamfers = e;
        let mut edits = crate::edits::Edits::default();
        let mut refused = request.clone();
        refused.destination = PathBuf::from("refused.fcad");
        let generation = edits
            .start_chamfer(refused, |_, _, _| std::thread::spawn(|| {}))
            .expect("started");
        assert_eq!(
            finish_chamfer(
                &mut editor,
                &mut edits,
                generation + 1,
                Ok(published_stub())
            ),
            None,
            "a stale reply is ignored"
        );
        assert!(editor.chamfers.active());
        assert_eq!(
            finish_chamfer(
                &mut editor,
                &mut edits,
                generation,
                Err(ferritecad_types::CadError::kernel("refused by the worker"))
            ),
            None
        );
        assert!(editor.chamfers.active(), "a refusal keeps the draft");
        let draft = editor.chamfers.draft.as_ref().expect("draft");
        assert_eq!(draft.typed, typed);
        assert!(
            draft
                .refusal
                .as_deref()
                .is_some_and(|r| r.contains("refused by the worker")),
            "the form shows why: {:?}",
            draft.refusal
        );
        let mut e = std::mem::take(&mut editor.chamfers);
        click(&ctx, &mut e, "Cancel chamfer draft");
        assert!(!e.active());
        assert!(e.take_request().is_none());
    }

    /// The distance form shows the saved edge, never chooses one, steps through
    /// the requests, refuses what the document would and keeps its draft.
    #[test]
    fn chamfer_distance_widgets_show_the_saved_edge_and_keep_the_draft() {
        let (_root, path, source) = chamfered(2.375);
        let choice = source.chamfer_features[0].clone();
        let saved = choice.saved.clone().expect("a saved Chamfer");
        let mut e = Editor::default();
        // The plate's own creation button is refused: it ends in a Chamfer.
        assert!(source.chamfer_bodies[0].target.is_none());
        begin_by(&mut e, &path, &source, "Edit Chamfer distance Chamfer");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        let out = frame(&ctx, &mut e, false);
        assert!(painted(&out, &saved.feature.to_string()));
        assert!(painted(&out, "Saved distance 2.375 mm along each face"));
        assert!(painted(
            &out,
            &format!(
                "Corner ({}, {})",
                saved.corner.corner_mm[0], saved.corner.corner_mm[1]
            )
        ));
        assert!(painted(&out, "the edge cannot be changed here"));
        assert!(
            !painted(&out, "Corner (-4.5, 15.5)"),
            "no other edge is offered"
        );
        assert!(painted(
            &out,
            "Confirm the draft number before saving a copy."
        ));
        let max = saved.corner.max_distance_mm;
        for (text, why) in [
            ("banana", "finite"),
            ("0.0005", "at least"),
            (max.next_up().to_string().as_str(), "too large"),
        ] {
            new_distance(&ctx, &mut e, text);
            click(&ctx, &mut e, "Confirm draft number");
            let draft = e.distance.as_ref().expect("draft");
            assert!(!draft.history.can_undo(), "{text} was recorded");
            assert!(
                draft.refusal.as_deref().expect("refusal").contains(why),
                "{text}"
            );
        }
        new_distance(&ctx, &mut e, "4.5");
        click(&ctx, &mut e, "Confirm draft number");
        assert!(painted(
            &frame(&ctx, &mut e, false),
            &format!(
                "Ready: distance 2.375 mm -> 4.5 mm at ({}, {})",
                saved.corner.corner_mm[0], saved.corner.corner_mm[1]
            )
        ));
        new_distance(&ctx, &mut e, "6");
        click(&ctx, &mut e, "Confirm draft number");
        click(&ctx, &mut e, "Undo request");
        assert_eq!(e.distance.as_ref().expect("draft").typed, "4.5");
        click(&ctx, &mut e, "Undo request");
        assert_eq!(e.distance.as_ref().expect("draft").typed, "2.375");
        assert!(painted(
            &frame(&ctx, &mut e, false),
            "Confirm the draft number before saving a copy."
        ));
        click(&ctx, &mut e, "Redo request");
        click(&ctx, &mut e, "Redo request");
        assert_eq!(e.distance.as_ref().expect("draft").typed, "6");
        click(&ctx, &mut e, "Save distance copy…");
        let request = e.take_distance_request().expect("the widgets' request");
        assert!(e.take_distance_request().is_none());
        assert_eq!(request.source, path);
        assert_eq!(request.expected, source.version);
        assert_eq!(request.feature, saved.feature);
        assert_eq!(request.distance_mm, 6.0);
        assert!(e.active(), "a cancelled Save keeps the draft");

        let mut editor = crate::sketch::Editor::default();
        editor.chamfers = e;
        let mut edits = crate::edits::Edits::default();
        let mut refused = request.clone();
        refused.destination = PathBuf::from("refused.fcad");
        let generation = edits
            .start_chamfer_distance(refused, |_, _, _| std::thread::spawn(|| {}))
            .expect("started");
        assert_eq!(
            finish_chamfer_distance(
                &mut editor,
                &mut edits,
                generation,
                Err(ferritecad_types::CadError::kernel("refused by the worker"))
            ),
            None
        );
        let draft = editor.chamfers.distance.as_ref().expect("draft kept");
        assert_eq!(draft.typed, "6");
        assert!(
            draft
                .refusal
                .as_deref()
                .is_some_and(|r| r.contains("refused by the worker"))
        );
        let mut e = std::mem::take(&mut editor.chamfers);
        click(&ctx, &mut e, "Cancel distance draft");
        assert!(!e.active());

        // A constrained profile's stored sides are only the solver's initial
        // guess. No part of the form may present their bound as the real limit.
        let mut constrained = source.clone();
        constrained.chamfer_features[0]
            .saved
            .as_mut()
            .expect("saved Chamfer")
            .constrained = true;
        assert!(e.begin_distance(&path, &constrained, saved.feature));
        let out = frame(&ctx, &mut e, false);
        assert!(painted(&out, "judged on the solved plate"));
        assert!(
            !painted(&out, "d ≤"),
            "stored geometry is not a solved bound"
        );
        assert!(painted(&out, "stored sides"));
        new_distance(&ctx, &mut e, &(max + 1.0).to_string());
        click(&ctx, &mut e, "Confirm draft number");
        assert_eq!(
            e.distance.as_ref().expect("draft").confirmed(),
            Some(max + 1.0)
        );
    }

    /// The worker and the shipped command line publish one part, for both the
    /// creation and the distance edit: every SQL cell is the same once the
    /// identifiers this operation minted are matched, and the STL and FBX bytes
    /// are the same.
    #[test]
    fn native_chamfer_widgets_worker_and_cli_publish_the_same_part() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: the chamfer worker needs OCCT");
            return;
        }
        let (root, path, source) = crate::fillets::tests::plate();
        let before = std::fs::read(&path).expect("source");
        let mut e = Editor::default();
        begin_by(&mut e, &path, &source, "Chamfer edge of Plate");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        click(&ctx, &mut e, "Corner (-4.5, 15.5)");
        distance(&ctx, &mut e, "3.0625");
        click(&ctx, &mut e, "Confirm draft edge and distance");
        click(&ctx, &mut e, "Save chamfer copy…");
        let mut request = e.take_request().expect("widget request");
        let chamfer = request.chamfer;

        let ui = root.path().join("worker.fcad");
        request.destination = ui.clone();
        let mut edits = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        edits
            .start_chamfer(request, move |r, g, c| {
                crate::edits::spawn_chamfer(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (generation, result) = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("worker response");
        let published = result.as_ref().expect("published").clone();
        assert_eq!(published.corner.corner_mm, [-4.5, 15.5]);
        let typed = e.draft.as_ref().expect("draft").typed.clone();
        let mut editor = crate::sketch::Editor::default();
        editor.chamfers = e;
        assert_eq!(
            finish_chamfer(&mut editor, &mut edits, generation, result),
            Some(ui.clone()),
            "publication goes to the ordinary async Open"
        );
        assert!(!editor.active());
        editor.draft_load_finished(&ui, false);
        assert!(
            editor.chamfers.active(),
            "a refused Open restores the draft"
        );
        assert_eq!(editor.chamfers.draft.as_ref().expect("draft").typed, typed);
        editor.draft_published(&ui);
        editor.draft_load_finished(&ui, true);
        assert!(!editor.active());

        // The same request through the shipped command line.
        let input = root.path().join("request.json");
        let [a, b] = chamfer.edge.joint.segments();
        std::fs::write(
            &input,
            format!(
                r#"{{"request_version":1,"edge":{{"feature_id":"{}","joint":["{b}","{a}"]}},"distance_mm":{}}}"#,
                chamfer.edge.feature, chamfer.distance_mm
            ),
        )
        .expect("input");
        let peer = root.path().join("peer.fcad");
        let out = std::process::Command::new(crate::creates::tests::ferritecad())
            .arg("chamfer-edge-copy")
            .arg(&path)
            .arg("--body")
            .arg(source.chamfer_bodies[0].body.to_string())
            .arg("--expect-version")
            .arg(source.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&peer)
            .arg("--json")
            .output()
            .expect("peer");
        assert!(out.status.success(), "{out:?}");
        crate::fillets::tests::same_publication(&ui, &peer);
        assert_eq!(std::fs::read(&path).expect("source"), before);

        // The distance edit of the published copy: the worker and the CLI.
        let copy = Document::open_read_only(&ui).expect("copy");
        let reading = ExtrudeEditSource::read(&copy).expect("snapshot");
        copy.close().expect("close");
        let mut e = Editor::default();
        begin_by(&mut e, &ui, &reading, "Edit Chamfer distance Chamfer");
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        new_distance(&ctx, &mut e, "5.25");
        click(&ctx, &mut e, "Confirm draft number");
        click(&ctx, &mut e, "Save distance copy…");
        let mut request = e.take_distance_request().expect("widget request");
        let feature = request.feature;
        let edited = root.path().join("worker-edit.fcad");
        request.destination = edited.clone();
        let mut edits = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        edits
            .start_chamfer_distance(request, move |r, g, c| {
                crate::edits::spawn_chamfer_distance(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (generation, result) = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("worker response");
        let done = result.as_ref().expect("published").clone();
        assert_eq!(
            (done.previous_distance_mm, done.distance_mm),
            (3.0625, 5.25)
        );
        let mut editor = crate::sketch::Editor::default();
        editor.chamfers = e;
        assert_eq!(
            finish_chamfer_distance(&mut editor, &mut edits, generation, result),
            Some(edited.clone())
        );
        let input = root.path().join("distance.json");
        std::fs::write(&input, r#"{"request_version":1,"distance_mm":5.25}"#).expect("input");
        let peer_edit = root.path().join("peer-edit.fcad");
        let out = std::process::Command::new(crate::creates::tests::ferritecad())
            .arg("edit-chamfer-distance")
            .arg(&ui)
            .arg("--feature")
            .arg(feature.to_string())
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&peer_edit)
            .arg("--json")
            .output()
            .expect("peer");
        assert!(out.status.success(), "{out:?}");
        crate::fillets::tests::same_radius_publication(&ui, &edited, &peer_edit);
    }
}
