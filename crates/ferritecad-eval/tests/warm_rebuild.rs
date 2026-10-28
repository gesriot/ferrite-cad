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
    Body, CacheStore, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression,
    Extrude, ObjectPayload, Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
};
use ferritecad_eval::{
    CacheOutcome, RebuildResult, extrude_archive_key, extrude_request, profile_from_sketch,
    rebuild_cached, rebuild_cold,
};
use ferritecad_kernel::{
    CancelToken, GeometryKernel, OperationContext, ProgressSink, SketchPlane, mock::MockKernel,
};
use ferritecad_topology::ARCHIVE_CACHE_KIND;
use ferritecad_topology::generative::Name;
#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;
use ferritecad_types::{ContentHash, DocumentId, ObjectId, Result, StableEntityId, Transform};

/// The plate, plus the references a document would store about it.
struct Plate {
    extrude: ObjectId,
    body: ObjectId,
    /// A second extrusion of the same profile, evaluated after the first.
    second: ObjectId,
    start_cap: Name,
    end_cap: Name,
    /// One per segment, selected as a family.
    sides: Vec<Name>,
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
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves,
                constraints: Vec::new(),
            }),
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
                regions: Vec::new(),
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(height)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                previous: None,
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
                regions: Vec::new(),
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(height * 2.0)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                previous: None,
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

        Ok(())
    })?;

    Ok(Plate {
        extrude,
        body,
        second,
        start_cap: naming::cap_name(extrude, plane, false),
        end_cap: naming::cap_name(extrude, plane, true),
        sides: segments
            .iter()
            .map(|s| naming::side_name(extrude, plane, *s))
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
            naming::resolve(built.topology(), plate.extrude, reference)
                .unwrap_or_else(|e| panic!("every stored reference must resolve: {e}"))
        })
        .map(|face| face.to_string())
        .collect();
    all.sort();
    all
}

