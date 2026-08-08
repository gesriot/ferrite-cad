// SPDX-License-Identifier: MIT
//! Rebuilding from a cache, and refusing to depend on one.
//!
//! A warm rebuild is only worth having if it is indistinguishable from a cold
//! one except in time. These tests hold it to that: the same names resolve to
//! the same six faces, the kernel is provably not asked to compute anything on
//! a hit, and every way the cache can disappoint — empty, damaged, written by
//! another build — costs a rebuild and nothing else.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_document::{
    Body, CacheStore, CapSide, DatumPlane, Dependency, DependencyRole, Document, EndCondition,
    EntityKind, Expression, Extrude, ObjectPayload, Point2, SelectionRule, SemanticRole, Sketch,
    SketchCurve, SketchGeometry, SolidOperation, TopologyRef,
};
use ferritecad_eval::{
    CacheOutcome, RebuildResult, extrude_archive_key, extrude_request, profile_from_sketch,
    rebuild_cached, rebuild_cold,
};
use ferritecad_kernel::{
    CancelToken, GeometryKernel, OperationContext, ProgressSink, SketchPlane, mock::MockKernel,
};
use ferritecad_topology::ARCHIVE_CACHE_KIND;
use ferritecad_types::{ContentHash, DocumentId, ObjectId, Result, StableEntityId, Transform};

/// The plate, plus the references a document would store about it.
struct Plate {
    extrude: ObjectId,
    body: ObjectId,
    /// A second extrusion of the same profile, evaluated after the first.
    second: ObjectId,
    start_cap: TopologyRef,
    end_cap: TopologyRef,
    /// One per segment, selected as a family.
    sides: Vec<TopologyRef>,
}

fn cap_reference(feature: ObjectId, side: CapSide) -> TopologyRef {
    TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Face,
        output_role: SemanticRole::ExtrudeCap { side },
        selection: SelectionRule::Exact,
        fallback_signature: None,
    }
}

fn side_reference(feature: ObjectId, segment: StableEntityId) -> TopologyRef {
    TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Face,
        output_role: SemanticRole::ExtrudeSide {
            profile_segment: segment,
        },
        selection: SelectionRule::AllDerivedFrom { ancestor: segment },
        fallback_signature: None,
    }
}

/// Writes the plate. `order` permutes how the curves are stored, which must
/// change nothing about what resolves.
fn populate(document: &mut Document, height: f64, order: &[usize]) -> Result<Plate> {
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let extrude = ObjectId::new();
    let body = ObjectId::new();
    let second = ObjectId::new();
    let second_body = ObjectId::new();
    let segments: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();

    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let mut curves = Vec::new();
    for index in order {
        let start = corners[*index];
        let end = corners[(index + 1) % corners.len()];
        curves.push(SketchCurve {
            id: segments[*index],
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(start.0, start.1)?,
                end: Point2::new(end.0, end.1)?,
            },
        });
    }

    document.write(|w| {
        w.put_object(
            plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;
        w.put_object(
            sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch { plane, curves }),
        )?;
        w.add_dependency(Dependency {
            dependent: sketch,
            dependency: plane,
            role: DependencyRole::Plane,
        })?;
        w.put_object(
            extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(height)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                target_body: None,
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: extrude,
            dependency: sketch,
            role: DependencyRole::Profile,
        })?;
        w.put_object(
            body,
            None,
            3,
            Some("Plate"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(extrude),
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: body,
            dependency: extrude,
            role: DependencyRole::BodyTip,
        })?;

        // A second solid from the same sketch. It exists so a rebuild has
        // somewhere to fail after the first feature has already been restored.
        w.put_object(
            second,
            None,
            4,
            Some("Extrude2"),
            &ObjectPayload::Extrude(Extrude {
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(height * 2.0)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                target_body: None,
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: second,
            dependency: sketch,
            role: DependencyRole::Profile,
        })?;
        w.put_object(
            second_body,
            None,
            5,
            Some("Plate2"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(second),
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: second_body,
            dependency: second,
            role: DependencyRole::BodyTip,
        })?;

        // The document stores these; the rebuild resolves them.
        for reference in [
            cap_reference(extrude, CapSide::Start),
            cap_reference(extrude, CapSide::End),
        ] {
            w.put_topology_ref(&reference)?;
        }
        for segment in &segments {
            w.put_topology_ref(&side_reference(extrude, *segment))?;
        }
        Ok(())
    })?;

    Ok(Plate {
        extrude,
        body,
        second,
        start_cap: cap_reference(extrude, CapSide::Start),
        end_cap: cap_reference(extrude, CapSide::End),
        sides: segments
            .iter()
            .map(|s| side_reference(extrude, *s))
            .collect(),
    })
}

