// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_fixtures::datum_planes::{boss, sweep_face};
use ferritecad_jobs::{DocumentSession, EditRequest, edit_copy};
use ferritecad_kernel::{NamingInput, OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_types::ObjectId;

#[test]
fn plane_addition_face_change_offset_and_undo_redo_share_atomic_transactions() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let source = Document::open_read_only(&path).expect("source");
    let mut kernel = OcctKernel::new().expect("kernel");
    let f = boss(&source, &mut kernel).expect("fixture");
    let built = ferritecad_eval::rebuild_cold(&source, &mut kernel, &OperationContext::default())
        .expect("base");
    let bottom =
        sweep_face(&built, &mut kernel, f.base, NamingInput::Cap { end: false }).expect("bottom");
    built.release_all(&mut kernel);
    drop(source);
    let original = std::fs::read(&path).expect("bytes");
    let mut session = DocumentSession::open(&path).expect("session");
    session
        .current()
        .load_scene(
            &mut kernel,
            |_, _| unreachable!("no imports"),
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("baseline");
    let expected = session.current().version();
    let step = session
        .begin_step()
        .edit(
            f.changes,
            expected,
            &mut kernel,
            |_, _| unreachable!("no imports"),
            &OperationContext::default(),
        )
        .expect("one addition");
    session.commit_step(step).expect("commit");
    let saved = Document::open_read_only(session.current().path()).expect("snapshot");
    let original_content = saved.content_version().expect("content");
    let ObjectPayload::Sketch(sketch) = saved
        .object(f.sketch)
        .expect("object")
        .expect("sketch")
        .payload
    else {
        panic!("sketch");
    };
    drop(saved);
    let changes = vec![
        EditChange::SetDatumFace {
            object: f.plane,
            face: bottom,
        },
        EditChange::SetCurveGeometry {
            sketch: f.sketch,
            curve: sketch.curves[0].id,
            geometry: SketchGeometry::Circle {
                center: Point2::new(25., -20.).expect("point"),
                radius: 3.,
            },
        },
        EditChange::constant(f.plane, ExpressionSlot::DatumOffset, -1.).expect("offset"),
    ];
    let step = session
        .begin_step()
        .edit(
            changes,
            session.current().version(),
            &mut kernel,
            |_, _| unreachable!("no imports"),
            &OperationContext::default(),
        )
        .expect("face and offset");
    session.commit_step(step).expect("commit");
    let saved = Document::open_read_only(session.current().path()).expect("changed snapshot");
    let changed_content = saved.content_version().expect("changed hash");
    let built = ferritecad_eval::rebuild_cold(&saved, &mut kernel, &OperationContext::default())
        .expect("rebuild");
    assert!(built.all_built());
    let plane = built.plane(f.plane).expect("computed");
    assert_eq!(plane.normal().z, -1.);
    assert_eq!(plane.origin().z, 1.);
    built.release_all(&mut kernel);
    drop(saved);
    session
        .commit_move(session.begin_undo().expect("undo"))
        .expect("undo commit");
    assert_eq!(
        Document::open_read_only(session.current().path())
            .expect("undo document")
            .content_version()
            .expect("hash"),
        original_content
    );
    session
        .commit_move(session.begin_redo().expect("redo"))
        .expect("redo commit");
    assert_eq!(
        Document::open_read_only(session.current().path())
            .expect("redo document")
            .content_version()
            .expect("hash"),
        changed_content
    );
    let source = session.current().path().to_owned();
    for (i, changes) in [
        vec![EditChange::constant(f.plane, ExpressionSlot::DatumOffset, 5.).expect("detached")],
        vec![EditChange::SetDatumFace {
            object: f.plane,
            face: EntityReference {
                meaning: ReferenceMeaning {
                    kind: EntityKind::Edge,
                    ..bottom.meaning
                },
                ..bottom
            },
        }],
    ]
    .into_iter()
    .enumerate()
    {
        let destination = root.path().join(format!("refused-{i}.fcad"));
        let request = EditRequest {
            source: source.clone(),
            destination: destination.clone(),
            expected: session.current().version(),
            changes,
        };
        let before = std::fs::read(&source).expect("source bytes");
        let error = edit_copy(&request, &mut kernel, &OperationContext::default())
            .expect_err("no invalid publication");
        if i == 0 {
            assert!(error.to_string().contains("unbuilt"), "{error}");
        } else {
            let cause = std::error::Error::source(&error).expect("input cause");
            assert!(cause.to_string().contains("face"), "{cause}");
        }
        assert_eq!(std::fs::read(&source).expect("source unchanged"), before);
        assert!(!destination.exists());
    }
    assert_eq!(std::fs::read(path).expect("saved original"), original);
}

#[test]
fn deleted_support_and_self_dependent_plane_refuse_without_publishing() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_fixtures::{datum_planes::circle, feature_changes::*};
    use ferritecad_kernel::{NamingOperation, SubShapeKind};
    use ferritecad_topology::generative::{cap_root, generated};
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source).expect("plate");
    let mut doc = Document::open(&source).expect("document");
    let body = body(&doc).expect("body");
    let (_, _, base) = base_profile(&doc, body).expect("base");
    let cut = circular_tool(
        &doc,
        body,
        [20., 20.],
        3.,
        EndCondition::Blind {
            distance: Expression::constant(4.).expect("distance"),
        },
    )
    .expect("pocket");
    doc.write_changes(&cut.changes).expect("pocket");
    let floor = generated(
        cut.feature,
        NamingOperation::Extrude,
        cap_root(cut.feature, true, Some(base.plane)),
        SubShapeKind::Face,
    );
    let face = EntityReference::new(ReferenceMeaning {
        producer: cut.feature,
        kind: EntityKind::Face,
        digest: floor.digest,
        selection: Selection::One,
    });
    let (plane, sketch, feature) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    let addition = |result| {
        vec![
            EditChange::AddDatumPlane {
                object: plane,
                name: None,
                plane: DatumPlane::Face {
                    result,
                    face,
                    offset_mm: Expression::constant(0.).expect("offset"),
                },
            },
            EditChange::AddSketch {
                object: sketch,
                name: None,
                sketch: circle(plane, [20., 20.], 1.).expect("circle"),
            },
            EditChange::AddBody {
                object: ObjectId::new(),
                name: None,
                feature_object: feature,
                feature_name: None,
                feature: FeatureAddition::Extrude {
                    profile: sketch,
                    regions: vec![],
                    operation: SolidOperation::NewBody,
                    reversed: false,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(1.).expect("height"),
                    },
                },
            },
        ]
    };
    let expected = DocumentVersion {
        document_id: doc.meta().document_id,
        content: doc.content_version().expect("version"),
    };
    doc.close().expect("close");
    let mut kernel = OcctKernel::new().expect("kernel");
    let before = std::fs::read(&source).expect("source bytes");
    let refused = root.path().join("cycle.fcad");
    let error = edit_copy(
        &EditRequest {
            source: source.clone(),
            destination: refused.clone(),
            expected,
            changes: addition(feature),
        },
        &mut kernel,
        &OperationContext::default(),
    )
    .expect_err("support -> feature -> sketch -> support cycle");
    assert!(error.to_string().contains("cycle"), "{error}");
    assert_eq!(std::fs::read(&source).expect("source unchanged"), before);
    assert!(!refused.exists());
    let supported = root.path().join("supported.fcad");
    edit_copy(
        &EditRequest {
            source,
            destination: supported.clone(),
            expected,
            changes: addition(cut.feature),
        },
        &mut kernel,
        &OperationContext::default(),
    )
    .expect("floor support");
    let saved = Document::open_read_only(&supported).expect("saved");
    let expected = DocumentVersion {
        document_id: saved.meta().document_id,
        content: saved.content_version().expect("version"),
    };
    drop(saved);
    let before = std::fs::read(&supported).expect("supported bytes");
    let refused = root.path().join("deleted.fcad");
    let error = edit_copy(
        &EditRequest {
            source: supported.clone(),
            destination: refused.clone(),
            expected,
            changes: vec![EditChange::SetEndCondition {
                object: cut.feature,
                end_condition: EndCondition::ThroughAll,
            }],
        },
        &mut kernel,
        &OperationContext::default(),
    )
    .expect_err("deleted support");
    let text = error.to_string();
    assert!(text.contains("face deleted"), "{text}");
    assert!(text.contains(&face.id.to_string()), "{text}");
    assert!(text.contains(&plane.to_string()), "{text}");
    assert_eq!(std::fs::read(supported).expect("source unchanged"), before);
    assert!(!refused.exists());
}
