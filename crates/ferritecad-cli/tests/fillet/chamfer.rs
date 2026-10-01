// SPDX-License-Identifier: MIT
//! §29A: one equal-distance Chamfer on one vertical edge of a saved rectangular
//! plate, in a copy, and its distance edited in another. Everything is
//! measured: the B-Rep through the document's own references (volume, the one
//! new plane's normal, position and area, the corner it is at), the mesh with
//! an independent reader (closed, one winding, vertices, signed volume), the
//! SQL cell by cell, and the cache outcome feature by feature.
use super::*;
use ferritecad_eval::CacheOutcome;
use ferritecad_types::ObjectId;

const OPC: &str = "chamfer-edge-copy";
const OPE: &str = "edit-chamfer-distance";
const RECT: [f64; 4] = [X0, Y0, W, D];

fn uuid(v: &Value) -> ObjectId {
    v.as_str().expect("UUID").parse().expect("UUID")
}

fn discovery(f: &Fixture) -> &Value {
    &f.catalog["bodies"][0]["chamfer_edge"]
}
fn candidates(f: &Fixture) -> &Vec<Value> {
    discovery(f)["target"]["candidates"]
        .as_array()
        .expect("candidates")
}
fn at(f: &Fixture, corner: [f64; 2]) -> &Value {
    candidates(f)
        .iter()
        .find(|c| c["corner_mm"] == json!(corner))
        .unwrap_or_else(|| panic!("no candidate at {corner:?}"))
}
/// The request, with the candidate's own edge and its pair in the other order:
/// the same edge.
fn ask(f: &Fixture, corner: [f64; 2], distance: f64) {
    let edge = &at(f, corner)["edge"];
    write(
        &f.request,
        &json!({"request_version":1,
                "edge":{"feature_id":edge["feature_id"],
                        "joint":[edge["joint"][1], edge["joint"][0]]},
                "distance_mm":distance}),
    );
}
fn chamfer_from(f: &Fixture, source: &Path, body: &str, version: &str, output: &Path) -> Command {
    let mut c = cli();
    c.arg(OPC)
        .arg(source)
        .arg("--body")
        .arg(body)
        .arg("--expect-version")
        .arg(version)
        .arg("--request")
        .arg(&f.request)
        .arg("-o")
        .arg(output)
        .arg("--json");
    c
}
fn chamfer(f: &Fixture, output: &Path) -> Command {
    chamfer_from(f, &f.source, f.body_id(), f.version(), output)
}
fn edit_from(
    source: &Path,
    feature: &str,
    version: &str,
    request: &Path,
    output: &Path,
) -> Command {
    let mut c = cli();
    c.arg(OPE)
        .arg(source)
        .arg("--feature")
        .arg(feature)
        .arg("--expect-version")
        .arg(version)
        .arg("--request")
        .arg(request)
        .arg("-o")
        .arg(output)
        .arg("--json");
    c
}

/// A Chamfer written with the shipped preparation and writer and no kernel, so
/// discovery and the protocol are testable in a stub build.
fn chamfered_without_kernel(plate: Plate, corner: [f64; 2], distance: f64) -> Fixture {
    let f = Fixture::drawn(plate);
    let mut d = Document::open(&f.source).expect("writable");
    let body = uuid(&json!(f.body_id()));
    let edge = &at(&f, corner)["edge"];
    let chamfer = ferritecad_document::EdgeChamfer {
        edge: ferritecad_document::SweptEdge {
            feature: uuid(&edge["feature_id"]),
            joint: ferritecad_types::ProfileJoint::new(
                uuid_stable(&edge["joint"][0]),
                uuid_stable(&edge["joint"][1]),
            )
            .expect("joint"),
        },
        distance_mm: distance,
    };
    let prepared = ferritecad_document::prepare_edge_chamfer(&d, body, &chamfer).expect("prepared");
    d.write_edge_chamfer(&prepared).expect("written");
    d.close().expect("close");
    Fixture {
        catalog: inspect(&f.source),
        ..f
    }
}
fn uuid_stable(v: &Value) -> ferritecad_types::StableEntityId {
    v.as_str().expect("UUID").parse().expect("UUID")
}

/// What the real kernel says about the solid a document rebuilds into, read
/// through the document's own references.
#[derive(Debug, Clone)]
struct Cham {
    faces: u64,
    volume: f64,
    plane_origin: [f64; 3],
    normal: [f64; 3],
    area: f64,
    roles: BTreeMap<String, Vec<FaceSurface>>,
    shapes: usize,
    reference_count: usize,
}
impl Cham {
    fn same_as(&self, other: &Self) {
        assert_eq!(self.faces, other.faces);
        assert!((self.volume - other.volume).abs() < 1e-9 * self.volume);
        for k in 0..3 {
            assert!((self.plane_origin[k] - other.plane_origin[k]).abs() < 1e-9);
            assert!((self.normal[k] - other.normal[k]).abs() < 1e-12);
        }
        assert!((self.area - other.area).abs() < 1e-9);
        assert_eq!(self.roles, other.roles);
        assert_eq!(self.shapes, other.shapes);
        assert_eq!(self.reference_count, other.reference_count);
    }
}

fn role_of(role: &SemanticRole) -> String {
    match role {
        SemanticRole::ExtrudeCap { side } => format!("cap {side:?}"),
        SemanticRole::ExtrudeSide { .. } => "side".to_owned(),
        SemanticRole::OriginCap { side, .. } => format!("origin cap {side:?}"),
        SemanticRole::OriginSide { .. } => "origin side".to_owned(),
        SemanticRole::EdgeChamferFace { .. } => "edge chamfer face".to_owned(),
        other => panic!("unexpected role {other:?}"),
    }
}

fn measure_c(path: &Path, cache: Option<&Path>) -> (Cham, Vec<ferritecad_eval::CacheEvent>) {
    let doc = Document::open_read_only(path).expect("reopen");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let mut events = Vec::new();
    let built = match cache {
        Some(cache_path) => {
            let mut store = ferritecad_document::CacheStore::open(
                cache_path,
                doc.meta().document_id,
                kernel.identity().id(),
                kernel.identity().version(),
            )
            .expect("cache in a new kernel session");
            let (built, got) = ferritecad_eval::rebuild_cached(
                &doc,
                &mut kernel,
                &mut store,
                &OperationContext::default(),
            )
            .expect("cached rebuild");
            events = got;
            built
        }
        None => ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("cold rebuild after reopen"),
    };
    let objects = doc.objects().expect("objects");
    let body = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("a body");
    let tip = built.shape(body.id).expect("the body was built");
    let refs = doc.topology_refs().expect("refs");
    let mut roles: BTreeMap<String, Vec<FaceSurface>> = BTreeMap::new();
    let mut chamfer = None;
    for reference in &refs {
        let resolved = built.resolve(reference).expect("resolve");
        assert!(!resolved.is_empty(), "{} resolved to nothing", reference.id);
        // A reference keeps addressing its own producer's output.
        let producer = built
            .shape(reference.producer_feature)
            .expect("the producer was built");
        for handle in &resolved {
            assert_eq!(handle.shape(), producer, "{} was repointed", reference.id);
        }
        let kind = objects
            .iter()
            .find(|o| o.id == reference.producer_feature)
            .expect("producer")
            .payload
            .type_name();
        let key = format!("{} of {kind}", role_of(&reference.output_role));
        for handle in &resolved {
            roles
                .entry(key.clone())
                .or_default()
                .push(kernel.face_surface(*handle).expect("surface"));
        }
        if matches!(reference.output_role, SemanticRole::EdgeChamferFace { .. }) {
            assert_eq!(resolved.len(), 1, "the chamfer face is exactly one face");
            assert_eq!(producer, tip, "the Chamfer's face is on the Body's tip");
            chamfer = Some(resolved[0]);
        }
    }
    for surfaces in roles.values_mut() {
        surfaces.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    }
    let face = chamfer.expect("the document names the chamfer face");
    assert_eq!(
        kernel.face_surface(face).expect("surface"),
        FaceSurface::Plane,
        "a chamfer is planar"
    );
    let (plane_origin, normal, area) = kernel.face_plane(face).expect("plane");
    let (faces, volume) = kernel.shape_stats(tip).expect("stats");
    let measured = Cham {
        faces,
        volume,
        plane_origin,
        normal,
        area,
        roles,
        shapes: built.shape_count(),
        reference_count: refs.len(),
    };
    built.release_all(&mut kernel);
    doc.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0, "every handle was released");
    (measured, events)
}

