// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{CacheStore, ObjectPayload};
use ferritecad_eval::{RebuildResult, rebuild_cached, rebuild_cold};
use ferritecad_kernel::GeometryKernel;
use std::collections::BTreeMap;

fn same_results(
    actual: &RebuildResult,
    kernel: &mut OcctKernel,
    expected: &RebuildResult,
    reference: &mut OcctKernel,
) -> Result<()> {
    assert!(actual.report().all_built());
    assert_eq!(actual.report(), expected.report());
    assert_eq!(
        actual.topology().features.len(),
        expected.topology().features.len()
    );
    for (id, names) in &expected.topology().features {
        assert_eq!(
            naming::signature(&actual.topology().features[id]),
            naming::signature(names)
        );
        let actual = kernel.shape_stats(actual.shape(*id).expect("actual shape"))?;
        let expected = reference.shape_stats(expected.shape(*id).expect("reference shape"))?;
        assert_eq!(actual.0, expected.0);
        assert!(
            (actual.1 - expected.1).abs() < 1e-8,
            "{id}: {actual:?} versus {expected:?}"
        );
    }
    Ok(())
}

#[test]
fn native_inserted_fillet_preserves_later_chamfer_names_and_matches_append_reference() -> Result<()>
{
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let path = root.path().join("insert.fcad");
    ferritecad_fixtures::write_plate(&path)?;
    let mut doc = Document::open(&path)?;
    let body = body(&doc)?;
    let (base, sketch, _) = base_profile(&doc, body)?;
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
    let reference_path = root.path().join("reference.fcad");
    doc.snapshot_to(&reference_path)?;
    let fillet = edge_addition(&doc, body, 0, 2., false)?;
    let chamfer = edge_addition(&doc, body, 2, 1., true)?;
    let (fillet_id, chamfer_id) = (fillet.object(), chamfer.object());
    doc.write_changes(std::slice::from_ref(&chamfer))?;
    doc.close()?;
    let mut reference = Document::open(&reference_path)?;
    reference.write_changes(&[fillet.clone(), chamfer])?;
    let context = OperationContext::default();
    let mut live = LiveDocument::open(
        DocumentSession::open(&path)?,
        Counting::wrap(OcctKernel::new()?),
        |_: &mut Observed, _: &[u8]| unreachable!("no imports"),
        TessellationParams::default(),
        &context,
    )?;
    assert!(live.scene().rebuild.all_built());
    live.evaluator.kernel.calls.take();
    let mut step = live.apply(
        live.session().current().version(),
        vec![marker(body, Some(base)), fillet, marker(body, None)],
        &context,
    )?;
    let calls = step.owner.evaluator.kernel.calls.take();
    assert_eq!(
        [
            calls.extrude_count,
            calls.revolve_count,
            calls.boolean,
            calls.fillet,
            calls.chamfer,
            calls.tessellate,
            calls.restore
        ],
        [0, 0, 0, 1, 1, 1, 0],
    );
    assert_eq!(
        step.scene()
            .catalogue
            .iter()
            .find(|r| r.item == ferritecad_scene::SceneItem::Body(other)),
        step.owner
            .scene
            .catalogue
            .iter()
            .find(|r| r.item == ferritecad_scene::SceneItem::Body(other)),
    );
    let candidate = Document::open_read_only(step.snapshot().path())?;
    let facts = |doc: &Document| -> Result<BTreeMap<_, _>> {
        Ok(doc
            .objects()?
            .into_iter()
            .map(|o| (o.id, (o.name, o.payload)))
            .collect())
    };
    assert_eq!(facts(&candidate)?, facts(&reference)?);
    assert_eq!(candidate.dependencies()?, reference.dependencies()?);
    assert_eq!(
        candidate
            .object(chamfer_id)?
            .expect("Chamfer")
            .payload
            .previous_feature(),
        Some(fillet_id)
    );
    let mut reference_kernel = OcctKernel::new()?;
    let expected = rebuild_cold(&reference, &mut reference_kernel, &context)?;
    assert!(expected.report().all_built());
    same_results(
        step.owner.evaluator.model.prepared(),
        &mut step.owner.evaluator.kernel.inner,
        &expected,
        &mut reference_kernel,
    )?;
    let mut cache_kernel = OcctKernel::new()?;
    let mut cache = CacheStore::open(
        root.path().join("insert.fcad-cache"),
        candidate.meta().document_id,
        cache_kernel.identity().id(),
        cache_kernel.identity().version(),
    )?;
    for _ in 0..2 {
        let (cached, _) = rebuild_cached(&candidate, &mut cache_kernel, &mut cache, &context)?;
        same_results(&cached, &mut cache_kernel, &expected, &mut reference_kernel)?;
        cached.release_all(&mut cache_kernel);
    }
    expected.release_all(&mut reference_kernel);
    compare(&mut step)?;
    step.commit_step()?;
    Ok(())
}

