// SPDX-License-Identifier: MIT
//! §28A: one named Fillet on one vertical edge of a saved plate, in a copy.
//!
//! Everything is measured, not written down in advance: the removed volume and
//! the rounded face's surface, radius and axis from the B-Rep; the mesh from
//! the exported STL with its own reader; the history, names and SQL from the
//! document. The class is asserted from the other side too: every composition
//! outside it is refused by name, and a refusal leaves every file as it was.
#![allow(clippy::panic)]
use ferritecad_document::{Document, ObjectPayload, SemanticRole};
use ferritecad_kernel::{FaceSurface, GeometryKernel, OperationContext};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    f64::consts::PI,
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;

const OP: &str = "fillet-edge-copy";
/// The asymmetric, translated plate of the probe, with fractional sizes.
const X0: f64 = -4.5;
const Y0: f64 = 3.25;
const W: f64 = 37.5;
const D: f64 = 12.25;
const H: f64 = 6.75;
/// Counter-clockwise from the lower left.
const CCW: [[f64; 2]; 4] = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]];
/// Clockwise, starting at the upper right: the other winding and another
/// starting segment at once.
const CW_FROM_UPPER_RIGHT: [[f64; 2]; 4] = [[X0 + W, Y0 + D], [X0 + W, Y0], [X0, Y0], [X0, Y0 + D]];
/// Counter-clockwise again, but drawn from the third corner.
const CCW_FROM_THIRD: [[f64; 2]; 4] = [[X0 + W, Y0 + D], [X0, Y0 + D], [X0, Y0], [X0 + W, Y0]];
const LINEAR_MM: f64 = 0.01;
const ANGULAR_RAD: f64 = 0.05;
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
    if ferritecad_occt::is_available() {
        true
    } else {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: this build has no Open CASCADE");
        false
    }
}
fn refused(v: &Value) -> &str {
    v["error"]["kind"].as_str().expect("an error kind")
}

/// What a plate document is made of, written without a kernel.
#[derive(Clone, Copy)]
struct Plate {
    corners: [[f64; 2]; 4],
    height: f64,
    reversed: bool,
    symmetric: bool,
    constrained: bool,
    placement: bool,
    second_body: bool,
    cut_side: bool,
}
impl Plate {
    fn new(corners: [[f64; 2]; 4]) -> Self {
        Self {
            corners,
            height: H,
            reversed: false,
            symmetric: false,
            constrained: false,
            placement: false,
            second_body: false,
            cut_side: false,
        }
    }

    fn write(self, path: &Path) {
        use ferritecad_document::{
            Body, CapSide, DatumPlane, Dependency, DependencyRole, EndCondition, EntityKind,
            Expression, Extrude, Point2, SelectionRule, Sketch, SketchConstraint,
            SketchConstraintRule, SketchCurve, SketchGeometry, SketchPointRef, SketchPointSelector,
            SolidOperation, TopologyRef,
        };
        use ferritecad_types::{ObjectId, StableEntityId, Transform};
        let mut d = Document::create(path).expect("document");
        d.write(|w| {
            let bodies = if self.second_body { 2 } else { 1 };
            for copy in 0..bodies {
                let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
                let offset = 100.0 * f64::from(copy);
                let segments: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();
                let at = |i: usize| {
                    let [x, y] = self.corners[i % 4];
                    Point2::new(x + offset, y)
                };
                w.put_object(
                    plane,
                    None,
                    i64::from(copy) * 4,
                    Some("XY"),
                    &ObjectPayload::DatumPlane(DatumPlane {
                        placement: if self.placement {
                            Transform::from_translation(ferritecad_types::Vec3::new(0., 0., 5.)?)?
                        } else {
                            Transform::IDENTITY
                        },
                    }),
                )?;
                let curves = (0..4)
                    .map(|i| {
                        Ok(SketchCurve {
                            id: segments[i],
                            construction: false,
                            geometry: SketchGeometry::Line {
                                start: at(i)?,
                                end: at(i + 1)?,
                            },
                        })
                    })
                    .collect::<ferritecad_types::Result<Vec<_>>>()?;
                let constraints = if self.constrained {
                    vec![SketchConstraint {
                        id: StableEntityId::new(),
                        rule: SketchConstraintRule::Horizontal {
                            a: SketchPointRef::new(segments[0], SketchPointSelector::Start),
                            b: SketchPointRef::new(segments[0], SketchPointSelector::End),
                        },
                    }]
                } else {
                    Vec::new()
                };
                w.put_object(
                    sketch,
                    None,
                    i64::from(copy) * 4 + 1,
                    Some("Profile"),
                    &ObjectPayload::Sketch(Sketch {
                        plane,
                        curves,
                        constraints,
                    }),
                )?;
                let distance = Expression::constant(self.height)?;
                w.put_object(
                    extrude,
                    None,
                    i64::from(copy) * 4 + 2,
                    Some("Extrude1"),
                    &ObjectPayload::Extrude(Extrude {
                        profile: sketch,
                        end_condition: if self.symmetric {
                            EndCondition::Symmetric { distance }
                        } else {
                            EndCondition::Blind { distance }
                        },
                        reversed: self.reversed,
                        operation: SolidOperation::NewBody,
                        target_body: None,
                        previous: None,
                    }),
                )?;
                w.put_object(
                    body,
                    None,
                    i64::from(copy) * 4 + 3,
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
                if self.cut_side {
                    w.put_topology_ref(&TopologyRef {
                        id: StableEntityId::new(),
                        owner: extrude,
                        producer_feature: extrude,
                        expected_kind: EntityKind::Face,
                        output_role: SemanticRole::ExtrudeSide {
                            profile_segment: segments[0],
                        },
                        selection: SelectionRule::AllDerivedFrom {
                            ancestor: segments[0],
                        },
                        fallback_signature: None,
                    })?;
                }
            }
            Ok(())
        })
        .expect("plate document");
        d.close().expect("close");
    }
}

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    request: PathBuf,
    catalog: Value,
}
impl Fixture {
    fn drawn(plate: Plate) -> Self {
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("плита space.fcad");
        plate.write(&source);
        let catalog = inspect(&source);
        Self {
            request: root.path().join("request.json"),
            root,
            source,
            catalog,
        }
    }
    fn discovery(&self) -> &Value {
        &self.catalog["bodies"][0]["fillet_edge"]
    }
    fn body_id(&self) -> &str {
        self.catalog["bodies"][0]["body_id"]
            .as_str()
            .expect("body UUID")
    }
    fn version(&self) -> &str {
        self.catalog["content_version"].as_str().expect("version")
    }
    fn candidates(&self) -> &Vec<Value> {
        self.discovery()["target"]["candidates"]
            .as_array()
            .expect("candidates")
    }
    /// The candidate discovery lists at a corner, found by its label.
    fn at(&self, corner: [f64; 2]) -> &Value {
        self.candidates()
            .iter()
            .find(|c| c["corner_mm"] == json!(corner))
            .unwrap_or_else(|| panic!("no candidate at {corner:?}"))
    }
    fn ask_edge(&self, edge: &Value, radius: f64) {
        write(
            &self.request,
            &json!({"request_version":1,"edge":edge,"radius_mm":radius}),
        );
    }
    /// The candidate's own edge, with the pair written in the other order.
    fn ask_reversed(&self, corner: [f64; 2], radius: f64) {
        let edge = &self.at(corner)["edge"];
        self.ask_edge(
            &json!({"feature_id":edge["feature_id"],
                    "joint":[edge["joint"][1], edge["joint"][0]]}),
            radius,
        );
    }
    fn fillet(&self, output: &Path) -> Command {
        self.fillet_from(&self.source, self.body_id(), self.version(), output)
    }
    fn fillet_from(&self, source: &Path, body: &str, version: &str, output: &Path) -> Command {
        let mut c = cli();
        c.arg(OP)
            .arg(source)
            .arg("--body")
            .arg(body)
            .arg("--expect-version")
            .arg(version)
            .arg("--request")
            .arg(&self.request)
            .arg("-o")
            .arg(output)
            .arg("--json");
        c
    }
}

/// What the real kernel says about the solid a document rebuilds into, read
/// through the document's own references.
#[derive(Debug, Clone)]
struct Measured {
    faces: u64,
    volume: f64,
    /// The one face the Fillet's own reference resolves to.
    fillet: FaceSurface,
    axis_origin: [f64; 3],
    axis_direction: [f64; 3],
    /// Every reference, by role and producer, with the surfaces it names.
    roles: BTreeMap<String, Vec<FaceSurface>>,
    shapes: usize,
}
impl Measured {
    fn same_as(&self, other: &Self) {
        assert_eq!(self.faces, other.faces);
        assert!((self.volume - other.volume).abs() < 1e-9 * self.volume);
        assert_eq!(self.fillet, other.fillet);
        for k in 0..3 {
            assert!((self.axis_origin[k] - other.axis_origin[k]).abs() < 1e-9);
        }
        assert_eq!(self.roles, other.roles);
        assert_eq!(self.shapes, other.shapes);
    }
}

fn role_name(role: &SemanticRole) -> String {
    match role {
        SemanticRole::ExtrudeCap { side } => format!("cap {side:?}"),
        SemanticRole::ExtrudeSide { .. } => "side".to_owned(),
        SemanticRole::OriginCap { side, .. } => format!("origin cap {side:?}"),
        SemanticRole::OriginSide { .. } => "origin side".to_owned(),
        SemanticRole::EdgeFilletFace { .. } => "edge fillet face".to_owned(),
        other => panic!("unexpected role {other:?}"),
    }
}

fn measure(path: &Path, cache: Option<(&Path, ferritecad_eval::CacheOutcome)>) -> Measured {
    let (measured, events) = measure_events(path, cache.map(|(p, _)| p));
    if let Some((_, expected)) = cache {
        assert!(
            !events.is_empty() && events.iter().all(|e| e.outcome == expected),
            "every feature was expected to {expected:?}: {events:?}"
        );
    }
    measured
}

/// [`measure`], returning the cache events for the caller to judge feature
/// by feature.
fn measure_events(
    path: &Path,
    cache: Option<&Path>,
) -> (Measured, Vec<ferritecad_eval::CacheEvent>) {
    let doc = Document::open_read_only(path).expect("reopen");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let mut cache_events = Vec::new();
    let built = if let Some(cache_path) = cache {
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
        cache_events = events;
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
    let tip = built.shape(body.id).expect("the body was built");
    let mut roles: BTreeMap<String, Vec<FaceSurface>> = BTreeMap::new();
    let mut fillet = None;
    for reference in &doc.topology_refs().expect("refs") {
        let resolved = built.resolve(reference).expect("resolve");
        assert!(!resolved.is_empty(), "{} resolved to nothing", reference.id);
        // A reference keeps addressing its own producer's output: the base's
        // names stay on the base, and only the Fillet's are on the tip.
        let producer = built
            .shape(reference.producer_feature)
            .expect("the producer was built");
        for handle in &resolved {
            assert_eq!(handle.shape(), producer, "{} was repointed", reference.id);
        }
        let producer_kind = objects
            .iter()
            .find(|o| o.id == reference.producer_feature)
            .expect("producer")
            .payload
            .type_name();
        let key = format!("{} of {producer_kind}", role_name(&reference.output_role));
        for handle in &resolved {
            roles
                .entry(key.clone())
                .or_default()
                .push(kernel.face_surface(*handle).expect("surface"));
        }
        if matches!(reference.output_role, SemanticRole::EdgeFilletFace { .. }) {
            assert_eq!(resolved.len(), 1, "the rounded face is exactly one face");
            assert_eq!(producer, tip, "the Fillet's face is on the Body's tip");
            fillet = Some(resolved[0]);
        }
    }
    for surfaces in roles.values_mut() {
        surfaces.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    }
    let face = fillet.expect("the document names the rounded face");
    let (axis_origin, axis_direction) = kernel.cylinder_axis(face).expect("axis");
    let (faces, volume) = kernel.shape_stats(tip).expect("stats");
    let measured = Measured {
        faces,
        volume,
        fillet: kernel.face_surface(face).expect("surface"),
        axis_origin,
        axis_direction,
        roles,
        shapes: built.shape_count(),
    };
    built.release_all(&mut kernel);
    doc.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0, "every handle was released");
    (measured, cache_events)
}

/// An independent reading of the exported mesh.
struct Mesh {
    triangles: usize,
    lo: [f64; 3],
    hi: [f64; 3],
    volume: f64,
    faces: Vec<[[f64; 3]; 3]>,
}
fn read_stl(bytes: &[u8]) -> Mesh {
    let count = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + 50 * count);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut six = 0.;
    let mut faces = Vec::new();
    for t in bytes[84..].chunks_exact(50) {
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
    Mesh {
        triangles: count,
        lo,
        hi,
        volume: six / 6.,
        faces,
    }
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
    let m = read_stl(&std::fs::read(out).expect("STL bytes"));
    assert_eq!(report["result"]["triangles"], m.triangles);
    m
}

/// Everything the exported mesh must be for exactly this corner to be rounded
/// at this radius, and nothing else touched.
fn check_mesh(m: &Mesh, corner: [f64; 2], r: f64, height: f64) {
    check_mesh_in(m, [X0, Y0, W, D], corner, r, height);
}

/// [`check_mesh`] for the plate `[x0, y0, width, depth]` (§28D moves it).
fn check_mesh_in(m: &Mesh, [x0, y0, w, d]: [f64; 4], corner: [f64; 2], r: f64, height: f64) {
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
    // The part is still the part.
    for (k, (lo, hi)) in [(x0, x0 + w), (y0, y0 + d), (0.0, height)]
        .into_iter()
        .enumerate()
    {
        assert!((m.lo[k] - lo).abs() < ROUNDING_MM && (m.hi[k] - hi).abs() < ROUNDING_MM);
    }
    // The rounded corner has no vertex; the other three still do, top and
    // bottom. Changing the selection changes exactly this.
    let vertices: Vec<[f64; 3]> = m.faces.iter().flatten().copied().collect();
    let has = |x: f64, y: f64, z: f64| {
        vertices.iter().any(|v| {
            (v[0] - x).abs() < ROUNDING_MM
                && (v[1] - y).abs() < ROUNDING_MM
                && (v[2] - z).abs() < ROUNDING_MM
        })
    };
    for c in [[x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]] {
        for z in [0.0, height] {
            assert_eq!(
                has(c[0], c[1], z),
                c != corner,
                "corner {c:?} at z={z}: rounded is {corner:?}"
            );
        }
    }
    // Every vertex inside the corner's r x r square lies on the arc, about the
    // centre r inward of the corner, from z = 0 to the top.
    let centre = inward_in([x0, y0], corner, r);
    let inside = |v: &[f64; 3]| {
        (v[0] - corner[0]).abs() < r - ROUNDING_MM && (v[1] - corner[1]).abs() < r - ROUNDING_MM
    };
    let on_arc: Vec<_> = vertices.iter().filter(|v| inside(v)).collect();
    assert!(on_arc.len() >= 4, "the mesh has no fillet wall");
    for v in &on_arc {
        let d = (v[0] - centre[0]).hypot(v[1] - centre[1]);
        assert!(
            (d - r).abs() < 10. * ROUNDING_MM,
            "{v:?} is {d} from the axis"
        );
    }
    assert!(on_arc.iter().any(|v| v[2].abs() < ROUNDING_MM));
    assert!(on_arc.iter().any(|v| (v[2] - height).abs() < ROUNDING_MM));
    // Chords of a convex arc lie inside it, so a mesh removes a little more
    // than the exact fillet, never less, and not more than its sagitta allows.
    let exact = w * d * height - (1. - PI / 4.) * r * r * height;
    let slack = PI / 2. * r * LINEAR_MM * height;
    assert!(
        m.volume <= exact + 1e-6 * exact && m.volume >= exact - slack - 1e-6 * exact,
        "STL volume {} is not a mesh of {exact}",
        m.volume
    );
}

/// The axis of the fillet at a corner: r inward along both sides.
fn inward(corner: [f64; 2], r: f64) -> [f64; 2] {
    inward_in([X0, Y0], corner, r)
}

/// [`inward`] for a plate whose lower-left corner is `[x0, y0]`.
fn inward_in([x0, y0]: [f64; 2], corner: [f64; 2], r: f64) -> [f64; 2] {
    [
        corner[0] + if corner[0] == x0 { r } else { -r },
        corner[1] + if corner[1] == y0 { r } else { -r },
    ]
}

/// FBX, complete, and both exports kept for the pinned reader when asked.
fn fbx(path: &Path, artifact: &str) {
    let out = path.with_extension("fbx");
    let r = cli()
        .arg("export-fbx")
        .arg(path)
        .arg("-o")
        .arg(&out)
        .arg("--json")
        .output()
        .expect("FBX");
    assert!(r.status.success(), "{r:?}");
    let v: Value = serde_json::from_slice(&r.stdout).expect("JSON");
    assert_eq!(v["result"]["complete"], true, "{v}");
    let text = std::fs::read(&out).expect("FBX bytes");
    assert!(text.starts_with(b"; FBX 7.4.0 project file"));
    if let Some(dir) = std::env::var_os("FCAD_FILLET_ARTIFACTS") {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).expect("artifacts");
        // Both at the default tessellation, the only one export-fbx has, so
        // the CI join compares one mesh.
        for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
            let r = cli()
                .arg(op)
                .arg(path)
                .arg("-o")
                .arg(dir.join(format!("{artifact}.{extension}")))
                .arg("--json")
                .output()
                .expect("artifact");
            assert!(r.status.success(), "{r:?}");
        }
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
fn capability_names(path: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("SQL");
    c.prepare("SELECT name FROM capabilities ORDER BY rowid")
        .expect("names")
        .query_map([], |r| r.get(0))
        .expect("names")
        .collect::<rusqlite::Result<_>>()
        .expect("names")
}

/// Every SQL cell of the source survives into the copy, and the new cells are
/// exactly the ones a Fillet is allowed to add: one object, the Body's tip,
/// the predecessor and tip edges, seven names, capabilities, and the copy's
/// own timestamp.
fn only_the_fillet_was_added(source: &Path, copy: &Path, body: ferritecad_types::ObjectId) {
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
                    "topology_refs" => assert_eq!(added, 7, "seven new names"),
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
    assert!(new.contains(&"feature.fillet.v1".to_owned()));
    for name in &new {
        assert!(
            old.contains(name)
                || matches!(
                    name.as_str(),
                    "feature.fillet.v1" | "topology.origin-face.v1" | "feature.predecessor.v1"
                ),
            "capability {name} was added"
        );
    }
}

