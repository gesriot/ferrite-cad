// SPDX-License-Identifier: MIT
//! §26F: move the plate's boundaries while every absolute tool stays put.
use super::*;
use ferritecad_document::{ExtrudeEditSource, SketchChoice, SketchVertex};
use ferritecad_types::{ObjectId, StableEntityId};

const EDIT: &str = "edit-sketch-copy";
const EXPANDED: [[f64; 2]; 2] = [[-4., -3.], [86., 56.]];

/// Name all six native base faces as well as all historical carried/origin faces.
/// The two-Cut case stores the opposite winding, without changing curve identity.
pub(super) fn measured_fixture(root: &Path, tools: &[CircularCut]) -> PathBuf {
    let path = history::fixture(root, tools);
    let caps = tables(&path)["capabilities"].clone();
    let mut d = Document::open(&path).expect("doc");
    let (_, selected) = choice(&path);
    let context = selected.cut_history.as_ref().expect("context");
    let mut record = d.object(selected.sketch).expect("row").expect("base");
    let ObjectPayload::Sketch(sketch) = &mut record.payload else {
        panic!("Sketch");
    };
    if tools.len() == 2 {
        sketch.curves.reverse();
        for curve in &mut sketch.curves {
            let SketchGeometry::Line { start, end } = &mut curve.geometry else {
                panic!("Line")
            };
            std::mem::swap(start, end);
        }
    }
    let refs = d.topology_refs().expect("refs");
    let missing: Vec<_> = sketch
        .curves
        .iter()
        .filter(|c| {
            !refs.iter().any(|r| {
                r.producer_feature == context.base_feature
                    && r.output_role
                        == SemanticRole::ExtrudeSide {
                            profile_segment: c.id,
                        }
            })
        })
        .map(|c| ferritecad_document::TopologyRef {
            id: StableEntityId::new(),
            owner: context.base_feature,
            producer_feature: context.base_feature,
            expected_kind: ferritecad_document::EntityKind::Face,
            output_role: SemanticRole::ExtrudeSide {
                profile_segment: c.id,
            },
            selection: ferritecad_document::SelectionRule::AllDerivedFrom { ancestor: c.id },
            fallback_signature: None,
        })
        .collect();
    d.write(|w| {
        if tools.len() == 2 {
            w.put_object(
                record.id,
                record.parent,
                record.ordinal,
                record.name.as_deref(),
                &record.payload,
            )?;
        }
        for r in &missing {
            w.put_topology_ref(r)?;
        }
        Ok(())
    })
    .expect("fixture names");
    assert_eq!(
        d.topology_refs()
            .expect("refs")
            .iter()
            .filter(|r| r.producer_feature == context.base_feature)
            .count(),
        6
    );
    d.close().expect("close");
    // Retain the scrambled capability rows/optional extension of this fixture.
    let sql = rusqlite::Connection::open(&path).expect("SQL");
    sql.execute("DELETE FROM capabilities", [])
        .expect("fixture capabilities");
    for row in caps.1 {
        sql.execute(
            "INSERT INTO capabilities(rowid,name,required) VALUES (?1,?2,?3)",
            rusqlite::params_from_iter(row),
        )
        .expect("restore fixture capability row");
    }
    path
}

fn choice(path: &Path) -> (ExtrudeEditSource, SketchChoice) {
    let d = Document::open_read_only(path).expect("snapshot");
    let source = ExtrudeEditSource::read(&d).expect("catalogue");
    let selected = source
        .sketches
        .iter()
        .find(|s| s.cut_history.is_some())
        .expect("base choice")
        .clone();
    (source, selected)
}

fn vertices(choice: &SketchChoice, bounds: [[f64; 2]; 2]) -> Vec<SketchVertex> {
    choice
        .vertices
        .as_ref()
        .expect("vertices")
        .iter()
        .map(|v| SketchVertex {
            curve_id: v.curve_id,
            start_mm: std::array::from_fn(|axis| bounds[usize::from(v.start_mm[axis] != 0.)][axis]),
        })
        .collect()
}

