// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::{Document, ObjectPayload, SemanticRole, SketchGeometry};
use ferritecad_kernel::{FaceSurface, GeometryKernel, OperationContext, TessellationParams};
use serde_json::{Value, json};
use std::{
    f64::consts::PI,
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;
const OP: &str = "edit-circle";
/// Explicit, so the mesh tolerance below is a property of this test and not of
/// whatever the exporter happens to default to.
const LINEAR_DEFLECTION: f64 = 0.01;

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
    assert_eq!(v.get("error").is_some(), code != 0);
    assert_eq!(v.get("result").is_some(), code == 0);
    v
}
fn inspect(path: &Path) -> Value {
    reply(
        cli()
            .arg("inspect")
            .arg(path)
            .arg("--json")
            .output()
            .expect("inspect"),
        "inspect",
        0,
    )["result"]
        .clone()
}
fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).expect("JSON")).expect("write");
}
fn entries(path: &Path) -> Vec<std::ffi::OsString> {
    let mut e: Vec<_> = std::fs::read_dir(path)
        .expect("directory")
        .map(|e| e.expect("entry").file_name())
        .collect();
    e.sort();
    e
}
fn native() -> bool {
    if ferritecad_occt::is_available() {
        true
    } else {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: no OCCT for saved Circle geometry");
        false
    }
}
/// Keeps a published artefact where a CI step hands it to the pinned reader.
fn keep(path: &Path, name: &str) {
    let Some(dir) = std::env::var_os("FCAD_CIRCLE_ARTIFACTS") else {
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
    /// One off-centre circle, written through the document API.
    ///
    /// Not through `create-circle-extrude`: that command checks the geometry
    /// with a kernel before it publishes, and discovery and the request
    /// protocol have to be testable in a build that has no kernel at all. The
    /// native gate below also edits a document that command really made.
    fn circle() -> Self {
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("круг space.fcad");
        let request = root.path().join("request.json");
        write_circle_document(&source, [12., -7.], 10., 15.);
        let catalog = inspect(&source);
        let f = Self {
            root,
            source,
            request,
            catalog,
        };
        f.ask([-3.5, 4.25], 6.75);
        f
    }
    fn saved(&self) -> &Value {
        &self.catalog["sketches"][0]["circle_edit"]["circle"]
    }
    fn ask(&self, center: [f64; 2], radius: f64) {
        write(
            &self.request,
            &json!({"request_version":1,"curve_id":self.saved()["curve_id"],
                    "center_mm":center,"radius_mm":radius}),
        );
    }
    fn edit(&self, output: &Path) -> Command {
        self.edit_from(&self.source, &self.catalog, output)
    }
    fn edit_from(&self, source: &Path, catalog: &Value, output: &Path) -> Command {
        let mut c = cli();
        c.arg(OP)
            .arg(source)
            .arg("--sketch")
            .arg(catalog["sketches"][0]["sketch_id"].as_str().expect("id"))
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

/// The four objects a circle extrusion is, written without a kernel.
fn write_circle_document(path: &Path, center: [f64; 2], radius: f64, height: f64) {
    use ferritecad_document::{
        Body, CapSide, DatumPlane, Dependency, DependencyRole, EndCondition, EntityKind,
        Expression, Extrude, Point2, SelectionRule, SemanticRole, Sketch, SketchCurve,
        SolidOperation, TopologyRef,
    };
    use ferritecad_types::{ObjectId, StableEntityId, Transform};
    let mut d = Document::create(path).expect("document");
    let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
    let curve = StableEntityId::new();
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
                curves: vec![SketchCurve {
                    id: curve,
                    construction: false,
                    geometry: SketchGeometry::Circle {
                        center: Point2::new(center[0], center[1])?,
                        radius,
                    },
                }],
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
        Ok(())
    })
    .expect("circle document");
    d.close().expect("close");
}

/// The one circle a published document stores, plus every identity beside it.
struct Stored {
    sketch: ferritecad_types::ObjectId,
    curve: ferritecad_types::StableEntityId,
    center: [f64; 2],
    radius: f64,
    height_mm: f64,
    refs: Vec<ferritecad_document::TopologyRef>,
}
fn stored(path: &Path) -> Stored {
    let d = Document::open_read_only(path).expect("reopen");
    let objects = d.objects().expect("objects");
    let (id, sketch) = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Sketch(s) => Some((o.id, s)),
            _ => None,
        })
        .expect("sketch");
    assert_eq!(sketch.curves.len(), 1);
    assert!(sketch.constraints.is_empty());
    let curve = &sketch.curves[0];
    let SketchGeometry::Circle { center, radius } = curve.geometry else {
        panic!("the document stores a Circle")
    };
    let height_mm = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Extrude(e) => Some(
                ferritecad_document::editable_extrude(&d, o)
                    .unwrap_or_else(|_| panic!("literal height, got {e:?}")),
            ),
            _ => None,
        })
        .expect("extrude");
    let refs = d.topology_refs().expect("refs");
    d.close().expect("close");
    Stored {
        sketch: id,
        curve: curve.id,
        center: [center.x, center.y],
        radius,
        height_mm,
        refs,
    }
}

