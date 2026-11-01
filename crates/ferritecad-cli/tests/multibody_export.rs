// SPDX-License-Identifier: MIT
use ferritecad_document::{Document, EditChange, KeepSelection};
use std::{path::Path, process::Command};

fn export(command: &str, document: &Path, target: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg(command)
        .arg(document)
        .arg("--output")
        .arg(target)
        .output()
        .expect("export");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn one_body_exports_every_component_once_with_its_body_identity() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let output = std::env::var_os("FCAD_MULTIBODY_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.path().join("exports"));
    std::fs::create_dir_all(&output).expect("artifacts");
    let source = root.path().join("body.fcad");
    let mut document = Document::create(&source).expect("document");
    let plate = ferritecad_fixtures::result_sets::plate(&mut document).expect("plate");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let slot =
        ferritecad_fixtures::result_sets::slot(&document, &plate, &mut kernel).expect("slot");
    for (name, count, volume) in [
        ("single", 1, 24000.),
        ("split", 2, 22400.),
        ("selected", 1, 11200.),
    ] {
        match name {
            "split" => {
                document.write_changes(&slot.changes).expect("split");
            }
            "selected" => {
                document
                    .write_changes(&[EditChange::SetKeepSelection {
                        object: slot.keep,
                        selection: KeepSelection::ContainingFaces(vec![slot.side]),
                    }])
                    .expect("select");
            }
            _ => {}
        }
        document.close().expect("close");
        export("export-fbx", &source, &output.join(format!("{name}.fbx")));
        let stl = output.join(format!("{name}.stl"));
        export("export-stl", &source, &stl);
        let bytes = std::fs::read(&stl).expect("STL");
        let triangles = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
        assert_eq!(bytes.len(), 84 + triangles * 50);
        let mut measured = 0.;
        let mut left = 0;
        let mut right = 0;
        for triangle in bytes[84..].chunks_exact(50) {
            let mut points = [[0_f64; 3]; 3];
            for (point, coordinates) in points.iter_mut().zip(triangle[12..48].chunks_exact(12)) {
                for (value, field) in point.iter_mut().zip(coordinates.chunks_exact(4)) {
                    *value = f64::from(f32::from_le_bytes(field.try_into().expect("float")));
                }
            }
            let [a, b, c] = points;
            measured += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.;
            if name != "single" {
                if points.iter().all(|p| p[0] <= 28.) {
                    left += 1;
                } else {
                    assert!(
                        points.iter().all(|p| p[0] >= 32.),
                        "triangle crosses the slot"
                    );
                    right += 1;
                }
            }
        }
        assert!((measured - volume).abs() < 1e-5, "{name}: {measured}");
        if name != "single" {
            assert!(left > 0);
            assert_eq!(right > 0, count == 2);
        }
        std::fs::write(
            output.join(format!("{name}.json")),
            serde_json::to_vec(
                &serde_json::json!({"body": plate.body, "solids": count, "volume_mm3": volume}),
            )
            .expect("metadata"),
        )
        .expect("metadata");
        document = Document::open(&source).expect("reopen");
    }
}
