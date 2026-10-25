// SPDX-License-Identifier: MIT
//! §26I: the same bounded Cut history in a simple Line polygon — a concave L,
//! sloped triangles and quadrilaterals, either winding, far from the origin.
//!
//! Containment is judged against the saved Lines, never their bounding box;
//! every rule runs in the catalogue, preparation and the writer's transaction.
use super::*;

use crate::cut_facts::CutObservation;
use ferritecad_types::{ObjectId, StableEntityId};

/// The acceptance L: 1600 mm², notch at the top right, reflex vertex (20, 20).
pub(super) const L: [[f64; 2]; 6] = [
    [0., 0.],
    [60., 0.],
    [60., 20.],
    [20., 20.],
    [20., 40.],
    [0., 40.],
];
const H: f64 = 10.;

/// Sixteen separated disks inside the L, in both branches and the corner
/// they share; slot 15 crosses y = 20 left of the reflex vertex, which is
/// inside the part although it is a side of the lower branch's box.
const SLOTS: [[f64; 2]; 16] = [
    [26.125, 5.625],
    [33.125, 5.625],
    [40.125, 5.625],
    [47.125, 5.625],
    [54.125, 5.625],
    [26.125, 13.625],
    [33.125, 13.625],
    [40.125, 13.625],
    [47.125, 13.625],
    [54.125, 13.625],
    [6.125, 25.625],
    [14.125, 25.625],
    [6.125, 33.625],
    [14.125, 33.625],
    [10.125, 9.625],
    [10.125, 18.625],
];

/// History order is not geometric order; ThroughAll, Blind-through and
/// pockets alternate. The first two links are in different branches.
pub(super) fn tools(n: usize) -> Vec<CutSpec> {
    (0..n)
        .map(|i| CutSpec {
            center_mm: SLOTS[(i * 11) % 16],
            radius_mm: 1.5 + (i % 4) as f64 * 0.25,
            extent: match i % 3 {
                0 => CutDepth::ThroughAll,
                1 => CutDepth::Blind { depth_mm: H },
                _ => CutDepth::Blind {
                    depth_mm: 3.5 + (i % 7) as f64,
                },
            },
        })
        .collect()
}

fn reach(tools: &[CutSpec], height: f64) -> f64 {
    tools
        .iter()
        .map(|t| PI * t.radius_mm.powi(2) * t.extent.reach_mm(height))
        .sum()
}

pub(super) fn ordered(path: &Path) -> Vec<CutObservation> {
    crate::cut_facts::observed(path)
}

/// The base Sketch's saved Lines, by identity and in stored order.
fn base_lines(path: &Path) -> Vec<(StableEntityId, [f64; 2], [f64; 2])> {
    let d = Document::open_read_only(path).expect("doc");
    let saved = &ordered(path)[0];
    let lines = d
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| o.id == saved.profile_sketch)
        .map(|o| match o.payload {
            ObjectPayload::Sketch(s) => s
                .curves
                .iter()
                .map(|c| match c.geometry {
                    SketchGeometry::Line { start, end } => {
                        (c.id, [start.x, start.y], [end.x, end.y])
                    }
                    _ => panic!("a Line"),
                })
                .collect(),
            _ => panic!("the base Sketch"),
        })
        .expect("base");
    d.close().expect("close");
    lines
}

