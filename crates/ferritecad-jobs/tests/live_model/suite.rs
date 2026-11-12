// SPDX-License-Identifier: MIT
#[path = "../support/counting.rs"]
mod counting;
use ferritecad_fixtures::live_model as fixture;
#[path = "../../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;
#[path = "operations.rs"]
mod operations;
#[path = "tree_operations.rs"]
mod tree_operations;

use crate::{DocumentSession, LiveDocument, PendingRevision};
use counting::{Calls, Counting, Stop, TopologyMock};
use ferritecad_document::{EditChange, ExpressionSlot};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{ErrorKind, ObjectId, Result};
type Observed = Counting<OcctKernel>;

trait Oracle: ferritecad_kernel::GeometryKernel + counting::Occupancy + Sized {
    fn fresh() -> Result<Self>;
    fn volume(&mut self, shape: ferritecad_kernel::ShapeHandle) -> Result<Option<f64>>;
}
impl Oracle for OcctKernel {
    fn fresh() -> Result<Self> {
        Self::new()
    }
    fn volume(&mut self, shape: ferritecad_kernel::ShapeHandle) -> Result<Option<f64>> {
        Ok(Some(self.shape_stats(shape)?.1))
    }
}
impl Oracle for TopologyMock {
    fn fresh() -> Result<Self> {
        Ok(Self::new())
    }
    fn volume(&mut self, _: ferritecad_kernel::ShapeHandle) -> Result<Option<f64>> {
        Ok(None)
    }
}

fn compare<K: Oracle, R>(pending: &mut PendingRevision<Counting<K>, R>) -> Result<()> {
    let doc = ferritecad_document::Document::open_read_only(pending.snapshot().path())?;
    let mut kernel = K::fresh()?;
    let ctx = OperationContext::default();
    let cold = ferritecad_eval::rebuild_cold(&doc, &mut kernel, &ctx)?;
    let built = pending.owner.evaluator.model.prepared();
    assert_eq!(built.order(), cold.order());
    assert_eq!(built.report(), cold.report());
    assert_eq!(
        built.topology().features.len(),
        cold.topology().features.len()
    );
    for (id, expected) in &cold.topology().features {
        assert_eq!(
            naming::signature(&built.topology().features[id]),
            naming::signature(expected)
        );
        let volume = pending
            .owner
            .evaluator
            .kernel
            .inner
            .volume(built.shape(*id).expect("live feature"))?;
        let reference = kernel.volume(cold.shape(*id).expect("cold feature"))?;
        assert_eq!(volume.is_some(), reference.is_some());
        if let (Some(actual), Some(expected)) = (volume, reference) {
            assert!(
                (actual - expected).abs() <= 1e-8,
                "{id}: {actual} versus {expected}"
            );
        }
    }
    let fresh = ferritecad_scene::snapshot_from_rebuild(
        &doc,
        &cold,
        &mut kernel,
        |_, _| unreachable!("no imports"),
        &TessellationParams::default(),
        &ctx,
    )?;
    cold.release_all(&mut kernel);
    assert_eq!(pending.scene().rebuild, fresh.rebuild);
    assert_eq!(pending.scene().catalogue, fresh.catalogue);
    assert_eq!(pending.scene().snapshot.meshes(), fresh.snapshot.meshes());
    assert_eq!(pending.scene(), &fresh);
    Ok(())
}

