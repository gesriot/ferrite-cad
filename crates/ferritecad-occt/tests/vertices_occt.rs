// SPDX-License-Identifier: MIT
//! Topological vertices, against the kernel that ships.
//!
//! A B-Rep vertex is one point of the model and several points of the mesh:
//! every face meeting there carries its own copy. This is about whether the
//! kernel can say which packed positions are which corner, exactly, without
//! comparing coordinates.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::BTreeSet;

use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint, Profile,
    ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane, SubShapeHandle, SubShapeKind,
    TessellationParams,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{Result, StableEntityId};

fn plate() -> Result<ExtrudeRequest> {
    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let points: Vec<PlanarPoint> = corners
        .iter()
        .map(|(x, y)| PlanarPoint::new(*x, *y))
        .collect::<Result<_>>()?;
    let mut segments = Vec::new();
    for (index, start) in points.iter().enumerate() {
        segments.push(ProfileSegment::new(
            StableEntityId::new(),
            SegmentGeometry::line(*start, points[(index + 1) % points.len()])?,
        ));
    }
    Ok(ExtrudeRequest::new(
        Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(segments)?,
            Vec::new(),
        )?,
        ExtrudeExtent::blind(10.0)?,
        false,
    ))
}

#[test]
fn the_eight_corners_of_a_plate_are_each_drawn_wherever_they_appear() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let request = plate().expect("a valid plate");
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("Open CASCADE builds the plate");
    let mesh = kernel
        .tessellate(
            result.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");

    let corners = mesh
        .topological_vertices
        .as_ref()
        .expect("the kernel says which positions are which corner");
    assert_eq!(
        corners.ranges.len(),
        8,
        "a plate has eight topological vertices"
    );

    // Every corner is a vertex of this shape, drawn somewhere, and no two of
    // them are drawn at the same packed position.
    let mut claimed = BTreeSet::new();
    for range in &corners.ranges {
        assert_eq!(range.vertex.kind(), SubShapeKind::Vertex);
        assert_eq!(range.vertex.shape(), result.shape);
        assert!(range.occurrence_count > 0, "a corner drawn nowhere");
        let first = range.first_occurrence as usize;
        let last = first + range.occurrence_count as usize;
        for index in &corners.occurrences[first..last] {
            assert!(
                claimed.insert(*index),
                "two corners claim packed position {index}"
            );
        }
    }

    // Three faces meet at every corner of a box, so each is drawn three times.
    for range in &corners.ranges {
        assert_eq!(
            range.occurrence_count, 3,
            "three faces meet at a corner of a plate"
        );
    }
    assert_eq!(corners.occurrences.len(), 24);

    kernel.release(result.shape);
}

/// Every packed position one corner of a tessellated shape is drawn at.
fn corners_of(mesh: &ferritecad_kernel::Mesh) -> Vec<Vec<u32>> {
    let corners = mesh
        .topological_vertices
        .as_ref()
        .expect("the kernel named the corners");
    corners
        .ranges
        .iter()
        .map(|range| {
            let first = range.first_occurrence as usize;
            let last = first + range.occurrence_count as usize;
            corners.occurrences[first..last].to_vec()
        })
        .collect()
}

/// Where a packed position is, for verifying what topology already claimed.
fn position_at(mesh: &ferritecad_kernel::Mesh, index: u32) -> [f32; 3] {
    let at = index as usize * 3;
    [
        mesh.positions[at],
        mesh.positions[at + 1],
        mesh.positions[at + 2],
    ]
}

#[test]
fn the_two_ends_of_an_edge_are_never_swapped() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let request = plate().expect("a valid plate");
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("builds");
    let mesh = kernel
        .tessellate(
            result.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");

    // Each corner of a 60 x 40 x 10 plate sits at one of the eight box
    // corners, and every position it claims is at that same point. A swapped
    // pair of endpoints would put one corner's occurrences at two different
    // places, which is what this measures. Coordinates verify here; they are
    // not how the association was formed.
    let mut found = Vec::new();
    for occurrences in corners_of(&mesh) {
        assert!(!occurrences.is_empty());
        let first = position_at(&mesh, occurrences[0]);
        for index in &occurrences[1..] {
            let other = position_at(&mesh, *index);
            for axis in 0..3 {
                assert!(
                    (first[axis] - other[axis]).abs() < 1.0e-4,
                    "one corner is drawn at two different points: {first:?} and {other:?}"
                );
            }
        }
        found.push(first);
    }
    assert_eq!(found.len(), 8);

    // And the eight are the eight corners of the box, each once.
    for corner in &found {
        for (axis, extent) in [60.0f32, 40.0, 10.0].iter().enumerate() {
            let at = corner[axis];
            assert!(
                at.abs() < 1.0e-4 || (at - extent).abs() < 1.0e-4,
                "a corner is not at a box corner: {corner:?}"
            );
        }
    }
    for a in 0..found.len() {
        for b in (a + 1)..found.len() {
            let same = (0..3).all(|axis| (found[a][axis] - found[b][axis]).abs() < 1.0e-4);
            assert!(!same, "two corners are at the same point");
        }
    }

    kernel.release(result.shape);
}

