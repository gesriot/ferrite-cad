// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_kernel::OperationContext;
use ferritecad_scene::{SceneItem, Selection};
use ferritecad_types::ErrorKind;
use ferritecad_viewport::{
    Camera, EdgePickId, FacePickId, Hovered, Marked, StandardView, VertexPickId, Visibility,
};
use ferritecad_viewport_gpu::Renderer;
use std::sync::Arc;

fn renderer() -> Result<Option<Renderer>> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(None);
    }
    match Renderer::new() {
        Ok(renderer) => Ok(Some(renderer)),
        Err(error) if error.kind() == ErrorKind::Unsupported => {
            ferritecad_fixtures::native_dependencies(&[(false, "GPU")]);
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[test]
fn native_gpu_faces_vertices_and_fillet_edges_select_their_producer_and_expand_it() -> Result<()> {
    let Some(mut renderer) = renderer()? else {
        return Ok(());
    };
    let root = tempfile::tempdir()?;
    let path = root.path().join("three-bodies.fcad");
    let model = ferritecad_fixtures::live_model::write(&path)?;
    let (loaded, tab) =
        crate::sessions::open_for_view(root.path(), &path, &OperationContext::default())?;
    let source: ExtrudeEditSource = loaded.edit_source.expect("facts").into();
    let snapshot = Arc::new(loaded.snapshot);
    assert_eq!(snapshot.meshes().len(), 3);
    let prepared = renderer.prepare(Arc::clone(&snapshot))?;
    let mut camera = Camera::new();
    camera.resize(480, 480);
    camera.frame(snapshot.bounds().expect("bodies"))?;
    let mut face_rows = Vec::new();
    let mut picked_fillet = false;
    let mut picked_vertex = false;
    for (index, entry) in loaded.catalogue.iter().enumerate() {
        let SceneItem::Body(body) = entry.item else {
            panic!("native Body");
        };
        let mut visibility = Visibility::new(&snapshot);
        visibility.isolate(
            Marked::Definition(snapshot.pick_of(index).expect("body")),
            &snapshot,
        );
        let mut picked_face = false;
        for view in [StandardView::Top, StandardView::Front, StandardView::Left] {
            camera.look_from(view);
            let frame = renderer.render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )?;
            for (x, y) in (0..frame.height()).flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
            {
                let hit = frame.hit_at(x, y);
                let edge = loaded.edges.of(hit.edge(), &snapshot);
                let face = loaded.faces.of(hit.face(), &snapshot);
                let vertex = loaded.vertices.of(hit.vertex(), &snapshot);
                let want_face = !picked_face
                    && face.len() == 1
                    && hit.face() != FacePickId::NOTHING
                    && hit.edge() == EdgePickId::NOTHING
                    && hit.vertex() == VertexPickId::NOTHING;
                let want_edge = !picked_fillet
                    && edge.len() == 1
                    && hit.vertex() == VertexPickId::NOTHING
                    && edge.iter().any(|m| m.producer == model.first);
                let want_vertex = !picked_vertex && vertex.len() == 1;
                if !want_face && !want_edge && !want_vertex {
                    continue;
                }
                let selection = crate::selection_at(
                    hit,
                    &snapshot,
                    &loaded.faces,
                    &loaded.edges,
                    &loaded.vertices,
                );
                let meanings = match &selection {
                    Selection::Face(_) if want_face => {
                        picked_face = true;
                        face
                    }
                    Selection::Edge(_) if want_edge => {
                        picked_fillet = true;
                        edge
                    }
                    Selection::Vertex(_) if want_vertex => {
                        picked_vertex = true;
                        vertex
                    }
                    Selection::Nothing
                    | Selection::Definition(_)
                    | Selection::Face(_)
                    | Selection::Edge(_)
                    | Selection::Vertex(_) => continue,
                };
                assert_eq!(
                    meanings.len(),
                    1,
                    "ordinary native entity needs one meaning"
                );
                let row = picked_row(&source, &selection, &loaded.catalogue, &snapshot)
                    .expect("unique producer")
                    .expect("model row");
                assert_eq!(row, (meanings[0].producer, Some(body), None));
                let mut navigation = ferritecad_ui::ModelNavigation::default();
                navigation
                    .collapsed
                    .extend(source.model_tree.rows.iter().map(|r| r.id));
                select_row(&source, &mut navigation, row);
                assert_eq!(navigation.selected, Some(row));
                assert!(!navigation.collapsed.contains(&row));
                let mut parent = source
                    .model_tree
                    .rows
                    .iter()
                    .find(|r| r.id == row)
                    .expect("row")
                    .parent;
                while let Some(key) = parent {
                    assert!(!navigation.collapsed.contains(&key));
                    parent = source
                        .model_tree
                        .rows
                        .iter()
                        .find(|r| r.id == key)
                        .expect("parent")
                        .parent;
                }
                if matches!(selection, Selection::Face(_)) {
                    face_rows.push(row);
                }
            }
        }
        assert!(picked_face, "every Body has a picked face");
    }
    assert_eq!(face_rows.len(), 3);
    assert!(
        picked_fillet,
        "an edge created by Fillet selects that Fillet"
    );
    assert!(picked_vertex, "a picked vertex selects its producer");
    assert_eq!(
        picked_row(&source, &Selection::Nothing, &loaded.catalogue, &snapshot),
        Ok(None)
    );
    let definition = Selection::Definition(snapshot.pick_of(0).expect("definition"));
    assert_eq!(
        picked_row(&source, &definition, &[], &snapshot),
        Err("The selection belongs to an older picture.".into())
    );
    assert_eq!(
        picked_row(&source, &definition, &loaded.catalogue, &snapshot),
        Err("The selected entity has no stored meaning; the tree selection is unchanged.".into())
    );
    drop(tab);
    Ok(())
}

#[test]
fn native_gpu_imported_entity_selects_the_stored_import_object() -> Result<()> {
    let Some(mut renderer) = renderer()? else {
        return Ok(());
    };
    let root = tempfile::tempdir()?;
    let path = root.path().join("import.fcad");
    let step = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step/canonical/01-single-part.step");
    let outcome = ferritecad_jobs::import_step_document(
        &ferritecad_jobs::ImportStepRequest {
            source: &step,
            destination: &path,
            name: Some("Imported part"),
            existing: ferritecad_jobs::Existing::Keep {
                advice: "choose another output",
            },
        },
        ferritecad_occt::OcctKernel::new,
        ferritecad_occt::OcctKernel::import_step,
        &OperationContext::default(),
    )?;
    let ferritecad_jobs::StepImportOutcome::Published(import) = outcome else {
        panic!("supported import");
    };
    let (loaded, tab) =
        crate::sessions::open_for_view(root.path(), &path, &OperationContext::default())?;
    let mut source: ExtrudeEditSource = loaded.edit_source.expect("facts").into();
    let snapshot = Arc::new(loaded.snapshot);
    let prepared = renderer.prepare(Arc::clone(&snapshot))?;
    let mut camera = Camera::new();
    camera.resize(240, 240);
    camera.frame(snapshot.bounds().expect("import bounds"))?;
    let frame = renderer.render(
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Nothing,
        &Visibility::new(&snapshot),
    )?;
    let hit = (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .map(|(x, y)| frame.hit_at(x, y))
        .find(|h| h.face() != FacePickId::NOTHING)
        .expect("imported face visible");
    let selection = crate::selection_at(
        hit,
        &snapshot,
        &loaded.faces,
        &loaded.edges,
        &loaded.vertices,
    );
    assert_eq!(
        picked_row(&source, &selection, &loaded.catalogue, &snapshot).expect("import object"),
        Some((import.object_id, None, None))
    );
    let mut record = source
        .objects
        .iter()
        .find(|o| o.id == import.object_id)
        .expect("stored import")
        .clone();
    record.id = ObjectId::new();
    source.model_tree.rows.clear();
    assert_eq!(
        picked_row(&source, &selection, &loaded.catalogue, &snapshot),
        Err("The producer is not in the displayed model tree.".into())
    );
    source.objects.push(record);
    assert_eq!(
        picked_row(&source, &selection, &loaded.catalogue, &snapshot),
        Err("Several import objects own this source; the tree selection is unchanged.".into())
    );
    source.objects.retain(|o| {
        !matches!(
            o.payload,
            ferritecad_document::ObjectPayload::ImportedStep(_)
        )
    });
    assert_eq!(
        picked_row(&source, &selection, &loaded.catalogue, &snapshot),
        Err("The selected import is no longer in the document.".into())
    );
    drop(tab);
    Ok(())
}
