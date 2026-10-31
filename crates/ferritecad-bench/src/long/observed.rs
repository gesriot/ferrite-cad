// SPDX-License-Identifier: MIT
use std::time::Instant;

use ferritecad_kernel::*;
use ferritecad_occt::OcctKernel;
use ferritecad_types::{Result, Transform};

pub(super) struct Observed {
    kernel: OcctKernel,
    phase: &'static str,
    count: usize,
    next: usize,
    pub build_ms: f64,
}

impl Observed {
    pub fn new(phase: &'static str, count: usize) -> Result<Self> {
        Ok(Self {
            kernel: OcctKernel::new()?,
            phase,
            count,
            next: 0,
            build_ms: 0.,
        })
    }

    fn progress(&mut self) {
        eprintln!("phase={} feature={}/{}", self.phase, self.next, self.count);
        self.next += 1;
    }
}

macro_rules! forward {
    ($name:ident($($argument:ident: $type:ty),*) -> $result:ty) => {
        fn $name(&mut self, $($argument: $type),*) -> $result {
            self.kernel.$name($($argument),*)
        }
    };
}

impl GeometryKernel for Observed {
    fn identity(&self) -> &KernelIdentity {
        self.kernel.identity()
    }

    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.progress();
        let start = Instant::now();
        let result = self.kernel.extrude(request, context);
        self.build_ms += super::ms(start);
        result
    }

    fn boolean(
        &mut self,
        request: &BooleanRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        let start = Instant::now();
        let result = self.kernel.boolean(request, context);
        self.build_ms += super::ms(start);
        result
    }

    fn decode_shape_with(
        &mut self,
        blob: &BrepBlob,
        slots: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        self.progress();
        self.kernel.decode_shape_with(blob, slots)
    }

    forward!(shape_topology(shape: ShapeHandle) -> Result<ShapeTopology>);
    forward!(naming_history(shape: ShapeHandle) -> Result<NamingHistory>);
    forward!(solid_count(shape: ShapeHandle) -> Result<usize>);
    forward!(validate_solids(shape: ShapeHandle) -> Result<()>);
    forward!(transform(shape: ShapeHandle, transform: &Transform, context: &OperationContext)
        -> Result<OperationResult>);
    forward!(tessellate(shape: ShapeHandle, params: &TessellationParams, context: &OperationContext)
        -> Result<Mesh>);
    forward!(encode_shape_with(shape: ShapeHandle, sub_shapes: &[SubShapeHandle])
        -> Result<(BrepBlob, Vec<ArchiveSlot>)>);
    forward!(encode_shape(shape: ShapeHandle) -> Result<BrepBlob>);
    forward!(decode_shape(blob: &BrepBlob) -> Result<ShapeHandle>);
    forward!(release(shape: ShapeHandle) -> ());
}
