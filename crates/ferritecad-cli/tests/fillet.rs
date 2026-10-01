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
fn check_mesh_in(m: &Mesh, rect: [f64; 4], corner: [f64; 2], r: f64, height: f64) {
    check_mesh_rounded(m, rect, &[(corner, r)], height);
}

/// [`check_mesh_in`] for every rounded corner at once (§28G: two).
fn check_mesh_rounded(
    m: &Mesh,
    [x0, y0, w, d]: [f64; 4],
    rounded: &[([f64; 2], f64)],
    height: f64,
) {
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
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9;
    for c in [[x0, y0], [x0 + w, y0], [x0 + w, y0 + d], [x0, y0 + d]] {
        let is_rounded = rounded.iter().any(|(corner, _)| near(c, *corner));
        for z in [0.0, height] {
            assert_eq!(
                has(c[0], c[1], z),
                !is_rounded,
                "corner {c:?} at z={z}: rounded are {rounded:?}"
            );
        }
    }
    // Every vertex inside a corner's r x r square lies on its arc, about the
    // centre r inward of the corner, from z = 0 to the top.
    for &(corner, r) in rounded {
        let centre = inward_in([x0, y0], corner, r);
        let inside = |v: &[f64; 3]| {
            (v[0] - corner[0]).abs() < r - ROUNDING_MM && (v[1] - corner[1]).abs() < r - ROUNDING_MM
        };
        let on_arc: Vec<_> = vertices.iter().filter(|v| inside(v)).collect();
        assert!(
            on_arc.len() >= 4,
            "the mesh has no fillet wall at {corner:?}"
        );
        for v in &on_arc {
            let d = (v[0] - centre[0]).hypot(v[1] - centre[1]);
            assert!(
                (d - r).abs() < 10. * ROUNDING_MM,
                "{v:?} is {d} from the axis"
            );
        }
        assert!(on_arc.iter().any(|v| v[2].abs() < ROUNDING_MM));
        assert!(on_arc.iter().any(|v| (v[2] - height).abs() < ROUNDING_MM));
    }
    // Chords of a convex arc lie inside it, so a mesh removes a little more
    // than the exact fillet, never less, and not more than its sagitta allows.
    let squares: f64 = rounded.iter().map(|(_, r)| r * r).sum();
    let exact = w * d * height - (1. - PI / 4.) * squares * height;
    let slack: f64 = rounded
        .iter()
        .map(|(_, r)| PI / 2. * r * LINEAR_MM * height)
        .sum();
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
    only_a_fillet_was_added(
        source,
        copy,
        body,
        7,
        &[
            "feature.fillet.v1",
            "topology.origin-face.v1",
            "feature.predecessor.v1",
        ],
    );
}