#[test]
fn a_curved_shape_names_its_corners_without_inventing_any() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    // A half disc swept: an arc, a chord, and the corners where they meet. Its
    // seam and its curvature must add no corner that is not a B-Rep vertex.
    let arc = ProfileSegment::new(
        StableEntityId::new(),
        SegmentGeometry::arc(PlanarPoint::ORIGIN, 10.0, 0.0, std::f64::consts::PI).expect("valid"),
    );
    let chord = ProfileSegment::new(
        StableEntityId::new(),
        SegmentGeometry::line(
            PlanarPoint::new(-10.0, 0.0).expect("valid"),
            PlanarPoint::new(10.0, 0.0).expect("valid"),
        )
        .expect("valid"),
    );
    let request = ExtrudeRequest::new(
        Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(vec![arc, chord]).expect("a closed loop"),
            Vec::new(),
        )
        .expect("a valid profile"),
        ExtrudeExtent::blind(5.0).expect("valid"),
        false,
    );

    let mut kernel = OcctKernel::new().expect("opens");
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("builds");
    let mesh = kernel
        .tessellate(
            result.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");

    // Four corners: two on each cap, where the arc meets the chord.
    let corners = corners_of(&mesh);
    assert_eq!(corners.len(), 4, "a swept half disc has four corners");

    // Far fewer corners than positions: the tessellation's interior nodes are
    // no B-Rep vertex and must not be named.
    let named: usize = corners.iter().map(Vec::len).sum();
    assert!(
        named < mesh.vertex_count(),
        "every packed position was called a corner"
    );

    // And no two corners share a position.
    let mut claimed = BTreeSet::new();
    for occurrences in &corners {
        for index in occurrences {
            assert!(claimed.insert(*index), "two corners claim position {index}");
        }
    }

    kernel.release(result.shape);
}

#[test]
fn two_identical_shapes_do_not_share_corner_identities() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    // Two plates of identical geometry in one session. Their corners sit at
    // the same eight points and are sixteen different corners, because a
    // corner belongs to the shape it is a corner of.
    //
    // This does not prove the association is topological rather than
    // coordinate-based: measurement found topology and coordinates agreeing on
    // every shape in the corpus, so nothing here can tell the two apart. See
    // the plan for what that leaves open.
    let mut kernel = OcctKernel::new().expect("opens");
    let one = kernel
        .extrude(&plate().expect("valid"), &OperationContext::default())
        .expect("builds");
    let other = kernel
        .extrude(&plate().expect("valid"), &OperationContext::default())
        .expect("builds");

    for shape in [one.shape, other.shape] {
        let mesh = kernel
            .tessellate(
                shape,
                &TessellationParams::default(),
                &OperationContext::default(),
            )
            .expect("meshes");
        let corners = mesh.topological_vertices.as_ref().expect("named");
        assert_eq!(corners.ranges.len(), 8);
        // Every handle belongs to the shape it was asked about, so one plate's
        // corners can never answer for the other's.
        for range in &corners.ranges {
            assert_eq!(range.vertex.shape(), shape);
        }
    }

    kernel.release(one.shape);
    kernel.release(other.shape);
}

