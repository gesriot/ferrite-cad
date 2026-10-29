// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::{Document, ObjectPayload};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{
    ChamferRequest, FilletRequest, GeometryKernel, NamingInput, NamingRelation, OperationContext,
    SubShapeKind,
};
use ferritecad_occt::OcctKernel;

#[test]
fn edge_sets_report_every_face_including_vertex_junctions() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    for chamfer in [false, true] {
        for all in [false, true] {
            let root = tempfile::tempdir().expect("directory");
            let path = root.path().join("plate.fcad");
            ferritecad_fixtures::write_plate(&path).expect("plate");
            let mut document = Document::open(&path).expect("document");
            let mut kernel = OcctKernel::new().expect("kernel");
            let context = OperationContext::default();
            let built = rebuild_cold(&document, &mut kernel, &context).expect("base");
            assert!(built.all_built());
            let mut model = built.topology().clone();
            let previous = *model.features.keys().next().expect("extrusion");
            let target = built.shape(previous).expect("shape");
            let history = kernel.naming_history(target).expect("sweep history");
            let top = history
                .relations
                .iter()
                .find_map(|(input, _, output)| {
                    (*input == NamingInput::Cap { end: true })
                        .then_some(*output)
                        .flatten()
                })
                .expect("top cap");
            let input = kernel.shape_topology(target).expect("input topology");
            let edges: Vec<_> = input
                .entities(SubShapeKind::Edge)
                .filter(|edge| all || input.adjacent_faces[edge].contains(&top))
                .collect();
            assert_eq!(edges.len(), if all { 12 } else { 4 });
            let result = if chamfer {
                kernel
                    .chamfer_edges(
                        &ChamferRequest::new(target, &edges, 1.).expect("request"),
                        &context,
                    )
                    .expect("chamfer")
            } else {
                kernel
                    .fillet_edges(
                        &FilletRequest::new(target, &edges, 1.).expect("request"),
                        &context,
                    )
                    .expect("fillet")
            };
            assert!(
                kernel
                    .is_valid(result.shape)
                    .expect("independent BRepCheck")
            );
            let output = kernel
                .shape_topology(result.shape)
                .expect("output topology");
            let history = kernel
                .naming_history(result.shape)
                .expect("operation history");
            let accounted: BTreeSet<_> = history
                .relations
                .iter()
                .filter_map(|(_, _, output)| *output)
                .filter(|entity| entity.kind() == SubShapeKind::Face)
                .collect();
            assert_eq!(accounted, output.entities(SubShapeKind::Face).collect());
            for edge in &edges {
                assert!(history.relations.iter().any(|(input, relation, output)| {
                    *input == NamingInput::SubShape(*edge)
                        && *relation == NamingRelation::Generated
                        && output.is_some_and(|entity| entity.kind() == SubShapeKind::Face)
                }));
            }
            let corners: BTreeSet<_> = history
                .relations
                .iter()
                .filter(|(input, relation, _)| {
                    matches!(input,
                    NamingInput::SubShape(entity) if entity.kind() == SubShapeKind::Vertex)
                        && *relation == NamingRelation::Generated
                })
                .filter_map(|(_, _, output)| *output)
                .filter(|entity| entity.kind() == SubShapeKind::Face)
                .collect();
            assert_eq!(corners.len(), if all { 8 } else { 0 });
            let body = ferritecad_fixtures::feature_changes::body(&document).expect("body");
            let change = ferritecad_fixtures::feature_changes::edge_addition(
                &document, body, 0, 1., chamfer,
            )
            .expect("feature");
            document
                .write_changes(&[change])
                .expect("add operation identity");
            let objects: BTreeMap<_, _> = document
                .objects()
                .expect("objects")
                .into_iter()
                .map(|object| (object.id, object))
                .collect();
            let owner = objects
                .values()
                .find(|o| {
                    matches!(
                        o.payload,
                        ObjectPayload::Fillet(_) | ObjectPayload::Chamfer(_)
                    )
                })
                .expect("owner")
                .id;
            model.prepare(&objects).expect("prepare lineage");
            let names = model
                .name_operation(
                    owner,
                    result.shape,
                    &[&model.features[&previous]],
                    &mut kernel,
                )
                .expect("name complete result");
            assert_eq!(names.entities.len(), output.adjacent_faces.len());
            model.insert_feature(owner, names).expect("publish names");
            kernel.release(result.shape);
            built.release_all(&mut kernel);
        }
    }
}

