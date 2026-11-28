// SPDX-License-Identifier: MIT
//! Compare the actual Finish changes, a version accepted by the owner, and a cold reading.
use super::*;
use ferritecad_document::{Document, Expression};
use ferritecad_jobs::{DocumentSession, HistoryLimits, LiveDocument, StepCommit};
use ferritecad_kernel::{OperationContext, TessellationParams, mock::MockKernel};

type TestResult = fixtures::TestResult;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Neighbour {
    None,
    CoincidentEnd,
    HorizontalPoint,
}

fn position(curve: &SketchGeometry) -> Point2 {
    match curve {
        SketchGeometry::Point { at } | SketchGeometry::Line { start: at, .. } => *at,
        _ => unreachable!("only points and lines in this fixture"),
    }
}

fn fixed(id: StableEntityId, x: f64, y: f64) -> SketchConstraintRule {
    SketchConstraintRule::Fixed {
        point: SketchPointRef::new(id, SketchPointSelector::At),
        x: Expression::constant(x).expect("finite"),
        y: Expression::constant(y).expect("finite"),
    }
}

#[test]
fn releasing_a_fixed_point_preserves_the_preview_after_finish() -> TestResult {
    release(Neighbour::None)
}

#[test]
fn release_propagates_to_a_coincident_line_end_without_writing_an_independent_curve() -> TestResult
{
    release(Neighbour::CoincidentEnd)
}

#[test]
fn release_preserves_a_remaining_horizontal_between_two_points() -> TestResult {
    release(Neighbour::HorizontalPoint)
}

