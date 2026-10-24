// SPDX-License-Identifier: MIT
#![allow(
    clippy::panic,
    reason = "a mismatch reports both resolutions in the test failure"
)]
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::{Document, EditChange, Expression, ObjectPayload};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{OperationContext, SubShapeKind};
use ferritecad_occt::OcctKernel;
use ferritecad_topology::generative::{ModelNaming, name_rebuilt};

fn names(document: &Document) -> ModelNaming {
    let mut kernel = OcctKernel::new().expect("native kernel");
    let built = rebuild_cold(document, &mut kernel, &OperationContext::default()).expect("rebuild");
    assert!(built.all_built(), "{:?}", built.report());
    let shapes = built
        .order()
        .iter()
        .filter_map(|id| built.shape(*id).map(|shape| (*id, shape)))
        .collect();
    let names =
        name_rebuilt(document, &shapes, &mut kernel).expect("complete operation provenance");
    for reference in document.topology_refs().expect("stored references") {
        let old = ferritecad_topology::resolve(built.topology(), &reference)
            .map(|v| v.into_iter().collect::<BTreeSet<_>>());
        let new = names.resolve_legacy(&reference);
        match (old, new) {
            (Ok(old), Ok(new)) => assert_eq!(old, new, "reference {reference:?}"),
            (Err(_), Err(_)) => {}
            (old, new) => panic!("reference {reference:?}: old={old:?}, new={new:?}"),
        }
    }
    for object in document.objects().expect("objects") {
        if let ObjectPayload::Extrude(e) = &object.payload
            && e.previous.is_some()
        {
            use ferritecad_document::{
                CapSide, EntityKind, SelectionRule, SemanticRole, TopologyRef,
            };
            let curve = built
                .profile(e.profile)
                .expect("tool profile")
                .outer()
                .segments()[0]
                .label;
            let old_undefined = TopologyRef {
                id: ferritecad_types::StableEntityId::new(),
                owner: object.id,
                producer_feature: object.id,
                expected_kind: EntityKind::Edge,
                output_role: SemanticRole::ExtrudeCapEdge {
                    side: CapSide::End,
                    profile_segment: curve,
                },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            };
            assert!(ferritecad_topology::resolve(built.topology(), &old_undefined).is_err());
            assert!(
                names.resolve_legacy(&old_undefined).is_err(),
                "a previously undefined role never acquires a neighbour"
            );
        }
        let (previous, edge) = match object.payload {
            ObjectPayload::Fillet(f) => (f.previous, f.edge),
            ObjectPayload::Chamfer(c) => (c.previous, c.edge),
            _ => continue,
        };
        let old = built
            .topology()
            .feature(previous)
            .expect("predecessor names");
        let old: BTreeSet<_> = if previous == edge.feature {
            old.sweep_edge(edge.joint).collect()
        } else {
            old.origin_sweep_edge(edge.feature, edge.joint).collect()
        };
        let new = names.swept_edge(previous, edge).expect("generative edge");
        assert_eq!(old, BTreeSet::from([new]), "consumed edge of {}", object.id);
    }
    for (feature, result) in &names.features {
        for entity in result.topology.adjacent_faces.keys() {
            let name = result.entities[entity];
            match result.resolve(name) {
                Ok(resolved) => assert_eq!(resolved, *entity),
                Err(_) => assert!(
                    result
                        .ambiguous()
                        .any(|(ambiguous, _, _)| ambiguous == name)
                ),
            }
        }
        eprintln!(
            "feature {feature}: faces={} edges={} vertices={} ambiguous={:?}",
            result.topology.entities(SubShapeKind::Face).count(),
            result.topology.entities(SubShapeKind::Edge).count(),
            result.topology.entities(SubShapeKind::Vertex).count(),
            result.ambiguous().collect::<Vec<_>>()
        );
    }
    let mut body_names = BTreeSet::new();
    for object in document.objects().expect("objects") {
        if let ObjectPayload::Body(body) = object.payload
            && let Some(tip) = body.tip_feature
        {
            for name in names.features[&tip].bindings.keys() {
                assert!(body_names.insert(*name), "two bodies share {name:?}");
            }
        }
    }
    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
    names
}