fn command(
    source: &Path,
    dest: &Path,
    sketch: ObjectId,
    version: &str,
    vertices: &[SketchVertex],
) -> Command {
    let request = source.with_extension("base-request.json");
    write(
        &request,
        &json!({"request_version":1,"vertices": vertices.iter().map(|v|
        json!({"curve_id":v.curve_id,"start_mm":v.start_mm})).collect::<Vec<_>>()}),
    );
    let mut c = cli();
    c.arg(EDIT)
        .arg(source)
        .arg("--sketch")
        .arg(sketch.to_string())
        .arg("--expect-version")
        .arg(version)
        .arg("--request")
        .arg(request)
        .arg("-o")
        .arg(dest)
        .arg("--json");
    c
}

fn only_base(source: &Path, copy: &Path, sketch: ObjectId) {
    use rusqlite::types::Value as Sql;
    let a = tables(source);
    let b = tables(copy);
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    let mut payload_changed = false;
    for (table, (cols, rows)) in a {
        let (newcols, newrows) = &b[&table];
        assert_eq!(&cols, newcols);
        assert_eq!(rows.len(), newrows.len());
        for (old, new) in rows.iter().zip(newrows) {
            for (i, (x, y)) in old.iter().zip(new).enumerate() {
                if x == y {
                    continue;
                }
                let allowed = (table == "meta" && cols[i] == "modified_at")
                    || (table == "objects"
                        && old[cols.iter().position(|c| c == "id").expect("id")]
                            == Sql::Blob(sketch.to_bytes().to_vec())
                        && ["payload", "payload_hash"].contains(&cols[i].as_str()));
                assert!(allowed, "unexpected SQL cell {table}.{}", cols[i]);
                payload_changed |= table == "objects" && cols[i] == "payload";
            }
        }
    }
    assert!(payload_changed, "writer lost the base coordinate change");
}

#[test]
fn coordinate_refusals_preserve_their_error_kind_and_cause() {
    use std::error::Error;
    let root = tempfile::tempdir().expect("root");
    let path = history::fixture(root.path(), &[]);
    let d = Document::open(&path).expect("document");
    let source = ExtrudeEditSource::read(&d).expect("catalogue");
    let base = source
        .sketches
        .iter()
        .find(|s| s.refusal.is_none())
        .expect("base");
    let vertices = base.vertices.as_ref().expect("vertices");
    let (body, _) = tip(&d);
    let error = ferritecad_document::replace_sketch_coordinates(&d, body, vertices)
        .expect_err("a Body is not a Sketch");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Unsupported);
    let refusal = error.to_string();

    // A failed dependency read must remain an I/O error with its SQLite cause,
    // rather than becoming an unsupported-model refusal through discovery text.
    let sql = rusqlite::Connection::open(&path).expect("SQL");
    sql.execute_batch("ALTER TABLE deps RENAME TO unreadable_deps")
        .expect("unreadable dependency fixture");
    drop(sql);
    let expected = d.dependencies().expect_err("missing dependency table");
    let error = ferritecad_document::replace_sketch_coordinates(&d, base.sketch, vertices)
        .expect_err("dependency read failure");
    assert_eq!(expected.kind(), ferritecad_types::ErrorKind::Io);
    assert_eq!(error.kind(), expected.kind());
    assert_eq!(
        error.source().expect("retained cause").to_string(),
        expected.source().expect("SQLite cause").to_string()
    );
    assert_eq!(refusal, "unsupported: selected object is not a Sketch");
}

