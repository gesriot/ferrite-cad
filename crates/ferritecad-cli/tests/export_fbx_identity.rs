// SPDX-License-Identifier: MIT
//! §22B-1e3b: the identity a document recorded is the identity the shipped FBX
//! carries.
//!
//! The route is the shipped one and nothing about it is fabricated:
//! `import-step` publishes an `.fcad`, the external STEP is deleted, and
//! `export-fbx` is run twice as a command in two processes that share nothing
//! with this one but the file. What is measured is the file a person gets.
//!
//! # Why the document is read a second time here
//!
//! The join has two sides and they must come from different places. One side
//! is the file; the other is what the *document* stored, which this gate reads
//! for itself through `export_scene`. A file compared only with itself would
//! prove that the writer is self-consistent, which is not the claim. The claim
//! is that node `n` of the file carries the identity the document recorded for
//! placement `n`, and only two independent readings can say that.
//!
//! # Delivery first, identity second
//!
//! The assembly is asserted to have arrived — 46 definitions, 140 nodes, one
//! root, the omitted definition still placed — before a single identity is
//! looked at. A gate that went straight to the properties could pass on an
//! export that had lost half the model, and would then be measuring nothing.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use ferritecad_export::{ExportOccurrence, ExportScene, ExportSource};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_scene::export_scene;

const NOTICED: i32 = 4;
/// A published export that is not the whole model.
const PARTIAL: i32 = 6;
const OMITTED: &str = "step.product_definition#2583";

/// The two properties this slice adds, and the prefix each of their values
/// must carry. Spelled out here rather than imported from the writer: a gate
/// that asked the writer what it wrote would agree with it by construction.
const DEFINITION_PROPERTY: &str = "FerriteCADDefinitionId";
const OCCURRENCE_PROPERTY: &str = "FerriteCADOccurrenceId";
const DEFINITION_PREFIX: &str = "fcad1:def:source:";
const OCCURRENCE_PREFIX: &str = "fcad1:occ:place:";

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

// --------------------------------------------------------------- the reader

/// One `Model` of a written file, reduced to what this gate asks about.
#[derive(Debug, Default)]
struct WrittenModel {
    name: String,
    properties: BTreeMap<String, String>,
}

/// Reads the `Model` objects of an FBX 7.4 ASCII file, a line at a time.
///
/// Only the object headers and the user-defined properties inside them: the
/// complex assembly's file is hundreds of megabytes of vertex arrays, and the
/// payload of those arrays is the independent ufbx gate's business.
fn models(path: &Path) -> Vec<WrittenModel> {
    let file = std::fs::File::open(path).expect("the command left a file");
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut out: Vec<WrittenModel> = Vec::new();
    let mut in_model = false;
    for line in reader.lines() {
        let line = line.expect("the file is readable");
        let trimmed = line.trim();
        let Some((name, rest)) = trimmed.split_once(':') else {
            continue;
        };
        let name = name.trim();
        match name {
            "Model" => {
                let props = fields(rest);
                in_model = true;
                out.push(WrittenModel {
                    name: object_name(&props[1]),
                    properties: BTreeMap::new(),
                });
            }
            "Geometry" | "Material" | "Connections" => in_model = false,
            "P" if in_model => {
                let props = fields(rest);
                // Only what the writer marked user-defined, which is where
                // every FerriteCAD property lives.
                if props.len() >= 5 && unquote(&props[3]).contains('U') {
                    let model = out.last_mut().expect("a property inside a model");
                    model
                        .properties
                        .insert(unquote(&props[0]), unquote(&props[4]));
                }
            }
            _ => {}
        }
    }
    out
}

fn fields(rest: &str) -> Vec<String> {
    let rest = rest.trim_end().trim_end_matches('{');
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in rest.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                current.push(character);
            }
            ',' if !quoted => {
                let token = current.trim().to_owned();
                current.clear();
                if !token.is_empty() {
                    out.push(token);
                }
            }
            _ => current.push(character),
        }
    }
    let token = current.trim().to_owned();
    if !token.is_empty() {
        out.push(token);
    }
    out
}

fn unquote(token: &str) -> String {
    token
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(token)
        .replace("&quot;", "\"")
        .replace("&cr;", "\r")
        .replace("&lf;", "\n")
}

fn object_name(token: &str) -> String {
    let full = unquote(token);
    match full.find("::") {
        Some(at) => full[at + 2..].to_owned(),
        None => full,
    }
}

// -------------------------------------------------------------- the document

