// SPDX-License-Identifier: MIT
//! Projection, hit distances and exact snapping without window state.
use ferritecad_document::{Point2, SketchCurve, SketchGeometry};
use ferritecad_kernel::SketchPlane;
use ferritecad_types::StableEntityId;
use ferritecad_viewport::Camera;

pub(super) const PICK_PIXELS: f64 = 8.0;
pub(super) const SNAP_PIXELS: f64 = 10.0;
pub(super) const EDGE_ON: &str = "The view is edge-on to the sketch plane.";

pub(super) fn point_on_plane(
    camera: &Camera,
    pixel: [f64; 2],
    plane: SketchPlane,
) -> Option<Point2> {
    let (eye, ray) = camera.pixel_ray(pixel)?;
    let origin = plane.origin();
    let normal = plane.normal();
    let n = [normal.x, normal.y, normal.z];
    let delta = [origin.x - eye[0], origin.y - eye[1], origin.z - eye[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| (0..3).map(|i| a[i] * b[i]).sum::<f64>();
    let denominator = dot(ray, n);
    if denominator.abs() < 1e-8 {
        return None;
    }
    let t = dot(delta, n) / denominator;
    if t < 0.0 {
        return None;
    }
    let local = std::array::from_fn(|i| ray[i] * t - delta[i]);
    let x = plane.x_axis();
    let y = plane.y_axis();
    Point2::new(dot(local, [x.x, x.y, x.z]), dot(local, [y.x, y.y, y.z])).ok()
}

pub(super) fn screen(camera: &Camera, plane: SketchPlane, point: Point2) -> Option<[f64; 2]> {
    let origin = plane.origin();
    let x = plane.x_axis();
    let y = plane.y_axis();
    camera.project_point([
        origin.x + x.x * point.x + y.x * point.y,
        origin.y + x.y * point.x + y.y * point.y,
        origin.z + x.z * point.x + y.z * point.y,
    ])
}

/// The same screen polyline is painted and measured for selection.
pub(super) fn projected(
    camera: &Camera,
    plane: SketchPlane,
    geometry: &SketchGeometry,
) -> Vec<[f64; 2]> {
    let points = match geometry {
        SketchGeometry::Point { at } => vec![*at],
        SketchGeometry::Line { start, end } => vec![*start, *end],
        SketchGeometry::Circle { center, radius } => {
            arc(*center, *radius, 0.0, std::f64::consts::TAU)
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => arc(*center, *radius, *start_angle, *end_angle),
        _ => Vec::new(),
    };
    points
        .into_iter()
        .filter_map(|p| screen(camera, plane, p))
        .collect()
}

fn arc(center: Point2, radius: f64, start: f64, end: f64) -> Vec<Point2> {
    let count = ((end - start).abs() / std::f64::consts::TAU * 256.0)
        .ceil()
        .clamp(1.0, 256.0) as usize;
    (0..=count)
        .map(|i| {
            polar(
                center,
                radius,
                start + (end - start) * i as f64 / count as f64,
            )
        })
        .collect()
}

fn polar(center: Point2, radius: f64, angle: f64) -> Point2 {
    Point2 {
        x: center.x + radius * angle.cos(),
        y: center.y + radius * angle.sin(),
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

pub(super) fn curve_distance(points: &[[f64; 2]], at: [f64; 2]) -> f64 {
    if let [point] = points {
        return distance(*point, at);
    }
    points
        .windows(2)
        .map(|pair| {
            let [a, b] = [pair[0], pair[1]];
            let d = [b[0] - a[0], b[1] - a[1]];
            let length = d[0] * d[0] + d[1] * d[1];
            let t = if length == 0.0 {
                0.0
            } else {
                (((at[0] - a[0]) * d[0] + (at[1] - a[1]) * d[1]) / length).clamp(0.0, 1.0)
            };
            distance(at, [a[0] + t * d[0], a[1] + t * d[1]])
        })
        .fold(f64::INFINITY, f64::min)
}

pub(super) fn pick(
    curves: &[SketchCurve],
    camera: &Camera,
    plane: SketchPlane,
    at: [f64; 2],
) -> Option<StableEntityId> {
    curves
        .iter()
        .map(|c| {
            (
                c.id,
                curve_distance(&projected(camera, plane, &c.geometry), at),
            )
        })
        .filter(|(_, d)| *d <= PICK_PIXELS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// The one catalogue of exact sketch-coordinate snap targets, in curve order.
pub(super) fn targets(curves: &[SketchCurve]) -> impl Iterator<Item = Point2> + '_ {
    curves.iter().flat_map(|curve| match curve.geometry {
        SketchGeometry::Point { at } => vec![at],
        SketchGeometry::Line { start, end } => vec![start, end],
        SketchGeometry::Circle { center, .. } => vec![center],
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            vec![
                polar(center, radius, start_angle),
                polar(center, radius, end_angle),
                center,
            ]
        }
        _ => Vec::new(),
    })
}

pub(super) fn snap(
    targets: impl Iterator<Item = Point2>,
    camera: &Camera,
    plane: SketchPlane,
    at: [f64; 2],
) -> Option<Point2> {
    targets
        .filter_map(|point| Some((point, distance(screen(camera, plane, point)?, at))))
        .filter(|(_, d)| *d <= SNAP_PIXELS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(point, _)| point)
}
