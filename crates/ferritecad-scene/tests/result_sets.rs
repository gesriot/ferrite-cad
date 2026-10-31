// SPDX-License-Identifier: MIT
use ferritecad_document::{Document, EditChange, KeepSelection};
use ferritecad_kernel::{OperationContext, TessellationParams};

#[test]
fn one_body_mesh_contains_both_pieces_and_named_selection_removes_only_one() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut doc = Document::create(root.path().join("set.fcad")).expect("document");
    let plate = ferritecad_fixtures::result_sets::plate(&mut doc).expect("plate");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let slot = ferritecad_fixtures::result_sets::slot(&doc, &plate, &mut kernel).expect("slot");
    doc.write_changes(&slot.changes).expect("set");
    for selected in [false, true] {
        if selected {
            doc.write_changes(&[EditChange::SetKeepSelection {
                object: slot.keep,
                selection: KeepSelection::ContainingFaces(vec![slot.side]),
            }])
            .expect("select");
        }
        let built = ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("rebuild");
        let loaded = ferritecad_scene::snapshot_from_rebuild(
            &doc,
            &built,
            &mut kernel,
            |_, _| unreachable!("no imports"),
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("scene without GPU");
        assert_eq!(loaded.snapshot.meshes().len(), 1, "one mesh for the Body");
        let mesh = &loaded.snapshot.meshes()[0];
        let mut left = 0;
        let mut right = 0;
        for triangle in mesh.indices().chunks_exact(3) {
            let xs: Vec<_> = triangle
                .iter()
                .map(|i| mesh.vertices()[*i as usize * 6])
                .collect();
            if xs.iter().all(|x| *x <= 28.) {
                left += 1;
            } else {
                assert!(
                    xs.iter().all(|x| *x >= 32.),
                    "triangle crosses the empty slot: {xs:?}"
                );
                right += 1;
            }
        }
        assert!(left > 0);
        assert_eq!(right > 0, !selected);
        built.release_all(&mut kernel);
    }
}
