// SPDX-License-Identifier: MIT
//! The complex STEP assembly, from the shipped command to an `ExportScene`.
//!
//! The route is the production one and nothing about it is fabricated:
//! `import-step` publishes an `.fcad`, the external STEP is deleted, a new
//! Open CASCADE session opens the document, and the export is built from the
//! bytes the document stores. What is measured here is the assembly a writer
//! would be handed, which is not what a `RenderSnapshot` holds: the picture
//! flattens 46 definitions and 140 nodes into meshes and draws.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

use ferritecad_export::{ExportGeometry, ExportScene, ExportSource};
use ferritecad_kernel::{OperationContext, TessellationParams, TessellationRefusal};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_scene::{export_scene, snapshot_of};

const NOTICED: i32 = 4;
const REAL_GEOMETRY: &str = "step.product_definition#2428";
const OMITTED: &str = "step.product_definition#2583";
/// Two assemblies with the same multiset of children, which stay distinct.
const EQUAL_CHILDREN: [&str; 2] = [
    "step.product_definition#1764",
    "step.product_definition#2927",
];

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

fn key_of(source: &ExportSource) -> &str {
    match source {
        ExportSource::Imported { definition_key, .. } => definition_key,
        ExportSource::Body { .. } => panic!("the imported assembly exported a native body"),
    }
}

/// A comparable description of everything the export claims, so two
/// constructions can be compared without comparing float noise.
fn described(scene: &ExportScene) -> String {
    let mut out = String::new();
    for definition in scene.definitions() {
        let geometry = match &definition.geometry {
            ExportGeometry::Mesh(mesh) => format!(
                "mesh v={} t={} m={}",
                mesh.vertex_count(),
                mesh.triangle_count(),
                mesh.materials().len()
            ),
            ExportGeometry::Structural => "structural".to_owned(),
            ExportGeometry::Omitted(omission) => {
                format!("omitted {} {:?}", omission.finding.entity, omission.refusal)
            }
        };
        out.push_str(&format!(
            "definition {} {} name={:?} unit={:?} schema={:?} solids={:?} {geometry}\n",
            definition.id.index(),
            key_of(&definition.source),
            definition.display_name,
            definition.provenance.source_unit,
            definition.provenance.schema,
            definition.provenance.solids,
        ));
    }
    for node in scene.nodes() {
        out.push_str(&format!(
            "node {} parent={:?} definition={} name={:?} colour={:?} rows={:?}\n",
            node.id.index(),
            node.parent.map(|parent| parent.index()),
            node.definition.index(),
            node.display_name,
            node.colour_override,
            node.local_transform.rows(),
        ));
    }
    for report in scene.completeness().omissions() {
        out.push_str(&format!(
            "omission {} {:?} nodes={:?}\n",
            key_of(&report.source),
            report.omission.refusal,
            report
                .nodes
                .iter()
                .map(|node| node.index())
                .collect::<Vec<_>>()
        ));
    }
    out
}

fn export_of(path: &std::path::Path) -> ExportScene {
    // Import ran in another process. This session therefore holds no shape
    // from that command and can use only the bytes stored in the FCAD.
    let mut kernel = OcctKernel::new().expect("opens a fresh export kernel session");
    let scene = export_scene(
        path,
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the stored partial import reopens and exports");
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the export retained imported shapes"
    );
    scene
}

