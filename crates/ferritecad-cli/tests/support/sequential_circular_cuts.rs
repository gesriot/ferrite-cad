// SPDX-License-Identifier: MIT
//! §26C. Process, SQL, semantic geometry and independent STL evidence.
use super::*;
use ferritecad_document::{CapSide, CircularCut, EndCondition, Expression};
use ferritecad_eval::CacheOutcome;
use ferritecad_kernel::{FaceSurface, TessellationParams};
use ferritecad_types::ObjectId;

const SIZE: [f64; 3] = [80., 50., 12.];
const TOOLS: [CircularCut; 2] = [
    CircularCut {
        center_mm: [20., 20.],
        radius_mm: 5.,
        extent: CutExtent::Blind { depth_mm: 4. },
    },
    CircularCut {
        center_mm: [55., 30.],
        radius_mm: 7.,
        extent: CutExtent::Blind { depth_mm: 7. },
    },
];

fn ask(source: &Path, destination: &Path, cut: CircularCut, code: i32) -> Value {
    let catalog = inspect(source);
    let request = source.with_extension("request.json");
    write(
        &request,
        &json!({"request_version":1,"center_mm":cut.center_mm,
        "radius_mm":cut.radius_mm,"depth_mm":cut.extent.blind_depth_mm().expect("blind")}),
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

fn first(root: &Path, tool: CircularCut, native_build: bool) -> PathBuf {
    let plate = root.join("plate.fcad");
    write_plate_document(&plate, SIZE[0], SIZE[1], SIZE[2]);
    let source = root.join("first.fcad");
    if native_build {
        ask(&plate, &source, tool, 0);
    } else {
        std::fs::copy(&plate, &source).expect("fixture copy");
        let mut d = Document::open(&source).expect("open");
        let body = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
            .expect("body")
            .id;
        let p = ferritecad_document::prepare_circular_cut(&d, body, &tool).expect("prepare");
        d.write_circular_cut(&p).expect("write");
        d.close().expect("close");
    }
    source
}

fn tip(d: &Document) -> (ObjectId, ObjectId) {
    d.objects()
        .expect("objects")
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Body(b) => Some((o.id, b.tip_feature.expect("tip"))),
            _ => None,
        })
        .expect("body")
}

/// Read the triangles of the *resolved handle*. Their position/normal is an
/// assertion about the name, never a way of finding or assigning that name.
fn measure(path: &Path, tools: [CircularCut; 2], cache: Option<[CacheOutcome; 3]>) -> f64 {
    measure_history(path, &tools, cache.as_ref().map(|v| v.as_slice()))
}

fn measure_history(path: &Path, tools: &[CircularCut], cache: Option<&[CacheOutcome]>) -> f64 {
    measure_history_in(path, tools, [[0., 0.], [SIZE[0], SIZE[1]]], cache)
}

fn measure_history_in(
    path: &Path,
    tools: &[CircularCut],
    bounds: [[f64; 2]; 2],
    cache: Option<&[CacheOutcome]>,
) -> f64 {
    measure_history_at(path, tools, bounds, SIZE[2], cache)
}

