// SPDX-License-Identifier: MIT
//! Compare a draft solve with the scene accepted by the session transaction used by Finish.
#[path = "../../ferritecad-fixtures/tests/support/curve_edits.rs"]
mod support;

use ferritecad_document::{EditChange, Expression, SketchConstraint, SketchConstraintRule};
use ferritecad_document::{Point2, SketchGeometry, SketchPointRef, SketchPointSelector};
use ferritecad_jobs::{DocumentSession, HistoryLimits, LiveDocument, StepCommit};
use ferritecad_kernel::{OperationContext, TessellationParams, mock::MockKernel};
use ferritecad_types::StableEntityId;
use support::{Drawing, TestResult, curve, point};

fn position(geometry: &SketchGeometry) -> Point2 {
    match geometry {
        SketchGeometry::Point { at } => *at,
        _ => unreachable!("the fixture contains only points"),
    }
}

#[test]
fn releasing_a_fixed_point_preserves_the_preview_after_finish() -> TestResult {
    release_and_compare(false)
}

#[test]
fn explicitly_saving_the_released_point_keeps_its_preview_position() -> TestResult {
    release_and_compare(true)
}

fn release_and_compare(save_released_point: bool) -> TestResult {
    if !ferritecad_fixtures::native_dependencies(&[(
        ferritecad_eval::solver_available(),
        "PlaneGCS",
    )]) {
        return Ok(());
    }
    let free = curve(SketchGeometry::Point { at: point(0., 0.) });
    let anchor = curve(SketchGeometry::Point { at: point(30., 0.) });
    let free_id = free.id;
    let anchor_id = anchor.id;
    let mut f = Drawing::new(vec![free, anchor], false);
    let mut fixes = Vec::new();
    for (id, x) in [(free_id, 10.), (anchor_id, 30.)] {
        fixes.push(EditChange::AddConstraint {
            sketch: f.sketch,
            constraint: SketchConstraint {
                id: StableEntityId::new(),
                rule: SketchConstraintRule::Fixed {
                    point: SketchPointRef::new(id, SketchPointSelector::At),
                    x: Expression::constant(x)?,
                    y: Expression::constant(0.)?,
                },
            },
        });
    }
    f.doc.write_changes(&fixes)?;
    let context = OperationContext::default();
    let sessions = tempfile::tempdir()?;
    let mut live = LiveDocument::open(
        DocumentSession::open_in(sessions.path(), &f.path, HistoryLimits::default())?,
        MockKernel::new(),
        |_, _| unreachable!("no imports"),
        TessellationParams::default(),
        &context,
    )?;
    let mut draft = f.saved();
    assert_eq!(position(&draft.curves[0].geometry), point(0., 0.));
    let drawing = &live.scene().sketch_presentations[0];
    for (curve, shown) in draft.curves.iter_mut().zip(drawing.curves()) {
        assert_eq!(curve.id, shown.id());
        curve.geometry = shown.geometry().clone();
    }
    let accepted = position(&draft.curves[0].geometry);
    assert!((accepted.x - 10.).abs() < 1e-6);
    let released = draft.constraints.remove(0);
    let (preview, _) =
        ferritecad_eval::solve_sketch(&draft, f.sketch)?.expect("anchor still fixed");
    let predicted = position(&preview.curves[0].geometry);
    assert!((predicted.x - accepted.x).abs() <= 1e-9);
    assert!((predicted.y - accepted.y).abs() <= 1e-9);
    // Both solved positions are unchanged relative to the accepted drawing.
    assert_eq!(preview.curves[1].geometry, draft.curves[1].geometry);
    let version = live.session().current().version();
    let mut changes = vec![EditChange::RemoveConstraint {
        sketch: f.sketch,
        constraint: released.id,
    }];
    if save_released_point {
        changes.push(EditChange::SetCurveGeometry {
            sketch: f.sketch,
            curve: free_id,
            geometry: preview.curves[0].geometry.clone(),
        });
    }
    assert_eq!(
        live.apply(version, changes, &context)?.commit_step()?,
        StepCommit::Accepted
    );
    let result = position(live.scene().sketch_presentations[0].curves()[0].geometry());
    let file = ferritecad_document::Document::open_read_only(live.session().current().path())?;
    let mut cold_kernel = MockKernel::new();
    let cold = ferritecad_eval::rebuild_cold(&file, &mut cold_kernel, &context)?;
    let shown = cold.sketch_presentation(f.sketch).expect("rebuilt drawing");
    assert_eq!(position(shown.curves()[0].geometry()), result);
    cold.release_all(&mut cold_kernel);
    let error = (predicted.x - result.x).hypot(predicted.y - result.y);
    assert!(
        error <= 1e-6,
        "preview ({}, {}), accepted ({}, {}), difference {error} mm",
        predicted.x,
        predicted.y,
        result.x,
        result.y
    );
    Ok(())
}