/// What the real kernel says about the solid this document rebuilds into.
struct Analytic {
    faces: u64,
    volume: f64,
    center: [f64; 2],
    side: Vec<FaceSurface>,
    caps: Vec<FaceSurface>,
}
fn analytic(path: &Path) -> Analytic {
    analytic_with_cache(path, None)
}
fn analytic_with_cache(
    path: &Path,
    cache: Option<(&Path, ferritecad_eval::CacheOutcome)>,
) -> Analytic {
    let d = Document::open_read_only(path).expect("reopen");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = if let Some((path, expected)) = cache {
        let mut cache = ferritecad_document::CacheStore::open(
            path,
            d.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("cache");
        let (built, events) = ferritecad_eval::rebuild_cached(
            &d,
            &mut kernel,
            &mut cache,
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
        ferritecad_eval::rebuild_cold(&d, &mut kernel, &OperationContext::default())
            .expect("cold rebuild")
    };
    assert_eq!(built.shape_count(), 1);
    let mut side = Vec::new();
    let mut caps = Vec::new();
    let mut stats = None;
    let mut shape = None;
    for reference in &d.topology_refs().expect("refs") {
        let resolved = built.resolve(reference).expect("resolve");
        assert!(
            !resolved.is_empty(),
            "{:?} resolved to nothing after a cold reopen",
            reference.output_role
        );
        for handle in &resolved {
            let surface = kernel.face_surface(*handle).expect("surface");
            match reference.output_role {
                SemanticRole::ExtrudeSide { .. } => side.push(surface),
                SemanticRole::ExtrudeCap { .. } => caps.push(surface),
                _ => panic!("unexpected role"),
            }
            stats = Some(kernel.shape_stats(handle.shape()).expect("stats"));
            shape = Some(handle.shape());
        }
    }
    let (faces, volume) = stats.expect("a resolved reference names the solid");
    let mesh = kernel
        .tessellate(
            shape.expect("solid"),
            &TessellationParams::new(LINEAR_DEFLECTION, 0.5, false).expect("params"),
            &OperationContext::default(),
        )
        .expect("cached/cold solid mesh");
    let center = std::array::from_fn(|axis| {
        let lo = mesh
            .positions
            .chunks_exact(3)
            .map(|p| f64::from(p[axis]))
            .fold(f64::INFINITY, f64::min);
        let hi = mesh
            .positions
            .chunks_exact(3)
            .map(|p| f64::from(p[axis]))
            .fold(f64::NEG_INFINITY, f64::max);
        (lo + hi) / 2.
    });
    built.release_all(&mut kernel);
    d.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0, "every handle was released");
    Analytic {
        faces,
        volume,
        center,
        side,
        caps,
    }
}

/// An independent reading of the exported mesh at an explicit deflection.
///
/// The base ring is measured as a polygon: its own vertices, its own perimeter
/// and its own bound box. Nothing here divides a triangle count to guess how
/// many sides the exporter chose.
struct Mesh {
    lo: [f64; 3],
    hi: [f64; 3],
    perimeter: f64,
    sides: usize,
    base: Vec<[f64; 2]>,
}
fn mesh(path: &Path, out: &Path) -> Mesh {
    let r = cli()
        .arg("export-stl")
        .arg(path)
        .arg("-o")
        .arg(out)
        .arg("--linear-deflection")
        .arg(LINEAR_DEFLECTION.to_string())
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
    let mut base: Vec<[f64; 2]> = Vec::new();
    for t in b[84..].chunks_exact(50) {
        for i in 0..3 {
            let mut p = [0.; 3];
            for (j, value) in p.iter_mut().enumerate() {
                let k = 12 + 12 * i + 4 * j;
                *value = f64::from(f32::from_le_bytes(
                    t[k..k + 4].try_into().expect("coordinate"),
                ));
                assert!(value.is_finite(), "finite STL coordinate");
                lo[j] = lo[j].min(*value);
                hi[j] = hi[j].max(*value);
            }
            if p[2].abs() < 1e-6 {
                let xy = [p[0], p[1]];
                if !base
                    .iter()
                    .any(|q| (q[0] - xy[0]).hypot(q[1] - xy[1]) < 1e-6)
                {
                    base.push(xy);
                }
            }
        }
    }
    assert!(base.len() >= 3, "nonempty base perimeter");
    // Order the base ring around its own centroid, then measure it.
    let cx = base.iter().map(|p| p[0]).sum::<f64>() / base.len() as f64;
    let cy = base.iter().map(|p| p[1]).sum::<f64>() / base.len() as f64;
    base.sort_by(|a, b| {
        (a[1] - cy)
            .atan2(a[0] - cx)
            .total_cmp(&(b[1] - cy).atan2(b[0] - cx))
    });
    let perimeter = base
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = base[(i + 1) % base.len()];
            (q[0] - p[0]).hypot(q[1] - p[1])
        })
        .sum();
    Mesh {
        lo,
        hi,
        perimeter,
        sides: base.len(),
        base,
    }
}
/// The measured circle the mesh draws, against the analytic one it approximates.
fn mesh_matches(m: &Mesh, center: [f64; 2], radius: f64, height: f64) {
    // Check every actual chord; a total side count cannot bound the largest gap.
    let mut angles: Vec<_> = m
        .base
        .iter()
        .map(|p| {
            let delta = [p[0] - center[0], p[1] - center[1]];
            assert!((delta[0].hypot(delta[1]) - radius).abs() < 1e-4);
            delta[1].atan2(delta[0])
        })
        .collect();
    angles.sort_by(f64::total_cmp);
    assert_eq!(angles.len(), m.sides);
    for i in 0..angles.len() {
        let next = if i + 1 == angles.len() {
            angles[0] + 2. * PI
        } else {
            angles[i + 1]
        };
        let gap = next - angles[i];
        assert!(gap > 0. && gap < PI);
        assert!(radius * (1. - (gap / 2.).cos()) <= LINEAR_DEFLECTION + 1e-4);
    }
    let exact = 2. * PI * radius;
    assert!(
        m.perimeter <= exact && m.perimeter > exact * (1. - 1e-3),
        "measured perimeter {} is not just inside {exact}",
        m.perimeter
    );
    for (j, (middle, half)) in [
        (center[0], radius),
        (center[1], radius),
        (height / 2., height / 2.),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            m.lo[j] >= middle - half - 1e-4 && m.hi[j] <= middle + half + 1e-4,
            "axis {j} mesh [{}, {}] escapes the analytic extent",
            m.lo[j],
            m.hi[j]
        );
        assert!(
            (m.hi[j] - m.lo[j]) > 2. * half - 0.05,
            "axis {j} mesh extent {} is far from {}",
            m.hi[j] - m.lo[j],
            2. * half
        );
    }
}

