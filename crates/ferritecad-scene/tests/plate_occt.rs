// SPDX-License-Identifier: MIT
//! The committed plate, drawn from geometry Open CASCADE actually built.
//!
//! Everything else about the loader is settled against the mock, which is what
//! lets those rules be stated on every platform. What cannot be settled that
//! way is whether the numbers are real: a mock that reported a 60 x 40 x 10 box
//! would satisfy the same assertions while computing nothing. So this file runs
//! the same path against the pinned kernel and measures what came back.
//!
//! Skipped rather than failed on a build without Open CASCADE: its absence is a
//! build configuration. The pin workflow sets `FERRITECAD_REQUIRE_OCCT=1`, so
//! the run whose purpose is to prove the adapter works cannot pass by skipping.

use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_scene::snapshot_of;
use ferritecad_types::{CadError, Result};

/// The plate is a native document: nothing here reads a STEP file, and this
/// refusing before it can do anything is what says so.
fn no_imports<K>(_: &mut K, _: &[u8]) -> Result<ferritecad_exchange::Import> {
    Err(CadError::unsupported("the plate holds no imports"))
}

#[test]
fn the_plate_is_read_from_disk_into_real_geometry() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");
    let before = std::fs::read(&path).expect("reads the copy");

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let snapshot = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads")
    .snapshot;

    // One body, and a box's worth of triangles at the very least: six planar
    // faces cannot be covered by fewer than twelve.
    assert_eq!(snapshot.meshes().len(), 1);
    assert_eq!(snapshot.draws().len(), 1);
    assert!(
        snapshot.meshes()[0].triangle_count() >= 12,
        "a box came back as {} triangles",
        snapshot.meshes()[0].triangle_count()
    );

    // 60 x 40 x 10 is what the fixture describes, so it is what the kernel must
    // have built. A loader that dropped the extrusion or the placement would
    // still produce a mesh, and it would be the wrong size.
    let (min, max) = snapshot.bounds().expect("the plate has extent");
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    assert!((size[0] - 60.0).abs() < 1e-3, "{size:?}");
    assert!((size[1] - 40.0).abs() < 1e-3, "{size:?}");
    assert!((size[2] - 10.0).abs() < 1e-3, "{size:?}");

    // The real session, not a counter kept by a mock.
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the picture is packed and Open CASCADE is still holding the solids"
    );

    // And the document is exactly as it was found.
    assert_eq!(std::fs::read(&path).expect("reads the copy"), before);
    for sidecar in ["fcad-wal", "fcad-shm", "fcad-cache"] {
        assert!(
            !path.with_extension(sidecar).exists(),
            "reading the plate left a .{sidecar} beside it"
        );
    }
}

#[test]
fn a_cancelled_load_leaves_the_real_session_empty() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");

    // Load it once so the session has certainly held real shapes, then abandon
    // a second load. Cancellation is checked before each feature, so this ends
    // between the document being read and the first solid surviving.
    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");

    let cancel = ferritecad_kernel::CancelToken::new();
    cancel.cancel();
    let error = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default().with_cancel(cancel),
    )
    .expect_err("a cancelled load must not produce a picture");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
    assert_eq!(kernel.live_shape_count(), 0);
}

/// The plate, with an exact durable name written for every edge where a cap
/// meets a face raised from a profile segment.
///
/// The segment labels are read out of the references the fixture already
/// stores, so nothing here invents a label or depends on one being written
/// down twice.
fn plate_with_named_cap_edges(path: &std::path::Path) -> Vec<ferritecad_document::TopologyRef> {
    use ferritecad_document::{CapSide, Document, EntityKind, SelectionRule, SemanticRole};

    std::fs::copy(ferritecad_fixtures::plate_source(), path).expect("copies the fixture");
    let mut document = Document::open(path).expect("opens the plate");
    let stored = document.topology_refs().expect("reads");
    let sides: Vec<(ferritecad_types::ObjectId, ferritecad_types::StableEntityId)> = stored
        .iter()
        .filter_map(|reference| match &reference.output_role {
            SemanticRole::ExtrudeSide { profile_segment } => {
                Some((reference.producer_feature, *profile_segment))
            }
            _ => None,
        })
        .collect();
    assert!(
        !sides.is_empty(),
        "the fixture names the faces raised from its profile segments"
    );

    let owner = stored[0].owner;
    let mut written = Vec::new();
    for (producer, segment) in &sides {
        for side in [CapSide::Start, CapSide::End] {
            written.push(ferritecad_document::TopologyRef {
                id: ferritecad_types::StableEntityId::new(),
                owner,
                producer_feature: *producer,
                expected_kind: EntityKind::Edge,
                output_role: SemanticRole::ExtrudeCapEdge {
                    side,
                    profile_segment: *segment,
                },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            });
        }
    }
    document
        .write(|w| {
            for reference in &written {
                w.put_topology_ref(reference)?;
            }
            Ok(())
        })
        .expect("stores the cap edge references");
    written
}

