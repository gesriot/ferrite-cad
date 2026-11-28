// SPDX-License-Identifier: MIT
use super::constraint_widgets::{click_label, press_rule};
use super::widgets::Ui;
use super::window::{click, enter, open, version};
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window};
use ferritecad_document::{DirectedAngle, Expression, SketchSegmentRef};
use ferritecad_ui::ModelEdit;

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[
        (ferritecad_eval::solver_available(), "PLANEGCS"),
        (ferritecad_occt::is_available(), "OCCT"),
    ])
}

fn draft(w: &Window) -> &Draft {
    w.creates
        .sketch
        .viewport
        .draft
        .as_ref()
        .expect("open draft")
}

fn lengths() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    ObjectId,
    Sketch,
    [StableEntityId; 2],
) {
    let root = tempfile::tempdir().expect("fixture directory");
    let path = root.path().join("plate.fcad");
    ferritecad_jobs::create_document(
        ferritecad_jobs::CreateDocumentRequest::new(
            &path,
            ferritecad_jobs::NewDocument::SamplePlate(Default::default()),
            "test",
        ),
        &Default::default(),
    )
    .expect("plate");
    crate::feature_tests::persist_rectangle_lengths(&path);
    let (sketch, saved) = super::curve_edits::stored(&path);
    let rules = [60., 30.].map(|mm| {
        saved
            .constraints
            .iter()
            .find(|c| draft::dimension(c).is_some_and(|(_, v)| v.value() == mm))
            .expect("length rule")
            .id
    });
    (root, path, sketch, saved, rules)
}

#[test]
fn set_updates_each_saved_distance_in_place_and_restores_only_its_own_rule() {
    if !native() {
        return;
    }
    let (root, path, sketch, before, [one, two]) = lengths();
    let mut w = open(&path, Drawn::Native);
    w.input.frame(([-40.; 3], [40.; 3])).expect("fit");
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, one);
    let values = |w: &Window| {
        let d = draft(w);
        [one, two].map(|id| {
            let c = d
                .state
                .constraints
                .iter()
                .find(|c| c.id == id)
                .expect("retained");
            (c.id, draft::dimension(c).expect("length").1.value())
        })
    };
    let set = |w: &mut Window, ui: &mut Ui, value: &str| {
        ui.replace_dimension_text(w, value);
        ui.press(w, "Set", false);
        assert!(draft(w).message.is_empty(), "{}", draft(w).message);
    };
    ui.replace_dimension_text(&mut w, "70");
    assert!(draft(&w).undo.is_empty());
    assert_eq!(draft(&w).state.constraints, before.constraints);
    ui.press(&mut w, "Set", false);
    assert_eq!(values(&w), [(one, 70.), (two, 30.)]);
    let first = draft(&w).changes().expect("prepared");
    let original_version = version(&w);
    set(&mut w, &mut ui, "55");
    assert_eq!(values(&w), [(one, 55.), (two, 30.)]);
    let second = draft(&w).changes().expect("prepared");
    ui.press(&mut w, "Undo", true);
    assert_eq!(draft(&w).changes().expect("restored"), first);
    ui.press(&mut w, "Redo", true);
    assert_eq!(draft(&w).changes().expect("restored"), second);
    set(&mut w, &mut ui, "55");
    assert_eq!(draft(&w).undo.len(), 2);
    set(&mut w, &mut ui, "60");
    assert_eq!(draft(&w).undo.len(), 3);
    // Returning the number restores the saved rule. The underconstrained plate can still move.
    let solve = |source: &Sketch| {
        ferritecad_eval::solve_sketch(source, sketch)
            .expect("plate solve")
            .expect("constrained")
    };
    let (mut predicted, mut report) = solve(&before);
    for mm in [70., 55., 60.] {
        let rule = predicted
            .constraints
            .iter_mut()
            .find(|c| c.id == one)
            .expect("first length");
        let SketchConstraintRule::Distance { distance, .. } = &mut rule.rule else {
            panic!("Distance")
        };
        *distance = Expression::constant(mm).expect("length");
        (predicted, report) = solve(&predicted);
    }
    assert_eq!(draft(&w).state.constraints, before.constraints);
    assert_eq!(
        report.degrees_of_freedom(),
        draft(&w)
            .state
            .report
            .as_ref()
            .expect("report")
            .degrees_of_freedom()
    );
    let geometry: Vec<_> = predicted
        .curves
        .iter()
        .map(|c| EditChange::SetCurveGeometry {
            sketch,
            curve: c.id,
            geometry: c.geometry.clone(),
        })
        .collect();
    assert_eq!(draft(&w).changes().expect("geometry only"), geometry);
    ui.press(&mut w, "Undo", true);
    assert_eq!(draft(&w).changes().expect("restored"), second);
    ui.press(&mut w, "Redo", true);
    assert_eq!(draft(&w).changes().expect("restored"), geometry);
    assert!(w.creates.sketch.viewport.take_request().is_none());
    assert_eq!(version(&w), original_version);
    let rules_only = root.path().join("rules-only.fcad");
    let same_value = EditChange::constant(
        sketch,
        ferritecad_document::ExpressionSlot::Distance { constraint: one },
        60.,
    )
    .expect("original length");
    super::cli::cli_edit(&path, &rules_only, original_version, &[same_value]);
    assert_eq!(super::curve_edits::stored(&rules_only).1, before);
    let full = root.path().join("displayed.fcad");
    super::cli::cli_edit(&path, &full, original_version, &geometry);
    let request = super::window::finish(&mut w, &mut ui);
    assert_eq!(request.changes, geometry);
    super::window::publish(&mut w, request);
    let accepted = w.sessions.export_path().expect("accepted");
    assert_eq!(
        super::curve_edits::stored(&accepted),
        super::curve_edits::stored(&full)
    );
    let (_, cold_full) = solve(&super::curve_edits::stored(&full).1);
    let (_, cold_window) = solve(&super::curve_edits::stored(&accepted).1);
    assert_eq!(cold_full.degrees_of_freedom(), 4);
    assert_eq!(cold_window.degrees_of_freedom(), 4);
    for format in ["stl", "fbx"] {
        assert_eq!(
            super::cli::export(&accepted, format),
            super::cli::export(&full, format)
        );
        assert_eq!(
            super::cli::export(&path, format),
            super::cli::export(&rules_only, format)
        );
    }

    let mut w = open(&path, Drawn::Native);
    w.input.frame(([-60.; 3], [60.; 3])).expect("fit");
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    let SketchGeometry::Line { start, end } = draft(&w).state.curves[2].geometry else {
        panic!("third side");
    };
    click(
        &mut w,
        point((start.x + end.x) / 2., (start.y + end.y) / 2.),
        false,
    );
    press_rule(&mut ui, &mut w, "Horizontal");
    let other = draft(&w).state.constraints.last().expect("added").clone();
    click_label(&mut w, &mut ui, one);
    set(&mut w, &mut ui, "70");
    click_label(&mut w, &mut ui, two);
    set(&mut w, &mut ui, "33");
    assert_eq!(values(&w), [(one, 70.), (two, 33.)]);
    click_label(&mut w, &mut ui, one);
    set(&mut w, &mut ui, "60");
    assert_eq!(
        draft(&w).state.constraints.iter().find(|c| c.id == one),
        before.constraints.iter().find(|c| c.id == one)
    );
    assert_eq!(values(&w), [(one, 60.), (two, 33.)]);
    assert_eq!(draft(&w).state.constraints.last(), Some(&other));
    assert_eq!(draft(&w).constraint_counts(), (1, 0, 1));
    assert!(w.creates.sketch.viewport.take_request().is_none());
    assert_eq!(super::curve_edits::stored(&path).1, before);
}

