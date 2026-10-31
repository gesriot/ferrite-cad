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
fn lines(points: [[f64; 2]; 4]) -> Vec<SketchCurve> {
    (0..4)
        .map(|i| SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(points[i][0], points[i][1]).expect("start"),
                end: Point2::new(points[(i + 1) % 4][0], points[(i + 1) % 4][1]).expect("end"),
            },
        })
        .collect()
}
fn sketch(d: &mut Document, plane: ObjectId, curves: Vec<SketchCurve>) -> ObjectId {
    put(
        d,
        ObjectPayload::Sketch(Sketch {
            plane,
            curves,
            constraints: vec![],
        }),
    )
}
fn rectangle(d: &mut Document, plane: ObjectId, points: [[f64; 2]; 4]) -> ObjectId {
    sketch(d, plane, lines(points))
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
            regions: Vec::new(),
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
fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
}
fn circle(x: f64, y: f64, radius: f64) -> SketchCurve {
    SketchCurve {
        id: StableEntityId::new(),
        construction: false,
        geometry: SketchGeometry::Circle {
            center: Point2::new(x, y).expect("point"),
            radius,
        },
    }
}
fn setup() -> (tempfile::TempDir, Document, ObjectId) {
    let temp = tempfile::tempdir().expect("root");
    let mut d = Document::create(temp.path().join("model.fcad")).expect("document");
    let plane = put(
        &mut d,
        ObjectPayload::DatumPlane(DatumPlane::Fixed {
            placement: Transform::IDENTITY,
        }),
    );
    (temp, d, plane)
}
fn body(d: &mut Document, tip: ObjectId) {
    put(
        d,
        ObjectPayload::Body(Body {
            tip_feature: Some(tip),
        }),
    );
}
#[test]
fn holes_islands_and_multiregion_tools_have_complete_names_and_cache() {
    if !native() {
        return;
    }
    for case in ["holes", "island", "two_cuts", "intersect"] {
        let (_temp, mut d, plane) = setup();
        let base = rectangle(&mut d, plane, [[0., 0.], [40., 0.], [40., 30.], [0., 30.]]);
        let root = extrude(
            &mut d,
            base,
            None,
            SolidOperation::NewBody,
            blind(10.),
            false,
        );
        let (curves, previous, operation, depth, volume) = match case {
            "holes" => {
                let mut curves = lines([[0., 0.], [40., 0.], [40., 30.], [0., 30.]]);
                curves.push(circle(8., 8., 3.));
                // A semicircle closed by its diameter is a mixed Line/Arc hole.
                curves.push(SketchCurve {
                    id: StableEntityId::new(),
                    construction: false,
                    geometry: SketchGeometry::Arc {
                        center: Point2::new(25., 10.).expect("point"),
                        radius: 3.,
                        start_angle: 0.,
                        end_angle: std::f64::consts::PI,
                    },
                });
                curves.push(SketchCurve {
                    id: StableEntityId::new(),
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: Point2::new(22., 10.).expect("point"),
                        end: Point2::new(28., 10.).expect("point"),
                    },
                });
                (
                    curves,
                    None,
                    SolidOperation::NewBody,
                    4.,
                    (1200. - 13.5 * std::f64::consts::PI) * 4.,
                )
            }
            "island" => (
                vec![
                    circle(20., 15., 10.),
                    circle(20., 15., 6.),
                    circle(20., 15., 2.),
                ],
                Some(root),
                SolidOperation::Cut,
                4.,
                12000. - 68. * std::f64::consts::PI * 4.,
            ),
            _ => {
                let mut curves = lines([[5., 5.], [10., 5.], [10., 10.], [5., 10.]]);
                // Intersect keeps only the first region; the second misses the target.
                let x = if case == "intersect" { 50. } else { 20. };
                curves.extend(lines([[x, 5.], [x + 5., 5.], [x + 5., 10.], [x, 10.]]));
                (
                    curves,
                    Some(root),
                    if case == "intersect" {
                        SolidOperation::Intersect
                    } else {
                        SolidOperation::Cut
                    },
                    12.,
                    if case == "intersect" { 250. } else { 11500. },
                )
            }
        };
        let profile = sketch(&mut d, plane, curves);
        let tip = extrude(&mut d, profile, previous, operation, blind(depth), false);
        body(&mut d, tip);
        check(&d, tip, volume);
    }
}
#[test]
fn selection_refuses_missing_or_hole_curves_and_collapses_duplicates() {
    if !native() {
        return;
    }
    let (_temp, mut d, plane) = setup();
    let mut curves = lines([[0., 0.], [10., 0.], [10., 10.], [0., 10.]]);
    let selected = vec![curves[0].id, curves[2].id];
    let hole = circle(5., 5., 2.);
    let hole_id = hole.id;
    curves.push(hole);
    curves.extend(lines([[20., 0.], [30., 0.], [30., 10.], [20., 10.]]));
    let profile = sketch(&mut d, plane, curves);
    let tip = extrude(
        &mut d,
        profile,
        None,
        SolidOperation::NewBody,
        blind(5.),
        false,
    );
    body(&mut d, tip);
    let mut kernel = OcctKernel::new().expect("kernel");
    let built = rebuild_cold(&d, &mut kernel, &OperationContext::default()).expect("report");
    assert!(built.all_built(), "{:?}", built.report());
    let shape = built.shape(tip).expect("complete result");
    kernel.validate_solids(shape).expect("every member valid");
    assert_eq!(kernel.solid_count(shape).expect("member count"), 2);
    built.release_all(&mut kernel);
    check(&d, tip, (200. - 4. * std::f64::consts::PI) * 5.);
    for selection in [selected, vec![hole_id], vec![StableEntityId::new()]] {
        let mut payload = d.object(tip).expect("read").expect("feature").payload;
        let ObjectPayload::Extrude(ref mut e) = payload else {
            unreachable!("extrude")
        };
        e.regions = selection.clone();
        d.write(|w| w.put_object(tip, None, 0, None, &payload))
            .expect("selection");
        if selection.len() == 2 {
            check(&d, tip, (100. - 4. * std::f64::consts::PI) * 5.);
        } else {
            let built =
                rebuild_cold(&d, &mut kernel, &OperationContext::default()).expect("report");
            let diagnostic = format!("{:?}", built.report());
            assert!(
                diagnostic.contains(&selection[0].to_string()),
                "{diagnostic}"
            );
            assert!(!built.all_built());
            built.release_all(&mut kernel);
        }
    }
}
#[test]
fn compound_sweeps_keep_curve_history_and_distinct_cap_names() {
    if !native() {
        return;
    }
    let mut curves = lines([[2., 0.], [4., 0.], [4., 2.], [2., 2.]]);
    curves.extend(lines([[6., 0.], [8., 0.], [8., 2.], [6., 2.]]));
    let drawing = Sketch {
        plane: ObjectId::new(),
        curves,
        constraints: vec![],
    };
    let profile = ferritecad_eval::profile_from_sketch(
        &drawing,
        ObjectId::new(),
        ferritecad_kernel::SketchPlane::world_xy(),
    )
    .expect("profile")
    .profile;
    let mut kernel = OcctKernel::new().expect("kernel");
    for revolve in [false, true] {
        let result = if revolve {
            kernel.revolve(
                &ferritecad_kernel::RevolveRequest::new(
                    profile.clone(),
                    ferritecad_kernel::RevolveAxis::PlaneY,
                    ferritecad_kernel::RevolveTurn::Partial(
                        ferritecad_kernel::PartialTurn::new(1.).expect("angle"),
                    ),
                ),
                &OperationContext::default(),
            )
        } else {
            kernel.extrude(
                &ferritecad_kernel::ExtrudeRequest::new(
                    profile.clone(),
                    ferritecad_kernel::ExtrudeExtent::blind(3.).expect("depth"),
                    false,
                ),
                &OperationContext::default(),
            )
        }
        .expect("compound sweep");
        assert!(
            kernel
                .is_valid(result.shape)
                .expect("independent BRepCheck")
        );
        let history = kernel.naming_history(result.shape).expect("history");
        assert_eq!(history.cap_boundaries.len(), 4);
        let feature = ObjectId::new();
        let names = naming::bind(&mut kernel, feature, &profile, &result).expect("complete names");
        let named = names.feature(feature).expect("feature");
        for cap in history.cap_boundaries.keys() {
            assert_eq!(
                named.bindings[&named.entities[cap]].len(),
                1,
                "distinct cap"
            );
        }
        kernel.release(result.shape);
    }
}
#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;
#[test]
fn revolve_annulus_and_two_region_cut_keep_full_history() {
    if !native() {
        return;
    }
    for cut in [false, true] {
        let (_temp, mut d, plane) = setup();
        let base = rectangle(&mut d, plane, [[2., -4.], [18., -4.], [18., 4.], [2., 4.]]);
        let root = put(
            &mut d,
            ObjectPayload::Revolve(Revolve {
                profile: base,
                regions: vec![],
                previous: None,
                operation: SolidOperation::NewBody,
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::FullTurn,
            }),
        );
        let curves = if cut {
            vec![circle(6., 0., 1.), circle(14., 0., 1.)]
        } else {
            vec![circle(10., 0., 2.), circle(10., 0., 1.)]
        };
        let profile = sketch(&mut d, plane, curves);
        let tip = put(
            &mut d,
            ObjectPayload::Revolve(Revolve {
                profile,
                regions: vec![],
                previous: cut.then_some(root),
                operation: if cut {
                    SolidOperation::Cut
                } else {
                    SolidOperation::NewBody
                },
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::FullTurn,
            }),
        );
        body(&mut d, tip);
        let pi = std::f64::consts::PI;
        check(
            &d,
            tip,
            if cut {
                2560. * pi - 40. * pi * pi
            } else {
                60. * pi * pi
            },
        );
    }
}

