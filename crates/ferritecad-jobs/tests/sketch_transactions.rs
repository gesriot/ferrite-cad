// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_jobs::{EditRequest, edit_copy};
use ferritecad_kernel::OperationContext;
use ferritecad_types::StableEntityId;

fn stored_sketch(document: &Document) -> (ferritecad_types::ObjectId, Sketch) {
    document
        .objects()
        .expect("objects")
        .into_iter()
        .find_map(|o| match o.payload {
            ObjectPayload::Sketch(s) => Some((o.id, s)),
            _ => None,
        })
        .expect("sketch")
}

#[test]
fn ordered_geometry_and_constraints_validate_the_final_payload_atomically() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let mut document = Document::open(&path).expect("document");
    let (id, sketch) = stored_sketch(&document);
    let curve = sketch.curves[0].id;
    let original = sketch.curves[0].geometry.clone();
    let point = SketchPointRef::new(curve, SketchPointSelector::Start);
    let constraint = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Fixed {
            point,
            x: Expression::constant(0.0).expect("x"),
            y: Expression::constant(0.0).expect("y"),
        },
    };
    let invalid_geometry = SketchGeometry::Line {
        start: Point2 {
            x: f64::NAN,
            y: 0.0,
        },
        end: Point2::new(0.0, 0.0).expect("point"),
    };
    document
        .write_changes(&[
            EditChange::SetCurveGeometry {
                sketch: id,
                curve,
                geometry: invalid_geometry.clone(),
            },
            EditChange::SetCurveGeometry {
                sketch: id,
                curve,
                geometry: original.clone(),
            },
            EditChange::AddConstraint {
                sketch: id,
                constraint: constraint.clone(),
            },
            EditChange::constant(
                id,
                ExpressionSlot::FixedX {
                    constraint: constraint.id,
                },
                2.0,
            )
            .expect("expression"),
            EditChange::RemoveConstraint {
                sketch: id,
                constraint: constraint.id,
            },
        ])
        .expect("last geometry wins and removed constraints leave no payload");
    assert_eq!(stored_sketch(&document).1, sketch);
    let before = document.content_version().expect("version");
    for changes in [
        vec![
            EditChange::AddConstraint {
                sketch: id,
                constraint: constraint.clone(),
            },
            EditChange::SetCurveGeometry {
                sketch: id,
                curve,
                geometry: invalid_geometry,
            },
        ],
        vec![EditChange::SetCurveGeometry {
            sketch: id,
            curve,
            geometry: SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0).expect("center"),
                radius: 2.0,
            },
        }],
        vec![
            EditChange::RemoveConstraint {
                sketch: id,
                constraint: constraint.id,
            },
            EditChange::AddConstraint {
                sketch: id,
                constraint: constraint.clone(),
            },
        ],
    ] {
        assert!(document.write_changes(&changes).is_err());
        assert_eq!(document.content_version().expect("version"), before);
    }
}

#[test]
fn a_construction_line_at_the_vertex_does_not_join_the_moved_boundary() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let document = Document::open_read_only(&path).expect("document");
    let (id, mut sketch) = stored_sketch(&document);
    let axis = construction_axis(&sketch);
    sketch.curves.push(axis.clone());
    let changes = move_sketch_vertices(
        id,
        &sketch,
        &[SketchVertex {
            curve_id: sketch.curves[0].id,
            start_mm: [-2., 3.],
        }],
    )
    .expect("ignore the construction line ending at this vertex");
    assert_eq!(changes.len(), 2);
    for change in &changes {
        let EditChange::SetCurveGeometry {
            curve, geometry, ..
        } = change
        else {
            panic!("geometry")
        };
        assert_ne!(*curve, axis.id);
        let SketchGeometry::Line { start, end } = geometry else {
            panic!("line")
        };
        if *curve == sketch.curves[0].id {
            assert_eq!(*start, Point2 { x: -2., y: 3. });
        } else {
            assert_eq!(*curve, sketch.curves[3].id);
            assert_eq!(*end, Point2 { x: -2., y: 3. });
        }
    }
    assert_eq!(sketch.curves[4], axis);
}

