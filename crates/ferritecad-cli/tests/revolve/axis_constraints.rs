// SPDX-License-Identifier: MIT
//! §27H: dimensional constraints on the saved profile of a solid Revolve —
//! closed on the axis along its stated Line — through the existing
//! `edit`, and measured after the fact.
//!
//! Nothing is snapped to the axis: a solution keeps the stated Line on it
//! only because the request pins one of its ends at X 0 and keeps it
//! vertical. Each copy is read back cold and through a shared cache, and the
//! Body is measured against the solved Lines alone by the §27G measure, which
//! also requires both ends of the stated Line at exactly 0.0 and no face and
//! no name for it.
use super::angle::{angle_edit, angle_reply, refs, revolve_payload, target, write_angle};
use super::axis::write_solid_document;
use super::constraints::{
    adds, assert_solved, check_constraint_cells, closure, constrain, constraint_ids, create_full,
    create_sector, curve, distance, edit_reply, exports, fixed, horizontal, measure_solved, pair,
    publish, refuse, rewrite_revolve, rule, sketch_of, solver, stored_id,
};
use super::partial::{doc_sketch, pappus};
use super::*;

/// A solid cylinder off the origin in Y, with fractional sizes. Line 3 is on
/// the axis.
const CYLINDER_T: [[f64; 2]; 4] = [[0., -1.5], [7.25, -1.5], [7.25, 13.75], [0., 13.75]];
/// The same cylinder once its bottom Line is dimensioned 8.5 mm.
const CYLINDER_WIDE: [[f64; 2]; 4] = [[0., -1.5], [8.5, -1.5], [8.5, 13.75], [0., 13.75]];
/// A solid cone; Line 2 is on the axis.
const CONE_T: [[f64; 2]; 3] = [[0., -1.5], [8.5, -1.5], [0., 13.25]];
/// The same cone with a 9.75 mm base and 15.5 mm tall.
const CONE_WIDE: [[f64; 2]; 3] = [[0., -1.5], [9.75, -1.5], [0., 14.]];
/// A stepped shaft turned through a fractional angle; Line 5 is on the axis.
const SHAFT_T: [[f64; 2]; 6] = [
    [0., -2.25],
    [8., -2.25],
    [8., 3.5],
    [5.5, 3.5],
    [5.5, 12.75],
    [0., 12.75],
];
/// The same shaft once its base is dimensioned 9.25 mm: the step keeps its
/// 2.5 mm, so the upper diameter follows.
const SHAFT_WIDE: [[f64; 2]; 6] = [
    [0., -2.25],
    [9.25, -2.25],
    [9.25, 3.5],
    [6.75, 3.5],
    [6.75, 12.75],
    [0., 12.75],
];
const SHAFT_DEG: f64 = 137.5;

/// The saved Revolve's stated axis Line, as discovery and the constraint
/// editor both report it.
fn stated_axis(catalog: &Value, index: usize) -> Value {
    let _edit = &catalog["sketches"][0];
    let axis = curve(catalog, index);

    assert_eq!(catalog["revolves"][0]["axis_curve_id"], axis);
    axis
}

