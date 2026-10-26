// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_eval::{rebuild_cached, rebuild_cold};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{ObjectId, StableEntityId, Transform};
use std::collections::BTreeMap;

fn put(d: &mut Document, payload: ObjectPayload) -> ObjectId {
    let id = ObjectId::new();
    d.write(|w| {
        w.put_object(id, None, 0, None, &payload)?;
        for dep in payload.dependencies(id) {
            w.add_dependency(dep)?;
        }
        Ok(())
    })
    .expect("write fixture");
    id
}
fn rectangle(d: &mut Document, plane: ObjectId, points: [[f64; 2]; 4]) -> ObjectId {
    let curves = (0..4)
        .map(|i| SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(points[i][0], points[i][1]).expect("start"),
                end: Point2::new(points[(i + 1) % 4][0], points[(i + 1) % 4][1]).expect("end"),
            },
        })
        .collect();
    put(
        d,
        ObjectPayload::Sketch(Sketch {
            plane,
            curves,
            constraints: vec![],
        }),
    )
}
fn blind(distance: f64) -> EndCondition {
    EndCondition::Blind {
        distance: Expression::constant(distance).expect("distance"),
    }
}
fn extrude(
    d: &mut Document,
    profile: ObjectId,
    previous: Option<ObjectId>,
    operation: SolidOperation,
    end_condition: EndCondition,
    reversed: bool,
) -> ObjectId {
    put(
        d,
        ObjectPayload::Extrude(Extrude {
            profile,
            previous,
            operation,
            end_condition,
            reversed,
        }),
    )
}
fn check(d: &Document, id: ObjectId, volume: f64) {
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = rebuild_cold(d, &mut kernel, &context).expect("rebuild");
    assert!(built.report().failed.is_empty(), "{:?}", built.report());
    let shape = built.shape(id).expect("shape");
    assert!(kernel.is_valid(shape).expect("independent BRepCheck"));
    let (_, measured) = kernel.shape_stats(shape).expect("volume");
    assert!(
        (measured - volume).abs() < 1e-6 * volume.max(1.),
        "{measured} != {volume}"
    );
    let names = built.topology().feature(id).expect("names");
    assert_eq!(
        names.entities.len(),
        kernel
            .shape_topology(shape)
            .expect("topology")
            .adjacent_faces
            .len()
    );
    let signature = names
        .bindings
        .iter()
        .map(|(name, entities)| (*name, entities.len()))
        .collect::<BTreeMap<_, _>>();
    built.release_all(&mut kernel);
    let temp = tempfile::tempdir().expect("cache root");
    let mut cache = CacheStore::open(
        temp.path().join("cache.sqlite"),
        d.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("cache");
    for attempt in 0..2 {
        let (cached, events) =
            rebuild_cached(d, &mut kernel, &mut cache, &context).expect("cached");
        assert!(cached.all_built(), "{:?}", cached.report());
        if attempt == 1 {
            assert!(
                events
                    .iter()
                    .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit),
                "{events:?}"
            );
        }
        let names = cached.topology().feature(id).expect("cached names");
        assert_eq!(
            signature,
            names
                .bindings
                .iter()
                .map(|(name, entities)| (*name, entities.len()))
                .collect::<BTreeMap<_, _>>()
        );
        cached.release_all(&mut kernel);
    }
}
#[test]
fn boolean_sweep_provenance_and_cache() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    for (operation, volume) in [
        (SolidOperation::Add, 1500.),
        (SolidOperation::Cut, 500.),
        (SolidOperation::Intersect, 500.),
    ] {
        let temp = tempfile::tempdir().expect("root");
        let mut d = Document::create(temp.path().join("model.fcad")).expect("document");
        let plane = put(
            &mut d,
            ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        );
        let base = rectangle(&mut d, plane, [[0., 0.], [10., 0.], [10., 10.], [0., 10.]]);
        let root = extrude(
            &mut d,
            base,
            None,
            SolidOperation::NewBody,
            blind(10.),
            false,
        );
        let profile = rectangle(&mut d, plane, [[5., 0.], [15., 0.], [15., 10.], [5., 10.]]);
        let tip = extrude(&mut d, profile, Some(root), operation, blind(10.), false);
        put(
            &mut d,
            ObjectPayload::Body(Body {
                tip_feature: Some(tip),
            }),
        );
        check(&d, tip, volume);
    }
}

