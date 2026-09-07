// SPDX-License-Identifier: MIT
//! What `ferritecad create` leaves on disk, asked of the real process.
//!
//! Driven through the built binary rather than through the shared operation
//! behind it, because what is being checked is a command: its exit status, what
//! it prints, and above all what is in the directory afterwards. None of that
//! is visible from inside a library call, and a gate that called the library
//! twice would go on passing while the command that ships stopped reaching it.
//!
//! # The defect this file exists for
//!
//! Before §24B, `ferritecad create bad.fcad --sample --size NaN 50 12` exited 2
//! with `model values must be finite, found NaN` and left `bad.fcad` behind: an
//! ordinary, openable document with nothing in it. The file was created before
//! the model was built, and rolling back the transaction that failed could not
//! remove a file the transaction never made.
//!
//! Two refusals are gated here rather than one, because a single case would
//! leave the guarantee resting on where a check happens to sit. A width that is
//! not a number is refused before the writing transaction opens; a height that
//! is not a number is refused inside it, after the plane, the profile and the
//! body have already been written. Neither may leave anything at the
//! destination.
//!
//! # And what the command still promises
//!
//! Everything `create` did before it took the shared route: the same flags, the
//! same default size, the same display units, the same `created …` line, the
//! same note for a path that is not a `.fcad`, the same exit codes, and no
//! `--force`. A destination that is already taken is still refused, in the same
//! sentence, and the refusal still names no flag — because there is none.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ferritecad_document::{Document, ObjectPayload, SketchGeometry};

fn ferritecad() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ferritecad"))
}

fn run(args: &[&str]) -> Output {
    Command::new(ferritecad())
        .args(args)
        .output()
        .expect("the command runs")
}

/// Every entry in a directory, sorted, as text.
///
/// Names rather than a count. What a failed creation can leave behind is a
/// document, a private scratch directory or a stray SQLite journal, and a
/// failure message has to be able to say which.
fn entries(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .expect("lists the directory")
        .map(|entry| {
            entry
                .expect("reads an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The failing-first case, run as the command a person types.
///
/// The width never reaches a transaction: it is refused while the rectangle is
/// being described. What must hold is that this makes no difference to the
/// directory, which before §24B it did.
#[test]
fn a_size_that_is_not_a_number_leaves_no_document_behind() {
    let dir = tempfile::tempdir().expect("temp dir");
    let destination = dir.path().join("bad.fcad");

    let output = run(&[
        "create",
        &destination.to_string_lossy(),
        "--sample",
        "--size",
        "NaN",
        "50",
        "12",
    ]);

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("model values must be finite, found NaN"),
        "the refusal does not say what is wrong: {}",
        stderr(&output)
    );
    assert!(
        !destination.exists(),
        "the refused creation left a document behind"
    );
    assert!(
        entries(dir.path()).is_empty(),
        "the refused creation left {:?}",
        entries(dir.path())
    );
}

/// And the same for a refusal that happens after the document exists.
///
/// The height is the extrusion's blind distance, built inside the writing
/// transaction and after three objects have gone into it. This is the case the
/// transaction alone cannot answer: it rolls back, and the database it rolled
/// back inside is still a file at the destination unless something else
/// prevents it.
#[test]
fn a_refusal_after_the_document_exists_leaves_nothing_behind() {
    let dir = tempfile::tempdir().expect("temp dir");
    let destination = dir.path().join("bad.fcad");

    let output = run(&[
        "create",
        &destination.to_string_lossy(),
        "--sample",
        "--size",
        "60",
        "40",
        "NaN",
    ]);

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("model values must be finite, found NaN"),
        "{}",
        stderr(&output)
    );
    assert!(!destination.exists(), "a refused creation left a document");
    assert!(
        entries(dir.path()).is_empty(),
        "the refused creation left {:?}",
        entries(dir.path())
    );
}

/// A refusal the model makes rather than the arithmetic: an extrusion of no
/// height. Also after the document exists, and also publishing nothing.
#[test]
fn a_height_the_model_refuses_leaves_nothing_behind() {
    let dir = tempfile::tempdir().expect("temp dir");
    let destination = dir.path().join("flat.fcad");

    let output = run(&[
        "create",
        &destination.to_string_lossy(),
        "--sample",
        "--size",
        "60",
        "40",
        "0",
    ]);

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("must be positive"),
        "{}",
        stderr(&output)
    );
    assert!(entries(dir.path()).is_empty());
}

#[test]
fn an_empty_document_is_created_and_says_so() {
    let dir = tempfile::tempdir().expect("temp dir");
    let destination = dir.path().join("empty.fcad");

    let output = run(&["create", &destination.to_string_lossy()]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stdout(&output).starts_with(&format!("created {} (", destination.display())),
        "{}",
        stdout(&output)
    );
    assert_eq!(entries(dir.path()), vec!["empty.fcad".to_owned()]);

    let document = Document::open(&destination).expect("opens what was created");
    assert!(document.objects().expect("reads objects").is_empty());
    assert_eq!(document.meta().display_length_unit.symbol(), "mm");
    assert_eq!(document.meta().display_angle_unit.symbol(), "deg");
    document.close().expect("closes");
}

/// The default plate, which the command has always offered without a `--size`.
#[test]
fn the_sample_plate_still_defaults_to_sixty_by_forty_by_ten() {
    let dir = tempfile::tempdir().expect("temp dir");
    let destination = dir.path().join("plate.fcad");

    let output = run(&["create", &destination.to_string_lossy(), "--sample"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));

    assert_eq!(
        corners(&destination),
        vec![(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)]
    );
    let document = Document::open_read_only(&destination).expect("opens the default plate");
    let extrude = document
        .objects()
        .expect("reads objects")
        .into_iter()
        .find_map(|object| match object.payload {
            ObjectPayload::Extrude(extrude) => Some(extrude),
            _ => None,
        })
        .expect("the default plate has an extrusion");
    let ferritecad_document::EndCondition::Blind { distance } = extrude.end_condition else {
        panic!("the default extrusion is not blind");
    };
    assert_eq!(distance.value(), 10.0, "the default height changed");
    document.close().expect("closes the default plate");
}

/// A size that was accepted before is accepted now, and stored in millimetres
/// whatever the document is asked to display.
#[test]
fn a_named_size_is_stored_in_millimetres_whatever_is_displayed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let destination = dir.path().join("plate.fcad");

    let output = run(&[
        "create",
        &destination.to_string_lossy(),
        "--sample",
        "--size",
        "80",
        "50",
        "12",
        "--length-unit",
        "in",
        "--angle-unit",
        "rad",
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));

    assert_eq!(
        corners(&destination),
        vec![(0.0, 0.0), (80.0, 0.0), (80.0, 50.0), (0.0, 50.0)]
    );

    let document = Document::open(&destination).expect("opens it");
    assert_eq!(document.meta().display_length_unit.symbol(), "in");
    assert_eq!(document.meta().display_angle_unit.symbol(), "rad");
    document.close().expect("closes");
}

