// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::{EntityKind, ObjectPayload};
use ferritecad_eval::rebuild_cold;
use ferritecad_fixtures::datum_planes::sweep_face;
use ferritecad_kernel::{GeometryKernel, NamingInput, OperationContext};
use ferritecad_occt::OcctKernel;

#[test]
fn parallel_surface_retains_blind_geometry_and_all_names() {
    for loops in 0..3 {
        compare_parallel_surface(loops);
    }
}

fn compare_parallel_surface(loops: usize) {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path).expect("fixture");
    let mut doc = ferritecad_document::Document::open(path).expect("document");
    if loops > 0 {
        let sketch = doc
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("sketch");
        let ObjectPayload::Sketch(old) = sketch.payload else {
            unreachable!()
        };
        let mut new =
            ferritecad_fixtures::datum_planes::circle(old.plane, [25., 20.], 3.).expect("circle");
        if loops == 2 {
            new.curves.extend(
                ferritecad_fixtures::datum_planes::circle(old.plane, [25., 20.], 1.)
                    .expect("hole")
                    .curves,
            );
        }
        doc.write(|w| {
            w.put_object(
                sketch.id,
                sketch.parent,
                sketch.ordinal,
                sketch.name.as_deref(),
                &ObjectPayload::Sketch(new),
            )
        })
        .expect("circular profile");
    }
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = rebuild_cold(&doc, &mut kernel, &context).expect("baseline");
    assert!(built.all_built());
    let feature = *built.topology().features.keys().next().expect("extrusion");
    let ObjectPayload::Extrude(extrude) = doc
        .object(feature)
        .expect("object")
        .expect("exists")
        .payload
    else {
        panic!("extrusion");
    };
    let reference =
        sweep_face(&built, &mut kernel, feature, NamingInput::Cap { end: true }).expect("cap");
    let faces = built
        .topology()
        .resolve(EntityKind::Face, feature, feature, &[reference])
        .expect("end cap");
    let result = kernel
        .extrude_to_face(
            built.profile(extrude.profile).expect("profile"),
            false,
            &faces,
            0.,
            &context,
        )
        .expect("up to face");
    assert!(kernel.is_valid(result.shape).expect("BRepCheck"));
    assert_eq!(kernel.solid_count(result.shape).expect("solids"), 1);
    let original = built.shape(feature).expect("shape");
    let a = kernel.shape_stats(original).expect("before").1;
    let b = kernel.shape_stats(result.shape).expect("after").1;
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
    let names = built
        .topology()
        .name_operation(feature, result.shape, &[], &mut kernel)
        .expect("complete provenance");
    let before = &built.topology().features[&feature];
    let signature = |n: &ferritecad_topology::generative::FeatureNaming| {
        n.bindings
            .iter()
            .map(|(name, entities)| (*name, entities.len()))
            .collect::<Vec<_>>()
    };
    assert_eq!(signature(before), signature(&names));
    kernel.release(result.shape);
    built.release_all(&mut kernel);
}