/// Discovery, the strict request protocol and every structural refusal, with
/// no kernel needed.
#[test]
fn fillet_discovery_and_protocol_without_native() {
    let f = Fixture::drawn(Plate::new(CCW));
    let before = std::fs::read(&f.source).expect("source bytes");

    let discovery = f.discovery();
    assert_eq!(discovery["available"], true, "{discovery:?}");
    assert!(discovery["refusal"].is_null());
    assert!(discovery["document_refusal"].is_null());
    let target = &discovery["target"];
    assert_eq!(target["body_id"], f.body_id());
    assert_eq!(target["height_mm"], H);
    assert_eq!(target["request_versions"], json!([1]));
    assert_eq!(target["min_radius_mm"], 0.01);
    assert_eq!(target["max_radius_fraction"], 0.5);
    let base = target["base_feature_id"].as_str().expect("base").to_owned();
    let candidates = f.candidates();
    assert_eq!(candidates.len(), 4, "four vertical edges");
    let mut corners: Vec<String> = Vec::new();
    let mut lines = std::collections::BTreeSet::new();
    for candidate in candidates {
        assert_eq!(candidate["edge"]["feature_id"], base.as_str());
        let joint = candidate["edge"]["joint"].as_array().expect("pair");
        assert_eq!(joint.len(), 2);
        let [a, b] = [0, 1].map(|i| {
            joint[i]
                .as_str()
                .expect("UUID")
                .parse::<ferritecad_types::StableEntityId>()
                .expect("a real curve UUID")
        });
        assert!(a < b, "the pair is listed in canonical order");
        lines.insert(a);
        lines.insert(b);
        let mut lengths: Vec<f64> = candidate["adjacent_lengths_mm"]
            .as_array()
            .expect("lengths")
            .iter()
            .map(|l| l.as_f64().expect("length"))
            .collect();
        lengths.sort_by(f64::total_cmp);
        assert_eq!(lengths, [D, W]);
        assert_eq!(candidate["max_radius_mm"], D / 2.);
        assert!(
            candidate["label"]
                .as_str()
                .expect("label")
                .contains(&a.to_string())
        );
        corners.push(candidate["corner_mm"].to_string());
    }
    assert_eq!(lines.len(), 4, "every Line meets two corners");
    corners.sort();
    let mut wanted: Vec<String> = CCW.iter().map(|c| json!(c).to_string()).collect();
    wanted.sort();
    assert_eq!(corners, wanted);

    let never = f.root.path().join("never.fcad");
    let corner = CCW[1];
    let edge = f.at(corner)["edge"].clone();
    f.ask_edge(&edge, 2.5);
    let names = entries(f.root.path());
    let joint = edge["joint"].clone();
    let far = f.at(CCW[3])["edge"]["joint"].clone();
    // Two Lines that do not meet: the corner's first Line and the far
    // corner's other one.
    let opposite = [joint[0].clone(), {
        if far[0] == joint[0] || far[0] == joint[1] {
            far[1].clone()
        } else {
            far[0].clone()
        }
    }];

    // The request itself: strict JSON v1, refused before anything is read.
    let raw = |body: &str| {
        std::fs::write(&f.request, body).expect("request");
        reply(f.fillet(&never).output().expect("process"), OP, 2)
    };
    let e = edge.to_string();
    for (why, body) in [
        (
            "an unknown key",
            format!(r#"{{"request_version":1,"edge":{e},"radius_mm":2.5,"chain":true}}"#),
        ),
        (
            "an unknown edge key",
            format!(
                r#"{{"request_version":1,"edge":{{"feature_id":{},"joint":{},"index":0}},"radius_mm":2.5}}"#,
                edge["feature_id"], joint
            ),
        ),
        (
            "a duplicate key",
            format!(r#"{{"request_version":1,"edge":{e},"radius_mm":2.5,"radius_mm":3}}"#),
        ),
        (
            "an escaped duplicate",
            format!(r#"{{"request_version":1,"edge":{e},"radius_mm":2.5,"radius\u005fmm":3}}"#),
        ),
        (
            "an edge as an array",
            format!(
                r#"{{"request_version":1,"edge":[{},{}],"radius_mm":2.5}}"#,
                edge["feature_id"], joint
            ),
        ),
        ("a request as an array", format!(r#"[1,{e},2.5]"#)),
        (
            "three Lines",
            format!(
                r#"{{"request_version":1,"edge":{{"feature_id":{},"joint":[{},{},{}]}},"radius_mm":2.5}}"#,
                edge["feature_id"], joint[0], joint[1], joint[0]
            ),
        ),
        (
            "one Line twice",
            format!(
                r#"{{"request_version":1,"edge":{{"feature_id":{},"joint":[{},{}]}},"radius_mm":2.5}}"#,
                edge["feature_id"], joint[0], joint[0]
            ),
        ),
        (
            "a missing radius",
            format!(r#"{{"request_version":1,"edge":{e}}}"#),
        ),
        (
            "a radius as text",
            format!(r#"{{"request_version":1,"edge":{e},"radius_mm":"2.5"}}"#),
        ),
        (
            "an infinite radius",
            format!(r#"{{"request_version":1,"edge":{e},"radius_mm":1e999}}"#),
        ),
        (
            "an index",
            r#"{"request_version":1,"edge":0,"radius_mm":2.5}"#.to_owned(),
        ),
        (
            "coordinates",
            r#"{"request_version":1,"edge":{"corner_mm":[33,3.25]},"radius_mm":2.5}"#.to_owned(),
        ),
    ] {
        let v = raw(&body);
        assert_eq!(refused(&v), "input", "{why}: {v}");
    }
    let v = raw(&format!(
        r#"{{"request_version":2,"edge":{e},"radius_mm":2.5}}"#
    ));
    assert_eq!(refused(&v), "unsupported", "a future request version: {v}");
    let v = raw(&format!(
        r#"{{"request_version":1,"edge":{e},"radius_mm":2.5,"pad":"{}"}}"#,
        "x".repeat(70_000)
    ));
    assert_eq!(refused(&v), "input", "an oversized request: {v}");

    // Structural refusals, each with every other argument correct. A build
    // with no kernel answers `unsupported` before it reaches the geometry,
    // which is a different true thing about the same refusal.
    let fabricated = ferritecad_types::StableEntityId::new().to_string();
    for (why, asked, radius) in [
        ("no radius", edge.clone(), 0.0),
        ("a negative radius", edge.clone(), -1.0),
        ("below the measured floor", edge.clone(), 0.005),
        ("past half the shorter side", edge.clone(), D / 2. + 0.01),
        ("the kernel's own limit", edge.clone(), D),
        (
            "a foreign feature",
            json!({"feature_id": ferritecad_types::ObjectId::new().to_string(), "joint": joint}),
            2.5,
        ),
        (
            "the profile, not the Extrude",
            json!({"feature_id": target["profile_sketch_id"], "joint": joint}),
            2.5,
        ),
        (
            "two Lines that do not meet",
            json!({"feature_id": base, "joint": opposite}),
            2.5,
        ),
        (
            "a fabricated Line",
            json!({"feature_id": base, "joint": [joint[0], fabricated]}),
            2.5,
        ),
    ] {
        f.ask_edge(&asked, radius);
        let v = reply(f.fillet(&never).output().expect("process"), OP, 2);
        assert!(matches!(refused(&v), "input" | "unsupported"), "{why}: {v}");
    }
    // A body that is not there, and a version that is not this one.
    f.ask_edge(&edge, 2.5);
    let foreign = ferritecad_types::ObjectId::new().to_string();
    for (body, version) in [
        (foreign.as_str(), f.version()),
        (f.body_id(), &"0".repeat(64)),
    ] {
        let v = reply(
            f.fillet_from(&f.source, body, version, &never)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert!(matches!(refused(&v), "input" | "unsupported"), "{v}");
    }
    // A refused operation whose report cannot be written is still a refusal.
    f.ask_edge(&edge, 0.0);
    assert_eq!(
        f.fillet(&never)
            .stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
    assert_eq!(entries(f.root.path()), names, "a refusal left something");
    assert_eq!(
        std::fs::read(&f.source).expect("source bytes"),
        before,
        "the source was touched"
    );
}

/// Every composition outside the class is refused by discovery with its
/// reason, and by the operation, before anything is written.
#[test]
fn fillet_discovery_refuses_every_composition_outside_the_class_without_native() {
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
            "constrained",
            Plate {
                constrained: true,
                ..Plate::new(CCW)
            },
            "constrain",
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
        let f = Fixture::drawn(plate);
        let before = std::fs::read(&f.source).expect("source bytes");
        for body in f.catalog["bodies"].as_array().expect("bodies") {
            let d = &body["fillet_edge"];
            assert_eq!(d["available"], false, "{why}: {d}");
            assert!(d["target"].is_null(), "{why}");
            let reason = d["refusal"].as_str().expect("a reason");
            assert!(!reason.is_empty());
            assert!(reason.to_lowercase().contains(expect), "{why}: {reason}");
        }
        // Whatever the request, nothing is published.
        // The first Extrude and two adjacent Lines of its profile, read from
        // the document itself.
        let d = Document::open_read_only(&f.source).expect("open");
        let objects = d.objects().expect("objects");
        d.close().expect("close");
        let feature = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
            .expect("an Extrude")
            .id
            .to_string();
        let lines: Vec<String> = objects
            .iter()
            .find_map(|o| match &o.payload {
                ObjectPayload::Sketch(s) => {
                    Some(s.curves.iter().map(|c| c.id.to_string()).collect())
                }
                _ => None,
            })
            .expect("a Sketch");
        let joint = json!([lines[0], lines[1]]);
        f.ask_edge(&json!({"feature_id": feature, "joint": joint}), 1.0);
        let never = f.root.path().join("never.fcad");
        let names = entries(f.root.path());
        let v = reply(f.fillet(&never).output().expect("process"), OP, 2);
        assert!(matches!(refused(&v), "input" | "unsupported"), "{why}: {v}");
        assert_eq!(
            entries(f.root.path()),
            names,
            "{why}: a refusal left something"
        );
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before, "{why}");
    }
}

/// Publishes one fillet at `corner`, checks everything a copy must be, and
/// returns the copy with what the kernel measured.
fn fillet_at(f: &Fixture, corner: [f64; 2], r: f64, name: &str) -> (PathBuf, Measured) {
    let before = std::fs::read(&f.source).expect("source bytes");
    let source_refs = {
        let d = Document::open_read_only(&f.source).expect("source");
        let refs = d.topology_refs().expect("refs");
        d.close().expect("close");
        refs
    };
    let candidate = f.at(corner).clone();
    let copy = f.root.path().join(format!("{name}.fcad"));
    // The pair in the other order is the same edge.
    f.ask_reversed(corner, r);
    let published = reply(f.fillet(&copy).output().expect("process"), OP, 0);
    let result = &published["result"];
    assert_eq!(
        result["body_id"],
        f.body_id(),
        "the Body keeps its identity"
    );
    let base = f.discovery()["target"]["base_feature_id"].clone();
    assert_eq!(result["previous_feature_id"], base, "the tip was the plate");
    assert_eq!(result["edge"], candidate["edge"], "the canonical meaning");
    assert_eq!(result["corner_mm"], json!(corner));
    assert_eq!(result["radius_mm"], r);
    let references = result["references"].as_array().expect("references");
    assert_eq!(references.len(), 7);
    let mut roles: Vec<&str> = references
        .iter()
        .map(|r| r["role"].as_str().expect("role"))
        .collect();
    roles.sort_unstable();
    assert_eq!(
        roles,
        [
            "edge_fillet_face",
            "origin_cap",
            "origin_cap",
            "origin_side",
            "origin_side",
            "origin_side",
            "origin_side"
        ]
    );
    assert_eq!(
        std::fs::read(&f.source).expect("source bytes"),
        before,
        "the source was touched"
    );

    // The history the copy holds.
    let fillet_id: ferritecad_types::ObjectId = result["feature_id"]
        .as_str()
        .expect("feature")
        .parse()
        .expect("UUID");
    let base_id: ferritecad_types::ObjectId = base.as_str().expect("base").parse().expect("UUID");
    let body_id: ferritecad_types::ObjectId = f.body_id().parse().expect("UUID");
    let d = Document::open_read_only(&copy).expect("copy");
    let objects = d.objects().expect("objects");
    assert_eq!(objects.len(), 5);
    let saved = objects.iter().find(|o| o.id == fillet_id).expect("Fillet");
    let ObjectPayload::Fillet(stored) = &saved.payload else {
        panic!("a Fillet object");
    };
    assert_eq!(stored.previous, base_id);
    assert_eq!(stored.edge.feature, base_id);
    assert_eq!(stored.radius_mm, r);
    assert_eq!(
        json!(stored.edge.joint.segments().map(|s| s.to_string())),
        candidate["edge"]["joint"]
    );
    let body = objects.iter().find(|o| o.id == body_id).expect("Body");
    assert!(matches!(
        body.payload,
        ObjectPayload::Body(ferritecad_document::Body { tip_feature: Some(t) }) if t == fillet_id
    ));
    let refs = d.topology_refs().expect("refs");
    for reference in &source_refs {
        assert!(refs.contains(reference), "lost {}", reference.id);
    }
    assert_eq!(refs.len(), source_refs.len() + 7);
    for reference in refs.iter().filter(|r| !source_refs.contains(r)) {
        assert_eq!(reference.owner, fillet_id);
        assert_eq!(reference.producer_feature, fillet_id);
        match &reference.output_role {
            SemanticRole::EdgeFilletFace {
                edge_feature,
                joint,
            } => {
                assert_eq!(*edge_feature, base_id);
                assert_eq!(*joint, stored.edge.joint);
            }
            SemanticRole::OriginCap { origin_feature, .. }
            | SemanticRole::OriginSide { origin_feature, .. } => {
                assert_eq!(*origin_feature, base_id);
            }
            other => panic!("unexpected new role {other:?}"),
        }
    }
    d.close().expect("close");
    only_the_fillet_was_added(&f.source, &copy, body_id);

    // Reopen, validate and rebuild cold through the shipped command.
    let checked = cli()
        .arg("validate")
        .arg(&copy)
        .arg("--json")
        .output()
        .expect("validate");
    assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
    let rebuilt = cli()
        .arg("rebuild")
        .arg(&copy)
        .arg("--cold")
        .output()
        .expect("rebuild");
    let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
    let n = source_refs.len() + 7;
    assert!(
        text.contains(&format!("{n} of {n} stored references resolved")),
        "{text}"
    );
    assert!(text.contains("tip Fillet"), "{text}");
    assert!(text.contains(&format!("r{r} mm")), "{text}");

    // The B-Rep: cold, then a real cache Miss and Hit, all the same solid.
    let cold = measure(&copy, None);
    let cache = copy.with_extension("fcad-cache");
    let miss = measure(&copy, Some((&cache, ferritecad_eval::CacheOutcome::Miss)));
    let hit = measure(&copy, Some((&cache, ferritecad_eval::CacheOutcome::Hit)));
    cold.same_as(&miss);
    cold.same_as(&hit);
    assert_eq!(cold.faces, 7, "two caps, four sides and one fillet face");
    let exact = W * D * H - (1. - PI / 4.) * r * r * H;
    assert!(
        (cold.volume - exact).abs() < 1e-9 * exact,
        "{} is not {exact}",
        cold.volume
    );
    assert_eq!(cold.fillet, FaceSurface::Cylinder { radius: r });
    let centre = inward(corner, r);
    assert!(
        (cold.axis_origin[0] - centre[0]).abs() < 1e-9
            && (cold.axis_origin[1] - centre[1]).abs() < 1e-9,
        "the axis {:?} is not r inward of {corner:?}",
        cold.axis_origin
    );
    assert!(cold.axis_direction[0].abs() < 1e-12 && cold.axis_direction[1].abs() < 1e-12);
    // Carried faces: both caps and all four sides are still planes, named at
    // the tip under the plate; the base's own names are still the base's.
    assert_eq!(
        cold.roles["origin cap Start of feature.fillet"],
        [FaceSurface::Plane]
    );
    assert_eq!(
        cold.roles["origin cap End of feature.fillet"],
        [FaceSurface::Plane]
    );
    assert_eq!(
        cold.roles["origin side of feature.fillet"],
        [FaceSurface::Plane; 4]
    );
    assert_eq!(
        cold.roles["edge fillet face of feature.fillet"],
        [FaceSurface::Cylinder { radius: r }]
    );
    assert_eq!(
        cold.roles["cap Start of feature.extrude"],
        [FaceSurface::Plane]
    );
    assert_eq!(cold.shapes, 2, "the plate and the rounded plate");

    // The mesh, read independently; and the FBX.
    let m = mesh(&copy, &copy.with_extension("stl"));
    check_mesh(&m, corner, r, H);
    fbx(&copy, name);
    (copy, cold)
}

/// The measured slice: two different corners of an asymmetric, translated
/// plate, for both windings and another starting segment.
#[test]
fn native_two_corners_both_windings_and_starts_are_what_the_numbers_say() {
    if !native() {
        return;
    }
    for (label, corners) in [
        ("ccw", CCW),
        ("cw", CW_FROM_UPPER_RIGHT),
        ("third", CCW_FROM_THIRD),
    ] {
        let f = Fixture::drawn(Plate {
            cut_side: true,
            ..Plate::new(corners)
        });
        assert_eq!(f.discovery()["available"], true, "{label}");
        let (a, first) = fillet_at(&f, [X0 + W, Y0], 2.375, &format!("fillet-{label}-a"));
        let (b, second) = fillet_at(&f, [X0, Y0 + D], 3.0625, &format!("fillet-{label}-b"));
        // Changing the selection changes the corner, not only the numbers.
        assert!((first.axis_origin[0] - second.axis_origin[0]).abs() > 1.0);
        assert_ne!(
            std::fs::read(a.with_extension("stl")).expect("a"),
            std::fs::read(b.with_extension("stl")).expect("b")
        );
    }
}

/// The cached fillet belongs to the plate it rounded: changing the plate
/// changes its key, and the rebuilt part is the new plate rounded.
#[test]
fn native_changing_the_plate_invalidates_the_cached_fillet() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate {
        cut_side: true,
        ..Plate::new(CCW)
    });
    let corner = [X0 + W, Y0 + D];
    let r = 4.5;
    let copy = f.root.path().join("rounded.fcad");
    f.ask_reversed(corner, r);
    reply(f.fillet(&copy).output().expect("process"), OP, 0);
    let cache = copy.with_extension("fcad-cache");
    let miss = measure(&copy, Some((&cache, ferritecad_eval::CacheOutcome::Miss)));
    measure(&copy, Some((&cache, ferritecad_eval::CacheOutcome::Hit))).same_as(&miss);

    // Every editor refuses a filleted part, so the plate's height is changed
    // under the document API, keeping every identity.
    let taller = 9.5;
    let mut d = Document::open(&copy).expect("writable");
    let base = d
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
        .expect("the plate");
    let ObjectPayload::Extrude(mut extrude) = base.payload.clone() else {
        panic!("an Extrude");
    };
    extrude.end_condition = ferritecad_document::EndCondition::Blind {
        distance: ferritecad_document::Expression::constant(taller).expect("height"),
    };
    d.write(|w| {
        w.put_object(
            base.id,
            base.parent,
            base.ordinal,
            base.name.as_deref(),
            &ObjectPayload::Extrude(extrude),
        )
        .map(|_| ())
    })
    .expect("taller plate");
    d.close().expect("close");

    // Both the plate and its fillet miss: a key of the fillet's own numbers
    // alone would have handed back the old, thinner part.
    let after = measure(&copy, Some((&cache, ferritecad_eval::CacheOutcome::Miss)));
    let exact = W * D * taller - (1. - PI / 4.) * r * r * taller;
    assert!(
        (after.volume - exact).abs() < 1e-9 * exact,
        "{} is not {exact}",
        after.volume
    );
    assert!((after.volume - miss.volume).abs() > 1.0);
    measure(&copy, None).same_as(&after);
    measure(&copy, Some((&cache, ferritecad_eval::CacheOutcome::Hit))).same_as(&after);
}

/// A fillet that cannot be what it claims publishes nothing, and a
/// cancelled one leaves nothing behind.
#[test]
fn native_fillet_refusals_and_cancellation_are_atomic() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let before = std::fs::read(&f.source).expect("source bytes");
    let never = f.root.path().join("never.fcad");
    let corner = [X0, Y0];
    f.ask_reversed(corner, 2.0);

    // The source as its own output, an occupied output, and a hard link to
    // the source: each with every other argument correct.
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
        let v = reply(f.fillet(&destination).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "{why}: {v}");
    }
    assert_eq!(
        std::fs::read(&taken).expect("occupied"),
        b"another process owns this"
    );

    // Radii the policy refuses, with the kernel present: nothing is clamped
    // and the unmodified block is never returned.
    for radius in [0.0, 0.009, D / 2. + 1e-9, D, 100.0] {
        f.ask_reversed(corner, radius);
        let v = reply(f.fillet(&never).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "{radius}: {v}");
    }
    assert_eq!(entries(f.root.path()), names, "a refusal left something");

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
    stale.ask_reversed(corner, 2.0);
    let stale_out = stale.root.path().join("stale.fcad");
    let v = reply(
        stale
            .fillet_from(&stale.source, stale.body_id(), &old_version, &stale_out)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    assert!(!stale_out.exists());

    // Cancellation, before the job starts and at its last barrier.
    for at in [0.0, 0.95] {
        let destination = f.root.path().join(format!("cancelled-{at}.fcad"));
        let token = ferritecad_kernel::CancelToken::new();
        let stop = token.clone();
        if at == 0.0 {
            token.cancel();
        }
        let context = OperationContext::default()
            .with_cancel(token)
            .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                if fraction >= at {
                    stop.cancel();
                }
            }));
        let d = Document::open_read_only(&f.source).expect("source");
        let expected = ferritecad_document::DocumentVersion {
            document_id: d.meta().document_id,
            content: d.content_version().expect("version"),
        };
        d.close().expect("close");
        let corner_edge = &f.at(corner)["edge"];
        let request = ferritecad_jobs::EdgeFilletRequest {
            source: f.source.clone(),
            expected,
            body: f.body_id().parse().expect("UUID"),
            fillet: ferritecad_document::EdgeFillet {
                edge: ferritecad_document::FilletEdge {
                    feature: corner_edge["feature_id"]
                        .as_str()
                        .expect("feature")
                        .parse()
                        .expect("UUID"),
                    joint: ferritecad_types::ProfileJoint::new(
                        corner_edge["joint"][0]
                            .as_str()
                            .expect("a")
                            .parse()
                            .expect("UUID"),
                        corner_edge["joint"][1]
                            .as_str()
                            .expect("b")
                            .parse()
                            .expect("UUID"),
                    )
                    .expect("joint"),
                },
                radius_mm: 2.0,
            },
            destination: destination.clone(),
        };
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let result = ferritecad_jobs::fillet_edge_copy(&request, &mut kernel, &context);
        assert!(result.is_err(), "cancelled at {at}: {result:?}");
        assert!(!destination.exists());
        assert_eq!(kernel.live_shape_count(), 0);
    }
    assert_eq!(entries(f.root.path()), names, "cancellation left something");
    assert_eq!(std::fs::read(&f.source).expect("source bytes"), before);

    // A success whose report cannot be written is published and says so by
    // its exit status; the file is kept, not rolled back.
    let lost = f.root.path().join("lost-report.fcad");
    f.ask_reversed(corner, 2.0);
    assert_eq!(
        f.fillet(&lost)
            .stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
    assert!(lost.exists(), "the published copy is kept");
    assert_eq!(
        inspect(&lost)["bodies"][0]["fillet_edge"]["available"],
        false
    );
    assert_eq!(std::fs::read(&f.source).expect("source bytes"), before);
}

/// A filleted copy is refused by every editor but its plate's height (§28C)
/// by name, a second fillet is refused, and the unsupported histories made by
/// the shipped creators and Cut are refused by discovery.
#[test]
fn native_a_filleted_copy_and_other_histories_are_refused_by_name() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate {
        cut_side: true,
        ..Plate::new(CCW)
    });
    let copy = f.root.path().join("rounded.fcad");
    f.ask_reversed([X0, Y0], 2.0);
    let published = reply(f.fillet(&copy).output().expect("process"), OP, 0);
    let fillet_id = published["result"]["feature_id"]
        .as_str()
        .expect("feature")
        .to_owned();
    let bytes = std::fs::read(&copy).expect("copy bytes");
    let catalog = inspect(&copy);
    let names_it = |reason: &Value| {
        let text = reason.as_str().expect("a reason");
        assert!(text.contains(&fillet_id), "{text}");
    };
    names_it(&catalog["bodies"][0]["fillet_edge"]["refusal"]);
    names_it(&catalog["bodies"][0]["cut_edit"]["refusal"]);
    assert_eq!(catalog["edit_extrude"]["available"], true, "§28C");
    assert_eq!(catalog["sketches"][0]["editable"], true, "§28D");
    assert_eq!(
        catalog["sketches"][0]["constraint_edit"]["available"], true,
        "§28E"
    );
    names_it(&catalog["sketches"][0]["circle_edit"]["refusal"]);
    names_it(&catalog["sketches"][0]["annulus_edit"]["refusal"]);
    let version = catalog["content_version"].as_str().expect("version");
    let body = catalog["bodies"][0]["body_id"].as_str().expect("body");
    let base = f.discovery()["target"]["base_feature_id"]
        .as_str()
        .expect("base")
        .to_owned();

    // The commands themselves, each with a request that would otherwise do.
    let out = f.root.path().join("again.fcad");
    f.ask_reversed([X0 + W, Y0], 2.0);
    let v = reply(
        f.fillet_from(&copy, body, version, &out)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert_eq!(refused(&v), "unsupported", "a second fillet: {v}");
    let cut = f.root.path().join("cut.json");
    write(
        &cut,
        &json!({"request_version":1,"center_mm":[10.0,8.0],"radius_mm":1.0,"depth_mm":2.0}),
    );
    let v = reply(
        cli()
            .arg("cut-circular-copy")
            .arg(&copy)
            .args(["--body", body, "--expect-version", version])
            .arg("--request")
            .arg(&cut)
            .arg("-o")
            .arg(&out)
            .arg("--json")
            .output()
            .expect("cut"),
        "cut-circular-copy",
        2,
    );
    assert_eq!(refused(&v), "unsupported", "a Cut after a Fillet: {v}");
    // The height is the plate's (§28C); the Fillet itself is no extrusion.
    assert_eq!(catalog["features"][0]["feature_id"], base.as_str());
    let raised = cli()
        .arg("edit-extrude")
        .arg(&copy)
        .args(["--feature", &fillet_id, "--distance-mm", "9"])
        .arg("-o")
        .arg(&out)
        .output()
        .expect("edit-extrude");
    assert!(!raised.status.success(), "{raised:?}");
    assert!(!out.exists());
    assert_eq!(std::fs::read(&copy).expect("copy bytes"), bytes);

    // Histories this slice does not round: a Cut, a Revolve, a circle and an
    // annulus, each made by the shipped commands.
    let cut_copy = f.root.path().join("with-cut.fcad");
    let v = reply(
        cli()
            .arg("cut-circular-copy")
            .arg(&f.source)
            .args(["--body", f.body_id(), "--expect-version", f.version()])
            .arg("--request")
            .arg(&cut)
            .arg("-o")
            .arg(&cut_copy)
            .arg("--json")
            .output()
            .expect("cut"),
        "cut-circular-copy",
        0,
    );
    assert_eq!(v["ok"], true);
    let root = f.root.path();
    let make = |command: &str, request: Value, name: &str| -> PathBuf {
        let input = root.join(format!("{name}.json"));
        write(&input, &request);
        let out = root.join(format!("{name}.fcad"));
        let r = cli()
            .arg(command)
            .arg(&input)
            .arg("-o")
            .arg(&out)
            .arg("--json")
            .output()
            .expect("create");
        assert!(r.status.success(), "{command}: {r:?}");
        out
    };
    let revolve = make(
        "create-sketch-revolve",
        json!({"request_version":1,"points_mm":[[2.0,0.0],[6.0,0.0],[6.0,5.0],[2.0,5.0]],"axis":"sketch_y","angle":"full_turn"}),
        "revolve",
    );
    let circle = make(
        "create-circle-extrude",
        json!({"schema_version":1,"center_mm":[0.0,0.0],"radius_mm":5.0,"height_mm":4.0}),
        "circle",
    );
    let annulus = make(
        "create-annular-extrude",
        json!({"schema_version":1,"center_mm":[0.0,0.0],"outer_radius_mm":5.0,
               "inner_radius_mm":2.0,"height_mm":4.0}),
        "annulus",
    );
    for (why, path) in [
        ("a Cut history", cut_copy),
        ("a Revolve", revolve),
        ("a circle", circle),
        ("an annulus", annulus),
    ] {
        let catalog = inspect(&path);
        let d = &catalog["bodies"][0]["fillet_edge"];
        assert_eq!(d["available"], false, "{why}: {d}");
        assert!(
            d["refusal"].as_str().is_some_and(|r| !r.is_empty()),
            "{why}"
        );
    }
}

/// The evaluator states the class again at every rebuild: a saved Fillet this
/// build would not have written is refused by name, never built as something
/// nearby.
#[test]
fn native_the_evaluator_refuses_a_saved_fillet_outside_the_class() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let copy = f.root.path().join("rounded.fcad");
    f.ask_reversed([X0, Y0], 2.0);
    reply(f.fillet(&copy).output().expect("process"), OP, 0);
    let d = Document::open_read_only(&copy).expect("copy");
    let objects = d.objects().expect("objects");
    d.close().expect("close");
    let saved = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
        .expect("the Fillet")
        .clone();
    let ObjectPayload::Fillet(fillet) = &saved.payload else {
        panic!("a Fillet");
    };
    let profile = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("profile");
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        panic!("a Sketch");
    };
    let [a, b] = fillet.edge.joint.segments();
    let opposite = sketch
        .curves
        .iter()
        .map(|c| c.id)
        .find(|id| {
            *id != a && *id != b && {
                // A Line that shares no end with `a`.
                let ends = |id: ferritecad_types::StableEntityId| match sketch
                    .curves
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.geometry.clone())
                {
                    Some(ferritecad_document::SketchGeometry::Line { start, end }) => {
                        [[start.x, start.y], [end.x, end.y]]
                    }
                    _ => panic!("a Line"),
                };
                let (x, y) = (ends(a), ends(*id));
                !x.iter().any(|p| y.contains(p))
            }
        })
        .expect("the opposite Line");
    let mut forged = Vec::new();
    let mut too_large = fillet.clone();
    too_large.radius_mm = D / 2. + 0.5;
    forged.push(("a radius past the policy", too_large, "too large"));
    let mut apart = fillet.clone();
    apart.edge.joint = ferritecad_types::ProfileJoint::new(a, opposite).expect("joint");
    forged.push(("two Lines that do not meet", apart, "not a corner"));
    let mut elsewhere = fillet.clone();
    elsewhere.edge.feature = profile.id;
    // The validator refuses this one before the evaluator is asked.
    forged.push(("an edge of a Sketch", elsewhere, "not a feature"));
    for (why, payload, expect) in forged {
        let path = f.root.path().join("forged.fcad");
        std::fs::copy(&copy, &path).expect("copy");
        let mut d = Document::open(&path).expect("writable");
        d.write(|w| {
            w.put_object(
                saved.id,
                saved.parent,
                saved.ordinal,
                saved.name.as_deref(),
                &ObjectPayload::Fillet(payload),
            )
            .map(|_| ())
        })
        .expect("forge");
        d.close().expect("close");
        let out = cli()
            .arg("rebuild")
            .arg(&path)
            .arg("--cold")
            .output()
            .expect("rebuild");
        assert!(!out.status.success(), "{why} was built");
        let text = String::from_utf8_lossy(&out.stderr).into_owned()
            + &String::from_utf8_lossy(&out.stdout);
        assert!(text.contains(expect), "{why}: {text}");
        std::fs::remove_file(&path).expect("clean");
    }
}