#[test]
fn base_discovery_and_writer_forgery_without_kernel() {
    for n in [1, 2, 4, 16] {
        let root = tempfile::tempdir().expect("root");
        let path = history::fixture(root.path(), &history::tools(n));
        let before = std::fs::read(&path).expect("bytes");
        let (catalog, base) = choice(&path);
        assert_eq!(
            catalog
                .sketches
                .iter()
                .filter(|s| s.refusal.is_none())
                .count(),
            1
        );
        let context = base.cut_history.as_ref().expect("history");
        assert_eq!(context.tools.len(), n);
        assert!(catalog.circle_sketches.iter().all(|s| s.refusal.is_some()));
        assert!(catalog.annulus_sketches.iter().all(|s| s.refusal.is_some()));
        assert!(
            catalog
                .constraint_sketches
                .iter()
                .all(|s| s.refusal.is_some())
        );
        let wire = inspect(&path);
        let row = wire["sketches"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|s| s["sketch_id"] == json!(base.sketch))
            .expect("base");
        assert_eq!(row["editable"], true);
        assert_eq!(
            row["cut_history"]["tools"].as_array().expect("tools").len(),
            n
        );
        assert_eq!(
            row["cut_history"]["base_feature_id"],
            json!(context.base_feature)
        );
        let proposed = vertices(&base, EXPANDED);
        let mut d = Document::open(&path).expect("doc");
        let prepared = ferritecad_document::replace_sketch_coordinates(&d, base.sketch, &proposed)
            .expect("prepare");
        for kind in 0..6 {
            let mut fake = prepared.clone();
            let ObjectPayload::Sketch(s) = &mut fake.payload else {
                panic!("Sketch");
            };
            match kind {
                0 => s.plane = ObjectId::new(),
                1 => s.curves[0].construction = true,
                2 => s.curves[0].id = StableEntityId::new(),
                3 => {
                    let SketchGeometry::Line { ref mut end, .. } = s.curves[0].geometry else {
                        panic!("Line");
                    };
                    end.x += 1.;
                }
                4 => s.curves.swap(0, 1),
                _ => fake.name = Some("forged".into()),
            }
            d.write_sketch_geometry(&fake)
                .expect_err("forgery must refuse");
            assert_eq!(std::fs::read(&path).expect("bytes"), before);
        }
        // The current history, not prepared metadata, is authoritative inside write.
        let far = context.tools.last().expect("last");
        let mut collision = prepared.clone();
        let bad = vertices(&base, [[far.center_mm[0] - far.radius_mm, -3.], [86., 56.]]);
        let ObjectPayload::Sketch(s) = &mut collision.payload else {
            panic!("Sketch");
        };
        for (i, c) in s.curves.iter_mut().enumerate() {
            c.geometry = SketchGeometry::Line {
                start: ferritecad_document::Point2::new(bad[i].start_mm[0], bad[i].start_mm[1])
                    .expect("point"),
                end: ferritecad_document::Point2::new(
                    bad[(i + 1) % 4].start_mm[0],
                    bad[(i + 1) % 4].start_mm[1],
                )
                .expect("point"),
            };
        }
        d.write_sketch_geometry(&collision)
            .expect_err("numeric forgery");
        assert_eq!(std::fs::read(&path).expect("bytes"), before);
        d.write_sketch_geometry(&prepared)
            .expect("write valid coordinates");
        let after = ExtrudeEditSource::read(&d).expect("new catalogue");
        assert_eq!(
            after
                .sketches
                .iter()
                .find(|s| s.sketch == base.sketch)
                .expect("base")
                .vertices
                .as_ref(),
            Some(&proposed)
        );
    }
}

