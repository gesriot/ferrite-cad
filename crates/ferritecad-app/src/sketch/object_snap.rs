// SPDX-License-Identifier: MIT
//! §31C: object snaps for the Sketch canvas. Candidate points of the Line
//! contour the canvas actually draws, and the two-pass choice among them.
//!
//! Geometry is document millimetres; the choice is made in logical egui points
//! (the canvas's points-per-mm turns one into the other exactly once per query).
//! Nothing here knows egui, the draft or the document: a snap is editor state,
//! it creates no constraint, no ID and no history.
use ferritecad_jobs::PolygonExtrusion;

/// How far from the pointer a candidate may be and still be offered, in logical
/// points. Larger than the 9-point vertex-picking radius, so that a midpoint or a
/// crossing just outside a vertex is still reachable.
pub(super) const APERTURE_POINTS: f64 = 12.0;
/// Candidates whose distance exceeds the nearest one's by **strictly less** than
/// this compete on kind first; at exactly this gap the nearer wins.
pub(super) const COMPETITION_POINTS: f64 = 1.0;
/// Two points closer than this are the same point, and an edge shorter than this
/// has no direction: the document's own tolerance, below which the polygon
/// policy already refuses "repeated vertices or a zero-length edge". It is a
/// model length, not a screen distance, and unrelated to the aperture.
pub(super) const COINCIDENT_MM: f64 = PolygonExtrusion::TOLERANCE_MM;
/// Two edges whose cross product is at most this fraction of the product of their
/// lengths (the sine of the angle between them) are parallel for this purpose.
/// The cross product of f64 coordinates carries about 1e-16 of relative noise;
/// 1e-9 leaves seven orders above it and still finds crossings below a
/// millionth of a degree. Collinear overlap is a case of it: no single point.
const PARALLEL_SINE: f64 = 1e-9;

/// What a candidate is. The declaration order **is** the priority order of the
/// ranking (Intersection, Endpoint, Midpoint); a circle centre, when the canvas
/// draws a circle, goes after Midpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SnapKind {
    Intersection,
    Endpoint,
    Midpoint,
}

impl SnapKind {
    /// The name shown beside the pointer for the frame; never stored or compared.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Intersection => "Intersection",
            Self::Endpoint => "Endpoint",
            Self::Midpoint => "Midpoint",
        }
    }
}

/// One snap point. `order` is the stored order of the entity that makes it
/// (vertex index; segment index; the two segment indices of a crossing, lower
/// first), so ties are settled by the drawing and never by hashing or text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Candidate {
    pub(super) kind: SnapKind,
    pub(super) at: [f64; 2],
    order: (usize, usize),
}

impl Candidate {
    /// What makes this snap, in the canvas's own 1-based numbers: a vertex, an edge
    /// (`from>to` around a contour of `vertices` points) or the two edges that cross.
    /// Words for the frame only; never stored or compared.
    pub(super) fn source(&self, vertices: usize) -> String {
        let edge = |i: usize| format!("{}>{}", i + 1, (i + 1) % vertices.max(1) + 1);
        match self.kind {
            SnapKind::Endpoint => format!("vertex {}", self.order.0 + 1),
            SnapKind::Midpoint => format!("edge {}", edge(self.order.0)),
            SnapKind::Intersection => {
                format!("edges {} × {}", edge(self.order.0), edge(self.order.1))
            }
        }
    }
}

/// An axis-aligned rectangle in document millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Window {
    lo: [f64; 2],
    hi: [f64; 2],
}

