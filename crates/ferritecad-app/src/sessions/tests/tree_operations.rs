// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::EditChange;

fn shown(scene: &LoadedScene, feature: ObjectId, suppressed: bool) {
    assert!(scene.rebuild.all_built());
    assert_eq!(scene.rebuild.suppressed.contains(&feature), suppressed);
    assert_eq!(scene.snapshot.is_empty(), suppressed);
    let source: crate::sketch_facts::Source = scene.edit_source.clone().expect("facts").into();
    assert_eq!(source.model_tree.unbuilt_features, 0);
    let row = source
        .model_tree
        .rows
        .iter()
        .find(|row| row.id.0 == feature)
        .expect("feature row");
    assert_eq!(row.name.contains("suppressed"), suppressed);
    let rows = crate::workspace::model_rows(Some(&source));
    let context = egui::Context::default();
    let mut navigation = ferritecad_ui::ModelNavigation::default();
    let mut output = None;
    for _ in 0..3 {
        let mut frame = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(420., 900.),
                )),
                ..Default::default()
            },
            |ui| {
                ferritecad_ui::model_panel::<ferritecad_ui::ModelEdit>(
                    ui,
                    0,
                    &rows,
                    &mut navigation,
                    |_| vec![],
                );
            },
        );
        frame.textures_delta.clear();
        output = Some(frame);
    }
    let visible = output.expect("frame").shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(t) if t.galley.text().contains("suppressed"))
    });
    assert_eq!(
        visible, suppressed,
        "the tree draws the status without selecting the row"
    );
}

fn accept(sessions: &mut Sessions, address: Address, feature: ObjectId, suppressed: bool) {
    let (tx, rx) = mpsc::channel();
    sessions.draw_staged(address, move |scene| {
        tx.send(scene).expect("deliver picture")
    });
    let scene = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("picture")
        .expect("scene");
    shown(&scene, feature, suppressed);
    sessions.bind(Bind::Staged).expect("commit");
    assert!(sessions.finish_scene(address, Ok(())));
}

#[test]
fn native_suppressed_document_opens_and_undo_redo_preserve_the_visible_status() {
    if !native() {
        return;
    }
    let f = fixture();
    let mut document = Document::open(&f.file).expect("document");
    document
        .write_changes(&[EditChange::SetSuppressed {
            object: f.feature,
            suppressed: true,
        }])
        .expect("suppress root");
    document.close().expect("close");
    let source_bytes = std::fs::read(&f.file).expect("bytes");
    let session = DocumentSession::open(&f.file).expect("session");
    let (scene, tab) = owner::start(&OperationContext::default(), move |_| {
        Ok((session, ferritecad_occt::OcctKernel::new()?))
    })
    .expect("open suppressed document");
    shown(&scene, f.feature, true);
    let mut sessions = Sessions::default();
    sessions.adopt(tab);
    let version = sessions.export_source().expect("current").version();
    let (tx, rx) = mpsc::channel();
    let address = sessions
        .apply(
            version,
            vec![EditChange::SetSuppressed {
                object: f.feature,
                suppressed: false,
            }],
            move |_, result| tx.send(result).expect("reply"),
        )
        .expect("apply");
    let reply = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("reply");
    assert!(matches!(
        sessions.finish_apply(address, reply),
        Edited::Show(_)
    ));
    accept(&mut sessions, address, f.feature, false);
    for (undo, suppressed) in [(true, true), (false, false)] {
        let (address, _) = sessions.begin_move(undo).expect("history");
        accept(&mut sessions, address, f.feature, suppressed);
    }
    assert_eq!(
        std::fs::read(&f.file).expect("source unchanged"),
        source_bytes
    );
    sessions.stop_all();
}
