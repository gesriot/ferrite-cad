// SPDX-License-Identifier: MIT
//! §26B: changing the numbers of a circular cut that is already in a history.
//!
//! What this slice has to keep is narrower and harder than what adding a cut
//! had to: every identity in the document already exists, so "the same body,
//! the same feature, the same circle" is checkable down to the UUID, and the
//! only things allowed to move are two payloads and — in one direction only —
//! one name a cut gains by stopping inside the part. Everything below is read
//! back: the geometry from the kernel, the solid from the exported bytes, the
//! identities and the SQL from the file.
#![allow(clippy::panic)]
#[path = "support/cut_facts.rs"]
mod cut_facts;
#[path = "support/names.rs"]
mod names;
#[path = "support/transaction.rs"]
mod transaction;
use crate::cut_facts::CutDepth;
use ferritecad_document::{Document, ObjectPayload, SketchGeometry};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    f64::consts::PI,
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;

const OP: &str = "edit";
const LINEAR_MM: f64 = 0.05;
const ANGULAR_RAD: f64 = 0.1;
/// Single-precision STL coordinates land a few parts in 10^7 from the number
/// they belong to; the tessellation's own chord budget is separate.
const ROUNDING_MM: f64 = 1e-4;

/// The acceptance part: 60 x 40 x 10 from the world origin.
const WIDTH: f64 = 60.0;
const DEPTH: f64 = 40.0;
const HEIGHT: f64 = 10.0;
/// The cut as it is saved, before any edit: r5 at (20, 15).
const CENTER: [f64; 2] = [20.0, 15.0];
const RADIUS: f64 = 5.0;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn reply(out: Output, operation: &str, code: i32) -> Value {
    assert_eq!(out.status.code(), Some(code), "{out:?}");
    assert_eq!(out.stdout.iter().filter(|&&b| b == b'\n').count(), 1);
    let v: Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["operation"], operation);
    assert_eq!(v["ok"], code == 0);
    v
}
fn inspect(path: &Path) -> Value {
    let out = cli()
        .arg("inspect")
        .arg(path)
        .arg("--json")
        .output()
        .expect("inspect");
    reply(out, "inspect", 0)["result"].clone()
}
fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).expect("JSON")).expect("request");
}
fn entries(path: &Path) -> Vec<std::ffi::OsString> {
    let mut v: Vec<_> = std::fs::read_dir(path)
        .expect("directory")
        .map(|e| e.expect("entry").file_name())
        .collect();
    v.sort();
    v
}
fn native() -> bool {
    if ferritecad_occt::is_available() {
        true
    } else {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        false
    }
}
/// Keeps a published artefact where a CI step can hand it to the pinned reader.
fn keep(path: &Path, name: &str) {
    let Some(dir) = std::env::var_os("FCAD_CUT_EDIT_ARTIFACTS") else {
        return;
    };
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).expect("artifact directory");
    std::fs::copy(path, dir.join(name)).expect("artifact");
}