fn release(neighbour: Neighbour) -> TestResult {
    if !ferritecad_fixtures::native_dependencies(&[(
        ferritecad_eval::solver_available(),
        "PlaneGCS",
    )]) {
        return Ok(());
    }
    let at = |x, y| curve(SketchGeometry::Point { at: point(x, y) });
    let free = at(0., 0.);
    let free_id = free.id;
    let mut curves = vec![free];
    let mut rules = vec![fixed(
        free_id,
        10.,
        if neighbour == Neighbour::HorizontalPoint {
            5.
        } else {
            0.
        },
    )];
    let free_ref = SketchPointRef::new(free_id, SketchPointSelector::At);
    match neighbour {
        Neighbour::None => {}
        Neighbour::CoincidentEnd => {
            let side = line([0., 0.], [50., 0.]);
            rules.push(SketchConstraintRule::Coincident {
                a: free_ref,
                b: SketchPointRef::new(side.id, SketchPointSelector::Start),
            });
            curves.push(side);
        }
        Neighbour::HorizontalPoint => {
            let other = at(30., 10.);
            rules.push(SketchConstraintRule::Horizontal {
                a: free_ref,
                b: SketchPointRef::new(other.id, SketchPointSelector::At),
            });
            curves.push(other);
        }
    }
    // The unrelated point has a different saved seed too. Prediction must leave it alone.
    let anchor = at(25., 0.);
    rules.push(fixed(anchor.id, 30., 0.));
    curves.push(anchor);
    let mut f = fixtures::Drawing::new(curves, false);
    let changes = rules
        .into_iter()
        .map(|rule| EditChange::AddConstraint {
            sketch: f.sketch,
            constraint: SketchConstraint {
                id: StableEntityId::new(),
                rule,
            },
        })
        .collect::<Vec<_>>();
    f.doc.write_changes(&changes)?;
    let context = OperationContext::default();
    let sessions = tempfile::tempdir()?;
    let mut live = LiveDocument::open(
        DocumentSession::open_in(sessions.path(), &f.path, HistoryLimits::default())?,
        MockKernel::new(),
        |_, _| unreachable!("no imports"),
        TessellationParams::default(),
        &context,
    )?;
    let original = f.saved();
    let mut draft = existing(original.curves.clone());
    draft.sketch = f.sketch;
    draft.planes[0].id = original.plane;
    draft.original = Some(original.clone());
    let mut drawing = original.clone();
    for (curve, shown) in drawing
        .curves
        .iter_mut()
        .zip(live.scene().sketch_presentations[0].curves())
    {
        assert_eq!(curve.id, shown.id());
        curve.geometry = shown.geometry().clone();
    }
    let first_position = position(&drawing.curves[0].geometry);
    assert!((first_position.x - 10.).abs() < 1e-6);
    let released = drawing.constraints[0].clone();
    draft.state.curves = drawing.curves;
    draft.state.constraints = drawing.constraints;
    draft.select(Some(Selection::Constraint(released.id)), false);
    draft.delete();
    assert_eq!(draft.undo.len(), 1);
    assert_eq!(draft.message, "");
    let drawing = Sketch {
        plane: f.plane,
        curves: draft.state.curves.clone(),
        constraints: draft.state.constraints.clone(),
    };
    let after = position(&drawing.curves[0].geometry);
    assert!((first_position.x - after.x).abs() <= 1e-6);
    assert!((first_position.y - after.y).abs() <= 1e-6);
    let mut predictions = Vec::new();
    let geometry =
        super::super::prediction::geometry_changes(f.sketch, Some(&original), &drawing, |seed| {
            let (solved, _) = ferritecad_eval::solve_sketch(seed, f.sketch)
                .map_err(|e| e.to_string())?
                .expect("remaining constraints");
            predictions.push(solved.clone());
            Ok(Some(solved))
        });
    if let Err(reason) = &geometry {
        for (i, c) in drawing.curves.iter().enumerate() {
            eprintln!(
                "curve {i}: stored {:?}, preview {:?}, predictions {:?}",
                position(&original.curves[i].geometry),
                position(&c.geometry),
                predictions
                    .iter()
                    .map(|s| position(&s.curves[i].geometry))
                    .collect::<Vec<_>>()
            );
        }
        panic!("{reason}");
    }
    assert_eq!(predictions.len(), 2);
    let before_finish = draft::SOLVES.with(|n| n.get());
    let changes = draft.changes()?;
    assert_eq!(draft::SOLVES.with(|n| n.get()) - before_finish, 2);
    let mut expected = vec![EditChange::RemoveConstraint {
        sketch: f.sketch,
        constraint: released.id,
    }];
    let count = if neighbour != Neighbour::None { 2 } else { 1 };
    expected.extend(
        drawing.curves[..count]
            .iter()
            .map(|c| EditChange::SetCurveGeometry {
                sketch: f.sketch,
                curve: c.id,
                geometry: c.geometry.clone(),
            }),
    );
    assert_eq!(changes, expected);
    let version = live.session().current().version();
    assert_eq!(
        live.apply(version, changes, &context)?.commit_step()?,
        StepCommit::Accepted
    );
    let file = Document::open_read_only(live.session().current().path())?;
    let mut kernel = MockKernel::new();
    let cold = ferritecad_eval::rebuild_cold(&file, &mut kernel, &context)?;
    let accepted = &live.scene().sketch_presentations[0];
    assert_eq!(cold.sketch_presentation(f.sketch), Some(accepted));
    let last = predictions.last().expect("final prediction");
    for ((curve, shown), predicted) in drawing
        .curves
        .iter()
        .zip(accepted.curves())
        .zip(&last.curves)
    {
        assert_eq!(shown.id(), curve.id);
        for (a, b) in [(curve, 1e-6), (predicted, 1e-9)] {
            let coords = |g: &SketchGeometry| match *g {
                SketchGeometry::Point { at } => vec![at.x, at.y],
                SketchGeometry::Line { start, end } => vec![start.x, start.y, end.x, end.y],
                _ => unreachable!("only points and lines"),
            };
            for (v, w) in coords(shown.geometry())
                .into_iter()
                .zip(coords(&a.geometry))
            {
                assert!((v - w).abs() <= b, "{v} vs {w} at {b}");
            }
        }
    }
    cold.release_all(&mut kernel);
    Ok(())
}

