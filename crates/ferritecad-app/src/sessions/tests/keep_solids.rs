// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{EditChange, EntityReference, KeepSelection, ObjectPayload};
use ferritecad_jobs::EditRequest;
use ferritecad_kernel::{GeometryKernel, TessellationParams};
use ferritecad_viewport::{EdgePickId, FacePickId, VertexPickId};

fn frame(
    context: &egui::Context,
    commands: &mut crate::keep_solids::Commands,
    path: &Path,
    source: &crate::sketch_facts::Source,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 900.),
            )),
            events,
            ..Default::default()
        },
        |ui| commands.draw(ui, Some(path), Some(source)),
    );
    output.textures_delta.clear();
    output
}

fn click(
    context: &egui::Context,
    commands: &mut crate::keep_solids::Commands,
    path: &Path,
    source: &crate::sketch_facts::Source,
    label: &str,
) {
    click_row(context, commands, path, source, label, 0);
}

fn click_row(
    context: &egui::Context,
    commands: &mut crate::keep_solids::Commands,
    path: &Path,
    source: &crate::sketch_facts::Source,
    label: &str,
    row: usize,
) {
    for _ in 0..3 {
        frame(context, commands, path, source, vec![]);
    }
    let output = frame(context, commands, path, source, vec![]);
    let at = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.visual_bounding_rect().center())
            }
            _ => None,
        })
        .nth(row)
        .unwrap_or_else(|| panic!("missing widget {label}"));
    frame(
        context,
        commands,
        path,
        source,
        vec![egui::Event::PointerMoved(at)],
    );
    for pressed in [true, false] {
        frame(
            context,
            commands,
            path,
            source,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }],
        );
    }
}

fn picture(
    sessions: &Sessions,
    renderer: &mut ferritecad_viewport_gpu::Renderer,
) -> crate::LiveScene<crate::PreparedSnapshot> {
    let path = sessions.export_path().expect("session source");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let loaded = ferritecad_scene::snapshot_of(
        &path,
        &mut kernel,
        |k, bytes| k.import_step(bytes),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("scene");
    assert_eq!(loaded.snapshot.meshes().len(), 1);
    assert_eq!(loaded.snapshot.draws().len(), 1);
    let snapshot = Arc::new(loaded.snapshot);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("upload");
    crate::LiveScene {
        edit_source: loaded.edit_source.map(Into::into),
        document: Some(path),
        prepared,
        catalogue: loaded.catalogue,
        faces: loaded.faces,
        edges: loaded.edges,
        vertices: loaded.vertices,
        visibility: crate::Visibility::new(&snapshot),
        selection: Default::default(),
        hovered: crate::Hovered::Nothing,
        sketch_solves: loaded.sketch_solves,
        sketches: Default::default(),
    }
}

fn pick(
    scene: &mut crate::LiveScene<crate::PreparedSnapshot>,
    renderer: &mut ferritecad_viewport_gpu::Renderer,
    wanted: EntityReference,
) {
    let snapshot = scene.prepared.snapshot();
    let mut camera = ferritecad_viewport::Camera::new();
    camera.resize(480, 480);
    camera
        .frame(snapshot.bounds().expect("bounds"))
        .expect("camera");
    for view in [
        ferritecad_viewport::StandardView::Left,
        ferritecad_viewport::StandardView::Right,
    ] {
        camera.look_from(view);
        let pixels = renderer
            .render(
                &scene.prepared,
                &camera,
                crate::Marked::Nothing,
                crate::Hovered::Nothing,
                &scene.visibility,
            )
            .expect("pixels");
        let hit = (0..pixels.height())
            .flat_map(|y| (0..pixels.width()).map(move |x| (x, y)))
            .map(|(x, y)| pixels.hit_at(x, y))
            .find(|hit| {
                hit.face() != FacePickId::NOTHING
                    && hit.edge() == EdgePickId::NOTHING
                    && hit.vertex() == VertexPickId::NOTHING
                    && scene.faces.of(hit.face(), snapshot).iter().any(|meaning| {
                        meaning.digest == wanted.meaning.digest
                            && meaning.producer == wanted.meaning.producer
                    })
            });
        if let Some(hit) = hit {
            scene.selection =
                crate::selection_at(hit, snapshot, &scene.faces, &scene.edges, &scene.vertices);
            assert!(matches!(
                scene.selection,
                ferritecad_scene::Selection::Face(_)
            ));
            return;
        }
    }
    panic!("named witness was not pickable on its component");
}

fn peer_and_apply(sessions: &mut Sessions, request: &EditRequest, root: &Path) {
    let peer = root.join("peer.fcad");
    let file = root.join("edit.json");
    std::fs::write(
        &file,
        ferritecad_fixtures::edit_request_json(&request.changes).to_string(),
    )
    .expect("request");
    cli(&[
        "edit".as_ref(),
        request.source.as_os_str(),
        "--expect-version".as_ref(),
        request.expected.content.to_string().as_ref(),
        "--request".as_ref(),
        file.as_os_str(),
        "--output".as_ref(),
        peer.as_os_str(),
    ]);
    assert!(matches!(
        super::fillet::apply(sessions, request),
        Edited::Show(_)
    ));
    assert_eq!(
        super::fillet::sql(&sessions.export_path().expect("current")),
        super::fillet::sql(&peer),
        "every SQL cell except meta.modified_at"
    );
    std::fs::remove_file(peer).expect("remove peer");
}

fn count(sessions: &Sessions) -> usize {
    let document =
        Document::open_read_only(sessions.export_path().expect("current")).expect("open");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = ferritecad_eval::rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("rebuild");
    assert!(built.report().all_built());
    let body = ferritecad_fixtures::feature_changes::body(&document).expect("body");
    let n = kernel
        .solid_count(built.shape(body).expect("shape"))
        .expect("count");
    built.release_all(&mut kernel);
    n
}

fn visible_count(source: &crate::sketch_facts::Source, expected: &str) {
    let context = egui::Context::default();
    let rows = crate::workspace::model_rows(Some(source));
    // No selected row or hover: this must be text in the tree itself.
    let mut navigation = ferritecad_ui::ModelNavigation::default();
    let mut output = None;
    for _ in 0..3 {
        output = Some(context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360., 900.),
                )),
                ..Default::default()
            },
            |ui| {
                ferritecad_ui::model_panel(ui, 0, &rows, &mut navigation, |_| vec![], Ok(()));
            },
        ));
    }
    assert!(output.expect("frame").shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == expected)
    }));
}