/// What the document recorded for one placement, as raw facts.
///
/// Deliberately not the wire form: rendering these into the grammar here would
/// be a second copy of the writer's encoding, and two copies of one rule agree
/// with each other rather than with the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Recorded {
    display_name: String,
    definition_key: String,
    source: String,
    occurrence: String,
}

fn recorded_of(scene: &ExportScene) -> Vec<Recorded> {
    scene
        .nodes()
        .iter()
        .map(|node| {
            let definition = scene
                .definition(node.definition)
                .expect("every node places a definition of its own scene");
            let (source, definition_key) = match &definition.source {
                ExportSource::Imported {
                    source,
                    definition_key,
                } => (source.to_string(), definition_key.clone()),
                ExportSource::Body { .. } => {
                    panic!("the imported assembly exported a native body")
                }
            };
            let occurrence = match node.occurrence {
                ExportOccurrence::Occurrence(occurrence) => occurrence.to_string(),
                ExportOccurrence::Object(object) => panic!("an imported placement is {object}"),
                ExportOccurrence::Unrecorded => {
                    panic!("§22B-1e3a persisted this document's placement identities")
                }
            };
            Recorded {
                display_name: node.display_name.clone().unwrap_or_default(),
                definition_key,
                source,
                occurrence,
            }
        })
        .collect()
}

