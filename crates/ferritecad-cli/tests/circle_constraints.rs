// SPDX-License-Identifier: MIT
//! §25N: a stored radius and a pinned centre on a saved analytic circle.
//!
//! Everything measured here is read back — the degrees of freedom from the
//! solver, the faces and the volume from the kernel, the shape of the solid
//! from the exported bytes — rather than compared against a number written down
//! in advance. The one fact that is asserted rather than measured is the one
//! this slice exists to keep: the stored sketch is still the starting guess it
//! always was, and the solved answer never quietly replaces it.
#![allow(clippy::panic)]
#[path = "support/transaction.rs"]
mod transaction;
use ferritecad_document::{Document, ObjectPayload, SemanticRole, SketchGeometry};
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
/// Single-precision STL coordinates land a few parts in 10^7 from the radius
/// they belong to; the tessellation's own chord budget is separate.
const ROUNDING_MM: f64 = 1e-4;

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
/// Whether this build has a solver, asked of the crate that owns one.
///
/// The CLI test binary does not depend on the solver crate directly — the
/// evaluator does — so this asks the same question through the shipped path a
/// request would take.
fn solver_available() -> bool {
    ferritecad_eval::solver_available()
}
fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (solver_available(), "PLANEGCS"),
    ])
}
/// Keeps a published artefact where a CI step can hand it to the pinned reader.
fn keep(path: &Path, name: &str) {
    let Some(dir) = std::env::var_os("FCAD_CIRCLE_CONSTRAINT_ARTIFACTS") else {
        return;
    };
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).expect("artifact directory");
    std::fs::copy(path, dir.join(name)).expect("artifact");
}

/// A saved §25J cylinder, written by the shipped creation command, and the one
/// accepted reading a request may take its identifiers from.
struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    request: PathBuf,
    catalog: Value,
}
impl Fixture {
    fn cylinder() -> Self {
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("цилиндр space.fcad");
        let create = root.path().join("create.json");
        write(
            &create,
            &json!({"schema_version":1,"center_mm":[12.0,-7.0],"radius_mm":10.0,
                    "height_mm":15.0}),
        );
        let made = cli()
            .arg("create-circle-extrude")
            .arg(&create)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(made.status.success(), "{made:?}");
        std::fs::remove_file(&create).expect("the request is not part of the fixture");
        let catalog = inspect(&source);
        let request = root.path().join("request.json");
        Self {
            root,
            source,
            request,
            catalog,
        }
    }
    /// The circle this profile is, as discovery names it.
    fn circle(&self) -> &Value {
        &self.catalog["sketches"][0]["curves"][0]
    }
    fn curve(&self) -> &str {
        self.circle()["curve_id"].as_str().expect("curve UUID")
    }
    fn ask(&self, remove: &[Value], add: &[Value]) {
        write(
            &self.request,
            &transaction::constraints(&self.catalog["sketches"][0], remove, add),
        );
    }
    fn edit(&self, output: &Path) -> Command {
        self.edit_from(&self.source, &self.catalog, output)
    }
    fn edit_from(&self, source: &Path, catalog: &Value, output: &Path) -> Command {
        transaction::sketch_address(&self.request, &catalog["sketches"][0]["sketch_id"]);
        transaction::command(
            source,
            catalog["content_version"].as_str().expect("version"),
            &self.request,
            output,
        )
    }
}
fn radius_add(curve: &str, mm: f64) -> Value {
    json!({"kind":"radius","curve":curve,"radius":transaction::number(mm,"mm")})
}
fn center_add(curve: &str, x: f64, y: f64) -> Value {
    json!({"kind":"fixed","point":transaction::point_ref(&json!(curve),"center"),"x":transaction::number(x,"mm"),"y":transaction::number(y,"mm")})
}

