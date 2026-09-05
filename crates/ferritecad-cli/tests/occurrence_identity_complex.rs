// SPDX-License-Identifier: MIT
//! §22B-1e3a: every placement of the complex assembly has its own durable
//! identity, and it is the document's rather than this run's.
//!
//! The route is the shipped one and nothing about it is fabricated:
//! `import-step` publishes an `.fcad`, the external STEP is deleted, and every
//! reading afterwards can only have come from the bytes the document stores.
//! What is measured is the identity each [`ExportNode`] arrives with at the
//! neutral export boundary.
//!
//! # Why this needs two more processes
//!
//! "The same identity comes back" is a claim about what was written down, and a
//! single process could satisfy it by remembering. So the identities are read
//! by two child processes that share nothing with this one but the file: each
//! opens the document cold, exports it, prints what it found and exits. If the
//! identity were minted at open, at export, or from anything this run happens
//! to traverse, the two children would disagree with each other and with the
//! parent.
//!
//! This does not measure the FBX writer, which in this slice does not read the
//! identity at all; that boundary is gated in `ferritecad-export`.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use ferritecad_export::{ExportGeometry, ExportOccurrence, ExportScene, ExportSource};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_scene::export_scene;

const NOTICED: i32 = 4;
/// The definition this build cannot give triangles to. Its placements are held
/// to the same identity contract as any other, which is the whole point of the
/// §22B-1c boundary: a partial export must not start to look complete.
const OMITTED: &str = "step.product_definition#2583";

/// The environment variable that turns this binary into the reader child.
const CHILD_DOCUMENT: &str = "FCAD_OCCURRENCE_CHILD_DOCUMENT";

/// What the child prints before each identity it found.
const LINE: &str = "FCAD_OCCURRENCE ";

fn ferritecad() -> PathBuf {
    let mut path = std::env::current_exe().expect("the test knows where it is");
    path.pop();
    path.pop();
    path.push(format!("ferritecad{}", std::env::consts::EXE_SUFFIX));
    path
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step/interoperability/c3d-ap203-complex-assembly.stp")
}

fn key_of(source: &ExportSource) -> &str {
    match source {
        ExportSource::Imported { definition_key, .. } => definition_key,
        ExportSource::Body { .. } => panic!("the imported assembly exported a native body"),
    }
}

/// One cold export of a stored document, in a session that never imported it.
fn export_of(path: &Path) -> ExportScene {
    let mut kernel = OcctKernel::new().expect("opens a fresh export kernel session");
    let scene = export_scene(
        path,
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the stored import reopens and exports");
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the export retained imported shapes"
    );
    scene
}

/// The identity of every node, in scene order, as a comparable line each.
///
/// Rendered rather than compared as values so a child process can print them
/// and the parent can compare exactly what the child saw. `Unrecorded` is
/// spelled out rather than skipped: a run that lost every identity must differ
/// from one that kept them, not produce a shorter list.
fn identities(scene: &ExportScene) -> Vec<String> {
    scene
        .nodes()
        .iter()
        .map(|node| match node.occurrence {
            ExportOccurrence::Occurrence(occurrence) => format!("occurrence {occurrence}"),
            ExportOccurrence::Object(object) => format!("object {object}"),
            ExportOccurrence::Unrecorded => "unrecorded".to_owned(),
        })
        .collect()
}

