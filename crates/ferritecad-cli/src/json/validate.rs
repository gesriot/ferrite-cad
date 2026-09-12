// SPDX-License-Identifier: MIT
//! Explicit wire projection of the document validator's ordered facts.

use ferritecad_jobs::ValidatedDocument;
use ferritecad_types::{DocumentId, ObjectId};
use serde::Serialize;

#[derive(Serialize)]
pub struct Validated {
    document_id: DocumentId,
    valid: bool,
    errors: u64,
    warnings: u64,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
struct Diagnostic {
    code: &'static str,
    severity: &'static str,
    message: String,
    object_id: Option<ObjectId>,
}

impl From<ValidatedDocument> for Validated {
    fn from(checked: ValidatedDocument) -> Self {
        Self {
            document_id: checked.document_id,
            valid: checked.report.is_ok(),
            errors: checked.report.errors().count() as u64,
            warnings: checked.report.warnings().count() as u64,
            diagnostics: checked
                .report
                .diagnostics
                .into_iter()
                .map(|d| Diagnostic {
                    code: d.code,
                    severity: d.severity.as_str(),
                    message: d.message,
                    object_id: d.object,
                })
                .collect(),
        }
    }
}
