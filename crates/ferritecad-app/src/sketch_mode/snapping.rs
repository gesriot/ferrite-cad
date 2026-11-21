// SPDX-License-Identifier: MIT
//! Choose sketch coordinates from the same projection used to paint the drawing.
use crate::sketch::{grid::Snap, guide};
use ferritecad_document::Point2;
use ferritecad_kernel::SketchPlane;
use ferritecad_viewport::Camera;

#[derive(Debug, Default)]
pub(super) struct Snapping {
    pub grid: Snap,
    pub guides: guide::Mode,
    pub held: Option<(Point2, usize)>,
    pub pixels_per_point: Option<f32>,
}

impl Snapping {
    /// Object coordinates take precedence, followed by a guide, the grid and the raw point.
    pub fn resolve(
        &mut self,
        raw: Point2,
        object: Option<Point2>,
        anchor: Option<Point2>,
        camera: &Camera,
        plane: SketchPlane,
    ) -> (Point2, bool) {
        let previous = self.held.take();
        if let Some(point) = object {
            return (point, true);
        }
        if let Some(a) = anchor {
            let offset = [raw.x - a.x, raw.y - a.y];
            let scale = local_scale(camera, plane, a, offset)
                / f64::from(self.pixels_per_point.unwrap_or(1.0));
            let held = previous.filter(|(old, _)| *old == a).map(|(_, h)| h);
            if let Some(h) = guide::acquire(self.guides, offset, scale, held) {
                self.held = Some((a, h));
                let [x, y] = guide::unit(h);
                let along = x * offset[0] + y * offset[1];
                return (
                    Point2 {
                        x: a.x + along * x,
                        y: a.y + along * y,
                    },
                    true,
                );
            }
        }
        let x = self.grid.round(raw.x);
        let y = self.grid.round(raw.y);
        (Point2 { x, y }, self.grid != Snap::Off)
    }
}

/// Physical pixels per sketch millimetre, along the pointer direction at the anchor.
fn local_scale(camera: &Camera, plane: SketchPlane, a: Point2, offset: [f64; 2]) -> f64 {
    let length = offset[0].hypot(offset[1]);
    if length == 0.0 {
        return 0.0;
    }
    let step = Point2 {
        x: a.x + offset[0] / length,
        y: a.y + offset[1] / length,
    };
    match (
        super::geometry::screen(camera, plane, a),
        super::geometry::screen(camera, plane, step),
    ) {
        (Some(a), Some(b)) => (a[0] - b[0]).hypot(a[1] - b[1]),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve_fixtures::point;
    use ferritecad_viewport::Projection;

    #[test]
    fn every_grid_step_rounds_halves_away_from_zero_in_sketch_coordinates() {
        for (grid, half, expected) in [
            (Snap::Off, 0.125, 0.125),
            (Snap::Tenth, 0.15, 0.2),
            (Snap::One, 0.5, 1.),
            (Snap::Five, 2.5, 5.),
            (Snap::Ten, 5., 10.),
        ] {
            assert_eq!(grid.round(half), expected);
            assert_eq!(grid.round(-half), -expected);
        }
    }

    #[test]
    fn an_object_beats_a_guide_and_a_guide_keeps_the_exact_anchor_coordinate() {
        let plane = SketchPlane::world_xy();
        let mut camera = Camera::new();
        camera.resize(800, 600);
        camera.frame(([-10.; 3], [10.; 3])).expect("frame");
        camera.look_along([0., 0., 1.], [0., 1., 0.]);
        let mut snaps = Snapping {
            grid: Snap::Five,
            guides: guide::Mode::Orthogonal,
            ..Default::default()
        };
        let anchor = point(-2.25, 0.1234567890123);
        let raw = point(6.37, 0.14);
        let object = point(6.4, 0.3);
        let hit = snaps.resolve(raw, Some(object), Some(anchor), &camera, plane);
        assert_eq!(hit, (object, true));
        assert_eq!(snaps.held, None);
        let (guided, marker) = snaps.resolve(raw, None, Some(anchor), &camera, plane);
        assert!(marker);
        assert_eq!(guided.y, anchor.y);
        assert!((guided.x - raw.x).abs() < 1e-12);
        assert_eq!(snaps.held, Some((anchor, 0)));
        assert_eq!(
            snaps.resolve(raw, None, None, &camera, plane),
            (point(5., 0.), true)
        );
        snaps.grid = Snap::Off;
        assert_eq!(snaps.resolve(raw, None, None, &camera, plane), (raw, false));
    }

    #[test]
    fn guide_scale_tracks_foreshortening_at_its_anchor() {
        let mut camera = Camera::new();
        camera.resize(800, 600);
        camera.frame(([-10.; 3], [10.; 3])).expect("frame");
        camera.set_projection(Projection::Orthographic);
        camera.look_along([0., 0.8660254, 0.5], [0., 0.5, -0.8660254]);
        let plane = SketchPlane::world_xy();
        let a = point(2., 3.);
        let x = local_scale(&camera, plane, a, [10., 0.]);
        let y = local_scale(&camera, plane, a, [0., 10.]);
        assert!(x > 0.);
        assert!((y / x - 0.5).abs() < 1e-6);
    }
}