/// What a document stores about its one circle, and every identity beside it.
struct Stored {
    sketch: ferritecad_types::ObjectId,
    curve: ferritecad_types::StableEntityId,
    center: [f64; 2],
    radius: f64,
    height_mm: f64,
    constraints: Vec<(ferritecad_types::StableEntityId, String)>,
    refs: Vec<ferritecad_document::TopologyRef>,
    objects: usize,
}
fn stored(path: &Path) -> Stored {
    let d = Document::open_read_only(path).expect("reopen");
    let objects = d.objects().expect("objects");
    let row = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("sketch");
    let ObjectPayload::Sketch(sketch) = &row.payload else {
        unreachable!("Sketch")
    };
    let [curve] = sketch.curves.as_slice() else {
        panic!("one curve")
    };
    let SketchGeometry::Circle { center, radius } = curve.geometry else {
        panic!("the document stores a Circle")
    };
    let height_mm = match objects.iter().find_map(|o| match &o.payload {
        ObjectPayload::Extrude(e) => Some(e.end_condition.clone()),
        _ => None,
    }) {
        Some(ferritecad_document::EndCondition::Blind { distance }) => distance.value(),
        other => panic!("one Blind extrusion, got {other:?}"),
    };
    let constraints = sketch
        .constraints
        .iter()
        .map(|c| {
            (
                c.id,
                match c.rule.clone() {
                    ferritecad_document::SketchConstraintRule::Radius { radius, .. } => {
                        format!("radius {}", radius.value())
                    }
                    ferritecad_document::SketchConstraintRule::Fixed { point, x, y } => {
                        format!("fixed {} ({}, {})", point.at.as_str(), x.value(), y.value())
                    }
                    ref other => format!("{other:?}"),
                },
            )
        })
        .collect();
    let refs = d.topology_refs().expect("refs");
    let count = objects.len();
    d.close().expect("close");
    Stored {
        sketch: row.id,
        curve: curve.id,
        center: [center.x, center.y],
        radius,
        height_mm,
        constraints,
        refs,
        objects: count,
    }
}

/// What the real kernel says about the solid a document rebuilds into.
struct Analytic {
    faces: u64,
    volume: f64,
    side: Vec<ferritecad_kernel::FaceSurface>,
    caps: Vec<ferritecad_kernel::FaceSurface>,
    /// The circle UUID the side reference names.
    side_of: Option<ferritecad_types::StableEntityId>,
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
        assert_eq!(
            events.iter().map(|e| e.outcome).collect::<Vec<_>>(),
            vec![expected],
            "{events:?}"
        );
        built
    } else {
        ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("cold rebuild after reopen")
    };
    assert_eq!(built.shape_count(), 1);
    let mut side = Vec::new();
    let mut caps = Vec::new();
    let mut side_of = None;
    let mut stats = None;
    for reference in &doc.topology_refs().expect("refs") {
        let resolved = built.resolve(reference).expect("resolve");
        assert!(
            !resolved.is_empty(),
            "reference {:?} resolved to nothing",
            reference.output_role
        );
        for handle in &resolved {
            let surface = kernel.face_surface(*handle).expect("surface");
            match reference.output_role {
                SemanticRole::ExtrudeSide { profile_segment } => {
                    side_of = Some(profile_segment);
                    side.push(surface);
                }
                SemanticRole::ExtrudeCap { .. } => caps.push(surface),
                _ => panic!("unexpected role"),
            }
            stats = Some(kernel.shape_stats(handle.shape()).expect("stats"));
        }
    }
    let (faces, volume) = stats.expect("a resolved reference names the solid");
    built.release_all(&mut kernel);
    doc.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0, "every handle was released");
    Analytic {
        faces,
        volume,
        side,
        caps,
        side_of,
    }
}

/// The solid is the cylinder those numbers describe, under the original ref.
fn analytic_matches(
    a: &Analytic,
    curve: ferritecad_types::StableEntityId,
    radius: f64,
    height: f64,
) {
    assert_eq!(a.faces, 3, "one cylindrical side and two planar caps");
    assert_eq!(
        a.side,
        vec![ferritecad_kernel::FaceSurface::Cylinder { radius }],
        "the solved radius is the cylinder's"
    );
    assert_eq!(
        a.side_of,
        Some(curve),
        "under the original Circle's own ref"
    );
    assert_eq!(a.caps.len(), 2);
    assert!(
        a.caps
            .iter()
            .all(|s| *s == ferritecad_kernel::FaceSurface::Plane)
    );
    let exact = PI * radius * radius * height;
    assert!(
        (a.volume - exact).abs() < 1e-6 * exact,
        "analytic volume {} is not {exact}",
        a.volume
    );
}

