// SPDX-License-Identifier: MIT
//! Read-only validation through the actual CLI, with no native probe or skips.
#![allow(clippy::panic)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Output};
use std::time::SystemTime;

use ferritecad_document::{Document, Envelope, ObjectPayload, StepImportRequest};
use ferritecad_exchange::{Import, Scene};
use ferritecad_jobs::validate_document;
use ferritecad_kernel::KernelIdentity;
use ferritecad_types::ObjectId;
use rusqlite::Connection;
use serde_json::{Value, json};

#[path = "support/pipe.rs"]
mod pipe;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn command(path: &Path, json: bool) -> Command {
    let mut cmd = cli();
    cmd.arg("validate").arg(path);
    if json {
        cmd.arg("--json");
    }
    cmd
}
fn run(cmd: &mut Command) -> Output {
    cmd.output().expect("real CLI process")
}
fn create(path: &Path, sample: bool) {
    let mut cmd = cli();
    cmd.arg("create").arg(path);
    if sample {
        cmd.arg("--sample");
    }
    let output = run(&mut cmd);
    assert!(output.status.success(), "{output:?}");
}
fn sql(path: &Path, statements: &str) {
    Connection::open(path)
        .expect("private test DB")
        .execute_batch(statements)
        .expect("specific corruption");
}
#[derive(Debug, PartialEq)]
struct Files {
    directory_mtime: SystemTime,
    entries: BTreeMap<OsString, (Vec<u8>, SystemTime)>,
}
fn files(root: &Path) -> Files {
    Files {
        directory_mtime: root
            .metadata()
            .expect("directory metadata")
            .modified()
            .expect("mtime"),
        entries: std::fs::read_dir(root)
            .expect("directory")
            .map(|entry| {
                let entry = entry.expect("entry");
                let metadata = entry.metadata().expect("metadata");
                assert!(
                    metadata.is_file(),
                    "unexpected scratch directory: {entry:?}"
                );
                (
                    entry.file_name(),
                    (
                        std::fs::read(entry.path()).expect("bytes"),
                        metadata.modified().expect("mtime"),
                    ),
                )
            })
            .collect(),
    }
}
fn reply(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert_eq!(output.stdout.iter().filter(|b| **b == b'\n').count(), 1);
    let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON object");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["operation"], "validate");
    assert_eq!(value["ok"], exit != 2);
    if exit == 2 {
        assert!(value.get("result").is_none());
        let error = value["error"].as_object().expect("error");
        assert_eq!(error.len(), 3, "no STEP reader rejection fields");
        assert!(error["kind"].is_string());
        assert!(error["message"].is_string());
        assert!(error["causes"].is_array());
        if error["kind"] == "io" {
            assert!(!error["causes"].as_array().expect("causes").is_empty());
        }
    } else {
        assert!(value.get("error").is_none());
        assert_eq!(value["result"]["valid"], exit == 0);
        assert!(output.stderr.is_empty(), "{output:?}");
    }
    value
}

