// SPDX-License-Identifier: MIT
//! The gate for durable sketch constraints in the document.
//!
//! What a constrained sketch has to survive: being stored, being read by a
//! build that predates constraints, being read by this one, and being handed
//! to an evaluator that cannot yet solve it. Nothing here calls a solver, and
//! nothing here checks a coordinate: this slice is about whether the document
//! can hold what a constrained sketch means.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    Access, CORE_CAPABILITY, DatumPlane, Dependency, DependencyRole, Document, Envelope,
    ObjectKind, ObjectPayload, Point2, SKETCH_CONSTRAINTS_CAPABILITY, Sketch, SketchConstraint,
    SketchConstraintRule, SketchCurve, SketchGeometry, SketchPointRef, SketchPointSelector,
    SketchSegmentRef,
};
use ferritecad_types::{ContentHash, ErrorKind, ObjectId, StableEntityId, Transform};
use tempfile::TempDir;

fn workspace() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("part.fcad");
    (dir, path)
}

fn line(start: (f64, f64), end: (f64, f64)) -> SketchCurve {
    SketchCurve {
        id: StableEntityId::new(),
        construction: false,
        geometry: SketchGeometry::Line {
            start: Point2::new(start.0, start.1).expect("finite"),
            end: Point2::new(end.0, end.1).expect("finite"),
        },
    }
}

fn point(at: (f64, f64)) -> SketchCurve {
    SketchCurve {
        id: StableEntityId::new(),
        construction: false,
        geometry: SketchGeometry::Point {
            at: Point2::new(at.0, at.1).expect("finite"),
        },
    }
}

fn circle(center: (f64, f64), radius: f64) -> SketchCurve {
    SketchCurve {
        id: StableEntityId::new(),
        construction: false,
        geometry: SketchGeometry::Circle {
            center: Point2::new(center.0, center.1).expect("finite"),
            radius,
        },
    }
}

fn arc(center: (f64, f64), radius: f64) -> SketchCurve {
    SketchCurve {
        id: StableEntityId::new(),
        construction: false,
        geometry: SketchGeometry::Arc {
            center: Point2::new(center.0, center.1).expect("finite"),
            radius,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
        },
    }
}

fn at(curve: &SketchCurve, selector: SketchPointSelector) -> SketchPointRef {
    SketchPointRef::new(curve.id, selector)
}

fn constrain(rule: SketchConstraintRule) -> SketchConstraint {
    SketchConstraint {
        id: StableEntityId::new(),
        rule,
    }
}

/// A square of four lines, and the eight constraints stated over it.
///
/// One of every family the document implements, so any check that walks them
/// walks all eight rather than the one that happened to be convenient.
struct Corpus {
    curves: Vec<SketchCurve>,
    constraints: Vec<SketchConstraint>,
}

fn corpus() -> Corpus {
    let bottom = line((0.0, 0.0), (10.0, 0.0));
    let right = line((10.0, 0.0), (10.0, 10.0));
    let top = line((10.0, 10.0), (0.0, 10.0));
    let left = line((0.0, 10.0), (0.0, 0.0));
    let origin = point((0.0, 0.0));

    let bottom_seg = SketchSegmentRef::new(
        at(&bottom, SketchPointSelector::Start),
        at(&bottom, SketchPointSelector::End),
    );
    let right_seg = SketchSegmentRef::new(
        at(&right, SketchPointSelector::Start),
        at(&right, SketchPointSelector::End),
    );
    let top_seg = SketchSegmentRef::new(
        at(&top, SketchPointSelector::Start),
        at(&top, SketchPointSelector::End),
    );

    let constraints = vec![
        constrain(SketchConstraintRule::Coincident {
            a: at(&bottom, SketchPointSelector::End),
            b: at(&right, SketchPointSelector::Start),
        }),
        constrain(SketchConstraintRule::Fixed {
            point: at(&origin, SketchPointSelector::At),
            x: 0.0,
            y: 0.0,
        }),
        constrain(SketchConstraintRule::Distance {
            a: at(&bottom, SketchPointSelector::Start),
            b: at(&bottom, SketchPointSelector::End),
            distance: 10.0,
        }),
        constrain(SketchConstraintRule::Horizontal {
            a: at(&bottom, SketchPointSelector::Start),
            b: at(&bottom, SketchPointSelector::End),
        }),
        constrain(SketchConstraintRule::Vertical {
            a: at(&right, SketchPointSelector::Start),
            b: at(&right, SketchPointSelector::End),
        }),
        constrain(SketchConstraintRule::EqualLength {
            a: bottom_seg,
            b: top_seg,
        }),
        constrain(SketchConstraintRule::Perpendicular {
            a: bottom_seg,
            b: right_seg,
        }),
        constrain(SketchConstraintRule::Parallel {
            a: bottom_seg,
            b: top_seg,
        }),
    ];

    Corpus {
        curves: vec![bottom, right, top, left, origin],
        constraints,
    }
}

