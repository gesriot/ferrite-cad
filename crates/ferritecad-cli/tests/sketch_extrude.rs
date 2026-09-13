// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::{Document, ObjectPayload, SketchGeometry};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn request(path: &Path) {
    std::fs::write(path,serde_json::to_vec(&json!({"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10})).expect("JSON")).expect("request");
}
fn create(input: &Path, out: &Path) -> Command {
    let mut c = cli();
    c.arg("create-sketch-extrude")
        .arg(input)
        .arg("-o")
        .arg(out)
        .arg("--json");
    c
}
fn reply(out: Output, code: i32) -> Value {
    assert_eq!(out.status.code(), Some(code), "{out:?}");
    assert_eq!(out.stdout.iter().filter(|&&b| b == b'\n').count(), 1);
    let v: Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["operation"], "create-sketch-extrude");
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
#[test]
fn sketch_request_refusals_and_usage_preserve_files() {
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("profile.json");
    let out = d.path().join("out.fcad");
    request(&input);
    let missing = d.path().join("missing.json");
    let v = reply(create(&missing, &out).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "io");
    for value in [
        json!({"request_version":2,"points_mm":[[0,0],[1,0],[0,1]],"height_mm":1}),
        json!({"request_version":1,"points_mm":[[0,0],[2,2],[0,2],[2,0]],"height_mm":10}),
        json!({"request_version":1,"points_mm":[[0,0],[1,0],[0,1]],"height_mm":0}),
        json!({"request_version":1,"points_mm":[[0,0],[1,0],[0,1]],"height_mm":1,"holes":[]}),
    ] {
        std::fs::write(&input, serde_json::to_vec(&value).expect("JSON")).expect("request");
        let before = std::fs::read(&input).expect("input");
        let names = entries(d.path());
        reply(create(&input, &out).output().expect("process"), 2);
        assert_eq!(std::fs::read(&input).expect("input"), before);
        assert_eq!(entries(d.path()), names);
    }
    request(&input);
    std::fs::write(&out, b"occupied").expect("occupied");
    let v = reply(create(&input, &out).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "input");
    assert_eq!(std::fs::read(&out).expect("output"), b"occupied");
    assert!(
        cli()
            .args(["create-sketch-extrude", "--help"])
            .output()
            .expect("help")
            .status
            .success()
    );
    let usage = cli()
        .args(["create-sketch-extrude", "--json"])
        .output()
        .expect("usage");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    std::fs::write(d.path().join("--json"), "bad request").expect("flag file");
    let text = cli()
        .current_dir(d.path())
        .args(["create-sketch-extrude", "-o", "new.fcad", "--", "--json"])
        .output()
        .expect("text");
    assert_eq!(text.status.code(), Some(2));
    assert!(text.stdout.is_empty());
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
fn native_sketch_extrude_process_cold_geometry_and_delivery() {
    if !ferritecad_occt::is_available() {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: no OCCT for polygon geometry");
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("контур space.json");
    let out = d.path().join("модель space.fcad");
    request(&input);
    let v = reply(create(&input, &out).output().expect("process"), 0);
    assert_eq!(v["result"]["destination"], out.to_str().expect("UTF-8"));
    let saved = std::fs::read(&out).expect("source");
    let doc = Document::open_read_only(&out).expect("reopen");
    assert_eq!(
        v["result"]["document_id"],
        doc.meta().document_id.to_string()
    );
    let objects = doc.objects().expect("objects");
    assert_eq!(objects.len(), 4);
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
    assert_eq!(sketch.curves.len(), 6);
    assert!(sketch.constraints.is_empty());
    for (c, p) in sketch.curves.iter().zip([
        [0., 0.],
        [60., 0.],
        [60., 20.],
        [20., 20.],
        [20., 40.],
        [0., 40.],
    ]) {
        let SketchGeometry::Line { start, .. } = c.geometry else {
            panic!("Line")
        };
        assert_eq!([start.x, start.y], p);
    }
    let ids: Vec<_> = sketch.curves.iter().map(|c| c.id).collect();
    let refs = doc.topology_refs().expect("refs");
    assert_eq!(refs.len(), 3);
    doc.close().expect("close");
    std::fs::remove_file(input).expect("private request no longer needed");
    for _ in 0..2 {
        let r = cli()
            .arg("rebuild")
            .arg(&out)
            .arg("--cold")
            .output()
            .expect("rebuild");
        assert!(r.status.success(), "{r:?}");
        let doc = Document::open_read_only(&out).expect("reopen");
        let objects = doc.objects().expect("objects");
        let s = objects
            .iter()
            .find_map(|o| {
                if let ObjectPayload::Sketch(s) = &o.payload {
                    Some(s)
                } else {
                    None
                }
            })
            .expect("sketch");
        assert_eq!(s.curves.iter().map(|c| c.id).collect::<Vec<_>>(), ids);
        assert_eq!(doc.topology_refs().expect("refs"), refs);
        doc.close().expect("close");
    }
    let stl = d.path().join("result.stl");
    let r = cli()
        .arg("export-stl")
        .arg(&out)
        .arg("-o")
        .arg(&stl)
        .arg("--json")
        .output()
        .expect("STL");
    assert!(r.status.success(), "{r:?}");
    let b = std::fs::read(&stl).expect("STL");
    let count = u32::from_le_bytes(b[80..84].try_into().expect("count")) as usize;
    assert_eq!(b.len(), 84 + 50 * count);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut six_volume = 0.;
    for t in b[84..].chunks_exact(50) {
        let mut p = [[0.; 3]; 3];
        for (i, point) in p.iter_mut().enumerate() {
            for j in 0..3 {
                let k = 12 + 12 * i + 4 * j;
                point[j] = f64::from(f32::from_le_bytes(
                    t[k..k + 4].try_into().expect("coordinate"),
                ));
                lo[j] = lo[j].min(point[j]);
                hi[j] = hi[j].max(point[j]);
            }
        }
        let [a, b, c] = p;
        six_volume += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    assert_eq!(lo, [0., 0., 0.]);
    assert_eq!(hi, [60., 40., 10.]);
    assert!(
        (six_volume.abs() / 6. - 16000.).abs() < 0.016,
        "{six_volume}"
    );
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
    assert_eq!(std::fs::read(&out).expect("source"), saved);
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
        let d = Document::open_read_only(published).expect("publication survived");
        assert_eq!(d.objects().expect("objects").len(), 4);
        d.close().expect("close");
    }
    let mut expected: Vec<std::ffi::OsString> = [
        "модель space.fcad",
        "result.stl",
        "result.fbx",
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

#[cfg(unix)]
#[test]
fn sketch_json_non_utf8_path_refuses_before_reading() {
    use std::os::unix::ffi::OsStringExt;
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join(std::ffi::OsString::from_vec(vec![b'p', 255]));
    let out = d.path().join("out.fcad");
    let v = reply(create(&input, &out).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "input");
    assert!(entries(d.path()).is_empty());
}

#[test]
fn sketch_alias_and_stub_kernel_refusals_preserve_storage() {
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
        let output = d.path().join("new.fcad");
        let names = entries(d.path());
        let v = reply(create(&input, &output).output().expect("stub process"), 2);
        assert_eq!(v["error"]["kind"], "unsupported");
        assert_eq!(entries(d.path()), names);
        let v = reply(
            create(&input, &output)
                .stderr(pipe::closed_pipe())
                .output()
                .expect("closed stderr"),
            2,
        );
        assert_eq!(v["error"]["kind"], "unsupported");
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

#[cfg(windows)]
#[test]
fn sketch_json_non_utf16_argument_refuses_before_reading() {
    use std::os::windows::ffi::OsStringExt;
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join(std::ffi::OsString::from_wide(&[0xd800]));
    let output = d.path().join("out.fcad");
    let v = reply(create(&input, &output).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "input");
    assert!(entries(d.path()).is_empty());
}