#[test]
fn fixture_references_round_all_four_plate_edges_in_one_feature() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let mut document = Document::open(&path).expect("document");
    let body = ferritecad_fixtures::feature_changes::body(&document).expect("body");
    let edges = (0..4)
        .map(|corner| {
            ferritecad_fixtures::feature_changes::plate_edge(&document, body, corner)
                .expect("reference from fixture derivation")
        })
        .collect();
    let feature = ferritecad_types::ObjectId::new();
    document
        .write_changes(&[ferritecad_document::EditChange::AddFeature {
            object: feature,
            body,
            name: None,
            feature: ferritecad_document::FeatureAddition::Fillet {
                edges,
                radius_mm: ferritecad_document::Expression::constant(2.).expect("radius"),
            },
        }])
        .expect("add");
    let mut kernel = OcctKernel::new().expect("kernel");
    let built =
        rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("rebuild");
    assert!(built.all_built(), "{:?}", built.report());
    let shape = built.shape(feature).expect("fillet");
    assert!(kernel.is_valid(shape).expect("independent BRepCheck"));
    let (_, volume) = kernel.shape_stats(shape).expect("volume");
    let expected = (60. * 40. - 4. * (4. - std::f64::consts::PI)) * 10.;
    assert!((volume - expected).abs() < 1e-7);
    built.release_all(&mut kernel);
}

fn reference(
    producer: ferritecad_types::ObjectId,
    name: ferritecad_topology::generative::Name,
) -> ferritecad_document::EntityReference {
    ferritecad_document::EntityReference::new(ferritecad_document::ReferenceMeaning {
        producer,
        kind: ferritecad_document::EntityKind::Edge,
        digest: name.digest,
        selection: ferritecad_document::Selection::One,
    })
}

fn append_edge_feature(
    document: &mut Document,
    body: ferritecad_types::ObjectId,
    edges: Vec<ferritecad_document::EntityReference>,
    chamfer: bool,
    amount: f64,
) -> ferritecad_types::ObjectId {
    use ferritecad_document::{EditChange, Expression, FeatureAddition};
    let id = ferritecad_types::ObjectId::new();
    let amount = Expression::constant(amount).expect("amount");
    document
        .write_changes(&[EditChange::AddFeature {
            object: id,
            body,
            name: None,
            feature: if chamfer {
                FeatureAddition::Chamfer {
                    edges,
                    distance_mm: amount,
                }
            } else {
                FeatureAddition::Fillet {
                    edges,
                    radius_mm: amount,
                }
            },
        }])
        .expect("append edge feature");
    id
}

