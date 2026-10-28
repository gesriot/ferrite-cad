// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
#[path = "support/names.rs"]
mod names;
#[path = "support/transaction.rs"]
mod transaction;
use ferritecad_document::{Document, ObjectPayload, SketchGeometry};
use ferritecad_kernel::{FaceSurface, GeometryKernel, OperationContext, TessellationParams};
use serde_json::{Value, json};
use std::{
    f64::consts::PI,
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;
const OP: &str = "edit";
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

fn native() -> bool {
    if ferritecad_occt::is_available() {
        true
    } else {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
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

    fn ask(&self, center: [f64; 2], radius: f64) {
        write(
            &self.request,
            &json!({"request_version":1,"changes":[transaction::circle(
                &self.catalog["sketches"][0]["sketch_id"], &self.catalog["sketches"][0]["curves"][0]["curve_id"], center, radius
            )]}),
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

/// The four objects a circle extrusion is, written without a kernel.
fn write_circle_document(path: &Path, center: [f64; 2], radius: f64, height: f64) {
    use ferritecad_document::{
        Body, DatumPlane, Dependency, DependencyRole, EndCondition, Expression, Extrude, Point2,
        Sketch, SketchCurve, SolidOperation,
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
    refs: crate::names::Names,
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
    let refs = crate::names::snapshot(&d);
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
    crate::names::assert_primary(&built);
    for (_, input, handle) in crate::names::root_faces(&d, &built) {
        let surface = kernel.face_surface(handle).expect("surface");
        match input {
            ferritecad_kernel::NamingInput::Curve(_) => side.push(surface),
            ferritecad_kernel::NamingInput::Cap { .. } => caps.push(surface),
            _ => panic!("unexpected root"),
        }
        stats = Some(kernel.shape_stats(handle.shape()).expect("stats"));
        shape = Some(handle.shape());
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
    assert_eq!(after.refs, original.refs, "same primary name digests");
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
    let r = transaction::constant(
        &both,
        &Document::open_read_only(&both)
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
    let curve = &catalog["sketches"][0]["curves"][0];
    let saved = &curve["geometry"];
    assert_eq!(saved["kind"], "circle");
    assert_eq!(saved["center_mm"], json!([12.0, -7.0]));
    assert_eq!(saved["radius_mm"], 10.0);
    write(
        &f.request,
        &json!({"request_version":1,"changes":[transaction::circle(&catalog["sketches"][0]["sketch_id"], &curve["curve_id"], [-3.5,4.25], 6.75)]}),
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
