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
    ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane, SubShapeKind, TessellationParams,
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
