// SPDX-License-Identifier: MIT
//! Camera geometry for tools working on a model-space plane.
use super::{Camera, Projection, cross, normalise};

impl Camera {
    /// Looks from `direction` towards the existing target with `up` upwards.
    /// Keeps distance and projection; refuses a degenerate or non-finite frame.
    pub fn look_along(&mut self, direction: [f32; 3], up: [f32; 3]) -> bool {
        let (Some(direction), Some(up)) = (normalise(direction), normalise(up)) else {
            return false;
        };
        if normalise(cross(direction, up)).is_none() {
            return false;
        }
        self.look_from_frame(direction, up)
    }

    pub(super) fn look_from_frame(&mut self, direction: [f32; 3], up: [f32; 3]) -> bool {
        let distance = self.distance();
        let distance = if distance.is_finite() && distance > f32::EPSILON {
            distance
        } else {
            self.radius.max(f32::EPSILON)
        };
        let eye = std::array::from_fn(|i| self.target[i] + direction[i] * distance);
        self.accept_pose(eye, self.target, up)
    }

    /// A model-space ray through a viewport-local pixel, evaluated in f64.
    /// The direction is unit length. Empty viewports and non-finite pixels refuse.
    pub fn pixel_ray(&self, pixel: [f64; 2]) -> Option<([f64; 3], [f64; 3])> {
        if !self.is_drawable() || !pixel.iter().all(|v| v.is_finite()) {
            return None;
        }
        let (right, up) = self.screen_axes();
        let eye = self.eye.map(f64::from);
        let forward = self.direction().map(|v| -f64::from(v));
        let scale = match self.projection {
            Projection::Perspective => {
                2.0 * (f64::from(self.fov) * 0.5).tan() / f64::from(self.height)
            }
            Projection::Orthographic => 2.0 * f64::from(self.half_height) / f64::from(self.height),
        };
        let x = (pixel[0] - f64::from(self.width) * 0.5) * scale;
        let y = (f64::from(self.height) * 0.5 - pixel[1]) * scale;
        let offset: [f64; 3] =
            std::array::from_fn(|i| f64::from(right[i]) * x + f64::from(up[i]) * y);
        match self.projection {
            Projection::Perspective => {
                let ray: [f64; 3] = std::array::from_fn(|i| forward[i] + offset[i]);
                let length = ray[0].hypot(ray[1]).hypot(ray[2]);
                Some((eye, ray.map(|v| v / length)))
            }
            Projection::Orthographic => {
                Some((std::array::from_fn(|i| eye[i] + offset[i]), forward))
            }
        }
    }

    /// Projects a model-space point into viewport-local pixels in f64.
    /// Points behind the camera or in an empty viewport have no screen position.
    pub fn project_point(&self, point: [f64; 3]) -> Option<[f64; 2]> {
        if !self.is_drawable() || !point.iter().all(|v| v.is_finite()) {
            return None;
        }
        let (right, up) = self.screen_axes();
        let delta: [f64; 3] = std::array::from_fn(|i| point[i] - f64::from(self.eye[i]));
        let dot = |axis: [f32; 3]| (0..3).map(|i| delta[i] * f64::from(axis[i])).sum::<f64>();
        let depth = -dot(self.direction());
        if depth <= 0.0 {
            return None;
        }
        let half = match self.projection {
            Projection::Perspective => depth * (f64::from(self.fov) * 0.5).tan(),
            Projection::Orthographic => f64::from(self.half_height),
        };
        let scale = f64::from(self.height) / (2.0 * half);
        Some([
            f64::from(self.width) * 0.5 + dot(right) * scale,
            f64::from(self.height) * 0.5 - dot(up) * scale,
        ])
    }
}
