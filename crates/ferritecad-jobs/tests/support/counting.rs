// SPDX-License-Identifier: MIT
//! A topology-only kernel double: count work without measuring geometry or time.
use ferritecad_kernel::*;
use ferritecad_types::{CadError, Result, Transform};
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
    topology: BTreeMap<ShapeHandle, ShapeTopology>,
    histories: BTreeMap<ShapeHandle, NamingHistory>,
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
            topology: BTreeMap::new(),
            histories: BTreeMap::new(),
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
    fn sweep(&mut self, profile: &Profile, operation: NamingOperation) -> OperationResult {
        let shape = self.shape();
        let mut topology = ShapeTopology::default();
        let mut history = NamingHistory {
            operation: Some(operation),
            ..Default::default()
        };
        let caps = [Self::face(shape, 1000), Self::face(shape, 1001)];
        for (end, cap) in [false, true].into_iter().zip(caps) {
            topology.adjacent_faces.insert(cap, BTreeSet::new());
            history.relations.insert((
                NamingInput::Cap { end },
                NamingRelation::Generated,
                Some(cap),
            ));
        }
        let mut offset = 0;
        for ring in std::iter::once(profile.outer()).chain(profile.inner()) {
            let segments = ring.segments();
            let n = segments.len();
            for (i, segment) in segments.iter().enumerate() {
                let side = Self::face(shape, (offset + i) as u64);
                let previous = Self::face(shape, (offset + (i + n - 1) % n) as u64);
                topology.adjacent_faces.insert(side, BTreeSet::new());
                history.relations.insert((
                    NamingInput::Curve(segment.label),
                    NamingRelation::Generated,
                    Some(side),
                ));
                for (end, cap) in [false, true].into_iter().zip(caps) {
                    let edge = SubShapeHandle::new(
                        shape,
                        SubShapeKind::Edge,
                        2000 + 2 * (offset + i) as u64 + u64::from(end),
                    );
                    topology
                        .adjacent_faces
                        .insert(edge, BTreeSet::from([side, cap]));
                    history.relations.insert((
                        NamingInput::Curve(segment.label),
                        NamingRelation::Generated,
                        Some(edge),
                    ));
                }
                if !matches!(segment.geometry, SegmentGeometry::Circle { .. }) {
                    let edge =
                        SubShapeHandle::new(shape, SubShapeKind::Edge, 4000 + (offset + i) as u64);
                    topology
                        .adjacent_faces
                        .insert(edge, BTreeSet::from([previous, side]));
                    for input in [
                        NamingInput::CurveEnd {
                            curve: segment.label,
                            end: false,
                        },
                        NamingInput::CurveEnd {
                            curve: segments[(i + n - 1) % n].label,
                            end: true,
                        },
                    ] {
                        history
                            .relations
                            .insert((input, NamingRelation::Generated, Some(edge)));
                        for (end, cap) in [false, true].into_iter().zip(caps) {
                            let vertex = SubShapeHandle::new(
                                shape,
                                SubShapeKind::Vertex,
                                6000 + 2 * (offset + i) as u64 + u64::from(end),
                            );
                            topology
                                .adjacent_faces
                                .insert(vertex, BTreeSet::from([previous, side, cap]));
                            history.relations.insert((
                                input,
                                NamingRelation::Generated,
                                Some(vertex),
                            ));
                        }
                    }
                }
            }
            offset += n;
        }
        self.topology.insert(shape, topology);
        self.histories.insert(shape, history);
        OperationResult {
            shape,
            removed_volume: 0.,
        }
    }
    fn carry(
        &mut self,
        inputs: &[ShapeHandle],
        deleted: Option<SubShapeHandle>,
        operation: NamingOperation,
    ) -> OperationResult {
        let shape = self.shape();
        let mut topology = ShapeTopology::default();
        let mut history = NamingHistory {
            operation: Some(operation),
            ..Default::default()
        };
        let output = |s: SubShapeHandle| {
            SubShapeHandle::new(shape, s.kind(), s.shape().index() * 10000 + s.index())
        };
        for input in inputs {
            for (sub, faces) in &self.topology[input].adjacent_faces {
                if Some(*sub) == deleted {
                    history.relations.insert((
                        NamingInput::SubShape(*sub),
                        NamingRelation::Deleted,
                        None,
                    ));
                } else {
                    let out = output(*sub);
                    topology
                        .adjacent_faces
                        .insert(out, faces.iter().copied().map(output).collect());
                    history.relations.insert((
                        NamingInput::SubShape(*sub),
                        NamingRelation::Modified,
                        Some(out),
                    ));
                }
            }
        }
        if let Some(edge) = deleted {
            let face = Self::face(shape, 9999);
            topology.adjacent_faces.insert(face, BTreeSet::new());
            history.relations.insert((
                NamingInput::SubShape(edge),
                NamingRelation::Generated,
                Some(face),
            ));
        }
        self.topology.insert(shape, topology);
        self.histories.insert(shape, history);
        OperationResult {
            shape,
            removed_volume: 1.,
        }
    }
}
impl GeometryKernel for CountingMock {
    fn projected_bounds(
        &mut self,
        shape: ShapeHandle,
        direction: ferritecad_types::Vec3,
    ) -> Result<[f64; 2]> {
        if !self.live.contains(&shape) {
            return Err(CadError::input("unknown counting shape"));
        }
        if direction.x == 0. && direction.y == 0. && direction.z == 0. {
            return Err(CadError::input("a projection needs a direction"));
        }
        // This topology-only double needs finite reach, not a geometric measurement.
        Ok([0., 10.])
    }
    fn identity(&self) -> &KernelIdentity {
        &self.identity
    }
    fn shape_topology(&mut self, shape: ShapeHandle) -> Result<ShapeTopology> {
        self.topology
            .get(&shape)
            .cloned()
            .ok_or_else(|| CadError::kernel("missing test topology"))
    }
    fn naming_history(&mut self, shape: ShapeHandle) -> Result<NamingHistory> {
        self.histories
            .get(&shape)
            .cloned()
            .ok_or_else(|| CadError::kernel("missing test history"))
    }
    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        self.extrude_count += 1;
        Ok(self.sweep(request.profile(), NamingOperation::Extrude))
    }
    fn revolve(
        &mut self,
        request: &RevolveRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        self.revolve_count += 1;
        Ok(self.sweep(request.profile(), NamingOperation::Revolve))
    }
    fn boolean(
        &mut self,
        request: &BooleanRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        Ok(self.carry(
            &[request.target(), request.tool()],
            None,
            NamingOperation::Cut,
        ))
    }
    fn fillet_edge(
        &mut self,
        request: &FilletRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        Ok(self.carry(
            &[request.target()],
            Some(request.edge()),
            NamingOperation::Fillet,
        ))
    }
    fn chamfer_edge(
        &mut self,
        request: &ChamferRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        Ok(self.carry(
            &[request.target()],
            Some(request.edge()),
            NamingOperation::Chamfer,
        ))
    }
    fn transform(
        &mut self,
        source: ShapeHandle,
        _: &Transform,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        Ok(self.carry(&[source], None, NamingOperation::Extrude))
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
        self.topology.remove(&shape);
        self.histories.remove(&shape);
    }
}
