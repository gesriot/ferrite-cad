// SPDX-License-Identifier: MIT
//! Bounded, unconstrained XY Line polygon creation input. No persistence IDs
//! exist in a draft; the writer allocates those once when building the model.
use ferritecad_document::Point2;
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
        if points.len() < 3 || points.len() > Self::MAX_POINTS {
            return Err(CadError::input("polygon needs 3..256 distinct vertices"));
        }
        let points = points
            .into_iter()
            .map(|[x, y]| Point2::new(x, y))
            .collect::<Result<Vec<_>>>()?;
        // A bounded first editor. Besides making O(n²) checks cheap, this range
        // keeps orientation arithmetic away from overflow; it is not a kernel limit.
        if height > 1e6 || points.iter().any(|p| p.x.abs() > 1e6 || p.y.abs() > 1e6) {
            return Err(CadError::input(
                "polygon coordinates and height must fit within 1000000 mm",
            ));
        }
        let eps = Self::TOLERANCE_MM;
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
        Ok(Self { points, height })
    }
    pub fn points(&self) -> &[Point2] {
        &self.points
    }
    pub fn height_mm(&self) -> f64 {
        self.height
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
}
