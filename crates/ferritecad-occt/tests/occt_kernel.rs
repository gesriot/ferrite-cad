// SPDX-License-Identifier: MIT
//! The Open CASCADE adapter against real geometry.
//!
//! Every test skips when the crate was built without Open CASCADE, because
//! that is a build configuration rather than a defect. The pin workflow sets
//! `FERRITECAD_REQUIRE_OCCT=1`, so a run whose purpose is to prove the adapter
//! works cannot pass by skipping.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_kernel::{
    CancelToken, ExtrudeExtent, ExtrudeRequest, GeometryKernel, HistoryInput, OperationContext,
    PlanarPoint, Profile, ProfileLoop, ProfileSegment, ProgressSink, SegmentGeometry, SketchPlane,
    TessellationParams,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{CadError, ErrorKind, Result, StableEntityId, Transform, Vec3};

/// Returns `None`, having said why, when there is no kernel to test.
macro_rules! kernel_or_skip {
    () => {{
        if !is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return;
        }
        OcctKernel::new().expect("a build with Open CASCADE opens a session")
    }};
}

struct Rectangle {
    request: ExtrudeRequest,
    labels: Vec<StableEntityId>,
}

fn rectangle(width: f64, depth: f64, height: f64) -> Result<Rectangle> {
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

    Ok(Rectangle {
        request: ExtrudeRequest::new(profile, ExtrudeExtent::blind(height)?, false),
        labels,
    })
}

#[test]
fn the_adapter_reports_the_open_cascade_it_was_built_against() {
    let kernel = kernel_or_skip!();
    let identity = kernel.identity();

    assert_eq!(identity.id(), "occt");
    // Read from the library, not assumed: it keys every cache entry.
    assert!(
        identity.version().starts_with(char::is_numeric),
        "expected a version number, got {:?}",
        identity.version()
    );
    assert!(identity.build().contains("bridge"));
}

#[test]
fn a_rectangle_extrudes_into_a_solid_of_the_right_size() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(60.0, 40.0, 10.0).expect("a valid rectangle");

    let result = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("Open CASCADE builds a prism from a rectangle");

    let (faces, volume) = kernel.shape_stats(result.shape).expect("measures");
    assert_eq!(faces, 6, "a rectangular prism has six faces");
    assert!(
        (volume - 24_000.0).abs() < 1e-6,
        "60 x 40 x 10 is 24000 mm^3, got {volume}"
    );

    kernel.release(result.shape);
}

#[test]
fn every_profile_segment_raised_exactly_one_side_face() {
    // The regression for the finding that shaped the bridge: MakeWire welds
    // vertices by *replacing* edges, so history queried with the edges we
    // built returned a face for the first segment and nothing for the rest.
    // Sharing the corner vertices up front is what makes this pass.
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(60.0, 40.0, 10.0).expect("a valid rectangle");

    let result = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");

    for label in &plate.labels {
        let generated: Vec<_> = result
            .history
            .generated(HistoryInput::Segment(*label))
            .collect();
        assert_eq!(
            generated.len(),
            1,
            "segment {label} raised {} faces, expected exactly one",
            generated.len()
        );
    }

    kernel.release(result.shape);
}

#[test]
fn the_caps_are_reported_apart_from_history_and_are_distinct() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(20.0, 20.0, 5.0).expect("a valid rectangle");

    let result = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");

    // FirstShape and LastShape are faces on OCCT 7.9.x and 8.0.x; the adapter
    // reports them apart because they are generated from no input.
    assert_eq!(result.start_cap.len(), 1);
    assert_eq!(result.end_cap.len(), 1);
    assert_ne!(result.start_cap[0], result.end_cap[0]);

    // Four sides plus two caps, and no side face is also a cap.
    let mut all: Vec<_> = plate
        .labels
        .iter()
        .flat_map(|l| result.history.generated(HistoryInput::Segment(*l)))
        .chain(result.start_cap.iter().copied())
        .chain(result.end_cap.iter().copied())
        .collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 6);

    kernel.release(result.shape);
}

#[test]
fn a_profile_containing_an_arc_builds() {
    let mut kernel = kernel_or_skip!();

    // A half disc: an arc from (10,0) round to (-10,0), closed by a diameter.
    let arc_label = StableEntityId::new();
    let line_label = StableEntityId::new();
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(vec![
            ProfileSegment::new(
                arc_label,
                SegmentGeometry::arc(PlanarPoint::ORIGIN, 10.0, 0.0, std::f64::consts::PI)
                    .expect("a positive radius"),
            ),
            ProfileSegment::new(
                line_label,
                SegmentGeometry::line(
                    PlanarPoint::new(-10.0, 0.0).expect("finite"),
                    PlanarPoint::new(10.0, 0.0).expect("finite"),
                )
                .expect("distinct"),
            ),
        ])
        .expect("closes"),
        Vec::new(),
    )
    .expect("valid");

    let request = ExtrudeRequest::new(profile, ExtrudeExtent::blind(4.0).expect("positive"), false);
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("a half disc is extrudable");

    let (_, volume) = kernel.shape_stats(result.shape).expect("measures");
    // Half of pi r^2 h = 0.5 * pi * 100 * 4.
    let expected = 0.5 * std::f64::consts::PI * 100.0 * 4.0;
    assert!(
        (volume - expected).abs() < 1.0,
        "expected about {expected} mm^3, got {volume}"
    );

    assert_eq!(
        result
            .history
            .generated(HistoryInput::Segment(arc_label))
            .count(),
        1,
        "the arc raised a cylindrical side face"
    );

    kernel.release(result.shape);
}

