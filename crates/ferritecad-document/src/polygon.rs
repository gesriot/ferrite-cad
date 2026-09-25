// SPDX-License-Identifier: MIT
//! Bounded, unconstrained XY creation input: a Line polygon or one analytic
//! circle. No persistence IDs exist in a draft; the writer allocates those once
//! when building the model.
use crate::Point2;
use ferritecad_kernel::ExtrudeExtent;
use ferritecad_types::{CadError, Result};

/// Shared UI/CLI validity policy, in mm. Closure is implicit last -> first;
/// callers must not duplicate the first vertex. Both windings are preserved.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonExtrusion {
    points: Vec<Point2>,
    height: f64,
}

impl PolygonExtrusion {
    pub const MAX_POINTS: usize = 256;
    pub const TOLERANCE_MM: f64 = 1e-6;

    pub fn new(points: Vec<[f64; 2]>, height: f64) -> Result<Self> {
        let height = ExtrudeExtent::blind(height)?.total_length();
        let points = simple_line_polygon(points, Some(height))?;
        Ok(Self { points, height })
    }
    pub fn points(&self) -> &[Point2] {
        &self.points
    }
    pub fn height_mm(&self) -> f64 {
        self.height
    }
}
/// The one simplicity policy for an unconstrained closed Line polygon, in mm.
///
/// Shared by every creation that takes such a profile, so an extrusion and a
/// revolution accept and refuse exactly the same drawings for exactly the same
/// reasons, in the same order. `height` is the extrusion's validated height,
/// which shares the coordinate bound; a revolution has none.
pub(crate) fn simple_line_polygon(
    points: Vec<[f64; 2]>,
    height: Option<f64>,
) -> Result<Vec<Point2>> {
    if points.len() < 3 || points.len() > PolygonExtrusion::MAX_POINTS {
        return Err(CadError::input("polygon needs 3..256 distinct vertices"));
    }
    let points = points
        .into_iter()
        .map(|[x, y]| Point2::new(x, y))
        .collect::<Result<Vec<_>>>()?;
    // A bounded first editor. Besides making O(n²) checks cheap, this range
    // keeps orientation arithmetic away from overflow; it is not a kernel limit.
    if height.is_some_and(|h| h > 1e6) || points.iter().any(|p| p.x.abs() > 1e6 || p.y.abs() > 1e6)
    {
        return Err(CadError::input(match height {
            Some(_) => "polygon coordinates and height must fit within 1000000 mm",
            None => "polygon coordinates must fit within 1000000 mm",
        }));
    }
    let eps = PolygonExtrusion::TOLERANCE_MM;
    let n = points.len();
    for i in 0..n {
        for j in i + 1..n {
            if distance(points[i], points[j]) <= eps {
                return Err(CadError::input(
                    "polygon has repeated vertices or a zero-length edge; do not repeat the closing vertex",
                ));
            }
        }
        let (a, b, c) = (points[i], points[(i + 1) % n], points[(i + 2) % n]);
        if cross(a, b, c).abs() <= eps * distance(a, b).max(distance(b, c)) {
            return Err(CadError::input(
                "polygon has a collinear or backtracking vertex within 0.000001 mm",
            ));
        }
    }
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let (a, b, c, d) = (
                points[i],
                points[(i + 1) % n],
                points[j],
                points[(j + 1) % n],
            );
            if intersects(a, b, c, d, eps) {
                return Err(CadError::input(
                    "polygon edges intersect or touch within 0.000001 mm",
                ));
            }
        }
    }
    // Triangulated signed area about the first vertex avoids cancellation
    // caused solely by translating a small profile far from the origin.
    let twice_area: f64 = (1..n - 1)
        .map(|i| cross(points[0], points[i], points[i + 1]))
        .sum();
    let perimeter: f64 = (0..n)
        .map(|i| distance(points[i], points[(i + 1) % n]))
        .sum();
    if twice_area.abs() <= eps * perimeter {
        return Err(CadError::input("polygon has zero or tolerance-sized area"));
    }
    Ok(points)
}