#[test]
fn extrusion_end_conditions_and_directions() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    for reversed in [false, true] {
        for end in [
            blind(6.),
            EndCondition::Symmetric {
                distance: Expression::constant(3.).expect("distance"),
            },
            EndCondition::ThroughAll,
        ] {
            for operation in [
                SolidOperation::NewBody,
                SolidOperation::Add,
                SolidOperation::Cut,
                SolidOperation::Intersect,
            ] {
                if matches!(operation, SolidOperation::NewBody | SolidOperation::Add)
                    && end == EndCondition::ThroughAll
                {
                    continue;
                }
                let temp = tempfile::tempdir().expect("root");
                let mut d = Document::create(temp.path().join("model.fcad")).expect("document");
                let plane = put(
                    &mut d,
                    ObjectPayload::DatumPlane(DatumPlane {
                        placement: Transform::IDENTITY,
                    }),
                );
                let base = rectangle(&mut d, plane, [[0., 0.], [10., 0.], [10., 10.], [0., 10.]]);
                let root = extrude(
                    &mut d,
                    base,
                    None,
                    SolidOperation::NewBody,
                    EndCondition::Symmetric {
                        distance: Expression::constant(10.).expect("distance"),
                    },
                    false,
                );
                let profile =
                    rectangle(&mut d, plane, [[5., 0.], [15., 0.], [15., 10.], [5., 10.]]);
                let previous = (operation != SolidOperation::NewBody).then_some(root);
                let tip = extrude(&mut d, profile, previous, operation, end.clone(), reversed);
                let through = end == EndCondition::ThroughAll;
                let overlap = if through { 500. } else { 300. };
                let tool = if through { 1100. } else { 600. };
                let volume = match operation {
                    SolidOperation::NewBody => tool,
                    SolidOperation::Add => 2000. + tool - overlap,
                    SolidOperation::Cut => 2000. - overlap,
                    SolidOperation::Intersect => overlap,
                    _ => unreachable!(),
                };
                check(&d, tip, volume);
            }
        }
    }
}
fn revolve(
    d: &mut Document,
    profile: ObjectId,
    previous: Option<ObjectId>,
    operation: SolidOperation,
    axis: RevolveAxis,
    extent: RevolveExtent,
) -> ObjectId {
    put(
        d,
        ObjectPayload::Revolve(Revolve {
            profile,
            previous,
            operation,
            axis,
            extent,
        }),
    )
}
#[test]
fn revolutions_combine_full_and_partial_tools() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    for partial in [false, true] {
        for operation in [
            SolidOperation::NewBody,
            SolidOperation::Add,
            SolidOperation::Cut,
            SolidOperation::Intersect,
        ] {
            let temp = tempfile::tempdir().expect("root");
            let mut d = Document::create(temp.path().join("model.fcad")).expect("document");
            let plane = put(
                &mut d,
                ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            );
            let base = rectangle(&mut d, plane, [[0., 0.], [10., 0.], [10., 10.], [0., 10.]]);
            let root = revolve(
                &mut d,
                base,
                None,
                SolidOperation::NewBody,
                RevolveAxis::SketchY,
                RevolveExtent::FullTurn,
            );
            let profile = rectangle(&mut d, plane, [[5., 0.], [15., 0.], [15., 10.], [5., 10.]]);
            let previous = (operation != SolidOperation::NewBody).then_some(root);
            let extent = if partial {
                RevolveExtent::Partial {
                    angle: Expression::constant(std::f64::consts::PI).expect("angle"),
                }
            } else {
                RevolveExtent::FullTurn
            };
            let tip = revolve(
                &mut d,
                profile,
                previous,
                operation,
                RevolveAxis::SketchY,
                extent,
            );
            let fraction = if partial { 0.5 } else { 1. };
            let overlap = 750. * fraction;
            let tool = 2000. * fraction;
            let volume = match operation {
                SolidOperation::NewBody => tool,
                SolidOperation::Add => 1000. + tool - overlap,
                SolidOperation::Cut => 1000. - overlap,
                SolidOperation::Intersect => overlap,
                _ => unreachable!(),
            } * std::f64::consts::PI;
            check(&d, tip, volume);
        }
    }
}

