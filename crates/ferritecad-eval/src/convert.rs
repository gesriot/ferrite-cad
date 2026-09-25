// SPDX-License-Identifier: MIT
//! Turning stored objects into kernel requests.
//!
//! The kernel crate says the evaluator will convert between persistence types
//! and kernel DTOs explicitly, where the conversion can be read and tested.
//! This is that conversion, and it is where the first slice's boundaries are
//! enforced: everything outside them fails with
//! [`CadError::Unsupported`][ferritecad_types::CadError::Unsupported] rather
//! than being approximated into something plausible.

use ferritecad_document::{
    AnnularExtrusion, DatumPlane, EndCondition, Extrude, Point2, Revolve, RevolveAxis,
    RevolveExtent, Sketch, SketchCurve, SketchGeometry, SolidOperation,
};
use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, PlanarPoint, Profile, ProfileLoop, ProfileSegment,
    RevolveRequest, SegmentGeometry, SketchPlane,
};
use ferritecad_types::{CadError, ObjectId, Point3, Result, StableEntityId, Vec3};

use crate::presentation::SketchPresentation;
use crate::solve::SketchSolveReport;

/// How close two endpoints must be to count as joined, in millimetres.
///
/// Matches the kernel's own loop tolerance. Chosen to be far below anything a
/// user could have meant on a millimetre-scale part, so a gap this small is
/// arithmetic and a gap any larger is a different sketch.
const JOIN_TOLERANCE: f64 = 1.0e-6;

/// Reads a datum's placement as a plane the kernel can build on.
pub fn plane_from_datum(datum: &DatumPlane) -> Result<SketchPlane> {
    let origin = datum.placement.apply_to_point(Point3::ORIGIN)?;
    let x_axis = datum.placement.apply_to_vector(Vec3::X)?;
    let normal = datum.placement.apply_to_vector(Vec3::Z)?;
    SketchPlane::new(origin, x_axis, normal)
}