/// The B-Rep is a chamfer of `distance` at `corner` of `rect` and of no other
/// corner: the volume, and one planar face whose outward normal is the diagonal
/// of exactly this corner, which passes through the two points `distance` along
/// each adjacent edge and whose area is `distance · √2 · height`.
fn check_brep(m: &Cham, rect: [f64; 4], corner: [f64; 2], distance: f64, height: f64) {
    let [x0, y0, w, d] = rect;
    assert_eq!(m.faces, 7, "two caps, four sides and the chamfer face");
    let exact = (w * d - distance * distance / 2.0) * height;
    assert!(
        (m.volume - exact).abs() < 1e-9 * w * d * height,
        "{} is not {exact}",
        m.volume
    );
    let sx = if corner[0] == x0 { -1.0 } else { 1.0 };
    let sy = if corner[1] == y0 { -1.0 } else { 1.0 };
    let root = 0.5f64.sqrt();
    assert!(
        (m.normal[0] - sx * root).abs() < 1e-12
            && (m.normal[1] - sy * root).abs() < 1e-12
            && m.normal[2].abs() < 1e-12,
        "the normal {:?} does not point out of the corner {corner:?}",
        m.normal
    );
    for p in [
        [corner[0] - sx * distance, corner[1]],
        [corner[0], corner[1] - sy * distance],
    ] {
        let off =
            (p[0] - m.plane_origin[0]) * m.normal[0] + (p[1] - m.plane_origin[1]) * m.normal[1];
        assert!(off.abs() < 1e-9, "{p:?} is {off} mm off the plane");
    }
    // The vertex of the corner is cut away: it is on the cut side of the plane.
    let corner_off = (corner[0] - m.plane_origin[0]) * m.normal[0]
        + (corner[1] - m.plane_origin[1]) * m.normal[1];
    assert!(
        (corner_off - distance * root).abs() < 1e-9,
        "the corner is {corner_off} mm beyond the plane"
    );
    assert!(
        (m.area - distance * 2.0f64.sqrt() * height).abs() < 1e-9 * w * d,
        "area {}",
        m.area
    );
}

/// The exported mesh, read independently, is exactly that part.
fn check_mesh_c(m: &Mesh, [x0, y0, w, d]: [f64; 4], corner: [f64; 2], distance: f64, height: f64) {
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
        "not one oriented surface"
    );
    for (from, to) in directed.keys() {
        assert!(directed.contains_key(&(*to, *from)), "the mesh is open");
    }
    for (k, (lo, hi)) in [(x0, x0 + w), (y0, y0 + d), (0.0, height)]
        .into_iter()
        .enumerate()
    {
        assert!((m.lo[k] - lo).abs() < ROUNDING_MM && (m.hi[k] - hi).abs() < ROUNDING_MM);
    }
    let [sx, sy] = signs_in([x0, y0], corner);
    let vertices: Vec<[f64; 3]> = m.faces.iter().flatten().copied().collect();
    let has = |x: f64, y: f64, z: f64| {
        vertices.iter().any(|v| {
            (v[0] - x).abs() < ROUNDING_MM
                && (v[1] - y).abs() < ROUNDING_MM
                && (v[2] - z).abs() < ROUNDING_MM
        })
    };
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9;
    for c in [[x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]] {
        for z in [0.0, height] {
            assert_eq!(
                has(c[0], c[1], z),
                !near(c, corner),
                "corner {c:?} at z={z}: the chamfer is at {corner:?}"
            );
        }
    }
    for z in [0.0, height] {
        assert!(has(corner[0] - sx * distance, corner[1], z), "z={z}");
        assert!(has(corner[0], corner[1] - sy * distance, z), "z={z}");
    }
    // The triangles of the slanted flat: on the plane, facing out of the corner,
    // adding up to d·√2·h.
    let root = 0.5f64.sqrt();
    let mut flat = 0.0;
    for tri in &m.faces {
        let on_plane = tri.iter().all(|v| {
            (sx * (v[0] - corner[0]) + sy * (v[1] - corner[1]) + distance).abs() < ROUNDING_MM
        });
        if !on_plane {
            continue;
        }
        let [a, b, c] = *tri;
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!(
            (n[0] * sx + n[1] * sy) / length / root > 1.0 - 1e-4,
            "a triangle of the flat faces {n:?}, not out of the corner"
        );
        flat += length / 2.0;
    }
    assert!(
        (flat - distance * 2.0f64.sqrt() * height).abs() < 1e-3,
        "the flat's triangles add up to {flat}"
    );
    let exact = (w * d - distance * distance / 2.0) * height;
    assert!(
        (m.volume - exact).abs() < 1e-6 * exact,
        "STL volume {} is not {exact}",
        m.volume
    );
}
fn signs_in([x0, y0]: [f64; 2], corner: [f64; 2]) -> [f64; 2] {
    [
        if corner[0] == x0 { -1.0 } else { 1.0 },
        if corner[1] == y0 { -1.0 } else { 1.0 },
    ]
}

/// Every SQL cell of the source survives into the copy, and the new cells are
/// exactly the ones a Chamfer is allowed to add: one object, the Body's tip,
/// the predecessor and tip edges, seven names, its capabilities and the copy's
/// own timestamp.
fn only_a_chamfer_was_added(source: &Path, copy: &Path, body: ObjectId) {
    let before = tables(source);
    let after = tables(copy);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    let selected = rusqlite::types::Value::Blob(body.to_bytes().to_vec());
    for (table, (columns, rows)) in &before {
        let (mine_columns, mine) = &after[table];
        assert_eq!(columns, mine_columns, "{table} columns");
        match table.as_str() {
            "objects" => {
                for row in rows {
                    let found = mine
                        .iter()
                        .find(|m| m[1] == row[1])
                        .unwrap_or_else(|| panic!("object {:?} disappeared", row[1]));
                    for (i, cell) in row.iter().enumerate() {
                        assert!(
                            cell == &found[i]
                                || (row.contains(&selected)
                                    && matches!(columns[i].as_str(), "payload" | "payload_hash")),
                            "objects.{} changed",
                            columns[i]
                        );
                    }
                }
                assert_eq!(mine.len(), rows.len() + 1, "one new object");
            }
            "deps" | "topology_refs" | "capabilities" => {
                for row in rows {
                    let kept = mine.iter().any(|m| m[1..] == row[1..]);
                    let is_old_tip = table == "deps" && row.contains(&selected);
                    assert!(kept || is_old_tip, "{table} row {row:?} disappeared");
                }
                let added = mine.len() - rows.len();
                match table.as_str() {
                    "deps" => assert_eq!(added, 1, "two edges in, one out"),
                    "topology_refs" => assert_eq!(added, 7, "the new names"),
                    _ => {}
                }
            }
            "meta" => {
                for (a, b) in rows.iter().zip(mine) {
                    for (i, cell) in a.iter().enumerate() {
                        assert!(
                            cell == &b[i] || columns[i] == "modified_at",
                            "meta.{} changed",
                            columns[i]
                        );
                    }
                }
            }
            _ => assert_eq!(rows, mine, "{table} changed"),
        }
    }
    let old = capability_names(source);
    let new = capability_names(copy);
    assert!(new.contains(&"feature.chamfer.v1".to_owned()));
    for name in &new {
        assert!(
            old.contains(name)
                || [
                    "feature.chamfer.v1",
                    "feature.predecessor.v1",
                    "topology.origin-face.v1"
                ]
                .contains(&name.as_str()),
            "capability {name} was added"
        );
    }
}