/// Which side of the axis a full-turn profile keeps, decided once by
/// [`FullTurnRevolution::new`] from the coordinates themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevolutionClosure {
    /// Every vertex strictly off the axis: a part with a bore (§27A).
    RadialClear,
    /// Exactly one whole Line on the axis, from `points[axis_line]` to the
    /// next vertex: a solid part (§27C). That Line turns into nothing; every
    /// other Line turns into a face.
    AxisClosed { axis_line: usize },
}

/// Shared UI/CLI validity policy for one full-turn revolution, in mm.
///
/// One unconstrained simple closed Line polygon on the sketch's XY plane,
/// turned once — exactly 2π — about the sketch's local Y axis through its
/// origin. On the canvas X is the radial distance and Y the axial coordinate.
/// The polygon rules are [`PolygonExtrusion`]'s own; what is added is how the
/// profile meets the axis, which is one of two named classes
/// ([`RevolutionClosure`]):
///
/// * every vertex strictly on the positive radial side, by more than
///   [`Self::AXIS_CLEARANCE_MM`] — a part with a bore; or
/// * exactly two vertices exactly on the axis (`x == 0`), which are the two
///   ends of one Line, and every other vertex beyond the clearance — a solid
///   part closed on the axis along that Line.
///
/// "On the axis" is exact. IEEE −0 is the same number and is stored as +0; no
/// small positive coordinate is ever taken for 0. A vertex inside the
/// clearance but not on the axis, a vertex on the negative side, an isolated
/// touch, two separate touches and several axis intervals are refused. Partial
/// angles and other axes are later slices.
#[derive(Debug, Clone, PartialEq)]
pub struct FullTurnRevolution {
    points: Vec<Point2>,
    closure: RevolutionClosure,
}

impl FullTurnRevolution {
    /// How far every vertex must stay from the axis, in mm. The polygon
    /// policy's own tolerance: a vertex nearer than this is on the axis.
    pub const AXIS_CLEARANCE_MM: f64 = PolygonExtrusion::TOLERANCE_MM;

    pub fn new(points: Vec<[f64; 2]>) -> Result<Self> {
        // −0 and +0 are one number; store the one a reader expects.
        let points = points
            .into_iter()
            .map(|p| p.map(|v| if v == 0.0 { 0.0 } else { v }))
            .collect();
        let points = simple_line_polygon(points, None)?;
        let n = points.len();
        let mut on_axis = Vec::new();
        for (index, p) in points.iter().enumerate() {
            if p.x == 0.0 {
                on_axis.push(index);
            } else if p.x < 0.0 {
                return Err(CadError::input(format!(
                    "vertex {} at ({}, {}) mm is not on the positive radial side: the profile \
                     would cross the sketch Y axis, and a full-turn revolution needs every x >= 0",
                    index + 1,
                    p.x,
                    p.y
                )));
            } else if p.x <= Self::AXIS_CLEARANCE_MM {
                return Err(CadError::input(format!(
                    "vertex {} at ({}, {}) mm is not strictly on the positive radial side and not \
                     on the axis: a vertex needs x > {} mm, or x = 0 exactly as one end of the one \
                     Line a solid part closes on",
                    index + 1,
                    p.x,
                    p.y,
                    Self::AXIS_CLEARANCE_MM
                )));
            }
        }
        let closure = match on_axis.as_slice() {
            [] => RevolutionClosure::RadialClear,
            [a, b] if b - a == 1 => RevolutionClosure::AxisClosed { axis_line: *a },
            [0, b] if *b == n - 1 => RevolutionClosure::AxisClosed { axis_line: n - 1 },
            [only] => {
                return Err(CadError::input(format!(
                    "vertex {} touches the axis alone: a solid part closes on the sketch Y axis \
                     along one whole Line, whose two ends both have x = 0",
                    only + 1
                )));
            }
            [a, b] => {
                return Err(CadError::input(format!(
                    "vertices {} and {} touch the axis but are not the ends of one Line: a solid \
                     part closes on the sketch Y axis along exactly one whole Line",
                    a + 1,
                    b + 1
                )));
            }
            many => {
                return Err(CadError::input(format!(
                    "{} vertices lie on the axis: a solid part closes on the sketch Y axis along \
                     exactly one Line, never along several",
                    many.len()
                )));
            }
        };
        Ok(Self { points, closure })
    }
    /// How this profile meets the axis.
    pub fn closure(&self) -> RevolutionClosure {
        self.closure
    }
    /// The index of the Line on the axis, for a solid part.
    pub fn axis_line(&self) -> Option<usize> {
        match self.closure {
            RevolutionClosure::AxisClosed { axis_line } => Some(axis_line),
            RevolutionClosure::RadialClear => None,
        }
    }
    pub fn points(&self) -> &[Point2] {
        &self.points
    }
    /// Pappus: the swept volume, 2π × area × centroid radius, in mm³.
    pub fn volume_mm3(&self) -> f64 {
        let p = &self.points;
        let n = p.len();
        // ∮ x² dy / 2 = ∫∫ x dA, independent of translation along Y.
        let moment: f64 = (0..n)
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % n]);
                (b.y - a.y) * (a.x * a.x + a.x * b.x + b.x * b.x) / 6.0
            })
            .sum();
        2.0 * std::f64::consts::PI * moment.abs()
    }
}

