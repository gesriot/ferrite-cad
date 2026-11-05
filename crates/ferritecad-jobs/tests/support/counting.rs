// SPDX-License-Identifier: MIT
//! A topology-only kernel double: count work without measuring geometry or time.
use ferritecad_kernel::*;
use ferritecad_types::{CadError, Result, Transform};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

#[derive(Debug)]
pub struct TopologyMock {
    identity: KernelIdentity,
    session: SessionId,
    next: u64,
    pub live: BTreeSet<ShapeHandle>,
    pub refuse_mesh: bool,
    topology: BTreeMap<ShapeHandle, ShapeTopology>,
    histories: BTreeMap<ShapeHandle, NamingHistory>,
}
impl TopologyMock {
    pub fn new() -> Self {
        Self {
            identity: KernelIdentity::new("counting", "1", "test").expect("identity"),
            session: SessionId::new(),
            next: 0,
            live: BTreeSet::new(),
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
        deleted: &[SubShapeHandle],
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
                if deleted.contains(sub) {
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
        for (index, edge) in deleted.iter().enumerate() {
            let face = Self::face(shape, 9999 + index as u64);
            topology.adjacent_faces.insert(face, BTreeSet::new());
            history.relations.insert((
                NamingInput::SubShape(*edge),
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
impl GeometryKernel for TopologyMock {
    fn solid_count(&mut self, shape: ShapeHandle) -> Result<usize> {
        if !self.live.contains(&shape) {
            return Err(CadError::input("unknown counting shape"));
        }
        // This work-counting double represents each live shape as one solid.
        Ok(1)
    }

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
        Ok(self.sweep(request.profile(), NamingOperation::Extrude))
    }
    fn revolve(
        &mut self,
        request: &RevolveRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
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
            &[],
            NamingOperation::Cut,
        ))
    }
    fn fillet_edges(
        &mut self,
        request: &FilletRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        Ok(self.carry(
            &[request.target()],
            request.edges(),
            NamingOperation::Fillet,
        ))
    }
    fn chamfer_edges(
        &mut self,
        request: &ChamferRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        Ok(self.carry(
            &[request.target()],
            request.edges(),
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
        Ok(self.carry(&[source], &[], NamingOperation::Extrude))
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

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Calls {
    pub extrude_count: usize,
    pub boolean: usize,
    pub fillet: usize,
    pub chamfer: usize,
    pub revolve_count: usize,
    pub restore: usize,
    pub tessellate: usize,
    pub queries: usize,
    pub identity: usize,
    pub released: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Extrude,
    BetweenNodes,
    Mesh,
}

impl Occupancy for mock::MockKernel {
    fn population(&self) -> usize {
        self.live_shape_count()
    }
}

#[derive(Debug, Default)]
pub struct Probe {
    pub calls: Calls,
    pub live: usize,
    pub closed: bool,
    pub fail: Option<(&'static str, usize)>,
    pub panic: Option<(&'static str, usize)>,
}

pub type SharedProbe = std::sync::Arc<std::sync::Mutex<Probe>>;

pub trait Occupancy {
    fn population(&self) -> usize;
}
impl Occupancy for TopologyMock {
    fn population(&self) -> usize {
        self.live.len()
    }
}
impl Occupancy for ferritecad_occt::OcctKernel {
    fn population(&self) -> usize {
        self.live_shape_count()
    }
}

pub type CountingMock = Counting<TopologyMock>;

#[derive(Debug)]
pub struct Counting<K> {
    pub inner: K,
    pub probe: Option<SharedProbe>,
    pub calls: Cell<Calls>,
    pub stop: Option<Stop>,
    pub remaining: Rc<Cell<usize>>,
    cancel: Option<CancelToken>,
    pub extrude_count: usize,
    pub revolve_count: usize,
}

impl CountingMock {
    pub fn new() -> Self {
        Self::wrap(TopologyMock::new())
    }
}

impl<K> std::ops::Deref for Counting<K> {
    type Target = K;
    fn deref(&self) -> &K {
        &self.inner
    }
}
impl<K> std::ops::DerefMut for Counting<K> {
    fn deref_mut(&mut self) -> &mut K {
        &mut self.inner
    }
}

impl<K> Counting<K> {
    pub fn wrap(inner: K) -> Self {
        Self {
            inner,
            probe: None,
            calls: Cell::default(),
            stop: None,
            remaining: Rc::default(),
            cancel: None,
            extrude_count: 0,
            revolve_count: 0,
        }
    }

    // A failed assertion may poison the probe. Cleanup still records every release.
    fn count(&self, f: impl FnOnce(&mut Calls)) {
        let mut counts = self.calls.get();
        f(&mut counts);
        self.calls.set(counts);
        if let Some(probe) = &self.probe {
            probe
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .calls = counts;
        }
    }
}

impl<K: Occupancy> Counting<K> {
    fn published(&self) {
        if let Some(probe) = &self.probe {
            probe
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .live = self.inner.population();
        }
    }

    fn fault(&self, name: &str, count: usize) -> Result<()> {
        let (panic, fail) = self
            .probe
            .as_ref()
            .map(|probe| {
                let probe = probe
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                (
                    probe.panic == Some((name, count)),
                    probe.fail == Some((name, count)),
                )
            })
            .unwrap_or_default();
        assert!(!panic, "requested kernel panic");
        if fail {
            return Err(CadError::kernel("requested kernel failure"));
        }
        Ok(())
    }
}

impl<K> Drop for Counting<K> {
    fn drop(&mut self) {
        if let Some(probe) = &self.probe {
            let mut probe = probe
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            probe.live = 0;
            probe.closed = true;
        }
    }
}

macro_rules! forward {
    ($count:ident, $name:ident($($arg:ident: $ty:ty),*) -> $out:ty) => {
        fn $name(&mut self,$($arg:$ty),*) -> $out {
            self.count(|c| c.$count += 1);
            self.fault(stringify!($count), self.calls.get().$count)?;
            let result = self.inner.$name($($arg),*);
            self.published();
            result
        }
    };
}

impl<K: GeometryKernel + Occupancy> GeometryKernel for Counting<K> {
    fn identity(&self) -> &KernelIdentity {
        self.count(|c| c.identity += 1);
        self.inner.identity()
    }
    fn extrude(&mut self, r: &ExtrudeRequest, ctx: &OperationContext) -> Result<OperationResult> {
        self.extrude_count += 1;
        self.count(|c| c.extrude_count += 1);
        if self.stop == Some(Stop::Extrude) {
            let cancel = ctx.cancel().clone();
            let inner = ctx
                .clone()
                .with_progress(ProgressSink::new(move |_| cancel.cancel()));
            return self.inner.extrude(r, &inner);
        }
        self.fault("extrude_count", self.calls.get().extrude_count)?;
        let result = self.inner.extrude(r, ctx);
        self.published();
        result
    }
    fn tessellate(
        &mut self,
        s: ShapeHandle,
        p: &TessellationParams,
        ctx: &OperationContext,
    ) -> Result<Mesh> {
        self.count(|c| c.tessellate += 1);
        self.fault("tessellate", self.calls.get().tessellate)?;
        let mesh = self.inner.tessellate(s, p, ctx)?;
        if self.stop == Some(Stop::Mesh) {
            // The mesher polls the token but does not emit intermediate progress.
            // Cancel at the return boundary of a successful native mesh.
            ctx.cancel().cancel();
        }
        Ok(mesh)
    }
    fn naming_history(&mut self, s: ShapeHandle) -> Result<NamingHistory> {
        self.count(|c| c.queries += 1);
        let history = self.inner.naming_history(s)?;
        if let Some(token) = self.cancel.take() {
            token.cancel();
        }
        Ok(history)
    }
    fn boolean(&mut self, r: &BooleanRequest, ctx: &OperationContext) -> Result<OperationResult> {
        self.count(|c| c.boolean += 1);
        if self.stop == Some(Stop::BetweenNodes) {
            self.cancel = Some(ctx.cancel().clone());
        }
        self.fault("boolean", self.calls.get().boolean)?;
        let result = self.inner.boolean(r, ctx);
        self.published();
        result
    }
    fn revolve(
        &mut self,
        request: &RevolveRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.revolve_count += 1;
        self.count(|c| c.revolve_count += 1);
        self.fault("revolve_count", self.calls.get().revolve_count)?;
        let result = self.inner.revolve(request, context);
        self.published();
        result
    }
    forward!(queries, support_plane(faces: &[SubShapeHandle], context: &OperationContext)
        -> Result<FacePlane>);
    forward!(queries, projected_bounds(shape: ShapeHandle, direction: ferritecad_types::Vec3)
        -> Result<[f64; 2]>);
    forward!(fillet, fillet_edges(request: &FilletRequest, context: &OperationContext)
        -> Result<OperationResult>);
    forward!(chamfer, chamfer_edges(request: &ChamferRequest, context: &OperationContext)
        -> Result<OperationResult>);
    forward!(queries, shape_topology(shape: ShapeHandle) -> Result<ShapeTopology>);
    forward!(queries, solid_count(shape: ShapeHandle) -> Result<usize>);
    forward!(queries, transform(shape: ShapeHandle, placement: &Transform, ctx: &OperationContext)
        -> Result<OperationResult>);
    forward!(queries, encode_shape_with(shape: ShapeHandle, entities: &[SubShapeHandle])
        -> Result<(BrepBlob, Vec<ArchiveSlot>)>);
    forward!(restore, decode_shape_with(blob: &BrepBlob, slots: &[ArchiveSlot])
        -> Result<(ShapeHandle, Vec<SubShapeHandle>)>);
    forward!(queries, encode_shape(shape: ShapeHandle) -> Result<BrepBlob>);
    forward!(restore, decode_shape(blob: &BrepBlob) -> Result<ShapeHandle>);
    fn release(&mut self, shape: ShapeHandle) {
        self.count(|c| c.released += 1);
        self.inner.release(shape);
        self.remaining.set(self.inner.population());
        self.published();
    }
}
