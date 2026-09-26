// SPDX-License-Identifier: MIT
//! §27B: the saved profile of a §27A Revolve, edited through the existing
//! `edit-sketch-copy` into a new copy and measured after the fact.
//!
//! Every expected volume here is written out from closed-form geometry —
//! annuli and frusta — and never taken from the production Pappus helper,
//! so a wrong policy and a wrong measurement cannot agree by construction.
use super::*;
use ferritecad_document::{
    SketchConstraint, SketchConstraintRule, SketchPointRef, SketchPointSelector, SketchVertex,
};
use ferritecad_kernel::{CancelToken, ProgressSink};
use std::collections::BTreeSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const EDIT: &str = "edit-sketch-copy";

/// π·(R² − r²)·h: a straight bushing.
pub(super) fn annulus(outer: f64, inner: f64, height: f64) -> f64 {
    PI * (outer * outer - inner * inner) * height
}
/// π·h·(R² + R·r + r²)/3: a solid frustum.
pub(super) fn frustum(r1: f64, r2: f64, height: f64) -> f64 {
    PI * height * (r1 * r1 + r1 * r2 + r2 * r2) / 3.
}

/// One edit: the profile created, the profile it becomes, both volumes.
struct Case {
    name: &'static str,
    from: Vec<[f64; 2]>,
    to: Vec<[f64; 2]>,
    from_mm3: f64,
    to_mm3: f64,
}

fn cases() -> Vec<Case> {
    let bushing = BUSHING.to_vec();
    let stepped = STEPPED.to_vec();
    let sloped = SLOPED.to_vec();
    let base = vec![
        Case {
            name: "bushing-smaller",
            from: bushing.clone(),
            to: vec![[3., 2.5], [8., 2.5], [8., 12.5], [3., 12.5]],
            from_mm3: annulus(10., 4., 15.),
            to_mm3: annulus(8., 3., 10.),
        },
        // The outer wall's upper end moves in: the same Line, the same saved
        // name, now a cone instead of a cylinder.
        Case {
            name: "bushing-to-cone",
            from: bushing,
            to: vec![[4., 0.], [10., 0.], [7., 15.], [4., 15.]],
            from_mm3: annulus(10., 4., 15.),
            to_mm3: frustum(10., 7., 15.) - annulus(4., 0., 15.),
        },
        Case {
            name: "stepped",
            from: stepped,
            to: vec![
                [5., 1.],
                [12., 1.],
                [12., 4.],
                [8., 4.],
                [8., 14.],
                [5., 14.],
            ],
            from_mm3: annulus(10., 4., 5.) + annulus(7., 4., 10.),
            to_mm3: annulus(12., 5., 3.) + annulus(8., 5., 10.),
        },
        // A fractional translation along the axis as well as new radii.
        Case {
            name: "sloped",
            from: sloped,
            to: vec![[3., -1.25], [9., -1.25], [6., 13.75], [3., 13.75]],
            from_mm3: frustum(10., 7., 15.) - annulus(4., 0., 15.),
            to_mm3: frustum(9., 6., 15.) - annulus(3., 0., 15.),
        },
    ];
    // The same edits drawn the other way round: saved order and winding are
    // the document's, and the request follows them.
    let reversed: Vec<Case> = base
        .iter()
        .map(|c| Case {
            name: Box::leak(format!("{}-cw", c.name).into_boxed_str()),
            from: c.from.iter().rev().copied().collect(),
            to: c.to.iter().rev().copied().collect(),
            from_mm3: c.from_mm3,
            to_mm3: c.to_mm3,
        })
        .collect();
    base.into_iter().chain(reversed).collect()
}

pub(super) fn edit(
    source: &Path,
    sketch: &str,
    version: &str,
    request: &Path,
    out: &Path,
) -> Command {
    let mut c = cli();
    c.arg(EDIT)
        .arg(source)
        .arg("--sketch")
        .arg(sketch)
        .arg("--expect-version")
        .arg(version)
        .arg("--request")
        .arg(request)
        .arg("-o")
        .arg(out)
        .arg("--json");
    c
}
pub(super) fn edit_reply(out: Output, code: i32) -> Value {
    assert_eq!(out.status.code(), Some(code), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["operation"], EDIT);
    assert_eq!(v["ok"], code == 0, "{v}");
    v
}

/// What discovery says about the one Sketch: its ID, the version to pin and
/// the saved vertices in order.
pub(super) struct Saved {
    pub(super) sketch: String,
    pub(super) version: String,
    pub(super) ids: Vec<String>,
}
pub(super) fn saved(path: &Path) -> Saved {
    let c = inspect(path);
    let s = &c["sketches"][0];
    Saved {
        sketch: s["sketch_id"].as_str().expect("id").to_owned(),
        version: c["content_version"].as_str().expect("version").to_owned(),
        ids: s["vertices"]
            .as_array()
            .expect("editable vertices")
            .iter()
            .map(|v| v["curve_id"].as_str().expect("curve").to_owned())
            .collect(),
    }
}
pub(super) fn write_edit(path: &Path, ids: &[String], points: &[[f64; 2]]) {
    let vertices: Vec<Value> = ids
        .iter()
        .zip(points)
        .map(|(id, p)| json!({"curve_id": id, "start_mm": p}))
        .collect();
    write(path, &json!({"request_version":1,"vertices":vertices}));
}

