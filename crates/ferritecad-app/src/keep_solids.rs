// SPDX-License-Identifier: MIT
use crate::sketch_facts::Source;
use ferritecad_document::{
    EditChange, EntityKind, EntityReference, FeatureAddition, KeepSelection, ObjectPayload,
    ReferenceMeaning, Selection,
};
use ferritecad_jobs::EditRequest;
use ferritecad_types::ObjectId;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub(crate) struct Commands {
    selected: Option<Result<EntityReference, String>>,
    ready: bool,
    pending: Option<EditRequest>,
    refusal: Option<String>,
}

impl Commands {
    pub(crate) fn update(
        &mut self,
        scene: &crate::LiveScene<crate::PreparedSnapshot>,
        sessions: &crate::sessions::Sessions,
        ready: bool,
    ) {
        self.ready = ready;
        self.selected = Some(
            (|| {
                let current = sessions.export_source().ok_or("no open document")?;
                let shown = scene
                    .edit_source
                    .as_ref()
                    .ok_or("the scene has no document version")?;
                if shown.version != current.version() {
                    return Err("the scene is older than the document; wait for the current scene");
                }
                let ferritecad_scene::Selection::Face(face) = &scene.selection else {
                    return Err("select a named face in the viewport first");
                };
                let [meaning] = face.meanings() else {
                    return Err(
                        "this face has no single stored meaning; the piece cannot be selected \
                            individually through this witness. Choose another face or All",
                    );
                };
                Ok(EntityReference::new(ReferenceMeaning {
                    producer: meaning.producer,
                    kind: EntityKind::Face,
                    digest: meaning.digest,
                    selection: Selection::One,
                }))
            })()
            .map_err(str::to_owned),
        );
    }

    pub(crate) fn take_request(&mut self) -> Option<EditRequest> {
        self.pending.take()
    }

    fn request(&mut self, path: &Path, source: &Source, target: Target, all: bool) {
        let selection = if all {
            KeepSelection::All
        } else {
            match self.selected.as_ref() {
                Some(Ok(face)) => KeepSelection::ContainingFaces(vec![*face]),
                Some(Err(reason)) => {
                    self.refusal = Some(reason.clone());
                    return;
                }
                None => {
                    self.refusal = Some("select a named face in the viewport first".into());
                    return;
                }
            }
        };
        self.pending = Some(EditRequest {
            source: path.to_path_buf(),
            expected: source.version,
            changes: vec![match target {
                Target::Body(body) => EditChange::AddFeature {
                    object: ObjectId::new(),
                    body,
                    name: Some("Keep solids".into()),
                    feature: FeatureAddition::KeepSolids { selection },
                },
                Target::Feature(object) => EditChange::SetKeepSelection { object, selection },
            }],
            destination: PathBuf::new(),
        });
        self.refusal = None;
    }

    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, path: Option<&Path>, source: Option<&Source>) {
        let (Some(path), Some(source)) = (path, source) else {
            return;
        };
        ui.collapsing("Keep solid pieces", |ui| {
            ui.label("Select a face, then keep its containing solid. All restores the full set.");
            ui.small(
                "A piece without an unambiguous face witness cannot be selected individually.",
            );
            for object in &source.objects {
                let (target, label) = match &object.payload {
                    ObjectPayload::Body(body) => {
                        let Some(tip) = body.tip_feature else {
                            continue;
                        };
                        if source.objects.iter().any(|o| {
                            o.id == tip && matches!(o.payload, ObjectPayload::KeepSolids(_))
                        }) {
                            continue;
                        }
                        (Target::Body(object.id), "Body")
                    }
                    ObjectPayload::KeepSolids(_) => (Target::Feature(object.id), "KeepSolids"),
                    _ => continue,
                };
                ui.push_id(object.id, |ui| {
                    ui.label(format!(
                        "{label} {} — {}",
                        object.name.as_deref().unwrap_or("Unnamed"),
                        object.id
                    ));
                    if let Some(row) = source.model_tree.rows.iter().find(|r| r.id.0 == object.id) {
                        ui.small(&row.note);
                    }
                    ui.add_enabled_ui(self.ready && source.refusal.is_none(), |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Keep selected piece").clicked() {
                                self.request(path, source, target, false);
                            }
                            if ui.button("All").clicked() {
                                self.request(path, source, target, true);
                            }
                        });
                    });
                });
            }
            if let Some(reason) = &self.refusal {
                ui.colored_label(ui.visuals().error_fg_color, reason);
            }
            ui.small(
                "Changes remain unsaved until Save or Save As. Undo/Redo restores the selection.",
            );
        });
    }
}

#[derive(Clone, Copy)]
enum Target {
    Body(ObjectId),
    Feature(ObjectId),
}
