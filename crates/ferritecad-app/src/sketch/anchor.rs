// SPDX-License-Identifier: MIT
//! §31D-2: the anchor of the explicit "From object" gesture. A shown endpoint,
//! midpoint or finite crossing of the drawn Lines (§31C) is acquired by hovering
//! it, held while the pointer stays near, and confirmed by the press that starts a
//! gesture; the guides of §31D-1 are then measured from it.
//!
//! Geometry is document millimetres; every distance below is in logical egui
//! points, the canvas's points-per-mm applied once per query. Nothing here knows
//! egui, the draft or the document: a provisional anchor is frame state that only
//! proposes where a gesture would start. It creates no constraint, no ID and no
//! history, and its words are for the frame only.
use super::guide::shown;
use super::object_snap::{self, Candidate, Query};

/// A provisional anchor is kept until the pointer is farther than this from it:
/// twice the aperture it was acquired in.
pub(super) const RELEASE_POINTS: f64 = 2.0 * object_snap::APERTURE_POINTS;
/// Another candidate replaces the held one only when nearer by **more than** this;
/// at exactly this the held one stays, so two points never trade places.
pub(super) const REPLACE_POINTS: f64 = object_snap::COMPETITION_POINTS;

fn points_away(candidate: &Candidate, query: &Query) -> f64 {
    (candidate.at[0] - query.cursor[0]).hypot(candidate.at[1] - query.cursor[1])
        * query.points_per_mm
}

/// The provisional anchor once the pointer is at `query.cursor`.
///
/// `candidates` holds every candidate within [`RELEASE_POINTS`] of the pointer (more
/// is harmless). The held anchor stays while it is still one of them, still shown
/// and no farther than [`RELEASE_POINTS`]; a rival takes over only if it would be
/// offered on its own (inside the §31C aperture, shown) and is nearer by more than
/// [`REPLACE_POINTS`], the best of such rivals by the §31C ranking. With nothing
/// held, or after a release, the first anchor is the §31C choice itself: least
/// distance, the group strictly less than one point further ranked by kind, then
/// distance, then stored order. No bonus, no pairwise epsilon, no text.
///
/// A scale that is not a positive finite number, or a non-finite pointer, holds
/// nothing.
pub(super) fn track(
    held: Option<Candidate>,
    candidates: &[Candidate],
    query: &Query,
) -> Option<Candidate> {
    if !query.points_per_mm.is_finite()
        || query.points_per_mm <= 0.0
        || !query.cursor.iter().all(|v| v.is_finite())
    {
        return None;
    }
    if let Some(held) = held
        && candidates.contains(&held)
        && query.visible.contains(held.at)
        && points_away(&held, query) <= RELEASE_POINTS
    {
        let held_away = points_away(&held, query);
        let nearer: Vec<Candidate> = candidates
            .iter()
            .filter(|c| **c != held && held_away - points_away(c, query) > REPLACE_POINTS)
            .copied()
            .collect();
        return Some(object_snap::pick(&nearer, query).unwrap_or(held));
    }
    object_snap::pick(candidates, query)
}

/// What a gesture from an anchor changes, named before it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Moves {
    /// An open New drawing with room: one new vertex, with this 0-based index.
    Adds(usize),
    /// A saved Sketch: its selected vertex, with this 0-based index.
    Vertex(usize),
}

impl Moves {
    pub(super) fn effect(self) -> String {
        match self {
            Self::Adds(i) => format!("adds vertex {}", i + 1),
            Self::Vertex(v) => format!("moves vertex {}", v + 1),
        }
    }
}

/// Where the explicit mode stands, for the standing words painted on the canvas.
#[derive(Debug, Clone, Copy)]
pub(super) enum Situation {
    /// The mode is on but Guides are Off: nothing is offered.
    NeedsGuides,
    /// A closed New drawing: a gesture would add no point.
    Closed,
    /// A New drawing that holds as many points as a profile may.
    Full,
    /// A saved Sketch with no selected vertex: there is nothing to move.
    SelectFirst,
    Looking(Moves),
    /// Acquired by hovering; a press now confirms it.
    Provisional(Candidate, Moves),
    /// Confirmed by the press that is still held: fixed until release.
    Held(Candidate, Moves),
}

/// The anchor in words: its kind, its source and where it is.
pub(super) fn describe(anchor: &Candidate, vertices: usize) -> String {
    format!(
        "{} · {} at ({}, {}) mm",
        anchor.kind.label(),
        anchor.source(vertices),
        shown(anchor.at[0]),
        shown(anchor.at[1]),
    )
}