#[test]
fn native_reused_plate_and_cut_suffix_match_fresh_scenes() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let path = root.path().join("part.fcad");
    let case = fixture::write(&path)?;
    let kernel = Observed::wrap(OcctKernel::new()?);
    let context = OperationContext::default();
    let mut live = LiveDocument::open(
        DocumentSession::open(&path)?,
        kernel,
        |_: &mut Observed, _: &[u8]| unreachable!("no imports"),
        TessellationParams::default(),
        &context,
    )?;
    assert!(
        live.scene().rebuild.all_built(),
        "{:?}",
        live.scene().rebuild
    );
    let initial = live.evaluator.kernel.calls.take();
    assert_eq!(
        (
            initial.extrude_count,
            initial.boolean,
            initial.fillet,
            initial.tessellate
        ),
        (33, 30, 2, 3)
    );
    let features = 35;
    assert_eq!(live.evaluator.kernel.inner.live_shape_count(), features);
    let parameter = |object: ObjectId, value: &str| EditChange::SetParameter {
        object,
        expression: value.into(),
    };
    for (change, expected) in [
        (parameter(case.late, "4 mm"), (1, 1, 0, 1)),
        (parameter(case.early, "12 mm"), (29, 28, 2, 1)),
        (
            EditChange::constant(case.b, ExpressionSlot::ExtrudeDistance, 4.)?,
            (1, 1, 0, 1),
        ),
        (
            EditChange::RenameParameter {
                object: case.late,
                name: "pocket_depth".into(),
            },
            (0, 0, 0, 0),
        ),
        (
            EditChange::SetExpressionText {
                object: case.last,
                slot: ExpressionSlot::ExtrudeDistance,
                source: "2 * pocket_depth / 2".into(),
            },
            (0, 0, 0, 0),
        ),
    ] {
        for action in 0..3 {
            let before = ferritecad_sketch_solver::native_solves();
            let mut step = match action {
                0 => live.apply(
                    live.session().current().version(),
                    vec![change.clone()],
                    &context,
                )?,
                1 => live.undo(&context)?,
                _ => live.redo(&context)?,
            };
            assert_eq!(ferritecad_sketch_solver::native_solves() - before, 0);
            let calls = step.owner.evaluator.kernel.calls.take();
            assert_eq!(
                (
                    calls.extrude_count,
                    calls.boolean,
                    calls.fillet,
                    calls.tessellate
                ),
                expected
            );
            assert_eq!(
                (
                    calls.revolve_count,
                    calls.chamfer,
                    calls.restore,
                    calls.identity
                ),
                (0, 0, 0, 0)
            );
            if expected.0 == 0 {
                assert_eq!(calls, Calls::default());
            }
            // Boolean tools are unpublished. Completed feature results remain shared.
            assert_eq!(
                step.owner.evaluator.kernel.inner.live_shape_count(),
                features + expected.0 + expected.2
            );
            compare(&mut step)?;
            step.commit_step()?;
            assert_eq!(live.evaluator.kernel.inner.live_shape_count(), features);
            live.evaluator.kernel.calls.take();
        }
    }
    let scene = live.scene().clone();
    let version = live.session().current().version();
    let edit = parameter(case.late, "5 mm");
    let candidate = live.apply(version, vec![edit.clone()], &context)?;
    assert_eq!(
        candidate.owner.evaluator.kernel.inner.live_shape_count(),
        features + 1
    );
    drop(candidate);
    assert_eq!(live.evaluator.kernel.inner.live_shape_count(), features);
    let interrupted = OperationContext::default();
    let candidate = live.apply(version, vec![edit.clone()], &interrupted)?;
    interrupted.cancel().cancel();
    assert_eq!(
        candidate
            .commit_step()
            .expect_err("cancel before commit")
            .kind(),
        ErrorKind::Cancellation
    );
    assert_eq!(live.session().current().version(), version);
    assert_eq!(live.scene(), &scene);
    assert_eq!(live.evaluator.kernel.inner.live_shape_count(), features);
    for stop in [Stop::Extrude, Stop::BetweenNodes, Stop::Mesh] {
        live.evaluator.kernel.stop = Some(stop);
        let err = live
            .apply(version, vec![edit.clone()], &OperationContext::default())
            .err()
            .expect("cancelled candidate");
        assert_eq!(err.kind(), ErrorKind::Cancellation, "{err}");
        assert_eq!(live.scene(), &scene);
        assert_eq!(live.session().current().version(), version);
        assert_eq!(live.evaluator.kernel.inner.live_shape_count(), features);
    }
    live.evaluator.kernel.stop = None;
    let refused = live
        .apply(version, vec![parameter(case.early, "0 mm")], &context)
        .err()
        .expect("invalid height");
    assert_eq!(refused.kind(), ErrorKind::Input);
    let refusal = live
        .apply(
            version,
            vec![EditChange::constant(
                case.first,
                ExpressionSlot::FilletRadius,
                1000.,
            )?],
            &context,
        )
        .err()
        .expect("invalid fillet");
    assert!(
        ferritecad_eval::RebuildRefusal::from_error(&refusal).is_some(),
        "{refusal}"
    );
    assert_eq!(live.scene(), &scene);
    assert_eq!(live.evaluator.kernel.inner.live_shape_count(), features);
    live.evaluator.kernel.calls.take();
    live.apply(version, vec![edit], &context)?.commit_step()?;
    let calls = live.evaluator.kernel.calls.get();
    assert_eq!(
        (calls.extrude_count, calls.boolean, calls.tessellate),
        (1, 1, 1)
    );
    assert_eq!(live.evaluator.kernel.inner.live_shape_count(), features);
    let remaining = live.evaluator.kernel.remaining.clone();
    drop(live);
    assert_eq!(remaining.get(), 0);
    Ok(())
}

