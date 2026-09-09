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

use ferritecad_exchange::{Diagnostic, Severity};
use ferritecad_jobs::{
    ImportStepRequest, PublishedStepImport, StepImportOutcome, StepReadFacts, import_step_document,
};
use ferritecad_kernel::OperationContext;
use ferritecad_occt::OcctKernel;
use ferritecad_types::Result;

use crate::{EXIT_NOTICED, EXIT_REJECTED, ImportStepArgs, replacing};

pub fn import_step_result(args: &ImportStepArgs) -> Result<StepImportOutcome> {
    if args.json {
        crate::json::require_utf8_path(&args.path)?;
        crate::json::require_utf8_path(&args.output)?;
    }
    import_step_document(
        &ImportStepRequest {
            source: &args.path,
            destination: &args.output,
            name: args.name.as_deref(),
            existing: replacing(args.force),
        },
        OcctKernel::new,
        OcctKernel::import_step,
        &OperationContext::default(),
    )
}

pub fn import_step(args: ImportStepArgs) -> Result<ExitCode> {
    match import_step_result(&args)? {
        StepImportOutcome::Rejected(read) => {
            print!("{}", refused(&args.path, &read));
            Ok(ExitCode::from(EXIT_REJECTED))
        }
        StepImportOutcome::Published(published) => {
            print!("{}", imported(&args.path, &published));
            Ok(if published.read.diagnostics.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(EXIT_NOTICED)
            })
        }
    }
}

/// The report for a file that produced a document.
fn imported(source: &Path, published: &PublishedStepImport) -> String {
    use std::fmt::Write as _;

    let scene = &published.scene;
    let read = &published.read;
    let object = published.object_id;
    let name = &published.name;
    let mut out = String::new();
    writeln!(
        out,
        "imported {} -> {}",
        source.display(),
        published.destination.display()
    )
    .expect("writing to a String cannot fail");
    writeln!(out, "  kernel        {}", read.imported_by).expect("cannot fail");
    writeln!(
        out,
        "  source        {} byte(s) stored whole, blake3 {}",
        read.source_byte_len, read.source_hash
    )
    .expect("cannot fail");
    writeln!(
        out,
        "  declared      {} in {}",
        blank_as(&scene.schema, "no schema"),
        blank_as(&scene.source_unit, "no unit")
    )
    .expect("cannot fail");
    writeln!(
        out,
        "  object        {object}  {name}  ({} definition{}, {} placement{})",
        scene.definitions.len(),
        plural(scene.definitions.len()),
        scene.instances.len(),
        plural(scene.instances.len())
    )
    .expect("cannot fail");

    out.push('\n');
    for definition in &scene.definitions {
        let placements = scene
            .instances
            .iter()
            .filter(|instance| instance.definition == definition.key)
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
        writeln!(out, "      {}", definition.key).expect("cannot fail");
    }

    out.push('\n');
    out.push_str(&notes(&read.diagnostics));
    out
}

/// The report for a file the importer would not read.
fn refused(path: &Path, read: &StepReadFacts) -> String {
    let byte_len = read.source_byte_len;
    use std::fmt::Write as _;

    let mut out = String::new();
    writeln!(out, "refused {} ({byte_len} byte(s))", path.display())
        .expect("writing to a String cannot fail");
    writeln!(out, "  kernel        {}", read.imported_by).expect("cannot fail");
    writeln!(out, "  nothing was written").expect("cannot fail");
    out.push('\n');
    out.push_str(&notes(&read.diagnostics));
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