#[test]
fn unrelated_hole_changes_preserve_every_name_of_the_other_region() {
    if !native() {
        return;
    }
    let mut curves = lines([[0., 0.], [10., 0.], [10., 10.], [0., 10.]]);
    curves.push(circle(5., 5., 2.));
    curves.extend(lines([[20., 0.], [30., 0.], [30., 10.], [20., 10.]]));
    let second: std::collections::BTreeSet<_> = curves[5..].iter().map(|c| c.id).collect();
    let mut drawing = Sketch {
        plane: ObjectId::new(),
        curves,
        constraints: vec![],
    };
    let feature = ObjectId::new();
    let mut signatures = Vec::new();
    let mut kernel = OcctKernel::new().expect("kernel");
    for radius in [2., 3.] {
        drawing.curves[4].geometry = SketchGeometry::Circle {
            center: Point2::new(5., 5.).expect("center"),
            radius,
        };
        let profile = ferritecad_eval::profile_from_sketch(
            &drawing,
            ObjectId::new(),
            ferritecad_kernel::SketchPlane::world_xy(),
        )
        .expect("profile")
        .profile;
        let result = kernel
            .extrude(
                &ferritecad_kernel::ExtrudeRequest::new(
                    profile.clone(),
                    ferritecad_kernel::ExtrudeExtent::blind(3.).expect("height"),
                    false,
                ),
                &OperationContext::default(),
            )
            .expect("sweep");
        let history = kernel.naming_history(result.shape).expect("history");
        let model = naming::bind(&mut kernel, feature, &profile, &result).expect("naming");
        let named = model.feature(feature).expect("feature");
        let mut faces: std::collections::BTreeSet<_> = history
            .relations
            .iter()
            .filter_map(|(input, _, output)| {
                if let ferritecad_kernel::NamingInput::Curve(curve) = input
                    && second.contains(curve)
                {
                    output.filter(|o| o.kind() == ferritecad_kernel::SubShapeKind::Face)
                } else {
                    None
                }
            })
            .collect();
        faces.extend(
            history
                .cap_boundaries
                .iter()
                .filter_map(|(face, witnesses)| (witnesses == &second).then_some(*face)),
        );
        let signature: std::collections::BTreeSet<_> = named
            .entities
            .iter()
            .filter(|(entity, _)| {
                faces.contains(entity)
                    || named.topology.adjacent_faces[*entity]
                        .iter()
                        .any(|f| faces.contains(f))
            })
            .map(|(_, name)| *name)
            .collect();
        assert_eq!(
            signature.len(),
            26,
            "all six faces, twelve edges and eight vertices"
        );
        signatures.push(signature);
        kernel.release(result.shape);
    }
    assert_eq!(signatures[0], signatures[1]);
}