#[test]
fn an_assembly_read_from_a_file_names_the_corners_of_every_part() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    // The two assembly files are where the location proved load-bearing:
    // measured on 7.9.3, asking for the polylines without it loses every
    // association both files have, 30 of 30 and 96 of 96. If the bridge ever
    // stops passing it, these shapes are where it shows.
    let mut kernel = OcctKernel::new().expect("opens");
    for name in [
        "01-single-part.step",
        "02-flat-assembly.step",
        "03-nested-assembly.step",
        "05-inch-units.step",
        "07-bare-geometry.step",
    ] {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/step/canonical")
            .join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {name}: {e}"));
        let import = kernel
            .import_step(&bytes)
            .unwrap_or_else(|e| panic!("{name}: import failed: {e}"));
        let Some(scene) = import.scene() else {
            panic!("{name}: the import produced no scene");
        };

        let mut named = 0;
        for shape in scene.shapes() {
            let mesh = kernel
                .tessellate(
                    shape,
                    &TessellationParams::default(),
                    &OperationContext::default(),
                )
                .unwrap_or_else(|e| panic!("{name}: meshing failed: {e}"));
            let corners = mesh
                .topological_vertices
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: the kernel named no corners"));
            // Every part of every one of these files is a solid with corners.
            assert!(
                !corners.ranges.is_empty(),
                "{name}: a part reported no corner at all"
            );
            let mut claimed = BTreeSet::new();
            for range in &corners.ranges {
                assert_eq!(range.vertex.kind(), SubShapeKind::Vertex, "{name}");
                assert_eq!(range.vertex.shape(), shape, "{name}");
                assert!(range.occurrence_count > 0, "{name}");
                let first = range.first_occurrence as usize;
                let last = first + range.occurrence_count as usize;
                for index in &corners.occurrences[first..last] {
                    assert!(
                        (*index as usize) < mesh.vertex_count(),
                        "{name}: a corner addresses a position that is not there"
                    );
                    assert!(claimed.insert(*index), "{name}: two corners claim {index}");
                }
                named += 1;
            }
        }
        assert!(named > 0, "{name}: nothing was named");

        for shape in scene.shapes() {
            kernel.release(shape);
        }
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

/// Every cap vertex the kernel named for one sweep, by side.
fn cap_vertices(result: &ferritecad_kernel::ExtrudeResult) -> (usize, usize) {
    (
        result.start_cap_vertices.len(),
        result.end_cap_vertices.len(),
    )
}

#[test]
fn the_eight_corners_of_a_plate_are_named_by_the_joint_and_the_cap() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let request = plate().expect("a valid plate");
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("Open CASCADE builds the plate");

    let (start, end) = cap_vertices(&result);
    assert_eq!(start, 4, "four corners reach the start cap");
    assert_eq!(end, 4, "four corners reach the end cap");

    // Eight distinct vertices of this shape, and no corner named twice.
    let mut seen = BTreeSet::new();
    for (side, named) in [
        ("start", &result.start_cap_vertices),
        ("end", &result.end_cap_vertices),
    ] {
        for (joint, vertex) in named {
            assert_eq!(vertex.kind(), SubShapeKind::Vertex, "{side} {joint}");
            assert_eq!(vertex.shape(), result.shape, "{side} {joint}");
            assert!(seen.insert(*vertex), "{side} {joint} names a vertex twice");
        }
    }
    assert_eq!(seen.len(), 8);

    kernel.release(result.shape);
}

/// A profile of straight segments through the given planar points.
fn loop_of(points: &[(f64, f64)], height: f64) -> Result<ExtrudeRequest> {
    let planar: Vec<PlanarPoint> = points
        .iter()
        .map(|(x, y)| PlanarPoint::new(*x, *y))
        .collect::<Result<_>>()?;
    let mut segments = Vec::new();
    for (index, start) in planar.iter().enumerate() {
        segments.push(ProfileSegment::new(
            StableEntityId::new(),
            SegmentGeometry::line(*start, planar[(index + 1) % planar.len()])?,
        ));
    }
    Ok(ExtrudeRequest::new(
        Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(segments)?,
            Vec::new(),
        )?,
        ExtrudeExtent::blind(height)?,
        false,
    ))
}

