// SPDX-License-Identifier: MIT
//! Wire-contract gates invoke the public process, not its serializer in isolation.
#![allow(clippy::panic)]

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output};

use ferritecad_document::{
    Dependency, DependencyRole, Document, EndCondition, Envelope, Expression, ExtrudeEditSource,
    ObjectPayload, Parameter, SketchGeometry,
};
use ferritecad_jobs::{CreateDocumentRequest, NewDocument, PlateSize, create_document};
use ferritecad_kernel::OperationContext;
use ferritecad_types::{Dimension, ObjectId, Unit};
use serde_json::Value;

#[path = "json_v1/stl.rs"]
mod stl;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}

#[path = "support/pipe.rs"]
mod pipe;
use pipe::closed_pipe;

fn run(command: &mut Command) -> Output {
    command.output().expect("real CLI process")
}

fn success(command: &mut Command) -> Output {
    let output = run(command);
    assert!(output.status.success(), "{output:?}");
    output
}

fn reply(output: &Output, operation: &str, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert_eq!(
        output.stdout.iter().filter(|b| **b == b'\n').count(),
        1,
        "one object and newline, including escaped names: {output:?}"
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("standard JSON parser");
    assert!(value.is_object());
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["operation"], operation);
    assert_eq!(value["ok"], exit == 0);
    if exit == 0 {
        assert!(value["result"].is_object());
        assert!(value.get("error").is_none());
        assert!(output.stderr.is_empty(), "{output:?}");
    } else {
        assert!(value.get("result").is_none());
        assert!(value["error"]["kind"].is_string());
        assert!(
            !value["error"]["message"]
                .as_str()
                .expect("message")
                .is_empty()
        );
        assert!(value["error"]["causes"].is_array());
        assert!(!output.stderr.is_empty());
    }
    value
}

fn inspect(path: &Path) -> Value {
    let value = reply(
        &run(cli().arg("inspect").arg(path).arg("--json")),
        "inspect",
        0,
    )["result"]
        .clone();
    // Indexing a missing serde_json key also returns Null. Check presence
    // explicitly so omission cannot accidentally satisfy the v1 null policy.
    for field in ["available", "refusal", "document_refusal"] {
        assert!(value["edit_extrude"].get(field).is_some(), "{field}");
    }
    for feature in value["features"].as_array().expect("catalog") {
        for field in ["feature_id", "name", "distance_mm", "editable", "refusal"] {
            assert!(feature.get(field).is_some(), "{field}");
        }
    }
    for body in value["bodies"].as_array().expect("required Body catalog") {
        assert!(body["body_id"].is_string());
        assert!(body.get("name").is_some(), "explicit null for unnamed Body");
    }
    value
}

fn create(path: &Path, size: Option<[&str; 3]>) {
    let mut command = cli();
    command.arg("create").arg(path);
    if let Some(size) = size {
        command
            .args(["--sample", "--size"])
            .args(size)
            .args(["--length-unit", "in"]);
    }
    success(&mut command);
}

fn create_json(path: &Path, size: Option<[&str; 3]>) -> Command {
    let mut command = cli();
    command.arg("create").arg(path).arg("--json");
    if let Some(size) = size {
        command
            .args(["--sample", "--size"])
            .args(size)
            .args(["--length-unit", "in"]);
    }
    command
}

