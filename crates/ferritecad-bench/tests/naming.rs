// SPDX-License-Identifier: MIT
#![allow(
    clippy::panic,
    reason = "a mismatch reports both resolutions in the test failure"
)]
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::{Document, EditChange, Expression, ObjectPayload};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{GeometryKernel, OperationContext, SubShapeKind};
use ferritecad_occt::OcctKernel;
use ferritecad_topology::generative::ModelNaming;

fn names(document: &Document) -> ModelNaming {
    let mut kernel = OcctKernel::new().expect("native kernel");
    let built = rebuild_cold(document, &mut kernel, &OperationContext::default()).expect("rebuild");
    assert!(built.all_built(), "{:?}", built.report());
    let names = built.topology().clone();
    assert_scene_names(document, &built, &mut kernel);
    for object in document.objects().expect("objects") {
        let (previous, edges) = match object.payload {
            ObjectPayload::Fillet(f) => (f.previous, f.edges),
            ObjectPayload::Chamfer(c) => (c.previous, c.edges),
            _ => continue,
        };
        let selected = names
            .resolve_edges(object.id, previous, &edges)
            .expect("generative edges");
        for selected in selected {
            assert_eq!(selected.kind(), SubShapeKind::Edge);
            assert_eq!(
                selected.shape(),
                built.shape(previous).expect("predecessor")
            );
            assert!(names.features[&previous].entities.contains_key(&selected));
        }
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
    let dir = tempfile::tempdir().expect("cache directory");
    let mut cache = ferritecad_document::CacheStore::open(
        dir.path().join("names.cache"),
        document.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("cache");
    let (prime, events) = ferritecad_eval::rebuild_cached(
        document,
        &mut kernel,
        &mut cache,
        &OperationContext::default(),
    )
    .expect("prime");
    assert!(prime.all_built(), "{:?}", prime.report());
    assert!(
        events
            .iter()
            .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Miss)
    );
    assert_same_names(&names, prime.topology());
    prime.release_all(&mut kernel);
    let mut reader = OcctKernel::new().expect("new session");
    let (restored, events) = ferritecad_eval::rebuild_cached(
        document,
        &mut reader,
        &mut cache,
        &OperationContext::default(),
    )
    .expect("restore");
    assert!(restored.all_built());
    assert!(
        events
            .iter()
            .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
    );
    assert_same_names(&names, restored.topology());
    for named in restored.topology().features.values() {
        assert!(
            reader
                .naming_history(named.shape().expect("shape"))
                .is_err()
        );
    }
    restored.release_all(&mut reader);
    assert_eq!(reader.live_shape_count(), 0);
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
                let face = inside.features[&base].ancestry
                    [&ferritecad_topology::generative::generated(
                        base,
                        ferritecad_kernel::NamingOperation::Extrude,
                        root,
                        SubShapeKind::Face,
                    )]
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
            let ObjectPayload::Sketch(tool) =
                doc.object(cut.sketch).expect("tool").expect("row").payload
            else {
                panic!("Sketch")
            };
            let floor_name = ferritecad_topology::generative::generated(
                cut.feature,
                ferritecad_kernel::NamingOperation::Extrude,
                ferritecad_topology::generative::cap_root(cut.feature, true, Some(tool.plane)),
                SubShapeKind::Face,
            );
            assert!(
                pocket_names.features[&cut.feature]
                    .resolve(floor_name)
                    .is_ok()
            );
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
fn keep_downstream_names_when_an_unrelated_upstream_cut_changes() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_document::EndCondition;
    use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool};
    use ferritecad_topology::generative::curve_root;
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("two-holes.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let mut doc = Document::open(&path).expect("open");
    let b = body(&doc).expect("body");
    let first = circular_tool(&doc, b, [10., 20.], 2., EndCondition::ThroughAll).expect("first");
    doc.write_changes(&first.changes).expect("first hole");
    let second = circular_tool(&doc, b, [45., 20.], 2., EndCondition::ThroughAll).expect("second");
    doc.write_changes(&second.changes).expect("second hole");
    let (_, _, sketch) = base_profile(&doc, b).expect("base");
    let rim = |model: &ModelNaming| -> BTreeSet<_> {
        let result = &model.features[&second.feature];
        let wall = &result.ancestry[&ferritecad_topology::generative::generated(
            second.feature,
            ferritecad_kernel::NamingOperation::Extrude,
            curve_root(second.feature, second.curve, Some(sketch.plane)),
            SubShapeKind::Face,
        )];
        result
            .entities
            .iter()
            .filter(|(entity, _)| {
                wall.contains(entity) || !result.topology.adjacent_faces[entity].is_disjoint(wall)
            })
            .map(|(_, name)| *name)
            .collect()
    };
    let through = rim(&names(&doc));
    // The first hole stops short of the top face, which it then no longer modifies.
    doc.write_changes(&[EditChange::SetEndCondition {
        object: first.feature,
        end_condition: EndCondition::Blind {
            distance: Expression::constant(4.).expect("depth"),
        },
    }])
    .expect("blind first hole");
    assert_eq!(
        through,
        rim(&names(&doc)),
        "the second hole's rim keeps its names"
    );
}

#[test]
fn retain_tool_cap_derivations_when_the_same_cut_deletes_their_descendants() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_document::EndCondition;
    use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool};
    use ferritecad_topology::generative::{Selection, cap_root, generated};

    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("tool-caps.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let mut document = Document::open(&path).expect("open");
    let body = body(&document).expect("body");
    let (plate, _, sketch) = base_profile(&document, body).expect("base profile");
    let cut = circular_tool(&document, body, [20., 20.], 2., EndCondition::ThroughAll)
        .expect("circular tool");
    document.write_changes(&cut.changes).expect("add cut");
    for through in [true, false] {
        if !through {
            document
                .write_changes(&[EditChange::SetEndCondition {
                    object: cut.feature,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(4.).expect("depth"),
                    },
                }])
                .expect("blind cut");
        }
        let mut kernel = OcctKernel::new().expect("kernel");
        let built =
            rebuild_cold(&document, &mut kernel, &OperationContext::default()).expect("rebuild");
        assert!(built.all_built(), "{:?}", built.report());
        let model = built.topology();
        let predecessor = &model.features[&plate];
        let named = &model.features[&cut.feature];
        for end in [false, true] {
            let cap = generated(
                cut.feature,
                ferritecad_kernel::NamingOperation::Extrude,
                cap_root(cut.feature, end, Some(sketch.plane)),
                SubShapeKind::Face,
            );
            assert!(!predecessor.derivations.contains_key(&cap.digest));
            assert!(named.derivations.contains_key(&cap.digest));
            let descendants = named.ancestry.get(&cap).expect("own output row").len();
            assert_eq!(descendants, usize::from(!through && end));
            for selection in [Selection::One, Selection::AllDescendants] {
                assert_eq!(named.resolve_set(cap, selection).is_ok(), !through && end);
            }
            eprintln!(
                "through={through} end={end}: own generated cap {0}, descendants={descendants}",
                cap.digest
            );
        }
        let archive = ferritecad_topology::archive_feature(&mut kernel, model, cut.feature)
            .expect("the current complete archive accepts the cut");
        archive.encode().expect("valid current archive");
        built.release_all(&mut kernel);
        assert_eq!(kernel.live_shape_count(), 0);
    }
}

