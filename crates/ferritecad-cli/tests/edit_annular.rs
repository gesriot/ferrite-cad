// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
#[path = "support/names.rs"]
mod names;
#[path = "support/transaction.rs"]
mod transaction;
use ferritecad_document::{Document, ObjectPayload, SketchGeometry};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    f64::consts::PI,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const OP: &str = "edit";
const LINEAR_MM: f64 = 0.05;
const ANGULAR_RAD: f64 = 0.1;
/// How far a measured coordinate may sit from the radius it belongs to.
///
/// Reading, not tessellation: STL stores single-precision floats, so a vertex
/// exactly on a 10 mm circle comes back a few parts in 10^7 away from it. The
/// chord error the tessellation is allowed is a separate budget.
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
    let Some(dir) = std::env::var_os("FCAD_ANNULUS_EDIT_ARTIFACTS") else {
        return;
    };
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).expect("artifact directory");
    std::fs::copy(path, dir.join(name)).expect("artifact");
}

/// A source document and the one accepted reading of it a request may use.
struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    request: PathBuf,
    catalog: Value,
}
impl Fixture {
    /// The same drawing with the bore written first, which creation never does.
    fn swapped() -> Self {
        Self::built(|path| write_annular_document(path, [12., -7.], 10., 4., 15., true))
    }
    fn built(build: impl FnOnce(&Path)) -> Self {
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("кольцо space.fcad");
        let request = root.path().join("request.json");
        build(&source);
        let catalog = inspect(&source);
        let f = Self {
            root,
            source,
            request,
            catalog,
        };
        f.ask([-3.5, 4.25], 6.75, 2.125);
        f
    }

    fn ask(&self, center: [f64; 2], outer: f64, inner: f64) {
        let saved = stored(&self.source);
        write(
            &self.request,
            &json!({"request_version":1,"changes":[
                transaction::circle(&json!(saved.sketch), &json!(saved.outer), center, outer),
                transaction::circle(&json!(saved.sketch), &json!(saved.inner), center, inner),
            ]}),
        );
    }
    fn edit(&self, output: &Path) -> Command {
        self.edit_from(&self.source, &self.catalog, output)
    }
    fn edit_from(&self, source: &Path, catalog: &Value, output: &Path) -> Command {
        let mut c = cli();
        c.arg(OP)
            .arg(source)
            .arg("--expect-version")
            .arg(catalog["content_version"].as_str().expect("version"))
            .arg("--request")
            .arg(&self.request)
            .arg("-o")
            .arg(output)
            .arg("--json");
        c
    }
}

/// The four objects an annular extrusion is, written without a kernel.
///
/// `swapped` writes the bore first. Creation always writes the boundary first,
/// so this is the one way to produce the document whose stored order must not
/// decide the roles.
fn write_annular_document(
    path: &Path,
    center: [f64; 2],
    outer_radius: f64,
    inner_radius: f64,
    height: f64,
    swapped: bool,
) {
    use ferritecad_document::{
        Body, DatumPlane, Dependency, DependencyRole, EndCondition, Expression, Extrude, Point2,
        Sketch, SketchCurve, SolidOperation,
    };
    use ferritecad_types::{ObjectId, StableEntityId, Transform};
    let mut d = Document::create(path).expect("document");
    let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
    let outer = StableEntityId::new();
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
        let circle = |id, radius| -> ferritecad_types::Result<SketchCurve> {
            Ok(SketchCurve {
                id,
                construction: false,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(center[0], center[1])?,
                    radius,
                },
            })
        };
        let mut curves = vec![circle(outer, outer_radius)?, circle(inner, inner_radius)?];
        if swapped {
            curves.reverse();
        }
        w.put_object(
            sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves,
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
                previous: None,
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

        Ok(())
    })
    .expect("annular document");
    d.close().expect("close");
}