/// The distance edit changes exactly the Chamfer row's payload and hash and the
/// stamp; every other cell of every table is the source's.
fn only_the_distance_changed(source: &Path, copy: &Path, feature: ObjectId) {
    let before = tables(source);
    let after = tables(copy);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    let selected = rusqlite::types::Value::Blob(feature.to_bytes().to_vec());
    for (table, (columns, rows)) in &before {
        let (_, mine) = &after[table];
        assert_eq!(rows.len(), mine.len(), "{table} row count");
        for (a, b) in rows.iter().zip(mine) {
            for (i, cell) in a.iter().enumerate() {
                let allowed = (table == "meta" && columns[i] == "modified_at")
                    || (table == "objects"
                        && a.contains(&selected)
                        && matches!(columns[i].as_str(), "payload" | "payload_hash"));
                assert!(cell == &b[i] || allowed, "{table}.{} changed", columns[i]);
            }
        }
    }
}

/// Publishes one chamfer at `corner`, checks everything a copy must be, and
/// returns the copy and the feature's identity.
fn chamfer_at(
    f: &Fixture,
    source: &Path,
    corner: [f64; 2],
    d: f64,
    name: &str,
) -> (PathBuf, ObjectId, Cham) {
    let before = std::fs::read(source).expect("source bytes");
    let catalog = inspect(source);
    let row = Fixture {
        root: tempfile::tempdir().expect("directory"),
        request: f.request.clone(),
        source: source.to_path_buf(),
        catalog,
    };
    let candidate = at(&row, corner).clone();
    let copy = f.root.path().join(format!("{name}.fcad"));
    ask(&row, corner, d);
    let published = reply(
        chamfer_from(&row, source, row.body_id(), row.version(), &copy)
            .output()
            .expect("process"),
        OPC,
        0,
    );
    let result = &published["result"];
    assert_eq!(
        result["body_id"],
        row.body_id(),
        "the Body keeps its identity"
    );
    let base = discovery(&row)["target"]["base_feature_id"].clone();
    assert_eq!(result["previous_feature_id"], base, "the tip was the plate");
    assert_eq!(result["edge"], candidate["edge"], "the canonical meaning");
    assert_eq!(result["corner_mm"], json!(corner));
    assert_eq!(result["distance_mm"], d);
    assert_eq!(result["distance_unit"], "mm");
    let mut roles: Vec<&str> = result["references"]
        .as_array()
        .expect("references")
        .iter()
        .map(|r| r["role"].as_str().expect("role"))
        .collect();
    roles.sort_unstable();
    assert_eq!(
        roles,
        [
            "edge_chamfer_face",
            "origin_cap",
            "origin_cap",
            "origin_side",
            "origin_side",
            "origin_side",
            "origin_side"
        ]
    );
    assert_eq!(
        std::fs::read(source).expect("source bytes"),
        before,
        "the source was touched"
    );
    let id = uuid(&result["feature_id"]);
    only_a_chamfer_was_added(source, &copy, uuid(&json!(row.body_id())));

    // What the copy holds.
    let document = Document::open_read_only(&copy).expect("copy");
    let objects = document.objects().expect("objects");
    let saved = objects.iter().find(|o| o.id == id).expect("Chamfer");
    let ObjectPayload::Chamfer(stored) = &saved.payload else {
        panic!("not a Chamfer: {saved:?}");
    };
    assert_eq!(stored.previous, uuid(&base));
    assert_eq!(stored.edge.feature, uuid(&base));
    assert_eq!(stored.distance_mm, d);
    assert_eq!(saved.payload.schema_version(), 1);
    assert_eq!(
        saved.payload.required_capabilities(),
        [
            "core.part.v1",
            "feature.predecessor.v1",
            "feature.chamfer.v1"
        ]
    );
    let tip = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Body(b) => b.tip_feature,
            _ => None,
        })
        .expect("a tip");
    assert_eq!(tip, id, "the Body ends in the Chamfer");
    document.close().expect("close");
    let checked = cli()
        .arg("validate")
        .arg(&copy)
        .arg("--json")
        .output()
        .expect("validate");
    assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
    let n = stored_count(&copy);
    let rebuilt = cli()
        .arg("rebuild")
        .arg(&copy)
        .arg("--cold")
        .output()
        .expect("rebuild");
    let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
    assert!(
        text.contains(&format!("{n} of {n} stored references resolved")),
        "{text}"
    );
    let (cold, _) = measure_c(&copy, None);
    check_brep(&cold, RECT, corner, d, H);
    (copy, id, cold)
}
fn stored_count(path: &Path) -> usize {
    let d = Document::open_read_only(path).expect("document");
    let n = d.topology_refs().expect("refs").len();
    d.close().expect("close");
    n
}

