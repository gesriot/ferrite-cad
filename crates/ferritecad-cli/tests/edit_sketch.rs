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
        let mut vertices = self.catalog["sketches"][0]["vertices"].clone();
        for v in vertices.as_array_mut().expect("vertices") {
            if v["start_mm"][0] == 60. {
                v["start_mm"][0] = json!(80.);
            }
        }
        write(
            &self.request,
            &json!({"request_version":1,"vertices":vertices}),
        );
    }
    fn edit(&self, output: &Path) -> Command {
        let mut c = cli();
        c.arg("edit-sketch-copy")
            .arg(&self.source)
            .arg("--sketch")
            .arg(
                self.catalog["sketches"][0]["sketch_id"]
                    .as_str()
                    .expect("id"),
            )
            .arg("--expect-version")
            .arg(self.catalog["content_version"].as_str().expect("version"))
            .arg("--request")
            .arg(&self.request)
            .arg("-o")
            .arg(output)
            .arg("--json");
        c
    }
}
fn native() -> bool {
    if ferritecad_occt::is_available() {
        true
    } else {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: no OCCT for saved Sketch geometry");
        false
    }
}

#[test]
fn sketch_discovery_and_protocol_without_kernel() {
    let f = Fixture::sample();
    let before = std::fs::read(&f.source).expect("bytes");
    let mtime = std::fs::metadata(&f.source)
        .expect("metadata")
        .modified()
        .expect("mtime");
    let names = entries(f.root.path());
    let d = Document::open_read_only(&f.source).expect("doc");
    let reading = ferritecad_document::ExtrudeEditSource::read(&d).expect("shared catalog");
    let row = &reading.sketches[0];
    assert!(row.refusal.is_none());
    assert_eq!(
        f.catalog["sketches"][0]["sketch_id"],
        row.sketch.to_string()
    );
    assert_eq!(
        f.catalog["content_version"],
        reading.version.content.to_string()
    );
    assert_eq!(
        f.catalog["sketches"][0]["vertices"]
            .as_array()
            .expect("vertices")
            .len(),
        4
    );
    for (wire, v) in f.catalog["sketches"][0]["vertices"]
        .as_array()
        .expect("vertices")
        .iter()
        .zip(row.vertices.as_ref().expect("vertices"))
    {
        assert_eq!(wire["curve_id"], v.curve_id.to_string());
        assert_eq!(wire["start_mm"], json!(v.start_mm));
    }
    d.close().expect("close");
    inspect(&f.source);
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    assert_eq!(
        std::fs::metadata(&f.source)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        mtime
    );
    assert_eq!(entries(f.root.path()), names);
    let output = f.root.path().join("out.fcad");
    // Execution refusal before a kernel is needed; stderr may be closed.
    write(&f.request, &json!({"request_version":2,"vertices":[]}));
    let v = reply(
        f.edit(&output)
            .stderr(pipe::closed_pipe())
            .output()
            .expect("process"),
        "edit-sketch-copy",
        2,
    );
    assert_eq!(v["error"]["kind"], "unsupported");
    assert_eq!(
        f.edit(&output)
            .stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
    f.write_edit();
    if !ferritecad_occt::is_available() {
        let v = reply(
            f.edit(&output).output().expect("stub refusal"),
            "edit-sketch-copy",
            2,
        );
        assert_eq!(v["error"]["kind"], "unsupported");
    }
    for args in [
        vec!["edit-sketch-copy", "--help"],
        vec!["edit-sketch-copy", "--json"],
    ] {
        let o = cli().args(&args).output().expect("usage");
        assert_eq!(
            o.status.code(),
            Some(if args[1] == "--help" { 0 } else { 2 })
        );
        assert!(serde_json::from_slice::<Value>(&o.stdout).is_err());
    }
    assert!(!output.exists());
    // Stored strings are not filenames: quotes and LF are valid on Windows too.
    let mut d = Document::open(&f.source).expect("write names");
    let mut o = d.object(row.sketch).expect("row").expect("sketch");
    o.name = Some("имя \"Sketch\"\nline".into());
    d.write(|w| w.put_object(o.id, o.parent, o.ordinal, o.name.as_deref(), &o.payload))
        .expect("name");
    d.close().expect("close");
    let v = inspect(&f.source);
    assert_eq!(v["sketches"][0]["name"], o.name.expect("name"));
    let raw = rusqlite::Connection::open(&f.source).expect("SQL");
    raw.execute_batch("CREATE TRIGGER no_edit AFTER UPDATE ON objects BEGIN SELECT 1; END;")
        .expect("trigger");
    drop(raw);
    let v = inspect(&f.source);
    assert_eq!(v["sketches"][0]["editable"], false);
    assert!(v["sketches"][0]["refusal"].is_null());
    assert!(
        v["sketches"][0]["document_refusal"]
            .as_str()
            .expect("global")
            .contains("no_edit")
    );
    let empty = f.root.path().join("empty.fcad");
    reply(
        cli()
            .arg("create")
            .arg(&empty)
            .arg("--json")
            .output()
            .expect("empty"),
        "create",
        0,
    );
    assert_eq!(inspect(&empty)["sketches"], json!([]));
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
    raw.execute_batch("CREATE TABLE extension(value BLOB); INSERT INTO extension(rowid,value) VALUES(73,X'00FF'); UPDATE capabilities SET rowid=rowid+100; INSERT INTO capabilities(rowid,name,required) VALUES(999,'future.optional',0);").expect("extra data");
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
    let v = reply(f.edit(&out).output().expect("edit"), "edit-sketch-copy", 0);
    assert_eq!(v["result"]["destination"], out.to_str().expect("path"));
    assert_eq!(v["result"]["document_id"], f.catalog["document_id"]);
    assert_eq!(
        v["result"]["sketch_id"],
        f.catalog["sketches"][0]["sketch_id"]
    );
    check_copy(&f.source, &out, 6);
    let after = inspect(&out);
    assert_ne!(after["content_version"], f.catalog["content_version"]);
    assert_eq!(after["features"], f.catalog["features"]);
    assert_eq!(after["bodies"], f.catalog["bodies"]);
    assert_eq!(
        after["sketches"][0]["vertices"][1]["start_mm"],
        json!([80., 0.])
    );
    assert_eq!(
        after["sketches"][0]["vertices"][2]["start_mm"],
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
    assert_eq!(a.meta(), b.meta());
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
    for path in [source, out] {
        let db =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .expect("capabilities");
        let rows: Vec<(i64, String, i64)> = db
            .prepare("SELECT rowid,name,required FROM capabilities ORDER BY rowid")
            .expect("query")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .expect("rows")
            .collect::<rusqlite::Result<_>>()
            .expect("values");
        assert_eq!(
            rows,
            vec![
                (101, "core.part.v1".into(), 1),
                (999, "future.optional".into(), 0)
            ],
            "capability rows and identities must survive"
        );
    }
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

#[test]
fn native_sketch_edit_refusals_preserve_source_and_destinations() {
    if !native() {
        return;
    }
    for case in [
        "missing_id",
        "duplicate",
        "foreign",
        "order",
        "crossed",
        "winding",
        "stale",
        "busy",
        "source",
        "hardlink",
        "unsupported",
        "reordered_objects",
    ] {
        let f = Fixture::sample();
        let out = f.root.path().join("refused.fcad");
        let original: Value =
            serde_json::from_slice(&std::fs::read(&f.request).expect("request")).expect("JSON");
        let mut input = original.clone();
        match case {
            "missing_id" => {
                input["vertices"].as_array_mut().expect("vertices").pop();
            }
            "duplicate" => {
                input["vertices"][1]["curve_id"] = input["vertices"][0]["curve_id"].clone()
            }
            "foreign" => {
                input["vertices"][1]["curve_id"] =
                    json!(ferritecad_types::StableEntityId::new().to_string())
            }
            "order" => input["vertices"]
                .as_array_mut()
                .expect("vertices")
                .swap(1, 2),
            "crossed" => {
                input["vertices"][1]["start_mm"] = json!([80., 40.]);
                input["vertices"][2]["start_mm"] = json!([80., 0.]);
            }
            "winding" => {
                for v in input["vertices"].as_array_mut().expect("vertices") {
                    v["start_mm"][0] = json!(-v["start_mm"][0].as_f64().expect("number"));
                }
            }
            "stale" | "reordered_objects" => {
                let sql = rusqlite::Connection::open(&f.source).expect("SQL");
                sql.execute(
                    "UPDATE objects SET ordinal=ordinal+10,name='renamed' WHERE kind='sketch'",
                    [],
                )
                .expect("change");
            }
            "busy" => std::fs::write(&out, b"sentinel").expect("occupied"),
            "hardlink" => std::fs::hard_link(&f.source, &out).expect("alias"),
            "unsupported" => {
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
                s.curves[0].construction = true;
                d.write(|w| w.put_object(o.id, o.parent, o.ordinal, o.name.as_deref(), &o.payload))
                    .expect("construction");
                d.close().expect("close");
            }
            _ => {}
        }
        write(&f.request, &input);
        let before = std::fs::read(&f.source).expect("bytes");
        let names = entries(f.root.path());
        let destination = if case == "source" { &f.source } else { &out };
        let mut c = f.edit(destination);
        // Refresh version where the specific property under test is eligibility,
        // not staleness. Changed order/name alone is intentionally still editable.
        if matches!(case, "unsupported" | "reordered_objects") {
            let fresh = inspect(&f.source);
            let renewed = Fixture {
                root: f.root,
                source: f.source,
                request: f.request,
                catalog: fresh,
            };
            let r = reply(
                renewed.edit(&out).output().expect("edit"),
                "edit-sketch-copy",
                if case == "reordered_objects" { 0 } else { 2 },
            );
            if case == "unsupported" {
                assert_eq!(r["error"]["kind"], "unsupported");
                assert_eq!(entries(renewed.root.path()), names);
            } else {
                assert_eq!(
                    inspect(&out)["sketches"][0]["sketch_id"],
                    renewed.catalog["sketches"][0]["sketch_id"]
                );
            }
            assert_eq!(std::fs::read(&renewed.source).expect("source"), before);
            continue;
        }
        let r = reply(c.output().expect("refusal"), "edit-sketch-copy", 2);
        assert_eq!(r["error"]["kind"], "input", "{case}: {r}");
        if case == "stale" {
            assert!(
                r["error"]["message"]
                    .as_str()
                    .expect("message")
                    .contains("source has changed")
            );
        }
        assert_eq!(std::fs::read(&f.source).expect("source"), before, "{case}");
        assert_eq!(entries(f.root.path()), names, "{case}");
        if case == "busy" {
            assert_eq!(std::fs::read(&out).expect("out"), b"sentinel");
        }
    }
    #[cfg(unix)]
    {
        let f = Fixture::sample();
        let out = f.root.path().join("symlink.fcad");
        std::os::unix::fs::symlink(&f.source, &out).expect("symlink");
        let before = std::fs::read(&f.source).expect("bytes");
        reply(f.edit(&out).output().expect("alias"), "edit-sketch-copy", 2);
        assert_eq!(std::fs::read(&f.source).expect("source"), before);
    }
}

#[test]
fn sketch_snapshot_and_unsupported_catalog_without_kernel() {
    let f = Fixture::sample();
    let pinned = Document::open_read_only(&f.source).expect("pinned");
    let before = ferritecad_document::ExtrudeEditSource::read(&pinned).expect("catalog");
    let conn = rusqlite::Connection::open(&f.source).expect("writer");
    conn.execute_batch(
        "BEGIN IMMEDIATE; UPDATE objects SET name='uncommitted name' WHERE kind='sketch';",
    )
    .expect("uncommitted writer");
    let same = ferritecad_document::ExtrudeEditSource::read(&pinned).expect("same snapshot");
    assert_eq!(same, before);
    let copy = f.root.path().join("snapshot.fcad");
    pinned.snapshot_to(&copy).expect("backup");
    pinned.close().expect("close");
    conn.execute_batch("COMMIT").expect("commit");
    drop(conn);
    assert_ne!(
        inspect(&f.source)["content_version"],
        f.catalog["content_version"]
    );
    assert_eq!(inspect(&copy), f.catalog);
    let mut d = Document::open(&copy).expect("doc");
    d.write(|w| {
        w.put_object(
            ferritecad_types::ObjectId::new(),
            None,
            30,
            Some("second Body"),
            &ObjectPayload::Body(ferritecad_document::Body { tip_feature: None }),
        )
    })
    .expect("second body");
    d.close().expect("close");
    let c = inspect(&copy);
    assert_eq!(c["sketches"][0]["editable"], false);
    assert!(c["sketches"][0]["vertices"].is_null());
    assert!(
        c["sketches"][0]["refusal"]
            .as_str()
            .expect("refusal")
            .contains("exactly one")
    );
}

#[test]
fn sketch_edit_os_paths_and_flag_shaped_values() {
    let f = Fixture::sample();
    let out = f.root.path().join("out.fcad");
    #[cfg(unix)]
    let invalid = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(b"non-utf8-\xff".to_vec())
    };
    #[cfg(windows)]
    let invalid = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xD800])
    };
    #[cfg(any(unix, windows))]
    {
        // Each path is independently rejected before file or native work.
        for slot in ["source", "output", "request"] {
            let source = if slot == "source" {
                Path::new(&invalid)
            } else {
                &f.source
            };
            let dest = if slot == "output" {
                Path::new(&invalid)
            } else {
                &out
            };
            let request = if slot == "request" {
                Path::new(&invalid)
            } else {
                &f.request
            };
            let o = cli()
                .arg("edit-sketch-copy")
                .arg(source)
                .arg("--sketch")
                .arg(f.catalog["sketches"][0]["sketch_id"].as_str().expect("id"))
                .arg("--expect-version")
                .arg(f.catalog["content_version"].as_str().expect("version"))
                .arg("--request")
                .arg(request)
                .arg("-o")
                .arg(dest)
                .arg("--json")
                .output()
                .expect("invalid path");
            let r = reply(o, "edit-sketch-copy", 2);
            assert_eq!(r["error"]["kind"], "input");
            assert!(
                r["error"]["message"]
                    .as_str()
                    .expect("message")
                    .contains("UTF-8")
            );
        }
    }
    // -- terminates options. A source named --json does not request JSON output.
    std::fs::write(f.root.path().join("--json"), b"not SQLite").expect("flag file");
    let p = cli()
        .current_dir(f.root.path())
        .args([
            "edit-sketch-copy",
            "--sketch",
            f.catalog["sketches"][0]["sketch_id"].as_str().expect("id"),
            "--expect-version",
            f.catalog["content_version"].as_str().expect("version"),
            "--request",
        ])
        .arg(&f.request)
        .arg("-o")
        .arg(&out)
        .args(["--", "--json"])
        .output()
        .expect("flag file");
    assert_eq!(p.status.code(), Some(2));
    assert!(p.stdout.is_empty());
    assert!(!out.exists());
}
