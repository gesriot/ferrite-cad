// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::{Document, ExtrudeEditSource, ObjectPayload, SketchGeometry};
use ferritecad_kernel::OperationContext;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;
const OP: &str = "edit-sketch-constraints-copy";
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn reply(o: Output, op: &str, code: i32) -> Value {
    assert_eq!(o.status.code(), Some(code), "{o:?}");
    assert_eq!(o.stdout.iter().filter(|&&b| b == b'\n').count(), 1, "{o:?}");
    let v: Value = serde_json::from_slice(&o.stdout).expect("one JSON object");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["operation"], op);
    assert_eq!(v["ok"], code == 0);
    assert_eq!(v.get("result").is_some(), code == 0);
    assert_eq!(v.get("error").is_some(), code != 0);
    v
}
fn inspect(p: &Path) -> Value {
    reply(
        cli()
            .arg("inspect")
            .arg(p)
            .arg("--json")
            .output()
            .expect("inspect"),
        "inspect",
        0,
    )["result"]
        .clone()
}
fn write(p: &Path, v: &Value) {
    std::fs::write(p, serde_json::to_vec(v).expect("encode")).expect("write");
}
fn entries(p: &Path) -> Vec<std::ffi::OsString> {
    let mut v: Vec<_> = std::fs::read_dir(p)
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    v.sort();
    v
}
fn native() -> bool {
    if ferritecad_occt::is_available() && cfg!(feature = "planegcs") {
        true
    } else {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: native constraint geometry requires OCCT and PlaneGCS");
        false
    }
}
struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    request: PathBuf,
    catalog: Value,
}
impl Fixture {
    fn new(native: bool) -> Self {
        Self::with_points(native.then_some([[-20., -10.], [40., -8.], [42., 30.], [-20., 30.]]))
    }
    fn with_points(points: Option<[[f64; 2]; 4]>) -> Self {
        let root = tempfile::tempdir().expect("dir");
        let source = root.path().join("контур space.fcad");
        let request = root.path().join("request.json");
        if let Some(points) = points {
            write(
                &request,
                &json!({"request_version":1,"points_mm":points,"height_mm":10}),
            );
            reply(
                cli()
                    .arg("create-sketch-extrude")
                    .arg(&request)
                    .arg("-o")
                    .arg(&source)
                    .arg("--json")
                    .output()
                    .expect("create slanted polygon"),
                "create-sketch-extrude",
                0,
            );
        } else {
            reply(
                cli()
                    .arg("create")
                    .arg(&source)
                    .args(["--sample", "--json"])
                    .output()
                    .expect("create"),
                "create",
                0,
            );
        }
        let mut d = Document::open(&source).expect("names");
        let mut o = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("sketch");
        o.name = Some("имя \"H/V\"\nстрока".into());
        d.write(|w| w.put_object(o.id, o.parent, o.ordinal, o.name.as_deref(), &o.payload))
            .expect("stored name");
        d.close().expect("close");
        let catalog = inspect(&source);
        let f = Self {
            root,
            source,
            request,
            catalog,
        };
        f.add(0, "horizontal");
        f
    }
    fn add(&self, i: usize, rule: &str) {
        write(
            &self.request,
            &json!({"request_version":1,"remove":[],"add":[{"curve_id":self.catalog["sketches"][0]["constraint_edit"]["curves"][i]["curve_id"],"rule":rule}]}),
        );
    }
    fn edit(&self, out: &Path) -> Command {
        self.edit_from(&self.source, &self.catalog, out)
    }
    fn edit_from(&self, source: &Path, catalog: &Value, out: &Path) -> Command {
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
            .arg(out)
            .arg("--json");
        c
    }
}
#[test]
fn constraint_discovery_requests_and_delivery_without_native() {
    let f = Fixture::new(false);
    let out = f.root.path().join("out.fcad");
    let before = std::fs::read(&f.source).expect("source");
    let names = entries(f.root.path());
    let modified = std::fs::metadata(&f.source)
        .expect("meta")
        .modified()
        .expect("mtime");
    let d = Document::open_read_only(&f.source).expect("snapshot");
    let facts = ExtrudeEditSource::read(&d).expect("facts");
    let row = &f.catalog["sketches"][0];
    let discovery = &row["constraint_edit"];
    assert_eq!(row["name"], "имя \"H/V\"\nстрока");
    assert_eq!(discovery["available"], true);
    assert!(discovery["refusal"].is_null());
    assert!(discovery["document_refusal"].is_null());
    assert_eq!(discovery["constraints"], json!([]));
    assert_eq!(
        f.catalog["content_version"],
        facts.version.content.to_string()
    );
    for (curve, actual) in discovery["curves"].as_array().expect("curves").iter().zip(
        &facts.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("supported")
            .curves,
    ) {
        assert_eq!(curve["curve_id"], actual.id.to_string());
        let SketchGeometry::Line { start, end } = actual.geometry else {
            panic!("line")
        };
        assert_eq!(curve["start_mm"], json!([start.x, start.y]));
        assert_eq!(curve["end_mm"], json!([end.x, end.y]));
    }
    d.close().expect("close");
    for bad in [
        json!({"request_version":2,"remove":[],"add":[]}),
        json!({"request_version":1,"remove":[],"add":[],"future":true}),
        json!({"request_version":1,"remove":[],"add":[{"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed"}]}),
        json!({"request_version":1,"add":[]}),
    ] {
        write(&f.request, &bad);
        let v = reply(
            f.edit(&out)
                .stderr(pipe::closed_pipe())
                .output()
                .expect("refusal"),
            OP,
            2,
        );
        assert_eq!(
            v["error"]["kind"],
            if bad["request_version"] == 2 {
                "unsupported"
            } else {
                "input"
            }
        );
        assert!(v["error"].get("constraint_conflict").is_none());
    }
    for addition in [
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"horizontal","distance_mm":10}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"vertical","distance_mm":null}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"distance"}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"distance","distance_mm":null}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"distance","distance_mm":"30"}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"distance","distance_mm":true}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"distance","distance_mm":0}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"distance","distance_mm":-1}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"distance","distance_mm":30,"future":1}),
    ] {
        write(
            &f.request,
            &json!({"request_version":1,"remove":[],"add":[addition]}),
        );
        let v = reply(f.edit(&out).output().expect("invalid dimension"), OP, 2);
        assert_eq!(v["error"]["kind"], "input");
        assert!(v["error"].get("constraint_conflict").is_none());
    }
    for addition in [
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"start","x_mm":10}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"start","y_mm":-5}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","x_mm":10,"y_mm":-5}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"middle","x_mm":10,"y_mm":-5}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"at","x_mm":10,"y_mm":-5}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"Start","x_mm":10,"y_mm":-5}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":0,"x_mm":10,"y_mm":-5}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"start","x_mm":"10","y_mm":-5}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"start","x_mm":null,"y_mm":-5}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"start","x_mm":true,"y_mm":-5}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"start","x_mm":10,"y_mm":-5,"distance_mm":30}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"start","x_mm":10,"y_mm":-5,"future":1}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"horizontal","at":"start"}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"distance","distance_mm":30,"at":"start"}),
    ] {
        write(
            &f.request,
            &json!({"request_version":1,"remove":[],"add":[addition]}),
        );
        let v = reply(f.edit(&out).output().expect("invalid pin"), OP, 2);
        assert_eq!(v["error"]["kind"], "input");
        assert!(v["error"].get("constraint_conflict").is_none());
    }
    for addition in [
        json!({"rule":"equal_length","a_curve_id":discovery["curves"][0]["curve_id"]}),
        json!({"rule":"equal_length","b_curve_id":discovery["curves"][1]["curve_id"]}),
        json!({"rule":"equal_length","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":discovery["curves"][1]["curve_id"],"curve_id":discovery["curves"][2]["curve_id"]}),
        json!({"rule":"equal_length","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":discovery["curves"][1]["curve_id"],"distance_mm":30}),
        json!({"rule":"equal_length","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":discovery["curves"][1]["curve_id"],"at":"start"}),
        json!({"rule":"equal_length","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":null}),
        json!({"rule":"equal_length","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":7}),
        json!({"rule":"equal_length","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":"not-a-uuid"}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"equal_length"}),
        json!({"rule":"equallength","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":discovery["curves"][1]["curve_id"]}),
        json!({"rule":"parallel","a_curve_id":discovery["curves"][0]["curve_id"]}),
        json!({"rule":"parallel","b_curve_id":discovery["curves"][1]["curve_id"]}),
        json!({"rule":"perpendicular","a_curve_id":discovery["curves"][0]["curve_id"]}),
        json!({"rule":"perpendicular","b_curve_id":discovery["curves"][1]["curve_id"]}),
        json!({"rule":"parallel","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":discovery["curves"][1]["curve_id"],"curve_id":discovery["curves"][2]["curve_id"]}),
        json!({"rule":"parallel","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":discovery["curves"][1]["curve_id"],"distance_mm":30}),
        json!({"rule":"perpendicular","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":discovery["curves"][1]["curve_id"],"at":"start"}),
        json!({"rule":"perpendicular","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":discovery["curves"][1]["curve_id"],"future":1}),
        json!({"rule":"parallel","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":null}),
        json!({"rule":"parallel","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":7}),
        json!({"rule":"perpendicular","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":true}),
        json!({"rule":"perpendicular","a_curve_id":"not-a-uuid","b_curve_id":discovery["curves"][1]["curve_id"]}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"parallel"}),
        json!({"rule":"Parallel","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":discovery["curves"][1]["curve_id"]}),
        json!({"rule":"perpendicularity","a_curve_id":discovery["curves"][0]["curve_id"],"b_curve_id":discovery["curves"][1]["curve_id"]}),
        // The pair fields belong to the pair rules and to nothing else.
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"horizontal","a_curve_id":discovery["curves"][1]["curve_id"]}),
        json!({"curve_id":discovery["curves"][0]["curve_id"],"rule":"distance","distance_mm":30,"b_curve_id":discovery["curves"][1]["curve_id"]}),
    ] {
        write(
            &f.request,
            &json!({"request_version":1,"remove":[],"add":[addition]}),
        );
        // Wire-level refusals only: a self-pair is structural and is measured
        // against a real document in the native gate below.
        let v = reply(f.edit(&out).output().expect("invalid pair"), OP, 2);
        assert_eq!(v["error"]["kind"], "input");
        assert!(v["error"].get("constraint_conflict").is_none());
    }
    let pin=json!({"request_version":1,"remove":[],"add":[{"curve_id":discovery["curves"][0]["curve_id"],"rule":"fixed","at":"start","x_mm":10,"y_mm":-5}]}).to_string();
    for (field, bad) in [
        ("x_mm", "NaN"),
        ("x_mm", "Infinity"),
        ("x_mm", "-Infinity"),
        ("y_mm", "1e999"),
        ("y_mm", "-1e999"),
    ] {
        std::fs::write(
            &f.request,
            pin.replace(
                &format!("\"{field}\":{}", if field == "x_mm" { "10" } else { "-5" }),
                &format!("\"{field}\":{bad}"),
            ),
        )
        .expect("bad coordinate");
        assert_eq!(
            reply(f.edit(&out).output().expect("bad pin number"), OP, 2)["error"]["kind"],
            "input"
        );
    }
    let valid=json!({"request_version":1,"remove":[],"add":[{"curve_id":discovery["curves"][0]["curve_id"],"rule":"distance","distance_mm":30}]}).to_string();
    for bad in ["1e999", "NaN", "Infinity"] {
        std::fs::write(
            &f.request,
            valid.replace("\"distance_mm\":30", &format!("\"distance_mm\":{bad}")),
        )
        .expect("bad number");
        assert_eq!(
            reply(f.edit(&out).output().expect("bad JSON number"), OP, 2)["error"]["kind"],
            "input"
        );
    }
    for both in [false, true] {
        let mut c = f.edit(&out);
        c.stdout(pipe::closed_pipe());
        if both {
            c.stderr(pipe::closed_pipe());
        }
        assert_eq!(c.output().expect("refusal pipe").status.code(), Some(7));
    }
    write(
        &f.request,
        &serde_json::from_str::<Value>(&valid).expect("valid length"),
    );
    if !ferritecad_occt::is_available() {
        let v = reply(f.edit(&out).output().expect("stub"), OP, 2);
        assert_eq!(v["error"]["kind"], "unsupported");
    }
    for args in [
        vec![OP, "--help"],
        vec![OP, "--json"],
        vec![OP, "--sketch", "not-a-uuid", "--json"],
    ] {
        let o = cli().args(&args).output().expect("clap");
        assert_eq!(
            o.status.code(),
            Some(if args[1] == "--help" { 0 } else { 2 })
        );
        assert!(serde_json::from_slice::<Value>(&o.stdout).is_err());
    }
    assert!(!out.exists());
    assert_eq!(entries(f.root.path()), names);
    assert_eq!(std::fs::read(&f.source).expect("source"), before);
    assert_eq!(
        std::fs::metadata(&f.source)
            .expect("meta")
            .modified()
            .expect("mtime"),
        modified
    );
}
fn stored_same(a: &Path, b: &Path) {
    let a = Document::open_read_only(a).expect("source");
    let b = Document::open_read_only(b).expect("copy");
    assert_eq!(a.meta(), b.meta());
    assert_eq!(
        a.dependencies().expect("deps"),
        b.dependencies().expect("deps")
    );
    assert_eq!(
        a.topology_refs().expect("refs"),
        b.topology_refs().expect("refs")
    );
    for o in a.objects().expect("objects") {
        let n = b.object(o.id).expect("read").expect("same ID");
        if let (ObjectPayload::Sketch(s), ObjectPayload::Sketch(t)) = (&o.payload, &n.payload) {
            assert_eq!(s.plane, t.plane);
            assert_eq!(s.curves, t.curves);
            assert_eq!(o.name, n.name);
            assert_eq!(o.parent, n.parent);
            assert_eq!(o.ordinal, n.ordinal);
        } else {
            assert_eq!(o, n);
        }
    }
    assert!(b.validate().expect("validate").is_ok());
    a.close().expect("close");
    b.close().expect("close");
}
/// Independent STL triangle integration, compared with the measured solved polygon;
/// no arbitrary under-constrained position is pinned across solver platforms.
/// `dof` names the exact count where this slice fixes it; `None` keeps the
/// older gates' weaker "still under-constrained" statement.
fn geometry_checked(
    path: &Path,
    dof: Option<usize>,
    check: impl FnOnce(&[[f64; 2]], &[[f64; 2]], &ferritecad_document::Sketch),
) {
    let d = Document::open_read_only(path).expect("doc");
    let objects = d.objects().expect("objects");
    let o = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("sketch");
    let ObjectPayload::Sketch(s) = &o.payload else {
        panic!("sketch")
    };
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    let b = ferritecad_eval::rebuild_cold(&d, &mut k, &OperationContext::default())
        .expect("cold after reopen");
    let report = b.solve_report(o.id).expect("solve report");
    match dof {
        Some(n) => assert_eq!(report.degrees_of_freedom(), n, "cold reopen solved DOF"),
        None => assert!(report.degrees_of_freedom() > 0),
    }
    assert!(report.redundant().is_empty());
    let p = b.sketch_presentation(o.id).expect("solved presentation");
    let mut starts = vec![];
    let mut ends = vec![];
    for (curve, stored) in p.curves().iter().zip(&s.curves) {
        assert_eq!(curve.id(), stored.id);
        let SketchGeometry::Line { start, end } = curve.geometry() else {
            panic!("line")
        };
        starts.push([start.x, start.y]);
        ends.push([end.x, end.y]);
    }
    for (i, end) in ends.iter().enumerate() {
        let next = starts[(i + 1) % starts.len()];
        assert!(
            (end[0] - next[0]).abs() < 1e-6 && (end[1] - next[1]).abs() < 1e-6,
            "joint {i}: {end:?} != {next:?}"
        );
    }
    check(&starts, &ends, s);
    let area: f64 = starts
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let z = starts[(i + 1) % starts.len()];
            a[0] * z[1] - a[1] * z[0]
        })
        .sum::<f64>()
        .abs()
        / 2.;
    let lo = [
        starts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
        starts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
        0.,
    ];
    let hi = [
        starts
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max),
        starts
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max),
        10.,
    ];
    for r in d.topology_refs().expect("refs") {
        assert_eq!(b.resolve(&r).expect("same topology ref").len(), 1);
    }
    b.release_all(&mut k);
    d.close().expect("close");
    let stl = path.with_extension("stl");
    let v = reply(
        cli()
            .arg("export-stl")
            .arg(path)
            .arg("-o")
            .arg(&stl)
            .arg("--json")
            .output()
            .expect("STL"),
        "export-stl",
        0,
    );
    let bytes = std::fs::read(&stl).expect("STL bytes");
    let n = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + 50 * n);
    assert_eq!(v["result"]["triangles"], n);
    assert_eq!(v["result"]["bytes"], bytes.len());
    let mut actual_lo = [f64::INFINITY; 3];
    let mut actual_hi = [f64::NEG_INFINITY; 3];
    let mut volume6 = 0.;
    for t in bytes[84..].chunks_exact(50) {
        let mut p = [[0.; 3]; 3];
        for (i, v) in p.iter_mut().enumerate() {
            for j in 0..3 {
                let at = 12 + 12 * i + 4 * j;
                v[j] = f64::from(f32::from_le_bytes(
                    t[at..at + 4].try_into().expect("coordinate"),
                ));
                actual_lo[j] = actual_lo[j].min(v[j]);
                actual_hi[j] = actual_hi[j].max(v[j]);
            }
        }
        let [a, b, c] = p;
        volume6 += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    for i in 0..3 {
        assert!((lo[i] - actual_lo[i]).abs() < 1e-4);
        assert!((hi[i] - actual_hi[i]).abs() < 1e-4);
    }
    assert!(
        (volume6.abs() / 6. - area * 10.).abs() < area * 10. * 1e-6,
        "independent volume disagrees with solved polygon"
    );
    let fbx = path.with_extension("fbx");
    let r = reply(
        cli()
            .arg("export-fbx")
            .arg(path)
            .arg("-o")
            .arg(&fbx)
            .arg("--json")
            .output()
            .expect("FBX"),
        "export-fbx",
        0,
    );
    assert_eq!(r["result"]["complete"], true);
    if let Some(root) = std::env::var_os("FERRITECAD_CONSTRAINT_ARTIFACTS") {
        std::fs::create_dir_all(&root).expect("artifacts");
        for p in [path, &stl, &fbx] {
            std::fs::copy(p, Path::new(&root).join(p.file_name().expect("name")))
                .expect("artifact");
        }
    }
}
fn geometry(path: &Path, horizontal: bool) {
    geometry_checked(path, None, |starts, ends, s| {
        let (line, axis) = if horizontal { (0, 1) } else { (1, 0) };
        let SketchGeometry::Line {
            start: old_a,
            end: old_b,
        } = s.curves[line].geometry
        else {
            panic!("line")
        };
        assert!(
            (if horizontal {
                old_a.y - old_b.y
            } else {
                old_a.x - old_b.x
            })
            .abs()
                > 1.
        );
        assert!(
            (starts[line][axis] - ends[line][axis]).abs() < 1e-7,
            "H/V was NOT delivered to solver: {starts:?} {ends:?}"
        );
        assert!(
            starts.iter().zip(&s.curves).any(|(p, c)| {
                let SketchGeometry::Line { start, .. } = c.geometry else {
                    return false;
                };
                (p[0] - start.x).abs() > 0.1 || (p[1] - start.y).abs() > 0.1
            }),
            "slanted stored coordinates did not move"
        );
    });
}
fn length_geometry(path: &Path, lengths: &[(usize, f64)], rectangle: bool) {
    geometry_checked(path, None, |starts, ends, s| {
        for &(i, length) in lengths {
            let dx = ends[i][0] - starts[i][0];
            let dy = ends[i][1] - starts[i][1];
            assert!(
                (dx.hypot(dy) - length).abs() < 1e-6,
                "length NOT solved: line {i}, {dx}/{dy}, expected {length}"
            );
            if !rectangle {
                assert!(
                    (dx.abs() - length).abs() > 0.01 && (dy.abs() - length).abs() > 0.01,
                    "slanted length must not be a projection"
                );
            }
        }
        if rectangle {
            let widths: Vec<_> = (0..2)
                .map(|j| {
                    starts
                        .iter()
                        .map(|p| p[j])
                        .fold(f64::NEG_INFINITY, f64::max)
                        - starts.iter().map(|p| p[j]).fold(f64::INFINITY, f64::min)
                })
                .collect();
            assert!(
                (widths[0] - 60.).abs() < 1e-6 && (widths[1] - 30.).abs() < 1e-6,
                "{widths:?}"
            );
            assert_eq!(s.curves.len(), 4);
            let area2 = starts
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let b = starts[(i + 1) % 4];
                    a[0] * b[1] - a[1] * b[0]
                })
                .sum::<f64>()
                .abs();
            assert!(
                (area2 / 2. * 10. - 18000.).abs() < 1e-4,
                "expected 18000 mm3, before independent STL integration"
            );
        }
    });
}
#[test]
fn native_constraint_copy_process_solves_slanted_h_v_and_removes_exact_ids() {
    if !native() {
        return;
    }
    let f = Fixture::new(true);
    let before = std::fs::read(&f.source).expect("source");
    let mtime = std::fs::metadata(&f.source)
        .expect("meta")
        .modified()
        .expect("mtime");
    for h in [true, false] {
        f.add(
            if h { 0 } else { 1 },
            if h { "horizontal" } else { "vertical" },
        );
        let out = f.root.path().join(if h {
            "horizontal.fcad"
        } else {
            "vertical.fcad"
        });
        let result = reply(f.edit(&out).output().expect("edit"), OP, 0)["result"].clone();
        stored_same(&f.source, &out);
        let after = inspect(&out);
        let choice = &after["sketches"][0]["constraint_edit"];
        assert_eq!(result["destination"], out.to_str().expect("path"));
        assert_eq!(result["document_id"], f.catalog["document_id"]);
        assert_eq!(result["sketch_id"], f.catalog["sketches"][0]["sketch_id"]);
        assert_eq!(result["added_constraints"], choice["constraints"]);
        assert_eq!(
            result["added_constraints"]
                .as_array()
                .expect("constraints")
                .len(),
            5
        );
        assert_eq!(result["removed_constraint_ids"], json!([]));
        assert!(result["solve"]["degrees_of_freedom"].as_u64().expect("DOF") > 0);
        assert_eq!(
            choice["curves"],
            f.catalog["sketches"][0]["constraint_edit"]["curves"]
        );
        assert_eq!(choice["available"], true);
        assert_eq!(after["sketches"][0]["editable"], false);
        assert_eq!(after["features"], f.catalog["features"]);
        assert_eq!(after["bodies"], f.catalog["bodies"]);
        geometry(&out, h);
        let constraint = &choice["constraints"][4]["constraint_id"];
        write(
            &f.request,
            &json!({"request_version":1,"remove":[constraint],"add":[]}),
        );
        let removed = f.root.path().join(format!("removed-{h}.fcad"));
        let r = reply(
            f.edit_from(&out, &after, &removed)
                .output()
                .expect("remove"),
            OP,
            0,
        )["result"]
            .clone();
        assert_eq!(r["added_constraints"], json!([]));
        assert_eq!(r["removed_constraint_ids"], json!([constraint]));
        stored_same(&out, &removed);
        let now = inspect(&removed);
        assert_eq!(
            now["sketches"][0]["constraint_edit"]["constraints"],
            json!(&choice["constraints"].as_array().expect("rules")[..4])
        );
        assert!(
            cli()
                .arg("rebuild")
                .arg(&removed)
                .arg("--cold")
                .output()
                .expect("cold removal")
                .status
                .success()
        );
    }
    f.add(0, "horizontal");
    for both in [false, true] {
        let out = f.root.path().join(format!("pipe-{both}.fcad"));
        let mut c = f.edit(&out);
        c.stdout(pipe::closed_pipe());
        if both {
            c.stderr(pipe::closed_pipe());
        }
        let o = c.output().expect("pipe");
        assert_eq!(o.status.code(), Some(7), "{o:?}");
        stored_same(&f.source, &out);
        assert_eq!(
            inspect(&out)["sketches"][0]["constraint_edit"]["constraints"]
                .as_array()
                .expect("persisted")
                .len(),
            5
        );
    }
    assert_eq!(std::fs::read(&f.source).expect("unchanged"), before);
    assert_eq!(
        std::fs::metadata(&f.source)
            .expect("meta")
            .modified()
            .expect("mtime"),
        mtime
    );
    assert!(
        !entries(f.root.path())
            .iter()
            .any(|n| n.to_string_lossy().starts_with(".ferritecad-"))
    );
}
#[test]
fn native_constraint_refusals_are_atomic() {
    if !native() {
        return;
    }
    for length in [false, true] {
        let f = Fixture::new(true);
        let out = f.root.path().join("out.fcad");
        let before = std::fs::read(&f.source).expect("source");
        let curve = f.catalog["sketches"][0]["constraint_edit"]["curves"][0]["curve_id"].clone();
        for bad in [
            json!({"request_version":1,"remove":[],"add":[]}),
            json!({"request_version":1,"remove":[],"add":[{"curve_id":curve,"rule":"horizontal"},{"curve_id":curve,"rule":"horizontal"}]}),
            json!({"request_version":1,"remove":[],"add":[{"curve_id":curve,"rule":"horizontal"},{"curve_id":curve,"rule":"vertical"}]}),
            json!({"request_version":1,"remove":[ferritecad_types::StableEntityId::new()],"add":[]}),
        ] {
            write(&f.request, &bad);
            let names = entries(f.root.path());
            assert_eq!(
                reply(f.edit(&out).output().expect("refusal"), OP, 2)["error"]["kind"],
                "input"
            );
            assert_eq!(entries(f.root.path()), names);
        }
        // All four lines horizontal collapse the area: a solved system is not a valid profile.
        write(
            &f.request,
            &json!({"request_version":1,"remove":[],"add":f.catalog["sketches"][0]["constraint_edit"]["curves"].as_array().expect("curves").iter().map(|c|json!({"curve_id":c["curve_id"],"rule":"horizontal"})).collect::<Vec<_>>()}),
        );
        reply(
            f.edit(&out).output().expect("invalid solved profile"),
            OP,
            2,
        );
        assert!(!out.exists());
        if length {
            write(
                &f.request,
                &json!({"request_version":1,"remove":[],"add":[length_add(&f.catalog,0,50.)]}),
            );
        } else {
            f.add(0, "horizontal");
        }
        std::fs::write(&out, b"occupied").expect("busy");
        let names = entries(f.root.path());
        reply(f.edit(&out).output().expect("no clobber"), OP, 2);
        assert_eq!(std::fs::read(&out).expect("busy"), b"occupied");
        assert_eq!(entries(f.root.path()), names);
        reply(f.edit(&f.source).output().expect("source"), OP, 2);
        let alias = f.root.path().join("alias.fcad");
        std::fs::hard_link(&f.source, &alias).expect("hardlink");
        reply(f.edit(&alias).output().expect("alias"), OP, 2);
        assert_eq!(std::fs::read(&alias).expect("alias"), before);
        #[cfg(unix)]
        {
            let link = f.root.path().join("symlink.fcad");
            std::os::unix::fs::symlink(&f.source, &link).expect("symlink");
            reply(f.edit(&link).output().expect("symlink refusal"), OP, 2);
        }
        assert_eq!(std::fs::read(&f.source).expect("source"), before);
        let mut d = Document::open(&f.source).expect("change");
        let mut o = d.objects().expect("objects").remove(0);
        o.name = Some("stale".into());
        d.write(|w| w.put_object(o.id, o.parent, o.ordinal, o.name.as_deref(), &o.payload))
            .expect("write");
        d.close().expect("close");
        let stale = f.root.path().join("stale.fcad");
        let changed = std::fs::read(&f.source).expect("changed");
        let r = reply(f.edit(&stale).output().expect("stale"), OP, 2);
        assert_eq!(r["error"]["kind"], "input");
        assert!(
            r["error"]["message"]
                .as_str()
                .expect("message")
                .contains("changed")
        );
        assert!(!stale.exists());
        assert_eq!(std::fs::read(&f.source).expect("preserved"), changed);
    }
}

