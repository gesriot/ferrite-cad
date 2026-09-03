// SPDX-License-Identifier: MIT
//! Bringing a STEP file into a document, and saying only what happened.
//!
//! The first command that reads somebody else's file. Two things follow from
//! that and shape everything here.
//!
//! The input is never written to. It is opened once, read, and not touched
//! again — not rewritten, not normalised, not moved. The bytes go into the
//! document whole, so the document stops depending on that file existing at
//! all, and the file itself is left exactly as it was found.
//!
//! And the report never says the file is correct. Measured on Open CASCADE
//! 8.0.1, one of the committed damaged files is read, transferred, and reported
//! clean while carrying a malformed coordinate. "Nothing was reported" is a
//! fact about the reader; a command that printed it as a verdict on the file
//! would be stating something nothing here established.

use std::path::Path;
use std::process::ExitCode;

use ferritecad_document::{Document, StepImportRequest};
use ferritecad_exchange::{Diagnostic, Import, Severity};
use ferritecad_kernel::GeometryKernel;
use ferritecad_occt::OcctKernel;
use ferritecad_types::{CadError, ObjectId, Result};

use crate::{EXIT_NOTICED, EXIT_REJECTED, ImportStepArgs, REPLACE_ADVICE, replacing};
use ferritecad_jobs::{Temporary, path_entry_exists, refuse_source_as_destination};

pub fn import_step(args: ImportStepArgs) -> Result<ExitCode> {
    // Takes precedence over the ordinary no-clobber message: the file being
    // read is not a destination `--force` can ever make acceptable.
    refuse_source_as_destination(
        &args.path,
        &args.output,
        "the STEP file cannot also be the document written from it",
    )?;

    // Checked before the kernel is opened. Reading a large assembly only to
    // refuse at the last step wastes the user's time and tells them nothing
    // they could not have been told at once.
    if !args.force && path_entry_exists(&args.output)? {
        return Err(CadError::input(format!(
            "{} already exists; {REPLACE_ADVICE}",
            args.output.display()
        )));
    }

    // The only time this command touches the input, and it only reads.
    let source = std::fs::read(&args.path)
        .map_err(|e| CadError::io(format!("reading {}", args.path.display()), e))?;

    let mut kernel = OcctKernel::new()?;
    let outcome = kernel.import_step(&source)?;

    let stored = match &outcome {
        // Nothing was built, so there is nothing to write and nothing to
        // release. The diagnostics are the whole result, and are often the most
        // useful thing an import can offer.
        Import::Rejected { diagnostics } => {
            print!(
                "{}",
                refused(&args.path, source.len(), diagnostics, &kernel)
            );
            return Ok(ExitCode::from(EXIT_REJECTED));
        }
        Import::Imported { .. } => write_document(&args, &source, &outcome, &kernel),
    };

    // Whatever happened to the document, the session gets its shapes back.
    if let Some(scene) = outcome.scene() {
        for shape in scene.shapes() {
            kernel.release(shape);
        }
    }
    let stored = stored?;

    print!("{stored}");
    Ok(if outcome.diagnostics().is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_NOTICED)
    })
}

/// Builds the document under a scratch name and publishes it in one step.
///
/// Everything that can fail — reading the file, transferring it, projecting the
/// scene, hashing the source — has already happened or happens before the
/// document reaches its destination. A failure anywhere here leaves no file at
/// the destination and no scratch file beside it.
fn write_document(
    args: &ImportStepArgs,
    source: &[u8],
    outcome: &Import,
    kernel: &OcctKernel,
) -> Result<String> {
    let object = ObjectId::new();
    let name = args
        .name
        .clone()
        .or_else(|| file_stem(&args.path))
        .unwrap_or_else(|| "Imported".to_owned());

    let temporary = Temporary::beside(&args.output)?;
    let mut document = Document::create(temporary.path())?;
    let stored = document.store_step_import(StepImportRequest {
        object,
        name: Some(&name),
        source,
        // The file's own name, never the path it was read from. A path would
        // put the layout of one machine's disk into a document meant to travel,
        // and nothing ever opens it: the bytes are here.
        source_name: file_name(&args.path).as_deref(),
        import: outcome,
        importer: kernel.identity(),
    })?;
    document.close()?;
    temporary.publish(&args.output, replacing(args.force))?;

    Ok(imported(
        args, source, outcome, kernel, &name, object, &stored,
    ))
}

