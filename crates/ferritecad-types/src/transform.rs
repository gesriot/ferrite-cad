// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};

use crate::error::CadError;
use crate::hash::{CanonicalHasher, normalize_f64};

/// A position in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Point3Wire")]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Deserialize)]
struct Point3Wire {
    x: f64,
    y: f64,
    z: f64,
}

impl TryFrom<Point3Wire> for Point3 {
    type Error = CadError;

    fn try_from(value: Point3Wire) -> Result<Self, Self::Error> {
        Self::new(value.x, value.y, value.z)
    }
}

/// A displacement or direction in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec3Wire")]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Deserialize)]
struct Vec3Wire {
    x: f64,
    y: f64,
    z: f64,
}

impl TryFrom<Vec3Wire> for Vec3 {
    type Error = CadError;

    fn try_from(value: Vec3Wire) -> Result<Self, Self::Error> {
        Self::new(value.x, value.y, value.z)
    }
}

impl Point3 {
    pub const ORIGIN: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, CadError> {
        Ok(Self {
            x: normalize_f64(x)?,
            y: normalize_f64(y)?,
            z: normalize_f64(z)?,
        })
    }
}

impl Vec3 {
    pub const X: Self = Self {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    pub const Y: Self = Self {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    pub const Z: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };

    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, CadError> {
        Ok(Self {
            x: normalize_f64(x)?,
            y: normalize_f64(y)?,
            z: normalize_f64(z)?,
        })
    }

    pub fn length(self) -> f64 {
        self.x.hypot(self.y).hypot(self.z)
    }

    /// Scales to unit length, refusing a vector too short to have a direction.
    pub fn normalized(self) -> Result<Self, CadError> {
        let length = self.length();
        if length < f64::EPSILON {
            return Err(CadError::input(
                "a zero-length vector has no direction".to_string(),
            ));
        }
        Self::new(self.x / length, self.y / length, self.z / length)
    }

    pub fn cross(self, other: Self) -> Result<Self, CadError> {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }
}

/// An affine placement, stored row-major as three rows of four columns.
///
/// The implicit fourth row is `[0, 0, 0, 1]`, so this represents rotation,
/// scaling and translation but not projection. It is used for datum placements
/// and feature transforms; it is not a general matrix type and deliberately
/// offers no inverse — a placement that needs inverting should be stored the
/// way it is used.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TransformWire")]
pub struct Transform {
    rows: [[f64; 4]; 3],
}

#[derive(Deserialize)]
struct TransformWire {
    rows: [[f64; 4]; 3],
}

impl TryFrom<TransformWire> for Transform {
    type Error = CadError;

    fn try_from(value: TransformWire) -> Result<Self, Self::Error> {
        Self::from_rows(value.rows)
    }
}

impl Transform {
    pub const IDENTITY: Self = Self {
        rows: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    };

    pub fn from_rows(rows: [[f64; 4]; 3]) -> Result<Self, CadError> {
        let mut normalized = rows;
        for row in &mut normalized {
            for value in row.iter_mut() {
                *value = normalize_f64(*value)?;
            }
        }
        Ok(Self { rows: normalized })
    }

    pub fn from_translation(offset: Vec3) -> Result<Self, CadError> {
        Self::from_rows([
            [1.0, 0.0, 0.0, offset.x],
            [0.0, 1.0, 0.0, offset.y],
            [0.0, 0.0, 1.0, offset.z],
        ])
    }

    /// Right-handed rotation of `angle` radians about `axis` through the
    /// origin, by Rodrigues' formula.
    pub fn from_rotation(axis: Vec3, angle: f64) -> Result<Self, CadError> {
        let axis = axis.normalized()?;
        let angle = normalize_f64(angle)?;
        let (sin, cos) = angle.sin_cos();
        let inv = 1.0 - cos;
        let (x, y, z) = (axis.x, axis.y, axis.z);

        Self::from_rows([
            [
                cos + x * x * inv,
                x * y * inv - z * sin,
                x * z * inv + y * sin,
                0.0,
            ],
            [
                y * x * inv + z * sin,
                cos + y * y * inv,
                y * z * inv - x * sin,
                0.0,
            ],
            [
                z * x * inv - y * sin,
                z * y * inv + x * sin,
                cos + z * z * inv,
                0.0,
            ],
        ])
    }

