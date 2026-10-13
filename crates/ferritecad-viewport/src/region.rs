// SPDX-License-Identifier: MIT
//! The one rectangle of a window that the model is drawn into and pointed at in.
//!
//! A window holds panels beside the model, so the model's picture is a part of
//! it. Everything that depends on where that part is reads this one value:
//! the camera's aspect ratio, the viewport the renderer draws through, the
//! offscreen frame a pick is read from, and the translation of a pointer from
//! the window's pixels into the picture's. A second copy of "where the viewport
//! is" would be the copy that drifts, and the symptom would be a click that
//! selects what is a panel's width away from the pointer.
//!
//! Physical pixels, origin at the window's top-left corner, integers: an
//! interface lays out in logical points, but a surface, a viewport and a
//! readback are all grids of pixels, and the conversion between the two happens
//! exactly once, in [`PixelRect::from_points`].

/// A rectangle of the window, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl PixelRect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// A rectangle at the window's origin, which is the whole of a window that
    /// has nothing beside its picture.
    pub const fn sized(width: u32, height: u32) -> Self {
        Self::new(0, 0, width, height)
    }

    /// The pixels an interface's rectangle covers, clamped to the surface.
    ///
    /// `min` and `max` are the rectangle's corners in logical points, as an
    /// interface lays them out, and `pixels_per_point` is the scale the same
    /// interface is rendered at: the 1 of an ordinary display and the 2 of a
    /// Retina one go through this one function. Each edge is rounded to the
    /// nearest pixel, so a rectangle that touches a panel's edge in points
    /// touches it in pixels too and the two never overlap or leave a seam.
    ///
    /// Anything that is not a usable rectangle - a non-finite corner, a scale
    /// that is not positive, corners in the wrong order - is an empty one, which
    /// is what a window with no room for a picture is.
    pub fn from_points(
        min: [f32; 2],
        max: [f32; 2],
        pixels_per_point: f32,
        surface: (u32, u32),
    ) -> Self {
        let usable = pixels_per_point.is_finite()
            && pixels_per_point > 0.0
            && [min[0], min[1], max[0], max[1]]
                .iter()
                .all(|value| value.is_finite());
        if !usable {
            return Self::default();
        }
        let edge = |points: f32, limit: u32| -> u32 {
            let pixels = (points * pixels_per_point).round();
            // Negative and past-the-end edges are clamped, not refused: an
            // interface may lay a panel partly outside a window being dragged
            // smaller, and the part that is inside is still a rectangle.
            pixels.clamp(0.0, limit as f32) as u32
        };
        let left = edge(min[0], surface.0);
        let top = edge(min[1], surface.1);
        let right = edge(max[0], surface.0).max(left);
        let bottom = edge(max[1], surface.1).max(top);
        Self::new(left, top, right - left, bottom - top)
    }

    pub const fn x(&self) -> u32 {
        self.x
    }

    pub const fn y(&self) -> u32 {
        self.y
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Whether there is no area to draw into or to point at.
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Whether a window point lies in this rectangle. Half-open, like pixels:
    /// the left and top edges are in, the right and bottom edges are not.
    pub fn contains(&self, window: (f32, f32)) -> bool {
        let (x, y) = window;
        x >= self.x as f32
            && y >= self.y as f32
            && x < self.x as f32 + self.width as f32
            && y < self.y as f32 + self.height as f32
    }

    /// A window point in this rectangle's own pixels.
    ///
    /// Not checked against the size: a drag that began inside and went on
    /// across a panel still has a position relative to the picture.
    pub fn local(&self, window: (f32, f32)) -> (f32, f32) {
        (window.0 - self.x as f32, window.1 - self.y as f32)
    }

    /// The window point of a point in this rectangle's own pixels.
    pub fn window(&self, local: (f32, f32)) -> (f32, f32) {
        (local.0 + self.x as f32, local.1 + self.y as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_become_pixels_at_one_and_at_two_pixels_per_point() {
        let (min, max) = ([260.0, 52.0], [800.0, 690.0]);
        assert_eq!(
            PixelRect::from_points(min, max, 1.0, (1024, 768)),
            PixelRect::new(260, 52, 540, 638)
        );
        // The same layout on a display with twice the pixels: the same points,
        // twice the pixels in every edge and in the surface.
        assert_eq!(
            PixelRect::from_points(min, max, 2.0, (2048, 1536)),
            PixelRect::new(520, 104, 1080, 1276)
        );
    }

    #[test]
    fn a_fractional_scale_rounds_each_edge_not_the_size() {
        // 1.5 pixels per point: 10.2 points is 15.3 pixels and 20.2 is 30.3.
        // Rounding the edges keeps neighbours seamless; rounding the width
        // would let a panel's edge and this rectangle's overlap by a pixel.
        let rect = PixelRect::from_points([10.2, 0.0], [20.2, 10.0], 1.5, (100, 100));
        assert_eq!((rect.x(), rect.width()), (15, 15));
        let neighbour = PixelRect::from_points([20.2, 0.0], [30.2, 10.0], 1.5, (100, 100));
        assert_eq!(rect.x() + rect.width(), neighbour.x());
    }

    #[test]
    fn a_rectangle_is_clamped_to_the_surface_and_never_inverted() {
        assert_eq!(
            PixelRect::from_points([-30.0, -10.0], [5000.0, 5000.0], 1.0, (800, 600)),
            PixelRect::new(0, 0, 800, 600)
        );
        // Dragged smaller than the panels around it: nothing is left.
        assert!(PixelRect::from_points([300.0, 40.0], [100.0, 20.0], 1.0, (800, 600)).is_empty());
        assert!(PixelRect::from_points([900.0, 0.0], [1200.0, 50.0], 1.0, (800, 600)).is_empty());
    }

    #[test]
    fn something_that_is_not_a_rectangle_is_an_empty_one() {
        for (min, max, scale) in [
            ([f32::NAN, 0.0], [10.0, 10.0], 1.0),
            ([0.0, 0.0], [f32::INFINITY, 10.0], 1.0),
            ([0.0, 0.0], [10.0, 10.0], 0.0),
            ([0.0, 0.0], [10.0, 10.0], f32::NAN),
            ([0.0, 0.0], [10.0, 10.0], -1.0),
        ] {
            assert_eq!(
                PixelRect::from_points(min, max, scale, (800, 600)),
                PixelRect::default()
            );
        }
        assert!(PixelRect::default().is_empty());
    }

    #[test]
    fn window_and_local_points_are_inverses_and_containment_is_half_open() {
        let rect = PixelRect::new(260, 52, 540, 638);
        assert_eq!(rect.local((260.0, 52.0)), (0.0, 0.0));
        assert_eq!(rect.local((300.5, 70.0)), (40.5, 18.0));
        assert_eq!(rect.window(rect.local((421.0, 99.0))), (421.0, 99.0));

        assert!(rect.contains((260.0, 52.0)));
        assert!(rect.contains((799.9, 689.9)));
        assert!(!rect.contains((259.9, 100.0)));
        assert!(!rect.contains((300.0, 51.9)));
        assert!(!rect.contains((800.0, 100.0)));
        assert!(!rect.contains((300.0, 690.0)));
        assert!(!PixelRect::default().contains((0.0, 0.0)));
    }
}
