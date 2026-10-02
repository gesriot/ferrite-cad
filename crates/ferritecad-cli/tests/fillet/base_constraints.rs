// SPDX-License-Identifier: MIT
//! §29D: the Line constraints of the rectangle under the one saved Chamfer,
//! through the existing `edit-sketch-constraints-copy`. The stored Lines stay the
//! solver's starting guess; the part, and the Chamfer's corner and distance, are
//! the solved plate's. Every geometric claim is measured on a rebuilt B-Rep and
//! on an independently read mesh at the chosen corner of the SOLVED plate.
use super::*;
use crate::constraints::{
    dimensioned, horizontal, length, only_this_sketch_changed, pin, rect_of, rule, solved,
    solved_vertex, solving,
};
use crate::radius::stored_refs;
use ferritecad_eval::CacheOutcome;

const CONSTRAIN: &str = "edit-sketch-constraints-copy";
const DIST: f64 = 0.75;

/// The whole SQL row of the plate's Sketch.
fn sketch_row(path: &Path, sketch: &str) -> Vec<rusqlite::types::Value> {
    let id: ObjectId = sketch.parse().expect("UUID");
    let key = rusqlite::types::Value::Blob(id.to_bytes().to_vec());
    tables(path)["objects"]
        .1
        .iter()
        .find(|r| r.contains(&key))
        .expect("the Sketch row")
        .clone()
}

fn base_of(path: &Path) -> (String, String) {
    let catalog = inspect(path);
    (
        catalog["features"][0]["feature_id"]
            .as_str()
            .expect("base")
            .to_owned(),
        catalog["content_version"]
            .as_str()
            .expect("version")
            .to_owned(),
    )
}

/// What the rebuild solved, measured: the rectangle `[x0, y0, w, d]` of the
/// solved Lines and where the stored corner went, by its Line UUIDs' order.
fn solved_rect(path: &Path, stored: &[[f64; 2]], corner: [f64; 2]) -> ([f64; 4], [f64; 2]) {
    let (starts, _) = solved(path);
    let at = stored.iter().position(|v| *v == corner).expect("corner");
    (rect_of(&starts), starts[at])
}

/// One constraint copy of a chamfered plate `f`, whose request is already
/// written, checked against everything a copy must be, and measured against the
/// solved plate `expected` (Line for Line): identities, the exact SQL
/// allowlist, the stored Lines, the Chamfer row, the solve and its DOF, the
/// B-Rep at the chosen corner, a mesh read apart.
fn published(
    f: &Fixture,
    corner: [f64; 2],
    d: f64,
    expected: &[[f64; 2]],
    dof: usize,
    name: &str,
) -> PathBuf {
    let before = std::fs::read(&f.source).expect("source bytes");
    let refs = stored_refs(&f.source);
    let stored = f.stored_starts();
    let chamfer_before = f.catalog["chamfers"][0].clone();
    let copy = f.root.path().join(format!("{name}.fcad"));
    let v = reply(f.constrain(&copy).output().expect("process"), CONSTRAIN, 0);
    assert_eq!(v["result"]["sketch_id"], f.plate_sketch_id());
    assert_eq!(v["result"]["solve"]["degrees_of_freedom"], dof, "{v}");
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    only_this_sketch_changed(&f.source, &copy, f.plate_sketch_id());
    assert_eq!(stored_refs(&copy), refs, "every name and its UUID kept");

    // The stored Lines are still the starting guess, UUID for UUID; the
    // Chamfer is the saved one, and says its numbers are stored facts.
    let after = inspect(&copy);
    let g = f.at_copy(&copy);
    assert_eq!(g.stored_starts(), stored, "stored coordinates are kept");
    for i in 0..4 {
        assert_eq!(g.line(i), f.line(i), "Line {i}");
    }
    let chamfer = &after["chamfers"][0];
    for key in ["feature_id", "edge", "distance_mm", "previous_feature_id"] {
        assert_eq!(chamfer[key], chamfer_before[key], "{key}");
    }
    assert_eq!(chamfer["profile_constrained"], true);
    assert_eq!(chamfer["distance_edit"]["max_distance_mm"], Value::Null);
    assert_eq!(chamfer["distance_edit"]["available"], true);
    let context = &g.constraint_row()["chamfer_base"];
    assert_eq!(context["chamfer_feature_id"], chamfer_before["feature_id"]);
    assert_eq!(context["corner_mm"], json!(corner), "a stored fact");
    assert_eq!(context["distance_mm"], d);
    assert_eq!(context["profile_constrained"], true);
    assert_eq!(
        after["features"][0]["chamfer_base"], *context,
        "one object on the base Extrude and the Sketch's constraint editor"
    );
    let row = &after["sketches"][0];
    assert_eq!(row["editable"], false, "the coordinates are the solver's");
    assert!(
        row["refusal"]
            .as_str()
            .expect("a reason")
            .contains(chamfer["feature_id"].as_str().expect("id")),
        "{row}"
    );
    assert_eq!(row["chamfer_base"], Value::Null);

    // The solve, measured.
    let (starts, measured_dof) = solved(&copy);
    assert_eq!(measured_dof, dof, "DOF of the cold rebuild");
    let mut worst: f64 = 0.;
    for (s, e) in starts.iter().zip(expected) {
        worst = worst.max((s[0] - e[0]).abs()).max((s[1] - e[1]).abs());
    }
    assert!(worst < 1e-9, "{starts:?} is not {expected:?}");
    let rect = rect_of(expected);
    let moved = expected[stored.iter().position(|v| *v == corner).expect("corner")];

    // The B-Rep: 7 faces, the volume of the SOLVED plate, the plane, the
    // outward normal and the area at the chosen corner; the mesh read apart.
    let n = refs.len();
    let rebuilt = cli()
        .arg("rebuild")
        .arg(&copy)
        .arg("--cold")
        .output()
        .expect("rebuild");
    let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
    assert!(
        text.contains(&format!("{n} of {n} stored references resolved")),
        "{text}"
    );
    let (cold, _) = measure_c(&copy, None);
    check_brep(&cold, rect, moved, d, H);
    let m = mesh(&copy, &f.root.path().join(format!("{name}.stl")));
    check_mesh_c(&m, rect, moved, d, H);
    copy
}

