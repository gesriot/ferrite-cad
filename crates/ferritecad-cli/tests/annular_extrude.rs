// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::{
    CapSide, Document, ObjectPayload, SemanticRole, SketchGeometry, TopologyRef,
};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    f64::consts::PI,
    path::Path,
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;
const OP: &str = "create-annular-extrude";
const LINEAR_MM: f64 = 0.05;
const ANGULAR_RAD: f64 = 0.1;
/// How far a measured coordinate may sit from the radius it belongs to.
///
/// Reading, not tessellation: STL stores single-precision floats, so a vertex
/// exactly on a 10 mm circle comes back a few parts in 10^7 away from it. The
/// chord error the tessellation is allowed is a separate budget, checked
/// separately below.
const ROUNDING_MM: f64 = 1e-4;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).expect("JSON")).expect("request");
}
fn request(path: &Path) {
    write(
        path,
        &json!({"schema_version":1,"center_mm":[12.0,-7.0],"outer_radius_mm":10.0,
                "inner_radius_mm":4.0,"height_mm":15.0}),
    );
}
fn create(input: &Path, out: &Path) -> Command {
    let mut c = cli();
    c.arg(OP).arg(input).arg("-o").arg(out).arg("--json");
    c
}
fn reply(out: Output, code: i32) -> Value {
    assert_eq!(out.status.code(), Some(code), "{out:?}");
    assert_eq!(out.stdout.iter().filter(|&&b| b == b'\n').count(), 1);
    let v: Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["operation"], OP);
    assert_eq!(v["ok"], code == 0);
    v
}
fn entries(p: &Path) -> Vec<std::ffi::OsString> {
    let mut v: Vec<_> = std::fs::read_dir(p)
        .expect("directory")
        .map(|e| e.expect("entry").file_name())
        .collect();
    v.sort();
    v
}
/// Keeps a published artefact where a CI step can hand it to the pinned reader.
fn keep(path: &Path, name: &str) {
    let Some(dir) = std::env::var_os("FCAD_ANNULUS_ARTIFACTS") else {
        return;
    };
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).expect("artifact directory");
    std::fs::copy(path, dir.join(name)).expect("artifact");
}
fn native() -> bool {
    if ferritecad_occt::is_available() {
        true
    } else {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: no OCCT for analytic annular geometry");
        false
    }
}

/// The two stored circles of a published document, and every identity beside
/// them.
///
/// Both circles are read out and kept apart by radius, which is the same rule
/// the evaluator applies. Nothing here trusts the order they are stored in.
struct Stored {
    outer: (ferritecad_types::StableEntityId, [f64; 2], f64),
    inner: (ferritecad_types::StableEntityId, [f64; 2], f64),
    /// Both curves in the order the Sketch actually stores them.
    stored_order: Vec<ferritecad_types::StableEntityId>,
    refs: Vec<TopologyRef>,
    objects: usize,
}
fn stored(path: &Path) -> Stored {
    let doc = Document::open_read_only(path).expect("reopen");
    let objects = doc.objects().expect("objects");
    let sketch = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Sketch(s) => Some(s),
            _ => None,
        })
        .expect("sketch");
    assert_eq!(sketch.curves.len(), 2, "two model curves, not a polygon");
    assert!(sketch.constraints.is_empty());
    let mut circles = Vec::new();
    for curve in &sketch.curves {
        assert!(!curve.construction, "both boundaries are model geometry");
        let SketchGeometry::Circle { center, radius } = curve.geometry else {
            panic!("the document stores Circles, not an approximation of them")
        };
        circles.push((curve.id, [center.x, center.y], radius));
    }
    assert_ne!(circles[0].0, circles[1].0, "two curves, two identities");
    let stored_order = circles.iter().map(|c| c.0).collect();
    circles.sort_by(|a, b| b.2.total_cmp(&a.2));
    let refs = doc.topology_refs().expect("refs");
    let count = objects.len();
    doc.close().expect("close");
    Stored {
        outer: circles[0],
        inner: circles[1],
        stored_order,
        refs,
        objects: count,
    }
}

