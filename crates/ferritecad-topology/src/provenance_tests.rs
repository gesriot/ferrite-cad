// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_kernel::*;
use ferritecad_types::Transform;

struct TraceKernel {
    base: mock::MockKernel,
    trace: Option<NamingHistory>,
    topology: ShapeTopology,
    slot_fault: u8,
}
impl GeometryKernel for TraceKernel {
    fn identity(&self) -> &KernelIdentity {
        self.base.identity()
    }
    fn shape_topology(&mut self, _: ShapeHandle) -> Result<ShapeTopology> {
        Ok(self.topology.clone())
    }
    fn naming_history(&mut self, _: ShapeHandle) -> Result<NamingHistory> {
        self.trace
            .clone()
            .ok_or_else(|| CadError::unsupported("missing live history"))
    }
    fn extrude(&mut self, r: &ExtrudeRequest, c: &OperationContext) -> Result<OperationResult> {
        self.base.extrude(r, c)
    }
    fn boolean(&mut self, r: &BooleanRequest, c: &OperationContext) -> Result<OperationResult> {
        self.base.boolean(r, c)
    }
    fn transform(
        &mut self,
        s: ShapeHandle,
        t: &Transform,
        c: &OperationContext,
    ) -> Result<OperationResult> {
        self.base.transform(s, t, c)
    }
    fn tessellate(
        &mut self,
        s: ShapeHandle,
        p: &TessellationParams,
        c: &OperationContext,
    ) -> Result<Mesh> {
        self.base.tessellate(s, p, c)
    }
    fn encode_shape_with(
        &mut self,
        s: ShapeHandle,
        n: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
        let (blob, mut slots) = self.base.encode_shape_with(s, n)?;
        match self.slot_fault {
            4 => slots[1] = slots[0],
            5 => slots[0] = ArchiveSlot::ROOT,
            6 => {
                slots.pop();
            }
            _ => {}
        }
        Ok((blob, slots))
    }
    fn decode_shape_with(
        &mut self,
        b: &BrepBlob,
        n: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        let (shape, mut entities) = self.base.decode_shape_with(b, n)?;
        match self.slot_fault {
            1 => entities[1] = entities[0],
            2 => {
                entities[0] = SubShapeHandle::new(
                    ShapeHandle::new(SessionId::new(), 999),
                    entities[0].kind(),
                    entities[0].index(),
                )
            }
            3 => {
                entities[0] = SubShapeHandle::new(
                    shape,
                    if entities[0].kind() == SubShapeKind::Face {
                        SubShapeKind::Edge
                    } else {
                        SubShapeKind::Face
                    },
                    entities[0].index(),
                )
            }
            _ => {}
        }
        Ok((shape, entities))
    }
    fn encode_shape(&mut self, s: ShapeHandle) -> Result<BrepBlob> {
        self.base.encode_shape(s)
    }
    fn decode_shape(&mut self, b: &BrepBlob) -> Result<ShapeHandle> {
        self.base.decode_shape(b)
    }
    fn release(&mut self, s: ShapeHandle) {
        self.base.release(s);
    }
}
#[test]
fn incomplete_foreign_and_contradictory_live_provenance_cannot_publish_names() {
    let (mut base, model, owner, request) = crate::archive::tests::plate();
    let shape = model.features[&owner].shape().expect("shape");
    let topology = base.shape_topology(shape).expect("topology");
    let trace = base.naming_history(shape).expect("history");
    let face = topology.entities(SubShapeKind::Face).next().expect("face");
    let curve = request.profile().segments().next().expect("curve").label;
    let mut cases = vec![None, Some(NamingHistory::default())];
    let mut bad = trace.clone();
    bad.operation = Some(NamingOperation::Revolve);
    cases.push(Some(bad));
    let mut bad = trace.clone();
    bad.relations
        .retain(|(i, _, _)| *i != NamingInput::Cap { end: true });
    cases.push(Some(bad));
    let mut bad = trace.clone();
    bad.relations
        .retain(|(i, _, _)| *i != NamingInput::Curve(curve));
    cases.push(Some(bad));
    let mut bad = trace.clone();
    bad.relations.insert((
        NamingInput::Curve(StableEntityId::new()),
        NamingRelation::Generated,
        Some(face),
    ));
    cases.push(Some(bad));
    let mut bad = trace.clone();
    bad.relations.insert((
        NamingInput::Curve(curve),
        NamingRelation::Deleted,
        Some(face),
    ));
    cases.push(Some(bad));
    let mut bad = trace.clone();
    bad.relations
        .insert((NamingInput::Curve(curve), NamingRelation::Generated, None));
    cases.push(Some(bad));
    let mut bad = trace.clone();
    bad.relations.insert((
        NamingInput::Curve(curve),
        NamingRelation::Generated,
        Some(SubShapeHandle::new(shape, SubShapeKind::Face, u64::MAX)),
    ));
    cases.push(Some(bad));
    let mut bad = trace.clone();
    bad.relations.insert((
        NamingInput::SubShape(face),
        NamingRelation::Modified,
        Some(face),
    ));
    cases.push(Some(bad));
    let mut kernel = TraceKernel {
        base,
        trace: Some(trace.clone()),
        topology,
        slot_fault: 0,
    };
    let before = model.derivations.clone();
    for fault in cases {
        kernel.trace = fault;
        assert!(
            model
                .name_operation(owner, shape, &[], &mut kernel)
                .is_err()
        );
        assert_eq!(model.derivations, before);
    }
    kernel.trace = Some(trace);
    let named = model
        .name_operation(owner, shape, &[], &mut kernel)
        .expect("complete account");
    assert_eq!(named.entities.len(), 26);
    for kind in [SubShapeKind::Face, SubShapeKind::Edge, SubShapeKind::Vertex] {
        let entity = kernel.topology.entities(kind).next().expect("kind");
        let mut bad = kernel.topology.clone();
        bad.adjacent_faces.insert(entity, BTreeSet::from([entity]));
        assert!(validate_history(shape, &bad, kernel.trace.as_ref().expect("trace"), &[]).is_err());
    }
}
#[test]
fn each_input_subform_requires_one_complete_survival_account() {
    let (_, model, owner, _) = crate::archive::tests::plate();
    let source = &model.features[&owner];
    let shape = ShapeHandle::new(SessionId::new(), 9);
    let map = |s: SubShapeHandle| SubShapeHandle::new(shape, s.kind(), s.index());
    let topology = ShapeTopology {
        adjacent_faces: source
            .topology
            .adjacent_faces
            .iter()
            .map(|(s, f)| (map(*s), f.iter().copied().map(map).collect()))
            .collect(),
    };
    let trace = NamingHistory {
        cap_boundaries: BTreeMap::new(),
        operation: Some(NamingOperation::Cut),
        relations: source
            .entities
            .keys()
            .map(|s| {
                (
                    NamingInput::SubShape(*s),
                    NamingRelation::Kept,
                    Some(map(*s)),
                )
            })
            .collect(),
    };
    validate_history(shape, &topology, &trace, &[source]).expect("all inputs accounted for");
    for input in source.entities.keys() {
        let mut absent = trace.clone();
        absent
            .relations
            .retain(|(i, _, _)| *i != NamingInput::SubShape(*input));
        assert!(validate_history(shape, &topology, &absent, &[source]).is_err());
        let mut contradiction = trace.clone();
        contradiction.relations.insert((
            NamingInput::SubShape(*input),
            NamingRelation::Deleted,
            None,
        ));
        assert!(validate_history(shape, &topology, &contradiction, &[source]).is_err());
        let mut deleted = absent;
        deleted
            .relations
            .insert((NamingInput::SubShape(*input), NamingRelation::Deleted, None));
        validate_history(shape, &topology, &deleted, &[source])
            .expect("an explicit deletion is an account");
    }
}