fn construction_axis(sketch: &Sketch) -> SketchCurve {
    let SketchGeometry::Line { start, .. } = sketch.curves[0].geometry else {
        panic!("line")
    };
    SketchCurve {
        id: StableEntityId::new(),
        construction: true,
        geometry: SketchGeometry::Line {
            start: Point2 {
                x: start.x,
                y: start.y - 10.,
            },
            end: start,
        },
    }
}

#[test]
fn a_cut_base_moves_and_remains_editable_through_the_transaction() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source).expect("plate");
    let mut document = Document::open(&source).expect("document");
    let (id, sketch) = stored_sketch(&document);
    let body = document
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("body")
        .id;
    let cut = ferritecad_fixtures::feature_changes::circular_tool(
        &document,
        body,
        [20., 20.],
        2.,
        EndCondition::ThroughAll,
    )
    .expect("tool");
    document.write_changes(&cut.changes).expect("write cut");
    let refs = model_names::names_with(
        &document,
        &mut ferritecad_occt::OcctKernel::new().expect("baseline kernel"),
    );
    let moves = sketch
        .curves
        .iter()
        .map(|c| {
            let SketchGeometry::Line { start, .. } = c.geometry else {
                panic!("plate lines")
            };
            SketchVertex {
                curve_id: c.id,
                start_mm: [start.x - 5.0, start.y - 5.0],
            }
        })
        .collect::<Vec<_>>();
    let request = EditRequest {
        source: source.clone(),
        expected: ExtrudeEditSource::read(&document).expect("reading").version,
        changes: move_sketch_vertices(id, &sketch, &moves).expect("move all vertices"),
        destination: root.path().join("moved.fcad"),
    };
    document.close().expect("close");
    let before = std::fs::read(&source).expect("bytes");
    let context = OperationContext::default();
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    edit_copy(&request, &mut kernel, &context).expect("move cut base");
    let reopened = Document::open_read_only(&request.destination).expect("reopen");
    let built = ferritecad_eval::rebuild_cold(&reopened, &mut kernel, &context).expect("rebuild");
    assert!(built.all_built());
    for (feature, names) in refs {
        let found: std::collections::BTreeSet<_> = built.topology().features[&feature]
            .bindings
            .keys()
            .map(|n| (n.kind.to_string(), n.digest))
            .collect();
        assert_eq!(found, names, "same derivations after moving the base");
    }
    built.release_all(&mut kernel);
    let source = ExtrudeEditSource::read(&reopened).expect("reading");
    edit_copy(
        &EditRequest {
            source: request.destination,
            expected: source.version,
            changes: vec![EditChange::SetCurveGeometry {
                sketch: cut.sketch,
                curve: cut.curve,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(21., 20.).expect("point"),
                    radius: 2.,
                },
            }],
            destination: root.path().join("following.fcad"),
        },
        &mut kernel,
        &context,
    )
    .expect("following transaction");
    assert_eq!(
        std::fs::read(&request.source).expect("unchanged source"),
        before
    );
}

