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
    chain_with_ids(document, std::array::from_fn(|_| ObjectId::new()))
}

fn chain_with_ids(
    document: &mut Document,
    [plane, sketch, body, base, middle, tip]: [ObjectId; 6],
) -> Result<Chain> {
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
fn dependent_closure_distinguishes_inputs_roots_and_spliceable_history() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut doc = Document::create(dir.path().join("closure.fcad"))?;
    let c = chain(&mut doc)?;
    let sorted = |ids: &[ObjectId]| {
        let mut ids = ids.to_vec();
        ids.sort();
        ids
    };
    assert_eq!(doc.dependent_closure(c.middle)?, vec![c.middle]);
    assert_eq!(doc.dependent_closure(c.tip)?, vec![c.tip]);
    assert_eq!(
        doc.dependent_closure(c.base)?,
        sorted(&[c.base, c.middle, c.tip])
    );
    assert_eq!(
        doc.dependent_closure(c.sketch)?,
        sorted(&[c.sketch, c.base, c.middle, c.tip])
    );
    assert_eq!(
        doc.dependent_closure(c.body)?,
        sorted(&[c.body, c.base, c.middle, c.tip])
    );
    assert_eq!(
        doc.dependent_closure(c.plane)?,
        sorted(&[c.plane, c.sketch, c.base, c.middle, c.tip])
    );
    let missing = ObjectId::new();
    let error = doc.dependent_closure(missing).expect_err("unknown object");
    assert_eq!(error.kind(), ErrorKind::Input);
    assert!(error.to_string().contains(&missing.to_string()));

    let parameter = ObjectId::new();
    let formula = ObjectId::new();
    doc.write_changes(&[
        Edit::AddParameter {
            object: parameter,
            name: "height".into(),
            dimension: Dimension::Length,
            expression: "10 mm".into(),
        },
        Edit::AddParameter {
            object: formula,
            name: "twice".into(),
            dimension: Dimension::Length,
            expression: "height * 2".into(),
        },
        Edit::SetExpressionText {
            object: c.middle,
            slot: ExpressionSlot::ExtrudeDistance,
            source: "twice".into(),
        },
    ])?;
    assert_eq!(
        doc.dependent_closure(parameter)?,
        sorted(&[parameter, formula, c.middle])
    );
    let fillet = ObjectId::new();
    doc.write_changes(&[Edit::AddFeature {
        object: fillet,
        body: c.body,
        name: Some("Distant edge consumer".into()),
        feature: FeatureAddition::Fillet {
            edges: vec![ferritecad_document::EntityReference::new(
                ferritecad_document::ReferenceMeaning {
                    producer: c.middle,
                    kind: ferritecad_document::EntityKind::Edge,
                    digest: ContentHash::of_bytes(b"stored edge"),
                    selection: ferritecad_document::Selection::One,
                },
            )],
            radius_mm: Expression::constant(1.)?,
        },
    }])?;
    assert_eq!(
        doc.dependent_closure(c.middle)?,
        sorted(&[c.middle, fillet])
    );
    assert!(
        !doc.dependencies()?
            .iter()
            .any(|d| d.dependent == fillet && d.dependency == c.middle)
    );
    let removals: Vec<_> = doc
        .dependent_closure(c.base)?
        .into_iter()
        .map(|object| Edit::RemoveObject { object })
        .collect();
    doc.write_changes(&removals)?;
    assert_eq!(tip(&doc, c.body)?, None);
    assert!(doc.object(c.sketch)?.is_some());
    Ok(())
}

