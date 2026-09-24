// SPDX-License-Identifier: MIT
//! §26G: absolute Cut depths under a changing plate height.
use super::*;
use ferritecad_document::{ExtrudeChoice, ExtrudeEditSource, TopologyRef, prepare_extrude_height};
use ferritecad_types::ObjectId;
const EDIT: &str = "edit-extrude";
const BOUNDS: [[f64; 2]; 2] = [[0., 0.], [80., 50.]];

fn choice(path: &Path) -> (ExtrudeEditSource, ExtrudeChoice) {
    let d = Document::open_read_only(path).expect("snapshot");
    let source = ExtrudeEditSource::read(&d).expect("catalogue");
    let base = source
        .features
        .iter()
        .find(|f| f.cut_history.is_some())
        .expect("base")
        .clone();
    (source, base)
}
fn command(source: &Path, dest: &Path, feature: ObjectId, version: &str, height: &str) -> Command {
    let mut c = cli();
    c.arg(EDIT)
        .arg(source)
        .arg("--feature")
        .arg(feature.to_string())
        .arg("--expect-version")
        .arg(version)
        .arg("--distance-mm")
        .arg(height)
        .arg("-o")
        .arg(dest)
        .arg("--json");
    c
}
fn ask(source: &Path, dest: &Path, height: f64, code: i32) -> Value {
    let (reading, base) = choice(source);
    reply(
        command(
            source,
            dest,
            base.feature,
            &reading.version.content.to_string(),
            &height.to_string(),
        )
        .output()
        .expect("CLI"),
        EDIT,
        code,
    )
}
fn only_height(source: &Path, copy: &Path, feature: ObjectId, changed: bool) -> Vec<TopologyRef> {
    use rusqlite::types::Value as Sql;
    let a = tables(source);
    let b = tables(copy);
    let old = Document::open_read_only(source)
        .expect("doc")
        .topology_refs()
        .expect("refs");
    let now = Document::open_read_only(copy)
        .expect("doc")
        .topology_refs()
        .expect("refs");
    let added: Vec<_> = now
        .iter()
        .filter(|r| !old.iter().any(|s| s.id == r.id))
        .cloned()
        .collect();
    assert!(
        old.iter().all(|r| now.contains(r)),
        "old meanings remain under the same UUIDs"
    );
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    let mut payload_changed = false;
    for (table, (cols, rows)) in a {
        let (newcols, newrows) = &b[&table];
        assert_eq!(&cols, newcols);
        if table == "topology_refs" {
            assert_eq!(newrows.len(), rows.len() + added.len());
            assert!(
                rows.iter().all(|r| newrows.contains(r)),
                "every original ref SQL cell/rowid"
            );
            continue;
        }
        assert_eq!(rows.len(), newrows.len());
        for (old, new) in rows.iter().zip(newrows) {
            for (i, (x, y)) in old.iter().zip(new).enumerate() {
                if x == y {
                    continue;
                }
                assert!(
                    (table == "meta" && cols[i] == "modified_at")
                        || (table == "objects"
                            && old[cols.iter().position(|c| c == "id").expect("id")]
                                == Sql::Blob(feature.to_bytes().to_vec())
                            && ["payload", "payload_hash"].contains(&cols[i].as_str())),
                    "unexpected SQL {table}.{}",
                    cols[i]
                );
                payload_changed |= table == "objects" && cols[i] == "payload";
            }
        }
    }
    assert_eq!(
        payload_changed, changed,
        "height payload must reflect the request"
    );
    added
}
fn check_added(source: &Path, copy: &Path, height: f64, changed: bool) {
    let (_, base) = choice(source);
    let h = base.cut_history.as_ref().expect("history");
    let mut added = only_height(source, copy, base.feature, changed);
    for (i, t) in h.tools.iter().enumerate() {
        if t.leaves_a_floor || t.depth_mm >= height {
            continue;
        }
        for descendant in &h.tools[i..] {
            let role = if descendant.feature == t.feature {
                SemanticRole::ExtrudeCap { side: CapSide::End }
            } else {
                SemanticRole::OriginCap {
                    origin_feature: t.feature,
                    side: CapSide::End,
                }
            };
            let at = added
                .iter()
                .position(|r| {
                    r.owner == descendant.feature
                        && r.producer_feature == descendant.feature
                        && r.output_role == role
                        && r.expected_kind == ferritecad_document::EntityKind::Face
                        && r.selection == ferritecad_document::SelectionRule::Exact
                        && r.fallback_signature.is_none()
                })
                .expect("every new own/descendant floor has exactly its required name");
            added.remove(at);
        }
    }
    assert!(added.is_empty(), "no other new refs");
    let (after, now) = choice(copy);
    assert_eq!(now.distance_mm, Some(height));
    assert!(now.refusal.is_none());
    assert!(
        after
            .sketches
            .iter()
            .any(|s| s.sketch == h.profile && s.refusal.is_none())
    );
    assert_eq!(
        after
            .cut_features
            .iter()
            .filter(|f| f.saved.is_some())
            .count(),
        h.tools.len()
    );
    assert_eq!(
        after.cut_bodies.iter().any(|b| b.refusal.is_none()),
        h.tools.len() < 16
    );
    let now = now.cut_history.expect("history");
    for (old, new) in h.tools.iter().zip(&now.tools) {
        let mut old = old.clone();
        old.leaves_a_floor = old.depth_mm < height;
        assert_eq!(&old, new, "absolute tool data/UUIDs unchanged");
    }
    // The advertised catalogue is actionable for the next edit, not just labels.
    let d = Document::open_read_only(copy).expect("reopen");
    prepare_extrude_height(&d, base.feature, height + 1.).expect("next height");
    let sketch = after
        .sketches
        .iter()
        .find(|s| s.sketch == h.profile)
        .expect("base sketch");
    ferritecad_document::replace_sketch_coordinates(
        &d,
        h.profile,
        sketch.vertices.as_ref().expect("vertices"),
    )
    .expect("next Sketch edit");
    let t = &now.tools[0];
    ferritecad_document::prepare_cut_parameters(
        &d,
        t.feature,
        &ferritecad_document::CircularCutEdit {
            tool_curve: t.tool_curve,
            center_mm: t.center_mm,
            radius_mm: t.radius_mm,
            depth_mm: t.depth_mm,
        },
    )
    .expect("next Cut edit");
}