fn plate_facts(path: &Path) -> (Vec<(f64, f64)>, f64, String, String) {
    let document = Document::open_read_only(path).expect("open");
    let length = document.meta().display_length_unit.symbol().to_owned();
    let angle = document.meta().display_angle_unit.symbol().to_owned();
    let objects = document.objects().expect("objects");
    let corners = objects
        .iter()
        .find_map(|object| match &object.payload {
            ObjectPayload::Sketch(sketch) => Some(
                sketch
                    .curves
                    .iter()
                    .map(|curve| match curve.geometry {
                        SketchGeometry::Line { start, .. } => (start.x, start.y),
                        ref other => panic!("the profile holds {other:?}"),
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("profile");
    let height = objects
        .iter()
        .find_map(|object| match &object.payload {
            ObjectPayload::Extrude(extrude) => match &extrude.end_condition {
                EndCondition::Blind { distance } => Some(distance.value()),
                _ => None,
            },
            _ => None,
        })
        .expect("height");
    document.close().expect("close");
    (corners, height, length, angle)
}

fn selected(catalog: &Value, name: &str) -> String {
    // Deliberately explicit, including in the one-feature sample. Names may be
    // duplicated, so the recipe refuses ambiguity before choosing a UUID.
    let candidates: Vec<_> = catalog["features"]
        .as_array()
        .expect("catalog")
        .iter()
        .filter(|feature| feature["name"] == name && feature["editable"] == true)
        .collect();
    assert_eq!(candidates.len(), 1, "choose one UUID explicitly: {catalog}");
    candidates[0]["feature_id"]
        .as_str()
        .expect("UUID")
        .to_owned()
}

fn edit(source: &Path, feature: &str, distance: &str, version: &str, output: &Path) -> Command {
    let mut command = cli();
    command
        .arg("edit-extrude")
        .arg(source)
        .args([
            "--json",
            "--feature",
            feature,
            "--distance-mm",
            distance,
            "--expect-version",
            version,
            "-o",
        ])
        .arg(output);
    command
}

fn entries(root: &Path) -> Vec<OsString> {
    let mut names: Vec<_> = std::fs::read_dir(root)
        .expect("directory")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    names.sort();
    names
}

#[test]
fn empty_inspect_is_a_read_with_an_unavailable_edit_and_no_sidecars() {
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join("Пустой документ.fcad");
    create(&source, None);
    let before = std::fs::read(&source).expect("before");
    let catalog = inspect(&source);
    assert_eq!(catalog["features"], serde_json::json!([]));
    assert_eq!(catalog["bodies"], serde_json::json!([]));
    assert_eq!(catalog["distance_unit"], "mm");
    assert_eq!(
        catalog["display_units"],
        serde_json::json!({"length":"mm", "angle":"deg"})
    );
    assert_eq!(catalog["edit_extrude"]["available"], false);
    assert!(catalog["edit_extrude"]["refusal"].is_string());
    assert!(catalog["edit_extrude"]["document_refusal"].is_null());
    let document = Document::open_read_only(&source).expect("reading");
    assert_eq!(
        catalog["document_id"],
        document.meta().document_id.to_string()
    );
    assert_eq!(
        catalog["content_version"],
        document.content_version().expect("version").to_string()
    );
    document.close().expect("close");
    assert_eq!(before, std::fs::read(&source).expect("after"));
    assert_eq!(
        entries(root.path()),
        vec![source.file_name().expect("name")]
    );
}

#[test]
fn json_create_empty_and_sample_match_inspect_and_the_shared_operation() {
    let root = tempfile::tempdir().expect("dir");
    let empty = root.path().join("Пустой документ.fcad");
    let created = reply(&run(&mut create_json(&empty, None)), "create", 0)["result"].clone();
    assert_eq!(created["destination"], serde_json::json!(empty));
    let reading = inspect(&empty);
    assert_eq!(created["document_id"], reading["document_id"]);
    assert_eq!(reading["features"], serde_json::json!([]));
    assert_eq!(
        reading["display_units"],
        serde_json::json!({"length":"mm","angle":"deg"})
    );
    let document = Document::open_read_only(&empty).expect("empty");
    assert!(document.objects().expect("objects").is_empty());
    assert_eq!(
        created["document_id"],
        document.meta().document_id.to_string()
    );
    document.close().expect("close");

    // Quotes exercise JSON escaping on Unix, but cannot name a Windows file.
    let sample = root.path().join(if cfg!(windows) {
        "Плита А.fcad"
    } else {
        "Плита \"А\".fcad"
    });
    let mut command = create_json(&sample, Some(["80", "50", "12"]));
    command.args(["--angle-unit", "rad"]);
    let created = reply(&run(&mut command), "create", 0)["result"].clone();
    assert_eq!(created["destination"], serde_json::json!(sample));
    let reading = inspect(&sample);
    assert_eq!(created["document_id"], reading["document_id"]);
    assert_eq!(reading["display_units"]["length"], "in");
    assert_eq!(reading["display_units"]["angle"], "rad");
    assert_eq!(reading["features"][0]["distance_mm"], 12.0);
    assert_eq!(selected(&reading, "Extrude1").len(), 36);

    let jobs = root.path().join("jobs plate.fcad");
    create_document(
        CreateDocumentRequest::new(
            &jobs,
            NewDocument::SamplePlate(PlateSize {
                width: 80.0,
                depth: 50.0,
                height: 12.0,
            }),
            "keep",
        )
        .displaying(Unit::Inch, Unit::Radian),
        &OperationContext::default(),
    )
    .expect("shared create");
    assert_eq!(plate_facts(&sample), plate_facts(&jobs));
    let other = root.path().join("second plate.fcad");
    let again = reply(
        &run(&mut create_json(&other, Some(["80", "50", "12"]))),
        "create",
        0,
    )["result"]
        .clone();
    assert_ne!(again["document_id"], created["document_id"]);
    assert_eq!(plate_facts(&other).0, plate_facts(&sample).0);
}

#[test]
fn json_create_refusals_leave_destination_and_scratch_untouched() {
    let root = tempfile::tempdir().expect("dir");
    let occupied = root.path().join("taken.fcad");
    std::fs::write(&occupied, b"somebody else's file").expect("sentinel");
    let value = reply(&run(&mut create_json(&occupied, None)), "create", 2);
    assert_eq!(value["error"]["kind"], "input");
    assert!(
        value["error"]["message"]
            .as_str()
            .expect("message")
            .contains("already exists")
    );
    assert_eq!(
        std::fs::read(&occupied).expect("sentinel"),
        b"somebody else's file"
    );

    let zero_width = root.path().join("zero-width.fcad");
    reply(
        &run(&mut create_json(&zero_width, Some(["0", "40", "10"]))),
        "create",
        0,
    );
    assert_eq!(plate_facts(&zero_width).0[0], (0.0, 0.0));

    for (size, kind) in [
        (["NaN", "50", "12"], "input"),
        (["60", "40", "NaN"], "input"),
        (["inf", "50", "12"], "input"),
        (["60", "40", "0"], "input"),
    ] {
        let destination = root.path().join(format!("{}.fcad", size.join("x")));
        let names = entries(root.path());
        let value = reply(
            &run(&mut create_json(&destination, Some(size))),
            "create",
            2,
        );
        assert_eq!(value["error"]["kind"], kind, "{size:?}");
        assert!(!destination.exists());
        assert_eq!(entries(root.path()), names, "no scratch or sidecars");
    }
}

#[test]
fn json_create_closed_pipes_keep_publication_and_structured_errors() {
    let root = tempfile::tempdir().expect("dir");
    let occupied = root.path().join("taken.fcad");
    std::fs::write(&occupied, b"existing").expect("sentinel");
    let output = run(create_json(&occupied, None).stderr(closed_pipe()));
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON error");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["operation"], "create");
    assert_eq!(value["ok"], false);
    assert!(value.get("result").is_none());
    assert_eq!(value["error"]["kind"], "input");
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read(&occupied).expect("sentinel"), b"existing");

    let published = root.path().join("published without report.fcad");
    let names = entries(root.path());
    let lost = run(create_json(&published, None).stdout(closed_pipe()));
    assert_eq!(lost.status.code(), Some(7), "{lost:?}");
    assert!(lost.stdout.is_empty());
    let diagnostic = String::from_utf8(lost.stderr).expect("diagnostic");
    assert!(diagnostic.contains("JSON report delivery failed"));
    assert!(!diagnostic.contains("panicked"));
    assert!(published.exists());
    assert_eq!(inspect(&published)["features"], serde_json::json!([]));
    assert_eq!(entries(root.path()).len(), names.len() + 1);

    let silent = root.path().join("published with both pipes closed.fcad");
    let names = entries(root.path());
    let lost = run(create_json(&silent, Some(["80", "50", "12"]))
        .stdout(closed_pipe())
        .stderr(closed_pipe()));
    assert_eq!(lost.status.code(), Some(7), "{lost:?}");
    assert!(lost.stdout.is_empty() && lost.stderr.is_empty());
    assert_eq!(inspect(&silent)["features"][0]["distance_mm"], 12.0);
    assert_eq!(entries(root.path()).len(), names.len() + 1);

    let note = root.path().join("noext");
    let output = run(create_json(&note, None).stderr(closed_pipe()));
    let created = reply(&output, "create", 0)["result"].clone();
    assert_eq!(created["destination"], serde_json::json!(note));
    assert!(output.stderr.is_empty());
    assert_eq!(inspect(&note)["document_id"], created["document_id"]);
}

fn mixed_catalog(path: &Path) {
    create(path, Some(["80", "50", "12"]));
    let mut document = Document::open(path).expect("fixture");
    let original = document
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
        .expect("sample feature");
    let ObjectPayload::Extrude(extrude) = &original.payload else {
        unreachable!()
    };
    let parameter = ObjectId::new();
    document
        .write(|writer| {
            writer.put_object(
                original.id,
                original.parent,
                original.ordinal,
                Some("Плита \"А\"\nline\\two"),
                &original.payload,
            )?;
            writer.put_object(
                parameter,
                None,
                15,
                Some("height"),
                &ObjectPayload::Parameter(Parameter {
                    name: "height".into(),
                    dimension: Dimension::Length,
                    expression: Expression::constant(9.0)?,
                }),
            )?;
            for (ordinal, name, end, parameterized) in [
                (
                    8,
                    None,
                    EndCondition::Blind {
                        distance: Expression::constant(5.0)?,
                    },
                    false,
                ),
                (
                    5,
                    Some("Formula"),
                    EndCondition::Blind {
                        distance: Expression::new("height * 2", 18.0)?,
                    },
                    false,
                ),
                (
                    6,
                    Some("Parameter"),
                    EndCondition::Blind {
                        distance: Expression::constant(9.0)?,
                    },
                    true,
                ),
                (7, Some("Through"), EndCondition::ThroughAll, false),
                (
                    4,
                    Some("Symmetric"),
                    EndCondition::Symmetric {
                        distance: Expression::constant(6.0)?,
                    },
                    false,
                ),
            ] {
                let id = ObjectId::new();
                let mut feature = extrude.clone();
                feature.end_condition = end;
                writer.put_object(id, None, ordinal, name, &ObjectPayload::Extrude(feature))?;
                writer.add_dependency(Dependency {
                    dependent: id,
                    dependency: extrude.profile,
                    role: DependencyRole::Profile,
                })?;
                if parameterized {
                    writer.add_dependency(Dependency {
                        dependent: id,
                        dependency: parameter,
                        role: DependencyRole::Parameter,
                    })?;
                }
            }
            Ok(())
        })
        .expect("mixed catalog");
    document.close().expect("close");
}

#[test]
fn mixed_catalog_matches_common_facts_order_optional_values_and_copy_access() {
    let root = tempfile::tempdir().expect("dir");
    // Quotes/newlines are legal filenames on Unix; on Windows they are still
    // exercised as feature names. Neither variant uses lossy conversion.
    let name = if cfg!(unix) {
        "Каталог \"А\"\nс пробелами.fcad"
    } else {
        "Каталог с пробелами.fcad"
    };
    let source = root.path().join(name);
    mixed_catalog(&source);
    for guard in ["none", "trigger", "future"] {
        if guard == "trigger" {
            rusqlite::Connection::open(&source)
                .expect("fixture")
                .execute_batch(
                    "CREATE TRIGGER extension_write AFTER UPDATE ON objects BEGIN SELECT 1; END;",
                )
                .expect("guarded schema");
        } else if guard == "future" {
            rusqlite::Connection::open(&source)
                .expect("fixture")
                .execute_batch("DROP TRIGGER extension_write;")
                .expect("remove own fixture trigger");
            let mut document = Document::open(&source).expect("fixture");
            let envelope = Envelope::new(
                "future.feature",
                1,
                vec!["future.required.v1".into()],
                vec![0x01],
            )
            .to_bytes()
            .expect("envelope");
            let payload = ObjectPayload::from_storage_bytes(&envelope).expect("unknown");
            document
                .write(|w| w.put_object(ObjectId::new(), None, 99, None, &payload))
                .expect("future capability");
            document.close().expect("close");
        }
        let before = std::fs::read(&source).expect("bytes");
        let catalog = inspect(&source);
        let document = Document::open_read_only(&source).expect("read");
        let common = ExtrudeEditSource::read(&document).expect("common catalog");
        assert_eq!(
            catalog["content_version"],
            common.version.content.to_string()
        );
        assert_eq!(
            catalog["document_id"],
            common.version.document_id.to_string()
        );
        assert_eq!(catalog["display_units"]["length"], "in");
        assert_eq!(catalog["distance_unit"], "mm");
        let features = catalog["features"].as_array().expect("features");
        assert_eq!(features.len(), 6);
        assert_eq!(
            features
                .iter()
                .map(|f| f["name"].clone())
                .collect::<Vec<_>>(),
            vec![
                serde_json::json!("Плита \"А\"\nline\\two"),
                serde_json::json!("Symmetric"),
                serde_json::json!("Formula"),
                serde_json::json!("Parameter"),
                serde_json::json!("Through"),
                Value::Null
            ]
        );
        for (wire, fact) in features.iter().zip(&common.features) {
            assert_eq!(wire["feature_id"], fact.feature.to_string());
            assert_eq!(
                wire["name"],
                serde_json::to_value(&fact.name).expect("name")
            );
            assert_eq!(
                wire["distance_mm"],
                serde_json::to_value(fact.distance_mm).expect("distance")
            );
            assert_eq!(
                wire["refusal"],
                serde_json::to_value(&fact.refusal).expect("refusal")
            );
            assert_eq!(
                wire["editable"],
                common.refusal.is_none() && fact.refusal.is_none()
            );
        }
        assert!(features[4]["distance_mm"].is_null());
        assert!(features[0]["refusal"].is_null());
        assert_eq!(features[0]["distance_mm"], 12.0);
        assert_eq!(catalog["edit_extrude"]["available"], guard == "none");
        assert_eq!(
            catalog["edit_extrude"]["refusal"],
            serde_json::to_value(common.unavailable_reason()).expect("reason")
        );
        assert_eq!(
            catalog["edit_extrude"]["document_refusal"],
            serde_json::to_value(&common.refusal).expect("document refusal")
        );
        if guard != "none" {
            assert!(
                features.iter().all(|f| f["editable"] == false),
                "copy access must not be hidden"
            );
        }
        document.close().expect("close");
        assert_eq!(before, std::fs::read(&source).expect("unchanged"));
        assert_eq!(
            entries(root.path()),
            vec![source.file_name().expect("name")]
        );
    }
}

#[test]
fn read_failures_are_structured_and_never_migrate_or_create_files() {
    let root = tempfile::tempdir().expect("dir");
    let missing = root.path().join("missing.fcad");
    let output = run(cli().arg("inspect").arg(&missing).arg("--json"));
    let value = reply(&output, "inspect", 2);
    assert_eq!(value["error"]["kind"], "io");
    assert!(
        !value["error"]["causes"]
            .as_array()
            .expect("causes")
            .is_empty()
    );
    assert!(entries(root.path()).is_empty());
    for case in [
        "corrupt",
        "old schema",
        "future reader",
        "wal",
        "hidden rowid",
    ] {
        let path = root.path().join(format!("{case}.fcad"));
        if case == "corrupt" {
            std::fs::write(&path, b"not a document").expect("broken fixture");
        } else {
            create(&path, None);
            let connection = rusqlite::Connection::open(&path).expect("fixture");
            connection
                .execute_batch(match case {
                    "old schema" => "PRAGMA user_version = 1;",
                    "future reader" => "UPDATE meta SET minimum_reader_version = 999;",
                    "wal" => "PRAGMA journal_mode = WAL;",
                    "hidden rowid" => "CREATE TABLE extension(rowid TEXT, _ROWID_ TEXT, oid TEXT);",
                    _ => unreachable!(),
                })
                .expect("refusal fixture");
        }
        let names = entries(root.path());
        let bytes = std::fs::read(&path).expect("before");
        let output = run(cli().arg("inspect").arg(&path).arg("--json"));
        let value = reply(&output, "inspect", 2);
        assert_eq!(
            value["error"]["kind"],
            if case == "corrupt" {
                "io"
            } else {
                "unsupported"
            },
            "{output:?}"
        );
        assert_eq!(bytes, std::fs::read(&path).expect("after"));
        assert_eq!(entries(root.path()), names, "no sidecars or scratch");
    }
}

#[test]
fn clap_usage_help_and_flag_shaped_values_do_not_guess_a_global_json_mode() {
    let valid_feature = ObjectId::new().to_string();
    for args in [
        vec!["inspect", "--json"],
        vec![
            "edit-extrude",
            "source.fcad",
            "--json",
            "--feature",
            "bad",
            "--distance-mm",
            "27",
            "-o",
            "out.fcad",
        ],
        vec![
            "edit-extrude",
            "source.fcad",
            "--json",
            "--feature",
            &valid_feature,
            "--distance-mm",
            "not-number",
            "-o",
            "out.fcad",
        ],
        vec![
            "edit-extrude",
            "source.fcad",
            "--json",
            "--feature",
            &valid_feature,
            "--distance-mm",
            "27",
            "--expect-version",
            "short",
            "-o",
            "out.fcad",
        ],
        vec!["wrong-command", "--json"],
        vec!["validate", "source.fcad", "--json"],
        vec!["--json", "inspect", "source.fcad"],
        vec!["create", "--json"],
        vec!["export-stl", "source.fcad", "--json"],
        vec![
            "export-stl",
            "source.fcad",
            "-o",
            "out.stl",
            "--json",
            "--solid",
            &valid_feature,
            "--linear-deflection",
            "not-number",
        ],
        vec![
            "export-stl",
            "source.fcad",
            "-o",
            "out.stl",
            "--json",
            "--expect-version",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ],
        vec![
            "create",
            "dest.fcad",
            "--json",
            "--sample",
            "--size",
            "not-number",
            "50",
            "12",
        ],
    ] {
        let output = run(cli().args(&args));
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "clap usage belongs to stderr");
        let usage = String::from_utf8(output.stderr).expect("usage");
        assert!(usage.contains("error:"));
        if args.contains(&"not-number") && args.contains(&"--distance-mm") {
            assert!(
                usage.contains("--distance-mm") && usage.contains("not-number"),
                "{usage}"
            );
        }
        if args.contains(&"short") {
            assert!(
                usage.contains("--expect-version") && usage.contains("short"),
                "{usage}"
            );
        }
        if args.contains(&"not-number") && args.contains(&"--size") {
            assert!(
                usage.contains("--size") && usage.contains("not-number"),
                "{usage}"
            );
        }
    }
    for args in [
        vec!["inspect", "--json", "--help"],
        vec!["edit-extrude", "--json", "--help"],
        vec!["create", "--json", "--help"],
        vec!["export-stl", "--json", "--help"],
        vec!["--version"],
    ] {
        let output = success(cli().args(args));
        assert!(serde_json::from_slice::<Value>(&output.stdout).is_err());
        assert!(output.stderr.is_empty());
    }
    let root = tempfile::tempdir().expect("dir");
    // A path after -- is a path, even when its entire spelling is --json.
    create(&root.path().join("--json"), None);
    let output = success(
        cli()
            .current_dir(root.path())
            .args(["inspect", "--", "--json"]),
    );
    assert!(
        String::from_utf8(output.stdout)
            .expect("text")
            .starts_with("document --json\n")
    );
    let creation_root = tempfile::tempdir().expect("create a literal flag name");
    let created = success(
        cli()
            .current_dir(creation_root.path())
            .args(["create", "--", "--json"]),
    );
    assert!(
        String::from_utf8(created.stdout)
            .expect("text")
            .starts_with("created --json (")
    );
    let json_named = reply(
        &run(cli().current_dir(root.path()).args([
            "create",
            "--json",
            "--",
            "--json-created.fcad",
        ])),
        "create",
        0,
    );
    assert_eq!(json_named["result"]["destination"], "--json-created.fcad");
    assert_eq!(
        inspect(&root.path().join("--json-created.fcad"))["document_id"],
        json_named["result"]["document_id"]
    );
    // A value of --feature likewise cannot activate serialization before clap
    // has parsed a valid command; it remains an ordinary usage error.
    let output = run(cli().args([
        "edit-extrude",
        "source",
        "--feature=--json",
        "--distance-mm",
        "27",
        "-o",
        "out",
    ]));
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[cfg(any(unix, windows))]
fn non_unicode() -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(b"invalid-\xff.fcad".to_vec())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[b'x' as u16, 0xD800, b'.' as u16, b'f' as u16])
    }
}

#[cfg(any(unix, windows))]
#[test]
fn non_utf8_paths_refuse_before_reading_or_publishing() {
    let root = tempfile::tempdir().expect("dir");
    let invalid = root.path().join(non_unicode());
    let output = run(cli().arg("inspect").arg(&invalid).arg("--json"));
    let value = reply(&output, "inspect", 2);
    assert_eq!(
        value["error"]["kind"], "input",
        "path policy must precede filesystem access"
    );
    assert!(entries(root.path()).is_empty());
    let created = run(cli().arg("create").arg(&invalid).arg("--json"));
    let value = reply(&created, "create", 2);
    assert_eq!(value["error"]["kind"], "input");
    assert!(
        value["error"]["message"]
            .as_str()
            .expect("message")
            .contains("UTF-8")
    );
    assert!(entries(root.path()).is_empty());
    let source = root.path().join("source.fcad");
    create(&source, Some(["80", "50", "12"]));
    let before = std::fs::read(&source).expect("before");
    let reading = inspect(&source);
    let id = selected(&reading, "Extrude1");
    let version = reading["content_version"].as_str().expect("version");
    for (from, to) in [
        (&source, &invalid),
        (&invalid, &root.path().join("out.fcad")),
    ] {
        let output = run(&mut edit(from, &id, "27", version, to));
        let value = reply(&output, "edit-extrude", 2);
        assert_eq!(value["error"]["kind"], "input");
        assert!(
            value["error"]["message"]
                .as_str()
                .expect("message")
                .contains("UTF-8")
        );
        let body = reading["bodies"][0]["body_id"].as_str().expect("Body");
        let output = run(&mut stl::export(from, to, Some(body), true));
        let value = reply(&output, "export-stl", 2);
        assert_eq!(value["error"]["kind"], "input");
        assert!(
            value["error"]["message"]
                .as_str()
                .expect("message")
                .contains("UTF-8")
        );
    }
    assert_eq!(before, std::fs::read(&source).expect("after"));
    assert_eq!(
        entries(root.path()),
        vec![source.file_name().expect("name")]
    );
}

#[test]
fn no_native_inspection_succeeds_and_edit_reports_unavailable_without_publication() {
    if ferritecad_occt::is_available() {
        return;
    }
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join("source.fcad");
    create(&source, Some(["80", "50", "12"]));
    let before = std::fs::read(&source).expect("before");
    let reading = inspect(&source);
    let output = run(&mut edit(
        &source,
        &selected(&reading, "Extrude1"),
        "27",
        reading["content_version"].as_str().expect("version"),
        &root.path().join("out.fcad"),
    ));
    assert_eq!(
        reply(&output, "edit-extrude", 2)["error"]["kind"],
        "unsupported"
    );
    let body = reading["bodies"][0]["body_id"]
        .as_str()
        .expect("discovered Body");
    let destination = root.path().join("out.stl");
    let output = run(&mut stl::export(&source, &destination, Some(body), true));
    assert_eq!(
        reply(&output, "export-stl", 2)["error"]["kind"],
        "unsupported"
    );
    assert_eq!(before, std::fs::read(&source).expect("after"));
    assert_eq!(
        entries(root.path()),
        vec![source.file_name().expect("name")]
    );
    std::fs::write(&destination, b"stub keeps output").expect("sentinel");
    let output = run(stl::export(&source, &destination, Some(body), true).arg("--force"));
    assert_eq!(
        reply(&output, "export-stl", 2)["error"]["kind"],
        "unsupported"
    );
    assert_eq!(
        std::fs::read(&destination).expect("kept"),
        b"stub keeps output"
    );
    assert_eq!(std::fs::read(&source).expect("source unchanged"), before);
    assert_eq!(entries(root.path()).len(), 2, "no scratch or sidecars");
}

/// Compare complete typed SQL content of two copies of ONE source. Only the
/// actual modification timestamp may differ; identifiers are never normalized.
fn tables(path: &Path) -> Vec<(String, Vec<Vec<rusqlite::types::Value>>)> {
    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("read");
    let names: Vec<String> = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .expect("schema")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<rusqlite::Result<_>>()
        .expect("names");
    names
        .into_iter()
        .map(|name| {
            let mut query = connection
                .prepare(&format!("SELECT * FROM \"{}\"", name.replace('"', "\"\"")))
                .expect("query");
            let count = query.column_count();
            let modified = query
                .column_names()
                .iter()
                .position(|c| *c == "modified_at");
            let mut rows: Vec<Vec<rusqlite::types::Value>> = query
                .query_map([], |row| {
                    (0..count)
                        .map(|i| {
                            if name == "meta" && Some(i) == modified {
                                Ok(rusqlite::types::Value::Null)
                            } else {
                                row.get(i)
                            }
                        })
                        .collect::<rusqlite::Result<_>>()
                })
                .expect("rows")
                .collect::<rusqlite::Result<_>>()
                .expect("values");
            rows.sort_by_key(|row| format!("{row:?}"));
            (name, rows)
        })
        .collect()
}

fn stl_bounds(bytes: &[u8]) -> ([f64; 3], f64) {
    let count = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + count * 50);
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut volume = 0.0;
    for triangle in bytes[84..].chunks_exact(50) {
        let mut points = [[0.0; 3]; 3];
        for (v, point) in points.iter_mut().enumerate() {
            for (axis, value) in point.iter_mut().enumerate() {
                let offset = 12 + v * 12 + axis * 4;
                *value = f32::from_le_bytes(
                    triangle[offset..offset + 4].try_into().expect("coordinate"),
                ) as f64;
                low[axis] = low[axis].min(*value);
                high[axis] = high[axis].max(*value);
            }
        }
        let [a, b, c] = points;
        volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.0;
    }
    assert_eq!(low, [0.0; 3]);
    (high, volume.abs())
}