#[test]
fn a_named_cap_vertex_is_on_its_cap_and_ends_its_joints_sweep_edge() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let request = plate().expect("a valid plate");
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("builds");
    let mesh = kernel
        .tessellate(
            result.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");

    // Every corner the tessellation names, so "these eight cover the plate"
    // is checked against the picture rather than asserted.
    let drawn: BTreeSet<_> = mesh
        .topological_vertices
        .as_ref()
        .expect("the kernel named the corners")
        .ranges
        .iter()
        .map(|range| range.vertex)
        .collect();
    assert_eq!(drawn.len(), 8);

    // Which packed positions each corner is drawn at, and which face each
    // position belongs to: everything below is handle identity and index
    // membership, never a coordinate.
    let corners = mesh.topological_vertices.as_ref().expect("named");
    let occurrences = |vertex: SubShapeHandle| -> Vec<u32> {
        corners
            .ranges
            .iter()
            .find(|range| range.vertex == vertex)
            .map(|range| {
                let first = range.first_occurrence as usize;
                let last = first + range.occurrence_count as usize;
                corners.occurrences[first..last].to_vec()
            })
            .unwrap_or_default()
    };
    let face_positions = |face: SubShapeHandle| -> BTreeSet<u32> {
        mesh.faces
            .iter()
            .filter(|range| range.face == face)
            .flat_map(|range| {
                let first = range.first_index as usize;
                let last = first + range.index_count as usize;
                mesh.indices[first..last].iter().copied()
            })
            .collect()
    };
    let edge_positions = |edge: SubShapeHandle| -> BTreeSet<u32> {
        mesh.edges
            .as_ref()
            .map(|edges| {
                edges
                    .ranges
                    .iter()
                    .filter(|range| range.edge == edge)
                    .flat_map(|range| {
                        let first = range.first_segment as usize * 2;
                        let last = first + range.segment_count as usize * 2;
                        edges.segments[first..last].iter().copied()
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    let mut named = BTreeSet::new();
    for (side, entries, caps) in [
        ("start", &result.start_cap_vertices, &result.start_cap),
        ("end", &result.end_cap_vertices, &result.end_cap),
    ] {
        for (joint, vertex) in entries {
            named.insert(*vertex);
            // Reached by the tessellation association, by handle identity.
            assert!(
                drawn.contains(vertex),
                "{side} {joint} names a vertex the mesh never reaches"
            );
            let mine = occurrences(*vertex);
            assert!(!mine.is_empty(), "{side} {joint} is drawn nowhere");

            // On the cap it is claimed for: one of its occurrences belongs to
            // a face this result calls that cap.
            let on_cap = caps.iter().any(|cap| {
                let positions = face_positions(*cap);
                mine.iter().any(|at| positions.contains(at))
            });
            assert!(on_cap, "{side} {joint} names a vertex not on that cap");

            // And it ends the edge swept from its own joint: one of its
            // occurrences is a position that edge is drawn through.
            let swept = result
                .sweep_edges
                .get(joint)
                .unwrap_or_else(|| panic!("{joint} swept no edge"));
            assert_eq!(swept.kind(), SubShapeKind::Edge);
            assert_eq!(swept.shape(), result.shape);
            let along = edge_positions(*swept);
            assert!(
                mine.iter().any(|at| along.contains(at)),
                "{side} {joint} names a vertex that does not end its own sweep edge"
            );
        }
    }
    assert_eq!(named, drawn, "the eight names do not cover the plate");

    // Start and End of one joint are different vertices and cannot be swapped.
    for joint in result.start_cap_vertices.keys() {
        let start = result.start_cap_vertices[joint];
        let end = result.end_cap_vertices[joint];
        assert_ne!(start, end, "{joint} named one vertex on both caps");
    }

    kernel.release(result.shape);
}

#[test]
fn every_way_of_sweeping_a_profile_keeps_the_side_meanings() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let base = plate().expect("a valid plate");
    let profile = base.profile().clone();

    let mut answers = Vec::new();
    for (what, extent, reversed) in [
        ("blind", ExtrudeExtent::blind(10.0).expect("valid"), false),
        ("reversed", ExtrudeExtent::blind(10.0).expect("valid"), true),
        (
            "symmetric",
            ExtrudeExtent::symmetric(10.0).expect("valid"),
            false,
        ),
    ] {
        let request = ExtrudeRequest::new(profile.clone(), extent, reversed);
        let result = kernel
            .extrude(&request, &OperationContext::default())
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(result.start_cap_vertices.len(), 4, "{what}");
        assert_eq!(result.end_cap_vertices.len(), 4, "{what}");
        // The two sides never name one vertex, however the sweep ran.
        for joint in result.start_cap_vertices.keys() {
            assert_ne!(
                result.start_cap_vertices[joint], result.end_cap_vertices[joint],
                "{what}: {joint} named one vertex on both caps"
            );
        }
        let joints: BTreeSet<_> = result.start_cap_vertices.keys().copied().collect();
        answers.push((what, joints));
        kernel.release(result.shape);
    }
    let (_, first) = &answers[0];
    for (what, joints) in &answers {
        assert_eq!(joints, first, "{what} named a different set of joints");
    }
}

#[test]
fn a_triangle_names_six_and_an_arc_profile_names_all_its_unique_joints() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");

    let triangle = loop_of(&[(0.0, 0.0), (30.0, 0.0), (10.0, 25.0)], 10.0).expect("valid");
    let result = kernel
        .extrude(&triangle, &OperationContext::default())
        .expect("builds");
    assert_eq!(result.start_cap_vertices.len(), 3);
    assert_eq!(result.end_cap_vertices.len(), 3);
    kernel.release(result.shape);

    // Three segments, one of them curved, blind and symmetric.
    for extent in [
        ExtrudeExtent::blind(10.0).expect("valid"),
        ExtrudeExtent::symmetric(5.0).expect("valid"),
    ] {
        let labels = [
            StableEntityId::new(),
            StableEntityId::new(),
            StableEntityId::new(),
        ];
        let arc = ProfileSegment::new(
            labels[0],
            SegmentGeometry::arc(PlanarPoint::ORIGIN, 10.0, 0.0, std::f64::consts::PI)
                .expect("valid"),
        );
        let down = ProfileSegment::new(
            labels[1],
            SegmentGeometry::line(
                PlanarPoint::new(-10.0, 0.0).expect("valid"),
                PlanarPoint::new(0.0, -20.0).expect("valid"),
            )
            .expect("valid"),
        );
        let up = ProfileSegment::new(
            labels[2],
            SegmentGeometry::line(
                PlanarPoint::new(0.0, -20.0).expect("valid"),
                PlanarPoint::new(10.0, 0.0).expect("valid"),
            )
            .expect("valid"),
        );
        let request = ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(vec![arc, down, up]).expect("a closed loop"),
                Vec::new(),
            )
            .expect("valid"),
            extent,
            false,
        );
        let result = kernel
            .extrude(&request, &OperationContext::default())
            .expect("builds");
        assert_eq!(result.start_cap_vertices.len(), 3, "every joint is unique");
        assert_eq!(result.end_cap_vertices.len(), 3);
        kernel.release(result.shape);
    }
}

#[test]
fn a_pair_that_meets_at_two_corners_names_neither_cap_vertex() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    // A half disc: the arc and the chord meet twice, so their one unordered
    // pair names two corners and therefore neither. The bridge still sees four
    // perfectly distinct vertices, which is why the refusal has to live here.
    let arc_label = StableEntityId::new();
    let chord_label = StableEntityId::new();
    let request = ExtrudeRequest::new(
        Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(vec![
                ProfileSegment::new(
                    arc_label,
                    SegmentGeometry::arc(PlanarPoint::ORIGIN, 10.0, 0.0, std::f64::consts::PI)
                        .expect("valid"),
                ),
                ProfileSegment::new(
                    chord_label,
                    SegmentGeometry::line(
                        PlanarPoint::new(-10.0, 0.0).expect("valid"),
                        PlanarPoint::new(10.0, 0.0).expect("valid"),
                    )
                    .expect("valid"),
                ),
            ])
            .expect("a closed loop"),
            Vec::new(),
        )
        .expect("valid"),
        ExtrudeExtent::blind(5.0).expect("valid"),
        false,
    );

    let mut kernel = OcctKernel::new().expect("opens");
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("builds");

    assert!(
        result.start_cap_vertices.is_empty(),
        "an ambiguous pair named a start cap vertex"
    );
    assert!(
        result.end_cap_vertices.is_empty(),
        "an ambiguous pair named an end cap vertex"
    );
    // And the cap edges of the same shape are still named, so the refusal is
    // about this pair rather than about the shape being unnameable.
    assert_eq!(result.start_cap_edges.len(), 2);

    kernel.release(result.shape);
}

