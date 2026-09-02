// SPDX-License-Identifier: MIT
//! The one measured conversion between FerriteCAD and FBX.
//!
//! Everything the §22B-1a measurement settled about axes, units and
//! transforms is here, applied exactly once each. FerriteCAD is right-handed,
//! Z up, in millimetres. The measured FBX contract is right-handed, Y up,
//! with `Z` opposite forward and metre units, so the coordinate map is
//!
//! ```text
//! C(x, y, z) = (x, z, -y) * 0.001
//! ```
//!
//! Its rotation-only determinant is `+1`, which is why polygon order is kept
//! and no winding is reversed. A placement is converted by conjugation,
//! `M_fbx = C * M_fcad * C^-1`; the `0.001` cancels in the linear part and
//! survives in the translation, which is exactly the mm-to-metre step.
//!
//! # Why the numbers written here are quantised
//!
//! A decomposition needs `asin` and `atan2`, and an sRGB transfer needs
//! `powf`. Those come from the platform's maths library and may differ in the
//! last bit between Linux, macOS and Windows. Writing them raw would make the
//! bytes a property of the machine, so each is rounded to a fixed number of
//! decimals: nine for an angle in degrees, which is far finer than the
//! tolerance the recomposition is checked against, and six for a colour,
//! which is finer than the float a renderer stores it in.

use ferritecad_types::{CadError, Result};

use crate::scene::{ExportTransform, TRANSFORM_TOLERANCE};

/// What one FBX metre is, in the centimetres the format counts in.
pub(crate) const UNIT_SCALE_FACTOR: f64 = 100.0;

/// FerriteCAD millimetres to FBX metres.
///
/// A divisor rather than a factor of `0.001`, because `0.001` is not a double
/// and multiplying by it turns 300 mm into `0.30000000000000004` where
/// dividing by 1000 gives the nearest double to `0.3`. Both are within any
/// tolerance that matters; only one of them writes the number a person
/// entered.
const MILLIMETRES_PER_METRE: f64 = 1000.0;

/// How many decimals of a degree survive into the file.
const ANGLE_DECIMALS: f64 = 1.0e9;

/// How many decimals of a colour component survive into the file.
const COLOUR_DECIMALS: f64 = 1.0e6;

/// How finely a uniform scale is recorded.
///
/// Not for determinism — a square root is exact on every platform — but
/// because a rotation built from angles has column lengths of
/// `0.9999999999999999`, and a file recording every node of an assembly as
/// scaled by that is recording rounding noise as geometry. Twelve decimals is
/// two orders finer than the tolerance the recomposition is checked against,
/// so nothing a placement actually says can be quantised away unnoticed.
const SCALE_DECIMALS: f64 = 1.0e12;

/// Below this the second and third angle are the same rotation, so the third
/// is chosen rather than computed.
const GIMBAL_LOCK: f64 = 1.0e-9;

/// A placement as FBX records one: a translation, three angles in one
/// declared order, and a uniform scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Trs {
    pub(crate) translation: [f64; 3],
    /// Degrees, applied in the FBX `eEulerXYZ` order: `Rz * Ry * Rx`.
    pub(crate) rotation_degrees: [f64; 3],
    pub(crate) scale: f64,
}

/// One position, converted once.
pub(crate) fn point(value: [f32; 3]) -> [f64; 3] {
    let [x, y, z] = value.map(f64::from);
    [
        x / MILLIMETRES_PER_METRE,
        z / MILLIMETRES_PER_METRE,
        -y / MILLIMETRES_PER_METRE,
    ]
}

/// One direction, converted once. A normal is not a position and is not
/// scaled: the map's rotation is all that applies to it.
pub(crate) fn direction(value: [f32; 3]) -> [f64; 3] {
    let [x, y, z] = value.map(f64::from);
    [x, z, -y]
}

/// One linear intensity as the display value FBX records.
///
/// The standard piecewise sRGB transfer, applied once at this boundary
/// because FerriteCAD stores linear RGB and the measurement showed Unity
/// reading FBX base colour as a display value.
pub(crate) fn srgb(linear: f64) -> Result<f64> {
    if !linear.is_finite() || !(0.0..=1.0).contains(&linear) {
        return Err(CadError::unsupported(format!(
            "a colour component is {linear}, outside the measured linear range from 0 to 1"
        )));
    }
    let encoded = if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    Ok(quantise(encoded, COLOUR_DECIMALS))
}

/// One placement, converted and decomposed.
pub(crate) fn local_transform(transform: &ExportTransform) -> Result<Trs> {
    let rows = transform.rows();
    let linear = [
        [rows[0][0], rows[0][1], rows[0][2]],
        [rows[1][0], rows[1][1], rows[1][2]],
        [rows[2][0], rows[2][1], rows[2][2]],
    ];
    decompose(
        conjugate(linear),
        converted_translation(transform.translation()),
    )
}

