// SPDX-License-Identifier: MIT
use ferritecad_document::*;
use ferritecad_eval::{CacheOutcome, RebuildResult, rebuild_cached, rebuild_cold};
use ferritecad_fixtures::result_sets::{extrusion, plate, rectangle, slot};
use ferritecad_kernel::{GeometryKernel, OperationContext, SubShapeKind};
use ferritecad_occt::OcctKernel;
use ferritecad_types::ObjectId;
use std::collections::BTreeMap;

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
}
fn build(doc: &Document, kernel: &mut OcctKernel) -> RebuildResult {
    let built = rebuild_cold(doc, kernel, &OperationContext::default()).expect("rebuild");
    assert!(built.all_built(), "{:?}", built.report());
    built
}
fn measure(
    built: &RebuildResult,
    id: ObjectId,
    kernel: &mut OcctKernel,
    solids: usize,
    volume: f64,
) {
    let shape = built.shape(id).expect("result");
    kernel
        .validate_solids(shape)
        .expect("every member valid and positive");
    assert!(kernel.is_valid(shape).expect("independent BRepCheck"));
    assert_eq!(kernel.solid_count(shape).expect("solid count"), solids);
    let actual = kernel.shape_stats(shape).expect("volume").1;
    assert!(
        (actual - volume).abs() < 1e-6 * volume.max(1.),
        "{actual} != {volume}"
    );
    let named = &built.topology().features[&id];
    assert_eq!(
        named.entities.len(),
        kernel
            .shape_topology(shape)
            .expect("inventory")
            .adjacent_faces
            .len()
    );
}
fn signature(
    built: &RebuildResult,
) -> BTreeMap<ObjectId, BTreeMap<ferritecad_topology::generative::Name, usize>> {
    built
        .topology()
        .features
        .iter()
        .map(|(id, names)| {
            (
                *id,
                names
                    .bindings
                    .iter()
                    .map(|(name, entities)| (*name, entities.len()))
                    .collect(),
            )
        })
        .collect()
}
fn warm(doc: &Document, cold: &RebuildResult, kernel: &mut OcctKernel) {
    let root = tempfile::tempdir().expect("cache");
    let mut cache = CacheStore::open(
        root.path().join("cache"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("sidecar");
    for attempt in 0..2 {
        let (restored, events) =
            rebuild_cached(doc, kernel, &mut cache, &OperationContext::default())
                .expect("cached rebuild");
        assert!(restored.all_built(), "{:?}", restored.report());
        assert_eq!(signature(cold), signature(&restored));
        assert!(
            events.iter().all(|e| e.outcome
                == if attempt == 0 {
                    CacheOutcome::Miss
                } else {
                    CacheOutcome::Hit
                }),
            "{events:?}"
        );
        for id in cold.topology().producers() {
            let shape = cold.shape(id).expect("cold shape");
            let count = kernel.solid_count(shape).expect("count");
            let volume = kernel.shape_stats(shape).expect("volume").1;
            measure(&restored, id, kernel, count, volume);
        }
        restored.release_all(kernel);
    }
}

#[test]
fn slot_keep_all_named_piece_and_following_cut_preserve_complete_names_and_cache() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut doc = Document::create(root.path().join("slot.fcad")).expect("document");
    let p = plate(&mut doc).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let s = slot(&doc, &p, &mut kernel).expect("slot");
    doc.write_changes(&s.changes)
        .expect("slot and KeepSolids together");
    let count = doc.objects().expect("objects").len();
    let all = build(&doc, &mut kernel);
    measure(&all, s.feature, &mut kernel, 2, 22400.);
    measure(&all, s.keep, &mut kernel, 2, 22400.);
    assert_eq!(signature(&all)[&s.feature], signature(&all)[&s.keep]);
    assert!(all.topology().features[&s.keep].derivations.is_empty());
    warm(&doc, &all, &mut kernel);
    all.release_all(&mut kernel);
    assert_eq!(doc.objects().expect("objects").len(), count);
    doc.write_changes(&[EditChange::SetKeepSelection {
        object: s.keep,
        selection: KeepSelection::ContainingFaces(vec![s.side]),
    }])
    .expect("choose witness");
    let selected = build(&doc, &mut kernel);
    measure(&selected, s.keep, &mut kernel, 1, 11200.);
    let named = &selected.topology().features[&s.keep];
    assert!(named.derivations.is_empty());
    assert!(named.bindings.keys().all(|name| {
        selected.topology().features[&s.feature]
            .bindings
            .contains_key(name)
    }));
    assert!(
        selected
            .topology()
            .resolve(EntityKind::Face, s.keep, s.keep, &[s.other_side])
            .expect_err("discarded side")
            .to_string()
            .contains("face deleted")
    );
    assert!(
        !named
            .ancestry
            .contains_key(&ferritecad_topology::generative::Name {
                kind: SubShapeKind::Face,
                digest: s.other_side.meaning.digest,
            })
    );
    let mut split = s.split;
    split.meaning.selection = Selection::AllDescendants;
    let descendant = selected
        .topology()
        .resolve(EntityKind::Face, s.keep, s.keep, &[split])
        .expect("one retained descendant");
    assert_eq!(descendant.len(), 1);
    assert_ne!(
        named.entities[&descendant[0]].digest, split.meaning.digest,
        "the retained split face must not revert to its parent name"
    );
    warm(&doc, &selected, &mut kernel);
    selected.release_all(&mut kernel);
    let (profile, cut) = (ObjectId::new(), ObjectId::new());
    doc.write_changes(&[
        EditChange::AddSketch {
            object: profile,
            name: None,
            sketch: rectangle(p.plane, [5., 5.], [10., 10.]).expect("tool"),
        },
        EditChange::AddFeature {
            object: cut,
            body: p.body,
            name: None,
            feature: extrusion(profile, SolidOperation::Cut, 4.).expect("cut"),
        },
    ])
    .expect("following cut");
    let built = build(&doc, &mut kernel);
    measure(&built, cut, &mut kernel, 1, 11100.);
    warm(&doc, &built, &mut kernel);
    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn split_families_and_deleted_witnesses_refuse_without_retargeting() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut doc = Document::create(root.path().join("refs.fcad")).expect("document");
    let p = plate(&mut doc).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let s = slot(&doc, &p, &mut kernel).expect("slot");
    doc.write_changes(&s.changes).expect("slot");
    for (selection, reason) in [
        (Selection::One, "split into 2 faces"),
        (Selection::AllDescendants, "faces in 2 solids"),
    ] {
        let mut face = s.split;
        face.meaning.selection = selection;
        doc.write_changes(&[EditChange::SetKeepSelection {
            object: s.keep,
            selection: KeepSelection::ContainingFaces(vec![face]),
        }])
        .expect("broken saved state");
        let built = rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("repair");
        let report = built.report();
        let failure = &report.failed[0];
        assert_eq!(failure.id, s.keep);
        assert_eq!(failure.kind, "topology");
        for text in [reason.to_owned(), s.keep.to_string(), face.id.to_string()] {
            assert!(failure.message.contains(&text), "{}", failure.message);
        }
        assert_eq!(
            built.shape(p.body),
            built.shape(s.feature),
            "whole degraded result"
        );
        assert_eq!(
            kernel
                .solid_count(built.shape(p.body).expect("fallback"))
                .expect("count"),
            2
        );
        built.release_all(&mut kernel);
    }
    doc.write_changes(&[EditChange::SetKeepSelection {
        object: s.keep,
        selection: KeepSelection::ContainingFaces(vec![s.side]),
    }])
    .expect("repair");
    let second = ObjectId::new();
    doc.write_changes(&[EditChange::AddFeature {
        object: second,
        body: p.body,
        name: None,
        feature: FeatureAddition::KeepSolids {
            selection: KeepSelection::ContainingFaces(vec![s.other_side]),
        },
    }])
    .expect("lost side");
    let built = rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("repair");
    let failure = &built.report().failed[0];
    assert_eq!(failure.id, second);
    assert!(failure.message.contains("face deleted"));
    assert!(failure.message.contains(&s.other_side.id.to_string()));
    built.release_all(&mut kernel);
}

#[test]
fn disconnected_sweeps_and_booleans_return_every_valid_member() {
    if !native() {
        return;
    }
    for case in ["new", "add", "intersect", "revolve"] {
        let root = tempfile::tempdir().expect("root");
        let mut doc = Document::create(root.path().join("set.fcad")).expect("document");
        let p = plate(&mut doc).expect("plate");
        let (profile, feature) = (ObjectId::new(), ObjectId::new());
        let mut circles =
            ferritecad_fixtures::datum_planes::circle(p.plane, [15., 20.], 3.).expect("circle");
        circles.curves.extend(
            ferritecad_fixtures::datum_planes::circle(p.plane, [45., 20.], 3.)
                .expect("circle")
                .curves,
        );
        let sketch = if case == "add" {
            rectangle(p.plane, [70., 0.], [80., 10.]).expect("detached boss")
        } else {
            circles
        };
        let addition = if case == "revolve" {
            FeatureAddition::Revolve {
                profile,
                regions: vec![],
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::FullTurn,
                operation: SolidOperation::NewBody,
            }
        } else {
            extrusion(
                profile,
                match case {
                    "add" => SolidOperation::Add,
                    "intersect" => SolidOperation::Intersect,
                    _ => SolidOperation::NewBody,
                },
                10.,
            )
            .expect("sweep")
        };
        let change = if case == "new" || case == "revolve" {
            EditChange::AddBody {
                object: ObjectId::new(),
                name: None,
                feature_object: feature,
                feature_name: None,
                feature: addition,
            }
        } else {
            EditChange::AddFeature {
                object: feature,
                body: p.body,
                name: None,
                feature: addition,
            }
        };
        doc.write_changes(&[
            EditChange::AddSketch {
                object: profile,
                name: None,
                sketch,
            },
            change,
        ])
        .expect("set");
        let mut kernel = OcctKernel::new().expect("kernel");
        let built = build(&doc, &mut kernel);
        let volume = match case {
            "add" => 25000.,
            "revolve" => 2. * std::f64::consts::PI.powi(2) * 9. * 60.,
            _ => 180. * std::f64::consts::PI,
        };
        measure(&built, feature, &mut kernel, 2, volume);
        warm(&doc, &built, &mut kernel);
        built.release_all(&mut kernel);
    }
}

#[test]
fn edge_operation_on_a_set_refuses_with_its_solid_count() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut doc = Document::create(root.path().join("edge.fcad")).expect("document");
    let p = plate(&mut doc).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let s = slot(&doc, &p, &mut kernel).expect("slot");
    doc.write_changes(&s.changes).expect("slot");
    let built = build(&doc, &mut kernel);
    let name = *built.topology().features[&s.keep]
        .bindings
        .iter()
        .find(|(name, members)| name.kind == SubShapeKind::Edge && members.len() == 1)
        .expect("unique edge")
        .0;
    let edge = EntityReference::new(ReferenceMeaning {
        producer: built.topology().producer(&name.digest).expect("producer"),
        kind: EntityKind::Edge,
        digest: name.digest,
        selection: Selection::One,
    });
    built.release_all(&mut kernel);
    for chamfer in [false, true] {
        let feature = ObjectId::new();
        let payload = if chamfer {
            ObjectPayload::Chamfer(Chamfer {
                previous: s.keep,
                edges: vec![edge],
                distance_mm: Expression::constant(0.5).expect("size"),
            })
        } else {
            ObjectPayload::Fillet(Fillet {
                previous: s.keep,
                edges: vec![edge],
                radius_mm: Expression::constant(0.5).expect("size"),
            })
        };
        doc.write(|w| {
            w.put_object(feature, None, 0, None, &payload)?;
            for dep in payload.dependencies(feature) {
                w.add_dependency(dep)?;
            }
            Ok(())
        })
        .expect("edge feature");
        let built = rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("repair");
        assert_eq!(built.report().failed.len(), 1);
        assert!(
            built.report().failed[0]
                .message
                .contains("edge operations on a result with 2 solids are not supported yet")
        );
        built.release_all(&mut kernel);
        // Keep separate chains to exercise both builders without introducing a fork.
        doc.write(|w| w.remove_object(feature))
            .expect("remove test consumer");
    }
}