/// Builds a profile from a sketch's curves.
///
/// The first slice accepts exactly one closed outer loop of lines and arcs.
/// Construction geometry is ignored, as it produces no edges by definition.
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
    /// What the kernel is asked to sweep. Less than the drawing by design:
    /// construction geometry bounds no face, and the rest is chained into one
    /// loop.
    pub profile: Profile,
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
    Ok(SketchEvaluation {
        profile: profile_from_curves(evaluated, plane)?,
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
    let model: Vec<&SketchCurve> = sketch.curves.iter().filter(|c| !c.construction).collect();

    if model.is_empty() {
        return Err(CadError::input(
            "the sketch has no model geometry, so there is no profile to extrude",
        ));
    }

    // One analytic curve that closes on itself is a whole profile by itself.
    // It is read here rather than through the chain below, which exists to
    // join segments end to end and has no endpoints to work with.
    if let [only] = model.as_slice()
        && let SketchGeometry::Circle { center, radius } = only.geometry
    {
        let segment = ProfileSegment::new(
            only.id,
            SegmentGeometry::circle(planar(center.x, center.y)?, radius)?,
        );
        return Profile::new(plane, ProfileLoop::closed_curve(segment)?, Vec::new());
    }

    // Two of them are a boundary and a hole, for the one nesting this slice
    // builds. Read here for the same reason: neither closed curve has an
    // endpoint the chain below could follow.
    if let [first, second] = model.as_slice()
        && let SketchGeometry::Circle {
            center: first_center,
            radius: first_radius,
        } = first.geometry
        && let SketchGeometry::Circle {
            center: second_center,
            radius: second_radius,
        } = second.geometry
    {
        return annulus_from_circles(
            plane,
            (first.id, first_center, first_radius),
            (second.id, second_center, second_radius),
        );
    }

    let mut segments = Vec::with_capacity(model.len());
    for curve in &model {
        segments.push(ProfileSegment::new(curve.id, segment_geometry(curve)?));
    }

    let ordered = chain_into_one_loop(segments)?;
    Profile::new(plane, ProfileLoop::new(ordered)?, Vec::new())
}

/// One circular hole in one circular boundary, from two stored circles.
///
/// # Which circle is the boundary is a question about the geometry
///
/// The larger radius bounds the region and the smaller one is the hole. That is
/// read from the two radii and from nothing else — in particular not from the
/// order the two curves happen to sit in the sketch, which is presentation
/// order and may be either way round. A sketch whose two circles are stored the
/// other way round is the same drawing and must extrude to the same solid, with
/// each wall still belonging to the circle that drew it.
///
/// Everything narrower than that is the published policy, asked of
/// [`AnnularExtrusion`] rather than restated here: one centre within the
/// kernel's own point tolerance, a hole strictly inside its boundary, and a
/// wall thick enough that the solid is the one that was asked for. A drawing
/// outside it is refused rather than approximated into the nearest one that
/// would build.
fn annulus_from_circles(
    plane: SketchPlane,
    first: (StableEntityId, Point2, f64),
    second: (StableEntityId, Point2, f64),
) -> Result<Profile> {
    let (outer, inner) = if first.2 >= second.2 {
        (first, second)
    } else {
        (second, first)
    };
    if !AnnularExtrusion::concentric(outer.1, inner.1) {
        return Err(CadError::unsupported(format!(
            "sketch curves {} and {} are circles about different centres, and an off-centre hole \
             needs more than this slice builds",
            outer.0, inner.0
        )));
    }
    // The stored numbers have to be inside the policy a new one is judged by,
    // or a document written by something else could extrude to a solid this
    // slice would refuse to create.
    AnnularExtrusion::new(
        [outer.1.x, outer.1.y],
        outer.2,
        inner.2,
        // A height this function does not have and does not need: the extent is
        // the extrusion's, and is checked where it is read. One that a document
        // will store stands in so the two radii can be judged on their own.
        1.0,
    )
    .map_err(|error| {
        CadError::unsupported(format!(
            "sketch curves {} and {} are two circles this slice will not extrude: {error}",
            outer.0, inner.0
        ))
    })?;

    let loop_of = |(id, center, radius): (StableEntityId, Point2, f64)| -> Result<ProfileLoop> {
        ProfileLoop::closed_curve(ProfileSegment::new(
            id,
            SegmentGeometry::circle(planar(center.x, center.y)?, radius)?,
        ))
    };
    Profile::new(plane, loop_of(outer)?, vec![loop_of(inner)?])
}

/// Builds the tool an in-place boolean sweeps, from the same stored feature.
///
/// The tool is an ordinary extrusion: the feature's own sketch, swept forward
/// along its plane's normal for the stored blind depth. That is the whole of
/// what this slice cuts with, and it is stated here rather than inferred, so a
/// feature that asks for something else is refused with the reason rather than
/// approximated into the nearest thing that would build.
///
/// A ThroughAll tool has no stored length. It runs for exactly the [`Reach`] of
/// the body it cuts: the forward Blind height of the extrusion that started that
/// body, read in this rebuild. A boolean cut only removes material, so every
/// solid along the history lies inside that first prism, which spans `[0, h]`
/// along the shared datum normal; a tool of that length on that datum reaches
/// the far side of everything present. It is the same tool, down to the last
/// bit, as a Blind cut whose depth equals the height, which is the "through"
/// case every earlier slice already measured: no epsilon is added and no other
/// plane is guessed at.
///
/// A separate function from [`extrude_request`] because the two answer
/// different questions. That one asks what solid a feature *is*, and refuses a
/// boolean because a boolean is not a solid on its own; this one asks what a
/// boolean removes with, which is a solid and is built the same way every
/// extrusion is.
pub fn cut_tool_request(
    feature: &Extrude,
    profile: Profile,
    tool_datum: ObjectId,
    reach: Option<Reach>,
) -> Result<ExtrudeRequest> {
    if feature.operation != SolidOperation::Cut {
        return Err(CadError::unsupported(format!(
            "feature operation {:?} is not a cut, and this slice implements no other boolean",
            feature.operation
        )));
    }
    if feature.reversed {
        return Err(CadError::unsupported(
            "this slice cuts along the sketch plane's normal; a reversed cut is not implemented,              and silently cutting the other way would remove material nobody asked for",
        ));
    }
    let extent = match &feature.end_condition {
        EndCondition::Blind { distance } => ExtrudeExtent::blind(distance.value())?,
        EndCondition::Symmetric { .. } => {
            return Err(CadError::unsupported(
                "a symmetric cut removes material on both sides of the sketch plane, which this                  slice does not implement",
            ));
        }
        EndCondition::ThroughAll => {
            let reach = reach.ok_or_else(|| {
                CadError::unsupported(
                    "a ThroughAll cut needs the body it cuts to start with a forward Blind \
                     extrusion, whose height says how far everything in it reaches",
                )
            })?;
            if reach.datum != tool_datum {
                return Err(CadError::unsupported(format!(
                    "a ThroughAll tool is evaluated on the datum its body started from ({}); this \
                     one is drawn on {tool_datum}, and this slice measures no other direction",
                    reach.datum
                )));
            }
            ExtrudeExtent::blind(reach.distance)?
        }
        other => {
            return Err(CadError::unsupported(format!(
                "end condition {other:?} is not implemented"
            )));
        }
    };
    Ok(ExtrudeRequest::new(profile, extent, false))
}

/// How far a body reaches ahead of the datum its first extrusion was drawn on.
///
/// Recorded for a forward Blind NewBody and inherited unchanged by every Cut
/// that modifies it, because a cut removes material and cannot reach further.
/// Absent for anything whose extent this slice does not state (Symmetric,
/// reversed or ThroughAll roots); a ThroughAll Cut of such a body is refused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reach {
    pub datum: ObjectId,
    pub distance: f64,
}

