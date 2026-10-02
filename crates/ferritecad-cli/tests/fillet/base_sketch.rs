// SPDX-License-Identifier: MIT
//! §29C: the four stored vertices of the plate under the one saved Chamfer are
//! moved or resized by the existing `edit-sketch-copy`, in a new copy. The
//! Chamfer keeps its corner (found by its two Line UUIDs, never by a row or an
//! absolute coordinate), its distance and every name; the only SQL cell pair
//! that moves is the Sketch's payload and hash. The B-Rep, the independently
//! read mesh and the cache are measured at the chosen corner of the NEW plate.
use super::*;
use crate::radius::stored_refs;
use crate::sketch::{mapped, only_this_row_changed};
use ferritecad_eval::CacheOutcome;

const SKETCH: &str = "edit-sketch-copy";
/// A distance far from every bound, so only the plate varies.
const DIST: f64 = 0.75;
/// Larger on both sides and moved.
const BIG: [f64; 4] = [-10.125, 7.5, 52.25, 20.5];
/// Smaller on both sides and moved.
const SMALL: [f64; 4] = [30.0, -2.75, 9.5, 6.25];
/// Moved only.
const SHIFTED: [f64; 4] = [X0 + 10.5, Y0 - 5.25, W, D];
/// Resized only, keeping the saved lower-left corner.
const RESIZED: [f64; 4] = [X0, Y0, W + 3.5, D - 2.0];

/// The current plate of `f` moved and resized to `rect`, corner for corner:
/// each saved vertex goes to the same corner of the new rectangle, whichever
/// coordinates the plate has now.
fn ask_rect(f: &Fixture, rect: [f64; 4]) {
    let vertices = f.sketch_row()["vertices"].as_array().expect("vertices");
    let at = |v: &Value, k: usize| v["start_mm"][k].as_f64().expect("number");
    let low = |k: usize| vertices.iter().map(|v| at(v, k)).fold(f64::MAX, f64::min);
    let (lx, ly) = (low(0), low(1));
    let points: Vec<[f64; 2]> = vertices
        .iter()
        .map(|v| {
            [
                if at(v, 0) == lx {
                    rect[0]
                } else {
                    rect[0] + rect[2]
                },
                if at(v, 1) == ly {
                    rect[1]
                } else {
                    rect[1] + rect[3]
                },
            ]
        })
        .collect();
    f.ask_starts(&points);
}

fn catalog_of(path: &Path) -> (String, String, Value) {
    let catalog = inspect(path);
    (
        catalog["sketches"][0]["sketch_id"]
            .as_str()
            .expect("sketch")
            .to_owned(),
        catalog["content_version"]
            .as_str()
            .expect("version")
            .to_owned(),
        catalog,
    )
}

fn object_ids(path: &Path) -> Vec<ObjectId> {
    let d = Document::open_read_only(path).expect("document");
    let mut ids: Vec<ObjectId> = d
        .objects()
        .expect("objects")
        .into_iter()
        .map(|o| o.id)
        .collect();
    d.close().expect("close");
    ids.sort_by_key(|i| i.to_string());
    ids
}

