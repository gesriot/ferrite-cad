// SPDX-License-Identifier: MIT
use ferritecad_fixtures::result_sets::rectangle;
use ferritecad_kernel::{
    BooleanKind, BooleanRequest, ExtrudeExtent, ExtrudeRequest, GeometryKernel, NamingHistory,
    NamingInput, NamingRelation, OperationContext, ShapeHandle, SketchPlane, SolidWitness,
    SubShapeHandle, SubShapeKind,
};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{ObjectId, Point3, Result, StableEntityId, Vec3};
use std::collections::{BTreeMap, BTreeSet};

fn block(
    kernel: &mut OcctKernel,
    min: [f64; 2],
    max: [f64; 2],
    bottom: f64,
    top: f64,
) -> Result<ShapeHandle> {
    let plane = SketchPlane::new(Point3::new(0., 0., bottom)?, Vec3::X, Vec3::Z)?;
    let sketch = rectangle(ObjectId::new(), min, max)?;
    let profile = ferritecad_eval::profile_from_sketch(&sketch, ObjectId::new(), plane)?.profile?;
    let request = ExtrudeRequest::new(profile, ExtrudeExtent::blind(top - bottom)?, false);
    Ok(kernel
        .extrude(&request, &OperationContext::default())?
        .shape)
}

fn face_from(
    history: &NamingHistory,
    input: NamingInput,
    relation: NamingRelation,
) -> SubShapeHandle {
    let faces: Vec<_> = history
        .relations
        .iter()
        .filter(|(source, link, _)| *source == input && *link == relation)
        .filter_map(|(_, _, output)| *output)
        .collect();
    let [face]: [_; 1] = faces.try_into().expect("one output face");
    face
}

struct StartCell {
    shape: ShapeHandle,
    prism_history: NamingHistory,
    cut_history: NamingHistory,
    selection: NamingHistory,
}

fn start_cell(kernel: &mut OcctKernel, target: ShapeHandle) -> Result<StartCell> {
    let context = OperationContext::default();
    let prism = block(kernel, [0., 0.], [10., 8.], 0., 11.)?;
    let caps = kernel.naming_history(prism)?;
    let cut = BooleanRequest::new(BooleanKind::Cut, prism, target)?;
    let outside = kernel.boolean(&cut, &context)?.shape;
    let split = kernel.naming_history(outside)?;
    let surviving_cap = |end| {
        let cap = face_from(&caps, NamingInput::Cap { end }, NamingRelation::Generated);
        face_from(&split, NamingInput::SubShape(cap), NamingRelation::Kept)
    };
    let start = surviving_cap(false);
    let far = surviving_cap(true);
    let witness = SolidWitness {
        reference: StableEntityId::new(),
        faces: vec![start],
    };
    let cell = kernel.keep_solids(outside, &[witness], &context)?.shape;
    assert_eq!(kernel.solid_count(cell)?, 1);
    assert!(kernel.is_valid(cell)?);
    let kept = kernel.naming_history(cell)?;
    assert!(
        kept.relations
            .contains(&(NamingInput::SubShape(far), NamingRelation::Deleted, None))
    );

    Ok(StartCell {
        shape: cell,
        prism_history: caps,
        cut_history: split,
        selection: kept,
    })
}

