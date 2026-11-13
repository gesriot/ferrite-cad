// SPDX-License-Identifier: MIT
//! Turning stored objects into kernel requests.
//!
//! The kernel crate says the evaluator will convert between persistence types
//! and kernel DTOs explicitly, where the conversion can be read and tested.
//! This conversion keeps persistence types separate from kernel requests.

use ferritecad_document::{
    DatumPlane, EndCondition, Extrude, Revolve, RevolveAxis, RevolveExtent, Sketch, SketchCurve,
    SketchGeometry,
};
use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, PlanarPoint, Profile, ProfileLoop, ProfileSegment,
    RevolveRequest, SegmentGeometry, SketchPlane,
};
use ferritecad_types::{CadError, ObjectId, Point3, Result, Vec3};

use crate::presentation::SketchPresentation;
use crate::solve::SketchSolveReport;

/// How close two endpoints must be to count as joined, in millimetres.
///
/// Matches the kernel's own loop tolerance. Chosen to be far below anything a
/// user could have meant on a millimetre-scale part, so a gap this small is
/// arithmetic and a gap any larger is a different sketch.
pub(crate) const JOIN_TOLERANCE: f64 = 1.0e-6;

/// Reads a fixed datum's placement; dependent datums require an evaluated model.
pub fn plane_from_datum(datum: &DatumPlane) -> Result<SketchPlane> {
    let DatumPlane::Fixed { placement } = datum else {
        return Err(CadError::input(
            "this plane depends on geometry; use its evaluated plane",
        ));
    };
    let origin = placement.apply_to_point(Point3::ORIGIN)?;
    let x_axis = placement.apply_to_vector(Vec3::X)?;
    let normal = placement.apply_to_vector(Vec3::Z)?;
    SketchPlane::new(origin, x_axis, normal)
}

pub(crate) fn vertex_extent(
    plane: SketchPlane,
    reversed: bool,
    p: Point3,
) -> Result<ExtrudeExtent> {
    let p = Point3::new(p.x, p.y, p.z)
        .map_err(|error| CadError::kernel_because("vertex point is not finite", error))?;
    let origin = plane.origin();
    let n = plane.normal();
    let sign = if reversed { -1. } else { 1. };
    let distance =
        sign * ((p.x - origin.x) * n.x + (p.y - origin.y) * n.y + (p.z - origin.z) * n.z);
    if !distance.is_finite() {
        return Err(CadError::kernel("vertex projection is not finite"));
    }
    if distance <= 0. {
        return Err(CadError::input(
            "end vertex is not ahead of the sketch plane",
        ));
    }
    ExtrudeExtent::blind(distance)
}

/// Evaluate a sketch drawing once, retaining its profile or the reason no profile exists.
///
/// Main curves form disjoint closed boundaries with even/odd nesting.
/// Points and construction geometry remain in the drawing but bound no profile.
///
/// A sketch that carries constraints is solved first, and the profile is built
/// from the answer rather than from the stored coordinates: the stored ones
/// are wherever the curves were last left, and the constraints are what the
/// drawing means. The solve happens on a temporary copy and changes nothing
/// the document holds. See [`crate::solve`].
///
/// A sketch with no constraints has nothing to solve and asks no solver
/// anything, so a document written before constraints existed still rebuilds
/// in a build that never linked one. That is also why the report is optional:
/// no solve, nothing to report, and an empty report would be a claim that a
/// sketch nobody constrained is fully constrained.
///
/// All three sides of [`SketchEvaluation`] come out of the one solve. A caller
/// that wanted the profile, the drawing or the facts separately would have to
/// ask twice, and the second answer would be a second solver run of a sketch
/// that had already been solved.
///
/// `id` is what the document calls this sketch. It is not used to build
/// anything: it is the one word a solver cannot supply, and it is needed so a
/// refusal can say which sketch it is about. Passing it in beats attaching it
/// to the error afterwards, which would let a report exist without a sketch.
pub fn profile_from_sketch(
    sketch: &Sketch,
    id: ObjectId,
    plane: SketchPlane,
) -> Result<SketchEvaluation> {
    match crate::solve::solved(sketch, id)? {
        Some((solved, report)) => evaluation_of(&solved, id, plane, Some(report)),
        None => evaluation_of(sketch, id, plane, None),
    }
}

