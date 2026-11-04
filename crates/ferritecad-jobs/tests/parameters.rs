// SPDX-License-Identifier: MIT
use ferritecad_document::{
    CacheStore, DatumPlane, Document, EditChange as Edit, EndCondition, Expression,
    ExpressionSlot as Slot, FeatureAddition, ObjectPayload, RevolveAxis, RevolveExtent,
    SketchConstraint, SketchConstraintRule, SolidOperation,
};
use ferritecad_eval::{CacheOutcome, rebuild_cached, rebuild_cold};
use ferritecad_fixtures::datum_planes::circle;
use ferritecad_jobs::DocumentSession;
use ferritecad_kernel::mock::MockKernel;
use ferritecad_kernel::{GeometryKernel, OperationContext};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{Dimension, ErrorKind, ObjectId, Result, StableEntityId};

fn parameter(id: ObjectId, name: &str, expression: &str) -> Edit {
    Edit::AddParameter {
        object: id,
        name: name.into(),
        dimension: Dimension::Length,
        expression: expression.into(),
    }
}

fn formula(id: ObjectId, slot: Slot, text: &str) -> Edit {
    Edit::SetExpressionText {
        object: id,
        slot,
        source: text.into(),
    }
}

#[test]
fn each_parameter_edit_undoes_and_redoes_as_one_snapshot() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let doc = ferritecad_fixtures::open_plate(dir.path())?;
    let path = doc.path().to_owned();
    let tip = ferritecad_fixtures::feature_changes::body(&doc)?;
    let (extrude, _, _) = ferritecad_fixtures::feature_changes::base_profile(&doc, tip)?;
    doc.close()?;
    let id = ObjectId::new();
    let mut session = DocumentSession::open(&path)?;
    let mut kernel = MockKernel::new();
    let context = OperationContext::default();
    session.current().load_scene(
        &mut kernel,
        |_, _| unreachable!("no imports"),
        &ferritecad_kernel::TessellationParams::default(),
        &context,
    )?;
    for changes in [
        vec![
            parameter(id, "height", "7 mm"),
            formula(extrude, Slot::ExtrudeDistance, "2 * height"),
        ],
        vec![Edit::SetParameter {
            object: id,
            expression: "3.25 cm".into(),
        }],
        vec![Edit::RenameParameter {
            object: id,
            name: "stock".into(),
        }],
        vec![
            formula(extrude, Slot::ExtrudeDistance, "12"),
            Edit::RemoveParameter { object: id },
        ],
    ] {
        let before = session.current().version();
        let step = session.begin_step().edit(
            changes,
            before,
            &mut kernel,
            |_, _| unreachable!("no imports"),
            &context,
        )?;
        session.commit_step(step)?;
        let after = session.current().version();
        assert_ne!(after, before);
        session.commit_move(session.begin_undo().expect("undo"))?;
        assert_eq!(session.current().version(), before);
        session.commit_move(session.begin_redo().expect("redo"))?;
        assert_eq!(session.current().version(), after);
        Document::open_read_only(session.current().path())?;
    }
    Ok(())
}

