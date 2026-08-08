// SPDX-License-Identifier: MIT
use ferritecad_types::{CanonicalHasher, ContentHash, Result, Transform};

use crate::context::OperationContext;
use crate::handle::{ShapeHandle, SubShapeHandle};
use crate::identity::KernelIdentity;
use crate::request::{ExtrudeRequest, TessellationParams};
use crate::result::{ArchiveSlot, BrepBlob, ExtrudeResult, Mesh, OperationResult};

/// The operations FerriteCAD needs from a geometry kernel.
///
/// A trait rather than a concrete type for one reason: the OCCT adapter cannot
/// be written or tested until OCCT is present, and everything above it can.
/// `mock::MockKernel` implements this contract with arithmetic, so the
/// evaluator, the topology layer and their tests exist independently of whether
/// a kernel is installed.
///
/// # A session is not shared
///
/// Every method takes `&mut self`, because a kernel session owns mutable state
/// — a table of live shapes — and the kernel this contract was designed for is
/// not thread-safe. Concurrency belongs above: give each worker its own
/// session, or serialise access to one. Nothing here may be called from two
/// threads at once, and the signature says so rather than leaving it to a
/// comment nobody reads.
///
/// # Errors
///
/// Every failure is a [`ferritecad_types::CadError`]. Cancellation is
/// `CadError::Cancelled` and nothing else, so a caller can tell a user's change
/// of mind from geometry that cannot be built. An adapter must never let a C++
/// exception cross into Rust: unwinding across that boundary is undefined
/// behaviour, not a rough edge.
pub trait GeometryKernel {
    /// Which kernel this is, for cache keys and error messages.
    fn identity(&self) -> &KernelIdentity;

    /// Sweeps a planar profile into a solid.
    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<ExtrudeResult>;

    /// Places a shape somewhere else.
    fn transform(
        &mut self,
        shape: ShapeHandle,
        transform: &Transform,
        context: &OperationContext,
    ) -> Result<OperationResult>;

    /// Approximates a shape with triangles.
    fn tessellate(
        &mut self,
        shape: ShapeHandle,
        params: &TessellationParams,
        context: &OperationContext,
    ) -> Result<Mesh>;

    /// Serialises a shape together with sub-shapes to be found again.
    ///
    /// Returns one [`ArchiveSlot`] per requested sub-shape, in the order given.
    /// A slot is a position inside the returned blob and means nothing without
    /// it; the caller keeps the correspondence between its own names and these
    /// slots, and the kernel never learns what those names are.
    ///
    /// This is what makes a cached shape usable by a layer that names faces.
    /// [`Self::decode_shape`] alone cannot be: a B-Rep stores a shape, not the
    /// operation that made it.
    fn encode_shape_with(
        &mut self,
        shape: ShapeHandle,
        sub_shapes: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)>;

    /// Restores a shape and the sub-shapes named by their slots.
    ///
    /// Implementations must refuse a slot outside the archive, and
    /// [`ArchiveSlot::ROOT`], which is the shape and not a sub-shape. The
    /// restored shape still carries no history; what comes back is the
    /// sub-shapes that were archived, not how they were made.
    fn decode_shape_with(
        &mut self,
        blob: &BrepBlob,
        slots: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)>;

    /// Serialises a shape into an opaque cache blob.
    fn encode_shape(&mut self, shape: ShapeHandle) -> Result<BrepBlob>;

    /// Restores a shape from a blob this same kernel build wrote.
    ///
    /// Implementations must reject a blob carrying a different
    /// [`KernelIdentity`]. A blob is cache: refusing it costs a rebuild, while
    /// decoding it under the wrong kernel costs correctness.
    fn decode_shape(&mut self, blob: &BrepBlob) -> Result<ShapeHandle>;

    /// Drops a shape the caller no longer needs.
    ///
    /// Releasing an unknown or already-released handle is not an error: a
    /// caller unwinding from a failure should be able to release everything it
    /// might hold without first working out what it actually holds.
    fn release(&mut self, shape: ShapeHandle);
}

/// The cache key for an extrusion under a given kernel and tolerance.
///
/// A free function rather than a trait method: it is the same computation for
/// every kernel, and an implementation that could vary it would be able to
/// collide two different results onto one key.
pub fn extrude_cache_key(
    kernel: &KernelIdentity,
    request: &ExtrudeRequest,
    context: &OperationContext,
) -> ContentHash {
    let mut hasher = CanonicalHasher::new("kernel.extrude");
    hasher.algorithm_version(ALGORITHM_VERSION);
    kernel.feed(&mut hasher);
    context.tolerance().feed(&mut hasher);
    request.feed(&mut hasher);
    hasher.finish()
}

/// The cache key for a tessellation of an already-keyed shape.
///
/// Takes the shape's own key rather than its handle: a handle is
/// session-local and would key the same solid differently on every run. The
/// operation tolerance is included as well because the adapter receives it and
/// may use it while preparing geometry for meshing.
pub fn tessellation_cache_key(
    kernel: &KernelIdentity,
    shape_key: &ContentHash,
    params: &TessellationParams,
    context: &OperationContext,
) -> ContentHash {
    let mut hasher = CanonicalHasher::new("kernel.tessellation");
    hasher.algorithm_version(ALGORITHM_VERSION);
    kernel.feed(&mut hasher);
    context.tolerance().feed(&mut hasher);
    hasher.field("shape").hash(shape_key);
    params.feed(&mut hasher);
    hasher.finish()
}

