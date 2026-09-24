// SPDX-License-Identifier: MIT
//! §26A: the first real Cut into an existing body.
//!
//! Everything measured here is read back — the volume and the surfaces from the
//! kernel, the shape of the solid from the exported bytes, the history from the
//! document — rather than compared against a number written down in advance.
//! Two facts are asserted rather than measured, and they are the two this slice
//! exists to keep: the body is the one it always was, and the cut is a feature
//! in its history rather than a replacement for what was there.
#![allow(clippy::panic)]
use ferritecad_document::{CutExtent, ExtentVocabulary};
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

const OP: &str = "cut-circular-copy";
const LINEAR_MM: f64 = 0.05;
const ANGULAR_RAD: f64 = 0.1;
/// Single-precision STL coordinates land a few parts in 10^7 from the number
/// they belong to; the tessellation's own chord budget is separate.
const ROUNDING_MM: f64 = 1e-4;

/// The acceptance part: 60 x 40 x 10 from the world origin.
const WIDTH: f64 = 60.0;
const DEPTH: f64 = 40.0;
const HEIGHT: f64 = 10.0;
/// The acceptance tool: r5 at (20, 15).
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
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: this build has no Open CASCADE");
        false
    }
}
/// Keeps a published artefact where a CI step can hand it to the pinned reader.
fn keep(path: &Path, name: &str) {
    let Some(dir) = std::env::var_os("FCAD_CUT_ARTIFACTS") else {
        return;
    };
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).expect("artifact directory");
    std::fs::copy(path, dir.join(name)).expect("artifact");
}

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    request: PathBuf,
    catalog: Value,
}
impl Fixture {
    /// The plate the shipped creation command really makes.
    fn plate() -> Self {
        Self::built(|path| {
            let create = path.with_extension("create.json");
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
                .arg(path)
                .arg("--json")
                .output()
                .expect("create");
            assert!(made.status.success(), "{made:?}");
            std::fs::remove_file(&create).expect("the request is not part of the fixture");
        })
    }
    /// The same drawing written without a kernel, so discovery and the request
    /// protocol are testable in a build that has none.
    fn drawn() -> Self {
        Self::built(|path| write_plate_document(path, WIDTH, DEPTH, HEIGHT))
    }
    fn built(build: impl FnOnce(&Path)) -> Self {
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("плита space.fcad");
        build(&source);
        let catalog = inspect(&source);
        Self {
            request: root.path().join("request.json"),
            root,
            source,
            catalog,
        }
    }
    fn body(&self) -> &Value {
        &self.catalog["bodies"][0]
    }
    fn body_id(&self) -> &str {
        self.body()["body_id"].as_str().expect("body UUID")
    }
    fn version(&self) -> &str {
        self.catalog["content_version"].as_str().expect("version")
    }
    fn ask(&self, center: [f64; 2], radius: f64, depth: f64) {
        write(
            &self.request,
            &json!({"request_version":1,"center_mm":center,"radius_mm":radius,
                    "depth_mm":depth}),
        );
    }
    fn cut(&self, output: &Path) -> Command {
        self.cut_from(&self.source, self.body_id(), self.version(), output)
    }
    fn cut_from(&self, source: &Path, body: &str, version: &str, output: &Path) -> Command {
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

/// The four objects a plate is, written without a kernel.
fn write_plate_document(path: &Path, width: f64, depth: f64, height: f64) {
    use ferritecad_document::{
        Body, CapSide, DatumPlane, Dependency, DependencyRole, EndCondition, EntityKind,
        Expression, Extrude, Point2, SelectionRule, Sketch, SketchCurve, SolidOperation,
        TopologyRef,
    };
    use ferritecad_types::{ObjectId, StableEntityId, Transform};
    let mut d = Document::create(path).expect("document");
    let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
    let corners = [[0.0, 0.0], [width, 0.0], [width, depth], [0.0, depth]];
    let segments: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();
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
        let curves = segments
            .iter()
            .enumerate()
            .map(|(i, id)| {
                Ok(SketchCurve {
                    id: *id,
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: Point2::new(corners[i][0], corners[i][1])?,
                        end: Point2::new(corners[(i + 1) % 4][0], corners[(i + 1) % 4][1])?,
                    },
                })
            })
            .collect::<ferritecad_types::Result<Vec<_>>>()?;
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
                target_body: None,
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
        Ok(())
    })
    .expect("plate document");
    d.close().expect("close");
}

