// SPDX-License-Identifier: MIT
//! STEP JSON is proved through real CLI processes and direct closed OS pipes.
#![allow(clippy::panic)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ferritecad_document::{Document, ImportedStep};
use ferritecad_exchange::{Diagnostic, Severity, Stage, StoredScene};
use ferritecad_kernel::GeometryKernel;
use ferritecad_occt::OcctKernel;
use ferritecad_types::{ContentHash, DocumentId, ErrorKind, ObjectId};
use serde_json::{Value, json};

#[path = "support/pipe.rs"]
mod pipe;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn command(source: &Path, output: &Path) -> Command {
    let mut cmd = cli();
    cmd.arg("import-step")
        .arg(source)
        .arg("-o")
        .arg(output)
        .arg("--json");
    cmd
}
fn run(cmd: &mut Command) -> Output {
    cmd.output().expect("real CLI")
}
fn fixture(root: &Path, group: &str, name: &str) -> PathBuf {
    let original = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(group)
        .join(name);
    let path = root.join(name);
    std::fs::copy(original, &path).expect("private STEP copy");
    path
}
fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    std::fs::read_dir(root)
        .expect("directory")
        .map(|e| {
            let path = e.expect("entry").path();
            let bytes = std::fs::read(&path).expect("file; scratch directories must be absent");
            (path, bytes)
        })
        .collect()
}
fn reply(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert_eq!(output.stdout.iter().filter(|c| **c == b'\n').count(), 1);
    let value: Value = serde_json::from_slice(&output.stdout).expect("one standard JSON object");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["operation"], "import-step");
    assert_eq!(value["ok"], exit == 0 || exit == 4);
    if exit == 0 || exit == 4 {
        assert!(value.get("error").is_none());
        assert!(value["result"].is_object());
        assert!(output.stderr.is_empty());
        assert_eq!(
            value["result"]["diagnostics"]
                .as_array()
                .expect("diagnostics")
                .is_empty(),
            exit == 0
        );
    } else {
        assert!(value.get("result").is_none());
        let error = &value["error"];
        assert!(error["kind"].is_string());
        assert!(!error["message"].as_str().expect("message").is_empty());
        assert!(error["causes"].is_array());
        if exit == 5 {
            assert_eq!(error["kind"], "input");
            assert_eq!(error["code"], "reader_rejected");
            assert_eq!(error["causes"], json!([]));
            assert!(error["step_read"].is_object());
            assert!(output.stderr.is_empty());
            for field in [
                "destination",
                "document_id",
                "object_id",
                "source_id",
                "occurrence_id",
            ] {
                assert!(error.get(field).is_none());
                assert!(error["step_read"].get(field).is_none());
            }
        } else {
            assert!(error.get("code").is_none());
            assert!(error.get("step_read").is_none());
        }
    }
    value
}
fn diagnostics(value: &Value, expected: &[Diagnostic]) {
    let array = value.as_array().expect("ordered diagnostics");
    assert_eq!(array.len(), expected.len());
    for (actual, expected) in array.iter().zip(expected) {
        let stage = match expected.stage {
            Stage::Load => "load",
            Stage::Transfer => "transfer",
            Stage::Identity => "identity",
            Stage::Validation => "validation",
            _ => "unknown",
        };
        let severity = match expected.severity {
            Severity::Warning => "warning",
            Severity::Fail => "fail",
            _ => "unknown",
        };
        assert_eq!(
            actual,
            &json!({"stage": stage, "severity": severity, "entity": expected.entity, "message": expected.message})
        );
    }
}
fn saved(path: &Path, bytes: &[u8]) -> (DocumentId, ObjectId, String, ImportedStep) {
    let doc = Document::open_read_only(path).expect("closed, published SQLite");
    let id = doc.meta().document_id;
    let objects = doc.objects().expect("objects");
    assert_eq!(objects.len(), 1);
    let object = &objects[0];
    assert_eq!(object.parent, None);
    assert_eq!(object.ordinal, 0);
    let stored = doc
        .step_import(object.id)
        .expect("stored import")
        .expect("ImportedStep domain");
    assert_eq!(stored.source, bytes);
    assert_eq!(stored.imported.source_hash, ContentHash::of_bytes(bytes));
    let result = (
        id,
        object.id,
        object.name.clone().expect("name"),
        stored.imported,
    );
    doc.close().expect("close");
    result
}
fn published(value: &Value, output: &Path, source: &Path, bytes: &[u8]) -> ImportedStep {
    let (document, object, name, stored) = saved(output, bytes);
    assert_eq!(value["destination"], output.to_str().expect("UTF-8"));
    assert_eq!(value["document_id"], document.to_string());
    assert_eq!(value["object_id"], object.to_string());
    assert_eq!(value["source_id"], stored.source.to_string());
    assert_eq!(value["name"], name);
    assert_eq!(value.get("source_name"), Some(&json!(stored.source_name)));
    assert_eq!(
        stored.source_name.as_deref(),
        source.file_name().and_then(|s| s.to_str())
    );
    assert_eq!(value["source_byte_len"].as_u64(), Some(bytes.len() as u64));
    assert_eq!(value["source_hash"], stored.source_hash.to_string());
    assert_eq!(
        value["importer"],
        json!({"id": stored.imported_by.id, "version": stored.imported_by.version, "build": stored.imported_by.build})
    );
    assert_eq!(value["step_schema"], stored.scene.schema());
    assert_eq!(value["source_unit"], stored.scene.source_unit());
    assert_eq!(
        value["definitions"].as_u64(),
        Some(stored.scene.definition_count() as u64)
    );
    assert_eq!(
        value["placements"].as_u64(),
        Some(stored.scene.instance_count() as u64)
    );
    diagnostics(&value["diagnostics"], &stored.diagnostics_at_import);
    stored
}
fn refusal_pipes(source: &Path, output: &Path, exit: i32, force: bool) {
    let root = output.parent().expect("root");
    for (close_stdout, close_stderr) in [(false, true), (true, false), (true, true)] {
        let before = files(root);
        let mut cmd = command(source, output);
        if force {
            cmd.arg("--force");
        }
        if close_stdout {
            cmd.stdout(pipe::closed_pipe());
        }
        if close_stderr {
            cmd.stderr(pipe::closed_pipe());
        }
        let output = run(&mut cmd);
        if close_stdout {
            assert_eq!(output.status.code(), Some(7), "{output:?}");
        } else {
            reply(&output, exit);
        }
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
        assert_eq!(files(root), before);
    }
}

