// SPDX-License-Identifier: MIT
//! Writes the §22B-1e1 identity variants with the production FBX writer.
//!
//! Every byte Unity is shown in that measurement comes from here, which means
//! it comes from `write_fbx_ascii_7400` and from nothing else. The §22B-1a
//! Python generator is not involved: it settled the axis contract and has no
//! business producing the file whose asset identity is being measured.
//!
//! This is an example, not a route. It adds nothing to the writer, to
//! `ExportScene`, or to any shipped command: it only describes documents that
//! differ from one base document in exactly one measured way, so the editor
//! can be asked what its references do when a document changes that way.

// The measured scene's helpers are defined once, beside the §22B-1b2 gate.
#[path = "../tests/fbx_scene/mod.rs"]
mod fbx_scene;

use std::io::BufWriter;
use std::path::PathBuf;

use ferritecad_export::{
    ExportColourOrigin, ExportGeometry, ExportMaterial, ExportMesh, ExportOccurrence,
    ExportProvenance, ExportScene, ExportSceneBuilder, ExportSource, ExportTransform,
    write_fbx_ascii_7400,
};
use ferritecad_types::ImportedSourceId;

use fbx_scene::placement;

/// The durable source-local keys the measurement tracks.
///
/// These are what a FerriteCAD document keeps across an edit. Display names,
/// positions in the definition list and positions among siblings are not on
/// this list on purpose: the whole question is whether an editor's references
/// follow the first or the second.
const EARLY: &str = "step.product_definition#50";
const ALPHA: &str = "step.product_definition#100";
const BETA: &str = "step.product_definition#200";
const GAMMA: &str = "step.product_definition#300";
const INSERTED: &str = "step.product_definition#10";
const ROOT: &str = "step.product_definition#1";

/// The one immutable imported source shared by every variant.
///
/// Minting a source inside `variant_scene` would make the underlying
/// FerriteCAD identity change between files even though the current FBX
/// property exposes only its source-local half. Keeping the source fixed makes
/// the measurement's "same definition" premise true before the writer runs.
fn measured_source() -> ImportedSourceId {
    "019ffc72-2996-7000-8000-000000000001"
        .parse()
        .expect("the measured source is an RFC 4122 UUIDv7")
}

/// Which definition a variant leaves out, reorders or renames.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    /// The document every other variant is one change away from.
    Base,
    /// One display name changes; every durable key stays.
    RenamedDisplayName,
    /// An unrelated definition appears before the tracked ones.
    InsertedEarlierDefinition,
    /// An unrelated definition that was before the tracked ones goes away.
    RemovedEarlierDefinition,
    /// Two unrelated definitions swap places in export order.
    ReorderedDefinitions,
    /// A sibling occurrence appears between two tracked ones.
    InsertedSibling,
    /// A sibling occurrence goes away.
    RemovedSibling,
    /// Two sibling occurrences swap places.
    ReorderedSiblings,
    /// One whole definition, and its only placement, goes away.
    RemovedDefinition,
}

/// A mesh whose vertex count alone says which definition it came from.
///
/// Deliberately not a shared shape: if a reference silently moves from one
/// definition's geometry to another's, the count changes, and that is visible
/// without trusting a name, a key or a position.
fn mesh(vertices: usize, slots: Vec<ExportMaterial>) -> ExportMesh {
    assert!(vertices >= 3, "a measured mesh needs a triangle");
    let positions: Vec<[f32; 3]> = (0..vertices)
        .map(|index| {
            let step = index as f32;
            [100.0 * step, 200.0 * step, 300.0 * step]
        })
        .collect();
    let normals: Vec<[f32; 3]> = (0..vertices)
        .map(|index| match index % 3 {
            0 => [1.0, 0.0, 0.0],
            1 => [0.0, 1.0, 0.0],
            _ => [0.0, 0.0, 1.0],
        })
        .collect();
    let triangles: Vec<[u32; 3]> = (0..vertices - 2)
        .map(|index| {
            let base = index as u32;
            [base, base + 1, base + 2]
        })
        .collect();
    let slot_count = slots.len() as u32;
    let triangle_materials: Vec<u32> = (0..triangles.len())
        .map(|index| (index as u32) % slot_count)
        .collect();
    ExportMesh::new(positions, normals, triangles, triangle_materials, slots)
        .expect("a measured identity mesh is valid")
}

