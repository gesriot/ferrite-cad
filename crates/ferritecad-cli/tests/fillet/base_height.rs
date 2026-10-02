// SPDX-License-Identifier: MIT
//! §29B: the plate under the one saved Chamfer is made taller or shorter by the
//! existing `edit-extrude`, in a new copy. The Chamfer keeps its edge and
//! distance; the only SQL cell pair that moves is the base Extrude's payload and
//! hash. The B-Rep, the independently read mesh and the cache are measured at
//! the chosen corner, not only for a volume that another corner would share.
use super::*;
use crate::radius::stored_refs;

const OP: &str = "edit-extrude";
/// A distance that is far from every bound, so only the height varies.
const DIST: f64 = 0.75;

fn base_and_version(path: &Path) -> (String, String, Value) {
    let catalog = inspect(path);
    (
        catalog["features"][0]["feature_id"]
            .as_str()
            .expect("feature")
            .to_owned(),
        catalog["content_version"]
            .as_str()
            .expect("version")
            .to_owned(),
        catalog,
    )
}

fn raise(source: &Path, feature: &str, height: &str, version: &str, output: &Path) -> Command {
    let mut c = cli();
    c.arg(OP)
        .arg(source)
        .arg("--feature")
        .arg(feature)
        .arg("--distance-mm")
        .arg(height)
        .arg("--expect-version")
        .arg(version)
        .arg("-o")
        .arg(output)
        .arg("--json");
    c
}

/// One height edit of a chamfered copy into `name`, checked against everything
/// a copy must be, and the new copy's measured B-Rep and mesh.
fn heightened(
    f: &Fixture,
    copy: &Path,
    id: ObjectId,
    corner: [f64; 2],
    h: f64,
    name: &str,
) -> (PathBuf, Cham) {
    let (base, version, before_catalog) = base_and_version(copy);
    let before = std::fs::read(copy).expect("source bytes");
    let refs = stored_refs(copy);
    let out = f.root.path().join(format!("{name}.fcad"));
    let published = reply(
        raise(copy, &base, &h.to_string(), &version, &out)
            .output()
            .expect("process"),
        OP,
        0,
    );
    assert_eq!(published["result"]["feature_id"], base.as_str());
    assert_eq!(
        published["result"]["document_id"],
        before_catalog["document_id"]
    );
    assert_eq!(
        std::fs::read(copy).expect("bytes"),
        before,
        "source touched"
    );
    // The one SQL cell pair that moved, and nothing else.
    assert!(crate::height::only_the_height_changed(copy, &out, &base) >= 2);
    assert_eq!(stored_refs(&out), refs, "every name and its UUID kept");

    let after = inspect(&out);
    assert_eq!(after["features"][0]["distance_mm"], h);
    assert_eq!(
        after["features"][0]["chamfer_base"], before_catalog["features"][0]["chamfer_base"],
        "the Chamfer is the same one at the same edge and distance"
    );
    assert_eq!(after["features"][0]["chamfer_base"]["distance_mm"], DIST);
    assert_eq!(
        after["features"][0]["chamfer_base"]["corner_mm"],
        json!(corner)
    );
    assert_eq!(after["chamfers"][0]["feature_id"], json!(id.to_string()));
    assert_eq!(
        after["chamfers"][0]["edge"],
        before_catalog["chamfers"][0]["edge"]
    );
    assert_eq!(after["chamfers"][0]["distance_mm"], DIST);
    assert_eq!(after["bodies"][0]["chamfer_edge"]["available"], false);
    assert_eq!(
        after["bodies"][0]["body_id"],
        before_catalog["bodies"][0]["body_id"]
    );
    let checked = cli()
        .arg("validate")
        .arg(&out)
        .arg("--json")
        .output()
        .expect("validate");
    assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
    let n = stored_count(&out);
    let rebuilt = cli()
        .arg("rebuild")
        .arg(&out)
        .arg("--cold")
        .output()
        .expect("rebuild");
    let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
    assert!(
        text.contains(&format!("{n} of {n} stored references resolved")),
        "{text}"
    );

    // The B-Rep, through the document's own references: the same corner, the
    // same distance, the volume and the face the new height gives. The mesh is
    // read independently and holds the same part.
    let (cold, _) = measure_c(&out, None);
    check_brep(&cold, RECT, corner, DIST, h);
    let m = mesh(&out, &f.root.path().join(format!("{name}.stl")));
    check_mesh_c(&m, RECT, corner, DIST, h);
    (out, cold)
}

