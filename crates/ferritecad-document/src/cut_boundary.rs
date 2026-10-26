// SPDX-License-Identifier: MIT
//! Saved polygon dimensions used by the existing creation and edge-edit forms.
use ferritecad_types::{CadError, Result, StableEntityId};

use crate::{PolygonExtrusion, SketchCurve, SketchGeometry};

/// How far one stored Line end may be from the next Line's start and still
/// close the profile. The kernel's linear tolerance, as the rectangle reader
/// that preceded this one used.
const CLOSURE_MM: f64 = ferritecad_types::Tolerance::DEFAULT_LINEAR;

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

    #[test]
    fn polygon_dimensions_ignore_winding_and_translation() {
        let here = boundary(&L);
        assert_eq!(here.area_mm2(), 1600.);
        assert_eq!(here.bounds_mm(), [[0., 0.], [60., 40.]]);
        assert_eq!(here.rectangle_mm(), None);
        assert_eq!(here.orientation(), BoundaryOrientation::CounterClockwise);
        assert_eq!(
            boundary(&reversed(&L)).orientation(),
            BoundaryOrientation::Clockwise
        );
        for shift in [[0., 0.], [1000., -250.], [-9.5e5, 9.5e5]] {
            let moved: Vec<_> = L
                .iter()
                .map(|p| [p[0] + shift[0], p[1] + shift[1]])
                .collect();
            assert!((boundary(&moved).area_mm2() - here.area_mm2()).abs() < 1e-6);
        }
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
