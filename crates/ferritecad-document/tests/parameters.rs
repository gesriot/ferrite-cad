// SPDX-License-Identifier: MIT
use ferritecad_document::{DependencyRole, Document, EditChange, ObjectPayload, Parameter};
use ferritecad_types::{Dimension, ObjectId, Result};

fn add(id: ObjectId, name: &str, expression: &str) -> EditChange {
    EditChange::AddParameter {
        object: id,
        name: name.into(),
        dimension: Dimension::Length,
        expression: expression.into(),
    }
}

fn param(doc: &Document, id: ObjectId) -> Result<Parameter> {
    match doc.object(id)?.expect("parameter object").payload {
        ObjectPayload::Parameter(p) => Ok(p),
        _ => unreachable!("parameter"),
    }
}

#[test]
fn forward_references_rename_removal_and_cycles_are_atomic() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("parameters.fcad");
    let mut doc = Document::create(&path)?;
    let [a, ab, c] = std::array::from_fn(|_| ObjectId::new());
    doc.write_changes(&[
        add(c, "c", "\t a + ab  - a / 2 "),
        add(ab, "ab", "3 * a"),
        add(a, "a", "1in"),
    ])?;
    assert_eq!(
        param(&doc, c)?.expression.value(),
        25.4 + 3. * 25.4 - 25.4 / 2.
    );
    doc.write_changes(&[EditChange::RenameParameter {
        object: a,
        name: "base".into(),
    }])?;
    assert_eq!(
        param(&doc, c)?.expression.source,
        "\t base + ab  - base / 2 "
    );
    assert_eq!(param(&doc, ab)?.expression.source, "3 * base");
    doc.write_changes(&[EditChange::SetParameter {
        object: a,
        expression: "2 mm".into(),
    }])?;
    assert_eq!(param(&doc, c)?.expression.value(), 7.);
    let before = doc.content_version()?;
    for (change, words) in [
        (
            EditChange::SetParameter {
                object: a,
                expression: "ab".into(),
            },
            vec!["cycle", "base", "ab"],
        ),
        (
            EditChange::RemoveParameter { object: a },
            vec!["referenced by", "ab", "c"],
        ),
        (
            EditChange::RenameParameter {
                object: a,
                name: "ab".into(),
            },
            vec!["duplicate", "ab"],
        ),
        (
            EditChange::RenameParameter {
                object: a,
                name: "deg".into(),
            },
            vec!["reserved", "deg"],
        ),
        (
            EditChange::SetParameter {
                object: a,
                expression: "1 mm / 0".into(),
            },
            vec!["division by zero", "base", "which failed", "parameter ab"],
        ),
    ] {
        let error = doc.write_changes(&[change]).expect_err("refused candidate");
        for word in words {
            assert!(error.to_string().contains(word), "{error}");
        }
        assert_eq!(doc.content_version()?, before);
    }
    doc.close()?;
    let mut doc = Document::open(path)?;
    assert_eq!(param(&doc, a)?.expression.source, "2 mm");
    let deps = doc.dependencies()?;
    assert_eq!(deps.len(), 3);
    assert!(deps.iter().all(|e| e.role == DependencyRole::Parameter));
    doc.write_changes(&[
        EditChange::RemoveParameter { object: c },
        EditChange::RemoveParameter { object: ab },
        EditChange::RemoveParameter { object: a },
    ])?;
    assert!(doc.objects()?.is_empty());
    assert!(doc.dependencies()?.is_empty());
    Ok(())
}

#[test]
fn syntax_and_unknown_names_prevent_open_but_evaluation_errors_are_readable() -> Result<()> {
    for (text, problem) in [
        ("bad_name * 2", Some("unknown parameter")),
        ("2 mm + * 4 mm", Some("expected a number")),
        ("(3 mm", Some("unclosed")),
        ("4 mm / 0", None),
        ("2 rad", None),
        ("6 mm", None),
    ] {
        let root = tempfile::tempdir()?;
        let path = root.path().join("record.fcad");
        let mut doc = Document::create(&path)?;
        let id = ObjectId::new();
        doc.write(|w| {
            w.put_object(
                id,
                None,
                0,
                Some("width"),
                &ObjectPayload::Parameter(Parameter {
                    name: "width".into(),
                    dimension: Dimension::Length,
                    expression: ferritecad_document::Expression::new(text, 5.)?,
                }),
            )
        })?;
        doc.close()?;
        let bytes = std::fs::read(&path)?;
        if let Some(reason) = problem {
            let e = Document::open_read_only(&path).expect_err("cannot resolve source");
            assert_eq!(e.kind(), ferritecad_types::ErrorKind::Input);
            for word in [reason, &id.to_string(), "width", "byte"] {
                assert!(e.to_string().contains(word), "{e}");
            }
        } else {
            let doc = Document::open_read_only(&path)?;
            assert_eq!(param(&doc, id)?.expression.value(), 5.);
        }
        assert_eq!(std::fs::read(&path)?, bytes);
    }
    Ok(())
}

