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
    BrepBlob, CancelToken, ExtrudeExtent, ExtrudeRequest, GeometryKernel, HistoryInput,
    KernelIdentity, OperationContext, PlanarPoint, Profile, ProfileLoop, ProfileSegment,
    ProgressSink, SegmentGeometry, SketchPlane, TessellationParams,
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
fn cancelling_at_completion_releases_the_finished_shape() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(10.0, 10.0, 2.0).expect("a valid rectangle");

    let token = CancelToken::new();
    let trigger = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ProgressSink::new(move |fraction| {
            if fraction >= 1.0 {
                trigger.cancel();
            }
        }));

    let err = kernel
        .extrude(&plate.request, &context)
        .expect_err("cancelling at the completion report still cancels the operation");

    assert!(matches!(err, CadError::Cancelled));
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the solid already returned by the bridge must be released"
    );
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
fn a_blob_round_trips_inside_one_session() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(30.0, 20.0, 4.0).expect("a valid rectangle");

    let built = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");
    let before = kernel.shape_stats(built.shape).expect("measures");

    let blob = kernel.encode_shape(built.shape).expect("encodes");
    assert!(!blob.bytes().is_empty());
    assert_eq!(blob.kernel(), kernel.identity());

    let restored = kernel.decode_shape(&blob).expect("decodes");
    let after = kernel.shape_stats(restored).expect("measures");

    assert_eq!(before.0, after.0, "face count survives the round trip");
    assert!(
        (before.1 - after.1).abs() < 1e-9,
        "volume survives the round trip: {} vs {}",
        before.1,
        after.1
    );

    kernel.release(built.shape);
    kernel.release(restored);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_blob_round_trips_into_a_different_session() {
    // The point of a cache: the session that wrote it is gone.
    let plate = rectangle(30.0, 20.0, 4.0).expect("a valid rectangle");

    // The writing session ends with this block, which is the situation a cache
    // exists for. A scope rather than an explicit drop: without Open CASCADE
    // this type is an uninhabited stub that implements no Drop at all.
    let (before, blob) = {
        let mut writer = kernel_or_skip!();
        let built = writer
            .extrude(&plate.request, &OperationContext::default())
            .expect("builds");
        let stats = writer.shape_stats(built.shape).expect("measures");
        let blob = writer.encode_shape(built.shape).expect("encodes");
        writer.release(built.shape);
        (stats, blob)
    };

    let mut reader = OcctKernel::new().expect("a second session opens");
    let restored = reader.decode_shape(&blob).expect("decodes");
    let after = reader.shape_stats(restored).expect("measures");

    assert_eq!(before.0, after.0);
    assert!((before.1 - after.1).abs() < 1e-9);

    reader.release(restored);
    assert_eq!(reader.live_shape_count(), 0);
}

#[test]
fn a_decoded_shape_is_the_same_geometry_and_nothing_more() {
    // Open CASCADE's B-Rep format stores a shape, not what made it.
    //
    // At this level the absence of history is structural rather than checked:
    // decode_shape returns a ShapeHandle, not an ExtrudeResult, so there is no
    // empty history to mistake for a real one. The bridge additionally refuses
    // face and cap queries on a decoded shape, which is the same guarantee one
    // layer down. What is worth asserting here is that the geometry really did
    // survive.
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(20.0, 20.0, 3.0).expect("a valid rectangle");

    let built = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");
    let blob = kernel.encode_shape(built.shape).expect("encodes");
    let restored = kernel.decode_shape(&blob).expect("decodes");

    // The geometry is there, and it is the same geometry.
    assert_eq!(
        kernel.shape_stats(restored).expect("measures").0,
        kernel.shape_stats(built.shape).expect("measures").0
    );

    // A second extrusion of the same profile still names its faces, so the
    // refusal below is about the decoded shape and not about the session.
    let again = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");
    assert_eq!(
        again
            .history
            .generated(HistoryInput::Segment(plate.labels[0]))
            .count(),
        1
    );

    kernel.release(built.shape);
    kernel.release(restored);
    kernel.release(again.shape);
}

#[test]
fn a_blob_from_another_kernel_identity_is_refused() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(10.0, 10.0, 2.0).expect("a valid rectangle");
    let built = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");
    let blob = kernel.encode_shape(built.shape).expect("encodes");

    let foreign = BrepBlob::new(
        KernelIdentity::new("occt", "0.0.0", "some other bridge").expect("valid"),
        blob.bytes().to_vec(),
    );
    let err = kernel
        .decode_shape(&foreign)
        .expect_err("a blob from another build must not be decoded");

    assert_eq!(err.kind(), ErrorKind::Kernel);
    assert!(err.to_string().contains("discard the cache"));

    kernel.release(built.shape);
}