#[test]
fn height_discovery_and_structural_refusals_without_kernel() {
    for n in [1, 2, 4, 16] {
        let root = tempfile::tempdir().expect("root");
        let source = history::fixture(root.path(), &history::tools(n));
        let (reading, base) = choice(&source);
        let h = base.cut_history.as_ref().expect("history");
        assert_eq!(h.tools.len(), n);
        assert!(base.refusal.is_none());
        let json = inspect(&source);
        let row = json["features"]
            .as_array()
            .expect("features")
            .iter()
            .find(|f| f["feature_id"] == base.feature.to_string())
            .expect("base");
        assert_eq!(
            row["base_height_edit"]["tools"]
                .as_array()
                .expect("tools")
                .len(),
            n
        );
        let d = Document::open(&source).expect("doc");
        for bad in [0., -1., f64::NAN, f64::INFINITY, 1e6 + 1.] {
            assert!(prepare_extrude_height(&d, base.feature, bad).is_err());
        }
        let error = prepare_extrude_height(&d, base.feature, 10.).expect_err("absolute depth");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
        assert!(error.to_string().contains(&h.tools[0].feature.to_string()));
        assert!(prepare_extrude_height(&d, ObjectId::new(), 14.).is_err());
        assert!(prepare_extrude_height(&d, h.tools[0].feature, 14.).is_err());
        d.close().expect("close");
        if !ferritecad_occt::is_available() {
            let bytes = std::fs::read(&source).expect("source");
            let never = root.path().join("never.fcad");
            let error = reply(
                command(
                    &source,
                    &never,
                    base.feature,
                    &reading.version.content.to_string(),
                    "14",
                )
                .output()
                .expect("CLI"),
                EDIT,
                2,
            );
            assert!(
                error["error"]["message"]
                    .as_str()
                    .expect("message")
                    .contains("Open CASCADE"),
                "kernel-first refusal, not a late guard"
            );
            assert!(!never.exists());
            assert_eq!(std::fs::read(&source).expect("bytes"), bytes);
        }
        for mode in 0..5 {
            let bad = root.path().join(format!("invalid-{mode}.fcad"));
            std::fs::copy(&source, &bad).expect("copy");
            let sql = rusqlite::Connection::open(&bad).expect("SQL");
            match mode {
                0 => {
                    sql.execute(
                        "DELETE FROM deps WHERE role='predecessor' AND dependent_id=?1",
                        [h.tools[n - 1].feature.to_bytes().to_vec()],
                    )
                    .expect("edge");
                }
                1 => {
                    sql.execute(
                        "DELETE FROM topology_refs WHERE id=?1",
                        [h.protected_floors
                            .iter()
                            .flat_map(|p| &p.references)
                            .next()
                            .copied()
                            .unwrap_or_else(|| {
                                Document::open_read_only(&source)
                                    .expect("doc")
                                    .topology_refs()
                                    .expect("refs")
                                    .into_iter()
                                    .find(|r| r.producer_feature == h.tools[0].feature)
                                    .expect("cut ref")
                                    .id
                            })
                            .to_bytes()
                            .to_vec()],
                    )
                    .expect("ref");
                }
                2 => {
                    sql.execute("INSERT INTO deps(dependent_id,dependency_id,role) VALUES(?1,?2,'predecessor')",rusqlite::params![base.feature.to_bytes().as_slice(),h.tools[n-1].feature.to_bytes().as_slice()]).expect("cycle edge");
                }
                3 => {
                    sql.execute(
                        "UPDATE objects SET parent_id=?1 WHERE id=?2",
                        rusqlite::params![
                            h.body.to_bytes().as_slice(),
                            h.profile.to_bytes().as_slice()
                        ],
                    )
                    .expect("unsupported parent");
                }
                4 => {
                    // Even erased boolean payload/edge signals leave saved
                    // carried names: this is not a free standalone history.
                    let d = Document::open_read_only(&bad).expect("doc");
                    let objects = d.objects().expect("objects");
                    d.close()
                        .expect("close pinned read before SQL fixture changes");
                    for mut object in objects {
                        let ObjectPayload::Extrude(e) = &mut object.payload else {
                            continue;
                        };
                        e.operation = ferritecad_document::SolidOperation::NewBody;
                        e.previous = None;
                        e.target_body = None;
                        let bytes = object.payload.to_storage_bytes().expect("payload");
                        sql.execute(
                            "UPDATE objects SET payload=?1,payload_hash=?2,schema_version=?4 WHERE id=?3",
                            rusqlite::params![
                                &bytes,
                                ferritecad_types::ContentHash::of_bytes(&bytes)
                                    .as_bytes()
                                    .as_slice(),
                                object.id.to_bytes().as_slice(),
                                object.payload.schema_version()
                            ],
                        )
                        .expect("erase boolean payload");
                    }
                    sql.execute(
                        "DELETE FROM deps WHERE role IN ('predecessor','target_body')",
                        [],
                    )
                    .expect("erase history edges");
                }
                _ => unreachable!(),
            }
            drop(sql);
            let d = Document::open(&bad).expect("damaged doc");
            assert!(
                prepare_extrude_height(&d, base.feature, 14.).is_err(),
                "invalid history cannot fall back to standalone {mode}"
            );
            let seen = ExtrudeEditSource::read(&d).expect("catalogue");
            assert!(
                seen.features
                    .iter()
                    .find(|f| f.feature == base.feature)
                    .expect("base")
                    .refusal
                    .is_some()
            );
        }
    }
}