/// §28B: `edit-fillet-radius` on the saved Fillet, in a new copy.
mod radius {
    use super::*;

    const OP: &str = "edit-fillet-radius";

    /// A §28A Fillet written with the shipped preparation and writer and no
    /// kernel, so discovery and the protocol are testable in a stub build.
    pub(super) fn filleted_without_kernel(
        corners: [[f64; 2]; 4],
        corner: [f64; 2],
        r: f64,
    ) -> Fixture {
        let f = Fixture::drawn(Plate {
            cut_side: true,
            ..Plate::new(corners)
        });
        let mut d = Document::open(&f.source).expect("writable");
        let body: ferritecad_types::ObjectId = f.body_id().parse().expect("UUID");
        let edge = &f.at(corner)["edge"];
        let fillet = ferritecad_document::EdgeFillet {
            edge: ferritecad_document::FilletEdge {
                feature: edge["feature_id"]
                    .as_str()
                    .expect("feature")
                    .parse()
                    .expect("UUID"),
                joint: ferritecad_types::ProfileJoint::new(
                    edge["joint"][0].as_str().expect("a").parse().expect("UUID"),
                    edge["joint"][1].as_str().expect("b").parse().expect("UUID"),
                )
                .expect("joint"),
            },
            radius_mm: r,
        };
        let prepared =
            ferritecad_document::prepare_edge_fillet(&d, body, &fillet).expect("prepared");
        d.write_edge_fillet(&prepared).expect("written");
        d.close().expect("close");
        Fixture {
            catalog: inspect(&f.source),
            ..f
        }
    }

    /// A §28A Fillet published by the shipped command.
    pub(super) fn filleted_by_cli(
        corners: [[f64; 2]; 4],
        corner: [f64; 2],
        r: f64,
        name: &str,
    ) -> Fixture {
        let f = Fixture::drawn(Plate {
            cut_side: true,
            ..Plate::new(corners)
        });
        let rounded = f.root.path().join(format!("{name}.fcad"));
        f.ask_reversed(corner, r);
        reply(f.fillet(&rounded).output().expect("process"), super::OP, 0);
        let catalog = inspect(&rounded);
        Fixture {
            source: rounded,
            catalog,
            ..f
        }
    }

    impl Fixture {
        pub(super) fn fillet_row(&self) -> &Value {
            &self.catalog["fillets"][0]
        }
        pub(super) fn fillet_id(&self) -> &str {
            self.fillet_row()["feature_id"]
                .as_str()
                .expect("Fillet UUID")
        }
        pub(super) fn ask_radius(&self, r: f64) {
            write(&self.request, &json!({"request_version":1,"radius_mm":r}));
        }
        pub(super) fn edit(&self, output: &Path) -> Command {
            self.edit_from(&self.source, self.fillet_id(), self.version(), output)
        }
        fn edit_from(&self, source: &Path, feature: &str, version: &str, output: &Path) -> Command {
            let mut c = cli();
            c.arg(OP)
                .arg(source)
                .arg("--feature")
                .arg(feature)
                .arg("--expect-version")
                .arg(version)
                .arg("--request")
                .arg(&self.request)
                .arg("-o")
                .arg(output)
                .arg("--json");
            c
        }
    }

