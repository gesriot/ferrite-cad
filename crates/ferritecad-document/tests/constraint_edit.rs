// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]

#[path = "../src/test_changes.rs"]
mod changes;
use ferritecad_document::*;
use ferritecad_types::{ErrorKind, ObjectId, StableEntityId, Transform};
use rusqlite::{Connection, types::Value};
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
                    regions: Vec::new(),
                    profile: id,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(10.)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    previous: None,
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
        add: vec![AddSketchConstraint::Line(AddLineConstraint::Line {
            curve,
            kind,
        })],
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
fn ordered_replacement_and_address_failures_are_atomic() {
    let (root, mut d, id) = fixture();
    let curve = stored(&d, id).curves[0].id;
    let initial = changes::constraint_changes(&d, id, &add(curve, LineConstraintKind::Horizontal))
        .expect("changes");
    d.write_changes(&initial).expect("write");
    let before = stored(&d, id);
    let horizontal = before.constraints.last().expect("horizontal").clone();
    for request in [
        vec![EditChange::RemoveConstraint {
            sketch: id,
            constraint: StableEntityId::new(),
        }],
        vec![EditChange::AddConstraint {
            sketch: id,
            constraint: horizontal.clone(),
        }],
        vec![
            EditChange::RemoveConstraint {
                sketch: id,
                constraint: horizontal.id,
            },
            EditChange::RemoveConstraint {
                sketch: id,
                constraint: horizontal.id,
            },
        ],
        vec![EditChange::AddConstraint {
            sketch: id,
            constraint: SketchConstraint {
                id: StableEntityId::new(),
                rule: SketchConstraintRule::Horizontal {
                    a: whole(StableEntityId::new()).from,
                    b: whole(StableEntityId::new()).to,
                },
            },
        }],
    ] {
        let rows = cells(&root.path().join("source.fcad"));
        assert!(d.write_changes(&request).is_err());
        assert_eq!(cells(&root.path().join("source.fcad")), rows);
    }
    let replacement = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Vertical {
            a: whole(curve).from,
            b: whole(curve).to,
        },
    };
    d.write_changes(&[
        EditChange::RemoveConstraint {
            sketch: id,
            constraint: horizontal.id,
        },
        EditChange::AddConstraint {
            sketch: id,
            constraint: replacement.clone(),
        },
    ])
    .expect("replace");
    let after = stored(&d, id);
    assert_eq!(after.curves, before.curves);
    assert_eq!(after.constraints[..4], before.constraints[..4]);
    assert_eq!(after.constraints.last(), Some(&replacement));
    // Closure constraints are ordinary records too; the rebuild decides whether a profile solves.
    d.write_changes(&[EditChange::RemoveConstraint {
        sketch: id,
        constraint: before.constraints[0].id,
    }])
    .expect("remove closure");
    assert!(
        !stored(&d, id)
            .constraints
            .iter()
            .any(|c| c.id == before.constraints[0].id)
    );
}

fn whole(curve: StableEntityId) -> SketchSegmentRef {
    SketchSegmentRef::new(
        SketchPointRef::new(curve, SketchPointSelector::Start),
        SketchPointRef::new(curve, SketchPointSelector::End),
    )
}