/// The report for a file that produced a document.
fn imported(
    args: &ImportStepArgs,
    source: &[u8],
    outcome: &Import,
    kernel: &OcctKernel,
    name: &str,
    object: ObjectId,
    stored: &ferritecad_document::ImportedStep,
) -> String {
    use std::fmt::Write as _;

    let scene = &stored.scene;
    let mut out = String::new();
    writeln!(
        out,
        "imported {} -> {}",
        args.path.display(),
        args.output.display()
    )
    .expect("writing to a String cannot fail");
    writeln!(out, "  kernel        {}", kernel.identity()).expect("cannot fail");
    writeln!(
        out,
        "  source        {} byte(s) stored whole, blake3 {}",
        source.len(),
        stored.source_hash
    )
    .expect("cannot fail");
    writeln!(
        out,
        "  declared      {} in {}",
        blank_as(scene.schema(), "no schema"),
        blank_as(scene.source_unit(), "no unit")
    )
    .expect("cannot fail");
    writeln!(
        out,
        "  object        {object}  {name}  ({} definition{}, {} placement{})",
        scene.definition_count(),
        plural(scene.definition_count()),
        scene.instance_count(),
        plural(scene.instance_count())
    )
    .expect("cannot fail");

    // Listed from the scene that was just read rather than from the stored
    // projection: the two are the same by construction here, and the live one
    // does not have to be matched against a layout version to be read.
    out.push('\n');
    if let Some(live) = outcome.scene() {
        for (index, definition) in live.definitions.iter().enumerate() {
            let placements = live
                .instances
                .iter()
                .filter(|instance| instance.definition == index)
                .count();
            writeln!(
                out,
                "  {:<30}  {} solid{}, {placements} placement{}",
                blank_as(&definition.name, "(unnamed)"),
                definition.solids,
                plural(definition.solids as usize),
                plural(placements)
            )
            .expect("cannot fail");
            // What a later reference to this part will have to name it by.
            writeln!(out, "      {}", definition.key).expect("cannot fail");
        }
    }

    out.push('\n');
    out.push_str(&notes(outcome.diagnostics()));
    out
}

/// The report for a file the importer would not read.
fn refused(
    path: &Path,
    byte_len: usize,
    diagnostics: &[Diagnostic],
    kernel: &OcctKernel,
) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    writeln!(out, "refused {} ({byte_len} byte(s))", path.display())
        .expect("writing to a String cannot fail");
    writeln!(out, "  kernel        {}", kernel.identity()).expect("cannot fail");
    writeln!(out, "  nothing was written").expect("cannot fail");
    out.push('\n');
    out.push_str(&notes(diagnostics));
    out
}

/// What the reading reported, and what its silence does and does not mean.
fn notes(diagnostics: &[Diagnostic]) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    if diagnostics.is_empty() {
        writeln!(out, "  nothing was reported while reading it").expect("cannot fail");
        writeln!(
            out,
            "  that describes this reader, not the file: a malformed value the reader does not \
             recognise is read silently"
        )
        .expect("cannot fail");
        return out;
    }

    let failures = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Fail)
        .count();
    let warnings = diagnostics.len() - failures;
    writeln!(
        out,
        "  reading reported {failures} problem{} and {warnings} warning{}",
        plural(failures),
        plural(warnings)
    )
    .expect("cannot fail");
    for diagnostic in diagnostics {
        writeln!(out, "    {diagnostic}").expect("cannot fail");
    }
    out
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn blank_as<'a>(value: &'a str, empty: &'a str) -> &'a str {
    if value.is_empty() { empty } else { value }
}

fn file_name(path: &Path) -> Option<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

fn file_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}