#[test]
fn native_height_transitions_refs_sql_cache_and_mesh() {
    if !native() {
        return;
    }
    use CacheOutcome::{Hit, Miss};
    let mut artifact = 0;
    for (n, pockets) in [
        (1, false),
        (2, false),
        (4, false),
        (16, false),
        (4, true),
        (16, true),
    ] {
        let root = tempfile::tempdir().expect("root");
        let mut tools = history::tools(n);
        if pockets {
            for (i, t) in tools.iter_mut().enumerate() {
                t.depth_mm = 3. + (i % 7) as f64;
            }
        }
        let source = base_sketch::measured_fixture(root.path(), &tools);
        let original = std::fs::read(&source).expect("bytes");
        let (_, base) = choice(&source);
        for height in if pockets {
            vec![10., 14.]
        } else if n == 1 {
            vec![12., 14.]
        } else {
            vec![14.]
        } {
            let copy = root.path().join(format!("height-{height}.fcad"));
            ask(&source, &copy, height, 0);
            check_added(&source, &copy, height, height != 12.);
            let cold = measure_history_at(&copy, &tools, BOUNDS, height, None);
            assert_eq!(
                cold,
                measure_history_at(&copy, &tools, BOUNDS, height, Some(&vec![Miss; n + 1]))
            );
            assert_eq!(
                cold,
                measure_history_at(&copy, &tools, BOUNDS, height, Some(&vec![Hit; n + 1]))
            );
            let mesh = mesh(&copy, &root.path().join(format!("height-{height}.stl")));
            check_history_mesh_at(&mesh, &tools, BOUNDS, height);
            assert_eq!(std::fs::read(&source).expect("source"), original);
            if n == 4 && artifact < 3 {
                if let Some(dir) = std::env::var_os("FCAD_CUT_HEIGHT_ARTIFACTS") {
                    let dir = Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("artifacts");
                    let fbx = dir.join(format!("height-{artifact}.fbx"));
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
        }
        // An existing sidecar belongs to this same document, then the base changes.
        measure_history_at(&source, &tools, BOUNDS, 12., Some(&vec![Miss; n + 1]));
        measure_history_at(&source, &tools, BOUNDS, 12., Some(&vec![Hit; n + 1]));
        let mut d = Document::open(&source).expect("doc");
        let p = prepare_extrude_height(&d, base.feature, 14.).expect("height");
        d.write_extrude_height(&p).expect("write");
        d.close().expect("close");
        let changed = measure_history_at(&source, &tools, BOUNDS, 14., Some(&vec![Miss; n + 1]));
        assert_eq!(
            changed,
            measure_history_at(&source, &tools, BOUNDS, 14., Some(&vec![Hit; n + 1]))
        );
        assert_eq!(
            changed,
            measure_history_at(&source, &tools, BOUNDS, 14., None)
        );
        let next = root.path().join("next.fcad");
        ask(&source, &next, 15., 0);
        check_added(&source, &next, 15., true);
    }
    assert_eq!(artifact, 3);
}

#[test]
fn native_height_protected_floors_distant_depth_and_late_guards() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut tools = history::tools(16);
    for (i, t) in tools.iter_mut().enumerate() {
        t.depth_mm = 3. + (i % 7) as f64;
    }
    tools[9].depth_mm = 12.;
    let source = history::fixture(root.path(), &tools);
    let original = std::fs::read(&source).expect("source");
    let (reading, base) = choice(&source);
    let h = base.cut_history.as_ref().expect("history");
    let never = root.path().join("never.fcad");
    let error = ask(&source, &never, 11., 2);
    assert!(
        error["error"]["message"]
            .as_str()
            .expect("message")
            .contains(&h.tools[9].feature.to_string()),
        "distant depth guard"
    );
    for value in ["NaN", "inf", "1e999", "0", "-1", "1000001"] {
        reply(
            command(
                &source,
                &never,
                base.feature,
                &reading.version.content.to_string(),
                value,
            )
            .output()
            .expect("invalid value"),
            EDIT,
            2,
        );
        assert!(!never.exists());
    }
    let grown = root.path().join("grown.fcad");
    ask(&source, &grown, 14., 0);
    let (_, after) = choice(&grown);
    let protected = &after
        .cut_history
        .as_ref()
        .expect("context")
        .protected_floors[9];
    let refused = ask(&grown, &never, 12., 2);
    let message = refused["error"]["message"].as_str().expect("message");
    assert!(message.contains(&protected.feature.to_string()));
    for id in &protected.references {
        assert!(
            message.contains(&id.to_string()),
            "protected own/descendant floor {id}"
        );
    }
    assert!(!never.exists());
    for dest in [&source, &never] {
        if *dest == never {
            std::fs::write(&never, b"occupied").expect("occupied");
        }
        ask(&source, dest, 14., 2);
    }
    assert_eq!(std::fs::read(&never).expect("occupied"), b"occupied");
    let alias = root.path().join("alias.fcad");
    std::fs::hard_link(&source, &alias).expect("hardlink");
    ask(&source, &alias, 14., 2);
    #[cfg(unix)]
    {
        let link = root.path().join("link.fcad");
        std::os::unix::fs::symlink(&source, &link).expect("symlink");
        ask(&source, &link, 14., 2);
    }
    let clean = root.path().join("no-publication.fcad");
    reply(
        command(&source, &clean, base.feature, &"f".repeat(64), "14")
            .output()
            .expect("stale"),
        EDIT,
        2,
    );
    reply(
        command(
            &source,
            &clean,
            ObjectId::new(),
            &reading.version.content.to_string(),
            "14",
        )
        .output()
        .expect("foreign"),
        EDIT,
        2,
    );
    assert_eq!(std::fs::read(&source).expect("source"), original);
    // Closed stdout is a publication, never a retry instruction.
    let delivered = root.path().join("lost-report.fcad");
    let mut child = command(
        &source,
        &delivered,
        base.feature,
        &reading.version.content.to_string(),
        "14",
    )
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .expect("child");
    drop(child.stdout.take());
    assert_eq!(child.wait().expect("exit").code(), Some(7));
    check_added(&source, &delivered, 14., true);
    measure_history_at(&delivered, &tools, BOUNDS, 14., None);
    let request = ferritecad_jobs::EditExtrudeRequest {
        source: source.clone(),
        expected: reading.version,
        feature: base.feature,
        distance_mm: 14.,
        destination: clean.clone(),
    };
    edits::copy_late_guards(root.path(), &source, &original, &clean, |k, c| {
        ferritecad_jobs::edit_extrude_copy(&request, k, c).map(|_| ())
    });
}

#[cfg(not(feature = "planegcs"))]
#[test]
fn occt_without_solver_edits_cut_base_height() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let tools = history::tools(4);
    let source = history::fixture(root.path(), &tools);
    let copy = root.path().join("mixed.fcad");
    ask(&source, &copy, 14., 0);
    check_added(&source, &copy, 14., true);
    measure_history_at(&copy, &tools, BOUNDS, 14., None);
}
