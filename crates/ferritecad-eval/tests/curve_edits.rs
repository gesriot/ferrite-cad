// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
#[path = "../../ferritecad-fixtures/tests/support/curve_edits.rs"]
mod support;
use ferritecad_document::*;
use ferritecad_eval::{LiveModel, RebuildResult, RebuildStatus, rebuild_cached, rebuild_cold};
use ferritecad_kernel::{GeometryKernel, OperationContext, mock::MockKernel};
use support::*;

const EMPTY: &str =
    "invalid input: the sketch has no main Line, Arc or Circle, so it gives no profile";
fn no_profile(curves: Vec<SketchCurve>, expected: String) {
    let f = Drawing::new(curves.clone(), true);
    let mut k = MockKernel::new();
    let result =
        rebuild_cold(&f.doc, &mut k, &OperationContext::default()).expect("drawing rebuild");
    assert!(matches!(
        result.status(f.sketch),
        Some(RebuildStatus::Built)
    ));
    assert_eq!(
        result
            .profile_failure(f.sketch)
            .expect("profile reason")
            .to_string(),
        expected
    );
    let drawn = result
        .sketch_presentation(f.sketch)
        .expect("complete drawing");
    assert_eq!(drawn.curves().len(), curves.len());
    for (a, b) in drawn.curves().iter().zip(&curves) {
        assert_eq!(a.id(), b.id);
        assert_eq!(a.geometry(), &b.geometry);
        assert_eq!(a.is_construction(), b.construction);
    }
    let report = result.report();
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].id, f.feature);
    assert_eq!(
        report.failed[0].message,
        format!(
            "invalid input: sketch {} gives no profile: {expected}",
            f.sketch
        )
    );
    assert_eq!(
        report.skipped,
        vec![ferritecad_eval::SkippedFeature {
            id: f.body,
            cause: f.feature
        }]
    );
    assert_eq!(k.extrude_count(), 0);
}
#[test]
fn an_empty_drawing_builds_without_a_profile() {
    no_profile(vec![], EMPTY.into());
}
#[test]
fn construction_only_drawing_builds_without_a_profile() {
    let mut c = line([0., 0.], [10., 0.]);
    c.construction = true;
    no_profile(vec![c], EMPTY.into());
}
#[test]
fn point_only_drawing_builds_without_a_profile() {
    no_profile(
        vec![curve(SketchGeometry::Point { at: point(0., 0.) })],
        EMPTY.into(),
    );
}
#[test]
fn an_open_chain_names_its_free_stored_endpoint() {
    let curves = vec![line([0., 0.], [10., 0.]), line([10., 0.], [10., 8.])];
    let reason = format!(
        "invalid input: the profile does not close: curve {} has a free end at (10, 8)",
        curves[1].id
    );
    no_profile(curves, reason);
}
#[test]
fn an_open_reversed_segment_names_its_free_stored_start() {
    let curves = vec![line([0., 0.], [10., 0.]), line([10., 8.], [10., 0.])];
    let reason = format!(
        "invalid input: the profile does not close: curve {} has a free start at (10, 8)",
        curves[1].id
    );
    no_profile(curves, reason);
}
#[test]
fn a_branched_joint_names_the_point_and_every_curve() {
    let curves = vec![
        line([0., 0.], [10., 0.]),
        line([0., 0.], [0., 8.]),
        line([0., 0.], [-2., 0.]),
    ];
    let mut ids: Vec<_> = curves.iter().map(|c| c.id).collect();
    ids.sort();
    let ids = ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    no_profile(
        curves,
        format!("invalid input: profile curves {ids} have more than two ends at (0, 0)"),
    );
}
#[test]
fn a_dangling_main_line_is_not_dropped_beside_a_closed_loop() {
    let mut curves = rectangle();
    let extra = line([20., 0.], [20., 8.]);
    let reason = format!(
        "invalid input: the profile does not close: curve {} has a free end at (20, 8)",
        extra.id
    );
    curves.push(extra);
    no_profile(curves, reason);
}
#[test]
fn points_and_crossing_guides_do_not_change_the_profile_or_names() {
    let mut f = Drawing::new(rectangle(), true);
    let mut k = MockKernel::new();
    let context = OperationContext::default();
    let before = rebuild_cold(&f.doc, &mut k, &context).expect("base");
    let mut curves = f.saved().curves;
    curves.push(curve(SketchGeometry::Point { at: point(0., 0.) }));
    let mut guide = line([-2., 4.], [12., 4.]);
    guide.construction = true;
    curves.push(guide);
    f.replace(curves);
    let after = rebuild_cold(&f.doc, &mut k, &context).expect("drawing");
    assert!(after.all_built());
    assert_eq!(before.profile(f.sketch), after.profile(f.sketch));
    assert_eq!(before.result_key(f.feature), after.result_key(f.feature));
    let keys = |b: &RebuildResult| {
        b.topology()
            .feature(f.feature)
            .expect("names")
            .bindings
            .keys()
            .copied()
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&before), keys(&after));
    assert_eq!(
        after
            .sketch_presentation(f.sketch)
            .expect("drawing")
            .curves()
            .len(),
        6
    );
}
#[test]
fn consumer_only_edit_retains_the_complete_profile_reason_in_cold_cached_and_live() -> TestResult {
    let mut f = Drawing::new(vec![], true);
    let mut k = MockKernel::new();
    let context = OperationContext::default();
    let mut live = LiveModel::default();
    let baseline = live.prepare(&f.doc, &mut k, &context, None)?.report();
    live.accept(&mut k);
    f.doc.write_changes(&[EditChange::constant(
        f.feature,
        ExpressionSlot::ExtrudeDistance,
        4.,
    )?])?;
    let reused = live.prepare(&f.doc, &mut k, &context, None)?;
    assert_eq!(
        reused
            .profile_failure(f.sketch)
            .expect("reused reason")
            .to_string(),
        EMPTY
    );
    assert_eq!(reused.report(), baseline);
    let cold = rebuild_cold(&f.doc, &mut k, &context)?;
    assert_eq!(cold.report(), baseline);
    let mut cache = CacheStore::open(
        f.root.path().join("cache"),
        f.doc.meta().document_id,
        k.identity().id(),
        k.identity().version(),
    )?;
    for _ in 0..2 {
        let (cached, _) = rebuild_cached(&f.doc, &mut k, &mut cache, &context)?;
        assert_eq!(cached.report(), baseline);
        assert_eq!(
            cached
                .profile_failure(f.sketch)
                .expect("reason")
                .to_string(),
            EMPTY
        );
    }
    live.release(&mut k);
    Ok(())
}

