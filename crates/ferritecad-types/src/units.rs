// SPDX-License-Identifier: MIT
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::CadError;
use crate::hash::normalize_f64;

/// What a quantity measures.
///
/// Conversions are only defined within a dimension: asking for a length in
/// degrees is a programming error and is reported as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Dimension {
    Length,
    Angle,
    /// A dimensionless count or ratio, such as a pattern instance count.
    Scalar,
}

impl Dimension {
    /// The unit this dimension is stored in internally.
    pub fn base_unit(self) -> Unit {
        match self {
            Self::Length => Unit::Millimeter,
            Self::Angle => Unit::Radian,
            Self::Scalar => Unit::Unitless,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::Angle => "angle",
            Self::Scalar => "scalar",
        }
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A unit a user may type or read.
///
/// Units exist at the boundary only. Nothing inside the model stores a unit
/// alongside a value: internal lengths are millimetres and internal angles are
/// radians, always.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Unit {
    Millimeter,
    Centimeter,
    Meter,
    Inch,
    Foot,
    Radian,
    Degree,
    Unitless,
}

impl Unit {
    pub fn dimension(self) -> Dimension {
        match self {
            Self::Millimeter | Self::Centimeter | Self::Meter | Self::Inch | Self::Foot => {
                Dimension::Length
            }
            Self::Radian | Self::Degree => Dimension::Angle,
            Self::Unitless => Dimension::Scalar,
        }
    }

    /// How many base units one of this unit is worth.
    ///
    /// The inch is exactly 25.4 mm by definition, so lengths convert without
    /// an approximation constant.
    pub fn per_base_unit(self) -> f64 {
        match self {
            Self::Millimeter => 1.0,
            Self::Centimeter => 10.0,
            Self::Meter => 1000.0,
            Self::Inch => 25.4,
            Self::Foot => 304.8,
            Self::Radian => 1.0,
            Self::Degree => std::f64::consts::PI / 180.0,
            Self::Unitless => 1.0,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Millimeter => "mm",
            Self::Centimeter => "cm",
            Self::Meter => "m",
            Self::Inch => "in",
            Self::Foot => "ft",
            Self::Radian => "rad",
            Self::Degree => "deg",
            Self::Unitless => "",
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

impl FromStr for Unit {
    type Err = CadError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "mm" | "millimeter" | "millimetre" => Ok(Self::Millimeter),
            "cm" | "centimeter" | "centimetre" => Ok(Self::Centimeter),
            "m" | "meter" | "metre" => Ok(Self::Meter),
            "in" | "inch" => Ok(Self::Inch),
            "ft" | "foot" | "feet" => Ok(Self::Foot),
            "rad" | "radian" => Ok(Self::Radian),
            "deg" | "degree" | "°" => Ok(Self::Degree),
            "" | "none" | "unitless" => Ok(Self::Unitless),
            other => Err(CadError::input(format!("unknown unit {other:?}"))),
        }
    }
}

/// A finite value held in internal units.
///
/// Construction goes through a unit so the conversion happens exactly once, at
/// the point the value enters the system.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "QuantityWire")]
pub struct Quantity {
    /// Millimetres, radians, or a bare number, per `dimension`.
    base_value: f64,
    dimension: Dimension,
}

#[derive(Deserialize)]
struct QuantityWire {
    base_value: f64,
    dimension: Dimension,
}

impl TryFrom<QuantityWire> for Quantity {
    type Error = CadError;

    fn try_from(value: QuantityWire) -> Result<Self, Self::Error> {
        Self::from_base(value.base_value, value.dimension)
    }
}

impl Quantity {
    /// Converts `value` from `unit` into internal units.
    pub fn new(value: f64, unit: Unit) -> Result<Self, CadError> {
        let base_value = normalize_f64(value * unit.per_base_unit())?;
        Ok(Self {
            base_value,
            dimension: unit.dimension(),
        })
    }

    /// Takes a value that is already in internal units.
    pub fn from_base(base_value: f64, dimension: Dimension) -> Result<Self, CadError> {
        Ok(Self {
            base_value: normalize_f64(base_value)?,
            dimension,
        })
    }

    pub fn base_value(self) -> f64 {
        self.base_value
    }

    pub fn dimension(self) -> Dimension {
        self.dimension
    }

