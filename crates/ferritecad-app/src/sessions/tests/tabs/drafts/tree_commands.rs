// SPDX-License-Identifier: MIT
use super::*;
use crate::commands::{self, Action, Command, Launch};
use ferritecad_document::{DocumentVersion, EditChange};
use ferritecad_types::StableEntityId;

#[test]
fn a_tree_command_rejects_each_stale_address_before_submitting_an_edit() {
    let (_root, path, source) = crate::cuts::tests::session_apply::fixture(1);
    let feature = crate::cuts::tests::session_apply::selected(&source, 0).feature;
    let mut w = Window::new(Drawn::Mock, None);
    super::model_tree::open_facts(&mut w, &path);
    let original = std::fs::read(&path).expect("source bytes");
    let version = w.sessions.export_source().expect("accepted").version();
    let source = w.scene.edit_source.as_ref().expect("facts").clone();
    let row = source
        .model_tree
        .rows
        .iter()
        .find(|r| r.id.0 == feature)
        .expect("feature row")
        .id;
    for stale in 0..5 {
        let navigation = w.tabs.model_navigation(w.sessions.tab());
        navigation.selected = Some(row);
        let mut action = Action {
            tab: w.sessions.tab().key(),
            epoch: navigation.epoch,
            row,
            edit: Command::Delete,
        };
        let mut reading = source.clone();
        match stale {
            0 => {}
            1 => action.tab += 1,
            2 => action.epoch += 1,
            3 => navigation.selected = None,
            4 => reading.version.content = ferritecad_types::ContentHash::of_bytes(b"older model"),
            _ => unreachable!("four stale address fields"),
        }
        let result = commands::launch(
            action,
            &reading,
            &w.scene.sketches,
            &mut w.creates,
            &mut w.edits,
            &w.sessions,
            &w.tabs,
            &Loads::default(),
            &Exports::default(),
            &w.input,
            false,
        );
        if stale == 0 {
            assert!(matches!(result, Ok(Launch::Apply(expected, _)) if expected == version));
        } else {
            let Err(reason) = result else {
                panic!("stale address field {stale} accepted");
            };
            assert_eq!(reason, "The command belongs to an older document view.");
        }
        assert_eq!(
            w.sessions.export_source().expect("accepted").version(),
            version
        );
        assert_eq!(std::fs::read(&path).expect("source bytes"), original);
        assert!(!w.sessions.dirty());
        assert!(w.sessions.begin_move(true).is_none());
    }
}

