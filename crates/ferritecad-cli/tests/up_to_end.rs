// SPDX-License-Identifier: MIT
use ferritecad_document::*;
use serde_json::Value;
use std::{path::Path, process::Command};

fn inspect(path: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg("inspect")
        .arg(path)
        .arg("--json")
        .output()
        .expect("inspect");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<Value>(&output.stdout).expect("JSON")["result"].clone()
}
fn edit(source: &Path, output: &Path, changes: &[EditChange], success: bool) -> Value {
    let request = output.with_extension("json");
    std::fs::write(
        &request,
        serde_json::to_vec(&ferritecad_fixtures::edit_request_json(changes))
            .expect("encode request"),
    )
    .expect("request");
    let before = std::fs::read(source).expect("source bytes");
    let version = inspect(source)["content_version"]
        .as_str()
        .expect("version")
        .to_owned();
    let result = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg("edit")
        .arg(source)
        .args(["--expect-version", &version])
        .arg("--request")
        .arg(request)
        .arg("--output")
        .arg(output)
        .arg("--json")
        .output()
        .expect("edit");
    assert_eq!(
        result.status.success(),
        success,
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(std::fs::read(source).expect("unchanged"), before);
    assert_eq!(output.exists(), success);
    serde_json::from_slice(&result.stdout).expect("response")
}

#[test]
fn cli_edits_both_references_offsets_and_reports_meanings_and_atomic_refusals() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let mut doc = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let f = ferritecad_fixtures::up_to_end::ceiling(&mut doc, &mut kernel).expect("ceiling");
    let mut source = doc.path().to_owned();
    doc.close().expect("close");
    for (index, (condition, label)) in [
        (f.face_end(0.).expect("face"), "up_to_face"),
        (f.vertex_end(), "up_to_vertex"),
        (
            EndCondition::Blind {
                distance: Expression::constant(20.).expect("height"),
            },
            "blind",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let output = root.path().join(format!("edit-{index}.fcad"));
        edit(
            &source,
            &output,
            &[EditChange::SetEndCondition {
                object: f.boss.feature,
                end_condition: condition,
            }],
            true,
        );
        let info = inspect(&output);
        let saved = &info["features"]
            .as_array()
            .expect("features")
            .iter()
            .find(|row| row["feature_id"] == f.boss.feature.to_string())
            .expect("boss")["end_condition"];
        assert_eq!(saved["kind"], label);
        if label != "blind" {
            assert_eq!(saved["result"], f.result.to_string());
            let field = if label == "up_to_face" {
                "face"
            } else {
                "vertex"
            };
            assert_eq!(saved[field]["producer"], f.result.to_string());
            assert!(saved[field]["id"].as_str().is_some());
            for command in ["rebuild", "inspect"] {
                let mut cmd = Command::new(env!("CARGO_BIN_EXE_ferritecad"));
                cmd.arg(command).arg(&output);
                if command == "rebuild" {
                    cmd.arg("--cold");
                }
                let text = cmd.output().expect("text output");
                assert!(
                    text.status.success(),
                    "{command}: {} {}",
                    String::from_utf8_lossy(&text.stdout),
                    String::from_utf8_lossy(&text.stderr)
                );
                assert!(String::from_utf8_lossy(&text.stdout).contains(&label.replace('_', " ")));
            }
        }
        source = output;
    }
    let offset = root.path().join("offset.fcad");
    edit(
        &source,
        &offset,
        &[
            EditChange::SetEndCondition {
                object: f.boss.feature,
                end_condition: f.face_end(0.).expect("face"),
            },
            EditChange::constant(f.boss.feature, ExpressionSlot::ExtrudeEndOffset, 2.)
                .expect("offset"),
        ],
        true,
    );
    let info = inspect(&offset);
    assert_eq!(
        info["features"]
            .as_array()
            .expect("features")
            .iter()
            .find(|r| r["feature_id"] == f.boss.feature.to_string())
            .expect("boss")["end_condition"]["offset"]["value"],
        2.
    );
    for (index, condition) in [
        EndCondition::UpToFace {
            result: f.result,
            face: EntityReference {
                meaning: ReferenceMeaning {
                    kind: EntityKind::Edge,
                    ..f.face.meaning
                },
                ..f.face
            },
            offset_mm: Expression::constant(0.).expect("zero"),
        },
        EndCondition::UpToVertex {
            result: f.result,
            vertex: EntityReference {
                meaning: ReferenceMeaning {
                    selection: Selection::AllDescendants,
                    ..f.vertex.meaning
                },
                ..f.vertex
            },
        },
    ]
    .into_iter()
    .enumerate()
    {
        let error = edit(
            &offset,
            &root.path().join(format!("refused-{index}.fcad")),
            &[EditChange::SetEndCondition {
                object: f.boss.feature,
                end_condition: condition,
            }],
            false,
        );
        assert_eq!(error["error"]["kind"], "input");
    }
}
