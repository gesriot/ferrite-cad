// SPDX-License-Identifier: MIT
//! Importing a STEP file from the command line.
//!
//! Driven through the built binary, because most of what matters here is not a
//! property a function can be asked about: whether the input file came back
//! unchanged, whether a refused import left anything in the directory, whether
//! the destination ever existed in a half-written state.
//!
//! What the exit codes mean is a contract too. A script has to be able to tell
//! "there is a document and the reader said nothing" from "there is a document
//! and the reader said things" from "there is no document" without reading
//! prose, and none of the three may be described as the file being correct.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ferritecad_document::{Document, ObjectPayload};
use ferritecad_exchange::{Stage, StoredScene};

const CLEAN: i32 = 0;
const FAILED: i32 = 2;
const NOTICED: i32 = 4;
const REJECTED: i32 = 5;

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

fn corpus(kind: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(kind)
        .join(name)
}

/// Copies one corpus file into a directory the test owns.
///
/// The committed corpus is never handed to the command directly. The whole
/// claim being tested is that the input is not written to, and proving it on a
/// copy costs nothing while proving it on the checkout risks the corpus.
fn source(directory: &Path, kind: &str, name: &str) -> PathBuf {
    let target = directory.join(name);
    std::fs::copy(corpus(kind, name), &target).expect("copies the fixture");
    target
}

fn import(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        "import-step",
        &*input.to_string_lossy().into_owned().leak(),
        "-o",
        &*output.to_string_lossy().into_owned().leak(),
    ];
    args.extend_from_slice(extra);
    run(&args)
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("the command exited normally")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
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
fn has_kernel(directory: &Path) -> bool {
    let input = source(directory, "canonical", "01-single-part.step");
    let output = directory.join("probe.fcad");
    let result = import(&input, &output, &[]);
    if result.status.success() {
        std::fs::remove_file(&output).expect("removes the probe");
        std::fs::remove_file(&input).expect("removes the probe input");
        return true;
    }
    let complaint = String::from_utf8_lossy(&result.stderr);
    if complaint.contains("no Open CASCADE") {
        eprintln!("skipped: this build has no Open CASCADE");
        return false;
    }
    panic!("importing failed for another reason: {complaint}");
}

#[test]
fn a_sound_file_becomes_a_document_and_is_not_called_correct() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    let input = source(dir.path(), "canonical", "04-instance-colours.step");
    let output = dir.path().join("bolts.fcad");
    let result = import(&input, &output, &[]);

    assert_eq!(code(&result), CLEAN, "{}", stdout(&result));
    assert!(output.is_file(), "no document was written");

    let report = stdout(&result);
    assert!(report.contains("2 definitions, 5 placements"), "{report}");
    assert!(report.contains("blake3 "), "the source digest is reported");
    assert!(
        report.contains("nothing was reported while reading it"),
        "{report}"
    );

    // The one sentence this command must never produce, in any of its forms.
    for claim in [
        "is valid",
        "is correct",
        "is sound",
        "no problems",
        "file is fine",
    ] {
        assert!(
            !report.contains(claim),
            "the report claims {claim:?}, which nothing here established:\n{report}"
        );
    }
    // And it says what its own silence is worth.
    assert!(
        report.contains("describes this reader, not the file"),
        "{report}"
    );
}

#[test]
fn the_step_file_is_read_and_never_written_to() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    let input = source(dir.path(), "canonical", "03-nested-assembly.step");
    let before = std::fs::read(&input).expect("reads");
    let modified_before = std::fs::metadata(&input).expect("stats").modified().ok();

    let result = import(&input, &dir.path().join("nested.fcad"), &[]);
    assert_eq!(code(&result), CLEAN, "{}", stdout(&result));

    assert_eq!(
        std::fs::read(&input).expect("reads"),
        before,
        "the STEP file was written to"
    );
    assert_eq!(
        std::fs::metadata(&input).expect("stats").modified().ok(),
        modified_before,
        "the STEP file's modification time moved"
    );
}