/// A solid cylinder: dimensioned rigid with the axis pinned, widened by
/// replacing one length, refused wherever a solution leaves the axis or the
/// class, and stripped back to its closure. A cone follows, with both of its
/// sizes replaced in one request.
#[test]
fn native_axis_closed_cylinder_and_cone_dimension_replace_remove_and_refuse() {
    if !solver() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let root = d.path();
    let source = create_full(root, &CYLINDER_T, "cylinder");
    let request = root.join("constraints.json");
    let (sketch, stored) = sketch_of(&source);
    let catalog = inspect(&source);
    let _edit = &catalog["sketches"][0];

    let axis = stated_axis(&catalog, 3);
    let source_refs = refs(&source);
    assert_eq!(
        crate::names::face_count(&source_refs),
        3,
        "no name for the axis Line"
    );
    let source_bytes = std::fs::read(&source).expect("source");

    // Rigid: H/V everywhere, the axis Line's lower end pinned at X 0, both
    // sizes as stored. The solver keeps the axis ends at exactly 0.0.
    let rigid = root.join("rigid.fcad");
    let published = publish(
        &source,
        &request,
        &adds(
            &catalog,
            vec![
                rule(&catalog, 0, "horizontal"),
                rule(&catalog, 1, "vertical"),
                rule(&catalog, 2, "horizontal"),
                rule(&catalog, 3, "vertical"),
                fixed(&catalog, 3, "end", 0., -1.5),
                distance(&catalog, 0, 7.25),
                distance(&catalog, 1, 15.25),
            ],
        ),
        &rigid,
    );
    assert_eq!(published["solve"]["degrees_of_freedom"], 0);
    assert_eq!(published["solve"]["redundant_constraint_ids"], json!([]));
    check_constraint_cells(&source, &rigid, sketch);
    assert_eq!(sketch_of(&rigid).1.curves, stored.curves, "stored inputs");
    assert_eq!(refs(&rigid), source_refs);
    assert_eq!(revolve_payload(&rigid), revolve_payload(&source));
    assert_eq!(stated_axis(&inspect(&rigid), 3), axis);
    let cache = root.join("shared.fcad-cache");
    let solved = measure_solved(&rigid, None, Some((&cache, &[CacheOutcome::Miss])));
    assert_eq!((solved.dof, solved.redundant.len()), (0, 0));
    assert_solved(&solved, &CYLINDER_T);
    let cylinder = |r: f64, h: f64| PI * r * r * h;
    assert!((solved.volume - cylinder(7.25, 15.25)).abs() < 1e-7 * solved.volume);
    measure_solved(&rigid, None, Some((&cache, &[CacheOutcome::Hit])));

    // Replace the radius: exact old UUID out, one new one in.
    let catalog = inspect(&rigid);
    let old_width = stored_id(&catalog, "distance", 0);
    let wide = root.join("wide.fcad");
    let replaced = publish(
        &rigid,
        &request,
        &transaction::constraints(
            &catalog["sketches"][0],
            std::slice::from_ref(&old_width),
            &[distance(&catalog, 0, 8.5)],
        ),
        &wide,
    );
    assert_eq!(replaced["deleted"], json!([old_width]));
    assert_eq!(replaced["solve"]["degrees_of_freedom"], 0);
    let mut kept = constraint_ids(&catalog);
    kept.retain(|id| *id != old_width);
    kept.push(replaced["inserted"][0]["constraint_id"].clone());
    assert_eq!(constraint_ids(&inspect(&wide)), kept);
    check_constraint_cells(&rigid, &wide, sketch);
    assert_eq!(sketch_of(&wide).1.curves, stored.curves);
    assert_eq!(refs(&wide), source_refs);
    let grown = measure_solved(&wide, None, Some((&cache, &[CacheOutcome::Miss])));
    measure_solved(&wide, None, Some((&cache, &[CacheOutcome::Hit])));
    let cold = measure_solved(&wide, None, None);
    assert_eq!(cold.lines, grown.lines, "cold and cached solve alike");
    assert_solved(&grown, &CYLINDER_WIDE);
    assert!((grown.volume - cylinder(8.5, 15.25)).abs() < 1e-7 * grown.volume);
    exports(&wide, &grown, None, "axis-constraints-cylinder");

    // Solutions outside the class refuse publication, whatever the solver's
    // residual; nothing is written and nothing is snapped.
    let refused = root.join("refused.fcad");
    let pin = stored_id(&catalog, "fixed", 3);
    for (why, edits) in [
        (
            "no pin: the radius change drags the axis Line off X 0",
            transaction::constraints(
                &catalog["sketches"][0],
                &[pin.clone(), old_width.clone()],
                &[distance(&catalog, 0, 8.5)],
            ),
        ),
        (
            "only the outer corner pinned",
            transaction::constraints(
                &catalog["sketches"][0],
                &[pin.clone(), old_width.clone()],
                &[
                    fixed(&catalog, 1, "start", 7.25, -1.5),
                    distance(&catalog, 0, 9.75),
                ],
            ),
        ),
        (
            "the axis Line moved away from the axis",
            transaction::constraints(
                &catalog["sketches"][0],
                std::slice::from_ref(&pin),
                &[fixed(&catalog, 3, "end", 0.5, -1.5)],
            ),
        ),
        (
            "the axis Line crosses the axis",
            transaction::constraints(
                &catalog["sketches"][0],
                std::slice::from_ref(&pin),
                &[fixed(&catalog, 3, "end", -1., -1.5)],
            ),
        ),
    ] {
        let error = refuse(&rigid, &request, &edits, &refused);
        let message = error["message"].as_str().expect("message");
        assert!(message.contains("axis"), "{why}: {error}");
        assert!(error.get("constraint_conflict").is_none(), "{why}");
    }
    let conflict = refuse(
        &rigid,
        &request,
        &adds(&catalog, vec![pair(&catalog, "equal_length", 0, 1)]),
        &refused,
    );
    assert_eq!(conflict["kind"], "constraint", "{conflict}");
    assert!(
        conflict["constraint_conflict"]["constraints"]
            .as_array()
            .expect("typed conflict")
            .iter()
            .any(|c| c["rule"]["kind"] == "equal_length")
    );
    let catalog = inspect(&source);
    let collapsed = refuse(
        &source,
        &request,
        &adds(&catalog, vec![fixed(&catalog, 0, "end", 0., -1.5)]),
        &refused,
    );
    let message = collapsed["message"].as_str().expect("message");
    assert!(
        ["zero-length", "left over", "two distinct endpoints", "axis"]
            .iter()
            .any(|w| message.contains(w)),
        "{collapsed}"
    );

    // Every user constraint removed: the closure and the requirement stay,
    // coordinates stay refused, the solver starts from the stored inputs.
    let catalog = inspect(&wide);
    let user: Vec<Value> = catalog["sketches"][0]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .filter(|c| c["rule"]["kind"] != "coincident")
        .map(|c| c["constraint_id"].clone())
        .collect();
    assert_eq!(user.len(), 7);
    let bare = root.join("bare.fcad");
    let removed = publish(
        &wide,
        &request,
        &transaction::constraints(&catalog["sketches"][0], &user, &[]),
        &bare,
    );
    assert_eq!(removed["solve"]["degrees_of_freedom"], 8);
    let row = inspect(&bare)["sketches"][0].clone();
    let left = row["constraints"].as_array().expect("closure");
    assert_eq!(left.len(), 4);
    assert!(left.iter().all(|c| c["rule"]["kind"] == "coincident"));

    check_constraint_cells(&source, &bare, sketch);
    let free = measure_solved(&bare, None, None);
    assert_eq!(free.dof, 8);
    assert_solved(&free, &CYLINDER_T);
    assert_eq!(std::fs::read(&source).expect("source"), source_bytes);

    // A cone: the apex is on the axis Line; both sizes replaced at once.
    let cone = create_full(root, &CONE_T, "cone");
    let (cone_sketch, _) = sketch_of(&cone);
    let catalog = inspect(&cone);
    stated_axis(&catalog, 2);
    let cone_refs = refs(&cone);
    let pinned = root.join("cone-rigid.fcad");
    let published = publish(
        &cone,
        &request,
        &adds(
            &catalog,
            vec![
                rule(&catalog, 0, "horizontal"),
                rule(&catalog, 2, "vertical"),
                fixed(&catalog, 0, "start", 0., -1.5),
                distance(&catalog, 0, 8.5),
                distance(&catalog, 2, 14.75),
            ],
        ),
        &pinned,
    );
    assert_eq!(published["solve"]["degrees_of_freedom"], 0);
    assert_solved(&measure_solved(&pinned, None, None), &CONE_T);
    let catalog = inspect(&pinned);
    let (base, height) = (
        stored_id(&catalog, "distance", 0),
        stored_id(&catalog, "distance", 2),
    );
    let taller = root.join("cone-wide.fcad");
    publish(
        &pinned,
        &request,
        &transaction::constraints(
            &catalog["sketches"][0],
            &[base, height],
            &[distance(&catalog, 0, 9.75), distance(&catalog, 2, 15.5)],
        ),
        &taller,
    );
    check_constraint_cells(&pinned, &taller, cone_sketch);
    assert_eq!(refs(&taller), cone_refs);
    let grown = measure_solved(&taller, None, None);
    assert_eq!(grown.dof, 0);
    assert_solved(&grown, &CONE_WIDE);
    let exact = PI * 9.75 * 9.75 * 15.5 / 3.;
    assert!((grown.volume - exact).abs() < 1e-7 * exact);
    exports(&taller, &grown, None, "axis-constraints-cone");
}

