// SPDX-License-Identifier: MIT
use ferritecad_document::ObjectPayload;
use ferritecad_ui::{ModelAction, ModelNavigation};

use super::{Draft, EditChange, Editor, Field, ObjectId, Request, Source, parameter, words};
use crate::commands::{self, Access, Availability, Command};

impl Editor {
    pub(crate) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        source: &Source,
        tab: u64,
        navigation: &mut ModelNavigation,
        available: Availability,
        chosen: &mut commands::Chosen,
    ) {
        let open = available == Ok(Access::Idle) && source.refusal.is_none();
        let button = ui.add_enabled(open, egui::Button::new("Parameters…"));
        if button.clicked() {
            self.begin(source);
        }
        if let Some(reason) = source.refusal.as_deref().or_else(|| available.err()) {
            button.on_disabled_hover_text(reason);
        }
        self.document_arrived(Some(source));
        let Some(draft) = &mut self.draft else { return };
        draft.select(navigation.selected, source);
        let can_apply = available.and_then(Access::operation).is_ok()
            && source.refusal.is_none()
            && draft.pending.is_none();
        let mut close = false;
        let height = ui.ctx().content_rect().height() * 0.75;
        crate::form_window(ui, "Parameters")
            .default_width(440.)
            .max_height(height)
            .show(ui.ctx(), |ui| {
                if ui
                    .add_enabled(draft.pending.is_none(), egui::Button::new("Close"))
                    .clicked()
                {
                    close = true;
                }
                egui::ScrollArea::vertical()
                    .id_salt("parameter fields")
                    .max_height((height - 48.).max(0.))
                    .show(ui, |ui| {
                        draft.table(ui, source, navigation);
                        // A table click is a tree selection, visible to the fields in this frame.
                        draft.select(navigation.selected, source);
                        if let Some(row) = navigation.selected
                            && parameter(source, row.0).is_some()
                        {
                            let offers: Vec<_> = commands::offers(source, row, available)
                                .into_iter()
                                .filter(|offer| {
                                    matches!(
                                        offer.command,
                                        Command::Delete | Command::DeleteWithDependents
                                    )
                                })
                                .collect();
                            if let Some(edit) = ferritecad_ui::model_commands(ui, &offers) {
                                chosen.model = Some(ModelAction {
                                    tab,
                                    epoch: navigation.epoch,
                                    row,
                                    edit,
                                });
                            }
                        }
                        ui.add_enabled_ui(can_apply, |ui| {
                            chosen.parameter = draft.fields(ui, source);
                        });
                    });
            });
        if close {
            self.draft = None;
        }
    }
}