#[test]
fn the_start_cell_extends_behind_a_partially_covering_shelf() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let mut kernel = OcctKernel::new()?;
    let context = OperationContext::default();
    let shelf = block(&mut kernel, [-2., -2.], [5., 10.], 3., 4.)?;
    let roof = block(&mut kernel, [-2., -2.], [12., 10.], 8., 10.)?;
    let fuse = BooleanRequest::new(BooleanKind::Fuse, shelf, roof)?;
    let target = kernel.boolean(&fuse, &context)?.shape;
    assert_eq!(kernel.solid_count(target)?, 2);

    let cell = start_cell(&mut kernel, target)?.shape;

    // The two halves first meet material at 3 and 8 mm. The selected solid
    // also fills the gap above the shelf, which starts at 4 mm.
    let first_hit_volume = 5. * 8. * 3. + 5. * 8. * 8.;
    let hidden = block(&mut kernel, [0., 0.], [5., 8.], 4., 8.)?;
    let common = BooleanRequest::new(BooleanKind::Common, cell, hidden)?;
    let overlap = kernel.boolean(&common, &context)?.shape;
    let cell_volume = kernel.shape_stats(cell)?.1;
    let hidden_volume = kernel.shape_stats(overlap)?.1;
    assert!((hidden_volume - 160.).abs() < 1e-7, "{hidden_volume}");
    assert!(
        (cell_volume - first_hit_volume - hidden_volume).abs() < 1e-7,
        "{cell_volume}"
    );

    let left = block(&mut kernel, [0., 0.], [5., 8.], 0., 3.)?;
    let right = block(&mut kernel, [5., 0.], [10., 8.], 0., 8.)?;
    let stepped = BooleanRequest::new(BooleanKind::Fuse, left, right)?;
    let first_hit_tool = kernel.boolean(&stepped, &context)?.shape;
    for (tool, expected_volume) in [(first_hit_tool, 860.), (cell, 1020.)] {
        let add = BooleanRequest::new(BooleanKind::Fuse, target, tool)?;
        let result = kernel.boolean(&add, &context)?.shape;
        assert!(kernel.is_valid(result)?);
        assert_eq!(kernel.solid_count(result)?, 1);
        let volume = kernel.shape_stats(result)?.1;
        assert!((volume - expected_volume).abs() < 1e-7, "{volume}");
    }
    Ok(())
}

#[test]
fn compare_end_orientation_and_outer_curve_witnesses_on_shelves_and_steps() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let mut kernel = OcctKernel::new()?;
    let roof = block(&mut kernel, [-2., -2.], [12., 10.], 8., 10.)?;
    for (min, max, top, volume, backwards, expected_counts) in [
        ([-2., -2.], [5., 10.], 4., 600., 1, vec![2, 3, 3, 4]),
        ([-2., -2.], [5., 10.], 8., 440., 0, vec![2, 3, 3]),
        ([3., 2.], [7., 6.], 8., 560., 0, vec![0, 0, 0, 0, 0, 4]),
    ] {
        let lower = block(&mut kernel, min, max, 3., top)?;
        let fuse = BooleanRequest::new(BooleanKind::Fuse, roof, lower)?;
        let target = kernel.boolean(&fuse, &OperationContext::default())?.shape;
        let cell = start_cell(&mut kernel, target)?;
        assert!((kernel.shape_stats(cell.shape)?.1 - volume).abs() < 1e-7);

        let retained: BTreeMap<_, _> = cell
            .selection
            .relations
            .iter()
            .filter_map(
                |&(input, relation, output)| match (input, relation, output) {
                    (NamingInput::SubShape(source), NamingRelation::Kept, Some(face)) => {
                        Some((source, face))
                    }
                    _ => None,
                },
            )
            .collect();
        let mut ends = BTreeSet::new();
        let mut edge_curves: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        for &(input, _, output) in &cell.cut_history.relations {
            let (NamingInput::SubShape(source), Some(output)) = (input, output) else {
                continue;
            };
            let Some(&part) = retained.get(&output) else {
                continue;
            };
            if source.shape() == target
                && source.kind() == SubShapeKind::Face
                && part.kind() == SubShapeKind::Face
            {
                ends.insert(part);
            }
            if part.kind() == SubShapeKind::Edge {
                for &(root, _, ancestor) in &cell.prism_history.relations {
                    if let NamingInput::Curve(curve) = root
                        && ancestor == Some(source)
                    {
                        edge_curves.entry(part).or_default().insert(curve);
                    }
                }
            }
        }
        let topology = kernel.shape_topology(cell.shape)?;
        let mut witness_counts = Vec::new();
        let mut backward_faces = 0;
        for end in ends {
            // These are planar patches. The outward normal applies to the whole patch.
            let (_, normal, _) = kernel.face_plane(end)?;
            backward_faces += usize::from(normal[2] < 0.);
            let curves: BTreeSet<_> = edge_curves
                .iter()
                .filter(|(edge, _)| topology.adjacent_faces[edge].contains(&end))
                .flat_map(|(_, curves)| curves.iter().copied())
                .collect();
            witness_counts.push(curves.len());
        }
        witness_counts.sort_unstable();
        assert_eq!(witness_counts, expected_counts);
        assert_eq!(backward_faces, backwards);
    }
    Ok(())
}

