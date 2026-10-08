// SPDX-License-Identifier: MIT
//! §30M: `list-recovery` and `extract-recovery`, through the library claim the
//! window recovers from. No kernel: the copies are made by the library here.

use std::path::Path;
use std::process::Command;

use ferritecad_document::Document;
use ferritecad_jobs::{
    DocumentSession, HistoryLimits, NewDocument, PlateSize, RecordId, RecoveryStore,
};
use ferritecad_kernel::{OperationContext, mock::MockKernel};
use ferritecad_types::{CadError, Result};
use serde_json::Value;

#[path = "support/pipe.rs"]
mod pipe;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferritecad"));
    command.env_remove(ferritecad_jobs::RECOVERY_DIR_ENV);
    command
}

fn no_kernel() -> Result<MockKernel> {
    Err(CadError::unsupported("no kernel was expected here"))
}

/// An orphaned record holding a new sample plate, and its document.
fn orphan(root: &Path, sessions: &Path) -> (RecordId, DocumentSession) {
    let session = DocumentSession::create_document_in(
        sessions,
        HistoryLimits::default(),
        NewDocument::SamplePlate(PlateSize {
            width: 70.0,
            depth: 40.0,
            height: 13.0,
        }),
        no_kernel,
        &OperationContext::default(),
    )
    .expect("new");
    let store = RecoveryStore::open(root).expect("store");
    let mut record = store.create_record().expect("record");
    record
        .publish(1, &session.current(), "plate.fcad")
        .expect("copy");
    let id = record.id();
    drop(record);
    (id, session)
}

fn json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("one JSON document")
}

fn content_of(path: &Path) -> String {
    let document = Document::open_read_only(path).expect("document");
    let content = document.content_version().expect("content").to_string();
    document.close().expect("close");
    content
}

#[test]
fn list_and_extract_share_the_claim_keep_the_record_and_refuse_occupied_outputs() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let (id, session) = orphan(root.path(), sessions.path());
    let expected = session.current().version();

    let listed = cli()
        .args(["list-recovery", "--json", "--recovery-dir"])
        .arg(root.path())
        .output()
        .expect("list");
    assert_eq!(listed.status.code(), Some(0));
    let value = json(&listed);
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["operation"], "list-recovery");
    assert_eq!(value["ok"], true);
    let records = value["result"]["records"].as_array().expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["state"], "recoverable");
    assert_eq!(records[0]["record"], id.to_string());
    assert_eq!(records[0]["name"], "plate.fcad");
    assert_eq!(records[0]["document_id"], expected.document_id.to_string());
    assert_eq!(records[0]["content_version"], expected.content.to_string());
    assert_eq!(value["result"]["active"], 0);

    let output = user.path().join("recovered.fcad");
    let extracted = cli()
        .args(["extract-recovery", &id.to_string(), "--json", "--output"])
        .arg(&output)
        .arg("--recovery-dir")
        .arg(root.path())
        .output()
        .expect("extract");
    assert_eq!(extracted.status.code(), Some(0), "{extracted:?}");
    let value = json(&extracted);
    assert_eq!(value["operation"], "extract-recovery");
    assert_eq!(value["result"]["record"], id.to_string());
    assert_eq!(
        value["result"]["content_version"],
        expected.content.to_string()
    );
    assert_eq!(content_of(&output), expected.content.to_string());

    // Occupied: a structured refusal, the file as it was.
    let before = std::fs::read(&output).expect("bytes");
    let refused = cli()
        .args(["extract-recovery", &id.to_string(), "--json", "--output"])
        .arg(&output)
        .arg("--recovery-dir")
        .arg(root.path())
        .output()
        .expect("extract");
    assert_eq!(refused.status.code(), Some(2));
    let value = json(&refused);
    assert_eq!(value["ok"], false);
    assert!(
        value["error"]["message"]
            .as_str()
            .expect("message")
            .contains("already exists")
    );
    assert!(value["error"].get("recovery_refusal").is_none());
    assert_eq!(std::fs::read(&output).expect("bytes"), before);

    // Inside the recovery folder: refused, nothing written there.
    let inside = root.path().join("inside.fcad");
    let refused = cli()
        .args(["extract-recovery", &id.to_string(), "--json", "--output"])
        .arg(&inside)
        .arg("--recovery-dir")
        .arg(root.path())
        .output()
        .expect("extract");
    assert_eq!(refused.status.code(), Some(2));
    assert!(!inside.exists());

    // The record is still there: extraction never takes it over.
    let store = RecoveryStore::at(root.path()).expect("store");
    assert_eq!(store.list().expect("list").recoverable().count(), 1);

    // Text mode reads the same listing.
    let text = cli()
        .args(["list-recovery", "--recovery-dir"])
        .arg(root.path())
        .output()
        .expect("list");
    assert_eq!(text.status.code(), Some(0));
    let text = String::from_utf8(text.stdout).expect("text");
    assert!(text.contains(&format!("{id}  recoverable")), "{text}");
    assert!(text.contains("plate.fcad"), "{text}");
}