#[test]
fn every_line_form_translates_and_replaces_only_its_constraint() {
    let (_root, mut d, id) = fixture();
    let original = stored(&d, id);
    let a = original.curves[0].id;
    let b = original.curves[1].id;
    let line = |kind| AddSketchConstraint::Line(AddLineConstraint::Line { curve: a, kind });
    let pair = |relation| AddSketchConstraint::Line(AddLineConstraint::Relation { a, b, relation });
    let additions = [
        line(LineConstraintKind::Horizontal),
        line(LineConstraintKind::Vertical),
        line(LineConstraintKind::Distance(
            LineLengthMm::new(60.).expect("length"),
        )),
        line(LineConstraintKind::Fixed {
            at: LineEndpoint::End,
            x: SketchCoordinateMm::new(-20.).expect("x"),
            y: SketchCoordinateMm::new(30.).expect("y"),
        }),
        AddSketchConstraint::Line(AddLineConstraint::EqualLength { a, b }),
        pair(LineRelation::Parallel),
        pair(LineRelation::Perpendicular),
        pair(LineRelation::Collinear),
        AddSketchConstraint::Line(AddLineConstraint::DirectedAngle {
            a: DirectedLine::new(a, LineEndpoint::End, LineEndpoint::Start).expect("vector"),
            b: DirectedLine::new(b, LineEndpoint::Start, LineEndpoint::End).expect("vector"),
            angle: DirectedAngle::from_radians(-1.).expect("angle"),
        }),
    ];
    let mut previous = None;
    let mut closures = Vec::new();
    for addition in additions {
        let edits = SketchConstraintEdits {
            remove: previous.into_iter().collect(),
            add: vec![addition],
        };
        let request = changes::constraint_changes(&d, id, &edits).expect("translate");
        let rule = changes::added_constraints(&request)
            .last()
            .expect("new rule")
            .rule
            .clone();
        d.write_changes(&request).expect("write");
        let now = stored(&d, id);
        assert_eq!(now.curves, original.curves);
        assert_eq!(now.constraints.len(), 5);
        if closures.is_empty() {
            closures = now.constraints[..4].to_vec();
        }
        assert_eq!(now.constraints[..4], closures);
        let last = now.constraints.last().expect("rule");
        assert_eq!(last.rule, rule);
        assert_ne!(Some(last.id), previous);
        previous = Some(last.id);
    }
    let request = changes::constraint_changes(
        &d,
        id,
        &SketchConstraintEdits {
            remove: previous.into_iter().collect(),
            add: vec![],
        },
    )
    .expect("remove");
    d.write_changes(&request).expect("write");
    assert_eq!(stored(&d, id).constraints, closures);
}

#[test]
fn existing_reversed_closure_is_reused_without_an_editor_family_whitelist() {
    let (_root, mut d, id) = fixture();
    let sketch = stored(&d, id);
    let (a, b) = (sketch.curves[0].id, sketch.curves[1].id);
    let retained = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Coincident {
            a: SketchPointRef::new(b, SketchPointSelector::Start),
            b: SketchPointRef::new(a, SketchPointSelector::End),
        },
    };
    let relation = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Perpendicular {
            a: whole(a),
            b: whole(b),
        },
    };
    d.write_changes(&[
        EditChange::AddConstraint {
            sketch: id,
            constraint: retained.clone(),
        },
        EditChange::AddConstraint {
            sketch: id,
            constraint: relation.clone(),
        },
    ])
    .expect("stored constraints");
    let request = changes::constraint_changes(&d, id, &add(a, LineConstraintKind::Horizontal))
        .expect("translate");
    assert_eq!(changes::added_constraints(&request).len(), 4);
    d.write_changes(&request).expect("write");
    assert_eq!(stored(&d, id).constraints[..2], [retained, relation]);
}

#[test]
fn quantities_reject_nonfinite_values_and_lengths_require_positive_values() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1., -0., 0.] {
        assert_eq!(
            LineLengthMm::new(bad).expect_err("invalid length").kind(),
            ErrorKind::Input
        );
    }
    for good in [f64::MIN_POSITIVE, f64::from_bits(1), 30., f64::MAX] {
        assert_eq!(LineLengthMm::new(good).expect("length").get(), good);
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(SketchCoordinateMm::new(bad).is_err());
    }
    assert_eq!(
        SketchCoordinateMm::new(-0.).expect("zero"),
        SketchCoordinateMm::new(0.).expect("zero")
    );
}
#[test]
fn a_payload_this_build_reads_only_in_part_is_refused_instead_of_losing_the_rest() {
    let (root, d, id) = fixture();
    let path = root.path().join("source.fcad");
    let sketch = stored(&d, id);
    d.close().expect("close");
    let mut envelope = Envelope::from_bytes(
        &ObjectPayload::Sketch(sketch.clone())
            .to_storage_bytes()
            .expect("bytes"),
    )
    .expect("envelope");
    let mut value: ciborium::value::Value =
        ciborium::from_reader(envelope.payload.as_slice()).expect("CBOR");
    value
        .as_map_mut()
        .expect("map")
        .push(("future_detail".into(), "must not disappear".into()));
    envelope.payload.clear();
    ciborium::into_writer(&value, &mut envelope.payload).expect("CBOR");
    let bytes = envelope.to_bytes().expect("envelope");
    let raw = Connection::open(&path).expect("SQL");
    raw.execute(
        "UPDATE objects SET payload=?1,payload_hash=?2 WHERE id=?3",
        rusqlite::params![
            bytes,
            ferritecad_types::ContentHash::of_bytes(&bytes)
                .as_bytes()
                .as_slice(),
            id.to_bytes().as_slice()
        ],
    )
    .expect("future field");
    drop(raw);
    let rows = cells(&path);
    let mut d = Document::open(&path).expect("still readable");
    // The same geometry: only the unread field could change, so only it is at stake.
    let curve = &sketch.curves[0];
    let error = d
        .write_changes(&[EditChange::SetCurveGeometry {
            sketch: id,
            curve: curve.id,
            geometry: curve.geometry.clone(),
        }])
        .expect_err("must refuse loss");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(error.to_string().contains("losslessly"), "{error}");
    d.close().expect("close");
    assert_eq!(cells(&path), rows);
}