/// The three sides of one sketch evaluation.
///
/// Named rather than returned as a tuple because the three are the same
/// answer read three ways, and which of them a caller took would otherwise be
/// a position to count out. What holds them together is that they are built
/// from one `&Sketch`: whichever coordinates that sketch carries, the profile
/// and the drawing carry the same ones, and there is no arrangement in which a
/// caller gets a solved profile beside a stored drawing.
#[derive(Debug)]
#[non_exhaustive]
pub struct SketchEvaluation {
    /// The strict reading for sweeps, or its complete failure. A failed reading
    /// does not invalidate the drawing, which can be empty or unfinished.
    pub profile: Result<Profile>,
    /// The whole drawing, at the coordinates the profile was built from.
    pub presentation: SketchPresentation,
    /// What the solve found out, and `None` when there was nothing to solve.
    pub report: Option<SketchSolveReport>,
}

/// Reads one set of coordinates three ways.
///
/// `evaluated` is whatever the profile is entitled to be built from – the
/// solved sketch when there were constraints, the stored one when there were
/// none. Both sides are built from it here, in one place, so neither can come
/// from anywhere else.
fn evaluation_of(
    evaluated: &Sketch,
    id: ObjectId,
    plane: SketchPlane,
    report: Option<SketchSolveReport>,
) -> Result<SketchEvaluation> {
    for curve in &evaluated.curves {
        match &curve.geometry {
            SketchGeometry::Point { .. }
            | SketchGeometry::Line { .. }
            | SketchGeometry::Arc { .. }
            | SketchGeometry::Circle { .. } => {}
            other => {
                return Err(CadError::unsupported(format!(
                    "sketch curve {} has geometry {other:?}, which is not implemented",
                    curve.id
                )));
            }
        }
    }
    Ok(SketchEvaluation {
        profile: profile_from_curves(evaluated, plane),
        presentation: SketchPresentation::of(id, plane, evaluated),
        report,
    })
}

/// The profile arithmetic itself, over whichever coordinates it was given.
///
/// Unchanged by the solver's arrival, and deliberately so: a solved sketch is
/// a sketch, and there is one account of what a closed loop of lines and arcs
/// means rather than one for each way the coordinates were arrived at.
fn profile_from_curves(sketch: &Sketch, plane: SketchPlane) -> Result<Profile> {
    let model: Vec<&SketchCurve> = sketch
        .curves
        .iter()
        .filter(|c| c.bounds_profile())
        .collect();

    if model.is_empty() {
        return Err(CadError::input(
            "the sketch has no main Line, Arc or Circle, so it gives no profile",
        ));
    }

    let segments = model
        .iter()
        .map(|c| Ok(ProfileSegment::new(c.id, segment_geometry(c)?)))
        .collect::<Result<Vec<_>>>()?;
    crate::regions::profile(plane, chain_loops(segments)?)
}

/// Resolve an axis from the same solved drawing that supplied the profile.
pub fn revolve_request(
    feature: &Revolve,
    profile: Profile,
    drawing: &SketchPresentation,
) -> Result<RevolveRequest> {
    let axis = match feature.axis {
        RevolveAxis::SketchX => ferritecad_kernel::RevolveAxis::PlaneX,
        RevolveAxis::SketchY => ferritecad_kernel::RevolveAxis::PlaneY,
        RevolveAxis::Line(id) => {
            let geometry = drawing
                .curves()
                .iter()
                .find(|c| c.id() == id)
                .map(|c| c.geometry());
            let Some(SketchGeometry::Line { start, end }) = geometry else {
                return Err(CadError::input(format!(
                    "axis {id} is not a Line in the profile sketch"
                )));
            };
            ferritecad_kernel::RevolveAxis::Line {
                start: planar(start.x, start.y)?,
                end: planar(end.x, end.y)?,
            }
        }
        other => {
            return Err(CadError::unsupported(format!(
                "unknown revolve axis {other:?}"
            )));
        }
    };
    let turn = match &feature.extent {
        extent if extent.is_full_turn() => ferritecad_kernel::RevolveTurn::Full,
        RevolveExtent::Partial { angle } | RevolveExtent::ByAngle { angle } => {
            ferritecad_kernel::RevolveTurn::Partial(ferritecad_kernel::PartialTurn::new(
                angle.value(),
            )?)
        }
        other => {
            return Err(CadError::unsupported(format!(
                "unknown revolve extent {other:?}"
            )));
        }
    };
    Ok(RevolveRequest::new(profile, axis, turn))
}

