// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::ExpressionSlot;

#[test]
fn native_height_transaction_preserves_tools_references_and_cold_geometry() {
    if !native() {
        return;
    }
    for n in [1, 4, 16] {
        let root = tempfile::tempdir().expect("root");
        let tools = history::tools(n);
        let source = history::fixture(root.path(), &tools);
        let original = std::fs::read(&source).expect("source");
        let d = Document::open_read_only(&source).expect("document");
        let base = d.objects().expect("objects").into_iter().find(|o|matches!(&o.payload,
            ferritecad_document::ObjectPayload::Extrude(e) if e.operation == ferritecad_document::SolidOperation::NewBody && e.previous.is_none())).expect("base").id;
        let refs = d.topology_refs().expect("refs");
        let request = root.path().join("transaction.json");
        std::fs::write(&request,serde_json::to_vec(&json!({"request_version":1,"changes":[{"object":base,"slot":ExpressionSlot::ExtrudeDistance,"value":14.0,"unit":"mm"}]})).expect("JSON")).expect("request");
        let output = root.path().join("height.fcad");
        reply(
            cli()
                .arg("edit")
                .arg(&source)
                .arg("--expect-version")
                .arg(d.content_version().expect("version").to_string())
                .arg("--request")
                .arg(&request)
                .arg("--output")
                .arg(&output)
                .arg("--json")
                .output()
                .expect("edit"),
            "edit",
            0,
        );
        let edited = Document::open_read_only(&output).expect("copy");
        let after_refs = edited.topology_refs().expect("refs");
        assert!(
            refs.iter().all(|r| after_refs.contains(r)),
            "existing references survive"
        );
        assert_eq!(
            edited.dependencies().expect("deps"),
            d.dependencies().expect("deps")
        );
        for old in d.objects().expect("objects") {
            let now = edited
                .object(old.id)
                .expect("object")
                .expect("same identity");
            if old.id != base {
                assert_eq!(old, now);
            }
        }
        // New pocket floors must remain named so structural edits can follow.
        let catalog = ferritecad_document::ExtrudeEditSource::read(&edited).expect("reading");
        for choice in &catalog.cut_features {
            if choice.feature != base {
                assert!(
                    choice.saved.is_some(),
                    "Cut history must remain editable: {:?}",
                    choice.refusal
                );
            }
        }
        super::measure_history_at(&output, &tools, [[0., 0.], [SIZE[0], SIZE[1]]], 14.0, None);
        assert_eq!(std::fs::read(&source).expect("unchanged"), original);
    }
}