#[test]
fn a_symmetric_end_is_written_and_left_to_the_rebuild() {
    let (_root, mut d, profile) = fixture();
    let objects = d.objects().expect("objects");
    let find = |kind: fn(&ObjectPayload) -> bool| {
        objects.iter().find(|o| kind(&o.payload)).expect("saved").id
    };
    let extrude = find(|p| matches!(p, ObjectPayload::Extrude(_)));
    let body = find(|p| matches!(p, ObjectPayload::Body(_)));
    let symmetric = || EndCondition::Symmetric {
        distance: Expression::constant(6.).expect("distance"),
    };
    let cut = ObjectId::new();
    d.write_changes(&[
        EditChange::SetEndCondition {
            object: extrude,
            end_condition: symmetric(),
        },
        EditChange::AddFeature {
            object: cut,
            body,
            name: None,
            feature: FeatureAddition::Extrude {
                regions: Vec::new(),
                operation: ferritecad_document::SolidOperation::Cut,
                profile,
                end_condition: symmetric(),
                reversed: false,
            },
        },
    ])
    .expect("the document does not judge what the evaluator builds");
    for id in [extrude, cut] {
        let Some(ObjectPayload::Extrude(e)) = d.object(id).expect("read").map(|o| o.payload) else {
            panic!("extrude {id}");
        };
        assert_eq!(e.end_condition, symmetric());
    }
}

#[test]
fn end_changes_and_body_tip_moves_preserve_unread_payload_fields() {
    for body_tip in [false, true] {
        let (root, d, profile) = fixture();
        let path = root.path().join("source.fcad");
        let object = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| {
                if body_tip {
                    matches!(o.payload, ObjectPayload::Body(_))
                } else {
                    matches!(o.payload, ObjectPayload::Extrude(_))
                }
            })
            .expect("saved target");
        let changes = if body_tip {
            vec![EditChange::AddFeature {
                object: ObjectId::new(),
                body: object.id,
                name: None,
                feature: FeatureAddition::Extrude {
                    regions: Vec::new(),
                    operation: ferritecad_document::SolidOperation::Cut,
                    profile,
                    end_condition: EndCondition::ThroughAll,
                    reversed: false,
                },
            }]
        } else {
            vec![EditChange::SetEndCondition {
                object: object.id,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(12.).expect("distance"),
                },
            }]
        };
        let mut envelope = Envelope::from_bytes(&object.payload.to_storage_bytes().expect("bytes"))
            .expect("envelope");
        d.close().expect("close");
        let mut value: ciborium::value::Value =
            ciborium::from_reader(envelope.payload.as_slice()).expect("CBOR");
        let target = if body_tip {
            &mut value
        } else {
            &mut value
                .as_map_mut()
                .expect("map")
                .iter_mut()
                .find(|(key, _)| key.as_text() == Some("end_condition"))
                .expect("end condition")
                .1
        };
        target
            .as_map_mut()
            .expect("map")
            .push(("future_detail".into(), "preserve".into()));
        envelope.payload.clear();
        ciborium::into_writer(&value, &mut envelope.payload).expect("CBOR");
        let bytes = envelope.to_bytes().expect("envelope");
        let raw = Connection::open(&path).expect("SQL");
        raw.execute(
            "UPDATE objects SET payload=?1,payload_hash=?2 WHERE id=?3",
            rusqlite::params![
                bytes,
                ferritecad_types::ContentHash::of_bytes(&bytes)
                    .as_bytes()
                    .as_slice(),
                object.id.to_bytes().as_slice()
            ],
        )
        .expect("future field");
        drop(raw);
        let rows = cells(&path);
        let mut d = Document::open(&path).expect("readable");
        let error = d.write_changes(&changes).expect_err("lossless refusal");
        assert!(error.to_string().contains("losslessly"), "{error}");
        assert!(error.to_string().contains(&object.id.to_string()));
        d.close().expect("close");
        assert_eq!(cells(&path), rows);
    }
}