fn distance(a: Point2, b: Point2) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}
fn cross(a: Point2, b: Point2, c: Point2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}
fn intersects(a: Point2, b: Point2, c: Point2, d: Point2, eps: f64) -> bool {
    let side = |p, q, r| {
        let v = cross(p, q, r);
        let tolerance = eps * distance(p, q);
        if v.abs() <= tolerance {
            0
        } else if v > 0.0 {
            1
        } else {
            -1
        }
    };
    let within = |p: Point2, q: Point2, r: Point2| {
        r.x >= p.x.min(q.x) - eps
            && r.x <= p.x.max(q.x) + eps
            && r.y >= p.y.min(q.y) - eps
            && r.y <= p.y.max(q.y) + eps
    };
    let (abc, abd, cda, cdb) = (side(a, b, c), side(a, b, d), side(c, d, a), side(c, d, b));
    (abc * abd < 0 && cda * cdb < 0)
        || (abc == 0 && within(a, b, c))
        || (abd == 0 && within(a, b, d))
        || (cda == 0 && within(c, d, a))
        || (cdb == 0 && within(c, d, b))
}

/// Shared UI/CLI validity policy for one analytic circle, in mm.
///
/// A circle is not a polygon with many sides: it is a centre and a radius, and
/// it stays that in the document and in the B-Rep. Nothing here approximates
/// it, so there is no vertex count to bound and no self-intersection to check —
/// what is left is that every number is one a document will store.
#[derive(Debug, Clone, PartialEq)]
pub struct CircleExtrusion {
    center: Point2,
    radius: f64,
    height: f64,
}

impl CircleExtrusion {
    /// The same bound the polygon editor uses, and for the same reason: a
    /// first editor with a stated range, not a kernel limit.
    pub const MAX_MM: f64 = 1e6;

    pub fn new(center: [f64; 2], radius: f64, height: f64) -> Result<Self> {
        let height = ExtrudeExtent::blind(height)?.total_length();
        let center = Point2::new(center[0], center[1])?;
        if !radius.is_finite() || radius <= 0. {
            return Err(CadError::input(
                "circle radius must be finite and positive in mm",
            ));
        }
        if height > Self::MAX_MM
            || radius > Self::MAX_MM
            || center.x.abs() > Self::MAX_MM
            || center.y.abs() > Self::MAX_MM
        {
            return Err(CadError::input(
                "circle centre, radius and height must fit within 1000000 mm",
            ));
        }
        Ok(Self {
            center,
            radius,
            height,
        })
    }
    pub fn center(&self) -> Point2 {
        self.center
    }
    pub fn radius_mm(&self) -> f64 {
        self.radius
    }
    pub fn height_mm(&self) -> f64 {
        self.height
    }
}