#[test]
fn the_complex_assembly_exports_its_whole_hierarchy_and_says_what_is_missing() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let input = directory.path().join("complex.stp");
    let output = directory.path().join("complex.fcad");
    let committed = fixture();
    let original = std::fs::read(&committed).expect("reads the exact fixture");
    assert_eq!(original.len(), 1_896_140, "the fixture baseline changed");
    std::fs::write(&input, &original).expect("copies the fixture byte for byte");

    let imported = Command::new(ferritecad())
        .arg("import-step")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .output()
        .expect("the shipped import-step command runs");
    let code = imported.status.code().expect("the command exits normally");
    assert_eq!(
        code,
        NOTICED,
        "partial import is neither clean success nor refusal: {}{}",
        String::from_utf8_lossy(&imported.stdout),
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(output.is_file(), "exit 4 did not publish the FCAD document");

    // From here on the external STEP does not exist. Everything the export
    // knows comes from the document.
    std::fs::remove_file(&input).expect("hides the external STEP before exporting");
    let document_before = std::fs::read(&output).expect("snapshots the FCAD bytes");

    let scene = export_of(&output);

    assert_eq!(
        scene.definitions().len(),
        46,
        "a definition of the assembly was lost or invented"
    );
    assert_eq!(
        scene.nodes().len(),
        140,
        "a hierarchy node was lost or invented"
    );
    assert_eq!(scene.roots().count(), 1, "the one root changed");
    assert_eq!(
        scene
            .nodes()
            .iter()
            .filter(|node| node.parent.is_some())
            .count(),
        139,
        "a non-root occurrence was lost or invented"
    );

    // Parents always come before their children, and every parent is a node of
    // this scene.
    for node in scene.nodes() {
        let Some(parent) = node.parent else { continue };
        assert!(
            parent.index() < node.id.index(),
            "node {} sits inside {}, which the scene lists after it",
            node.id.index(),
            parent.index()
        );
        assert!(scene.node(parent).is_some());
    }

    let mut meshes = 0usize;
    let mut structural = 0usize;
    let mut omitted = 0usize;
    let mut triangles = 0usize;
    for definition in scene.definitions() {
        match &definition.geometry {
            ExportGeometry::Mesh(mesh) => {
                meshes += 1;
                triangles += mesh.triangle_count();
                assert!(mesh.triangle_count() > 0);
                assert_eq!(mesh.normals().len(), mesh.positions().len());
                assert_eq!(mesh.triangle_materials().len(), mesh.triangle_count());
                assert!(!mesh.materials().is_empty());
            }
            ExportGeometry::Structural => structural += 1,
            ExportGeometry::Omitted(omission) => {
                omitted += 1;
                assert_eq!(omission.refusal, TessellationRefusal::IncompleteFace);
            }
        }
    }
    // Measured, not copied from the picture: the flattened snapshot has 35
    // meshes, one of which is the empty placeholder the omitted definition is
    // drawn as. The export has no such placeholder, so it has 34 real meshes,
    // one typed omission and eleven assembly frames.
    assert_eq!(
        meshes, 34,
        "the number of non-empty mesh definitions changed"
    );
    assert_eq!(omitted, 1, "the typed omission changed");
    assert_eq!(structural, 11, "the number of assembly frames changed");
    assert_eq!(meshes + omitted + structural, 46);
    assert!(triangles > 0);
    eprintln!(
        "FCAD_EXPORT_SCENE_COMPLEX definitions=46 nodes=140 meshes={meshes} structural={structural} omitted={omitted} triangles={triangles}"
    );

    // Keys are unique, source-local, and the two equal-children assemblies
    // stay two definitions.
    let keys: Vec<&str> = scene
        .definitions()
        .iter()
        .map(|definition| key_of(&definition.source))
        .collect();
    assert_eq!(
        keys.iter().collect::<BTreeSet<_>>().len(),
        46,
        "two definitions collapsed into one"
    );
    for key in EQUAL_CHILDREN {
        assert!(
            keys.contains(&key),
            "{key} is not a definition of this export"
        );
    }
    let sources: BTreeSet<_> = scene
        .definitions()
        .iter()
        .map(|definition| match &definition.source {
            ExportSource::Imported { source, .. } => *source,
            ExportSource::Body { .. } => panic!("a native body appeared"),
        })
        .collect();
    assert_eq!(sources.len(), 1, "one file is one source identity");

    // The invalid-but-tessellable definition is real geometry, and every one
    // of its placements shares that one definition.
    let real = scene
        .definitions()
        .iter()
        .find(|definition| key_of(&definition.source) == REAL_GEOMETRY)
        .expect("the assembly still holds #2428");
    let real_mesh = real
        .geometry
        .mesh()
        .expect("#2428 is real geometry and is never healed away");
    assert!(real_mesh.triangle_count() > 0);
    let real_places: Vec<_> = scene
        .nodes()
        .iter()
        .filter(|node| node.definition == real.id)
        .collect();
    assert!(!real_places.is_empty());
    for node in &real_places {
        assert_eq!(node.definition, real.id, "a placement got its own copy");
    }

    // The omitted definition keeps every placement, invents no geometry, and
    // is reported once with the persisted finding and the typed refusal.
    let missing = scene
        .definitions()
        .iter()
        .find(|definition| key_of(&definition.source) == OMITTED)
        .expect("the assembly still holds #2583");
    let omission = missing
        .geometry
        .omission()
        .expect("#2583 is a typed omission rather than an empty mesh");
    assert!(
        missing.geometry.mesh().is_none(),
        "#2583 invented triangles"
    );
    assert_eq!(omission.finding.entity, OMITTED);
    assert_eq!(omission.refusal, TessellationRefusal::IncompleteFace);
    let missing_places: Vec<_> = scene
        .nodes()
        .iter()
        .filter(|node| node.definition == missing.id)
        .map(|node| node.id)
        .collect();
    assert!(
        !missing_places.is_empty(),
        "an omitted definition lost its placements"
    );

    assert!(!scene.completeness().is_complete());
    let reports = scene.completeness().omissions();
    assert_eq!(
        reports.len(),
        1,
        "an assembly frame was reported as missing"
    );
    assert_eq!(key_of(&reports[0].source), OMITTED);
    assert_eq!(reports[0].definition, missing.id);
    assert_eq!(reports[0].nodes, missing_places);

    // Structural definitions are frames, not omissions.
    for definition in scene.definitions() {
        if definition.geometry.is_structural() {
            assert!(definition.geometry.omission().is_none());
            assert!(
                !reports
                    .iter()
                    .any(|report| report.definition == definition.id),
                "an assembly frame was reported as a missing part"
            );
        }
    }

    // Names, unit, schema and colours survived, and names merged nothing.
    let named = scene
        .definitions()
        .iter()
        .filter(|definition| definition.display_name.is_some())
        .count();
    assert!(named > 0, "every definition lost its name");
    for definition in scene.definitions() {
        assert_eq!(
            definition
                .provenance
                .source_unit
                .as_deref()
                .map(str::to_ascii_uppercase)
                .as_deref(),
            Some("MILLIMETRE"),
            "the source unit was lost"
        );
        assert!(
            definition
                .provenance
                .schema
                .as_deref()
                .is_some_and(|schema| !schema.is_empty()),
            "the source schema was lost"
        );
        assert!(definition.provenance.solids.is_some());
        assert!(
            definition.provenance.file_name.as_deref() == Some("complex.stp"),
            "the source file name was lost or turned into a path"
        );
    }
    let mut by_name: BTreeMap<&str, usize> = BTreeMap::new();
    for definition in scene.definitions() {
        if let Some(name) = definition.display_name.as_deref() {
            *by_name.entry(name).or_default() += 1;
        }
    }
    assert!(
        by_name.values().any(|count| *count > 1),
        "no two definitions of this assembly share a name, so this gate proves nothing"
    );

    // Local transforms, never accumulated: at least one node sits somewhere
    // other than its world position.
    let mut moved = 0usize;
    for node in scene.nodes() {
        assert!(
            node.local_transform
                .rows()
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        );
        if node.local_transform.translation() != [0.0, 0.0, 0.0] {
            moved += 1;
        }
    }
    assert!(moved > 0, "every placement is at the origin");

    // A repeated definition shares one definition across its placements.
    let mut places: BTreeMap<usize, usize> = BTreeMap::new();
    for node in scene.nodes() {
        *places.entry(node.definition.index()).or_default() += 1;
    }
    assert!(
        places.values().any(|count| *count > 1),
        "no definition is placed twice, so instancing proves nothing"
    );

    // Nothing about the Debug output leaks a transient identity.
    let debug = format!("{scene:?}");
    for forbidden in [
        "ShapeHandle",
        "SubShapeHandle",
        "SessionId",
        "PickId",
        "RenderSnapshot",
    ] {
        assert!(
            !debug.contains(forbidden),
            "the export's Debug output mentions {forbidden}"
        );
    }

    // Building it again gives the same scene, and the document is unchanged.
    let again = export_of(&output);
    assert_eq!(
        described(&scene),
        described(&again),
        "two constructions of one document differ"
    );
    assert_eq!(scene, again);
    assert_eq!(
        std::fs::read(&output).expect("rereads the FCAD"),
        document_before,
        "exporting changed the document"
    );
    assert_eq!(
        std::fs::read(&committed).expect("rereads the fixture"),
        original,
        "the committed fixture changed"
    );
    assert!(!input.exists(), "the external STEP came back");
}

