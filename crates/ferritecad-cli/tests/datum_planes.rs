// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_fixtures::{datum_planes::boss, edit_request_json};
use serde_json::Value;
use std::{path::Path, process::Command};

fn inspect(path: &Path) -> Value {
    let result = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .args(["inspect", "--json"])
        .arg(path)
        .output()
        .expect("inspect");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice::<Value>(&result.stdout).expect("json")["result"].clone()
}
fn edit(source: &Path, destination: &Path, changes: &[EditChange], code: i32) -> Value {
    let input = destination.with_extension("json");
    std::fs::write(
        &input,
        serde_json::to_vec(&edit_request_json(changes)).expect("request"),
    )
    .expect("write request");
    let before = std::fs::read(source).expect("bytes");
    let version = inspect(source)["content_version"]
        .as_str()
        .expect("version")
        .to_owned();
    let result = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg("edit")
        .arg(source)
        .arg("--expect-version")
        .arg(version)
        .arg("--request")
        .arg(input)
        .arg("--output")
        .arg(destination)
        .arg("--json")
        .output()
        .expect("edit");
    assert_eq!(
        result.status.code(),
        Some(code),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(std::fs::read(source).expect("source unchanged"), before);
    assert_eq!(destination.exists(), code == 0);
    serde_json::from_slice(&result.stdout).expect("response")
}

#[test]
fn cli_adds_plane_sketch_and_boss_then_reports_frames_and_atomic_reference_refusals() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&source).expect("plate");
    let doc = Document::open_read_only(&source).expect("document");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let f = boss(&doc, &mut kernel).expect("fixture");
    drop(doc);
    let added = root.path().join("boss.fcad");
    edit(&source, &added, &f.changes, 0);
    let inspection = inspect(&added);
    let plane = inspection["datum_planes"]
        .as_array()
        .expect("planes")
        .iter()
        .find(|p| p["object"].as_str() == Some(&f.plane.to_string()))
        .expect("face datum");
    assert_eq!(plane["definition"]["kind"], "face");
    assert_eq!(
        plane["definition"]["face"]["producer"],
        f.face.meaning.producer.to_string()
    );
    assert_eq!(plane["computed"]["origin"]["z"], 10.);
    assert_eq!(plane["computed"]["normal"]["z"], 1.);
    let result = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg("rebuild")
        .arg(&added)
        .arg("--cold")
        .output()
        .expect("rebuild");
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("face plane; normal X 0 Y 0 Z 1"));
    for (index, kind) in [EntityKind::Edge, EntityKind::Face].into_iter().enumerate() {
        let mut face = f.face;
        face.meaning.kind = kind;
        if kind == EntityKind::Face {
            face.meaning.digest = ferritecad_types::ContentHash::of_bytes(b"unknown face");
        }
        let error = edit(
            &added,
            &root.path().join(format!("refused-{index}.fcad")),
            &[EditChange::SetDatumFace {
                object: f.plane,
                face,
            }],
            2,
        );
        assert_eq!(
            error["error"]["kind"],
            if index == 0 { "input" } else { "topology" }
        );
    }
    let raised = root.path().join("raised.fcad");
    edit(
        &added,
        &raised,
        &[EditChange::constant(f.base, ExpressionSlot::ExtrudeDistance, 20.).expect("height")],
        0,
    );
    assert_eq!(
        inspect(&raised)["datum_planes"]
            .as_array()
            .expect("planes")
            .iter()
            .find(|p| p["object"] == f.plane.to_string())
            .expect("plane")["computed"]["origin"]["z"],
        20.
    );
}
