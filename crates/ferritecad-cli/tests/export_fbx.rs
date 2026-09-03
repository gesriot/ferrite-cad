// SPDX-License-Identifier: MIT
//! The shipped `export-fbx` command: document in, one FBX out, or nothing.
//!
//! Run against the built binary rather than the functions behind it. What is
//! being checked is a command — which exit status it returns, what it refuses,
//! what it says on each stream, and above all what it leaves on disk when it
//! fails — and none of that is visible from inside.
//!
//! The complex assembly's end-to-end route lives in `export_fbx_complex.rs`,
//! which needs Open CASCADE and a real partial import. Everything here that
//! needs geometry says so and skips when this build has no kernel.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ferritecad_fixtures::plate_source;

/// A partial export: the file is there, and something in the document is not.
const EXIT_PARTIAL: i32 = 6;

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

fn export(document: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        "export-fbx".to_owned(),
        document.to_string_lossy().into_owned(),
        "--output".to_owned(),
        output.to_string_lossy().into_owned(),
    ];
    args.extend(extra.iter().map(|value| (*value).to_owned()));
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&borrowed)
}

/// A private copy of the committed plate, which must never be opened in place.
fn plate(directory: &Path) -> PathBuf {
    let target = directory.join("plate.fcad");
    std::fs::copy(plate_source(), &target).expect("copies the fixture");
    target
}

/// Whether this build has a kernel at all.
///
/// Asked by exporting, because that is the path being gated: a build without
/// Open CASCADE refuses at the session and says so, and there is nothing here
/// for such a build to check.
fn has_kernel(document: &Path, out: &Path) -> bool {
    let output = export(document, out, &[]);
    if output.status.success() {
        let _ = std::fs::remove_file(out);
        return true;
    }
    let complaint = String::from_utf8_lossy(&output.stderr);
    if complaint.contains("built without Open CASCADE") || complaint.contains("no Open CASCADE") {
        eprintln!("skipped: this build has no Open CASCADE");
        return false;
    }
    panic!(
        "export-fbx failed for a reason other than a missing kernel:\n{complaint}{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// Every scratch name this command may have created and must not have left.
fn leftovers(directory: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(directory)
        .expect("lists the working directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.to_string_lossy().contains(".partial"))
        .collect()
}

#[test]
fn the_command_exists_and_writes_an_fbx_the_format_recognises() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let document = plate(directory.path());
    let output = directory.path().join("plate.fbx");
    if !has_kernel(&document, &output) {
        return;
    }

    let exported = export(&document, &output, &[]);
    assert_eq!(
        exported.status.code(),
        Some(0),
        "a complete export is a plain success:\n{}{}",
        String::from_utf8_lossy(&exported.stdout),
        String::from_utf8_lossy(&exported.stderr)
    );

    let bytes = std::fs::read(&output).expect("the command published an FBX");
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).into_owned();
    assert!(
        head.contains("FBXVersion: 7400"),
        "the published file is not FBX 7400:\n{head}"
    );
    assert!(
        head.contains("FBXHeaderExtension"),
        "the published file has no FBX header:\n{head}"
    );

    // The whole document is there, so nothing is said on the error stream.
    assert_eq!(
        String::from_utf8_lossy(&exported.stderr),
        "",
        "a complete export said something on standard error"
    );
    let said = String::from_utf8_lossy(&exported.stdout);
    assert!(
        said.contains("complete"),
        "a complete export does not say so:\n{said}"
    );
    for word in ["partial", "omission", "omitted", "refusal"] {
        assert!(
            !said.contains(word),
            "a complete export used the vocabulary of a partial one ({word}):\n{said}"
        );
    }
    assert!(leftovers(directory.path()).is_empty());
}

#[test]
fn the_same_document_exports_to_the_same_bytes_and_the_same_report() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let document = plate(directory.path());
    let first = directory.path().join("first.fbx");
    if !has_kernel(&document, &first) {
        return;
    }

    let second = directory.path().join("second.fbx");
    let one = export(&document, &first, &[]);
    let two = export(&document, &second, &[]);
    assert_eq!(one.status.code(), Some(0));
    assert_eq!(two.status.code(), Some(0));

    assert_eq!(
        std::fs::read(&first).expect("reads the first"),
        std::fs::read(&second).expect("reads the second"),
        "two exports of one document produced different bytes"
    );

    // The reports differ only where the paths do, which is what was asked for.
    let normalise = |output: &Output, path: &Path| {
        String::from_utf8_lossy(&output.stdout).replace(&*path.to_string_lossy(), "<output>")
    };
    assert_eq!(normalise(&one, &first), normalise(&two, &second));
    assert_eq!(one.stderr, two.stderr);
}

#[test]
fn an_existing_file_is_not_replaced_without_being_asked() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let document = plate(directory.path());
    let probe = directory.path().join("probe.fbx");
    if !has_kernel(&document, &probe) {
        return;
    }

    let output = directory.path().join("kept.fbx");
    std::fs::write(&output, b"yesterday's export").expect("writes what must survive");

    let refused = export(&document, &output, &[]);
    assert_eq!(refused.status.code(), Some(2), "an existing file was taken");
    assert_eq!(
        std::fs::read(&output).expect("the file is still there"),
        b"yesterday's export"
    );
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("--force"),
        "the refusal does not say what would allow it"
    );
    assert!(leftovers(directory.path()).is_empty());
}

