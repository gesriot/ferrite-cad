// SPDX-License-Identifier: MIT
//! Writes the §22B-1e2a base documents with the production FBX writer.
//!
//! This example produces the *control* bytes and nothing else: every file it
//! writes comes from `write_fbx_ascii_7400` exactly as the shipped route calls
//! it, so candidate A of the measurement is the current product and not a
//! reconstruction of it. The candidate channels B, C and D are made by the
//! measurement-only rewriter beside the harness, which is handed these bytes
//! and the manifest written here; nothing in this file writes an FBX by hand
//! and nothing here is a second serializer.
//!
//! The manifest is the half the writer does not put in the file. The current
//! `FerriteCADDefinitionKey` property carries only the source-local key, and
//! FerriteCAD persists no occurrence identity at all, so the source a
//! definition came from and the synthetic persistent occurrence identity used
//! by the placement experiment are written out beside the bytes rather than
//! into them. Both are measurement-only: no schema, no capability and no
//! writer changes here.
//!
//! Two definitions in these documents deliberately carry the *same*
//! source-local key `step.product_definition#42` under two different
//! `ImportedSourceId`s. That is the collision the current property cannot
//! express, and it is in the base document so no scenario can pass because
//! the confusion was never there.

// The measured scene's placement helper is defined once, beside the §22B-1b2
// gate, and is reused here rather than copied.
#[path = "../tests/fbx_scene/mod.rs"]
mod fbx_scene;

use std::fmt::Write as _;
use std::io::BufWriter;
use std::path::PathBuf;

use ferritecad_exchange::{Diagnostic, Severity, Stage};
use ferritecad_export::{
    ExportColourOrigin, ExportGeometry, ExportMaterial, ExportMesh, ExportOmission,
    ExportProvenance, ExportScene, ExportSceneBuilder, ExportSource, ExportTransform,
    write_fbx_ascii_7400,
};
use ferritecad_kernel::TessellationRefusal;
use ferritecad_types::ImportedSourceId;

use fbx_scene::placement;

/// The two imported sources. Two, because one is exactly what §22B-1e1 could
/// measure and exactly what left the general definition join open.
const FIRST_SOURCE: &str = "019ffc72-2996-7000-8000-0000000000a1";
const SECOND_SOURCE: &str = "019ffc72-2996-7000-8000-0000000000b2";

/// The source-local keys. `TWIN_KEY` is used under both sources on purpose.
const ROOT_KEY: &str = "step.product_definition#1";
const FRAME_KEY: &str = "step.product_definition#2";
const INSERTED_KEY: &str = "step.product_definition#10";
const TWIN_KEY: &str = "step.product_definition#42";
const EARLY_KEY: &str = "step.product_definition#50";
const ALPHA_KEY: &str = "step.product_definition#100";
const BETA_KEY: &str = "step.product_definition#200";
const GAMMA_KEY: &str = "step.product_definition#300";
const OMITTED_KEY: &str = "step.product_definition#400";

/// The synthetic persistent occurrence identities.
///
/// FerriteCAD persists nothing of the kind today: a placement's identity is
/// "the n-th placement of this definition in scene order", which §22B-1e1
/// recorded as an honest limit. These exist only inside this measurement, and
/// only so the placement experiment can compare an ordinal against a durable
/// identity instead of assuming which one would work.
const ROOT_OCCURRENCE: &str = "019ffc72-2996-7000-9000-000000000001";
const EARLY_OCCURRENCE: &str = "019ffc72-2996-7000-9000-000000000002";
const ALPHA_FIRST_OCCURRENCE: &str = "019ffc72-2996-7000-9000-000000000003";
const ALPHA_SECOND_OCCURRENCE: &str = "019ffc72-2996-7000-9000-000000000004";
const BETA_OCCURRENCE: &str = "019ffc72-2996-7000-9000-000000000005";
const GAMMA_OCCURRENCE: &str = "019ffc72-2996-7000-9000-000000000006";
const FIRST_TWIN_OCCURRENCE: &str = "019ffc72-2996-7000-9000-000000000007";
const SECOND_TWIN_OCCURRENCE: &str = "019ffc72-2996-7000-9000-000000000008";
const OMITTED_OCCURRENCE: &str = "019ffc72-2996-7000-9000-000000000009";
const FRAME_OCCURRENCE: &str = "019ffc72-2996-7000-9000-00000000000a";
const NESTED_BETA_OCCURRENCE: &str = "019ffc72-2996-7000-9000-00000000000b";
const INSERTED_OCCURRENCE: &str = "019ffc72-2996-7000-9000-00000000000c";
const INSERTED_SIBLING_OCCURRENCE: &str = "019ffc72-2996-7000-9000-00000000000d";

