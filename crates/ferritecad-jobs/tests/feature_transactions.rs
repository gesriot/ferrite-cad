// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_fixtures::feature_changes::{body, circular_tool, swept_edge};
use ferritecad_jobs::{EditRequest, edit_copy};
use ferritecad_kernel::OperationContext;
use ferritecad_types::ObjectId;
use std::path::Path;

fn blind(distance: f64) -> EndCondition {
    EndCondition::Blind {
        distance: Expression::constant(distance).expect("number"),
    }
}
fn version(d: &Document) -> DocumentVersion {
    DocumentVersion {
        document_id: d.meta().document_id,
        content: d.content_version().expect("content"),
    }
}
fn request(source: &Path, destination: &Path, changes: Vec<EditChange>) -> EditRequest {
    let d = Document::open_read_only(source).expect("source");
    EditRequest {
        source: source.into(),
        expected: version(&d),
        changes,
        destination: destination.into(),
    }
}
fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
}
fn verify(
    path: &Path,
) -> std::collections::BTreeMap<
    ObjectId,
    std::collections::BTreeSet<(String, ferritecad_types::ContentHash)>,
> {
    let d = Document::open_read_only(path).expect("document");
    assert!(d.validate().expect("validation").is_ok());
    model_names::names_with(&d, &mut ferritecad_occt::OcctKernel::new().expect("kernel"))
}
fn face_count(
    names: &std::collections::BTreeSet<(String, ferritecad_types::ContentHash)>,
) -> usize {
    names.iter().filter(|(kind, _)| kind == "face").count()
}
fn publish(req: &EditRequest) {
    let before = std::fs::read(&req.source).expect("source bytes");
    edit_copy(
        req,
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &OperationContext::default(),
    )
    .expect("accept");
    assert_eq!(std::fs::read(&req.source).expect("source bytes"), before);
    verify(&req.destination);
}
fn refuse(req: &EditRequest, guilty: ObjectId) {
    let before = std::fs::read(&req.source).expect("bytes");
    let e = edit_copy(
        req,
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &OperationContext::default(),
    )
    .expect_err("refuse");
    assert!(e.to_string().contains(&guilty.to_string()), "{e}");
    assert!(!req.destination.exists());
    assert_eq!(std::fs::read(&req.source).expect("bytes"), before);
}

#[test]
fn ordered_additions_dependencies_names_and_end_transitions() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let src = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&src).expect("plate");
    let d = Document::open_read_only(&src).expect("source");
    let b = body(&d).expect("body");
    let first = circular_tool(&d, b, [15., 15.], 2., EndCondition::ThroughAll).expect("first");
    let second = circular_tool(&d, b, [40., 20.], 3., blind(4.)).expect("second");
    let mut changes = first.changes;
    changes.extend(second.changes);
    let dest = root.path().join("cuts.fcad");
    publish(&request(&src, &dest, changes));
    let added = Document::open_read_only(&dest).expect("cuts");
    assert_eq!(
        added.object(b).expect("body").expect("row").payload,
        ObjectPayload::Body(Body {
            tip_feature: Some(second.feature)
        })
    );
    let refs = verify(&dest);
    assert_eq!(face_count(&refs[&first.feature]), 7);
    assert_eq!(face_count(&refs[&second.feature]), 9);
    assert!(
        !refs[&first.feature].is_disjoint(&refs[&second.feature]),
        "the second feature retains names from its predecessor"
    );
    assert!(added.dependencies().expect("deps").contains(&Dependency {
        dependent: second.feature,
        dependency: first.feature,
        role: DependencyRole::Predecessor
    }));
    let pocket = root.path().join("pocket.fcad");
    publish(&request(
        &dest,
        &pocket,
        vec![EditChange::SetEndCondition {
            object: first.feature,
            end_condition: blind(3.),
        }],
    ));
    let pocket_names = verify(&pocket);
    for feature in [first.feature, second.feature] {
        assert_eq!(
            face_count(&pocket_names[&feature]),
            face_count(&refs[&feature]) + 1
        );
    }
    let through = root.path().join("through-again.fcad");
    publish(&request(
        &pocket,
        &through,
        vec![EditChange::SetEndCondition {
            object: first.feature,
            end_condition: EndCondition::ThroughAll,
        }],
    ));
    assert_eq!(
        verify(&through),
        refs,
        "an unconsumed pocket floor may disappear"
    );
    // An entirely disjoint tool removes no material; exact contact is the kernel's decision.
    let outside =
        circular_tool(&d, b, [100., 100.], 2., EndCondition::ThroughAll).expect("outside tool");
    refuse(
        &request(&src, &root.path().join("outside.fcad"), outside.changes),
        outside.feature,
    );
}