#[test]
fn force_replaces_the_destination_completely() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let document = plate(directory.path());
    let output = directory.path().join("replaced.fbx");
    if !has_kernel(&document, &output) {
        return;
    }

    // Longer than the export, so a partial overwrite would leave a tail.
    let stale = vec![b'x'; 8 * 1024 * 1024];
    std::fs::write(&output, &stale).expect("writes the file to be replaced");

    let forced = export(&document, &output, &["--force"]);
    assert_eq!(
        forced.status.code(),
        Some(0),
        "--force did not export:\n{}",
        String::from_utf8_lossy(&forced.stderr)
    );

    let bytes = std::fs::read(&output).expect("reads the replacement");
    assert!(
        bytes.starts_with(b"; FBX 7.4.0 project file"),
        "the destination does not begin as an FBX"
    );
    assert!(
        !bytes.ends_with(b"xxxx"),
        "the old file's tail survived the replacement"
    );
    assert_ne!(bytes, stale);
    assert!(leftovers(directory.path()).is_empty());
}

#[test]
fn the_document_cannot_be_its_own_output() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let document = plate(directory.path());
    let before = std::fs::read(&document).expect("snapshots the document");

    for extra in [&[][..], &["--force"][..]] {
        let refused = export(&document, &document, extra);
        assert_eq!(
            refused.status.code(),
            Some(2),
            "the document was accepted as its own FBX output with {extra:?}"
        );
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("cannot also be"),
            "the refusal does not say why"
        );
        assert_eq!(
            std::fs::read(&document).expect("the document is still there"),
            before,
            "the document was changed by being named as its own output"
        );
    }
    assert!(leftovers(directory.path()).is_empty());
}

#[test]
fn exporting_does_not_disturb_the_document() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let document = plate(directory.path());
    let output = directory.path().join("plate.fbx");
    if !has_kernel(&document, &output) {
        return;
    }
    let before = std::fs::read(&document).expect("snapshots the document");

    let exported = export(&document, &output, &["--force"]);
    assert_eq!(exported.status.code(), Some(0));
    assert_eq!(
        std::fs::read(&document).expect("the document is still there"),
        before,
        "exporting rewrote the document it read"
    );
}

/// The nesting and the sharing a picture throws away.
///
/// This fixture is two levels deep and places one part four times. A file that
/// flattened it would still hold every triangle and would no longer describe
/// the model: seven roots instead of one, and four copies of a cube instead of
/// four placements of it.
#[test]
fn a_nested_assembly_keeps_its_hierarchy_and_shares_one_geometry() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step/canonical/03-nested-assembly.step");
    let document = directory.path().join("nested.fcad");

    let imported = run(&[
        "import-step",
        &source.to_string_lossy(),
        "--output",
        &document.to_string_lossy(),
    ]);
    let complaint = String::from_utf8_lossy(&imported.stderr);
    if complaint.contains("built without Open CASCADE") || complaint.contains("no Open CASCADE") {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    assert!(
        matches!(imported.status.code(), Some(0 | 4)),
        "the fixture no longer imports: {complaint}"
    );

    let output = directory.path().join("nested.fbx");
    let exported = export(&document, &output, &[]);
    assert_eq!(
        exported.status.code(),
        Some(0),
        "the nested assembly did not export cleanly:\n{}{}",
        String::from_utf8_lossy(&exported.stdout),
        String::from_utf8_lossy(&exported.stderr)
    );

    let file = std::fs::read_to_string(&output).expect("reads the published FBX");
    let connections: Vec<(i64, i64)> = file
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("C: \"OO\","))
        .filter_map(|rest| {
            let (from, to) = rest.split_once(',')?;
            Some((from.trim().parse().ok()?, to.trim().parse().ok()?))
        })
        .collect();
    assert!(!connections.is_empty(), "the file records no connections");

    // One root and six children: the two levels of nesting survived.
    let models = file.matches("\n\tModel: ").count() + file.matches("\n\t\tModel: ").count();
    let roots = connections.iter().filter(|(_, to)| *to == 0).count();
    assert_eq!(roots, 1, "the hierarchy was flattened into {roots} roots");
    let children = connections
        .iter()
        .filter(|(from, to)| *to != 0 && *from >> 33 == 2 && *to >> 33 == 2)
        .count();
    assert_eq!(
        children, 6,
        "the nested placements lost their parents ({models} models in the file)"
    );

    // And one cube, placed four times, is one geometry object bound to four
    // models rather than four copies of the same triangles.
    let geometries: BTreeSet<i64> = connections
        .iter()
        .filter(|(from, _)| *from >> 33 == 1)
        .map(|(from, _)| *from)
        .collect();
    assert_eq!(
        geometries.len(),
        1,
        "one part reused four times became {} geometry objects",
        geometries.len()
    );
    assert_eq!(
        connections
            .iter()
            .filter(|(from, _)| *from >> 33 == 1)
            .count(),
        4,
        "the four placements of the cube were not all connected to it"
    );
    assert_eq!(file.matches("\tGeometry: ").count(), 1);
}

#[test]
fn the_help_documents_the_command_and_its_one_flag() {
    let listed = run(&["--help"]);
    assert!(listed.status.success());
    let listing = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listing.contains("export-fbx"),
        "the command is not listed:\n{listing}"
    );

    let detail = run(&["export-fbx", "--help"]);
    assert!(detail.status.success(), "export-fbx has no help of its own");
    let help = String::from_utf8_lossy(&detail.stdout);
    for expected in ["--output", "--force", "FBX"] {
        assert!(
            help.contains(expected),
            "the help does not mention {expected}:\n{help}"
        );
    }
    assert!(
        help.contains(&EXIT_PARTIAL.to_string()),
        "the help does not name the partial exit code:\n{help}"
    );
}