#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;

#[test]
fn the_interior_step_has_five_bindings_for_one_end_name() -> Result<()> {
    use ferritecad_document::{EntityKind, EntityReference, ReferenceMeaning, Selection};
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let mut kernel = OcctKernel::new()?;
    let context = OperationContext::default();
    let profile = ferritecad_eval::profile_from_sketch(
        &rectangle(ObjectId::new(), [0., 0.], [10., 8.])?,
        ObjectId::new(),
        SketchPlane::world_xy(),
    )?
    .profile?;
    let roof = block(&mut kernel, [-2., -2.], [12., 10.], 8., 10.)?;
    let owner = ObjectId::new();
    for (min, max, top, tool_volume, add_volume) in [
        ([-2., -2.], [5., 10.], 4., 0., 0.),
        ([-2., -2.], [5., 10.], 8., 440., 1196.),
        ([3., 2.], [7., 6.], 8., 560., 976.),
    ] {
        let lower = block(&mut kernel, min, max, 3., top)?;
        let target = kernel
            .boolean(
                &BooleanRequest::new(BooleanKind::Fuse, lower, roof)?,
                &context,
            )?
            .shape;
        let before = kernel.encode_shape(target)?;
        let tool = kernel.extrude_to_next(&profile, false, target, &context);
        if top == 4. {
            let error = tool.expect_err("the air gap needs a new transition face");
            assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
            assert!(error.to_string().contains(concat!(
                "the target's next boundary does not close the profile without a new ",
                "transition face; choose an end face with up_to_face"
            )));
            assert_eq!(before.bytes(), kernel.encode_shape(target)?.bytes());
            continue;
        }
        let tool = tool?;
        let history = kernel.naming_history(tool.shape)?;
        let named = naming::bind(&mut kernel, owner, &profile, &tool)?;
        let feature = &named.features[&owner];
        let ends: BTreeSet<_> = history
            .relations
            .iter()
            .filter(|(input, _, _)| *input == NamingInput::Cap { end: true })
            .filter_map(|(_, _, output)| *output)
            .collect();
        let empty: BTreeSet<_> = ends
            .iter()
            .copied()
            .filter(|e| history.cap_boundaries[e].is_empty())
            .collect();
        if tool_volume == 560. {
            assert_eq!(ends.len(), 6);
            assert_eq!(empty.len(), 5);
            let inner = feature.entities[empty.first().expect("an inner face")];
            assert_eq!(feature.bindings[&inner], empty);
            let roof = ends.difference(&empty).next().expect("one roof");
            assert_eq!(
                feature.bindings[&feature.entities[roof]],
                BTreeSet::from([*roof])
            );
            let mut reference = EntityReference::new(ReferenceMeaning {
                producer: owner,
                kind: EntityKind::Face,
                digest: inner.digest,
                selection: Selection::One,
            });
            let one = named
                .resolve(EntityKind::Face, owner, owner, &[reference])
                .expect_err("an exact reference cannot select one of five faces");
            assert!(one.to_string().contains("split into 5 faces"));
            reference.meaning.selection = Selection::AllDescendants;
            let all = named.resolve(EntityKind::Face, owner, owner, &[reference])?;
            assert_eq!(all.into_iter().collect::<BTreeSet<_>>(), empty);
        } else {
            assert_eq!(ends.len(), 3);
            assert!(empty.is_empty());
            assert!(
                ends.iter()
                    .all(|face| feature.bindings[&feature.entities[face]].len() == 1)
            );
        }
        let add = kernel.boolean(
            &BooleanRequest::new(BooleanKind::Fuse, target, tool.shape)?,
            &context,
        )?;
        for (shape, volume) in [(tool.shape, tool_volume), (add.shape, add_volume)] {
            assert!(kernel.is_valid(shape)?);
            assert_eq!(kernel.solid_count(shape)?, 1);
            assert!((kernel.shape_stats(shape)?.1 - volume).abs() < 1e-7);
        }
    }
    Ok(())
}