impl Window {
    pub(super) fn new(a: [f64; 2], b: [f64; 2]) -> Self {
        Self {
            lo: [a[0].min(b[0]), a[1].min(b[1])],
            hi: [a[0].max(b[0]), a[1].max(b[1])],
        }
    }
    /// The square that contains every point within `radius` of `centre`, with a
    /// hair of slack so rounding never drops a point the exact distance keeps.
    pub(super) fn around(centre: [f64; 2], radius: f64) -> Self {
        let radius = radius * (1.0 + 1e-9);
        Self::new(
            [centre[0] - radius, centre[1] - radius],
            [centre[0] + radius, centre[1] + radius],
        )
    }
    pub(super) fn contains(&self, p: [f64; 2]) -> bool {
        (0..2).all(|i| p[i] >= self.lo[i] && p[i] <= self.hi[i])
    }
    fn overlaps(&self, a: [f64; 2], b: [f64; 2]) -> bool {
        // `crossing` accepts a junction within the endpoint tolerance. Keep
        // those edges in this broad phase even when their exact bounds miss.
        (0..2).all(|i| {
            a[i].min(b[i]) <= self.hi[i] + COINCIDENT_MM
                && a[i].max(b[i]) >= self.lo[i] - COINCIDENT_MM
        })
    }
}

/// What the pointer asks: where (document mm), how many logical points one mm is
/// on the canvas, and which part of the document is visible.
#[derive(Debug, Clone, Copy)]
pub(super) struct Query {
    pub(super) cursor: [f64; 2],
    pub(super) points_per_mm: f64,
    pub(super) visible: Window,
}

fn finite(p: [f64; 2]) -> bool {
    p[0].is_finite() && p[1].is_finite()
}

/// The text of a coordinate that came from an object: the shortest decimal that
/// reads back as the same number. Not rounded to the grid or to three places.
pub(super) fn coordinate(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        value.to_string()
    }
}

/// The single point where two **finite** segments cross, if there is exactly one
/// and it is not simply a shared end.
///
/// * An edge no longer than [`COINCIDENT_MM`] has no direction: no point.
/// * Parallel, nearly parallel (see [`PARALLEL_SINE`]) and collinear segments,
///   overlapping or not: no point. An overlap has no single crossing to choose.
/// * Extensions beyond an end do not count; an end within [`COINCIDENT_MM`] of the
///   other segment does, so a T-junction is a crossing.
/// * A meeting that is an end of **both** segments is an Endpoint, not a crossing.
/// * A non-finite input or result gives no point.
fn crossing(a0: [f64; 2], a1: [f64; 2], b0: [f64; 2], b1: [f64; 2]) -> Option<[f64; 2]> {
    let d = [a1[0] - a0[0], a1[1] - a0[1]];
    let e = [b1[0] - b0[0], b1[1] - b0[1]];
    let (dd, ee) = (d[0].hypot(d[1]), e[0].hypot(e[1]));
    let cross = |u: [f64; 2], v: [f64; 2]| u[0] * v[1] - u[1] * v[0];
    let denominator = cross(d, e);
    // NaN and infinity end here, so what follows compares numbers.
    if ![dd, ee, denominator].iter().all(|v| v.is_finite()) {
        return None;
    }
    if dd <= COINCIDENT_MM || ee <= COINCIDENT_MM {
        return None;
    }
    if denominator.abs() <= PARALLEL_SINE * dd * ee {
        return None;
    }
    let w = [b0[0] - a0[0], b0[1] - a0[1]];
    // The crossing is a0 + d·(along_a / dd) and b0 + e·(along_b / ee); the
    // distances along each segment are in mm so the tolerance is a length.
    let numerator = cross(w, e);
    let along_a = numerator / denominator * dd;
    let along_b = cross(w, d) / denominator * ee;
    let on = |along: f64, length: f64| along >= -COINCIDENT_MM && along <= length + COINCIDENT_MM;
    let at_end =
        |along: f64, length: f64| along <= COINCIDENT_MM || length - along <= COINCIDENT_MM;
    if !(on(along_a, dd) && on(along_b, ee)) || (at_end(along_a, dd) && at_end(along_b, ee)) {
        return None;
    }
    // Multiply before dividing so that whole-number drawings cross at whole
    // numbers; an end within the tolerance of the other segment is that end.
    let at = if along_a <= 0.0 {
        a0
    } else if along_a >= dd {
        a1
    } else {
        [
            a0[0] + d[0] * numerator / denominator,
            a0[1] + d[1] * numerator / denominator,
        ]
    };
    finite(at).then_some(at)
}