#[test]
fn native_base_bounds_refs_sql_cache_and_mesh() {
    if !native() {
        return;
    }
    use CacheOutcome::{Hit, Miss};
    for n in [1, 2, 4, 16] {
        let root = tempfile::tempdir().expect("root");
        let tools = history::tools(n);
        let path = measured_fixture(root.path(), &tools);
        let before = std::fs::read(&path).expect("source bytes");
        let (catalog, base) = choice(&path);
        measure_history(&path, &tools, Some(&vec![Miss; n + 1]));
        measure_history(&path, &tools, Some(&vec![Hit; n + 1]));
        for (case, bounds) in [EXPANDED, [[2., 2.], [73., 48.]], [[3., 1.], [83., 51.]]]
            .into_iter()
            .enumerate()
        {
            let dest = root.path().join(format!("base-{n}-{case}.fcad"));
            reply(
                command(
                    &path,
                    &dest,
                    base.sketch,
                    &catalog.version.content.to_string(),
                    &vertices(&base, bounds),
                )
                .output()
                .expect("CLI"),
                EDIT,
                0,
            );
            only_base(&path, &dest, base.sketch);
            let cold = measure_history_in(&dest, &tools, bounds, None);
            std::fs::copy(
                path.with_extension("fcad-cache"),
                dest.with_extension("fcad-cache"),
            )
            .expect("old sidecar");
            assert_eq!(
                measure_history_in(&dest, &tools, bounds, Some(&vec![Miss; n + 1])),
                cold
            );
            assert_eq!(
                measure_history_in(&dest, &tools, bounds, Some(&vec![Hit; n + 1])),
                cold
            );
            check_history_mesh_in(&mesh(&dest, &dest.with_extension("stl")), &tools, bounds);
            let (_, edited) = choice(&dest);
            assert_eq!(edited.vertices, Some(vertices(&base, bounds)));
            let context = |c: &ferritecad_document::SketchChoice| {
                let h = c.cut_history.as_ref().expect("context");
                (h.body, h.base_feature, h.tools.clone())
            };
            assert_eq!(
                context(&edited),
                context(&base),
                "absolute tools did not move"
            );
            // The boundary is the edited Lines, under their saved UUIDs.
            let boundary = &edited.cut_history.as_ref().expect("context").boundary;
            assert_eq!(boundary.rectangle_mm(), Some(bounds));
            assert_eq!(std::fs::read(&path).expect("bytes"), before);
            if n == 4 {
                let fbx = dest.with_extension("fbx");
                reply(
                    cli()
                        .arg("export-fbx")
                        .arg(&dest)
                        .arg("-o")
                        .arg(&fbx)
                        .arg("--json")
                        .output()
                        .expect("export"),
                    "export-fbx",
                    0,
                );
                if let Some(dir) = std::env::var_os("FCAD_CUT_BASE_ARTIFACTS") {
                    std::fs::create_dir_all(&dir).expect("artifacts");
                    std::fs::copy(fbx, Path::new(&dir).join(format!("base-{case}.fbx")))
                        .expect("keep");
                }
            }
        }
    }
}

#[test]
fn native_base_refuses_one_bad_aspect_and_distant_walls() {
    if !native() {
        return;
    }
    for selected in [0, 8, 15] {
        let root = tempfile::tempdir().expect("root");
        let mut tools = history::tools(16);
        tools[selected].center_mm[0] = 4.;
        let path = history::fixture(root.path(), &tools);
        let (catalog, base) = choice(&path);
        let before = std::fs::read(&path).expect("bytes");
        let dest = root.path().join("never.fcad");
        let feature = base.cut_history.as_ref().expect("context").tools[selected].feature;
        for gap in [0., 0.5 * ferritecad_document::WALL_CLEARANCE_MM, -0.25] {
            let v = vertices(
                &base,
                [[4. - tools[selected].radius_mm - gap, 0.], [80., 50.]],
            );
            let draft = base.validate_coordinates(&v).expect_err("wall guard");
            assert!(draft.to_string().contains(&feature.to_string()), "{draft}");
            let error = reply(
                command(
                    &path,
                    &dest,
                    base.sketch,
                    &catalog.version.content.to_string(),
                    &v,
                )
                .output()
                .expect("CLI"),
                EDIT,
                2,
            );
            assert!(error.to_string().contains(&feature.to_string()), "{error}");
            assert!(!dest.exists());
        }
        assert_eq!(std::fs::read(&path).expect("bytes"), before);
    }
    let root = tempfile::tempdir().expect("root");
    let path = history::fixture(root.path(), &history::tools(4));
    let (catalog, base) = choice(&path);
    let before = std::fs::read(&path).expect("bytes");
    let dest = root.path().join("never.fcad");
    let good = vertices(&base, EXPANDED);
    for case in 0..9 {
        let mut v = good.clone();
        let mut id = base.sketch;
        match case {
            0 => {
                v.pop();
            }
            1 => v.swap(0, 1),
            2 => v[1].curve_id = StableEntityId::new(),
            // §26I accepts a sloped base; crossing edges are still no part.
            3 => {
                let [a, b] = [v[1].start_mm, v[2].start_mm];
                v[1].start_mm = b;
                v[2].start_mm = a;
            }
            4 => v[1].start_mm = v[0].start_mm,
            5 => {
                v[1].start_mm[0] = 1e7;
                v[2].start_mm[0] = 1e7;
            }
            6 => id = base.cut_history.as_ref().expect("context").tools[0].tool_sketch,
            7 => id = ObjectId::new(),
            _ => {
                let points: Vec<_> = v.iter().rev().map(|p| p.start_mm).collect();
                for (vertex, point) in v.iter_mut().zip(points) {
                    vertex.start_mm = point;
                }
            }
        }
        reply(
            command(&path, &dest, id, &catalog.version.content.to_string(), &v)
                .output()
                .expect("CLI"),
            EDIT,
            2,
        );
        assert!(!dest.exists());
        assert_eq!(std::fs::read(&path).expect("bytes"), before);
    }
    let mut nonfinite = good.clone();
    nonfinite[0].start_mm[0] = f64::NAN;
    base.validate_coordinates(&nonfinite)
        .expect_err("nonfinite draft");
    let mut c = command(
        &path,
        &dest,
        base.sketch,
        &catalog.version.content.to_string(),
        &good,
    );
    let request = path.with_extension("base-request.json");
    let raw = std::fs::read_to_string(&request)
        .expect("request")
        .replacen("-4.0", "1e999", 1);
    std::fs::write(request, raw).expect("nonfinite JSON number");
    reply(c.output().expect("CLI"), EDIT, 2);
    assert!(!dest.exists());
}