fn slot(name: &str, colour: [f64; 3]) -> ExportMaterial {
    ExportMaterial::new(name, colour, ExportColourOrigin::Source).expect("a linear colour")
}

/// The material slots of the tracked definition.
///
/// Both are called `Shell` and they are different colours, so a reader that
/// merges materials by display name merges two things a document keeps apart.
fn alpha_slots() -> Vec<ExportMaterial> {
    vec![
        slot("Shell", [0.603_827, 0.033_105, 0.010_023]),
        slot("Shell", [0.010_023, 0.100_482, 0.787_412]),
    ]
}

fn variant_scene(variant: Variant) -> ExportScene {
    let source = measured_source();
    let mut builder = ExportSceneBuilder::new();
    let provenance = ExportProvenance::default();
    let imported = |key: &str| ExportSource::Imported {
        source,
        definition_key: key.to_owned(),
    };

    // The display name of the tracked definition, and of its placements. Only
    // this string moves in `RenamedDisplayName`; the key below does not.
    let alpha_name = if variant == Variant::RenamedDisplayName {
        "Alpha Part Renamed"
    } else {
        "Alpha Part"
    };

    let root = builder
        .definition(
            imported(ROOT),
            Some("Assembly Root".to_owned()),
            provenance.clone(),
            ExportGeometry::Structural,
        )
        .expect("the root definition");

    let inserted = (variant == Variant::InsertedEarlierDefinition).then(|| {
        builder
            .definition(
                imported(INSERTED),
                Some("Inserted Part".to_owned()),
                provenance.clone(),
                ExportGeometry::Mesh(mesh(7, vec![slot("Inserted", [0.5, 0.5, 0.5])])),
            )
            .expect("the inserted definition")
    });

    let early = (variant != Variant::RemovedEarlierDefinition).then(|| {
        builder
            .definition(
                imported(EARLY),
                Some("Early Part".to_owned()),
                provenance.clone(),
                ExportGeometry::Mesh(mesh(3, vec![slot("Early", [0.9, 0.1, 0.1])])),
            )
            .expect("the early definition")
    });

    let alpha = builder
        .definition(
            imported(ALPHA),
            Some(alpha_name.to_owned()),
            provenance.clone(),
            ExportGeometry::Mesh(mesh(4, alpha_slots())),
        )
        .expect("the tracked definition");

    // `ReorderedDefinitions` swaps two definitions that the tracked reference
    // does not name, so nothing about the tracked definition itself changes.
    let (beta, gamma) = if variant == Variant::ReorderedDefinitions {
        let gamma = (variant != Variant::RemovedDefinition).then(|| {
            builder
                .definition(
                    imported(GAMMA),
                    // Deliberately the same display name as the tracked one.
                    Some("Alpha Part".to_owned()),
                    provenance.clone(),
                    ExportGeometry::Mesh(mesh(6, vec![slot("Gamma", [0.2, 0.7, 0.3])])),
                )
                .expect("the same-named definition")
        });
        let beta = builder
            .definition(
                imported(BETA),
                Some("Beta Part".to_owned()),
                provenance.clone(),
                ExportGeometry::Mesh(mesh(5, vec![slot("Beta", [0.1, 0.2, 0.9])])),
            )
            .expect("the beta definition");
        (beta, gamma)
    } else {
        let beta = builder
            .definition(
                imported(BETA),
                Some("Beta Part".to_owned()),
                provenance.clone(),
                ExportGeometry::Mesh(mesh(5, vec![slot("Beta", [0.1, 0.2, 0.9])])),
            )
            .expect("the beta definition");
        let gamma = (variant != Variant::RemovedDefinition).then(|| {
            builder
                .definition(
                    imported(GAMMA),
                    Some("Alpha Part".to_owned()),
                    provenance.clone(),
                    ExportGeometry::Mesh(mesh(6, vec![slot("Gamma", [0.2, 0.7, 0.3])])),
                )
                .expect("the same-named definition")
        });
        (beta, gamma)
    };

    let root_node = builder
        .node(
            None,
            root,
            ExportTransform::IDENTITY,
            Some("Assembly Root".to_owned()),
            None,
            ExportOccurrence::Unrecorded,
        )
        .expect("the root node");
    let under = Some(root_node);

    if let Some(inserted) = inserted {
        builder
            .node(
                under,
                inserted,
                placement([10.0, 20.0, 30.0], [0.0, 0.0, 0.0]),
                Some("Inserted Part".to_owned()),
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("the inserted placement");
    }
    if let Some(early) = early {
        builder
            .node(
                under,
                early,
                placement([100.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
                Some("Early Part".to_owned()),
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("the early placement");
    }

    // The two tracked placements of one geometry, with the same display name.
    builder
        .node(
            under,
            alpha,
            placement([200.0, 0.0, 0.0], [11.0, 0.0, 0.0]),
            Some(alpha_name.to_owned()),
            None,
            ExportOccurrence::Unrecorded,
        )
        .expect("the first tracked placement");
    if variant == Variant::InsertedSibling {
        builder
            .node(
                under,
                beta,
                placement([250.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
                Some("Inserted Sibling".to_owned()),
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("the inserted sibling");
    }
    builder
        .node(
            under,
            alpha,
            placement([300.0, 0.0, 0.0], [0.0, 17.0, 0.0]),
            Some(alpha_name.to_owned()),
            None,
            ExportOccurrence::Unrecorded,
        )
        .expect("the second tracked placement");

    let beta_first = |builder: &mut ExportSceneBuilder| {
        builder
            .node(
                under,
                beta,
                placement([400.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
                Some("Beta Part".to_owned()),
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("the beta placement");
    };
    let gamma_placement = |builder: &mut ExportSceneBuilder| {
        if let Some(gamma) = gamma {
            builder
                .node(
                    under,
                    gamma,
                    placement([600.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
                    Some("Alpha Part".to_owned()),
                    None,
                    ExportOccurrence::Unrecorded,
                )
                .expect("the same-named placement");
        }
    };

    if variant == Variant::ReorderedSiblings {
        gamma_placement(&mut builder);
        beta_first(&mut builder);
    } else {
        beta_first(&mut builder);
        gamma_placement(&mut builder);
    }

    // A second occurrence of one definition, which `RemovedSibling` drops.
    if variant != Variant::RemovedSibling {
        builder
            .node(
                under,
                beta,
                placement([500.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
                Some("Beta Part".to_owned()),
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("the second beta placement");
    }

    builder.finish().expect("a measured identity variant")
}

/// Every file the measurement imports, and the one change each one carries.
fn variants() -> Vec<(&'static str, Variant)> {
    vec![
        ("base.fbx", Variant::Base),
        // Written a second time from an equal document: the same bytes if the
        // writer is a function of the scene, which §22B-1b2 measured.
        ("reexport.fbx", Variant::Base),
        ("renamed.fbx", Variant::RenamedDisplayName),
        (
            "inserted-definition.fbx",
            Variant::InsertedEarlierDefinition,
        ),
        (
            "removed-definition-earlier.fbx",
            Variant::RemovedEarlierDefinition,
        ),
        ("reordered-definitions.fbx", Variant::ReorderedDefinitions),
        ("inserted-sibling.fbx", Variant::InsertedSibling),
        ("removed-sibling.fbx", Variant::RemovedSibling),
        ("reordered-siblings.fbx", Variant::ReorderedSiblings),
        ("removed-tracked-definition.fbx", Variant::RemovedDefinition),
    ]
}

fn main() -> std::process::ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let Some(directory) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: fbx_identity_variants OUTPUT_DIRECTORY");
        return std::process::ExitCode::from(2);
    };
    if arguments.next().is_some() {
        eprintln!("usage: fbx_identity_variants OUTPUT_DIRECTORY");
        return std::process::ExitCode::from(2);
    }

    for (name, variant) in variants() {
        let scene = variant_scene(variant);
        let path = directory.join(name);
        let file = match std::fs::File::create(&path) {
            Ok(file) => file,
            Err(error) => {
                eprintln!("cannot write {}: {error}", path.display());
                return std::process::ExitCode::FAILURE;
            }
        };
        let mut sink = BufWriter::new(file);
        let report = match write_fbx_ascii_7400(&scene, &mut sink) {
            Ok(report) => report,
            Err(error) => {
                eprintln!("cannot write {name}: {error}");
                return std::process::ExitCode::FAILURE;
            }
        };
        if let Err(error) = std::io::Write::flush(&mut sink) {
            eprintln!("cannot finish {name}: {error}");
            return std::process::ExitCode::FAILURE;
        }
        println!(
            "{name} bytes={} models={} geometries={} materials={} complete={}",
            report.bytes(),
            report.models(),
            report.geometries(),
            report.materials(),
            report.is_complete()
        );
    }
    std::process::ExitCode::SUCCESS
}