impl Reach {
    /// The reach of a feature that starts a body, when it has one.
    pub fn of_new_body(feature: &Extrude, datum: ObjectId) -> Option<Self> {
        match (&feature.end_condition, feature.reversed, feature.previous) {
            (EndCondition::Blind { distance }, false, None) => Some(Self {
                datum,
                distance: distance.value(),
            }),
            _ => None,
        }
    }
}

/// Builds a full-turn revolution request from a stored Revolve.
///
/// The stored intent is checked against the one class this build evaluates —
/// a NewBody full turn about the sketch Y axis — and the profile against the
/// shared domain policy ([`ferritecad_document::FullTurnRevolution`]) before
/// any kernel sees it. A saved profile the policy refuses is refused here even
/// if a kernel could build it.
pub fn revolve_request(feature: &Revolve, profile: Profile) -> Result<RevolveRequest> {
    if feature.operation != SolidOperation::NewBody {
        return Err(CadError::unsupported(format!(
            "revolve operation {:?} needs a boolean, which this slice does not implement",
            feature.operation
        )));
    }
    let axis = match feature.axis {
        RevolveAxis::SketchY => ferritecad_kernel::RevolveAxis::PlaneY,
        other => {
            return Err(CadError::unsupported(format!(
                "revolve axis {other:?} is not implemented"
            )));
        }
    };
    let turn = match feature.extent {
        RevolveExtent::FullTurn => ferritecad_kernel::RevolveTurn::Full,
        other => {
            return Err(CadError::unsupported(format!(
                "revolve extent {other:?} is not implemented"
            )));
        }
    };
    if !profile.inner().is_empty() || profile.outer().is_closed_curve() {
        return Err(CadError::unsupported(
            "a revolution turns one closed polygon of Lines, without holes",
        ));
    }
    let mut points = Vec::with_capacity(profile.outer().segments().len());
    for segment in profile.outer().segments() {
        let SegmentGeometry::Line { start, .. } = segment.geometry else {
            return Err(CadError::unsupported(
                "a revolution turns a profile of Lines only",
            ));
        };
        points.push([start.x, start.y]);
    }
    ferritecad_document::FullTurnRevolution::new(points)?;
    Ok(RevolveRequest::new(profile, axis, turn))
}