fn measure_history_at(
    path: &Path,
    tools: &[CircularCut],
    bounds: [[f64; 2]; 2],
    height: f64,
    cache: Option<&[CacheOutcome]>,
) -> f64 {
    let plate_volume = (bounds[1][0] - bounds[0][0]) * (bounds[1][1] - bounds[0][1]) * height;
    let d = Document::open_read_only(path).expect("reopen");
    assert!(d.validate().expect("validate").is_ok());
    let (body, last) = tip(&d);
    let objects = d.objects().expect("objects");
    let features: Vec<_> = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
        .collect();
    let base = features
        .iter()
        .find(|o| matches!(&o.payload, ObjectPayload::Extrude(e) if e.previous.is_none()))
        .expect("base")
        .id;
    let mut chain = Vec::new();
    let mut cursor = last;
    while cursor != base {
        chain.push(cursor);
        let ObjectPayload::Extrude(e) = &objects
            .iter()
            .find(|o| o.id == cursor)
            .expect("linked feature")
            .payload
        else {
            panic!("Cut")
        };
        cursor = e.previous.expect("predecessor");
    }
    chain.reverse();
    assert_eq!(chain.len(), tools.len());
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = if let Some(expected) = cache {
        let mut store = ferritecad_document::CacheStore::open(
            path.with_extension("fcad-cache"),
            d.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("cache");
        let (built, events) =
            ferritecad_eval::rebuild_cached(&d, &mut kernel, &mut store, &context).expect("cached");
        assert_eq!(
            events
                .iter()
                .map(|e| (e.feature, e.outcome))
                .collect::<Vec<_>>(),
            std::iter::once(base)
                .chain(chain.iter().copied())
                .zip(expected.iter().copied())
                .collect::<Vec<_>>(),
            "dependent chain must invalidate exactly"
        );
        built
    } else {
        ferritecad_eval::rebuild_cold(&d, &mut kernel, &context).expect("cold")
    };
    let final_shape = built.shape(body).expect("Body");
    assert_eq!(
        built.shape(last),
        Some(final_shape),
        "Body must expose the final Cut"
    );
    let (count, volume) = kernel.shape_stats(final_shape).expect("stats");
    let floors = tools
        .iter()
        .filter(|t| t.extent.reach_mm(height) < height)
        .count();
    assert_eq!(count, 6 + tools.len() as u64 + floors as u64);
    let exact = plate_volume
        - tools
            .iter()
            .map(|t| PI * t.radius_mm.powi(2) * t.extent.reach_mm(height))
            .sum::<f64>();
    assert!((volume - exact).abs() < 1e-6, "{volume} != {exact}");
    for (index, feature) in std::iter::once(base)
        .chain(chain.iter().copied())
        .enumerate()
    {
        let (faces, volume) = kernel
            .shape_stats(built.shape(feature).expect("historical shape"))
            .expect("stats");
        let prior = &tools[..index];
        let exact = plate_volume
            - prior
                .iter()
                .map(|t| PI * t.radius_mm.powi(2) * t.extent.reach_mm(height))
                .sum::<f64>();
        assert!((volume - exact).abs() < 1e-6, "historical volume {index}");
        assert_eq!(
            faces,
            (6 + index
                + prior
                    .iter()
                    .filter(|t| t.extent.reach_mm(height) < height)
                    .count()) as u64
        );
    }
    let mut meshes = BTreeMap::new();
    for feature in std::iter::once(base).chain(chain.iter().copied()) {
        meshes.insert(
            feature,
            kernel
                .tessellate(
                    built.shape(feature).expect("shape"),
                    &TessellationParams::default(),
                    &context,
                )
                .expect("mesh"),
        );
    }
    let mut named_final = std::collections::BTreeSet::new();
    let mut named_producers = BTreeMap::<_, std::collections::BTreeSet<_>>::new();
    for reference in d.topology_refs().expect("refs") {
        let resolved = built.resolve(&reference).expect("resolve");
        assert_eq!(resolved.len(), 1);
        let face = resolved[0];
        assert!(
            named_producers
                .entry(reference.producer_feature)
                .or_default()
                .insert(face),
            "duplicate name in historical producer"
        );
        if reference.producer_feature == last {
            assert!(named_final.insert(face), "two meanings share a face");
        }
        assert_eq!(
            face.shape(),
            built
                .shape(reference.producer_feature)
                .expect("historical producer")
        );
        let (origin, cap, segment) = match reference.output_role {
            SemanticRole::ExtrudeCap { side } => (reference.producer_feature, Some(side), None),
            SemanticRole::ExtrudeSide { profile_segment } => {
                (reference.producer_feature, None, Some(profile_segment))
            }
            SemanticRole::CarriedCap { side } => (base, Some(side), None),
            SemanticRole::CarriedSide { profile_segment } => (base, None, Some(profile_segment)),
            SemanticRole::OriginCap {
                origin_feature,
                side,
            } => (origin_feature, Some(side), None),
            SemanticRole::OriginSide {
                origin_feature,
                profile_segment,
            } => (origin_feature, None, Some(profile_segment)),
            _ => panic!("unexpected role"),
        };
        let mesh = &meshes[&reference.producer_feature];
        let range = mesh
            .faces
            .iter()
            .find(|r| r.face == face)
            .expect("exact handle's mesh");
        let indices = &mesh.indices
            [range.first_index as usize..(range.first_index + range.index_count) as usize];
        let vertices: Vec<[f64; 3]> = indices
            .iter()
            .map(|i| {
                let n = *i as usize * 3;
                std::array::from_fn(|j| mesh.positions[n + j] as f64)
            })
            .collect();
        let normals: Vec<[f64; 3]> = indices
            .iter()
            .map(|i| {
                let n = *i as usize * 3;
                std::array::from_fn(|j| mesh.normals[n + j] as f64)
            })
            .collect();
        if origin == base {
            assert_eq!(
                kernel.face_surface(face).expect("surface"),
                FaceSurface::Plane
            );
            if let Some(side) = cap {
                let z = if side == CapSide::Start { 0. } else { height };
                assert!(
                    vertices.iter().all(|p| (p[2] - z).abs() < ROUNDING_MM),
                    "outer cap changed meaning"
                );
                let sign = if side == CapSide::Start { -1. } else { 1. };
                assert!(normals.iter().all(|n| n[2] * sign > 0.99));
            } else {
                let segment = segment.expect("wall segment");
                let line = objects
                    .iter()
                    .find_map(|o| match &o.payload {
                        ObjectPayload::Sketch(s) => s
                            .curves
                            .iter()
                            .find(|c| c.id == segment)
                            .map(|c| &c.geometry),
                        _ => None,
                    })
                    .expect("UUID-selected base segment");
                let SketchGeometry::Line { start, end } = line else {
                    panic!("line")
                };
                let axis = if start.x == end.x { 0 } else { 1 };
                let coord = if axis == 0 { start.x } else { start.y };
                assert!(
                    vertices
                        .iter()
                        .all(|p| (p[axis] - coord).abs() < ROUNDING_MM),
                    "outer wall changed meaning"
                );
                let sign = if coord == bounds[0][axis] { -1. } else { 1. };
                assert!(normals.iter().all(|n| n[axis] * sign > 0.99));
            }
        } else {
            let tool = tools[chain
                .iter()
                .position(|id| *id == origin)
                .expect("exact origin UUID")];
            if cap.is_some() {
                assert_eq!(cap, Some(CapSide::End));
                assert_eq!(
                    kernel.face_surface(face).expect("surface"),
                    FaceSurface::Plane
                );
                assert!(
                    vertices
                        .iter()
                        .all(|p| (p[2] - tool.extent.reach_mm(height)).abs() < ROUNDING_MM),
                    "pocket floor became another cap"
                );
                assert!(vertices.iter().all(|p| {
                    (p[0] - tool.center_mm[0]).hypot(p[1] - tool.center_mm[1])
                        <= tool.radius_mm + ROUNDING_MM
                }));
                assert!(normals.iter().all(|n| n[2] < -0.99));
            } else {
                let FaceSurface::Cylinder { radius } = kernel.face_surface(face).expect("surface")
                else {
                    panic!("cavity wall must be analytic cylinder")
                };
                assert!((radius - tool.radius_mm).abs() < 1e-8);
                let (origin, axis) = kernel
                    .cylinder_axis(face)
                    .expect("analytic axis of UUID handle");
                assert!(
                    (origin[0] - tool.center_mm[0]).abs() < 1e-8
                        && (origin[1] - tool.center_mm[1]).abs() < 1e-8
                );
                assert!(
                    axis[0].abs() < 1e-10
                        && axis[1].abs() < 1e-10
                        && (axis[2].abs() - 1.).abs() < 1e-10
                );
                assert!(vertices.iter().all(|p| {
                    ((p[0] - tool.center_mm[0]).hypot(p[1] - tool.center_mm[1]) - tool.radius_mm)
                        .abs()
                        < ROUNDING_MM
                }));
                assert!(
                    (vertices
                        .iter()
                        .map(|p| p[2])
                        .fold(f64::NEG_INFINITY, f64::max)
                        - tool.extent.reach_mm(height))
                    .abs()
                        < ROUNDING_MM
                );
                assert!(
                    vertices
                        .iter()
                        .map(|p| p[2])
                        .fold(f64::INFINITY, f64::min)
                        .abs()
                        < ROUNDING_MM
                );
                assert!(
                    vertices
                        .iter()
                        .zip(normals)
                        .all(|(p, n)| (p[0] - tool.center_mm[0]) * n[0]
                            + (p[1] - tool.center_mm[1]) * n[1]
                            < 0.)
                );
            }
        }
    }
    assert_eq!(
        named_final.len(),
        count as usize,
        "every final surface has a distinct meaning"
    );
    for (producer, names) in named_producers {
        if producer == base {
            continue;
        } // The source fixture may name only a subset of its base faces.
        let (count, _) = kernel
            .shape_stats(built.shape(producer).expect("producer"))
            .expect("stats");
        assert_eq!(
            names.len(),
            count as usize,
            "each historical producer names every surface"
        );
    }
    built.release_all(&mut kernel);
    d.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0);
    volume
}

