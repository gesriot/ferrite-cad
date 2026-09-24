// SPDX-License-Identifier: MIT
//! §26E: one bounded chain, including distant dependencies and disk conflicts.
use super::edits::{edit, only_numbers};
use super::*;
use ferritecad_document::{CircularCutEdit, ExtrudeEditSource, SavedCircularCut};

pub(super) fn tools(n: usize) -> Vec<CircularCut> {
    (0..n)
        .map(|i| {
            // Permutation deliberately separates geometric order from history order.
            let slot = (i * 7) % 16;
            CircularCut {
                center_mm: [10. + (slot % 4) as f64 * 19., 7. + (slot / 4) as f64 * 12.],
                radius_mm: 1.5 + (i % 5) as f64 * 0.25,
                extent: CutExtent::Blind {
                    depth_mm: if i % 2 == 0 { 12. } else { 3. + (i % 7) as f64 },
                },
            }
        })
        .collect()
}

pub(super) fn fixture(root: &Path, tools: &[CircularCut]) -> PathBuf {
    let path = root.join("history.fcad");
    write_plate_document(&path, SIZE[0], SIZE[1], SIZE[2]);
    let mut d = Document::open(&path).expect("doc");
    let (body, _) = tip(&d);
    for tool in tools {
        let p = ferritecad_document::prepare_circular_cut(&d, body, tool).expect("prepare link");
        d.write_circular_cut(&p).expect("write link");
    }
    d.close().expect("close");
    rusqlite::Connection::open(&path)
        .expect("SQL")
        .execute_batch(
            "UPDATE objects SET rowid=-rowid,ordinal=100-ordinal,name='same';
         UPDATE capabilities SET rowid=rowid+100;
         INSERT INTO capabilities(rowid,name,required) VALUES(900,'optional.extension',0);
         CREATE TABLE extra_data(id INTEGER PRIMARY KEY,bytes BLOB);
         INSERT INTO extra_data VALUES(91,x'010203');",
        )
        .expect("metadata");
    path
}

pub(super) fn ordered(path: &Path) -> Vec<SavedCircularCut> {
    let d = Document::open_read_only(path).expect("read");
    let source = ExtrudeEditSource::read(&d).expect("snapshot");
    let (_, tip) = tip(&d);
    let last = source
        .cut_features
        .iter()
        .find(|c| c.feature == tip)
        .expect("tip")
        .saved
        .as_ref()
        .expect("supported");
    last.tools
        .iter()
        .map(|tool| {
            source
                .cut_features
                .iter()
                .find(|c| c.feature == tool.feature)
                .expect("choice")
                .saved
                .clone()
                .expect("saved")
        })
        .collect()
}

#[test]
fn bounded_catalog_all_links_and_distant_conflicts_without_kernel() {
    for n in [1, 2, 3, 4, 16] {
        let root = tempfile::tempdir().expect("root");
        let tools = tools(n);
        let path = fixture(root.path(), &tools);
        let original = std::fs::read(&path).expect("bytes");
        let saved = ordered(&path);
        assert_eq!(saved.len(), n);
        let json = inspect(&path);
        assert_eq!(json["bodies"][0]["cut_edit"]["available"], n < 16);
        for (index, s) in saved.iter().enumerate() {
            assert_eq!(s.tools.len(), n);
            assert_eq!(s.neighboring_tool.is_some(), n == 2);
            assert_eq!(s.center_mm, tools[index].center_mm);
            assert_eq!(
                s.previous_feature,
                if index == 0 {
                    s.base_feature
                } else {
                    saved[index - 1].feature
                }
            );
            let f = json["features"]
                .as_array()
                .expect("features")
                .iter()
                .find(|f| f["feature_id"] == json!(s.feature))
                .expect("feature");
            assert_eq!(
                f["circular_cut_edit"]["saved"]["tools"]
                    .as_array()
                    .expect("tools")
                    .len(),
                n
            );
            let d = Document::open_read_only(&path).expect("doc");
            if tools[index].extent.blind_depth_mm().expect("blind") < 12. {
                let error = ferritecad_document::prepare_cut_parameters(
                    &d,
                    s.feature,
                    &CircularCutEdit {
                        tool_curve: s.tool_curve,
                        center_mm: s.center_mm,
                        radius_mm: s.radius_mm,
                        extent: CutExtent::Blind { depth_mm: 12. },
                        vocabulary: ExtentVocabulary::BlindOrThroughAll,
                    },
                )
                .expect_err("all protected floors");
                assert_eq!(s.protected_floor_references.len(), n - index);
                for id in &s.protected_floor_references {
                    assert!(error.to_string().contains(&id.to_string()));
                }
            }
            if n > 2 {
                let other = &saved[(index + 2) % n];
                let error = ferritecad_document::prepare_cut_parameters(
                    &d,
                    s.feature,
                    &CircularCutEdit {
                        tool_curve: s.tool_curve,
                        center_mm: other.center_mm,
                        radius_mm: s.radius_mm,
                        extent: CutExtent::Blind { depth_mm: 2. },
                        vocabulary: ExtentVocabulary::BlindOrThroughAll,
                    },
                )
                .expect_err("distant disk conflict");
                assert!(
                    error.to_string().contains(&other.feature.to_string()),
                    "{error}"
                );
            }
        }
        assert_eq!(std::fs::read(path).expect("bytes"), original);
    }
}