/// An independent reading of the exported mesh.
struct Mesh {
    triangles: usize,
    lo: [f64; 3],
    hi: [f64; 3],
    volume: f64,
    points: Vec<[f64; 3]>,
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
    let mut points = Vec::new();
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
        points.extend(p);
        faces.push(p);
        let [a, b, c] = p;
        six += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    Mesh {
        triangles: count,
        lo,
        hi,
        volume: six / 6.,
        points,
        faces,
    }
}

/// Everything the exported mesh must be for the solid to be that cylinder.
///
/// Read out of the file's own triangles: that it closes and is wound one way,
/// that every rim vertex sits on the solved radius about the solved centre,
/// that the extents follow from those numbers, and that the signed volume
/// matches the area its own measured perimeter encloses. No analytic pi is
/// demanded of a mesh.
fn mesh_matches(m: &Mesh, center: [f64; 2], radius: f64, height: f64) {
    assert!(m.triangles >= 24);

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
        "a directed edge is used more than once; not one oriented surface"
    );
    for (from, to) in directed.keys() {
        assert!(
            directed.contains_key(&(*to, *from)),
            "an edge has no opposite; the mesh is open"
        );
    }

    let mut angles: Vec<_> = m
        .points
        .iter()
        .filter_map(|p| {
            let (x, y) = (p[0] - center[0], p[1] - center[1]);
            ((x.hypot(y) - radius).abs() < ROUNDING_MM).then_some(y.atan2(x).rem_euclid(2. * PI))
        })
        .collect();
    angles.sort_by(f64::total_cmp);
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    assert!(
        angles.len() >= 3,
        "no circular perimeter at the solved radius"
    );
    let mut area = 0.;
    for (i, angle) in angles.iter().enumerate() {
        let next = angles.get(i + 1).copied().unwrap_or(angles[0] + 2. * PI);
        let gap = next - angle;
        assert!(gap < PI, "the perimeter misses a sector");
        let sagitta = radius * (1. - (gap / 2.).cos());
        assert!(sagitta <= LINEAR_MM + ROUNDING_MM, "chord error {sagitta}");
        area += radius.powi(2) * gap.sin() / 2.;
    }
    let exact = PI * radius * radius * height;
    assert!(
        (m.volume - area * height).abs() <= exact * 1e-5,
        "STL volume {} does not match its own measured perimeter",
        m.volume
    );
    assert!(m.volume > 0. && m.volume <= exact * (1. + 1e-6));

    for (j, (middle, half)) in [
        (center[0], radius),
        (center[1], radius),
        (height / 2., height / 2.),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            m.hi[j] <= middle + half + ROUNDING_MM && m.lo[j] >= middle - half - ROUNDING_MM,
            "axis {j} extends past the solved geometry"
        );
        assert!(m.hi[j] - m.lo[j] >= 2. * half - 2. * LINEAR_MM - ROUNDING_MM);
    }
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

/// Every SQL cell of the copy equals the source's, except a stated few.
///
/// The allow-list is short on purpose: the selected Sketch's payload, its hash
/// and its schema header, the capability rows a circle constraint newly
/// requires, and the copy's own modified timestamp. Anything else that moved is
/// a cell this edit had no business touching — the stored circle geometry
/// included, which is why it is not on the list.
fn only_the_constraint_payload_changed(
    source: &Path,
    copy: &Path,
    sketch: ferritecad_types::ObjectId,
) {
    let before = tables(source);
    let after = tables(copy);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "the copy has the same tables"
    );
    let selected = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
    for (table, (columns, rows)) in &before {
        let (mine_columns, mine) = &after[table];
        assert_eq!(columns, mine_columns, "{table} columns");

        assert_eq!(rows.len(), mine.len(), "{table} row count");
        for (a, b) in rows.iter().zip(mine) {
            for (i, cell) in a.iter().enumerate() {
                if cell == &b[i] {
                    continue;
                }
                let column = columns[i].as_str();
                let allowed = match table.as_str() {
                    "objects" => {
                        a.contains(&selected) && matches!(column, "payload" | "payload_hash")
                    }
                    "meta" => column == "modified_at",
                    _ => false,
                };
                assert!(allowed, "{table}.{column} changed to {:?}", b[i]);
            }
        }
    }
}

