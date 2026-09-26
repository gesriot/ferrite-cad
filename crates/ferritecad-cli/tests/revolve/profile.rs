// SPDX-License-Identifier: MIT
//! §27F: the saved profile of a §27D sector, edited by the existing
//! `edit-sketch-copy` into a new copy and measured after the fact.
//!
//! Expected volumes are this file's own Pappus × θ/360 of the edited profile,
//! never the production helper. The Revolve row — and so the angle — is
//! compared byte for byte with the source; both end faces are measured on
//! their planes under their saved UUIDs; the cache is given the archive of
//! the old profile and must not answer with it.
use super::angle::{
    CONE_SHIFTED, CYLINDER_SHIFTED, STEPPED_SHIFTED, angle_edit, angle_reply, default_stl, refs,
    revolve_payload, stub_sector, target, write_angle,
};
use super::edit::{cells, check_cells, edit, edit_reply, saved, write_edit};
use super::partial::{
    angle, axis_line, check_partial_mesh, expected_capabilities, measure_partial, pappus,
    payload_version, request_v2,
};
use super::*;
use ferritecad_document::SketchVertex;
use ferritecad_kernel::{CancelToken, ProgressSink};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Each saved profile with the edit applied to it: radial and axial sizes
/// change together, the class and the axis Line stay.
/// A profile's name, its saved Lines' starts and the edit's.
type Case = (&'static str, Vec<[f64; 2]>, Vec<[f64; 2]>);

fn profiles() -> Vec<Case> {
    vec![
        (
            "stepped",
            STEPPED_SHIFTED.to_vec(),
            vec![
                [3.75, 0.5],
                [11.25, 0.5],
                [11.25, 7.25],
                [8.125, 7.25],
                [8.125, 18.5],
                [3.75, 18.5],
            ],
        ),
        (
            "cylinder",
            CYLINDER_SHIFTED.to_vec(),
            vec![[0., -3.75], [7.625, -3.75], [7.625, 14.5], [0., 14.5]],
        ),
        (
            "cone",
            CONE_SHIFTED.to_vec(),
            vec![[0., -1.25], [11.5, -1.25], [0., 9.875]],
        ),
    ]
}

fn sketch_payload(path: &Path) -> Vec<u8> {
    let sql =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("SQL");
    sql.query_row("SELECT payload FROM objects WHERE kind='sketch'", [], |r| {
        r.get(0)
    })
    .expect("payload")
}

/// Every cell but `meta.modified_at`, which only the angle edit stamps.
fn cells_but_modified(path: &Path) -> BTreeMap<String, Vec<Vec<String>>> {
    let mut all = cells(path);
    for row in all.get_mut("meta").expect("meta") {
        row.retain(|c| !c.starts_with("modified_at="));
    }
    all
}

fn profile_edit(source: &Path, to: &[[f64; 2]], out: &Path, code: i32) -> Value {
    let s = saved(source);
    let request = out.with_extension("json");
    write_edit(&request, &s.ids, to);
    edit_reply(
        edit(source, &s.sketch, &s.version, &request, out)
            .output()
            .expect("edit"),
        code,
    )
}

fn export_both(path: &Path) -> (Vec<u8>, Vec<u8>) {
    let stl = default_stl(path, &path.with_extension("default.stl"));
    let fbx = path.with_extension("fbx");
    let r = cli()
        .arg("export-fbx")
        .arg(path)
        .arg("-o")
        .arg(&fbx)
        .arg("--json")
        .output()
        .expect("FBX");
    assert!(r.status.success(), "{r:?}");
    (stl, std::fs::read(&fbx).expect("FBX bytes"))
}

/// Three profiles, each saved at 137.5° and at 220° (both sides of a half
/// turn), edited A → B and back B → A. Every copy is measured cold, from the
/// archive of the old profile (a Miss) and from its own (a Hit), compared
/// cell by cell with its source, and read back as an independent STL.
#[test]
fn native_partial_profile_edits_measure_caps_names_cache_sql_and_exports() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let mut artifact = 0;
    for (name, a, b) in profiles() {
        let solid = axis_line(&a).is_some();
        assert_eq!(axis_line(&b), axis_line(&a), "the edit keeps the axis Line");
        let (full_a, full_b) = (pappus(&a), pappus(&b));
        for degrees in [137.5, 220.] {
            let input = d.path().join(format!("{name}-{degrees}.json"));
            let source = d.path().join(format!("{name}-{degrees}.fcad"));
            request_v2(&input, &a, angle(degrees));
            reply(create(&input, &source).output().expect("create"), 0);
            let kept = std::fs::read(&source).expect("source");
            let saved_refs = refs(&source);
            let revolve = revolve_payload(&source);
            // The old profile's archive, built before anything exports.
            measure_partial(&source, &a, degrees, full_a, Some(&[CacheOutcome::Miss]));
            measure_partial(&source, &a, degrees, full_a, Some(&[CacheOutcome::Hit]));
            let row = &inspect(&source)["sketches"][0];
            assert_eq!(row["editable"], true, "{row}");
            let sketch = row["sketch_id"].as_str().expect("id").to_owned();

            // A → B.
            let edited = d.path().join(format!("{name}-{degrees}-b.fcad"));
            let v = profile_edit(&source, &b, &edited, 0);
            assert_eq!(v["result"]["sketch_id"], sketch.as_str());
            assert_eq!(std::fs::read(&source).expect("source"), kept);
            check_cells(&source, &edited, &sketch);
            assert_eq!(
                revolve_payload(&edited),
                revolve,
                "the angle is not touched"
            );
            assert_eq!(refs(&edited), saved_refs, "every name, both caps");
            assert_eq!(payload_version(&edited), if solid { 4 } else { 3 });
            assert_eq!(capability_rows(&edited), expected_capabilities(solid));
            let c = inspect(&edited);
            let row = &c["sketches"][0];
            assert_eq!(row["editable"], true);
            assert_eq!(row["profile_feature"]["angle_deg"], json!(degrees));
            let starts: Vec<[f64; 2]> = row["vertices"]
                .as_array()
                .expect("vertices")
                .iter()
                .map(|v| serde_json::from_value(v["start_mm"].clone()).expect("point"))
                .collect();
            assert_eq!(starts, b);
            assert_eq!(c["revolves"][0]["angle_deg"], json!(degrees));
            // The old profile's archive under the new name: a Miss, never
            // the old sector; then this copy's own archive.
            std::fs::copy(
                source.with_extension("fcad-cache"),
                edited.with_extension("fcad-cache"),
            )
            .expect("cache copy");
            let cold = measure_partial(&edited, &b, degrees, full_b, None);
            assert_eq!(
                cold,
                measure_partial(&edited, &b, degrees, full_b, Some(&[CacheOutcome::Miss]))
            );
            assert_eq!(
                cold,
                measure_partial(&edited, &b, degrees, full_b, Some(&[CacheOutcome::Hit]))
            );
            let listed = cli()
                .arg("print-topology")
                .arg(&edited)
                .output()
                .expect("print-topology");
            let listed = String::from_utf8(listed.stdout).expect("UTF-8");
            let named = saved_refs.len();
            assert!(listed.contains(&format!("{named} of {named} references resolved")));
            assert!(listed.contains("revolve start cap") && listed.contains("revolve end cap"));
            let m = mesh(&edited, &edited.with_extension("stl"));
            check_partial_mesh(&m, &b, degrees, full_b);
            if degrees == 220. {
                if let Some(dir) = std::env::var_os("FCAD_REVOLVE_PROFILE_ARTIFACTS") {
                    let dir = Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("artifacts");
                    // Both at the default tessellation, the only one
                    // export-fbx has, so the CI join compares one mesh.
                    for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
                        let target = dir.join(format!("revolve-profile-{artifact}.{extension}"));
                        let r = cli()
                            .arg(op)
                            .arg(&edited)
                            .arg("-o")
                            .arg(&target)
                            .arg("--json")
                            .output()
                            .expect("artifact");
                        assert!(r.status.success(), "{r:?}");
                    }
                }
                artifact += 1;
            }

            // B → A: every cell is the source's again (a coordinate write
            // stamps nothing), and so is the mesh.
            let back = d.path().join(format!("{name}-{degrees}-back.fcad"));
            profile_edit(&edited, &a, &back, 0);
            assert_eq!(cells(&back), cells(&source), "A → B → A");
            assert_eq!(
                default_stl(&back, &back.with_extension("default.stl")),
                default_stl(&source, &source.with_extension("default.stl"))
            );
        }

        // One source, the same final intent reached two ways: the profile
        // then the angle, or the angle then the profile. Same names, same
        // stored Sketch and Revolve, same geometry, same export bytes.
        let source = d.path().join(format!("{name}-137.5.fcad"));
        let first_profile = d.path().join(format!("{name}-p.fcad"));
        profile_edit(&source, &b, &first_profile, 0);
        let (feature, version, _) = target(&first_profile);
        let angle_request = d.path().join(format!("{name}-220.json"));
        write_angle(&angle_request, 220.);
        let profile_then_angle = d.path().join(format!("{name}-p-a.fcad"));
        angle_reply(
            angle_edit(
                &first_profile,
                &feature,
                &version,
                &angle_request,
                &profile_then_angle,
            )
            .output()
            .expect("angle edit"),
            0,
        );
        let (_, version, _) = target(&source);
        let first_angle = d.path().join(format!("{name}-a.fcad"));
        angle_reply(
            angle_edit(&source, &feature, &version, &angle_request, &first_angle)
                .output()
                .expect("angle edit"),
            0,
        );
        let angle_then_profile = d.path().join(format!("{name}-a-p.fcad"));
        profile_edit(&first_angle, &b, &angle_then_profile, 0);
        assert_eq!(
            cells_but_modified(&profile_then_angle),
            cells_but_modified(&angle_then_profile),
            "the same stored model, whichever edit came first"
        );
        assert_eq!(
            sketch_payload(&profile_then_angle),
            sketch_payload(&angle_then_profile)
        );
        assert_eq!(refs(&profile_then_angle), refs(&source));
        measure_partial(&profile_then_angle, &b, 220., full_b, None);
        assert_eq!(
            export_both(&profile_then_angle),
            export_both(&angle_then_profile),
            "STL and FBX bytes"
        );
    }
    assert_eq!(artifact, 3);
}

