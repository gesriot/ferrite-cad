// SPDX-License-Identifier: MIT
//! §26H: an explicit ThroughAll Cut stays through across height edits, reopen,
//! cold rebuilds and cache hits; mode transitions keep, add or refuse names.
use super::*;
use crate::cut_facts::CutObservation;
use ferritecad_types::ObjectId;

const BOUNDS: [[f64; 2]; 2] = [[0., 0.], [80., 50.]];

/// Mixed ends at fractional, off-axis positions in a history order that is not
/// the geometric order: ThroughAll, a Blind cut exactly as deep as the plate,
/// and a pocket, repeating.
fn tools(n: usize) -> Vec<CutSpec> {
    history::tools(n)
        .into_iter()
        .enumerate()
        .map(|(i, t)| CutSpec {
            center_mm: [t.center_mm[0] + 0.125, t.center_mm[1] - 0.375],
            radius_mm: t.radius_mm + 0.0625,
            extent: match i % 3 {
                0 => CutDepth::ThroughAll,
                1 => CutDepth::Blind { depth_mm: SIZE[2] },
                _ => CutDepth::Blind {
                    depth_mm: 3.5 + (i % 7) as f64,
                },
            },
        })
        .collect()
}

fn add(source: &Path, destination: &Path, cut: CutSpec, code: i32) -> Value {
    let request = source.with_extension("request.json");
    crate::cut_facts::write_add(source, &request, cut);
    reply(
        transaction::command(
            source,
            inspect(source)["content_version"]
                .as_str()
                .expect("version"),
            &request,
            destination,
        )
        .output()
        .expect("edit"),
        "edit",
        code,
    )
}

/// A plate built through the real CLI, one request v2 per link.
fn build(root: &Path, tools: &[CutSpec]) -> PathBuf {
    let mut source = root.join("plate.fcad");
    write_plate_document(&source, SIZE[0], SIZE[1], SIZE[2]);
    for (i, tool) in tools.iter().enumerate() {
        let next = root.join(format!("link-{}.fcad", i + 1));
        add(&source, &next, *tool, 0);
        assert_eq!(
            history::ordered(&next).last().expect("Cut").extent,
            tool.extent
        );
        source = next;
    }
    source
}

fn edit(
    source: &Path,
    destination: &Path,
    saved: &CutObservation,
    cut: CutSpec,
    code: i32,
) -> Value {
    let request = source.with_extension("edit.json");
    crate::cut_facts::write_edit(&request, saved, cut);
    reply(
        transaction::command(
            source,
            inspect(source)["content_version"]
                .as_str()
                .expect("version"),
            &request,
            destination,
        )
        .output()
        .expect("edit"),
        "edit",
        code,
    )
}

fn height(source: &Path, destination: &Path, height: f64, code: i32) -> Value {
    let catalog = inspect(source);
    let d = Document::open_read_only(source).expect("document");
    let object = d.objects().expect("objects").into_iter().find(|o|matches!(&o.payload,ferritecad_document::ObjectPayload::Extrude(e) if e.previous.is_none())).expect("base").id;
    let request = tempfile::NamedTempFile::new().expect("request");
    crate::transaction::write(
        request.path(),
        &object.to_string(),
        "extrude_distance",
        height,
        "mm",
    );
    reply(
        crate::transaction::command(
            source,
            catalog["content_version"].as_str().expect("version"),
            request.path(),
            destination,
        )
        .output()
        .expect("height"),
        "edit",
        code,
    )
}

/// Every saved name of this Cut's floor, wherever it is stored.
fn floor_names(path: &Path, feature: ObjectId) -> crate::names::Names {
    crate::names::floor_names(&Document::open_read_only(path).expect("doc"), feature)
}