/// Shared UI/CLI validity policy for one circular hole in one circular
/// boundary, in mm.
///
/// Two analytic circles about one centre, and nothing else: both stay a centre
/// and a radius in the document and in the B-Rep, so there is still no vertex
/// count to bound and no self-intersection to check. What is added over
/// [`CircleExtrusion`] is the two facts that make a hole a hole — the circles
/// share a centre, and the wall between them is thick enough to be a wall.
///
/// # Why the wall has a minimum
///
/// This slice uses a conservative 0.001 mm wall floor, four orders of magnitude
/// above the kernel's 1e-7 mm linear tolerance. Thin walls can still build, so
/// successful construction alone is not an accuracy guarantee. Comparisons of
/// thin-wall volumes must use the stable `pi * (R-r) * (R+r) * h` reference;
/// subtracting the squared radii loses precision in the reference itself.
/// The floor is a product policy, not a universal error bound for OCCT.
///
/// Stored centres must be within the kernel's linear tolerance in Euclidean
/// distance. Their coordinates are preserved rather than snapped together.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnularExtrusion {
    center: Point2,
    outer_radius: f64,
    inner_radius: f64,
    height: f64,
}

impl AnnularExtrusion {
    /// The same bound as the polygon and circle editors, for the same reason.
    pub const MAX_MM: f64 = CircleExtrusion::MAX_MM;
    /// Thinnest wall this slice will publish, in mm. See the type's own note.
    pub const MIN_WALL_MM: f64 = 1e-3;
    /// How close two centres must be to be one centre, in mm.
    pub const CONCENTRIC_MM: f64 = ferritecad_types::Tolerance::DEFAULT_LINEAR;

    pub fn new(
        center: [f64; 2],
        outer_radius: f64,
        inner_radius: f64,
        height: f64,
    ) -> Result<Self> {
        // The boundary is checked by the policy that already exists for one
        // circle, so a boundary this accepts is one `CircleExtrusion` would
        // accept too and the two cannot drift apart on centres, radii,
        // heights, finiteness or the 1e6 bound.
        let outer = CircleExtrusion::new(center, outer_radius, height)?;
        if !inner_radius.is_finite() || inner_radius <= 0. {
            return Err(CadError::input(
                "inner radius must be finite and positive in mm",
            ));
        }
        if inner_radius >= outer.radius_mm() {
            return Err(CadError::input(
                "inner radius must be smaller than the outer radius; a hole is inside its boundary",
            ));
        }
        // Add at the radius scale so decimal boundary inputs such as 10 and
        // 9.999 are not rejected by cancellation in their difference. Rounding
        // here is at most one radius ULP (far below the kernel tolerance).
        if outer.radius_mm() < inner_radius + Self::MIN_WALL_MM {
            return Err(CadError::input(format!(
                "the wall between the two radii is {} mm, and this slice publishes at least {} mm",
                outer.radius_mm() - inner_radius,
                Self::MIN_WALL_MM
            )));
        }
        Ok(Self {
            center: outer.center(),
            outer_radius: outer.radius_mm(),
            inner_radius,
            height: outer.height_mm(),
        })
    }

    /// Whether two stored centres are the one centre this policy requires.
    ///
    /// Asked of a saved document rather than of a request, which is why it is
    /// here and not folded into [`Self::new`]: a request names one centre and
    /// cannot disagree with itself.
    pub fn concentric(a: Point2, b: Point2) -> bool {
        (a.x - b.x).hypot(a.y - b.y) <= Self::CONCENTRIC_MM
    }

    pub fn center(&self) -> Point2 {
        self.center
    }
    pub fn outer_radius_mm(&self) -> f64 {
        self.outer_radius
    }
    pub fn inner_radius_mm(&self) -> f64 {
        self.inner_radius
    }
    pub fn height_mm(&self) -> f64 {
        self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_polygon_policy_accepts_concavity_and_both_windings() {
        let mut l = vec![
            [0., 0.],
            [60., 0.],
            [60., 20.],
            [20., 20.],
            [20., 40.],
            [0., 40.],
        ];
        assert!(PolygonExtrusion::new(l.clone(), 10.).is_ok());
        l.reverse();
        assert!(PolygonExtrusion::new(l, 10.).is_ok());
        for p in [
            vec![[0., 0.], [1., 0.], [0., 0.]],
            vec![[0., 0.], [1., 0.], [2., 0.]],
            vec![[0., 0.], [2., 2.], [0., 2.], [2., 0.]],
            vec![[0., 0.], [2., 0.], [2., 2.], [1., 0.], [0., 2.]],
            vec![[0., 0.], [f64::NAN, 0.], [0., 2.]],
        ] {
            assert!(PolygonExtrusion::new(p, 10.).is_err());
        }
        for h in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(PolygonExtrusion::new(vec![[0., 0.], [2., 0.], [0., 2.]], h).is_err());
        }
    }

