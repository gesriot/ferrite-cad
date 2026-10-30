// SPDX-License-Identifier: MIT
use ferritecad_document::*;
use ferritecad_types::{ContentHash, ObjectId, Transform};

#[test]
fn definitions_round_trip_and_dependency_rows_are_exact() {
    let root = tempfile::tempdir().expect("directory");
    let mut doc = Document::create(root.path().join("planes.fcad")).expect("document");
    let fixed = ObjectId::new();
    let offset = ObjectId::new();
    doc.write_changes(&[
        EditChange::AddDatumPlane {
            object: fixed,
            name: None,
            plane: DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            },
        },
        EditChange::AddDatumPlane {
            object: offset,
            name: None,
            plane: DatumPlane::Offset {
                from: fixed,
                distance_mm: Expression::constant(-10.).expect("negative distance"),
            },
        },
    ])
    .expect("planes");
    assert!(doc.validate().expect("validate").is_ok());
    let mut face = EntityReference::new(ReferenceMeaning {
        producer: ObjectId::new(),
        kind: EntityKind::Face,
        digest: ContentHash::of_bytes(b"face"),
        selection: Selection::One,
    });
    let definition = DatumPlane::Face {
        result: face.meaning.producer,
        face,
        offset_mm: Expression::constant(0.).expect("zero"),
    };
    let payload = ObjectPayload::DatumPlane(definition);
    assert_eq!(
        ObjectPayload::from_storage_bytes(&payload.to_storage_bytes().expect("encode"))
            .expect("decode"),
        payload
    );
    assert!(
        doc.expression_slots()
            .expect("slots")
            .iter()
            .any(|slot| slot.object == offset && slot.slot == ExpressionSlot::DatumOffset)
    );
    doc.write_changes(&[
        EditChange::constant(offset, ExpressionSlot::DatumOffset, 0.).expect("zero"),
    ])
    .expect("zero offset");
    let dependency = Dependency {
        dependent: offset,
        dependency: fixed,
        role: DependencyRole::Plane,
    };
    doc.write(|w| w.remove_dependency(dependency))
        .expect("missing dependency");
    assert!(!doc.validate().expect("missing").is_ok());
    doc.write(|w| {
        w.add_dependency(dependency)?;
        w.add_dependency(Dependency {
            dependent: fixed,
            dependency: offset,
            role: DependencyRole::Plane,
        })
    })
    .expect("extra");
    let report = doc.validate().expect("cycle validation");
    assert!(report.errors().any(|e| e.message.contains("cycle")));
    assert!(
        report
            .errors()
            .any(|e| e.code == "reference.unexpected-edge")
    );
    face.meaning.kind = EntityKind::Edge;
    assert!(
        ObjectPayload::DatumPlane(DatumPlane::Face {
            result: face.meaning.producer,
            face,
            offset_mm: Expression::constant(0.).expect("zero")
        })
        .to_storage_bytes()
        .is_err()
    );
}