#[test]
fn a_stored_cap_edge_name_reaches_the_edge_of_the_picture_it_names() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    let written = plate_with_named_cap_edges(&path);

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let scene = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");

    // The picture numbers the plate's twelve edges, and the document has a
    // name for eight of them.
    assert_eq!(
        scene.snapshot.edge_count(),
        12,
        "the plate draws twelve topological edges"
    );

    // Every stored reference must be found at exactly one of this picture's
    // edges, and no two references at the same one.
    let mut found: std::collections::BTreeMap<u32, Vec<ferritecad_types::StableEntityId>> =
        std::collections::BTreeMap::new();
    for ordinal in 0..scene.snapshot.edge_count() {
        let edge = scene
            .snapshot
            .edge_of(0, ordinal)
            .expect("the picture numbers this edge");
        for meaning in scene.edges.of(edge, &scene.snapshot) {
            found
                .entry(edge.to_raw())
                .or_default()
                .push(meaning.reference);
        }
    }

    for reference in &written {
        assert!(
            found.values().any(|names| names.contains(&reference.id)),
            "the stored name {} reached no edge of the picture",
            reference.id
        );
    }
    assert_eq!(found.len(), 8, "eight edges are named, and four are not");
    // Each of the eight carries exactly one name, so no two references landed
    // on one edge and no reference landed on two.
    for (edge, names) in &found {
        assert_eq!(names.len(), 1, "edge {edge} carries {} names", names.len());
    }

    // Start and End of one segment are different edges, and two segments are
    // different edges on one side. What each edge is called is read back and
    // compared as the document's own words.
    use ferritecad_document::{CapSide, SemanticRole};
    let mut by_role: std::collections::BTreeMap<(bool, ferritecad_types::StableEntityId), u32> =
        std::collections::BTreeMap::new();
    for ordinal in 0..scene.snapshot.edge_count() {
        let edge = scene.snapshot.edge_of(0, ordinal).expect("numbered");
        for meaning in scene.edges.of(edge, &scene.snapshot) {
            assert!(
                matches!(meaning.output_role, SemanticRole::ExtrudeCapEdge { .. }),
                "an edge was named with a role that is not a cap edge"
            );
            let SemanticRole::ExtrudeCapEdge {
                side,
                profile_segment,
            } = &meaning.output_role
            else {
                continue;
            };
            assert_eq!(meaning.expected_kind, ferritecad_document::EntityKind::Edge);
            // The words the document stored, unaltered. A meaning built by
            // reinterpreting the role would still be eight distinct pairs and
            // would name the wrong end of the sweep.
            let stored = written
                .iter()
                .find(|reference| reference.id == meaning.reference)
                .expect("every name here was one this test wrote");
            assert_eq!(
                meaning.output_role, stored.output_role,
                "what the picture reports is not what the document stored"
            );
            assert_eq!(meaning.owner, stored.owner);
            assert_eq!(meaning.producer_feature, stored.producer_feature);
            assert_eq!(meaning.selection, stored.selection);
            assert!(
                by_role
                    .insert((*side == CapSide::Start, *profile_segment), edge.to_raw())
                    .is_none(),
                "two edges answer to one side and segment"
            );
        }
    }
    assert_eq!(by_role.len(), 8, "four segments times two sides");
    for segment in by_role
        .keys()
        .map(|(_, segment)| *segment)
        .collect::<std::collections::BTreeSet<_>>()
    {
        assert_ne!(
            by_role[&(true, segment)],
            by_role[&(false, segment)],
            "the two ends of segment {segment} are one edge"
        );
    }

    // An identity of another picture names nothing here, including one whose
    // raw value is perfectly in range.
    let other = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("loads again");
    let named_again = (0..other.snapshot.edge_count())
        .filter_map(|ordinal| other.snapshot.edge_of(0, ordinal))
        .filter(|edge| !other.edges.of(*edge, &other.snapshot).is_empty())
        .count();
    assert_eq!(
        named_again, 8,
        "the same document read twice names the same eight edges"
    );
    // The two loads are the same document under the same interpretation, so
    // their pictures share an identity. The refusal of a foreign identity is
    // proved where the two interpretations genuinely differ, in the unit tests
    // beside the loader.

    // And what the public result shows a reader carries no kernel or transient
    // identity at all.
    let written_debug = format!("{:?}", scene.edges);
    for word in [
        "session#",
        "shape#",
        "face#",
        "edge#",
        "SubShapeHandle",
        "ShapeHandle",
        "SessionId",
        "FacePickId",
        "EdgePickId",
    ] {
        assert!(!written_debug.contains(word), "the edge names carry {word}");
    }

    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the picture is packed and the kernel still holds solids"
    );
}