/// The largest distance the domain's one predicate accepts on the Lines the
/// rebuild of `path` solved its plate from: the evaluator's own question, asked
/// of the very presentation it was asked of.
fn solved_max_distance(path: &Path) -> f64 {
    let d = Document::open_read_only(path).expect("reopen");
    let objects = d.objects().expect("objects");
    let sketch = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("the plate's Sketch")
        .id;
    let chamfer = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Chamfer(c) => Some(c.clone()),
            _ => None,
        })
        .expect("the Chamfer");
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = ferritecad_eval::rebuild_cold(&d, &mut k, &OperationContext::default())
        .expect("cold rebuild");
    let curves: Vec<ferritecad_document::SketchCurve> = built
        .sketch_presentation(sketch)
        .expect("presentation")
        .curves()
        .iter()
        .map(|c| ferritecad_document::SketchCurve {
            id: c.id(),
            construction: c.is_construction(),
            geometry: c.geometry().clone(),
        })
        .collect();
    let corner = ferritecad_document::evaluable_chamfer(&objects, &chamfer, Some(&curves))
        .expect("the saved distance fits");
    built.release_all(&mut k);
    d.close().expect("close");
    corner.max_distance_mm
}

fn plate_with_chamfer(
    corners: [[f64; 2]; 4],
    corner: [f64; 2],
    d: f64,
    name: &str,
) -> (Fixture, Fixture, PathBuf, ObjectId) {
    let f = Fixture::drawn(Plate::new(corners));
    let (copy, id, _) = chamfer_at(&f, &f.source, corner, d, name);
    let g = f.at_copy(&copy);
    (f, g, copy, id)
}

/// Fully dimensioned in both windings and from another starting Line, at every
/// corner: translated and with both sides changed to fractional sizes. Stored
/// and solved differ; the Chamfer is the same named flat of the same distance at
/// the corner its two Lines now meet, with DOF 0 measured.
#[test]
fn native_constraints_solve_a_chamfered_plate_at_every_corner_in_three_orders() {
    if !solving() {
        return;
    }
    let (at, width, depth) = ([-9.25, -2.5], 41.125, 15.625);
    for (index, corners) in [CCW, CW_FROM_UPPER_RIGHT, CCW_FROM_THIRD]
        .into_iter()
        .enumerate()
    {
        for (k, corner) in corners.into_iter().enumerate() {
            let tag = format!("c{index}-{k}");
            let (_f, g, _, _) = plate_with_chamfer(corners, corner, DIST, &format!("{tag}-src"));
            g.ask_constraints(&[], &dimensioned(&g, at, width, depth));
            let starts = g.stored_starts();
            let expected: Vec<_> = starts
                .iter()
                .map(|v| solved_vertex(&starts, *v, at, width, depth))
                .collect();
            assert_ne!(expected, starts, "the solve moves the plate");
            let copy = published(&g, corner, DIST, &expected, 0, &format!("{tag}-solved"));
            if k == index {
                super::super::fbx(&copy, &format!("cons-{index}-solved"));
            }
        }
    }
}

