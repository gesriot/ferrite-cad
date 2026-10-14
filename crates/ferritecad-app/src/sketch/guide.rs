// SPDX-License-Identifier: MIT
//! §31D-1: gesture guides for the Sketch canvas. From one explicit, drawn anchor
//! (the last vertex when a point is added, the press-time position when a vertex is
//! dragged) the pointer can hold an orthogonal or 45° direction, optionally at a
//! typed length.
//!
//! Geometry is document millimetres in the XY plane (+X right, +Y up); the choice
//! of a direction is made in logical egui points, the canvas's points-per-mm turning
//! one into the other exactly once per query. Nothing here knows egui, the draft or
//! the document: a guide is editor state that only proposes coordinates. It creates
//! no constraint, no ID and no history, and the typed length is not a dimension.
use super::object_snap::{self, COINCIDENT_MM, Candidate, SnapKind};

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
/// An object point counts as being at the typed length when it is this close to the
/// point the length makes: the document's own tolerance (below which the polygon
/// policy calls two vertices one), a model length unrelated to any screen distance.
pub(super) const OBJECT_MATCH_MM: f64 = COINCIDENT_MM;
/// Coordinates the guide computes are rounded to this many parts per millimetre
/// (one picometre, a thousand times finer than the document tolerance) so that a
/// length added to an anchor reads `0.3`, not `0.30000000000000004`.
const PARTS_PER_MM: f64 = 1e9;

/// The guide choice. Editor state beside the grid step, off for a new editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Mode {
    #[default]
    Off,
    Orthogonal,
    Polar,
}

impl Mode {
    pub(super) const CHOICES: [(Self, &'static str); 3] = [
        (Self::Off, "Off"),
        (Self::Orthogonal, "Orthogonal"),
        (Self::Polar, "Polar 45°"),
    ];
    pub(super) fn label(self) -> &'static str {
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
pub(super) fn unit(heading: usize) -> [f64; 2] {
    HEADINGS[heading].unit
}

/// The optional Length mm field. Only a positive finite number is a length; any
/// other non-blank text is refused, and is never read as zero or as a free choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Length {
    None,
    Millimetres(f64),
    Refused,
}

impl Length {
    pub(super) fn parse(text: &str) -> Self {
        let text = text.trim();
        if text.is_empty() {
            return Self::None;
        }
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() && value > 0.0 => Self::Millimetres(value),
            _ => Self::Refused,
        }
    }
}

/// What a gesture freezes at its press (a click for a new point): the guide choice
/// and the length as typed then.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Settings {
    pub(super) mode: Mode,
    pub(super) length: Length,
}

/// Where the pointer is relative to the anchor, and the guide it held last.
#[derive(Debug, Clone, Copy)]
pub(super) struct Reach {
    pub(super) anchor: [f64; 2],
    /// Pointer minus anchor, in document millimetres.
    pub(super) offset: [f64; 2],
    pub(super) points_per_mm: f64,
    /// Index of the guide held before this query (`acquire`).
    pub(super) held: Option<usize>,
}