    /// Every SQL cell of the source survives into the copy, except exactly
    /// the Fillet row's payload and hash and the copy's own stamp. Returns
    /// how many cells moved.
    fn only_the_radius_changed(source: &Path, copy: &Path, fillet: &str) -> usize {
        let before = tables(source);
        let after = tables(copy);
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>()
        );
        let id: ferritecad_types::ObjectId = fillet.parse().expect("UUID");
        let selected = rusqlite::types::Value::Blob(id.to_bytes().to_vec());
        let mut moved = 0;
        for (table, (columns, rows)) in &before {
            let (theirs, mine) = &after[table];
            assert_eq!(columns, theirs, "{table} columns");
            assert_eq!(rows.len(), mine.len(), "{table} rows");
            for (a, b) in rows.iter().zip(mine) {
                for (i, column) in columns.iter().enumerate() {
                    if a[i] == b[i] {
                        continue;
                    }
                    moved += 1;
                    let allowed = match (table.as_str(), column.as_str()) {
                        ("objects", "payload" | "payload_hash") => a.contains(&selected),
                        ("meta", "modified_at") => true,
                        _ => false,
                    };
                    assert!(allowed, "{table}.{column} changed");
                }
            }
        }
        moved
    }

    pub(super) fn stored_refs(path: &Path) -> Vec<ferritecad_document::TopologyRef> {
        let d = Document::open_read_only(path).expect("open");
        let refs = d.topology_refs().expect("refs");
        d.close().expect("close");
        refs
    }

    /// Discovery on the saved Fillet, the strict request, and every structural
    /// refusal, with no kernel needed.
    #[test]
    fn radius_discovery_and_protocol_without_native() {
        // A plate with no Fillet offers none; an unrelated reader is unchanged.
        let plain = Fixture::drawn(Plate::new(CCW));
        assert_eq!(plain.catalog["fillets"], json!([]));

        let f = filleted_without_kernel(CCW, [X0 + W, Y0], 2.375);
        let row = f.fillet_row();
        let edit = &row["radius_edit"];
        assert_eq!(edit["available"], true, "{row}");
        assert!(edit["refusal"].is_null() && edit["document_refusal"].is_null());
        assert_eq!(edit["min_radius_mm"], 0.01);
        assert_eq!(edit["max_radius_mm"], D / 2.);
        assert_eq!(row["radius_mm"], 2.375);
        assert_eq!(row["corner_mm"], json!([X0 + W, Y0]));
        assert_eq!(row["body_id"], f.body_id());
        assert_eq!(row["previous_feature_id"], row["edge"]["feature_id"]);
        let joint = &row["edge"]["joint"];
        assert!(joint[0].as_str() < joint[1].as_str(), "canonical");
        let before = std::fs::read(&f.source).expect("source bytes");
        let never = f.root.path().join("never.fcad");
        f.ask_radius(3.0);
        let names = entries(f.root.path());

        let raw = |body: &str| {
            std::fs::write(&f.request, body).expect("request");
            reply(f.edit(&never).output().expect("process"), OP, 2)
        };
        for (why, body) in [
            (
                "an unknown key",
                r#"{"request_version":1,"radius_mm":3,"edge":{}}"#.to_owned(),
            ),
            (
                "a duplicate key",
                r#"{"request_version":1,"radius_mm":3,"radius_mm":4}"#.to_owned(),
            ),
            (
                "an escaped duplicate",
                r#"{"request_version":1,"radius_mm":3,"radius\u005fmm":4}"#.to_owned(),
            ),
            ("an array", r#"[1,3]"#.to_owned()),
            (
                "a string radius",
                r#"{"request_version":1,"radius_mm":"3"}"#.to_owned(),
            ),
            ("no radius", r#"{"request_version":1}"#.to_owned()),
            (
                "an infinite radius",
                r#"{"request_version":1,"radius_mm":1e999}"#.to_owned(),
            ),
            (
                "a retargeted edge",
                r#"{"request_version":1,"radius_mm":3,"joint":[]}"#.to_owned(),
            ),
            (
                "an oversized request",
                format!(
                    r#"{{"request_version":1,"radius_mm":3,"pad":"{}"}}"#,
                    "x".repeat(70_000)
                ),
            ),
        ] {
            let v = raw(&body);
            assert_eq!(refused(&v), "input", "{why}: {v}");
        }
        let v = raw(r#"{"request_version":2,"radius_mm":3}"#);
        assert_eq!(refused(&v), "unsupported", "{v}");

        // Structural refusals, each with every other argument correct. A build
        // with no kernel answers `unsupported` first; that order is recorded
        // in the contract.
        let base = row["previous_feature_id"]
            .as_str()
            .expect("base")
            .to_owned();
        let foreign = ferritecad_types::ObjectId::new().to_string();
        for (why, feature, radius) in [
            ("no radius", f.fillet_id(), 0.0),
            ("negative", f.fillet_id(), -1.0),
            ("below the floor", f.fillet_id(), 0.005),
            ("past half the shorter side", f.fillet_id(), D / 2. + 0.01),
            ("the kernel's own limit", f.fillet_id(), D),
            ("the base Extrude", base.as_str(), 3.0),
            ("a foreign UUID", foreign.as_str(), 3.0),
        ] {
            f.ask_radius(radius);
            let v = reply(
                f.edit_from(&f.source, feature, f.version(), &never)
                    .output()
                    .expect("process"),
                OP,
                2,
            );
            assert!(matches!(refused(&v), "input" | "unsupported"), "{why}: {v}");
        }
        f.ask_radius(3.0);
        let v = reply(
            f.edit_from(&f.source, f.fillet_id(), &"0".repeat(64), &never)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert!(matches!(refused(&v), "input" | "unsupported"), "stale: {v}");
        // A refusal whose report cannot be written is still a refusal.
        f.ask_radius(0.0);
        assert_eq!(
            f.edit(&never)
                .stdout(pipe::closed_pipe())
                .stderr(pipe::closed_pipe())
                .status()
                .expect("pipes")
                .code(),
            Some(7)
        );
        assert_eq!(entries(f.root.path()), names, "a refusal left something");
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);

        // A Fillet this build did not name the way §28A does is refused by
        // discovery with the shared domain reason.
        let odd = filleted_without_kernel(CCW, [X0, Y0], 2.0);
        let mut d = Document::open(&odd.source).expect("writable");
        let owner: ferritecad_types::ObjectId = odd.fillet_id().parse().expect("UUID");
        d.write(|w| {
            w.put_topology_ref(&ferritecad_document::TopologyRef {
                id: ferritecad_types::StableEntityId::new(),
                owner,
                producer_feature: owner,
                expected_kind: ferritecad_document::EntityKind::Face,
                output_role: SemanticRole::FilletFace {
                    source_edge: ferritecad_types::StableEntityId::new(),
                },
                selection: ferritecad_document::SelectionRule::Exact,
                fallback_signature: None,
            })
        })
        .expect("an extra name");
        d.close().expect("close");
        let row = &inspect(&odd.source)["fillets"][0];
        assert_eq!(row["radius_edit"]["available"], false, "{row}");
        assert!(
            row["radius_edit"]["refusal"]
                .as_str()
                .expect("reason")
                .contains("more faces")
        );
        assert!(row["corner_mm"].is_null() && row["radius_edit"]["max_radius_mm"].is_null());
        assert_eq!(row["radius_mm"], 2.0, "the stored radius is still reported");
    }

    /// One radius edit of `f`'s saved Fillet into `name`, checked against
    /// everything a copy must be. Returns the copy and what the kernel
    /// measured.
    fn edited(f: &Fixture, corner: [f64; 2], from: f64, r: f64, name: &str) -> (PathBuf, Measured) {
        let before = std::fs::read(&f.source).expect("source bytes");
        let refs = stored_refs(&f.source);
        let copy = f.root.path().join(format!("{name}.fcad"));
        f.ask_radius(r);
        let published = reply(f.edit(&copy).output().expect("process"), OP, 0);
        let result = &published["result"];
        assert_eq!(result["feature_id"], f.fillet_id(), "the same feature");
        assert_eq!(result["body_id"], f.body_id());
        assert_eq!(result["edge"], f.fillet_row()["edge"], "the same edge");
        assert_eq!(result["corner_mm"], json!(corner));
        assert_eq!(result["previous_radius_mm"], from);
        assert_eq!(result["radius_mm"], r);
        assert_eq!(
            std::fs::read(&f.source).expect("bytes"),
            before,
            "source touched"
        );
        assert!(only_the_radius_changed(&f.source, &copy, f.fillet_id()) >= 2);
        assert_eq!(stored_refs(&copy), refs, "every name and its UUID kept");
        let after = inspect(&copy);
        assert_eq!(after["fillets"][0]["radius_mm"], r);
        assert_eq!(after["fillets"][0]["edge"], f.fillet_row()["edge"]);

        let checked = cli()
            .arg("validate")
            .arg(&copy)
            .arg("--json")
            .output()
            .expect("validate");
        assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
        let rebuilt = cli()
            .arg("rebuild")
            .arg(&copy)
            .arg("--cold")
            .output()
            .expect("rebuild");
        let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
        let n = refs.len();
        assert!(
            text.contains(&format!("{n} of {n} stored references resolved")),
            "{text}"
        );
        assert!(text.contains(&format!("r{r} mm")), "{text}");

        let cold = measure(&copy, None);
        let cache = copy.with_extension("fcad-cache");
        cold.same_as(&measure(
            &copy,
            Some((&cache, ferritecad_eval::CacheOutcome::Miss)),
        ));
        cold.same_as(&measure(
            &copy,
            Some((&cache, ferritecad_eval::CacheOutcome::Hit)),
        ));
        assert_eq!(cold.faces, 7);
        let exact = W * D * H - (1. - PI / 4.) * r * r * H;
        assert!(
            (cold.volume - exact).abs() < 1e-9 * exact,
            "{} is not {exact}",
            cold.volume
        );
        assert_eq!(cold.fillet, FaceSurface::Cylinder { radius: r });
        let centre = inward(corner, r);
        assert!(
            (cold.axis_origin[0] - centre[0]).abs() < 1e-9
                && (cold.axis_origin[1] - centre[1]).abs() < 1e-9,
            "the axis {:?} is not r inward of {corner:?}",
            cold.axis_origin
        );
        assert_eq!(
            cold.roles["origin side of feature.fillet"],
            [FaceSurface::Plane; 4]
        );
        assert_eq!(
            cold.roles["origin cap Start of feature.fillet"],
            [FaceSurface::Plane]
        );
        assert_eq!(
            cold.roles["origin cap End of feature.fillet"],
            [FaceSurface::Plane]
        );

        let m = mesh(&copy, &copy.with_extension("stl"));
        check_mesh(&m, corner, r, H);
        fbx(&copy, name);
        (copy, cold)
    }

    /// Increase and then decrease a fractional radius, on two different corners
    /// across fixtures of both windings and another starting segment. The
    /// rounded face stays the same named face at the same corner; its axis
    /// moves by exactly the change of radius along both sides.
    #[test]
    fn native_radius_edits_move_the_named_cylinder_and_keep_every_identity() {
        if !native() {
            return;
        }
        for (label, corners, corner) in [
            ("ccw", CCW, [X0 + W, Y0]),
            ("cw", CW_FROM_UPPER_RIGHT, [X0, Y0 + D]),
            ("third", CCW_FROM_THIRD, [X0 + W, Y0]),
        ] {
            let f = filleted_by_cli(corners, corner, 2.375, &format!("rounded-{label}"));
            let original = measure(&f.source, None);
            let (up, grown) = edited(&f, corner, 2.375, 4.8125, &format!("radius-{label}-up"));
            // The same corner, further in: the axis moved by the change of
            // radius along both sides, and nowhere else.
            let step = inward(corner, 4.8125);
            let was = inward(corner, 2.375);
            assert!(
                (grown.axis_origin[0] - original.axis_origin[0] - (step[0] - was[0])).abs() < 1e-9
                    && (grown.axis_origin[1] - original.axis_origin[1] - (step[1] - was[1])).abs()
                        < 1e-9
            );
            // A second edit of the edited copy: repeated edits keep identity.
            let g = Fixture {
                catalog: inspect(&up),
                source: up,
                root: tempfile::tempdir().expect("dir"),
                request: f.request.clone(),
            };
            let g = Fixture { root: f.root, ..g };
            let (down, _) = edited(&g, corner, 4.8125, 1.1875, &format!("radius-{label}-down"));
            assert_eq!(stored_refs(&down), stored_refs(&g.source));
        }
    }

    /// Asking for the saved radius publishes the same model: the payload and
    /// its hash are unchanged, and only the stamp may differ.
    #[test]
    fn native_the_same_radius_publishes_the_same_model() {
        if !native() {
            return;
        }
        let f = filleted_by_cli(CCW, [X0, Y0], 3.0625, "rounded");
        let copy = f.root.path().join("same.fcad");
        f.ask_radius(3.0625);
        let v = reply(f.edit(&copy).output().expect("process"), OP, 0);
        assert_eq!(v["result"]["previous_radius_mm"], v["result"]["radius_mm"]);
        assert!(only_the_radius_changed(&f.source, &copy, f.fillet_id()) <= 1);
        measure(&f.source, None).same_as(&measure(&copy, None));
    }

    /// The cache under one document path: after the radius changes there, the
    /// Fillet misses and the unchanged plate is reused; then both hit, and
    /// both agree with cold.
    #[test]
    fn native_a_changed_radius_misses_in_place_and_the_plate_is_reused() {
        if !native() {
            return;
        }
        let f = filleted_by_cli(CCW, [X0 + W, Y0 + D], 2.375, "rounded");
        let place = f.root.path().join("in-place.fcad");
        std::fs::copy(&f.source, &place).expect("copy");
        let cache = place.with_extension("fcad-cache");
        let (first, events) = measure_events(&place, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Miss)
        );
        let (again, events) = measure_events(&place, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
        );
        first.same_as(&again);

        // Edit the radius into a new copy, then put it at the same path.
        let edited = f.root.path().join("edited.fcad");
        f.ask_radius(5.25);
        reply(f.edit(&edited).output().expect("process"), OP, 0);
        std::fs::copy(&edited, &place).expect("replace in place");
        let fillet: ferritecad_types::ObjectId = f.fillet_id().parse().expect("UUID");
        let (changed, events) = measure_events(&place, Some(&cache));
        for event in &events {
            let expected = if event.feature == fillet {
                ferritecad_eval::CacheOutcome::Miss
            } else {
                ferritecad_eval::CacheOutcome::Hit
            };
            assert_eq!(event.outcome, expected, "{events:?}");
        }
        assert!(events.iter().any(|e| e.feature == fillet));
        assert!(
            events.iter().any(|e| e.feature != fillet),
            "the plate was asked"
        );
        assert_eq!(changed.fillet, FaceSurface::Cylinder { radius: 5.25 });
        assert!((changed.volume - first.volume).abs() > 1.0);
        let (hit, events) = measure_events(&place, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
        );
        changed.same_as(&hit);
        changed.same_as(&measure(&place, None));
    }

    /// Destinations, a stale source, cancellation, an output race at the last
    /// barrier and a lost report: each atomic.
    #[test]
    fn native_radius_refusals_races_cancellation_and_report_loss_are_atomic() {
        if !native() {
            return;
        }
        let f = filleted_by_cli(CCW, [X0, Y0], 2.0, "rounded");
        let before = std::fs::read(&f.source).expect("bytes");
        let taken = f.root.path().join("taken.fcad");
        std::fs::write(&taken, b"another process owns this").expect("occupied");
        let alias = f.root.path().join("alias.fcad");
        std::fs::hard_link(&f.source, &alias).expect("hard link");
        f.ask_radius(3.0);
        for (destination, why) in [
            (f.source.clone(), "the source"),
            (taken.clone(), "an occupied output"),
            (alias.clone(), "an alias of the source"),
        ] {
            let v = reply(f.edit(&destination).output().expect("process"), OP, 2);
            assert_eq!(refused(&v), "input", "{why}: {v}");
        }
        assert_eq!(
            std::fs::read(&taken).expect("taken"),
            b"another process owns this"
        );
        let never = f.root.path().join("never.fcad");
        for radius in [0.0, 0.009, D / 2. + 1e-9, D, 100.0] {
            f.ask_radius(radius);
            let v = reply(f.edit(&never).output().expect("process"), OP, 2);
            assert_eq!(refused(&v), "input", "{radius}: {v}");
        }
        assert!(!never.exists());

        let d = Document::open_read_only(&f.source).expect("source");
        let expected = ferritecad_document::DocumentVersion {
            document_id: d.meta().document_id,
            content: d.content_version().expect("version"),
        };
        d.close().expect("close");
        let feature = f.fillet_id().parse().expect("UUID");
        let request = |destination: &Path| ferritecad_jobs::EditFilletRadiusRequest {
            source: f.source.clone(),
            expected,
            feature,
            radius_mm: 3.0,
            destination: destination.to_path_buf(),
        };
        // Cancellation before the job and at its last barrier.
        for at in [0.0, 0.95] {
            let destination = f.root.path().join(format!("cancelled-{at}.fcad"));
            let token = ferritecad_kernel::CancelToken::new();
            let stop = token.clone();
            if at == 0.0 {
                token.cancel();
            }
            let context = OperationContext::default()
                .with_cancel(token)
                .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                    if fraction >= at {
                        stop.cancel();
                    }
                }));
            let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
            let result = ferritecad_jobs::edit_fillet_radius_copy(
                &request(&destination),
                &mut kernel,
                &context,
            );
            assert!(result.is_err(), "cancelled at {at}: {result:?}");
            assert!(!destination.exists());
            assert_eq!(kernel.live_shape_count(), 0);
        }
        // Another process takes the output at the last barrier.
        let raced = f.root.path().join("raced.fcad");
        let racer = raced.clone();
        let context = OperationContext::default().with_progress(
            ferritecad_kernel::ProgressSink::new(move |fraction| {
                if fraction == 0.95 {
                    std::fs::write(&racer, b"racing file").expect("racer");
                }
            }),
        );
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        assert!(
            ferritecad_jobs::edit_fillet_radius_copy(&request(&raced), &mut kernel, &context)
                .is_err()
        );
        assert_eq!(std::fs::read(&raced).expect("racer kept"), b"racing file");
        // The source changes after it was inspected.
        let stale_copy = f.root.path().join("stale-source.fcad");
        std::fs::copy(&f.source, &stale_copy).expect("copy");
        let mut d = Document::open(&stale_copy).expect("writable");
        let first = d.objects().expect("objects").remove(0);
        d.write(|w| {
            w.put_object(
                first.id,
                first.parent,
                first.ordinal,
                Some("changed"),
                &first.payload,
            )
            .map(|_| ())
        })
        .expect("change");
        d.close().expect("close");
        f.ask_radius(3.0);
        let stale_out = f.root.path().join("stale.fcad");
        let v = reply(
            f.edit_from(&stale_copy, f.fillet_id(), f.version(), &stale_out)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert_eq!(refused(&v), "input", "{v}");
        assert!(!stale_out.exists());
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);

        // A publication whose report is lost stays published: exit 7.
        let lost = f.root.path().join("lost.fcad");
        assert_eq!(
            f.edit(&lost)
                .stdout(pipe::closed_pipe())
                .stderr(pipe::closed_pipe())
                .status()
                .expect("pipes")
                .code(),
            Some(7)
        );
        assert_eq!(inspect(&lost)["fillets"][0]["radius_mm"], 3.0);
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    }
}

/// §28C: the height of the plate under the saved Fillet, through the existing
/// `edit-extrude`. The Fillet keeps its row, its edge and radius, and every
/// name; only the base Extrude's distance changes.
mod height {
    use super::radius::{filleted_by_cli, filleted_without_kernel, stored_refs};
    use super::*;

    const OP: &str = "edit-extrude";

    impl Fixture {
        pub(super) fn base_row(&self) -> &Value {
            &self.catalog["features"][0]
        }
        pub(super) fn base_id(&self) -> &str {
            self.base_row()["feature_id"]
                .as_str()
                .expect("Extrude UUID")
        }
        pub(super) fn raise(
            &self,
            source: &Path,
            feature: &str,
            height: &str,
            output: &Path,
        ) -> Command {
            let mut c = cli();
            c.arg(OP)
                .arg(source)
                .arg("--feature")
                .arg(feature)
                .arg("--distance-mm")
                .arg(height)
                .arg("--expect-version")
                .arg(self.version())
                .arg("-o")
                .arg(output)
                .arg("--json");
            c
        }
    }

