// SPDX-License-Identifier: MIT
//! Real complete/partial JSON processes alongside the existing FBX campaign.

use super::*;
use serde_json::Value;

fn command(source: &Path, destination: &Path) -> Command {
    let mut command = Command::new(ferritecad());
    command
        .arg("export-fbx")
        .arg(source)
        .arg("-o")
        .arg(destination)
        .arg("--json");
    command
}

fn reply(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert_eq!(output.stdout.iter().filter(|b| **b == b'\n').count(), 1);
    let value: Value = serde_json::from_slice(&output.stdout).expect("one standard JSON object");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["operation"], "export-fbx");
    let published = exit == 0 || exit == 6;
    assert_eq!(value["ok"], published);
    if published {
        assert!(value["result"].is_object());
        assert!(value.get("error").is_none());
        assert!(output.stderr.is_empty(), "{output:?}");
    } else {
        assert!(value.get("result").is_none());
        assert!(value["error"]["kind"].is_string());
        assert!(value["error"]["message"].is_string());
        assert!(value["error"]["causes"].is_array());
    }
    value
}

fn counts(result: &Value, destination: &Path, complete: bool) -> Written {
    let written = Written::scan(destination);
    assert_eq!(written.version, 7400);
    assert_eq!(written.unit_scale_factor, 100.0);
    assert_eq!(result["destination"], destination.to_str().expect("UTF-8"));
    assert_eq!(
        result["bytes"].as_u64(),
        Some(std::fs::metadata(destination).expect("published").len())
    );
    assert_eq!(result["models"].as_u64(), Some(written.models.len() as u64));
    assert_eq!(
        result["geometries"].as_u64(),
        Some(written.geometries.len() as u64)
    );
    assert_eq!(result["materials"].as_u64(), Some(written.materials as u64));
    assert_eq!(result["complete"], complete);
    assert_eq!(
        result["omissions"]
            .as_array()
            .expect("required array")
            .is_empty(),
        complete
    );
    assert!(!written.geometries.is_empty(), "preserved real geometry");
    assert!(
        written
            .geometries
            .iter()
            .all(|g| g.polygons > 0 && g.polygon_vertices == g.polygons * 3)
    );
    written
}

fn files(root: &Path) -> BTreeMap<PathBuf, ferritecad_types::ContentHash> {
    std::fs::read_dir(root)
        .expect("directory")
        .map(|e| {
            let path = e.expect("entry").path();
            let bytes = std::fs::read(&path).expect("file, no untracked scratch directory");
            (path, ferritecad_types::ContentHash::of_bytes(&bytes))
        })
        .collect()
}

// One prepared document per campaign, never re-imported per pipe case. Each
// real process still cold rebuilds through the production publication route.
fn delivery(source: &Path, expected: &Path) {
    let root = source.parent().expect("root");
    let bytes = std::fs::read(expected).expect("reference FBX");
    for (force, close_stderr) in [(false, false), (true, false), (false, true), (true, true)] {
        let out = root.join("delivery.fbx");
        if force {
            std::fs::write(&out, b"previous confirmed destination").expect("old output");
        }
        let mut before = files(root);
        let mut cmd = command(source, &out);
        cmd.stdout(super::pipe::closed_pipe());
        if force {
            cmd.arg("--force");
        }
        if close_stderr {
            cmd.stderr(super::pipe::closed_pipe());
        }
        let result = cmd.output().expect("direct OS pipe process");
        assert_eq!(result.status.code(), Some(7), "{result:?}");
        if !close_stderr {
            assert!(!result.stderr.is_empty());
            assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
        }
        assert_eq!(
            std::fs::read(&out).expect("publication survives delivery"),
            bytes
        );
        assert_eq!(Written::scan(&out).version, 7400);
        before.insert(out.clone(), ferritecad_types::ContentHash::of_bytes(&bytes));
        assert_eq!(
            files(root),
            before,
            "only the published destination changes"
        );
        std::fs::remove_file(out).expect("remove test output");
    }
}

fn artefact(name: &str, path: &Path) {
    if let Some(root) = std::env::var_os("FCAD_FBX_JSON_OUT") {
        std::fs::copy(path, PathBuf::from(root).join(name)).expect("independent reader artefact");
    }
}

