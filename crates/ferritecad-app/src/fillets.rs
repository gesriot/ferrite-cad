// SPDX-License-Identifier: MIT
//! Draft fields over stored geometry; the shared transaction judges Apply.
use crate::feature_facts::{
    Corner as FilletCorner, EdgeChoice as FilletChoice, EdgeFillet, EdgeTarget as SavedFilletTarget,
};
use crate::sketch_facts::Source as ExtrudeEditSource;
use ferritecad_document::{DocumentVersion, ExpressionSlotValue, SweptEdge};
use ferritecad_jobs::EditRequest;
use ferritecad_types::{CadError, ObjectId, Result};
use std::path::{Path, PathBuf};

/// What the form holds before any of it is a request: the chosen corner, by
/// its position in the catalogue's list, and the radius as text.
///
/// The position never leaves the form. What is sent is the corner the
/// catalogue listed at that position, by its producer and its two Lines.
#[derive(Debug, Clone, Default, PartialEq)]
struct Typed {
    corner: Option<usize>,
    radius: String,
}

#[derive(Debug, Clone)]
struct Draft {
    source: PathBuf,
    version: DocumentVersion,
    choice: FilletChoice,
    typed: Typed,
    /// The last selection and radius a confirmed Apply accepted.
    applied: Option<Typed>,
    refusal: Option<String>,
}

impl Draft {
    fn corners(&self) -> &[FilletCorner] {
        self.choice
            .target
            .as_ref()
            .map(|t| t.corners.as_slice())
            .unwrap_or_default()
    }

    /// The typed state as one request, judged by the document's own rule.
    fn fillet(&self, typed: &Typed) -> Result<(FilletCorner, EdgeFillet)> {
        let corner = typed
            .corner
            .and_then(|i| self.corners().get(i))
            .ok_or_else(|| CadError::input("choose one vertical edge to round"))?;
        let radius_mm = typed
            .radius
            .trim()
            .parse::<f64>()
            .map_err(|_| CadError::input("enter a finite fillet radius in mm"))?;
        let fillet = EdgeFillet {
            edge: SweptEdge {
                feature: corner.feature,
                joint: corner.joint,
            },
            radius_mm,
        };
        let target = self.choice.target.as_ref().expect("form has target");
        target.addition(fillet.feature()?, "Fillet")?;
        Ok((*corner, fillet))
    }
}

/// §28B: a new radius for the saved Fillet, as typed. The edge is the saved
/// one and is shown, never chosen: this form cannot retarget the Fillet.
#[derive(Debug, Clone)]
struct RadiusDraft {
    source: PathBuf,
    version: DocumentVersion,
    choice: ExpressionSlotValue,
    typed: String,
    /// The radius confirmed for the copy workflow, as typed.
    applied: Option<String>,
    refusal: Option<String>,
}

impl RadiusDraft {
    /// The typed radius, judged by the document's own rule.
    fn radius(&self, typed: &str) -> Result<f64> {
        let radius_mm = typed
            .trim()
            .parse::<f64>()
            .map_err(|_| CadError::input("enter a finite fillet radius in mm"))?;
        self.choice.validate(radius_mm)?;
        Ok(radius_mm)
    }
}

#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<Draft>,
    pending: Option<EditRequest>,
    radius: Option<RadiusDraft>,
    pending_radius: Option<EditRequest>,
    pending_apply: Option<EditRequest>,
    /// §30J: the new Fillet the Add form asked to add to the open document.
    pending_add: Option<EditRequest>,
    /// Whether the window's `can_add_fillet` allows the Add form's own Add now.
    can_add: bool,
    can_begin_radius: bool,
    can_apply: bool,
    unsaved: bool,
    outcome: String,
}

impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some() || self.radius.is_some()
    }
    pub(crate) fn dismiss(&mut self) {
        let availability = self.session();
        let add = self.can_add;
        *self = Self::default();
        self.set_session(availability.0, availability.1, availability.2);
        self.can_add = add;
    }
    pub(crate) fn take_request(&mut self) -> Option<EditRequest> {
        self.pending.take()
    }
    pub(crate) fn take_radius_request(&mut self) -> Option<EditRequest> {
        self.pending_radius.take()
    }

    pub(crate) fn editing_radius(&self) -> bool {
        self.radius.is_some() && self.draft.is_none()
    }
    /// §30J: the Add form is open (not the radius edit of a saved Fillet).
    pub(crate) fn adding(&self) -> bool {
        self.draft.is_some() && self.radius.is_none()
    }
    /// What the window's `can_add_fillet` says, each frame.
    pub(crate) fn set_add(&mut self, can_add: bool) {
        self.can_add = can_add;
    }
    pub(crate) fn add_session(&self) -> bool {
        self.can_add
    }
    /// §30J: the new Fillet the user asked to add to the open document.
    pub(crate) fn take_add_request(&mut self) -> Option<EditRequest> {
        self.pending_add.take()
    }
    pub(crate) fn set_session(&mut self, can_begin: bool, can_apply: bool, unsaved: bool) {
        self.can_begin_radius = can_begin;
        self.can_apply = can_apply;
        self.unsaved = unsaved;
    }
    pub(crate) fn session(&self) -> (bool, bool, bool) {
        (self.can_begin_radius, self.can_apply, self.unsaved)
    }
    pub(crate) fn set_outcome(&mut self, outcome: &str) {
        self.outcome = outcome.to_owned();
    }
    pub(crate) fn take_apply_request(&mut self) -> Option<EditRequest> {
        self.pending_apply.take()
    }

    /// Begin changing the radius of one saved Fillet. The form opens on the
    /// stored radius.
    pub(crate) fn begin_radius(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        feature: ObjectId,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .expression_slot(feature, ferritecad_document::ExpressionSlot::FilletRadius)
            .filter(|c| c.refusal.is_none())
        else {
            return false;
        };
        self.radius = Some(RadiusDraft {
            source: path.to_path_buf(),
            version: source.version,
            typed: choice.expression.value().to_string(),
            choice: choice.clone(),
            applied: None,
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
            .fillet_bodies
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
            applied: None,
            refusal: None,
        });
        true
    }

    /// One button per Body the document says can be filleted, with the reason
    /// on the ones it says cannot.
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
        for choice in &source.fillet_bodies {
            let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
            // §30J: the Add form adds into the open document, so it may be opened
            // with unsaved changes whenever the session is idle.
            let response = ui.add_enabled(
                (can_begin || self.can_begin_radius) && refusal.is_none(),
                egui::Button::new(format!(
                    "Fillet edge of {} — {}…",
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
        // §28B: one per saved Fillet, with the shared domain reason on the
        // ones this build cannot edit.
        for choice in source
            .expression_slots
            .iter()
            .filter(|s| s.slot == ferritecad_document::ExpressionSlot::FilletRadius)
        {
            let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
            let response = ui.add_enabled(
                (can_begin || self.can_begin_radius) && refusal.is_none(),
                egui::Button::new(format!(
                    "Edit Fillet radius {} — {} (r{} mm)…",
                    choice.name.as_deref().unwrap_or("Unnamed fillet"),
                    choice.object,
                    choice.expression.value()
                )),
            );
            if response.clicked() {
                self.begin_radius(path, source, choice.object);
            }
            if let Some(reason) = refusal {
                response.on_hover_text(reason);
            }
        }
    }

    fn draw_radius(&mut self, ui: &mut egui::Ui, running: bool) {
        let Some(draft) = &mut self.radius else {
            return;
        };
        let saved = draft.choice.clone();
        let mut cancel = false;
        crate::form_window(ui, "Edit Fillet radius")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                if !self.outcome.is_empty() {
                    ui.label(format!("Document: {}", self.outcome));
                }
                ui.label(
                    "Changes only the radius. The Fillet keeps its edge, its UUID and every \
                     name; the edge cannot be changed here.",
                );
                ui.small(format!("Fillet {} · {}", saved.object, crate::sessions::shown_as(&draft.source)));
                ui.small(format!("Saved value {} mm", saved.expression.value()));
                ui.add_enabled_ui(!running, |ui| {
                    if ui.button("Cancel radius draft").clicked() {
                        cancel = true;
                    }
                    ui.horizontal(|ui| {
                        ui.label("New radius (mm):");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.typed)
                                .id_salt("fillet-new-radius")
                                .char_limit(32)
                                .desired_width(110.),
                        );
                    });
                    let valid = draft.radius(&draft.typed);
                    let differs = valid.as_ref().is_ok_and(|r| *r != saved.expression.value());
                    if ui.add_enabled(self.can_apply && differs && valid.is_ok(),
                        egui::Button::new("Apply radius")).clicked() {
                        self.pending_apply = Some(EditRequest {
                            source: draft.source.clone(),
                            expected: draft.version,

            changes: vec![ferritecad_document::EditChange::constant(saved.object, ferritecad_document::ExpressionSlot::FilletRadius, valid.as_ref().copied().expect("validated radius")).expect("validated expression")],
                            destination: PathBuf::new(),
                        });
                    }
                    if let Err(error) = valid {
                        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                    } else if !differs {
                        ui.small("This is the stored radius: there is nothing to apply.");
                    }
                    if ui.button("Confirm draft number").clicked() {
                        match draft.radius(&draft.typed) {
                            Ok(_) => {
                                draft.applied = Some(draft.typed.clone());
                                draft.refusal = None;
                            }
                            Err(error) => draft.refusal = Some(error.to_string()),
                        }
                    }
                    if let Some(refusal) = &draft.refusal {
                        egui::ScrollArea::vertical()
                            .id_salt("fillet-radius-refusal")
                            .max_height(72.)
                            .show(ui, |ui| {
                                ui.colored_label(ui.visuals().error_fg_color, refusal);
                            });
                    }
                    let confirmed = draft
                        .applied
                        .as_ref()
                        .filter(|applied| **applied == draft.typed)
                        .and_then(|applied| draft.radius(applied).ok());
                    match confirmed {
                        Some(radius_mm) => {
                            ui.small(format!("Ready: {} mm -> {radius_mm} mm", saved.expression.value()));
                            if ui.add_enabled(!self.unsaved, egui::Button::new("Save radius copy…")).clicked() {
                                self.pending_radius = Some(EditRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,

            changes: vec![ferritecad_document::EditChange::constant(saved.object, ferritecad_document::ExpressionSlot::FilletRadius, radius_mm).expect("validated expression")],
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
                    ui.small("Saving a copy is unavailable while the document has unsaved changes: Save or Undo them first.");
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
        if self.radius.is_some() {
            self.draw_radius(ui, running);
            return;
        }
        let Some(draft) = &mut self.draft else {
            return;
        };
        let Some(target) = draft.choice.target.clone() else {
            return;
        };
        let mut cancel = false;
        crate::form_window(ui, "Fillet one vertical edge")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                if !self.outcome.is_empty() {
                    ui.label(format!("Document: {}", self.outcome));
                }
                ui.label(
                    "Rounds one vertical edge of the plate with a constant radius. The edge is \
                     named by the base Extrude and the two Lines that meet at its corner.",
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
                    "Radius from {} mm to {} × the shorter adjacent side.",
                    ferritecad_document::MIN_RADIUS_MM,
                    ferritecad_document::MAX_RADIUS_FRACTION
                ));
                if target.constrained {
                    ui.label(format!(
                        "This plate's Sketch carries constraints. The corners and sides below \
                         are the stored drawing, the solver's starting guess. Where the corner \
                         is and the {} × shorter side limit are the solved plate's: the new copy \
                         is saved only if the solved plate is still this rectangle with every \
                         Line on its side and room for the radius at that corner.",
                        ferritecad_document::MAX_RADIUS_FRACTION
                    ));
                }
                ui.add_enabled_ui(!running, |ui| {
                    if ui.button("Cancel fillet draft").clicked() {
                        cancel = true;
                    }
                    ui.label("Edge:");
                    for (i, corner) in target.corners.iter().enumerate() {
                        ui.radio_value(
                            &mut draft.typed.corner,
                            Some(i),
                            describe_candidate(corner, &target),
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.label("Radius (mm):");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.typed.radius)
                                .id_salt("fillet-radius")
                                .char_limit(32)
                                .desired_width(110.),
                        );
                    });
                    // §30J: into the open document, no file name; the current
                    // corner and valid radius, whether or not the draft was confirmed.
                    let valid = draft.fillet(&draft.typed);
                    if ui
                        .add_enabled(
                            self.can_add && valid.is_ok(),
                            egui::Button::new("Add fillet"),
                        )
                        .clicked()
                    {
                        let (_, fillet) = valid.as_ref().copied().expect("validated draft");
                        self.pending_add = Some(EditRequest {
                            source: draft.source.clone(),
                            expected: draft.version,
                            changes: target
                                .addition(fillet.feature().expect("validated number"), "Fillet")
                                .expect("validated draft"),
                            destination: PathBuf::new(),
                        });
                    }
                    if let Err(error) = &valid {
                        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                    }
                    if ui.button("Confirm draft edge and radius").clicked() {
                        match draft.fillet(&draft.typed) {
                            Ok(_) => {
                                draft.applied = Some(draft.typed.clone());
                                draft.refusal = None;
                            }
                            Err(error) => draft.refusal = Some(error.to_string()),
                        }
                    }
                    if let Some(refusal) = &draft.refusal {
                        egui::ScrollArea::vertical()
                            .id_salt("fillet-refusal")
                            .max_height(72.)
                            .show(ui, |ui| {
                                ui.colored_label(ui.visuals().error_fg_color, refusal);
                            });
                    }
                    let confirmed = draft
                        .applied
                        .as_ref()
                        .filter(|applied| **applied == draft.typed)
                        .and_then(|applied| draft.fillet(applied).ok());
                    match confirmed {
                        Some((corner, fillet)) => {
                            ui.small(if target.constrained {
                                let [a, b] = corner.joint.segments();
                                format!(
                                    "Ready: round the edge between Lines {a} | {b} with r{} mm; \
                                     the radius is checked on the solved plate when saved",
                                    fillet.radius_mm
                                )
                            } else {
                                format!(
                                    "Ready: round the edge at ({}, {}) with r{} mm",
                                    corner.corner_mm[0], corner.corner_mm[1], fillet.radius_mm
                                )
                            });
                            if ui
                                .add_enabled(!self.unsaved, egui::Button::new("Save fillet copy…"))
                                .clicked()
                            {
                                self.pending = Some(EditRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,
                                    changes: target
                                        .addition(
                                            fillet.feature().expect("validated number"),
                                            "Fillet",
                                        )
                                        .expect("validated draft"),
                                    destination: PathBuf::new(),
                                });
                            }
                        }
                        None => {
                            ui.small("Confirm the draft edge and radius before saving a copy.");
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
                    "This document changes with Add fillet; the file on disk changes only on Save.",
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

/// One candidate as the form lists it: where, which Lines, and the limit.
/// For a constrained plate (§28F) the numbers are the stored drawing's and are
/// labelled so; the limit is the solved plate's and is not shown as a number.
fn describe(corner: &FilletCorner, _constrained: bool) -> String {
    let [a, b] = corner.joint.segments();
    format!(
        "Corner ({}, {}) — Lines {a} | {b}; stored sides {} × {} mm",
        corner.corner_mm[0],
        corner.corner_mm[1],
        corner.adjacent_lengths_mm[0],
        corner.adjacent_lengths_mm[1]
    )
}
fn describe_candidate(corner: &FilletCorner, target: &SavedFilletTarget) -> String {
    describe(corner, target.constrained)
}

/// §28B's completion, through exactly the same two steps and retention as
/// the Fillet's own.
pub(crate) fn finish_fillet_radius(
    editor: &mut crate::sketch::Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::EditedDocument>,
) -> Option<PathBuf> {
    if edits.accepts(generation) {
        match &result {
            Ok(saved) => editor.draft_published(&saved.destination),
            // §28H: the refusal is shown in the form it came from as well as
            // in the status line behind it; the draft is kept.
            Err(error) => {
                if let Some(draft) = editor.fillets.radius.as_mut() {
                    draft.refusal = Some(format!("Could not save: {error}"));
                }
            }
        }
    }
    edits.finish(generation, result)
}

#[cfg(test)]
#[allow(clippy::panic)]
pub(crate) mod tests {
    pub(crate) mod add_session;
    pub(crate) mod session_apply;
    use super::*;
    use ferritecad_document::{
        Body, CapSide, DatumPlane, Dependency, DependencyRole, Document, EndCondition, EntityKind,
        Expression, Extrude, ObjectPayload, Point2, SelectionRule, SemanticRole, Sketch,
        SketchCurve, SketchGeometry, SolidOperation, TopologyRef,
    };
    use ferritecad_types::{StableEntityId, Transform};

    /// The asymmetric, translated plate of the probe, drawn clockwise from a
    /// corner other than the first.
    const PLATE: [[f64; 2]; 4] = [[33., 15.5], [33., 3.25], [-4.5, 3.25], [-4.5, 15.5]];

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
    fn radius(ctx: &egui::Context, e: &mut Editor, value: &str) {
        enter(ctx, e, "Radius (mm):", value);
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

    /// The plate written straight into a document: no kernel is needed to
    /// discover it, and the native gate below rebuilds exactly this.
    pub(crate) fn plate() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("плита.fcad");
        let mut d = Document::create(&path).expect("document");
        let [plane, profile, extrude, body] = std::array::from_fn(|_| ObjectId::new());
        let segments: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();
        d.write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("XY"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            )?;
            let curves = (0..4)
                .map(|i| {
                    Ok(SketchCurve {
                        id: segments[i],
                        construction: false,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(PLATE[i][0], PLATE[i][1])?,
                            end: Point2::new(PLATE[(i + 1) % 4][0], PLATE[(i + 1) % 4][1])?,
                        },
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            w.put_object(
                profile,
                None,
                1,
                Some("Profile"),
                &ObjectPayload::Sketch(Sketch {
                    plane,
                    curves,
                    constraints: Vec::new(),
                }),
            )?;
            w.put_object(
                extrude,
                None,
                2,
                Some("Extrude1"),
                &ObjectPayload::Extrude(Extrude {
                    profile,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(6.75)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    previous: None,
                }),
            )?;
            w.put_object(
                body,
                None,
                3,
                Some("Plate"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(extrude),
                }),
            )?;
            for (dependent, dependency, role) in [
                (profile, plane, DependencyRole::Plane),
                (extrude, profile, DependencyRole::Profile),
                (body, extrude, DependencyRole::BodyTip),
            ] {
                w.add_dependency(Dependency {
                    dependent,
                    dependency,
                    role,
                })?;
            }
            for side in [CapSide::Start, CapSide::End] {
                w.put_topology_ref(&TopologyRef {
                    id: StableEntityId::new(),
                    owner: extrude,
                    producer_feature: extrude,
                    expected_kind: EntityKind::Face,
                    output_role: SemanticRole::ExtrudeCap { side },
                    selection: SelectionRule::Exact,
                    fallback_signature: None,
                })?;
            }
            Ok(())
        })
        .expect("plate");
        let source = crate::sketch_facts::Source::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }

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

    type Rows = Vec<Vec<rusqlite::types::Value>>;
    pub(crate) fn tables(path: &Path) -> std::collections::BTreeMap<String, (Vec<String>, Rows)> {
        let c =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .expect("SQL");
        let names: Vec<String> = c
            .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
            .expect("names")
            .query_map([], |r| r.get(0))
            .expect("names")
            .collect::<rusqlite::Result<_>>()
            .expect("names");
        names
            .into_iter()
            .map(|name| {
                let quoted = format!("\"{}\"", name.replace('"', "\"\""));
                let mut stmt = c
                    .prepare(&format!("SELECT rowid,* FROM {quoted} ORDER BY rowid"))
                    .or_else(|_| c.prepare(&format!("SELECT * FROM {quoted} ORDER BY 1,2")))
                    .expect("table");
                let n = stmt.column_count();
                let columns: Vec<String> = (0..n)
                    .map(|i| stmt.column_name(i).expect("column").to_owned())
                    .collect();
                let rows = stmt
                    .query_map([], |r| (0..n).map(|i| r.get(i)).collect())
                    .expect("rows")
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .expect("rows");
                (name, (columns, rows))
            })
            .collect()
    }

    /// §28F: [`plate`], dimensioned by the shipped preparation and writer
    /// with no kernel: H/V on every Line, the first Line's start fixed at
    /// (36.5, 17.75), 41 mm wide and 14.25 mm deep. Its stored corner
    /// (33, 3.25) solves to (36.5, 3.5); the stored sides allow r ≤ 6.125
    /// mm there and the solved ones r ≤ 7.125 mm.
    pub(crate) fn dimensioned() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        use ferritecad_document::{
            AddLineConstraint, AddSketchConstraint, LineConstraintKind, LineEndpoint, LineLengthMm,
            SketchConstraintEdits, SketchCoordinateMm,
        };
        let (root, path, _) = plate();
        let mut d = Document::open(&path).expect("writable");
        let (sketch, lines) = d
            .objects()
            .expect("objects")
            .into_iter()
            .find_map(|o| match o.payload {
                ObjectPayload::Sketch(s) => {
                    Some((o.id, s.curves.iter().map(|c| c.id).collect::<Vec<_>>()))
                }
                _ => None,
            })
            .expect("the Sketch");
        let line = |curve, kind| AddSketchConstraint::Line(AddLineConstraint::Line { curve, kind });
        let mut add: Vec<_> = (0..4)
            .map(|i| {
                line(
                    lines[i],
                    if i % 2 == 0 {
                        LineConstraintKind::Vertical
                    } else {
                        LineConstraintKind::Horizontal
                    },
                )
            })
            .collect();
        add.push(line(
            lines[0],
            LineConstraintKind::Fixed {
                at: LineEndpoint::Start,
                x: SketchCoordinateMm::new(36.5).expect("x"),
                y: SketchCoordinateMm::new(17.75).expect("y"),
            },
        ));
        add.push(line(
            lines[1],
            LineConstraintKind::Distance(LineLengthMm::new(41.).expect("width")),
        ));
        add.push(line(
            lines[0],
            LineConstraintKind::Distance(LineLengthMm::new(14.25).expect("depth")),
        ));
        let edits = SketchConstraintEdits {
            remove: Vec::new(),
            add,
        };
        let prepared =
            ferritecad_fixtures::constraint_changes(&d, sketch, &edits).expect("prepared");
        d.write_changes(&prepared).expect("written");
        let source = crate::sketch_facts::Source::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }

    /// §28B: the plate of [`plate`], with a §28A Fillet on its (33, 3.25)
    /// corner written by the shipped preparation and writer, and no kernel.
    /// §28C's height form tests use it too.
    /// A radius edit mints nothing: the worker's copy and the command line's
    /// are the same cells but for the stamp, and the exports the same bytes.
    pub(crate) fn same_radius_publication(path: &Path, ui: &Path, peer: &Path) {
        let (ui, peer) = (ui.to_path_buf(), peer.to_path_buf());
        let (left, right) = (tables(&ui), tables(&peer));
        let original = tables(path);
        assert_eq!(
            left.keys().collect::<Vec<_>>(),
            right.keys().collect::<Vec<_>>()
        );
        for (table, (columns, rows)) in &left {
            let (their_columns, theirs) = &right[table];
            assert_eq!(columns, their_columns);
            assert_eq!(rows.len(), theirs.len(), "{table} rows");
            assert_eq!(
                rows.len(),
                original[table].1.len(),
                "{table} rows vs source"
            );
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
                let result = std::process::Command::new(crate::creates::tests::ferritecad())
                    .arg(format!("export-{format}"))
                    .arg(model)
                    .arg("-o")
                    .arg(&output)
                    .arg("--json")
                    .output()
                    .expect("export");
                assert!(result.status.success(), "{result:?}");
                exports.push(std::fs::read(output).expect("export bytes"));
            }
            assert_eq!(exports[0], exports[1], "worker/CLI {format} bytes");
        }
    }

    pub(crate) fn rounded(radius: f64) -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let (root, path, source) = plate();
        let target = source.fillet_bodies[0].target.clone().expect("a target");
        let corner = target
            .corners
            .iter()
            .find(|c| c.corner_mm == [33., 3.25])
            .expect("that corner");
        let mut d = Document::open(&path).expect("writable");
        let changes = target
            .addition(
                (EdgeFillet {
                    edge: SweptEdge {
                        feature: target.base_feature,
                        joint: corner.joint,
                    },
                    radius_mm: radius,
                })
                .feature()
                .expect("number"),
                "Fillet",
            )
            .expect("changes");
        d.write_changes(&changes).expect("written");
        let source = crate::sketch_facts::Source::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }

    /// The same widget request through the app's worker and through the
    /// shipped CLI publishes one document: every SQL cell equal with no
    /// identifier mapped (nothing is minted), except each copy's own stamp,
    /// and byte-identical STL and FBX.
    #[test]
    fn native_fillet_radius_widgets_worker_and_cli_publish_the_same_part() {
        if !ferritecad_occt::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        let (root, path, source) = rounded(2.375);
        let before = std::fs::read(&path).expect("source");
        let mut e = Editor::default();
        begin_by(&mut e, &path, &source, "Edit Fillet radius");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        enter(&ctx, &mut e, "New radius (mm):", "1.1875");
        click(&ctx, &mut e, "Confirm draft number");
        click(&ctx, &mut e, "Save radius copy…");
        let mut request = e.take_radius_request().expect("widget request");
        let radius = request.changes[0]
            .expression()
            .expect("numeric form contains an expression change")
            .value();
        let feature = request.changes[0].object();

        let ui = root.path().join("worker.fcad");
        request.destination = ui.clone();
        let mut edits = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        edits
            .start(request, move |r, g, c| {
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
        let (generation, result) = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("worker response");
        assert!(result.as_ref().expect("published").rebuild.all_built());
        let mut editor = crate::sketch::Editor::default();
        editor.fillets = e;
        assert_eq!(
            finish_fillet_radius(&mut editor, &mut edits, generation, result),
            Some(ui.clone()),
            "publication goes to the ordinary async Open"
        );
        assert!(!editor.active());
        editor.draft_load_finished(&ui, false);
        assert!(editor.fillets.active(), "a refused Open restores the draft");
        editor.draft_published(&ui);
        editor.draft_load_finished(&ui, true);
        assert!(!editor.active());

        let input = root.path().join("request.json");
        std::fs::write(
            &input,
            format!(r#"{{"request_version":1,"changes":[{{"kind":"set_expression","object":"{feature}","slot":{{"kind":"fillet_radius"}},"value":{radius},"unit":"mm"}}]}}"#),
        )
        .expect("input");
        let peer = root.path().join("peer.fcad");
        let out = std::process::Command::new(crate::creates::tests::ferritecad())
            .arg("edit")
            .arg(&path)
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

        same_radius_publication(&path, &ui, &peer);
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }

    /// §28H: [`rounded`] with a second Fillet at (33, 15.5), across the 12.25
    /// mm side, written by the shipped preparation and writer.
    pub(crate) fn rounded_twice(
        r1: f64,
        r2: f64,
    ) -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let (root, path, source) = rounded(r1);
        let target = source.fillet_bodies[0]
            .target
            .clone()
            .expect("a second target");
        let corner = *target
            .corners
            .iter()
            .find(|c| c.corner_mm == [33., 15.5])
            .expect("that corner");
        let mut d = Document::open(&path).expect("writable");
        let changes = target
            .addition(
                (EdgeFillet {
                    edge: SweptEdge {
                        feature: target.base_feature,
                        joint: corner.joint,
                    },
                    radius_mm: r2,
                })
                .feature()
                .expect("number"),
                "Fillet",
            )
            .expect("changes");
        d.write_changes(&changes).expect("written");
        let source = crate::sketch_facts::Source::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }

    /// §28H: the same second-Fillet radius edit through the app's worker and
    /// through the shipped CLI publishes one document; nothing is minted.
    #[test]
    fn native_sequential_radius_worker_and_cli_publish_the_same_part() {
        if !ferritecad_occt::is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        let (root, path, source) = rounded_twice(2.375, 3.0625);
        let before = std::fs::read(&path).expect("source");
        let feature = session_apply::selected(&source, 0).feature;
        let mut e = Editor::default();
        begin_by(
            &mut e,
            &path,
            &source,
            &format!("Edit Fillet radius Fillet — {feature}"),
        );
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        enter(&ctx, &mut e, "New radius (mm):", "4.8125");
        click(&ctx, &mut e, "Confirm draft number");
        click(&ctx, &mut e, "Save radius copy…");
        let mut request = e.take_radius_request().expect("widget request");
        assert_eq!(request.changes[0].object(), feature);
        let ui = root.path().join("worker.fcad");
        request.destination = ui.clone();
        let mut edits = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        edits
            .start(request, move |r, g, c| {
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
        let (generation, result) = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("worker response");
        assert!(result.as_ref().expect("published").rebuild.all_built());

        let mut editor = crate::sketch::Editor::default();
        editor.fillets = e;
        assert_eq!(
            finish_fillet_radius(&mut editor, &mut edits, generation, result),
            Some(ui.clone()),
            "publication goes to the ordinary async Open"
        );
        editor.draft_load_finished(&ui, false);
        assert!(editor.fillets.active(), "a refused Open restores the draft");
        editor.draft_published(&ui);
        editor.draft_load_finished(&ui, true);
        assert!(!editor.active());

        let input = root.path().join("request.json");
        std::fs::write(&input, format!(r#"{{"request_version":1,"changes":[{{"kind":"set_expression","object":"{feature}","slot":{{"kind":"fillet_radius"}},"value":4.8125,"unit":"mm"}}]}}"#)).expect("input");
        let peer = root.path().join("peer.fcad");
        let out = std::process::Command::new(crate::creates::tests::ferritecad())
            .arg("edit")
            .arg(&path)
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
        same_radius_publication(&path, &ui, &peer);
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }

    /// §28L: [`rounded`]'s plate with a Fillet at each stored corner of
    /// `spec`, in order, written by the shipped preparation and writer.
    pub(crate) fn rounded_n(
        spec: &[([f64; 2], f64)],
    ) -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let (root, path, mut source) = plate();
        for (at, r) in spec {
            let choice = source.fillet_bodies[0].clone();
            let target = choice.target.clone().expect("a target");
            let corner = *target
                .corners
                .iter()
                .find(|c| c.corner_mm == *at)
                .expect("a candidate corner");
            let mut d = Document::open(&path).expect("writable");
            let changes = target
                .addition(
                    (EdgeFillet {
                        edge: SweptEdge {
                            feature: target.base_feature,
                            joint: corner.joint,
                        },
                        radius_mm: *r,
                    })
                    .feature()
                    .expect("number"),
                    "Fillet",
                )
                .expect("changes");
            d.write_changes(&changes).expect("written");
            source = crate::sketch_facts::Source::read(&d).expect("snapshot");
            d.close().expect("close");
        }
        (root, path, source)
    }
}
