// SPDX-License-Identifier: MIT
//! Writing a whole document out as FBX, and saying what did not fit.
//!
//! The one route from a stored document to a file another program opens as a
//! model rather than as a lump of triangles: the hierarchy, the placements and
//! the definitions survive, because [`export_scene`] keeps what a picture
//! throws away and [`write_fbx_ascii_7400`] is handed that and nothing else.
//!
//! # Nothing here is a second opinion
//!
//! One read of the document, one cold rebuild, one reading of each stored STEP
//! source and one call to the writer. This module never reopens the document,
//! never asks the kernel for more geometry, never touches the STEP file the
//! document was imported from — it no longer has to exist — and never works
//! out for itself what the export left behind. That last one matters most: the
//! report on standard error is built from [`FbxWriteReport::omissions`] and
//! from nothing else, so there is no second list that could disagree with the
//! file.
//!
//! # A partial export is a file, and says so
//!
//! A definition this build cannot turn into triangles keeps its place in the
//! hierarchy, and the export is published. It exits [`EXIT_PARTIAL`] rather
//! than zero, because a script must be able to tell the two apart without
//! reading prose, and rather than failing, because refusing to publish would
//! throw away the forty-five definitions that were fine.

use std::path::Path;
use std::process::ExitCode;

use ferritecad_export::{
    ExportNodeId, ExportOmissionReport, ExportScene, ExportSource, FbxWriteReport,
    write_fbx_ascii_7400,
};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_scene::export_scene;
use ferritecad_types::{CadError, Result};

use crate::publish::{Temporary, path_entry_exists, refuse_source_as_destination};
use crate::{EXIT_PARTIAL, ExportFbxArgs};

