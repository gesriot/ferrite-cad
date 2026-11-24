// SPDX-License-Identifier: MIT
//! Saved partial Revolve angle: text, validation, local history and copy requests.

use super::{Editor, ExtrudeEditSource, number, push_bounded};
use ferritecad_document::ExpressionSlotValue;
use ferritecad_jobs::{EditRequest, RevolveAngle};
use ferritecad_types::{CadError, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub(super) struct Draft {
    /// §27E: the saved partial Revolve whose angle is being edited, with the
    /// request it was read from, the angle as typed, and that draft's own
    /// bounded history under the same policy as the others.
    pub(super) editing_angle: Option<(EditRequest, ExpressionSlotValue)>,
    pub(super) pending_angle_edit: Option<EditRequest>,
    /// §30E: availability and request for Apply inside the open document.
    pub(super) can_apply_angle: bool,
    pub(super) pending_apply_angle: Option<EditRequest>,
    pub(super) angle_edit: String,
    pub(super) angle_undo: Vec<String>,
    pub(super) angle_redo: Vec<String>,
    pub(super) angle_applied: Option<String>,
}

impl Editor {
    pub(crate) fn editing_saved_angle(&self) -> bool {
        self.angle.editing_angle.is_some()
    }
    pub(crate) fn set_angle_apply(&mut self, can_apply: bool) {
        self.angle.can_apply_angle = can_apply;
    }
    pub(crate) fn take_apply_angle_request(&mut self) -> Option<EditRequest> {
        self.angle.pending_apply_angle.take()
    }

    pub(crate) fn take_angle_edit_request(&mut self) -> Option<EditRequest> {
        self.angle.pending_angle_edit.take()
    }
    /// Begin editing the angle of one saved partial Revolve of the accepted
    /// scene (§27E).
    ///
    /// The path and the version come from the reading that was accepted, so a
    /// later Open that has not been accepted cannot retarget this draft. The
    /// form opens on the saved angle, exactly as stored.
    pub(crate) fn begin_angle_edit(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        id: ferritecad_types::ObjectId,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .expression_slot(id, ferritecad_document::ExpressionSlot::RevolveAngle)
            .filter(|c| c.refusal.is_none())
        else {
            return false;
        };
        let radians = choice.expression.value();
        self.dismiss();
        self.angle.angle_edit = ferritecad_types::display_degrees(radians).to_string();
        self.angle.angle_applied = Some(self.angle.angle_edit.clone());
        self.angle.editing_angle = Some((
            EditRequest {
                source: path.to_path_buf(),
                expected: source.version,

                changes: vec![
                    ferritecad_document::EditChange::constant(
                        id,
                        ferritecad_document::ExpressionSlot::RevolveAngle,
                        radians,
                    )
                    .expect("validated expression"),
                ],
                destination: PathBuf::new(),
            },
            choice.clone(),
        ));
        true
    }
    /// What the angle edit form is asking for, or why it is not an edit yet.
    /// Parses here; whether the number is an angle is the document's answer.
    pub(super) fn angle_edit_request(&self) -> Result<EditRequest> {
        let (basis, choice) = self
            .angle
            .editing_angle
            .as_ref()
            .ok_or_else(|| CadError::input("no saved Revolve angle draft"))?;
        let mut request = basis.clone();
        let radians = number(&self.angle.angle_edit)?.to_radians();
        choice.validate(radians)?;
        request.changes[0] =
            ferritecad_document::EditChange::constant(choice.object, choice.slot, radians)?;
        Ok(request)
    }
    pub(super) fn apply_angle(&mut self) -> Result<()> {
        let request = self.angle_edit_request()?;
        self.angle.angle_edit = ferritecad_types::display_degrees(
            request.changes[0]
                .expression()
                .expect("numeric form contains an expression change")
                .value(),
        )
        .to_string();
        let before = self
            .angle
            .angle_applied
            .replace(self.angle.angle_edit.clone())
            .ok_or_else(|| CadError::input("no applied Revolve angle draft"))?;
        if self.angle.angle_edit != before {
            push_bounded(&mut self.angle.angle_undo, before);
            self.angle.angle_redo.clear();
        }
        Ok(())
    }
    /// Shows what is stored, by identity, and offers the one number this edit
    /// may change. The profile, the axis and the direction are named as kept:
    /// they are not editable here. The profile has its own saved Sketch editor.
    fn draw_angle_edit(&mut self, ui: &mut egui::Ui) {
        let Some((request, choice)) = &self.angle.editing_angle else {
            return;
        };
        ui.label("Saved partial Revolve · angle only · right-handed about the sketch +Y axis");
        ui.small(format!(
            "Revolve {} · {}",
            request.changes[0].object(),
            crate::sessions::shown_as(&request.source)
        ));
        ui.small(format!(
            "Saved angle {}°",
            ferritecad_types::display_degrees(choice.expression.value())
        ));
        egui::Grid::new("ferritecad saved revolve angle")
            .num_columns(3)
            .show(ui, |ui| {
                ui.label("Angle");
                ui.add(
                    egui::TextEdit::singleline(&mut self.angle.angle_edit)
                        .char_limit(64)
                        .desired_width(120.),
                );
                ui.label(format!(
                    "° ({}–{}, a full turn is not an angle)",
                    ferritecad_types::display_degrees(RevolveAngle::MIN_RADIANS),
                    ferritecad_types::display_degrees(RevolveAngle::MAX_RADIANS)
                ));
                ui.end_row();
            });
        match self.angle_edit_request() {
            Ok(request) => {
                let pending = self.angle.angle_applied.as_ref() != Some(&self.angle.angle_edit);
                let differs = self
                    .angle
                    .editing_angle
                    .as_ref()
                    .map(|(_, choice)| choice.expression.value())
                    != Some(
                        request.changes[0]
                            .expression()
                            .expect("numeric form contains an expression change")
                            .value(),
                    );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.angle.can_apply_angle && differs,
                            egui::Button::new("Apply angle"),
                        )
                        .clicked()
                    {
                        self.angle.pending_apply_angle = Some(request.clone());
                    }
                    if ui
                        .add_enabled(pending, egui::Button::new("Confirm draft numbers"))
                        .clicked()
                    {
                        self.apply_angle().expect("validated angle draft");
                    }
                });
                let applied = self.angle.angle_applied.as_ref() == Some(&self.angle.angle_edit);
                if ui
                    .add_enabled(
                        applied && !self.unsaved,
                        egui::Button::new("Save edited Revolve copy…"),
                    )
                    .clicked()
                {
                    self.angle.pending_angle_edit = Some(request);
                }
                if !differs {
                    ui.small("This is the stored angle: there is nothing to apply.");
                }
                if !applied && !self.unsaved {
                    ui.small("Confirm the draft numbers before saving a copy.");
                }
                if self.unsaved {
                    ui.small(
                        "Saving a copy as a new file is unavailable while the document has \
                        unsaved changes: Save or Undo them first. Apply changes this \
                        document; the file on disk changes only when you Save.",
                    );
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        }
    }

    pub(super) fn undo_angle(&mut self) {
        if let Some(previous) = self.angle.angle_undo.pop() {
            push_bounded(
                &mut self.angle.angle_redo,
                self.angle
                    .angle_applied
                    .replace(previous.clone())
                    .expect("applied angle"),
            );
            self.angle.angle_edit = previous;
        }
    }
    pub(super) fn redo_angle(&mut self) {
        if let Some(next) = self.angle.angle_redo.pop() {
            push_bounded(
                &mut self.angle.angle_undo,
                self.angle
                    .angle_applied
                    .replace(next.clone())
                    .expect("applied angle"),
            );
            self.angle.angle_edit = next;
        }
    }
    pub(super) fn draw_angle_draft(&mut self, ui: &mut egui::Ui, running: bool, outcome: &str) {
        ui.add_enabled_ui(!running, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.angle.angle_undo.is_empty(),
                        egui::Button::new("Undo draft"),
                    )
                    .clicked()
                {
                    self.undo_angle();
                }
                if ui
                    .add_enabled(
                        !self.angle.angle_redo.is_empty(),
                        egui::Button::new("Redo draft"),
                    )
                    .clicked()
                {
                    self.redo_angle();
                }
                if ui.button("Cancel draft").clicked() {
                    self.dismiss();
                }
            });
        });
        // Cancel took the draft down; there is nothing left to draw.
        if self.angle.editing_angle.is_none() {
            return;
        }
        ui.add_enabled_ui(!running, |ui| self.draw_angle_edit(ui));
        if running {
            ui.label("Working… Draft retained until the change is shown. Cancel job in toolbar.");
        }
        ui.small(
            "Undo/redo changes only this draft; document history waits until Apply or Cancel.",
        );
        Self::outcome_line(ui, outcome);
    }
}