/// The same volume at another corner is a different part: a solved plate whose
/// Chamfer sits at one corner is measured against the other.
#[test]
fn native_the_solved_chamfer_is_at_its_own_corner_not_another_with_the_same_volume() {
    if !solving() {
        return;
    }
    let (at, width, depth) = ([1.5, -3.25], 33.0, 14.5);
    let mut meshes = Vec::new();
    for (name, corner) in [("a", CCW[0]), ("b", CCW[2])] {
        let (_f, g, _, _) = plate_with_chamfer(CCW, corner, 2.5, &format!("same-{name}"));
        g.ask_constraints(&[], &dimensioned(&g, at, width, depth));
        let starts = g.stored_starts();
        let expected: Vec<_> = starts
            .iter()
            .map(|v| solved_vertex(&starts, *v, at, width, depth))
            .collect();
        let copy = published(
            &g,
            corner,
            2.5,
            &expected,
            0,
            &format!("same-{name}-solved"),
        );
        let (m, _) = measure_c(&copy, None);
        meshes.push((
            mesh(&copy, &g.root.path().join(format!("{name}.stl"))),
            m,
            expected,
            g.stored_starts(),
            corner,
        ));
    }
    let (ma, mb) = (&meshes[0].1, &meshes[1].1);
    assert!(
        (ma.volume - mb.volume).abs() < 1e-9 * ma.volume,
        "same volume"
    );
    assert!(
        (ma.normal[0] - mb.normal[0]).abs() > 1.0,
        "opposite corners"
    );
    let rect = rect_of(&meshes[0].2);
    let at_of = |m: &(Mesh, Cham, Vec<[f64; 2]>, Vec<[f64; 2]>, [f64; 2])| {
        m.2[m.3.iter().position(|v| *v == m.4).expect("corner")]
    };
    let (wrong, right) = (at_of(&meshes[1]), at_of(&meshes[0]));
    check_mesh_c(&meshes[0].0, rect, right, 2.5, H);
    let swapped = std::panic::catch_unwind(|| check_mesh_c(&meshes[0].0, rect, wrong, 2.5, H));
    assert!(swapped.is_err(), "the other corner must fail on this mesh");
    let stored_corner =
        std::panic::catch_unwind(|| check_mesh_c(&meshes[0].0, [X0, Y0, W, D], CCW[0], 2.5, H));
    assert!(
        stored_corner.is_err(),
        "the stored plate is not the solved one"
    );
}

