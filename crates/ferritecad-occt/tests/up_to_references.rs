// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_eval::{CacheOutcome, RebuildResult, rebuild_cached, rebuild_cold};
use ferritecad_fixtures::datum_planes::{circle, sweep_face};
use ferritecad_kernel::{GeometryKernel, NamingInput, OperationContext};
use ferritecad_occt::OcctKernel;
use ferritecad_types::ObjectId;

fn number(n: f64) -> Expression {
    Expression::constant(n).expect("finite")
}
fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
}
fn built(doc: &Document, kernel: &mut OcctKernel) -> RebuildResult {
    let result = rebuild_cold(doc, kernel, &OperationContext::default()).expect("rebuild");
    assert!(result.all_built(), "{:?}", result.report());
    for id in result.topology().features.keys() {
        let shape = result.shape(*id).expect("shape");
        assert!(kernel.is_valid(shape).expect("independent BRepCheck"));
        assert_eq!(
            result.topology().features[id].entities.len(),
            kernel
                .shape_topology(shape)
                .expect("topology")
                .adjacent_faces
                .len()
        );
    }
    result
}
fn warm(doc: &Document, kernel: &mut OcctKernel, root: &std::path::Path, cold: &RebuildResult) {
    let mut cache = CacheStore::open(
        root.join("cache.sqlite"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("cache");
    for pass in 0..2 {
        let (actual, events) =
            rebuild_cached(doc, kernel, &mut cache, &OperationContext::default()).expect("warm");
        assert!(actual.all_built());
        if pass == 1 {
            assert!(events.iter().all(|e| e.outcome == CacheOutcome::Hit));
        }
        for (id, names) in &cold.topology().features {
            let signature = |n: &ferritecad_topology::generative::FeatureNaming| {
                n.ancestry
                    .iter()
                    .map(|(name, rows)| (*name, rows.len()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(signature(names), signature(&actual.topology().features[id]));
            let a = kernel
                .shape_stats(cold.shape(*id).expect("cold"))
                .expect("volume")
                .1;
            let b = kernel
                .shape_stats(actual.shape(*id).expect("cache"))
                .expect("volume")
                .1;
            assert!((a - b).abs() < 1e-6);
        }
        actual.release_all(kernel);
    }
}

#[test]
fn a_second_cut_follows_the_first_floor_and_deleted_support_preserves_repairability() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let mut doc = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let f = ferritecad_fixtures::up_to_end::pockets(&mut doc, &mut kernel).expect("pockets");
    for depth in [4., 6.] {
        doc.write_changes(&[
            EditChange::constant(f.first, ExpressionSlot::ExtrudeDistance, depth).expect("depth"),
        ])
        .expect("change");
        let cold = built(&doc, &mut kernel);
        let volume = kernel
            .shape_stats(cold.shape(f.second).expect("second cut"))
            .expect("volume")
            .1;
        assert!((volume - (24000. - 2. * std::f64::consts::PI * 9. * depth)).abs() < 1e-6);
        warm(&doc, &mut kernel, root.path(), &cold);
        cold.release_all(&mut kernel);
    }
    let (plane, sketch, independent) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    doc.write_changes(&[
        EditChange::AddDatumPlane {
            object: plane,
            name: None,
            plane: DatumPlane::Face {
                result: f.second,
                face: f.face,
                offset_mm: number(0.),
            },
        },
        EditChange::AddSketch {
            object: sketch,
            name: None,
            sketch: circle(f.plane, [70., 20.], 2.).expect("independent profile"),
        },
        EditChange::AddBody {
            object: ObjectId::new(),
            name: None,
            feature_object: independent,
            feature_name: None,
            feature: FeatureAddition::Extrude {
                profile: sketch,
                regions: vec![],
                operation: SolidOperation::NewBody,
                reversed: false,
                end_condition: EndCondition::Blind {
                    distance: number(2.),
                },
            },
        },
        EditChange::SetEndCondition {
            object: f.first,
            end_condition: EndCondition::ThroughAll,
        },
    ])
    .expect("save broken model for repair");
    let path = doc.path().to_owned();
    doc.close().expect("close");
    let doc = Document::open_read_only(path).expect("broken model opens");
    let mut cache = CacheStore::open(
        root.path().join("broken-cache.sqlite"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("cache");
    for pass in 0..3 {
        let actual = if pass == 0 {
            rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("cold")
        } else {
            rebuild_cached(&doc, &mut kernel, &mut cache, &OperationContext::default())
                .expect("warm")
                .0
        };
        let report = actual.report();
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].id, f.second);
        assert_eq!(report.failed[0].kind, "topology");
        assert!(report.failed[0].message.contains("face deleted"));
        assert!(report.failed[0].message.contains(&f.face.id.to_string()));
        assert!(report.skipped.iter().any(|s| s.id == plane));
        assert!(actual.shape(independent).is_some());
        actual.release_all(&mut kernel);
    }
}

fn rectangle(plane: ObjectId, points: [[f64; 2]; 4]) -> Sketch {
    Sketch {
        plane,
        constraints: vec![],
        curves: (0..4)
            .map(|i| SketchCurve {
                id: ferritecad_types::StableEntityId::new(),
                construction: false,
                geometry: SketchGeometry::Line {
                    start: Point2::new(points[i][0], points[i][1]).expect("point"),
                    end: Point2::new(points[(i + 1) % 4][0], points[(i + 1) % 4][1])
                        .expect("point"),
                },
            })
            .collect(),
    }
}

#[test]
fn split_face_one_refuses_two_and_explicit_family_builds_an_independent_body() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let mut doc = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let body = ferritecad_fixtures::feature_changes::body(&doc).expect("body");
    let (base, _, profile) =
        ferritecad_fixtures::feature_changes::base_profile(&doc, body).expect("base");
    let before = built(&doc, &mut kernel);
    let top = sweep_face(&before, &mut kernel, base, NamingInput::Cap { end: true }).expect("top");
    let side = sweep_face(
        &before,
        &mut kernel,
        base,
        NamingInput::Curve(profile.curves[0].id),
    )
    .expect("side");
    before.release_all(&mut kernel);
    let (side_plane, slot_profile, slot) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    let (new_profile, new_feature) = (ObjectId::new(), ObjectId::new());
    doc.write_changes(&[
        EditChange::AddDatumPlane {
            object: side_plane,
            name: None,
            plane: DatumPlane::Face {
                result: base,
                face: side,
                offset_mm: number(0.),
            },
        },
        EditChange::AddSketch {
            object: slot_profile,
            name: None,
            sketch: rectangle(side_plane, [[28., 8.], [32., 8.], [32., 14.], [28., 14.]]),
        },
        EditChange::AddFeature {
            object: slot,
            body,
            name: None,
            feature: FeatureAddition::Extrude {
                profile: slot_profile,
                regions: vec![],
                operation: SolidOperation::Cut,
                reversed: true,
                end_condition: EndCondition::ThroughAll,
            },
        },
        EditChange::AddSketch {
            object: new_profile,
            name: None,
            sketch: circle(profile.plane, [75., 20.], 3.).expect("outside trimmed faces"),
        },
        EditChange::AddBody {
            object: ObjectId::new(),
            name: None,
            feature_object: new_feature,
            feature_name: None,
            feature: FeatureAddition::Extrude {
                profile: new_profile,
                regions: vec![],
                operation: SolidOperation::NewBody,
                reversed: false,
                end_condition: EndCondition::UpToFace {
                    result: slot,
                    face: top,
                    offset_mm: number(0.),
                },
            },
        },
    ])
    .expect("model");
    let mut cache = CacheStore::open(
        root.path().join("refused.sqlite"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("cache");
    for pass in 0..3 {
        let refused = if pass == 0 {
            rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("cold")
        } else {
            rebuild_cached(&doc, &mut kernel, &mut cache, &OperationContext::default())
                .expect("cache")
                .0
        };
        assert_eq!(refused.report().failed.len(), 1);
        let message = refused.report().failed[0].message.clone();
        assert!(message.contains("face split into 2 faces"), "{message}");
        refused.release_all(&mut kernel);
    }
    let mut family = top;
    family.meaning.selection = Selection::AllDescendants;
    doc.write_changes(&[EditChange::SetEndCondition {
        object: new_feature,
        end_condition: EndCondition::UpToFace {
            result: slot,
            face: family,
            offset_mm: number(0.),
        },
    }])
    .expect("explicit family");
    let cold = built(&doc, &mut kernel);
    let volume = kernel
        .shape_stats(cold.shape(new_feature).expect("new body"))
        .expect("stats")
        .1;
    assert!((volume - std::f64::consts::PI * 9. * 10.).abs() < 1e-6);
    warm(&doc, &mut kernel, root.path(), &cold);
    cold.release_all(&mut kernel);
}

#[test]
fn intersect_accepts_referenced_ends_and_vertex_builds_islands_but_refuses_behind() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let mut doc = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let f = ferritecad_fixtures::up_to_end::ceiling(&mut doc, &mut kernel).expect("ceiling");
    let body = ferritecad_fixtures::feature_changes::body(&doc).expect("body");
    let (_, _, base) =
        ferritecad_fixtures::feature_changes::base_profile(&doc, body).expect("base");
    let (sketch, feature) = (ObjectId::new(), ObjectId::new());
    doc.write_changes(&[
        EditChange::AddSketch {
            object: sketch,
            name: None,
            sketch: circle(base.plane, [45., 20.], 2.).expect("tool"),
        },
        EditChange::AddFeature {
            object: feature,
            body,
            name: None,
            feature: FeatureAddition::Extrude {
                profile: sketch,
                regions: vec![],
                operation: SolidOperation::Intersect,
                reversed: false,
                end_condition: f.face_end(0.).expect("end"),
            },
        },
    ])
    .expect("intersection");
    for end in [f.face_end(0.).expect("face"), f.vertex_end()] {
        doc.write_changes(&[EditChange::SetEndCondition {
            object: feature,
            end_condition: end,
        }])
        .expect("end");
        let cold = built(&doc, &mut kernel);
        let volume = kernel
            .shape_stats(cold.shape(feature).expect("intersection"))
            .expect("volume")
            .1;
        assert!((volume - std::f64::consts::PI * 4. * 10.).abs() < 1e-6);
        warm(&doc, &mut kernel, root.path(), &cold);
        cold.release_all(&mut kernel);
    }
    doc.write_changes(&[
        EditChange::constant(f.plane, ExpressionSlot::DatumOffset, -30.).expect("ceiling behind"),
    ])
    .expect("move");
    let result = rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("rebuild");
    assert_eq!(result.report().failed.len(), 1);
    assert_eq!(result.report().failed[0].kind, "input");
    assert!(result.report().failed[0].message.contains("not ahead"));
    result.release_all(&mut kernel);
    doc.write_changes(&[
        EditChange::constant(f.plane, ExpressionSlot::DatumOffset, 20.).expect("restore"),
    ])
    .expect("move");
    let (two, islands) = (ObjectId::new(), ObjectId::new());
    let mut profile = circle(base.plane, [70., 20.], 2.).expect("first island");
    profile.curves.extend(
        circle(base.plane, [80., 20.], 2.)
            .expect("second island")
            .curves,
    );
    doc.write_changes(&[
        EditChange::AddSketch {
            object: two,
            name: None,
            sketch: profile,
        },
        EditChange::AddBody {
            object: ObjectId::new(),
            name: None,
            feature_object: islands,
            feature_name: None,
            feature: FeatureAddition::Extrude {
                profile: two,
                regions: vec![],
                operation: SolidOperation::NewBody,
                reversed: false,
                end_condition: f.vertex_end(),
            },
        },
    ])
    .expect("two islands");
    let result = built(&doc, &mut kernel);
    let shape = result.shape(islands).expect("two islands");
    assert_eq!(kernel.solid_count(shape).expect("members"), 2);
    kernel.validate_solids(shape).expect("valid members");
    let expected = 2. * std::f64::consts::PI * 4. * 30.;
    assert!((kernel.shape_stats(shape).expect("volume").1 - expected).abs() < 1e-6);
    warm(&doc, &mut kernel, root.path(), &result);
    result.release_all(&mut kernel);
}