#[test]
fn native_json_step_import_publication_rejection_and_delivery() {
    if let Err(error) = OcctKernel::new() {
        assert_ne!(
            std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(),
            Ok("1"),
            "{error}"
        );
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        eprintln!("skipped: this build has no Open CASCADE (JSON STEP import)");
        return;
    }
    for (group, file, name, exit) in [
        ("canonical", "01-single-part.step", None, 0),
        (
            "canonical",
            "03-nested-assembly.step",
            Some("組立 \"quoted\"\nline"),
            0,
        ),
        ("canonical", "06-unicode-names.step", Some(""), 0),
        (
            "damaged",
            "02-broken-reference.step",
            Some("注意 \"quoted\"\nline"),
            4,
        ),
    ] {
        let root = tempfile::tempdir().expect("root");
        let source = fixture(root.path(), group, file);
        let bytes = std::fs::read(&source).expect("STEP bytes");
        let mtime = std::fs::metadata(&source)
            .expect("source")
            .modified()
            .expect("mtime");
        let output = root.path().join("JSON document 零.fcad");
        let mut cmd = command(&source, &output);
        if let Some(name) = name {
            cmd.args(["--name", name]);
        }
        let result = reply(&run(&mut cmd), exit);
        let stored = published(&result["result"], &output, &source, &bytes);
        let text_path = root.path().join("text.fcad");
        let mut text = cli();
        text.arg("import-step")
            .arg(&source)
            .arg("-o")
            .arg(&text_path);
        if let Some(name) = name {
            text.args(["--name", name]);
        }
        let text = run(&mut text);
        assert_eq!(text.status.code(), Some(exit), "{text:?}");
        assert!(text.stdout.starts_with(b"imported "));
        assert!(text.stderr.is_empty());
        let (text_id, text_object, text_name, text_stored) = saved(&text_path, &bytes);
        assert_ne!(result["result"]["document_id"], text_id.to_string());
        assert_ne!(result["result"]["object_id"], text_object.to_string());
        assert_eq!(result["result"]["name"], text_name);
        assert_ne!(stored.source, text_stored.source);
        assert_eq!(stored.source_hash, text_stored.source_hash);
        assert_eq!(stored.source_byte_len, text_stored.source_byte_len);
        assert_eq!(stored.source_name, text_stored.source_name);
        assert_eq!(stored.imported_by, text_stored.imported_by);
        assert_eq!(
            stored.diagnostics_at_import,
            text_stored.diagnostics_at_import
        );
        let (StoredScene::V3(scene), StoredScene::V3(mut peer_scene)) =
            (&stored.scene, text_stored.scene)
        else {
            panic!("V3")
        };
        assert_eq!(scene.instances.len(), peer_scene.instances.len());
        for (a, b) in scene.instances.iter().zip(&mut peer_scene.instances) {
            assert_ne!(a.occurrence, b.occurrence);
            b.occurrence = a.occurrence;
        }
        assert_eq!(scene, &peer_scene);
        assert_eq!(std::fs::read(&source).expect("source preserved"), bytes);
        assert_eq!(
            std::fs::metadata(&source)
                .expect("source")
                .modified()
                .expect("mtime"),
            mtime
        );
        assert_eq!(files(root.path()).len(), 3);

        if file == "01-single-part.step" {
            let before = files(root.path());
            let occupied = root.path().join("directory.fcad");
            std::fs::create_dir(&occupied).expect("private occupied directory");
            std::fs::write(occupied.join("keep"), b"foreign destination content").expect("keep");
            let failure = reply(&run(command(&source, &occupied).arg("--force")), 2);
            assert_eq!(failure["error"]["kind"], "io");
            assert_eq!(
                std::fs::read(occupied.join("keep")).expect("preserved content"),
                b"foreign destination content"
            );
            std::fs::remove_file(occupied.join("keep")).expect("private fixture cleanup");
            std::fs::remove_dir(occupied).expect("only the test's empty directory");
            assert_eq!(files(root.path()), before);
        }

        // Small clean/diagnostic fixtures cover every publication pipe case.
        if file == "01-single-part.step" || exit == 4 {
            for force in [false, true] {
                // A normal force reply must also describe the replacement.
                if force {
                    let replacement = root.path().join("normal-force.fcad");
                    std::fs::write(&replacement, b"confirmed old destination").expect("old output");
                    let result = reply(&run(command(&source, &replacement).arg("--force")), exit);
                    published(&result["result"], &replacement, &source, &bytes);
                    std::fs::remove_file(replacement).expect("private output");
                }
                for close_stderr in [false, true] {
                    let destination = root.path().join("delivery.fcad");
                    if force {
                        std::fs::write(&destination, b"confirmed old destination")
                            .expect("old output");
                    }
                    let mut before = files(root.path());
                    let mut cmd = command(&source, &destination);
                    cmd.stdout(pipe::closed_pipe());
                    if force {
                        cmd.arg("--force");
                    }
                    if close_stderr {
                        cmd.stderr(pipe::closed_pipe());
                    }
                    let result = run(&mut cmd);
                    assert_eq!(result.status.code(), Some(7), "{result:?}");
                    assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
                    let (_, _, _, kept) = saved(&destination, &bytes);
                    assert_eq!(kept.diagnostics_at_import, stored.diagnostics_at_import);
                    assert_eq!(
                        kept.scene.definition_count(),
                        stored.scene.definition_count()
                    );
                    before.insert(
                        destination.clone(),
                        std::fs::read(&destination).expect("whole publication"),
                    );
                    assert_eq!(files(root.path()), before);
                    std::fs::remove_file(destination).expect("private output");
                }
            }
        }
    }
    for file in ["01-truncated.step", "06-duplicate-product-definition.step"] {
        let root = tempfile::tempdir().expect("root");
        let source = fixture(root.path(), "damaged", file);
        let bytes = std::fs::read(&source).expect("bytes");
        let destination = root.path().join("rejected.fcad");
        let before = files(root.path());
        let result = reply(&run(&mut command(&source, &destination)), 5);
        let read = &result["error"]["step_read"];
        assert_eq!(
            read["source_hash"],
            ContentHash::of_bytes(&bytes).to_string()
        );
        assert_eq!(read["source_byte_len"].as_u64(), Some(bytes.len() as u64));
        let mut kernel = OcctKernel::new().expect("required kernel");
        let reading = kernel.import_step(&bytes).expect("native typed reading");
        assert!(matches!(
            reading,
            ferritecad_exchange::Import::Rejected { .. }
        ));
        diagnostics(&read["diagnostics"], reading.diagnostics());
        assert_eq!(
            read["importer"],
            json!({"id": kernel.identity().id(), "version": kernel.identity().version(), "build": kernel.identity().build()})
        );
        assert_eq!(kernel.live_shape_count(), 0);
        assert_eq!(files(root.path()), before);
        if file == "01-truncated.step" {
            refusal_pipes(&source, &destination, 5, false);
            std::fs::write(&destination, b"keep despite rejected force").expect("old destination");
            refusal_pipes(&source, &destination, 5, true);
        }
    }
    // The exact mandatory native gate also executes operational/usage/UTF-8
    // refusals. Standalone tests keep those checks runnable in the stub build.
    json_step_preflight_usage_and_stub_delivery_preserve_files();
    #[cfg(unix)]
    json_step_non_utf8_unix_arguments_refuse_before_io();
    #[cfg(windows)]
    json_step_unpaired_utf16_windows_arguments_refuse_before_io();
    println!("FCAD_JSON_STEP_IMPORT published=4 rejected=2 publication_pipes=8 rejection_pipes=6");
}