/// Stores a plane and one sketch, and returns the sketch's id.
fn store(path: &std::path::Path, sketch: Sketch) -> ObjectId {
    let id = ObjectId::new();
    let plane = sketch.plane;
    let mut document = Document::create(path).expect("creates");
    document
        .write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("XY"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            )?;
            w.put_object(id, None, 1, Some("Profile"), &ObjectPayload::Sketch(sketch))?;
            w.add_dependency(Dependency {
                dependent: id,
                dependency: plane,
                role: DependencyRole::Plane,
            })
        })
        .expect("stores");
    document.close().expect("closes");
    id
}

fn sketch_of(document: &mut Document, id: ObjectId) -> Sketch {
    let record = document.object(id).expect("reads").expect("is there");
    match record.payload {
        ObjectPayload::Sketch(sketch) => sketch,
        other => panic!("expected a sketch, found {other:?}"),
    }
}

fn raw_envelope(path: &std::path::Path, id: ObjectId) -> Envelope {
    let conn = rusqlite::Connection::open(path).expect("opens raw");
    let bytes: Vec<u8> = conn
        .query_row(
            "SELECT payload FROM objects WHERE id = ?1",
            [id.to_bytes().as_slice()],
            |row| row.get(0),
        )
        .expect("reads");
    conn.close().expect("closes");
    Envelope::from_bytes(&bytes).expect("decodes")
}

/// Overwrites one object's envelope, keeping its hash consistent, so what the
/// gate exercises is a well-formed document and not detectable damage.
fn overwrite(path: &std::path::Path, id: ObjectId, bytes: &[u8], version: u32) {
    let conn = rusqlite::Connection::open(path).expect("opens raw");
    conn.execute(
        "UPDATE objects SET schema_version = ?1, payload = ?2, payload_hash = ?3 WHERE id = ?4",
        rusqlite::params![
            version,
            bytes,
            ContentHash::of_bytes(bytes).as_bytes().as_slice(),
            id.to_bytes().as_slice()
        ],
    )
    .expect("updates");
    conn.close().expect("closes");
}

// ---------------------------------------------------------------- round trip

#[test]
fn every_constraint_family_survives_save_load_and_save() {
    let (_dir, path) = workspace();
    let Corpus {
        curves,
        constraints,
    } = corpus();
    let original = Sketch {
        plane: ObjectId::new(),
        curves,
        constraints,
    };
    let id = store(&path, original.clone());

    let mut document = Document::open(&path).expect("opens");
    assert_eq!(
        document.access(),
        &Access::ReadWrite,
        "this build implements the capability it declares"
    );
    let read = sketch_of(&mut document, id);
    assert_eq!(
        read, original,
        "a constrained sketch changed on the way out"
    );
    assert_eq!(read.constraints.len(), 8, "one of every family is stored");

    // And again, from the payload this reader produced rather than the one the
    // writer did, so a lossy re-encode cannot hide behind a single cycle.
    document
        .write(|w| {
            w.put_object(
                id,
                None,
                1,
                Some("Profile"),
                &ObjectPayload::Sketch(read.clone()),
            )
        })
        .expect("rewrites");
    document.close().expect("closes");

    let mut document = Document::open(&path).expect("reopens");
    assert_eq!(sketch_of(&mut document, id), original);
}

#[test]
fn a_constraint_keeps_the_identity_it_was_given() {
    let (_dir, path) = workspace();
    let Corpus {
        curves,
        constraints,
    } = corpus();
    let ids: Vec<StableEntityId> = constraints.iter().map(|c| c.id).collect();
    let id = store(
        &path,
        Sketch {
            plane: ObjectId::new(),
            curves,
            constraints,
        },
    );

    let mut document = Document::open(&path).expect("opens");
    let read = sketch_of(&mut document, id);
    assert_eq!(
        read.constraints.iter().map(|c| c.id).collect::<Vec<_>>(),
        ids,
        "constraint identity is not something a round trip may re-issue"
    );
}

#[test]
fn reordering_curves_retargets_nothing() {
    let (_dir, path) = workspace();
    let Corpus {
        mut curves,
        constraints,
    } = corpus();
    let plane = ObjectId::new();
    let before = Sketch {
        plane,
        curves: curves.clone(),
        constraints: constraints.clone(),
    };
    curves.reverse();
    let after = Sketch {
        plane,
        curves,
        constraints,
    };

    // The curve list is in the opposite order and every constraint still names
    // exactly the same points. An index would have named four other ones.
    assert_ne!(before.curves, after.curves);
    let id = store(&path, after.clone());
    let mut document = Document::open(&path).expect("opens");
    let read = sketch_of(&mut document, id);

    for (stored, expected) in read.constraints.iter().zip(before.constraints.iter()) {
        assert_eq!(stored.id, expected.id);
        assert_eq!(stored.rule.points(), expected.rule.points());
    }
}