#[test]
fn native_history_origins_floors_sql_and_old_sidecars() {
    if !native() {
        return;
    }
    use CacheOutcome::{Hit, Miss};
    for n in [3, 4, 16] {
        let root = tempfile::tempdir().expect("root");
        let tools = tools(n);
        let source = fixture(root.path(), &tools);
        let original = std::fs::read(&source).expect("bytes");
        let saved = ordered(&source);
        measure_history(&source, &tools, Some(&vec![Miss; n + 1]));
        measure_history(&source, &tools, Some(&vec![Hit; n + 1]));
        for index in [0, n / 2, n - 1] {
            let mut changed = tools.clone();
            changed[index].center_mm[0] += 0.375;
            changed[index].center_mm[1] -= 0.25;
            changed[index].radius_mm += 0.125;
            changed[index].extent = CutExtent::Blind { depth_mm: 2.75 };
            let copy = root.path().join(format!("edit-{n}-{index}.fcad"));
            edit(&source, &copy, &saved[index], changed[index], 0);
            let adds = if tools[index].extent.blind_depth_mm().expect("blind") == 12. {
                n - index
            } else {
                0
            };
            only_numbers(&source, &copy, &saved[index], adds);
            let cold = measure_history(&copy, &changed, None);
            std::fs::copy(
                source.with_extension("fcad-cache"),
                copy.with_extension("fcad-cache"),
            )
            .expect("actual old sidecar");
            let expected: Vec<_> = (0..=n)
                .map(|i| if i <= index { Hit } else { Miss })
                .collect();
            assert_eq!(measure_history(&copy, &changed, Some(&expected)), cold);
            assert_eq!(
                measure_history(&copy, &changed, Some(&vec![Hit; n + 1])),
                cold
            );
            check_history_mesh(&mesh(&copy, &copy.with_extension("stl")), &changed);
            let current = ordered(&copy);
            let again = root.path().join(format!("again-{n}-{index}.fcad"));
            edit(&copy, &again, &current[index], changed[index], 0);
            only_numbers(&copy, &again, &current[index], 0);
            let never = root.path().join("never.fcad");
            let failure = edit(
                &copy,
                &never,
                &current[index],
                CircularCut {
                    extent: CutExtent::Blind { depth_mm: 12. },
                    ..changed[index]
                },
                2,
            );
            assert_eq!(current[index].protected_floor_references.len(), n - index);
            for id in &current[index].protected_floor_references {
                assert!(failure.to_string().contains(&id.to_string()));
            }
            assert!(!never.exists());
            if n <= 4 {
                let fbx = copy.with_extension("fbx");
                reply(
                    cli()
                        .arg("export-fbx")
                        .arg(&copy)
                        .arg("-o")
                        .arg(&fbx)
                        .arg("--json")
                        .output()
                        .expect("export"),
                    "export-fbx",
                    0,
                );
                if let Some(dir) = std::env::var_os("FCAD_CUT_HISTORY_ARTIFACTS") {
                    std::fs::create_dir_all(&dir).expect("artifacts");
                    std::fs::copy(
                        &fbx,
                        Path::new(&dir).join(format!("history-{n}-{index}.fbx")),
                    )
                    .expect("keep");
                }
            }
            assert_eq!(std::fs::read(&source).expect("bytes"), original);
        }
    }
}

#[test]
fn native_adds_sixteen_and_refuses_seventeenth() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut source = fixture(root.path(), &[]);
    let tools = tools(16);
    for (index, tool) in tools.iter().enumerate() {
        let original = std::fs::read(&source).expect("bytes");
        let dest = root.path().join(format!("cut-{}.fcad", index + 1));
        ask(&source, &dest, *tool, 0);
        assert_eq!(std::fs::read(&source).expect("bytes"), original);
        let d = Document::open_read_only(&source).expect("source");
        only_the_cut_was_added(&source, &dest, tip(&d).0);
        let a = tables(&source);
        let b = tables(&dest);
        for table in ["capabilities", "topology_refs"] {
            for row in &a[table].1 {
                assert!(b[table].1.contains(row), "saved {table} rowid/cell changed");
            }
        }
        assert_eq!(
            b["topology_refs"].1.len() - a["topology_refs"].1.len(),
            6 + index
                + 1
                + tools[..=index]
                    .iter()
                    .filter(|t| t.extent.blind_depth_mm().expect("blind") < 12.)
                    .count()
        );
        let role = a["deps"].0.iter().position(|c| c == "role").expect("role");
        for row in &a["deps"].1 {
            if row[role] != rusqlite::types::Value::Text("body_tip".into()) {
                assert!(b["deps"].1.contains(row), "old dependency rowid changed");
            }
        }
        assert_eq!(b["deps"].1.len(), a["deps"].1.len() + 3);
        d.close().expect("close");
        source = dest;
    }
    assert_eq!(ordered(&source).len(), 16);
    let never = root.path().join("seventeen.fcad");
    let error = ask(
        &source,
        &never,
        CircularCut {
            center_mm: [40., 25.],
            radius_mm: 1.,
            extent: CutExtent::Blind { depth_mm: 2. },
        },
        2,
    );
    assert!(error.to_string().contains("16"));
    assert!(!never.exists());
    measure_history(&source, &tools, None);
    check_history_mesh(&mesh(&source, &source.with_extension("stl")), &tools);
}

#[test]
#[cfg(not(feature = "planegcs"))]
fn occt_without_solver_builds_and_edits_history() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut tools = tools(4);
    let source = fixture(root.path(), &tools);
    let saved = ordered(&source);
    tools[0].extent = CutExtent::Blind { depth_mm: 2.5 };
    let copy = root.path().join("edited.fcad");
    edit(&source, &copy, &saved[0], tools[0], 0);
    measure_history(&copy, &tools, None);
}

#[test]
fn native_history_late_guards_aliases_and_exit_seven() {
    super::edits::history_refusals(4);
}