/// Compare ordered domain facts with both presentations, including exact text.
fn check_report(path: &Path, exit: i32) -> Value {
    let root = path.parent().expect("parent");
    let before = files(root);
    let checked = validate_document(path).expect("shared operation completed");
    let output = run(&mut command(path, true));
    let result = reply(&output, exit)["result"].clone();
    let diagnostics: Vec<_> = checked.report.diagnostics.iter().map(|d| json!({
        "code": d.code, "severity": d.severity.as_str(), "message": d.message, "object_id": d.object
    })).collect();
    assert_eq!(
        result,
        json!({
            "document_id": checked.document_id,
            "valid": checked.report.is_ok(),
            "errors": checked.report.errors().count(),
            "warnings": checked.report.warnings().count(),
            "diagnostics": diagnostics
        })
    );
    let mut text = String::new();
    for d in &checked.report.diagnostics {
        let location = d.object.map(|id| format!(" [{id}]")).unwrap_or_default();
        text.push_str(&format!(
            "{}: {}{}: {}\n",
            d.severity.as_str(),
            d.code,
            location,
            d.message
        ));
    }
    let errors = checked.report.errors().count();
    let warnings = checked.report.warnings().count();
    if checked.report.is_ok() {
        text.push_str(&format!(
            "{} is valid ({} warning{})\n",
            path.display(),
            warnings,
            if warnings == 1 { "" } else { "s" }
        ));
    } else {
        text.push_str(&format!(
            "{} has {} error{} and {} warning{}\n",
            path.display(),
            errors,
            if errors == 1 { "" } else { "s" },
            warnings,
            if warnings == 1 { "" } else { "s" }
        ));
    }
    let plain = run(&mut command(path, false));
    assert_eq!(plain.status.code(), Some(exit));
    assert_eq!(plain.stdout, text.as_bytes());
    assert!(plain.stderr.is_empty());
    assert_eq!(
        files(root),
        before,
        "bytes, mtimes, entries and foreign sidecars"
    );
    result
}
fn refusal(path: &Path, kind: &str, reason: &str) {
    let root = path.parent().expect("parent");
    let before = files(root);
    let error = validate_document(path).expect_err("no completed report");
    assert_eq!(error.kind().as_str(), kind);
    assert!(error.to_string().contains(reason), "{error}");
    let json = reply(&run(&mut command(path, true)), 2);
    assert_eq!(json["error"]["kind"], kind);
    assert!(
        json["error"]["message"]
            .as_str()
            .expect("message")
            .contains(reason),
        "{json}"
    );
    let text = run(&mut command(path, false));
    assert_eq!(text.status.code(), Some(2));
    assert!(text.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&text.stderr).contains(reason),
        "{text:?}"
    );
    assert_eq!(files(root), before);
}
fn unknown(path: &Path) -> ObjectId {
    let id = ObjectId::new();
    let mut doc = Document::open(path).expect("fixture writer");
    let bytes = Envelope::new("future.雪\"\n", 9, vec![], vec![1])
        .to_bytes()
        .expect("envelope");
    let payload = ObjectPayload::from_storage_bytes(&bytes).expect("unknown");
    doc.write(|w| w.put_object(id, None, -3, Some("雪 \"quoted\"\nname"), &payload))
        .expect("stored name");
    doc.close().expect("close");
    id
}

#[test]
fn validation_reports_match_shared_operation_without_writes() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("модель 雪 with spaces.fcad");
    create(&path, false);
    std::fs::write(path.with_extension("fcad-cache"), b"foreign cache").expect("sidecar");
    assert_eq!(check_report(&path, 0)["diagnostics"], json!([]));
    let id = unknown(&path);
    let warned = check_report(&path, 0);
    assert_eq!(
        (warned["errors"].as_u64(), warned["warnings"].as_u64()),
        (Some(0), Some(1))
    );
    assert_eq!(warned["diagnostics"][0]["code"], "object.unknown-type");
    assert_eq!(warned["diagnostics"][0]["object_id"], id.to_string());
    assert!(
        warned["diagnostics"][0]["message"]
            .as_str()
            .expect("message")
            .contains("雪")
    );

    let sample = root.path().join("sample.fcad");
    create(&sample, true);
    assert_eq!(check_report(&sample, 0)["diagnostics"], json!([]));
    let doc = Document::open_read_only(&sample).expect("read");
    let extrude = doc
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
        .expect("Extrude")
        .id;
    doc.close().expect("close");
    unknown(&sample); // Repeated diagnostic codes and reader order must survive.
    unknown(&sample);
    sql(
        &sample,
        "DELETE FROM deps WHERE role = 'profile'; UPDATE objects SET payload_hash = zeroblob(32) WHERE kind = 'feature.extrude';",
    );
    let invalid = check_report(&sample, 1);
    let findings = invalid["diagnostics"].as_array().expect("diagnostics");
    for code in ["object.payload-hash-mismatch", "reference.missing-edge"] {
        assert!(
            findings.iter().any(|d| d["code"] == code
                && d["object_id"] == extrude.to_string()
                && d["severity"] == "error"),
            "{invalid}"
        );
    }
    assert_eq!(invalid["warnings"], 2);
    assert_eq!(
        findings
            .iter()
            .filter(|d| d["code"] == "object.unknown-type")
            .count(),
        2
    );
    assert!(invalid["errors"].as_u64().expect("errors") >= 2);

    // Validation concerns persisted facts, so a compact stored import requires
    // no native reader. Damage the owned source bytes after valid storage.
    let imported = root.path().join("imported.fcad");
    let mut doc = Document::create(&imported).expect("create");
    let object = ObjectId::new();
    let import = Import::Imported {
        scene: Scene::default(),
        diagnostics: vec![],
    };
    doc.store_step_import(StepImportRequest {
        object,
        name: Some("stored source"),
        source: b"private source bytes",
        source_name: Some("private.step"),
        import: &import,
        importer: &KernelIdentity::new("test", "1", "").expect("identity"),
    })
    .expect("store facts");
    doc.close().expect("close");
    assert_eq!(check_report(&imported, 0)["errors"], 0);
    sql(
        &imported,
        "UPDATE imported_sources SET bytes = zeroblob(byte_len)",
    );
    let result = check_report(&imported, 1);
    assert!(result["diagnostics"].as_array().expect("findings").iter().any(|d| d["code"] == "imported-source.invalid" && d["object_id"] == object.to_string()));
    sql(
        &imported,
        "DELETE FROM imported_source_refs; DELETE FROM objects;",
    );
    let result = check_report(&imported, 1);
    assert!(
        result["diagnostics"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|d| d["code"] == "imported-source.unreachable" && d["object_id"].is_null())
    );
}

