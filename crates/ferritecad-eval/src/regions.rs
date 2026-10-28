// SPDX-License-Identifier: MIT
//! Analytic intersections and ray parity for closed Line/Arc/Circle boundaries.
use std::f64::consts::{FRAC_PI_2, TAU};

use ferritecad_kernel::{
    PlanarPoint as P, Profile, ProfileLoop, ProfileRegion, SegmentGeometry as G, SketchPlane,
};
use ferritecad_types::{CadError, Result};

use crate::convert::JOIN_TOLERANCE;

pub(crate) fn profile(plane: SketchPlane, loops: Vec<ProfileLoop>) -> Result<Profile> {
    for (i, a) in loops.iter().enumerate() {
        for (j, b) in loops.iter().enumerate().skip(i) {
            for (x, first) in a.segments().iter().enumerate() {
                for (y, second) in b.segments().iter().enumerate() {
                    if i == j && x >= y {
                        continue;
                    }
                    let (points, overlap) = intersections(first.geometry, second.geometry);
                    let adjacent =
                        i == j && (y == x + 1 || (x == 0 && y + 1 == a.segments().len()));
                    let allowed = |point| {
                        adjacent
                            && [first.geometry.start(), first.geometry.end()]
                                .into_iter()
                                .flatten()
                                .any(|p| close(p, point))
                            && [second.geometry.start(), second.geometry.end()]
                                .into_iter()
                                .flatten()
                                .any(|p| close(p, point))
                    };
                    if overlap || points.into_iter().any(|p| !allowed(p)) {
                        return Err(CadError::input(format!(
                            "profile curves {} and {} intersect or touch; \
                             check for self-intersections",
                            first.label, second.label
                        )));
                    }
                }
            }
        }
    }
    // With simple, mutually disjoint boundaries, one boundary point determines
    // inclusion of the entire loop. Ray intersections below are analytic, not samples.
    let contained: Vec<Vec<bool>> = loops
        .iter()
        .map(|a| {
            let p = boundary_point(a.segments()[0].geometry);
            loops.iter().map(|b| contains(b, p)).collect()
        })
        .collect();
    let depth: Vec<_> = (0..loops.len())
        .map(|i| {
            (0..loops.len())
                .filter(|j| i != *j && contained[i][*j])
                .count()
        })
        .collect();
    let mut regions = Vec::new();
    for (i, outer) in loops.iter().enumerate() {
        if depth[i] % 2 != 0 {
            continue;
        }
        let inner = loops
            .iter()
            .enumerate()
            .filter(|(j, _)| depth[*j] == depth[i] + 1 && contained[*j][i])
            .map(|(_, l)| l.clone())
            .collect();
        regions.push(ProfileRegion {
            outer: outer.clone(),
            inner,
        });
    }
    Profile::from_regions(plane, regions)
}

fn close(a: P, b: P) -> bool {
    (a.x - b.x).abs() <= JOIN_TOLERANCE && (a.y - b.y).abs() <= JOIN_TOLERANCE
}
fn sub(a: P, b: P) -> P {
    P {
        x: a.x - b.x,
        y: a.y - b.y,
    }
}
fn cross(a: P, b: P) -> f64 {
    a.x * b.y - a.y * b.x
}
fn dot(a: P, b: P) -> f64 {
    a.x * b.x + a.y * b.y
}
fn along(a: P, d: P, t: f64) -> P {
    P {
        x: a.x + d.x * t,
        y: a.y + d.y * t,
    }
}
fn circle(g: G) -> (P, f64) {
    match g {
        G::Circle { center, radius } | G::Arc { center, radius, .. } => (center, radius),
        _ => unreachable!("only circular geometry reaches this helper"),
    }
}
fn at(center: P, radius: f64, angle: f64) -> P {
    P {
        x: center.x + radius * angle.cos(),
        y: center.y + radius * angle.sin(),
    }
}
fn boundary_point(g: G) -> P {
    match g {
        G::Line { start, .. } => start,
        G::Arc {
            center,
            radius,
            start_angle,
            ..
        } => at(center, radius, start_angle),
        G::Circle { center, radius } => at(center, radius, 0.),
        _ => unreachable!("conversion admits only Line, Arc and Circle"),
    }
}
fn intervals(g: G) -> Vec<(f64, f64)> {
    match g {
        G::Circle { .. } => vec![(0., TAU)],
        G::Arc {
            start_angle,
            end_angle,
            ..
        } => {
            let length = (end_angle - start_angle).abs();
            let start = start_angle.min(end_angle).rem_euclid(TAU);
            if start + length <= TAU {
                vec![(start, start + length)]
            } else {
                vec![(start, TAU), (0., start + length - TAU)]
            }
        }
        _ => unreachable!("circular geometry"),
    }
}
fn on(g: G, p: P) -> bool {
    match g {
        G::Line { start, end } => {
            let d = sub(end, start);
            let t = dot(sub(p, start), d) / dot(d, d);
            (0. ..=1.).contains(&t) || close(p, start) || close(p, end)
        }
        G::Circle { .. } => true,
        G::Arc { center, .. } => {
            let a = (p.y - center.y).atan2(p.x - center.x).rem_euclid(TAU);
            intervals(g).iter().any(|(lo, hi)| a >= *lo && a <= *hi)
                || g.start().is_ok_and(|s| close(p, s))
                || g.end().is_ok_and(|e| close(p, e))
        }
        _ => false,
    }
}