    /// Every SQL cell of the source survives into the copy, except exactly
    /// the base Extrude row's payload and hash and the copy's own stamp.
    /// Returns how many cells moved.
    fn only_the_height_changed(source: &Path, copy: &Path, base: &str) -> usize {
        let before = tables(source);
        let after = tables(copy);
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>()
        );
        let id: ferritecad_types::ObjectId = base.parse().expect("UUID");
        let selected = rusqlite::types::Value::Blob(id.to_bytes().to_vec());
        let mut moved = 0;
        for (table, (columns, rows)) in &before {
            let (theirs, mine) = &after[table];
            assert_eq!(columns, theirs, "{table} columns");
            assert_eq!(rows.len(), mine.len(), "{table} rows");
            for (a, b) in rows.iter().zip(mine) {
                for (i, column) in columns.iter().enumerate() {
                    if a[i] == b[i] {
                        continue;
                    }
                    moved += 1;
                    let allowed = match (table.as_str(), column.as_str()) {
                        ("objects", "payload" | "payload_hash") => a.contains(&selected),
                        ("meta", "modified_at") => true,
                        _ => false,
                    };
                    assert!(allowed, "{table}.{column} changed");
                }
            }
        }
        moved
    }

    /// The exact B-Rep volume of the plate with one corner rounded by r.
    fn exact(r: f64, h: f64) -> f64 {
        (W * D - (1. - PI / 4.) * r * r) * h
    }

    /// Discovery on the rounded plate, and the unchanged protocol with the
    /// stub build's real order of checks. No kernel is needed to run it.
    #[test]
    fn height_discovery_and_protocol_without_native() {
        // Both builds run this whole test, so which answers to expect is
        // decided here without `native()`, whose "skipped:" line would read
        // as a skipped gate.
        let kernel = ferritecad_occt::is_available();
        if !kernel {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        }
        let plain = Fixture::drawn(Plate::new(CCW));
        assert_eq!(plain.base_row()["fillet_base"], Value::Null);
        let f = filleted_without_kernel(CCW, [X0 + W, Y0 + D], 2.375);
        assert_eq!(f.catalog["edit_extrude"]["available"], true);
        let row = f.base_row();
        assert_eq!(row["editable"], true, "{row}");
        assert_eq!(row["refusal"], Value::Null);
        assert_eq!(row["distance_mm"], H);
        for key in [
            "base_height_edit",
            "base_height_edit_v2",
            "base_height_edit_v3",
        ] {
            assert_eq!(
                row[key],
                Value::Null,
                "{key}: a Fillet is not a Cut history"
            );
        }
        let fillet = f.fillet_row();
        assert_eq!(
            row["fillet_base"],
            json!({
                "fillet_feature_id": fillet["feature_id"],
                "body_id": f.body_id(),
                "edge": fillet["edge"],
                "corner_mm": [X0 + W, Y0 + D],
                "radius_mm": 2.375,
                // §28E, additive.
                "profile_constrained": false,
            })
        );
        // Every other editor still refuses the filleted plate by name.
        let names_it = |reason: &Value| {
            let text = reason.as_str().expect("a reason");
            assert!(text.contains(f.fillet_id()), "{text}");
        };
        names_it(&f.catalog["bodies"][0]["fillet_edge"]["refusal"]);
        names_it(&f.catalog["bodies"][0]["cut_edit"]["refusal"]);
        assert_eq!(f.catalog["sketches"][0]["editable"], true, "§28D");
        assert_eq!(
            f.catalog["sketches"][0]["constraint_edit"]["available"], true,
            "§28E"
        );

        // The protocol. A build without a kernel reads the source and then
        // asks for the kernel before any other check, so every well-formed
        // request is `unsupported` there; with a kernel each is its own kind.
        let before = std::fs::read(&f.source).expect("source bytes");
        let names = entries(f.root.path());
        let never = f.root.path().join("never.fcad");
        let foreign = ferritecad_types::ObjectId::new().to_string();
        let sketch = f.catalog["sketches"][0]["sketch_id"]
            .as_str()
            .expect("sketch")
            .to_owned();
        for (why, feature, height, kind) in [
            ("zero", f.base_id(), "0", "input"),
            ("negative", f.base_id(), "-1", "input"),
            ("not a number", f.base_id(), "NaN", "input"),
            ("infinite", f.base_id(), "inf", "input"),
            ("the Fillet", f.fillet_id(), "9", "unsupported"),
            ("the Sketch", sketch.as_str(), "9", "unsupported"),
            ("a foreign UUID", foreign.as_str(), "9", "input"),
        ] {
            let v = reply(
                f.raise(&f.source, feature, height, &never)
                    .output()
                    .expect("process"),
                OP,
                2,
            );
            if kernel {
                assert_eq!(refused(&v), kind, "{why}: {v}");
            } else {
                assert_eq!(refused(&v), "unsupported", "{why}: {v}");
                assert!(v.to_string().contains("Open CASCADE"), "{why}: {v}");
            }
        }
        // A refusal whose report cannot be written is still a refusal.
        assert_eq!(
            f.raise(&f.source, f.base_id(), "0", &never)
                .stdout(pipe::closed_pipe())
                .stderr(pipe::closed_pipe())
                .status()
                .expect("pipes")
                .code(),
            Some(7)
        );
        assert_eq!(entries(f.root.path()), names, "a refusal left something");
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);

        // A Fillet outside the frame keeps the height refused, naming it and
        // the reason the radius edit gives.
        let odd = filleted_without_kernel(CCW, [X0, Y0], 2.0);
        let mut d = Document::open(&odd.source).expect("writable");
        let owner: ferritecad_types::ObjectId = odd.fillet_id().parse().expect("UUID");
        d.write(|w| {
            w.put_topology_ref(&ferritecad_document::TopologyRef {
                id: ferritecad_types::StableEntityId::new(),
                owner,
                producer_feature: owner,
                expected_kind: ferritecad_document::EntityKind::Face,
                output_role: SemanticRole::FilletFace {
                    source_edge: ferritecad_types::StableEntityId::new(),
                },
                selection: ferritecad_document::SelectionRule::Exact,
                fallback_signature: None,
            })
        })
        .expect("an extra name");
        d.close().expect("close");
        let catalog = inspect(&odd.source);
        assert_eq!(catalog["edit_extrude"]["available"], false);
        let reason = catalog["edit_extrude"]["refusal"].as_str().expect("reason");
        assert!(
            reason.contains(odd.fillet_id()) && reason.contains("more faces"),
            "{reason}"
        );
        assert_eq!(catalog["features"][0]["editable"], false);
        assert_eq!(catalog["features"][0]["fillet_base"], Value::Null);
        if kernel {
            let odd = Fixture { catalog, ..odd };
            let v = reply(
                odd.raise(&odd.source, odd.base_id(), "9", &never)
                    .output()
                    .expect("process"),
                OP,
                2,
            );
            assert_eq!(refused(&v), "unsupported", "{v}");
            assert!(v.to_string().contains("more faces"), "{v}");
            assert!(!never.exists());
        }
    }

    /// One height edit of `f`'s plate into `name`, checked against
    /// everything a copy must be.
    fn raised(f: &Fixture, corner: [f64; 2], r: f64, h: f64, name: &str) -> (PathBuf, Measured) {
        let before = std::fs::read(&f.source).expect("source bytes");
        let refs = stored_refs(&f.source);
        let fillet_before = f.fillet_row().clone();
        let copy = f.root.path().join(format!("{name}.fcad"));
        let published = reply(
            f.raise(&f.source, f.base_id(), &h.to_string(), &copy)
                .output()
                .expect("process"),
            OP,
            0,
        );
        let result = &published["result"];
        assert_eq!(result["feature_id"], f.base_id(), "the base Extrude");
        assert_eq!(result["document_id"], f.catalog["document_id"]);
        assert_eq!(
            std::fs::read(&f.source).expect("bytes"),
            before,
            "source touched"
        );
        assert!(only_the_height_changed(&f.source, &copy, f.base_id()) >= 2);
        assert_eq!(stored_refs(&copy), refs, "every name and its UUID kept");
        let after = inspect(&copy);
        assert_eq!(after["features"][0]["distance_mm"], h);
        assert_eq!(
            after["features"][0]["fillet_base"],
            f.base_row()["fillet_base"]
        );
        assert_eq!(
            after["fillets"][0]["feature_id"],
            fillet_before["feature_id"]
        );
        assert_eq!(after["fillets"][0]["edge"], fillet_before["edge"]);
        assert_eq!(after["fillets"][0]["radius_mm"], r);
        assert_eq!(after["fillets"][0]["radius_edit"]["available"], true);

        let checked = cli()
            .arg("validate")
            .arg(&copy)
            .arg("--json")
            .output()
            .expect("validate");
        assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
        let rebuilt = cli()
            .arg("rebuild")
            .arg(&copy)
            .arg("--cold")
            .output()
            .expect("rebuild");
        let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
        let n = refs.len();
        assert!(
            text.contains(&format!("{n} of {n} stored references resolved")),
            "{text}"
        );
        assert!(text.contains(&format!("r{r} mm")), "{text}");

        let cold = measure(&copy, None);
        let cache = copy.with_extension("fcad-cache");
        cold.same_as(&measure(
            &copy,
            Some((&cache, ferritecad_eval::CacheOutcome::Miss)),
        ));
        cold.same_as(&measure(
            &copy,
            Some((&cache, ferritecad_eval::CacheOutcome::Hit)),
        ));
        assert_eq!(cold.faces, 7);
        // The exact B-Rep volume is the plate's at the new height: the
        // height is what it measures, since W, D and r are fixed.
        let exact = exact(r, h);
        assert!(
            (cold.volume - exact).abs() < 1e-9 * exact,
            "{} is not {exact}",
            cold.volume
        );
        // The same cylinder under the same named face, on the same vertical
        // axis at the same place: r inward of the same corner.
        assert_eq!(cold.fillet, FaceSurface::Cylinder { radius: r });
        let centre = inward(corner, r);
        assert!(
            (cold.axis_origin[0] - centre[0]).abs() < 1e-9
                && (cold.axis_origin[1] - centre[1]).abs() < 1e-9
                && cold.axis_direction[0].abs() < 1e-12
                && cold.axis_direction[1].abs() < 1e-12,
            "the axis {:?} {:?} is not r inward of {corner:?}",
            cold.axis_origin,
            cold.axis_direction
        );
        assert_eq!(
            cold.roles["origin side of feature.fillet"],
            [FaceSurface::Plane; 4]
        );
        for side in ["Start", "End"] {
            assert_eq!(
                cold.roles[&format!("origin cap {side} of feature.fillet")],
                [FaceSurface::Plane]
            );
        }
        // An independent reading of the exported mesh, to its tessellation:
        // the part spans 0..h, the chosen corner alone is rounded, and its
        // volume is the exact one less at most the chords' sagitta.
        let m = mesh(&copy, &copy.with_extension("stl"));
        check_mesh(&m, corner, r, h);
        fbx(&copy, name);
        (copy, cold)
    }

    /// Raise and then lower a fractional height, below the radius too, on two
    /// different corners across fixtures of both windings and another
    /// starting segment. The rounded face stays the same named face, of the
    /// same radius, on the same axis; only the plate's height moves.
    #[test]
    fn native_height_edits_keep_the_named_cylinder_and_every_identity() {
        if !native() {
            return;
        }
        for (label, corners, corner) in [
            ("ccw", CCW, [X0 + W, Y0]),
            ("cw", CW_FROM_UPPER_RIGHT, [X0, Y0 + D]),
            ("third", CCW_FROM_THIRD, [X0 + W, Y0]),
        ] {
            let r = 2.375;
            let f = filleted_by_cli(corners, corner, r, &format!("rounded-{label}"));
            let original = measure(&f.source, None);
            let (up, raised_up) = raised(&f, corner, r, 11.4375, &format!("height-{label}-up"));
            for k in 0..3 {
                assert!((raised_up.axis_origin[k] - original.axis_origin[k]).abs() < 1e-9);
            }
            assert_eq!(raised_up.roles, original.roles, "the same faces by name");
            // A second edit of the edited copy, to below the radius.
            let g = Fixture {
                catalog: inspect(&up),
                source: up,
                root: f.root,
                request: f.request.clone(),
            };
            let (down, _) = raised(&g, corner, r, 1.1875, &format!("height-{label}-down"));
            assert_eq!(stored_refs(&down), stored_refs(&g.source));
        }
    }

    /// Asking for the saved height publishes the same model; the text mode
    /// of `edit-extrude` is the one it always was.
    #[test]
    fn native_the_same_height_publishes_the_same_model() {
        if !native() {
            return;
        }
        let f = filleted_by_cli(CCW, [X0, Y0], 3.0625, "rounded");
        let copy = f.root.path().join("same.fcad");
        let out = f
            .raise(&f.source, f.base_id(), &H.to_string(), &copy)
            .output()
            .expect("process");
        let v = reply(out, OP, 0);
        assert_eq!(v["result"]["feature_id"], f.base_id());
        assert!(only_the_height_changed(&f.source, &copy, f.base_id()) <= 1);
        measure(&f.source, None).same_as(&measure(&copy, None));
        let plain = f.root.path().join("text.fcad");
        let out = cli()
            .arg(OP)
            .arg(&f.source)
            .args(["--feature", f.base_id(), "--distance-mm", "8"])
            .arg("-o")
            .arg(&plain)
            .output()
            .expect("text mode");
        assert!(out.status.success(), "{out:?}");
        let said = String::from_utf8(out.stdout).expect("UTF-8");
        assert!(
            said.starts_with("saved ") && said.contains(&format!("feature {}", f.base_id())),
            "{said}"
        );
    }

    /// The cache under one document path: after the height changes there, the
    /// plate and the Fillet over it both miss and neither returns the old
    /// solid; then both hit and agree with cold. The radius edit works on the
    /// height-edited copy and the height edit on the radius-edited one, with
    /// the same UUIDs.
    #[test]
    fn native_a_changed_height_misses_in_place_and_interleaves_with_the_radius() {
        if !native() {
            return;
        }
        let r = 2.375;
        let f = filleted_by_cli(CCW, [X0 + W, Y0 + D], r, "rounded");
        let place = f.root.path().join("in-place.fcad");
        std::fs::copy(&f.source, &place).expect("copy");
        let cache = place.with_extension("fcad-cache");
        let (first, _) = measure_events(&place, Some(&cache));
        let (again, events) = measure_events(&place, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
        );
        first.same_as(&again);

        let edited = f.root.path().join("edited.fcad");
        reply(
            f.raise(&f.source, f.base_id(), "9.25", &edited)
                .output()
                .expect("process"),
            OP,
            0,
        );
        std::fs::copy(&edited, &place).expect("replace in place");
        let base: ferritecad_types::ObjectId = f.base_id().parse().expect("UUID");
        let fillet: ferritecad_types::ObjectId = f.fillet_id().parse().expect("UUID");
        let (changed, events) = measure_events(&place, Some(&cache));
        for event in &events {
            let expected = if event.feature == fillet || event.feature == base {
                ferritecad_eval::CacheOutcome::Miss
            } else {
                ferritecad_eval::CacheOutcome::Hit
            };
            assert_eq!(event.outcome, expected, "{events:?}");
        }
        assert!(events.iter().any(|e| e.feature == fillet));
        assert!(events.iter().any(|e| e.feature == base));
        assert!((changed.volume - exact(r, 9.25)).abs() < 1e-9 * changed.volume);
        assert!(
            (changed.volume - first.volume).abs() > 1.0,
            "not the old solid"
        );
        assert_eq!(changed.fillet, FaceSurface::Cylinder { radius: r });
        let (hit, events) = measure_events(&place, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
        );
        changed.same_as(&hit);
        changed.same_as(&measure(&place, None));

        // The radius of the height-edited copy, then the height again.
        let g = Fixture {
            catalog: inspect(&edited),
            source: edited,
            root: f.root,
            request: f.request.clone(),
        };
        assert_eq!(g.fillet_id(), fillet.to_string());
        let rounder = g.root.path().join("rounder.fcad");
        g.ask_radius(4.8125);
        let v = reply(
            g.edit(&rounder).output().expect("process"),
            "edit-fillet-radius",
            0,
        );
        assert_eq!(v["result"]["feature_id"], fillet.to_string());
        let h = Fixture {
            catalog: inspect(&rounder),
            source: rounder,
            root: g.root,
            request: g.request.clone(),
        };
        assert_eq!(h.base_row()["fillet_base"]["radius_mm"], 4.8125);
        let lower = h.root.path().join("lower.fcad");
        reply(
            h.raise(&h.source, h.base_id(), "3.5", &lower)
                .output()
                .expect("process"),
            OP,
            0,
        );
        assert_eq!(
            stored_refs(&lower),
            stored_refs(&f.source),
            "the same names"
        );
        let m = measure(&lower, None);
        assert!((m.volume - exact(4.8125, 3.5)).abs() < 1e-9 * m.volume);
        assert_eq!(m.fillet, FaceSurface::Cylinder { radius: 4.8125 });
    }

    /// Destinations, a height OCCT cannot round, a stale source,
    /// cancellation, an output race at the last barrier and a lost report:
    /// each atomic.
    #[test]
    fn native_height_refusals_races_cancellation_and_report_loss_are_atomic() {
        if !native() {
            return;
        }
        let f = filleted_by_cli(CCW, [X0, Y0], 2.0, "rounded");
        let before = std::fs::read(&f.source).expect("bytes");
        let taken = f.root.path().join("taken.fcad");
        std::fs::write(&taken, b"another process owns this").expect("occupied");
        let alias = f.root.path().join("alias.fcad");
        std::fs::hard_link(&f.source, &alias).expect("hard link");
        for (destination, why) in [
            (f.source.clone(), "the source"),
            (taken.clone(), "an occupied output"),
            (alias.clone(), "an alias of the source"),
        ] {
            let v = reply(
                f.raise(&f.source, f.base_id(), "9", &destination)
                    .output()
                    .expect("process"),
                OP,
                2,
            );
            assert_eq!(refused(&v), "input", "{why}: {v}");
        }
        assert_eq!(
            std::fs::read(&taken).expect("taken"),
            b"another process owns this"
        );
        // Measured: OCCT does not round the edge of a plate 1e-5 mm or less
        // tall. The kernel says so during the strict rebuild; nothing is
        // clamped and nothing is published.
        let never = f.root.path().join("never.fcad");
        let v = reply(
            f.raise(&f.source, f.base_id(), "0.000001", &never)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert_eq!(refused(&v), "kernel", "{v}");
        assert!(!never.exists());

        let d = Document::open_read_only(&f.source).expect("source");
        let expected = ferritecad_document::DocumentVersion {
            document_id: d.meta().document_id,
            content: d.content_version().expect("version"),
        };
        d.close().expect("close");
        let feature = f.base_id().parse().expect("UUID");
        let request = |destination: &Path| ferritecad_jobs::EditExtrudeRequest {
            source: f.source.clone(),
            expected,
            feature,
            distance_mm: 9.0,
            destination: destination.to_path_buf(),
        };
        for at in [0.0, 0.95] {
            let destination = f.root.path().join(format!("cancelled-{at}.fcad"));
            let token = ferritecad_kernel::CancelToken::new();
            let stop = token.clone();
            if at == 0.0 {
                token.cancel();
            }
            let context = OperationContext::default()
                .with_cancel(token)
                .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                    if fraction >= at {
                        stop.cancel();
                    }
                }));
            let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
            let result =
                ferritecad_jobs::edit_extrude_copy(&request(&destination), &mut kernel, &context);
            assert!(result.is_err(), "cancelled at {at}: {result:?}");
            assert!(!destination.exists());
            assert_eq!(kernel.live_shape_count(), 0);
        }
        let raced = f.root.path().join("raced.fcad");
        let racer = raced.clone();
        let context = OperationContext::default().with_progress(
            ferritecad_kernel::ProgressSink::new(move |fraction| {
                if fraction == 0.95 {
                    std::fs::write(&racer, b"racing file").expect("racer");
                }
            }),
        );
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        assert!(
            ferritecad_jobs::edit_extrude_copy(&request(&raced), &mut kernel, &context).is_err()
        );
        assert_eq!(std::fs::read(&raced).expect("racer kept"), b"racing file");
        // The source changes after it was inspected.
        let stale_copy = f.root.path().join("stale-source.fcad");
        std::fs::copy(&f.source, &stale_copy).expect("copy");
        let mut d = Document::open(&stale_copy).expect("writable");
        let first = d.objects().expect("objects").remove(0);
        d.write(|w| {
            w.put_object(
                first.id,
                first.parent,
                first.ordinal,
                Some("changed"),
                &first.payload,
            )
            .map(|_| ())
        })
        .expect("change");
        d.close().expect("close");
        let stale_out = f.root.path().join("stale.fcad");
        let v = reply(
            f.raise(&stale_copy, f.base_id(), "9", &stale_out)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert_eq!(refused(&v), "input", "{v}");
        assert!(!stale_out.exists());
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);

        // A saved name of the plate that resolves to nothing. The legacy
        // standalone extrusion edit may keep such a name unresolved; a
        // rounded plate may not, so the copy is refused and nothing is
        // published.
        let dangling = f.root.path().join("dangling-source.fcad");
        std::fs::copy(&f.source, &dangling).expect("copy");
        let mut d = Document::open(&dangling).expect("writable");
        let base: ferritecad_types::ObjectId = f.base_id().parse().expect("UUID");
        d.write(|w| {
            w.put_topology_ref(&ferritecad_document::TopologyRef {
                id: ferritecad_types::StableEntityId::new(),
                owner: base,
                producer_feature: base,
                expected_kind: ferritecad_document::EntityKind::Face,
                output_role: SemanticRole::ExtrudeSide {
                    profile_segment: ferritecad_types::StableEntityId::new(),
                },
                selection: ferritecad_document::SelectionRule::Exact,
                fallback_signature: None,
            })
        })
        .expect("a name of nothing");
        d.close().expect("close");
        let g = Fixture {
            catalog: inspect(&dangling),
            source: dangling.clone(),
            root: tempfile::tempdir().expect("dir"),
            request: f.request.clone(),
        };
        let dangling_before = std::fs::read(&dangling).expect("bytes");
        let out = f.root.path().join("dangling.fcad");
        let v = reply(
            g.raise(&dangling, g.base_id(), "9", &out)
                .output()
                .expect("process"),
            OP,
            2,
        );
        // The frame accepts it; the copy's reference check is what refuses.
        assert_eq!(g.catalog["edit_extrude"]["available"], true);
        assert_eq!(refused(&v), "topology", "{v}");
        assert!(v.to_string().contains("unresolved"), "{v}");
        assert!(!out.exists());
        assert_eq!(std::fs::read(&dangling).expect("bytes"), dangling_before);

        // A publication whose report is lost stays published: exit 7.
        let lost = f.root.path().join("lost.fcad");
        assert_eq!(
            f.raise(&f.source, f.base_id(), "9", &lost)
                .stdout(pipe::closed_pipe())
                .stderr(pipe::closed_pipe())
                .status()
                .expect("pipes")
                .code(),
            Some(7)
        );
        assert_eq!(inspect(&lost)["features"][0]["distance_mm"], 9.0);
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    }
}

/// §28D: the base rectangle under the saved Fillet, moved or resized through
/// the existing `edit-sketch-copy`. Only the Sketch row changes; the Fillet
/// keeps its row, its corner and radius, and every name.
mod sketch {
    use super::radius::{filleted_by_cli, filleted_without_kernel, stored_refs};
    use super::*;

    const OP: &str = "edit-sketch-copy";

    /// The saved plate as `[x0, y0, width, depth]`.
    const SAVED: [f64; 4] = [X0, Y0, W, D];

    /// A saved vertex of the plate moved to the same corner of `rect`.
    fn mapped(rect: [f64; 4], v: [f64; 2]) -> [f64; 2] {
        [
            if v[0] == X0 {
                rect[0]
            } else {
                rect[0] + rect[2]
            },
            if v[1] == Y0 {
                rect[1]
            } else {
                rect[1] + rect[3]
            },
        ]
    }

    impl Fixture {
        fn sketch_row(&self) -> &Value {
            &self.catalog["sketches"][0]
        }
        fn sketch_id(&self) -> &str {
            self.sketch_row()["sketch_id"]
                .as_str()
                .expect("Sketch UUID")
        }
        /// Every saved vertex, in saved order, sent to `at`.
        fn ask_starts(&self, at: &[[f64; 2]]) {
            let vertices: Vec<Value> = self.sketch_row()["vertices"]
                .as_array()
                .expect("vertices")
                .iter()
                .zip(at)
                .map(|(v, p)| json!({"curve_id": v["curve_id"], "start_mm": p}))
                .collect();
            write(
                &self.request,
                &json!({"request_version": 1, "vertices": vertices}),
            );
        }
        /// The saved plate moved and resized to `rect`, corner for corner.
        fn ask_rect(&self, rect: [f64; 4]) {
            let at: Vec<[f64; 2]> = self.sketch_row()["vertices"]
                .as_array()
                .expect("vertices")
                .iter()
                .map(|v| {
                    let p = &v["start_mm"];
                    mapped(rect, [p[0].as_f64().expect("x"), p[1].as_f64().expect("y")])
                })
                .collect();
            self.ask_starts(&at);
        }
        fn redraw(&self, source: &Path, version: &str, output: &Path) -> Command {
            let mut c = cli();
            c.arg(OP)
                .arg(source)
                .arg("--sketch")
                .arg(self.sketch_id())
                .arg("--expect-version")
                .arg(version)
                .arg("--request")
                .arg(&self.request)
                .arg("-o")
                .arg(output)
                .arg("--json");
            c
        }
    }