    #[test]
    fn circle_policy_keeps_a_centre_and_a_positive_radius() {
        let ok = CircleExtrusion::new([12., -7.], 10., 15.).expect("a circle");
        assert_eq!((ok.center().x, ok.center().y), (12., -7.));
        assert_eq!((ok.radius_mm(), ok.height_mm()), (10., 15.));
        // Zero is a centre like any other, and a fractional radius is not a
        // special case; only the radius and height have to be positive.
        let small = CircleExtrusion::new([0., 0.], 0.25, 0.5).expect("small but real");
        assert_eq!((small.radius_mm(), small.height_mm()), (0.25, 0.5));
        for (center, radius, height) in [
            ([0., 0.], 0., 10.),
            ([0., 0.], -1., 10.),
            ([0., 0.], f64::NAN, 10.),
            ([0., 0.], f64::INFINITY, 10.),
            ([f64::NAN, 0.], 5., 10.),
            ([0., f64::INFINITY], 5., 10.),
            ([0., 0.], 5., 0.),
            ([0., 0.], 5., -1.),
            ([0., 0.], 5., f64::NAN),
            ([0., 0.], 5., f64::INFINITY),
            ([2e6, 0.], 5., 10.),
            ([0., 0.], 2e6, 10.),
            ([0., 0.], 5., 2e6),
        ] {
            assert!(
                CircleExtrusion::new(center, radius, height).is_err(),
                "{center:?} r{radius} h{height}"
            );
        }
    }

