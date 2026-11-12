// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{Body, KeepSelection, ModelTree};

fn body(doc: &Document, id: ObjectId) -> Result<Body> {
    let ObjectPayload::Body(body) = doc.object(id)?.expect("Body").payload else {
        unreachable!("Body")
    };
    Ok(body)
}

fn set(object: ObjectId, feature: Option<ObjectId>) -> Edit {
    Edit::SetRollback { object, feature }
}

fn named(id: ObjectId, name: &str) -> String {
    format!("feature '{name}' ({id})")
}

fn part(id: ObjectId) -> String {
    format!("body 'Part' ({id})")
}

#[test]
fn rollback_round_trips_and_refuses_wrong_targets_atomically() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("marker.fcad");
    let mut doc = Document::create(&path)?;
    let c = chain(&mut doc)?;
    for feature in [Some(c.base), Some(c.middle), None] {
        doc.write_changes(&[set(c.body, feature)])?;
        assert_eq!(body(&doc, c.body)?.rollback, feature);
        assert_eq!(body(&doc, c.body)?.shown_feature(), feature.or(Some(c.tip)));
        let sql = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let stored: Vec<u8> = sql.query_row(
            "SELECT payload FROM objects WHERE id=?1",
            [c.body.to_bytes().as_slice()],
            |row| row.get(0),
        )?;
        let envelope = Envelope::from_bytes(&stored)?;
        let value: ciborium::Value = ciborium::from_reader(envelope.payload.as_slice())?;
        let ciborium::Value::Map(fields) = value else {
            unreachable!("Body map")
        };
        assert_eq!(
            fields
                .iter()
                .any(|(key, _)| key == &ciborium::Value::Text("rollback".into())),
            feature.is_some(),
            "an absent marker is omitted, never null"
        );
        let bytes = std::fs::read(&path)?;
        let reopened = Document::open_read_only(&path)?;
        assert_eq!(body(&reopened, c.body)?, body(&doc, c.body)?);
        reopened.close()?;
        assert_eq!(std::fs::read(&path)?, bytes);
    }
    let original_version = doc.content_version()?;
    let original = std::fs::read(&path)?;
    for (change, message) in [
        (
            set(c.sketch, Some(c.base)),
            format!(
                "sketch 'Profile' ({}) is not a Body; only a Body can be rolled back.",
                c.sketch
            ),
        ),
        (
            set(c.body, Some(c.sketch)),
            format!(
                "{} is not in the history of {}.",
                named(c.sketch, "Profile"),
                part(c.body)
            ),
        ),
        (
            set(c.body, Some(c.tip)),
            format!(
                "{} is the tip of {}; use clear_rollback to show the end of its history.",
                named(c.tip, "Tip"),
                part(c.body)
            ),
        ),
    ] {
        let error = doc.write_changes(&[change]).expect_err("refuse marker");
        assert_eq!(error.kind(), ErrorKind::Input);
        assert_eq!(error.to_string(), format!("invalid input: {message}"));
        assert_eq!(doc.content_version()?, original_version);
        assert_eq!(std::fs::read(&path)?, original);
    }
    Ok(())
}

