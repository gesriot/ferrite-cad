// SPDX-License-Identifier: MIT
use ferritecad_eval::{DegradedBody, FailedFeature, RebuildReport, SkippedFeature};
#[test]
fn tree_occurrences_show_errors_root_dependencies_and_body_fallback() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("broken.fcad");
    let chain = ferritecad_fixtures::write_fillet_chain(&path, 25.0).expect("fixture");
    let document = ferritecad_document::Document::open_read_only(&path).expect("document");
    let objects = document.objects().expect("objects");
    let mut tree =
        ferritecad_document::ModelTree::from_dependencies(&objects, document.dependencies());
    let report = RebuildReport {
        failed: vec![FailedFeature {
            id: chain.first,
            kind: "kernel".into(),
            message: "radius does not fit".into(),
        }],
        skipped: vec![
            SkippedFeature {
                id: chain.second,
                cause: chain.first,
            },
            SkippedFeature {
                id: chain.body,
                cause: chain.first,
            },
        ],
        degraded_bodies: vec![DegradedBody {
            id: chain.body,
            rolled_back_to: Some(chain.extrude),
            cause: chain.first,
        }],
        warnings: vec![],
    };
    ferritecad_scene::project_rebuild_status(&mut tree, &objects, &report);
    assert_eq!(tree.unbuilt_features, 2);
    for row in &tree.rows {
        if row.id.0 == chain.first {
            assert!(row.note.contains("radius does not fit"));
            assert!(row.name.starts_with('⚠'));
        }
        if row.id.0 == chain.second {
            assert_eq!(row.note, "not built: depends on Fillet1");
        }
        if row.id.0 == chain.body {
            assert!(
                row.note
                    .contains("Rolled back to Extrude1 because of Fillet1")
            );
        }
    }
}
