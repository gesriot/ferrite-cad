// SPDX-License-Identifier: MIT
//! §31G: the collinearity of two saved Lines, in the document.
//!
//! No solver and no kernel: this is whether the document can hold what the
//! constraint means — its slot, its layout and its capability — and refuse what
//! it must. The solving is measured in the solver, evaluator and command-line
//! gates.
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_types::{ObjectId, StableEntityId, Transform};
use rusqlite::Connection;

/// A slanted four-Line frame, as the shipped editor reads one.
fn fixture() -> (tempfile::TempDir, Document, ObjectId, Vec<StableEntityId>) {
    let root = tempfile::tempdir().expect("dir");
    let mut d = Document::create(root.path().join("source.fcad")).expect("document");
    let [plane, id, extrude, body] = std::array::from_fn(|_| ObjectId::new());
    let points = [[-20., -10.], [40., -8.], [42., 30.], [-20., 30.]];
    let curves: Vec<SketchCurve> = points
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
    let curve_ids = curves.iter().map(|c| c.id).collect();
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
            w.put_object(oid, None, ordinal, Some("Profile"), &payload)?;
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
    (root, d, id, curve_ids)
}
fn stored(d: &Document, id: ObjectId) -> Sketch {
    let ObjectPayload::Sketch(s) = d.object(id).expect("read").expect("sketch").payload else {
        panic!("Sketch")
    };
    s
}
fn vector(curve: StableEntityId, from: LineEndpoint) -> DirectedLine {
    DirectedLine::new(curve, from, from.other()).expect("a vector")
}
fn angle(a: DirectedLine, b: DirectedLine, degrees: f64) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::DirectedAngle {
        a,
        b,
        angle: DirectedAngle::from_degrees(degrees).expect("a canonical angle"),
    })
}
fn edits(remove: Vec<StableEntityId>, add: Vec<AddSketchConstraint>) -> SketchConstraintEdits {
    SketchConstraintEdits { remove, add }
}
fn relation(a: StableEntityId, b: StableEntityId, relation: LineRelation) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::Relation { a, b, relation })
}
fn collinear(a: StableEntityId, b: StableEntityId) -> AddSketchConstraint {
    relation(a, b, LineRelation::Collinear)
}
fn write(d: &mut Document, id: ObjectId, e: &SketchConstraintEdits) -> PreparedSketchConstraints {
    let prepared = prepare_sketch_constraints(d, id, e).expect("prepared");
    d.write_sketch_constraints(&prepared).expect("written");
    prepared
}
fn refusal(d: &Document, id: ObjectId, e: &SketchConstraintEdits) -> String {
    let error = prepare_sketch_constraints(d, id, e).expect_err("a structural refusal");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input, "{error}");
    error.to_string()
}
fn segment(
    curve: StableEntityId,
    from: SketchPointSelector,
    to: SketchPointSelector,
) -> SketchSegmentRef {
    SketchSegmentRef::new(
        SketchPointRef::new(curve, from),
        SketchPointRef::new(curve, to),
    )
}

#[test]
fn a_collinear_is_stored_in_layout_v5_as_its_own_kind_and_declares_its_own_capability() {
    use SketchPointSelector::{End, Start};
    let (root, mut d, id, c) = fixture();
    let before = d.objects().expect("objects");
    // The two sides that do not touch: nothing about the pair is a joint.
    let prepared = write(&mut d, id, &edits(vec![], vec![collinear(c[2], c[0])]));
    let rule = prepared.added.last().expect("the collinear");
    let sketch = stored(&d, id);
    assert_eq!(sketch.schema_version(), 5);
    assert_eq!(
        sketch.required_capabilities(),
        vec![
            CORE_CAPABILITY.to_owned(),
            SKETCH_CONSTRAINTS_CAPABILITY.to_owned(),
            SKETCH_COLLINEAR_CONSTRAINTS_CAPABILITY.to_owned()
        ]
    );
    assert_eq!(
        sketch.constraints.last().map(|k| k.rule),
        Some(SketchConstraintRule::Collinear {
            a: segment(c[2], Start, End),
            b: segment(c[0], Start, End),
        }),
        "the Lines are named whole, in the order they were asked"
    );
    assert_eq!(rule.id, sketch.constraints.last().expect("stored").id);
    // Its own named kind on the wire: no old kind can carry it, and in
    // particular it is not a Parallel that an older build could rewrite.
    let bytes = ObjectPayload::Sketch(sketch.clone())
        .to_storage_bytes()
        .expect("encodes");
    let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    assert!(has(b"collinear"), "the rule is its own named kind");
    assert!(!has(b"parallel") && !has(b"directed_angle"));
    // The new layout and its capability are in the file's own table, required.
    d.close().expect("close");
    let connection = Connection::open(root.path().join("source.fcad")).expect("SQL");
    let required: i64 = connection
        .query_row(
            "SELECT required FROM capabilities WHERE name='sketch.constraints.collinear.v1'",
            [],
            |r| r.get(0),
        )
        .expect("the capability row");
    assert_eq!(required, 1);
    let version: i64 = connection
        .query_row(
            "SELECT schema_version FROM objects WHERE kind='sketch'",
            [],
            |r| r.get(0),
        )
        .expect("sketch row");
    assert_eq!(version, 5);
    // Only the Sketch changed; this build still opens it for writing.
    let d = Document::open(root.path().join("source.fcad")).expect("this build opens its own v5");
    assert!(d.access().is_writable(), "the new capability is supported");
    for object in before.iter().filter(|o| o.id != id) {
        assert_eq!(&d.object(object.id).expect("read").expect("kept"), object);
    }
    assert!(d.validate().expect("validate").is_ok());
    println!("\nFCAD_31G_DOCUMENT_V5_EXECUTED");
}

