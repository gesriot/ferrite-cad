// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::{CapSide, Document, ObjectPayload, SemanticRole, SketchGeometry};
use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;
#[path = "support/transaction.rs"]
mod transaction;
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn reply(out: Output, operation: &str, code: i32) -> Value {
    assert_eq!(out.status.code(), Some(code), "{out:?}");
    assert_eq!(
        out.stdout.iter().filter(|&&b| b == b'\n').count(),
        1,
        "{out:?}"
    );
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
struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    request: PathBuf,
    catalog: Value,
}
impl Fixture {
    fn sample() -> Self {
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("контур space.fcad");
        reply(
            cli()
                .arg("create")
                .arg(&source)
                .args(["--sample", "--size", "60", "40", "10", "--json"])
                .output()
                .expect("create"),
            "create",
            0,
        );
        let request = root.path().join("coordinates.json");
        let catalog = inspect(&source);
        let f = Self {
            root,
            source,
            request,
            catalog,
        };
        f.write_edit();
        f
    }
    fn write_edit(&self) {
        let sketch = &self.catalog["sketches"][0];
        let points: Vec<[f64; 2]> = sketch["curves"]
            .as_array()
            .expect("curves")
            .iter()
            .map(|curve| {
                let p = &curve["geometry"]["start_mm"];
                let x = p[0].as_f64().expect("x");
                [if x == 60. { 80. } else { x }, p[1].as_f64().expect("y")]
            })
            .collect();
        write(&self.request, &transaction::line_changes(sketch, &points));
    }
    fn edit(&self, output: &Path) -> Command {
        transaction::command(
            &self.source,
            self.catalog["content_version"].as_str().expect("version"),
            &self.request,
            output,
        )
    }
}
fn native() -> bool {
    if ferritecad_occt::is_available() {
        true
    } else {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        false
    }
}