/// Every corner of the translated, fractional plate, drawn three ways (both
/// windings, other starting Lines), taller and shorter; the Chamfer keeps its
/// corner and distance, the cache follows the height, a later distance edit
/// still works.
#[test]
fn native_the_height_of_a_chamfered_plate_changes_at_every_corner_in_three_orders() {
    if !native() {
        return;
    }
    for (index, corners) in [CCW, CW_FROM_UPPER_RIGHT, CCW_FROM_THIRD]
        .into_iter()
        .enumerate()
    {
        let f = Fixture::drawn(Plate::new(corners));
        for (k, corner) in corners.into_iter().enumerate() {
            let tag = format!("h{index}-{k}");
            let (copy, id, was) = chamfer_at(&f, &f.source, corner, DIST, &format!("{tag}-src"));
            // Taller, for every corner.
            let (taller, cold_up) = heightened(&f, &copy, id, corner, 9.5, corner_name(&tag, "up"));
            assert!(cold_up.volume > was.volume, "taller is more material");
            assert_eq!(cold_up.faces, 7);
            if k != index {
                continue;
            }
            // Shorter, below the chamfer's own distance too, from the same source.
            let (shorter, cold_down) =
                heightened(&f, &copy, id, corner, 0.4, corner_name(&tag, "down"));
            assert!(cold_down.volume < was.volume);
            super::super::fbx(&taller, &format!("height-{index}-up"));
            super::super::fbx(&shorter, &format!("height-{index}-down"));

            // The cache: the old copy fills it, the new height misses for the
            // plate and for the Chamfer over it and never reads the old plate;
            // a second run reads what the first wrote and equals the cold one.
            let cache = f.root.path().join(format!("{tag}.cache"));
            let (first, events) = measure_c(&copy, Some(&cache));
            assert!(
                events.iter().all(|e| e.outcome == CacheOutcome::Miss),
                "{events:?}"
            );
            first.same_as(&was);
            let (fresh, events) = measure_c(&taller, Some(&cache));
            assert!(
                events.iter().any(|e| e.feature == id)
                    && events.iter().all(|e| e.outcome == CacheOutcome::Miss),
                "the new height must not hit the old plate: {events:?}"
            );
            assert!((fresh.volume - cold_up.volume).abs() < 1e-9 * fresh.volume);
            assert!((fresh.volume - was.volume).abs() > 1.0, "not the old plate");
            let (again, events) = measure_c(&taller, Some(&cache));
            assert!(
                events.iter().all(|e| e.outcome == CacheOutcome::Hit),
                "{events:?}"
            );
            again.same_as(&cold_up);

            // The Chamfer's own distance edit still works on the edited plate,
            // and its bound does not depend on the height.
            let (_, version, catalog) = base_and_version(&shorter);
            let max = catalog["chamfers"][0]["distance_edit"]["max_distance_mm"]
                .as_f64()
                .expect("max");
            assert_eq!(max, D - 0.01);
            let request = f.root.path().join(format!("{tag}-d.json"));
            write(&request, &json!({"request_version":1,"distance_mm":max}));
            let out = f.root.path().join(format!("{tag}-d.fcad"));
            reply(
                edit_from(&shorter, &id.to_string(), &version, &request, &out)
                    .output()
                    .expect("process"),
                OPE,
                0,
            );
            let (m, _) = measure_c(&out, None);
            check_brep(&m, RECT, corner, max, 0.4);
        }
    }
}

fn corner_name(tag: &str, which: &str) -> &'static str {
    // Leaked on purpose: test-only names that live as long as the process.
    Box::leak(format!("{tag}-{which}").into_boxed_str())
}

