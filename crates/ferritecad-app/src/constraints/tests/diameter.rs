// SPDX-License-Identifier: MIT
//! §31E: a diameter typed for a circle, through the form's real widgets.
//!
//! A diameter is not a constraint of its own. It is the Radius `D/2`, in the
//! Radius slot, as one history step and one request, so these gates read the
//! draft, the request and the published copy, never a separate "diameter" state.
//! The widget gates need no kernel and no solver (the circle documents are
//! written through the document API, as the command-line gates do); the one
//! native gate needs both.

use super::*;
use ferritecad_document::{
    Body, DatumPlane, Dependency, DependencyRole, EndCondition, Expression, Extrude, ObjectPayload,
    Point2, SketchCurve, SolidOperation,
};
use ferritecad_types::Transform;

/// A §25J cylinder (one radius) or a §25L ring (two), written without a kernel
/// so the form can be driven in a build that has none. The circles are stored in
/// the order of `radii` and share the centre (12, -7); their UUIDs come back in
/// that order.
pub(crate) fn write_round_profile(path: &Path, radii: &[f64]) -> Vec<StableEntityId> {
    let mut d = Document::create(path).expect("document");
    let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
    let ids: Vec<_> = radii.iter().map(|_| StableEntityId::new()).collect();
    d.write(|w| {
        w.put_object(
            plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            }),
        )?;
        let mut curves = Vec::new();
        for (id, radius) in ids.iter().zip(radii) {
            curves.push(SketchCurve {
                id: *id,
                construction: false,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(12., -7.)?,
                    radius: *radius,
                },
            });
        }
        w.put_object(
            sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves,
                constraints: Vec::new(),
            }),
        )?;
        w.put_object(
            extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                regions: Vec::new(),
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(15.)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                previous: None,
            }),
        )?;
        w.put_object(
            body,
            None,
            3,
            Some("Body"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(extrude),
            }),
        )?;
        for (dependent, dependency, role) in [
            (sketch, plane, DependencyRole::Plane),
            (extrude, sketch, DependencyRole::Profile),
            (body, extrude, DependencyRole::BodyTip),
        ] {
            w.add_dependency(Dependency {
                dependent,
                dependency,
                role,
            })?;
        }
        Ok(())
    })
    .expect("round profile");
    d.close().expect("close");
    ids
}

pub(crate) fn round_fixture(
    radii: &[f64],
) -> (
    tempfile::TempDir,
    PathBuf,
    ExtrudeEditSource,
    Vec<StableEntityId>,
) {
    let root = tempfile::tempdir().expect("dir");
    let path = root.path().join("round.fcad");
    let ids = write_round_profile(&path, radii);
    let d = Document::open_read_only(&path).expect("doc");
    let source = crate::sketch_facts::Source::read(&d).expect("snapshot");
    d.close().expect("close");
    assert!(source.constraint_sketches[0].refusal.is_none());
    (root, path, source, ids)
}

fn radius_of(curve: StableEntityId, mm: f64) -> AddSketchConstraint {
    AddSketchConstraint::Circle {
        curve,
        kind: CircleConstraintKind::Radius(CircleRadiusMm::new(mm).expect("a radius")),
    }
}

/// The right edge of the widest painted text for which `pick` holds.
fn right_edge(out: &egui::FullOutput, pick: impl Fn(&str) -> bool) -> f32 {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if pick(t.galley.text()) => Some(t.visual_bounding_rect().right()),
            _ => None,
        })
        .fold(f32::MIN, f32::max)
}

fn opened(path: &Path, source: &ExtrudeEditSource) -> (Editor, egui::Context) {
    let mut e = Editor::default();
    assert!(e.begin(path, source, source.constraint_sketches[0].sketch));
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    (e, ctx)
}

/// A circle picked and a diameter typed through the widgets, optionally added.
/// For the window's tab gate: the form as a person leaves it.
pub(crate) fn typed_diameter(
    path: &Path,
    source: &ExtrudeEditSource,
    text: &str,
    add: bool,
) -> Editor {
    let (mut e, ctx) = opened(path, source);
    let curve = source.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .curves[0]
        .id;
    click(&ctx, &mut e, &format!("Circle · {}", short(curve)));
    enter_field(&ctx, &mut e, "Diameter mm", text, false);
    if add {
        click(&ctx, &mut e, "Add diameter");
    }
    e
}