#[test]
fn inconsistent_second_prediction_refuses_without_a_request() {
    let mut d = existing(vec![line([0., 0.], [3., 0.])]);
    let original = d.original.as_ref().expect("open draft");
    d.state.curves[0].geometry = line([4., 0.], [7., 0.]).geometry;
    let drawing = Sketch {
        plane: original.plane,
        curves: d.state.curves.clone(),
        constraints: vec![],
    };
    let mut calls = 0;
    let actual =
        super::super::prediction::geometry_changes(d.sketch, Some(original), &drawing, |s| {
            calls += 1;
            let mut answer = s.clone();
            answer.curves[0].geometry = line([1., 1.], [3., 4.]).geometry;
            Ok(Some(answer))
        });
    assert_eq!(calls, 2);
    assert_eq!(
        actual,
        Err("Finish would not keep the drawing as shown; the solver settles elsewhere.".into())
    );
}

#[test]
fn prediction_writes_only_beyond_the_display_tolerance_per_coordinate() {
    let d = existing(vec![line([0., 0.], [10., 0.])]);
    let original = d.original.as_ref().expect("open draft");
    let id = original.curves[0].id;
    let drawing = |offset: f64| Sketch {
        plane: original.plane,
        constraints: vec![],
        curves: vec![SketchCurve {
            geometry: line([offset, offset], [10., 0.]).geometry,
            ..original.curves[0].clone()
        }],
    };
    for (offset, write) in [(0., false), (1e-7, false), (1e-6, false), (1.01e-6, true)] {
        let sketch = drawing(offset);
        let mut calls = 0;
        let changes =
            super::super::prediction::geometry_changes(d.sketch, Some(original), &sketch, |_| {
                calls += 1;
                Ok(None)
            })
            .expect("fixture operation");
        let want = if write {
            vec![EditChange::SetCurveGeometry {
                sketch: d.sketch,
                curve: id,
                geometry: sketch.curves[0].geometry.clone(),
            }]
        } else {
            vec![]
        };
        assert_eq!(changes, want);
        assert_eq!(calls, if write { 2 } else { 1 });
    }
}

#[test]
fn new_curve_precedes_its_constraint_and_cancelled_rule_addition_is_omitted() {
    if !ferritecad_fixtures::native_dependencies(&[(
        ferritecad_eval::solver_available(),
        "PlaneGCS",
    )]) {
        return;
    }
    let mut d = existing(Vec::new());
    d.tool = Tool::Line;
    d.click(point(0., 0.));
    d.click(point(10., 0.));
    let curve = d.state.curves[0].clone();
    let rule = rules::Button::Horizontal
        .rule(&d.state.curves, &[Selection::Curve(curve.id)], true)
        .expect("fixture operation");
    d.add_rule(rule, rules::Button::Horizontal);
    let constraint = d.state.constraints[0].clone();
    assert_eq!(
        d.changes().expect("Finish prediction"),
        vec![
            EditChange::AddCurve {
                sketch: d.sketch,
                curve: curve.clone()
            },
            EditChange::AddConstraint {
                sketch: d.sketch,
                constraint
            }
        ]
    );
    d.delete();
    assert_eq!(
        d.changes().expect("Finish prediction"),
        vec![EditChange::AddCurve {
            sketch: d.sketch,
            curve
        }]
    );
}

#[test]
fn returning_a_dimension_to_its_original_value_keeps_its_saved_spelling() {
    if !ferritecad_fixtures::native_dependencies(&[(
        ferritecad_eval::solver_available(),
        "PlaneGCS",
    )]) {
        return;
    }
    let c = curve(SketchGeometry::Circle {
        center: point(0., 0.),
        radius: 10.,
    });
    let mut d = existing(vec![c.clone()]);
    let id = StableEntityId::new();
    let k = SketchConstraint {
        id,
        rule: SketchConstraintRule::Radius {
            curve: c.id,
            radius: Expression::new("1 cm", 10.).expect("fixture operation"),
        },
    };
    d.state.constraints.push(k.clone());
    d.original
        .as_mut()
        .expect("open draft")
        .constraints
        .push(k.clone());
    d.set_value(id, "12");
    assert_ne!(d.state.constraints[0], k);
    d.set_value(id, "10");
    assert_eq!(d.state.constraints[0], k);
    assert!(d.changes().expect("Finish prediction").is_empty());
}