/// Every candidate of the contour as drawn: one Endpoint per vertex, one
/// Midpoint per edge, one Intersection per pair of non-adjacent edges that cross.
/// The edges are `points[i] > points[i + 1]`, and `points[last] > points[0]` once
/// the contour is closed with more than two vertices (with two, that edge is the
/// first one drawn twice).
///
/// With a `window` only candidates inside it are made, and only edges that reach
/// it are tested against each other, so a pointer query stays proportional to
/// what is near it. The result is exactly the unrestricted one cut to the window.
pub(super) fn collect(points: &[[f64; 2]], closed: bool, window: Option<Window>) -> Vec<Candidate> {
    let near = |p: [f64; 2]| window.is_none_or(|w| w.contains(p));
    let mut out = Vec::new();
    for (i, &at) in points.iter().enumerate() {
        if finite(at) && near(at) {
            out.push(Candidate {
                kind: SnapKind::Endpoint,
                at,
                order: (i, 0),
            });
        }
    }
    let n = points.len();
    let wraps = closed && n > 2;
    let edges = match n {
        0 | 1 => 0,
        _ if wraps => n,
        _ => n - 1,
    };
    let edge = |i: usize| (points[i], points[(i + 1) % n]);
    for i in 0..edges {
        let (a, b) = edge(i);
        let at = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        if (b[0] - a[0]).hypot(b[1] - a[1]) > COINCIDENT_MM && finite(at) && near(at) {
            out.push(Candidate {
                kind: SnapKind::Midpoint,
                at,
                order: (i, 0),
            });
        }
    }
    let reaching: Vec<usize> = (0..edges)
        .filter(|&i| {
            let (a, b) = edge(i);
            window.is_none_or(|w| w.overlaps(a, b))
        })
        .collect();
    for (k, &i) in reaching.iter().enumerate() {
        for &j in &reaching[k + 1..] {
            // Edges that share a vertex meet there, and nowhere else unless they
            // are collinear; that meeting is the Endpoint.
            if j == i + 1 || (wraps && i == 0 && j == edges - 1) {
                continue;
            }
            let ((a0, a1), (b0, b1)) = (edge(i), edge(j));
            if let Some(at) = crossing(a0, a1, b0, b1)
                && near(at)
            {
                out.push(Candidate {
                    kind: SnapKind::Intersection,
                    at,
                    order: (i, j),
                });
            }
        }
    }
    out
}

/// The candidates a vertex drag keeps from its press: all of the contour's, minus
/// every one at the vertex's own press-time position (itself, a vertex stacked on
/// it, a crossing through it), within [`COINCIDENT_MM`]. Midpoints and crossings
/// of the incident edges stay where the press found them; nothing is recomputed
/// from the draft while it moves.
pub(super) fn frozen(points: &[[f64; 2]], closed: bool, moving: [f64; 2]) -> Vec<Candidate> {
    let mut all = collect(points, closed, None);
    drop_at(&mut all, moving);
    all
}

/// Removes every candidate within [`COINCIDENT_MM`] of `place`: the one rule that
/// keeps a moving vertex, and what stacks on it, out of a snap set.
pub(super) fn drop_at(candidates: &mut Vec<Candidate>, place: [f64; 2]) {
    candidates.retain(|c| (c.at[0] - place[0]).hypot(c.at[1] - place[1]) > COINCIDENT_MM);
}

