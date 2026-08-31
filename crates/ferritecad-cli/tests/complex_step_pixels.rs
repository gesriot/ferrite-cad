// SPDX-License-Identifier: MIT
//! The partial complex STEP import, from the shipped command to GPU pixels.
//!
//! This is deliberately separate from `imported_step_pixels`: the canonical
//! corpus is a clean-import contract, while this fixture is a published
//! document with recoverable diagnostics and two topology-invalid solids.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use ferritecad_document::{Document, ObjectPayload};
use ferritecad_exchange::{Stage, StoredScene};
use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_scene::{LoadedScene, SceneItem, snapshot_of};
use ferritecad_types::ErrorKind;
use ferritecad_viewport::{
    Camera, EdgePickId, FacePickId, Hovered, Marked, PickId, RenderSnapshot, VertexPickId,
    Visibility,
};
use ferritecad_viewport_gpu::{Frame, Renderer};
use rusqlite::OpenFlags;

const NOTICED: i32 = 4;
const WIDTH: u32 = 256;
const HEIGHT: u32 = 256;
const INVALID_DEFINITIONS: [&str; 2] = [
    "step.product_definition#2428",
    "step.product_definition#2583",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MissingAdapter {
    Skip,
    Fail,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PixelCounts {
    model: usize,
    faces: usize,
    edges: usize,
    vertices: usize,
}

fn ferritecad() -> PathBuf {
    let mut path = std::env::current_exe().expect("the test knows where it is");
    path.pop();
    path.pop();
    path.push(format!("ferritecad{}", std::env::consts::EXE_SUFFIX));
    path
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step/interoperability/c3d-ap203-complex-assembly.stp")
}

fn missing_adapter(required: bool) -> MissingAdapter {
    if required {
        MissingAdapter::Fail
    } else {
        MissingAdapter::Skip
    }
}

fn renderer_or_skip() -> Option<Renderer> {
    match Renderer::new() {
        Ok(renderer) => Some(renderer),
        Err(reason) if reason.kind() == ErrorKind::Unsupported => {
            match missing_adapter(std::env::var("FERRITECAD_REQUIRE_GPU").as_deref() == Ok("1")) {
                MissingAdapter::Fail => panic!(
                    "FERRITECAD_REQUIRE_GPU=1 was set, so the complex pixel gate may not skip: \
                     {reason}"
                ),
                MissingAdapter::Skip => {
                    eprintln!("skipped: {reason}");
                    None
                }
            }
        }
        Err(reason) => panic!("a renderer failed after adapter discovery: {reason}"),
    }
}

fn imported_definition(entry: &ferritecad_scene::CatalogueEntry) -> &str {
    let SceneItem::Imported(reference) = &entry.item else {
        panic!("the STEP loader catalogued an imported definition as a native body");
    };
    reference.definition_key()
}

fn transform_point(matrix: &[f32; 16], point: [f32; 3]) -> [f32; 3] {
    [
        matrix[0] * point[0] + matrix[4] * point[1] + matrix[8] * point[2] + matrix[12],
        matrix[1] * point[0] + matrix[5] * point[1] + matrix[9] * point[2] + matrix[13],
        matrix[2] * point[0] + matrix[6] * point[1] + matrix[10] * point[2] + matrix[14],
    ]
}

fn measured_bounds(snapshot: &RenderSnapshot) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for draw in snapshot.draws() {
        let (mesh_min, mesh_max) = snapshot.meshes()[draw.mesh].bounds();
        for x in [mesh_min[0], mesh_max[0]] {
            for y in [mesh_min[1], mesh_max[1]] {
                for z in [mesh_min[2], mesh_max[2]] {
                    let point = transform_point(&draw.transform, [x, y, z]);
                    for axis in 0..3 {
                        min[axis] = min[axis].min(point[axis]);
                        max[axis] = max[axis].max(point[axis]);
                    }
                }
            }
        }
    }
    (min, max)
}

fn assert_same_bounds(expected: ([f32; 3], [f32; 3]), measured: ([f32; 3], [f32; 3])) {
    for axis in 0..3 {
        let scale = expected.0[axis].abs().max(expected.1[axis].abs()).max(1.0);
        let tolerance = scale * 1.0e-5;
        assert!(
            (expected.0[axis] - measured.0[axis]).abs() <= tolerance
                && (expected.1[axis] - measured.1[axis]).abs() <= tolerance,
            "snapshot bounds {expected:?} do not cover all measured placements {measured:?}"
        );
    }
}

fn clip_point(matrix: &[f32; 16], point: [f32; 3]) -> [f32; 4] {
    let mut clip = [0.0; 4];
    for row in 0..4 {
        clip[row] = matrix[row] * point[0]
            + matrix[4 + row] * point[1]
            + matrix[8 + row] * point[2]
            + matrix[12 + row];
    }
    clip
}

fn assert_inside_clip(camera: &Camera, bounds: ([f32; 3], [f32; 3])) {
    let matrix = camera.view_projection();
    for x in [bounds.0[0], bounds.1[0]] {
        for y in [bounds.0[1], bounds.1[1]] {
            for z in [bounds.0[2], bounds.1[2]] {
                let clip = clip_point(&matrix, [x, y, z]);
                assert!(clip.iter().all(|value| value.is_finite()));
                assert!(clip[3] > 0.0, "framed point is behind the eye: {clip:?}");
                // The margin is 0.01 percent of clip W. It permits accumulated
                // f32 matrix error, but not even one pixel of a 256 pixel view.
                let tolerance = clip[3].max(1.0) * 1.0e-4;
                assert!(
                    clip[0] >= -clip[3] - tolerance && clip[0] <= clip[3] + tolerance,
                    "frame-all clipped X for {clip:?}"
                );
                assert!(
                    clip[1] >= -clip[3] - tolerance && clip[1] <= clip[3] + tolerance,
                    "frame-all clipped Y for {clip:?}"
                );
                assert!(
                    clip[2] >= -tolerance && clip[2] <= clip[3] + tolerance,
                    "frame-all clipped depth for {clip:?}"
                );
            }
        }
    }
}

fn inspect_pixels(frame: &Frame) -> PixelCounts {
    let snapshot = frame.snapshot();
    let mut counts = PixelCounts::default();
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let pick = frame.pick_at(x, y);
            let definition = snapshot.definition(pick);
            if pick != PickId::NOTHING {
                assert!(definition.is_some(), "foreign definition target at {x},{y}");
            }

            let hit = frame.hit_at(x, y);
            if let Some(definition) = definition {
                counts.model += 1;
                assert_eq!(snapshot.definition(hit.definition()), Some(definition));
                assert_ne!(
                    hit.face(),
                    FacePickId::NOTHING,
                    "the face target was cleared over model pixel {x},{y}"
                );
                assert_eq!(snapshot.definition_of_face(hit.face()), Some(definition));
            } else {
                assert_eq!(hit.definition(), PickId::NOTHING);
                assert_eq!(hit.face(), FacePickId::NOTHING);
            }

            if hit.face() != FacePickId::NOTHING {
                counts.faces += 1;
                assert!(snapshot.definition_of_face(hit.face()).is_some());
            }

            let edge = frame.edge_at(x, y);
            if edge != EdgePickId::NOTHING {
                counts.edges += 1;
                assert!(
                    snapshot.definition_of_edge(edge).is_some(),
                    "foreign edge target at {x},{y}"
                );
            }
            if hit.edge() != EdgePickId::NOTHING {
                assert_eq!(
                    snapshot.definition_of_edge(hit.edge()),
                    snapshot.definition(hit.definition())
                );
            }

            let vertex = frame.vertex_at(x, y);
            if vertex != VertexPickId::NOTHING {
                counts.vertices += 1;
                assert!(
                    snapshot.definition_of_vertex(vertex).is_some(),
                    "foreign vertex target at {x},{y}"
                );
            }
            if hit.vertex() != VertexPickId::NOTHING {
                assert_eq!(
                    snapshot.definition_of_vertex(hit.vertex()),
                    snapshot.definition(hit.definition())
                );
            }
        }
    }
    counts
}