/// Every table of a document, as named columns, for comparing two publications.
type Rows = Vec<Vec<rusqlite::types::Value>>;
fn tables(path: &Path) -> std::collections::BTreeMap<String, (Vec<String>, Rows)> {
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
fn only_the_circle_row_changed(source: &Path, copy: &Path, sketch: ferritecad_types::ObjectId) {
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
                    // The one payload this edit rewrites, and its hash.
                    "objects" => {
                        a.contains(&selected) && matches!(column, "payload" | "payload_hash")
                    }
                    // A copy is written at a later instant than its source.
                    "meta" => column == "modified_at",
                    _ => false,
                };
                assert!(allowed, "{table}.{column} changed to {:?}", b[i]);
            }
        }
    }
}

#[test]
fn circle_discovery_and_protocol_without_kernel() {
    let f = Fixture::circle();
    let out = f.root.path().join("copy.fcad");
    let before = std::fs::read(&f.source).expect("bytes");
    let names = entries(f.root.path());
    let sketch = &f.catalog["sketches"][0];

    // Discovery is typed, from this reading, and says nothing new about the
    // Line editor, which still refuses a circle exactly as it did.
    let discovery = &sketch["circle_edit"];
    assert_eq!(discovery["available"], true);
    assert!(discovery["refusal"].is_null() && discovery["document_refusal"].is_null());
    assert_eq!(discovery["circle"]["center_mm"], json!([12.0, -7.0]));
    assert_eq!(discovery["circle"]["radius_mm"], 10.0);
    assert_eq!(discovery["circle"]["height_mm"], 15.0);
    assert!(discovery["circle"]["curve_id"].is_string());
    assert_eq!(sketch["editable"], false, "the Line editor is unchanged");
    assert!(sketch["vertices"].is_null(), "no invented polygon");
    // The constraint editor accepts this Sketch since §25N added the circle
    // families, and says so about a circle rather than about Lines: it offers
    // the analytic circle and no invented segment.
    assert_eq!(sketch["constraint_edit"]["available"], true);
    assert_eq!(
        sketch["constraint_edit"]["curves"],
        json!([]),
        "a circle profile offers no Line"
    );
    assert_eq!(
        sketch["constraint_edit"]["circles"][0]["curve_id"], discovery["circle"]["curve_id"],
        "both discoveries name the same stored circle"
    );
    assert_eq!(
        sketch["constraint_edit"]["circles"][0]["radius_mm"], 10.0,
        "the stored radius, which is the solver's starting guess"
    );
    assert_eq!(sketch["constraint_edit"]["constraints"], json!([]));

    // Wire-level refusals are decided before anything opens a kernel; the ones
    // marked `Domain` are decided against the real document, so a build with no
    // kernel refuses them for that earlier reason instead. Either way nothing
    // is published.
    #[derive(Clone, Copy, PartialEq)]
    enum Stage {
        Wire,
        Domain,
    }
    use Stage::{Domain, Wire};
    for (stage, bad) in [
        // The version field has one spelling and one value.
        (
            Wire,
            json!({"request_version":2,"curve_id":f.saved()["curve_id"],"center_mm":[0,0],"radius_mm":1}),
        ),
        (
            Wire,
            json!({"schema_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[0,0],"radius_mm":1}),
        ),
        // Every field is required, and nothing else is accepted.
        (
            Wire,
            json!({"request_version":1,"center_mm":[0,0],"radius_mm":1}),
        ),
        (
            Wire,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"radius_mm":1}),
        ),
        (
            Wire,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[0,0]}),
        ),
        (
            Wire,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[0,0],"radius_mm":1,"height_mm":9}),
        ),
        (
            Wire,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[0,0],"radius_mm":1,"vertices":[]}),
        ),
        // A centre is exactly two numbers; a radius is one positive number.
        (
            Wire,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[0],"radius_mm":1}),
        ),
        (
            Wire,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[0,0,0],"radius_mm":1}),
        ),
        (
            Wire,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":["0","0"],"radius_mm":1}),
        ),
        (
            Domain,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[0,0],"radius_mm":0}),
        ),
        (
            Domain,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[0,0],"radius_mm":-1}),
        ),
        (
            Wire,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[0,0],"radius_mm":"1"}),
        ),
        (
            Domain,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[0,0],"radius_mm":2e6}),
        ),
        (
            Domain,
            json!({"request_version":1,"curve_id":f.saved()["curve_id"],"center_mm":[2e6,0],"radius_mm":1}),
        ),
        // A curve UUID is a UUID, and must be the saved one.
        (
            Wire,
            json!({"request_version":1,"curve_id":"not-a-uuid","center_mm":[0,0],"radius_mm":1}),
        ),
        (
            Wire,
            json!({"request_version":1,"curve_id":null,"center_mm":[0,0],"radius_mm":1}),
        ),
        (
            Domain,
            json!({"request_version":1,"curve_id":"00000000-0000-7000-8000-000000000000","center_mm":[0,0],"radius_mm":1}),
        ),
    ] {
        write(&f.request, &bad);
        let v = reply(f.edit(&out).output().expect("refusal"), OP, 2);
        let expected = if bad["request_version"] == 2
            || (stage == Domain && !ferritecad_occt::is_available())
        {
            "unsupported"
        } else {
            "input"
        };
        assert_eq!(v["error"]["kind"], expected, "{bad}");
        assert!(!out.exists());
        assert_eq!(
            entries(f.root.path()),
            names,
            "nothing published, no scratch"
        );
    }
    // JSON cannot spell these, so they arrive as raw text.
    let valid = json!({"request_version":1,"curve_id":f.saved()["curve_id"],
                       "center_mm":[1.0,2.0],"radius_mm":3.0})
    .to_string();
    for (field, bad) in [
        ("\"radius_mm\":3.0", "\"radius_mm\":NaN"),
        ("\"radius_mm\":3.0", "\"radius_mm\":1e999"),
        ("\"center_mm\":[1.0,2.0]", "\"center_mm\":[NaN,2.0]"),
        ("\"center_mm\":[1.0,2.0]", "\"center_mm\":[1.0,-1e999]"),
    ] {
        std::fs::write(&f.request, valid.replace(field, bad)).expect("bad number");
        assert_eq!(
            reply(f.edit(&out).output().expect("bad number"), OP, 2)["error"]["kind"],
            "input",
            "{bad}"
        );
        assert!(!out.exists());
    }
    // An oversize request is refused before it is parsed.
    std::fs::write(&f.request, vec![b' '; 65537]).expect("oversize");
    assert_eq!(
        reply(f.edit(&out).output().expect("oversize"), OP, 2)["error"]["kind"],
        "input"
    );

    f.ask([-3.5, 4.25], 6.75);
    // A foreign Sketch UUID, and a stale version, each with everything else
    // valid. Both are decided inside the job, so a build with no kernel refuses
    // them before reaching that point; what matters either way is the refusal.
    let domain = if ferritecad_occt::is_available() {
        "input"
    } else {
        "unsupported"
    };
    let mut foreign = f.catalog.clone();
    foreign["sketches"][0]["sketch_id"] = json!(ferritecad_types::ObjectId::new().to_string());
    assert_eq!(
        reply(
            f.edit_from(&f.source, &foreign, &out)
                .output()
                .expect("foreign Sketch"),
            OP,
            2
        )["error"]["kind"],
        domain
    );
    let mut stale = f.catalog.clone();
    stale["content_version"] = json!(ferritecad_types::ContentHash::of_bytes(b"stale").to_string());
    let v = reply(
        f.edit_from(&f.source, &stale, &out)
            .output()
            .expect("stale version"),
        OP,
        2,
    );
    assert_eq!(v["error"]["kind"], domain);
    if ferritecad_occt::is_available() {
        assert!(
            v["error"]["message"]
                .as_str()
                .expect("message")
                .contains("changed since it was read")
        );
    }

    // No-clobber, and the source may not be its own destination or an alias.
    std::fs::write(&out, b"occupied").expect("occupied");
    assert_eq!(
        reply(f.edit(&out).output().expect("occupied"), OP, 2)["error"]["kind"],
        domain
    );
    assert_eq!(std::fs::read(&out).expect("kept"), b"occupied");
    std::fs::remove_file(&out).expect("clean up");
    assert_eq!(
        reply(f.edit(&f.source).output().expect("itself"), OP, 2)["error"]["kind"],
        domain
    );
    let hard = f.root.path().join("hard.fcad");
    std::fs::hard_link(&f.source, &hard).expect("hard link");
    assert_eq!(
        reply(f.edit(&hard).output().expect("hard link"), OP, 2)["error"]["kind"],
        domain
    );
    #[cfg(unix)]
    {
        let soft = f.root.path().join("soft.fcad");
        std::os::unix::fs::symlink(&f.source, &soft).expect("symlink");
        assert_eq!(
            reply(f.edit(&soft).output().expect("symlink"), OP, 2)["error"]["kind"],
            domain
        );
    }

    // Usage stays clap text, and a lost report after a refusal is still 2/7.
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
    assert!(usage.stdout.is_empty());
    let missing = f.root.path().join("missing.json");
    let mut c = cli();
    c.arg(OP)
        .arg(&f.source)
        .arg("--sketch")
        .arg(f.catalog["sketches"][0]["sketch_id"].as_str().expect("id"))
        .arg("--expect-version")
        .arg(f.catalog["content_version"].as_str().expect("version"))
        .arg("--request")
        .arg(&missing)
        .arg("-o")
        .arg(&out)
        .arg("--json");
    assert_eq!(
        reply(
            c.stderr(pipe::closed_pipe())
                .output()
                .expect("closed stderr"),
            OP,
            2
        )["error"]["kind"],
        "io"
    );
    let mut c = cli();
    c.arg(OP)
        .arg(&f.source)
        .arg("--sketch")
        .arg(f.catalog["sketches"][0]["sketch_id"].as_str().expect("id"))
        .arg("--expect-version")
        .arg(f.catalog["content_version"].as_str().expect("version"))
        .arg("--request")
        .arg(&missing)
        .arg("-o")
        .arg(&out)
        .arg("--json");
    assert_eq!(
        c.stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("both closed")
            .code(),
        Some(7)
    );

    if !ferritecad_occt::is_available() {
        // Discovery worked above without a kernel; applying an edit cannot.
        f.ask([-3.5, 4.25], 6.75);
        let v = reply(f.edit(&out).output().expect("stub"), OP, 2);
        assert_eq!(v["error"]["kind"], "unsupported");
        assert!(!out.exists());
    }
    assert_eq!(std::fs::read(&f.source).expect("source"), before);
}