/// The winner for one query, or none.
///
/// Pass one: the least distance among the visible candidates inside the
/// aperture. Pass two: those within [`COMPETITION_POINTS`] of it, strictly, are
/// ranked by kind, then distance, then stored order. The order is total because
/// `(kind, order)` is unique, so it never depends on how `candidates` is laid
/// out or iterated.
pub(super) fn pick(candidates: &[Candidate], query: &Query) -> Option<Candidate> {
    let distance = |c: &Candidate| {
        (c.at[0] - query.cursor[0]).hypot(c.at[1] - query.cursor[1]) * query.points_per_mm
    };
    let offered = |c: &&Candidate| query.visible.contains(c.at) && distance(c) <= APERTURE_POINTS;
    let nearest = candidates
        .iter()
        .filter(offered)
        .map(distance)
        .min_by(f64::total_cmp)?;
    candidates
        .iter()
        .filter(offered)
        .map(|c| (c, distance(c)))
        .filter(|&(_, d)| d - nearest < COMPETITION_POINTS)
        .min_by(|(a, da), (b, db)| {
            a.kind
                .cmp(&b.kind)
                .then(da.total_cmp(db))
                .then(a.order.cmp(&b.order))
        })
        .map(|(c, _)| *c)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: Window = Window {
        lo: [-1e7, -1e7],
        hi: [1e7, 1e7],
    };

    fn query(cursor: [f64; 2], points_per_mm: f64) -> Query {
        Query {
            cursor,
            points_per_mm,
            visible: WIDE,
        }
    }
    fn candidate(kind: SnapKind, at: [f64; 2], order: (usize, usize)) -> Candidate {
        Candidate { kind, at, order }
    }
    fn of_kind(all: &[Candidate], kind: SnapKind) -> Vec<[f64; 2]> {
        let mut at: Vec<_> = all
            .iter()
            .filter(|c| c.kind == kind)
            .map(|c| c.at)
            .collect();
        at.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
        at
    }
    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn kinds_rank_intersection_endpoint_midpoint_and_name_themselves() {
        assert!(SnapKind::Intersection < SnapKind::Endpoint);
        assert!(SnapKind::Endpoint < SnapKind::Midpoint);
        assert_eq!(
            [
                SnapKind::Intersection,
                SnapKind::Endpoint,
                SnapKind::Midpoint
            ]
            .map(SnapKind::label),
            ["Intersection", "Endpoint", "Midpoint"]
        );
    }

    /// A segment 1.9, 2.0 and 2.1 logical points long, the pointer on its middle:
    /// the ends are 0.95, 1.0 and 1.05 points away. Below one point of gap the
    /// kind decides (the end wins); at exactly one point and above, the nearer.
    #[test]
    fn crowded_short_segments_resolve_either_side_of_two_points() {
        let points_per_mm = 4.0;
        for (points, expected) in [
            (1.9, SnapKind::Endpoint),
            (2.0, SnapKind::Midpoint),
            (2.1, SnapKind::Midpoint),
        ] {
            let length = points / points_per_mm;
            let contour = [[0.0, 0.0], [length, 0.0]];
            let all = collect(&contour, false, None);
            let middle = [length / 2.0, 0.0];
            let hit = pick(&all, &query(middle, points_per_mm)).expect("hit");
            assert_eq!(hit.kind, expected, "{points} points long");
            if expected == SnapKind::Endpoint {
                // The two ends are equally far: the stored order settles it.
                assert_eq!(hit.at, [0.0, 0.0]);
            } else {
                assert_eq!(hit.at, middle);
            }
        }
    }

    #[test]
    fn a_gap_of_exactly_one_point_gives_the_nearest_and_less_gives_the_kind() {
        let ppm = 2.0;
        let end = |x: f64| candidate(SnapKind::Endpoint, [x, 0.0], (0, 0));
        let crossing = |x: f64| candidate(SnapKind::Intersection, [x, 0.0], (0, 1));
        let winner = |x: f64| {
            pick(&[end(1.5), crossing(x)], &query([0.0, 0.0], ppm))
                .expect("hit")
                .kind
        };
        // End at 3.0 points; the crossing at 4.0 is exactly one point further.
        assert_eq!(winner(2.0), SnapKind::Endpoint);
        assert_eq!(winner(2.01), SnapKind::Endpoint, "1.02 points further");
        assert_eq!(winner(1.99), SnapKind::Intersection, "0.98 points further");
        assert_eq!(winner(1.5), SnapKind::Intersection, "same distance, kind");
    }

    #[test]
    fn ties_follow_kind_then_distance_then_stored_order_in_any_layout() {
        let a = candidate(SnapKind::Midpoint, [0.5, 0.0], (0, 0)); // 0.5 points
        let b = candidate(SnapKind::Endpoint, [0.0, 0.7], (2, 0)); // 0.7
        let c = candidate(SnapKind::Endpoint, [-0.7, 0.0], (1, 0)); // 0.7, earlier
        let d = candidate(SnapKind::Endpoint, [0.0, -0.6], (3, 0)); // 0.6
        let all = [a, b, c, d];
        let at = query([0.0, 0.0], 1.0);
        // Every ordering of the same candidates picks the same one: kind beats
        // distance (all within a point of 0.5), then the nearer end, not stored order.
        let mut permutations = 0;
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    for l in 0..4 {
                        if [i, j, k, l]
                            .iter()
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == 4
                        {
                            let layout = [all[i], all[j], all[k], all[l]];
                            assert_eq!(pick(&layout, &at), Some(d));
                            permutations += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(permutations, 24);
        // With the nearer end gone, the equal ends are settled by stored order.
        for layout in [[b, c, a], [c, a, b], [a, b, c]] {
            assert_eq!(pick(&layout, &at), Some(c));
        }
        // The same ends and the same distance, but the later one listed first.
        assert_eq!(pick(&[b, c], &at), Some(c));
    }

    #[test]
    fn the_aperture_is_inclusive_and_candidates_beyond_it_never_compete() {
        let ppm = 4.0;
        let at = |points: f64, kind| candidate(kind, [points / ppm, 0.0], (0, 0));
        let origin = query([0.0, 0.0], ppm);
        assert!(pick(&[at(APERTURE_POINTS, SnapKind::Midpoint)], &origin).is_some());
        assert!(pick(&[at(APERTURE_POINTS + 0.001, SnapKind::Midpoint)], &origin).is_none());
        // 11.5 is offered; a crossing at 12.2 is within a point of it but outside.
        let hit = pick(
            &[
                at(11.5, SnapKind::Midpoint),
                at(12.2, SnapKind::Intersection),
            ],
            &origin,
        )
        .expect("hit");
        assert_eq!(hit.kind, SnapKind::Midpoint);
        // A candidate that is not visible is not offered, however near.
        let hidden = Query {
            visible: Window::new([1.0, 1.0], [2.0, 2.0]),
            ..origin
        };
        assert!(pick(&[at(1.0, SnapKind::Endpoint)], &hidden).is_none());
    }

    /// The same drawing on screen at three canvas scales: the choice is made in
    /// points, so it is the same, whatever a millimetre is worth.
    #[test]
    fn the_choice_depends_on_logical_points_not_on_canvas_scale() {
        // Two long edges that cross, and a short one whose middle is near that
        // crossing; coordinates in points, converted to mm by the scale.
        let screen = [[0.0, 0.0], [40.0, 40.0], [40.0, 0.0], [0.0, 40.0]];
        let mut winners = Vec::new();
        for ppm in [0.5, 1.0, 2.0, 8.0] {
            let mm = screen.map(|p| [p[0] / ppm, p[1] / ppm]);
            let all = collect(&mm, false, None);
            for (pointer, label) in [([20.0, 20.0], "on the crossing"), ([22.0, 21.0], "near")] {
                let cursor = [pointer[0] / ppm, pointer[1] / ppm];
                let hit = pick(&all, &query(cursor, ppm)).expect(label);
                winners.push((
                    label,
                    hit.kind,
                    (hit.at[0] * ppm).round(),
                    (hit.at[1] * ppm).round(),
                ));
            }
        }
        let first = winners[..2].to_vec();
        for chunk in winners.chunks(2) {
            assert_eq!(chunk, first.as_slice());
        }
        assert_eq!(first[0].1, SnapKind::Intersection);
    }

    #[test]
    fn crossings_are_of_finite_segments_not_of_their_extensions() {
        // Lines meet at (12, 0), beyond the end (10, 0) of the first segment.
        let extension = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 3.0],
            [12.0, 3.0],
            [12.0, -5.0],
        ];
        assert!(of_kind(&collect(&extension, false, None), SnapKind::Intersection).is_empty());
        // Move the second one back under the first and they do cross.
        let crossing = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 3.0],
            [8.0, 3.0],
            [8.0, -5.0],
        ];
        let all = collect(&crossing, false, None);
        assert_eq!(of_kind(&all, SnapKind::Intersection), [[8.0, 0.0]]);
        // A zigzag crosses once at its middle; closing it adds edges but no more.
        let zigzag = [[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
        for closed in [false, true] {
            let all = collect(&zigzag, closed, None);
            assert_eq!(of_kind(&all, SnapKind::Intersection), [[5.0, 5.0]]);
            assert_eq!(of_kind(&all, SnapKind::Endpoint).len(), 4);
            assert_eq!(
                of_kind(&all, SnapKind::Midpoint).len(),
                if closed { 4 } else { 3 }
            );
        }
    }

    #[test]
    fn degenerate_parallel_overlapping_and_shared_cases_make_no_crossing() {
        let cross = |a0, a1, b0, b1| crossing(a0, a1, b0, b1);
        // Zero-length edge, on either side, even sitting on the other segment.
        assert_eq!(
            cross([5.0, 0.0], [5.0, 0.0], [0.0, -1.0], [10.0, 1.0]),
            None
        );
        assert_eq!(
            cross([0.0, -1.0], [10.0, 1.0], [5.0, 0.0], [5.0, 0.0]),
            None
        );
        // An edge shorter than the document tolerance has no direction either.
        assert_eq!(
            cross([5.0, 0.0], [5.0, 5e-7], [0.0, 2e-7], [10.0, 2e-7]),
            None
        );
        // Parallel, collinear overlap, collinear end to end: no single point.
        assert_eq!(
            cross([0.0, 0.0], [10.0, 0.0], [0.0, 1.0], [10.0, 1.0]),
            None
        );
        assert_eq!(
            cross([0.0, 0.0], [10.0, 0.0], [5.0, 0.0], [15.0, 0.0]),
            None
        );
        assert_eq!(
            cross([0.0, 0.0], [10.0, 0.0], [10.0, 0.0], [20.0, 0.0]),
            None
        );
        assert_eq!(
            cross([0.0, 0.0], [10.0, 0.0], [20.0, 0.0], [5.0, 0.0]),
            None
        );
        // Nearly parallel: a sine of 1e-10 is parallel, one of 1e-6 is a crossing
        // whose point is finite and on both segments.
        let slope = |s: f64| {
            cross(
                [0.0, 0.0],
                [1000.0, 0.0],
                [0.0, -s * 500.0],
                [1000.0, s * 500.0],
            )
        };
        assert_eq!(slope(1e-10), None);
        let tilted = slope(1e-6).expect("a real, if shallow, crossing");
        assert!(near(tilted, [500.0, 0.0]) && tilted.iter().all(|v| v.is_finite()));
        // Shared ends are Endpoints, whether the edges are adjacent in the drawing
        // or two vertices merely coincide.
        assert_eq!(cross([0.0, 0.0], [5.0, 5.0], [5.0, 5.0], [9.0, 0.0]), None);
        assert_eq!(cross([0.0, 0.0], [5.0, 5.0], [9.0, 0.0], [5.0, 5.0]), None);
        // A T-junction, either way round, is one crossing at the touching end.
        assert_eq!(
            cross([0.0, 0.0], [10.0, 0.0], [5.0, 0.0], [5.0, 5.0]),
            Some([5.0, 0.0])
        );
        assert_eq!(
            cross([5.0, 5.0], [5.0, 0.0], [0.0, 0.0], [10.0, 0.0]),
            Some([5.0, 0.0])
        );
        // Within the tolerance of the other edge counts, beyond it does not.
        assert!(cross([0.0, 0.0], [10.0, 0.0], [5.0, 5e-7], [5.0, 5.0]).is_some());
        assert_eq!(
            cross([0.0, 0.0], [10.0, 0.0], [5.0, 2e-6], [5.0, 5.0]),
            None
        );
        // Non-finite or enormous inputs give nothing and do not panic.
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(cross([0.0, 0.0], [bad, 0.0], [5.0, -1.0], [5.0, 1.0]), None);
            assert_eq!(
                cross([0.0, bad], [10.0, 0.0], [5.0, -1.0], [5.0, 1.0]),
                None
            );
            let all = collect(
                &[[0.0, 0.0], [bad, 1.0], [3.0, 3.0], [0.0, 3.0]],
                true,
                None,
            );
            assert!(all.iter().all(|c| finite(c.at)), "{bad}");
        }
        let far = cross([-1e6, -1e6], [1e6, 1e6], [-1e6, 1e6], [1e6, -1e6]).expect("crosses");
        assert!(near(far, [0.0, 0.0]));

        // In a drawing: a repeated vertex, a zero-length edge, a fold back on itself
        // and a stacked vertex offer their ends and nothing invented.
        let stacked = [[0.0, 0.0], [10.0, 0.0], [10.0, 0.0], [5.0, 0.0], [5.0, 5.0]];
        let all = collect(&stacked, false, None);
        assert_eq!(of_kind(&all, SnapKind::Intersection), [[5.0, 0.0]]);
        assert_eq!(
            of_kind(&all, SnapKind::Midpoint).len(),
            3,
            "not the zero edge's"
        );
        assert_eq!(of_kind(&all, SnapKind::Endpoint).len(), 5);
    }

    #[test]
    fn a_window_makes_exactly_the_unrestricted_candidates_inside_it() {
        // A star: eleven vertices, every other one joined, many crossings.
        let star: Vec<[f64; 2]> = (0..11)
            .map(|i| {
                let a = f64::from((i * 5) % 11) * std::f64::consts::TAU / 11.0;
                [30.0 * a.cos(), 30.0 * a.sin()]
            })
            .collect();
        for closed in [false, true] {
            let all = collect(&star, closed, None);
            assert!(of_kind(&all, SnapKind::Intersection).len() >= 10);
            for (centre, radius) in [([0.0, 0.0], 3.0), ([12.0, 4.0], 9.0), ([-25.0, 5.0], 40.0)] {
                let window = Window::around(centre, radius);
                let key = |c: &Candidate| (c.kind, c.order);
                let mut expected: Vec<_> = all
                    .iter()
                    .filter(|c| window.contains(c.at))
                    .map(key)
                    .collect();
                let mut got: Vec<_> = collect(&star, closed, Some(window))
                    .iter()
                    .map(key)
                    .collect();
                expected.sort();
                got.sort();
                assert_eq!(got, expected, "closed={closed} around {centre:?}");
            }
        }
        // The second edge stops just short of a T-junction accepted by the
        // crossing tolerance. At the query boundary its exact bounding box
        // misses the window, but the accepted crossing is inside it.
        let touching = [[0., 0.125], [20., 0.125], [5., 0.1250005], [5., 4.]];
        let window = Window::around([5., 0.], 0.125);
        let expected: Vec<_> = collect(&touching, false, None)
            .into_iter()
            .filter(|c| window.contains(c.at))
            .collect();
        assert_eq!(of_kind(&expected, SnapKind::Intersection), [[5., 0.125]]);
        assert_eq!(collect(&touching, false, Some(window)), expected);
    }

    #[test]
    fn reversing_the_drawing_gives_the_same_points() {
        let drawing = [
            [0.0, 0.0],
            [10.0, 10.0],
            [10.0, 0.0],
            [0.0, 10.0],
            [4.0, 12.0],
        ];
        let reversed: Vec<_> = drawing.iter().rev().copied().collect();
        for closed in [false, true] {
            let a = collect(&drawing, closed, None);
            let b = collect(&reversed, closed, None);
            for kind in [
                SnapKind::Intersection,
                SnapKind::Endpoint,
                SnapKind::Midpoint,
            ] {
                let (a, b) = (of_kind(&a, kind), of_kind(&b, kind));
                assert_eq!(a.len(), b.len(), "{kind:?} closed={closed}");
                assert!(a.iter().zip(&b).all(|(p, q)| near(*p, *q)), "{kind:?}");
            }
        }
    }

    #[test]
    fn a_drag_keeps_the_press_candidates_without_the_vertex_and_what_sits_on_it() {
        // The L of the editor's own tests, closed; vertex 1 (60, 0) is dragged.
        let l = [
            [0.0, 0.0],
            [60.0, 0.0],
            [60.0, 20.0],
            [20.0, 20.0],
            [20.0, 40.0],
            [0.0, 40.0],
        ];
        let all = collect(&l, true, None);
        let kept = frozen(&l, true, [60.0, 0.0]);
        assert_eq!(kept.len(), all.len() - 1, "only the vertex itself");
        assert!(kept.iter().all(|c| c.at != [60.0, 0.0]));
        // The midpoints of both incident edges are where the press found them.
        let midpoints = of_kind(&kept, SnapKind::Midpoint);
        assert!(midpoints.contains(&[30.0, 0.0]) && midpoints.contains(&[60.0, 10.0]));
        // A foreign vertex stacked on the press position goes with it, within the
        // document tolerance and not beyond: half of it yes, twice it no (aperture-free).
        for (offset, dropped) in [(0.0, true), (5e-7, true), (2e-6, false)] {
            let mut stacked = l.to_vec();
            stacked.push([60.0 + offset, 0.0]);
            stacked.push([61.0, 5.0]);
            let kept = frozen(&stacked, true, [60.0, 0.0]);
            let endpoints = of_kind(&kept, SnapKind::Endpoint);
            assert_eq!(
                endpoints
                    .iter()
                    .any(|p| (p[0] - 60.0).abs() < 1e-5 && p[1] == 0.0),
                !dropped,
                "offset {offset}"
            );
        }
        // Something that is neither: a crossing at the press position is dropped,
        // a crossing elsewhere on an incident edge stays frozen.
        let bow = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [5.0, -5.0],
            [5.0, 5.0],
        ];
        let kept = frozen(&bow, false, [10.0, 0.0]);
        assert!(
            of_kind(&kept, SnapKind::Intersection)
                .iter()
                .any(|p| near(*p, [5.0, 0.0]))
        );
    }

    /// The editor's largest drawing: 256 vertices, every edge crossing many.
    #[test]
    fn the_largest_drawing_has_a_bounded_finite_set_and_a_cheap_pointer_query() {
        let star: Vec<[f64; 2]> = (0..PolygonExtrusion::MAX_POINTS)
            .map(|i| {
                let a = ((i * 127) % 256) as f64 * std::f64::consts::TAU / 256.0;
                [1000.0 * a.cos(), 1000.0 * a.sin()]
            })
            .collect();
        let all = collect(&star, true, None);
        let crossings = of_kind(&all, SnapKind::Intersection).len();
        assert!(crossings > 10_000, "{crossings}");
        println!(
            "\nlargest drawing: {} candidates, {crossings} crossings",
            all.len()
        );
        assert_eq!(of_kind(&all, SnapKind::Endpoint).len(), 256);
        assert_eq!(of_kind(&all, SnapKind::Midpoint).len(), 256);
        assert!(all.iter().all(|c| finite(c.at)));
        assert!(all.len() <= 256 + 256 + 256 * 255 / 2);
        // A hover at a corner: the window keeps what is near, not the whole set.
        let corner = Window::around([990.0, 0.0], 3.0);
        let near = collect(&star, true, Some(corner));
        assert!(
            near.len() < all.len() / 50,
            "{} of {}",
            near.len(),
            all.len()
        );
        // Pressing the first vertex removes only what sits on it.
        let kept = frozen(&star, true, star[0]);
        assert!(kept.len() < all.len() && kept.len() + 300 > all.len());
    }

    #[test]
    fn coordinates_from_objects_are_exact_and_unrounded() {
        assert_eq!(coordinate(0.0), "0");
        assert_eq!(coordinate(-0.0), "0");
        assert_eq!(coordinate(80.0), "80");
        assert_eq!(coordinate(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(coordinate(-12.3456789), "-12.3456789");
        for v in [1.0 / 3.0, 5e-7, 123456.789012345, -1e-9] {
            assert_eq!(coordinate(v).parse::<f64>().expect("number"), v);
        }
    }
}