#[test]
fn insertion_referencing_a_later_result_refuses_the_existing_graph_cycle_atomically() -> Result<()>
{
    let root = tempfile::tempdir()?;
    let path = root.path().join("cycle.fcad");
    let case = small(&path)?;
    let doc = Document::open(&path)?;
    let (_, sketch, _) = base_profile(&doc, case.body)?;
    let mut face = ferritecad_fixtures::feature_changes::plate_edge(&doc, case.body, 2)?;
    face.meaning.kind = ferritecad_document::EntityKind::Face;
    let addition = EditChange::AddFeature {
        object: ObjectId::new(),
        body: case.body,
        name: Some("Later-result cut".into()),
        feature: FeatureAddition::Extrude {
            profile: sketch,
            regions: vec![],
            operation: ferritecad_document::SolidOperation::Cut,
            reversed: false,
            end_condition: EndCondition::UpToFace {
                result: case.fillet,
                face,
                offset_mm: Expression::constant(0.)?,
            },
        },
    };
    let inserted = addition.object();
    doc.close()?;
    let context = OperationContext::default();
    let mut live = LiveDocument::open(
        DocumentSession::open(&path)?,
        Counting::wrap(TopologyMock::new()),
        |_: &mut Counting<TopologyMock>, _: &[u8]| unreachable!("no imports"),
        TessellationParams::default(),
        &context,
    )?;
    let version = live.session().current().version();
    let bytes = std::fs::read(live.session().current().path())?;
    let scene = live.scene().clone();
    let error = live
        .apply(
            version,
            vec![marker(case.body, Some(case.base)), addition],
            &context,
        )
        .err()
        .expect("cycle refused");
    let mut cycle = [case.body, case.cut, case.fillet, inserted];
    cycle.sort();
    assert_eq!(error.kind(), ErrorKind::Input);
    assert_eq!(
        error.to_string(),
        format!(
            "invalid input: document validation failed before cold rebuild: graph.not-orderable: \
         invalid input: the feature graph contains a cycle among: {}",
            cycle.map(|id| id.to_string()).join(", "),
        )
    );
    assert_eq!(live.session().current().version(), version);
    assert_eq!(std::fs::read(live.session().current().path())?, bytes);
    assert_eq!(live.scene(), &scene);
    Ok(())
}

#[test]
fn native_inserted_cut_that_removes_a_later_fillet_edge_is_refused_atomically() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let path = root.path().join("lost-edge.fcad");
    let case = small(&path)?;
    let doc = Document::open(&path)?;
    let cut = circular_tool(&doc, case.body, [0., 0.], 4., EndCondition::ThroughAll)?;
    doc.close()?;
    let context = OperationContext::default();
    let mut live = LiveDocument::open(
        DocumentSession::open(&path)?,
        Counting::wrap(OcctKernel::new()?),
        |_: &mut Observed, _: &[u8]| unreachable!("no imports"),
        TessellationParams::default(),
        &context,
    )?;
    assert!(live.scene().rebuild.all_built());
    let version = live.session().current().version();
    let bytes = std::fs::read(live.session().current().path())?;
    let scene = live.scene().clone();
    let mut changes = vec![marker(case.body, Some(case.base))];
    changes.extend(cut.changes);
    changes.push(marker(case.body, None));
    let error = live
        .apply(version, changes, &context)
        .err()
        .expect("lost edge refuses insertion");
    assert_eq!(error.kind(), ErrorKind::Topology);
    let report = &ferritecad_eval::RebuildRefusal::from_error(&error)
        .expect("full report")
        .report;
    assert_eq!(
        report.failed.iter().map(|f| f.id).collect::<Vec<_>>(),
        vec![case.fillet]
    );
    assert_eq!(live.session().current().version(), version);
    assert_eq!(std::fs::read(live.session().current().path())?, bytes);
    assert_eq!(live.scene(), &scene);
    let doc = Document::open_read_only(live.session().current().path())?;
    let ObjectPayload::Fillet(fillet) = doc.object(case.fillet)?.expect("Fillet").payload else {
        unreachable!("Fillet")
    };
    let reason = format!(
        "unresolved topology reference: feature {}, reference {}: \
         edge deleted from the consumed result",
        case.fillet, fillet.edges[0].id,
    );
    assert_eq!(report.failed[0].message, reason);
    assert_eq!(
        report.skipped,
        vec![ferritecad_eval::SkippedFeature {
            id: case.body,
            cause: case.fillet,
        }]
    );
    assert_eq!(
        error.to_string(),
        format!(
            "Plate ({}), Fillet ({}): edit would leave features unbuilt:\n\
         {}: {reason}\n\
         {}: not built: depends on {}: {reason}\n",
            case.body, case.fillet, case.fillet, case.body, case.fillet,
        )
    );
    Ok(())
}
