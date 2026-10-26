// SPDX-License-Identifier: MIT
//! The boolean the Open CASCADE adapter added, against real geometry.
//!
//! Everything here is measured from the kernel rather than assumed: the volume
//! the cut removed, the surfaces the result lies on, and what the boolean says
//! became of each face it was given. A test that only counted faces would pass
//! against an adapter that returned the target unchanged.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;

use std::f64::consts::PI;

use ferritecad_kernel::{
    BooleanRequest, CancelToken, ExtrudeExtent, ExtrudeRequest, FaceSurface, GeometryKernel,
    NamingRelation, OperationContext, PlanarPoint, Profile, ProfileLoop, ProfileSegment,
    SegmentGeometry, ShapeHandle, SketchPlane, SubShapeHandle,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{ErrorKind, Point3, Result, StableEntityId, Vec3};

macro_rules! kernel_or_skip {
    () => {{
        if !is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        OcctKernel::new().expect("a build with Open CASCADE opens a session")
    }};
}

/// The plate of the measurable acceptance: 60 x 40 x 10 from the world origin.
fn plate(width: f64, depth: f64, height: f64) -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
    let corners = [
        PlanarPoint::new(0.0, 0.0)?,
        PlanarPoint::new(width, 0.0)?,
        PlanarPoint::new(width, depth)?,
        PlanarPoint::new(0.0, depth)?,
    ];
    let mut segments = Vec::new();
    let mut labels = Vec::new();
    for (index, start) in corners.iter().enumerate() {
        let label = StableEntityId::new();
        labels.push(label);
        segments.push(ProfileSegment::new(
            label,
            SegmentGeometry::line(*start, corners[(index + 1) % corners.len()])?,
        ));
    }
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(segments)?,
        Vec::new(),
    )?;
    Ok((
        ExtrudeRequest::new(profile, ExtrudeExtent::blind(height)?, false),
        labels,
    ))
}

/// A cylindrical tool on the same plane, running the same way.
fn tool(center: (f64, f64), radius: f64, depth: f64) -> Result<(ExtrudeRequest, StableEntityId)> {
    let label = StableEntityId::new();
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::closed_curve(ProfileSegment::new(
            label,
            SegmentGeometry::circle(PlanarPoint::new(center.0, center.1)?, radius)?,
        ))?,
        Vec::new(),
    )?;
    Ok((
        ExtrudeRequest::new(profile, ExtrudeExtent::blind(depth)?, false),
        label,
    ))
}

