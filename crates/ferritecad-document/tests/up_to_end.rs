// SPDX-License-Identifier: MIT
use ferritecad_document::*;
use ferritecad_types::{ContentHash, ObjectId, Transform};

fn end(kind: EntityKind, result: ObjectId) -> EndCondition {
    let reference = EntityReference::new(ReferenceMeaning {
        producer: result,
        kind,
        digest: ContentHash::of_bytes(b"end meaning"),
        selection: Selection::One,
    });
    if kind == EntityKind::Face {
        EndCondition::UpToFace {
            result,
            face: reference,
            offset_mm: Expression::constant(-2.).expect("offset"),
        }
    } else {
        EndCondition::UpToVertex {
            result,
            vertex: reference,
        }
    }
}

#[test]
fn referenced_end_payloads_round_trip_and_validate_kind_selection_and_offset() {
    for operation in [
        SolidOperation::NewBody,
        SolidOperation::Add,
        SolidOperation::Cut,
        SolidOperation::Intersect,
    ] {
        for kind in [EntityKind::Face, EntityKind::Vertex] {
            let mut extrude = Extrude {
                profile: ObjectId::new(),
                regions: vec![],
                reversed: true,
                operation,
                previous: (operation != SolidOperation::NewBody).then(ObjectId::new),
                end_condition: end(kind, ObjectId::new()),
            };
            let payload = ObjectPayload::Extrude(extrude.clone());
            assert_eq!(
                ObjectPayload::from_storage_bytes(&payload.to_storage_bytes().expect("encode"))
                    .expect("decode"),
                payload
            );
            let key = extrude.cache_key(Default::default());
            let reference = match &mut extrude.end_condition {
                EndCondition::UpToFace { face, .. } => face,
                EndCondition::UpToVertex { vertex, .. } => vertex,
                _ => unreachable!(),
            };
            reference.id = ferritecad_types::StableEntityId::new();
            assert_eq!(extrude.cache_key(Default::default()), key);
            if let EndCondition::UpToVertex { vertex, .. } = &mut extrude.end_condition {
                vertex.meaning.kind = EntityKind::Face;
                assert!(
                    ObjectPayload::Extrude(extrude.clone())
                        .to_storage_bytes()
                        .is_err()
                );
                let EndCondition::UpToVertex { vertex, .. } = &mut extrude.end_condition else {
                    unreachable!()
                };
                vertex.meaning.kind = EntityKind::Vertex;
            }
            match &mut extrude.end_condition {
                EndCondition::UpToFace { face, .. } => face.meaning.kind = EntityKind::Edge,
                EndCondition::UpToVertex { vertex, .. } => {
                    vertex.meaning.selection = Selection::AllDescendants
                }
                _ => unreachable!(),
            }
            assert!(ObjectPayload::Extrude(extrude).to_storage_bytes().is_err());
        }
    }
}

#[test]
fn existing_extrudes_add_remove_support_rows_and_reject_missing_wrong_and_cyclic_targets() {
    let root = tempfile::tempdir().expect("directory");
    let mut doc = Document::create(root.path().join("ends.fcad")).expect("document");
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let first = ObjectId::new();
    let second = ObjectId::new();
    let body = ObjectId::new();
    let blind = || EndCondition::Blind {
        distance: Expression::constant(10.).expect("height"),
    };
    doc.write_changes(&[
        EditChange::AddDatumPlane {
            object: plane,
            name: None,
            plane: DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            },
        },
        EditChange::AddSketch {
            object: sketch,
            name: None,
            sketch: Sketch {
                plane,
                curves: vec![],
                constraints: vec![],
            },
        },
        EditChange::AddBody {
            object: body,
            name: None,
            feature_object: first,
            feature_name: None,
            feature: FeatureAddition::Extrude {
                profile: sketch,
                regions: vec![],
                reversed: false,
                operation: SolidOperation::NewBody,
                end_condition: blind(),
            },
        },
        EditChange::AddFeature {
            object: second,
            body,
            name: None,
            feature: FeatureAddition::Extrude {
                profile: sketch,
                regions: vec![],
                reversed: false,
                operation: SolidOperation::Add,
                end_condition: blind(),
            },
        },
    ])
    .expect("objects");
    let change = |object, end_condition| EditChange::SetEndCondition {
        object,
        end_condition,
    };
    let original = doc.content_version().expect("content");
    for target in [ObjectId::new(), plane] {
        assert!(
            doc.write_changes(&[change(first, end(EntityKind::Face, target))])
                .is_err()
        );
        assert_eq!(doc.content_version().expect("unchanged"), original);
    }
    doc.write_changes(&[change(first, end(EntityKind::Face, second))])
        .expect("raw cyclic fixture");
    assert!(
        doc.validate()
            .expect("cycle")
            .errors()
            .any(|e| e.message.contains("cycle"))
    );
    doc.write_changes(&[change(first, blind())])
        .expect("repair cycle");
    let support = Dependency {
        dependent: second,
        dependency: first,
        role: DependencyRole::Support,
    };
    for condition in [
        end(EntityKind::Face, first),
        end(EntityKind::Vertex, first),
        blind(),
    ] {
        let present = condition.support().is_some();
        doc.write_changes(&[change(second, condition)])
            .expect("change end");
        assert_eq!(
            doc.dependencies().expect("dependencies").contains(&support),
            present
        );
        assert!(doc.validate().expect("validation").is_ok());
    }
    doc.write_changes(&[
        change(second, end(EntityKind::Face, first)),
        EditChange::constant(second, ExpressionSlot::ExtrudeEndOffset, 0.).expect("offset"),
    ])
    .expect("slot");
    assert!(
        doc.expression_slots()
            .expect("slots")
            .iter()
            .any(|s| s.object == second
                && s.slot == ExpressionSlot::ExtrudeEndOffset
                && s.expression.value() == 0.)
    );
    doc.write(|w| w.remove_dependency(support))
        .expect("remove row");
    assert!(
        doc.validate()
            .expect("missing")
            .errors()
            .any(|e| e.code == "reference.missing-edge")
    );
    doc.write(|w| {
        w.add_dependency(support)?;
        w.add_dependency(Dependency {
            dependent: first,
            dependency: second,
            role: DependencyRole::Support,
        })
    })
    .expect("extra row");
    assert!(
        doc.validate()
            .expect("extra")
            .errors()
            .any(|e| e.code == "reference.unexpected-edge")
    );
}