#[cfg(unix)]
#[test]
fn circle_edit_non_utf8_paths_refuse_before_reading() {
    use std::os::unix::ffi::OsStringExt;
    let f = Fixture::circle();
    let out = f.root.path().join(std::ffi::OsString::from_vec(vec![
        b'c', 255, b'.', b'f', b'c', b'a', b'd',
    ]));
    let v = reply(f.edit(&out).output().expect("non-UTF-8"), OP, 2);
    assert_eq!(v["error"]["kind"], "input");
    assert!(!out.exists());
}

#[test]
fn native_circle_edit_moves_and_resizes_only_the_named_circle() {
    if !native() {
        return;
    }
    let f = Fixture::circle();
    let before = std::fs::read(&f.source).expect("source");
    let original = stored(&f.source);
    assert_eq!(original.center, [12., -7.]);
    assert_eq!(original.radius, 10.);
    assert_eq!(original.height_mm, 15.);

    // Both numbers at once.
    let both = f.root.path().join("both.fcad");
    f.ask([-3.5, 4.25], 6.75);
    let published = reply(f.edit(&both).output().expect("edit"), OP, 0)["result"].clone();
    assert_eq!(published["destination"], both.to_str().expect("UTF-8"));
    assert_eq!(
        published["sketch_id"],
        original.sketch.to_string(),
        "the reply names the Sketch that was edited"
    );
    assert_eq!(published["curve_id"], original.curve.to_string());
    assert_eq!(published["document_id"], {
        let d = Document::open_read_only(&both).expect("copy");
        let id = d.meta().document_id.to_string();
        d.close().expect("close");
        id
    });
    let after = stored(&both);
    assert_eq!(after.curve, original.curve, "the circle keeps its UUID");
    assert_eq!(after.sketch, original.sketch);
    assert_eq!(after.center, [-3.5, 4.25]);
    assert_eq!(after.radius, 6.75);
    assert_eq!(
        after.height_mm, 15.,
        "the height is not this edit's to change"
    );
    assert_eq!(after.refs, original.refs, "same reference UUIDs and roles");
    only_the_circle_row_changed(&f.source, &both, original.sketch);

    let real = analytic(&both);
    assert_eq!(real.faces, 3);
    assert_eq!(real.side, vec![FaceSurface::Cylinder { radius: 6.75 }]);
    assert_eq!(real.caps, vec![FaceSurface::Plane, FaceSurface::Plane]);
    let exact = PI * 6.75_f64.powi(2) * 15.;
    assert!(
        (real.volume - exact).abs() < 1e-6 * exact,
        "analytic volume {} is not {exact}",
        real.volume
    );
    let m = mesh(&both, &f.root.path().join("both.stl"));
    mesh_matches(&m, [-3.5, 4.25], 6.75, 15.);
    let fbx = f.root.path().join("both.fbx");
    let r = cli()
        .arg("export-fbx")
        .arg(&both)
        .arg("-o")
        .arg(&fbx)
        .arg("--json")
        .output()
        .expect("FBX");
    assert!(r.status.success(), "{r:?}");
    let report: Value = serde_json::from_slice(&r.stdout).expect("JSON");
    assert_eq!(report["result"]["complete"], true);
    keep(&fbx, "circle-edited.fbx");

    // One component at a time, so losing the other could not pass unnoticed.
    let moved = f.root.path().join("moved.fcad");
    f.ask([-3.5, 4.25], 10.);
    reply(f.edit(&moved).output().expect("centre only"), OP, 0);
    let only_center = stored(&moved);
    assert_eq!(only_center.center, [-3.5, 4.25]);
    assert_eq!(only_center.radius, 10., "the radius was not asked to move");
    assert_eq!(
        analytic(&moved).side,
        vec![FaceSurface::Cylinder { radius: 10. }]
    );
    mesh_matches(
        &mesh(&moved, &f.root.path().join("moved.stl")),
        [-3.5, 4.25],
        10.,
        15.,
    );

    let resized = f.root.path().join("resized.fcad");
    f.ask([12., -7.], 6.75);
    reply(f.edit(&resized).output().expect("radius only"), OP, 0);
    let only_radius = stored(&resized);
    assert_eq!(only_radius.center, [12., -7.], "the centre did not move");
    assert_eq!(only_radius.radius, 6.75);
    mesh_matches(
        &mesh(&resized, &f.root.path().join("resized.stl")),
        [12., -7.],
        6.75,
        15.,
    );

    // Populate from the original, then reuse the same store and identities.
    // An edit must miss that entry; a second session must hit the edited shape.
    let cache = f.root.path().join("edit-cache.sqlite");
    use ferritecad_eval::CacheOutcome::{Hit, Miss};
    for (path, outcomes, center, radius) in [
        (&f.source, vec![Miss, Hit], [12., -7.], 10.),
        (&moved, vec![Miss, Hit], [-3.5, 4.25], 10.),
        (&both, vec![Miss, Hit], [-3.5, 4.25], 6.75),
    ] {
        for outcome in outcomes {
            let cached = analytic_with_cache(path, Some((&cache, outcome)));
            assert_eq!(cached.faces, 3);
            assert_eq!(cached.side, vec![FaceSurface::Cylinder { radius }]);
            assert_eq!(cached.caps, vec![FaceSurface::Plane; 2]);
            let volume = PI * radius * radius * 15.;
            assert!((cached.volume - volume).abs() < 1e-6 * volume);
            for (actual, expected) in cached.center.into_iter().zip(center) {
                assert!(
                    (actual - expected).abs() <= LINEAR_DEFLECTION + 1e-4,
                    "cached centre {actual} != {expected}"
                );
            }
        }
    }
    std::fs::remove_file(cache).expect("release owned cache");

    // The existing height edit still works on the edited copy, and the circle
    // survives it whole.
    let taller = f.root.path().join("taller.fcad");
    let feature = inspect(&both)["features"][0]["feature_id"]
        .as_str()
        .expect("feature")
        .to_owned();
    let r = cli()
        .arg("edit-extrude")
        .arg(&both)
        .arg("--feature")
        .arg(&feature)
        .arg("--distance-mm")
        .arg("25")
        .arg("-o")
        .arg(&taller)
        .arg("--json")
        .output()
        .expect("edit-extrude");
    assert!(r.status.success(), "{r:?}");
    let grown = stored(&taller);
    assert_eq!(grown.curve, original.curve, "the circle keeps its UUID");
    assert_eq!(grown.center, [-3.5, 4.25]);
    assert_eq!(grown.radius, 6.75);
    assert_eq!(grown.height_mm, 25.);
    assert_eq!(grown.refs, original.refs);
    let real = analytic(&taller);
    assert_eq!(real.side, vec![FaceSurface::Cylinder { radius: 6.75 }]);
    let exact = PI * 6.75_f64.powi(2) * 25.;
    assert!((real.volume - exact).abs() < 1e-6 * exact);

    assert_eq!(
        std::fs::read(&f.source).expect("source"),
        before,
        "the source is untouched"
    );

    // And the same edit on a document `create-circle-extrude` really made, so
    // the two slices are checked against each other and not only against this
    // suite's own writer.
    let made = f.root.path().join("made.fcad");
    write(
        &f.request,
        &json!({"schema_version":1,"center_mm":[12.0,-7.0],"radius_mm":10.0,"height_mm":15.0}),
    );
    reply(
        cli()
            .arg("create-circle-extrude")
            .arg(&f.request)
            .arg("-o")
            .arg(&made)
            .arg("--json")
            .output()
            .expect("create"),
        "create-circle-extrude",
        0,
    );
    let catalog = inspect(&made);
    let saved = &catalog["sketches"][0]["circle_edit"]["circle"];
    assert_eq!(catalog["sketches"][0]["circle_edit"]["available"], true);
    assert_eq!(saved["center_mm"], json!([12.0, -7.0]));
    assert_eq!(saved["radius_mm"], 10.0);
    write(
        &f.request,
        &json!({"request_version":1,"curve_id":saved["curve_id"],
                "center_mm":[-3.5,4.25],"radius_mm":6.75}),
    );
    let produced = f.root.path().join("made-edited.fcad");
    reply(
        f.edit_from(&made, &catalog, &produced)
            .output()
            .expect("edit a produced document"),
        OP,
        0,
    );
    let from_made = stored(&produced);
    assert_eq!(from_made.center, [-3.5, 4.25]);
    assert_eq!(from_made.radius, 6.75);
    assert_eq!(from_made.height_mm, 15.);
    only_the_circle_row_changed(&made, &produced, from_made.sketch);
    assert_eq!(
        analytic(&produced).side,
        vec![FaceSurface::Cylinder { radius: 6.75 }]
    );
    let source_bytes = std::fs::read(&made).expect("source before delivery loss");
    for both_closed in [false, true] {
        let output = f.root.path().join(format!("delivery-{both_closed}.fcad"));
        let mut command = f.edit_from(&made, &catalog, &output);
        command.stdout(pipe::closed_pipe());
        if both_closed {
            command.stderr(pipe::closed_pipe());
        }
        let lost = command.output().expect("delivery failure process");
        assert_eq!(lost.status.code(), Some(7), "{lost:?}");
        let saved = stored(&output);
        assert_eq!(saved.center, [-3.5, 4.25]);
        assert_eq!(saved.radius, 6.75);
        assert_eq!(saved.refs, from_made.refs);
        only_the_circle_row_changed(&made, &output, from_made.sketch);
        assert_eq!(std::fs::read(&made).expect("source"), source_bytes);
    }
}