#[test]
fn constraint_os_paths_and_flag_shaped_arguments() {
    let f = Fixture::new(false);
    let out = f.root.path().join("out.fcad");
    // Platform-invalid encoding is created with each OS's lossless argument API.
    #[cfg(unix)]
    let invalid = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(b"bad-\xff".to_vec())
    };
    #[cfg(windows)]
    let invalid = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xD800])
    };
    #[cfg(any(unix, windows))]
    {
        let path = f.root.path().join(invalid);
        let bad_catalog = &f.catalog;
        let v = reply(
            f.edit_from(&path, bad_catalog, &out)
                .output()
                .expect("bad source"),
            OP,
            2,
        );
        assert_eq!(v["error"]["kind"], "input");
        assert!(!path.exists());
        reply(f.edit(&path).output().expect("bad destination"), OP, 2);
        let mut c = cli();
        c.arg(OP)
            .arg(&f.source)
            .arg("--sketch")
            .arg(
                bad_catalog["sketches"][0]["sketch_id"]
                    .as_str()
                    .expect("id"),
            )
            .arg("--expect-version")
            .arg(bad_catalog["content_version"].as_str().expect("version"))
            .arg("--request")
            .arg(&path)
            .arg("-o")
            .arg(&out)
            .arg("--json");
        reply(c.output().expect("bad request path"), OP, 2);
    }
    let flag = f.root.path().join("--json");
    std::fs::copy(&f.source, &flag).expect("flag named source");
    let c = &f.catalog;
    let o = cli()
        .current_dir(f.root.path())
        .arg(OP)
        .arg("--sketch")
        .arg(c["sketches"][0]["sketch_id"].as_str().expect("id"))
        .arg("--expect-version")
        .arg(c["content_version"].as_str().expect("version"))
        .arg("--request")
        .arg(&f.request)
        .arg("-o")
        .arg(&out)
        .args(["--", "--json"])
        .output()
        .expect("flag source text");
    assert!(serde_json::from_slice::<Value>(&o.stdout).is_err());
}

