// SPDX-License-Identifier: MIT
use ferritecad_document::{
    Document, EditChange, ExpressionSlot, ObjectPayload, SketchGeometry, SketchVertex,
    move_sketch_vertices,
};
use ferritecad_eval::{RebuildRefusal, rebuild_cold};
use ferritecad_jobs::{DocumentSession, EditRequest, edit_copy};
use ferritecad_kernel::OperationContext;

#[test]
fn a_side_and_radius_are_one_transaction_while_one_vertex_is_refused() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    let chain = ferritecad_fixtures::write_fillet_chain(&source, 8.0).expect("fixture");
    let before = std::fs::read(&source).expect("source bytes");
    let context = OperationContext::default();
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let original = Document::open_read_only(&source).expect("source");
    let original_names = model_names::names_with(&original, &mut kernel);
    let reading = ferritecad_jobs::read_extrude_source(&source).expect("reading");
    let profile = original
        .object(chain.sketch)
        .expect("read")
        .expect("sketch");
    let ObjectPayload::Sketch(sketch) = profile.payload else {
        unreachable!("fixture sketch")
    };
    for (both_vertices, radius, refusal) in [
        (false, 8.0, Some("not an axis-aligned rectangle")),
        (false, 2.0, Some("not an axis-aligned rectangle")),
        (true, 8.0, Some("too large")),
        (true, 2.0, None),
    ] {
        let moves: Vec<_> = sketch
            .curves
            .iter()
            .filter_map(|c| match c.geometry {
                SketchGeometry::Line { start, .. }
                    if start.x == 60.0 && (both_vertices || start.y == 0.0) =>
                {
                    Some(SketchVertex {
                        curve_id: c.id,
                        start_mm: [10.0, start.y],
                    })
                }
                _ => None,
            })
            .collect();
        let mut changes =
            move_sketch_vertices(chain.sketch, &sketch, &moves).expect("endpoint moves");
        assert_eq!(
            changes.len(),
            if both_vertices { 3 } else { 2 },
            "each changed Line occurs once"
        );
        if radius != 8.0 {
            changes.push(
                EditChange::constant(chain.first, ExpressionSlot::FilletRadius, radius)
                    .expect("radius"),
            );
        }
        let request = EditRequest {
            source: source.clone(),
            expected: reading.version,
            changes: changes.clone(),
            destination: root
                .path()
                .join(format!("candidate-{both_vertices}-{radius}.fcad")),
        };
        let copy = edit_copy(&request, &mut kernel, &context);
        let mut session = DocumentSession::open(&source).expect("session");
        session
            .current()
            .load_scene(
                &mut kernel,
                |k, b| k.import_step(b),
                &Default::default(),
                &context,
            )
            .expect("baseline");
        let snapshot = std::fs::read(session.current().path()).expect("snapshot");
        let step = session.begin_step().edit(
            changes,
            session.current().version(),
            &mut kernel,
            |k, b| k.import_step(b),
            &context,
        );
        if let Some(reason) = refusal {
            for error in [
                copy.expect_err("copy refusal"),
                step.expect_err("session refusal"),
            ] {
                assert!(error.to_string().contains("Fillet1"), "{error}");
                assert!(error.to_string().contains("Fillet2"), "{error}");
                let report = &RebuildRefusal::from_error(&error)
                    .expect("rebuild refusal")
                    .report;
                assert!(
                    report
                        .failed
                        .iter()
                        .any(|f| f.id == chain.first && f.message.contains(reason)),
                    "{error}"
                );
                assert!(
                    report
                        .skipped
                        .iter()
                        .any(|f| f.id == chain.second && f.cause == chain.first),
                    "{error}"
                );
            }
            assert!(!request.destination.exists());
            assert_eq!(
                std::fs::read(session.current().path()).expect("snapshot"),
                snapshot
            );
        } else {
            assert!(copy.expect("copy accepted").rebuild.all_built());
            session
                .commit_step(step.expect("step accepted"))
                .expect("commit");
            let mut volumes = Vec::new();
            for path in [&request.destination, session.current().path()] {
                let reopened = Document::open_read_only(path).expect("reopen");
                let built = rebuild_cold(&reopened, &mut kernel, &context).expect("cold rebuild");
                assert!(built.all_built(), "{:?}", built.report());
                for (feature, names) in &original_names {
                    let actual = &built.topology().features[feature];
                    let found: std::collections::BTreeSet<_> = actual
                        .bindings
                        .keys()
                        .map(|name| (name.kind.to_string(), name.digest))
                        .collect();
                    assert_eq!(&found, names, "the same feature names survive the edit");
                }
                volumes.push(
                    kernel
                        .shape_stats(built.shape(chain.body).expect("body"))
                        .expect("volume")
                        .1,
                );
                built.release_all(&mut kernel);
            }
            assert!((volumes[0] - volumes[1]).abs() < 1e-7);
            let accepted = session.current().version();
            let undo = session.begin_undo().expect("undo");
            session.commit_move(undo).expect("commit undo");
            assert_eq!(session.current().version(), reading.version);
            let redo = session.begin_redo().expect("redo");
            session.commit_move(redo).expect("commit redo");
            assert_eq!(session.current().version(), accepted);
        }
        assert_eq!(std::fs::read(&source).expect("unchanged source"), before);
    }
}

#[path = "../../ferritecad-fixtures/tests/support/model_names.rs"]
mod model_names;
