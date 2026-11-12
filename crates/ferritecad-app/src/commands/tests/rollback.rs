// SPDX-License-Identifier: MIT
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window};
use ferritecad_document::{EndCondition, Expression, ObjectPayload};
use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool, edge_addition};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_scene::LoadedScene;
use std::{
    path::Path,
    sync::{Arc, mpsc},
    time::Duration,
};

struct Fixture {
    _root: tempfile::TempDir,
    path: std::path::PathBuf,
    body: ObjectId,
    base: ObjectId,
    middle: ObjectId,
    tip: ObjectId,
}

fn fixture() -> Result<Fixture> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("history.fcad");
    ferritecad_fixtures::write_plate(&path)?;
    let mut document = Document::open(&path)?;
    let body = body(&document)?;
    let (base, _, _) = base_profile(&document, body)?;
    let cut = circular_tool(
        &document,
        body,
        [20., 20.],
        2.,
        EndCondition::Blind {
            distance: Expression::constant(3.)?,
        },
    )?;
    document.write_changes(&cut.changes)?;
    let chamfer = edge_addition(&document, body, 2, 1., true)?;
    let tip = chamfer.object();
    document.write_changes(&[chamfer])?;
    document.close()?;
    Ok(Fixture {
        _root: root,
        path,
        body,
        base,
        middle: cut.feature,
        tip,
    })
}

fn cold(path: &Path) -> LoadedScene {
    ferritecad_scene::snapshot_of(
        path,
        &mut OcctKernel::new().expect("OCCT"),
        OcctKernel::import_step,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("cold scene")
}

fn accept(
    window: &mut Window,
    address: crate::sessions::Address,
    path: std::path::PathBuf,
) -> LoadedScene {
    let (tx, rx) = mpsc::channel();
    window
        .sessions
        .draw_staged(address, move |scene| tx.send(scene).expect("scene"));
    let scene = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("picture")
        .expect("built");
    assert_eq!(scene, cold(&path), "compare before publication");
    let outcome = window.present(
        &path,
        Ok(scene.clone()),
        crate::sessions::Bind::Staged,
        Ok(()),
    );
    assert!(outcome.is_ok(), "{outcome:?}");
    assert!(window.sessions.finish_scene(address, outcome));
    scene
}

fn apply(window: &mut Window, changes: Vec<EditChange>) -> LoadedScene {
    let version = window.sessions.export_source().expect("current").version();
    let (tx, rx) = mpsc::channel();
    let address = window
        .sessions
        .apply(version, changes, move |_, reply| {
            tx.send(reply).expect("reply");
        })
        .expect("apply");
    let reply = rx.recv_timeout(Duration::from_secs(60)).expect("reply");
    let crate::sessions::Edited::Show(path) = window.sessions.finish_apply(address, reply) else {
        panic!("refused: {}", window.sessions.status)
    };
    accept(window, address, path)
}

fn press(window: &mut Window, object: ObjectId, command: Command, surface: u8) -> Action {
    let source = window.scene.edit_source.as_ref().expect("source");
    let row = source
        .model_tree
        .rows
        .iter()
        .find(|r| r.id.0 == object)
        .expect("row")
        .id;
    let tab = window.sessions.tab();
    let navigation = window.tabs.model_navigation(tab);
    select_row(source, navigation, row);
    let ctx = egui::Context::default();
    let mut frame = |events| {
        let mut action = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                if surface == 2 {
                    action = Menu {
                        tab: tab.key(),
                        epoch: navigation.epoch,
                        row,
                        position: egui::pos2(20., 20.),
                    }
                    .draw(ui.ctx(), source, Ok(Access::Idle))
                    .0;
                } else {
                    action = ferritecad_ui::model_panel(
                        ui,
                        tab.key(),
                        &[ferritecad_ui::ModelRow {
                            key: row,
                            parent: None,
                            depth: 0,
                            name: "Chosen feature",
                            kind: "Feature",
                            note: "built",
                            solid_count: Some(1),
                        }],
                        navigation,
                        |key| offers(source, key, Ok(Access::Idle)),
                    );
                }
            },
        );
        output.textures_delta.clear();
        (output, action)
    };
    let mut output = frame(vec![]).0;
    for _ in 0..3 {
        output = frame(vec![]).0;
    }
    if surface == 1 {
        let label = if row.1.is_some() {
            "Ref: Chosen feature · Feature"
        } else {
            "Chosen feature · Feature"
        };
        let at = super::widgets::rect(&output, label, false).center();
        frame(super::widgets::clicks(at, egui::PointerButton::Secondary));
        output = frame(vec![]).0;
    }
    let at = super::widgets::rect(&output, command.label(), surface == 1).center();
    frame(super::widgets::clicks(at, egui::PointerButton::Primary))
        .1
        .expect("command widget")
}