/// Where one extrusion keeps its archive.
fn archive_key(document: &Document, kernel: &MockKernel, feature: ObjectId) -> ContentHash {
    let owner = feature;
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

    let profile = profile_from_sketch(
        &sketch,
        ferritecad_types::ObjectId::new(),
        SketchPlane::world_xy(),
    )
    .expect("converts")
    .profile;
    let request = extrude_request(&feature, profile, None).expect("converts");
    let mut names = ferritecad_topology::ModelNaming::new();
    names
        .prepare(&objects.into_iter().map(|o| (o.id, o)).collect())
        .expect("roots");
    names.cache_key(
        owner,
        extrude_archive_key(kernel.identity(), &request, &OperationContext::default()),
    )
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
fn a_later_failure_keeps_what_the_cache_restored() {
    let (dir, mut document, plate) = sample(10.0);
    let document_id = document.meta().document_id;
    let context = OperationContext::default();

    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document_id);
    let (built, _) =
        rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("rebuilds");
    built.release_all(&mut kernel);
    drop(cache);

    // Keep the document semantically valid, but make the second extrusion an
    // operation this evaluator does not implement. Validation must pass, the
    // first extrusion must be restored, and only then may conversion of the
    // second one fail. A missing body tip would be a vacuous test: validation
    // catches it before the rebuild loop and no cached shape ever exists.
    let mut unsupported = document
        .objects()
        .expect("reads objects")
        .into_iter()
        .find_map(|object| match object.payload {
            ObjectPayload::Extrude(feature) if object.id == plate.second => Some(feature),
            _ => None,
        })
        .expect("the second extrusion exists");
    // Initially these are independent bodies. Once the second feature
    // consumes the first, only its body may own the combined history.
    assert_eq!(
        document
            .objects()
            .expect("reads objects")
            .into_iter()
            .find_map(|object| match object.payload {
                ObjectPayload::Body(body) if object.id == plate.body => Some(body.tip_feature),
                _ => None,
            }),
        Some(Some(plate.extrude))
    );

    // Said the way this build says it: a feature that changes an existing
    // solid names the *feature* whose result it changes. Naming the body
    // instead would close the loop the body's own tip edge opens, and the
    // document would fail to order rather than reaching the conversion this
    // test is about.
    unsupported.operation = SolidOperation::Add;
    unsupported.previous = Some(plate.extrude);
    document
        .write(|w| {
            w.put_object(
                plate.second,
                None,
                4,
                Some("Extrude2"),
                &ObjectPayload::Extrude(unsupported),
            )?;
            w.add_dependency(Dependency {
                dependent: plate.second,
                dependency: plate.extrude,
                role: DependencyRole::Predecessor,
            })?;
            w.put_object(
                plate.body,
                None,
                3,
                Some("Plate"),
                &ObjectPayload::Body(Body { tip_feature: None }),
            )?;
            w.remove_dependency(Dependency {
                dependent: plate.body,
                dependency: plate.extrude,
                role: DependencyRole::BodyTip,
            })
        })
        .expect("writes");
    let report = document.validate().expect("validates");
    assert!(
        report.is_ok(),
        "the failure must follow cache restoration: {report:?}"
    );

    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document_id);
    let (built, _) =
        rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("partial rebuild");
    assert!(matches!(
        built.status(plate.second),
        Some(ferritecad_eval::RebuildStatus::Failed { .. })
    ));
    assert_eq!(
        kernel.extrude_count(),
        1,
        "only the Add tool was built; its predecessor was restored"
    );
    assert_eq!(kernel.live_shape_count(), 1);
    let warm_report = built.report();
    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
    let cold = rebuild_cold(&document, &mut kernel, &context).expect("partial cold rebuild");
    assert_eq!(warm_report, cold.report());
    cold.release_all(&mut kernel);
    let (again, events) =
        rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("again");
    assert_eq!(warm_report, again.report());
    assert!(
        !events
            .iter()
            .any(|e| e.feature == plate.second && e.outcome == CacheOutcome::Hit)
    );
    again.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
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
fn failed_archive_writes_do_not_fail_the_rebuild() {
    let (dir, document, plate) = sample(10.0);
    let document_id = document.meta().document_id;
    let context = OperationContext::default();
    let mut kernel = MockKernel::new();
    kernel.refuse_named_encoding();
    let mut cache = store(dir.path(), &kernel, document_id);

    let (built, events) = rebuild_cached(&document, &mut kernel, &mut cache, &context)
        .expect("cache write failures cannot invalidate computed geometry");

    assert_eq!(kernel.extrude_count(), 2);
    assert_eq!(named_faces(&built, &plate).len(), 6);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.outcome == CacheOutcome::Miss)
            .count(),
        2
    );
    let failed: Vec<_> = events
        .iter()
        .filter(|event| event.outcome == CacheOutcome::WriteFailed)
        .collect();
    assert_eq!(failed.len(), 2);
    assert!(failed.iter().all(|event| event.detail.is_some()));

    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
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

    // Same id and version, another build. The sidecar stays in place; the full
    // identity inside each operation key must make only this build miss.
    let mut other = MockKernel::with_build("another-bridge-build");
    let mut cache = store(dir.path(), &other, document_id);
    let (built, events) =
        rebuild_cached(&document, &mut other, &mut cache, &context).expect("rebuilds");

    assert_eq!(other.extrude_count(), 2);
    assert!(
        events.iter().all(|e| e.outcome == CacheOutcome::Miss),
        "another build must not find entries under the original build's keys: {events:?}"
    );
    built.release_all(&mut other);
    drop(cache);

    // The other build wrote different keys rather than replacing or causing
    // the original build's entries to be discarded.
    let mut original = MockKernel::new();
    let mut cache = store(dir.path(), &original, document_id);
    let (built, events) = rebuild_cached(&document, &mut original, &mut cache, &context)
        .expect("the original build still rebuilds");
    assert_eq!(original.extrude_count(), 0);
    assert!(events.iter().all(|e| e.outcome == CacheOutcome::Hit));
    built.release_all(&mut original);
}