    /// Every SQL cell survives except exactly one row's payload and hash and
    /// the stamp. Returns how many cells moved.
    fn only_this_row_changed(source: &Path, copy: &Path, row: &str) -> usize {
        let before = tables(source);
        let after = tables(copy);
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>()
        );
        let id: ferritecad_types::ObjectId = row.parse().expect("UUID");
        let selected = rusqlite::types::Value::Blob(id.to_bytes().to_vec());
        let mut moved = 0;
        for (table, (columns, rows)) in &before {
            let (theirs, mine) = &after[table];
            assert_eq!(columns, theirs, "{table} columns");
            assert_eq!(rows.len(), mine.len(), "{table} rows");
            for (a, b) in rows.iter().zip(mine) {
                for (i, column) in columns.iter().enumerate() {
                    if a[i] == b[i] {
                        continue;
                    }
                    moved += 1;
                    let allowed = match (table.as_str(), column.as_str()) {
                        ("objects", "payload" | "payload_hash") => a.contains(&selected),
                        ("meta", "modified_at") => true,
                        _ => false,
                    };
                    assert!(allowed, "{table}.{column} changed");
                }
            }
        }
        moved
    }

    fn exact([_, _, w, d]: [f64; 4], r: f64, h: f64) -> f64 {
        (w * d - (1. - PI / 4.) * r * r) * h
    }

    /// Discovery on the rounded plate and the unchanged protocol, with the
    /// stub build's real order of checks. No kernel is needed.
    #[test]
    fn sketch_discovery_and_protocol_without_native() {
        let kernel = ferritecad_occt::is_available();
        if !kernel {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        }
        let plain = Fixture::drawn(Plate::new(CCW));
        assert_eq!(plain.sketch_row()["fillet_base"], Value::Null);
        let f = filleted_without_kernel(CCW, [X0 + W, Y0], 2.375);
        let row = f.sketch_row();
        assert_eq!(row["editable"], true, "{row}");
        assert_eq!(row["refusal"], Value::Null);
        assert_eq!(row["vertices"].as_array().expect("vertices").len(), 4);
        assert_eq!(row["profile_feature"]["feature_id"], f.base_id());
        assert_eq!(row["cut_history_v3"], Value::Null, "not a Cut history");
        assert_eq!(row["fillet_base"], f.base_row()["fillet_base"]);
        assert_eq!(row["fillet_base"]["corner_mm"], json!([X0 + W, Y0]));
        assert_eq!(row["fillet_base"]["radius_mm"], 2.375);
        // The circle editors of this Sketch still refuse the filleted plate;
        // its Line constraints are §28E's.
        assert_eq!(row["constraint_edit"]["available"], true);
        for editor in ["circle_edit", "annulus_edit"] {
            let reason = row[editor]["refusal"].as_str().expect("a reason");
            assert!(reason.contains(f.fillet_id()), "{editor}: {reason}");
        }

        let before = std::fs::read(&f.source).expect("source bytes");
        f.ask_rect(SAVED);
        let names = entries(f.root.path());
        let never = f.root.path().join("never.fcad");
        for (why, at, kind) in [
            (
                "too small for the radius",
                [[0., 0.], [20., 0.], [20., 4.], [0., 4.]],
                "input",
            ),
            (
                "Lines on other sides",
                [[0., 20.], [0., 0.], [40., 0.], [40., 20.]],
                "input",
            ),
            (
                // Outside the class, as §28A refuses a non-rectangle.
                "a trapezoid",
                [[0., 0.], [40., 0.], [35., 12.], [5., 12.]],
                "unsupported",
            ),
        ] {
            f.ask_starts(&at);
            let v = reply(
                f.redraw(&f.source, f.version(), &never)
                    .output()
                    .expect("process"),
                OP,
                2,
            );
            if kernel {
                assert_eq!(refused(&v), kind, "{why}: {v}");
            } else {
                assert_eq!(refused(&v), "unsupported", "{why}: {v}");
                assert!(v.to_string().contains("Open CASCADE"), "{why}: {v}");
            }
        }
        // A malformed request is refused before any kernel is asked for.
        write(
            &f.request,
            &json!({"request_version": 1, "vertices": [], "radius_mm": 1}),
        );
        let v = reply(
            f.redraw(&f.source, f.version(), &never)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert_eq!(refused(&v), "input", "{v}");
        // A refusal whose report cannot be written is still a refusal.
        f.ask_starts(&[[0., 0.], [20., 0.], [20., 4.], [0., 4.]]);
        assert_eq!(
            f.redraw(&f.source, f.version(), &never)
                .stdout(pipe::closed_pipe())
                .stderr(pipe::closed_pipe())
                .status()
                .expect("pipes")
                .code(),
            Some(7)
        );
        assert_eq!(entries(f.root.path()), names, "a refusal left something");
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);

        // A Fillet outside the frame keeps the Sketch refused, naming it.
        let odd = filleted_without_kernel(CCW, [X0, Y0], 2.0);
        let mut d = Document::open(&odd.source).expect("writable");
        let owner: ferritecad_types::ObjectId = odd.fillet_id().parse().expect("UUID");
        d.write(|w| {
            w.put_topology_ref(&ferritecad_document::TopologyRef {
                id: ferritecad_types::StableEntityId::new(),
                owner,
                producer_feature: owner,
                expected_kind: ferritecad_document::EntityKind::Face,
                output_role: SemanticRole::FilletFace {
                    source_edge: ferritecad_types::StableEntityId::new(),
                },
                selection: ferritecad_document::SelectionRule::Exact,
                fallback_signature: None,
            })
        })
        .expect("an extra name");
        d.close().expect("close");
        let catalog = inspect(&odd.source);
        let row = &catalog["sketches"][0];
        assert_eq!(row["editable"], false);
        assert_eq!(row["fillet_base"], Value::Null);
        let reason = row["refusal"].as_str().expect("reason");
        assert!(
            reason.contains(odd.fillet_id()) && reason.contains("more faces"),
            "{reason}"
        );
    }

    /// One Sketch edit of `f`'s plate to `rect`, checked against everything
    /// a copy must be.
    fn redrawn(
        f: &Fixture,
        corner: [f64; 2],
        r: f64,
        h: f64,
        rect: [f64; 4],
        name: &str,
    ) -> (PathBuf, Measured) {
        let before = std::fs::read(&f.source).expect("source bytes");
        let refs = stored_refs(&f.source);
        let fillet_before = f.fillet_row().clone();
        let copy = f.root.path().join(format!("{name}.fcad"));
        f.ask_rect(rect);
        let published = reply(
            f.redraw(&f.source, f.version(), &copy)
                .output()
                .expect("process"),
            OP,
            0,
        );
        assert_eq!(published["result"]["sketch_id"], f.sketch_id());
        assert_eq!(
            std::fs::read(&f.source).expect("bytes"),
            before,
            "source touched"
        );
        // The Sketch row's payload and hash; the coordinate writer does not
        // stamp modified_at.
        assert_eq!(only_this_row_changed(&f.source, &copy, f.sketch_id()), 2);
        assert_eq!(stored_refs(&copy), refs, "every name and its UUID kept");
        let after = inspect(&copy);
        let moved = mapped(rect, corner);
        assert_eq!(after["features"][0]["distance_mm"], h, "the height");
        let fillet = &after["fillets"][0];
        assert_eq!(fillet["feature_id"], fillet_before["feature_id"]);
        assert_eq!(fillet["edge"], fillet_before["edge"], "the same edge");
        assert_eq!(fillet["radius_mm"], r, "the same radius");
        assert_eq!(fillet["corner_mm"], json!(moved), "the same corner, moved");
        assert_eq!(
            after["sketches"][0]["fillet_base"]["corner_mm"],
            json!(moved)
        );

        let checked = cli()
            .arg("validate")
            .arg(&copy)
            .arg("--json")
            .output()
            .expect("validate");
        assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
        let rebuilt = cli()
            .arg("rebuild")
            .arg(&copy)
            .arg("--cold")
            .output()
            .expect("rebuild");
        let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
        let n = refs.len();
        assert!(
            text.contains(&format!("{n} of {n} stored references resolved")),
            "{text}"
        );

        let cold = measure(&copy, None);
        let cache = copy.with_extension("fcad-cache");
        cold.same_as(&measure(
            &copy,
            Some((&cache, ferritecad_eval::CacheOutcome::Miss)),
        ));
        cold.same_as(&measure(
            &copy,
            Some((&cache, ferritecad_eval::CacheOutcome::Hit)),
        ));
        assert_eq!(cold.faces, 7);
        let exact = exact(rect, r, h);
        assert!(
            (cold.volume - exact).abs() < 1e-9 * exact,
            "{} is not {exact}",
            cold.volume
        );
        // The same named face is a cylinder of the same radius on a vertical
        // axis r inward of the moved corner.
        assert_eq!(cold.fillet, FaceSurface::Cylinder { radius: r });
        let centre = inward_in([rect[0], rect[1]], moved, r);
        assert!(
            (cold.axis_origin[0] - centre[0]).abs() < 1e-9
                && (cold.axis_origin[1] - centre[1]).abs() < 1e-9
                && cold.axis_direction[0].abs() < 1e-12
                && cold.axis_direction[1].abs() < 1e-12,
            "the axis {:?} is not r inward of {moved:?}",
            cold.axis_origin
        );
        assert_eq!(
            cold.roles["origin side of feature.fillet"],
            [FaceSurface::Plane; 4]
        );
        for side in ["Start", "End"] {
            assert_eq!(
                cold.roles[&format!("origin cap {side} of feature.fillet")],
                [FaceSurface::Plane]
            );
        }
        let m = mesh(&copy, &copy.with_extension("stl"));
        check_mesh_in(&m, rect, moved, r, h);
        fbx(&copy, name);
        (copy, cold)
    }

    /// Grow and move, then shrink, a fractional rectangle on two different
    /// corners across fixtures of both windings and another starting Line.
    /// The rounded face stays the same named face of the same radius, at the
    /// corner its two Lines now meet.
    #[test]
    fn native_sketch_edits_move_the_rounded_corner_and_keep_every_identity() {
        if !native() {
            return;
        }
        let up = [-9.25, -2.5, 51.0, 19.625];
        let down = [1.375, 4.0, 18.5, 8.25];
        for (label, corners, corner) in [
            ("ccw", CCW, [X0 + W, Y0]),
            ("cw", CW_FROM_UPPER_RIGHT, [X0, Y0 + D]),
            ("third", CCW_FROM_THIRD, [X0 + W, Y0]),
        ] {
            let r = 3.0625;
            let f = filleted_by_cli(corners, corner, r, &format!("rounded-{label}"));
            assert_eq!(f.sketch_row()["editable"], true);
            let (grown, _) = redrawn(&f, corner, r, H, up, &format!("sketch-{label}-up"));
            // A second edit of the edited copy, back inside the saved plate.
            let g = Fixture {
                catalog: inspect(&grown),
                source: grown,
                root: f.root,
                request: f.request.clone(),
            };
            // The copy's corner, as the saved vertices now stand.
            let now = mapped(up, corner);
            let shrink: Vec<[f64; 2]> = g.sketch_row()["vertices"]
                .as_array()
                .expect("vertices")
                .iter()
                .map(|v| {
                    let p = &v["start_mm"];
                    let (x, y) = (p[0].as_f64().expect("x"), p[1].as_f64().expect("y"));
                    [
                        if x == up[0] {
                            down[0]
                        } else {
                            down[0] + down[2]
                        },
                        if y == up[1] {
                            down[1]
                        } else {
                            down[1] + down[3]
                        },
                    ]
                })
                .collect();
            g.ask_starts(&shrink);
            let lower = g.root.path().join(format!("sketch-{label}-down.fcad"));
            reply(
                g.redraw(&g.source, g.version(), &lower)
                    .output()
                    .expect("process"),
                OP,
                0,
            );
            assert_eq!(stored_refs(&lower), stored_refs(&g.source));
            let m = measure(&lower, None);
            assert!((m.volume - exact(down, r, H)).abs() < 1e-9 * m.volume);
            let moved = [
                if now[0] == up[0] {
                    down[0]
                } else {
                    down[0] + down[2]
                },
                if now[1] == up[1] {
                    down[1]
                } else {
                    down[1] + down[3]
                },
            ];
            let centre = inward_in([down[0], down[1]], moved, r);
            assert!(
                (m.axis_origin[0] - centre[0]).abs() < 1e-9
                    && (m.axis_origin[1] - centre[1]).abs() < 1e-9
            );
            let copy = inspect(&lower);
            assert_eq!(copy["fillets"][0]["corner_mm"], json!(moved));
            let mesh_out = lower.with_extension("stl");
            check_mesh_in(&mesh(&lower, &mesh_out), down, moved, r, H);
            fbx(&lower, &format!("sketch-{label}-down"));
        }
    }

    /// The saved coordinates publish the same model.
    #[test]
    fn native_the_same_rectangle_publishes_the_same_model() {
        if !native() {
            return;
        }
        let f = filleted_by_cli(CCW, [X0, Y0], 2.375, "rounded");
        let copy = f.root.path().join("same.fcad");
        f.ask_rect(SAVED);
        reply(
            f.redraw(&f.source, f.version(), &copy)
                .output()
                .expect("process"),
            OP,
            0,
        );
        assert_eq!(only_this_row_changed(&f.source, &copy, f.sketch_id()), 0);
        measure(&f.source, None).same_as(&measure(&copy, None));

        // A rectangle accepted by the shared reader may contain tiny
        // off-axis noise. Both introducing and removing it must preserve
        // the side and the chosen Fillet, without silently snapping it.
        let mut noisy = CCW;
        noisy[2][1] += 1e-10;
        f.ask_starts(&noisy);
        let near = f.root.path().join("near-axis.fcad");
        reply(
            f.redraw(&f.source, f.version(), &near)
                .output()
                .expect("near-axis edit"),
            OP,
            0,
        );
        let near_catalog = inspect(&near);
        assert_eq!(
            near_catalog["sketches"][0]["vertices"][2]["start_mm"],
            json!(noisy[2]),
            "the accepted coordinates are not snapped"
        );
        assert_eq!(stored_refs(&near), stored_refs(&f.source));
        let measured = measure(&near, None);
        assert_eq!(measured.faces, 7);
        assert_eq!(measured.fillet, FaceSurface::Cylinder { radius: 2.375 });
        let volume = exact(SAVED, 2.375, H);
        assert!((measured.volume - volume).abs() < volume * 1e-9);
        let normalized = f.root.path().join("exact-axis.fcad");
        f.ask_rect(SAVED);
        reply(
            f.redraw(
                &near,
                near_catalog["content_version"].as_str().expect("version"),
                &normalized,
            )
            .output()
            .expect("exact-axis edit"),
            OP,
            0,
        );
        assert_eq!(
            only_this_row_changed(&f.source, &normalized, f.sketch_id()),
            0
        );
        measure(&f.source, None).same_as(&measure(&normalized, None));
    }

    /// The cache under one document path: after the rectangle changes there,
    /// the plate and the Fillet both miss and neither returns the old part.
    /// Then Sketch → height → radius → Sketch on the same UUIDs.
    #[test]
    fn native_a_moved_rectangle_misses_in_place_and_interleaves_with_height_and_radius() {
        if !native() {
            return;
        }
        let r = 2.375;
        let corner = [X0 + W, Y0 + D];
        let f = filleted_by_cli(CCW, corner, r, "rounded");
        let place = f.root.path().join("in-place.fcad");
        std::fs::copy(&f.source, &place).expect("copy");
        let cache = place.with_extension("fcad-cache");
        let (first, _) = measure_events(&place, Some(&cache));
        let (again, events) = measure_events(&place, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
        );
        first.same_as(&again);

        let step1 = [0.5, 1.25, 30.75, 10.5];
        let edited = f.root.path().join("step1.fcad");
        f.ask_rect(step1);
        reply(
            f.redraw(&f.source, f.version(), &edited)
                .output()
                .expect("process"),
            OP,
            0,
        );
        std::fs::copy(&edited, &place).expect("replace in place");
        let base: ferritecad_types::ObjectId = f.base_id().parse().expect("UUID");
        let fillet: ferritecad_types::ObjectId = f.fillet_id().parse().expect("UUID");
        let (changed, events) = measure_events(&place, Some(&cache));
        for event in &events {
            let expected = if event.feature == fillet || event.feature == base {
                ferritecad_eval::CacheOutcome::Miss
            } else {
                ferritecad_eval::CacheOutcome::Hit
            };
            assert_eq!(event.outcome, expected, "{events:?}");
        }
        assert!(events.iter().any(|e| e.feature == fillet));
        assert!(events.iter().any(|e| e.feature == base));
        assert!((changed.volume - exact(step1, r, H)).abs() < 1e-9 * changed.volume);
        assert!(
            (changed.volume - first.volume).abs() > 1.0,
            "not the old part"
        );
        let (hit, events) = measure_events(&place, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
        );
        changed.same_as(&hit);
        changed.same_as(&measure(&place, None));

        // Height, then radius, then the rectangle again.
        let fixture = |path: PathBuf, root| Fixture {
            catalog: inspect(&path),
            source: path,
            root,
            request: f.request.clone(),
        };
        let g = fixture(edited, f.root);
        let taller = g.root.path().join("step2.fcad");
        reply(
            g.raise(&g.source, g.base_id(), "9.75", &taller)
                .output()
                .expect("process"),
            "edit-extrude",
            0,
        );
        let h = fixture(taller, g.root);
        let rounder = h.root.path().join("step3.fcad");
        h.ask_radius(4.5);
        reply(
            h.edit(&rounder).output().expect("process"),
            "edit-fillet-radius",
            0,
        );
        let k = fixture(rounder, h.root);
        let step4 = [-6.0, 2.0, 22.25, 13.5];
        let last = k.root.path().join("step4.fcad");
        k.ask_starts(
            &k.sketch_row()["vertices"]
                .as_array()
                .expect("vertices")
                .iter()
                .map(|v| {
                    let p = &v["start_mm"];
                    let (x, y) = (p[0].as_f64().expect("x"), p[1].as_f64().expect("y"));
                    [
                        if x == step1[0] {
                            step4[0]
                        } else {
                            step4[0] + step4[2]
                        },
                        if y == step1[1] {
                            step4[1]
                        } else {
                            step4[1] + step4[3]
                        },
                    ]
                })
                .collect::<Vec<_>>(),
        );
        reply(
            k.redraw(&k.source, k.version(), &last)
                .output()
                .expect("process"),
            OP,
            0,
        );
        assert_eq!(stored_refs(&last), stored_refs(&f.source), "the same names");
        let m = measure(&last, None);
        assert!((m.volume - exact(step4, 4.5, 9.75)).abs() < 1e-9 * m.volume);
        assert_eq!(m.fillet, FaceSurface::Cylinder { radius: 4.5 });
        let corner_now = [step4[0] + step4[2], step4[1] + step4[3]];
        let centre = inward_in([step4[0], step4[1]], corner_now, 4.5);
        assert!(
            (m.axis_origin[0] - centre[0]).abs() < 1e-9
                && (m.axis_origin[1] - centre[1]).abs() < 1e-9,
            "the rounded edge was lost: {:?}",
            m.axis_origin
        );
        assert_eq!(inspect(&last)["fillets"][0]["corner_mm"], json!(corner_now));
    }

    /// Destinations, a stale source, cancellation, an output race at the last
    /// barrier and a lost report: each atomic. The structural refusals are in
    /// the protocol test above.
    #[test]
    fn native_sketch_refusals_races_cancellation_and_report_loss_are_atomic() {
        if !native() {
            return;
        }
        let f = filleted_by_cli(CCW, [X0, Y0], 2.0, "rounded");
        let before = std::fs::read(&f.source).expect("bytes");
        let taken = f.root.path().join("taken.fcad");
        std::fs::write(&taken, b"another process owns this").expect("occupied");
        let alias = f.root.path().join("alias.fcad");
        std::fs::hard_link(&f.source, &alias).expect("hard link");
        let target = [1.5, 2.25, 30.0, 11.0];
        f.ask_rect(target);
        for (destination, why) in [
            (f.source.clone(), "the source"),
            (taken.clone(), "an occupied output"),
            (alias.clone(), "an alias of the source"),
        ] {
            let v = reply(
                f.redraw(&f.source, f.version(), &destination)
                    .output()
                    .expect("process"),
                OP,
                2,
            );
            assert_eq!(refused(&v), "input", "{why}: {v}");
        }
        assert_eq!(
            std::fs::read(&taken).expect("taken"),
            b"another process owns this"
        );
        // Another Sketch UUID and a stale version.
        let never = f.root.path().join("never.fcad");
        let mut c = cli();
        c.arg(OP)
            .arg(&f.source)
            .arg("--sketch")
            .arg(ferritecad_types::ObjectId::new().to_string())
            .arg("--expect-version")
            .arg(f.version())
            .arg("--request")
            .arg(&f.request)
            .arg("-o")
            .arg(&never)
            .arg("--json");
        let v = reply(c.output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "a foreign UUID: {v}");
        let v = reply(
            f.redraw(&f.source, &"0".repeat(64), &never)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert_eq!(refused(&v), "input", "stale: {v}");
        assert!(!never.exists());

        // The Fillet's radius raised after the source was inspected: the old
        // version no longer describes it, and the job refuses before writing.
        let stale_copy = f.root.path().join("stale-source.fcad");
        let rounder = f.root.path().join("rounder.fcad");
        f.ask_radius(4.0);
        reply(
            f.edit(&rounder).output().expect("process"),
            "edit-fillet-radius",
            0,
        );
        std::fs::copy(&rounder, &stale_copy).expect("copy");
        f.ask_rect(target);
        let stale_out = f.root.path().join("stale.fcad");
        let v = reply(
            f.redraw(&stale_copy, f.version(), &stale_out)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert_eq!(refused(&v), "input", "{v}");
        assert!(!stale_out.exists());

        let d = Document::open_read_only(&f.source).expect("source");
        let expected = ferritecad_document::DocumentVersion {
            document_id: d.meta().document_id,
            content: d.content_version().expect("version"),
        };
        d.close().expect("close");
        let vertices: Vec<ferritecad_document::SketchVertex> = f.sketch_row()["vertices"]
            .as_array()
            .expect("vertices")
            .iter()
            .map(|v| {
                let p = &v["start_mm"];
                ferritecad_document::SketchVertex {
                    curve_id: v["curve_id"].as_str().expect("id").parse().expect("UUID"),
                    start_mm: mapped(
                        target,
                        [p[0].as_f64().expect("x"), p[1].as_f64().expect("y")],
                    ),
                }
            })
            .collect();
        let request = |destination: &Path| ferritecad_jobs::EditSketchRequest {
            source: f.source.clone(),
            expected,
            sketch: f.sketch_id().parse().expect("UUID"),
            vertices: vertices.clone(),
            destination: destination.to_path_buf(),
        };
        for at in [0.0, 0.95] {
            let destination = f.root.path().join(format!("cancelled-{at}.fcad"));
            let token = ferritecad_kernel::CancelToken::new();
            let stop = token.clone();
            if at == 0.0 {
                token.cancel();
            }
            let context = OperationContext::default()
                .with_cancel(token)
                .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                    if fraction >= at {
                        stop.cancel();
                    }
                }));
            let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
            let result =
                ferritecad_jobs::edit_sketch_copy(&request(&destination), &mut kernel, &context);
            assert!(result.is_err(), "cancelled at {at}: {result:?}");
            assert!(!destination.exists());
            assert_eq!(kernel.live_shape_count(), 0);
        }
        let raced = f.root.path().join("raced.fcad");
        let racer = raced.clone();
        let context = OperationContext::default().with_progress(
            ferritecad_kernel::ProgressSink::new(move |fraction| {
                if fraction == 0.95 {
                    std::fs::write(&racer, b"racing file").expect("racer");
                }
            }),
        );
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        assert!(
            ferritecad_jobs::edit_sketch_copy(&request(&raced), &mut kernel, &context).is_err()
        );
        assert_eq!(std::fs::read(&raced).expect("racer kept"), b"racing file");

        // A publication whose report is lost stays published: exit 7.
        let lost = f.root.path().join("lost.fcad");
        assert_eq!(
            f.redraw(&f.source, f.version(), &lost)
                .stdout(pipe::closed_pipe())
                .stderr(pipe::closed_pipe())
                .status()
                .expect("pipes")
                .code(),
            Some(7)
        );
        assert_eq!(
            inspect(&lost)["fillets"][0]["corner_mm"],
            json!(mapped(target, [X0, Y0]))
        );
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    }
}

