// SPDX-License-Identifier: MIT
//! What any geometry kernel must do, checked against the one we can run.
//!
//! These tests are written against `dyn GeometryKernel` wherever the point is
//! the contract rather than the implementation. When the OCCT adapter arrives
//! it should be possible to run this file against it by changing which kernel
//! the constructor returns.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_kernel::{
    CancelToken, ExtrudeExtent, ExtrudeRequest, GeometryKernel, HistoryInput, KernelIdentity,
    OperationContext, PlanarPoint, Profile, ProfileLoop, ProfileSegment, ProgressSink,
    SegmentGeometry, SketchPlane, TessellationParams, extrude_cache_key, mock::MockKernel,
};
use ferritecad_types::{
    CadError, ErrorKind, Point3, Result, StableEntityId, Tolerance, Transform, Vec3,
};

/// A square profile whose segment labels the caller keeps, so history can be
/// checked against them.
struct Square {
    request: ExtrudeRequest,
    labels: Vec<StableEntityId>,
}

fn square(side: f64, height: f64) -> Result<Square> {
    let corners = [
        PlanarPoint::new(0.0, 0.0)?,
        PlanarPoint::new(side, 0.0)?,
        PlanarPoint::new(side, side)?,
        PlanarPoint::new(0.0, side)?,
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

    Ok(Square {
        request: ExtrudeRequest::new(profile, ExtrudeExtent::blind(height)?, false),
        labels,
    })
}

#[test]
fn an_extrusion_reports_a_side_face_for_every_profile_segment() {
    let square = square(10.0, 8.0).expect("a valid square");
    let mut kernel = MockKernel::new();

    let result = kernel
        .extrude(&square.request, &OperationContext::default())
        .expect("the profile is buildable");

    for label in &square.labels {
        let generated: Vec<_> = result
            .history
            .generated(HistoryInput::Segment(*label))
            .collect();
        assert_eq!(
            generated.len(),
            1,
            "segment {label} should have raised exactly one side face"
        );
    }

    // Caps correspond to no input, so they are reported apart from history.
    assert_eq!(result.start_cap.len(), 1);
    assert_eq!(result.end_cap.len(), 1);
    assert_ne!(result.start_cap[0], result.end_cap[0]);
}

#[test]
fn history_and_mesh_are_reproducible() {
    // Two independent sessions must agree about everything except the handles,
    // which are session-local by design. Anything else differing would make a
    // rebuild non-comparable across runs.
    let square = square(10.0, 8.0).expect("a valid square");
    let context = OperationContext::default();

    let mut first = MockKernel::new();
    let one = first
        .extrude(&square.request, &context)
        .expect("the profile is buildable");
    let one_mesh = first
        .tessellate(one.shape, &TessellationParams::default(), &context)
        .expect("tessellates");

    let mut second = MockKernel::new();
    let other = second
        .extrude(&square.request, &context)
        .expect("the profile is buildable");
    let other_mesh = second
        .tessellate(other.shape, &TessellationParams::default(), &context)
        .expect("tessellates");

    assert_eq!(one_mesh.positions, other_mesh.positions);
    assert_eq!(one_mesh.normals, other_mesh.normals);
    assert_eq!(one_mesh.indices, other_mesh.indices);

    // History compares by structure: same inputs, same number of outputs each.
    let one_inputs: Vec<_> = one.history.inputs().collect();
    let other_inputs: Vec<_> = other.history.inputs().collect();
    assert_eq!(one_inputs, other_inputs);
    for input in &one_inputs {
        assert_eq!(
            one.history.generated(*input).count(),
            other.history.generated(*input).count()
        );
    }
}

#[test]
fn tessellating_twice_gives_the_same_mesh() {
    let square = square(10.0, 8.0).expect("a valid square");
    let context = OperationContext::default();
    let mut kernel = MockKernel::new();

    let result = kernel.extrude(&square.request, &context).expect("builds");
    let once = kernel
        .tessellate(result.shape, &TessellationParams::default(), &context)
        .expect("tessellates");
    let twice = kernel
        .tessellate(result.shape, &TessellationParams::default(), &context)
        .expect("tessellates");

    assert_eq!(once, twice);
}

#[test]
fn every_triangle_belongs_to_exactly_one_named_face() {
    let square = square(10.0, 8.0).expect("a valid square");
    let context = OperationContext::default();
    let mut kernel = MockKernel::new();

    let result = kernel.extrude(&square.request, &context).expect("builds");
    let mesh = kernel
        .tessellate(result.shape, &TessellationParams::default(), &context)
        .expect("tessellates");

    mesh.validate().expect("the mesh is internally consistent");

    // Four sides plus two caps, and the ranges must tile the index buffer.
    assert_eq!(mesh.faces.len(), 6);
    let covered: u32 = mesh.faces.iter().map(|f| f.index_count).sum();
    assert_eq!(covered as usize, mesh.indices.len());

    // Face selection depends on this: no triangle may be claimed twice.
    let mut faces: Vec<_> = mesh.faces.iter().map(|f| f.face).collect();
    faces.sort_unstable();
    faces.dedup();
    assert_eq!(faces.len(), 6);
}

#[test]
fn cancellation_before_the_work_returns_cancelled() {
    let square = square(10.0, 8.0).expect("a valid square");
    let token = CancelToken::new();
    token.cancel();
    let context = OperationContext::default().with_cancel(token);

    let mut kernel = MockKernel::new();
    let err = kernel
        .extrude(&square.request, &context)
        .expect_err("a cancelled context must not produce geometry");

    assert_eq!(err.kind(), ErrorKind::Cancellation);
    assert!(matches!(err, CadError::Cancelled));
}

#[test]
fn cancellation_partway_through_returns_cancelled_and_stores_nothing() {
    let square = square(10.0, 8.0).expect("a valid square");
    let token = CancelToken::new();

    // Cancel the moment the operation first reports progress.
    let trigger = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ProgressSink::new(move |_| trigger.cancel()));

    let mut kernel = MockKernel::new();
    let err = kernel
        .extrude(&square.request, &context)
        .expect_err("cancelling mid-operation must abandon it");
    assert!(matches!(err, CadError::Cancelled));
}