/// A destination that is already taken is refused, and left exactly as it was.
///
/// The advice names no flag, because `create` has none: a command that told
/// somebody to pass `--force` would be sending them after something that does
/// not exist.
#[test]
fn an_existing_destination_is_refused_and_untouched() {
    let dir = tempfile::tempdir().expect("temp dir");
    let destination = dir.path().join("plate.fcad");
    std::fs::write(&destination, b"somebody else's file").expect("writes the file");

    let output = run(&["create", &destination.to_string_lossy(), "--sample"]);

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("already exists; creating would destroy it"),
        "{}",
        stderr(&output)
    );
    assert!(
        !stderr(&output).contains("--force"),
        "the refusal offers a flag this command does not have: {}",
        stderr(&output)
    );
    assert_eq!(
        std::fs::read(&destination).expect("reads it"),
        b"somebody else's file"
    );
    assert_eq!(entries(dir.path()), vec!["plate.fcad".to_owned()]);
}

/// A path that is not a `.fcad` is still written, and still remarked on.
#[test]
fn a_path_without_the_extension_is_created_with_a_note() {
    let dir = tempfile::tempdir().expect("temp dir");
    let destination = dir.path().join("noext");

    let output = run(&["create", &destination.to_string_lossy()]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("does not end in .fcad"),
        "{}",
        stderr(&output)
    );
    assert!(stdout(&output).starts_with("created "));
    assert_eq!(entries(dir.path()), vec!["noext".to_owned()]);
}

/// Two creations of the same description are two documents.
///
/// Every identifier differs, and that is the contract rather than a defect: a
/// document is a thing, not a value, and two of them made from one description
/// must never be mistaken for one.
#[test]
fn two_creations_of_one_description_have_different_identifiers() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut identifiers = Vec::new();
    for name in ["one.fcad", "two.fcad"] {
        let destination = dir.path().join(name);
        let output = run(&[
            "create",
            &destination.to_string_lossy(),
            "--sample",
            "--size",
            "80",
            "50",
            "12",
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));

        let document = Document::open(&destination).expect("opens it");
        identifiers.push(document.meta().document_id);
        document.close().expect("closes");
    }

    assert_ne!(identifiers[0], identifiers[1]);
    // And the description they were both made from is the same one.
    assert_eq!(
        corners(&dir.path().join("one.fcad")),
        corners(&dir.path().join("two.fcad"))
    );
}

/// The profile's four corners, in the order the rectangle is drawn.
fn corners(document: &Path) -> Vec<(f64, f64)> {
    let opened = Document::open(document).expect("opens the document");
    let objects = opened.objects().expect("reads objects");
    let sketch = objects
        .iter()
        .find(|object| object.name.as_deref() == Some("Profile"))
        .expect("the profile is stored");
    let ObjectPayload::Sketch(profile) = &sketch.payload else {
        panic!("the profile is not a sketch");
    };
    let corners = profile
        .curves
        .iter()
        .map(|curve| match curve.geometry {
            SketchGeometry::Line { start, .. } => (start.x, start.y),
            ref other => panic!("the profile holds {other:?}"),
        })
        .collect();
    opened.close().expect("closes");
    corners
}
