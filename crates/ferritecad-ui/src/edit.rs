// SPDX-License-Identifier: MIT
//! A choice of stored feature, and a distance in mm. No document arithmetic.

use ferritecad_types::ObjectId;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum EditChoice {
    #[default]
    Waiting,
    Begin,
    /// Apply the height to the open document, with no file dialog: the change is
    /// accepted into the session and written only by Save.
    Apply,
    /// Write the edited model to a new file (the copy workflow). Offered only while
    /// the document has no unsaved changes: it opens its output as a new document.
    Save,
    Cancel,
}

/// What the height form may offer this frame, and what is running behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeightState {
    /// A copy edit is running, and whether it can still be told to stop.
    pub running: bool,
    pub can_cancel: bool,
    pub apply: bool,
    /// The copy workflow reads a source and writes a new file, then opens that
    /// file as the document. With unsaved changes that would silently drop them,
    /// so it is not offered until they are saved or undone.
    pub copy: bool,
    /// Why `copy` is off, when it is: unsaved changes (the only reason worth the
    /// words), or just that something else is running.
    pub unsaved: bool,
}

impl Default for HeightState {
    fn default() -> Self {
        Self {
            running: false,
            can_cancel: false,
            apply: false,
            copy: true,
            unsaved: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtrusionRow {
    pub feature: ObjectId,
    pub label: String,
    pub distance_mm: Option<f64>,
    pub refusal: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EditExtrudeForm {
    pub features: Vec<ExtrusionRow>,
    pub selected: Option<ObjectId>,
    pub distance: String,
    pub refusal: Option<String>,
}

pub fn edit_extrude_panel(
    ui: &mut egui::Ui,
    can_begin: bool,
    unavailable: Option<&str>,
    form: Option<&mut EditExtrudeForm>,
    status: &str,
    offer: HeightState,
) -> EditChoice {
    let HeightState {
        running,
        can_cancel,
        ..
    } = offer;
    let mut choice = EditChoice::Waiting;
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                can_begin && unavailable.is_none(),
                egui::Button::new("Edit extrusion…"),
            )
            .clicked()
        {
            choice = EditChoice::Begin;
        }
        if let Some(reason) = unavailable {
            ui.label(reason);
        }
        if running
            && ui
                .add_enabled(can_cancel, egui::Button::new("Cancel edit"))
                .clicked()
        {
            choice = EditChoice::Cancel;
        }
    });
    if !status.is_empty() {
        ui.label(status);
    }
    if let Some(form) = form {
        ui.group(|ui| {
            ui.strong("Edit extrusion");
            ui.label("Choose the existing extrusion to change:");
            egui::ScrollArea::vertical()
                .id_salt("height-features")
                .max_height(160.)
                .show(ui, |ui| {
                    for feature in &form.features {
                        let selected = form.selected == Some(feature.feature);
                        let response = ui.add_enabled(
                            feature.refusal.is_none(),
                            egui::Button::selectable(selected, &feature.label),
                        );
                        if response.clicked() {
                            form.selected = Some(feature.feature);
                            form.distance = feature
                                .distance_mm
                                .map(|v| v.to_string())
                                .unwrap_or_default();
                            form.refusal = None;
                        }
                        if let Some(reason) = &feature.refusal {
                            ui.label(reason);
                        }
                    }
                });
            if let Some(selected) = form
                .features
                .iter()
                .find(|f| Some(f.feature) == form.selected)
            {
                // §28L: the history of up to four Fillets is long; it scrolls in
                // a bounded area so Save and Cancel below stay in reach.
                if let Some(context) = &selected.context {
                    egui::ScrollArea::vertical()
                        .id_salt("height-context")
                        .max_height(96.)
                        .show(ui, |ui| {
                            ui.label(context);
                        });
                }
                if let Some(value) = selected.distance_mm {
                    ui.label(format!("Current distance: {value} mm"));
                }
                ui.horizontal(|ui| {
                    ui.label("New distance");
                    ui.add(egui::TextEdit::singleline(&mut form.distance).desired_width(110.0));
                    ui.label("mm");
                });
            }
            ui.label(
                "Apply changes the open model and keeps this model’s identities; the file on \
                 disk changes only when you Save.",
            );
            if !offer.copy {
                ui.label(if offer.unsaved {
                    "Saving a copy as a new file is unavailable while the document has unsaved \
                     changes: Save or Undo them first."
                } else {
                    "Saving a copy as a new file is unavailable while another operation is \
                     running."
                });
            }
            if let Some(reason) = &form.refusal {
                egui::ScrollArea::vertical()
                    .id_salt("height-refusal")
                    .max_height(72.)
                    .show(ui, |ui| {
                        ui.colored_label(ui.visuals().error_fg_color, reason);
                    });
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        offer.apply && form.selected.is_some() && form.refusal.is_none(),
                        egui::Button::new("Apply"),
                    )
                    .clicked()
                {
                    choice = EditChoice::Apply;
                }
                if ui
                    .add_enabled(
                        offer.copy && form.selected.is_some() && form.refusal.is_none(),
                        egui::Button::new("Save new file…"),
                    )
                    .clicked()
                {
                    choice = EditChoice::Save;
                }
                if ui.button("Cancel").clicked() {
                    choice = EditChoice::Cancel;
                }
            });
        });
    }
    choice
}
