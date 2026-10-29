// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_bench::corpus::{LongFamily, generate_long};
use ferritecad_document::{Document, EditChange, ObjectPayload, Point2, SketchGeometry};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{GeometryKernel, NamingOperation, OperationContext, SubShapeKind};
use ferritecad_occt::OcctKernel;
use ferritecad_topology::{ModelNaming, archive_feature, generative, restore_feature};

fn assert_union(model: &ModelNaming) {
    let mut union = BTreeMap::new();
    for feature in model.features.values() {
        for (digest, node) in &feature.derivations {
            assert!(
                union.insert(*digest, node.clone()).is_none(),
                "one live owner per node"
            );
        }
        for (name, members) in &feature.ancestry {
            if members.is_empty() {
                assert!(
                    feature.derivations.contains_key(&name.digest),
                    "no inherited empty rows"
                );
            }
        }
    }
    assert_eq!(&union, model.derivations());
}

#[test]
fn own_nodes_are_unique_and_inherited_deletions_do_not_leave_rows() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let directory = tempfile::tempdir().expect("directory");
    let case = generate_long(directory.path(), LongFamily::Rework, 10).expect("history");
    let mut document = Document::open(&case.path).expect("document");
    let mut kernel = OcctKernel::new().expect("kernel");
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("cold");
    assert!(built.all_built());
    let model = built.topology();
    assert_union(model);
    let mut inherited = BTreeSet::new();
    for id in &case.features {
        let feature = &model.features[id];
        for digest in feature.derivations.keys() {
            assert!(
                inherited.insert(*digest),
                "no predecessor nodes in the current record"
            );
        }
    }
    let first = case.features[1];
    let second = case.features[2];
    let objects: BTreeMap<_, _> = document
        .objects()
        .expect("objects")
        .into_iter()
        .map(|o| (o.id, o))
        .collect();
    let ObjectPayload::Extrude(extrude) = &objects[&first].payload else {
        unreachable!()
    };
    let ObjectPayload::Sketch(sketch) = &objects[&extrude.profile].payload else {
        unreachable!()
    };
    let floor = generative::generated(
        first,
        NamingOperation::Extrude,
        generative::cap_root(first, true, Some(sketch.plane)),
        SubShapeKind::Face,
    );
    assert!(model.features[&first].resolve(floor).is_ok());
    assert!(!model.features[&second].ancestry.contains_key(&floor));
    assert!(model.features[&second].resolve(floor).is_err());
    assert!(model.derivations().contains_key(&floor.digest));
    let removed_edges: Vec<_> = model.features[&first]
        .bindings
        .keys()
        .filter(|name| {
            name.kind == SubShapeKind::Edge && !model.features[&second].ancestry.contains_key(name)
        })
        .copied()
        .collect();
    assert!(!removed_edges.is_empty());
    for edge in removed_edges {
        assert!(model.features[&second].resolve(edge).is_err());
        assert!(model.derivations().contains_key(&edge.digest));
    }

    let archive = archive_feature(&mut kernel, model, second).expect("archive");
    let mut destination = ModelNaming::new();
    destination.prepare(&objects).expect("prepare");
    let before = kernel.live_shape_count();
    assert!(restore_feature(&mut kernel, &archive, &mut destination).is_err());
    assert!(destination.derivations().is_empty());
    assert_eq!(
        kernel.live_shape_count(),
        before,
        "reject before publishing or leaking a shape"
    );
    for id in &case.features[..2] {
        let parent = archive_feature(&mut kernel, model, *id).expect("parent archive");
        restore_feature(&mut kernel, &parent, &mut destination).expect("predecessor first");
    }
    restore_feature(&mut kernel, &archive, &mut destination).expect("complete lineage");
    assert_eq!(
        destination.features[&second].derivations,
        model.features[&second].derivations
    );
    assert_union(&destination);
    for feature in destination.features.values() {
        kernel.release(feature.shape().expect("restored shape"));
    }
    let mut removed = model.clone();
    removed.remove_feature(second);
    assert_union(&removed);
    for digest in model.features[&second].derivations.keys() {
        assert!(!removed.derivations().contains_key(digest));
    }
    for digest in model.features[&first].derivations.keys() {
        assert_eq!(removed.derivations()[digest], model.derivations()[digest]);
    }
    built.release_all(&mut kernel);

    let ObjectPayload::Extrude(middle) = &objects[&case.features[5]].payload else {
        unreachable!()
    };
    let ObjectPayload::Sketch(profile) = &objects[&middle.profile].payload else {
        unreachable!()
    };
    document
        .write_changes(&[EditChange::SetCurveGeometry {
            sketch: middle.profile,
            curve: profile.curves[0].id,
            geometry: SketchGeometry::Circle {
                center: Point2::new(1000., 1000.).expect("center"),
                radius: 1.,
            },
        }])
        .expect("disjoint tool");
    let failed =
        rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("report");
    assert!(!failed.all_built());
    assert!(failed.topology().feature(case.features[5]).is_none());
    assert!(failed.topology().feature(case.features[4]).is_some());
    assert_union(failed.topology());
    failed.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn archive_bytes_do_not_depend_on_unrelated_body_order() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let a = tempfile::tempdir().expect("first body");
    let b = tempfile::tempdir().expect("second body");
    let cases = [
        generate_long(a.path(), LongFamily::Pockets, 10).expect("pockets"),
        generate_long(b.path(), LongFamily::Rework, 10).expect("rework"),
    ];
    let objects: BTreeMap<_, _> = cases
        .iter()
        .flat_map(|case| {
            Document::open_read_only(&case.path)
                .expect("document")
                .objects()
                .expect("objects")
        })
        .map(|object| (object.id, object))
        .collect();
    let build = |order: [usize; 2]| {
        let mut model = ModelNaming::new();
        model.prepare(&objects).expect("prepare both bodies");
        let mut kernel = OcctKernel::new().expect("kernel");
        let context = OperationContext::default();
        let mut archives = BTreeMap::new();
        for index in order {
            for owner in &cases[index].features {
                let ObjectPayload::Extrude(extrude) = &objects[owner].payload else {
                    unreachable!()
                };
                let ObjectPayload::Sketch(sketch) = &objects[&extrude.profile].payload else {
                    unreachable!()
                };
                let ObjectPayload::DatumPlane(plane) = &objects[&sketch.plane].payload else {
                    unreachable!()
                };
                let plane = ferritecad_eval::plane_from_datum(plane).expect("plane");
                let profile = ferritecad_eval::profile_from_sketch(sketch, extrude.profile, plane)
                    .expect("profile")
                    .profile
                    .select(&extrude.regions)
                    .expect("regions");
                model.select_profile(*owner, &profile).expect("roots");
                let request =
                    ferritecad_eval::extrude_request(extrude, profile, None).expect("extrusion");
                let tool = kernel.extrude(&request, &context).expect("tool");
                let tool_names = model
                    .name_operation(*owner, tool.shape, &[], &mut kernel)
                    .expect("tool names");
                let names = if let Some(previous) = extrude.previous {
                    let predecessor = &model.features[&previous];
                    let request = ferritecad_kernel::BooleanRequest::new(
                        ferritecad_kernel::BooleanKind::Cut,
                        predecessor.shape().expect("target"),
                        tool.shape,
                    )
                    .expect("boolean");
                    let result = kernel.boolean(&request, &context).expect("cut");
                    model
                        .name_operation(
                            *owner,
                            result.shape,
                            &[predecessor, &tool_names],
                            &mut kernel,
                        )
                        .expect("result names")
                } else {
                    tool_names
                };
                model.insert_feature(*owner, names).expect("insert");
                archives.insert(
                    *owner,
                    archive_feature(&mut kernel, &model, *owner)
                        .expect("archive")
                        .encode()
                        .expect("bytes"),
                );
            }
        }
        assert_union(&model);
        archives
    };
    assert_eq!(build([0, 1]), build([1, 0]));
}
