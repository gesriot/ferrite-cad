// SPDX-License-Identifier: MIT
//! §26H: an explicit ThroughAll Cut stays through across height edits, reopen,
//! cold rebuilds and cache hits; mode transitions keep, add or refuse names.
use super::*;
use ferritecad_document::{
    SavedCircularCut, TopologyRef, prepare_cut_parameters, prepare_extrude_height,
};
use ferritecad_types::ObjectId;

const BOUNDS: [[f64; 2]; 2] = [[0., 0.], [80., 50.]];
const EDIT: &str = "edit-circular-cut";

/// Mixed ends at fractional, off-axis positions in a history order that is not
/// the geometric order: ThroughAll, a Blind cut exactly as deep as the plate,
/// and a pocket, repeating.
fn tools(n: usize) -> Vec<CircularCut> {
    history::tools(n)
        .into_iter()
        .enumerate()
        .map(|(i, t)| CircularCut {
            center_mm: [t.center_mm[0] + 0.125, t.center_mm[1] - 0.375],
            radius_mm: t.radius_mm + 0.0625,
            extent: match i % 3 {
                0 => CutExtent::ThroughAll,
                1 => CutExtent::Blind { depth_mm: SIZE[2] },
                _ => CutExtent::Blind {
                    depth_mm: 3.5 + (i % 7) as f64,
                },
            },
        })
        .collect()
}

fn extent_json(extent: CutExtent) -> Value {
    match extent {
        CutExtent::Blind { depth_mm } => json!({"kind":"blind","depth_mm":depth_mm}),
        CutExtent::ThroughAll => json!({"kind":"through_all"}),
    }
}

fn add(source: &Path, destination: &Path, cut: CircularCut, code: i32) -> Value {
    let catalog = inspect(source);
    let request = source.with_extension("add.json");
    write(
        &request,
        &json!({"request_version":2,"center_mm":cut.center_mm,"radius_mm":cut.radius_mm,
                "extent":extent_json(cut.extent)}),
    );
    reply(
        cli()
            .arg(OP)
            .arg(source)
            .args([
                "--body",
                catalog["bodies"][0]["body_id"].as_str().expect("body"),
                "--expect-version",
                catalog["content_version"].as_str().expect("version"),
                "--request",
            ])
            .arg(request)
            .arg("-o")
            .arg(destination)
            .arg("--json")
            .output()
            .expect("cut"),
        OP,
        code,
    )
}

/// A plate built through the real CLI, one request v2 per link.
fn build(root: &Path, tools: &[CircularCut]) -> PathBuf {
    let mut source = root.join("plate.fcad");
    write_plate_document(&source, SIZE[0], SIZE[1], SIZE[2]);
    for (i, tool) in tools.iter().enumerate() {
        let next = root.join(format!("link-{}.fcad", i + 1));
        let result = add(&source, &next, *tool, 0);
        assert_eq!(result["result"]["extent"], extent_json(tool.extent));
        source = next;
    }
    source
}

fn edit(
    source: &Path,
    destination: &Path,
    saved: &SavedCircularCut,
    cut: CircularCut,
    version: u32,
    code: i32,
) -> Value {
    let catalog = inspect(source);
    let request = source.with_extension("edit.json");
    let mut body = json!({"request_version":version,"tool_curve_id":saved.tool_curve,
        "center_mm":cut.center_mm,"radius_mm":cut.radius_mm});
    match version {
        1 => body["depth_mm"] = json!(cut.extent.blind_depth_mm().expect("v1 is Blind")),
        _ => body["extent"] = extent_json(cut.extent),
    }
    write(&request, &body);
    reply(
        cli()
            .arg(EDIT)
            .arg(source)
            .arg("--feature")
            .arg(saved.feature.to_string())
            .arg("--expect-version")
            .arg(catalog["content_version"].as_str().expect("version"))
            .arg("--request")
            .arg(request)
            .arg("-o")
            .arg(destination)
            .arg("--json")
            .output()
            .expect("edit"),
        EDIT,
        code,
    )
}

fn height(source: &Path, destination: &Path, height: f64, code: i32) -> Value {
    let catalog = inspect(source);
    let base = catalog["features"]
        .as_array()
        .expect("features")
        .iter()
        .find(|f| !f["base_height_edit"].is_null())
        .expect("base")["feature_id"]
        .as_str()
        .expect("id")
        .to_owned();
    reply(
        cli()
            .arg("edit-extrude")
            .arg(source)
            .args(["--feature", &base, "--expect-version"])
            .arg(catalog["content_version"].as_str().expect("version"))
            .args(["--distance-mm", &height.to_string(), "-o"])
            .arg(destination)
            .arg("--json")
            .output()
            .expect("height"),
        "edit-extrude",
        code,
    )
}