#[test]
fn native_json_fbx_complete_publication() {
    if !is_available() {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let native = root.path().join("Плита with spaces.fcad");
    let created = Command::new(ferritecad())
        .arg("create")
        .arg(&native)
        .args(["--json", "--sample", "--size", "80", "50", "12"])
        .output()
        .expect("JSON create");
    assert!(created.status.success());
    let created: Value = serde_json::from_slice(&created.stdout).expect("JSON create reply");
    assert_eq!(created["ok"], true);
    let imported = root.path().join("import.fcad");
    let step = root.path().join("private.step");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/step/canonical/01-single-part.step"),
        &step,
    )
    .expect("private STEP copy");
    let result = Command::new(ferritecad())
        .arg("import-step")
        .arg(&step)
        .arg("-o")
        .arg(&imported)
        .output()
        .expect("import");
    assert_eq!(result.status.code(), Some(0), "{result:?}");
    std::fs::remove_file(&step).expect("only our STEP copy removed");
    for (source, name) in [(&native, "native.fbx"), (&imported, "imported.fbx")] {
        let before = files(root.path());
        let out = root.path().join(name);
        let result = reply(&command(source, &out).output().expect("JSON export"), 0);
        let written = counts(&result["result"], &out, true);
        assert_eq!(written.geometries.len(), 1);
        assert_eq!(written.geometries[0].polygons, 12);
        let text = root.path().join("text.fbx");
        let result = export_command(source, &text);
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        assert_eq!(
            std::fs::read(&out).expect("JSON FBX"),
            std::fs::read(&text).expect("text FBX")
        );
        delivery(source, &out);
        artefact(name, &out);
        std::fs::remove_file(&out).expect("test output");
        std::fs::remove_file(&text).expect("test output");
        assert_eq!(files(root.path()), before, "source and directory unchanged");
    }
    assert!(!step.exists());
    eprintln!("FCAD_EXPORT_FBX_JSON_COMPLETE native=1 imported=1 delivery=8");
}