fn intersections(a: G, b: G) -> (Vec<P>, bool) {
    let mut points = Vec::new();
    if let G::Line { start: p, end: p1 } = a {
        let d = sub(p1, p);
        if let G::Line { start: q, end: q1 } = b {
            let e = sub(q1, q);
            let denominator = cross(d, e);
            if denominator != 0. {
                let point = along(p, d, cross(sub(q, p), e) / denominator);
                if on(a, point) && on(b, point) {
                    points.push(point);
                }
            } else if cross(sub(q, p), d) == 0. {
                for point in [p, p1, q, q1] {
                    if on(a, point) && on(b, point) && !points.iter().any(|old| close(*old, point))
                    {
                        points.push(point);
                    }
                }
                if points.len() > 1 {
                    return (points, true);
                }
            }
        } else {
            let (center, radius) = circle(b);
            let offset = sub(p, center);
            let dd = dot(d, d);
            let t = -dot(offset, d) / dd;
            let nearest = along(p, d, t);
            let h2 = radius * radius - dot(sub(nearest, center), sub(nearest, center));
            if h2 >= 0. {
                for v in [t - (h2 / dd).sqrt(), t + (h2 / dd).sqrt()] {
                    let point = along(p, d, v);
                    if on(a, point) && on(b, point) {
                        points.push(point);
                    }
                }
            }
        }
    } else if matches!(b, G::Line { .. }) {
        return intersections(b, a);
    } else {
        let (ca, ra) = circle(a);
        let (cb, rb) = circle(b);
        let delta = sub(cb, ca);
        let d2 = dot(delta, delta);
        if d2 == 0. && ra == rb {
            for (lo, hi) in intervals(a) {
                for (other_lo, other_hi) in intervals(b) {
                    let low = lo.max(other_lo);
                    let high = hi.min(other_hi);
                    if low < high {
                        return (points, true);
                    }
                    if low == high {
                        points.push(at(ca, ra, low));
                    }
                }
            }
            // Zero and tau are the same endpoint of the supporting circle.
            for p in [a.start(), a.end(), b.start(), b.end()]
                .into_iter()
                .flatten()
            {
                if on(a, p) && on(b, p) {
                    points.push(p);
                }
            }
        } else if d2 > 0. {
            let d = d2.sqrt();
            let x = (ra * ra - rb * rb + d2) / (2. * d);
            let h2 = ra * ra - x * x;
            if h2 >= 0. {
                let base = along(ca, delta, x / d);
                for sign in [-1., 1.] {
                    let p = P {
                        x: base.x - sign * delta.y * h2.sqrt() / d,
                        y: base.y + sign * delta.x * h2.sqrt() / d,
                    };
                    if on(a, p) && on(b, p) {
                        points.push(p);
                    }
                }
            }
        }
    }
    (points, false)
}

fn contains(boundary: &ProfileLoop, point: P) -> bool {
    let mut crossings = 0;
    for segment in boundary.segments() {
        match segment.geometry {
            G::Line { start, end } => {
                if (start.y > point.y) != (end.y > point.y) {
                    let x = start.x + (point.y - start.y) * (end.x - start.x) / (end.y - start.y);
                    crossings += usize::from(x > point.x);
                }
            }
            g @ (G::Circle { .. } | G::Arc { .. }) => {
                let (center, radius) = circle(g);
                for (lo, hi) in intervals(g) {
                    let mut breaks = vec![lo];
                    breaks.extend(
                        [FRAC_PI_2, 3. * FRAC_PI_2]
                            .into_iter()
                            .filter(|x| *x > lo && *x < hi),
                    );
                    breaks.push(hi);
                    for pair in breaks.windows(2) {
                        let start = at(center, radius, pair[0]);
                        let end = at(center, radius, pair[1]);
                        if (start.y > point.y) == (end.y > point.y) {
                            continue;
                        }
                        let dy = point.y - center.y;
                        let dx = (radius * radius - dy * dy).max(0.).sqrt();
                        // This interval is monotone in y; its x-side changes only at extrema.
                        let middle = (pair[0] + pair[1]) / 2.;
                        let sign = if middle > FRAC_PI_2 && middle < 3. * FRAC_PI_2 {
                            -1.
                        } else {
                            1.
                        };
                        crossings += usize::from(center.x + sign * dx > point.x);
                    }
                }
            }
            _ => unreachable!("conversion admits only profile curves"),
        }
    }
    crossings % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_kernel::ProfileSegment;
    use ferritecad_types::StableEntityId;
    use std::f64::consts::PI;
    fn ring(x: f64, radius: f64) -> ProfileLoop {
        ProfileLoop::closed_curve(ProfileSegment::new(
            StableEntityId::new(),
            G::circle(P { x, y: 0. }, radius).expect("radius"),
        ))
        .expect("closed")
    }
    #[test]
    fn nested_and_separate_boundaries_use_parity() {
        let result = profile(
            SketchPlane::world_xy(),
            vec![ring(0., 10.), ring(1., 6.), ring(1., 2.), ring(30., 4.)],
        )
        .expect("regions");
        assert_eq!(result.regions().len(), 3);
        assert_eq!(result.regions()[0].inner.len(), 1);
        assert!(result.regions()[1].inner.is_empty());
        assert!(result.regions()[2].inner.is_empty());
    }
    #[test]
    fn intersections_and_tangencies_name_both_curves() {
        for x in [0., 1., 2.] {
            let a = ring(0., 1.);
            let b = ring(x, 1.);
            let ids = [a.segments()[0].label, b.segments()[0].label];
            let error = profile(SketchPlane::world_xy(), vec![a, b]).expect_err("touching");
            for id in ids {
                assert!(error.to_string().contains(&id.to_string()));
            }
        }
    }
    #[test]
    fn circular_ray_crossings_are_analytic() {
        for theta in [0., PI / 4., FRAC_PI_2, PI, 3. * FRAC_PI_2] {
            assert!(contains(&ring(0., 3.), at(P::ORIGIN, 2., theta)));
            assert!(!contains(&ring(0., 3.), at(P::ORIGIN, 4., theta)));
        }
    }
}
