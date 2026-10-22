// SPDX-License-Identifier: MIT
use ferritecad_types::{CadError, ObjectId, StableEntityId};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Outcome of evaluating one plan object.
#[derive(Debug)]
pub enum RebuildStatus {
    Built,
    Failed { error: CadError },
    Skipped { cause: ObjectId },
}

impl RebuildStatus {
    pub fn root_failure(&self, id: ObjectId) -> Option<ObjectId> {
        match self {
            Self::Built => None,
            Self::Failed { .. } => Some(id),
            Self::Skipped { cause } => Some(*cause),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedFeature {
    pub id: ObjectId,
    pub kind: String,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedFeature {
    pub id: ObjectId,
    pub cause: ObjectId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DegradedBody {
    pub id: ObjectId,
    pub rolled_back_to: Option<ObjectId>,
    pub cause: ObjectId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReferenceWarning {
    pub owner: ObjectId,
    pub reference: StableEntityId,
    pub message: String,
}
/// Portable rebuild facts, without session-owned handles.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct RebuildReport {
    pub failed: Vec<FailedFeature>,
    pub skipped: Vec<SkippedFeature>,
    pub degraded_bodies: Vec<DegradedBody>,
    pub warnings: Vec<ReferenceWarning>,
}
impl RebuildReport {
    pub fn all_built(&self) -> bool {
        self.failed.is_empty() && self.skipped.is_empty()
    }
    pub fn unbuilt(&self) -> BTreeSet<ObjectId> {
        self.failed
            .iter()
            .map(|f| f.id)
            .chain(self.skipped.iter().map(|s| s.id))
            .collect()
    }
}

/// Carries the complete refusal and retains the original evaluation errors.
#[derive(Debug)]
pub struct RebuildRefusal {
    pub report: RebuildReport,
    pub errors: BTreeMap<ObjectId, CadError>,
    pub lost_references: BTreeMap<StableEntityId, CadError>,
}
impl std::fmt::Display for RebuildRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for failure in &self.report.failed {
            writeln!(f, "{}: {}", failure.id, failure.message)?;
        }
        for skipped in &self.report.skipped {
            let reason = self
                .report
                .failed
                .iter()
                .find(|failure| failure.id == skipped.cause);
            writeln!(
                f,
                "{}: not built: depends on {}{}",
                skipped.id,
                skipped.cause,
                reason
                    .map(|r| format!(": {}", r.message))
                    .unwrap_or_default()
            )?;
        }
        for (id, error) in &self.lost_references {
            writeln!(f, "lost reference {id}: {error}")?;
        }
        Ok(())
    }
}
impl std::error::Error for RebuildRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.errors
            .values()
            .next()
            .or_else(|| self.lost_references.values().next())
            .map(|e| e as &dyn std::error::Error)
    }
}
impl RebuildRefusal {
    pub fn from_error(error: &CadError) -> Option<&Self> {
        let mut source = std::error::Error::source(error);
        while let Some(cause) = source {
            if let Some(report) = cause.downcast_ref::<Self>() {
                return Some(report);
            }
            source = cause.source();
        }
        None
    }
}