fn kept(path: &Path, name: &str) {
    if let Some(root) = std::env::var_os("FCAD_SEQUENTIAL_CUT_ARTIFACTS") {
        std::fs::create_dir_all(&root).expect("artifacts");
        std::fs::copy(path, Path::new(&root).join(name)).expect("keep");
    }
}

#[test]
fn native_four_pairs_keep_every_surface_origin_cold_and_cached() {
    if !native() {
        return;
    }
    use CacheOutcome::{Hit, Miss};
    for alternate in [false, true] {
        for [d1, d2] in [[12., 12.], [12., 7.], [4., 12.], [4., 7.]] {
            let root = tempfile::tempdir().expect("root");
            let mut tools = TOOLS;
            tools[0].extent = CutExtent::Blind { depth_mm: d1 };
            tools[1].extent = CutExtent::Blind { depth_mm: d2 };
            if alternate {
                tools[0].center_mm = [58., 17.];
                tools[0].radius_mm = 3.5;
                tools[1].center_mm = [21., 33.];
                tools[1].radius_mm = 6.;
            }
            let source = first(root.path(), tools[0], true);
            // Display metadata is deliberately misleading; identities come from links.
            let sql = rusqlite::Connection::open(&source).expect("SQL");
            sql.execute("UPDATE objects SET name='same',ordinal=100-ordinal", [])
                .expect("reorder");
            sql.execute("UPDATE capabilities SET rowid=rowid+100", [])
                .expect("rowids");
            sql.execute(
                "INSERT INTO capabilities(rowid,name,required) VALUES(900,'optional.extension',0)",
                [],
            )
            .expect("optional");
            sql.execute_batch("CREATE TABLE extra_data(id INTEGER PRIMARY KEY, bytes BLOB); INSERT INTO extra_data VALUES(91,x'010203');").expect("extra");
            drop(sql);
            let original = std::fs::read(&source).expect("bytes");
            let copy = root.path().join("two.fcad");
            let before = inspect(&source);
            assert_eq!(before["bodies"][0]["cut_edit"]["available"], true);
            assert_eq!(
                before["bodies"][0]["cut_edit"]["target"]["existing_cut"]["depth_mm"],
                d1
            );
            let result = ask(&source, &copy, tools[1], 0);
            assert_eq!(
                result["result"]["previous_feature_id"],
                before["bodies"][0]["cut_edit"]["target"]["tip_feature_id"]
            );
            let doc = Document::open_read_only(&copy).expect("doc");
            let (body, _) = tip(&doc);
            assert_eq!(doc.objects().expect("objects").len(), 8);
            assert_eq!(doc.dependencies().expect("deps").len(), 9);
            doc.close().expect("close");
            only_the_cut_was_added(&source, &copy, body);
            let a = tables(&source);
            let b = tables(&copy);
            for row in &a["capabilities"].1 {
                assert!(
                    b["capabilities"].1.contains(row),
                    "optional rows and rowids survive"
                );
            }
            assert_eq!(b["capabilities"].1.len(), a["capabilities"].1.len() + 1);
            let named = 8 + tools
                .iter()
                .filter(|t| t.extent.blind_depth_mm().expect("blind") < SIZE[2])
                .count();
            assert_eq!(
                b["topology_refs"].1.len(),
                a["topology_refs"].1.len() + named
            );
            for row in &a["topology_refs"].1 {
                assert!(
                    b["topology_refs"].1.contains(row),
                    "saved reference including rowid changed"
                );
            }
            let role_column = a["deps"].0.iter().position(|c| c == "role").expect("role");
            for row in &a["deps"].1 {
                if row[role_column] != rusqlite::types::Value::Text("body_tip".into()) {
                    assert!(
                        b["deps"].1.contains(row),
                        "non-tip dependency including rowid changed"
                    );
                }
            }
            let volume = measure(&copy, tools, None);
            for mode in [[Miss; 3], [Hit; 3]] {
                assert!((measure(&copy, tools, Some(mode)) - volume).abs() < 1e-8);
            }
            let catalog = inspect(&copy);
            assert_eq!(catalog["bodies"][0]["cut_edit"]["available"], true);
            assert!(
                catalog["features"]
                    .as_array()
                    .expect("features")
                    .iter()
                    .filter(|f| f["circular_cut_edit"]["available"] == true)
                    .count()
                    == 2
            );
            let stl = copy.with_extension("stl");
            check_two_mesh(&mesh(&copy, &stl), tools);
            let fbx = copy.with_extension("fbx");
            let exported = reply(
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
            assert_eq!(exported["result"]["geometries"], 1);
            if !alternate {
                kept(&fbx, &format!("sequential-{d1}-{d2}.fbx"));
            }
            assert_eq!(std::fs::read(&source).expect("unchanged"), original);
            if let Some(root) = std::env::var_os("FCAD_SEQUENTIAL_CUT_ARTIFACTS") {
                use std::io::Write;
                let mut log = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(Path::new(&root).join("measurements.tsv"))
                    .expect("measurement log");
                writeln!(log, "{alternate}\t{d1}\t{d2}\t{volume:.9}").expect("measurement");
            }
        }
    }
}

#[test]
fn native_each_tool_invalidates_only_its_dependent_chain() {
    if !native() {
        return;
    }
    use CacheOutcome::{Hit, Miss};
    let root = tempfile::tempdir().expect("root");
    let source = first(root.path(), TOOLS[0], true);
    let copy = root.path().join("two.fcad");
    ask(&source, &copy, TOOLS[1], 0);
    measure(&copy, TOOLS, Some([Miss; 3]));
    measure(&copy, TOOLS, Some([Hit; 3]));
    let mut tools = TOOLS;
    for index in [1, 0] {
        let mut d = Document::open(&copy).expect("controlled fixture");
        let (_, last) = tip(&d);
        let last_record = d.object(last).expect("read").expect("last");
        let ObjectPayload::Extrude(e) = &last_record.payload else {
            panic!("extrude")
        };
        let id = if index == 1 {
            last
        } else {
            e.previous.expect("first")
        };
        let mut changed = d.object(id).expect("read").expect("cut");
        tools[index].extent = CutExtent::Blind {
            depth_mm: tools[index].extent.blind_depth_mm().expect("blind") - 1.,
        };
        let ObjectPayload::Extrude(e) = &mut changed.payload else {
            panic!("cut")
        };
        e.end_condition = EndCondition::Blind {
            distance: Expression::constant(tools[index].extent.blind_depth_mm().expect("blind"))
                .expect("depth"),
        };
        // Fixture-only write; this is explicitly not a public edit API.
        d.write(|w| {
            w.put_object(
                changed.id,
                changed.parent,
                changed.ordinal,
                changed.name.as_deref(),
                &changed.payload,
            )
        })
        .expect("fixture edit");
        d.close().expect("close");
        measure(
            &copy,
            tools,
            Some(if index == 1 {
                [Hit, Hit, Miss]
            } else {
                [Hit, Miss, Miss]
            }),
        );
        measure(&copy, tools, None);
        measure(&copy, tools, Some([Hit; 3]));
    }
}

#[test]
fn sequential_discovery_and_refusals_without_native() {
    let root = tempfile::tempdir().expect("root");
    let source = first(root.path(), TOOLS[0], false);
    let catalog = inspect(&source);
    assert_eq!(catalog["bodies"][0]["cut_edit"]["available"], true);
    let d = Document::open_read_only(&source).expect("doc");
    let (body, _) = tip(&d);
    for bad in [
        CircularCut {
            center_mm: [20., 20.],
            ..TOOLS[1]
        }, // overlap / nesting
        CircularCut {
            center_mm: [32., 20.],
            ..TOOLS[1]
        }, // tangent
        CircularCut {
            center_mm: [32. + 0.5e-7, 20.],
            ..TOOLS[1]
        },
        CircularCut {
            center_mm: [7., 30.],
            ..TOOLS[1]
        }, // external wall
        CircularCut {
            extent: CutExtent::Blind { depth_mm: 13. },
            ..TOOLS[1]
        },
    ] {
        assert!(ferritecad_document::prepare_circular_cut(&d, body, &bad).is_err());
    }
    ferritecad_document::prepare_circular_cut(&d, body, &TOOLS[1]).expect("separate");
    d.close().expect("close");
    if !ferritecad_occt::is_available() {
        let before = std::fs::read(&source).expect("bytes");
        let never = root.path().join("never.fcad");
        ask(&source, &never, TOOLS[1], 2);
        assert!(!never.exists());
        assert_eq!(std::fs::read(source).expect("bytes"), before);
    }
}

#[test]
fn native_sequential_refusals_publish_nothing() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = first(root.path(), TOOLS[0], true);
    let never = root.path().join("never.fcad");
    let before = std::fs::read(&source).expect("bytes");
    for bad in [
        CircularCut {
            center_mm: [20., 20.],
            ..TOOLS[1]
        },
        CircularCut {
            center_mm: [30., 20.],
            ..TOOLS[1]
        },
        CircularCut {
            center_mm: [32., 20.],
            ..TOOLS[1]
        },
        CircularCut {
            center_mm: [32. + 0.5e-7, 20.],
            ..TOOLS[1]
        },
        CircularCut {
            radius_mm: 2.,
            center_mm: [20., 20.],
            ..TOOLS[1]
        },
        CircularCut {
            center_mm: [7., 30.],
            ..TOOLS[1]
        },
        CircularCut {
            extent: CutExtent::Blind { depth_mm: 13. },
            ..TOOLS[1]
        },
    ] {
        let result = ask(&source, &never, bad, 2);
        assert_eq!(result["error"]["kind"], "input");
        assert!(!never.exists());
        assert_eq!(std::fs::read(&source).expect("bytes"), before);
        assert!(
            !entries(root.path())
                .iter()
                .any(|p| p.to_string_lossy().contains("scratch"))
        );
    }
    std::fs::write(&never, b"occupied").expect("occupied");
    ask(&source, &never, TOOLS[1], 2);
    assert_eq!(std::fs::read(&never).expect("bytes"), b"occupied");
    ask(&source, &source, TOOLS[1], 2);
    let alias = root.path().join("alias.fcad");
    std::fs::hard_link(&source, &alias).expect("hard link");
    ask(&source, &alias, TOOLS[1], 2);
    #[cfg(unix)]
    {
        let link = root.path().join("link.fcad");
        std::os::unix::fs::symlink(&source, &link).expect("symlink");
        ask(&source, &link, TOOLS[1], 2);
    }
    let catalog = inspect(&source);
    let request = source.with_extension("request.json");
    let stale = reply(
        cli()
            .arg(OP)
            .arg(&source)
            .args([
                "--body",
                catalog["bodies"][0]["body_id"].as_str().expect("body"),
                "--expect-version",
                &"0".repeat(64),
                "--request",
            ])
            .arg(request)
            .arg("-o")
            .arg(root.path().join("stale.fcad"))
            .arg("--json")
            .output()
            .expect("stale"),
        OP,
        2,
    );
    assert!(stale.to_string().contains("changed"), "{stale}");
    let copy = root.path().join("two.fcad");
    ask(&source, &copy, TOOLS[1], 0);
    let third = ask(
        &copy,
        &root.path().join("three.fcad"),
        CircularCut {
            center_mm: [40., 15.],
            radius_mm: 2.,
            extent: CutExtent::Blind { depth_mm: 3. },
        },
        0,
    );
    assert_eq!(third["ok"], true);
    assert_eq!(std::fs::read(&source).expect("bytes"), before);
}

