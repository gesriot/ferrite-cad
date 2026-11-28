// SPDX-License-Identifier: MIT
//! §27G: the existing constraints editor on the profile of a saved Revolve
//! with a bore, driven through its real widgets and its real worker; §27H:
//! the same on a solid profile closed on the axis along its stated Line.
use super::*;
use ferritecad_document::SketchCurve;

pub(crate) use crate::sketch_mode::tests::revolved_profile::{SECTOR_P, SHAFT_T};

use crate::feature_tests::write_sector;

pub(crate) fn reading(path: &Path) -> ExtrudeEditSource {
    let d = Document::open_read_only(path).expect("open");
    let source = crate::sketch_facts::Source::read(&d).expect("catalog");
    d.close().expect("close");
    source
}
fn painted_prefix(out: &egui::FullOutput, prefix: &str) -> Option<String> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text().starts_with(prefix) => {
            Some(t.galley.text().to_owned())
        }
        _ => None,
    })
}
fn line(add: AddLineConstraint) -> AddSketchConstraint {
    AddSketchConstraint::Line(add)
}
fn on(curve: StableEntityId, kind: LineConstraintKind) -> AddSketchConstraint {
    line(AddLineConstraint::Line { curve, kind })
}
fn mm(value: f64) -> LineConstraintKind {
    LineConstraintKind::Distance(LineLengthMm::new(value).expect("length"))
}
/// Select a Line, then add one constraint to it through the widgets.
fn add_on(ctx: &egui::Context, e: &mut Editor, segment: usize, button: &str) {
    click(ctx, e, &format!("Segment {segment}"));
    click(ctx, e, button);
}
fn add_length(ctx: &egui::Context, e: &mut Editor, segment: usize, value: &str) {
    click(ctx, e, &format!("Segment {segment}"));
    enter_length(ctx, e, value, false);
    click(ctx, e, "Add length");
}
fn add_pin(ctx: &egui::Context, e: &mut Editor, segment: usize, x: &str, y: &str) {
    click(ctx, e, &format!("Segment {segment}"));
    click(ctx, e, "Pin Start");
    enter_field(ctx, e, "Fixed X (mm):", x, false);
    enter_field(ctx, e, "Fixed Y (mm):", y, false);
    click(ctx, e, "Add Fixed point");
}
/// The rigid dimensioning of `SECTOR_P`, through the same widgets a person
/// uses, in the order the request records it.
fn dimension_sector(ctx: &egui::Context, e: &mut Editor) {
    add_on(ctx, e, 1, "Add Horizontal");
    add_on(ctx, e, 2, "Add Vertical");
    add_on(ctx, e, 4, "Add Horizontal");
    add_on(ctx, e, 5, "Add Vertical");
    add_pin(ctx, e, 1, "2.75", "-3.25");
    add_length(ctx, e, 1, "4.75");
    add_length(ctx, e, 2, "5.375");
    add_length(ctx, e, 4, "1.5");
    add_length(ctx, e, 5, "12.75");
}
fn expected_sector(curves: &[SketchCurve]) -> Vec<AddSketchConstraint> {
    let id = |i: usize| curves[i].id;
    vec![
        on(id(0), LineConstraintKind::Horizontal),
        on(id(1), LineConstraintKind::Vertical),
        on(id(3), LineConstraintKind::Horizontal),
        on(id(4), LineConstraintKind::Vertical),
        on(
            id(0),
            LineConstraintKind::Fixed {
                at: LineEndpoint::Start,
                x: SketchCoordinateMm::new(2.75).expect("x"),
                y: SketchCoordinateMm::new(-3.25).expect("y"),
            },
        ),
        on(id(0), mm(4.75)),
        on(id(1), mm(5.375)),
        on(id(3), mm(1.5)),
        on(id(4), mm(12.75)),
    ]
}