#[test]
fn the_source_bytes_are_in_the_document_and_the_file_is_no_longer_needed() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    let input = source(dir.path(), "canonical", "06-unicode-names.step");
    let original = std::fs::read(&input).expect("reads");
    let output = dir.path().join("unicode.fcad");
    assert_eq!(code(&import(&input, &output, &[])), CLEAN);

    // The document is the only copy from here on, which is the point of
    // storing the bytes rather than a path to them.
    std::fs::remove_file(&input).expect("removes the original");

    let conn = rusqlite::Connection::open(&output).expect("opens raw");
    let (bytes, byte_len, format): (Vec<u8>, i64, String) = conn
        .query_row(
            "SELECT bytes, byte_len, format FROM imported_sources",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("one source row");
    conn.close().expect("closes");

    assert_eq!(bytes, original, "the stored bytes are not the file's");
    assert_eq!(byte_len as usize, original.len());
    assert_eq!(format, ferritecad_document::STEP_SOURCE_FORMAT);

    // And the document still reads as a document with the file gone.
    let inspected = run(&["inspect", &output.to_string_lossy()]);
    assert!(inspected.status.success());
    assert!(stdout(&inspected).contains("exchange.step.imported"));
}

#[test]
fn the_complex_ap203_assembly_becomes_a_durable_document() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    let input = source(
        dir.path(),
        "interoperability",
        "c3d-ap203-complex-assembly.stp",
    );
    let original = std::fs::read(&input).expect("reads the interoperability fixture");
    let modified_before = std::fs::metadata(&input).expect("stats").modified().ok();
    let output = dir.path().join("complex.fcad");

    let result = import(&input, &output, &[]);
    assert_eq!(
        code(&result),
        NOTICED,
        "the transferred AP203 scene must be published with its diagnostics: {}{}",
        stdout(&result),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.is_file(), "no durable document was written");

    let report = stdout(&result);
    // The persisted scene contains one unplaced root node and 139 occurrences
    // placed below it. The CLI's long-standing summary calls all scene nodes
    // placements, so its total is 140.
    assert!(
        report.contains("46 definitions, 140 placements"),
        "{report}"
    );
    assert!(report.contains("reading reported"), "{report}");
    assert!(
        report.contains("problem while validating step.product_definition#"),
        "invalid solids were not surfaced as typed diagnostics:\n{report}"
    );
    assert_eq!(
        std::fs::read(&input).expect("reads the fixture again"),
        original,
        "the interoperability STEP bytes changed"
    );
    assert_eq!(
        std::fs::metadata(&input)
            .expect("stats again")
            .modified()
            .ok(),
        modified_before,
        "the interoperability STEP mtime changed"
    );

    std::fs::remove_file(&input).expect("hides the external STEP source");

    let document = Document::open(&output).expect("opens the standalone document");
    let objects = document.objects().expect("reads the imported object");
    let imported = objects
        .iter()
        .find_map(|record| match &record.payload {
            ObjectPayload::ImportedStep(imported) => Some(imported),
            _ => None,
        })
        .expect("the document carries an imported STEP object");
    let StoredScene::V3(scene) = &imported.scene else {
        panic!("a new import must carry source definition keys and placement identities");
    };
    assert!(
        imported.diagnostics_at_import.iter().any(|diagnostic| {
            diagnostic.stage == Stage::Load
                && diagnostic.entity == "FILL_AREA_STYLE"
                && diagnostic.message.contains("180 times")
        }),
        "the 180 grouped FILL_AREA_STYLE problems were hidden: {:?}",
        imported.diagnostics_at_import
    );
    let mut validation_entities: Vec<&str> = imported
        .diagnostics_at_import
        .iter()
        .filter(|diagnostic| diagnostic.stage == Stage::Validation)
        .map(|diagnostic| diagnostic.entity.as_str())
        .collect();
    validation_entities.sort_unstable();
    assert_eq!(
        validation_entities,
        [
            "step.product_definition#2428",
            "step.product_definition#2583"
        ],
        "invalid part solids must be retained and named, without assembly duplicates"
    );
    assert_eq!(scene.definitions.len(), 46);
    assert_eq!(scene.instances.len(), 140);
    assert_eq!(
        scene
            .instances
            .iter()
            .map(|instance| instance.occurrence)
            .collect::<BTreeSet<_>>()
            .len(),
        140,
        "a placement identity collided or went missing"
    );
    assert_eq!(
        scene
            .instances
            .iter()
            .filter(|instance| instance.parent.is_none())
            .count(),
        1,
        "the root assembly was lost"
    );
    assert_eq!(
        scene
            .instances
            .iter()
            .find(|instance| instance.parent.is_none())
            .expect("the root exists")
            .definition,
        "step.product_definition#1",
        "the root was assigned another definition identity"
    );
    assert_eq!(
        scene
            .instances
            .iter()
            .filter(|instance| instance.parent.is_some())
            .count(),
        139,
        "a placed occurrence was lost or invented"
    );

    let keys: BTreeMap<&str, usize> = scene
        .definitions
        .iter()
        .enumerate()
        .map(|(index, definition)| (definition.key.as_str(), index))
        .collect();
    assert_eq!(keys.len(), 46, "definition keys are not unique");
    for required in [
        "step.product_definition#1",
        "step.product_definition#1764",
        "step.product_definition#2927",
    ] {
        assert!(keys.contains_key(required), "missing {required}");
    }

    let repeated: Vec<_> = scene
        .instances
        .iter()
        .filter(|instance| instance.definition == "step.product_definition#2753")
        .collect();
    assert_eq!(repeated.len(), 8, "the repeated definition was not shared");
    // §22B-1e3a: eight places, eight identities. They share a definition and
    // nothing else the document could tell them apart by.
    assert_eq!(
        repeated
            .iter()
            .map(|instance| instance.occurrence)
            .collect::<BTreeSet<_>>()
            .len(),
        8,
        "the eight placements of one definition do not have eight identities"
    );
    assert!(
        repeated
            .iter()
            .skip(1)
            .any(|instance| instance.placement != repeated[0].placement),
        "repeated instances incorrectly share one placement"
    );

    let direct_children = |parent: usize| -> Vec<&str> {
        let mut children: Vec<&str> = scene
            .instances
            .iter()
            .filter(|instance| instance.parent == Some(parent as u32))
            .map(|instance| instance.definition.as_str())
            .collect();
        children.sort_unstable();
        children
    };
    let first_assembly = scene
        .instances
        .iter()
        .position(|instance| instance.definition == "step.product_definition#1764")
        .expect("places the first equal-children assembly");
    let second_assembly = scene
        .instances
        .iter()
        .position(|instance| instance.definition == "step.product_definition#2927")
        .expect("places the second equal-children assembly");
    assert_eq!(
        direct_children(first_assembly),
        direct_children(second_assembly),
        "the measurement no longer contains the equal-children pair"
    );

    let validated = run(&["validate", &output.to_string_lossy()]);
    assert!(
        validated.status.success(),
        "the document does not stand alone: {}{}",
        stdout(&validated),
        String::from_utf8_lossy(&validated.stderr)
    );
}