#[test]
fn native_circle_radius_and_pinned_centre_drive_the_solid() {
    if !native() {
        return;
    }
    let f = Fixture::cylinder();
    let before = stored(&f.source);
    let bytes = std::fs::read(&f.source).expect("source");
    assert_eq!(before.center, [12., -7.]);
    assert_eq!(before.radius, 10.);
    assert_eq!(before.height_mm, 15.);

    // One request: a radius and a pinned centre, both about the stored circle.
    f.ask(
        &[],
        &[
            radius_add(f.curve(), 6.75),
            center_add(f.curve(), -3.5, 4.25),
        ],
    );
    let sized = f.root.path().join("sized.fcad");
    let v = reply(f.edit(&sized).output().expect("process"), OP, 0);
    assert_eq!(v["result"]["destination"], sized.to_str().expect("UTF-8"));

    // The solver's own answer about the structure, not a number written here.
    assert_eq!(
        transaction::solve(&sized)["degrees_of_freedom"],
        0,
        "a sized, pinned circle cannot move"
    );
    assert_eq!(
        transaction::solve(&sized)["redundant_constraint_ids"],
        json!([])
    );
    let listed = inspect(&sized);
    let added = listed["sketches"][0]["constraints"]
        .as_array()
        .expect("constraints");
    assert_eq!(added.len(), 2);
    assert_ne!(added[0]["constraint_id"], added[1]["constraint_id"]);
    assert_eq!(
        std::fs::read(&f.source).expect("source"),
        bytes,
        "the source was touched"
    );

    // The stored sketch is still the starting guess, and the constraints are
    // stored beside it with the centre selector.
    let after = stored(&sized);
    assert_eq!(after.curve, before.curve, "the circle keeps its UUID");
    assert_eq!(
        after.center, before.center,
        "the solved centre did not overwrite the stored guess"
    );
    assert_eq!(
        after.radius, before.radius,
        "the solved radius did not overwrite the stored guess"
    );
    assert_eq!(after.height_mm, 15.);
    assert_eq!(after.refs, before.refs, "same reference UUIDs and roles");
    assert_eq!(after.objects, before.objects);

    let mut kinds: Vec<_> = after.constraints.iter().map(|(_, k)| k.clone()).collect();
    kinds.sort();
    assert_eq!(kinds, ["fixed center (-3.5, 4.25)", "radius 6.75"]);
    only_the_constraint_payload_changed(&f.source, &sized, before.sketch);

    // The solid is what the solver decided, under the original circle's ref.
    analytic_matches(&analytic(&sized), before.curve, 6.75, 15.);
    let m = mesh(&sized, &sized.with_extension("stl"));
    mesh_matches(&m, [-3.5, 4.25], 6.75, 15.);

    let fbx = f.root.path().join("sized.fbx");
    let r = cli()
        .arg("export-fbx")
        .arg(&sized)
        .arg("-o")
        .arg(&fbx)
        .arg("--json")
        .output()
        .expect("FBX");
    assert!(r.status.success(), "{r:?}");
    let report: Value = serde_json::from_slice(&r.stdout).expect("JSON");
    assert_eq!(report["result"]["complete"], true);
    assert_eq!(
        std::fs::read(&fbx).expect("FBX").len() as u64,
        report["result"]["bytes"].as_u64().expect("bytes")
    );
    keep(&fbx, "circle-constraint-cli.fbx");

    // A real cache miss then hit on the solved geometry, and a different
    // radius is a different key rather than the entry already there.
    let cache = f.root.path().join("shared.fcad-cache");
    for outcome in [
        ferritecad_eval::CacheOutcome::Miss,
        ferritecad_eval::CacheOutcome::Hit,
    ] {
        analytic_matches(
            &analytic_with_cache(&sized, Some((&cache, outcome))),
            before.curve,
            6.75,
            15.,
        );
    }

    // Replacement: one remove and one add, atomically, keeping the pin and
    // every other UUID.
    let catalog = inspect(&sized);
    let stored_radius = catalog["sketches"][0]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .find(|c| c["rule"]["kind"] == "radius")
        .expect("the stored radius")
        .clone();
    let pin = catalog["sketches"][0]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .find(|c| c["rule"]["kind"] == "fixed")
        .expect("the stored pin")
        .clone();
    let replaced = f.root.path().join("replaced.fcad");
    f.ask(
        &[stored_radius["constraint_id"].clone()],
        &[radius_add(f.curve(), 8.125)],
    );
    let _v = reply(
        f.edit_from(&sized, &catalog, &replaced)
            .output()
            .expect("process"),
        OP,
        0,
    );
    assert_eq!(transaction::solve(&replaced)["degrees_of_freedom"], 0);

    let widened = stored(&replaced);
    assert_eq!(widened.curve, before.curve);
    assert_eq!(widened.center, before.center, "still the stored guess");
    assert_eq!(widened.radius, before.radius);
    assert!(
        widened
            .constraints
            .iter()
            .any(|(id, _)| id.to_string() == pin["constraint_id"].as_str().expect("id")),
        "the pin lost its identity in a radius replacement"
    );
    analytic_matches(&analytic(&replaced), before.curve, 8.125, 15.);
    // The same cache file, a different hole: a new key, not the old geometry.
    for outcome in [
        ferritecad_eval::CacheOutcome::Miss,
        ferritecad_eval::CacheOutcome::Hit,
    ] {
        analytic_matches(
            &analytic_with_cache(&replaced, Some((&cache, outcome))),
            before.curve,
            8.125,
            15.,
        );
    }
    std::fs::remove_file(&cache).expect("private cache closed and removed");

    // Removing both constraints hands the sketch back to the unconstrained
    // circle editor, and the geometry returns to the stored starting guess
    // rather than to the last radius the solver found.
    let freed = f.root.path().join("freed.fcad");
    let catalog = inspect(&replaced);
    let ids: Vec<Value> = catalog["sketches"][0]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .map(|c| c["constraint_id"].clone())
        .collect();
    assert_eq!(ids.len(), 2);
    f.ask(&ids, &[]);
    reply(
        f.edit_from(&replaced, &catalog, &freed)
            .output()
            .expect("process"),
        OP,
        0,
    );
    let plain = stored(&freed);
    assert!(plain.constraints.is_empty());

    assert_eq!(plain.center, [12., -7.]);
    assert_eq!(plain.radius, 10.);
    // The stored guess is what the geometry is again: this build makes no
    // promise to keep the last solved radius once the dimension is gone.
    analytic_matches(&analytic(&freed), before.curve, 10., 15.);
    // The existing height edit keeps the constraints and the solved radius.
    let extrude = {
        let d = Document::open_read_only(&sized).expect("reopen");
        let id = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
            .expect("extrude")
            .id;
        d.close().expect("close");
        id
    };
    let taller = f.root.path().join("taller.fcad");
    let r = transaction::constant(
        &sized,
        &Document::open_read_only(&sized)
            .expect("document")
            .content_version()
            .expect("version")
            .to_string(),
        &extrude.to_string(),
        "extrude_distance",
        "25",
        "mm",
        &taller,
    )
    .output()
    .expect("edit");
    assert!(r.status.success(), "{r:?}");
    let raised = stored(&taller);
    assert_eq!(raised.height_mm, 25.);
    assert_eq!(raised.constraints.len(), 2);
    assert_eq!(raised.curve, before.curve);
    analytic_matches(&analytic(&taller), before.curve, 6.75, 25.);
    mesh_matches(
        &mesh(&taller, &taller.with_extension("stl")),
        [-3.5, 4.25],
        6.75,
        25.,
    );

    assert_eq!(
        std::fs::read(&f.source).expect("source"),
        bytes,
        "the source is untouched by everything above"
    );
}