/// Replace length, then the exact UUID removed, then every user constraint: the
/// closure links remain, the coordinate editor works again on the stored
/// coordinates (which were never overwritten), and nothing is dropped. The
/// height and the distance are edited between the steps with the Sketch row
/// byte for byte; a real cache misses on the plate and the Chamfer and never
/// returns the old part.
#[test]
fn native_replace_length_exact_removals_and_closure_recovery_keep_the_chamfer() {
    if !solving() {
        return;
    }
    let corner = [X0 + W, Y0 + D];
    let (_f, g, _, id) = plate_with_chamfer(CCW, corner, DIST, "src");
    let stored_original = g.stored_starts();
    let (at, width, depth) = ([0.5, 1.25], 30.75, 10.5);
    g.ask_constraints(&[], &dimensioned(&g, at, width, depth));
    let starts = g.stored_starts();
    let map = |w: f64, dep: f64| -> Vec<[f64; 2]> {
        starts
            .iter()
            .map(|v| solved_vertex(&starts, *v, at, w, dep))
            .collect()
    };
    let first = published(&g, corner, DIST, &map(width, depth), 0, "first");

    // Replace the width: the stored distance UUID out, a new one in.
    let h = g.at_copy(&first);
    let listed = h.constraint_row()["constraints"]
        .as_array()
        .expect("list")
        .clone();
    let width_rule = listed
        .iter()
        .find(|c| c["rule"]["kind"] == "distance" && c["rule"]["distance"] == width)
        .expect("the width")["constraint_id"]
        .clone();
    h.ask_constraints(
        std::slice::from_ref(&width_rule),
        &[length(&h.line(0), 22.25)],
    );
    let place = h.root.path().join("in-place.fcad");
    std::fs::copy(&first, &place).expect("copy");
    let cache = place.with_extension("fcad-cache");
    let (old, events) = measure_c(&place, Some(&cache));
    assert!(events.iter().all(|e| e.outcome == CacheOutcome::Miss));
    let replaced = published(&h, corner, DIST, &map(22.25, depth), 0, "replaced");
    let listed_now = inspect(&replaced)["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("list")
        .clone();
    assert!(
        !listed_now.iter().any(|c| c["constraint_id"] == width_rule),
        "the replaced UUID is gone"
    );
    assert_eq!(listed_now.len(), listed.len(), "one out, one in");
    let (fresh, events) = measure_c(&replaced, Some(&cache));
    assert!(
        events.iter().any(|e| e.feature == id)
            && events.iter().all(|e| e.outcome == CacheOutcome::Miss),
        "the new solved plate must not hit the old one: {events:?}"
    );
    assert!((fresh.volume - old.volume).abs() > 1.0, "not the old part");
    let (again, events) = measure_c(&replaced, Some(&cache));
    assert!(
        events.iter().all(|e| e.outcome == CacheOutcome::Hit),
        "{events:?}"
    );
    again.same_as(&fresh);

    // The height and the distance after constraints: constraints byte for byte.
    let r = h.at_copy(&replaced);
    let sketch_id = r.plate_sketch_id().to_owned();
    let row = sketch_row(&replaced, &sketch_id);
    let (base, version) = base_of(&replaced);
    let tall = r.root.path().join("tall.fcad");
    reply(
        super::base_height::raise(&replaced, &base, "9.5", &version, &tall)
            .output()
            .expect("process"),
        "edit-extrude",
        0,
    );
    assert_eq!(
        sketch_row(&tall, &sketch_id),
        row,
        "the Sketch row is byte-equal"
    );
    let rect = rect_of(&map(22.25, depth));
    let moved = map(22.25, depth)[stored_original
        .iter()
        .position(|v| *v == corner)
        .expect("corner")];
    let (m, _) = measure_c(&tall, None);
    check_brep(&m, rect, moved, DIST, 9.5);
    let (_, version, catalog) = (0, base_of(&tall).1, inspect(&tall));
    assert_eq!(
        catalog["chamfers"][0]["distance_edit"]["max_distance_mm"],
        Value::Null
    );
    let request = r.root.path().join("d.json");
    write(&request, &json!({"request_version":1,"distance_mm":3.0}));
    let wide = r.root.path().join("wide.fcad");
    reply(
        edit_from(&tall, &id.to_string(), &version, &request, &wide)
            .output()
            .expect("process"),
        OPE,
        0,
    );
    assert_eq!(
        sketch_row(&wide, &sketch_id),
        row,
        "the Sketch row is byte-equal"
    );
    let (m, _) = measure_c(&wide, None);
    check_brep(&m, rect, moved, 3.0, 9.5);

    // Exact removal of the width, then every user constraint.
    let w = r.at_copy(&wide);
    let listed = w.constraint_row()["constraints"]
        .as_array()
        .expect("list")
        .clone();
    let length_rule = listed
        .iter()
        .find(|c| c["rule"]["kind"] == "distance" && c["rule"]["distance"] == 22.25)
        .expect("the replaced width")["constraint_id"]
        .clone();
    w.ask_constraints(std::slice::from_ref(&length_rule), &[]);
    let removed = w.root.path().join("removed.fcad");
    reply(
        w.constrain(&removed).output().expect("process"),
        CONSTRAIN,
        0,
    );
    let gone = inspect(&removed)["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("list")
        .clone();
    assert!(!gone.iter().any(|c| c["constraint_id"] == length_rule));
    assert_eq!(gone.len(), listed.len() - 1);
    let (m, _) = measure_c(&removed, None);
    let (rect_now, moved_now) = solved_rect(&removed, &stored_original, corner);
    check_brep(&m, rect_now, moved_now, 3.0, 9.5);

    // Every user constraint removed: only the closure links are left.
    let all = r.at_copy(&removed);
    let user: Vec<Value> = all.constraint_row()["constraints"]
        .as_array()
        .expect("list")
        .iter()
        .filter(|c| c["rule"]["kind"] != "coincident")
        .map(|c| c["constraint_id"].clone())
        .collect();
    assert!(!user.is_empty());
    all.ask_constraints(&user, &[]);
    let bare = all.root.path().join("bare.fcad");
    reply(
        all.constrain(&bare).output().expect("process"),
        CONSTRAIN,
        0,
    );
    let after = inspect(&bare);
    let left = after["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("list");
    assert_eq!(left.len(), 4, "the four closure links remain: {left:?}");
    assert!(left.iter().all(|c| c["rule"]["kind"] == "coincident"));
    assert_eq!(after["chamfers"][0]["profile_constrained"], false);
    assert_ne!(
        after["chamfers"][0]["distance_edit"]["max_distance_mm"],
        Value::Null,
        "the stored plate is the plate again"
    );
    // Nothing was baked: the stored coordinates are the saved ones.
    assert_eq!(r.at_copy(&bare).stored_starts(), stored_original);
    // The coordinate editor works again, on those stored coordinates.
    let c = r.at_copy(&bare);
    assert_eq!(after["sketches"][0]["editable"], true);
    assert_eq!(
        after["sketches"][0]["chamfer_base"]["profile_constrained"],
        false
    );
    let rect = [-10.125, 7.5, 52.25, 20.5];
    c.ask_rect(rect);
    let (sketch, version, _) = (c.sketch_id().to_owned(), c.version().to_owned(), 0);
    let moved_copy = c.root.path().join("moved.fcad");
    reply(
        c.redraw(&bare, &version, &moved_copy)
            .output()
            .expect("process"),
        crate::sketch::OP,
        0,
    );
    let _ = sketch;
    let (m, _) = measure_c(&moved_copy, None);
    check_brep(&m, rect, crate::sketch::mapped(rect, corner), 3.0, 9.5);
}

/// The distance bound is the SOLVED plate's, in both directions and exactly.
/// A stored side too short for a distance the solved plate allows publishes it;
/// a stored side long enough for a distance the solved plate refuses does not;
/// and on a solved plate the largest accepted distance (read from the very Lines
/// the rebuild solved) is accepted and the next float above it is refused.
#[test]
fn native_the_distance_bound_is_the_solved_plates_exactly() {
    if !solving() {
        return;
    }
    let corner = [X0 + W, Y0];
    // Stored 3.5 mm deep (largest stored distance 3.49), solved 20 mm deep.
    let small = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + 3.5], [X0, Y0 + 3.5]];
    let g = chamfered_without_kernel(Plate::new(small), corner, 3.0);
    assert_eq!(
        g.catalog["chamfers"][0]["distance_edit"]["max_distance_mm"],
        3.49
    );
    let at = [0.0, 0.0];
    g.ask_constraints(&[], &dimensioned(&g, at, 40.0, 20.0));
    let starts = g.stored_starts();
    let expected: Vec<_> = starts
        .iter()
        .map(|v| solved_vertex(&starts, *v, at, 40.0, 20.0))
        .collect();
    let first = published(&g, corner, 3.0, &expected, 0, "large");
    let solved_deep = g.at_copy(&first);
    assert_eq!(
        solved_deep.catalog["chamfers"][0]["distance_edit"]["max_distance_mm"],
        Value::Null,
        "the stored 3.49 is no bound"
    );
    let (rect, moved) = solved_rect(&first, &starts, corner);
    let id = solved_deep.catalog["chamfers"][0]["feature_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let request = g.root.path().join("d.json");
    let (_, version) = base_of(&first);
    let try_distance = |source: &Path, d: f64, name: &str| -> (PathBuf, Value) {
        write(&request, &json!({"request_version":1,"distance_mm":d}));
        let out = g.root.path().join(format!("{name}.fcad"));
        let (_, version) = base_of(source);
        let v = serde_json::from_slice::<Value>(
            &edit_from(source, &id, &version, &request, &out)
                .output()
                .expect("process")
                .stdout,
        )
        .expect("JSON");
        (out, v)
    };
    // 15 mm: far above the stored 3.49, inside the solved 19.99.
    let (fifteen, v) = try_distance(&first, 15.0, "fifteen");
    assert_eq!(v["ok"], true, "{v}");
    let (m, _) = measure_c(&fifteen, None);
    check_brep(&m, rect, moved, 15.0, H);
    // The largest distance, from the solved sides themselves, and the next float.
    let largest = solved_max_distance(&first);
    assert!(
        (largest - (rect[2].min(rect[3]) - 0.01)).abs() < 1e-9,
        "{largest}"
    );
    let (edge, v) = try_distance(&first, largest, "largest");
    assert_eq!(v["ok"], true, "{v}");
    let (m, _) = measure_c(&edge, None);
    check_brep(&m, rect, moved, largest, H);
    // The next float above the largest, exactly. A JSON number is not exact to
    // the last digit through the request reader, so the exact neighbours are
    // written through the shipped preparation and writer and judged by the real
    // solver's cold rebuild and the evaluator's own predicate.
    for (distance, fits) in [(largest, true), (largest.next_up(), false)] {
        let copy = g.root.path().join(format!("exact-{fits}.fcad"));
        std::fs::copy(&first, &copy).expect("copy");
        let mut d = Document::open(&copy).expect("writable");
        let prepared =
            ferritecad_document::prepare_chamfer_distance(&d, id.parse().expect("UUID"), distance)
                .expect("a finite distance is prepared on a constrained plate");
        d.write_chamfer_distance(&prepared).expect("written");
        d.close().expect("close");
        let d = Document::open_read_only(&copy).expect("reopen");
        let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
        let built = ferritecad_eval::rebuild_cold(&d, &mut k, &OperationContext::default());
        match (fits, built) {
            (true, Ok(built)) => built.release_all(&mut k),
            (false, Err(e)) => {
                let text = e.to_string();
                assert!(
                    text.contains(&id) && text.contains("does not fit the solved plate"),
                    "{text}"
                );
            }
            (fits, other) => panic!("{distance}: fits={fits}, got {:?}", other.map(|_| ())),
        }
        d.close().expect("close");
    }
    let names = entries(g.root.path());
    let (never, v) = try_distance(&first, largest + 1e-6, "never");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["error"]["kind"], "input", "{v}");
    let text = v.to_string();
    assert!(
        text.contains(&id) && text.contains("does not fit the solved plate"),
        "{text}"
    );
    assert!(!never.exists());
    assert_eq!(entries(g.root.path()), names, "a refusal left something");
    let _ = version;

    // Stored 12.25 mm deep (largest stored 12.24) with a distance of 5; solved
    // 4 mm deep: refused by the rebuild with the numbers, the source whole.
    let (_f, h, _, id) = plate_with_chamfer(CCW, corner, 5.0, "reverse-src");
    assert_eq!(
        h.catalog["chamfers"][0]["distance_edit"]["max_distance_mm"],
        D - 0.01
    );
    let before = std::fs::read(&h.source).expect("bytes");
    let names = entries(h.root.path());
    h.ask_constraints(&[], &dimensioned(&h, [0.0, 0.0], 30.0, 4.0));
    let never = h.root.path().join("never.fcad");
    let v = reply(h.constrain(&never).output().expect("process"), CONSTRAIN, 2);
    assert_eq!(refused(&v), "input", "{v}");
    let text = v.to_string();
    assert!(
        text.contains(&id.to_string()) && text.contains("does not fit the solved plate"),
        "{text}"
    );
    assert_eq!(entries(h.root.path()), names, "no destination or scratch");
    assert_eq!(std::fs::read(&h.source).expect("bytes"), before);
}

/// Real solver outcomes and refusals, each leaving the directory and the source
/// as they were: a solved plate whose sides turned, a plate that is no longer a
/// rectangle, a real conflict naming UUIDs, a stale version, an occupied
/// destination; and a real redundant constraint is reported by UUID while the
/// Chamfer stays.
#[test]
fn native_constraint_refusals_under_a_chamfer_are_atomic() {
    if !solving() {
        return;
    }
    let corner = [X0 + W, Y0];
    let (_f, f, _, id) = plate_with_chamfer(CCW, corner, 2.375, "src");
    let before = std::fs::read(&f.source).expect("bytes");
    let names = entries(f.root.path());
    let never = f.root.path().join("never.fcad");
    let refuse = |add: &[Value], kinds: &[&str], words: &[&str]| {
        f.ask_constraints(&[], add);
        let v = reply(f.constrain(&never).output().expect("process"), CONSTRAIN, 2);
        assert!(kinds.contains(&refused(&v)), "{v}");
        let text = v.to_string();
        for w in words {
            assert!(text.contains(w), "{w}: {v}");
        }
        assert_eq!(entries(f.root.path()), names, "a refusal left something");
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
        v
    };
    let chamfer = id.to_string();
    // 2.0 mm deep leaves less than 2.375 + 0.01 for the distance.
    refuse(
        &dimensioned(&f, [0., 0.], 30., 2.0),
        &["input"],
        &[&chamfer, "does not fit the solved plate"],
    );
    // The first Line's start pinned 5 mm beyond its own end, the sides held
    // H/V: a rectangle 5 mm wide whose first Line runs the other way, which is
    // not the corner that was chamfered.
    let starts = f.stored_starts();
    let across = horizontal(&starts);
    let mut add: Vec<Value> = (0..4)
        .map(|i| {
            rule(
                &f.line(i),
                if across[i] { "horizontal" } else { "vertical" },
            )
        })
        .collect();
    add.push(pin(&f.line(0), starts[1][0] + 5.0, starts[0][1]));
    refuse(&add, &["input"], &[&chamfer, "side"]);
    // One length and nothing to hold the sides: the plate slants.
    refuse(&[length(&f.line(0), 30.)], &["input", "unsupported"], &[]);
    // A real conflict: both horizontal sides dimensioned differently.
    let mut conflict = dimensioned(&f, [0., 0.], 30., 10.);
    let opposite = (1..4)
        .find(|i| across[*i])
        .expect("the other horizontal Line");
    conflict.push(length(&f.line(opposite), 20.));
    let v = refuse(&conflict, &["constraint"], &[]);
    let named = v["error"]["constraint_conflict"]["constraints"]
        .as_array()
        .expect("the conflicting constraints")
        .clone();
    assert!(
        !named.is_empty() && named.iter().all(|c| c["constraint_id"].is_string()),
        "{v}"
    );
    assert!(
        !v.to_string().contains(&chamfer),
        "a solver conflict is not the Chamfer's"
    );
    // Stale version and an occupied destination.
    f.ask_constraints(&[], &[length(&f.line(1), 11.5)]);
    let mut c = cli();
    c.arg(CONSTRAIN)
        .arg(&f.source)
        .arg("--sketch")
        .arg(f.plate_sketch_id())
        .arg("--expect-version")
        .arg("0".repeat(64))
        .arg("--request")
        .arg(&f.request)
        .arg("-o")
        .arg(&never)
        .arg("--json");
    assert_eq!(
        refused(&reply(c.output().expect("process"), CONSTRAIN, 2)),
        "input"
    );
    let taken = f.root.path().join("taken.fcad");
    std::fs::write(&taken, b"another process owns this").expect("occupied");
    let v = reply(f.constrain(&taken).output().expect("process"), CONSTRAIN, 2);
    assert_eq!(refused(&v), "input", "{v}");
    assert_eq!(
        std::fs::read(&taken).expect("kept"),
        b"another process owns this"
    );
    std::fs::remove_file(&taken).expect("tidy");
    assert_eq!(entries(f.root.path()), names);
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);

    // A real redundant constraint publishes and is reported by UUID; the
    // Chamfer is the saved one.
    let mut redundant: Vec<Value> = (0..4)
        .map(|i| {
            rule(
                &f.line(i),
                if across[i] { "horizontal" } else { "vertical" },
            )
        })
        .collect();
    let h = across.iter().position(|a| *a).expect("horizontal");
    let v = across.iter().position(|a| !*a).expect("vertical");
    redundant.extend([
        length(&f.line(h), 30.0),
        length(&f.line(v), 12.0),
        pin(&f.line(0), 2.0, 1.0),
        json!({"rule":"equal_length","a_curve_id":f.line(h),"b_curve_id":f.line(h)}),
    ]);
    redundant.pop();
    let opposite_h = (0..4)
        .filter(|i| across[*i])
        .nth(1)
        .expect("the other horizontal");
    redundant.push(
        json!({"rule":"equal_length","a_curve_id":f.line(h),"b_curve_id":f.line(opposite_h)}),
    );
    f.ask_constraints(&[], &redundant);
    let out = f.root.path().join("redundant.fcad");
    let done = reply(f.constrain(&out).output().expect("process"), CONSTRAIN, 0);
    let ids = done["result"]["solve"]["redundant_constraint_ids"]
        .as_array()
        .expect("typed list")
        .clone();
    assert_eq!(ids.len(), 1, "{done}");
    assert!(
        done["result"]["added_constraints"]
            .as_array()
            .expect("added")
            .iter()
            .any(|c| c["constraint_id"] == ids[0] && c["rule"]["kind"] == "equal_length"),
        "the solver names the equality two lengths already said: {done}"
    );
    assert_eq!(inspect(&out)["chamfers"][0]["feature_id"], json!(chamfer));
}