/// Every cell of every table, keyed by table name, rows in a stable order.
pub(super) fn cells(path: &Path) -> BTreeMap<String, Vec<Vec<String>>> {
    let c = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("SQL");
    let tables: Vec<String> = c
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .expect("tables")
        .query_map([], |r| r.get(0))
        .expect("rows")
        .collect::<rusqlite::Result<_>>()
        .expect("names");
    let mut all = BTreeMap::new();
    for table in tables {
        let mut s = c
            .prepare(&format!("SELECT * FROM \"{table}\""))
            .expect("select");
        let n = s.column_count();
        let names: Vec<String> = s.column_names().iter().map(|n| (*n).to_owned()).collect();
        let mut rows: Vec<Vec<String>> = s
            .query_map([], |r| {
                (0..n)
                    .map(|i| {
                        let v: rusqlite::types::Value = r.get(i)?;
                        Ok(format!("{}={v:?}", names[i]))
                    })
                    .collect()
            })
            .expect("rows")
            .collect::<rusqlite::Result<_>>()
            .expect("values");
        rows.sort();
        all.insert(table, rows);
    }
    all
}

/// The allowlist: only the selected Sketch row's `payload` and
/// `payload_hash` may differ, and nothing may be added or removed.
pub(super) fn check_cells(source: &Path, copy: &Path, sketch: &str) {
    let sketch = sketch.parse::<ObjectId>().expect("UUID");
    let id = format!("id=Blob({:?})", sketch.to_bytes().to_vec());
    let (a, b) = (cells(source), cells(copy));
    assert_eq!(
        a.keys().collect::<Vec<_>>(),
        b.keys().collect::<Vec<_>>(),
        "tables"
    );
    for (table, old) in &a {
        let new = &b[table];
        assert_eq!(old.len(), new.len(), "rows of {table}");
        if table != "objects" {
            assert_eq!(old, new, "{table} changed");
            continue;
        }
        let strip = |rows: &Vec<Vec<String>>| -> Vec<Vec<String>> {
            rows.iter()
                .map(|r| {
                    if r.contains(&id) {
                        r.iter()
                            .filter(|c| {
                                !c.starts_with("payload=") && !c.starts_with("payload_hash=")
                            })
                            .cloned()
                            .collect()
                    } else {
                        r.clone()
                    }
                })
                .collect()
        };
        let (mut x, mut y) = (strip(old), strip(new));
        x.sort();
        y.sort();
        assert_eq!(x, y, "only the Sketch payload may change");
        let changed: Vec<_> = old.iter().filter(|r| !new.contains(r)).collect();
        assert_eq!(changed.len(), 1, "exactly the Sketch row changes");
        assert!(changed[0].contains(&id));
    }
}

/// The Lines of a profile, in order, as `(start, end)`.
pub(super) fn lines_of(points: &[[f64; 2]]) -> Vec<([f64; 2], [f64; 2])> {
    (0..points.len())
        .map(|i| (points[i], points[(i + 1) % points.len()]))
        .collect()
}