fn signatures(
    names: &ModelNaming,
) -> BTreeMap<ferritecad_types::ObjectId, BTreeMap<ferritecad_topology::generative::Name, usize>> {
    names
        .features
        .iter()
        .map(|(id, result)| {
            (
                *id,
                result
                    .bindings
                    .iter()
                    .map(|(name, entities)| (*name, entities.len()))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn name_every_corpus_entity_and_preserve_names_across_dimension_edits() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let dir = tempfile::tempdir().expect("corpus dir");
    let mut cases = ferritecad_bench::corpus::generate(dir.path(), false).expect("corpus");
    let path = dir.path().join("chamfer.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let mut document = Document::open(&path).expect("open");
    let body = ferritecad_fixtures::feature_changes::body(&document).expect("body");
    let change = ferritecad_fixtures::feature_changes::edge_addition(&document, body, 0, 2., true)
        .expect("chamfer");
    document.write_changes(&[change]).expect("add chamfer");
    document.close().expect("close");
    cases.push(ferritecad_bench::corpus::Case {
        name: "chamfer".into(),
        path,
        bodies: 1,
        body,
        extrude: None,
        fillet: None,
        revolve: None,
        additions: Vec::new(),
        vertex: None,
    });
    for case in cases {
        if case.name.contains("constraints")
            && !ferritecad_fixtures::native_dependencies(&[(
                ferritecad_sketch_solver::is_available(),
                "PLANEGCS",
            )])
        {
            continue;
        }
        eprintln!("CASE {}", case.name);
        let mut document = Document::open(&case.path).expect("open");
        let before = signatures(&names(&document));
        assert_eq!(
            before,
            signatures(&names(&document)),
            "deterministic {}",
            case.name
        );
        let objects = document.objects().expect("objects");
        let mut edits = Vec::new();
        if let Some((sketch, profile, vertices)) = &case.vertex {
            edits.push(
                ferritecad_document::move_sketch_vertices(*sketch, profile, vertices)
                    .expect("vertex edit"),
            );
        }
        if let Some(object) = case.fillet {
            edits.push(vec![EditChange::SetExpression {
                object,
                slot: ferritecad_document::ExpressionSlot::FilletRadius,
                expression: Expression::constant(1.5).expect("radius"),
            }]);
        }
        if let Some(object) = case.revolve {
            edits.push(vec![EditChange::SetExpression {
                object,
                slot: ferritecad_document::ExpressionSlot::RevolveAngle,
                expression: Expression::constant(2.4).expect("angle"),
            }]);
        }
        let edited = objects.iter().find_map(|o| match &o.payload {
            ObjectPayload::Extrude(e) if e.previous.is_none() => Some(EditChange::SetExpression {
                object: o.id,
                slot: ferritecad_document::ExpressionSlot::ExtrudeDistance,
                expression: Expression::constant(12.).expect("height"),
            }),
            _ => None,
        });
        if let Some(change) = edited {
            edits.push(vec![change]);
        }
        for changes in edits {
            document.write_changes(&changes).expect("dimension edit");
            assert_eq!(
                before,
                signatures(&names(&document)),
                "stable {}",
                case.name
            );
        }
        if let Some(revolve) = case.revolve {
            let objects = document.objects().expect("revolve objects");
            document
                .write(|writer| {
                    for mut object in objects {
                        if object.id != revolve {
                            continue;
                        }
                        if let ObjectPayload::Revolve(r) = &mut object.payload {
                            r.extent = ferritecad_document::RevolveExtent::FullTurn;
                        }
                        writer.put_object(
                            object.id,
                            object.parent,
                            object.ordinal,
                            object.name.as_deref(),
                            &object.payload,
                        )?;
                    }
                    Ok(())
                })
                .expect("full revolution");
            let full = signatures(&names(&document));
            assert_ne!(before, full);
            assert_eq!(full, signatures(&names(&document)));
        }
    }
}

#[test]
fn name_boolean_intersections_pocket_births_and_periodic_seams() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_document::{EndCondition, Point2, SketchGeometry};
    use ferritecad_fixtures::feature_changes::{body, circular_tool};
    let dir = tempfile::tempdir().expect("dir");
    for (case, center) in [
        ("open-slot", [1., 20.]),
        ("overlap", [20., 20.]),
        ("pocket", [20., 20.]),
    ] {
        eprintln!("CASE {case}");
        let path = dir.path().join(format!("{case}.fcad"));
        ferritecad_fixtures::write_plate(&path).expect("plate");
        let mut doc = Document::open(&path).expect("open");
        let b = body(&doc).expect("body");
        let initial_center = if case == "open-slot" {
            [5., 20.]
        } else {
            center
        };
        let cut =
            circular_tool(&doc, b, initial_center, 2., EndCondition::ThroughAll).expect("tool");
        let mut changes = cut.changes;
        if case == "overlap" {
            changes.extend(
                circular_tool(&doc, b, [22., 20.], 2., EndCondition::ThroughAll)
                    .expect("overlap tool")
                    .changes,
            );
        }
        doc.write_changes(&changes).expect("add");
        if case == "open-slot" {
            use ferritecad_topology::generative::{Selection, curve_root};
            let (base, _, sketch) =
                ferritecad_fixtures::feature_changes::base_profile(&doc, b).expect("base");
            let inside = names(&doc);
            let side_name = |curve| {
                let root = curve_root(base, curve, Some(sketch.plane));
                let face = inside.features[&base].ancestry[&root]
                    .iter()
                    .find(|s| s.kind() == SubShapeKind::Face)
                    .expect("side face");
                inside.features[&base].entities[face]
            };
            let split_wall = side_name(sketch.curves[3].id);
            let untouched_wall = side_name(sketch.curves[1].id);
            assert!(inside.features[&cut.feature].resolve(split_wall).is_ok());
            doc.write_changes(&[EditChange::SetCurveGeometry {
                sketch: cut.sketch,
                curve: cut.curve,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(center[0], center[1]).expect("slot center"),
                    radius: 2.,
                },
            }])
            .expect("open slot");
            let opened = names(&doc);
            assert!(
                opened.features[&cut.feature].resolve(split_wall).is_err(),
                "never pick one wall fragment"
            );
            assert_eq!(
                opened.features[&cut.feature]
                    .resolve_set(split_wall, Selection::AllDescendants)
                    .expect("all wall parts")
                    .len(),
                2
            );
            assert!(
                opened.features[&cut.feature]
                    .resolve(untouched_wall)
                    .is_ok()
            );
        }
        let before = signatures(&names(&doc));
        if case == "pocket" {
            doc.write_changes(&[EditChange::SetEndCondition {
                object: cut.feature,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(4.).expect("depth"),
                },
            }])
            .expect("blind");
            let pocket_names = names(&doc);
            let floor_reference = doc
                .topology_refs()
                .expect("refs")
                .into_iter()
                .find(|r| {
                    r.producer_feature == cut.feature
                        && matches!(
                            r.output_role,
                            ferritecad_document::SemanticRole::ExtrudeCap {
                                side: ferritecad_document::CapSide::End
                            }
                        )
                })
                .expect("floor reference");
            let floor = *pocket_names
                .resolve_legacy(&floor_reference)
                .expect("floor")
                .first()
                .expect("one floor");
            let floor_name = pocket_names.features[&cut.feature].entities[&floor];
            let pocket = signatures(&pocket_names);
            assert_ne!(before[&cut.feature], pocket[&cut.feature]);
            doc.write_changes(&[EditChange::SetEndCondition {
                object: cut.feature,
                end_condition: EndCondition::ThroughAll,
            }])
            .expect("through");
            let reopened = names(&doc);
            assert!(
                reopened.features[&cut.feature].resolve(floor_name).is_err(),
                "a removed floor never becomes a wall"
            );
            assert_eq!(before, signatures(&reopened));
        } else {
            doc.write_changes(&[EditChange::SetCurveGeometry {
                sketch: cut.sketch,
                curve: cut.curve,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(center[0], center[1] + 0.5).expect("center"),
                    radius: 2.1,
                },
            }])
            .expect("edit tool");
            assert_eq!(before, signatures(&names(&doc)), "stable {case}");
        }
    }
}

