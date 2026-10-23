// SPDX-License-Identifier: MIT
//! §26F: move the plate's boundaries while every absolute tool stays put.
use super::*;
use ferritecad_document::{ExtrudeEditSource, SketchVertex};
use ferritecad_types::{ObjectId, StableEntityId};

const EDIT: &str = "edit";
struct BaseFacts {
    sketch: ObjectId,
    vertices: Option<Vec<SketchVertex>>,
    cut_history: Option<ferritecad_document::SavedCircularCut>,
}
const EXPANDED: [[f64; 2]; 2] = [[-4., -3.], [86., 56.]];

/// Name all six native base faces as well as all historical carried/origin faces.
/// The two-Cut case stores the opposite winding, without changing curve identity.
pub(super) fn measured_fixture(root: &Path, tools: &[CircularCut]) -> PathBuf {
    let path = history::fixture(root, tools);
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
    path
}

fn choice(path: &Path) -> (ExtrudeEditSource, BaseFacts) {
    let d = Document::open_read_only(path).expect("snapshot");
    let source = ExtrudeEditSource::read(&d).expect("structural readers");
    let context = history::ordered(path).remove(0);
    let ObjectPayload::Sketch(stored) = d
        .object(context.profile_sketch)
        .expect("object")
        .expect("sketch")
        .payload
    else {
        panic!("sketch")
    };
    let vertices = stored
        .curves
        .iter()
        .map(|c| {
            let SketchGeometry::Line { start, .. } = c.geometry else {
                panic!("line")
            };
            SketchVertex {
                curve_id: c.id,
                start_mm: [start.x, start.y],
            }
        })
        .collect();
    let selected = BaseFacts {
        sketch: context.profile_sketch,
        vertices: Some(vertices),
        cut_history: Some(context),
    };
    (source, selected)
}

fn vertices(choice: &BaseFacts, bounds: [[f64; 2]; 2]) -> Vec<SketchVertex> {
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
    let changes: Vec<_> = vertices
        .iter()
        .enumerate()
        .map(|(i, v)| ferritecad_document::EditChange::SetCurveGeometry {
            sketch,
            curve: v.curve_id,
            geometry: SketchGeometry::Line {
                start: ferritecad_document::Point2 {
                    x: v.start_mm[0],
                    y: v.start_mm[1],
                },
                end: {
                    let p = vertices[(i + 1) % vertices.len()].start_mm;
                    ferritecad_document::Point2 { x: p[0], y: p[1] }
                },
            },
        })
        .collect();
    write(&request, &ferritecad_fixtures::edit_request_json(&changes));
    let mut c = cli();
    c.arg(EDIT)
        .arg(source)
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
            let context = |c: &BaseFacts| {
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
        for gap in [0., 0.5 * ferritecad_document::WALL_CLEARANCE_MM] {
            let v = vertices(
                &base,
                [[4. - tools[selected].radius_mm - gap, 0.], [80., 50.]],
            );
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
    for case in [2, 3, 4, 6, 7] {
        let mut v = good.clone();
        let mut id = base.sketch;
        match case {
            2 => v[1].curve_id = StableEntityId::new(),
            // §26I accepts a sloped base; crossing edges are still no part.
            3 => {
                let [a, b] = [v[1].start_mm, v[2].start_mm];
                v[1].start_mm = b;
                v[2].start_mm = a;
            }
            4 => v[1].start_mm = v[0].start_mm,
            6 => id = base.cut_history.as_ref().expect("context").tools[0].tool_sketch,
            7 => id = ObjectId::new(),
            _ => unreachable!("listed invalid case"),
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
fn native_built_cut_base_can_leave_the_structural_readers_domain() {
    if native() {
        boundary::run();
    }
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
    let request = ferritecad_jobs::EditRequest {
        changes: {
            let document =
                ferritecad_document::Document::open_read_only(source.clone()).expect("source");
            ferritecad_fixtures::vertex_changes(&document, base.sketch, &(proposed.clone()))
                .expect("changes")
        },
        source: source.clone(),
        expected: catalog.version,
        destination: never.clone(),
    };
    edits::copy_late_guards(
        root.path(),
        &source,
        &original,
        &never,
        |kernel, context| ferritecad_jobs::edit_copy(&request, kernel, context).map(|_| ()),
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

#[path = "cut_transaction_boundary.rs"]
mod boundary;