#[test]
fn constraint_discovery_separates_document_and_feature_refusals() {
    for global in [false, true] {
        let f = Fixture::new(false);
        if global {
            rusqlite::Connection::open(&f.source)
                .expect("SQL")
                .execute_batch(
                    "CREATE TRIGGER preserve_external AFTER UPDATE ON objects BEGIN SELECT 1; END;",
                )
                .expect("trigger");
        } else {
            let mut d = Document::open(&f.source).expect("doc");
            let mut o = d
                .objects()
                .expect("objects")
                .into_iter()
                .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
                .expect("sketch");
            let ObjectPayload::Sketch(s) = &mut o.payload else {
                panic!("sketch")
            };
            // Arbitrary point-to-point Distance between two different Lines is
            // still outside the managed class, and says so. Perpendicular over
            // two whole Lines used to stand here and is now supported (§25I),
            // which is exactly why it is no longer the example.
            let end_of = |i: usize| {
                ferritecad_document::SketchPointRef::new(
                    s.curves[i].id,
                    ferritecad_document::SketchPointSelector::End,
                )
            };
            s.constraints.push(ferritecad_document::SketchConstraint {
                id: ferritecad_types::StableEntityId::new(),
                rule: ferritecad_document::SketchConstraintRule::Distance {
                    a: end_of(0),
                    b: end_of(2),
                    distance: 30.,
                },
            });
            d.write(|w| w.put_object(o.id, o.parent, o.ordinal, o.name.as_deref(), &o.payload))
                .expect("unsupported family");
            d.close().expect("close");
        }
        let before = std::fs::read(&f.source).expect("source");
        let names = entries(f.root.path());
        let c = inspect(&f.source);
        let edit = &c["sketches"][0]["constraint_edit"];
        assert_eq!(edit["available"], false);
        if global {
            assert!(edit["refusal"].is_null());
            assert!(
                edit["document_refusal"]
                    .as_str()
                    .expect("global refusal")
                    .contains("trigger")
            );
            assert!(edit["curves"].is_array());
            assert_eq!(edit["constraints"], json!([]));
        } else {
            assert!(edit["document_refusal"].is_null());
            assert!(
                edit["refusal"]
                    .as_str()
                    .expect("family refusal")
                    .contains("families")
            );
            assert!(edit["curves"].is_null());
            assert!(edit["constraints"].is_null());
        }
        assert_eq!(std::fs::read(&f.source).expect("unchanged"), before);
        assert_eq!(entries(f.root.path()), names);
    }
}

