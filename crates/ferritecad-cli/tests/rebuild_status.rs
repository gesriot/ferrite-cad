// SPDX-License-Identifier: MIT
use ferritecad_document::{Document, ObjectPayload};
use serde_json::{Value, json};
use std::process::{Command, Output};
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn reply(output: Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("JSON response")
}
#[test]
fn incomplete_rebuild_and_export_report_statuses_and_edit_refusals_are_structured() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let broken = root.path().join("broken.fcad");
    let chain = ferritecad_fixtures::write_constrained_fillet_chain(&broken, 25.0, 20.0, 20.0)
        .expect("fixture");
    let output = cli()
        .args(["rebuild", "--cold"])
        .arg(&broken)
        .output()
        .expect("CLI");
    assert_eq!(output.status.code(), Some(2));
    let text = String::from_utf8(output.stdout).expect("text");
    assert!(text.contains("Failed:"));
    assert!(text.contains("Skipped: depends on Fillet1"));
    let inspected = reply(
        cli()
            .arg("inspect")
            .arg(&broken)
            .arg("--json")
            .output()
            .expect("CLI"),
        0,
    );
    assert_eq!(
        inspected["result"]["rebuild"]["failed"][0]["id"],
        json!(chain.first)
    );
    let stl = root.path().join("broken.stl");
    let exported = reply(
        cli()
            .arg("export-stl")
            .arg(&broken)
            .args(["--json", "--output"])
            .arg(&stl)
            .output()
            .expect("CLI"),
        6,
    );
    assert_eq!(exported["result"]["complete"], false);
    assert_eq!(
        exported["result"]["omissions"][0]["body_id"],
        json!(chain.body)
    );
    assert!(!stl.exists());
    let healthy = root.path().join("healthy.fcad");
    let chain = ferritecad_fixtures::write_fillet_chain(&healthy, 3.0).expect("fixture");
    let document = Document::open_read_only(&healthy).expect("document");
    let version = document.content_version().expect("version");
    let objects = document.objects().expect("objects");
    let ObjectPayload::Sketch(sketch) = &objects
        .iter()
        .find(|o| o.id == chain.sketch)
        .expect("sketch")
        .payload
    else {
        unreachable!()
    };
    let curves: Vec<_> = sketch.curves.iter().map(|c| c.id).collect();
    let request = root.path().join("shrink.json");
    std::fs::write(
        &request,
        json!({ "request_version": 1, "remove": [], "add": [
        {"rule":"horizontal", "curve_id": curves[0]}, {"rule":"vertical", "curve_id":curves[1]},
        {"rule":"horizontal", "curve_id": curves[2]}, {"rule":"vertical", "curve_id":curves[3]},
        {"rule":"fixed", "curve_id":curves[0], "at":"start", "x_mm":0.0, "y_mm":0.0},
        {"rule":"distance", "curve_id":curves[0], "distance_mm":4.0},
        {"rule":"distance", "curve_id":curves[1], "distance_mm":4.0}
    ] })
        .to_string(),
    )
    .expect("request");
    let destination = root.path().join("refused.fcad");
    let refused = reply(
        cli()
            .arg("edit-sketch-constraints-copy")
            .arg(&healthy)
            .args([
                "--sketch",
                &chain.sketch.to_string(),
                "--expect-version",
                &version.to_string(),
                "--json",
                "--request",
            ])
            .arg(&request)
            .arg("--output")
            .arg(&destination)
            .output()
            .expect("CLI"),
        2,
    );
    assert_eq!(
        refused["error"]["rebuild"]["failed"][0]["id"],
        json!(chain.first)
    );
    assert!(
        refused["error"]["rebuild"]["skipped"]
            .as_array()
            .expect("skipped")
            .iter()
            .any(|s| s["id"] == json!(chain.second) && s["cause"] == json!(chain.first))
    );
    assert!(!destination.exists());
}
