// SPDX-License-Identifier: MIT
//! Public parsing, explicit addressing and no-kernel refusal, through real processes.
use ferritecad_types::ObjectId;
use std::process::Command;

#[test]
fn public_edit_help_and_invalid_requests_never_create_an_output() {
    let help = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .args(["edit-extrude", "--help"])
        .output()
        .expect("help");
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("text");
    for flag in ["--feature", "--distance-mm", "--output", "--expect-version"] {
        assert!(help.contains(flag));
    }
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join("plate.fcad");
    let output = root.path().join("out.fcad");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_ferritecad"))
            .arg("create")
            .arg(&source)
            .arg("--sample")
            .status()
            .expect("create")
            .success()
    );
    let before = std::fs::read(&source).expect("before");
    for feature in ["not-a-uuid".to_owned(), ObjectId::new().to_string()] {
        let result = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
            .arg("edit-extrude")
            .arg(&source)
            .args(["--feature", &feature, "--distance-mm", "27", "-o"])
            .arg(&output)
            .output()
            .expect("edit");
        assert_eq!(result.status.code(), Some(2));
        assert!(!output.exists());
    }
    assert_eq!(std::fs::read(&source).expect("after"), before);
    assert_eq!(std::fs::read_dir(root.path()).expect("dir").count(), 1);
}

#[test]
fn no_kernel_build_refuses_edit_without_touching_source_or_destination() {
    if ferritecad_occt::is_available() {
        return;
    }
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join("plate.fcad");
    let output = root.path().join("out.fcad");
    assert!(
        Command::new(env!("CARGO_BIN_EXE_ferritecad"))
            .arg("create")
            .arg(&source)
            .arg("--sample")
            .status()
            .expect("create")
            .success()
    );
    let source_bytes = std::fs::read(&source).expect("before");
    let doc = ferritecad_document::Document::open_read_only(&source).expect("doc");
    let reading = ferritecad_document::ExtrudeEditSource::read(&doc).expect("reading");
    doc.close().expect("close");
    let result = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg("edit-extrude")
        .arg(&source)
        .args([
            "--feature",
            &reading.features[0].feature.to_string(),
            "--distance-mm",
            "27",
            "-o",
        ])
        .arg(&output)
        .output()
        .expect("edit");
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("Open CASCADE"));
    assert_eq!(std::fs::read(&source).expect("after"), source_bytes);
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(root.path()).expect("dir").count(), 1);
}
