// SPDX-License-Identifier: MIT
#![allow(
    clippy::panic,
    reason = "exact witnesses must fail on a different request"
)]
use super::*;
use draft::Tool;
use ferritecad_document::{
    EditChange, Sketch, SketchConstraint, SketchConstraintRule, SketchGeometry, SketchPointRef,
    SketchPointSelector,
};
use ferritecad_kernel::SketchPlane;
use ferritecad_types::{Point3, Vec3};
use ferritecad_viewport::Projection;

use crate::curve_fixtures as fixtures;
use fixtures::{curve, line, point};

fn empty() -> Draft {
    Draft::new("Sketch 1".into(), draft::planes(&[], &[]), None)
}

fn existing(curves: Vec<SketchCurve>) -> Draft {
    let mut draft = empty();
    draft.original = Some(Sketch {
        plane: draft.planes[0].id,
        curves: curves.clone(),
        constraints: Vec::new(),
    });
    draft.accepted = curves.clone();
    draft.state.curves = curves;
    draft
}

fn camera(plane: SketchPlane, projection: Projection) -> Camera {
    let mut camera = Camera::new();
    camera.resize(800, 600);
    camera
        .frame(([-20., -20., -20.], [20., 20., 20.]))
        .expect("frame");
    camera.set_projection(projection);
    let n = plane.normal();
    let y = plane.y_axis();
    assert!(camera.look_along(
        [n.x as f32, n.y as f32, n.z as f32],
        [y.x as f32, y.y as f32, y.z as f32]
    ));
    camera
}