#[test]
fn native_face_piece_commands_match_cli_repair_history_and_save() {
    if !native() {
        return;
    }
    let mut renderer = match ferritecad_viewport_gpu::Renderer::new() {
        Ok(r) => r,
        Err(e) if e.kind() == ferritecad_types::ErrorKind::Unsupported => {
            ferritecad_fixtures::native_dependencies(&[(false, "GPU")]);
            return;
        }
        Err(e) => panic!("renderer: {e}"),
    };
    let root = tempfile::tempdir().expect("root");
    let private = tempfile::tempdir().expect("private");
    let path = root.path().join("set.fcad");
    let mut doc = Document::create(&path).expect("document");
    let plate = ferritecad_fixtures::result_sets::plate(&mut doc).expect("plate");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let slot = ferritecad_fixtures::result_sets::slot(&doc, &plate, &mut kernel).expect("slot");
    // Start without KeepSolids so the first actual widget command adds it.
    doc.write_changes(&slot.changes[..2]).expect("split");
    doc.close().expect("close");
    let mut sessions = Sessions::default();
    sessions.adopt(
        crate::sessions::test_open::open_in(private.path(), &path, HistoryLimits::default())
            .expect("open"),
    );
    let mut scene = picture(&sessions, &mut renderer);
    let source = scene.edit_source.as_ref().expect("source").clone();
    visible_count(&source, "2 solids");
    assert!(
        source
            .model_tree
            .rows
            .iter()
            .any(|row| row.id.0 == plate.body && row.note.starts_with("2 solids;"))
    );
    for witness in [slot.side, slot.other_side] {
        pick(&mut scene, &mut renderer, witness);
    }
    let context = egui::Context::default();
    let mut commands = crate::keep_solids::Commands::default();
    commands.update(&scene, &sessions, true);
    click(&context, &mut commands, &path, &source, "Keep solid pieces");
    scene.edit_source.as_mut().expect("source").version.content =
        ferritecad_types::ContentHash::of_bytes(b"stale scene");
    commands.update(&scene, &sessions, true);
    click(
        &context,
        &mut commands,
        &path,
        &source,
        "Keep selected piece",
    );
    assert!(
        commands.take_request().is_none(),
        "stale picking must refuse"
    );
    let output = frame(&context, &mut commands, &path, &source, vec![]);
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(t) if t.galley.text().contains("older than the document"))));
    scene.edit_source = Some(source.clone());
    commands.update(&scene, &sessions, true);
    click(
        &context,
        &mut commands,
        &path,
        &source,
        "Keep selected piece",
    );
    let request = commands.take_request().expect("add selected piece");
    let keep = request.changes[0].object();
    peer_and_apply(&mut sessions, &request, root.path());
    assert_eq!(count(&sessions), 1);
    for (undo, expected) in [(true, 2), (false, 1)] {
        let (generation, _) = sessions.begin_move(undo).expect("history");
        sessions.bind(Bind::Staged).expect("bind");
        assert!(sessions.finish_scene(generation, Ok(())));
        assert_eq!(count(&sessions), expected);
    }
    let mut scene = picture(&sessions, &mut renderer);
    let source = scene.edit_source.as_ref().expect("source").clone();
    visible_count(&source, "1 solid");
    assert!(
        source
            .model_tree
            .rows
            .iter()
            .any(|row| { row.id.0 == plate.body && row.note.starts_with("1 solid;") })
    );
    commands.update(&scene, &sessions, true);
    let current = sessions.export_path().expect("selected current");
    click(&context, &mut commands, &current, &source, "All");
    let request = commands.take_request().expect("restore the complete set");
    peer_and_apply(&mut sessions, &request, root.path());
    assert_eq!(count(&sessions), 2);
    scene = picture(&sessions, &mut renderer);
    pick(&mut scene, &mut renderer, slot.other_side);
    commands.update(&scene, &sessions, true);
    let current = sessions.export_path().expect("complete current");
    click(
        &context,
        &mut commands,
        &current,
        scene.edit_source.as_ref().expect("source"),
        "Keep selected piece",
    );
    let request = commands.take_request().expect("replace the selection");
    peer_and_apply(&mut sessions, &request, root.path());
    assert_eq!(count(&sessions), 1);
    super::fillet::saved(&mut sessions, SaveTarget::InPlace);
    assert_eq!(
        super::fillet::sql(&path),
        super::fillet::sql(&sessions.export_path().expect("current"))
    );
    let saved = root.path().join("saved.fcad");
    super::fillet::saved(&mut sessions, SaveTarget::As(saved.clone()));
    assert_eq!(
        super::fillet::sql(&saved),
        super::fillet::sql(&sessions.export_path().expect("current"))
    );
    // Deliberately persist a disappeared witness to exercise opening for repair.
    sessions.stop_all();
    let mut doc = Document::open(&saved).expect("saved");
    let missing = slot.side;
    let EditChange::AddSketch {
        object: tool,
        sketch,
        ..
    } = &slot.changes[0]
    else {
        panic!("slot profile");
    };
    let removed = ferritecad_fixtures::result_sets::rectangle(plate.plane, [-2., -2.], [32., 42.])
        .expect("remove the left piece");
    let mut changes = vec![EditChange::SetKeepSelection {
        object: keep,
        selection: KeepSelection::ContainingFaces(vec![missing]),
    }];
    changes.extend(
        sketch
            .curves
            .iter()
            .zip(removed.curves)
            .map(|(before, after)| EditChange::SetCurveGeometry {
                sketch: *tool,
                curve: before.id,
                geometry: after.geometry,
            }),
    );
    doc.write_changes(&changes)
        .expect("upstream edit deletes the witness");
    doc.close().expect("close");
    for all in [false, true] {
        sessions.adopt(
            crate::sessions::test_open::open_in(private.path(), &saved, HistoryLimits::default())
                .expect("broken document opens"),
        );
        let mut scene = picture(&sessions, &mut renderer);
        let source = scene.edit_source.as_ref().expect("source").clone();
        assert!(source.model_tree.rows.iter().any(|row| row.id.0 == keep
            && row.note.contains(&missing.id.to_string())
            && row.note.starts_with("Failed:")));
        pick(&mut scene, &mut renderer, slot.other_side);
        commands.update(&scene, &sessions, true);
        let current = sessions.export_path().expect("broken current");
        click(
            &context,
            &mut commands,
            &current,
            &source,
            if all { "All" } else { "Keep selected piece" },
        );
        let request = commands.take_request().expect("repair request");
        assert!(
            matches!(&request.changes[0], EditChange::SetKeepSelection { object, .. }
            if *object == keep)
        );
        peer_and_apply(&mut sessions, &request, root.path());
        assert_eq!(count(&sessions), 1);
        let source_path = sessions.export_path().expect("current");
        let document = Document::open_read_only(&source_path).expect("document");
        assert_eq!(
            document
                .objects()
                .expect("objects")
                .iter()
                .filter(|o| matches!(o.payload, ObjectPayload::KeepSolids(_)))
                .count(),
            1
        );
        sessions.stop_all();
    }
}