/// Discovery, the strict request protocol, every structural refusal, a saved
/// Chamfer's discovery and every other editor's refusal of it — with no kernel
/// needed.
#[test]
fn chamfer_discovery_and_protocol_without_native() {
    let f = Fixture::drawn(Plate::new(CCW));
    let before = std::fs::read(&f.source).expect("source bytes");
    let d = discovery(&f);
    assert_eq!(d["available"], true, "{d}");
    assert!(d["refusal"].is_null() && d["document_refusal"].is_null());
    let target = &d["target"];
    assert_eq!(target["body_id"], f.body_id());
    assert_eq!(target["height_mm"], H);
    assert_eq!(target["request_versions"], json!([1]));
    assert_eq!(target["distance_unit"], "mm");
    assert_eq!(target["min_distance_mm"], 0.001);
    assert_eq!(target["min_flat_mm"], 0.01);
    assert_eq!(candidates(&f).len(), 4, "four vertical edges");
    for c in candidates(&f) {
        assert_eq!(c["edge"]["feature_id"], target["base_feature_id"]);
        let [a, b] = [0, 1].map(|i| uuid_stable(&c["edge"]["joint"][i]));
        assert!(a < b, "the pair is listed in canonical order");
        let mut lengths: Vec<f64> = c["adjacent_lengths_mm"]
            .as_array()
            .expect("lengths")
            .iter()
            .map(|l| l.as_f64().expect("length"))
            .collect();
        lengths.sort_by(f64::total_cmp);
        assert_eq!(lengths, [D, W]);
        assert_eq!(c["max_distance_mm"], D - 0.01);
        assert_eq!(c["offerable"], true);
        assert!(
            c["label"]
                .as_str()
                .expect("label")
                .contains("vertical edge")
        );
    }
    assert_eq!(f.catalog["chamfers"], json!([]));
    // The Fillet's own discovery says what it always said.
    assert_eq!(f.discovery()["available"], true);
    assert_eq!(f.discovery()["target"]["min_radius_mm"], 0.01);

    // The strict request: nothing outside it is a request.
    let never = f.root.path().join("never.fcad");
    let edge = at(&f, [X0, Y0])["edge"].clone();
    write(&f.request, &json!({}));
    let names = entries(f.root.path());
    let good = json!({"request_version":1,"edge":edge,"distance_mm":2.0});
    let mut extra = good.clone();
    extra["angle_deg"] = json!(45);
    let mut second = good.clone();
    second["second_distance_mm"] = json!(1.0);
    let mut wrong_edge = good.clone();
    wrong_edge["edge"]["extra"] = json!(1);
    let mut as_array = json!([1, edge, 2.0]);
    let _ = &mut as_array;
    for (why, request, code) in [
        ("an unknown field", extra, "input"),
        ("a second distance", second, "input"),
        ("an unknown edge field", wrong_edge, "input"),
        ("an array", as_array, "input"),
        (
            "request_version 2",
            json!({"request_version":2,"edge":edge,"distance_mm":2.0}),
            "unsupported",
        ),
        (
            "a string distance",
            json!({"request_version":1,"edge":edge,"distance_mm":"2"}),
            "input",
        ),
        (
            "no distance",
            json!({"request_version":1,"edge":edge}),
            "input",
        ),
        (
            "a malformed UUID",
            json!({"request_version":1,"edge":{"feature_id":"not-a-uuid","joint":edge["joint"]},"distance_mm":2.0}),
            "input",
        ),
        (
            "a joint of three",
            json!({"request_version":1,"edge":{"feature_id":edge["feature_id"],"joint":[edge["joint"][0],edge["joint"][1],edge["joint"][1]]},"distance_mm":2.0}),
            "input",
        ),
    ] {
        write(&f.request, &request);
        let v = reply(chamfer(&f, &never).output().expect("process"), OPC, 2);
        assert_eq!(refused(&v), code, "{why}: {v}");
        assert!(!never.exists(), "{why}");
    }
    // Numbers JSON can say that no distance is, and the policy's own bounds.
    for (why, text) in [
        ("zero", "0"),
        ("negative", "-1"),
        ("NaN", "NaN"),
        ("too small", "0.0009"),
        ("past the side", "12.25"),
        ("past the bound", "12.241"),
    ] {
        let id = edge.to_string();
        std::fs::write(
            &f.request,
            format!("{{\"request_version\":1,\"edge\":{id},\"distance_mm\":{text}}}"),
        )
        .expect("request");
        let v = reply(chamfer(&f, &never).output().expect("process"), OPC, 2);
        assert_eq!(refused(&v), "input", "{why}: {v}");
        assert!(!never.exists());
    }
    // A bound-exact request is accepted or refused only for the kernel.
    ask(&f, [X0, Y0], D - 0.01);
    let v = chamfer(&f, &never).output().expect("process");
    if ferritecad_occt::is_available() {
        let v = reply(v, OPC, 0);
        assert_eq!(v["result"]["distance_mm"], D - 0.01);
        std::fs::remove_file(&never).expect("cleanup");
    } else {
        let v = reply(v, OPC, 2);
        assert_eq!(refused(&v), "unsupported", "no kernel: {v}");
        assert!(!never.exists(), "an unsupported kernel creates no file");
    }
    assert_eq!(entries(f.root.path()), names, "a refusal left something");
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);

    // Every composition outside the class is refused by name, and nothing is
    // published.
    let trapezoid = [[X0, Y0], [X0 + W, Y0], [X0 + W - 3., Y0 + D], [X0, Y0 + D]];
    for (why, plate, expect) in [
        (
            "reversed",
            Plate {
                reversed: true,
                ..Plate::new(CCW)
            },
            "forward",
        ),
        (
            "symmetric",
            Plate {
                symmetric: true,
                ..Plate::new(CCW)
            },
            "",
        ),
        (
            "dimensioned",
            Plate {
                constrained: true,
                ..Plate::new(CCW)
            },
            "horizontal",
        ),
        (
            "placed",
            Plate {
                placement: true,
                ..Plate::new(CCW)
            },
            "",
        ),
        (
            "two bodies",
            Plate {
                second_body: true,
                ..Plate::new(CCW)
            },
            "",
        ),
        ("a trapezoid", Plate::new(trapezoid), "rectangle"),
    ] {
        let g = Fixture::drawn(plate);
        let before = std::fs::read(&g.source).expect("source bytes");
        for body in g.catalog["bodies"].as_array().expect("bodies") {
            let d = &body["chamfer_edge"];
            assert_eq!(d["available"], false, "{why}: {d}");
            assert!(d["target"].is_null(), "{why}");
            let reason = d["refusal"].as_str().expect("a reason");
            assert!(reason.to_lowercase().contains(expect), "{why}: {reason}");
        }
        assert_eq!(std::fs::read(&g.source).expect("bytes"), before, "{why}");
    }

    // A saved Chamfer, discovered without a kernel.
    let c = chamfered_without_kernel(Plate::new(CW_FROM_UPPER_RIGHT), [X0 + W, Y0], 2.375);
    let saved = c.catalog["chamfers"].as_array().expect("chamfers");
    assert_eq!(saved.len(), 1);
    let row = &saved[0];
    assert_eq!(row["name"], "Chamfer");
    assert_eq!(row["corner_mm"], json!([X0 + W, Y0]));
    assert_eq!(row["distance_mm"], 2.375);
    assert_eq!(row["distance_unit"], "mm");
    let edit = &row["distance_edit"];
    assert_eq!(edit["available"], true, "{edit}");
    assert_eq!(edit["request_versions"], json!([1]));
    assert_eq!(edit["min_distance_mm"], 0.001);
    assert_eq!(edit["max_distance_mm"], D - 0.01);
    let feature = row["feature_id"].as_str().expect("feature").to_owned();
    assert_eq!(c.catalog["bodies"][0]["chamfer_edge"]["available"], false);
    assert!(
        c.catalog["bodies"][0]["chamfer_edge"]["refusal"]
            .as_str()
            .expect("reason")
            .contains("Chamfer")
    );
    // The other editors say so too, by name: none offers an edit.
    assert_eq!(c.catalog["edit_extrude"]["available"], false);
    let why = c.catalog["edit_extrude"]["refusal"]
        .as_str()
        .expect("a reason");
    assert!(why.contains(&feature) && why.contains("Chamfer"), "{why}");
    assert_eq!(c.catalog["bodies"][0]["fillet_edge"]["available"], false);
    assert_eq!(c.catalog["bodies"][0]["cut_edit"]["available"], false);
    assert!(
        c.catalog["fillets"].as_array().expect("fillets").is_empty(),
        "a Chamfer is not a Fillet"
    );
    for sketch in c.catalog["sketches"].as_array().expect("sketches") {
        assert_eq!(sketch["editable"], false, "{sketch}");
        assert_eq!(sketch["constraint_edit"]["available"], false, "{sketch}");
    }

    // The distance request protocol.
    let version = c.version().to_owned();
    let request = c.root.path().join("distance.json");
    let out = c.root.path().join("edited.fcad");
    let kept = std::fs::read(&c.source).expect("bytes");
    for (why, body, code) in [
        (
            "an unknown field",
            json!({"request_version":1,"distance_mm":3.0,"edge":{}}),
            "input",
        ),
        (
            "request_version 2",
            json!({"request_version":2,"distance_mm":3.0}),
            "unsupported",
        ),
        ("no distance", json!({"request_version":1}), "input"),
        ("an array", json!([1, 3.0]), "input"),
        (
            "zero",
            json!({"request_version":1,"distance_mm":0.0}),
            "input",
        ),
        (
            "below the minimum",
            json!({"request_version":1,"distance_mm":0.0009}),
            "input",
        ),
        (
            "past the bound",
            json!({"request_version":1,"distance_mm":12.241}),
            "input",
        ),
    ] {
        write(&request, &body);
        let v = reply(
            edit_from(&c.source, &feature, &version, &request, &out)
                .output()
                .expect("process"),
            OPE,
            2,
        );
        assert_eq!(refused(&v), code, "{why}: {v}");
        assert!(!out.exists(), "{why}");
    }
    // An edit of a feature that is not a Chamfer.
    write(&request, &json!({"request_version":1,"distance_mm":3.0}));
    let base = c.catalog["bodies"][0]["chamfer_edge"]["target"].is_null();
    assert!(base);
    let body_id = c.body_id().to_owned();
    let v = reply(
        edit_from(&c.source, &body_id, &version, &request, &out)
            .output()
            .expect("process"),
        OPE,
        2,
    );
    assert!(matches!(refused(&v), "input" | "unsupported"), "{v}");
    // Without a kernel the bound-exact edit is refused as unsupported and
    // writes nothing; with one it publishes.
    write(
        &request,
        &json!({"request_version":1,"distance_mm":D - 0.01}),
    );
    let v = edit_from(&c.source, &feature, &version, &request, &out)
        .output()
        .expect("process");
    if ferritecad_occt::is_available() {
        reply(v, OPE, 0);
    } else {
        let v = reply(v, OPE, 2);
        assert_eq!(refused(&v), "unsupported", "{v}");
        assert!(!out.exists());
    }
    assert_eq!(std::fs::read(&c.source).expect("bytes"), kept);
}