/// A solid stepped shaft turned through 137.5°: dimensioned rigid, its base
/// widened so the upper step follows, both end faces measured against the
/// solved profile under their saved names; the angle then edited with every
/// constraint kept.
#[test]
fn native_axis_closed_sector_caps_names_angle_edit_and_exports() {
    if !solver() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let root = d.path();
    let source = create_sector(root, &SHAFT_T, SHAFT_DEG, "shaft");
    let request = root.join("constraints.json");
    let (sketch, stored) = sketch_of(&source);
    let catalog = inspect(&source);
    let feature = &catalog["revolves"][0];
    assert_eq!(feature["angle_deg"], SHAFT_DEG);
    stated_axis(&catalog, 5);
    let source_refs = refs(&source);
    assert_eq!(
        crate::names::face_count(&source_refs),
        5 + 2,
        "five faces and two caps"
    );

    let rigid = root.join("rigid.fcad");
    let published = publish(
        &source,
        &request,
        &adds(
            &catalog,
            vec![
                rule(&catalog, 0, "horizontal"),
                rule(&catalog, 1, "vertical"),
                rule(&catalog, 2, "horizontal"),
                rule(&catalog, 3, "vertical"),
                rule(&catalog, 4, "horizontal"),
                rule(&catalog, 5, "vertical"),
                fixed(&catalog, 0, "start", 0., -2.25),
                distance(&catalog, 0, 8.),
                distance(&catalog, 1, 5.75),
                distance(&catalog, 2, 2.5),
                distance(&catalog, 3, 9.25),
            ],
        ),
        &rigid,
    );
    assert_eq!(published["solve"]["degrees_of_freedom"], 0);
    assert_eq!(published["solve"]["redundant_constraint_ids"], json!([]));
    check_constraint_cells(&source, &rigid, sketch);
    assert_eq!(refs(&rigid), source_refs);
    let cache = root.join("shared.fcad-cache");
    let solved = measure_solved(
        &rigid,
        Some(SHAFT_DEG),
        Some((&cache, &[CacheOutcome::Miss])),
    );
    assert_solved(&solved, &SHAFT_T);

    let catalog = inspect(&rigid);
    let old_base = stored_id(&catalog, "distance", 0);
    let wide = root.join("wide.fcad");
    let replaced = publish(
        &rigid,
        &request,
        &transaction::constraints(
            &catalog["sketches"][0],
            &[old_base],
            &[distance(&catalog, 0, 9.25)],
        ),
        &wide,
    );
    assert_eq!(replaced["solve"]["degrees_of_freedom"], 0);
    check_constraint_cells(&rigid, &wide, sketch);
    assert_eq!(sketch_of(&wide).1.curves, stored.curves);
    assert_eq!(refs(&wide), source_refs, "both caps and every face name");
    assert_eq!(
        revolve_payload(&wide),
        revolve_payload(&source),
        "angle kept"
    );
    let grown = measure_solved(
        &wide,
        Some(SHAFT_DEG),
        Some((&cache, &[CacheOutcome::Miss])),
    );
    let cached = measure_solved(&wide, Some(SHAFT_DEG), Some((&cache, &[CacheOutcome::Hit])));
    assert_eq!(cached.lines, grown.lines);
    assert_solved(&grown, &SHAFT_WIDE);
    let exact = pappus(&SHAFT_WIDE) * SHAFT_DEG / 360.;
    assert!((grown.volume - exact).abs() < 1e-7 * exact);
    exports(&wide, &grown, Some(SHAFT_DEG), "axis-constraints-sector");

    // The angle edit writes only the Revolve row and keeps the constraints.
    let (revolve, version, _) = target(&wide);

    let angle_request = root.join("angle.json");
    write_angle(&angle_request, 212.25);
    let turned = root.join("turned.fcad");
    angle_reply(
        angle_edit(&wide, &revolve, &version, &angle_request, &turned)
            .output()
            .expect("angle"),
        0,
    );
    assert_eq!(
        sketch_of(&turned).1,
        sketch_of(&wide).1,
        "the Sketch is kept"
    );
    assert_eq!(refs(&turned), source_refs);
    let further = measure_solved(&turned, Some(212.25), None);
    assert_eq!(further.dof, 0);
    assert_solved(&further, &SHAFT_WIDE);

    // Pulled off the axis: refused, the constrained copy untouched.
    let catalog = inspect(&wide);
    let pin = stored_id(&catalog, "fixed", 0);
    let error = refuse(
        &wide,
        &request,
        &transaction::constraints(
            &catalog["sketches"][0],
            &[pin],
            &[fixed(&catalog, 0, "start", 0.75, -2.25)],
        ),
        &root.join("refused.fcad"),
    );
    assert!(
        error["message"].as_str().expect("message").contains("axis"),
        "{error}"
    );
}