fn sample(height: f64) -> (tempfile::TempDir, Document, Plate) {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = Document::create(dir.path().join("plate.fcad")).expect("creates");
    let plate = populate(&mut document, height, &[0, 1, 2, 3]).expect("populates");
    (dir, document, plate)
}

use std::path::Path;

fn store(dir: &Path, kernel: &MockKernel, document: DocumentId) -> CacheStore {
    CacheStore::open(
        dir.join("plate.fcad-cache"),
        document,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("the sidecar opens")
}

/// The six faces the plate's stored references name, sorted for comparison.
fn named_faces(built: &RebuildResult, plate: &Plate) -> Vec<String> {
    let mut all: Vec<String> = plate
        .sides
        .iter()
        .chain([&plate.start_cap, &plate.end_cap])
        .flat_map(|reference| {
            built
                .resolve(reference)
                .unwrap_or_else(|e| panic!("every stored reference must resolve: {e}"))
        })
        .map(|face| face.to_string())
        .collect();
    all.sort();
    all
}

/// Where one extrusion keeps its archive.
fn archive_key(document: &Document, kernel: &MockKernel, feature: ObjectId) -> ContentHash {
    let objects = document.objects().expect("reads objects");
    let sketch = objects
        .iter()
        .find_map(|object| match &object.payload {
            ObjectPayload::Sketch(sketch) => Some(sketch.clone()),
            _ => None,
        })
        .expect("the plate has a sketch");
    let feature = objects
        .iter()
        .find_map(|object| match &object.payload {
            ObjectPayload::Extrude(extrude) if object.id == feature => Some(extrude.clone()),
            _ => None,
        })
        .expect("the plate has an extrusion");

    let profile = profile_from_sketch(&sketch, SketchPlane::world_xy()).expect("converts");
    let request = extrude_request(&feature, profile).expect("converts");
    extrude_archive_key(kernel.identity(), &request, &OperationContext::default())
}

#[test]
fn a_second_rebuild_restores_instead_of_computing() {
    let (dir, document, plate) = sample(10.0);
    let document_id = document.meta().document_id;
    let context = OperationContext::default();

    // The first run: cold in effect, and it leaves an archive behind.
    let mut first = MockKernel::new();
    let mut cache = store(dir.path(), &first, document_id);
    let (built, events) =
        rebuild_cached(&document, &mut first, &mut cache, &context).expect("rebuilds");

    assert_eq!(first.extrude_count(), 2, "nothing was cached yet");
    assert_eq!(
        events
            .iter()
            .filter(|e| e.outcome == CacheOutcome::Miss)
            .count(),
        2,
        "both extrusions had to be computed"
    );
    assert!(
        events
            .iter()
            .all(|e| e.outcome != CacheOutcome::WriteFailed),
        "the archive should have been written: {events:?}"
    );
    let before = named_faces(&built, &plate);
    built.release_all(&mut first);
    drop(cache);

    // A different session, which has never seen this geometry.
    let mut second = MockKernel::new();
    let mut cache = store(dir.path(), &second, document_id);
    let (built, events) =
        rebuild_cached(&document, &mut second, &mut cache, &context).expect("rebuilds");

    assert_eq!(
        second.extrude_count(),
        0,
        "a hit must not ask the kernel to compute anything"
    );
    assert_eq!(
        events.iter().map(|e| e.outcome).collect::<Vec<_>>(),
        vec![CacheOutcome::Hit, CacheOutcome::Hit]
    );

    // Same references, same six distinct faces, resolved out of a file.
    let after = named_faces(&built, &plate);
    assert_eq!(after.len(), 6);
    assert_eq!(
        after
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        6,
        "six names must not collapse onto fewer faces"
    );
    assert_ne!(
        after, before,
        "handles belong to the session that made them"
    );

    built.release_all(&mut second);
    assert_eq!(second.live_shape_count(), 0);
}

#[test]
fn a_damaged_entry_costs_a_rebuild_and_is_replaced() {
    let (dir, document, plate) = sample(10.0);
    let document_id = document.meta().document_id;
    let context = OperationContext::default();

    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document_id);
    let (built, _) =
        rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("rebuilds");
    built.release_all(&mut kernel);

    // Bytes a sidecar will hand back intact and a codec will not accept.
    let key = archive_key(&document, &kernel, plate.extrude);
    cache
        .put(plate.extrude, key, ARCHIVE_CACHE_KIND, b"not an archive")
        .expect("overwrites the entry");
    drop(cache);

    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document_id);
    let (built, events) =
        rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("rebuilds anyway");

    assert_eq!(
        kernel.extrude_count(),
        1,
        "only the refused entry falls back to computing its feature"
    );
    assert_eq!(events[0].outcome, CacheOutcome::Rejected);
    assert!(events[0].detail.is_some(), "a refusal should say why");
    assert_eq!(named_faces(&built, &plate).len(), 6);
    built.release_all(&mut kernel);
    drop(cache);

    // The cold fallback replaced the damaged entry, so the next run hits.
    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document_id);
    let (built, events) =
        rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("rebuilds");
    assert_eq!(kernel.extrude_count(), 0);
    assert_eq!(events[0].outcome, CacheOutcome::Hit);
    built.release_all(&mut kernel);
}