fn command(window: &mut Window, object: ObjectId, command: Command, surface: u8) -> LoadedScene {
    let asked = press(window, object, command, surface);
    let source = window.scene.edit_source.as_ref().expect("source");
    let Launch::Apply(version, changes) = launch(
        asked,
        source,
        &window.scene.sketches,
        &mut window.creates,
        &mut window.edits,
        &window.sessions,
        &window.tabs,
        &Loads::default(),
        &Exports::default(),
        &window.input,
        false,
    )
    .expect("launch") else {
        panic!("direct command")
    };
    assert_eq!(version, source.version);
    assert_eq!(
        changes,
        vec![EditChange::SetRollback {
            object: asked.row.1.unwrap_or(asked.row.0),
            feature: (command == Command::Rollback).then_some(object),
        }]
    );
    apply(window, changes)
}

#[test]
fn rollback_addition_facts_use_shown_and_later_existing_edit_routes_remain_available() -> Result<()>
{
    let f = fixture()?;
    let mut document = Document::open(&f.path)?;
    document.write_changes(&[EditChange::SetRollback {
        object: f.body,
        feature: Some(f.base),
    }])?;
    let source = crate::sketch_facts::Source::read(&document)?;
    assert_eq!(
        source.cut_bodies[0]
            .target
            .as_ref()
            .expect("Cut")
            .shown_feature,
        f.base
    );
    assert_eq!(
        source.fillet_bodies[0]
            .target
            .as_ref()
            .expect("Fillet")
            .previous_feature,
        f.base
    );
    assert_eq!(
        source.chamfer_bodies[0]
            .target
            .as_ref()
            .expect("Chamfer")
            .previous_feature,
        f.base
    );
    let mut editor = crate::sketch::Editor::default();
    assert!(editor.cuts.begin_edit(&f.path, &source, f.middle));
    editor.dismiss();
    assert!(editor.chamfers.begin_distance(&f.path, &source, f.tip));
    Ok(())
}

#[test]
fn native_rollback_widgets_restore_exact_versions_scenes_and_pick_the_shown_producer() -> Result<()>
{
    let Some(mut renderer) = super::native::renderer()? else {
        return Ok(());
    };
    let f = fixture()?;
    let original = cold(&f.path);
    // A saved marker also survives opening. Both subsequent command destinations
    // differ from this checkpoint, so both must make the document dirty.
    let mut document = Document::open(&f.path)?;
    document.write_changes(&[EditChange::SetRollback {
        object: f.body,
        feature: Some(f.base),
    }])?;
    document.close()?;
    let mut window = Window::new(Drawn::Native, None);
    window.open(&f.path);
    let initial_version = window.sessions.export_source().expect("version").version();
    for surface in 0..3 {
        let marked = command(&mut window, f.middle, Command::Rollback, surface);
        assert!(window.sessions.dirty());
        let version = window.sessions.export_source().expect("version").version();
        let path = window.sessions.export_path().expect("path");
        let bytes = std::fs::read(&path)?;
        let source: ExtrudeEditSource = marked.edit_source.clone().expect("facts").into();
        assert_eq!(
            source.fillet_bodies[0]
                .target
                .as_ref()
                .expect("target")
                .previous_feature,
            f.middle
        );
        let snapshot = Arc::new(marked.snapshot.clone());
        let prepared = renderer.prepare(Arc::clone(&snapshot))?;
        let mut camera = ferritecad_viewport::Camera::new();
        camera.resize(160, 160);
        camera.frame(snapshot.bounds().expect("Body"))?;
        camera.look_from(ferritecad_viewport::StandardView::Top);
        let frame = renderer.render(
            &prepared,
            &camera,
            crate::Marked::Nothing,
            crate::Hovered::Nothing,
            &crate::Visibility::new(&snapshot),
        )?;
        let mut picked = false;
        for (x, y) in (0..frame.height()).flat_map(|y| (0..frame.width()).map(move |x| (x, y))) {
            let hit = frame.hit_at(x, y);
            let selection = crate::selection_at(
                hit,
                &snapshot,
                &marked.faces,
                &marked.edges,
                &marked.vertices,
            );
            if let ferritecad_scene::Selection::Face(face) = &selection
                && let [meaning] = face.meanings()
            {
                let row = picked_row(&source, &selection, &marked.catalogue, &snapshot)
                    .expect("producer")
                    .expect("row");
                assert_eq!(row, (meaning.producer, Some(f.body), None));
                assert_ne!(
                    row.0, f.tip,
                    "hidden suffix must not supply the picked picture"
                );
                picked = true;
                break;
            }
        }
        assert!(picked, "a face of the shown result is pickable");
        let end = command(&mut window, f.body, Command::RollToEnd, surface);
        assert_eq!(end.snapshot, original.snapshot);
        assert_eq!(end.catalogue, original.catalogue);
        assert_eq!(end.faces, original.faces);
        assert_eq!(end.edges, original.edges);
        assert_eq!(end.vertices, original.vertices);
        assert!(window.sessions.dirty());
        let end_version = window.sessions.export_source().expect("version").version();
        for (undo, expected, expected_version) in
            [(true, &marked, version), (false, &end, end_version)]
        {
            let (address, path) = window.sessions.begin_move(undo).expect("history");
            assert_eq!(&accept(&mut window, address, path), expected);
            assert_eq!(
                window.sessions.export_source().expect("version").version(),
                expected_version
            );
            if undo {
                assert_eq!(
                    std::fs::read(window.sessions.export_path().expect("path"))?,
                    bytes
                );
            }
        }
    }
    assert_ne!(
        window.sessions.export_source().expect("version").version(),
        initial_version
    );
    Ok(())
}

