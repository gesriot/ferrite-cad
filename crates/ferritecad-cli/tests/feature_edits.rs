// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_fixtures::feature_changes::{body, circular_tool, edge_addition};
use serde_json::{Value, json};
use std::{path::Path, process::Command};
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn inspect(path: &Path) -> Value {
    let out = cli()
        .args(["inspect", "--json"])
        .arg(path)
        .output()
        .expect("inspect");
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice::<Value>(&out.stdout).expect("json")["result"].clone()
}
fn run(source: &Path, destination: &Path, input: Value, code: i32) -> Value {
    let request = destination.with_extension("json");
    std::fs::write(&request, serde_json::to_vec(&input).expect("json")).expect("request");
    let d = Document::open_read_only(source).expect("source");
    let version = d.content_version().expect("version").to_string();
    drop(d);
    let out = cli()
        .arg("edit")
        .arg(source)
        .arg("--expect-version")
        .arg(version)
        .arg("--request")
        .arg(request)
        .arg("--output")
        .arg(destination)
        .arg("--json")
        .output()
        .expect("edit");
    assert_eq!(
        out.status.code(),
        Some(code),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("json result")
}
#[test]
fn new_changes_require_units_and_explicit_identities_before_kernel_work() {
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source).expect("plate");
    let d = Document::open_read_only(&source).expect("source");
    let b = body(&d).expect("body");
    let tool = circular_tool(
        &d,
        b,
        [20., 20.],
        2.,
        EndCondition::Blind {
            distance: Expression::constant(4.).expect("depth"),
        },
    )
    .expect("tool");
    let valid = ferritecad_fixtures::edit_request_json(&tool.changes);
    let before = std::fs::read(&source).expect("bytes");
    let mut cases = Vec::new();
    let mut value = valid.clone();
    value["changes"][0]["curves"][0]["geometry"]["radius"]
        .as_object_mut()
        .expect("radius")
        .remove("unit");
    cases.push(value);
    let mut value = valid.clone();
    value["changes"][1]["feature"]["end_condition"]["distance"]["unit"] = json!("deg");
    cases.push(value);
    let mut value = valid.clone();
    value["changes"][1]
        .as_object_mut()
        .expect("feature")
        .remove("object");
    cases.push(value);
    let mut value = valid;
    value["changes"][0]["object"] = json!(b);
    cases.push(value);
    for chamfer in [false, true] {
        let mut value =
            ferritecad_fixtures::edit_request_json(&[
                edge_addition(&d, b, 0, 2., chamfer).expect("edge")
            ]);
        value["changes"][0]["feature"][if chamfer { "distance" } else { "radius" }]
            .as_object_mut()
            .expect("number")
            .remove("unit");
        cases.push(value);
    }
    for (i, input) in cases.into_iter().enumerate() {
        let dest = root.path().join(format!("invalid-{i}.fcad"));
        run(&source, &dest, input, 2);
        assert!(!dest.exists());
        assert_eq!(std::fs::read(&source).expect("bytes"), before);
    }
}
#[test]
fn edit_and_inspect_cover_sketch_cut_end_fillet_and_chamfer() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source).expect("plate");
    let d = Document::open_read_only(&source).expect("source");
    let b = body(&d).expect("body");
    for kind in ["cut", "fillet", "chamfer"] {
        let changes = if kind == "cut" {
            circular_tool(&d, b, [20., 20.], 2., EndCondition::ThroughAll)
                .expect("tool")
                .changes
        } else {
            vec![edge_addition(&d, b, 0, 2., kind == "chamfer").expect("edge")]
        };
        let feature = changes.last().expect("feature").object();
        let destination = root.path().join(format!("{kind}.fcad"));
        run(
            &source,
            &destination,
            ferritecad_fixtures::edit_request_json(&changes),
            0,
        );
        let facts = inspect(&destination);
        let history = facts["bodies"][0]["history"].as_array().expect("history");
        assert_eq!(history.len(), 2);
        assert_eq!(history[1], json!(feature));
        if kind == "cut" {
            let cut = facts["features"]
                .as_array()
                .expect("features")
                .iter()
                .find(|f| f["feature_id"] == json!(feature))
                .expect("Cut");
            assert_eq!(cut["operation"], "cut");
            assert_eq!(cut["end_condition"]["kind"], "through_all");
            assert_eq!(cut["profile_sketch_id"], json!(changes[0].object()));
            let pocket = root.path().join("pocket.fcad");
            run(
                &destination,
                &pocket,
                json!({"request_version":1,"changes":[{"kind":"set_end_condition","object":feature,"end_condition":{"kind":"blind","distance":{"value":0.4,"unit":"cm"}}}]}),
                0,
            );
            let pocket_facts = inspect(&pocket);
            let cut = pocket_facts["features"]
                .as_array()
                .expect("features")
                .iter()
                .find(|f| f["feature_id"] == json!(feature))
                .expect("Cut");
            assert_eq!(
                cut["end_condition"]["distance"],
                json!({"value":4.,"unit":"mm"})
            );
        } else {
            let edge = &facts[if kind == "fillet" {
                "fillets"
            } else {
                "chamfers"
            }][0];
            assert_eq!(edge["feature_id"], json!(feature));
            assert_eq!(edge["edges"].as_array().expect("references").len(), 1);
            assert_eq!(edge["display_value"], 2.);
            assert_eq!(edge["display_unit"], "mm");
        }
        assert!(
            !facts["bodies"][0]
                .as_object()
                .expect("Body")
                .keys()
                .any(|k| k.ends_with("_edit") || k.ends_with("_edge"))
        );
    }
}