/// Discovery, the writer and the refusals a build reaches without a solver,
/// in any build: axis-closed profiles, full and partial, are offered with
/// their stated Line; forged ones whose stored geometry disagrees with it are
/// refused by the shared class rule.
#[test]
fn axis_closed_constraint_discovery_writer_and_refusals_without_solver() {
    let d = tempfile::tempdir().expect("dir");
    let root = d.path();
    let solid = root.join("solid.fcad");
    let labels = write_solid_document(&solid, &CYLINDER_T, Some(3));
    let solid_sector = root.join("solid-sector.fcad");
    let sector_labels = write_solid_document(&solid_sector, &CYLINDER_T, Some(3));
    rewrite_revolve(&solid_sector, |r| {
        r.extent = RevolveExtent::Partial {
            angle: ferritecad_document::Expression::constant(f64::to_radians(SHAFT_DEG))
                .expect("angle"),
        }
    });
    for (path, _kind, _axis) in [
        (&solid, "full_turn_revolve_axis_closed", labels[3]),
        (
            &solid_sector,
            "partial_turn_revolve_axis_closed",
            sector_labels[3],
        ),
    ] {
        let c = inspect(path);
        let row = &c["sketches"][0];
        let edit = &row;

        assert_eq!(edit["refusal"], Value::Null);
        assert_eq!(edit["constraints"], json!([]));
        assert_eq!(edit["curves"].as_array().expect("curves").len(), 4);

        // The unconstrained part keeps its coordinate editor.
    }

    // The writer, with no solver or kernel: the stated Line travels in the
    // prepared use, and exactly the allowlisted cells are written.
    let written = root.join("written.fcad");
    std::fs::copy(&solid, &written).expect("copy");
    let sketch_id = doc_sketch(&solid);
    let pin = |x: f64, y: f64| {
        ferritecad_document::AddSketchConstraint::Line(
            ferritecad_document::AddLineConstraint::Line {
                curve: labels[3],
                kind: ferritecad_document::LineConstraintKind::Fixed {
                    at: ferritecad_document::LineEndpoint::End,
                    x: ferritecad_document::SketchCoordinateMm::new(x).expect("x"),
                    y: ferritecad_document::SketchCoordinateMm::new(y).expect("y"),
                },
            },
        )
    };
    let edits = ferritecad_document::SketchConstraintEdits {
        remove: vec![],
        add: vec![
            ferritecad_document::AddSketchConstraint::Line(
                ferritecad_document::AddLineConstraint::Line {
                    curve: labels[3],
                    kind: ferritecad_document::LineConstraintKind::Vertical,
                },
            ),
            pin(0., -1.5),
        ],
    };
    let source_doc = Document::open_read_only(&solid).expect("open");
    let prepared =
        ferritecad_fixtures::constraint_changes(&source_doc, sketch_id, &edits).expect("prepared");
    source_doc.close().expect("close");

    assert_eq!(
        ferritecad_fixtures::added_constraints(&prepared).len(),
        4 + 2
    );
    let mut doc = Document::open(&written).expect("open");
    doc.write_changes(&prepared).expect("honest write");
    doc.close().expect("close");
    check_constraint_cells(&solid, &written, sketch_id);
    assert_eq!(revolve_payload(&written), revolve_payload(&solid));
    if solver() {
        assert_eq!(refs(&written), refs(&solid));
    }

    // Constrained, the other editors keep their deliberate policies:
    // coordinates refused, the constraint editor and the Revolve profile
    // discovery offered, the angle of a sector offered.
    let c = inspect(&written);
    let _row = &c["sketches"][0];

    let constrained_sector = root.join("constrained-sector.fcad");
    std::fs::copy(&solid_sector, &constrained_sector).expect("copy");
    super::edit::rewrite_sketch(&constrained_sector, |s| {
        s.constraints.extend(closure(&sector_labels));
        s.constraints.push(horizontal(sector_labels[0]));
    });
    let (_, _, _discovered) = target(&constrained_sector);

    if let Some(dir) = std::env::var_os("FCAD_AXIS_CONSTRAINT_FIXTURES") {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).expect("fixtures");
        for path in [&solid, &solid_sector, &written, &constrained_sector] {
            std::fs::copy(path, dir.join(path.file_name().expect("name"))).expect("fixture");
        }
    }
}

/// With Open CASCADE but no solver: the solid class is offered, a copy is
/// refused with the typed solver error and nothing is published.
#[test]
fn occt_without_solver_refuses_axis_closed_constraints_honestly() {
    if !native() {
        return;
    }
    if cfg!(feature = "planegcs") {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let root = d.path();
    for source in [
        create_full(root, &CYLINDER_T, "cylinder"),
        create_sector(root, &SHAFT_T, SHAFT_DEG, "shaft"),
    ] {
        let catalog = inspect(&source);

        let before = std::fs::read(&source).expect("source");
        let request = root.join("request.json");
        let out = root.join("copy.fcad");
        write(
            &request,
            &adds(&catalog, vec![rule(&catalog, 0, "horizontal")]),
        );
        let names = entries(root);
        let v = edit_reply(
            constrain(&source, &catalog, &request, &out)
                .output()
                .expect("no solver"),
            2,
        );
        assert_eq!(v["error"]["kind"], "unsupported", "{v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .expect("message")
                .contains("planegcs"),
            "{v}"
        );
        assert!(!out.exists());
        assert_eq!(entries(root), names);
        assert_eq!(std::fs::read(&source).expect("source"), before);
    }
}