// A millimetre beyond the target prevents a ThroughAll end cap from coinciding
// with the farthest target face; this is a tool margin, not a contact policy.
const THROUGH_ALL_MARGIN_MM: f64 = 1.0;

/// Builds an extrusion tool from a stored feature and target bounds.
pub fn extrude_request(
    feature: &Extrude,
    profile: Profile,
    bounds: Option<[f64; 2]>,
) -> Result<ExtrudeRequest> {
    let extent = match &feature.end_condition {
        // Both sides read `distance` as the distance *per side*, so the total
        // sweep is twice it. The two crates use different field names for the
        // same quantity, which is exactly the sort of thing an explicit
        // conversion exists to pin down.
        EndCondition::Blind { distance } => ExtrudeExtent::blind(distance.value())?,
        EndCondition::Symmetric { distance } => ExtrudeExtent::symmetric(distance.value())?,
        EndCondition::UpToNext => {
            return Err(CadError::input("up to next needs its target result"));
        }
        EndCondition::ThroughAll => {
            let bounds = bounds.ok_or_else(|| {
                CadError::input("ThroughAll needs a target body; there is nothing to go through")
            })?;
            return through_all_request(profile, feature.reversed, bounds);
        }
        EndCondition::UpToFace { .. } | EndCondition::UpToVertex { .. } => {
            return Err(CadError::input(
                "a referenced end condition needs its resolved support",
            ));
        }
        _ => return Err(CadError::unsupported("unsupported extrusion end condition")),
    };

    Ok(ExtrudeRequest::new(profile, extent, feature.reversed))
}

pub(crate) fn through_all_request(
    profile: Profile,
    reversed: bool,
    bounds: [f64; 2],
) -> Result<ExtrudeRequest> {
    let normal = profile.plane().normal();
    let origin = profile.plane().origin();
    let sign = if reversed { -1. } else { 1. };
    let ahead =
        bounds[1] - sign * (origin.x * normal.x + origin.y * normal.y + origin.z * normal.z);
    if ahead <= 0. {
        return Err(CadError::input(
            "ThroughAll has nothing ahead of the sketch plane to go through",
        ));
    }
    let extent = ExtrudeExtent::blind(ahead + THROUGH_ALL_MARGIN_MM)?;
    Ok(ExtrudeRequest::new(profile, extent, reversed))
}