/// A diameter is the half radius, in the Radius slot, as one step and one
/// request; text that is no diameter stays in the box and changes nothing.
#[test]
fn a_circle_diameter_is_the_half_radius_in_one_step_through_the_real_widgets() {
    let (_root, path, source, ids) = round_fixture(&[10.]);
    let (curve, sketch) = (ids[0], source.constraint_sketches[0].sketch);
    let (mut e, ctx) = opened(&path, &source);

    // The row is there, below the radius row, and widens nothing: some text that
    // was already in the form reaches at least as far right as the new row does.
    let out = frame(&ctx, &mut e, vec![]);
    for label in [
        "Radius mm",
        "Add radius",
        "Add Fixed centre",
        "Fixed X (mm):",
        "Diameter mm",
        "Add diameter",
        "Undo",
        "Redo",
        "Cancel constraints draft",
        "Clear pending changes",
    ] {
        assert!(painted(&out, label), "{label} is not painted");
    }
    let new_row =
        |t: &str| matches!(t, "Diameter mm" | "Add diameter") || t.starts_with("Stored as Radius");
    assert!(painted(&out, "Stored as Radius = Diameter / 2"));
    let (newest, oldest) = (right_edge(&out, new_row), right_edge(&out, |t| !new_row(t)));
    println!("Diameter row right edge {newest} pt, rest of the form {oldest} pt");
    assert!(
        newest <= oldest,
        "the Diameter row ({newest}) reaches past the rest of the form ({oldest})"
    );

    // Nothing is picked: nothing is added.
    click(&ctx, &mut e, "Add diameter");
    assert!(e.draft.as_ref().expect("draft").edits.add.is_empty());
    click(&ctx, &mut e, &format!("Circle · {}", short(curve)));
    assert_eq!(e.draft.as_ref().expect("draft").selected, Some(curve));

    // Text that is no diameter: refused in words, kept exactly as typed, no step,
    // and the picked circle and the radius box are as they were.
    for bad in [
        "", "0", "-0", "-4", "banana", "13,5", "NaN", "inf", "1e999", "5e-324",
    ] {
        enter_field(&ctx, &mut e, "Diameter mm", bad, false);
        click(&ctx, &mut e, "Add diameter");
        let draft = e.draft.as_ref().expect("draft");
        assert!(draft.edits.add.is_empty(), "diameter {bad:?} was accepted");
        assert!(draft.refusal.is_some(), "diameter {bad:?} said nothing");
        assert!(draft.history.undo.is_empty() && draft.history.redo.is_empty());
        assert_eq!(draft.diameter_mm, bad, "the text is the person's");
        assert_eq!(draft.selected, Some(curve));
        assert_eq!(draft.radius_mm, "", "the radius box was written");
    }
    assert!(
        e.draft
            .as_ref()
            .expect("draft")
            .refusal
            .as_deref()
            .is_some_and(|r| r.contains("halve")),
        "the smallest subnormal is refused for what it is"
    );

    // Typing is not a checkpoint; the add is exactly one.
    enter_field(&ctx, &mut e, "Diameter mm", "13.5", false);
    assert_eq!(history_state(&e).1.len(), 0, "typing made a history step");
    click(&ctx, &mut e, "Add diameter");
    let draft = e.draft.as_ref().expect("draft");
    assert_eq!(draft.refusal, None);
    assert_eq!(draft.edits.add, vec![radius_of(curve, 6.75)]);
    assert_eq!(draft.history.undo.len(), 1, "one add, one step");
    assert_eq!(draft.diameter_mm, "13.5", "the typed text is not rewritten");
    assert_eq!(draft.radius_mm, "");
    assert_eq!(draft.selected, Some(curve));
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        painted(&out, &format!("Radius 6.75 mm · Circle {curve}")),
        "the pending line must say Radius and its value"
    );

    // Undo and Redo move the draft and ask no job for anything.
    click(&ctx, &mut e, "Undo");
    assert!(e.draft.as_ref().expect("draft").edits.add.is_empty());
    assert!(e.take_request().is_none() && e.take_apply_request().is_none());
    click(&ctx, &mut e, "Redo");
    assert_eq!(
        e.draft.as_ref().expect("draft").edits.add,
        vec![radius_of(curve, 6.75)]
    );
    assert!(e.take_request().is_none() && e.take_apply_request().is_none());

    // A running job freezes the box, even one that already holds the focus, and
    // the button.
    let frozen = history_state(&e);
    enter_field(&ctx, &mut e, "Diameter mm", "20", false);
    let typed = e.draft.as_ref().expect("draft").diameter_mm.clone();
    frame_running(&ctx, &mut e, vec![egui::Event::Text("7".into())], true);
    assert_eq!(
        e.draft.as_ref().expect("draft").diameter_mm,
        typed,
        "a focused box kept taking text while a job ran"
    );
    click_running(&ctx, &mut e, "Add diameter", true);
    assert_eq!(history_state(&e), frozen);

    // Save a copy and Apply hand over the one request the draft is, and a
    // cancelled, refused or stale reply keeps the draft, the picked circle and
    // the typed text.
    enter_field(&ctx, &mut e, "Diameter mm", "13.5", false);
    e.set_session(true, true, true);
    click(&ctx, &mut e, "Apply constraints");
    let applied = e.take_apply_request().expect("Apply asked");
    e.set_session(true, true, false);
    click(&ctx, &mut e, "Save constraints copy…");
    let request = e.take_request().expect("Save asked");
    assert_eq!(
        crate::transaction_tests::constraints(&request),
        crate::transaction_tests::constraints(&applied)
    );
    assert_eq!(
        crate::transaction_tests::constraints(&request).add,
        vec![radius_of(curve, 6.75)]
    );
    assert_eq!(
        (crate::transaction_tests::sketch(&request), request.expected),
        (sketch, source.version)
    );
    assert!(e.take_request().is_none(), "one press, one request");
    let (text, history) = (
        e.draft.as_ref().expect("draft").diameter_mm.clone(),
        history_state(&e),
    );
    let mut state = crate::edits::Edits::default();
    for cancelled in [false, true] {
        let (tx, rx) = std::sync::mpsc::channel();
        let generation = state
            .start(request.clone(), move |_, g, c| {
                std::thread::spawn(move || {
                    if cancelled {
                        c.cancel();
                    }
                    tx.send(g).expect("reply")
                })
            })
            .expect("start");
        let error = if cancelled {
            ferritecad_types::CadError::Cancelled
        } else {
            ferritecad_types::CadError::topology("lost stored reference")
        };
        let stale = Err(ferritecad_types::CadError::input("stale"));
        assert!(finish_edit(&mut e, &mut state, generation + 1, stale).is_none());
        let g = rx.recv().expect("done");
        assert!(finish_edit(&mut e, &mut state, g, Err(error)).is_none());
        let draft = e.draft.as_ref().expect("retained");
        assert_eq!(draft.diameter_mm, text);
        assert_eq!(draft.selected, Some(curve));
        assert_eq!(history_state(&e), history);
    }
    click(&ctx, &mut e, "Cancel constraints draft");
    assert!(!e.active());
}

