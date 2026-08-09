// SPDX-License-Identifier: MIT
//! Rebuilding a document and saying what came of it.
//!
//! The command that answers "does this file still build, and into what?"
//! without producing anything. It is the thing to reach for when an export
//! fails or a reference stops resolving, so its report has to be worth reading
//! and worth comparing: two runs over an unchanged document print the same
//! text, which makes a difference between them evidence rather than noise.
//!
//! Nothing here writes. Not the document, not the cache sidecar — a diagnostic
//! that changes what it is diagnosing is worse than none.

use std::process::ExitCode;

use ferritecad_document::{Document, ObjectPayload, ObjectRecord};
use ferritecad_eval::{RebuildResult, rebuild_cold};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{CadError, ObjectId, Result};

use crate::RebuildArgs;

pub fn rebuild(args: RebuildArgs) -> Result<ExitCode> {
    if !args.cold {
        return Err(CadError::input(
            "rebuild needs --cold; a rebuild that consults the cache is not implemented yet, and \
             defaulting to one would make this command's answer depend on a sidecar",
        ));
    }

    let document = Document::open(&args.path)?;
    let mut kernel = OcctKernel::new()?;

    // `rebuild_cold` takes no cache and has no way to reach one, so the
    // sidecar is untouched by construction rather than by promise.
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())?;
    let outcome = report(&document, &built, kernel.identity().to_string());
    built.release_all(&mut kernel);

    print!("{}", outcome?);
    Ok(ExitCode::SUCCESS)
}

/// The whole report as text, built before anything is printed.
///
/// Assembled rather than streamed so a failure halfway through cannot leave
/// half a report on the terminal, and so the result can be compared in a test
/// without capturing a process's output.
fn report(document: &Document, built: &RebuildResult, kernel: String) -> Result<String> {
    use std::fmt::Write as _;

    let objects: Vec<ObjectRecord> = document.objects()?;
    let named = |id: ObjectId| -> String {
        objects
            .iter()
            .find(|object| object.id == id)
            .and_then(|object| object.name.clone())
            .unwrap_or_else(|| "(unnamed)".to_owned())
    };

    let mut out = String::new();
    writeln!(
        out,
        "rebuilt {} ({})",
        document.path().display(),
        document.meta().document_id
    )
    .expect("writing to a String cannot fail");
    writeln!(out, "  kernel {kernel}").expect("writing to a String cannot fail");

    // Deliberately no elapsed time. It would be the only line that changed
    // between two runs of the same document, and it would change every one.
    let shapes = built.shape_count();
    writeln!(
        out,
        "  {} objects evaluated, {shapes} shape{} built",
        built.order().len(),
        if shapes == 1 { "" } else { "s" }
    )
    .expect("writing to a String cannot fail");
    out.push('\n');

    for id in built.order() {
        let Some(object) = objects.iter().find(|object| object.id == *id) else {
            continue;
        };
        let mut line = format!(
            "  {id}  {:<12}  {:<16}",
            named(*id),
            object.payload.type_name()
        );

        match &object.payload {
            ObjectPayload::Sketch(_) => {
                if let Some(profile) = built.profile(*id) {
                    write!(line, "{} segments", profile.outer().segments().len())
                        .expect("writing to a String cannot fail");
                }
            }
            ObjectPayload::Extrude(_) => {
                if let Some(names) = built.topology().feature(*id) {
                    let caps = [
                        ferritecad_document::CapSide::Start,
                        ferritecad_document::CapSide::End,
                    ]
                    .into_iter()
                    .filter_map(|side| names.cap(side))
                    .map(|faces| faces.len())
                    .sum::<usize>();
                    let sides: usize = names
                        .named_segments()
                        .map(|segment| names.side(segment).count())
                        .sum();
                    write!(line, "solid, {} named faces", caps + sides)
                        .expect("writing to a String cannot fail");
                }
            }
            ObjectPayload::Body(body) => {
                if let Some(tip) = body.tip_feature {
                    write!(line, "tip {}", named(tip)).expect("writing to a String cannot fail");
                }
            }
            _ => {}
        }
        writeln!(out, "{}", line.trim_end()).expect("writing to a String cannot fail");
    }

    // What the document says about its own geometry, checked against what was
    // just built. A reference that no longer resolves is the failure this
    // whole project is arranged to make visible, so it is named, not counted.
    let references = document.topology_refs()?;
    let mut lost = Vec::new();
    for reference in &references {
        if let Err(error) = built.resolve(reference) {
            lost.push(format!(
                "    {} on {}: {error}",
                reference.id,
                named(reference.producer_feature)
            ));
        }
    }

    out.push('\n');
    writeln!(
        out,
        "  {} of {} stored references resolved",
        references.len() - lost.len(),
        references.len()
    )
    .expect("writing to a String cannot fail");
    for line in &lost {
        writeln!(out, "{line}").expect("writing to a String cannot fail");
    }
    Ok(out)
}