#[test]
fn native_conflicting_sketch_opens_with_named_rules_and_an_unbuilt_dependent() {
    if !native()
        || !ferritecad_fixtures::native_dependencies(&[(
            ferritecad_sketch_solver::is_available(),
            "PLANEGCS",
        )])
    {
        return;
    }
    let (_root, path, source, curves) = crate::constraints::tests::diameter::round_fixture(&[5.]);
    let sketch = source.constraint_sketches[0].sketch;
    let ids = [StableEntityId::new(), StableEntityId::new()];
    let mut document = Document::open(&path).expect("valid document");
    let changes: Vec<_> = ids
        .iter()
        .zip([5., 6.])
        .map(|(id, radius)| EditChange::AddConstraint {
            sketch,
            constraint: ferritecad_document::SketchConstraint {
                id: *id,
                rule: ferritecad_document::SketchConstraintRule::Radius {
                    curve: curves[0],
                    radius: ferritecad_document::Expression::constant(radius).expect("radius"),
                },
            },
        })
        .collect();
    document
        .write_changes(&changes)
        .expect("stored conflicting constraints");
    document.close().expect("close");
    let bytes = std::fs::read(&path).expect("source");
    let mut w = Window::new(Drawn::Native, None);
    w.open(&path);
    assert!(!w.sessions.dirty());
    let facts = w.scene.edit_source.as_ref().expect("open model");
    let row = facts
        .model_tree
        .rows
        .iter()
        .find(|r| r.id.0 == sketch)
        .expect("sketch row");
    assert!(row.note.contains("Constraint conflict:"), "{}", row.note);
    assert!(
        facts
            .model_tree
            .rows
            .iter()
            .any(|r| r.name.contains("Extrude1")
                && r.note.contains("not built")
                && r.note.contains("depends on"))
    );
    let solves = ferritecad_sketch_solver::native_solves();
    let context = egui::Context::default();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        crate::sketch_conflicts::panel(ui, &[], Some(facts));
    });
    output.textures_delta.clear();
    let text = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    for change in &changes {
        let EditChange::AddConstraint { constraint, .. } = change else {
            unreachable!();
        };
        let words = crate::ConstraintWords::of(constraint.id, &constraint.rule);
        assert!(words.says.starts_with("Radius: curve "));
        for expected in [&words.says, &words.identifier] {
            assert!(row.note.contains(expected), "{} lacks {expected}", row.note);
            assert!(text.contains(expected), "{text} lacks {expected}");
        }
    }
    assert!(text.contains(&sketch.to_string()));
    assert!(text.contains(&curves[0].to_string()));
    assert!(!text.contains("No solved constrained sketches"));
    assert_eq!(
        ferritecad_sketch_solver::native_solves(),
        solves,
        "presentation never solves"
    );
    let note = row.note.clone();
    let name = row.name.clone();
    let shown = w.sessions.export_path().expect("accepted file");
    let document = Document::open_read_only(&shown).expect("same model");
    let next = crate::sketch_facts::Source::read(&document).expect("stored facts");
    document.close().expect("close");
    crate::retarget_scene(&mut w.scene, shown, next);
    assert_eq!(
        w.scene
            .edit_source
            .as_ref()
            .expect("retargeted facts")
            .model_tree
            .rows
            .iter()
            .find(|r| r.id.0 == sketch)
            .expect("retargeted sketch")
            .note,
        note
    );
    assert_eq!(
        w.scene
            .edit_source
            .as_ref()
            .expect("retargeted facts")
            .model_tree
            .rows
            .iter()
            .find(|r| r.id.0 == sketch)
            .expect("retargeted sketch")
            .name,
        name
    );
    assert_eq!(std::fs::read(&path).expect("unchanged source"), bytes);
}

fn ask(w: &mut Window, object: ObjectId, command: Command) -> Launch {
    let source = w.scene.edit_source.as_ref().expect("model facts");
    let row = source
        .model_tree
        .rows
        .iter()
        .find(|r| r.id.0 == object)
        .expect("row")
        .id;
    let tab = w.sessions.tab();
    let navigation = w.tabs.model_navigation(tab);
    navigation.selected = Some(row);
    let action = Action {
        tab: tab.key(),
        epoch: navigation.epoch,
        row,
        edit: command,
    };
    commands::launch(
        action,
        source,
        &w.scene.sketches,
        &mut w.creates,
        &mut w.edits,
        &w.sessions,
        &w.tabs,
        &Loads::default(),
        &Exports::default(),
        &w.input,
        false,
    )
    .unwrap_or_else(|error| panic!("command: {error}"))
}

fn finish_picture(w: &mut Window, address: Address, path: PathBuf) -> LoadedScene {
    let old = w.sessions.export_source().expect("accepted").version();
    let (send, receive) = mpsc::channel();
    w.sessions.draw_staged(address, move |result| {
        send.send(result).expect("picture");
    });
    let scene = wait(&receive).expect("candidate scene");
    let cold = w.picture(&path).expect("independent cold scene");
    assert_eq!(scene, cold);
    assert_eq!(scene.snapshot.meshes(), cold.snapshot.meshes());
    assert_eq!(
        w.sessions
            .export_source()
            .expect("still accepted")
            .version(),
        old
    );
    w.present(&path, Ok(scene), Bind::Staged, Ok(()))
        .expect("publish picture");
    assert!(w.sessions.finish_scene(address, Ok(())));
    cold
}

fn apply(
    w: &mut Window,
    expected: DocumentVersion,
    changes: Vec<EditChange>,
) -> Option<LoadedScene> {
    let (send, receive) = mpsc::channel();
    let address = w
        .sessions
        .apply(expected, changes, move |_, result| {
            send.send(result).expect("transaction");
        })
        .expect("idle owner");
    match w.sessions.finish_apply(address, wait(&receive)) {
        Edited::Show(path) => Some(finish_picture(w, address, path)),
        Edited::Failed => None,
        Edited::Ignore | Edited::NoChange => panic!("a current changing command"),
        Edited::Keep(_) => panic!("geometry command must publish its picture"),
    }
}