fn segment_geometry(curve: &SketchCurve) -> Result<SegmentGeometry> {
    match &curve.geometry {
        SketchGeometry::Line { start, end } => {
            SegmentGeometry::line(planar(start.x, start.y)?, planar(end.x, end.y)?)
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => SegmentGeometry::arc(
            planar(center.x, center.y)?,
            *radius,
            *start_angle,
            *end_angle,
        ),
        SketchGeometry::Circle { center, radius } => {
            SegmentGeometry::circle(planar(center.x, center.y)?, *radius)
        }
        SketchGeometry::Point { .. } => Err(CadError::unsupported(format!(
            "sketch curve {} is a point, which bounds no face",
            curve.id
        ))),
        other => Err(CadError::unsupported(format!(
            "sketch curve {} has geometry {other:?}, which is not implemented",
            curve.id
        ))),
    }
}

fn planar(x: f64, y: f64) -> Result<PlanarPoint> {
    PlanarPoint::new(x, y)
}

/// Connect every boundary through either stored endpoint, retaining an existing directed walk.
fn chain_loops(mut remaining: Vec<ProfileSegment>) -> Result<Vec<ProfileLoop>> {
    let mut ends = Vec::new();
    for segment in &remaining {
        if !segment.geometry.is_closed() {
            ends.push((segment.label, false, segment.geometry.start()?));
            ends.push((segment.label, true, segment.geometry.end()?));
        }
    }
    // Check all joints before walking: a completed loop must not conceal an attached branch.
    let mut free = None;
    for &(curve, end, point) in &ends {
        let neighbours: Vec<_> = ends.iter().filter(|(_, _, p)| joins(point, *p)).collect();
        if neighbours.len() > 2 {
            let mut curves: Vec<_> = neighbours.iter().map(|(id, _, _)| *id).collect();
            curves.sort();
            curves.dedup();
            let curves = curves
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(CadError::input(format!(
                "profile curves {curves} have more than two ends at ({}, {})",
                point.x, point.y
            )));
        }
        if neighbours.len() == 1 && free.is_none() {
            free = Some((curve, end, point));
        }
    }
    if let Some((curve, end, point)) = free {
        let end = if end { "end" } else { "start" };
        return Err(CadError::input(format!(
            "the profile does not close: curve {curve} has a free {end} at ({}, {})",
            point.x, point.y
        )));
    }
    let mut loops = Vec::new();
    while !remaining.is_empty() {
        let first = remaining.remove(0);
        if first.geometry.is_closed() {
            loops.push(ProfileLoop::closed_curve(first)?);
            continue;
        }
        let start = first.geometry.start()?;
        let mut ordered = vec![first];
        loop {
            let last = ordered.last().expect("the chain starts with one segment");
            let tail = last.geometry.end()?;
            if joins(tail, start) {
                break;
            }
            let next = remaining.iter().enumerate().find_map(|(i, segment)| {
                if segment.geometry.start().is_ok_and(|p| joins(tail, p)) {
                    Some((i, false))
                } else if segment.geometry.end().is_ok_and(|p| joins(tail, p)) {
                    Some((i, true))
                } else {
                    None
                }
            });
            let Some((index, reverse)) = next else {
                return Err(CadError::input(format!(
                    "the profile does not close: curve {} has a free end at ({}, {})",
                    last.label, tail.x, tail.y
                )));
            };
            let mut segment = remaining.remove(index);
            if reverse {
                segment.reverse()?;
            }
            ordered.push(segment);
        }
        loops.push(ProfileLoop::new(ordered)?);
    }
    Ok(loops)
}

fn joins(a: PlanarPoint, b: PlanarPoint) -> bool {
    (a.x - b.x).abs() <= JOIN_TOLERANCE && (a.y - b.y).abs() <= JOIN_TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_document::{Expression, Point2, SolidOperation};
    use ferritecad_types::{ErrorKind, ObjectId, StableEntityId, Transform};

    #[test]
    fn vertex_extent_checks_kernel_numbers_and_the_requested_direction() {
        let plane = SketchPlane::new(Point3::ORIGIN, Vec3::X, Vec3::Z).expect("plane");
        for p in [
            Point3 {
                x: f64::NAN,
                y: 0.,
                z: 1.,
            },
            Point3 {
                x: 0.,
                y: 0.,
                z: f64::INFINITY,
            },
        ] {
            assert_eq!(
                vertex_extent(plane, false, p)
                    .expect_err("nonfinite")
                    .kind(),
                ErrorKind::Kernel
            );
        }
        for z in [0., -1.] {
            assert_eq!(
                vertex_extent(plane, false, Point3::new(0., 0., z).expect("point"))
                    .expect_err("not ahead")
                    .kind(),
                ErrorKind::Input
            );
        }
        assert_eq!(
            vertex_extent(plane, true, Point3::new(10., 20., -3.).expect("point"))
                .expect("reversed"),
            ExtrudeExtent::blind(3.).expect("blind")
        );
        let far = SketchPlane::new(
            Point3::new(0., 0., -f64::MAX).expect("origin"),
            Vec3::X,
            Vec3::Z,
        )
        .expect("plane");
        assert_eq!(
            vertex_extent(far, false, Point3::new(0., 0., f64::MAX).expect("point"))
                .expect_err("overflow")
                .kind(),
            ErrorKind::Kernel
        );
    }

    fn line(start: (f64, f64), end: (f64, f64)) -> SketchCurve {
        SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(start.0, start.1).expect("finite"),
                end: Point2::new(end.0, end.1).expect("finite"),
            },
        }
    }

    fn square_curves() -> Vec<SketchCurve> {
        vec![
            line((0.0, 0.0), (10.0, 0.0)),
            line((10.0, 0.0), (10.0, 10.0)),
            line((10.0, 10.0), (0.0, 10.0)),
            line((0.0, 10.0), (0.0, 0.0)),
        ]
    }

    fn sketch(curves: Vec<SketchCurve>) -> Sketch {
        Sketch {
            plane: ObjectId::new(),
            curves,
            constraints: Vec::new(),
        }
    }

    fn extrude(end_condition: EndCondition) -> Extrude {
        Extrude {
            regions: Vec::new(),
            profile: ObjectId::new(),
            end_condition,
            reversed: false,
            operation: SolidOperation::NewBody,
            previous: None,
        }
    }

    fn cut_of(end_condition: EndCondition) -> Extrude {
        Extrude {
            operation: SolidOperation::Cut,
            previous: Some(ObjectId::new()),
            ..extrude(end_condition)
        }
    }

    fn square() -> Profile {
        profile_from_sketch(
            &sketch(square_curves()),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("converts")
        .profile
        .expect("closed profile")
    }

    #[test]
    fn through_all_uses_projected_bounds_and_a_margin() {
        let tool = extrude_request(
            &cut_of(EndCondition::ThroughAll),
            square(),
            Some([-7., 14.25]),
        )
        .expect("through target");
        assert_eq!(tool.extent().total_length(), 14.25 + THROUGH_ALL_MARGIN_MM);
        let error = extrude_request(
            &cut_of(EndCondition::ThroughAll),
            square(),
            Some([-14., 0.]),
        )
        .expect_err("nothing ahead");
        assert!(error.to_string().contains("nothing ahead"));
    }

    #[test]
    fn a_square_becomes_a_four_segment_profile() {
        let profile = profile_from_sketch(
            &sketch(square_curves()),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("a closed square converts")
        .profile
        .expect("closed profile");
        assert_eq!(profile.outer().segments().len(), 4);
        assert!(profile.inner().is_empty());
    }

    #[test]
    fn curves_stored_out_of_order_are_chained() {
        // Presentation order is not connection order, and the evaluator must
        // not depend on them agreeing.
        let mut curves = square_curves();
        curves.swap(1, 3);

        let profile =
            profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
                .expect("order is recovered by walking the chain")
                .profile
                .expect("closed profile");
        assert_eq!(profile.outer().segments().len(), 4);
    }

    #[test]
    fn construction_geometry_is_ignored() {
        let mut curves = square_curves();
        curves.push(SketchCurve {
            id: StableEntityId::new(),
            construction: true,
            geometry: SketchGeometry::Circle {
                center: Point2::new(5.0, 5.0).expect("finite"),
                radius: 2.0,
            },
        });

        let profile =
            profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
                .expect("a construction circle bounds nothing and is skipped")
                .profile
                .expect("closed profile");
        assert_eq!(profile.outer().segments().len(), 4);
    }

    fn model_circle(center: (f64, f64), radius: f64) -> SketchCurve {
        SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Circle {
                center: Point2::new(center.0, center.1).expect("finite"),
                radius,
            },
        }
    }

    #[test]
    fn one_model_circle_is_one_analytic_loop() {
        let curve = model_circle((12.0, -7.0), 10.0);
        let id = curve.id;
        let profile = profile_from_sketch(
            &sketch(vec![curve]),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("a circle is a whole profile")
        .profile
        .expect("closed profile");
        assert!(profile.inner().is_empty());
        let outer = profile.outer();
        assert!(outer.is_closed_curve(), "not approximated by a chain");
        let segment = outer.segments().first().expect("one curve");
        assert_eq!(segment.label, id, "the circle's own identity");
        assert!(
            matches!(segment.geometry, SegmentGeometry::Circle { center, radius }
                if (center.x, center.y, radius) == (12.0, -7.0, 10.0)),
            "the kernel is handed a circle, not a polygon: {:?}",
            segment.geometry
        );
    }

    #[test]
    fn a_circle_beside_lines_is_a_hole() {
        let mut curves = square_curves();
        curves.push(model_circle((5.0, 5.0), 1.0));
        let profile =
            profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
                .expect("mixed boundaries")
                .profile
                .expect("closed profile");
        assert_eq!(profile.regions().len(), 1);
        assert_eq!(profile.inner().len(), 1);
    }

    /// Two concentric circles are one region with a hole, whichever order they
    /// are stored in.
    #[test]
    fn two_concentric_circles_are_a_boundary_and_a_hole_in_either_stored_order() {
        let outer = model_circle((12.0, -7.0), 10.0);
        let inner = model_circle((12.0, -7.0), 4.0);
        // Both orders, because presentation order is not a fact about the
        // drawing and must not decide which circle is the boundary.
        for curves in [
            vec![outer.clone(), inner.clone()],
            vec![inner.clone(), outer.clone()],
        ] {
            let profile =
                profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
                    .expect("a circle inside a circle is a region with a hole")
                    .profile
                    .expect("closed profile");

            let boundary = profile.outer();
            assert!(boundary.is_closed_curve(), "not approximated by a chain");
            let drawn = boundary.segments().first().expect("one curve");
            assert_eq!(drawn.label, outer.id, "the larger circle bounds the region");
            assert!(matches!(
                drawn.geometry,
                SegmentGeometry::Circle { center, radius }
                    if (center.x, center.y, radius) == (12.0, -7.0, 10.0)
            ));

            assert_eq!(profile.inner().len(), 1);
            let hole = &profile.inner()[0];
            assert!(hole.is_closed_curve());
            let drawn = hole.segments().first().expect("one curve");
            assert_eq!(drawn.label, inner.id, "the smaller circle is the hole");
            assert!(matches!(
                drawn.geometry,
                SegmentGeometry::Circle { center, radius }
                    if (center.x, center.y, radius) == (12.0, -7.0, 4.0)
            ));
            assert_eq!(profile.segments().count(), 2);
        }
    }

    #[test]
    fn circle_regions_have_no_concentricity_size_or_wall_policy() {
        for (outer, inner, regions) in [
            (model_circle((0., 0.), 1.), model_circle((9., 0.), 1.), 2),
            (model_circle((0., 0.), 10.), model_circle((3., 0.), 4.), 1),
            (
                model_circle((0., 0.), 10.),
                model_circle((0., 0.), 9.9999),
                1,
            ),
            (model_circle((0., 0.), 2e6), model_circle((0., 0.), 4.), 1),
        ] {
            let profile = profile_from_sketch(
                &sketch(vec![outer, inner]),
                ObjectId::new(),
                SketchPlane::world_xy(),
            )
            .expect("general regions")
            .profile
            .expect("closed profile");
            assert_eq!(profile.regions().len(), regions);
        }
    }

    #[test]
    fn three_nested_circles_are_a_region_and_an_island() {
        let profile = profile_from_sketch(
            &sketch(vec![
                model_circle((0., 0.), 10.),
                model_circle((0., 0.), 6.),
                model_circle((0., 0.), 2.),
            ]),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("nested contours")
        .profile
        .expect("closed profile");
        assert_eq!(profile.regions().len(), 2);
        assert_eq!(profile.regions()[0].inner.len(), 1);
        assert!(profile.regions()[1].inner.is_empty());
    }

    #[test]
    fn a_construction_circle_does_not_become_a_hole() {
        // Construction geometry bounds no face, so a model circle with a
        // construction circle inside it is still one plain cylinder.
        let mut inner = model_circle((0.0, 0.0), 4.0);
        inner.construction = true;
        let profile = profile_from_sketch(
            &sketch(vec![model_circle((0.0, 0.0), 10.0), inner]),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("one model circle")
        .profile
        .expect("closed profile");
        assert!(
            profile.inner().is_empty(),
            "a construction circle is not a hole"
        );
    }

    #[test]
    fn a_circle_with_no_radius_is_refused_before_the_kernel() {
        for radius in [0.0, -3.0] {
            let err = profile_from_sketch(
                &sketch(vec![model_circle((0.0, 0.0), radius)]),
                ObjectId::new(),
                SketchPlane::world_xy(),
            )
            .expect("drawing")
            .profile
            .expect_err("a circle needs a positive radius");
            assert_eq!(err.kind(), ErrorKind::Input);
        }
    }

    #[test]
    fn a_lone_line_is_still_an_open_profile_rather_than_a_loop() {
        // The circle exception must not become "one curve is always a loop".
        let err = profile_from_sketch(
            &sketch(vec![line((0.0, 0.0), (10.0, 0.0))]),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("drawing")
        .profile
        .expect_err("one line closes nothing");
        assert_eq!(err.kind(), ErrorKind::Input);
    }

    #[test]
    fn a_model_point_does_not_change_the_profile() {
        let mut curves = square_curves();
        curves.push(SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Point { at: Point2::ORIGIN },
        });

        let evaluation =
            profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
                .expect("drawing");
        assert_eq!(evaluation.profile.expect("profile").segments().count(), 4);
        assert_eq!(evaluation.presentation.curves().len(), 5);
    }

    #[test]
    fn a_second_loop_inside_the_first_is_a_hole() {
        let mut curves = square_curves();
        curves.extend([
            line((2., 2.), (4., 2.)),
            line((4., 2.), (4., 4.)),
            line((4., 4.), (2., 2.)),
        ]);
        let profile =
            profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
                .expect("nested loops")
                .profile
                .expect("closed profile");
        assert_eq!(profile.regions().len(), 1);
        assert_eq!(profile.inner().len(), 1);
    }

    #[test]
    fn an_open_drawing_retains_an_input_profile_reason() {
        // The distinction matters to the user: one is a sketch to finish, the
        // other is a feature to wait for.
        let curves = vec![
            line((0.0, 0.0), (10.0, 0.0)),
            line((10.0, 0.0), (10.0, 10.0)),
        ];

        let err = profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
            .expect("drawing")
            .profile
            .expect_err("an open chain has no face");
        assert_eq!(err.kind(), ErrorKind::Input);
        assert!(err.to_string().contains("does not close"));
    }

    #[test]
    fn an_empty_drawing_has_no_profile() {
        let err = profile_from_sketch(
            &sketch(Vec::new()),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("drawing")
        .profile
        .expect_err("nothing to extrude");
        assert_eq!(err.kind(), ErrorKind::Input);
    }

    #[test]
    fn a_blind_extrude_converts() {
        let profile = profile_from_sketch(
            &sketch(square_curves()),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("converts")
        .profile
        .expect("closed profile");
        let request = extrude_request(
            &extrude(EndCondition::Blind {
                distance: Expression::constant(8.0).expect("finite"),
            }),
            profile,
            None,
        )
        .expect("converts");

        assert_eq!(request.extent().total_length(), 8.0);
        assert!(!request.reversed());
    }

    #[test]
    fn a_symmetric_extrude_sweeps_the_distance_on_each_side() {
        let profile = profile_from_sketch(
            &sketch(square_curves()),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("converts")
        .profile
        .expect("closed profile");
        let request = extrude_request(
            &extrude(EndCondition::Symmetric {
                distance: Expression::constant(4.0).expect("finite"),
            }),
            profile,
            None,
        )
        .expect("converts");

        // Four either side is eight in total; this test is the record of that
        // reading, since both field names say only "distance".
        assert_eq!(request.extent().total_length(), 8.0);
    }

    #[test]
    fn a_datum_placement_becomes_a_plane() {
        let plane = plane_from_datum(&DatumPlane::Fixed {
            placement: Transform::IDENTITY,
        })
        .expect("identity is a valid frame");

        assert_eq!(plane.origin(), Point3::ORIGIN);
        assert_eq!(plane.normal(), Vec3::Z);
    }

    #[test]
    fn a_rotated_datum_carries_its_orientation_through() {
        let rotate = Transform::from_rotation(Vec3::X, std::f64::consts::FRAC_PI_2)
            .expect("a quarter turn about X");
        let plane = plane_from_datum(&DatumPlane::Fixed { placement: rotate })
            .expect("a rotated frame is still a frame");

        // Z rotated a quarter turn about X points along -Y.
        assert!((plane.normal().y + 1.0).abs() < 1e-12);
        assert!(plane.normal().z.abs() < 1e-12);
    }
}
