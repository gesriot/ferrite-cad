// SPDX-License-Identifier: MIT
//! The `export-fbx` command: what it refuses, what it prints and what it
//! returns.
//!
//! The export itself is not here. It is
//! [`ferritecad_jobs::export_document_as_fbx`], which is the one route from a
//! stored document to a published FBX and is the same route the window takes.
//! What is here is everything that is about *this* interface rather than about
//! the work: which flag authorises a replacement, what a person reads on each
//! stream, and which number the process leaves behind.
//!
//! # Two checks before the kernel is opened
//!
//! Both are courtesies rather than decisions. The job refuses a document that
//! is its own destination on its own account, and publication is what actually
//! decides whether something already there may be replaced. Doing them here as
//! well means a run that was never going to work says so at once, instead of
//! after rebuilding and tessellating a whole assembly.
//!
//! # A partial export is a file, and says so
//!
//! A definition this build cannot turn into triangles keeps its place in the
//! hierarchy, and the export is published. It exits [`EXIT_PARTIAL`] rather
//! than zero, because a script must be able to tell the two apart without
//! reading prose, and rather than failing, because refusing to publish would
//! throw away the forty-five definitions that were fine.
//!
//! The report on standard error is built from [`FbxWriteReport::omissions`]
//! and from nothing else. There is no second list here that could disagree
//! with the file that was published.

use std::process::ExitCode;

use ferritecad_export::{ExportNodeId, ExportOmissionReport, ExportSource, FbxWriteReport};
use ferritecad_jobs::{
    FbxExport, FbxExportRequest, SOURCE_IS_DESTINATION, export_document_as_fbx, path_entry_exists,
    refuse_source_as_destination,
};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{CadError, Result};

use crate::{EXIT_PARTIAL, ExportFbxArgs, REPLACE_ADVICE, replacing};

pub fn export_fbx(args: ExportFbxArgs) -> Result<ExitCode> {
    // This check takes precedence over the ordinary no-clobber message: the
    // document is not a destination `--force` can ever make acceptable.
    refuse_source_as_destination(&args.path, &args.output, SOURCE_IS_DESTINATION)?;

    // Checked before the kernel is opened. Rebuilding and tessellating a whole
    // assembly only to refuse at the last step wastes the user's time and tells
    // them nothing they could not have been told at once. It is not the only
    // check: a file that appears while the work is going on is refused at
    // publication, where the decision is atomic.
    if !args.force && path_entry_exists(&args.output)? {
        return Err(CadError::input(format!(
            "{} already exists; {REPLACE_ADVICE}",
            args.output.display()
        )));
    }

    // The session belongs to this process and ends with it. Everything after
    // this line is the shared job, which is what the window runs too.
    let mut kernel = OcctKernel::new()?;
    let exported = export_document_as_fbx(
        FbxExportRequest::new(&args.path, &args.output, replacing(args.force)),
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        &OperationContext::default(),
    )?;

    print!("{}", summary(&args, &exported));
    let report = exported.report();
    if !report.is_complete() {
        eprint!("{}", omission_report(report.omissions()));
    }
    Ok(ExitCode::from(exit_code(report)))
}

/// What a finished export is worth to whatever ran it.
///
/// A partial export is neither of the two things it is easiest to turn it into.
/// It is not a success: the file does not describe the whole document. It is
/// not a failure either: the file is real, it was published, and every
/// definition the document holds kept its place in it. So it has a code of its
/// own, and the choice lives in one place rather than being spelled out at
/// every return.
fn exit_code(report: &FbxWriteReport) -> u8 {
    if report.is_complete() {
        0
    } else {
        EXIT_PARTIAL
    }
}

/// What was written, in terms that do not depend on how long it took.
fn summary(args: &ExportFbxArgs, exported: &FbxExport) -> String {
    use std::fmt::Write as _;

    let report = exported.report();
    let mut out = String::new();
    writeln!(
        out,
        "wrote {} from {}",
        exported.destination().display(),
        args.path.display()
    )
    .expect("writing to a String cannot fail");
    writeln!(out, "  FBX 7.4.0 ASCII, {} byte(s)", report.bytes()).expect("cannot fail");
    writeln!(
        out,
        "  {} node{}, {} geometry object{}, {} material{}",
        report.models(),
        plural(report.models() as usize),
        report.geometries(),
        plural(report.geometries() as usize),
        report.materials(),
        plural(report.materials() as usize)
    )
    .expect("cannot fail");

    let missing = report.omissions().len();
    if missing == 0 {
        // The whole contract of a clean export, stated here and nowhere else.
        // Deliberately without the vocabulary of the report below: a person
        // who greps their build log for what went wrong must not find this.
        writeln!(
            out,
            "  complete: nothing this document holds was left out of the file"
        )
        .expect("cannot fail");
    } else {
        writeln!(
            out,
            "  {missing} definition{} could not be given triangles; the file says so and so does \
             standard error",
            plural(missing)
        )
        .expect("cannot fail");
    }
    out
}

