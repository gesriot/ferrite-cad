// SPDX-License-Identifier: MIT
//! The outer wall a circular Cut must stay inside, read from the saved Lines.
//!
//! One value, read once per snapshot and carried by every catalogue, draft
//! check, preparation and writer re-derivation. Its axis-aligned bounding box
//! is reported as *bounds* and is never used to decide containment; see
//! [`CutBoundary::disk_clearance`] for the rule that is.
use ferritecad_types::{CadError, Result, StableEntityId};

use crate::{PolygonExtrusion, SketchCurve, SketchGeometry};

/// How far one stored Line end may be from the next Line's start and still
/// close the profile. The kernel's linear tolerance, as the rectangle reader
/// that preceded this one used.
const CLOSURE_MM: f64 = crate::cut_edit::WALL_CLEARANCE_MM;

/// One saved side of the part, exactly as stored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundarySegment {
    /// The Line's own saved identity; side names are derived from it.
    pub curve_id: StableEntityId,
    pub start_mm: [f64; 2],
    pub end_mm: [f64; 2],
}

/// The winding the person drew. Reported, never normalised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryOrientation {
    CounterClockwise,
    Clockwise,
}

/// A simple closed Line polygon: the part's outer wall in the base XY datum.
#[derive(Debug, Clone, PartialEq)]
pub struct CutBoundary {
    segments: Vec<BoundarySegment>,
    orientation: BoundaryOrientation,
}

/// Where a disk sits relative to the boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiskClearance {
    /// Whether the centre is inside the polygon.
    pub center_inside: bool,
    /// The smallest distance from the disk's rim to any finite segment,
    /// `min(distance(center, segment)) - radius`. Negative when the disk
    /// crosses the wall.
    pub clearance_mm: f64,
    /// The saved Line that closest approach is measured to.
    pub nearest: StableEntityId,
}

impl CutBoundary {
    /// Reads the saved profile, or says why it is outside this class.
    ///
    /// Lines only, none construction, stored end to end and closing, and a
    /// polygon the shared creation policy accepts at this height.
    pub(crate) fn read(curves: &[SketchCurve], height_mm: f64) -> Result<Self> {
        if curves.len() < 3 || curves.len() > PolygonExtrusion::MAX_POINTS {
            return Err(CadError::unsupported(
                "this slice cuts a part whose profile is 3..256 Lines",
            ));
        }
        let mut segments = Vec::with_capacity(curves.len());
        for curve in curves {
            if curve.construction {
                return Err(CadError::unsupported(
                    "construction geometry bounds no face",
                ));
            }
            let SketchGeometry::Line { start, end } = curve.geometry else {
                return Err(CadError::unsupported(
                    "this slice cuts a part whose profile is drawn with Lines only",
                ));
            };
            segments.push(BoundarySegment {
                curve_id: curve.id,
                start_mm: [start.x, start.y],
                end_mm: [end.x, end.y],
            });
        }
        for (index, segment) in segments.iter().enumerate() {
            let next = segments[(index + 1) % segments.len()].start_mm;
            if (segment.end_mm[0] - next[0]).abs() > CLOSURE_MM
                || (segment.end_mm[1] - next[1]).abs() > CLOSURE_MM
            {
                return Err(CadError::unsupported(
                    "this slice cuts a closed profile, and these Lines do not meet end to end in \
                     stored order",
                ));
            }
        }
        let boundary = Self {
            orientation: BoundaryOrientation::CounterClockwise,
            segments,
        };
        // The one simplicity policy creation applies, asked of the saved
        // numbers so a part outside it is refused rather than narrowed.
        let polygon = boundary.polygon(height_mm).map_err(|e| {
            CadError::unsupported(format!("the saved part is outside cut policy: {e}"))
        })?;
        let orientation = if signed_twice_area(polygon.points()) > 0. {
            BoundaryOrientation::CounterClockwise
        } else {
            BoundaryOrientation::Clockwise
        };
        Ok(Self {
            orientation,
            ..boundary
        })
    }