#[test]
fn json_step_preflight_usage_and_stub_delivery_preserve_files() {
    let root = tempfile::tempdir().expect("root");
    let source = fixture(root.path(), "canonical", "01-single-part.step");
    let destination = root.path().join("new.fcad");
    let missing = root.path().join("missing.step");
    let before = files(root.path());
    let result = reply(&run(&mut command(&missing, &destination)), 2);
    assert_eq!(result["error"]["kind"], "io");
    assert!(
        !result["error"]["causes"]
            .as_array()
            .expect("I/O cause")
            .is_empty()
    );
    assert_eq!(files(root.path()), before);
    refusal_pipes(&missing, &destination, 2, false);
    std::fs::write(&destination, b"old destination").expect("existing");
    refusal_pipes(&missing, &destination, 2, true);
    let before = files(root.path());
    let result = reply(&run(&mut command(&source, &destination)), 2);
    assert_eq!(result["error"]["kind"], "input");
    assert_eq!(files(root.path()), before);
    let alias = root.path().join("hardlink.fcad");
    std::fs::hard_link(&source, &alias).expect("hardlink");
    let aliases = vec![source.clone(), alias];
    #[cfg(unix)]
    let aliases = {
        let mut aliases = aliases;
        let alias = root.path().join("symlink.fcad");
        std::os::unix::fs::symlink(&source, &alias).expect("symlink");
        aliases.push(alias);
        aliases
    };
    let before = files(root.path());
    for alias in aliases {
        for force in [false, true] {
            let mut cmd = command(&source, &alias);
            if force {
                cmd.arg("--force");
            }
            let result = reply(&run(&mut cmd), 2);
            assert_eq!(result["error"]["kind"], "input");
            assert!(
                result["error"]["message"]
                    .as_str()
                    .expect("alias refusal")
                    .contains("STEP file cannot also")
            );
            assert_eq!(files(root.path()), before);
        }
    }
    // One malformed argument per case; valid source and fresh output otherwise.
    let usage_path = root.path().join("usage.fcad");
    let mut unknown_flag = command(&source, &usage_path);
    unknown_flag.arg("--batch");
    let mut missing_name = command(&source, &usage_path);
    missing_name.arg("--name");
    let mut missing_source = cli();
    missing_source
        .args(["import-step", "--json", "-o"])
        .arg(&usage_path);
    let mut missing_output = cli();
    missing_output.args(["import-step", "--json"]).arg(&source);
    let mut unknown_command = cli();
    unknown_command.args(["missing-command", "--json"]);
    for mut cmd in [
        unknown_flag,
        missing_name,
        missing_source,
        missing_output,
        unknown_command,
    ] {
        let result = run(&mut cmd);
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
        assert!(!result.stderr.is_empty());
        assert_eq!(files(root.path()), before);
    }
    let help = run(cli().args(["import-step", "--json", "--help"]));
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--json"));
    assert!(help.stderr.is_empty());
    let version = run(cli().arg("--version"));
    assert!(version.status.success());
    assert!(serde_json::from_slice::<Value>(&version.stdout).is_err());

    let shaped = root.path().join("--json");
    std::fs::copy(&source, &shaped).expect("flag-shaped private source");
    let output = root.path().join("flag-value.fcad");
    let text = run(cli()
        .current_dir(root.path())
        .arg("import-step")
        .arg("-o")
        .arg(&output)
        .args(["--name=--json", "--", "--json"]));
    assert!(
        serde_json::from_slice::<Value>(&text.stdout).is_err(),
        "no accidental JSON mode"
    );
    if OcctKernel::new().is_ok() {
        assert_eq!(text.status.code(), Some(0));
        assert_eq!(
            saved(&output, &std::fs::read(&source).expect("source")).2,
            "--json"
        );
        std::fs::remove_file(&output).expect("private output");
        let result = reply(
            &run(cli()
                .current_dir(root.path())
                .arg("import-step")
                .arg("-o")
                .arg(&output)
                .args(["--json", "--name=--json", "--", "--json"])),
            0,
        );
        assert_eq!(result["result"]["name"], "--json");
        assert_eq!(result["result"]["source_name"], "--json");
    } else {
        assert_eq!(text.status.code(), Some(2));
        assert!(!output.exists());
        let before = files(root.path());
        let result = reply(&run(&mut command(&source, &output)), 2);
        assert_eq!(result["error"]["kind"], "unsupported");
        refusal_pipes(&source, &output, 2, false);
        refusal_pipes(&source, &destination, 2, true);
        assert_eq!(files(root.path()), before);
    }
}

