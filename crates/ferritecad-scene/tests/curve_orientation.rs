// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
#[path = "../../ferritecad-fixtures/tests/support/curve_edits.rs"]
mod support;
use ferritecad_document::*;
use ferritecad_eval::{RebuildResult, rebuild_cached, rebuild_cold};
use ferritecad_kernel::{GeometryKernel, OperationContext, SubShapeKind};
use ferritecad_occt::OcctKernel;
use ferritecad_topology::generative::curve_root;
use std::collections::BTreeMap;
use std::f64::consts::PI;
use support::*;

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
}
fn names(
    built: &RebuildResult,
    f: &Drawing,
) -> BTreeMap<ferritecad_topology::generative::Name, usize> {
    built
        .topology()
        .feature(f.feature)
        .expect("names")
        .bindings
        .iter()
        .map(|(name, entities)| (*name, entities.len()))
        .collect()
}
fn stored_ends(built: &RebuildResult, f: &Drawing, k: &mut OcctKernel) {
    let saved = f.saved();
    let mut witnessed = 0;
    for (name, entities) in &built.topology().feature(f.feature).expect("names").bindings {
        if name.kind != SubShapeKind::Vertex {
            continue;
        }
        let description = built
            .topology()
            .describe(f.feature, *name)
            .expect("derivation");
        for curve in saved.curves.iter().filter(|c| c.bounds_profile()) {
            let ends = match curve.geometry {
                SketchGeometry::Line { start, end } => [start, end],
                SketchGeometry::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                } => [
                    point(
                        center.x + radius * start_angle.cos(),
                        center.y + radius * start_angle.sin(),
                    ),
                    point(
                        center.x + radius * end_angle.cos(),
                        center.y + radius * end_angle.sin(),
                    ),
                ],
                SketchGeometry::Circle { .. } => continue,
                _ => unreachable!("profile curve"),
            };
            let root = curve_root(f.feature, curve.id, Some(f.plane));
            for (i, at) in ends.iter().enumerate() {
                let end = if i == 0 { "start" } else { "end" };
                let witness = format!("curve-end {end}, inputs [edge {}]", root.digest);
                if description
                    .split("; ")
                    .filter_map(|s| s.split_once(" = "))
                    .any(|(_, term)| term == witness)
                {
                    for entity in entities {
                        let actual = k.vertex_point(*entity).expect("native vertex");
                        assert!(
                            (actual.x - at.x).abs() < 1e-6 && (actual.y - at.y).abs() < 1e-6,
                            "stored {end} of {}: {actual:?} != {at:?}",
                            curve.id
                        );
                        assert!(actual.z.abs() < 1e-6 || (actual.z - 3.).abs() < 1e-6);
                        witnessed += 1;
                    }
                }
            }
        }
    }
    let open = saved
        .curves
        .iter()
        .filter(|c| c.bounds_profile() && !matches!(c.geometry, SketchGeometry::Circle { .. }))
        .count();
    assert_eq!(
        witnessed,
        open * 4,
        "every stored curve end has its two sweep vertices"
    );
}
fn check(built: &RebuildResult, f: &Drawing, k: &mut OcctKernel, volume: f64) -> [usize; 3] {
    assert!(built.all_built(), "{:?}", built.report());
    let shape = built.shape(f.body).expect("body");
    assert!(k.is_valid(shape).expect("validity"));
    let (_, actual) = k.shape_stats(shape).expect("volume");
    assert_eq!(k.solid_count(shape).expect("solids"), 1);
    assert!((actual - volume).abs() < 1e-6, "{actual} != {volume}");
    stored_ends(built, f, k);
    let inventory = k.shape_topology(shape).expect("inventory");
    [SubShapeKind::Face, SubShapeKind::Edge, SubShapeKind::Vertex].map(|kind| {
        inventory
            .adjacent_faces
            .keys()
            .filter(|e| e.kind() == kind)
            .count()
    })
}
#[test]
fn six_rectangle_records_preserve_geometry_names_and_stored_endpoint_witnesses() -> TestResult {
    if !native() {
        return Ok(());
    }
    let base = rectangle();
    let mut f = Drawing::new(base.clone(), true);
    let mut k = OcctKernel::new()?;
    let context = OperationContext::default();
    let mut reference = None;
    for (i, curves) in variants(&base).into_iter().enumerate() {
        f.replace(curves);
        let saved = std::fs::read(&f.path)?;
        let built = rebuild_cold(&f.doc, &mut k, &context)?;
        assert_eq!(check(&built, &f, &mut k, 240.), [6, 12, 8]);
        if i == 0 {
            reference = Some(names(&built, &f));
        }
        if i == 4 {
            assert_eq!(
                Some(names(&built, &f)),
                reference,
                "list order changes no names"
            );
        }
        assert_eq!(std::fs::read(&f.path)?, saved);
        built.release_all(&mut k);
    }
    Ok(())
}
fn arc_loop(end_angle: f64) -> Vec<SketchCurve> {
    vec![
        line([0., 0.], [10., 0.]),
        line([10., 0.], [10., 8.]),
        curve(SketchGeometry::Arc {
            center: point(5., 8.),
            radius: 5.,
            start_angle: 0.,
            end_angle,
        }),
        line([0., 8.], [0., 0.]),
    ]
}
#[test]
fn reversed_arc_and_alternating_lines_keep_their_saved_end_names() -> TestResult {
    if !native() {
        return Ok(());
    }
    let base = arc_loop(PI);
    let mut f = Drawing::new(base.clone(), true);
    let mut k = OcctKernel::new()?;
    let context = OperationContext::default();
    let reference = rebuild_cold(&f.doc, &mut k, &context)?;
    let counts = check(&reference, &f, &mut k, 3. * (80. + 12.5 * PI));
    let mut changed = base;
    for i in [0, 2] {
        reverse(&mut changed[i]);
    }
    changed.swap(1, 3);
    f.replace(changed);
    let built = rebuild_cold(&f.doc, &mut k, &context)?;
    assert_eq!(check(&built, &f, &mut k, 3. * (80. + 12.5 * PI)), counts);
    Ok(())
}
#[test]
fn a_scrambled_hole_keeps_every_stored_endpoint_and_topology_count() -> TestResult {
    if !native() {
        return Ok(());
    }
    let mut base = rectangle();
    let mut hole = rectangle();
    for c in &mut hole {
        if let SketchGeometry::Line { start, end } = &mut c.geometry {
            for p in [start, end] {
                p.x = p.x * 0.2 + 3.;
                p.y = p.y * 0.2 + 2.;
            }
        }
    }
    base.extend(hole.clone());
    let mut f = Drawing::new(base.clone(), true);
    let mut k = OcctKernel::new()?;
    let context = OperationContext::default();
    let reference = rebuild_cold(&f.doc, &mut k, &context)?;
    let counts = check(&reference, &f, &mut k, 230.4);
    base.truncate(4);
    base.extend(variants(&hole).remove(5));
    f.replace(base);
    let built = rebuild_cold(&f.doc, &mut k, &context)?;
    assert_eq!(check(&built, &f, &mut k, 230.4), counts);
    Ok(())
}
#[test]
fn reversed_saved_line_cannot_reuse_opposite_endpoint_names_from_the_shared_cache() -> TestResult {
    if !native() {
        return Ok(());
    }
    let base = rectangle();
    let mut f = Drawing::new(base.clone(), true);
    let mut k = OcctKernel::new()?;
    let context = OperationContext::default();
    let mut cache = CacheStore::open(
        f.root.path().join("shared-cache"),
        f.doc.meta().document_id,
        k.identity().id(),
        k.identity().version(),
    )?;
    let mut keys = vec![];
    for curves in [base.clone(), variants(&base).remove(2), base] {
        f.replace(curves);
        for run in 0..2 {
            let (built, events) = rebuild_cached(&f.doc, &mut k, &mut cache, &context)?;
            assert_eq!(check(&built, &f, &mut k, 240.), [6, 12, 8]);
            if run == 1 {
                assert!(
                    events
                        .iter()
                        .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
                );
                keys.push(built.result_key(f.feature).expect("key"));
            }
            built.release_all(&mut k);
        }
    }
    assert_ne!(keys[0], keys[1]);
    assert_eq!(keys[0], keys[2]);
    Ok(())
}
#[test]
fn clockwise_arc_agrees_in_drawing_region_containment_and_native_volume() -> TestResult {
    if !native() {
        return Ok(());
    }
    let mut curves = arc_loop(-PI);
    // This circle lies inside the rectangle but above the clockwise arc's boundary.
    // It is a separate region; reading the arc counter-clockwise makes it a hole.
    curves.push(curve(SketchGeometry::Circle {
        center: point(5., 7.),
        radius: 0.25,
    }));
    let f = Drawing::new(curves, true);
    let mut k = OcctKernel::new()?;
    let built = rebuild_cold(&f.doc, &mut k, &OperationContext::default())?;
    assert!(built.all_built(), "{:?}", built.report());
    let profile = built.profile(f.sketch).expect("profile");
    assert_eq!(profile.regions().len(), 2);
    assert!(profile.regions().iter().all(|r| r.inner.is_empty()));
    let drawing =
        ferritecad_scene::sketch_drawing(built.sketch_presentation(f.sketch).expect("drawing"))?;
    let arc = &drawing.strokes()[2];
    assert_eq!(arc.points().first().copied(), Some([10., 8., 0.]));
    let middle = arc.points()[arc.points().len() / 2];
    assert!((middle[0] - 5.).abs() < 1e-5 && (middle[1] - 3.).abs() < 1e-5);
    assert!((arc.points().last().expect("end")[0]).abs() < 1e-5);
    let shape = built.shape(f.body).expect("body");
    let (_, volume) = k.shape_stats(shape)?;
    assert_eq!(k.solid_count(shape)?, 2);
    assert!((volume - 3. * (80. - 12.5 * PI + PI * 0.25 * 0.25)).abs() < 1e-6);
    Ok(())
}