#[test]
fn native_circle_constraint_refusals_and_late_delivery_are_atomic() {
    if !native() {
        return;
    }
    let f = Fixture::cylinder();
    let bytes = std::fs::read(&f.source).expect("source");
    let out = f.root.path().join("copy.fcad");
    f.ask(&[], &[radius_add(f.curve(), 6.75)]);
    let names = entries(f.root.path());

    // A conflict is the solver's own answer, named against the caller's own
    // constraint UUIDs, and publishes nothing.
    f.ask(
        &[],
        &[radius_add(f.curve(), 6.75), radius_add(f.curve(), 8.125)],
    );
    let v = reply(f.edit(&out).output().expect("process"), OP, 2);
    assert!(
        matches!(
            v["error"]["kind"].as_str(),
            Some("input") | Some("unsupported") | Some("constraint")
        ),
        "{}",
        v["error"]
    );
    assert_eq!(std::fs::read(&f.source).expect("source"), bytes);
    assert_eq!(entries(f.root.path()), names);

    // Stale version, foreign sketch, occupied output, source as destination.
    f.ask(&[], &[radius_add(f.curve(), 6.75)]);
    let mut stale = f.catalog.clone();
    stale["content_version"] =
        json!("0000000000000000000000000000000000000000000000000000000000000000");
    let v = reply(
        f.edit_from(&f.source, &stale, &out).output().expect("p"),
        OP,
        2,
    );
    assert_eq!(v["error"]["kind"], "input");
    assert!(
        v["error"]["message"]
            .as_str()
            .expect("message")
            .contains("has changed"),
        "{}",
        v["error"]["message"]
    );
    let mut foreign = f.catalog.clone();
    foreign["sketches"][0]["sketch_id"] = json!("01a00000-0000-7000-8000-000000000000");
    reply(
        f.edit_from(&f.source, &foreign, &out).output().expect("p"),
        OP,
        2,
    );
    std::fs::write(&out, b"occupied").expect("occupied");
    reply(f.edit(&out).output().expect("process"), OP, 2);
    assert_eq!(std::fs::read(&out).expect("output"), b"occupied");
    std::fs::remove_file(&out).expect("clear");
    reply(f.edit(&f.source).output().expect("process"), OP, 2);

    // A hard link and a symlink to the source are the source.
    let alias = f.root.path().join("alias.fcad");
    std::fs::hard_link(&f.source, &alias).expect("hard link");
    reply(f.edit(&alias).output().expect("process"), OP, 2);
    std::fs::remove_file(&alias).expect("clear");
    #[cfg(unix)]
    {
        let link = f.root.path().join("symlink.fcad");
        std::os::unix::fs::symlink(&f.source, &link).expect("symlink");
        reply(f.edit(&link).output().expect("process"), OP, 2);
        std::fs::remove_file(&link).expect("clear");
    }

    assert_eq!(std::fs::read(&f.source).expect("source"), bytes);
    assert_eq!(entries(f.root.path()), names, "a refusal published");

    // Losing stdout after a real publication is late, not a rollback.
    let published = f.root.path().join("published.fcad");
    assert_eq!(
        f.edit(&published)
            .stdout(pipe::closed_pipe())
            .stderr(std::process::Stdio::piped())
            .output()
            .expect("closed stdout")
            .status
            .code(),
        Some(7)
    );
    let kept = stored(&published);
    assert_eq!(kept.constraints.len(), 1);
    analytic_matches(&analytic(&published), kept.curve, 6.75, 15.);
}