#[test]
fn cancellation_reaches_tessellation_too() {
    let square = square(10.0, 8.0).expect("a valid square");
    let mut kernel = MockKernel::new();
    let result = kernel
        .extrude(&square.request, &OperationContext::default())
        .expect("builds");

    let token = CancelToken::new();
    token.cancel();
    let err = kernel
        .tessellate(
            result.shape,
            &TessellationParams::default(),
            &OperationContext::default().with_cancel(token),
        )
        .expect_err("a cancelled tessellation must not return a mesh");
    assert!(matches!(err, CadError::Cancelled));
}

#[test]
fn a_blob_round_trips_within_one_kernel_build() {
    let square = square(10.0, 8.0).expect("a valid square");
    let context = OperationContext::default();
    let mut kernel = MockKernel::new();

    let result = kernel.extrude(&square.request, &context).expect("builds");
    let blob = kernel.encode_shape(result.shape).expect("encodes");
    let restored = kernel.decode_shape(&blob).expect("decodes");

    let original_mesh = kernel
        .tessellate(result.shape, &TessellationParams::default(), &context)
        .expect("tessellates");
    let restored_mesh = kernel
        .tessellate(restored, &TessellationParams::default(), &context)
        .expect("tessellates");

    assert_eq!(original_mesh.positions, restored_mesh.positions);
    assert_eq!(original_mesh.indices, restored_mesh.indices);
}

#[test]
fn a_blob_from_another_kernel_version_is_refused() {
    let square = square(10.0, 8.0).expect("a valid square");
    let mut old_build = MockKernel::with_version("1.0.0");
    let result = old_build
        .extrude(&square.request, &OperationContext::default())
        .expect("builds");
    let blob = old_build.encode_shape(result.shape).expect("encodes");

    let mut new_build = MockKernel::with_version("2.0.0");
    let err = new_build
        .decode_shape(&blob)
        .expect_err("a blob from another build must not be decoded");

    assert_eq!(err.kind(), ErrorKind::Kernel);
    assert!(err.to_string().contains("discard the cache"));
}

#[test]
fn a_corrupt_blob_is_refused_rather_than_misread() {
    let square = square(10.0, 8.0).expect("a valid square");
    let mut kernel = MockKernel::new();
    let result = kernel
        .extrude(&square.request, &OperationContext::default())
        .expect("builds");
    let blob = kernel.encode_shape(result.shape).expect("encodes");

    let mut truncated = blob.bytes().to_vec();
    truncated.truncate(truncated.len() - 8);
    let damaged = ferritecad_kernel::BrepBlob::new(kernel.identity().clone(), truncated);

    assert!(kernel.decode_shape(&damaged).is_err());
}