/// A height at the same corner, same distance and the same volume at another
/// corner is a different part: the measurement here finds the chosen corner.
#[test]
fn native_the_height_edit_keeps_the_chosen_corner_not_another_with_the_same_volume() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let (a, ia, _) = chamfer_at(&f, &f.source, [X0, Y0], 2.5, "ca");
    let (b, ib, _) = chamfer_at(&f, &f.source, [X0 + W, Y0 + D], 2.5, "cb");
    let (a_out, ma) = {
        let (base, version, _) = base_and_version(&a);
        let out = f.root.path().join("ca-up.fcad");
        reply(
            raise(&a, &base, "8", &version, &out)
                .output()
                .expect("process"),
            OP,
            0,
        );
        (out.clone(), measure_c(&out, None).0)
    };
    let (b_out, mb) = {
        let (base, version, _) = base_and_version(&b);
        let out = f.root.path().join("cb-up.fcad");
        reply(
            raise(&b, &base, "8", &version, &out)
                .output()
                .expect("process"),
            OP,
            0,
        );
        (out.clone(), measure_c(&out, None).0)
    };
    assert!(
        (ma.volume - mb.volume).abs() < 1e-9 * ma.volume,
        "same volume"
    );
    assert!(
        (ma.normal[0] - mb.normal[0]).abs() > 1.0,
        "opposite corners face opposite ways"
    );
    check_brep(&ma, RECT, [X0, Y0], 2.5, 8.0);
    check_brep(&mb, RECT, [X0 + W, Y0 + D], 2.5, 8.0);
    let ma_mesh = mesh(&a_out, &f.root.path().join("a8.stl"));
    let mb_mesh = mesh(&b_out, &f.root.path().join("b8.stl"));
    check_mesh_c(&ma_mesh, RECT, [X0, Y0], 2.5, 8.0);
    check_mesh_c(&mb_mesh, RECT, [X0 + W, Y0 + D], 2.5, 8.0);
    let swapped = std::panic::catch_unwind(|| {
        check_mesh_c(&ma_mesh, RECT, [X0 + W, Y0 + D], 2.5, 8.0);
    });
    assert!(
        swapped.is_err(),
        "the other corner's test must fail on this mesh"
    );
    // The Chamfers are the ones created, with their own UUIDs.
    assert_ne!(ia, ib);
    assert_eq!(
        inspect(&a_out)["chamfers"][0]["feature_id"],
        json!(ia.to_string())
    );
    assert_eq!(
        inspect(&b_out)["chamfers"][0]["feature_id"],
        json!(ib.to_string())
    );
}