#[test]
fn reordering_the_profile_keeps_each_joint_owning_its_cap_vertices() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let planar: Vec<PlanarPoint> = corners
        .iter()
        .map(|(x, y)| PlanarPoint::new(*x, *y).expect("valid"))
        .collect();
    // One set of labels, reused, so the joints mean the same thing however the
    // loop is walked.
    let labels: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();

    let mut owners: Vec<BTreeSet<ferritecad_types::ProfileJoint>> = Vec::new();
    for start in 0..4 {
        let mut segments = Vec::new();
        for step in 0..4 {
            let index = (start + step) % 4;
            segments.push(ProfileSegment::new(
                labels[index],
                SegmentGeometry::line(planar[index], planar[(index + 1) % 4]).expect("valid"),
            ));
        }
        let request = ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(segments).expect("a closed loop"),
                Vec::new(),
            )
            .expect("valid"),
            ExtrudeExtent::blind(10.0).expect("valid"),
            false,
        );
        let result = kernel
            .extrude(&request, &OperationContext::default())
            .expect("builds");
        assert_eq!(result.start_cap_vertices.len(), 4, "start at {start}");
        owners.push(result.start_cap_vertices.keys().copied().collect());
        kernel.release(result.shape);
    }

    for (start, joints) in owners.iter().enumerate() {
        assert_eq!(
            joints, &owners[0],
            "starting the loop at segment {start} changed which joints own cap vertices"
        );
    }
}