#[test]
fn reordering_constraints_changes_no_identity() {
    let (_dir, path) = workspace();
    let Corpus {
        curves,
        mut constraints,
    } = corpus();
    let plane = ObjectId::new();
    let forwards: Vec<StableEntityId> = constraints.iter().map(|c| c.id).collect();
    constraints.reverse();
    let backwards: Vec<StableEntityId> = constraints.iter().map(|c| c.id).collect();

    let id = store(
        &path,
        Sketch {
            plane,
            curves,
            constraints,
        },
    );
    let mut document = Document::open(&path).expect("opens");
    let read = sketch_of(&mut document, id);

    // Document order is given back as it was given, and it is not identity:
    // the same eight constraints come back under the same eight names.
    assert_eq!(
        read.constraints.iter().map(|c| c.id).collect::<Vec<_>>(),
        backwards
    );
    let mut sorted_read: Vec<StableEntityId> = backwards.clone();
    sorted_read.sort();
    let mut sorted_forwards = forwards;
    sorted_forwards.sort();
    assert_eq!(sorted_read, sorted_forwards);
}

#[test]
fn two_curves_at_the_same_place_stay_two_identities() {
    let (_dir, path) = workspace();
    // Same coordinates, drawn twice. Nothing about that makes them one point,
    // and only the Coincident says otherwise.
    let first = line((0.0, 0.0), (10.0, 0.0));
    let second = line((0.0, 0.0), (10.0, 0.0));
    assert_ne!(first.id, second.id);

    let coincident = constrain(SketchConstraintRule::Coincident {
        a: at(&first, SketchPointSelector::Start),
        b: at(&second, SketchPointSelector::Start),
    });
    let id = store(
        &path,
        Sketch {
            plane: ObjectId::new(),
            curves: vec![first.clone(), second.clone()],
            constraints: vec![coincident],
        },
    );

    let mut document = Document::open(&path).expect("opens");
    let read = sketch_of(&mut document, id);
    assert_eq!(read.curves.len(), 2, "identical coordinates were welded");
    let points = read.constraints[0].rule.points();
    assert_eq!(points.len(), 2);
    assert_ne!(
        points[0], points[1],
        "a coincidence between one point and itself says nothing"
    );
    assert_eq!(points[0].curve, first.id);
    assert_eq!(points[1].curve, second.id);
}

#[test]
fn a_constraint_on_construction_geometry_is_kept() {
    let (_dir, path) = workspace();
    let mut guide = line((0.0, 0.0), (10.0, 10.0));
    guide.construction = true;
    let edge = line((0.0, 0.0), (10.0, 0.0));

    let rule = SketchConstraintRule::Parallel {
        a: SketchSegmentRef::new(
            at(&guide, SketchPointSelector::Start),
            at(&guide, SketchPointSelector::End),
        ),
        b: SketchSegmentRef::new(
            at(&edge, SketchPointSelector::Start),
            at(&edge, SketchPointSelector::End),
        ),
    };
    let id = store(
        &path,
        Sketch {
            plane: ObjectId::new(),
            curves: vec![guide.clone(), edge],
            constraints: vec![constrain(rule)],
        },
    );

    let mut document = Document::open(&path).expect("opens");
    let read = sketch_of(&mut document, id);
    assert!(
        read.curves
            .iter()
            .any(|c| c.id == guide.id && c.construction),
        "the construction curve a constraint depends on disappeared"
    );
    assert_eq!(
        read.constraints.len(),
        1,
        "a constraint on construction geometry was dropped"
    );
}

// ---------------------------------------------------------------- validation

/// Every refusal below goes through the persistence boundary, because that is
/// where a bad constraint has to stop.
fn refuse(sketch: Sketch) -> ferritecad_types::CadError {
    ObjectPayload::Sketch(sketch)
        .to_storage_bytes()
        .expect_err("this sketch must not be storable")
}

#[test]
fn a_reference_to_no_curve_of_this_sketch_is_refused() {
    let bottom = line((0.0, 0.0), (10.0, 0.0));
    let deleted = line((10.0, 0.0), (10.0, 10.0));
    let rule = SketchConstraintRule::Coincident {
        a: at(&bottom, SketchPointSelector::End),
        b: at(&deleted, SketchPointSelector::Start),
    };
    // The curve is gone from the list and its constraint is still there. The
    // reference must dangle loudly rather than land on whatever now occupies
    // that position.
    let error = refuse(Sketch {
        plane: ObjectId::new(),
        curves: vec![bottom],
        constraints: vec![constrain(rule)],
    });
    assert_eq!(error.kind(), ErrorKind::Input);
    assert!(
        error.to_string().contains("not a curve of this sketch"),
        "{error}"
    );
}

