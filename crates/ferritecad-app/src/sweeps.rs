// SPDX-License-Identifier: MIT
//! Create a sweep through the ordinary tab-owned saved-object form.
mod request;
use crate::{sketch::accepted::Accepted, sketch_facts::Source};
use ferritecad_document::{DocumentVersion, ObjectRecord, RevolveAxis, SolidOperation};
use ferritecad_jobs::EditRequest;
use ferritecad_types::ObjectId;
pub(crate) use request::Kind;
use request::{Draft, End, object_label};
use std::path::{Path, PathBuf};

pub(crate) fn available(source: &Source, object: &ObjectRecord) -> Result<(), String> {
    match source
        .profile_failures
        .iter()
        .find(|(id, _)| *id == object.id)
    {
        Some((_, reason)) => Err(format!(
            "{} gives no profile: {reason}",
            object_label(object)
        )),
        None => Ok(()),
    }
}

#[derive(Debug)]
struct Body {
    id: ObjectId,
    label: String,
    inserted_after: String,
}
#[derive(Debug)]
struct Form {
    path: PathBuf,
    version: DocumentVersion,
    objects: Vec<ObjectRecord>,
    heading: String,
    axes: Vec<(RevolveAxis, String)>,
    bodies: Vec<Body>,
    draft: Draft,
    problem: String,
}
#[derive(Debug, Default)]
pub(crate) struct Editor {
    form: Option<Form>,
    pending: Option<EditRequest>,
    can_apply: bool,
}
impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.form.is_some()
    }
    pub(crate) fn set_apply(&mut self, available: bool) {
        self.can_apply = available;
    }
    pub(crate) fn take_request(&mut self) -> Option<EditRequest> {
        self.pending.take()
    }
    pub(crate) fn begin_with(
        &mut self,
        path: &Path,
        source: &Source,
        accepted: &Accepted,
        id: ObjectId,
        kind: Kind,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(presentation) = accepted.presentation(source, id) else {
            return false;
        };
        let Some(object) = source.objects.iter().find(|o| o.id == id) else {
            return false;
        };
        if available(source, object).is_err() {
            return false;
        }
        let bodies: Vec<_> = source
            .fillet_bodies
            .iter()
            .filter_map(|choice| {
                let target = choice.target.as_ref()?;
                let body = source.objects.iter().find(|o| o.id == target.body)?;
                let shown = source
                    .objects
                    .iter()
                    .find(|o| o.id == target.previous_feature)?;
                Some(Body {
                    id: body.id,
                    label: object_label(body),
                    inserted_after: format!("Inserted after {}", object_label(shown)),
                })
            })
            .collect();
        self.form = Some(Form {
            path: path.to_owned(),
            version: source.version,
            heading: format!(
                "{} {}",
                kind.label(),
                object.name.as_deref().unwrap_or("Unnamed")
            ),
            axes: request::axes(presentation),
            draft: Draft::new(kind, id, bodies.first().map(|b| b.id)),
            bodies,
            objects: source.objects.clone(),
            problem: String::new(),
        });
        true
    }

    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, running: bool, outcome: &str) {
        let Some(form) = &mut self.form else { return };
        ui.heading(&form.heading);
        let mut apply = false;
        let mut cancel = false;
        ui.add_enabled_ui(!running, |ui| {
            let draft = &mut form.draft;
            ui.label("Operation");
            ui.horizontal_wrapped(|ui| {
                for (operation, label) in [
                    (SolidOperation::NewBody, "New body"),
                    (SolidOperation::Add, "Add"),
                    (SolidOperation::Cut, "Cut"),
                    (SolidOperation::Intersect, "Intersect"),
                ] {
                    let enabled = operation == SolidOperation::NewBody || !form.bodies.is_empty();
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::new(label).selected(draft.operation == operation),
                        )
                        .on_disabled_hover_text("The document has no body to change.")
                        .clicked()
                    {
                        draft.operation = operation;
                    }
                }
            });
            if draft.operation != SolidOperation::NewBody {
                ui.label("Body");
                let label = form
                    .bodies
                    .iter()
                    .find(|b| Some(b.id) == draft.body)
                    .map(|b| b.label.as_str())
                    .unwrap_or("Choose a body");
                egui::ComboBox::from_id_salt("sweep body")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for body in &form.bodies {
                            ui.selectable_value(&mut draft.body, Some(body.id), &body.label);
                        }
                    });
                if let Some(body) = form.bodies.iter().find(|b| Some(b.id) == draft.body) {
                    ui.label(&body.inserted_after);
                }
            }
            match draft.kind {
                Kind::Extrude => {
                    ui.label("End");
                    egui::ComboBox::from_id_salt("sweep end")
                        .selected_text(draft.end.label())
                        .show_ui(ui, |ui| {
                            for end in [End::Blind, End::Symmetric, End::ThroughAll, End::UpToNext]
                            {
                                ui.selectable_value(&mut draft.end, end, end.label());
                            }
                        });
                    if matches!(draft.end, End::Blind | End::Symmetric) {
                        ui.label("Distance (mm)");
                        ui.text_edit_singleline(&mut draft.distance);
                        if draft.end == End::Symmetric {
                            ui.label("Distance on each side of the sketch.");
                        }
                    }
                    ui.checkbox(&mut draft.reversed, "Reversed");
                }
                Kind::Revolve => {
                    ui.label("Axis");
                    let label = form
                        .axes
                        .iter()
                        .find(|(axis, _)| *axis == draft.axis)
                        .expect("selected axis belongs to the accepted sketch")
                        .1
                        .as_str();
                    egui::ComboBox::from_id_salt("sweep axis")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (axis, label) in &form.axes {
                                ui.selectable_value(&mut draft.axis, *axis, label);
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut draft.full_turn, true, "Full turn");
                        ui.selectable_value(&mut draft.full_turn, false, "Angle (°)");
                    });
                    if !draft.full_turn {
                        ui.text_edit_singleline(&mut draft.angle);
                    }
                }
            }
            ui.label("All closed regions of the sketch are used.");
            ui.horizontal(|ui| {
                apply = ui
                    .add_enabled(self.can_apply, egui::Button::new("Apply"))
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        ui.label(&form.problem);
        ui.label(outcome);
        if apply {
            match form.draft.changes(&form.objects) {
                Ok(changes) => {
                    form.problem.clear();
                    self.pending = Some(EditRequest {
                        source: form.path.clone(),
                        expected: form.version,
                        changes,
                        destination: PathBuf::new(),
                    });
                }
                Err(error) => form.problem = error.to_string(),
            }
        }
        if cancel {
            *self = Self::default();
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
pub(crate) mod tests;
