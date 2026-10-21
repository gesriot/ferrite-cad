// SPDX-License-Identifier: MIT
//! Internal document consistency, read once without migration or a kernel.

use std::path::Path;

use ferritecad_document::{Document, ValidationReport};
use ferritecad_types::{DocumentId, Result};

/// One completed check, including findings that make the document invalid.
/// No connection or geometry survives the operation. Read/decode failures are
/// `Err`, not a report claiming the document has no findings.
#[derive(Debug)]
pub struct ValidatedDocument {
    pub document_id: DocumentId,
    pub report: ValidationReport,
}

/// Validate one pinned, read-only snapshot and close SQLite before returning.
///
/// The path is the entire request. Existing schema, WAL and reader guards apply;
/// this operation cannot migrate, repair, rebuild or create a missing document.
/// A report is about stored consistency, not successful geometry or STEP import.
pub fn validate_document(source: &Path) -> Result<ValidatedDocument> {
    validate_snapshot(Document::open_read_only(source)?)
}

fn validate_snapshot(document: Document) -> Result<ValidatedDocument> {
    let document_id = document.meta().document_id;
    let report = document.validate();
    // Close on both report and decoding failure. Prefer the original failure
    // if both reading and closing fail; it explains why no report was possible.
    let closed = document.close();
    let report = report?;
    closed?;
    Ok(ValidatedDocument {
        document_id,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_document::{ObjectPayload, Sketch};
    use ferritecad_types::ObjectId;

    #[test]
    fn validation_keeps_one_snapshot_and_closes_before_return() {
        let root = tempfile::tempdir().expect("directory");
        let path = root.path().join("source.fcad");
        let mut writer = Document::create(&path).expect("create");
        let id = writer.meta().document_id;
        let snapshot = Document::open_read_only(&path).expect("pinned metadata");
        let object = ObjectId::new();
        let payload = ObjectPayload::Sketch(Sketch {
            plane: ObjectId::new(),
            curves: vec![],
            constraints: vec![],
        });
        writer
            .write(|w| {
                w.put_object(object, None, 0, None, &payload)?;
                let checked = validate_snapshot(snapshot)?;
                assert_eq!(checked.document_id, id);
                assert!(checked.report.diagnostics.is_empty());
                Ok(())
            })
            .expect("commit after the job releases its SQLite read lock");
        writer.close().expect("close writer");
        let later = validate_document(&path).expect("new reading");
        assert_eq!(later.document_id, id);
        assert!(
            later
                .report
                .errors()
                .any(|finding| finding.object == Some(object))
        );
    }
}