#[test]
fn native_rollback_fillet_form_inserts_at_the_shown_result_and_undo_restores_it() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let f = fixture()?;
    let mut window = Window::new(Drawn::Native, None);
    window.open(&f.path);
    let before = command(&mut window, f.base, Command::Rollback, 0);
    let path = window.sessions.export_path().expect("snapshot");
    let bytes = std::fs::read(&path)?;
    let source = window.scene.edit_source.as_ref().expect("source");
    assert_eq!(
        source.fillet_bodies[0]
            .target
            .as_ref()
            .expect("Fillet")
            .previous_feature,
        f.base
    );
    assert_eq!(
        source.chamfer_bodies[0]
            .target
            .as_ref()
            .expect("Chamfer")
            .previous_feature,
        f.base
    );
    assert_eq!(
        source.cut_bodies[0]
            .target
            .as_ref()
            .expect("Cut")
            .shown_feature,
        f.base
    );
    let (form, request) = crate::fillets::tests::add_session::typed_add(
        &path,
        source,
        &window.sessions,
        [0., 0.],
        "2.00",
    );
    let inserted = request.changes[0].object();
    window.creates.sketch = form;
    let after = apply(&mut window, request.changes);
    assert!(window.sessions.dirty());
    let source = after.edit_source.as_ref().expect("facts");
    let ObjectPayload::Body(body) = &source
        .objects
        .iter()
        .find(|o| o.id == f.body)
        .expect("Body")
        .payload
    else {
        unreachable!("Body")
    };
    assert_eq!(body.rollback, Some(inserted));
    assert_eq!(body.tip_feature, Some(f.tip));
    assert_eq!(
        source
            .model_tree
            .rows
            .iter()
            .filter(|row| row.id.1 == Some(f.body) && row.id.2.is_none())
            .map(|row| row.id.0)
            .collect::<Vec<_>>(),
        vec![f.base, inserted, f.middle, f.tip]
    );
    let (address, path) = window.sessions.begin_move(true).expect("one Undo");
    assert_eq!(accept(&mut window, address, path), before);
    assert_eq!(
        std::fs::read(window.sessions.export_path().expect("path"))?,
        bytes
    );

    // Same form, a named edge that the later Chamfer already modifies: the owner
    // refuses the entire insertion and the open editor retains its draft.
    let path = window.sessions.export_path().expect("snapshot");
    let source = window.scene.edit_source.as_ref().expect("source");
    let (form, request) = crate::fillets::tests::add_session::typed_add(
        &path,
        source,
        &window.sessions,
        [60., 40.],
        "2.00",
    );
    window.creates.sketch = form;
    let draft = crate::fillets::tests::add_session::add_draft_state(&window.creates.sketch);
    let version = source.version;
    let (tx, rx) = mpsc::channel();
    let address = window
        .sessions
        .apply(version, request.changes, move |_, reply| {
            tx.send(reply).expect("reply");
        })
        .expect("apply");
    let reply = rx.recv_timeout(Duration::from_secs(60)).expect("reply");
    let error = reply.as_ref().expect_err("late edge refuses").to_string();
    assert!(matches!(
        window.sessions.finish_apply(address, reply),
        crate::sessions::Edited::Failed
    ));
    window
        .creates
        .sketch
        .fillets
        .set_outcome(&window.sessions.status);
    assert_eq!(
        crate::fillets::tests::add_session::add_draft_state(&window.creates.sketch),
        draft
    );
    assert_eq!(
        window.sessions.export_source().expect("version").version(),
        version
    );
    assert_eq!(
        std::fs::read(window.sessions.export_path().expect("path"))?,
        bytes
    );
    assert_eq!(cold(&window.sessions.export_path().expect("path")), before);
    assert_eq!(
        window.sessions.status,
        format!("Could not apply the change: {error}")
    );
    let document = Document::open_read_only(&path)?;
    let ObjectPayload::Chamfer(chamfer) = document.object(f.tip)?.expect("Chamfer").payload else {
        unreachable!("Chamfer")
    };
    let reason = format!(
        "unresolved topology reference: feature {}, reference {}: \
        edge deleted from the consumed result",
        f.tip, chamfer.edges[0].id
    );
    assert_eq!(
        error,
        format!(
            "Plate ({}), Chamfer ({}): edit would leave features unbuilt:\n\
         {}: {reason}\n\
         {}: not built: depends on {}: {reason}\n",
            f.body, f.tip, f.tip, f.body, f.tip
        )
    );
    Ok(())
}
