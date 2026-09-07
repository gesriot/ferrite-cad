// SPDX-License-Identifier: MIT
//! A choice of stored feature, and a distance in mm. No document arithmetic.

use ferritecad_types::ObjectId;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum EditChoice {
    #[default]
    Waiting,
    Begin,
    Save,
    Cancel,
}

#[derive(Debug, Clone)]
pub struct ExtrusionRow {
    pub feature: ObjectId,
    pub label: String,
    pub distance_mm: Option<f64>,
    pub refusal: Option<String>,
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
    running: bool,
    can_cancel: bool,
    status: &str,
) -> EditChoice {
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
            ui.strong("Edit extrusion — save a new file");
            ui.label("Choose the existing extrusion to change:");
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
            if let Some(selected) = form
                .features
                .iter()
                .find(|f| Some(f.feature) == form.selected)
            {
                if let Some(value) = selected.distance_mm {
                    ui.label(format!("Current distance: {value} mm"));
                }
                ui.horizontal(|ui| {
                    ui.label("New distance");
                    ui.add(egui::TextEdit::singleline(&mut form.distance).desired_width(110.0));
                    ui.label("mm");
                });
            }
            ui.label("The source stays unchanged. The new file keeps this model’s identities.");
            if let Some(reason) = &form.refusal {
                ui.colored_label(ui.visuals().error_fg_color, reason);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(form.selected.is_some(), egui::Button::new("Save new file…"))
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