#[test]
fn revolve_axes_and_curved_profiles() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    for case in [
        "x",
        "construction",
        "profile_line",
        "arc",
        "circle",
        "annulus",
    ] {
        let temp = tempfile::tempdir().expect("root");
        let mut d = Document::create(temp.path().join("model.fcad")).expect("document");
        let plane = put(
            &mut d,
            ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        );
        let circle = |radius| SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Circle {
                center: Point2::new(10., 0.).expect("center"),
                radius,
            },
        };
        let (profile, axis, volume) = match case {
            "x" => (
                rectangle(&mut d, plane, [[0., 5.], [10., 5.], [10., 15.], [0., 15.]]),
                RevolveAxis::SketchX,
                2000. * std::f64::consts::PI,
            ),
            "construction" => {
                let profile =
                    rectangle(&mut d, plane, [[8., 0.], [18., 0.], [18., 10.], [8., 10.]]);
                let mut payload = d.object(profile).expect("object").expect("sketch").payload;
                let ObjectPayload::Sketch(s) = &mut payload else {
                    unreachable!()
                };
                let id = StableEntityId::new();
                s.curves.push(SketchCurve {
                    id,
                    construction: true,
                    geometry: SketchGeometry::Line {
                        start: Point2::new(3., 0.).expect("start"),
                        end: Point2::new(3., 10.).expect("end"),
                    },
                });
                let profile = put(&mut d, payload);
                (profile, RevolveAxis::Line(id), 2000. * std::f64::consts::PI)
            }
            "profile_line" => {
                let profile =
                    rectangle(&mut d, plane, [[0., 0.], [10., 0.], [10., 10.], [0., 10.]]);
                let ObjectPayload::Sketch(s) =
                    d.object(profile).expect("object").expect("sketch").payload
                else {
                    unreachable!()
                };
                (
                    profile,
                    RevolveAxis::Line(s.curves[3].id),
                    1000. * std::f64::consts::PI,
                )
            }
            "arc" => {
                let curves = vec![
                    SketchCurve {
                        id: StableEntityId::new(),
                        construction: false,
                        geometry: SketchGeometry::Arc {
                            center: Point2::new(10., 0.).expect("center"),
                            radius: 2.,
                            start_angle: 0.,
                            end_angle: std::f64::consts::PI,
                        },
                    },
                    SketchCurve {
                        id: StableEntityId::new(),
                        construction: false,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(8., 0.).expect("start"),
                            end: Point2::new(12., 0.).expect("end"),
                        },
                    },
                ];
                (
                    put(
                        &mut d,
                        ObjectPayload::Sketch(Sketch {
                            plane,
                            curves,
                            constraints: vec![],
                        }),
                    ),
                    RevolveAxis::SketchY,
                    40. * std::f64::consts::PI.powi(2),
                )
            }
            _ => {
                let mut curves = vec![circle(2.)];
                if case == "annulus" {
                    curves.push(circle(1.));
                }
                (
                    put(
                        &mut d,
                        ObjectPayload::Sketch(Sketch {
                            plane,
                            curves,
                            constraints: vec![],
                        }),
                    ),
                    RevolveAxis::SketchY,
                    if case == "annulus" { 60. } else { 80. } * std::f64::consts::PI.powi(2),
                )
            }
        };
        let tip = revolve(
            &mut d,
            profile,
            None,
            SolidOperation::NewBody,
            axis,
            RevolveExtent::FullTurn,
        );
        check(&d, tip, volume);
    }
}