#[test]
fn failed_kernel_results_are_never_cached_and_independent_hits_keep_the_same_statuses() {
    let (dir, document, plate) = sample(10.0);
    let context = OperationContext::default();
    let mut kernel = MockKernel::new();
    kernel.fail_extrude_at(1);
    let mut cache = store(dir.path(), &kernel, document.meta().document_id);
    let (first, _) =
        rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("partial rebuild");
    let report = first.report();
    assert_eq!(report.failed[0].id, plate.extrude);
    first.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
    let mut cold_kernel = MockKernel::new();
    cold_kernel.fail_extrude_at(1);
    let cold = rebuild_cold(&document, &mut cold_kernel, &context).expect("cold reference");
    assert_eq!(report, cold.report());
    cold.release_all(&mut cold_kernel);
    let mut warm_kernel = MockKernel::new();
    warm_kernel.fail_extrude_at(1);
    let (warm, events) =
        rebuild_cached(&document, &mut warm_kernel, &mut cache, &context).expect("warm rebuild");
    assert_eq!(report, warm.report());
    assert!(
        events
            .iter()
            .any(|e| e.feature == plate.extrude && e.outcome == CacheOutcome::Miss)
    );
    assert!(
        !events
            .iter()
            .any(|e| e.feature == plate.extrude && e.outcome == CacheOutcome::Hit)
    );
    assert!(
        events
            .iter()
            .any(|e| e.feature == plate.second && e.outcome == CacheOutcome::Hit)
    );
    warm.release_all(&mut warm_kernel);
    assert_eq!(warm_kernel.live_shape_count(), 0);
}

#[test]
fn every_damaged_naming_record_rebuilds_atomically_without_retargeting() {
    let (dir, document, plate) = sample(10.);
    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document.meta().document_id);
    let (cold, _) = rebuild_cached(
        &document,
        &mut kernel,
        &mut cache,
        &OperationContext::default(),
    )
    .expect("prime");
    let expected = cold.topology().features[&plate.extrude]
        .bindings
        .keys()
        .copied()
        .collect::<Vec<_>>();
    cold.release_all(&mut kernel);
    let key = archive_key(&document, &kernel, plate.extrude);
    let bytes = cache
        .get(plate.extrude, key, ARCHIVE_CACHE_KIND)
        .expect("read")
        .expect("entry")
        .bytes;
    // Locate the typed entity, ancestry and incidence tables in this codec's record.
    fn count(bytes: &[u8], at: &mut usize) -> usize {
        let n = u32::from_le_bytes(bytes[*at..*at + 4].try_into().expect("count")) as usize;
        *at += 4;
        n
    }
    fn optional_id(bytes: &[u8], at: &mut usize) {
        let present = bytes[*at];
        *at += 1 + 16 * usize::from(present);
    }
    let mut at = 16;
    for _ in 0..4 {
        let n = count(&bytes, &mut at);
        at += n;
    }
    for _ in 0..count(&bytes, &mut at) {
        at += 32;
        let tag = bytes[at];
        at += 1;
        match tag {
            0 => {
                at += 32;
                optional_id(&bytes, &mut at);
            }
            1 => {
                at += 17;
                optional_id(&bytes, &mut at);
            }
            2 => {
                at += 2;
                optional_id(&bytes, &mut at);
                at += 1;
                let n = count(&bytes, &mut at);
                at += 33 * n;
            }
            _ => panic!("unexpected derivation"),
        }
    }
    let entity_count_at = at;
    let n = count(&bytes, &mut at);
    let entities = at;
    at += 37 * n;
    let ancestry_count_at = at;
    let n_ancestry = count(&bytes, &mut at);
    let ancestry_start = at;
    for _ in 0..n_ancestry {
        at += 33;
        let n = count(&bytes, &mut at);
        at += 4 * n;
    }
    let incidence_count_at = at;
    let n_incidence = count(&bytes, &mut at);
    let incidence_start = at;
    let mut slot_positions = (0..n).map(|i| entities + 37 * i).collect::<Vec<_>>();
    let mut scan = ancestry_start;
    for _ in 0..n_ancestry {
        scan += 33;
        let size = count(&bytes, &mut scan);
        for _ in 0..size {
            slot_positions.push(scan);
            scan += 4;
        }
    }
    for _ in 0..n_incidence {
        slot_positions.push(at);
        at += 4;
        let size = count(&bytes, &mut at);
        for _ in 0..size {
            slot_positions.push(at);
            at += 4;
        }
    }
    assert_eq!(at + 32, bytes.len());
    let mut cases = Vec::new();
    let mut bad = bytes.clone();
    bad[entities + 4] = 1;
    cases.push(("wrong entity kind", bad));
    let mut bad = bytes.clone();
    bad[entities + 5] ^= 1;
    cases.push(("unknown entity name", bad));
    let mut bad = bytes.clone();
    bad[entities..entities + 4].copy_from_slice(&99999u32.to_le_bytes());
    cases.push(("unknown slot", bad));
    let mut bad = bytes.clone();
    bad.drain(entities..entities + 37);
    bad[entity_count_at..entity_count_at + 4].copy_from_slice(&((n - 1) as u32).to_le_bytes());
    cases.push(("missing entity", bad));
    let mut end = ancestry_start + 33;
    let size = count(&bytes, &mut end);
    end += 4 * size;
    let mut bad = bytes.clone();
    bad.drain(ancestry_start..end);
    bad[ancestry_count_at..ancestry_count_at + 4]
        .copy_from_slice(&((n_ancestry - 1) as u32).to_le_bytes());
    cases.push(("missing ancestry", bad));
    let mut end = incidence_start + 4;
    let size = count(&bytes, &mut end);
    end += 4 * size;
    let mut bad = bytes.clone();
    bad.drain(incidence_start..end);
    bad[incidence_count_at..incidence_count_at + 4]
        .copy_from_slice(&((n_incidence - 1) as u32).to_le_bytes());
    cases.push(("missing incidence", bad));
    // Swap two face slots everywhere: the graph stays internally consistent,
    // so only the comparison against the actual restored incidence can catch it.
    let face_slots = (0..n)
        .filter(|i| bytes[entities + 37 * i + 4] == 0)
        .take(2)
        .map(|i| bytes[entities + 37 * i..entities + 37 * i + 4].to_vec())
        .collect::<Vec<_>>();
    let mut bad = bytes.clone();
    for pos in slot_positions {
        let old = &bytes[pos..pos + 4];
        if old == face_slots[0] {
            bad[pos..pos + 4].copy_from_slice(&face_slots[1]);
        } else if old == face_slots[1] {
            bad[pos..pos + 4].copy_from_slice(&face_slots[0]);
        }
    }
    cases.push(("shifted slots and incidence", bad));
    for (label, mut damaged) in cases {
        damaged.truncate(damaged.len() - 32);
        let checksum = ContentHash::of_bytes(&damaged);
        damaged.extend_from_slice(checksum.as_bytes());
        cache
            .put(plate.extrude, key, ARCHIVE_CACHE_KIND, &damaged)
            .expect("outer checksum is valid");
        let mut reader = MockKernel::new();
        let (built, events) = rebuild_cached(
            &document,
            &mut reader,
            &mut cache,
            &OperationContext::default(),
        )
        .expect("fallback");
        assert!(built.all_built(), "{label}");
        assert_eq!(reader.extrude_count(), 1, "{label}");
        assert_eq!(events[0].outcome, CacheOutcome::Rejected, "{label}");
        assert_eq!(events[1].outcome, CacheOutcome::Hit, "{label}");
        assert_eq!(
            built.topology().features[&plate.extrude]
                .bindings
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            expected,
            "{label}"
        );
        let faces = named_faces(&built, &plate);
        assert_eq!(
            faces
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            6
        );
        built.release_all(&mut reader);
        assert_eq!(reader.live_shape_count(), 0);
    }
}

