// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_eval::{CacheOutcome, RebuildResult, rebuild_cached, rebuild_cold};
use ferritecad_fixtures::datum_planes::{boss, circle, sweep_face};
use ferritecad_kernel::{GeometryKernel, NamingInput, OperationContext, SubShapeKind};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{ObjectId, Vec3};

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
}
fn number(value: f64) -> Expression {
    Expression::constant(value).expect("finite")
}
fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
}
fn plate() -> (tempfile::TempDir, Document, OcctKernel) {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let doc = Document::open(path).expect("document");
    (root, doc, OcctKernel::new().expect("kernel"))
}
fn build(doc: &Document, kernel: &mut OcctKernel) -> RebuildResult {
    let built = rebuild_cold(doc, kernel, &OperationContext::default()).expect("rebuild");
    assert!(built.all_built(), "{:?}", built.report());
    for (id, names) in &built.topology().features {
        let shape = built.shape(*id).expect("feature shape");
        assert!(kernel.is_valid(shape).expect("independent BRepCheck"));
        assert_eq!(
            names.entities.len(),
            kernel
                .shape_topology(shape)
                .expect("topology")
                .adjacent_faces
                .len()
        );
    }
    built
}
type NameSnapshot = Vec<(ObjectId, Vec<(String, String, usize)>)>;