#[test]
fn native_revolve_profile_edits_keep_names_measure_cache_sql_and_exports() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let mut artifact = 0;
    for case in cases() {
        let input = d.path().join(format!("{}.json", case.name));
        let source = d.path().join(format!("{}.fcad", case.name));
        request(&input, &case.from);
        reply(create(&input, &source).output().expect("create"), 0);
        let before = inspect(&source);
        let s = &before["sketches"][0];
        assert_eq!(s["editable"], true, "{s}");
        assert_eq!(s["profile_feature"]["kind"], "full_turn_revolve");
        assert_eq!(
            s["profile_feature"]["feature_id"],
            before["revolves"][0]["feature_id"]
        );
        assert_eq!(
            s["profile_feature"]["body_id"],
            before["revolves"][0]["body_id"]
        );
        assert_eq!(before["features"], json!([]), "never an Extrude");
        let volume = measure(&source, &case.from, None);
        assert!((volume - case.from_mm3).abs() < 1e-6 * case.from_mm3);

        let saved = saved(&source);
        let request_path = d.path().join(format!("{}-edit.json", case.name));
        write_edit(&request_path, &saved.ids, &case.to);
        let bytes = std::fs::read(&source).expect("source");
        // Warm the source's own sidecar first: its entries must not answer
        // for the edited profile.
        measure(&source, &case.from, Some(&[CacheOutcome::Miss]));
        let out = d.path().join(format!("{} edited.fcad", case.name));
        let v = edit_reply(
            edit(&source, &saved.sketch, &saved.version, &request_path, &out)
                .output()
                .expect("edit"),
            0,
        );
        assert_eq!(v["result"]["destination"], out.to_str().expect("UTF-8"));
        assert_eq!(v["result"]["document_id"], before["document_id"]);
        assert_eq!(v["result"]["sketch_id"], saved.sketch.as_str());
        assert_eq!(std::fs::read(&source).expect("source"), bytes);
        check_cells(&source, &out, &saved.sketch);

        let after = inspect(&out);
        assert_ne!(after["content_version"], before["content_version"]);
        assert_eq!(after["features"], json!([]));
        assert_eq!(after["bodies"], before["bodies"]);
        let row = &after["sketches"][0];
        assert_eq!(row["profile_feature"], s["profile_feature"]);
        assert_eq!(
            row["vertices"]
                .as_array()
                .expect("vertices")
                .iter()
                .map(|v| (
                    v["curve_id"].as_str().expect("id").to_owned(),
                    [
                        v["start_mm"][0].as_f64().expect("x"),
                        v["start_mm"][1].as_f64().expect("y")
                    ]
                ))
                .collect::<Vec<_>>(),
            saved
                .ids
                .iter()
                .cloned()
                .zip(case.to.clone())
                .collect::<Vec<_>>()
        );
        let mut revolve_before = before["revolves"][0].clone();
        let mut revolve_after = after["revolves"][0].clone();
        revolve_before["profile"]
            .as_object_mut()
            .expect("profile")
            .remove("segments");
        revolve_after["profile"]
            .as_object_mut()
            .expect("profile")
            .remove("segments");
        assert_eq!(revolve_before, revolve_after, "the Revolve is untouched");

        // B-Rep: every saved name, under its old UUID, is the face its Line
        // raises now. `measure` checks each face's kind, axis and extent
        // against the Line's current coordinates.
        let cold = measure(&out, &case.to, None);
        assert!(
            (cold - case.to_mm3).abs() < 1e-6 * case.to_mm3,
            "{}: {cold} != {}",
            case.name,
            case.to_mm3
        );
        if case.name.starts_with("bushing-to-cone") {
            let (from, to) = (lines_of(&case.from), lines_of(&case.to));
            let changed: Vec<_> = (0..from.len())
                .filter(|&i| {
                    expected_surface(from[i].0, from[i].1) != expected_surface(to[i].0, to[i].1)
                })
                .collect();
            let [i] = changed.as_slice() else {
                panic!("one Line changes its surface: {changed:?}")
            };
            assert_eq!(expected_surface(from[*i].0, from[*i].1), "cylinder");
            assert_eq!(expected_surface(to[*i].0, to[*i].1), "cone");
        }
        // The source's warmed sidecar, placed beside the copy, misses: the
        // key follows the coordinates, so no old geometry is returned.
        std::fs::copy(
            source.with_extension("fcad-cache"),
            out.with_extension("fcad-cache"),
        )
        .expect("stale sidecar");
        let missed = measure(&out, &case.to, Some(&[CacheOutcome::Miss]));
        let hit = measure(&out, &case.to, Some(&[CacheOutcome::Hit]));
        assert_eq!(missed, cold);
        assert_eq!(hit, cold);

        let listed = cli()
            .arg("print-topology")
            .arg(&out)
            .output()
            .expect("print-topology");
        let listed = String::from_utf8(listed.stdout).expect("UTF-8");
        for id in &saved.ids {
            assert!(listed.contains(&format!("revolve face from segment {id}")));
        }
        let n = case.to.len();
        assert!(listed.contains(&format!("{n} of {n} references resolved")));

        // An independent reading of the mesh, against the analytic volume.
        let m = mesh(&out, &out.with_extension("stl"));
        check_mesh(&m, &case.to);
        let band: f64 = lines_of(&case.to)
            .iter()
            .map(|(a, b)| 2. * PI * a[0].max(b[0]) * LINEAR_MM * (b[1] - a[1]).abs())
            .sum();
        assert!((m.volume - case.to_mm3).abs() <= band);
        if !case.name.ends_with("-cw") && artifact < 2 && case.name != "bushing-smaller" {
            if let Some(dir) = std::env::var_os("FCAD_REVOLVE_EDIT_ARTIFACTS") {
                let dir = Path::new(&dir);
                std::fs::create_dir_all(dir).expect("artifacts");
                let fbx = dir.join(format!("revolve-edit-{artifact}.fbx"));
                let r = cli()
                    .arg("export-fbx")
                    .arg(&out)
                    .arg("-o")
                    .arg(&fbx)
                    .arg("--json")
                    .output()
                    .expect("FBX");
                assert!(r.status.success(), "{r:?}");
            }
            artifact += 1;
        }
    }
    assert_eq!(artifact, 2);

    // A second edit of an edited copy is the same operation again.
    let first = d.path().join("bushing-smaller edited.fcad");
    let saved = saved(&first);
    let request_path = d.path().join("again.json");
    let again: Vec<[f64; 2]> = vec![[2.5, 0.], [6., 0.], [6., 20.], [2.5, 20.]];
    write_edit(&request_path, &saved.ids, &again);
    let second = d.path().join("again.fcad");
    edit_reply(
        edit(
            &first,
            &saved.sketch,
            &saved.version,
            &request_path,
            &second,
        )
        .output()
        .expect("edit"),
        0,
    );
    let volume = measure(&second, &again, None);
    assert!((volume - annulus(6., 2.5, 20.)).abs() < 1e-6 * volume);

    // An extrusion's Sketch states its own feature, and its old fields keep
    // their types and values.
    let l = d.path().join("l.json");
    write(
        &l,
        &json!({"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10}),
    );
    let plate = d.path().join("l.fcad");
    let r = cli()
        .arg("create-sketch-extrude")
        .arg(&l)
        .arg("-o")
        .arg(&plate)
        .arg("--json")
        .output()
        .expect("create L");
    assert!(r.status.success(), "{r:?}");
    let c = inspect(&plate);
    let row = &c["sketches"][0];
    assert_eq!(row["editable"], true);
    assert_eq!(
        row["profile_feature"],
        json!({"kind":"blind_extrude","feature_id":c["features"][0]["feature_id"],"height_mm":10.0})
    );
    assert_eq!(c["revolves"], json!([]));
}

#[test]
fn occt_without_solver_edits_a_revolve_profile() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("request.json");
    let source = d.path().join("sloped.fcad");
    request(&input, &SLOPED);
    reply(create(&input, &source).output().expect("create"), 0);
    let saved = saved(&source);
    let to = [[3., -1.25], [9., -1.25], [6., 13.75], [3., 13.75]];
    let request_path = d.path().join("edit.json");
    write_edit(&request_path, &saved.ids, &to);
    let out = d.path().join("edited.fcad");
    edit_reply(
        edit(&source, &saved.sketch, &saved.version, &request_path, &out)
            .output()
            .expect("edit"),
        0,
    );
    let expected = frustum(9., 6., 15.) - annulus(3., 0., 15.);
    let volume = measure(&out, &to, None);
    assert!((volume - expected).abs() < 1e-6 * expected);
}