    /// The shared validator at another height, e.g. for a base height edit.
    pub(crate) fn polygon(&self, height_mm: f64) -> Result<PolygonExtrusion> {
        PolygonExtrusion::new(
            self.segments.iter().map(|s| s.start_mm).collect(),
            height_mm,
        )
    }

    pub fn segments(&self) -> &[BoundarySegment] {
        &self.segments
    }

    pub fn orientation(&self) -> BoundaryOrientation {
        self.orientation
    }

    /// `[[min_x, min_y], [max_x, max_y]]`. A range to offer numbers in, never
    /// evidence of containment.
    pub fn bounds_mm(&self) -> [[f64; 2]; 2] {
        let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        for s in &self.segments {
            for p in [s.start_mm, s.end_mm] {
                for axis in 0..2 {
                    bounds[0][axis] = bounds[0][axis].min(p[axis]);
                    bounds[1][axis] = bounds[1][axis].max(p[axis]);
                }
            }
        }
        bounds
    }

    /// The enclosed area, in mm², whatever the winding.
    pub fn area_mm2(&self) -> f64 {
        let points: Vec<_> = self.segments.iter().map(|s| s.start_mm).collect();
        (signed_twice_area_xy(&points) / 2.).abs()
    }

    /// The part's extents when it is literally an axis-aligned rectangle —
    /// four axis-parallel Lines whose corners are two x and two y values —
    /// and `None` otherwise. Exactly the test the pre-§26I reader applied; it
    /// exists so the older wire blocks keep describing rectangles and never
    /// anything else.
    pub fn rectangle_mm(&self) -> Option<[[f64; 2]; 2]> {
        if self.segments.len() != 4 {
            return None;
        }
        let axis_aligned = self.segments.iter().all(|s| {
            (s.start_mm[0] - s.end_mm[0]).abs() <= CLOSURE_MM
                || (s.start_mm[1] - s.end_mm[1]).abs() <= CLOSURE_MM
        });
        if !axis_aligned {
            return None;
        }
        let corners: Vec<_> = self.segments.iter().map(|s| s.start_mm).collect();
        let min_x = corners.iter().map(|c| c[0]).fold(f64::INFINITY, f64::min);
        let max_x = corners
            .iter()
            .map(|c| c[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let min_y = corners.iter().map(|c| c[1]).fold(f64::INFINITY, f64::min);
        let max_y = corners
            .iter()
            .map(|c| c[1])
            .fold(f64::NEG_INFINITY, f64::max);
        let two = |axis: usize, low: f64, high: f64| {
            corners.iter().all(|c| {
                (c[axis] - low).abs() <= CLOSURE_MM || (c[axis] - high).abs() <= CLOSURE_MM
            })
        };
        (two(0, min_x, max_x) && two(1, min_y, max_y)).then_some([[min_x, min_y], [max_x, max_y]])
    }

    /// Where a disk sits: whether its centre is inside the polygon, and how
    /// far its rim is from the closest point of any **finite** segment,
    /// vertices included. Only relative vectors are used, so translation and
    /// winding change nothing.
    pub fn disk_clearance(&self, center_mm: [f64; 2], radius_mm: f64) -> DiskClearance {
        let mut inside = false;
        let mut nearest = (f64::INFINITY, self.segments[0].curve_id);
        for s in &self.segments {
            let (a, b) = (s.start_mm, s.end_mm);
            // Crossing test on a ray towards +x, half-open in y so a ray
            // through a vertex is counted once.
            if (a[1] > center_mm[1]) != (b[1] > center_mm[1]) {
                let t = (center_mm[1] - a[1]) / (b[1] - a[1]);
                let x = a[0] + t * (b[0] - a[0]);
                if center_mm[0] < x {
                    inside = !inside;
                }
            }
            let d = segment_distance(center_mm, a, b);
            if d < nearest.0 {
                nearest = (d, s.curve_id);
            }
        }
        DiskClearance {
            center_inside: inside,
            clearance_mm: nearest.0 - radius_mm,
            nearest: nearest.1,
        }
    }
}

/// Distance from `p` to the closed segment `a`–`b`.
fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (px, py) = (p[0] - a[0], p[1] - a[1]);
    let length2 = dx * dx + dy * dy;
    let t = if length2 > 0. {
        ((px * dx + py * dy) / length2).clamp(0., 1.)
    } else {
        0.
    };
    (px - t * dx).hypot(py - t * dy)
}

fn signed_twice_area(points: &[crate::Point2]) -> f64 {
    signed_twice_area_xy(&points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>())
}

/// Triangulated about the first vertex, so translating a small profile far
/// from the origin costs no precision to cancellation.
fn signed_twice_area_xy(points: &[[f64; 2]]) -> f64 {
    let o = points[0];
    (1..points.len() - 1)
        .map(|i| {
            let (a, b) = (points[i], points[i + 1]);
            (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
        })
        .sum()
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::Point2;

    fn boundary(points: &[[f64; 2]]) -> CutBoundary {
        let curves: Vec<_> = (0..points.len())
            .map(|i| SketchCurve {
                id: StableEntityId::new(),
                construction: false,
                geometry: SketchGeometry::Line {
                    start: Point2::new(points[i][0], points[i][1]).expect("point"),
                    end: Point2::new(
                        points[(i + 1) % points.len()][0],
                        points[(i + 1) % points.len()][1],
                    )
                    .expect("point"),
                },
            })
            .collect();
        CutBoundary::read(&curves, 10.).expect("a simple polygon")
    }

    const L: [[f64; 2]; 6] = [
        [0., 0.],
        [60., 0.],
        [60., 20.],
        [20., 20.],
        [20., 40.],
        [0., 40.],
    ];

    fn reversed(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
        points.iter().rev().copied().collect()
    }

    fn inside(b: &CutBoundary, c: [f64; 2], r: f64) -> bool {
        let d = b.disk_clearance(c, r);
        d.center_inside && d.clearance_mm > crate::WALL_CLEARANCE_MM
    }

    #[test]
    fn an_l_profile_is_its_real_boundary_and_not_its_bounding_box() {
        for points in [L.to_vec(), reversed(&L)] {
            let b = boundary(&points);
            assert_eq!(b.area_mm2(), 1600.);
            assert_eq!(b.bounds_mm(), [[0., 0.], [60., 40.]]);
            assert_eq!(b.rectangle_mm(), None, "an L is not a rectangle");
            // Inside the bounding box, outside the part.
            let notch = b.disk_clearance([40., 30.], 3.);
            assert!(!notch.center_inside);
            assert!(!inside(&b, [40., 30.], 3.));
            // One in each branch.
            assert!(inside(&b, [45., 10.], 5.));
            assert!(inside(&b, [10., 30.], 5.));
            // Centre inside, near the reflex vertex (20, 20). Both concave
            // edges' supporting lines are 3 mm away, but those lines run on
            // through the part; the finite edges are sqrt(18) mm away, at
            // the vertex itself.
            let c = [17., 17.];
            let near = b.disk_clearance(c, 4.2);
            assert!(near.center_inside);
            assert!((near.clearance_mm - (18f64.sqrt() - 4.2)).abs() < 1e-12);
            assert!(inside(&b, c, 4.2), "finite segments, not their lines");
            assert!(!inside(&b, c, 4.3), "a disk over the reflex vertex");
            // Centre inside, disk across the concave edge y = 20.
            assert!(!inside(&b, [40., 17.], 4.));
            // Exactly at the clearance is refused, a hair more is accepted.
            let edge = b.disk_clearance([40., 10.], 10.);
            assert_eq!(edge.clearance_mm, 0.);
            assert!(!inside(&b, [40., 10.], 10.));
            assert!(inside(&b, [40., 10.], 10. - 1e-3));
        }
        assert_eq!(
            boundary(&L).orientation(),
            BoundaryOrientation::CounterClockwise
        );
        assert_eq!(
            boundary(&reversed(&L)).orientation(),
            BoundaryOrientation::Clockwise
        );
    }

    #[test]
    fn the_rule_ignores_winding_and_translation() {
        let triangle = [[0., 0.], [50., 5.], [10., 40.]];
        let quad = [[0., 0.], [50., 10.], [45., 45.], [-5., 35.]];
        for shape in [&triangle[..], &quad[..], &L[..]] {
            for shift in [[0., 0.], [1000., -250.], [-9.5e5, 9.5e5]] {
                let moved: Vec<_> = shape
                    .iter()
                    .map(|p| [p[0] + shift[0], p[1] + shift[1]])
                    .collect();
                let there = boundary(&moved);
                let back = boundary(&reversed(&moved));
                let here = boundary(shape);
                assert!((there.area_mm2() - here.area_mm2()).abs() < 1e-6);
                for (c, r) in [([15., 12.], 3.), ([30., 10.], 2.), ([5., 30.], 4.)] {
                    let at = [c[0] + shift[0], c[1] + shift[1]];
                    assert_eq!(inside(&here, c, r), inside(&there, at, r), "{c:?}");
                    assert_eq!(inside(&there, at, r), inside(&back, at, r), "{c:?}");
                }
            }
        }
    }

    #[test]
    fn a_sloped_wall_is_measured_to_the_segment_not_its_bounding_box() {
        // Hypotenuse from (40, 0) to (0, 40): x + y = 40.
        let b = boundary(&[[0., 0.], [40., 0.], [0., 40.]]);
        let c = [15., 15.];
        let to_wall = (40. - 30.) / 2f64.sqrt();
        let d = b.disk_clearance(c, 5.);
        assert!((d.clearance_mm - (to_wall - 5.)).abs() < 1e-12);
        assert_eq!(d.nearest, b.segments()[1].curve_id);
        // Inside the bounding box at every rim point, but over the sloped wall.
        assert!(!inside(&b, c, 7.1));
        assert!(inside(&b, c, 7.));
    }

    #[test]
    fn a_rectangle_is_still_exactly_a_rectangle() {
        let b = boundary(&[[0., 0.], [60., 0.], [60., 40.], [0., 40.]]);
        assert_eq!(b.rectangle_mm(), Some([[0., 0.], [60., 40.]]));
        let skew = boundary(&[[0., 0.], [60., 5.], [60., 40.], [0., 40.]]);
        assert_eq!(skew.rectangle_mm(), None);
    }

    #[test]
    fn anything_outside_the_shared_policy_is_refused() {
        let line = |a: [f64; 2], b: [f64; 2]| SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(a[0], a[1]).expect("point"),
                end: Point2::new(b[0], b[1]).expect("point"),
            },
        };
        // A bow tie: closed, but self-intersecting.
        let bow = [[0., 0.], [10., 10.], [10., 0.], [0., 10.]];
        let curves: Vec<_> = (0..4).map(|i| line(bow[i], bow[(i + 1) % 4])).collect();
        assert!(CutBoundary::read(&curves, 10.).is_err());
        // Open.
        let open = vec![
            line([0., 0.], [10., 0.]),
            line([10., 0.], [10., 10.]),
            line([10., 10.], [0., 9.]),
        ];
        assert!(CutBoundary::read(&open, 10.).is_err());
        // Two Lines.
        assert!(CutBoundary::read(&open[..2], 10.).is_err());
        // A circle.
        let circle = SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Circle {
                center: Point2::ORIGIN,
                radius: 5.,
            },
        };
        assert!(CutBoundary::read(&[circle.clone(), circle.clone(), circle], 10.).is_err());
    }
}
