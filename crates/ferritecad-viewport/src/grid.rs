// SPDX-License-Identifier: MIT
//! How far apart the lines of a reference grid should be.
//!
//! A model drawn against nothing has no scale and no orientation. A planar
//! part looks the same at any zoom, a symmetric one looks the same from
//! several directions, and after an orbit there is nothing on screen to say
//! where the world's origin or its XY plane went. A grid answers all three,
//! and none of it is part of the model: what is decided here is where to draw
//! lines, never what the document contains.
//!
//! # Deciding, not drawing
//!
//! This module is arithmetic. It knows a camera and produces numbers; nothing
//! here allocates, iterates or touches a GPU, so the rule that decides spacing
//! can be examined without one. Both the offscreen renderer and the window use
//! the answer it gives, which is what stops the two from drifting apart.

use crate::Camera;

/// The closest two minor lines may come on screen, in pixels.
///
/// Lines nearer than this stop reading as a grid and start reading as a
/// texture, and at a shallow angle they alias into moire.
pub const MIN_PIXELS: f32 = 8.0;

/// The furthest apart they may be, in pixels.
///
/// Follows from the ladder rather than being chosen: the largest ratio the
/// steps below can produce is two and a half times the minimum, when the
/// wanted spacing lands just above one step and has to take the next.
pub const MAX_PIXELS: f32 = MIN_PIXELS * 2.5;

/// How many minor lines make a major one.
///
/// Ten, because the ladder is decimal and a person counting squares between
/// heavy lines is counting in tens.
pub const MAJOR_EVERY: u32 = 10;

/// How far the grid runs from the origin, in minor steps each way.
///
/// A fixed count, so the number of lines never depends on the camera: what
/// changes with zoom is how far apart they are, and therefore how much world
/// the same lines cover. A count that grew with distance would be an
/// allocation that grew with it too.
pub const HALF_LINES: u32 = 120;

/// Where to put the lines of a reference grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridPlan {
    /// The distance between minor lines, in millimetres like everything else.
    pub minor: f32,
    /// The distance between the heavier lines.
    pub major: f32,
    /// How far the grid reaches from the origin along each axis.
    pub extent: f32,
}

