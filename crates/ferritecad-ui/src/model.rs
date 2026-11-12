// SPDX-License-Identifier: MIT
//! Navigation is separate from viewport selection and changes no model.

use ferritecad_types::ObjectId;
use std::collections::HashSet;

pub type ModelKey = (ObjectId, Option<ObjectId>, Option<ObjectId>);

#[derive(Debug, Default)]
pub struct ModelNavigation {
    pub selected: Option<ModelKey>,
    pub collapsed: HashSet<ModelKey>,
    pub epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelEdit {
    Height,
    Vertices,
    Circle,
    Annulus,
    Constraints,
    Cut,
    Fillet,
    Chamfer,
    RevolveAngle,
    /// Edit parameter names and expression text in the document's Parameters form.
    Parameters,
}

impl ModelEdit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Height => "Edit height…",
            Self::Vertices => "Edit vertices…",
            Self::Circle => "Edit circle…",
            Self::Annulus => "Edit annulus…",
            Self::Constraints => "Edit constraints…",
            Self::Cut => "Edit Cut…",
            Self::Fillet => "Edit Fillet radius…",
            Self::Chamfer => "Edit Chamfer distance…",
            Self::RevolveAngle => "Edit Revolve angle…",
            Self::Parameters => "Edit…",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelAction<C = ModelEdit> {
    pub tab: u64,
    pub epoch: u64,
    pub row: ModelKey,
    pub edit: C,
}

#[derive(Debug)]
pub struct ModelRow<'a> {
    pub key: ModelKey,
    pub parent: Option<ModelKey>,
    pub depth: usize,
    pub name: &'a str,
    pub kind: &'a str,
    pub note: &'a str,
    pub solid_count: Option<u32>,
}

/// Pure navigation plus an explicitly addressed request. `commands_for` supplies the
/// accepted catalogue's supported actions for the currently selected UUID.
pub fn model_panel<C: Copy>(
    ui: &mut egui::Ui,
    tab: u64,
    rows: &[ModelRow<'_>],
    navigation: &mut ModelNavigation,
    commands_for: impl Fn(ModelKey) -> Vec<ModelCommand<C>>,
) -> Option<ModelAction<C>> {
    ui.heading("Model");
    if rows.is_empty() {
        ui.label("No stored model objects");
        return None;
    }
    let mut asked = None;
    let mut hidden = HashSet::new();
    egui::ScrollArea::vertical()
        .id_salt(("model-rows", tab))
        .max_height((ui.available_height() * 0.60).clamp(80., 360.))
        .show(ui, |ui| {
            let mut objects = false;
            for row in rows {
                if row.parent.is_none() && row.kind != "body" && !objects {
                    ui.separator();
                    ui.label("Objects and references");
                    objects = true;
                }
                if row
                    .parent
                    .is_some_and(|p| hidden.contains(&p) || navigation.collapsed.contains(&p))
                {
                    hidden.insert(row.key);
                    continue;
                }
                ui.push_id((tab, row.key), |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(row.depth as f32 * 12.);
                        if rows.iter().any(|r| r.parent == Some(row.key)) {
                            let collapsed = navigation.collapsed.contains(&row.key);
                            // Paint the disclosure arrow: these Unicode glyphs are
                            // not present in every bundled font.
                            let (_, toggle) = ui.allocate_exact_size(
                                egui::vec2(18., ui.spacing().interact_size.y),
                                egui::Sense::click(),
                            );
                            toggle.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    if collapsed { "Expand" } else { "Collapse" },
                                )
                            });
                            egui::collapsing_header::paint_default_icon(
                                ui,
                                if collapsed { 0. } else { 1. },
                                &toggle,
                            );
                            if toggle.clicked() {
                                if collapsed {
                                    navigation.collapsed.remove(&row.key);
                                } else {
                                    navigation.collapsed.insert(row.key);
                                }
                            }
                        }
                        let response = ui
                            .selectable_label(
                                navigation.selected == Some(row.key),
                                format!(
                                    "{}{} · {}",
                                    if row.key.1.is_some() { "Ref: " } else { "" },
                                    row.name,
                                    row.kind
                                ),
                            )
                            .on_hover_text(format!("{}\n{}", row.note, row.key.0));
                        if response.clicked() {
                            navigation.selected = Some(row.key);
                        }
                        if response.secondary_clicked() {
                            navigation.selected = Some(row.key);
                        }
                        response.context_menu(|ui| {
                            if let Some(edit) = model_commands(ui, &commands_for(row.key)) {
                                asked = Some(ModelAction {
                                    tab,
                                    epoch: navigation.epoch,
                                    row: row.key,
                                    edit,
                                });
                                ui.close();
                            }
                        });
                        if let Some(count) = row.solid_count {
                            let noun = if count == 1 { "solid" } else { "solids" };
                            ui.small(format!("{count} {noun}"));
                        }
                    });
                });
            }
        });
    let row = rows.iter().find(|r| Some(r.key) == navigation.selected)?;
    ui.separator();
    ui.label(format!("{} · {}", row.name, row.kind));
    ui.label(row.note);
    egui::CollapsingHeader::new("UUID")
        .id_salt((tab, row.key, "uuid"))
        .show(ui, |ui| {
            ui.monospace(row.key.0.to_string());
        });
    let commands = commands_for(row.key);
    if let Some(edit) = model_commands(ui, &commands) {
        asked = Some(ModelAction {
            tab,
            epoch: navigation.epoch,
            row: row.key,
            edit,
        });
    }
    if ui.is_enabled() && !ui.ctx().egui_wants_keyboard_input() {
        for command in &commands {
            if command.available.is_ok()
                && command
                    .shortcuts
                    .iter()
                    .any(|shortcut| ui.input_mut(|input| input.consume_shortcut(shortcut)))
            {
                asked = Some(ModelAction {
                    tab,
                    epoch: navigation.epoch,
                    row: row.key,
                    edit: command.command,
                });
            }
        }
    }
    asked
}

/// Presentation supplied by the command owner, shared by buttons and menus.
#[derive(Debug)]
pub struct ModelCommand<C> {
    pub command: C,
    pub label: &'static str,
    /// Equivalent keys defined by the command owner, in display order.
    pub shortcuts: &'static [egui::KeyboardShortcut],
    pub available: Result<(), String>,
}

pub fn model_commands<C: Copy>(ui: &mut egui::Ui, commands: &[ModelCommand<C>]) -> Option<C> {
    let mut asked = None;
    for command in commands {
        let mut button = egui::Button::new(command.label);
        if !command.shortcuts.is_empty() {
            button = button.shortcut_text(
                command
                    .shortcuts
                    .iter()
                    .map(|key| ui.ctx().format_shortcut(key))
                    .collect::<Vec<_>>()
                    .join(" / "),
            );
        }
        let response = ui.add_enabled(command.available.is_ok(), button);
        if response.clicked() {
            asked = Some(command.command);
        }
        if let Err(reason) = &command.available {
            response.on_disabled_hover_text(reason);
        }
    }
    asked
}