    pub fn rows(&self) -> &[[f64; 4]; 3] {
        &self.rows
    }

    /// Returns the transform that applies `self` and then `outer`.
    pub fn then(&self, outer: &Self) -> Result<Self, CadError> {
        let mut rows = [[0.0f64; 4]; 3];
        for (i, row) in rows.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                let mut sum = (0..3)
                    .map(|k| outer.rows[i][k] * self.rows[k][j])
                    .sum::<f64>();
                if j == 3 {
                    sum += outer.rows[i][3];
                }
                *value = sum;
            }
        }
        Self::from_rows(rows)
    }

    pub fn apply_to_point(&self, point: Point3) -> Result<Point3, CadError> {
        let coords = [point.x, point.y, point.z];
        let mut out = [0.0f64; 3];
        for (i, value) in out.iter_mut().enumerate() {
            *value = (0..3).map(|k| self.rows[i][k] * coords[k]).sum::<f64>() + self.rows[i][3];
        }
        Point3::new(out[0], out[1], out[2])
    }

    /// Applies the linear part only, ignoring translation.
    pub fn apply_to_vector(&self, vector: Vec3) -> Result<Vec3, CadError> {
        let coords = [vector.x, vector.y, vector.z];
        let mut out = [0.0f64; 3];
        for (i, value) in out.iter_mut().enumerate() {
            *value = (0..3).map(|k| self.rows[i][k] * coords[k]).sum::<f64>();
        }
        Vec3::new(out[0], out[1], out[2])
    }

    /// Feeds the placement into a cache key.
    pub fn feed(&self, hasher: &mut CanonicalHasher) {
        hasher.field("transform");
        for row in &self.rows {
            for value in row {
                hasher
                    .f64(*value)
                    .expect("transform components are validated finite on construction");
            }
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-12, "{a} != {b}");
    }

    #[test]
    fn identity_leaves_a_point_alone() {
        let point = Point3::new(1.0, 2.0, 3.0).expect("finite");
        let moved = Transform::IDENTITY.apply_to_point(point).expect("finite");
        assert_eq!(moved, point);
    }

    #[test]
    fn translation_moves_points_but_not_directions() {
        let t =
            Transform::from_translation(Vec3::new(5.0, 0.0, 0.0).expect("finite")).expect("finite");
        let point = t
            .apply_to_point(Point3::new(1.0, 0.0, 0.0).expect("finite"))
            .expect("finite");
        assert_close(point.x, 6.0);

        let direction = t.apply_to_vector(Vec3::X).expect("finite");
        assert_eq!(direction, Vec3::X);
    }

    #[test]
    fn quarter_turn_about_z_maps_x_to_y() {
        let t = Transform::from_rotation(Vec3::Z, std::f64::consts::FRAC_PI_2).expect("finite");
        let rotated = t.apply_to_vector(Vec3::X).expect("finite");
        assert_close(rotated.x, 0.0);
        assert_close(rotated.y, 1.0);
        assert_close(rotated.z, 0.0);
    }

    #[test]
    fn composition_applies_self_then_outer() {
        let rotate = Transform::from_rotation(Vec3::Z, std::f64::consts::FRAC_PI_2).expect("ok");
        let translate =
            Transform::from_translation(Vec3::new(10.0, 0.0, 0.0).expect("finite")).expect("ok");

        // Rotate x onto y, then shift along x.
        let combined = rotate.then(&translate).expect("finite");
        let point = combined
            .apply_to_point(Point3::new(1.0, 0.0, 0.0).expect("finite"))
            .expect("finite");
        assert_close(point.x, 10.0);
        assert_close(point.y, 1.0);
    }

    #[test]
    fn zero_length_axis_is_refused() {
        let zero = Vec3::new(0.0, 0.0, 0.0).expect("finite");
        assert!(zero.normalized().is_err());
        assert!(Transform::from_rotation(zero, 1.0).is_err());
    }

    #[test]
    fn non_finite_components_are_refused() {
        assert!(Point3::new(f64::NAN, 0.0, 0.0).is_err());
        assert!(
            Transform::from_translation(Vec3 {
                x: f64::INFINITY,
                y: 0.0,
                z: 0.0
            })
            .is_err()
        );
    }
}
