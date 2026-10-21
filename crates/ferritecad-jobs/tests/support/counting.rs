// SPDX-License-Identifier: MIT
//! A topology-only kernel double: count work without measuring geometry or time.
use ferritecad_kernel::*;
use ferritecad_types::{CadError, ProfileJoint, Result, Transform};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct CountingMock {
    identity: KernelIdentity,
    session: SessionId,
    next: u64,
    pub live: BTreeSet<ShapeHandle>,
    pub extrude_count: usize,
    pub revolve_count: usize,
    pub refuse_mesh: bool,
}
impl CountingMock {
    pub fn new() -> Self {
        Self {
            identity: KernelIdentity::new("counting", "1", "test").expect("identity"),
            session: SessionId::new(),
            next: 0,
            live: BTreeSet::new(),
            extrude_count: 0,
            revolve_count: 0,
            refuse_mesh: false,
        }
    }
    fn shape(&mut self) -> ShapeHandle {
        self.next += 1;
        let shape = ShapeHandle::new(self.session, self.next);
        self.live.insert(shape);
        shape
    }
    fn face(shape: ShapeHandle, index: u64) -> SubShapeHandle {
        SubShapeHandle::new(shape, SubShapeKind::Face, index)
    }
    fn carry(
        &mut self,
        track: &[SubShapeHandle],
        deleted: Option<SubShapeHandle>,
    ) -> (
        ShapeHandle,
        History,
        BTreeMap<SubShapeHandle, CarriedOutcome>,
    ) {
        let shape = self.shape();
        let mut history = History::new();
        let mut carried = BTreeMap::new();
        for sub in track {
            let input = HistoryInput::SubShape(*sub);
            if Some(*sub) == deleted {
                history.record_deleted(input);
                carried.insert(*sub, CarriedOutcome::Deleted);
            } else {
                let output = SubShapeHandle::new(
                    shape,
                    sub.kind(),
                    sub.shape().index() * 10000 + sub.index(),
                );
                history.record_modified(input, output);
                carried.insert(*sub, CarriedOutcome::Modified);
            }
        }
        (shape, history, carried)
    }
}
impl GeometryKernel for CountingMock {
    fn identity(&self) -> &KernelIdentity {
        &self.identity
    }
    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<ExtrudeResult> {
        context.check_cancelled()?;
        self.extrude_count += 1;
        let shape = self.shape();
        let mut history = History::new();
        for (i, segment) in request.profile().segments().enumerate() {
            history.record_generated(
                HistoryInput::Segment(segment.label),
                Self::face(shape, i as u64),
            );
        }
        let mut sweep_edges = BTreeMap::new();
        let segments = request.profile().outer().segments();
        if segments.len() > 2 {
            for (i, segment) in segments.iter().enumerate() {
                let previous = segments[(i + segments.len() - 1) % segments.len()].label;
                sweep_edges.insert(
                    ProfileJoint::new(previous, segment.label)?,
                    SubShapeHandle::new(shape, SubShapeKind::Edge, i as u64),
                );
            }
        }
        Ok(ExtrudeResult {
            shape,
            history,
            start_cap: vec![Self::face(shape, 1000)],
            end_cap: vec![Self::face(shape, 1001)],
            start_cap_edges: BTreeMap::new(),
            end_cap_edges: BTreeMap::new(),
            sweep_edges,
            start_cap_vertices: BTreeMap::new(),
            end_cap_vertices: BTreeMap::new(),
        })
    }
    fn revolve(
        &mut self,
        request: &RevolveRequest,
        context: &OperationContext,
    ) -> Result<RevolveResult> {
        context.check_cancelled()?;
        self.revolve_count += 1;
        let shape = self.shape();
        let mut history = History::new();
        for (i, segment) in request.profile().segments().enumerate() {
            history.record_generated(
                HistoryInput::Segment(segment.label),
                Self::face(shape, i as u64),
            );
        }
        Ok(RevolveResult {
            shape,
            history,
            start_cap: vec![Self::face(shape, 1000)],
            end_cap: vec![Self::face(shape, 1001)],
        })
    }
    fn cut(
        &mut self,
        _: &CutRequest,
        track: &[SubShapeHandle],
        _: &OperationContext,
    ) -> Result<CutResult> {
        let (shape, history, carried) = self.carry(track, None);
        Ok(CutResult {
            shape,
            history,
            carried,
            removed_volume: 1.0,
        })
    }
    fn fillet_edge(
        &mut self,
        request: &FilletRequest,
        track: &[SubShapeHandle],
        _: &OperationContext,
    ) -> Result<FilletResult> {
        let (shape, history, carried) = self.carry(track, Some(request.edge()));
        Ok(FilletResult {
            shape,
            history,
            carried,
            fillet_faces: vec![Self::face(shape, 9999)],
            removed_volume: 1.0,
        })
    }
    fn chamfer_edge(
        &mut self,
        request: &ChamferRequest,
        track: &[SubShapeHandle],
        _: &OperationContext,
    ) -> Result<ChamferResult> {
        let (shape, history, carried) = self.carry(track, Some(request.edge()));
        Ok(ChamferResult {
            shape,
            history,
            carried,
            chamfer_faces: vec![Self::face(shape, 9999)],
            removed_volume: 1.0,
        })
    }
    fn transform(
        &mut self,
        _: ShapeHandle,
        _: &Transform,
        _: &OperationContext,
    ) -> Result<OperationResult> {
        Ok(OperationResult {
            shape: self.shape(),
            history: History::new(),
        })
    }
    fn tessellate(
        &mut self,
        shape: ShapeHandle,
        _: &TessellationParams,
        context: &OperationContext,
    ) -> Result<Mesh> {
        context.check_cancelled()?;
        if self.refuse_mesh {
            return Err(CadError::kernel("requested mesh failure"));
        }
        Ok(Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            faces: vec![MeshFaceRange {
                face: Self::face(shape, 1000),
                first_index: 0,
                index_count: 3,
            }],
            indices: vec![0, 1, 2],
            ..Mesh::default()
        })
    }
    fn encode_shape_with(
        &mut self,
        _: ShapeHandle,
        _: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
        Err(CadError::unsupported("no cache"))
    }
    fn decode_shape_with(
        &mut self,
        _: &BrepBlob,
        _: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        Err(CadError::unsupported("no cache"))
    }
    fn encode_shape(&mut self, _: ShapeHandle) -> Result<BrepBlob> {
        Err(CadError::unsupported("no cache"))
    }
    fn decode_shape(&mut self, _: &BrepBlob) -> Result<ShapeHandle> {
        Err(CadError::unsupported("no cache"))
    }
    fn release(&mut self, shape: ShapeHandle) {
        self.live.remove(&shape);
    }
}