/// Focused text fields must honor the same busy state as their action buttons.
#[test]
fn running_jobs_freeze_focused_constraint_numbers_until_editing_resumes() {
    let (_circle_root, circle_path, circle_source, _) = round_fixture(&[10.]);
    let (_line_root, line_path, line_source) = fixture();
    let mut changed = Vec::new();
    for (path, source, label) in [
        (&circle_path, &circle_source, "Diameter mm"),
        (&circle_path, &circle_source, "Radius mm"),
        (&line_path, &line_source, "Line length (mm):"),
        (&circle_path, &circle_source, "Fixed X (mm):"),
        (&circle_path, &circle_source, "Fixed Y (mm):"),
        (&line_path, &line_source, "Fixed X (mm):"),
        (&line_path, &line_source, "Fixed Y (mm):"),
    ] {
        let (mut e, ctx) = opened(path, source);
        let text = |e: &Editor| {
            let d = e.draft.as_ref().expect("draft");
            match label {
                "Diameter mm" => &d.diameter_mm,
                "Radius mm" => &d.radius_mm,
                "Line length (mm):" => &d.length_mm,
                "Fixed X (mm):" => &d.pin_x_mm,
                "Fixed Y (mm):" => &d.pin_y_mm,
                _ => unreachable!(),
            }
            .clone()
        };
        enter_field(&ctx, &mut e, label, "20", false);
        let before = history_state(&e);
        frame_running(&ctx, &mut e, vec![egui::Event::Text("7".into())], true);
        if text(&e) != "20" {
            changed.push((label, text(&e)));
        }
        assert_eq!(history_state(&e), before);
        // A completed job releases the box for ordinary editing again.
        enter_field(&ctx, &mut e, label, "13.5", false);
        assert_eq!(text(&e), "13.5");
        assert_eq!(history_state(&e), before);
    }
    assert!(
        changed.is_empty(),
        "focused fields changed during a job: {changed:?}"
    );
}

