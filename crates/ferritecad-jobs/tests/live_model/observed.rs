// SPDX-License-Identifier: MIT
use ferritecad_kernel::*;
use ferritecad_occt::OcctKernel;
use ferritecad_types::{Result, Transform};
use std::{cell::Cell, rc::Rc};

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Calls {
    pub extrude: usize,
    pub boolean: usize,
    pub fillet: usize,
    pub chamfer: usize,
    pub revolve: usize,
    pub restore: usize,
    pub tessellate: usize,
    pub queries: usize,
    pub identity: usize,
    pub released: usize,
}

#[derive(Default)]
pub struct Reading {
    pub calls: Cell<Calls>,
    pub live: Cell<usize>,
    pub stop: Cell<Option<Stop>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Extrude,
    BetweenNodes,
    Mesh,
}

pub struct Observed {
    inner: OcctKernel,
    state: Rc<Reading>,
    cancel: Option<CancelToken>,
}

impl Observed {
    pub fn new() -> Result<(Self, Rc<Reading>)> {
        let state = Rc::new(Reading::default());
        Ok((
            Self {
                inner: OcctKernel::new()?,
                state: state.clone(),
                cancel: None,
            },
            state,
        ))
    }

    fn count(&self, f: impl FnOnce(&mut Calls)) {
        let mut counts = self.state.calls.get();
        f(&mut counts);
        self.state.calls.set(counts);
    }

    fn sample(&self) {
        self.state.live.set(self.inner.live_shape_count());
    }
}

macro_rules! forward {
    ($count:ident, $name:ident($($arg:ident: $ty:ty),*) -> $out:ty) => {
        fn $name(&mut self,$($arg:$ty),*) -> $out {
            self.count(|c| c.$count += 1);
            let out=self.inner.$name($($arg),*);
            self.sample();
            out
        }
    };
}

impl GeometryKernel for Observed {
    fn identity(&self) -> &KernelIdentity {
        self.count(|c| c.identity += 1);
        self.inner.identity()
    }
    fn extrude(&mut self, r: &ExtrudeRequest, ctx: &OperationContext) -> Result<OperationResult> {
        self.count(|c| c.extrude += 1);
        if self.state.stop.get() == Some(Stop::Extrude) {
            let cancel = ctx.cancel().clone();
            let inner = ctx
                .clone()
                .with_progress(ProgressSink::new(move |_| cancel.cancel()));
            let answer = self.inner.extrude(r, &inner);
            self.sample();
            return answer;
        }
        let answer = self.inner.extrude(r, ctx);
        self.sample();
        answer
    }
    fn tessellate(
        &mut self,
        s: ShapeHandle,
        p: &TessellationParams,
        ctx: &OperationContext,
    ) -> Result<Mesh> {
        self.count(|c| c.tessellate += 1);
        let mesh = self.inner.tessellate(s, p, ctx)?;
        if self.state.stop.get() == Some(Stop::Mesh) {
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
        if self.state.stop.get() == Some(Stop::BetweenNodes) {
            self.cancel = Some(ctx.cancel().clone());
        }
        let result = self.inner.boolean(r, ctx);
        self.sample();
        result
    }
    forward!(revolve, revolve(request: &RevolveRequest, context: &OperationContext)
        -> Result<OperationResult>);
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
    forward!(released, release(shape: ShapeHandle) -> ());
}