/// Every saved name of this Cut's floor, wherever it is stored.
fn floor_names(path: &Path, feature: ObjectId) -> Vec<TopologyRef> {
    Document::open_read_only(path)
        .expect("doc")
        .topology_refs()
        .expect("refs")
        .into_iter()
        .filter(|r| match r.output_role {
            SemanticRole::ExtrudeCap {
                side: ferritecad_document::CapSide::End,
            } => r.producer_feature == feature,
            SemanticRole::OriginCap {
                origin_feature,
                side: ferritecad_document::CapSide::End,
            } => origin_feature == feature,
            _ => false,
        })
        .collect()
}

/// The SQL allowlist of one Cut edit: the tool Sketch and Cut payload/hash,
/// the Cut's `schema_version` column when its end changes layout, the one
/// `feature.through-all.v1` index row when first needed, the added refs and
/// `meta.modified_at`. Every other cell and every rowid is unchanged.
fn only_the_cut(
    source: &Path,
    copy: &Path,
    saved: &SavedCircularCut,
    added: usize,
    layout: bool,
    capability: bool,
) {
    use rusqlite::types::Value as Sql;
    let a = tables(source);
    let b = tables(copy);
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (name, (columns, rows)) in &a {
        let (cols, now) = &b[name];
        assert_eq!(columns, cols);
        if name == "topology_refs" {
            assert_eq!(now.len(), rows.len() + added, "added refs");
            assert!(rows.iter().all(|r| now.contains(r)), "a saved ref moved");
            continue;
        }
        if name == "capabilities" && capability {
            assert_eq!(
                &now[..rows.len()],
                rows.as_slice(),
                "capability rowids kept"
            );
            assert_eq!(now.len(), rows.len() + 1);
            let last = &now[rows.len()];
            assert!(
                last.contains(&Sql::Text(
                    ferritecad_document::FEATURE_THROUGH_ALL_CAPABILITY.to_owned()
                )),
                "{last:?}"
            );
            continue;
        }
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
                            || layout && row(saved.feature) && columns[i] == "schema_version"
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
fn through_all_discovery_and_requests_without_kernel() {
    let root = tempfile::tempdir().expect("root");
    let tools = tools(4);
    let path = history::fixture(root.path(), &tools);
    let before = std::fs::read(&path).expect("bytes");
    let catalog = inspect(&path);
    let target = &catalog["bodies"][0]["cut_edit"]["target"];
    assert_eq!(target["request_versions"], json!([1, 2]));
    let listed = target["tools"].as_array().expect("tools");
    assert_eq!(listed.len(), 4);
    for (tool, listed) in tools.iter().zip(listed) {
        assert_eq!(listed["extent"], extent_json(tool.extent));
        match tool.extent {
            CutExtent::ThroughAll => assert!(listed["depth_mm"].is_null(), "no invented depth"),
            CutExtent::Blind { depth_mm } => assert_eq!(listed["depth_mm"], json!(depth_mm)),
        }
    }
    let saved: Vec<_> = catalog["features"]
        .as_array()
        .expect("features")
        .iter()
        .filter(|f| !f["circular_cut_edit"]["saved"].is_null())
        .map(|f| f["circular_cut_edit"]["saved"].clone())
        .collect();
    assert_eq!(saved.len(), 4);
    for s in &saved {
        let through = s["extent"]["kind"] == "through_all";
        assert_eq!(
            s["request_versions"],
            if through { json!([2]) } else { json!([1, 2]) }
        );
        assert_eq!(s["depth_mm"].is_null(), through);
        if through {
            assert_eq!(s["leaves_a_floor"], json!(false));
            assert!(s["floor_reference_id"].is_null());
            assert_eq!(s["through_allowed"], json!(true));
        }
    }
    let height_tools = catalog["features"]
        .as_array()
        .expect("features")
        .iter()
        .find(|f| !f["base_height_edit"].is_null())
        .expect("base")["base_height_edit"]["tools"]
        .clone();
    assert_eq!(height_tools, target["tools"]);

    // The layout and the document-level declaration a reader negotiates on.
    let d = Document::open_read_only(&path).expect("doc");
    for object in d.objects().expect("objects") {
        if let ObjectPayload::Extrude(e) = &object.payload {
            let expected = match (&e.previous, &e.end_condition) {
                (None, _) => 1,
                (Some(_), ferritecad_document::EndCondition::ThroughAll) => 3,
                _ => 2,
            };
            // The decoded payload's own layout (its envelope is checked against
            // it on read) and the SQL column capability negotiation reads.
            assert_eq!(object.payload.schema_version(), expected);
            let column: u32 = rusqlite::Connection::open(&path)
                .expect("sql")
                .query_row(
                    "SELECT schema_version FROM objects WHERE id=?1",
                    [object.id.to_bytes().to_vec()],
                    |r| r.get(0),
                )
                .expect("column");
            assert_eq!(column, expected);
        }
    }
    d.close().expect("close");
    assert!(tables(&path)["capabilities"].1.iter().any(|r| r.contains(
        &rusqlite::types::Value::Text(
            ferritecad_document::FEATURE_THROUGH_ALL_CAPABILITY.to_owned()
        )
    )));
    let valid = reply(
        cli()
            .arg("validate")
            .arg(&path)
            .arg("--json")
            .output()
            .expect("validate"),
        "validate",
        0,
    );
    assert_eq!(valid["result"]["valid"], json!(true));

    // Request forms refused before any kernel or document work, in any build.
    let never = root.path().join("never.fcad");
    let first = history::ordered(&path).remove(0);
    let body = catalog["bodies"][0]["body_id"]
        .as_str()
        .expect("body")
        .to_owned();
    let version = catalog["content_version"]
        .as_str()
        .expect("version")
        .to_owned();
    let curve = first.tool_curve.to_string();
    for (op, bad) in [
        (
            OP,
            json!({"request_version":3,"center_mm":[20,15],"radius_mm":1,"extent":{"kind":"through_all"}}),
        ),
        (
            OP,
            json!({"request_version":2,"center_mm":[20,15],"radius_mm":1,"depth_mm":4}),
        ),
        (
            OP,
            json!({"request_version":2,"center_mm":[20,15],"radius_mm":1,"extent":{"kind":"through_all","depth_mm":4}}),
        ),
        (
            OP,
            json!({"request_version":2,"center_mm":[20,15],"radius_mm":1,"extent":{"kind":"blind"}}),
        ),
        (
            OP,
            json!({"request_version":2,"center_mm":[20,15],"radius_mm":1,"extent":{"kind":"through"}}),
        ),
        (
            OP,
            json!({"request_version":1,"center_mm":[20,15],"radius_mm":1,"extent":{"kind":"through_all"}}),
        ),
        (
            EDIT,
            json!({"request_version":2,"tool_curve_id":curve,"center_mm":[20,15],"radius_mm":1}),
        ),
        (
            EDIT,
            json!({"request_version":2,"tool_curve_id":curve,"center_mm":[20,15],"radius_mm":1,"extent":{"kind":"through_all"},"depth_mm":4}),
        ),
        (
            EDIT,
            json!({"request_version":0,"tool_curve_id":curve,"center_mm":[20,15],"radius_mm":1,"depth_mm":4}),
        ),
    ] {
        let request = root.path().join("bad.json");
        write(&request, &bad);
        let mut c = cli();
        c.arg(op).arg(&path);
        if op == OP {
            c.args(["--body", &body]);
        } else {
            c.args(["--feature", &first.feature.to_string()]);
        }
        let refused = reply(
            c.args(["--expect-version", &version, "--request"])
                .arg(&request)
                .arg("-o")
                .arg(&never)
                .arg("--json")
                .output()
                .expect("refusal"),
            op,
            2,
        );
        assert!(
            ["input", "unsupported"].contains(&refused["error"]["kind"].as_str().expect("kind")),
            "{bad}: {refused}"
        );
        assert!(!never.exists());
    }
    // Request v1 aimed at a ThroughAll Cut: refused. A stub refuses on its
    // kernel first, which the job constructs before preparation.
    assert_eq!(first.extent, CutExtent::ThroughAll);
    let refused = edit(
        &path,
        &never,
        &first,
        CircularCut {
            extent: CutExtent::Blind { depth_mm: 4. },
            ..tools[0]
        },
        1,
        2,
    );
    if native() {
        let message = refused["error"]["message"].as_str().expect("message");
        assert!(message.contains(&first.feature.to_string()), "{message}");
        assert!(message.contains("request_version 2"), "{message}");
    }
    assert!(!never.exists());
    assert_eq!(std::fs::read(&path).expect("bytes"), before);
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
            .filter(|s| s.extent == CutExtent::ThroughAll)
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
                if tool.extent == (CutExtent::Blind { depth_mm: SIZE[2] }) {
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
            // The reopened copy advertises exactly what can still be done.
            let next = inspect(&copy);
            assert_eq!(next["bodies"][0]["cut_edit"]["available"], json!(n < 16));
            let editable = next["features"]
                .as_array()
                .expect("features")
                .iter()
                .filter(|f| f["circular_cut_edit"]["available"] == json!(true))
                .count();
            assert_eq!(editable, n, "every link stays editable");
            assert!(
                next["features"]
                    .as_array()
                    .expect("features")
                    .iter()
                    .any(|f| f["base_height_edit"].is_object() && f["editable"] == json!(true)),
                "the base height stays editable"
            );
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
        let p = prepare_extrude_height(&d, base, 14.25).expect("height");
        d.write_extrude_height(&p).expect("write");
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
        let p = prepare_cut_parameters(
            &d,
            first.feature,
            &ferritecad_document::CircularCutEdit {
                tool_curve: first.tool_curve,
                center_mm: moved[0].center_mm,
                radius_mm: moved[0].radius_mm,
                extent: moved[0].extent,
                vocabulary: ExtentVocabulary::BlindOrThroughAll,
            },
        )
        .expect("first link");
        d.write_cut_parameters(&p).expect("write");
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
        let never = root.path().join("never.fcad");
        for index in [0, n / 2, n - 1] {
            let saved = &ordered[index];
            let tool = tools[index];
            let later = n - index;
            match tool.extent {
                CutExtent::ThroughAll => {
                    // ThroughAll -> pocket: its own floor and one origin floor
                    // at every later producer, and back to payload v2.
                    let copy = root.path().join(format!("pocket-{index}.fcad"));
                    let changed = CircularCut {
                        center_mm: [tool.center_mm[0] - 0.25, tool.center_mm[1] + 0.125],
                        radius_mm: tool.radius_mm + 0.125,
                        extent: CutExtent::Blind { depth_mm: 2.75 },
                    };
                    let out = edit(&source, &copy, saved, changed, 2, 0);
                    assert_eq!(out["result"]["extent"], extent_json(changed.extent));
                    assert_eq!(out["result"]["leaves_a_floor"], json!(true));
                    only_the_cut(&source, &copy, saved, later, true, false);
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
                    // Request v1 cannot state this Cut's intent.
                    let refused = edit(&source, &never, saved, changed, 1, 2);
                    let message = refused["error"]["message"].as_str().expect("message");
                    assert!(
                        message.contains(&saved.feature.to_string())
                            && message.contains("request_version 2"),
                        "{message}"
                    );
                    assert!(!never.exists());
                    // Numbers only, intent kept: nothing added, layout unchanged.
                    let copy = root.path().join(format!("moved-{index}.fcad"));
                    let moved = CircularCut {
                        radius_mm: tool.radius_mm - 0.25,
                        ..tool
                    };
                    edit(&source, &copy, saved, moved, 2, 0);
                    only_the_cut(&source, &copy, saved, 0, false, false);
                }
                CutExtent::Blind { depth_mm } if depth_mm == SIZE[2] => {
                    // Blind through -> ThroughAll keeps every name; it stays
                    // through after the base grows; and back again keeps them.
                    let copy = root.path().join(format!("intent-{index}.fcad"));
                    let changed = CircularCut {
                        extent: CutExtent::ThroughAll,
                        ..tool
                    };
                    let out = edit(&source, &copy, saved, changed, 2, 0);
                    assert_eq!(out["result"]["leaves_a_floor"], json!(false));
                    only_the_cut(&source, &copy, saved, 0, true, false);
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
                    edit(&copy, &back, &history::ordered(&copy)[index], tool, 2, 0);
                    only_the_cut(
                        &copy,
                        &back,
                        &history::ordered(&copy)[index],
                        0,
                        true,
                        false,
                    );
                }
                CutExtent::Blind { .. } => {
                    // A pocket whose floor is named refuses ThroughAll during
                    // preparation, naming the Cut and every protected UUID.
                    assert!(!saved.protected_floor_references.is_empty());
                    let refused = edit(
                        &source,
                        &never,
                        saved,
                        CircularCut {
                            extent: CutExtent::ThroughAll,
                            ..tool
                        },
                        2,
                        2,
                    );
                    let message = refused["error"]["message"].as_str().expect("message");
                    assert!(message.contains(&saved.feature.to_string()), "{message}");
                    for id in &saved.protected_floor_references {
                        assert!(message.contains(&id.to_string()), "{message}");
                    }
                    assert!(!never.exists());
                }
            }
        }
        // A plate that has never declared ThroughAll gains exactly one index row.
        let blind_root = tempfile::tempdir().expect("root");
        let blind = build(
            blind_root.path(),
            &[CircularCut {
                extent: CutExtent::Blind { depth_mm: SIZE[2] },
                ..tools[0]
            }],
        );
        let saved = &history::ordered(&blind)[0];
        let copy = blind_root.path().join("first-intent.fcad");
        edit(
            &blind,
            &copy,
            saved,
            CircularCut {
                extent: CutExtent::ThroughAll,
                ..tools[0]
            },
            2,
            0,
        );
        only_the_cut(&blind, &copy, saved, 0, true, true);
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
