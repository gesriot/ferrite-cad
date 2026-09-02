// SPDX-License-Identifier: MIT
//! §22B-1a measurement: classify every imported STEP placement, and check the
//! classification against the boundary that acts on it.
//!
//! The tolerance is [`ferritecad_export::TRANSFORM_TOLERANCE`] rather than a
//! second copy of the same number. A measurement and a refusal that disagreed
//! about how far from exact is exact enough would be a corpus this project
//! believes representable and an export that refuses it.

#![allow(clippy::panic)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ferritecad_exchange::Import;
use ferritecad_export::{
    ExportGeometry, ExportProvenance, ExportSceneBuilder, ExportSource, ExportTransform,
    TRANSFORM_TOLERANCE, write_fbx_ascii_7400,
};
use ferritecad_kernel::GeometryKernel;
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::ImportedSourceId;

const EXPECTED: &str =
    "../../tools/unity-fbx-smoke/Assets/Expected/fcad-step-transform-report.json";
const TOLERANCE: f64 = TRANSFORM_TOLERANCE;

/// Whether the boundary that will hand a placement to a writer accepts it.
///
/// Asked of every placement beside the arithmetic above, so the two cannot
/// drift: a corpus this test calls representable is one an export accepts.
fn representable(placement: &[f64; 12]) -> bool {
    ExportTransform::new([
        [placement[0], placement[1], placement[2], placement[3]],
        [placement[4], placement[5], placement[6], placement[7]],
        [placement[8], placement[9], placement[10], placement[11]],
    ])
    .is_ok()
}