#[test]
fn invalid_kernel_slot_tables_never_publish_or_leak_a_restored_shape() {
    let (mut base, model, owner, _) = crate::archive::tests::plate();
    let shape = model.features[&owner].shape().expect("shape");
    let archive = crate::archive_feature(&mut base, &model, owner).expect("archive");
    let topology = base.shape_topology(shape).expect("topology");
    let mut kernel = TraceKernel {
        base,
        trace: None,
        topology,
        slot_fault: 0,
    };
    for fault in 1..=6 {
        kernel.slot_fault = fault;
        let before = kernel.base.live_shape_count();
        if fault <= 3 {
            let mut destination = model.clone();
            destination.remove_feature(owner);
            assert!(crate::restore_feature(&mut kernel, &archive, &mut destination).is_err());
            assert!(destination.features.is_empty());
        } else {
            assert!(crate::archive_feature(&mut kernel, &model, owner).is_err());
        }
        assert_eq!(kernel.base.live_shape_count(), before);
    }
}

#[test]
fn restoration_requires_local_output_rows_and_nonempty_inherited_rows() {
    let (mut kernel, mut model, parent, request) = crate::archive::tests::plate();
    let child = ObjectId::new();
    let prepared = crate::archive::tests::support::prepared(child, request.profile(), false)
        .expect("child roots");
    model.operations.extend(prepared.operations);
    model.planes.extend(prepared.planes);
    model.roots.extend(prepared.roots);
    model.sweeps.extend(prepared.sweeps);
    model.previous.insert(child, parent);
    model.index_lineage().expect("lineage");
    let mut valid = crate::archive_feature(&mut kernel, &model, parent).expect("parent archive");
    valid.producer = child;
    valid.derivations.clear();
    let root = derivation::intern(
        &mut valid.derivations,
        model.derivations(),
        Derivation::Cap(child, false, model.planes.get(&child).copied()),
    )
    .expect("own root");
    let own = derivation::expression(
        &mut valid.derivations,
        model.derivations(),
        SubShapeKind::Face,
        "generated",
        Some(child),
        [root],
        NamingOperation::Extrude.tag(),
    )
    .expect("own output");
    valid.ancestry.insert(own, BTreeSet::new());
    let mut destination = model.clone();
    crate::restore_feature(&mut kernel, &valid, &mut destination).expect("inherited live names");
    assert_eq!(destination.features[&child].derivations.len(), 2);
    assert!(destination.features[&child].ancestry[&own].is_empty());
    kernel.release(destination.features[&child].shape().expect("restored"));

    let mut cases = Vec::new();
    let mut bad = valid.clone();
    bad.ancestry.remove(&own);
    cases.push(bad);
    let mut bad = valid.clone();
    let inherited = *bad.ancestry.keys().find(|n| **n != own).expect("inherited");
    bad.ancestry.get_mut(&inherited).expect("row").clear();
    cases.push(bad);
    let mut bad = valid.clone();
    bad.derivations.remove(&root.digest);
    cases.push(bad);
    for bad in cases {
        let mut destination = model.clone();
        let before = kernel.live_shape_count();
        assert!(crate::restore_feature(&mut kernel, &bad, &mut destination).is_err());
        assert_eq!(destination.derivations(), model.derivations());
        assert!(!destination.features.contains_key(&child));
        assert_eq!(kernel.live_shape_count(), before);
    }
    let mut without_parent = model.clone();
    without_parent.remove_feature(parent);
    assert!(crate::restore_feature(&mut kernel, &valid, &mut without_parent).is_err());
    assert!(without_parent.derivations().is_empty());
}