/// One coordinate edit of a chamfered copy into `name`, checked against what a
/// copy must be, and the new copy's measured B-Rep and mesh at the chosen
/// corner of the new plate. `corner` is the saved corner of the original plate.
fn resketched(
    f: &Fixture,
    copy: &Path,
    id: ObjectId,
    corner: [f64; 2],
    rect: [f64; 4],
    name: &str,
) -> (PathBuf, Cham) {
    let (sketch, version, before_catalog) = catalog_of(copy);
    let g = f.at_copy(copy);
    let before = std::fs::read(copy).expect("source bytes");
    let refs = stored_refs(copy);
    let ids = object_ids(copy);
    let out = f.root.path().join(format!("{name}.fcad"));
    ask_rect(&g, rect);
    let published = reply(
        g.redraw(copy, &version, &out).output().expect("process"),
        SKETCH,
        0,
    );
    assert_eq!(published["result"]["sketch_id"], sketch.as_str());
    assert_eq!(
        std::fs::read(copy).expect("bytes"),
        before,
        "source touched"
    );
    // The one SQL cell pair that moved, and nothing else.
    assert!(only_this_row_changed(copy, &out, &sketch) >= 2);
    assert_eq!(stored_refs(&out), refs, "every name and its UUID kept");
    assert_eq!(object_ids(&out), ids, "no UUID minted, none dropped");

    let new_corner = mapped(rect, corner);
    let after = inspect(&out);
    assert_eq!(after["features"][0]["distance_mm"], H, "the height is kept");
    assert_eq!(after["chamfers"][0]["feature_id"], json!(id.to_string()));
    assert_eq!(
        after["chamfers"][0]["edge"], before_catalog["chamfers"][0]["edge"],
        "the same Lines meet at the Chamfer's corner"
    );
    assert_eq!(after["chamfers"][0]["distance_mm"], DIST);
    let base = &after["sketches"][0]["chamfer_base"];
    assert_eq!(base["distance_mm"], DIST);
    assert_eq!(base["corner_mm"], json!(new_corner));
    assert_eq!(base, &after["features"][0]["chamfer_base"]);
    assert_eq!(
        base["edge"],
        before_catalog["sketches"][0]["chamfer_base"]["edge"]
    );
    assert_eq!(after["sketches"][0]["editable"], true);
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

    // The B-Rep through the document's own references: the chosen corner of
    // the new plate, the same distance, the volume, plane, outward normal and
    // area; the mesh is read independently and holds the same part.
    let (cold, _) = measure_c(&out, None);
    check_brep(&cold, rect, new_corner, DIST, H);
    let m = mesh(&out, &f.root.path().join(format!("{name}.stl")));
    check_mesh_c(&m, rect, new_corner, DIST, H);
    (out, cold)
}

fn leaked(tag: &str, which: &str) -> &'static str {
    // Leaked on purpose: test-only names that live as long as the process.
    Box::leak(format!("{tag}-{which}").into_boxed_str())
}