#[test]
fn closed_stderr_does_not_prevent_structured_errors_or_delivery_exit() {
    let root = tempfile::tempdir().expect("dir");
    let missing = root.path().join("missing.fcad");
    let feature = ObjectId::new().to_string();
    for operation in ["inspect", "edit-extrude", "export-stl"] {
        let command = || {
            let mut command = cli();
            command.arg(operation).arg(&missing).arg("--json");
            if operation == "edit-extrude" {
                command
                    .args(["--feature", &feature, "--distance-mm", "27", "-o"])
                    .arg(root.path().join("output.fcad"));
            }
            if operation == "export-stl" {
                command
                    .args(["--solid", &feature, "-o"])
                    .arg(root.path().join("output.stl"));
            }
            command
        };
        let output = run(command().stderr(closed_pipe()));
        assert_eq!(output.status.code(), Some(2), "{operation}: {output:?}");
        assert_eq!(
            output.stdout.last(),
            Some(&b'\n'),
            "{operation}: {output:?}"
        );
        assert_eq!(output.stdout.iter().filter(|b| **b == b'\n').count(), 1);
        let value: Value = serde_json::from_slice(&output.stdout).expect("JSON error");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["operation"], operation);
        assert_eq!(value["ok"], false);
        assert!(value.get("result").is_none());
        assert_eq!(value["error"]["kind"], "io");
        assert!(
            !value["error"]["message"]
                .as_str()
                .expect("message")
                .is_empty()
        );
        assert!(
            !value["error"]["causes"]
                .as_array()
                .expect("causes")
                .is_empty()
        );
        assert!(output.stderr.is_empty());

        let lost = run(command().stdout(closed_pipe()).stderr(closed_pipe()));
        assert_eq!(lost.status.code(), Some(7), "{operation}: {lost:?}");
        assert!(lost.stdout.is_empty() && lost.stderr.is_empty());
        assert!(
            entries(root.path()).is_empty(),
            "no publication or sidecars"
        );
    }
}