/// The SQL allowlist of one Cut edit: the tool Sketch and Cut payload/hash,
/// the Cut's `schema_version` column when its end changes layout, the one
/// `feature.through-all.v1` index row when first needed, the added refs and
/// `meta.modified_at`. Every other cell and every rowid is unchanged.
fn only_the_cut(source: &Path, copy: &Path, saved: &CutObservation, _added: usize) {
    use rusqlite::types::Value as Sql;
    let a = tables(source);
    let b = tables(copy);
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (name, (columns, rows)) in &a {
        let (cols, now) = &b[name];
        assert_eq!(columns, cols);

        assert_eq!(rows.len(), now.len(), "{name} rows");
        for (old, new) in rows.iter().zip(now) {
            for (i, (x, y)) in old.iter().zip(new).enumerate() {
                if x == y {
                    continue;
                }
                let id = columns.iter().position(|c| c == "id");
                let row =
                    |v: ObjectId| id.is_some_and(|id| old[id] == Sql::Blob(v.to_bytes().to_vec()));
                let allowed = match name.as_str() {
                    "meta" => columns[i] == "modified_at",
                    "objects" => {
                        (row(saved.feature) || row(saved.tool_sketch))
                            && matches!(columns[i].as_str(), "payload" | "payload_hash")
                    }
                    _ => false,
                };
                assert!(
                    allowed,
                    "unexpected cell {name}.{}: {x:?} -> {y:?}",
                    columns[i]
                );
            }
        }
    }
    let a = Document::open_read_only(source).expect("source");
    let b = Document::open_read_only(copy).expect("copy");
    assert_eq!(a.meta().document_id, b.meta().document_id);
    assert_eq!(
        a.dependencies().expect("deps"),
        b.dependencies().expect("deps")
    );
    assert_eq!(tip(&a), tip(&b));
}