/// A real kernel without the optional solver is a distinct product build, not
/// the all-stub refusal path. Kept in the existing target with no features.
#[cfg(not(feature = "planegcs"))]
#[test]
fn occt_without_solver_builds_plain_circles_and_refuses_circle_constraints() {
    if !ferritecad_occt::is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    assert!(!solver_available(), "the mixed gate must not link a solver");
    let f = Fixture::cylinder();
    let original = stored(&f.source);
    analytic_matches(&analytic(&f.source), original.curve, 10., 15.);
    let source_bytes = std::fs::read(&f.source).expect("source bytes");
    f.ask(&[], &[radius_add(f.curve(), 6.75)]);
    let names = entries(f.root.path());
    let output = f.root.path().join("unsupported.fcad");
    let refused = reply(f.edit(&output).output().expect("mixed refusal"), OP, 2);
    assert_eq!(refused["error"]["kind"], "unsupported");
    assert!(
        refused["error"]["message"]
            .as_str()
            .expect("message")
            .contains("constraint")
    );
    assert_eq!(entries(f.root.path()), names, "no destination or scratch");
    assert_eq!(std::fs::read(&f.source).expect("source"), source_bytes);

    // Persistence is kernel-free. Install a real constraint through the
    // document API, then try to remove the last one through the CLI. Copy jobs
    // cold-rebuild the source before writing, so this still needs a solver:
    // the unchecked source must not bypass the baseline/ref validation.
    let mut document = Document::open(&f.source).expect("open");
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        original.sketch,
        &ferritecad_document::SketchConstraintEdits {
            remove: vec![],
            add: vec![ferritecad_document::AddSketchConstraint::Circle {
                curve: original.curve,
                kind: ferritecad_document::CircleConstraintKind::Radius(
                    ferritecad_document::CircleRadiusMm::new(6.75).expect("radius"),
                ),
            }],
        },
    )
    .expect("prepare");
    let radius_id = ferritecad_fixtures::added_constraints(&prepared)[0].id;
    document.write_changes(&prepared).expect("write");
    document.close().expect("close");
    let constrained_bytes = std::fs::read(&f.source).expect("constrained source");
    let catalog = inspect(&f.source);

    f.ask(&[json!(radius_id.to_string())], &[]);
    let plain = f.root.path().join("plain-again.fcad");
    let names = entries(f.root.path());
    let refused = reply(
        f.edit_from(&f.source, &catalog, &plain)
            .output()
            .expect("remove"),
        OP,
        2,
    );
    assert_eq!(refused["error"]["kind"], "unsupported");
    assert_eq!(
        entries(f.root.path()),
        names,
        "baseline refusal must not publish"
    );
    assert_eq!(std::fs::read(&f.source).expect("source"), constrained_bytes);

    // The earlier two-circle route also remains independent of the solver.
    let annulus = f.root.path().join("annulus.fcad");
    write(
        &f.request,
        &json!({"schema_version":1,"center_mm":[12.,-7.],
        "outer_radius_mm":10.,"inner_radius_mm":4.,"height_mm":15.}),
    );
    reply(
        cli()
            .arg("create-annular-extrude")
            .arg(&f.request)
            .arg("-o")
            .arg(&annulus)
            .arg("--json")
            .output()
            .expect("annulus"),
        "create-annular-extrude",
        0,
    );
    let a = analytic(&annulus);
    assert_eq!(a.faces, 4);
    assert!((a.volume - PI * (100. - 16.) * 15.).abs() < 1e-6);
}