#[test]
fn a_corrupt_or_foreign_blob_is_refused_rather_than_misread() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(10.0, 10.0, 2.0).expect("a valid rectangle");
    let built = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");
    let good = kernel.encode_shape(built.shape).expect("encodes");
    let identity = kernel.identity().clone();

    // Truncated payload.
    let mut truncated = good.bytes().to_vec();
    truncated.truncate(truncated.len() / 2);
    assert!(
        kernel
            .decode_shape(&BrepBlob::new(identity.clone(), truncated))
            .is_err()
    );

    // Same length but changed contents: framing alone cannot detect this, so
    // the payload digest must refuse it before BinTools sees another shape.
    let mut changed = good.bytes().to_vec();
    let last = changed.last_mut().expect("a B-Rep has a payload");
    *last ^= 1;
    let err = kernel
        .decode_shape(&BrepBlob::new(identity.clone(), changed))
        .expect_err("a changed payload must fail its integrity check");
    assert!(err.to_string().contains("checksum"));

    // A valid shape followed by unrelated bytes is corrupt too.
    let mut extended = good.bytes().to_vec();
    extended.push(0);
    assert!(
        kernel
            .decode_shape(&BrepBlob::new(identity.clone(), extended))
            .is_err()
    );

    // Not our framing at all.
    assert!(
        kernel
            .decode_shape(&BrepBlob::new(identity.clone(), b"not a shape".to_vec()))
            .is_err()
    );

    // Empty.
    assert!(
        kernel
            .decode_shape(&BrepBlob::new(identity.clone(), Vec::new()))
            .is_err()
    );

    // The first framing revision had no length or checksum. It must be named
    // as unsupported rather than guessed at through the new layout.
    let mut older = good.bytes().to_vec();
    older[4..8].copy_from_slice(&1u32.to_le_bytes());
    let err = kernel
        .decode_shape(&BrepBlob::new(identity.clone(), older))
        .expect_err("an older blob format must not be guessed at");
    assert_eq!(err.kind(), ErrorKind::Unsupported);

    // Our framing, a future format version this build does not write.
    let mut future = good.bytes().to_vec();
    future[4..8].copy_from_slice(&9_999u32.to_le_bytes());
    let err = kernel
        .decode_shape(&BrepBlob::new(identity, future))
        .expect_err("a newer blob format must not be guessed at");
    assert_eq!(err.kind(), ErrorKind::Unsupported);

    // None of that may have left anything behind.
    kernel.release(built.shape);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn encoding_a_released_shape_is_refused() {
    let mut kernel = kernel_or_skip!();
    let plate = rectangle(10.0, 10.0, 2.0).expect("a valid rectangle");
    let built = kernel
        .extrude(&plate.request, &OperationContext::default())
        .expect("builds");

    kernel.release(built.shape);
    let err = kernel
        .encode_shape(built.shape)
        .expect_err("a released shape cannot be encoded");
    assert_eq!(err.kind(), ErrorKind::Kernel);
}

#[test]
fn the_build_field_fingerprints_the_bridge_toolchain_and_names_the_target() {
    let kernel = kernel_or_skip!();
    let build = kernel.identity().build();

    // A digest of the bridge sources, target and C++ toolchain, not the crate
    // version: the things that compute the geometry move independently from
    // releases, and it is those which must invalidate a cached result.
    assert!(
        build.starts_with("bridge "),
        "unexpected build field {build:?}"
    );
    let parts: Vec<&str> = build.split_whitespace().collect();
    assert_eq!(
        parts.len(),
        3,
        "expected `bridge <digest> <target>`, got {build:?}"
    );
    assert_eq!(parts[1].len(), 64);
    assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()));
    assert!(
        parts[2].contains('-'),
        "expected a target triple, got {:?}",
        parts[2]
    );
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
    let err = kernel
        .transform(result.shape, &moved, &context)
        .map(|_| ())
        .expect_err("transform is not implemented");

    // Refusing is the honest answer; the alternative is a plausible wrong one
    // that nobody notices until much later.
    assert_eq!(err.kind(), ErrorKind::Unsupported);

    // Tessellation used to be in this list and is not any more; see
    // tests/tessellation_occt.rs for what it now has to get right.
    assert!(
        kernel
            .tessellate(result.shape, &TessellationParams::default(), &context)
            .is_ok()
    );

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
