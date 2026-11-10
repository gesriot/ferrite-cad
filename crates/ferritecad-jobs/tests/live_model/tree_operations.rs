// SPDX-License-Identifier: MIT
use super::*;

fn exercise<K: Oracle>() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("source.fcad");
    let model = fixture::write(&path)?;
    let doc = ferritecad_document::Document::open_read_only(&path)?;
    let middle = doc
        .object(model.last)?
        .expect("last cut")
        .payload
        .previous_feature()
        .expect("penultimate cut");
    doc.close()?;
    let ctx = OperationContext::default();
    let mut live = LiveDocument::open(
        DocumentSession::open(&path)?,
        Counting::wrap(K::fresh()?),
        |_: &mut Counting<K>, _: &[u8]| unreachable!("native document"),
        TessellationParams::default(),
        &ctx,
    )?;
    assert!(live.scene().rebuild.all_built());
    assert_eq!(live.evaluator.kernel.inner.population(), 35);
    live.evaluator.kernel.calls.take();
    for (change, work, union, retained) in [
        (
            EditChange::SetSuppressed {
                object: model.last,
                suppressed: true,
            },
            [0, 0, 0, 0, 0, 1],
            35,
            34,
        ),
        (
            EditChange::SetSuppressed {
                object: model.last,
                suppressed: false,
            },
            [1, 0, 1, 0, 0, 1],
            35,
            35,
        ),
        (
            EditChange::RemoveObject { object: middle },
            [1, 0, 1, 0, 0, 1],
            36,
            34,
        ),
        (
            EditChange::SetSuppressed {
                object: model.b,
                suppressed: true,
            },
            [0, 0, 0, 0, 0, 1],
            34,
            33,
        ),
    ] {
        let version = live.session().current().version();
        let before_path = live.session().current().path().to_owned();
        let bytes = std::fs::read(&before_path)?;
        let solves = ferritecad_sketch_solver::native_solves();
        let mut step = live.apply(version, vec![change], &ctx)?;
        let c = step.owner.evaluator.kernel.calls.take();
        assert_eq!(
            [
                c.extrude_count,
                c.revolve_count,
                c.boolean,
                c.fillet,
                c.chamfer,
                c.tessellate
            ],
            work
        );
        assert_eq!((c.restore, c.identity), (0, 0));
        assert_eq!(ferritecad_sketch_solver::native_solves() - solves, 0);
        assert_eq!(step.owner.evaluator.kernel.inner.population(), union);
        assert_eq!(step.owner.session().current().version(), version);
        assert_eq!(std::fs::read(&before_path)?, bytes);
        compare(&mut step)?;
        step.commit_step()?;
        assert_eq!(live.evaluator.kernel.inner.population(), retained);
        live.evaluator.kernel.calls.take();
    }
    // Undo restores the accepted pre-suppression revision, then Redo shares its predecessor.
    for (undo, work, union, retained) in [(true, 1, 34, 34), (false, 0, 34, 33)] {
        let mut step = if undo {
            live.undo(&ctx)?
        } else {
            live.redo(&ctx)?
        };
        let c = step.owner.evaluator.kernel.calls.take();
        assert_eq!(
            (
                c.extrude_count,
                c.boolean,
                c.fillet,
                c.chamfer,
                c.tessellate
            ),
            (work, work, 0, 0, 1)
        );
        assert_eq!(step.owner.evaluator.kernel.inner.population(), union);
        compare(&mut step)?;
        step.commit_step()?;
        assert_eq!(live.evaluator.kernel.inner.population(), retained);
        live.evaluator.kernel.calls.take();
    }
    // The suppressed tip's key must carry the changed predecessor into Body reuse.
    for (change, work, union, retained) in [
        (
            EditChange::SetSuppressed {
                object: model.last,
                suppressed: true,
            },
            [0, 0, 0, 1],
            33,
            32,
        ),
        (
            EditChange::SetParameter {
                object: model.early,
                expression: "12 mm".into(),
            },
            [27, 26, 2, 1],
            61,
            32,
        ),
    ] {
        let version = live.session().current().version();
        let mut step = live.apply(version, vec![change], &ctx)?;
        let c = step.owner.evaluator.kernel.calls.take();
        assert_eq!([c.extrude_count, c.boolean, c.fillet, c.tessellate], work);
        assert_eq!(step.owner.evaluator.kernel.inner.population(), union);
        compare(&mut step)?;
        step.commit_step()?;
        assert_eq!(live.evaluator.kernel.inner.population(), retained);
        live.evaluator.kernel.calls.take();
    }
    Ok(())
}
#[test]
fn mock_tree_edits_reuse_prefixes_and_independent_bodies() -> Result<()> {
    exercise::<TopologyMock>()
}
#[test]
fn native_tree_edits_match_cold_names_volumes_and_pictures() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    exercise::<OcctKernel>()
}