fn length_add(catalog: &Value, i: usize, mm: f64) -> Value {
    json!({"curve_id":catalog["sketches"][0]["constraint_edit"]["curves"][i]["curve_id"],"rule":"distance","distance_mm":mm})
}
fn rectangle_edits(catalog: &Value) -> Value {
    let mut add: Vec<_> = (0..4).map(|i| json!({"curve_id":catalog["sketches"][0]["constraint_edit"]["curves"][i]["curve_id"],"rule":if i%2==0 {"horizontal"} else {"vertical"}})).collect();
    add.extend([length_add(catalog, 0, 60.), length_add(catalog, 1, 30.)]);
    json!({"request_version":1,"remove":[],"add":add})
}
#[test]
fn native_line_length_process_geometry_replacement_conflict_and_delivery() {
    if !native() {
        return;
    }
    let f = Fixture::with_points(Some([[-40., -20.], [40., -20.], [40., 20.], [-40., 20.]]));
    let curves = &f.catalog["sketches"][0]["constraint_edit"]["curves"];
    assert_eq!(curves[0]["start_mm"], json!([-40., -20.]));
    assert_eq!(curves[0]["end_mm"], json!([40., -20.]));
    assert_eq!(curves[1]["end_mm"], json!([40., 20.]));
    let before = std::fs::read(&f.source).expect("source");
    let mtime = std::fs::metadata(&f.source)
        .expect("meta")
        .modified()
        .expect("mtime");
    let out = f.root.path().join("length-rectangle.fcad");
    let edits = rectangle_edits(&f.catalog);
    for additions in [
        json!([
            length_add(&f.catalog, 0, 60.),
            length_add(&f.catalog, 0, 60.)
        ]),
        json!([
            length_add(&f.catalog, 0, 60.),
            length_add(&f.catalog, 0, 30.)
        ]),
        json!([{"curve_id":ferritecad_types::StableEntityId::new(),"rule":"distance","distance_mm":60}]),
    ] {
        write(
            &f.request,
            &json!({"request_version":1,"remove":[],"add":additions}),
        );
        let directory = entries(f.root.path());
        let refusal = reply(
            f.edit(&out).output().expect("duplicate or foreign length"),
            OP,
            2,
        );
        assert_eq!(refusal["error"]["kind"], "input");
        assert_eq!(entries(f.root.path()), directory);
        assert_eq!(std::fs::read(&f.source).expect("source"), before);
    }
    write(&f.request, &edits);
    let r = reply(f.edit(&out).output().expect("length publish"), OP, 0)["result"].clone();
    assert_eq!(
        r["added_constraints"]
            .as_array()
            .expect("constraints")
            .len(),
        10
    );
    let after = inspect(&out);
    assert_eq!(after["sketches"][0]["constraint_edit"]["available"], true);
    assert_eq!(
        after["sketches"][0]["constraint_edit"]["constraints"],
        r["added_constraints"]
    );
    for (i, mm) in [(8, 60.), (9, 30.)] {
        let c = &r["added_constraints"][i];
        assert_eq!(c["rule"]["kind"], "distance");
        assert_eq!(c["rule"]["distance"], mm);
        assert!(
            c["rule"].get("distance_mm").is_none(),
            "response wire unchanged"
        );
        assert_eq!(c["rule"]["a"]["at"], "start");
        assert_eq!(c["rule"]["b"]["at"], "end");
        assert_eq!(c["rule"]["a"]["curve_id"], c["rule"]["b"]["curve_id"]);
    }
    stored_same(&f.source, &out);
    length_geometry(&out, &[(0, 60.), (1, 30.)], true);
    let text_out = f.root.path().join("length-text.fcad");
    let command = f.edit(&text_out);
    let mut args: Vec<_> = command
        .get_args()
        .map(std::ffi::OsStr::to_os_string)
        .collect();
    assert_eq!(args.pop().expect("known final protocol flag"), "--json");
    let text = cli().args(args).output().expect("text length publish");
    assert!(text.status.success(), "{text:?}");
    let text_catalog = inspect(&text_out);
    let text_constraints = text_catalog["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("text constraints");
    let lines: Vec<_> = std::str::from_utf8(&text.stdout)
        .expect("text stdout")
        .lines()
        .collect();
    assert_eq!(lines.len(), 12);
    assert_eq!(
        lines[0],
        format!(
            "saved {} ({}, sketch {})",
            text_out.display(),
            text_catalog["document_id"].as_str().expect("document"),
            text_catalog["sketches"][0]["sketch_id"]
                .as_str()
                .expect("Sketch")
        )
    );
    for (i, c) in text_constraints.iter().enumerate() {
        assert_eq!(
            lines[i + 1],
            format!(
                "added constraint {}",
                c["constraint_id"].as_str().expect("constraint")
            )
        );
        assert_eq!(c["rule"], r["added_constraints"][i]["rule"]);
    }
    assert_eq!(
        lines[11],
        format!("degrees of freedom: {}", r["solve"]["degrees_of_freedom"])
    );
    stored_same(&f.source, &text_out);
    let replacement = f.root.path().join("length-replaced.fcad");
    let old = r["added_constraints"][8]["constraint_id"].clone();
    write(
        &f.request,
        &json!({"request_version":1,"remove":[old],"add":[length_add(&after,0,55.)]}),
    );
    let replace = reply(
        f.edit_from(&out, &after, &replacement)
            .output()
            .expect("replace"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(replace["removed_constraint_ids"], json!([old]));
    assert_eq!(
        replace["added_constraints"]
            .as_array()
            .expect("new length")
            .len(),
        1
    );
    assert_ne!(replace["added_constraints"][0]["constraint_id"], old);
    let replaced = inspect(&replacement);
    let mut expected = r["added_constraints"].as_array().expect("old").clone();
    expected.remove(8);
    expected.push(replace["added_constraints"][0].clone());
    assert_eq!(
        replaced["sketches"][0]["constraint_edit"]["constraints"],
        json!(expected)
    );
    stored_same(&out, &replacement);
    geometry_checked(&replacement, None, |starts, ends, _| {
        assert!(((ends[0][0] - starts[0][0]).hypot(ends[0][1] - starts[0][1]) - 55.).abs() < 1e-6)
    });
    let removed = f.root.path().join("length-removed.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[replace["added_constraints"][0]["constraint_id"]],"add":[]}),
    );
    reply(
        f.edit_from(&replacement, &replaced, &removed)
            .output()
            .expect("remove exact"),
        OP,
        0,
    );
    expected.pop();
    assert_eq!(
        inspect(&removed)["sketches"][0]["constraint_edit"]["constraints"],
        json!(expected)
    );
    stored_same(&replacement, &removed);
    // Structural validation accepts these dimensions; the native solver must refuse.
    let mut contradictory = edits.clone();
    contradictory["add"]
        .as_array_mut()
        .expect("add")
        .push(length_add(&f.catalog, 2, 70.));
    write(&f.request, &contradictory);
    let failed = f.root.path().join("conflict.fcad");
    let directory = entries(f.root.path());
    let refusal = reply(
        f.edit(&failed).output().expect("real solve conflict"),
        OP,
        2,
    );
    assert_eq!(refusal["error"]["kind"], "constraint", "{refusal}");
    let conflict = refusal["error"]["constraint_conflict"]["constraints"]
        .as_array()
        .expect("typed native conflict");
    assert!(!conflict.is_empty());
    assert!(conflict.iter().any(|c| c["rule"]["kind"] == "distance"));
    assert!(conflict.iter().all(|c| c["constraint_id"].is_string()));
    assert_eq!(entries(f.root.path()), directory);
    assert!(!failed.exists());
    // Valid length reaches publication even if its report cannot be delivered.
    for both in [false, true] {
        write(&f.request, &edits);
        let lost = f.root.path().join(format!("length-lost-{both}.fcad"));
        let mut cmd = f.edit(&lost);
        cmd.stdout(pipe::closed_pipe());
        if both {
            cmd.stderr(pipe::closed_pipe());
        }
        assert_eq!(cmd.output().expect("lost report").status.code(), Some(7));
        let d = Document::open_read_only(&lost).expect("published despite lost report");
        assert!(d.validate().expect("valid").is_ok());
        assert_eq!(
            inspect(&lost)["sketches"][0]["constraint_edit"]["constraints"]
                .as_array()
                .expect("constraints")
                .len(),
            10
        );
        d.close().expect("close");
    }
    assert_eq!(std::fs::read(&f.source).expect("source"), before);
    assert_eq!(
        std::fs::metadata(&f.source)
            .expect("meta")
            .modified()
            .expect("mtime"),
        mtime
    );
    let slanted = Fixture::new(true);
    write(
        &slanted.request,
        &json!({"request_version":1,"remove":[],"add":[length_add(&slanted.catalog,0,50.)]}),
    );
    let out = slanted.root.path().join("length-slanted.fcad");
    reply(slanted.edit(&out).output().expect("slanted length"), OP, 0);
    stored_same(&slanted.source, &out);
    length_geometry(&out, &[(0, 50.)], false);
}

fn pin_add(catalog: &Value, i: usize, at: &str, x: f64, y: f64) -> Value {
    json!({"curve_id":catalog["sketches"][0]["constraint_edit"]["curves"][i]["curve_id"],
           "rule":"fixed","at":at,"x_mm":x,"y_mm":y})
}
fn constraint_ids(catalog: &Value) -> Vec<Value> {
    catalog["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .map(|c| c["constraint_id"].clone())
        .collect()
}
/// Axis-aligned bounds of the solved polygon, measured from the presentation.
fn bounds(starts: &[[f64; 2]]) -> [[f64; 2]; 2] {
    [
        std::array::from_fn(|j| starts.iter().map(|p| p[j]).fold(f64::INFINITY, f64::min)),
        std::array::from_fn(|j| {
            starts
                .iter()
                .map(|p| p[j])
                .fold(f64::NEG_INFINITY, f64::max)
        }),
    ]
}

#[test]
fn native_fixed_endpoint_pins_the_body_and_removal_restores_two_degrees_of_freedom() {
    if !native() {
        return;
    }
    let f = Fixture::with_points(Some([[-40., -20.], [40., -20.], [40., 20.], [-40., 20.]]));
    let before = std::fs::read(&f.source).expect("source");
    let mtime = std::fs::metadata(&f.source)
        .expect("meta")
        .modified()
        .expect("mtime");

    // A fully dimensioned rectangle still floats: two translations remain.
    let dimensioned = f.root.path().join("dimensioned.fcad");
    write(&f.request, &rectangle_edits(&f.catalog));
    let sizes = reply(f.edit(&dimensioned).output().expect("dimensions"), OP, 0)["result"].clone();
    assert_eq!(sizes["solve"]["degrees_of_freedom"], 2);
    let sized = inspect(&dimensioned);
    let sized_ids = constraint_ids(&sized);
    assert_eq!(sized_ids.len(), 10);

    // Structural refusals keep the one-pin rule, publish nothing and touch nothing.
    for add in [
        json!([
            pin_add(&sized, 0, "start", 10., -5.),
            pin_add(&sized, 0, "end", 70., -5.)
        ]),
        json!([
            pin_add(&sized, 0, "start", 10., -5.),
            pin_add(&sized, 2, "start", 70., 25.)
        ]),
        json!([
            pin_add(&sized, 0, "start", 10., -5.),
            pin_add(&sized, 0, "start", 10., -5.)
        ]),
        json!([{"curve_id":ferritecad_types::StableEntityId::new(),"rule":"fixed","at":"start","x_mm":0,"y_mm":0}]),
    ] {
        let refused = f.root.path().join("refused.fcad");
        let directory = entries(f.root.path());
        write(
            &f.request,
            &json!({"request_version":1,"remove":[],"add":add}),
        );
        let v = reply(
            f.edit_from(&dimensioned, &sized, &refused)
                .output()
                .expect("structural refusal"),
            OP,
            2,
        );
        assert_eq!(v["error"]["kind"], "input");
        assert!(v["error"].get("constraint_conflict").is_none());
        assert!(!refused.exists());
        assert_eq!(entries(f.root.path()), directory);
    }

    // One pin at an explicit place removes exactly the two remaining freedoms.
    let pinned = f.root.path().join("pinned.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[],"add":[pin_add(&sized,0,"start",10.,-5.)]}),
    );
    let published = reply(
        f.edit_from(&dimensioned, &sized, &pinned)
            .output()
            .expect("pin"),
        OP,
        0,
    )["result"]
        .clone();
    let added = published["added_constraints"]
        .as_array()
        .expect("added")
        .clone();
    assert_eq!(
        added.len(),
        1,
        "closure already persisted; only the pin is new"
    );
    let rule = &added[0]["rule"];
    assert_eq!(rule["kind"], "fixed");
    assert_eq!(
        rule["point"]["curve_id"],
        sized["sketches"][0]["constraint_edit"]["curves"][0]["curve_id"]
    );
    assert_eq!(rule["point"]["at"], "start");
    assert_eq!(rule["x"], 10.);
    assert_eq!(rule["y"], -5.);
    assert!(
        rule.get("x_mm").is_none() && rule.get("y_mm").is_none() && rule.get("a").is_none(),
        "response wire keeps point/x/y"
    );
    assert_eq!(published["solve"]["degrees_of_freedom"], 0);
    assert_eq!(published["removed_constraint_ids"], json!([]));
    let pin_id = added[0]["constraint_id"].clone();
    assert!(!sized_ids.contains(&pin_id));
    let after = inspect(&pinned);
    assert_eq!(
        constraint_ids(&after),
        [sized_ids.clone(), vec![pin_id.clone()]].concat()
    );
    stored_same(&dimensioned, &pinned);

    // Cold reopen, real solver, independent STL: DOF is zero and the pinned
    // stored endpoint really landed on the millimetres that were asked for.
    let mut placed = [[0.; 2]; 2];
    geometry_checked(&pinned, Some(0), |starts, _, _| {
        placed = bounds(starts);
        let widths = [placed[1][0] - placed[0][0], placed[1][1] - placed[0][1]];
        assert!(
            (widths[0] - 60.).abs() < 1e-6 && (widths[1] - 30.).abs() < 1e-6,
            "{widths:?}"
        );
        assert!(
            (starts[0][0] - 10.).abs() < 1e-6 && (starts[0][1] + 5.).abs() < 1e-6,
            "the selected Line Start is not at (10, -5): {starts:?}"
        );
    });

    // Moving the pin translates the body and changes no dimension.
    let moved = f.root.path().join("pin-moved.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[pin_id],"add":[pin_add(&after,0,"start",-25.,40.)]}),
    );
    let replace = reply(
        f.edit_from(&pinned, &after, &moved)
            .output()
            .expect("move pin"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(replace["removed_constraint_ids"], json!([pin_id]));
    assert_eq!(replace["solve"]["degrees_of_freedom"], 0);
    let new_pin = replace["added_constraints"][0]["constraint_id"].clone();
    assert_ne!(new_pin, pin_id);
    let moved_catalog = inspect(&moved);
    assert_eq!(
        constraint_ids(&moved_catalog),
        [sized_ids.clone(), vec![new_pin.clone()]].concat(),
        "every other UUID and its order survive the replacement"
    );
    stored_same(&pinned, &moved);
    geometry_checked(&moved, Some(0), |starts, _, _| {
        let now = bounds(starts);
        for j in 0..2 {
            let delta = [-35., 45.][j];
            assert!((now[0][j] - placed[0][j] - delta).abs() < 1e-6, "{now:?}");
            assert!((now[1][j] - placed[1][j] - delta).abs() < 1e-6, "{now:?}");
        }
        assert!(
            (starts[0][0] + 25.).abs() < 1e-6 && (starts[0][1] - 40.).abs() < 1e-6,
            "the selected Line Start did not move with the pin: {starts:?}"
        );
    });

    // Removing the exact pin gives the two translations back and keeps closure.
    let unpinned = f.root.path().join("unpinned.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[new_pin],"add":[]}),
    );
    let removed = reply(
        f.edit_from(&moved, &moved_catalog, &unpinned)
            .output()
            .expect("remove pin"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(removed["solve"]["degrees_of_freedom"], 2);
    assert_eq!(removed["added_constraints"], json!([]));
    assert_eq!(constraint_ids(&inspect(&unpinned)), sized_ids);
    stored_same(&moved, &unpinned);
    length_geometry(&unpinned, &[(0, 60.), (1, 30.)], true);

    // Zero and negative millimetres are ordinary coordinates.
    let origin = f.root.path().join("pin-origin.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[],"add":[pin_add(&sized,1,"end",0.,-0.)]}),
    );
    let at_origin = reply(
        f.edit_from(&dimensioned, &sized, &origin)
            .output()
            .expect("pin at the origin"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(at_origin["solve"]["degrees_of_freedom"], 0);
    assert_eq!(at_origin["added_constraints"][0]["rule"]["x"], 0.);
    assert_eq!(at_origin["added_constraints"][0]["rule"]["y"], 0.);
    geometry_checked(&origin, Some(0), |_, ends, _| {
        assert!(
            ends[1][0].abs() < 1e-6 && ends[1][1].abs() < 1e-6,
            "the selected second Line End is not at the origin: {ends:?}"
        );
    });

    // A real solver failure is separate from the structural ones, and the
    // one-pin rule is not weakened to produce it.
    let mut contradictory = rectangle_edits(&f.catalog);
    contradictory["add"].as_array_mut().expect("add").extend([
        length_add(&f.catalog, 2, 70.),
        pin_add(&f.catalog, 0, "start", 1., 2.),
    ]);
    write(&f.request, &contradictory);
    let failed = f.root.path().join("pin-conflict.fcad");
    let directory = entries(f.root.path());
    let refusal = reply(f.edit(&failed).output().expect("real conflict"), OP, 2);
    assert_eq!(refusal["error"]["kind"], "constraint", "{refusal}");
    assert!(
        !refusal["error"]["constraint_conflict"]["constraints"]
            .as_array()
            .expect("typed conflict")
            .is_empty()
    );
    assert!(!failed.exists());
    assert_eq!(entries(f.root.path()), directory);

    // A pin reaches publication even when its report cannot be delivered.
    write(
        &f.request,
        &json!({"request_version":1,"remove":[],"add":[pin_add(&sized,0,"start",10.,-5.)]}),
    );
    let lost = f.root.path().join("pin-lost.fcad");
    let mut cmd = f.edit_from(&dimensioned, &sized, &lost);
    cmd.stdout(pipe::closed_pipe());
    assert_eq!(cmd.output().expect("lost report").status.code(), Some(7));
    let d = Document::open_read_only(&lost).expect("published despite lost report");
    assert!(d.validate().expect("valid").is_ok());
    d.close().expect("close");
    assert_eq!(constraint_ids(&inspect(&lost)).len(), 11);

    assert_eq!(std::fs::read(&f.source).expect("source"), before);
    assert_eq!(
        std::fs::metadata(&f.source)
            .expect("meta")
            .modified()
            .expect("mtime"),
        mtime
    );
}

fn equal_add(catalog: &Value, i: usize, j: usize) -> Value {
    let c = &catalog["sketches"][0]["constraint_edit"]["curves"];
    json!({"rule":"equal_length","a_curve_id":c[i]["curve_id"],"b_curve_id":c[j]["curve_id"]})
}
/// Where the named curve sits in the stored order the solved presentation uses.
fn index_of(catalog: &Value, id: &Value) -> usize {
    catalog["sketches"][0]["constraint_edit"]["curves"]
        .as_array()
        .expect("curves")
        .iter()
        .position(|c| c["curve_id"] == *id)
        .expect("stored curve")
}
fn solved_length(starts: &[[f64; 2]], ends: &[[f64; 2]], i: usize) -> f64 {
    (ends[i][0] - starts[i][0]).hypot(ends[i][1] - starts[i][1])
}
/// The solved footprint of a four-Line profile, before independent STL integration.
fn square_of(starts: &[[f64; 2]], side: f64) {
    let [lo, hi] = bounds(starts);
    for j in 0..2 {
        assert!((hi[j] - lo[j] - side).abs() < 1e-6, "{lo:?} {hi:?}");
    }
    let area2 = starts
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let b = starts[(i + 1) % starts.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        .abs();
    assert!(
        (area2 / 2. * 10. - side * side * 10.).abs() < 1e-4,
        "expected {} mm3",
        side * side * 10.
    );
}

#[test]
fn native_equal_length_ties_two_lines_and_removal_restores_the_free_dimension() {
    if !native() {
        return;
    }
    let f = Fixture::with_points(Some([[-40., -20.], [40., -20.], [40., 20.], [-40., 20.]]));
    let before = std::fs::read(&f.source).expect("source");
    let mtime = std::fs::metadata(&f.source)
        .expect("meta")
        .modified()
        .expect("mtime");

    // H/V, one length and one pin leave exactly one free dimension.
    let base = f.root.path().join("one-size.fcad");
    let mut add: Vec<_> = (0..4)
        .map(|i| json!({"curve_id":f.catalog["sketches"][0]["constraint_edit"]["curves"][i]["curve_id"],"rule":if i%2==0 {"horizontal"} else {"vertical"}}))
        .collect();
    add.extend([
        length_add(&f.catalog, 0, 60.),
        pin_add(&f.catalog, 0, "start", 10., -5.),
    ]);
    write(
        &f.request,
        &json!({"request_version":1,"remove":[],"add":add}),
    );
    let sized = reply(f.edit(&base).output().expect("one size and a pin"), OP, 0)["result"].clone();
    assert_eq!(sized["solve"]["degrees_of_freedom"], 1);
    let catalog = inspect(&base);
    let base_ids = constraint_ids(&catalog);
    assert_eq!(base_ids.len(), 10);
    let length_id = catalog["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .find(|c| c["rule"]["kind"] == "distance")
        .expect("leading length")["constraint_id"]
        .clone();

    // Structural refusals happen before the solver and publish nothing.
    for add in [
        json!([equal_add(&catalog, 0, 0)]),
        json!([{"rule":"equal_length","a_curve_id":catalog["sketches"][0]["constraint_edit"]["curves"][0]["curve_id"],"b_curve_id":ferritecad_types::StableEntityId::new()}]),
        json!([equal_add(&catalog, 0, 1), equal_add(&catalog, 0, 1)]),
        json!([equal_add(&catalog, 0, 1), equal_add(&catalog, 1, 0)]),
    ] {
        let refused = f.root.path().join("never.fcad");
        let directory = entries(f.root.path());
        write(
            &f.request,
            &json!({"request_version":1,"remove":[],"add":add}),
        );
        let v = reply(
            f.edit_from(&base, &catalog, &refused)
                .output()
                .expect("structural refusal"),
            OP,
            2,
        );
        assert_eq!(v["error"]["kind"], "input");
        assert!(v["error"].get("constraint_conflict").is_none());
        assert!(!refused.exists());
        assert_eq!(entries(f.root.path()), directory);
    }

    // One equality between two named Lines removes the last dimension.
    let square = f.root.path().join("square.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[],"add":[equal_add(&catalog,0,1)]}),
    );
    let tied = reply(
        f.edit_from(&base, &catalog, &square)
            .output()
            .expect("equal length"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(tied["solve"]["degrees_of_freedom"], 0);
    assert_eq!(tied["solve"]["redundant_constraint_ids"], json!([]));
    let added = tied["added_constraints"].as_array().expect("added").clone();
    assert_eq!(added.len(), 1, "closure already persisted");
    let rule = &added[0]["rule"];
    assert_eq!(rule["kind"], "equal_length");
    let curves = &catalog["sketches"][0]["constraint_edit"]["curves"];
    for (side, i) in [("a", 0), ("b", 1)] {
        assert_eq!(rule[side]["from"]["curve_id"], curves[i]["curve_id"]);
        assert_eq!(rule[side]["from"]["at"], "start");
        assert_eq!(rule[side]["to"]["curve_id"], curves[i]["curve_id"]);
        assert_eq!(rule[side]["to"]["at"], "end");
    }
    assert!(
        rule.get("a_curve_id").is_none() && rule.get("b_curve_id").is_none(),
        "the response DTO keeps its Segment a/b, not the request fields"
    );
    let equal_id = added[0]["constraint_id"].clone();
    assert!(!base_ids.contains(&equal_id));
    let square_catalog = inspect(&square);
    assert_eq!(
        constraint_ids(&square_catalog),
        [base_ids.clone(), vec![equal_id.clone()]].concat()
    );
    stored_same(&base, &square);
    let (i, j) = (
        index_of(&square_catalog, &rule["a"]["from"]["curve_id"]),
        index_of(&square_catalog, &rule["b"]["from"]["curve_id"]),
    );
    assert_ne!(i, j);
    geometry_checked(&square, Some(0), |starts, ends, _| {
        let (a, b) = (
            solved_length(starts, ends, i),
            solved_length(starts, ends, j),
        );
        assert!(
            (a - b).abs() < 1e-6 && (a - 60.).abs() < 1e-6,
            "the two Lines named by the equality are not both 60: {a} {b}"
        );
        square_of(starts, 60.);
    });

    // Changing the leading length moves both sides and keeps the equality UUID.
    let smaller = f.root.path().join("smaller.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[length_id],"add":[length_add(&square_catalog,0,45.)]}),
    );
    let replaced = reply(
        f.edit_from(&square, &square_catalog, &smaller)
            .output()
            .expect("replace the leading length"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(replaced["removed_constraint_ids"], json!([length_id]));
    assert_eq!(replaced["solve"]["degrees_of_freedom"], 0);
    let smaller_catalog = inspect(&smaller);
    let smaller_ids = constraint_ids(&smaller_catalog);
    let kept_rules: Vec<_> = square_catalog["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .filter(|c| c["constraint_id"] != length_id)
        .cloned()
        .collect();
    assert_eq!(
        smaller_catalog["sketches"][0]["constraint_edit"]["constraints"],
        json!(
            [
                kept_rules,
                replaced["added_constraints"]
                    .as_array()
                    .expect("new length")
                    .clone()
            ]
            .concat()
        ),
        "replacement keeps every other constraint UUID, rule and order"
    );
    assert!(
        smaller_ids.contains(&equal_id),
        "the equality keeps its own UUID across a length replacement"
    );
    assert_eq!(smaller_ids.len(), 11);
    stored_same(&square, &smaller);
    geometry_checked(&smaller, Some(0), |starts, ends, _| {
        let (a, b) = (
            solved_length(starts, ends, i),
            solved_length(starts, ends, j),
        );
        assert!(
            (a - 45.).abs() < 1e-6 && (b - 45.).abs() < 1e-6,
            "the equality did not follow the new leading length: {a} {b}"
        );
        square_of(starts, 45.);
    });

    // The stored pair occupies its slot in either order.
    for add in [
        json!([equal_add(&smaller_catalog, 0, 1)]),
        json!([equal_add(&smaller_catalog, 1, 0)]),
    ] {
        write(
            &f.request,
            &json!({"request_version":1,"remove":[],"add":add}),
        );
        let refused = f.root.path().join("duplicate.fcad");
        let v = reply(
            f.edit_from(&smaller, &smaller_catalog, &refused)
                .output()
                .expect("stored duplicate"),
            OP,
            2,
        );
        assert_eq!(v["error"]["kind"], "input");
        assert!(!refused.exists());
    }

    // Removing the exact equality gives the free dimension back and keeps the rest.
    let freed = f.root.path().join("freed.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[equal_id],"add":[]}),
    );
    let removed = reply(
        f.edit_from(&smaller, &smaller_catalog, &freed)
            .output()
            .expect("remove the equality"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(removed["solve"]["degrees_of_freedom"], 1);
    assert_eq!(removed["added_constraints"], json!([]));
    let freed_catalog = inspect(&freed);
    assert_eq!(
        constraint_ids(&freed_catalog),
        smaller_ids
            .iter()
            .filter(|id| **id != equal_id)
            .cloned()
            .collect::<Vec<_>>(),
        "every other UUID and its order survive"
    );
    let kinds: Vec<_> = freed_catalog["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .map(|c| c["rule"]["kind"].clone())
        .collect();
    assert_eq!(kinds.iter().filter(|k| **k == "coincident").count(), 4);
    assert_eq!(kinds.iter().filter(|k| **k == "horizontal").count(), 2);
    assert_eq!(kinds.iter().filter(|k| **k == "vertical").count(), 2);
    assert_eq!(kinds.iter().filter(|k| **k == "distance").count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == "fixed").count(), 1);
    stored_same(&smaller, &freed);
    // H/V, the leading length and the pin still hold in the solved model; only
    // the side the equality used to tie is free again.
    geometry_checked(&freed, Some(1), |starts, ends, _| {
        assert!((solved_length(starts, ends, i) - 45.).abs() < 1e-6);
        assert!(
            (starts[0][0] - 10.).abs() < 1e-6 && (starts[0][1] + 5.).abs() < 1e-6,
            "the pinned Start moved: {starts:?}"
        );
        for k in [0, 2] {
            assert!(
                (starts[k][1] - ends[k][1]).abs() < 1e-7,
                "H lost on line {k}"
            );
        }
        for k in [1, 3] {
            assert!(
                (starts[k][0] - ends[k][0]).abs() < 1e-7,
                "V lost on line {k}"
            );
        }
    });

    // Redundancy is the real solver's diagnosis, not a structural refusal.
    let redundant = f.root.path().join("redundant.fcad");
    let mut both: Vec<_> = (0..4)
        .map(|i| json!({"curve_id":f.catalog["sketches"][0]["constraint_edit"]["curves"][i]["curve_id"],"rule":if i%2==0 {"horizontal"} else {"vertical"}}))
        .collect();
    both.extend([
        length_add(&f.catalog, 0, 60.),
        length_add(&f.catalog, 1, 60.),
        pin_add(&f.catalog, 0, "start", 10., -5.),
        equal_add(&f.catalog, 0, 1),
    ]);
    write(
        &f.request,
        &json!({"request_version":1,"remove":[],"add":both}),
    );
    let said_twice = reply(
        f.edit(&redundant).output().expect("redundant equality"),
        OP,
        0,
    )["result"]
        .clone();
    let equality = said_twice["added_constraints"]
        .as_array()
        .expect("added")
        .iter()
        .find(|c| c["rule"]["kind"] == "equal_length")
        .expect("equality")["constraint_id"]
        .clone();
    assert_eq!(
        said_twice["solve"]["redundant_constraint_ids"],
        json!([equality]),
        "the solver names the equality that says what two lengths already said"
    );
    assert_eq!(said_twice["solve"]["degrees_of_freedom"], 0);

    // Contradictory stored lengths plus an equality are a real solver conflict.
    let mut contradictory = rectangle_edits(&f.catalog);
    contradictory["add"]
        .as_array_mut()
        .expect("add")
        .push(equal_add(&f.catalog, 0, 1));
    write(&f.request, &contradictory);
    let failed = f.root.path().join("conflict.fcad");
    let directory = entries(f.root.path());
    let refusal = reply(f.edit(&failed).output().expect("solver conflict"), OP, 2);
    assert_eq!(refusal["error"]["kind"], "constraint", "{refusal}");
    let conflict = refusal["error"]["constraint_conflict"]["constraints"]
        .as_array()
        .expect("typed conflict");
    assert!(conflict.iter().any(|c| c["rule"]["kind"] == "equal_length"));
    assert!(conflict.iter().all(|c| c["constraint_id"].is_string()));
    assert!(!failed.exists());
    assert_eq!(entries(f.root.path()), directory);
    assert_eq!(std::fs::read(&f.source).expect("source"), before);
    assert_eq!(
        std::fs::metadata(&f.source)
            .expect("meta")
            .modified()
            .expect("mtime"),
        mtime
    );

    // Equal length is the Euclidean length of a Line, not its X or Y extent.
    let slanted = Fixture::new(true);
    write(
        &slanted.request,
        &json!({"request_version":1,"remove":[],"add":[length_add(&slanted.catalog,0,50.),equal_add(&slanted.catalog,0,1)]}),
    );
    let out = slanted.root.path().join("slanted-equal.fcad");
    reply(
        slanted.edit(&out).output().expect("slanted equality"),
        OP,
        0,
    );
    stored_same(&slanted.source, &out);
    geometry_checked(&out, None, |starts, ends, _| {
        for k in [0, 1] {
            let (dx, dy) = (ends[k][0] - starts[k][0], ends[k][1] - starts[k][1]);
            assert!(
                (dx.hypot(dy) - 50.).abs() < 1e-6,
                "equal length is not Euclidean on line {k}: {dx}/{dy}"
            );
            assert!(
                (dx.abs() - 50.).abs() > 0.01 && (dy.abs() - 50.).abs() > 0.01,
                "a slanted equal length must not be an axis projection: {dx}/{dy}"
            );
        }
    });
}

fn relation_add(catalog: &Value, rule: &str, i: usize, j: usize) -> Value {
    let c = &catalog["sketches"][0]["constraint_edit"]["curves"];
    json!({"rule":rule,"a_curve_id":c[i]["curve_id"],"b_curve_id":c[j]["curve_id"]})
}
/// The unit direction of one solved Line. A relative orientation is scored on
/// directions, so a side that solved to nothing would satisfy anything.
fn direction(starts: &[[f64; 2]], ends: &[[f64; 2]], i: usize) -> [f64; 2] {
    let (dx, dy) = (ends[i][0] - starts[i][0], ends[i][1] - starts[i][1]);
    let length = dx.hypot(dy);
    assert!(length > 1., "line {i} solved to a degenerate {length} mm");
    [dx / length, dy / length]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
/// The two stored-order indices one published pair rule names, in its own order.
fn related_indices(catalog: &Value, rule: &Value) -> (usize, usize) {
    for side in ["a", "b"] {
        assert_eq!(rule[side]["from"]["curve_id"], rule[side]["to"]["curve_id"]);
        assert_eq!(rule[side]["from"]["at"], "start");
        assert_eq!(rule[side]["to"]["at"], "end");
    }
    assert!(
        rule.get("a_curve_id").is_none()
            && rule.get("b_curve_id").is_none()
            && rule.get("distance").is_none(),
        "the response DTO keeps its Segment a/b, not the request fields"
    );
    let (i, j) = (
        index_of(catalog, &rule["a"]["from"]["curve_id"]),
        index_of(catalog, &rule["b"]["from"]["curve_id"]),
    );
    assert_ne!(i, j);
    (i, j)
}
/// The solved footprint of a profile whose shape relative orientation decided.
/// Its area is what the sides say; its axis-aligned extents are not, because
/// the whole profile is still free to turn.
fn area_of(starts: &[[f64; 2]]) -> f64 {
    starts
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let b = starts[(i + 1) % starts.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        .abs()
        / 2.
}
/// Neither side of this profile may have become an axis, which is what a
/// Parallel or Perpendicular quietly replaced by H/V would look like.
fn slanted(starts: &[[f64; 2]], ends: &[[f64; 2]]) {
    for i in 0..starts.len() {
        let u = direction(starts, ends, i);
        assert!(
            u[0].abs() > 0.01 && u[1].abs() > 0.01,
            "line {i} solved onto an axis: {u:?}"
        );
    }
}

#[test]
fn native_line_relations_orient_two_lines_and_removal_restores_the_free_dimension() {
    if !native() {
        return;
    }
    let f = Fixture::new(true);
    let before = std::fs::read(&f.source).expect("source");
    let mtime = std::fs::metadata(&f.source)
        .expect("meta")
        .modified()
        .expect("mtime");

    // A rectangle built only out of relative orientation and two sizes. The
    // closed four-Line profile has eight planar degrees of freedom; two
    // Parallels and one Perpendicular fix its shape to a rectangle (three), and
    // the two lengths fix its size, leaving exactly the three the profile is
    // entitled to keep: two of position and one of rotation. The solver is
    // asked to confirm that count, and reports nothing redundant.
    let rectangle = f.root.path().join("relations-rect.fcad");
    let add = json!([
        relation_add(&f.catalog, "parallel", 0, 2),
        relation_add(&f.catalog, "parallel", 1, 3),
        relation_add(&f.catalog, "perpendicular", 0, 1),
        length_add(&f.catalog, 0, 60.),
        length_add(&f.catalog, 1, 30.),
    ]);
    write(
        &f.request,
        &json!({"request_version":1,"remove":[],"add":add}),
    );
    let built = reply(
        f.edit(&rectangle).output().expect("relative orientation"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(built["solve"]["degrees_of_freedom"], 3);
    assert_eq!(built["solve"]["redundant_constraint_ids"], json!([]));
    let added = built["added_constraints"]
        .as_array()
        .expect("added")
        .clone();
    assert_eq!(added.len(), 9, "four Coincident joints and five additions");
    let kinds: Vec<_> = added.iter().map(|c| c["rule"]["kind"].clone()).collect();
    assert_eq!(
        kinds,
        json!([
            "coincident",
            "coincident",
            "coincident",
            "coincident",
            "parallel",
            "parallel",
            "perpendicular",
            "distance",
            "distance"
        ])
        .as_array()
        .expect("kinds")
        .clone()
    );
    // Measure the requested pair, not just whichever pair the response names.
    // Other corners of a rectangle can satisfy the same relation too.
    for (published, requested) in added[4..7]
        .iter()
        .zip(add.as_array().expect("requested additions"))
    {
        let rule = &published["rule"];
        assert_eq!(rule["kind"], requested["rule"]);
        for (side, field) in [("a", "a_curve_id"), ("b", "b_curve_id")] {
            for endpoint in ["from", "to"] {
                assert_eq!(
                    rule[side][endpoint]["curve_id"], requested[field],
                    "published relation must name the requested {side} Line"
                );
            }
        }
    }
    let rect_catalog = inspect(&rectangle);
    let rect_ids = constraint_ids(&rect_catalog);
    let named = |kind: &str, nth: usize| -> (Value, (usize, usize)) {
        let c = added
            .iter()
            .filter(|c| c["rule"]["kind"] == kind)
            .nth(nth)
            .expect("published relation")
            .clone();
        let pair = related_indices(&rect_catalog, &c["rule"]);
        (c["constraint_id"].clone(), pair)
    };
    let (parallel_id, (p0, p1)) = named("parallel", 0);
    let (other_parallel_id, (q0, q1)) = named("parallel", 1);
    let (perpendicular_id, (r0, r1)) = named("perpendicular", 0);
    let length_id = added
        .iter()
        .find(|c| c["rule"]["kind"] == "distance")
        .expect("leading length")["constraint_id"]
        .clone();
    stored_same(&f.source, &rectangle);
    // Exactly the UUIDs the relations name are measured, not any side that
    // happens to fit. The profile remains non-axis-aligned; its angle is free.
    let measured = move |starts: &[[f64; 2]], ends: &[[f64; 2]], long: f64| {
        for (a, b) in [(p0, p1), (q0, q1)] {
            let (u, v) = (direction(starts, ends, a), direction(starts, ends, b));
            assert!(
                cross(u, v).abs() < 1e-7 && dot(u, v).abs() > 1. - 1e-7,
                "Lines {a} and {b} are not parallel: {u:?} {v:?}"
            );
        }
        let (u, v) = (direction(starts, ends, r0), direction(starts, ends, r1));
        assert!(
            dot(u, v).abs() < 1e-7 && cross(u, v).abs() > 1. - 1e-7,
            "Lines {r0} and {r1} are not perpendicular: {u:?} {v:?}"
        );
        assert!((solved_length(starts, ends, 0) - long).abs() < 1e-6);
        assert!((solved_length(starts, ends, 1) - 30.).abs() < 1e-6);
        slanted(starts, ends);
    };
    geometry_checked(&rectangle, Some(3), |starts, ends, _| {
        measured(starts, ends, 60.);
        assert!(
            (area_of(starts) * 10. - 18000.).abs() < 1e-4,
            "expected 18000 mm3, before independent STL integration"
        );
    });

    // Structural refusals happen before the solver and publish nothing.
    for add in [
        json!([relation_add(&rect_catalog, "parallel", 0, 0)]),
        json!([relation_add(&rect_catalog, "perpendicular", 2, 2)]),
        json!([{"rule":"parallel","a_curve_id":rect_catalog["sketches"][0]["constraint_edit"]["curves"][0]["curve_id"],"b_curve_id":ferritecad_types::StableEntityId::new()}]),
        // One question per pair, whichever way round it is asked and whichever
        // of the two answers it asks for.
        json!([relation_add(&rect_catalog, "parallel", p0, p1)]),
        json!([relation_add(&rect_catalog, "parallel", p1, p0)]),
        json!([relation_add(&rect_catalog, "perpendicular", p0, p1)]),
        json!([relation_add(&rect_catalog, "perpendicular", p1, p0)]),
        json!([
            relation_add(&rect_catalog, "parallel", 1, 2),
            relation_add(&rect_catalog, "perpendicular", 2, 1)
        ]),
    ] {
        let refused = f.root.path().join("never.fcad");
        let directory = entries(f.root.path());
        write(
            &f.request,
            &json!({"request_version":1,"remove":[],"add":add}),
        );
        let v = reply(
            f.edit_from(&rectangle, &rect_catalog, &refused)
                .output()
                .expect("structural refusal"),
            OP,
            2,
        );
        assert_eq!(v["error"]["kind"], "input");
        assert!(v["error"].get("constraint_conflict").is_none());
        assert!(!refused.exists());
        assert_eq!(entries(f.root.path()), directory);
    }
    // An equal length on a pair that already holds a relative orientation is a
    // different property of the same two Lines, not a second answer to one
    // question, so it is accepted and published.
    let both = f.root.path().join("relations-and-equal.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[],"add":[equal_add(&rect_catalog,p0,p1)]}),
    );
    let independent = reply(
        f.edit_from(&rectangle, &rect_catalog, &both)
            .output()
            .expect("independent property"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(
        independent["added_constraints"][0]["rule"]["kind"],
        "equal_length"
    );
    assert_eq!(
        constraint_ids(&inspect(&both)).len(),
        rect_ids.len() + 1,
        "the equality is one more constraint, not a replacement"
    );

    // Changing the leading length keeps every relation, its UUID and the shape.
    let smaller = f.root.path().join("relations-smaller.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[length_id],"add":[length_add(&rect_catalog,0,45.)]}),
    );
    let replaced = reply(
        f.edit_from(&rectangle, &rect_catalog, &smaller)
            .output()
            .expect("replace the leading length"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(replaced["removed_constraint_ids"], json!([length_id]));
    assert_eq!(replaced["solve"]["degrees_of_freedom"], 3);
    let smaller_catalog = inspect(&smaller);
    let smaller_ids = constraint_ids(&smaller_catalog);
    let kept_rules: Vec<_> = rect_catalog["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .filter(|c| c["constraint_id"] != length_id)
        .cloned()
        .collect();
    assert_eq!(
        smaller_catalog["sketches"][0]["constraint_edit"]["constraints"],
        json!(
            [
                kept_rules,
                replaced["added_constraints"]
                    .as_array()
                    .expect("new length")
                    .clone()
            ]
            .concat()
        ),
        "replacement keeps every other constraint UUID, rule and order"
    );
    for id in [&parallel_id, &other_parallel_id, &perpendicular_id] {
        assert!(
            smaller_ids.contains(id),
            "a relation keeps its own UUID across a length replacement"
        );
    }
    stored_same(&rectangle, &smaller);
    geometry_checked(&smaller, Some(3), |starts, ends, _| {
        measured(starts, ends, 45.);
        assert!(
            (area_of(starts) * 10. - 13500.).abs() < 1e-4,
            "expected 13500 mm3, before independent STL integration"
        );
    });

    // Removing the exact Perpendicular gives one degree of freedom back and
    // leaves every other relationship standing. Where the freed profile lands
    // is the solver's business; that it kept both Parallels and both sizes is
    // not.
    let freed = f.root.path().join("relations-freed.fcad");
    write(
        &f.request,
        &json!({"request_version":1,"remove":[perpendicular_id],"add":[]}),
    );
    let removed = reply(
        f.edit_from(&smaller, &smaller_catalog, &freed)
            .output()
            .expect("remove the perpendicular"),
        OP,
        0,
    )["result"]
        .clone();
    assert_eq!(removed["added_constraints"], json!([]));
    assert_eq!(removed["solve"]["degrees_of_freedom"], 4);
    let freed_catalog = inspect(&freed);
    assert_eq!(
        constraint_ids(&freed_catalog),
        smaller_ids
            .iter()
            .filter(|id| **id != perpendicular_id)
            .cloned()
            .collect::<Vec<_>>(),
        "every other UUID and its order survive"
    );
    stored_same(&smaller, &freed);
    geometry_checked(&freed, Some(4), |starts, ends, _| {
        for (a, b) in [(p0, p1), (q0, q1)] {
            let (u, v) = (direction(starts, ends, a), direction(starts, ends, b));
            assert!(
                cross(u, v).abs() < 1e-7,
                "Lines {a} and {b} lost their Parallel: {u:?} {v:?}"
            );
        }
        assert!((solved_length(starts, ends, 0) - 45.).abs() < 1e-6);
        assert!((solved_length(starts, ends, 1) - 30.).abs() < 1e-6);
    });

    // Redundancy is the real solver's diagnosis, not a structural refusal: a
    // fourth corner that the other three already decided.
    let redundant = f.root.path().join("relations-redundant.fcad");
    let mut said_again = add.as_array().expect("add").clone();
    said_again.push(relation_add(&f.catalog, "perpendicular", 1, 2));
    write(
        &f.request,
        &json!({"request_version":1,"remove":[],"add":said_again}),
    );
    let twice = reply(
        f.edit(&redundant).output().expect("redundant corner"),
        OP,
        0,
    )["result"]
        .clone();
    let repeated = twice["added_constraints"]
        .as_array()
        .expect("added")
        .iter()
        .filter(|c| c["rule"]["kind"] == "perpendicular")
        .nth(1)
        .expect("the fourth corner")["constraint_id"]
        .clone();
    assert_eq!(
        twice["solve"]["redundant_constraint_ids"],
        json!([repeated]),
        "the solver names the corner the other relations already decided"
    );
    assert_eq!(twice["solve"]["degrees_of_freedom"], 3);

    // A relation that contradicts the rest is a real solver conflict.
    let mut contradictory = add.as_array().expect("add").clone();
    contradictory.push(relation_add(&f.catalog, "parallel", 1, 2));
    write(
        &f.request,
        &json!({"request_version":1,"remove":[],"add":contradictory}),
    );
    let failed = f.root.path().join("relations-conflict.fcad");
    let directory = entries(f.root.path());
    let refusal = reply(f.edit(&failed).output().expect("solver conflict"), OP, 2);
    assert_eq!(refusal["error"]["kind"], "constraint", "{refusal}");
    let conflict = refusal["error"]["constraint_conflict"]["constraints"]
        .as_array()
        .expect("typed conflict");
    assert!(conflict.iter().any(|c| c["rule"]["kind"] == "parallel"));
    assert!(
        conflict
            .iter()
            .any(|c| c["rule"]["kind"] == "perpendicular")
    );
    assert!(conflict.iter().all(|c| c["constraint_id"].is_string()));
    assert!(!failed.exists());
    assert_eq!(entries(f.root.path()), directory);
    assert_eq!(std::fs::read(&f.source).expect("source"), before);
    assert_eq!(
        std::fs::metadata(&f.source)
            .expect("meta")
            .modified()
            .expect("mtime"),
        mtime
    );
}