#[test]
fn interior_step_add_and_intersect_keep_cold_names_after_restore() -> Result<()> {
    use ferritecad_document::*;
    use ferritecad_eval::{CacheOutcome, rebuild_cached, rebuild_cold};
    use ferritecad_fixtures::result_sets::{extrusion, plate};
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    for (operation, volume) in [
        (SolidOperation::Add, 976.),
        (SolidOperation::Intersect, 560.),
    ] {
        let dir = tempfile::tempdir().expect("model directory");
        let mut doc = Document::create(dir.path().join("step.fcad"))?;
        let datum = plate(&mut doc)?.plane;
        let [
            body,
            base,
            step,
            start,
            level,
            base_sk,
            step_sk,
            profile,
            feature,
        ] = std::array::from_fn(|_| ObjectId::new());
        let (bottom, height, modify) = match operation {
            SolidOperation::Add => (8., 2., SolidOperation::Add),
            SolidOperation::Intersect => (-2., 10., SolidOperation::Cut),
            _ => unreachable!("the two fixtures"),
        };
        let plane = |object, distance| -> Result<EditChange> {
            Ok(EditChange::AddDatumPlane {
                object,
                name: None,
                plane: DatumPlane::Offset {
                    from: datum,
                    distance_mm: Expression::constant(distance)?,
                },
            })
        };
        let sketch = |object, plane, min, max| -> Result<EditChange> {
            Ok(EditChange::AddSketch {
                object,
                name: None,
                sketch: rectangle(plane, min, max)?,
            })
        };
        doc.write_changes(&[
            plane(start, bottom)?,
            plane(level, 3.)?,
            sketch(base_sk, start, [-2., -2.], [12., 10.])?,
            EditChange::AddBody {
                object: body,
                name: None,
                feature_object: base,
                feature_name: None,
                feature: extrusion(base_sk, SolidOperation::NewBody, height)?,
            },
            sketch(step_sk, level, [3., 2.], [7., 6.])?,
            EditChange::AddFeature {
                object: step,
                name: None,
                body,
                feature: extrusion(step_sk, modify, 5.)?,
            },
            sketch(profile, datum, [0., 0.], [10., 8.])?,
            EditChange::AddFeature {
                object: feature,
                name: None,
                body,
                feature: FeatureAddition::Extrude {
                    profile,
                    regions: Vec::new(),
                    operation,
                    reversed: false,
                    end_condition: EndCondition::UpToNext,
                },
            },
        ])?;
        let mut kernel = OcctKernel::new()?;
        let context = OperationContext::default();
        let cold = rebuild_cold(&doc, &mut kernel, &context)?;
        assert!(cold.all_built(), "{:?}", cold.report());
        let shape = cold.shape(feature).expect("final feature");
        assert!(kernel.is_valid(shape)?);
        assert_eq!(kernel.solid_count(shape)?, 1);
        assert!((kernel.shape_stats(shape)?.1 - volume).abs() < 1e-7);
        let names = &cold.topology().features[&feature];
        let expected = naming::signature(names);
        if operation == SolidOperation::Intersect {
            let model = cold.topology();
            let target = cold.shape(step).expect("target");
            let history = kernel.naming_history(shape)?;
            let mut end_faces = BTreeSet::new();
            for &(source, _, output) in &history.relations {
                if let (NamingInput::SubShape(face), Some(piece)) = (source, output)
                    && face.shape() == target
                    && face.kind() == SubShapeKind::Face
                    && piece.kind() == SubShapeKind::Face
                {
                    end_faces.insert(piece);
                }
            }
            assert_eq!(end_faces.len(), 6);
            for face in &end_faces {
                let name = names.entities[face];
                let description = model.describe(feature, name)?;
                assert!(description.contains(&format!("; {} = merge", name.digest)));
                assert_eq!(names.bindings[&name].len(), 1);
            }
        }
        let mut cache = CacheStore::open(
            dir.path().join("cache"),
            doc.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )?;
        for expected_event in [CacheOutcome::Miss, CacheOutcome::Hit] {
            let (restored, events) = rebuild_cached(&doc, &mut kernel, &mut cache, &context)?;
            assert!(restored.all_built(), "{:?}", restored.report());
            assert!(events.iter().all(|event| event.outcome == expected_event));
            assert_eq!(
                naming::signature(&restored.topology().features[&feature]),
                expected
            );
            let shape = restored.shape(feature).expect("restored feature");
            assert!(kernel.is_valid(shape)?);
            assert!((kernel.shape_stats(shape)?.1 - volume).abs() < 1e-7);
            restored.release_all(&mut kernel);
        }
        cold.release_all(&mut kernel);
        assert_eq!(kernel.live_shape_count(), 0);
    }
    Ok(())
}