/// Whether the production FBX writer can express this placement, and whether
/// what it wrote rebuilds the matrix it was given.
///
/// Asked of every placement of the corpus beside the classification above,
/// through the shipped writer and its public entry point rather than through
/// an internal helper. The writer converts by conjugation and then splits the
/// result into a translation, three angles in one declared order and a uniform
/// scale; this recomputes the conversion the long way and rebuilds the matrix
/// from the three values that reached the file. A corpus this measurement
/// calls representable is one an export writes correctly, or it is neither.
fn writes_and_rebuilds(placement: &[f64; 12]) -> Result<(), String> {
    let rows = [
        [placement[0], placement[1], placement[2], placement[3]],
        [placement[4], placement[5], placement[6], placement[7]],
        [placement[8], placement[9], placement[10], placement[11]],
    ];
    let transform = ExportTransform::new(rows).map_err(|error| error.to_string())?;

    let mut builder = ExportSceneBuilder::new();
    let definition = builder
        .definition(
            ExportSource::Imported {
                source: ImportedSourceId::new(),
                definition_key: "placement".to_owned(),
            },
            Some("Placement".to_owned()),
            ExportProvenance::default(),
            ExportGeometry::Structural,
        )
        .map_err(|error| error.to_string())?;
    builder
        .node(
            None,
            definition,
            transform,
            Some("Placement".to_owned()),
            None,
        )
        .map_err(|error| error.to_string())?;
    let scene = builder.finish().map_err(|error| error.to_string())?;

    let mut bytes = Vec::new();
    write_fbx_ascii_7400(&scene, &mut bytes).map_err(|error| error.to_string())?;
    let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;

    let read = |name: &str| -> Result<[f64; 3], String> {
        let needle = format!("P: \"{name}\", \"{name}\", \"\", \"A\", ");
        let line = text
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with(&needle))
            .ok_or_else(|| format!("the writer wrote no {name}"))?;
        let mut values = [0.0; 3];
        let mut parts = line[needle.len()..].split(", ");
        for value in &mut values {
            *value = parts
                .next()
                .ok_or_else(|| format!("{name} has fewer than three values"))?
                .trim()
                .parse()
                .map_err(|_| format!("{name} is not three numbers"))?;
        }
        Ok(values)
    };
    let translation = read("Lcl Translation")?;
    let rotation = read("Lcl Rotation")?;
    let scaling = read("Lcl Scaling")?;

    // The conversion, computed here rather than taken from the writer.
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
    for axis in 0..3 {
        let expected = (0..3).map(|k| c[axis][k] * rows[k][3]).sum::<f64>() / 1000.0;
        if (translation[axis] - expected).abs() > TOLERANCE {
            return Err(format!(
                "translation {axis} is {} and not {expected}",
                translation[axis]
            ));
        }
    }
    if (scaling[0] - scaling[1]).abs() > TOLERANCE || (scaling[0] - scaling[2]).abs() > TOLERANCE {
        return Err("the writer produced a non-uniform scale".to_owned());
    }

    // `Rz * Ry * Rx`, the order the writer declares.
    let [x, y, z] = rotation.map(f64::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rebuilt = [
        [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
        [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
        [-sy, cy * sx, cy * cx],
    ];
    for row in 0..3 {
        for column in 0..3 {
            let difference = (rebuilt[row][column] * scaling[0] - linear[row][column]).abs();
            if difference > TOLERANCE * scaling[0].max(1.0) {
                return Err(format!(
                    "rebuilding element {row},{column} changes it by {difference}"
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
struct Metrics {
    transforms: usize,
    finite: usize,
    orthogonal: usize,
    uniform: usize,
    non_uniform: usize,
    reflections: usize,
    shears: usize,
    singular: usize,
    determinant_min: f64,
    determinant_max: f64,
    scale_min: f64,
    scale_max: f64,
}

impl Metrics {
    fn new() -> Self {
        Self {
            determinant_min: f64::INFINITY,
            determinant_max: f64::NEG_INFINITY,
            scale_min: f64::INFINITY,
            scale_max: f64::NEG_INFINITY,
            ..Self::default()
        }
    }

    fn observe(&mut self, placement: &[f64; 12]) {
        self.transforms += 1;
        if !placement.iter().all(|value| value.is_finite()) {
            return;
        }
        self.finite += 1;

        let columns = [
            [placement[0], placement[4], placement[8]],
            [placement[1], placement[5], placement[9]],
            [placement[2], placement[6], placement[10]],
        ];
        let scales = columns.map(|column| dot(column, column).sqrt());
        for scale in scales {
            self.scale_min = self.scale_min.min(scale);
            self.scale_max = self.scale_max.max(scale);
        }

        let determinant = placement[0]
            * (placement[5] * placement[10] - placement[6] * placement[9])
            - placement[1] * (placement[4] * placement[10] - placement[6] * placement[8])
            + placement[2] * (placement[4] * placement[9] - placement[5] * placement[8]);
        self.determinant_min = self.determinant_min.min(determinant);
        self.determinant_max = self.determinant_max.max(determinant);
        if determinant < -TOLERANCE {
            self.reflections += 1;
        }
        if determinant.abs() <= TOLERANCE {
            self.singular += 1;
        }

        let orthogonal = scales.iter().all(|scale| *scale > TOLERANCE)
            && [(0, 1), (0, 2), (1, 2)].iter().all(|&(left, right)| {
                dot(columns[left], columns[right]).abs() <= TOLERANCE * scales[left] * scales[right]
            });
        if orthogonal {
            self.orthogonal += 1;
        } else {
            self.shears += 1;
        }

        let largest = scales.into_iter().fold(0.0_f64, f64::max);
        let smallest = scales.into_iter().fold(f64::INFINITY, f64::min);
        if largest - smallest <= TOLERANCE * largest.max(1.0) {
            self.uniform += 1;
        } else {
            self.non_uniform += 1;
        }
    }

    fn append_json(&self, output: &mut String) {
        write!(
            output,
            "{{\"transforms\":{},\"finite\":{},\"determinant_min\":{},\"determinant_max\":{},\"orthogonal\":{},\"uniform_scale\":{},\"non_uniform_scale\":{},\"reflections\":{},\"shears\":{},\"singular\":{},\"scale_min\":{},\"scale_max\":{}}}",
            self.transforms,
            self.finite,
            number(self.determinant_min),
            number(self.determinant_max),
            self.orthogonal,
            self.uniform,
            self.non_uniform,
            self.reflections,
            self.shears,
            self.singular,
            number(self.scale_min),
            number(self.scale_max),
        )
        .expect("writing to a String cannot fail");
    }
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn number(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_owned();
    }
    let mut result = format!("{value:.12}");
    while result.contains('.') && result.ends_with('0') {
        result.pop();
    }
    if result.ends_with('.') {
        result.push('0');
    }
    if result == "-0.0" {
        result = "0.0".to_owned();
    }
    result
}

fn fixture_paths() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/step");
    let mut paths = Vec::new();
    for directory in ["canonical", "damaged", "interoperability"] {
        let mut found: Vec<_> = std::fs::read_dir(root.join(directory))
            .unwrap_or_else(|error| panic!("reading {directory}: {error}"))
            .map(|entry| entry.expect("reads fixture entry").path())
            .filter(|path| {
                matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("step" | "stp")
                )
            })
            .collect();
        found.sort();
        paths.extend(found);
    }
    paths
}

fn relative_name(path: &Path) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/step");
    path.strip_prefix(root)
        .expect("fixture is below the corpus root")
        .to_string_lossy()
        .replace('\\', "/")
}

fn release(kernel: &mut OcctKernel, import: &Import) {
    if let Some(scene) = import.scene() {
        let shapes: Vec<_> = scene.shapes().collect();
        for shape in shapes {
            kernel.release(shape);
        }
    }
}

#[test]
fn every_step_placement_is_unity_trs_representable() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let paths = fixture_paths();
    assert_eq!(paths.len(), 14, "the complete STEP corpus changed");
    let mut kernel = OcctKernel::new().expect("opens a real OCCT session");
    let mut output = String::from(
        "{\"schema\":\"ferritecad.step-transform-measurement.v1\",\"classification_tolerance\":1e-10,\"files\":[",
    );
    let mut total = Metrics::new();
    let mut imported_files = 0usize;

    for (file_index, path) in paths.iter().enumerate() {
        if file_index > 0 {
            output.push(',');
        }
        let name = relative_name(path);
        let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("reading {name}: {error}"));
        write!(output, "{{\"file\":\"{name}\",").expect("writes String");
        match kernel.import_step(&bytes) {
            Ok(import @ Import::Imported { .. }) => {
                imported_files += 1;
                let scene = import.scene().expect("the imported arm has a scene");
                let mut metrics = Metrics::new();
                for instance in &scene.instances {
                    metrics.observe(&instance.placement);
                    total.observe(&instance.placement);
                    assert!(
                        representable(&instance.placement),
                        "{name}: the export boundary refuses a placement this measurement calls \
                         representable"
                    );
                    if let Err(reason) = writes_and_rebuilds(&instance.placement) {
                        panic!(
                            "{name}: the FBX writer cannot express a placement this measurement \
                             calls representable: {reason}"
                        );
                    }
                }
                write!(
                    output,
                    "\"status\":\"imported\",\"source_unit\":\"{}\",\"schema_name\":\"{}\",\"definitions\":{},\"scene_nodes\":{},\"root_nodes\":{},\"metrics\":",
                    scene.source_unit,
                    scene.schema,
                    scene.definitions.len(),
                    scene.instances.len(),
                    scene.roots().count(),
                )
                .expect("writes String");
                metrics.append_json(&mut output);

                if name.ends_with("c3d-ap203-complex-assembly.stp") {
                    assert_eq!(scene.definitions.len(), 46);
                    assert_eq!(scene.instances.len(), 140);
                    assert_eq!(scene.roots().count(), 1);
                }
                assert_eq!(
                    metrics.finite, metrics.transforms,
                    "{name}: non-finite transform"
                );
                assert_eq!(metrics.orthogonal, metrics.transforms, "{name}: shear");
                assert_eq!(
                    metrics.uniform, metrics.transforms,
                    "{name}: non-uniform scale"
                );
                assert_eq!(metrics.reflections, 0, "{name}: reflection");
                assert_eq!(metrics.singular, 0, "{name}: singular transform");
                release(&mut kernel, &import);
            }
            Ok(import @ Import::Rejected { .. }) => {
                output.push_str("\"status\":\"rejected\"");
                release(&mut kernel, &import);
            }
            Err(_) => output.push_str("\"status\":\"reader_error\""),
        }
        output.push('}');
    }
    output.push_str("],\"summary\":{");
    write!(
        output,
        "\"files\":{},\"imported_files\":{},\"metrics\":",
        paths.len(),
        imported_files
    )
    .expect("writes String");
    total.append_json(&mut output);
    output.push_str("}}\n");
    assert_eq!(total.finite, total.transforms);
    assert_eq!(total.orthogonal, total.transforms);
    assert_eq!(total.uniform, total.transforms);
    assert_eq!(total.non_uniform, 0);
    assert_eq!(total.reflections, 0);
    assert_eq!(total.shears, 0);
    assert_eq!(total.singular, 0);
    assert_eq!(kernel.live_shape_count(), 0, "measurement leaked shapes");

    let expected = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(EXPECTED);
    if std::env::var_os("FCAD_RECORD_STEP_TRANSFORMS").is_some() {
        std::fs::write(&expected, &output)
            .unwrap_or_else(|error| panic!("writing {}: {error}", expected.display()));
    } else {
        let committed = std::fs::read_to_string(&expected)
            .unwrap_or_else(|error| panic!("reading {}: {error}", expected.display()));
        assert_eq!(output, committed, "STEP transform report changed");
    }
    eprintln!(
        "FCAD_STEP_TRANSFORM_MEASUREMENT transforms={}",
        total.transforms
    );
    eprintln!(
        "FCAD_FBX_TRANSFORM_MEASUREMENT written_and_rebuilt={}",
        total.transforms
    );
}
