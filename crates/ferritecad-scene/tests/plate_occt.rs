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

use ferritecad_document::Document;
use ferritecad_kernel::GeometryKernel;
use std::collections::{BTreeMap, BTreeSet};

#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;

fn copy_plate(path: &std::path::Path) {
    std::fs::copy(ferritecad_fixtures::plate_source(), path).expect("copies the fixture");
}

fn no_imports<K>(_: &mut K, _: &[u8]) -> Result<ferritecad_exchange::Import> {
    Err(CadError::unsupported("the plate holds no imports"))
}

#[test]
fn the_plate_is_read_from_disk_into_real_geometry() {
    if !is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
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
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
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

#[test]
fn reading_the_same_document_twice_names_the_same_corners() {
    if !is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    copy_plate(&path);

    // What each corner is called, said without a transient identity: the two
    // reads are different pictures and cannot compare pick values.
    let read = || {
        let mut kernel = OcctKernel::new().expect("opens a kernel session");
        let scene = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("the plate loads");
        let mut named: Vec<Vec<ferritecad_types::ContentHash>> = Vec::new();
        for ordinal in 0..scene.snapshot.vertex_count() {
            let vertex = scene.snapshot.vertex_of(0, ordinal).expect("numbered");
            named.push(
                scene
                    .vertices
                    .of(vertex, &scene.snapshot)
                    .iter()
                    .map(|meaning| meaning.digest)
                    .collect(),
            );
        }
        assert_eq!(kernel.live_shape_count(), 0);
        named
    };

    let first = read();
    assert_eq!(first.len(), 8);
    assert_eq!(
        first.iter().filter(|names| !names.is_empty()).count(),
        8,
        "every corner is named on the first read"
    );
    assert_eq!(read(), first, "a second read named the corners differently");
}

#[test]
fn nothing_transient_survives_into_what_a_corner_is_called() {
    if !is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    copy_plate(&path);

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let scene = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");

    // A handle or a pick value that reached this text would be one edit away
    // from being written into a document, and that is exactly the reference
    // that silently retargets after an upstream change.
    let debug = format!("{:?}", scene.vertices);
    assert!(debug.contains("VertexNames"), "{debug}");
    for forbidden in [
        "session#",
        "shape#",
        "vertex#",
        "face#",
        "edge#",
        "SubShapeHandle",
        "ShapeHandle",
        "SessionId",
        "FacePickId",
        "EdgePickId",
        "VertexPickId",
        "PickId",
    ] {
        assert!(
            !debug.contains(forbidden),
            "what a corner is called mentions {forbidden}"
        );
    }
    // And it says something: an empty structure would pass the check above.
    assert!(
        debug.contains("boundary") && debug.contains("curve-end"),
        "the corner names are absent from their own Debug: {debug}"
    );

    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_load_that_names_corners_and_then_fails_leaves_no_shape_behind() {
    if !is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    copy_plate(&path);

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    // Success first, so the session has certainly held real solids while the
    // corner names were being built.
    snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");
    assert_eq!(kernel.live_shape_count(), 0);

    // And a cancelled load of the same named document.
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

#[test]
fn curve_end_derivations_separate_the_two_corners_of_a_lens() {
    if !is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    use ferritecad_document::{
        Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression, Extrude,
        ObjectPayload, Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
    };

    // A lens: one arc and one line, so the single unordered pair of segments
    // occurs at both corners and names neither. The solid has four corners and
    // the picture numbers them; what must not happen is one of them acquiring
    // a durable name the document cannot justify.
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("lens.fcad");
    let mut document = Document::create(&path).expect("creates");

    let plane = ferritecad_types::ObjectId::new();
    let sketch = ferritecad_types::ObjectId::new();
    let extrude = ferritecad_types::ObjectId::new();
    let body = ferritecad_types::ObjectId::new();
    let arc = ferritecad_types::StableEntityId::new();
    let line = ferritecad_types::StableEntityId::new();

    document
        .write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("XY"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: ferritecad_types::Transform::IDENTITY,
                }),
            )?;
            w.put_object(
                sketch,
                None,
                1,
                Some("Lens"),
                &ObjectPayload::Sketch(Sketch {
                    plane,
                    curves: vec![
                        SketchCurve {
                            id: arc,
                            construction: false,
                            geometry: SketchGeometry::Arc {
                                center: Point2::new(0.0, 0.0)?,
                                radius: 10.0,
                                start_angle: 0.0,
                                end_angle: std::f64::consts::PI,
                            },
                        },
                        SketchCurve {
                            id: line,
                            construction: false,
                            geometry: SketchGeometry::Line {
                                start: Point2::new(-10.0, 0.0)?,
                                end: Point2::new(10.0, 0.0)?,
                            },
                        },
                    ],
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
                        distance: Expression::constant(5.0)?,
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
                Some("Lens"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(extrude),
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: body,
                dependency: extrude,
                role: DependencyRole::BodyTip,
            })?;

            Ok(())
        })
        .expect("writes the lens");
    document.close().expect("closes");

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let scene = snapshot_of(
        &path,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the lens loads");

    assert_eq!(scene.snapshot.vertex_count(), 4);
    let mut names = BTreeSet::new();
    for ordinal in 0..scene.snapshot.vertex_count() {
        let vertex = scene.snapshot.vertex_of(0, ordinal).expect("numbered");
        let meanings = scene.vertices.of(vertex, &scene.snapshot);
        assert_eq!(meanings.len(), 1);
        assert!(meanings[0].description.contains("curve-end"));
        assert!(names.insert(meanings[0].digest));
    }

    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn the_corners_of_an_imported_part_carry_no_durable_names() {
    if !is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    use ferritecad_document::{Document, StepImportRequest};
    use ferritecad_kernel::GeometryKernel;

    // An imported part has corners the kernel identifies just as a native
    // body's does. What it has no route to is a durable name: nothing in the
    // document says what a vertex of a STEP file *is*, and inventing one is
    // exactly the failure the whole naming layer exists to prevent.
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step/canonical/01-single-part.step");
    let bytes = std::fs::read(&source).expect("the corpus is committed");

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("imported.fcad");
    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let import = kernel.import_step(&bytes).expect("Open CASCADE reads it");
    let identity = kernel.identity().clone();

    let mut document = Document::create(&path).expect("creates");
    document
        .store_step_import(StepImportRequest {
            object: ferritecad_types::ObjectId::new(),
            name: Some("Part"),
            source: &bytes,
            source_name: Some("01-single-part.step"),
            import: &import,
            importer: &identity,
        })
        .expect("stores the import");
    document.close().expect("closes");

    // The shapes this test read to build the document are its own; the loader
    // below reads the stored bytes again and owns what it makes.
    if let ferritecad_exchange::Import::Imported { scene, .. } = &import {
        for shape in scene.shapes() {
            kernel.release(shape);
        }
    }
    assert_eq!(kernel.live_shape_count(), 0);

    let scene = snapshot_of(
        &path,
        &mut kernel,
        |kernel: &mut OcctKernel, bytes: &[u8]| kernel.import_step(bytes),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the imported part loads");

    assert!(
        scene.snapshot.vertex_count() > 0,
        "the imported part draws corners"
    );
    for definition in 0..scene.snapshot.meshes().len() {
        let mut ordinal = 0;
        while let Some(vertex) = scene.snapshot.vertex_of(definition, ordinal) {
            assert!(
                scene.vertices.of(vertex, &scene.snapshot).is_empty(),
                "a corner of an imported part was given a durable name it does not have"
            );
            ordinal += 1;
        }
    }

    // And a click on one of those corners falls through to the definition,
    // which is the most particular thing this document can say honestly about
    // an imported part.
    let pick = scene.snapshot.pick_of(0).expect("the import is drawn");
    let vertex = scene.snapshot.vertex_of(0, 0).expect("numbered");
    let face = (0..scene.snapshot.face_count())
        .filter_map(|ordinal| scene.snapshot.face_of(0, ordinal))
        .find(|face| scene.snapshot.vertex_touches_face(vertex, *face))
        .expect("the corner touches a face of the picture");
    let chosen = ferritecad_scene::Selection::at(
        pick,
        face,
        ferritecad_viewport::EdgePickId::NOTHING,
        vertex,
        &scene.snapshot,
        &scene.faces,
        &scene.edges,
        &scene.vertices,
    );
    assert_eq!(
        chosen,
        ferritecad_scene::Selection::Definition(pick),
        "an imported corner has no durable name and must not be chosen as one"
    );

    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn plate_primary_names_match_every_drawn_entity_and_its_incidence() {
    if !is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    let directory = tempfile::tempdir().expect("directory");
    let document = ferritecad_fixtures::open_plate(directory.path()).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&document, &mut kernel, &context).expect("rebuild");
    assert!(built.all_built());
    let scene = ferritecad_scene::snapshot_from_rebuild(
        &document,
        &built,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &context,
    )
    .expect("scene");
    let (feature, names) = built.topology().features.iter().next().expect("feature");
    let shape = names.shape().expect("shape");
    let mesh = kernel
        .tessellate(shape, &TessellationParams::default(), &context)
        .expect("mesh");
    let verify = |entity, meanings: &[ferritecad_scene::EntityMeaning]| {
        let name = names.entities[&entity];
        assert_eq!(meanings.len(), 1);
        assert_eq!(meanings[0].feature, *feature);
        assert_eq!(meanings[0].producer, *feature);
        assert_eq!(meanings[0].kind, name.kind);
        assert_eq!(meanings[0].digest, name.digest);
        assert_eq!(
            meanings[0].description,
            built
                .topology()
                .describe(*feature, name)
                .expect("full derivation")
        );
        assert_eq!(names.resolve(name).expect("one entity"), entity);
    };
    let mut faces = BTreeMap::new();
    for (ordinal, range) in mesh.faces.iter().enumerate() {
        let pick = scene.snapshot.face_of(0, ordinal).expect("face");
        verify(range.face, scene.faces.of(pick, &scene.snapshot));
        faces.insert(range.face, pick);
    }
    let mut edges = BTreeMap::new();
    for (ordinal, range) in mesh
        .edges
        .as_ref()
        .expect("edges")
        .ranges
        .iter()
        .enumerate()
    {
        let pick = scene.snapshot.edge_of(0, ordinal).expect("edge");
        verify(range.edge, scene.edges.of(pick, &scene.snapshot));
        for (face, face_pick) in &faces {
            assert_eq!(
                scene.snapshot.edge_bounds_face(pick, *face_pick),
                names.topology.adjacent_faces[&range.edge].contains(face)
            );
        }
        edges.insert(range.edge, pick);
    }
    let mut vertices = BTreeMap::new();
    for (ordinal, range) in mesh
        .topological_vertices
        .as_ref()
        .expect("vertices")
        .ranges
        .iter()
        .enumerate()
    {
        let pick = scene.snapshot.vertex_of(0, ordinal).expect("vertex");
        verify(range.vertex, scene.vertices.of(pick, &scene.snapshot));
        assert_eq!(
            scene
                .snapshot
                .occurrences_of_vertex(pick)
                .expect("occurrences")
                .len(),
            3
        );
        for (face, face_pick) in &faces {
            assert_eq!(
                scene.snapshot.vertex_touches_face(pick, *face_pick),
                names.topology.adjacent_faces[&range.vertex].contains(face)
            );
        }
        for (edge, edge_pick) in &edges {
            assert_eq!(
                scene.snapshot.vertex_ends_edge(pick, *edge_pick),
                names.topology.adjacent_faces[edge]
                    .is_subset(&names.topology.adjacent_faces[&range.vertex])
            );
        }
        vertices.insert(range.vertex, pick);
    }
    assert_eq!((faces.len(), edges.len(), vertices.len()), (6, 12, 8));
    assert_eq!(names.bindings.len(), 26);
    assert!(names.bindings.values().all(|set| set.len() == 1));

    // Independent kernel history identifies the cap boundaries and corners.
    // Each named corner must end its own sweep edge and never its neighbour's.
    let swept = naming::sweep_edges(&mut kernel, shape);
    assert_eq!(swept.len(), 4);
    let mut cap_edges = BTreeSet::new();
    let mut cap_vertices = BTreeSet::new();
    for end in [false, true] {
        let cap = naming::cap(&mut kernel, shape, end);
        let opposite = naming::cap(&mut kernel, shape, !end);
        for edge in naming::cap_edges(&mut kernel, shape, end).into_values() {
            assert!(cap_edges.insert(edge));
            assert!(scene.snapshot.edge_bounds_face(edges[&edge], faces[&cap]));
            assert!(
                !scene
                    .snapshot
                    .edge_bounds_face(edges[&edge], faces[&opposite])
            );
        }
        for (joint, vertex) in naming::cap_vertices(&mut kernel, shape, end) {
            assert!(cap_vertices.insert(vertex));
            let pick = vertices[&vertex];
            assert!(scene.snapshot.vertex_touches_face(pick, faces[&cap]));
            assert!(!scene.snapshot.vertex_touches_face(pick, faces[&opposite]));
            for (other, edge) in &swept {
                assert_eq!(
                    scene.snapshot.vertex_ends_edge(pick, edges[edge]),
                    *other == joint
                );
            }
        }
    }
    assert_eq!(cap_edges.len(), 8);
    assert_eq!(cap_vertices.len(), 8);
    assert!(swept.values().all(|edge| !cap_edges.contains(edge)));
    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}

/// A plate edge kept through a Cut is drawn in the Cut's result, but its
/// producer stays the Extrude that minted it; the pocket's rims are the Cut's.
#[test]
fn edges_kept_through_a_cut_keep_the_producer_that_minted_them() {
    if !is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool};
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("pocket.fcad");
    ferritecad_fixtures::write_plate(&path).expect("plate");
    let mut document = Document::open(&path).expect("document");
    let body = body(&document).expect("body");
    let (base, _, _) = base_profile(&document, body).expect("profile");
    let cut = circular_tool(
        &document,
        body,
        [20., 20.],
        5.,
        ferritecad_document::EndCondition::ThroughAll,
    )
    .expect("pocket");
    document.write_changes(&cut.changes).expect("pocket");
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&document, &mut kernel, &context).expect("rebuild");
    assert!(built.all_built());
    let scene = ferritecad_scene::snapshot_from_rebuild(
        &document,
        &built,
        &mut kernel,
        no_imports,
        &TessellationParams::default(),
        &context,
    )
    .expect("scene");
    let mut producers = BTreeSet::new();
    for pick in (0..).map_while(|ordinal| scene.snapshot.edge_of(0, ordinal)) {
        for meaning in scene.edges.of(pick, &scene.snapshot) {
            assert_eq!(meaning.feature, cut.feature);
            assert_eq!(
                built.topology().producer(&meaning.digest),
                Some(meaning.producer)
            );
            producers.insert(meaning.producer);
        }
    }
    assert_eq!(producers, BTreeSet::from([base, cut.feature]));
    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn changing_only_a_curve_identity_invalidates_old_picks() {
    if !is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    let directory = tempfile::tempdir().expect("directory");
    let first_path = directory.path().join("first.fcad");
    let other_path = directory.path().join("other.fcad");
    copy_plate(&first_path);
    copy_plate(&other_path);
    let mut document = Document::open(&other_path).expect("document");
    let mut record = document
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Sketch(_)))
        .expect("sketch");
    if let ferritecad_document::ObjectPayload::Sketch(sketch) = &mut record.payload {
        sketch.curves[0].id = ferritecad_types::StableEntityId::new();
    }
    document
        .write(|w| {
            w.put_object(
                record.id,
                record.parent,
                record.ordinal,
                record.name.as_deref(),
                &record.payload,
            )
        })
        .expect("new curve identity");
    document.close().expect("close");
    let mut kernel = OcctKernel::new().expect("kernel");
    let mut read = |path: &std::path::Path| {
        snapshot_of(
            path,
            &mut kernel,
            no_imports,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("scene")
    };
    let first = read(&first_path);
    let other = read(&other_path);
    assert_eq!(
        first.snapshot.meshes()[0].indices(),
        other.snapshot.meshes()[0].indices()
    );
    assert_eq!(first.snapshot.edge_count(), other.snapshot.edge_count());
    let pick = first.snapshot.edge_of(0, 0).expect("edge");
    let other_pick = other.snapshot.edge_of(0, 0).expect("other edge");
    assert_eq!(pick.to_raw(), other_pick.to_raw());
    assert_eq!(other.snapshot.definition_of_edge(pick), None);
    assert_eq!(first.snapshot.definition_of_edge(other_pick), None);
    assert!(other.edges.of(pick, &first.snapshot).is_empty());
    assert_eq!(
        other
            .snapshot
            .definition_of_face(first.snapshot.face_of(0, 0).expect("face")),
        None
    );
    assert!(
        first
            .edges
            .of(ferritecad_viewport::EdgePickId::NOTHING, &first.snapshot)
            .is_empty()
    );
    let past = ferritecad_viewport::EdgePickId::from_raw(
        u32::try_from(first.snapshot.edge_count()).expect("small") + 500,
        &first.snapshot,
    );
    assert_eq!(past, ferritecad_viewport::EdgePickId::NOTHING);
    assert!(first.edges.of(past, &first.snapshot).is_empty());
    assert_eq!(kernel.live_shape_count(), 0);
}
