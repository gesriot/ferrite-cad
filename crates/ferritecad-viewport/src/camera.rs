// SPDX-License-Identifier: MIT
//! Where the view is from, and how big the window is.
//!
//! Separate from the snapshot because the two change for different reasons and
//! at different rates: orbiting the camera does not touch the model, and
//! rebuilding the model does not move the camera. Keeping them apart is what
//! lets a view survive a rebuild.
//!
//! # A window of no size is a normal thing to be handed
//!
//! Minimised windows, collapsed panes and the moment before a first layout all
//! produce a zero width or height. That is not an error and it is not worth an
//! `Option` at every call site, so the arithmetic here stays finite at any size
//! and [`Camera::is_drawable`] says whether there is any point drawing. A
//! projection that divided by a zero aspect ratio would put a `NaN` into a
//! uniform buffer, and the picture would go black one frame later somewhere
//! else entirely.

use ferritecad_types::{CadError, Result};

/// Looking at a model from somewhere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    eye: [f32; 3],
    target: [f32; 3],
    up: [f32; 3],
    /// Vertical field of view, radians.
    fov: f32,
    near: f32,
    far: f32,
    width: u32,
    height: u32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: [0.0, -1.0, 0.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            fov: std::f32::consts::FRAC_PI_4,
            near: 0.1,
            far: 1000.0,
            width: 0,
            height: 0,
        }
    }
}

impl Camera {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn eye(&self) -> [f32; 3] {
        self.eye
    }

    pub fn target(&self) -> [f32; 3] {
        self.target
    }

    /// Records a new surface size.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    /// Whether the surface has any area to draw into.
    pub fn is_drawable(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Width over height, or 1.0 when there is no area.
    ///
    /// One rather than zero or infinity: it keeps every matrix finite, and a
    /// square projection nothing is drawn through is harmless.
    pub fn aspect(&self) -> f32 {
        if self.width == 0 || self.height == 0 {
            1.0
        } else {
            self.width as f32 / self.height as f32
        }
    }

    /// Points the camera at a box, far enough away to see all of it.
    ///
    /// Refuses a box it cannot make sense of rather than producing a view from
    /// nowhere. An empty model has no bounds to frame and the caller decides
    /// what to show instead.
    pub fn frame(&mut self, bounds: ([f32; 3], [f32; 3])) -> Result<()> {
        let (min, max) = bounds;
        for axis in 0..3 {
            if !min[axis].is_finite() || !max[axis].is_finite() {
                return Err(CadError::input(
                    "a camera cannot frame a model whose extent is not finite",
                ));
            }
            if max[axis] < min[axis] {
                return Err(CadError::input(
                    "a camera cannot frame a box whose maximum is below its minimum",
                ));
            }
        }

        // Halving first avoids overflowing two large, same-sign coordinates
        // merely while finding the point between them.
        let centre = [
            min[0] * 0.5 + max[0] * 0.5,
            min[1] * 0.5 + max[1] * 0.5,
            min[2] * 0.5 + max[2] * 0.5,
        ];
        let extent = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        if extent.iter().any(|value| !value.is_finite()) {
            return Err(CadError::input(
                "a camera cannot frame a box whose extent exceeds its number format",
            ));
        }
        let radius = extent[0].hypot(extent[1]).hypot(extent[2]) * 0.5;
        if !radius.is_finite() {
            return Err(CadError::input(
                "a camera cannot frame a box whose diagonal exceeds its number format",
            ));
        }
        // A single point still needs a distance to be looked at from, or the
        // eye lands on the target and the view matrix has no direction.
        let radius = if radius > f32::EPSILON { radius } else { 1.0 };

        let vertical_half_fov = self.fov * 0.5;
        let horizontal_half_fov = (vertical_half_fov.tan() * self.aspect()).atan();
        // A sphere fits through a perspective cone at r / sin(theta), not at
        // r / tan(theta). Use the narrower axis, which is horizontal in a
        // portrait viewport, and leave a small margin for f32 rounding.
        let limiting_half_fov = vertical_half_fov.min(horizontal_half_fov);
        let distance = radius / limiting_half_fov.sin() * 1.05;
        let eye_y = centre[1] - distance;
        if !distance.is_finite() || !eye_y.is_finite() || eye_y == centre[1] {
            return Err(CadError::input(
                "a camera cannot represent a useful view of a box at this scale",
            ));
        }

        let near = (distance - radius).max(radius * 1e-3);
        let far = distance + radius * 1.05;
        if !near.is_finite() || !far.is_finite() || far <= near {
            return Err(CadError::input(
                "a camera cannot represent the clipping range this box requires",
            ));
        }

        self.target = centre;
        self.eye = [centre[0], eye_y, centre[2]];
        self.near = near;
        self.far = far;
        Ok(())
    }

    /// The matrix a vertex shader multiplies by, column-major.
    ///
    /// Finite at every size, including none: see the module documentation.
    pub fn view_projection(&self) -> [f32; 16] {
        multiply(&self.projection(), &self.view())
    }

    /// Right-handed look-at, column-major.
    fn view(&self) -> [f32; 16] {
        let forward = normalise(sub(self.target, self.eye)).unwrap_or([0.0, 1.0, 0.0]);
        let side = normalise(cross(forward, self.up)).unwrap_or([1.0, 0.0, 0.0]);
        let up = cross(side, forward);

        [
            side[0],
            up[0],
            -forward[0],
            0.0,
            side[1],
            up[1],
            -forward[1],
            0.0,
            side[2],
            up[2],
            -forward[2],
            0.0,
            -dot(side, self.eye),
            -dot(up, self.eye),
            dot(forward, self.eye),
            1.0,
        ]
    }

    /// Perspective with depth in 0..1, which is what wgpu expects.
    fn projection(&self) -> [f32; 16] {
        let focal = 1.0 / (self.fov * 0.5).tan();
        let depth = self.far - self.near;
        // Guarded so a degenerate frustum cannot divide by zero. A camera whose
        // near and far have met shows nothing either way; what matters is that
        // it shows nothing rather than writing a NaN into a uniform.
        let (scale, offset) = if depth > f32::EPSILON {
            // `view` is right-handed: points in front have negative Z. wgpu's
            // depth interval is 0..1, hence clip.w = -view.z and the negative
            // depth scale. The opposite signs put the whole model behind the
            // clip volume even though every matrix entry remains finite.
            (-self.far / depth, -self.far * self.near / depth)
        } else {
            (-1.0, 0.0)
        };

        [
            focal / self.aspect(),
            0.0,
            0.0,
            0.0,
            0.0,
            focal,
            0.0,
            0.0,
            0.0,
            0.0,
            scale,
            -1.0,
            0.0,
            0.0,
            offset,
            0.0,
        ]
    }
}

fn multiply(left: &[f32; 16], right: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for column in 0..4 {
        for row in 0..4 {
            out[column * 4 + row] = (0..4)
                .map(|k| left[k * 4 + row] * right[column * 4 + k])
                .sum();
        }
    }
    out
}

fn sub(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

/// A unit vector, or `None` when there is no direction to normalise.
fn normalise(vector: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(vector, vector).sqrt();
    (length > f32::EPSILON && length.is_finite())
        .then(|| [vector[0] / length, vector[1] / length, vector[2] / length])
}