#[test]
fn payload_errors_leave_all_rows_names_and_version_unchanged() {
    let root = tempfile::tempdir().expect("directory");
    let src = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&src).expect("plate");
    let mut d = Document::open(&src).expect("source");
    let b = body(&d).expect("body");
    let initial = (
        d.objects().expect("objects"),
        d.dependencies().expect("deps"),
        model_names::names(&d),
        version(&d),
    );
    let tool = circular_tool(&d, b, [15., 15.], 2., blind(3.)).expect("tool");
    for failure in [
        EditChange::AddSketch {
            object: b,
            name: None,
            sketch: Sketch {
                plane: ObjectId::new(),
                curves: vec![],
                constraints: vec![],
            },
        },
        EditChange::AddFeature {
            object: ObjectId::new(),
            body: ObjectId::new(),
            name: None,
            feature: FeatureAddition::Extrude {
                operation: ferritecad_document::SolidOperation::Cut,
                profile: tool.sketch,
                end_condition: blind(3.),
                reversed: false,
            },
        },
        EditChange::AddFeature {
            object: ObjectId::new(),
            body: b,
            name: None,
            feature: FeatureAddition::Extrude {
                operation: ferritecad_document::SolidOperation::Cut,
                profile: ObjectId::new(),
                end_condition: blind(3.),
                reversed: false,
            },
        },
    ] {
        let mut changes = tool.changes.clone();
        changes.push(failure);
        assert!(d.write_changes(&changes).is_err());
        assert_eq!(
            (
                d.objects().expect("objects"),
                d.dependencies().expect("deps"),
                model_names::names(&d),
                version(&d)
            ),
            initial
        );
    }
    // Both new rows must fit; a failure after reserving the first still writes nothing.
    let record = d.objects().expect("objects").remove(0);
    for ordinal in [i64::MAX, i64::MAX - 1, i64::MAX - 2] {
        d.write(|w| {
            w.put_object(
                record.id,
                record.parent,
                ordinal,
                record.name.as_deref(),
                &record.payload,
            )
            .map(|_| ())
        })
        .expect("last ordinal");
        let before = (
            d.objects().expect("objects"),
            d.dependencies().expect("deps"),
            model_names::names(&d),
            version(&d),
        );
        let result = d.write_changes(&tool.changes);
        if ordinal == i64::MAX - 2 {
            result.expect("two remaining slots");
            assert_eq!(
                d.object(tool.feature)
                    .expect("feature")
                    .expect("row")
                    .ordinal,
                i64::MAX
            );
        } else {
            let error = result.expect_err("ordinal exhausted");
            assert!(error.to_string().contains("ordinal"), "{error}");
            assert_eq!(
                (
                    d.objects().expect("objects"),
                    d.dependencies().expect("deps"),
                    model_names::names(&d),
                    version(&d)
                ),
                before
            );
        }
    }
}