#[test]
fn curved_end_surface_preserves_cap_history_offset_and_full_coverage() {
    use ferritecad_kernel::{ExtrudeExtent, ExtrudeRequest, SketchPlane, SubShapeKind};
    use ferritecad_types::{ObjectId, Point3, Vec3};
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path).expect("fixture");
    let doc = ferritecad_document::Document::open(path).expect("document");
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = rebuild_cold(&doc, &mut kernel, &context).expect("baseline");
    let feature = *built.topology().features.keys().next().expect("extrusion");
    let ObjectPayload::Extrude(e) = doc
        .object(feature)
        .expect("object")
        .expect("exists")
        .payload
    else {
        panic!("extrusion");
    };
    for (radius, slope) in [(50., 0.), (50., 0.25), (10., 0.)] {
        let sketch = ferritecad_fixtures::datum_planes::circle(ObjectId::new(), [0., 0.], radius)
            .expect("roof section");
        let curve = sketch.curves[0].id;
        let plane = SketchPlane::new(
            Point3::new(0., 20., 100.).expect("axis origin"),
            Vec3::Y,
            Vec3::new(1., 0., slope).expect("axis"),
        )
        .expect("roof section plane");
        let profile = ferritecad_eval::profile_from_sketch(&sketch, ObjectId::new(), plane)
            .expect("profile")
            .profile;
        let roof = kernel
            .extrude(
                &ExtrudeRequest::new(
                    profile,
                    ExtrudeExtent::blind(60.).expect("roof length"),
                    false,
                ),
                &context,
            )
            .expect("roof");
        let faces: Vec<_> = kernel
            .naming_history(roof.shape)
            .expect("roof history")
            .relations
            .into_iter()
            .filter(|(i, _, _)| *i == NamingInput::Curve(curve))
            .filter_map(|(_, _, o)| o)
            .filter(|e| e.kind() == SubShapeKind::Face)
            .collect();
        assert_eq!(faces.len(), 1);
        for offset in [0., 2., -2.] {
            let result = kernel.extrude_to_face(
                built.profile(e.profile).expect("plate profile"),
                false,
                &faces,
                offset,
                &context,
            );
            if radius == 10. {
                let error = result.expect_err("roof narrower than profile");
                assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
                assert!(error.to_string().contains("does not cover"), "{error}");
                continue;
            }
            let shape = result.expect("curved end").shape;
            assert!(kernel.is_valid(shape).expect("BRepCheck"));
            let radius = radius + offset;
            let arc_area =
                20. * (radius * radius - 400.).sqrt() + radius * radius * (20. / radius).asin();
            let expected =
                60. * (40. * (100. + slope * 30.) - (1. + slope * slope).sqrt() * arc_area);
            let volume = kernel.shape_stats(shape).expect("volume").1;
            assert!((volume - expected).abs() < 1e-4, "{volume} != {expected}");
            let history = kernel.naming_history(shape).expect("end history");
            let end: Vec<_> = history
                .relations
                .iter()
                .filter(|(i, _, _)| *i == NamingInput::Cap { end: true })
                .filter_map(|(_, _, o)| *o)
                .collect();
            assert_eq!(end.len(), 1);
            assert_eq!(
                kernel.face_surface(end[0]).expect("type"),
                ferritecad_kernel::FaceSurface::Cylinder { radius }
            );
            let names = built
                .topology()
                .name_operation(feature, shape, &[], &mut kernel)
                .expect("all inputs retained");
            assert_eq!(
                names.entities.len(),
                kernel
                    .shape_topology(shape)
                    .expect("topology")
                    .adjacent_faces
                    .len()
            );
            kernel.release(shape);
        }
        kernel.release(roof.shape);
    }
    built.release_all(&mut kernel);
}

