// SPDX-License-Identifier: MIT
//! Triangles, and which face each of them belongs to.
//!
//! A mesh is the first thing in this project a person will actually look at,
//! and the ways it goes wrong are quiet: a solid lit from inside, a face whose
//! triangles are filed under its neighbour, a picture that changes between two
//! runs of the same build. Each of those is checked here against arithmetic
//! that does not depend on Open CASCADE agreeing with itself.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::{
    collections::{BTreeMap, BTreeSet},
    f64::consts::PI,
};

use ferritecad_document::CapSide;
use ferritecad_kernel::{
    CancelToken, ExtrudeExtent, ExtrudeRequest, GeometryKernel, Mesh, OperationContext,
    PlanarPoint, Profile, ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane,
    SubShapeHandle, SubShapeKind, TessellationParams,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_topology::{TopologyMap, resolve};
use ferritecad_types::{ErrorKind, ObjectId, Result, StableEntityId};

const WIDTH: f64 = 60.0;
const DEPTH: f64 = 40.0;
const HEIGHT: f64 = 10.0;

fn plate() -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
    let corners = [(0.0, 0.0), (WIDTH, 0.0), (WIDTH, DEPTH), (0.0, DEPTH)];
    let points: Vec<PlanarPoint> = corners
        .iter()
        .map(|(x, y)| PlanarPoint::new(*x, *y))
        .collect::<Result<_>>()?;

    let mut segments = Vec::new();
    let mut labels = Vec::new();
    for (index, start) in points.iter().enumerate() {
        let label = StableEntityId::new();
        labels.push(label);
        segments.push(ProfileSegment::new(
            label,
            SegmentGeometry::line(*start, points[(index + 1) % points.len()])?,
        ));
    }

    Ok((
        ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(segments)?,
                Vec::new(),
            )?,
            ExtrudeExtent::blind(HEIGHT)?,
            false,
        ),
        labels,
    ))
}

/// A curved face whose triangle count makes deflection reuse observable.
fn curved() -> Result<ExtrudeRequest> {
    let arc = ProfileSegment::new(
        StableEntityId::new(),
        SegmentGeometry::arc(PlanarPoint::ORIGIN, 10.0, 0.0, PI)?,
    );
    let diameter = ProfileSegment::new(
        StableEntityId::new(),
        SegmentGeometry::line(PlanarPoint::new(-10.0, 0.0)?, PlanarPoint::new(10.0, 0.0)?)?,
    );
    Ok(ExtrudeRequest::new(
        Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(vec![arc, diameter])?,
            Vec::new(),
        )?,
        ExtrudeExtent::blind(5.0)?,
        false,
    ))
}

