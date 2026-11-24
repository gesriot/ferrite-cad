// SPDX-License-Identifier: MIT
//! Gesture headings shared by viewport drawing and dragging.
//! Choices use logical screen points; resolving them changes no model or constraint.

/// How far from a guide the pointer may be, measured across it, and still hold it,
/// in logical points. The distance to the guide, never to its origin: the pointer
/// may be as far along it as the canvas goes.
pub(super) const APERTURE_POINTS: f64 = 8.0;
/// The held guide is kept until another is nearer by **more than** this; a first
/// choice treats the guides within **strictly less** than this of the nearest as one
/// group and ranks it axis before diagonal, then by distance, then by table order.
pub(super) const COMPETITION_POINTS: f64 = 1.0;
/// A guide has a direction only once the pointer is at least this far along it from
/// the anchor: twice the aperture, where the neighbouring 45° guides are already
/// separated. Nearer than that (in particular on the anchor itself, where every
/// direction is the same distance away) no guide is held and nothing is forced.
pub(super) const MIN_REACH_POINTS: f64 = 2.0 * APERTURE_POINTS;
/// The guide choice. Editor state beside the grid step, off for a new editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Mode {
    #[default]
    Off,
    Orthogonal,
    Polar,
}

impl Mode {
    pub(crate) const CHOICES: [(Self, &'static str); 3] = [
        (Self::Off, "Off"),
        (Self::Orthogonal, "Orthogonal"),
        (Self::Polar, "Polar 45°"),
    ];
    pub(crate) fn label(self) -> &'static str {
        Self::CHOICES
            .iter()
            .find(|(value, _)| *value == self)
            .map(|(_, label)| *label)
            .expect("every Mode value has a label")
    }
    fn headings(self) -> &'static [Heading] {
        match self {
            Self::Off => &[],
            Self::Orthogonal => &HEADINGS[..4],
            Self::Polar => &HEADINGS,
        }
    }
}

/// One guide direction: a unit vector and its angle counter-clockwise from +X.
#[derive(Debug, Clone, Copy)]
struct Heading {
    unit: [f64; 2],
    degrees: u16,
}

const DIAGONAL: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// The table order is the stable order of ties; the first four are the axes and are
/// exact, so a guide along an axis never moves the other coordinate by a rounding.
const HEADINGS: [Heading; 8] = [
    Heading {
        unit: [1.0, 0.0],
        degrees: 0,
    },
    Heading {
        unit: [0.0, 1.0],
        degrees: 90,
    },
    Heading {
        unit: [-1.0, 0.0],
        degrees: 180,
    },
    Heading {
        unit: [0.0, -1.0],
        degrees: 270,
    },
    Heading {
        unit: [DIAGONAL, DIAGONAL],
        degrees: 45,
    },
    Heading {
        unit: [-DIAGONAL, DIAGONAL],
        degrees: 135,
    },
    Heading {
        unit: [-DIAGONAL, -DIAGONAL],
        degrees: 225,
    },
    Heading {
        unit: [DIAGONAL, -DIAGONAL],
        degrees: 315,
    },
];

impl Heading {
    fn diagonal(self) -> bool {
        !self.degrees.is_multiple_of(90)
    }
}

/// The unit vector (document axes) of a held guide, to draw it.
pub(crate) fn unit(heading: usize) -> [f64; 2] {
    HEADINGS[heading].unit
}

