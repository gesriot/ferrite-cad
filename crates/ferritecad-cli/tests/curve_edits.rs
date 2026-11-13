// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
#[path = "../../ferritecad-fixtures/tests/support/curve_edits.rs"]
mod support;
use ferritecad_document::*;
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output};
use support::*;
fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
}
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn edit(source: &Path, destination: &Path, input: &Value) -> Output {
    let request = destination.with_extension("json");
    std::fs::write(&request, input.to_string()).expect("request");
    let document = Document::open_read_only(source).expect("document");
    let version = document.content_version().expect("version").to_string();
    let bytes = std::fs::read(source).expect("source bytes");
    let output = cli()
        .arg("edit")
        .arg(source)
        .args(["--expect-version", &version, "--request"])
        .arg(request)
        .arg("--output")
        .arg(destination)
        .arg("--json")
        .output()
        .expect("edit");
    assert_eq!(std::fs::read(source).expect("unchanged"), bytes);
    assert_eq!(
        document.content_version().expect("version").to_string(),
        version
    );
    if !output.status.success() {
        assert!(!destination.exists());
    }
    output
}
fn response(output: Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).expect("JSON response")
}
fn accepted(source: &Path, destination: &Path, changes: &[EditChange]) {
    let out = response(
        edit(
            source,
            destination,
            &ferritecad_fixtures::edit_request_json(changes),
        ),
        0,
    );
    assert_eq!(out["ok"], true, "{out}");
}
#[test]
fn cli_adds_scrambled_rectangle_then_extrudes_rebuilds_and_exports_it() -> TestResult {
    if !native() {
        return Ok(());
    }
    let mut f = Drawing::new(vec![], false);
    put(
        &mut f.doc,
        f.body,
        "Part",
        ObjectPayload::Body(Body {
            tip_feature: None,
            rollback: None,
        }),
    );
    let curves = variants(&rectangle()).remove(5);
    let added = f.root.path().join("rectangle.fcad");
    accepted(
        &f.path,
        &added,
        &curves
            .iter()
            .map(|curve| EditChange::AddCurve {
                sketch: f.sketch,
                curve: curve.clone(),
            })
            .collect::<Vec<_>>(),
    );
    let solid = f.root.path().join("extruded.fcad");
    accepted(
        &added,
        &solid,
        &[EditChange::AddFeature {
            object: f.feature,
            body: f.body,
            name: Some("Extrude".into()),
            feature: FeatureAddition::Extrude {
                profile: f.sketch,
                regions: vec![],
                reversed: false,
                operation: SolidOperation::NewBody,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(3.)?,
                },
            },
        }],
    );
    let rebuilt = cli().arg("rebuild").arg(&solid).arg("--cold").output()?;
    assert!(
        rebuilt.status.success(),
        "{}",
        String::from_utf8_lossy(&rebuilt.stdout)
    );
    let stl = f.root.path().join("body.stl");
    let exported = cli()
        .arg("export-stl")
        .arg(&solid)
        .arg("--output")
        .arg(&stl)
        .output()?;
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert!(std::fs::metadata(stl)?.len() > 84);
    Ok(())
}
#[test]
fn cli_remove_curve_preserves_the_remaining_stored_order() -> TestResult {
    if !native() {
        return Ok(());
    }
    let f = Drawing::new(rectangle(), false);
    let mut expected = f.saved();
    let removed = expected.curves.remove(1);
    let out = f.root.path().join("removed.fcad");
    accepted(
        &f.path,
        &out,
        &[EditChange::RemoveCurve {
            sketch: f.sketch,
            curve: removed.id,
        }],
    );
    assert_eq!(
        Document::open_read_only(out)?
            .object(f.sketch)?
            .expect("sketch")
            .payload,
        ObjectPayload::Sketch(expected)
    );
    Ok(())
}
#[test]
fn cli_set_construction_changes_only_the_named_flag() -> TestResult {
    if !native() {
        return Ok(());
    }
    let f = Drawing::new(rectangle(), false);
    let mut expected = f.saved();
    expected.curves[1].construction = true;
    let out = f.root.path().join("construction.fcad");
    accepted(
        &f.path,
        &out,
        &[EditChange::SetConstruction {
            sketch: f.sketch,
            curve: expected.curves[1].id,
            construction: true,
        }],
    );
    assert_eq!(
        Document::open_read_only(out)?
            .object(f.sketch)?
            .expect("sketch")
            .payload,
        ObjectPayload::Sketch(expected)
    );
    Ok(())
}
#[test]
fn cli_rebuild_calls_an_open_sketch_built_and_gives_its_complete_profile_reason() -> TestResult {
    if !native() {
        return Ok(());
    }
    let c = line([0., 0.], [10., 0.]);
    let f = Drawing::new(vec![c.clone()], false);
    let output = cli().arg("rebuild").arg(&f.path).arg("--cold").output()?;
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout)?;
    let row = text
        .lines()
        .find(|l| l.starts_with(&format!("  {}  ", f.sketch)))
        .expect("sketch line");
    assert_eq!(
        row,
        format!(
            "  {}  {:<12}  {:<16}Built; no profile: invalid input: the profile does not \
             close: curve {} has a free end at (10, 0)",
            f.sketch, "Drawing", "sketch", c.id
        )
    );
    Ok(())
}
#[test]
fn cli_curve_refusals_have_complete_text_and_leave_the_source_unchanged() -> TestResult {
    if !native() {
        return Ok(());
    }
    let f = Drawing::new(rectangle(), false);
    let duplicate = f.saved().curves[0].clone();
    let absent = ferritecad_types::StableEntityId::new();
    for (change, expected) in [
        (
            EditChange::AddCurve {
                sketch: f.sketch,
                curve: duplicate.clone(),
            },
            format!("invalid input: curve {} already exists", duplicate.id),
        ),
        (
            EditChange::RemoveCurve {
                sketch: f.sketch,
                curve: absent,
            },
            format!("invalid input: sketch has no curve {absent}"),
        ),
        (
            EditChange::SetConstruction {
                sketch: f.sketch,
                curve: absent,
                construction: true,
            },
            format!("invalid input: sketch has no curve {absent}"),
        ),
    ] {
        let out = response(
            edit(
                &f.path,
                &f.root.path().join("refused.fcad"),
                &ferritecad_fixtures::edit_request_json(&[change]),
            ),
            2,
        );
        assert_eq!(out["error"]["message"], expected);
    }
    Ok(())
}
fn unknown(mut change: Value) {
    let f = Drawing::new(vec![], false);
    change["sketch"] = json!(f.sketch);
    change["unexpected"] = json!(true);
    let out = response(
        edit(
            &f.path,
            &f.root.path().join("unknown.fcad"),
            &json!({"request_version":1,"changes":[change]}),
        ),
        2,
    );
    assert_eq!(
        out["error"]["message"],
        "invalid input: invalid edit request JSON"
    );
}
#[test]
fn add_curve_rejects_unknown_fields() {
    let f = Drawing::new(vec![], false);
    unknown(
        ferritecad_fixtures::edit_request_json(&[EditChange::AddCurve {
            sketch: f.sketch,
            curve: line([0., 0.], [1., 0.]),
        }])["changes"][0]
            .clone(),
    );
}
#[test]
fn remove_curve_rejects_unknown_fields() {
    unknown(json!({"kind":"remove_curve","curve":ferritecad_types::StableEntityId::new()}));
}
#[test]
fn set_construction_rejects_unknown_fields() {
    unknown(json!({
        "kind":"set_construction",
        "curve":ferritecad_types::StableEntityId::new(),
        "construction":true
    }));
}