/// The area of one face's triangles, and their area-weighted centroid.
fn measure(mesh: &Mesh, face: SubShapeHandle) -> (f64, [f64; 3]) {
    let range = mesh
        .faces
        .iter()
        .find(|range| range.face == face)
        .expect("the mesh names this face");

    let point = |index: u32| -> [f64; 3] {
        let at = index as usize * 3;
        [
            f64::from(mesh.positions[at]),
            f64::from(mesh.positions[at + 1]),
            f64::from(mesh.positions[at + 2]),
        ]
    };

    let mut area = 0.0;
    let mut centroid = [0.0; 3];
    let first = range.first_index as usize;
    for triangle in mesh.indices[first..first + range.index_count as usize].chunks_exact(3) {
        let (a, b, c) = (point(triangle[0]), point(triangle[1]), point(triangle[2]));
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let double = (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt();
        area += double / 2.0;
        for axis in 0..3 {
            centroid[axis] += (a[axis] + b[axis] + c[axis]) / 3.0 * (double / 2.0);
        }
    }

    for value in &mut centroid {
        *value /= area;
    }
    (area, centroid)
}

/// The outward normal a face's vertices agree on, if they agree.
fn normal(mesh: &Mesh, face: SubShapeHandle) -> [f64; 3] {
    let range = mesh
        .faces
        .iter()
        .find(|range| range.face == face)
        .expect("the mesh names this face");
    let first = range.first_index as usize;
    let vertices: Vec<u32> = mesh.indices[first..first + range.index_count as usize].to_vec();

    let read = |index: u32| -> [f64; 3] {
        let at = index as usize * 3;
        [
            f64::from(mesh.normals[at]),
            f64::from(mesh.normals[at + 1]),
            f64::from(mesh.normals[at + 2]),
        ]
    };

    let first_normal = read(vertices[0]);
    for index in &vertices {
        let other = read(*index);
        for axis in 0..3 {
            assert!(
                (other[axis] - first_normal[axis]).abs() < 1e-5,
                "a planar face's vertices should share one normal"
            );
        }
    }
    first_normal
}

struct Built {
    kernel: OcctKernel,
    map: TopologyMap,
    feature: ObjectId,
    segments: Vec<StableEntityId>,
    shape: ferritecad_kernel::ShapeHandle,
}

fn build() -> Built {
    let (request, segments) = plate().expect("a valid plate");
    let feature = ObjectId::new();
    let mut kernel = OcctKernel::new().expect("opens");
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("builds");
    let mut map = TopologyMap::new();
    map.record_extrude(feature, request.profile(), &result)
        .expect("records");
    let shape = result.shape;
    Built {
        kernel,
        map,
        feature,
        segments,
        shape,
    }
}

fn face_of(built: &Built, role: ferritecad_document::SemanticRole) -> SubShapeHandle {
    use ferritecad_document::{EntityKind, SelectionRule, TopologyRef};
    let selection = match role {
        ferritecad_document::SemanticRole::ExtrudeSide { profile_segment } => {
            SelectionRule::AllDerivedFrom {
                ancestor: profile_segment,
            }
        }
        _ => SelectionRule::Exact,
    };
    let found = resolve(
        &built.map,
        &TopologyRef {
            id: StableEntityId::new(),
            owner: built.feature,
            producer_feature: built.feature,
            expected_kind: EntityKind::Face,
            output_role: role,
            selection,
            fallback_signature: None,
        },
    )
    .expect("the name resolves");
    assert_eq!(found.len(), 1);
    found[0]
}

#[test]
fn every_named_face_gets_the_area_and_place_it_should_have() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    use ferritecad_document::SemanticRole;
    let mut built = build();
    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("Open CASCADE meshes the plate");

    assert_eq!(mesh.faces.len(), 6, "a box has six faces");
    assert_eq!(mesh.triangle_count(), 12, "each face is two triangles");

    // Caps: the full rectangle, one at each end, facing away from the solid.
    for (side, z, direction) in [(CapSide::Start, 0.0, -1.0), (CapSide::End, HEIGHT, 1.0)] {
        let face = face_of(&built, SemanticRole::ExtrudeCap { side });
        let (area, centroid) = measure(&mesh, face);
        assert!(
            (area - WIDTH * DEPTH).abs() < 1e-6,
            "the {side:?} cap should be {} mm^2, measured {area}",
            WIDTH * DEPTH
        );
        assert!(
            (centroid[2] - z).abs() < 1e-9,
            "the {side:?} cap sits at z={z}"
        );
        assert!(
            (normal(&mesh, face)[2] - direction).abs() < 1e-5,
            "the {side:?} cap must face out of the solid, not into it"
        );
    }

    // Sides: alternating 60 x 10 and 40 x 10, each facing away from the centre.
    let expected = [
        WIDTH * HEIGHT,
        DEPTH * HEIGHT,
        WIDTH * HEIGHT,
        DEPTH * HEIGHT,
    ];
    for (index, segment) in built.segments.iter().enumerate() {
        let face = face_of(
            &built,
            SemanticRole::ExtrudeSide {
                profile_segment: *segment,
            },
        );
        let (area, centroid) = measure(&mesh, face);
        assert!(
            (area - expected[index]).abs() < 1e-6,
            "side {index} should be {} mm^2, measured {area}",
            expected[index]
        );
        assert!((centroid[2] - HEIGHT / 2.0).abs() < 1e-9);

        // Outward means pointing away from the solid's axis.
        let outward = normal(&mesh, face);
        let to_centre = [WIDTH / 2.0 - centroid[0], DEPTH / 2.0 - centroid[1], 0.0];
        let dot: f64 = (0..3).map(|axis| outward[axis] * to_centre[axis]).sum();
        assert!(dot < 0.0, "side {index} faces inward");
    }

    built.kernel.release(built.shape);
}