fn assert_same_frame(first: &Frame, second: &Frame) {
    assert_eq!(first.width(), second.width());
    assert_eq!(first.height(), second.height());
    assert_eq!(first.colour(), second.colour(), "colour readback changed");
    for y in 0..first.height() {
        for x in 0..first.width() {
            assert_eq!(first.pick_at(x, y), second.pick_at(x, y), "pick at {x},{y}");
            assert_eq!(first.hit_at(x, y), second.hit_at(x, y), "hit at {x},{y}");
            assert_eq!(first.edge_at(x, y), second.edge_at(x, y), "edge at {x},{y}");
            assert_eq!(
                first.vertex_at(x, y),
                second.vertex_at(x, y),
                "vertex at {x},{y}"
            );
        }
    }
}

fn picture(path: &Path) -> LoadedScene {
    // Import ran in another process. This session therefore cannot retain any
    // shape from that command and can use only the bytes stored in the FCAD.
    let mut kernel = OcctKernel::new().expect("opens a fresh viewer kernel session");
    let loaded = snapshot_of(
        path,
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the stored partial import reopens and tessellates");
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the viewer retained imported shapes after packing the snapshot"
    );
    loaded
}

#[test]
fn each_complex_leaf_definition_is_measured_without_healing() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let source = std::fs::read(fixture()).expect("reads the exact fixture");
    let mut kernel = OcctKernel::new().expect("opens a measurement kernel session");
    let imported = kernel
        .import_step(&source)
        .expect("imports the exact fixture");
    let scene = imported
        .scene()
        .expect("the fixture yields a partial scene");

    let mut structural = vec![false; scene.instances.len()];
    for instance in &scene.instances {
        if let Some(parent) = instance.parent {
            structural[parent] = true;
        }
    }
    let leaves: BTreeSet<usize> = scene
        .instances
        .iter()
        .enumerate()
        .filter(|(index, _)| !structural[*index])
        .map(|(_, instance)| instance.definition)
        .collect();

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for &index in &leaves {
        let definition = &scene.definitions[index];
        let valid = kernel
            .is_valid(definition.shape)
            .expect("asks OCCT about the original B-Rep");
        match kernel.tessellate(
            definition.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        ) {
            Ok(mesh) => {
                eprintln!(
                    "complex tessellation {}: valid {valid}, solids {}, faces {}, edges {}, \
                     vertices {}, triangles {}",
                    definition.key,
                    definition.solids,
                    mesh.faces.len(),
                    mesh.edges.as_ref().map_or(0, |edges| edges.ranges.len()),
                    mesh.topological_vertices
                        .as_ref()
                        .map_or(0, |vertices| vertices.ranges.len()),
                    mesh.indices.len() / 3
                );
                succeeded.push(definition.key.clone());
            }
            Err(reason) => {
                eprintln!(
                    "complex tessellation {}: valid {valid}, solids {}, failed: {reason}",
                    definition.key, definition.solids
                );
                failed.push((definition.key.clone(), reason.to_string()));
            }
        }
        assert_eq!(
            kernel
                .is_valid(definition.shape)
                .expect("asks OCCT about the B-Rep after tessellation"),
            valid,
            "tessellation changed the validity of {}",
            definition.key
        );
    }

    assert_eq!(
        succeeded.len() + failed.len(),
        leaves.len(),
        "a leaf definition was not measured"
    );
    assert_eq!(leaves.len(), 35);
    assert_eq!(succeeded.len(), 34);
    assert!(
        succeeded
            .iter()
            .any(|definition| definition == "step.product_definition#2428"),
        "the first invalid definition no longer tessellates without healing"
    );
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, "step.product_definition#2583");
    assert!(
        failed[0].1.contains("status 6"),
        "the measured refusal changed: {}",
        failed[0].1
    );
    eprintln!(
        "complex tessellation summary: leaf definitions {}, succeeded {}, failed {:?}",
        leaves.len(),
        succeeded.len(),
        failed
    );

    let shapes: Vec<_> = scene.shapes().collect();
    for shape in shapes {
        kernel.release(shape);
    }
    assert_eq!(kernel.live_shape_count(), 0, "measurement retained shapes");
}