fn diameter_add(curve: &str, mm: f64) -> Value {
    radius_add(curve, mm / 2.)
}

/// Every SQL cell of two published copies equals the other's, except the
/// selected Sketch's payload (the newly minted constraint UUIDs live in it) and
/// its hash, and the copy's own modified timestamp.
fn same_sql_but_the_sketch_payload(a: &Path, b: &Path, sketch: ferritecad_types::ObjectId) {
    let (first, second) = (tables(a), tables(b));
    assert_eq!(
        first.keys().collect::<Vec<_>>(),
        second.keys().collect::<Vec<_>>()
    );
    let selected = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
    let mut compared = 0;
    for (table, (columns, rows)) in &first {
        let (_, other) = &second[table];
        assert_eq!(rows.len(), other.len(), "{table} row count");
        for (x, y) in rows.iter().zip(other) {
            for (i, cell) in x.iter().enumerate() {
                compared += 1;
                if cell == &y[i] {
                    continue;
                }
                let column = columns[i].as_str();
                let allowed = match table.as_str() {
                    "objects" => {
                        x.contains(&selected) && matches!(column, "payload" | "payload_hash")
                    }
                    "meta" => column == "modified_at",
                    _ => false,
                };
                assert!(allowed, "{table}.{column} differs between the two copies");
            }
        }
    }
    assert!(
        compared > 50,
        "a comparison of {compared} cells is not a comparison"
    );
}

