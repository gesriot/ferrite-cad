// SPDX-License-Identifier: MIT
//! Literal grid movement retains the former guide test's publication witnesses.
/// Rebuild cold and export identical bytes for independent reader measurements.
#[test]
fn native_guided_length_drag_worker_and_cli_agree_on_ids_sql_and_geometry() {
    use super::drag_tests::{Published, attach_source_claim, sql_facts, verify_published};
    use crate::creates::tests::{ferritecad, read_semantics};
    use ferritecad_document::Document;
    use ferritecad_kernel::OperationContext;
    use rusqlite::types::Value;
    if !ferritecad_occt::is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("original.fcad");
    let input = root.path().join("L.json");
    std::fs::write(
        &input,
        r#"{"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10}"#,
    )
    .expect("input");
    let p = std::process::Command::new(ferritecad())
        .arg("create-sketch-extrude")
        .arg(&input)
        .arg("-o")
        .arg(&source)
        .arg("--json")
        .output()
        .expect("create");
    assert!(p.status.success(), "{p:?}");
    attach_source_claim(&source);
    let bytes = std::fs::read(&source).expect("source");
    let modified = std::fs::metadata(&source)
        .expect("metadata")
        .modified()
        .expect("mtime");
    let loaded = {
        let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
        ferritecad_scene::snapshot_of(
            &source,
            &mut k,
            |k, b| k.import_step(b),
            &Default::default(),
            &OperationContext::default(),
        )
        .expect("accepted scene")
    };
    let reading =
        crate::sketch_facts::Source::from(loaded.edit_source.expect("accepted edit facts"));
    let literal = [
        [0., 0.],
        [60., 0.],
        [45., 20.],
        [20., 20.],
        [20., 40.],
        [0., 40.],
    ];
    let (mut window, request, expected) =
        crate::sketch_mode::tests::curve_edits::polygon(&source, &literal);
    assert_eq!(request.expected, reading.version);
    assert!(crate::sketch_mode::tests::window::apply(
        &mut window,
        request
    ));
    let ui = root.path().join("ui.fcad");
    assert!(
        window
            .save(ferritecad_jobs::SaveTarget::As(ui.clone()))
            .published
    );

    std::fs::write(
        &input,
        ferritecad_fixtures::edit_request_json(&expected).to_string(),
    )
    .expect("request");
    let cli = root.path().join("cli.fcad");
    let p = std::process::Command::new(ferritecad())
        .arg("edit")
        .arg(&source)
        .arg("--expect-version")
        .arg(reading.version.content.to_string())
        .arg("--request")
        .arg(&input)
        .arg("-o")
        .arg(&cli)
        .arg("--json")
        .output()
        .expect("peer CLI");
    assert!(p.status.success(), "{p:?}");
    assert_eq!(
        read_semantics(&ui),
        read_semantics(&cli),
        "same source: all identities and semantic relationships identical"
    );
    let (a, b) = (
        Document::open_read_only(&ui).expect("UI"),
        Document::open_read_only(&cli).expect("CLI"),
    );
    crate::transaction_tests::assert_meta(a.meta(), b.meta());
    assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
    assert_eq!(
        a.dependencies().expect("deps"),
        b.dependencies().expect("deps")
    );
    assert_eq!(crate::test_names::names(&a), crate::test_names::names(&b));
    drop((a, b));

    // The exact SQL allowlist: against the source, only the sketch object's payload
    // and its hash differ, in one row of one table; no table appears or goes.
    let (before, after) = (sql_facts(&source), sql_facts(&ui));
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    let mut changed = Vec::new();
    for (table, old_rows) in &before {
        let new_rows = &after[table];
        assert_eq!(old_rows.len(), new_rows.len(), "{table}");
        for (row, (old, new)) in old_rows.iter().zip(new_rows).enumerate() {
            for (column, (o, n)) in old.iter().zip(new).enumerate() {
                if o != n {
                    changed.push((table.clone(), row, column, old[2].clone()));
                }
            }
        }
    }
    // objects: rowid, id, kind, parent_id, ordinal, name, payload,
    // payload_hash. The kind cell is at column 2 of the same row.
    let kind = Value::Text("sketch".into());
    assert_eq!(changed.len(), 2, "{changed:?}");
    assert!(
        changed
            .iter()
            .all(|c| c.0 == "objects" && c.1 == changed[0].1 && c.3 == kind)
    );
    assert_eq!(changed.iter().map(|c| c.2).collect::<Vec<_>>(), [6, 7]);

    verify_published(
        &source,
        &ui,
        &cli,
        &Published {
            vertices: [
                [0., 0.],
                [60., 0.],
                [45., 20.],
                [20., 20.],
                [20., 40.],
                [0., 40.],
            ],
            side_x: 60.,
            high: [60., 40., 10.],
            volume_mm3: 14500.,
            artifacts: ("FCAD_SKETCH_GUIDE_ARTIFACT_DIR", "guide"),
        },
    );
    assert_eq!(std::fs::read(&source).expect("source"), bytes);
    assert_eq!(
        std::fs::metadata(&source)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        modified
    );
}
