// SPDX-License-Identifier: MIT
use ferritecad_document::{
    DatumPlane, DependencyRole, Document, EditChange as Edit, EndCondition, Envelope, Expression,
    ExpressionSlot, FeatureAddition, ObjectPayload, Point2, Sketch, SketchCurve, SketchGeometry,
    SolidOperation,
};
use ferritecad_types::{
    ContentHash, Dimension, ErrorKind, ObjectId, Result, StableEntityId, Transform,
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

struct Chain {
    plane: ObjectId,
    sketch: ObjectId,
    body: ObjectId,
    base: ObjectId,
    middle: ObjectId,
    tip: ObjectId,
}

fn chain(document: &mut Document) -> Result<Chain> {
    let [plane, sketch, body, base, middle, tip] = std::array::from_fn(|_| ObjectId::new());
    let feature = |operation| FeatureAddition::Extrude {
        profile: sketch,
        regions: Vec::new(),
        end_condition: EndCondition::Blind {
            distance: Expression::constant(10.).expect("finite"),
        },
        reversed: false,
        operation,
    };
    document.write_changes(&[
        Edit::AddDatumPlane {
            object: plane,
            name: Some("XY".into()),
            plane: DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            },
        },
        Edit::AddSketch {
            object: sketch,
            name: Some("Profile".into()),
            sketch: Sketch {
                plane,
                curves: vec![SketchCurve {
                    id: StableEntityId::new(),
                    construction: false,
                    geometry: SketchGeometry::Circle {
                        center: Point2::ORIGIN,
                        radius: 4.,
                    },
                }],
                constraints: Vec::new(),
            },
        },
        Edit::AddBody {
            object: body,
            name: Some("Part".into()),
            feature_object: base,
            feature_name: Some("Base".into()),
            feature: feature(SolidOperation::NewBody),
        },
        Edit::AddFeature {
            object: middle,
            body,
            name: Some("Middle".into()),
            feature: feature(SolidOperation::Add),
        },
        Edit::AddFeature {
            object: tip,
            body,
            name: Some("Tip".into()),
            feature: feature(SolidOperation::Add),
        },
    ])?;
    Ok(Chain {
        plane,
        sketch,
        body,
        base,
        middle,
        tip,
    })
}

fn tip(document: &Document, id: ObjectId) -> Result<Option<ObjectId>> {
    let ObjectPayload::Body(body) = document.object(id)?.expect("body").payload else {
        unreachable!("body")
    };
    Ok(body.tip_feature)
}

#[test]
fn removal_splices_the_complete_batch_and_retains_an_empty_body() -> TestResult {
    for remove_base in [false, true] {
        let dir = tempfile::tempdir()?;
        let mut doc = Document::create(dir.path().join("part.fcad"))?;
        let c = chain(&mut doc)?;
        doc.write_changes(&[Edit::RemoveObject { object: c.middle }])?;
        assert_eq!(
            doc.object(c.tip)?.expect("tip").payload.previous_feature(),
            Some(c.base)
        );
        assert_eq!(tip(&doc, c.body)?, Some(c.tip));
        assert!(
            !doc.dependencies()?
                .iter()
                .any(|d| d.dependency == c.middle || d.dependent == c.middle)
        );
        assert!(doc.dependencies()?.iter().any(|d| d.dependent == c.tip
            && d.dependency == c.base
            && d.role == DependencyRole::Predecessor));
        let mut changes = vec![Edit::RemoveObject { object: c.tip }];
        if remove_base {
            changes.insert(0, Edit::RemoveObject { object: c.base });
        }
        doc.write_changes(&changes)?;
        assert_eq!(tip(&doc, c.body)?, (!remove_base).then_some(c.base));
        assert!(doc.validate()?.is_ok());
    }
    Ok(())
}

#[test]
fn body_and_all_its_inputs_can_be_removed_explicitly_in_any_order() -> TestResult {
    for reverse in [false, true] {
        let dir = tempfile::tempdir()?;
        let mut doc = Document::create(dir.path().join("part.fcad"))?;
        let c = chain(&mut doc)?;
        let mut ids = vec![c.body, c.plane, c.base, c.sketch, c.middle, c.tip];
        if reverse {
            ids.reverse();
        }
        let changes: Vec<_> = ids
            .into_iter()
            .map(|object| Edit::RemoveObject { object })
            .collect();
        doc.write_changes(&changes)?;
        assert!(doc.objects()?.is_empty());
        assert!(doc.dependencies()?.is_empty());
    }
    Ok(())
}