/// Every corner of the translated, fractional plate, drawn three ways (both
/// windings, other starting Lines), made larger and moved; the Chamfer stays on
/// the same corner by its Line UUIDs. For one corner of each order: smaller,
/// only moved, only resized, the cache, and a later height and distance edit.
#[test]
fn native_the_sketch_of_a_chamfered_plate_changes_at_every_corner_in_three_orders() {
    if !native() {
        return;
    }
    for (index, corners) in [CCW, CW_FROM_UPPER_RIGHT, CCW_FROM_THIRD]
        .into_iter()
        .enumerate()
    {
        let f = Fixture::drawn(Plate::new(corners));
        for (k, corner) in corners.into_iter().enumerate() {
            let tag = format!("s{index}-{k}");
            let (copy, id, was) = chamfer_at(&f, &f.source, corner, DIST, &format!("{tag}-src"));
            let (big, cold_big) = resketched(&f, &copy, id, corner, BIG, leaked(&tag, "big"));
            assert_eq!(cold_big.faces, 7);
            assert!(
                cold_big.volume > was.volume,
                "a larger plate has more material"
            );
            if k != index {
                continue;
            }
            let (small, cold_small) =
                resketched(&f, &copy, id, corner, SMALL, leaked(&tag, "small"));
            assert!(cold_small.volume < was.volume);
            let (shifted, cold_shifted) =
                resketched(&f, &copy, id, corner, SHIFTED, leaked(&tag, "shifted"));
            assert!(
                (cold_shifted.volume - was.volume).abs() < 1e-9 * was.volume,
                "moving a plate keeps its material"
            );
            // Only the position of the Chamfer's face changes.
            assert!(
                (cold_shifted.plane_origin[0] - was.plane_origin[0] - (SHIFTED[0] - X0)).abs()
                    < 1e-9
            );
            assert_eq!(
                cold_shifted.normal, was.normal,
                "the same corner faces the same way"
            );
            resketched(&f, &copy, id, corner, RESIZED, leaked(&tag, "resized"));
            // A second edit of an edited plate is the same operation again.
            let (_, again) = resketched(&f, &big, id, corner, SHIFTED, leaked(&tag, "twice"));
            assert!((again.volume - was.volume).abs() < 1e-9 * was.volume);
            super::super::fbx(&big, &format!("sketch-{index}-big"));
            super::super::fbx(&small, &format!("sketch-{index}-small"));
            super::super::fbx(&shifted, &format!("sketch-{index}-shifted"));

            // The cache: the old copy fills it, the new plate misses for the
            // Extrude and for the Chamfer over it and never reads the old
            // contour or the old corner; a second run reads what the first
            // wrote and equals the cold one.
            let cache = f.root.path().join(format!("{tag}.cache"));
            let (first, events) = measure_c(&copy, Some(&cache));
            assert!(
                events.iter().all(|e| e.outcome == CacheOutcome::Miss),
                "{events:?}"
            );
            first.same_as(&was);
            let (fresh, events) = measure_c(&big, Some(&cache));
            assert!(
                events.iter().any(|e| e.feature == id)
                    && events.len() >= 2
                    && events.iter().all(|e| e.outcome == CacheOutcome::Miss),
                "the new plate must not hit the old one: {events:?}"
            );
            check_brep(&fresh, BIG, mapped(BIG, corner), DIST, H);
            assert!((fresh.volume - was.volume).abs() > 1.0, "not the old plate");
            let (again, events) = measure_c(&big, Some(&cache));
            assert!(
                events.iter().all(|e| e.outcome == CacheOutcome::Hit),
                "{events:?}"
            );
            again.same_as(&cold_big);
            let (old, events) = measure_c(&copy, Some(&cache));
            assert!(
                events.iter().all(|e| e.outcome == CacheOutcome::Hit),
                "the old plate is still its own entry: {events:?}"
            );
            old.same_as(&was);

            // The height and the distance editors still work on the edited
            // plate, and the distance bound follows the NEW adjacent sides.
            let new_corner = mapped(SMALL, corner);
            let (_, version, catalog) = catalog_of(&small);
            let base = catalog["features"][0]["feature_id"]
                .as_str()
                .expect("base")
                .to_owned();
            let taller = f.root.path().join(format!("{tag}-tall.fcad"));
            reply(
                super::base_height::raise(&small, &base, "9.5", &version, &taller)
                    .output()
                    .expect("process"),
                "edit-extrude",
                0,
            );
            let (m, _) = measure_c(&taller, None);
            check_brep(&m, SMALL, new_corner, DIST, 9.5);
            let max = catalog["chamfers"][0]["distance_edit"]["max_distance_mm"]
                .as_f64()
                .expect("max");
            assert_eq!(max, SMALL[3].min(SMALL[2]) - 0.01);
            let request = f.root.path().join(format!("{tag}-d.json"));
            write(&request, &json!({"request_version":1,"distance_mm":max}));
            let wide = f.root.path().join(format!("{tag}-d.fcad"));
            reply(
                edit_from(&small, &id.to_string(), &version, &request, &wide)
                    .output()
                    .expect("process"),
                OPE,
                0,
            );
            let (m, _) = measure_c(&wide, None);
            check_brep(&m, SMALL, new_corner, max, H);
        }
    }
}

