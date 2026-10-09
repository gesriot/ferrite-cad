// SPDX-License-Identifier: MIT
//! §30N: `list-checkpoints`, `create-checkpoint`, `delete-checkpoint` and
//! `extract-checkpoint` on the committed schema v3 plate. No kernel: none of the
//! four needs one. The source file is compared byte for byte after every run.

use std::path::Path;
use std::process::{Command, Output};

use ferritecad_document::Document;
use ferritecad_fixtures::plate_source;
use serde_json::Value;

#[path = "support/pipe.rs"]
mod pipe;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("one JSON document")
}

fn content_of(path: &Path) -> String {
    let document = Document::open_read_only(path).expect("document");
    let content = document.content_version().expect("content").to_string();
    document.close().expect("close");
    content
}

fn schema(path: &Path) -> i64 {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("opens")
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("reads")
}

fn run(args: &[&str], paths: &[&Path]) -> Output {
    let mut command = cli();
    command.args(args);
    for path in paths {
        command.arg(path);
    }
    command.output().expect("runs")
}

#[test]
fn list_create_extract_and_delete_never_write_their_source() {
    let dir = tempfile::tempdir().expect("dir");
    let old = dir.path().join("old.fcad");
    std::fs::copy(plate_source(), &old).expect("the committed plate");
    assert_eq!(schema(&old), 3);
    let original = std::fs::read(&old).expect("bytes");
    let version = content_of(&old);

    let listed = run(&["list-checkpoints", "--json"], &[&old]);
    assert_eq!(listed.status.code(), Some(0), "{listed:?}");
    let value = json(&listed);
    assert_eq!(value["operation"], "list-checkpoints");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["result"]["content_version"], version.as_str());
    assert_eq!(value["result"]["checkpoints"], serde_json::json!([]));

    // --expect-version is required to write.
    let usage = run(
        &["create-checkpoint", "--name", "A", "--output"],
        &[&dir.path().join("x.fcad"), &old],
    );
    assert_eq!(usage.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&usage.stderr).contains("--expect-version"));

    let with_a = dir.path().join("with-a.fcad");
    let created = run(
        &[
            "create-checkpoint",
            "--json",
            "--name",
            " Plate as committed ",
            "--expect-version",
            &version,
            "--output",
        ],
        &[&with_a, &old],
    );
    assert_eq!(created.status.code(), Some(0), "{created:?}");
    let value = json(&created);
    assert_eq!(value["operation"], "create-checkpoint");
    let a = value["result"]["checkpoint"]["checkpoint_id"]
        .as_str()
        .expect("uuid")
        .to_owned();
    assert_eq!(value["result"]["checkpoint"]["name"], "Plate as committed");
    assert_eq!(
        value["result"]["content_version"],
        content_of(&with_a).as_str()
    );
    assert_eq!(
        schema(&with_a),
        4,
        "the copy is migrated, the source is not"
    );
    assert_eq!(schema(&old), 3);

    let listed = json(&run(&["list-checkpoints", "--json"], &[&with_a]));
    let rows = listed["result"]["checkpoints"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["checkpoint_id"], a.as_str());
    assert_eq!(rows[0]["holds_current_model"], true);

    // Text mode says the same in words.
    let text = run(&["list-checkpoints"], &[&with_a]);
    assert!(
        String::from_utf8_lossy(&text.stdout)
            .contains("Plate as committed  (the model as it is now)")
    );

    // Extract: the committed plate's model, every row identity and cell, with an
    // empty catalog; v4 because the image was made from the migrated copy.
    let extracted = dir.path().join("a.fcad");
    let out = run(
        &[
            "extract-checkpoint",
            "--json",
            "--checkpoint",
            &a,
            "--output",
        ],
        &[&extracted, &with_a],
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(json(&out)["operation"], "extract-checkpoint");
    let source = Document::open_read_only(&old).expect("old");
    let image = Document::open_read_only(&extracted).expect("image");
    assert_eq!(
        image.model_without_checkpoints().expect("hash"),
        source.model_without_checkpoints().expect("hash"),
        "the model of the v3 file, in a v4 file"
    );
    assert!(image.checkpoints().expect("lists").is_empty());
    drop((source, image));

    // Refusals: stale version, occupied output, unknown UUID, the source as
    // output. Each leaves every file as it was and prints a structured error.
    let stale = run(
        &[
            "delete-checkpoint",
            "--json",
            "--checkpoint",
            &a,
            "--expect-version",
            &version,
            "--output",
        ],
        &[&dir.path().join("stale.fcad"), &with_a],
    );
    assert_eq!(stale.status.code(), Some(2));
    assert_eq!(json(&stale)["ok"], false);
    assert!(!dir.path().join("stale.fcad").exists());
    let occupied = run(
        &[
            "extract-checkpoint",
            "--json",
            "--checkpoint",
            &a,
            "--output",
        ],
        &[&extracted, &with_a],
    );
    assert_eq!(occupied.status.code(), Some(2));
    assert!(
        json(&occupied)["error"]["message"]
            .as_str()
            .expect("message")
            .contains("already exists")
    );
    let unknown = run(
        &[
            "extract-checkpoint",
            "--checkpoint",
            &ferritecad_types::CheckpointId::new().to_string(),
            "--output",
        ],
        &[&dir.path().join("unknown.fcad"), &with_a],
    );
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("no checkpoint"));
    let itself = run(
        &["extract-checkpoint", "--checkpoint", &a, "--output"],
        &[&with_a, &with_a],
    );
    assert_eq!(itself.status.code(), Some(2));
    let name_usage = run(
        &["extract-checkpoint", "--checkpoint", "Plate", "--output"],
        &[&dir.path().join("n.fcad"), &with_a],
    );
    assert_eq!(
        name_usage.status.code(),
        Some(2),
        "a name is not an identity"
    );

    // Delete with the right version: a copy without A, the model unchanged.
    let with_a_version = content_of(&with_a);
    let without = dir.path().join("without.fcad");
    let deleted = run(
        &[
            "delete-checkpoint",
            "--checkpoint",
            &a,
            "--expect-version",
            &with_a_version,
            "--output",
        ],
        &[&without, &with_a],
    );
    assert_eq!(deleted.status.code(), Some(0), "{deleted:?}");
    assert!(String::from_utf8_lossy(&deleted.stdout).starts_with("deleted checkpoint"));
    assert_eq!(
        json(&run(&["list-checkpoints", "--json"], &[&without]))["result"]["checkpoints"],
        serde_json::json!([])
    );

    assert_eq!(
        std::fs::read(&old).expect("bytes"),
        original,
        "never written"
    );
    assert_eq!(content_of(&with_a), with_a_version, "never written");
}

#[test]
fn a_lost_report_after_publication_exits_7_and_the_copy_stands() {
    let dir = tempfile::tempdir().expect("dir");
    let old = dir.path().join("old.fcad");
    std::fs::copy(plate_source(), &old).expect("the committed plate");
    let version = content_of(&old);
    let output = dir.path().join("delivered-or-not.fcad");
    let status = cli()
        .args([
            "create-checkpoint",
            "--json",
            "--name",
            "A",
            "--expect-version",
            &version,
            "--output",
        ])
        .arg(&output)
        .arg(&old)
        .stdout(pipe::closed_pipe())
        .stderr(pipe::closed_pipe())
        .status()
        .expect("pipes");
    assert_eq!(status.code(), Some(7));
    let listed = ferritecad_jobs::list_checkpoints(&output).expect("published");
    assert_eq!(
        listed.entries.len(),
        1,
        "the copy stands; nothing is repeated"
    );
}