#[test]
fn revision_keys_and_mesh_parameters_invalidate_their_own_results() -> Result<()> {
    use ferritecad_document::Document;
    use ferritecad_eval::LiveModel;
    use ferritecad_scene::{BodyMeshes, snapshot_from_revision};
    let root = tempfile::tempdir()?;
    let path = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&path)?;
    let doc = Document::open_read_only(&path)?;
    let body = ferritecad_fixtures::feature_changes::body(&doc)?;
    let (feature, sketch, profile) =
        ferritecad_fixtures::feature_changes::base_profile(&doc, body)?;
    let nodes = [profile.plane, sketch, feature, body];
    let mut kernel = counting::CountingMock::new();
    let ctx = OperationContext::default();
    let mut model = LiveModel::default();
    let normal = TessellationParams::default();
    let fine = TessellationParams::new(0.005, 0.1, false)?;
    let import = |_: &mut counting::CountingMock,
                  _: &[u8]|
     -> Result<ferritecad_exchange::Import> { unreachable!("no imports") };
    let first = model.prepare(&doc, &mut kernel, &ctx, None)?;
    let keys = nodes.map(|id| first.result_key(id).expect("keyed built node"));
    let (_, meshes) = snapshot_from_revision(
        &doc,
        first,
        &mut kernel,
        import,
        &normal,
        &ctx,
        &BodyMeshes::default(),
    )?;
    assert!(
        model
            .prepare(&doc, &mut kernel, &ctx, None)
            .expect_err("one candidate")
            .to_string()
            .contains("pending revision")
    );
    model.accept(&mut kernel);
    kernel.calls.take();
    let next = model.prepare(&doc, &mut kernel, &ctx, None)?;
    assert_eq!(
        keys,
        nodes.map(|id| next.result_key(id).expect("restored key"))
    );
    assert_eq!(kernel.calls.take(), Calls::default());
    let (_, fine_meshes) =
        snapshot_from_revision(&doc, next, &mut kernel, import, &fine, &ctx, &meshes)?;
    assert_eq!(kernel.calls.take().tessellate, 1);
    snapshot_from_revision(&doc, next, &mut kernel, import, &fine, &ctx, &fine_meshes)?;
    assert_eq!(kernel.calls.take(), Calls::default());
    model.discard(&mut kernel);
    assert_eq!(kernel.live.len(), 1);
    let changed = OperationContext::new(ferritecad_types::Tolerance::new(1e-6, 1e-11)?);
    let next = model.prepare(&doc, &mut kernel, &changed, None)?;
    for (id, key) in nodes.into_iter().zip(keys) {
        assert_ne!(next.result_key(id), Some(key));
    }
    assert_eq!(kernel.calls.take().extrude_count, 1);
    assert_eq!(kernel.live.len(), 2);
    model.accept(&mut kernel);
    assert_eq!(kernel.live.len(), 1);
    model.release(&mut kernel);
    assert!(kernel.live.is_empty());
    Ok(())
}