#[test]
fn invalid_results_fail_the_feature_with_a_reason() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    for (case, reason) in [
        ("empty", "one solid"),
        ("disjoint", "one solid"),
        ("split", "one solid"),
        ("behind", "nothing ahead"),
        ("cross_axis", "cross the axis"),
    ] {
        let temp = tempfile::tempdir().expect("root");
        let mut d = Document::create(temp.path().join("model.fcad")).expect("document");
        let plane = put(
            &mut d,
            ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        );
        let profile = rectangle(&mut d, plane, [[0., 0.], [10., 0.], [10., 10.], [0., 10.]]);
        let base = extrude(
            &mut d,
            profile,
            None,
            SolidOperation::NewBody,
            blind(10.),
            false,
        );
        let tool = rectangle(
            &mut d,
            plane,
            if case == "split" {
                [[4., -1.], [6., -1.], [6., 11.], [4., 11.]]
            } else if case == "cross_axis" {
                [[-5., 0.], [10., 0.], [10., 10.], [-5., 10.]]
            } else {
                [[15., 0.], [20., 0.], [20., 10.], [15., 10.]]
            },
        );
        let tip = if case == "cross_axis" {
            revolve(
                &mut d,
                tool,
                None,
                SolidOperation::NewBody,
                RevolveAxis::SketchY,
                RevolveExtent::FullTurn,
            )
        } else {
            extrude(
                &mut d,
                tool,
                Some(base),
                match case {
                    "empty" => SolidOperation::Intersect,
                    "disjoint" => SolidOperation::Add,
                    _ => SolidOperation::Cut,
                },
                if case == "behind" {
                    EndCondition::ThroughAll
                } else {
                    blind(10.)
                },
                case == "behind",
            )
        };
        let mut kernel = OcctKernel::new().expect("kernel");
        let built =
            rebuild_cold(&d, &mut kernel, &OperationContext::default()).expect("opens for repair");
        let report = built.report();
        let failure = report
            .failed
            .iter()
            .find(|f| f.id == tip)
            .expect("feature failed");
        assert!(
            failure.message.contains(reason),
            "{case}: {}",
            failure.message
        );
        assert!(built.shape(base).is_some(), "independent result survives");
        built.release_all(&mut kernel);
    }
}

