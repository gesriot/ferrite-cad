// SPDX-License-Identifier: MIT
use crate::feature_facts::PickedEdge;
use ferritecad_document::{EntityKind, EntityReference, ReferenceMeaning, Selection};
use ferritecad_viewport::Marked;

pub(crate) fn update(
    editor: &mut crate::sketch::Editor,
    scene: &crate::LiveScene<crate::PreparedSnapshot>,
    sessions: &crate::sessions::Sessions,
    can_begin: bool,
    apply: [bool; 2],
    add: [bool; 2],
) {
    let selected = (|| {
        let current = sessions.export_source().ok_or("no open document")?;
        let shown = scene
            .edit_source
            .as_ref()
            .ok_or("the scene has no document version")?;
        if shown.version != current.version() {
            return Err("the scene is older than the document; wait for the current scene");
        }
        let Marked::Edge(edge) = scene.selection.marked() else {
            return Err("select an edge in the viewport first");
        };
        let meanings = scene.edges.of(edge, scene.prepared.snapshot());
        let [meaning] = meanings else {
            return Err("the selected edge has no single stored meaning");
        };
        Ok(PickedEdge {
            reference: EntityReference::new(ReferenceMeaning {
                producer: meaning.feature,
                kind: EntityKind::Edge,
                digest: meaning.digest,
                selection: Selection::One,
            }),
            description: meaning.description.clone(),
        })
    })()
    .map_err(str::to_owned);
    editor.fillets.set_selected(selected.clone());
    editor.chamfers.set_selected(selected);
    editor
        .fillets
        .set_session(can_begin, apply[0], sessions.dirty());
    editor
        .chamfers
        .set_session(can_begin, apply[1], sessions.dirty());
    editor.fillets.set_add(add[0]);
    editor.chamfers.set_add(add[1]);
}

pub(crate) fn draw(
    ui: &mut egui::Ui,
    edges: &mut Vec<PickedEdge>,
    selected: Option<&Result<PickedEdge, String>>,
    refusal: &mut Option<String>,
) {
    if ui.button("Add selected edge").clicked() {
        match selected {
            Some(Ok(edge)) => {
                if edges
                    .iter()
                    .any(|saved| saved.reference.meaning == edge.reference.meaning)
                {
                    *refusal = Some("this edge meaning is already in the list".into());
                } else {
                    edges.push(edge.clone());
                    *refusal = None;
                }
            }
            Some(Err(reason)) => *refusal = Some(reason.clone()),
            None => *refusal = Some("select an edge in the viewport first".into()),
        }
    }
    let mut remove = None;
    for (index, edge) in edges.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(&edge.description);
            if ui
                .push_id(index, |ui| ui.button("Remove edge"))
                .inner
                .clicked()
            {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        edges.remove(index);
    }
}