#[test]
fn the_picture_of_the_same_document_is_the_flattened_one_it_always_was() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let input = directory.path().join("complex.stp");
    let output = directory.path().join("complex.fcad");
    std::fs::write(&input, std::fs::read(fixture()).expect("reads the fixture"))
        .expect("copies the fixture");
    let code = Command::new(ferritecad())
        .arg("import-step")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .output()
        .expect("the shipped import-step command runs")
        .status
        .code()
        .expect("the command exits normally");
    assert_eq!(code, NOTICED);
    std::fs::remove_file(&input).expect("hides the external STEP");

    let mut kernel = OcctKernel::new().expect("opens a viewer kernel session");
    let loaded = snapshot_of(
        &output,
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the picture still loads");
    assert_eq!(kernel.live_shape_count(), 0);

    // Unchanged by this slice: the picture is 35 packed meshes and 112 draws,
    // and its catalogue holds only what is drawn. That is exactly why an
    // export is not built from it.
    assert_eq!(loaded.snapshot.meshes().len(), 35);
    assert_eq!(loaded.snapshot.draws().len(), 112);
    assert_eq!(loaded.catalogue.len(), 35);
    assert_eq!(
        loaded
            .catalogue
            .iter()
            .filter(|entry| entry.geometry_omission.is_some())
            .count(),
        1
    );
}