/// The guide the pointer holds, as an index into the table, or none.
///
/// A guide is offered when the pointer is at least [`MIN_REACH_POINTS`] along it
/// and no more than [`APERTURE_POINTS`] across it. The held guide stays while it is
/// offered and no other is nearer by more than [`COMPETITION_POINTS`]. Without a
/// held guide, the offered guides within strictly less than that of the nearest are
/// ranked axis before diagonal, then by distance, then by table order. Non-finite
/// numbers and the anchor itself offer nothing.
pub(crate) fn acquire(
    mode: Mode,
    offset: [f64; 2],
    points_per_mm: f64,
    held: Option<usize>,
) -> Option<usize> {
    // A scale that is not a positive number gives no distance in points at all.
    if !points_per_mm.is_finite() || points_per_mm <= 0.0 {
        return None;
    }
    let offered: Vec<(usize, f64)> = mode
        .headings()
        .iter()
        .enumerate()
        .filter_map(|(i, heading)| {
            let unit = heading.unit;
            let along = (offset[0] * unit[0] + offset[1] * unit[1]) * points_per_mm;
            let across = (offset[0] * unit[1] - offset[1] * unit[0]).abs() * points_per_mm;
            (along.is_finite()
                && across.is_finite()
                && along >= MIN_REACH_POINTS
                && across <= APERTURE_POINTS)
                .then_some((i, across))
        })
        .collect();
    choose(&offered, held)
}