impl Draft {
    fn table(&self, ui: &mut egui::Ui, source: &Source, navigation: &mut ModelNavigation) {
        let mut rows: Vec<_> = source
            .objects
            .iter()
            .filter_map(|object| {
                if let ObjectPayload::Parameter(parameter) = &object.payload {
                    Some((object.id, parameter))
                } else {
                    None
                }
            })
            .collect();
        rows.sort_by(|(_, a), (_, b)| a.name.as_bytes().cmp(b.name.as_bytes()));
        egui::ScrollArea::vertical()
            .id_salt("parameter rows")
            .max_height(144.)
            .show(ui, |ui| {
                egui::Grid::new("parameters")
                    .striped(true)
                    .max_col_width(180.)
                    .show(ui, |ui| {
                        for label in ["Name", "Expression", "Value", "Consumers"] {
                            ui.strong(label);
                        }
                        ui.end_row();
                        for (id, parameter) in rows {
                            if ui
                                .selectable_label(self.selected == Some(id), &parameter.name)
                                .clicked()
                                && let Some(row) =
                                    source.model_tree.rows.iter().find(|r| r.id.0 == id)
                            {
                                commands::select_row(source, navigation, row.id);
                            }
                            ui.label(&parameter.expression.source);
                            ui.label(
                                source
                                    .unbuilt_statuses
                                    .iter()
                                    .find(|(object, _)| *object == id)
                                    .map(|(_, reason)| reason.clone())
                                    .unwrap_or_else(|| {
                                        words::value(
                                            parameter.expression.value(),
                                            parameter.dimension,
                                        )
                                    }),
                            );
                            match &source.parameter_consumers {
                                Ok(consumers) => {
                                    let count = consumers
                                        .iter()
                                        .filter(|c| c.parameter == id)
                                        .map(|c| c.object)
                                        .collect::<std::collections::BTreeSet<_>>()
                                        .len();
                                    ui.label(count.to_string());
                                }
                                Err(reason) => {
                                    ui.label("Unavailable").on_hover_text(reason);
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    fn fields(&mut self, ui: &mut egui::Ui, source: &Source) -> Option<Request> {
        let mut submitted = None;
        if let Some(id) = self.selected {
            if let Some(saved) = parameter(source, id) {
                ui.separator();
                self.consumers(ui, source, id);
                ui.label("Parameter name");
                ui.text_edit_singleline(&mut self.name);
                if ui
                    .add_enabled(self.name != saved.name, egui::Button::new("Rename"))
                    .clicked()
                {
                    submitted = Some((
                        Field::Name(id),
                        EditChange::RenameParameter {
                            object: id,
                            name: self.name.clone(),
                        },
                    ));
                }
                self.error(ui, Field::Name(id));
                ui.label("Parameter expression");
                ui.text_edit_singleline(&mut self.text);
                if ui
                    .add_enabled(
                        self.text != saved.expression.source,
                        egui::Button::new("Apply parameter"),
                    )
                    .clicked()
                {
                    submitted = Some((
                        Field::Text(id),
                        EditChange::SetParameter {
                            object: id,
                            expression: self.text.clone(),
                        },
                    ));
                }
                self.error(ui, Field::Text(id));
            }
            ui.separator();
            ui.heading("Values");
            let status = source
                .unbuilt_statuses
                .iter()
                .find(|(object, _)| *object == id)
                .map(|(_, status)| status);
            if let Some(status) = status {
                ui.label(status);
            }
            if self.slots.is_empty() {
                ui.label("The selected object has no value slots.");
            }
            for index in 0..self.slots.len() {
                let (slot, text) = &mut self.slots[index];
                let Some(saved) = source.expression_slot(id, *slot) else {
                    continue;
                };
                let field = Field::Slot(id, *slot);
                ui.push_id((id, index), |ui| {
                    ui.label(words::slot(source, id, *slot));
                    if status.is_none() {
                        ui.label(words::value(saved.expression.value(), saved.dimension));
                    }
                    ui.text_edit_singleline(text);
                    if ui
                        .add_enabled(
                            *text != saved.expression.source,
                            egui::Button::new("Apply value"),
                        )
                        .clicked()
                    {
                        submitted = Some((
                            field,
                            EditChange::SetExpressionText {
                                object: id,
                                slot: *slot,
                                source: text.clone(),
                            },
                        ));
                    }
                });
                self.error(ui, field);
            }
        } else {
            ui.label("Choose a parameter or another object in Model to edit its values.");
        }
        ui.separator();
        ui.heading("Add parameter");
        ui.label("New name");
        ui.text_edit_singleline(&mut self.add_name);
        ui.horizontal(|ui| {
            for (dimension, label) in [
                (super::Dimension::Length, "Length"),
                (super::Dimension::Angle, "Angle"),
                (super::Dimension::Scalar, "Scalar"),
            ] {
                ui.radio_value(&mut self.dimension, dimension, label);
            }
        });
        ui.label("New expression");
        ui.text_edit_singleline(&mut self.add_text);
        if ui.button("Add parameter").clicked() {
            submitted = Some((
                Field::Add,
                EditChange::AddParameter {
                    object: ObjectId::new(),
                    name: self.add_name.clone(),
                    dimension: self.dimension,
                    expression: self.add_text.clone(),
                },
            ));
        }
        self.error(ui, Field::Add);
        submitted.map(|(field, change)| Request {
            expected: source.version,
            field,
            change,
        })
    }

    fn error(&self, ui: &mut egui::Ui, field: Field) {
        if let Some((failed, reason)) = &self.refusal
            && *failed == field
        {
            ui.colored_label(ui.visuals().error_fg_color, reason);
        }
    }

    fn consumers(&self, ui: &mut egui::Ui, source: &Source, id: ObjectId) {
        ui.label("Consumers");
        match &source.parameter_consumers {
            Ok(consumers) => {
                for consumer in consumers.iter().filter(|consumer| consumer.parameter == id) {
                    if let Some(object) = source.objects.iter().find(|o| o.id == consumer.object) {
                        let slot = consumer
                            .slot
                            .map(|s| words::slot(source, object.id, s))
                            .unwrap_or_else(|| "Parameter expression".into());
                        ui.label(format!(
                            "{} · {} · {slot}",
                            object.name.as_deref().unwrap_or("Unnamed"),
                            object
                                .payload
                                .kind()
                                .map(|k| k.label())
                                .unwrap_or("Unknown object"),
                        ));
                    }
                }
            }
            Err(reason) => {
                ui.label(reason);
            }
        }
    }
}
