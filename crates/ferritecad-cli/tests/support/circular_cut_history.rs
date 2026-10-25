// SPDX-License-Identifier: MIT
//! §26E: one bounded chain, including distant dependencies and disk conflicts.
use super::edits::{edit, only_numbers};
use super::*;

use crate::cut_facts::CutObservation;

pub(super) fn tools(n: usize) -> Vec<CutSpec> {
    (0..n)
        .map(|i| {
            // Permutation deliberately separates geometric order from history order.
            let slot = (i * 7) % 16;
            CutSpec {
                center_mm: [10. + (slot % 4) as f64 * 19., 7. + (slot / 4) as f64 * 12.],
                radius_mm: 1.5 + (i % 5) as f64 * 0.25,
                extent: CutDepth::Blind {
                    depth_mm: if i % 2 == 0 { 12. } else { 3. + (i % 7) as f64 },
                },
            }
        })
        .collect()
}

pub(super) fn fixture(root: &Path, tools: &[CutSpec]) -> PathBuf {
    let path = root.join("history.fcad");
    write_plate_document(&path, SIZE[0], SIZE[1], SIZE[2]);
    let mut d = Document::open(&path).expect("doc");
    let (body, _) = tip(&d);
    for tool in tools {
        let p = (tool).additions(&d, body);
        d.write_changes(&p).expect("write link");
    }
    d.close().expect("close");
    rusqlite::Connection::open(&path)
        .expect("SQL")
        .execute_batch(
            "UPDATE objects SET rowid=-rowid,ordinal=100-ordinal,name='same';


         CREATE TABLE extra_data(id INTEGER PRIMARY KEY,bytes BLOB);
         INSERT INTO extra_data VALUES(91,x'010203');",
        )
        .expect("metadata");
    path
}

pub(super) fn ordered(path: &Path) -> Vec<CutObservation> {
    crate::cut_facts::observed(path)
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
            changed[index].extent = CutDepth::Blind { depth_mm: 2.75 };
            let copy = root.path().join(format!("edit-{n}-{index}.fcad"));
            edit(&source, &copy, &saved[index], changed[index], 0);
            only_numbers(&source, &copy, &saved[index]);
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
            only_numbers(&copy, &again, &current[index]);
            let through = root.path().join(format!("through-{n}-{index}.fcad"));
            let floors = crate::names::floor_names(
                &Document::open_read_only(&copy).expect("copy"),
                current[index].feature,
            );
            assert_eq!(floors.len(), n - index);
            let mut no_floor = changed.clone();
            no_floor[index].extent = CutDepth::Blind { depth_mm: 12. };
            edit(&copy, &through, &current[index], no_floor[index], 0);
            assert!(
                crate::names::floor_names(
                    &Document::open_read_only(&through).expect("through"),
                    current[index].feature
                )
                .is_empty()
            );
            measure_history(&through, &no_floor, None);
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
fn native_adds_seventeen_with_names_cold_geometry_and_mesh() {
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
        let next = Document::open_read_only(&dest).expect("copy");
        let prior_names = crate::names::snapshot(&d);
        let next_names = crate::names::snapshot(&next);
        let added = ordered(&dest)[index].feature;
        crate::names::assert_added(
            &prior_names,
            &next_names,
            added,
            6 + index
                + 1
                + tools[..=index]
                    .iter()
                    .filter(|t| t.extent.blind_depth_mm().expect("blind") < 12.)
                    .count(),
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
    let next = root.path().join("seventeen.fcad");
    let extra = CutSpec {
        center_mm: [40., 25.],
        radius_mm: 1.,
        extent: CutDepth::Blind { depth_mm: 2. },
    };
    ask(&source, &next, extra, 0);
    let mut all = tools;
    all.push(extra);
    assert_eq!(ordered(&next).len(), 17);
    measure_history(&next, &all, None);
    check_history_mesh(&mesh(&next, &next.with_extension("stl")), &all);
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
    tools[0].extent = CutDepth::Blind { depth_mm: 2.5 };
    let copy = root.path().join("edited.fcad");
    edit(&source, &copy, &saved[0], tools[0], 0);
    measure_history(&copy, &tools, None);
}

#[test]
fn native_history_late_guards_aliases_and_exit_seven() {
    super::edits::history_refusals(4);
}