    /// Converts to `unit` for display, refusing a cross-dimension conversion.
    pub fn value_in(self, unit: Unit) -> Result<f64, CadError> {
        if unit.dimension() != self.dimension {
            return Err(CadError::input(format!(
                "cannot express a {} as {}",
                self.dimension,
                unit.dimension()
            )));
        }
        Ok(self.base_value / unit.per_base_unit())
    }
}

/// The counter-clockwise turn from the first vector of a directed angle to the
/// second, held in radians and canonical in `(-π, π]` (§31F).
///
/// This is the unit boundary: degrees live in the UI and the CLI, radians in
/// the document and the solver, and [`DirectedAngle::from_degrees`] and
/// [`DirectedAngle::degrees`] are the only road between them. The input is one
/// full circle, `[-180, 180]`, and everything outside it is refused rather
/// than wrapped: `270` is not `-90`, because a policy chosen by floating-point
/// arithmetic is no policy. Exactly `-180` is the same direction as `180` and
/// is stored only as `+180`; `-0` is `0`. Private validated bits make exact
/// `Eq` safe: NaN and the second zero are excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectedAngle(u64);
impl DirectedAngle {
    pub fn from_degrees(degrees: f64) -> crate::Result<Self> {
        if !degrees.is_finite() || !(-180. ..=180.).contains(&degrees) {
            return Err(CadError::input(
                "a directed angle is a counter-clockwise turn in degrees from -180 to 180 \
                 (-180 is stored as 180); no other value is wrapped into that range",
            ));
        }
        let radians = if degrees.abs() == 180. {
            std::f64::consts::PI
        } else if degrees == 0. {
            0.
        } else {
            degrees * std::f64::consts::PI / 180.
        };
        if degrees != 0. && radians == 0. {
            return Err(CadError::input(
                "this nonzero directed angle is too small to represent in radians",
            ));
        }
        Ok(Self(radians.to_bits()))
    }
    /// A stored value read back, which must already be canonical: radians in
    /// `(-π, π]`. `-0` is `0`. Nothing is wrapped.
    pub fn from_radians(radians: f64) -> crate::Result<Self> {
        if !radians.is_finite()
            || radians <= -std::f64::consts::PI
            || radians > std::f64::consts::PI
        {
            return Err(CadError::input(
                "a stored directed angle is canonical in (-pi, pi] radians",
            ));
        }
        Ok(Self((radians + 0.).to_bits()))
    }
    /// The stored value, radians in `(-π, π]`.
    pub fn radians(self) -> f64 {
        f64::from_bits(self.0)
    }
    /// The value for display and for the CLI, in degrees. Prefer the shorter
    /// value rounded to `1e-9°` only if reading it back gives exactly the same
    /// stored radians. A distinct small angle or near-half-turn must not be
    /// reported as zero or the opposite direction.
    pub fn degrees(self) -> f64 {
        let degrees = self.radians() * 180. / std::f64::consts::PI;
        let shorter = (degrees * 1e9).round() / 1e9 + 0.;
        if shorter > -180. && Self::from_degrees(shorter).is_ok_and(|angle| angle == self) {
            shorter
        } else {
            degrees
        }
    }
}

/// The degrees the UI and the CLI show for an angle they stored as
/// `degrees.to_radians()`: the shortest decimal that converts back to exactly
/// `radians`. A typed 30° is stored as radians that convert straight back to
/// 29.999999999999996°; this shows 30, and taking 30 again stores the same
/// radians.
pub fn display_degrees(radians: f64) -> f64 {
    let degrees = radians.to_degrees();
    (0..=15)
        .map(|places| {
            let scale = 10_f64.powi(places);
            (degrees * scale).round() / scale
        })
        .find(|shown| shown.to_radians() == radians)
        .unwrap_or(degrees)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inch_is_exactly_25_4_mm() {
        let q = Quantity::new(1.0, Unit::Inch).expect("finite");
        assert_eq!(q.base_value(), 25.4);
    }

    #[test]
    fn round_trips_through_a_display_unit() {
        let q = Quantity::new(2.5, Unit::Inch).expect("finite");
        let back = q.value_in(Unit::Inch).expect("same dimension");
        assert!((back - 2.5).abs() < 1e-12);
    }

    #[test]
    fn half_turn_is_pi_radians() {
        let q = Quantity::new(180.0, Unit::Degree).expect("finite");
        assert!((q.base_value() - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn cross_dimension_conversion_is_refused() {
        let length = Quantity::new(10.0, Unit::Millimeter).expect("finite");
        assert!(length.value_in(Unit::Degree).is_err());
    }

    #[test]
    fn non_finite_input_is_refused() {
        assert!(Quantity::new(f64::NAN, Unit::Millimeter).is_err());
        assert!(Quantity::new(f64::INFINITY, Unit::Millimeter).is_err());
    }

    #[test]
    fn displayed_degrees_are_the_degrees_typed() {
        // Every angle typed to two places, 0.01° to 359.99°: 30 is shown as 30.
        for hundredths in 1..36_000 {
            let typed = f64::from(hundredths) / 100.0;
            assert_eq!(display_degrees(typed.to_radians()), typed, "{typed}");
        }
        let third = 100.0 / 3.0;
        assert_eq!(display_degrees(f64::to_radians(third)), third);
    }

    #[test]
    fn unit_parsing_accepts_both_spellings() {
        assert_eq!("MM".parse::<Unit>().expect("known"), Unit::Millimeter);
        assert_eq!("metre".parse::<Unit>().expect("known"), Unit::Meter);
        assert_eq!("meter".parse::<Unit>().expect("known"), Unit::Meter);
        assert!("furlong".parse::<Unit>().is_err());
    }
}
