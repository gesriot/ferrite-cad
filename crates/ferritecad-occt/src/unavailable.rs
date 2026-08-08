// SPDX-License-Identifier: MIT
//! What this crate is when Open CASCADE was not available at build time.
//!
//! The type exists so callers compile either way, and cannot be constructed so
//! nothing can accidentally believe it computes geometry.

use std::convert::Infallible;

use ferritecad_kernel::{
    ArchiveSlot, BrepBlob, ExtrudeRequest, ExtrudeResult, GeometryKernel, KernelIdentity, Mesh,
    OperationContext, OperationResult, ShapeHandle, SubShapeHandle, TessellationParams,
};
use ferritecad_types::{CadError, Result, Transform};

/// A kernel that cannot be opened, because this binary has no Open CASCADE.
///
/// Uninhabited on purpose: [`OcctKernel::new`] is the only constructor and it
/// always fails, so every method below is unreachable by construction rather
/// than by convention.
#[derive(Debug)]
pub struct OcctKernel(Infallible);

impl OcctKernel {
    /// Always fails, explaining what to do about it.
    pub fn new() -> Result<Self> {
        Err(CadError::unsupported(
            "this build has no Open CASCADE: the bridge could not be compiled. Install Open \
             CASCADE and rebuild, or set FERRITECAD_REQUIRE_OCCT=1 to make its absence a build \
             failure instead of a warning.",
        ))
    }

    pub fn live_shape_count(&self) -> usize {
        match self.0 {}
    }

    pub fn shape_stats(&mut self, _shape: ShapeHandle) -> Result<(u64, f64)> {
        match self.0 {}
    }
}

impl GeometryKernel for OcctKernel {
    fn identity(&self) -> &KernelIdentity {
        match self.0 {}
    }

    fn extrude(
        &mut self,
        _request: &ExtrudeRequest,
        _context: &OperationContext,
    ) -> Result<ExtrudeResult> {
        match self.0 {}
    }

    fn transform(
        &mut self,
        _shape: ShapeHandle,
        _transform: &Transform,
        _context: &OperationContext,
    ) -> Result<OperationResult> {
        match self.0 {}
    }

    fn tessellate(
        &mut self,
        _shape: ShapeHandle,
        _params: &TessellationParams,
        _context: &OperationContext,
    ) -> Result<Mesh> {
        match self.0 {}
    }

    fn encode_shape_with(
        &mut self,
        _shape: ShapeHandle,
        _sub_shapes: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
        match self.0 {}
    }

    fn decode_shape_with(
        &mut self,
        _blob: &BrepBlob,
        _slots: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        match self.0 {}
    }

    fn encode_shape(&mut self, _shape: ShapeHandle) -> Result<BrepBlob> {
        match self.0 {}
    }

    fn decode_shape(&mut self, _blob: &BrepBlob) -> Result<ShapeHandle> {
        match self.0 {}
    }

    fn release(&mut self, _shape: ShapeHandle) {
        match self.0 {}
    }
}