#[test]
fn pixel_rays_intersect_all_three_standard_planes_in_both_projections() {
    for plane in draft::planes(&[], &[]) {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let plane = plane.frame.expect("standard plane");
            let camera = camera(plane, projection);
            let expected = point(7.125, -3.75);
            let pixel = geometry::screen(&camera, plane, expected).expect("screen");
            let actual = geometry::point_on_plane(&camera, pixel, plane).expect("intersection");
            assert!((actual.x - expected.x).abs() < 1e-10, "{actual:?}");
            assert!((actual.y - expected.y).abs() < 1e-10, "{actual:?}");
            let n = plane.normal();
            let y = plane.y_axis();
            let direction =
                std::array::from_fn::<_, 3, _>(|i| camera.eye()[i] - camera.target()[i]);
            for (actual, expected) in direction
                .into_iter()
                .map(|v| v / camera.distance())
                .zip([n.x, n.y, n.z])
            {
                assert!((f64::from(actual) - expected).abs() < 1e-6);
            }
            for (actual, expected) in camera.up().into_iter().zip([y.x, y.y, y.z]) {
                assert!((f64::from(actual) - expected).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn arbitrary_plane_look_keeps_the_target_and_distance_and_refuses_invalid_axes() {
    let plane = SketchPlane::new(
        Point3::new(1., 2., 3.).expect("origin"),
        Vec3::X,
        Vec3::new(0., 1., 1.).expect("normal"),
    )
    .expect("plane");
    let mut camera = camera(SketchPlane::world_xy(), Projection::Perspective);
    camera.pan(12., 8.);
    let target = camera.target();
    let distance = camera.distance();
    let n = plane.normal();
    let y = plane.y_axis();
    assert!(camera.look_along(
        [n.x as f32, n.y as f32, n.z as f32],
        [y.x as f32, y.y as f32, y.z as f32]
    ));
    assert_eq!(camera.target(), target);
    assert!((camera.distance() - distance).abs() < 1e-5);
    let before = camera;
    assert!(!camera.look_along([0.; 3], [0., 1., 0.]));
    assert!(!camera.look_along([0., 0., 1.], [0., 0., 1.]));
    assert_eq!(camera, before);
}

#[test]
fn edge_on_and_empty_viewports_have_no_plane_point() {
    let plane = SketchPlane::world_xy();
    let camera = camera(
        SketchPlane::new(Point3::ORIGIN, Vec3::X, Vec3::Y).expect("side"),
        Projection::Orthographic,
    );
    assert_eq!(geometry::point_on_plane(&camera, [410., 315.], plane), None);
    assert_eq!(
        geometry::point_on_plane(&Camera::new(), [0., 0.], plane),
        None
    );
}

#[test]
fn hit_testing_uses_the_same_projected_line_circle_arc_and_point() {
    let plane = SketchPlane::world_xy();
    let camera = camera(plane, Projection::Orthographic);
    let shapes = [
        (line([-2., 0.], [2., 0.]), point(0., 0.)),
        (
            curve(SketchGeometry::Circle {
                center: point(0., 0.),
                radius: 2.,
            }),
            point(2., 0.),
        ),
        (
            curve(SketchGeometry::Arc {
                center: point(0., 0.),
                radius: 2.,
                start_angle: 0.,
                end_angle: -std::f64::consts::FRAC_PI_2,
            }),
            point(2., 0.),
        ),
        (
            curve(SketchGeometry::Point { at: point(1., 2.) }),
            point(1., 2.),
        ),
    ];
    for (curve, witness) in shapes {
        let projected = geometry::projected(&camera, plane, &curve.geometry);
        let at = geometry::screen(&camera, plane, witness).expect("screen");
        assert!(geometry::curve_distance(&projected, at) < 1e-10);
        assert_eq!(
            geometry::pick(std::slice::from_ref(&curve), &camera, plane, at),
            Some(curve.id)
        );
        assert_eq!(geometry::pick(&[curve], &camera, plane, [799., 599.]), None);
    }
}

#[test]
fn snapping_chooses_the_nearest_exact_coordinate_without_rounding() {
    let plane = SketchPlane::world_xy();
    let camera = camera(plane, Projection::Orthographic);
    let a = point(0.1234567890123, 2.2345678901234);
    let b = point(0.8, 2.);
    let mut pixel = geometry::screen(&camera, plane, a).expect("screen");
    pixel[0] += 0.25;
    let snapped = geometry::snap([b, a].into_iter(), &camera, plane, pixel).expect("snap");
    assert_eq!(
        [snapped.x.to_bits(), snapped.y.to_bits()],
        [a.x.to_bits(), a.y.to_bits()]
    );
    assert_eq!(
        geometry::snap([a, b].into_iter(), &camera, plane, [799., 599.]),
        None
    );
}

#[test]
fn snap_targets_include_arc_ends_and_all_centres_and_points() {
    let curves = vec![
        line([1., 2.], [3., 4.]),
        curve(SketchGeometry::Circle {
            center: point(5., 6.),
            radius: 2.,
        }),
        curve(SketchGeometry::Point { at: point(7., 8.) }),
        curve(SketchGeometry::Arc {
            center: point(9., 10.),
            radius: 2.,
            start_angle: 0.,
            end_angle: std::f64::consts::PI,
        }),
    ];
    assert_eq!(
        geometry::targets(&curves).collect::<Vec<_>>(),
        vec![
            point(1., 2.),
            point(3., 4.),
            point(5., 6.),
            point(7., 8.),
            point(11., 10.),
            point(7., 10.),
            point(9., 10.)
        ]
    );
}

#[test]
fn polyline_preserves_shared_endpoint_bits_and_closes_only_on_the_start_click() {
    let mut d = empty();
    d.tool = Tool::Line;
    let p = [
        point(0.123456789, 0.2),
        point(10., 0.2),
        point(10., 8.),
        point(0.123456789, 8.),
    ];
    for at in p {
        d.click(at);
    }
    assert_eq!(d.state.curves.len(), 3);
    assert!(d.pending.is_some());
    d.click(p[0]);
    assert_eq!(d.state.curves.len(), 4);
    assert_eq!(d.pending, None);
    for (i, c) in d.state.curves.iter().enumerate() {
        let SketchGeometry::Line { start, end } = c.geometry else {
            panic!("not a line")
        };
        assert_eq!(
            [start.x.to_bits(), start.y.to_bits()],
            [p[i].x.to_bits(), p[i].y.to_bits()]
        );
        let next = p[(i + 1) % 4];
        assert_eq!(
            [end.x.to_bits(), end.y.to_bits()],
            [next.x.to_bits(), next.y.to_bits()]
        );
    }
    assert_eq!(d.undo.len(), 4);
}

#[test]
fn current_endpoint_and_escape_end_a_polyline_without_a_curve() {
    let mut d = empty();
    d.tool = Tool::Line;
    d.click(point(1., 2.));
    d.click(point(1., 2.));
    assert!(d.state.curves.is_empty());
    assert_eq!(d.pending, None);
    d.click(point(1., 2.));
    d.escape();
    assert_eq!(d.pending, None);
    assert_eq!(d.tool, Tool::Line);
    d.escape();
    assert_eq!(d.tool, Tool::Select);
    assert!(d.undo.is_empty());
}

#[test]
fn circle_zero_radius_and_escape_add_nothing_but_a_completed_circle_is_one_step() {
    let mut d = empty();
    d.tool = Tool::Circle;
    d.click(point(1., 2.));
    d.click(point(1., 2.));
    assert!(d.state.curves.is_empty());
    d.click(point(2., 3.));
    d.escape();
    assert!(d.state.curves.is_empty());
    d.construction = true;
    d.click(point(1., 2.));
    d.click(point(4., 6.));
    assert_eq!(
        d.state.curves[0].geometry,
        SketchGeometry::Circle {
            center: point(1., 2.),
            radius: 5.
        }
    );
    assert!(d.state.curves[0].construction);
    assert_eq!(d.undo.len(), 1);
}

#[test]
fn selection_toggles_and_one_delete_or_construction_is_one_history_step() {
    let mut d = existing(vec![line([0., 0.], [1., 0.]), line([0., 1.], [1., 1.])]);
    let ids = [d.state.curves[0].id, d.state.curves[1].id];
    d.select(Some(ids[0]), false);
    d.select(Some(ids[1]), true);
    d.toggle();
    assert!(d.state.curves.iter().all(|c| c.construction));
    assert_eq!(d.undo.len(), 1);
    d.toggle();
    assert!(d.changes().is_empty());
    d.delete();
    assert!(d.state.curves.is_empty());
    assert_eq!(d.undo.len(), 3);
    d.history(true);
    assert_eq!(d.state.curves.len(), 2);
    d.history(false);
    assert!(d.state.curves.is_empty());
    d.history(true);
    d.select(Some(ids[1]), true);
    assert_eq!(d.state.selected, [ids[0]].into_iter().collect());
    d.select(None, true);
    assert!(d.state.selected.is_empty());
}

#[test]
fn request_omits_added_then_removed_and_double_toggled_curves() {
    let mut d = existing(vec![line([0., 0.], [1., 0.])]);
    d.tool = Tool::Point;
    d.click(point(2., 3.));
    let added = d.state.curves[1].id;
    d.select(Some(added), false);
    d.delete();
    let kept = d.state.curves[0].id;
    d.select(Some(kept), false);
    d.toggle();
    d.toggle();
    assert_eq!(d.changes(), vec![]);
    assert_eq!(d.counts(), (0, 0, 0));
}

#[test]
fn request_preserves_untouched_stored_coordinates_in_a_solved_draft() {
    let mut d = existing(vec![line([0., 0.], [1., 0.])]);
    d.state.curves[0].geometry = SketchGeometry::Line {
        start: point(3., 4.),
        end: point(8., 4.),
    };
    assert!(d.changes().is_empty());
    let id = d.state.curves[0].id;
    d.select(Some(id), false);
    d.toggle();
    assert_eq!(
        d.changes(),
        vec![EditChange::SetConstruction {
            sketch: d.sketch,
            curve: id,
            construction: true
        }]
    );
}

#[test]
fn request_explicitly_removes_constraints_before_curves_then_adds_and_flags() {
    let mut d = existing(vec![line([0., 0.], [1., 0.]), line([0., 1.], [1., 1.])]);
    let removed = d.state.curves[0].id;
    let kept = d.state.curves[1].id;
    let constraint = StableEntityId::new();
    d.original
        .as_mut()
        .expect("stored")
        .constraints
        .push(SketchConstraint {
            id: constraint,
            rule: SketchConstraintRule::Horizontal {
                a: SketchPointRef::new(removed, SketchPointSelector::Start),
                b: SketchPointRef::new(removed, SketchPointSelector::End),
            },
        });
    d.select(Some(removed), false);
    d.delete();
    d.tool = Tool::Point;
    d.click(point(2., 3.));
    let added = d.state.curves[1].clone();
    d.select(Some(kept), false);
    d.toggle();
    assert_eq!(d.removed_constraints(), vec![constraint]);
    assert_eq!(
        d.changes(),
        vec![
            EditChange::RemoveConstraint {
                sketch: d.sketch,
                constraint
            },
            EditChange::RemoveCurve {
                sketch: d.sketch,
                curve: removed
            },
            EditChange::AddCurve {
                sketch: d.sketch,
                curve: added
            },
            EditChange::SetConstruction {
                sketch: d.sketch,
                curve: kept,
                construction: true
            }
        ]
    );
}

#[test]
fn new_empty_sketch_creates_only_its_missing_standard_plane_and_sketch() {
    let d = empty();
    let plane = &d.planes[0];
    assert_eq!(
        d.changes(),
        vec![
            EditChange::AddDatumPlane {
                object: plane.id,
                name: Some("XY".into()),
                plane: ferritecad_document::DatumPlane::Fixed {
                    placement: ferritecad_types::Transform::IDENTITY
                }
            },
            EditChange::AddSketch {
                object: d.sketch,
                name: Some("Sketch 1".into()),
                sketch: Sketch {
                    plane: plane.id,
                    curves: vec![],
                    constraints: vec![]
                }
            }
        ]
    );
}

#[test]
fn standard_plane_reuse_is_by_exact_placement_and_sketch_name_is_first_free() {
    let f = fixtures::Drawing::new(vec![], false);
    let mut source = Source::read(&f.doc).expect("facts");
    let choices = draft::planes(&source.objects, &[(f.plane, SketchPlane::world_xy())]);
    assert_eq!(
        choices.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["XY", "XZ", "YZ"]
    );
    assert_eq!(choices[0].id, f.plane);
    assert_eq!(choices[0].addition, None);
    let d = Draft::new("Sketch 1".into(), choices, Some(f.plane));
    assert_eq!(d.changes().len(), 1);
    source.objects[0].name = Some("Sketch 1".into());
    source.objects[1].name = Some("Sketch 3".into());
    assert_eq!(draft::sketch_name(&source.objects), "Sketch 2");
}

mod gpu;
mod widgets;
mod window;