/// The strict reference rule of the shared job covers the constraint copy: an
/// extra saved base reference that resolves is kept; one that does not is never
/// published. Document-layer extras, real solver and kernel.
#[test]
fn native_constraint_copy_requires_every_saved_reference_to_resolve() {
    if !solving() {
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
        document.close().expect("close");
        let f = Fixture {
            catalog: inspect(&f.source),
            ..f
        };
        let before = std::fs::read(&f.source).expect("bytes");
        f.ask_constraints(&[], &dimensioned(&f, [1.0, 2.0], 31.0, 10.5));
        let names = entries(f.root.path());
        let out = f.root.path().join("constrained.fcad");
        let answer = reply(
            f.constrain(&out).output().expect("process"),
            CONSTRAIN,
            if unresolved { 2 } else { 0 },
        );
        if unresolved {
            assert_eq!(refused(&answer), "topology", "{answer}");
            assert!(!out.exists());
            assert_eq!(entries(f.root.path()), names, "scratch leaked");
        } else {
            let was = stored_refs(&f.source);
            assert_eq!(stored_refs(&out), was, "the extra reference is kept");
            only_this_sketch_changed(&f.source, &out, f.plate_sketch_id());
        }
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    }
}

/// Discovery on a chamfered plate whose Sketch carries constraints, and the
/// protocol, with no kernel or solver needed: the stored corner is named as
/// stored, the bound is deferred, the coordinate editor refuses with the
/// Chamfer named, the constraint editor offers the Sketch, and a build without
/// the kernel or the solver refuses a well-formed request before writing anything.
#[test]
fn chamfer_constraint_discovery_and_protocol_without_native() {
    let plain = chamfered_without_kernel(Plate::new(CCW), [X0 + W, Y0], 2.375);
    let row = plain.constraint_row();
    assert_eq!(row["available"], true, "{row}");
    assert_eq!(row["chamfer_base"]["corner_mm"], json!([X0 + W, Y0]));
    assert_eq!(
        row["chamfer_base"]["chamfer_feature_id"],
        plain.catalog["chamfers"][0]["feature_id"]
    );
    assert_eq!(row["chamfer_base"]["profile_constrained"], false);
    assert_eq!(row["fillet_base"], Value::Null);
    assert_eq!(plain.catalog["chamfers"][0]["profile_constrained"], false);
    assert_eq!(
        plain.catalog["chamfers"][0]["distance_edit"]["max_distance_mm"],
        D - 0.01
    );

    // Constraints written by the shipped preparation and writer, no kernel.
    let mut d = Document::open(&plain.source).expect("writable");
    let sketch: ObjectId = plain.plate_sketch_id().parse().expect("UUID");
    let line = |i: usize| -> ferritecad_types::StableEntityId {
        plain.line(i).as_str().expect("UUID").parse().expect("UUID")
    };
    let edits = ferritecad_document::SketchConstraintEdits {
        remove: Vec::new(),
        add: vec![ferritecad_document::AddSketchConstraint::Line(
            ferritecad_document::AddLineConstraint::Line {
                curve: line(0),
                kind: ferritecad_document::LineConstraintKind::Horizontal,
            },
        )],
    };
    let prepared =
        ferritecad_document::prepare_sketch_constraints(&d, sketch, &edits).expect("prepared");
    assert_eq!(prepared.chamfer().map(|c| c.constrained), Some(false));
    d.write_sketch_constraints(&prepared).expect("written");
    d.close().expect("close");
    let f = Fixture {
        catalog: inspect(&plain.source),
        ..plain
    };
    let row = f.constraint_row();
    assert_eq!(row["available"], true);
    assert_eq!(
        row["constraints"].as_array().expect("list").len(),
        5,
        "{row}"
    );
    let chamfer = &f.catalog["chamfers"][0];
    assert_eq!(chamfer["profile_constrained"], true);
    assert_eq!(
        chamfer["corner_mm"],
        json!([X0 + W, Y0]),
        "the stored corner"
    );
    assert_eq!(chamfer["distance_edit"]["available"], true);
    assert_eq!(chamfer["distance_edit"]["max_distance_mm"], Value::Null);
    assert_eq!(row["chamfer_base"]["profile_constrained"], true);
    assert_eq!(
        f.catalog["features"][0]["chamfer_base"]["profile_constrained"],
        true
    );
    assert_eq!(f.catalog["edit_extrude"]["available"], true, "§29B");
    let sketch_row = &f.catalog["sketches"][0];
    assert_eq!(
        sketch_row["editable"], false,
        "coordinates of a constrained Sketch"
    );
    let reason = sketch_row["refusal"].as_str().expect("reason");
    assert!(
        reason.contains(chamfer["feature_id"].as_str().expect("id"))
            && reason.contains("constraint editor"),
        "{reason}"
    );
    assert_eq!(sketch_row["chamfer_base"], Value::Null);
    // Creating a Chamfer on a constrained source stays refused, naming the
    // constraint's family.
    assert_eq!(f.catalog["bodies"][0]["chamfer_edge"]["available"], false);

    // The protocol. Without a kernel the kernel is asked for after the request
    // is read; with a kernel and no solver the solve is refused; with both,
    // this example publishes (covered natively).
    let before = std::fs::read(&f.source).expect("bytes");
    let never = f.root.path().join("never.fcad");
    f.ask_constraints(&[], &[length(&f.line(1), 11.5)]);
    let names = entries(f.root.path());
    let solver = ferritecad_occt::is_available() && ferritecad_eval::solver_available();
    if !solver {
        let v = reply(f.constrain(&never).output().expect("process"), CONSTRAIN, 2);
        assert_eq!(refused(&v), "unsupported", "{v}");
        if !ferritecad_occt::is_available() {
            assert!(v.to_string().contains("Open CASCADE"), "{v}");
        }
        assert_eq!(entries(f.root.path()), names, "a refusal left something");
        assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
    }
    // A malformed request is refused before any kernel is asked for.
    write(
        &f.request,
        &json!({"request_version": 1, "remove": [], "add": [], "distance_mm": 1}),
    );
    let v = reply(f.constrain(&never).output().expect("process"), CONSTRAIN, 2);
    assert_eq!(refused(&v), "input", "{v}");
    assert_eq!(entries(f.root.path()), names);
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
}