#[test]
fn through_all_projects_rotated_planes_over_symmetric_and_revolved_targets() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_types::Vec3;
    for revolved in [false, true] {
        for reversed in [false, true] {
            let temp = tempfile::tempdir().expect("root");
            let mut d = Document::create(temp.path().join("model.fcad")).expect("document");
            let plane = put(
                &mut d,
                ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            );
            let profile = rectangle(&mut d, plane, [[0., 0.], [10., 0.], [10., 10.], [0., 10.]]);
            let root = if revolved {
                revolve(
                    &mut d,
                    profile,
                    None,
                    SolidOperation::NewBody,
                    RevolveAxis::SketchY,
                    RevolveExtent::FullTurn,
                )
            } else {
                extrude(
                    &mut d,
                    profile,
                    None,
                    SolidOperation::NewBody,
                    EndCondition::Symmetric {
                        distance: Expression::constant(10.).expect("depth"),
                    },
                    false,
                )
            };
            let (rotation, offset) = if revolved {
                (
                    Vec3::X,
                    Vec3::new(0., if reversed { 0. } else { 10. }, 0.).expect("origin"),
                )
            } else {
                (
                    Vec3::Y,
                    Vec3::new(if reversed { 10. } else { 0. }, 0., 0.).expect("origin"),
                )
            };
            let transform = Transform::from_rotation(rotation, std::f64::consts::FRAC_PI_2)
                .expect("rotation")
                .then(&Transform::from_translation(offset).expect("offset"))
                .expect("placement");
            let plane = put(
                &mut d,
                ObjectPayload::DatumPlane(DatumPlane {
                    placement: transform,
                }),
            );
            let tool = if revolved {
                put(
                    &mut d,
                    ObjectPayload::Sketch(Sketch {
                        plane,
                        constraints: vec![],
                        curves: vec![SketchCurve {
                            id: StableEntityId::new(),
                            construction: false,
                            geometry: SketchGeometry::Circle {
                                center: Point2::ORIGIN,
                                radius: 2.,
                            },
                        }],
                    }),
                )
            } else {
                rectangle(&mut d, plane, [[-2., 2.], [2., 2.], [2., 8.], [-2., 8.]])
            };
            let tip = extrude(
                &mut d,
                tool,
                Some(root),
                SolidOperation::Cut,
                EndCondition::ThroughAll,
                reversed,
            );
            check(
                &d,
                tip,
                if revolved {
                    960. * std::f64::consts::PI
                } else {
                    1760.
                },
            );
        }
    }
}

#[test]
fn an_unrelated_add_keeps_faces_edges_and_vertices_when_an_earlier_add_changes_extent() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_fixtures::feature_changes::{body, circular_tool};
    use ferritecad_kernel::{NamingOperation, SubShapeKind};
    use ferritecad_topology::generative::{curve_root, generated};
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let mut d = Document::open(path).expect("document");
    let b = body(&d).expect("body");
    let mut features = Vec::new();
    for (y, height) in [(10., 12.), (30., 14.)] {
        let mut tool = circular_tool(&d, b, [60., y], 3., blind(height)).expect("tool");
        for c in &mut tool.changes {
            if let EditChange::AddFeature {
                feature: FeatureAddition::Extrude { operation, .. },
                ..
            } = c
            {
                *operation = SolidOperation::Add;
            }
        }
        d.write_changes(&tool.changes).expect("append Add");
        features.push(tool);
    }
    let [first, second] = features.as_slice() else {
        unreachable!()
    };
    let ObjectPayload::Sketch(profile) = d
        .object(second.sketch)
        .expect("read")
        .expect("sketch")
        .payload
    else {
        unreachable!()
    };
    let collect = |d: &Document| {
        let mut k = OcctKernel::new().expect("kernel");
        let built = rebuild_cold(d, &mut k, &OperationContext::default()).expect("rebuild");
        assert!(built.all_built(), "{:?}", built.report());
        let names = built.topology().feature(second.feature).expect("feature");
        let wall = &names.ancestry[&generated(
            second.feature,
            NamingOperation::Extrude,
            curve_root(second.feature, second.curve, Some(profile.plane)),
            SubShapeKind::Face,
        )];
        let found: std::collections::BTreeSet<_> = names
            .entities
            .iter()
            .filter(|(e, _)| {
                wall.contains(e) || !names.topology.adjacent_faces[e].is_disjoint(wall)
            })
            .map(|(_, n)| *n)
            .collect();
        for kind in [SubShapeKind::Face, SubShapeKind::Edge, SubShapeKind::Vertex] {
            assert!(found.iter().any(|n| n.kind == kind));
        }
        let details = found
            .iter()
            .map(|name| (*name, names.derivations[&name.digest].clone()))
            .collect::<BTreeMap<_, _>>();
        built.release_all(&mut k);
        (found, details)
    };
    let before = collect(&d);
    // The early boss remains above the plate and below the later boss's cap.
    // Its changed top is not incident on any boundary of the later boss.
    for extent in [blind(11.), blind(13.), blind(12.)] {
        d.write_changes(&[EditChange::SetEndCondition {
            object: first.feature,
            end_condition: extent,
        }])
        .expect("change early Add");
        let after = collect(&d);
        if before.0 != after.0 {
            eprintln!("before: {:#?}\nafter: {:#?}", before.1, after.1);
        }
        assert_eq!(before.0, after.0);
    }
}