/// §28E: the Line constraints of the rectangle under the saved Fillet,
/// through the existing `edit-sketch-constraints-copy`. The stored Lines stay
/// the solver's starting guess; the part, and the Fillet's corner and radius,
/// are the solved plate's.
mod constraints {
    use super::radius::{filleted_by_cli, filleted_without_kernel, stored_refs};
    use super::*;

    const OP: &str = "edit-sketch-constraints-copy";

    /// A kernel and a solver, or why this build has none. A build required
    /// to link PlaneGCS must have it: this never skips there.
    fn solving() -> bool {
        if !super::native() {
            return false;
        }
        if ferritecad_eval::solver_available() {
            return true;
        }
        assert_ne!(
            std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
            Ok("1")
        );
        eprintln!("skipped: constrained geometry requires PlaneGCS");
        false
    }

    impl Fixture {
        fn constraint_row(&self) -> &Value {
            &self.catalog["sketches"][0]["constraint_edit"]
        }
        fn plate_sketch_id(&self) -> &str {
            self.catalog["sketches"][0]["sketch_id"]
                .as_str()
                .expect("Sketch UUID")
        }
        /// The stored Line `i`, in stored order.
        fn line(&self, i: usize) -> Value {
            self.constraint_row()["curves"][i]["curve_id"].clone()
        }
        fn stored_starts(&self) -> Vec<[f64; 2]> {
            self.constraint_row()["curves"]
                .as_array()
                .expect("curves")
                .iter()
                .map(|c| {
                    let p = &c["start_mm"];
                    [p[0].as_f64().expect("x"), p[1].as_f64().expect("y")]
                })
                .collect()
        }
        fn ask_constraints(&self, remove: &[Value], add: &[Value]) {
            write(
                &self.request,
                &json!({"request_version": 1, "remove": remove, "add": add}),
            );
        }
        fn constrain(&self, output: &Path) -> Command {
            let mut c = cli();
            c.arg(OP)
                .arg(&self.source)
                .arg("--sketch")
                .arg(self.plate_sketch_id())
                .arg("--expect-version")
                .arg(self.version())
                .arg("--request")
                .arg(&self.request)
                .arg("-o")
                .arg(output)
                .arg("--json");
            c
        }
        /// The same fixture, reading `path` instead.
        fn at_copy(&self, path: &Path) -> Fixture {
            Fixture {
                root: tempfile::tempdir().expect("directory"),
                source: path.to_path_buf(),
                request: self.request.clone(),
                catalog: inspect(path),
            }
        }
    }

    fn rule(line: &Value, rule: &str) -> Value {
        json!({"curve_id": line, "rule": rule})
    }
    fn length(line: &Value, mm: f64) -> Value {
        json!({"curve_id": line, "rule": "distance", "distance_mm": mm})
    }
    fn pin(line: &Value, x: f64, y: f64) -> Value {
        json!({"curve_id": line, "rule": "fixed", "at": "start", "x_mm": x, "y_mm": y})
    }

    /// Which stored Lines run along X, by index.
    fn horizontal(starts: &[[f64; 2]]) -> Vec<bool> {
        (0..starts.len())
            .map(|i| starts[i][1] == starts[(i + 1) % starts.len()][1])
            .collect()
    }

    /// Where a stored vertex lands when the stored rectangle is solved to one
    /// whose first Line starts at `at`, `width` along X and `depth` along Y,
    /// every side kept.
    fn solved_vertex(
        starts: &[[f64; 2]],
        v: [f64; 2],
        at: [f64; 2],
        width: f64,
        depth: f64,
    ) -> [f64; 2] {
        let s = starts[0];
        [
            if v[0] == s[0] {
                at[0]
            } else {
                at[0] + (v[0] - s[0]).signum() * width
            },
            if v[1] == s[1] {
                at[1]
            } else {
                at[1] + (v[1] - s[1]).signum() * depth
            },
        ]
    }

    /// `[x0, y0, width, depth]` of the rectangle through these vertices.
    fn rect_of(vertices: &[[f64; 2]]) -> [f64; 4] {
        let lo = |k: usize| vertices.iter().map(|v| v[k]).fold(f64::INFINITY, f64::min);
        let hi = |k: usize| {
            vertices
                .iter()
                .map(|v| v[k])
                .fold(f64::NEG_INFINITY, f64::max)
        };
        [lo(0), lo(1), hi(0) - lo(0), hi(1) - lo(1)]
    }

    /// The Line starts the rebuild built the plate from, and the solve's
    /// degrees of freedom, read in-process from a cold rebuild.
    fn solved(path: &Path) -> (Vec<[f64; 2]>, usize) {
        let d = Document::open_read_only(path).expect("reopen");
        let objects = d.objects().expect("objects");
        let sketch = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("the plate's Sketch");
        let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
        let built = ferritecad_eval::rebuild_cold(&d, &mut k, &OperationContext::default())
            .expect("cold rebuild");
        let report = built.solve_report(sketch.id).expect("a solve report");
        assert!(report.redundant().is_empty(), "{report:?}");
        let starts = built
            .sketch_presentation(sketch.id)
            .expect("presentation")
            .curves()
            .iter()
            .map(|c| match c.geometry() {
                ferritecad_document::SketchGeometry::Line { start, .. } => [start.x, start.y],
                other => panic!("a Line, not {other:?}"),
            })
            .collect();
        let dof = report.degrees_of_freedom();
        built.release_all(&mut k);
        d.close().expect("close");
        (starts, dof)
    }