pub fn export_fbx(args: ExportFbxArgs) -> Result<ExitCode> {
    // This check takes precedence over the ordinary no-clobber message: the
    // document is not a destination `--force` can ever make acceptable.
    refuse_source_as_destination(
        &args.path,
        &args.output,
        "the native document cannot also be the FBX output",
    )?;

    // Checked before the kernel is opened. Rebuilding and tessellating a whole
    // assembly only to refuse at the last step wastes the user's time and tells
    // them nothing they could not have been told at once. It is not the only
    // check: a file that appears while the work is going on is refused at
    // publication, where the decision is atomic.
    if !args.force && path_entry_exists(&args.output)? {
        return Err(CadError::input(format!(
            "{} already exists; pass --force to replace it",
            args.output.display()
        )));
    }

    // Cold on purpose, and the same reading a picture is built from with the
    // hierarchy kept. The stored STEP bytes are what an imported definition is
    // read from, so an export needs no file beside the document. Every shape
    // this makes is released before it returns, whatever happens.
    let mut kernel = OcctKernel::new()?;
    let scene = export_scene(
        &args.path,
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        &OperationContext::default(),
    )?;

    let report = write_and_publish(&args.output, &scene, args.force)?;

    print!("{}", summary(&args, &report));
    if !report.is_complete() {
        eprint!("{}", omission_report(report.omissions()));
    }
    Ok(ExitCode::from(exit_code(&report)))
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

/// Writes the scene into a scratch file beside the destination and publishes
/// it once, after the writer has finished.
///
/// See [`crate::publish`] for why the scratch file lives where it does and what
/// makes the last step atomic. Streamed rather than built in memory first: the
/// complex assembly's FBX is hundreds of megabytes, almost all of it vertex and
/// normal arrays, and holding a second copy of that would be the difference
/// between a large export and one that cannot be done at all.
fn write_and_publish(
    destination: &Path,
    scene: &ExportScene,
    force: bool,
) -> Result<FbxWriteReport> {
    let temporary = Temporary::beside(destination)?;

    // `create_new`, so this cannot open something already at that name and
    // cannot follow a symlink to somewhere else.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.path())
        .map_err(|e| CadError::io(format!("creating {}", temporary.path().display()), e))?;

    let mut sink = std::io::BufWriter::with_capacity(1 << 20, file);
    // The one call. A writer invoked twice is a writer whose second file could
    // differ from the report describing the first.
    let report = write_fbx_ascii_7400(scene, &mut sink)?;

    // `into_inner` flushes what the buffer still holds and hands back the
    // error rather than swallowing it in a drop.
    let file = sink.into_inner().map_err(|error| {
        CadError::io(
            format!("writing {}", temporary.path().display()),
            error.into_error(),
        )
    })?;
    file.sync_all()
        .map_err(|e| CadError::io(format!("syncing {}", temporary.path().display()), e))?;
    drop(file);

    // Only now. Nothing is at the destination until the writer has said it
    // finished and every byte of what it wrote is on the disk.
    temporary.publish(destination, force)?;
    Ok(report)
}

/// What was written, in terms that do not depend on how long it took.
fn summary(args: &ExportFbxArgs, report: &FbxWriteReport) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    writeln!(
        out,
        "wrote {} from {}",
        args.output.display(),
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
        ExportColourOrigin, ExportGeometry, ExportMaterial, ExportMesh, ExportOmission,
        ExportProvenance, ExportSceneBuilder, ExportTransform,
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
            )
            .expect("a placement");
        builder.finish().expect("a scene")
    }

    fn leftovers(directory: &Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(directory)
            .expect("lists the directory")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.to_string_lossy().contains(".partial"))
            .collect()
    }

    #[test]
    fn a_writer_that_fails_after_it_has_started_publishes_nothing() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");

        // A name FBX ASCII cannot spell. It is refused while the objects are
        // being written, which is well after the header, the settings and the
        // definitions have gone into the scratch file — so this is a failure
        // with a half-written file behind it, not a refusal before any byte.
        let scene = one_node(ExportGeometry::Mesh(mesh([0.5, 0.5, 0.5])), "bad\u{7}name");
        let error = write_and_publish(&destination, &scene, false)
            .expect_err("a name the format cannot spell must stop the write");

        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Unsupported);
        assert!(
            !destination.exists(),
            "a failed write published a destination"
        );
        assert!(
            leftovers(directory.path()).is_empty(),
            "a failed write left scratch space behind: {:?}",
            leftovers(directory.path())
        );
    }

    #[test]
    fn a_colour_the_format_cannot_record_is_refused_before_anything_is_published() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");

        // A linear intensity the export model accepts and FBX has no way of
        // recording. The writer refuses it while working out what the file will
        // say, which is before the scratch file is given a single byte.
        let scene = one_node(ExportGeometry::Mesh(mesh([2.0, 0.0, 0.0])), "unremarkable");
        let error = write_and_publish(&destination, &scene, false)
            .expect_err("a colour outside the measured range must stop the export");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Unsupported);

        assert!(!destination.exists());
        assert!(leftovers(directory.path()).is_empty());
    }

    /// A shear cannot even become a scene, so it can never reach the writer.
    #[test]
    fn a_placement_no_static_hierarchy_can_express_never_becomes_a_scene() {
        let sheared = ExportTransform::new([
            [1.0, 0.3, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]);
        assert!(sheared.is_err(), "a shear was accepted as a placement");
    }

    /// The early check is a courtesy; this is the decision.
    ///
    /// A file that appears between the check at the top of the command and the
    /// end of a long tessellation must not be overwritten, and the export that
    /// lost the race must leave nothing of itself behind.
    #[test]
    fn a_destination_that_appears_while_the_writer_works_is_not_overwritten() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");
        std::fs::write(&destination, b"arrived during the export").expect("writes");

        let scene = one_node(ExportGeometry::Mesh(mesh([0.5, 0.5, 0.5])), "part");
        let error = write_and_publish(&destination, &scene, false)
            .expect_err("publishing without force must not replace anything");

        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
        assert_eq!(
            std::fs::read(&destination).expect("the other file remains"),
            b"arrived during the export"
        );
        assert!(leftovers(directory.path()).is_empty());
    }

    /// And with `--force`, the same race replaces the whole file rather than
    /// the part of it the new one happens to reach.
    #[test]
    fn force_replaces_a_destination_that_appeared_while_the_writer_worked() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");
        let stale = vec![b'x'; 1 << 20];
        std::fs::write(&destination, &stale).expect("writes");

        let scene = one_node(ExportGeometry::Mesh(mesh([0.5, 0.5, 0.5])), "part");
        write_and_publish(&destination, &scene, true).expect("force publishes");

        let published = std::fs::read(&destination).expect("reads the replacement");
        assert!(published.starts_with(b"; FBX 7.4.0 project file"));
        assert!(published.len() < stale.len(), "the old tail survived");
        assert!(leftovers(directory.path()).is_empty());
    }

    #[test]
    fn a_partial_export_is_published_and_reported_and_is_not_a_success() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");

        let scene = one_node(omission(), "frame");
        let report =
            write_and_publish(&destination, &scene, false).expect("an omission is still writable");

        assert!(destination.exists(), "a partial export published nothing");
        assert!(std::fs::metadata(&destination).expect("stats").len() > 0);
        assert!(!report.is_complete());
        assert_eq!(report.omissions().len(), 1);
        assert!(leftovers(directory.path()).is_empty());

        // And it is neither a success nor a failure.
        assert_eq!(exit_code(&report), EXIT_PARTIAL);
        assert_eq!(EXIT_PARTIAL, 6);
        assert_ne!(exit_code(&report), 0);
    }

    #[test]
    fn a_complete_export_is_a_plain_success() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");

        let scene = one_node(ExportGeometry::Mesh(mesh([0.5, 0.5, 0.5])), "part");
        let report = write_and_publish(&destination, &scene, false).expect("writes");

        assert!(report.is_complete());
        assert_eq!(exit_code(&report), 0);
        assert!(destination.exists());
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
