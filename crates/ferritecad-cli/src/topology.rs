// SPDX-License-Identifier: MIT
//! What a document says about its own geometry, and whether it still holds.
//!
//! Every reference is printed in the terms the document itself stores: who
//! owns it, which feature made the geometry, what that geometry is, and how
//! many entities the rule selects. Nothing session-local appears — no handles,
//! no archive slots, no face indices. Those change on every run, and a report
//! that named them would be inviting somebody to depend on them.
//!
//! A reference that no longer resolves is why this command exists, so all
//! non-resolutions are listed rather than the first. Their error classes stay
//! distinct: lost geometry is not a contradictory reference, and neither is a
//! role this build cannot implement. Stopping at one would hide whether a
//! single edit broke one name or every name, which is the difference between a
//! small mistake and a lost model.

use std::process::ExitCode;

use ferritecad_document::{
    CapSide, Document, ObjectRecord, SelectionRule, SemanticRole, TopologyRef,
};
use ferritecad_eval::{RebuildResult, rebuild_cold};
use ferritecad_kernel::OperationContext;
use ferritecad_occt::OcctKernel;
use ferritecad_types::{CadError, ErrorKind, ObjectId, Result};

use crate::{DocumentArgs, EXIT_FAILED, EXIT_INVALID, EXIT_UNRESOLVED};

pub fn print_topology(args: DocumentArgs) -> Result<ExitCode> {
    // Read-only: a command that reports on a document must not be capable of
    // changing the thing it is reporting on.
    let document = Document::open_read_only(&args.path)?;
    let mut kernel = OcctKernel::new()?;

    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())?;
    let outcome = report(&document, &built);
    built.release_all(&mut kernel);

    let (text, status) = outcome?;
    print!("{text}");
    Ok(status.exit_code())
}

/// The report and the most serious outcome it contains.
fn report(document: &Document, built: &RebuildResult) -> Result<(String, ReportStatus)> {
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

    let mut issues = Vec::new();
    for reference in &references {
        out.push('\n');
        let resolved = built.resolve(reference);
        let (status, count) = match &resolved {
            Ok(found) => (ReferenceStatus::Resolved, found.len().to_string()),
            Err(error) => (ReferenceStatus::from_error(error), "-".to_owned()),
        };

        writeln!(out, "  {}  {}", reference.id, status.as_str())
            .expect("writing to a String cannot fail");
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
            issues.push(ReferenceIssue {
                reference,
                status,
                error,
            });
        }
    }

    out.push('\n');
    writeln!(
        out,
        "  {} of {} references resolved",
        references.len() - issues.len(),
        references.len()
    )
    .expect("writing to a String cannot fail");

    // All of them, with the reason each gave. A caller fixing a model needs to
    // see the whole extent of the damage in one pass.
    for issue in &issues {
        writeln!(
            out,
            "    {} {} {}: {error}",
            issue.reference.id,
            issue.status.as_str(),
            describe_role(&issue.reference.output_role),
            error = issue.error,
        )
        .expect("writing to a String cannot fail");
    }
    Ok((
        out,
        ReportStatus::from_statuses(issues.iter().map(|issue| issue.status)),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReferenceStatus {
    Resolved,
    Lost,
    Invalid,
    Unsupported,
    Failed,
}

struct ReferenceIssue<'a> {
    reference: &'a TopologyRef,
    status: ReferenceStatus,
    error: CadError,
}

impl ReferenceStatus {
    fn from_error(error: &CadError) -> Self {
        match error.kind() {
            ErrorKind::Topology => Self::Lost,
            ErrorKind::Input => Self::Invalid,
            ErrorKind::Unsupported => Self::Unsupported,
            // Resolution is currently pure and returns only the three kinds
            // above. Keep future kernel, IO, cancellation or constraint errors
            // visible without pretending that geometry merely disappeared.
            _ => Self::Failed,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::Lost => "lost",
            Self::Invalid => "invalid",
            Self::Unsupported => "unsupported",
            Self::Failed => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportStatus {
    Resolved,
    Lost,
    Invalid,
    Failed,
}

impl ReportStatus {
    fn from_statuses(statuses: impl IntoIterator<Item = ReferenceStatus>) -> Self {
        let mut lost = false;
        let mut invalid = false;
        let mut failed = false;
        for status in statuses {
            match status {
                ReferenceStatus::Resolved => {}
                ReferenceStatus::Lost => lost = true,
                ReferenceStatus::Invalid => invalid = true,
                ReferenceStatus::Unsupported | ReferenceStatus::Failed => failed = true,
            }
        }

        if failed {
            Self::Failed
        } else if invalid {
            Self::Invalid
        } else if lost {
            Self::Lost
        } else {
            Self::Resolved
        }
    }

    fn exit_code(self) -> ExitCode {
        match self {
            Self::Resolved => ExitCode::SUCCESS,
            Self::Invalid => ExitCode::from(EXIT_INVALID),
            Self::Failed => ExitCode::from(EXIT_FAILED),
            Self::Lost => ExitCode::from(EXIT_UNRESOLVED),
        }
    }
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
        _ => "unknown semantic role".to_owned(),
    }
}

fn describe_selection(selection: &SelectionRule) -> String {
    match selection {
        SelectionRule::Exact => "exactly one".to_owned(),
        SelectionRule::AllDerivedFrom { ancestor } => {
            format!("all derived from {ancestor}")
        }
        _ => "unknown selection rule".to_owned(),
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

#[cfg(test)]
mod tests {
    use super::{ReferenceStatus, ReportStatus};

    #[test]
    fn the_exit_status_keeps_error_classes_distinct_and_prioritised() {
        assert_eq!(ReportStatus::from_statuses([]), ReportStatus::Resolved);
        assert_eq!(
            ReportStatus::from_statuses([ReferenceStatus::Lost]),
            ReportStatus::Lost
        );
        assert_eq!(
            ReportStatus::from_statuses([ReferenceStatus::Lost, ReferenceStatus::Invalid]),
            ReportStatus::Invalid
        );
        assert_eq!(
            ReportStatus::from_statuses([
                ReferenceStatus::Lost,
                ReferenceStatus::Invalid,
                ReferenceStatus::Unsupported,
            ]),
            ReportStatus::Failed
        );
        assert_eq!(
            ReportStatus::from_statuses([ReferenceStatus::Failed]),
            ReportStatus::Failed
        );
    }
}