#[test]
fn projected_bounds_ignore_triangulation_and_preserve_symmetric_face_planes() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_kernel::{ExtrudeExtent, ExtrudeRequest, TessellationParams};
    use ferritecad_types::Vec3;
    let root = tempfile::tempdir().expect("root");
    let mut d = Document::create(root.path().join("cylinder.fcad")).expect("document");
    let plane = put(
        &mut d,
        ObjectPayload::DatumPlane(DatumPlane {
            placement: Transform::IDENTITY,
        }),
    );
    let sketch = put(
        &mut d,
        ObjectPayload::Sketch(Sketch {
            plane,
            curves: vec![SketchCurve {
                id: StableEntityId::new(),
                construction: false,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(2., 3.).expect("point"),
                    radius: 5.,
                },
            }],
            constraints: vec![],
        }),
    );
    let base = extrude(
        &mut d,
        sketch,
        None,
        SolidOperation::NewBody,
        EndCondition::Symmetric {
            distance: Expression::constant(7.).expect("distance"),
        },
        false,
    );
    let context = OperationContext::default();
    let mut kernel = OcctKernel::new().expect("kernel");
    let built = rebuild_cold(&d, &mut kernel, &context).expect("rebuild");
    let shape = built.shape(base).expect("cylinder");
    assert!(
        kernel
            .is_valid(shape)
            .expect("symmetric face and wire agree")
    );
    assert_eq!(
        kernel
            .projected_bounds(shape, Vec3::new(0., 0., 1.).expect("z"))
            .expect("bounds"),
        [-7., 7.]
    );
    let direction = Vec3::new(1., 2., 3.).expect("direction");
    let before = kernel
        .projected_bounds(shape, direction)
        .expect("analytic bounds");
    for deflection in [2., 0.01] {
        kernel
            .tessellate(
                shape,
                &TessellationParams::new(deflection, 0.5, false).expect("params"),
                &context,
            )
            .expect("triangulate");
        assert_eq!(
            kernel.projected_bounds(shape, direction).expect("bounds"),
            before
        );
    }
    let request = ExtrudeRequest::new(
        built.profile(sketch).expect("profile").clone(),
        ExtrudeExtent::symmetric(7.).expect("distance"),
        true,
    );
    let reverse = kernel
        .extrude(&request, &context)
        .expect("reverse symmetric");
    assert!(kernel.is_valid(reverse.shape).expect("valid"));
    assert_eq!(
        kernel
            .projected_bounds(reverse.shape, direction)
            .expect("bounds"),
        before
    );
    kernel.release(reverse.shape);
    built.release_all(&mut kernel);
}

#[test]
fn through_all_refuses_new_bodies_and_additive_bosses_at_the_write_boundary() {
    let root = tempfile::tempdir().expect("root");
    let mut d = Document::create(root.path().join("invalid.fcad")).expect("document");
    for operation in [SolidOperation::NewBody, SolidOperation::Add] {
        let payload = ObjectPayload::Extrude(Extrude {
            profile: ObjectId::new(),
            previous: (operation == SolidOperation::Add).then(ObjectId::new),
            operation,
            reversed: false,
            end_condition: EndCondition::ThroughAll,
        });
        let error = d
            .write(|w| w.put_object(ObjectId::new(), None, 0, None, &payload))
            .expect_err("ThroughAll has no meaning for new material");
        assert!(
            error
                .to_string()
                .contains("ThroughAll requires Cut or Intersect"),
            "{error}"
        );
        assert!(d.objects().expect("objects").is_empty());
    }
}