/// §31E, native: a diameter is the stored Radius `D/2` in the Radius slot, and
/// replacing or removing it is the existing exact-UUID operation.
#[test]
fn native_circle_diameter_is_the_stored_radius_in_the_same_slot() {
    if !native() {
        return;
    }
    let f = Fixture::cylinder();
    let source_bytes = std::fs::read(&f.source).expect("source");
    let before = stored(&f.source);
    let curve = f.curve().to_owned();

    // The same circle sized by a diameter and by the radius that is its half.
    let by_diameter = f.root.path().join("by-diameter.fcad");
    let by_radius = f.root.path().join("by-radius.fcad");
    f.ask(
        &[],
        &[diameter_add(&curve, 13.5), center_add(&curve, -3.5, 4.25)],
    );
    reply(f.edit(&by_diameter).output().expect("process"), OP, 0);
    f.ask(
        &[],
        &[radius_add(&curve, 6.75), center_add(&curve, -3.5, 4.25)],
    );
    reply(f.edit(&by_radius).output().expect("process"), OP, 0);

    let left = inspect(&by_diameter);
    let right = inspect(&by_radius);
    assert_eq!(
        left["sketches"][0]["constraints"][0]["rule"],
        right["sketches"][0]["constraints"][0]["rule"]
    );
    assert_eq!(
        transaction::solve(&by_diameter),
        transaction::solve(&by_radius)
    );
    assert_eq!(transaction::solve(&by_diameter)["degrees_of_freedom"], 0);
    assert_eq!(std::fs::read(&f.source).expect("source"), source_bytes);

    // Stored: the same Radius under the same circle, and nothing else moved.
    let (sd, sr) = (stored(&by_diameter), stored(&by_radius));
    let texts = |s: &Stored| {
        let mut t: Vec<_> = s.constraints.iter().map(|(_, k)| k.clone()).collect();
        t.sort();
        t
    };
    assert_eq!(texts(&sd), ["fixed center (-3.5, 4.25)", "radius 6.75"]);
    assert_eq!(texts(&sd), texts(&sr));
    for s in [&sd, &sr] {
        assert_eq!(s.curve, before.curve);
        assert_eq!((s.center, s.radius), (before.center, before.radius));
        assert_eq!(s.refs, before.refs, "same reference UUIDs and roles");
    }
    let minted: std::collections::BTreeSet<_> = sd
        .constraints
        .iter()
        .chain(&sr.constraints)
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(minted.len(), 4, "each publication minted its own UUIDs");
    only_the_constraint_payload_changed(&f.source, &by_diameter, before.sketch);
    same_sql_but_the_sketch_payload(&by_diameter, &by_radius, before.sketch);
    analytic_matches(&analytic(&by_diameter), before.curve, 6.75, 15.);
    mesh_matches(
        &mesh(&by_diameter, &by_diameter.with_extension("stl")),
        [-3.5, 4.25],
        6.75,
        15.,
    );

    let never = f.root.path().join("never.fcad");
    let names = entries(f.root.path());
    // A stored Radius also occupies it against a later diameter.
    let catalog = inspect(&by_diameter);
    let stored_constraints = catalog["sketches"][0]["constraints"]
        .as_array()
        .expect("constraints")
        .clone();
    let find = |kind: &str| {
        stored_constraints
            .iter()
            .find(|c| c["rule"]["kind"] == kind)
            .unwrap_or_else(|| panic!("no stored {kind}"))["constraint_id"]
            .clone()
    };
    let (radius_id, pin_id) = (find("radius"), find("fixed"));
    f.ask(&[], &[diameter_add(&curve, 9.)]);
    reply(
        f.edit_from(&by_diameter, &catalog, &never)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert_eq!(entries(f.root.path()), names);

    // Replacement: remove exactly the stored Radius and add a diameter, in one
    // atomic request; the pin keeps its identity.
    let replaced = f.root.path().join("replaced.fcad");
    f.ask(
        std::slice::from_ref(&radius_id),
        &[diameter_add(&curve, 17.)],
    );
    let _v = reply(
        f.edit_from(&by_diameter, &catalog, &replaced)
            .output()
            .expect("process"),
        OP,
        0,
    );

    assert_eq!(transaction::solve(&replaced)["degrees_of_freedom"], 0);
    let wide = stored(&replaced);
    assert_eq!(texts(&wide), ["fixed center (-3.5, 4.25)", "radius 8.5"]);
    assert!(
        wide.constraints
            .iter()
            .any(|(id, _)| id.to_string() == pin_id.as_str().expect("id"))
    );
    assert!(
        wide.constraints
            .iter()
            .all(|(id, _)| id.to_string() != radius_id.as_str().expect("id")),
        "the replaced Radius kept its UUID"
    );
    assert_eq!((wide.center, wide.radius), (before.center, before.radius));
    analytic_matches(&analytic(&replaced), before.curve, 8.5, 15.);

    // Removal of that Radius by its UUID alone hands the circle back to the
    // stored guess, still pinned.
    let catalog = inspect(&replaced);
    let new_radius = catalog["sketches"][0]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .find(|c| c["rule"]["kind"] == "radius")
        .expect("the replacement Radius")["constraint_id"]
        .clone();
    let freed = f.root.path().join("freed.fcad");
    f.ask(&[new_radius], &[]);
    reply(
        f.edit_from(&replaced, &catalog, &freed)
            .output()
            .expect("process"),
        OP,
        0,
    );
    let rest = stored(&freed);
    assert_eq!(texts(&rest), ["fixed center (-3.5, 4.25)"]);
    analytic_matches(&analytic(&freed), before.curve, 10., 15.);
    assert_eq!(std::fs::read(&f.source).expect("source"), source_bytes);
}