/// The same volume at another corner is a different part: the measurement
/// finds the chosen corner of the edited plate, and an edit moves the Chamfer
/// with its own vertex.
#[test]
fn native_the_sketch_edit_keeps_the_chosen_corner_not_another_with_the_same_volume() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let (a, ia, _) = chamfer_at(&f, &f.source, [X0, Y0], 2.5, "ca");
    let (b, ib, _) = chamfer_at(&f, &f.source, [X0 + W, Y0 + D], 2.5, "cb");
    let (a_out, ma) = resketched_at(&f, &a, ia, [X0, Y0], 2.5, "ca-big");
    let (b_out, mb) = resketched_at(&f, &b, ib, [X0 + W, Y0 + D], 2.5, "cb-big");
    assert!(
        (ma.volume - mb.volume).abs() < 1e-9 * ma.volume,
        "same volume"
    );
    assert!(
        (ma.normal[0] - mb.normal[0]).abs() > 1.0,
        "opposite corners face opposite ways"
    );
    let a_corner = mapped(BIG, [X0, Y0]);
    let b_corner = mapped(BIG, [X0 + W, Y0 + D]);
    check_brep(&ma, BIG, a_corner, 2.5, H);
    check_brep(&mb, BIG, b_corner, 2.5, H);
    let ma_mesh = mesh(&a_out, &f.root.path().join("a-big.stl"));
    let mb_mesh = mesh(&b_out, &f.root.path().join("b-big.stl"));
    check_mesh_c(&ma_mesh, BIG, a_corner, 2.5, H);
    check_mesh_c(&mb_mesh, BIG, b_corner, 2.5, H);
    for (wrong, why) in [
        (b_corner, "the other corner"),
        (
            [a_corner[0], a_corner[1] + BIG[3]],
            "the corner above the chosen one",
        ),
        ([X0, Y0], "the saved coordinates of the old plate"),
    ] {
        let swapped = std::panic::catch_unwind(|| {
            check_mesh_c(&ma_mesh, BIG, wrong, 2.5, H);
        });
        assert!(swapped.is_err(), "{why} must fail on this mesh");
    }
    assert_ne!(ia, ib);
    assert_eq!(
        inspect(&a_out)["chamfers"][0]["feature_id"],
        json!(ia.to_string())
    );
}

fn resketched_at(
    f: &Fixture,
    copy: &Path,
    id: ObjectId,
    corner: [f64; 2],
    distance: f64,
    name: &str,
) -> (PathBuf, Cham) {
    let (_, version, _) = catalog_of(copy);
    let g = f.at_copy(copy);
    let out = f.root.path().join(format!("{name}.fcad"));
    ask_rect(&g, BIG);
    reply(
        g.redraw(copy, &version, &out).output().expect("process"),
        SKETCH,
        0,
    );
    assert_eq!(
        inspect(&out)["chamfers"][0]["feature_id"],
        json!(id.to_string())
    );
    let (m, _) = measure_c(&out, None);
    check_brep(&m, BIG, mapped(BIG, corner), distance, H);
    (out, m)
}

/// The smallest plate side a distance of `d` still fits, and the next
/// representable value below it, from the policy's own expression.
fn bound_of(d: f64) -> (f64, f64) {
    let fits = |s: f64| s - 0.01 >= d;
    let mut s = d + 0.01;
    while !fits(s) {
        s = s.next_up();
    }
    while fits(s.next_down()) {
        s = s.next_down();
    }
    (s, s.next_down())
}