#[test]
fn validation_read_only_refusals_preserve_storage() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("source.fcad");
    refusal(&path, "io", "opening");
    assert!(!path.exists());
    std::fs::write(&path, b"not a SQLite document").expect("broken fixture");
    refusal(&path, "io", "application");
    std::fs::remove_file(&path).expect("private broken copy");
    create(&path, true);
    let clean = std::fs::read(&path).expect("baseline");
    for (corrupt, kind, reason) in [
        // Exact v2 schema: missing v3 import tables, not merely a changed tag.
        (
            "DROP TABLE imported_source_refs; DROP TABLE imported_sources; PRAGMA user_version = 2;",
            "unsupported",
            "needs migration",
        ),
        (
            "UPDATE meta SET minimum_reader_version = 999",
            "unsupported",
            "needs a reader",
        ),
        (
            "UPDATE meta SET format_version = 999",
            "unsupported",
            "document format",
        ),
        ("PRAGMA user_version = 999", "unsupported", "schema"),
        ("PRAGMA application_id = 42", "input", "application"),
        (
            "UPDATE objects SET payload = X'FF' WHERE kind = 'body'",
            "input",
            "decoding object envelope",
        ),
        (
            "UPDATE objects SET kind = 'wrong-kind' WHERE kind = 'body'",
            "input",
            "metadata disagrees",
        ),
        (
            "UPDATE deps SET role = 'future-role'",
            "input",
            "unknown dependency role",
        ),
    ] {
        std::fs::write(&path, &clean).expect("reset private bytes");
        sql(&path, corrupt);
        refusal(&path, kind, reason);
    }
    std::fs::write(&path, &clean).expect("reset private bytes");
    sql(&path, "PRAGMA journal_mode = WAL;");
    for suffix in ["-wal", "-shm", "-journal", "-other"] {
        std::fs::write(
            root.path().join(format!("source.fcad{suffix}")),
            format!("foreign {suffix}"),
        )
        .expect("foreign sidecar");
    }
    refusal(&path, "unsupported", "WAL");
    // The DELETE header alone cannot prevent SQLite from opening a stale
    // WAL and writing SHM, even through SQLITE_OPEN_READ_ONLY.
    std::fs::write(&path, &clean).expect("restore DELETE header");
    std::fs::remove_file(root.path().join("source.fcad-journal")).expect("own journal sentinel");
    refusal(&path, "unsupported", "WAL sidecar");
    // With no WAL sidecars, hot rollback recovery remains an operational I/O
    // refusal. The reader must not repair it or delete another owner's files.
    for suffix in ["-wal", "-shm"] {
        std::fs::remove_file(root.path().join(format!("source.fcad{suffix}")))
            .expect("own sentinels");
    }
    std::fs::write(
        root.path().join("source.fcad-journal"),
        b"foreign hot journal",
    )
    .expect("sentinel");
    refusal(&path, "io", "application id");
}