/// Unsupported neighbours of the §27A class, written without a kernel.
fn unsupported_variants(root: &Path) -> Vec<(&'static str, PathBuf, &'static str)> {
    let mut all = Vec::new();
    let constrained = root.join("constrained.fcad");
    let (_, labels) = write_revolve_document(&constrained, &BUSHING);
    rewrite_sketch(&constrained, |sketch| {
        sketch.constraints.push(SketchConstraint {
            id: StableEntityId::new(),
            rule: SketchConstraintRule::Horizontal {
                a: SketchPointRef::new(labels[0], SketchPointSelector::Start),
                b: SketchPointRef::new(labels[0], SketchPointSelector::End),
            },
        });
    });
    all.push(("constraints", constrained, "unconstrained"));
    let moved = root.join("transformed.fcad");
    write_revolve_document(&moved, &BUSHING);
    {
        let mut d = Document::open(&moved).expect("open");
        let plane = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::DatumPlane(_)))
            .expect("plane");
        d.write(|w| {
            w.put_object(
                plane.id,
                None,
                plane.ordinal,
                plane.name.as_deref(),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::from_translation(
                        ferritecad_types::Vec3::new(0., 0., 5.).expect("vector"),
                    )
                    .expect("transform"),
                }),
            )
        })
        .expect("moved plane");
        d.close().expect("close");
    }
    all.push(("transform", moved, "untransformed XY plane"));
    let extra = root.join("extra.fcad");
    write_revolve_document(&extra, &BUSHING);
    {
        let mut d = Document::open(&extra).expect("open");
        d.write(|w| {
            w.put_object(
                ObjectId::new(),
                None,
                9,
                Some("Spare"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            )
        })
        .expect("extra object");
        d.close().expect("close");
    }
    all.push(("extra history", extra, "exactly one XY plane"));
    all
}
pub(super) fn rewrite_sketch(path: &Path, change: impl FnOnce(&mut Sketch)) {
    let mut d = Document::open(path).expect("open");
    let o = d
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("sketch");
    let ObjectPayload::Sketch(mut sketch) = o.payload.clone() else {
        unreachable!()
    };
    change(&mut sketch);
    d.write(|w| {
        w.put_object(
            o.id,
            o.parent,
            o.ordinal,
            o.name.as_deref(),
            &ObjectPayload::Sketch(sketch),
        )
    })
    .expect("sketch");
    d.close().expect("close");
}

