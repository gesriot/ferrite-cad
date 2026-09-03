// SPDX-License-Identifier: MIT
//! The complex STEP assembly, from the shipped command to production FBX.
//!
//! The route is the shipped one, end to end: `import-step` publishes an
//! `.fcad`, the external STEP is deleted, and `export-fbx` is run as a
//! command. Nothing here hands the writer a scene or a sink — the file being
//! measured is the one a person gets. What is measured is that file: its
//! hierarchy, its geometry sharing, its local transforms and what it says
//! about the one definition this build cannot give triangles to, together with
//! the exit status and the report the command produced beside it.
//!
//! The `ExportScene` built in this process is the gate's own second reading,
//! never the command's. It is what the file and the report are compared
//! against; a command that agreed with itself would prove nothing.
//!
//! The written file is a temporary artefact and is never committed. When
//! `FCAD_FBX_COMPLEX_OUT` names a path it is left there as well, so
//! [`tools/check-fbx-complex.sh`](../../../tools/check-fbx-complex.sh) can
//! hand the same bytes to pinned ufbx.
//!
//! # Why this reads the file with a scanner of its own
//!
//! The complex assembly's FBX is hundreds of megabytes, nearly all of it
//! vertex and normal arrays. This walks it a line at a time and keeps only the
//! structure, which is what the gate is about; the payload of every array is
//! read by the independent ufbx gate instead.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ferritecad_export::{ExportGeometry, ExportScene, ExportSource};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_scene::export_scene;

const NOTICED: i32 = 4;
/// A published export that is not the whole model.
const PARTIAL: i32 = 6;
const REAL_GEOMETRY: &str = "step.product_definition#2428";
const OMITTED: &str = "step.product_definition#2583";

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

/// One run of the shipped export command, exactly as a person would run it.
fn export_command(document: &Path, output: &Path) -> Output {
    Command::new(ferritecad())
        .arg("export-fbx")
        .arg(document)
        .arg("--output")
        .arg(output)
        .output()
        .expect("the shipped export-fbx command runs")
}

fn key_of(source: &ExportSource) -> &str {
    match source {
        ExportSource::Imported { definition_key, .. } => definition_key,
        ExportSource::Body { .. } => panic!("the imported assembly exported a native body"),
    }
}