#[test]
fn validation_json_arguments_and_closed_pipes() {
    let root = tempfile::tempdir().expect("directory");
    let valid = root.path().join("valid.fcad");
    create(&valid, true);
    let invalid = root.path().join("invalid.fcad");
    std::fs::copy(&valid, &invalid).expect("private copy");
    sql(&invalid, "DELETE FROM deps WHERE role = 'profile'");
    let missing = root.path().join("missing.fcad");
    let before = files(root.path());
    for (path, exit) in [(&valid, 0), (&invalid, 1), (&missing, 2)] {
        for (close_out, close_err) in [(true, false), (false, true), (true, true)] {
            let mut cmd = command(path, true);
            if close_out {
                cmd.stdout(pipe::closed_pipe());
            }
            if close_err {
                cmd.stderr(pipe::closed_pipe());
            }
            let output = run(&mut cmd);
            if close_out {
                assert_eq!(output.status.code(), Some(7), "{output:?}");
                assert!(output.stdout.is_empty());
                if !close_err {
                    assert!(
                        String::from_utf8_lossy(&output.stderr).contains("deliver"),
                        "{output:?}"
                    );
                }
            } else {
                reply(&output, exit);
                assert!(output.stderr.is_empty());
            }
            assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
            assert_eq!(files(root.path()), before);
        }
    }
    for args in [
        vec!["validate", "--json"],
        vec!["validate", "--json", "--bad"],
        vec!["--json", "validate"],
        vec!["wrong", "--json"],
    ] {
        let output = run(cli().args(args));
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("error:"));
    }
    for args in [vec!["validate", "--json", "--help"], vec!["--version"]] {
        let output = run(cli().args(args));
        assert!(output.status.success());
        assert!(serde_json::from_slice::<Value>(&output.stdout).is_err());
    }
    let named = root.path().join("--json");
    std::fs::copy(&valid, &named).expect("flag-shaped filename");
    let before = files(root.path());
    let output = run(cli()
        .current_dir(root.path())
        .args(["validate", "--", "--json"]));
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"--json is valid (0 warnings)\n");
    reply(
        &run(cli()
            .current_dir(root.path())
            .args(["validate", "--json", "--", "--json"])),
        0,
    );
    #[cfg(unix)]
    let non_unicode = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(b"invalid-\xff.fcad".to_vec())
    };
    #[cfg(windows)]
    let non_unicode = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0x78, 0xd800, 0x2e, 0x66])
    };
    #[cfg(any(unix, windows))]
    {
        let bad = root.path().join(non_unicode);
        let value = reply(&run(&mut command(&bad, true)), 2);
        assert_eq!(value["error"]["kind"], "input");
        assert!(
            value["error"]["message"]
                .as_str()
                .expect("message")
                .contains("UTF-8")
        );
        assert!(!bad.exists());
        #[cfg(target_os = "linux")]
        {
            std::fs::copy(&valid, &bad).expect("valid document at non-UTF-8 path");
            let bytes = files(root.path());
            assert_eq!(run(&mut command(&bad, false)).status.code(), Some(0));
            reply(&run(&mut command(&bad, true)), 2);
            assert_eq!(files(root.path()), bytes);
            std::fs::remove_file(bad).expect("only private copy");
        }
    }
    // Own non-Unicode fixture creation/removal changes the directory mtime.
    assert_eq!(files(root.path()).entries, before.entries);
}

#[test]
fn validation_really_read_only_permissions() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("read-only.fcad");
    create(&path, true);
    let original = path.metadata().expect("metadata").permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&path, readonly).expect("read-only file");
    // An actual denied write, not an assumption based on chmod in a privileged
    // process. Ordinary product CI runs unprivileged on all three platforms.
    let denied = std::fs::OpenOptions::new().write(true).open(&path);
    if denied.is_ok() {
        std::fs::set_permissions(&path, original).expect("restore private permissions");
        panic!("test requires enforced write denial; privileged chmod is not evidence");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir_mode = root.path().metadata().expect("directory").permissions();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o500))
            .expect("read-only directory");
        let probe = root.path().join("write-probe");
        assert!(std::fs::write(&probe, b"must be denied").is_err());
        check_report(&path, 0);
        std::fs::set_permissions(root.path(), dir_mode).expect("restore private directory");
        let before = files(root.path());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0))
            .expect("unreadable file");
        assert!(std::fs::read(&path).is_err(), "actual read denial");
        let output = run(&mut command(&path, true));
        let error = reply(&output, 2);
        assert_eq!(error["error"]["kind"], "io");
        assert!(
            error["error"]["message"]
                .as_str()
                .expect("message")
                .contains("opening")
        );
        std::fs::set_permissions(&path, original.clone()).expect("restore private file");
        assert_eq!(files(root.path()), before);
    }
    #[cfg(not(unix))]
    check_report(&path, 0);
    std::fs::set_permissions(path, original).expect("restore private permissions");
}