#[test]
fn each_triangle_belongs_to_exactly_one_face() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut built = build();
    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");

    let mut owner: BTreeMap<usize, SubShapeHandle> = BTreeMap::new();
    for range in &mesh.faces {
        assert!(
            range.index_count > 0,
            "an empty range names an invisible face"
        );
        assert!(range.index_count.is_multiple_of(3));
        let first = range.first_index as usize;
        for triangle in (first..first + range.index_count as usize).step_by(3) {
            assert!(
                owner.insert(triangle, range.face).is_none(),
                "triangle at {triangle} was claimed twice"
            );
        }
    }
    assert_eq!(
        owner.len(),
        mesh.triangle_count(),
        "every triangle belongs to exactly one face"
    );

    // A vertex is never shared between faces, so one face can be drawn alone.
    let mut seen: BTreeMap<u32, SubShapeHandle> = BTreeMap::new();
    for range in &mesh.faces {
        let first = range.first_index as usize;
        for index in &mesh.indices[first..first + range.index_count as usize] {
            if let Some(other) = seen.insert(*index, range.face) {
                assert_eq!(other, range.face, "vertex {index} is shared between faces");
            }
        }
    }

    built.kernel.release(built.shape);
}

#[test]
fn a_finer_deflection_is_the_same_shape_measured_more_closely() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut built = build();
    let context = OperationContext::default();
    let coarse = built
        .kernel
        .tessellate(built.shape, &TessellationParams::default(), &context)
        .expect("meshes");
    let fine = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::new(0.001, 0.1, false).expect("valid"),
            &context,
        )
        .expect("meshes finely");

    // The plate is flat, so the areas must agree exactly whatever the chord
    // tolerance; what a finer setting must never do is change the answer.
    use ferritecad_document::SemanticRole;
    let face = face_of(&built, SemanticRole::ExtrudeCap { side: CapSide::End });
    let (coarse_area, _) = measure(&coarse, face);
    let (fine_area, _) = measure(&fine, face);
    assert!((coarse_area - fine_area).abs() < 1e-6);

    built.kernel.release(built.shape);
}

#[test]
fn a_coarse_request_does_not_reuse_an_earlier_fine_mesh() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let request = curved().expect("a curved profile");
    let fine = TessellationParams::new(0.001, 0.05, false).expect("fine parameters");
    let coarse = TessellationParams::new(2.0, 1.0, false).expect("coarse parameters");
    let context = OperationContext::default();

    let mut reused = OcctKernel::new().expect("opens");
    let reused_shape = reused.extrude(&request, &context).expect("builds").shape;
    let fine_mesh = reused
        .tessellate(reused_shape, &fine, &context)
        .expect("meshes finely");
    let coarse_after_fine = reused
        .tessellate(reused_shape, &coarse, &context)
        .expect("remeshes coarsely");

    let mut fresh = OcctKernel::new().expect("opens another session");
    let fresh_shape = fresh.extrude(&request, &context).expect("builds").shape;
    let coarse_fresh = fresh
        .tessellate(fresh_shape, &coarse, &context)
        .expect("meshes coarsely from scratch");

    assert!(
        fine_mesh.triangle_count() > coarse_fresh.triangle_count(),
        "the curved fixture must distinguish fine from coarse"
    );
    assert_eq!(
        coarse_after_fine.positions, coarse_fresh.positions,
        "the same request must not depend on an earlier tessellation"
    );
    assert_eq!(coarse_after_fine.normals, coarse_fresh.normals);
    assert_eq!(coarse_after_fine.indices, coarse_fresh.indices);

    reused.release(reused_shape);
    fresh.release(fresh_shape);
}

#[test]
fn the_same_shape_meshes_the_same_way_twice() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut built = build();
    let context = OperationContext::default();
    let once = built
        .kernel
        .tessellate(built.shape, &TessellationParams::default(), &context)
        .expect("meshes");
    let twice = built
        .kernel
        .tessellate(built.shape, &TessellationParams::default(), &context)
        .expect("meshes again");

    assert_eq!(once, twice, "a picture that changes between runs is a bug");
    built.kernel.release(built.shape);
}

#[test]
fn a_cancelled_tessellation_produces_no_mesh() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut built = build();
    let cancel = CancelToken::new();
    cancel.cancel();

    let err = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default().with_cancel(cancel),
        )
        .expect_err("a cancelled tessellation has no result");
    assert_eq!(err.kind(), ErrorKind::Cancellation);

    built.kernel.release(built.shape);
    assert_eq!(built.kernel.live_shape_count(), 0);
}