#[test]
fn graph_values_drive_geometry_and_only_changed_geometry_misses() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("part.fcad");
    let chain = ferritecad_fixtures::write_fillet_chain(&path, 1.5)?;
    let mut doc = Document::open(&path)?;
    let (_, _, profile) = ferritecad_fixtures::feature_changes::base_profile(&doc, chain.body)?;
    let [h, plane, sketch, revolve, independent] = std::array::from_fn(|_| ObjectId::new());
    doc.write_changes(&[
        parameter(h, "h", "3 mm"),
        formula(chain.extrude, Slot::ExtrudeDistance, "4 * h"),
        formula(chain.first, Slot::FilletRadius, "h / 2"),
        Edit::AddDatumPlane {
            object: plane,
            name: None,
            plane: DatumPlane::Offset {
                from: profile.plane,
                distance_mm: Expression::constant(1.)?,
            },
        },
        formula(plane, Slot::DatumOffset, "h"),
        Edit::AddSketch {
            object: sketch,
            name: None,
            sketch: circle(plane, [8., 0.], 2.)?,
        },
        Edit::AddBody {
            object: ObjectId::new(),
            name: None,
            feature_object: revolve,
            feature_name: None,
            feature: FeatureAddition::Revolve {
                profile: sketch,
                regions: vec![],
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::FullTurn,
                operation: SolidOperation::NewBody,
            },
        },
        formula(revolve, Slot::RevolveAngle, "h / 1 mm * 60 deg"),
        Edit::AddBody {
            object: ObjectId::new(),
            name: None,
            feature_object: independent,
            feature_name: None,
            feature: FeatureAddition::Extrude {
                profile: chain.sketch,
                regions: vec![],
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(9.)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
            },
        },
    ])?;
    let mut kernel = OcctKernel::new()?;
    let context = OperationContext::default();
    let mut cache = CacheStore::open(
        dir.path().join("cache"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )?;
    for (index, change) in [
        None,
        Some(Edit::SetParameter {
            object: h,
            expression: "6 mm".into(),
        }),
        Some(Edit::RenameParameter {
            object: h,
            name: "stock".into(),
        }),
        Some(formula(chain.extrude, Slot::ExtrudeDistance, "stock * 4")),
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(change) = change {
            doc.write_changes(&[change])?;
        }
        let height = if index == 0 { 3.0 } else { 6.0 };
        let slots = doc.expression_slots()?;
        let radius = slots
            .iter()
            .find(|s| s.object == chain.first)
            .expect("Fillet radius");
        assert_eq!(radius.expression.value(), height / 2.);
        let cold = rebuild_cold(&doc, &mut kernel, &context)?;
        let (warm, events) = rebuild_cached(&doc, &mut kernel, &mut cache, &context)?;
        assert!(cold.all_built() && warm.all_built(), "{:?}", warm.report());
        assert_eq!(events.len(), 5);
        for event in events {
            let hit = index >= 2 || (index == 1 && event.feature == independent);
            assert_eq!(
                event.outcome,
                if hit {
                    CacheOutcome::Hit
                } else {
                    CacheOutcome::Miss
                }
            );
        }
        for state in [&cold, &warm] {
            assert_eq!(state.plane(plane).expect("offset").origin().z, height);
            let shape = state.shape(revolve).expect("revolve");
            assert!(kernel.is_valid(shape)?);
            let torus = 2. * std::f64::consts::PI.powi(2) * 8. * 4.;
            assert!((kernel.shape_stats(shape)?.1 - torus * height / 6.).abs() < 1e-6);
        }
        for id in [
            chain.extrude,
            chain.first,
            chain.second,
            independent,
            revolve,
        ] {
            let volume = |kernel: &mut OcctKernel, s: &ferritecad_eval::RebuildResult| {
                kernel
                    .shape_stats(s.shape(id).expect("built"))
                    .map(|stats| stats.1)
            };
            assert!((volume(&mut kernel, &cold)? - volume(&mut kernel, &warm)?).abs() < 1e-6);
            let keys = |s: &ferritecad_eval::RebuildResult| {
                s.topology().features[&id]
                    .bindings
                    .keys()
                    .copied()
                    .collect::<Vec<_>>()
            };
            assert_eq!(keys(&cold), keys(&warm));
        }
        cold.release_all(&mut kernel);
        warm.release_all(&mut kernel);
    }
    doc.write_changes(&[formula(revolve, Slot::RevolveAngle, "360 deg")])?;
    let (same, events) = rebuild_cached(&doc, &mut kernel, &mut cache, &context)?;
    assert!(same.all_built());
    assert!(events.iter().all(|e| e.outcome == CacheOutcome::Hit));
    assert!(doc.expression_slots()?.iter().any(|s| s.object == revolve
        && s.expression.source == "360 deg"
        && s.expression.value().to_bits() == std::f64::consts::TAU.to_bits()));
    same.release_all(&mut kernel);

    let original = doc.object(chain.extrude)?.expect("base");
    let mut corrupt = original.payload.clone();
    let ObjectPayload::Extrude(extrude) = &mut corrupt else {
        unreachable!("Extrude")
    };
    extrude.end_condition = EndCondition::Blind {
        distance: Expression::new("stock * 4", 25.)?,
    };
    doc.write(|w| {
        w.put_object(
            original.id,
            original.parent,
            original.ordinal,
            original.name.as_deref(),
            &corrupt,
        )
    })?;
    let read = Document::open_read_only(&path)?;
    let cold = rebuild_cold(&read, &mut kernel, &context)?;
    let (cached, events) = rebuild_cached(&read, &mut kernel, &mut cache, &context)?;
    assert_eq!(cold.report(), cached.report());
    assert!(cached.report().failed.iter().any(|f| f.id == original.id
        && f.kind == "input"
        && f.message.contains("extrude_distance")
        && f.message.contains("stored value 25")));
    assert!(
        events
            .iter()
            .any(|e| e.feature == independent && e.outcome == CacheOutcome::Hit)
    );
    assert!(!events.iter().any(|e| e.feature == chain.extrude));
    cold.release_all(&mut kernel);
    cached.release_all(&mut kernel);
    read.close()?;
    doc.write(|w| {
        w.put_object(
            original.id,
            original.parent,
            original.ordinal,
            original.name.as_deref(),
            &original.payload,
        )
    })?;
    // Corrupt an already cached input. Stored-value verification must precede a cache hit.
    for (text, saved) in [("3 mm", 6.), ("6 mm / 0", 6.), ("6 rad", 6.)] {
        let mut obj = doc.object(h)?.expect("stock");
        let ObjectPayload::Parameter(p) = &mut obj.payload else {
            unreachable!("stock")
        };
        p.expression = Expression::new(text, saved)?;
        doc.write(|w| {
            w.put_object(
                h,
                obj.parent,
                obj.ordinal,
                obj.name.as_deref(),
                &obj.payload,
            )
        })?;
        let read = Document::open_read_only(&path)?;
        let cold = rebuild_cold(&read, &mut kernel, &context)?;
        let (warm, events) = rebuild_cached(&read, &mut kernel, &mut cache, &context)?;
        assert_eq!(cold.report(), warm.report());
        let report = warm.report();
        assert!(
            report
                .failed
                .iter()
                .any(|f| f.id == h && f.kind == ErrorKind::Input.as_str())
        );
        assert!(report.failed.iter().any(|f| {
            f.id == chain.extrude
                && f.message
                    .contains("depends on parameter stock, which failed")
        }));
        assert!(warm.shape(independent).is_some());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].feature, independent);
        assert_eq!(events[0].outcome, CacheOutcome::Hit);
        cold.release_all(&mut kernel);
        warm.release_all(&mut kernel);
    }
    Ok(())
}