/// A saved document holding one plate with one circular cut in it.
struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    request: PathBuf,
    catalog: Value,
}
impl Fixture {
    /// The part and the cut the shipped commands really make.
    fn built(depth: f64) -> Self {
        let root = tempfile::tempdir().expect("directory");
        let plate = root.path().join("плита space.fcad");
        let create = root.path().join("create.json");
        write(
            &create,
            &json!({"request_version":1,
                    "points_mm":[[0.0,0.0],[WIDTH,0.0],[WIDTH,DEPTH],[0.0,DEPTH]],
                    "height_mm":HEIGHT}),
        );
        let made = cli()
            .arg("create-sketch-extrude")
            .arg(&create)
            .arg("-o")
            .arg(&plate)
            .arg("--json")
            .output()
            .expect("create");
        assert!(made.status.success(), "{made:?}");
        let catalog = inspect(&plate);
        let cut = root.path().join("saved.fcad");
        let request = root.path().join("request.json");
        cut_facts::write_add(
            &plate,
            &request,
            cut_facts::CutSpec {
                center_mm: CENTER,
                radius_mm: RADIUS,
                extent: CutDepth::Blind { depth_mm: depth },
            },
        );
        let out = transaction::command(
            &plate,
            catalog["content_version"].as_str().expect("version"),
            &request,
            &cut,
        )
        .output()
        .expect("Cut");
        assert!(out.status.success(), "{out:?}");
        // Neither the plate nor the request it was cut with is part of what is
        // being edited; leaving them would make "a refusal added nothing" a
        // statement about a directory that already had spares in it.
        std::fs::remove_file(&plate).expect("the plate is not the fixture");
        std::fs::remove_file(&create).expect("nor its request");
        let catalog = inspect(&cut);
        Self {
            root,
            source: cut,
            request,
            catalog,
        }
    }
    /// The one editable cut, from the same reading everything else comes from.
    fn cut(&self) -> &Value {
        self.catalog["features"]
            .as_array()
            .expect("features")
            .iter()
            .find(|f| !f["previous_feature_id"].is_null())
            .expect("one editable cut")
    }
    fn saved(&self) -> cut_facts::CutObservation {
        cut_facts::observed(&self.source).remove(0)
    }
    fn feature_id(&self) -> &str {
        self.cut()["feature_id"].as_str().expect("feature UUID")
    }
    fn curve_id(&self) -> &str {
        self.catalog["sketches"]
            .as_array()
            .expect("sketches")
            .iter()
            .find(|s| s["sketch_id"] == self.cut()["profile_sketch_id"])
            .expect("tool")["curves"][0]["curve_id"]
            .as_str()
            .expect("curve")
    }
    fn version(&self) -> &str {
        self.catalog["content_version"].as_str().expect("version")
    }
    fn ask(&self, center: [f64; 2], radius: f64, depth: f64) {
        cut_facts::write_edit(
            &self.request,
            &self.saved(),
            cut_facts::CutSpec {
                center_mm: center,
                radius_mm: radius,
                extent: CutDepth::Blind { depth_mm: depth },
            },
        );
    }
    fn edit(&self, output: &Path) -> Command {
        self.edit_from(&self.source, self.feature_id(), self.version(), output)
    }
    fn edit_from(&self, source: &Path, _feature: &str, version: &str, output: &Path) -> Command {
        let mut c = cli();
        c.arg(OP)
            .arg(source)
            .arg("--expect-version")
            .arg(version)
            .arg("--request")
            .arg(&self.request)
            .arg("-o")
            .arg(output)
            .arg("--json");
        c
    }
    /// The cut as it is stored right now, moved to these numbers, published.
    fn published(&self, center: [f64; 2], radius: f64, depth: f64, name: &str) -> PathBuf {
        let out = self.root.path().join(format!("{name}.fcad"));
        self.ask(center, radius, depth);
        let reported = reply(self.edit(&out).output().expect("process"), OP, 0);
        let result = &reported["result"];
        assert_eq!(result["content_version"], inspect(&out)["content_version"]);
        let saved = self.saved();
        let now = cut_facts::observed(&out).remove(0);
        assert_eq!(
            (
                saved.feature,
                saved.tool_sketch,
                saved.tool_curve,
                saved.body,
                saved.previous_feature
            ),
            (
                now.feature,
                now.tool_sketch,
                now.tool_curve,
                now.body,
                now.previous_feature
            )
        );
        assert_eq!(
            (now.center_mm, now.radius_mm, now.extent),
            (center, radius, CutDepth::Blind { depth_mm: depth })
        );
        out
    }
}

/// What a document stores about the cut and everything it is made of.
struct Stored {
    objects: usize,
    /// Every object, by identity, with its whole record.
    by_id: BTreeMap<ferritecad_types::ObjectId, ferritecad_document::ObjectRecord>,
    features: Vec<(
        ferritecad_types::ObjectId,
        Option<ferritecad_types::ObjectId>,
        String,
    )>,
    tip: Option<ferritecad_types::ObjectId>,
    circles: BTreeMap<ferritecad_types::StableEntityId, ([f64; 2], f64)>,
    refs: crate::names::Names,
    dependencies: Vec<ferritecad_document::Dependency>,
}
fn stored(path: &Path) -> Stored {
    let d = Document::open_read_only(path).expect("reopen");
    let objects = d.objects().expect("objects");
    let mut features = Vec::new();
    let mut circles = BTreeMap::new();
    let mut tip = None;
    let mut by_id = BTreeMap::new();
    for object in &objects {
        by_id.insert(object.id, object.clone());
        match &object.payload {
            ObjectPayload::Extrude(feature) => {
                features.push((
                    object.id,
                    feature.previous,
                    format!("{:?}", feature.operation),
                ));
            }
            ObjectPayload::Sketch(sketch) => {
                for curve in &sketch.curves {
                    if let SketchGeometry::Circle { center, radius } = curve.geometry {
                        circles.insert(curve.id, ([center.x, center.y], radius));
                    }
                }
            }
            ObjectPayload::Body(b) => tip = b.tip_feature,
            _ => {}
        }
    }
    let refs = crate::names::snapshot(&d);
    let dependencies = d.dependencies().expect("dependencies");
    let count = objects.len();
    d.close().expect("close");
    Stored {
        objects: count,
        by_id,
        features,
        tip,
        circles,
        refs,
        dependencies,
    }
}