#[test]
fn a_duplicate_curve_id_is_refused() {
    let bottom = line((0.0, 0.0), (10.0, 0.0));
    let mut twin = line((10.0, 0.0), (10.0, 10.0));
    twin.id = bottom.id;
    let error = refuse(Sketch {
        plane: ObjectId::new(),
        curves: vec![bottom, twin],
        constraints: Vec::new(),
    });
    assert_eq!(error.kind(), ErrorKind::Input);
    assert!(error.to_string().contains("duplicate curve id"), "{error}");
}

#[test]
fn a_duplicate_constraint_id_is_refused() {
    let bottom = line((0.0, 0.0), (10.0, 0.0));
    let one = constrain(SketchConstraintRule::Horizontal {
        a: at(&bottom, SketchPointSelector::Start),
        b: at(&bottom, SketchPointSelector::End),
    });
    let twin = SketchConstraint {
        id: one.id,
        rule: SketchConstraintRule::Distance {
            a: at(&bottom, SketchPointSelector::Start),
            b: at(&bottom, SketchPointSelector::End),
            distance: 10.0,
        },
    };
    let error = refuse(Sketch {
        plane: ObjectId::new(),
        curves: vec![bottom],
        constraints: vec![one, twin],
    });
    assert_eq!(error.kind(), ErrorKind::Input);
    assert!(
        error.to_string().contains("duplicate constraint id"),
        "{error}"
    );
}

#[test]
fn a_selector_the_geometry_does_not_have_is_refused() {
    // Every selector against every geometry it does not belong to, so no arm
    // of the table can be widened without a gate noticing.
    let cases: Vec<(SketchCurve, SketchPointSelector, &str)> = vec![
        (
            line((0.0, 0.0), (1.0, 0.0)),
            SketchPointSelector::At,
            "line",
        ),
        (point((0.0, 0.0)), SketchPointSelector::Start, "point"),
        (point((0.0, 0.0)), SketchPointSelector::End, "point"),
        (circle((0.0, 0.0), 5.0), SketchPointSelector::At, "circle"),
        (
            circle((0.0, 0.0), 5.0),
            SketchPointSelector::Start,
            "circle",
        ),
        (circle((0.0, 0.0), 5.0), SketchPointSelector::End, "circle"),
        (arc((0.0, 0.0), 5.0), SketchPointSelector::At, "arc"),
        (arc((0.0, 0.0), 5.0), SketchPointSelector::Start, "arc"),
        (arc((0.0, 0.0), 5.0), SketchPointSelector::End, "arc"),
    ];

    for (curve, selector, expected) in cases {
        let anchor = point((1.0, 1.0));
        let rule = SketchConstraintRule::Coincident {
            a: at(&curve, selector),
            b: at(&anchor, SketchPointSelector::At),
        };
        let error = refuse(Sketch {
            plane: ObjectId::new(),
            curves: vec![curve, anchor],
            constraints: vec![constrain(rule)],
        });
        assert_eq!(error.kind(), ErrorKind::Input);
        assert!(
            error.to_string().contains(expected) && error.to_string().contains("has no such point"),
            "{expected}/{}: {error}",
            selector.as_str()
        );
    }
}

#[test]
fn a_non_finite_parameter_is_refused() {
    let anchor = point((0.0, 0.0));
    let other = point((10.0, 0.0));
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for rule in [
            SketchConstraintRule::Fixed {
                point: at(&anchor, SketchPointSelector::At),
                x: bad,
                y: 0.0,
            },
            SketchConstraintRule::Fixed {
                point: at(&anchor, SketchPointSelector::At),
                x: 0.0,
                y: bad,
            },
            SketchConstraintRule::Distance {
                a: at(&anchor, SketchPointSelector::At),
                b: at(&other, SketchPointSelector::At),
                distance: bad,
            },
        ] {
            let error = refuse(Sketch {
                plane: ObjectId::new(),
                curves: vec![anchor.clone(), other.clone()],
                constraints: vec![constrain(rule)],
            });
            assert_eq!(error.kind(), ErrorKind::Input, "{bad} was accepted");
        }
    }
}

#[test]
fn a_distance_that_is_not_positive_is_refused() {
    let anchor = point((0.0, 0.0));
    let other = point((10.0, 0.0));
    for bad in [0.0, -1.0, -1e-9] {
        let rule = SketchConstraintRule::Distance {
            a: at(&anchor, SketchPointSelector::At),
            b: at(&other, SketchPointSelector::At),
            distance: bad,
        };
        let error = refuse(Sketch {
            plane: ObjectId::new(),
            curves: vec![anchor.clone(), other.clone()],
            constraints: vec![constrain(rule)],
        });
        assert_eq!(error.kind(), ErrorKind::Input, "{bad} was accepted");
        assert!(error.to_string().contains("must be positive"), "{error}");
    }
}

