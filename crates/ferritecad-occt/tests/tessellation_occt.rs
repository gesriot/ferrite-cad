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

use std::{collections::BTreeMap, f64::consts::PI};

use ferritecad_document::CapSide;
use ferritecad_kernel::{
    CancelToken, ExtrudeExtent, ExtrudeRequest, GeometryKernel, Mesh, OperationContext,
    PlanarPoint, Profile, ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane,
    SubShapeHandle, TessellationParams,
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