/// What the real kernel says about the solid a document rebuilds into.
struct Analytic {
    faces: u64,
    volume: f64,
    by_role: BTreeMap<String, Vec<ferritecad_kernel::FaceSurface>>,
}
fn analytic(path: &Path) -> Analytic {
    analytic_with_cache(path, None)
}
fn analytic_with_cache(
    path: &Path,
    cache: Option<(&Path, ferritecad_eval::CacheOutcome)>,
) -> Analytic {
    let doc = Document::open_read_only(path).expect("reopen");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = if let Some((cache_path, expected)) = cache {
        let mut store = ferritecad_document::CacheStore::open(
            cache_path,
            doc.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("cache in a new kernel session");
        let (built, events) = ferritecad_eval::rebuild_cached(
            &doc,
            &mut kernel,
            &mut store,
            &OperationContext::default(),
        )
        .expect("cached rebuild");
        assert!(
            events.iter().all(|e| e.outcome == expected),
            "every feature was expected to {expected:?}: {events:?}"
        );
        built
    } else {
        ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("cold rebuild after reopen")
    };
    let objects = doc.objects().expect("objects");
    let body = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("a body");
    let shape = built.shape(body.id).expect("the body was built");
    let mut by_role: BTreeMap<String, Vec<_>> = BTreeMap::new();
    let mut stats = None;
    for reference in crate::names::generated_faces(&doc, &built) {
        let resolved = &reference.entities;
        assert!(
            !resolved.is_empty(),
            "{:?} resolved to nothing",
            reference.seed.digest
        );
        let role = reference.label();
        let owner = format!("{role} of {}", reference.at);
        for handle in resolved {
            let surface = kernel.face_surface(*handle).expect("surface");
            by_role.entry(owner.clone()).or_default().push(surface);
            if handle.shape() == shape {
                stats = Some(kernel.shape_stats(shape).expect("stats"));
            }
        }
    }
    let (faces, volume) = stats.expect("some reference names the finished body");
    built.release_all(&mut kernel);
    doc.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0, "every handle was released");
    Analytic {
        faces,
        volume,
        by_role,
    }
}