#[test]
fn circle_geometry_and_constraint_changes_share_rebuild_stale_and_cancel_gates() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("circle.fcad");
    let context = OperationContext::default();
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            &source,
            ferritecad_jobs::NewDocument::CircleExtrude(
                CircleExtrusion::new([0.0, 0.0], 5.0, 10.0).expect("circle"),
            ),
            "keep",
        ),
        ferritecad_occt::OcctKernel::new,
        &context,
    )
    .expect("create");
    let document = Document::open_read_only(&source).expect("document");
    let (id, sketch) = stored_sketch(&document);
    let curve = sketch.curves[0].id;
    let radius = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Radius {
            curve,
            radius: Expression::constant(8.0).expect("radius"),
        },
    };
    let fixed = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Fixed {
            point: SketchPointRef::new(curve, SketchPointSelector::Center),
            x: Expression::constant(0.0).expect("x"),
            y: Expression::constant(0.0).expect("y"),
        },
    };
    let request = EditRequest {
        source: source.clone(),
        expected: ExtrudeEditSource::read(&document).expect("version").version,
        changes: vec![EditChange::AddConstraint {
            sketch: id,
            constraint: radius.clone(),
        }],
        destination: root.path().join("constrained.fcad"),
    };
    edit_copy(&request, &mut kernel, &context).expect("add radius through rebuild");
    let accepted = request.destination;
    let saved = Document::open_read_only(&accepted).expect("saved");
    let expected = ExtrudeEditSource::read(&saved).expect("version").version;
    let original = std::fs::read(&accepted).expect("bytes");
    let geometry = EditChange::SetCurveGeometry {
        sketch: id,
        curve,
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0).expect("center"),
            radius: 9.0,
        },
    };
    for (i, change) in [
        geometry.clone(),
        EditChange::AddConstraint {
            sketch: id,
            constraint: fixed,
        },
        EditChange::RemoveConstraint {
            sketch: id,
            constraint: radius.id,
        },
        EditChange::constant(
            id,
            ExpressionSlot::Radius {
                constraint: radius.id,
            },
            7.0,
        )
        .expect("expression"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = EditRequest {
            source: accepted.clone(),
            expected,
            changes: vec![change],
            destination: root.path().join(format!("accepted-{i}.fcad")),
        };
        edit_copy(&request, &mut kernel, &context).expect("kind accepted");
        request.destination = root.path().join(format!("refused-{i}.fcad"));
        request.expected.content = ferritecad_types::ContentHash::of_bytes(b"stale");
        assert!(
            edit_copy(&request, &mut kernel, &context)
                .expect_err("stale")
                .to_string()
                .contains("changed")
        );
        request.expected = expected;
        let cancel = ferritecad_kernel::CancelToken::new();
        cancel.cancel();
        assert!(edit_copy(&request, &mut kernel, &context.clone().with_cancel(cancel)).is_err());
        assert!(!request.destination.exists());
        assert_eq!(std::fs::read(&accepted).expect("unchanged"), original);
    }
    let mut conflict = radius.clone();
    conflict.id = StableEntityId::new();
    conflict.rule = SketchConstraintRule::Radius {
        curve,
        radius: Expression::constant(11.0).expect("radius"),
    };
    let request = EditRequest {
        source: accepted.clone(),
        expected,
        changes: vec![EditChange::AddConstraint {
            sketch: id,
            constraint: conflict,
        }],
        destination: root.path().join("conflict.fcad"),
    };
    let error = edit_copy(&request, &mut kernel, &context).expect_err("inconsistent radii");
    let refusal = ferritecad_eval::RebuildRefusal::from_error(&error).expect("rebuild decision");
    assert!(refusal.report.failed.iter().any(|f| f.id == id), "{error}");
    assert!(!refusal.report.skipped.is_empty(), "{error}");
    assert_eq!(std::fs::read(&accepted).expect("unchanged"), original);
    let request = EditRequest {
        source: accepted,
        expected,
        changes: vec![
            geometry,
            EditChange::RemoveConstraint {
                sketch: id,
                constraint: radius.id,
            },
        ],
        destination: root.path().join("removed.fcad"),
    };
    edit_copy(&request, &mut kernel, &context).expect("remove and change geometry");
    let reopened = Document::open_read_only(&request.destination).expect("reopen");
    assert!(stored_sketch(&reopened).1.constraints.is_empty());
    let built =
        ferritecad_eval::rebuild_cold(&reopened, &mut kernel, &context).expect("cold rebuild");
    assert!(built.all_built());
    built.release_all(&mut kernel);
}