/// What a document stores about its features and its body, by identity.
struct Stored {
    /// Every feature, in `objects()` order, with what it consumes.
    features: Vec<(
        ferritecad_types::ObjectId,
        Option<ferritecad_types::ObjectId>,
        String,
    )>,
    body: ferritecad_types::ObjectId,
    tip: Option<ferritecad_types::ObjectId>,
    /// Every sketch, by identity, with the curves it holds.
    sketches: BTreeMap<ferritecad_types::ObjectId, Vec<SketchGeometry>>,
    refs: Vec<ferritecad_document::TopologyRef>,
    dependencies: Vec<ferritecad_document::Dependency>,
    objects: usize,
    extrude_schema: BTreeMap<ferritecad_types::ObjectId, u32>,
}
fn stored(path: &Path) -> Stored {
    let d = Document::open_read_only(path).expect("reopen");
    let objects = d.objects().expect("objects");
    let mut features = Vec::new();
    let mut sketches = BTreeMap::new();
    let mut body = None;
    let mut tip = None;
    let mut extrude_schema = BTreeMap::new();
    for object in &objects {
        match &object.payload {
            ObjectPayload::Extrude(feature) => {
                features.push((
                    object.id,
                    feature.previous,
                    format!("{:?}", feature.operation),
                ));
                extrude_schema.insert(object.id, feature.schema_version());
                assert!(
                    feature.target_body.is_none(),
                    "this build names the feature it modifies, never a body"
                );
            }
            ObjectPayload::Sketch(sketch) => {
                sketches.insert(
                    object.id,
                    sketch.curves.iter().map(|c| c.geometry.clone()).collect(),
                );
            }
            ObjectPayload::Body(b) => {
                body = Some(object.id);
                tip = b.tip_feature;
            }
            _ => {}
        }
    }
    let refs = d.topology_refs().expect("refs");
    let dependencies = d.dependencies().expect("dependencies");
    let count = objects.len();
    d.close().expect("close");
    Stored {
        features,
        body: body.expect("a body"),
        tip,
        sketches,
        refs,
        dependencies,
        objects: count,
        extrude_schema,
    }
}

/// What the real kernel says about the solid a document rebuilds into.
struct Analytic {
    faces: u64,
    volume: f64,
    /// Every surface the body's own references resolve to, by role.
    by_role: BTreeMap<String, Vec<ferritecad_kernel::FaceSurface>>,
    /// How many separate shapes the rebuild produced and still holds.
    shapes: usize,
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
        let outcomes: Vec<_> = events.iter().map(|e| e.outcome).collect();
        assert!(
            outcomes.iter().all(|o| *o == expected),
            "every feature was expected to {expected:?}: {events:?}"
        );
        built
    } else {
        ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("cold rebuild after reopen")
    };

    // The body's shape is the tip's, and it is the one thing exported. The
    // intermediate extrusion is still addressable, which is what makes a
    // reference to the *earlier* feature's own output a different reference
    // from one to the finished part.
    let objects = doc.objects().expect("objects");
    let body = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("a body");
    let shape = built.shape(body.id).expect("the body was built");
    let mut by_role: BTreeMap<String, Vec<_>> = BTreeMap::new();
    let mut stats = None;
    for reference in &doc.topology_refs().expect("refs") {
        let resolved = built.resolve(reference).expect("resolve");
        assert!(
            !resolved.is_empty(),
            "{:?} resolved to nothing",
            reference.id
        );
        let role = match &reference.output_role {
            SemanticRole::ExtrudeCap { side } => format!("cap {side:?}"),
            SemanticRole::ExtrudeSide { .. } => "side".to_owned(),
            SemanticRole::CarriedCap { side } => format!("carried cap {side:?}"),
            SemanticRole::CarriedSide { .. } => "carried side".to_owned(),
            other => panic!("unexpected role {other:?}"),
        };
        let owner = format!("{role} of {}", reference.producer_feature);
        for handle in &resolved {
            let surface = kernel.face_surface(*handle).expect("surface");
            by_role.entry(owner.clone()).or_default().push(surface);
            if handle.shape() == shape {
                stats = Some(kernel.shape_stats(shape).expect("stats"));
            }
        }
    }
    let (faces, volume) = stats.expect("some reference names the finished body");
    let shapes = built.shape_count();
    built.release_all(&mut kernel);
    doc.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0, "every handle was released");
    Analytic {
        faces,
        volume,
        by_role,
        shapes,
    }
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