#[test]
fn moving_ceiling_and_vertex_keep_boss_names_and_invalidate_cached_geometry() {
    use ferritecad_document::*;
    use ferritecad_eval::{CacheOutcome, rebuild_cached};
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let mut doc = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let f = ferritecad_fixtures::up_to_end::ceiling(&mut doc, &mut kernel).expect("ceiling");
    let baseline = rebuild_cold(&doc, &mut kernel, &context).expect("blind");
    assert!(baseline.all_built());
    let signature = |built: &ferritecad_eval::RebuildResult| {
        built.topology().features[&f.boss.feature]
            .bindings
            .iter()
            .map(|(n, entities)| (*n, entities.len()))
            .collect::<Vec<_>>()
    };
    let blind_names = signature(&baseline);
    baseline.release_all(&mut kernel);
    let mut cache = CacheStore::open(
        root.path().join("cache.sqlite"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("cache");
    for (end, height) in [
        (f.face_end(0.).expect("end"), 20.),
        (f.face_end(2.).expect("end"), 18.),
        (f.face_end(-2.).expect("end"), 22.),
        (f.vertex_end(), 20.),
    ] {
        doc.write_changes(&[EditChange::SetEndCondition {
            object: f.boss.feature,
            end_condition: end,
        }])
        .expect("change end");
        let cold = rebuild_cold(&doc, &mut kernel, &context).expect("cold");
        assert!(cold.all_built(), "{:?}", cold.report());
        assert_eq!(signature(&cold), blind_names);
        let shape = cold.shape(f.boss.feature).expect("boss");
        let volume = kernel.shape_stats(shape).expect("volume").1;
        assert!((volume - (24000. + std::f64::consts::PI * 9. * height)).abs() < 1e-6);
        assert!(kernel.is_valid(shape).expect("BRepCheck"));
        for pass in 0..2 {
            let (warm, events) =
                rebuild_cached(&doc, &mut kernel, &mut cache, &context).expect("cache rebuild");
            assert!(warm.all_built(), "{:?}", warm.report());
            assert_eq!(signature(&warm), blind_names);
            let warm_volume = kernel
                .shape_stats(warm.shape(f.boss.feature).expect("warm boss"))
                .expect("warm volume")
                .1;
            assert!((volume - warm_volume).abs() < 1e-6);
            if pass == 1 {
                assert!(events.iter().all(|e| e.outcome == CacheOutcome::Hit));
            }
            warm.release_all(&mut kernel);
        }
        cold.release_all(&mut kernel);
    }
    doc.write_changes(&[
        EditChange::SetEndCondition {
            object: f.boss.feature,
            end_condition: f.face_end(0.).expect("end"),
        },
        EditChange::constant(f.plane, ExpressionSlot::DatumOffset, 30.).expect("raise ceiling"),
    ])
    .expect("move ceiling");
    let (raised, events) = rebuild_cached(&doc, &mut kernel, &mut cache, &context).expect("raised");
    assert!(raised.all_built(), "{:?}", raised.report());
    assert_eq!(signature(&raised), blind_names);
    assert!(events.iter().any(|e| e.outcome != CacheOutcome::Hit));
    let volume = kernel
        .shape_stats(raised.shape(f.boss.feature).expect("raised boss"))
        .expect("volume")
        .1;
    assert!((volume - (24000. + std::f64::consts::PI * 9. * 30.)).abs() < 1e-6);
    raised.release_all(&mut kernel);
}

#[test]
fn tilted_untrimmed_plane_and_invalid_surface_families_use_kernel_topology() {
    use ferritecad_kernel::{ExtrudeExtent, ExtrudeRequest, SketchPlane, SubShapeKind};
    use ferritecad_types::{ObjectId, Point3, Vec3};
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let doc = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = rebuild_cold(&doc, &mut kernel, &context).expect("baseline");
    let feature = *built.topology().features.keys().next().expect("extrusion");
    let ObjectPayload::Extrude(e) = doc
        .object(feature)
        .expect("object")
        .expect("exists")
        .payload
    else {
        panic!("extrusion");
    };
    let profile = built.profile(e.profile).expect("profile");
    for (origin, normal, accepted) in [
        (
            Point3::new(0., 0., 30.).expect("point"),
            Vec3::new(0., 0.5, 1.).expect("normal"),
            true,
        ),
        (Point3::new(0., 100., 0.).expect("point"), Vec3::Y, false),
        (Point3::new(0., 0., -10.).expect("point"), Vec3::Z, false),
    ] {
        let sketch = ferritecad_fixtures::datum_planes::circle(ObjectId::new(), [0., 0.], 1.)
            .expect("small support trim");
        let plane = SketchPlane::new(origin, Vec3::X, normal).expect("plane");
        let roof = ferritecad_eval::profile_from_sketch(&sketch, ObjectId::new(), plane)
            .expect("roof profile")
            .profile;
        let roof = kernel
            .extrude(
                &ExtrudeRequest::new(roof, ExtrudeExtent::blind(2.).expect("thickness"), false),
                &context,
            )
            .expect("roof");
        let history = kernel.naming_history(roof.shape).expect("history");
        let caps = |end| {
            history
                .relations
                .iter()
                .filter(|(i, _, _)| *i == NamingInput::Cap { end })
                .filter_map(|(_, _, o)| *o)
                .collect::<Vec<_>>()
        };
        let faces = caps(false);
        let result = kernel.extrude_to_face(profile, false, &faces, 0., &context);
        if accepted {
            let shape = result.expect("tilted plane").shape;
            assert!(kernel.is_valid(shape).expect("BRepCheck"));
            let volume = kernel.shape_stats(shape).expect("volume").1;
            assert!((volume - 60. * 40. * 20.).abs() < 1e-6, "{volume}");
            let names = built
                .topology()
                .name_operation(feature, shape, &[], &mut kernel)
                .expect("complete tilted provenance");
            assert_eq!(
                names.entities.len(),
                kernel
                    .shape_topology(shape)
                    .expect("topology")
                    .adjacent_faces
                    .len()
            );
            let end = kernel
                .naming_history(shape)
                .expect("end history")
                .relations
                .into_iter()
                .find(|(i, _, _)| *i == NamingInput::Cap { end: true })
                .and_then(|(_, _, o)| o)
                .expect("end cap");
            let measured = kernel.support_plane(&[end], &context).expect("plane");
            assert!(
                (measured.normal().y.abs() - normal.normalized().expect("unit").y).abs() < 1e-8
            );
            kernel.release(shape);
            let ObjectPayload::Sketch(original_sketch) = doc
                .object(e.profile)
                .expect("profile object")
                .expect("sketch")
                .payload
            else {
                panic!("sketch profile");
            };
            let circle_sketch =
                ferritecad_fixtures::datum_planes::circle(original_sketch.plane, [25., 20.], 3.)
                    .expect("circular boss");
            let circle =
                ferritecad_eval::profile_from_sketch(&circle_sketch, e.profile, *profile.plane())
                    .expect("boss profile")
                    .profile;
            let mut objects: std::collections::BTreeMap<_, _> = doc
                .objects()
                .expect("objects")
                .into_iter()
                .map(|object| (object.id, object))
                .collect();
            objects.get_mut(&e.profile).expect("sketch").payload =
                ObjectPayload::Sketch(circle_sketch);
            let mut circle_names = ferritecad_topology::generative::ModelNaming::default();
            circle_names.prepare(&objects).expect("circle roots");
            let boss = kernel
                .extrude_to_face(&circle, false, &faces, 0., &context)
                .expect("cylinder under tilted plane")
                .shape;
            assert!(kernel.is_valid(boss).expect("boss BRepCheck"));
            let volume = kernel.shape_stats(boss).expect("boss volume").1;
            assert!((volume - std::f64::consts::PI * 9. * 20.).abs() < 1e-6);
            let end = kernel
                .naming_history(boss)
                .expect("boss history")
                .relations
                .into_iter()
                .find(|(i, _, _)| *i == NamingInput::Cap { end: true })
                .and_then(|(_, _, o)| o)
                .expect("boss end cap");
            let measured = kernel
                .support_plane(&[end], &context)
                .expect("boss end plane");
            assert!(measured.normal().dot(normal.normalized().expect("unit")) > 1. - 1e-8);
            circle_names
                .name_operation(feature, boss, &[], &mut kernel)
                .expect("complete circular tilted provenance");
            kernel.release(boss);
        } else {
            let error = result.expect_err("not forward full coverage");
            assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
            assert!(error.to_string().contains("does not cover"), "{error}");
        }
        let both: Vec<_> = caps(false).into_iter().chain(caps(true)).collect();
        let error = kernel
            .extrude_to_face(profile, false, &both, 0., &context)
            .expect_err("different parallel surfaces");
        assert!(
            error.to_string().contains("do not lie on one surface"),
            "{error}"
        );
        kernel.release(roof.shape);
    }
    let mut annulus =
        ferritecad_fixtures::datum_planes::circle(ObjectId::new(), [0., 0.], 10.).expect("outer");
    annulus.curves.extend(
        ferritecad_fixtures::datum_planes::circle(annulus.plane, [0., 0.], 3.)
            .expect("inner")
            .curves,
    );
    let annulus = ferritecad_eval::profile_from_sketch(
        &annulus,
        ObjectId::new(),
        SketchPlane::new(Point3::ORIGIN, Vec3::X, Vec3::Z).expect("plane"),
    )
    .expect("annulus")
    .profile;
    let roof = kernel
        .extrude(
            &ExtrudeRequest::new(annulus, ExtrudeExtent::blind(20.).expect("height"), false),
            &context,
        )
        .expect("annular roof");
    let sides: Vec<_> = kernel
        .naming_history(roof.shape)
        .expect("history")
        .relations
        .into_iter()
        .filter(|(i, _, _)| matches!(i, NamingInput::Curve(_)))
        .filter_map(|(_, _, o)| o)
        .filter(|e| e.kind() == SubShapeKind::Face)
        .collect();
    assert_eq!(sides.len(), 2);
    let error = kernel
        .extrude_to_face(profile, false, &sides, 0., &context)
        .expect_err("different curved surfaces");
    assert!(
        error.to_string().contains("do not lie on one surface"),
        "{error}"
    );
    kernel.release(roof.shape);
    built.release_all(&mut kernel);
}

#[test]
fn a_consumed_boss_rim_survives_blind_to_face_and_vertex() {
    use ferritecad_document::*;
    use ferritecad_kernel::{NamingOperation, SubShapeKind};
    use ferritecad_topology::generative::{cap_root, generated};
    use ferritecad_types::ObjectId;
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let mut doc = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let f = ferritecad_fixtures::up_to_end::ceiling(&mut doc, &mut kernel).expect("ceiling");
    let before = rebuild_cold(&doc, &mut kernel, &context).expect("blind");
    let cap = generated(
        f.boss.feature,
        NamingOperation::Extrude,
        cap_root(f.boss.feature, true, Some(f.boss.plane)),
        SubShapeKind::Face,
    );
    let cap = before.topology().features[&f.boss.feature]
        .resolve(cap)
        .expect("top cap");
    let shape = before.shape(f.boss.feature).expect("boss");
    let topology = kernel.shape_topology(shape).expect("incidence");
    let edges: Vec<_> = topology
        .adjacent_faces
        .iter()
        .filter(|(e, faces)| e.kind() == SubShapeKind::Edge && faces.contains(&cap))
        .map(|(e, _)| *e)
        .collect();
    assert_eq!(edges.len(), 1);
    let name = before.topology().features[&f.boss.feature].entities[&edges[0]];
    let reference = EntityReference::new(ReferenceMeaning {
        producer: before.topology().producer(&name.digest).expect("owner"),
        kind: EntityKind::Edge,
        digest: name.digest,
        selection: Selection::One,
    });
    before.release_all(&mut kernel);
    let fillet = ObjectId::new();
    let body = ferritecad_fixtures::feature_changes::body(&doc).expect("body");
    doc.write_changes(&[EditChange::AddFeature {
        object: fillet,
        body,
        name: None,
        feature: FeatureAddition::Fillet {
            edges: vec![reference],
            radius_mm: Expression::constant(0.5).expect("radius"),
        },
    }])
    .expect("fillet");
    let blind = rebuild_cold(&doc, &mut kernel, &context).expect("blind fillet");
    assert!(blind.all_built(), "{:?}", blind.report());
    let signature = |built: &ferritecad_eval::RebuildResult| {
        built.topology().features[&fillet]
            .bindings
            .iter()
            .map(|(n, entities)| (*n, entities.len()))
            .collect::<Vec<_>>()
    };
    let original = signature(&blind);
    let original_volume = kernel
        .shape_stats(blind.shape(fillet).expect("fillet"))
        .expect("volume")
        .1;
    blind.release_all(&mut kernel);
    for end_condition in [f.face_end(0.).expect("end"), f.vertex_end()] {
        doc.write_changes(&[EditChange::SetEndCondition {
            object: f.boss.feature,
            end_condition,
        }])
        .expect("change end");
        let built = rebuild_cold(&doc, &mut kernel, &context).expect("updated");
        assert!(built.all_built(), "{:?}", built.report());
        assert_eq!(signature(&built), original);
        let shape = built.shape(fillet).expect("fillet persists");
        assert!(kernel.is_valid(shape).expect("BRepCheck"));
        assert!((kernel.shape_stats(shape).expect("volume").1 - original_volume).abs() < 1e-6);
        built.release_all(&mut kernel);
    }
}