#[test]
fn drawing_does_not_change_the_shape_that_is_archived() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    // Open CASCADE keeps triangulation on the shape and reuses it across calls.
    // The bridge cleans that transient state before and after drawing, so the
    // requested picture cannot affect either later parameters or persistence.
    let mut built = build();
    let before = built.kernel.encode_shape(built.shape).expect("encodes");
    let (before_faces, before_volume) = built.kernel.shape_stats(built.shape).expect("measures");

    built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");

    let after = built
        .kernel
        .encode_shape(built.shape)
        .expect("encodes again");
    assert_eq!(
        before.bytes(),
        after.bytes(),
        "drawing transient data must not change the persisted B-Rep"
    );
    let restored = built.kernel.decode_shape(&after).expect("decodes");
    let (faces, volume) = built.kernel.shape_stats(restored).expect("measures");

    assert_eq!(faces, before_faces);
    assert!((volume - before_volume).abs() < 1e-9);
    // And so does the one written before it was ever drawn.
    let earlier = built.kernel.decode_shape(&before).expect("decodes");
    let (earlier_faces, earlier_volume) = built.kernel.shape_stats(earlier).expect("measures");
    assert_eq!(earlier_faces, before_faces);
    assert!((earlier_volume - before_volume).abs() < 1e-9);

    built.kernel.release(earlier);
    built.kernel.release(restored);
    built.kernel.release(built.shape);
    assert_eq!(built.kernel.live_shape_count(), 0);
}

#[test]
fn a_restored_solid_draws_the_same_as_the_one_that_was_built() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    use ferritecad_document::SemanticRole;
    use ferritecad_topology::{archive_feature, restore_feature};

    // What a warm rebuild must guarantee about pictures: the solid that comes
    // out of the cache is drawn exactly as the one that went in. Handles
    // differ between sessions, so the comparison is by name and by geometry.
    let mut built = build();
    let context = OperationContext::default();
    let fresh = built
        .kernel
        .tessellate(built.shape, &TessellationParams::default(), &context)
        .expect("meshes");

    let mut names = Vec::new();
    for side in [CapSide::Start, CapSide::End] {
        names.push((
            format!("cap {side:?}"),
            measure(&fresh, face_of(&built, SemanticRole::ExtrudeCap { side })),
        ));
    }
    for segment in &built.segments {
        names.push((
            format!("side {segment}"),
            measure(
                &fresh,
                face_of(
                    &built,
                    SemanticRole::ExtrudeSide {
                        profile_segment: *segment,
                    },
                ),
            ),
        ));
    }

    let archived = archive_feature(&mut built.kernel, &built.map, built.feature).expect("archives");
    built.kernel.release(built.shape);

    let mut reader = OcctKernel::new().expect("opens");
    let mut restored_map = TopologyMap::new();
    restore_feature(&mut reader, &archived, &mut restored_map).expect("restores");
    let shape = restored_map
        .feature(built.feature)
        .and_then(|names| names.shape())
        .expect("a restored shape");
    let restored_mesh = reader
        .tessellate(shape, &TessellationParams::default(), &context)
        .expect("meshes the restored solid");

    assert_eq!(restored_mesh.triangle_count(), fresh.triangle_count());
    assert_eq!(restored_mesh.faces.len(), fresh.faces.len());

    let restored_built = Built {
        kernel: reader,
        map: restored_map,
        feature: built.feature,
        segments: built.segments.clone(),
        shape,
    };
    for (label, (area, centroid)) in names {
        let face = if let Some(rest) = label.strip_prefix("side ") {
            let segment: StableEntityId = rest.parse().expect("a segment id");
            face_of(
                &restored_built,
                SemanticRole::ExtrudeSide {
                    profile_segment: segment,
                },
            )
        } else {
            let side = if label.contains("Start") {
                CapSide::Start
            } else {
                CapSide::End
            };
            face_of(&restored_built, SemanticRole::ExtrudeCap { side })
        };

        let (restored_area, restored_centroid) = measure(&restored_mesh, face);
        assert!(
            (area - restored_area).abs() < 1e-9,
            "{label} was {area} mm^2 and is now {restored_area}"
        );
        for axis in 0..3 {
            assert!(
                (centroid[axis] - restored_centroid[axis]).abs() < 1e-9,
                "{label} moved on axis {axis}"
            );
        }
    }

    let mut reader = restored_built.kernel;
    reader.release(shape);
    assert_eq!(reader.live_shape_count(), 0);
}

