// SPDX-License-Identifier: MIT
//
// Temporary: isolates which part of the face target a backend refuses.

#![allow(clippy::panic)]

/// What the smoke suite actually does: a device per test, in parallel, each
/// drawing and reading back a frame.
#[test]
fn many_renderers_each_drawing() {
    use std::sync::{Arc, Mutex};

    let report = Arc::new(Mutex::new(Vec::new()));
    let mut threads = Vec::new();
    for worker in 0..8 {
        let report = Arc::clone(&report);
        threads.push(std::thread::spawn(move || {
            for round in 0..4 {
                let label = format!("worker {worker} round {round}");
                let renderer = match ferritecad_viewport_gpu::Renderer::new() {
                    Ok(renderer) => renderer,
                    Err(error) => {
                        report
                            .lock()
                            .expect("no panic held the lock")
                            .push(format!("{label}: new refused: {:?} {error}", error.kind()));
                        return;
                    }
                };
                let mut renderer = renderer;
                let mut builder = ferritecad_viewport::SnapshotBuilder::new();
                let mesh = ferritecad_kernel::Mesh {
                    positions: vec![-5.0, 0.0, -5.0, 5.0, 0.0, -5.0, 5.0, 0.0, 5.0],
                    normals: vec![0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0],
                    indices: vec![0, 1, 2],
                    faces: vec![ferritecad_kernel::MeshFaceRange {
                        face: ferritecad_kernel::SubShapeHandle::new(
                            ferritecad_kernel::ShapeHandle::new(
                                ferritecad_kernel::SessionId::new(),
                                1,
                            ),
                            ferritecad_kernel::SubShapeKind::Face,
                            0,
                        ),
                        first_index: 0,
                        index_count: 3,
                    }],
                };
                let definition = builder.add_mesh(&mesh).expect("packs");
                builder
                    .place(
                        definition,
                        None,
                        &ferritecad_types::Transform::IDENTITY,
                        [0.0, 1.0, 0.0],
                    )
                    .expect("places");
                let snapshot = std::sync::Arc::new(builder.build());
                let mut camera = ferritecad_viewport::Camera::new();
                camera.resize(96, 96);
                camera
                    .frame(snapshot.bounds().expect("geometry"))
                    .expect("frames");
                let prepared = match renderer.prepare(snapshot) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        report
                            .lock()
                            .expect("no panic held the lock")
                            .push(format!("{label}: prepare refused: {error}"));
                        return;
                    }
                };
                if let Err(error) = renderer.render(
                    &prepared,
                    &camera,
                    ferritecad_viewport::PickId::NOTHING,
                    ferritecad_viewport::Hovered::Nothing,
                ) {
                    report
                        .lock()
                        .expect("no panic held the lock")
                        .push(format!("{label}: render refused: {error}"));
                    return;
                }
            }
        }));
    }
    for thread in threads {
        thread.join().expect("a worker panicked");
    }

    let report = report.lock().expect("no panic held the lock").join("\n");
    panic!(
        "PARALLEL:\n{}",
        if report.is_empty() {
            "all 32 drew".to_string()
        } else {
            report
        }
    );
}