/// OCCT without PlaneGCS: the unconstrained chamfered plate still builds, free
/// and closure-only routes work, and a constraint copy is refused, typed,
/// publishing nothing. N/A where a solver is linked: the no-solver CI step runs
/// this by exact name and fails on any `skipped:`.
#[test]
fn occt_without_solver_refuses_a_constrained_chamfered_plate() {
    if !native() {
        return;
    }
    if ferritecad_eval::solver_available() {
        eprintln!("skipped: the mixed gate needs a build without PlaneGCS");
        return;
    }
    let corner = [X0 + W, Y0];
    let (_f, f, copy, _) = plate_with_chamfer(CCW, corner, 2.375, "src");
    let (m, _) = measure_c(&copy, None);
    check_brep(&m, RECT, corner, 2.375, H);
    // The free routes of §29A–§29C are unchanged without a solver.
    let (base, version) = base_of(&copy);
    let tall = f.root.path().join("tall.fcad");
    reply(
        super::base_height::raise(&copy, &base, "9.5", &version, &tall)
            .output()
            .expect("process"),
        "edit-extrude",
        0,
    );
    let rect = [-10.125, 7.5, 52.25, 20.5];
    f.ask_rect(rect);
    let sketched = f.root.path().join("sketched.fcad");
    reply(
        f.redraw(&copy, f.version(), &sketched)
            .output()
            .expect("process"),
        crate::sketch::OP,
        0,
    );
    let before = std::fs::read(&f.source).expect("bytes");
    let names = entries(f.root.path());
    f.ask_constraints(&[], &dimensioned(&f, [1.5, 2.25], 30.25, 9.5));
    let never = f.root.path().join("never.fcad");
    let v = reply(f.constrain(&never).output().expect("process"), CONSTRAIN, 2);
    assert_eq!(refused(&v), "unsupported", "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .expect("message")
            .contains("constraint"),
        "{v}"
    );
    assert_eq!(entries(f.root.path()), names, "no destination or scratch");
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
}