#[test]
fn cancelling_after_a_hit_releases_what_was_restored() {
    let (dir, document, plate) = sample(10.0);
    let document_id = document.meta().document_id;

    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document_id);
    let (built, _) = rebuild_cached(
        &document,
        &mut kernel,
        &mut cache,
        &OperationContext::default(),
    )
    .expect("rebuilds");
    built.release_all(&mut kernel);

    // Leave the first extrusion's archive alone and ruin the second's, so the
    // next rebuild restores one feature and has to compute the other.
    let key = archive_key(&document, &kernel, plate.second);
    cache
        .put(plate.second, key, ARCHIVE_CACHE_KIND, b"not an archive")
        .expect("overwrites");
    drop(cache);

    // Cancelled from the kernel's own progress callback, which fires while the
    // second extrusion is computed — by which time the first has been decoded
    // out of the cache and is being held.
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    let context = OperationContext::default()
        .with_cancel(cancel)
        .with_progress(ProgressSink::new(move |_| trigger.cancel()));

    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document_id);
    let err = rebuild_cached(&document, &mut kernel, &mut cache, &context)
        .expect_err("a cancelled rebuild returns no result");

    assert_eq!(err.kind(), ferritecad_types::ErrorKind::Cancellation);
    assert_eq!(
        kernel.extrude_count(),
        1,
        "only the ruined entry was rebuilt"
    );
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a shape decoded out of the cache is owned from the moment it exists"
    );
}

#[test]
fn a_later_failure_releases_what_the_cache_restored() {
    let (dir, mut document, plate) = sample(10.0);
    let document_id = document.meta().document_id;
    let context = OperationContext::default();

    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document_id);
    let (built, _) =
        rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("rebuilds");
    built.release_all(&mut kernel);
    drop(cache);

    // Break the body that follows the extrusion, so the rebuild fails after
    // the cache has already produced a solid.
    document
        .write(|w| {
            w.put_object(
                plate.body,
                None,
                3,
                Some("Plate"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(ObjectId::new()),
                }),
            )
        })
        .expect("writes");

    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document_id);
    assert!(rebuild_cached(&document, &mut kernel, &mut cache, &context).is_err());
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a failed rebuild hands back cached geometry too"
    );
}

#[test]
fn a_cold_rebuild_neither_reads_nor_writes_the_cache() {
    let (dir, document, plate) = sample(10.0);
    let document_id = document.meta().document_id;
    let context = OperationContext::default();

    // A sidecar holding a perfectly good archive.
    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document_id);
    let (built, _) =
        rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("rebuilds");
    built.release_all(&mut kernel);
    let key = archive_key(&document, &kernel, plate.extrude);
    let stored = cache
        .get(plate.extrude, key, ARCHIVE_CACHE_KIND)
        .expect("reads")
        .expect("the archive is there");
    drop(cache);

    let mut cold = MockKernel::new();
    let built = rebuild_cold(&document, &mut cold, &context).expect("rebuilds");
    assert_eq!(
        cold.extrude_count(),
        2,
        "the cold path must compute even when an archive exists"
    );
    assert_eq!(named_faces(&built, &plate).len(), 6);
    built.release_all(&mut cold);

    let cache = store(dir.path(), &cold, document_id);
    assert_eq!(
        cache
            .get(plate.extrude, key, ARCHIVE_CACHE_KIND)
            .expect("reads")
            .expect("the archive is still there"),
        stored,
        "the cold path must not have touched the entry"
    );
}

#[test]
fn an_entry_from_another_kernel_build_is_a_miss() {
    let (dir, document, _) = sample(10.0);
    let document_id = document.meta().document_id;
    let context = OperationContext::default();

    let mut writer = MockKernel::new();
    let mut cache = store(dir.path(), &writer, document_id);
    let (built, _) =
        rebuild_cached(&document, &mut writer, &mut cache, &context).expect("rebuilds");
    built.release_all(&mut writer);
    drop(cache);

    // A kernel claiming another version. The sidecar itself is discarded on
    // open, so this is a miss before the codec is ever reached.
    let mut other = MockKernel::with_version("9.9.9");
    let mut cache = store(dir.path(), &other, document_id);
    let (built, events) =
        rebuild_cached(&document, &mut other, &mut cache, &context).expect("rebuilds");

    assert_eq!(other.extrude_count(), 2);
    assert!(
        events.iter().all(|e| e.outcome == CacheOutcome::Miss),
        "a sidecar bound to another kernel is discarded whole: {events:?}"
    );
    built.release_all(&mut other);
}