#[test]
fn native_circle_edit_refusals_preserve_source_and_destinations() {
    if !native() {
        return;
    }
    let f = Fixture::circle();
    let before = std::fs::read(&f.source).expect("source");
    let names = entries(f.root.path());
    let out = f.root.path().join("never.fcad");

    // A Sketch this editor does not own: a Line polygon of the same shape.
    let polygon = f.root.path().join("polygon.fcad");
    let profile = f.root.path().join("polygon.json");
    write(
        &profile,
        &json!({"request_version":1,"points_mm":[[0,0],[10,0],[10,10],[0,10]],"height_mm":5}),
    );
    let r = cli()
        .arg("create-sketch-extrude")
        .arg(&profile)
        .arg("-o")
        .arg(&polygon)
        .arg("--json")
        .output()
        .expect("polygon");
    assert!(r.status.success(), "{r:?}");
    let catalog = inspect(&polygon);
    let discovery = &catalog["sketches"][0]["circle_edit"];
    assert_eq!(discovery["available"], false);
    assert!(discovery["circle"].is_null(), "no invented circle");
    assert!(
        discovery["refusal"]
            .as_str()
            .expect("reason")
            .contains("Circle"),
        "{discovery}"
    );
    // The Line editor still accepts it, which is the contract this must keep.
    assert_eq!(catalog["sketches"][0]["editable"], true);
    assert!(catalog["sketches"][0]["vertices"].is_array());
    write(
        &f.request,
        &json!({"request_version":1,"curve_id":catalog["sketches"][0]["vertices"][0]["curve_id"],
                "center_mm":[0,0],"radius_mm":1}),
    );
    let v = reply(
        f.edit_from(&polygon, &catalog, &out)
            .output()
            .expect("a polygon is not a circle"),
        OP,
        2,
    );
    assert_eq!(v["error"]["kind"], "unsupported");
    assert!(!out.exists());

    // A read-only document is refused by the shared copy policy.
    let readonly = f.root.path().join("readonly.fcad");
    std::fs::copy(&f.source, &readonly).expect("copy");
    let db = rusqlite::Connection::open(&readonly).expect("SQL");
    db.execute_batch("CREATE TRIGGER block AFTER UPDATE ON objects BEGIN SELECT 1; END;")
        .expect("trigger");
    drop(db);
    let locked = inspect(&readonly);
    assert_eq!(
        locked["sketches"][0]["circle_edit"]["available"], false,
        "a document-wide refusal keeps its priority"
    );
    assert!(
        locked["sketches"][0]["circle_edit"]["document_refusal"]
            .as_str()
            .is_some()
    );
    assert!(
        locked["sketches"][0]["circle_edit"]["circle"].is_object(),
        "the structure is still reported; only editing is refused"
    );
    f.ask([-3.5, 4.25], 6.75);
    let v = reply(
        f.edit_from(&readonly, &locked, &out)
            .output()
            .expect("read-only"),
        OP,
        2,
    );
    assert_eq!(v["error"]["kind"], "unsupported");
    assert!(!out.exists());

    assert_eq!(std::fs::read(&f.source).expect("source"), before);
    let mut expected = names;
    for name in ["polygon.fcad", "polygon.json", "readonly.fcad"] {
        expected.push(name.into());
    }
    expected.sort();
    assert_eq!(entries(f.root.path()), expected, "no scratch or sidecars");
}

