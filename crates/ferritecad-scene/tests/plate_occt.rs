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

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

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
            let SemanticRole::ExtrudeCapEdge {
                side,
                profile_segment,
            } = &meaning.output_role
            else {
                panic!("an edge was named with a role that is not a cap edge");
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
