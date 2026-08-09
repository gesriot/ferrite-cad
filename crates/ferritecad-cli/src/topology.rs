// SPDX-License-Identifier: MIT
//! What a document says about its own geometry, and whether it still holds.
//!
//! Every reference is printed in the terms the document itself stores: who
//! owns it, which feature made the geometry, what that geometry is, and how
//! many entities the rule selects. Nothing session-local appears — no handles,
//! no archive slots, no face indices. Those change on every run, and a report
//! that named them would be inviting somebody to depend on them.
//!
//! A reference that no longer resolves is why this command exists, so all of
//! them are listed rather than the first. Stopping at one would hide whether
//! a single edit broke one name or every name, which is the difference between
//! a small mistake and a lost model.

use std::process::ExitCode;

use ferritecad_document::{CapSide, Document, ObjectRecord, SelectionRule, SemanticRole};
use ferritecad_eval::{RebuildResult, rebuild_cold};
use ferritecad_kernel::OperationContext;
use ferritecad_occt::OcctKernel;
use ferritecad_types::{ObjectId, Result};

use crate::{DocumentArgs, EXIT_UNRESOLVED};

pub fn print_topology(args: DocumentArgs) -> Result<ExitCode> {
    // Read-only: a command that reports on a document must not be capable of
    // changing the thing it is reporting on.
    let document = Document::open_read_only(&args.path)?;
    let mut kernel = OcctKernel::new()?;

    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())?;
    let outcome = report(&document, &built);
    built.release_all(&mut kernel);

    let (text, lost) = outcome?;
    print!("{text}");
    Ok(if lost == 0 {
        ExitCode::SUCCESS
    } else {
        // Distinct from both success and the code a command that could not run
        // returns: this document opened, rebuilt, and is missing names. A
        // script wants to tell those three apart.
        ExitCode::from(EXIT_UNRESOLVED)
    })
}

/// The report, and how many references failed to resolve.
fn report(document: &Document, built: &RebuildResult) -> Result<(String, usize)> {
    use std::fmt::Write as _;

    let objects = document.objects()?;
    // Already ordered by owner and then by identifier, so two runs over one
    // document print the same lines in the same order.
    let references = document.topology_refs()?;

    let mut out = String::new();
    writeln!(
        out,
        "topology of {} ({})",
        document.path().display(),
        document.meta().document_id
    )
    .expect("writing to a String cannot fail");
    writeln!(
        out,
        "  {} reference{}",
        references.len(),
        if references.len() == 1 { "" } else { "s" }
    )
    .expect("writing to a String cannot fail");

    let mut lost = Vec::new();
    for reference in &references {
        out.push('\n');
        let resolved = built.resolve(reference);
        let (status, count) = match &resolved {
            Ok(found) => ("resolved", found.len().to_string()),
            Err(_) => ("lost", "-".to_owned()),
        };

        writeln!(out, "  {}  {status}", reference.id).expect("writing to a String cannot fail");
        writeln!(
            out,
            "    role       {}",
            describe_role(&reference.output_role)
        )
        .expect("writing to a String cannot fail");
        writeln!(
            out,
            "    selection  {}",
            describe_selection(&reference.selection)
        )
        .expect("writing to a String cannot fail");
        writeln!(out, "    expects    {}", reference.expected_kind.as_str())
            .expect("writing to a String cannot fail");
        writeln!(out, "    owner      {}", label(&objects, reference.owner))
            .expect("writing to a String cannot fail");
        writeln!(
            out,
            "    producer   {}",
            label(&objects, reference.producer_feature)
        )
        .expect("writing to a String cannot fail");
        writeln!(out, "    selects    {count}").expect("writing to a String cannot fail");

        if let Err(error) = resolved {
            lost.push((reference, error));
        }
    }

    out.push('\n');
    writeln!(
        out,
        "  {} of {} references resolved",
        references.len() - lost.len(),
        references.len()
    )
    .expect("writing to a String cannot fail");

    // All of them, with the reason each gave. A caller fixing a model needs to
    // see the whole extent of the damage in one pass.
    for (reference, error) in &lost {
        writeln!(
            out,
            "    {} {}: {error}",
            reference.id,
            describe_role(&reference.output_role)
        )
        .expect("writing to a String cannot fail");
    }
    Ok((out, lost.len()))
}

/// A durable description of what geometry a reference means.
///
/// Written out here rather than derived from `Debug`: this is the text a
/// person reads and a script may match on, and it must not change because a
/// variant was renamed.
fn describe_role(role: &SemanticRole) -> String {
    match role {
        SemanticRole::ExtrudeCap { side } => match side {
            CapSide::Start => "extrude cap start".to_owned(),
            CapSide::End => "extrude cap end".to_owned(),
            // A cap this build does not know is said to be unknown rather than
            // folded into one of the two it does.
            _ => "extrude cap (unknown side)".to_owned(),
        },
        SemanticRole::ExtrudeSide { profile_segment } => {
            format!("extrude side from segment {profile_segment}")
        }
        SemanticRole::SketchSegment { segment } => format!("sketch segment {segment}"),
        SemanticRole::FilletFace { source_edge } => format!("fillet face from edge {source_edge}"),
        other => format!("unknown role {other:?}"),
    }
}

fn describe_selection(selection: &SelectionRule) -> String {
    match selection {
        SelectionRule::Exact => "exactly one".to_owned(),
        SelectionRule::AllDerivedFrom { ancestor } => {
            format!("all derived from {ancestor}")
        }
        other => format!("unknown rule {other:?}"),
    }
}

fn label(objects: &[ObjectRecord], id: ObjectId) -> String {
    match objects
        .iter()
        .find(|object| object.id == id)
        .and_then(|object| object.name.as_deref())
    {
        Some(name) => format!("{name} ({id})"),
        None => id.to_string(),
    }
}
