// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::EditChange;

#[test]
fn native_referenced_end_draws_the_boss_height_and_cut_form_explains_editing() {
    if !native() {
        return;
    }
    let f = fixture();
    let mut document = Document::open(&f.file).expect("document");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let ceiling = ferritecad_fixtures::up_to_end::ceiling(&mut document, &mut kernel)
        .expect("ceiling fixture");
    let body = ferritecad_fixtures::feature_changes::body(&document).expect("body");
    document.close().expect("close");
    let mut renderer = match ferritecad_viewport_gpu::Renderer::new() {
        Ok(renderer) => renderer,
        Err(error) if error.kind() == ferritecad_types::ErrorKind::Unsupported => {
            ferritecad_fixtures::native_dependencies(&[(false, "GPU")]);
            return;
        }
        Err(error) => panic!("renderer: {error}"),
    };
    let mut pixels = Vec::new();
    for offset in [0., 2.] {
        let mut document = Document::open(&f.file).expect("document");
        document
            .write_changes(&[EditChange::SetEndCondition {
                object: ceiling.boss.feature,
                end_condition: ceiling.face_end(offset).expect("end"),
            }])
            .expect("end");
        document.close().expect("close");
        let (tx, rx) = mpsc::channel();
        let worker = spawn_scene(f.file.clone(), CancelToken::new(), move |scene| {
            tx.send(scene).expect("scene");
        });
        let scene = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("worker")
            .expect("scene");
        worker.join().expect("joined");
        assert!(scene.rebuild.all_built());
        let index = scene
            .catalogue
            .iter()
            .position(|e| e.item == ferritecad_scene::SceneItem::Body(body))
            .expect("boss body");
        let snapshot = Arc::new(scene.snapshot);
        let pick = snapshot.pick_of(index).expect("pick");
        let bounds = snapshot.bounds_of(pick).expect("body bounds");
        assert!((f64::from(bounds.1[2]) - (32. - offset)).abs() < 1e-5);
        let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("upload");
        let mut view = crate::ViewportInput::new();
        view.resize(320, 320);
        view.frame(snapshot.bounds().expect("whole picture"))
            .expect("camera");
        let mut visibility = crate::Visibility::new(&snapshot);
        assert!(visibility.isolate(crate::Marked::Definition(pick), &snapshot));
        let frame = renderer
            .render(
                &prepared,
                view.camera(),
                crate::Marked::Nothing,
                crate::Hovered::Nothing,
                &visibility,
            )
            .expect("render boss");
        pixels.push(frame.colour().to_vec());
    }
    assert_ne!(
        pixels[0], pixels[1],
        "the signed end offset changes visible pixels"
    );

    let root = tempfile::tempdir().expect("pockets directory");
    let mut document = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let pockets =
        ferritecad_fixtures::up_to_end::pockets(&mut document, &mut kernel).expect("pockets");
    let path = document.path().to_owned();
    document.close().expect("close");
    let source = crate::sketch_facts::read(&path).expect("form facts");
    let choice = source
        .cut_features
        .iter()
        .find(|c| c.feature == pockets.second)
        .expect("referenced end remains visible");
    let refusal = "this feature's end condition is edited through `edit`";
    assert_eq!(choice.refusal.as_deref(), Some(refusal));
    assert!(choice.saved.is_none());
    let mut editor = crate::cuts::Editor::default();
    assert!(!editor.begin_edit(&path, &source, pockets.second));
    let context = egui::Context::default();
    let mut visible = false;
    for _ in 0..3 {
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                ..Default::default()
            },
            |ui| editor.choices(ui, true, Some(&path), Some(&source)),
        );
        visible |= output.shapes.iter().any(|s| {
            matches!(&s.shape,
            egui::Shape::Text(t) if t.galley.text() == refusal)
        });
        output.textures_delta.clear();
    }
    assert!(visible, "the real Cut widgets show the refusal");
}