#[test]
fn keep_a_reference_compact_after_fifty_generations() {
    use ciborium::value::Value;
    use ferritecad_topology::generative::{curve_root, generated};
    let feature = ferritecad_types::ObjectId::new();
    let mut name = curve_root(feature, ferritecad_types::StableEntityId::new(), None);
    assert_ne!(
        generated(
            feature,
            ferritecad_kernel::NamingOperation::Extrude,
            name,
            SubShapeKind::Face
        ),
        generated(
            feature,
            ferritecad_kernel::NamingOperation::Revolve,
            name,
            SubShapeKind::Face
        )
    );
    for _ in 0..50 {
        let producer = ferritecad_types::ObjectId::new();
        name = generated(
            producer,
            ferritecad_kernel::NamingOperation::Cut,
            name,
            SubShapeKind::Face,
        );
        let reference = Value::Array(vec![
            Value::Bytes(producer.to_bytes().to_vec()),
            Value::Integer(0.into()),
            Value::Bytes(name.digest.as_bytes().to_vec()),
            Value::Integer(0.into()),
        ]);
        let mut cbor = Vec::new();
        ciborium::ser::into_writer(&reference, &mut cbor).expect("encode reference");
        assert_eq!(cbor.len(), 54);
        let decoded: Value = ciborium::de::from_reader(cbor.as_slice()).expect("decode reference");
        assert_eq!(reference, decoded);
    }
}