#[test]
fn scrambled_profile_revolves_about_its_stored_construction_line() -> TestResult {
    if !native() {
        return Ok(());
    }
    let mut base = rectangle();
    for c in &mut base {
        if let SketchGeometry::Line { start, end } = &mut c.geometry {
            start.x += 2.;
            end.x += 2.;
        }
    }
    let mut axis = line([0., -5.], [0., 15.]);
    axis.construction = true;
    let mut f = Drawing::new(vec![], true);
    put(
        &mut f.doc,
        f.feature,
        "Revolve",
        ObjectPayload::Revolve(Revolve {
            profile: f.sketch,
            regions: vec![],
            axis: RevolveAxis::Line(axis.id),
            extent: RevolveExtent::FullTurn,
            operation: SolidOperation::NewBody,
            previous: None,
        }),
    );
    let mut k = OcctKernel::new()?;
    let context = OperationContext::default();
    let mut reference: Option<f64> = None;
    for mut curves in [base.clone(), variants(&base).remove(5)] {
        curves.push(axis.clone());
        f.replace(curves);
        let saved = std::fs::read(&f.path)?;
        let built = rebuild_cold(&f.doc, &mut k, &context)?;
        assert!(built.all_built(), "{:?}", built.report());
        let shape = built.shape(f.body).expect("body");
        assert!(k.is_valid(shape)?);
        assert_eq!(k.solid_count(shape)?, 1);
        let volume = k.shape_stats(shape)?.1;
        assert!((volume - PI * (144. - 4.) * 8.).abs() < 1e-6);
        if let Some(expected) = reference {
            assert!((volume - expected).abs() < 1e-6);
        }
        reference = Some(volume);
        assert_eq!(std::fs::read(&f.path)?, saved);
        built.release_all(&mut k);
    }
    Ok(())
}
#[test]
fn directed_angle_solves_stored_directions_before_profile_reversal() -> TestResult {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return Ok(());
    }
    let curves = variants(&rectangle()).remove(2);
    let mut f = Drawing::new(curves.clone(), true);
    f.doc.write_changes(&[EditChange::AddConstraint {
        sketch: f.sketch,
        constraint: SketchConstraint {
            id: ferritecad_types::StableEntityId::new(),
            rule: SketchConstraintRule::DirectedAngle {
                a: SketchSegmentRef::new(
                    SketchPointRef::new(curves[0].id, SketchPointSelector::Start),
                    SketchPointRef::new(curves[0].id, SketchPointSelector::End),
                ),
                b: SketchSegmentRef::new(
                    SketchPointRef::new(curves[1].id, SketchPointSelector::Start),
                    SketchPointRef::new(curves[1].id, SketchPointSelector::End),
                ),
                angle: Expression::constant(-PI / 2.)?,
            },
        },
    }])?;
    let bytes = std::fs::read(&f.path)?;
    let mut k = OcctKernel::new()?;
    let built = rebuild_cold(&f.doc, &mut k, &OperationContext::default())?;
    check(&built, &f, &mut k, 240.);
    assert!(built.solve_report(f.sketch).is_some());
    for (actual, stored) in built
        .sketch_presentation(f.sketch)
        .expect("solved drawing")
        .curves()
        .iter()
        .zip(curves)
    {
        assert_eq!(actual.geometry(), &stored.geometry);
    }
    assert_eq!(std::fs::read(&f.path)?, bytes);
    Ok(())
}