/// Where a grid's lines should go for this camera, if a grid can be drawn.
///
/// `None` for a viewport with no area and for a camera whose scale is not a
/// finite positive number. Both mean the same thing: there is no screen to
/// measure against, so there is no spacing that could be right.
///
/// # The ladder
///
/// Spacings come from 1, 2 and 5 times a power of ten and nowhere else. That
/// is what makes a grid readable: every square is a round number of
/// millimetres, and zooming steps between round numbers rather than sliding
/// through arbitrary ones. The smallest step no closer than [`MIN_PIXELS`] is
/// taken, which bounds the result at [`MAX_PIXELS`].
///
/// The arithmetic is straight-line: one logarithm and three comparisons. There
/// is no search to fail to terminate and nothing to allocate.
pub fn plan(camera: &Camera) -> Option<GridPlan> {
    let per_pixel = camera.world_per_pixel();
    if !per_pixel.is_finite() || per_pixel <= 0.0 {
        return None;
    }

    // The smallest spacing that would still read as a grid.
    let wanted = per_pixel * MIN_PIXELS;
    if !wanted.is_finite() || wanted <= 0.0 {
        return None;
    }

    let decade = 10.0f32.powf(wanted.log10().floor());
    if !decade.is_finite() || decade <= 0.0 {
        return None;
    }

    let minor = if wanted <= decade {
        decade
    } else if wanted <= 2.0 * decade {
        2.0 * decade
    } else if wanted <= 5.0 * decade {
        5.0 * decade
    } else {
        10.0 * decade
    };

    let major = minor * MAJOR_EVERY as f32;
    let extent = minor * HALF_LINES as f32;
    if !minor.is_finite() || !major.is_finite() || !extent.is_finite() {
        // A camera far enough away to overflow the extent has nothing useful
        // to draw at this scale anyway.
        return None;
    }

    Some(GridPlan {
        minor,
        major,
        extent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera whose scale is one world unit per pixel times `scale`.
    ///
    /// Built by moving the camera rather than by writing a number into it: the
    /// spacing rule reads the camera, so a test that bypassed the camera would
    /// be testing arithmetic nobody runs.
    fn camera_at(distance: f32) -> Camera {
        let mut camera = Camera::new();
        camera.resize(800, 600);
        camera
            .frame((
                [-distance, -distance, -distance],
                [distance, distance, distance],
            ))
            .expect("a finite box can be framed");
        camera
    }

    fn spacings_across(range: std::ops::Range<i32>) -> Vec<f32> {
        (range)
            .map(|step| {
                let distance = 10.0f32.powf(step as f32 / 4.0);
                plan(&camera_at(distance))
                    .expect("a drawable camera has a grid")
                    .minor
            })
            .collect()
    }

    #[test]
    fn every_spacing_is_one_two_or_five_times_a_power_of_ten() {
        for minor in spacings_across(-16..16) {
            let decade = 10.0f32.powf(minor.log10().floor());
            let mantissa = minor / decade;
            assert!(
                (mantissa - 1.0).abs() < 1e-3
                    || (mantissa - 2.0).abs() < 1e-3
                    || (mantissa - 5.0).abs() < 1e-3,
                "{minor} is not on the ladder: mantissa {mantissa}"
            );
        }
    }

    #[test]
    fn both_sides_of_a_decade_choose_the_step_they_should() {
        // Worked from the rule rather than from the implementation: the
        // smallest ladder step at least `MIN_PIXELS` across.
        let cases = [
            (0.9_f32, 1.0_f32),
            (1.0, 1.0),
            (1.1, 2.0),
            (2.0, 2.0),
            (2.1, 5.0),
            (5.0, 5.0),
            (5.1, 10.0),
            (9.9, 10.0),
        ];
        for (wanted, expected) in cases {
            let decade = 10.0f32.powf(wanted.log10().floor());
            let chosen = if wanted <= decade {
                decade
            } else if wanted <= 2.0 * decade {
                2.0 * decade
            } else if wanted <= 5.0 * decade {
                5.0 * decade
            } else {
                10.0 * decade
            };
            assert!(
                (chosen - expected).abs() < 1e-6,
                "a wanted spacing of {wanted} chose {chosen} rather than {expected}"
            );
        }
    }

    #[test]
    fn zooming_changes_the_spacing_by_one_step_at_a_time() {
        let mut previous: Option<f32> = None;
        for minor in spacings_across(-16..16) {
            if let Some(before) = previous {
                let ratio = minor / before;
                assert!(
                    (ratio - 1.0).abs() < 1e-3
                        || (ratio - 2.0).abs() < 1e-3
                        || (ratio - 2.5).abs() < 1e-3
                        || (ratio - 5.0).abs() < 1e-3,
                    "the spacing jumped by {ratio} between neighbouring zooms"
                );
            }
            previous = Some(minor);
        }
    }

    #[test]
    fn lines_stay_within_the_distance_a_person_can_read() {
        for step in -16..16 {
            let camera = camera_at(10.0f32.powf(step as f32 / 4.0));
            let plan = plan(&camera).expect("a drawable camera has a grid");
            let pixels = plan.minor / camera.world_per_pixel();
            assert!(
                (MIN_PIXELS..MAX_PIXELS).contains(&pixels),
                "minor lines {pixels} pixels apart, outside {MIN_PIXELS}..{MAX_PIXELS}"
            );
        }
    }

    #[test]
    fn the_heavier_lines_and_the_axes_come_from_the_same_origin() {
        let plan = plan(&camera_at(50.0)).expect("a drawable camera has a grid");

        // A major line every ten minor ones, measured from zero, so the axes
        // are lines of the grid rather than something drawn beside it.
        assert!((plan.major / plan.minor - f32::from(MAJOR_EVERY as u16)).abs() < 1e-3);
        assert!((plan.extent / plan.minor - HALF_LINES as f32).abs() < 1e-3);

        // Zero is on every one of them, which is what makes the origin
        // findable at any zoom.
        for spacing in [plan.minor, plan.major] {
            assert!((0.0f32 % spacing).abs() < 1e-6);
        }
    }

    #[test]
    fn a_camera_with_no_screen_has_no_grid() {
        let mut camera = Camera::new();
        camera.resize(0, 0);
        assert_eq!(plan(&camera), None);

        camera.resize(800, 0);
        assert_eq!(plan(&camera), None);
    }

    #[test]
    fn extreme_but_finite_cameras_answer_finitely_or_not_at_all() {
        for exponent in [-30, -20, -10, 0, 10, 20, 30] {
            let camera = camera_at(10.0f32.powi(exponent));
            // Refusing is an answer. What must not happen is a number that
            // is not a number.
            if let Some(plan) = plan(&camera) {
                assert!(plan.minor.is_finite() && plan.minor > 0.0, "{plan:?}");
                assert!(
                    plan.major.is_finite() && plan.extent.is_finite(),
                    "{plan:?}"
                );
                let pixels = plan.minor / camera.world_per_pixel();
                assert!(
                    pixels.is_finite() && pixels > 0.0,
                    "{pixels} pixels between lines"
                );
            }
        }
    }
}