#[test]
fn every_stored_name_of_one_edge_is_kept_in_document_order() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    use ferritecad_document::{CapSide, Document, EntityKind, SelectionRule, SemanticRole};

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");

    let mut document = Document::open(&path).expect("opens");
    let stored = document.topology_refs().expect("reads");
    let (producer, segment) = stored
        .iter()
        .find_map(|reference| match &reference.output_role {
            SemanticRole::ExtrudeSide { profile_segment } => {
                Some((reference.producer_feature, *profile_segment))
            }
            _ => None,
        })
        .expect("the fixture names a swept face");
    let owner = stored[0].owner;

    // Three references for one edge, and one family reference which is not a
    // name for a single edge at all.
    let exact: Vec<ferritecad_types::StableEntityId> = (0..3)
        .map(|_| ferritecad_types::StableEntityId::new())
        .collect();
    let family = ferritecad_types::StableEntityId::new();
    document
        .write(|w| {
            for id in &exact {
                w.put_topology_ref(&ferritecad_document::TopologyRef {
                    id: *id,
                    owner,
                    producer_feature: producer,
                    expected_kind: EntityKind::Edge,
                    output_role: SemanticRole::ExtrudeCapEdge {
                        side: CapSide::Start,
                        profile_segment: segment,
                    },
                    selection: SelectionRule::Exact,
                    fallback_signature: None,
                })?;
            }
            w.put_topology_ref(&ferritecad_document::TopologyRef {
                id: family,
                owner,
                producer_feature: producer,
                expected_kind: EntityKind::Edge,
                output_role: SemanticRole::ExtrudeCapEdge {
                    side: CapSide::Start,
                    profile_segment: segment,
                },
                selection: SelectionRule::AllDerivedFrom { ancestor: segment },
                fallback_signature: None,
            })
        })
        .expect("stores");

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let scene = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");

    let named: Vec<Vec<ferritecad_types::StableEntityId>> = (0..scene.snapshot.edge_count())
        .filter_map(|ordinal| scene.snapshot.edge_of(0, ordinal))
        .map(|edge| {
            scene
                .edges
                .of(edge, &scene.snapshot)
                .iter()
                .map(|meaning| meaning.reference)
                .collect()
        })
        .filter(|names: &Vec<_>| !names.is_empty())
        .collect();

    assert_eq!(named.len(), 1, "one edge is named");
    // All three, and in the order the document stores them: choosing one would
    // be presenting storage order as a decision about which name is right.
    assert_eq!(
        named[0], exact,
        "the three stored names of one edge did not all survive in order"
    );
    // And the family reference is nowhere: it selects a family, and a family
    // of one is still not a name for that one.
    assert!(
        !named[0].contains(&family),
        "a family reference was taken as the name of a single edge"
    );
}

