// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{
    DatumPlane, EditChange, EndCondition, Expression, FeatureAddition, SolidOperation,
};
use ferritecad_fixtures::datum_planes::{boss, circle};

#[test]
fn native_face_and_offset_sketches_follow_their_bodies_in_the_window_scene() {
    if !native() {
        return;
    }
    let f = fixture();
    let mut document = Document::open(&f.file).expect("document");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let supported = boss(&document, &mut kernel).expect("support fixture");
    document.write_changes(&supported.changes).expect("boss");
    let (plane, sketch) = (ObjectId::new(), ObjectId::new());
    document
        .write_changes(&[
            EditChange::AddDatumPlane {
                object: plane,
                name: None,
                plane: DatumPlane::Offset {
                    from: supported.plane,
                    distance_mm: Expression::constant(10.).expect("offset"),
                },
            },
            EditChange::AddSketch {
                object: sketch,
                name: None,
                sketch: circle(plane, [45., 20.], 1.).expect("circle"),
            },
            EditChange::AddBody {
                object: ObjectId::new(),
                name: None,
                feature_object: ObjectId::new(),
                feature_name: None,
                feature: FeatureAddition::Extrude {
                    profile: sketch,
                    regions: vec![],
                    operation: SolidOperation::NewBody,
                    reversed: false,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(2.).expect("height"),
                    },
                },
            },
        ])
        .expect("offset body");
    document.close().expect("close");
    let mut s = open(&f);
    let mut renderer = match ferritecad_viewport_gpu::Renderer::new() {
        Ok(renderer) => renderer,
        Err(e) if e.kind() == ferritecad_types::ErrorKind::Unsupported => {
            ferritecad_fixtures::native_dependencies(&[(false, "GPU")]);
            return;
        }
        Err(e) => panic!("renderer failed: {e}"),
    };
    for height in [12., 20.] {
        if height == 20. {
            apply_native(&mut s, f.feature, height);
        }
        let path = s
            .session
            .as_ref()
            .expect("session")
            .current()
            .path()
            .to_owned();
        let (tx, rx) = mpsc::channel();
        let worker = spawn_scene(path, CancelToken::new(), move |scene| {
            tx.send(scene).expect("scene");
        });
        let scene = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("worker")
            .expect("built scene");
        worker.join().expect("joined");
        assert!(scene.rebuild.all_built());
        for (id, z) in [(supported.sketch, height), (sketch, height + 10.)] {
            let presentation = scene
                .sketch_presentations
                .iter()
                .find(|p| p.sketch() == id)
                .expect("sketch presentation");
            let drawing = ferritecad_scene::sketch_drawing(presentation).expect("drawing");
            assert!(
                drawing
                    .strokes()
                    .iter()
                    .flat_map(|s| s.points())
                    .all(|p| (f64::from(p[2]) - z).abs() < 1e-5)
            );
        }
        let snapshot = Arc::new(scene.snapshot);
        let bounds = snapshot.bounds().expect("bounds");
        assert!((f64::from(bounds.1[2]) - (height + 12.)).abs() < 1e-5);
        let drawings = ferritecad_scene::sketch_drawings(&scene.sketch_presentations)
            .expect("window drawings");
        let plain = renderer
            .prepare(Arc::clone(&snapshot))
            .expect("upload bodies");
        let prepared = renderer
            .prepare(Arc::clone(&snapshot))
            .expect("upload bodies");
        let prepared = renderer
            .prepare_sketches(prepared, &drawings)
            .expect("upload sketches");
        let mut view = crate::ViewportInput::new();
        view.resize(320, 320);
        view.frame(bounds).expect("frame");
        let visibility = crate::Visibility::new(&snapshot);
        let plain_frame = renderer
            .render(
                &plain,
                view.camera(),
                crate::Marked::Nothing,
                crate::Hovered::Nothing,
                &visibility,
            )
            .expect("bodies");
        let sketch_frame = renderer
            .render(
                &prepared,
                view.camera(),
                crate::Marked::Nothing,
                crate::Hovered::Nothing,
                &visibility,
            )
            .expect("sketches and bodies");
        assert_ne!(
            plain_frame.colour(),
            sketch_frame.colour(),
            "sketches change visible pixels"
        );
    }
}