/// `C * t`, which is where millimetres become metres.
fn converted_translation(translation: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = translation;
    [
        x / MILLIMETRES_PER_METRE,
        z / MILLIMETRES_PER_METRE,
        -y / MILLIMETRES_PER_METRE,
    ]
}

/// `C * m * C^-1` for the rotation-only part of `C`, whose inverse is its
/// transpose. Every entry of the axis map is `0`, `1` or `-1`, so this only
/// permutes and negates and introduces no rounding of its own.
fn conjugate(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    // C = [[1, 0, 0], [0, 0, 1], [0, -1, 0]], written out rather than
    // multiplied so the map is readable as the map it is.
    let permuted = [
        [m[0][0], m[0][1], m[0][2]],
        [m[2][0], m[2][1], m[2][2]],
        [-m[1][0], -m[1][1], -m[1][2]],
    ];
    [
        [permuted[0][0], permuted[0][2], -permuted[0][1]],
        [permuted[1][0], permuted[1][2], -permuted[1][1]],
        [permuted[2][0], permuted[2][2], -permuted[2][1]],
    ]
}

/// Splits a converted placement into the translation, rotation and scale FBX
/// stores, and refuses one the three of them cannot rebuild.
///
/// The check is the point. A decomposition that is merely computed is a
/// guess; one whose recomposition is compared with the matrix it came from
/// either reproduces the placement or says that it could not, and a shear, a
/// reflection and a non-uniform scale all fail it.
pub(crate) fn decompose(linear: [[f64; 3]; 3], translation: [f64; 3]) -> Result<Trs> {
    for value in linear.iter().flatten().chain(translation.iter()) {
        if !value.is_finite() {
            return Err(CadError::unsupported(format!(
                "a converted placement holds {value}, which is not a position"
            )));
        }
    }

    let measured = (linear[0][0].powi(2) + linear[1][0].powi(2) + linear[2][0].powi(2)).sqrt();
    let scale = quantise(measured, SCALE_DECIMALS);
    if !measured.is_finite() || measured <= 0.0 || scale <= 0.0 {
        return Err(CadError::unsupported(format!(
            "a converted placement scales by {measured}, so it has no rotation to write down"
        )));
    }
    let rotation = linear.map(|row| row.map(|value| value / measured));

    // One declared order, FBX `eEulerXYZ`: the matrix is `Rz * Ry * Rx`.
    let sin_y = (-rotation[2][0]).clamp(-1.0, 1.0);
    let y = sin_y.asin();
    let cos_y = y.cos();
    let (x, z) = if cos_y.abs() > GIMBAL_LOCK {
        (
            rotation[2][1].atan2(rotation[2][2]),
            rotation[1][0].atan2(rotation[0][0]),
        )
    } else {
        // The second angle has turned the first and third onto one axis, so
        // only their difference is determined. Naming the third zero is what
        // makes the answer a function of the matrix rather than of rounding.
        ((-rotation[1][2]).atan2(rotation[1][1]), 0.0)
    };

    let rotation_degrees = [x, y, z].map(|angle| quantise(angle.to_degrees(), ANGLE_DECIMALS));
    let trs = Trs {
        translation,
        rotation_degrees,
        scale,
    };

    let rebuilt = euler_xyz(rotation_degrees);
    let tolerance = TRANSFORM_TOLERANCE * scale.max(1.0);
    for row in 0..3 {
        for column in 0..3 {
            let difference = (rebuilt[row][column] * scale - linear[row][column]).abs();
            if difference > tolerance {
                return Err(CadError::unsupported(format!(
                    "a placement does not survive being written as a translation, three angles \
                     and one scale: rebuilding it changes element {row},{column} by {difference}"
                )));
            }
        }
    }
    Ok(trs)
}