#[test]
fn restore_all_name_sets_with_their_brep_without_replaying_history() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_kernel::GeometryKernel;
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let document = Document::open(&path).expect("open");
    let mut kernel = OcctKernel::new().expect("kernel");
    let built =
        rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("rebuild");
    let shapes = built
        .order()
        .iter()
        .filter_map(|id| built.shape(*id).map(|shape| (*id, shape)))
        .collect();
    let model = name_rebuilt(&document, &shapes, &mut kernel).expect("names");
    let named = model.features.values().next().expect("extrusion");
    let entities: Vec<_> = named.entities.keys().copied().collect();
    let shape = entities[0].shape();
    let (blob, slots) = kernel.encode_shape_with(shape, &entities).expect("encode");
    built.release_all(&mut kernel);
    let (restored_shape, restored) = kernel.decode_shape_with(&blob, &slots).expect("decode");
    assert!(
        kernel.naming_history(restored_shape).is_err(),
        "geometry does not invent history"
    );
    let rebinding: BTreeMap<_, _> = entities.into_iter().zip(restored).collect();
    for (name, members) in &named.bindings {
        let returned: BTreeSet<_> = members.iter().map(|s| rebinding[s]).collect();
        assert_eq!(returned.len(), members.len(), "{name:?}");
        assert!(
            returned
                .iter()
                .all(|s| s.shape() == restored_shape && s.kind() == name.kind)
        );
    }
    // A decoded input has no registered sub-shapes except its archive slots.
    // Complete history must not depend on a caller enumerating or tracking them.
    use ferritecad_kernel::{
        CutRequest, ExtrudeExtent, ExtrudeRequest, NamingInput, NamingRelation, PlanarPoint,
        Profile, ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane,
    };
    let circle = ProfileSegment::new(
        ferritecad_types::StableEntityId::new(),
        SegmentGeometry::circle(PlanarPoint::new(20., 20.).expect("center"), 2.).expect("circle"),
    );
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::closed_curve(circle).expect("loop"),
        Vec::new(),
    )
    .expect("profile");
    let tool = kernel
        .extrude(
            &ExtrudeRequest::new(profile, ExtrudeExtent::blind(12.).expect("depth"), false),
            &OperationContext::default(),
        )
        .expect("tool");
    let plain_blob = kernel.encode_shape(restored_shape).expect("plain B-Rep");
    let bare = kernel
        .decode_shape(&plain_blob)
        .expect("input without named slots");
    let cut = kernel
        .cut(
            &CutRequest::new(bare, tool.shape).expect("cut request"),
            &[],
            &OperationContext::default(),
        )
        .expect("cut");
    assert!(cut.history.is_empty(), "legacy tracking remains opt-in");
    let trace = kernel.naming_history(cut.shape).expect("full history");
    for input in [bare, tool.shape] {
        let topology = kernel.shape_topology(input).expect("input inventory");
        for entity in topology.adjacent_faces.keys() {
            assert!(
                trace
                    .relations
                    .iter()
                    .any(|(source, _, _)| *source == NamingInput::SubShape(*entity)),
                "missing {entity}"
            );
        }
    }
    assert!(
        trace.relations.iter().any(
            |(_, relation, output)| *relation == NamingRelation::Generated
                && output.is_some_and(|s| s.kind() == SubShapeKind::Edge)
        ),
        "boolean boundary provenance"
    );
    use ferritecad_kernel::{ChamferRequest, FilletRequest, NamingOperation};
    let selected = *rebinding
        .values()
        .find(|s| s.kind() == SubShapeKind::Edge)
        .expect("edge");
    let (one_edge_blob, one_edge_slots) = kernel
        .encode_shape_with(restored_shape, &[selected])
        .expect("one named edge");
    for chamfer in [false, true] {
        let (input, restored_edge) = kernel
            .decode_shape_with(&one_edge_blob, &one_edge_slots)
            .expect("partial named input");
        let edge = restored_edge[0];
        let result = if chamfer {
            kernel
                .chamfer_edge(
                    &ChamferRequest::new(input, edge, 1.).expect("chamfer request"),
                    &[],
                    &OperationContext::default(),
                )
                .expect("chamfer")
                .shape
        } else {
            kernel
                .fillet_edge(
                    &FilletRequest::new(input, edge, 1.).expect("fillet request"),
                    &[],
                    &OperationContext::default(),
                )
                .expect("fillet")
                .shape
        };
        let history = kernel
            .naming_history(result)
            .expect("complete local history");
        assert_eq!(
            history.operation,
            Some(if chamfer {
                NamingOperation::Chamfer
            } else {
                NamingOperation::Fillet
            })
        );
        for entity in kernel
            .shape_topology(input)
            .expect("all input entities")
            .adjacent_faces
            .keys()
        {
            assert!(
                history
                    .relations
                    .iter()
                    .any(|(source, _, _)| *source == NamingInput::SubShape(*entity)),
                "missing {entity}"
            );
        }
        kernel.release(result);
        kernel.release(input);
    }
    kernel.release(cut.shape);
    kernel.release(bare);
    kernel.release(tool.shape);
    kernel.release(restored_shape);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn match_the_committed_fixture_and_unconstrained_fillet_chain() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let dir = tempfile::tempdir().expect("dir");
    let plate = dir.path().join("committed.fcad");
    std::fs::copy(ferritecad_fixtures::plate_source(), &plate).expect("copy fixture");
    names(&Document::open_read_only(plate).expect("fixture"));
    let path = dir.path().join("fillet-chain.fcad");
    ferritecad_fixtures::write_fillet_chain(&path, 2.).expect("chain");
    names(&Document::open_read_only(path).expect("chain"));
}