fn names(built: &RebuildResult) -> NameSnapshot {
    built
        .topology()
        .features
        .iter()
        .map(|(id, f)| {
            (
                *id,
                f.ancestry
                    .iter()
                    .map(|(n, s)| (n.kind.to_string(), n.digest.to_string(), s.len()))
                    .collect(),
            )
        })
        .collect()
}
fn warm(doc: &Document, kernel: &mut OcctKernel, cold: &RebuildResult, root: &std::path::Path) {
    let mut cache = CacheStore::open(
        root.join("cache.sqlite"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("cache");
    for pass in 0..2 {
        let (built, events) = rebuild_cached(doc, kernel, &mut cache, &OperationContext::default())
            .expect("cached rebuild");
        assert!(built.all_built(), "{:?}", built.report());
        assert_eq!(names(cold), names(&built));
        if pass == 1 {
            assert!(events.iter().all(|e| e.outcome == CacheOutcome::Hit));
        }
        for id in cold.order() {
            assert_eq!(cold.plane(*id), built.plane(*id));
            if let Some(a) = cold.shape(*id) {
                let b = built.shape(*id).expect("restored shape");
                assert!(kernel.is_valid(b).expect("restored BRepCheck"));
                near(
                    kernel.shape_stats(a).expect("cold stats").1,
                    kernel.shape_stats(b).expect("warm stats").1,
                );
            }
        }
        built.release_all(kernel);
    }
}

#[test]
fn six_box_faces_point_outwards_and_use_the_documented_frame() {
    if !native() {
        return;
    }
    let (_root, doc, mut kernel) = plate();
    let built = build(&doc, &mut kernel);
    let feature = *built.topology().features.keys().next().expect("extrude");
    let shape = built.shape(feature).expect("shape");
    let faces: Vec<_> = kernel
        .shape_topology(shape)
        .expect("faces")
        .entities(SubShapeKind::Face)
        .collect();
    assert_eq!(faces.len(), 6);
    for face in &faces {
        let result = kernel
            .support_plane(&[*face], &OperationContext::default())
            .expect("plane");
        let p = result.point();
        let n = result.normal();
        assert!(
            Vec3::new(p.x - 30., p.y - 20., p.z - 5.)
                .expect("vector")
                .dot(n)
                > 0.
        );
        let frame = result.sketch_plane(Default::default()).expect("frame");
        let origin = frame.origin();
        near(
            Vec3::new(origin.x - p.x, origin.y - p.y, origin.z - p.z)
                .expect("vector")
                .dot(n),
            0.,
        );
        near(frame.x_axis().dot(n), 0.);
        assert_eq!(
            frame.x_axis(),
            if n.x.abs() == 1. { Vec3::Y } else { Vec3::X }
        );
        near(origin.x * n.y - origin.y * n.x, 0.);
        near(origin.x * n.z - origin.z * n.x, 0.);
    }
    let refusal = kernel
        .support_plane(&faces, &OperationContext::default())
        .expect_err("six planes");
    assert_eq!(refusal.kind(), ferritecad_types::ErrorKind::Input);
    assert!(refusal.to_string().contains("do not lie in one plane"));
    built.release_all(&mut kernel);
}

#[test]
fn supported_boss_moves_with_the_plate_without_changing_names_and_offsets_make_a_new_body() {
    if !native() {
        return;
    }
    let (root, mut doc, mut kernel) = plate();
    let f = boss(&doc, &mut kernel).expect("fixture");
    doc.write_changes(&f.changes).expect("one addition");
    let before = build(&doc, &mut kernel);
    near(before.plane(f.plane).expect("plane").origin().z, 10.);
    near(
        kernel
            .shape_stats(before.shape(f.feature).expect("boss"))
            .expect("volume")
            .1,
        24000. + std::f64::consts::PI * 9. * 4.,
    );
    warm(&doc, &mut kernel, &before, root.path());
    doc.write_changes(&[
        EditChange::constant(f.base, ExpressionSlot::ExtrudeDistance, 15.).expect("height"),
    ])
    .expect("edit");
    let after = build(&doc, &mut kernel);
    near(after.plane(f.plane).expect("plane").origin().z, 15.);
    assert_eq!(names(&before), names(&after));
    let ObjectPayload::Extrude(extrude) = doc
        .object(f.feature)
        .expect("object")
        .expect("boss")
        .payload
    else {
        panic!("extrude");
    };
    let key = |built: &RebuildResult| {
        let request = ferritecad_eval::extrude_request(
            &extrude,
            built.profile(f.sketch).expect("profile").clone(),
            None,
        )
        .expect("request");
        ferritecad_eval::extrude_archive_key(
            kernel.identity(),
            &request,
            &OperationContext::default(),
        )
    };
    assert_ne!(key(&before), key(&after));
    warm(&doc, &mut kernel, &after, root.path());
    before.release_all(&mut kernel);
    after.release_all(&mut kernel);
    let (plane, sketch, feature) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
    doc.write_changes(&[
        EditChange::AddDatumPlane {
            object: plane,
            name: None,
            plane: DatumPlane::Offset {
                from: f.plane,
                distance_mm: number(10.),
            },
        },
        EditChange::AddSketch {
            object: sketch,
            name: None,
            sketch: circle(plane, [25., 20.], 2.).expect("circle"),
        },
        EditChange::AddBody {
            object: ObjectId::new(),
            name: None,
            feature_object: feature,
            feature_name: None,
            feature: FeatureAddition::Extrude {
                profile: sketch,
                regions: vec![],
                operation: SolidOperation::NewBody,
                reversed: false,
                end_condition: EndCondition::Blind {
                    distance: number(3.),
                },
            },
        },
    ])
    .expect("offset and body");
    let built = build(&doc, &mut kernel);
    near(built.plane(plane).expect("offset").origin().z, 25.);
    let bounds = kernel
        .projected_bounds(built.shape(feature).expect("second body"), Vec3::Z)
        .expect("bounds");
    near(bounds[0], 25.);
    near(bounds[1], 28.);
    warm(&doc, &mut kernel, &built, root.path());
    built.release_all(&mut kernel);
}

fn put(doc: &mut Document, payload: ObjectPayload) -> ObjectId {
    let id = ObjectId::new();
    let ordinal = doc.objects().expect("objects").len() as i64;
    doc.write(|writer| {
        writer.put_object(id, None, ordinal, None, &payload)?;
        for dependency in payload.dependencies(id) {
            writer.add_dependency(dependency)?;
        }
        Ok(())
    })
    .expect("object and dependencies");
    id
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
fn side_pocket_and_through_slot_use_outward_normals_and_split_selection() {
    if !native() {
        return;
    }
    for slot in [false, true] {
        let (root, mut doc, mut kernel) = plate();
        let body = ferritecad_fixtures::feature_changes::body(&doc).expect("body");
        let (base, _, profile) =
            ferritecad_fixtures::feature_changes::base_profile(&doc, body).expect("base");
        let built = build(&doc, &mut kernel);
        let top =
            sweep_face(&built, &mut kernel, base, NamingInput::Cap { end: true }).expect("top");
        let side = sweep_face(
            &built,
            &mut kernel,
            base,
            NamingInput::Curve(profile.curves[0].id),
        )
        .expect("side");
        built.release_all(&mut kernel);
        let plane = put(
            &mut doc,
            ObjectPayload::DatumPlane(DatumPlane::Face {
                result: base,
                face: side,
                offset_mm: number(0.),
            }),
        );
        let sketch = put(
            &mut doc,
            ObjectPayload::Sketch(if slot {
                rectangle(plane, [[28., 8.], [32., 8.], [32., 14.], [28., 14.]])
            } else {
                circle(plane, [20., 5.], 2.).expect("profile")
            }),
        );
        let feature = ObjectId::new();
        doc.write_changes(&[EditChange::AddFeature {
            object: feature,
            body,
            name: None,
            feature: FeatureAddition::Extrude {
                profile: sketch,
                regions: vec![],
                operation: SolidOperation::Cut,
                reversed: true,
                end_condition: if slot {
                    EndCondition::ThroughAll
                } else {
                    EndCondition::Blind {
                        distance: number(4.),
                    }
                },
            },
        }])
        .expect("cut");
        let built = build(&doc, &mut kernel);
        assert_eq!(
            built.plane(plane).expect("plane").normal(),
            Vec3::new(0., -1., 0.).expect("n")
        );
        let volume = kernel
            .shape_stats(built.shape(feature).expect("cut"))
            .expect("volume")
            .1;
        near(
            volume,
            24000.
                - if slot {
                    4. * 2. * 40.
                } else {
                    std::f64::consts::PI * 4. * 4.
                },
        );
        built.release_all(&mut kernel);
        if slot {
            let support = put(
                &mut doc,
                ObjectPayload::DatumPlane(DatumPlane::Face {
                    result: feature,
                    face: top,
                    offset_mm: number(0.),
                }),
            );
            let refused =
                rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("open");
            let message = refused.report().failed[0].message.clone();
            assert!(message.contains("face split into 2 faces"), "{message}");
            assert!(message.contains(&top.id.to_string()));
            refused.release_all(&mut kernel);
            let mut family = top;
            family.meaning.selection = Selection::AllDescendants;
            doc.write_changes(&[EditChange::SetDatumFace {
                object: support,
                face: family,
            }])
            .expect("explicit family");
            let built = build(&doc, &mut kernel);
            near(built.plane(support).expect("family plane").origin().z, 10.);
            warm(&doc, &mut kernel, &built, root.path());
            built.release_all(&mut kernel);
        }
    }
}

#[test]
fn partial_revolve_caps_support_planes_and_curved_faces_refuse() {
    if !native() {
        return;
    }
    let (root, mut doc, mut kernel) = plate();
    let plane = put(
        &mut doc,
        ObjectPayload::DatumPlane(DatumPlane::Fixed {
            placement: ferritecad_types::Transform::IDENTITY,
        }),
    );
    let sketch_payload = rectangle(plane, [[5., 0.], [10., 0.], [10., 8.], [5., 8.]]);
    let outer = sketch_payload.curves[1].id;
    let sketch = put(&mut doc, ObjectPayload::Sketch(sketch_payload));
    let feature = put(
        &mut doc,
        ObjectPayload::Revolve(Revolve {
            profile: sketch,
            regions: vec![],
            axis: RevolveAxis::SketchY,
            extent: RevolveExtent::Partial { angle: number(1.) },
            operation: SolidOperation::NewBody,
            previous: None,
        }),
    );
    put(
        &mut doc,
        ObjectPayload::Body(Body {
            tip_feature: Some(feature),
        }),
    );
    let built = build(&doc, &mut kernel);
    let cap = sweep_face(&built, &mut kernel, feature, NamingInput::Cap { end: true })
        .expect("revolve cap");
    let curved = sweep_face(&built, &mut kernel, feature, NamingInput::Curve(outer))
        .expect("outer revolve wall");
    built.release_all(&mut kernel);
    let support = put(
        &mut doc,
        ObjectPayload::DatumPlane(DatumPlane::Face {
            result: feature,
            face: cap,
            offset_mm: number(0.),
        }),
    );
    let built = build(&doc, &mut kernel);
    warm(&doc, &mut kernel, &built, root.path());
    built.release_all(&mut kernel);
    doc.write_changes(&[EditChange::SetDatumFace {
        object: support,
        face: curved,
    }])
    .expect("stored for repair");
    let built = rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("open");
    let report = built.report();
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].kind, "input");
    assert!(report.failed[0].message.contains("not planar"));
    assert!(report.failed[0].message.contains(&curved.id.to_string()));
    assert!(built.shape(feature).is_some());
    built.release_all(&mut kernel);
}

#[test]
fn deleted_support_skips_dependents_and_other_reference_failures_name_the_consumption() {
    if !native() {
        return;
    }
    use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool};
    use ferritecad_topology::generative::{cap_root, generated};
    let (root, mut doc, mut kernel) = plate();
    let body = body(&doc).expect("body");
    let (_, _, base_sketch) = base_profile(&doc, body).expect("base");
    let cut = circular_tool(
        &doc,
        body,
        [20., 20.],
        3.,
        EndCondition::Blind {
            distance: number(4.),
        },
    )
    .expect("cut");
    doc.write_changes(&cut.changes).expect("pocket");
    let floor = generated(
        cut.feature,
        ferritecad_kernel::NamingOperation::Extrude,
        cap_root(cut.feature, true, Some(base_sketch.plane)),
        SubShapeKind::Face,
    );
    let face = EntityReference::new(ReferenceMeaning {
        producer: cut.feature,
        kind: EntityKind::Face,
        digest: floor.digest,
        selection: Selection::One,
    });
    let plane = put(
        &mut doc,
        ObjectPayload::DatumPlane(DatumPlane::Face {
            result: cut.feature,
            face,
            offset_mm: number(0.),
        }),
    );
    let sketch = put(
        &mut doc,
        ObjectPayload::Sketch(circle(plane, [20., 20.], 1.).expect("circle")),
    );
    let feature = put(
        &mut doc,
        ObjectPayload::Extrude(Extrude {
            profile: sketch,
            regions: vec![],
            operation: SolidOperation::NewBody,
            previous: None,
            reversed: false,
            end_condition: EndCondition::Blind {
                distance: number(1.),
            },
        }),
    );
    let built = build(&doc, &mut kernel);
    warm(&doc, &mut kernel, &built, root.path());
    built.release_all(&mut kernel);
    doc.write_changes(&[EditChange::SetEndCondition {
        object: cut.feature,
        end_condition: EndCondition::ThroughAll,
    }])
    .expect("broken model for repair");
    let broken =
        rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("open broken");
    let report = broken.report();
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].id, plane);
    assert_eq!(report.failed[0].kind, "topology");
    assert!(report.failed[0].message.contains("face deleted"));
    assert!(report.skipped.iter().any(|s| s.id == sketch));
    assert!(report.skipped.iter().any(|s| s.id == feature));
    assert!(broken.shape(body).is_some());
    for (changed, text) in [
        (
            ReferenceMeaning {
                producer: ObjectId::new(),
                ..face.meaning
            },
            "not in the consumed",
        ),
        (
            ReferenceMeaning {
                digest: ferritecad_types::ContentHash::of_bytes(b"unknown"),
                ..face.meaning
            },
            "unknown name",
        ),
        (
            ReferenceMeaning {
                kind: EntityKind::Edge,
                ..face.meaning
            },
            "wrong kind",
        ),
    ] {
        let reference = EntityReference {
            meaning: changed,
            ..face
        };
        let error = broken
            .topology()
            .resolve(EntityKind::Face, plane, cut.feature, &[reference])
            .expect_err("specific refusal");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Topology);
        assert!(error.to_string().contains(text), "{error}");
        assert!(error.to_string().contains(&face.id.to_string()));
    }
    broken.release_all(&mut kernel);
}
