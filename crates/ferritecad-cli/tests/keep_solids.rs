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
fn keep_solids_cli_publishes_both_pieces_then_selects_and_refuses_atomically() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("plate.fcad");
    let mut doc = Document::create(&source).expect("document");
    let p = ferritecad_fixtures::result_sets::plate(&mut doc).expect("plate");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let slot = ferritecad_fixtures::result_sets::slot(&doc, &p, &mut kernel).expect("slot");
    doc.close().expect("close");
    let all = root.path().join("all.fcad");
    edit(&source, &all, &slot.changes, true);
    let facts = inspect(&all);
    assert_eq!(facts["keep_solids"][0]["selection"]["kind"], "all");
    let row = facts["feature_results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|r| r["feature_id"] == slot.keep.to_string())
        .expect("KeepSolids result");
    assert_eq!(row["solid_count"], 2);
    let selected = root.path().join("selected.fcad");
    edit(
        &all,
        &selected,
        &[EditChange::SetKeepSelection {
            object: slot.keep,
            selection: KeepSelection::ContainingFaces(vec![slot.side]),
        }],
        true,
    );
    let facts = inspect(&selected);
    let reference = &facts["keep_solids"][0]["selection"]["faces"][0];
    assert!(reference["id"].as_str().is_some());
    assert_eq!(reference["digest"], slot.side.meaning.digest.to_string());
    assert_eq!(reference["producer"], p.base.to_string());
    let row = facts["feature_results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|r| r["feature_id"] == slot.keep.to_string())
        .expect("selected result");
    assert_eq!(row["solid_count"], 1);
    for command in [vec!["rebuild", "--cold"], vec!["inspect"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
            .args(command)
            .arg(&selected)
            .output()
            .expect("text");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("solids containing 1 face references"));
        assert!(text.contains("1 solids"));
    }
    for (index, selection) in [Selection::One, Selection::AllDescendants]
        .into_iter()
        .enumerate()
    {
        let mut face = slot.split;
        face.meaning.selection = selection;
        let refused = edit(
            &selected,
            &root.path().join(format!("refused-{index}.fcad")),
            &[EditChange::SetKeepSelection {
                object: slot.keep,
                selection: KeepSelection::ContainingFaces(vec![face]),
            }],
            false,
        );
        assert_eq!(refused["error"]["kind"], "topology");
        let text = refused["error"]["message"].as_str().expect("diagnostic");
        assert!(text.contains(&slot.keep.to_string()));
        assert!(text.contains(if index == 0 {
            "split into 2 faces"
        } else {
            "faces in 2 solids"
        }));
    }
    for (index, selection) in [
        KeepSelection::ContainingFaces(vec![]),
        KeepSelection::ContainingFaces(vec![EntityReference::new(ReferenceMeaning {
            kind: EntityKind::Edge,
            ..slot.side.meaning
        })]),
    ]
    .into_iter()
    .enumerate()
    {
        let refused = edit(
            &selected,
            &root.path().join(format!("input-{index}.fcad")),
            &[EditChange::SetKeepSelection {
                object: slot.keep,
                selection,
            }],
            false,
        );
        assert_eq!(refused["error"]["kind"], "input");
    }
}
