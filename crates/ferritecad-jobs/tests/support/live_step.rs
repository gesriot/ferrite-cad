// SPDX-License-Identifier: MIT
use ferritecad_kernel::*;
use ferritecad_types::{Result, Transform};

/// Borrow the test's kernel while the scoped live evaluator owns its revisions.
struct Borrowed<'a, K: ?Sized>(&'a mut K);

impl<K: GeometryKernel + ?Sized> GeometryKernel for Borrowed<'_, K> {
    fn identity(&self) -> &KernelIdentity {
        self.0.identity()
    }
    fn shape_topology(&mut self, shape: ShapeHandle) -> Result<ferritecad_kernel::ShapeTopology> {
        self.0.shape_topology(shape)
    }
    fn solid_count(&mut self, shape: ShapeHandle) -> Result<usize> {
        self.0.solid_count(shape)
    }
    fn validate_solid_structure(&mut self, shape: ShapeHandle) -> Result<()> {
        self.0.validate_solid_structure(shape)
    }
    fn validate_solids(&mut self, shape: ShapeHandle) -> Result<()> {
        self.0.validate_solids(shape)
    }
    fn keep_solids(
        &mut self,
        target: ShapeHandle,
        witnesses: &[ferritecad_kernel::SolidWitness],
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.0.keep_solids(target, witnesses, context)
    }
    fn projected_bounds(
        &mut self,
        shape: ShapeHandle,
        direction: ferritecad_types::Vec3,
    ) -> Result<[f64; 2]> {
        self.0.projected_bounds(shape, direction)
    }
    fn support_plane(
        &mut self,
        faces: &[SubShapeHandle],
        context: &OperationContext,
    ) -> Result<ferritecad_kernel::FacePlane> {
        self.0.support_plane(faces, context)
    }
    fn naming_history(&mut self, shape: ShapeHandle) -> Result<ferritecad_kernel::NamingHistory> {
        self.0.naming_history(shape)
    }
    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.0.extrude(request, context)
    }
    fn vertex_point(&mut self, vertex: SubShapeHandle) -> Result<ferritecad_types::Point3> {
        self.0.vertex_point(vertex)
    }
    fn extrude_to_face(
        &mut self,
        profile: &ferritecad_kernel::Profile,
        reversed: bool,
        faces: &[SubShapeHandle],
        offset_mm: f64,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.0
            .extrude_to_face(profile, reversed, faces, offset_mm, context)
    }
    fn extrude_to_next(
        &mut self,
        _profile: &ferritecad_kernel::Profile,
        _reversed: bool,
        _target: ShapeHandle,
        _context: &OperationContext,
    ) -> Result<OperationResult> {
        self.0
            .extrude_to_next(_profile, _reversed, _target, _context)
    }
    fn revolve(
        &mut self,
        request: &RevolveRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.0.revolve(request, context)
    }
    fn boolean(
        &mut self,
        request: &BooleanRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.0.boolean(request, context)
    }
    fn fillet_edges(
        &mut self,
        request: &FilletRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.0.fillet_edges(request, context)
    }
    fn chamfer_edges(
        &mut self,
        request: &ChamferRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.0.chamfer_edges(request, context)
    }
    fn transform(
        &mut self,
        shape: ShapeHandle,
        transform: &Transform,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.0.transform(shape, transform, context)
    }
    fn tessellate(
        &mut self,
        shape: ShapeHandle,
        params: &TessellationParams,
        context: &OperationContext,
    ) -> Result<Mesh> {
        self.0.tessellate(shape, params, context)
    }
    fn encode_shape_with(
        &mut self,
        shape: ShapeHandle,
        sub_shapes: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
        self.0.encode_shape_with(shape, sub_shapes)
    }
    fn decode_shape_with(
        &mut self,
        blob: &BrepBlob,
        slots: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        self.0.decode_shape_with(blob, slots)
    }
    fn encode_shape(&mut self, shape: ShapeHandle) -> Result<BrepBlob> {
        self.0.encode_shape(shape)
    }
    fn decode_shape(&mut self, blob: &BrepBlob) -> Result<ShapeHandle> {
        self.0.decode_shape(blob)
    }
    fn release(&mut self, shape: ShapeHandle) {
        self.0.release(shape)
    }
}

pub fn prepare<K: GeometryKernel + ?Sized>(
    ticket: ferritecad_jobs::StepTicket,
    changes: Vec<ferritecad_document::EditChange>,
    expected: ferritecad_document::DocumentVersion,
    kernel: &mut K,
    mut read_step: impl FnMut(&mut K, &[u8]) -> Result<ferritecad_exchange::Import>,
    context: &OperationContext,
) -> Result<ferritecad_jobs::ProducedStep> {
    let (mut evaluator, _) = ferritecad_jobs::LiveEvaluator::open(
        ticket.source(),
        Borrowed(kernel),
        |kernel: &mut Borrowed<'_, K>, bytes| read_step(kernel.0, bytes),
        TessellationParams::default(),
        context,
    )?;
    evaluator.prepare_edit(ticket, expected, changes, context)
}