#[test]
#[cfg(not(feature = "planegcs"))]
fn occt_without_solver_builds_two_unconstrained_cuts() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = first(root.path(), TOOLS[0], true);
    let copy = root.path().join("two.fcad");
    ask(&source, &copy, TOOLS[1], 0);
    measure(&copy, TOOLS, None);
}

fn check_two_mesh(m: &Mesh, tools: [CircularCut; 2]) {
    check_history_mesh(m, &tools)
}

fn check_history_mesh(m: &Mesh, tools: &[CircularCut]) {
    check_history_mesh_in(m, tools, [[0., 0.], [SIZE[0], SIZE[1]]]);
}

fn check_history_mesh_in(m: &Mesh, tools: &[CircularCut], bounds: [[f64; 2]; 2]) {
    check_history_mesh_at(m, tools, bounds, SIZE[2]);
}

fn check_history_mesh_at(m: &Mesh, tools: &[CircularCut], bounds: [[f64; 2]; 2], height: f64) {
    let plate_volume = (bounds[1][0] - bounds[0][0]) * (bounds[1][1] - bounds[0][1]) * height;
    // Closed and wound one way.
    let quantise = |v: [f64; 3]| {
        let q = |x: f64| (x * 1e4).round() as i64;
        [q(v[0]), q(v[1]), q(v[2])]
    };
    let mut directed: BTreeMap<_, usize> = BTreeMap::new();
    for face in &m.faces {
        let q: Vec<_> = face.iter().map(|v| quantise(*v)).collect();
        for k in 0..3 {
            *directed.entry((q[k], q[(k + 1) % 3])).or_default() += 1;
        }
    }
    assert!(
        directed.values().all(|used| *used == 1),
        "a directed edge is used more than once; the mesh is not one oriented surface"
    );
    for (from, to) in directed.keys() {
        assert!(
            directed.contains_key(&(*to, *from)),
            "an edge has no opposite; the mesh is open"
        );
    }

    for tool in tools {
        let CircularCut {
            center_mm: center,
            radius_mm: radius,
            extent,
        } = *tool;
        let depth = extent.reach_mm(height);
        // The bore wall exists, at the radius asked for and about the centre asked
        // for. A facet of the pocket's floor also has its corners on that circle —
        // the floor is a disc whose rim is the bore — so the wall is the part of it
        // that spans the depth, which is what "wall" means.
        let on_bore = |t: &[[f64; 3]; 3]| {
            t.iter().all(|v| {
                ((v[0] - center[0]).hypot(v[1] - center[1]) - radius).abs()
                    < LINEAR_MM + ROUNDING_MM
            })
        };
        let spans_depth = |t: &[[f64; 3]; 3]| {
            let zs = [t[0][2], t[1][2], t[2][2]];
            zs.iter().fold(f64::NEG_INFINITY, |a, b| a.max(*b))
                - zs.iter().fold(f64::INFINITY, |a, b| a.min(*b))
                > ROUNDING_MM
        };
        let bore: Vec<_> = m
            .faces
            .iter()
            .filter(|t| on_bore(t) && spans_depth(t))
            .collect();
        assert!(bore.len() >= 12, "the mesh has no bore wall at all");

        // A cavity is wound the other way from the outside: its facets face the
        // axis. That is what tells a hole from a boss of the same size.
        let radial = |t: &[[f64; 3]; 3]| {
            let [a, b, c] = t;
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2]];
            let mx = (a[0] + b[0] + c[0]) / 3. - center[0];
            let my = (a[1] + b[1] + c[1]) / 3. - center[1];
            n[0] * mx + n[1] * my
        };
        assert!(
            bore.iter().all(|t| radial(t) < 0.),
            "a bore facet faces away from the axis; this is a boss, not a hole"
        );

        // How deep it goes, measured from the bore's own vertices.
        let mut zs: Vec<f64> = bore.iter().flat_map(|t| t.iter().map(|v| v[2])).collect();
        zs.sort_by(f64::total_cmp);
        let (bottom, top) = (zs[0], zs[zs.len() - 1]);
        assert!(
            bottom.abs() < ROUNDING_MM,
            "the cut does not start at z = 0"
        );
        assert!(
            (top - depth).abs() < ROUNDING_MM,
            "the bore runs to {top} rather than {depth}"
        );

        // Through or not, said by the mesh rather than by the request.
        let covers = |t: &[[f64; 3]; 3], z: f64| {
            if t.iter().any(|v| (v[2] - z).abs() > ROUNDING_MM) {
                return false;
            }
            let [(x1, y1), (x2, y2), (x3, y3)] =
                [(t[0][0], t[0][1]), (t[1][0], t[1][1]), (t[2][0], t[2][1])];
            let d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3);
            if d.abs() < 1e-12 {
                return false;
            }
            let a = ((y2 - y3) * (center[0] - x3) + (x3 - x2) * (center[1] - y3)) / d;
            let b = ((y3 - y1) * (center[0] - x3) + (x1 - x3) * (center[1] - y3)) / d;
            a >= -1e-9 && b >= -1e-9 && 1. - a - b >= -1e-9
        };
        let floored = m.faces.iter().any(|t| covers(t, depth));
        let open_at_top = !m.faces.iter().any(|t| covers(t, height));
        if depth < height {
            assert!(floored, "a pocket has a floor at its depth");
            assert!(!open_at_top, "a pocket leaves the far side closed");
        } else {
            assert!(open_at_top, "a through hole is open at the far side");
        }
    }
    let expected_lo = [bounds[0][0], bounds[0][1], 0.];
    let expected_hi = [bounds[1][0], bounds[1][1], height];
    for (axis, (low, high)) in expected_lo.into_iter().zip(expected_hi).enumerate() {
        assert!((m.lo[axis] - low).abs() < ROUNDING_MM && (m.hi[axis] - high).abs() < ROUNDING_MM);
    }
    let exact = plate_volume
        - tools
            .iter()
            .map(|t| PI * t.radius_mm.powi(2) * t.extent.reach_mm(height))
            .sum::<f64>();
    let upper = plate_volume
        - tools
            .iter()
            .map(|t| PI * (t.radius_mm - LINEAR_MM).powi(2) * t.extent.reach_mm(height))
            .sum::<f64>();
    assert!(
        m.volume >= exact - 1e-3 && m.volume <= upper + 1e-3,
        "mesh volume {} outside [{exact},{upper}]",
        m.volume
    );
}