#[test]
fn native_piece_selection_refuses_newly_failed_dependents_atomically() {
    if !native() {
        return;
    }
    let mut renderer = match ferritecad_viewport_gpu::Renderer::new() {
        Ok(renderer) => renderer,
        Err(error) if error.kind() == ferritecad_types::ErrorKind::Unsupported => {
            ferritecad_fixtures::native_dependencies(&[(false, "GPU")]);
            return;
        }
        Err(error) => panic!("renderer: {error}"),
    };
    let root = tempfile::tempdir().expect("root");
    let private = tempfile::tempdir().expect("private");
    let path = root.path().join("dependent.fcad");
    let mut document = Document::create(&path).expect("document");
    let plate = ferritecad_fixtures::result_sets::plate(&mut document).expect("plate");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let slot =
        ferritecad_fixtures::result_sets::slot(&document, &plate, &mut kernel).expect("slot");
    document
        .write_changes(&slot.changes)
        .expect("split and Keep All");
    let (sketch, cut) = (ObjectId::new(), ObjectId::new());
    document
        .write_changes(&[
            EditChange::AddSketch {
                object: sketch,
                name: None,
                sketch: ferritecad_fixtures::datum_planes::circle(plate.plane, [10., 20.], 2.)
                    .expect("left piece tool"),
            },
            EditChange::AddFeature {
                object: cut,
                body: plate.body,
                name: Some("Left hole".into()),
                feature: ferritecad_fixtures::result_sets::extrusion(
                    sketch,
                    ferritecad_document::SolidOperation::Cut,
                    11.,
                )
                .expect("Cut"),
            },
        ])
        .expect("dependent");
    document.close().expect("close");
    let original = std::fs::read(&path).expect("source");
    let mut sessions = Sessions::default();
    sessions.adopt(
        crate::sessions::test_open::open_in(private.path(), &path, HistoryLimits::default())
            .expect("open"),
    );
    let mut scene = picture(&sessions, &mut renderer);
    pick(&mut scene, &mut renderer, slot.other_side);
    let source = scene.edit_source.as_ref().expect("source");
    let mut commands = crate::keep_solids::Commands::default();
    commands.update(&scene, &sessions, true);
    let context = egui::Context::default();
    click(&context, &mut commands, &path, source, "Keep solid pieces");
    // Body addition is the first row; address the existing KeepSolids below it.
    click_row(
        &context,
        &mut commands,
        &path,
        source,
        "Keep selected piece",
        1,
    );
    let request = commands.take_request().expect("selection");
    assert!(
        matches!(request.changes[0], EditChange::SetKeepSelection { object, .. }
        if object == slot.keep)
    );
    let before = super::fillet::state(&sessions);
    let cells = super::fillet::sql(&sessions.export_path().expect("current"));
    assert_eq!(
        super::fillet::apply(&mut sessions, &request),
        Edited::Failed
    );
    assert!(
        sessions.status.contains(&cut.to_string()),
        "{}",
        sessions.status
    );
    assert!(sessions.status.contains("material"), "{}", sessions.status);
    assert_eq!(super::fillet::state(&sessions), before);
    assert_eq!(
        super::fillet::sql(&sessions.export_path().expect("current")),
        cells
    );
    assert_eq!(std::fs::read(path).expect("source"), original);
}
