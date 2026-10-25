// SPDX-License-Identifier: MIT
//! Reporting complete current model names.
#![allow(clippy::panic)]
use ferritecad_fixtures::plate_source;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return false;
    }
    panic!("print-topology failed for another reason: {complaint}");
}

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
                    constraints: Vec::new(),
                }),
            )?;
            Ok(())
        })
        .expect("writes");
    opened.close().expect("closes");
    removed.id
}

#[test]
fn a_whole_document_reports_every_primary_name_in_portable_terms() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }

    let output = print_topology(&document);
    let report = String::from_utf8_lossy(&output.stdout);

    assert!(report.starts_with("topology names\n"));
    for (kind, count) in [("face", 6), ("edge", 12), ("vertex", 8)] {
        let lines: Vec<_> = report
            .lines()
            .filter(|line| line.starts_with(&format!("    {kind} ")))
            .collect();
        assert_eq!(lines.len(), count);
        for line in lines {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields[1].len(), 64);
            assert!(fields[1].bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert_eq!(fields[2], "entities=1");
        }
    }
    for root in [
        "start cap",
        "end cap",
        "curve ",
        "generated Extrude",
        "inputs [",
    ] {
        assert!(report.contains(root), "{report}");
    }

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
fn removing_an_unconsumed_segment_reports_only_current_names() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = plate(dir.path());
    if !has_kernel(&document) {
        return;
    }
    let gone = drop_a_segment(&document);
    let output = print_topology(&document);
    assert!(output.status.success(), "{output:?}");
    let report = String::from_utf8(output.stdout).expect("UTF-8");
    assert!(!report.contains(&gone.to_string()), "{report}");
    assert_eq!(
        report
            .lines()
            .filter(|line| line.starts_with("    face "))
            .count(),
        5
    );
    assert!(report.contains("start cap") && report.contains("end cap"));
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
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
}