#[test]
fn two_readings_of_one_picture_differ_when_only_the_edge_names_do() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    let directory = tempfile::tempdir().expect("a temporary directory is available");

    // The same geometry twice: once as the fixture stores it, once with edge
    // names added and nothing else changed.
    let plain_path = directory.path().join("plain.fcad");
    std::fs::copy(ferritecad_fixtures::plate_source(), &plain_path).expect("copies");
    let named_path = directory.path().join("named.fcad");
    plate_with_named_cap_edges(&named_path);

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let load = |kernel: &mut OcctKernel, path: &std::path::Path| {
        snapshot_of(
            path,
            kernel,
            no_imports,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("loads")
    };
    let plain = load(&mut kernel, &plain_path);
    let named = load(&mut kernel, &named_path);

    // The triangles are the same picture and the edge partition is the same;
    // only what the document says about it changed.
    assert_eq!(
        plain.snapshot.meshes()[0].indices(),
        named.snapshot.meshes()[0].indices()
    );
    assert_eq!(plain.snapshot.edge_count(), named.snapshot.edge_count());

    // So the two interpretations must not share transient identities: an
    // identity issued under one reading resolves to nothing under the other.
    let from_plain = plain.snapshot.edge_of(0, 0).expect("numbered");
    let from_named = named.snapshot.edge_of(0, 0).expect("numbered");
    assert_eq!(
        from_plain.to_raw(),
        from_named.to_raw(),
        "the same raw value"
    );
    assert_eq!(
        named.snapshot.definition_of_edge(from_plain),
        None,
        "adding edge names left the old transient identities valid"
    );
    assert_eq!(plain.snapshot.definition_of_edge(from_named), None);

    // And the faces of the two readings are equally separated.
    let face = plain.snapshot.face_of(0, 0).expect("numbered");
    assert_eq!(named.snapshot.definition_of_face(face), None);
}

/// The committed plate, copied, with an exact name for every edge that runs
/// along the sweep and for every cap edge too.
///
/// The corners come from `ProfileLoop::joints`. Pairing the sketch curves here
/// would be a second piece of adjacency arithmetic, free to disagree with the
/// one the kernel and the topology map already share.
fn plate_with_every_edge_named(
    path: &std::path::Path,
) -> (
    Vec<ferritecad_document::TopologyRef>,
    Vec<ferritecad_document::TopologyRef>,
) {
    use ferritecad_document::{Document, EntityKind, ObjectPayload, SelectionRule, SemanticRole};

    let caps = plate_with_named_cap_edges(path);

    let mut document = Document::open(path).expect("opens the plate");
    let objects = document.objects().expect("reads objects");
    let sketch = objects
        .iter()
        .find_map(|object| match &object.payload {
            ObjectPayload::Sketch(sketch) => Some(sketch.clone()),
            _ => None,
        })
        .expect("the fixture has a sketch");
    let datum = objects
        .iter()
        .find_map(|object| match &object.payload {
            ObjectPayload::DatumPlane(datum) => Some(datum.clone()),
            _ => None,
        })
        .expect("the fixture has a datum plane");
    let plane = ferritecad_eval::plane_from_datum(&datum).expect("reads the plane");
    let profile = ferritecad_eval::profile_from_sketch(&sketch, plane).expect("builds a profile");

    let stored = document.topology_refs().expect("reads");
    let producer = stored
        .iter()
        .find_map(|reference| match &reference.output_role {
            SemanticRole::ExtrudeSide { .. } => Some(reference.producer_feature),
            _ => None,
        })
        .expect("the fixture names its swept faces");
    let owner = stored[0].owner;

    let mut written = Vec::new();
    for joint in profile.outer().joints() {
        written.push(ferritecad_document::TopologyRef {
            id: ferritecad_types::StableEntityId::new(),
            owner,
            producer_feature: producer,
            expected_kind: EntityKind::Edge,
            output_role: SemanticRole::ExtrudeSweepEdge { joint },
            selection: SelectionRule::Exact,
            fallback_signature: None,
        });
    }
    assert_eq!(written.len(), 4, "the plate has four corners");
    document
        .write(|w| {
            for reference in &written {
                w.put_topology_ref(reference)?;
            }
            Ok(())
        })
        .expect("stores the sweep edge references");
    (caps, written)
}