#[test]
fn certify_the_entire_trimmed_curved_end() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let context = OperationContext::default();
    let mut kernel = OcctKernel::new()?;
    let plane = SketchPlane::new(Point3::new(5., -2., 10.)?, Vec3::X, Vec3::Y)?;
    let profile = ferritecad_eval::profile_from_sketch(
        &rectangle(ObjectId::new(), [0., 0.], [10., 8.])?,
        ObjectId::new(),
        SketchPlane::world_xy(),
    )?
    .profile?;
    for radius in [8., 3.] {
        let sketch = ferritecad_fixtures::datum_planes::circle(ObjectId::new(), [0., 0.], radius)?;
        let section =
            ferritecad_eval::profile_from_sketch(&sketch, ObjectId::new(), plane)?.profile?;
        let cylinder = kernel
            .extrude(
                &ExtrudeRequest::new(section, ExtrudeExtent::blind(12.)?, false),
                &context,
            )?
            .shape;
        let roof = block(&mut kernel, [-2., -2.], [12., 10.], 22., 24.)?;
        let target = kernel
            .boolean(
                &BooleanRequest::new(BooleanKind::Fuse, cylinder, roof)?,
                &context,
            )?
            .shape;
        let end = kernel.extrude_to_next(&profile, false, target, &context);
        if radius == 3. {
            let error = end.expect_err("the narrow cylinder turns away before the complete roof");
            assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input, "{error}");
            assert!(error.to_string().contains("without a new transition face"));
        } else {
            let shape = end?.shape;
            assert!(kernel.is_valid(shape)?);
            let arc_area =
                5. * (radius * radius - 25.).sqrt() + radius * radius * (5. / radius).asin();
            assert!((kernel.shape_stats(shape)?.1 - 8. * (100. - arc_area)).abs() < 1e-5);
        }
    }
    Ok(())
}

#[test]
fn first_exit_cavity_mixed_side_and_missing_coverage() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let mut kernel = OcctKernel::new()?;
    let context = OperationContext::default();
    let plate = block(&mut kernel, [0., 0.], [10., 10.], 0., 10.)?;
    let void = block(&mut kernel, [2., 2.], [8., 8.], 2., 5.)?;
    let hollow = kernel
        .boolean(
            &BooleanRequest::new(BooleanKind::Cut, plate, void)?,
            &context,
        )?
        .shape;
    let profile = |min, max, z| -> Result<_> {
        ferritecad_eval::profile_from_sketch(
            &rectangle(ObjectId::new(), min, max)?,
            ObjectId::new(),
            SketchPlane::new(Point3::new(0., 0., z)?, Vec3::X, Vec3::Z)?,
        )?
        .profile
    };
    for (target, depth, remaining) in [(plate, 10., 840.), (hollow, 5., 812.)] {
        let p = profile([3., 3.], [7., 7.], 10.)?;
        let end = kernel.extrude_to_next(&p, true, target, &context)?.shape;
        let cut = kernel
            .boolean(
                &BooleanRequest::new(BooleanKind::Cut, target, end)?,
                &context,
            )?
            .shape;
        for (shape, volume) in [(end, 16. * depth), (cut, remaining)] {
            assert!(kernel.is_valid(shape)?);
            assert!((kernel.shape_stats(shape)?.1 - volume).abs() < 1e-7);
        }
    }
    for (z, min, max, message) in [
        (
            4.,
            [8., 2.],
            [12., 6.],
            "the profile starts partly inside and partly outside the target",
        ),
        (
            -2.,
            [8., 2.],
            [12., 6.],
            "nothing next ahead of the sketch plane covers the profile",
        ),
        (
            12.,
            [2., 2.],
            [6., 6.],
            "nothing next ahead of the sketch plane covers the profile",
        ),
    ] {
        let error = kernel
            .extrude_to_next(&profile(min, max, z)?, false, plate, &context)
            .expect_err("side or coverage must refuse");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
        assert!(error.to_string().contains(message), "{error}");
    }
    Ok(())
}