#[test]
fn a_file_that_imports_with_problems_is_distinguished_from_a_clean_one() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    // Read completely, and not silently. Measured on 8.0.1: this one imports
    // and reports why it should not be trusted.
    let input = source(dir.path(), "damaged", "02-broken-reference.step");
    let output = dir.path().join("broken.fcad");
    let result = import(&input, &output, &[]);

    assert_eq!(
        code(&result),
        NOTICED,
        "an import with diagnostics is neither a clean import nor a failure: {}",
        stdout(&result)
    );
    assert!(
        output.is_file(),
        "the document is complete and must have been written"
    );

    let report = stdout(&result);
    assert!(report.starts_with("imported "), "{report}");
    assert!(report.contains("reading reported"), "{report}");
    assert!(report.to_lowercase().contains("unresolved"), "{report}");

    // The document keeps what was said, so it can be read again later.
    let inspected = run(&["inspect", &output.to_string_lossy()]);
    assert!(
        stdout(&inspected).contains("reported 3 thing(s) at the time"),
        "{}",
        stdout(&inspected)
    );
}

#[test]
fn a_refused_file_writes_nothing_at_all() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    for name in ["01-truncated.step", "03-missing-terminator.step"] {
        let directory = tempfile::tempdir().expect("temp dir");
        let input = source(directory.path(), "damaged", name);
        let before = snapshot(directory.path());
        let output = directory.path().join("refused.fcad");

        let result = import(&input, &output, &[]);
        assert_eq!(code(&result), REJECTED, "{name}: {}", stdout(&result));

        let report = stdout(&result);
        assert!(report.starts_with("refused "), "{name}: {report}");
        assert!(report.contains("nothing was written"), "{name}: {report}");
        // A refusal with no reason is a refusal nobody can act on.
        assert!(report.contains("reading reported"), "{name}: {report}");

        assert!(!output.exists(), "{name}: a refused import left a document");
        assert_eq!(
            snapshot(directory.path()),
            before,
            "{name}: a refused import changed the directory"
        );
    }
}