fn execute(w: &mut Window, object: ObjectId, command: Command) -> Option<LoadedScene> {
    let Launch::Apply(expected, changes) = ask(w, object, command) else {
        panic!("an immediate model edit");
    };
    apply(w, expected, changes)
}

fn history(w: &mut Window, undo: bool) -> LoadedScene {
    let (address, path) = w.sessions.begin_move(undo).expect("one history step");
    finish_picture(w, address, path)
}

#[test]
fn native_picked_face_context_suppresses_the_same_feature_as_the_tree() {
    if !native() {
        return;
    }
    let (_root, path, source) = crate::cuts::tests::session_apply::fixture(3);
    let tip = crate::cuts::tests::session_apply::selected(&source, 2).feature;
    let mut w = Window::new(Drawn::Native, None);
    w.open(&path);
    let loaded = w.picture(&path).expect("cold scene");
    let snapshot = Arc::new(loaded.snapshot);
    let mut renderer = match ferritecad_viewport_gpu::Renderer::new() {
        Ok(renderer) => renderer,
        Err(error) if error.kind() == ferritecad_types::ErrorKind::Unsupported => {
            ferritecad_fixtures::native_dependencies(&[(false, "GPU")]);
            return;
        }
        Err(error) => panic!("GPU: {error}"),
    };
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("upload");
    let mut camera = ferritecad_viewport::Camera::new();
    camera.resize(480, 480);
    camera.look_from(ferritecad_viewport::StandardView::Isometric);
    camera
        .frame(snapshot.bounds().expect("body"))
        .expect("frame");
    let frame = renderer
        .render(
            &prepared,
            &camera,
            ferritecad_viewport::Marked::Nothing,
            ferritecad_viewport::Hovered::Nothing,
            &ferritecad_viewport::Visibility::new(&snapshot),
        )
        .expect("draw");
    let selection = (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .map(|(x, y)| {
            crate::selection_at(
                frame.hit_at(x, y),
                &snapshot,
                &loaded.faces,
                &loaded.edges,
                &loaded.vertices,
            )
        })
        .find(|selection| {
            matches!(selection, ferritecad_scene::Selection::Face(face)
            if face.meanings().len() == 1 && face.meanings()[0].producer == tip)
        })
        .expect("visible face created by the last Cut");
    let facts = w.scene.edit_source.as_ref().expect("facts");
    let row = commands::picked_row(facts, &selection, &loaded.catalogue, &snapshot)
        .expect("producer")
        .expect("row");
    commands::select_row(facts, w.tabs.model_navigation(w.sessions.tab()), row);
    let menu = commands::Menu::new(&w.sessions, &mut w.tabs, row, egui::pos2(20., 20.));
    let context = egui::Context::default();
    let paint = |events| {
        let mut action = None;
        let mut out = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                action = menu.draw(ui.ctx(), facts, Ok(())).0;
            },
        );
        out.textures_delta.clear();
        (out, action)
    };
    let mut output = paint(vec![]).0;
    for _ in 0..3 {
        output = paint(vec![]).0;
    }
    let position = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Suppress" => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()).center())
            }
            _ => None,
        })
        .expect("shared Suppress item");
    let action = paint(vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    ])
    .1
    .expect("context command");
    assert_eq!(action.edit, Command::Suppress);
    assert_eq!(action.row.0, tip);
    let Launch::Apply(expected, changes) = commands::launch(
        action,
        facts,
        &w.scene.sketches,
        &mut w.creates,
        &mut w.edits,
        &w.sessions,
        &w.tabs,
        &Loads::default(),
        &Exports::default(),
        &w.input,
        false,
    )
    .expect("context route") else {
        panic!("one Apply");
    };
    let Launch::Apply(tree_version, tree_changes) = ask(&mut w, tip, Command::Suppress) else {
        panic!("tree route");
    };
    assert_eq!(expected, tree_version);
    assert_eq!(changes, tree_changes);
    let from_menu = apply(&mut w, expected, changes).expect("context suppression");
    history(&mut w, true);
    let from_tree = apply(&mut w, tree_version, tree_changes).expect("tree suppression");
    assert_eq!(from_menu.snapshot, from_tree.snapshot);
    assert_eq!(from_menu.catalogue, from_tree.catalogue);
    assert_eq!(from_menu.faces, from_tree.faces);
    assert_eq!(from_menu.edges, from_tree.edges);
    assert_eq!(from_menu.vertices, from_tree.vertices);
    assert_eq!(from_menu.rebuild, from_tree.rebuild);
    assert_eq!(from_menu.rebuild_facts, from_tree.rebuild_facts);
}