#[test]
fn solved_annulus_cannot_exchange_boundary_and_bore_in_the_evaluator() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("annulus.fcad");
    let context = OperationContext::default();
    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            &source,
            ferritecad_jobs::NewDocument::AnnularExtrude(
                AnnularExtrusion::new([0.0, 0.0], 10.0, 4.0, 10.0).expect("annulus"),
            ),
            "keep",
        ),
        ferritecad_occt::OcctKernel::new,
        &context,
    )
    .expect("create");
    let document = Document::open_read_only(&source).expect("document");
    let (id, sketch) = stored_sketch(&document);
    let expected = ExtrudeEditSource::read(&document).expect("reading").version;
    let changes: Vec<_> = sketch
        .curves
        .iter()
        .map(|curve| {
            let SketchGeometry::Circle { radius, .. } = curve.geometry else {
                panic!("circle")
            };
            EditChange::AddConstraint {
                sketch: id,
                constraint: SketchConstraint {
                    id: StableEntityId::new(),
                    rule: SketchConstraintRule::Radius {
                        curve: curve.id,
                        radius: Expression::constant(if radius == 10.0 { 2.0 } else { 8.0 })
                            .expect("radius"),
                    },
                },
            }
        })
        .collect();
    document.close().expect("close");
    let before = std::fs::read(&source).expect("bytes");
    let output = root.path().join("refused.fcad");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let error = edit_copy(
        &EditRequest {
            source: source.clone(),
            expected,
            changes: changes.clone(),
            destination: output.clone(),
        },
        &mut kernel,
        &context,
    )
    .expect_err("wall identities must not exchange");
    let refusal = ferritecad_eval::RebuildRefusal::from_error(&error).expect("rebuild refusal");
    assert!(
        refusal
            .report
            .failed
            .iter()
            .any(|f| f.id == id && f.message.contains("exchanged boundary")),
        "{error}"
    );
    assert!(!refusal.report.skipped.is_empty());
    assert!(!output.exists());
    assert_eq!(std::fs::read(&source).expect("bytes"), before);
    // A cold read has the same guard even when there is no edit operation.
    let raw = root.path().join("stored-conflict.fcad");
    std::fs::copy(&source, &raw).expect("private copy");
    let mut document = Document::open(&raw).expect("private document");
    document.write_changes(&changes).expect("valid payload");
    let built = ferritecad_eval::rebuild_cold(&document, &mut kernel, &context).expect("report");
    assert!(
        built
            .report()
            .failed
            .iter()
            .any(|f| f.id == id && f.message.contains("exchanged boundary"))
    );
    built.release_all(&mut kernel);
}

#[test]
fn moving_a_revolve_profile_across_its_axis_is_refused_by_rebuild() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("revolve.fcad");
    let context = OperationContext::default();
    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            &source,
            ferritecad_jobs::NewDocument::SketchRevolve(
                FullTurnRevolution::new(vec![[5.0, 0.0], [15.0, 0.0], [15.0, 20.0], [5.0, 20.0]])
                    .expect("profile"),
            ),
            "keep",
        ),
        ferritecad_occt::OcctKernel::new,
        &context,
    )
    .expect("create");
    let document = Document::open_read_only(&source).expect("document");
    let (id, sketch) = stored_sketch(&document);
    let feature = document
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Revolve(_)))
        .expect("revolve")
        .id;
    let expected = ExtrudeEditSource::read(&document).expect("reading").version;
    let changes = move_sketch_vertices(
        id,
        &sketch,
        &[SketchVertex {
            curve_id: sketch.curves[0].id,
            start_mm: [-2.0, 0.0],
        }],
    )
    .expect("payload permits coordinates across the axis");
    document.close().expect("close");
    let before = std::fs::read(&source).expect("bytes");
    let output = root.path().join("refused.fcad");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let error = edit_copy(
        &EditRequest {
            source: source.clone(),
            expected,
            changes,
            destination: output.clone(),
        },
        &mut kernel,
        &context,
    )
    .expect_err("axis crossing");
    let refusal = ferritecad_eval::RebuildRefusal::from_error(&error).expect("rebuild refusal");
    assert!(
        refusal
            .report
            .failed
            .iter()
            .any(|f| f.id == feature && f.message.contains("axis")),
        "{error}"
    );
    assert!(!output.exists());
    assert_eq!(std::fs::read(&source).expect("bytes"), before);
}

#[path = "../../ferritecad-fixtures/tests/support/model_names.rs"]
mod model_names;
