// SPDX-License-Identifier: MIT
//! One saved-object form, painting its disposable drawing over the viewport.
mod draft;
mod geometry;
mod integration;
mod paint;
pub(crate) use integration::sync;

#[cfg(test)]
mod tests;

use crate::{sketch::accepted::Accepted, sketch_facts::Source};
use draft::{Draft, Pending, Plane, Tool};
use ferritecad_document::{DocumentVersion, ObjectPayload, Point2, SketchCurve};
use ferritecad_jobs::EditRequest;
use ferritecad_types::{ObjectId, StableEntityId};
use ferritecad_viewport::Camera;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<Draft>,
    source: PathBuf,
    version: Option<DocumentVersion>,
    pending: Option<EditRequest>,
    look: bool,
    cursor: Option<Point2>,
    snapped: Option<Point2>,
    hovered: Option<StableEntityId>,
    status: String,
    can_apply: bool,
}

impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some()
    }

    pub(crate) fn set_apply(&mut self, available: bool) {
        self.can_apply = available;
    }

    pub(crate) fn begin_with(
        &mut self,
        path: &Path,
        source: &Source,
        accepted: &Accepted,
        id: ObjectId,
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
        let ObjectPayload::Sketch(stored) = &object.payload else {
            return false;
        };
        let mut draft = Draft::new(
            object.name.clone().unwrap_or_else(|| "Sketch".into()),
            vec![Plane {
                id: stored.plane,
                name: source
                    .objects
                    .iter()
                    .find(|o| o.id == stored.plane)
                    .and_then(|o| o.name.clone())
                    .unwrap_or_else(|| "Datum plane".into()),
                frame: Some(presentation.plane()),
                addition: None,
            }],
            None,
        );
        draft.sketch = id;
        draft.original = Some(stored.clone());
        draft.accepted = presentation
            .curves()
            .iter()
            .map(|c| SketchCurve {
                id: c.id(),
                construction: c.is_construction(),
                geometry: c.geometry().clone(),
            })
            .collect();
        draft.state.curves = draft.accepted.clone();
        self.open(path, source.version, draft);
        true
    }

    pub(crate) fn begin_new(
        &mut self,
        path: &Path,
        source: &Source,
        plane: Option<ObjectId>,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let planes = draft::planes(&source.objects, &source.datum_planes);
        if plane.is_some_and(|id| !planes.iter().any(|p| p.id == id)) {
            return false;
        }
        self.open(
            path,
            source.version,
            Draft::new(draft::sketch_name(&source.objects), planes, plane),
        );
        true
    }

    fn open(&mut self, path: &Path, version: DocumentVersion, draft: Draft) {
        *self = Self {
            draft: Some(draft),
            source: path.to_path_buf(),
            version: Some(version),
            look: true,
            ..Self::default()
        };
    }

    pub(crate) fn history(&mut self, undo: bool) {
        if let Some(draft) = &mut self.draft {
            draft.history(undo);
        }
    }

    pub(crate) fn take_request(&mut self) -> Option<EditRequest> {
        self.pending.take()
    }

    pub(crate) fn filter(&self) -> Option<Option<ObjectId>> {
        self.draft
            .as_ref()
            .map(|d| d.original.as_ref().map(|_| d.sketch))
    }

    pub(crate) fn look(&mut self, input: &mut ferritecad_ui::ViewportInput) {
        if !std::mem::take(&mut self.look) {
            return;
        }
        if let Some(plane) = self.draft.as_ref().and_then(|d| d.planes[d.plane].frame) {
            let n = plane.normal();
            let y = plane.y_axis();
            input.look_along(
                [n.x as f32, n.y as f32, n.z as f32],
                [y.x as f32, y.y as f32, y.z as f32],
            );
        }
    }

    pub(crate) fn pointer(&mut self, camera: &Camera, at: [f64; 2], click: bool, shift: bool) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        let Some(plane) = draft.planes[draft.plane].frame else {
            self.status = "The selected plane is not built.".into();
            return;
        };
        self.hovered = geometry::pick(&draft.state.curves, camera, plane, at);
        let Some(point) = geometry::point_on_plane(camera, at, plane) else {
            self.cursor = None;
            self.snapped = None;
            self.status = geometry::EDGE_ON.into();
            return;
        };
        let pending = match draft.pending {
            Some(Pending::Line { first, last }) => vec![first, last],
            Some(Pending::Circle(center)) => vec![center],
            None => Vec::new(),
        };
        self.snapped = geometry::snap(
            geometry::targets(&draft.state.curves)
                .chain(geometry::targets(&draft.accepted))
                .chain(pending),
            camera,
            plane,
            at,
        );
        let point = self.snapped.unwrap_or(point);
        self.cursor = Some(point);
        self.status.clear();
        if click {
            match draft.tool {
                Tool::Select => draft.select(self.hovered, shift),
                Tool::Line | Tool::Circle | Tool::Point => draft.click(point),
            }
        }
    }

    pub(crate) fn clear_pointer(&mut self) {
        self.cursor = None;
        self.snapped = None;
        self.hovered = None;
    }

    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, running: bool, outcome: &str) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        let mut finish = false;
        let mut cancel = false;
        ui.separator();
        ui.heading("Sketch drawing");
        ui.label(&draft.name);
        ui.add_enabled_ui(!running, |ui| {
            if draft.original.is_none() {
                ui.add_enabled_ui(
                    draft.state.curves.is_empty() && draft.pending.is_none(),
                    |ui| {
                        egui::ComboBox::from_label("Plane")
                            .selected_text(&draft.planes[draft.plane].name)
                            .show_ui(ui, |ui| {
                                for (i, plane) in draft.planes.iter().enumerate() {
                                    if ui
                                        .selectable_value(&mut draft.plane, i, &plane.name)
                                        .changed()
                                    {
                                        self.look = true;
                                    }
                                }
                            });
                    },
                );
            } else {
                ui.label(format!("Plane: {}", draft.planes[draft.plane].name));
            }
            ui.horizontal_wrapped(|ui| {
                for tool in [Tool::Select, Tool::Line, Tool::Circle, Tool::Point] {
                    if ui
                        .selectable_value(&mut draft.tool, tool, tool.label())
                        .changed()
                    {
                        draft.pending = None;
                    }
                }
            });
            ui.checkbox(&mut draft.construction, "Construction");
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !draft.state.selected.is_empty(),
                        egui::Button::new("Delete"),
                    )
                    .clicked()
                {
                    draft.delete();
                }
                if ui
                    .add_enabled(
                        !draft.state.selected.is_empty(),
                        egui::Button::new("Construction"),
                    )
                    .clicked()
                {
                    draft.toggle();
                }
                if ui
                    .add_enabled(!draft.undo.is_empty(), egui::Button::new("Undo"))
                    .clicked()
                {
                    draft.history(true);
                }
                if ui
                    .add_enabled(!draft.redo.is_empty(), egui::Button::new("Redo"))
                    .clicked()
                {
                    draft.history(false);
                }
            });
            if !ui.ctx().egui_wants_keyboard_input() {
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                    draft.escape();
                }
                if [egui::Key::Delete, egui::Key::Backspace]
                    .iter()
                    .any(|key| ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, *key)))
                {
                    draft.delete();
                }
            }
            if ui.button("Look at sketch").clicked() {
                self.look = true;
            }
            let (added, removed, changed) = draft.counts();
            let count = draft.state.curves.len();
            let noun = if count == 1 { "curve" } else { "curves" };
            ui.label(format!(
                "{count} {noun}: {added} added, {removed} removed, {changed} changed"
            ));
            let constraints = draft.removed_constraints().len();
            if constraints != 0 {
                let noun = if constraints == 1 {
                    "constraint"
                } else {
                    "constraints"
                };
                ui.label(format!("Removes {constraints} {noun} of deleted curves."));
            }
            ui.horizontal(|ui| {
                finish = ui
                    .add_enabled(self.can_apply, egui::Button::new("Finish"))
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if let Some(p) = self.cursor {
            ui.label(format!("{:.6}, {:.6} mm", p.x, p.y));
        }
        ui.label(&self.status);
        ui.label(outcome);
        if finish {
            let changes = draft.changes();
            if changes.is_empty() {
                cancel = true;
            } else {
                self.pending = Some(EditRequest {
                    destination: PathBuf::new(),
                    source: self.source.clone(),
                    expected: self.version.expect("an open form has a version"),
                    changes,
                });
            }
        }
        if cancel {
            *self = Self::default();
        }
    }
}