fn invalid_paths(bad: OsString) {
    let root = tempfile::tempdir().expect("root");
    let source = fixture(root.path(), "canonical", "01-single-part.step");
    let output = root.path().join("busy.fcad");
    std::fs::write(&output, b"busy").expect("old output");
    let invalid = root.path().join(bad);
    let before = files(root.path());
    for (source, output) in [(&invalid, &output), (&source, &invalid)] {
        let result = reply(&run(command(source, output).arg("--force")), 2);
        assert_eq!(result["error"]["kind"], "input");
        assert!(
            result["error"]["message"]
                .as_str()
                .expect("UTF-8 refusal")
                .contains("UTF-8")
        );
        assert_eq!(files(root.path()), before);
    }
}
#[cfg(unix)]
#[test]
fn json_step_non_utf8_unix_arguments_refuse_before_io() {
    use std::os::unix::ffi::OsStringExt;
    invalid_paths(OsString::from_vec(b"invalid-\xff.step".to_vec()));
}
#[cfg(windows)]
#[test]
fn json_step_unpaired_utf16_windows_arguments_refuse_before_io() {
    use std::os::windows::ffi::OsStringExt;
    invalid_paths(OsString::from_wide(&[
        b'x' as u16,
        0xd800,
        b'.' as u16,
        b's' as u16,
    ]));
}