#[test]
fn translate_cap_edges_sweep_edges_and_cap_vertices_independently() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_document::{CapSide, EntityKind, SelectionRule, SemanticRole, TopologyRef};
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let document = Document::open_read_only(&path).expect("open");
    let mut kernel = OcctKernel::new().expect("kernel");
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("build");
    let shapes = built
        .order()
        .iter()
        .filter_map(|id| built.shape(*id).map(|shape| (*id, shape)))
        .collect();
    let model = name_rebuilt(&document, &shapes, &mut kernel).expect("names");
    let (feature, _) = model.features.iter().next().expect("extrusion");
    let old = built.topology().feature(*feature).expect("old names");
    let mut meanings = Vec::new();
    for segment in old.named_segments() {
        for side in [CapSide::Start, CapSide::End] {
            meanings.push((
                EntityKind::Edge,
                SemanticRole::ExtrudeCapEdge {
                    side,
                    profile_segment: segment,
                },
            ));
        }
    }
    for joint in old.named_joints() {
        meanings.push((EntityKind::Edge, SemanticRole::ExtrudeSweepEdge { joint }));
        for side in [CapSide::Start, CapSide::End] {
            meanings.push((
                EntityKind::Vertex,
                SemanticRole::ExtrudeCapVertex { side, joint },
            ));
        }
    }
    assert_eq!(meanings.len(), 20);
    let mut wrong = document
        .topology_refs()
        .expect("refs")
        .into_iter()
        .find(|r| matches!(r.output_role, SemanticRole::ExtrudeSide { .. }))
        .expect("side ref");
    wrong.selection = SelectionRule::AllDerivedFrom {
        ancestor: ferritecad_types::StableEntityId::new(),
    };
    assert!(ferritecad_topology::resolve(built.topology(), &wrong).is_err());
    assert!(model.resolve_legacy(&wrong).is_err());
    wrong.selection = SelectionRule::Exact;
    wrong.output_role = SemanticRole::RevolveCap {
        side: CapSide::Start,
    };
    assert!(ferritecad_topology::resolve(built.topology(), &wrong).is_err());
    assert!(model.resolve_legacy(&wrong).is_err());

    for (expected_kind, output_role) in meanings {
        let reference = TopologyRef {
            id: ferritecad_types::StableEntityId::new(),
            owner: *feature,
            producer_feature: *feature,
            expected_kind,
            output_role,
            selection: SelectionRule::Exact,
            fallback_signature: None,
        };
        let old: BTreeSet<_> = ferritecad_topology::resolve(built.topology(), &reference)
            .expect("old reference")
            .into_iter()
            .collect();
        assert_eq!(
            old,
            model
                .resolve_legacy(&reference)
                .expect("translated reference")
        );
    }
    built.release_all(&mut kernel);
}