#[test]
fn a_file_whose_parts_cannot_be_named_writes_no_document() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    // Open CASCADE reads this one and produces the same geometry as the
    // undamaged assembly. What it cannot produce is an identity for one of its
    // definitions, and a document written from it could never be reopened —
    // the handles go with the session and there would be nothing left to
    // re-attach them by. So it is refused here, before a file exists.
    let input = source(
        dir.path(),
        "damaged",
        "06-duplicate-product-definition.step",
    );
    let before = snapshot(dir.path());
    let output = dir.path().join("collided.fcad");

    let result = import(&input, &output, &[]);
    assert_eq!(code(&result), REJECTED, "{}", stdout(&result));

    let report = stdout(&result);
    assert!(report.starts_with("refused "), "{report}");
    assert!(report.contains("nothing was written"), "{report}");
    // Reported as this project's own finding, not as something the kernel
    // said: it read the file without complaining about anything of the sort.
    assert!(
        report.contains("identifying"),
        "the refusal should say it was an identity that was missing: {report}"
    );

    assert!(!output.exists(), "a document was written from it");
    assert_eq!(
        snapshot(dir.path()),
        before,
        "a refused import changed the directory"
    );
}

#[test]
fn an_existing_document_is_not_replaced_without_being_asked() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    let input = source(dir.path(), "canonical", "01-single-part.step");
    let output = dir.path().join("taken.fcad");
    std::fs::write(&output, b"somebody else's work").expect("writes");

    let result = import(&input, &output, &[]);
    assert_eq!(code(&result), FAILED);
    assert!(String::from_utf8_lossy(&result.stderr).contains("--force"));
    assert_eq!(
        std::fs::read(&output).expect("reads"),
        b"somebody else's work",
        "the existing file was replaced without being asked"
    );

    // With --force it is replaced, and with a whole document.
    let forced = import(&input, &output, &["--force"]);
    assert_eq!(code(&forced), CLEAN, "{}", stdout(&forced));
    assert!(
        run(&["validate", &output.to_string_lossy()])
            .status
            .success()
    );
}

#[test]
fn a_failed_import_leaves_no_scratch_file_behind() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    // A file that is refused after the destination has been checked and the
    // kernel opened: the point in the command where a scratch file would exist
    // if one were created before the outcome was known.
    let input = source(dir.path(), "damaged", "01-truncated.step");
    assert_eq!(
        code(&import(&input, &dir.path().join("nothing.fcad"), &[])),
        REJECTED
    );

    let leftovers: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .expect("lists")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.to_string_lossy().contains(".partial"))
        .collect();
    assert!(leftovers.is_empty(), "scratch files remain: {leftovers:?}");
}