#[test]
fn a_symmetric_extrusion_straddles_the_plane() {
    let mut kernel = kernel_or_skip!();
    let mut plate = rectangle(10.0, 10.0, 1.0).expect("a valid rectangle");
    plate.request = ExtrudeRequest::new(
        plate.request.profile().clone(),
        ExtrudeExtent::symmetric(3.0).expect("positive"),
        false,
    );

    let result = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");
    let (_, volume) = kernel.shape_stats(result.shape).expect("measures");

    // Three either side is six in total: 10 x 10 x 6.
    assert!((volume - 600.0).abs() < 1e-6, "got {volume}");
    kernel.release(result.shape);
}

#[test]
fn a_reversed_extrusion_has_the_same_volume() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(10.0, 10.0, 2.0).expect("a valid rectangle");
    let reversed = ExtrudeRequest::new(
        plate.request.profile().clone(),
        plate.request.extent(),
        true,
    );

    let result = kernel
        .extrude(&reversed, &OperationContext::default())
        .expect("builds downwards");
    let (faces, volume) = kernel.shape_stats(result.shape).expect("measures");

    assert_eq!(faces, 6);
    assert!((volume - 200.0).abs() < 1e-6, "got {volume}");
    kernel.release(result.shape);
}

#[test]
fn cancelling_before_the_call_returns_cancelled_and_builds_nothing() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(10.0, 10.0, 2.0).expect("a valid rectangle");

    let token = CancelToken::new();
    token.cancel();
    let err = kernel
        .extrude(
            &plate.request,
            &OperationContext::default().with_cancel(token),
        )
        .expect_err("a cancelled context must not produce geometry");

    assert!(matches!(err, CadError::Cancelled));
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn cancelling_as_the_operation_starts_is_honoured() {
    // This is the granularity Open CASCADE actually offers for a prism.
    // Measured on 7.9.3, BRepPrimAPI_MakePrism polls the progress indicator
    // zero times, so cancellation is checked between steps — before the
    // profile is built and before the sweep — and not inside the sweep. The
    // indicator is still installed for the algorithms that do poll it.
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(10.0, 10.0, 2.0).expect("a valid rectangle");

    let token = CancelToken::new();
    let trigger = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ProgressSink::new(move |_| trigger.cancel()));

    let err = kernel
        .extrude(&plate.request, &context)
        .expect_err("cancelling at the first progress report stops the build");

    assert!(matches!(err, CadError::Cancelled));
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn releasing_gives_the_shape_back_and_releasing_twice_is_harmless() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(10.0, 10.0, 2.0).expect("a valid rectangle");

    let result = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");
    assert_eq!(kernel.live_shape_count(), 1);

    kernel.release(result.shape);
    assert_eq!(kernel.live_shape_count(), 0);

    // An unwinding caller releases whatever it might hold.
    kernel.release(result.shape);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_handle_from_another_session_is_refused() {
    let mut owner = kernel_or_skip!();
    let plate = rectangle(10.0, 10.0, 2.0).expect("a valid rectangle");
    let result = owner
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");

    let mut stranger = OcctKernel::new().expect("a second session opens");
    let err = stranger
        .shape_stats(result.shape)
        .expect_err("a handle does not survive its session");
    assert_eq!(err.kind(), ErrorKind::Kernel);

    // Releasing a foreign handle must not touch the stranger's own shapes.
    stranger.release(result.shape);
    assert_eq!(owner.live_shape_count(), 1);

    owner.release(result.shape);
}

#[test]
fn the_operations_this_slice_omits_say_so() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(10.0, 10.0, 2.0).expect("a valid rectangle");
    let result = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");
    let context = OperationContext::default();

    let moved =
        Transform::from_translation(Vec3::new(1.0, 0.0, 0.0).expect("finite")).expect("finite");
    for err in [
        kernel
            .transform(result.shape, &moved, &context)
            .map(|_| ())
            .expect_err("transform is not implemented"),
        kernel
            .tessellate(result.shape, &TessellationParams::default(), &context)
            .map(|_| ())
            .expect_err("tessellation is not implemented"),
        kernel
            .encode_shape(result.shape)
            .map(|_| ())
            .expect_err("encoding is not implemented"),
    ] {
        // Refusing is the honest answer; the alternative is a plausible wrong
        // one that nobody notices until much later.
        assert_eq!(err.kind(), ErrorKind::Unsupported);
    }

    kernel.release(result.shape);
}

#[test]
fn a_profile_with_holes_is_refused() {
    let mut kernel = kernel_or_skip!();
    let outer = rectangle(20.0, 20.0, 2.0).expect("a valid rectangle");

    let hole = ProfileLoop::new(vec![
        ProfileSegment::new(
            StableEntityId::new(),
            SegmentGeometry::line(
                PlanarPoint::new(5.0, 5.0).expect("finite"),
                PlanarPoint::new(10.0, 5.0).expect("finite"),
            )
            .expect("distinct"),
        ),
        ProfileSegment::new(
            StableEntityId::new(),
            SegmentGeometry::line(
                PlanarPoint::new(10.0, 5.0).expect("finite"),
                PlanarPoint::new(5.0, 5.0).expect("finite"),
            )
            .expect("distinct"),
        ),
    ])
    .expect("closes");

    let profile = Profile::new(
        SketchPlane::world_xy(),
        outer.request.profile().outer().clone(),
        vec![hole],
    )
    .expect("valid as a profile");

    let request = ExtrudeRequest::new(profile, ExtrudeExtent::blind(2.0).expect("positive"), false);
    let err = kernel
        .extrude(&request, &OperationContext::default())
        .expect_err("holes need more than one wire");

    assert_eq!(err.kind(), ErrorKind::Unsupported);
    assert_eq!(kernel.live_shape_count(), 0);
}