#[test]
fn opening_refuses_each_invalid_marker_without_touching_the_file() -> TestResult {
    let root = tempfile::tempdir()?;
    for case in ["outside", "tip", "empty", "text", "null"] {
        let path = root.path().join(format!("{case}.fcad"));
        let mut doc = Document::create(&path)?;
        let c = chain(&mut doc)?;
        let mut payload = body(&doc, c.body)?;
        payload.rollback = Some(match case {
            "outside" => c.sketch,
            "tip" => c.tip,
            _ => c.base,
        });
        if case == "empty" {
            payload.tip_feature = None;
        }
        doc.write(|writer| {
            writer.put_object(c.body, None, 4, Some("Part"), &ObjectPayload::Body(payload))
        })?;
        doc.close()?;
        if matches!(case, "text" | "null") {
            let sql = rusqlite::Connection::open(&path)?;
            let bytes: Vec<u8> = sql.query_row(
                "SELECT payload FROM objects WHERE id=?1",
                [c.body.to_bytes().as_slice()],
                |row| row.get(0),
            )?;
            let mut envelope = Envelope::from_bytes(&bytes)?;
            let mut value: ciborium::Value = ciborium::from_reader(envelope.payload.as_slice())?;
            let ciborium::Value::Map(fields) = &mut value else {
                unreachable!("map")
            };
            let (_, marker_value) = fields
                .iter_mut()
                .find(|(key, _)| key == &ciborium::Value::Text("rollback".into()))
                .expect("marker");
            *marker_value = if case == "null" {
                ciborium::Value::Null
            } else {
                ciborium::Value::Text("not-a-uuid".into())
            };
            envelope.payload.clear();
            ciborium::into_writer(&value, &mut envelope.payload)?;
            let bytes = envelope.to_bytes()?;
            sql.execute(
                "UPDATE objects SET payload=?1, payload_hash=?2 WHERE id=?3",
                rusqlite::params![
                    bytes,
                    ContentHash::of_bytes(&bytes).as_bytes().as_slice(),
                    c.body.to_bytes().as_slice()
                ],
            )?;
        }
        let bytes = std::fs::read(&path)?;
        let error = Document::open_read_only(&path).expect_err("invalid marker");
        assert_eq!(error.kind(), ErrorKind::Input);
        let message = match case {
            "outside" => format!(
                "{} is not in the history of {}.",
                named(c.sketch, "Profile"),
                part(c.body)
            ),
            "tip" => format!(
                "{} is the tip of {}; use clear_rollback to show the end of its history.",
                named(c.tip, "Tip"),
                part(c.body)
            ),
            "empty" => format!(
                "{} is empty and cannot roll back to {}.",
                part(c.body),
                named(c.base, "Base")
            ),
            "text" | "null" => {
                format!(
                    "Body 'Part' ({}) has an unreadable history position; \
                     tip_feature and rollback must name feature UUIDs.",
                    c.body
                )
            }
            _ => unreachable!("case"),
        };
        assert_eq!(error.to_string(), format!("invalid input: {message}"));
        assert_eq!(std::fs::read(&path)?, bytes);
    }
    Ok(())
}

fn add(body: ObjectId, object: ObjectId, name: &str) -> Edit {
    Edit::AddFeature {
        object,
        body,
        name: Some(name.into()),
        feature: FeatureAddition::KeepSolids {
            selection: KeepSelection::All,
        },
    }
}

#[test]
fn ordered_insertions_match_a_history_constructed_in_that_order() -> TestResult {
    let root = tempfile::tempdir()?;
    let mut doc = Document::create(root.path().join("insert.fcad"))?;
    let c = chain(&mut doc)?;
    let ordered_path = root.path().join("ordered.fcad");
    doc.snapshot_to(&ordered_path)?;
    let initial = doc.objects()?;
    let [first, second] = std::array::from_fn(|_| ObjectId::new());
    doc.write_changes(&[set(c.body, Some(c.middle)), add(c.body, first, "First")])?;
    assert_eq!(
        body(&doc, c.body)?,
        Body {
            tip_feature: Some(c.tip),
            rollback: Some(first)
        }
    );
    doc.write_changes(&[add(c.body, second, "Second")])?;
    assert_eq!(body(&doc, c.body)?.rollback, Some(second));
    assert_eq!(
        doc.object(second)?
            .expect("second")
            .payload
            .previous_feature(),
        Some(first)
    );
    assert_eq!(
        doc.object(c.tip)?.expect("tip").payload.previous_feature(),
        Some(second)
    );
    assert_eq!(
        doc.object(first)?
            .expect("first")
            .payload
            .previous_feature(),
        Some(c.middle)
    );
    assert!(doc.validate()?.is_ok());
    let mut reference = Document::create(root.path().join("reference.fcad"))?;
    reference.write(|writer| {
        for object in initial.iter().filter(|o| o.id != c.tip && o.id != c.body) {
            writer.put_object(
                object.id,
                object.parent,
                object.ordinal,
                object.name.as_deref(),
                &object.payload,
            )?;
            for edge in object.payload.dependencies(object.id) {
                writer.add_dependency(edge)?;
            }
        }
        let payload = ObjectPayload::Body(Body {
            tip_feature: Some(c.middle),
            rollback: None,
        });
        writer.put_object(c.body, None, 99, Some("Part"), &payload)?;
        for edge in payload.dependencies(c.body) {
            writer.add_dependency(edge)?;
        }
        Ok(())
    })?;
    let ObjectPayload::Extrude(tip) = &initial.iter().find(|o| o.id == c.tip).expect("tip").payload
    else {
        unreachable!("Extrude")
    };
    reference.write_changes(&[
        add(c.body, first, "First"),
        add(c.body, second, "Second"),
        Edit::AddFeature {
            object: c.tip,
            body: c.body,
            name: Some("Tip".into()),
            feature: FeatureAddition::Extrude {
                profile: tip.profile,
                regions: tip.regions.clone(),
                end_condition: tip.end_condition.clone(),
                reversed: tip.reversed,
                operation: tip.operation,
            },
        },
    ])?;
    doc.write_changes(&[set(c.body, None)])?;
    let facts = |d: &Document| -> Result<std::collections::BTreeMap<_, _>> {
        Ok(d.objects()?
            .into_iter()
            .map(|o| (o.id, (o.name, o.payload)))
            .collect())
    };
    assert_eq!(facts(&doc)?, facts(&reference)?);
    assert_eq!(doc.dependencies()?, reference.dependencies()?);
    // Consecutive ordered requests see their own earlier marker change.
    let mut ordered = Document::open(&ordered_path)?;
    ordered.write_changes(&[
        set(c.body, Some(c.middle)),
        add(c.body, first, "First"),
        set(c.body, None),
    ])?;
    ordered.write_changes(&[
        set(c.body, Some(first)),
        add(c.body, second, "Second"),
        set(c.body, None),
    ])?;
    assert_eq!(facts(&ordered)?, facts(&reference)?);
    assert_eq!(ordered.dependencies()?, reference.dependencies()?);
    let tree = ModelTree::project(&doc.objects()?, &doc.dependencies()?);
    let ids: Vec<_> = tree
        .rows
        .iter()
        .filter(|r| r.id.1 == Some(c.body) && r.id.2.is_none())
        .map(|r| r.id.0)
        .collect();
    assert_eq!(ids, vec![c.base, c.middle, first, second, c.tip]);
    Ok(())
}