#[test]
fn native_through_all_survives_height_reopen_cold_and_cache() {
    if !native() {
        return;
    }
    use CacheOutcome::{Hit, Miss};
    let mut artifact = 0;
    for n in [1, 2, 4, 16] {
        let root = tempfile::tempdir().expect("root");
        let tools = tools(n);
        let source = build(root.path(), &tools);
        let original = std::fs::read(&source).expect("bytes");
        let ordered = history::ordered(&source);
        let through: Vec<_> = ordered
            .iter()
            .filter(|s| s.extent == CutDepth::ThroughAll)
            .map(|s| s.feature)
            .collect();
        assert!(!through.is_empty());
        let cold = measure_history_at(&source, &tools, BOUNDS, SIZE[2], None);
        check_history_mesh_at(
            &mesh(&source, &root.path().join("source.stl")),
            &tools,
            BOUNDS,
            SIZE[2],
        );
        for feature in &through {
            assert!(
                floor_names(&source, *feature).is_empty(),
                "ThroughAll has no floor"
            );
        }

        // 12 -> 14.25 -> 13 on unchanged sources: ThroughAll stays through;
        // a Blind 12 becomes a pocket and gains its floor names.
        let mut from = source.clone();
        for h in [14.25, 13.] {
            let copy = root.path().join(format!("h-{h}.fcad"));
            height(&from, &copy, h, 0);
            for feature in &through {
                assert!(floor_names(&copy, *feature).is_empty(), "no floor at {h}");
            }
            for (tool, saved) in tools.iter().zip(&ordered) {
                if tool.extent == (CutDepth::Blind { depth_mm: SIZE[2] }) {
                    let later = ordered
                        .iter()
                        .skip_while(|s| s.feature != saved.feature)
                        .count();
                    assert_eq!(
                        floor_names(&copy, saved.feature).len(),
                        later,
                        "grown floor"
                    );
                }
            }
            let at = measure_history_at(&copy, &tools, BOUNDS, h, None);
            assert_eq!(
                at,
                measure_history_at(&copy, &tools, BOUNDS, h, Some(&vec![Miss; n + 1]))
            );
            assert_eq!(
                at,
                measure_history_at(&copy, &tools, BOUNDS, h, Some(&vec![Hit; n + 1]))
            );
            check_history_mesh_at(
                &mesh(&copy, &root.path().join(format!("h-{h}.stl"))),
                &tools,
                BOUNDS,
                h,
            );
            let next = inspect(&copy);
            assert_eq!(
                next["bodies"][0]["history"]
                    .as_array()
                    .expect("history")
                    .len(),
                n + 1
            );
            assert_eq!(history::ordered(&copy).len(), n);

            if n == 4 && artifact < 2 {
                if let Some(dir) = std::env::var_os("FCAD_CUT_THROUGH_ALL_ARTIFACTS") {
                    let dir = Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("artifacts");
                    let fbx = dir.join(format!("through-{artifact}.fbx"));
                    reply(
                        cli()
                            .arg("export-fbx")
                            .arg(&copy)
                            .arg("-o")
                            .arg(&fbx)
                            .arg("--json")
                            .output()
                            .expect("fbx"),
                        "export-fbx",
                        0,
                    );
                }
                artifact += 1;
            }
            from = copy;
        }
        assert_eq!(std::fs::read(&source).expect("source"), original);

        // The same path, the same sidecar: fill it, then change the file under
        // it. A height change misses every link; nothing old is served.
        assert_eq!(
            cold,
            measure_history_at(&source, &tools, BOUNDS, SIZE[2], Some(&vec![Miss; n + 1]))
        );
        assert_eq!(
            cold,
            measure_history_at(&source, &tools, BOUNDS, SIZE[2], Some(&vec![Hit; n + 1]))
        );
        let mut d = Document::open(&source).expect("doc");
        let base = ordered[0].base_feature;
        let p = ferritecad_document::EditChange::constant(
            base,
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
            14.25,
        )
        .expect("height");
        d.write_changes(&[p]).expect("write");
        d.close().expect("close");
        let grown = measure_history_at(&source, &tools, BOUNDS, 14.25, Some(&vec![Miss; n + 1]));
        assert_eq!(
            grown,
            measure_history_at(&source, &tools, BOUNDS, 14.25, Some(&vec![Hit; n + 1]))
        );
        assert_eq!(
            grown,
            measure_history_at(&source, &tools, BOUNDS, 14.25, None)
        );

        // An early link: moving the first (ThroughAll) tool misses it and
        // everything after it, and still hits the base.
        let mut d = Document::open(&source).expect("doc");
        let first = history::ordered(&source).remove(0);
        let mut moved = tools.clone();
        moved[0].center_mm[0] += 0.25;
        d.write_changes(&moved[0].changes(&first)).expect("write");
        d.close().expect("close");
        let mut expected = vec![Miss; n + 1];
        expected[0] = Hit;
        let changed = measure_history_at(&source, &moved, BOUNDS, 14.25, Some(&expected));
        assert_eq!(
            changed,
            measure_history_at(&source, &moved, BOUNDS, 14.25, None)
        );
    }
    assert_eq!(artifact, 2);
}