#[test]
fn a_collinear_shares_the_relation_slot_in_either_order_and_only_an_exact_replace_frees_it() {
    let (_root, mut d, id, c) = fixture();
    let forward = |i: usize| vector(c[i], LineEndpoint::Start);
    let first = write(&mut d, id, &edits(vec![], vec![collinear(c[0], c[2])]));
    let collinear_id = first.added.last().expect("the collinear").id;
    let kept = stored(&d, id);
    // The same pair under any answer to the same question, in either order.
    for (name, add) in [
        ("again", collinear(c[0], c[2])),
        ("swapped", collinear(c[2], c[0])),
        ("parallel", relation(c[0], c[2], LineRelation::Parallel)),
        (
            "parallel swapped",
            relation(c[2], c[0], LineRelation::Parallel),
        ),
        (
            "perpendicular",
            relation(c[0], c[2], LineRelation::Perpendicular),
        ),
        ("directed angle", angle(forward(0), forward(2), 0.)),
        (
            "directed angle swapped",
            angle(forward(2), forward(0), 180.),
        ),
    ] {
        let message = refusal(&d, id, &edits(vec![], vec![add]));
        assert!(
            message.contains("Collinear"),
            "{name}: the refusal does not name the collinearity: {message}"
        );
        assert_eq!(
            stored(&d, id),
            kept,
            "{name}: a refusal changed the document"
        );
    }
    // Two different Lines, both of this Sketch.
    for (name, add) in [
        ("one Line twice", collinear(c[1], c[1])),
        ("a foreign Line", collinear(c[1], StableEntityId::new())),
    ] {
        refusal(&d, id, &edits(vec![], vec![add]));
        assert_eq!(stored(&d, id), kept, "{name}");
    }
    // Another pair is another slot, and so is an equal length on this one: a
    // different property of the same two Lines.
    write(
        &mut d,
        id,
        &edits(
            vec![],
            vec![
                collinear(c[1], c[3]),
                AddSketchConstraint::Line(AddLineConstraint::EqualLength { a: c[0], b: c[2] }),
            ],
        ),
    );
    // Exact removal plus a different answer on the pair, in one request, and
    // back again: a Parallel replaces the collinearity and the collinearity
    // replaces the Parallel.
    let replaced = write(
        &mut d,
        id,
        &edits(
            vec![collinear_id],
            vec![relation(c[2], c[0], LineRelation::Parallel)],
        ),
    );
    assert_eq!(replaced.removed, vec![collinear_id]);
    let after = stored(&d, id);
    for k in kept.constraints.iter().filter(|k| k.id != collinear_id) {
        assert!(after.constraints.contains(k), "{k:?} lost its UUID or rule");
    }
    let parallel_id = replaced.added.last().expect("the parallel").id;
    write(
        &mut d,
        id,
        &edits(vec![parallel_id], vec![collinear(c[0], c[2])]),
    );
    // Removing the last collinearities gives back the layout every older build
    // writes: an EqualLength and the closure links only.
    let ids: Vec<_> = stored(&d, id)
        .constraints
        .iter()
        .filter(|k| matches!(k.rule, SketchConstraintRule::Collinear { .. }))
        .map(|k| k.id)
        .collect();
    assert_eq!(ids.len(), 2);
    write(&mut d, id, &edits(ids, vec![]));
    assert_eq!(
        stored(&d, id).schema_version(),
        2,
        "EqualLength + closure only"
    );
    assert!(d.validate().expect("validate").is_ok());
    println!("\nFCAD_31G_DOCUMENT_SLOT_EXECUTED");
}

