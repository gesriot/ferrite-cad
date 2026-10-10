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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelAction {
    pub tab: u64,
    pub epoch: u64,
    pub row: ModelKey,
    pub edit: ModelEdit,
}

#[derive(Debug)]
pub struct ModelRow<'a> {
    pub key: ModelKey,
    pub parent: Option<ModelKey>,
    pub depth: usize,
    pub name: &'a str,
    pub kind: &'a str,
    pub note: &'a str,
}

/// Pure navigation plus an explicitly addressed request. `edits_for` supplies the
/// accepted catalogue's supported actions for the currently selected UUID.
pub fn model_panel(
    ui: &mut egui::Ui,
    tab: u64,
    rows: &[ModelRow<'_>],
    navigation: &mut ModelNavigation,
    edits_for: impl Fn(ModelKey) -> Vec<ModelEdit>,
    available: Result<(), &str>,
) -> Option<ModelAction> {
    ui.heading("Model");
    if rows.is_empty() {
        ui.label("No stored model objects");
        return None;
    }
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
                        if ui
                            .selectable_label(
                                navigation.selected == Some(row.key),
                                format!(
                                    "{}{} · {}",
                                    if row.key.1.is_some() { "Ref: " } else { "" },
                                    row.name,
                                    row.kind
                                ),
                            )
                            .on_hover_text(format!("{}\n{}", row.note, row.key.0))
                            .clicked()
                        {
                            navigation.selected = Some(row.key);
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
    let edits = edits_for(row.key);
    if edits.is_empty() {
        ui.label("No supported editor for this object.");
    }
    if let Err(reason) = available {
        ui.label(reason);
    }
    let mut asked = None;
    for edit in &edits {
        if ui
            .add_enabled(available.is_ok(), egui::Button::new(edit.label()))
            .clicked()
        {
            asked = Some(ModelAction {
                tab,
                epoch: navigation.epoch,
                row: row.key,
                edit: *edit,
            });
        }
    }
    asked
}