#[test]
fn the_sweep_edge_names_and_the_cap_edge_names_together_cover_the_plate() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    use ferritecad_document::SemanticRole;

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    let (caps, sweeps) = plate_with_every_edge_named(&path);

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let scene = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");
    assert_eq!(scene.snapshot.edge_count(), 12);

    // Which edges each kind of name reached.
    let mut along = std::collections::BTreeSet::new();
    let mut at_caps = std::collections::BTreeSet::new();
    let mut otherwise = Vec::new();
    let mut reached: std::collections::BTreeSet<ferritecad_types::StableEntityId> =
        std::collections::BTreeSet::new();
    for ordinal in 0..scene.snapshot.edge_count() {
        let edge = scene
            .snapshot
            .edge_of(0, ordinal)
            .expect("the picture numbers this edge");
        for meaning in scene.edges.of(edge, &scene.snapshot) {
            reached.insert(meaning.reference);
            match meaning.output_role {
                SemanticRole::ExtrudeSweepEdge { .. } => {
                    along.insert(edge.to_raw());
                }
                SemanticRole::ExtrudeCapEdge { .. } => {
                    at_caps.insert(edge.to_raw());
                }
                ref other => otherwise.push(format!("{other:?}")),
            }
        }
    }

    assert!(
        otherwise.is_empty(),
        "an edge of the plate is named something else: {otherwise:?}"
    );

    // Four along the sweep, eight at the caps, and no edge answering to both.
    assert_eq!(along.len(), 4, "four edges run along the sweep");
    assert_eq!(at_caps.len(), 8, "eight edges bound a cap");
    assert!(
        along.is_disjoint(&at_caps),
        "an edge is named both along the sweep and at a cap"
    );
    assert_eq!(
        along.union(&at_caps).count(),
        12,
        "the twelve edges are not covered exactly"
    );

    // And every stored reference of either kind found its edge.
    for reference in caps.iter().chain(sweeps.iter()) {
        assert!(
            reached.contains(&reference.id),
            "the stored name {} reached no edge of the picture",
            reference.id
        );
    }

    // Which edge, not merely how many. A corner's name must land on the edge
    // between the two faces raised from its own two segments; landing on the
    // corner beside it would keep every count above intact.
    let face_of_segment = |segment: ferritecad_types::StableEntityId| {
        (0..scene.snapshot.face_count())
            .filter_map(|ordinal| scene.snapshot.face_of(0, ordinal))
            .find(|face| {
                scene.faces.of(*face, &scene.snapshot).iter().any(|meaning| {
                    matches!(
                        meaning.output_role,
                        SemanticRole::ExtrudeSide { profile_segment } if profile_segment == segment
                    )
                })
            })
            .expect("the picture has a face raised from every profile segment")
    };
    let mut checked = 0;
    for ordinal in 0..scene.snapshot.edge_count() {
        let edge = scene
            .snapshot
            .edge_of(0, ordinal)
            .expect("the picture numbers this edge");
        for meaning in scene.edges.of(edge, &scene.snapshot) {
            let SemanticRole::ExtrudeSweepEdge { joint } = &meaning.output_role else {
                continue;
            };
            for segment in joint.segments() {
                let face = face_of_segment(segment);
                assert!(
                    scene.snapshot.edge_bounds_face(edge, face),
                    "the name of the corner of segments {:?} landed on an edge that does not \
                     touch the face raised from {segment}",
                    joint.segments()
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 4, "four corners were checked against their faces");
}

#[test]
fn a_picture_with_no_sweep_edge_names_invents_none() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    use ferritecad_document::SemanticRole;

    // The same plate with cap-edge names only. The four edges along the sweep
    // must come back unnamed rather than borrowing a neighbour's name.
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    let _caps = plate_with_named_cap_edges(&path);

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let scene = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");

    let mut unnamed = 0;
    for ordinal in 0..scene.snapshot.edge_count() {
        let edge = scene
            .snapshot
            .edge_of(0, ordinal)
            .expect("the picture numbers this edge");
        let meanings = scene.edges.of(edge, &scene.snapshot);
        if meanings.is_empty() {
            unnamed += 1;
            continue;
        }
        for meaning in meanings {
            assert!(
                !matches!(meaning.output_role, SemanticRole::ExtrudeSweepEdge { .. }),
                "a picture with no sweep-edge names produced one"
            );
        }
    }
    assert_eq!(unnamed, 4, "the four edges along the sweep stay unnamed");
}

#[test]
fn every_stored_sweep_edge_name_of_one_edge_is_kept_in_document_order() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    use ferritecad_document::{Document, EntityKind, SelectionRule, SemanticRole};

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    let (_caps, sweeps) = plate_with_every_edge_named(&path);

    // A second and a third exact reference to one corner, which a document may
    // hold: two objects can both name the same edge.
    let first = sweeps.first().expect("four corners were written").clone();
    let mut again = Vec::new();
    for _ in 0..2 {
        let mut copy = first.clone();
        copy.id = ferritecad_types::StableEntityId::new();
        again.push(copy);
    }
    let mut document = Document::open(&path).expect("opens the plate");
    document
        .write(|w| {
            for reference in &again {
                w.put_topology_ref(reference)?;
            }
            Ok(())
        })
        .expect("stores the extra references");
    let _ = (EntityKind::Edge, SelectionRule::Exact);
    drop(document);

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let scene = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");

    // The corner now carries three names, and they arrive in the order the
    // document stores them rather than in the order they were written.
    let wanted: std::collections::BTreeSet<ferritecad_types::StableEntityId> =
        std::iter::once(first.id)
            .chain(again.iter().map(|reference| reference.id))
            .collect();
    let mut seen = Vec::new();
    for ordinal in 0..scene.snapshot.edge_count() {
        let edge = scene
            .snapshot
            .edge_of(0, ordinal)
            .expect("the picture numbers this edge");
        let names: Vec<ferritecad_types::StableEntityId> = scene
            .edges
            .of(edge, &scene.snapshot)
            .iter()
            .filter(|meaning| {
                matches!(meaning.output_role, SemanticRole::ExtrudeSweepEdge { .. })
                    && wanted.contains(&meaning.reference)
            })
            .map(|meaning| meaning.reference)
            .collect();
        if !names.is_empty() {
            seen.push(names);
        }
    }
    assert_eq!(seen.len(), 1, "three names of one corner reached one edge");
    let names = &seen[0];
    assert_eq!(names.len(), 3);

    let stored = Document::open(&path)
        .expect("reopens")
        .topology_refs()
        .expect("reads");
    let order: Vec<ferritecad_types::StableEntityId> = stored
        .iter()
        .filter(|entry| wanted.contains(&entry.id))
        .map(|entry| entry.id)
        .collect();
    assert_eq!(names, &order, "the names are not in the document's order");
}

