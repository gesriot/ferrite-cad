// SPDX-License-Identifier: MIT
//! Rebuilding a document from the command line, and changing nothing by it.
//!
//! A diagnostic that alters what it is diagnosing is worse than none, so most
//! of what these check is absence: no sidecar appears, no byte of the document
//! moves, and two runs say the same thing. Driven through the built binary,
//! because "left the directory as it found it" is not a property a function
//! can be asked about.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ferritecad_fixtures::plate_source;

fn ferritecad() -> PathBuf {
    // `current_exe` is target/<profile>/deps/<test>; the binary is two up.
    let mut path = std::env::current_exe().expect("the test knows where it is");
    path.pop();
    path.pop();
    path.push(format!("ferritecad{}", std::env::consts::EXE_SUFFIX));
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(ferritecad())
        .args(args)
        .output()
        .expect("the command runs")
}

fn plate(directory: &Path) -> PathBuf {
    let target = directory.join("plate.fcad");
    std::fs::copy(plate_source(), &target).expect("copies the fixture");
    target
}

fn rebuild(document: &Path) -> Output {
    run(&["rebuild", &document.to_string_lossy(), "--cold"])
}

/// Every file in a directory, by name and contents.
fn snapshot(directory: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    std::fs::read_dir(directory)
        .expect("lists")
        .filter_map(|entry| entry.ok())
        .map(|entry| {
            let path = entry.path();
            let bytes = std::fs::read(&path).unwrap_or_default();
            (path, bytes)
        })
        .collect()
}

/// Whether this build has a kernel, skipping the caller if it does not.
fn has_kernel(document: &Path) -> bool {
    let output = rebuild(document);
    if output.status.success() {
        return true;
    }
    let complaint = String::from_utf8_lossy(&output.stderr);
    if complaint.contains("no Open CASCADE") {
        eprintln!("skipped: this build has no Open CASCADE");
        return false;
    }
    panic!("rebuild failed for another reason: {complaint}");
}

#[test]
fn a_cold_rebuild_reports_what_the_document_produced() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    let output = rebuild(&document);
    let report = String::from_utf8_lossy(&output.stdout);

    assert!(
        report.contains("4 objects evaluated, 1 shape built"),
        "{report}"
    );
    assert!(report.contains("solid, 6 named faces"), "{report}");
    assert!(report.contains("4 segments"), "{report}");
    assert!(
        report.contains("6 of 6 stored references resolved"),
        "{report}"
    );
}

#[test]
fn two_rebuilds_of_one_document_say_exactly_the_same_thing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    let once = rebuild(&document);
    let twice = rebuild(&document);
    assert_eq!(
        String::from_utf8_lossy(&once.stdout),
        String::from_utf8_lossy(&twice.stdout),
        "a report that differs between runs cannot be compared, and comparing \
         reports is the whole use of this command"
    );

    // The most likely way that could fail is an elapsed time.
    let report = String::from_utf8_lossy(&once.stdout);
    for unit in ["ms", "µs", "seconds", "elapsed", "took"] {
        assert!(
            !report.contains(unit),
            "the report mentions {unit}: {report}"
        );
    }
}

#[test]
fn rebuilding_writes_nothing_at_all() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    let before = snapshot(dir.path());
    assert!(rebuild(&document).status.success());
    let after = snapshot(dir.path());

    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "a cold rebuild must not leave a cache sidecar, or anything else, behind"
    );
    assert_eq!(before, after, "the document was rewritten by reading it");
}

#[test]
fn an_existing_sidecar_is_neither_read_nor_touched() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    // Not a real sidecar: if the command opened it, it would fail on the
    // application id, and if it replaced it, these bytes would be gone.
    let sidecar = dir.path().join("plate.fcad-cache");
    std::fs::write(&sidecar, b"not a sidecar, and not yours to touch").expect("writes");

    assert!(
        rebuild(&document).status.success(),
        "the sidecar is not consulted"
    );
    assert_eq!(
        std::fs::read(&sidecar).expect("reads"),
        b"not a sidecar, and not yours to touch"
    );
}

#[test]
fn a_rebuild_without_cold_refuses_rather_than_choosing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());

    let output = run(&["rebuild", &document.to_string_lossy()]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--cold"),
        "it must say what is missing"
    );
}

#[test]
fn a_document_that_cannot_be_built_fails_and_says_why() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    // A body whose tip feature does not exist: valid enough to store, not
    // enough to build.
    use ferritecad_document::{Body, Document, ObjectPayload};
    use ferritecad_types::ObjectId;
    let mut opened = Document::open(&document).expect("opens");
    let body = opened
        .objects()
        .expect("reads")
        .into_iter()
        .find(|object| matches!(object.payload, ObjectPayload::Body(_)))
        .expect("the plate has a body");
    opened
        .write(|w| {
            w.put_object(
                body.id,
                body.parent,
                body.ordinal,
                body.name.as_deref(),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(ObjectId::new()),
                }),
            )?;
            Ok(())
        })
        .expect("writes");
    opened.close().expect("closes");

    let output = rebuild(&document);
    assert!(!output.status.success());
    assert!(
        !String::from_utf8_lossy(&output.stderr).is_empty(),
        "a failure with no message is not a failure a person can act on"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).is_empty(),
        "half a report is worse than none"
    );
}

#[test]
fn a_missing_document_is_an_error_not_an_empty_report() {
    let dir = tempfile::tempdir().expect("temp dir");
    let output = run(&[
        "rebuild",
        &dir.path().join("nothing.fcad").to_string_lossy(),
        "--cold",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
}