#[test]
fn native_through_all_transitions_refs_sql_and_refusals() {
    if !native() {
        return;
    }
    for n in [4, 16] {
        let root = tempfile::tempdir().expect("root");
        let tools = tools(n);
        let source = build(root.path(), &tools);
        let original = std::fs::read(&source).expect("bytes");
        let ordered = history::ordered(&source);
        for index in [0, n / 2, n - 1] {
            let saved = &ordered[index];
            let tool = tools[index];
            let later = n - index;
            match tool.extent {
                CutDepth::ThroughAll => {
                    // ThroughAll -> pocket: its own floor and one origin floor
                    // at every later producer, and back to payload v2.
                    let copy = root.path().join(format!("pocket-{index}.fcad"));
                    let changed = CutSpec {
                        center_mm: [tool.center_mm[0] - 0.25, tool.center_mm[1] + 0.125],
                        radius_mm: tool.radius_mm + 0.125,
                        extent: CutDepth::Blind { depth_mm: 2.75 },
                    };
                    edit(&source, &copy, saved, changed, 0);
                    assert_eq!(history::ordered(&copy)[index].extent, changed.extent);
                    assert!(!floor_names(&copy, saved.feature).is_empty());
                    only_the_cut(&source, &copy, saved, later);
                    assert_eq!(floor_names(&copy, saved.feature).len(), later);
                    let mut all = tools.clone();
                    all[index] = changed;
                    measure_history_at(&copy, &all, BOUNDS, SIZE[2], None);
                    check_history_mesh_at(
                        &mesh(&copy, &copy.with_extension("stl")),
                        &all,
                        BOUNDS,
                        SIZE[2],
                    );

                    // Numbers only, intent kept: nothing added, layout unchanged.
                    let copy = root.path().join(format!("moved-{index}.fcad"));
                    let moved = CutSpec {
                        radius_mm: tool.radius_mm - 0.25,
                        ..tool
                    };
                    edit(&source, &copy, saved, moved, 0);
                    only_the_cut(&source, &copy, saved, 0);
                }
                CutDepth::Blind { depth_mm } if depth_mm == SIZE[2] => {
                    // Blind through -> ThroughAll keeps every name; it stays
                    // through after the base grows; and back again keeps them.
                    let copy = root.path().join(format!("intent-{index}.fcad"));
                    let changed = CutSpec {
                        extent: CutDepth::ThroughAll,
                        ..tool
                    };
                    edit(&source, &copy, saved, changed, 0);
                    assert!(floor_names(&copy, saved.feature).is_empty());
                    only_the_cut(&source, &copy, saved, 0);
                    let mut all = tools.clone();
                    all[index] = changed;
                    let grown = root.path().join(format!("intent-{index}-grown.fcad"));
                    height(&copy, &grown, 14.25, 0);
                    assert!(floor_names(&grown, saved.feature).is_empty());
                    measure_history_at(&grown, &all, BOUNDS, 14.25, None);
                    check_history_mesh_at(
                        &mesh(&grown, &grown.with_extension("stl")),
                        &all,
                        BOUNDS,
                        14.25,
                    );
                    let back = root.path().join(format!("back-{index}.fcad"));
                    edit(&copy, &back, &history::ordered(&copy)[index], tool, 0);
                    only_the_cut(&copy, &back, &history::ordered(&copy)[index], 0);
                }
                CutDepth::Blind { .. } => {
                    assert!(!floor_names(&source, saved.feature).is_empty());
                    let through = root.path().join(format!("through-{index}.fcad"));
                    let mut changed = tools.clone();
                    changed[index].extent = CutDepth::ThroughAll;
                    edit(&source, &through, saved, changed[index], 0);
                    assert!(floor_names(&through, saved.feature).is_empty());
                    measure_history_at(&through, &changed, BOUNDS, SIZE[2], None);
                    check_history_mesh_at(
                        &mesh(&through, &through.with_extension("stl")),
                        &changed,
                        BOUNDS,
                        SIZE[2],
                    );
                }
            }
        }
        // A plate that has never declared ThroughAll gains exactly one index row.
        let blind_root = tempfile::tempdir().expect("root");
        let blind = build(
            blind_root.path(),
            &[CutSpec {
                extent: CutDepth::Blind { depth_mm: SIZE[2] },
                ..tools[0]
            }],
        );
        let saved = &history::ordered(&blind)[0];
        let copy = blind_root.path().join("first-intent.fcad");
        edit(
            &blind,
            &copy,
            saved,
            CutSpec {
                extent: CutDepth::ThroughAll,
                ..tools[0]
            },
            0,
        );
        only_the_cut(&blind, &copy, saved, 0);
        assert_eq!(std::fs::read(&source).expect("source"), original);
    }
}

/// A kernel and no solver: an unconstrained history needs no planegcs.
#[test]
fn occt_without_solver_keeps_through_all_through() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let tools = tools(2);
    let source = build(root.path(), &tools);
    let grown = root.path().join("grown.fcad");
    height(&source, &grown, 14.25, 0);
    let first = history::ordered(&grown)[0].feature;
    assert!(floor_names(&grown, first).is_empty());
    measure_history_at(&grown, &tools, BOUNDS, 14.25, None);
    check_history_mesh_at(
        &mesh(&grown, &root.path().join("grown.stl")),
        &tools,
        BOUNDS,
        14.25,
    );
}