#[test]
fn a_handle_from_another_session_is_refused() {
    let square = square(10.0, 8.0).expect("a valid square");
    let mut owner = MockKernel::new();
    let result = owner
        .extrude(&square.request, &OperationContext::default())
        .expect("builds");

    // Exactly what happens if a handle were persisted and reloaded.
    let mut stranger = MockKernel::new();
    let err = stranger
        .tessellate(
            result.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect_err("a handle does not survive its session");

    assert_eq!(err.kind(), ErrorKind::Kernel);
    assert!(err.to_string().contains("another kernel session"));
}

#[test]
fn a_released_shape_is_gone_and_releasing_again_is_harmless() {
    let square = square(10.0, 8.0).expect("a valid square");
    let mut kernel = MockKernel::new();
    let result = kernel
        .extrude(&square.request, &OperationContext::default())
        .expect("builds");

    kernel.release(result.shape);
    assert!(kernel.encode_shape(result.shape).is_err());

    // An unwinding caller releases whatever it might hold; that must be safe.
    kernel.release(result.shape);
}

#[test]
fn a_transform_preserves_every_face_as_modified() {
    let square = square(10.0, 8.0).expect("a valid square");
    let context = OperationContext::default();
    let mut kernel = MockKernel::new();

    let built = kernel.extrude(&square.request, &context).expect("builds");
    let offset =
        Transform::from_translation(Vec3::new(100.0, 0.0, 0.0).expect("finite")).expect("finite");
    let moved = kernel
        .transform(built.shape, &offset, &context)
        .expect("transforms");

    // Six faces in, six correspondences out; a transform destroys nothing.
    let inputs: Vec<_> = moved.history.inputs().collect();
    assert_eq!(inputs.len(), 6);
    for input in &inputs {
        assert_eq!(moved.history.modified(*input).count(), 1);
        assert!(!moved.history.is_deleted(*input));
    }

    let before = kernel
        .tessellate(built.shape, &TessellationParams::default(), &context)
        .expect("tessellates");
    let after = kernel
        .tessellate(moved.shape, &TessellationParams::default(), &context)
        .expect("tessellates");

    assert_eq!(before.indices, after.indices, "topology is unchanged");
    assert!(
        (after.positions[0] - before.positions[0] - 100.0).abs() < 1e-4,
        "the shape actually moved"
    );
}

#[test]
fn an_unbuildable_profile_is_refused_rather_than_approximated() {
    // Two segments cannot bound a face the mock understands.
    let a = PlanarPoint::new(0.0, 0.0).expect("finite");
    let b = PlanarPoint::new(10.0, 0.0).expect("finite");
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(vec![
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(a, b).expect("distinct"),
            ),
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(b, a).expect("distinct"),
            ),
        ])
        .expect("closes"),
        Vec::new(),
    )
    .expect("valid");

    let request = ExtrudeRequest::new(profile, ExtrudeExtent::blind(5.0).expect("positive"), false);
    let err = MockKernel::new()
        .extrude(&request, &OperationContext::default())
        .expect_err("a degenerate profile has no solid");
    assert_eq!(err.kind(), ErrorKind::Kernel);
}

#[test]
fn the_mock_refuses_holes_instead_of_silently_filling_them() {
    fn square_loop(min: f64, max: f64) -> ProfileLoop {
        let points = [
            PlanarPoint::new(min, min).expect("finite"),
            PlanarPoint::new(max, min).expect("finite"),
            PlanarPoint::new(max, max).expect("finite"),
            PlanarPoint::new(min, max).expect("finite"),
        ];
        let segments = points
            .iter()
            .enumerate()
            .map(|(index, start)| {
                ProfileSegment::new(
                    StableEntityId::new(),
                    SegmentGeometry::line(*start, points[(index + 1) % points.len()])
                        .expect("distinct"),
                )
            })
            .collect();
        ProfileLoop::new(segments).expect("closes")
    }

    let profile = Profile::new(
        SketchPlane::world_xy(),
        square_loop(0.0, 10.0),
        vec![square_loop(2.0, 4.0)],
    )
    .expect("valid profile with one hole");
    let request = ExtrudeRequest::new(profile, ExtrudeExtent::blind(5.0).expect("positive"), false);

    let err = MockKernel::new()
        .extrude(&request, &OperationContext::default())
        .expect_err("the mock must not erase the hole");
    assert_eq!(err.kind(), ErrorKind::Unsupported);
}