fn check_cold_and_cache(document: &Document, kernel: &mut OcctKernel) {
    use ferritecad_eval::{CacheOutcome, rebuild_cached};
    let context = OperationContext::default();
    let cold = rebuild_cold(document, kernel, &context).expect("cold");
    assert!(cold.all_built(), "{:?}", cold.report());
    let signatures = |built: &ferritecad_eval::RebuildResult| -> BTreeMap<_, Vec<_>> {
        built
            .topology()
            .features
            .iter()
            .map(|(id, names)| {
                (
                    *id,
                    names
                        .ancestry
                        .iter()
                        .map(|(name, entities)| {
                            let members: BTreeSet<_> =
                                entities.iter().map(|e| names.entities[e]).collect();
                            (*name, members)
                        })
                        .collect(),
                )
            })
            .collect()
    };
    let expected = signatures(&cold);
    for names in cold.topology().features.values() {
        let shape = names.shape().expect("shape");
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
    let root = tempfile::tempdir().expect("cache directory");
    let mut cache = ferritecad_document::CacheStore::open(
        root.path().join("cache"),
        document.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("cache");
    for outcome in [CacheOutcome::Miss, CacheOutcome::Hit] {
        let (built, events) =
            rebuild_cached(document, kernel, &mut cache, &context).expect("cached");
        assert!(built.all_built(), "{:?}", built.report());
        assert!(events.iter().all(|e| e.outcome == outcome), "{events:?}");
        assert_eq!(signatures(&built), expected);
        built.release_all(kernel);
    }
    cold.release_all(kernel);
}

#[test]
fn pocket_rims_and_edges_modified_by_fillet_are_consumable_without_corner_policies() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_document::{EndCondition, Expression};
    use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool, plate_edge};
    use ferritecad_kernel::NamingOperation;
    use ferritecad_topology::generative::{cap_root, curve_root, generated};
    for floor in [false, true] {
        let root = tempfile::tempdir().expect("directory");
        let path = root.path().join("pocket.fcad");
        ferritecad_fixtures::write_plate(&path).expect("plate");
        let mut document = Document::open(&path).expect("document");
        let b = body(&document).expect("body");
        let (base, _, sketch) = base_profile(&document, b).expect("profile");
        let cut = circular_tool(
            &document,
            b,
            [20., 20.],
            5.,
            EndCondition::Blind {
                distance: Expression::constant(4.).expect("depth"),
            },
        )
        .expect("pocket");
        document.write_changes(&cut.changes).expect("cut");
        let mut kernel = OcctKernel::new().expect("kernel");
        let built =
            rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("cut");
        assert!(built.all_built());
        let names = &built.topology().features[&cut.feature];
        let cap = generated(
            if floor { cut.feature } else { base },
            NamingOperation::Extrude,
            cap_root(
                if floor { cut.feature } else { base },
                floor,
                Some(sketch.plane),
            ),
            SubShapeKind::Face,
        );
        let wall = generated(
            cut.feature,
            NamingOperation::Extrude,
            curve_root(cut.feature, cut.curve, Some(sketch.plane)),
            SubShapeKind::Face,
        );
        let cap = names.resolve(cap).expect("cap");
        let wall = names.resolve(wall).expect("wall");
        let rim: Vec<_> = names
            .topology
            .entities(SubShapeKind::Edge)
            .filter(|e| {
                let faces = &names.topology.adjacent_faces[e];
                faces.contains(&cap) && faces.contains(&wall)
            })
            .collect();
        assert_eq!(rim.len(), 1, "one circular intersection of the named faces");
        let edge = reference(cut.feature, names.entities[&rim[0]]);
        let before_volume = kernel
            .shape_stats(built.shape(cut.feature).expect("cut"))
            .expect("stats")
            .1;
        built.release_all(&mut kernel);
        let fillet = append_edge_feature(&mut document, b, vec![edge], false, 0.5);
        let built =
            rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("round");
        assert!(built.all_built(), "{:?}", built.report());
        let volume = kernel
            .shape_stats(built.shape(fillet).expect("fillet"))
            .expect("stats")
            .1;
        assert!(if floor {
            volume > before_volume
        } else {
            volume < before_volume
        });
        built.release_all(&mut kernel);
        check_cold_and_cache(&document, &mut kernel);
    }
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("changed-edge.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let mut document = Document::open(&path).expect("document");
    let b = body(&document).expect("body");
    let (base, _, sketch) = base_profile(&document, b).expect("profile");
    let mut kernel = OcctKernel::new().expect("kernel");
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("base");
    let names = &built.topology().features[&base];
    let top = names
        .resolve(generated(
            base,
            NamingOperation::Extrude,
            cap_root(base, true, Some(sketch.plane)),
            SubShapeKind::Face,
        ))
        .expect("top");
    let side = names
        .resolve(generated(
            base,
            NamingOperation::Extrude,
            curve_root(base, sketch.curves[0].id, Some(sketch.plane)),
            SubShapeKind::Face,
        ))
        .expect("side");
    let top_edge = names
        .topology
        .entities(SubShapeKind::Edge)
        .find(|e| {
            names.topology.adjacent_faces[e].contains(&top)
                && names.topology.adjacent_faces[e].contains(&side)
        })
        .expect("top side edge");
    let original = names.entities[&top_edge];
    built.release_all(&mut kernel);
    let edge = plate_edge(&document, b, 0).expect("vertical edge");
    let fillet = append_edge_feature(&mut document, b, vec![edge], false, 2.);
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("fillet");
    assert!(built.all_built(), "{:?}", built.report());
    let names = &built.topology().features[&fillet];
    let changed = names.resolve(original).expect("modified top edge survives");
    let old = built.topology().features[&base]
        .resolve(original)
        .expect("original edge");
    let trace = kernel
        .naming_history(built.shape(fillet).expect("fillet"))
        .expect("history");
    assert!(trace.relations.contains(&(
        NamingInput::SubShape(old),
        NamingRelation::Modified,
        Some(changed)
    )));
    // A one-to-one modification keeps the name, so the Extrude that minted it
    // stays the producer of the edge the Fillet changed.
    assert_eq!(names.entities[&changed], original);
    assert_eq!(built.topology().producer(&original.digest), Some(base));
    let edge = reference(base, original);
    built.release_all(&mut kernel);
    append_edge_feature(&mut document, b, vec![edge], true, 0.25);
    check_cold_and_cache(&document, &mut kernel);
}

#[test]
fn an_intervening_cut_preserves_or_splits_the_consumed_edge_without_retargeting() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_document::{EndCondition, Selection};
    use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool};
    use ferritecad_kernel::NamingOperation;
    use ferritecad_topology::generative::{cap_root, curve_root, generated};
    for split in [false, true] {
        let temp = tempfile::tempdir().expect("directory");
        let path = temp.path().join("split.fcad");
        ferritecad_fixtures::write_plate(&path).expect("plate");
        let mut d = Document::open(&path).expect("document");
        let b = body(&d).expect("body");
        let (base, _, sketch) = base_profile(&d, b).expect("profile");
        let mut k = OcctKernel::new().expect("kernel");
        let built = rebuild_cold(&d, &mut k, &OperationContext::default()).expect("base");
        let names = &built.topology().features[&base];
        let cap = names
            .resolve(generated(
                base,
                NamingOperation::Extrude,
                cap_root(base, true, Some(sketch.plane)),
                SubShapeKind::Face,
            ))
            .expect("top");
        let side = names
            .resolve(generated(
                base,
                NamingOperation::Extrude,
                curve_root(base, sketch.curves[0].id, Some(sketch.plane)),
                SubShapeKind::Face,
            ))
            .expect("side");
        let edges: Vec<_> = names
            .topology
            .entities(SubShapeKind::Edge)
            .filter(|e| {
                let faces = &names.topology.adjacent_faces[e];
                faces.contains(&cap) && faces.contains(&side)
            })
            .collect();
        assert_eq!(edges.len(), 1);
        let mut edge = reference(base, names.entities[&edges[0]]);
        built.release_all(&mut k);
        let cut = circular_tool(
            &d,
            b,
            [30., if split { 0. } else { 20. }],
            4.,
            EndCondition::ThroughAll,
        )
        .expect("cut");
        d.write_changes(&cut.changes).expect("insert cut");
        let fillet = append_edge_feature(&mut d, b, vec![edge], false, 0.5);
        if split {
            let built = rebuild_cold(&d, &mut k, &OperationContext::default()).expect("split");
            assert!(!built.all_built());
            assert!(built.shape(cut.feature).is_some());
            let error = built
                .topology()
                .resolve_edges(fillet, cut.feature, &[edge])
                .expect_err("One refuses both descendants")
                .to_string();
            assert!(error.contains("split into 2"), "{error}");
            assert!(error.contains(&fillet.to_string()) && error.contains(&edge.id.to_string()));
            built.release_all(&mut k);
            edge.meaning.selection = Selection::AllDescendants;
            d.write_changes(&[ferritecad_document::EditChange::SetEdges {
                object: fillet,
                edges: vec![edge],
            }])
            .expect("explicit descendant set");
            let built = rebuild_cold(&d, &mut k, &OperationContext::default()).expect("both");
            let resolved = built
                .topology()
                .resolve_edges(fillet, cut.feature, &[edge])
                .expect("resolve both");
            assert_eq!(resolved.len(), 2);
            let history = k
                .naming_history(built.shape(fillet).expect("fillet"))
                .expect("history");
            for source in resolved {
                assert!(history.relations.iter().any(|(input, relation, output)| {
                    *input == NamingInput::SubShape(source)
                        && *relation == NamingRelation::Generated
                        && output.is_some_and(|e| e.kind() == SubShapeKind::Face)
                }));
            }
            built.release_all(&mut k);
        } else {
            // The Cut holds the kept edge in its result but did not mint its
            // name, so it is not the reference's producer.
            let built = rebuild_cold(&d, &mut k, &OperationContext::default()).expect("kept");
            let mut observed = edge;
            observed.meaning.producer = cut.feature;
            let error = built
                .topology()
                .resolve_edges(fillet, cut.feature, &[observed])
                .expect_err("only the minting feature is the producer")
                .to_string();
            assert!(error.contains("unknown name"), "{error}");
            built.release_all(&mut k);
        }
        check_cold_and_cache(&d, &mut k);
    }
}