#[test]
fn keep_a_reference_compact_after_fifty_generations() {
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
        let reference = ferritecad_document::ReferenceMeaning {
            producer,
            kind: ferritecad_document::EntityKind::Face,
            digest: name.digest,
            selection: ferritecad_document::Selection::One,
        };
        let cbor = reference.to_bytes().expect("encode reference");
        assert_eq!(cbor.len(), 54);
        let decoded =
            ferritecad_document::ReferenceMeaning::from_bytes(&cbor).expect("decode reference");
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
    let model = built.topology().clone();
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
        BooleanRequest, ExtrudeExtent, ExtrudeRequest, NamingInput, NamingRelation, PlanarPoint,
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
        .boolean(
            &BooleanRequest::new(ferritecad_kernel::BooleanKind::Cut, bare, tool.shape)
                .expect("cut request"),
            &OperationContext::default(),
        )
        .expect("cut");
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
                .chamfer_edges(
                    &ChamferRequest::new(input, &[edge], 1.).expect("chamfer request"),
                    &OperationContext::default(),
                )
                .expect("chamfer")
                .shape
        } else {
            kernel
                .fillet_edges(
                    &FilletRequest::new(input, &[edge], 1.).expect("fillet request"),
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
fn committed_plate_scene_names_every_boundary_without_aliases() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let model = names(
        &Document::open_read_only(ferritecad_fixtures::plate_source()).expect("committed fixture"),
    );
    let feature = model.features.values().next().expect("plate");
    assert_eq!(feature.bindings.len(), 26);
    assert!(
        feature
            .bindings
            .values()
            .all(|entities| entities.len() == 1)
    );
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
        derivations: BTreeMap::new(),
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

fn assert_same_names(a: &ModelNaming, b: &ModelNaming) {
    assert_eq!(
        a.features.keys().collect::<Vec<_>>(),
        b.features.keys().collect::<Vec<_>>()
    );
    for (id, x) in &a.features {
        let y = &b.features[id];
        assert_eq!(x.derivations, y.derivations);
        let bindings = |n: &ferritecad_topology::generative::FeatureNaming| {
            n.bindings
                .iter()
                .map(|(name, set)| (*name, set.len()))
                .collect::<BTreeMap<_, _>>()
        };
        let ancestry = |n: &ferritecad_topology::generative::FeatureNaming| {
            n.ancestry
                .iter()
                .map(|(name, set)| (*name, set.iter().map(|s| n.entities[s]).collect::<Vec<_>>()))
                .map(|(name, mut set)| {
                    set.sort();
                    (name, set)
                })
                .collect::<BTreeMap<_, _>>()
        };
        let incidence = |n: &ferritecad_topology::generative::FeatureNaming| {
            let mut rows = n
                .topology
                .adjacent_faces
                .iter()
                .map(|(s, f)| {
                    let mut names = f.iter().map(|v| n.entities[v]).collect::<Vec<_>>();
                    names.sort();
                    (n.entities[s], names)
                })
                .collect::<Vec<_>>();
            rows.sort();
            rows
        };
        assert_eq!(bindings(x), bindings(y), "bindings {id}");
        assert_eq!(ancestry(x), ancestry(y), "ancestry {id}");
        assert_eq!(incidence(x), incidence(y), "incidence {id}");
    }
}

#[test]
fn cold_operations_after_restored_predecessors_match_fully_cold_chains() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool, edge_addition};
    for cut_chain in [false, true] {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("partial.fcad");
        ferritecad_fixtures::write_plate(&path).expect("plate");
        let mut doc = Document::open(path).expect("open");
        let b = body(&doc).expect("body");
        let (predecessor, edited, slot, value) = if cut_chain {
            let cut = circular_tool(
                &doc,
                b,
                [20., 20.],
                2.,
                ferritecad_document::EndCondition::ThroughAll,
            )
            .expect("cut");
            doc.write_changes(&cut.changes).expect("add cut");
            let next = circular_tool(
                &doc,
                b,
                [35., 20.],
                3.,
                ferritecad_document::EndCondition::Blind {
                    distance: Expression::constant(2.).expect("depth"),
                },
            )
            .expect("second cut");
            doc.write_changes(&next.changes).expect("add second cut");
            (
                cut.feature,
                next.feature,
                ferritecad_document::ExpressionSlot::ExtrudeDistance,
                3.,
            )
        } else {
            let base = base_profile(&doc, b).expect("base").0;
            let addition = edge_addition(&doc, b, 0, 1., false).expect("fillet");
            let EditChange::AddFeature { object: fillet, .. } = addition else {
                unreachable!()
            };
            doc.write_changes(&[addition]).expect("add fillet");
            (
                base,
                fillet,
                ferritecad_document::ExpressionSlot::FilletRadius,
                1.2,
            )
        };
        let mut kernel = OcctKernel::new().expect("kernel");
        let mut cache = ferritecad_document::CacheStore::open(
            dir.path().join("partial.cache"),
            doc.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("cache");
        let (prime, _) = ferritecad_eval::rebuild_cached(
            &doc,
            &mut kernel,
            &mut cache,
            &OperationContext::default(),
        )
        .expect("prime");
        assert!(prime.all_built(), "{:?}", prime.report());
        prime.release_all(&mut kernel);
        doc.write_changes(&[EditChange::SetExpression {
            object: edited,
            slot,
            expression: Expression::constant(value).expect("amount"),
        }])
        .expect("edit");
        let (warm, events) = ferritecad_eval::rebuild_cached(
            &doc,
            &mut kernel,
            &mut cache,
            &OperationContext::default(),
        )
        .expect("partial");
        assert!(warm.all_built(), "{:?}", warm.report());
        assert!(
            events.iter().any(
                |e| e.feature == predecessor && e.outcome == ferritecad_eval::CacheOutcome::Hit
            )
        );
        assert!(
            events
                .iter()
                .any(|e| e.feature == edited && e.outcome == ferritecad_eval::CacheOutcome::Miss)
        );
        let restored = warm.shape(predecessor).expect("restored input");
        assert!(
            kernel.naming_history(restored).is_err(),
            "the cold operation cannot query its input history"
        );
        let cold = rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("cold");
        assert!(cold.all_built());
        assert_same_names(cold.topology(), warm.topology());
        for feature in cold.topology().producers() {
            let archived = |kernel: &mut OcctKernel, model: &ModelNaming| {
                ferritecad_topology::archive_feature(kernel, model, feature).expect("archive")
            };
            let cold_archive = archived(&mut kernel, cold.topology());
            let warm_archive = archived(&mut kernel, warm.topology());
            assert_eq!(
                naming_bytes(&cold_archive),
                naming_bytes(&warm_archive),
                "producer, derivations, entity names, ancestry and incidence for {feature}"
            );
            let a = cold.shape(feature).expect("cold shape");
            let b = warm.shape(feature).expect("warm shape");
            assert!(kernel.is_valid(a).expect("cold BRepCheck"));
            assert!(kernel.is_valid(b).expect("warm BRepCheck"));
            let (faces_a, volume_a) = kernel.shape_stats(a).expect("cold geometry");
            let (faces_b, volume_b) = kernel.shape_stats(b).expect("warm geometry");
            assert_eq!(faces_a, faces_b);
            assert!((volume_a - volume_b).abs() <= 1e-9 * volume_a.abs().max(1.));
            for direction in [
                ferritecad_types::Vec3::new(1., 0., 0.),
                ferritecad_types::Vec3::new(0., 1., 0.),
                ferritecad_types::Vec3::new(0., 0., 1.),
            ] {
                let direction = direction.expect("unit direction");
                let x = kernel.projected_bounds(a, direction).expect("cold bounds");
                let y = kernel.projected_bounds(b, direction).expect("warm bounds");
                assert!(x.into_iter().zip(y).all(|(a, b)| (a - b).abs() < 1e-8));
            }
        }
        warm.release_all(&mut kernel);
        cold.release_all(&mut kernel);
        assert_eq!(kernel.live_shape_count(), 0);
    }
}

fn naming_bytes(archive: &ferritecad_topology::ArchivedFeature) -> Vec<u8> {
    let bytes = archive.encode().expect("encoded archive");
    let identity = archive.blob().kernel();
    let header =
        16 + 3 * 4 + identity.id().len() + identity.version().len() + identity.build().len();
    let after_brep = header + 4 + archive.blob().bytes().len();
    // Exclude the length-prefixed B-Rep and the checksum that covers its numeric bits.
    [&bytes[..header], &bytes[after_brep..bytes.len() - 32]].concat()
}

fn assert_scene_names(
    document: &Document,
    built: &ferritecad_eval::RebuildResult,
    kernel: &mut OcctKernel,
) {
    let params = ferritecad_kernel::TessellationParams::default();
    let scene = ferritecad_scene::snapshot_from_rebuild(
        document,
        built,
        kernel,
        |_, _| Err(ferritecad_types::CadError::input("unexpected import")),
        &params,
        &OperationContext::default(),
    )
    .expect("scene snapshot");
    let mut checked = 0;
    let mut ambiguous = 0;
    for (definition, entry) in scene.catalogue.iter().enumerate() {
        let ferritecad_scene::SceneItem::Body(body) = entry.item else {
            continue;
        };
        let shape = built.shape(body).expect("body shape");
        let (feature, names) = built
            .topology()
            .features
            .iter()
            .find(|(_, names)| names.shape() == Some(shape))
            .expect("body names");
        let mesh = kernel
            .tessellate(shape, &params, &OperationContext::default())
            .expect("mesh");
        let mut verify = |entity, meanings: &[ferritecad_scene::EntityMeaning]| {
            let name = names.entities[&entity];
            if names.bindings[&name].len() != 1 {
                assert!(meanings.is_empty(), "ambiguous entity was named");
                ambiguous += 1;
            } else {
                assert_eq!(meanings.len(), 1, "unique entity lacks description");
                assert_eq!(meanings[0].feature, *feature);
                assert_eq!(meanings[0].kind, name.kind);
                assert_eq!(meanings[0].digest, name.digest);
                assert_eq!(
                    meanings[0].description,
                    built
                        .topology()
                        .describe(*feature, name)
                        .expect("complete description")
                );
                assert_eq!(names.resolve(name).expect("unique resolution"), entity);
                checked += 1;
            }
        };
        for (ordinal, range) in mesh.faces.iter().enumerate() {
            let pick = scene
                .snapshot
                .face_of(definition, ordinal)
                .expect("face pick");
            verify(range.face, scene.faces.of(pick, &scene.snapshot));
        }
        if let Some(edges) = &mesh.edges {
            for (ordinal, range) in edges.ranges.iter().enumerate() {
                let pick = scene
                    .snapshot
                    .edge_of(definition, ordinal)
                    .expect("edge pick");
                verify(range.edge, scene.edges.of(pick, &scene.snapshot));
            }
        }
        if let Some(vertices) = &mesh.topological_vertices {
            for (ordinal, range) in vertices.ranges.iter().enumerate() {
                let pick = scene
                    .snapshot
                    .vertex_of(definition, ordinal)
                    .expect("vertex pick");
                verify(range.vertex, scene.vertices.of(pick, &scene.snapshot));
            }
        }
    }
    assert!(checked > 0);
    let _ = ambiguous;
}

#[test]
fn angle_actions_select_partial_revolutions_and_skip_full_turns() {
    let root = tempfile::tempdir().expect("root");
    let cases = ferritecad_bench::corpus::generate(root.path(), false).expect("corpus");
    for case in cases {
        if case.name == "revolve" {
            assert!(case.revolve.is_some());
        }
        if case.name == "revolve_cut" {
            assert!(case.revolve.is_none());
        }
        if let Some(id) = case.revolve {
            let d = Document::open_read_only(&case.path).expect("document");
            assert!(matches!(
                d.object(id).expect("row").expect("feature").payload,
                ObjectPayload::Revolve(ferritecad_document::Revolve {
                    extent: ferritecad_document::RevolveExtent::Partial { .. },
                    ..
                })
            ));
        }
    }
}
