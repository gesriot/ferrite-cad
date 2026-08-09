// SPDX-License-Identifier: MIT
//! Reporting what a document's stored names still find.
//!
//! The report exists for the moment a reference stops resolving, so most of
//! what is checked here is that moment: every broken name is listed, not the
//! first, and the command says so in its exit status without refusing to
//! finish the report.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ferritecad_fixtures::plate_source;

/// The exit status for a document whose names no longer all resolve.
const UNRESOLVED: i32 = 3;

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

fn print_topology(document: &Path) -> Output {
    run(&["print-topology", &document.to_string_lossy()])
}

fn has_kernel(document: &Path) -> bool {
    let output = print_topology(document);
    if output.status.success() {
        return true;
    }
    let complaint = String::from_utf8_lossy(&output.stderr);
    if complaint.contains("no Open CASCADE") {
        eprintln!("skipped: this build has no Open CASCADE");
        return false;
    }
    panic!("print-topology failed for another reason: {complaint}");
}

/// Removes the last sketch segment, closing the loop over the gap.
///
/// The reference to the segment that went is the one that must be reported
/// lost; the rest must not be.
fn drop_a_segment(document: &Path) -> ferritecad_types::StableEntityId {
    use ferritecad_document::{Document, ObjectPayload, Sketch, SketchGeometry};

    let mut opened = Document::open(document).expect("opens");
    let record = opened
        .objects()
        .expect("reads")
        .into_iter()
        .find(|object| matches!(object.payload, ObjectPayload::Sketch(_)))
        .expect("the plate has a sketch");
    let ObjectPayload::Sketch(mut sketch) = record.payload.clone() else {
        panic!("the sketch is not a sketch");
    };

    let removed = sketch.curves.pop().expect("there are curves");
    let SketchGeometry::Line { end, .. } = removed.geometry else {
        panic!("the fixture's segments are lines");
    };
    let previous = sketch.curves.last_mut().expect("three are left");
    let SketchGeometry::Line { start, .. } = previous.geometry else {
        panic!("the fixture's segments are lines");
    };
    previous.geometry = SketchGeometry::Line { start, end };

    let curves = sketch.curves.clone();
    opened
        .write(|w| {
            w.put_object(
                record.id,
                record.parent,
                record.ordinal,
                record.name.as_deref(),
                &ObjectPayload::Sketch(Sketch {
                    plane: sketch.plane,
                    curves,
                }),
            )?;
            Ok(())
        })
        .expect("writes");
    opened.close().expect("closes");
    removed.id
}

#[test]
fn a_whole_document_reports_every_reference_in_portable_terms() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    let output = print_topology(&document);
    let report = String::from_utf8_lossy(&output.stdout);

    assert!(report.contains("6 references"), "{report}");
    assert!(report.contains("extrude cap start"), "{report}");
    assert!(report.contains("extrude cap end"), "{report}");
    assert!(report.contains("all derived from"), "{report}");
    assert!(report.contains("expects    face"), "{report}");
    assert!(report.contains("6 of 6 references resolved"), "{report}");

    // Nothing session-local may appear. A handle prints as `shape 3 face 5`
    // and a slot as a bare number under a name; neither belongs in a document
    // report, and a reader who saw one might come to rely on it.
    for leak in ["shape ", "slot", "face index", "session"] {
        assert!(
            !report.contains(leak),
            "the report leaks `{leak}`: {report}"
        );
    }
}

#[test]
fn two_runs_report_the_same_thing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    assert_eq!(
        String::from_utf8_lossy(&print_topology(&document).stdout),
        String::from_utf8_lossy(&print_topology(&document).stdout)
    );
}

#[test]
fn a_lost_reference_is_reported_and_changes_the_exit_status() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    let gone = drop_a_segment(&document);
    let output = print_topology(&document);
    let report = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(UNRESOLVED),
        "a lost name is neither success nor a command that could not run"
    );
    assert!(report.contains("5 of 6 references resolved"), "{report}");
    assert!(
        report.contains(&format!("extrude side from segment {gone}")),
        "the lost reference must be named: {report}"
    );

    // The report is finished, not abandoned at the first failure. Counted
    // over the per-reference status lines, which end in two spaces and a
    // word; the closing summary ends in one space and "resolved".
    assert_eq!(status_lines(&report, "resolved"), 5, "{report}");
    assert_eq!(status_lines(&report, "lost"), 1, "{report}");
}

#[test]
fn every_lost_reference_is_listed_not_only_the_first() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    // Two segments gone, so two references have nothing left to name.
    let first = drop_a_segment(&document);
    let second = drop_a_segment(&document);

    let output = print_topology(&document);
    let report = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(UNRESOLVED));
    assert!(report.contains("4 of 6 references resolved"), "{report}");

    for segment in [first, second] {
        assert!(
            report.contains(&format!("extrude side from segment {segment}")),
            "stopping at the first lost name hides how much broke: {report}"
        );
    }
}

#[test]
fn reporting_on_a_document_changes_nothing_about_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    let before: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .expect("lists")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    let bytes = std::fs::read(&document).expect("reads");

    assert!(print_topology(&document).status.success());

    let after: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .expect("lists")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    assert_eq!(before, after, "something was written beside the document");
    assert_eq!(
        std::fs::read(&document).expect("reads"),
        bytes,
        "the document was rewritten by reporting on it"
    );
}

#[test]
fn a_missing_document_is_an_error_not_an_empty_report() {
    let dir = tempfile::tempdir().expect("temp dir");
    let output = run(&[
        "print-topology",
        &dir.path().join("nothing.fcad").to_string_lossy(),
    ]);
    assert!(!output.status.success());
    assert_ne!(
        output.status.code(),
        Some(UNRESOLVED),
        "a file that is not there is not a document with a lost name"
    );
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
}

/// How many references were reported with this status.
fn status_lines(report: &str, status: &str) -> usize {
    let suffix = format!("  {status}");
    report
        .lines()
        .filter(|line| line.ends_with(&suffix))
        .count()
}