/// The distinct positions the segments of one edge touch, and how many
/// segments there are.
///
/// Nothing here associates anything: the association arrives from the kernel,
/// and this only measures what arrived. Positions are compared exactly,
/// because two vertices of one tessellation that name one corner of a box
/// carry the same floats; a tolerance here would hide the very smearing the
/// association exists to avoid.
fn corners_of(mesh: &Mesh, range: &ferritecad_kernel::MeshEdgeRange) -> Vec<[f32; 3]> {
    let edges = mesh.edges.as_ref().expect("the mesh associates its edges");
    let first = range.first_segment as usize * 2;
    let end = first + range.segment_count as usize * 2;
    let mut seen: Vec<[f32; 3]> = Vec::new();
    for index in &edges.segments[first..end] {
        let at = *index as usize * 3;
        let point = [
            mesh.positions[at],
            mesh.positions[at + 1],
            mesh.positions[at + 2],
        ];
        if !seen.contains(&point) {
            seen.push(point);
        }
    }
    seen
}

#[test]
fn every_edge_of_the_plate_is_named_and_drawn_from_both_of_its_faces() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut built = build();
    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");

    let edges = mesh
        .edges
        .as_ref()
        .expect("Open CASCADE knows which topological edge each segment draws");

    // A box has twelve edges, and every one of them must be here. An
    // association that covered only the edges it found a polygon for would
    // draw part of the wireframe and silently omit the rest.
    assert_eq!(
        edges.ranges.len(),
        12,
        "a 60 x 40 x 10 plate has twelve topological edges"
    );

    // Which face each vertex belongs to. The tessellation gives each face its
    // own nodes, so this is exact.
    let mut face_of: BTreeMap<u32, SubShapeHandle> = BTreeMap::new();
    for range in &mesh.faces {
        let first = range.first_index as usize;
        for index in &mesh.indices[first..first + range.index_count as usize] {
            face_of.insert(*index, range.face);
        }
    }

    let mut lengths: Vec<i64> = Vec::new();
    for range in &edges.ranges {
        assert_eq!(
            range.edge.shape(),
            built.shape,
            "an edge of the mesh belongs to the shape that was tessellated"
        );

        // One topological edge of a box is shared by two faces, and each of
        // them draws it from its own triangulation. Two representations, one
        // identity: the handles must not have been split by orientation.
        let first = range.first_segment as usize * 2;
        let end = first + range.segment_count as usize * 2;
        let faces: BTreeSet<SubShapeHandle> = edges.segments[first..end]
            .iter()
            .map(|index| *face_of.get(index).expect("a segment vertex is a face's"))
            .collect();
        assert_eq!(
            faces.len(),
            2,
            "edge {} of a box is drawn from both of the faces that meet at it",
            range.edge
        );

        // The plate's edges are straight, so each of them is one segment per
        // side and touches exactly the two corners it runs between.
        let corners = corners_of(&mesh, range);
        assert_eq!(
            corners.len(),
            2,
            "a straight edge of the plate runs between two corners, found {corners:?}"
        );
        let length = ((f64::from(corners[0][0]) - f64::from(corners[1][0])).powi(2)
            + (f64::from(corners[0][1]) - f64::from(corners[1][1])).powi(2)
            + (f64::from(corners[0][2]) - f64::from(corners[1][2])).powi(2))
        .sqrt();
        lengths.push(length.round() as i64);
    }

    // Four edges of each dimension, and no edge of any other length. A
    // wireframe assembled by proximity would fail here the moment two corners
    // were joined that the topology does not join.
    lengths.sort_unstable();
    assert_eq!(
        lengths,
        vec![10, 10, 10, 10, 40, 40, 40, 40, 60, 60, 60, 60],
        "the twelve edges are the twelve sides of a 60 x 40 x 10 box"
    );

    built.kernel.release(built.shape);
}