/// The guide the pointer holds, as an index into the table, or none.
///
/// A guide is offered when the pointer is at least [`MIN_REACH_POINTS`] along it
/// and no more than [`APERTURE_POINTS`] across it. The held guide stays while it is
/// offered and no other is nearer by more than [`COMPETITION_POINTS`]. Without a
/// held guide, the offered guides within strictly less than that of the nearest are
/// ranked axis before diagonal, then by distance, then by table order. Non-finite
/// numbers and the anchor itself offer nothing.
pub(super) fn acquire(
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

/// What a query decided. The same value draws the frame and writes the point, so
/// the point shown is the point written.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Outcome {
    /// No guide took part: object, grid and free rules apply as before.
    Open,
    /// An object point won: without a length because objects rank above guides,
    /// with one only because it is at the typed length.
    Object(Candidate),
    /// The pointer holds a shown guide and this is the point it gives.
    Guide(Guided),
    /// The pointer holds a shown guide but the typed Length cannot be used: no
    /// point is placed and a dragged vertex keeps its strings.
    Refused { heading: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Guided {
    pub(super) heading: usize,
    pub(super) at: [f64; 2],
    /// Distance from the anchor, in millimetres: the typed length or the pointer's
    /// projection onto the guide.
    pub(super) millimetres: f64,
    pub(super) typed: bool,
    /// An object point near the pointer that is not at the typed length and so was
    /// not taken: the length kept, the conflict shown.
    pub(super) passed_over: Option<SnapKind>,
}

/// Whether a computed coordinate can be drawn, as every vertex must be to be shown
/// by the canvas: finite and within its range.
fn drawable(at: [f64; 2]) -> bool {
    at.iter()
        .all(|v| v.is_finite() && v.abs() <= super::DRAWABLE_MM)
}

/// Decides one query.
///
/// Precedence, fixed here: an object point (§31C) beats a guide; a guide beats the
/// grid and the free point, and the grid step is **not** applied to a guided point
/// (rounding either coordinate could take a diagonal off its direction, or an axis
/// guide behind its anchor). A typed length is the exception that makes the point
/// exact before coordinate formatting: it is never rounded to or replaced by a grid or object point. An object
/// point is shown and taken only if it is at that point within [`OBJECT_MATCH_MM`];
/// otherwise the length is kept, the point is not taken, and the label says so. A
/// refused length with a guide held places nothing, even when an object point is nearby. Returns the outcome and the guide to hold next.
pub(super) fn resolve(
    settings: Settings,
    reach: &Reach,
    object: Option<Candidate>,
) -> (Outcome, Option<usize>) {
    let Some(heading) = acquire(settings.mode, reach.offset, reach.points_per_mm, reach.held)
    else {
        return (object.map_or(Outcome::Open, Outcome::Object), None);
    };
    let unit = HEADINGS[heading].unit;
    let at_distance = |distance: f64| {
        [
            reach.anchor[0] + unit[0] * distance,
            reach.anchor[1] + unit[1] * distance,
        ]
    };
    let outcome = match (settings.length, object) {
        (Length::None, Some(hit)) => Outcome::Object(hit),
        (Length::None, None) => {
            let along = reach.offset[0] * unit[0] + reach.offset[1] * unit[1];
            Outcome::Guide(Guided {
                heading,
                at: at_distance(along),
                millimetres: along,
                typed: false,
                passed_over: None,
            })
        }
        (Length::Refused, _) => Outcome::Refused { heading },
        (Length::Millimetres(length), object) => {
            let at = at_distance(length);
            match object {
                _ if !drawable(at) => Outcome::Refused { heading },
                Some(hit) if (hit.at[0] - at[0]).hypot(hit.at[1] - at[1]) <= OBJECT_MATCH_MM => {
                    Outcome::Object(hit)
                }
                other => Outcome::Guide(Guided {
                    heading,
                    at,
                    millimetres: length,
                    typed: true,
                    passed_over: other.map(|c| c.kind),
                }),
            }
        }
    };
    (outcome, Some(heading))
}

/// The coordinate strings of a guided point. An axis guide keeps the anchor's own
/// text on the axis it does not move along (so `20.0` stays `20.0`); every other
/// coordinate is rounded to [`PARTS_PER_MM`] and written as the shortest decimal
/// that reads back as the same number.
pub(super) fn strings(heading: usize, at: [f64; 2], anchor: &[String; 2]) -> [String; 2] {
    let unit = HEADINGS[heading].unit;
    std::array::from_fn(|axis| {
        if unit[axis] == 0.0 {
            anchor[axis].clone()
        } else {
            let rounded = (at[axis] * PARTS_PER_MM).round() / PARTS_PER_MM;
            object_snap::coordinate(if rounded.is_finite() {
                rounded
            } else {
                at[axis]
            })
        }
    })
}

/// A distance as shown: at most three decimals, no trailing zeros.
fn shown(value: f64) -> String {
    let text = format!("{value:.3}");
    if value != 0.0 && text.parse::<f64>().ok() == Some(0.0) {
        return value.to_string();
    }
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// What the guide's anchor is, for the label: a new point after a vertex, or the
/// vertex being dragged. Both numbers are the 1-based numbers the canvas draws.
#[derive(Debug, Clone, Copy)]
pub(super) enum Role {
    Add { after: usize },
    Drag { vertex: usize },
}

/// The words beside the pointer for a guided outcome, in the frame only. A dragged
/// vertex's length is its movement from where the press found it, and says so; it is
/// not the length of a neighbouring edge.
pub(super) fn label(outcome: &Outcome, role: Role) -> Option<String> {
    let degrees = |heading: usize| HEADINGS[heading].degrees;
    let distance = |g: &Guided| {
        if g.typed {
            g.millimetres.to_string()
        } else {
            shown(g.millimetres)
        }
    };
    match (outcome, role) {
        (Outcome::Guide(g), Role::Add { after }) => {
            let length = if g.typed { "Length " } else { "" };
            Some(format!(
                "{}° · {length}{} mm from vertex {}{}",
                degrees(g.heading),
                distance(g),
                after + 1,
                conflict(g),
            ))
        }
        (Outcome::Guide(g), Role::Drag { vertex }) => Some(if g.typed {
            format!(
                "{}° · Length {} mm · vertex {} moved from its start{}",
                degrees(g.heading),
                distance(g),
                vertex + 1,
                conflict(g),
            )
        } else {
            format!(
                "{}° · vertex {} moved {} mm from its start",
                degrees(g.heading),
                vertex + 1,
                distance(g),
            )
        }),
        (Outcome::Refused { heading }, Role::Add { .. }) => Some(format!(
            "{}° · Length refused · no point added",
            degrees(*heading)
        )),
        (Outcome::Refused { heading }, Role::Drag { vertex }) => Some(format!(
            "{}° · Length refused · vertex {} not moved",
            degrees(*heading),
            vertex + 1
        )),
        (Outcome::Open | Outcome::Object(_), _) => None,
    }
}

fn conflict(g: &Guided) -> String {
    g.passed_over.map_or_else(String::new, |kind| {
        format!(" · {} is not at this length", kind.label())
    })
}

/// The standing explanation under the Guides row and whether it is a refusal. It
/// says what the Length field does once a gesture is over: nothing is consumed or
/// stored, so the text stays and applies again until it is cleared.
pub(super) fn note(mode: Mode, length: Length, text: &str) -> (String, bool) {
    match (mode, length) {
        (Mode::Off, _) => (
            "Guides are off: points follow the pointer, the grid and object snap as before. \
             Length is not used."
                .into(),
            false,
        ),
        (_, Length::Refused) => (
            format!(
                "Length {:?} is refused: use a positive number of mm or clear it. While it is \
                 refused, a point or drag on a shown guide places nothing.",
                text.trim()
            ),
            true,
        ),
        (_, Length::Millimetres(length)) => (
            format!(
                "Length {} mm is used for every point or drag on a shown guide until you clear \
                 it. It only places coordinates: no constraint, nothing saved, no Undo step. \
                 The grid step is not applied to guided points.",
                length
            ),
            false,
        ),
        (_, Length::None) => (
            "Move near a shown axis or diagonal from the ringed anchor to align; the distance \
             is the pointer's, or type Length mm first. The grid step is not applied to guided \
             points."
                .into(),
            false,
        ),
    }
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
    fn anchor_text() -> [String; 2] {
        ["20.0".into(), "30".into()]
    }
    fn candidate(at: [f64; 2]) -> Candidate {
        let points = [[at[0] - 1.0, at[1]], [at[0] + 1.0, at[1]]];
        object_snap::collect(&points, false, None)
            .into_iter()
            .find(|c| c.kind == SnapKind::Midpoint)
            .expect("the middle of a two-point line")
    }
    fn reach(offset: [f64; 2], points_per_mm: f64, held: Option<usize>) -> Reach {
        Reach {
            anchor: [20.0, 30.0],
            offset,
            points_per_mm,
            held,
        }
    }
    fn settings(mode: Mode, length: Length) -> Settings {
        Settings { mode, length }
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
        println!("\nFCAD_31D_HEADINGS_EXECUTED");
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
        println!("\nFCAD_31D_APERTURE_EXECUTED");
    }

    #[test]
    fn distance_is_to_the_guide_not_to_its_origin_so_far_pointers_still_hold_it() {
        // 100 000 points along, 3 across: far beyond twice the aperture, still held.
        let offset = at(0.0, 100_000.0, 3.0, 4.0);
        assert_eq!(acquire(Mode::Polar, offset, 4.0, None), Some(0));
        // The same far pointer, 9 points across, is not forced onto any angle.
        let off = at(0.0, 100_000.0, 9.0, 4.0);
        assert_eq!(acquire(Mode::Polar, off, 4.0, None), None);
        println!("\nFCAD_31D_FAR_ANCHOR_EXECUTED");
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
        let (outcome, held) = resolve(
            settings(Mode::Polar, Length::Millimetres(5.0)),
            &reach([0.0, 0.0], 4.0, Some(2)),
            None,
        );
        assert_eq!(
            (outcome, held),
            (Outcome::Open, None),
            "no direction, no length"
        );
        println!("\nFCAD_31D_ZERO_EXECUTED");
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
        println!("\nFCAD_31D_HYSTERESIS_EXECUTED");
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
        println!("\nFCAD_31D_TIES_EXECUTED");
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
        println!("\nFCAD_31D_SCALES_EXECUTED");
    }

    #[test]
    fn object_over_guide_over_free_point_and_the_grid_is_not_applied_to_a_guide() {
        let free = Settings {
            mode: Mode::Polar,
            length: Length::None,
        };
        let offset = [10.0, 0.25]; // 40 points along east, 1 across at 4 points/mm
        let (outcome, held) = resolve(free, &reach(offset, 4.0, None), None);
        let Outcome::Guide(g) = &outcome else {
            panic!("the guide, not {outcome:?}");
        };
        assert_eq!((g.heading, held), (0, Some(0)));
        assert_eq!(g.at, [30.0, 30.0], "projected on east: y is the anchor's");
        assert!(!g.typed && g.passed_over.is_none());
        assert_eq!(strings(g.heading, g.at, &anchor_text()), ["30", "30"]);
        // An object point near the pointer wins and is not moved onto the guide.
        let hit = candidate([29.9, 30.2]);
        let (outcome, held) = resolve(free, &reach(offset, 4.0, None), Some(hit));
        assert_eq!(outcome, Outcome::Object(hit));
        assert_eq!(
            held,
            Some(0),
            "the guide is still held under an object point"
        );
        // Without a guide in reach, the object is the answer, as in 31C.
        let (outcome, held) = resolve(free, &reach([10.0, 5.0], 4.0, None), Some(hit));
        assert_eq!((outcome, held), (Outcome::Object(hit), None));
        println!("\nFCAD_31D_GEOMETRY_PRECEDENCE_EXECUTED");
    }

    #[test]
    fn a_typed_length_is_exact_and_beats_the_pointer_the_grid_and_a_foreign_object() {
        let typed = |text| settings(Mode::Polar, Length::parse(text));
        let offset = [10.0, 0.0];
        // The pointer is 40 points along; the length is 2.5 mm: the point is exact.
        let (outcome, _) = resolve(typed("2.5"), &reach(offset, 4.0, None), None);
        let Outcome::Guide(g) = &outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!((g.at, g.millimetres, g.typed), ([22.5, 30.0], 2.5, true));
        assert_eq!(strings(0, g.at, &anchor_text()), ["22.5", "30"]);
        // A diagonal, rounded to a picometre, never an f64 artefact.
        let (outcome, _) = resolve(
            typed("10"),
            &reach(at(45.0, 40.0, 0.0, 4.0), 4.0, None),
            None,
        );
        let Outcome::Guide(g) = &outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(
            strings(g.heading, g.at, &anchor_text()),
            ["27.071067812", "37.071067812"]
        );
        // 0.1 + 0.2 reads 0.3 on an axis whose anchor is 0.1.
        let (outcome, _) = resolve(
            typed("0.2"),
            &Reach {
                anchor: [0.1, 0.0],
                ..reach([10.0, 0.0], 4.0, None)
            },
            None,
        );
        let Outcome::Guide(g) = &outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(strings(0, g.at, &["0.1".into(), "0".into()]), ["0.3", "0"]);
        // An object point near the pointer but not at the length does not win.
        let foreign = candidate([29.9, 30.0]);
        let (outcome, _) = resolve(typed("2.5"), &reach(offset, 4.0, None), Some(foreign));
        let Outcome::Guide(g) = &outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(
            (g.at, g.passed_over),
            ([22.5, 30.0], Some(SnapKind::Midpoint))
        );
        let words = label(&outcome, Role::Add { after: 2 }).expect("label");
        assert!(
            words.contains("Length 2.5 mm") && words.contains("Midpoint is not at this length"),
            "{words}"
        );
        // The object point exactly at the length is taken, as the object.
        let there = candidate([22.5, 30.0]);
        let (outcome, _) = resolve(typed("2.5"), &reach(offset, 4.0, None), Some(there));
        assert_eq!(outcome, Outcome::Object(there));
        // And one a hair beyond the tolerance is not.
        let hair = candidate([22.5 + 2.0 * OBJECT_MATCH_MM, 30.0]);
        let (outcome, _) = resolve(typed("2.5"), &reach(offset, 4.0, None), Some(hair));
        assert!(matches!(outcome, Outcome::Guide(_)), "{outcome:?}");
        println!("\nFCAD_31D_GEOMETRY_LENGTH_EXECUTED");
    }

    #[test]
    fn a_length_that_is_not_a_positive_finite_number_is_refused_never_zero() {
        for (text, expected) in [
            ("", Length::None),
            ("   ", Length::None),
            ("12", Length::Millimetres(12.0)),
            (" 0.5 ", Length::Millimetres(0.5)),
            ("1e-3", Length::Millimetres(0.001)),
            ("0", Length::Refused),
            ("-0", Length::Refused),
            ("-3", Length::Refused),
            ("NaN", Length::Refused),
            ("inf", Length::Refused),
            ("-inf", Length::Refused),
            ("1e999", Length::Refused),
            ("12 mm", Length::Refused),
            ("1,5", Length::Refused),
        ] {
            assert_eq!(Length::parse(text), expected, "{text:?}");
        }
        let refused = settings(Mode::Polar, Length::Refused);
        let (outcome, held) = resolve(refused, &reach([10.0, 0.0], 4.0, None), None);
        assert_eq!(
            (outcome.clone(), held),
            (Outcome::Refused { heading: 0 }, Some(0))
        );
        let words = label(&outcome, Role::Drag { vertex: 2 }).expect("label");
        assert!(
            words.contains("Length refused") && words.contains("vertex 3 not moved"),
            "{words}"
        );
        // The refusal reaches the standing note, with the offending text in it.
        let (text, error) = note(Mode::Polar, Length::Refused, " -3 ");
        assert!(error && text.contains("\"-3\""), "{text}");
        // A nearby object cannot bypass a refused length on an acquired guide.
        let hit = candidate([29.9, 30.0]);
        let (outcome, _) = resolve(refused, &reach([10.0, 0.0], 4.0, None), Some(hit));
        assert_eq!(outcome, Outcome::Refused { heading: 0 });
        // A length that would leave the drawable range is refused, not drawn blank.
        let huge = settings(Mode::Polar, Length::parse("1e9"));
        let (outcome, _) = resolve(huge, &reach([10.0, 0.0], 4.0, None), None);
        assert_eq!(outcome, Outcome::Refused { heading: 0 });
        println!("\nFCAD_31D_GEOMETRY_REFUSED_EXECUTED");
    }

    #[test]
    fn axis_guides_keep_the_anchor_text_and_labels_say_what_the_length_measures() {
        let g = Guided {
            heading: 1,
            at: [20.0, 42.5],
            millimetres: 12.5,
            typed: false,
            passed_over: None,
        };
        assert_eq!(strings(1, g.at, &anchor_text()), ["20.0", "42.5"]);
        let add = label(&Outcome::Guide(g.clone()), Role::Add { after: 2 }).expect("label");
        assert_eq!(add, "90° · 12.5 mm from vertex 3");
        let drag = label(&Outcome::Guide(g.clone()), Role::Drag { vertex: 2 }).expect("label");
        assert_eq!(drag, "90° · vertex 3 moved 12.5 mm from its start");
        let typed = Guided { typed: true, ..g };
        let drag = label(&Outcome::Guide(typed), Role::Drag { vertex: 2 }).expect("label");
        assert_eq!(drag, "90° · Length 12.5 mm · vertex 3 moved from its start");
        assert_eq!(label(&Outcome::Open, Role::Add { after: 0 }), None);
        for (mode, length, fragment) in [
            (Mode::Off, Length::None, "Guides are off"),
            (Mode::Polar, Length::None, "ringed anchor"),
            (
                Mode::Polar,
                Length::Millimetres(20.0),
                "Length 20 mm is used",
            ),
        ] {
            let (text, error) = note(mode, length, "");
            assert!(!error && text.contains(fragment), "{text}");
        }
        assert_eq!(shown(2.0), "2");
        assert_eq!(shown(12.3456), "12.346");
        assert_eq!(shown(0.0004), "0.0004");
        for length in [0.0004, 12.34567] {
            let g = Guided {
                heading: 0,
                at: [length, 0.0],
                millimetres: length,
                typed: true,
                passed_over: None,
            };
            let words = label(&Outcome::Guide(g), Role::Add { after: 0 }).expect("label");
            assert!(words.contains(&format!("Length {length} mm")), "{words}");
            let (words, _) = note(Mode::Orthogonal, Length::Millimetres(length), "");
            assert!(words.contains(&format!("Length {length} mm")), "{words}");
        }
        println!("\nFCAD_31D_LABELS_EXECUTED");
    }
}