/// What could not be written, from the writer's own record of it.
///
/// Built from [`FbxWriteReport::omissions`] and from nothing else. Working the
/// list out again from the scene would be a second opinion about a question the
/// writer has already answered, and the first thing a second opinion can do is
/// disagree with the file that was published.
///
/// Every omission is printed, in the order the writer holds them, which is
/// scene order. Nothing is collapsed: two definitions are two entries even when
/// everything a person can read about them is the same.
fn omission_report(omissions: &[ExportOmissionReport]) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    writeln!(
        out,
        "partial export: {} definition{} could not be given triangles; each kept its place in \
         the file",
        omissions.len(),
        plural(omissions.len())
    )
    .expect("writing to a String cannot fail");

    for (index, report) in omissions.iter().enumerate() {
        writeln!(out).expect("cannot fail");
        writeln!(out, "  omission {} of {}", index + 1, omissions.len()).expect("cannot fail");
        writeln!(out, "    definition  {}", identity(&report.source)).expect("cannot fail");
        // What the document recorded when the file was imported, with the
        // stage, the severity, the entity and the message it recorded. A
        // historical warning cannot excuse an unrelated failure now, so it is
        // printed beside the refusal rather than instead of it.
        writeln!(out, "    finding     {}", report.omission.finding).expect("cannot fail");
        // The typed answer this build's kernel gave, by its stable name. Not
        // its message, which is written for a person and free to change, and
        // not its `Debug` rendering, which is a debugging aid and not a fact.
        writeln!(
            out,
            "    refusal     {}",
            report.omission.refusal.stable_name()
        )
        .expect("cannot fail");
        writeln!(
            out,
            "    placements  {} in the file: {}",
            report.nodes.len(),
            node_keys(&report.nodes)
        )
        .expect("cannot fail");
    }
    out
}

/// One definition's durable identity, qualified by where it came from.
///
/// The source identifier travels with the key because a key means nothing
/// without it: `#2583` occurs in most STEP files and names something different
/// in each. Two omissions that share a local key and came from two sources are
/// two omissions, and must read as two.
fn identity(source: &ExportSource) -> String {
    match source {
        ExportSource::Body { object } => format!("body {object}"),
        ExportSource::Imported {
            source,
            definition_key,
        } => format!("imported source {source}  key {definition_key}"),
    }
}