#[test]
fn a_curved_edge_is_many_segments_and_still_one_edge() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let request = curved().expect("a valid half cylinder");
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("builds");
    let shape = result.shape;
    let mesh = kernel
        .tessellate(
            shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");

    let edges = mesh.edges.as_ref().expect("the association is there");

    // A half disc swept once: two arcs, two straight sides of the caps, and
    // two vertical edges where the flat side meets the curved one.
    assert_eq!(edges.ranges.len(), 6, "six topological edges");

    // The arc is drawn by many segments and is one edge. Split by orientation
    // it would be two edges of half the segments each; welded by proximity it
    // would be one edge whose segments came from a single face.
    let longest = edges
        .ranges
        .iter()
        .max_by_key(|range| range.segment_count)
        .expect("there is an edge");
    assert!(
        longest.segment_count > 8,
        "an arc at the default deflection is many segments, got {}",
        longest.segment_count
    );

    let mut face_of: BTreeMap<u32, SubShapeHandle> = BTreeMap::new();
    for range in &mesh.faces {
        let first = range.first_index as usize;
        for index in &mesh.indices[first..first + range.index_count as usize] {
            face_of.insert(*index, range.face);
        }
    }
    for range in &edges.ranges {
        let first = range.first_segment as usize * 2;
        let end = first + range.segment_count as usize * 2;
        let faces: BTreeSet<SubShapeHandle> = edges.segments[first..end]
            .iter()
            .map(|index| *face_of.get(index).expect("a segment vertex is a face's"))
            .collect();
        assert_eq!(
            faces.len(),
            2,
            "edge {} is drawn from both of the faces that meet at it",
            range.edge
        );
        assert_eq!(range.edge.shape(), shape);
    }

    kernel.release(shape);
}

#[test]
fn the_same_shape_names_the_same_edges_twice() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut built = build();
    let once = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");
    let again = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes again");

    // Identity included: the same topological edge keeps the same handle
    // across two tessellations of one shape, which is what lets a caller ask
    // about an edge it learned of during an earlier draw.
    assert_eq!(once.edges, again.edges);

    built.kernel.release(built.shape);
}

/// A durable reference to the edge where one cap of an extrusion meets the
/// face raised from one profile segment.
fn cap_edge_reference(
    feature: ObjectId,
    side: ferritecad_document::CapSide,
    segment: StableEntityId,
) -> ferritecad_document::TopologyRef {
    ferritecad_document::TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: ferritecad_document::EntityKind::Edge,
        output_role: ferritecad_document::SemanticRole::ExtrudeCapEdge {
            side,
            profile_segment: segment,
        },
        selection: ferritecad_document::SelectionRule::Exact,
        fallback_signature: None,
    }
}

#[test]
fn every_profile_segment_names_its_two_cap_boundary_edges() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut built = build();
    let map = &built.map;
    let feature = built.feature;

    // The plate has four stable profile segments. Each of them must name one
    // edge on the start cap and one on the end cap, and all eight must be
    // different edges of the one shape.
    let mut named = BTreeSet::new();
    for segment in &built.segments {
        for side in [
            ferritecad_document::CapSide::Start,
            ferritecad_document::CapSide::End,
        ] {
            let reference = cap_edge_reference(feature, side, *segment);
            let resolved = resolve(map, &reference).unwrap_or_else(|error| {
                panic!("the {side:?} cap edge of segment {segment} is not named: {error}")
            });
            assert_eq!(resolved.len(), 1, "one edge, not {}", resolved.len());
            let edge = resolved[0];
            assert_eq!(edge.kind(), SubShapeKind::Edge);
            assert_eq!(edge.shape(), built.shape);
            assert!(named.insert(edge), "{edge} was named twice");
        }
    }
    assert_eq!(named.len(), 8, "four segments, two caps, eight edges");

    // And every one of them is an edge the tessellation reports, so a name and
    // a drawn line are the same edge rather than two things that resemble one.
    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");
    let drawn: BTreeSet<SubShapeHandle> = mesh
        .edges
        .as_ref()
        .expect("the association is there")
        .ranges
        .iter()
        .map(|range| range.edge)
        .collect();
    for edge in &named {
        assert!(drawn.contains(edge), "{edge} is named but never drawn");
    }

    built.kernel.release(built.shape);
}

/// One extrusion of a profile, with whatever extent is asked for, and the map
/// of what it produced.
fn swept(extent: ExtrudeExtent, reversed: bool, arc: bool) -> (Built, ExtrudeRequest) {
    let (request, segments) = if arc {
        let (request, labels) = arc_profile().expect("a valid half disc");
        (request, labels)
    } else {
        plate().expect("a valid plate")
    };
    let request = ExtrudeRequest::new(request.profile().clone(), extent, reversed);
    let feature = ObjectId::new();
    let mut kernel = OcctKernel::new().expect("opens");
    let result = kernel
        .extrude(&request, &OperationContext::default())
        .expect("builds");
    let mut map = TopologyMap::new();
    map.record_extrude(feature, request.profile(), &result)
        .expect("records");
    let shape = result.shape;
    (
        Built {
            kernel,
            map,
            feature,
            segments,
            shape,
        },
        request,
    )
}