/// What the real kernel says about the solid this document rebuilds into.
///
/// Analytic throughout: a face count, a B-Rep volume and the surface each named
/// face lies on, with each wall read through the reference that carries its own
/// circle's UUID. None of it comes from a mesh.
struct Analytic {
    faces: u64,
    volume: f64,
    /// The surface under the reference naming each circle, by that circle.
    walls: BTreeMap<ferritecad_types::StableEntityId, Vec<ferritecad_kernel::FaceSurface>>,
    caps: Vec<ferritecad_kernel::FaceSurface>,
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
    let built = if let Some((path, expected)) = cache {
        let mut cache = ferritecad_document::CacheStore::open(
            path,
            doc.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("cache in a new kernel session");
        let (built, events) = ferritecad_eval::rebuild_cached(
            &doc,
            &mut kernel,
            &mut cache,
            &OperationContext::default(),
        )
        .expect("cached annular rebuild");
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
    let refs = doc.topology_refs().expect("refs");
    let mut walls: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut caps = Vec::new();
    let mut stats = None;
    for reference in &refs {
        let resolved = built.resolve(reference).expect("resolve");
        assert!(
            !resolved.is_empty(),
            "reference {:?} resolved to nothing after a cold reopen",
            reference.output_role
        );
        for handle in &resolved {
            let surface = kernel.face_surface(*handle).expect("surface");
            match reference.output_role {
                // Filed under the circle the reference names, so which wall is
                // which is the document's answer rather than a face index.
                SemanticRole::ExtrudeSide { profile_segment } => {
                    walls.entry(profile_segment).or_default().push(surface)
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
        walls,
        caps,
    }
}

/// Everything the analytic solid must be, checked against the two radii.
fn check_analytic(a: &Analytic, s: &Stored, outer: f64, inner: f64, height: f64) {
    assert_eq!(a.faces, 4, "two cylindrical walls and two planar caps");
    assert_eq!(
        a.walls.get(&s.outer.0).map(Vec::as_slice),
        Some([ferritecad_kernel::FaceSurface::Cylinder { radius: outer }].as_slice()),
        "the outer circle's own reference names the outer wall"
    );
    assert_eq!(
        a.walls.get(&s.inner.0).map(Vec::as_slice),
        Some([ferritecad_kernel::FaceSurface::Cylinder { radius: inner }].as_slice()),
        "the inner circle's own reference names the bore"
    );
    assert_eq!(a.caps.len(), 2);
    assert!(
        a.caps
            .iter()
            .all(|s| *s == ferritecad_kernel::FaceSurface::Plane)
    );
    let exact = PI * (outer.powi(2) - inner.powi(2)) * height;
    assert!(
        (a.volume - exact).abs() < 1e-6 * exact,
        "analytic volume {} is not the annulus {exact}; a lost hole would be {}",
        a.volume,
        PI * outer.powi(2) * height
    );
}

/// An independent reading of the exported mesh.
struct Mesh {
    triangles: usize,
    lo: [f64; 3],
    hi: [f64; 3],
    /// Signed, from the winding the file actually carries.
    volume: f64,
    points: Vec<[f64; 3]>,
    /// Every triangle as three corners, for the closure and orientation walk.
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

/// The angles at which the mesh actually touches one radius about one centre.
fn perimeter(m: &Mesh, center: [f64; 2], radius: f64) -> Vec<f64> {
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
    angles
}

/// The area a measured perimeter encloses, and its worst chord error.
fn enclosed(angles: &[f64], radius: f64) -> (f64, f64) {
    assert!(angles.len() >= 3, "no circular perimeter in the actual STL");
    let mut area = 0.;
    let mut worst: f64 = 0.;
    for (i, angle) in angles.iter().enumerate() {
        let next = angles.get(i + 1).copied().unwrap_or(angles[0] + 2. * PI);
        let gap = next - angle;
        assert!(gap < PI, "the perimeter at radius {radius} misses a sector");
        worst = worst.max(radius * (1. - (gap / 2.).cos()));
        area += radius.powi(2) * gap.sin() / 2.;
    }
    (area, worst)
}

/// Everything the exported mesh must be for the part to really be hollow.
///
/// Read out of the file's own triangles: that it closes and is wound one way,
/// that both walls are there at their own radii, that the bore's facets face
/// the axis while the outside's face away, that no cap triangle covers the
/// axis, and that the volume the winding gives is the annulus rather than the
/// full cylinder. A triangle count, a bounding box or the export report could
/// not tell a tube from a rod.
fn check_annular_mesh(m: &Mesh, center: [f64; 2], outer: f64, inner: f64, height: f64) {
    assert!(m.triangles >= 48);

    // Closed and consistently oriented: every directed edge once, so every
    // undirected edge twice with opposite senses. An open shell or a flipped
    // facet makes the signed volume below meaningless.
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

    // Both perimeters exist at their own radii and within the chord budget.
    let (outer_area, outer_error) = enclosed(&perimeter(m, center, outer), outer);
    let (inner_area, inner_error) = enclosed(&perimeter(m, center, inner), inner);
    assert!(
        outer_error <= LINEAR_MM + ROUNDING_MM,
        "outer {outer_error}"
    );
    assert!(
        inner_error <= LINEAR_MM + ROUNDING_MM,
        "inner {inner_error}"
    );

    // The volume the winding gives is the measured annulus, not the disk.
    let exact = PI * (outer.powi(2) - inner.powi(2)) * height;
    assert!(
        (m.volume - (outer_area - inner_area) * height).abs() <= exact * 1e-4,
        "STL volume {} does not match its own two measured perimeters",
        m.volume
    );
    // And it is below the analytic annulus, because an inscribed prism is.
    let full = PI * outer.powi(2) * height;
    assert!(m.volume > 0. && m.volume <= exact * (1. + 1e-6));
    assert!(
        m.volume < full * 0.999,
        "volume {} is a solid cylinder, so the hole is not in the mesh",
        m.volume
    );

    // The bore's facets face the axis and the outside's face away from it.
    let on = |t: &[[f64; 3]; 3], radius: f64| {
        t.iter().all(|v| {
            ((v[0] - center[0]).hypot(v[1] - center[1]) - radius).abs() < LINEAR_MM + ROUNDING_MM
        })
    };
    let radial = |t: &[[f64; 3]; 3]| {
        let [a, b, c] = t;
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2]];
        let mx = (a[0] + b[0] + c[0]) / 3. - center[0];
        let my = (a[1] + b[1] + c[1]) / 3. - center[1];
        n[0] * mx + n[1] * my
    };
    let bore: Vec<_> = m.faces.iter().filter(|t| on(t, inner)).collect();
    let skin: Vec<_> = m.faces.iter().filter(|t| on(t, outer)).collect();
    assert!(bore.len() >= 12, "the mesh has no inner wall at all");
    assert!(skin.len() >= 12, "the mesh has no outer wall at all");
    assert!(
        bore.iter().all(|t| radial(t) < 0.),
        "a bore facet faces away from the axis; the cavity is inside out"
    );
    assert!(
        skin.iter().all(|t| radial(t) > 0.),
        "an outer facet faces the axis"
    );

    // Nothing covers the axis, so the hole goes through rather than being a
    // ring drawn on a solid cap.
    let covers = |t: &[[f64; 3]; 3], px: f64, py: f64| {
        let [(x1, y1), (x2, y2), (x3, y3)] =
            [(t[0][0], t[0][1]), (t[1][0], t[1][1]), (t[2][0], t[2][1])];
        let d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3);
        if d.abs() < 1e-12 {
            return false;
        }
        let a = ((y2 - y3) * (px - x3) + (x3 - x2) * (py - y3)) / d;
        let b = ((y3 - y1) * (px - x3) + (x1 - x3) * (py - y3)) / d;
        a >= -1e-9 && b >= -1e-9 && 1. - a - b >= -1e-9
    };
    assert!(
        !m.faces.iter().any(|t| covers(t, center[0], center[1])),
        "a triangle covers the axis, so the bore is not open"
    );

    for (j, (middle, half)) in [
        (center[0], outer),
        (center[1], outer),
        (height / 2., height / 2.),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(m.hi[j] <= middle + half + ROUNDING_MM && m.lo[j] >= middle - half - ROUNDING_MM);
        assert!(m.hi[j] - m.lo[j] >= 2. * half - 2. * LINEAR_MM - ROUNDING_MM);
    }
}

/// Every table of a document, as rows, for comparing two publications.
fn tables(path: &Path) -> BTreeMap<String, Vec<Vec<rusqlite::types::Value>>> {
    let c = rusqlite::Connection::open(path).expect("SQL");
    let names: Vec<String> = c
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .expect("names")
        .query_map([], |r| r.get(0))
        .expect("names")
        .collect::<std::result::Result<_, _>>()
        .expect("names");
    names
        .into_iter()
        .map(|name| {
            let quoted = format!("\"{}\"", name.replace('"', "\"\""));
            let mut stmt = c
                .prepare(&format!("SELECT * FROM {quoted} ORDER BY 1,2"))
                .expect("table");
            let n = stmt.column_count();
            let rows = stmt
                .query_map([], |r| (0..n).map(|i| r.get(i)).collect())
                .expect("rows")
                .collect::<std::result::Result<Vec<_>, _>>()
                .expect("rows");
            (name, rows)
        })
        .collect()
}

/// Rewrites the stored Sketch of a published document.
///
/// Test preparation, deliberately outside the shipped writers: this slice adds
/// no editor for either radius and no way to reorder the two curves, and the
/// two facts that need proving — that the stored order decides nothing, and
/// that a changed hole is never served from the cache — are about documents
/// nothing shipped can produce. Written through the same payload encoder the
/// document layer uses, so what lands in the row is a document rather than
/// bytes shaped like one.
fn rewrite_sketch(path: &Path, edit: impl FnOnce(&mut ferritecad_document::Sketch)) {
    let doc = Document::open_read_only(path).expect("reopen");
    let objects = doc.objects().expect("objects");
    let object = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("sketch")
        .clone();
    doc.close().expect("close");

    let ObjectPayload::Sketch(mut sketch) = object.payload else {
        panic!("checked Sketch")
    };
    edit(&mut sketch);
    let bytes = ObjectPayload::Sketch(sketch)
        .to_storage_bytes()
        .expect("payload");
    let c = rusqlite::Connection::open(path).expect("SQL");
    let changed = c
        .execute(
            "UPDATE objects SET payload=?1,payload_hash=?2 WHERE id=?3",
            rusqlite::params![
                bytes,
                ferritecad_types::ContentHash::of_bytes(&bytes)
                    .as_bytes()
                    .as_slice(),
                object.id.to_bytes().as_slice()
            ],
        )
        .expect("rewrite");
    assert_eq!(changed, 1, "one Sketch row was prepared");
}

#[test]
fn annulus_request_refusals_and_usage_preserve_files() {
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("annulus.json");
    let out = d.path().join("out.fcad");
    request(&input);
    let missing = d.path().join("missing.json");
    let v = reply(create(&missing, &out).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "io");
    for value in [
        // The version is this request's own, and only one of it is accepted.
        json!({"schema_version":2,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1}),
        json!({"request_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1}),
        // Every field is required, and nothing else is accepted.
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"inner_radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1}),
        // In particular the single-circle request is not this one.
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":2,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1,"radius_mm":3}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1,"holes":[]}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1,"inner_center_mm":[1,1]}),
        // A centre is exactly two numbers, and there is only one of them.
        json!({"schema_version":1,"center_mm":[0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":0,"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":["0","0"],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[null,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1}),
        // Both radii and the height are positive finite millimetres.
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":0,"inner_radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":-2,"inner_radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":0,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":-1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":"2","inner_radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":true,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":0}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":-1}),
        // The hole is inside its boundary, and the wall is a wall.
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":10,"inner_radius_mm":10,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":10,"inner_radius_mm":12,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":10,"inner_radius_mm":9.9999,"height_mm":1}),
        // The same 1e6 bound the one-circle request has.
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2e6,"inner_radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[2e6,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":2,"inner_radius_mm":1,"height_mm":2e6}),
    ] {
        write(&input, &value);
        let before = std::fs::read(&input).expect("input");
        let names = entries(d.path());
        let v = reply(create(&input, &out).output().expect("process"), 2);
        assert_eq!(
            v["error"]["kind"],
            if value["schema_version"] == 2 {
                "unsupported"
            } else {
                "input"
            },
            "{value}"
        );
        assert_eq!(std::fs::read(&input).expect("input"), before);
        assert_eq!(entries(d.path()), names, "nothing published, no scratch");
    }
    // JSON cannot spell these, so they arrive as raw text.
    let valid = json!({"schema_version":1,"center_mm":[0,0],"outer_radius_mm":10,
                       "inner_radius_mm":4,"height_mm":15})
    .to_string();
    for (field, bad) in [
        ("\"outer_radius_mm\":10", "\"outer_radius_mm\":NaN"),
        ("\"outer_radius_mm\":10", "\"outer_radius_mm\":1e999"),
        ("\"inner_radius_mm\":4", "\"inner_radius_mm\":NaN"),
        ("\"inner_radius_mm\":4", "\"inner_radius_mm\":Infinity"),
        ("\"height_mm\":15", "\"height_mm\":Infinity"),
        ("\"center_mm\":[0,0]", "\"center_mm\":[NaN,0]"),
        ("\"center_mm\":[0,0]", "\"center_mm\":[0,-1e999]"),
    ] {
        std::fs::write(&input, valid.replace(field, bad)).expect("bad number");
        let names = entries(d.path());
        assert_eq!(
            reply(create(&input, &out).output().expect("process"), 2)["error"]["kind"],
            "input",
            "{bad}"
        );
        assert_eq!(entries(d.path()), names);
    }
    request(&input);
    std::fs::write(&out, b"occupied").expect("occupied");
    let v = reply(create(&input, &out).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "input");
    assert_eq!(std::fs::read(&out).expect("output"), b"occupied");
    assert!(
        cli()
            .args([OP, "--help"])
            .output()
            .expect("help")
            .status
            .success()
    );
    let usage = cli().args([OP, "--json"]).output().expect("usage");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty(), "usage stays clap text");
    let output = create(&missing, &out)
        .stderr(pipe::closed_pipe())
        .output()
        .expect("pipe");
    reply(output, 2);
    assert_eq!(
        create(&missing, &out)
            .stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
}

#[test]
fn annulus_alias_and_stub_kernel_refusals_preserve_storage() {
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("request.json");
    request(&input);
    let saved = std::fs::read(&input).expect("request");
    let alias = d.path().join("alias.fcad");
    std::fs::hard_link(&input, &alias).expect("hard link");
    for dest in [&input, &alias] {
        let names = entries(d.path());
        reply(create(&input, dest).output().expect("process"), 2);
        assert_eq!(entries(d.path()), names);
        assert_eq!(std::fs::read(&input).expect("input"), saved);
    }
    #[cfg(unix)]
    {
        let link = d.path().join("symlink.fcad");
        std::os::unix::fs::symlink(&input, &link).expect("symlink");
        reply(create(&input, &link).output().expect("process"), 2);
        assert_eq!(std::fs::read(&input).expect("input"), saved);
    }
    if !ferritecad_occt::is_available() {
        // A build with no kernel cannot prove a hollow part is buildable, so
        // it refuses rather than publishing a document nobody checked.
        let output = d.path().join("new.fcad");
        let names = entries(d.path());
        let v = reply(create(&input, &output).output().expect("stub process"), 2);
        assert_eq!(v["error"]["kind"], "unsupported");
        assert_eq!(entries(d.path()), names);
        assert_eq!(
            create(&input, &output)
                .stdout(pipe::closed_pipe())
                .stderr(pipe::closed_pipe())
                .status()
                .expect("pipes")
                .code(),
            Some(7)
        );
        assert_eq!(entries(d.path()), names);
        assert_eq!(std::fs::read(input).expect("input"), saved);
    }
}

#[cfg(unix)]
#[test]
fn annulus_json_non_utf8_path_refuses_before_reading() {
    use std::os::unix::ffi::OsStringExt;
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join(std::ffi::OsString::from_vec(vec![b'a', 255]));
    let out = d.path().join("out.fcad");
    let v = reply(create(&input, &out).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "input");
    assert!(entries(d.path()).is_empty());
}

#[test]
fn native_annular_extrude_process_analytic_geometry_and_delivery() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("кольцо space.json");
    let out = d.path().join("втулка space.fcad");
    request(&input);
    let v = reply(create(&input, &out).output().expect("process"), 0);
    assert_eq!(v["result"]["destination"], out.to_str().expect("UTF-8"));
    let saved = std::fs::read(&out).expect("source");

    // The document stores two circles, by centre and radius, each with its own
    // UUID, and the report names both without anybody having to guess.
    let first = stored(&out);
    assert_eq!(first.objects, 4, "plane, sketch, extrude, body");
    assert_eq!(first.outer.1, [12., -7.]);
    assert_eq!(first.outer.2, 10.);
    assert_eq!(first.inner.1, [12., -7.]);
    assert_eq!(first.inner.2, 4.);
    assert_eq!(
        v["result"]["outer_curve_id"],
        first.outer.0.to_string(),
        "the report names the circle that bounds the part"
    );
    assert_eq!(
        v["result"]["inner_curve_id"],
        first.inner.0.to_string(),
        "and the circle that is the hole"
    );
    assert_ne!(v["result"]["outer_curve_id"], v["result"]["inner_curve_id"]);

    let doc = Document::open_read_only(&out).expect("reopen");
    assert_eq!(
        v["result"]["document_id"],
        doc.meta().document_id.to_string()
    );
    let objects = doc.objects().expect("objects");
    let id_of = |what: &str| {
        objects
            .iter()
            .find(|o| match &o.payload {
                ObjectPayload::Sketch(_) => what == "sketch",
                ObjectPayload::Extrude(_) => what == "extrude",
                ObjectPayload::Body(_) => what == "body",
                _ => false,
            })
            .unwrap_or_else(|| panic!("one {what}"))
            .id
    };
    for (field, what) in [
        ("sketch_id", "sketch"),
        ("extrude_id", "extrude"),
        ("body_id", "body"),
    ] {
        assert_eq!(v["result"][field], id_of(what).to_string(), "{field}");
    }
    doc.close().expect("close");

    assert_eq!(first.refs.len(), 4, "two caps and two walls");
    for (curve, what) in [(first.outer.0, "outer"), (first.inner.0, "inner")] {
        assert!(
            first.refs.iter().any(|r| r.output_role
                == SemanticRole::ExtrudeSide {
                    profile_segment: curve
                }),
            "the {what} wall is named by the circle that drew it"
        );
    }
    for want in [CapSide::Start, CapSide::End] {
        assert!(
            first
                .refs
                .iter()
                .any(|r| r.output_role == SemanticRole::ExtrudeCap { side: want }),
            "{want:?} cap is named"
        );
    }
    std::fs::remove_file(&input).expect("private request no longer needed");

    // Reopening and rebuilding cold twice changes no identity.
    for _ in 0..2 {
        let r = cli()
            .arg("rebuild")
            .arg(&out)
            .arg("--cold")
            .output()
            .expect("rebuild");
        assert!(r.status.success(), "{r:?}");
        let again = stored(&out);
        assert_eq!(again.outer, first.outer);
        assert_eq!(again.inner, first.inner);
        assert_eq!(again.refs, first.refs);
    }

    // The solid is analytic and really is hollow.
    let real = analytic(&out);
    check_analytic(&real, &first, 10., 4., 15.);

    // The archive outlives both its writing session and its open SQLite
    // connection, and a hit restores both walls under their own references.
    let cache = d.path().join("annulus.fcad-cache");
    for outcome in [
        ferritecad_eval::CacheOutcome::Miss,
        ferritecad_eval::CacheOutcome::Hit,
    ] {
        let warm = analytic_with_cache(&out, Some((&cache, outcome)));
        check_analytic(&warm, &first, 10., 4., 15.);
        assert_eq!(warm.faces, real.faces);
        assert!((warm.volume - real.volume).abs() < real.volume * 1e-9);
        assert_eq!(stored(&out).refs, first.refs);
    }
    std::fs::remove_file(cache).expect("private cache closed and removed");

    // The exported mesh is checked at explicit chord/angular settings,
    // separately from the B-Rep, and is measured for an actual cavity.
    let stl = d.path().join("result.stl");
    let m = mesh(&out, &stl);
    check_annular_mesh(&m, [12., -7.], 10., 4., 15.);

    let fbx = d.path().join("result.fbx");
    let r = cli()
        .arg("export-fbx")
        .arg(&out)
        .arg("-o")
        .arg(&fbx)
        .arg("--json")
        .output()
        .expect("FBX");
    assert!(r.status.success(), "{r:?}");
    let f: Value = serde_json::from_slice(&r.stdout).expect("JSON");
    assert_eq!(f["result"]["complete"], true);
    assert_eq!(
        std::fs::read(&fbx).expect("FBX").len() as u64,
        f["result"]["bytes"].as_u64().expect("bytes")
    );
    keep(&fbx, "annulus.fbx");
    assert_eq!(std::fs::read(&out).expect("source"), saved);

    // A second part with fractional radii, another centre and another height:
    // a hardcoded tube would pass the first measurement and fail this one.
    let other_input = d.path().join("other.json");
    let other = d.path().join("other.fcad");
    write(
        &other_input,
        &json!({"schema_version":1,"center_mm":[-3.5,4.25],"outer_radius_mm":6.75,
                "inner_radius_mm":2.125,"height_mm":2.5}),
    );
    reply(create(&other_input, &other).output().expect("process"), 0);
    let kept = stored(&other);
    assert_eq!(kept.outer.1, [-3.5, 4.25]);
    assert_eq!(kept.outer.2, 6.75);
    assert_eq!(kept.inner.2, 2.125);
    assert_ne!(kept.outer.0, first.outer.0, "two documents, two identities");
    assert_ne!(kept.inner.0, first.inner.0);
    check_analytic(&analytic(&other), &kept, 6.75, 2.125, 2.5);
    let m = mesh(&other, &d.path().join("other.stl"));
    check_annular_mesh(&m, [-3.5, 4.25], 6.75, 2.125, 2.5);

    // Losing the report after publication is late, not a rollback.
    for both in [false, true] {
        let input = d.path().join("pipe.json");
        request(&input);
        let published = d.path().join(format!("pipe-{both}.fcad"));
        let mut c = create(&input, &published);
        c.stdout(pipe::closed_pipe());
        if both {
            c.stderr(pipe::closed_pipe());
        } else {
            c.stderr(std::process::Stdio::piped());
        }
        let delivery = c.output().expect("closed stdout");
        assert_eq!(delivery.status.code(), Some(7));
        if both {
            assert!(delivery.stderr.is_empty());
        } else {
            assert!(
                String::from_utf8_lossy(&delivery.stderr).contains("JSON report delivery failed")
            );
        }
        let kept = stored(&published);
        assert_eq!(kept.outer.2, 10.);
        assert_eq!(kept.inner.2, 4.);
    }
    let mut expected: Vec<std::ffi::OsString> = [
        "втулка space.fcad",
        "result.stl",
        "result.fbx",
        "other.json",
        "other.fcad",
        "other.stl",
        "pipe.json",
        "pipe-false.fcad",
        "pipe-true.fcad",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    expected.sort();
    assert_eq!(
        entries(d.path()),
        expected,
        "no scratch/cache/SQLite sidecars"
    );
}

/// Which circle is written first decides nothing.
#[test]
fn native_annulus_ignores_the_order_the_two_circles_are_stored_in() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("request.json");
    let source = d.path().join("source.fcad");
    request(&input);
    reply(create(&input, &source).output().expect("process"), 0);
    let before = stored(&source);
    assert_eq!(
        before.stored_order,
        vec![before.outer.0, before.inner.0],
        "the writer stores the boundary first"
    );
    let straight = analytic(&source);
    check_analytic(&straight, &before, 10., 4., 15.);

    let swapped = d.path().join("swapped.fcad");
    std::fs::copy(&source, &swapped).expect("copy");
    rewrite_sketch(&swapped, |sketch| sketch.curves.swap(0, 1));

    let after = stored(&swapped);
    assert_eq!(
        after.stored_order,
        vec![before.inner.0, before.outer.0],
        "the hole is now written first"
    );
    // Same two circles, same two identities, same references.
    assert_eq!(after.outer, before.outer);
    assert_eq!(after.inner, before.inner);
    assert_eq!(after.refs, before.refs);

    // And the same solid, with each wall still under its own circle's name.
    let other_way = analytic(&swapped);
    check_analytic(&other_way, &after, 10., 4., 15.);
    assert!((other_way.volume - straight.volume).abs() < straight.volume * 1e-9);

    let m = mesh(&swapped, &d.path().join("swapped.stl"));
    check_annular_mesh(&m, [12., -7.], 10., 4., 15.);
    assert_eq!(
        std::fs::read(&source).expect("source"),
        std::fs::read(&source).expect("source")
    );
}

/// A changed hole is never served from the cache entry of the old one.
#[test]
fn native_annulus_cache_serves_the_hole_the_document_actually_has() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("request.json");
    let source = d.path().join("source.fcad");
    let cache = d.path().join("shared.fcad-cache");
    request(&input);
    reply(create(&input, &source).output().expect("process"), 0);
    let first = stored(&source);

    // A real miss, then a real hit, in two separate kernel sessions.
    for outcome in [
        ferritecad_eval::CacheOutcome::Miss,
        ferritecad_eval::CacheOutcome::Hit,
    ] {
        let warm = analytic_with_cache(&source, Some((&cache, outcome)));
        check_analytic(&warm, &first, 10., 4., 15.);
    }

    // The bore is widened in place, so the producer, the document identity and
    // therefore the cache file are all the same. Only the geometry moved.
    rewrite_sketch(&source, |sketch| {
        let smallest = sketch
            .curves
            .iter_mut()
            .min_by(|a, b| {
                let radius = |c: &ferritecad_document::SketchCurve| match c.geometry {
                    SketchGeometry::Circle { radius, .. } => radius,
                    _ => panic!("two circles"),
                };
                radius(a).total_cmp(&radius(b))
            })
            .expect("a hole");
        let SketchGeometry::Circle { center, .. } = smallest.geometry else {
            panic!("a circle")
        };
        smallest.geometry = SketchGeometry::Circle { center, radius: 7. };
    });
    let widened = stored(&source);
    assert_eq!(widened.inner.0, first.inner.0, "the hole keeps its UUID");
    assert_eq!(widened.inner.2, 7.);

    // A miss, because the key carries the geometry; and the measured cavity is
    // the new one rather than the entry the same file already holds.
    let after = analytic_with_cache(&source, Some((&cache, ferritecad_eval::CacheOutcome::Miss)));
    check_analytic(&after, &widened, 10., 7., 15.);
    let then = analytic_with_cache(&source, Some((&cache, ferritecad_eval::CacheOutcome::Hit)));
    check_analytic(&then, &widened, 10., 7., 15.);

    // Measured out of the actual mesh, not out of a result DTO.
    let m = mesh(&source, &d.path().join("widened.stl"));
    check_annular_mesh(&m, [12., -7.], 10., 7., 15.);
}

#[test]
fn native_annulus_height_copy_keeps_both_circles_and_every_identity() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("request.json");
    let source = d.path().join("source.fcad");
    request(&input);
    reply(create(&input, &source).output().expect("process"), 0);
    let before = stored(&source);
    let bytes = std::fs::read(&source).expect("source");
    let sql_before = tables(&source);

    let extrude = {
        let doc = Document::open_read_only(&source).expect("reopen");
        let id = doc
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
            .expect("extrude")
            .id;
        doc.close().expect("close");
        id
    };
    let taller = d.path().join("taller.fcad");
    let r = cli()
        .arg("edit-extrude")
        .arg(&source)
        .arg("--feature")
        .arg(extrude.to_string())
        .arg("--distance-mm")
        .arg("25")
        .arg("-o")
        .arg(&taller)
        .arg("--json")
        .output()
        .expect("edit-extrude");
    assert!(r.status.success(), "{r:?}");

    // Both circles, both identities and every other stored fact are the
    // copy's too.
    let after = stored(&taller);
    assert_eq!(after.outer, before.outer, "the boundary keeps its UUID");
    assert_eq!(after.inner, before.inner, "and so does the hole");
    assert_eq!(after.stored_order, before.stored_order);
    assert_eq!(after.refs, before.refs, "same reference UUIDs and roles");
    assert_eq!(after.objects, before.objects);
    assert_eq!(
        std::fs::read(&source).expect("source"),
        bytes,
        "the source is untouched"
    );

    // Only the height changed, and it changed the analytic volume with it.
    check_analytic(&analytic(&taller), &after, 10., 4., 25.);
    let m = mesh(&taller, &d.path().join("taller.stl"));
    check_annular_mesh(&m, [12., -7.], 10., 4., 25.);
    assert!((m.hi[2] - m.lo[2] - 25.).abs() < 1e-4, "{:?}", m.hi);
    let fbx = d.path().join("taller.fbx");
    let r = cli()
        .arg("export-fbx")
        .arg(&taller)
        .arg("-o")
        .arg(&fbx)
        .arg("--json")
        .output()
        .expect("FBX");
    assert!(r.status.success(), "{r:?}");
    keep(&fbx, "annulus-taller.fbx");

    // Everything the document stores apart from the one changed expression.
    let sql_after = tables(&taller);
    assert_eq!(
        sql_before.keys().collect::<Vec<_>>(),
        sql_after.keys().collect::<Vec<_>>()
    );
    for (table, rows) in &sql_before {
        match table.as_str() {
            "objects" => {
                assert_eq!(rows.len(), sql_after[table].len());
                let id = rusqlite::types::Value::Blob(extrude.to_bytes().to_vec());
                let mut edited = 0;
                for (a, b) in rows.iter().zip(&sql_after[table]) {
                    assert_eq!(a.len(), b.len());
                    if a[0] == id {
                        edited += 1;
                        assert_eq!(&a[..6], &b[..6], "Extrude metadata changed");
                        assert_ne!(a[6], b[6], "height payload did not change");
                        assert_ne!(a[7], b[7], "payload hash did not change");
                    } else {
                        assert_eq!(a, b, "an unedited object changed");
                    }
                }
                assert_eq!(edited, 1, "the selected Extrude row was not compared");
            }
            "meta" => {
                assert_eq!(rows.len(), sql_after[table].len());
                for (a, b) in rows.iter().zip(&sql_after[table]) {
                    assert_eq!(a.len(), b.len());
                    for (i, cell) in a.iter().enumerate() {
                        if i != 7 {
                            assert_eq!(cell, &b[i], "meta cell {i}");
                        }
                    }
                }
            }
            _ => assert_eq!(&sql_after[table], rows, "{table}"),
        }
    }
}

/// The editors of the previous slices refuse this Sketch and say why.
///
/// Creating geometry is not editing it. Neither the Line editor nor the circle
/// editor of §25K understands a Sketch with two curves in it, and both have to
/// say so rather than offering an edit that would rewrite the wrong one.
// Structural discovery must execute without a kernel; this fixture is not a
// production creation route and makes no geometric claim.
fn write_annular_fixture(path: &Path) {
    use ferritecad_document::{
        Body, CapSide, DatumPlane, Dependency, DependencyRole, EndCondition, EntityKind,
        Expression, Extrude, Point2, SelectionRule, SemanticRole, Sketch, SketchCurve,
        SolidOperation, TopologyRef,
    };
    use ferritecad_types::{ObjectId, StableEntityId, Transform};
    let mut d = Document::create(path).expect("document");
    let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
    let center = [12., -7.];
    let radius = 10.;
    let height = 15.;
    let curve = StableEntityId::new();
    let inner = StableEntityId::new();
    d.write(|w| {
        w.put_object(
            plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;
        w.put_object(
            sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves: vec![
                    SketchCurve {
                        id: curve,
                        construction: false,
                        geometry: SketchGeometry::Circle {
                            center: Point2::new(center[0], center[1])?,
                            radius,
                        },
                    },
                    SketchCurve {
                        id: inner,
                        construction: false,
                        geometry: SketchGeometry::Circle {
                            center: Point2::new(center[0], center[1])?,
                            radius: 4.,
                        },
                    },
                ],
                constraints: Vec::new(),
            }),
        )?;
        w.put_object(
            extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(height)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                target_body: None,
            }),
        )?;
        w.put_object(
            body,
            None,
            3,
            Some("Body"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(extrude),
            }),
        )?;
        for (dependent, dependency, role) in [
            (sketch, plane, DependencyRole::Plane),
            (extrude, sketch, DependencyRole::Profile),
            (body, extrude, DependencyRole::BodyTip),
        ] {
            w.add_dependency(Dependency {
                dependent,
                dependency,
                role,
            })?;
        }
        for side in [CapSide::Start, CapSide::End] {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: extrude,
                producer_feature: extrude,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::ExtrudeCap { side },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })?;
        }
        for curve in [curve, inner] {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: extrude,
                producer_feature: extrude,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::ExtrudeSide {
                    profile_segment: curve,
                },
                selection: SelectionRule::AllDerivedFrom { ancestor: curve },
                fallback_signature: None,
            })?;
        }
        Ok(())
    })
    .expect("circle document");
    d.close().expect("close");
}