#[test]
fn native_tree_deletion_refusal_confirmation_and_single_undo_publish_atomically() {
    if !native() {
        return;
    }
    let (_root, path, source) = crate::cuts::tests::session_apply::fixture(3);
    let tip = crate::cuts::tests::session_apply::selected(&source, 2).feature;
    let used_sketch = crate::cuts::tests::session_apply::selected(&source, 0).tool_sketch;
    let original = std::fs::read(&path).expect("source bytes");
    let mut w = Window::new(Drawn::Native, None);
    w.open(&path);
    let accepted_bytes = std::fs::read(w.sessions.export_path().expect("accepted snapshot"))
        .expect("accepted bytes");
    let before = w.picture(&path).expect("original scene");
    let deleted = execute(&mut w, tip, Command::Delete).expect("tip removed");
    assert!(w.sessions.dirty());
    assert!(
        !w.scene
            .edit_source
            .as_ref()
            .expect("facts")
            .objects
            .iter()
            .any(|o| o.id == tip)
    );
    assert_eq!(history(&mut w, true), before);
    assert!(!w.sessions.dirty());
    assert_eq!(
        std::fs::read(w.sessions.export_path().expect("restored file")).expect("read"),
        accepted_bytes
    );
    assert_eq!(history(&mut w, false), deleted);
    history(&mut w, true);
    let current = w.sessions.export_source().expect("current");
    let version = current.version();
    let bytes = std::fs::read(current.path()).expect("accepted bytes");
    let mut document = Document::open(current.path()).expect("document");
    let expected = document
        .write_changes(&[EditChange::RemoveObject {
            object: used_sketch,
        }])
        .expect_err("used sketch refuses")
        .to_string();
    document.close().expect("close");
    assert!(execute(&mut w, used_sketch, Command::Delete).is_none());
    assert!(
        w.sessions.status.contains(&expected),
        "{} must include {expected}",
        w.sessions.status
    );
    assert_eq!(
        w.sessions.export_source().expect("current").version(),
        version
    );
    assert_eq!(std::fs::read(current.path()).expect("bytes"), bytes);
    assert_eq!(w.picture(current.path()).expect("scene"), before);

    for delete in [false, true] {
        let Launch::Confirm(confirmation) = ask(&mut w, used_sketch, Command::DeleteWithDependents)
        else {
            panic!("confirmation required");
        };
        let document = Document::open_read_only(current.path()).expect("accepted");
        let closure = document.dependent_closure(used_sketch).expect("closure");
        let expected: Vec<_> = closure
            .iter()
            .map(|id| {
                let row = document.object(*id).expect("object").expect("exists");
                format!(
                    "{} · {}",
                    row.name.as_deref().unwrap_or("Unnamed"),
                    row.payload.kind().expect("kind").label()
                )
            })
            .collect();
        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            assert_eq!(confirmation.draw(ui), None);
        });
        output.textures_delta.clear();
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        for label in &expected {
            assert!(labels.contains(label), "missing {label}");
        }
        let listed: Vec<_> = labels
            .iter()
            .filter(|label| expected.contains(label))
            .collect();
        assert_eq!(listed, expected.iter().collect::<Vec<_>>());
        let result = confirmation
            .finish(
                delete,
                w.scene.edit_source.as_ref(),
                &w.sessions,
                &w.tabs,
                Ok(()),
            )
            .expect("confirmed current version");
        if delete {
            let (expected, changes) = result.expect("one batch");
            assert_eq!(changes.len(), closure.len());
            let deleted = apply(&mut w, expected, changes).expect("closure accepted");
            assert!(w.sessions.dirty());
            for id in closure {
                assert!(
                    !w.scene
                        .edit_source
                        .as_ref()
                        .expect("facts")
                        .objects
                        .iter()
                        .any(|o| o.id == id)
                );
            }
            assert_eq!(history(&mut w, true), before);
            assert!(
                w.sessions.begin_move(true).is_none(),
                "one Undo for the whole batch"
            );
            assert_eq!(history(&mut w, false), deleted);
        } else {
            assert!(result.is_none());
            assert_eq!(
                w.sessions.export_source().expect("current").version(),
                version
            );
            assert_eq!(std::fs::read(current.path()).expect("bytes"), bytes);
            assert!(!w.sessions.dirty());
        }
    }
    assert_eq!(std::fs::read(&path).expect("unsaved source"), original);
}

