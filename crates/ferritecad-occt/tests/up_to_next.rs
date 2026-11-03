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
    let profile = ferritecad_eval::profile_from_sketch(&sketch, ObjectId::new(), plane)?.profile;
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