#[test]
fn a_length_parameter_updates_the_solved_radius() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::availability().is_ok(), "PLANEGCS"),
    ]) {
        return Ok(());
    }
    let dir = tempfile::tempdir()?;
    let mut doc = ferritecad_fixtures::open_plate(dir.path())?;
    let body = ferritecad_fixtures::feature_changes::body(&doc)?;
    let (feature, sketch_id, base) =
        ferritecad_fixtures::feature_changes::base_profile(&doc, body)?;
    let mut obj = doc.object(sketch_id)?.expect("sketch");
    let mut sketch = circle(base.plane, [0., 0.], 3.)?;
    let id = StableEntityId::new();
    sketch.constraints.push(SketchConstraint {
        id,
        rule: SketchConstraintRule::Radius {
            curve: sketch.curves[0].id,
            radius: Expression::constant(3.)?,
        },
    });
    obj.payload = ObjectPayload::Sketch(sketch);
    doc.write(|w| {
        w.put_object(
            sketch_id,
            obj.parent,
            obj.ordinal,
            obj.name.as_deref(),
            &obj.payload,
        )
    })?;
    let h = ObjectId::new();
    doc.write_changes(&[
        parameter(h, "r", "4 mm"),
        formula(sketch_id, Slot::Radius { constraint: id }, "r"),
        formula(feature, Slot::ExtrudeDistance, "10 mm"),
    ])?;
    let mut kernel = OcctKernel::new()?;
    let mut cache = CacheStore::open(
        dir.path().join("r-cache"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )?;
    for radius in [4., 6.] {
        doc.write_changes(&[Edit::SetParameter {
            object: h,
            expression: radius.to_string(),
        }])?;
        let context = OperationContext::default();
        let cold = rebuild_cold(&doc, &mut kernel, &context)?;
        let (fill, events) = rebuild_cached(&doc, &mut kernel, &mut cache, &context)?;
        assert_eq!(events[0].outcome, CacheOutcome::Miss);
        let (warm, events) = rebuild_cached(&doc, &mut kernel, &mut cache, &context)?;
        assert_eq!(events[0].outcome, CacheOutcome::Hit);
        for state in [cold, fill, warm] {
            assert!(state.all_built(), "{:?}", state.report());
            let volume = kernel
                .shape_stats(state.shape(feature).expect("extrusion"))?
                .1;
            assert!((volume - std::f64::consts::PI * radius * radius * 10.).abs() < 1e-5);
            state.release_all(&mut kernel);
        }
    }
    Ok(())
}
