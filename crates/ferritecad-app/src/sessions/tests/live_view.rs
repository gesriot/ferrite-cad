// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{EditChange, ExpressionSlot};
use ferritecad_occt::OcctKernel;
use ferritecad_viewport::{Camera, FacePickId, Hovered, Marked, Visibility};
use ferritecad_viewport_gpu::{PreparedSnapshot, Renderer};

fn upload(
    scene: LoadedScene,
    path: &Path,
    renderer: &mut Renderer,
    changed: u64,
) -> Result<PreparedSnapshot> {
    let cold = ferritecad_scene::snapshot_of(
        path,
        &mut OcctKernel::new()?,
        OcctKernel::import_step,
        &TessellationParams::default(),
        &OperationContext::default(),
    )?;
    assert_eq!(scene, cold, "scene of {}", path.display());
    assert_eq!(scene.catalogue.len(), 3);
    let before = renderer.geometry_uploads();
    let snapshot = Arc::new(scene.snapshot);
    let prepared = renderer.prepare_keyed(Arc::clone(&snapshot), &scene.mesh_keys)?;
    assert_eq!(renderer.geometry_uploads() - before, changed);
    let mut camera = Camera::new();
    camera.resize(192, 192);
    camera.frame(snapshot.bounds().expect("three bodies"))?;
    for index in 0..3 {
        let pick = snapshot.pick_of(index).expect("packed body");
        let mut visibility = Visibility::new(&snapshot);
        assert!(visibility.isolate(Marked::Definition(pick), &snapshot));
        let frame = renderer.render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &visibility,
        )?;
        let hit = (0..frame.height())
            .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
            .map(|(x, y)| frame.hit_at(x, y))
            .find(|hit| {
                hit.face() != FacePickId::NOTHING
                    && !scene.faces.of(hit.face(), &snapshot).is_empty()
            })
            .expect("named visible face");
        assert_eq!(snapshot.definition_of_face(hit.face()), Some(index));
        assert_eq!(
            scene.faces.of(hit.face(), &snapshot),
            cold.faces.of(hit.face(), &cold.snapshot)
        );
    }
    Ok(prepared)
}

#[test]
fn native_window_reuses_other_bodies_on_apply_undo_redo() -> Result<()> {
    if !native() {
        return Ok(());
    }
    let mut renderer = match Renderer::new() {
        Ok(renderer) => renderer,
        Err(error) if error.kind() == ErrorKind::Unsupported => {
            ferritecad_fixtures::native_dependencies(&[(false, "GPU")]);
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let root = tempfile::tempdir()?;
    let path = root.path().join("three-bodies.fcad");
    let model = ferritecad_fixtures::live_model::write(&path)?;
    let context = OperationContext::default();
    let (scene, tab) = open_for_view(root.path(), &path, &context)?;
    let mut displayed = upload(scene, tab.current().path(), &mut renderer, 3)?;
    let mut sessions = Sessions::default();
    sessions.adopt(tab);
    for action in 0..4 {
        let old = sessions.export_source().expect("open");
        let address = if action < 2 {
            let change = if action == 0 {
                EditChange::SetParameter {
                    object: model.late,
                    expression: "4 mm".into(),
                }
            } else {
                EditChange::constant(model.b, ExpressionSlot::ExtrudeDistance, 4.)?
            };
            let (send, receive) = mpsc::channel();
            let address = sessions
                .apply(old.version(), vec![change], move |a, step| {
                    send.send((a, step)).expect("window");
                })
                .expect("idle tab");
            let (reply, result) = receive.recv().expect("evaluation");
            assert_eq!(reply, address);
            assert!(matches!(
                sessions.finish_apply(reply, result),
                Edited::Show(_)
            ));
            address
        } else {
            sessions.begin_move(action == 2).expect("history").0
        };
        let (send, receive) = mpsc::channel();
        sessions.draw_staged(address, move |result| {
            send.send(result).expect("window");
        });
        let candidate = receive.recv().expect("picture")?;
        let next = upload(
            candidate,
            &sessions.staged_path(address).expect("candidate"),
            &mut renderer,
            1,
        )?;
        assert_eq!(
            sessions.export_source().expect("still open").version(),
            old.version()
        );
        assert_eq!(displayed.snapshot().meshes().len(), 3);
        sessions.commit_staged()?;
        displayed = next;
        assert!(sessions.finish_scene(address, Ok(())));
    }
    sessions.stop_all();
    Ok(())
}

#[test]
fn stub_open_refuses_before_publishing_a_tab() -> Result<()> {
    if ferritecad_occt::is_available() {
        return Ok(());
    }
    let source = tempfile::tempdir()?;
    let private = tempfile::tempdir()?;
    let path = source.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path)?;
    let before = std::fs::read(&path)?;
    let failure = open_for_view(private.path(), &path, &OperationContext::default())
        .expect_err("no native evaluator can be created");
    assert_eq!(failure.kind(), ErrorKind::Unsupported);
    assert!(failure.to_string().contains("no Open CASCADE"));
    assert_eq!(std::fs::read(&path)?, before);
    assert_eq!(std::fs::read_dir(private.path())?.count(), 0);
    Ok(())
}
