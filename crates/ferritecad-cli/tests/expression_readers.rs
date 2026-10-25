// SPDX-License-Identifier: MIT
#[path = "support/names.rs"]
mod names;
use ferritecad_document::{
    Document, EditChange, EndCondition, Expression, ExpressionSlot, ExtrudeEditSource,
    ObjectPayload,
};
use ferritecad_kernel::OperationContext;
#[path = "support/transaction.rs"]
mod transaction;

#[test]
fn native_every_numeric_slot_preserves_history_and_edge_names() {
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
            match kind {
                "fillet" | "chamfer" | "constrained-chamfer" => {
                    let change = ferritecad_fixtures::feature_changes::edge_addition(
                        &document,
                        body,
                        0,
                        2.,
                        kind != "fillet",
                    )
                    .expect("addition");
                    document
                        .write_changes(&[change])
                        .expect("write edge feature");
                }
                "cut" => {
                    let tool = ferritecad_fixtures::feature_changes::circular_tool(
                        &document,
                        body,
                        [15., 15.],
                        2.,
                        EndCondition::Blind {
                            distance: Expression::constant(2.).expect("depth"),
                        },
                    )
                    .expect("tool");
                    document.write_changes(&tool.changes).expect("Cut");
                }
                _ => {}
            }
            if kind == "constrained-chamfer" {
                use ferritecad_document::{
                    AddLineConstraint, AddSketchConstraint, LineConstraintKind as Kind,
                    LineEndpoint, LineLengthMm, SketchConstraintEdits, SketchCoordinateMm,
                };
                let (sketch, stored) = ferritecad_fixtures::sketch(&document).expect("sketch");
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
                        curve: stored.curves[i].id,
                        kind,
                    })
                })
                .collect();
                let prepared = ferritecad_fixtures::constraint_changes(
                    &document,
                    sketch,
                    &SketchConstraintEdits {
                        remove: vec![],
                        add,
                    },
                )
                .expect("constraints");
                document
                    .write_changes(&prepared)
                    .expect("dimensioned Chamfer");
            }
        }
        let document = Document::open_read_only(&source).expect("saved");
        let reading = ExtrudeEditSource::read(&document).expect("reading");
        let references = crate::names::snapshot(&document);
        let bytes = std::fs::read(&source).expect("source");
        for (index, slot) in reading.expression_slots.iter().enumerate() {
            let value = match slot.slot {
                ExpressionSlot::FixedX { .. } | ExpressionSlot::FixedY { .. } => {
                    slot.expression.value() + 0.1
                }
                _ => slot.expression.value() * 0.95,
            };
            let change = EditChange::constant(slot.object, slot.slot, value).expect("change");
            let request = root.path().join("request.json");
            std::fs::write(&request, serde_json::to_vec(&serde_json::json!({"request_version":1,"changes":[{"kind":"set_expression","object":slot.object,"slot":slot.slot,"value":value,"unit":"mm"}]})).expect("JSON")).expect("request");
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

            let edited = Document::open_read_only(&destination).expect("reopen");
            assert_eq!(
                crate::names::snapshot(&edited),
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

            let accepted = Document::open_read_only(session.current().path()).expect("accepted");
            assert_eq!(crate::names::snapshot(&accepted), references);
        }
        assert_eq!(std::fs::read(&source).expect("source"), bytes);
    }
}