#[test]
fn refusal_lists_all_remaining_users_and_never_changes_the_source() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("part.fcad");
    let mut doc = Document::create(&path)?;
    let c = chain(&mut doc)?;
    let parameter = ObjectId::new();
    doc.write_changes(&[
        Edit::AddParameter {
            object: parameter,
            name: "height".into(),
            dimension: Dimension::Length,
            expression: "10 mm".into(),
        },
        Edit::SetExpressionText {
            object: c.base,
            slot: ExpressionSlot::ExtrudeDistance,
            source: "height".into(),
        },
        Edit::SetExpressionText {
            object: c.tip,
            slot: ExpressionSlot::ExtrudeDistance,
            source: "height".into(),
        },
    ])?;
    for (ids, words) in [
        (
            vec![c.sketch],
            vec!["sketch", "Profile", "Base", "Middle", "Tip"],
        ),
        (
            vec![parameter],
            vec!["parameter", "height", "referenced by", "Base", "Tip"],
        ),
        (vec![c.body], vec!["body", "Part", "Base", "Middle", "Tip"]),
        (vec![c.base], vec!["Middle", "predecessor"]),
        (
            vec![parameter, c.sketch],
            vec!["height", "Profile", "Base", "Middle", "Tip"],
        ),
    ] {
        let mut dependents = Vec::new();
        if ids.contains(&parameter) {
            dependents.extend([c.base, c.tip]);
        }
        if ids.contains(&c.sketch) || ids.contains(&c.body) {
            dependents.extend([c.base, c.middle, c.tip]);
        }
        if ids.contains(&c.base) {
            dependents.push(c.middle);
        }
        let before = std::fs::read(&path)?;
        let changes: Vec<_> = ids
            .into_iter()
            .map(|object| Edit::RemoveObject { object })
            .collect();
        let error = doc
            .write_changes(&changes)
            .expect_err("remaining dependents refuse");
        assert_eq!(error.kind(), ErrorKind::Input);
        for word in words {
            assert!(error.to_string().contains(word), "{error}");
        }
        for dependent in dependents {
            assert!(
                error.to_string().contains(&dependent.to_string()),
                "{error}"
            );
        }
        assert_eq!(std::fs::read(&path)?, before);
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 1);
    }
    Ok(())
}

#[test]
fn suppression_survives_edits_and_reopening_and_changes_the_evaluated_input() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("part.fcad");
    let mut doc = Document::create(&path)?;
    let c = chain(&mut doc)?;
    let original = doc
        .object(c.middle)?
        .expect("feature")
        .evaluated_payload_hash()?;
    doc.write_changes(&[Edit::SetSuppressed {
        object: c.middle,
        suppressed: true,
    }])?;
    let suppressed = doc.object(c.middle)?.expect("feature");
    assert!(suppressed.suppressed);
    assert_ne!(suppressed.evaluated_payload_hash()?, original);
    doc.write_changes(&[Edit::constant(
        c.middle,
        ExpressionSlot::ExtrudeDistance,
        12.,
    )?])?;
    doc.close()?;
    let mut doc = Document::open(&path)?;
    assert!(doc.object(c.middle)?.expect("feature").suppressed);
    doc.write_changes(&[
        Edit::constant(c.middle, ExpressionSlot::ExtrudeDistance, 10.)?,
        Edit::SetSuppressed {
            object: c.middle,
            suppressed: false,
        },
    ])?;
    let restored = doc.object(c.middle)?.expect("feature");
    assert!(!restored.suppressed);
    assert_eq!(restored.evaluated_payload_hash()?, original);
    Ok(())
}