/// The distance is kept and checked on the NEW adjacent sides: exact at the
/// bound, refused just below it, never reduced; and a candidate that leaves the
/// class, mirrors the plate, reorders or renames the Lines is refused naming
/// its reason. Every refusal leaves the directory and the source as they were.
#[test]
fn native_sketch_bounds_and_candidates_are_exact_under_a_chamfer() {
    if !native() {
        return;
    }
    let d = 2.5;
    let f = Fixture::drawn(Plate::new(CCW));
    let (copy, id, _) = chamfer_at(&f, &f.source, [X0 + W, Y0], d, "src");
    let (_, version, _) = catalog_of(&copy);
    let g = f.at_copy(&copy);
    let before = std::fs::read(&copy).expect("bytes");
    let out = f.root.path().join("never.fcad");
    let refuse = |why: &str, kind: &str| {
        let names = entries(f.root.path());
        let v = reply(
            g.redraw(&copy, &version, &out).output().expect("process"),
            SKETCH,
            2,
        );
        assert_eq!(refused(&v), kind, "{why}: {v}");
        assert!(!out.exists(), "{why}");
        assert_eq!(std::fs::read(&copy).expect("bytes"), before, "{why}");
        assert_eq!(
            entries(f.root.path()),
            names,
            "{why}: a refusal left something"
        );
        v
    };

    // The bound: the shorter adjacent side is the depth here.
    let (ok, below) = bound_of(d);
    let at_bound = [0.0, 0.0, 40.0, ok];
    ask_rect(&g, at_bound);
    let exact = f.root.path().join("exact.fcad");
    reply(
        g.redraw(&copy, &version, &exact).output().expect("process"),
        SKETCH,
        0,
    );
    let (m, _) = measure_c(&exact, None);
    check_brep(&m, at_bound, mapped(at_bound, [X0 + W, Y0]), d, H);
    assert_eq!(
        inspect(&exact)["chamfers"][0]["distance_mm"],
        d,
        "the distance is kept, not reduced"
    );
    ask_rect(&g, [0.0, 0.0, 40.0, below]);
    let v = refuse("just below the bound", "input");
    let text = v.to_string();
    assert!(
        text.contains(&id.to_string()) && text.contains("does not fit"),
        "{text}"
    );
    // The same on the other adjacent side.
    ask_rect(&g, [0.0, 0.0, below, 40.0]);
    refuse("the width just below the bound", "input");
    ask_rect(&g, [0.0, 0.0, ok, 40.0]);
    let wide = f.root.path().join("wide.fcad");
    reply(
        g.redraw(&copy, &version, &wide).output().expect("process"),
        SKETCH,
        0,
    );
    // A shorter side that is NOT adjacent to the Chamfer's corner does not
    // matter: the corner at (x0 + w, y0) touches the bottom and right Lines.
    // Degenerate and mirrored candidates.
    ask_rect(&g, [0.0, 0.0, 40.0, 0.0]);
    refuse("a zero depth", "input");
    ask_rect(&g, [X0 + W, Y0, -W, D]);
    refuse("a mirrored plate", "input");
    ask_rect(&g, [X0, Y0 + D, W, -D]);
    refuse("a flipped plate", "input");
    // Turned half a turn: the winding is the saved one, but every Line runs
    // the other way and the Chamfer would land on the opposite corner.
    ask_rect(&g, [X0 + W, Y0 + D, -W, -D]);
    let v = refuse("a plate turned half a turn", "input");
    assert!(v.to_string().contains("side"), "{v}");

    // Not a rectangle, a wrong order, a foreign Line and a missing Line.
    let row = g.sketch_row()["vertices"].clone();
    let mut at: Vec<[f64; 2]> = row
        .as_array()
        .expect("vertices")
        .iter()
        .map(|v| {
            mapped(
                BIG,
                [
                    v["start_mm"][0].as_f64().expect("x"),
                    v["start_mm"][1].as_f64().expect("y"),
                ],
            )
        })
        .collect();
    at[2][0] += 1.0;
    at[2][1] += 0.5;
    g.ask_starts(&at);
    refuse("a plate that is no longer a rectangle", "unsupported");
    let mut swapped = row.as_array().expect("vertices").clone();
    swapped.swap(0, 1);
    write(
        &g.request,
        &json!({"request_version":1,"vertices": swapped.iter().map(|v| json!({"curve_id": v["curve_id"], "start_mm": v["start_mm"]})).collect::<Vec<_>>()}),
    );
    refuse("the Lines in another order", "input");
    let mut foreign: Vec<Value> = row
        .as_array()
        .expect("vertices")
        .iter()
        .map(|v| json!({"curve_id": v["curve_id"], "start_mm": v["start_mm"]}))
        .collect();
    foreign[1]["curve_id"] = json!(ferritecad_types::StableEntityId::new().to_string());
    write(
        &g.request,
        &json!({"request_version":1,"vertices": foreign}),
    );
    refuse("a foreign Line", "input");
    foreign.truncate(3);
    write(
        &g.request,
        &json!({"request_version":1,"vertices": foreign}),
    );
    refuse("a missing Line", "input");
    // Not a finite number.
    let mut bad: Vec<Value> = row
        .as_array()
        .expect("vertices")
        .iter()
        .map(|v| json!({"curve_id": v["curve_id"], "start_mm": v["start_mm"]}))
        .collect();
    bad[0]["start_mm"] = json!([f64::MAX * 4.0, 0.0]);
    write(&g.request, &json!({"request_version":1,"vertices": bad}));
    let v = reply(
        g.redraw(&copy, &version, &out).output().expect("process"),
        SKETCH,
        2,
    );
    assert!(matches!(refused(&v), "input" | "unsupported"), "{v}");
    assert!(!out.exists());
}