#[test]
fn edge_additions_reconcile_names_after_mixed_geometry_changes() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    for chamfer in [false, true] {
        let src = root.path().join(format!("source-{chamfer}.fcad"));
        ferritecad_fixtures::write_plate(&src).expect("plate");
        let d = Document::open_read_only(&src).expect("source");
        let b = body(&d).expect("body");
        let feature = ObjectId::new();
        let edge = swept_edge(&d, b, 0).expect("edge");
        let addition = |value| EditChange::AddFeature {
            object: feature,
            body: b,
            name: Some("New edge feature".into()),
            feature: if chamfer {
                FeatureAddition::Chamfer {
                    edge,
                    distance_mm: Expression::constant(value).expect("number"),
                }
            } else {
                FeatureAddition::Fillet {
                    edge,
                    radius_mm: Expression::constant(value).expect("number"),
                }
            },
        };
        let (sketch_id, sketch) = ferritecad_fixtures::sketch(&d).expect("sketch");
        let vertices = sketch
            .curves
            .iter()
            .map(|c| {
                let SketchGeometry::Line { start, .. } = c.geometry else {
                    panic!("line")
                };
                SketchVertex {
                    curve_id: c.id,
                    start_mm: [if start.x > 0. { 70. } else { start.x }, start.y],
                }
            })
            .collect::<Vec<_>>();
        let mut changes = move_sketch_vertices(sketch_id, &sketch, &vertices).expect("move");
        changes.push(addition(2.));
        let dest = root.path().join(format!("edge-{chamfer}.fcad"));
        publish(&request(&src, &dest, changes));
        assert_eq!(face_count(&verify(&dest)[&feature]), 7);
        refuse(
            &request(
                &src,
                &root.path().join(format!("oversized-{chamfer}.fcad")),
                vec![addition(100.)],
            ),
            feature,
        );
        if !chamfer {
            let second = ObjectId::new();
            let edge = swept_edge(&d, b, 1).expect("adjacent edge");
            let next = root.path().join("two-fillets.fcad");
            publish(&request(
                &dest,
                &next,
                vec![EditChange::AddFeature {
                    object: second,
                    body: b,
                    name: None,
                    feature: FeatureAddition::Fillet {
                        edge,
                        radius_mm: Expression::constant(3.).expect("radius"),
                    },
                }],
            ));
            assert_eq!(face_count(&verify(&next)[&second]), 8);
        }
    }
}

#[test]
fn open_slots_overlapping_tools_and_constrained_tools_accept_following_edits() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    for case in ["slot", "overlap", "constraint"] {
        if case == "constraint"
            && !ferritecad_fixtures::native_dependencies(&[(
                ferritecad_sketch_solver::is_available(),
                "PLANEGCS",
            )])
        {
            continue;
        }
        let src = root.path().join(format!("{case}-source.fcad"));
        ferritecad_fixtures::write_plate(&src).expect("plate");
        let d = Document::open_read_only(&src).expect("source");
        let b = body(&d).expect("body");
        let center = if case == "slot" {
            [3., 20.]
        } else {
            [20., 20.]
        };
        let cut = circular_tool(&d, b, center, 2., EndCondition::ThroughAll).expect("tool");
        let mut changes = cut.changes.clone();
        if case == "slot" {
            let (id, sketch) = ferritecad_fixtures::sketch(&d).expect("sketch");
            let vertices = sketch
                .curves
                .iter()
                .map(|c| {
                    let SketchGeometry::Line { start, .. } = c.geometry else {
                        panic!("line")
                    };
                    SketchVertex {
                        curve_id: c.id,
                        start_mm: [if start.x == 0. { 2. } else { start.x }, start.y],
                    }
                })
                .collect::<Vec<_>>();
            changes.extend(move_sketch_vertices(id, &sketch, &vertices).expect("vertices"));
        } else if case == "overlap" {
            changes.extend(
                circular_tool(&d, b, [22., 20.], 2., EndCondition::ThroughAll)
                    .expect("overlap")
                    .changes,
            );
        } else {
            changes.push(EditChange::AddConstraint {
                sketch: cut.sketch,
                constraint: SketchConstraint {
                    id: ferritecad_types::StableEntityId::new(),
                    rule: SketchConstraintRule::Radius {
                        curve: cut.curve,
                        radius: Expression::constant(2.).expect("radius"),
                    },
                },
            });
        }
        let stage = root.path().join(format!("{case}-stage.fcad"));
        publish(&request(&src, &stage, changes));
        let stage_doc = Document::open_read_only(&stage).expect("stage");
        let mut changes = vec![
            EditChange::SetCurveGeometry {
                sketch: cut.sketch,
                curve: cut.curve,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(center[0], center[1]).expect("center"),
                    radius: 2.25,
                },
            },
            EditChange::SetEndCondition {
                object: cut.feature,
                end_condition: blind(4.),
            },
        ];
        changes.extend(
            circular_tool(&stage_doc, b, [45., 10.], 1., EndCondition::ThroughAll)
                .expect("additional tool")
                .changes,
        );
        publish(&request(
            &stage,
            &root.path().join(format!("{case}-edited.fcad")),
            changes,
        ));
    }
}