/// `Rz * Ry * Rx`, the one rotation order this writer declares.
fn euler_xyz(degrees: [f64; 3]) -> [[f64; 3]; 3] {
    let [x, y, z] = degrees.map(f64::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
        [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
        [-sy, cy * sx, cy * cx],
    ]
}

/// Rounds to a fixed number of decimals, so a value computed by the platform's
/// maths library becomes one every platform writes the same way.
fn quantise(value: f64, decimals: f64) -> f64 {
    let rounded = (value * decimals).round() / decimals;
    if rounded == 0.0 { 0.0 } else { rounded }
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use super::*;
    use ferritecad_types::ErrorKind;

    fn rotation(degrees: [f64; 3]) -> [[f64; 3]; 3] {
        euler_xyz(degrees)
    }

    fn placement(translation: [f64; 3], degrees: [f64; 3], scale: f64) -> ExportTransform {
        let linear = rotation(degrees);
        ExportTransform::new([
            [
                linear[0][0] * scale,
                linear[0][1] * scale,
                linear[0][2] * scale,
                translation[0],
            ],
            [
                linear[1][0] * scale,
                linear[1][1] * scale,
                linear[1][2] * scale,
                translation[1],
            ],
            [
                linear[2][0] * scale,
                linear[2][1] * scale,
                linear[2][2] * scale,
                translation[2],
            ],
        ])
        .expect("a rotation, a translation and a uniform scale are representable")
    }

    /// The converted matrix a placement should have become, computed the long
    /// way so the gate is not the implementation.
    fn expected(transform: &ExportTransform) -> ([[f64; 3]; 3], [f64; 3]) {
        let rows = transform.rows();
        let c = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];
        let mut left = [[0.0; 3]; 3];
        for row in 0..3 {
            for column in 0..3 {
                left[row][column] = (0..3).map(|k| c[row][k] * rows[k][column]).sum();
            }
        }
        let mut linear = [[0.0; 3]; 3];
        for row in 0..3 {
            for column in 0..3 {
                linear[row][column] = (0..3).map(|k| left[row][k] * c[column][k]).sum();
            }
        }
        let t = transform.translation();
        let translation = [
            (0..3).map(|k| c[0][k] * t[k]).sum::<f64>() * 0.001,
            (0..3).map(|k| c[1][k] * t[k]).sum::<f64>() * 0.001,
            (0..3).map(|k| c[2][k] * t[k]).sum::<f64>() * 0.001,
        ];
        (linear, translation)
    }

    fn rebuilds(transform: &ExportTransform) {
        let trs = local_transform(transform).expect("a representable placement");
        let (linear, translation) = expected(transform);
        for (axis, (written, wanted)) in trs.translation.iter().zip(translation).enumerate() {
            assert!(
                (written - wanted).abs() <= TRANSFORM_TOLERANCE,
                "translation {axis}: {written} is not {wanted}"
            );
        }
        let rebuilt = euler_xyz(trs.rotation_degrees);
        for row in 0..3 {
            for column in 0..3 {
                let difference = (rebuilt[row][column] * trs.scale - linear[row][column]).abs();
                assert!(
                    difference <= TRANSFORM_TOLERANCE * trs.scale.max(1.0),
                    "element {row},{column} differs by {difference}"
                );
            }
        }
    }

    #[test]
    fn the_axis_map_is_the_one_that_was_measured() {
        assert_eq!(point([1000.0, 0.0, 0.0]), [1.0, 0.0, -0.0]);
        assert_eq!(point([0.0, 2000.0, 0.0]), [0.0, 0.0, -2.0]);
        assert_eq!(point([0.0, 0.0, 3000.0]), [0.0, 3.0, -0.0]);
        // Dividing by a thousand rather than multiplying by an inexact
        // `0.001`: 300 mm is 0.3 m and not 0.30000000000000004 m.
        assert_eq!(point([100.0, 200.0, 300.0]), [0.1, 0.3, -0.2]);
        assert_eq!(direction([1.0, 0.0, 0.0]), [1.0, 0.0, -0.0]);
        assert_eq!(direction([0.0, 1.0, 0.0]), [0.0, 0.0, -1.0]);
        assert_eq!(direction([0.0, 0.0, 1.0]), [0.0, 1.0, -0.0]);
        // A direction is not scaled: only a position carries the unit.
        assert_eq!(direction([0.0, 3.0, 0.0]), [0.0, 0.0, -3.0]);
    }

    #[test]
    fn every_placement_the_corpus_can_hold_survives_being_decomposed() {
        // Identity, the three pure translations, the three pure rotations,
        // combinations of them, a uniform scale and the measured portrait
        // transforms.
        rebuilds(&ExportTransform::IDENTITY);
        for axis in 0..3 {
            let mut translation = [0.0; 3];
            translation[axis] = 1234.5;
            rebuilds(&placement(translation, [0.0; 3], 1.0));
        }
        for axis in 0..3 {
            for angle in [-179.0, -90.0, -37.0, 0.0, 1.0, 45.0, 90.0, 137.0, 180.0] {
                let mut degrees = [0.0; 3];
                degrees[axis] = angle;
                rebuilds(&placement([0.0; 3], degrees, 1.0));
            }
        }
        for degrees in [
            [11.0, 23.0, -17.0],
            [31.0, -19.0, 47.0],
            [-13.0, 29.0, -37.0],
            [7.0, 13.0, 29.0],
            [120.0, 60.0, -150.0],
        ] {
            rebuilds(&placement([100.0, 200.0, 300.0], degrees, 1.0));
        }
        for scale in [0.5, 1.0, 2.0, 1000.0] {
            rebuilds(&placement([1.0, -2.0, 3.0], [10.0, 20.0, 30.0], scale));
        }
    }

    #[test]
    fn the_gimbal_lock_boundary_is_decomposed_rather_than_guessed() {
        for y in [
            -90.0,
            90.0,
            90.0 - 1.0e-11,
            -90.0 + 1.0e-11,
            90.0 - 1.0e-7,
            89.999_999,
        ] {
            for other in [0.0, 37.0, -128.0] {
                let transform = placement([5.0, 6.0, 7.0], [other, y, other], 1.0);
                rebuilds(&transform);
            }
        }

        // At exactly ninety degrees about the second axis only the difference
        // of the first and third angle is determined, so the third is named
        // zero rather than guessed. Asked of the decomposition directly,
        // because a rotation that is gimbal-locked in FerriteCAD axes is not
        // the same rotation as the one the conversion produces.
        for second in [90.0, -90.0] {
            let locked = decompose(rotation([40.0, second, 25.0]), [0.0; 3])
                .expect("the boundary is representable");
            assert_eq!(locked.rotation_degrees[2], 0.0, "the third angle is named");
            assert!((locked.rotation_degrees[1].abs() - 90.0).abs() < 1.0e-6);
            // And the three values still rebuild the matrix they came from.
            let rebuilt = euler_xyz(locked.rotation_degrees);
            let source = rotation([40.0, second, 25.0]);
            for row in 0..3 {
                for column in 0..3 {
                    assert!(
                        (rebuilt[row][column] - source[row][column]).abs() <= TRANSFORM_TOLERANCE,
                        "element {row},{column} at the boundary"
                    );
                }
            }
        }
    }

    #[test]
    fn a_placement_the_three_values_cannot_rebuild_is_refused() {
        // These never reach the writer, because `ExportTransform` refuses them
        // first. The check exists so that the recomposition is a check rather
        // than an assumption, and so removing it is visible.
        let cases: [(&str, [[f64; 3]; 3]); 3] = [
            (
                "sheared",
                [[1.0, 0.5, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            ),
            (
                "reflected",
                [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            ),
            (
                "non-uniform",
                [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            ),
        ];
        for (what, linear) in cases {
            let Err(error) = decompose(linear, [0.0; 3]) else {
                panic!("a {what} placement was written as a translation, angles and a scale");
            };
            assert_eq!(error.kind(), ErrorKind::Unsupported, "{what}");
        }
        assert!(decompose([[0.0; 3]; 3], [0.0; 3]).is_err(), "collapsed");
        assert!(
            decompose(
                [[f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                [0.0; 3]
            )
            .is_err()
        );
    }

    #[test]
    fn the_colour_transfer_is_the_measured_one_and_is_applied_once() {
        // The two measured base colours, in the linear form FerriteCAD stores
        // and the display form Unity read back.
        for (linear, display) in [
            (0.603_827, 0.8),
            (0.033_105, 0.2),
            (0.010_023, 0.1),
            (0.100_482, 0.35),
            (0.787_412, 0.9),
        ] {
            let encoded = srgb(linear).expect("a finite intensity");
            assert!(
                (encoded - display).abs() < 1.0e-4,
                "{linear} encoded as {encoded} rather than {display}"
            );
        }
        assert_eq!(srgb(0.0).expect("black"), 0.0);
        assert_eq!(srgb(1.0).expect("white"), 1.0);
        assert!(srgb(2.0).is_err(), "an HDR value was silently clamped");
        assert!(srgb(-0.1).is_err(), "a negative value was silently clamped");
        assert!(srgb(f64::NAN).is_err());
    }

    #[test]
    fn what_the_platform_computes_is_rounded_to_what_every_platform_writes() {
        // Nine decimals of a degree is far finer than the tolerance the
        // recomposition is checked against, and six of a colour is finer than
        // the float a renderer keeps it in.
        let trs =
            local_transform(&placement([0.0; 3], [11.0, 23.0, -17.0], 1.0)).expect("a rotation");
        for angle in trs.rotation_degrees {
            assert_eq!(
                quantise(angle, ANGLE_DECIMALS),
                angle,
                "an angle is not rounded"
            );
        }
        let colour = srgb(0.603_827).expect("a colour");
        assert_eq!(quantise(colour, COLOUR_DECIMALS), colour);
        assert_eq!(quantise(-0.0, ANGLE_DECIMALS), 0.0);

        // A rotation built from angles has column lengths a hair under one,
        // and a file must not record that as a scale.
        assert_eq!(trs.scale, 1.0, "rounding noise reached the scale");
        let scaled = decompose(
            rotation([5.0, 6.0, 7.0]).map(|row| row.map(|value| value * 2.5)),
            [0.0; 3],
        )
        .expect("a uniform scale");
        assert_eq!(scaled.scale, 2.5, "a real scale is not quantised away");
    }
}