/// Refusals, races and the guards of the shared copy job on this branch: each
/// leaves the directory and the source exactly as they were; an extra saved
/// reference that resolves is kept, one that does not is never published.
#[test]
fn native_sketch_refusals_races_and_guards_are_atomic_under_a_chamfer() {
    if !native() {
        return;
    }
    let f = Fixture::drawn(Plate::new(CCW));
    let (copy, _id, _) = chamfer_at(&f, &f.source, [X0 + W, Y0], DIST, "src");
    let (sketch, version, _) = catalog_of(&copy);
    let g = f.at_copy(&copy);
    let before = std::fs::read(&copy).expect("bytes");
    let names = entries(f.root.path());
    let out = f.root.path().join("never.fcad");
    ask_rect(&g, BIG);
    // A stale version: another document's content version is not this one's.
    let (_, stale, _) = catalog_of(&f.source);
    let v = reply(
        g.redraw(&copy, &stale, &out).output().expect("process"),
        SKETCH,
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    assert!(!out.exists());
    // No clobber, and the source is never its own destination.
    std::fs::write(&out, b"keep me").expect("existing");
    let v = reply(
        g.redraw(&copy, &version, &out).output().expect("process"),
        SKETCH,
        2,
    );
    assert!(matches!(refused(&v), "input" | "io"), "{v}");
    assert_eq!(std::fs::read(&out).expect("bytes"), b"keep me");
    std::fs::remove_file(&out).expect("cleanup");
    let v = reply(
        g.redraw(&copy, &version, &copy).output().expect("process"),
        SKETCH,
        2,
    );
    assert!(matches!(refused(&v), "input" | "io"), "{v}");
    // A foreign Sketch UUID.
    let foreign = ObjectId::new().to_string();
    let v = reply(
        cli()
            .arg(SKETCH)
            .arg(&copy)
            .args(["--sketch", &foreign, "--expect-version", &version])
            .arg("--request")
            .arg(&g.request)
            .arg("-o")
            .arg(&out)
            .arg("--json")
            .output()
            .expect("process"),
        SKETCH,
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    // A refusal whose report cannot be written is still a refusal; so is a
    // refused request.
    ask_rect(&g, [0.0, 0.0, 40.0, 0.0]);
    assert_eq!(
        g.redraw(&copy, &version, &out)
            .stdout(super::super::pipe::closed_pipe())
            .stderr(super::super::pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
    assert_eq!(std::fs::read(&copy).expect("bytes"), before);
    assert_eq!(entries(f.root.path()), names, "a refusal left something");
    assert!(!out.exists());
    let _ = sketch;

    // An extra saved reference of the base Extrude: allowed when it resolves
    // against the rebuilt solid, never published when it does not.
    for unresolved in [false, true] {
        let h = chamfered_without_kernel(Plate::new(CCW), [X0 + W, Y0], DIST);
        let base = uuid(&h.catalog["features"][0]["feature_id"]);
        let mut document = Document::open(&h.source).expect("source");
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
        document.close().expect("close");
        let (sketch, version, catalog) = catalog_of(&h.source);
        assert_eq!(catalog["sketches"][0]["editable"], true, "{catalog}");
        let kept = std::fs::read(&h.source).expect("bytes");
        let output = h.root.path().join("sketch.fcad");
        ask_rect(&h, BIG);
        let names = entries(h.root.path());
        let answer = reply(
            h.redraw(&h.source, &version, &output)
                .output()
                .expect("process"),
            SKETCH,
            if unresolved { 2 } else { 0 },
        );
        if unresolved {
            assert_eq!(refused(&answer), "topology", "{answer}");
            assert!(!output.exists());
            assert_eq!(entries(h.root.path()), names, "scratch leaked");
        } else {
            assert_eq!(stored_refs(&output), stored_refs(&h.source));
            assert!(only_this_row_changed(&h.source, &output, &sketch) >= 2);
        }
        assert_eq!(std::fs::read(&h.source).expect("bytes"), kept);
    }
}

/// Discovery on a chamfered plate and the unchanged protocol, with the real
/// order of checks of whichever build this is; no kernel is needed.
#[test]
fn chamfer_base_sketch_discovery_and_protocol_without_native() {
    let kernel = ferritecad_occt::is_available();
    if !kernel {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
    }
    let plain = Fixture::drawn(Plate::new(CCW));
    assert_eq!(plain.catalog["sketches"][0]["chamfer_base"], Value::Null);
    let f = chamfered_without_kernel(Plate::new(CCW), [X0 + W, Y0], 2.375);
    let row = &f.catalog["sketches"][0];
    assert_eq!(row["editable"], true, "{row}");
    assert_eq!(row["refusal"], Value::Null);
    assert_eq!(row["vertices"].as_array().expect("vertices").len(), 4);
    assert_eq!(row["fillet_base"], Value::Null);
    assert_eq!(row["fillet_history"], Value::Null);
    assert_eq!(
        row["chamfer_base"],
        f.catalog["features"][0]["chamfer_base"]
    );
    assert_eq!(row["chamfer_base"]["corner_mm"], json!([X0 + W, Y0]));
    assert_eq!(row["chamfer_base"]["distance_mm"], 2.375);
    assert_eq!(row["chamfer_base"]["distance_unit"], "mm");
    assert_eq!(
        row["chamfer_base"]["chamfer_feature_id"],
        f.catalog["chamfers"][0]["feature_id"]
    );
    assert_eq!(
        f.catalog["sketches"][0]["constraint_edit"]["available"],
        false
    );

    // The request protocol is the existing one: a build without a kernel asks
    // for it before any other check.
    let before = std::fs::read(&f.source).expect("bytes");
    let (_, version, _) = catalog_of(&f.source);
    let out = f.root.path().join("never.fcad");
    ask_rect(&f, BIG);
    let names = entries(f.root.path());
    let v = reply(
        f.redraw(&f.source, &version, &out)
            .output()
            .expect("process"),
        SKETCH,
        if kernel { 0 } else { 2 },
    );
    if kernel {
        assert!(out.exists(), "{v}");
        std::fs::remove_file(&out).expect("cleanup");
    } else {
        assert_eq!(refused(&v), "unsupported", "{v}");
        assert!(v.to_string().contains("Open CASCADE"), "{v}");
        assert!(!out.exists());
    }
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    let mut now = entries(f.root.path());
    now.sort();
    let mut was = names;
    was.sort();
    assert_eq!(now, was);
}