#[test]
fn a_segment_naming_one_point_twice_is_refused() {
    let edge = line((0.0, 0.0), (10.0, 0.0));
    let other = line((0.0, 1.0), (10.0, 1.0));
    let degenerate = SketchSegmentRef::new(
        at(&edge, SketchPointSelector::Start),
        at(&edge, SketchPointSelector::Start),
    );
    let sound = SketchSegmentRef::new(
        at(&other, SketchPointSelector::Start),
        at(&other, SketchPointSelector::End),
    );
    for rule in [
        SketchConstraintRule::EqualLength {
            a: degenerate,
            b: sound,
        },
        SketchConstraintRule::Perpendicular {
            a: sound,
            b: degenerate,
        },
        SketchConstraintRule::Parallel {
            a: degenerate,
            b: sound,
        },
    ] {
        let error = refuse(Sketch {
            plane: ObjectId::new(),
            curves: vec![edge.clone(), other.clone()],
            constraints: vec![constrain(rule)],
        });
        assert_eq!(error.kind(), ErrorKind::Input);
        assert!(error.to_string().contains("names"), "{error}");
    }
}

#[test]
fn a_point_family_naming_one_point_twice_is_stored_for_the_solver_to_judge() {
    // Deliberately the other half of the rule above. A distance from a point
    // to itself is impossible, and that is a conflict the solver exists to
    // report; a document that refused to hold it is one where nobody is ever
    // told which constraint is wrong.
    let anchor = point((0.0, 0.0));
    let rule = SketchConstraintRule::Distance {
        a: at(&anchor, SketchPointSelector::At),
        b: at(&anchor, SketchPointSelector::At),
        distance: 5.0,
    };
    ObjectPayload::Sketch(Sketch {
        plane: ObjectId::new(),
        curves: vec![anchor],
        constraints: vec![constrain(rule)],
    })
    .to_storage_bytes()
    .expect("an unsatisfiable constraint is a diagnosis, not a malformed document");
}

// --------------------------------------------------------- persistence policy

#[test]
fn an_unconstrained_sketch_stays_at_v1_and_declares_nothing_new() {
    let (_dir, path) = workspace();
    let Corpus { curves, .. } = corpus();
    let id = store(
        &path,
        Sketch {
            plane: ObjectId::new(),
            curves,
            constraints: Vec::new(),
        },
    );

    let envelope = raw_envelope(&path, id);
    assert_eq!(
        envelope.schema_version, 1,
        "an unconstrained sketch was stamped with a layout it does not use"
    );
    assert_eq!(envelope.required_capabilities, vec![CORE_CAPABILITY]);
}

#[test]
fn a_constrained_sketch_is_v2_and_declares_the_constraint_capability() {
    let (_dir, path) = workspace();
    let Corpus {
        curves,
        constraints,
    } = corpus();
    let id = store(
        &path,
        Sketch {
            plane: ObjectId::new(),
            curves,
            constraints,
        },
    );

    let envelope = raw_envelope(&path, id);
    assert_eq!(envelope.schema_version, 2);
    assert_eq!(
        envelope.required_capabilities,
        vec![CORE_CAPABILITY, SKETCH_CONSTRAINTS_CAPABILITY]
    );
}

#[test]
fn adding_a_constraint_to_a_stored_sketch_moves_it_from_v1_to_v2() {
    let (_dir, path) = workspace();
    let Corpus {
        curves,
        constraints,
    } = corpus();
    let id = store(
        &path,
        Sketch {
            plane: ObjectId::new(),
            curves,
            constraints: Vec::new(),
        },
    );
    assert_eq!(raw_envelope(&path, id).schema_version, 1);

    let mut document = Document::open(&path).expect("opens");
    let mut sketch = sketch_of(&mut document, id);
    sketch.constraints = constraints;
    document
        .write(|w| w.put_object(id, None, 1, Some("Profile"), &ObjectPayload::Sketch(sketch)))
        .expect("stores the constrained sketch");
    document.close().expect("closes");

    let envelope = raw_envelope(&path, id);
    assert_eq!(envelope.schema_version, 2);
    assert!(
        envelope
            .required_capabilities
            .iter()
            .any(|name| name == SKETCH_CONSTRAINTS_CAPABILITY)
    );
}

#[test]
fn a_v1_sketch_written_before_constraints_existed_still_reads() {
    let (_dir, path) = workspace();
    let Corpus { curves, .. } = corpus();
    let plane = ObjectId::new();
    let sketch = Sketch {
        plane,
        curves: curves.clone(),
        constraints: Vec::new(),
    };
    let id = store(&path, sketch.clone());

    // Byte for byte what a build that had never heard of constraints wrote:
    // the same layout, the same capability, and a payload with no room for a
    // constraint list at all.
    #[derive(serde::Serialize)]
    struct OldSketch {
        plane: ObjectId,
        curves: Vec<SketchCurve>,
    }
    let old = Envelope::encode(
        "sketch",
        1,
        vec![CORE_CAPABILITY.to_owned()],
        &OldSketch { plane, curves },
    )
    .expect("encodes")
    .to_bytes()
    .expect("serialises");
    assert_eq!(
        raw_envelope(&path, id).to_bytes().expect("serialises"),
        old,
        "this build no longer writes an unconstrained sketch the way it used to"
    );
    overwrite(&path, id, &old, 1);

    let mut document = Document::open(&path).expect("opens");
    assert_eq!(document.access(), &Access::ReadWrite);
    let read = sketch_of(&mut document, id);
    assert_eq!(read, sketch);
    assert!(read.constraints.is_empty());
}