fn solver() -> bool {
    ferritecad_fixtures::native_dependencies(&[(
        ferritecad_sketch_solver::is_available(),
        "PLANEGCS",
    )])
}
fn constrained_drawing(
    mut curve: SketchCurve,
    at: SketchPointSelector,
    construction: bool,
) -> TestResult {
    if !solver() {
        return Ok(());
    }
    curve.construction = construction;
    let mut f = Drawing::new(vec![curve.clone()], false);
    f.doc.write_changes(&[EditChange::AddConstraint {
        sketch: f.sketch,
        constraint: SketchConstraint {
            id: ferritecad_types::StableEntityId::new(),
            rule: SketchConstraintRule::Fixed {
                point: SketchPointRef::new(curve.id, at),
                x: Expression::constant(0.)?,
                y: Expression::constant(0.)?,
            },
        },
    }])?;
    let before = std::fs::read(&f.path)?;
    let built = rebuild_cold(&f.doc, &mut MockKernel::new(), &OperationContext::default())?;
    assert!(built.all_built());
    assert!(built.profile(f.sketch).is_none());
    assert!(built.profile_failure(f.sketch).is_some());
    let report = built
        .solve_report(f.sketch)
        .expect("one solve keeps its report");
    assert!(report.redundant().is_empty());
    let drawing = built.sketch_presentation(f.sketch).expect("solved drawing");
    assert_eq!(drawing.curves().len(), 1);
    assert_eq!(drawing.curves()[0].geometry(), &curve.geometry);
    assert_eq!(drawing.curves()[0].is_construction(), construction);
    assert_eq!(std::fs::read(&f.path)?, before);
    Ok(())
}
#[test]
fn a_solved_open_line_keeps_its_drawing_and_solver_report() -> TestResult {
    constrained_drawing(line([0., 0.], [10., 0.]), SketchPointSelector::Start, false)
}
#[test]
fn a_solved_construction_line_keeps_its_drawing_and_solver_report() -> TestResult {
    constrained_drawing(line([0., 0.], [10., 0.]), SketchPointSelector::Start, true)
}
#[test]
fn a_solved_point_keeps_its_drawing_and_solver_report() -> TestResult {
    constrained_drawing(
        curve(SketchGeometry::Point { at: point(0., 0.) }),
        SketchPointSelector::At,
        false,
    )
}
#[test]
fn all_six_rectangle_records_build_on_the_mock_kernel() -> TestResult {
    let base = rectangle();
    let mut f = Drawing::new(base.clone(), true);
    for curves in variants(&base) {
        f.replace(curves);
        let built = rebuild_cold(&f.doc, &mut MockKernel::new(), &OperationContext::default())?;
        assert!(built.all_built(), "{:?}", built.report());
        assert_eq!(
            built.profile(f.sketch).expect("profile").segments().count(),
            4
        );
    }
    Ok(())
}