#[test]
fn a_toroidal_end_refuses_without_changing_its_predecessor() -> Result<()> {
    use ferritecad_kernel::{RevolveAxis, RevolveRequest, RevolveTurn};
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let mut kernel = OcctKernel::new()?;
    let context = OperationContext::default();
    let circle = ferritecad_fixtures::datum_planes::circle(ObjectId::new(), [10., 8.], 3.)?;
    let axial = SketchPlane::new(Point3::ORIGIN, Vec3::X, Vec3::new(0., -1., 0.)?)?;
    let profile = ferritecad_eval::profile_from_sketch(&circle, ObjectId::new(), axial)?.profile?;
    let torus = kernel
        .revolve(
            &RevolveRequest::new(profile, RevolveAxis::PlaneY, RevolveTurn::Full),
            &context,
        )?
        .shape;
    let tool = rectangle(ObjectId::new(), [9., -1.], [11., 1.])?;
    let profile =
        ferritecad_eval::profile_from_sketch(&tool, ObjectId::new(), SketchPlane::world_xy())?
            .profile?;
    let before = kernel.encode_shape(torus)?;
    let count = kernel.live_shape_count();
    let error = kernel
        .extrude_to_next(&profile, false, torus, &context)
        .expect_err("torus");
    assert_eq!(
        error.kind(),
        ferritecad_types::ErrorKind::Unsupported,
        "{error}"
    );
    assert!(
        error.to_string().contains(concat!(
            "up to next supports planar, cylindrical, conical and spherical end faces, ",
            "not a toroidal face; choose an end face with up_to_face"
        )),
        "{error}"
    );
    assert_eq!(kernel.encode_shape(torus)?.bytes(), before.bytes());
    assert_eq!(kernel.live_shape_count(), count);
    Ok(())
}

fn tilted_step(round: bool, gap: bool, tilt: f64, azimuth: f64) -> Result<()> {
    let (sin_tilt, cos_tilt) = tilt.to_radians().sin_cos();
    let (sin_az, cos_az) = azimuth.to_radians().sin_cos();
    let normal = Vec3::new(sin_tilt * cos_az, sin_tilt * sin_az, cos_tilt)?;
    let x_axis = Vec3::new(cos_tilt * cos_az, cos_tilt * sin_az, -sin_tilt)?;
    let plane = |height: f64| {
        SketchPlane::new(
            Point3::new(
                11. + height * normal.x,
                -7. + height * normal.y,
                3. + height * normal.z,
            )?,
            x_axis,
            normal,
        )
    };
    let profile = |sketch, z| -> Result<_> {
        ferritecad_eval::profile_from_sketch(&sketch, ObjectId::new(), plane(z)?)?.profile
    };
    let context = OperationContext::default();
    let mut kernel = OcctKernel::new()?;
    let roof = profile(rectangle(ObjectId::new(), [-2., -2.], [12., 10.])?, 8.)?;
    let lower = if round {
        ferritecad_fixtures::datum_planes::circle(ObjectId::new(), [5., 4.], 2.)?
    } else {
        rectangle(ObjectId::new(), [-2., -2.], [5., 10.])?
    };
    let mut extrude = |profile, depth| -> Result<_> {
        Ok(kernel
            .extrude(
                &ExtrudeRequest::new(profile, ExtrudeExtent::blind(depth)?, false),
                &context,
            )?
            .shape)
    };
    let roof = extrude(roof, 2.)?;
    let shelf = extrude(profile(lower, 3.)?, if gap { 1. } else { 5. })?;
    let target = kernel
        .boolean(
            &BooleanRequest::new(BooleanKind::Fuse, roof, shelf)?,
            &context,
        )?
        .shape;
    let tool = profile(rectangle(ObjectId::new(), [0., 0.], [10., 8.])?, 0.)?;
    let end = kernel.extrude_to_next(&tool, false, target, &context);
    if gap {
        let error = end.expect_err("the cell extends behind the shelf");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
        assert!(
            error.to_string().contains("without a new transition face"),
            "{error}"
        );
    } else {
        let shape = end?.shape;
        assert!(kernel.is_valid(shape)?);
        let expected = if round {
            640. - 20. * std::f64::consts::PI
        } else {
            440.
        };
        assert!((kernel.shape_stats(shape)?.1 - expected).abs() < 1e-6);
    }
    Ok(())
}