#[test]
fn removal_moves_or_clears_the_marker_and_tree_uses_full_chain_order() -> TestResult {
    let root = tempfile::tempdir()?;
    for (case, marker) in [("marker", 1), ("root", 0), ("tail", 1), ("body", 1)] {
        let mut doc = Document::create(root.path().join(format!("{case}.fcad")))?;
        let c = chain(&mut doc)?;
        let feature = [c.base, c.middle][marker];
        doc.write_changes(&[set(c.body, Some(feature))])?;
        let tree = ModelTree::project(&doc.objects()?, &doc.dependencies()?);
        let body_row = tree
            .rows
            .iter()
            .find(|r| r.id.0 == c.body)
            .expect("body row");
        assert_eq!(body_row.name, "Part · rolled back");
        assert_eq!(
            body_row.note,
            if marker == 0 {
                "Shows the result of Base; 2 later features are not shown."
            } else {
                "Shows the result of Middle; 1 later feature is not shown."
            }
        );
        for (id, name) in [(c.base, "Base"), (c.middle, "Middle"), (c.tip, "Tip")] {
            let row = tree
                .rows
                .iter()
                .find(|r| r.id.0 == id)
                .expect("feature row");
            let hidden = id == c.tip || (marker == 0 && id == c.middle);
            assert_eq!(
                row.name,
                if hidden {
                    format!("{name} · not shown")
                } else {
                    name.into()
                }
            );
        }
        let remove = |object| Edit::RemoveObject { object };
        match case {
            "marker" => {
                doc.write_changes(&[remove(c.middle)])?;
                assert_eq!(body(&doc, c.body)?.rollback, Some(c.base));
            }
            "root" => {
                doc.write_changes(&[
                    remove(c.base),
                    Edit::SetOperation {
                        object: c.middle,
                        operation: SolidOperation::NewBody,
                    },
                ])?;
                assert_eq!(body(&doc, c.body)?.rollback, None);
            }
            "tail" => {
                doc.write_changes(&[remove(c.tip)])?;
                assert_eq!(
                    body(&doc, c.body)?,
                    Body {
                        tip_feature: Some(c.middle),
                        rollback: None
                    }
                );
            }
            "body" => {
                assert_eq!(doc.dependent_closure(c.body)?.len(), 4);
                doc.write_changes(&[
                    remove(c.body),
                    remove(c.base),
                    remove(c.middle),
                    remove(c.tip),
                ])?;
                assert!(doc.object(c.body)?.is_none());
            }
            _ => unreachable!("case"),
        }
        assert!(doc.validate()?.is_ok());
    }
    Ok(())
}