#[path = "../../ferritecad-fixtures/tests/support/region_edits.rs"]
mod region_edits;

#[test]
fn region_addition_selection_and_dependent_refusal_use_the_edit_boundary() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    for revolve in [false, true] {
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("source.fcad");
        ferritecad_fixtures::write_plate(&source).expect("plate");
        let d = Document::open_read_only(&source).expect("source");
        let f = region_edits::fixture(&d, revolve);
        let added = root.path().join("added.fcad");
        run(
            &source,
            &added,
            ferritecad_fixtures::edit_request_json(&f.changes),
            0,
        );
        let facts = inspect(&added);
        assert!(
            facts["bodies"]
                .as_array()
                .expect("bodies")
                .iter()
                .any(|b| b["body_id"] == json!(f.body))
        );
        let feature_rows = &facts[if revolve { "revolves" } else { "features" }];
        let added_feature = feature_rows
            .as_array()
            .expect("features")
            .iter()
            .find(|row| row["feature_id"] == json!(f.feature))
            .expect("added feature");
        assert_eq!(added_feature["regions"], json!([f.curves[0]]));
        let pi = std::f64::consts::PI;
        let expected = if revolve { 64. * pi * pi } else { 20. * pi };
        assert!((region_edits::volume(&added, f.feature) - expected).abs() < 1e-6);
        let resized = root.path().join("resized.fcad");
        run(
            &added,
            &resized,
            ferritecad_fixtures::edit_request_json(&f.resize_and_move()),
            0,
        );
        let expected = if revolve { 162. * pi * pi } else { 45. * pi };
        assert!((region_edits::volume(&resized, f.feature) - expected).abs() < 1e-6);
        let switched = root.path().join("switched.fcad");
        run(
            &added,
            &switched,
            ferritecad_fixtures::edit_request_json(&f.switch()),
            0,
        );
        let expected = if revolve { 144. * pi * pi } else { 20. * pi };
        assert!((region_edits::volume(&switched, f.feature) - expected).abs() < 1e-6);
        let dependent = root.path().join("dependent.fcad");
        run(
            &added,
            &dependent,
            ferritecad_fixtures::edit_request_json(&f.dependency),
            0,
        );
        let before = std::fs::read(&dependent).expect("source bytes");
        let refused = root.path().join("refused.fcad");
        let diagnostic = run(
            &dependent,
            &refused,
            ferritecad_fixtures::edit_request_json(&f.switch()),
            2,
        )
        .to_string();
        assert!(
            diagnostic.contains(&f.dependent.to_string()),
            "{diagnostic}"
        );
        assert!(!refused.exists());
        assert_eq!(std::fs::read(&dependent).expect("source bytes"), before);
    }
}

#[test]
fn rebuild_counts_the_holes_of_every_region() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source).expect("plate");
    let d = Document::open_read_only(&source).expect("source");
    let mut f = region_edits::fixture(&d, false);
    drop(d);
    // The hole belongs to the second region, which a first-region count misses.
    let EditChange::AddSketch { sketch, .. } = &mut f.changes[0] else {
        panic!("the fixture starts with its sketch");
    };
    sketch.curves.push(SketchCurve {
        id: ferritecad_types::StableEntityId::new(),
        construction: false,
        geometry: SketchGeometry::Circle {
            center: Point2::new(18., 0.).expect("center"),
            radius: 1.,
        },
    });
    let added = root.path().join("added.fcad");
    run(
        &source,
        &added,
        ferritecad_fixtures::edit_request_json(&f.changes),
        0,
    );
    let out = cli()
        .arg("rebuild")
        .arg(&added)
        .arg("--cold")
        .output()
        .expect("rebuild");
    assert!(out.status.success(), "{out:?}");
    let report = String::from_utf8_lossy(&out.stdout);
    let row = report
        .lines()
        .find(|line| line.contains(&f.sketch.to_string()))
        .expect("the sketch row");
    assert!(
        row.ends_with("3 segments, 2 regions, 1 hole(s)"),
        "{report}"
    );
}