fn export_of(path: &Path) -> ExportScene {
    let mut kernel = OcctKernel::new().expect("opens a fresh export kernel session");
    let scene = export_scene(
        path,
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the stored partial import reopens and exports");
    assert_eq!(kernel.live_shape_count(), 0, "the export retained shapes");
    scene
}

// --------------------------------------------------------------- the reader

#[derive(Debug, Default)]
struct WrittenGeometry {
    id: i64,
    vertices: usize,
    polygon_vertices: usize,
    polygons: usize,
}

#[derive(Debug, Default)]
struct WrittenModel {
    id: i64,
    name: String,
    class: String,
    translation: Vec<f64>,
    rotation: Vec<f64>,
    scale: Vec<f64>,
    properties: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
struct Written {
    version: i64,
    unit_scale_factor: f64,
    axes: BTreeMap<String, i64>,
    geometries: Vec<WrittenGeometry>,
    models: Vec<WrittenModel>,
    connections: Vec<(i64, i64)>,
}

impl Written {
    /// Reads only what this gate asks about, a line at a time.
    fn scan(path: &Path) -> Self {
        let file = std::fs::File::open(path).expect("the writer left a file");
        let reader = BufReader::with_capacity(1 << 20, file);
        let mut out = Self::default();
        // How many closing braces of an array payload are still to come.
        let mut skipping = 0usize;
        let mut in_geometry = false;
        let mut in_model = false;
        let mut in_settings = false;

        for line in reader.lines() {
            let line = line.expect("the file is readable");
            let trimmed = line.trim();
            if skipping > 0 {
                if trimmed == "}" {
                    skipping -= 1;
                }
                continue;
            }
            if trimmed.is_empty() || trimmed.starts_with(';') {
                continue;
            }
            let Some((name, rest)) = trimmed.split_once(':') else {
                continue;
            };
            let name = name.trim();
            let props = fields(rest);

            match name {
                "FBXVersion" => out.version = number(&props[0]) as i64,
                "GlobalSettings" => in_settings = true,
                "Geometry" => {
                    in_geometry = true;
                    in_model = false;
                    out.geometries.push(WrittenGeometry {
                        id: number(&props[0]) as i64,
                        ..WrittenGeometry::default()
                    });
                }
                "Model" => {
                    in_model = true;
                    in_geometry = false;
                    in_settings = false;
                    out.models.push(WrittenModel {
                        id: number(&props[0]) as i64,
                        name: object_name(&props[1]),
                        class: unquote(&props[2]),
                        ..WrittenModel::default()
                    });
                }
                "Material" => {
                    in_model = false;
                    in_geometry = false;
                }
                "Vertices" | "PolygonVertexIndex" | "Normals" | "Materials" => {
                    let count = declared(&props[0]);
                    if in_geometry && let Some(geometry) = out.geometries.last_mut() {
                        match name {
                            "Vertices" => geometry.vertices = count / 3,
                            "PolygonVertexIndex" => geometry.polygon_vertices = count,
                            "Materials" => geometry.polygons = count,
                            _ => {}
                        }
                    }
                    // The payload is the independent reader's business.
                    skipping = 1;
                }
                "C" => out
                    .connections
                    .push((number(&props[1]) as i64, number(&props[2]) as i64)),
                "P" => {
                    let property = unquote(&props[0]);
                    if in_settings {
                        match property.as_str() {
                            "UnitScaleFactor" => out.unit_scale_factor = number(&props[4]),
                            "UpAxis" | "UpAxisSign" | "FrontAxis" | "FrontAxisSign"
                            | "CoordAxis" | "CoordAxisSign" => {
                                out.axes.insert(property, number(&props[4]) as i64);
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if !in_model {
                        continue;
                    }
                    let Some(model) = out.models.last_mut() else {
                        continue;
                    };
                    let values =
                        || -> Vec<f64> { props[4..].iter().map(|value| number(value)).collect() };
                    match property.as_str() {
                        "Lcl Translation" => model.translation = values(),
                        "Lcl Rotation" => model.rotation = values(),
                        "Lcl Scaling" => model.scale = values(),
                        _ if unquote(&props[3]).contains('U') => {
                            model.properties.insert(property, unquote(&props[4]));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn model(&self, id: i64) -> &WrittenModel {
        self.models
            .iter()
            .find(|model| model.id == id)
            .unwrap_or_else(|| panic!("no model {id}"))
    }

    fn geometry_of(&self, model: i64) -> Option<i64> {
        let ids: BTreeSet<i64> = self.geometries.iter().map(|g| g.id).collect();
        self.connections
            .iter()
            .find(|(from, to)| *to == model && ids.contains(from))
            .map(|(from, _)| *from)
    }

    fn parent_of(&self, model: i64) -> i64 {
        self.connections
            .iter()
            .find(|(from, _)| *from == model)
            .map(|(_, to)| *to)
            .unwrap_or_else(|| panic!("model {model} has no parent connection"))
    }
}

fn fields(rest: &str) -> Vec<String> {
    // A block's opening brace is punctuation, not a property.
    let rest = rest.trim_end().trim_end_matches('{');
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in rest.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                current.push(character);
            }
            ',' if !quoted => {
                let token = current.trim().to_owned();
                current.clear();
                if !token.is_empty() {
                    out.push(token);
                }
            }
            _ => current.push(character),
        }
    }
    let token = current.trim().to_owned();
    if !token.is_empty() {
        out.push(token);
    }
    out
}

fn declared(token: &str) -> usize {
    token
        .trim_start_matches('*')
        .trim_end_matches('{')
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("not an array length: {token}"))
}

fn number(token: &str) -> f64 {
    token
        .trim_end_matches('{')
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("not a number: {token}"))
}

fn unquote(token: &str) -> String {
    token
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(token)
        .replace("&quot;", "\"")
        .replace("&cr;", "\r")
        .replace("&lf;", "\n")
}

fn object_name(token: &str) -> String {
    let full = unquote(token);
    match full.find("::") {
        Some(at) => full[at + 2..].to_owned(),
        None => full,
    }
}

// ------------------------------------------------------ the measured contract

/// `C * M * C^-1`, computed here rather than taken from the writer.
fn converted(rows: &[[f64; 4]; 3]) -> ([[f64; 3]; 3], [f64; 3]) {
    let c = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];
    let mut left = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            left[row][column] = (0..3).map(|k| c[row][k] * rows[k][column]).sum();
        }
    }
    let mut linear = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            linear[row][column] = (0..3).map(|k| left[row][k] * c[column][k]).sum();
        }
    }
    let translation = [
        (0..3).map(|k| c[0][k] * rows[k][3]).sum::<f64>() / 1000.0,
        (0..3).map(|k| c[1][k] * rows[k][3]).sum::<f64>() / 1000.0,
        (0..3).map(|k| c[2][k] * rows[k][3]).sum::<f64>() / 1000.0,
    ];
    (linear, translation)
}

/// `Rz * Ry * Rx`, the order the writer declares.
fn euler_xyz(degrees: [f64; 3]) -> [[f64; 3]; 3] {
    let [x, y, z] = degrees.map(f64::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
        [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
        [-sy, cy * sx, cy * cx],
    ]
}

// ----------------------------------------------------------------- the gate

#[test]
fn the_complex_assembly_becomes_one_fbx_that_keeps_every_definition_and_says_what_is_missing() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let input = directory.path().join("complex.stp");
    let document = directory.path().join("complex.fcad");
    let committed = fixture();
    let original = std::fs::read(&committed).expect("reads the exact fixture");
    assert_eq!(original.len(), 1_896_140, "the fixture baseline changed");
    std::fs::write(&input, &original).expect("copies the fixture byte for byte");

    let imported = Command::new(ferritecad())
        .arg("import-step")
        .arg(&input)
        .arg("--output")
        .arg(&document)
        .output()
        .expect("the shipped import-step command runs");
    assert_eq!(
        imported.status.code().expect("the command exits normally"),
        NOTICED,
        "partial import is neither clean success nor refusal: {}{}",
        String::from_utf8_lossy(&imported.stdout),
        String::from_utf8_lossy(&imported.stderr)
    );

    // From here on the external STEP does not exist.
    std::fs::remove_file(&input).expect("hides the external STEP before exporting");
    let document_before = std::fs::read(&document).expect("snapshots the FCAD bytes");

    // The production route, from the published document alone. The external
    // STEP is gone, so every triangle in the file came out of the bytes the
    // `.fcad` stores.
    let written_path = directory.path().join("complex.fbx");
    let exported = export_command(&document, &written_path);
    assert_eq!(
        exported.status.code().expect("the command exits normally"),
        PARTIAL,
        "a partial export is neither a plain success nor a refusal: {}{}",
        String::from_utf8_lossy(&exported.stdout),
        String::from_utf8_lossy(&exported.stderr)
    );
    let said = String::from_utf8_lossy(&exported.stdout).into_owned();
    let reported = String::from_utf8_lossy(&exported.stderr).into_owned();
    let published = std::fs::metadata(&written_path)
        .expect("the command published an FBX")
        .len();
    assert!(published > 0, "the command published an empty file");

    // The gate's own reading of the same document, which the file and the
    // report are measured against.
    let scene = export_of(&document);
    assert_eq!(scene.definitions().len(), 46);
    assert_eq!(scene.nodes().len(), 140);

    let written = Written::scan(&written_path);
    assert_eq!(written.version, 7400, "the file is not FBX 7400");
    assert_eq!(
        written.unit_scale_factor, 100.0,
        "the unit metadata changed"
    );
    assert_eq!(
        written.axes,
        BTreeMap::from([
            ("CoordAxis".to_owned(), 0),
            ("CoordAxisSign".to_owned(), 1),
            ("FrontAxis".to_owned(), 2),
            ("FrontAxisSign".to_owned(), 1),
            ("UpAxis".to_owned(), 1),
            ("UpAxisSign".to_owned(), 1),
        ]),
        "the axis metadata changed"
    );

    // One model per node, one root, and thirty-four geometries rather than the
    // hundred and twelve draws a flattened picture of this document has.
    assert_eq!(written.models.len(), 140, "a hierarchy node was lost");
    assert_eq!(
        written
            .connections
            .iter()
            .filter(|(_, to)| *to == 0)
            .count(),
        1,
        "the one root changed"
    );
    assert_eq!(
        written.geometries.len(),
        34,
        "a definition's geometry was copied or lost"
    );
    let triangles: usize = written.geometries.iter().map(|g| g.polygons).sum();
    assert!(triangles > 0);
    for geometry in &written.geometries {
        assert!(geometry.vertices > 0, "a geometry has no vertices");
        assert_eq!(
            geometry.polygon_vertices,
            geometry.polygons * 3,
            "a polygon is not a triangle"
        );
    }

    // Every definition of the scene is still represented, by geometry or by a
    // hierarchy node, and named by its source-local key.
    let keys: BTreeSet<&str> = written
        .models
        .iter()
        .map(|model| {
            model
                .properties
                .get("FerriteCADDefinitionKey")
                .map(String::as_str)
                .unwrap_or_else(|| panic!("model {} has no definition key", model.id))
        })
        .collect();
    assert_eq!(keys.len(), 46, "a definition stopped being represented");
    for definition in scene.definitions() {
        assert!(keys.contains(key_of(&definition.source)));
    }

    let mut structural = 0usize;
    let mut omitted = 0usize;
    for definition in scene.definitions() {
        match &definition.geometry {
            ExportGeometry::Mesh(_) => {}
            ExportGeometry::Structural => structural += 1,
            ExportGeometry::Omitted(_) => omitted += 1,
        }
    }
    assert_eq!(structural, 11, "the number of assembly frames changed");
    assert_eq!(omitted, 1, "the typed omission changed");

    // The hierarchy and every local transform are the scene's, node for node.
    for node in scene.nodes() {
        let model = written.model(2_i64.pow(33) * 2 + node.id.index() as i64);
        assert_eq!(
            model.name,
            node.display_name.clone().unwrap_or_default(),
            "the writer renamed node {}",
            node.id.index()
        );
        let expected_parent = match node.parent {
            None => 0,
            Some(parent) => 2_i64.pow(33) * 2 + parent.index() as i64,
        };
        assert_eq!(
            written.parent_of(model.id),
            expected_parent,
            "node {} lost its parent",
            node.id.index()
        );

        let (linear, translation) = converted(node.local_transform.rows());
        for (axis, (written, wanted)) in model.translation.iter().zip(translation).enumerate() {
            assert!(
                (written - wanted).abs() <= 1.0e-9,
                "node {} translation {axis}: {written} is not {wanted}",
                node.id.index()
            );
        }
        assert_eq!(model.scale.len(), 3);
        let scale = model.scale[0];
        assert!(
            (scale - model.scale[1]).abs() <= 1.0e-12 && (scale - model.scale[2]).abs() <= 1.0e-12,
            "node {} was written with a non-uniform scale",
            node.id.index()
        );
        let rebuilt = euler_xyz([model.rotation[0], model.rotation[1], model.rotation[2]]);
        for row in 0..3 {
            for column in 0..3 {
                let difference = (rebuilt[row][column] * scale - linear[row][column]).abs();
                assert!(
                    difference <= 1.0e-9,
                    "node {} element {row},{column} differs by {difference}",
                    node.id.index()
                );
            }
        }

        // A node's class says exactly what its definition holds.
        let definition = scene.definition(node.definition).expect("a definition");
        let expected_class = if definition.geometry.mesh().is_some() {
            "Mesh"
        } else {
            "Null"
        };
        assert_eq!(model.class, expected_class, "node {}", node.id.index());
    }

    // The invalid-but-tessellable definition is real geometry, and every one
    // of its placements is connected to that one geometry object.
    let real: Vec<&WrittenModel> = written
        .models
        .iter()
        .filter(|model| {
            model
                .properties
                .get("FerriteCADDefinitionKey")
                .map(String::as_str)
                == Some(REAL_GEOMETRY)
        })
        .collect();
    assert!(!real.is_empty(), "#2428 left the assembly");
    let shared: BTreeSet<Option<i64>> = real
        .iter()
        .map(|model| written.geometry_of(model.id))
        .collect();
    assert_eq!(
        shared.len(),
        1,
        "#2428's placements do not share one geometry"
    );
    let shared = shared
        .into_iter()
        .next()
        .expect("one value")
        .expect("#2428 has geometry");
    assert!(
        written
            .geometries
            .iter()
            .any(|geometry| geometry.id == shared && geometry.polygons > 0),
        "#2428 was healed away into an empty geometry"
    );
    for model in &real {
        assert!(!model.properties.contains_key("FerriteCADGeometryOmission"));
    }

    // The omitted definition keeps every placement, invents no triangles, and
    // says why in properties an importer can read.
    let missing: Vec<&WrittenModel> = written
        .models
        .iter()
        .filter(|model| {
            model
                .properties
                .get("FerriteCADDefinitionKey")
                .map(String::as_str)
                == Some(OMITTED)
        })
        .collect();
    assert!(!missing.is_empty(), "#2583 lost its placements");
    for model in &missing {
        assert_eq!(model.class, "Null");
        assert_eq!(
            written.geometry_of(model.id),
            None,
            "#2583 invented triangles"
        );
        assert_eq!(
            model
                .properties
                .get("FerriteCADGeometryOmission")
                .map(String::as_str),
            Some(OMITTED)
        );
        assert_eq!(
            model
                .properties
                .get("FerriteCADOmissionFinding")
                .map(String::as_str),
            Some(OMITTED)
        );
        assert_eq!(
            model
                .properties
                .get("FerriteCADOmissionRefusal")
                .map(String::as_str),
            Some("IncompleteFace")
        );
        assert_eq!(
            model
                .properties
                .get("FerriteCADComplete")
                .map(String::as_str),
            Some("0")
        );
    }

    // Structure is not an omission: eleven assembly frames carry no marker.
    let marked = written
        .models
        .iter()
        .filter(|model| model.properties.contains_key("FerriteCADGeometryOmission"))
        .count();
    assert_eq!(
        marked,
        missing.len(),
        "a frame was marked as a missing part"
    );

    // What the command said it wrote is what is on the disk.
    assert!(
        said.contains("140 nodes, 34 geometry objects"),
        "the summary does not describe the file it wrote:\n{said}"
    );
    assert!(
        said.contains(&format!("{published} byte(s)")),
        "the summary counts other bytes than the published file has:\n{said}"
    );

    // The report on standard error is partial, and says everything the scene's
    // own completeness holds about the definition that has no triangles: its
    // source-qualified identity, the finding the import persisted, the typed
    // refusal this build got, and every placement.
    let omissions = scene.completeness().omissions();
    assert_eq!(omissions.len(), 1, "the typed omission changed");
    let expected = &omissions[0];
    assert!(
        reported.starts_with("partial export: 1 definition could not be given triangles"),
        "the report does not open by saying what happened:\n{reported}"
    );
    assert!(
        reported.contains("omission 1 of 1"),
        "the report does not number its entries:\n{reported}"
    );
    let source_id = match &expected.source {
        ExportSource::Imported { source, .. } => source.to_string(),
        ExportSource::Body { .. } => panic!("the imported assembly reported a native body"),
    };
    assert!(
        reported.contains(&format!("imported source {source_id}  key {OMITTED}")),
        "the report drops the identity of the file the key belongs to:\n{reported}"
    );
    assert!(
        reported.contains(&format!("{}", expected.omission.finding)),
        "the report drops the finding the document persisted:\n{reported}"
    );
    assert!(
        reported.contains("refusal     IncompleteFace"),
        "the report drops the typed refusal:\n{reported}"
    );
    assert!(
        reported.contains(&format!(
            "placements  {} in the file: ",
            expected.nodes.len()
        )),
        "the report does not say how many placements are affected:\n{reported}"
    );
    assert_eq!(
        expected.nodes.len(),
        missing.len(),
        "the report and the file disagree about how many placements are affected"
    );
    for node in &expected.nodes {
        assert!(
            reported.contains(&format!("node/{}", node.index())),
            "the report lost placement node/{}:\n{reported}",
            node.index()
        );
    }
    // And nothing in it is a `Debug` rendering of a value.
    for rendering in ["Diagnostic {", "ExportSource::", "Imported {", "NodeId("] {
        assert!(
            !reported.contains(rendering),
            "the report used a Debug rendering as data ({rendering}):\n{reported}"
        );
    }

    eprintln!(
        "FCAD_EXPORT_FBX_COMPLEX definitions=46 nodes=140 geometries={} triangles={triangles} \
         omissions={} bytes={published}",
        written.geometries.len(),
        omissions.len(),
    );

    // Running the command again gives the same file and the same report, and
    // neither the document nor the committed fixture moved.
    let again_path = directory.path().join("complex-again.fbx");
    let again = export_command(&document, &again_path);
    assert_eq!(again.status.code(), Some(PARTIAL));
    assert_eq!(
        String::from_utf8_lossy(&again.stderr),
        reported,
        "two exports of one document reported different things"
    );
    assert_eq!(
        String::from_utf8_lossy(&again.stdout).replace(
            &*again_path.to_string_lossy(),
            &written_path.to_string_lossy()
        ),
        said,
        "two exports of one document summarised differently"
    );
    assert_eq!(
        std::fs::read(&written_path).expect("rereads the first"),
        std::fs::read(&again_path).expect("rereads the second"),
        "two exports of one document differ"
    );
    std::fs::remove_file(&again_path).expect("the second copy is not needed");
    assert_eq!(
        std::fs::read(&document).expect("rereads the FCAD"),
        document_before,
        "exporting changed the document"
    );
    assert_eq!(
        std::fs::read(&committed).expect("rereads the fixture"),
        original,
        "the committed fixture changed"
    );
    assert!(!input.exists(), "the external STEP came back");

    // Left for the independent reader when a gate script asked for it. Never
    // committed: this is one build's tessellation, not a fixture.
    if let Ok(destination) = std::env::var("FCAD_FBX_COMPLEX_OUT") {
        std::fs::copy(&written_path, &destination).expect("the gate directory is writable");
        eprintln!("FCAD_EXPORT_FBX_COMPLEX_ARTEFACT {destination}");
    }
}