/// A chamfer at each of the four corners of the translated, fractional plate,
/// drawn three ways (both windings, other starting Lines), at more than one
/// distance, is what the numbers say: the B-Rep, the cold reopen, the names
/// and their cache outcomes, the independent mesh and the SQL.
#[test]
fn native_a_chamfer_at_every_corner_in_three_orders_is_what_the_numbers_say() {
    if !native() {
        return;
    }
    for (index, corners) in [CCW, CW_FROM_UPPER_RIGHT, CCW_FROM_THIRD]
        .into_iter()
        .enumerate()
    {
        let f = Fixture::drawn(Plate::new(corners));
        for (k, corner) in corners.into_iter().enumerate() {
            for d in [0.75, 3.0625] {
                // Only one corner per order takes the second distance as a
                // cold-and-cached run; the rest are published and measured.
                if d == 3.0625 && k != index {
                    continue;
                }
                let name = format!("c{index}-{k}-{d}");
                let (copy, id, cold) = chamfer_at(&f, &f.source, corner, d, &name);
                let m = mesh(&copy, &f.root.path().join(format!("{name}.stl")));
                check_mesh_c(&m, RECT, corner, d, H);
                super::fbx(&copy, &name);

                // The cache: cold writes (Miss), a second run reads (Hit) and
                // equals the cold result in every number and name.
                let cache = f.root.path().join(format!("{name}.cache"));
                let (first, events) = measure_c(&copy, Some(&cache));
                assert!(
                    !events.is_empty() && events.iter().all(|e| e.outcome == CacheOutcome::Miss),
                    "{events:?}"
                );
                first.same_as(&cold);
                let (again, events) = measure_c(&copy, Some(&cache));
                assert!(
                    !events.is_empty() && events.iter().all(|e| e.outcome == CacheOutcome::Hit),
                    "{events:?}"
                );
                again.same_as(&cold);
                assert!(events.iter().any(|e| e.feature == id));
            }
        }
    }
}

/// A plane at another corner with the same volume is noticed: the same volume
/// at the same distance is not the same part.
#[test]
fn native_the_corner_and_the_volume_are_measured_apart() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let (a, _, one) = chamfer_at(&f, &f.source, [X0, Y0], 2.5, "one");
    let (b, _, two) = chamfer_at(&f, &f.source, [X0 + W, Y0 + D], 2.5, "two");
    assert!(
        (one.volume - two.volume).abs() < 1e-9 * one.volume,
        "same volume"
    );
    assert!((one.area - two.area).abs() < 1e-9, "same area");
    assert!(
        (one.normal[0] - two.normal[0]).abs() > 1.0,
        "{:?} against {:?}: opposite corners face opposite ways",
        one.normal,
        two.normal
    );
    // Each mesh passes its own corner's test and fails the other's.
    let ma = mesh(&a, &f.root.path().join("a.stl"));
    let mb = mesh(&b, &f.root.path().join("b.stl"));
    check_mesh_c(&ma, RECT, [X0, Y0], 2.5, H);
    check_mesh_c(&mb, RECT, [X0 + W, Y0 + D], 2.5, H);
    assert!(
        std::panic::catch_unwind(|| check_mesh_c(&ma, RECT, [X0 + W, Y0 + D], 2.5, H)).is_err(),
        "a chamfer at the wrong corner passed the mesh check"
    );
    assert!(
        std::panic::catch_unwind(|| check_brep(&one, RECT, [X0 + W, Y0 + D], 2.5, H)).is_err(),
        "a chamfer at the wrong corner passed the B-Rep check"
    );
}

/// The policy is exact at its bounds through the whole route: the offered
/// maximum publishes and measures, the next representable value above it is
/// refused with nothing written; so are the smallest and a below-minimum
/// distance; a corner never changes another.
#[test]
fn native_the_distance_bounds_are_exact_and_the_cache_follows_the_distance() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let corner = [X0 + W, Y0];
    let max = at(&f, corner)["max_distance_mm"].as_f64().expect("max");
    assert_eq!(max, D - 0.01);
    let never = f.root.path().join("never.fcad");
    ask(&f, corner, max);
    let names = entries(f.root.path());
    for (why, d) in [
        ("the next float above the maximum", max.next_up()),
        ("below the minimum", 0.001f64.next_down()),
        ("the whole side", D),
        ("past the side", D + 1.0),
    ] {
        ask(&f, corner, d);
        let v = reply(chamfer(&f, &never).output().expect("process"), OPC, 2);
        assert_eq!(refused(&v), "input", "{why}: {v}");
        let message = v["error"]["message"].as_str().expect("message");
        assert!(message.contains("mm"), "{why}: {message}");
    }
    assert_eq!(entries(f.root.path()), names, "a refusal left something");
    for (why, d) in [("the maximum", max), ("the minimum", 0.001)] {
        let (copy, _, built) = chamfer_at(&f, &f.source, corner, d, why.replace(' ', "-").as_str());
        check_brep(&built, RECT, corner, d, H);
        let m = mesh(&copy, &f.root.path().join(format!("bound-{d}.stl")));
        check_mesh_c(&m, RECT, corner, d, H);
    }

    // Another Chamfer over the same plate misses by its own entry and hits the
    // plate it cuts. (That the same Chamfer at another distance misses is the
    // edit test's: there the producer is the same and only the distance moved.)
    let (three, id, _) = chamfer_at(&f, &f.source, corner, 3.0, "three");
    let (four, id_four, _) = chamfer_at(&f, &f.source, corner, 4.0, "four");
    assert_ne!(id, id_four);
    let cache = f.root.path().join("shared.cache");
    measure_c(&three, Some(&cache));
    // The same plate and corner at another distance: the Chamfer misses (its
    // own key moved) and, as the plate's key did not, the plate hits.
    let (_, events) = measure_c(&four, Some(&cache));
    let outcome = |feature: ObjectId| {
        events
            .iter()
            .find(|e| e.feature == feature)
            .map(|e| e.outcome)
            .expect("an event for the feature")
    };
    assert_eq!(outcome(id_four), CacheOutcome::Miss, "{events:?}");
    let others: Vec<_> = events.iter().filter(|e| e.feature != id_four).collect();
    assert!(
        !others.is_empty() && others.iter().all(|e| e.outcome == CacheOutcome::Hit),
        "{events:?}"
    );
}