/// The two circles a document stores, plus every identity beside them.
struct Stored {
    sketch: ferritecad_types::ObjectId,
    outer: ferritecad_types::StableEntityId,
    inner: ferritecad_types::StableEntityId,
    center: [f64; 2],
    inner_center: [f64; 2],
    outer_radius: f64,
    inner_radius: f64,
    height_mm: f64,
    /// Both curves in the order the Sketch actually stores them.
    stored_order: Vec<ferritecad_types::StableEntityId>,
    refs: crate::names::Names,
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
    assert_eq!(sketch.curves.len(), 2, "two model curves");
    assert!(sketch.constraints.is_empty());
    let mut circles = Vec::new();
    for curve in &sketch.curves {
        assert!(!curve.construction);
        let SketchGeometry::Circle { center, radius } = curve.geometry else {
            panic!("the document stores Circles")
        };
        circles.push((curve.id, [center.x, center.y], radius));
    }
    assert_ne!(circles[0].0, circles[1].0);
    let stored_order = circles.iter().map(|c| c.0).collect();
    circles.sort_by(|a, b| b.2.total_cmp(&a.2));
    let height_mm = match objects.iter().find_map(|o| match &o.payload {
        ObjectPayload::Extrude(e) => Some(e.end_condition.clone()),
        _ => None,
    }) {
        Some(ferritecad_document::EndCondition::Blind { distance }) => distance.value(),
        other => panic!("one Blind extrusion, got {other:?}"),
    };
    let refs = crate::names::snapshot(&d);
    let count = objects.len();
    d.close().expect("close");
    Stored {
        sketch: row.id,
        outer: circles[0].0,
        inner: circles[1].0,
        center: circles[0].1,
        inner_center: circles[1].1,
        outer_radius: circles[0].2,
        inner_radius: circles[1].2,
        height_mm,
        stored_order,
        refs,
        objects: count,
    }
}

/// What the real kernel says about the solid a document rebuilds into.
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
    let mut walls: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut caps = Vec::new();
    let mut stats = None;
    crate::names::assert_primary(&built);
    for (_, input, handle) in crate::names::root_faces(&doc, &built) {
        let surface = kernel.face_surface(handle).expect("surface");
        match input {
            ferritecad_kernel::NamingInput::Curve(curve) => {
                walls.entry(curve).or_default().push(surface)
            }
            ferritecad_kernel::NamingInput::Cap { .. } => caps.push(surface),
            _ => panic!("unexpected root"),
        }
        stats = Some(kernel.shape_stats(handle.shape()).expect("stats"));
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

/// Everything the analytic solid must be, checked against the stored numbers.
///
/// The volume reference is `pi*(R-r)*(R+r)*h` rather than `pi*(R^2-r^2)*h`:
/// subtracting squared radii loses precision in the reference itself once the
/// two are close, which is exactly where a wall check matters.
fn analytic_matches(a: &Analytic, s: &Stored) {
    assert_eq!(a.faces, 4, "two cylindrical walls and two planar caps");
    assert_eq!(
        a.walls.get(&s.outer).map(Vec::as_slice),
        Some(
            [ferritecad_kernel::FaceSurface::Cylinder {
                radius: s.outer_radius
            }]
            .as_slice()
        ),
        "the boundary circle's own reference names the outer wall"
    );
    assert_eq!(
        a.walls.get(&s.inner).map(Vec::as_slice),
        Some(
            [ferritecad_kernel::FaceSurface::Cylinder {
                radius: s.inner_radius
            }]
            .as_slice()
        ),
        "the bore circle's own reference names the bore"
    );
    assert_eq!(a.caps.len(), 2);
    assert!(
        a.caps
            .iter()
            .all(|s| *s == ferritecad_kernel::FaceSurface::Plane)
    );
    let exact =
        PI * (s.outer_radius - s.inner_radius) * (s.outer_radius + s.inner_radius) * s.height_mm;
    assert!(
        (a.volume - exact).abs() < 1e-6 * exact,
        "analytic volume {} is not the annulus {exact}; a lost hole would be {}",
        a.volume,
        PI * s.outer_radius * s.outer_radius * s.height_mm
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
/// that both walls are there at their own radii, that the bore's facets face the
/// axis while the outside's face away, that no cap triangle covers the axis, and
/// that the volume the winding gives is the area its own measured perimeters
/// enclose. No analytic pi is demanded of the mesh.
fn mesh_matches(m: &Mesh, center: [f64; 2], outer: f64, inner: f64, height: f64) {
    assert!(m.triangles >= 48);

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

    let exact = PI * (outer - inner) * (outer + inner) * height;
    assert!(
        (m.volume - (outer_area - inner_area) * height).abs() <= exact * 1e-4,
        "STL volume {} does not match its own two measured perimeters",
        m.volume
    );
    let full = PI * outer * outer * height;
    assert!(m.volume > 0. && m.volume <= exact * (1. + 1e-6));
    assert!(
        m.volume < full * 0.999,
        "volume {} is a solid cylinder, so the hole is not in the mesh",
        m.volume
    );

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
/// The allow-list is short on purpose: the selected Sketch's payload and its
/// hash, and the copy's own modified timestamp. Anything else that moved is a
/// cell this edit had no business touching.
fn only_the_sketch_row_changed(source: &Path, copy: &Path, sketch: ferritecad_types::ObjectId) {
    let before = tables(source);
    let after = tables(copy);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "the copy has the same tables"
    );
    let selected = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
    let mut changed = 0;
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
                if table == "objects" {
                    changed += 1;
                }
            }
        }
    }
    assert_eq!(
        changed, 2,
        "the payload and its hash are the two that moved"
    );
}

