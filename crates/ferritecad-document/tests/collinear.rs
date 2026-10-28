// SPDX-License-Identifier: MIT
//! §31G: the collinearity of two saved Lines, in the document.
//!
//! No solver and no kernel: this is whether the document can hold what the
//! constraint means — its slot, its layout and its capability — and refuse what
//! it must. The solving is measured in the solver, evaluator and command-line
//! gates.
#![allow(clippy::panic)]
#[path = "../src/test_changes.rs"]
mod changes;
use ferritecad_document::*;
use ferritecad_types::{ObjectId, StableEntityId, Transform};

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
fn edits(remove: Vec<StableEntityId>, add: Vec<AddSketchConstraint>) -> SketchConstraintEdits {
    SketchConstraintEdits { remove, add }
}
fn relation(a: StableEntityId, b: StableEntityId, relation: LineRelation) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::Relation { a, b, relation })
}
fn collinear(a: StableEntityId, b: StableEntityId) -> AddSketchConstraint {
    relation(a, b, LineRelation::Collinear)
}
fn write(d: &mut Document, id: ObjectId, e: &SketchConstraintEdits) -> Vec<EditChange> {
    let prepared = changes::constraint_changes(d, id, e).expect("prepared");
    d.write_changes(&prepared).expect("written");
    prepared
}
fn refusal(d: &Document, id: ObjectId, e: &SketchConstraintEdits) -> String {
    let error = changes::constraint_changes(d, id, e).expect_err("a structural refusal");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input, "{error}");
    error.to_string()
}

#[test]
fn collinear_replacement_keeps_the_other_constraint_ids() {
    let (_root, mut d, id, c) = fixture();
    let first = write(&mut d, id, &edits(vec![], vec![collinear(c[0], c[2])]));
    let collinear_id = changes::added_constraints(&first)
        .last()
        .expect("the collinear")
        .id;
    let kept = stored(&d, id);
    // Two different Lines, both of this Sketch.
    {
        let (name, add) = ("a foreign Line", collinear(c[1], StableEntityId::new()));
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
    assert!(
        matches!(replaced[0], EditChange::RemoveConstraint {constraint,..} if constraint == collinear_id)
    );
    let after = stored(&d, id);
    for k in kept.constraints.iter().filter(|k| k.id != collinear_id) {
        assert!(after.constraints.contains(k), "{k:?} lost its UUID or rule");
    }
    let parallel_id = changes::added_constraints(&replaced)
        .last()
        .expect("the parallel")
        .id;
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

    assert!(d.validate().expect("validate").is_ok());
}