#[test]
#[cfg(not(feature = "planegcs"))]
fn occt_without_solver_edits_cut_base() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let tools = history::tools(4);
    let source = history::fixture(root.path(), &tools);
    let (catalog, base) = choice(&source);
    let dest = root.path().join("base.fcad");
    reply(
        command(
            &source,
            &dest,
            base.sketch,
            &catalog.version.content.to_string(),
            &vertices(&base, EXPANDED),
        )
        .output()
        .expect("CLI"),
        EDIT,
        0,
    );
    measure_history_in(&dest, &tools, EXPANDED, None);
}

#[test]
fn native_base_copy_late_guards_aliases_and_exit_seven() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let tools = history::tools(4);
    let source = history::fixture(root.path(), &tools);
    let (catalog, base) = choice(&source);
    let original = std::fs::read(&source).expect("bytes");
    let proposed = vertices(&base, EXPANDED);
    let version = catalog.version.content.to_string();
    let never = root.path().join("never.fcad");
    let delivered = root.path().join("lost-report.fcad");
    assert_eq!(
        command(&source, &delivered, base.sketch, &version, &proposed)
            .stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("closed reports")
            .code(),
        Some(7)
    );
    only_base(&source, &delivered, base.sketch);
    measure_history_in(&delivered, &tools, EXPANDED, None);
    for alias in [&source, &root.path().join("alias.fcad")] {
        if alias != &source {
            std::fs::hard_link(&source, alias).expect("alias");
        }
        reply(
            command(&source, alias, base.sketch, &version, &proposed)
                .output()
                .expect("CLI"),
            EDIT,
            2,
        );
    }
    #[cfg(unix)]
    {
        let link = root.path().join("link.fcad");
        std::os::unix::fs::symlink(&source, &link).expect("link");
        reply(
            command(&source, &link, base.sketch, &version, &proposed)
                .output()
                .expect("CLI"),
            EDIT,
            2,
        );
    }
    std::fs::write(&never, b"occupied").expect("sentinel");
    reply(
        command(&source, &never, base.sketch, &version, &proposed)
            .output()
            .expect("CLI"),
        EDIT,
        2,
    );
    assert_eq!(std::fs::read(&never).expect("bytes"), b"occupied");
    std::fs::remove_file(&never).expect("owned sentinel");
    assert_eq!(std::fs::read(&source).expect("bytes"), original);
    let request = ferritecad_jobs::EditSketchRequest {
        source: source.clone(),
        destination: never.clone(),
        expected: catalog.version,
        sketch: base.sketch,
        vertices: proposed.clone(),
    };
    edits::copy_late_guards(
        root.path(),
        &source,
        &original,
        &never,
        |kernel, context| ferritecad_jobs::edit_sketch_copy(&request, kernel, context).map(|_| ()),
    );
    let report = reply(
        command(&source, &never, base.sketch, &version, &proposed)
            .output()
            .expect("stale CLI"),
        EDIT,
        2,
    );
    assert!(report.to_string().contains("source has changed"));
    assert!(!never.exists());
}