#[test]
fn native_second_cut_cancellation_and_late_version_guard_are_atomic() {
    if !native() {
        return;
    }
    use ferritecad_kernel::{CancelToken, ProgressSink};
    let root = tempfile::tempdir().expect("root");
    let source = first(root.path(), TOOLS[0], true);
    let d = Document::open_read_only(&source).expect("source");
    let read = ferritecad_document::ExtrudeEditSource::read(&d).expect("catalog");
    let (body, _) = tip(&d);
    d.close().expect("close");
    let destination = root.path().join("never.fcad");
    let request = ferritecad_jobs::CircularCutRequest {
        source: source.clone(),
        destination: destination.clone(),
        expected: read.version,
        body,
        cut: TOOLS[1],
    };
    let before = std::fs::read(&source).expect("bytes");
    let files = entries(root.path());
    let cancel = CancelToken::new();
    let signal = cancel.clone();
    let context = OperationContext::default()
        .with_cancel(cancel)
        .with_progress(ProgressSink::new(move |p| {
            if p >= 0.95 {
                signal.cancel();
            }
        }));
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let error = ferritecad_jobs::circular_cut_copy(&request, &mut kernel, &context)
        .expect_err("cancel after both builds");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
    assert!(!destination.exists());
    assert_eq!(std::fs::read(&source).expect("bytes"), before);
    assert_eq!(entries(root.path()), files, "scratch cleaned");
    let path = source.clone();
    let reached = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = reached.clone();
    let context = OperationContext::default().with_progress(ProgressSink::new(move |p| {
        if p >= 0.95 && !observed.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let sql = rusqlite::Connection::open(&path).expect("concurrent edit");
            sql.execute(
                "UPDATE objects SET name='changed while building' WHERE kind='body'",
                [],
            )
            .expect("change source version");
        }
    }));
    let error = ferritecad_jobs::circular_cut_copy(&request, &mut kernel, &context)
        .expect_err("late source change");
    assert!(reached.load(std::sync::atomic::Ordering::SeqCst));
    assert!(error.to_string().contains("source has changed"), "{error}");
    assert!(!destination.exists());
    assert_eq!(entries(root.path()), files, "scratch cleaned");
}

#[path = "edit_sequential_cuts.rs"]
mod edits;

#[path = "circular_cut_history.rs"]
mod history;

#[path = "edit_cut_base_sketch.rs"]
mod base_sketch;

#[path = "edit_cut_base_height.rs"]
mod base_height;

#[path = "circular_cut_through_all.rs"]
mod through_all;