#[test]
fn non_features_refuse_suppression_at_edit_and_at_open_even_for_a_false_flag() -> TestResult {
    for flag in [false, true] {
        for kind in 0..4 {
            let dir = tempfile::tempdir()?;
            let path = dir.path().join("part.fcad");
            let mut doc = Document::create(&path)?;
            let c = chain(&mut doc)?;
            let parameter = ObjectId::new();
            doc.write_changes(&[Edit::AddParameter {
                object: parameter,
                name: "height".into(),
                dimension: Dimension::Length,
                expression: "10 mm".into(),
            }])?;
            let (id, label) = [
                (c.sketch, "sketch"),
                (c.plane, "datum plane"),
                (c.body, "body"),
                (parameter, "parameter"),
            ][kind];
            let error = doc
                .write_changes(&[Edit::SetSuppressed {
                    object: id,
                    suppressed: flag,
                }])
                .expect_err("wrong kind");
            assert_eq!(error.kind(), ErrorKind::Input);
            assert!(error.to_string().contains(label), "{error}");
            doc.close()?;
            let sql = rusqlite::Connection::open(&path)?;
            let raw: Vec<u8> = sql.query_row(
                "SELECT payload FROM objects WHERE id=?1",
                [id.to_bytes().as_slice()],
                |r| r.get(0),
            )?;
            let mut envelope = Envelope::from_bytes(&raw)?;
            envelope.suppressed = Some(flag);
            let bytes = envelope.to_bytes()?;
            let hash = ContentHash::of_bytes(&bytes);
            sql.execute(
                "UPDATE objects SET payload=?1,payload_hash=?2 WHERE id=?3",
                rusqlite::params![bytes, hash.as_bytes().as_slice(), id.to_bytes().as_slice()],
            )?;
            drop(sql);
            let before = std::fs::read(&path)?;
            let error = Document::open_read_only(&path).expect_err("strict flag opening");
            assert_eq!(error.kind(), ErrorKind::Input);
            assert!(error.to_string().contains(label), "{error}");
            assert_eq!(std::fs::read(&path)?, before);
            assert_eq!(std::fs::read_dir(dir.path())?.count(), 1);
        }
    }
    Ok(())
}

#[test]
fn explicit_new_body_edit_can_replace_a_deleted_root_and_adjacent_links_splice() -> TestResult {
    for reverse in [false, true] {
        let mut document = Document::create(":memory:")?;
        let c = chain(&mut document)?;
        let mut changes = vec![
            Edit::RemoveObject { object: c.base },
            Edit::RemoveObject { object: c.middle },
            Edit::SetOperation {
                object: c.tip,
                operation: SolidOperation::NewBody,
            },
        ];
        if reverse {
            changes.reverse();
        }
        document.write_changes(&changes)?;
        assert_eq!(
            document
                .object(c.tip)?
                .expect("new root")
                .payload
                .previous_feature(),
            None
        );
        assert_eq!(tip(&document, c.body)?, Some(c.tip));
        assert!(document.validate()?.is_ok());
    }
    Ok(())
}

#[test]
fn a_presentation_parent_never_silently_deletes_its_child() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("source.fcad");
    let mut document = Document::create(&path)?;
    let parent = ObjectId::new();
    let child = ObjectId::new();
    document.write(|w| {
        w.put_object(
            parent,
            None,
            0,
            Some("Parent"),
            &ObjectPayload::Body(ferritecad_document::Body { tip_feature: None }),
        )?;
        w.put_object(
            child,
            Some(parent),
            1,
            Some("Child"),
            &ObjectPayload::DatumPlane(DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            }),
        )?;
        Ok(())
    })?;
    let bytes = std::fs::read(&path)?;
    let error = document
        .write_changes(&[Edit::RemoveObject { object: parent }])
        .expect_err("no implicit cascade");
    for word in ["Parent", "Child", "datum plane", &child.to_string()] {
        assert!(error.to_string().contains(word), "{error}");
    }
    assert_eq!(std::fs::read(&path)?, bytes);
    document.write_changes(&[
        Edit::RemoveObject { object: parent },
        Edit::RemoveObject { object: child },
    ])?;
    assert!(document.objects()?.is_empty());
    Ok(())
}

#[test]
fn unknown_objects_and_non_boolean_flags_are_input_errors() -> TestResult {
    let mut doc = Document::create(":memory:")?;
    let c = chain(&mut doc)?;
    for change in [
        Edit::RemoveObject {
            object: ObjectId::new(),
        },
        Edit::SetSuppressed {
            object: ObjectId::new(),
            suppressed: true,
        },
    ] {
        assert_eq!(
            doc.write_changes(&[change])
                .expect_err("unknown object")
                .kind(),
            ErrorKind::Input
        );
    }
    let object = doc.object(c.base)?.expect("base");
    for flag in [
        ciborium::Value::Null,
        ciborium::Value::Integer(1.into()),
        ciborium::Value::Text("true".into()),
    ] {
        let mut envelope: ciborium::Value =
            ciborium::from_reader(object.payload.to_storage_bytes()?.as_slice())?;
        let ciborium::Value::Map(fields) = &mut envelope else {
            unreachable!("envelope map")
        };
        fields.push((ciborium::Value::Text("suppressed".into()), flag));
        let mut bytes = Vec::new();
        ciborium::into_writer(&envelope, &mut bytes)?;
        assert_eq!(
            Envelope::from_bytes(&bytes)
                .expect_err("must be a boolean")
                .kind(),
            ErrorKind::Input
        );
    }
    Ok(())
}