/// The standing words painted on the canvas while the mode is on: the mode, the
/// anchor and the vertex the gesture would change, in two short lines at most so they
/// fit the canvas and cost the form no height. `vertices` is the contour's point
/// count before the gesture.
pub(super) fn status(situation: Situation, vertices: usize) -> String {
    match situation {
        Situation::NeedsGuides => "From object: choose Orthogonal or Polar 45° first;\n\
                                   it is unavailable while Guides are Off."
            .into(),
        Situation::Closed => {
            "From object: this drawing is closed;\na gesture would add no point.".into()
        }
        Situation::Full => "From object: the drawing holds as many points as it may.".into(),
        Situation::SelectFirst => "From object: select the vertex to move first\n\
                                   (press it), then press another shown point."
            .into(),
        Situation::Looking(moves) => format!(
            "From object: hover an endpoint, midpoint or crossing,\npress it, drag and release; \
             it {}.",
            moves.effect()
        ),
        Situation::Provisional(anchor, moves) => format!(
            "From object: {}\npress to start; it {}.",
            describe(&anchor, vertices),
            moves.effect()
        ),
        Situation::Held(anchor, moves) => format!(
            "From object: from {}\nfixed until release; it {}.",
            describe(&anchor, vertices),
            moves.effect()
        ),
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::object_snap::{SnapKind, Window};
    use super::*;

    fn wide() -> Window {
        Window::new([-1e7, -1e7], [1e7, 1e7])
    }
    fn query(cursor: [f64; 2], points_per_mm: f64) -> Query {
        Query {
            cursor,
            points_per_mm,
            visible: wide(),
        }
    }
    /// The Endpoint candidate of every vertex of a row of points on the x axis, the
    /// vertex `i` being `distances[i]` logical points east of the origin at `scale`.
    /// Only Endpoints, so the tests below compare nothing but distances.
    fn row(distances: &[f64], scale: f64) -> Vec<Candidate> {
        let contour: Vec<[f64; 2]> = distances.iter().map(|d| [d / scale, 0.0]).collect();
        let endpoints = object_snap::collect(&contour, false, None)
            .into_iter()
            .filter(|c| c.kind == SnapKind::Endpoint);
        endpoints.collect()
    }
    /// An open zigzag: one crossing at (10, 10); middles (15, 15), (30, 15), (15, 7.5).
    fn zigzag() -> Vec<Candidate> {
        object_snap::collect(
            &[[0.0, 0.0], [30.0, 30.0], [30.0, 0.0], [0.0, 15.0]],
            false,
            None,
        )
    }
    fn named(all: &[Candidate], kind: SnapKind, at: [f64; 2]) -> Candidate {
        *all.iter()
            .find(|c| {
                c.kind == kind && (c.at[0] - at[0]).abs() < 1e-9 && (c.at[1] - at[1]).abs() < 1e-9
            })
            .expect("that candidate")
    }

    #[test]
    fn a_pointer_acquires_inside_twelve_points_and_holds_until_beyond_twenty_four() {
        for scale in [0.5, 1.0, 2.0, 4.0, 16.0, 100.0] {
            let q = query([0.0, 0.0], scale);
            let alone = |held: bool, points: f64| {
                let only = row(&[points], scale);
                track(held.then_some(only[0]), &only, &q).map(|_| ())
            };
            assert!(
                alone(false, 12.0).is_some(),
                "exactly the aperture, scale {scale}"
            );
            assert!(
                alone(false, 12.001).is_none(),
                "just beyond it, scale {scale}"
            );
            assert!(alone(false, 13.0).is_none());
            // Held, the anchor stays out to twice the aperture, then goes.
            for points in [12.001, 20.0, 24.0] {
                assert!(
                    alone(true, points).is_some(),
                    "held at {points}, scale {scale}"
                );
            }
            assert!(
                alone(true, 24.001).is_none(),
                "beyond twice the aperture the hold ends, scale {scale}"
            );
        }
        // Released by distance, the pointer takes the candidate inside the aperture.
        let both = row(&[30.0, 5.0], 4.0);
        let q = query([0.0, 0.0], 4.0);
        assert_eq!(track(Some(both[0]), &both, &q), Some(both[1]));
        println!("\nFCAD_31H_APERTURE_EXECUTED");
    }

    #[test]
    fn a_nearer_candidate_replaces_the_held_one_only_by_more_than_one_point() {
        for scale in [1.0, 2.0, 4.0] {
            let q = query([0.0, 0.0], scale);
            // Distances are exact binary fractions, so the gap is exactly the figure.
            for (held, rival, replaced) in [
                (5.0, 4.0, false), // exactly one point nearer: the held one stays
                (5.0, 3.9, true),  // 1.1
                (5.0, 4.25, false),
                (5.0, 3.5, true),
                (13.0, 12.0, false), // held beyond the aperture, rival exactly one nearer
                (13.0, 11.75, true),
                (24.0, 23.0, false), // the rival is beyond the aperture: never offered
                (24.0, 11.0, true),
            ] {
                let all = row(&[held, rival], scale);
                let winner = track(Some(all[0]), &all, &q);
                assert_eq!(
                    winner,
                    Some(all[usize::from(replaced)]),
                    "held {held}, rival {rival}, scale {scale}"
                );
                // Asking again at the same pointer changes nothing: no flip-flop.
                assert_eq!(track(winner, &all, &q), winner);
            }
            // Each in turn held at the same pointer, two within a point of each other
            // never trade places.
            let pair = row(&[5.0, 4.5], scale);
            assert_eq!(track(Some(pair[0]), &pair, &q), Some(pair[0]));
            assert_eq!(track(Some(pair[1]), &pair, &q), Some(pair[1]));
        }
        // On a real drawing, with kinds: from (12, 11) the crossing (10, 10) is 2.236
        // away and the held middle (15, 15) is 5 away: replaced. From (14, 9.5) the held
        // crossing is 4.03 away and the middle (15, 7.5) 2.24 away: replaced as well; a
        // pointer 0.4 closer to the held crossing than to that middle keeps it.
        let all = zigzag();
        let crossing = named(&all, SnapKind::Intersection, [10.0, 10.0]);
        let upper = named(&all, SnapKind::Midpoint, [15.0, 15.0]);
        let lower = named(&all, SnapKind::Midpoint, [15.0, 7.5]);
        assert_eq!(
            track(Some(upper), &all, &query([12.0, 11.0], 1.0)),
            Some(crossing)
        );
        assert_eq!(
            track(Some(crossing), &all, &query([14.0, 9.5], 1.0)),
            Some(lower)
        );
        assert_eq!(
            track(Some(crossing), &all, &query([12.0, 9.5], 1.0)),
            Some(crossing)
        );
        println!("\nFCAD_31H_REPLACE_EXECUTED");
    }

    #[test]
    fn the_first_anchor_is_the_object_snap_choice_and_stays_stable_for_any_layout() {
        let scale = 4.0;
        // Segments 1.9, 2.0 and 2.1 points long, the pointer on the middle: the same
        // answers as §31C. Under two points the end wins, from two on the middle.
        let q = query([0.0, 0.0], scale);
        for (points, expected) in [
            (1.9, SnapKind::Endpoint),
            (2.0, SnapKind::Midpoint),
            (2.1, SnapKind::Midpoint),
        ] {
            let half = points / scale / 2.0;
            let all = object_snap::collect(&[[-half, 0.0], [half, 0.0]], false, None);
            let first = track(None, &all, &q).expect("an anchor");
            assert_eq!(first.kind, expected, "{points} points long");
        }
        // The first anchor is the §31C pick itself, wherever the pointer is.
        let all = zigzag();
        for x in 0..=40 {
            for y in 0..=40 {
                let q = query([f64::from(x) * 0.75, f64::from(y) * 0.5], 1.0);
                assert_eq!(
                    track(None, &all, &q),
                    object_snap::pick(&all, &q),
                    "{x},{y}"
                );
            }
        }
        // Coincident vertices are one place; the stored order settles which, in
        // whatever layout the candidates arrive.
        let all: Vec<_> = object_snap::collect(&[[1.0, 0.0], [1.0, 0.0], [1.0, 0.0]], false, None)
            .into_iter()
            .filter(|c| c.kind == SnapKind::Endpoint)
            .collect();
        assert_eq!(all.len(), 3);
        let q = query([1.0, 0.5], scale);
        let first = track(None, &all, &q).expect("an anchor");
        assert_eq!(first.source(3), "vertex 1");
        for layout in [[0, 1, 2], [2, 1, 0], [1, 2, 0], [2, 0, 1]] {
            let permuted: Vec<_> = layout.iter().map(|&i| all[i]).collect();
            assert_eq!(track(None, &permuted, &q), Some(first));
        }
        println!("\nFCAD_31H_FIRST_EXECUTED");
    }

    #[test]
    fn a_held_anchor_that_is_gone_hidden_or_unmeasurable_is_released() {
        let scale = 4.0;
        let q = query([0.0, 0.0], scale);
        let all = row(&[5.0], scale);
        let held = all[0];
        assert_eq!(track(Some(held), &all, &q), Some(held));
        // The drawing changed under it: it is not one of the candidates any more.
        assert_eq!(track(Some(held), &[], &q), None);
        let moved = row(&[5.5], scale);
        assert_eq!(
            track(Some(held), &moved, &q),
            Some(moved[0]),
            "the new one, afresh"
        );
        // Scrolled out of view: neither held nor offered.
        let hidden = Query {
            visible: Window::new([10.0, 10.0], [11.0, 11.0]),
            ..q
        };
        assert_eq!(track(Some(held), &all, &hidden), None);
        // Nothing is measurable without a positive finite scale or a finite pointer.
        for scale in [0.0, -4.0, f64::NAN, f64::INFINITY] {
            let q = query([0.0, 0.0], scale);
            assert_eq!(track(Some(held), &all, &q), None, "scale {scale}");
            assert_eq!(track(None, &all, &q), None, "scale {scale}");
        }
        for cursor in [[f64::NAN, 0.0], [0.0, f64::INFINITY]] {
            assert_eq!(track(Some(held), &all, &query(cursor, scale)), None);
        }
        println!("\nFCAD_31H_RELEASE_EXECUTED");
    }

    #[test]
    fn the_same_pointer_in_points_holds_the_same_anchor_at_any_scale() {
        let mut seen = Vec::new();
        for scale in [0.25, 1.0, 2.0, 8.0, 100.0] {
            // Candidates 11 and 8 points away: in points, always the same answer,
            // fresh or held, and a held one is replaced by the nearer.
            let all = row(&[11.0, 8.0], scale);
            let q = query([0.0, 0.0], scale);
            let fresh = track(None, &all, &q).map(|c| c.source(2));
            let kept = track(Some(all[0]), &all, &q).map(|c| c.source(2));
            let swapped = track(Some(all[1]), &all, &q).map(|c| c.source(2));
            seen.push((fresh, kept, swapped));
        }
        assert!(seen.windows(2).all(|pair| pair[0] == pair[1]), "{seen:?}");
        // Held 11 against 8 is three points: replaced; held 8 stays.
        assert_eq!(seen[0].1.as_deref(), Some("vertex 2"));
        assert_eq!(seen[0].2.as_deref(), Some("vertex 2"));
        println!("\nFCAD_31H_SCALES_EXECUTED");
    }

    #[test]
    fn words_name_the_real_anchor_and_the_vertex_the_gesture_changes() {
        let all = object_snap::collect(&[[0.0, 0.0], [60.0, 0.0], [60.0, 20.0]], true, None);
        let end = named(&all, SnapKind::Endpoint, [60.0, 0.0]);
        assert_eq!(describe(&end, 3), "Endpoint · vertex 2 at (60, 0) mm");
        let middle = named(&all, SnapKind::Midpoint, [30.0, 10.0]);
        assert_eq!(describe(&middle, 3), "Midpoint · edge 3→1 at (30, 10) mm");
        for (situation, fragments) in [
            (
                Situation::NeedsGuides,
                &["Orthogonal or Polar 45°", "Guides are Off"][..],
            ),
            (Situation::Closed, &["closed", "add no point"]),
            (Situation::SelectFirst, &["select the vertex to move first"]),
            (
                Situation::Looking(Moves::Adds(3)),
                &[
                    "hover an endpoint, midpoint or crossing",
                    "it adds vertex 4",
                ],
            ),
            (
                Situation::Provisional(end, Moves::Vertex(0)),
                &[
                    "Endpoint · vertex 2 at (60, 0) mm",
                    "press to start",
                    "it moves vertex 1",
                ],
            ),
            (
                Situation::Held(middle, Moves::Adds(3)),
                &[
                    "from Midpoint · edge 3→1",
                    "fixed until release",
                    "it adds vertex 4",
                ],
            ),
        ] {
            let words = status(situation, 3);
            for fragment in fragments {
                assert!(words.contains(fragment), "{fragment:?} in {words:?}");
            }
        }
        println!("\nFCAD_31H_WORDS_EXECUTED");
    }
}
