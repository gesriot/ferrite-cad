// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_types::{ContentHash, ErrorKind, ObjectId, StableEntityId, Transform};
use rusqlite::{Connection, params, types::Value};
use std::{collections::BTreeMap, path::Path};

fn fixture() -> (tempfile::TempDir, Document, ObjectId) {
    let root = tempfile::tempdir().expect("dir");
    let mut d = Document::create(root.path().join("source.fcad")).expect("document");
    let [plane, id, extrude, body] = std::array::from_fn(|_| ObjectId::new());
    let points = [[-20., -10.], [40., -8.], [42., 30.], [-20., 30.]];
    let curves = points
        .iter()
        .enumerate()
        .map(|(i, p)| SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(p[0], p[1]).expect("p"),
                end: Point2::new(points[(i + 1) % 4][0], points[(i + 1) % 4][1]).expect("p"),
            },
        })
        .collect();
    d.write(|w| {
        for (oid, ordinal, payload) in [
            (
                plane,
                0,
                ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            ),
            (
                id,
                1,
                ObjectPayload::Sketch(Sketch {
                    plane,
                    curves,
                    constraints: vec![],
                }),
            ),
            (
                extrude,
                2,
                ObjectPayload::Extrude(Extrude {
                    profile: id,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(10.)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    target_body: None,
                }),
            ),
            (
                body,
                3,
                ObjectPayload::Body(Body {
                    tip_feature: Some(extrude),
                }),
            ),
        ] {
            w.put_object(oid, None, ordinal, Some("Чертёж \"H/V\"\nname"), &payload)?;
        }
        for (dependent, dependency, role) in [
            (id, plane, DependencyRole::Plane),
            (extrude, id, DependencyRole::Profile),
            (body, extrude, DependencyRole::BodyTip),
        ] {
            w.add_dependency(Dependency {
                dependent,
                dependency,
                role,
            })?;
        }
        Ok(())
    })
    .expect("fixture");
    (root, d, id)
}
fn stored(d: &Document, id: ObjectId) -> Sketch {
    let ObjectPayload::Sketch(s) = d.object(id).expect("read").expect("sketch").payload else {
        panic!("Sketch")
    };
    s
}
fn add(curve: StableEntityId, kind: LineConstraintKind) -> SketchConstraintEdits {
    SketchConstraintEdits {
        remove: vec![],
        add: vec![AddLineConstraint { curve, kind }],
    }
}
fn cells(path: &Path) -> BTreeMap<String, Vec<Vec<Value>>> {
    let c = Connection::open(path).expect("SQL");
    let names = c
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .expect("names")
        .query_map([], |r| r.get::<_, String>(0))
        .expect("names")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("names");
    names
        .into_iter()
        .map(|name| {
            let q = format!("\"{}\"", name.replace('"', "\"\""));
            let mut stmt = c
                .prepare(&format!("SELECT rowid,* FROM {q} ORDER BY rowid"))
                .or_else(|_| c.prepare(&format!("SELECT * FROM {q} ORDER BY 1,2")))
                .expect("table");
            let n = stmt.column_count();
            let rows = stmt
                .query_map([], |r| (0..n).map(|i| r.get(i)).collect())
                .expect("rows")
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("rows");
            (name, rows)
        })
        .collect()
}
#[test]
fn constraint_write_preserves_claims_rows_metadata_and_remaining_ids() {
    for (optional, kind) in [false, true].into_iter().flat_map(|optional| {
        [
            LineConstraintKind::Horizontal,
            LineConstraintKind::Distance(LineLengthMm::new(60.).expect("length")),
        ]
        .map(|kind| (optional, kind))
    }) {
        let (root, d, id) = fixture();
        let path = root.path().join("source.fcad");
        d.close().expect("close");
        let c = Connection::open(&path).expect("SQL");
        let bytes = b"complete attached source\0\xff";
        c.execute("INSERT INTO imported_sources VALUES(?1,'future.sketch-source',?2,?3,?4,'keep-created')",params![StableEntityId::new().to_bytes().as_slice(),bytes.as_slice(),ContentHash::of_bytes(bytes).as_bytes().as_slice(),bytes.len()]).expect("source");
        c.execute(
            "INSERT INTO imported_source_refs SELECT ?1,id FROM imported_sources",
            params![id.to_bytes().as_slice()],
        )
        .expect("claim");
        c.execute_batch("UPDATE capabilities SET rowid=101; INSERT INTO capabilities(rowid,name,required) VALUES(999,'optional.future',0); CREATE TABLE extra(k TEXT,v BLOB); INSERT INTO extra(rowid,k,v) VALUES(73,'keep',x'0022ff');").expect("sentinels");
        if optional {
            c.execute(
                "INSERT INTO capabilities(rowid,name,required) VALUES(501,?1,0)",
                [SKETCH_CONSTRAINTS_CAPABILITY],
            )
            .expect("optional constraint capability");
        }
        drop(c);
        let baseline = cells(&path);
        let mut d = Document::open(&path).expect("open");
        let original = stored(&d, id);
        let meta = d.meta().clone();
        let plan =
            prepare_sketch_constraints(&d, id, &add(original.curves[0].id, kind)).expect("plan");
        assert_eq!(plan.added.len(), 5);
        d.write_sketch_constraints(&plan).expect("write");
        assert_eq!(d.meta(), &meta);
        let valid = d.validate().expect("validate").is_ok();
        d.close().expect("close");
        let after = cells(&path);
        for (table, rows) in &baseline {
            if !["objects", "capabilities"].contains(&table.as_str()) {
                assert_eq!(&after[table], rows, "{table}");
            }
        }
        assert!(valid, "constraint payload and source claims validate");
        for row in &baseline["objects"] {
            let actual = after["objects"]
                .iter()
                .find(|r| r[1] == row[1])
                .expect("same ID");
            if row[1] != Value::Blob(id.to_bytes().to_vec()) {
                assert_eq!(actual, row);
            } else {
                for col in [0, 1, 2, 4, 5, 6] {
                    assert_eq!(actual[col], row[col]);
                }
            }
        }
        for row in &baseline["capabilities"] {
            let actual = after["capabilities"]
                .iter()
                .find(|r| r[1] == row[1])
                .expect("capability");
            if row[1] != Value::Text(SKETCH_CONSTRAINTS_CAPABILITY.into()) {
                assert_eq!(actual, row);
            } else {
                assert_eq!(actual[0], row[0]);
                assert_eq!(actual[2], Value::Integer(1));
            }
        }
        let mut d = Document::open(&path).expect("reopen");
        let now = stored(&d, id);
        assert_eq!(original.curves, now.curves);
        assert_eq!(now.constraints, plan.added);
        let raw: Vec<u8> = Connection::open(&path)
            .expect("SQL")
            .query_row(
                "SELECT payload FROM objects WHERE id=?1",
                [id.to_bytes().as_slice()],
                |r| r.get(0),
            )
            .expect("payload");
        let envelope = Envelope::from_bytes(&raw).expect("envelope");
        assert_eq!(envelope.schema_version, 2);
        assert!(
            envelope
                .required_capabilities
                .contains(&SKETCH_CONSTRAINTS_CAPABILITY.to_owned())
        );
        let h = now.constraints.last().expect("H").id;
        let removed = prepare_sketch_constraints(
            &d,
            id,
            &SketchConstraintEdits {
                remove: vec![h],
                add: vec![],
            },
        )
        .expect("remove exact");
        d.write_sketch_constraints(&removed).expect("remove");
        let closed = stored(&d, id);
        assert_eq!(closed.constraints, now.constraints[..4]);
        assert_eq!(closed.schema_version(), 2);
        assert!(
            ExtrudeEditSource::read(&d).expect("catalog").sketches[0]
                .refusal
                .is_some()
        );
        let v = prepare_sketch_constraints(
            &d,
            id,
            &add(original.curves[1].id, LineConstraintKind::Vertical),
        )
        .expect("add V");
        assert_eq!(v.added.len(), 1);
        d.write_sketch_constraints(&v).expect("write V");
        assert_eq!(&stored(&d, id).constraints[..4], &now.constraints[..4]);
    }
}
#[test]
fn ordered_requests_refuse_duplicates_foreign_ids_and_closure_removal_atomically() {
    let (root, mut d, id) = fixture();
    let curve = stored(&d, id).curves[0].id;
    let p = prepare_sketch_constraints(&d, id, &add(curve, LineConstraintKind::Horizontal))
        .expect("plan");
    d.write_sketch_constraints(&p).expect("write");
    let h = p.added.last().expect("h").id;
    let closure = p.added[0].id;
    for edits in [
        SketchConstraintEdits::default(),
        add(curve, LineConstraintKind::Horizontal),
        add(curve, LineConstraintKind::Vertical),
        add(StableEntityId::new(), LineConstraintKind::Horizontal),
        SketchConstraintEdits {
            remove: vec![h, h],
            add: vec![],
        },
        SketchConstraintEdits {
            remove: vec![closure],
            add: vec![],
        },
        SketchConstraintEdits {
            remove: vec![StableEntityId::new()],
            add: vec![],
        },
    ] {
        let bytes = std::fs::read(root.path().join("source.fcad")).expect("bytes");
        assert_eq!(
            prepare_sketch_constraints(&d, id, &edits)
                .expect_err("refused")
                .kind(),
            ErrorKind::Input
        );
        assert_eq!(
            bytes,
            std::fs::read(root.path().join("source.fcad")).expect("unchanged")
        );
    }
    let replace = SketchConstraintEdits {
        remove: vec![h],
        add: vec![AddLineConstraint {
            curve,
            kind: LineConstraintKind::Vertical,
        }],
    };
    let p = prepare_sketch_constraints(&d, id, &replace).expect("remove before add");
    assert_eq!(p.removed, vec![h]);
    assert_eq!(p.added.len(), 1);
    d.write_sketch_constraints(&p).expect("replace");
    assert!(matches!(
        stored(&d, id).constraints.last().expect("last").rule,
        SketchConstraintRule::Vertical { .. }
    ));
}
#[test]
fn existing_reversed_closure_is_reused_and_other_families_are_refused() {
    for unsupported in [false, true] {
        let (_root, mut d, id) = fixture();
        let mut o = d.object(id).expect("read").expect("object");
        let ObjectPayload::Sketch(s) = &mut o.payload else {
            panic!("sketch")
        };
        let curve = s.curves[0].id;
        let retained = StableEntityId::new();
        s.constraints.push(SketchConstraint {
            id: retained,
            rule: if unsupported {
                SketchConstraintRule::Fixed {
                    point: SketchPointRef::new(curve, SketchPointSelector::Start),
                    x: -20.,
                    y: -10.,
                }
            } else {
                SketchConstraintRule::Coincident {
                    a: SketchPointRef::new(s.curves[1].id, SketchPointSelector::Start),
                    b: SketchPointRef::new(curve, SketchPointSelector::End),
                }
            },
        });
        d.write(|w| w.put_object(o.id, o.parent, o.ordinal, o.name.as_deref(), &o.payload))
            .expect("fixture rule");
        let result =
            prepare_sketch_constraints(&d, id, &add(curve, LineConstraintKind::Horizontal));
        if unsupported {
            assert_eq!(
                result.expect_err("Fixed outside edit class").kind(),
                ErrorKind::Unsupported
            );
        } else {
            let p = result.expect("reuse");
            assert_eq!(p.added.len(), 4);
            d.write_sketch_constraints(&p).expect("write");
            assert_eq!(stored(&d, id).constraints[0].id, retained);
        }
    }
}