/// The distance changes in its own copy: one cell of one row moves, the
/// Chamfer keeps its UUID and every reference keeps its, the plate is a hit
/// and the Chamfer a miss, the copy measures at the new distance, and the
/// source is not touched.
#[test]
fn native_the_distance_edit_changes_one_number_and_keeps_every_identity() {
    if !native() {
        return;
    }
    for (index, corners) in [CCW, CW_FROM_UPPER_RIGHT].into_iter().enumerate() {
        let f = Fixture::drawn(Plate::new(corners));
        let corner = corners[2];
        let (one, id, first) = chamfer_at(&f, &f.source, corner, 2.375, &format!("e{index}-one"));
        let before = std::fs::read(&one).expect("source bytes");
        let catalog = inspect(&one);
        let row = &catalog["chamfers"][0];
        assert_eq!(row["feature_id"], json!(id.to_string()));
        let version = catalog["content_version"]
            .as_str()
            .expect("version")
            .to_owned();
        let request = f.root.path().join("distance.json");
        let refs_before = {
            let d = Document::open_read_only(&one).expect("source");
            let refs = d.topology_refs().expect("refs");
            d.close().expect("close");
            refs
        };
        // Edited twice in a row from the same source: up and down.
        let mut previous = (one.clone(), 2.375, first);
        for (k, d) in [4.5, 0.75, 9.0].into_iter().enumerate() {
            let out = f.root.path().join(format!("e{index}-edited-{k}.fcad"));
            write(&request, &json!({"request_version":1,"distance_mm":d}));
            let source = &previous.0;
            let catalog = inspect(source);
            let version = catalog["content_version"]
                .as_str()
                .expect("version")
                .to_owned();
            let v = reply(
                edit_from(source, &id.to_string(), &version, &request, &out)
                    .output()
                    .expect("process"),
                OPE,
                0,
            );
            let result = &v["result"];
            assert_eq!(result["feature_id"], json!(id.to_string()));
            assert_eq!(result["previous_distance_mm"], previous.1);
            assert_eq!(result["distance_mm"], d);
            assert_eq!(result["distance_unit"], "mm");
            assert_eq!(result["corner_mm"], json!(corner));
            assert_eq!(result["edge"], catalog["chamfers"][0]["edge"]);
            only_the_distance_changed(source, &out, id);
            let refs_after = {
                let doc = Document::open_read_only(&out).expect("copy");
                let refs = doc.topology_refs().expect("refs");
                doc.close().expect("close");
                refs
            };
            assert_eq!(refs_after, refs_before, "every name keeps its UUID");
            let n = stored_count(&out);
            let rebuilt = cli()
                .arg("rebuild")
                .arg(&out)
                .arg("--cold")
                .output()
                .expect("rebuild");
            assert!(
                String::from_utf8(rebuilt.stdout)
                    .expect("UTF-8")
                    .contains(&format!("{n} of {n} stored references resolved"))
            );
            let (cold, _) = measure_c(&out, None);
            check_brep(&cold, RECT, corner, d, H);
            let m = mesh(&out, &f.root.path().join(format!("e{index}-{k}.stl")));
            check_mesh_c(&m, RECT, corner, d, H);
            super::fbx(&out, &format!("edits-{index}-{k}"));

            // The cache: the plate is a hit, the Chamfer a miss, and the
            // repeat equals the cold result.
            let cache = f.root.path().join(format!("e{index}-{k}.cache"));
            measure_c(source, Some(&cache));
            let (miss, events) = measure_c(&out, Some(&cache));
            let for_chamfer = events.iter().find(|e| e.feature == id).expect("event");
            assert_eq!(for_chamfer.outcome, CacheOutcome::Miss, "{events:?}");
            assert!(
                events
                    .iter()
                    .filter(|e| e.feature != id)
                    .all(|e| e.outcome == CacheOutcome::Hit),
                "{events:?}"
            );
            miss.same_as(&cold);
            let (hit, events) = measure_c(&out, Some(&cache));
            assert!(
                events.iter().all(|e| e.outcome == CacheOutcome::Hit),
                "{events:?}"
            );
            hit.same_as(&cold);
            previous = (out, d, cold);
        }
        assert_eq!(std::fs::read(&one).expect("source bytes"), before);
        let _ = version;

        // The same distance again is a copy that differs only in its stamp.
        let out = f.root.path().join(format!("e{index}-same.fcad"));
        write(&request, &json!({"request_version":1,"distance_mm":2.375}));
        reply(
            edit_from(
                &one,
                &id.to_string(),
                &inspect(&one)["content_version"]
                    .as_str()
                    .expect("v")
                    .to_owned(),
                &request,
                &out,
            )
            .output()
            .expect("process"),
            OPE,
            0,
        );
        only_the_distance_changed(&one, &out, id);
    }
}