/// Everything around the published file on this route, with the kernel
/// that ships: a lost report, a kernel that cannot rebuild, cancellation,
/// a publication race, a late change of the source and the writer's own gate
/// against a forged or stale Sketch.
#[test]
fn native_partial_profile_edit_delivery_late_guards_and_cancellation() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("cone.json");
    let source = d.path().join("cone.fcad");
    request_v2(&input, &CONE_SHIFTED, angle(220.));
    reply(create(&input, &source).output().expect("create"), 0);
    let (_, _, to) = profiles().remove(2);
    let full = pappus(&to);
    let s = saved(&source);
    let request_path = d.path().join("edit.json");
    write_edit(&request_path, &s.ids, &to);

    // Occupied, the source itself and a hard link to it: nothing written.
    let taken = d.path().join("taken.fcad");
    std::fs::write(&taken, b"keep").expect("occupied");
    let hard = d.path().join("hard.fcad");
    std::fs::hard_link(&source, &hard).expect("hard link");
    let bytes = std::fs::read(&source).expect("source");
    for dest in [&taken, &source, &hard] {
        let names = entries(d.path());
        edit_reply(
            edit(&source, &s.sketch, &s.version, &request_path, dest)
                .output()
                .expect("edit"),
            2,
        );
        assert_eq!(entries(d.path()), names);
    }
    std::fs::remove_file(&hard).expect("unlink");
    assert_eq!(std::fs::read(&taken).expect("taken"), b"keep");
    assert_eq!(std::fs::read(&source).expect("source"), bytes);

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
        measure_partial(&published, &to, 220., full, None);
    }

    let reading = ferritecad_jobs::read_extrude_source(&source).expect("reading");
    let choice = &reading.sketches[0];
    let vertices: Vec<SketchVertex> = choice
        .vertices
        .clone()
        .expect("editable")
        .into_iter()
        .zip(&to)
        .map(|(v, p)| SketchVertex {
            curve_id: v.curve_id,
            start_mm: *p,
        })
        .collect();
    let job = |name: &str| ferritecad_jobs::EditSketchRequest {
        source: source.clone(),
        expected: reading.version,
        sketch: choice.sketch,
        vertices: vertices.clone(),
        destination: d.path().join(name),
    };
    let names = entries(d.path());
    // A kernel that cannot build the saved part refuses at the baseline.
    let error = ferritecad_jobs::edit_sketch_copy(
        &job("mock.fcad"),
        &mut ferritecad_kernel::mock::MockKernel::new(),
        &OperationContext::default(),
    )
    .expect_err("no rebuild");
    assert!(
        error.to_string().contains("cannot turn a profile"),
        "{error}"
    );
    assert_eq!(entries(d.path()), names, "no copy and no scratch");
    // Cancelled after the baseline check, before the write: nothing at all.
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
    // Publication race: another writer takes the destination mid-job.
    let raced = d.path().join("raced.fcad");
    let path = raced.clone();
    let context = OperationContext::default().with_progress(ProgressSink::new(move |f| {
        if f >= 0.95 && !path.exists() {
            std::fs::write(&path, b"theirs").expect("race");
        }
    }));
    let error = ferritecad_jobs::edit_sketch_copy(
        &job("raced.fcad"),
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect_err("taken during the job");
    assert!(error.to_string().contains("exists"), "{error}");
    assert_eq!(std::fs::read(&raced).expect("theirs"), b"theirs");
    std::fs::remove_file(&raced).expect("clean up");
    assert_eq!(entries(d.path()), names, "no scratch left behind");

    // The writer's own gate, reached directly on a writable copy.
    let direct = d.path().join("direct.fcad");
    std::fs::copy(&source, &direct).expect("copy");
    let mut doc = Document::open(&direct).expect("open");
    let prepared = ferritecad_document::replace_sketch_coordinates(&doc, choice.sketch, &vertices)
        .expect("prepared");
    // Forged: an axis end moved off the axis inside the prepared payload.
    let mut forged = prepared.clone();
    let ObjectPayload::Sketch(sketch) = &mut forged.payload else {
        panic!("a Sketch")
    };
    let SketchGeometry::Line { start, .. } = &mut sketch.curves[0].geometry else {
        panic!("a Line")
    };
    *start = Point2::new(1.5, start.y).expect("point");
    let error = doc.write_sketch_geometry(&forged).expect_err("forged");
    assert!(
        error.to_string().contains("touches the axis alone"),
        "{error}"
    );
    // Forged: a construction flag the request never stated.
    let mut forged = prepared.clone();
    let ObjectPayload::Sketch(sketch) = &mut forged.payload else {
        panic!("a Sketch")
    };
    sketch.curves[1].construction = true;
    let error = doc.write_sketch_geometry(&forged).expect_err("forged flag");
    assert!(error.to_string().contains("coordinate"), "{error}");
    // Stale: the Sketch changed after preparation.
    let other: Vec<SketchVertex> = vertices
        .iter()
        .map(|v| SketchVertex {
            curve_id: v.curve_id,
            start_mm: [v.start_mm[0] * 0.5, v.start_mm[1]],
        })
        .collect();
    doc.write_sketch_coordinates(choice.sketch, &other)
        .expect("another edit first");
    let error = doc.write_sketch_geometry(&prepared).expect_err("stale");
    assert!(error.to_string().contains("changed after"), "{error}");
    doc.close().expect("close");

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
    // And a stale expected version is refused before anything is copied.
    let names = entries(d.path());
    edit_reply(
        edit(
            &source,
            &s.sketch,
            &s.version,
            &request_path,
            &d.path().join("stale.fcad"),
        )
        .output()
        .expect("edit"),
        2,
    );
    assert_eq!(entries(d.path()), names);
}