/// Discovery, protocol refusals and the writer's own re-derivation, with no
/// kernel at all. Runs unchanged in the stub build.
#[test]
fn revolve_profile_discovery_protocol_and_writer_without_kernel() {
    let d = tempfile::tempdir().expect("dir");
    let source = d.path().join("bushing.fcad");
    let (revolve, labels) = write_revolve_document(&source, &BUSHING);
    let c = inspect(&source);
    let row = &c["sketches"][0];
    assert_eq!(row["editable"], true, "{row}");
    assert_eq!(row["refusal"], Value::Null);
    assert_eq!(row["document_refusal"], Value::Null);
    assert_eq!(
        row["vertices"],
        json!(
            labels
                .iter()
                .zip(BUSHING)
                .map(|(id, p)| json!({"curve_id": id.to_string(), "start_mm": [p[0], p[1]]}))
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(row["profile_feature"]["kind"], "full_turn_revolve");
    assert_eq!(row["profile_feature"]["feature_id"], revolve.to_string());
    assert_eq!(row["profile_feature"]["axis"], "sketch_y");
    assert_eq!(row["profile_feature"]["extent"], "full_turn");
    assert_eq!(
        row["profile_feature"]["axis_clearance_mm"],
        FullTurnRevolution::AXIS_CLEARANCE_MM
    );
    // The Revolve keeps its own discovery and meaning; the circle editors
    // refuse. §27G deliberately replaces the old refusal of the constraint
    // editor: a profile with a bore now names its Revolve there too.
    assert_eq!(c["features"], json!([]));
    assert_eq!(c["revolves"][0]["profile"]["available"], true);
    assert_eq!(row["constraint_edit"]["available"], true);
    assert_eq!(
        row["constraint_edit"]["profile_feature"],
        row["profile_feature"]
    );
    assert_eq!(row["circle_edit"]["available"], false);
    assert_eq!(row["annulus_edit"]["available"], false);
    assert_eq!(row["cut_history_v3"], Value::Null);
    assert_eq!(c["edit_extrude"]["available"], false);

    // Structures outside the class: discovery refuses and the command
    // publishes nothing. Without a kernel the command stops at the kernel
    // first; the structural refusal itself is checked natively.
    for (why, path, refusal) in unsupported_variants(d.path()) {
        let c = inspect(&path);
        let row = &c["sketches"][0];
        assert_eq!(row["editable"], false, "{why}");
        assert_eq!(row["vertices"], Value::Null, "{why}");
        assert_eq!(row["profile_feature"], Value::Null, "{why}");
        assert!(
            row["refusal"].as_str().expect("refusal").contains(refusal),
            "{why}: {row}"
        );
    }

    // Request protocol refusals come before any kernel or file work.
    let ids: Vec<String> = labels.iter().map(ToString::to_string).collect();
    let sketch = row_id(&c);
    let version = c["content_version"].as_str().expect("version").to_owned();
    let request_path = d.path().join("request.json");
    let out = d.path().join("never.fcad");
    let good: Vec<Value> = ids
        .iter()
        .zip(BUSHING)
        .map(|(id, p)| json!({"curve_id": id, "start_mm": p}))
        .collect();
    for (why, body) in [
        ("not JSON", b"{".to_vec()),
        (
            "unknown field",
            serde_json::to_vec(&json!({"request_version":1,"vertices":good,"axis":"sketch_y"}))
                .expect("JSON"),
        ),
        (
            "later version",
            serde_json::to_vec(&json!({"request_version":2,"vertices":good})).expect("JSON"),
        ),
        (
            "null",
            serde_json::to_vec(&json!({"request_version":1,"vertices":null})).expect("JSON"),
        ),
        (
            "text coordinate",
            serde_json::to_vec(
                &json!({"request_version":1,"vertices":[{"curve_id":ids[0],"start_mm":["4",0]}]}),
            )
            .expect("JSON"),
        ),
        ("oversize", vec![b' '; 65537]),
    ] {
        std::fs::write(&request_path, &body).expect("request");
        let names = entries(d.path());
        let bytes = std::fs::read(&source).expect("source");
        edit_reply(
            edit(&source, &sketch, &version, &request_path, &out)
                .output()
                .expect("edit"),
            2,
        );
        assert!(!out.exists(), "{why}");
        assert_eq!(entries(d.path()), names, "{why}");
        assert_eq!(std::fs::read(&source).expect("source"), bytes, "{why}");
    }
    // UTF-8 policy: a JSON command refuses a non-UTF-8 path before anything.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        write_edit(&request_path, &ids, &BUSHING);
        let odd = d
            .path()
            .join(std::ffi::OsStr::from_bytes(b"not-utf8-\xff.fcad"));
        let v = edit_reply(
            edit(&source, &sketch, &version, &request_path, &odd)
                .output()
                .expect("edit"),
            2,
        );
        assert!(
            v["error"]["message"]
                .as_str()
                .expect("message")
                .contains("UTF-8")
        );
        assert!(!odd.exists());
    }

    // The writer re-derives the edit inside its own transaction: prepared
    // payloads that were never produced by the rules are refused, and the
    // document is left exactly as it was.
    let target = d.path().join("writer.fcad");
    std::fs::copy(&source, &target).expect("copy");
    let bytes = std::fs::read(&target).expect("target");
    let sketch_id = sketch.parse::<ObjectId>().expect("UUID");
    let vertices = |points: &[[f64; 2]]| -> Vec<SketchVertex> {
        labels
            .iter()
            .zip(points)
            .map(|(id, p)| SketchVertex {
                curve_id: *id,
                start_mm: *p,
            })
            .collect()
    };
    let doc = Document::open_read_only(&target).expect("open");
    let prepared = ferritecad_document::replace_sketch_coordinates(
        &doc,
        sketch_id,
        &vertices(&[[3., 2.5], [8., 2.5], [8., 12.5], [3., 12.5]]),
    )
    .expect("a valid edit prepares");
    // Preparation refuses what the policy refuses, by name.
    for (why, points, message) in [
        (
            // §27C: a whole Line on the axis would make this bored part
            // solid, which an edit may not do.
            "axis contact",
            vec![[0., 0.], [10., 0.], [10., 15.], [0., 15.]],
            "cannot change between hollow and solid",
        ),
        (
            "axis crossing",
            vec![[-1., 0.], [10., 0.], [10., 15.], [-1., 15.]],
            "positive radial side",
        ),
        (
            "self-intersection",
            vec![[4., 0.], [10., 15.], [10., 0.], [4., 15.]],
            "",
        ),
        (
            "degenerate",
            vec![[4., 0.], [7., 0.], [10., 0.], [4., 0.]],
            "",
        ),
        (
            "winding",
            vec![[4., 15.], [10., 15.], [10., 0.], [4., 0.]],
            "winding",
        ),
    ] {
        let error =
            ferritecad_document::replace_sketch_coordinates(&doc, sketch_id, &vertices(&points))
                .expect_err(why);
        assert!(error.to_string().contains(message), "{why}: {error}");
    }
    let mut ids_wrong = vertices(&BUSHING);
    ids_wrong.swap(1, 2);
    let mut missing = vertices(&BUSHING);
    missing.pop();
    let mut duplicate = vertices(&BUSHING);
    duplicate[1].curve_id = duplicate[0].curve_id;
    let mut foreign = vertices(&BUSHING);
    foreign[3].curve_id = StableEntityId::new();
    for (why, request) in [
        ("reordered", ids_wrong),
        ("missing", missing),
        ("duplicate", duplicate),
        ("foreign", foreign),
    ] {
        let error = ferritecad_document::replace_sketch_coordinates(&doc, sketch_id, &request)
            .expect_err(why);
        assert!(
            error
                .to_string()
                .contains("every saved curve UUID exactly once"),
            "{why}: {error}"
        );
    }
    doc.close().expect("close");
    let forged = |change: &dyn Fn(&mut Sketch)| {
        let mut record = prepared.clone();
        let ObjectPayload::Sketch(sketch) = &mut record.payload else {
            unreachable!()
        };
        change(sketch);
        record
    };
    let line = |a: [f64; 2], b: [f64; 2]| SketchGeometry::Line {
        start: Point2::new(a[0], a[1]).expect("point"),
        end: Point2::new(b[0], b[1]).expect("point"),
    };
    for (why, record) in [
        (
            "a consistent profile on the axis",
            forged(&|s: &mut Sketch| {
                let p = [[0., 2.5], [8., 2.5], [8., 12.5], [0., 12.5]];
                for (i, c) in s.curves.iter_mut().enumerate() {
                    c.geometry = line(p[i], p[(i + 1) % 4]);
                }
            }),
        ),
        (
            "an end moved without its start",
            forged(&|s: &mut Sketch| s.curves[0].geometry = line([3., 2.5], [9., 2.5])),
        ),
        (
            "a constraint slipped in",
            forged(&|s: &mut Sketch| {
                s.constraints.push(SketchConstraint {
                    id: StableEntityId::new(),
                    rule: SketchConstraintRule::Horizontal {
                        a: SketchPointRef::new(labels[0], SketchPointSelector::Start),
                        b: SketchPointRef::new(labels[0], SketchPointSelector::End),
                    },
                })
            }),
        ),
        (
            "a construction Line",
            forged(&|s: &mut Sketch| s.curves[2].construction = true),
        ),
    ] {
        let mut doc = Document::open(&target).expect("open");
        doc.write_sketch_geometry(&record).expect_err(why);
        doc.close().expect("close");
        assert_eq!(std::fs::read(&target).expect("target"), bytes, "{why}");
    }
    // And the honest one is accepted by the same writer.
    let mut doc = Document::open(&target).expect("open");
    doc.write_sketch_geometry(&prepared).expect("prepared edit");
    doc.close().expect("close");
}

fn row_id(catalog: &Value) -> String {
    catalog["sketches"][0]["sketch_id"]
        .as_str()
        .expect("id")
        .to_owned()
}

#[test]
fn native_revolve_profile_edit_refusals_preserve_every_file() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("bushing.json");
    let source = d.path().join("bushing.fcad");
    request(&input, &BUSHING);
    reply(create(&input, &source).output().expect("create"), 0);
    let s = saved(&source);
    let request_path = d.path().join("edit.json");
    let out = d.path().join("never.fcad");
    let refuse = |source: &Path, sketch: &str, version: &str, out: &Path, why: &str| {
        let names = entries(d.path());
        let bytes = std::fs::read(source).expect("source");
        let v = edit_reply(
            edit(source, sketch, version, &request_path, out)
                .output()
                .expect("edit"),
            2,
        );
        assert_eq!(std::fs::read(source).expect("source"), bytes, "{why}");
        assert_eq!(entries(d.path()), names, "{why}: nothing written");
        v["error"]["message"].as_str().expect("message").to_owned()
    };
    for (why, points, message) in [
        (
            // §27C: a whole Line on the axis would make this bored part
            // solid, which an edit may not do.
            "axis contact",
            vec![[0., 0.], [10., 0.], [10., 15.], [0., 15.]],
            "cannot change between hollow and solid",
        ),
        (
            "axis crossing",
            vec![[-2., 0.], [10., 0.], [10., 15.], [-2., 15.]],
            "positive radial side",
        ),
        (
            "within the clearance",
            vec![[1e-7, 0.], [10., 0.], [10., 15.], [1e-7, 15.]],
            "positive radial side",
        ),
        (
            "self-intersection",
            vec![[4., 0.], [10., 15.], [10., 0.], [4., 15.]],
            "",
        ),
        (
            "zero area",
            vec![[4., 0.], [6., 0.], [8., 0.], [10., 0.]],
            "",
        ),
        (
            "beyond the limit",
            vec![[4., 0.], [2e6, 0.], [10., 15.], [4., 15.]],
            "",
        ),
        (
            "winding",
            vec![[4., 15.], [10., 15.], [10., 0.], [4., 0.]],
            "winding",
        ),
    ] {
        write_edit(&request_path, &s.ids, &points);
        let refusal = refuse(&source, &s.sketch, &s.version, &out, why);
        assert!(refusal.contains(message), "{why}: {refusal}");
    }
    // Identities: foreign, missing, duplicate and reordered UUIDs.
    let mut reordered = s.ids.clone();
    reordered.swap(0, 1);
    let mut duplicate = s.ids.clone();
    duplicate[2] = duplicate[1].clone();
    let foreign: Vec<String> = s
        .ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            if i == 3 {
                StableEntityId::new().to_string()
            } else {
                id.clone()
            }
        })
        .collect();
    for (why, ids, points) in [
        ("reordered", reordered, BUSHING.to_vec()),
        ("duplicate", duplicate, BUSHING.to_vec()),
        ("foreign", foreign, BUSHING.to_vec()),
        ("missing", s.ids[..3].to_vec(), BUSHING[..3].to_vec()),
    ] {
        write_edit(&request_path, &ids, &points);
        let message = refuse(&source, &s.sketch, &s.version, &out, why);
        assert!(
            message.contains("every saved curve UUID exactly once"),
            "{why}: {message}"
        );
    }
    // A stale version, a Sketch that is not this one, and destinations that
    // are the source or already exist.
    write_edit(
        &request_path,
        &s.ids,
        &[[3., 2.5], [8., 2.5], [8., 12.5], [3., 12.5]],
    );
    let stale = ferritecad_types::ContentHash::of_bytes(b"another document").to_string();
    let message = refuse(&source, &s.sketch, &stale, &out, "stale");
    assert!(message.contains("source has changed"), "{message}");
    let message = refuse(
        &source,
        &ObjectId::new().to_string(),
        &s.version,
        &out,
        "unknown Sketch",
    );
    assert!(message.contains("does not exist"), "{message}");
    let taken = d.path().join("taken.fcad");
    std::fs::write(&taken, b"keep").expect("taken");
    let message = refuse(&source, &s.sketch, &s.version, &taken, "occupied");
    assert!(message.contains("already exists"), "{message}");
    assert_eq!(std::fs::read(&taken).expect("taken"), b"keep");
    let message = refuse(&source, &s.sketch, &s.version, &source, "the source itself");
    assert!(message.contains("different files"), "{message}");
    #[cfg(unix)]
    {
        let link = d.path().join("link.fcad");
        std::os::unix::fs::symlink(&source, &link).expect("symlink");
        let message = refuse(
            &source,
            &s.sketch,
            &s.version,
            &link,
            "symlink to the source",
        );
        assert!(
            message.contains("different files") || message.contains("already exists"),
            "{message}"
        );
        let hard = d.path().join("hard.fcad");
        std::fs::hard_link(&source, &hard).expect("hard link");
        let message = refuse(
            &source,
            &s.sketch,
            &s.version,
            &hard,
            "hard link to the source",
        );
        assert!(
            message.contains("different files") || message.contains("already exists"),
            "{message}"
        );
    }
    // Documents outside the class refuse at the job, by their structure.
    for (why, path, refusal) in unsupported_variants(d.path()) {
        let s = saved_any(&path);
        write_edit(&request_path, &s.1, &BUSHING);
        let message = refuse(&path, &s.0, &s.2, &out, why);
        assert!(message.contains(refusal), "{why}: {message}");
    }
}