#[test]
fn an_invalid_saved_distance_opens_for_repair_when_its_source_disagrees() -> Result<()> {
    use ferritecad_document::{
        EndCondition, Envelope, Expression, ExpressionSlot, Extrude, ParameterValues,
        SolidOperation,
    };
    use ferritecad_types::ContentHash;
    let root = tempfile::tempdir()?;
    let path = root.path().join("stale.fcad");
    let mut doc = Document::create(&path)?;
    let id = ObjectId::new();
    let mut extrude = Extrude {
        profile: ObjectId::new(),
        regions: vec![],
        reversed: false,
        previous: None,
        operation: SolidOperation::NewBody,
        end_condition: EndCondition::Blind {
            distance: Expression::constant(8.)?,
        },
    };
    doc.write(|w| w.put_object(id, None, 0, None, &ObjectPayload::Extrude(extrude.clone())))?;
    doc.close()?;
    extrude.end_condition = EndCondition::Blind {
        distance: Expression::new("8 mm", -1.)?,
    };
    let bytes = Envelope::encode("feature.extrude", &extrude)?.to_bytes()?;
    let conn = rusqlite::Connection::open(&path).expect("SQLite");
    conn.execute(
        "UPDATE objects SET payload=?1, payload_hash=?2 WHERE id=?3",
        rusqlite::params![
            &bytes,
            ContentHash::of_bytes(&bytes).as_bytes().as_slice(),
            id.to_bytes().as_slice()
        ],
    )
    .expect("stale value with valid hash");
    drop(conn);
    let mut doc = Document::open(&path)?;
    let stale = doc.object(id)?.expect("Extrude");
    let error = ParameterValues::default()
        .verify(id, &stale.payload)
        .expect_err("value mismatch");
    assert!(error.to_string().contains("stored value -1"), "{error}");
    doc.write_changes(&[EditChange::SetExpressionText {
        object: id,
        slot: ExpressionSlot::ExtrudeDistance,
        source: "8 mm".into(),
    }])?;
    ParameterValues::default().verify(id, &doc.object(id)?.expect("repaired").payload)
}

#[test]
fn stored_parameter_names_must_be_unique_and_valid_at_open() -> Result<()> {
    for (second, reason) in [
        ("h", "duplicate parameter name"),
        ("cm", "reserved for a unit"),
    ] {
        let root = tempfile::tempdir()?;
        let path = root.path().join("names.fcad");
        let mut doc = Document::create(&path)?;
        doc.write(|w| {
            for (ordinal, name) in ["h", second].into_iter().enumerate() {
                let p = Parameter {
                    name: name.into(),
                    dimension: Dimension::Length,
                    expression: ferritecad_document::Expression::constant(1.)?,
                };
                w.put_object(
                    ObjectId::new(),
                    None,
                    ordinal as i64,
                    None,
                    &ObjectPayload::Parameter(p),
                )?;
            }
            Ok(())
        })?;
        doc.close()?;
        let error = Document::open_read_only(&path).expect_err("invalid stored names");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
        assert!(error.to_string().contains(reason), "{error}");
    }
    Ok(())
}

#[path = "support/expression.rs"]
mod expression;

#[test]
fn even_a_zero_sign_difference_is_a_stored_value_mismatch() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("negative-zero.fcad");
    let id = ObjectId::new();
    let mut saved = expression::unchecked(-0.0);
    saved.source = "0".into();
    let payload = ObjectPayload::Parameter(Parameter {
        name: "offset".into(),
        dimension: Dimension::Length,
        expression: saved,
    });
    let mut doc = Document::create(&path)?;
    doc.write(|w| w.put_object(id, None, 0, None, &payload))?;
    doc.close()?;
    let read = Document::open_read_only(&path)?;
    let stored = read.object(id)?.expect("parameter");
    assert_eq!(
        param(&read, id)?.expression.value().to_bits(),
        (-0.0_f64).to_bits()
    );
    let error = ferritecad_document::ParameterValues::default()
        .verify(id, &stored.payload)
        .expect_err("signed zero must fail bitwise comparison");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
    assert!(
        error
            .to_string()
            .contains("stored value -0 differs from the evaluated value 0")
    );
    Ok(())
}