#[test]
fn a_v1_header_over_a_payload_carrying_constraints_is_refused_both_ways() {
    let (_dir, path) = workspace();
    let Corpus {
        curves,
        constraints,
    } = corpus();
    let plane = ObjectId::new();
    let id = store(
        &path,
        Sketch {
            plane,
            curves: curves.clone(),
            constraints: constraints.clone(),
        },
    );

    // The dangerous lie: a header that tells an older build "nothing here you
    // cannot handle" over a payload full of meaning that build would drop.
    let smuggled = Envelope::encode(
        "sketch",
        1,
        vec![CORE_CAPABILITY.to_owned()],
        &Sketch {
            plane,
            curves,
            constraints,
        },
    )
    .expect("encodes")
    .to_bytes()
    .expect("serialises");
    overwrite(&path, id, &smuggled, 1);

    // Both roads: capability negotiation at open time, and the real read.
    let opening = Document::open(&path).expect_err("a lying header must not open writable");
    assert_eq!(opening.kind(), ErrorKind::Input);
    assert!(opening.to_string().contains("schema v2"), "{opening}");

    let reading = ObjectPayload::from_storage_bytes(&smuggled)
        .expect_err("a lying header must not decode either");
    assert_eq!(reading.kind(), ErrorKind::Input);
    assert!(reading.to_string().contains("schema v2"), "{reading}");
}

#[test]
fn a_v1_payload_declaring_the_constraint_capability_is_refused_both_ways() {
    let (_dir, path) = workspace();
    let Corpus { curves, .. } = corpus();
    let plane = ObjectId::new();
    let id = store(
        &path,
        Sketch {
            plane,
            curves: curves.clone(),
            constraints: Vec::new(),
        },
    );

    // The other half of the same dishonesty: an unconstrained sketch claiming
    // a contract it does not need, locking out readers for nothing.
    let overclaimed = Envelope::encode(
        "sketch",
        1,
        vec![
            CORE_CAPABILITY.to_owned(),
            SKETCH_CONSTRAINTS_CAPABILITY.to_owned(),
        ],
        &Sketch {
            plane,
            curves,
            constraints: Vec::new(),
        },
    )
    .expect("encodes")
    .to_bytes()
    .expect("serialises");
    overwrite(&path, id, &overclaimed, 1);

    let opening = Document::open(&path).expect_err("an over-declared sketch must be refused");
    assert_eq!(opening.kind(), ErrorKind::Input);
    assert!(
        opening.to_string().contains(SKETCH_CONSTRAINTS_CAPABILITY),
        "{opening}"
    );

    let reading = ObjectPayload::from_storage_bytes(&overclaimed)
        .expect_err("an over-declared sketch must not decode either");
    assert_eq!(reading.kind(), ErrorKind::Input);
}

#[test]
fn a_constrained_sketch_hiding_its_capability_is_refused_both_ways() {
    let (_dir, path) = workspace();
    let Corpus {
        curves,
        constraints,
    } = corpus();
    let plane = ObjectId::new();
    let id = store(
        &path,
        Sketch {
            plane,
            curves: curves.clone(),
            constraints: constraints.clone(),
        },
    );

    let hidden = Envelope::encode(
        "sketch",
        2,
        vec![CORE_CAPABILITY.to_owned()],
        &Sketch {
            plane,
            curves,
            constraints,
        },
    )
    .expect("encodes")
    .to_bytes()
    .expect("serialises");
    overwrite(&path, id, &hidden, 2);

    let opening = Document::open(&path).expect_err("an under-declared sketch must be refused");
    assert_eq!(opening.kind(), ErrorKind::Input);
    assert!(
        opening.to_string().contains(SKETCH_CONSTRAINTS_CAPABILITY),
        "{opening}"
    );

    let reading = ObjectPayload::from_storage_bytes(&hidden)
        .expect_err("an under-declared sketch must not decode either");
    assert_eq!(reading.kind(), ErrorKind::Input);
}