/// The choice among the offered guides, each as (table index, points across it).
fn choose(offered: &[(usize, f64)], held: Option<usize>) -> Option<usize> {
    let nearest = offered.iter().map(|&(_, d)| d).min_by(f64::total_cmp)?;
    if let Some(&(i, across)) = offered.iter().find(|&&(i, _)| Some(i) == held)
        && across - nearest <= COMPETITION_POINTS
    {
        return Some(i);
    }
    offered
        .iter()
        .filter(|&&(_, across)| across - nearest < COMPETITION_POINTS)
        .min_by(|&&(a, da), &&(b, db)| {
            HEADINGS[a]
                .diagonal()
                .cmp(&HEADINGS[b].diagonal())
                .then(da.total_cmp(&db))
                .then(a.cmp(&b))
        })
        .map(|&(i, _)| i)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    /// A pointer `across` points off the guide `degrees` and `along` points along it
    /// at `points_per_mm`, as the offset (mm) from the anchor in the XY plane.
    fn at(degrees: f64, along: f64, across: f64, points_per_mm: f64) -> [f64; 2] {
        let (sin, cos) = degrees.to_radians().sin_cos();
        [
            (along * cos - across * sin) / points_per_mm,
            (along * sin + across * cos) / points_per_mm,
        ]
    }
    #[test]
    fn axes_and_diagonals_are_in_the_xy_plane_with_y_up() {
        let named: Vec<_> = Mode::Polar
            .headings()
            .iter()
            .map(|h| (h.degrees, h.unit.map(|v| (v * 1e12).round() / 1e12)))
            .collect();
        let d = (DIAGONAL * 1e12).round() / 1e12;
        assert_eq!(
            named,
            [
                (0, [1.0, 0.0]),
                (90, [0.0, 1.0]),
                (180, [-1.0, 0.0]),
                (270, [0.0, -1.0]),
                (45, [d, d]),
                (135, [-d, d]),
                (225, [-d, -d]),
                (315, [d, -d]),
            ]
        );
        assert_eq!(Mode::Orthogonal.headings().len(), 4, "axes only");
        assert!(Mode::Off.headings().is_empty());
        // The table's axes are exact, not cos/sin of an angle.
        assert!(
            HEADINGS[..4]
                .iter()
                .all(|h| h.unit.iter().all(|v| v.abs() == 0.0 || v.abs() == 1.0))
        );
        for (degrees, expected) in [(0.0, 0), (90.0, 1), (180.0, 2), (270.0, 3)] {
            let offset = at(degrees, 40.0, 0.0, 4.0);
            assert_eq!(acquire(Mode::Orthogonal, offset, 4.0, None), Some(expected));
        }
        for (degrees, expected) in [(45.0, 4), (135.0, 5), (225.0, 6), (315.0, 7)] {
            let offset = at(degrees, 40.0, 0.0, 4.0);
            assert_eq!(acquire(Mode::Polar, offset, 4.0, None), Some(expected));
            assert_eq!(
                acquire(Mode::Orthogonal, offset, 4.0, None),
                None,
                "the diagonal belongs to Polar 45° only"
            );
        }
    }

    #[test]
    fn a_guide_is_held_within_the_aperture_across_it_and_not_beyond() {
        for points_per_mm in [0.5, 1.0, 4.0, 16.0] {
            for sign in [1.0, -1.0] {
                let inside = at(0.0, 60.0, sign * APERTURE_POINTS, points_per_mm);
                let outside = at(0.0, 60.0, sign * (APERTURE_POINTS + 0.001), points_per_mm);
                assert_eq!(
                    acquire(Mode::Orthogonal, inside, points_per_mm, None),
                    Some(0),
                    "exactly at the aperture, scale {points_per_mm}"
                );
                assert_eq!(
                    acquire(Mode::Orthogonal, outside, points_per_mm, None),
                    None,
                    "just beyond it, scale {points_per_mm}"
                );
            }
        }
    }

    #[test]
    fn distance_is_to_the_guide_not_to_its_origin_so_far_pointers_still_hold_it() {
        // 100 000 points along, 3 across: far beyond twice the aperture, still held.
        let offset = at(0.0, 100_000.0, 3.0, 4.0);
        assert_eq!(acquire(Mode::Polar, offset, 4.0, None), Some(0));
        // The same far pointer, 9 points across, is not forced onto any angle.
        let off = at(0.0, 100_000.0, 9.0, 4.0);
        assert_eq!(acquire(Mode::Polar, off, 4.0, None), None);
    }

    #[test]
    fn the_anchor_itself_a_zero_vector_and_non_finite_numbers_offer_no_guide() {
        for offset in [
            [0.0, 0.0],
            [1e-9, -1e-9],
            at(0.0, MIN_REACH_POINTS - 0.001, 0.0, 4.0),
            [f64::NAN, 0.0],
            [0.0, f64::INFINITY],
            [f64::NEG_INFINITY, f64::NAN],
        ] {
            assert_eq!(acquire(Mode::Polar, offset, 4.0, None), None, "{offset:?}");
        }
        assert_eq!(
            acquire(Mode::Polar, at(0.0, MIN_REACH_POINTS, 0.0, 4.0), 4.0, None),
            Some(0),
            "from the minimum reach on"
        );
        for scale in [0.0, f64::NAN, f64::INFINITY, -4.0] {
            assert_eq!(acquire(Mode::Polar, [50.0, 0.0], scale, None), None);
        }
        assert_eq!(
            acquire(Mode::Polar, [0.0, 0.0], 4.0, Some(2)),
            None,
            "no direction even with a previously held guide"
        );
    }

    /// The pointer `radius` points from the anchor at `degrees`, as an offset in mm.
    fn ring(degrees: f64, radius: f64, points_per_mm: f64) -> [f64; 2] {
        at(degrees, radius, 0.0, points_per_mm)
    }
    /// East and north-east are both offered only on a ring of 17.3–20.9 points
    /// around the anchor; between 20.1° and 24.9° at 19 points, both are.
    const RING_POINTS: f64 = 19.0;
    fn across(degrees_from_guide: f64) -> f64 {
        RING_POINTS * degrees_from_guide.to_radians().sin()
    }
    /// The angle in (20.2, 24.8) where east is farther than north-east by `gap`.
    fn angle_with_east_farther_by(gap: f64) -> f64 {
        let (mut lo, mut hi) = (20.2_f64, 24.8_f64);
        for _ in 0..80 {
            let mid = (lo + hi) / 2.0;
            if across(mid) - across(45.0 - mid) < gap {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    }

    #[test]
    fn a_held_guide_is_kept_until_another_is_nearer_by_more_than_one_point() {
        // The rule itself, at exact numbers: switch only for a gap above one point.
        let both = |east: f64, northeast: f64| [(0, east), (4, northeast)];
        assert_eq!(choose(&both(5.0, 4.5), Some(0)), Some(0));
        assert_eq!(
            choose(&both(5.0, 4.0), Some(0)),
            Some(0),
            "exactly one point: kept"
        );
        assert_eq!(choose(&both(5.0, 3.9), Some(0)), Some(4));
        assert_eq!(choose(&both(4.5, 5.0), Some(4)), Some(4));
        assert_eq!(
            choose(&both(4.0, 5.0), Some(4)),
            Some(4),
            "exactly one point: kept"
        );
        assert_eq!(choose(&both(3.9, 5.0), Some(4)), Some(0));
        // A held guide that is no longer offered is dropped for the nearest.
        assert_eq!(choose(&both(5.0, 4.0), Some(1)), Some(4));
        assert_eq!(
            choose(&both(5.0, 4.0), None),
            Some(4),
            "at exactly one point the nearer"
        );
        assert_eq!(choose(&[], Some(0)), None);

        // The same rule on real pointers: east held while north-east gets nearer.
        let points_per_mm = 4.0;
        for (gap, held, expected) in [(0.9, 0, 0), (1.1, 0, 4), (-0.9, 4, 4), (-1.1, 4, 0)] {
            let a = angle_with_east_farther_by(gap);
            let offset = ring(a, RING_POINTS, points_per_mm);
            assert!(across(a) <= APERTURE_POINTS && across(45.0 - a) <= APERTURE_POINTS);
            assert_eq!(
                acquire(Mode::Polar, offset, points_per_mm, Some(held)),
                Some(expected),
                "held {held}, east farther by {gap}"
            );
        }
        // Dropping out of range ends the hold: the guide is chosen afresh.
        assert_eq!(
            acquire(Mode::Polar, ring(0.0, 60.0, 4.0), 4.0, Some(1)),
            Some(0)
        );
        // A held index the mode does not have (Polar → Orthogonal) is ignored.
        assert_eq!(
            acquire(Mode::Orthogonal, ring(0.0, 60.0, 4.0), 4.0, Some(6)),
            Some(0)
        );
    }

    #[test]
    fn a_first_choice_among_near_equal_guides_is_axis_then_distance_then_table_order() {
        let points_per_mm = 4.0;
        // The rule at exact numbers: within strictly less than a point the axis
        // wins over a nearer diagonal; at a point or more, the nearer.
        let both = |east: f64, northeast: f64| [(0, east), (4, northeast)];
        assert_eq!(choose(&both(4.9, 4.0), None), Some(0));
        assert_eq!(choose(&both(5.0, 4.0), None), Some(4));
        assert_eq!(
            choose(&both(4.0, 4.9), None),
            Some(0),
            "the axis, though farther"
        );
        assert_eq!(choose(&[(5, 2.0), (4, 2.0)], None), Some(4), "table order");
        assert_eq!(
            choose(&[(2, 3.0), (0, 3.0)], None),
            Some(0),
            "two axes: nearer, then table order"
        );
        // On the bisector the two are equal up to rounding; the axis wins whatever
        // the rounding says, and a diagonal wins once it is a point nearer.
        for degrees in [22.5, 22.500001, 22.499999] {
            let offset = ring(degrees, RING_POINTS, points_per_mm);
            assert_eq!(acquire(Mode::Polar, offset, points_per_mm, None), Some(0));
        }
        let offset = ring(angle_with_east_farther_by(1.2), RING_POINTS, points_per_mm);
        assert_eq!(acquire(Mode::Polar, offset, points_per_mm, None), Some(4));
        // The answer does not depend on how often it is asked.
        let first = acquire(Mode::Polar, offset, points_per_mm, None);
        assert!((0..5).all(|_| acquire(Mode::Polar, offset, points_per_mm, None) == first));
    }

    #[test]
    fn the_same_pointer_in_points_picks_the_same_guide_at_any_scale() {
        for points_per_mm in [0.25, 1.0, 2.0, 8.0, 100.0] {
            for (degrees, along, across, expected) in [
                (0.0, 50.0, 7.9, Some(0)),
                (90.0, 50.0, -7.9, Some(1)),
                (45.0, 50.0, 7.9, Some(4)),
                (0.0, 50.0, 8.5, None),
            ] {
                let offset = at(degrees, along, across, points_per_mm);
                assert_eq!(
                    acquire(Mode::Polar, offset, points_per_mm, None),
                    expected,
                    "{degrees}° at {points_per_mm} points per mm"
                );
            }
        }
    }
}
