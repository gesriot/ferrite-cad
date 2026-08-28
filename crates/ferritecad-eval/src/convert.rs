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
    DatumPlane, EndCondition, Extrude, Sketch, SketchCurve, SketchGeometry, SolidOperation,
};
use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, PlanarPoint, Profile, ProfileLoop, ProfileSegment,
    SegmentGeometry, SketchPlane,
};
use ferritecad_types::{CadError, Point3, Result, Vec3};

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
/// in a build that never linked one.
pub fn profile_from_sketch(sketch: &Sketch, plane: SketchPlane) -> Result<Profile> {
    match crate::solve::solved(sketch)? {
        Some(solved) => profile_from_curves(&solved, plane),
        None => profile_from_curves(sketch, plane),
    }
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

    let mut segments = Vec::with_capacity(model.len());
    for curve in &model {
        segments.push(ProfileSegment::new(curve.id, segment_geometry(curve)?));
    }

    let ordered = chain_into_one_loop(segments)?;
    Profile::new(plane, ProfileLoop::new(ordered)?, Vec::new())
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
        // A circle is a closed loop on its own, so it cannot be one segment of
        // a chain; supporting it means supporting multi-loop profiles.
        SketchGeometry::Circle { .. } => Err(CadError::unsupported(format!(
            "sketch curve {} is a circle, which this slice does not implement",
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
        }
    }

    #[test]
    fn a_square_becomes_a_four_segment_profile() {
        let profile = profile_from_sketch(&sketch(square_curves()), SketchPlane::world_xy())
            .expect("a closed square converts");
        assert_eq!(profile.outer().segments().len(), 4);
        assert!(profile.inner().is_empty());
    }

    #[test]
    fn curves_stored_out_of_order_are_chained() {
        // Presentation order is not connection order, and the evaluator must
        // not depend on them agreeing.
        let mut curves = square_curves();
        curves.swap(1, 3);

        let profile = profile_from_sketch(&sketch(curves), SketchPlane::world_xy())
            .expect("order is recovered by walking the chain");
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

        let profile = profile_from_sketch(&sketch(curves), SketchPlane::world_xy())
            .expect("a construction circle bounds nothing and is skipped");
        assert_eq!(profile.outer().segments().len(), 4);
    }

    #[test]
    fn a_model_circle_is_unsupported_rather_than_approximated() {
        let curves = vec![SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Circle {
                center: Point2::ORIGIN,
                radius: 5.0,
            },
        }];

        let err = profile_from_sketch(&sketch(curves), SketchPlane::world_xy())
            .expect_err("a circle is not a chain segment");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn a_model_point_is_unsupported() {
        let mut curves = square_curves();
        curves.push(SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Point { at: Point2::ORIGIN },
        });

        let err = profile_from_sketch(&sketch(curves), SketchPlane::world_xy())
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

        let err = profile_from_sketch(&sketch(curves), SketchPlane::world_xy())
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

        let err = profile_from_sketch(&sketch(curves), SketchPlane::world_xy())
            .expect_err("an open chain has no face");
        assert_eq!(err.kind(), ErrorKind::Input);
        assert!(err.to_string().contains("does not close"));
    }

    #[test]
    fn an_empty_sketch_is_refused() {
        let err = profile_from_sketch(&sketch(Vec::new()), SketchPlane::world_xy())
            .expect_err("nothing to extrude");
        assert_eq!(err.kind(), ErrorKind::Input);
    }

    #[test]
    fn a_blind_extrude_converts() {
        let profile = profile_from_sketch(&sketch(square_curves()), SketchPlane::world_xy())
            .expect("converts");
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
        let profile = profile_from_sketch(&sketch(square_curves()), SketchPlane::world_xy())
            .expect("converts");
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
        let profile = profile_from_sketch(&sketch(square_curves()), SketchPlane::world_xy())
            .expect("converts");
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
            let profile = profile_from_sketch(&sketch(square_curves()), SketchPlane::world_xy())
                .expect("converts");
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
        let profile = profile_from_sketch(&sketch(square_curves()), SketchPlane::world_xy())
            .expect("converts");
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
