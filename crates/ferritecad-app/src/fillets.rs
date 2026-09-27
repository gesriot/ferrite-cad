// SPDX-License-Identifier: MIT
//! A disposable single-edge fillet request over stored facts; no kernel, no
//! file reads (§28A).
//!
//! The form offers only what the document says can be rounded: the four
//! vertical edges of one saved rectangular plate, each named by the base
//! Extrude and the two Lines that meet there. It lists them rather than
//! letting a pointer pick geometry, because this slice has no picking, and a
//! control that suggested otherwise would be promising it.
use ferritecad_document::{
    DocumentVersion, EdgeFillet, ExtrudeEditSource, FilletChoice, FilletCorner, FilletEdge,
};
use ferritecad_jobs::EdgeFilletRequest;
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
            edge: FilletEdge {
                feature: corner.feature,
                joint: corner.joint,
            },
            radius_mm,
        };
        let corner = self.choice.validate(&fillet)?;
        Ok((corner, fillet))
    }
}

#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<Draft>,
    pending: Option<EdgeFilletRequest>,
}

impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some()
    }
    pub(crate) fn dismiss(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn take_request(&mut self) -> Option<EdgeFilletRequest> {
        self.pending.take()
    }

    fn begin(&mut self, path: &Path, source: &ExtrudeEditSource, body: ObjectId) -> bool {
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
            let response = ui.add_enabled(
                can_begin && refusal.is_none(),
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
    }

    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, running: bool) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        let Some(target) = draft.choice.target.clone() else {
            return;
        };
        let mut cancel = false;
        egui::Window::new("Fillet one vertical edge — new copy")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
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
                    draft.source.display()
                ));
                ui.small(format!(
                    "Radius from {} mm to {} × the shorter adjacent side.",
                    ferritecad_document::MIN_RADIUS_MM,
                    ferritecad_document::MAX_RADIUS_FRACTION
                ));
                ui.add_enabled_ui(!running, |ui| {
                    if ui.button("Cancel fillet draft").clicked() {
                        cancel = true;
                    }
                    ui.label("Edge:");
                    for (i, corner) in target.corners.iter().enumerate() {
                        ui.radio_value(&mut draft.typed.corner, Some(i), describe(corner));
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
                    if ui.button("Apply fillet").clicked() {
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
                            ui.small(format!(
                                "Ready: round the edge at ({}, {}) with r{} mm",
                                corner.corner_mm[0], corner.corner_mm[1], fillet.radius_mm
                            ));
                            if ui.button("Save fillet copy…").clicked() {
                                self.pending = Some(EdgeFilletRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,
                                    body: target.body,
                                    fillet,
                                    destination: PathBuf::new(),
                                });
                            }
                        }
                        None => {
                            ui.small("Apply an edge and a radius before saving.");
                        }
                    }
                });
                if running {
                    ui.label("Saving… Draft retained until publication. Cancel job in toolbar.");
                }
            });
        if cancel {
            self.dismiss();
        }
    }
}

/// One candidate as the form lists it: where, which Lines, and the limit.
fn describe(corner: &FilletCorner) -> String {
    let [a, b] = corner.joint.segments();
    format!(
        "Corner ({}, {}) — Lines {a} | {b}; sides {} × {} mm; r ≤ {} mm",
        corner.corner_mm[0],
        corner.corner_mm[1],
        corner.adjacent_lengths_mm[0],
        corner.adjacent_lengths_mm[1],
        corner.max_radius_mm
    )
}

/// The same two steps every published copy takes: the draft is handed to the
/// shared retention so an Open that is later refused can give it back.
pub(crate) fn finish_fillet(
    editor: &mut crate::sketch::Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::AddedEdgeFillet>,
) -> Option<PathBuf> {
    if edits.accepts(generation)
        && let Ok(saved) = &result
    {
        editor.draft_published(&saved.destination);
    }
    edits.finish_fillet(generation, result)
}
