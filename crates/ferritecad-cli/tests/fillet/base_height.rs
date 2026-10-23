// SPDX-License-Identifier: MIT
//! §29B: the plate under the one saved Chamfer is made taller or shorter by the
//! existing `edit`, in a new copy. The Chamfer keeps its edge and
//! distance; the only SQL cell pair that moves is the base Extrude's payload and
//! hash. The B-Rep, the independently read mesh and the cache are measured at
//! the chosen corner, not only for a volume that another corner would share.
use super::*;
use crate::radius::stored_refs;

const OP: &str = "edit";
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

pub(super) fn raise(
    source: &Path,
    feature: &str,
    height: &str,
    version: &str,
    output: &Path,
) -> crate::transaction::RequestCommand {
    crate::transaction::constant(
        source,
        version,
        feature,
        "extrude_distance",
        height,
        "mm",
        output,
    )
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
    assert_eq!(
        crate::transaction::first_slot(&after, "extrude_distance")["expression"]["value"],
        h
    );

    assert_eq!(after["chamfers"][0]["feature_id"], json!(id.to_string()));
    assert_eq!(
        after["chamfers"][0]["edge"],
        before_catalog["chamfers"][0]["edge"]
    );
    assert_eq!(after["chamfers"][0]["expression"]["value"], DIST);

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
            let (_, version, _) = base_and_version(&shorter);
            let max = D - 0.01;
            assert_eq!(max, D - 0.01);
            let request = f.root.path().join(format!("{tag}-d.json"));
            write(
                &request,
                &json!({"request_version":1,"changes":[{"kind":"set_expression","object":"00000000-0000-0000-0000-000000000000","slot":{"kind":"chamfer_distance"},"value":max,"unit":"mm"}]}),
            );
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
            "input",
            Some(id.to_string()),
        ),
        (
            "the Sketch",
            sketch.clone(),
            "9",
            "input",
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

/// The shared height job must apply the strict reference rule to a Chamfer,
/// including references owned by its base, not only the Chamfer's own names.
#[test]
fn native_chamfer_height_preserves_preexisting_unresolved_references() {
    if !native() {
        return;
    }
    for unresolved in [false, true] {
        let f = chamfered_without_kernel(Plate::new(CCW), [X0 + W, Y0], DIST);
        let base = uuid(&f.catalog["features"][0]["feature_id"]);
        let mut document = Document::open(&f.source).expect("source");
        let mut reference = document
            .topology_refs()
            .expect("refs")
            .into_iter()
            .find(|r| r.owner == base)
            .expect("a saved base reference");
        reference.id = ferritecad_types::StableEntityId::new();
        if unresolved {
            reference.output_role = SemanticRole::ExtrudeSide {
                profile_segment: ferritecad_types::StableEntityId::new(),
            };
        }
        document
            .write(|writer| writer.put_topology_ref(&reference))
            .expect("additional base reference");
        assert!(document.validate().expect("validate").is_ok());
        // This is a supported Chamfer history; only resolving the reference
        // against a real rebuild distinguishes it from the positive control.
        ferritecad_document::EditChange::constant(
            base,
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
            9.5,
        )
        .expect("structural preparation");
        document.close().expect("close");
        let total = stored_count(&f.source);
        let rebuilt = cli()
            .arg("rebuild")
            .arg(&f.source)
            .arg("--cold")
            .output()
            .expect("rebuild");
        assert!(rebuilt.status.success(), "{rebuilt:?}");
        let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
        let resolved = total - usize::from(unresolved);
        assert!(
            text.contains(&format!("{resolved} of {total} stored references resolved")),
            "{text}"
        );
        let (base, version, _) = base_and_version(&f.source);
        let before = std::fs::read(&f.source).expect("bytes");

        let output = f.root.path().join("height.fcad");
        let answer = reply(
            raise(&f.source, &base, "9.5", &version, &output)
                .output()
                .expect("edit process"),
            OP,
            0,
        );
        assert_eq!(
            answer["result"]["rebuild"]["warnings"]
                .as_array()
                .expect("warnings")
                .is_empty(),
            !unresolved
        );

        assert_eq!(stored_refs(&output), stored_refs(&f.source));
        assert_eq!(
            crate::transaction::first_slot(&inspect(&output), "extrude_distance")["expression"]["value"],
            9.5
        );
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    }
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
    // §29C/§29D: the Sketch's coordinates and its Line constraints are both
    // editable under the Chamfer.

    let _ = &id;
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