    #[test]
    fn annular_policy_wants_one_centre_and_a_wall_thick_enough_to_be_one() {
        let ok = AnnularExtrusion::new([12., -7.], 10., 4., 15.).expect("an annulus");
        assert_eq!((ok.center().x, ok.center().y), (12., -7.));
        assert_eq!(ok.outer_radius_mm(), 10.);
        assert_eq!(ok.inner_radius_mm(), 4.);
        assert_eq!(ok.height_mm(), 15.);
        // Fractional radii are not a special case, and the wall may be exactly
        // the minimum.
        let thin = AnnularExtrusion::new([-3.5, 4.25], 6.75, 6.749, 2.5).expect("a thin wall");
        AnnularExtrusion::new([0., 0.], 10., 9.999, 15.)
            .expect("decimal minimum wall must not depend on subtraction rounding");
        assert!(AnnularExtrusion::new([0., 0.], 10., 9.99900001, 15.).is_err());
        assert!(
            thin.outer_radius_mm() - thin.inner_radius_mm() >= AnnularExtrusion::MIN_WALL_MM,
            "a wall at the minimum is accepted, not rounded through it"
        );
        for (outer, inner) in [(10., 10.), (10., 12.), (10., 9.9995)] {
            assert!(
                AnnularExtrusion::new([0., 0.], outer, inner, 5.).is_err(),
                "R{outer} r{inner}"
            );
        }
        for inner in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(
                AnnularExtrusion::new([0., 0.], 10., inner, 5.).is_err(),
                "r{inner}"
            );
        }
        // Everything the boundary policy refuses, this refuses too: it is the
        // same policy, asked once.
        for (center, outer, height) in [
            ([0., 0.], 0., 10.),
            ([0., 0.], f64::NAN, 10.),
            ([f64::NAN, 0.], 10., 10.),
            ([0., 0.], 10., 0.),
            ([0., 0.], 10., f64::INFINITY),
            ([2e6, 0.], 10., 10.),
            ([0., 0.], 2e6, 10.),
            ([0., 0.], 10., 2e6),
        ] {
            assert!(
                AnnularExtrusion::new(center, outer, 1., height).is_err(),
                "{center:?} R{outer} h{height}"
            );
        }
    }

    #[test]
    fn concentric_is_the_kernels_own_idea_of_one_point() {
        let at = |x, y| Point2::new(x, y).expect("finite");
        assert!(AnnularExtrusion::concentric(at(12., -7.), at(12., -7.)));
        // Inside the kernel's confusion the two centres are one point.
        let nudge = AnnularExtrusion::CONCENTRIC_MM / 2.;
        let diagonal = AnnularExtrusion::CONCENTRIC_MM * 0.9;
        assert!(!AnnularExtrusion::concentric(
            at(0., 0.),
            at(diagonal, diagonal)
        ));
        assert!(AnnularExtrusion::concentric(
            at(12., -7.),
            at(12. + nudge, -7. - nudge)
        ));
        // Ten times it is two points, and a millimetre plainly is.
        let apart = AnnularExtrusion::CONCENTRIC_MM * 10.;
        assert!(!AnnularExtrusion::concentric(
            at(12., -7.),
            at(12. + apart, -7.)
        ));
        assert!(!AnnularExtrusion::concentric(at(12., -7.), at(12., -6.)));
        assert_eq!(
            AnnularExtrusion::CONCENTRIC_MM,
            ferritecad_types::Tolerance::DEFAULT_LINEAR,
            "concentricity is the kernel's point tolerance, not a second opinion"
        );
    }

    #[test]
    fn a_full_turn_is_pappus_in_either_winding_and_any_axial_shift() {
        use std::f64::consts::PI;
        for (points, volume) in [
            (
                vec![[4., 0.], [10., 0.], [10., 15.], [4., 15.]],
                PI * 84. * 15.,
            ),
            (
                vec![
                    [4., 0.],
                    [10., 0.],
                    [10., 5.],
                    [7., 5.],
                    [7., 15.],
                    [4., 15.],
                ],
                750. * PI,
            ),
            (vec![[4., 0.], [10., 0.], [7., 15.], [4., 15.]], 855. * PI),
        ] {
            let shifted: Vec<_> = points.iter().map(|[x, y]| [*x, y - 3.375]).collect();
            let mut reversed = shifted.clone();
            reversed.reverse();
            for p in [points.clone(), shifted, reversed] {
                let turn = FullTurnRevolution::new(p.clone()).expect("a turn");
                assert_eq!(turn.points().len(), p.len());
                assert!((turn.volume_mm3() - volume).abs() < 1e-9 * volume, "{p:?}");
            }
        }
    }

    #[test]
    fn a_full_turn_refuses_the_axis_and_whatever_the_polygon_refuses() {
        let clear = FullTurnRevolution::AXIS_CLEARANCE_MM;
        for p in [
            vec![[0., 0.], [10., 0.], [10., 15.], [0., 15.]],
            vec![[-2., 0.], [10., 0.], [10., 15.], [-2., 15.]],
            vec![[clear, 0.], [10., 0.], [10., 15.]],
            vec![[-4., 0.], [-10., 0.], [-10., 15.], [-4., 15.]],
        ] {
            let error = FullTurnRevolution::new(p.clone()).expect_err("axis");
            assert!(
                error
                    .to_string()
                    .contains("not strictly on the positive radial side"),
                "{p:?}: {error}"
            );
        }
        assert!(FullTurnRevolution::new(vec![[clear * 2., 0.], [10., 0.], [10., 15.]]).is_ok());
        for p in [
            vec![[4., 0.], [10., 0.], [4., 0.]],
            vec![[4., 0.], [5., 0.], [6., 0.]],
            vec![[4., 0.], [6., 2.], [4., 2.], [6., 0.]],
            vec![[4., 0.], [f64::NAN, 0.], [4., 2.]],
            vec![[4., 0.], [2e6, 0.], [4., 2.]],
            vec![[4., 0.], [10., 0.]],
        ] {
            assert!(FullTurnRevolution::new(p.clone()).is_err(), "{p:?}");
        }
        let many: Vec<_> = (0..257)
            .map(|i| {
                let a = f64::from(i) * std::f64::consts::TAU / 257.;
                [20. + 5. * a.cos(), 5. * a.sin()]
            })
            .collect();
        assert!(FullTurnRevolution::new(many).is_err());
    }
}