/// [`only_the_fillet_was_added`] with the number of new names and the
/// capabilities a copy may add as parameters (§28G: eight, and only
/// `feature.fillet.sequential.v1`).
fn only_a_fillet_was_added(
    source: &Path,
    copy: &Path,
    body: ferritecad_types::ObjectId,
    names: usize,
    new_capabilities: &[&str],
) {
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
                    "topology_refs" => assert_eq!(added, names, "the new names"),
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
            old.contains(name) || new_capabilities.contains(&name.as_str()),
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
    // §28G: the kept copy is a rounded plate, offered for a second Fillet.
    let kept = &inspect(&lost)["bodies"][0]["fillet_edge"];
    assert_eq!(kept["available"], true, "{kept}");
    assert_eq!(kept["target"]["fillets"].as_array().map(Vec::len), Some(1));
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
    // §28G: a second Fillet is offered, on the three other corners.
    let second = &catalog["bodies"][0]["fillet_edge"];
    assert_eq!(second["available"], true, "{second}");
    assert_eq!(second["target"]["previous_feature_id"], fillet_id.as_str());
    assert_eq!(
        second["target"]["candidates"].as_array().map(Vec::len),
        Some(3)
    );
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
    // The corner already rounded is refused as input (§28G).
    let out = f.root.path().join("again.fcad");
    f.ask_reversed([X0, Y0], 2.0);
    let v = reply(
        f.fillet_from(&copy, body, version, &out)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert_eq!(refused(&v), "input", "the same corner twice: {v}");
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
        pub(super) fn edit_from(
            &self,
            source: &Path,
            feature: &str,
            version: &str,
            output: &Path,
        ) -> Command {
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
    pub(super) fn only_the_radius_changed(source: &Path, copy: &Path, fillet: &str) -> usize {
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
    pub(super) fn only_the_height_changed(source: &Path, copy: &Path, base: &str) -> usize {
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
                // §28I, additive: no second Fillet over this plate.
                "second_fillet": null,
            })
        );
        // Every other editor still refuses the filleted plate by name.
        let names_it = |reason: &Value| {
            let text = reason.as_str().expect("a reason");
            assert!(text.contains(f.fillet_id()), "{text}");
        };
        // §28G: a second Fillet is offered on the rounded plate.
        assert_eq!(
            f.catalog["bodies"][0]["fillet_edge"]["target"]["previous_feature_id"],
            f.fillet_id()
        );
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

    pub(super) const OP: &str = "edit-sketch-copy";

    /// The saved plate as `[x0, y0, width, depth]`.
    const SAVED: [f64; 4] = [X0, Y0, W, D];

    /// A saved vertex of the plate moved to the same corner of `rect`.
    pub(super) fn mapped(rect: [f64; 4], v: [f64; 2]) -> [f64; 2] {
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
        pub(super) fn sketch_row(&self) -> &Value {
            &self.catalog["sketches"][0]
        }
        pub(super) fn sketch_id(&self) -> &str {
            self.sketch_row()["sketch_id"]
                .as_str()
                .expect("Sketch UUID")
        }
        /// Every saved vertex, in saved order, sent to `at`.
        pub(super) fn ask_starts(&self, at: &[[f64; 2]]) {
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
        pub(super) fn ask_rect(&self, rect: [f64; 4]) {
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
        pub(super) fn redraw(&self, source: &Path, version: &str, output: &Path) -> Command {
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
    pub(super) fn only_this_row_changed(source: &Path, copy: &Path, row: &str) -> usize {
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

    pub(super) fn exact([_, _, w, d]: [f64; 4], r: f64, h: f64) -> f64 {
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
    pub(crate) fn solving() -> bool {
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
        pub(crate) fn constraint_row(&self) -> &Value {
            &self.catalog["sketches"][0]["constraint_edit"]
        }
        pub(crate) fn plate_sketch_id(&self) -> &str {
            self.catalog["sketches"][0]["sketch_id"]
                .as_str()
                .expect("Sketch UUID")
        }
        /// The stored Line `i`, in stored order.
        pub(crate) fn line(&self, i: usize) -> Value {
            self.constraint_row()["curves"][i]["curve_id"].clone()
        }
        pub(crate) fn stored_starts(&self) -> Vec<[f64; 2]> {
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
        pub(crate) fn ask_constraints(&self, remove: &[Value], add: &[Value]) {
            write(
                &self.request,
                &json!({"request_version": 1, "remove": remove, "add": add}),
            );
        }
        pub(crate) fn constrain(&self, output: &Path) -> Command {
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
        pub(crate) fn at_copy(&self, path: &Path) -> Fixture {
            Fixture {
                root: tempfile::tempdir().expect("directory"),
                source: path.to_path_buf(),
                request: self.request.clone(),
                catalog: inspect(path),
            }
        }
    }

    pub(crate) fn rule(line: &Value, rule: &str) -> Value {
        json!({"curve_id": line, "rule": rule})
    }
    pub(crate) fn length(line: &Value, mm: f64) -> Value {
        json!({"curve_id": line, "rule": "distance", "distance_mm": mm})
    }
    pub(crate) fn pin(line: &Value, x: f64, y: f64) -> Value {
        json!({"curve_id": line, "rule": "fixed", "at": "start", "x_mm": x, "y_mm": y})
    }

    /// Which stored Lines run along X, by index.
    pub(crate) fn horizontal(starts: &[[f64; 2]]) -> Vec<bool> {
        (0..starts.len())
            .map(|i| starts[i][1] == starts[(i + 1) % starts.len()][1])
            .collect()
    }

    /// Where a stored vertex lands when the stored rectangle is solved to one
    /// whose first Line starts at `at`, `width` along X and `depth` along Y,
    /// every side kept.
    pub(crate) fn solved_vertex(
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
    pub(crate) fn rect_of(vertices: &[[f64; 2]]) -> [f64; 4] {
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
    pub(crate) fn solved(path: &Path) -> (Vec<[f64; 2]>, usize) {
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
    pub(crate) fn only_this_sketch_changed(source: &Path, copy: &Path, sketch: &str) -> usize {
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
    pub(crate) fn axis_of(rect: [f64; 4], corner: [f64; 2], r: f64) -> [f64; 2] {
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
    pub(crate) fn dimensioned(f: &Fixture, at: [f64; 2], width: f64, depth: f64) -> Vec<Value> {
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
        g.ask_constraints(
            std::slice::from_ref(&width_rule),
            &[length(&g.line(0), 22.25)],
        );
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
        refuse(&add, "input", &["side"]);
        // One length and nothing to hold the sides: the solve shortens the
        // first Line and slants its neighbours, so the plate is no longer an
        // axis-aligned rectangle.
        refuse(&[length(&f.line(0), 30.)], "unsupported", &["rectangle"]);
        // A real conflict: both horizontal sides dimensioned, differently,
        // on a plate whose sides are held H/V.
        let mut conflict = dimensioned(&f, [0., 0.], 30., 10.);
        let opposite = (1..4)
            .find(|i| across[*i])
            .expect("the other horizontal Line");
        conflict.push(length(&f.line(opposite), 20.));
        let v = refuse(&conflict, "constraint", &[]);
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

    /// §28F: the natural order. The rectangle is dimensioned first, by the
    /// shipped constraint command, and then rounded by `fillet-edge-copy`.
    pub(crate) mod first {
        use super::*;

        const ROUND: &str = "fillet-edge-copy";
        const CONSTRAIN: &str = "edit-sketch-constraints-copy";

        impl Fixture {
            /// The candidate whose stored corner is `corner`.
            pub(crate) fn stored_at(&self, corner: [f64; 2]) -> &Value {
                self.candidates()
                    .iter()
                    .find(|c| c["stored_corner_mm"] == json!(corner))
                    .unwrap_or_else(|| panic!("no candidate stored at {corner:?}"))
            }
            /// The request for that candidate, its pair written the other way.
            pub(crate) fn ask_stored(&self, corner: [f64; 2], r: f64) {
                let edge = &self.stored_at(corner)["edge"];
                self.ask_edge(
                    &json!({"feature_id": edge["feature_id"],
                            "joint": [edge["joint"][1], edge["joint"][0]]}),
                    r,
                );
            }
        }

        /// A plain plate, and the same plate dimensioned by the shipped
        /// command: translated to `at`, `width` along X and `depth` along Y.
        /// Returns both fixtures and the Line starts the solve must reach.
        pub(crate) fn dimensioned_plate(
            corners: [[f64; 2]; 4],
            at: [f64; 2],
            width: f64,
            depth: f64,
            name: &str,
        ) -> (Fixture, Fixture, Vec<[f64; 2]>) {
            let plain = Fixture::drawn(Plate {
                cut_side: true,
                ..Plate::new(corners)
            });
            assert_eq!(plain.discovery()["target"]["profile_constrained"], false);
            plain.ask_constraints(&[], &dimensioned(&plain, at, width, depth));
            let copy = plain.root.path().join(format!("{name}.fcad"));
            reply(
                plain.constrain(&copy).output().expect("process"),
                CONSTRAIN,
                0,
            );
            let starts = plain.stored_starts();
            let expected: Vec<_> = starts
                .iter()
                .map(|v| solved_vertex(&starts, *v, at, width, depth))
                .collect();
            let g = plain.at_copy(&copy);
            (plain, g, expected)
        }

        /// The whole SQL row of the plate's Sketch.
        pub(crate) fn sketch_row(path: &Path, sketch: &str) -> Vec<rusqlite::types::Value> {
            let id: ferritecad_types::ObjectId = sketch.parse().expect("UUID");
            let key = rusqlite::types::Value::Blob(id.to_bytes().to_vec());
            tables(path)["objects"]
                .1
                .iter()
                .find(|r| r.contains(&key))
                .expect("the Sketch row")
                .clone()
        }

        /// One Fillet published over the dimensioned plate `g`, at the
        /// candidate stored at `stored`, measured against the solved plate
        /// `expected`: identities, the exact §28A allowlist, the Sketch row
        /// byte for byte, the solve and its DOF, the B-Rep cold and through a
        /// real cache Miss and Hit, the mesh and the FBX.
        fn rounded(
            g: &Fixture,
            stored: [f64; 2],
            r: f64,
            expected: &[[f64; 2]],
            dof: usize,
            name: &str,
        ) -> PathBuf {
            let before = std::fs::read(&g.source).expect("source bytes");
            let refs = stored_refs(&g.source);
            let sketch = sketch_row(&g.source, g.plate_sketch_id());
            let starts = g.stored_starts();
            let candidate = g.stored_at(stored).clone();
            let copy = g.root.path().join(format!("{name}.fcad"));
            g.ask_stored(stored, r);
            let v = reply(g.fillet(&copy).output().expect("process"), ROUND, 0);
            let result = &v["result"];
            assert_eq!(result["body_id"], g.body_id());
            assert_eq!(result["edge"], candidate["edge"], "the canonical meaning");
            assert_eq!(result["profile_constrained"], true, "{v}");
            assert_eq!(result["stored_corner_mm"], json!(stored));
            assert_eq!(result["radius_mm"], r);
            let rect = rect_of(expected);
            let moved = expected[starts.iter().position(|v| *v == stored).expect("corner")];
            for k in 0..2 {
                let got = result["corner_mm"][k].as_f64().expect("corner");
                assert!((got - moved[k]).abs() < 1e-9, "{v} is not at {moved:?}");
            }
            let mut sides: Vec<f64> = (0..2)
                .map(|k| result["adjacent_lengths_mm"][k].as_f64().expect("side"))
                .collect();
            sides.sort_by(f64::total_cmp);
            let (w, d) = (rect[2], rect[3]);
            assert!((sides[0] - w.min(d)).abs() < 1e-9 && (sides[1] - w.max(d)).abs() < 1e-9);
            assert_eq!(std::fs::read(&g.source).expect("bytes"), before);

            // SQL: exactly §28A's allowlist; the Sketch row, constraints and
            // their UUIDs included, is the same bytes.
            let body: ferritecad_types::ObjectId = g.body_id().parse().expect("UUID");
            only_the_fillet_was_added(&g.source, &copy, body);
            assert_eq!(sketch_row(&copy, g.plate_sketch_id()), sketch);
            let now = stored_refs(&copy);
            assert!(refs.iter().all(|r| now.contains(r)), "a name was lost");
            assert_eq!(now.len(), refs.len() + 7);
            let after = inspect(&copy);
            let fillet = &after["fillets"][0];
            assert_eq!(fillet["feature_id"], result["feature_id"]);
            assert_eq!(fillet["profile_constrained"], true);
            assert_eq!(fillet["radius_edit"]["max_radius_mm"], Value::Null);
            assert_eq!(after["sketches"][0]["editable"], false, "coordinates");
            assert_eq!(after["sketches"][0]["constraint_edit"]["available"], true);

            // The solve, read from the solver: the expected rectangle, no
            // snapping, and its DOF.
            let (solved_starts, measured) = solved(&copy);
            assert_eq!(measured, dof, "DOF of the cold rebuild");
            for (s, e) in solved_starts.iter().zip(expected) {
                assert!(
                    (s[0] - e[0]).abs() < 1e-9 && (s[1] - e[1]).abs() < 1e-9,
                    "{solved_starts:?} is not {expected:?}"
                );
            }

            let checked = cli()
                .arg("validate")
                .arg(&copy)
                .arg("--json")
                .output()
                .expect("validate");
            assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
            let n = now.len();
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
            assert_eq!(
                cold.roles["edge fillet face of feature.fillet"],
                [FaceSurface::Cylinder { radius: r }]
            );
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

        /// A plate whose Sketch carries the managed family, written by the
        /// shipped preparation and writer with no kernel: H/V on every Line
        /// of the counter-clockwise plate.
        fn constrained_without_kernel() -> Fixture {
            let plain = Fixture::drawn(Plate {
                cut_side: true,
                ..Plate::new(CCW)
            });
            let mut d = Document::open(&plain.source).expect("writable");
            let sketch: ferritecad_types::ObjectId = plain.plate_sketch_id().parse().expect("UUID");
            let across = horizontal(&plain.stored_starts());
            let add = (0..4)
                .map(|i| {
                    ferritecad_document::AddSketchConstraint::Line(
                        ferritecad_document::AddLineConstraint::Line {
                            curve: plain.line(i).as_str().expect("UUID").parse().expect("UUID"),
                            kind: if across[i] {
                                ferritecad_document::LineConstraintKind::Horizontal
                            } else {
                                ferritecad_document::LineConstraintKind::Vertical
                            },
                        },
                    )
                })
                .collect();
            let edits = ferritecad_document::SketchConstraintEdits {
                remove: Vec::new(),
                add,
            };
            let prepared = ferritecad_document::prepare_sketch_constraints(&d, sketch, &edits)
                .expect("prepared");
            d.write_sketch_constraints(&prepared).expect("written");
            d.close().expect("close");
            Fixture {
                catalog: inspect(&plain.source),
                ..plain
            }
        }

        /// Discovery on a dimensioned plate and the protocol, with no kernel
        /// or solver needed: each candidate is a joint of two Line UUIDs
        /// with its stored numbers labelled as stored, the part's own numbers
        /// are `null` because inspect solves nothing, the unconstrained
        /// plate's fields keep their numbers, and a build without the kernel
        /// or the solver refuses a well-formed request, writing nothing.
        #[test]
        fn rounding_discovery_and_protocol_without_native() {
            let plain = Fixture::drawn(Plate {
                cut_side: true,
                ..Plate::new(CCW)
            });
            let t = &plain.discovery()["target"];
            assert_eq!(t["profile_constrained"], false);
            for c in t["candidates"].as_array().expect("candidates") {
                assert_eq!(c["corner_mm"], c["stored_corner_mm"]);
                assert_eq!(c["adjacent_lengths_mm"], c["stored_adjacent_lengths_mm"]);
                assert!(c["max_radius_mm"].is_f64(), "{c}");
                assert!(
                    c["label"]
                        .as_str()
                        .expect("label")
                        .starts_with("vertical edge at")
                );
            }

            let f = constrained_without_kernel();
            let d = f.discovery();
            assert_eq!(d["available"], true, "{d}");
            assert!(d["refusal"].is_null() && d["document_refusal"].is_null());
            let t = &d["target"];
            assert_eq!(t["profile_constrained"], true);
            assert_eq!(t["min_radius_mm"], 0.01);
            assert_eq!(t["max_radius_fraction"], 0.5);
            assert_eq!(t["base_feature_id"], f.catalog["features"][0]["feature_id"]);
            let candidates = t["candidates"].as_array().expect("candidates");
            assert_eq!(candidates.len(), 4);
            let stored: Vec<Value> = candidates
                .iter()
                .map(|c| c["stored_corner_mm"].clone())
                .collect();
            assert_eq!(stored, CCW.iter().map(|c| json!(c)).collect::<Vec<_>>());
            for c in candidates {
                assert!(c["corner_mm"].is_null(), "{c}");
                assert!(c["adjacent_lengths_mm"].is_null(), "{c}");
                assert!(c["max_radius_mm"].is_null(), "{c}");
                assert!(c["stored_adjacent_lengths_mm"].is_array());
                let label = c["label"].as_str().expect("label");
                assert!(
                    label.contains("stored") && label.contains("solved"),
                    "{label}"
                );
                let joint = &c["edge"]["joint"];
                assert!(joint[0].as_str() < joint[1].as_str(), "canonical");
            }
            assert_eq!(f.catalog["fillets"], json!([]));
            assert_eq!(f.catalog["sketches"][0]["editable"], false);

            // A radius the stored rectangle could never carry is still a
            // well-formed request: its fate is the solved plate's.
            let before = std::fs::read(&f.source).expect("bytes");
            let never = f.root.path().join("never.fcad");
            f.ask_stored([X0 + W, Y0], 100.);
            let names = entries(f.root.path());
            let occt = ferritecad_occt::is_available();
            let solver = occt && ferritecad_eval::solver_available();
            let v = reply(f.fillet(&never).output().expect("process"), ROUND, 2);
            if !occt {
                assert_eq!(refused(&v), "unsupported", "{v}");
                assert!(v.to_string().contains("Open CASCADE"), "{v}");
            } else if !solver {
                assert_eq!(refused(&v), "unsupported", "{v}");
                assert!(v.to_string().contains("constraint"), "{v}");
            } else {
                // H/V alone solve to the stored plate, 12.25 mm deep.
                assert_eq!(refused(&v), "input", "{v}");
                assert!(v.to_string().contains("too short"), "{v}");
            }
            // A malformed request is refused before any kernel is asked for.
            write(
                &f.request,
                &json!({"request_version": 1, "edge": {}, "radius_mm": 1}),
            );
            let v = reply(f.fillet(&never).output().expect("process"), ROUND, 2);
            assert_eq!(refused(&v), "input", "{v}");
            // A radius below the value policy: input once a kernel is there.
            f.ask_stored([X0 + W, Y0], 0.005);
            let v = reply(f.fillet(&never).output().expect("process"), ROUND, 2);
            assert_eq!(
                refused(&v),
                if occt { "input" } else { "unsupported" },
                "{v}"
            );
            assert_eq!(entries(f.root.path()), names, "a refusal left something");
            assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
        }

        /// OCCT without PlaneGCS: the unconstrained route still rounds a
        /// plate, and rounding a dimensioned plate is refused, typed, before
        /// anything is published.
        #[test]
        fn occt_without_solver_refuses_rounding_a_dimensioned_plate() {
            if !super::super::native() {
                return;
            }
            // N/A where a solver is linked; the mixed CI step runs this by
            // exact name and fails on `skipped:`.
            if ferritecad_eval::solver_available() {
                eprintln!("skipped: the mixed gate needs a build without PlaneGCS");
                return;
            }
            let f = filleted_by_cli(CCW, [X0 + W, Y0], 2.375, "rounded");
            assert_eq!(measure(&f.source, None).faces, 7);
            let g = constrained_without_kernel();
            let before = std::fs::read(&g.source).expect("bytes");
            g.ask_stored([X0 + W, Y0], 2.375);
            let names = entries(g.root.path());
            let never = g.root.path().join("never.fcad");
            let v = reply(g.fillet(&never).output().expect("process"), ROUND, 2);
            assert_eq!(refused(&v), "unsupported", "{v}");
            assert!(v.to_string().contains("constraint"), "{v}");
            assert_eq!(entries(g.root.path()), names, "no destination or scratch");
            assert_eq!(std::fs::read(&g.source).expect("bytes"), before);
        }

        /// Dimensioned first and then rounded: both windings and another
        /// starting Line, three different corners, translated and resized to
        /// fractional sizes. The Fillet names the joint of two stored Line
        /// UUIDs and rounds that corner of the solved plate. DOF 0.
        #[test]
        fn native_a_dimensioned_plate_is_rounded_at_its_solved_corner() {
            if !solving() {
                return;
            }
            let r = 3.0625;
            for (label, corners, corner) in [
                ("ccw", CCW, [X0 + W, Y0]),
                ("cw", CW_FROM_UPPER_RIGHT, [X0, Y0 + D]),
                ("third", CCW_FROM_THIRD, [X0 + W, Y0 + D]),
            ] {
                let (at, width, depth) = ([-9.25, -2.5], 41.125, 15.625);
                let (_plain, g, expected) =
                    dimensioned_plate(corners, at, width, depth, &format!("dimensioned-{label}"));
                assert_ne!(expected, g.stored_starts(), "the solve moves the plate");
                assert_eq!(g.discovery()["target"]["profile_constrained"], true);
                rounded(&g, corner, r, &expected, 0, &format!("first-{label}"));
            }
        }

        /// The identifiers the two routes each minted, matched by meaning:
        /// the Fillet, its seven names by role and selection, and the Sketch's
        /// constraints by their stored position.
        fn minted(a: &Path, b: &Path) -> Vec<(Vec<u8>, Vec<u8>)> {
            let read = |p: &Path| {
                let d = Document::open_read_only(p).expect("open");
                let objects = d.objects().expect("objects");
                let refs = d.topology_refs().expect("refs");
                d.close().expect("close");
                let fillet = objects
                    .iter()
                    .find(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
                    .expect("a Fillet")
                    .id;
                let constraints: Vec<_> = objects
                    .iter()
                    .find_map(|o| match &o.payload {
                        ObjectPayload::Sketch(s) => Some(s.constraints.clone()),
                        _ => None,
                    })
                    .expect("the Sketch");
                let own: Vec<_> = refs.into_iter().filter(|r| r.owner == fillet).collect();
                (fillet, own, constraints)
            };
            let (fa, ra, ca) = read(a);
            let (fb, rb, cb) = read(b);
            assert_eq!(ra.len(), 7);
            assert_eq!(ca.len(), cb.len(), "the same number of constraints");
            let mut pairs = vec![(fb.to_bytes().to_vec(), fa.to_bytes().to_vec())];
            for theirs in &rb {
                let mine = ra
                    .iter()
                    .find(|m| {
                        m.output_role == theirs.output_role && m.selection == theirs.selection
                    })
                    .expect("the same meaning");
                pairs.push((theirs.id.to_bytes().to_vec(), mine.id.to_bytes().to_vec()));
            }
            for (mine, theirs) in ca.iter().zip(&cb) {
                pairs.push((theirs.id.to_bytes().to_vec(), mine.id.to_bytes().to_vec()));
            }
            pairs
        }

        /// Every SQL cell of `b`, with `pairs` mapped onto `a`'s identifiers,
        /// is a cell of `a`, table by table and row for row, whatever order
        /// the two routes inserted them in. The copies' own timestamps and the
        /// hashes over minted identifiers (compared through their payloads)
        /// are the only exceptions.
        fn same_sql(a: &Path, b: &Path, pairs: &[(Vec<u8>, Vec<u8>)]) {
            use rusqlite::types::Value as Cell;
            let map = |value: &Cell| -> Cell {
                match value {
                    Cell::Blob(bytes) => {
                        let mut out = bytes.clone();
                        for (from, to) in pairs {
                            let mut i = 0;
                            while i + 16 <= out.len() {
                                if out[i..i + 16] == from[..] {
                                    out[i..i + 16].copy_from_slice(to);
                                    i += 16;
                                } else {
                                    i += 1;
                                }
                            }
                        }
                        Cell::Blob(out)
                    }
                    other => other.clone(),
                }
            };
            let (left, right) = (tables(a), tables(b));
            assert_eq!(
                left.keys().collect::<Vec<_>>(),
                right.keys().collect::<Vec<_>>()
            );
            for (table, (columns, rows)) in &left {
                let (theirs_columns, theirs) = &right[table];
                assert_eq!(columns, theirs_columns, "{table} columns");
                assert_eq!(rows.len(), theirs.len(), "{table} rows");
                let keep = |column: &str| {
                    column != "rowid"
                        && !(table == "meta" && column == "modified_at")
                        && !(table == "objects" && column == "payload_hash")
                };
                let canonical = |rows: &Vec<Vec<Cell>>, mapped: bool| {
                    let mut out: Vec<String> = rows
                        .iter()
                        .map(|r| {
                            let cells: Vec<Cell> = r
                                .iter()
                                .zip(columns)
                                .filter(|(_, c)| keep(c))
                                .map(|(v, _)| if mapped { map(v) } else { v.clone() })
                                .collect();
                            format!("{cells:?}")
                        })
                        .collect();
                    out.sort();
                    out
                };
                assert_eq!(canonical(rows, false), canonical(theirs, true), "{table}");
            }
        }

        /// constraints → Fillet and Fillet → constraints (§28E) end in the
        /// same model once the identifiers each route minted are matched, and
        /// in the same part, measured and meshed independently.
        #[test]
        fn native_both_orders_publish_the_same_part() {
            if !solving() {
                return;
            }
            let (r, corner) = (2.875, [X0 + W, Y0]);
            let (at, width, depth) = ([1.375, -0.625], 33.875, 14.25);
            let plain = Fixture::drawn(Plate {
                cut_side: true,
                ..Plate::new(CCW)
            });
            let request = dimensioned(&plain, at, width, depth);

            // Constraints, then the Fillet.
            plain.ask_constraints(&[], &request);
            let a1 = plain.root.path().join("a-dimensioned.fcad");
            reply(
                plain.constrain(&a1).output().expect("process"),
                CONSTRAIN,
                0,
            );
            let ga = plain.at_copy(&a1);
            ga.ask_stored(corner, r);
            let a = ga.root.path().join("a.fcad");
            reply(ga.fillet(&a).output().expect("process"), ROUND, 0);

            // The Fillet, then the same constraints.
            plain.ask_reversed(corner, r);
            let b1 = plain.root.path().join("b-rounded.fcad");
            reply(plain.fillet(&b1).output().expect("process"), ROUND, 0);
            let gb = plain.at_copy(&b1);
            gb.ask_constraints(&[], &request);
            let b = gb.root.path().join("b.fcad");
            reply(gb.constrain(&b).output().expect("process"), CONSTRAIN, 0);

            same_sql(&a, &b, &minted(&a, &b));
            let (ma, mb) = (measure(&a, None), measure(&b, None));
            ma.same_as(&mb);
            let starts = plain.stored_starts();
            let expected: Vec<_> = starts
                .iter()
                .map(|v| solved_vertex(&starts, *v, at, width, depth))
                .collect();
            let rect = rect_of(&expected);
            assert!((ma.volume - exact(rect, r, H)).abs() < 1e-9 * ma.volume);
            let (sa, sb) = (
                mesh(&a, &a.with_extension("stl")),
                mesh(&b, &b.with_extension("stl")),
            );
            assert_eq!(sa.triangles, sb.triangles);
            assert_eq!(sa.faces, sb.faces, "the same triangles");
            assert_eq!(solved(&a), solved(&b));
        }

        /// The result behaves as any §28E part: Replace length, the radius
        /// and the height each change what they say, keeping the constraints,
        /// every name and the Fillet's joint; the coordinate editor refuses.
        #[test]
        fn native_edits_after_rounding_a_dimensioned_plate() {
            if !solving() {
                return;
            }
            let (r, corner) = (2.375, [X0 + W, Y0 + D]);
            let (at, width, depth) = ([0.5, 1.25], 30.75, 10.5);
            let (plain, g, expected) = dimensioned_plate(CCW, at, width, depth, "dimensioned");
            let first = rounded(&g, corner, r, &expected, 0, "first-rounded");
            let starts = plain.stored_starts();
            let map = |w: f64, d: f64| -> Vec<[f64; 2]> {
                starts
                    .iter()
                    .map(|v| solved_vertex(&starts, *v, at, w, d))
                    .collect()
            };
            let k = g.at_copy(&first);
            let names = stored_refs(&first);
            assert_eq!(k.catalog["sketches"][0]["editable"], false);

            // Replace the width.
            let width_rule = k.constraint_row()["constraints"]
                .as_array()
                .expect("list")
                .iter()
                .find(|c| c["rule"]["kind"] == "distance" && c["rule"]["distance"] == width)
                .expect("the width")["constraint_id"]
                .clone();
            k.ask_constraints(
                std::slice::from_ref(&width_rule),
                &[length(&k.line(0), 22.25)],
            );
            let replaced = k.root.path().join("replaced.fcad");
            let v = reply(
                k.constrain(&replaced).output().expect("process"),
                CONSTRAIN,
                0,
            );
            assert_eq!(v["result"]["solve"]["degrees_of_freedom"], 0);
            assert_eq!(stored_refs(&replaced), names);
            let m = measure(&replaced, None);
            assert!((m.volume - exact(rect_of(&map(22.25, depth)), r, H)).abs() < 1e-9 * m.volume);
            let fillet_before = k.catalog["fillets"][0].clone();
            let after = inspect(&replaced)["fillets"][0].clone();
            for key in ["feature_id", "edge", "radius_mm"] {
                assert_eq!(after[key], fillet_before[key], "{key}");
            }

            // The radius, on the replaced plate: half of 10.5 mm allows 5.25.
            let q = k.at_copy(&replaced);
            let payload = sketch_row(&replaced, q.plate_sketch_id());
            q.ask_radius(5.25);
            let rounder = q.root.path().join("rounder.fcad");
            reply(
                q.edit(&rounder).output().expect("process"),
                "edit-fillet-radius",
                0,
            );
            assert_eq!(sketch_row(&rounder, q.plate_sketch_id()), payload);
            let m = measure(&rounder, None);
            assert_eq!(m.fillet, FaceSurface::Cylinder { radius: 5.25 });
            assert!(
                (m.volume - exact(rect_of(&map(22.25, depth)), 5.25, H)).abs() < 1e-9 * m.volume
            );

            // The height.
            let t = q.at_copy(&rounder);
            let taller = t.root.path().join("taller.fcad");
            let mut c = cli();
            c.arg("edit-extrude")
                .arg(&rounder)
                .arg("--feature")
                .arg(
                    t.catalog["features"][0]["feature_id"]
                        .as_str()
                        .expect("base"),
                )
                .arg("--distance-mm")
                .arg("9.75")
                .arg("--expect-version")
                .arg(t.version())
                .arg("-o")
                .arg(&taller)
                .arg("--json");
            reply(c.output().expect("process"), "edit-extrude", 0);
            assert_eq!(sketch_row(&taller, t.plate_sketch_id()), payload);
            assert_eq!(stored_refs(&taller), names);
            let m = measure(&taller, None);
            assert!(
                (m.volume - exact(rect_of(&map(22.25, depth)), 5.25, 9.75)).abs() < 1e-9 * m.volume
            );
        }

        /// The radius bound is the solved plate's, in both directions: 7 mm,
        /// beyond the stored rectangle's 6.125 mm, publishes on a plate solved
        /// wider and deeper; 5 mm, within it, is refused on one solved
        /// narrower; a solved depth of exactly 2r publishes and 1e-7 mm less
        /// is refused. Each refusal publishes nothing.
        #[test]
        fn native_the_radius_bound_is_the_solved_plates_in_both_directions() {
            if !solving() {
                return;
            }
            let corner = [X0 + W, Y0];
            let (at, width, depth) = ([-6.0, 2.0], 41.25, 15.5);
            let (_p, g, expected) = dimensioned_plate(CCW, at, width, depth, "wide");
            rounded(&g, corner, 7.0, &expected, 0, "first-wide");

            let refuse = |g: &Fixture, r: f64, words: &[&str]| {
                let before = std::fs::read(&g.source).expect("bytes");
                let names = entries(g.root.path());
                g.ask_stored(corner, r);
                let never = g.root.path().join("never.fcad");
                let v = reply(g.fillet(&never).output().expect("process"), ROUND, 2);
                assert_eq!(refused(&v), "input", "{v}");
                let text = v.to_string();
                for w in words {
                    assert!(text.contains(w), "{w}: {v}");
                }
                assert_eq!(entries(g.root.path()), names, "a refusal left something");
                assert_eq!(std::fs::read(&g.source).expect("bytes"), before);
            };
            let (_p, g, _) = dimensioned_plate(CCW, [1.0, 1.0], 30.0, 8.5, "narrow");
            refuse(&g, 5.0, &["solve", "too short", "8.5"]);

            let r = 2.375;
            let (_p, g, expected) = dimensioned_plate(CCW, [2.5, -1.5], 28.5, 2. * r, "bound");
            rounded(&g, corner, r, &expected, 0, "first-bound");
            let (_p, g, _) = dimensioned_plate(CCW, [2.5, -1.5], 28.5, 2. * r - 1e-7, "below");
            refuse(&g, r, &["too short"]);
        }

        /// Refusals that only the solve can decide, each atomic: a solve that
        /// turns a Line off its side, and one that is no longer an
        /// axis-aligned rectangle; then a real solver conflict on the rounded
        /// result, a stale version and an occupied destination.
        #[test]
        fn native_rounding_refusals_on_a_dimensioned_plate_are_atomic() {
            if !solving() {
                return;
            }
            let plain = Fixture::drawn(Plate {
                cut_side: true,
                ..Plate::new(CCW)
            });
            let starts = plain.stored_starts();
            let across = horizontal(&starts);
            let dimension = |add: &[Value], name: &str| -> Fixture {
                plain.ask_constraints(&[], add);
                let copy = plain.root.path().join(format!("{name}.fcad"));
                reply(
                    plain.constrain(&copy).output().expect("process"),
                    CONSTRAIN,
                    0,
                );
                plain.at_copy(&copy)
            };
            let refuse = |g: &Fixture, kind: &str, words: &[&str]| -> Value {
                let before = std::fs::read(&g.source).expect("bytes");
                let names = entries(g.root.path());
                g.ask_stored([X0 + W, Y0], 1.5);
                let never = g.root.path().join("never.fcad");
                let v = reply(g.fillet(&never).output().expect("process"), ROUND, 2);
                assert_eq!(refused(&v), kind, "{v}");
                let text = v.to_string();
                for w in words {
                    assert!(text.contains(w), "{w}: {v}");
                }
                assert_eq!(entries(g.root.path()), names, "a refusal left something");
                assert_eq!(std::fs::read(&g.source).expect("bytes"), before);
                v
            };
            // The first Line's start pinned 5 mm beyond its own end, the
            // sides held H/V: a rectangle whose first Line runs the other way.
            let mut add: Vec<Value> = (0..4)
                .map(|i| {
                    rule(
                        &plain.line(i),
                        if across[i] { "horizontal" } else { "vertical" },
                    )
                })
                .collect();
            add.push(pin(&plain.line(0), starts[1][0] + 5.0, starts[0][1]));
            let flipped = dimension(&add, "flipped");
            refuse(&flipped, "input", &["side"]);
            // One length and nothing to hold the sides.
            let slanted = dimension(&[length(&plain.line(0), 30.)], "slanted");
            refuse(&slanted, "unsupported", &["rectangle"]);

            // On the rounded result: a real conflict, typed with its UUIDs.
            let (at, width, depth) = ([0., 0.], 30., 10.);
            let (_p, g, expected) = dimensioned_plate(CCW, at, width, depth, "dimensioned");
            let copy = rounded(&g, [X0 + W, Y0], 1.5, &expected, 0, "first-conflict");
            let k = g.at_copy(&copy);
            let opposite = (1..4)
                .find(|i| horizontal(&k.stored_starts())[*i])
                .expect("the other horizontal Line");
            k.ask_constraints(&[], &[length(&k.line(opposite), 20.)]);
            let before = std::fs::read(&copy).expect("bytes");
            let names = entries(k.root.path());
            let never = k.root.path().join("never.fcad");
            let v = reply(k.constrain(&never).output().expect("process"), CONSTRAIN, 2);
            assert_eq!(refused(&v), "constraint", "{v}");
            let named = v["error"]["constraint_conflict"]["constraints"]
                .as_array()
                .expect("the conflicting constraints")
                .clone();
            assert!(!named.is_empty() && named.iter().all(|c| c["constraint_id"].is_string()));
            assert_eq!(entries(k.root.path()), names);
            assert_eq!(std::fs::read(&copy).expect("bytes"), before);

            // A stale version, and a destination that is already there.
            g.ask_stored([X0 + W, Y0], 1.5);
            let names = entries(g.root.path());
            let never = g.root.path().join("never.fcad");
            let v = reply(
                g.fillet_from(&g.source, g.body_id(), &"0".repeat(64), &never)
                    .output()
                    .expect("process"),
                ROUND,
                2,
            );
            assert_eq!(refused(&v), "input", "{v}");
            let taken = g.root.path().join("taken.fcad");
            std::fs::write(&taken, b"another process owns this").expect("occupied");
            let v = reply(g.fillet(&taken).output().expect("process"), ROUND, 2);
            assert_eq!(refused(&v), "input", "{v}");
            assert_eq!(
                std::fs::read(&taken).expect("kept"),
                b"another process owns this"
            );
            std::fs::remove_file(&taken).expect("tidy");
            assert_eq!(entries(g.root.path()), names);
        }
    }
}

/// §28G: a second Fillet on another vertical corner of the rounded plate,
/// through the same `fillet-edge-copy`, into a new copy. History Extrude →
/// Fillet 1 → Fillet 2 → Body tip; both cylinders measured under their own
/// names; the pair policy at its edge; the cache cold, Miss, Hit and with the
/// first Fillet restored; every refusal atomic.
mod sequential {
    use super::constraints::first::{dimensioned_plate, sketch_row};
    use super::constraints::{axis_of, rect_of, solved, solving};
    use super::radius::{filleted_by_cli, filleted_without_kernel, stored_refs};
    use super::*;
    use ferritecad_types::ObjectId;

    const SEQUENTIAL: &str = "feature.fillet.sequential.v1";

    /// What one reference resolves to on the rebuilt part.
    #[derive(Debug, Clone, PartialEq)]
    struct Named {
        role: String,
        producer: ObjectId,
        surfaces: Vec<FaceSurface>,
        /// The axis of a single cylinder, `[x, y]`, with its direction.
        axis: Option<([f64; 2], [f64; 3])>,
        on_tip: bool,
    }

    #[derive(Debug, Clone)]
    struct Two {
        faces: u64,
        volume: f64,
        shapes: usize,
        named: BTreeMap<String, Named>,
    }
    impl Two {
        fn same_as(&self, other: &Self) {
            assert_eq!(self.faces, other.faces);
            assert!((self.volume - other.volume).abs() < 1e-9 * self.volume);
            assert_eq!(self.shapes, other.shapes);
            assert_eq!(
                self.named.keys().collect::<Vec<_>>(),
                other.named.keys().collect::<Vec<_>>()
            );
            for (id, mine) in &self.named {
                let theirs = &other.named[id];
                assert_eq!(mine.role, theirs.role, "{id}");
                assert_eq!(mine.producer, theirs.producer, "{id}");
                assert_eq!(mine.surfaces, theirs.surfaces, "{id}");
                assert_eq!(mine.on_tip, theirs.on_tip, "{id}");
                match (mine.axis, theirs.axis) {
                    (Some((a, _)), Some((b, _))) => {
                        assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
                    }
                    (None, None) => {}
                    other => panic!("{id}: {other:?}"),
                }
            }
        }
        /// The one reference of `role` owned by `producer`.
        fn one(&self, role: &str, producer: ObjectId) -> &Named {
            let found: Vec<_> = self
                .named
                .values()
                .filter(|n| n.role == role && n.producer == producer)
                .collect();
            assert_eq!(found.len(), 1, "{role} of {producer}: {found:?}");
            found[0]
        }
        fn all(&self, role: &str, producer: ObjectId) -> Vec<&Named> {
            self.named
                .values()
                .filter(|n| n.role == role && n.producer == producer)
                .collect()
        }
    }

    fn role_of(role: &SemanticRole) -> String {
        match role {
            SemanticRole::OriginFilletFace { .. } => "origin fillet face".to_owned(),
            other => role_name(other),
        }
    }

    /// Rebuilds `path` cold or through the cache at `cache`, resolving every
    /// stored reference in the kernel that built it.
    fn measure_two(path: &Path, cache: Option<&Path>) -> (Two, Vec<ferritecad_eval::CacheEvent>) {
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
                .expect("cache");
                let (built, e) = ferritecad_eval::rebuild_cached(
                    &doc,
                    &mut kernel,
                    &mut store,
                    &OperationContext::default(),
                )
                .expect("cached rebuild");
                events = e;
                built
            }
            None => ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
                .expect("cold rebuild"),
        };
        let objects = doc.objects().expect("objects");
        let body = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
            .expect("a body");
        let tip = built.shape(body.id).expect("the body was built");
        let mut named = BTreeMap::new();
        for reference in &doc.topology_refs().expect("refs") {
            let resolved = built.resolve(reference).expect("resolve");
            assert!(!resolved.is_empty(), "{} resolved to nothing", reference.id);
            let producer = built
                .shape(reference.producer_feature)
                .expect("the producer was built");
            for handle in &resolved {
                assert_eq!(handle.shape(), producer, "{} was repointed", reference.id);
            }
            let mut surfaces: Vec<FaceSurface> = resolved
                .iter()
                .map(|h| kernel.face_surface(*h).expect("surface"))
                .collect();
            surfaces.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
            let axis = match (resolved.as_slice(), surfaces.as_slice()) {
                ([one], [FaceSurface::Cylinder { .. }]) => {
                    let (origin, direction) = kernel.cylinder_axis(*one).expect("axis");
                    Some(([origin[0], origin[1]], direction))
                }
                _ => None,
            };
            named.insert(
                reference.id.to_string(),
                Named {
                    role: role_of(&reference.output_role),
                    producer: reference.producer_feature,
                    surfaces,
                    axis,
                    on_tip: producer == tip,
                },
            );
        }
        let (faces, volume) = kernel.shape_stats(tip).expect("stats");
        let two = Two {
            faces,
            volume,
            shapes: built.shape_count(),
            named,
        };
        built.release_all(&mut kernel);
        doc.close().expect("close");
        assert_eq!(kernel.live_shape_count(), 0, "every handle was released");
        (two, events)
    }

    /// The outcome the cache reported for each feature, by feature.
    fn outcomes(
        events: &[ferritecad_eval::CacheEvent],
    ) -> BTreeMap<ObjectId, Vec<ferritecad_eval::CacheOutcome>> {
        let mut by: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for e in events {
            by.entry(e.feature).or_default().push(e.outcome);
        }
        by
    }

    fn exact_two([_, _, w, d]: [f64; 4], r1: f64, r2: f64, h: f64) -> f64 {
        (w * d - (1. - PI / 4.) * (r1 * r1 + r2 * r2)) * h
    }

    fn assert_axis(named: &Named, rect: [f64; 4], corner: [f64; 2], r: f64) {
        let (origin, direction) = named.axis.expect("a cylinder axis");
        let want = axis_of(rect, corner, r);
        assert!(
            (origin[0] - want[0]).abs() < 1e-9 && (origin[1] - want[1]).abs() < 1e-9,
            "the axis {origin:?} is not r inward of {corner:?}"
        );
        assert!(direction[0].abs() < 1e-12 && direction[1].abs() < 1e-12);
    }

    fn id(v: &Value) -> ObjectId {
        v.as_str().expect("UUID").parse().expect("UUID")
    }

    /// Whether two stored corners of an axis-aligned plate share a Line.
    fn adjacent(a: [f64; 2], b: [f64; 2]) -> bool {
        a != b && (a[0] == b[0] || a[1] == b[1])
    }

    /// The part's plate and the map from a stored corner to the part's.
    struct Part {
        rect: [f64; 4],
        stored: Vec<[f64; 2]>,
        solved: Vec<[f64; 2]>,
        constrained: bool,
    }
    impl Part {
        fn plain(corners: [[f64; 2]; 4]) -> Self {
            Self {
                rect: [X0, Y0, W, D],
                stored: corners.to_vec(),
                solved: corners.to_vec(),
                constrained: false,
            }
        }
        fn corner(&self, stored: [f64; 2]) -> [f64; 2] {
            self.solved[self
                .stored
                .iter()
                .position(|v| *v == stored)
                .expect("corner")]
        }
    }

    /// The second Fillet of `g` — a rounded plate whose Fillet is at stored
    /// corner `first` with radius `r1` — at stored corner `second` with
    /// radius `r2`, published by the shipped command and measured:
    /// discovery, identities, the exact allowlist, validation, the B-Rep
    /// cold and through a real cache Miss and Hit, both cylinders under
    /// their own names, the mesh and the FBX.
    #[allow(clippy::too_many_arguments)]
    fn second_fillet(
        g: &Fixture,
        part: &Part,
        first: [f64; 2],
        r1: f64,
        second: [f64; 2],
        r2: f64,
        name: &str,
    ) -> PathBuf {
        let before = std::fs::read(&g.source).expect("source bytes");
        let refs = stored_refs(&g.source);
        let first_id = id(&g.catalog["fillets"][0]["feature_id"]);
        let first_edge = g.catalog["fillets"][0]["edge"].clone();

        // Discovery: the one saved Fillet, and the three other corners.
        let discovery = g.discovery();
        assert_eq!(discovery["available"], true, "{discovery}");
        let target = &discovery["target"];
        let base = id(&target["base_feature_id"]);
        assert_eq!(id(&target["previous_feature_id"]), first_id);
        assert_eq!(target["profile_constrained"], part.constrained);
        let fillets = target["fillets"].as_array().expect("fillets");
        assert_eq!(fillets.len(), 1);
        assert_eq!(id(&fillets[0]["feature_id"]), first_id);
        assert_eq!(fillets[0]["edge"], first_edge);
        assert_eq!(fillets[0]["radius_mm"], r1);
        assert_eq!(fillets[0]["stored_corner_mm"], json!(first));
        let candidates = g.candidates();
        assert_eq!(candidates.len(), 3, "the three other corners");
        assert!(
            candidates
                .iter()
                .all(|c| c["stored_corner_mm"] != json!(first) && c["edge"] != first_edge)
        );
        let candidate = candidates
            .iter()
            .find(|c| c["stored_corner_mm"] == json!(second))
            .expect("the second corner is offered")
            .clone();
        assert_eq!(
            id(&candidate["edge"]["feature_id"]),
            base,
            "named by the base"
        );
        if adjacent(first, second) {
            assert_eq!(id(&candidate["adjacent_fillet_feature_id"]), first_id);
            assert!(candidate["shared_line_id"].is_string());
            assert!(
                candidate["label"]
                    .as_str()
                    .expect("label")
                    .contains("shares Line")
            );
        } else {
            assert!(candidate["adjacent_fillet_feature_id"].is_null());
            assert!(candidate["shared_line_id"].is_null());
        }
        if part.constrained {
            assert!(candidate["max_radius_mm"].is_null(), "{candidate}");
        } else {
            let lengths: Vec<f64> = (0..2)
                .map(|k| candidate["adjacent_lengths_mm"][k].as_f64().expect("side"))
                .collect();
            let mut max = lengths[0].min(lengths[1]) / 2.;
            if adjacent(first, second) {
                let shared = if first[0] == second[0] { D } else { W };
                max = max.min(shared - r1 - 0.01);
            }
            let got = candidate["max_radius_mm"].as_f64().expect("bound");
            assert!((got - max).abs() < 1e-12, "{got} is not {max}");
        }

        // Publish, the pair written the other way round.
        let copy = g.root.path().join(format!("{name}.fcad"));
        g.ask_edge(
            &json!({"feature_id": candidate["edge"]["feature_id"],
                    "joint": [candidate["edge"]["joint"][1], candidate["edge"]["joint"][0]]}),
            r2,
        );
        let v = reply(g.fillet(&copy).output().expect("process"), OP, 0);
        let result = &v["result"];
        assert_eq!(result["body_id"], g.body_id());
        assert_eq!(id(&result["previous_feature_id"]), first_id, "{v}");
        assert_eq!(result["edge"], candidate["edge"], "the canonical meaning");
        assert_eq!(result["radius_mm"], r2);
        assert_eq!(result["stored_corner_mm"], json!(second));
        assert_eq!(result["profile_constrained"], part.constrained);
        let moved = part.corner(second);
        for k in 0..2 {
            let got = result["corner_mm"][k].as_f64().expect("corner");
            assert!((got - moved[k]).abs() < 1e-9, "{v} is not at {moved:?}");
        }
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
                "edge_fillet_face",
                "origin_cap",
                "origin_cap",
                "origin_fillet_face",
                "origin_side",
                "origin_side",
                "origin_side",
                "origin_side"
            ]
        );
        assert_eq!(std::fs::read(&g.source).expect("bytes"), before, "source");

        // The stored history.
        let second_id = id(&result["feature_id"]);
        let body = id(&json!(g.body_id()));
        let d = Document::open_read_only(&copy).expect("copy");
        let objects = d.objects().expect("objects");
        assert_eq!(objects.len(), 6);
        let ObjectPayload::Fillet(stored) = &objects
            .iter()
            .find(|o| o.id == second_id)
            .expect("Fillet 2")
            .payload
        else {
            panic!("a Fillet");
        };
        assert_eq!(stored.previous, first_id, "it rounds Fillet 1's result");
        assert_eq!(stored.edge.feature, base, "the producer is not substituted");
        assert_eq!(stored.radius_mm, r2);
        assert_eq!(stored.schema_version(), 2);
        let ObjectPayload::Fillet(kept) = &objects
            .iter()
            .find(|o| o.id == first_id)
            .expect("Fillet 1")
            .payload
        else {
            panic!("a Fillet");
        };
        assert_eq!(kept.schema_version(), 1, "Fillet 1 stays a §28A Fillet");
        assert!(objects.iter().any(|o| matches!(
            o.payload,
            ObjectPayload::Body(ferritecad_document::Body { tip_feature: Some(t) }) if t == second_id
        )));
        let now = d.topology_refs().expect("refs");
        d.close().expect("close");
        assert!(refs.iter().all(|r| now.contains(r)), "a name was lost");
        assert_eq!(now.len(), refs.len() + 8);
        let first_joint = kept.edge.joint;
        for r in now.iter().filter(|r| !refs.contains(r)) {
            assert_eq!((r.owner, r.producer_feature), (second_id, second_id));
            match &r.output_role {
                SemanticRole::EdgeFilletFace {
                    edge_feature,
                    joint,
                } => assert_eq!((*edge_feature, *joint), (base, stored.edge.joint)),
                SemanticRole::OriginFilletFace {
                    origin_feature,
                    edge_feature,
                    joint,
                } => assert_eq!(
                    (*origin_feature, *edge_feature, *joint),
                    (first_id, base, first_joint)
                ),
                SemanticRole::OriginCap { origin_feature, .. }
                | SemanticRole::OriginSide { origin_feature, .. } => {
                    assert_eq!(*origin_feature, base)
                }
                other => panic!("unexpected new role {other:?}"),
            }
        }
        only_a_fillet_was_added(&g.source, &copy, body, 8, &[SEQUENTIAL]);
        assert!(!capability_names(&g.source).contains(&SEQUENTIAL.to_owned()));
        assert!(capability_names(&copy).contains(&SEQUENTIAL.to_owned()));
        let schema = |path: &Path, object: ObjectId| -> i64 {
            let c = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .expect("SQL");
            c.query_row(
                "SELECT schema_version FROM objects WHERE id=?1",
                [object.to_bytes().to_vec()],
                |r| r.get(0),
            )
            .expect("row")
        };
        assert_eq!(schema(&copy, second_id), 2, "payload v2 on disk");
        assert_eq!(schema(&copy, first_id), 1);
        if part.constrained {
            let sketch = g.plate_sketch_id();
            assert_eq!(sketch_row(&copy, sketch), sketch_row(&g.source, sketch));
        }

        // The copy as the catalogue and the editors see it.
        let after = inspect(&copy);
        let row = &after["bodies"][0]["fillet_edge"];
        // §28L: a third Fillet is offered on the two corners still sharp.
        assert_eq!(row["available"], true, "{row}");
        assert_eq!(row["target"]["candidates"].as_array().map(Vec::len), Some(2));
        assert_eq!(row["target"]["fillets"].as_array().map(Vec::len), Some(2));
        assert_eq!(after["fillets"].as_array().map(Vec::len), Some(2));
        // §28H: each radius is editable, by its own UUID; §28I: and the
        // plate's height; the Sketch and its constraints are not.
        for (f, index) in after["fillets"]
            .as_array()
            .expect("fillets")
            .iter()
            .zip([1, 2])
        {
            assert_eq!(f["radius_edit"]["available"], true, "{f}");
            assert!(f["radius_edit"]["refusal"].is_null());
            assert_eq!(f["history_index"], index, "{f}");
        }
        assert_eq!(after["edit_extrude"]["available"], true, "{after}");
        let base_row = after["features"]
            .as_array()
            .expect("features")
            .iter()
            .find(|f| !f["fillet_base"].is_null())
            .expect("the base Extrude row");
        assert_eq!(base_row["refusal"], Value::Null, "{base_row}");
        assert_eq!(
            base_row["fillet_base"]["second_fillet"]["previous_feature_id"],
            base_row["fillet_base"]["fillet_feature_id"],
            "Fillet 2 rounds Fillet 1's result, not the base"
        );
        // §28J: a free plate's Sketch coordinates are editable under both
        // Fillets and a dimensioned one's are refused; §28K: the constraint
        // editor reads the history either way.
        assert_eq!(after["sketches"][0]["editable"], !part.constrained);
        assert_eq!(after["sketches"][0]["constraint_edit"]["available"], true);

        // Validation and the shipped rebuild.
        let checked = cli()
            .arg("validate")
            .arg(&copy)
            .arg("--json")
            .output()
            .expect("validate");
        assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
        let n = now.len();
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
        if part.constrained {
            let (starts, _) = solved(&copy);
            for (s, e) in starts.iter().zip(&part.solved) {
                assert!((s[0] - e[0]).abs() < 1e-9 && (s[1] - e[1]).abs() < 1e-9);
            }
        }

        // The B-Rep: cold, then a real Miss and Hit.
        let (cold, _) = measure_two(&copy, None);
        let cache = copy.with_extension("fcad-cache");
        let (miss, events) = measure_two(&copy, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Miss),
            "{events:?}"
        );
        assert_eq!(outcomes(&events).len(), 3, "the plate and both Fillets");
        let (hit, events) = measure_two(&copy, Some(&cache));
        assert!(
            events
                .iter()
                .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit),
            "{events:?}"
        );
        cold.same_as(&miss);
        cold.same_as(&hit);
        check_two(
            &cold,
            part.rect,
            (first_id, part.corner(first), r1),
            (second_id, moved, r2),
            base,
        );

        // The mesh, read independently; and the FBX.
        let m = mesh(&copy, &copy.with_extension("stl"));
        check_mesh_rounded(&m, part.rect, &[(part.corner(first), r1), (moved, r2)], H);
        fbx(&copy, name);
        copy
    }

    /// Both cylinders under their own names, the plate's faces under the
    /// base, the analytic volume.
    fn check_two(
        two: &Two,
        rect: [f64; 4],
        first: (ObjectId, [f64; 2], f64),
        second: (ObjectId, [f64; 2], f64),
        base: ObjectId,
    ) {
        check_two_at(two, rect, first, second, base, H);
    }

    /// [`check_two`] for a plate of height `h` (§28I changes it).
    fn check_two_at(
        two: &Two,
        rect: [f64; 4],
        (first, c1, r1): (ObjectId, [f64; 2], f64),
        (second, c2, r2): (ObjectId, [f64; 2], f64),
        base: ObjectId,
        h: f64,
    ) {
        assert_eq!(two.faces, 8, "two caps, four sides and two fillet faces");
        let volume = exact_two(rect, r1, r2, h);
        assert!(
            (two.volume - volume).abs() < 1e-9 * volume,
            "{} is not {volume}",
            two.volume
        );
        assert_eq!(two.shapes, 3, "the plate and both rounded plates");
        // Fillet 2's own cylinder, on the tip.
        let own = two.one("edge fillet face", second);
        assert!(own.on_tip);
        assert_eq!(own.surfaces, [FaceSurface::Cylinder { radius: r2 }]);
        assert_axis(own, rect, c2, r2);
        // Fillet 1's cylinder as the finished part has it: its own name on
        // Fillet 2, a separate proof it survived.
        let carried = two.one("origin fillet face", second);
        assert!(carried.on_tip);
        assert_eq!(carried.surfaces, [FaceSurface::Cylinder { radius: r1 }]);
        assert_axis(carried, rect, c1, r1);
        // Fillet 1's own name still addresses Fillet 1's result.
        let earlier = two.one("edge fillet face", first);
        assert!(!earlier.on_tip);
        assert_eq!(earlier.surfaces, [FaceSurface::Cylinder { radius: r1 }]);
        assert_axis(earlier, rect, c1, r1);
        assert_ne!(own.axis.map(|a| a.0), carried.axis.map(|a| a.0));
        // The plate's faces under the base, on the tip.
        for cap in ["origin cap Start", "origin cap End"] {
            let n = two.one(cap, second);
            assert!(n.on_tip);
            assert_eq!(n.surfaces, [FaceSurface::Plane]);
        }
        let sides = two.all("origin side", second);
        assert_eq!(sides.len(), 4);
        assert!(
            sides
                .iter()
                .all(|n| n.on_tip && n.surfaces == [FaceSurface::Plane])
        );
        assert_eq!(two.one("cap Start", base).surfaces, [FaceSurface::Plane]);
    }

    /// Discovery on a rounded plate and on a plate with two Fillets, and the
    /// protocol, with no kernel needed: the three other corners with the
    /// adjacent one marked, the saved Fillet's history, the same corner
    /// refused, a third Fillet and every editor of the two-Fillet history
    /// refused by name, and nothing written.
    #[test]
    fn sequential_discovery_and_protocol_without_native() {
        let f = filleted_without_kernel(CCW, [X0 + W, Y0], 2.375);
        let first = f.fillet_id().to_owned();
        let d = f.discovery();
        assert_eq!(d["available"], true, "{d}");
        let t = &d["target"];
        assert_eq!(t["previous_feature_id"], first.as_str());
        assert_eq!(t["fillets"][0]["feature_id"], first.as_str());
        assert_eq!(t["fillets"][0]["radius_mm"], 2.375);
        assert_eq!(t["fillets"][0]["corner_mm"], json!([X0 + W, Y0]));
        let candidates = f.candidates();
        assert_eq!(candidates.len(), 3);
        let marked: Vec<bool> = candidates
            .iter()
            .map(|c| c["adjacent_fillet_feature_id"] == first.as_str())
            .collect();
        assert_eq!(marked.iter().filter(|m| **m).count(), 2, "two neighbours");
        let short = f.at([X0 + W, Y0 + D]);
        assert_eq!(short["adjacent_fillet_feature_id"], first.as_str());
        assert_eq!(short["max_radius_mm"], D / 2.);
        let opposite = f.at([X0, Y0 + D]);
        assert!(opposite["adjacent_fillet_feature_id"].is_null());

        // The protocol: the same corner is refused as input before the kernel
        // is asked; a malformed request as input; and a build without the
        // kernel refuses a well-formed one as unsupported.
        let before = std::fs::read(&f.source).expect("bytes");
        let never = f.root.path().join("never.fcad");
        let edge = &t["fillets"][0]["edge"];
        f.ask_edge(edge, 1.0);
        let names = entries(f.root.path());
        // A build without a kernel reads the source and then asks for the
        // kernel before any other check, so a well-formed request is
        // `unsupported` there (the domain test refuses the same corner
        // without a kernel); with a kernel each is `input`.
        let kernel = ferritecad_occt::is_available();
        let well_formed = if kernel { "input" } else { "unsupported" };
        let v = reply(f.fillet(&never).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), well_formed, "the same corner: {v}");
        if kernel {
            assert!(v["error"]["message"].as_str().expect("m").contains(&first));
        }
        write(
            &f.request,
            &json!({"request_version": 1, "edge": short["edge"]}),
        );
        let v = reply(f.fillet(&never).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "no radius: {v}");
        for r in [0.0, 0.009, D / 2. + 1e-9] {
            f.ask_edge(&short["edge"], r);
            let v = reply(f.fillet(&never).output().expect("process"), OP, 2);
            assert_eq!(refused(&v), well_formed, "{r}: {v}");
        }
        f.ask_edge(&short["edge"], 3.0);
        let out = f.fillet(&never).output().expect("process");
        if kernel {
            assert_eq!(out.status.code(), Some(0), "{out:?}");
            std::fs::remove_file(&never).expect("clean");
        } else {
            let v = reply(out, OP, 2);
            assert_eq!(refused(&v), "unsupported", "no kernel: {v}");
        }
        assert_eq!(entries(f.root.path()), names, "a refusal left something");
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);

        // Two Fillets, written with the shipped preparation and writer.
        let mut doc = Document::open(&f.source).expect("writable");
        let body: ObjectId = f.body_id().parse().expect("UUID");
        let e = &short["edge"];
        let fillet = ferritecad_document::EdgeFillet {
            edge: ferritecad_document::FilletEdge {
                feature: id(&e["feature_id"]),
                joint: ferritecad_types::ProfileJoint::new(
                    e["joint"][0].as_str().expect("a").parse().expect("UUID"),
                    e["joint"][1].as_str().expect("b").parse().expect("UUID"),
                )
                .expect("joint"),
            },
            radius_mm: 3.0,
        };
        let prepared =
            ferritecad_document::prepare_edge_fillet(&doc, body, &fillet).expect("prepared");
        doc.write_edge_fillet(&prepared).expect("written");
        doc.close().expect("close");
        let two = inspect(&f.source);
        let row = &two["bodies"][0]["fillet_edge"];
        // §28L: a third Fillet is offered on the two corners still sharp.
        assert_eq!(row["available"], true, "{row}");
        assert_eq!(row["target"]["candidates"].as_array().map(Vec::len), Some(2));
        assert_eq!(two["fillets"].as_array().map(Vec::len), Some(2));
        // §28H: both radii are editable; §28I: and the base height; §28J:
        // and the free plate's Sketch coordinates; the constraint editor
        // still refuses.
        for fillet in two["fillets"].as_array().expect("fillets") {
            assert_eq!(fillet["radius_edit"]["available"], true, "{fillet}");
            assert!(fillet["radius_edit"]["neighbour"].is_object(), "{fillet}");
        }
        assert_eq!(two["edit_extrude"]["available"], true);
        assert_eq!(two["sketches"][0]["editable"], true);
        assert_eq!(two["sketches"][0]["constraint_edit"]["available"], true);
        assert!(two["bodies"][0]["cut_edit"]["refusal"].is_string());
        let version = two["content_version"].as_str().expect("version");
        f.ask_edge(&opposite["edge"], 1.0);
        // §28L: a third Fillet is now in the class. A build with a kernel
        // publishes it; one without refuses it as every Fillet is refused,
        // typed, before anything is written.
        let out = f
            .fillet_from(&f.source, f.body_id(), version, &never)
            .output()
            .expect("process");
        if native() {
            let v = reply(out, OP, 0);
            assert_eq!(v["result"]["references"].as_array().map(Vec::len), Some(9));
            assert!(never.exists());
        } else {
            let v = reply(out, OP, 2);
            assert_eq!(refused(&v), "unsupported", "no kernel: {v}");
            assert!(!never.exists());
        }
        let valid = cli()
            .arg("validate")
            .arg(&f.source)
            .arg("--json")
            .output()
            .expect("validate");
        assert_eq!(reply(valid, "validate", 0)["result"]["valid"], true);
    }

    /// The measured slice: adjacent (across the short and the long side) and
    /// opposite corners, fractional radii, both windings and another starting
    /// Line.
    #[test]
    fn native_second_fillets_on_adjacent_and_opposite_corners_are_what_the_numbers_say() {
        if !native() {
            return;
        }
        let first = [X0 + W, Y0];
        let r1 = 2.375;
        for (label, corners) in [
            ("ccw", CCW),
            ("cw", CW_FROM_UPPER_RIGHT),
            ("third", CCW_FROM_THIRD),
        ] {
            let g = filleted_by_cli(corners, first, r1, &format!("first-{label}"));
            let part = Part::plain(corners);
            let short = second_fillet(
                &g,
                &part,
                first,
                r1,
                [X0 + W, Y0 + D],
                3.0625,
                &format!("second-{label}-short"),
            );
            let opposite = second_fillet(
                &g,
                &part,
                first,
                r1,
                [X0, Y0 + D],
                4.5625,
                &format!("second-{label}-opposite"),
            );
            assert_ne!(
                std::fs::read(short.with_extension("stl")).expect("a"),
                std::fs::read(opposite.with_extension("stl")).expect("b")
            );
            if label == "ccw" {
                second_fillet(&g, &part, first, r1, [X0, Y0], 1.625, "second-ccw-long");
            }
        }
    }

    /// The pair policy at its edge on the short side: the bound discovery
    /// states is published and measured, anything past it — touching arcs
    /// included — is refused before publication, and §28A's own bound still
    /// holds at each corner.
    #[test]
    fn native_the_pair_policy_is_exact_at_its_bound() {
        if !native() {
            return;
        }
        let first = [X0 + W, Y0];
        let second = [X0 + W, Y0 + D];
        let r1 = D / 2.;
        let g = filleted_by_cli(CCW, first, r1, "widest-first");
        let bound = g.at(second)["max_radius_mm"].as_f64().expect("bound");
        assert!((bound - (D - r1 - 0.01)).abs() < 1e-12);
        let names = entries(g.root.path());
        let before = std::fs::read(&g.source).expect("bytes");
        let never = g.root.path().join("never.fcad");
        for (r, why) in [
            (D - r1, "touching"),
            (bound + 1e-9, "past the bound"),
            (6.12, "a flat narrower than the smallest radius"),
        ] {
            g.ask_edge(&g.at(second)["edge"], r);
            let v = reply(g.fillet(&never).output().expect("process"), OP, 2);
            assert_eq!(refused(&v), "input", "{why}: {v}");
            assert!(
                v["error"]["message"].as_str().expect("m").contains("flat"),
                "{why}: {v}"
            );
        }
        assert_eq!(entries(g.root.path()), names, "a refusal left something");
        assert_eq!(std::fs::read(&g.source).expect("bytes"), before);
        second_fillet(
            &g,
            &Part::plain(CCW),
            first,
            r1,
            second,
            bound,
            "second-bound",
        );
    }

    /// A dimensioned plate, rounded and rounded again: the stored drawing and
    /// the solved part differ visibly, discovery defers the bound, and the
    /// part — both corners, both cylinders, the volume — is the solved one.
    /// A second radius that fits the stored sides but not the solved ones is
    /// refused by the rebuild before anything is published.
    #[test]
    fn native_a_dimensioned_plate_is_rounded_twice_at_its_solved_corners() {
        if !solving() {
            return;
        }
        let at = [2.5, -1.75];
        let (width, depth) = (30.5, 8.0);
        let (_plain, g, expected) = dimensioned_plate(CCW, at, width, depth, "dimensioned");
        let first = [X0 + W, Y0];
        let r1 = 4.0;
        let rounded = g.root.path().join("first.fcad");
        g.ask_stored(first, r1);
        reply(g.fillet(&rounded).output().expect("process"), OP, 0);
        let h = g.at_copy(&rounded);
        let part = Part {
            rect: rect_of(&expected),
            stored: g.stored_starts(),
            solved: expected.clone(),
            constrained: true,
        };
        assert_ne!(part.stored, part.solved, "stored and solved differ visibly");
        let second = [X0 + W, Y0 + D];
        // Room on the stored 12.25 mm side, not on the solved 8 mm one.
        let names = entries(h.root.path());
        let never = h.root.path().join("never.fcad");
        h.ask_stored(second, 3.995);
        let v = reply(h.fillet(&never).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "{v}");
        assert!(
            v["error"]["message"].as_str().expect("m").contains("flat"),
            "{v}"
        );
        assert_eq!(entries(h.root.path()), names, "a refusal left something");
        second_fillet(&h, &part, first, r1, second, 3.25, "second-dimensioned");
        second_fillet(
            &h,
            &part,
            first,
            r1,
            [X0, Y0 + D],
            3.875,
            "second-dimensioned-opposite",
        );
    }

    /// Every refusal of the second Fillet publishes nothing and leaves no
    /// scratch file; a cancelled one leaves nothing; a lost report keeps the
    /// published copy with exit 7.
    #[test]
    fn native_second_fillet_refusals_races_cancellation_and_report_loss_are_atomic() {
        if !native() {
            return;
        }
        let first = [X0, Y0];
        let g = filleted_by_cli(CCW, first, 2.0, "rounded");
        let before = std::fs::read(&g.source).expect("bytes");
        let never = g.root.path().join("never.fcad");
        let second = [X0 + W, Y0];
        let taken = g.root.path().join("taken.fcad");
        std::fs::write(&taken, b"another process owns this").expect("occupied");
        let alias = g.root.path().join("alias.fcad");
        std::fs::hard_link(&g.source, &alias).expect("hard link");
        let names = entries(g.root.path());

        g.ask_edge(&g.at(second)["edge"], 1.5);
        for (destination, why) in [
            (g.source.clone(), "the source is not its own output"),
            (taken.clone(), "an occupied output is not replaced"),
            (alias.clone(), "an alias of the source is the source"),
        ] {
            let v = reply(g.fillet(&destination).output().expect("process"), OP, 2);
            assert_eq!(refused(&v), "input", "{why}: {v}");
        }
        assert_eq!(
            std::fs::read(&taken).expect("occupied"),
            b"another process owns this"
        );
        // The same corner, and radii the policy refuses.
        g.ask_edge(&g.fillet_row()["edge"], 1.0);
        let v = reply(g.fillet(&never).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "{v}");
        for r in [0.0, 0.009, D / 2. + 1e-9, 100.0] {
            g.ask_edge(&g.at(second)["edge"], r);
            let v = reply(g.fillet(&never).output().expect("process"), OP, 2);
            assert_eq!(refused(&v), "input", "{r}: {v}");
        }
        // A stale version: the rounded copy changed after it was inspected.
        let stale = g.root.path().join("stale-source.fcad");
        std::fs::copy(&g.source, &stale).expect("copy");
        let old = g.version().to_owned();
        let mut d = Document::open(&stale).expect("writable");
        let o = d.objects().expect("objects").remove(0);
        d.write(|w| {
            w.put_object(o.id, o.parent, o.ordinal, Some("renamed"), &o.payload)
                .map(|_| ())
        })
        .expect("change");
        d.close().expect("close");
        g.ask_edge(&g.at(second)["edge"], 1.5);
        let v = reply(
            g.fillet_from(&stale, g.body_id(), &old, &never)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert_eq!(refused(&v), "input", "stale: {v}");
        std::fs::remove_file(&stale).expect("clean");
        assert_eq!(entries(g.root.path()), names, "a refusal left something");

        // Cancellation, before the job and at its last barrier.
        let e = &g.at(second)["edge"];
        let fillet = ferritecad_document::EdgeFillet {
            edge: ferritecad_document::FilletEdge {
                feature: id(&e["feature_id"]),
                joint: ferritecad_types::ProfileJoint::new(
                    e["joint"][0].as_str().expect("a").parse().expect("UUID"),
                    e["joint"][1].as_str().expect("b").parse().expect("UUID"),
                )
                .expect("joint"),
            },
            radius_mm: 1.5,
        };
        for at in [0.0, 0.95] {
            let destination = g.root.path().join(format!("cancelled-{at}.fcad"));
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
            let d = Document::open_read_only(&g.source).expect("source");
            let expected = ferritecad_document::DocumentVersion {
                document_id: d.meta().document_id,
                content: d.content_version().expect("version"),
            };
            d.close().expect("close");
            let request = ferritecad_jobs::EdgeFilletRequest {
                source: g.source.clone(),
                expected,
                body: g.body_id().parse().expect("UUID"),
                fillet,
                destination: destination.clone(),
            };
            let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
            let result = ferritecad_jobs::fillet_edge_copy(&request, &mut kernel, &context);
            assert!(result.is_err(), "cancelled at {at}: {result:?}");
            assert!(!destination.exists());
            assert_eq!(kernel.live_shape_count(), 0);
        }
        assert_eq!(entries(g.root.path()), names, "cancellation left something");
        assert_eq!(std::fs::read(&g.source).expect("bytes"), before);

        // A lost report: the copy is published and kept, exit 7.
        let lost = g.root.path().join("lost-report.fcad");
        assert_eq!(
            g.fillet(&lost)
                .stdout(pipe::closed_pipe())
                .stderr(pipe::closed_pipe())
                .status()
                .expect("pipes")
                .code(),
            Some(7)
        );
        assert!(lost.exists(), "the published copy is kept");
        let kept = inspect(&lost);
        assert_eq!(kept["fillets"].as_array().map(Vec::len), Some(2));
        assert_eq!(kept["bodies"][0]["fillet_edge"]["available"], true);
        assert_eq!(std::fs::read(&g.source).expect("bytes"), before);
    }

    /// The first Fillet restored from the cache: Fillet 2 rebuilt on it picks
    /// the same edge a cold build does. A changed first radius or Sketch
    /// invalidates the suffix and nothing before it.
    #[test]
    fn native_a_second_fillet_on_a_restored_first_and_invalidated_suffixes() {
        if !native() {
            return;
        }
        let first = [X0 + W, Y0];
        let second = [X0 + W, Y0 + D];
        let (r1, r2) = (2.375, 3.0625);
        let g = filleted_by_cli(CCW, first, r1, "first");
        let copy = second_fillet(
            &g,
            &Part::plain(CCW),
            first,
            r1,
            second,
            r2,
            "second-restored",
        );
        let cache = copy.with_extension("fcad-cache");
        let d = Document::open_read_only(&copy).expect("copy");
        let objects = d.objects().expect("objects");
        d.close().expect("close");
        let base = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
            .expect("base")
            .clone();
        let fillet = |previous_is_base: bool| {
            objects
                .iter()
                .find(|o| {
                    matches!(&o.payload, ObjectPayload::Fillet(f)
                        if (f.previous == base.id) == previous_is_base)
                })
                .expect("a Fillet")
                .clone()
        };
        let (one, two) = (fillet(true), fillet(false));
        let rewrite = |object: &ferritecad_document::ObjectRecord, payload: ObjectPayload| {
            let mut d = Document::open(&copy).expect("writable");
            d.write(|w| {
                w.put_object(
                    object.id,
                    object.parent,
                    object.ordinal,
                    object.name.as_deref(),
                    &payload,
                )
                .map(|_| ())
            })
            .expect("in place");
            d.close().expect("close");
        };
        use ferritecad_eval::CacheOutcome::{Hit, Miss};
        let judge = |events: &[ferritecad_eval::CacheEvent], want: [(ObjectId, _); 3]| {
            let by = outcomes(events);
            for (feature, outcome) in want {
                assert_eq!(by.get(&feature), Some(&vec![outcome]), "{events:?}");
            }
        };

        // Fillet 2's radius, in place: the plate and Fillet 1 are restored,
        // Fillet 2 is rebuilt on the restored Fillet 1.
        let ObjectPayload::Fillet(mut p) = two.payload.clone() else {
            panic!("a Fillet");
        };
        let r2b = 1.8125;
        p.radius_mm = r2b;
        rewrite(&two, ObjectPayload::Fillet(p));
        let (warm, events) = measure_two(&copy, Some(&cache));
        judge(&events, [(base.id, Hit), (one.id, Hit), (two.id, Miss)]);
        let (cold, _) = measure_two(&copy, None);
        cold.same_as(&warm);
        let rect = [X0, Y0, W, D];
        check_two(
            &cold,
            rect,
            (one.id, first, r1),
            (two.id, second, r2b),
            base.id,
        );
        let (hit, events) = measure_two(&copy, Some(&cache));
        judge(&events, [(base.id, Hit), (one.id, Hit), (two.id, Hit)]);
        cold.same_as(&hit);

        // Fillet 1's radius: the suffix misses, the plate is reused.
        let ObjectPayload::Fillet(mut p) = one.payload.clone() else {
            panic!("a Fillet");
        };
        let r1b = 1.4375;
        p.radius_mm = r1b;
        rewrite(&one, ObjectPayload::Fillet(p));
        let (warm, events) = measure_two(&copy, Some(&cache));
        judge(&events, [(base.id, Hit), (one.id, Miss), (two.id, Miss)]);
        let (cold, _) = measure_two(&copy, None);
        cold.same_as(&warm);
        check_two(
            &cold,
            rect,
            (one.id, first, r1b),
            (two.id, second, r2b),
            base.id,
        );

        // The Sketch, moved along X: everything misses, both axes move.
        let sketch = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("the Sketch")
            .clone();
        let ObjectPayload::Sketch(mut s) = sketch.payload.clone() else {
            panic!("a Sketch");
        };
        let dx = 1.5;
        for c in &mut s.curves {
            if let ferritecad_document::SketchGeometry::Line { start, end } = &mut c.geometry {
                *start = ferritecad_document::Point2::new(start.x + dx, start.y).expect("p");
                *end = ferritecad_document::Point2::new(end.x + dx, end.y).expect("p");
            }
        }
        rewrite(&sketch, ObjectPayload::Sketch(s));
        let (warm, events) = measure_two(&copy, Some(&cache));
        judge(&events, [(base.id, Miss), (one.id, Miss), (two.id, Miss)]);
        let (cold, _) = measure_two(&copy, None);
        cold.same_as(&warm);
        check_two(
            &cold,
            [X0 + dx, Y0, W, D],
            (one.id, [first[0] + dx, first[1]], r1b),
            (two.id, [second[0] + dx, second[1]], r2b),
            base.id,
        );
    }

    /// The evaluator states the class again at every rebuild: a saved second
    /// Fillet this build would not have written is refused by name.
    #[test]
    fn native_the_evaluator_refuses_a_saved_second_fillet_outside_the_class() {
        if !native() {
            return;
        }
        let first = [X0 + W, Y0];
        let g = filleted_by_cli(CCW, first, 6.125, "first");
        let copy = second_fillet(
            &g,
            &Part::plain(CCW),
            first,
            6.125,
            [X0, Y0 + D],
            2.0,
            "second-forged-base",
        );
        let d = Document::open_read_only(&copy).expect("copy");
        let objects = d.objects().expect("objects");
        d.close().expect("close");
        let (saved, stored, one) = objects
            .iter()
            .find_map(|o| match &o.payload {
                ObjectPayload::Fillet(f)
                    if !objects.iter().any(|b| {
                        b.id == f.previous && matches!(b.payload, ObjectPayload::Extrude(_))
                    }) =>
                {
                    Some((o.clone(), f.clone(), f.previous))
                }
                _ => None,
            })
            .expect("Fillet 2");
        let ObjectPayload::Fillet(first_payload) = &objects
            .iter()
            .find(|o| o.id == one)
            .expect("Fillet 1")
            .payload
        else {
            panic!("a Fillet");
        };
        let adjacent_joint = {
            // The corner sharing the 12.25 mm Line with Fillet 1.
            let target = &g.at([X0 + W, Y0 + D])["edge"]["joint"];
            ferritecad_types::ProfileJoint::new(
                target[0].as_str().expect("a").parse().expect("UUID"),
                target[1].as_str().expect("b").parse().expect("UUID"),
            )
            .expect("joint")
        };
        let mut forged = Vec::new();
        let mut same = stored.clone();
        same.edge.joint = first_payload.edge.joint;
        forged.push(("the same corner twice", same, "already rounded"));
        let mut touching = stored.clone();
        touching.edge.joint = adjacent_joint;
        touching.radius_mm = 6.125;
        forged.push(("touching arcs", touching, "flat"));
        let mut producer = stored.clone();
        producer.edge.feature = one;
        forged.push((
            "the first Fillet as the producer",
            producer,
            "not an Extrude",
        ));
        for (why, payload, expect) in forged {
            let path = g.root.path().join("forged.fcad");
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

    /// §28H: the radius of either Fillet of the §28G history, through the
    /// existing `edit-fillet-radius`, into a new copy. Nothing is minted;
    /// only the selected payload moves; both cylinders stay under their own
    /// names; the suffix of the cache is rebuilt and nothing before it.
    mod radius {
        use super::super::radius::only_the_radius_changed;
        use super::*;

        const EDIT: &str = "edit-fillet-radius";

        /// A plate rounded twice by the shipped command, as a fixture reading
        /// the two-Fillet copy. The first fixture owns the files.
        pub(super) fn rounded_twice(
            part: &Part,
            corners: [[f64; 2]; 4],
            (first, r1): ([f64; 2], f64),
            (second, r2): ([f64; 2], f64),
            name: &str,
        ) -> (Fixture, Fixture) {
            let g = filleted_by_cli(corners, first, r1, &format!("first-{name}"));
            let two = second_fillet(&g, part, first, r1, second, r2, &format!("two-{name}"));
            let h = g.at_copy(&two);
            (g, h)
        }

        /// The two Fillet rows of a two-Fillet copy, in history order.
        pub(super) fn rows(f: &Fixture) -> [Value; 2] {
            let rows = f.catalog["fillets"].as_array().expect("fillets").clone();
            assert_eq!(rows.len(), 2);
            let first = rows
                .iter()
                .find(|r| r["history_index"] == 1)
                .expect("Fillet 1")
                .clone();
            let second = rows
                .iter()
                .find(|r| r["history_index"] == 2)
                .expect("Fillet 2")
                .clone();
            [first, second]
        }

        /// One radius edit of Fillet `which` (1 or 2) of `h`, published by
        /// the shipped command and measured: the result, the exact allowlist,
        /// every name kept, the other Fillet unchanged, validation, the
        /// B-Rep cold and through a real Miss and Hit (both cylinders under
        /// their own names, Fillet 1's carried into the final Body, the
        /// analytic volume), the mesh and the FBX.
        #[allow(clippy::too_many_arguments)]
        pub(super) fn edited_either(
            h: &Fixture,
            part: &Part,
            which: usize,
            (c1, r1): ([f64; 2], f64),
            (c2, r2): ([f64; 2], f64),
            r: f64,
            name: &str,
        ) -> PathBuf {
            let before = std::fs::read(&h.source).expect("source bytes");
            let refs = stored_refs(&h.source);
            let [one, two] = rows(h);
            let (row, other) = if which == 1 {
                (&one, &two)
            } else {
                (&two, &one)
            };
            let id = row["feature_id"].as_str().expect("UUID").to_owned();
            let from = if which == 1 { r1 } else { r2 };
            let copy = h.root.path().join(format!("{name}.fcad"));
            h.ask_radius(r);
            let v = reply(
                h.edit_from(&h.source, &id, h.version(), &copy)
                    .output()
                    .expect("process"),
                EDIT,
                0,
            );
            let result = &v["result"];
            assert_eq!(result["feature_id"], id.as_str(), "the selected Fillet");
            assert_eq!(result["history_index"], which);
            assert_eq!(result["previous_feature_id"], row["previous_feature_id"]);
            assert_eq!(result["edge"], row["edge"], "the same edge");
            assert_eq!(result["previous_radius_mm"], from);
            assert_eq!(result["radius_mm"], r);
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before, "source");
            assert!(only_the_radius_changed(&h.source, &copy, &id) >= 2);
            assert_eq!(stored_refs(&copy), refs, "every name and its UUID kept");
            let after = inspect(&copy);
            let [a1, a2] = rows(&h.at_copy(&copy));
            let (mine, theirs) = if which == 1 { (&a1, &a2) } else { (&a2, &a1) };
            assert_eq!(mine["radius_mm"], r);
            assert_eq!(theirs["radius_mm"], other["radius_mm"], "the other radius");
            for key in ["feature_id", "edge", "previous_feature_id"] {
                assert_eq!(mine[key], row[key], "{key}");
                assert_eq!(theirs[key], other[key], "{key}");
            }
            // §28L: two corners are still sharp, so a third Fillet is offered.
            assert_eq!(after["bodies"][0]["fillet_edge"]["available"], true);
            // §28I: the plate's height is editable under both Fillets.
            assert_eq!(after["edit_extrude"]["available"], true);

            let checked = cli()
                .arg("validate")
                .arg(&copy)
                .arg("--json")
                .output()
                .expect("validate");
            assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
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

            let (r1, r2) = if which == 1 { (r, r2) } else { (r1, r) };
            let first = id_of(&one["feature_id"]);
            let second = id_of(&two["feature_id"]);
            let base = id_of(&one["edge"]["feature_id"]);
            let (cold, _) = measure_two(&copy, None);
            let cache = copy.with_extension("fcad-cache");
            let (miss, events) = measure_two(&copy, Some(&cache));
            assert!(
                events
                    .iter()
                    .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Miss)
            );
            let (hit, events) = measure_two(&copy, Some(&cache));
            assert!(
                events
                    .iter()
                    .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
            );
            cold.same_as(&miss);
            cold.same_as(&hit);
            let c1 = part.corner(c1);
            let c2 = part.corner(c2);
            check_two(&cold, part.rect, (first, c1, r1), (second, c2, r2), base);
            let m = mesh(&copy, &copy.with_extension("stl"));
            check_mesh_rounded(&m, part.rect, &[(c1, r1), (c2, r2)], H);
            fbx(&copy, name);
            copy
        }

        pub(super) fn id_of(v: &Value) -> ferritecad_types::ObjectId {
            v.as_str().expect("UUID").parse().expect("UUID")
        }

        /// Discovery on a two-Fillet plate written without a kernel, and the
        /// protocol: both rows editable with their place and neighbour, the
        /// exact bound, and a build without the kernel refusing a well-formed
        /// request before writing anything.
        #[test]
        fn radius_discovery_and_protocol_without_native() {
            let f = filleted_without_kernel(CCW, [X0 + W, Y0], 6.0);
            // The second Fillet across the 12.25 mm side, written by the
            // shipped preparation and writer.
            let short = f.at([X0 + W, Y0 + D]).clone();
            let mut doc = Document::open(&f.source).expect("writable");
            let body: ferritecad_types::ObjectId = f.body_id().parse().expect("UUID");
            let e = &short["edge"];
            let fillet = ferritecad_document::EdgeFillet {
                edge: ferritecad_document::FilletEdge {
                    feature: id_of(&e["feature_id"]),
                    joint: ferritecad_types::ProfileJoint::new(
                        e["joint"][0].as_str().expect("a").parse().expect("UUID"),
                        e["joint"][1].as_str().expect("b").parse().expect("UUID"),
                    )
                    .expect("joint"),
                },
                radius_mm: 6.12,
            };
            let prepared =
                ferritecad_document::prepare_edge_fillet(&doc, body, &fillet).expect("prepared");
            doc.write_edge_fillet(&prepared).expect("written");
            doc.close().expect("close");
            let h = Fixture {
                catalog: inspect(&f.source),
                root: tempfile::tempdir().expect("directory"),
                source: f.source.clone(),
                request: f.request.clone(),
            };
            let [one, two] = rows(&h);
            for (row, other, index) in [(&one, &two, 1), (&two, &one, 2)] {
                let edit = &row["radius_edit"];
                assert_eq!(edit["available"], true, "{row}");
                assert!(edit["refusal"].is_null());
                let n = &edit["neighbour"];
                assert_eq!(n["feature_id"], other["feature_id"]);
                assert_eq!(n["history_index"], 3 - index);
                assert_eq!(n["radius_mm"], other["radius_mm"]);
                assert_eq!(n["stored_shared_length_mm"], D);
                assert!(n["shared_line_id"].is_string());
            }
            assert_eq!(one["previous_feature_id"], one["edge"]["feature_id"]);
            assert_eq!(two["previous_feature_id"], one["feature_id"]);
            // Fillet 1 beside r2 = 6.12: the pair bound, below §28A's 6.125,
            // exactly as the domain states it; Fillet 2: §28A's 6.125 caps
            // 12.25 − 6 − 0.01.
            let max1 = one["radius_edit"]["max_radius_mm"].as_f64().expect("bound");
            assert_eq!(max1, ferritecad_document::pair_bound_of_first(D, 6.12));
            assert!(max1 < D / 2.);
            assert_eq!(two["radius_edit"]["max_radius_mm"], D / 2.);

            // The protocol.
            let before = std::fs::read(&h.source).expect("bytes");
            let never = h.root.path().join("never.fcad");
            h.ask_radius(1.0);
            let names = entries(h.root.path());
            let kernel = ferritecad_occt::is_available();
            let well_formed = if kernel { "input" } else { "unsupported" };
            let id1 = one["feature_id"].as_str().expect("UUID");
            let id2 = two["feature_id"].as_str().expect("UUID");
            for (id, r, why) in [
                (id1, max1.next_up(), "past the pair bound"),
                (id1, 0.009, "below the smallest radius"),
                (id2, D / 2. + 1e-9, "past §28A's bound"),
            ] {
                h.ask_radius(r);
                let v = reply(
                    h.edit_from(&h.source, id, h.version(), &never)
                        .output()
                        .expect("process"),
                    EDIT,
                    2,
                );
                assert_eq!(refused(&v), well_formed, "{why}: {v}");
            }
            // The base Extrude is no Fillet; a malformed request is input.
            h.ask_radius(1.0);
            let base = one["edge"]["feature_id"].as_str().expect("UUID");
            let v = reply(
                h.edit_from(&h.source, base, h.version(), &never)
                    .output()
                    .expect("process"),
                EDIT,
                2,
            );
            assert!(matches!(refused(&v), "input" | "unsupported"), "{v}");
            write(&h.request, &json!({"request_version": 1}));
            let v = reply(
                h.edit_from(&h.source, id2, h.version(), &never)
                    .output()
                    .expect("process"),
                EDIT,
                2,
            );
            assert_eq!(refused(&v), "input", "no radius: {v}");
            h.ask_radius(1.0);
            assert_eq!(entries(h.root.path()), names, "a refusal left something");
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
        }

        /// Both Fillets of adjacent and opposite corners, up and down, both
        /// windings; equal radii keep the two cylinders apart by name.
        #[test]
        fn native_editing_either_radius_is_what_the_numbers_say() {
            if !native() {
                return;
            }
            let first = [X0 + W, Y0];
            for (label, corners) in [("ccw", CCW), ("cw", CW_FROM_UPPER_RIGHT)] {
                let part = Part::plain(corners);
                let (r1, r2) = (2.375, 3.0625);
                let short = [X0 + W, Y0 + D];
                let (_g, h) = rounded_twice(
                    &part,
                    corners,
                    (first, r1),
                    (short, r2),
                    &format!("r-{label}"),
                );
                edited_either(
                    &h,
                    &part,
                    1,
                    (first, r1),
                    (short, r2),
                    4.8125,
                    &format!("radius-{label}-f1-up"),
                );
                edited_either(
                    &h,
                    &part,
                    1,
                    (first, r1),
                    (short, r2),
                    1.1875,
                    &format!("radius-{label}-f1-down"),
                );
                edited_either(
                    &h,
                    &part,
                    2,
                    (first, r1),
                    (short, r2),
                    5.4375,
                    &format!("radius-{label}-f2-up"),
                );
                // Equal radii: two cylinders of one radius, each still under
                // its own UUIDs at its own corner.
                edited_either(
                    &h,
                    &part,
                    2,
                    (first, r1),
                    (short, r2),
                    r1,
                    &format!("radius-{label}-f2-equal"),
                );
            }
            let part = Part::plain(CCW);
            let opposite = [X0, Y0 + D];
            let (_g, h) =
                rounded_twice(&part, CCW, (first, 4.5625), (opposite, 1.625), "r-opposite");
            edited_either(
                &h,
                &part,
                1,
                (first, 4.5625),
                (opposite, 1.625),
                6.125,
                "radius-opposite-f1",
            );
            edited_either(
                &h,
                &part,
                2,
                (first, 4.5625),
                (opposite, 1.625),
                6.125,
                "radius-opposite-f2",
            );
        }

        /// The bound discovery states for Fillet 1 beside a wide Fillet 2 is
        /// published and measured; the next float is refused and nothing is
        /// written.
        #[test]
        fn native_the_pair_bound_is_exact_when_editing_the_first_radius() {
            if !native() {
                return;
            }
            let part = Part::plain(CCW);
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            let (_g, h) = rounded_twice(&part, CCW, (first, 6.0), (second, 6.12), "r-bound");
            let [one, _] = rows(&h);
            let bound = one["radius_edit"]["max_radius_mm"].as_f64().expect("bound");
            assert_eq!(bound, ferritecad_document::pair_bound_of_first(D, 6.12));
            let never = h.root.path().join("never.fcad");
            let names = entries(h.root.path());
            let id = one["feature_id"].as_str().expect("UUID");
            h.ask_radius(bound.next_up());
            let v = reply(
                h.edit_from(&h.source, id, h.version(), &never)
                    .output()
                    .expect("process"),
                EDIT,
                2,
            );
            assert_eq!(refused(&v), "input", "{v}");
            assert!(
                v["error"]["message"].as_str().expect("m").contains("flat"),
                "{v}"
            );
            h.ask_radius(bound);
            assert_eq!(entries(h.root.path()), names, "a refusal left something");
            edited_either(
                &h,
                &part,
                1,
                (first, 6.0),
                (second, 6.12),
                bound,
                "radius-bound",
            );
        }

        /// The first Fillet restored from the cache stays when the second
        /// changes; a changed first radius rebuilds both and nothing before.
        #[test]
        fn native_either_radius_edit_invalidates_exactly_its_suffix() {
            if !native() {
                return;
            }
            let part = Part::plain(CCW);
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            let (_g, h) = rounded_twice(&part, CCW, (first, 2.375), (second, 3.0625), "r-cache");
            let warm = h.source.with_extension("fcad-cache");
            measure_two(&h.source, Some(&warm));
            let (_, events) = measure_two(&h.source, Some(&warm));
            assert!(
                events
                    .iter()
                    .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
            );
            let [one, two] = rows(&h);
            let (f1, f2, base) = (
                id_of(&one["feature_id"]),
                id_of(&two["feature_id"]),
                id_of(&one["edge"]["feature_id"]),
            );
            use ferritecad_eval::CacheOutcome::{Hit, Miss};
            for (which, r, want, name) in [
                (2, 4.25, [Hit, Hit, Miss], "cache-f2"),
                (1, 4.25, [Hit, Miss, Miss], "cache-f1"),
            ] {
                let id = if which == 1 { f1 } else { f2 };
                let copy = h.root.path().join(format!("{name}.fcad"));
                h.ask_radius(r);
                reply(
                    h.edit_from(&h.source, &id.to_string(), h.version(), &copy)
                        .output()
                        .expect("process"),
                    EDIT,
                    0,
                );
                // The copy keeps the document's identity, so the source's warm
                // entries are the ones a reader of the copy would find.
                let cache = copy.with_extension("fcad-cache");
                std::fs::copy(&warm, &cache).expect("the warm cache");
                let (warmed, events) = measure_two(&copy, Some(&cache));
                let by = outcomes(&events);
                for (feature, outcome) in [(base, want[0]), (f1, want[1]), (f2, want[2])] {
                    assert_eq!(by.get(&feature), Some(&vec![outcome]), "{name}: {events:?}");
                }
                let (cold, _) = measure_two(&copy, None);
                cold.same_as(&warmed);
                let (r1, r2) = if which == 1 { (r, 3.0625) } else { (2.375, r) };
                check_two(&cold, part.rect, (f1, first, r1), (f2, second, r2), base);
                let (hit, events) = measure_two(&copy, Some(&cache));
                assert!(events.iter().all(|e| e.outcome == Hit), "{events:?}");
                cold.same_as(&hit);
            }
        }

        /// A dimensioned plate: the stored drawing and the solved part differ;
        /// discovery defers every bound; the rebuild judges both corners and
        /// the pair on the solved Lines, refusing a radius the stored drawing
        /// would allow, and publishing the solved part.
        #[test]
        fn native_dimensioned_radius_edits_answer_to_the_solved_plate() {
            if !solving() {
                return;
            }
            let at = [2.5, -1.75];
            let (width, depth) = (30.5, 8.0);
            let (_plain, g, expected) = dimensioned_plate(CCW, at, width, depth, "r-dimensioned");
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            let one = g.root.path().join("one.fcad");
            g.ask_stored(first, 3.5);
            reply(g.fillet(&one).output().expect("process"), OP, 0);
            let g1 = g.at_copy(&one);
            let part = Part {
                rect: rect_of(&expected),
                stored: g.stored_starts(),
                solved: expected.clone(),
                constrained: true,
            };
            let two = second_fillet(&g1, &part, first, 3.5, second, 3.0, "r-dimensioned-two");
            let h = g1.at_copy(&two);
            let [r1row, r2row] = rows(&h);
            for row in [&r1row, &r2row] {
                assert!(row["radius_edit"]["max_radius_mm"].is_null(), "{row}");
                assert_eq!(row["profile_constrained"], true);
                assert_eq!(
                    row["radius_edit"]["neighbour"]["stored_shared_length_mm"],
                    D
                );
            }
            // 4.0 each fits §28A on the solved 8 mm side, the stored 12.25 mm
            // side lets both through, and together they touch: refused by the
            // rebuild, nothing written.
            let e1 = edited_either(
                &h,
                &part,
                1,
                (first, 3.5),
                (second, 3.0),
                4.0,
                "radius-dimensioned-f1",
            );
            let h1 = h.at_copy(&e1);
            let never = h1.root.path().join("never.fcad");
            let names = entries(h1.root.path());
            h1.ask_radius(4.0);
            let id2 = r2row["feature_id"].as_str().expect("UUID");
            let v = reply(
                h1.edit_from(&h1.source, id2, h1.version(), &never)
                    .output()
                    .expect("process"),
                EDIT,
                2,
            );
            assert_eq!(refused(&v), "input", "{v}");
            assert!(
                v["error"]["message"].as_str().expect("m").contains("flat"),
                "{v}"
            );
            h1.ask_radius(3.99);
            assert_eq!(entries(h1.root.path()), names, "a refusal left something");
            edited_either(
                &h1,
                &part,
                2,
                (first, 4.0),
                (second, 3.0),
                3.99,
                "radius-dimensioned-f2",
            );
        }

        /// Refusals of either edit publish nothing and leave no scratch file; a
        /// cancelled one leaves nothing; a lost report keeps the copy, exit 7.
        #[test]
        fn native_either_radius_refusals_cancellation_and_report_loss_are_atomic() {
            if !native() {
                return;
            }
            let part = Part::plain(CCW);
            let (first, second) = ([X0, Y0], [X0 + W, Y0]);
            let (_g, h) = rounded_twice(&part, CCW, (first, 2.0), (second, 1.5), "r-atomic");
            let [one, two] = rows(&h);
            let (id1, id2) = (
                one["feature_id"].as_str().expect("UUID").to_owned(),
                two["feature_id"].as_str().expect("UUID").to_owned(),
            );
            let before = std::fs::read(&h.source).expect("bytes");
            let taken = h.root.path().join("taken.fcad");
            std::fs::write(&taken, b"another process owns this").expect("occupied");
            let alias = h.root.path().join("alias.fcad");
            std::fs::hard_link(&h.source, &alias).expect("hard link");
            h.ask_radius(2.5);
            let names = entries(h.root.path());
            for (id, destination, why) in [
                (&id1, h.source.clone(), "the source is not its own output"),
                (&id2, taken.clone(), "an occupied output is not replaced"),
                (&id1, alias.clone(), "an alias of the source is the source"),
            ] {
                let v = reply(
                    h.edit_from(&h.source, id, h.version(), &destination)
                        .output()
                        .expect("process"),
                    EDIT,
                    2,
                );
                assert_eq!(refused(&v), "input", "{why}: {v}");
            }
            assert_eq!(
                std::fs::read(&taken).expect("taken"),
                b"another process owns this"
            );
            // A stale version.
            let stale = h.root.path().join("stale-source.fcad");
            std::fs::copy(&h.source, &stale).expect("copy");
            let mut d = Document::open(&stale).expect("writable");
            let o = d.objects().expect("objects").remove(0);
            d.write(|w| {
                w.put_object(o.id, o.parent, o.ordinal, Some("renamed"), &o.payload)
                    .map(|_| ())
            })
            .expect("change");
            d.close().expect("close");
            let never = h.root.path().join("never.fcad");
            let v = reply(
                h.edit_from(&stale, &id2, h.version(), &never)
                    .output()
                    .expect("process"),
                EDIT,
                2,
            );
            assert_eq!(refused(&v), "input", "stale: {v}");
            std::fs::remove_file(&stale).expect("clean");
            assert_eq!(entries(h.root.path()), names, "a refusal left something");

            // Cancelled at the last barrier, through the shared job.
            let d = Document::open_read_only(&h.source).expect("source");
            let expected = ferritecad_document::DocumentVersion {
                document_id: d.meta().document_id,
                content: d.content_version().expect("version"),
            };
            d.close().expect("close");
            let destination = h.root.path().join("cancelled.fcad");
            let token = ferritecad_kernel::CancelToken::new();
            let stop = token.clone();
            let context = OperationContext::default()
                .with_cancel(token)
                .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                    if fraction >= 0.95 {
                        stop.cancel();
                    }
                }));
            let request = ferritecad_jobs::EditFilletRadiusRequest {
                source: h.source.clone(),
                expected,
                feature: id_of(&json!(id1)),
                radius_mm: 2.5,
                destination: destination.clone(),
            };
            let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
            let result = ferritecad_jobs::edit_fillet_radius_copy(&request, &mut kernel, &context);
            assert!(result.is_err(), "{result:?}");
            assert!(!destination.exists());
            assert_eq!(kernel.live_shape_count(), 0);
            assert_eq!(entries(h.root.path()), names, "cancellation left something");

            // A lost report: published, kept, exit 7.
            let lost = h.root.path().join("lost-report.fcad");
            assert_eq!(
                h.edit_from(&h.source, &id2, h.version(), &lost)
                    .stdout(pipe::closed_pipe())
                    .stderr(pipe::closed_pipe())
                    .status()
                    .expect("pipes")
                    .code(),
                Some(7)
            );
            assert!(lost.exists());
            let [_, kept] = rows(&h.at_copy(&lost));
            assert_eq!(kept["radius_mm"], 2.5);
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
        }
    }

    /// §28I: the base height of Extrude -> Fillet 1 -> Fillet 2 -> Body,
    /// through the existing `edit-extrude`.
    mod height {
        use super::radius::{id_of, rounded_twice, rows};
        use super::*;

        const EDIT: &str = "edit-extrude";

        /// One height edit of `h`'s base Extrude, published by the shipped
        /// command and measured: discovery naming both Fillets in history
        /// order, the exact allowlist, every name kept, both Fillet rows and
        /// the Sketch unchanged, validation, the B-Rep cold and through a
        /// real Miss and Hit (both cylinders under their own names at the new
        /// height, Fillet 1's carried into the final Body, the analytic
        /// volume), the independent mesh and the FBX.
        #[allow(clippy::too_many_arguments)]
        pub(super) fn raised_twice(
            h: &Fixture,
            part: &Part,
            (c1, r1): ([f64; 2], f64),
            (c2, r2): ([f64; 2], f64),
            height: f64,
            name: &str,
        ) -> PathBuf {
            let before = std::fs::read(&h.source).expect("source bytes");
            let refs = stored_refs(&h.source);
            let [one, two] = rows(h);
            let base = h.base_id().to_owned();
            let row = h.base_row().clone();
            assert_eq!(row["editable"], true, "{row}");
            assert!(row["refusal"].is_null(), "{row}");
            let context = &row["fillet_base"];
            assert_eq!(context["fillet_feature_id"], one["feature_id"]);
            assert_eq!(context["radius_mm"], r1);
            assert_eq!(context["profile_constrained"], part.constrained);
            let later = &context["second_fillet"];
            assert_eq!(later["fillet_feature_id"], two["feature_id"]);
            assert_eq!(
                later["previous_feature_id"], one["feature_id"],
                "Fillet 2 rounds Fillet 1's result, not the base"
            );
            assert_eq!(later["history_index"], 2);
            assert_eq!(later["edge"], two["edge"]);
            assert_eq!(later["radius_mm"], r2);
            assert_eq!(id_of(&later["edge"]["feature_id"]), id_of(&json!(base)));

            let copy = h.root.path().join(format!("{name}.fcad"));
            let v = reply(
                h.raise(&h.source, &base, &height.to_string(), &copy)
                    .output()
                    .expect("process"),
                EDIT,
                0,
            );
            assert_eq!(v["result"]["feature_id"], base.as_str(), "{v}");
            assert_eq!(v["result"]["document_id"], h.catalog["document_id"]);
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before, "source");
            let moved = super::super::height::only_the_height_changed(&h.source, &copy, &base);
            if row["distance_mm"] == height {
                assert!(moved <= 1, "the same height moves only the stamp");
            } else {
                assert!(moved >= 2, "the payload and its hash");
            }
            assert_eq!(stored_refs(&copy), refs, "every name and its UUID kept");
            let after = h.at_copy(&copy);
            assert_eq!(after.base_row()["distance_mm"], height);
            assert_eq!(
                after.base_row()["fillet_base"],
                row["fillet_base"],
                "both Fillets as they were"
            );
            assert_eq!(rows(&after), [one.clone(), two.clone()]);
            assert_eq!(after.catalog["edit_extrude"]["available"], true);
            assert_eq!(after.catalog["sketches"][0]["editable"], !part.constrained);
            assert_eq!(
                after.catalog["sketches"][0]["constraint_edit"]["available"],
                true
            );
            if part.constrained {
                let sketch = after.catalog["sketches"][0]["sketch_id"]
                    .as_str()
                    .expect("Sketch UUID");
                assert_eq!(
                    sketch_row(&copy, sketch),
                    sketch_row(&h.source, sketch),
                    "the stored Sketch stays the solver's starting guess"
                );
            }

            let checked = cli()
                .arg("validate")
                .arg(&copy)
                .arg("--json")
                .output()
                .expect("validate");
            assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
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
            if part.constrained {
                let (starts, _) = solved(&copy);
                for (s, e) in starts.iter().zip(&part.solved) {
                    assert!((s[0] - e[0]).abs() < 1e-9 && (s[1] - e[1]).abs() < 1e-9);
                }
            }

            let (first, second, b) = (
                id_of(&one["feature_id"]),
                id_of(&two["feature_id"]),
                id_of(&json!(base)),
            );
            let (cold, _) = measure_two(&copy, None);
            let cache = copy.with_extension("fcad-cache");
            let (miss, events) = measure_two(&copy, Some(&cache));
            assert!(
                events
                    .iter()
                    .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Miss),
                "{events:?}"
            );
            assert_eq!(outcomes(&events).len(), 3, "the plate and both Fillets");
            let (hit, events) = measure_two(&copy, Some(&cache));
            assert!(
                events
                    .iter()
                    .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit),
                "{events:?}"
            );
            cold.same_as(&miss);
            cold.same_as(&hit);
            let (c1, c2) = (part.corner(c1), part.corner(c2));
            check_two_at(
                &cold,
                part.rect,
                (first, c1, r1),
                (second, c2, r2),
                b,
                height,
            );
            let m = mesh(&copy, &copy.with_extension("stl"));
            check_mesh_rounded(&m, part.rect, &[(c1, r1), (c2, r2)], height);
            fbx(&copy, name);
            copy
        }

        /// Discovery on a two-Fillet plate written without a kernel, and the
        /// protocol: the base row editable with both Fillets in history order,
        /// the Sketch editors still refused, every other feature and every bad
        /// height refused before anything is written.
        #[test]
        fn height_discovery_and_protocol_without_native() {
            let f = filleted_without_kernel(CCW, [X0 + W, Y0], 2.375);
            let short = f.at([X0 + W, Y0 + D]).clone();
            let mut doc = Document::open(&f.source).expect("writable");
            let body: ferritecad_types::ObjectId = f.body_id().parse().expect("UUID");
            let e = &short["edge"];
            let fillet = ferritecad_document::EdgeFillet {
                edge: ferritecad_document::FilletEdge {
                    feature: id_of(&e["feature_id"]),
                    joint: ferritecad_types::ProfileJoint::new(
                        e["joint"][0].as_str().expect("a").parse().expect("UUID"),
                        e["joint"][1].as_str().expect("b").parse().expect("UUID"),
                    )
                    .expect("joint"),
                },
                radius_mm: 3.0625,
            };
            let prepared =
                ferritecad_document::prepare_edge_fillet(&doc, body, &fillet).expect("prepared");
            doc.write_edge_fillet(&prepared).expect("written");
            doc.close().expect("close");
            let h = Fixture {
                catalog: inspect(&f.source),
                root: tempfile::tempdir().expect("directory"),
                source: f.source.clone(),
                request: f.request.clone(),
            };
            let [one, two] = rows(&h);
            assert_eq!(h.catalog["edit_extrude"]["available"], true);
            let row = h.base_row();
            assert_eq!(row["editable"], true, "{row}");
            assert!(row["refusal"].is_null());
            assert_eq!(row["distance_mm"], H);
            for key in [
                "base_height_edit",
                "base_height_edit_v2",
                "base_height_edit_v3",
            ] {
                assert_eq!(
                    row[key],
                    Value::Null,
                    "{key}: Fillets are not a Cut history"
                );
            }
            assert_eq!(
                row["fillet_base"],
                json!({
                    "fillet_feature_id": one["feature_id"],
                    "body_id": f.body_id(),
                    "edge": one["edge"],
                    "corner_mm": [X0 + W, Y0],
                    "radius_mm": 2.375,
                    "profile_constrained": false,
                    "second_fillet": {
                        "fillet_feature_id": two["feature_id"],
                        "previous_feature_id": one["feature_id"],
                        "history_index": 2,
                        "edge": two["edge"],
                        "corner_mm": [X0 + W, Y0 + D],
                        "radius_mm": 3.0625,
                    },
                })
            );
            // §28J/§28K: the free plate's Sketch and its constraints read both
            // Fillets.
            let sketch = &h.catalog["sketches"][0];
            assert_eq!(sketch["editable"], true);
            assert_eq!(sketch["fillet_base"], row["fillet_base"]);
            assert_eq!(sketch["constraint_edit"]["available"], true);

            // The protocol: nothing written by any refusal. A build without a
            // kernel asks for it before any other check (§28C), so every
            // request is `unsupported` there; with a kernel each is its kind.
            let before = std::fs::read(&h.source).expect("bytes");
            let never = h.root.path().join("never.fcad");
            let names = entries(h.root.path());
            let base = h.base_id().to_owned();
            let sketch_id = sketch["sketch_id"].as_str().expect("UUID").to_owned();
            let foreign = ferritecad_types::ObjectId::new().to_string();
            let kernel = ferritecad_occt::is_available();
            let f1 = one["feature_id"].as_str().expect("F1").to_owned();
            let f2 = two["feature_id"].as_str().expect("F2").to_owned();
            for (why, feature, height, kind) in [
                ("zero", &base, "0", "input"),
                ("negative", &base, "-1", "input"),
                ("not a number", &base, "NaN", "input"),
                ("infinite", &base, "inf", "input"),
                ("Fillet 1", &f1, "9", "unsupported"),
                ("Fillet 2", &f2, "9", "unsupported"),
                ("the Sketch", &sketch_id, "9", "unsupported"),
                ("a foreign UUID", &foreign, "9", "input"),
            ] {
                let v = reply(
                    h.raise(&h.source, feature, height, &never)
                        .output()
                        .expect("process"),
                    EDIT,
                    2,
                );
                if kernel {
                    assert_eq!(refused(&v), kind, "{why}: {v}");
                } else {
                    assert_eq!(refused(&v), "unsupported", "{why}: {v}");
                    assert!(v.to_string().contains("Open CASCADE"), "{why}: {v}");
                }
            }
            if !kernel {
                let v = reply(
                    h.raise(&h.source, &base, "9", &never)
                        .output()
                        .expect("process"),
                    EDIT,
                    2,
                );
                assert_eq!(refused(&v), "unsupported", "no kernel: {v}");
            }
            assert_eq!(entries(h.root.path()), names, "a refusal left something");
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
        }

        /// Adjacent and opposite corners, both windings of the offset
        /// fractional plate, different radii; the height raised, lowered and
        /// kept, each copy measured in full.
        #[test]
        fn native_heights_under_both_fillets_are_what_the_numbers_say() {
            if !native() {
                return;
            }
            type Case = (
                &'static str,
                [[f64; 2]; 4],
                [f64; 2],
                [f64; 2],
                (f64, f64),
                Vec<f64>,
            );
            let cases: [Case; 3] = [
                (
                    "ccw-adjacent",
                    CCW,
                    [X0 + W, Y0],
                    [X0 + W, Y0 + D],
                    (2.375, 3.0625),
                    vec![11.5, 2.25],
                ),
                (
                    "cw-adjacent",
                    CW_FROM_UPPER_RIGHT,
                    [X0 + W, Y0],
                    [X0 + W, Y0 + D],
                    (4.8125, 1.1875),
                    vec![H, 3.375],
                ),
                (
                    "ccw-opposite",
                    CCW,
                    [X0, Y0],
                    [X0 + W, Y0 + D],
                    (2.0, 5.5),
                    vec![9.125],
                ),
            ];
            for (label, corners, first, second, (r1, r2), heights) in cases {
                let part = Part::plain(corners);
                let (_g, mut h) = rounded_twice(
                    &part,
                    corners,
                    (first, r1),
                    (second, r2),
                    &format!("h-{label}"),
                );
                let mut kept = Vec::new();
                for (k, height) in heights.into_iter().enumerate() {
                    let copy = raised_twice(
                        &h,
                        &part,
                        (first, r1),
                        (second, r2),
                        height,
                        &format!("height-{label}-{k}"),
                    );
                    let next = h.at_copy(&copy);
                    kept.push(h);
                    h = next;
                }
            }
        }

        /// A new height misses the plate and both Fillets on a warm cache and
        /// hits all three after; both radii are then edited on the raised
        /// copy, each measured at the new height.
        #[test]
        fn native_a_new_height_misses_the_whole_chain_and_radii_stay_editable() {
            if !native() {
                return;
            }
            let part = Part::plain(CCW);
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            let (_g, h) = rounded_twice(&part, CCW, (first, 2.375), (second, 3.0625), "h-cache");
            let warm = h.source.with_extension("fcad-cache");
            measure_two(&h.source, Some(&warm));
            let (_, events) = measure_two(&h.source, Some(&warm));
            use ferritecad_eval::CacheOutcome::{Hit, Miss};
            assert!(events.iter().all(|e| e.outcome == Hit), "{events:?}");
            let [one, two] = rows(&h);
            let (f1, f2, base) = (
                id_of(&one["feature_id"]),
                id_of(&two["feature_id"]),
                id_of(&one["edge"]["feature_id"]),
            );
            let copy = h.root.path().join("raised.fcad");
            reply(
                h.raise(&h.source, &base.to_string(), "10.5", &copy)
                    .output()
                    .expect("process"),
                EDIT,
                0,
            );
            // The copy keeps the document's identity, so the source's warm
            // entries are the ones a reader of the copy would find.
            let cache = copy.with_extension("fcad-cache");
            std::fs::copy(&warm, &cache).expect("the warm cache");
            let (warmed, events) = measure_two(&copy, Some(&cache));
            let by = outcomes(&events);
            for feature in [base, f1, f2] {
                assert_eq!(by.get(&feature), Some(&vec![Miss]), "{events:?}");
            }
            let (cold, _) = measure_two(&copy, None);
            cold.same_as(&warmed);
            check_two_at(
                &cold,
                part.rect,
                (f1, first, 2.375),
                (f2, second, 3.0625),
                base,
                10.5,
            );
            let (hit, events) = measure_two(&copy, Some(&cache));
            assert!(events.iter().all(|e| e.outcome == Hit), "{events:?}");
            cold.same_as(&hit);

            // Each radius on the raised plate, through the shipped command.
            let refs = stored_refs(&copy);
            let mut at = h.at_copy(&copy);
            let mut kept = Vec::new();
            let (mut r1, mut r2) = (2.375, 3.0625);
            for (feature, r, name) in [(f1, 4.25, "raised-f1"), (f2, 1.5, "raised-f2")] {
                let out = at.root.path().join(format!("{name}.fcad"));
                at.ask_radius(r);
                reply(
                    at.edit_from(&at.source, &feature.to_string(), at.version(), &out)
                        .output()
                        .expect("process"),
                    "edit-fillet-radius",
                    0,
                );
                if feature == f1 {
                    r1 = r;
                } else {
                    r2 = r;
                }
                assert_eq!(stored_refs(&out), refs);
                let next = at.at_copy(&out);
                assert_eq!(next.base_row()["distance_mm"], 10.5);
                let (cold, _) = measure_two(&out, None);
                check_two_at(
                    &cold,
                    part.rect,
                    (f1, first, r1),
                    (f2, second, r2),
                    base,
                    10.5,
                );
                let m = mesh(&out, &out.with_extension("stl"));
                check_mesh_rounded(&m, part.rect, &[(first, r1), (second, r2)], 10.5);
                // The copy lives in `at`'s directory; keep it for the next edit.
                kept.push(std::mem::replace(&mut at, next));
            }
        }

        /// A dimensioned plate whose stored and solved rectangles differ:
        /// the height changes only the base row, the stored Sketch stays the
        /// solver's starting guess, and both Fillets stand at the solved
        /// corners at the new height.
        #[test]
        fn native_dimensioned_height_keeps_the_stored_sketch_and_the_solved_plate() {
            if !solving() {
                return;
            }
            let at = [2.5, -1.75];
            let (width, depth) = (30.5, 8.0);
            let (_plain, g, expected) = dimensioned_plate(CCW, at, width, depth, "h-dimensioned");
            let (first, second) = ([X0 + W, Y0], [X0, Y0 + D]);
            let one = g.root.path().join("one.fcad");
            g.ask_stored(first, 3.5);
            reply(g.fillet(&one).output().expect("process"), OP, 0);
            let g1 = g.at_copy(&one);
            let part = Part {
                rect: rect_of(&expected),
                stored: g.stored_starts(),
                solved: expected.clone(),
                constrained: true,
            };
            assert_ne!(part.stored, part.solved, "stored and solved differ");
            let two = second_fillet(&g1, &part, first, 3.5, second, 2.0, "h-dimensioned-two");
            let h = g1.at_copy(&two);
            let up = raised_twice(
                &h,
                &part,
                (first, 3.5),
                (second, 2.0),
                13.25,
                "height-dimensioned",
            );
            raised_twice(
                &h.at_copy(&up),
                &part,
                (first, 3.5),
                (second, 2.0),
                4.5,
                "height-dimensioned-down",
            );
        }

        /// Refusals publish nothing and leave no scratch file: a height the
        /// kernel cannot round, the source as its own output, an occupied
        /// output, an alias of the source and a stale version. A cancelled job
        /// leaves nothing; a lost report keeps the copy, exit 7.
        #[test]
        fn native_height_refusals_cancellation_and_report_loss_are_atomic() {
            if !native() {
                return;
            }
            let part = Part::plain(CCW);
            let (first, second) = ([X0, Y0], [X0 + W, Y0]);
            let (_g, h) = rounded_twice(&part, CCW, (first, 2.0), (second, 1.5), "h-atomic");
            let base = h.base_id().to_owned();
            let before = std::fs::read(&h.source).expect("bytes");
            let taken = h.root.path().join("taken.fcad");
            std::fs::write(&taken, b"another process owns this").expect("occupied");
            let alias = h.root.path().join("alias.fcad");
            std::fs::hard_link(&h.source, &alias).expect("hard link");
            let names = entries(h.root.path());
            let never = h.root.path().join("never.fcad");
            let v = reply(
                h.raise(&h.source, &base, "0.000001", &never)
                    .output()
                    .expect("process"),
                EDIT,
                2,
            );
            assert_eq!(refused(&v), "kernel", "too thin to round: {v}");
            for (destination, why) in [
                (h.source.clone(), "the source is not its own output"),
                (taken.clone(), "an occupied output is not replaced"),
                (alias.clone(), "an alias of the source is the source"),
            ] {
                let v = reply(
                    h.raise(&h.source, &base, "9", &destination)
                        .output()
                        .expect("process"),
                    EDIT,
                    2,
                );
                assert_eq!(refused(&v), "input", "{why}: {v}");
            }
            assert_eq!(
                std::fs::read(&taken).expect("taken"),
                b"another process owns this"
            );
            let stale = h.root.path().join("stale-source.fcad");
            std::fs::copy(&h.source, &stale).expect("copy");
            let mut d = Document::open(&stale).expect("writable");
            let o = d.objects().expect("objects").remove(0);
            d.write(|w| {
                w.put_object(o.id, o.parent, o.ordinal, Some("renamed"), &o.payload)
                    .map(|_| ())
            })
            .expect("change");
            d.close().expect("close");
            let v = reply(
                h.raise(&stale, &base, "9", &never)
                    .output()
                    .expect("process"),
                EDIT,
                2,
            );
            assert_eq!(refused(&v), "input", "stale: {v}");
            std::fs::remove_file(&stale).expect("clean");
            assert_eq!(entries(h.root.path()), names, "a refusal left something");

            // Cancelled at the last barrier, through the shared job.
            let d = Document::open_read_only(&h.source).expect("source");
            let expected = ferritecad_document::DocumentVersion {
                document_id: d.meta().document_id,
                content: d.content_version().expect("version"),
            };
            d.close().expect("close");
            let destination = h.root.path().join("cancelled.fcad");
            let token = ferritecad_kernel::CancelToken::new();
            let stop = token.clone();
            let context = OperationContext::default()
                .with_cancel(token)
                .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                    if fraction >= 0.95 {
                        stop.cancel();
                    }
                }));
            let request = ferritecad_jobs::EditExtrudeRequest {
                source: h.source.clone(),
                expected,
                feature: id_of(&json!(base)),
                distance_mm: 9.0,
                destination: destination.clone(),
            };
            let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
            let result = ferritecad_jobs::edit_extrude_copy(&request, &mut kernel, &context);
            assert!(result.is_err(), "{result:?}");
            assert!(!destination.exists());
            assert_eq!(kernel.live_shape_count(), 0);
            assert_eq!(entries(h.root.path()), names, "cancellation left something");

            // A lost report: published, kept, exit 7.
            let lost = h.root.path().join("lost-report.fcad");
            assert_eq!(
                h.raise(&h.source, &base, "9", &lost)
                    .stdout(pipe::closed_pipe())
                    .stderr(pipe::closed_pipe())
                    .status()
                    .expect("pipes")
                    .code(),
                Some(7)
            );
            assert!(lost.exists());
            assert_eq!(h.at_copy(&lost).base_row()["distance_mm"], 9.0);
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
        }
    }

    /// §28J: moving and resizing the base rectangle under two Fillets.
    mod sketch {
        use super::radius::{edited_either, id_of, rounded_twice, rows};
        use super::*;
        use crate::sketch::{OP as EDIT, mapped, only_this_row_changed};

        /// The plate moved and resized to `rect`, corner for corner: each vertex
        /// of `template` (the plate as first drawn) goes to the same corner of
        /// `rect`, whatever the current plate is.
        fn starts(template: [[f64; 2]; 4], rect: [f64; 4]) -> Vec<[f64; 2]> {
            template.iter().map(|v| mapped(rect, *v)).collect()
        }

        fn part_at(template: [[f64; 2]; 4], rect: [f64; 4]) -> Part {
            let at = starts(template, rect);
            Part {
                rect,
                stored: at.clone(),
                solved: at,
                constrained: false,
            }
        }

        /// One Sketch edit of `h`'s base rectangle to `rect`, published by the
        /// shipped `edit-sketch-copy` and measured: discovery naming both
        /// Fillets in history order, the exact allowlist (the Sketch row alone),
        /// every name kept, both Fillet rows unchanged with their corners moved,
        /// validation, the B-Rep cold and through a real Miss and Hit over the
        /// whole chain (both cylinders under their own names at their new axes,
        /// Fillet 1's carried into the final Body, the analytic volume), the
        /// independent mesh and the FBX.
        #[allow(clippy::too_many_arguments)]
        pub(super) fn moved_twice(
            h: &Fixture,
            template: [[f64; 2]; 4],
            (c1, r1): ([f64; 2], f64),
            (c2, r2): ([f64; 2], f64),
            rect: [f64; 4],
            height: f64,
            name: &str,
        ) -> PathBuf {
            let before = std::fs::read(&h.source).expect("source bytes");
            let refs = stored_refs(&h.source);
            let [one, two] = rows(h);
            let row = h.sketch_row().clone();
            assert_eq!(row["editable"], true, "{row}");
            assert!(row["refusal"].is_null(), "{row}");
            let context = &row["fillet_base"];
            assert_eq!(context["fillet_feature_id"], one["feature_id"]);
            assert_eq!(context["radius_mm"], r1);
            let later = &context["second_fillet"];
            assert_eq!(later["fillet_feature_id"], two["feature_id"]);
            assert_eq!(
                later["previous_feature_id"], one["feature_id"],
                "Fillet 2 rounds Fillet 1's result, not the base"
            );
            assert_eq!(later["history_index"], 2);
            assert_eq!(later["edge"], two["edge"]);
            assert_eq!(later["radius_mm"], r2);

            let copy = h.root.path().join(format!("{name}.fcad"));
            h.ask_starts(&starts(template, rect));
            let v = reply(
                h.redraw(&h.source, h.version(), &copy)
                    .output()
                    .expect("process"),
                EDIT,
                0,
            );
            assert_eq!(v["result"]["sketch_id"], h.sketch_id(), "{v}");
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before, "source");
            // The Sketch row's payload and hash; the coordinate writer does not
            // stamp modified_at.
            assert_eq!(only_this_row_changed(&h.source, &copy, h.sketch_id()), 2);
            assert_eq!(stored_refs(&copy), refs, "every name and its UUID kept");
            let (m1, m2) = (mapped(rect, c1), mapped(rect, c2));
            let after = h.at_copy(&copy);
            assert_eq!(after.base_row()["distance_mm"], height, "the height");
            let [a1, a2] = rows(&after);
            for (was, now, corner, r) in [(&one, &a1, m1, r1), (&two, &a2, m2, r2)] {
                for key in ["feature_id", "edge", "previous_feature_id", "history_index"] {
                    assert_eq!(now[key], was[key], "{key}");
                }
                assert_eq!(now["radius_mm"], r, "the same radius");
                assert_eq!(now["corner_mm"], json!(corner), "the same corner, moved");
            }
            let moved = &after.sketch_row()["fillet_base"];
            assert_eq!(moved["corner_mm"], json!(m1));
            assert_eq!(moved["second_fillet"]["corner_mm"], json!(m2));
            assert_eq!(after.catalog["edit_extrude"]["available"], true);
            assert_eq!(
                after.sketch_row()["constraint_edit"]["available"],
                true,
                "the constraint editor reads both Fillets"
            );

            let checked = cli()
                .arg("validate")
                .arg(&copy)
                .arg("--json")
                .output()
                .expect("validate");
            assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
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

            let (first, second, base) = (
                id_of(&one["feature_id"]),
                id_of(&two["feature_id"]),
                id_of(&one["edge"]["feature_id"]),
            );
            let (cold, _) = measure_two(&copy, None);
            let cache = copy.with_extension("fcad-cache");
            let (miss, events) = measure_two(&copy, Some(&cache));
            assert!(
                events
                    .iter()
                    .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Miss),
                "{events:?}"
            );
            assert_eq!(outcomes(&events).len(), 3, "the plate and both Fillets");
            let (hit, events) = measure_two(&copy, Some(&cache));
            assert!(
                events
                    .iter()
                    .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit),
                "{events:?}"
            );
            cold.same_as(&miss);
            cold.same_as(&hit);
            check_two_at(&cold, rect, (first, m1, r1), (second, m2, r2), base, height);
            let m = mesh(&copy, &copy.with_extension("stl"));
            check_mesh_rounded(&m, rect, &[(m1, r1), (m2, r2)], height);
            fbx(&copy, name);
            copy
        }

        /// A two-Fillet plate written without a kernel, by the shipped
        /// preparation and writer, with its own directory.
        pub(super) fn twice_without_kernel(r1: f64, r2: f64) -> (Fixture, Fixture) {
            let f = filleted_without_kernel(CCW, [X0 + W, Y0], r1);
            let short = f.at([X0 + W, Y0 + D]).clone();
            let mut doc = Document::open(&f.source).expect("writable");
            let body: ferritecad_types::ObjectId = f.body_id().parse().expect("UUID");
            let e = &short["edge"];
            let fillet = ferritecad_document::EdgeFillet {
                edge: ferritecad_document::FilletEdge {
                    feature: id_of(&e["feature_id"]),
                    joint: ferritecad_types::ProfileJoint::new(
                        e["joint"][0].as_str().expect("a").parse().expect("UUID"),
                        e["joint"][1].as_str().expect("b").parse().expect("UUID"),
                    )
                    .expect("joint"),
                },
                radius_mm: r2,
            };
            let prepared =
                ferritecad_document::prepare_edge_fillet(&doc, body, &fillet).expect("prepared");
            doc.write_edge_fillet(&prepared).expect("written");
            doc.close().expect("close");
            let h = Fixture {
                catalog: inspect(&f.source),
                root: tempfile::tempdir().expect("directory"),
                source: f.source.clone(),
                request: f.request.clone(),
            };
            (f, h)
        }

        /// Discovery on a two-Fillet plate written without a kernel: the base
        /// Sketch editable with both Fillets in history order, the height
        /// editor's reading of them unchanged, and the protocol with the stub
        /// build's real order of checks — with a kernel each bad request is its
        /// own kind; without one the kernel is asked for first — nothing
        /// written either way.
        #[test]
        fn sketch_discovery_and_protocol_without_native() {
            let (_keep, h) = twice_without_kernel(2.375, 3.0625);
            let [one, two] = rows(&h);
            let row = h.sketch_row();
            assert_eq!(row["editable"], true, "{row}");
            assert!(row["refusal"].is_null());
            assert_eq!(
                row["fillet_base"],
                json!({
                    "fillet_feature_id": one["feature_id"],
                    "body_id": h.body_id(),
                    "edge": one["edge"],
                    "corner_mm": [X0 + W, Y0],
                    "radius_mm": 2.375,
                    "profile_constrained": false,
                    "second_fillet": {
                        "fillet_feature_id": two["feature_id"],
                        "previous_feature_id": one["feature_id"],
                        "history_index": 2,
                        "edge": two["edge"],
                        "corner_mm": [X0 + W, Y0 + D],
                        "radius_mm": 3.0625,
                    },
                })
            );
            assert_eq!(row["vertices"].as_array().map(Vec::len), Some(4));
            assert_eq!(row["constraint_edit"]["available"], true);
            assert_eq!(h.base_row()["fillet_base"], row["fillet_base"]);

            let before = std::fs::read(&h.source).expect("bytes");
            let never = h.root.path().join("never.fcad");
            let names = entries(h.root.path());
            let kernel = ferritecad_occt::is_available();
            let mut swapped = starts(CCW, [X0, Y0, W, D]);
            swapped.rotate_left(1);
            for (why, at, kind) in [
                ("Lines that swap their sides", swapped, "input"),
                ("under 2 r2 deep", starts(CCW, [X0, Y0, W, 6.0]), "input"),
                (
                    "no longer a rectangle",
                    vec![[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0 + 1., Y0 + D]],
                    "unsupported",
                ),
            ] {
                h.ask_starts(&at);
                let v = reply(
                    h.redraw(&h.source, h.version(), &never)
                        .output()
                        .expect("process"),
                    EDIT,
                    2,
                );
                if kernel {
                    assert_eq!(refused(&v), kind, "{why}: {v}");
                } else {
                    assert_eq!(refused(&v), "unsupported", "{why}: {v}");
                    assert!(v.to_string().contains("Open CASCADE"), "{why}: {v}");
                }
            }
            assert_eq!(entries(h.root.path()), names, "a refusal left something");
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
        }

        /// Adjacent and opposite corners, both windings of the offset
        /// fractional plate, different radii: moved and grown, and shrunk to
        /// the narrowest side the radii allow, each copy measured in full.
        #[test]
        fn native_moving_and_resizing_under_two_fillets_are_what_the_numbers_say() {
            if !native() {
                return;
            }
            type Case = (
                &'static str,
                [[f64; 2]; 4],
                [f64; 2],
                [f64; 2],
                (f64, f64),
                Vec<[f64; 4]>,
            );
            let cases: [Case; 3] = [
                (
                    "ccw-adjacent",
                    CCW,
                    [X0 + W, Y0],
                    [X0 + W, Y0 + D],
                    (2.375, 3.0625),
                    // 2 r2 = 6.125 is the narrowest depth both radii fit.
                    vec![[-1.25, 0.5, 41.75, 13.5], [2.0, -3.5, 20.25, 6.125]],
                ),
                (
                    "cw-adjacent",
                    CW_FROM_UPPER_RIGHT,
                    [X0 + W, Y0],
                    [X0 + W, Y0 + D],
                    (4.8125, 1.1875),
                    // 2 r1 = 9.625.
                    vec![[0.75, -2.25, 24.5, 9.625]],
                ),
                (
                    "ccw-opposite",
                    CCW,
                    [X0, Y0],
                    [X0 + W, Y0 + D],
                    (2.0, 5.5),
                    // 2 r2 = 11: a square is the smallest.
                    vec![[-10., -6.5, 60.25, 20.5], [1.0, 2.0, 11.0, 11.0]],
                ),
            ];
            for (label, corners, first, second, (r1, r2), rects) in cases {
                let part = Part::plain(corners);
                let (_g, mut h) = rounded_twice(
                    &part,
                    corners,
                    (first, r1),
                    (second, r2),
                    &format!("s-{label}"),
                );
                let mut kept = Vec::new();
                for (k, rect) in rects.into_iter().enumerate() {
                    let copy = moved_twice(
                        &h,
                        corners,
                        (first, r1),
                        (second, r2),
                        rect,
                        H,
                        &format!("sketch-{label}-{k}"),
                    );
                    let next = h.at_copy(&copy);
                    kept.push(h);
                    h = next;
                }
            }
        }

        /// A moved rectangle misses the plate and both Fillets on a warm cache
        /// and hits all three after; then the second radius, and the height,
        /// are edited on the moved copy, each measured against the new plate.
        #[test]
        fn native_a_moved_rectangle_misses_the_whole_chain_and_edits_interleave() {
            if !native() {
                return;
            }
            let part = Part::plain(CCW);
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            let (_g, h) = rounded_twice(&part, CCW, (first, 2.375), (second, 3.0625), "s-cache");
            let warm = h.source.with_extension("fcad-cache");
            measure_two(&h.source, Some(&warm));
            let (_, events) = measure_two(&h.source, Some(&warm));
            use ferritecad_eval::CacheOutcome::{Hit, Miss};
            assert!(events.iter().all(|e| e.outcome == Hit), "{events:?}");
            let [one, two] = rows(&h);
            let (f1, f2, base) = (
                id_of(&one["feature_id"]),
                id_of(&two["feature_id"]),
                id_of(&one["edge"]["feature_id"]),
            );
            let rect = [1.5, -2.0, 30.75, 9.5];
            let copy = h.root.path().join("moved.fcad");
            h.ask_starts(&starts(CCW, rect));
            reply(
                h.redraw(&h.source, h.version(), &copy)
                    .output()
                    .expect("process"),
                EDIT,
                0,
            );
            // The copy keeps the document's identity, so the source's warm
            // entries are the ones a reader of the copy would find.
            let cache = copy.with_extension("fcad-cache");
            std::fs::copy(&warm, &cache).expect("the warm cache");
            let (warmed, events) = measure_two(&copy, Some(&cache));
            let by = outcomes(&events);
            for feature in [base, f1, f2] {
                assert_eq!(by.get(&feature), Some(&vec![Miss]), "{events:?}");
            }
            let (m1, m2) = (mapped(rect, first), mapped(rect, second));
            let (cold, _) = measure_two(&copy, None);
            cold.same_as(&warmed);
            check_two_at(&cold, rect, (f1, m1, 2.375), (f2, m2, 3.0625), base, H);
            let (hit, events) = measure_two(&copy, Some(&cache));
            assert!(events.iter().all(|e| e.outcome == Hit), "{events:?}");
            cold.same_as(&hit);

            // The second radius, then the height, on the moved plate.
            let moved = part_at(CCW, rect);
            let at = h.at_copy(&copy);
            let radius = edited_either(&at, &moved, 2, (m1, 2.375), (m2, 3.0625), 1.5, "s-radius");
            let after = at.at_copy(&radius);
            let raised = super::height::raised_twice(
                &after,
                &moved,
                (m1, 2.375),
                (m2, 1.5),
                12.0,
                "s-height",
            );
            assert!(raised.exists());
        }

        /// The flat between adjacent arcs is the predicate itself: the least
        /// depth that leaves it publishes and measures; one float below is
        /// refused with the numbers, publishing nothing. Opposite corners owe
        /// no flat: each radius alone bounds them, at exactly 2 r.
        #[test]
        fn native_the_shared_flat_is_exact_and_opposite_corners_owe_none() {
            if !native() {
                return;
            }
            let part = Part::plain(CCW);
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            let (_g, h) = rounded_twice(&part, CCW, (first, 3.0), (second, 3.0), "s-pair");
            let mut least = 6.01_f64;
            while 3.0 > ferritecad_document::pair_bound(least, 3.0) {
                least = least.next_up();
            }
            let never = h.root.path().join("never.fcad");
            let before = std::fs::read(&h.source).expect("bytes");
            let names = entries(h.root.path());
            for depth in [least.next_down(), 6.005] {
                h.ask_starts(&starts(CCW, [-1., 0., 25., depth]));
                let v = reply(
                    h.redraw(&h.source, h.version(), &never)
                        .output()
                        .expect("process"),
                    EDIT,
                    2,
                );
                assert_eq!(refused(&v), "input", "{depth}: {v}");
                assert!(v.to_string().contains("flat"), "{depth}: {v}");
            }
            assert_eq!(entries(h.root.path()), names);
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
            let copy = moved_twice(
                &h,
                CCW,
                (first, 3.0),
                (second, 3.0),
                [-1., 0., 25., least],
                H,
                "s-pair-least",
            );
            assert!(copy.exists());

            // Opposite corners: a 6 mm square-ish plate holds both r = 3.
            let (opposite, past) = ([X0, Y0], [X0 + W, Y0 + D]);
            let (_g, h) = rounded_twice(&part, CCW, (opposite, 3.0), (past, 3.0), "s-opposite");
            h.ask_starts(&starts(CCW, [-1., 0., 25., 5.999]));
            let v = reply(
                h.redraw(&h.source, h.version(), &never)
                    .output()
                    .expect("process"),
                EDIT,
                2,
            );
            assert_eq!(refused(&v), "input", "{v}");
            assert!(v.to_string().contains("too large"), "{v}");
            let copy = moved_twice(
                &h,
                CCW,
                (opposite, 3.0),
                (past, 3.0),
                [-1., 0., 25., 6.0],
                H,
                "s-opposite-least",
            );
            assert!(copy.exists());
        }

        /// Refusals publish nothing and leave no scratch file: a side swap, a
        /// non-rectangle, each Fillet's own bound naming that Fillet, a repeated
        /// or foreign curve, a Sketch that is not the plate's, the source as its
        /// own output, an occupied output, an alias and a stale version. A
        /// cancelled job leaves nothing; a lost report keeps the copy, exit 7.
        #[test]
        fn native_sketch_refusals_cancellation_and_report_loss_are_atomic() {
            if !native() {
                return;
            }
            let part = Part::plain(CW_FROM_UPPER_RIGHT);
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            // r1 is the wider Fillet, so its bound (2 r1 = 9.625) fails first.
            let (_g, h) = rounded_twice(
                &part,
                CW_FROM_UPPER_RIGHT,
                (first, 4.8125),
                (second, 1.1875),
                "s-atomic",
            );
            let [one, two] = rows(&h);
            let before = std::fs::read(&h.source).expect("bytes");
            let taken = h.root.path().join("taken.fcad");
            std::fs::write(&taken, b"another process owns this").expect("occupied");
            let alias = h.root.path().join("alias.fcad");
            std::fs::hard_link(&h.source, &alias).expect("hard link");
            let names = entries(h.root.path());
            let never = h.root.path().join("never.fcad");
            let template = CW_FROM_UPPER_RIGHT;
            let mut swapped = starts(template, [X0, Y0, W, D]);
            swapped.rotate_left(1);
            let skew = {
                let mut v = starts(template, [X0, Y0, W, D]);
                v[0][0] += 1.;
                v
            };
            for (why, at, kind, culprit) in [
                ("Lines swap their sides", swapped, "input", None),
                ("not a rectangle", skew, "unsupported", None),
                (
                    "under 2 r1: names Fillet 1",
                    starts(template, [X0, Y0, W, 9.5]),
                    "input",
                    Some(&one["edge"]["joint"]),
                ),
            ] {
                h.ask_starts(&at);
                let v = reply(
                    h.redraw(&h.source, h.version(), &never)
                        .output()
                        .expect("process"),
                    EDIT,
                    2,
                );
                assert_eq!(refused(&v), kind, "{why}: {v}");
                if let Some(joint) = culprit {
                    for line in joint.as_array().expect("joint") {
                        assert!(v.to_string().contains(line.as_str().expect("UUID")), "{v}");
                    }
                    // Fillet 2's own joint is not named: r2 = 1.1875 fits.
                    let other = two["edge"]["joint"].as_array().expect("joint");
                    assert!(
                        other
                            .iter()
                            .any(|l| !v.to_string().contains(l.as_str().expect("UUID"))),
                        "{v}"
                    );
                }
            }
            // A repeated curve, a foreign one and a short request.
            let good = starts(template, [X0, Y0, W, D]);
            let vertices = h.sketch_row()["vertices"]
                .as_array()
                .expect("vertices")
                .clone();
            let foreign = ferritecad_types::StableEntityId::new().to_string();
            for (why, request) in [
                (
                    "a repeated curve",
                    json!({"request_version": 1, "vertices": [
                        {"curve_id": vertices[0]["curve_id"], "start_mm": good[0]},
                        {"curve_id": vertices[0]["curve_id"], "start_mm": good[1]},
                        {"curve_id": vertices[2]["curve_id"], "start_mm": good[2]},
                        {"curve_id": vertices[3]["curve_id"], "start_mm": good[3]},
                    ]}),
                ),
                (
                    "a foreign curve",
                    json!({"request_version": 1, "vertices": [
                        {"curve_id": foreign, "start_mm": good[0]},
                        {"curve_id": vertices[1]["curve_id"], "start_mm": good[1]},
                        {"curve_id": vertices[2]["curve_id"], "start_mm": good[2]},
                        {"curve_id": vertices[3]["curve_id"], "start_mm": good[3]},
                    ]}),
                ),
                (
                    "three vertices",
                    json!({"request_version": 1, "vertices": [
                        {"curve_id": vertices[0]["curve_id"], "start_mm": good[0]},
                        {"curve_id": vertices[1]["curve_id"], "start_mm": good[1]},
                        {"curve_id": vertices[2]["curve_id"], "start_mm": good[2]},
                    ]}),
                ),
            ] {
                write(&h.request, &request);
                let v = reply(
                    h.redraw(&h.source, h.version(), &never)
                        .output()
                        .expect("process"),
                    EDIT,
                    2,
                );
                assert_eq!(refused(&v), "input", "{why}: {v}");
            }
            // Not the plate's Sketch: a Fillet and the Extrude.
            h.ask_starts(&good);
            for other in [
                one["feature_id"].clone(),
                h.base_row()["feature_id"].clone(),
            ] {
                let mut c = cli();
                c.arg(EDIT)
                    .arg(&h.source)
                    .arg("--sketch")
                    .arg(other.as_str().expect("UUID"))
                    .arg("--expect-version")
                    .arg(h.version())
                    .arg("--request")
                    .arg(&h.request)
                    .arg("-o")
                    .arg(&never)
                    .arg("--json");
                let v = reply(c.output().expect("process"), EDIT, 2);
                assert!(
                    ["input", "unsupported"].contains(&refused(&v)),
                    "not a Sketch: {v}"
                );
            }
            for (destination, why) in [
                (h.source.clone(), "the source is not its own output"),
                (taken.clone(), "an occupied output is not replaced"),
                (alias.clone(), "an alias of the source is the source"),
            ] {
                let v = reply(
                    h.redraw(&h.source, h.version(), &destination)
                        .output()
                        .expect("process"),
                    EDIT,
                    2,
                );
                assert_eq!(refused(&v), "input", "{why}: {v}");
            }
            assert_eq!(
                std::fs::read(&taken).expect("taken"),
                b"another process owns this"
            );
            let stale = h.root.path().join("stale-source.fcad");
            std::fs::copy(&h.source, &stale).expect("copy");
            let mut d = Document::open(&stale).expect("writable");
            let o = d.objects().expect("objects").remove(0);
            d.write(|w| {
                w.put_object(o.id, o.parent, o.ordinal, Some("renamed"), &o.payload)
                    .map(|_| ())
            })
            .expect("change");
            d.close().expect("close");
            let v = reply(
                h.redraw(&stale, h.version(), &never)
                    .output()
                    .expect("process"),
                EDIT,
                2,
            );
            assert_eq!(refused(&v), "input", "stale: {v}");
            std::fs::remove_file(&stale).expect("clean");
            assert_eq!(entries(h.root.path()), names, "a refusal left something");

            // Cancelled at the last barrier, through the shared job.
            let d = Document::open_read_only(&h.source).expect("source");
            let expected = ferritecad_document::DocumentVersion {
                document_id: d.meta().document_id,
                content: d.content_version().expect("version"),
            };
            d.close().expect("close");
            let destination = h.root.path().join("cancelled.fcad");
            let token = ferritecad_kernel::CancelToken::new();
            let stop = token.clone();
            let context = OperationContext::default()
                .with_cancel(token)
                .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                    if fraction >= 0.95 {
                        stop.cancel();
                    }
                }));
            let request = ferritecad_jobs::EditSketchRequest {
                source: h.source.clone(),
                expected,
                sketch: id_of(&json!(h.sketch_id())),
                vertices: vertices
                    .iter()
                    .zip(&starts(template, [X0 - 2., Y0 - 2., W + 3., D + 3.]))
                    .map(|(v, at)| ferritecad_document::SketchVertex {
                        curve_id: v["curve_id"].as_str().expect("UUID").parse().expect("UUID"),
                        start_mm: *at,
                    })
                    .collect(),
                destination: destination.clone(),
            };
            let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
            let result = ferritecad_jobs::edit_sketch_copy(&request, &mut kernel, &context);
            assert!(result.is_err(), "{result:?}");
            assert!(!destination.exists());
            assert_eq!(kernel.live_shape_count(), 0);
            assert_eq!(entries(h.root.path()), names, "cancellation left something");

            // A lost report: published, kept, exit 7.
            let lost = h.root.path().join("lost-report.fcad");
            h.ask_starts(&starts(template, [X0 - 2., Y0 - 2., W + 3., D + 3.]));
            assert_eq!(
                h.redraw(&h.source, h.version(), &lost)
                    .stdout(pipe::closed_pipe())
                    .stderr(pipe::closed_pipe())
                    .status()
                    .expect("pipes")
                    .code(),
                Some(7)
            );
            assert!(lost.exists());
            assert_eq!(
                h.at_copy(&lost).sketch_row()["fillet_base"]["corner_mm"],
                json!(mapped([X0 - 2., Y0 - 2., W + 3., D + 3.], first))
            );
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
        }

        /// A dimensioned plate rounded twice keeps refusing coordinate edits,
        /// by name, publishing nothing; a plate that keeps only the Coincident
        /// closure links §28E leaves is edited, its links kept byte for byte
        /// and the solve agreeing with the stored Lines.
        #[test]
        fn native_constraints_stay_refused_and_closure_links_are_kept() {
            if !solving() {
                return;
            }
            // Dimensioned: refused, nothing published.
            let at = [2.5, -1.75];
            let (width, depth) = (30.5, 8.0);
            let (_plain, g, expected) = dimensioned_plate(CCW, at, width, depth, "s-dimensioned");
            let (first, second) = ([X0 + W, Y0], [X0, Y0 + D]);
            let one = g.root.path().join("one.fcad");
            g.ask_stored(first, 3.5);
            reply(g.fillet(&one).output().expect("process"), OP, 0);
            let g1 = g.at_copy(&one);
            let part = Part {
                rect: rect_of(&expected),
                stored: g.stored_starts(),
                solved: expected.clone(),
                constrained: true,
            };
            let two = second_fillet(&g1, &part, first, 3.5, second, 2.0, "s-dimensioned-two");
            let h = g1.at_copy(&two);
            let row = h.sketch_row();
            assert_eq!(row["editable"], false, "{row}");
            assert!(
                row["refusal"]
                    .as_str()
                    .is_some_and(|r| r.contains("constraints")),
                "{row}"
            );
            let never = h.root.path().join("never.fcad");
            let names = entries(h.root.path());
            let before = std::fs::read(&h.source).expect("bytes");
            let request = json!({"request_version": 1, "vertices": g
                .stored_starts()
                .iter()
                .enumerate()
                .map(|(i, p)| json!({"curve_id": g.line(i), "start_mm": p}))
                .collect::<Vec<_>>()});
            write(&h.request, &request);
            let v = reply(
                h.redraw(&h.source, h.version(), &never)
                    .output()
                    .expect("process"),
                EDIT,
                2,
            );
            assert_eq!(refused(&v), "unsupported", "{v}");
            assert_eq!(entries(h.root.path()), names);
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);

            // Closure links only: forged into a plain two-Fillet copy.
            let part = Part::plain(CCW);
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            let (_g, plain) =
                rounded_twice(&part, CCW, (first, 2.375), (second, 3.0625), "s-links");
            let linked = plain.root.path().join("linked.fcad");
            std::fs::copy(&plain.source, &linked).expect("copy");
            let mut d = Document::open(&linked).expect("writable");
            let object = d
                .object(id_of(&json!(plain.sketch_id())))
                .expect("read")
                .expect("Sketch");
            let ObjectPayload::Sketch(mut sketch) = object.payload.clone() else {
                panic!("a Sketch")
            };
            let lines: Vec<_> = sketch.curves.iter().map(|c| c.id).collect();
            sketch.constraints = (0..4)
                .map(|i| ferritecad_document::SketchConstraint {
                    id: ferritecad_types::StableEntityId::new(),
                    rule: ferritecad_document::SketchConstraintRule::Coincident {
                        a: ferritecad_document::SketchPointRef::new(
                            lines[i],
                            ferritecad_document::SketchPointSelector::End,
                        ),
                        b: ferritecad_document::SketchPointRef::new(
                            lines[(i + 1) % 4],
                            ferritecad_document::SketchPointSelector::Start,
                        ),
                    },
                })
                .collect();
            let links = sketch.constraints.clone();
            d.write(|w| {
                w.put_object(
                    object.id,
                    object.parent,
                    object.ordinal,
                    object.name.as_deref(),
                    &ObjectPayload::Sketch(sketch),
                )
                .map(|_| ())
            })
            .expect("closure links");
            d.close().expect("close");
            let h = plain.at_copy(&linked);
            assert_eq!(h.sketch_row()["editable"], true, "{}", h.sketch_row());
            let rect = [-2.5, 1.25, 31.0, 10.0];
            let copy = moved_twice(
                &h,
                CCW,
                (first, 2.375),
                (second, 3.0625),
                rect,
                H,
                "s-links-moved",
            );
            let reopened = Document::open_read_only(&copy).expect("reopen");
            let ObjectPayload::Sketch(now) = reopened
                .object(id_of(&json!(h.sketch_id())))
                .expect("read")
                .expect("Sketch")
                .payload
            else {
                panic!("a Sketch")
            };
            assert_eq!(now.constraints, links, "the links, byte for byte");
            let (solved_starts, _) = solved(&copy);
            for (s, e) in solved_starts.iter().zip(starts(CCW, rect)) {
                assert!((s[0] - e[0]).abs() < 1e-9 && (s[1] - e[1]).abs() < 1e-9);
            }
        }
    }

    /// §28K: the constraints of the base Sketch under two Fillets.
    mod constraints {
        use super::radius::{edited_either, id_of, rounded_twice, rows};
        use super::sketch::twice_without_kernel;
        use super::*;
        use crate::constraints::{
            dimensioned, horizontal, length, only_this_sketch_changed, pin, rule, solved_vertex,
        };

        const EDIT: &str = "edit-sketch-constraints-copy";

        /// Every saved constraint of the base Sketch, in the catalogue's order.
        fn listed(f: &Fixture) -> Vec<Value> {
            f.constraint_row()["constraints"]
                .as_array()
                .expect("constraints")
                .clone()
        }

        /// The UUIDs of the user's constraints: everything but the closure links.
        fn user_constraints(f: &Fixture) -> Vec<Value> {
            listed(f)
                .iter()
                .filter(|c| c["rule"]["kind"] != "coincident")
                .map(|c| c["constraint_id"].clone())
                .collect()
        }

        /// The solved plate of the stored rectangle of `f` moved so its first
        /// Line starts at `at`, `width` along X and `depth` along Y.
        fn plate_of(f: &Fixture, at: [f64; 2], width: f64, depth: f64) -> Vec<[f64; 2]> {
            let starts = f.stored_starts();
            starts
                .iter()
                .map(|v| solved_vertex(&starts, *v, at, width, depth))
                .collect()
        }

        /// One constraint edit of `h`'s base Sketch, published by the shipped
        /// command and measured against the plate the solver must make:
        /// discovery naming both Fillets in history order with their stored
        /// corners, the exact allowlist, every name and Line UUID kept, both
        /// Fillet rows unchanged, the stored coordinates still the solver's
        /// guess, the solve read in-process from a cold rebuild, validation,
        /// the B-Rep cold and through a real Miss and Hit over the plate and
        /// both Fillets (both cylinders under their own names on their solved
        /// axes, the analytic volume), the independent mesh and the FBX.
        #[allow(clippy::too_many_arguments)]
        pub(super) fn constrained_twice(
            h: &Fixture,
            (remove, add): (&[Value], &[Value]),
            (c1, r1): ([f64; 2], f64),
            (c2, r2): ([f64; 2], f64),
            expected: &[[f64; 2]],
            dof: usize,
            name: &str,
        ) -> PathBuf {
            let before = std::fs::read(&h.source).expect("source bytes");
            let refs = stored_refs(&h.source);
            let stored = h.stored_starts();
            let [one, two] = rows(h);
            let context = h.constraint_row()["fillet_base"].clone();
            assert_eq!(context["fillet_feature_id"], one["feature_id"], "{context}");
            assert_eq!(context["stored_corner_mm"], json!(c1));
            assert_eq!(context["radius_mm"], r1);
            let later = &context["second_fillet"];
            assert_eq!(later["fillet_feature_id"], two["feature_id"]);
            assert_eq!(
                later["previous_feature_id"], one["feature_id"],
                "Fillet 2 rounds Fillet 1's result, not the base"
            );
            assert_eq!(later["history_index"], 2);
            assert_eq!(later["edge"], two["edge"]);
            assert_eq!(later["stored_corner_mm"], json!(c2));
            assert_eq!(later["radius_mm"], r2);

            let copy = h.root.path().join(format!("{name}.fcad"));
            h.ask_constraints(remove, add);
            let v = reply(h.constrain(&copy).output().expect("process"), EDIT, 0);
            assert_eq!(v["result"]["sketch_id"], h.plate_sketch_id(), "{v}");
            assert_eq!(v["result"]["solve"]["degrees_of_freedom"], dof, "{v}");
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before, "source");
            only_this_sketch_changed(&h.source, &copy, h.plate_sketch_id());
            assert_eq!(stored_refs(&copy), refs, "every name and its UUID kept");

            let g = h.at_copy(&copy);
            assert_eq!(g.stored_starts(), stored, "stored coordinates are kept");
            for i in 0..4 {
                assert_eq!(g.line(i), h.line(i), "Line {i}");
            }
            let [a1, a2] = rows(&g);
            for key in [
                "feature_id",
                "edge",
                "previous_feature_id",
                "history_index",
                "radius_mm",
            ] {
                assert_eq!(a1[key], one[key], "{key}");
                assert_eq!(a2[key], two[key], "{key}");
            }
            assert_eq!(a1["profile_constrained"], true);
            assert_eq!(a2["profile_constrained"], true);
            assert_eq!(g.catalog["edit_extrude"]["available"], true);
            let after = &g.constraint_row()["fillet_base"];
            assert_eq!(after["fillet_feature_id"], context["fillet_feature_id"]);
            assert_eq!(after["stored_corner_mm"], json!(c1), "the stored corner");
            assert_eq!(after["second_fillet"]["stored_corner_mm"], json!(c2));

            let (starts, measured) = solved(&copy);
            assert_eq!(measured, dof, "DOF of the cold rebuild");
            for (s, e) in starts.iter().zip(expected) {
                assert!(
                    (s[0] - e[0]).abs() < 1e-9 && (s[1] - e[1]).abs() < 1e-9,
                    "{starts:?} is not {expected:?}"
                );
            }
            let checked = cli()
                .arg("validate")
                .arg(&copy)
                .arg("--json")
                .output()
                .expect("validate");
            assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
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

            let (first, second, base) = (
                id_of(&one["feature_id"]),
                id_of(&two["feature_id"]),
                id_of(&one["edge"]["feature_id"]),
            );
            let (cold, _) = measure_two(&copy, None);
            let cache = copy.with_extension("fcad-cache");
            let (miss, events) = measure_two(&copy, Some(&cache));
            assert!(
                events
                    .iter()
                    .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Miss),
                "{events:?}"
            );
            assert_eq!(outcomes(&events).len(), 3, "the plate and both Fillets");
            let (hit, events) = measure_two(&copy, Some(&cache));
            assert!(
                events
                    .iter()
                    .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit),
                "{events:?}"
            );
            cold.same_as(&miss);
            cold.same_as(&hit);
            let rect = rect_of(expected);
            let at = |c: [f64; 2]| expected[stored.iter().position(|v| *v == c).expect("corner")];
            let (s1, s2) = (at(c1), at(c2));
            check_two_at(&cold, rect, (first, s1, r1), (second, s2, r2), base, H);
            let m = mesh(&copy, &copy.with_extension("stl"));
            check_mesh_rounded(&m, rect, &[(s1, r1), (s2, r2)], H);
            fbx(&copy, name);
            copy
        }

        /// A refusal publishes nothing, leaves no scratch file and keeps the
        /// source; it returns the reply for the caller to read its reasons.
        fn refuse(h: &Fixture, add: &[Value], kind: &str, words: &[String]) -> Value {
            let before = std::fs::read(&h.source).expect("bytes");
            let names = entries(h.root.path());
            let never = h.root.path().join("never.fcad");
            h.ask_constraints(&[], add);
            let v = reply(h.constrain(&never).output().expect("process"), EDIT, 2);
            assert_eq!(refused(&v), kind, "{v}");
            let text = v.to_string();
            for w in words {
                assert!(text.contains(w), "{w}: {v}");
            }
            assert_eq!(entries(h.root.path()), names, "a refusal left something");
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
            v
        }

        /// The two UUIDs of a joint, as the reasons name them.
        fn joint(row: &Value) -> Vec<String> {
            row["edge"]["joint"]
                .as_array()
                .expect("joint")
                .iter()
                .map(|u| u.as_str().expect("UUID").to_owned())
                .collect()
        }

        /// A two-Fillet plate: the free plate, rounded by the shipped command.
        fn twice(
            corners: [[f64; 2]; 4],
            (first, r1): ([f64; 2], f64),
            (second, r2): ([f64; 2], f64),
            name: &str,
        ) -> (Fixture, Fixture) {
            rounded_twice(
                &Part::plain(corners),
                corners,
                (first, r1),
                (second, r2),
                name,
            )
        }

        /// Discovery on a two-Fillet plate written without a kernel: the
        /// constraint editor offers the base Sketch with both Fillets in
        /// history order and their stored corners, the coordinate editor reads
        /// the same history, and the protocol — a build without a kernel asks
        /// for it first, one without the solver refuses the solve — writes
        /// nothing.
        #[test]
        fn constraint_discovery_and_protocol_without_native() {
            let (_keep, h) = twice_without_kernel(2.375, 3.0625);
            let [one, two] = rows(&h);
            let row = h.constraint_row();
            assert_eq!(row["available"], true, "{row}");
            assert!(row["refusal"].is_null());
            assert_eq!(
                row["fillet_base"],
                json!({
                    "fillet_feature_id": one["feature_id"],
                    "body_id": h.body_id(),
                    "edge": one["edge"],
                    "radius_mm": 2.375,
                    "stored_corner_mm": [X0 + W, Y0],
                    "second_fillet": {
                        "fillet_feature_id": two["feature_id"],
                        "previous_feature_id": one["feature_id"],
                        "history_index": 2,
                        "edge": two["edge"],
                        "radius_mm": 3.0625,
                        "stored_corner_mm": [X0 + W, Y0 + D],
                    },
                })
            );
            assert_eq!(h.catalog["sketches"][0]["editable"], true);
            assert_eq!(
                h.catalog["sketches"][0]["fillet_base"]["second_fillet"]["fillet_feature_id"],
                two["feature_id"]
            );
            let before = std::fs::read(&h.source).expect("bytes");
            let never = h.root.path().join("never.fcad");
            let names = entries(h.root.path());
            let kernel = ferritecad_occt::is_available();
            let solver = kernel && ferritecad_eval::solver_available();
            h.ask_constraints(&[], &dimensioned(&h, [0., 0.], 30., 10.));
            if !solver {
                let v = reply(h.constrain(&never).output().expect("process"), EDIT, 2);
                assert_eq!(refused(&v), "unsupported", "{v}");
                if !kernel {
                    assert!(v.to_string().contains("Open CASCADE"), "{v}");
                }
            }
            assert_eq!(entries(h.root.path()), names, "a refusal left something");
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
        }

        /// Adding a full dimensioning to a free plate rounded twice solves the
        /// plate: adjacent and opposite corners, both windings, different
        /// radii, the plate grown and shrunk to the exact bound of a radius,
        /// every copy measured in full.
        #[test]
        fn native_dimensioning_a_plate_rounded_twice_solves_it_and_keeps_both_fillets() {
            if !solving() {
                return;
            }
            type Case = (
                &'static str,
                [[f64; 2]; 4],
                [f64; 2],
                [f64; 2],
                (f64, f64),
                [f64; 2],
                (f64, f64),
            );
            let cases: [Case; 3] = [
                // Larger than stored in both directions.
                (
                    "ccw-adjacent",
                    CCW,
                    [X0 + W, Y0],
                    [X0 + W, Y0 + D],
                    (2.375, 3.0625),
                    [2.5, -1.75],
                    (41.25, 14.5),
                ),
                // 2 r1 = 9.625: smaller than stored, exactly at the bound.
                (
                    "cw-adjacent",
                    CW_FROM_UPPER_RIGHT,
                    [X0 + W, Y0],
                    [X0 + W, Y0 + D],
                    (4.8125, 1.1875),
                    [0.5, 1.25],
                    (24.5, 9.625),
                ),
                // 2 r2 = 11: a square, exactly at the bound; no shared Line.
                (
                    "ccw-opposite",
                    CCW,
                    [X0, Y0],
                    [X0 + W, Y0 + D],
                    (2.0, 5.5),
                    [-3.0, 2.0],
                    (20.25, 11.0),
                ),
            ];
            for (label, corners, first, second, (r1, r2), at, (width, depth)) in cases {
                let (_g, h) = twice(corners, (first, r1), (second, r2), &format!("k-{label}"));
                let expected = plate_of(&h, at, width, depth);
                assert_ne!(expected, h.stored_starts(), "solved and stored differ");
                let add = dimensioned(&h, at, width, depth);
                constrained_twice(
                    &h,
                    (&[], &add),
                    (first, r1),
                    (second, r2),
                    &expected,
                    0,
                    &format!("constraints-{label}"),
                );
            }
        }

        /// Replacing the leading dimension and the pin moves both cylinders'
        /// axes; on a warm cache the plate and both Fillets miss and then hit;
        /// then a radius and the height are edited on the solved plate; then
        /// every user constraint goes, leaving the closure links, the stored
        /// plate is the part again and the Sketch's coordinates (§28J) are
        /// offered and edited.
        #[test]
        fn native_replacing_removing_and_editing_on_after_constraints() {
            if !solving() {
                return;
            }
            let (first, second, (r1, r2)) = ([X0 + W, Y0], [X0 + W, Y0 + D], (2.375, 3.0625));
            let (_g, h) = twice(CCW, (first, r1), (second, r2), "k-chain");
            let (at, width, depth) = ([0.5, 1.25], 30.75, 10.5);
            let stored = h.stored_starts();
            let add = dimensioned(&h, at, width, depth);
            let one = constrained_twice(
                &h,
                (&[], &add),
                (first, r1),
                (second, r2),
                &plate_of(&h, at, width, depth),
                0,
                "k-first",
            );
            // The plate is cached; the copy that replaces the width and the pin
            // shares the document, so its warm entries are what a reader finds.
            let g = h.at_copy(&one);
            let warm = g.root.path().join("warm.fcad-cache");
            measure_two(&one, Some(&warm));
            let (_, events) = measure_two(&one, Some(&warm));
            use ferritecad_eval::CacheOutcome::{Hit, Miss};
            assert!(events.iter().all(|e| e.outcome == Hit), "{events:?}");
            let width_rule = listed(&g)
                .iter()
                .find(|c| c["rule"]["kind"] == "distance" && c["rule"]["distance"] == width)
                .expect("the width")["constraint_id"]
                .clone();
            let pin_rule = listed(&g)
                .iter()
                .find(|c| c["rule"]["kind"] == "fixed")
                .expect("the pin")["constraint_id"]
                .clone();
            let (at2, width2) = ([-2.25, 3.5], 22.25);
            let expected = plate_of(&g, at2, width2, depth);
            let two = constrained_twice(
                &g,
                (
                    &[width_rule.clone(), pin_rule],
                    &[length(&g.line(0), width2), pin(&g.line(0), at2[0], at2[1])],
                ),
                (first, r1),
                (second, r2),
                &expected,
                0,
                "k-replaced",
            );
            let g2 = g.at_copy(&two);
            assert!(!listed(&g2).iter().any(|c| c["constraint_id"] == width_rule));
            assert_eq!(listed(&g2).len(), listed(&g).len(), "two out, two in");
            // In place: the plate and both Fillets miss; neither is the old part.
            let [f1, f2] = rows(&g2);
            let (f1, f2, base) = (
                id_of(&f1["feature_id"]),
                id_of(&f2["feature_id"]),
                id_of(&f1["edge"]["feature_id"]),
            );
            let cache = two.with_extension("warm");
            std::fs::copy(&warm, &cache).expect("the warm cache");
            let (warmed, events) = measure_two(&two, Some(&cache));
            let by = outcomes(&events);
            for feature in [base, f1, f2] {
                assert_eq!(by.get(&feature), Some(&vec![Miss]), "{events:?}");
            }
            let (cold, _) = measure_two(&two, None);
            cold.same_as(&warmed);
            let (hit, events) = measure_two(&two, Some(&cache));
            assert!(events.iter().all(|e| e.outcome == Hit), "{events:?}");
            cold.same_as(&hit);

            // A radius, then the height, on the solved plate.
            let part = Part {
                rect: rect_of(&expected),
                stored: stored.clone(),
                solved: expected.clone(),
                constrained: true,
            };
            let radius = edited_either(&g2, &part, 2, (first, r1), (second, r2), 1.5, "k-radius");
            let g3 = g2.at_copy(&radius);
            let raised = super::height::raised_twice(
                &g3,
                &part,
                (first, r1),
                (second, 1.5),
                9.5,
                "k-height",
            );
            assert!(raised.exists());

            // Every user constraint removed: closure only, the stored plate.
            let user = user_constraints(&g2);
            assert_eq!(
                user.len(),
                7,
                "H/V on four Lines, the pin, the width and the depth"
            );
            let closed = constrained_twice(
                &g2,
                (&user, &[]),
                (first, r1),
                (second, r2),
                &stored,
                8,
                "k-closure",
            );
            let k = g2.at_copy(&closed);
            assert!(listed(&k).iter().all(|c| c["rule"]["kind"] == "coincident"));
            assert_eq!(listed(&k).len(), 4);
            assert_eq!(k.catalog["sketches"][0]["editable"], true, "§28J again");
            assert_eq!(
                k.catalog["sketches"][0]["fillet_base"]["second_fillet"]["fillet_feature_id"],
                rows(&k)[1]["feature_id"]
            );
            let moved = super::sketch::moved_twice(
                &k,
                CCW,
                (first, r1),
                (second, r2),
                [-1.25, 1.0, 40.0, 12.5],
                H,
                "k-closure-moved",
            );
            assert!(moved.exists());
        }

        /// The solved plate decides, each Fillet separately, and the flat.
        /// Each refusal names that Fillet's own joint UUIDs — or the shared
        /// Line's — on a plate whose stored rectangle would still allow it, and
        /// writes nothing; the least depth that leaves the flat publishes and is
        /// measured; opposite corners owe none.
        #[test]
        fn native_the_solved_plate_decides_each_radius_and_the_shared_flat() {
            if !solving() {
                return;
            }
            // The wider Fillet is the first: its bound (2 r1 = 9.625) fails.
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            let (_a, h) = twice(
                CW_FROM_UPPER_RIGHT,
                (first, 4.8125),
                (second, 1.1875),
                "k-first",
            );
            let [one, two] = rows(&h);
            let v = refuse(
                &h,
                &dimensioned(&h, [0., 0.], 24.5, 9.5),
                "input",
                &joint(&one)
                    .into_iter()
                    .chain(["too short".to_owned()])
                    .collect::<Vec<_>>(),
            );
            let text = v["error"]["message"].as_str().expect("message").to_owned();
            assert!(
                !joint(&two).iter().all(|u| text.contains(u.as_str())),
                "Fillet 2 fits 9.5 mm (2 r2 = 2.375): {text}"
            );
            // The wider Fillet is the second: Fillet 1 fits, Fillet 2's does not.
            let (first, second) = ([X0, Y0], [X0 + W, Y0 + D]);
            let (_b, h) = twice(CCW, (first, 2.0), (second, 5.5), "k-second");
            let [one, two] = rows(&h);
            let v = refuse(
                &h,
                &dimensioned(&h, [0., 0.], 20.25, 10.9),
                "input",
                &joint(&two),
            );
            let text = v["error"]["message"].as_str().expect("message").to_owned();
            assert!(
                !joint(&one).iter().all(|u| text.contains(u.as_str())),
                "Fillet 1 fits 10.9 mm: {text}"
            );
            // Adjacent r1 = r2 = 3: each fits 6.005 mm alone, the flat does not.
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            let (_c, h) = twice(CCW, (first, 3.0), (second, 3.0), "k-flat");
            let [one, two] = rows(&h);
            let shared: Vec<String> = joint(&one)
                .into_iter()
                .filter(|u| joint(&two).contains(u))
                .collect();
            assert_eq!(shared.len(), 1, "adjacent corners share one Line");
            let mut least = 6.01_f64;
            while 3.0 > ferritecad_document::pair_bound(least, 3.0) {
                least = least.next_up();
            }
            for depth in [least.next_down(), 6.005] {
                let mut words = shared.clone();
                words.push("flat".to_owned());
                refuse(&h, &dimensioned(&h, [0., 0.], 25., depth), "input", &words);
            }
            let add = dimensioned(&h, [0., 0.], 25., least);
            constrained_twice(
                &h,
                (&[], &add),
                (first, 3.0),
                (second, 3.0),
                &plate_of(&h, [0., 0.], 25., least),
                0,
                "k-flat-least",
            );
            // A solve that flips a Line, and one that loses the rectangle.
            let starts = h.stored_starts();
            let across = horizontal(&starts);
            let mut flip: Vec<Value> = (0..4)
                .map(|i| {
                    rule(
                        &h.line(i),
                        if across[i] { "horizontal" } else { "vertical" },
                    )
                })
                .collect();
            flip.push(pin(&h.line(0), starts[1][0] + 5.0, starts[0][1]));
            refuse(&h, &flip, "input", &["side".to_owned()]);
            refuse(
                &h,
                &[length(&h.line(0), 30.)],
                "unsupported",
                &["rectangle".to_owned()],
            );
            // Opposite corners share no Line: 2 r = 6 mm is the bound, alone.
            let (first, second) = ([X0, Y0], [X0 + W, Y0 + D]);
            let (_d, h) = twice(CCW, (first, 3.0), (second, 3.0), "k-opposite");
            refuse(
                &h,
                &dimensioned(&h, [0., 0.], 25., 5.99),
                "input",
                &["too short".to_owned()],
            );
            let add = dimensioned(&h, [0., 0.], 25., 6.0);
            constrained_twice(
                &h,
                (&[], &add),
                (first, 3.0),
                (second, 3.0),
                &plate_of(&h, [0., 0.], 25., 6.0),
                0,
                "k-opposite-least",
            );
        }

        /// A plate whose stored rectangle is too small for its radii and whose
        /// solved one is not: dimensioned, then rounded twice on the solved
        /// plate. Widening it is accepted — the stored bound would have refused
        /// — and narrowing it below the radius's bound is refused naming that
        /// Fillet, publishing nothing.
        #[test]
        fn native_a_solved_plate_may_outgrow_its_stored_bound_and_not_shrink_below_it() {
            if !solving() {
                return;
            }
            let (at, width, depth) = ([2.5, -1.75], 41.0, 15.0);
            let (_plain, g, expected) = dimensioned_plate(CCW, at, width, depth, "k-small");
            let (first, second) = ([X0 + W, Y0], [X0 + W, Y0 + D]);
            // 12.25 mm deep as stored, 14 mm needed by r1 = 7: the stored plate
            // could not hold either radius; the solved 15 mm plate holds both.
            assert!(rect_of(&g.stored_starts())[3] < 2. * 7.0);
            let one = g.root.path().join("one.fcad");
            g.ask_stored(first, 7.0);
            reply(g.fillet(&one).output().expect("process"), OP, 0);
            let g1 = g.at_copy(&one);
            let part = Part {
                rect: rect_of(&expected),
                stored: g.stored_starts(),
                solved: expected.clone(),
                constrained: true,
            };
            let two = second_fillet(&g1, &part, first, 7.0, second, 7.5, "k-small-two");
            let h = g1.at_copy(&two);
            let starts = h.stored_starts();
            let across = horizontal(&starts);
            let v = across.iter().position(|a| !*a).expect("a vertical Line");
            let depth_rule = listed(&h)
                .iter()
                .find(|c| c["rule"]["kind"] == "distance" && c["rule"]["distance"] == depth)
                .expect("the depth")["constraint_id"]
                .clone();
            let [_, two_row] = rows(&h);
            // 7.5 mm × 2 = 15: one hundredth short is refused, naming Fillet 2.
            refuse_replacing(
                &h,
                &depth_rule,
                &length(&h.line(v), 14.99),
                &joint(&two_row),
            );
            // Wider: accepted, measured.
            let wider = constrained_twice(
                &h,
                (
                    std::slice::from_ref(&depth_rule),
                    &[length(&h.line(v), 16.0)],
                ),
                (first, 7.0),
                (second, 7.5),
                &plate_of(&h, at, width, 16.0),
                0,
                "k-wider",
            );
            assert!(wider.exists());
        }

        /// Replace one constraint with another, expecting a refusal that names
        /// `words`, and nothing published.
        fn refuse_replacing(h: &Fixture, out: &Value, add: &Value, words: &[String]) {
            let before = std::fs::read(&h.source).expect("bytes");
            let names = entries(h.root.path());
            let never = h.root.path().join("never.fcad");
            h.ask_constraints(std::slice::from_ref(out), std::slice::from_ref(add));
            let v = reply(h.constrain(&never).output().expect("process"), EDIT, 2);
            assert_eq!(refused(&v), "input", "{v}");
            for w in words {
                assert!(v.to_string().contains(w), "{w}: {v}");
            }
            assert_eq!(entries(h.root.path()), names, "a refusal left something");
            assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
        }

        /// The real solver's diagnoses, with the UUIDs the document holds: a
        /// conflict names stored constraints and publishes nothing; a
        /// redundancy publishes and names the equality it found redundant.
        /// Stale, occupied, aliased and self outputs, cancellation and a lost
        /// report are atomic.
        #[test]
        fn native_solver_diagnoses_and_atomic_refusals_under_two_fillets() {
            if !solving() {
                return;
            }
            let (first, second, (r1, r2)) = ([X0 + W, Y0], [X0 + W, Y0 + D], (2.375, 3.0625));
            let (_g, h) = twice(CCW, (first, r1), (second, r2), "k-solver");
            let (at, width, depth) = ([0.5, 1.25], 30.75, 10.5);
            let add = dimensioned(&h, at, width, depth);
            let copy = constrained_twice(
                &h,
                (&[], &add),
                (first, r1),
                (second, r2),
                &plate_of(&h, at, width, depth),
                0,
                "k-dimensioned",
            );
            let g = h.at_copy(&copy);
            let stored_ids: Vec<Value> = listed(&g)
                .iter()
                .map(|c| c["constraint_id"].clone())
                .collect();
            let starts = g.stored_starts();
            let across = horizontal(&starts);
            let opposite = (1..4)
                .find(|i| across[*i])
                .expect("the other horizontal Line");
            // A real conflict: the two horizontal sides, dimensioned differently.
            let v = refuse(&g, &[length(&g.line(opposite), 20.)], "constraint", &[]);
            let named = v["error"]["constraint_conflict"]["constraints"]
                .as_array()
                .expect("the conflicting constraints")
                .clone();
            assert!(!named.is_empty(), "{v}");
            assert!(named.iter().all(|c| c["constraint_id"].is_string()), "{v}");
            assert!(
                named
                    .iter()
                    .any(|c| stored_ids.contains(&c["constraint_id"])),
                "the conflict names a constraint the document holds: {v}"
            );
            // A real redundancy: the equality says what H/V and the lengths said.
            let before = std::fs::read(&g.source).expect("bytes");
            let catalog = &g.catalog["sketches"][0]["constraint_edit"]["curves"];
            g.ask_constraints(
                &[],
                &[json!({
                    "rule": "equal_length",
                    "a_curve_id": catalog[0]["curve_id"],
                    "b_curve_id": catalog[opposite]["curve_id"],
                })],
            );
            let said = g.root.path().join("redundant.fcad");
            let v = reply(g.constrain(&said).output().expect("process"), EDIT, 0);
            let equality = v["result"]["added_constraints"][0]["constraint_id"].clone();
            assert_eq!(
                v["result"]["solve"]["redundant_constraint_ids"],
                json!([equality]),
                "{v}"
            );
            assert_eq!(v["result"]["solve"]["degrees_of_freedom"], 0);
            assert_eq!(std::fs::read(&g.source).expect("bytes"), before);
            let (cold, _) = measure_two(&said, None);
            let [f1, f2] = rows(&g);
            check_two_at(
                &cold,
                rect_of(&plate_of(&g, at, width, depth)),
                (
                    id_of(&f1["feature_id"]),
                    plate_of(&g, at, width, depth)
                        [starts.iter().position(|v| *v == first).expect("corner")],
                    r1,
                ),
                (
                    id_of(&f2["feature_id"]),
                    plate_of(&g, at, width, depth)
                        [starts.iter().position(|v| *v == second).expect("corner")],
                    r2,
                ),
                id_of(&f1["edge"]["feature_id"]),
                H,
            );

            // Atomic: stale, occupied, alias, self, cancelled, a lost report.
            let names = entries(g.root.path());
            let never = g.root.path().join("never.fcad");
            let taken = g.root.path().join("taken.fcad");
            std::fs::write(&taken, b"another process owns this").expect("occupied");
            let alias = g.root.path().join("alias.fcad");
            std::fs::hard_link(&g.source, &alias).expect("hard link");
            let names = {
                let mut n = names;
                n.extend([
                    taken.file_name().expect("n").to_owned(),
                    alias.file_name().expect("n").to_owned(),
                ]);
                n.sort();
                n
            };
            // A valid edit: the width replaced by a narrower one.
            let width_rule = listed(&g)
                .iter()
                .find(|c| c["rule"]["kind"] == "distance" && c["rule"]["distance"] == width)
                .expect("the width")["constraint_id"]
                .clone();
            g.ask_constraints(
                std::slice::from_ref(&width_rule),
                &[length(&g.line(0), 22.0)],
            );
            for (destination, why) in [
                (g.source.clone(), "the source is not its own output"),
                (taken.clone(), "an occupied output is not replaced"),
                (alias.clone(), "an alias of the source is the source"),
            ] {
                let v = reply(
                    g.constrain(&destination).output().expect("process"),
                    EDIT,
                    2,
                );
                assert_eq!(refused(&v), "input", "{why}: {v}");
            }
            assert_eq!(
                std::fs::read(&taken).expect("taken"),
                b"another process owns this"
            );
            let mut stale = cli();
            stale
                .arg(EDIT)
                .arg(&g.source)
                .arg("--sketch")
                .arg(g.plate_sketch_id())
                .arg("--expect-version")
                .arg("0".repeat(64))
                .arg("--request")
                .arg(&g.request)
                .arg("-o")
                .arg(&never)
                .arg("--json");
            assert_eq!(
                refused(&reply(stale.output().expect("process"), EDIT, 2)),
                "input"
            );
            assert_eq!(entries(g.root.path()), names, "a refusal left something");

            let d = Document::open_read_only(&g.source).expect("source");
            let expected = ferritecad_document::DocumentVersion {
                document_id: d.meta().document_id,
                content: d.content_version().expect("version"),
            };
            d.close().expect("close");
            let destination = g.root.path().join("cancelled.fcad");
            let token = ferritecad_kernel::CancelToken::new();
            let stop = token.clone();
            let context = OperationContext::default()
                .with_cancel(token)
                .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                    if fraction >= 0.95 {
                        stop.cancel();
                    }
                }));
            let request = ferritecad_jobs::EditSketchConstraintsRequest {
                source: g.source.clone(),
                expected,
                sketch: id_of(&json!(g.plate_sketch_id())),
                edits: ferritecad_document::SketchConstraintEdits {
                    remove: vec![width_rule.as_str().expect("UUID").parse().expect("UUID")],
                    add: vec![ferritecad_document::AddSketchConstraint::Line(
                        ferritecad_document::AddLineConstraint::Line {
                            curve: g.line(0).as_str().expect("UUID").parse().expect("UUID"),
                            kind: ferritecad_document::LineConstraintKind::Distance(
                                ferritecad_document::LineLengthMm::new(22.0).expect("length"),
                            ),
                        },
                    )],
                },
                destination: destination.clone(),
            };
            let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
            let result =
                ferritecad_jobs::edit_sketch_constraints_copy(&request, &mut kernel, &context);
            assert!(result.is_err(), "{result:?}");
            assert!(!destination.exists());
            assert_eq!(kernel.live_shape_count(), 0);
            assert_eq!(entries(g.root.path()), names, "cancellation left something");

            let lost = g.root.path().join("lost-report.fcad");
            assert_eq!(
                g.constrain(&lost)
                    .stdout(pipe::closed_pipe())
                    .stderr(pipe::closed_pipe())
                    .status()
                    .expect("pipes")
                    .code(),
                Some(7)
            );
            assert!(lost.exists());
            assert_eq!(std::fs::read(&g.source).expect("bytes"), before);
        }
    }
}
