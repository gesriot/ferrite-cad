// SPDX-License-Identifier: MIT
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, Document, DocumentVersion, LineConstraintKind,
    LineEndpoint, LineLengthMm, ObjectPayload, SketchConstraintEdits, SketchCoordinateMm,
};
use ferritecad_eval::{RebuildRefusal, rebuild_cold};
use ferritecad_jobs::{
    BodySelection, DocumentSession, Existing, HistoryLimits, StlExportRequest,
    export_document_as_stl,
};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;

fn ready(solver: bool) -> bool {
    ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (
            !solver || ferritecad_sketch_solver::is_available(),
            "PLANEGCS",
        ),
    ])
}
fn version(path: &std::path::Path) -> DocumentVersion {
    let document = Document::open_read_only(path).expect("document");
    DocumentVersion {
        document_id: document.meta().document_id,
        content: document.content_version().expect("version"),
    }
}
#[test]
fn a_broken_history_opens_exports_partially_and_can_be_repaired_and_undone() {
    if !ready(true) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("broken.fcad");
    let chain = ferritecad_fixtures::write_constrained_fillet_chain(&path, 25.0, 20.0, 20.0)
        .expect("direct writer fixture");
    let context = OperationContext::default();
    let params = TessellationParams::new(0.1, 0.3, false).expect("params");
    let mut kernel = OcctKernel::new().expect("kernel");
    let loaded = ferritecad_scene::snapshot_of(
        &path,
        &mut kernel,
        |_, _| unreachable!("no imports"),
        &params,
        &context,
    )
    .expect("repairable scene");
    assert_eq!(loaded.rebuild.failed[0].id, chain.first);
    assert_eq!(
        loaded
            .rebuild
            .skipped
            .iter()
            .find(|s| s.id == chain.second)
            .expect("dependent")
            .cause,
        chain.first
    );
    assert_eq!(
        loaded.rebuild.degraded_bodies[0].rolled_back_to,
        Some(chain.extrude)
    );
    assert_eq!(loaded.snapshot.meshes().len(), 1);
    assert_eq!(
        loaded
            .edit_source
            .as_ref()
            .expect("catalogue")
            .model_tree
            .unbuilt_features,
        2
    );
    let destination = root.path().join("broken.stl");
    let exported = export_document_as_stl(
        StlExportRequest {
            document: &path,
            destination: &destination,
            body: BodySelection::Id(chain.body),
            params,
            existing: Existing::Keep { advice: "keep" },
        },
        OcctKernel::new,
        &context,
    )
    .expect("partial export");
    assert!(!exported.complete);
    assert_eq!(exported.omissions[0].id, chain.body);
    assert!(
        !destination.exists(),
        "no fallback geometry may be published"
    );
    let scene = ferritecad_scene::export_scene(
        &path,
        &mut kernel,
        |_, _| unreachable!("no imports"),
        &params,
        &context,
    )
    .expect("partial FBX scene");
    assert!(!scene.completeness().is_complete());
    assert_eq!(scene.completeness().omissions().len(), 1);
    let sessions = root.path().join("sessions");
    std::fs::create_dir(&sessions).expect("sessions");
    let mut session =
        DocumentSession::open_in(&sessions, &path, HistoryLimits::default()).expect("session");
    let expected = session.current().version();
    let step = session
        .begin_step()
        .edit_fillet_radius(chain.first, 2.0, expected, &mut kernel, &context)
        .expect("repair accepted");
    session.commit_step(step).expect("commit repair");
    let document = Document::open_read_only(session.current().path()).expect("repaired");
    let built = rebuild_cold(&document, &mut kernel, &context).expect("rebuild");
    assert!(built.all_built(), "{:?}", built.report());
    built.release_all(&mut kernel);
    let undo = session.begin_undo().expect("undo");
    session.commit_move(undo).expect("undo committed");
    let document = Document::open_read_only(session.current().path()).expect("undo snapshot");
    let built = rebuild_cold(&document, &mut kernel, &context).expect("repairable again");
    assert_eq!(built.report(), loaded.rebuild);
    built.release_all(&mut kernel);
}
#[test]
fn shrinking_a_constrained_plate_lists_both_fillets_and_publishes_nothing() {
    if !ready(true) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("healthy.fcad");
    let chain = ferritecad_fixtures::write_fillet_chain(&path, 3.0).expect("fixture");
    let document = Document::open_read_only(&path).expect("document");
    let objects = document.objects().expect("objects");
    let ObjectPayload::Sketch(sketch) = &objects
        .iter()
        .find(|o| o.id == chain.sketch)
        .expect("sketch")
        .payload
    else {
        unreachable!()
    };
    let line = |i: usize, kind| {
        AddSketchConstraint::Line(AddLineConstraint::Line {
            curve: sketch.curves[i].id,
            kind,
        })
    };
    let edits = SketchConstraintEdits {
        remove: vec![],
        add: vec![
            line(0, LineConstraintKind::Horizontal),
            line(1, LineConstraintKind::Vertical),
            line(2, LineConstraintKind::Horizontal),
            line(3, LineConstraintKind::Vertical),
            line(
                0,
                LineConstraintKind::Fixed {
                    at: LineEndpoint::Start,
                    x: SketchCoordinateMm::new(0.0).expect("x"),
                    y: SketchCoordinateMm::new(0.0).expect("y"),
                },
            ),
            line(
                0,
                LineConstraintKind::Distance(LineLengthMm::new(4.0).expect("width")),
            ),
            line(
                1,
                LineConstraintKind::Distance(LineLengthMm::new(4.0).expect("height")),
            ),
        ],
    };
    let destination = root.path().join("refused.fcad");
    let before = std::fs::read(&path).expect("bytes");
    let error = ferritecad_jobs::edit_sketch_constraints_copy(
        &ferritecad_jobs::EditSketchConstraintsRequest {
            source: path.clone(),
            expected: version(&path),
            sketch: chain.sketch,
            edits,
            destination: destination.clone(),
        },
        &mut OcctKernel::new().expect("kernel"),
        &OperationContext::default(),
    )
    .expect_err("refuse both dependents");
    let refusal = RebuildRefusal::from_error(&error);
    assert!(refusal.is_some(), "expected structured refusal: {error:?}");
    let refusal = refusal.expect("structured refusal");
    assert!(refusal.report.failed.iter().any(|f| f.id == chain.first));
    assert!(
        refusal
            .report
            .skipped
            .iter()
            .any(|s| s.id == chain.second && s.cause == chain.first)
    );
    assert!(error.to_string().contains(&chain.first.to_string()));
    assert!(error.to_string().contains(&chain.second.to_string()));
    assert!(!destination.exists());
    assert_eq!(std::fs::read(&path).expect("source"), before);
}
