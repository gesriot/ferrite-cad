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
            !events.is_empty() && events.iter().all(|e| e.outcome == expected),
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
    measured
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
    for (k, (lo, hi)) in [(X0, X0 + W), (Y0, Y0 + D), (0.0, height)]
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
    for c in CCW {
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
    let centre = inward(corner, r);
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
    let exact = W * D * height - (1. - PI / 4.) * r * r * height;
    let slack = PI / 2. * r * LINEAR_MM * height;
    assert!(
        m.volume <= exact + 1e-6 * exact && m.volume >= exact - slack - 1e-6 * exact,
        "STL volume {} is not a mesh of {exact}",
        m.volume
    );
}

/// The axis of the fillet at a corner: r inward along both sides.
fn inward(corner: [f64; 2], r: f64) -> [f64; 2] {
    [
        corner[0] + if corner[0] == X0 { r } else { -r },
        corner[1] + if corner[1] == Y0 { r } else { -r },
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

/// A filleted copy is refused by every editor by name, a second fillet is
/// refused, and the unsupported histories made by the shipped creators and
/// Cut are refused by discovery.
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
    names_it(&catalog["edit_extrude"]["refusal"]);
    names_it(&catalog["sketches"][0]["refusal"]);
    names_it(&catalog["sketches"][0]["constraint_edit"]["refusal"]);
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
    let raised = cli()
        .arg("edit-extrude")
        .arg(&copy)
        .args(["--feature", &base, "--distance-mm", "9"])
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