#[test]
fn active_damaged_and_unknown_records_are_refused_as_data() {
    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let (held, _) = orphan(root.path(), sessions.path());
    let (damaged, _) = orphan(root.path(), sessions.path());
    let copy = root.path().join(format!("r-{damaged}")).join("c1.fcad");
    let mut bytes = std::fs::read(&copy).expect("copy");
    let last = bytes.len() - 1;
    bytes[last] ^= 0x55;
    std::fs::write(&copy, bytes).expect("flip");

    let store = RecoveryStore::at(root.path()).expect("store");
    let claim = store.claim(held).expect("held by this process");
    let refusal = |id: String| {
        let output = cli()
            .args(["extract-recovery", &id, "--json", "--output"])
            .arg(user.path().join(format!("{id}.fcad")))
            .arg("--recovery-dir")
            .arg(root.path())
            .output()
            .expect("extract");
        assert_eq!(output.status.code(), Some(2));
        let value = json(&output);
        assert_eq!(value["ok"], false);
        value["error"]["recovery_refusal"]
            .as_str()
            .map(str::to_owned)
    };
    assert_eq!(refusal(held.to_string()).as_deref(), Some("active"));
    assert_eq!(refusal(damaged.to_string()).as_deref(), Some("mismatch"));
    assert_eq!(
        refusal("01a11bec-7115-7200-89eb-ad517d927abd".to_owned()).as_deref(),
        Some("not-found")
    );
    assert_eq!(std::fs::read_dir(user.path()).expect("user").count(), 0);

    let listed = cli()
        .args(["list-recovery", "--json", "--recovery-dir"])
        .arg(root.path())
        .output()
        .expect("list");
    let value = json(&listed);
    assert_eq!(value["result"]["active"], 1);
    let records = value["result"]["records"].as_array().expect("records");
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["state"], "refused");
    assert_eq!(records[0]["reason"], "mismatch");
    drop(claim);
}

#[test]
fn a_missing_folder_lists_nothing_and_is_not_created_and_the_variable_names_the_folder() {
    let parent = tempfile::tempdir().expect("parent");
    let missing = parent.path().join("never-made");
    let listed = cli()
        .args(["list-recovery", "--json", "--recovery-dir"])
        .arg(&missing)
        .output()
        .expect("list");
    assert_eq!(listed.status.code(), Some(0));
    assert_eq!(json(&listed)["result"]["records"], serde_json::json!([]));
    assert!(!missing.exists(), "listing created the folder");

    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let (id, _) = orphan(root.path(), sessions.path());
    let listed = cli()
        .env(ferritecad_jobs::RECOVERY_DIR_ENV, root.path())
        .args(["list-recovery", "--json"])
        .output()
        .expect("list");
    let value = json(&listed);
    assert_eq!(value["result"]["records"][0]["record"], id.to_string());
}

#[test]
fn usage_stays_text_and_a_lost_report_after_extraction_exits_7_and_keeps_the_file() {
    let usage = cli()
        .args(["extract-recovery", "--json"])
        .output()
        .expect("usage");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty(), "usage stays clap text");
    let usage = cli()
        .args([
            "extract-recovery",
            "not-a-uuid",
            "--json",
            "--output",
            "x.fcad",
        ])
        .output()
        .expect("usage");
    assert_eq!(usage.status.code(), Some(2));
    assert!(
        usage.stdout.is_empty(),
        "a malformed record is a usage error"
    );

    let root = tempfile::tempdir().expect("root");
    let sessions = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let (id, session) = orphan(root.path(), sessions.path());
    let output = user.path().join("delivered-or-not.fcad");
    let status = cli()
        .args(["extract-recovery", &id.to_string(), "--json", "--output"])
        .arg(&output)
        .arg("--recovery-dir")
        .arg(root.path())
        .stdout(pipe::closed_pipe())
        .stderr(pipe::closed_pipe())
        .status()
        .expect("pipes");
    assert_eq!(status.code(), Some(7));
    assert_eq!(
        content_of(&output),
        session.current().version().content.to_string(),
        "the extraction stands"
    );
    let store = RecoveryStore::at(root.path()).expect("store");
    assert_eq!(store.list().expect("list").recoverable().count(), 1);
}