#[test]
fn line_length_is_checked_and_replacement_preserves_other_constraints() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1., -0., 0.] {
        assert_eq!(
            LineLengthMm::new(bad).expect_err("invalid length").kind(),
            ErrorKind::Input
        );
    }
    for good in [f64::MIN_POSITIVE, f64::from_bits(1), 30., f64::MAX] {
        assert_eq!(
            LineLengthMm::new(good).expect("finite positive").get(),
            good
        );
    }
    let (_root, mut d, id) = fixture();
    let original = stored(&d, id);
    let curve = original.curves[0].id;
    let length = |v| LineConstraintKind::Distance(LineLengthMm::new(v).expect("length"));
    let p = prepare_sketch_constraints(&d, id, &add(curve, length(60.)))
        .expect("first length adds closure");
    assert_eq!(p.added.len(), 5);
    assert!(
        matches!(p.added[4].rule, SketchConstraintRule::Distance { a, b, distance } if a.curve == curve && b.curve == curve && a.at == SketchPointSelector::Start && b.at == SketchPointSelector::End && distance == 60.)
    );
    d.write_sketch_constraints(&p).expect("length");
    let h = prepare_sketch_constraints(&d, id, &add(curve, LineConstraintKind::Horizontal))
        .expect("orientation alongside length");
    d.write_sketch_constraints(&h).expect("H");
    let before = stored(&d, id);
    for edits in [
        add(curve, length(60.)),
        add(curve, length(61.)),
        add(StableEntityId::new(), length(10.)),
        add(curve, LineConstraintKind::Vertical),
    ] {
        assert_eq!(
            prepare_sketch_constraints(&d, id, &edits)
                .expect_err("duplicate/foreign")
                .kind(),
            ErrorKind::Input
        );
        assert_eq!(stored(&d, id), before);
    }
    let old = p.added[4].id;
    let replacement = SketchConstraintEdits {
        remove: vec![old],
        add: vec![AddLineConstraint {
            curve,
            kind: length(55.),
        }],
    };
    let p = prepare_sketch_constraints(&d, id, &replacement).expect("atomic replacement");
    assert_eq!(p.removed, vec![old]);
    assert_eq!(p.added.len(), 1);
    assert_ne!(p.added[0].id, old);
    let proposed_id = p.added[0].id;
    d.write_sketch_constraints(&p).expect("replace");
    let after = stored(&d, id);
    assert_eq!(after.curves, original.curves);
    assert_eq!(after.constraints.last().expect("length").id, proposed_id);
    assert_eq!(
        after.constraints[..5],
        before
            .constraints
            .iter()
            .filter(|c| c.id != old)
            .copied()
            .collect::<Vec<_>>()
    );
    assert!(
        ExtrudeEditSource::read(&d)
            .expect("discovery")
            .constraint_sketches[0]
            .refusal
            .is_none()
    );
    let remove = prepare_sketch_constraints(
        &d,
        id,
        &SketchConstraintEdits {
            remove: vec![proposed_id, h.added[0].id],
            add: vec![],
        },
    )
    .expect("remove both");
    d.write_sketch_constraints(&remove).expect("remove");
    assert_eq!(stored(&d, id).constraints, before.constraints[..4]);
}