/// Without a kernel or a solver: the window names the Revolve and its kept
/// angle, the widgets build the rigid request one checkpoint at a time, a
/// refused addition keeps the draft, and Replace length on a stored length is
/// one exact removal plus one addition.
#[test]
fn revolve_constraint_widgets_name_the_revolve_build_and_replace_a_length() {
    let root = tempfile::tempdir().expect("dir");
    let path = root.path().join("sector.fcad");
    let revolve = write_sector(&path, &SECTOR_P, 137.5);
    let source = reading(&path);
    let choice = &source.constraint_sketches[0];
    assert_eq!(choice.refusal, None);
    assert_eq!(choice.height_mm, None, "a Revolve has no height");
    let Some(SketchProfileUse::PartialRevolve {
        feature,
        axis: ferritecad_document::RevolveAxis::SketchY,
        radians: angle,
        ..
    }) = choice.profile_use
    else {
        panic!("a sector with a bore: {:?}", choice.profile_use)
    };
    assert_eq!((feature, angle.radians().to_degrees()), (revolve, 137.5));
    let curves = choice.stored.as_ref().expect("stored").curves.clone();
    let mut e = Editor::default();
    assert!(e.begin(&path, &source, choice.sketch));
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    let owner = painted_prefix(&frame(&ctx, &mut e, vec![]), "Profile of Revolve")
        .expect("the owning Revolve is named");
    assert_eq!(
        owner,
        format!(
            "Profile of Revolve {revolve}: 137.5° about the sketch Y axis. \
             Apply rebuilds the feature and its dependents."
        )
    );
    dimension_sector(&ctx, &mut e);
    let rigid = expected_sector(&curves);
    assert_eq!(history_state(&e).0.add, rigid);
    assert_eq!(
        history_state(&e).1.len(),
        rigid.len(),
        "one checkpoint each"
    );
    click(&ctx, &mut e, "Undo");
    assert_eq!(history_state(&e).0.add, rigid[..rigid.len() - 1]);
    click(&ctx, &mut e, "Redo");
    assert_eq!(history_state(&e).0.add, rigid);
    let kept = history_state(&e);
    click(&ctx, &mut e, "Save constraints copy…");
    let request = e.take_request().expect("widget request");
    assert_eq!(crate::transaction_tests::constraints(&request).add, rigid);
    assert!(
        crate::transaction_tests::constraints(&request)
            .remove
            .is_empty()
    );
    assert_eq!(
        (
            request.source.as_path(),
            crate::transaction_tests::sketch(&request),
            request.expected
        ),
        (path.as_path(), choice.sketch, source.version)
    );
    assert_eq!(
        history_state(&e),
        kept,
        "Save keeps the draft until a reply"
    );
    // Where there is Open CASCADE but no solver, the real worker refuses
    // with the typed reason; the draft, its history and the source stay.
    if ferritecad_occt::is_available() && !ferritecad_sketch_solver::is_available() {
        let before = std::fs::read(&path).expect("source");
        let mut r = request.clone();
        r.destination = root.path().join("copy.fcad");
        let mut state = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start(r, move |r, g, c| {
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
        let (g, result) = rx.recv().expect("worker reply");
        let error = result.as_ref().expect_err("no solver").to_string();
        assert!(error.contains("planegcs"), "{error}");
        assert_eq!(finish_edit(&mut e, &mut state, g, result), None);
        assert_eq!(history_state(&e), kept, "the draft survives the refusal");
        assert!(!root.path().join("copy.fcad").exists());
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }

    // A stored length, written without a solver, is replaced exactly.
    let mut document = Document::open(&path).expect("open");
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        choice.sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: rigid.clone(),
        },
    )
    .expect("prepared");
    document
        .write_changes(&prepared)
        .expect("stored constraints");
    document.close().expect("close");
    let stored = reading(&path);
    let wall = stored.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .constraints
        .iter()
        .find(|c| matches!(&c.rule, SketchConstraintRule::Distance { distance, .. } if distance.value() == 5.375))
        .expect("the wall height")
        .id;
    let mut e = Editor::default();
    assert!(e.begin(&path, &stored, choice.sketch));
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    click(&ctx, &mut e, "Segment 2");
    enter_length(&ctx, &mut e, "7", false);
    click(&ctx, &mut e, "Replace length");
    assert_eq!(
        history_state(&e).0,
        SketchConstraintEdits {
            remove: vec![wall],
            add: vec![on(curves[1].id, mm(7.))],
        }
    );
    assert_eq!(history_state(&e).1.len(), 1, "one checkpoint");
}

/// The rigid dimensioning of `SHAFT_T`: H/V on every Line, the base's
/// start — the axis Line's lower end — pinned at X 0, four lengths.
fn dimension_shaft(ctx: &egui::Context, e: &mut Editor) {
    for (segment, button) in [
        (1, "Add Horizontal"),
        (2, "Add Vertical"),
        (3, "Add Horizontal"),
        (4, "Add Vertical"),
        (5, "Add Horizontal"),
        (6, "Add Vertical"),
    ] {
        add_on(ctx, e, segment, button);
    }
    add_pin(ctx, e, 1, "0", "-2.25");
    add_length(ctx, e, 1, "8");
    add_length(ctx, e, 2, "5.75");
    add_length(ctx, e, 3, "2.5");
    add_length(ctx, e, 4, "9.25");
}
fn expected_shaft(curves: &[SketchCurve]) -> Vec<AddSketchConstraint> {
    let id = |i: usize| curves[i].id;
    let mut add: Vec<_> = (0..6)
        .map(|i| {
            on(
                id(i),
                if i % 2 == 0 {
                    LineConstraintKind::Horizontal
                } else {
                    LineConstraintKind::Vertical
                },
            )
        })
        .collect();
    add.push(on(
        id(0),
        LineConstraintKind::Fixed {
            at: LineEndpoint::Start,
            x: SketchCoordinateMm::new(0.).expect("x"),
            y: SketchCoordinateMm::new(-2.25).expect("y"),
        },
    ));
    add.extend([
        on(id(0), mm(8.)),
        on(id(1), mm(5.75)),
        on(id(2), mm(2.5)),
        on(id(3), mm(9.25)),
    ]);
    add
}