#[test]
fn additions_to_the_second_body_survive_session_undo_redo_and_save() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    for kind in ["cut", "fillet", "chamfer"] {
        let src = root.path().join(format!("{kind}.fcad"));
        let other = root.path().join(format!("{kind}-other.fcad"));
        ferritecad_fixtures::write_plate(&src).expect("plate");
        ferritecad_fixtures::write_plate(&other).expect("second plate");
        let extra = Document::open_read_only(&other).expect("second");
        let second = body(&extra).expect("second body");
        let mut d = Document::open(&src).expect("source");
        let first = body(&d).expect("first body");
        let first_record = d.object(first).expect("first");
        d.write(|w| {
            for o in extra.objects()? {
                w.put_object(
                    o.id,
                    o.parent,
                    o.ordinal + 100,
                    o.name.as_deref(),
                    &o.payload,
                )?;
            }
            for dep in extra.dependencies()? {
                w.add_dependency(dep)?;
            }

            Ok(())
        })
        .expect("two bodies");
        let changes = if kind == "cut" {
            circular_tool(&d, second, [20., 20.], 2., EndCondition::ThroughAll)
                .expect("tool")
                .changes
        } else {
            vec![
                ferritecad_fixtures::feature_changes::edge_addition(
                    &d,
                    second,
                    0,
                    2.,
                    kind == "chamfer",
                )
                .expect("edge"),
            ]
        };
        d.close().expect("close");
        let original = std::fs::read(&src).expect("bytes");
        let mut session = ferritecad_jobs::DocumentSession::open(&src).expect("session");
        let before = session.current().model();
        let expected = session.current().version();
        let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
        session
            .current()
            .load_scene(
                &mut k,
                |_, _| unreachable!("no imports"),
                &ferritecad_kernel::TessellationParams::default(),
                &OperationContext::default(),
            )
            .expect("shown baseline");
        let step = session
            .begin_step()
            .edit(
                changes,
                expected,
                &mut k,
                |_, _| unreachable!("no imports"),
                &OperationContext::default(),
            )
            .expect("step");
        session.commit_step(step).expect("commit");
        let names = verify(session.current().path());
        assert_eq!(
            Document::open_read_only(session.current().path())
                .expect("accepted")
                .object(first)
                .expect("first"),
            first_record
        );
        session
            .commit_move(session.begin_undo().expect("undo"))
            .expect("commit undo");
        assert_eq!(session.current().model(), before);
        session
            .commit_move(session.begin_redo().expect("redo"))
            .expect("commit redo");
        assert_eq!(verify(session.current().path()), names);
        assert_eq!(std::fs::read(&src).expect("unchanged disk"), original);
        let copy = root.path().join(format!("{kind}-saved.fcad"));
        Document::open_read_only(session.current().path())
            .expect("accepted")
            .snapshot_to(&copy)
            .expect("saved snapshot");
        assert_eq!(verify(&copy), names);
    }
}

#[test]
fn stored_history_integrity_rejects_forks_and_shared_bodies() {
    for case in ["forked-history", "shared-tip", "shared-history"] {
        let root = tempfile::tempdir().expect("directory");
        let src = root.path().join("plate.fcad");
        ferritecad_fixtures::write_plate(&src).expect("plate");
        let mut d = Document::open(&src).expect("document");
        let body = body(&d).expect("body");
        let base = ferritecad_fixtures::feature_changes::base_profile(&d, body)
            .expect("base")
            .0;
        let tool = circular_tool(&d, body, [15., 15.], 2., blind(3.)).expect("tool");
        d.write_changes(&tool.changes).expect("addition");
        assert!(d.validate().expect("validate").is_ok());
        let id = ObjectId::new();
        let payload = if case == "forked-history" {
            let ObjectPayload::Extrude(cut) = d
                .object(tool.feature)
                .expect("feature")
                .expect("row")
                .payload
            else {
                panic!("Cut")
            };
            ObjectPayload::Extrude(cut)
        } else {
            ObjectPayload::Body(Body {
                tip_feature: Some(if case == "shared-tip" {
                    tool.feature
                } else {
                    base
                }),
            })
        };
        d.write(|w| {
            w.put_object(id, None, 100, Some("Invalid branch"), &payload)?;
            for dependency in payload.dependencies(id) {
                w.add_dependency(dependency)?;
            }
            Ok(())
        })
        .expect("forged graph");
        let report = d.validate().expect("validate");
        assert!(
            report.errors().any(|d| d.code.ends_with(case)),
            "{case}: {report:?}"
        );
    }
}

#[path = "../../ferritecad-fixtures/tests/support/model_names.rs"]
mod model_names;
