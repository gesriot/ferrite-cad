// SPDX-License-Identifier: MIT
#[path = "support/counting.rs"]
mod counting;
use counting::CountingMock;
use ferritecad_document::*;
use ferritecad_jobs::*;
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_types::{ObjectId, ProfileJoint};

fn no_imports(
    _: &mut CountingMock,
    _: &[u8],
) -> ferritecad_types::Result<ferritecad_exchange::Import> {
    unreachable!("test documents have no imports")
}

#[test]
fn every_session_edit_builds_once_and_returns_its_picture() {
    let context = OperationContext::default();
    for operation in [
        "height",
        "vertices",
        "constraints",
        "circle",
        "annulus",
        "cut_parameters",
        "add_cut",
        "add_fillet",
        "fillet_radius",
        "add_chamfer",
        "chamfer_distance",
        "revolve_angle",
    ] {
        if operation == "constraints"
            && !ferritecad_fixtures::native_dependencies(&[(
                ferritecad_sketch_solver::is_available(),
                "PLANEGCS",
            )])
        {
            continue;
        }
        let root = tempfile::tempdir().expect("directory");
        let path = root.path().join("model.fcad");
        let content = match operation {
            "circle" => Some(NewDocument::CircleExtrude(
                CircleExtrusion::new([10.0, 10.0], 5.0, 10.0).expect("circle"),
            )),
            "annulus" => Some(NewDocument::AnnularExtrude(
                AnnularExtrusion::new([10.0, 10.0], 5.0, 2.0, 10.0).expect("annulus"),
            )),
            "revolve_angle" => Some(NewDocument::SketchPartialRevolve {
                profile: FullTurnRevolution::new(vec![
                    [5.0, 0.0],
                    [15.0, 0.0],
                    [15.0, 20.0],
                    [5.0, 20.0],
                ])
                .expect("profile"),
                angle: RevolveAngle::new(std::f64::consts::PI).expect("angle"),
            }),
            _ => None,
        };
        if let Some(content) = content {
            create_document_with_kernel(
                CreateDocumentRequest::new(&path, content, "keep"),
                || Ok(CountingMock::new()),
                &context,
            )
            .expect("create");
        } else {
            ferritecad_fixtures::write_plate(&path).expect("plate");
        }
        let mut document = Document::open(&path).expect("document");
        let objects = document.objects().expect("objects");
        let body = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
            .expect("body")
            .id;
        let (sketch_id, sketch) = objects
            .iter()
            .find_map(|o| match &o.payload {
                ObjectPayload::Sketch(s) => Some((o.id, s.clone())),
                _ => None,
            })
            .expect("sketch");
        let feature = objects
            .iter()
            .find(|o| {
                matches!(
                    o.payload,
                    ObjectPayload::Extrude(_) | ObjectPayload::Revolve(_)
                )
            })
            .expect("feature")
            .id;
        let edge = || SweptEdge {
            feature,
            joint: ProfileJoint::new(sketch.curves[3].id, sketch.curves[0].id).expect("joint"),
        };
        let mut selected = ObjectId::new();
        if operation == "fillet_radius" {
            let prepared = prepare_edge_fillet(
                &document,
                body,
                &EdgeFillet {
                    edge: edge(),
                    radius_mm: 2.0,
                },
            )
            .expect("fillet");
            selected = prepared.feature().id;
            document.write_edge_fillet(&prepared).expect("write fillet");
        } else if operation == "chamfer_distance" {
            let prepared = prepare_edge_chamfer(
                &document,
                body,
                &EdgeChamfer {
                    edge: edge(),
                    distance_mm: 2.0,
                },
            )
            .expect("chamfer");
            selected = prepared.feature().id;
            document
                .write_edge_chamfer(&prepared)
                .expect("write chamfer");
        } else if operation == "cut_parameters" {
            let prepared = prepare_circular_cut(
                &document,
                body,
                &CircularCut {
                    center_mm: [20.0, 20.0],
                    radius_mm: 2.0,
                    extent: CutExtent::ThroughAll,
                },
            )
            .expect("cut");
            selected = prepared.feature().id;
            document.write_circular_cut(&prepared).expect("write cut");
        }
        document.close().expect("close");
        let mut session = DocumentSession::open(&path).expect("session");
        let mut kernel = CountingMock::new();
        session
            .current()
            .load_scene(
                &mut kernel,
                no_imports,
                &TessellationParams::default(),
                &context,
            )
            .expect("Open picture and baseline");
        let extrusions = kernel.extrude_count;
        let revolutions = kernel.revolve_count;
        let expected = session.current().version();
        let ticket = session.begin_step();
        let result = match operation {
            "height" => {
                let ticket = ticket;
                let expected = ticket.source().version();
                ticket.edit(
                    vec![
                        ferritecad_document::EditChange::constant(
                            feature,
                            ferritecad_document::ExpressionSlot::ExtrudeDistance,
                            12.0,
                        )
                        .expect("test value"),
                    ],
                    expected,
                    &mut kernel,
                    no_imports,
                    &context,
                )
            }
            "vertices" => {
                let vertices = sketch
                    .curves
                    .iter()
                    .map(|curve| {
                        let SketchGeometry::Line { start, .. } = curve.geometry else {
                            unreachable!("lines")
                        };
                        SketchVertex {
                            curve_id: curve.id,
                            start_mm: [start.x * 0.9, start.y],
                        }
                    })
                    .collect::<Vec<_>>();
                ticket.edit(
                    move_sketch_vertices(sketch_id, &sketch, &vertices).expect("vertex changes"),
                    expected,
                    &mut kernel,
                    no_imports,
                    &context,
                )
            }
            "constraints" => ticket.edit(
                sketch_constraint_changes(
                    sketch_id,
                    &sketch,
                    &SketchConstraintEdits {
                        remove: vec![],
                        add: vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                            curve: sketch.curves[0].id,
                            kind: LineConstraintKind::Horizontal,
                        })],
                    },
                )
                .expect("constraint changes"),
                expected,
                &mut kernel,
                no_imports,
                &context,
            ),
            "circle" | "annulus" => ticket.edit(
                vec![EditChange::SetCurveGeometry {
                    sketch: sketch_id,
                    curve: sketch.curves[0].id,
                    geometry: SketchGeometry::Circle {
                        center: Point2::new(10., 10.).expect("center"),
                        radius: 6.,
                    },
                }],
                expected,
                &mut kernel,
                no_imports,
                &context,
            ),
            "add_cut" => ticket.add_circular_cut(
                body,
                CircularCut {
                    center_mm: [20.0, 20.0],
                    radius_mm: 2.0,
                    extent: CutExtent::ThroughAll,
                },
                expected,
                &mut kernel,
                no_imports,
                &context,
            ),
            "cut_parameters" => {
                let d = Document::open_read_only(session.current().path()).expect("doc");
                let choice = cut_parameter_choices(&d, &d.objects().expect("objects"))
                    .into_iter()
                    .find(|c| c.feature == selected)
                    .expect("cut choice");
                let saved = choice.saved.expect("editable cut");
                ticket.edit_circular_cut(
                    selected,
                    CircularCutEdit {
                        tool_curve: saved.tool_curve,
                        center_mm: [20.0, 20.0],
                        radius_mm: 2.5,
                        extent: CutExtent::ThroughAll,
                        vocabulary: ExtentVocabulary::BlindOrThroughAll,
                    },
                    expected,
                    &mut kernel,
                    no_imports,
                    &context,
                )
            }
            "add_fillet" => ticket.add_edge_fillet(
                body,
                EdgeFillet {
                    edge: edge(),
                    radius_mm: 2.0,
                },
                expected,
                &mut kernel,
                no_imports,
                &context,
            ),
            "fillet_radius" => ticket.edit(
                vec![
                    ferritecad_document::EditChange::constant(
                        selected,
                        ferritecad_document::ExpressionSlot::FilletRadius,
                        2.5,
                    )
                    .expect("test value"),
                ],
                expected,
                &mut kernel,
                no_imports,
                &context,
            ),
            "add_chamfer" => ticket.add_edge_chamfer(
                body,
                EdgeChamfer {
                    edge: edge(),
                    distance_mm: 2.0,
                },
                expected,
                &mut kernel,
                no_imports,
                &context,
            ),
            "chamfer_distance" => ticket.edit(
                vec![
                    ferritecad_document::EditChange::constant(
                        selected,
                        ferritecad_document::ExpressionSlot::ChamferDistance,
                        2.5,
                    )
                    .expect("test value"),
                ],
                expected,
                &mut kernel,
                no_imports,
                &context,
            ),
            "revolve_angle" => ticket.edit(
                vec![
                    ferritecad_document::EditChange::constant(
                        feature,
                        ferritecad_document::ExpressionSlot::RevolveAngle,
                        200.0_f64.to_radians(),
                    )
                    .expect("test value"),
                ],
                expected,
                &mut kernel,
                no_imports,
                &context,
            ),
            _ => unreachable!("listed operation"),
        };
        let mut step = result.expect(operation);
        let expected_extrusions = match operation {
            "add_cut" | "cut_parameters" => 2,
            "revolve_angle" => 0,
            _ => 1,
        };
        assert_eq!(
            kernel.extrude_count - extrusions,
            expected_extrusions,
            "{operation}"
        );
        assert_eq!(
            kernel.revolve_count - revolutions,
            usize::from(operation == "revolve_angle"),
            "{operation}"
        );
        let scene = step.take_scene().expect("step carries its picture");
        assert_eq!(scene.catalogue.len(), 1, "{operation}");
        assert!(scene.rebuild.unbuilt().is_empty(), "{operation}");
        assert!(
            kernel.live.is_empty(),
            "{operation}: shapes released after tessellation"
        );
        session.commit_step(step).expect("commit");
    }
}