fn check_refused(
    path: &std::path::Path,
    sketch: ObjectId,
    id: StableEntityId,
    inputs: &[&str],
    good: &str,
) {
    let mut w = open(path, Drawn::Native);
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, id);
    let before = draft(&w).state.clone();
    let accepted = version(&w);
    let bytes = std::fs::read(path).expect("saved document");
    for &input in inputs {
        ui.replace_dimension_text(&mut w, input);
        ui.press(&mut w, "Set", false);
        let d = draft(&w);
        assert!(!d.message.is_empty(), "accepted {input:?}");
        assert_eq!(
            w.creates
                .sketch
                .viewport
                .value_field
                .as_ref()
                .expect("selected Value")
                .2,
            input
        );
        assert_eq!(d.state, before);
        assert!(d.undo.is_empty() && d.redo.is_empty());
        assert!(d.changes().expect("no pending edit").is_empty());
        assert!(w.creates.sketch.viewport.take_request().is_none());
    }
    ui.replace_dimension_text(&mut w, good);
    ui.press(&mut w, "Set", false);
    assert!(draft(&w).message.is_empty());
    assert_eq!(draft(&w).undo.len(), 1);
    ui.press(&mut w, "Undo", true);
    assert_eq!(draft(&w).state, before);
    assert_eq!(version(&w), accepted);
    assert_eq!(std::fs::read(path).expect("source bytes"), bytes);
}

#[test]
fn a_refused_viewport_distance_keeps_its_text_selection_and_history() {
    if native() {
        let (_root, path, sketch, _, [id, _]) = lengths();
        check_refused(
            &path,
            sketch,
            id,
            &[
                "", " ", "-", "abc", "NaN", "inf", "-inf", "0", "-0", "-1", "1e309", "7,5", "6 0",
            ],
            "70",
        );
    }
}

#[test]
fn a_refused_viewport_angle_keeps_its_text_selection_and_history() {
    if !native() {
        return;
    }
    let r = DirectedAngle::from_degrees(-70.).expect("angle").radians();
    let lines = [
        line([-10., 0.], [0., 0.]),
        line([3., 5.], [3. + 10. * r.cos(), 5. + 10. * r.sin()]),
    ];
    let [a, b] = lines.each_ref().map(|l| {
        SketchSegmentRef::new(
            SketchPointRef::new(l.id, SketchPointSelector::Start),
            SketchPointRef::new(l.id, SketchPointSelector::End),
        )
    });
    let id = StableEntityId::new();
    let mut f = fixtures::Drawing::new(lines.into(), false);
    f.doc
        .write_changes(&[EditChange::AddConstraint {
            sketch: f.sketch,
            constraint: SketchConstraint {
                id,
                rule: SketchConstraintRule::DirectedAngle {
                    a,
                    b,
                    angle: Expression::constant(r).expect("angle"),
                },
            },
        }])
        .expect("save angle");
    check_refused(
        &f.path,
        f.sketch,
        id,
        &[
            "",
            " ",
            "-",
            "abc",
            "NaN",
            "inf",
            "-inf",
            "1e999",
            "181",
            "-181",
            "180.000001",
            "-180.000001",
            "270",
            "360",
            "30,5",
            "−30",
            "3 0",
            "5e-324",
            "0x10",
            "--5",
        ],
        "110",
    );
}
