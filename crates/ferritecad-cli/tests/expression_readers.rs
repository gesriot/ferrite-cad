// SPDX-License-Identifier: MIT
use ferritecad_document::{
    Document, ExpressionChange, ExpressionSlot, ExtrudeEditSource, ObjectPayload,
};
use ferritecad_kernel::OperationContext;
use std::{collections::BTreeMap, path::Path};
#[path = "support/transaction.rs"]
mod transaction;

fn readers(path: &Path) -> BTreeMap<String, bool> {
    let document = Document::open_read_only(path).expect("document");
    let source = ExtrudeEditSource::read(&document).expect("reading");
    let mut result = BTreeMap::new();
    for row in source.sketches {
        result.insert(format!("sketch {}", row.sketch), row.vertices.is_some());
        result.insert(
            format!("sketch Fillets {}", row.sketch),
            !row.fillets.is_empty(),
        );
        result.insert(
            format!("sketch Chamfer {}", row.sketch),
            row.chamfer.is_some(),
        );
    }
    for row in source.constraint_sketches {
        result.insert(format!("constraints {}", row.sketch), row.stored.is_some());
        result.insert(
            format!("constraint Fillets {}", row.sketch),
            !row.fillets.is_empty(),
        );
        result.insert(
            format!("constraint Chamfer {}", row.sketch),
            row.chamfer.is_some(),
        );
    }
    for row in source.cut_features {
        result.insert(format!("Cut {}", row.feature), row.saved.is_some());
    }
    for row in source.fillet_bodies {
        result.insert(format!("Fillet {}", row.body), row.target.is_some());
    }
    for row in source.chamfer_bodies {
        result.insert(format!("Chamfer {}", row.body), row.target.is_some());
    }
    result
}