#[test]
fn invalid_boundaries_and_lost_selections_name_the_curves_without_retargeting() {
    if !native() {
        return;
    }
    for case in [
        "crossing",
        "self",
        "open",
        "deleted",
        "construction",
        "hole",
    ] {
        let (_temp, mut d, plane) = setup();
        let mut curves = if case == "self" {
            lines([[0., 0.], [10., 10.], [0., 10.], [10., 0.]])
        } else if case == "open" {
            let mut c = lines([[0., 0.], [10., 0.], [10., 10.], [0., 10.]]);
            c.pop();
            c
        } else {
            vec![circle(0., 0., 3.), circle(10., 0., 2.)]
        };
        let selected = curves[0].id;
        let expected = if case == "open" {
            curves[2].id
        } else {
            selected
        };
        if case == "crossing" {
            curves[1].geometry = SketchGeometry::Circle {
                center: Point2::new(3., 0.).expect("center"),
                radius: 3.,
            };
        }
        let profile = sketch(&mut d, plane, curves.clone());
        let tip = extrude(
            &mut d,
            profile,
            None,
            SolidOperation::NewBody,
            blind(5.),
            false,
        );
        body(&mut d, tip);
        if matches!(case, "deleted" | "construction" | "hole") {
            let mut payload = d.object(tip).expect("feature").expect("row").payload;
            let ObjectPayload::Extrude(ref mut e) = payload else {
                unreachable!("Extrude")
            };
            e.regions = vec![selected];
            d.write(|w| w.put_object(tip, None, 0, None, &payload))
                .expect("selection");
            check(&d, tip, 45. * std::f64::consts::PI);
            match case {
                "deleted" => {
                    curves.remove(0);
                }
                "construction" => curves[0].construction = true,
                "hole" => {
                    curves[1].geometry = SketchGeometry::Circle {
                        center: Point2::new(0., 0.).expect("center"),
                        radius: 6.,
                    }
                }
                _ => unreachable!("selected cases"),
            }
            let payload = ObjectPayload::Sketch(Sketch {
                plane,
                curves,
                constraints: vec![],
            });
            d.write(|w| w.put_object(profile, None, 0, None, &payload))
                .expect("changed sketch");
        }
        let mut k = OcctKernel::new().expect("kernel");
        let built = rebuild_cold(&d, &mut k, &OperationContext::default()).expect("report");
        assert!(!built.all_built(), "{case} must refuse");
        let report = built.report();
        assert!(
            report
                .failed
                .iter()
                .any(|f| f.message.contains(&expected.to_string())),
            "{case}: {report:?}"
        );
        built.release_all(&mut k);
    }
}