fn export_of(path: &Path) -> ExportScene {
    let mut kernel = OcctKernel::new().expect("opens a fresh export kernel session");
    let scene = export_scene(
        path,
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the stored partial import reopens and exports");
    assert_eq!(kernel.live_shape_count(), 0, "the export retained shapes");
    scene
}

/// The payload the outside gate joins against, one line per node in scene
/// order: display name, source-local key, source identity, occurrence
/// identity, each field tab separated and none of them wire-encoded.
fn write_payload(path: &Path, recorded: &[Recorded]) {
    let mut file = std::fs::File::create(path).expect("the payload file is writable");
    for entry in recorded {
        writeln!(
            file,
            "{}\t{}\t{}\t{}",
            entry.display_name, entry.definition_key, entry.source, entry.occurrence
        )
        .expect("the payload file is writable");
    }
    file.flush().expect("the payload file is writable");
}

// ----------------------------------------------------------------- the gate

#[test]
fn every_placement_of_the_exported_complex_assembly_carries_the_identity_its_document_recorded() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let input = directory.path().join("complex.stp");
    let document = directory.path().join("complex.fcad");
    let original = std::fs::read(fixture()).expect("reads the exact fixture");
    assert_eq!(original.len(), 1_896_140, "the fixture baseline changed");
    std::fs::write(&input, &original).expect("copies the fixture byte for byte");

    let imported = Command::new(ferritecad())
        .arg("import-step")
        .arg(&input)
        .arg("--output")
        .arg(&document)
        .output()
        .expect("the shipped import-step command runs");
    assert_eq!(
        imported.status.code().expect("the command exits normally"),
        NOTICED,
        "partial import is neither clean success nor refusal: {}{}",
        String::from_utf8_lossy(&imported.stdout),
        String::from_utf8_lossy(&imported.stderr)
    );

    // From here on the external STEP does not exist. Everything any later
    // reading knows came out of the document.
    std::fs::remove_file(&input).expect("hides the external STEP before exporting");

    // Two cold exports, in two processes that share nothing with this one but
    // the file. A value minted at export time would differ between them.
    let first = directory.path().join("first.fbx");
    let second = directory.path().join("second.fbx");
    for output in [&first, &second] {
        let exported = Command::new(ferritecad())
            .arg("export-fbx")
            .arg(&document)
            .arg("--output")
            .arg(output)
            .output()
            .expect("the shipped export-fbx command runs");
        assert_eq!(
            exported.status.code().expect("the command exits normally"),
            PARTIAL,
            "a partial export is neither a plain success nor a refusal: {}{}",
            String::from_utf8_lossy(&exported.stdout),
            String::from_utf8_lossy(&exported.stderr)
        );
    }
    let first_bytes = std::fs::read(&first).expect("the first export published a file");
    let second_bytes = std::fs::read(&second).expect("the second export published a file");
    assert!(!first_bytes.is_empty(), "the command published nothing");
    assert_eq!(
        first_bytes, second_bytes,
        "two cold exports of one stored document produced different bytes"
    );
    drop(second_bytes);
    drop(first_bytes);

    // The assembly arrived. Everything below this point is about identity, and
    // asking about identity in a file that lost the model would measure
    // nothing.
    let scene = export_of(&document);
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
    let recorded = recorded_of(&scene);
    assert!(
        recorded.iter().any(|entry| entry.definition_key == OMITTED),
        "the definition this build cannot mesh left the export"
    );

    let written = models(&first);
    assert_eq!(
        written.len(),
        140,
        "the file does not have one model per placement"
    );
    for (index, (model, entry)) in written.iter().zip(&recorded).enumerate() {
        assert_eq!(
            model.name, entry.display_name,
            "model {index} is not the placement the document reports at that position"
        );
    }

    // Published here, as soon as delivery is proven and before a single
    // identity is looked at, so the outside gate can read the same bytes and
    // the same payload even on a run where the assertions below fail. That is
    // what let the failing-first measurement be taken from both readers at
    // once rather than from this one alone.
    if let Ok(destination) = std::env::var("FCAD_FBX_IDENTITY_OUT") {
        let destination = PathBuf::from(destination);
        std::fs::copy(&first, &destination).expect("the artefact directory is writable");
        write_payload(&destination.with_extension("payload"), &recorded);
        eprintln!(
            "FCAD_EXPORT_FBX_IDENTITY_ARTEFACT {}",
            destination.display()
        );
    }

    // Every node, including the assembly frames and the placements of the
    // definition this build cannot mesh, carries both properties.
    let mut missing_occurrence = 0usize;
    let mut missing_definition = 0usize;
    for model in &written {
        if !model.properties.contains_key(OCCURRENCE_PROPERTY) {
            missing_occurrence += 1;
        }
        if !model.properties.contains_key(DEFINITION_PROPERTY) {
            missing_definition += 1;
        }
    }
    assert_eq!(
        missing_occurrence, 0,
        "{missing_occurrence} of 140 placements reached the file without a \
         {OCCURRENCE_PROPERTY} the document recorded"
    );
    assert_eq!(
        missing_definition, 0,
        "{missing_definition} of 140 placements reached the file without a \
         {DEFINITION_PROPERTY} the document recorded"
    );

    // And the value on node `n` is the identity stored for placement `n`,
    // rather than merely some stable unique value. Checked by containment of
    // the exact identifier text plus the domain prefix, so this gate joins on
    // what the document says and does not re-implement the grammar.
    for (index, (model, entry)) in written.iter().zip(&recorded).enumerate() {
        let occurrence = &model.properties[OCCURRENCE_PROPERTY];
        assert!(
            occurrence.starts_with(OCCURRENCE_PREFIX),
            "placement {index} carries {occurrence}, which is not in the occurrence domain"
        );
        assert!(
            occurrence.contains(&entry.occurrence),
            "placement {index} carries {occurrence}, and the document recorded {} for it",
            entry.occurrence
        );
        let definition = &model.properties[DEFINITION_PROPERTY];
        assert!(
            definition.starts_with(DEFINITION_PREFIX),
            "placement {index} carries {definition}, which is not in the definition domain"
        );
        assert!(
            definition.contains(&entry.source),
            "placement {index} carries {definition}, which does not name source {}",
            entry.source
        );
    }

    // One definition identity per definition, one occurrence identity per
    // placement, and the two domains disjoint.
    let definitions: BTreeSet<&String> = written
        .iter()
        .map(|model| &model.properties[DEFINITION_PROPERTY])
        .collect();
    assert_eq!(
        definitions.len(),
        46,
        "the file names {} definition identities for 46 definitions",
        definitions.len()
    );
    let occurrences: BTreeSet<&String> = written
        .iter()
        .map(|model| &model.properties[OCCURRENCE_PROPERTY])
        .collect();
    assert_eq!(
        occurrences.len(),
        140,
        "the file names {} placement identities for 140 placements",
        occurrences.len()
    );
    assert!(
        definitions.is_disjoint(&occurrences),
        "a definition identity and a placement identity are the same string"
    );

    // The shared definition keeps one identity across all of its placements.
    let by_definition: BTreeMap<&String, usize> =
        written.iter().fold(BTreeMap::new(), |mut counts, model| {
            *counts
                .entry(&model.properties[DEFINITION_PROPERTY])
                .or_default() += 1;
            counts
        });
    let shared = by_definition
        .values()
        .copied()
        .max()
        .expect("the file has definitions");
    assert!(
        shared >= 2,
        "no definition of this assembly is placed twice, so the gate measures nothing"
    );

    eprintln!(
        "FCAD_EXPORT_FBX_IDENTITY nodes=140 definitions={} occurrences={} shared_places={shared}",
        definitions.len(),
        occurrences.len()
    );
}
