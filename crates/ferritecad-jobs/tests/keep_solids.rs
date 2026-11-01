// SPDX-License-Identifier: MIT
use ferritecad_document::*;
use ferritecad_jobs::DocumentSession;
use ferritecad_kernel::{OperationContext, TessellationParams};

fn contents(path: &std::path::Path) -> (Vec<ObjectRecord>, Vec<Dependency>) {
    let doc = Document::open_read_only(path).expect("document");
    (
        doc.objects().expect("objects"),
        doc.dependencies().expect("dependencies"),
    )
}

#[test]
fn selection_transaction_undo_redo_and_lost_upstream_witness_are_atomic() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.fcad");
    let mut doc = Document::create(&source).expect("document");
    let p = ferritecad_fixtures::result_sets::plate(&mut doc).expect("plate");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let slot = ferritecad_fixtures::result_sets::slot(&doc, &p, &mut kernel).expect("slot");
    doc.close().expect("close");
    let source_bytes = std::fs::read(&source).expect("source");
    let mut session = DocumentSession::open(&source).expect("session");
    session
        .current()
        .load_scene(
            &mut kernel,
            |_, _| unreachable!("no imports"),
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("initial scene");
    for changes in [
        slot.changes.clone(),
        vec![EditChange::SetKeepSelection {
            object: slot.keep,
            selection: KeepSelection::ContainingFaces(vec![slot.side]),
        }],
    ] {
        let before = contents(session.current().path());
        let step = session
            .begin_step()
            .edit(
                changes,
                session.current().version(),
                &mut kernel,
                |_, _| unreachable!("no imports"),
                &OperationContext::default(),
            )
            .expect("step");
        session.commit_step(step).expect("publish");
        let after = contents(session.current().path());
        assert_ne!(before, after);
        session
            .commit_move(session.begin_undo().expect("undo"))
            .expect("undo publication");
        assert_eq!(contents(session.current().path()), before);
        session
            .commit_move(session.begin_redo().expect("redo"))
            .expect("redo publication");
        assert_eq!(contents(session.current().path()), after);
    }
    let before = contents(session.current().path());
    let model = session.current().model();
    // Change the preceding KeepSolids first; its dependent witness now disappears.
    let downstream = ferritecad_types::ObjectId::new();
    let step = session
        .begin_step()
        .edit(
            vec![EditChange::AddFeature {
                object: downstream,
                body: p.body,
                name: None,
                feature: FeatureAddition::KeepSolids {
                    selection: KeepSelection::ContainingFaces(vec![slot.side]),
                },
            }],
            session.current().version(),
            &mut kernel,
            |_, _| unreachable!("no imports"),
            &OperationContext::default(),
        )
        .expect("dependent witness");
    session.commit_step(step).expect("publish dependent");
    let accepted = contents(session.current().path());
    let refused = session
        .begin_step()
        .edit(
            vec![EditChange::SetKeepSelection {
                object: slot.keep,
                selection: KeepSelection::ContainingFaces(vec![slot.other_side]),
            }],
            session.current().version(),
            &mut kernel,
            |_, _| unreachable!("no imports"),
            &OperationContext::default(),
        )
        .expect_err("lost dependent witness refuses entire edit");
    let report = ferritecad_eval::RebuildRefusal::from_error(&refused).expect("D2 reasons");
    assert!(
        report
            .report
            .failed
            .iter()
            .any(|f| f.id == downstream && f.message.contains("deleted"))
    );
    assert_eq!(contents(session.current().path()), accepted);
    session
        .commit_move(session.begin_undo().expect("undo addition"))
        .expect("undo");
    assert_eq!(contents(session.current().path()), before);
    assert_eq!(session.current().model(), model);
    assert_eq!(
        std::fs::read(&source).expect("source unchanged"),
        source_bytes
    );
}