/// A half disc: one arc and one chord, each with a stable label.
fn arc_profile() -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
    let arc_label = StableEntityId::new();
    let chord_label = StableEntityId::new();
    let arc = ProfileSegment::new(
        arc_label,
        SegmentGeometry::arc(PlanarPoint::ORIGIN, 10.0, 0.0, PI)?,
    );
    let chord = ProfileSegment::new(
        chord_label,
        SegmentGeometry::line(PlanarPoint::new(-10.0, 0.0)?, PlanarPoint::new(10.0, 0.0)?)?,
    );
    Ok((
        ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(vec![arc, chord])?,
                Vec::new(),
            )?,
            ExtrudeExtent::blind(5.0)?,
            false,
        ),
        vec![arc_label, chord_label],
    ))
}

/// The one edge a cap-edge reference resolves to, or the failure.
fn cap_edge_of(
    built: &Built,
    side: ferritecad_document::CapSide,
    segment: StableEntityId,
) -> Result<SubShapeHandle> {
    let resolved = resolve(
        &built.map,
        &cap_edge_reference(built.feature, side, segment),
    )?;
    assert_eq!(resolved.len(), 1, "an exact reference selects one edge");
    Ok(resolved[0])
}

#[test]
fn the_two_ends_of_a_sweep_are_never_confused_for_each_other() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    let mut built = build();
    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");
    let edges = mesh.edges.as_ref().expect("the association is there");

    // Which face each vertex belongs to. A tessellation gives every face its
    // own nodes, so this is exact.
    let mut face_of: BTreeMap<u32, SubShapeHandle> = BTreeMap::new();
    for range in &mesh.faces {
        let first = range.first_index as usize;
        for index in &mesh.indices[first..first + range.index_count as usize] {
            face_of.insert(*index, range.face);
        }
    }
    // Whether a named edge is drawn from the vertices of a named face, which
    // is what "this edge bounds that cap" means once both have names.
    let bounds = |edge: SubShapeHandle, face: SubShapeHandle| {
        let range = edges
            .ranges
            .iter()
            .find(|range| range.edge == edge)
            .expect("a named edge is drawn");
        let first = range.first_segment as usize * 2;
        let end = first + range.segment_count as usize * 2;
        edges.segments[first..end]
            .iter()
            .any(|vertex| face_of.get(vertex).copied() == Some(face))
    };

    let start_cap = face_of_side(&built, ferritecad_document::CapSide::Start);
    let end_cap = face_of_side(&built, ferritecad_document::CapSide::End);
    assert_ne!(start_cap, end_cap);

    for segment in &built.segments {
        let start =
            cap_edge_of(&built, ferritecad_document::CapSide::Start, *segment).expect("named");
        let end = cap_edge_of(&built, ferritecad_document::CapSide::End, *segment).expect("named");
        assert_ne!(
            start, end,
            "segment {segment} named one edge for both ends of the sweep"
        );
        // And each is on the cap it claims. Two edges that merely differ could
        // still be the two ends the wrong way round; this cannot.
        assert!(
            bounds(start, start_cap),
            "the start cap edge of {segment} does not bound the start cap"
        );
        assert!(
            bounds(end, end_cap),
            "the end cap edge of {segment} does not bound the end cap"
        );
        assert!(!bounds(start, end_cap), "the start edge is on the end cap");
        assert!(!bounds(end, start_cap), "the end edge is on the start cap");
    }
    built.kernel.release(built.shape);
}

/// The face closing one end of the sweep, by its own durable name.
fn face_of_side(built: &Built, side: ferritecad_document::CapSide) -> SubShapeHandle {
    face_of(
        built,
        ferritecad_document::SemanticRole::ExtrudeCap { side },
    )
}