/// The analytic volume of the L with its tools, and the mesh the CLI exports.
///
/// A part with no Cut yet is measured by its mesh only: its creation names
/// only some of its faces, and the name checks here are about Cut histories.
fn measured(path: &Path, tools: &[CutSpec], points: &[[f64; 2]], height: f64) -> f64 {
    let m = mesh(path, &path.with_extension("stl"));
    check_part_mesh_at(&m, tools, points, height);
    if tools.is_empty() {
        return m.volume;
    }
    let volume = measure_part_at(path, tools, points, height, None);
    let exact = polygon_area(points) * height - reach(tools, height);
    assert!((volume - exact).abs() < 1e-6, "{volume} != {exact}");
    volume
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

/// Coordinates for the base Sketch, from its `_v3` boundary: same UUIDs,
/// same order, only numbers.
fn sketch(source: &Path, destination: &Path, points: &[[f64; 2]], code: i32) -> Value {
    let catalog = inspect(source);
    let context = ordered(source).remove(0);
    let base = catalog["sketches"]
        .as_array()
        .expect("sketches")
        .iter()
        .find(|s| s["sketch_id"] == context.profile_sketch.to_string())
        .expect("base Sketch");
    let request = source.with_extension("sketch.json");
    write(&request, &transaction::line_changes(base, points));
    reply(
        transaction::command(
            source,
            catalog["content_version"].as_str().expect("v"),
            &request,
            destination,
        )
        .output()
        .expect("sketch"),
        "edit",
        code,
    )
}

/// A part made by the shipped creation command.
fn created(root: &Path, points: &[[f64; 2]], name: &str) -> PathBuf {
    let path = root.join(format!("{name}.fcad"));
    let request = root.join(format!("{name}.create.json"));
    write(
        &request,
        &json!({"request_version":1,"points_mm":points,"height_mm":H}),
    );
    reply(
        cli()
            .arg("create-sketch-extrude")
            .arg(&request)
            .arg("-o")
            .arg(&path)
            .arg("--json")
            .output()
            .expect("create"),
        "create-sketch-extrude",
        0,
    );
    path
}

/// The SQL allowlist of one Cut parameter edit: the tool Sketch and Cut
/// payload/hash (and the Cut's layout column when its end changes form) and
/// `meta.modified_at`. Every other cell, rowid and byte is kept.
fn only_the_cut(source: &Path, copy: &Path, saved: &CutObservation) {
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
                            || row(saved.feature) && columns[i] == "schema_version"
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
}

/// The L with 1, 2 and 16 Cuts: add, edit the first, a middle and the last
/// link, base height and base coordinates, and refusals on the notch, the
/// reflex vertex and a far wall — every result measured, meshed and read back.
#[test]
fn native_l_profile_history_matrix() {
    if !native() {
        return;
    }
    use CacheOutcome::{Hit, Miss};
    let mut artifact = 0;
    for n in [1, 2, 16] {
        let root = tempfile::tempdir().expect("root");
        let tools = tools(n);
        let mut source = created(root.path(), &L, "L");
        let plate = measured(&source, &[], &L, H);
        assert!((plate - 16000.).abs() < 1e-3, "{plate}");
        for (i, tool) in tools.iter().enumerate() {
            let next = root.path().join(format!("link-{}.fcad", i + 1));
            add(&source, &next, *tool, 0);
            assert_eq!(ordered(&next).last().expect("Cut").extent, tool.extent);
            source = next;
        }
        let original = std::fs::read(&source).expect("bytes");
        let cold = measured(&source, &tools, &L, H);
        assert!((cold - (16000. - reach(&tools, H))).abs() < 1e-6);
        assert_eq!(
            cold,
            measure_part_at(&source, &tools, &L, H, Some(&vec![Miss; n + 1]))
        );
        assert_eq!(
            cold,
            measure_part_at(&source, &tools, &L, H, Some(&vec![Hit; n + 1]))
        );
        let ordered = ordered(&source);
        let never = root.path().join("never.fcad");

        // Edit the first, a middle and the last link.
        for index in std::collections::BTreeSet::from([0, n / 2, n - 1]) {
            let saved = &ordered[index];
            let tool = tools[index];
            let copy = root.path().join(format!("edit-{index}.fcad"));
            let changed = CutSpec {
                center_mm: [tool.center_mm[0] - 0.25, tool.center_mm[1] + 0.125],
                radius_mm: tool.radius_mm - 0.125,
                ..tool
            };
            edit(&source, &copy, saved, changed, 0);
            only_the_cut(&source, &copy, saved);
            let mut all = tools.clone();
            all[index] = changed;
            measured(&copy, &all, &L, H);
            // Into the notch: refused, and names nothing it did not check.
            let out = edit(
                &source,
                &never,
                saved,
                CutSpec {
                    center_mm: [40., 30.],
                    ..tool
                },
                2,
            );
            let message = out["error"]["message"].as_str().expect("message");
            assert!(message.contains(&saved.feature.to_string()), "{message}");
            assert!(!never.exists());
        }

        // Height: ThroughAll follows, Blind stays absolute.
        let grown = root.path().join("grown.fcad");
        height(&source, &grown, 13., 0);
        measured(&grown, &tools, &L, 13.);
        assert_eq!(
            measure_part_at(&grown, &tools, &L, 13., Some(&vec![Miss; n + 1])),
            measure_part_at(&grown, &tools, &L, 13., Some(&vec![Hit; n + 1]))
        );

        // Coordinates: the L widens and its notch moves, every tool clear.
        let wider = [
            [-2., -1.5],
            [61.25, 0.],
            [61.25, 21.],
            [20.5, 21.],
            [20.5, 41.],
            [0., 40.],
        ];
        let moved = root.path().join("moved.fcad");
        sketch(&grown, &moved, &wider, 0);
        measured(&moved, &tools, &wider, 13.);
        assert_eq!(
            base_lines(&moved)
                .iter()
                .map(|(_, a, _)| *a)
                .collect::<Vec<_>>(),
            wider
        );
        if n == 2 && artifact < 2 {
            for path in [&grown, &moved] {
                if let Some(dir) = std::env::var_os("FCAD_POLYGON_CUT_ARTIFACTS") {
                    let dir = Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("artifacts");
                    reply(
                        cli()
                            .arg("export-fbx")
                            .arg(path)
                            .arg("-o")
                            .arg(dir.join(format!("polygon-{artifact}.fbx")))
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
        if n == 16 {
            // A far wall tangent to link 11 (slot 9) is refused by the evaluator.
            let mut near = L;
            near[1][0] = tools[11].center_mm[0] + tools[11].radius_mm;
            near[2][0] = near[1][0];
            let out = sketch(&source, &never, &near, 2);
            let message = out["error"]["message"].as_str().expect("message");
            assert!(
                message.contains(&ordered[11].feature.to_string()),
                "{message}"
            );
            assert!(!never.exists());
        }
        assert_eq!(std::fs::read(&source).expect("source"), original);
    }
    assert_eq!(artifact, 2);
}

/// Sloped convex parts, both windings, far from the origin: every wall is
/// found by its saved UUID and faces out of the part.
#[test]
fn native_sloped_polygons_both_windings_translated() {
    if !native() {
        return;
    }
    let shift = [1000.25, -250.5];
    let triangle = [[0., 0.], [50., 5.], [10., 40.]];
    let quad = [[0., 0.], [50., 10.], [45., 45.], [-5., 35.]];
    for (name, shape, inside, over) in [
        (
            "triangle",
            &triangle[..],
            [[20., 15.], [15., 28.]],
            [30., 22.],
        ),
        ("quad", &quad[..], [[15., 12.], [30., 30.]], [44., 30.]),
    ] {
        for reversed in [false, true] {
            let root = tempfile::tempdir().expect("root");
            let mut points: Vec<_> = shape
                .iter()
                .map(|p| [p[0] + shift[0], p[1] + shift[1]])
                .collect();
            if reversed {
                points.reverse();
            }
            let tools: Vec<_> = inside
                .iter()
                .zip([CutDepth::ThroughAll, CutDepth::Blind { depth_mm: 4.5 }])
                .map(|(c, extent)| CutSpec {
                    center_mm: [c[0] + shift[0], c[1] + shift[1]],
                    radius_mm: 3.,
                    extent,
                })
                .collect();
            let mut source = created(root.path(), &points, name);
            for (i, tool) in tools.iter().enumerate() {
                let next = root.path().join(format!("{name}-{i}.fcad"));
                add(&source, &next, *tool, 0);
                source = next;
            }
            let before_volume = measured(&source, &tools, &points, H);
            assert_eq!(
                base_lines(&source)
                    .iter()
                    .map(|(_, a, _)| *a)
                    .collect::<Vec<_>>(),
                points
            );
            // Crossing a sloped wall now produces an open slot.
            let slot = root.path().join("open-slot.fcad");
            add(
                &source,
                &slot,
                CutSpec {
                    center_mm: [over[0] + shift[0], over[1] + shift[1]],
                    radius_mm: 4.,
                    extent: CutDepth::ThroughAll,
                },
                0,
            );
            let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
            let d = Document::open_read_only(&slot).expect("reopen");
            let built =
                ferritecad_eval::rebuild_cold(&d, &mut kernel, &OperationContext::default())
                    .expect("cold");
            assert!(built.all_built(), "{:?}", built.report());
            crate::names::assert_primary(&built);
            let volume = kernel
                .shape_stats(built.shape(tip(&d).0).expect("body"))
                .expect("volume")
                .1;
            built.release_all(&mut kernel);
            assert!(volume < before_volume);
        }
    }
}