#[test]
fn tilted_planar_step_keeps_its_wall_and_refuses_a_shelf_gap() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    tilted_step(false, false, 39., 31.)?;
    tilted_step(false, true, 39., 31.)
}

#[test]
fn tilted_cylindrical_wall_parallel_to_the_sweep_keeps_its_volume() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    tilted_step(true, false, 37., 29.)
}

#[test]
fn separate_regions_cut_to_their_exits_with_cold_names_after_restore() -> Result<()> {
    use ferritecad_document::*;
    use ferritecad_eval::{CacheOutcome, rebuild_cached, rebuild_cold};
    use ferritecad_fixtures::result_sets::plate;
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let dir = tempfile::tempdir().expect("model directory");
    let mut doc = Document::create(dir.path().join("regions.fcad"))?;
    let plate = plate(&mut doc)?;
    let [top, sketch, feature] = std::array::from_fn(|_| ObjectId::new());
    let mut regions = rectangle(top, [5., 5.], [15., 15.])?;
    regions
        .curves
        .extend(rectangle(top, [30., 10.], [50., 30.])?.curves);
    doc.write_changes(&[
        EditChange::AddDatumPlane {
            object: top,
            name: None,
            plane: DatumPlane::Offset {
                from: plate.plane,
                distance_mm: Expression::constant(10.)?,
            },
        },
        EditChange::AddSketch {
            object: sketch,
            name: None,
            sketch: regions,
        },
        EditChange::AddFeature {
            object: feature,
            name: None,
            body: plate.body,
            feature: FeatureAddition::Extrude {
                profile: sketch,
                regions: Vec::new(),
                operation: SolidOperation::Cut,
                reversed: true,
                end_condition: EndCondition::UpToNext,
            },
        },
    ])?;
    // Both regions exit through the bottom: 60 × 40 × 10 − (100 + 400) × 10.
    let volume = 19_000.;
    let mut kernel = OcctKernel::new()?;
    let context = OperationContext::default();
    let cold = rebuild_cold(&doc, &mut kernel, &context)?;
    assert!(cold.all_built(), "{:?}", cold.report());
    let shape = cold.shape(feature).expect("final feature");
    assert!(kernel.is_valid(shape)?);
    assert_eq!(kernel.solid_count(shape)?, 1);
    assert!((kernel.shape_stats(shape)?.1 - volume).abs() < 1e-7);
    let expected = naming::signature(&cold.topology().features[&feature]);
    let mut cache = CacheStore::open(
        dir.path().join("cache"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )?;
    for expected_event in [CacheOutcome::Miss, CacheOutcome::Hit] {
        let (restored, events) = rebuild_cached(&doc, &mut kernel, &mut cache, &context)?;
        assert!(restored.all_built(), "{:?}", restored.report());
        assert!(events.iter().all(|event| event.outcome == expected_event));
        assert_eq!(
            naming::signature(&restored.topology().features[&feature]),
            expected
        );
        let shape = restored.shape(feature).expect("restored feature");
        assert!(kernel.is_valid(shape)?);
        assert!((kernel.shape_stats(shape)?.1 - volume).abs() < 1e-7);
        restored.release_all(&mut kernel);
    }
    cold.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
    Ok(())
}