#[test]
fn damaged_keep_archive_is_rejected_atomically_and_replaced() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let mut doc = Document::create(root.path().join("keep.fcad")).expect("document");
    let p = plate(&mut doc).expect("plate");
    let keep = ObjectId::new();
    doc.write_changes(&[EditChange::AddFeature {
        object: keep,
        body: p.body,
        name: None,
        feature: FeatureAddition::KeepSolids {
            selection: KeepSelection::All,
        },
    }])
    .expect("KeepSolids");
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let mut cache = CacheStore::open(
        root.path().join("cache"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("cache");
    let (cold, _) = rebuild_cached(&doc, &mut kernel, &mut cache, &context).expect("prime");
    assert!(cold.all_built());
    let expected = signature(&cold);
    let ObjectPayload::Extrude(base) = doc.object(p.base).expect("object").expect("base").payload
    else {
        unreachable!("plate starts with Extrude")
    };
    let request = ferritecad_eval::extrude_request(
        &base,
        cold.profile(base.profile)
            .expect("resolved profile")
            .clone(),
        None,
    )
    .expect("request");
    let previous = cold.topology().cache_key(
        p.base,
        ferritecad_eval::extrude_archive_key(kernel.identity(), &request, &context),
    );
    let payload = KeepSolids {
        previous: p.base,
        selection: KeepSelection::All,
    };
    let mut hash = ferritecad_types::CanonicalHasher::new("eval.keep-solids.named");
    hash.algorithm_version(1);
    kernel.identity().feed(&mut hash);
    hash.field("target").hash(&previous);
    hash.field("payload").hash(&payload.cache_key());
    let key = cold.topology().cache_key(keep, hash.finish());
    let kind = ferritecad_topology::ARCHIVE_CACHE_KIND;
    assert!(cache.get(keep, key, kind).expect("stored record").is_some());
    cold.release_all(&mut kernel);
    cache
        .put(keep, key, kind, b"damaged named result")
        .expect("corrupt inner record");
    for outcome in [CacheOutcome::Rejected, CacheOutcome::Hit] {
        let (actual, events) =
            rebuild_cached(&doc, &mut kernel, &mut cache, &context).expect("rebuild after damage");
        assert!(actual.all_built());
        assert_eq!(
            events
                .iter()
                .find(|e| e.feature == keep)
                .expect("selection event")
                .outcome,
            outcome
        );
        assert_eq!(
            events
                .iter()
                .find(|e| e.feature == p.base)
                .expect("base event")
                .outcome,
            CacheOutcome::Hit
        );
        assert_eq!(signature(&actual), expected);
        assert!(actual.topology().features[&keep].derivations.is_empty());
        measure(&actual, keep, &mut kernel, 1, 24000.);
        actual.release_all(&mut kernel);
        assert_eq!(
            kernel.live_shape_count(),
            0,
            "no partially restored shape survives"
        );
    }
}

#[test]
fn each_boolean_consumes_the_whole_predecessor_set() {
    if !native() {
        return;
    }
    for operation in [
        SolidOperation::Add,
        SolidOperation::Cut,
        SolidOperation::Intersect,
    ] {
        let root = tempfile::tempdir().expect("directory");
        let mut doc = Document::create(root.path().join("set.fcad")).expect("document");
        let p = plate(&mut doc).expect("plate");
        let mut kernel = OcctKernel::new().expect("kernel");
        let s = slot(&doc, &p, &mut kernel).expect("split");
        doc.write_changes(&s.changes)
            .expect("complete predecessor set");
        let mut circles =
            ferritecad_fixtures::datum_planes::circle(p.plane, [15., 20.], 2.).expect("first tool");
        circles.curves.extend(
            ferritecad_fixtures::datum_planes::circle(p.plane, [45., 20.], 2.)
                .expect("second tool")
                .curves,
        );
        let (profile, feature) = (ObjectId::new(), ObjectId::new());
        let distance = if operation == SolidOperation::Cut {
            4.
        } else {
            14.
        };
        doc.write_changes(&[
            EditChange::AddSketch {
                object: profile,
                name: None,
                sketch: circles,
            },
            EditChange::AddFeature {
                object: feature,
                body: p.body,
                name: None,
                feature: extrusion(profile, operation, distance).expect("boolean"),
            },
        ])
        .expect("consumer");
        let built = build(&doc, &mut kernel);
        let pi = std::f64::consts::PI;
        let expected = match operation {
            SolidOperation::Add => 22400. + 32. * pi,
            SolidOperation::Cut => 22400. - 32. * pi,
            SolidOperation::Intersect => 80. * pi,
            _ => unreachable!("tested boolean operations"),
        };
        measure(&built, feature, &mut kernel, 2, expected);
        assert_eq!(built.shape(p.body), built.shape(feature));
        warm(&doc, &built, &mut kernel);
        built.release_all(&mut kernel);
    }
}

#[test]
fn a_degenerate_prism_tool_is_refused_by_the_boolean_result_checks() {
    if !native() {
        return;
    }
    use ferritecad_kernel::{
        BooleanKind, BooleanRequest, ExtrudeExtent, ExtrudeRequest, SketchPlane,
    };
    use ferritecad_types::ErrorKind;

    let root = tempfile::tempdir().expect("directory");
    let mut doc = Document::create(root.path().join("thin-tool.fcad")).expect("document");
    let p = plate(&mut doc).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = build(&doc, &mut kernel);
    let target = built.shape(p.base).expect("plate result");
    let before = kernel.encode_shape(target).expect("target bytes");
    let sketch = rectangle(p.plane, [5., 5.], [10., 10.]).expect("simple profile");
    let profile =
        ferritecad_eval::profile_from_sketch(&sketch, ObjectId::new(), SketchPlane::world_xy())
            .expect("profile")
            .profile;
    // Positive public input below the native geometric tolerance produces a degenerate tool.
    let request = ExtrudeRequest::new(
        profile,
        ExtrudeExtent::blind(1.0e-10).expect("positive extent"),
        false,
    )
    .for_boolean_tool();
    let tool = kernel.extrude(&request, &context).expect("temporary prism");
    assert!(!kernel.is_valid(tool.shape).expect("independent BRepCheck"));
    let live = kernel.live_shape_count();
    for (kind, expected, detail) in [
        (BooleanKind::Cut, ErrorKind::Kernel, "invalid solid"),
        (
            BooleanKind::Common,
            ErrorKind::Unsupported,
            "intersect result is empty",
        ),
    ] {
        let request = BooleanRequest::new(kind, target, tool.shape).expect("boolean");
        let error = kernel
            .boolean(&request, &context)
            .expect_err("no valid result");
        assert_eq!(error.kind(), expected);
        assert!(error.to_string().contains(detail), "{error}");
        assert_eq!(
            kernel.live_shape_count(),
            live,
            "no result published or leaked"
        );
        assert_eq!(kernel.encode_shape(target).expect("target bytes"), before);
    }
    kernel.release(tool.shape);
    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}