#[test]
fn dependent_closure_follows_every_saved_reference_and_presentation_children() -> TestResult {
    use ferritecad_document::{EntityKind, EntityReference, ReferenceMeaning, Selection};
    let mut doc = Document::create(":memory:")?;
    let c = chain(&mut doc)?;
    let reference = |kind| {
        EntityReference::new(ReferenceMeaning {
            producer: c.middle,
            kind,
            digest: ContentHash::of_bytes(b"named input"),
            selection: Selection::One,
        })
    };
    let mut expected = vec![c.middle];
    for feature in [
        FeatureAddition::Chamfer {
            edges: vec![reference(EntityKind::Edge)],
            distance_mm: Expression::constant(1.)?,
        },
        FeatureAddition::KeepSolids {
            selection: ferritecad_document::KeepSelection::ContainingFaces(vec![reference(
                EntityKind::Face,
            )]),
        },
        FeatureAddition::Extrude {
            profile: c.sketch,
            regions: vec![],
            reversed: false,
            operation: SolidOperation::Add,
            end_condition: EndCondition::UpToFace {
                result: c.tip,
                face: reference(EntityKind::Face),
                offset_mm: Expression::constant(0.)?,
            },
        },
        FeatureAddition::Extrude {
            profile: c.sketch,
            regions: vec![],
            reversed: false,
            operation: SolidOperation::Add,
            end_condition: EndCondition::UpToVertex {
                result: c.tip,
                vertex: reference(EntityKind::Vertex),
            },
        },
    ] {
        let object = ObjectId::new();
        doc.write_changes(&[Edit::AddFeature {
            object,
            body: c.body,
            name: Some("Named consumer".into()),
            feature,
        }])?;
        expected.push(object);
    }
    let datum = ObjectId::new();
    let offset = ObjectId::new();
    doc.write_changes(&[
        Edit::AddDatumPlane {
            object: datum,
            name: Some("Face plane".into()),
            plane: DatumPlane::Face {
                result: c.tip,
                face: reference(EntityKind::Face),
                offset_mm: Expression::constant(0.)?,
            },
        },
        Edit::AddDatumPlane {
            object: offset,
            name: Some("Offset plane".into()),
            plane: DatumPlane::Offset {
                from: datum,
                distance_mm: Expression::constant(2.)?,
            },
        },
    ])?;
    let child = ObjectId::new();
    doc.write(|w| {
        w.put_object(
            child,
            Some(c.middle),
            20,
            Some("Presentation child"),
            &ObjectPayload::DatumPlane(DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            }),
        )
    })?;
    expected.extend([datum, offset, child]);
    expected.sort();
    assert_eq!(doc.dependent_closure(c.middle)?, expected);
    assert!(doc.dependencies()?.iter().any(|d| d.dependent == datum
        && d.dependency == c.tip
        && d.role == DependencyRole::Support));
    assert!(doc.dependent_closure(c.tip)?.contains(&datum));
    assert!(doc.dependent_closure(datum)?.contains(&offset));
    doc.write_changes(
        &expected
            .into_iter()
            .map(|object| Edit::RemoveObject { object })
            .collect::<Vec<_>>(),
    )?;
    assert_eq!(tip(&doc, c.body)?, Some(c.tip));
    assert!(doc.object(c.base)?.is_some());
    Ok(())
}

#[test]
fn dependent_closure_refuses_a_history_changed_after_opening() -> TestResult {
    for mode in 0..3 {
        let root = tempfile::tempdir()?;
        let path = root.path().join("changed.fcad");
        let mut doc = Document::create(&path)?;
        // Visit feature links before the Body, so the two independent broken-link
        // guards are exercised separately rather than racing in UUID order.
        let mut ids: [ObjectId; 6] = std::array::from_fn(|_| ObjectId::new());
        ids.sort();
        let [plane, sketch, base, middle, tip, body] = ids;
        let c = chain_with_ids(&mut doc, [plane, sketch, body, base, middle, tip])?;
        let missing = ObjectId::new();
        let id = if mode == 2 { c.body } else { c.middle };
        let mut payload = doc.object(id)?.expect("stored row").payload;
        if mode == 2 {
            let ObjectPayload::Body(body) = &mut payload else {
                unreachable!("Body");
            };
            body.tip_feature = Some(missing);
        } else {
            let ObjectPayload::Extrude(feature) = &mut payload else {
                unreachable!("Extrude");
            };
            feature.previous = Some(if mode == 0 { missing } else { c.tip });
        }
        // Simulate another writer invalidating the already-open view. The normal
        // transaction never admits either a broken link or a history cycle.
        let bytes = payload.to_storage_bytes()?;
        let hash = ContentHash::of_bytes(&bytes);
        let sql = rusqlite::Connection::open(&path)?;
        sql.execute(
            "UPDATE objects SET payload=?1,payload_hash=?2 WHERE id=?3",
            rusqlite::params![bytes, hash.as_bytes().as_slice(), id.to_bytes().as_slice()],
        )?;
        drop(sql);
        let before = std::fs::read(&path)?;
        let error = doc
            .dependent_closure(c.body)
            .expect_err("broken history must refuse");
        assert_eq!(error.kind(), ErrorKind::Input);
        let message = if mode == 2 {
            format!("missing history feature {missing}")
        } else {
            "the stored history is incomplete or cyclic".into()
        };
        assert!(error.to_string().contains(&message), "{error}");
        assert_eq!(std::fs::read(&path)?, before);
    }
    Ok(())
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