#[test]
fn the_complex_partial_import_reaches_repeatable_identified_pixels() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    let Some(mut renderer) = renderer_or_skip() else {
        return;
    };

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let input = directory.path().join("complex.stp");
    let output = directory.path().join("complex.fcad");
    let committed_fixture = fixture();
    let original = std::fs::read(&committed_fixture).expect("reads the exact fixture");
    assert_eq!(original.len(), 1_896_140, "the fixture baseline changed");
    std::fs::write(&input, &original).expect("copies the fixture byte for byte");
    let modified_before = std::fs::metadata(&input)
        .expect("stats the source")
        .modified()
        .ok();

    let imported = Command::new(ferritecad())
        .arg("import-step")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .output()
        .expect("the shipped import-step command runs");
    let code = imported.status.code().expect("the command exits normally");
    let report = String::from_utf8_lossy(&imported.stdout);
    assert_eq!(
        code,
        NOTICED,
        "partial import is neither clean success nor refusal: {report}{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(output.is_file(), "exit 4 did not publish the FCAD document");
    for definition in INVALID_DEFINITIONS {
        assert!(
            report.contains(definition),
            "the command did not enumerate {definition}: {report}"
        );
    }
    assert_eq!(std::fs::read(&input).expect("rereads source"), original);
    assert_eq!(
        std::fs::metadata(&input)
            .expect("restats source")
            .modified()
            .ok(),
        modified_before,
        "import-step changed the source mtime"
    );

    {
        let document = Document::open(&output).expect("opens the partial document");
        let objects = document.objects().expect("reads the imported object");
        let stored = objects
            .iter()
            .find_map(|record| match &record.payload {
                ObjectPayload::ImportedStep(imported) => Some(imported),
                _ => None,
            })
            .expect("the FCAD contains its imported STEP object");
        let StoredScene::V2(scene) = &stored.scene else {
            panic!("the import did not persist durable definition keys");
        };

        let validation: BTreeSet<&str> = stored
            .diagnostics_at_import
            .iter()
            .filter(|diagnostic| diagnostic.stage == Stage::Validation)
            .map(|diagnostic| diagnostic.entity.as_str())
            .collect();
        assert_eq!(validation, INVALID_DEFINITIONS.into_iter().collect());
        assert_eq!(scene.definitions.len(), 46);
        let keys: BTreeSet<&str> = scene
            .definitions
            .iter()
            .map(|definition| definition.key.as_str())
            .collect();
        assert_eq!(keys.len(), 46, "a durable definition identity collided");
        for definition in INVALID_DEFINITIONS {
            assert!(
                keys.contains(definition),
                "{definition} silently disappeared"
            );
        }
        assert_eq!(scene.instances.len(), 140);
        assert_eq!(
            scene
                .instances
                .iter()
                .filter(|instance| instance.parent.is_none())
                .count(),
            1,
            "the one root was lost or duplicated"
        );
        assert_eq!(
            scene
                .instances
                .iter()
                .filter(|instance| instance.parent.is_some())
                .count(),
            139,
            "a non-root occurrence was lost or invented"
        );
    }

    let connection =
        rusqlite::Connection::open_with_flags(&output, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("opens the FCAD source store read-only");
    let stored_source: Vec<u8> = connection
        .query_row("SELECT bytes FROM imported_sources", [], |row| row.get(0))
        .expect("reads the stored STEP bytes");
    connection.close().expect("closes the source store");
    assert_eq!(
        stored_source, original,
        "the FCAD did not preserve exact STEP bytes"
    );

    std::fs::remove_file(&input).expect("hides the external STEP before viewer load");
    let document_before_render = std::fs::read(&output).expect("snapshots the FCAD bytes");
    let fixture_before_render = std::fs::read(&committed_fixture).expect("snapshots the fixture");

    let loaded = picture(&output);
    let catalogue = loaded.catalogue;
    let snapshot = Arc::new(loaded.snapshot);
    assert_eq!(
        catalogue.len(),
        snapshot.meshes().len(),
        "catalogued geometry and snapshot meshes diverged"
    );
    assert!(
        !snapshot.meshes().is_empty(),
        "the real loader made no meshes"
    );
    assert!(
        !snapshot.draws().is_empty(),
        "the real loader made no draws"
    );
    let triangles: usize = snapshot
        .meshes()
        .iter()
        .map(|mesh| mesh.triangle_count())
        .sum();
    assert!(triangles > 0, "the real loader made no triangles");
    for draw in snapshot.draws() {
        assert!(
            draw.transform.iter().all(|value| value.is_finite()),
            "a draw transform is not finite: {:?}",
            draw.transform
        );
    }

    let invalid_mesh = |definition: &str| {
        catalogue
            .iter()
            .position(|entry| imported_definition(entry) == definition)
            .unwrap_or_else(|| panic!("{definition} has no catalogue entry or typed omission"))
    };
    let first_invalid = invalid_mesh("step.product_definition#2428");
    let first_draws = snapshot
        .draws()
        .iter()
        .filter(|draw| draw.mesh == first_invalid)
        .count();
    let first_triangles = snapshot.meshes()[first_invalid].triangle_count();
    assert!(first_triangles > 0, "#2428 lost its renderable mesh");
    assert!(first_draws > 0, "#2428 lost its placed draw");
    assert_eq!(catalogue[first_invalid].geometry_omission, None);

    let refused_invalid = invalid_mesh("step.product_definition#2583");
    let refused_draws = snapshot
        .draws()
        .iter()
        .filter(|draw| draw.mesh == refused_invalid)
        .count();
    assert_eq!(
        snapshot.meshes()[refused_invalid].triangle_count(),
        0,
        "#2583 unexpectedly gained geometry without an explicit healing policy"
    );
    assert!(refused_draws > 0, "#2583 lost its placed occurrence");
    let omission = catalogue[refused_invalid]
        .geometry_omission
        .as_ref()
        .expect("#2583 has no typed reason for its empty mesh");
    assert_eq!(omission.diagnostic.stage, Stage::Validation);
    assert_eq!(omission.diagnostic.entity, "step.product_definition#2583");
    assert!(
        omission.reason.contains("status 6"),
        "the current tessellation reason was lost: {}",
        omission.reason
    );
    assert_eq!(
        catalogue
            .iter()
            .filter(|entry| entry.geometry_omission.is_some())
            .count(),
        1,
        "a definition other than #2583 was silently omitted"
    );
    eprintln!(
        "complex invalid definitions: #2428 mesh {first_invalid}, draws {first_draws}, triangles \
         {first_triangles}; #2583 empty mesh {refused_invalid}, draws {refused_draws}, reason {}",
        omission.reason
    );

    let bounds = snapshot.bounds().expect("the model has finite bounds");
    assert!(
        bounds
            .0
            .iter()
            .chain(bounds.1.iter())
            .all(|value| value.is_finite()),
        "model bounds are not finite: {bounds:?}"
    );
    assert!(
        (0..3).any(|axis| bounds.1[axis] > bounds.0[axis]),
        "model bounds have no extent: {bounds:?}"
    );
    let independently_measured = measured_bounds(&snapshot);
    assert_same_bounds(bounds, independently_measured);

    let mut camera = Camera::new();
    camera.resize(WIDTH, HEIGHT);
    camera
        .frame(bounds)
        .expect("frame-all accepts the model bounds");
    assert_inside_clip(&camera, independently_measured);

    let uploads_before = renderer.geometry_uploads();
    let prepared = renderer
        .prepare(Arc::clone(&snapshot))
        .expect("uploads the real complex snapshot");
    let uploads_after_prepare = renderer.geometry_uploads();
    assert_eq!(
        uploads_after_prepare - uploads_before,
        snapshot.meshes().len() as u64,
        "GPU preparation did not upload every real mesh exactly once"
    );
    let first = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("renders the complex model offscreen");
    assert_eq!(
        renderer.geometry_uploads(),
        uploads_after_prepare,
        "render uploaded geometry after preparation"
    );
    let pixels = inspect_pixels(&first);
    let substantial = (first.width() * first.height()) as usize / 100;
    assert!(
        pixels.model > substantial,
        "only {} of {} pixels carry model identity; frame-all did not produce a substantial \
         model image",
        pixels.model,
        first.width() * first.height()
    );
    assert!(pixels.faces > 0, "no model pixel carried a face identity");
    assert!(pixels.edges > 0, "the edge target never named a real edge");
    assert!(
        pixels.vertices > 0,
        "the vertex target never named a real vertex"
    );

    let second = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("renders the same prepared snapshot again");
    assert_same_frame(&first, &second);
    assert_eq!(
        renderer.geometry_uploads(),
        uploads_after_prepare,
        "the repeated render uploaded geometry again"
    );
    assert_eq!(inspect_pixels(&second), pixels);

    assert_eq!(
        std::fs::read(&output).expect("rereads the FCAD"),
        document_before_render,
        "viewer loading or rendering changed the document"
    );
    assert_eq!(
        std::fs::read(&committed_fixture).expect("rereads the fixture"),
        fixture_before_render,
        "viewer loading or rendering changed the committed fixture"
    );
    assert!(
        !input.exists(),
        "the viewer recreated or required the external STEP"
    );

    eprintln!(
        "complex pixel measurement: definitions 46, roots 1, non-root occurrences 139, meshes \
         {}, draws {}, triangles {}, model pixels {}, face pixels {}, edge targets {}, vertex \
         targets {}, bounds {:?}",
        snapshot.meshes().len(),
        snapshot.draws().len(),
        triangles,
        pixels.model,
        pixels.faces,
        pixels.edges,
        pixels.vertices,
        bounds
    );
}

#[test]
fn the_required_gpu_run_cannot_turn_a_missing_adapter_into_a_green_skip() {
    assert_eq!(missing_adapter(false), MissingAdapter::Skip);
    assert_eq!(missing_adapter(true), MissingAdapter::Fail);
}