#[test]
fn a_stored_radius_is_replaced_by_a_diameter_in_one_exact_remove_and_add() {
    let (_root, path, source, ids) = round_fixture(&[10.]);
    let (curve, sketch) = (ids[0], source.constraint_sketches[0].sketch);
    let mut document = Document::open(&path).expect("open fixture");
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![radius_of(curve, 6.75)],
        },
    )
    .expect("prepare");
    let stored_radius = ferritecad_fixtures::added_constraints(&prepared)[0].id;
    document.write_changes(&prepared).expect("persist");
    let reading = crate::sketch_facts::Source::read(&document).expect("read persisted");
    document.close().expect("close");

    let (mut e, ctx) = opened(&path, &reading);
    let row = format!("Radius 6.75 mm · {stored_radius}");
    assert!(painted(&frame(&ctx, &mut e, vec![]), &row));
    click(&ctx, &mut e, &format!("Circle · {}", short(curve)));
    enter_field(&ctx, &mut e, "Diameter mm", "17", false);
    click_remove_on_row(&ctx, &mut e, &row);
    click(&ctx, &mut e, "Add diameter");
    let replacement = history_state(&e).0;
    assert_eq!(replacement.remove, vec![stored_radius]);
    assert_eq!(replacement.add, vec![radius_of(curve, 8.5)]);
    assert_eq!(e.draft.as_ref().expect("draft").history.undo.len(), 2);
    click(&ctx, &mut e, "Undo");
    assert!(history_state(&e).0.add.is_empty());
    assert_eq!(history_state(&e).0.remove, vec![stored_radius]);
    click(&ctx, &mut e, "Undo");
    assert_eq!(history_state(&e).0, SketchConstraintEdits::default());
    click(&ctx, &mut e, "Redo");
    click(&ctx, &mut e, "Redo");
    assert_eq!(history_state(&e).0, replacement);
    click(&ctx, &mut e, "Save constraints copy…");
    assert_eq!(
        crate::transaction_tests::constraints(&e.take_request().expect("one atomic request")),
        replacement
    );
}

/// A ring's diameters follow the circle that is picked, by its UUID, whatever
/// order it is stored or painted in; the same box serves both circles.
#[test]
fn ring_diameters_follow_the_picked_uuid_not_the_stored_order() {
    for bore_first in [false, true] {
        let radii = if bore_first { [4., 10.] } else { [10., 4.] };
        let (_root, path, source, ids) = round_fixture(&radii);
        let (bore, boundary) = if bore_first {
            (ids[0], ids[1])
        } else {
            (ids[1], ids[0])
        };
        let (mut e, ctx) = opened(&path, &source);
        let (bore_row, boundary_row) = (
            format!("Bore circle · {}", short(bore)),
            format!("Boundary circle · {}", short(boundary)),
        );
        let out = frame(&ctx, &mut e, vec![]);
        let rows: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if t.galley.text().starts_with("Bore circle")
                        || t.galley.text().starts_with("Boundary circle") =>
                {
                    Some(t.galley.text().to_owned())
                }
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(
            rows[0].starts_with("Bore circle"),
            bore_first,
            "the rows are painted in the stored order"
        );

        // The bore's diameter first, the boundary's second, so that neither the
        // order of the rows nor the order of the requests is the order of roles.
        click(&ctx, &mut e, &bore_row);
        enter_field(&ctx, &mut e, "Diameter mm", "4.25", false);
        click(&ctx, &mut e, "Add diameter");
        assert_eq!(e.draft.as_ref().expect("draft").selected, Some(bore));
        click(&ctx, &mut e, &boundary_row);
        enter_field(&ctx, &mut e, "Diameter mm", "13.5", false);
        click(&ctx, &mut e, "Add diameter");
        let draft = e.draft.as_ref().expect("draft");
        assert_eq!(draft.refusal, None);
        assert_eq!(
            draft.edits.add,
            vec![radius_of(bore, 2.125), radius_of(boundary, 6.75)],
            "bore_first={bore_first}"
        );
        assert_eq!(draft.history.undo.len(), 2, "one add, one step each");

        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("one request");
        assert_eq!(
            crate::transaction_tests::constraints(&request).add,
            vec![radius_of(bore, 2.125), radius_of(boundary, 6.75)]
        );
    }
}