/// The one change each document carries, relative to the base document.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    /// The document every other one is one change away from.
    Base,
    /// One display name moves; every durable identity stays.
    RenamedDisplayName,
    /// An unrelated definition appears before the tracked ones.
    InsertedDefinition,
    /// An unrelated earlier definition goes away, with its only placement.
    RemovedDefinition,
    /// Two unrelated definitions swap places in export order.
    ReorderedDefinitions,
    /// A sibling occurrence appears between two tracked ones.
    InsertedSibling,
    /// A sibling occurrence goes away; its definition stays placed elsewhere.
    RemovedSibling,
    /// Two sibling occurrences swap places.
    ReorderedSiblings,
    /// One whole definition and its only placement go away. Its designation is
    /// shared with a definition that stays, so a retarget would be silent.
    RemovedTrackedDefinition,
    /// One material slot changes its colour and its slot designation.
    ChangedMaterial,
    /// A second definition gains a slot with the same designation and colour
    /// as another definition's, which a reader that merges by name would fold.
    ReusedMaterial,
}

/// What the writer does not put in the file, recorded beside the bytes.
struct NodeFacts {
    node_key: String,
    source: &'static str,
    definition_key: &'static str,
    /// The source-qualified identity a durable join would need. Assembled
    /// here, not exported: the production property carries only the second
    /// half of it.
    definition_id: String,
    occurrence: &'static str,
    definition_display_name: String,
    node_display_name: String,
    geometry: &'static str,
    slots: Vec<String>,
}

/// A mesh whose vertex count alone says which definition it came from.
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
        .expect("a measured channel mesh is valid")
}

fn slot(name: &str, colour: [f64; 3]) -> ExportMaterial {
    ExportMaterial::new(name, colour, ExportColourOrigin::Source).expect("a linear colour")
}

fn omission(key: &str) -> ExportGeometry {
    ExportGeometry::Omitted(ExportOmission::new(
        Diagnostic {
            stage: Stage::Validation,
            severity: Severity::Fail,
            entity: key.to_owned(),
            message: "the imported definition contains an invalid solid".to_owned(),
        },
        TessellationRefusal::IncompleteFace,
    ))
}

fn source_of(text: &str) -> ImportedSourceId {
    text.parse().expect("a measured source is an RFC 4122 UUID")
}

/// One definition as this measurement describes it, before the builder sees it.
struct Described {
    source: &'static str,
    key: &'static str,
    display_name: String,
    geometry: &'static str,
    slots: Vec<String>,
}