/// Imports the committed fixture through the shipped command and deletes the
/// STEP, leaving a document that is the only remaining copy of the assembly.
fn imported_document(directory: &Path) -> PathBuf {
    let input = directory.join("complex.stp");
    let output = directory.join("complex.fcad");
    let original = std::fs::read(fixture()).expect("reads the exact fixture");
    assert_eq!(original.len(), 1_896_140, "the fixture baseline changed");
    std::fs::write(&input, &original).expect("copies the fixture byte for byte");

    let imported = Command::new(ferritecad())
        .arg("import-step")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .output()
        .expect("the shipped import-step command runs");
    let code = imported.status.code().expect("the command exits normally");
    assert_eq!(
        code,
        NOTICED,
        "partial import is neither clean success nor refusal: {}{}",
        String::from_utf8_lossy(&imported.stdout),
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(output.is_file(), "exit 4 did not publish the FCAD document");

    // From here on the external STEP does not exist. Everything any later
    // reading knows came out of the document.
    std::fs::remove_file(&input).expect("hides the external STEP before exporting");
    output
}

/// The reader role of this binary: one cold export, one line per node.
///
/// A separate `#[test]` because that is the only entry point a test binary
/// offers, and it does nothing at all unless the parent asked for it by
/// setting the document in the environment.
#[test]
fn print_the_occurrence_identities_of_one_document() {
    let Ok(document) = std::env::var(CHILD_DOCUMENT) else {
        return;
    };
    let scene = export_of(Path::new(&document));
    // Printed before the identities so a parent can tell a child that read a
    // different document from one that read the same one differently.
    println!("{LINE}nodes {}", scene.nodes().len());
    for line in identities(&scene) {
        println!("{LINE}{line}");
    }
}

/// Runs this binary again, in its reader role, over the given document.
fn identities_from_another_process(document: &Path) -> Vec<String> {
    let binary = std::env::current_exe().expect("the test knows where it is");
    let run = Command::new(&binary)
        .arg("--exact")
        .arg("print_the_occurrence_identities_of_one_document")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(CHILD_DOCUMENT, document)
        .output()
        .expect("this test binary runs as its own child");
    let out = String::from_utf8_lossy(&run.stdout).into_owned();
    assert!(
        run.status.success(),
        "the reader child failed: {out}{}",
        String::from_utf8_lossy(&run.stderr)
    );
    // Found anywhere in the line rather than at its start: with `--nocapture`
    // libtest writes `test <name> ... ` without a newline, so the child's first
    // line arrives with that prefix already on it.
    let lines: Vec<String> = out
        .lines()
        .filter_map(|line| line.find(LINE).map(|at| line[at + LINE.len()..].to_owned()))
        .collect();
    assert!(
        !lines.is_empty(),
        "the reader child printed no identities at all: {out}"
    );
    lines
}

#[test]
fn every_placement_of_the_reopened_complex_assembly_has_its_own_durable_identity() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let document = imported_document(directory.path());
    let scene = export_of(&document);

    // First that the assembly itself arrived. A gate that went straight to the
    // identities could pass on an export that had lost half the model, and
    // would then be measuring nothing.
    assert_eq!(
        scene.definitions().len(),
        46,
        "a definition of the assembly was lost or invented"
    );
    assert_eq!(
        scene.nodes().len(),
        140,
        "a hierarchy node was lost or invented"
    );
    assert_eq!(scene.roots().count(), 1, "the one root changed");

    // Then that every one of those nodes carries an identity the document
    // recorded, and that no two carry the same one.
    let recorded: Vec<&ExportOccurrence> = scene
        .nodes()
        .iter()
        .map(|node| &node.occurrence)
        .filter(|occurrence| occurrence.is_recorded())
        .collect();
    assert_eq!(
        recorded.len(),
        140,
        "{} of 140 placements reached the export boundary without an identity the document \
         recorded",
        140 - recorded.len()
    );
    let distinct: BTreeSet<&ExportOccurrence> = recorded.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        140,
        "{} placements share an identity with another",
        140 - distinct.len()
    );

    // Every one of them is an occurrence rather than an object: this document
    // holds no native body, and a native identity appearing here would mean the
    // two kinds had been confused.
    assert!(
        scene
            .nodes()
            .iter()
            .all(|node| matches!(node.occurrence, ExportOccurrence::Occurrence(_))),
        "an imported placement was identified as something other than an occurrence"
    );

    // The structural frames and the placements of the definition this build
    // cannot mesh are held to exactly the same rule. Eleven definitions of this
    // assembly are structure, and they are placed as twenty-eight of its nodes.
    let structural: Vec<&_> = scene
        .definitions()
        .iter()
        .filter(|definition| matches!(definition.geometry, ExportGeometry::Structural))
        .collect();
    assert_eq!(
        structural.len(),
        11,
        "the number of structural definitions changed"
    );
    let frames: Vec<&_> = scene
        .nodes()
        .iter()
        .filter(|node| {
            structural
                .iter()
                .any(|definition| definition.id == node.definition)
        })
        .collect();
    assert_eq!(
        frames.len(),
        28,
        "the number of assembly frame nodes changed"
    );
    assert!(
        frames.iter().all(|node| node.occurrence.is_recorded()),
        "an assembly frame reached the boundary without an identity"
    );

    let omitted = scene
        .definitions()
        .iter()
        .find(|definition| key_of(&definition.source) == OMITTED)
        .expect("the omitted definition is still in the export");
    let omitted_nodes: Vec<&_> = scene
        .nodes()
        .iter()
        .filter(|node| node.definition == omitted.id)
        .collect();
    assert!(
        !omitted_nodes.is_empty(),
        "nothing places the omitted definition"
    );
    assert!(
        omitted_nodes
            .iter()
            .all(|node| node.occurrence.is_recorded()),
        "a placement of the definition this build cannot mesh was left without an identity"
    );

    // Two placements of one definition are two identities on one shared mesh.
    let shared = scene
        .definitions()
        .iter()
        .map(|definition| {
            (
                definition.id,
                scene
                    .nodes()
                    .iter()
                    .filter(|node| node.definition == definition.id)
                    .count(),
            )
        })
        .max_by_key(|(_, places)| *places)
        .expect("the export has definitions");
    assert!(
        shared.1 >= 2,
        "no definition of this assembly is placed twice, so the gate measures nothing"
    );
    let together: BTreeSet<&ExportOccurrence> = scene
        .nodes()
        .iter()
        .filter(|node| node.definition == shared.0)
        .map(|node| &node.occurrence)
        .collect();
    assert_eq!(
        together.len(),
        shared.1,
        "the {} placements of one definition answer to {} identities",
        shared.1,
        together.len()
    );

    // And two more processes, sharing nothing with this one but the file, read
    // back the same identities in the same order.
    let mine: Vec<String> = std::iter::once(format!("nodes {}", scene.nodes().len()))
        .chain(identities(&scene))
        .collect();
    let first = identities_from_another_process(&document);
    let second = identities_from_another_process(&document);
    assert_eq!(
        first, mine,
        "an independent process read different identities out of the same document"
    );
    assert_eq!(
        second, first,
        "two independent processes disagreed about the same document"
    );

    eprintln!(
        "FCAD_OCCURRENCE_IDENTITY_COMPLEX nodes=140 recorded={} distinct={} frames={} \
         shared_places={}",
        recorded.len(),
        distinct.len(),
        frames.len(),
        shared.1
    );
}