/// The swapped fixture really does store the bore first.
fn stored_swapped_check(f: &Fixture) -> Stored {
    let s = stored(&f.source);
    assert_eq!(
        s.stored_order,
        vec![s.inner, s.outer],
        "the fixture did not store the bore first"
    );
    s
}

#[test]
fn native_annulus_edit_moves_and_resizes_only_the_named_pair() {
    if !native() {
        return;
    }
    // A source this project's own shipped command really made.
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("tube.fcad");
    let create = root.path().join("create.json");
    write(
        &create,
        &json!({"schema_version":1,"center_mm":[12.0,-7.0],"outer_radius_mm":10.0,
                "inner_radius_mm":4.0,"height_mm":15.0}),
    );
    let made = reply(
        cli()
            .arg("create-annular-extrude")
            .arg(&create)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create"),
        "create-annular-extrude",
        0,
    );
    let before = stored(&source);
    let bytes = std::fs::read(&source).expect("source");
    assert_eq!(
        made["result"]["outer_curve_id"],
        before.outer.to_string(),
        "creation and discovery agree about the boundary"
    );
    assert_eq!(made["result"]["inner_curve_id"], before.inner.to_string());

    let catalog = inspect(&source);
    let request = root.path().join("edit.json");
    let ask = |center: [f64; 2], outer: f64, inner: f64| {
        write(
            &request,
            &json!({"request_version":1,"changes":[
                transaction::circle(&json!(before.sketch), &json!(before.outer), center, outer),
                transaction::circle(&json!(before.sketch), &json!(before.inner), center, inner)
            ]}),
        );
    };
    let edit = |output: &Path| {
        let mut c = cli();
        c.arg(OP)
            .arg(&source)
            .arg("--expect-version")
            .arg(catalog["content_version"].as_str().expect("version"))
            .arg("--request")
            .arg(&request)
            .arg("-o")
            .arg(output)
            .arg("--json");
        c
    };

    // All three numbers at once, then each on its own. Every one is a whole
    // publication, measured on its own terms.
    let mut artefacts = Vec::new();
    for (name, center, outer, inner) in [
        ("all.fcad", [-3.5, 4.25], 6.75, 2.125),
        ("centre.fcad", [-3.5, 4.25], 10., 4.),
        ("outer.fcad", [12., -7.], 6.75, 4.),
        ("inner.fcad", [12., -7.], 10., 2.125),
    ] {
        ask(center, outer, inner);
        let copy = root.path().join(name);
        let v = reply(edit(&copy).output().expect("process"), OP, 0);
        assert_eq!(v["result"]["destination"], copy.to_str().expect("UTF-8"));

        assert_eq!(
            v["result"]["document_id"], catalog["document_id"],
            "a copy keeps the document identity"
        );

        // Every identity, the stored order and the height survive; only the
        // three numbers moved.
        let after = stored(&copy);
        assert_eq!(after.sketch, before.sketch);
        assert_eq!(after.outer, before.outer, "the boundary keeps its UUID");
        assert_eq!(after.inner, before.inner, "and so does the bore");
        assert_eq!(after.stored_order, before.stored_order, "{name}");
        assert_eq!(after.refs, before.refs, "same primary name digests");
        assert_eq!(after.objects, before.objects);
        assert_eq!(after.height_mm, 15., "the height is not in this request");
        assert_eq!(after.center, center);
        assert_eq!(
            after.inner_center, center,
            "an accepted edit leaves both circles at one centre"
        );
        assert_eq!(after.outer_radius, outer);
        assert_eq!(after.inner_radius, inner);
        assert_eq!(
            std::fs::read(&source).expect("source"),
            bytes,
            "{name}: the source was touched"
        );
        only_the_sketch_row_changed(&source, &copy, before.sketch);

        // Reopened cold, the copy is the hollow part those numbers describe,
        // with each wall under its own circle's reference.
        analytic_matches(&analytic(&copy), &after);
        let m = mesh(&copy, &copy.with_extension("stl"));
        mesh_matches(&m, center, outer, inner, 15.);
        artefacts.push((name, copy, m));
    }

    // The FBX of the fully edited copy, for the pinned reader in the same job.
    let (_, all, _) = &artefacts[0];
    let fbx = root.path().join("all.fbx");
    let r = cli()
        .arg("export-fbx")
        .arg(all)
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
    keep(&fbx, "annulus-edited-cli.fbx");

    // A real cache miss then hit, and then the same store after the hole has
    // changed: the key moved with it, so the old cavity is never served.
    let cache = root.path().join("shared.fcad-cache");
    let all_stored = stored(all);
    for outcome in [
        ferritecad_eval::CacheOutcome::Miss,
        ferritecad_eval::CacheOutcome::Hit,
    ] {
        analytic_matches(
            &analytic_with_cache(all, Some((&cache, outcome))),
            &all_stored,
        );
    }
    // `inner.fcad` is the same document identity with a different bore, so it
    // addresses the same cache file and the same producer.
    let (_, widened, _) = artefacts
        .iter()
        .find(|(name, _, _)| *name == "inner.fcad")
        .expect("the bore-only copy");
    let widened_stored = stored(widened);
    assert_eq!(widened_stored.inner_radius, 2.125);
    for outcome in [
        ferritecad_eval::CacheOutcome::Miss,
        ferritecad_eval::CacheOutcome::Hit,
    ] {
        analytic_matches(
            &analytic_with_cache(widened, Some((&cache, outcome))),
            &widened_stored,
        );
    }
    std::fs::remove_file(&cache).expect("private cache closed and removed");

    // The existing height edit still works on an edited copy, and keeps both
    // circles, both side refs and every other cell.
    let extrude = {
        let d = Document::open_read_only(all).expect("reopen");
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
    let taller = root.path().join("taller.fcad");
    let r = transaction::constant(
        all,
        &Document::open_read_only(all)
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
    assert_eq!(raised.outer, all_stored.outer);
    assert_eq!(raised.inner, all_stored.inner);
    assert_eq!(raised.refs, all_stored.refs);
    assert_eq!(raised.height_mm, 25.);
    assert_eq!(raised.outer_radius, 6.75);
    assert_eq!(raised.inner_radius, 2.125);
    analytic_matches(&analytic(&taller), &raised);
    mesh_matches(
        &mesh(&taller, &root.path().join("taller.stl")),
        [-3.5, 4.25],
        6.75,
        2.125,
        25.,
    );

    assert_eq!(
        std::fs::read(&source).expect("source"),
        bytes,
        "the source is untouched by everything above"
    );
}

/// A document whose two circles were stored the other way round edits with the
/// same UUID roles and produces the same solid.
#[test]
fn native_annulus_edit_ignores_the_order_the_circles_are_stored_in() {
    if !native() {
        return;
    }
    let f = Fixture::swapped();
    let before = stored_swapped_check(&f);
    let bytes = std::fs::read(&f.source).expect("source");
    let copy = f.root.path().join("copy.fcad");
    f.ask([-3.5, 4.25], 6.75, 2.125);
    reply(f.edit(&copy).output().expect("process"), OP, 0);

    let after = stored(&copy);
    assert_eq!(
        after.stored_order,
        vec![before.inner, before.outer],
        "the bore is still written first"
    );
    assert_eq!(after.outer, before.outer);
    assert_eq!(after.inner, before.inner);
    assert_eq!(after.outer_radius, 6.75, "the boundary got the boundary's");
    assert_eq!(after.inner_radius, 2.125, "and the bore got the bore's");
    assert_eq!(after.refs, before.refs);
    assert_eq!(std::fs::read(&f.source).expect("source"), bytes);
    only_the_sketch_row_changed(&f.source, &copy, before.sketch);

    analytic_matches(&analytic(&copy), &after);
    mesh_matches(
        &mesh(&copy, &f.root.path().join("copy.stl")),
        [-3.5, 4.25],
        6.75,
        2.125,
        15.,
    );
}
