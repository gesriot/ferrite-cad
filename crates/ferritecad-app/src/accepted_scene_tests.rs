// SPDX-License-Identifier: MIT
//! §31K: the one statement in which the edit catalogue and the drawing of the accepted scene
//! become current together. The scenes here are real rebuilds (`snapshot_of`, the mock kernel
//! and an unconstrained plate, so the solved drawing equals the stored one): they show the
//! pairing, not the difference between stored and solved, which the native gates measure.
#![allow(clippy::panic)]
use super::*;
use crate::sketch::accepted::Accepted;
use crate::sketch_facts::Source as ExtrudeEditSource;
use crate::sketch_mode::tests::curve_edits::{curves, sketch_id};
use ferritecad_document::{Document, ObjectPayload};
use ferritecad_kernel::mock::MockKernel;

/// A sample plate on disk, its facts and the real scene a rebuild makes of it.
fn plate(name: &str) -> (tempfile::TempDir, PathBuf, ExtrudeEditSource, LoadedScene) {
    let root = tempfile::tempdir().expect("dir");
    let path = root.path().join(name);
    ferritecad_jobs::create_document(
        ferritecad_jobs::CreateDocumentRequest::new(
            &path,
            NewDocument::SamplePlate(Default::default()),
            "test",
        ),
        &OperationContext::default(),
    )
    .expect("a plate");
    let loaded = load(&path);
    let source = loaded
        .edit_source
        .clone()
        .expect("the scene carries its facts");
    (root, path, source.into(), loaded)
}

fn load(path: &Path) -> LoadedScene {
    ferritecad_scene::snapshot_of(
        path,
        &mut MockKernel::new(),
        |_: &mut MockKernel, _: &[u8]| Err(CadError::unsupported("no imports")),
        &ferritecad_kernel::TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads")
}

fn empty() -> (LiveScene<()>, ViewportInput) {
    let mut camera = ViewportInput::new();
    camera.resize(800, 600);
    let scene = LiveScene::new(
        None,
        (),
        Vec::new(),
        FaceNames::default(),
        EdgeNames::default(),
        VertexNames::default(),
        Visibility::default(),
        Vec::new(),
    );
    (scene, camera)
}

fn arrive(scene: &mut LiveScene<()>, camera: &mut ViewportInput, path: &Path, loaded: LoadedScene) {
    let prepared = prepare_load(camera, path, Ok(loaded), |_, _| Ok(()));
    commit_scene(scene, camera, prepared).expect("the scene commits");
}

/// The same document with one object renamed: another version, the same document.
fn renamed(path: &Path) -> PathBuf {
    let next = path.with_file_name("renamed.fcad");
    std::fs::copy(path, &next).expect("copy");
    let mut document = Document::open(&next).expect("document");
    let object = document
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
        .expect("an Extrude");
    document
        .write(|w| {
            w.put_object(
                object.id,
                object.parent,
                object.ordinal,
                Some("Renamed"),
                &object.payload,
            )
        })
        .expect("rename");
    document.close().expect("close");
    next
}

#[test]
fn the_accepted_scene_hands_a_form_the_real_presentation_of_its_own_version() {
    let (_root, path, source, loaded) = plate("plate.fcad");
    let sketch = sketch_id(&source);
    let presentation = loaded
        .sketch_presentations
        .iter()
        .find(|p| p.sketch() == sketch)
        .expect("rebuilt sketch")
        .clone();
    let (mut scene, mut camera) = empty();
    assert!(scene.sketches.presentation(&source, sketch).is_none());
    let mut form = crate::sketch::Editor::default();
    assert!(
        !form
            .viewport
            .begin_with(&path, &source, &scene.sketches, sketch)
    );
    arrive(&mut scene, &mut camera, &path, loaded);
    let facts = scene.edit_source.as_ref().expect("facts");
    assert_eq!(
        scene.sketches.presentation(facts, sketch),
        Some(&presentation)
    );
    assert!(
        form.viewport
            .begin_with(&path, facts, &scene.sketches, sketch)
    );
    let shown = curves(&form);
    assert_eq!(shown.len(), presentation.curves().len());
    for (curve, built) in shown.iter().zip(presentation.curves()) {
        assert_eq!(curve.id, built.id());
        assert_eq!(&curve.geometry, built.geometry());
        assert_eq!(curve.construction, built.is_construction());
    }
}

#[test]
fn a_form_is_never_handed_the_drawing_of_another_version_or_document() {
    let (_a, path_a, source_a, loaded_a) = plate("a.fcad");
    let (_b, path_b, source_b, loaded_b) = plate("b.fcad");
    let a = sketch_id(&source_a);
    let b = sketch_id(&source_b);
    let (mut scene, mut camera) = empty();
    arrive(&mut scene, &mut camera, &path_a, loaded_a);
    assert!(scene.sketches.presentation(&source_a, a).is_some());
    assert!(scene.sketches.presentation(&source_a, b).is_none());
    assert!(scene.sketches.presentation(&source_b, b).is_none());
    let next = renamed(&path_a);
    let newer = crate::sketch_facts::read(&next).expect("facts");
    assert_eq!(newer.version.document_id, source_a.version.document_id);
    assert_ne!(newer.version.content, source_a.version.content);
    assert!(scene.sketches.presentation(&newer, a).is_none());
    arrive(&mut scene, &mut camera, &path_b, loaded_b);
    assert!(scene.sketches.presentation(&source_a, a).is_none());
    assert!(scene.sketches.presentation(&source_b, b).is_some());
    assert!(Accepted::default().presentation(&source_b, b).is_none());
}

#[test]
fn a_kept_picture_follows_its_version_and_a_failed_arrival_keeps_the_drawing_and_the_facts() {
    let (_root, path, source, loaded) = plate("plate.fcad");
    let (mut scene, mut camera) = empty();
    arrive(&mut scene, &mut camera, &path, loaded);
    let sketch = sketch_id(&source);

    // §30N: a version that draws the same model re-points the picture. The drawing is the
    // picture's and follows the facts' version in the same statement.
    let next = renamed(&path);
    let newer =
        crate::sketch_facts::Source::read(&Document::open_read_only(&next).expect("document"))
            .expect("facts");
    assert!(scene.sketches.presentation(&newer, sketch).is_none());
    retarget_scene(&mut scene, next.clone(), newer.clone());
    assert!(scene.sketches.presentation(&newer, sketch).is_some());
    assert!(scene.sketches.presentation(&source, sketch).is_none());
    assert_eq!(scene.document.as_deref(), Some(next.as_path()));

    // A scene that has no accepted drawing stays without one when it is retargeted.
    let (mut bare, _) = empty();
    retarget_scene(&mut bare, next.clone(), newer.clone());
    assert!(bare.sketches.presentation(&newer, sketch).is_none());

    // An arrival that fails, before or after its picture is prepared, changes nothing.
    let before = scene
        .sketches
        .presentation(&newer, sketch)
        .expect("kept drawing")
        .clone();
    let facts_before = scene.edit_source.clone();
    let other = plate("c.fcad");
    let failed = prepare_load(
        &camera,
        &other.1,
        Err(CadError::input("the file is gone")),
        |_, _| Ok(()),
    );
    assert!(commit_scene(&mut scene, &mut camera, failed).is_err());
    let refused = prepare_load(&camera, &other.1, Ok(other.3.clone()), |_, _| {
        Err::<(), _>(CadError::input("the device refused the picture"))
    });
    assert!(commit_scene(&mut scene, &mut camera, refused).is_err());
    assert_eq!(scene.sketches.presentation(&newer, sketch), Some(&before));
    assert_eq!(scene.edit_source, facts_before);
}