#[test]
fn a_picture_failure_releases_the_rebuild_and_keeps_the_accepted_version() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let mut session = DocumentSession::open(&path).expect("session");
    let mut kernel = CountingMock::new();
    let context = OperationContext::default();
    session
        .current()
        .load_scene(
            &mut kernel,
            no_imports,
            &TessellationParams::default(),
            &context,
        )
        .expect("Open");
    let original = session.current().version();
    let feature = read_extrude_source(session.current().path())
        .expect("source")
        .expression_slots
        .iter()
        .find(|s| s.slot == ferritecad_document::ExpressionSlot::ExtrudeDistance)
        .expect("height slot")
        .object;
    kernel.refuse_mesh = true;
    let ticket = session.begin_step();
    let destination = ticket.destination().to_path_buf();
    let error = {
        let ticket = ticket;
        let expected = ticket.source().version();
        ticket.edit(
            vec![
                ferritecad_document::EditChange::constant(
                    feature,
                    ferritecad_document::ExpressionSlot::ExtrudeDistance,
                    12.0,
                )
                .expect("test value"),
            ],
            expected,
            &mut kernel,
            no_imports,
            &context,
        )
    }
    .expect_err("mesh refused");
    assert!(error.to_string().contains("requested mesh failure"));
    assert!(!destination.exists());
    assert_eq!(session.current().version(), original);
    assert!(!session.can_undo());
    assert!(kernel.live.is_empty());
}
