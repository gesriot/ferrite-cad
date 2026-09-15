// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::{
    CapSide, Document, ObjectPayload, SemanticRole, SketchGeometry, TopologyRef,
};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use serde_json::{Value, json};
use std::{
    f64::consts::PI,
    path::Path,
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;
const OP: &str = "create-circle-extrude";
const LINEAR_MM: f64 = 0.05;
const ANGULAR_RAD: f64 = 0.1;
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).expect("JSON")).expect("request");
}
fn request(path: &Path) {
    write(
        path,
        &json!({"schema_version":1,"center_mm":[12.0,-7.0],"radius_mm":10.0,"height_mm":15.0}),
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
///
/// Named rather than numbered, so the reader loop asks for the same files on
/// every platform and a missing one is a failure rather than a silent skip.
fn keep(path: &Path, name: &str) {
    let Some(dir) = std::env::var_os("FCAD_CIRCLE_ARTIFACTS") else {
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
        eprintln!("skipped: no OCCT for analytic circle geometry");
        false
    }
}

/// The one stored circle of a published document, and every identity beside it.
struct Stored {
    curve: ferritecad_types::StableEntityId,
    center: [f64; 2],
    radius: f64,
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
    assert_eq!(sketch.curves.len(), 1, "one model curve, not a polygon");
    assert!(sketch.constraints.is_empty());
    let curve = &sketch.curves[0];
    assert!(!curve.construction, "the boundary is model geometry");
    let SketchGeometry::Circle { center, radius } = curve.geometry else {
        panic!("the document stores a Circle, not an approximation of one")
    };
    let refs = doc.topology_refs().expect("refs");
    let count = objects.len();
    doc.close().expect("close");
    Stored {
        curve: curve.id,
        center: [center.x, center.y],
        radius,
        refs,
        objects: count,
    }
}

/// What the real kernel says about the solid this document rebuilds into.
///
/// Analytic throughout: a face count, a B-Rep volume and the surface each
/// named face lies on. None of it is read from a mesh, which is the whole
/// distinction a circle makes.
struct Analytic {
    faces: u64,
    volume: f64,
    side: Vec<ferritecad_kernel::FaceSurface>,
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
        .expect("cached circle rebuild");
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
    let mut side = Vec::new();
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
                SemanticRole::ExtrudeSide { .. } => side.push(surface),
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
    }
}

/// An independent reading of the exported mesh.
///
/// A tessellation of a cylinder is an inscribed prism, so its volume is below
/// the analytic one and its bound box meets it only at the facet corners. The
/// error bound comes from explicitly requested tessellation settings, and
/// perimeter angles are read from actual vertices rather than guessed from
/// the number of cap triangles. The analytic volume is checked separately.
struct Mesh {
    triangles: usize,
    lo: [f64; 3],
    hi: [f64; 3],
    volume: f64,
    points: Vec<[f64; 3]>,
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
        let [a, b, c] = p;
        six += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    Mesh {
        triangles: count,
        lo,
        hi,
        volume: six.abs() / 6.,
        points,
    }
}

fn check_circle_mesh(m: &Mesh, center: [f64; 2], radius: f64, height: f64) {
    assert!(m.triangles >= 24);
    // A convex inscribed profile with chord error <= e contains the disk
    // r-e. This bound does not assume a particular cap triangulation.
    const ROUNDING_MM: f64 = 1e-4;
    let exact = PI * radius.powi(2) * height;
    let inside = PI * (radius - LINEAR_MM).max(0.).powi(2) * height;
    assert!(m.volume >= inside * (1. - 1e-6) && m.volume <= exact * (1. + 1e-6));
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
    assert!(angles.len() >= 3, "no circular perimeter in the actual STL");
    let mut area = 0.;
    for (i, angle) in angles.iter().enumerate() {
        let next = angles.get(i + 1).copied().unwrap_or(angles[0] + 2. * PI);
        let gap = next - angle;
        assert!(gap < PI, "the perimeter is missing a sector");
        let sagitta = radius * (1. - (gap / 2.).cos());
        assert!(sagitta <= LINEAR_MM + ROUNDING_MM, "chord error {sagitta}");
        area += radius.powi(2) * gap.sin() / 2.;
    }
    assert!(
        (m.volume - area * height).abs() <= exact * 1e-5,
        "STL volume {} does not match its measured perimeter",
        m.volume
    );
    for (j, (middle, half)) in [
        (center[0], radius),
        (center[1], radius),
        (height / 2., height / 2.),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(m.hi[j] <= middle + half + ROUNDING_MM && m.lo[j] >= middle - half - ROUNDING_MM);
        assert!(m.hi[j] - m.lo[j] >= 2. * half - 2. * LINEAR_MM - ROUNDING_MM);
    }
}

#[test]
fn circle_request_refusals_and_usage_preserve_files() {
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("circle.json");
    let out = d.path().join("out.fcad");
    request(&input);
    let missing = d.path().join("missing.json");
    let v = reply(create(&missing, &out).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "io");
    for value in [
        // The version is this request's own, and only one of it is accepted.
        json!({"schema_version":2,"center_mm":[0,0],"radius_mm":1,"height_mm":1}),
        json!({"request_version":1,"center_mm":[0,0],"radius_mm":1,"height_mm":1}),
        // Every field is required, and nothing else is accepted.
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"height_mm":1}),
        json!({"schema_version":1,"radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":1,"height_mm":1,"holes":[]}),
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":1,"height_mm":1,"points_mm":[]}),
        // A centre is exactly two numbers.
        json!({"schema_version":1,"center_mm":[0],"radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0,0],"radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":0,"radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":["0","0"],"radius_mm":1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[null,0],"radius_mm":1,"height_mm":1}),
        // A radius and a height are positive finite millimetres.
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":0,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":-1,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":"1","height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":true,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":1,"height_mm":0}),
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":1,"height_mm":-1}),
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":2e6,"height_mm":1}),
        json!({"schema_version":1,"center_mm":[2e6,0],"radius_mm":1,"height_mm":1}),
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
    let valid =
        json!({"schema_version":1,"center_mm":[0,0],"radius_mm":10,"height_mm":15}).to_string();
    for (field, bad) in [
        ("\"radius_mm\":10", "\"radius_mm\":NaN"),
        ("\"radius_mm\":10", "\"radius_mm\":1e999"),
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
fn circle_alias_and_stub_kernel_refusals_preserve_storage() {
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
        // A build with no kernel cannot prove a circle is buildable, so it
        // refuses rather than publishing a document nobody checked.
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
fn circle_json_non_utf8_path_refuses_before_reading() {
    use std::os::unix::ffi::OsStringExt;
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join(std::ffi::OsString::from_vec(vec![b'c', 255]));
    let out = d.path().join("out.fcad");
    let v = reply(create(&input, &out).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "input");
    assert!(entries(d.path()).is_empty());
}

#[test]
fn native_circle_extrude_process_analytic_geometry_and_delivery() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("круг space.json");
    let out = d.path().join("цилиндр space.fcad");
    request(&input);
    let v = reply(create(&input, &out).output().expect("process"), 0);
    assert_eq!(v["result"]["destination"], out.to_str().expect("UTF-8"));
    let saved = std::fs::read(&out).expect("source");
    let doc = Document::open_read_only(&out).expect("reopen");
    assert_eq!(
        v["result"]["document_id"],
        doc.meta().document_id.to_string()
    );
    doc.close().expect("close");

    // The document stores a circle, by centre and radius, with its own UUID.
    let first = stored(&out);
    assert_eq!(first.objects, 4, "plane, sketch, extrude, body");
    assert_eq!(first.center, [12., -7.]);
    assert_eq!(first.radius, 10.);
    assert_eq!(first.refs.len(), 3);
    let side = first
        .refs
        .iter()
        .find(|r| matches!(r.output_role, SemanticRole::ExtrudeSide { .. }))
        .expect("a side reference");
    assert_eq!(
        side.output_role,
        SemanticRole::ExtrudeSide {
            profile_segment: first.curve
        },
        "the side is named by the circle that drew it, not by a face index"
    );
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
        assert_eq!(again.curve, first.curve);
        assert_eq!(again.center, first.center);
        assert_eq!(again.radius, first.radius);
        assert_eq!(again.refs, first.refs);
    }

    // The solid is analytic: three faces, one of them a cylinder of the radius
    // that was asked for, and a B-Rep volume of pi r^2 h rather than a
    // polygon's approximation of it.
    let real = analytic(&out);
    assert_eq!(real.faces, 3, "one cylindrical side and two planar caps");
    assert_eq!(
        real.side,
        vec![ferritecad_kernel::FaceSurface::Cylinder { radius: 10. }],
        "the face raised from the circle is a cylinder, not a fan of planes"
    );
    assert_eq!(real.caps.len(), 2);
    assert!(
        real.caps
            .iter()
            .all(|s| *s == ferritecad_kernel::FaceSurface::Plane)
    );
    let exact = PI * 10.0_f64.powi(2) * 15.;
    assert!(
        (real.volume - exact).abs() < 1e-6 * exact,
        "analytic volume {} is not {exact}",
        real.volume
    );

    // The archive outlives both its writing session and its open SQLite
    // connection. A hit must restore the same analytic faces under every
    // stored reference, without inventing circle corners in the cache map.
    let cache = d.path().join("circle.fcad-cache");
    for outcome in [
        ferritecad_eval::CacheOutcome::Miss,
        ferritecad_eval::CacheOutcome::Hit,
    ] {
        let warm = analytic_with_cache(&out, Some((&cache, outcome)));
        assert_eq!(warm.faces, real.faces);
        assert_eq!(warm.side, real.side);
        assert_eq!(warm.caps, real.caps);
        assert!((warm.volume - real.volume).abs() < exact * 1e-6);
        assert_eq!(stored(&out).refs, first.refs);
    }
    std::fs::remove_file(cache).expect("private cache closed and removed");

    // The exported mesh is checked at explicit chord/angular settings,
    // separately from the B-Rep. Cap triangles do not count perimeter facets.
    let stl = d.path().join("result.stl");
    let m = mesh(&out, &stl);
    check_circle_mesh(&m, [12., -7.], 10., 15.);

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
    keep(&fbx, "circle.fbx");
    assert_eq!(std::fs::read(&out).expect("source"), saved);

    // A second circle of another centre and a fractional radius: a hardcoded
    // cylinder would pass the first measurement and fail this one.
    let other_input = d.path().join("other.json");
    let other = d.path().join("other.fcad");
    write(
        &other_input,
        &json!({"schema_version":1,"center_mm":[-3.5,4.25],"radius_mm":6.75,"height_mm":2.5}),
    );
    reply(create(&other_input, &other).output().expect("process"), 0);
    let kept = stored(&other);
    assert_eq!(kept.center, [-3.5, 4.25]);
    assert_eq!(kept.radius, 6.75);
    assert_ne!(kept.curve, first.curve, "two documents, two identities");
    let real = analytic(&other);
    assert_eq!(real.faces, 3);
    assert_eq!(
        real.side,
        vec![ferritecad_kernel::FaceSurface::Cylinder { radius: 6.75 }]
    );
    let exact = PI * 6.75_f64.powi(2) * 2.5;
    assert!(
        (real.volume - exact).abs() < 1e-6 * exact,
        "analytic volume {} is not {exact}",
        real.volume
    );
    let m = mesh(&other, &d.path().join("other.stl"));
    check_circle_mesh(&m, [-3.5, 4.25], 6.75, 2.5);

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
        assert_eq!(kept.radius, 10.);
    }
    let mut expected: Vec<std::ffi::OsString> = [
        "цилиндр space.fcad",
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

#[test]
fn native_circle_height_copy_keeps_the_circle_and_every_identity() {
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

    // The circle, its identity and every other stored fact are the copy's too.
    let after = stored(&taller);
    assert_eq!(after.curve, before.curve, "the circle keeps its UUID");
    assert_eq!(after.center, before.center);
    assert_eq!(after.radius, before.radius);
    assert_eq!(after.refs, before.refs, "same reference UUIDs and roles");
    assert_eq!(after.objects, before.objects);
    assert_eq!(
        std::fs::read(&source).expect("source"),
        bytes,
        "the source is untouched"
    );

    // Only the height changed, and it changed the analytic volume with it.
    let real = analytic(&taller);
    assert_eq!(real.faces, 3);
    assert_eq!(
        real.side,
        vec![ferritecad_kernel::FaceSurface::Cylinder { radius: 10. }]
    );
    let exact = PI * 10.0_f64.powi(2) * 25.;
    assert!(
        (real.volume - exact).abs() < 1e-6 * exact,
        "analytic volume {} is not {exact}",
        real.volume
    );
    let m = mesh(&taller, &d.path().join("taller.stl"));
    check_circle_mesh(&m, [12., -7.], 10., 25.);
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
    keep(&fbx, "circle-taller.fbx");

    // Everything the document stores apart from the one changed expression.
    let sql_after = tables(&taller);
    assert_eq!(
        sql_before.keys().collect::<Vec<_>>(),
        sql_after.keys().collect::<Vec<_>>()
    );
    for (table, rows) in &sql_before {
        match table.as_str() {
            // Only the selected Extrude's payload/hash may change. Check every
            // cell, including IDs and the unedited plane, Sketch and Body.
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
            // A copy is written at a later instant than its source, so the
            // modified timestamp is the one cell that must differ; everything
            // else about the document, its id included, is the same.
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

/// Every table of a document, as rows, for comparing two publications.
fn tables(path: &Path) -> std::collections::BTreeMap<String, Vec<Vec<rusqlite::types::Value>>> {
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
