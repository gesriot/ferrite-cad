// SPDX-License-Identifier: MIT
//! §25O: a parametric annulus — two radii, one shared centre, one pin.
//!
//! Everything measured here is read back — the degrees of freedom from the
//! solver, the faces and the volume from the kernel, the shape of the solid
//! from the exported bytes — rather than compared against a number written down
//! in advance. Two facts are asserted rather than measured, and they are the
//! two this slice exists to keep: the stored sketch is still the starting guess
//! it always was, and the boundary and the bore are still the circles they were
//! before the solve.
#![allow(clippy::panic)]
#[path = "support/names.rs"]
mod names;
#[path = "support/transaction.rs"]
mod transaction;
use ferritecad_document::{
    Document, ObjectPayload, SketchConstraintRule, SketchGeometry, SketchPointSelector,
};
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
fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_eval::solver_available(), "PLANEGCS"),
    ])
}
/// Keeps a published artefact where a CI step can hand it to the pinned reader.
fn keep(path: &Path, name: &str) {
    let Some(dir) = std::env::var_os("FCAD_ANNULAR_CONSTRAINT_ARTIFACTS") else {
        return;
    };
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).expect("artifact directory");
    std::fs::copy(path, dir.join(name)).expect("artifact");
}

/// A saved §25L hollow cylinder, and the one accepted reading of it.
struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    request: PathBuf,
    catalog: Value,
}
impl Fixture {
    /// The document the shipped creation command really makes.
    fn annulus() -> Self {
        Self::built(|path| {
            let create = path.with_extension("create.json");
            write(
                &create,
                &json!({"schema_version":1,"center_mm":[12.0,-7.0],"outer_radius_mm":10.0,
                        "inner_radius_mm":4.0,"height_mm":15.0}),
            );
            let made = cli()
                .arg("create-annular-extrude")
                .arg(&create)
                .arg("-o")
                .arg(path)
                .arg("--json")
                .output()
                .expect("create");
            assert!(made.status.success(), "{made:?}");
            std::fs::remove_file(&create).expect("the request is not part of the fixture");
        })
    }
    /// The same drawing written without a kernel, so discovery and the request
    /// protocol are testable in a build that has neither kernel nor solver.
    fn drawn(swapped: bool) -> Self {
        Self::built(move |path| write_annular_document(path, [12., -7.], 10., 4., 15., swapped))
    }
    fn built(build: impl FnOnce(&Path)) -> Self {
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("кольцо space.fcad");
        build(&source);
        let catalog = inspect(&source);
        Self {
            request: root.path().join("request.json"),
            root,
            source,
            catalog,
        }
    }
    fn discovery(&self) -> &Value {
        &self.catalog["sketches"][0]
    }
    /// The circle discovery reports in the named role, from the same snapshot.
    fn role(&self, role: &str) -> &str {
        let curves = self.discovery()["curves"].as_array().expect("curves");
        let selected = if role == "boundary" {
            curves.iter().max_by(|a, b| {
                a["geometry"]["radius_mm"]
                    .as_f64()
                    .expect("radius")
                    .total_cmp(&b["geometry"]["radius_mm"].as_f64().expect("radius"))
            })
        } else {
            curves.iter().min_by(|a, b| {
                a["geometry"]["radius_mm"]
                    .as_f64()
                    .expect("radius")
                    .total_cmp(&b["geometry"]["radius_mm"].as_f64().expect("radius"))
            })
        };
        selected.expect("circle")["curve_id"]
            .as_str()
            .expect("curve UUID")
    }
    fn boundary(&self) -> &str {
        self.role("boundary")
    }
    fn bore(&self) -> &str {
        self.role("bore")
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
fn concentric_add(a: &str, b: &str) -> Value {
    json!({"kind":"coincident","a":transaction::point_ref(&json!(a),"center"),"b":transaction::point_ref(&json!(b),"center")})
}

/// The four objects an annular extrusion is, written without a kernel.
///
/// `swapped` writes the bore first, which creation never does: it is the one
/// way to produce the document whose stored order must not decide the roles.
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
                regions: Vec::new(),
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

/// What a document stores about its two circles, and every identity beside them.
struct Stored {
    sketch: ferritecad_types::ObjectId,
    /// Both curves in the order the Sketch actually stores them.
    stored_order: Vec<ferritecad_types::StableEntityId>,
    /// Centre and radius per curve UUID, so nothing is read by position.
    circles: BTreeMap<ferritecad_types::StableEntityId, ([f64; 2], f64)>,
    height_mm: f64,
    constraints: Vec<(ferritecad_types::StableEntityId, String)>,
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
    let mut circles = BTreeMap::new();
    for curve in &sketch.curves {
        assert!(!curve.construction, "both boundaries are model geometry");
        let SketchGeometry::Circle { center, radius } = curve.geometry else {
            panic!("the document stores Circles, not an approximation of them")
        };
        circles.insert(curve.id, ([center.x, center.y], radius));
    }
    assert_eq!(circles.len(), 2, "two curves, two identities");
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
        .map(|c| (c.id, rule_text(c.rule.clone())))
        .collect();
    let refs = crate::names::snapshot(&d);
    let count = objects.len();
    let stored_order = sketch.curves.iter().map(|c| c.id).collect();
    d.close().expect("close");
    Stored {
        sketch: row.id,
        stored_order,
        circles,
        height_mm,
        constraints,
        refs,
        objects: count,
    }
}
/// One stored rule in words, so a comparison reads as what it is.
fn rule_text(rule: SketchConstraintRule) -> String {
    match rule {
        SketchConstraintRule::Radius { curve, radius } => {
            format!("radius {} of {curve}", radius.value())
        }
        SketchConstraintRule::Fixed { point, x, y } => {
            format!(
                "fixed {} of {} ({}, {})",
                point.at.as_str(),
                point.curve,
                x.value(),
                y.value()
            )
        }
        SketchConstraintRule::Coincident { a, b }
            if a.at == SketchPointSelector::Center && b.at == SketchPointSelector::Center =>
        {
            let (a, b) = if a.curve <= b.curve {
                (a.curve, b.curve)
            } else {
                (b.curve, a.curve)
            };
            format!("concentric {a} {b}")
        }
        other => format!("{other:?}"),
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

/// The solid is the ring those numbers describe, each wall under its own ref.
fn analytic_matches(
    a: &Analytic,
    outer_curve: ferritecad_types::StableEntityId,
    inner_curve: ferritecad_types::StableEntityId,
    outer: f64,
    inner: f64,
    height: f64,
) {
    assert_eq!(a.faces, 4, "two cylindrical walls and two planar caps");
    for (curve, radius, which) in [
        (outer_curve, outer, "boundary"),
        (inner_curve, inner, "bore"),
    ] {
        assert_eq!(
            a.walls.get(&curve).map(Vec::as_slice),
            Some([ferritecad_kernel::FaceSurface::Cylinder { radius }].as_slice()),
            "the {which} circle's own reference names its solved wall"
        );
    }
    assert_eq!(a.caps.len(), 2);
    assert!(
        a.caps
            .iter()
            .all(|s| *s == ferritecad_kernel::FaceSurface::Plane)
    );
    // The stable reference: pi (R - r)(R + r) h, which is not the subtraction
    // of two nearly equal squares.
    let exact = PI * (outer - inner) * (outer + inner) * height;
    assert!(
        (a.volume - exact).abs() < 1e-6 * exact,
        "analytic volume {} is not the annulus {exact}; a lost hole would be {}",
        a.volume,
        PI * outer * outer * height
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

/// Everything the exported mesh must be for the solved part to really be hollow.
///
/// A mesh is an inscribed prism, so nothing here demands an analytic pi of it:
/// the volume is compared with the area its *own* measured perimeters enclose,
/// and only bounded above by the exact solid it approximates.
fn check_annular_mesh(m: &Mesh, center: [f64; 2], outer: f64, inner: f64, height: f64) {
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
        "a directed edge is used more than once; the mesh is not one oriented surface"
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

/// The measured slice: concentricity, two radii and one pin drive the ring.
#[test]
fn native_concentric_radii_and_a_pinned_centre_drive_the_hollow_solid() {
    if !native() {
        return;
    }
    let f = Fixture::annulus();
    let before = std::fs::read(&f.source).expect("source bytes");
    let saved = stored(&f.source);

    let boundary = f.boundary().to_owned();
    let bore = f.bore().to_owned();
    let boundary_id: ferritecad_types::StableEntityId = boundary.parse().expect("UUID");
    let bore_id: ferritecad_types::StableEntityId = bore.parse().expect("UUID");

    // One request: the two circles share a centre, each has its own radius, and
    // the pair's one pin puts that shared centre somewhere the stored guess is
    // not. Only the boundary's centre is named; the bore follows through the
    // concentricity, which is the whole point.
    let sized = f.root.path().join("sized.fcad");
    f.ask(
        &[],
        &[
            concentric_add(&boundary, &bore),
            radius_add(&boundary, 6.75),
            radius_add(&bore, 2.125),
            center_add(&boundary, -3.5, 4.25),
        ],
    );
    let published = reply(f.edit(&sized).output().expect("process"), OP, 0);
    let result = &published["result"];

    assert_eq!(
        transaction::solve(&sized)["degrees_of_freedom"],
        0,
        "six unknowns, six constraints' worth of removal: {result:?}"
    );
    assert_eq!(
        transaction::solve(&sized)["redundant_constraint_ids"],
        json!([])
    );
    let listed = inspect(&sized);
    let added = listed["sketches"][0]["constraints"]
        .as_array()
        .expect("constraints");
    assert_eq!(added.len(), 4);
    assert_eq!(
        added
            .iter()
            .map(|c| c["constraint_id"].as_str().expect("id"))
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4
    );
    assert_eq!(
        std::fs::read(&f.source).expect("source bytes"),
        before,
        "the source was touched"
    );

    // The stored sketch is still the starting guess, and both roles are intact.
    let kept = stored(&sized);

    assert_eq!(kept.stored_order, saved.stored_order, "curve order moved");
    assert_eq!(
        kept.circles, saved.circles,
        "solved values overwrote the guess"
    );
    assert_eq!(kept.refs, saved.refs);
    assert_eq!(kept.objects, saved.objects);
    assert_eq!(kept.height_mm, 15.0);
    let copy = inspect(&sized);
    assert_eq!(
        copy["sketches"][0]["curves"],
        f.catalog["sketches"][0]["curves"]
    );
    let rules: Vec<String> = kept.constraints.iter().map(|c| c.1.clone()).collect();
    let (a, b) = if boundary_id <= bore_id {
        (boundary_id, bore_id)
    } else {
        (bore_id, boundary_id)
    };
    for expected in [
        format!("radius 6.75 of {boundary_id}"),
        format!("radius 2.125 of {bore_id}"),
        format!("fixed center of {boundary_id} (-3.5, 4.25)"),
        format!("concentric {a} {b}"),
    ] {
        assert!(
            rules.contains(&expected),
            "{expected} missing from {rules:?}"
        );
    }

    // Validation, a cold rebuild and every stored name.
    let checked = cli()
        .arg("validate")
        .arg(&sized)
        .arg("--json")
        .output()
        .expect("validate");
    assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
    let rebuilt = cli()
        .arg("rebuild")
        .arg(&sized)
        .arg("--cold")
        .output()
        .expect("rebuild");
    let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
    assert!(text.contains("solid, 4 named faces"), "{text}");
    assert!(
        text.lines()
            .filter(|line| line.contains(" kind=face "))
            .count()
            == 4,
        "{text}"
    );

    // The B-Rep, each wall under the reference carrying its own circle's UUID.
    let solved = analytic(&sized);
    analytic_matches(&solved, boundary_id, bore_id, 6.75, 2.125, 15.0);
    only_the_constraint_payload_changed(&f.source, &sized, saved.sketch);

    // And the exported bytes, read independently.
    let stl = f.root.path().join("sized.stl");
    check_annular_mesh(&mesh(&sized, &stl), [-3.5, 4.25], 6.75, 2.125, 15.0);
    let fbx = f.root.path().join("sized.fbx");
    let written = cli()
        .arg("export-fbx")
        .arg(&sized)
        .arg("-o")
        .arg(&fbx)
        .arg("--json")
        .output()
        .expect("FBX");
    let report = reply(written, "export-fbx", 0);
    assert_eq!(report["result"]["complete"], true);
    assert_eq!(report["result"]["geometries"], 1);
    keep(&fbx, "annular-constraint-cli.fbx");

    // Replacing one radius keeps the concentricity, the pin and every other
    // identity, and really changes the solid.
    let catalog = inspect(&sized);
    let listed = catalog["sketches"][0]["constraints"]
        .as_array()
        .expect("stored")
        .clone();
    let id_of = |pick: &dyn Fn(&Value) -> bool| -> String {
        listed
            .iter()
            .find(|c| pick(&c["rule"]))
            .expect("a stored rule")["constraint_id"]
            .as_str()
            .expect("UUID")
            .to_owned()
    };
    let bore_radius = id_of(&|r: &Value| r["kind"] == "radius" && r["curve_id"] == bore);
    let pin = id_of(&|r: &Value| r["kind"] == "fixed");
    let concentric = id_of(&|r: &Value| r["kind"] == "coincident");
    let widened = f.root.path().join("widened.fcad");
    f.ask(&[json!(bore_radius)], &[radius_add(&bore, 3.5)]);
    let _again = reply(
        f.edit_from(&sized, &catalog, &widened)
            .output()
            .expect("process"),
        OP,
        0,
    );

    assert_eq!(transaction::solve(&widened)["degrees_of_freedom"], 0);
    let wider = inspect(&widened)["sketches"][0]["constraints"]
        .as_array()
        .expect("stored")
        .iter()
        .map(|c| c["constraint_id"].as_str().expect("UUID").to_owned())
        .collect::<Vec<_>>();
    for survivor in [&pin, &concentric] {
        assert!(
            wider.contains(survivor),
            "{survivor} lost its identity in a radius replacement"
        );
    }
    analytic_matches(&analytic(&widened), boundary_id, bore_id, 6.75, 3.5, 15.0);

    // Real cache Miss then Hit, so the new numbers are not a stale entry.
    let cache = f.root.path().join("widened.fcad-cache");
    analytic_with_cache(
        &widened,
        Some((&cache, ferritecad_eval::CacheOutcome::Miss)),
    );
    analytic_with_cache(&widened, Some((&cache, ferritecad_eval::CacheOutcome::Hit)));

    // Removing concentricity leaves these circles disjoint: the pinned centre
    // is far from the other circle's stored guess. NewBody still requires one
    // solid, so the feature is refused and nothing is published.
    let catalog = inspect(&widened);
    let never = f.root.path().join("never.fcad");
    let present = entries(f.root.path());
    f.ask(&[json!(concentric)], &[]);
    let split = reply(
        f.edit_from(&widened, &catalog, &never)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert_eq!(split["error"]["kind"], "unsupported", "{split:?}");
    assert!(
        split["error"]["message"]
            .as_str()
            .expect("diagnostic")
            .contains("sweep into 2 separate solids, and a new body must be one solid"),
        "{split:?}"
    );
    assert_eq!(entries(f.root.path()), present, "a refusal left something");

    // Taking the pin off with it leaves both centres at the stored guess they
    // share, so the same removal publishes — and the freedom really came back,
    // measured rather than assumed.
    let freed = f.root.path().join("freed.fcad");
    f.ask(&[json!(concentric), json!(pin)], &[]);
    let loosened = reply(
        f.edit_from(&widened, &catalog, &freed)
            .output()
            .expect("process"),
        OP,
        0,
    );
    assert_eq!(
        transaction::solve(&freed)["degrees_of_freedom"],
        4,
        "two centres are free again, and the two radii are not: {loosened:?}"
    );
    let catalog = inspect(&freed);
    assert_eq!(
        catalog["sketches"][0]["constraints"]
            .as_array()
            .expect("stored")
            .len(),
        2
    );
    // The two radii are still the solved ones; the centres fell back to the
    // saved approximation, which is what this slice promises and all it does.
    analytic_matches(&analytic(&freed), boundary_id, bore_id, 6.75, 3.5, 15.0);

    // Taking everything off hands the profile back to the annulus editor.
    let ids: Vec<Value> = catalog["sketches"][0]["constraints"]
        .as_array()
        .expect("stored")
        .iter()
        .map(|c| c["constraint_id"].clone())
        .collect();
    let plain = f.root.path().join("plain.fcad");
    f.ask(&ids, &[]);
    let _gone = reply(
        f.edit_from(&freed, &catalog, &plain)
            .output()
            .expect("process"),
        OP,
        0,
    );
    assert!(
        transaction::solve(&plain).is_null(),
        "nothing left to solve is not zero freedom"
    );
    let bare = inspect(&plain);
    assert_eq!(bare["sketches"][0]["constraints"], json!([]));

    analytic_matches(&analytic(&plain), boundary_id, bore_id, 10.0, 4.0, 15.0);

    // The existing height edit keeps the constraints and the solved radii.
    let taller = f.root.path().join("taller.fcad");
    let feature = inspect(&sized)["features"][0]["feature_id"]
        .as_str()
        .expect("feature")
        .to_owned();
    let raised = transaction::constant(
        &sized,
        &Document::open_read_only(&sized)
            .expect("document")
            .content_version()
            .expect("version")
            .to_string(),
        &feature,
        "extrude_distance",
        "25",
        "mm",
        &taller,
    )
    .output()
    .expect("edit");
    assert!(raised.status.success(), "{raised:?}");
    let up = inspect(&taller);
    assert_eq!(
        transaction::first_slot(&up, "extrude_distance")["expression"]["value"],
        25.0
    );
    assert_eq!(
        up["sketches"][0]["constraints"]
            .as_array()
            .expect("stored")
            .len(),
        4
    );
    analytic_matches(&analytic(&taller), boundary_id, bore_id, 6.75, 2.125, 25.0);
}

/// The same measured result from a document whose curves are stored the other
/// way round.
#[test]
fn native_stored_order_does_not_decide_the_roles() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(true);
    let saved = stored(&f.source);
    let boundary = f.boundary().to_owned();
    let bore = f.bore().to_owned();
    assert_eq!(
        saved.stored_order[0].to_string(),
        bore,
        "this fixture stores the bore first"
    );
    let boundary_id: ferritecad_types::StableEntityId = boundary.parse().expect("UUID");
    let bore_id: ferritecad_types::StableEntityId = bore.parse().expect("UUID");
    let sized = f.root.path().join("sized.fcad");
    f.ask(
        &[],
        &[
            concentric_add(&bore, &boundary),
            radius_add(&boundary, 6.75),
            radius_add(&bore, 2.125),
            center_add(&bore, -3.5, 4.25),
        ],
    );
    let _published = reply(f.edit(&sized).output().expect("process"), OP, 0);
    assert_eq!(transaction::solve(&sized)["degrees_of_freedom"], 0);
    let kept = stored(&sized);
    assert_eq!(kept.stored_order, saved.stored_order);
    assert_eq!(kept.circles, saved.circles);
    analytic_matches(&analytic(&sized), boundary_id, bore_id, 6.75, 2.125, 15.0);
    let stl = f.root.path().join("sized.stl");
    check_annular_mesh(&mesh(&sized, &stl), [-3.5, 4.25], 6.75, 2.125, 15.0);
}

/// Intersecting or coincident solved boundaries publish nothing.
#[test]
fn native_annular_constraint_refusals_are_atomic() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(false);
    let before = std::fs::read(&f.source).expect("source bytes");
    let boundary = f.boundary().to_owned();
    let bore = f.bore().to_owned();
    let never = f.root.path().join("never.fcad");
    f.ask(&[], &[radius_add(&boundary, 5.0)]);
    let names = entries(f.root.path());
    for add in [
        vec![
            concentric_add(&boundary, &bore),
            radius_add(&boundary, 6.),
            radius_add(&bore, 6.),
            center_add(&boundary, 0., 0.),
        ],
        vec![
            radius_add(&boundary, 6.),
            radius_add(&bore, 4.),
            center_add(&boundary, 0., 0.),
            center_add(&bore, 5., 0.),
        ],
    ] {
        f.ask(&[], &add);
        let refused = reply(f.edit(&never).output().expect("process"), OP, 2);
        let diagnostic = refused.to_string();
        assert!(diagnostic.contains(&boundary), "{diagnostic}");
        assert!(diagnostic.contains(&bore), "{diagnostic}");
    }
    assert_eq!(entries(f.root.path()), names, "a refusal left something");
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
}

#[test]
fn native_solved_regions_have_no_saved_circle_roles_or_template_radius_bound() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(false);
    let boundary = f.boundary();
    let bore = f.bore();
    for (index, outer, inner, offset, swap) in [
        (0, 10., 2., 0., true),
        (1, 6.75, 2.125, 1., false),
        (2, 2.0e6, 1., 0., false),
    ] {
        let (outer_id, inner_id) = if swap {
            (bore, boundary)
        } else {
            (boundary, bore)
        };
        f.ask(
            &[],
            &[
                radius_add(outer_id, outer),
                radius_add(inner_id, inner),
                center_add(outer_id, 0., 0.),
                center_add(inner_id, offset, 0.),
            ],
        );
        let out = f.root.path().join(format!("region-{index}.fcad"));
        reply(f.edit(&out).output().expect("process"), OP, 0);
        analytic_matches(
            &analytic(&out),
            outer_id.parse().expect("curve"),
            inner_id.parse().expect("curve"),
            outer,
            inner,
            15.,
        );
        assert_eq!(
            stored(&out).circles,
            stored(&f.source).circles,
            "the solver never rewrites the stored approximation"
        );
    }
}

/// A build with a kernel and no solver builds rings and refuses to constrain
/// them, and the refusal publishes nothing.
#[cfg(not(feature = "planegcs"))]
#[test]
fn occt_without_solver_builds_plain_annuli_and_refuses_annular_constraints() {
    if !ferritecad_occt::is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    assert!(
        !ferritecad_eval::solver_available(),
        "the mixed gate must not link a solver"
    );
    let f = Fixture::annulus();
    let before = std::fs::read(&f.source).expect("source bytes");
    // The unconstrained ring still builds and still measures.
    let saved = stored(&f.source);
    let mut ordered: Vec<_> = saved.circles.iter().map(|(id, v)| (*id, v.1)).collect();
    ordered.sort_by(|a, b| b.1.total_cmp(&a.1));
    analytic_matches(
        &analytic(&f.source),
        ordered[0].0,
        ordered[1].0,
        10.0,
        4.0,
        15.0,
    );
    // Adding a constraint needs a solver, and says so without publishing.
    let never = f.root.path().join("never.fcad");
    f.ask(
        &[],
        &[
            concentric_add(f.boundary(), f.bore()),
            radius_add(f.boundary(), 6.75),
        ],
    );
    let names = entries(f.root.path());
    let refused = reply(f.edit(&never).output().expect("process"), OP, 2);
    assert_eq!(refused["error"]["kind"], "unsupported", "{refused:?}");
    assert_eq!(entries(f.root.path()), names);
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
}

fn diameter_add(curve: &str, mm: f64) -> Value {
    radius_add(curve, mm / 2.)
}

/// §31E, native: a diameter names its circle by UUID. The request lists the bore
/// before the boundary, and the document is run once with the boundary stored
/// first and once with the bore stored first; neither order decides who is who.
#[test]
fn native_diameter_on_each_circle_follows_the_uuid_not_the_order() {
    if !native() {
        return;
    }
    for bore_first in [false, true] {
        let f = Fixture::drawn(bore_first);
        let source_bytes = std::fs::read(&f.source).expect("source");
        let saved = stored(&f.source);
        let (boundary, bore) = (f.boundary().to_owned(), f.bore().to_owned());
        assert_eq!(saved.stored_order[0].to_string() == bore, bore_first);
        let (boundary_id, bore_id): (
            ferritecad_types::StableEntityId,
            ferritecad_types::StableEntityId,
        ) = (boundary.parse().expect("UUID"), bore.parse().expect("UUID"));

        let sized = f.root.path().join("sized.fcad");
        f.ask(
            &[],
            &[
                concentric_add(&boundary, &bore),
                diameter_add(&bore, 4.25),
                diameter_add(&boundary, 13.5),
                center_add(&boundary, -3.5, 4.25),
            ],
        );
        let _v = reply(f.edit(&sized).output().expect("process"), OP, 0);
        assert_eq!(transaction::solve(&sized)["degrees_of_freedom"], 0);
        // The response keeps calling them radii, with the fields it always had.
        let listed = inspect(&sized);
        let radii: Vec<(String, f64)> = listed["sketches"][0]["constraints"]
            .as_array()
            .expect("added")
            .iter()
            .filter(|c| c["rule"]["kind"] == "radius")
            .map(|c| {
                (
                    c["rule"]["curve_id"].as_str().expect("curve").to_owned(),
                    c["rule"]["radius"]["value"].as_f64().expect("radius"),
                )
            })
            .collect();
        assert_eq!(radii, [(bore.clone(), 2.125), (boundary.clone(), 6.75)]);

        let kept = stored(&sized);
        assert_eq!(kept.stored_order, saved.stored_order, "curve order moved");
        assert_eq!(kept.circles, saved.circles, "the guess was overwritten");
        assert_eq!(kept.refs, saved.refs);
        let rules: Vec<String> = kept.constraints.iter().map(|c| c.1.clone()).collect();
        for expected in [
            format!("radius 6.75 of {boundary_id}"),
            format!("radius 2.125 of {bore_id}"),
        ] {
            assert!(
                rules.contains(&expected),
                "{expected} missing from {rules:?}"
            );
        }
        analytic_matches(&analytic(&sized), boundary_id, bore_id, 6.75, 2.125, 15.0);
        if bore_first {
            check_annular_mesh(
                &mesh(&sized, &f.root.path().join("sized.stl")),
                [-3.5, 4.25],
                6.75,
                2.125,
                15.0,
            );
        }

        // Swapping solved sizes changes containment, not either curve's identity.
        let crossed = f.root.path().join("crossed.fcad");
        f.ask(
            &[],
            &[
                concentric_add(&boundary, &bore),
                diameter_add(&boundary, 4.25),
                diameter_add(&bore, 13.5),
                center_add(&boundary, -3.5, 4.25),
            ],
        );
        reply(f.edit(&crossed).output().expect("process"), OP, 0);
        analytic_matches(&analytic(&crossed), bore_id, boundary_id, 6.75, 2.125, 15.);
        let never = f.root.path().join("never.fcad");
        let names = entries(f.root.path());

        // One Radius slot per circle, whichever unit filled it; replacing the
        // bore's keeps the boundary's radius, the pair and the pin as they were.
        let catalog = inspect(&sized);
        let stored_rules = catalog["sketches"][0]["constraints"]
            .as_array()
            .expect("constraints")
            .clone();
        let bore_radius = stored_rules
            .iter()
            .find(|c| c["rule"]["kind"] == "radius" && c["rule"]["curve_id"] == bore)
            .expect("the bore's Radius")["constraint_id"]
            .clone();
        f.ask(&[], &[diameter_add(&bore, 5.5)]);
        reply(
            f.edit_from(&sized, &catalog, &never)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert_eq!(entries(f.root.path()), names, "the occupied slot published");
        let replaced = f.root.path().join("replaced.fcad");
        f.ask(
            std::slice::from_ref(&bore_radius),
            &[diameter_add(&bore, 5.5)],
        );
        let _v = reply(
            f.edit_from(&sized, &catalog, &replaced)
                .output()
                .expect("process"),
            OP,
            0,
        );

        let after = stored(&replaced);
        let ids = |s: &Stored| -> std::collections::BTreeSet<_> {
            s.constraints.iter().map(|(id, _)| *id).collect()
        };
        assert_eq!(
            ids(&after).intersection(&ids(&kept)).count(),
            3,
            "three kept their UUIDs"
        );
        assert!(
            after
                .constraints
                .iter()
                .any(|c| c.1 == format!("radius 2.75 of {bore_id}")),
            "{:?}",
            after.constraints
        );
        analytic_matches(&analytic(&replaced), boundary_id, bore_id, 6.75, 2.75, 15.0);
        assert_eq!(std::fs::read(&f.source).expect("source"), source_bytes);
    }
}