/// An independent reading of the exported mesh.
struct Mesh {
    lo: [f64; 3],
    hi: [f64; 3],
    volume: f64,
    faces: Vec<[[f64; 3]; 3]>,
}
fn mesh(path: &Path, out: &Path) -> Mesh {
    let r = cli()
        .arg("export-stl")
        .arg(path)
        .arg("-o")
        .arg(out)
        .args(["--linear-deflection", &LINEAR_MM.to_string()])
        .args(["--angular-deflection", &ANGULAR_RAD.to_string()])
        .arg("--json")
        .output()
        .expect("STL");
    assert!(r.status.success(), "{r:?}");
    let report: Value = serde_json::from_slice(&r.stdout).expect("JSON");
    let b = std::fs::read(out).expect("STL bytes");
    let count = u32::from_le_bytes(b[80..84].try_into().expect("count")) as usize;
    assert_eq!(b.len(), 84 + 50 * count);
    assert_eq!(report["result"]["triangles"], count);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut six = 0.;
    let mut faces = Vec::new();
    for t in b[84..].chunks_exact(50) {
        let mut p = [[0.; 3]; 3];
        for (i, point) in p.iter_mut().enumerate() {
            for j in 0..3 {
                let k = 12 + 12 * i + 4 * j;
                point[j] = f64::from(f32::from_le_bytes(
                    t[k..k + 4].try_into().expect("coordinate"),
                ));
                assert!(point[j].is_finite(), "non-finite STL coordinate");
                lo[j] = lo[j].min(point[j]);
                hi[j] = hi[j].max(point[j]);
            }
        }
        faces.push(p);
        let [a, b, c] = p;
        six += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    assert!(count >= 24);
    Mesh {
        lo,
        hi,
        volume: six / 6.,
        faces,
    }
}

/// Everything the exported mesh must be for the part to really have *that* cut.
///
/// A mesh bore is inscribed, so it removes slightly less than the exact
/// cylinder; the volume is bounded on both sides rather than compared to an
/// analytic pi.
fn check_cut_mesh(m: &Mesh, center: [f64; 2], radius: f64, depth: f64) {
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

    // The bore wall: on the tool's circle and spanning the depth. A facet of a
    // pocket floor also has its corners on that circle, which is why "wall"
    // means the part of it that is not flat.
    let on_bore = |t: &[[f64; 3]; 3]| {
        t.iter().all(|v| {
            ((v[0] - center[0]).hypot(v[1] - center[1]) - radius).abs() < LINEAR_MM + ROUNDING_MM
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
    assert!(zs[0].abs() < ROUNDING_MM, "the cut does not start at z = 0");
    assert!(
        (zs[zs.len() - 1] - depth).abs() < ROUNDING_MM,
        "the bore runs to {} rather than {depth}",
        zs[zs.len() - 1]
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
    if depth < HEIGHT {
        assert!(
            m.faces.iter().any(|t| covers(t, depth)),
            "a pocket has a floor at its depth"
        );
        assert!(
            m.faces.iter().any(|t| covers(t, HEIGHT)),
            "a pocket leaves the far side closed"
        );
    } else {
        assert!(
            !m.faces.iter().any(|t| covers(t, HEIGHT)),
            "a through hole is open at the far side"
        );
    }

    // The part is still the part.
    assert!(m.lo[0].abs() < ROUNDING_MM && m.lo[1].abs() < ROUNDING_MM);
    assert!((m.hi[0] - WIDTH).abs() < ROUNDING_MM && (m.hi[1] - DEPTH).abs() < ROUNDING_MM);
    assert!((m.hi[2] - HEIGHT).abs() < ROUNDING_MM && m.lo[2].abs() < ROUNDING_MM);

    let exact = WIDTH * DEPTH * HEIGHT - PI * radius * radius * depth;
    let removed_lower = PI * (radius - LINEAR_MM).powi(2) * depth;
    assert!(
        m.volume >= exact - 1e-6 && m.volume <= WIDTH * DEPTH * HEIGHT - removed_lower + 1e-6,
        "STL volume {} is outside what a mesh of this part can be",
        m.volume
    );
}

/// Writes the finished part as FBX and keeps it for the pinned reader. One
/// finished body, not the tool and the intermediate solid beside it.
fn exported_fbx(copy: &Path, name: &str) {
    let fbx = copy.with_extension("fbx");
    let written = cli()
        .arg("export-fbx")
        .arg(copy)
        .arg("-o")
        .arg(&fbx)
        .arg("--json")
        .output()
        .expect("FBX");
    let report = reply(written, "export-fbx", 0);
    assert_eq!(report["result"]["complete"], json!(true));
    assert_eq!(
        report["result"]["geometries"],
        json!(1),
        "one finished body, not the tool and an intermediate"
    );
    keep(&fbx, name);
}

type Rows = Vec<Vec<rusqlite::types::Value>>;
fn tables(path: &Path) -> BTreeMap<String, (Vec<String>, Rows)> {
    let c = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("SQL");
    let names: Vec<String> = c
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .expect("names")
        .query_map([], |r| r.get(0))
        .expect("names")
        .collect::<rusqlite::Result<_>>()
        .expect("names");
    names
        .into_iter()
        .map(|name| {
            let quoted = format!("\"{}\"", name.replace('"', "\"\""));
            let mut stmt = c
                .prepare(&format!("SELECT rowid,* FROM {quoted} ORDER BY rowid"))
                .or_else(|_| c.prepare(&format!("SELECT * FROM {quoted} ORDER BY 1,2")))
                .expect("table");
            let n = stmt.column_count();
            let columns: Vec<String> = (0..n)
                .map(|i| stmt.column_name(i).expect("column").to_owned())
                .collect();
            let rows = stmt
                .query_map([], |r| (0..n).map(|i| r.get(i)).collect())
                .expect("rows")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("rows");
            (name, (columns, rows))
        })
        .collect()
}

/// Every SQL cell of the source survives into the copy except the ones this
/// operation is allowed to change.
///
/// The allow-list, stated once: the payload and payload hash of exactly the two
/// selected `objects` rows, the copy's own `meta.modified_at`, and — only when
/// the edit turns a hole back into a pocket — one newly derived floor name. No
/// row of any table may disappear, and nothing else may be altered.
fn only_the_numbers_moved(
    source: &Path,
    copy: &Path,
    edited: &[ferritecad_types::ObjectId],
    _added_refs: usize,
) {
    let before = tables(source);
    let after = tables(copy);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "the copy has the same tables"
    );
    let selected: Vec<rusqlite::types::Value> = edited
        .iter()
        .map(|id| rusqlite::types::Value::Blob(id.to_bytes().to_vec()))
        .collect();
    for (table, (columns, rows)) in &before {
        let (mine_columns, mine) = &after[table];
        assert_eq!(columns, mine_columns, "{table} columns");
        match table.as_str() {
            "objects" => {
                assert_eq!(mine.len(), rows.len(), "this edit adds no object");
                for row in rows {
                    let found = mine
                        .iter()
                        .find(|m| m[1] == row[1])
                        .unwrap_or_else(|| panic!("object {:?} disappeared", row[1]));
                    for (i, cell) in row.iter().enumerate() {
                        if cell == &found[i] {
                            continue;
                        }
                        assert!(
                            selected.iter().any(|s| row.contains(s))
                                && matches!(columns[i].as_str(), "payload" | "payload_hash"),
                            "objects.{} changed to {:?}",
                            columns[i],
                            found[i]
                        );
                    }
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
}

/// Everything about the edited copy that must be the source's, whatever the
/// numbers did. Returns the copy's own catalogue.
fn identities_survived(source: &Path, copy: &Path, added_faces: usize) -> Value {
    let before = stored(source);
    let after = stored(copy);
    assert_eq!(after.objects, before.objects, "an object appeared or left");
    assert_eq!(after.features, before.features, "the history changed");
    assert_eq!(after.tip, before.tip, "the body tips elsewhere");
    assert_eq!(after.dependencies, before.dependencies, "an edge moved");
    assert_eq!(
        after.circles.keys().collect::<Vec<_>>(),
        before.circles.keys().collect::<Vec<_>>(),
        "the tool circle lost its identity"
    );
    let faces = |names: &crate::names::Names| {
        names
            .iter()
            .copied()
            .filter(|(_, n)| n.kind == ferritecad_kernel::SubShapeKind::Face)
            .collect::<std::collections::BTreeSet<_>>()
    };
    let before_faces = faces(&before.refs);
    let after_faces = faces(&after.refs);
    assert_eq!(after_faces.len(), before_faces.len() + added_faces);
    assert!(
        before_faces.is_subset(&after_faces),
        "an existing face name changed"
    );
    if added_faces == 0 {
        assert_eq!(
            after.refs, before.refs,
            "unchanged topology preserves all names"
        );
    }
    let checked = cli()
        .arg("validate")
        .arg(copy)
        .arg("--json")
        .output()
        .expect("validate");
    let report = reply(checked, "validate", 0);
    assert_eq!(report["result"]["valid"], json!(true), "{report}");
    inspect(copy)
}

/// A publication whose report is lost is still a publication.
#[test]
fn native_a_lost_report_after_publication_leaves_the_copy_whole() {
    if !native() {
        return;
    }
    let f = Fixture::built(4.0);
    let out = f.root.path().join("delivered.fcad");
    f.ask([30.0, 20.0], 7.5, 6.0);
    assert_eq!(
        f.edit(&out)
            .stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
    assert!(out.exists(), "exit 7 must not undo the publication");
    let saved = cut_facts::observed(&out).remove(0);
    assert_eq!(saved.center_mm, [30., 20.]);
    assert_eq!(saved.radius_mm, 7.5);
    assert_eq!(saved.extent, CutDepth::Blind { depth_mm: 6. });
    assert_eq!(saved.tool_curve.to_string(), f.curve_id());
}

/// The measured slice: each number alone, then all three, then again on the
/// copy that was already edited once.
#[test]
fn native_the_centre_radius_and_depth_move_alone_and_together() {
    if !native() {
        return;
    }
    let f = Fixture::built(4.0);
    let before = std::fs::read(&f.source).expect("source bytes");
    let start = analytic(&f.source);
    assert_eq!(start.faces, 8, "a saved pocket");
    assert!(
        (start.volume - (WIDTH * DEPTH * HEIGHT - PI * RADIUS * RADIUS * 4.0)).abs() < 1e-6,
        "{}",
        start.volume
    );

    for (name, center, radius, depth) in [
        ("centre", [30.0, 20.0], RADIUS, 4.0),
        ("radius", CENTER, 7.5, 4.0),
        ("depth", CENTER, RADIUS, 6.5),
        ("all", [35.0, 22.5], 8.25, 2.75),
    ] {
        let copy = f.published(center, radius, depth, name);
        // Nothing but the two payloads moved, and no name was added: a pocket
        // that stays a pocket keeps exactly the references it had.
        let s = stored(&f.source);
        let tool = s
            .by_id
            .values()
            .find(|o| {
                matches!(&o.payload, ObjectPayload::Sketch(sk)
                    if sk.curves.iter().any(|c| matches!(c.geometry, SketchGeometry::Circle{..})))
            })
            .expect("the tool sketch")
            .id;
        let cut = s
            .features
            .iter()
            .find(|(_, previous, _)| previous.is_some())
            .expect("the cut")
            .0;
        only_the_numbers_moved(&f.source, &copy, &[tool, cut], 0);
        let catalog = identities_survived(&f.source, &copy, 0);
        let saved = cut_facts::observed(&copy).remove(0);
        assert_eq!(saved.center_mm, center);
        assert_eq!(saved.radius_mm, radius);
        assert_eq!(saved.extent, CutDepth::Blind { depth_mm: depth });

        // Cold: the real solid, measured by the kernel through the document's
        // own names.
        let a = analytic(&copy);
        assert_eq!(a.faces, 8, "{name}: a pocket has eight faces");
        let exact = WIDTH * DEPTH * HEIGHT - PI * radius * radius * depth;
        assert!(
            (a.volume - exact).abs() < 1e-6,
            "{name}: volume {} is not {exact}",
            a.volume
        );
        // The bore wall is a cylinder of the radius asked for, under the name
        // the tool's own circle gave it — the cut's own `side`, not the
        // planar sides the plate's extrusion named.
        let wall = a
            .by_role
            .get(&format!("curve of {}", f.feature_id()))
            .expect("the cut's own bore wall");
        assert!(
            wall.iter().any(|s| matches!(s,
                ferritecad_kernel::FaceSurface::Cylinder { radius: r } if (r - radius).abs() < 1e-9)),
            "{name}: the bore is not a cylinder of r{radius}: {wall:?}"
        );

        // And the exported solid really has that cut in it.
        let m = mesh(&copy, &copy.with_extension("stl"));
        check_cut_mesh(&m, center, radius, depth);
        exported_fbx(&copy, &format!("cut-edit-{name}-cli.fbx"));

        // Editing the already-edited copy again is the ordinary case, not a
        // special one: the identities are still the original document's.
        let again = Fixture {
            root: tempfile::tempdir().expect("directory"),
            source: copy.clone(),
            request: f.root.path().join(format!("{name}-again.json")),
            catalog: catalog.clone(),
        };
        let twice = f.root.path().join(format!("{name}-twice.fcad"));
        again.ask(CENTER, RADIUS, 4.0);
        let _reported = reply(again.edit(&twice).output().expect("process"), OP, 0);
        assert_eq!(
            cut_facts::observed(&copy)[0].feature.to_string(),
            f.feature_id()
        );
        assert_eq!(
            cut_facts::observed(&copy)[0].tool_curve.to_string(),
            f.curve_id()
        );
        identities_survived(&copy, &twice, 0);
        let back = analytic(&twice);
        assert!(
            (back.volume - start.volume).abs() < 1e-6,
            "editing back gives a different solid: {} vs {}",
            back.volume,
            start.volume
        );
    }
    assert_eq!(
        std::fs::read(&f.source).expect("source bytes"),
        before,
        "the source was touched"
    );
}

/// The pocket/through policy, in both directions and separately.
#[test]
fn native_holes_and_pockets_can_transition_when_no_feature_consumes_the_floor() {
    if !native() {
        return;
    }
    // A cut that already runs through the part: shortening it adds the floor's
    // name and loses nothing, which is why it is supported.
    let hole = Fixture::built(HEIGHT);
    assert!(!hole.saved().extent.leaves_a_floor(HEIGHT));
    assert!(
        crate::names::floor_names(
            &Document::open_read_only(&hole.source).expect("source"),
            hole.saved().feature
        )
        .is_empty()
    );
    let before = analytic(&hole.source);
    assert_eq!(before.faces, 7, "a through hole");

    let pocket = hole.published(CENTER, RADIUS, 3.0, "shortened");
    identities_survived(&hole.source, &pocket, 1);
    let s = stored(&hole.source);
    let tool = s
        .by_id
        .values()
        .find(|o| {
            matches!(&o.payload, ObjectPayload::Sketch(sk)
                if sk.curves.iter().any(|c| matches!(c.geometry, SketchGeometry::Circle{..})))
        })
        .expect("the tool sketch")
        .id;
    let cut = s
        .features
        .iter()
        .find(|(_, previous, _)| previous.is_some())
        .expect("the cut")
        .0;
    only_the_numbers_moved(&hole.source, &pocket, &[tool, cut], 1);
    let a = analytic(&pocket);
    assert_eq!(a.faces, 8, "a floor appeared");
    assert!(
        (a.volume - (WIDTH * DEPTH * HEIGHT - PI * RADIUS * RADIUS * 3.0)).abs() < 1e-6,
        "{}",
        a.volume
    );
    // The floor is named by the cut, and the part's own far side by the carried
    // name beside it. Two flat faces, two different meanings.
    assert!(
        a.by_role.keys().any(|role| role.starts_with("end cap of")),
        "the new floor has no name: {:?}",
        a.by_role.keys().collect::<Vec<_>>()
    );
    assert!(
        a.by_role
            .keys()
            .any(|role| role.starts_with("inherited end cap of")),
        "the part's far side lost its name"
    );
    check_cut_mesh(
        &mesh(&pocket, &pocket.with_extension("stl")),
        CENTER,
        RADIUS,
        3.0,
    );
    exported_fbx(&pocket, "cut-edit-shortened-cli.fbx");
    // Keeping it through is the other half of the same case.
    let still = hole.published([25.0, 18.0], 6.0, HEIGHT, "still-through");
    identities_survived(&hole.source, &still, 0);
    assert_eq!(analytic(&still).faces, 7);
    check_cut_mesh(
        &mesh(&still, &still.with_extension("stl")),
        [25.0, 18.0],
        6.0,
        HEIGHT,
    );

    let p = Fixture::built(4.0);
    assert!(
        !crate::names::floor_names(
            &Document::open_read_only(&p.source).expect("pocket"),
            p.saved().feature
        )
        .is_empty()
    );
    let through = p.published(CENTER, RADIUS, HEIGHT, "through");
    assert!(
        crate::names::floor_names(
            &Document::open_read_only(&through).expect("through"),
            p.saved().feature
        )
        .is_empty()
    );
    assert_eq!(analytic(&through).faces, 7);
    check_cut_mesh(
        &mesh(&through, &through.with_extension("stl")),
        CENTER,
        RADIUS,
        HEIGHT,
    );
    // Everything just short of the part's height is still accepted.
    p.published(CENTER, RADIUS, HEIGHT - 0.001, "nearly-through");
}

/// The edited cut is keyed by its own numbers, and the part it cuts by its own.
#[test]
fn native_the_edited_cut_is_keyed_by_its_own_numbers() {
    if !native() {
        return;
    }
    let f = Fixture::built(4.0);
    let copy = f.published([30.0, 20.0], 7.5, 6.0, "moved");
    let cache = copy.with_extension("fcad-cache");

    let rebuild = |expected: &[(&str, ferritecad_eval::CacheOutcome)]| -> (f64, usize) {
        let doc = Document::open_read_only(&copy).expect("reopen");
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let mut store = ferritecad_document::CacheStore::open(
            &cache,
            doc.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("cache");
        let (built, events) = ferritecad_eval::rebuild_cached(
            &doc,
            &mut kernel,
            &mut store,
            &OperationContext::default(),
        )
        .expect("cached rebuild");
        let objects = doc.objects().expect("objects");
        let named = |id: ferritecad_types::ObjectId| -> String {
            match &objects.iter().find(|o| o.id == id).expect("object").payload {
                ObjectPayload::Extrude(feature) if feature.previous.is_some() => "cut".to_owned(),
                ObjectPayload::Extrude(_) => "plate".to_owned(),
                _ => "other".to_owned(),
            }
        };
        let seen: Vec<(String, ferritecad_eval::CacheOutcome)> = events
            .iter()
            .map(|event| (named(event.feature), event.outcome))
            .collect();
        let want: Vec<(String, ferritecad_eval::CacheOutcome)> = expected
            .iter()
            .map(|(name, outcome)| ((*name).to_owned(), *outcome))
            .collect();
        assert_eq!(seen, want, "the cache did not do what it was expected to");
        let body = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
            .expect("a body");
        let shape = built.shape(body.id).expect("the body was built");
        let (_, volume) = kernel.shape_stats(shape).expect("stats");
        // Every name still resolves out of the cached archive, exactly as it
        // does cold.
        assert_eq!(crate::names::primary(&built), crate::names::snapshot(&doc));
        crate::names::assert_primary(&built);
        let resolved = crate::names::primary(&built).len();
        built.release_all(&mut kernel);
        doc.close().expect("close");
        (volume, resolved)
    };

    use ferritecad_eval::CacheOutcome::{Hit, Miss};
    let cold = analytic(&copy);
    let first = rebuild(&[("plate", Miss), ("cut", Miss)]);
    let warm = rebuild(&[("plate", Hit), ("cut", Hit)]);
    assert_eq!(first, warm, "a warm rebuild is a different solid");
    assert!((first.0 - cold.volume).abs() < 1e-9, "cold and warm differ");

    // Now edit only the tool, in place, in the document the cache belongs to.
    // The part above the cut is untouched, so its archive may be served; the
    // cut's may not, and the cavity it used to make must not come back.
    let mut document = Document::open(&copy).expect("opens for editing");
    let objects = document.objects().expect("objects");
    let (tool_id, mut sketch) = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Sketch(s)
                if s.curves
                    .iter()
                    .any(|c| matches!(c.geometry, SketchGeometry::Circle { .. })) =>
            {
                Some((o.id, s.clone()))
            }
            _ => None,
        })
        .expect("the tool sketch");
    let tool_row = objects.iter().find(|o| o.id == tool_id).expect("row");
    sketch.curves[0].geometry = SketchGeometry::Circle {
        center: ferritecad_document::Point2::new(30.0, 20.0).expect("centre"),
        radius: 3.0,
    };
    let (ordinal, name) = (tool_row.ordinal, tool_row.name.clone());
    document
        .write(|w| {
            w.put_object(
                tool_id,
                None,
                ordinal,
                name.as_deref(),
                &ObjectPayload::Sketch(sketch),
            )
        })
        .expect("narrows the tool");
    document.close().expect("closes");

    let narrowed = rebuild(&[("plate", Hit), ("cut", Miss)]);
    let exact = WIDTH * DEPTH * HEIGHT - PI * 3.0 * 3.0 * 6.0;
    assert!(
        (narrowed.0 - exact).abs() < 1e-6,
        "the old cavity came back out of the cache: {} rather than {exact}",
        narrowed.0
    );
    assert_eq!(
        narrowed.1, first.1,
        "a cached rebuild resolved a different number of faces than a cold one"
    );
    // And a cold rebuild of the same file agrees with the cached one.
    let cold_again = analytic(&copy);
    assert!((cold_again.volume - narrowed.0).abs() < 1e-9);
}

/// Every refusal a real filesystem can produce, with every other argument
/// correct, and nothing published by any of them.
#[test]
fn native_cut_edit_refusals_are_atomic() {
    if !native() {
        return;
    }
    let f = Fixture::built(4.0);
    let before = std::fs::read(&f.source).expect("source bytes");
    f.ask([30.0, 20.0], 7.5, 6.0);
    let names = entries(f.root.path());

    let taken = f.root.path().join("taken.fcad");
    std::fs::write(&taken, b"another process owns this").expect("occupied");
    let alias = f.root.path().join("alias.fcad");
    std::fs::hard_link(&f.source, &alias).expect("hard link");
    for (destination, why) in [
        (f.source.clone(), "the source is not its own output"),
        (taken.clone(), "an occupied output is not replaced"),
        (alias.clone(), "an alias of the source is the source"),
    ] {
        let refused = reply(f.edit(&destination).output().expect("process"), OP, 2);
        assert_eq!(refused["error"]["kind"], "input", "{why}: {refused:?}");
    }
    #[cfg(unix)]
    {
        let linked = f.root.path().join("linked.fcad");
        std::os::unix::fs::symlink(&f.source, &linked).expect("symlink");
        let refused = reply(f.edit(&linked).output().expect("process"), OP, 2);
        assert_eq!(refused["error"]["kind"], "input", "symlink: {refused:?}");
        std::fs::remove_file(linked).expect("tidy symlink");
    }
    assert_eq!(
        std::fs::read(&taken).expect("the occupied file"),
        b"another process owns this"
    );

    // JSON v1 needs UTF-8 paths, and says so rather than publishing to a name
    // it cannot report.
    #[cfg(unix)]
    let rough = {
        use std::os::unix::ffi::OsStrExt;
        f.root.path().join(std::ffi::OsStr::from_bytes(&[
            b'x', 0xff, b'.', b'f', b'c', b'a', b'd',
        ]))
    };
    #[cfg(windows)]
    let rough = {
        use std::os::windows::ffi::OsStringExt;
        f.root.path().join(std::ffi::OsString::from_wide(&[0xd800]))
    };
    let refused = reply(f.edit(&rough).output().expect("process"), OP, 2);
    assert!(
        refused["error"]["message"]
            .as_str()
            .expect("message")
            .contains("UTF-8"),
        "{refused}"
    );
    assert!(!rough.exists());

    for name in ["alias.fcad", "taken.fcad"] {
        std::fs::remove_file(f.root.path().join(name)).expect("tidy");
    }
    let mut expected = names;
    expected.retain(|name| {
        !["alias.fcad", "taken.fcad", "linked.fcad"].contains(&name.to_string_lossy().as_ref())
    });
    assert_eq!(entries(f.root.path()), expected, "a refusal left something");
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
}

/// Data this build did not write survives the edit, and a document that cannot
/// promise a bounded write is refused before one is attempted.
#[test]
fn native_unknown_tables_survive_and_a_stored_trigger_stops_the_edit() {
    if !native() {
        return;
    }
    let f = Fixture::built(4.0);
    {
        let c = rusqlite::Connection::open(&f.source).expect("SQL");
        c.execute_batch(
            "CREATE TABLE extra_data (id INTEGER PRIMARY KEY, payload BLOB);
             INSERT INTO extra_data VALUES (1, X'001122FF');

             ",
        )
        .expect("an extension table this build knows nothing about");
    }
    // The catalogue is read again: writing to the file changed its version.
    let f = Fixture {
        catalog: inspect(&f.source),
        ..f
    };
    let curve = f.curve_id().to_owned();
    let copy = f.published([30.0, 20.0], 7.5, 6.0, "with-extras");
    let s = stored(&f.source);
    let tool = s
        .by_id
        .values()
        .find(|o| {
            matches!(&o.payload, ObjectPayload::Sketch(sk)
                if sk.curves.iter().any(|c| matches!(c.geometry, SketchGeometry::Circle{..})))
        })
        .expect("the tool sketch")
        .id;
    let cut = s
        .features
        .iter()
        .find(|(_, previous, _)| previous.is_some())
        .expect("the cut")
        .0;
    // The whole-table comparison covers the extension table by construction;
    // asserting it is here so the reason it is covered is not an accident.
    only_the_numbers_moved(&f.source, &copy, &[tool, cut], 0);
    assert!(
        tables(&copy).contains_key("extra_data"),
        "the table vanished"
    );
    assert_eq!(
        tables(&copy)["extra_data"].1,
        tables(&f.source)["extra_data"].1
    );

    // A stored trigger is an unbounded write program. The document says so
    // before the operation, and the operation refuses rather than running it.
    {
        let c = rusqlite::Connection::open(&f.source).expect("SQL");
        c.execute_batch(
            "CREATE TRIGGER meddle AFTER UPDATE ON objects BEGIN
                 INSERT INTO extra_data(payload) VALUES (X'FF');
             END;",
        )
        .expect("a stored trigger");
    }
    let after = inspect(&f.source);
    let cut_row = after["features"]
        .as_array()
        .expect("features")
        .iter()
        .find(|x| x["feature_id"] == json!(cut.to_string()))
        .expect("the cut");
    assert_eq!(cut_row["profile_sketch_id"], json!(tool));
    assert_eq!(f.saved().tool_curve.to_string(), curve);
    f.ask([30., 20.], 7.5, 6.);
    let never = f.root.path().join("never.fcad");
    let version = after["content_version"]
        .as_str()
        .expect("version")
        .to_owned();
    let refused = reply(
        f.edit_from(&f.source, &cut.to_string(), &version, &never)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert_eq!(refused["error"]["kind"], json!("unsupported"), "{refused}");
    assert!(!never.exists());
}

/// A cut needs no solver at all: the part it cuts is unconstrained.
#[cfg(not(feature = "planegcs"))]
#[test]
fn occt_without_solver_edits_an_unconstrained_cut() {
    if !ferritecad_occt::is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    assert!(
        !ferritecad_eval::solver_available(),
        "mixed gate requires no solver"
    );
    let f = Fixture::built(4.0);
    let copy = f.published([30.0, 20.0], 7.5, 6.0, "no-solver");
    let a = analytic(&copy);
    assert_eq!(a.faces, 8);
    let exact = WIDTH * DEPTH * HEIGHT - PI * 7.5 * 7.5 * 6.0;
    assert!((a.volume - exact).abs() < 1e-6, "{}", a.volume);
    check_cut_mesh(
        &mesh(&copy, &copy.with_extension("stl")),
        [30.0, 20.0],
        7.5,
        6.0,
    );
}