/// Refusals, races and the guards of the shared copy job on this branch: each
/// leaves the directory and the source exactly as they were.
#[test]
fn native_height_refusals_races_and_guards_are_atomic_under_a_chamfer() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let (copy, id, _) = chamfer_at(&f, &f.source, [X0 + W, Y0], DIST, "src");
    let (base, version, catalog) = base_and_version(&copy);
    let before = std::fs::read(&copy).expect("bytes");
    let names = entries(f.root.path());
    let out = f.root.path().join("never.fcad");
    let sketch = catalog["sketches"][0]["sketch_id"]
        .as_str()
        .expect("sketch")
        .to_owned();
    let foreign = ObjectId::new().to_string();
    // The existing typed routes, with the guilty UUID.
    for (why, feature, height, kind, names_uuid) in [
        (
            "the Chamfer itself",
            id.to_string(),
            "9",
            "unsupported",
            Some(id.to_string()),
        ),
        (
            "the Sketch",
            sketch.clone(),
            "9",
            "unsupported",
            Some(sketch.clone()),
        ),
        ("a foreign UUID", foreign.clone(), "9", "input", None),
        ("zero", base.clone(), "0", "input", None),
        ("negative", base.clone(), "-2", "input", None),
        ("not a number", base.clone(), "NaN", "input", None),
        ("infinite", base.clone(), "inf", "input", None),
        // The kernel's own limit: measured at 1e-5 mm and below, and typed.
        (
            "a height OCCT cannot chamfer",
            base.clone(),
            "1e-6",
            "kernel",
            None,
        ),
    ] {
        let v = reply(
            raise(&copy, &feature, height, &version, &out)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert_eq!(refused(&v), kind, "{why}: {v}");
        if let Some(uuid) = names_uuid {
            assert!(v.to_string().contains(&uuid), "{why}: {v}");
        }
        assert!(!out.exists(), "{why}");
    }
    // A height just above that limit builds.
    reply(
        raise(&copy, &base, "1e-4", &version, &out)
            .output()
            .expect("process"),
        OP,
        0,
    );
    let (m, _) = measure_c(&out, None);
    check_brep(&m, RECT, [X0 + W, Y0], DIST, 1e-4);
    std::fs::remove_file(&out).expect("cleanup");

    // A stale version: another copy's content version is not this one's.
    let (_, stale_version, _) = base_and_version(&f.source);
    let v = reply(
        raise(&copy, &base, "9", &stale_version, &out)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    assert!(!out.exists());
    // No clobber, and the source is never its own destination.
    std::fs::write(&out, b"keep me").expect("existing");
    let v = reply(
        raise(&copy, &base, "9", &version, &out)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert!(matches!(refused(&v), "input" | "io"), "{v}");
    assert_eq!(std::fs::read(&out).expect("bytes"), b"keep me");
    std::fs::remove_file(&out).expect("cleanup");
    let v = reply(
        raise(&copy, &base, "9", &version, &copy)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert!(matches!(refused(&v), "input" | "io"), "{v}");
    // A refusal whose report cannot be written is still a refusal.
    assert_eq!(
        raise(&copy, &base, "0", &version, &out)
            .stdout(super::super::pipe::closed_pipe())
            .stderr(super::super::pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
    assert_eq!(std::fs::read(&copy).expect("bytes"), before);
    assert_eq!(entries(f.root.path()), names, "a refusal left something");
}

/// After a height edit every other editor still names the Chamfer, and the
/// evaluator still refuses a saved Chamfer outside its class.
#[test]
fn native_after_a_height_edit_every_other_editor_still_names_the_chamfer() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let (copy, id, _) = chamfer_at(&f, &f.source, [X0, Y0 + D], 2.0, "src");
    let (edited, _) = heightened_plain(&f, &copy, 7.25, "edited");
    let (base, version, catalog) = base_and_version(&edited);
    let sketch = catalog["sketches"][0]["sketch_id"]
        .as_str()
        .expect("sketch")
        .to_owned();
    assert_eq!(catalog["sketches"][0]["editable"], false);
    let out = f.root.path().join("never.fcad");
    // The plain plate's catalogue: a refused Sketch lists no vertices.
    let vertices: Vec<Value> = f.catalog["sketches"][0]["vertices"]
        .as_array()
        .expect("vertices")
        .iter()
        .map(|p| json!({"curve_id": p["curve_id"], "start_mm": p["start_mm"]}))
        .collect();
    let request = f.root.path().join("sketch.json");
    write(&request, &json!({"request_version":1,"vertices":vertices}));
    let v = reply(
        cli()
            .arg("edit-sketch-copy")
            .arg(&edited)
            .args(["--sketch", &sketch, "--expect-version", &version])
            .arg("--request")
            .arg(&request)
            .arg("-o")
            .arg(&out)
            .arg("--json")
            .output()
            .expect("process"),
        "edit-sketch-copy",
        2,
    );
    assert!(v.to_string().contains(&id.to_string()), "{v}");
    assert!(!out.exists());
    // A second height edit of the edited plate is the same operation again.
    let again = f.root.path().join("again.fcad");
    reply(
        raise(&edited, &base, "3.5", &version, &again)
            .output()
            .expect("process"),
        OP,
        0,
    );
    let (m, _) = measure_c(&again, None);
    check_brep(&m, RECT, [X0, Y0 + D], 2.0, 3.5);
}

fn heightened_plain(f: &Fixture, copy: &Path, h: f64, name: &str) -> (PathBuf, ()) {
    let (base, version, _) = base_and_version(copy);
    let out = f.root.path().join(format!("{name}.fcad"));
    reply(
        raise(copy, &base, &h.to_string(), &version, &out)
            .output()
            .expect("process"),
        OP,
        0,
    );
    (out, ())
}

/// Discovery and the protocol without a kernel: the base Extrude carries the
/// Chamfer as context, every other row is `null`, the request protocol is the
/// existing one, a Chamfer outside the class keeps the height refused with its
/// UUID — nothing here needs a kernel or a solver.
#[test]
fn chamfer_base_height_discovery_and_protocol_without_native() {
    // Both builds run this whole test; which answers to expect is decided here
    // without `native()`, whose skip line would read as a skipped gate.
    let kernel = ferritecad_occt::is_available();
    if !kernel {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
    }
    let plain = Fixture::drawn(Plate::new(CCW));
    assert_eq!(plain.catalog["features"][0]["chamfer_base"], Value::Null);
    let f = chamfered_without_kernel(Plate::new(CCW), [X0 + W, Y0 + D], 2.375);
    assert_eq!(f.catalog["edit_extrude"]["available"], true);
    let row = &f.catalog["features"][0];
    assert_eq!(row["editable"], true, "{row}");
    assert_eq!(row["refusal"], Value::Null);
    assert_eq!(row["distance_mm"], H);
    for key in [
        "base_height_edit",
        "base_height_edit_v2",
        "base_height_edit_v3",
        "fillet_base",
        "fillet_history",
    ] {
        assert_eq!(
            row[key],
            Value::Null,
            "{key}: a Chamfer is not that history"
        );
    }
    let chamfer = &f.catalog["chamfers"][0];
    assert_eq!(
        row["chamfer_base"],
        json!({
            "chamfer_feature_id": chamfer["feature_id"],
            "body_id": f.body_id(),
            "edge": chamfer["edge"],
            "corner_mm": [X0 + W, Y0 + D],
            "distance_mm": 2.375,
            "distance_unit": "mm",
        })
    );
    assert_eq!(
        f.catalog["sketches"][0]["editable"], false,
        "the Sketch still names the Chamfer"
    );
    let chamfer_id = chamfer["feature_id"].as_str().expect("id").to_owned();
    // The request protocol is the existing one; a build without a kernel asks
    // for it before any other check, so every well-formed request is
    // `unsupported` there, and with a kernel each is its own kind.
    let before = std::fs::read(&f.source).expect("source bytes");
    let names = entries(f.root.path());
    let never = f.root.path().join("never.fcad");
    let (base, version, _) = base_and_version(&f.source);
    let sketch = f.catalog["sketches"][0]["sketch_id"]
        .as_str()
        .expect("sketch")
        .to_owned();
    let foreign = ObjectId::new().to_string();
    for (why, feature, height, kind) in [
        ("zero", base.as_str(), "0", "input"),
        ("negative", base.as_str(), "-1", "input"),
        ("not a number", base.as_str(), "NaN", "input"),
        ("infinite", base.as_str(), "inf", "input"),
        ("the Chamfer", chamfer_id.as_str(), "9", "unsupported"),
        ("the Sketch", sketch.as_str(), "9", "unsupported"),
        ("a foreign UUID", foreign.as_str(), "9", "input"),
    ] {
        let v = reply(
            raise(&f.source, feature, height, &version, &never)
                .output()
                .expect("process"),
            OP,
            2,
        );
        if kernel {
            assert_eq!(refused(&v), kind, "{why}: {v}");
        } else {
            assert_eq!(refused(&v), "unsupported", "{why}: {v}");
            assert!(v.to_string().contains("Open CASCADE"), "{why}: {v}");
        }
    }
    assert_eq!(entries(f.root.path()), names, "a refusal left something");
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);

    // A Chamfer outside the class keeps the height refused, naming the
    // feature: an extra name, and a dimension.
    let odd = chamfered_without_kernel(Plate::new(CCW), [X0, Y0], 2.0);
    let odd_id: ObjectId = uuid(&odd.catalog["chamfers"][0]["feature_id"]);
    let mut d = Document::open(&odd.source).expect("writable");
    d.write(|w| {
        w.put_topology_ref(&ferritecad_document::TopologyRef {
            id: ferritecad_types::StableEntityId::new(),
            owner: odd_id,
            producer_feature: odd_id,
            expected_kind: ferritecad_document::EntityKind::Face,
            output_role: SemanticRole::EdgeChamferFace {
                edge_feature: ObjectId::new(),
                joint: ferritecad_types::ProfileJoint::new(
                    ferritecad_types::StableEntityId::new(),
                    ferritecad_types::StableEntityId::new(),
                )
                .expect("joint"),
            },
            selection: ferritecad_document::SelectionRule::Exact,
            fallback_signature: None,
        })
    })
    .expect("an extra name");
    d.close().expect("close");
    let catalog = inspect(&odd.source);
    assert_eq!(catalog["edit_extrude"]["available"], false);
    let reason = catalog["edit_extrude"]["refusal"].as_str().expect("reason");
    assert!(
        reason.contains(&odd_id.to_string()) && reason.contains("more faces"),
        "{reason}"
    );
    assert_eq!(catalog["features"][0]["editable"], false);
    assert_eq!(catalog["features"][0]["chamfer_base"], Value::Null);
    if kernel {
        let (base, version, _) = base_and_version(&odd.source);
        let v = reply(
            raise(&odd.source, &base, "9", &version, &never)
                .output()
                .expect("process"),
            OP,
            2,
        );
        assert_eq!(refused(&v), "unsupported", "{v}");
        assert!(v.to_string().contains(&odd_id.to_string()), "{v}");
        assert!(!never.exists());
    }
}
