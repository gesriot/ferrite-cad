// SPDX-License-Identifier: MIT
//! §31E: a diameter typed for a circle, through the form's real widgets.
//!
//! A diameter is not a constraint of its own. It is the Radius `D/2`, in the
//! Radius slot, as one history step and one request, so these gates read the
//! draft, the request and the published copy, never a separate "diameter" state.
//! These widget tests write their circle documents through the document API.
//! The viewport diameter tests compare the published geometry with a CLI edit.

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
                rollback: None,
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