pub(super) fn partial_contract(source: &Path, text: &Path, scene: &ExportScene) {
    let out = source.with_file_name("complex-json.fbx");
    let before = files(source.parent().expect("root"));
    let envelope = reply(
        &command(source, &out)
            .output()
            .expect("real partial JSON process"),
        6,
    );
    let result = &envelope["result"];
    let written = counts(result, &out, false);
    assert_eq!(
        std::fs::read(text).expect("text bytes"),
        std::fs::read(&out).expect("JSON bytes")
    );
    let omissions = result["omissions"].as_array().expect("omissions");
    assert_eq!(omissions.len(), scene.completeness().omissions().len());
    for (actual, expected) in omissions.iter().zip(scene.completeness().omissions()) {
        let ExportSource::Imported {
            source,
            definition_key,
        } = &expected.source
        else {
            panic!("imported source")
        };
        assert_eq!(actual["source"]["kind"], "imported");
        assert_eq!(actual["source"]["source_id"], source.to_string());
        assert_eq!(actual["source"]["definition_key"], *definition_key);
        assert_eq!(actual["finding"]["stage"], "validation");
        assert_eq!(actual["finding"]["severity"], "fail");
        assert_eq!(
            actual["finding"]["entity"],
            expected.omission.finding.entity
        );
        assert_eq!(
            actual["finding"]["message"],
            expected.omission.finding.message
        );
        assert_eq!(actual["refusal"], "IncompleteFace");
        let keys: Vec<String> = expected
            .nodes
            .iter()
            .map(|n| format!("node/{}", n.index()))
            .collect();
        assert_eq!(
            actual["placements"],
            serde_json::to_value(&keys).expect("keys")
        );
        for key in &keys {
            let models: Vec<_> = written
                .models
                .iter()
                .filter(|m| m.properties.get("FerriteCADNodeKey") == Some(key))
                .collect();
            assert_eq!(models.len(), 1);
            let model = models[0];
            assert_eq!(model.class, "Null");
            assert_eq!(written.geometry_of(model.id), None);
            assert_eq!(
                model.properties.get("FerriteCADDefinitionKey"),
                Some(definition_key)
            );
            assert_eq!(
                model.properties.get("FerriteCADOmissionFinding"),
                Some(&expected.omission.finding.entity)
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
    }
    assert_eq!(written.geometries.len(), 34, "retained geometry");
    delivery(source, &out);
    artefact("partial.fbx", &out);
    std::fs::remove_file(out).expect("test output");
    assert_eq!(files(source.parent().expect("root")), before);
    eprintln!(
        "FCAD_EXPORT_FBX_JSON_PARTIAL omissions={} delivery=4",
        omissions.len()
    );
}

#[test]
fn json_fbx_refusals_preserve_files_and_protocol_in_native_and_stub_builds() {
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.fcad");
    let out = root.path().join("output.fbx");
    ferritecad_document::Document::create(&source)
        .expect("empty document")
        .close()
        .expect("close");
    let refuses = |cmd: &mut Command, kind: &str| {
        let before = files(root.path());
        let output = cmd.output().expect("refused process");
        let value = reply(&output, 2);
        assert_eq!(value["error"]["kind"], kind, "{output:?}");
        assert_eq!(
            files(root.path()),
            before,
            "refusal changed files or left scratch"
        );
    };
    std::fs::write(&out, b"occupied destination").expect("sentinel");
    refuses(&mut command(&source, &out), "input");
    refuses(command(&source, &source).arg("--force"), "input");
    let hard = root.path().join("hardlink.fbx");
    std::fs::hard_link(&source, &hard).expect("hardlink");
    refuses(command(&source, &hard).arg("--force"), "input");
    std::fs::remove_file(hard).expect("private alias");
    #[cfg(unix)]
    {
        let alias = root.path().join("symlink.fbx");
        std::os::unix::fs::symlink(&source, &alias).expect("symlink");
        refuses(command(&source, &alias).arg("--force"), "input");
        std::fs::remove_file(alias).expect("private alias");
    }
    for case in ["missing", "broken", "wal", "old", "future"] {
        let bad = root.path().join(format!("{case}.fcad"));
        if case == "broken" {
            std::fs::write(&bad, b"broken document").expect("broken");
        } else if case != "missing" {
            ferritecad_document::Document::create(&bad)
                .expect("fixture")
                .close()
                .expect("close");
            let conn = rusqlite::Connection::open(&bad).expect("fixture SQL");
            conn.execute_batch(match case {
                "wal" => "PRAGMA journal_mode = WAL;",
                "old" => "PRAGMA user_version = 1;",
                "future" => "UPDATE meta SET minimum_reader_version = 999;",
                _ => unreachable!(),
            })
            .expect("restricted fixture");
        }
        let expected = if case == "missing" || (case == "broken" && is_available()) {
            "io"
        } else {
            "unsupported"
        };
        // A valid --force reaches the source refusal instead of no-clobber.
        refuses(command(&bad, &out).arg("--force"), expected);
    }
    let before = files(root.path());
    for both in [false, true] {
        let mut cmd = command(&source, &out); // no-clobber even in a stub build
        cmd.stderr(super::pipe::closed_pipe());
        if both {
            cmd.stdout(super::pipe::closed_pipe());
        }
        let output = cmd.output().expect("closed diagnostics");
        if both {
            assert_eq!(output.status.code(), Some(7), "{output:?}");
        } else {
            assert_eq!(reply(&output, 2)["error"]["kind"], "input");
        }
    }
    assert_eq!(files(root.path()), before);

    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let bad = root
            .path()
            .join(OsString::from_vec(b"non-utf8-\xff".to_vec()));
        refuses(command(&bad, &out).arg("--force"), "input");
        refuses(&mut command(&source, &bad), "input");
    }
    // The parser's policy remains text before successful parsing.
    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        let bad = root.path().join(OsString::from_wide(&[0xd800]));
        refuses(command(&bad, &out).arg("--force"), "input");
        refuses(&mut command(&source, &bad), "input");
    }
    let output = Command::new(ferritecad())
        .arg("export-fbx")
        .arg(&source)
        .arg("--json")
        .output()
        .expect("missing output is clap usage");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    for extra in [vec!["--solid", "Plate"], vec!["--expect-version", "x"]] {
        let before = files(root.path());
        let output = command(&source, &out).args(extra).output().expect("usage");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(files(root.path()), before);
    }
    let help = command(&source, &out).arg("--help").output().expect("help");
    assert!(help.status.success());
    assert!(serde_json::from_slice::<Value>(&help.stdout).is_err());
    assert!(help.stderr.is_empty());
    let flag = root.path().join("--json");
    std::fs::copy(&source, &flag).expect("flag-shaped source");
    let before = files(root.path());
    let output = Command::new(ferritecad())
        .current_dir(root.path())
        .args(["export-fbx", "-o", "output.fbx", "--", "--json"])
        .output()
        .expect("literal filename");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "filename enabled JSON mode");
    assert_eq!(files(root.path()), before);
    let output = Command::new(ferritecad())
        .current_dir(root.path())
        .arg("export-fbx")
        .arg(&source)
        .arg("--output=--json")
        .output()
        .expect("flag-shaped output value");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "output value enabled JSON mode");
    assert_eq!(files(root.path()), before);

    if !is_available() {
        // Actual geometry refusal, not a skipped export or a path-only check.
        let plate = root.path().join("native.fcad");
        let created = Command::new(ferritecad())
            .arg("create")
            .arg(&plate)
            .args(["--sample", "--json"])
            .output()
            .expect("stub create");
        assert!(created.status.success());
        refuses(command(&plate, &out).arg("--force"), "unsupported");
        std::fs::remove_file(&out).expect("sentinel");
        refuses(&mut command(&plate, &out), "unsupported");
    }
}