#[test]
fn native_support_and_solved_sketch_follow_input_keys() -> Result<()> {
    use ferritecad_document::{Document, ObjectPayload, SketchConstraint, SketchConstraintRule};
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let path = root.path().join("boss.fcad");
    ferritecad_fixtures::write_plate(&path)?;
    let mut doc = Document::open(&path)?;
    let boss = ferritecad_fixtures::datum_planes::boss(&doc, &mut OcctKernel::new()?)?;
    doc.write_changes(&boss.changes)?;
    let ObjectPayload::Sketch(s) = doc.object(boss.sketch)?.expect("circle").payload else {
        unreachable!("boss uses a sketch")
    };
    doc.write_changes(&[EditChange::AddConstraint {
        sketch: boss.sketch,
        constraint: SketchConstraint {
            id: ferritecad_types::StableEntityId::new(),
            rule: SketchConstraintRule::Radius {
                curve: s.curves[0].id,
                radius: ferritecad_document::Expression::constant(3.)?,
            },
        },
    }])?;
    doc.close()?;
    let ctx = OperationContext::default();
    let mut live = LiveDocument::open(
        DocumentSession::open(&path)?,
        Observed::wrap(OcctKernel::new()?),
        |_: &mut Observed, _: &[u8]| unreachable!("no imports"),
        TessellationParams::default(),
        &ctx,
    )?;
    assert!(
        live.scene().rebuild.all_built(),
        "{:?}",
        live.scene().rebuild
    );
    for (edit, solves) in [
        (
            EditChange::constant(boss.base, ExpressionSlot::ExtrudeDistance, 14.)?,
            1,
        ),
        (
            EditChange::constant(boss.plane, ExpressionSlot::DatumOffset, -1.)?,
            1,
        ),
        (
            EditChange::SetExpressionText {
                object: boss.plane,
                slot: ExpressionSlot::DatumOffset,
                source: "-2 mm / 2".into(),
            },
            0,
        ),
    ] {
        live.evaluator.kernel.calls.take();
        let before = ferritecad_sketch_solver::native_solves();
        let mut step = live.apply(live.session().current().version(), vec![edit], &ctx)?;
        assert_eq!(ferritecad_sketch_solver::native_solves() - before, solves);
        if solves == 0 {
            assert_eq!(step.owner.evaluator.kernel.calls.take(), Calls::default());
        }
        compare(&mut step)?;
        step.commit_step()?;
    }
    Ok(())
}

#[test]
fn parameter_key_uses_value_and_no_change_keeps_the_accepted_picture() -> Result<()> {
    use ferritecad_document::Document;
    use ferritecad_eval::LiveModel;
    use ferritecad_types::Dimension;
    let root = tempfile::tempdir()?;
    let path = root.path().join("parameters.fcad");
    ferritecad_fixtures::write_plate(&path)?;
    let mut doc = Document::open(&path)?;
    let id = ObjectId::new();
    doc.write_changes(&[EditChange::AddParameter {
        object: id,
        name: "stock".into(),
        dimension: Dimension::Length,
        expression: "10 mm".into(),
    }])?;
    let mut model = LiveModel::default();
    let mut kernel = counting::CountingMock::new();
    let ctx = OperationContext::default();
    let key = model
        .prepare(&doc, &mut kernel, &ctx, None)?
        .result_key(id)
        .expect("parameter key");
    model.accept(&mut kernel);
    for (change, same) in [
        (
            EditChange::RenameParameter {
                object: id,
                name: "height".into(),
            },
            true,
        ),
        (
            EditChange::SetParameter {
                object: id,
                expression: "1 cm".into(),
            },
            true,
        ),
        (
            EditChange::SetParameter {
                object: id,
                expression: "11 mm".into(),
            },
            false,
        ),
    ] {
        doc.write_changes(&[change])?;
        assert_eq!(
            model.prepare(&doc, &mut kernel, &ctx, None)?.result_key(id) == Some(key),
            same
        );
        model.accept(&mut kernel);
    }
    model.release(&mut kernel);
    doc.close()?;
    let mut live = LiveDocument::open(
        DocumentSession::open(&path)?,
        counting::CountingMock::new(),
        |_: &mut counting::CountingMock, _: &[u8]| unreachable!("no imports"),
        TessellationParams::default(),
        &ctx,
    )?;
    let original = live.session().current();
    let before = live.scene().clone();
    let same = live.apply(
        original.version(),
        vec![EditChange::SetParameter {
            object: id,
            expression: "11 mm".into(),
        }],
        &ctx,
    )?;
    assert_eq!(same.commit_step()?, crate::StepCommit::NoChange);
    assert_eq!(live.session().current().version(), original.version());
    assert_eq!(live.scene(), &before);
    assert!(!live.session().can_undo());
    Ok(())
}

#[path = "rollback.rs"]
mod rollback;