#[test]
fn a_revolved_edge_is_selected_by_its_named_adjacent_faces() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_document::{Revolve, RevolveAxis, RevolveExtent, SolidOperation};
    use ferritecad_fixtures::feature_changes::{base_profile, body};
    use ferritecad_kernel::NamingOperation;
    use ferritecad_topology::generative::{curve_root, generated};
    let temp = tempfile::tempdir().expect("directory");
    let path = temp.path().join("revolved.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let mut d = Document::open(&path).expect("document");
    let b = body(&d).expect("body");
    let (base, profile, sketch) = base_profile(&d, b).expect("profile");
    let original = d.object(base).expect("read").expect("extrusion");
    d.write(|w| {
        w.put_object(
            base,
            original.parent,
            original.ordinal,
            original.name.as_deref(),
            &ObjectPayload::Revolve(Revolve {
                profile,
                regions: vec![],
                previous: None,
                operation: SolidOperation::NewBody,
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::FullTurn,
            }),
        )
    })
    .expect("revolved fixture");
    let mut k = OcctKernel::new().expect("kernel");
    let built = rebuild_cold(&d, &mut k, &OperationContext::default()).expect("revolve");
    assert!(built.all_built(), "{:?}", built.report());
    let names = &built.topology().features[&base];
    let faces: Vec<_> = [1, 2]
        .into_iter()
        .map(|i| {
            names
                .resolve(generated(
                    base,
                    NamingOperation::Revolve,
                    curve_root(base, sketch.curves[i].id, Some(sketch.plane)),
                    SubShapeKind::Face,
                ))
                .expect("generated face")
        })
        .collect();
    let edges: Vec<_> = names
        .topology
        .entities(SubShapeKind::Edge)
        .filter(|e| {
            faces
                .iter()
                .all(|f| names.topology.adjacent_faces[e].contains(f))
        })
        .collect();
    assert_eq!(edges.len(), 1);
    let edge = reference(base, names.entities[&edges[0]]);
    built.release_all(&mut k);
    append_edge_feature(&mut d, b, vec![edge], false, 1.);
    check_cold_and_cache(&d, &mut k);
}
