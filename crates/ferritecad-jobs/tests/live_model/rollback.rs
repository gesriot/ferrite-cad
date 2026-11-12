// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{Document, EndCondition, Expression, FeatureAddition};
use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool, edge_addition};
use std::path::Path;

#[path = "rollback_insertion.rs"]
mod insertion;

struct Small {
    body: ObjectId,
    base: ObjectId,
    cut: ObjectId,
    fillet: ObjectId,
    other: ObjectId,
}

fn small(path: &Path) -> Result<Small> {
    ferritecad_fixtures::write_plate(path)?;
    let mut doc = Document::open(path)?;
    let body = body(&doc)?;
    let (base, sketch, _) = base_profile(&doc, body)?;
    let cut = circular_tool(
        &doc,
        body,
        [20., 20.],
        2.,
        EndCondition::Blind {
            distance: Expression::constant(3.)?,
        },
    )?;
    doc.write_changes(&cut.changes)?;
    let fillet = edge_addition(&doc, body, 0, 2., false)?;
    let fillet_id = fillet.object();
    doc.write_changes(&[fillet])?;
    let other = ObjectId::new();
    doc.write_changes(&[EditChange::AddBody {
        object: other,
        name: Some("Independent".into()),
        feature_object: ObjectId::new(),
        feature_name: Some("Independent extrusion".into()),
        feature: FeatureAddition::Extrude {
            profile: sketch,
            regions: vec![],
            reversed: false,
            operation: ferritecad_document::SolidOperation::NewBody,
            end_condition: EndCondition::Blind {
                distance: Expression::constant(4.)?,
            },
        },
    }])?;
    doc.close()?;
    Ok(Small {
        body,
        base,
        cut: cut.feature,
        fillet: fillet_id,
        other,
    })
}

fn marker(object: ObjectId, feature: Option<ObjectId>) -> EditChange {
    EditChange::SetRollback { object, feature }
}

fn exercise<K: Oracle>() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("small.fcad");
    let case = small(&path)?;
    let context = OperationContext::default();
    let mut live = LiveDocument::open(
        DocumentSession::open(&path)?,
        Counting::wrap(K::fresh()?),
        |_: &mut Counting<K>, _: &[u8]| unreachable!("no imports"),
        TessellationParams::default(),
        &context,
    )?;
    assert!(live.scene().rebuild.all_built());
    for shown in [Some(case.base), Some(case.cut), None] {
        live.evaluator.kernel.calls.take();
        let version = live.session().current().version();
        let mut step = live.apply(version, vec![marker(case.body, shown)], &context)?;
        let calls = step.owner.evaluator.kernel.calls.take();
        assert_eq!(
            [
                calls.extrude_count,
                calls.revolve_count,
                calls.boolean,
                calls.fillet,
                calls.chamfer,
                calls.restore
            ],
            [0; 6],
            "moving a marker must reuse feature results"
        );
        assert_eq!(calls.tessellate, 1, "only the selected Body is tessellated");
        let built = step.owner.evaluator.model.prepared();
        assert_eq!(
            built.shape(case.body),
            built.shape(shown.unwrap_or(case.fillet))
        );
        assert!(matches!(
            built.status(case.cut),
            Some(ferritecad_eval::RebuildStatus::Built)
        ));
        assert!(matches!(
            built.status(case.fillet),
            Some(ferritecad_eval::RebuildStatus::Built)
        ));
        assert!(built.report().all_built());
        let independent = step
            .owner
            .scene
            .catalogue
            .iter()
            .find(|row| row.item == ferritecad_scene::SceneItem::Body(case.other))
            .expect("other Body");
        assert_eq!(
            step.scene()
                .catalogue
                .iter()
                .find(|row| row.item == independent.item),
            Some(independent)
        );
        compare(&mut step)?;
        step.commit_step()?;
        // Same marker follows the ordinary no-change session rule.
        let no_change = live.apply(
            live.session().current().version(),
            vec![marker(case.body, shown)],
            &context,
        )?;
        assert!(matches!(
            no_change.commit_step()?,
            crate::StepCommit::NoChange
        ));
        assert_eq!(live.evaluator.kernel.calls.take().tessellate, 0);
    }
    // A suppressed shown feature exposes its inherited predecessor result.
    let mut suppressed = live.apply(
        live.session().current().version(),
        vec![
            EditChange::SetSuppressed {
                object: case.cut,
                suppressed: true,
            },
            marker(case.body, Some(case.cut)),
        ],
        &context,
    )?;
    let built = suppressed.owner.evaluator.model.prepared();
    assert_eq!(built.shape(case.body), built.shape(case.base));
    assert!(matches!(
        built.status(case.cut),
        Some(ferritecad_eval::RebuildStatus::Suppressed)
    ));
    compare(&mut suppressed)?;
    suppressed.commit_step()?;
    Ok(())
}

#[test]
fn mock_rollback_reuses_all_features_and_keeps_the_independent_body() -> Result<()> {
    exercise::<TopologyMock>()
}

#[test]
fn native_rollback_reuses_all_features_and_matches_cold_pictures() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    exercise::<OcctKernel>()
}

#[test]
fn native_degraded_body_search_begins_at_the_shown_feature() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let path = root.path().join("repair.fcad");
    let case = small(&path)?;
    let mut doc = Document::open(&path)?;
    doc.write_changes(&[
        marker(case.body, Some(case.base)),
        EditChange::constant(case.fillet, ExpressionSlot::FilletRadius, 1000.)?,
    ])?;
    doc.close()?;
    let live = LiveDocument::open(
        DocumentSession::open(&path)?,
        OcctKernel::new()?,
        |_: &mut OcctKernel, _: &[u8]| unreachable!("no imports"),
        TessellationParams::default(),
        &OperationContext::default(),
    )?;
    let report = &live.scene().rebuild;
    assert_eq!(report.degraded_bodies.len(), 1);
    assert_eq!(report.degraded_bodies[0].rolled_back_to, Some(case.base));
    assert_eq!(report.degraded_bodies[0].cause, case.fillet);
    let source = live.scene().edit_source.as_ref().expect("source");
    let row = source
        .model_tree
        .rows
        .iter()
        .find(|r| r.id.0 == case.body)
        .expect("Body row");
    let name = |id| {
        source
            .objects
            .iter()
            .find(|o| o.id == id)
            .expect("object")
            .name
            .as_deref()
            .expect("named")
    };
    assert_eq!(row.name, format!("⚠ {} · rolled back", name(case.body)));
    assert_eq!(
        row.note,
        format!(
            "1 solid; Rolled back to {} because of {}",
            name(case.base),
            name(case.fillet)
        )
    );
    Ok(())
}