#[test]
fn annulus_discovery_refuses_both_earlier_editors_and_says_so() {
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("request.json");
    let source = d.path().join("source.fcad");
    request(&input);
    if ferritecad_occt::is_available() {
        reply(create(&input, &source).output().expect("process"), 0);
    } else {
        write_annular_fixture(&source);
    }
    let bytes = std::fs::read(&source).expect("source");

    let r = cli()
        .arg("inspect")
        .arg(&source)
        .arg("--json")
        .output()
        .expect("inspect");
    assert!(r.status.success(), "{r:?}");
    let v: Value = serde_json::from_slice(&r.stdout).expect("JSON");
    let sketches = v["result"]["sketches"].as_array().expect("sketches");
    assert_eq!(sketches.len(), 1);
    let sketch = &sketches[0];

    // The Line editor: not editable, with a reason, and no invented contour.
    assert_eq!(sketch["editable"], false);
    assert!(sketch["vertices"].is_null(), "no invented polygon");
    assert!(
        sketch["refusal"]
            .as_str()
            .expect("a reason")
            .contains("Line"),
        "{}",
        sketch["refusal"]
    );

    // The §25K circle editor: not available, with a reason, and no circle.
    let circle = &sketch["circle_edit"];
    assert_eq!(circle["available"], false);
    assert!(circle["circle"].is_null(), "no invented circle");
    assert!(
        circle["refusal"]
            .as_str()
            .expect("a reason")
            .contains("exactly one unconstrained Circle"),
        "{}",
        circle["refusal"]
    );

    // The height, which this slice does leave editable, is still offered.
    assert_eq!(v["result"]["edit_extrude"]["available"], true);
    assert_eq!(v["result"]["features"][0]["editable"], true);
    assert_eq!(v["result"]["features"][0]["distance_mm"], 15.0);

    // An attempted Circle edit is refused and writes nothing. With a kernel,
    // it reaches the structural refusal; without one, the existing CLI kernel
    // availability check has priority. Discovery above executes in both builds.
    let names = entries(d.path());
    let edit = d.path().join("edited.fcad");
    let request = d.path().join("edit.json");
    let saved = stored(&source);
    std::fs::write(
        &request,
        json!({"request_version":1,"curve_id":saved.inner.0.to_string(),
               "center_mm":[12.0,-7.0],"radius_mm":3.0})
        .to_string(),
    )
    .expect("edit request");
    let content_version = v["result"]["content_version"]
        .as_str()
        .expect("a content version")
        .to_owned();
    let sketch_id = sketch["sketch_id"]
        .as_str()
        .expect("a sketch UUID")
        .to_owned();
    let r = cli()
        .arg("edit-circle")
        .arg(&source)
        .args(["--sketch", &sketch_id])
        .args(["--expect-version", &content_version])
        .arg("--request")
        .arg(&request)
        .arg("-o")
        .arg(&edit)
        .arg("--json")
        .output()
        .expect("edit-circle");
    assert_eq!(r.status.code(), Some(2), "{r:?}");
    let refusal: Value = serde_json::from_slice(&r.stdout).expect("JSON");
    assert_eq!(refusal["ok"], false);
    assert_eq!(refusal["error"]["kind"], "unsupported");
    assert!(
        refusal["error"]["message"]
            .as_str()
            .expect("a reason")
            .contains(if ferritecad_occt::is_available() {
                "exactly one unconstrained Circle"
            } else {
                "this build has no Open CASCADE"
            }),
        "{}",
        refusal["error"]["message"]
    );
    assert_eq!(std::fs::read(&source).expect("source"), bytes);
    let mut still = names;
    still.push("edit.json".into());
    still.sort();
    assert_eq!(entries(d.path()), still, "nothing published, no scratch");
}