/// Every placement of an omitted definition, named the way the file names it.
///
/// `node/<n>` is not a number that means something only while this process
/// runs: it is exactly the `FerriteCADNodeKey` property the writer put on that
/// model, so a person holding this report can find the node in the file they
/// were just given.
fn node_keys(nodes: &[ExportNodeId]) -> String {
    nodes
        .iter()
        .map(|node| format!("node/{}", node.index()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use super::*;
    use ferritecad_exchange::{Diagnostic, Severity, Stage};
    use ferritecad_export::{
        ExportColourOrigin, ExportGeometry, ExportMaterial, ExportMesh, ExportOccurrence,
        ExportOmission, ExportProvenance, ExportScene, ExportSceneBuilder, ExportTransform,
    };
    use ferritecad_kernel::TessellationRefusal;
    use ferritecad_types::{ImportedSourceId, ObjectId};

    /// One triangle, one slot, in whatever colour is asked for.
    fn mesh(colour: [f64; 3]) -> ExportMesh {
        ExportMesh::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![[0.0, 0.0, 1.0]; 3],
            vec![[0, 1, 2]],
            vec![0],
            vec![
                ExportMaterial::new("slot".to_owned(), colour, ExportColourOrigin::Default)
                    .expect("a material in range"),
            ],
        )
        .expect("a mesh that holds together")
    }

    fn omission() -> ExportGeometry {
        ExportGeometry::Omitted(ExportOmission::new(
            Diagnostic {
                stage: Stage::Validation,
                severity: Severity::Warning,
                entity: "step.product_definition#2583".to_owned(),
                message: "the solid is not valid".to_owned(),
            },
            TessellationRefusal::IncompleteFace,
        ))
    }

    /// A scene of one definition with one placement, called `name`.
    fn one_node(geometry: ExportGeometry, name: &str) -> ExportScene {
        let mut builder = ExportSceneBuilder::new();
        let definition = builder
            .definition(
                ExportSource::Body {
                    object: ObjectId::new(),
                },
                Some("part".to_owned()),
                ExportProvenance::default(),
                geometry,
            )
            .expect("a definition");
        builder
            .node(
                None,
                definition,
                ExportTransform::IDENTITY,
                Some(name.to_owned()),
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("a placement");
        builder.finish().expect("a scene")
    }

    /// The writer's own record of a scene, with no file anywhere.
    ///
    /// What this file decides is the number and the words, and both are
    /// functions of that record. Where the bytes go is the job's business and
    /// is gated where the job lives.
    fn written(scene: &ExportScene) -> FbxWriteReport {
        ferritecad_export::write_fbx_ascii_7400(scene, &mut Vec::new()).expect("writes")
    }

    /// Whether the file is the whole document decides the number, and
    /// nothing else does.
    #[test]
    fn a_partial_export_is_not_a_success_and_is_not_a_failure() {
        let report = written(&one_node(omission(), "frame"));

        assert!(!report.is_complete());
        assert_eq!(report.omissions().len(), 1);
        assert_eq!(exit_code(&report), EXIT_PARTIAL);
        assert_eq!(EXIT_PARTIAL, 6);
        assert_ne!(exit_code(&report), 0);
    }

    #[test]
    fn a_complete_export_is_a_plain_success() {
        let report = written(&one_node(
            ExportGeometry::Mesh(mesh([0.5, 0.5, 0.5])),
            "part",
        ));

        assert!(report.is_complete());
        assert_eq!(exit_code(&report), 0);
    }

    /// Two files may both call a definition `#31`, and they are not one thing.
    #[test]
    fn two_sources_with_one_local_key_are_two_entries_in_the_report() {
        let first = ImportedSourceId::new();
        let second = ImportedSourceId::new();
        assert_ne!(first, second);

        let mut builder = ExportSceneBuilder::new();
        for source in [first, second] {
            let definition = builder
                .definition(
                    ExportSource::Imported {
                        source,
                        definition_key: "step.product_definition#31".to_owned(),
                    },
                    Some("part".to_owned()),
                    ExportProvenance::default(),
                    omission(),
                )
                .expect("a definition");
            builder
                .node(
                    None,
                    definition,
                    ExportTransform::IDENTITY,
                    Some("part".to_owned()),
                    None,
                    ExportOccurrence::Unrecorded,
                )
                .expect("a placement");
        }
        let scene = builder.finish().expect("a scene");

        let written = omission_report(scene.completeness().omissions());
        assert!(written.contains("omission 1 of 2"), "{written}");
        assert!(written.contains("omission 2 of 2"), "{written}");
        assert!(
            written.contains(&first.to_string()) && written.contains(&second.to_string()),
            "the two sources were collapsed into one:\n{written}"
        );
        assert_eq!(
            written.matches("step.product_definition#31").count(),
            2,
            "one local key stood for two definitions:\n{written}"
        );
    }

    /// Several omissions are several entries, in the order the scene holds
    /// them. A report that stopped at the first would describe a smaller
    /// problem than the one the user has.
    #[test]
    fn every_omission_is_reported_in_a_stable_order() {
        let mut builder = ExportSceneBuilder::new();
        let keys = ["#11", "#22", "#33"];
        let source = ImportedSourceId::new();
        for key in keys {
            let definition = builder
                .definition(
                    ExportSource::Imported {
                        source,
                        definition_key: key.to_owned(),
                    },
                    Some(key.to_owned()),
                    ExportProvenance::default(),
                    omission(),
                )
                .expect("a definition");
            for _ in 0..2 {
                builder
                    .node(
                        None,
                        definition,
                        ExportTransform::IDENTITY,
                        Some(key.to_owned()),
                        None,
                        ExportOccurrence::Unrecorded,
                    )
                    .expect("a placement");
            }
        }
        let scene = builder.finish().expect("a scene");

        let written = omission_report(scene.completeness().omissions());
        let at = |needle: &str| {
            written
                .find(needle)
                .unwrap_or_else(|| panic!("{needle} is not in the report:\n{written}"))
        };
        assert!(
            written.contains("3 definitions could not be given triangles"),
            "{written}"
        );
        assert!(
            at(keys[0]) < at(keys[1]) && at(keys[1]) < at(keys[2]),
            "{written}"
        );
        assert!(at("omission 1 of 3") < at("omission 2 of 3"), "{written}");
        assert!(at("omission 2 of 3") < at("omission 3 of 3"), "{written}");
        // Six placements across three definitions, none of them dropped.
        assert_eq!(
            written.matches("placements  2 in the file").count(),
            3,
            "{written}"
        );
        for node in 0..6 {
            assert!(
                written.contains(&format!("node/{node}")),
                "placement node/{node} is missing:\n{written}"
            );
        }
    }

    #[test]
    fn the_report_carries_the_typed_refusal_and_not_a_rendering_of_it() {
        let scene = one_node(omission(), "frame");
        let written = omission_report(scene.completeness().omissions());

        assert!(
            written.contains("refusal     IncompleteFace"),
            "the stable name of the refusal is missing:\n{written}"
        );
        assert!(
            !written.contains("one or more faces have no usable triangles"),
            "the report used the refusal's message as data:\n{written}"
        );
        assert!(
            !written.contains("IncompleteFace,") && !written.contains("Diagnostic {"),
            "the report used a Debug rendering as data:\n{written}"
        );
        // The persisted finding, with everything the document recorded in it.
        assert!(
            written.contains("step.product_definition#2583")
                && written.contains("the solid is not valid")
                && written.contains("warning")
                && written.contains("validating"),
            "the persisted finding lost something:\n{written}"
        );
        assert!(
            written.contains("placements  1 in the file: node/0"),
            "the affected placements are missing:\n{written}"
        );
    }
}
