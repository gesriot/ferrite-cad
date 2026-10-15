// SPDX-License-Identifier: MIT
//! §31K: the one statement in which the edit catalogue and the drawing of the accepted scene
//! become current together. The scenes here are real rebuilds (`snapshot_of`, the mock kernel
//! and an unconstrained plate, so the solved drawing equals the stored one): they show the
//! pairing, not the difference between stored and solved, which the native gates measure.
#![allow(clippy::panic)]
use super::*;
use crate::sketch::accepted::{Accepted, Drawing, Shape, Unavailable};
use ferritecad_document::{Document, ExtrudeEditSource, ObjectPayload};
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
    (root, path, source, loaded)
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
    let sketch = source.constraint_sketches[0].sketch;
    let presentation = loaded
        .sketch_presentations
        .iter()
        .find(|p| p.sketch() == sketch)
        .expect("the rebuild drew the Sketch")
        .clone();
    let (mut scene, mut camera) = empty();
    assert_eq!(
        scene
            .sketches
            .drawing(&source, &source.constraint_sketches[0]),
        Drawing::Unavailable(Unavailable::NotHanded),
        "before any scene is accepted there is no drawing"
    );
    arrive(&mut scene, &mut camera, &path, loaded);

    // The scene hands the form the rebuild's own curves, in its order, under its UUIDs.
    let facts = scene.edit_source.clone().expect("facts");
    let choice = &facts.constraint_sketches[0];
    let Drawing::Solved(curves) = scene.sketches.drawing(&facts, choice) else {
        panic!("the accepted scene has the drawing of its own Sketch")
    };
    assert_eq!(curves.len(), presentation.curves().len());
    for (shown, drawn) in curves.iter().zip(presentation.curves()) {
        assert_eq!(shown.id, drawn.id());
        let ferritecad_document::SketchGeometry::Line { start, end } = drawn.geometry() else {
            panic!("a plate is Lines")
        };
        assert_eq!(
            shown.shape,
            Shape::Line {
                start: [start.x, start.y],
                end: [end.x, end.y]
            }
        );
    }
    // The form opened through the window's route (`begin_with`) carries it; opened with no
    // scene (`begin`) it says nobody handed it one.
    let mut form = constraints::Editor::default();
    assert!(form.begin_with(&path, &facts, &scene.sketches, choice.sketch));
    assert!(matches!(
        form.draft_canvas().expect("draft").0,
        Drawing::Solved(_)
    ));
    let mut bare = constraints::Editor::default();
    assert!(bare.begin(&path, &facts, choice.sketch));
    assert_eq!(
        bare.draft_canvas().expect("draft").0,
        &Drawing::Unavailable(Unavailable::NotHanded)
    );
    println!("\nFCAD_31K_SCENE_HANDOFF_EXECUTED");
}

#[test]
fn a_form_is_never_handed_the_drawing_of_another_version_or_document() {
    let (_a, path_a, source_a, loaded_a) = plate("a.fcad");
    let (_b, path_b, source_b, loaded_b) = plate("b.fcad");
    let choice_a = &source_a.constraint_sketches[0];
    let (mut scene, mut camera) = empty();
    arrive(&mut scene, &mut camera, &path_a, loaded_a);
    assert!(matches!(
        scene.sketches.drawing(&source_a, choice_a),
        Drawing::Solved(_)
    ));
    // Another document's facts: not this scene's drawing, whatever the Sketch is called.
    let other = Unavailable::OtherVersion;
    assert_eq!(
        scene
            .sketches
            .drawing(&source_b, &source_b.constraint_sketches[0]),
        Drawing::Unavailable(other)
    );
    // The same document at another version: not this scene's drawing either.
    let next = renamed(&path_a);
    let newer = ExtrudeEditSource::read(&Document::open_read_only(&next).expect("document"))
        .expect("facts");
    assert_eq!(newer.version.document_id, source_a.version.document_id);
    assert_ne!(newer.version.content, source_a.version.content);
    assert_eq!(
        scene
            .sketches
            .drawing(&newer, &newer.constraint_sketches[0]),
        Drawing::Unavailable(other),
        "the drawing of version 1 beside the UUIDs of version 2"
    );
    // The scene of B replaces A's whole: A's facts are not handed B's drawing, and B's are.
    arrive(&mut scene, &mut camera, &path_b, loaded_b);
    assert_eq!(
        scene.sketches.drawing(&source_a, choice_a),
        Drawing::Unavailable(other)
    );
    assert!(matches!(
        scene
            .sketches
            .drawing(&source_b, &source_b.constraint_sketches[0]),
        Drawing::Solved(_)
    ));
    // A scene without facts hands out nothing.
    assert_eq!(
        Accepted::default().drawing(&source_b, &source_b.constraint_sketches[0]),
        Drawing::Unavailable(Unavailable::NotHanded)
    );
    println!("\nFCAD_31K_SCENE_FOREIGN_EXECUTED");
}

#[test]
fn a_kept_picture_follows_its_version_and_a_failed_arrival_keeps_the_drawing_and_the_facts() {
    let (_root, path, source, loaded) = plate("plate.fcad");
    let (mut scene, mut camera) = empty();
    arrive(&mut scene, &mut camera, &path, loaded);
    let choice = &source.constraint_sketches[0];

    // §30N: a version that draws the same model re-points the picture. The drawing is the
    // picture's and follows the facts' version in the same statement.
    let next = renamed(&path);
    let newer = ExtrudeEditSource::read(&Document::open_read_only(&next).expect("document"))
        .expect("facts");
    assert_eq!(
        scene
            .sketches
            .drawing(&newer, &newer.constraint_sketches[0]),
        Drawing::Unavailable(Unavailable::OtherVersion)
    );
    retarget_scene(&mut scene, next.clone(), newer.clone());
    assert!(matches!(
        scene
            .sketches
            .drawing(&newer, &newer.constraint_sketches[0]),
        Drawing::Solved(_)
    ));
    assert_eq!(
        scene.sketches.drawing(&source, choice),
        Drawing::Unavailable(Unavailable::OtherVersion),
        "the version that was replaced is not handed the drawing any more"
    );
    assert_eq!(scene.document.as_deref(), Some(next.as_path()));

    // A scene that has no accepted drawing stays without one when it is retargeted.
    let (mut bare, _) = empty();
    retarget_scene(&mut bare, next.clone(), newer.clone());
    assert_eq!(
        bare.sketches.drawing(&newer, &newer.constraint_sketches[0]),
        Drawing::Unavailable(Unavailable::NotHanded)
    );

    // An arrival that fails, before or after its picture is prepared, changes nothing.
    let before = scene
        .sketches
        .drawing(&newer, &newer.constraint_sketches[0]);
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
    assert_eq!(
        scene
            .sketches
            .drawing(&newer, &newer.constraint_sketches[0]),
        before
    );
    assert_eq!(scene.edit_source, facts_before);
    println!("\nFCAD_31K_SCENE_KEPT_EXECUTED");
}