#[test]
fn native_json_inspect_edit_contract() {
    if !ferritecad_occt::is_available() {
        assert_ne!(
            std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(),
            Ok("1"),
            "native JSON gate requires OCCT"
        );
        eprintln!("skipped: this build has no Open CASCADE (JSON edit/STL process gate)");
        return;
    }
    stl::native_contract();
    for (index, size) in [["80", "50", "12"], ["91", "53", "17"]]
        .into_iter()
        .enumerate()
    {
        let root = tempfile::tempdir().expect("dir");
        let source = root.path().join("Исходная плита.fcad");
        let reading = if index == 0 {
            let created =
                reply(&run(&mut create_json(&source, Some(size))), "create", 0)["result"].clone();
            assert_eq!(created["destination"], serde_json::json!(source));
            let reading = inspect(&source);
            assert_eq!(created["document_id"], reading["document_id"]);
            reading
        } else {
            create(&source, Some(size));
            inspect(&source)
        };
        let before = std::fs::read(&source).expect("bytes");
        let feature = selected(&reading, "Extrude1");
        let version = reading["content_version"].as_str().expect("full token");
        assert_eq!(version.len(), 64);
        let destination = root.path().join("Изменённая плита.fcad");
        let output = run(&mut edit(&source, &feature, "27", version, &destination));
        let result = reply(&output, "edit-extrude", 0);
        assert_eq!(
            result["result"],
            serde_json::json!({"destination":destination,"document_id":reading["document_id"],"feature_id":feature})
        );
        let edited = inspect(&destination);
        assert_eq!(edited["document_id"], reading["document_id"]);
        assert_eq!(selected(&edited, "Extrude1"), feature);
        assert_eq!(edited["features"][0]["distance_mm"], 27.0);
        assert_ne!(edited["content_version"], reading["content_version"]);
        assert_eq!(edited["display_units"]["length"], "in");

        // The JSON CLI is compared with the existing shared UI/jobs operation.
        // The eight viewer gates independently exercise its actual UI wiring.
        let jobs_output = root.path().join("jobs copy.fcad");
        let common = ferritecad_jobs::read_extrude_source(&source).expect("source");
        let request = ferritecad_jobs::EditExtrudeRequest {
            source: source.clone(),
            expected: common.version,
            feature: feature.parse().expect("UUID"),
            distance_mm: 27.0,
            destination: jobs_output.clone(),
        };
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        ferritecad_jobs::edit_extrude_copy(&request, &mut kernel, &OperationContext::default())
            .expect("shared operation");
        assert_eq!(
            tables(&destination),
            tables(&jobs_output),
            "whole content, including every UUID"
        );
        let old = Document::open_read_only(&source).expect("source");
        let new = Document::open_read_only(&destination).expect("reopen");
        assert_eq!(new.objects().expect("objects").len(), 4);
        assert_eq!(
            old.dependencies().expect("deps"),
            new.dependencies().expect("deps")
        );
        assert_eq!(
            old.topology_refs().expect("refs"),
            new.topology_refs().expect("refs")
        );
        for object in old.objects().expect("objects") {
            let mut expected = object.clone();
            if object.id.to_string() == feature {
                let ObjectPayload::Extrude(e) = &mut expected.payload else {
                    unreachable!()
                };
                e.end_condition = EndCondition::Blind {
                    distance: Expression::constant(27.0).expect("constant"),
                };
                // Payload hash is derived from the changed payload.
                assert_eq!(
                    new.object(object.id)
                        .expect("object")
                        .expect("same UUID")
                        .payload,
                    expected.payload
                );
            } else {
                assert_eq!(new.object(object.id).expect("object"), Some(object));
            }
        }
        old.close().expect("close");
        new.close().expect("close");
        success(cli().arg("validate").arg(&destination));
        let rebuilt = success(cli().arg("rebuild").arg(&destination).arg("--cold"));
        let rebuilt = String::from_utf8(rebuilt.stdout).expect("report");
        assert!(rebuilt.contains("4 objects evaluated, 1 shape built"));
        assert!(rebuilt.contains("3 of 3 stored references resolved"));
        for (path, height) in [
            (&source, size[2].parse::<f64>().expect("height")),
            (&destination, 27.0),
            (&jobs_output, 27.0),
        ] {
            success(
                cli()
                    .arg("export-stl")
                    .arg(path)
                    .arg("-o")
                    .arg(path.with_extension("stl")),
            );
            success(
                cli()
                    .arg("export-fbx")
                    .arg(path)
                    .arg("-o")
                    .arg(path.with_extension("fbx")),
            );
            let (bounds, volume) =
                stl_bounds(&std::fs::read(path.with_extension("stl")).expect("STL"));
            let x = size[0].parse::<f64>().expect("x");
            let y = size[1].parse::<f64>().expect("y");
            assert_eq!(bounds, [x, y, height]);
            assert!((volume - x * y * height).abs() < 1e-6);
        }
        for ext in ["stl", "fbx"] {
            assert_eq!(
                std::fs::read(destination.with_extension(ext)).expect("CLI"),
                std::fs::read(jobs_output.with_extension(ext)).expect("jobs")
            );
        }
        // Text mode still has its old report and codes, and uses the same edit.
        let text_copy = root.path().join("text.fcad");
        let text = success(
            cli()
                .arg("edit-extrude")
                .arg(&source)
                .args(["--feature", &feature, "--distance-mm", "27", "-o"])
                .arg(&text_copy),
        );
        assert!(
            String::from_utf8(text.stdout)
                .expect("text")
                .starts_with("saved ")
        );
        assert_eq!(tables(&destination), tables(&text_copy));

        let refused = root.path().join("refused.fcad");
        for (id, distance, token, kind) in [
            (feature.clone(), "NaN", version.to_owned(), "input"),
            (feature.clone(), "inf", version.to_owned(), "input"),
            (feature.clone(), "-1", version.to_owned(), "input"),
            (feature.clone(), "0", version.to_owned(), "input"),
            (
                ObjectId::new().to_string(),
                "27",
                version.to_owned(),
                "input",
            ),
            (feature.clone(), "27", "0".repeat(64), "input"),
        ] {
            let names = entries(root.path());
            let error = run(&mut edit(&source, &id, distance, &token, &refused));
            assert_eq!(reply(&error, "edit-extrude", 2)["error"]["kind"], kind);
            assert_eq!(entries(root.path()), names);
        }
        std::fs::write(&refused, b"existing output").expect("sentinel");
        let error = run(&mut edit(&source, &feature, "27", version, &refused));
        assert_eq!(reply(&error, "edit-extrude", 2)["error"]["kind"], "input");
        assert_eq!(
            std::fs::read(&refused).expect("sentinel"),
            b"existing output"
        );
        let alias = root.path().join("hard link.fcad");
        std::fs::hard_link(&source, &alias).expect("hard link");
        for to in [&source, &alias] {
            let error = run(&mut edit(&source, &feature, "27", version, to));
            assert_eq!(reply(&error, "edit-extrude", 2)["error"]["kind"], "input");
        }
        // Broken pipe after actual publication is delivery exit 7, not panic
        // exit 101, rollback, or a second edit attempt.
        let delivered = root.path().join("published without report.fcad");
        let names = entries(root.path());
        let output = run(edit(&source, &feature, "27", version, &delivered).stdout(closed_pipe()));
        assert_eq!(output.status.code(), Some(7), "{output:?}");
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8(output.stderr).expect("diagnostic");
        assert!(diagnostic.contains("JSON report delivery failed"));
        assert!(!diagnostic.contains("panicked"));
        assert_eq!(tables(&destination), tables(&delivered));
        assert_eq!(entries(root.path()).len(), names.len() + 1);
        assert_eq!(inspect(&delivered)["features"][0]["distance_mm"], 27.0);
        assert_eq!(before, std::fs::read(&source).expect("source unchanged"));
    }
    native_selection_and_stale_source();
    native_structured_refusals();
}