#[test]
fn a_constraint_family_this_build_does_not_know_is_kept_verbatim() {
    let (_dir, path) = workspace();
    let Corpus { curves, .. } = corpus();
    let plane = ObjectId::new();
    let id = store(
        &path,
        Sketch {
            plane,
            curves,
            constraints: Vec::new(),
        },
    );

    // How a later family arrives: under a capability of its own, carrying a
    // constraint kind whose tag this build has never seen. It must come back
    // out byte for byte, and it must not be read as one of the eight.
    let future = Envelope::new(
        "sketch",
        2,
        vec![
            CORE_CAPABILITY.to_owned(),
            "sketch.constraints.v2".to_owned(),
        ],
        vec![0xa1, 0x61, b'x', 0xf6],
    )
    .to_bytes()
    .expect("serialises");
    overwrite(&path, id, &future, 2);

    let document = Document::open(&path).expect("a future capability opens read-only");
    match document.access() {
        Access::ReadOnly { reason } => {
            assert!(reason.contains("sketch.constraints.v2"), "{reason}")
        }
        other => panic!("expected read-only access, got {other:?}"),
    }
    let record = document.object(id).expect("reads").expect("is there");
    match &record.payload {
        ObjectPayload::Unknown(_) => {}
        other => panic!("a future constraint family was interpreted as {other:?}"),
    }
    assert_eq!(
        record.payload.to_storage_bytes().expect("writes back"),
        future,
        "a payload this build cannot read must go back exactly as it came"
    );
}

#[test]
fn the_stored_bytes_change_when_a_constraint_changes() {
    let Corpus {
        curves,
        constraints,
    } = corpus();
    let plane = ObjectId::new();
    let base = Sketch {
        plane,
        curves,
        constraints,
    };

    let of = |sketch: &Sketch| {
        ContentHash::of_bytes(
            &ObjectPayload::Sketch(sketch.clone())
                .to_storage_bytes()
                .expect("writes"),
        )
    };
    let original = of(&base);
    assert_eq!(original, of(&base.clone()), "encoding is not deterministic");

    // A different distance is a different sketch.
    let mut changed = base.clone();
    changed.constraints[2].rule = match changed.constraints[2].rule {
        SketchConstraintRule::Distance { a, b, .. } => SketchConstraintRule::Distance {
            a,
            b,
            distance: 12.0,
        },
        other => panic!("the corpus moved: {other:?}"),
    };
    assert_ne!(
        original,
        of(&changed),
        "a changed constraint hashed the same"
    );

    // A dropped constraint is a different sketch.
    let mut fewer = base.clone();
    fewer.constraints.pop();
    assert_ne!(original, of(&fewer));

    // And a constraint whose identity changed is a different sketch, because
    // that identity is what a diagnosis will be reported against.
    let mut renamed = base.clone();
    renamed.constraints[0].id = StableEntityId::new();
    assert_ne!(original, of(&renamed));
}

#[test]
fn a_stored_sketch_hashes_the_same_in_a_second_session() {
    let (_dir, path) = workspace();
    let Corpus {
        curves,
        constraints,
    } = corpus();
    let sketch = Sketch {
        plane: ObjectId::new(),
        curves,
        constraints,
    };
    let id = store(&path, sketch.clone());

    let hash_of = |path: &std::path::Path| -> ContentHash {
        let conn = rusqlite::Connection::open(path).expect("opens raw");
        let bytes: Vec<u8> = conn
            .query_row(
                "SELECT payload_hash FROM objects WHERE id = ?1",
                [id.to_bytes().as_slice()],
                |row| row.get(0),
            )
            .expect("reads");
        conn.close().expect("closes");
        ContentHash::from_slice(&bytes).expect("is a hash")
    };
    let first = hash_of(&path);

    // A whole other session reads it and writes it straight back. Nothing that
    // happened in between - no ordering a solver would have imposed, no
    // numbering a session would have minted - may show up in the bytes.
    let mut document = Document::open(&path).expect("reopens");
    let read = sketch_of(&mut document, id);
    document
        .write(|w| w.put_object(id, None, 1, Some("Profile"), &ObjectPayload::Sketch(read)))
        .expect("rewrites");
    document.close().expect("closes");

    assert_eq!(
        first,
        hash_of(&path),
        "storing the same sketch twice produced different bytes"
    );
}

#[test]
fn a_constrained_sketch_passes_document_validation() {
    let (_dir, path) = workspace();
    let Corpus {
        curves,
        constraints,
    } = corpus();
    let id = store(
        &path,
        Sketch {
            plane: ObjectId::new(),
            curves,
            constraints,
        },
    );

    let document = Document::open(&path).expect("opens");
    let report = document.validate().expect("validates");
    assert!(
        report.is_ok(),
        "a well-formed constrained sketch was reported as broken: {:?}",
        report.diagnostics
    );
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.object == Some(id) && d.code == "object.unknown-type"),
        "a constrained sketch was not understood: {:?}",
        report.diagnostics
    );
}

#[test]
fn debug_output_names_nothing_a_solver_session_invented() {
    let Corpus {
        curves,
        constraints,
    } = corpus();
    let rendered = format!(
        "{:?}",
        Sketch {
            plane: ObjectId::new(),
            curves,
            constraints,
        }
    );

    for forbidden in [
        "PointId",
        "ConstraintId",
        "GCS",
        "Gcs",
        "planegcs",
        "0x",
        "handle",
        "Handle",
        "session",
        "Session",
        "tag",
        "Tag",
        "ptr",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "sketch Debug leaked {forbidden}: {rendered}"
        );
    }
}

