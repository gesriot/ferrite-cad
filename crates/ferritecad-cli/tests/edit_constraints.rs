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
fn geometry_checked(
    path: &Path,
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
    assert!(report.degrees_of_freedom() > 0);
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
    geometry_checked(path, |starts, ends, s| {
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
    geometry_checked(path, |starts, ends, s| {
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
            s.constraints.push(ferritecad_document::SketchConstraint {
                id: ferritecad_types::StableEntityId::new(),
                rule: ferritecad_document::SketchConstraintRule::Fixed {
                    point: ferritecad_document::SketchPointRef::new(
                        s.curves[0].id,
                        ferritecad_document::SketchPointSelector::Start,
                    ),
                    x: 0.,
                    y: 0.,
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
    geometry_checked(&replacement, |starts, ends, _| {
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