#[test]
fn refuse_missing_wrong_kind_and_ambiguous_names_without_picking_a_neighbour() {
    use ferritecad_kernel::{SessionId, ShapeHandle, ShapeTopology, SubShapeHandle};
    use ferritecad_topology::generative::{FeatureNaming, Name, Selection};
    use ferritecad_types::ContentHash;
    let shape = ShapeHandle::new(SessionId::new(), 1);
    let left = SubShapeHandle::new(shape, SubShapeKind::Edge, 3);
    let right = SubShapeHandle::new(shape, SubShapeKind::Edge, 9);
    let name = Name {
        kind: SubShapeKind::Edge,
        digest: ContentHash::of_bytes(b"same incidence and history"),
    };
    let members = BTreeSet::from([left, right]);
    let named = FeatureNaming {
        topology: ShapeTopology::default(),
        entities: BTreeMap::from([(left, name), (right, name)]),
        bindings: BTreeMap::from([(name, members.clone())]),
        ancestry: BTreeMap::from([(name, members.clone())]),
    };
    assert!(named.resolve(name).is_err());
    assert_eq!(
        named
            .resolve_set(name, Selection::AllDescendants)
            .expect("whole family"),
        &members
    );
    assert!(
        named
            .resolve(Name {
                kind: SubShapeKind::Face,
                ..name
            })
            .is_err()
    );
    assert!(
        named
            .resolve(Name {
                digest: ContentHash::of_bytes(b"deleted"),
                ..name
            })
            .is_err()
    );
}
