// SPDX-License-Identifier: MIT
//! The first path a person can actually walk: document in, mesh file out.
//!
//! Run against the built binary rather than the functions behind it, because
//! what is being checked is a command — its exit status, what it refuses, and
//! above all what it leaves on disk when it fails. None of that is visible
//! from inside.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ferritecad_fixtures::plate_source;

/// The binary this test was built alongside.
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

/// A private copy of the committed plate, which must never be opened in place.
fn plate(directory: &Path) -> PathBuf {
    let target = directory.join("plate.fcad");
    std::fs::copy(plate_source(), &target).expect("copies the fixture");
    target
}

/// Whether this build has a kernel at all.
fn has_kernel(document: &Path, out: &Path) -> bool {
    let output = run(&[
        "export-stl",
        &document.to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    if output.status.success() {
        return true;
    }
    let complaint = String::from_utf8_lossy(&output.stderr);
    if complaint.contains("no Open CASCADE") {
        eprintln!("skipped: this build has no Open CASCADE");
        return false;
    }
    panic!("export failed for another reason: {complaint}");
}

#[test]
fn the_committed_plate_exports_to_684_bytes_every_time() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    let first = dir.path().join("first.stl");
    if !has_kernel(&document, &first) {
        return;
    }

    assert_eq!(
        std::fs::metadata(&first).expect("the file is there").len(),
        684,
        "a 60 x 40 x 10 plate is twelve triangles"
    );

    // A second export, by a second process, into a different file.
    let second = dir.path().join("second.stl");
    let output = run(&[
        "export-stl",
        &document.to_string_lossy(),
        "-o",
        &second.to_string_lossy(),
    ]);
    assert!(output.status.success());

    assert_eq!(
        std::fs::read(&first).expect("reads"),
        std::fs::read(&second).expect("reads"),
        "the same part must export to the same bytes, run to run"
    );
}

#[test]
fn an_existing_file_is_not_replaced_without_being_asked() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    let out = dir.path().join("plate.stl");
    if !has_kernel(&document, &out) {
        return;
    }

    // Something valuable, where the export wants to go.
    std::fs::write(&out, b"somebody's work").expect("writes");

    let output = run(&[
        "export-stl",
        &document.to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    assert!(!output.status.success(), "it must refuse");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--force"),
        "and say how to proceed"
    );
    assert_eq!(
        std::fs::read(&out).expect("reads"),
        b"somebody's work",
        "the file that was there is still there, byte for byte"
    );

    let output = run(&[
        "export-stl",
        &document.to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
        "--force",
    ]);
    assert!(output.status.success());
    assert_eq!(std::fs::metadata(&out).expect("reads").len(), 684);
}

#[test]
fn a_failed_export_leaves_nothing_behind() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    let probe = dir.path().join("probe.stl");
    if !has_kernel(&document, &probe) {
        return;
    }

    let out = dir.path().join("wanted.stl");
    std::fs::write(&out, b"older export").expect("writes");

    // Names a body this document does not have, so the export fails after the
    // document has been opened and before anything is written.
    let output = run(&[
        "export-stl",
        &document.to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
        "--force",
        "--solid",
        "NoSuchBody",
    ]);
    assert!(!output.status.success());

    assert_eq!(
        std::fs::read(&out).expect("reads"),
        b"older export",
        "a failed export must not disturb the file it was going to replace"
    );
    let leftovers: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .expect("lists")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.to_string_lossy().contains(".partial"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "a partial file survived: {leftovers:?}"
    );
}

#[test]
fn a_deflection_that_makes_no_sense_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    let probe = dir.path().join("probe.stl");
    if !has_kernel(&document, &probe) {
        return;
    }

    let out = dir.path().join("wanted.stl");
    let output = run(&[
        "export-stl",
        &document.to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
        "--linear-deflection",
        "0",
    ]);
    assert!(!output.status.success());
    assert!(!out.exists(), "nothing should have been written");
}

#[test]
fn the_document_is_not_disturbed_by_exporting_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    let out = dir.path().join("plate.stl");
    let before = std::fs::read(&document).expect("reads");
    if !has_kernel(&document, &out) {
        return;
    }

    // A cache sidecar would be regenerable and harmless, but the document
    // itself is the user's data and an export is a read.
    assert_eq!(
        std::fs::read(&document).expect("reads"),
        before,
        "exporting rewrote the document"
    );
}

#[test]
fn several_bodies_are_listed_rather_than_guessed_between() {
    use ferritecad_document::{Body, Dependency, DependencyRole, Document, ObjectPayload};
    use ferritecad_types::ObjectId;

    let dir = tempfile::tempdir().expect("temp dir");
    let path = plate(dir.path());
    let out = dir.path().join("plate.stl");
    if !has_kernel(&path, &out) {
        return;
    }
    std::fs::remove_file(&out).expect("clears the probe's output");

    // A second body over the same feature. Contrived, but the ambiguity it
    // creates is the real one: two things a person could have meant.
    let second = ObjectId::new();
    let mut document = Document::open(&path).expect("opens");
    let tip = document
        .objects()
        .expect("reads")
        .into_iter()
        .find_map(|object| match object.payload {
            ObjectPayload::Body(body) => body.tip_feature,
            _ => None,
        })
        .expect("the plate has a body over a feature");

    document
        .write(|w| {
            w.put_object(
                second,
                None,
                4,
                Some("Second"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(tip),
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: second,
                dependency: tip,
                role: DependencyRole::BodyTip,
            })?;
            Ok(())
        })
        .expect("writes a second body");
    document.close().expect("closes");

    let output = run(&[
        "export-stl",
        &path.to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    assert!(!output.status.success(), "two bodies is not one body");
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(complaint.contains("--solid"), "it must say how to choose");
    assert!(
        complaint.contains("Plate"),
        "and list what there is: {complaint}"
    );
    assert!(complaint.contains("Second"));
    assert!(!out.exists(), "nothing is written while the choice is open");

    // Named, it exports.
    let output = run(&[
        "export-stl",
        &path.to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
        "--solid",
        "Second",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::metadata(&out).expect("reads").len(), 684);
}