#[test]
fn native_tree_suppression_rebuilds_only_the_suffix_and_undo_restores_the_picture() {
    if !native() {
        return;
    }
    let (_root, path, source) = crate::cuts::tests::session_apply::fixture(3);
    let middle = crate::cuts::tests::session_apply::selected(&source, 1).feature;
    let bytes = std::fs::read(&path).expect("source");
    let mut w = Window::new(Drawn::Native, None);
    let probe = crate::sessions::test_open::counting::SharedProbe::default();
    let shared = Arc::clone(&probe);
    let (scene, tab) = crate::sessions::open_for_view_with(
        w.private.path(),
        &path,
        &OperationContext::default(),
        move || {
            let mut kernel = crate::sessions::test_open::counting::Counting::wrap(
                ferritecad_occt::OcctKernel::new()?,
            );
            kernel.probe = Some(shared);
            Ok(kernel)
        },
        |kernel, bytes| kernel.inner.import_step(bytes),
    )
    .expect("counted owner");
    assert_eq!(probe.lock().expect("probe").live, 4);
    let shown = tab.current().path().to_owned();
    w.present(&shown, Ok(scene), Bind::Open(Box::new(tab)), Ok(()))
        .expect("open");
    let original = w.picture(&shown).expect("original cold");
    let mut suppressed = None;
    for (operation, work, live, is_suppressed) in [
        (0, (1, 1, 1), 3, true),
        (1, (2, 2, 1), 4, false),
        (2, (1, 1, 1), 3, true),
        (3, (2, 2, 1), 4, false),
    ] {
        let before = probe.lock().expect("probe").calls;
        let scene = match operation {
            0 => execute(&mut w, middle, Command::Suppress).expect("suppress"),
            1 => execute(&mut w, middle, Command::Unsuppress).expect("unsuppress"),
            2 => history(&mut w, true),
            3 => history(&mut w, false),
            _ => unreachable!("four named steps"),
        };
        let state = probe.lock().expect("probe");
        assert_eq!(
            (
                state.calls.extrude_count - before.extrude_count,
                state.calls.boolean - before.boolean,
                state.calls.tessellate - before.tessellate
            ),
            work
        );
        assert_eq!(state.calls.fillet, before.fillet);
        assert_eq!(state.calls.chamfer, before.chamfer);
        assert_eq!(state.calls.restore, before.restore);
        assert_eq!(state.live, live);
        let row = w
            .scene
            .edit_source
            .as_ref()
            .expect("facts")
            .model_tree
            .rows
            .iter()
            .find(|row| row.id.0 == middle)
            .expect("feature");
        assert_eq!(row.name.contains("suppressed"), is_suppressed);
        if is_suppressed {
            if let Some(expected) = &suppressed {
                assert_eq!(&scene, expected);
            } else {
                suppressed = Some(scene);
            }
        } else {
            // Unsuppress is a new revision; its model must equal the original,
            // while its edit-source version records the newly accepted file.
            assert_eq!(scene.rebuild_facts, original.rebuild_facts);
            assert_eq!(scene.rebuild, original.rebuild);
            assert_eq!(scene.snapshot, original.snapshot);
            assert_eq!(scene.catalogue, original.catalogue);
            assert_eq!(scene.faces, original.faces);
            assert_eq!(scene.edges, original.edges);
            assert_eq!(scene.vertices, original.vertices);
            assert_eq!(scene.sketch_solves, original.sketch_solves);
            assert_eq!(scene.sketch_conflicts, original.sketch_conflicts);
            assert_eq!(scene.sketch_presentations, original.sketch_presentations);
            assert_eq!(
                scene.edit_source.as_ref().expect("facts").objects,
                original
                    .edit_source
                    .as_ref()
                    .expect("original facts")
                    .objects
            );
        }
        assert_eq!(std::fs::read(&path).expect("source preserved"), bytes);
    }
}