#[test]
fn native_every_numeric_slot_preserves_structural_readers_and_edge_names() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    for kind in [
        "plate",
        "cut",
        "fillet",
        "chamfer",
        "constrained-fillet",
        "constrained-chamfer",
    ] {
        if kind.starts_with("constrained-")
            && !ferritecad_fixtures::native_dependencies(&[(
                ferritecad_sketch_solver::is_available(),
                "PLANEGCS",
            )])
        {
            continue;
        }
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("source.fcad");
        if kind == "constrained-fillet" {
            ferritecad_fixtures::write_constrained_fillet_chain(&source, 2., 60., 40.)
                .expect("fixture");
        } else {
            ferritecad_fixtures::write_plate(&source).expect("plate");
            let mut document = Document::open(&source).expect("document");
            let body = document
                .objects()
                .expect("objects")
                .iter()
                .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
                .expect("body")
                .id;
            let reading = ExtrudeEditSource::read(&document).expect("reading");
            match kind {
                "fillet" => {
                    let corner = &reading.fillet_bodies[0]
                        .target
                        .as_ref()
                        .expect("fillet target")
                        .corners[0];
                    let edge = ferritecad_document::SweptEdge {
                        feature: corner.feature,
                        joint: corner.joint,
                    };
                    let prepared = ferritecad_document::prepare_edge_fillet(
                        &document,
                        body,
                        &ferritecad_document::EdgeFillet {
                            edge,
                            radius_mm: 2.,
                        },
                    )
                    .expect("prepare");
                    document.write_edge_fillet(&prepared).expect("fillet");
                }
                "chamfer" | "constrained-chamfer" => {
                    let corner = &reading.chamfer_bodies[0]
                        .target
                        .as_ref()
                        .expect("chamfer target")
                        .corners[0];
                    let edge = ferritecad_document::SweptEdge {
                        feature: corner.feature,
                        joint: corner.joint,
                    };
                    let prepared = ferritecad_document::prepare_edge_chamfer(
                        &document,
                        body,
                        &ferritecad_document::EdgeChamfer {
                            edge,
                            distance_mm: 2.,
                        },
                    )
                    .expect("prepare");
                    document.write_edge_chamfer(&prepared).expect("chamfer");
                }
                "cut" => {
                    let prepared = ferritecad_document::prepare_circular_cut(
                        &document,
                        body,
                        &ferritecad_document::CircularCut {
                            center_mm: [15., 15.],
                            radius_mm: 2.,
                            extent: ferritecad_document::CutExtent::Blind { depth_mm: 2. },
                        },
                    )
                    .expect("prepare");
                    document.write_circular_cut(&prepared).expect("Cut");
                }
                _ => {}
            }
            if kind == "constrained-chamfer" {
                use ferritecad_document::{
                    AddLineConstraint, AddSketchConstraint, LineConstraintKind as Kind,
                    LineEndpoint, LineLengthMm, SketchConstraintEdits, SketchCoordinateMm,
                };
                let sketch = &reading.sketches[0];
                let vertices = sketch.vertices.as_ref().expect("rectangle");
                let add = [
                    (0, Kind::Horizontal),
                    (1, Kind::Vertical),
                    (2, Kind::Horizontal),
                    (3, Kind::Vertical),
                    (0, Kind::Distance(LineLengthMm::new(60.).expect("width"))),
                    (1, Kind::Distance(LineLengthMm::new(40.).expect("depth"))),
                    (
                        0,
                        Kind::Fixed {
                            at: LineEndpoint::Start,
                            x: SketchCoordinateMm::new(0.).expect("x"),
                            y: SketchCoordinateMm::new(0.).expect("y"),
                        },
                    ),
                ]
                .into_iter()
                .map(|(i, kind)| {
                    AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: vertices[i].curve_id,
                        kind,
                    })
                })
                .collect();
                let prepared = ferritecad_document::prepare_sketch_constraints(
                    &document,
                    sketch.sketch,
                    &SketchConstraintEdits {
                        remove: vec![],
                        add,
                    },
                )
                .expect("constraints");
                document
                    .write_sketch_constraints(&prepared)
                    .expect("dimensioned Chamfer");
            }
        }
        let document = Document::open_read_only(&source).expect("saved");
        let reading = ExtrudeEditSource::read(&document).expect("reading");
        let expected_readers = readers(&source);
        assert!(expected_readers.values().any(|v| *v));
        let references = document.topology_refs().expect("names");
        let bytes = std::fs::read(&source).expect("source");
        for (index, slot) in reading.expression_slots.iter().enumerate() {
            let value = match slot.slot {
                ExpressionSlot::FixedX { .. } | ExpressionSlot::FixedY { .. } => {
                    slot.expression.value() + 0.1
                }
                _ => slot.expression.value() * 0.95,
            };
            let change = ExpressionChange::constant(slot.object, slot.slot, value).expect("change");
            let request = root.path().join("request.json");
            std::fs::write(&request, serde_json::to_vec(&serde_json::json!({"request_version":1,"changes":[{"object":slot.object,"slot":slot.slot,"value":value,"unit":"mm"}]})).expect("JSON")).expect("request");
            let destination = root.path().join(format!("edited-{index}.fcad"));
            let output = transaction::command(
                &source,
                &reading.version.content.to_string(),
                &request,
                &destination,
            )
            .output()
            .expect("CLI");
            assert!(output.status.success(), "{kind} {}: {output:?}", slot.slot);
            assert_eq!(
                readers(&destination),
                expected_readers,
                "{kind} {}",
                slot.slot
            );
            let edited = Document::open_read_only(&destination).expect("reopen");
            assert_eq!(
                edited.topology_refs().expect("names"),
                references,
                "numbers do not alter edge-operation naming contracts"
            );
            let context = OperationContext::default();
            let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
            let mut session = ferritecad_jobs::DocumentSession::open(&source).expect("session");
            session
                .current()
                .load_scene(
                    &mut kernel,
                    |k, b| k.import_step(b),
                    &Default::default(),
                    &context,
                )
                .expect("baseline");
            let step = session
                .begin_step()
                .edit(
                    vec![change],
                    reading.version,
                    &mut kernel,
                    |k, b| k.import_step(b),
                    &context,
                )
                .expect("session transaction");
            session.commit_step(step).expect("commit");
            assert_eq!(readers(session.current().path()), expected_readers);
            let accepted = Document::open_read_only(session.current().path()).expect("accepted");
            assert_eq!(accepted.topology_refs().expect("names"), references);
        }
        assert_eq!(std::fs::read(&source).expect("source"), bytes);
    }
}