fn native_structured_refusals() {
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join("refusals.fcad");
    mixed_catalog(&source);
    let reading = inspect(&source);
    let version = reading["content_version"].as_str().expect("version");
    let before = std::fs::read(&source).expect("source");
    let destination = root.path().join("refused.fcad");
    for feature in reading["features"].as_array().expect("catalog") {
        if feature["editable"] == false {
            let error = run(&mut edit(
                &source,
                feature["feature_id"].as_str().expect("UUID"),
                "27",
                version,
                &destination,
            ));
            assert_eq!(
                reply(&error, "edit-extrude", 2)["error"]["kind"],
                "unsupported"
            );
            assert!(!destination.exists());
            assert_eq!(before, std::fs::read(&source).expect("unchanged"));
        }
    }
    // The same feature has no local refusal, but a document-wide write guard
    // must still refuse the operation and expose its coarse typed category.
    for guard in [
        "CREATE TRIGGER extension_write AFTER UPDATE ON objects BEGIN SELECT 1; END;",
        "DROP TRIGGER extension_write; CREATE TABLE extension_fk (object_id BLOB REFERENCES objects(id) ON UPDATE CASCADE);",
    ] {
        rusqlite::Connection::open(&source)
            .expect("fixture")
            .execute_batch(guard)
            .expect("guard");
        let catalog = inspect(&source);
        let before = std::fs::read(&source).expect("guarded source");
        assert!(catalog["edit_extrude"]["document_refusal"].is_string());
        let feature = &catalog["features"][0];
        assert!(feature["refusal"].is_null());
        assert_eq!(feature["editable"], false);
        let error = run(&mut edit(
            &source,
            feature["feature_id"].as_str().expect("UUID"),
            "27",
            catalog["content_version"].as_str().expect("version"),
            &destination,
        ));
        assert_eq!(
            reply(&error, "edit-extrude", 2)["error"]["kind"],
            "unsupported"
        );
        assert!(!destination.exists());
        assert_eq!(before, std::fs::read(&source).expect("unchanged"));
        assert_eq!(
            entries(root.path()),
            vec![source.file_name().expect("name")]
        );
    }
}