/// Everything the exported mesh must be for the part to really have that cut.
///
/// A mesh is an inscribed prism where the hole is concerned, so it removes
/// slightly *less* than the exact cylinder and its volume is bounded below by
/// the exact solid rather than above. Nothing here demands an analytic pi of a
/// mesh: the volume is compared against what its own measured perimeter says
/// the hole encloses.
fn check_cut_mesh(m: &Mesh, center: [f64; 2], radius: f64, depth: f64) {
    assert!(m.triangles >= 24);

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

    // The bore wall exists, at the radius asked for and about the centre asked
    // for. A facet of the pocket's floor also has its corners on that circle —
    // the floor is a disc whose rim is the bore — so the wall is the part of it
    // that spans the depth, which is what "wall" means.
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
    let open_at_top = !m.faces.iter().any(|t| covers(t, HEIGHT));
    if depth < HEIGHT {
        assert!(floored, "a pocket has a floor at its depth");
        assert!(!open_at_top, "a pocket leaves the far side closed");
    } else {
        assert!(open_at_top, "a through hole is open at the far side");
    }

    // The part is still the part.
    assert!(m.lo[0].abs() < ROUNDING_MM && m.lo[1].abs() < ROUNDING_MM);
    assert!((m.hi[0] - WIDTH).abs() < ROUNDING_MM && (m.hi[1] - DEPTH).abs() < ROUNDING_MM);
    assert!((m.hi[2] - HEIGHT).abs() < ROUNDING_MM && m.lo[2].abs() < ROUNDING_MM);

    // The volume the winding gives is the plate less a cylinder, within the
    // tessellation's own budget: an inscribed bore removes a little less.
    let exact = WIDTH * DEPTH * HEIGHT - PI * radius * radius * depth;
    let removed_lower = PI * (radius - LINEAR_MM).powi(2) * depth;
    assert!(
        m.volume >= exact - 1e-6 && m.volume <= WIDTH * DEPTH * HEIGHT - removed_lower + 1e-6,
        "STL volume {} is outside what a mesh of this part can be",
        m.volume
    );
    let _ = m.points.len();
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

/// Every SQL row of the source survives into the copy, and the new rows are
/// exactly the ones this operation is allowed to add.
///
/// A short explicit allow-list: two new `objects` rows, the selected `body`
/// row's payload and hash, the new `deps` and `topology_refs` rows, the
/// capabilities a predecessor and a carried name newly require, and the copy's
/// own timestamp. Anything else that moved is a cell this operation had no
/// business touching.
fn only_the_cut_was_added(source: &Path, copy: &Path, body: ferritecad_types::ObjectId) {
    let before = tables(source);
    let after = tables(copy);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "the copy has the same tables"
    );
    let selected = rusqlite::types::Value::Blob(body.to_bytes().to_vec());
    for (table, (columns, rows)) in &before {
        let (mine_columns, mine) = &after[table];
        assert_eq!(columns, mine_columns, "{table} columns");
        match table.as_str() {
            // Rows may be added; none may be lost or altered, except the one
            // body whose tip moved.
            "objects" => {
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
                            row.contains(&selected)
                                && matches!(columns[i].as_str(), "payload" | "payload_hash"),
                            "objects.{} changed to {:?}",
                            columns[i],
                            found[i]
                        );
                    }
                }
                assert_eq!(
                    mine.len(),
                    rows.len() + 2,
                    "a cut adds a sketch and a feature"
                );
            }
            "deps" | "topology_refs" | "capabilities" => {
                for row in rows {
                    // The body's old tip edge is the one row a cut replaces:
                    // the body tips at the new feature now, and leaving the old
                    // edge would be an ordering constraint no payload asks for.
                    let kept = mine.iter().any(|m| m[1..] == row[1..]);
                    let is_old_tip = table == "deps" && row.contains(&selected);
                    assert!(kept || is_old_tip, "{table} row {row:?} disappeared");
                }
                assert!(mine.len() >= rows.len(), "{table} lost rows");
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

/// Discovery, the request protocol and every structural refusal, with no kernel
/// needed.
#[test]
fn cut_discovery_and_protocol_without_native() {
    let f = Fixture::drawn();
    let before = std::fs::read(&f.source).expect("source bytes");

    let discovery = &f.body()["cut_edit"];
    assert_eq!(discovery["available"], true, "{discovery:?}");
    assert!(discovery["refusal"].is_null());
    let target = &discovery["target"];
    assert_eq!(target["body_id"], f.body_id());
    assert_eq!(target["height_mm"], HEIGHT);
    assert_eq!(target["extents_mm"], json!([[0.0, 0.0], [WIDTH, DEPTH]]));
    assert_eq!(target["direction"], "+z along the plane normal");
    assert!(target["wall_clearance_mm"].as_f64().expect("clearance") > 0.0);
    // The plane and the feature it would modify are named, not implied.
    let saved = stored(&f.source);
    assert_eq!(
        target["tip_feature_id"],
        saved.tip.expect("a tip").to_string()
    );
    assert_eq!(
        target["profile_sketch_id"],
        saved
            .sketches
            .keys()
            .next()
            .expect("one sketch")
            .to_string()
    );

    let never = f.root.path().join("never.fcad");
    f.ask(CENTER, RADIUS, 4.0);
    let names = entries(f.root.path());
    let foreign = ferritecad_types::ObjectId::new().to_string();

    // Structural and numeric refusals, each with every other argument correct.
    for (center, radius, depth, why) in [
        (CENTER, 0.0, 4.0, "a tool of no radius"),
        (CENTER, -5.0, 4.0, "a negative radius"),
        (CENTER, RADIUS, 0.0, "a cut of no depth"),
        (CENTER, RADIUS, -4.0, "a negative depth"),
        (CENTER, RADIUS, HEIGHT + 1.0, "deeper than the part"),
        ([0.0, 15.0], RADIUS, 4.0, "a tool hanging off the edge"),
        (
            [RADIUS, 15.0],
            RADIUS,
            4.0,
            "a tool exactly touching the wall",
        ),
        ([200.0, 200.0], RADIUS, 4.0, "a tool nowhere near the part"),
        (CENTER, 1.0e7, 4.0, "a radius past the published bound"),
    ] {
        f.ask(center, radius, depth);
        let refused = reply(f.cut(&never).output().expect("process"), OP, 2);
        assert!(
            matches!(
                refused["error"]["kind"].as_str(),
                Some("input" | "unsupported")
            ),
            "{why}: {refused:?}"
        );
    }
    // A body that is not there, and a version that is not this one.
    f.ask(CENTER, RADIUS, 4.0);
    for (body, version) in [
        (foreign.as_str(), f.version()),
        (f.body_id(), &"0".repeat(64)),
    ] {
        let refused = reply(
            f.cut_from(&f.source, body, version, &never)
                .output()
                .expect("process"),
            OP,
            2,
        );
        // A build with no kernel answers `unsupported` before it reaches the
        // request, which is a different true thing about the same refusal.
        assert!(
            matches!(
                refused["error"]["kind"].as_str(),
                Some("input" | "unsupported")
            ),
            "{refused:?}"
        );
    }
    // Request forms this reader will not accept at all.
    for raw in [
        r#"{"request_version":2,"center_mm":[20,15],"radius_mm":5,"depth_mm":4}"#,
        r#"{"request_version":1,"center_mm":[20,15],"radius_mm":5}"#,
        r#"{"request_version":1,"center_mm":[20,15],"radius_mm":5,"depth_mm":4,"plane":"xy"}"#,
        r#"{"request_version":1,"center_mm":[20,15,0],"radius_mm":5,"depth_mm":4}"#,
        r#"{"request_version":1,"center_mm":[20,15],"radius_mm":5,"depth_mm":1e999}"#,
    ] {
        std::fs::write(&f.request, raw).expect("request");
        let refused = reply(f.cut(&never).output().expect("process"), OP, 2);
        assert!(
            matches!(
                refused["error"]["kind"].as_str(),
                Some("input" | "unsupported")
            ),
            "{raw}: {refused:?}"
        );
    }
    // A refused operation whose report cannot be written is still a refusal.
    f.ask(CENTER, 0.0, 4.0);
    assert_eq!(
        f.cut(&never)
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
    assert_eq!(saved.objects, 4);
}

/// The measured slice: a through hole and a pocket, both from the same part.
#[test]
fn native_a_through_hole_and_a_pocket_are_what_the_numbers_say() {
    if !native() {
        return;
    }
    let f = Fixture::plate();
    let before = std::fs::read(&f.source).expect("source bytes");
    let saved = stored(&f.source);
    assert_eq!(saved.objects, 4);
    assert_eq!(saved.features.len(), 1);
    assert_eq!(
        saved.features[0].1, None,
        "the first feature consumes nothing"
    );
    assert_eq!(
        saved.extrude_schema[&saved.features[0].0], 1,
        "a feature that starts a body is the v1 feature it always was"
    );
    let plate_feature = saved.features[0].0;

    for (name, depth, walls, refs) in [("holed", HEIGHT, 7u64, 10usize), ("pocket", 4.0, 8, 11)] {
        let copy = f.root.path().join(format!("{name}.fcad"));
        f.ask(CENTER, RADIUS, depth);
        let published = reply(f.cut(&copy).output().expect("process"), OP, 0);
        let result = &published["result"];
        assert_eq!(
            result["body_id"],
            saved.body.to_string(),
            "the body is the same body"
        );
        assert_eq!(
            result["previous_feature_id"],
            plate_feature.to_string(),
            "the cut modifies the feature that was the tip"
        );
        assert_eq!(
            std::fs::read(&f.source).expect("source bytes"),
            before,
            "the source was touched"
        );

        // The history the copy holds: the original feature is still there and
        // still consumes nothing; the cut consumes it; the body tips at the cut.
        let now = stored(&copy);
        assert_eq!(
            now.objects, 6,
            "a plane, two sketches, two features, one body"
        );
        assert_eq!(now.body, saved.body);
        let cut = result["feature_id"]
            .as_str()
            .expect("feature")
            .parse::<ferritecad_types::ObjectId>()
            .expect("UUID");
        assert_eq!(now.tip, Some(cut), "the body's tip is the cut");
        let history: BTreeMap<_, _> = now
            .features
            .iter()
            .map(|(id, previous, op)| (*id, (*previous, op.clone())))
            .collect();
        assert_eq!(history[&plate_feature], (None, "NewBody".to_owned()));
        assert_eq!(history[&cut], (Some(plate_feature), "Cut".to_owned()));
        assert_eq!(
            now.extrude_schema[&plate_feature], 1,
            "the feature that was there is stored exactly as it was"
        );
        assert_eq!(
            now.extrude_schema[&cut], 2,
            "a feature that names a predecessor is a v2 feature"
        );
        // The part's own drawing is untouched; the tool has its own sketch.
        for (id, curves) in &saved.sketches {
            assert_eq!(
                now.sketches.get(id),
                Some(curves),
                "the part's profile moved"
            );
        }
        let tool: Vec<_> = now
            .sketches
            .iter()
            .filter(|(id, _)| !saved.sketches.contains_key(id))
            .collect();
        assert_eq!(tool.len(), 1, "one new sketch");
        assert!(matches!(
            tool[0].1.as_slice(),
            [SketchGeometry::Circle { radius, .. }] if (*radius - RADIUS).abs() < 1e-12
        ));
        // Every reference the source had is still there, unchanged.
        for reference in &saved.refs {
            assert!(
                now.refs.contains(reference),
                "the cut lost reference {}",
                reference.id
            );
        }
        assert_eq!(now.refs.len(), refs, "the cut names its own geometry too");
        for dependency in &saved.dependencies {
            let replaced_tip = dependency.role == ferritecad_document::DependencyRole::BodyTip;
            assert!(
                now.dependencies.contains(dependency) || replaced_tip,
                "the cut lost dependency {dependency:?}"
            );
        }
        only_the_cut_was_added(&f.source, &copy, saved.body);

        // Reopen and cold rebuild.
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
        assert!(
            text.contains(&format!("{refs} of {refs} stored references resolved")),
            "{text}"
        );
        assert!(text.contains("tip Cut"), "{text}");

        // The B-Rep, and the surfaces its own names resolve to.
        let solid = analytic(&copy);
        assert_eq!(solid.faces, walls, "the finished part's faces");
        let exact = WIDTH * DEPTH * HEIGHT - PI * RADIUS * RADIUS * depth;
        assert!(
            (solid.volume - exact).abs() < 1e-6 * exact,
            "analytic volume {} is not {exact}",
            solid.volume
        );
        // The tool did not stay behind as a second body: the rebuild holds the
        // plate, the tool and the result, and the document draws only the body.
        assert_eq!(
            solid.shapes, 3,
            "the intermediates are addressable, not drawn"
        );
        let bore = solid
            .by_role
            .iter()
            .find(|(role, _)| role.starts_with("side of") && role.contains(&cut.to_string()))
            .map(|(_, surfaces)| surfaces.clone())
            .expect("the bore is named under the cut");
        assert_eq!(
            bore,
            vec![ferritecad_kernel::FaceSurface::Cylinder { radius: RADIUS }],
            "the bore is analytic and the radius the tool had"
        );
        // The part's own faces are still named, and under the cut they are the
        // faces the finished part has.
        assert!(
            solid
                .by_role
                .keys()
                .any(|role| role.starts_with("carried cap Start of")),
            "{:?}",
            solid.by_role.keys().collect::<Vec<_>>()
        );

        // Real cache Miss then Hit: the same geometry either way, and the body
        // is the cut rather than the extrusion it was before.
        let cache = copy.with_extension("fcad-cache");
        let miss = analytic_with_cache(&copy, Some((&cache, ferritecad_eval::CacheOutcome::Miss)));
        let hit = analytic_with_cache(&copy, Some((&cache, ferritecad_eval::CacheOutcome::Hit)));
        for warm in [&miss, &hit] {
            assert_eq!(warm.faces, walls, "a cached rebuild is a different solid");
            assert!((warm.volume - exact).abs() < 1e-6 * exact);
            assert_eq!(
                warm.by_role.keys().collect::<Vec<_>>(),
                solid.by_role.keys().collect::<Vec<_>>()
            );
        }

        // And the exported bytes, read independently.
        let stl = f.root.path().join(format!("{name}.stl"));
        check_cut_mesh(&mesh(&copy, &stl), CENTER, RADIUS, depth);
        let fbx = f.root.path().join(format!("{name}.fbx"));
        let written = cli()
            .arg("export-fbx")
            .arg(&copy)
            .arg("-o")
            .arg(&fbx)
            .arg("--json")
            .output()
            .expect("FBX");
        let report = reply(written, "export-fbx", 0);
        assert_eq!(report["result"]["complete"], true);
        assert_eq!(
            report["result"]["geometries"], 1,
            "one finished body, not the tool and two intermediates"
        );
        keep(&fbx, &format!("cut-{name}-cli.fbx"));

        // §26F admits only the original Line base; analytic and constraint
        // editors keep their four-object boundary.
        let after = inspect(&copy);
        for sketch in after["sketches"].as_array().expect("sketches") {
            assert_eq!(
                sketch["editable"],
                sketch["sketch_id"] == f.catalog["sketches"][0]["sketch_id"]
            );
        }
        assert!(
            after["sketches"]
                .as_array()
                .expect("sketches")
                .iter()
                .all(|s| s["circle_edit"]["available"] == false
                    && s["constraint_edit"]["available"] == false),
            "a document with a cut is not one the profile editors accept: {:?}",
            after["sketches"]
        );
        assert_eq!(after["bodies"][0]["cut_edit"]["available"], true);
    }
}

/// Changing the input invalidates the right part of the chain.
#[test]
fn native_changing_the_part_changes_the_cut_and_not_the_other_way_round() {
    if !native() {
        return;
    }
    let f = Fixture::plate();
    let saved = stored(&f.source);
    let copy = f.root.path().join("pocket.fcad");
    f.ask(CENTER, RADIUS, 4.0);
    assert!(
        reply(f.cut(&copy).output().expect("process"), OP, 0)["ok"]
            .as_bool()
            .expect("ok")
    );

    // Warm the cache for the copy as it stands.
    let cache = copy.with_extension("fcad-cache");
    analytic_with_cache(&copy, Some((&cache, ferritecad_eval::CacheOutcome::Miss)));
    let before = analytic_with_cache(&copy, Some((&cache, ferritecad_eval::CacheOutcome::Hit)));

    // The earlier feature is what the cut consumes, so changing its height
    // changes both. The existing height edit is the operation that does it.
    let catalog = inspect(&copy);
    let feature = catalog["features"]
        .as_array()
        .expect("features")
        .iter()
        .find(|feature| feature["distance_mm"] == HEIGHT)
        .expect("the part's own extrusion")["feature_id"]
        .as_str()
        .expect("UUID")
        .to_owned();
    let taller = f.root.path().join("taller.fcad");
    let raised = cli()
        .arg("edit-extrude")
        .arg(&copy)
        .args(["--feature", &feature])
        .args(["--distance-mm", "16"])
        .arg("-o")
        .arg(&taller)
        .arg("--json")
        .output()
        .expect("edit-extrude");
    assert!(raised.status.success(), "{raised:?}");

    // The pocket is still 4 mm deep and the part is 16 mm tall, so the solid
    // is a different one. A cache that keyed the cut by its own numbers alone
    // would have handed back the old one.
    let after = analytic(&taller);
    let exact = WIDTH * DEPTH * 16.0 - PI * RADIUS * RADIUS * 4.0;
    assert!(
        (after.volume - exact).abs() < 1e-6 * exact,
        "the taller part measures {} rather than {exact}",
        after.volume
    );
    assert!(
        (after.volume - before.volume).abs() > 1.0,
        "the chain did not rebuild"
    );
    // Every identity survived the height edit.
    let now = stored(&taller);
    assert_eq!(now.body, saved.body);
    assert_eq!(now.objects, 6);
}

/// A cut that cannot be what it claims publishes nothing.
#[test]
fn native_cut_refusals_are_atomic() {
    if !native() {
        return;
    }
    let f = Fixture::plate();
    let before = std::fs::read(&f.source).expect("source bytes");
    let never = f.root.path().join("never.fcad");
    f.ask(CENTER, RADIUS, 4.0);
    let names = entries(f.root.path());

    // A destination that is the source, a destination that exists, and a
    // hard link to the source: each with every other argument correct.
    let taken = f.root.path().join("taken.fcad");
    std::fs::write(&taken, b"another process owns this").expect("occupied");
    let alias = f.root.path().join("alias.fcad");
    std::fs::hard_link(&f.source, &alias).expect("hard link");
    for (destination, why) in [
        (f.source.clone(), "the source is not its own output"),
        (taken.clone(), "an occupied output is not replaced"),
        (alias.clone(), "an alias of the source is the source"),
    ] {
        let refused = reply(f.cut(&destination).output().expect("process"), OP, 2);
        assert_eq!(refused["error"]["kind"], "input", "{why}: {refused:?}");
    }
    assert_eq!(
        std::fs::read(&taken).expect("the occupied file"),
        b"another process owns this"
    );

    // A cut that would leave nothing, refused by the kernel contract rather
    // than published as an empty result. The policy above refuses a tool that
    // reaches the wall, so this asks for one the size of the part itself.
    f.ask([30.0, 20.0], 40.0, HEIGHT);
    let refused = reply(f.cut(&never).output().expect("process"), OP, 2);
    assert!(
        matches!(
            refused["error"]["kind"].as_str(),
            Some("input" | "unsupported" | "kernel")
        ),
        "{refused:?}"
    );

    std::fs::remove_file(&alias).expect("clears the link");
    std::fs::remove_file(&taken).expect("clears the occupied file");
    let mut expected = names;
    expected.retain(|name| name != "alias.fcad" && name != "taken.fcad");
    assert_eq!(entries(f.root.path()), expected, "a refusal left something");
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
}

/// A build with a kernel and no solver cuts an unconstrained part exactly as
/// one with both does: nothing here asks a solver anything.
#[cfg(not(feature = "planegcs"))]
#[test]
fn occt_without_solver_cuts_an_unconstrained_part() {
    if !ferritecad_occt::is_available() {
        eprintln!("skipped: this gate is for a build with OCCT and no planegcs");
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        return;
    }
    assert!(
        !ferritecad_eval::solver_available(),
        "mixed gate requires no solver"
    );
    let f = Fixture::plate();
    let copy = f.root.path().join("holed.fcad");
    f.ask(CENTER, RADIUS, HEIGHT);
    assert!(
        reply(f.cut(&copy).output().expect("process"), OP, 0)["ok"]
            .as_bool()
            .expect("ok")
    );
    let solid = analytic(&copy);
    assert_eq!(solid.faces, 7);
    let exact = WIDTH * DEPTH * HEIGHT - PI * RADIUS * RADIUS * HEIGHT;
    assert!((solid.volume - exact).abs() < 1e-6 * exact);
}

/// The cut's cache entry moves when what it cut moves.
///
/// One document, one sidecar, and a change made in place, which is the only
/// arrangement in which a stale entry could actually be served. A key that
/// named only the tool would hand back the old solid here, and the volume would
/// be the one the part used to have.
#[test]
fn native_the_cut_is_keyed_by_what_it_cut() {
    if !native() {
        return;
    }
    let f = Fixture::plate();
    let copy = f.root.path().join("pocket.fcad");
    f.ask(CENTER, RADIUS, 4.0);
    assert!(
        reply(f.cut(&copy).output().expect("process"), OP, 0)["ok"]
            .as_bool()
            .expect("ok")
    );
    let cache = copy.with_extension("fcad-cache");

    let rebuild = |expected: &[(&str, ferritecad_eval::CacheOutcome)]| -> f64 {
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
        built.release_all(&mut kernel);
        doc.close().expect("close");
        volume
    };

    use ferritecad_eval::CacheOutcome::{Hit, Miss};
    let first = rebuild(&[("plate", Miss), ("cut", Miss)]);
    let warm = rebuild(&[("plate", Hit), ("cut", Hit)]);
    assert_eq!(first, warm, "a warm rebuild is a different solid");

    // Change the part in place. The tool is untouched, so an entry keyed only
    // by the tool would still be found.
    let mut document = Document::open(&copy).expect("opens for editing");
    let (plate_feature, mut feature) = document
        .objects()
        .expect("objects")
        .into_iter()
        .find_map(|object| match object.payload {
            ObjectPayload::Extrude(feature) if feature.previous.is_none() => {
                Some((object.id, feature))
            }
            _ => None,
        })
        .expect("the part's own extrusion");
    feature.end_condition = ferritecad_document::EndCondition::Blind {
        distance: ferritecad_document::Expression::constant(16.0).expect("a height"),
    };
    document
        .write(|w| {
            w.put_object(
                plate_feature,
                None,
                2,
                Some("Extrude1"),
                &ObjectPayload::Extrude(feature),
            )
        })
        .expect("raises the part");
    document.close().expect("closes");

    let taller = rebuild(&[("plate", Miss), ("cut", Miss)]);
    let exact = WIDTH * DEPTH * 16.0 - PI * RADIUS * RADIUS * 4.0;
    assert!(
        (taller - exact).abs() < 1e-6 * exact,
        "the cut was served from an entry that did not depend on what it cut: {taller}"
    );
}

#[path = "support/sequential_circular_cuts.rs"]
mod sequential;