#[test]
fn native_edit_sketch_process_identity_geometry_and_delivery() {
    if !native() {
        return;
    }
    let mut f = Fixture::sample();
    std::fs::remove_file(&f.source).expect("only private sample");
    let input = f.root.path().join("L.json");
    write(
        &input,
        &json!({"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10}),
    );
    reply(
        cli()
            .arg("create-sketch-extrude")
            .arg(&input)
            .arg("-o")
            .arg(&f.source)
            .arg("--json")
            .output()
            .expect("create L"),
        "create-sketch-extrude",
        0,
    );
    let raw = rusqlite::Connection::open(&f.source).expect("SQL");
    raw.execute_batch("CREATE TABLE extension(value BLOB); INSERT INTO extension(rowid,value) VALUES(73,X'00FF');").expect("extra data");
    // A coordinate edit must also preserve source claims associated with the
    // selected object, even when they are not needed to rebuild this polygon.
    let stored = b"preserve attached sketch source";
    raw.execute(
        "INSERT INTO imported_sources(id,format,bytes,content_hash,byte_len,created_at) \
         VALUES(zeroblob(16),'future.sketch-source',?1,?2,?3,'saved')",
        rusqlite::params![
            stored.as_slice(),
            ferritecad_types::ContentHash::of_bytes(stored)
                .as_bytes()
                .as_slice(),
            stored.len()
        ],
    )
    .expect("attached bytes");
    raw.execute_batch(
        "INSERT INTO imported_source_refs SELECT id,zeroblob(16) FROM objects WHERE kind='sketch';",
    )
    .expect("source claim");
    drop(raw);
    f.catalog = inspect(&f.source);
    f.write_edit();
    let before = std::fs::read(&f.source).expect("bytes");
    let mtime = std::fs::metadata(&f.source)
        .expect("metadata")
        .modified()
        .expect("mtime");
    let out = f.root.path().join("edited L.fcad");
    let v = reply(f.edit(&out).output().expect("edit"), "edit", 0);
    assert_eq!(v["result"]["destination"], out.to_str().expect("path"));
    assert_eq!(v["result"]["document_id"], f.catalog["document_id"]);
    check_copy(&f.source, &out, 6);
    let after = inspect(&out);
    assert_ne!(after["content_version"], f.catalog["content_version"]);
    assert_eq!(after["features"], f.catalog["features"]);
    // §26I: `cut_edit_v3` states the part's real outline, which this edit
    // changed; every other Body block says what it said before.
    let without_v3 = |bodies: &Value| {
        let mut bodies = bodies.clone();
        for body in bodies.as_array_mut().expect("bodies") {
            body.as_object_mut().expect("body").remove("cut_edit_v3");
        }
        bodies
    };
    assert_eq!(
        without_v3(&after["bodies"]),
        without_v3(&f.catalog["bodies"])
    );
    let target = &after["bodies"][0]["cut_edit_v3"]["target"];
    assert_eq!(target["bounds_mm"], json!([[0., 0.], [80., 40.]]));
    assert_eq!(target["boundary"]["area_mm2"], json!(2000.));
    assert_eq!(
        after["sketches"][0]["curves"][1]["geometry"]["start_mm"],
        json!([80., 0.])
    );
    assert_eq!(
        after["sketches"][0]["curves"][2]["geometry"]["start_mm"],
        json!([80., 20.])
    );
    let checked = reply(
        cli()
            .arg("validate")
            .arg(&out)
            .arg("--json")
            .output()
            .expect("validate"),
        "validate",
        0,
    );
    assert_eq!(checked["result"]["valid"], true);
    let rebuild = cli()
        .arg("rebuild")
        .arg(&out)
        .arg("--cold")
        .output()
        .expect("cold");
    assert!(rebuild.status.success(), "{rebuild:?}");
    check_reference_domains(&out);
    let stl = f.root.path().join("edited.stl");
    let mesh = reply(
        cli()
            .arg("export-stl")
            .arg(&out)
            .arg("--solid")
            .arg(after["bodies"][0]["body_id"].as_str().expect("body"))
            .arg("-o")
            .arg(&stl)
            .arg("--json")
            .output()
            .expect("STL"),
        "export-stl",
        0,
    );
    let bytes = std::fs::read(&stl).expect("STL");
    let n = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + 50 * n);
    assert_eq!(mesh["result"]["triangles"], n);
    assert_eq!(mesh["result"]["bytes"], bytes.len());
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut volume6 = 0.;
    for t in bytes[84..].chunks_exact(50) {
        let mut p = [[0.; 3]; 3];
        for (i, v) in p.iter_mut().enumerate() {
            for j in 0..3 {
                let k = 12 + 12 * i + 4 * j;
                v[j] = f64::from(f32::from_le_bytes(t[k..k + 4].try_into().expect("float")));
                lo[j] = lo[j].min(v[j]);
                hi[j] = hi[j].max(v[j]);
            }
        }
        let [a, b, c] = p;
        volume6 += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    assert_eq!(lo, [0., 0., 0.]);
    assert_eq!(hi, [80., 40., 10.]);
    assert!(
        (volume6.abs() / 6. - 20000.).abs() < 0.02,
        "1 ppm tolerance; got {}",
        volume6 / 6.
    );
    let fbx = f.root.path().join("edited.fbx");
    let r = reply(
        cli()
            .arg("export-fbx")
            .arg(&out)
            .arg("-o")
            .arg(&fbx)
            .arg("--json")
            .output()
            .expect("FBX"),
        "export-fbx",
        0,
    );
    assert_eq!(r["result"]["complete"], true);
    assert_eq!(r["result"]["omissions"], json!([]));
    for both in [false, true] {
        let p = f.root.path().join(format!("pipe-{both}.fcad"));
        let mut c = f.edit(&p);
        c.stdout(pipe::closed_pipe());
        if both {
            c.stderr(pipe::closed_pipe());
        } else {
            c.stderr(std::process::Stdio::piped());
        }
        let r = c.output().expect("closed pipes");
        assert_eq!(r.status.code(), Some(7), "{r:?}");
        check_copy(&f.source, &p, 6);
    }
    assert_eq!(std::fs::read(&f.source).expect("source"), before);
    assert_eq!(
        std::fs::metadata(&f.source)
            .expect("metadata")
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

fn check_copy(source: &Path, out: &Path, count: usize) {
    let a = Document::open_read_only(source).expect("source");
    let b = Document::open_read_only(out).expect("copy");
    let mut before_meta = a.meta().clone();
    before_meta.modified_at = b.meta().modified_at.clone();
    assert_eq!(&before_meta, b.meta());
    assert_eq!(
        a.dependencies().expect("deps"),
        b.dependencies().expect("deps")
    );
    assert_eq!(
        a.topology_refs().expect("refs"),
        b.topology_refs().expect("refs")
    );
    for old in a.objects().expect("objects") {
        let new = b.object(old.id).expect("row").expect("ID preserved");
        if let (ObjectPayload::Sketch(s), ObjectPayload::Sketch(t)) = (&old.payload, &new.payload) {
            assert_eq!(s.curves.len(), count);
            assert_eq!(
                s.curves.iter().map(|c| c.id).collect::<Vec<_>>(),
                t.curves.iter().map(|c| c.id).collect::<Vec<_>>()
            );
            assert_eq!(s.plane, t.plane);
            assert_eq!(s.constraints, t.constraints);
            assert_eq!(old.name, new.name);
            assert_eq!(old.ordinal, new.ordinal);
            assert_eq!(old.parent, new.parent);
            for (i, c) in t.curves.iter().enumerate() {
                let SketchGeometry::Line { end, .. } = c.geometry else {
                    panic!("line")
                };
                let SketchGeometry::Line { start, .. } = t.curves[(i + 1) % count].geometry else {
                    panic!("line")
                };
                assert_eq!(start, end);
            }
        } else {
            assert_eq!(old, new);
        }
    }
    a.close().expect("close");
    b.close().expect("close");

    let sql = rusqlite::Connection::open(out).expect("SQL");
    let extra: (i64, Vec<u8>) = sql
        .query_row("SELECT rowid,value FROM extension", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("extension");
    assert_eq!(extra, (73, vec![0, 255]));
    let attached: Vec<u8> = sql.query_row(
        "SELECT s.bytes FROM imported_source_refs r JOIN imported_sources s ON s.id=r.source_id \
         JOIN objects o ON o.id=r.object_id WHERE o.kind='sketch'", [], |r| r.get(0),
    ).expect("selected Sketch source claim must survive coordinate-only edit");
    assert_eq!(attached, b"preserve attached sketch source");
}
fn check_reference_domains(path: &Path) {
    let d = Document::open_read_only(path).expect("doc");
    let objects = d.objects().expect("objects");
    let sketch = objects
        .iter()
        .find_map(|o| {
            if let ObjectPayload::Sketch(s) = &o.payload {
                Some(s)
            } else {
                None
            }
        })
        .expect("sketch");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&d, &mut kernel, &context).expect("cold");
    let refs = d.topology_refs().expect("refs");
    assert_eq!(refs.len(), 3);
    for r in &refs {
        let faces = built.resolve(r).expect("resolve");
        assert_eq!(faces.len(), 1);
        let face = faces[0];
        let mesh = kernel
            .tessellate(face.shape(), &TessellationParams::default(), &context)
            .expect("mesh");
        let range = mesh
            .faces
            .iter()
            .find(|m| m.face == face)
            .expect("actual kernel face");
        let points: Vec<_> = mesh.indices
            [range.first_index as usize..(range.first_index + range.index_count) as usize]
            .iter()
            .map(|&i| &mesh.positions[3 * i as usize..3 * i as usize + 3])
            .collect();
        assert!(!points.is_empty());
        match r.output_role {
            SemanticRole::ExtrudeCap {
                side: CapSide::Start,
            } => assert!(points.iter().all(|p| p[2] == 0.)),
            SemanticRole::ExtrudeCap { side: CapSide::End } => {
                assert!(points.iter().all(|p| p[2] == 10.))
            }
            SemanticRole::ExtrudeSide { profile_segment } => {
                assert_eq!(profile_segment, sketch.curves[0].id);
                assert!(points.iter().all(|p| p[1] == 0.));
                assert!(points.iter().any(|p| p[0] == 80.));
                assert!(points.iter().any(|p| p[2] == 10.));
            }
            _ => panic!("unexpected role"),
        }
    }
    built.release_all(&mut kernel);
}