/// The signed volume, bounds and edge pairing of a binary STL, read here and not
/// by the code that wrote it.
pub(crate) fn read_stl(bytes: &[u8]) -> (Vec<[[f64; 3]; 3]>, f64) {
    let count = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + 50 * count);
    let mut faces = Vec::new();
    let mut six = 0.;
    for t in bytes[84..].chunks_exact(50) {
        let mut p = [[0.; 3]; 3];
        for (i, point) in p.iter_mut().enumerate() {
            for (j, coordinate) in point.iter_mut().enumerate() {
                let k = 12 + 12 * i + 4 * j;
                *coordinate = f64::from(f32::from_le_bytes(t[k..k + 4].try_into().expect("f32")));
                assert!(coordinate.is_finite());
            }
        }
        let [a, b, c] = p;
        six += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
        faces.push(p);
    }
    (faces, six / 6.)
}

/// Native, once: a ring stored bore-first, both diameters typed into the form's
/// boxes and saved by the window's worker; then the same numbers given to the
/// peer CLI as `diameter`. The two files must carry the same Radius per UUID and
/// the same identities, rebuild cold into the walls those radii describe under the
/// right references, and export bytes an independent reader measures.
#[test]
fn native_diameter_form_worker_and_cli_publish_the_same_radii_ids_sql_and_solid() {
    use crate::creates::tests::ferritecad;
    type Rows = Vec<Vec<rusqlite::types::Value>>;
    use ferritecad_kernel::FaceSurface;
    // Each missing piece is excused only where that piece is not required: a build
    // with the kernel and no solver skips here and still runs the kernel-only gates.
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return;
    }
    // The ring carries data this editor knows nothing about (an attached source
    // claim and an extension table): it has to come through.
    let root = tempfile::tempdir().expect("dir");
    let source_path = root.path().join("round.fcad");
    let ids = write_round_profile(&source_path, &[4., 10.]);
    crate::sketch::drag_tests::attach_source_claim(&source_path);
    let source = {
        let d = Document::open_read_only(&source_path).expect("doc");
        let source = crate::sketch_facts::Source::read(&d).expect("snapshot");
        d.close().expect("close");
        source
    };
    let (bore, boundary) = (ids[0], ids[1]);
    let choice = &source.constraint_sketches[0];
    let (sketch, saved) = (choice.sketch, choice.stored.clone().expect("stored"));
    assert_eq!(saved.curves[0].id, bore, "this ring stores the bore first");
    assert_eq!(
        ferritecad_document::constraint_circle_roles(&saved),
        Some((boundary, bore))
    );
    let before = std::fs::read(&source_path).expect("source bytes");

    let (bore_row, boundary_row) = (
        format!("Bore circle · {}", short(bore)),
        format!("Boundary circle · {}", short(boundary)),
    );
    let fill = |bore_diameter: &str, boundary_diameter: &str| {
        let (mut e, ctx) = opened(&source_path, &source);
        click(&ctx, &mut e, &bore_row);
        enter_field(&ctx, &mut e, "Diameter mm", bore_diameter, false);
        click(&ctx, &mut e, "Add diameter");
        click(&ctx, &mut e, &boundary_row);
        enter_field(&ctx, &mut e, "Diameter mm", boundary_diameter, false);
        click(&ctx, &mut e, "Add diameter");
        click(&ctx, &mut e, "Pair Circle A");
        click(&ctx, &mut e, &bore_row);
        click(&ctx, &mut e, "Pair Circle B");
        click(&ctx, &mut e, "Add Concentric");
        click(&ctx, &mut e, &boundary_row);
        enter_field(&ctx, &mut e, "Fixed X (mm):", "-3.5", false);
        enter_field(&ctx, &mut e, "Fixed Y (mm):", "4.25", false);
        click(&ctx, &mut e, "Add Fixed centre");
        assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("widget request");
        (e, request)
    };
    let work = |e: &mut Editor, mut request: EditRequest, to: PathBuf| {
        request.destination = to;
        let mut state = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start(request, move |r, g, c| {
                crate::edits::spawn_edit(
                    r.source,
                    r.expected,
                    r.changes,
                    r.destination,
                    c,
                    move |result| tx.send((g, result)).expect("reply"),
                )
            })
            .expect("worker");
        let (g, result) = rx
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("worker response");
        let dof = result.as_ref().ok().map(|r| {
            crate::transaction_tests::solve(r)
                .as_ref()
                .expect("solve")
                .degrees_of_freedom()
        });
        (finish_edit(e, &mut state, g, result), dof)
    };

    // The worker, from the boxes: bore D 4.25, boundary D 13.5.
    let (mut e, request) = fill("4.25", "13.5");
    assert_eq!(
        crate::transaction_tests::constraints(&request).add[..2],
        [radius_of(bore, 2.125), radius_of(boundary, 6.75)]
    );
    let ui = root.path().join("ui.fcad");
    let (published, dof) = work(&mut e, request, ui.clone());
    assert_eq!(published, Some(ui.clone()));
    assert_eq!(dof, Some(0));
    assert!(!e.active(), "an accepted copy ends the form");

    // Region containment follows the solved circles; their UUIDs do not swap.
    let crossed = root.path().join("crossed.fcad");
    let (mut e, request) = fill("13.5", "4.25");
    let (published, dof) = work(&mut e, request, crossed.clone());
    assert_eq!(published, Some(crossed.clone()));
    assert_eq!(dof, Some(0));
    assert!(!e.active(), "accepted region change ends the form");
    let d = Document::open_read_only(&crossed).expect("published circles");
    let crossed_source = crate::sketch_facts::Source::read(&d).expect("source");
    let stored = crossed_source.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("sketch");
    assert!(stored.curves.iter().any(|c| c.id == boundary));
    assert!(stored.curves.iter().any(|c| c.id == bore));

    // The peer: the same two diameters, spelled in the request.
    let request_file = root.path().join("request.json");
    let (_, accepted) = fill("4.25", "13.5");
    std::fs::write(
        &request_file,
        ferritecad_fixtures::edit_request_json(&accepted.changes).to_string(),
    )
    .expect("request");
    let cli = root.path().join("cli.fcad");
    let run = |args: &[&std::ffi::OsStr]| {
        let out = std::process::Command::new(ferritecad())
            .args(args)
            .output()
            .expect("peer CLI");
        assert!(out.status.success(), "{out:?}");
        out
    };
    let peer = run(&[
        "edit".as_ref(),
        source_path.as_os_str(),
        "--expect-version".as_ref(),
        source.version.content.to_string().as_ref(),
        "--request".as_ref(),
        request_file.as_os_str(),
        "-o".as_ref(),
        cli.as_os_str(),
        "--json".as_ref(),
    ]);
    let peer = String::from_utf8(peer.stdout).expect("UTF-8");
    assert_eq!(
        crate::transaction_tests::solve_path(&cli)
            .expect("solve")
            .degrees_of_freedom(),
        0,
        "{peer}"
    );
    assert_eq!(std::fs::read(&source_path).expect("source"), before);

    // Same Radius per UUID; the only difference is the freshly minted constraint
    // UUIDs, matched here by the rule they carry, once, explicitly.
    let sketch_of = |path: &Path| {
        let d = Document::open_read_only(path).expect("copy");
        let objects = d.objects().expect("objects");
        let row = objects.iter().find(|o| o.id == sketch).expect("sketch");
        let ObjectPayload::Sketch(s) = row.payload.clone() else {
            panic!("a Sketch")
        };
        let others: Vec<_> = objects.iter().filter(|o| o.id != sketch).cloned().collect();
        let facts = (
            d.meta().document_id,
            others,
            d.dependencies().expect("dependencies"),
            crate::test_names::names(&d),
        );
        d.close().expect("close");
        (s, facts)
    };
    let (ui_sketch, ui_facts) = sketch_of(&ui);
    let (cli_sketch, cli_facts) = sketch_of(&cli);
    let (source_sketch, source_facts) = sketch_of(&source_path);
    assert_eq!(
        ui_facts, cli_facts,
        "objects, links and refs of the two copies"
    );
    assert_eq!(
        ui_facts.1, source_facts.1,
        "every other object is the source's"
    );
    assert_eq!(
        ui_facts.3, source_facts.3,
        "the stored references are the source's"
    );
    assert_eq!(ui_sketch.curves, source_sketch.curves, "the guess was kept");
    assert_eq!(cli_sketch.curves, source_sketch.curves);
    let by_rule = |s: &Sketch| {
        let mut v: Vec<_> = s
            .constraints
            .iter()
            .map(|c| (format!("{:?}", c.rule), c.id))
            .collect();
        v.sort();
        v
    };
    let (u, c) = (by_rule(&ui_sketch), by_rule(&cli_sketch));
    assert_eq!(u.len(), 4);
    assert_eq!(
        u.iter().map(|x| &x.0).collect::<Vec<_>>(),
        c.iter().map(|x| &x.0).collect::<Vec<_>>(),
        "the same four rules"
    );
    for (left, right) in u.iter().zip(&c) {
        assert_ne!(left.1, right.1, "each publication minted its own UUID");
    }
    let radii: std::collections::BTreeMap<_, _> = ui_sketch
        .constraints
        .iter()
        .filter_map(|c| match c.rule.clone() {
            SketchConstraintRule::Radius { curve, radius } => Some((curve, radius.value())),
            _ => None,
        })
        .collect();
    assert_eq!(radii, [(bore, 2.125), (boundary, 6.75)].into());

    // Every SQL cell is literally the other file's, but the Sketch row's payload
    // and hash (where the new UUIDs live) and the modified timestamp.
    let (a, b, s) = (
        crate::fillets::tests::tables(&ui),
        crate::fillets::tests::tables(&cli),
        crate::fillets::tests::tables(&source_path),
    );
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for table in [
        "meta",
        "objects",
        "imported_sources",
        "imported_source_refs",
        "extension",
    ] {
        assert!(
            !a[table].1.is_empty(),
            "{table} must participate in the comparison"
        );
    }
    let selected = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
    let mut differing = Vec::new();
    for (table, (columns, rows)) in &a {
        assert_eq!(rows.len(), b[table].1.len(), "{table}");
        for (x, y) in rows.iter().zip(&b[table].1) {
            for (i, cell) in x.iter().enumerate() {
                if cell == &y[i] {
                    continue;
                }
                let column = columns[i].as_str();
                let allowed = match table.as_str() {
                    "objects" => {
                        x.contains(&selected) && matches!(column, "payload" | "payload_hash")
                    }
                    "meta" => column == "modified_at",
                    _ => false,
                };
                assert!(allowed, "{table}.{column} differs between the two copies");
                differing.push(format!("{table}.{column}"));
            }
        }
    }
    assert!(
        ["objects.payload", "objects.payload_hash"]
            .iter()
            .all(|c| differing.iter().any(|d| d == c)),
        "the minted UUIDs must have differed: {differing:?}"
    );
    // And the copy differs from the source in the Sketch row (payload, hash)
    // and the timestamp;
    // every other table, and every other object row, is the source's.
    for (table, (_, rows)) in &s {
        match table.as_str() {
            "meta" => {}
            "objects" => {
                let rest = |rows: &Rows| {
                    rows.iter()
                        .filter(|r| !r.contains(&selected))
                        .cloned()
                        .collect::<Vec<_>>()
                };
                assert_eq!(rest(&a[table].1), rest(rows), "another object changed");
            }
            _ => assert_eq!(a[table].1, *rows, "{table} was not preserved"),
        }
    }

    // Reopen and rebuild cold: the kernel's own faces, under the right references.
    let analytic = |path: &Path| {
        let doc = Document::open_read_only(path).expect("reopen");
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let built = ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("cold rebuild after reopen");
        assert_eq!(built.shape_count(), 1);
        let mut walls = std::collections::BTreeMap::new();
        let mut volume = None;
        for (input, face) in crate::test_names::root_faces(&doc, &built) {
            if let ferritecad_kernel::NamingInput::Curve(curve) = input {
                walls.insert(curve, kernel.face_surface(face).expect("surface"));
            }
            let stats = kernel.shape_stats(face.shape()).expect("stats");
            assert_eq!(stats.0, 4, "two walls and two caps");
            volume = Some(stats.1);
        }
        built.release_all(&mut kernel);
        doc.close().expect("close");
        (walls, volume.expect("a resolved reference"))
    };
    let (outer, inner, height) = (6.75, 2.125, 15.);
    let exact = std::f64::consts::PI * (outer - inner) * (outer + inner) * height;
    for path in [&ui, &cli] {
        let (walls, volume) = analytic(path);
        assert_eq!(
            walls,
            [
                (boundary, FaceSurface::Cylinder { radius: outer }),
                (bore, FaceSurface::Cylinder { radius: inner }),
            ]
            .into(),
            "each wall under its own circle's reference"
        );
        assert!((volume - exact).abs() < 1e-6 * exact, "{volume} vs {exact}");
        let rebuilt = run(&["rebuild".as_ref(), path.as_os_str(), "--cold".as_ref()]);
        let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
        assert!(
            text.lines()
                .filter(|line| line.contains(" kind=face "))
                .count()
                == 4,
            "{text}"
        );
    }

    // The exported mesh, read here: a closed oriented surface whose rims sit at the
    // two solved radii about the solved centre. A mesh is a prism inscribed in a
    // circle, so it is bounded by the analytic solid, not compared with it for pi.
    let (stl, fbx) = (ui.with_extension("stl"), ui.with_extension("fbx"));
    run(&[
        "export-stl".as_ref(),
        ui.as_os_str(),
        "-o".as_ref(),
        stl.as_os_str(),
        "--linear-deflection".as_ref(),
        "0.05".as_ref(),
        "--angular-deflection".as_ref(),
        "0.1".as_ref(),
    ]);
    run(&[
        "export-fbx".as_ref(),
        ui.as_os_str(),
        "-o".as_ref(),
        fbx.as_os_str(),
    ]);
    let (faces, volume) = read_stl(&std::fs::read(&stl).expect("STL"));
    let q = |v: [f64; 3]| v.map(|x| (x * 1e4).round() as i64);
    let mut directed = std::collections::BTreeMap::<_, usize>::new();
    for f in &faces {
        for k in 0..3 {
            *directed.entry((q(f[k]), q(f[(k + 1) % 3]))).or_default() += 1;
        }
    }
    assert!(
        directed.values().all(|n| *n == 1),
        "not one oriented surface"
    );
    assert!(
        directed
            .keys()
            .all(|(a, b)| directed.contains_key(&(*b, *a)))
    );
    let (centre, delta) = ([-3.5, 4.25], 0.05 + 1e-4);
    let on = |radius: f64| {
        faces
            .iter()
            .filter(|f| {
                f.iter()
                    .all(|p| ((p[0] - centre[0]).hypot(p[1] - centre[1]) - radius).abs() < 1e-4)
            })
            .count()
    };
    assert!(on(outer) >= 12 && on(inner) >= 12, "a wall is missing");
    let pi = std::f64::consts::PI;
    assert!(
        volume >= pi * ((outer - delta).powi(2) - inner * inner) * height
            && volume <= pi * (outer * outer - (inner - delta).powi(2)) * height,
        "mesh volume {volume}"
    );
    assert!(
        faces
            .iter()
            .all(|f| f.iter().all(|p| p[2] >= -1e-4 && p[2] <= height + 1e-4))
    );
    if let Some(dir) = std::env::var_os("FCAD_DIAMETER_ARTIFACT_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifact directory");
        std::fs::copy(&fbx, dir.join("diameter.fbx")).expect("FBX for the pinned reader");
    }
}