/// Discovery of a Sketch whose row carries no vertices: its ID and version,
/// and the saved curve IDs read from the document itself.
fn saved_any(path: &Path) -> (String, Vec<String>, String) {
    let c = inspect(path);
    let d = Document::open_read_only(path).expect("open");
    let ids = d
        .objects()
        .expect("objects")
        .into_iter()
        .find_map(|o| match o.payload {
            ObjectPayload::Sketch(s) => Some(s.curves.iter().map(|c| c.id.to_string()).collect()),
            _ => None,
        })
        .expect("sketch");
    d.close().expect("close");
    (
        row_id(&c),
        ids,
        c["content_version"].as_str().expect("version").to_owned(),
    )
}

#[test]
fn native_revolve_profile_edit_delivery_late_guards_and_cancellation() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("stepped.json");
    let source = d.path().join("stepped.fcad");
    request(&input, &STEPPED);
    reply(create(&input, &source).output().expect("create"), 0);
    let s = saved(&source);
    let to = [
        [5., 1.],
        [12., 1.],
        [12., 4.],
        [8., 4.],
        [8., 14.],
        [5., 14.],
    ];
    let request_path = d.path().join("edit.json");
    write_edit(&request_path, &s.ids, &to);

    // Exit 7: the report is lost, the publication stands.
    for both in [false, true] {
        let published = d.path().join(format!("pipe-{both}.fcad"));
        let mut c = edit(&source, &s.sketch, &s.version, &request_path, &published);
        c.stdout(pipe::closed_pipe());
        if both {
            c.stderr(pipe::closed_pipe());
        } else {
            c.stderr(std::process::Stdio::piped());
        }
        let r = c.output().expect("closed stdout");
        assert_eq!(r.status.code(), Some(7), "{r:?}");
        let volume = measure(&published, &to, None);
        let expected = annulus(12., 5., 3.) + annulus(8., 5., 10.);
        assert!((volume - expected).abs() < 1e-6 * expected);
    }

    // The shared job, with the kernel that ships, at its own phases.
    let reading = ferritecad_jobs::read_extrude_source(&source).expect("reading");
    let choice = &reading.sketches[0];
    let vertices: Vec<SketchVertex> = choice
        .vertices
        .clone()
        .expect("editable")
        .into_iter()
        .zip(to)
        .map(|(v, p)| SketchVertex {
            curve_id: v.curve_id,
            start_mm: p,
        })
        .collect();
    let job = |name: &str| ferritecad_jobs::EditSketchRequest {
        source: source.clone(),
        expected: reading.version,
        sketch: choice.sketch,
        vertices: vertices.clone(),
        destination: d.path().join(name),
    };
    let bytes = std::fs::read(&source).expect("source");
    // Cancelled after the baseline check, before the write: nothing at all.
    let names = entries(d.path());
    let token = CancelToken::new();
    let stop = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ProgressSink::new(move |f| {
            if f >= 0.4 {
                stop.cancel();
            }
        }));
    let error = ferritecad_jobs::edit_sketch_copy(
        &job("cancelled.fcad"),
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect_err("cancelled");
    assert_eq!(error.kind(), ErrorKind::Cancellation, "{error}");
    assert_eq!(entries(d.path()), names, "no copy and no scratch");
    // The source replaced while the job ran: the late guard refuses it.
    let late = d.path().join("late.fcad");
    let path = source.clone();
    let done = Arc::new(AtomicBool::new(false));
    let once = done.clone();
    let context = OperationContext::default().with_progress(ProgressSink::new(move |f| {
        if f >= 0.95 && !once.swap(true, Ordering::SeqCst) {
            let mut d = Document::open(&path).expect("source");
            let plane = d
                .objects()
                .expect("objects")
                .into_iter()
                .find(|o| matches!(o.payload, ObjectPayload::DatumPlane(_)))
                .expect("plane");
            d.write(|w| {
                w.put_object(
                    plane.id,
                    None,
                    plane.ordinal,
                    Some("renamed while editing"),
                    &plane.payload,
                )
            })
            .expect("change");
            d.close().expect("close");
        }
    }));
    let error = ferritecad_jobs::edit_sketch_copy(
        &job("late.fcad"),
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect_err("stale at publication");
    assert!(done.load(Ordering::SeqCst), "the late phase ran");
    assert!(error.to_string().contains("source has changed"), "{error}");
    assert!(!late.exists());
    assert!(
        !entries(d.path())
            .iter()
            .any(|n| n.to_string_lossy().starts_with(".ferritecad-")),
        "no scratch left behind"
    );
    assert_ne!(std::fs::read(&source).expect("source"), bytes);
    // Cancellation that arrives after publication does not undo it.
    let reading = ferritecad_jobs::read_extrude_source(&source).expect("reading");
    let token = CancelToken::new();
    let stop = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ProgressSink::new(move |f| {
            if f >= 1.0 {
                stop.cancel();
            }
        }));
    let mut request = job("after.fcad");
    request.expected = reading.version;
    let edited = ferritecad_jobs::edit_sketch_copy(
        &request,
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect("published before the cancellation");
    assert_eq!(edited.sketch, choice.sketch);
    let volume = measure(&request.destination, &to, None);
    let expected = annulus(12., 5., 3.) + annulus(8., 5., 10.);
    assert!((volume - expected).abs() < 1e-6 * expected);
    let ids: BTreeSet<_> = s.ids.iter().collect();
    assert_eq!(
        saved(&request.destination)
            .ids
            .iter()
            .collect::<BTreeSet<_>>(),
        ids
    );
}
