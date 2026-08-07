// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};

use crate::error::CadError;
use crate::hash::{CanonicalHasher, normalize_f64};

/// The tolerances a computation was carried out with.
///
/// These are carried explicitly rather than left to a kernel default, because
/// they are part of every cache key: the same inputs at a different tolerance
/// are a different result, not a reusable one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ToleranceWire")]
pub struct Tolerance {
    /// Distance below which two positions are the same point, in millimetres.
    linear: f64,
    /// Angle below which two directions are the same direction, in radians.
    angular: f64,
}

#[derive(Deserialize)]
struct ToleranceWire {
    linear: f64,
    angular: f64,
}

impl TryFrom<ToleranceWire> for Tolerance {
    type Error = CadError;

    fn try_from(value: ToleranceWire) -> Result<Self, Self::Error> {
        Self::new(value.linear, value.angular)
    }
}

impl Tolerance {
    /// Matches Open CASCADE's `Precision::Confusion` and `Precision::Angular`
    /// at a model scale of millimetres, so adapter results and our own
    /// comparisons agree by default.
    pub const DEFAULT_LINEAR: f64 = 1.0e-7;
    pub const DEFAULT_ANGULAR: f64 = 1.0e-12;

    pub fn new(linear: f64, angular: f64) -> Result<Self, CadError> {
        let linear = normalize_f64(linear)?;
        let angular = normalize_f64(angular)?;
        if linear <= 0.0 || angular <= 0.0 {
            return Err(CadError::input(format!(
                "tolerances must be positive, got linear {linear} and angular {angular}"
            )));
        }
        Ok(Self { linear, angular })
    }

    pub fn linear(self) -> f64 {
        self.linear
    }

    pub fn angular(self) -> f64 {
        self.angular
    }

    /// Feeds the tolerances into a cache key.
    pub fn feed(self, hasher: &mut CanonicalHasher) {
        const VALIDATED: &str = "tolerance components are validated finite on construction";
        hasher.field("tolerance");
        hasher.f64(self.linear).expect(VALIDATED);
        hasher.f64(self.angular).expect(VALIDATED);
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            linear: Self::DEFAULT_LINEAR,
            angular: Self::DEFAULT_ANGULAR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_positive_tolerances_are_refused() {
        assert!(Tolerance::new(0.0, 1e-9).is_err());
        assert!(Tolerance::new(1e-7, -1e-9).is_err());
        assert!(Tolerance::new(f64::NAN, 1e-9).is_err());
    }

    #[test]
    fn tolerance_participates_in_the_cache_key() {
        let mut coarse = CanonicalHasher::new("test");
        Tolerance::new(1e-5, 1e-9)
            .expect("positive")
            .feed(&mut coarse);

        let mut fine = CanonicalHasher::new("test");
        Tolerance::default().feed(&mut fine);

        assert_ne!(coarse.finish(), fine.finish());
    }
}