#[test]
fn the_cache_key_covers_the_kernel_the_tolerance_and_the_request() {
    let shorter = square(10.0, 8.0).expect("a valid square");
    let taller = square(10.0, 9.0).expect("a valid square");

    let kernel = KernelIdentity::new("mock", "1.0.0", "").expect("valid");
    let other_kernel = KernelIdentity::new("mock", "2.0.0", "").expect("valid");
    let fine = OperationContext::new(Tolerance::default());
    let coarse = OperationContext::new(Tolerance::new(1e-3, 1e-6).expect("positive"));

    let baseline = extrude_cache_key(&kernel, &shorter.request, &fine);

    assert_eq!(
        baseline,
        extrude_cache_key(&kernel, &shorter.request, &fine)
    );
    assert_ne!(
        baseline,
        extrude_cache_key(&other_kernel, &shorter.request, &fine)
    );
    assert_ne!(
        baseline,
        extrude_cache_key(&kernel, &shorter.request, &coarse)
    );
    assert_ne!(baseline, extrude_cache_key(&kernel, &taller.request, &fine));
}

#[test]
fn the_whole_slice_runs_without_naming_a_kernel_type() {
    // The contract's real test: a routine written against the trait object,
    // the way the evaluator will be, driving a build end to end.
    fn build_and_measure(kernel: &mut dyn GeometryKernel, request: &ExtrudeRequest) -> Result<f32> {
        let context = OperationContext::new(Tolerance::default());
        let result = kernel.extrude(request, &context)?;

        let blob = kernel.encode_shape(result.shape)?;
        blob.require_kernel(kernel.identity())?;

        let mesh = kernel.tessellate(result.shape, &TessellationParams::default(), &context)?;
        mesh.validate()?;

        let highest = mesh
            .positions
            .chunks_exact(3)
            .map(|p| p[2])
            .fold(f32::NEG_INFINITY, f32::max);

        kernel.release(result.shape);
        Ok(highest)
    }

    let square = square(10.0, 8.0).expect("a valid square");
    let mut kernel = MockKernel::new();
    let height = build_and_measure(&mut kernel, &square.request).expect("the mock builds");

    assert!((height - 8.0).abs() < 1e-4, "extruded to the asked height");
}

#[test]
fn a_symmetric_extrusion_straddles_its_plane() {
    let a = PlanarPoint::new(0.0, 0.0).expect("finite");
    let b = PlanarPoint::new(10.0, 0.0).expect("finite");
    let c = PlanarPoint::new(10.0, 10.0).expect("finite");
    let d = PlanarPoint::new(0.0, 10.0).expect("finite");

    let mut segments = Vec::new();
    for (start, end) in [(a, b), (b, c), (c, d), (d, a)] {
        segments.push(ProfileSegment::new(
            StableEntityId::new(),
            SegmentGeometry::line(start, end).expect("distinct"),
        ));
    }

    let plane = SketchPlane::new(Point3::ORIGIN, Vec3::X, Vec3::Z).expect("a valid frame");
    let profile = Profile::new(
        plane,
        ProfileLoop::new(segments).expect("closes"),
        Vec::new(),
    )
    .expect("valid");
    let request = ExtrudeRequest::new(
        profile,
        ExtrudeExtent::symmetric(4.0).expect("positive"),
        false,
    );

    let context = OperationContext::default();
    let mut kernel = MockKernel::new();
    let result = kernel.extrude(&request, &context).expect("builds");
    let mesh = kernel
        .tessellate(result.shape, &TessellationParams::default(), &context)
        .expect("tessellates");

    let heights: Vec<f32> = mesh.positions.chunks_exact(3).map(|p| p[2]).collect();
    let lowest = heights.iter().copied().fold(f32::INFINITY, f32::min);
    let highest = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);

    assert!((lowest + 4.0).abs() < 1e-4, "reaches four below the plane");
    assert!((highest - 4.0).abs() < 1e-4, "reaches four above the plane");
}