/// Refusals, races, cancellation and a lost report are atomic, for both
/// operations: a refused run leaves the directory as it found it and the
/// source byte for byte; a published run whose report cannot be written is
/// kept and exits 7.
#[test]
fn native_chamfer_refusals_races_cancellation_and_report_loss_are_atomic() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let before = std::fs::read(&f.source).expect("source bytes");
    let never = f.root.path().join("never.fcad");
    let corner = [X0, Y0];
    ask(&f, corner, 2.0);
    let taken = f.root.path().join("taken.fcad");
    std::fs::write(&taken, b"another process owns this").expect("occupied");
    let alias = f.root.path().join("alias.fcad");
    std::fs::hard_link(&f.source, &alias).expect("hard link");
    let names = entries(f.root.path());
    for (destination, why) in [
        (f.source.clone(), "the source is not its own output"),
        (taken.clone(), "an occupied output is not replaced"),
        (alias.clone(), "an alias of the source is the source"),
    ] {
        let v = reply(chamfer(&f, &destination).output().expect("process"), OPC, 2);
        assert_eq!(refused(&v), "input", "{why}: {v}");
    }
    assert_eq!(
        std::fs::read(&taken).expect("occupied"),
        b"another process owns this"
    );
    assert_eq!(entries(f.root.path()), names);

    // A stale version: the source changed after it was inspected.
    let stale = Fixture::drawn(Plate::new(CCW));
    let old_version = stale.version().to_owned();
    let mut d = Document::open(&stale.source).expect("writable");
    let first = d.objects().expect("objects").remove(0);
    d.write(|w| {
        w.put_object(
            first.id,
            first.parent,
            first.ordinal,
            Some("renamed after inspect"),
            &first.payload,
        )
        .map(|_| ())
    })
    .expect("change");
    d.close().expect("close");
    ask(&stale, corner, 2.0);
    let stale_out = stale.root.path().join("stale.fcad");
    let v = reply(
        chamfer_from(
            &stale,
            &stale.source,
            stale.body_id(),
            &old_version,
            &stale_out,
        )
        .output()
        .expect("process"),
        OPC,
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    assert!(!stale_out.exists());

    // Cancellation, before the job starts and at its last barrier, for the
    // creation and for the edit.
    let published = f.root.path().join("published.fcad");
    let (_, id, _) = chamfer_at(&f, &f.source, corner, 2.0, "published");
    let _ = &published;
    let copy = f.root.path().join("published.fcad");
    let names = entries(f.root.path());
    let edge = &at(&f, corner)["edge"];
    for at_fraction in [0.0, 0.95] {
        let destination = f.root.path().join(format!("cancelled-{at_fraction}.fcad"));
        let token = ferritecad_kernel::CancelToken::new();
        let stop = token.clone();
        if at_fraction == 0.0 {
            token.cancel();
        }
        let context = OperationContext::default()
            .with_cancel(token)
            .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                if fraction >= at_fraction {
                    stop.cancel();
                }
            }));
        let doc = Document::open_read_only(&f.source).expect("source");
        let expected = ferritecad_document::DocumentVersion {
            document_id: doc.meta().document_id,
            content: doc.content_version().expect("version"),
        };
        doc.close().expect("close");
        let request = ferritecad_jobs::EdgeChamferRequest {
            source: f.source.clone(),
            expected,
            body: uuid(&json!(f.body_id())),
            chamfer: ferritecad_document::EdgeChamfer {
                edge: ferritecad_document::SweptEdge {
                    feature: uuid(&edge["feature_id"]),
                    joint: ferritecad_types::ProfileJoint::new(
                        uuid_stable(&edge["joint"][0]),
                        uuid_stable(&edge["joint"][1]),
                    )
                    .expect("joint"),
                },
                distance_mm: 2.0,
            },
            destination: destination.clone(),
        };
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let result = ferritecad_jobs::chamfer_edge_copy(&request, &mut kernel, &context);
        assert!(result.is_err(), "cancelled at {at_fraction}: {result:?}");
        assert!(!destination.exists());
        assert_eq!(kernel.live_shape_count(), 0);

        // The edit of the published copy.
        let destination = f
            .root
            .path()
            .join(format!("cancelled-edit-{at_fraction}.fcad"));
        let token = ferritecad_kernel::CancelToken::new();
        let stop = token.clone();
        if at_fraction == 0.0 {
            token.cancel();
        }
        let context = OperationContext::default()
            .with_cancel(token)
            .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                if fraction >= at_fraction {
                    stop.cancel();
                }
            }));
        let doc = Document::open_read_only(&copy).expect("copy");
        let expected = ferritecad_document::DocumentVersion {
            document_id: doc.meta().document_id,
            content: doc.content_version().expect("version"),
        };
        doc.close().expect("close");
        let request = ferritecad_jobs::EditChamferDistanceRequest {
            source: copy.clone(),
            expected,
            feature: id,
            distance_mm: 3.0,
            destination: destination.clone(),
        };
        let result = ferritecad_jobs::edit_chamfer_distance_copy(&request, &mut kernel, &context);
        assert!(
            result.is_err(),
            "edit cancelled at {at_fraction}: {result:?}"
        );
        assert!(!destination.exists());
        assert_eq!(kernel.live_shape_count(), 0);
    }
    assert_eq!(entries(f.root.path()), names, "cancellation left something");
    assert_eq!(std::fs::read(&f.source).expect("source bytes"), before);

    // A stale version, an alias and an occupied output for the edit.
    let request = f.root.path().join("distance.json");
    write(&request, &json!({"request_version":1,"distance_mm":3.0}));
    let catalog = inspect(&copy);
    let version = catalog["content_version"]
        .as_str()
        .expect("version")
        .to_owned();
    let before_copy = std::fs::read(&copy).expect("bytes");
    let names = entries(f.root.path());
    for (destination, why) in [
        (copy.clone(), "the source is not its own output"),
        (taken.clone(), "an occupied output is not replaced"),
    ] {
        let v = reply(
            edit_from(&copy, &id.to_string(), &version, &request, &destination)
                .output()
                .expect("process"),
            OPE,
            2,
        );
        assert_eq!(refused(&v), "input", "{why}: {v}");
    }
    let v = reply(
        edit_from(&copy, &id.to_string(), &old_version, &request, &never)
            .output()
            .expect("process"),
        OPE,
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    assert_eq!(entries(f.root.path()), names);
    assert_eq!(std::fs::read(&copy).expect("bytes"), before_copy);

    // A success whose report cannot be written is published and says so by its
    // exit status; the file is kept, not rolled back.
    let lost = f.root.path().join("lost-report.fcad");
    ask(&f, corner, 2.0);
    assert_eq!(
        chamfer(&f, &lost)
            .stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
    assert!(lost.exists(), "the published copy is kept");
    let kept = inspect(&lost);
    assert_eq!(kept["chamfers"].as_array().map(Vec::len), Some(1));
    let lost_edit = f.root.path().join("lost-edit.fcad");
    assert_eq!(
        edit_from(&copy, &id.to_string(), &version, &request, &lost_edit)
            .stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
    assert!(lost_edit.exists());
    assert_eq!(std::fs::read(&f.source).expect("source bytes"), before);
    assert_eq!(std::fs::read(&copy).expect("bytes"), before_copy);
}

/// Every other editor, a second Chamfer and every feature after one refuse the
/// chamfered plate by name through the real commands: nothing is dropped,
/// ignored or rebuilt on a changed plate, and every source stays byte-identical.
#[test]
fn native_every_other_editor_refuses_a_chamfered_plate_by_name() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let (copy, id, _) = chamfer_at(&f, &f.source, [X0 + W, Y0], 2.0, "refused");
    let catalog = inspect(&copy);
    let version = catalog["content_version"]
        .as_str()
        .expect("version")
        .to_owned();
    let sketch = catalog["sketches"][0]["sketch_id"]
        .as_str()
        .expect("sketch")
        .to_owned();
    let base = catalog["bodies"][0]["chamfer_edge"].clone();
    assert_eq!(base["available"], false);
    let before = std::fs::read(&copy).expect("bytes");
    let out = f.root.path().join("never.fcad");
    let request = f.root.path().join("other.json");
    write(&request, &json!({}));
    let names = entries(f.root.path());
    let named = |why: &str, v: &Value| {
        assert!(matches!(refused(v), "unsupported" | "input"), "{why}: {v}");
        let message = v["error"]["message"].as_str().expect("message");
        assert!(
            message.contains("Chamfer") || message.contains(&id.to_string()),
            "{why}: {message}"
        );
    };
    // edit-extrude: the plate's height.
    let feature = catalog["features"][0]["feature_id"]
        .as_str()
        .expect("feature")
        .to_owned();
    let v = reply(
        cli()
            .arg("edit-extrude")
            .arg(&copy)
            .args([
                "--feature",
                &feature,
                "--distance-mm",
                "9",
                "--expect-version",
                &version,
            ])
            .arg("-o")
            .arg(&out)
            .arg("--json")
            .output()
            .expect("process"),
        "edit-extrude",
        2,
    );
    named("edit-extrude", &v);
    // edit-sketch-copy: the plate's coordinates.
    let vertices: Vec<Value> = f.catalog["sketches"][0]["vertices"]
        .as_array()
        .expect("vertices")
        .iter()
        .map(|p| json!({"curve_id": p["curve_id"], "start_mm": p["start_mm"]}))
        .collect();
    write(&request, &json!({"request_version":1,"vertices":vertices}));
    let v = reply(
        cli()
            .arg("edit-sketch-copy")
            .arg(&copy)
            .args(["--sketch", &sketch, "--expect-version", &version])
            .arg("--request")
            .arg(&request)
            .arg("-o")
            .arg(&out)
            .arg("--json")
            .output()
            .expect("process"),
        "edit-sketch-copy",
        2,
    );
    named("edit-sketch-copy", &v);
    // A Fillet on another corner, a second Chamfer and a Fillet radius edit of
    // the Chamfer's own UUID.
    let g = Fixture {
        root: tempfile::tempdir().expect("directory"),
        request: request.clone(),
        source: copy.clone(),
        catalog: catalog.clone(),
    };
    let body = g.body_id().to_owned();
    let any = f.at([X0, Y0])["edge"].clone();
    write(
        &request,
        &json!({"request_version":1,"edge":any,"radius_mm":1.0}),
    );
    let v = reply(
        g.fillet_from(&copy, &body, &version, &out)
            .output()
            .expect("process"),
        "fillet-edge-copy",
        2,
    );
    named("fillet-edge-copy", &v);
    write(
        &request,
        &json!({"request_version":1,"edge":any,"distance_mm":1.0}),
    );
    let v = reply(
        chamfer_from(&g, &copy, &body, &version, &out)
            .output()
            .expect("process"),
        OPC,
        2,
    );
    named("a second chamfer", &v);
    write(&request, &json!({"request_version":1,"radius_mm":1.0}));
    let v = reply(
        cli()
            .arg("edit-fillet-radius")
            .arg(&copy)
            .args(["--feature", &id.to_string(), "--expect-version", &version])
            .arg("--request")
            .arg(&request)
            .arg("-o")
            .arg(&out)
            .arg("--json")
            .output()
            .expect("process"),
        "edit-fillet-radius",
        2,
    );
    assert!(matches!(refused(&v), "unsupported" | "input"), "{v}");
    write(
        &request,
        &json!({"request_version":1,"center_mm":[10.0,8.0],"radius_mm":1.0,"depth_mm":1.0}),
    );
    let v = cli()
        .arg("cut-circular-copy")
        .arg(&copy)
        .args(["--body", &body, "--expect-version", &version])
        .arg("--request")
        .arg(&request)
        .arg("-o")
        .arg(&out)
        .arg("--json")
        .output()
        .expect("process");
    assert_eq!(v.status.code(), Some(2), "{v:?}");
    let v: Value = serde_json::from_slice(&v.stdout).expect("JSON");
    named("cut-circular-copy", &v);
    assert_eq!(
        entries(f.root.path()).len(),
        names.len(),
        "a refusal left something"
    );
    assert!(!out.exists());
    assert_eq!(std::fs::read(&copy).expect("bytes"), before);

    // Exports and rebuild are of the whole part: the chamfer is in the STL.
    let m = mesh(&copy, &f.root.path().join("whole.stl"));
    check_mesh_c(&m, RECT, [X0 + W, Y0], 2.0, H);
}

/// The evaluator refuses every saved Chamfer outside the class, at every
/// rebuild, cold or cached, with the numbers: never a partial plate. Documents
/// are forged through the document layer, as an older or hostile writer might.
#[test]
fn native_the_evaluator_refuses_a_saved_chamfer_outside_the_class() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let (copy, id, _) = chamfer_at(&f, &f.source, [X0, Y0], 2.0, "honest");
    let forge = |name: &str, change: &dyn Fn(&mut Document, ObjectId)| -> PathBuf {
        let path = f.root.path().join(format!("{name}.fcad"));
        std::fs::copy(&copy, &path).expect("copy");
        let mut d = Document::open(&path).expect("writable");
        change(&mut d, id);
        d.close().expect("close");
        path
    };
    let rewrite = |d: &mut Document, object: ObjectId, payload: ObjectPayload| {
        let record = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| o.id == object)
            .expect("object");
        d.write(|w| {
            w.put_object(
                record.id,
                record.parent,
                record.ordinal,
                record.name.as_deref(),
                &payload,
            )
            .map(|_| ())
        })
        .expect("forged");
    };
    let chamfer_of = |d: &Document, id: ObjectId| match d
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| o.id == id)
        .expect("chamfer")
        .payload
    {
        ObjectPayload::Chamfer(c) => c,
        other => panic!("{other:?}"),
    };
    let refused_everywhere = |why: &str, path: &Path| {
        for command in [vec!["rebuild", "--cold"], vec!["rebuild"]] {
            let out = cli()
                .arg(command[0])
                .arg(path)
                .args(&command[1..])
                .output()
                .expect("process");
            assert!(!out.status.success(), "{why} {command:?}: {out:?}");
        }
        let stl = f.root.path().join(format!("{}.stl", why.replace(' ', "-")));
        let out = cli()
            .arg("export-stl")
            .arg(path)
            .arg("-o")
            .arg(&stl)
            .arg("--json")
            .output()
            .expect("export");
        assert_eq!(out.status.code(), Some(2), "{why}: {out:?}");
        assert!(!stl.exists(), "{why}: a partial plate was exported");
    };
    // A distance past the bound a writer never checked.
    let wide = forge("wide", &|d, id| {
        let mut c = chamfer_of(d, id);
        c.distance_mm = 100.0;
        rewrite(d, id, ObjectPayload::Chamfer(c));
    });
    refused_everywhere("a distance past the bound", &wide);
    // A distance below the minimum.
    let thin = forge("thin", &|d, id| {
        let mut c = chamfer_of(d, id);
        c.distance_mm = 1e-9;
        rewrite(d, id, ObjectPayload::Chamfer(c));
    });
    refused_everywhere("a distance below the minimum", &thin);
    // A joint that is not a corner of this plate.
    let alien = forge("alien", &|d, id| {
        let mut c = chamfer_of(d, id);
        c.edge.joint = ferritecad_types::ProfileJoint::new(
            ferritecad_types::StableEntityId::new(),
            ferritecad_types::StableEntityId::new(),
        )
        .expect("joint");
        rewrite(d, id, ObjectPayload::Chamfer(c));
    });
    refused_everywhere("a joint of no corner", &alien);
    // The plate drawn narrower after the Chamfer was saved: the distance no
    // longer fits the Lines the plate is built from.
    let narrow = forge("narrow", &|d, _| {
        let sketch = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("sketch");
        let ObjectPayload::Sketch(mut s) = sketch.payload.clone() else {
            unreachable!()
        };
        for curve in &mut s.curves {
            if let ferritecad_document::SketchGeometry::Line { start, end } = &mut curve.geometry {
                for p in [start, end] {
                    p.y = Y0 + (p.y - Y0) * 0.1;
                }
            }
        }
        d.write(|w| {
            w.put_object(
                sketch.id,
                sketch.parent,
                sketch.ordinal,
                sketch.name.as_deref(),
                &ObjectPayload::Sketch(s),
            )
            .map(|_| ())
        })
        .expect("narrowed");
    });
    refused_everywhere("a plate too narrow for the distance", &narrow);
    // A second feature over the same plate: a Chamfer cutting it too.
    let branch = forge("branch", &|d, id| {
        let c = chamfer_of(d, id);
        let record = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| o.id == id)
            .expect("chamfer");
        let twin = ObjectId::new();
        let mut other = c.clone();
        other.distance_mm = 1.0;
        d.write(|w| {
            w.put_object(
                twin,
                None,
                record.ordinal + 10,
                Some("Twin"),
                &ObjectPayload::Chamfer(other),
            )?;
            w.add_dependency(ferritecad_document::Dependency {
                dependent: twin,
                dependency: c.previous,
                role: ferritecad_document::DependencyRole::Predecessor,
            })
        })
        .expect("branched");
    });
    refused_everywhere("two Chamfers over one plate", &branch);
    // Every forgery is also refused as an edit target, and none was repaired.
    for path in [&wide, &thin, &alien, &narrow, &branch] {
        let request = f.root.path().join("forged.json");
        write(&request, &json!({"request_version":1,"distance_mm":1.0}));
        let catalog = inspect(path);
        let version = catalog["content_version"]
            .as_str()
            .expect("version")
            .to_owned();
        let out = f.root.path().join("forged-out.fcad");
        let v = edit_from(path, &id.to_string(), &version, &request, &out)
            .output()
            .expect("process");
        assert!(!v.status.success() || out.exists() == false, "{v:?}");
        assert!(!out.exists(), "a forged document was edited");
    }
}