#[test]
fn named_edge_sets_add_replace_round_trip_and_refuse_atomically() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&source).expect("plate");
    let d = Document::open_read_only(&source).expect("source");
    let b = body(&d).expect("body");
    let references: Vec<_> = (0..4)
        .map(|corner| {
            ferritecad_fixtures::feature_changes::plate_edge(&d, b, corner).expect("fixture edge")
        })
        .collect();
    let topology = cli()
        .arg("print-topology")
        .arg(&source)
        .output()
        .expect("topology");
    assert!(topology.status.success(), "{topology:?}");
    let printed = String::from_utf8(topology.stdout).expect("text");
    for reference in &references {
        assert!(printed.contains(&format!("producer={}", reference.meaning.producer)));
        assert!(printed.contains(&format!("digest={}", reference.meaning.digest)));
    }
    for chamfer in [false, true] {
        let feature = ferritecad_types::ObjectId::new();
        let added = root.path().join(format!("added-{chamfer}.fcad"));
        let amount = Expression::constant(1.).expect("amount");
        let change = EditChange::AddFeature {
            object: feature,
            body: b,
            name: None,
            feature: if chamfer {
                FeatureAddition::Chamfer {
                    edges: references[..1].to_vec(),
                    distance_mm: amount,
                }
            } else {
                FeatureAddition::Fillet {
                    edges: references[..1].to_vec(),
                    radius_mm: amount,
                }
            },
        };
        run(
            &source,
            &added,
            ferritecad_fixtures::edit_request_json(&[change]),
            0,
        );
        let replaced = root.path().join(format!("set-{chamfer}.fcad"));
        run(
            &added,
            &replaced,
            ferritecad_fixtures::edit_request_json(&[EditChange::SetEdges {
                object: feature,
                edges: references.clone(),
            }]),
            0,
        );
        let reopened = Document::open_read_only(&replaced).expect("reopen");
        let payload = reopened
            .object(feature)
            .expect("object")
            .expect("row")
            .payload;
        assert_eq!(
            ObjectPayload::from_storage_bytes(&payload.to_storage_bytes().expect("CBOR"))
                .expect("decode"),
            payload
        );
        let edges = match &payload {
            ObjectPayload::Fillet(f) => &f.edges,
            ObjectPayload::Chamfer(c) => &c.edges,
            _ => panic!("edge feature"),
        };
        assert_eq!(
            edges.iter().map(|r| r.meaning).collect::<Vec<_>>(),
            references.iter().map(|r| r.meaning).collect::<Vec<_>>()
        );
        let shown = inspect(&replaced);
        let row = &shown[if chamfer { "chamfers" } else { "fillets" }][0];
        assert_eq!(row["edges"].as_array().expect("references").len(), 4);
        let rebuild = cli()
            .arg("rebuild")
            .arg(&replaced)
            .arg("--cold")
            .output()
            .expect("rebuild");
        assert!(rebuild.status.success(), "{rebuild:?}");
        let text = String::from_utf8(rebuild.stdout).expect("text");
        assert!(
            text.contains("on 4 edge(s) from 4 edge reference(s)"),
            "{text}"
        );
        drop(reopened);
        let original = std::fs::read(&replaced).expect("source bytes");
        let mut foreign = references[0];
        foreign.meaning.producer = ferritecad_types::ObjectId::new();
        let mut unknown = references[1];
        unknown.meaning.digest = ferritecad_types::ContentHash::of_bytes(b"missing edge");
        for (index, bad, reasons) in [
            (
                0,
                vec![references[0], references[0]],
                vec!["repeats an edge meaning"],
            ),
            (1, Vec::new(), vec!["at least one reference"]),
            (
                2,
                vec![foreign, unknown],
                vec!["not in the consumed result's history", "unknown name"],
            ),
        ] {
            let refused = root.path().join(format!("refused-{chamfer}-{index}.fcad"));
            let result = run(
                &replaced,
                &refused,
                ferritecad_fixtures::edit_request_json(&[EditChange::SetEdges {
                    object: feature,
                    edges: bad,
                }]),
                2,
            );
            let message = result.to_string();
            for reason in reasons {
                assert!(message.contains(reason), "{message}");
            }
            if index == 2 {
                assert!(message.contains(&feature.to_string()));
            }
            assert!(!refused.exists());
            assert_eq!(std::fs::read(&replaced).expect("unchanged"), original);
        }
    }
}