/// Cancellation, races and cleanup on the shared copy lifecycle.
///
/// Driven against the real kernel rather than a mock, because the mock draws
/// every curve as its chord and cannot build a circle at all. The fault seams
/// are the shared operation's own progress thresholds, so what is measured
/// here is that the circle route really goes through them.
#[test]
fn native_circle_edit_cancellation_races_and_cleanup_are_atomic() {
    use ferritecad_kernel::{CancelToken, ProgressSink};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    if !native() {
        return;
    }
    for case in [
        "before", "snapshot", "rebuilt", "closed", "late", "occupied", "changed", "alias",
    ] {
        let f = Fixture::circle();
        let saved = stored(&f.source);
        let bytes = std::fs::read(&f.source).expect("source");
        let mtime = std::fs::metadata(&f.source)
            .expect("metadata")
            .modified()
            .expect("mtime");
        let destination = f.root.path().join("copy.fcad");
        let request = ferritecad_jobs::EditCircleRequest {
            source: f.source.clone(),
            expected: ferritecad_document::DocumentVersion {
                document_id: {
                    let d = Document::open_read_only(&f.source).expect("source");
                    let id = d.meta().document_id;
                    d.close().expect("close");
                    id
                },
                content: f.catalog["content_version"]
                    .as_str()
                    .expect("version")
                    .parse()
                    .expect("hash"),
            },
            sketch: saved.sketch,
            edit: ferritecad_document::CircleEdit {
                curve_id: saved.curve,
                center_mm: [-3.5, 4.25],
                radius_mm: 6.75,
            },
            destination: destination.clone(),
        };
        let cancel = CancelToken::new();
        if case == "before" {
            cancel.cancel();
        }
        let token = cancel.clone();
        let source = f.source.clone();
        let dest = destination.clone();
        let once = Arc::new(AtomicBool::new(false));
        let context = ferritecad_kernel::OperationContext::default()
            .with_cancel(cancel)
            .with_progress(ProgressSink::new(move |p| {
                let threshold = match case {
                    "snapshot" => 0.1,
                    "rebuilt" => 0.4,
                    "late" => 1.,
                    _ => 0.95,
                };
                if p != threshold || once.swap(true, Ordering::SeqCst) {
                    return;
                }
                match case {
                    "snapshot" | "rebuilt" | "closed" | "late" => token.cancel(),
                    "occupied" => std::fs::write(&dest, b"raced destination").expect("race"),
                    "changed" => {
                        let mut d = Document::open(&source).expect("source writer");
                        let mut o = d.objects().expect("objects").remove(0);
                        o.name = Some("changed during edit".into());
                        d.write(|w| {
                            w.put_object(o.id, o.parent, o.ordinal, o.name.as_deref(), &o.payload)
                        })
                        .expect("change");
                        d.close().expect("close");
                    }
                    "alias" => std::fs::hard_link(&source, &dest).expect("late alias"),
                    _ => {}
                }
            }));
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let result = ferritecad_jobs::edit_circle_copy(&request, &mut kernel, &context);
        if case == "late" {
            // Cancellation that arrives after publication is simply late.
            result.as_ref().expect(case);
            assert_eq!(stored(&destination).center, [-3.5, 4.25]);
        } else {
            let error = result.expect_err(case);
            let expected = match case {
                "before" | "snapshot" | "rebuilt" | "closed" => {
                    ferritecad_types::ErrorKind::Cancellation
                }
                _ => ferritecad_types::ErrorKind::Input,
            };
            assert_eq!(error.kind(), expected, "{case}: {error}");
        }
        assert_eq!(kernel.live_shape_count(), 0, "{case} leaked handles");
        if case != "changed" {
            assert_eq!(std::fs::read(&f.source).expect("source"), bytes, "{case}");
            assert_eq!(
                std::fs::metadata(&f.source)
                    .expect("metadata")
                    .modified()
                    .expect("mtime"),
                mtime,
                "{case}"
            );
        }
        if case == "occupied" {
            assert_eq!(
                std::fs::read(&destination).expect("destination"),
                b"raced destination"
            );
        }
        if case == "alias" {
            assert_eq!(std::fs::read(&destination).expect("alias"), bytes);
        }
        let mut expected: Vec<std::ffi::OsString> =
            vec!["круг space.fcad".into(), "request.json".into()];
        if matches!(case, "late" | "occupied" | "alias") {
            expected.push("copy.fcad".into());
        }
        expected.sort();
        assert_eq!(
            entries(f.root.path()),
            expected,
            "{case}: scratch or sidecars remained"
        );
    }
}