#[test]
fn a_blind_tool_through_a_plate_leaves_one_solid_with_a_cylindrical_hole() {
    let mut kernel = kernel_or_skip!();
    let context = OperationContext::default();

    let (plate_request, plate_labels) = plate(60.0, 40.0, 10.0).expect("a plate");
    let built = kernel.extrude(&plate_request, &context).expect("the plate");
    let (tool_request, circle) = tool((20.0, 15.0), 5.0, 10.0).expect("a tool");
    let cutter = kernel.extrude(&tool_request, &context).expect("the tool");

    // What the plate's own faces are called, before anything is removed.
    let plate_faces = naming::faces(&mut kernel, &built, &plate_labels);
    assert_eq!(plate_faces.len(), 6, "a plate has six named faces");
    let tool_faces = naming::faces(&mut kernel, &cutter, &[circle]);
    assert_eq!(tool_faces.len(), 3, "a cylinder has three named faces");

    let mut track = plate_faces.clone();
    track.extend(tool_faces.iter().copied());
    let target_before = kernel.encode_shape(built.shape).expect("archive target");
    let tool_before = kernel.encode_shape(cutter.shape).expect("archive tool");
    let request = BooleanRequest::new(
        ferritecad_kernel::BooleanKind::Cut,
        built.shape,
        cutter.shape,
    )
    .expect("two shapes");
    let result = kernel.boolean(&request, &context).expect("a real cut");
    let trace = kernel
        .naming_history(result.shape)
        .expect("complete history");
    assert!(
        kernel.encode_shape(built.shape).expect("target after") == target_before,
        "a cut must not mutate the historical feature"
    );
    assert!(
        kernel.encode_shape(cutter.shape).expect("tool after") == tool_before,
        "a cut must not mutate its tool"
    );

    // The material that went is the cylinder's, measured by the kernel.
    let expected = PI * 25.0 * 10.0;
    assert!(
        (result.removed_volume - expected).abs() < 1e-6 * expected,
        "removed {} rather than {expected}",
        result.removed_volume
    );
    let (faces, volume) = kernel.shape_stats(result.shape).expect("stats");
    assert_eq!(faces, 7, "six plate faces and the bore wall");
    assert!(
        (volume - (24000.0 - expected)).abs() < 1e-6 * volume,
        "the solid measures {volume}"
    );

    // The bore wall is the tool's own side face, carried through the boolean
    // and reported as such — not a face picked out of the result by position.
    // Non-destructive Open CASCADE makes a modified copy of that face rather
    // than changing the tool in place. Its history still identifies the exact
    // input, and the measured surface below verifies the result.
    let wall_input = tool_faces[0];
    assert_eq!(
        Some(&naming::survival(&trace, wall_input)),
        Some(&NamingRelation::Modified),
        "the tool's own wall is what bounds the bore"
    );
    let carried: Vec<SubShapeHandle> = naming::surviving(&trace, wall_input).collect();
    assert_eq!(carried.len(), 1, "one wall came from one tool face");
    assert_eq!(
        kernel.face_surface(carried[0]).expect("a surface"),
        FaceSurface::Cylinder { radius: 5.0 },
        "the bore is analytic and the radius the tool had"
    );

    // The plate's own faces survived, and the boolean says so. The top and
    // bottom are modified because the hole is now a boundary of each; the four
    // sides are untouched.
    let mut kept = 0;
    let mut modified = 0;
    for face in &plate_faces {
        match naming::survival(&trace, *face) {
            NamingRelation::Kept => kept += 1,
            NamingRelation::Modified => modified += 1,
            NamingRelation::Deleted => panic!("a through hole deleted a face of the plate"),
            other => panic!("this build has no reading of {other:?}"),
        }
    }
    assert_eq!(
        (kept, modified),
        (4, 2),
        "four walls kept, two caps reshaped"
    );

    // The tool's caps are gone: a through hole has no floor.
    let vanished = tool_faces
        .iter()
        .filter(|face| Some(&naming::survival(&trace, **face)) == Some(&NamingRelation::Deleted))
        .count();
    assert_eq!(vanished, 2, "both caps of a through tool are removed");
    for shape in [built.shape, cutter.shape, result.shape] {
        kernel.release(shape);
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_shallow_tool_leaves_a_pocket_with_a_floor() {
    let mut kernel = kernel_or_skip!();
    let context = OperationContext::default();

    let (plate_request, _) = plate(60.0, 40.0, 10.0).expect("a plate");
    let built = kernel.extrude(&plate_request, &context).expect("the plate");
    let (tool_request, circle) = tool((20.0, 15.0), 5.0, 4.0).expect("a tool");
    let cutter = kernel.extrude(&tool_request, &context).expect("the tool");

    let tool_faces = naming::faces(&mut kernel, &cutter, &[circle]);
    let request = BooleanRequest::new(
        ferritecad_kernel::BooleanKind::Cut,
        built.shape,
        cutter.shape,
    )
    .expect("two shapes");
    let result = kernel.boolean(&request, &context).expect("a real cut");
    let trace = kernel
        .naming_history(result.shape)
        .expect("complete history");

    let expected = PI * 25.0 * 4.0;
    assert!((result.removed_volume - expected).abs() < 1e-6 * expected);
    let (faces, volume) = kernel.shape_stats(result.shape).expect("stats");
    assert_eq!(faces, 8, "six plate faces, the bore wall and its floor");
    assert!((volume - (24000.0 - expected)).abs() < 1e-6 * volume);

    // The pocket has exactly one planar floor, and it is the tool's end cap
    // carried through — the cap at z = 4, not the one at z = 0 which the cut
    // opens onto.
    let surviving: Vec<(SubShapeHandle, FaceSurface)> = tool_faces
        .iter()
        .filter(|face| Some(&naming::survival(&trace, **face)) != Some(&NamingRelation::Deleted))
        .flat_map(|face| naming::surviving(&trace, *face))
        .map(|out| (out, kernel.face_surface(out).expect("a surface")))
        .collect();
    assert_eq!(surviving.len(), 2, "the wall and the floor");
    assert!(
        surviving
            .iter()
            .any(|(_, s)| *s == FaceSurface::Cylinder { radius: 5.0 })
    );
    assert!(surviving.iter().any(|(_, s)| *s == FaceSurface::Plane));
    assert_eq!(
        tool_faces
            .iter()
            .filter(|face| Some(&naming::survival(&trace,**face)) == Some(&NamingRelation::Deleted))
            .count(),
        1,
        "only the cap the pocket opens onto is removed"
    );

    for shape in [built.shape, cutter.shape, result.shape] {
        kernel.release(shape);
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_tool_that_misses_a_foreign_handle_and_a_cancelled_run_are_all_refused() {
    let mut kernel = kernel_or_skip!();
    let context = OperationContext::default();

    let (plate_request, _) = plate(60.0, 40.0, 10.0).expect("a plate");
    let built = kernel.extrude(&plate_request, &context).expect("the plate");

    // A tool outside the plate removes nothing, and a boolean that removed
    // nothing is not a feature.
    let (miss_request, _) = tool((200.0, 200.0), 5.0, 10.0).expect("a tool");
    let missed = kernel.extrude(&miss_request, &context).expect("the tool");
    let error = kernel
        .boolean(
            &BooleanRequest::new(
                ferritecad_kernel::BooleanKind::Cut,
                built.shape,
                missed.shape,
            )
            .expect("two shapes"),
            &context,
        )
        .expect_err("a tool that misses is refused");
    assert_eq!(error.kind(), ErrorKind::Unsupported, "{error}");

    // A tool that swallows the plate leaves no solid.
    let (swallow, _) = tool((30.0, 20.0), 500.0, 40.0).expect("a tool");
    let big = kernel.extrude(&swallow, &context).expect("the tool");
    let error = kernel
        .boolean(
            &BooleanRequest::new(ferritecad_kernel::BooleanKind::Cut, built.shape, big.shape)
                .expect("two shapes"),
            &context,
        )
        .expect_err("a cut that leaves nothing is refused");
    assert_eq!(error.kind(), ErrorKind::Unsupported, "{error}");

    let (other_request, _) = tool((20.0, 15.0), 5.0, 10.0).expect("a tool");
    let other = kernel.extrude(&other_request, &context).expect("the tool");

    // Cancellation before the boolean runs.
    let token = CancelToken::new();
    token.cancel();
    let cancelled = OperationContext::default().with_cancel(token);
    let error = kernel
        .boolean(
            &BooleanRequest::new(
                ferritecad_kernel::BooleanKind::Cut,
                built.shape,
                other.shape,
            )
            .expect("two shapes"),
            &cancelled,
        )
        .expect_err("a cancelled cut is refused");
    assert_eq!(error.kind(), ErrorKind::Cancellation, "{error}");

    for shape in [built.shape, missed.shape, big.shape, other.shape] {
        kernel.release(shape);
    }
    assert_eq!(kernel.live_shape_count(), 0);
    let _ = (Point3::ORIGIN, Vec3::Z);
}

#[test]
fn a_cut_of_a_shape_with_itself_is_refused_before_any_kernel_is_asked() {
    let session = ferritecad_kernel::SessionId::new();
    let shape = ShapeHandle::new(session, 7);
    let error = BooleanRequest::new(ferritecad_kernel::BooleanKind::Cut, shape, shape)
        .expect_err("a shape cannot cut itself");
    assert_eq!(error.kind(), ErrorKind::Input, "{error}");

    let elsewhere = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 7);
    let error = BooleanRequest::new(ferritecad_kernel::BooleanKind::Cut, shape, elsewhere)
        .expect_err("two sessions are not one");
    assert_eq!(error.kind(), ErrorKind::Input, "{error}");
}
