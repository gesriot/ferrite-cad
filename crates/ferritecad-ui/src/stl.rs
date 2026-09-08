// SPDX-License-Identifier: MIT
//! Choosing one durable Body and tessellation settings. No document or kernel.

use ferritecad_types::ObjectId;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StlChoice {
    #[default]
    Waiting,
    Begin,
    Save,
    Cancel,
}

#[derive(Debug, Clone)]
pub struct StlBodyRow {
    pub id: ObjectId,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct StlExportForm {
    pub bodies: Vec<StlBodyRow>,
    pub selected: Option<ObjectId>,
    pub linear_mm: String,
    pub angular_rad: String,
    pub error: Option<String>,
}

pub fn stl_export_form(ui: &mut egui::Ui, form: Option<&mut StlExportForm>) -> StlChoice {
    let Some(form) = form else {
        return StlChoice::Waiting;
    };
    let mut choice = StlChoice::Waiting;
    ui.group(|ui| {
        ui.strong("Export STL — one native body");
        ui.label("Choose the body to export:");
        egui::ScrollArea::vertical()
            .id_salt("stl bodies")
            .max_height(150.0)
            .show(ui, |ui| {
                for body in &form.bodies {
                    if ui
                        .selectable_label(form.selected == Some(body.id), &body.label)
                        .clicked()
                    {
                        form.selected = Some(body.id);
                        form.error = None;
                    }
                }
            });
        egui::Grid::new("stl tessellation").show(ui, |ui| {
            ui.label("Linear deflection (mm)");
            ui.add(egui::TextEdit::singleline(&mut form.linear_mm).desired_width(100.0));
            ui.end_row();
            ui.label("Angular deflection (rad)");
            ui.add(egui::TextEdit::singleline(&mut form.angular_rad).desired_width(100.0));
            ui.end_row();
        });
        ui.label(
            "Reads the saved file again. Exports the whole chosen body, including hidden geometry.",
        );
        if let Some(error) = &form.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(form.selected.is_some(), egui::Button::new("Save STL…"))
                .clicked()
            {
                choice = StlChoice::Save;
            }
            if ui.button("Cancel STL").clicked() {
                choice = StlChoice::Cancel;
            }
        });
    });
    choice
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishedStl<'a> {
    pub destination: &'a str,
    pub body: &'a str,
    pub triangles: usize,
    pub bytes: usize,
}