#[test]
fn the_retired_role_archive_kind_is_ignored_without_migration() {
    let (dir, document, plate) = sample(10.);
    let mut kernel = MockKernel::new();
    let mut cache = store(dir.path(), &kernel, document.meta().document_id);
    let (built, _) = rebuild_cached(
        &document,
        &mut kernel,
        &mut cache,
        &OperationContext::default(),
    )
    .expect("prime");
    built.release_all(&mut kernel);
    let key = archive_key(&document, &kernel, plate.extrude);
    let bytes = cache
        .get(plate.extrude, key, ARCHIVE_CACHE_KIND)
        .expect("read")
        .expect("entry")
        .bytes;
    cache.clear().expect("clear");
    cache
        .put(plate.extrude, key, "brep.named.v1", &bytes)
        .expect("old kind");
    let mut reader = MockKernel::new();
    let (built, events) = rebuild_cached(
        &document,
        &mut reader,
        &mut cache,
        &OperationContext::default(),
    )
    .expect("rebuild");
    assert!(built.all_built());
    assert_eq!(reader.extrude_count(), 2);
    assert!(events.iter().all(|e| e.outcome == CacheOutcome::Miss));
    assert!(
        cache
            .get(plate.extrude, key, "brep.named.v1")
            .expect("read")
            .is_some()
    );
    assert!(
        cache
            .get(plate.extrude, key, ARCHIVE_CACHE_KIND)
            .expect("read")
            .is_some()
    );
    built.release_all(&mut reader);
}