    /// Every SQL cell survives except the selected Sketch row's schema
    /// version, payload and hash, and the `sketch.constraints` capability the
    /// existing policy records; rows keep their counts but for that one row.
    /// Returns how many cells moved.
    fn only_this_sketch_changed(source: &Path, copy: &Path, sketch: &str) -> usize {
        let before = tables(source);
        let after = tables(copy);
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>()
        );
        let id: ferritecad_types::ObjectId = sketch.parse().expect("UUID");
        let selected = rusqlite::types::Value::Blob(id.to_bytes().to_vec());
        let mut moved = 0;
        for (table, (columns, rows)) in &before {
            let (theirs, mine) = &after[table];
            assert_eq!(columns, theirs, "{table} columns");
            if table == "capabilities" {
                let text = |r: &Vec<rusqlite::types::Value>| format!("{r:?}");
                let kept: Vec<_> = rows.iter().map(text).collect();
                let new: Vec<_> = mine.iter().map(text).collect();
                assert!(kept.iter().all(|r| new.contains(r)), "a capability changed");
                assert!(
                    new.iter()
                        .filter(|r| !kept.contains(r))
                        .all(|r| r.contains("sketch.constraints.v1")),
                    "{new:?}"
                );
                assert!(mine.len() <= rows.len() + 1);
                continue;
            }
            assert_eq!(rows.len(), mine.len(), "{table} rows");
            for (a, b) in rows.iter().zip(mine) {
                for (i, column) in columns.iter().enumerate() {
                    if a[i] == b[i] {
                        continue;
                    }
                    moved += 1;
                    let allowed = match (table.as_str(), column.as_str()) {
                        ("objects", "schema_version" | "payload" | "payload_hash") => {
                            a.contains(&selected)
                        }
                        ("meta", "modified_at") => true,
                        _ => false,
                    };
                    assert!(allowed, "{table}.{column} changed");
                }
            }
        }
        assert!(
            capability_names(copy).contains(&"sketch.constraints.v1".to_owned()),
            "{:?}",
            capability_names(copy)
        );
        moved
    }

    fn exact([_, _, w, d]: [f64; 4], r: f64, h: f64) -> f64 {
        (w * d - (1. - PI / 4.) * r * r) * h
    }

    /// The axis a Fillet of radius `r` at `corner` of `rect` has: r inward
    /// along both sides.
    fn axis_of(rect: [f64; 4], corner: [f64; 2], r: f64) -> [f64; 2] {
        let [x0, y0, w, d] = rect;
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(near(corner[0], x0) || near(corner[0], x0 + w));
        assert!(near(corner[1], y0) || near(corner[1], y0 + d));
        [
            corner[0] + if near(corner[0], x0) { r } else { -r },
            corner[1] + if near(corner[1], y0) { r } else { -r },
        ]
    }

    /// One published constraint copy of `f` measured against the rectangle
    /// it must solve to: identities, SQL, the stored guess, the solve, the
    /// B-Rep, the cache, the mesh and FBX. Returns the copy and its rect.
    #[allow(clippy::too_many_arguments)]
    fn published(
        f: &Fixture,
        corner: [f64; 2],
        r: f64,
        expected: &[[f64; 2]],
        dof: usize,
        name: &str,
    ) -> PathBuf {
        let before = std::fs::read(&f.source).expect("source bytes");
        let refs = stored_refs(&f.source);
        let stored = f.stored_starts();
        let fillet_before = f.catalog["fillets"][0].clone();
        let copy = f.root.path().join(format!("{name}.fcad"));
        let v = reply(f.constrain(&copy).output().expect("process"), OP, 0);
        assert_eq!(v["result"]["sketch_id"], f.plate_sketch_id());
        assert_eq!(v["result"]["solve"]["degrees_of_freedom"], dof, "{v}");
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
        only_this_sketch_changed(&f.source, &copy, f.plate_sketch_id());
        assert_eq!(stored_refs(&copy), refs, "every name and its UUID kept");

        // The stored Lines are still the starting guess, UUID for UUID; the
        // Fillet row is the saved one.
        let after = inspect(&copy);
        let g = f.at_copy(&copy);
        assert_eq!(g.stored_starts(), stored, "stored coordinates are kept");
        for i in 0..4 {
            assert_eq!(g.line(i), f.line(i), "Line {i}");
        }
        let fillet = &after["fillets"][0];
        for key in ["feature_id", "edge", "radius_mm", "previous_feature_id"] {
            assert_eq!(fillet[key], fillet_before[key], "{key}");
        }
        assert_eq!(fillet["profile_constrained"], true);
        assert_eq!(fillet["radius_edit"]["max_radius_mm"], Value::Null);
        let context = &g.constraint_row()["fillet_base"];
        assert_eq!(context["fillet_feature_id"], fillet_before["feature_id"]);
        assert_eq!(context["stored_corner_mm"], json!(corner));
        assert_eq!(context["radius_mm"], r);

        // The solve, measured: the solved plate is the expected rectangle,
        // Line for Line, with no snapping and no residue beyond 1e-9 mm.
        let (starts, measured_dof) = solved(&copy);
        assert_eq!(measured_dof, dof, "DOF of the cold rebuild");
        let mut worst: f64 = 0.;
        for (s, e) in starts.iter().zip(expected) {
            worst = worst.max((s[0] - e[0]).abs()).max((s[1] - e[1]).abs());
        }
        eprintln!("FCAD_28E_SOLVED {name} worst_mm={worst:e}");
        assert!(worst < 1e-9, "{starts:?} is not {expected:?}");
        let rect = rect_of(expected);
        let moved = expected[stored.iter().position(|v| *v == corner).expect("corner")];

        // The B-Rep: 7 faces, the analytic volume, the same named face a
        // cylinder of the saved radius about an axis r inward of the solved
        // corner.
        let n = refs.len();
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
        let cold = measure(&copy, None);
        let cache = copy.with_extension("fcad-cache");
        cold.same_as(&measure(
            &copy,
            Some((&cache, ferritecad_eval::CacheOutcome::Miss)),
        ));
        cold.same_as(&measure(
            &copy,
            Some((&cache, ferritecad_eval::CacheOutcome::Hit)),
        ));
        assert_eq!(cold.faces, 7);
        let volume = exact(rect, r, H);
        assert!(
            (cold.volume - volume).abs() < 1e-9 * volume,
            "{} is not {volume}",
            cold.volume
        );
        assert_eq!(cold.fillet, FaceSurface::Cylinder { radius: r });
        let axis = axis_of(rect, moved, r);
        assert!(
            (cold.axis_origin[0] - axis[0]).abs() < 1e-9
                && (cold.axis_origin[1] - axis[1]).abs() < 1e-9
                && cold.axis_direction[0].abs() < 1e-12
                && cold.axis_direction[1].abs() < 1e-12,
            "the axis {:?} is not r inward of {moved:?}",
            cold.axis_origin
        );
        let m = mesh(&copy, &copy.with_extension("stl"));
        check_mesh_in(&m, rect, moved, r, H);
        fbx(&copy, name);
        copy
    }

    /// The full dimensioning of `f`'s rectangle: H/V on every Line, the first
    /// Line's start pinned at `at`, the width and the depth.
    fn dimensioned(f: &Fixture, at: [f64; 2], width: f64, depth: f64) -> Vec<Value> {
        let starts = f.stored_starts();
        let across = horizontal(&starts);
        let mut add: Vec<Value> = (0..4)
            .map(|i| {
                rule(
                    &f.line(i),
                    if across[i] { "horizontal" } else { "vertical" },
                )
            })
            .collect();
        add.push(pin(&f.line(0), at[0], at[1]));
        let h = across.iter().position(|a| *a).expect("a horizontal Line");
        let v = across.iter().position(|a| !*a).expect("a vertical Line");
        add.push(length(&f.line(h), width));
        add.push(length(&f.line(v), depth));
        add
    }

    /// Discovery on a rounded plate whose Sketch carries constraints, and the
    /// protocol, with no kernel or solver needed: the stored corner is named
    /// as stored, the bound is deferred, the coordinate editor refuses, the
    /// constraint editor offers the Sketch, and a build without the kernel or
    /// the solver refuses a well-formed request before writing anything.
    #[test]
    fn constraint_discovery_and_protocol_without_native() {
        let plain = filleted_without_kernel(CCW, [X0 + W, Y0], 2.375);
        let row = plain.constraint_row();
        assert_eq!(row["available"], true, "{row}");
        assert_eq!(row["fillet_base"]["stored_corner_mm"], json!([X0 + W, Y0]));
        assert_eq!(
            row["fillet_base"]["fillet_feature_id"],
            plain.catalog["fillets"][0]["feature_id"]
        );
        assert_eq!(plain.catalog["fillets"][0]["profile_constrained"], false);
        assert_eq!(
            plain.catalog["fillets"][0]["radius_edit"]["max_radius_mm"],
            6.125
        );
        assert_eq!(
            plain.catalog["features"][0]["fillet_base"]["profile_constrained"],
            false
        );

        // Constraints written by the shipped preparation and writer, no kernel.
        let mut d = Document::open(&plain.source).expect("writable");
        let sketch: ferritecad_types::ObjectId = plain.plate_sketch_id().parse().expect("UUID");
        let line = |i: usize| -> ferritecad_types::StableEntityId {
            plain.line(i).as_str().expect("UUID").parse().expect("UUID")
        };
        let edits = ferritecad_document::SketchConstraintEdits {
            remove: Vec::new(),
            add: vec![ferritecad_document::AddSketchConstraint::Line(
                ferritecad_document::AddLineConstraint::Line {
                    curve: line(0),
                    kind: ferritecad_document::LineConstraintKind::Horizontal,
                },
            )],
        };
        let prepared =
            ferritecad_document::prepare_sketch_constraints(&d, sketch, &edits).expect("prepared");
        d.write_sketch_constraints(&prepared).expect("written");
        d.close().expect("close");
        let f = Fixture {
            catalog: inspect(&plain.source),
            ..plain
        };
        let row = f.constraint_row();
        assert_eq!(row["available"], true);
        assert_eq!(
            row["constraints"].as_array().expect("list").len(),
            5,
            "{row}"
        );
        let fillet = &f.catalog["fillets"][0];
        assert_eq!(fillet["profile_constrained"], true);
        assert_eq!(
            fillet["corner_mm"],
            json!([X0 + W, Y0]),
            "the stored corner"
        );
        assert_eq!(fillet["radius_edit"]["available"], true);
        assert_eq!(fillet["radius_edit"]["max_radius_mm"], Value::Null);
        assert_eq!(
            f.catalog["features"][0]["fillet_base"]["profile_constrained"],
            true
        );
        assert_eq!(f.catalog["edit_extrude"]["available"], true, "§28C");
        let sketch_row = &f.catalog["sketches"][0];
        assert_eq!(
            sketch_row["editable"], false,
            "coordinates of a constrained Sketch"
        );
        assert!(
            sketch_row["refusal"]
                .as_str()
                .expect("reason")
                .contains("unconstrained"),
            "{sketch_row}"
        );

        // The protocol. Without a kernel the kernel is asked for after the
        // request is read; with a kernel and no solver the solve is refused;
        // with both, this example publishes (covered natively).
        let before = std::fs::read(&f.source).expect("bytes");
        let never = f.root.path().join("never.fcad");
        f.ask_constraints(&[], &[length(&f.line(1), 11.5)]);
        let names = entries(f.root.path());
        let solver = ferritecad_occt::is_available() && ferritecad_eval::solver_available();
        if !solver {
            let v = reply(f.constrain(&never).output().expect("process"), OP, 2);
            assert_eq!(refused(&v), "unsupported", "{v}");
            if !ferritecad_occt::is_available() {
                assert!(v.to_string().contains("Open CASCADE"), "{v}");
            }
        }
        // A malformed request is refused before any kernel is asked for.
        write(
            &f.request,
            &json!({"request_version": 1, "remove": [], "add": [], "radius_mm": 1}),
        );
        let v = reply(f.constrain(&never).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "{v}");
        // Closure links are not removable, on this plate as on any other; a
        // build without a kernel asks for it first, as it always has.
        let closure = f.constraint_row()["constraints"]
            .as_array()
            .expect("list")
            .iter()
            .find(|c| c["rule"]["kind"] == "coincident")
            .expect("a closure link")["constraint_id"]
            .clone();
        f.ask_constraints(&[closure], &[]);
        let v = reply(f.constrain(&never).output().expect("process"), OP, 2);
        let expected = if ferritecad_occt::is_available() {
            "input"
        } else {
            "unsupported"
        };
        assert_eq!(refused(&v), expected, "{v}");
        assert_eq!(entries(f.root.path()), names, "a refusal left something");
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    }

    /// OCCT without PlaneGCS: the unconstrained rounded plate still builds,
    /// and a constraint copy of it is refused, typed, publishing nothing.
    #[test]
    fn occt_without_solver_refuses_a_constrained_rounded_plate() {
        if !super::native() {
            return;
        }
        // N/A where a solver is linked. The mixed CI step runs this by exact
        // name and fails on any `skipped:`, so a solver that crept into that
        // build is caught there rather than asserted here.
        if ferritecad_eval::solver_available() {
            eprintln!("skipped: the mixed gate needs a build without PlaneGCS");
            return;
        }
        let f = filleted_by_cli(CCW, [X0 + W, Y0], 2.375, "rounded");
        let m = measure(&f.source, None);
        assert_eq!(m.faces, 7);
        let before = std::fs::read(&f.source).expect("bytes");
        let names = entries(f.root.path());
        f.ask_constraints(&[], &dimensioned(&f, [1.5, 2.25], 30.25, 9.5));
        let never = f.root.path().join("never.fcad");
        let v = reply(f.constrain(&never).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "unsupported", "{v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .expect("message")
                .contains("constraint"),
            "{v}"
        );
        assert_eq!(entries(f.root.path()), names, "no destination or scratch");
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    }

    /// Fully dimensioned in both windings and from another starting Line, on
    /// two different corners: translated and with both sides changed to
    /// fractional sizes. Stored and solved differ; the rounded face is the
    /// same named face of the same radius, at the corner its two Lines now
    /// meet. DOF 0, measured.
    #[test]
    fn native_constraints_solve_the_rounded_plate_and_keep_every_identity() {
        if !solving() {
            return;
        }
        let r = 3.0625;
        for (label, corners, corner) in [
            ("ccw", CCW, [X0 + W, Y0]),
            ("cw", CW_FROM_UPPER_RIGHT, [X0, Y0 + D]),
            ("third", CCW_FROM_THIRD, [X0 + W, Y0]),
        ] {
            let f = filleted_by_cli(corners, corner, r, &format!("rounded-{label}"));
            let (at, width, depth) = ([-9.25, -2.5], 41.125, 15.625);
            f.ask_constraints(&[], &dimensioned(&f, at, width, depth));
            let starts = f.stored_starts();
            let expected: Vec<_> = starts
                .iter()
                .map(|v| solved_vertex(&starts, *v, at, width, depth))
                .collect();
            assert_ne!(expected, starts, "the solve moves the plate");
            published(&f, corner, r, &expected, 0, &format!("constraints-{label}"));
        }
    }

    /// Replace length (one removal and one addition, atomically), then the
    /// exact pin UUID removed, then every user constraint: closure remains,
    /// the coordinate editor refuses, and nothing is dropped. The cache
    /// under one path misses on the plate and the Fillet and never returns
    /// the old part.
    #[test]
    fn native_replace_length_and_exact_removals_resize_the_rounded_plate() {
        if !solving() {
            return;
        }
        let r = 2.375;
        let corner = [X0 + W, Y0 + D];
        let f = filleted_by_cli(CCW, corner, r, "rounded");
        let (at, width, depth) = ([0.5, 1.25], 30.75, 10.5);
        f.ask_constraints(&[], &dimensioned(&f, at, width, depth));
        let starts = f.stored_starts();
        let map = |w: f64, d: f64| -> Vec<[f64; 2]> {
            starts
                .iter()
                .map(|v| solved_vertex(&starts, *v, at, w, d))
                .collect()
        };
        let first = published(&f, corner, r, &map(width, depth), 0, "constraints-first");

        // Replace the width: the stored distance UUID out, a new one in.
        let g = f.at_copy(&first);
        let listed = g.constraint_row()["constraints"]
            .as_array()
            .expect("list")
            .clone();
        let width_rule = listed
            .iter()
            .find(|c| c["rule"]["kind"] == "distance" && c["rule"]["distance"] == width)
            .expect("the width")["constraint_id"]
            .clone();
        g.ask_constraints(&[width_rule.clone()], &[length(&g.line(0), 22.25)]);
        let place = g.root.path().join("in-place.fcad");
        std::fs::copy(&first, &place).expect("copy");
        let cache = place.with_extension("fcad-cache");
        let (old, _) = measure_events(&place, Some(&cache));
        let replaced = published(&g, corner, r, &map(22.25, depth), 0, "constraints-replaced");
        let listed_now = inspect(&replaced)["sketches"][0]["constraint_edit"]["constraints"]
            .as_array()
            .expect("list")
            .clone();
        assert!(!listed_now.iter().any(|c| c["constraint_id"] == width_rule));
        assert_eq!(listed_now.len(), listed.len(), "one out, one in");
        // In place: the plate and the Fillet miss; neither is the old part.
        std::fs::copy(&replaced, &place).expect("replace in place");
        let (changed, events) = measure_events(&place, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Miss),
            "{events:?}"
        );
        assert!(
            (changed.volume - old.volume).abs() > 1.0,
            "not the old part"
        );
        let (hit, events) = measure_events(&place, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
        );
        changed.same_as(&hit);

        // The exact pin UUID removed: two degrees of freedom, measured.
        let k = g.at_copy(&replaced);
        let pin_rule = k.constraint_row()["constraints"]
            .as_array()
            .expect("list")
            .iter()
            .find(|c| c["rule"]["kind"] == "fixed")
            .expect("the pin")["constraint_id"]
            .clone();
        k.ask_constraints(&[pin_rule], &[]);
        let unpinned = k.root.path().join("unpinned.fcad");
        let v = reply(k.constrain(&unpinned).output().expect("process"), OP, 0);
        assert_eq!(v["result"]["solve"]["degrees_of_freedom"], 2);
        let (_, dof) = solved(&unpinned);
        assert_eq!(dof, 2);
        let m = measure(&unpinned, None);
        assert_eq!(m.faces, 7);
        assert!((m.volume - exact([0., 0., 22.25, depth], r, H)).abs() < 1e-9 * m.volume);

        // Every user constraint removed: the four closure links remain, the
        // part is the stored plate again, and the coordinate editor refuses
        // a constrained Sketch as it always has.
        let u = k.at_copy(&unpinned);
        let user: Vec<Value> = u.constraint_row()["constraints"]
            .as_array()
            .expect("list")
            .iter()
            .filter(|c| c["rule"]["kind"] != "coincident")
            .map(|c| c["constraint_id"].clone())
            .collect();
        assert_eq!(user.len(), 6, "H/V on four Lines, the width and the depth");
        u.ask_constraints(&user, &[]);
        let bare = u.root.path().join("closure-only.fcad");
        let v = reply(u.constrain(&bare).output().expect("process"), OP, 0);
        assert_eq!(v["result"]["solve"]["degrees_of_freedom"], 8);
        let catalog = inspect(&bare);
        let left = catalog["sketches"][0]["constraint_edit"]["constraints"]
            .as_array()
            .expect("list")
            .clone();
        assert_eq!(left.len(), 4);
        assert!(left.iter().all(|c| c["rule"]["kind"] == "coincident"));
        assert_eq!(catalog["sketches"][0]["editable"], false);
        assert_eq!(catalog["sketches"][0]["constraint_edit"]["available"], true);
        let (starts_now, _) = solved(&bare);
        assert_eq!(
            starts_now, starts,
            "closure alone solves to the stored plate"
        );
        let m = measure(&bare, None);
        assert!((m.volume - exact([X0, Y0, W, D], r, H)).abs() < 1e-9 * m.volume);
    }

    /// Radius and height after constraints, on the same UUIDs. The radius
    /// bound is the solved plate's: a radius the stored rectangle would
    /// refuse publishes when the solved one fits it, and one the stored
    /// rectangle would accept is refused when the solved one does not. The
    /// constraints are kept byte for byte.
    #[test]
    fn native_radius_and_height_after_constraints_answer_to_the_solved_plate() {
        if !solving() {
            return;
        }
        let r = 2.375;
        let corner = [X0 + W, Y0];
        let f = filleted_by_cli(CCW, corner, r, "rounded");
        let starts = f.stored_starts();
        // Wider and deeper than stored: half the shorter side is 7.75 mm,
        // where the stored rectangle allows 6.125 mm.
        let (at, width, depth) = ([-6.0, 2.0], 41.25, 15.5);
        f.ask_constraints(&[], &dimensioned(&f, at, width, depth));
        let wide = f.root.path().join("wide.fcad");
        reply(f.constrain(&wide).output().expect("process"), OP, 0);
        let g = f.at_copy(&wide);
        let sketch_bytes = |p: &Path| {
            let t = tables(p);
            let (columns, rows) = &t["objects"];
            let at = columns
                .iter()
                .position(|c| c == "payload")
                .expect("payload");
            let id: ferritecad_types::ObjectId = g.plate_sketch_id().parse().expect("UUID");
            let key = rusqlite::types::Value::Blob(id.to_bytes().to_vec());
            rows.iter()
                .find(|r| r.contains(&key))
                .expect("the Sketch row")[at]
                .clone()
        };
        let constrained_payload = sketch_bytes(&wide);
        g.ask_radius(7.0);
        let rounder = g.root.path().join("rounder.fcad");
        reply(
            g.edit(&rounder).output().expect("process"),
            "edit-fillet-radius",
            0,
        );
        assert_eq!(
            sketch_bytes(&rounder),
            constrained_payload,
            "constraints kept"
        );
        let m = measure(&rounder, None);
        assert_eq!(m.fillet, FaceSurface::Cylinder { radius: 7.0 });
        let rect = rect_of(
            &starts
                .iter()
                .map(|v| solved_vertex(&starts, *v, at, width, depth))
                .collect::<Vec<_>>(),
        );
        assert!((m.volume - exact(rect, 7.0, H)).abs() < 1e-9 * m.volume);

        // Narrower than stored: 4.25 mm allowed, where the stored rectangle
        // would allow 6.125 mm. 5 mm is refused by the rebuild, atomically.
        let (at, width, depth) = ([1.0, 1.0], 30.0, 8.5);
        f.ask_constraints(&[], &dimensioned(&f, at, width, depth));
        let narrow = f.root.path().join("narrow.fcad");
        reply(f.constrain(&narrow).output().expect("process"), OP, 0);
        let k = f.at_copy(&narrow);
        k.ask_radius(5.0);
        let names = entries(k.root.path());
        let never = k.root.path().join("never.fcad");
        let v = reply(
            k.edit(&never).output().expect("process"),
            "edit-fillet-radius",
            2,
        );
        assert_eq!(refused(&v), "input", "{v}");
        let message = v["error"]["message"].as_str().expect("message");
        assert!(
            message.contains("solve") && message.contains("too short"),
            "{message}"
        );
        assert_eq!(entries(k.root.path()), names);

        // The height, on the constrained plate: the Sketch row is untouched.
        let taller = k.root.path().join("taller.fcad");
        let mut c = cli();
        c.arg("edit-extrude")
            .arg(&narrow)
            .arg("--feature")
            .arg(
                k.catalog["features"][0]["feature_id"]
                    .as_str()
                    .expect("base"),
            )
            .arg("--distance-mm")
            .arg("9.75")
            .arg("--expect-version")
            .arg(k.version())
            .arg("-o")
            .arg(&taller)
            .arg("--json");
        reply(c.output().expect("process"), "edit-extrude", 0);
        assert_eq!(sketch_bytes(&taller), sketch_bytes(&narrow));
        let m = measure(&taller, None);
        let rect = rect_of(
            &starts
                .iter()
                .map(|v| solved_vertex(&starts, *v, at, width, depth))
                .collect::<Vec<_>>(),
        );
        assert!((m.volume - exact(rect, r, 9.75)).abs() < 1e-9 * m.volume);
        assert_eq!(stored_refs(&taller), stored_refs(&f.source));
    }

    /// Refusals, each atomic: the solved rectangle too narrow for the saved
    /// radius; a solve that turns a Line off its side; a real solver
    /// conflict naming its UUIDs; a stale version; an occupied destination.
    #[test]
    fn native_constraint_refusals_under_a_fillet_are_atomic() {
        if !solving() {
            return;
        }
        let r = 2.375;
        let f = filleted_by_cli(CCW, [X0 + W, Y0], r, "rounded");
        let before = std::fs::read(&f.source).expect("bytes");
        let names = entries(f.root.path());
        let never = f.root.path().join("never.fcad");
        let refuse = |add: &[Value], kind: &str, words: &[&str]| {
            f.ask_constraints(&[], add);
            let v = reply(f.constrain(&never).output().expect("process"), OP, 2);
            assert_eq!(refused(&v), kind, "{v}");
            let text = v.to_string();
            for w in words {
                assert!(text.contains(w), "{w}: {v}");
            }
            assert_eq!(entries(f.root.path()), names, "a refusal left something");
            assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
            v
        };
        // 4.5 mm deep leaves 2.25 mm for a radius of 2.375 mm.
        refuse(
            &dimensioned(&f, [0., 0.], 30., 4.5),
            "input",
            &["too short", "4.5"],
        );
        // The first Line's start pinned 5 mm beyond its own end, the sides
        // held H/V: the solved plate is a rectangle 5 mm wide whose first
        // Line runs the other way, which is not the corner that was rounded.
        let starts = f.stored_starts();
        let across = horizontal(&starts);
        let mut add: Vec<Value> = (0..4)
            .map(|i| {
                rule(
                    &f.line(i),
                    if across[i] { "horizontal" } else { "vertical" },
                )
            })
            .collect();
        add.push(pin(&f.line(0), starts[1][0] + 5.0, starts[0][1]));
        let v = refuse(&add, "input", &["side"]);
        eprintln!("FCAD_28E_SIDE_REFUSAL {v}");
        // One length and nothing to hold the sides: the solve shortens the
        // first Line and slants its neighbours, so the plate is no longer an
        // axis-aligned rectangle.
        let v = refuse(&[length(&f.line(0), 30.)], "unsupported", &["rectangle"]);
        eprintln!("FCAD_28E_CLASS_REFUSAL {v}");
        // A real conflict: both horizontal sides dimensioned, differently,
        // on a plate whose sides are held H/V.
        let mut conflict = dimensioned(&f, [0., 0.], 30., 10.);
        let opposite = (1..4)
            .find(|i| across[*i])
            .expect("the other horizontal Line");
        conflict.push(length(&f.line(opposite), 20.));
        let v = refuse(&conflict, "constraint", &[]);
        eprintln!("FCAD_28E_CONFLICT {v}");
        let named = v["error"]["constraint_conflict"]["constraints"]
            .as_array()
            .expect("the conflicting constraints")
            .clone();
        assert!(!named.is_empty(), "{v}");
        assert!(named.iter().all(|c| c["constraint_id"].is_string()), "{v}");
        // Stale version and an occupied destination.
        f.ask_constraints(&[], &[length(&f.line(1), 11.5)]);
        let mut c = cli();
        c.arg(OP)
            .arg(&f.source)
            .arg("--sketch")
            .arg(f.plate_sketch_id())
            .arg("--expect-version")
            .arg("0".repeat(64))
            .arg("--request")
            .arg(&f.request)
            .arg("-o")
            .arg(&never)
            .arg("--json");
        assert_eq!(
            refused(&reply(c.output().expect("process"), OP, 2)),
            "input"
        );
        let taken = f.root.path().join("taken.fcad");
        std::fs::write(&taken, b"another process owns this").expect("occupied");
        let v = reply(f.constrain(&taken).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "{v}");
        assert_eq!(
            std::fs::read(&taken).expect("kept"),
            b"another process owns this"
        );
        std::fs::remove_file(&taken).expect("tidy");
        assert_eq!(entries(f.root.path()), names);
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    }

    /// The radius bound on the solved plate, on both sides of it: a solved
    /// depth of exactly 2 r publishes, 1e-7 mm less is refused with the
    /// numbers, publishing nothing.
    #[test]
    fn native_the_radius_bound_is_exact_on_the_solved_plate() {
        if !solving() {
            return;
        }
        let r = 2.375;
        let corner = [X0 + W, Y0];
        let f = filleted_by_cli(CCW, corner, r, "rounded");
        let starts = f.stored_starts();
        let (at, width) = ([2.5, -1.5], 28.5);
        f.ask_constraints(&[], &dimensioned(&f, at, width, 2. * r));
        let expected: Vec<_> = starts
            .iter()
            .map(|v| solved_vertex(&starts, *v, at, width, 2. * r))
            .collect();
        published(&f, corner, r, &expected, 0, "constraints-bound");
        f.ask_constraints(&[], &dimensioned(&f, at, width, 2. * r - 1e-7));
        let names = entries(f.root.path());
        let never = f.root.path().join("never.fcad");
        let v = reply(f.constrain(&never).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "{v}");
        assert!(v.to_string().contains("too short"), "{v}");
        assert_eq!(entries(f.root.path()), names);
    }
}