#[test]
fn arbitrary_distance_and_duplicate_lengths_refuse_whole_document() {
    for case in ["cross-line", "same-point", "duplicate", "missing-closure"] {
        let (_root, mut d, id) = fixture();
        let curve = stored(&d, id).curves[0].id;
        let p = prepare_sketch_constraints(
            &d,
            id,
            &add(
                curve,
                LineConstraintKind::Distance(LineLengthMm::new(60.).expect("length")),
            ),
        )
        .expect("prepare");
        d.write_sketch_constraints(&p).expect("store");
        let mut o = d.object(id).expect("object").expect("Sketch");
        let ObjectPayload::Sketch(s) = &mut o.payload else {
            panic!("Sketch")
        };
        let mut rule = s.constraints[4].rule;
        if let SketchConstraintRule::Distance { a, b, .. } = &mut rule {
            if case == "cross-line" {
                b.curve = s.curves[1].id;
            }
            if case == "same-point" {
                *b = *a;
            }
        }
        s.constraints[4].rule = rule;
        if case == "duplicate" {
            s.constraints.push(SketchConstraint {
                id: StableEntityId::new(),
                rule,
            });
        }
        if case == "missing-closure" {
            s.constraints.remove(0);
        }
        d.write(|w| w.put_object(o.id, o.parent, o.ordinal, o.name.as_deref(), &o.payload))
            .expect("foreign policy fixture");
        let before = stored(&d, id);
        let source = ExtrudeEditSource::read(&d).expect("discovery");
        assert!(source.constraint_sketches[0].stored.is_none(), "{case}");
        assert_eq!(
            prepare_sketch_constraints(&d, id, &add(curve, LineConstraintKind::Horizontal))
                .expect_err(case)
                .kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(stored(&d, id), before);
    }
}