fn native_selection_and_stale_source() {
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join("multiple.fcad");
    create(&source, Some(["80", "50", "12"]));
    let initial = inspect(&source);
    let first: ObjectId = selected(&initial, "Extrude1").parse().expect("UUID");
    let mut doc = Document::open(&source).expect("fixture");
    let original = doc.object(first).expect("object").expect("extrude");
    let second = ObjectId::new();
    let ObjectPayload::Extrude(extrude) = &original.payload else {
        unreachable!()
    };
    doc.write(|w| {
        w.put_object(second, None, 9, Some("Explicit second"), &original.payload)?;
        w.add_dependency(Dependency {
            dependent: second,
            dependency: extrude.profile,
            role: DependencyRole::Profile,
        })?;
        Ok(())
    })
    .expect("second feature");
    doc.close().expect("close");
    let reading = inspect(&source);
    assert_eq!(reading["features"].as_array().expect("features").len(), 2);
    let id = selected(&reading, "Explicit second");
    assert_eq!(id, second.to_string());
    let version = reading["content_version"].as_str().expect("version");
    let destination = root.path().join("second changed.fcad");
    reply(
        &run(&mut edit(&source, &id, "29", version, &destination)),
        "edit-extrude",
        0,
    );
    let changed = inspect(&destination);
    assert_eq!(
        changed["features"][0]["distance_mm"], 12.0,
        "must not choose first"
    );
    assert_eq!(changed["features"][1]["distance_mm"], 29.0);
    // Change actual content, not mtime, after public discovery.
    let sql = rusqlite::Connection::open(&source).expect("fixture");
    let updated = sql
        .execute(
            "UPDATE objects SET name='changed since inspect' WHERE id=?1",
            [first.to_bytes()],
        )
        .expect("change");
    assert_eq!(updated, 1, "the stale-source fixture must change a row");
    drop(sql);
    let current = inspect(&source);
    assert_ne!(current["content_version"], reading["content_version"]);
    let before = std::fs::read(&source).expect("changed source");
    let refused = root.path().join("stale.fcad");
    let error = run(&mut edit(&source, &id, "31", version, &refused));
    assert_eq!(reply(&error, "edit-extrude", 2)["error"]["kind"], "input");
    assert!(!refused.exists());
    assert_eq!(before, std::fs::read(&source).expect("unchanged"));
}