#[test]
fn the_kind_and_the_payload_agree_about_what_a_sketch_declares() {
    // The two roads that must not drift: what a header says a layout requires,
    // and what a payload works out for itself.
    assert_eq!(
        ObjectKind::Sketch.required_capabilities(1),
        vec![CORE_CAPABILITY]
    );
    assert_eq!(
        ObjectKind::Sketch.required_capabilities(2),
        vec![CORE_CAPABILITY, SKETCH_CONSTRAINTS_CAPABILITY]
    );
    assert_eq!(ObjectKind::Sketch.readable_schema_versions(), &[2, 1]);

    let Corpus {
        curves,
        constraints,
    } = corpus();
    let unconstrained = Sketch {
        plane: ObjectId::new(),
        curves: curves.clone(),
        constraints: Vec::new(),
    };
    assert_eq!(unconstrained.schema_version(), 1);
    assert_eq!(unconstrained.required_capabilities(), vec![CORE_CAPABILITY]);

    let constrained = Sketch {
        plane: ObjectId::new(),
        curves,
        constraints,
    };
    assert_eq!(constrained.schema_version(), 2);
    assert_eq!(
        constrained.required_capabilities(),
        vec![CORE_CAPABILITY, SKETCH_CONSTRAINTS_CAPABILITY]
    );
}

/// Rebuilds one family from the points it names, in exactly the order
/// [`SketchConstraintRule::points`] names them.
///
/// So a gate can poison any one position of any one family, rather than the
/// position that happened to be convenient to write by hand.
fn rule_from(family: usize, p: &[SketchPointRef]) -> SketchConstraintRule {
    let seg = |i: usize| SketchSegmentRef::new(p[i], p[i + 1]);
    match family {
        0 => SketchConstraintRule::Coincident { a: p[0], b: p[1] },
        1 => SketchConstraintRule::Fixed {
            point: p[0],
            x: 1.0,
            y: 2.0,
        },
        2 => SketchConstraintRule::Distance {
            a: p[0],
            b: p[1],
            distance: 3.0,
        },
        3 => SketchConstraintRule::Horizontal { a: p[0], b: p[1] },
        4 => SketchConstraintRule::Vertical { a: p[0], b: p[1] },
        5 => SketchConstraintRule::EqualLength {
            a: seg(0),
            b: seg(2),
        },
        6 => SketchConstraintRule::Perpendicular {
            a: seg(0),
            b: seg(2),
        },
        7 => SketchConstraintRule::Parallel {
            a: seg(0),
            b: seg(2),
        },
        other => panic!("there are eight families, not {other}"),
    }
}

const ARITIES: [usize; 8] = [2, 1, 2, 2, 2, 4, 4, 4];

#[test]
fn every_position_of_every_family_is_checked() {
    // Checking the first reference of a constraint and trusting the rest is a
    // hole that a well-formed corpus never reveals: everything it holds is
    // valid in every position. So poison one position at a time, for every
    // position of every family, and insist each one is refused on its own.
    let carriers: Vec<SketchCurve> = (0..4)
        .map(|i| line((i as f64, 0.0), (i as f64, 5.0)))
        .collect();
    let absent = line((9.0, 9.0), (9.0, 10.0));
    let round = circle((20.0, 20.0), 2.0);

    for (family, arity) in ARITIES.iter().copied().enumerate() {
        let sound: Vec<SketchPointRef> = (0..arity)
            .map(|i| {
                at(
                    &carriers[i],
                    if i % 2 == 0 {
                        SketchPointSelector::Start
                    } else {
                        SketchPointSelector::End
                    },
                )
            })
            .collect();

        // The unpoisoned form must be storable, or a refusal below proves
        // nothing about the position that was poisoned.
        ObjectPayload::Sketch(Sketch {
            plane: ObjectId::new(),
            curves: carriers.clone(),
            constraints: vec![constrain(rule_from(family, &sound))],
        })
        .to_storage_bytes()
        .unwrap_or_else(|e| panic!("family {family} is not storable even when sound: {e}"));

        for position in 0..arity {
            for (what, poison) in [
                (
                    "a curve this sketch does not hold",
                    at(&absent, SketchPointSelector::Start),
                ),
                (
                    "a selector that geometry has not got",
                    at(&round, SketchPointSelector::Start),
                ),
            ] {
                let mut points = sound.clone();
                points[position] = poison;
                let error = refuse(Sketch {
                    plane: ObjectId::new(),
                    curves: {
                        let mut curves = carriers.clone();
                        curves.push(round.clone());
                        curves
                    },
                    constraints: vec![constrain(rule_from(family, &points))],
                });
                assert_eq!(
                    error.kind(),
                    ErrorKind::Input,
                    "family {family} position {position} accepted {what}"
                );
            }
        }
    }
}