/// The request of the changed route, judged before any kernel in every build:
/// duplicate keys (escaped ones too) at every level, arrays where objects
/// belong, unknown fields — including an angle, which is not part of this
/// request — and discovery of a sector written without a kernel. Then what
/// only a native job decides, said per build.
#[test]
fn partial_profile_requests_refuse_duplicates_arrays_and_foreign_fields() {
    let d = tempfile::tempdir().expect("dir");
    let source = d.path().join("sector.fcad");
    stub_sector(&source, 137.5);
    let c = inspect(&source);
    let row = &c["sketches"][0];
    assert_eq!(row["editable"], true, "{row}");
    assert_eq!(row["profile_feature"]["kind"], "partial_turn_revolve");
    assert_eq!(row["profile_feature"]["angle_deg"], 137.5);
    assert_eq!(row["profile_feature"]["extent"], "partial_turn");
    assert_eq!(
        row["profile_feature"]["feature_id"],
        c["revolves"][0]["feature_id"]
    );
    let s = saved(&source);
    let bytes = std::fs::read(&source).expect("source");
    let request = d.path().join("request.json");
    let out = d.path().join("never.fcad");
    let refuse = |body: &[u8], kind: &str, why: &str| -> String {
        std::fs::write(&request, body).expect("request");
        let names = entries(d.path());
        let v = edit_reply(
            edit(&source, &s.sketch, &s.version, &request, &out)
                .output()
                .expect("edit"),
            2,
        );
        assert_eq!(v["error"]["kind"], kind, "{why}: {v}");
        assert_eq!(entries(d.path()), names, "{why}: nothing written");
        assert_eq!(std::fs::read(&source).expect("source"), bytes, "{why}");
        v["error"]["message"].as_str().expect("message").to_owned()
    };
    let vertex =
        |id: &str, x: f64, y: f64| format!(r#"{{"curve_id":"{id}","start_mm":[{x},{y}]}}"#);
    let points = [[3.5, 0.5], [10.5, 0.5], [10.5, 14.5], [3.5, 14.5]];
    let list: Vec<String> = s
        .ids
        .iter()
        .zip(points)
        .map(|(id, [x, y])| vertex(id, x, y))
        .collect();
    let list = list.join(",");
    let id0 = &s.ids[0];
    let rest: String = s.ids[1..]
        .iter()
        .zip(&points[1..])
        .map(|(id, [x, y])| format!(",{}", vertex(id, *x, *y)))
        .collect();
    for (body, why) in [
        (
            format!(r#"{{"request_version":2,"request_version":1,"vertices":[{list}]}}"#),
            "a duplicate version",
        ),
        (
            format!(r#"{{"request_version":1,"vertices":[{list}],"vertices":[{list}]}}"#),
            "duplicate vertices",
        ),
        (
            format!(
                r#"{{"request_version":1,"vertices":[{{"curve_id":"{id0}","curve_id":"{id0}","start_mm":[3.5,0.5]}}{rest}]}}"#
            ),
            "a duplicate curve_id",
        ),
        (
            format!(
                r#"{{"request_version":1,"vertices":[{{"curve_id":"{id0}","start_mm":[3.5,0.5],"start_mm":[9,9]}}{rest}]}}"#
            ),
            "an escaped duplicate start_mm",
        ),
        (
            format!(r#"{{"request_version":1,"request_version":1,"vertices":[{list}]}}"#),
            "an escaped duplicate version",
        ),
        (format!("[1,[{list}]]"), "an array for the request"),
        (
            format!(r#"{{"request_version":1,"vertices":[["{id0}",[3.5,0.5]]{rest}]}}"#),
            "an array for a vertex",
        ),
        (
            format!(r#"{{"request_version":1,"vertices":[{list}],"angle_deg":220}}"#),
            "an angle, which this request does not carry",
        ),
        (
            format!(
                r#"{{"request_version":1,"vertices":[{list}],"extent":{{"kind":"full_turn"}}}}"#
            ),
            "an extent",
        ),
        (
            format!(
                r#"{{"request_version":1,"vertices":[{{"curve_id":"{id0}","start_mm":[3.5,0.5],"z":1}}{rest}]}}"#
            ),
            "an unknown vertex field",
        ),
        (
            format!(
                r#"{{"request_version":1,"vertices":[{{"curve_id":"{id0}","start_mm":["3.5",0.5]}}{rest}]}}"#
            ),
            "a string coordinate",
        ),
        (
            format!(
                r#"{{"request_version":1,"vertices":[{{"curve_id":"{id0}","start_mm":[1e400,0.5]}}{rest}]}}"#
            ),
            "an overflowing coordinate",
        ),
        (r#"{"request_version":1}"#.to_owned(), "no vertices"),
        ("{".to_owned(), "malformed JSON"),
    ] {
        refuse(body.as_bytes(), "input", why);
    }
    let message = refuse(
        format!(r#"{{"request_version":2,"vertices":[{list}]}}"#).as_bytes(),
        "unsupported",
        "v2",
    );
    assert!(message.contains("expected 1"), "{message}");

    // What only the native job decides: UUIDs and the profile policy. A
    // build without a kernel refuses every well-formed request for that.
    let native = ferritecad_occt::is_available();
    let foreign: Vec<String> = s
        .ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            if i == 2 {
                StableEntityId::new().to_string()
            } else {
                id.clone()
            }
        })
        .collect();
    for (ids, to, wanted, why) in [
        (
            foreign,
            points.to_vec(),
            "every saved curve UUID exactly once",
            "a foreign UUID",
        ),
        (
            s.ids.clone(),
            vec![[-2., 0.5], [10.5, 0.5], [10.5, 14.5], [-2., 14.5]],
            "positive radial side",
            "across the axis",
        ),
        (
            s.ids.clone(),
            vec![[0., 0.5], [10.5, 0.5], [10.5, 14.5], [0., 14.5]],
            "cannot change between hollow and solid",
            "hollow to solid",
        ),
    ] {
        write_edit(&request, &ids, &to);
        let body = std::fs::read(&request).expect("request");
        if native {
            let text = refuse(&body, "input", why);
            assert!(text.contains(wanted), "{why}: {text}");
        } else {
            refuse(&body, "unsupported", why);
        }
    }
    if !native {
        write_edit(&request, &s.ids, &points);
        let body = std::fs::read(&request).expect("request");
        refuse(&body, "unsupported", "a valid edit without a kernel");
    }
}

#[test]
fn occt_without_solver_edits_a_partial_revolve_profile() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("stepped.json");
    let source = d.path().join("stepped.fcad");
    request_v2(&input, &STEPPED_SHIFTED, angle(220.));
    reply(create(&input, &source).output().expect("create"), 0);
    let (_, _, to) = profiles().remove(0);
    let out = d.path().join("edited.fcad");
    profile_edit(&source, &to, &out, 0);
    let sketch = saved(&source).sketch;
    check_cells(&source, &out, &sketch);
    assert_eq!(revolve_payload(&out), revolve_payload(&source));
    let full = pappus(&to);
    measure_partial(&out, &to, 220., full, None);
    let m = mesh(&out, &out.with_extension("stl"));
    check_partial_mesh(&m, &to, 220., full);
}