/// Bumped whenever the meaning of a cached result changes.
const ALGORITHM_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockKernel;
    use crate::profile::{
        PlanarPoint, Profile, ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane,
    };
    use crate::request::ExtrudeExtent;
    use ferritecad_types::{StableEntityId, Tolerance};

    fn request() -> ExtrudeRequest {
        let corners = [
            PlanarPoint::new(0.0, 0.0),
            PlanarPoint::new(10.0, 0.0),
            PlanarPoint::new(10.0, 10.0),
            PlanarPoint::new(0.0, 10.0),
        ]
        .map(|p| p.expect("finite"));

        let mut segments = Vec::new();
        for (index, start) in corners.iter().enumerate() {
            segments.push(ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(*start, corners[(index + 1) % corners.len()])
                    .expect("distinct"),
            ));
        }

        let profile = Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(segments).expect("closes"),
            Vec::new(),
        )
        .expect("valid");

        ExtrudeRequest::new(profile, ExtrudeExtent::blind(8.0).expect("positive"), false)
    }

    #[test]
    fn the_kernel_identity_changes_the_extrude_key() {
        let request = request();
        let context = OperationContext::default();

        let one = KernelIdentity::new("occt", "8.0.0", "").expect("valid");
        let other = KernelIdentity::new("occt", "8.0.1", "").expect("valid");

        assert_ne!(
            extrude_cache_key(&one, &request, &context),
            extrude_cache_key(&other, &request, &context)
        );
    }

    #[test]
    fn the_tolerance_changes_the_extrude_key() {
        let request = request();
        let kernel = KernelIdentity::new("occt", "8.0.1", "").expect("valid");

        let fine = OperationContext::new(Tolerance::default());
        let coarse = OperationContext::new(Tolerance::new(1e-3, 1e-6).expect("positive"));

        assert_ne!(
            extrude_cache_key(&kernel, &request, &fine),
            extrude_cache_key(&kernel, &request, &coarse)
        );
    }

    #[test]
    fn the_same_request_keys_the_same_way() {
        let request = request();
        let kernel = KernelIdentity::new("occt", "8.0.1", "").expect("valid");
        let context = OperationContext::default();

        assert_eq!(
            extrude_cache_key(&kernel, &request, &context),
            extrude_cache_key(&kernel, &request, &context)
        );
    }

    #[test]
    fn cancellation_does_not_change_the_key() {
        // A cancelled attempt and a completed one describe the same result;
        // only one of them produced it.
        let request = request();
        let kernel = KernelIdentity::new("occt", "8.0.1", "").expect("valid");
        let token = crate::context::CancelToken::new();
        let context = OperationContext::default().with_cancel(token.clone());
        let before = extrude_cache_key(&kernel, &request, &context);

        token.cancel();
        assert_eq!(extrude_cache_key(&kernel, &request, &context), before);
    }

    #[test]
    fn tessellation_parameters_change_the_mesh_key() {
        let kernel = KernelIdentity::new("occt", "8.0.1", "").expect("valid");
        let shape_key = ContentHash::of_bytes(b"a solid");

        let fine = TessellationParams::default();
        let coarse = TessellationParams::new(0.5, 0.5, false).expect("positive");

        assert_ne!(
            tessellation_cache_key(&kernel, &shape_key, &fine, &OperationContext::default()),
            tessellation_cache_key(&kernel, &shape_key, &coarse, &OperationContext::default())
        );
    }

    #[test]
    fn the_tolerance_changes_the_mesh_key() {
        let kernel = KernelIdentity::new("occt", "8.0.1", "").expect("valid");
        let shape_key = ContentHash::of_bytes(b"a solid");
        let params = TessellationParams::default();
        let fine = OperationContext::new(Tolerance::default());
        let coarse = OperationContext::new(Tolerance::new(1e-3, 1e-6).expect("positive"));

        assert_ne!(
            tessellation_cache_key(&kernel, &shape_key, &params, &fine),
            tessellation_cache_key(&kernel, &shape_key, &params, &coarse)
        );
    }

    #[test]
    fn a_different_shape_changes_the_mesh_key() {
        let kernel = KernelIdentity::new("occt", "8.0.1", "").expect("valid");
        let params = TessellationParams::default();

        assert_ne!(
            tessellation_cache_key(
                &kernel,
                &ContentHash::of_bytes(b"one"),
                &params,
                &OperationContext::default()
            ),
            tessellation_cache_key(
                &kernel,
                &ContentHash::of_bytes(b"other"),
                &params,
                &OperationContext::default()
            )
        );
    }

    #[test]
    fn the_contract_is_usable_without_naming_an_implementation() {
        // The point of the trait: this function knows no kernel type, and the
        // evaluator will be written the same way.
        fn build(kernel: &mut dyn GeometryKernel, request: &ExtrudeRequest) -> Result<usize> {
            let context = OperationContext::default();
            let result = kernel.extrude(request, &context)?;
            let mesh = kernel.tessellate(result.shape, &TessellationParams::default(), &context)?;
            mesh.validate()?;
            kernel.release(result.shape);
            Ok(mesh.triangle_count())
        }

        let mut kernel = MockKernel::new();
        assert!(build(&mut kernel, &request()).expect("the mock builds") > 0);
    }
}