#[test]
fn base_refuses_invalid_history_and_rechecks_current_tools_without_kernel() {
    for case in 0..5 {
        let root = tempfile::tempdir().expect("root");
        let path = history::fixture(root.path(), &history::tools(4));
        let (_, base) = choice(&path);
        let proposed = vertices(&base, EXPANDED);
        let context = base.cut_history.as_ref().expect("context");
        let mut d = Document::open(&path).expect("doc");
        match case {
            0 => {
                // A second Body claims an already owned history.
                d.write(|w| {
                    w.put_object(
                        ObjectId::new(),
                        None,
                        100,
                        Some("same"),
                        &ObjectPayload::Body(ferritecad_document::Body {
                            tip_feature: Some(context.base_feature),
                        }),
                    )
                })
                .expect("fixture");
            }
            1 => {
                let mut record = d.object(base.sketch).expect("row").expect("base");
                let ObjectPayload::Sketch(s) = &mut record.payload else {
                    panic!("Sketch")
                };
                let curve = s.curves[0].id;
                s.constraints.push(ferritecad_document::SketchConstraint {
                    id: StableEntityId::new(),
                    rule: ferritecad_document::SketchConstraintRule::Horizontal {
                        a: ferritecad_document::SketchPointRef::new(
                            curve,
                            ferritecad_document::SketchPointSelector::Start,
                        ),
                        b: ferritecad_document::SketchPointRef::new(
                            curve,
                            ferritecad_document::SketchPointSelector::End,
                        ),
                    },
                });
                d.write(|w| {
                    w.put_object(
                        record.id,
                        record.parent,
                        record.ordinal,
                        record.name.as_deref(),
                        &record.payload,
                    )
                })
                .expect("fixture");
            }
            2 | 3 => {
                let mut record = d
                    .object(context.tools[3].feature)
                    .expect("row")
                    .expect("Cut");
                let ObjectPayload::Extrude(e) = &mut record.payload else {
                    panic!("Cut")
                };
                if case == 2 {
                    e.previous = Some(record.id);
                } else {
                    e.previous = Some(context.base_feature);
                }
                d.write(|w| {
                    w.put_object(
                        record.id,
                        record.parent,
                        record.ordinal,
                        record.name.as_deref(),
                        &record.payload,
                    )
                })
                .expect("fixture");
            }
            _ => {
                d.close().expect("close");
                let sql = rusqlite::Connection::open(&path).expect("SQL");
                sql.execute(
                    "DELETE FROM deps WHERE role='predecessor' AND dependent_id=?1",
                    [context.tools[2].feature.to_bytes().as_slice()],
                )
                .expect("missing edge");
                drop(sql);
                d = Document::open(&path).expect("readable");
            }
        }
        let before = d.content_version().expect("version");
        let reading = ExtrudeEditSource::read(&d).expect("discovery");
        assert!(reading.sketches.iter().all(|s| s.refusal.is_some()));
        ferritecad_document::replace_sketch_coordinates(&d, base.sketch, &proposed)
            .expect_err("unsupported history");
        assert_eq!(d.content_version().expect("version"), before);
    }
    let root = tempfile::tempdir().expect("root");
    let path = history::fixture(root.path(), &history::tools(16));
    let (_, base) = choice(&path);
    let mut d = Document::open(&path).expect("doc");
    let proposed = vertices(&base, [[2., 2.], [73., 48.]]);
    let prepared = ferritecad_document::replace_sketch_coordinates(&d, base.sketch, &proposed)
        .expect("prepare before tool change");
    let far = &base.cut_history.as_ref().expect("context").tools[15];
    let changed = ferritecad_document::prepare_cut_parameters(
        &d,
        far.feature,
        &ferritecad_document::CircularCutEdit {
            tool_curve: far.tool_curve,
            center_mm: [76., far.center_mm[1]],
            radius_mm: far.radius_mm,
            extent: CutExtent::Blind {
                depth_mm: far.extent.blind_depth_mm().expect("blind"),
            },
            vocabulary: ExtentVocabulary::BlindOrThroughAll,
        },
    )
    .expect("move tool within original plate");
    d.write_cut_parameters(&changed)
        .expect("concurrent tool edit");
    let before = d.content_version().expect("version");
    let error = d
        .write_sketch_geometry(&prepared)
        .expect_err("must re-read current far disk");
    assert!(
        error.to_string().contains(&far.feature.to_string()),
        "{error}"
    );
    assert_eq!(d.content_version().expect("version"), before);
}