#[test]
fn a_reversed_and_a_symmetric_sweep_name_their_cap_edges_too() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    for (what, extent, reversed) in [
        ("blind", ExtrudeExtent::blind(HEIGHT).expect("valid"), false),
        (
            "reversed",
            ExtrudeExtent::blind(HEIGHT).expect("valid"),
            true,
        ),
        (
            "symmetric",
            ExtrudeExtent::symmetric(HEIGHT).expect("valid"),
            false,
        ),
        (
            "reversed symmetric",
            ExtrudeExtent::symmetric(HEIGHT).expect("valid"),
            true,
        ),
    ] {
        let (mut built, _) = swept(extent, reversed, false);
        let mut named = BTreeSet::new();
        for segment in &built.segments {
            for side in [
                ferritecad_document::CapSide::Start,
                ferritecad_document::CapSide::End,
            ] {
                let edge = cap_edge_of(&built, side, *segment)
                    .unwrap_or_else(|e| panic!("{what}: {side:?} of {segment} is unnamed: {e}"));
                assert_eq!(edge.kind(), SubShapeKind::Edge);
                assert!(named.insert(edge), "{what}: {edge} was named twice");
            }
        }
        assert_eq!(named.len(), 8, "{what}: eight distinct cap edges");
        built.kernel.release(built.shape);
    }
}

#[test]
fn an_arc_segment_names_the_curved_edge_of_each_cap() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    let (mut built, _) = swept(ExtrudeExtent::blind(5.0).expect("valid"), false, true);
    let arc = built.segments[0];
    let chord = built.segments[1];

    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");
    let edges = mesh.edges.as_ref().expect("the association is there");
    let segments_of = |handle: SubShapeHandle| {
        edges
            .ranges
            .iter()
            .find(|range| range.edge == handle)
            .map(|range| range.segment_count)
            .expect("the named edge is drawn")
    };

    for side in [
        ferritecad_document::CapSide::Start,
        ferritecad_document::CapSide::End,
    ] {
        let curved = cap_edge_of(&built, side, arc).expect("named");
        let straight = cap_edge_of(&built, side, chord).expect("named");
        assert_ne!(curved, straight);
        // The curved one is the one a tessellation needs many chords for. A
        // straight chord needs one per face side; an arc at this deflection
        // needs a great many, so the two cannot be swapped unnoticed.
        assert!(
            segments_of(curved) > segments_of(straight) * 4,
            "the arc's cap edge is drawn with {} segments and the chord's with {}",
            segments_of(curved),
            segments_of(straight)
        );
    }
    built.kernel.release(built.shape);
}

#[test]
fn the_edges_along_the_sweep_stay_unnamed() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    let mut built = build();
    let mut named = BTreeSet::new();
    for segment in &built.segments {
        for side in [
            ferritecad_document::CapSide::Start,
            ferritecad_document::CapSide::End,
        ] {
            named.insert(cap_edge_of(&built, side, *segment).expect("named"));
        }
    }

    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");
    let drawn: BTreeSet<SubShapeHandle> = mesh
        .edges
        .as_ref()
        .expect("the association is there")
        .ranges
        .iter()
        .map(|range| range.edge)
        .collect();

    // A plate has twelve edges: eight around the two caps and four running
    // along the sweep. The four are drawn and deliberately have no name.
    assert_eq!(drawn.len(), 12, "the plate draws twelve edges");
    assert_eq!(named.len(), 8, "eight of them are named");
    assert_eq!(
        drawn.difference(&named).count(),
        4,
        "and four are not, which is the honest answer for an edge along the sweep"
    );
    built.kernel.release(built.shape);
}

#[test]
fn two_features_with_the_same_shape_do_not_share_a_name() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    // Two extrusions of geometrically identical profiles, each with its own
    // labels. Nothing about the geometry may make one answer for the other.
    let (mut first, _) = swept(ExtrudeExtent::blind(HEIGHT).expect("valid"), false, false);
    let (mut second, _) = swept(ExtrudeExtent::blind(HEIGHT).expect("valid"), false, false);

    let side = ferritecad_document::CapSide::Start;
    let mine = cap_edge_of(&first, side, first.segments[0]).expect("named");
    let theirs = cap_edge_of(&second, side, second.segments[0]).expect("named");
    assert_ne!(mine.shape(), theirs.shape(), "two shapes, two sessions");

    // The other feature's label is not a name here, and this feature's label
    // is not a name there.
    assert!(
        cap_edge_of(&first, side, second.segments[0]).is_err(),
        "a label of another feature resolved against this one"
    );
    // And a reference naming the wrong producer resolves to nothing.
    let wrong_producer = ferritecad_document::TopologyRef {
        producer_feature: second.feature,
        ..cap_edge_reference(first.feature, side, first.segments[0])
    };
    assert!(
        resolve(&first.map, &wrong_producer).is_err(),
        "a reference to another feature's output was answered"
    );

    first.kernel.release(first.shape);
    second.kernel.release(second.shape);
}