/// §27H, without a kernel or a solver: the window names the stated axis Line
/// and says what keeps it on the axis; the widgets build the rigid request,
/// Undo/Redo and a refused addition keep the draft, a cancelled Save keeps
/// it retryable, and Replace length is one exact removal plus one addition.
#[test]
fn axis_closed_constraint_widgets_name_the_axis_line_and_keep_the_draft() {
    let root = tempfile::tempdir().expect("dir");
    let path = root.path().join("shaft.fcad");
    let revolve = write_sector(&path, &SHAFT_T, 137.5);
    let source = reading(&path);
    let choice = &source.constraint_sketches[0];
    assert_eq!(choice.refusal, None);
    assert_eq!(choice.height_mm, None);
    let curves = choice.stored.as_ref().expect("stored").curves.clone();
    let Some(SketchProfileUse::PartialRevolve {
        feature,
        axis: stated,
        radians: angle,
        ..
    }) = choice.profile_use
    else {
        panic!("a solid sector: {:?}", choice.profile_use)
    };
    assert_eq!(
        (feature, stated, angle.radians().to_degrees()),
        (revolve, ferritecad_document::RevolveAxis::SketchY, 137.5)
    );
    let mut e = Editor::default();
    assert!(e.begin(&path, &source, choice.sketch));
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    let owner = painted_prefix(&frame(&ctx, &mut e, vec![]), "Profile of Revolve")
        .expect("the owning Revolve is named");
    assert_eq!(
        owner,
        format!(
            "Profile of Revolve {revolve}: 137.5° about the sketch Y axis. \
             Apply rebuilds the feature and its dependents."
        )
    );
    dimension_shaft(&ctx, &mut e);
    let rigid = expected_shaft(&curves);
    assert_eq!(history_state(&e).0.add, rigid);
    assert_eq!(history_state(&e).1.len(), rigid.len());
    click(&ctx, &mut e, "Undo");
    assert_eq!(history_state(&e).0.add, rigid[..rigid.len() - 1]);
    click(&ctx, &mut e, "Redo");
    assert_eq!(history_state(&e).0.add, rigid);
    let kept = history_state(&e);
    click(&ctx, &mut e, "Save constraints copy…");
    let request = e.take_request().expect("widget request");
    assert_eq!(crate::transaction_tests::constraints(&request).add, rigid);
    assert_eq!(
        (
            request.source.as_path(),
            crate::transaction_tests::sketch(&request),
            request.expected
        ),
        (path.as_path(), choice.sketch, source.version)
    );
    // Save Cancel returns no path: nothing is submitted, the same draft and
    // history stay, and Save offers the same request again.
    assert!(e.take_request().is_none());
    assert_eq!(history_state(&e), kept);
    click(&ctx, &mut e, "Save constraints copy…");
    let retry = e.take_request().expect("retry");
    assert_eq!(
        (
            crate::transaction_tests::constraints(&retry),
            retry.source.clone(),
            crate::transaction_tests::sketch(&retry),
            retry.expected
        ),
        (
            crate::transaction_tests::constraints(&request).clone(),
            request.source.clone(),
            crate::transaction_tests::sketch(&request),
            request.expected
        )
    );
    if ferritecad_occt::is_available() && !ferritecad_sketch_solver::is_available() {
        let before = std::fs::read(&path).expect("source");
        let mut r = request.clone();
        r.destination = root.path().join("copy.fcad");
        let mut state = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start(r, move |r, g, c| {
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
        let (g, result) = rx.recv().expect("worker reply");
        let error = result.as_ref().expect_err("no solver").to_string();
        assert!(error.contains("planegcs"), "{error}");
        assert_eq!(finish_edit(&mut e, &mut state, g, result), None);
        assert_eq!(history_state(&e), kept, "the draft survives the refusal");
        assert!(!root.path().join("copy.fcad").exists());
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }

    let mut document = Document::open(&path).expect("open");
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        choice.sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: rigid.clone(),
        },
    )
    .expect("prepared");
    document
        .write_changes(&prepared)
        .expect("stored constraints");
    document.close().expect("close");
    let stored = reading(&path);
    let base = stored.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .constraints
        .iter()
        .find(
            |c| matches!(&c.rule, SketchConstraintRule::Distance { distance, .. } if distance.value() == 8.),
        )
        .expect("the base")
        .id;
    let mut e = Editor::default();
    assert!(e.begin(&path, &stored, choice.sketch));
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    click(&ctx, &mut e, "Segment 1");
    enter_length(&ctx, &mut e, "9.25", false);
    click(&ctx, &mut e, "Replace length");
    assert_eq!(
        history_state(&e).0,
        SketchConstraintEdits {
            remove: vec![base],
            add: vec![on(curves[0].id, mm(9.25))],
        }
    );
    assert_eq!(history_state(&e).1.len(), 1, "one checkpoint");
}