/// Builds an extrusion request from a stored feature.
pub fn extrude_request(feature: &Extrude, profile: Profile) -> Result<ExtrudeRequest> {
    if feature.operation != SolidOperation::NewBody {
        return Err(CadError::unsupported(format!(
            "extrude operation {:?} needs a boolean, which this slice does not implement; \
             only NewBody is supported",
            feature.operation
        )));
    }
    if feature.target_body.is_some() {
        return Err(CadError::unsupported(
            "an extrude targeting an existing body needs a boolean, which this slice does not \
             implement",
        ));
    }

    let extent = match &feature.end_condition {
        // Both sides read `distance` as the distance *per side*, so the total
        // sweep is twice it. The two crates use different field names for the
        // same quantity, which is exactly the sort of thing an explicit
        // conversion exists to pin down.
        EndCondition::Blind { distance } => ExtrudeExtent::blind(distance.value())?,
        EndCondition::Symmetric { distance } => ExtrudeExtent::symmetric(distance.value())?,
        EndCondition::ThroughAll => {
            return Err(CadError::unsupported(
                "ThroughAll needs to know what else exists, which requires booleans; \
                 this slice does not implement it",
            ));
        }
        other => {
            return Err(CadError::unsupported(format!(
                "end condition {other:?} is not implemented"
            )));
        }
    };

    Ok(ExtrudeRequest::new(profile, extent, feature.reversed))
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
        // A circle is a closed loop on its own. One alone is a profile and two
        // are a boundary and a hole; both are read before this point. What is
        // left is a circle mixed in with lines or arcs, which would be a loop
        // beside a chain — more than one region, rather than one with a hole.
        SketchGeometry::Circle { .. } => Err(CadError::unsupported(format!(
            "sketch curve {} is a circle, which is a whole loop on its own; a circle alongside \
             lines or arcs needs more than the one boundary and one hole this slice builds",
            curve.id
        ))),
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

/// Orders the segments head to tail and insists they form exactly one loop.
///
/// A sketch stores its curves in presentation order, which need not be the
/// order they connect in, so the chain has to be walked rather than assumed.
/// Walking it also separates two failures that would otherwise look alike: a
/// sketch that closes and has segments left over is a valid multi-loop profile
/// this slice does not implement, while a sketch that never closes is an open
/// profile and cannot be extruded by anything.
///
/// Segments are followed in the orientation they were stored. A chain that
/// would only close by reversing a segment is reported rather than silently
/// reversed: reversing an arc changes which way it sweeps, and guessing at that
/// produces a different solid from the one drawn.
fn chain_into_one_loop(segments: Vec<ProfileSegment>) -> Result<Vec<ProfileSegment>> {
    let mut remaining = segments;
    let first = remaining.remove(0);
    let start = first.geometry.start()?;

    let mut ordered = vec![first];
    loop {
        let tail = ordered
            .last()
            .ok_or_else(|| CadError::input("the chain lost its head"))?
            .geometry
            .end()?;

        if joins(tail, start) {
            break;
        }

        let next = remaining
            .iter()
            .position(|candidate| match candidate.geometry.start() {
                Ok(head) => joins(tail, head),
                Err(_) => false,
            });

        match next {
            Some(index) => ordered.push(remaining.remove(index)),
            None => {
                return Err(CadError::input(format!(
                    "the profile does not close: no segment starts at ({}, {}), where segment {} \
                     ends. Segments must be stored head to tail in one direction.",
                    tail.x,
                    tail.y,
                    ordered
                        .last()
                        .map(|s| s.label.to_string())
                        .unwrap_or_default()
                )));
            }
        }
    }

    if !remaining.is_empty() {
        return Err(CadError::unsupported(format!(
            "the sketch closes one loop and has {} segment(s) left over; profiles with holes or \
             several loops are not implemented in this slice",
            remaining.len()
        )));
    }

    Ok(ordered)
}

fn joins(a: PlanarPoint, b: PlanarPoint) -> bool {
    (a.x - b.x).abs() <= JOIN_TOLERANCE && (a.y - b.y).abs() <= JOIN_TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_document::{Expression, Point2};
    use ferritecad_types::{ErrorKind, ObjectId, StableEntityId, Transform};

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
            profile: ObjectId::new(),
            end_condition,
            reversed: false,
            operation: SolidOperation::NewBody,
            target_body: None,
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
    }

    #[test]
    fn a_through_all_tool_runs_exactly_the_reach_of_the_body_it_cuts() {
        let datum = ObjectId::new();
        let base = extrude(EndCondition::Blind {
            distance: Expression::constant(14.25).expect("finite"),
        });
        let reach = Reach::of_new_body(&base, datum).expect("a forward Blind body has a reach");
        assert_eq!(
            reach,
            Reach {
                datum,
                distance: 14.25
            }
        );
        let tool = square();
        let request = cut_tool_request(
            &cut_of(EndCondition::ThroughAll),
            tool.clone(),
            datum,
            Some(reach),
        )
        .expect("through all");
        // No epsilon, no other number: exactly the Blind tool of that height.
        assert_eq!(
            request.extent(),
            ExtrudeExtent::blind(14.25).expect("blind")
        );
        assert!(!request.reversed());
        let blind = cut_tool_request(
            &cut_of(EndCondition::Blind {
                distance: Expression::constant(14.25).expect("finite"),
            }),
            tool,
            datum,
            Some(reach),
        )
        .expect("blind");
        assert_eq!(blind, request, "Blind at the height is the same tool");
    }

    #[test]
    fn through_all_without_a_stated_reach_or_on_another_datum_is_refused() {
        let datum = ObjectId::new();
        let reach = Reach {
            datum,
            distance: 12.0,
        };
        let err = cut_tool_request(
            &cut_of(EndCondition::ThroughAll),
            square(),
            ObjectId::new(),
            Some(reach),
        )
        .expect_err("another plane");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
        let err = cut_tool_request(&cut_of(EndCondition::ThroughAll), square(), datum, None)
            .expect_err("no reach");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
        // Only a forward Blind NewBody states a reach.
        for base in [
            extrude(EndCondition::Symmetric {
                distance: Expression::constant(5.0).expect("finite"),
            }),
            extrude(EndCondition::ThroughAll),
            Extrude {
                reversed: true,
                ..extrude(EndCondition::Blind {
                    distance: Expression::constant(5.0).expect("finite"),
                })
            },
        ] {
            assert_eq!(Reach::of_new_body(&base, datum), None, "{base:?}");
        }
    }

    #[test]
    fn a_square_becomes_a_four_segment_profile() {
        let profile = profile_from_sketch(
            &sketch(square_curves()),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("a closed square converts")
        .profile;
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
                .profile;
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
                .profile;
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
        .profile;
        assert!(profile.inner().is_empty());
        let outer = profile.outer();
        assert!(outer.is_closed_curve(), "not approximated by a chain");
        assert_eq!(outer.joints().len(), 0);
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
    fn a_circle_beside_lines_is_a_loop_beside_a_chain_and_unsupported() {
        // One circle is a profile and two concentric ones are a region with a
        // hole; a circle *and* a square is two regions, which needs more.
        let mut curves = square_curves();
        curves.push(model_circle((5.0, 5.0), 1.0));
        let err = profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
            .expect_err("a loop beside a chain is not one region");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
        assert!(err.to_string().contains("whole loop on its own"));
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
                    .profile;

            let boundary = profile.outer();
            assert!(boundary.is_closed_curve(), "not approximated by a chain");
            assert_eq!(boundary.joints().len(), 0);
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
            assert_eq!(hole.joints().len(), 0);
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
    fn two_circles_outside_the_annular_policy_are_refused_rather_than_approximated() {
        for (what, a, b) in [
            // Side by side: two regions, not one with a hole.
            (
                "disjoint",
                model_circle((0.0, 0.0), 1.0),
                model_circle((9.0, 0.0), 1.0),
            ),
            // Nested but off centre: geometry the kernel builds, outside this slice.
            (
                "eccentric",
                model_circle((0.0, 0.0), 10.0),
                model_circle((3.0, 0.0), 4.0),
            ),
            // The same circle twice: no wall at all.
            (
                "coincident",
                model_circle((0.0, 0.0), 5.0),
                model_circle((0.0, 0.0), 5.0),
            ),
            // A wall below the published minimum.
            (
                "hairline",
                model_circle((0.0, 0.0), 10.0),
                model_circle((0.0, 0.0), 9.9999),
            ),
            // Outside the shared 1e6 bound.
            (
                "huge",
                model_circle((0.0, 0.0), 2e6),
                model_circle((0.0, 0.0), 4.0),
            ),
        ] {
            let err = profile_from_sketch(
                &sketch(vec![a, b]),
                ObjectId::new(),
                SketchPlane::world_xy(),
            )
            .expect_err(what);
            assert_eq!(err.kind(), ErrorKind::Unsupported, "{what}: {err}");
        }
        // A circle that is not a circle at all is still refused before this.
        for radius in [0.0, -3.0] {
            assert!(
                profile_from_sketch(
                    &sketch(vec![
                        model_circle((0.0, 0.0), 10.0),
                        model_circle((0.0, 0.0), radius),
                    ]),
                    ObjectId::new(),
                    SketchPlane::world_xy(),
                )
                .is_err(),
                "r{radius}"
            );
        }
    }

    #[test]
    fn three_circles_are_more_than_one_hole_and_unsupported() {
        let err = profile_from_sketch(
            &sketch(vec![
                model_circle((0.0, 0.0), 10.0),
                model_circle((0.0, 0.0), 6.0),
                model_circle((0.0, 0.0), 2.0),
            ]),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect_err("two holes are not one hole");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
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
        .profile;
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
        .expect_err("one line closes nothing");
        assert_eq!(err.kind(), ErrorKind::Input);
    }

    #[test]
    fn a_model_point_is_unsupported() {
        let mut curves = square_curves();
        curves.push(SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Point { at: Point2::ORIGIN },
        });

        let err = profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
            .expect_err("a point bounds no face");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn a_second_loop_is_unsupported_not_misread_as_one() {
        let mut curves = square_curves();
        curves.extend([
            line((2.0, 2.0), (4.0, 2.0)),
            line((4.0, 2.0), (4.0, 4.0)),
            line((4.0, 4.0), (2.0, 2.0)),
        ]);

        let err = profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
            .expect_err("two loops are a profile with a hole");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
        assert!(err.to_string().contains("left over"));
    }

    #[test]
    fn an_open_chain_is_an_input_error_not_an_unsupported_one() {
        // The distinction matters to the user: one is a sketch to finish, the
        // other is a feature to wait for.
        let curves = vec![
            line((0.0, 0.0), (10.0, 0.0)),
            line((10.0, 0.0), (10.0, 10.0)),
        ];

        let err = profile_from_sketch(&sketch(curves), ObjectId::new(), SketchPlane::world_xy())
            .expect_err("an open chain has no face");
        assert_eq!(err.kind(), ErrorKind::Input);
        assert!(err.to_string().contains("does not close"));
    }

    #[test]
    fn an_empty_sketch_is_refused() {
        let err = profile_from_sketch(
            &sketch(Vec::new()),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
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
        .profile;
        let request = extrude_request(
            &extrude(EndCondition::Blind {
                distance: Expression::constant(8.0).expect("finite"),
            }),
            profile,
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
        .profile;
        let request = extrude_request(
            &extrude(EndCondition::Symmetric {
                distance: Expression::constant(4.0).expect("finite"),
            }),
            profile,
        )
        .expect("converts");

        // Four either side is eight in total; this test is the record of that
        // reading, since both field names say only "distance".
        assert_eq!(request.extent().total_length(), 8.0);
    }

    #[test]
    fn through_all_is_unsupported() {
        let profile = profile_from_sketch(
            &sketch(square_curves()),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("converts")
        .profile;
        let err = extrude_request(&extrude(EndCondition::ThroughAll), profile)
            .expect_err("ThroughAll needs booleans");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn a_boolean_operation_is_unsupported() {
        for operation in [
            SolidOperation::Add,
            SolidOperation::Cut,
            SolidOperation::Intersect,
        ] {
            let profile = profile_from_sketch(
                &sketch(square_curves()),
                ObjectId::new(),
                SketchPlane::world_xy(),
            )
            .expect("converts")
            .profile;
            let mut feature = extrude(EndCondition::Blind {
                distance: Expression::constant(8.0).expect("finite"),
            });
            feature.operation = operation;

            let err = extrude_request(&feature, profile).expect_err("booleans are not implemented");
            assert_eq!(err.kind(), ErrorKind::Unsupported);
        }
    }

    #[test]
    fn a_target_body_is_unsupported() {
        let profile = profile_from_sketch(
            &sketch(square_curves()),
            ObjectId::new(),
            SketchPlane::world_xy(),
        )
        .expect("converts")
        .profile;
        let mut feature = extrude(EndCondition::Blind {
            distance: Expression::constant(8.0).expect("finite"),
        });
        feature.target_body = Some(ObjectId::new());

        let err = extrude_request(&feature, profile).expect_err("modifying a body needs booleans");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn a_datum_placement_becomes_a_plane() {
        let plane = plane_from_datum(&DatumPlane {
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
        let plane = plane_from_datum(&DatumPlane { placement: rotate })
            .expect("a rotated frame is still a frame");

        // Z rotated a quarter turn about X points along -Y.
        assert!((plane.normal().y + 1.0).abs() < 1e-12);
        assert!(plane.normal().z.abs() < 1e-12);
    }
}