#[test]
fn a_stale_or_foreign_edge_identity_names_no_sweep_edge() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    let _ = plate_with_every_edge_named(&path);

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let scene = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");

    // A different picture: the same plate read with cap-edge names only. It
    // is a different interpretation of the geometry, so its names must not
    // answer about this one's edges. Two readings of the *same* file would
    // not do: a picture's identity is what it means, so two readings of one
    // unchanged document are the same picture and answer alike.
    let other_directory = tempfile::tempdir().expect("a temporary directory is available");
    let other_path = other_directory.path().join("plate.fcad");
    let _ = plate_with_named_cap_edges(&other_path);
    let other = snapshot_of(
        &other_path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the other plate loads");

    let named = (0..scene.snapshot.edge_count())
        .filter_map(|ordinal| scene.snapshot.edge_of(0, ordinal))
        .find(|edge| !scene.edges.of(*edge, &scene.snapshot).is_empty())
        .expect("the document names an edge");

    assert!(
        other.edges.of(named, &scene.snapshot).is_empty(),
        "one picture's names answered about another picture's edge"
    );
    assert!(
        scene
            .edges
            .of(ferritecad_viewport::EdgePickId::NOTHING, &scene.snapshot)
            .is_empty(),
        "an edge identity of nothing was given a name"
    );
    // A number this picture never issued does not become an edge of it at
    // all, so it cannot carry a name either.
    let past_the_end = ferritecad_viewport::EdgePickId::from_raw(
        u32::try_from(scene.snapshot.edge_count()).expect("small") + 500,
        &scene.snapshot,
    );
    assert_eq!(past_the_end, ferritecad_viewport::EdgePickId::NOTHING);
    assert!(
        scene.edges.of(past_the_end, &scene.snapshot).is_empty(),
        "an edge identity this picture never issued was given a name"
    );
}