#[test]
fn what_a_sketch_declares_is_decided_by_what_it_holds_and_collinear_is_never_an_older_kind() {
    use SketchPointSelector::{End, Start};
    let (_root, mut d, id, c) = fixture();
    // The older families stay where they were and never name the new capability.
    write(
        &mut d,
        id,
        &edits(
            vec![],
            vec![
                relation(c[0], c[2], LineRelation::Parallel),
                angle(
                    vector(c[0], LineEndpoint::Start),
                    vector(c[1], LineEndpoint::Start),
                    30.,
                ),
            ],
        ),
    );
    let angled = stored(&d, id);
    assert_eq!(angled.schema_version(), 4);
    assert_eq!(
        angled.required_capabilities(),
        vec![
            CORE_CAPABILITY.to_owned(),
            SKETCH_CONSTRAINTS_CAPABILITY.to_owned(),
            SKETCH_ANGLE_CONSTRAINTS_CAPABILITY.to_owned()
        ]
    );
    // A stored collinearity moves the layout to v5 whatever else it holds, and
    // v5 is cumulative: it needs the collinear capability and no more.
    let mut with_collinear = angled.clone();
    with_collinear.constraints.push(SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Collinear {
            a: segment(c[1], Start, End),
            b: segment(c[3], Start, End),
        },
    });
    assert_eq!(with_collinear.schema_version(), 5);
    assert_eq!(
        with_collinear.required_capabilities(),
        ObjectKind::Sketch.required_capabilities(5)
    );
    let bytes = ObjectPayload::Sketch(with_collinear.clone())
        .to_storage_bytes()
        .expect("encodes");
    assert_eq!(
        ObjectPayload::from_storage_bytes(&bytes).expect("decodes"),
        ObjectPayload::Sketch(with_collinear.clone())
    );
    assert!(ObjectKind::Sketch.readable_schema_versions().contains(&5));
    // A header that misdescribes the payload is refused in every direction: a
    // v2 or v4 header over a collinearity would let a build that cannot see it
    // rewrite the sketch, and the v5 header over a sketch without one locks
    // readers out for nothing.
    for older in [1, 2, 3, 4] {
        let lying = Envelope::encode(
            "sketch".to_owned(),
            older,
            ObjectKind::Sketch.required_capabilities(older),
            &with_collinear,
        )
        .expect("envelope")
        .to_bytes()
        .expect("bytes");
        assert!(
            ObjectPayload::from_storage_bytes(&lying).is_err(),
            "v{older}"
        );
    }
    let hoarding = Envelope::encode(
        "sketch".to_owned(),
        5,
        ObjectKind::Sketch.required_capabilities(5),
        &angled,
    )
    .expect("envelope")
    .to_bytes()
    .expect("bytes");
    assert!(ObjectPayload::from_storage_bytes(&hoarding).is_err());
    // A segment runs between two points: a collinearity over a segment that
    // names one point twice is refused where it is stored.
    for (a, b) in [
        (segment(c[1], Start, Start), segment(c[3], Start, End)),
        (segment(c[1], Start, End), segment(c[3], End, End)),
    ] {
        let mut zero = angled.clone();
        zero.constraints.push(SketchConstraint {
            id: StableEntityId::new(),
            rule: SketchConstraintRule::Collinear { a, b },
        });
        assert!(ObjectPayload::Sketch(zero).to_storage_bytes().is_err());
    }
    println!("\nFCAD_31G_DOCUMENT_LAYOUT_EXECUTED");
}

#[test]
fn a_profile_with_a_construction_line_takes_no_collinear_on_any_pair() {
    let (_root, mut d, id, c) = fixture();
    let mut object = d.object(id).expect("read").expect("sketch");
    if let ObjectPayload::Sketch(s) = &mut object.payload {
        s.curves[1].construction = true;
    }
    d.write(|w| {
        w.put_object(
            object.id,
            object.parent,
            object.ordinal,
            object.name.as_deref(),
            &object.payload,
        )
    })
    .expect("write");
    let before = stored(&d, id);
    // The construction Line itself and the pair that leaves it out are refused
    // alike: the editor takes a profile of real Lines or none.
    for (a, b) in [(0, 2), (1, 3), (2, 1)] {
        let error = prepare_sketch_constraints(&d, id, &edits(vec![], vec![collinear(c[a], c[b])]))
            .expect_err("a construction Line in the profile");
        assert_eq!(
            error.kind(),
            ferritecad_types::ErrorKind::Unsupported,
            "{error}"
        );
        assert!(error.to_string().contains("construction"), "{error}");
    }
    assert_eq!(stored(&d, id), before);
    println!("\nFCAD_31G_DOCUMENT_CONSTRUCTION_EXECUTED");
}