/// Builds one document and the facts the rewriter needs about it.
///
/// The two halves are produced together on purpose: a manifest assembled by a
/// second pass over the finished scene could describe a document the writer
/// never saw.
fn document(variant: Variant) -> (ExportScene, Vec<NodeFacts>) {
    let first = source_of(FIRST_SOURCE);
    let second = source_of(SECOND_SOURCE);
    let provenance = ExportProvenance::default();
    let mut builder = ExportSceneBuilder::new();
    let mut described: Vec<Described> = Vec::new();
    let mut facts: Vec<NodeFacts> = Vec::new();

    let alpha_name = if variant == Variant::RenamedDisplayName {
        "Alpha Part Renamed"
    } else {
        "Alpha Part"
    };
    // The changed slot moves both its designation and its colour, so a reader
    // that merged on either one alone would still see a change.
    let (alpha_second_slot, alpha_second_colour) = if variant == Variant::ChangedMaterial {
        ("Shell Blue", [0.010_023, 0.100_482, 0.787_412])
    } else {
        ("Shell", [0.100_482, 0.010_023, 0.787_412])
    };

    let add = |builder: &mut ExportSceneBuilder,
               described: &mut Vec<Described>,
               source: &'static str,
               key: &'static str,
               display_name: &str,
               geometry: ExportGeometry,
               slots: Vec<String>| {
        let kind = match &geometry {
            ExportGeometry::Mesh(_) => "mesh",
            ExportGeometry::Structural => "structural",
            ExportGeometry::Omitted(_) => "omitted",
        };
        let id = builder
            .definition(
                ExportSource::Imported {
                    source: source_of(source),
                    definition_key: key.to_owned(),
                },
                Some(display_name.to_owned()),
                provenance.clone(),
                geometry,
            )
            .expect("a measured channel definition");
        described.push(Described {
            source,
            key,
            display_name: display_name.to_owned(),
            geometry: kind,
            slots,
        });
        id
    };

    let root = add(
        &mut builder,
        &mut described,
        FIRST_SOURCE,
        ROOT_KEY,
        "Assembly Root",
        ExportGeometry::Structural,
        Vec::new(),
    );
    let inserted = (variant == Variant::InsertedDefinition).then(|| {
        add(
            &mut builder,
            &mut described,
            FIRST_SOURCE,
            INSERTED_KEY,
            "Inserted Part",
            ExportGeometry::Mesh(mesh(7, vec![slot("Inserted", [0.5, 0.5, 0.5])])),
            vec!["Inserted".to_owned()],
        )
    });
    let early = (variant != Variant::RemovedDefinition).then(|| {
        add(
            &mut builder,
            &mut described,
            FIRST_SOURCE,
            EARLY_KEY,
            "Early Part",
            ExportGeometry::Mesh(mesh(3, vec![slot("Early", [0.9, 0.1, 0.1])])),
            vec!["Early".to_owned()],
        )
    });
    let alpha = add(
        &mut builder,
        &mut described,
        FIRST_SOURCE,
        ALPHA_KEY,
        alpha_name,
        ExportGeometry::Mesh(mesh(
            4,
            vec![
                slot("Shell", [0.603_827, 0.033_105, 0.010_023]),
                slot(alpha_second_slot, alpha_second_colour),
            ],
        )),
        vec!["Shell".to_owned(), alpha_second_slot.to_owned()],
    );

    // Beta gains a second slot that repeats Alpha's designation and colour in
    // one variant, which is the material-reuse scenario.
    let beta_of = |builder: &mut ExportSceneBuilder, described: &mut Vec<Described>| {
        let (slots, names) = if variant == Variant::ReusedMaterial {
            (
                vec![
                    slot("Beta", [0.1, 0.2, 0.9]),
                    slot("Shell", [0.603_827, 0.033_105, 0.010_023]),
                ],
                vec!["Beta".to_owned(), "Shell".to_owned()],
            )
        } else {
            (vec![slot("Beta", [0.1, 0.2, 0.9])], vec!["Beta".to_owned()])
        };
        add(
            builder,
            described,
            FIRST_SOURCE,
            BETA_KEY,
            "Beta Part",
            ExportGeometry::Mesh(mesh(5, slots)),
            names,
        )
    };
    let gamma_of = |builder: &mut ExportSceneBuilder, described: &mut Vec<Described>| {
        add(
            builder,
            described,
            FIRST_SOURCE,
            GAMMA_KEY,
            // Deliberately the designation Alpha carries in the base document.
            "Alpha Part",
            ExportGeometry::Mesh(mesh(6, vec![slot("Gamma", [0.2, 0.7, 0.3])])),
            vec!["Gamma".to_owned()],
        )
    };

    let (beta, gamma) = if variant == Variant::ReorderedDefinitions {
        let gamma = gamma_of(&mut builder, &mut described);
        let beta = beta_of(&mut builder, &mut described);
        (beta, Some(gamma))
    } else {
        let beta = beta_of(&mut builder, &mut described);
        let gamma = (variant != Variant::RemovedTrackedDefinition)
            .then(|| gamma_of(&mut builder, &mut described));
        (beta, gamma)
    };

    // The pair the whole slice is about: one source-local key, two sources.
    let first_twin = add(
        &mut builder,
        &mut described,
        FIRST_SOURCE,
        TWIN_KEY,
        "Twin Part",
        ExportGeometry::Mesh(mesh(8, vec![slot("Twin", [0.8, 0.3, 0.1])])),
        vec!["Twin".to_owned()],
    );
    let second_twin = add(
        &mut builder,
        &mut described,
        SECOND_SOURCE,
        TWIN_KEY,
        "Twin Part",
        ExportGeometry::Mesh(mesh(9, vec![slot("Twin", [0.1, 0.3, 0.8])])),
        vec!["Twin".to_owned()],
    );

    let omitted = add(
        &mut builder,
        &mut described,
        FIRST_SOURCE,
        OMITTED_KEY,
        "Omitted Part",
        omission(OMITTED_KEY),
        Vec::new(),
    );
    let frame = add(
        &mut builder,
        &mut described,
        FIRST_SOURCE,
        FRAME_KEY,
        "Sub Frame",
        ExportGeometry::Structural,
        Vec::new(),
    );

    // ------------------------------------------------------------ the nodes
    let _ = first;
    let _ = second;
    let place = |builder: &mut ExportSceneBuilder,
                 facts: &mut Vec<NodeFacts>,
                 parent: Option<ferritecad_export::ExportNodeId>,
                 definition: ferritecad_export::ExportDefinitionId,
                 transform: ExportTransform,
                 display_name: &str,
                 occurrence: &'static str| {
        let id = builder
            .node(
                parent,
                definition,
                transform,
                Some(display_name.to_owned()),
                None,
            )
            .expect("a measured channel placement");
        let about = &described[definition.index()];
        facts.push(NodeFacts {
            node_key: format!("node/{}", id.index()),
            source: about.source,
            definition_key: about.key,
            definition_id: format!("{}/{}", about.source, about.key),
            occurrence,
            definition_display_name: about.display_name.clone(),
            node_display_name: display_name.to_owned(),
            geometry: about.geometry,
            slots: about.slots.clone(),
        });
        id
    };

    let root_node = place(
        &mut builder,
        &mut facts,
        None,
        root,
        ExportTransform::IDENTITY,
        "Assembly Root",
        ROOT_OCCURRENCE,
    );
    let under = Some(root_node);

    if let Some(inserted) = inserted {
        place(
            &mut builder,
            &mut facts,
            under,
            inserted,
            placement([10.0, 20.0, 30.0], [0.0, 0.0, 0.0]),
            "Inserted Part",
            INSERTED_OCCURRENCE,
        );
    }
    if let Some(early) = early {
        place(
            &mut builder,
            &mut facts,
            under,
            early,
            placement([100.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
            "Early Part",
            EARLY_OCCURRENCE,
        );
    }
    // Two placements of one definition, sharing one geometry and one
    // designation.
    place(
        &mut builder,
        &mut facts,
        under,
        alpha,
        placement([200.0, 0.0, 0.0], [11.0, 0.0, 0.0]),
        alpha_name,
        ALPHA_FIRST_OCCURRENCE,
    );
    if variant == Variant::InsertedSibling {
        place(
            &mut builder,
            &mut facts,
            under,
            beta,
            placement([250.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
            "Beta Part",
            INSERTED_SIBLING_OCCURRENCE,
        );
    }
    place(
        &mut builder,
        &mut facts,
        under,
        alpha,
        placement([300.0, 0.0, 0.0], [0.0, 17.0, 0.0]),
        alpha_name,
        ALPHA_SECOND_OCCURRENCE,
    );

    let beta_placement = |builder: &mut ExportSceneBuilder, facts: &mut Vec<NodeFacts>| {
        place(
            builder,
            facts,
            under,
            beta,
            placement([400.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
            "Beta Part",
            BETA_OCCURRENCE,
        );
    };
    let gamma_placement = |builder: &mut ExportSceneBuilder, facts: &mut Vec<NodeFacts>| {
        if let Some(gamma) = gamma {
            place(
                builder,
                facts,
                under,
                gamma,
                placement([600.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
                // The designation Alpha carries: removing this node must make
                // a reference missing rather than move it onto Alpha.
                "Alpha Part",
                GAMMA_OCCURRENCE,
            );
        }
    };
    if variant == Variant::ReorderedSiblings {
        gamma_placement(&mut builder, &mut facts);
        beta_placement(&mut builder, &mut facts);
    } else {
        beta_placement(&mut builder, &mut facts);
        gamma_placement(&mut builder, &mut facts);
    }

    place(
        &mut builder,
        &mut facts,
        under,
        first_twin,
        placement([700.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        "Twin Part",
        FIRST_TWIN_OCCURRENCE,
    );
    place(
        &mut builder,
        &mut facts,
        under,
        second_twin,
        placement([800.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        "Twin Part",
        SECOND_TWIN_OCCURRENCE,
    );
    place(
        &mut builder,
        &mut facts,
        under,
        omitted,
        placement([900.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        "Omitted Part",
        OMITTED_OCCURRENCE,
    );

    // A structural frame with a nested placement below it, so the measurement
    // has a placement that is not a child of the root.
    let frame_node = place(
        &mut builder,
        &mut facts,
        under,
        frame,
        placement([1000.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        "Sub Frame",
        FRAME_OCCURRENCE,
    );
    if variant != Variant::RemovedSibling {
        place(
            &mut builder,
            &mut facts,
            Some(frame_node),
            beta,
            placement([50.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
            "Beta Part",
            NESTED_BETA_OCCURRENCE,
        );
    }

    (
        builder.finish().expect("a measured channel document"),
        facts,
    )
}

/// Every document, and the file it is written to.
fn documents() -> Vec<(&'static str, Variant)> {
    vec![
        ("base.fbx", Variant::Base),
        // Written a second time from an equal document. The writer is a
        // function of the scene, so this must be the same bytes; that is
        // checked by the harness rather than assumed here.
        ("reexport.fbx", Variant::Base),
        ("renamed.fbx", Variant::RenamedDisplayName),
        ("inserted-definition.fbx", Variant::InsertedDefinition),
        ("removed-definition.fbx", Variant::RemovedDefinition),
        ("reordered-definitions.fbx", Variant::ReorderedDefinitions),
        ("inserted-sibling.fbx", Variant::InsertedSibling),
        ("removed-sibling.fbx", Variant::RemovedSibling),
        ("reordered-siblings.fbx", Variant::ReorderedSiblings),
        (
            "removed-tracked-definition.fbx",
            Variant::RemovedTrackedDefinition,
        ),
        ("changed-material.fbx", Variant::ChangedMaterial),
        ("reused-material.fbx", Variant::ReusedMaterial),
    ]
}

fn escape(value: &str, into: &mut String) {
    into.push('"');
    for character in value.chars() {
        match character {
            '"' => into.push_str("\\\""),
            '\\' => into.push_str("\\\\"),
            '\n' => into.push_str("\\n"),
            '\r' => into.push_str("\\r"),
            '\t' => into.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                let _ = write!(into, "\\u{:04x}", control as u32);
            }
            other => into.push(other),
        }
    }
    into.push('"');
}

fn manifest(entries: &[(&str, Vec<NodeFacts>)]) -> String {
    let mut text = String::new();
    text.push_str("{\n \"schema\": \"ferritecad.fbx-channel-manifest.v1\",\n");
    text.push_str(
        " \"generator\": \"fbx_channel_documents example over write_fbx_ascii_7400\",\n \"documents\": [\n",
    );
    for (index, (file, facts)) in entries.iter().enumerate() {
        if index > 0 {
            text.push_str(",\n");
        }
        text.push_str("  {\"file\": ");
        escape(file, &mut text);
        text.push_str(", \"nodes\": [\n");
        for (position, node) in facts.iter().enumerate() {
            if position > 0 {
                text.push_str(",\n");
            }
            text.push_str("   {\"node_key\": ");
            escape(&node.node_key, &mut text);
            text.push_str(", \"source\": ");
            escape(node.source, &mut text);
            text.push_str(", \"definition_key\": ");
            escape(node.definition_key, &mut text);
            text.push_str(", \"definition_id\": ");
            escape(&node.definition_id, &mut text);
            text.push_str(", \"occurrence\": ");
            escape(node.occurrence, &mut text);
            text.push_str(", \"definition_display_name\": ");
            escape(&node.definition_display_name, &mut text);
            text.push_str(", \"node_display_name\": ");
            escape(&node.node_display_name, &mut text);
            text.push_str(", \"geometry\": ");
            escape(node.geometry, &mut text);
            text.push_str(", \"slots\": [");
            for (slot_index, name) in node.slots.iter().enumerate() {
                if slot_index > 0 {
                    text.push_str(", ");
                }
                escape(name, &mut text);
            }
            text.push_str("]}");
        }
        text.push_str("\n  ]}");
    }
    text.push_str("\n ]\n}\n");
    text
}

fn main() -> std::process::ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let Some(directory) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: fbx_channel_documents OUTPUT_DIRECTORY");
        return std::process::ExitCode::from(2);
    };
    if arguments.next().is_some() {
        eprintln!("usage: fbx_channel_documents OUTPUT_DIRECTORY");
        return std::process::ExitCode::from(2);
    }

    let mut entries: Vec<(&str, Vec<NodeFacts>)> = Vec::new();
    for (name, variant) in documents() {
        let (scene, facts) = document(variant);
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
        entries.push((name, facts));
    }

    let path = directory.join("manifest.json");
    if let Err(error) = std::fs::write(&path, manifest(&entries)) {
        eprintln!("cannot write {}: {error}", path.display());
        return std::process::ExitCode::FAILURE;
    }
    println!("manifest.json documents={}", entries.len());
    std::process::ExitCode::SUCCESS
}
