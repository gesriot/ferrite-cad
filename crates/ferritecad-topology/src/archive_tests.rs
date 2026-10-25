// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_kernel::*;
use ferritecad_types::{ContentHash, StableEntityId};
#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod support;

pub(crate) fn plate() -> (mock::MockKernel, TopologyMap, ObjectId, ExtrudeRequest) {
    let corners = [(0., 0.), (20., 0.), (20., 10.), (0., 10.)]
        .map(|(x, y)| PlanarPoint::new(x, y).expect("point"));
    let segments = (0..4)
        .map(|i| {
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(corners[i], corners[(i + 1) % 4]).expect("line"),
            )
        })
        .collect();
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(segments).expect("loop"),
        vec![],
    )
    .expect("profile");
    let request = ExtrudeRequest::new(profile, ExtrudeExtent::blind(5.).expect("height"), false);
    let mut kernel = mock::MockKernel::new();
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("prism");
    let feature = ObjectId::new();
    let model = support::bind(&mut kernel, feature, request.profile(), &result).expect("names");
    (kernel, model, feature, request)
}

#[test]
fn every_prism_entity_and_typed_name_survives_an_atomic_archive() {
    let (mut writer, model, feature, request) = plate();
    let named = &model.features[&feature];
    assert_eq!(named.entities.len(), 26);
    assert_eq!(
        [SubShapeKind::Face, SubShapeKind::Edge, SubShapeKind::Vertex]
            .map(|k| named.topology.entities(k).count()),
        [6, 12, 8]
    );
    let archive = archive_feature(&mut writer, &model, feature).expect("archive");
    assert_eq!(archive.entities.len(), 26);
    let bytes = archive.encode().expect("encode");
    assert_eq!(
        bytes,
        archive_feature(&mut writer, &model, feature)
            .expect("repeat")
            .encode()
            .expect("encode")
    );
    let decoded = ArchivedFeature::decode(&bytes, feature, writer.identity()).expect("decode");
    assert_eq!(archive, decoded);
    let mut reader = mock::MockKernel::new();
    let mut restored = support::prepared(feature, request.profile(), false).expect("roots");
    restore_feature(&mut reader, &decoded, &mut restored).expect("restore");
    let back = &restored.features[&feature];
    assert_ne!(named.shape(), back.shape());
    assert_eq!(support::signature(named), support::signature(back));
    let incidence = |f: &FeatureNaming| {
        f.topology
            .adjacent_faces
            .iter()
            .map(|(s, faces)| {
                (
                    f.entities[s],
                    faces.iter().map(|v| f.entities[v]).collect::<BTreeSet<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(incidence(named), incidence(back));
    assert!(reader.naming_history(back.shape().expect("shape")).is_err());
    for (slot, name) in &archive.entities {
        assert_eq!(archive.ancestry[name], BTreeSet::from([*slot]));
    }
}

#[test]
fn reject_each_incomplete_or_contradictory_graph_before_restoring_any_shape() {
    let (mut kernel, model, feature, _) = plate();
    let archive = archive_feature(&mut kernel, &model, feature).expect("archive");
    let (slot, name) = archive.entities().next().expect("entity");
    let face = archive
        .entities
        .iter()
        .find(|(_, n)| n.kind == SubShapeKind::Face)
        .map(|(s, _)| *s)
        .expect("face");
    let edge = archive
        .entities
        .iter()
        .find(|(_, n)| n.kind == SubShapeKind::Edge)
        .map(|(s, _)| *s)
        .expect("edge");
    let mut cases = Vec::new();
    let mut empty = archive.clone();
    empty.entities.clear();
    empty.ancestry.clear();
    empty.incidence.clear();
    cases.push(empty);
    let mut bad = archive.clone();
    bad.entities.remove(&slot);
    cases.push(bad);
    let mut bad = archive.clone();
    bad.ancestry.remove(&name);
    cases.push(bad);
    let alias = archive
        .ancestry
        .keys()
        .find(|n| !archive.entities.values().any(|primary| primary == *n))
        .expect("generated boundary witness");
    let mut bad = archive.clone();
    bad.ancestry.remove(alias);
    cases.push(bad);
    let mut bad = archive.clone();
    bad.incidence.remove(&slot);
    cases.push(bad);
    let mut bad = archive.clone();
    bad.incidence.get_mut(&face).expect("face").insert(face);
    cases.push(bad);
    let mut bad = archive.clone();
    bad.incidence.get_mut(&edge).expect("edge").clear();
    cases.push(bad);
    let mut bad = archive.clone();
    bad.incidence.get_mut(&edge).expect("edge").insert(edge);
    cases.push(bad);
    let mut bad = archive.clone();
    bad.ancestry
        .get_mut(&name)
        .expect("name")
        .insert(ArchiveSlot::ROOT);
    cases.push(bad);
    let mut bad = archive.clone();
    bad.entities.insert(
        slot,
        Name {
            kind: if name.kind == SubShapeKind::Face {
                SubShapeKind::Edge
            } else {
                SubShapeKind::Face
            },
            ..name
        },
    );
    cases.push(bad);
    let mut bad = archive.clone();
    bad.entities.insert(
        slot,
        Name {
            digest: ContentHash::of_bytes(b"foreign"),
            ..name
        },
    );
    cases.push(bad);
    let mut bad = archive.clone();
    bad.derivations.remove(&name.digest);
    cases.push(bad);
    let mut bad = archive.clone();
    let node = bad.derivations.remove(&name.digest).expect("node");
    bad.derivations
        .insert(ContentHash::of_bytes(b"wrong digest"), node);
    cases.push(bad);
    for bad in cases {
        assert!(bad.validate().is_err());
        assert!(bad.encode().is_err());
    }
}

#[test]
fn reject_slot_shifts_wrong_kinds_and_incidence_against_the_brep_without_partial_publication() {
    let (mut kernel, model, feature, request) = plate();
    let good = archive_feature(&mut kernel, &model, feature).expect("archive");
    let mut cases = Vec::new();
    // Rename a slot consistently throughout the graph: structurally sound, absent from this B-Rep.
    for replacement in [ArchiveSlot::new(9999)] {
        let mut bad = good.clone();
        let (slot, name) = bad.entities().next().expect("first");
        bad.entities.remove(&slot);
        bad.entities.insert(replacement, name);
        let incidence = bad.incidence.remove(&slot).expect("incidence");
        bad.incidence.insert(replacement, incidence);
        for set in bad.ancestry.values_mut().chain(bad.incidence.values_mut()) {
            if set.remove(&slot) {
                set.insert(replacement);
            }
        }
        cases.push(bad);
    }
    let mut bad = good.clone();
    let edge = bad
        .entities
        .iter()
        .find(|(_, n)| n.kind == SubShapeKind::Edge)
        .map(|(s, _)| *s)
        .expect("edge");
    let face = bad
        .entities
        .iter()
        .find(|(s, n)| n.kind == SubShapeKind::Face && !bad.incidence[&edge].contains(s))
        .map(|(s, _)| *s)
        .expect("unrelated face");
    bad.incidence.get_mut(&edge).expect("edge").insert(face);
    cases.push(bad);
    let mut bad = good.clone();
    let first = *bad.entities.keys().next().expect("slot");
    let second = *bad.entities.keys().next_back().expect("slot");
    let rename = |slot: ArchiveSlot| {
        if slot == first {
            second
        } else if slot == second {
            first
        } else {
            slot
        }
    };
    bad.entities = bad
        .entities
        .into_iter()
        .map(|(s, n)| (rename(s), n))
        .collect();
    bad.ancestry = bad
        .ancestry
        .into_iter()
        .map(|(n, set)| (n, set.into_iter().map(rename).collect()))
        .collect();
    bad.incidence = bad
        .incidence
        .into_iter()
        .map(|(s, set)| (rename(s), set.into_iter().map(rename).collect()))
        .collect();
    cases.push(bad);
    for bad in cases {
        let mut reader = mock::MockKernel::new();
        let mut destination = support::prepared(feature, request.profile(), false).expect("roots");
        assert!(restore_feature(&mut reader, &bad, &mut destination).is_err());
        assert!(destination.features.is_empty());
        assert_eq!(reader.live_shape_count(), 0);
    }
}

#[test]
fn allow_sets_shared_slots_and_explicit_deleted_meanings_without_choosing_a_member() {
    let (mut kernel, model, feature, request) = plate();
    let mut archive = archive_feature(&mut kernel, &model, feature).expect("archive");
    let faces: Vec<_> = archive
        .entities
        .iter()
        .filter(|(_, n)| n.kind == SubShapeKind::Face)
        .map(|(s, n)| (*s, *n))
        .collect();
    let alias = faces[0].1;
    archive
        .ancestry
        .get_mut(&alias)
        .expect("ancestry")
        .insert(faces[1].0);
    let root = crate::generative::cap_root(
        feature,
        true,
        Some(
            "00000000-0000-7000-8000-000000000001"
                .parse::<ObjectId>()
                .expect("id"),
        ),
    );
    let deleted = crate::derivation::expression(
        &mut archive.derivations,
        SubShapeKind::Edge,
        "generated",
        Some(feature),
        [root],
        0,
    )
    .expect("typed output");
    archive.ancestry.insert(deleted, BTreeSet::new());
    let mut reader = mock::MockKernel::new();
    let mut restored = support::prepared(feature, request.profile(), false).expect("roots");
    restore_feature(&mut reader, &archive, &mut restored).expect("sets are legal");
    let named = &restored.features[&feature];
    assert!(named.resolve(alias).is_err());
    assert_eq!(
        named
            .resolve_set(alias, crate::generative::Selection::AllDescendants)
            .expect("whole family")
            .len(),
        2
    );
    assert!(named.resolve(deleted).is_err());
    assert!(named.ancestry[&deleted].is_empty());
}

#[test]
fn codec_rejects_damage_truncation_trailing_data_and_other_owners_or_kernel_builds() {
    let (mut kernel, model, feature, _) = plate();
    let archive = archive_feature(&mut kernel, &model, feature).expect("archive");
    let bytes = archive.encode().expect("encode");
    for end in 0..bytes.len() {
        assert!(ArchivedFeature::decode(&bytes[..end], feature, kernel.identity()).is_err());
    }
    let mut damaged = bytes.clone();
    damaged[20] ^= 1;
    assert!(ArchivedFeature::decode(&damaged, feature, kernel.identity()).is_err());
    let mut trailing = bytes[..bytes.len() - 32].to_vec();
    trailing.push(0);
    let hash = ContentHash::of_bytes(&trailing);
    trailing.extend_from_slice(hash.as_bytes());
    assert!(ArchivedFeature::decode(&trailing, feature, kernel.identity()).is_err());
    assert!(ArchivedFeature::decode(&bytes, ObjectId::new(), kernel.identity()).is_err());
    assert!(
        ArchivedFeature::decode(
            &bytes,
            feature,
            &KernelIdentity::new("mock", "different", "build").expect("identity")
        )
        .is_err()
    );
    let mut reader = mock::MockKernel::new();
    assert!(reader.decode_shape(archive.blob()).is_err());
    assert!(archive_feature(&mut reader, &TopologyMap::new(), feature).is_err());
}

#[test]
fn unequal_derivations_under_one_digest_are_refused_atomically() {
    use crate::derivation::{Derivation, insert, intern};
    use std::sync::Arc;
    let (_, model, feature, _) = plate();
    let graph = &model.features[&feature].derivations;
    let (digest, node) = graph.first_key_value().expect("node");
    let mut collision = graph.clone();
    let foreign = Arc::new(Derivation::Cap(ObjectId::new(), false, None));
    assert!(
        insert(&mut collision, *digest, foreign.clone())
            .expect_err("structural collision")
            .to_string()
            .contains("unequal derivations collide")
    );
    assert_eq!(&collision, graph);
    collision.insert(*digest, foreign);
    assert!(
        model
            .accept_derivations(feature, &collision)
            .expect_err("model collision")
            .to_string()
            .contains("unequal derivations collide")
    );
    assert!(crate::derivation::validate(&collision).is_err());
    insert(&mut collision, *digest, node.clone()).expect_err("the unequal node is still refused");
    let mut equal = graph.clone();
    assert_eq!(
        intern(&mut equal, (**node).clone())
            .expect("same expression")
            .digest,
        *digest
    );
    assert_eq!(&equal, graph);
}

#[test]
fn a_valid_hash_does_not_make_foreign_roots_or_incomplete_derivations_valid() {
    use crate::derivation::{Derivation, expression, intern};
    let (mut kernel, model, feature, request) = plate();
    for node in [
        Derivation::Curve(
            feature,
            StableEntityId::new(),
            model.planes.get(&feature).copied(),
        ),
        Derivation::Curve(
            feature,
            request.profile().segments().next().expect("curve").label,
            Some(ObjectId::new()),
        ),
        Derivation::Cap(ObjectId::new(), false, None),
    ] {
        let mut graph = crate::derivation::Derivations::new();
        intern(&mut graph, node).expect("hash");
        assert!(crate::derivation::validate(&graph).is_ok());
        assert!(model.accept_derivations(feature, &graph).is_err());
    }
    let good = archive_feature(&mut kernel, &model, feature).expect("archive");
    let name = *good.entities.values().next().expect("name");
    let mut bad = good.clone();
    let parent = bad.derivations[&name.digest]
        .inputs()
        .next()
        .expect("parent");
    bad.derivations.remove(&parent.digest);
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    let wrong = expression(
        &mut bad.derivations,
        SubShapeKind::Edge,
        "generated",
        Some(feature),
        [name],
        255,
    )
    .expect("hash");
    bad.ancestry.insert(wrong, BTreeSet::new());
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    let root = bad
        .derivations
        .values()
        .find(|n| matches!(&***n, Derivation::Curve(..)))
        .expect("root")
        .name();
    bad.ancestry.insert(root, BTreeSet::new());
    assert!(bad.validate().is_err());
    let mut reader = mock::MockKernel::new();
    let mut wrong_model =
        support::prepared(ObjectId::new(), request.profile(), false).expect("other producer");
    assert!(restore_feature(&mut reader, &good, &mut wrong_model).is_err());
    assert!(wrong_model.features.is_empty());
    assert_eq!(reader.live_shape_count(), 0);
}

#[test]
fn roots_of_another_known_body_cannot_enter_a_feature_lineage() {
    let (mut kernel, mut model, feature, request) = plate();
    let other = ObjectId::new();
    let another = support::prepared(other, request.profile(), false).expect("other body");
    model.operations.extend(another.operations);
    model.planes.extend(another.planes);
    let root = crate::derivation::Derivation::Cap(other, false, model.planes.get(&other).copied());
    let mut graph = crate::derivation::Derivations::new();
    crate::derivation::intern(&mut graph, root).expect("hash");
    assert!(model.accept_derivations(feature, &graph).is_err());
    let archive = archive_feature(&mut kernel, &model, feature).expect("original graph unchanged");
    assert!(archive.validate().is_ok());
}