#[test]
fn a_publication_failure_after_writing_the_document_leaves_no_scratch_directory() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    let input = source(dir.path(), "canonical", "01-single-part.step");
    let output = dir.path().join("occupied");
    std::fs::create_dir(&output).expect("creates a destination that cannot be replaced by a file");
    std::fs::write(output.join("keep"), b"somebody else's work").expect("writes valuable data");

    let result = import(&input, &output, &["--force"]);
    assert_eq!(code(&result), FAILED, "{}", stdout(&result));
    assert_eq!(
        std::fs::read(output.join("keep")).expect("the destination remains"),
        b"somebody else's work"
    );

    let leftovers: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .expect("lists")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.to_string_lossy().contains(".partial"))
        .collect();
    assert!(leftovers.is_empty(), "scratch paths remain: {leftovers:?}");
}

#[test]
fn the_step_file_cannot_be_named_as_its_own_destination() {
    let dir = tempfile::tempdir().expect("temp dir");
    let input = source(dir.path(), "canonical", "01-single-part.step");
    let before = std::fs::read(&input).expect("reads");

    for extra in [&[][..], &["--force"][..]] {
        let result = import(&input, &input, extra);
        assert_eq!(code(&result), FAILED, "{extra:?}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("cannot also be"),
            "{extra:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            std::fs::read(&input).expect("reads"),
            before,
            "{extra:?}: the STEP file was overwritten by its own import"
        );
    }
}

#[test]
fn the_object_is_named_after_the_file_unless_told_otherwise() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    let input = source(dir.path(), "canonical", "02-flat-assembly.step");

    let by_default = dir.path().join("default.fcad");
    let default_report = stdout(&import(&input, &by_default, &[]));
    assert!(
        stdout(&run(&["inspect", &by_default.to_string_lossy()])).contains("02-flat-assembly"),
        "the object should be named after the file it came from"
    );

    let named = dir.path().join("named.fcad");
    let named_report = stdout(&import(&input, &named, &["--name", "Chassis"]));
    assert!(
        stdout(&run(&["inspect", &named.to_string_lossy()])).contains("Chassis"),
        "the object should carry the name it was given"
    );

    // What the file said about its own parts is untouched by --name. The only
    // lines that may differ are the one naming the destination and the one
    // naming the object; the definitions below them are the file's own words.
    let differing: Vec<(&str, &str)> = default_report
        .lines()
        .zip(named_report.lines())
        .filter(|(before, after)| before != after)
        .collect();
    assert_eq!(
        differing.len(),
        2,
        "--name changed more than the object's own name: {differing:?}"
    );
    assert!(differing[0].0.starts_with("imported "), "{differing:?}");
    assert!(differing[1].0.starts_with("  object "), "{differing:?}");
    assert!(differing[1].1.contains("Chassis"), "{differing:?}");
}

#[test]
fn nothing_that_is_not_a_step_file_becomes_a_document() {
    let dir = tempfile::tempdir().expect("temp dir");
    if !has_kernel(dir.path()) {
        return;
    }

    for (name, bytes) in [
        ("empty.step", b"".to_vec()),
        ("prose.step", b"not a step file at all".to_vec()),
        ("zeros.step", vec![0u8; 512]),
    ] {
        let input = dir.path().join(name);
        std::fs::write(&input, &bytes).expect("writes");
        let output = dir.path().join(format!("{name}.fcad"));

        let result = import(&input, &output, &[]);
        // Refused outright or failed to read at all; what is not acceptable is
        // a document.
        assert!(
            matches!(code(&result), REJECTED | FAILED),
            "{name} exited {} and should not have: {}",
            code(&result),
            stdout(&result)
        );
        assert!(!output.exists(), "{name} produced a document");
    }
}