#[test]
fn finished_seed_uses_saved_coordinates_new_draft_curves_and_final_rule_values() {
    let a = curve(SketchGeometry::Point { at: point(0., 0.) });
    let b = curve(SketchGeometry::Point { at: point(10., 0.) });
    let k = SketchConstraint {
        id: StableEntityId::new(),
        rule: fixed(a.id, 0., 0.),
    };
    let plane = ObjectId::new();
    let original = Sketch {
        plane,
        curves: vec![a.clone(), b],
        constraints: vec![k.clone()],
    };
    let extra = line([1., 3.], [4., 5.]);
    let modified = SketchConstraint {
        id: k.id,
        rule: fixed(a.id, 2., 0.),
    };
    let drawing = Sketch {
        plane,
        curves: vec![
            SketchCurve {
                construction: true,
                geometry: SketchGeometry::Point { at: point(2., 0.) },
                ..a.clone()
            },
            extra.clone(),
        ],
        constraints: vec![modified.clone()],
    };
    assert_eq!(
        super::super::prediction::seed(Some(&original), &drawing),
        Sketch {
            plane,
            curves: vec![
                SketchCurve {
                    construction: true,
                    ..a
                },
                extra
            ],
            constraints: vec![modified],
        }
    );
}

/// The link between the first and third stored lines runs through a newly drawn line.
fn predict_linked_lines(second_agrees: bool, third_agrees: bool) {
    let original_lines: Vec<_> = (0..4)
        .map(|i| line([0., i as f64], [1., i as f64]))
        .collect();
    let d = existing(original_lines);
    let original = d.original.as_ref().expect("stored sketch");
    let mut shown = original.clone();
    for (i, c) in shown.curves.iter_mut().enumerate() {
        c.geometry = line([4., i as f64], [5., i as f64]).geometry;
    }
    shown.curves.push(line([6., 4.], [7., 4.]));
    let join = |left: usize, right: usize| SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Coincident {
            a: SketchPointRef::new(shown.curves[left].id, SketchPointSelector::End),
            b: SketchPointRef::new(shown.curves[right].id, SketchPointSelector::Start),
        },
    };
    shown.constraints = vec![join(4, 2), join(1, 4), join(0, 1)];
    let mut calls = 0;
    let result =
        super::super::prediction::geometry_changes(d.sketch, Some(original), &shown, |s| {
            calls += 1;
            let mut answer = shown.clone();
            for i in 0..4 {
                let written = match calls {
                    1 => false,
                    2 => i == 0,
                    3 => i < 3,
                    _ => panic!("a fourth solve"),
                };
                let expected = if written {
                    &shown.curves[i]
                } else {
                    &original.curves[i]
                };
                assert_eq!(&s.curves[i], expected);
            }
            assert_eq!(s.curves[4], shown.curves[4]);
            if calls == 1 || (calls == 2 && !second_agrees) {
                answer.curves[0] = original.curves[0].clone();
            } else if calls == 3 && !third_agrees {
                // A mismatch outside the group must still refuse the whole edit.
                answer.curves[3] = original.curves[3].clone();
            }
            Ok(Some(answer))
        });
    assert_eq!(calls, if second_agrees { 2 } else { 3 });
    if !second_agrees && !third_agrees {
        assert_eq!(result, Err(super::super::prediction::ELSEWHERE.into()));
        return;
    }
    let count = if second_agrees { 1 } else { 3 };
    let expected: Vec<_> = shown.curves[..count]
        .iter()
        .map(|c| EditChange::SetCurveGeometry {
            sketch: d.sketch,
            curve: c.id,
            geometry: c.geometry.clone(),
        })
        .collect();
    assert_eq!(result, Ok(expected));
}

#[test]
fn matching_second_prediction_keeps_only_the_original_write() {
    predict_linked_lines(true, false);
}

#[test]
fn third_prediction_writes_only_the_saved_curves_in_the_connected_group() {
    predict_linked_lines(false, true);
}

#[test]
fn a_mismatch_after_the_connected_group_prediction_refuses_finish() {
    predict_linked_lines(false, false);
}
