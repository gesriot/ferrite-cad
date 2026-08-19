// SPDX-License-Identifier: MIT
//! What a viewport does before any GPU is involved.
//!
//! Everything here runs on a machine with no graphics at all, which is most of
//! what matters: packing, composing placements, the order things are drawn in,
//! and what a pick is allowed to say. A driver can only get those wrong after
//! they are already right.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_kernel::{
    Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeHandle, SubShapeKind,
};
use ferritecad_types::{ContentHash, ErrorKind, Transform, Vec3};
use ferritecad_viewport::{
    Camera, FacePickId, Marked, PickId, Projection, RenderSnapshot, SnapshotBuilder, StandardView,
    VERTEX_FLOATS, Visibility,
};

/// One triangle, with distinguishable positions and normals.
fn triangle() -> Mesh {
    let shape = ShapeHandle::new(SessionId::new(), 1);
    Mesh {
        positions: vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0],
        normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        indices: vec![0, 1, 2],
        faces: vec![MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
            first_index: 0,
            index_count: 3,
        }],
        edges: None,
    }
}

fn moved(x: f64, y: f64, z: f64) -> Transform {
    Transform::from_translation(Vec3::new(x, y, z).expect("finite")).expect("finite")
}

fn clip_coordinates(matrix: &[f32; 16], point: [f32; 3]) -> [f32; 4] {
    [
        matrix[0] * point[0] + matrix[4] * point[1] + matrix[8] * point[2] + matrix[12],
        matrix[1] * point[0] + matrix[5] * point[1] + matrix[9] * point[2] + matrix[13],
        matrix[2] * point[0] + matrix[6] * point[1] + matrix[10] * point[2] + matrix[14],
        matrix[3] * point[0] + matrix[7] * point[1] + matrix[11] * point[2] + matrix[15],
    ]
}

fn projected(matrix: &[f32; 16], point: [f32; 3]) -> [f32; 3] {
    let clip = clip_coordinates(matrix, point);
    assert!(clip[3] > 0.0, "a point in front has clip.w {}", clip[3]);
    [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]]
}

fn inside_clip_volume(matrix: &[f32; 16], point: [f32; 3]) -> bool {
    let clip = clip_coordinates(matrix, point);
    if clip[3] <= 0.0 {
        return false;
    }
    let ndc = [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]];
    (-1.0..=1.0).contains(&ndc[0])
        && (-1.0..=1.0).contains(&ndc[1])
        && (0.0..=1.0).contains(&ndc[2])
}

#[test]
fn a_mesh_is_packed_as_interleaved_position_and_normal() {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&triangle()).expect("packs");
    builder
        .place(mesh, None, &Transform::IDENTITY, [0.5, 0.5, 0.5])
        .expect("places");
    let snapshot = builder.build();

    let packed = &snapshot.meshes()[mesh];
    assert_eq!(packed.vertex_count(), 3);
    assert_eq!(packed.triangle_count(), 1);
    assert_eq!(packed.vertices().len(), 3 * VERTEX_FLOATS);

    // Vertex one is at (2, 0, 0) with a +Z normal, and both halves land where
    // a vertex layout of six floats says they will.
    let second = &packed.vertices()[VERTEX_FLOATS..VERTEX_FLOATS * 2];
    assert_eq!(second, &[2.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    assert_eq!(packed.indices(), &[0, 1, 2]);

    // And its own extent, before anything places it.
    assert_eq!(packed.bounds(), ([0.0, 0.0, 0.0], [2.0, 3.0, 0.0]));
}

#[test]
fn a_mesh_a_gpu_could_not_draw_is_refused_before_it_is_packed() {
    let mut builder = SnapshotBuilder::new();

    let mut dangling = triangle();
    dangling.indices = vec![0, 1, 9];
    assert!(
        builder.add_mesh(&dangling).is_err(),
        "an index past the last vertex is a driver fault waiting to happen"
    );

    let mut infinite = triangle();
    infinite.positions[0] = f32::INFINITY;
    assert!(builder.add_mesh(&infinite).is_err());

    let mut broken_normal = triangle();
    broken_normal.normals[2] = f32::NAN;
    assert!(builder.add_mesh(&broken_normal).is_err());
}

#[test]
fn values_that_only_overflow_when_narrowed_for_a_gpu_are_refused() {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&triangle()).expect("packs");

    assert!(
        builder
            .place(mesh, None, &moved(f64::MAX, 0.0, 0.0), [1.0; 3])
            .is_err()
    );
    assert!(
        builder
            .place(mesh, None, &Transform::IDENTITY, [f64::MAX, 1.0, 1.0])
            .is_err()
    );

    // Every matrix entry fits in f32, but multiplying the x coordinate by the
    // largest one does not. This is the arithmetic the vertex shader performs.
    let overflowing_scale = Transform::from_rows([
        [f64::from(f32::MAX), 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("finite f64 transform");
    assert!(
        builder
            .place(mesh, None, &overflowing_scale, [1.0; 3])
            .is_err()
    );
}

#[test]
fn placements_compose_down_the_tree() {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&triangle()).expect("packs");

    // An assembly at x=10 holding a part at y=5: the part is at (10, 5, 0),
    // which is the composition and not either half of it.
    let assembly = builder
        .place(mesh, None, &moved(10.0, 0.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    let part = builder
        .place(mesh, Some(assembly), &moved(0.0, 5.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    let deeper = builder
        .place(mesh, Some(part), &moved(0.0, 0.0, 2.0), [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    // Column-major: the translation is the last column, floats 12, 13 and 14.
    let translation = |item: usize| {
        let matrix = snapshot.draws()[item].transform;
        [matrix[12], matrix[13], matrix[14]]
    };
    assert_eq!(translation(assembly), [10.0, 0.0, 0.0]);
    assert_eq!(translation(part), [10.0, 5.0, 0.0]);
    assert_eq!(translation(deeper), [10.0, 5.0, 2.0]);

    // The world bounds cover every placement, not just the last one.
    let (min, max) = snapshot.bounds().expect("something is drawn");
    assert_eq!(min, [10.0, 0.0, 0.0]);
    assert_eq!(max, [12.0, 8.0, 2.0]);
}

#[test]
fn a_parent_must_already_be_there() {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&triangle()).expect("packs");

    // A forward reference would have to be resolved in a second pass, and a
    // tree that needs one is a tree that can contain a cycle.
    assert!(
        builder
            .place(mesh, Some(0), &Transform::IDENTITY, [1.0, 1.0, 1.0])
            .is_err()
    );
    assert!(
        builder
            .place(7, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
            .is_err()
    );
}

#[test]
fn the_draw_order_is_the_order_placements_were_given() {
    let build = || {
        let mut builder = SnapshotBuilder::new();
        let plate = builder.add_mesh(&triangle()).expect("packs");
        let bolt = builder.add_mesh(&triangle()).expect("packs");

        // Interleaved on purpose: a renderer that sorted by mesh to save state
        // changes would reorder these, and two frames of an unchanged model
        // would stop being comparable.
        builder
            .place(plate, None, &Transform::IDENTITY, [1.0, 0.0, 0.0])
            .expect("places");
        builder
            .place(bolt, None, &moved(1.0, 0.0, 0.0), [0.0, 1.0, 0.0])
            .expect("places");
        builder
            .place(plate, None, &moved(2.0, 0.0, 0.0), [0.0, 0.0, 1.0])
            .expect("places");
        builder.build()
    };

    let first = build();
    let second = build();
    assert_eq!(first, second, "two builds of one model must draw the same");

    let order: Vec<usize> = first.draws().iter().map(|item| item.mesh).collect();
    assert_eq!(order, vec![0, 1, 0]);
}

#[test]
fn identical_pixels_with_different_interpretations_issue_different_picks() {
    let build = |context: &[u8]| {
        let mut builder = SnapshotBuilder::new();
        builder
            .bind_identities_to(ContentHash::of_bytes(context))
            .expect("binds once");
        let mesh = builder.add_mesh(&triangle()).expect("packs");
        builder
            .place(mesh, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
            .expect("places");
        builder.build()
    };

    let first = build(b"face zero is named");
    let repeated = build(b"face zero is named");
    let other = build(b"no face is named");
    assert_eq!(first, repeated, "the binding must be deterministic");
    assert_eq!(first.meshes(), other.meshes(), "only meaning differs");
    assert_ne!(first, other, "meaning did not bind the transient ids");

    let pick = first.pick_of(0).expect("drawn");
    let face = first.face_of(0, 0).expect("numbered");
    assert_eq!(other.definition(pick), None);
    assert_eq!(other.definition_of_face(face), None);
}

#[test]
fn refusing_a_second_identity_binding_keeps_the_first() {
    let build = |try_twice: bool| {
        let mut builder = SnapshotBuilder::new();
        builder
            .bind_identities_to(ContentHash::of_bytes(b"first"))
            .expect("binds once");
        if try_twice {
            builder
                .bind_identities_to(ContentHash::of_bytes(b"second"))
                .expect_err("a second interpretation is contradictory");
        }
        let mesh = builder.add_mesh(&triangle()).expect("packs");
        builder
            .place(mesh, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
            .expect("places");
        builder.build()
    };

    assert_eq!(
        build(false),
        build(true),
        "refusing the second binding changed the picture anyway"
    );
}

#[test]
fn signed_zero_does_not_create_a_different_snapshot_identity() {
    let build = |negative: bool| {
        let mut source = triangle();
        source.positions[0] = if negative { -0.0 } else { 0.0 };
        let mut builder = SnapshotBuilder::new();
        let mesh = builder.add_mesh(&source).expect("packs");
        builder
            .place(
                mesh,
                None,
                &Transform::IDENTITY,
                [if negative { -0.0 } else { 0.0 }, 0.5, 0.5],
            )
            .expect("places");
        builder.build()
    };

    assert_eq!(
        build(false),
        build(true),
        "equal f32 values must not produce pick generations that compare unequal"
    );
}

/// A mesh with no triangles at all, which is a definition that draws nothing.
fn nothing_at_all() -> Mesh {
    Mesh::default()
}

/// Two triangles the kernel calls two faces, as a box's corner would be.
fn two_faced(first_triangles: u32, second_triangles: u32) -> Mesh {
    divided(&[first_triangles, second_triangles])
}

/// One strip of triangles, divided into faces of the given sizes.
///
/// A face of no triangles is not a face and is left out, which is what lets
/// `two_faced(n, 0)` mean one face.
fn divided(runs: &[u32]) -> Mesh {
    let shape = ShapeHandle::new(SessionId::new(), 1);
    let mut mesh = Mesh::default();
    let total: u32 = runs.iter().sum();
    for triangle in 0..total {
        let base = triangle as f32 * 4.0;
        mesh.positions
            .extend_from_slice(&[base, 0.0, 0.0, base + 2.0, 0.0, 0.0, base, 3.0, 0.0]);
        mesh.normals
            .extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
        let first = triangle * 3;
        mesh.indices
            .extend_from_slice(&[first, first + 1, first + 2]);
    }
    let mut first_index = 0;
    for (ordinal, triangles) in runs.iter().enumerate().filter(|(_, count)| **count > 0) {
        mesh.faces.push(MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, ordinal as u64),
            first_index,
            index_count: triangles * 3,
        });
        first_index += triangles * 3;
    }
    mesh
}

#[test]
fn two_faces_of_one_definition_are_two_identities_of_one_definition() {
    let mut builder = SnapshotBuilder::new();
    let part = builder.add_mesh(&two_faced(1, 1)).expect("packs");
    builder
        .place(part, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    // The same definition again, somewhere else: a face of it is one face
    // however many times the definition appears.
    builder
        .place(part, None, &moved(50.0, 0.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    // The packing keeps the division the kernel made. Before it did, the two
    // triangles were indistinguishable once packed, and the finest thing that
    // could be pointed at was the whole definition.
    let faces = snapshot.meshes()[part].faces_of_vertices();
    assert_eq!(
        faces.len(),
        snapshot.meshes()[part].vertex_count(),
        "one identity per vertex"
    );
    assert_ne!(faces[0], faces[3], "two faces were packed as one");
    assert_eq!(snapshot.meshes()[part].face_count(), 2);
    assert_eq!(snapshot.face_count(), 2);

    // Both belong to the definition they came from, and to no other.
    for face in faces {
        let identity = FacePickId::from_raw(*face, &snapshot);
        assert_eq!(snapshot.definition_of_face(identity), Some(part));
    }

    // And the two draws share them: a face is a face of a definition, not of
    // a placement, so pointing at one in either place is the same face.
    assert_eq!(snapshot.draws().len(), 2);
    assert_eq!(
        snapshot.definition(snapshot.draws()[0].pick),
        snapshot.definition(snapshot.draws()[1].pick)
    );
}

#[test]
fn a_face_of_many_triangles_is_still_one_face() {
    let mut builder = SnapshotBuilder::new();
    let part = builder.add_mesh(&two_faced(3, 2)).expect("packs");
    builder
        .place(part, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    // Three triangles of three vertices each, then two of three.
    let faces = snapshot.meshes()[part].faces_of_vertices();
    assert_eq!(faces.len(), 15);
    assert!(faces[..9].iter().all(|face| *face == faces[0]), "{faces:?}");
    assert!(faces[9..].iter().all(|face| *face == faces[9]), "{faces:?}");
    assert_ne!(faces[8], faces[9]);
    assert_eq!(snapshot.meshes()[part].face_count(), 2);
}

#[test]
fn the_same_triangles_divided_differently_are_a_different_picture() {
    let picture = |first, second| {
        let mut builder = SnapshotBuilder::new();
        let part = builder.add_mesh(&two_faced(first, second)).expect("packs");
        builder
            .place(part, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
            .expect("places");
        builder.build()
    };

    // Identical vertices and indices, divided into faces two ways. A picture
    // that called these the same would let a face identity taken from one of
    // them resolve in the other.
    let (a, b) = (picture(1, 3), picture(2, 2));
    assert_eq!(
        a.meshes()[0].vertices(),
        b.meshes()[0].vertices(),
        "the two pictures were meant to share their geometry"
    );
    assert_eq!(a.meshes()[0].indices(), b.meshes()[0].indices());

    let from_a = FacePickId::from_raw(a.meshes()[0].faces_of_vertices()[3], &a);
    assert_eq!(a.definition_of_face(from_a), Some(0));
    assert_eq!(
        b.definition_of_face(from_a),
        None,
        "a face of one picture resolved in another"
    );

    // The same again where counting is not enough to tell the two apart: the
    // same triangles, the same number of faces, and the last face the same
    // size in both. Only where the boundaries fall differs.
    let divided_as = |runs: &[u32]| {
        let mut builder = SnapshotBuilder::new();
        let part = builder.add_mesh(&divided(runs)).expect("packs");
        builder
            .place(part, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
            .expect("places");
        builder.build()
    };
    let (c, d) = (divided_as(&[1, 2, 1]), divided_as(&[2, 1, 1]));
    assert_eq!(c.face_count(), d.face_count());
    assert_eq!(c.meshes()[0].indices(), d.meshes()[0].indices());
    let from_c = FacePickId::from_raw(c.meshes()[0].faces_of_vertices()[0], &c);
    assert_eq!(
        d.definition_of_face(from_c),
        None,
        "two different partitions of the same triangles were one picture"
    );
}

#[test]
fn faces_are_numbered_across_the_whole_picture_and_not_within_a_definition() {
    let mut builder = SnapshotBuilder::new();
    let first = builder.add_mesh(&two_faced(1, 1)).expect("packs");
    let second = builder.add_mesh(&two_faced(2, 1)).expect("packs");
    for (part, x) in [(first, 0.0), (second, 50.0)] {
        builder
            .place(part, None, &moved(x, 0.0, 0.0), [1.0, 1.0, 1.0])
            .expect("places");
    }
    let snapshot = builder.build();

    // Four faces and four identities. Numbering them within each definition
    // would give the second definition the first one's numbers, and pointing
    // at a face of one would mark a face of the other.
    assert_eq!(snapshot.face_count(), 4);
    let mut seen = Vec::new();
    for (definition, mesh) in snapshot.meshes().iter().enumerate() {
        for raw in mesh.faces_of_vertices() {
            let face = FacePickId::from_raw(*raw, &snapshot);
            assert_eq!(
                snapshot.definition_of_face(face),
                Some(definition),
                "a face belongs to the definition it was packed with"
            );
            if !seen.contains(&face) {
                seen.push(face);
            }
        }
    }
    assert_eq!(seen.len(), 4);
}

#[test]
fn a_mesh_with_nothing_in_it_has_no_faces() {
    let mut builder = SnapshotBuilder::new();
    let empty = builder.add_mesh(&Mesh::default()).expect("packs");
    let snapshot = builder.build();

    assert_eq!(snapshot.meshes()[empty].face_count(), 0);
    assert!(snapshot.meshes()[empty].faces_of_vertices().is_empty());
    assert_eq!(snapshot.face_count(), 0);

    // Nothing is not a face, and no number names one here.
    assert_eq!(snapshot.definition_of_face(FacePickId::NOTHING), None);
    assert_eq!(
        snapshot.definition_of_face(FacePickId::from_raw(1, &snapshot)),
        None
    );
}

#[test]
fn a_camera_driven_off_the_model_can_be_brought_back_to_all_of_it() {
    let mut builder = SnapshotBuilder::new();
    let near = builder.add_mesh(&triangle()).expect("packs");
    let far = builder.add_mesh(&triangle()).expect("packs");

    // Several definitions, and placements a long way apart: what must come
    // back is the whole picture rather than whichever part is nearest.
    builder
        .place(near, None, &moved(0.0, 0.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    builder
        .place(near, None, &moved(-300.0, 0.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    builder
        .place(far, None, &moved(400.0, 250.0, -120.0), [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    let mut camera = Camera::new();
    camera.resize(800, 600);
    camera
        .frame(snapshot.bounds().expect("the picture has extent"))
        .expect("frames");
    let before = camera;

    // Pan and zoom until nothing is on screen. This is the state the action
    // exists for: no definition is chosen, so there is nothing to frame, and
    // the view directions do not bring a model back that is beside the camera
    // rather than behind it.
    // Zoom in until the model fills more than the window, then push it out of
    // sight: the ordinary way a person loses a model is close up.
    camera.zoom(3.0);
    for _ in 0..20 {
        camera.pan(-500.0, -300.0);
    }
    let (min, max) = snapshot.bounds().expect("the picture has extent");
    let corners: Vec<[f32; 3]> = (0..8)
        .map(|corner| {
            [
                if corner & 1 == 0 { min[0] } else { max[0] },
                if corner & 2 == 0 { min[1] } else { max[1] },
                if corner & 4 == 0 { min[2] } else { max[2] },
            ]
        })
        .collect();
    // A point behind the camera or outside either clipping plane is lost as
    // surely as one beside the window. Ask the complete WGPU clip volume,
    // including depth, rather than calling an x/y projection "inside" it.
    let on_screen =
        |camera: &Camera, point: [f32; 3]| inside_clip_volume(&camera.view_projection(), point);

    assert!(
        corners.iter().any(|corner| !on_screen(&camera, *corner)),
        "the model was still on screen, so nothing was recovered"
    );

    // Framing the whole picture brings every corner of it back inside the
    // clip volume.
    camera
        .frame(snapshot.bounds().expect("the picture has extent"))
        .expect("frames");
    for corner in &corners {
        assert!(
            on_screen(&camera, *corner),
            "a corner of the model at {corner:?} is still off screen"
        );
    }

    // And it looks from where it was looking from: recovering a model is not
    // an excuse to choose a viewpoint the user did not ask for.
    let direction = |camera: &Camera| {
        let (eye, target) = (camera.eye(), camera.target());
        let away = [eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]];
        let length = (away[0] * away[0] + away[1] * away[1] + away[2] * away[2]).sqrt();
        [away[0] / length, away[1] / length, away[2] / length]
    };
    let (was, now) = (direction(&before), direction(&camera));
    for axis in 0..3 {
        assert!(
            (was[axis] - now[axis]).abs() < 1e-3,
            "the viewing direction turned: {was:?} to {now:?}"
        );
    }
}

#[test]
fn a_picture_with_nothing_in_it_is_nowhere_to_point_a_camera() {
    let empty = SnapshotBuilder::new().build();
    assert_eq!(empty.bounds(), None);

    // A definition that is placed but draws nothing is still nothing to show.
    let mut builder = SnapshotBuilder::new();
    let nothing = builder.add_mesh(&Mesh::default()).expect("packs");
    builder
        .place(nothing, None, &moved(5.0, 5.0, 5.0), [1.0, 1.0, 1.0])
        .expect("places");
    assert_eq!(builder.build().bounds(), None);
}

#[test]
fn what_a_selected_definition_covers_is_all_of_it_and_none_of_anything_else() {
    let mut builder = SnapshotBuilder::new();
    let bolt = builder.add_mesh(&triangle()).expect("packs");
    let plate = builder.add_mesh(&triangle()).expect("packs");

    // Two placements of the bolt, a long way apart, and a plate beside them
    // that has nothing to do with either.
    builder
        .place(bolt, None, &moved(0.0, 0.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    builder
        .place(bolt, None, &moved(100.0, 0.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    builder
        .place(plate, None, &moved(0.0, 500.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    let pick = snapshot
        .pick_of(bolt)
        .expect("the picture has that definition");
    let (min, max) = snapshot.bounds_of(pick).expect("the bolt is somewhere");

    // Both placements fit: the triangle spans x 0..2, so two of them 100 apart
    // span 0..102. One placement of two would answer 0..2.
    assert!((min[0] - 0.0).abs() < 1e-4, "{min:?}");
    assert!((max[0] - 102.0).abs() < 1e-4, "{max:?}");

    // And the plate is not in it, though the whole picture reaches y 503.
    assert!(
        (max[1] - 3.0).abs() < 1e-4,
        "the neighbour was included: {max:?}"
    );
    let (_, whole) = snapshot.bounds().expect("the picture has extent");
    assert!(whole[1] > 500.0, "the picture really does reach further");
}

#[test]
fn a_turned_placement_is_measured_by_all_eight_of_its_corners() {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&triangle()).expect("packs");

    // A quarter turn about z, then moved. Rotating the two extreme corners
    // alone gives a box that is right only by accident; rotating all eight is
    // what makes this the extent of the thing rather than of its numbers.
    let turn =
        Transform::from_rotation(Vec3::new(0.0, 0.0, 1.0).expect("finite"), 0.5).expect("finite");
    builder
        .place(mesh, None, &turn, [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    let pick = snapshot
        .pick_of(mesh)
        .expect("the picture has that definition");
    let (min, max) = snapshot.bounds_of(pick).expect("it is somewhere");

    // Every corner of the turned box lies inside what came back.
    let (low, high) = snapshot.meshes()[mesh].bounds();
    for corner in 0..8 {
        let point = [
            if corner & 1 == 0 { low[0] } else { high[0] },
            if corner & 2 == 0 { low[1] } else { high[1] },
            if corner & 4 == 0 { low[2] } else { high[2] },
        ];
        let placed = snapshot
            .draws()
            .first()
            .map(|draw| {
                let m = &draw.transform;
                [
                    m[0] * point[0] + m[4] * point[1] + m[8] * point[2] + m[12],
                    m[1] * point[0] + m[5] * point[1] + m[9] * point[2] + m[13],
                    m[2] * point[0] + m[6] * point[1] + m[10] * point[2] + m[14],
                ]
            })
            .expect("the definition is drawn");
        for axis in 0..3 {
            assert!(
                placed[axis] >= min[axis] - 1e-4 && placed[axis] <= max[axis] + 1e-4,
                "corner {corner} axis {axis} at {} is outside {min:?}..{max:?}",
                placed[axis]
            );
        }
    }
}

#[test]
fn a_definition_that_is_nowhere_is_not_somewhere() {
    let mut builder = SnapshotBuilder::new();
    let drawn = builder.add_mesh(&triangle()).expect("packs");
    let empty = builder.add_mesh(&nothing_at_all()).expect("packs");
    builder
        .place(drawn, None, &moved(0.0, 0.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    builder
        .place(empty, None, &moved(50.0, 0.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    // A definition with no triangles is placed and draws nothing. Calling its
    // empty bounds a point would send a camera to wherever it was placed and
    // show nothing when it got there.
    let empty_pick = snapshot.pick_of(empty).expect("the picture has that row");
    assert_eq!(snapshot.bounds_of(empty_pick), None);

    // Nothing chosen is not a place either, and neither is a pick from a
    // picture that has been replaced.
    assert_eq!(snapshot.bounds_of(PickId::NOTHING), None);

    let mut other = SnapshotBuilder::new();
    let only = other.add_mesh(&triangle()).expect("packs");
    other
        .place(only, None, &moved(7.0, 7.0, 7.0), [1.0, 1.0, 1.0])
        .expect("places");
    let other = other.build();
    assert_eq!(
        snapshot.bounds_of(other.pick_of(only).expect("a definition")),
        None,
        "a choice made in another picture was measured in this one"
    );

    // What is drawn is still somewhere, so the refusals above are refusals
    // rather than a query that answers nothing at all.
    assert!(
        snapshot
            .bounds_of(snapshot.pick_of(drawn).expect("a definition"))
            .is_some()
    );
}

#[test]
fn a_definition_can_be_asked_for_by_position_and_only_in_its_own_picture() {
    let mut builder = SnapshotBuilder::new();
    let plate = builder.add_mesh(&triangle()).expect("packs");
    let bolt = builder.add_mesh(&triangle()).expect("packs");
    builder
        .place(plate, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    for x in 0..4 {
        builder
            .place(bolt, None, &moved(f64::from(x), 0.0, 0.0), [1.0, 1.0, 1.0])
            .expect("places");
    }
    let snapshot = builder.build();

    // What a list needs: a row's position becomes the same kind of value a
    // click yields, and reads back as the definition it named.
    for definition in [plate, bolt] {
        let pick = snapshot
            .pick_of(definition)
            .expect("this snapshot has that definition");
        assert_eq!(snapshot.definition(pick), Some(definition));
    }

    // Choosing the bolt by position is choosing what every bolt draw is, so a
    // list and a click cannot disagree about what was chosen.
    assert_eq!(
        snapshot.pick_of(bolt),
        Some(snapshot.draws()[1].pick),
        "asking for a definition by position named something else"
    );

    // A position this picture does not have is not a definition of it.
    assert_eq!(snapshot.pick_of(2), None);
    assert_eq!(snapshot.pick_of(usize::MAX), None);
    assert_eq!(SnapshotBuilder::new().build().pick_of(0), None);

    // And what comes back belongs to the picture that issued it. Another
    // picture numbers its definitions the same way and means other things by
    // them, which is exactly what must not resolve.
    let mut other = SnapshotBuilder::new();
    let only = other.add_mesh(&triangle()).expect("packs");
    other
        .place(only, None, &moved(9.0, 9.0, 9.0), [1.0, 1.0, 1.0])
        .expect("places");
    let other = other.build();
    assert_eq!(
        other.definition(snapshot.pick_of(plate).expect("a definition")),
        None,
        "a definition asked for in one picture answered in another"
    );
}

#[test]
fn a_pick_names_a_definition_and_cannot_name_a_placement() {
    let mut builder = SnapshotBuilder::new();
    let plate = builder.add_mesh(&triangle()).expect("packs");
    let bolt = builder.add_mesh(&triangle()).expect("packs");

    builder
        .place(plate, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    // Four bolts, four draws, four distinct places on screen.
    for x in 0..4 {
        builder
            .place(bolt, None, &moved(f64::from(x), 0.0, 0.0), [1.0, 1.0, 1.0])
            .expect("places");
    }
    let snapshot = builder.build();

    assert_eq!(snapshot.draws().len(), 5, "every placement is drawn");

    // Every bolt picks the same, and that is the point: what a click can say
    // is "this part", and there is nothing in the answer that could say which
    // one of them was under the cursor.
    let picks: Vec<PickId> = snapshot.draws()[1..].iter().map(|item| item.pick).collect();
    assert!(picks.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(snapshot.definition(picks[0]), Some(bolt));
    assert_ne!(picks[0], snapshot.draws()[0].pick);
    assert_eq!(snapshot.definition(snapshot.draws()[0].pick), Some(plate));

    // Nothing is not a definition, and a definition is never nothing.
    assert_eq!(snapshot.definition(PickId::NOTHING), None);
    assert!(picks.iter().all(|pick| *pick != PickId::NOTHING));
}

#[test]
fn a_pick_value_from_outside_lands_on_the_background() {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&triangle()).expect("packs");
    builder
        .place(mesh, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    // A pick buffer is written by a GPU and read back over a bus. A value that
    // names no definition has to land on the background rather than on
    // whichever one it happens to number.
    assert_eq!(PickId::from_raw(0, &snapshot), PickId::NOTHING);
    assert_eq!(PickId::from_raw(99, &snapshot), PickId::NOTHING);
    assert_eq!(PickId::from_raw(u32::MAX, &snapshot), PickId::NOTHING);

    let real = snapshot.draws()[0].pick;
    assert_eq!(PickId::from_raw(real.to_raw(), &snapshot), real);
}

#[test]
fn a_pick_already_decoded_for_one_snapshot_cannot_retarget_in_another() {
    let build = |colour| {
        let mut builder = SnapshotBuilder::new();
        let mesh = builder.add_mesh(&triangle()).expect("packs");
        builder
            .place(mesh, None, &Transform::IDENTITY, colour)
            .expect("places");
        builder.build()
    };
    let first = build([1.0, 0.0, 0.0]);
    let second = build([0.0, 0.0, 1.0]);
    let old_pick = first.draws()[0].pick;

    assert_eq!(first.definition(old_pick), Some(0));
    assert_eq!(
        second.definition(old_pick),
        None,
        "the same in-range integer must not silently name new geometry"
    );

    // Decoding the integer against the second snapshot is a distinct act. A
    // renderer may do it only while retaining the snapshot that produced its
    // readback; that lifetime is part of §19B rather than this headless layer.
    let second_pick = PickId::from_raw(old_pick.to_raw(), &second);
    assert_eq!(second.definition(second_pick), Some(0));
    assert_ne!(old_pick, second_pick);
}

#[test]
fn a_viewport_of_no_size_still_produces_numbers() {
    let mut camera = Camera::new();
    camera.resize(0, 0);

    // Minimised windows and the moment before a first layout both look like
    // this. A projection that divided by a zero aspect would put a NaN into a
    // uniform buffer, and the picture would go wrong a frame later somewhere
    // else entirely.
    assert!(!camera.is_drawable());
    assert_eq!(camera.aspect(), 1.0);
    assert!(
        camera
            .view_projection()
            .iter()
            .all(|value| value.is_finite()),
        "a zero-size viewport produced {:?}",
        camera.view_projection()
    );

    camera.resize(800, 0);
    assert!(!camera.is_drawable());
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));

    camera.resize(0, 600);
    assert!(!camera.is_drawable());
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));

    camera.resize(800, 600);
    assert!(camera.is_drawable());
    assert!((camera.aspect() - 4.0 / 3.0).abs() < 1e-6);
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));
}

#[test]
fn framing_puts_the_whole_model_in_front_of_the_camera() {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&triangle()).expect("packs");
    builder
        .place(mesh, None, &moved(100.0, 0.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    let mut camera = Camera::new();
    camera.resize(800, 600);
    camera
        .frame(snapshot.bounds().expect("something is drawn"))
        .expect("frames");

    // Looking at the model rather than at the origin it is nowhere near.
    assert!(
        (camera.target()[0] - 101.0).abs() < 1e-3,
        "{:?}",
        camera.target()
    );
    assert_ne!(camera.eye(), camera.target());
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));

    // A single point has no extent, and framing it must still give a view.
    let mut camera = Camera::new();
    camera.resize(800, 600);
    camera
        .frame(([1.0, 1.0, 1.0], [1.0, 1.0, 1.0]))
        .expect("frames a point");
    assert_ne!(camera.eye(), camera.target());
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));

    // And a box that makes no sense is refused rather than framed from nowhere.
    assert!(camera.frame(([1.0, 0.0, 0.0], [0.0, 0.0, 0.0])).is_err());
    assert!(
        camera
            .frame(([f32::NAN, 0.0, 0.0], [1.0, 1.0, 1.0]))
            .is_err()
    );
    assert!(
        camera
            .frame(([-1.0e38, 0.0, 0.0], [1.0e38, 0.0, 0.0]))
            .is_err(),
        "a finite box whose required far plane overflows must be refused"
    );
}

#[test]
fn framing_keeps_every_corner_inside_a_portrait_clip_volume() {
    let bounds = ([-20.0, -3.0, -10.0], [20.0, 3.0, 10.0]);
    let mut camera = Camera::new();
    camera.resize(240, 1200);
    camera.frame(bounds).expect("frames");
    let matrix = camera.view_projection();

    for corner in 0..8 {
        let point = [
            if corner & 1 == 0 {
                bounds.0[0]
            } else {
                bounds.1[0]
            },
            if corner & 2 == 0 {
                bounds.0[1]
            } else {
                bounds.1[1]
            },
            if corner & 4 == 0 {
                bounds.0[2]
            } else {
                bounds.1[2]
            },
        ];
        let ndc = projected(&matrix, point);
        assert!(
            (-1.0..=1.0).contains(&ndc[0])
                && (-1.0..=1.0).contains(&ndc[1])
                && (0.0..=1.0).contains(&ndc[2]),
            "corner {point:?} projected outside wgpu's clip volume: {ndc:?}"
        );
    }
}

#[test]
fn an_empty_snapshot_draws_nothing_and_says_so() {
    let snapshot: RenderSnapshot = SnapshotBuilder::new().build();
    assert!(snapshot.is_empty());
    assert!(snapshot.draws().is_empty());
    assert_eq!(
        snapshot.bounds(),
        None,
        "an empty model has no extent, and inventing one would frame nothing"
    );
}

#[test]
fn placing_an_empty_mesh_does_not_invent_or_enlarge_an_extent() {
    let mut empty_only = SnapshotBuilder::new();
    let empty = empty_only.add_mesh(&Mesh::default()).expect("packs");
    empty_only
        .place(empty, None, &moved(1000.0, 0.0, 0.0), [1.0; 3])
        .expect("places");
    let empty_only = empty_only.build();
    assert!(empty_only.is_empty());
    assert_eq!(empty_only.bounds(), None);
    assert_eq!(
        empty_only.draws().len(),
        1,
        "the scene tree is still retained"
    );

    let mut mixed = SnapshotBuilder::new();
    let empty = mixed.add_mesh(&Mesh::default()).expect("packs");
    let solid = mixed.add_mesh(&triangle()).expect("packs");
    mixed
        .place(empty, None, &moved(1000.0, 0.0, 0.0), [1.0; 3])
        .expect("places");
    mixed
        .place(solid, None, &Transform::IDENTITY, [1.0; 3])
        .expect("places");
    assert_eq!(
        mixed.build().bounds(),
        Some(([0.0, 0.0, 0.0], [2.0, 3.0, 0.0]))
    );
}

/// A camera looking at a unit-ish model from the front, on a real viewport.
fn framed() -> Camera {
    let mut camera = Camera::new();
    camera.resize(800, 600);
    camera
        .frame(([-5.0, -5.0, -5.0], [5.0, 5.0, 5.0]))
        .expect("frames");
    camera
}

fn length(vector: [f32; 3]) -> f32 {
    vector[0].hypot(vector[1]).hypot(vector[2])
}

fn offset(camera: &Camera) -> [f32; 3] {
    let (eye, target) = (camera.eye(), camera.target());
    [eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]]
}

#[test]
fn orbiting_turns_the_view_without_moving_or_approaching_the_model() {
    let mut camera = framed();
    let before = camera.distance();
    let target = camera.target();

    camera.orbit(0.7, 0.3);

    assert!(
        (camera.distance() - before).abs() < 1e-3,
        "orbiting changed the distance from {before} to {}",
        camera.distance()
    );
    assert_eq!(camera.target(), target, "orbiting moved what is looked at");
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));

    // Both halves of the drag arrived, each measured as the angle it asked
    // for. Checking only that something moved would let a camera that ignored
    // the yaw pass on the strength of the pitch alone.
    let bearing = |camera: &Camera| {
        let offset = offset(camera);
        offset[1].atan2(offset[0])
    };
    assert!(
        (bearing(&camera) - bearing(&framed()) - 0.7).abs() < 1e-3,
        "a yaw of 0.7 turned the view by {}",
        bearing(&camera) - bearing(&framed())
    );
    assert!(
        (elevation(&camera) - elevation(&framed()) - 0.3).abs() < 1e-3,
        "a pitch of 0.3 raised the view by {}",
        elevation(&camera) - elevation(&framed())
    );

    // And it is reversible, which is what makes a drag feel like a drag.
    camera.orbit(-0.7, -0.3);
    let back = offset(&camera);
    let original = offset(&framed());
    for axis in 0..3 {
        assert!(
            (back[axis] - original[axis]).abs() < 1e-3,
            "orbiting there and back landed at {back:?} instead of {original:?}"
        );
    }
}

/// How far above the horizontal the eye sits, in radians.
fn elevation(camera: &Camera) -> f32 {
    let offset = offset(camera);
    offset[2].atan2(offset[0].hypot(offset[1]))
}

#[test]
fn orbiting_stops_short_of_the_pole_it_would_flip_over() {
    const RIGHT_ANGLE: f32 = std::f32::consts::FRAC_PI_2;

    // Far past straight up, repeatedly, which is what holding a drag does.
    // The eye must come to rest just below vertical rather than tumbling over
    // it: at exactly vertical the up axis and the view direction are parallel,
    // there is no side vector to be had, and the view flips about an axis the
    // user did not touch.
    //
    // Measured as an angle, not as a sign. Twenty unclamped radians of pitch
    // land the eye somewhere with a positive height and a healthy horizontal
    // reach as well, so anything looser than this passes while proving nothing.
    let mut camera = framed();
    for _ in 0..20 {
        camera.orbit(0.0, 1.0);
    }
    let up = elevation(&camera);
    assert!(up < RIGHT_ANGLE, "the eye passed straight up: {up}");
    assert!(
        RIGHT_ANGLE - up < 1e-2,
        "the eye came to rest at {up}, nowhere near the top it was driven towards"
    );
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));

    // And the same going down.
    let mut camera = framed();
    for _ in 0..20 {
        camera.orbit(0.0, -1.0);
    }
    let down = elevation(&camera);
    assert!(down > -RIGHT_ANGLE, "the eye passed straight down: {down}");
    assert!(RIGHT_ANGLE + down < 1e-2, "the eye came to rest at {down}");
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));

    // One step at a time gets there too, without overshooting on the way.
    let mut camera = framed();
    let mut previous = elevation(&camera);
    for _ in 0..200 {
        camera.orbit(0.0, 0.05);
        let now = elevation(&camera);
        assert!(
            now >= previous - 1e-4 && now < RIGHT_ANGLE,
            "orbiting up went from {previous} to {now}"
        );
        previous = now;
    }
}

#[test]
fn panning_moves_the_view_by_the_pixels_it_was_given() {
    let mut camera = framed();
    let before_eye = camera.eye();
    let before_target = camera.target();

    // A point sitting at the target, which is the plane a drag is measured at.
    let mark = camera.target();
    let before = on_screen(&camera, mark);

    camera.pan(100.0, 0.0);

    // Measured on screen rather than against `world_per_pixel`, which is the
    // very thing being checked: a test that computed its expectation with the
    // function under test would agree with any answer it gave. Normalised
    // device coordinates run -1 to 1 across the viewport, so a hundred pixels
    // of an 800-wide view is a quarter of that span.
    let after = on_screen(&camera, mark);
    let expected = -2.0 * 100.0 / camera.width() as f32;
    assert!(
        (after[0] - before[0] - expected).abs() < 1e-3,
        "a hundred pixels of pan moved the mark by {} instead of {expected}",
        after[0] - before[0]
    );
    assert!(
        (after[1] - before[1]).abs() < 1e-4,
        "panning sideways moved the view vertically: {after:?}"
    );

    // Eye and target move together, so the view slides rather than turning.
    let moved_eye = [
        camera.eye()[0] - before_eye[0],
        camera.eye()[1] - before_eye[1],
        camera.eye()[2] - before_eye[2],
    ];
    let moved_target = [
        camera.target()[0] - before_target[0],
        camera.target()[1] - before_target[1],
        camera.target()[2] - before_target[2],
    ];
    for axis in 0..3 {
        assert!((moved_eye[axis] - moved_target[axis]).abs() < 1e-4);
    }

    // A front view's right is +X, and panning right moves the camera that way,
    // so what is on screen appears to move left.
    assert!(moved_eye[0] > 0.0, "panning right went {moved_eye:?}");

    // Vertically, by its own count of pixels against the view's height.
    let mut camera = framed();
    let before = on_screen(&camera, mark);
    camera.pan(0.0, 60.0);
    let after = on_screen(&camera, mark);
    let expected = -2.0 * 60.0 / camera.height() as f32;
    assert!(
        (after[1] - before[1] - expected).abs() < 1e-3,
        "sixty pixels of pan moved the mark by {} instead of {expected}",
        after[1] - before[1]
    );

    camera.pan(0.0, -60.0);
    for (axis, was) in before_eye.iter().enumerate() {
        assert!((camera.eye()[axis] - was).abs() < 1e-2);
    }
}

#[test]
fn panning_a_viewport_of_no_size_moves_nothing() {
    for size in [(0, 0), (800, 0), (0, 600)] {
        let mut camera = framed();
        camera.resize(size.0, size.1);
        let before = camera;

        // There is no drawable pixel when either dimension is zero, so a drag
        // has no length. Checking only 0x0 would let a collapsed vertical pane
        // move despite having no horizontal coordinate system.
        assert_eq!(camera.world_per_pixel(), 0.0, "{size:?}");
        camera.pan(50.0, -50.0);
        assert_eq!(camera, before, "{size:?}");
    }
}

#[test]
fn zooming_approaches_the_target_without_ever_arriving() {
    let mut camera = framed();
    let before = camera.distance();

    camera.zoom(0.5);
    assert!(camera.distance() < before, "zooming in did not come closer");
    camera.zoom(-0.5);
    assert!(
        (camera.distance() - before).abs() < 1e-2,
        "a zoom and its opposite did not cancel: {} against {before}",
        camera.distance()
    );

    // The direction is untouched: zooming is a dolly, not a turn.
    let direction_before = offset(&camera);
    camera.zoom(2.0);
    let direction_after = offset(&camera);
    let scale = length(direction_after) / length(direction_before);
    for axis in 0..3 {
        assert!((direction_after[axis] - direction_before[axis] * scale).abs() < 1e-3);
    }

    // Held down forever, it stops short of the target. An eye that arrived
    // would be inside the model with no direction left to look in.
    for _ in 0..200 {
        camera.zoom(1.0);
    }
    assert!(
        camera.distance() > 0.0,
        "zooming in reached the target itself"
    );
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));
    assert_ne!(camera.eye(), camera.target());

    // And out forever, without leaving the number format.
    for _ in 0..400 {
        camera.zoom(-1.0);
    }
    assert!(camera.distance().is_finite());
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));
}

#[test]
fn zooming_keeps_the_model_between_the_clipping_planes() {
    let mut camera = framed();

    // A near plane left where framing put it would clip away the very thing
    // the user just moved closer to.
    for _ in 0..12 {
        camera.zoom(0.5);
        let matrix = camera.view_projection();
        assert!(matrix.iter().all(|v| v.is_finite()));

        // The target sits in front of the near plane: its clip-space w is
        // positive and its depth is inside the unit range.
        let target = camera.target();
        let w = matrix[3] * target[0] + matrix[7] * target[1] + matrix[11] * target[2] + matrix[15];
        let z = matrix[2] * target[0] + matrix[6] * target[1] + matrix[10] * target[2] + matrix[14];
        assert!(w > 0.0, "the target fell behind the camera");
        let depth = z / w;
        assert!(
            (0.0..=1.0).contains(&depth),
            "the target was clipped at depth {depth} after zooming in"
        );
    }
}

#[test]
fn every_standard_view_looks_from_where_it_says() {
    let expected = [
        (StandardView::Front, [0.0, -1.0, 0.0]),
        (StandardView::Back, [0.0, 1.0, 0.0]),
        (StandardView::Left, [-1.0, 0.0, 0.0]),
        (StandardView::Right, [1.0, 0.0, 0.0]),
        (StandardView::Top, [0.0, 0.0, 1.0]),
        (StandardView::Bottom, [0.0, 0.0, -1.0]),
    ];

    for (view, direction) in expected {
        let mut camera = framed();
        let distance = camera.distance();
        let target = camera.target();

        camera.look_from(view);

        // Turning the model over, not stepping back from it.
        assert!((camera.distance() - distance).abs() < 1e-3, "{view:?}");
        assert_eq!(camera.target(), target, "{view:?}");

        let actual = offset(&camera);
        for axis in 0..3 {
            assert!(
                (actual[axis] - direction[axis] * distance).abs() < 1e-3,
                "{view:?} looked from {actual:?}"
            );
        }

        // Including the two where the world's up axis is no use as one.
        assert!(
            camera.view_projection().iter().all(|v| v.is_finite()),
            "{view:?} produced {:?}",
            camera.view_projection()
        );
    }
}

/// Where a world point lands on screen, in normalised device coordinates.
fn on_screen(camera: &Camera, point: [f32; 3]) -> [f32; 2] {
    let matrix = camera.view_projection();
    let mut clip = [0.0f32; 4];
    for (row, value) in clip.iter_mut().enumerate() {
        *value = matrix[row] * point[0]
            + matrix[4 + row] * point[1]
            + matrix[8 + row] * point[2]
            + matrix[12 + row];
    }
    assert!(clip[3] > 0.0, "{point:?} is not in front of the camera");
    [clip[0] / clip[3], clip[1] / clip[3]]
}

#[test]
fn a_plan_view_puts_north_up_rather_than_wherever_it_lands() {
    // Looking straight down, the world's up axis is the view direction, so it
    // is no use as an up vector. A camera that kept using it would have no
    // side vector at all and would fall back to an arbitrary one – which
    // renders, and is finite, and puts the model at whatever angle the
    // fallback happened to choose. So this asks about the picture instead.
    let mut camera = framed();
    camera.look_from(StandardView::Top);

    let centre = camera.target();
    let north = [centre[0], centre[1] + 1.0, centre[2]];
    let east = [centre[0] + 1.0, centre[1], centre[2]];

    let up = on_screen(&camera, north);
    let right = on_screen(&camera, east);
    assert!(up[1] > 0.05, "north is not up in a plan view: {up:?}");
    assert!(up[0].abs() < 1e-3, "north is not straight up: {up:?}");
    assert!(right[0] > 0.05, "east is not to the right: {right:?}");

    // The bottom view is the mirror of it, not a rotation: north goes down, so
    // the two cannot be confused for one another.
    camera.look_from(StandardView::Bottom);
    let up = on_screen(&camera, north);
    assert!(up[1] < -0.05, "north is not down in a bottom view: {up:?}");

    // An isometric view sees three faces, so none of its axes is edge-on.
    camera.look_from(StandardView::Isometric);
    let iso = offset(&camera);
    assert!(iso[0] > 0.0 && iso[1] < 0.0 && iso[2] > 0.0, "{iso:?}");
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));
}

#[test]
fn orbiting_from_a_plan_view_levels_it_rather_than_rolling_it() {
    let mut camera = framed();
    camera.look_from(StandardView::Top);

    // A plan view is tilted so that north is up. Orbiting is defined about the
    // world's up axis, so entering it restores that axis; a camera that kept
    // the tilt would roll a little further with every drag and never come
    // back level.
    camera.orbit(0.0, -0.6);
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));

    // Level means the world's up axis points up the screen and nowhere across
    // it – which is a statement about the picture, not about a field.
    let centre = camera.target();
    let above = [centre[0], centre[1], centre[2] + 1.0];
    let on = on_screen(&camera, above);
    assert!(on[1] > 0.05, "up is not up after orbiting: {on:?}");
    assert!(
        on[0].abs() < 1e-3,
        "the view is rolled: up lands at {on:?} instead of straight above"
    );

    let after = offset(&camera);
    assert!(
        length([after[0], after[1], 0.0]) > 1e-4,
        "the view is still looking straight down: {after:?}"
    );
}

#[test]
fn framing_shows_the_whole_model_without_turning_it_back_to_the_front() {
    let mut camera = framed();
    camera.look_from(StandardView::Right);
    let direction_before = offset(&camera);
    let scale_before = length(direction_before);

    // Something bigger, somewhere else. Framing answers "show me all of it",
    // not "and from the front": a user who has turned the model to look at a
    // feature has not asked to be sent back where they started.
    camera
        .frame(([90.0, 90.0, 90.0], [110.0, 110.0, 110.0]))
        .expect("frames");

    let direction_after = offset(&camera);
    let scale_after = length(direction_after);
    for axis in 0..3 {
        assert!(
            (direction_after[axis] / scale_after - direction_before[axis] / scale_before).abs()
                < 1e-3,
            "framing turned the view from {direction_before:?} to {direction_after:?}"
        );
    }
    assert!((camera.target()[0] - 100.0).abs() < 1e-3);
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));

    // Direction alone is not the whole plan view. Its own up vector carries
    // north, and framing after Top must keep that too rather than making the
    // new view degenerate by restoring WORLD_UP along its sight line.
    camera.look_from(StandardView::Top);
    camera
        .frame(([190.0, 190.0, 190.0], [210.0, 210.0, 210.0]))
        .expect("reframes a plan view");
    let centre = camera.target();
    let north = [centre[0], centre[1] + 1.0, centre[2]];
    let north = on_screen(&camera, north);
    assert!(north[1] > 0.0, "framing lost north-up: {north:?}");
    assert!(north[0].abs() < 1e-3, "framing rolled north: {north:?}");
}

#[test]
fn a_large_finite_model_still_has_a_usable_interactive_camera() {
    let mut camera = Camera::new();
    camera.resize(800, 600);
    camera
        .frame(([-1.0e20; 3], [1.0e20; 3]))
        .expect("the bounds and clipping range are representable");

    // A squared-length implementation overflows here even though the vector
    // and its length are finite. That used to make interaction stop and made a
    // standard view fall through to the default direction.
    assert!(camera.distance().is_finite());
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));

    camera.look_from(StandardView::Right);
    let direction = offset(&camera);
    let distance = length(direction);
    assert!(distance.is_finite() && distance > 0.0);
    assert!(
        (direction[0] / distance - 1.0).abs() < 1e-4,
        "{direction:?}"
    );
    assert!((direction[1] / distance).abs() < 1e-4, "{direction:?}");
    assert!((direction[2] / distance).abs() < 1e-4, "{direction:?}");

    camera.orbit(0.2, 0.1);
    camera.zoom(0.2);
    assert!(camera.distance().is_finite());
    assert!(camera.view_projection().iter().all(|v| v.is_finite()));
}

#[test]
fn a_finite_pan_cannot_overflow_half_of_the_camera() {
    let mut camera = framed();

    // Every individual delta and shift is finite. Repeating it eventually
    // reaches the coordinate range's edge; that last gesture must be refused
    // atomically rather than commit infinities to eye and target one field at
    // a time.
    for _ in 0..100 {
        camera.pan(f32::MAX, f32::MAX);
        assert!(camera.eye().iter().all(|value| value.is_finite()));
        assert!(camera.target().iter().all(|value| value.is_finite()));
        assert_ne!(camera.eye(), camera.target());
        assert!(
            camera
                .view_projection()
                .iter()
                .all(|value| value.is_finite())
        );
    }
}

#[test]
fn nothing_that_is_not_a_number_moves_the_camera() {
    let mut camera = framed();
    let before = camera;

    // A gesture handler that divided by a zero delta, or a wheel event that
    // arrived as a NaN, must not leave the view somewhere unreachable.
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        camera.orbit(bad, 0.0);
        camera.orbit(0.0, bad);
        camera.pan(bad, 0.0);
        camera.pan(0.0, bad);
        camera.zoom(bad);
    }
    assert_eq!(
        camera, before,
        "a camera moved by something that is not a number"
    );
}

#[test]
fn orbiting_and_panning_by_no_pixels_leave_the_camera_exactly_alone() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = framed();
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to exercise"
        );
        // A rolled, off-axis pose makes a zero orbit observably harmful: the
        // old implementation levelled the horizon and rebuilt the eye even
        // though the pointer had not moved.
        camera.orbit(0.31, -0.24);
        camera.roll(0.27);
        camera.pan(17.0, -11.0);
        let before = camera;

        for zero in [0.0, -0.0] {
            camera.orbit(zero, zero);
            camera.pan(zero, zero);
            assert_eq!(
                camera, before,
                "{projection:?}: a zero camera gesture changed the view"
            );
        }
    }
}

#[test]
fn a_face_value_from_outside_lands_on_no_face() {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&two_faced(1, 1)).expect("packs");
    builder
        .place(mesh, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    // A face buffer is written by a GPU and read back over a bus, exactly as a
    // pick buffer is. Nothing, and anything out of range, must land on no face
    // rather than on whichever one the number happens to reach.
    for raw in [0, 3, 99, u32::MAX] {
        let face = FacePickId::from_raw(raw, &snapshot);
        assert_eq!(face, FacePickId::NOTHING, "{raw} named a face");
        assert_eq!(snapshot.definition_of_face(face), None);
    }
    assert_eq!(snapshot.definition_of_face(FacePickId::NOTHING), None);

    for raw in [1, 2] {
        let face = FacePickId::from_raw(raw, &snapshot);
        assert_eq!(snapshot.definition_of_face(face), Some(0));
    }
}

#[test]
fn a_face_of_one_picture_names_nothing_in_another() {
    let build = |colour| {
        let mut builder = SnapshotBuilder::new();
        let mesh = builder.add_mesh(&two_faced(1, 1)).expect("packs");
        builder
            .place(mesh, None, &Transform::IDENTITY, colour)
            .expect("places");
        builder.build()
    };
    let first = build([1.0, 0.0, 0.0]);
    let second = build([0.0, 0.0, 1.0]);
    let face = FacePickId::from_raw(1, &first);

    assert_eq!(first.definition_of_face(face), Some(0));
    assert_eq!(
        second.definition_of_face(face),
        None,
        "the same in-range integer must not silently name another picture's face"
    );

    // Decoding the integer against the second picture is a distinct act, and
    // produces a distinct identity.
    let again = FacePickId::from_raw(face.to_raw(), &second);
    assert_eq!(second.definition_of_face(again), Some(0));
    assert_ne!(face, again);
}

#[test]
fn nothing_a_picture_shows_says_which_subshape_a_face_was() {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&two_faced(1, 1)).expect("packs");
    builder
        .place(mesh, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    // A packed picture keeps the partition and nothing of the session it came
    // out of. The kernel's handles are not in these types and cannot be read
    // back out of them: what a document stores about a face is a topology
    // reference, and this is not one.
    let shown = format!("{snapshot:?}");
    for word in ["SubShapeHandle", "ShapeHandle", "SessionId", "SubShapeKind"] {
        assert!(!shown.contains(word), "a picture shows its kernel {word}");
    }

    // The identities are the picture's own numbering, in packing order, and
    // say nothing about which subshape of which shape they were.
    assert_eq!(snapshot.face_count(), 2);
    assert_eq!(
        snapshot.meshes()[0].faces_of_vertices(),
        &[1, 1, 1, 2, 2, 2]
    );
}

#[test]
fn a_mesh_whose_faces_share_a_vertex_is_refused_rather_than_guessed_at() {
    let shape = ShapeHandle::new(SessionId::new(), 1);
    let mut mesh = Mesh::default();
    // Four vertices, two triangles, and the second triangle reuses two of the
    // first one's. A tessellation gives each face its own nodes, so this is
    // not a mesh the kernel produces – and if one ever did, a face identity
    // per vertex could not say which face those two belong to.
    mesh.positions
        .extend_from_slice(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0]);
    mesh.normals
        .extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    mesh.indices.extend_from_slice(&[0, 1, 2, 1, 2, 3]);
    for (ordinal, first_index) in [(0u64, 0u32), (1, 3)] {
        mesh.faces.push(MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, ordinal),
            first_index,
            index_count: 3,
        });
    }

    let mut builder = SnapshotBuilder::new();
    let refusal = builder
        .add_mesh(&mesh)
        .expect_err("a mesh whose faces share a vertex was packed");
    assert_eq!(refusal.kind(), ErrorKind::Input);
    assert!(
        refusal.to_string().contains("share a vertex"),
        "the refusal must say what is wrong with the mesh: {refusal}"
    );
}

#[test]
fn face_count_does_not_depend_on_vertex_storage_order() {
    let shape = ShapeHandle::new(SessionId::new(), 1);
    let mut mesh = divided(&[1, 1]);
    // Face ranges are ranges of indices, not ranges of the vertex array. This
    // remains a valid Mesh when the first face indexes the later vertices and
    // the second indexes the earlier ones.
    mesh.indices.copy_from_slice(&[3, 4, 5, 0, 1, 2]);
    for (ordinal, range) in mesh.faces.iter_mut().enumerate() {
        range.face = SubShapeHandle::new(shape, SubShapeKind::Face, ordinal as u64);
    }
    mesh.validate()
        .expect("the kernel contract accepts the mesh");

    let mut builder = SnapshotBuilder::new();
    let definition = builder.add_mesh(&mesh).expect("packs");
    builder
        .place(definition, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    assert_eq!(snapshot.meshes()[definition].face_count(), 2);
    assert_eq!(snapshot.face_count(), 2);
    for raw in [1, 2] {
        assert_eq!(
            snapshot.definition_of_face(FacePickId::from_raw(raw, &snapshot)),
            Some(definition)
        );
    }
}

#[test]
fn a_refused_mesh_consumes_no_face_identities() {
    let shape = ShapeHandle::new(SessionId::new(), 1);
    let mut shared = Mesh::default();
    shared
        .positions
        .extend_from_slice(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0]);
    shared
        .normals
        .extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    shared.indices.extend_from_slice(&[0, 1, 2, 1, 2, 3]);
    for (ordinal, first_index) in [(0u64, 0u32), (1, 3)] {
        shared.faces.push(MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, ordinal),
            first_index,
            index_count: 3,
        });
    }

    let mut builder = SnapshotBuilder::new();
    builder
        .add_mesh(&shared)
        .expect_err("the shared-vertex mesh must be refused");
    let definition = builder.add_mesh(&divided(&[1])).expect("packs afterwards");
    builder
        .place(definition, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    assert_eq!(snapshot.face_count(), 1, "the refusal consumed face ids");
    assert_eq!(
        snapshot.meshes()[definition].faces_of_vertices(),
        &[1, 1, 1]
    );
    assert_eq!(
        snapshot.definition_of_face(FacePickId::from_raw(1, &snapshot)),
        Some(definition)
    );
}

#[test]
fn the_bounds_of_a_face_are_its_own_triangles_in_every_placement() {
    let mut builder = SnapshotBuilder::new();
    let part = builder.add_mesh(&divided(&[1, 1])).expect("packs");
    builder
        .place(part, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    builder
        .place(part, None, &moved(100.0, 0.0, 0.0), [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    let first = snapshot.face_of(0, 0).expect("numbered");
    let second = snapshot.face_of(0, 1).expect("numbered");
    let (low, high) = snapshot.bounds_of_face(first).expect("the face is drawn");
    let whole = snapshot
        .bounds_of(snapshot.draws()[0].pick)
        .expect("the definition is drawn");

    // `divided` lays its triangles along x, four units apart, so the first
    // face ends where the second begins. The face's box is its own triangles:
    // narrower than the definition's along x, and reaching the second
    // placement a hundred units away.
    assert!(low[0] < high[0]);
    assert_eq!(
        low[0], whole.0[0],
        "the first face starts where the part does"
    );
    assert!(
        high[0] < whole.1[0],
        "the face's box reached past its own triangles: {high:?} against {:?}",
        whole.1
    );
    assert!(
        high[0] > 100.0,
        "the second placement of the face was left out: {high:?}"
    );

    // The two faces are different places, which is what makes framing one of
    // them different from framing the part.
    let other = snapshot.bounds_of_face(second).expect("the face is drawn");
    assert_ne!((low, high), other);
    assert_eq!(
        other.1[0], whole.1[0],
        "the last face ends where the part does"
    );

    // A face of a picture that has been replaced is nowhere at all.
    let elsewhere = {
        let mut builder = SnapshotBuilder::new();
        let part = builder.add_mesh(&divided(&[2, 1])).expect("packs");
        builder
            .place(part, None, &Transform::IDENTITY, [0.0, 0.0, 1.0])
            .expect("places");
        builder.build()
    };
    assert_eq!(elsewhere.bounds_of_face(first), None);
    assert_eq!(snapshot.bounds_of_face(FacePickId::NOTHING), None);
}

#[test]
fn a_picture_numbers_its_faces_where_it_says_it_does() {
    let mut builder = SnapshotBuilder::new();
    let first = builder.add_mesh(&divided(&[1, 1])).expect("packs");
    let second = builder.add_mesh(&divided(&[2, 1])).expect("packs");
    for (part, x) in [(first, 0.0), (second, 50.0)] {
        builder
            .place(part, None, &moved(x, 0.0, 0.0), [1.0, 1.0, 1.0])
            .expect("places");
    }
    let snapshot = builder.build();

    // Every face of every definition, asked for by position and answered with
    // an identity that resolves back to the same position.
    let mut seen = Vec::new();
    for definition in 0..snapshot.meshes().len() {
        for ordinal in 0..snapshot.meshes()[definition].face_count() {
            let face = snapshot.face_of(definition, ordinal).expect("numbered");
            assert_eq!(snapshot.definition_of_face(face), Some(definition));
            assert!(!seen.contains(&face), "two positions gave one identity");
            seen.push(face);
        }
    }
    assert_eq!(seen.len(), snapshot.face_count());

    // Positions this picture does not have are not numbered at all.
    assert_eq!(snapshot.face_of(0, 2), None);
    assert_eq!(snapshot.face_of(2, 0), None);

    // And a face of one picture is not a face of another, however alike the
    // two pictures look.
    let other = {
        let mut builder = SnapshotBuilder::new();
        let part = builder.add_mesh(&divided(&[1, 1])).expect("packs");
        builder
            .place(part, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
            .expect("places");
        builder.build()
    };
    assert_eq!(other.definition_of_face(seen[0]), None);
}

/// Two definitions, each placed twice, with two faces each.
fn two_definitions_placed_twice() -> RenderSnapshot {
    let mut builder = SnapshotBuilder::new();
    let first = builder.add_mesh(&divided(&[1, 1])).expect("packs");
    let second = builder.add_mesh(&divided(&[2, 1])).expect("packs");
    for definition in [first, second] {
        for x in [0.0, 200.0] {
            builder
                .place(
                    definition,
                    None,
                    &moved(x + definition as f64 * 50.0, 0.0, 0.0),
                    [1.0, 1.0, 1.0],
                )
                .expect("places");
        }
    }
    builder.build()
}

#[test]
fn a_picture_begins_with_every_definition_drawn() {
    let snapshot = two_definitions_placed_twice();
    let visibility = Visibility::new(&snapshot);

    assert!(!visibility.anything_hidden());
    for definition in 0..snapshot.meshes().len() {
        assert!(visibility.shows(definition, &snapshot));
    }
    assert_eq!(visibility.bounds(&snapshot), snapshot.bounds());
}

#[test]
fn a_definition_that_draws_nothing_cannot_be_hidden() {
    let mut builder = SnapshotBuilder::new();
    let empty = builder.add_mesh(&nothing_at_all()).expect("packs");
    builder
        .place(empty, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();
    let pick = snapshot.pick_of(empty).expect("the definition has a row");
    let mut visibility = Visibility::new(&snapshot);

    assert_eq!(snapshot.bounds_of(pick), None, "the gate needs no pixels");
    assert!(
        !visibility.hide(Marked::Definition(pick), &snapshot),
        "Hide selected reported a change for a definition that draws nothing"
    );
    assert!(
        !visibility.anything_hidden(),
        "an empty definition made Show all available"
    );
}

#[test]
fn hiding_one_definition_hides_all_of_it_and_none_of_its_neighbour() {
    let snapshot = two_definitions_placed_twice();
    let mut visibility = Visibility::new(&snapshot);
    let hidden = snapshot.pick_of(0).expect("drawn");

    assert!(visibility.hide(Marked::Definition(hidden), &snapshot));
    assert!(visibility.anything_hidden());

    // Every placement of it, because a definition is what is hidden and a
    // placement is only where it was put.
    assert!(!visibility.shows(0, &snapshot));
    assert_eq!(
        snapshot
            .draws()
            .iter()
            .filter(|item| item.mesh == 0)
            .count(),
        2,
        "the gate needs the hidden definition to be placed more than once"
    );
    // And its neighbour is untouched, in both of its placements.
    assert!(visibility.shows(1, &snapshot));

    // A face of a hidden definition is hidden with it: hiding is per
    // definition, and there is no state in which one face of a hidden part is
    // still on screen.
    for ordinal in 0..snapshot.meshes()[0].face_count() {
        let face = snapshot.face_of(0, ordinal).expect("numbered");
        assert_eq!(snapshot.definition_of_face(face), Some(0));
        assert!(!visibility.shows(0, &snapshot));
    }

    // Hiding a face hides the part it is on, not the face alone.
    let mut by_face = Visibility::new(&snapshot);
    let face = snapshot.face_of(1, 0).expect("numbered");
    assert!(by_face.hide(Marked::Face(face), &snapshot));
    assert!(!by_face.shows(1, &snapshot));
    assert!(by_face.shows(0, &snapshot));
}

#[test]
fn nothing_a_stale_or_foreign_mark_can_say_hides_anything() {
    let snapshot = two_definitions_placed_twice();
    let elsewhere = {
        let mut builder = SnapshotBuilder::new();
        let part = builder.add_mesh(&divided(&[1, 1])).expect("packs");
        builder
            .place(part, None, &Transform::IDENTITY, [0.0, 0.0, 1.0])
            .expect("places");
        builder.build()
    };
    let mut visibility = Visibility::new(&snapshot);

    // Nothing, a pick of another picture, and a face of another picture. The
    // raw numbers are all in range here, which is the point: what refuses them
    // is the identity they carry, not their size.
    let foreign_pick = elsewhere.pick_of(0).expect("drawn");
    let foreign_face = elsewhere.face_of(0, 0).expect("numbered");
    for mark in [
        Marked::Nothing,
        Marked::Definition(foreign_pick),
        Marked::Face(foreign_face),
        Marked::Definition(PickId::NOTHING),
        Marked::Face(FacePickId::NOTHING),
    ] {
        assert!(!visibility.hide(mark, &snapshot), "{mark:?} hid something");
    }
    assert!(!visibility.anything_hidden());

    // And a mask made for one picture applies to no other: applied to the
    // wrong one it hides nothing rather than hiding whatever sits at the same
    // index.
    let mut ours = Visibility::new(&snapshot);
    assert!(ours.hide(
        Marked::Definition(snapshot.pick_of(0).expect("drawn")),
        &snapshot
    ));
    assert!(
        ours.shows(0, &elsewhere),
        "a mask reached into another picture"
    );
    assert_eq!(ours.hidden_in(&elsewhere), &[] as &[bool]);
    assert_eq!(ours.bounds(&elsewhere), elsewhere.bounds());
}

#[test]
fn what_is_hidden_is_not_part_of_where_the_model_is() {
    let snapshot = two_definitions_placed_twice();
    let mut visibility = Visibility::new(&snapshot);
    let whole = snapshot.bounds().expect("the picture has an extent");

    assert!(visibility.hide(
        Marked::Definition(snapshot.pick_of(0).expect("drawn")),
        &snapshot
    ));
    let visible = visibility
        .bounds(&snapshot)
        .expect("something is still drawn");
    assert_ne!(visible, whole);
    assert_eq!(
        visible,
        snapshot
            .bounds_of(snapshot.pick_of(1).expect("drawn"))
            .expect("the other definition is somewhere"),
        "what is left is exactly the definition still drawn"
    );

    // Everything hidden is nowhere at all, which is what an empty picture is:
    // a camera has nothing to be pointed at rather than being pointed at the
    // origin.
    assert!(visibility.hide(
        Marked::Definition(snapshot.pick_of(1).expect("drawn")),
        &snapshot
    ));
    assert_eq!(visibility.bounds(&snapshot), None);

    // And showing everything again restores exactly what was there.
    assert!(visibility.show_all());
    assert_eq!(visibility.bounds(&snapshot), Some(whole));
    for definition in 0..snapshot.meshes().len() {
        assert!(visibility.shows(definition, &snapshot));
    }
}

#[test]
fn asking_twice_for_the_same_visibility_changes_nothing_the_second_time() {
    let snapshot = two_definitions_placed_twice();
    let mut visibility = Visibility::new(&snapshot);
    let pick = snapshot.pick_of(0).expect("drawn");

    assert!(visibility.hide(Marked::Definition(pick), &snapshot));
    let after = visibility.clone();
    assert!(
        !visibility.hide(Marked::Definition(pick), &snapshot),
        "hiding what is already hidden is not a change"
    );
    assert_eq!(visibility, after);

    assert!(visibility.show_all());
    let shown = visibility.clone();
    assert!(
        !visibility.show_all(),
        "showing everything when nothing is hidden is not a change"
    );
    assert_eq!(visibility, shown);
}

#[test]
fn two_pictures_of_the_same_triangles_do_not_share_what_is_hidden() {
    // The same geometry, drawn the same way, meaning different things: one
    // picture's faces carry a document's names and the other's do not. What is
    // hidden in one must not be hidden in the other, because "the same
    // definition" is not a claim geometry alone can make.
    let build = |context: Option<[u8; 32]>| {
        let mut builder = SnapshotBuilder::new();
        if let Some(context) = context {
            builder
                .bind_identities_to(ContentHash::from_bytes(context))
                .expect("binds once");
        }
        let part = builder.add_mesh(&divided(&[1, 1])).expect("packs");
        builder
            .place(part, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
            .expect("places");
        builder.build()
    };
    let one = build(Some([7; 32]));
    let other = build(Some([9; 32]));

    let mut visibility = Visibility::new(&one);
    assert!(visibility.hide(Marked::Definition(one.pick_of(0).expect("drawn")), &one));

    assert!(!visibility.shows(0, &one));
    assert!(
        visibility.shows(0, &other),
        "a picture with other meanings inherited what was hidden"
    );
    assert!(
        !visibility.hide(Marked::Definition(one.pick_of(0).expect("drawn")), &other),
        "a pick of one picture hid a definition of another"
    );
}

/// Three definitions, each placed twice, each divided into faces.
fn three_definitions_placed_twice() -> RenderSnapshot {
    let mut builder = SnapshotBuilder::new();
    let parts = [
        builder.add_mesh(&divided(&[1, 1])).expect("packs"),
        builder.add_mesh(&divided(&[2, 1])).expect("packs"),
        builder.add_mesh(&divided(&[1, 2])).expect("packs"),
    ];
    for part in parts {
        for x in [0.0, 200.0] {
            builder
                .place(
                    part,
                    None,
                    &moved(x + part as f64 * 40.0, 0.0, 0.0),
                    [1.0, 1.0, 1.0],
                )
                .expect("places");
        }
    }
    builder.build()
}

#[test]
fn isolating_one_definition_leaves_exactly_that_one_drawn() {
    let snapshot = three_definitions_placed_twice();
    let mut visibility = Visibility::new(&snapshot);
    let keep = snapshot.pick_of(1).expect("drawn");

    assert!(visibility.can_isolate(Marked::Definition(keep), &snapshot));
    assert!(visibility.isolate(Marked::Definition(keep), &snapshot));

    // The one chosen, and nothing else.
    assert!(visibility.shows(1, &snapshot));
    assert!(!visibility.shows(0, &snapshot));
    assert!(!visibility.shows(2, &snapshot));

    // Every placement of it, because what is kept is the definition and not
    // the spot it was drawn in.
    assert_eq!(
        snapshot
            .draws()
            .iter()
            .filter(|item| item.mesh == 1)
            .count(),
        2,
        "the gate needs the kept definition to be drawn in two places"
    );
    assert_eq!(
        visibility.bounds(&snapshot),
        snapshot.bounds_of(keep),
        "what is left is exactly the chosen definition, in both its places"
    );

    // And every face of both neighbours is gone with them: hiding is per
    // definition, so there is no state in which one face of a removed part is
    // still on screen.
    for definition in [0, 2] {
        for ordinal in 0..snapshot.meshes()[definition].face_count() {
            let face = snapshot.face_of(definition, ordinal).expect("numbered");
            assert_eq!(snapshot.definition_of_face(face), Some(definition));
            assert!(!visibility.shows(definition, &snapshot));
        }
    }
}

#[test]
fn isolating_a_face_keeps_the_part_it_is_on() {
    let snapshot = three_definitions_placed_twice();
    let mut visibility = Visibility::new(&snapshot);
    let face = snapshot.face_of(2, 1).expect("numbered");

    assert!(visibility.can_isolate(Marked::Face(face), &snapshot));
    assert!(visibility.isolate(Marked::Face(face), &snapshot));

    // The whole part the face is on, not the face alone: this operation deals
    // in definitions, and a part reduced to one face would be a different part.
    assert!(visibility.shows(2, &snapshot));
    assert!(!visibility.shows(0, &snapshot));
    assert!(!visibility.shows(1, &snapshot));
    assert_eq!(
        visibility.bounds(&snapshot),
        snapshot.bounds_of(snapshot.pick_of(2).expect("drawn"))
    );
}

#[test]
fn isolating_reveals_nothing_that_was_already_hidden() {
    let snapshot = three_definitions_placed_twice();
    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.hide(
        Marked::Definition(snapshot.pick_of(0).expect("drawn")),
        &snapshot
    ));

    // Two left, so there is still something to remove.
    assert!(visibility.isolate(
        Marked::Definition(snapshot.pick_of(1).expect("drawn")),
        &snapshot
    ));
    assert!(visibility.shows(1, &snapshot));
    assert!(
        !visibility.shows(0, &snapshot),
        "isolating put back something that had been hidden"
    );
    assert!(!visibility.shows(2, &snapshot));

    // Show all is the way back, and it puts back everything.
    assert!(visibility.show_all());
    for definition in 0..snapshot.meshes().len() {
        assert!(visibility.shows(definition, &snapshot));
    }
    assert_eq!(visibility.bounds(&snapshot), snapshot.bounds());
}

#[test]
fn a_definition_already_on_its_own_is_isolated_already() {
    let snapshot = three_definitions_placed_twice();
    let mut visibility = Visibility::new(&snapshot);
    let keep = snapshot.pick_of(1).expect("drawn");
    assert!(visibility.isolate(Marked::Definition(keep), &snapshot));
    let after = visibility.clone();

    // Nothing else is drawn, so there is nothing to remove and the action is
    // not offered. Asking again changes nothing at all.
    assert!(!visibility.can_isolate(Marked::Definition(keep), &snapshot));
    assert!(
        !visibility.isolate(Marked::Definition(keep), &snapshot),
        "isolating what is already alone claimed a change"
    );
    assert_eq!(visibility, after);

    // Nor is it offered for something that is not drawn at all.
    let hidden = snapshot.pick_of(0).expect("drawn");
    assert!(!visibility.can_isolate(Marked::Definition(hidden), &snapshot));
    assert!(!visibility.isolate(Marked::Definition(hidden), &snapshot));
    assert_eq!(visibility, after);
}

#[test]
fn geometry_that_is_already_nowhere_is_neither_isolated_nor_hidden_by_isolating() {
    let mut builder = SnapshotBuilder::new();
    let drawn = builder.add_mesh(&divided(&[1, 1])).expect("packs");
    // One with no triangles, and one with triangles that is never placed.
    let empty = builder.add_mesh(&Mesh::default()).expect("packs");
    let unplaced = builder.add_mesh(&divided(&[1, 1])).expect("packs");
    builder
        .place(drawn, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    builder
        .place(empty, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();
    let mut visibility = Visibility::new(&snapshot);

    // The only definition putting anything on screen is alone already, so
    // there is nothing to isolate away.
    assert!(!visibility.can_isolate(
        Marked::Definition(snapshot.pick_of(drawn).expect("has a row")),
        &snapshot
    ));
    assert!(!visibility.isolate(
        Marked::Definition(snapshot.pick_of(drawn).expect("has a row")),
        &snapshot
    ));

    // And neither of the two that draw nothing can be isolated to, for the
    // same reason neither can be hidden: they are already nowhere.
    for definition in [empty, unplaced] {
        let pick = snapshot.pick_of(definition).expect("has a row");
        assert!(!visibility.can_isolate(Marked::Definition(pick), &snapshot));
        assert!(!visibility.isolate(Marked::Definition(pick), &snapshot));
        assert!(!visibility.can_hide(Marked::Definition(pick), &snapshot));
    }

    // Nothing was marked hidden by any of it: a row that draws nothing must
    // not start reading as hidden.
    assert!(
        !visibility.anything_hidden(),
        "something already nowhere was marked as hidden"
    );
}

#[test]
fn isolating_something_this_picture_did_not_issue_isolates_nothing() {
    let snapshot = three_definitions_placed_twice();
    let elsewhere = {
        let mut builder = SnapshotBuilder::new();
        let part = builder.add_mesh(&divided(&[1, 1])).expect("packs");
        builder
            .place(part, None, &Transform::IDENTITY, [0.0, 0.0, 1.0])
            .expect("places");
        builder.build()
    };
    let mut visibility = Visibility::new(&snapshot);
    let before = visibility.clone();

    for mark in [
        Marked::Nothing,
        Marked::Definition(PickId::NOTHING),
        Marked::Face(FacePickId::NOTHING),
        Marked::Definition(elsewhere.pick_of(0).expect("drawn")),
        Marked::Face(elsewhere.face_of(0, 0).expect("numbered")),
    ] {
        assert!(
            !visibility.can_isolate(mark, &snapshot),
            "{mark:?} was offered"
        );
        assert!(
            !visibility.isolate(mark, &snapshot),
            "{mark:?} isolated something"
        );
    }
    assert_eq!(visibility, before);

    // And a mask made for one picture reaches into no other.
    let mut ours = Visibility::new(&snapshot);
    assert!(ours.isolate(
        Marked::Definition(snapshot.pick_of(0).expect("drawn")),
        &snapshot
    ));
    assert!(ours.shows(0, &elsewhere));
    assert_eq!(ours.bounds(&elsewhere), elsewhere.bounds());
}

#[test]
fn two_pictures_of_the_same_triangles_do_not_share_what_is_isolated() {
    // The same geometry meaning different things: one picture's faces carry a
    // document's names and the other's do not.
    let build = |context: [u8; 32]| {
        let mut builder = SnapshotBuilder::new();
        builder
            .bind_identities_to(ContentHash::from_bytes(context))
            .expect("binds once");
        let first = builder.add_mesh(&divided(&[1, 1])).expect("packs");
        let second = builder.add_mesh(&divided(&[2, 1])).expect("packs");
        for part in [first, second] {
            builder
                .place(part, None, &moved(part as f64 * 40.0, 0.0, 0.0), [1.0; 3])
                .expect("places");
        }
        builder.build()
    };
    let one = build([7; 32]);
    let other = build([9; 32]);

    let mut visibility = Visibility::new(&one);
    assert!(visibility.isolate(Marked::Definition(one.pick_of(0).expect("drawn")), &one));

    assert!(!visibility.shows(1, &one));
    assert!(
        visibility.shows(1, &other),
        "a picture with other meanings inherited what was isolated"
    );
    assert!(
        !visibility.can_isolate(Marked::Definition(one.pick_of(0).expect("drawn")), &other),
        "a mark of one picture was offered against another"
    );
}

#[test]
fn showing_one_hidden_definition_returns_that_one_and_no_other() {
    let snapshot = three_definitions_placed_twice();
    let mut visibility = Visibility::new(&snapshot);
    let first = snapshot.pick_of(0).expect("drawn");
    let third = snapshot.pick_of(2).expect("drawn");
    assert!(visibility.isolate(
        Marked::Definition(snapshot.pick_of(1).expect("drawn")),
        &snapshot
    ));

    assert!(visibility.can_show(Marked::Definition(first), &snapshot));
    assert!(visibility.show(Marked::Definition(first), &snapshot));

    // The one asked for, and nothing else that was hidden.
    assert!(visibility.shows(0, &snapshot));
    assert!(
        visibility.shows(1, &snapshot),
        "what was drawn stopped being drawn"
    );
    assert!(
        !visibility.shows(2, &snapshot),
        "showing one definition brought back another"
    );

    // Every placement of it, and every face of it: what was hidden was the
    // definition, so what returns is the definition.
    assert_eq!(
        snapshot
            .draws()
            .iter()
            .filter(|item| item.mesh == 0)
            .count(),
        2,
        "the gate needs the returned definition to be drawn in two places"
    );
    for ordinal in 0..snapshot.meshes()[0].face_count() {
        let face = snapshot.face_of(0, ordinal).expect("numbered");
        assert_eq!(snapshot.definition_of_face(face), Some(0));
    }

    // The extent grows by exactly what came back.
    let mut without = Visibility::new(&snapshot);
    assert!(without.isolate(
        Marked::Definition(snapshot.pick_of(1).expect("drawn")),
        &snapshot
    ));
    let mut both = Visibility::new(&snapshot);
    assert!(both.hide(Marked::Definition(third), &snapshot));
    assert_eq!(
        visibility.bounds(&snapshot),
        both.bounds(&snapshot),
        "what is drawn is exactly the two definitions, and their extent says so"
    );
    assert_ne!(visibility.bounds(&snapshot), without.bounds(&snapshot));
}

#[test]
fn showing_something_that_is_drawn_or_nowhere_or_foreign_changes_nothing() {
    let mut builder = SnapshotBuilder::new();
    let drawn = builder.add_mesh(&divided(&[1, 1])).expect("packs");
    let other = builder.add_mesh(&divided(&[2, 1])).expect("packs");
    let empty = builder.add_mesh(&Mesh::default()).expect("packs");
    let unplaced = builder.add_mesh(&divided(&[1, 1])).expect("packs");
    for part in [drawn, other, empty] {
        builder
            .place(part, None, &moved(part as f64 * 40.0, 0.0, 0.0), [1.0; 3])
            .expect("places");
    }
    let snapshot = builder.build();
    let elsewhere = {
        let mut builder = SnapshotBuilder::new();
        let part = builder.add_mesh(&divided(&[1, 1])).expect("packs");
        builder
            .place(part, None, &Transform::IDENTITY, [0.0, 0.0, 1.0])
            .expect("places");
        builder.build()
    };

    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.hide(
        Marked::Definition(snapshot.pick_of(other).expect("drawn")),
        &snapshot
    ));
    let before = visibility.clone();

    // Already drawn, drawing nothing wherever it is, and belonging to another
    // picture: none of these is a way to change what is on screen.
    for mark in [
        Marked::Definition(snapshot.pick_of(drawn).expect("has a row")),
        Marked::Definition(snapshot.pick_of(empty).expect("has a row")),
        Marked::Definition(snapshot.pick_of(unplaced).expect("has a row")),
        Marked::Nothing,
        Marked::Definition(PickId::NOTHING),
        Marked::Face(FacePickId::NOTHING),
        Marked::Definition(elsewhere.pick_of(0).expect("drawn")),
        Marked::Face(elsewhere.face_of(0, 0).expect("numbered")),
    ] {
        assert!(
            !visibility.can_show(mark, &snapshot),
            "{mark:?} was offered"
        );
        assert!(
            !visibility.show(mark, &snapshot),
            "{mark:?} changed something"
        );
    }
    assert_eq!(visibility, before);

    // The one that really is hidden still comes back, so the refusals above
    // are about those marks and not about the operation having stopped.
    assert!(visibility.show(
        Marked::Definition(snapshot.pick_of(other).expect("drawn")),
        &snapshot
    ));
    assert!(visibility.shows(other, &snapshot));

    // And asking again, now that it is drawn, is not a change.
    assert!(!visibility.show(
        Marked::Definition(snapshot.pick_of(other).expect("drawn")),
        &snapshot
    ));
    assert!(!visibility.anything_hidden());
}

#[test]
fn two_pictures_of_the_same_triangles_do_not_share_what_is_shown() {
    let build = |context: [u8; 32]| {
        let mut builder = SnapshotBuilder::new();
        builder
            .bind_identities_to(ContentHash::from_bytes(context))
            .expect("binds once");
        let first = builder.add_mesh(&divided(&[1, 1])).expect("packs");
        let second = builder.add_mesh(&divided(&[2, 1])).expect("packs");
        for part in [first, second] {
            builder
                .place(part, None, &moved(part as f64 * 40.0, 0.0, 0.0), [1.0; 3])
                .expect("places");
        }
        builder.build()
    };
    let one = build([7; 32]);
    let other = build([9; 32]);

    let mut visibility = Visibility::new(&one);
    assert!(visibility.hide(Marked::Definition(one.pick_of(0).expect("drawn")), &one));

    // A mark of one picture asks nothing of another, however alike the two
    // look: what refuses it is the identity it carries, not its size.
    assert!(!visibility.can_show(Marked::Definition(one.pick_of(0).expect("drawn")), &other));
    assert!(!visibility.show(Marked::Definition(one.pick_of(0).expect("drawn")), &other));
    assert!(
        !visibility.shows(0, &one),
        "the mask stopped applying to its own picture"
    );
    assert!(
        visibility.shows(0, &other),
        "a mask reached into another picture"
    );
}

#[test]
fn a_mask_made_for_one_picture_cannot_be_worked_on_through_another() {
    // The mirror of the marks-from-elsewhere gates: here the mark is this
    // picture's own and valid, and it is the *mask* that belongs somewhere
    // else. Without a check of its own, every operation would resolve the mark
    // happily and then set a bit belonging to a different picture.
    let one = three_definitions_placed_twice();
    let other = {
        let mut builder = SnapshotBuilder::new();
        let part = builder.add_mesh(&divided(&[1, 1])).expect("packs");
        let second = builder.add_mesh(&divided(&[2, 2])).expect("packs");
        for definition in [part, second] {
            builder
                .place(
                    definition,
                    None,
                    &moved(definition as f64 * 30.0, 0.0, 0.0),
                    [1.0, 1.0, 1.0],
                )
                .expect("places");
        }
        builder.build()
    };
    let foreign = Visibility::new(&one);
    let own_pick = other.pick_of(0).expect("drawn");
    let own_face = other.face_of(0, 0).expect("numbered");

    // Every question refuses, and every operation refuses.
    for mark in [Marked::Definition(own_pick), Marked::Face(own_face)] {
        let mut mask = foreign.clone();
        assert!(
            !mask.can_hide(mark, &other),
            "{mark:?} was offered a way out"
        );
        assert!(
            !mask.can_isolate(mark, &other),
            "{mark:?} was offered isolation"
        );
        assert!(
            !mask.can_show(mark, &other),
            "{mark:?} was offered a way back"
        );
        assert!(!mask.hide(mark, &other), "a foreign mask hid something");
        assert!(
            !mask.isolate(mark, &other),
            "a foreign mask isolated something"
        );
        assert!(!mask.show(mark, &other), "a foreign mask showed something");
        assert_eq!(mask, foreign, "a foreign mask was written through");
    }

    // And it still says the other picture is fully drawn, because a mask it
    // does not belong to hides nothing there.
    assert_eq!(foreign.bounds(&other), other.bounds());
    for definition in 0..other.meshes().len() {
        assert!(foreign.shows(definition, &other));
    }
}

#[test]
fn every_change_to_what_is_drawn_records_exactly_what_it_replaced() {
    let snapshot = three_definitions_placed_twice();
    let picks: Vec<_> = (0..3)
        .map(|definition| snapshot.pick_of(definition).expect("drawn"))
        .collect();

    // Each of the four ways what is drawn can change, each set up in an
    // arrangement where it really does change something, and each taken back.
    let hide_one = |mask: &mut Visibility| mask.hide(Marked::Definition(picks[0]), &snapshot);
    let isolate = |mask: &mut Visibility| mask.isolate(Marked::Definition(picks[1]), &snapshot);
    let show_one = |mask: &mut Visibility| mask.show(Marked::Definition(picks[0]), &snapshot);
    let show_all = |mask: &mut Visibility| mask.show_all();

    /// One way of changing what is drawn: what to call it, what it does, and
    /// whether it needs something already hidden to have anything to do.
    type Operation<'a> = (&'a str, &'a dyn Fn(&mut Visibility) -> bool, bool);

    let operations: [Operation<'_>; 4] = [
        ("hide", &hide_one, false),
        ("isolate", &isolate, false),
        ("show", &show_one, true),
        ("show all", &show_all, true),
    ];

    for (what, apply, needs_something_hidden) in operations {
        let mut visibility = Visibility::new(&snapshot);
        assert!(
            !visibility.can_undo(&snapshot),
            "a fresh picture has something to take back"
        );
        if needs_something_hidden {
            assert!(visibility.hide(Marked::Definition(picks[0]), &snapshot));
            assert!(visibility.hide(Marked::Definition(picks[2]), &snapshot));
        }

        let before = visibility.hidden_in(&snapshot).to_vec();
        assert!(
            apply(&mut visibility),
            "{what} did nothing, so the gate proves nothing"
        );
        assert_ne!(
            visibility.hidden_in(&snapshot),
            before.as_slice(),
            "{what} claimed a change and made none"
        );
        assert!(visibility.can_undo(&snapshot), "{what} recorded nothing");

        assert!(visibility.undo(&snapshot), "{what} could not be taken back");
        assert_eq!(
            visibility.hidden_in(&snapshot),
            before.as_slice(),
            "taking back {what} did not restore exactly what it replaced"
        );

        // One level, consumed. A second press has nothing to take back, and
        // does not put the change back either: this is not a toggle and not a
        // redo.
        assert!(!visibility.can_undo(&snapshot));
        assert!(
            !visibility.undo(&snapshot),
            "{what} could be taken back twice"
        );
        assert_eq!(visibility.hidden_in(&snapshot), before.as_slice());

        // And a later change records again, so history is not spent for good.
        assert!(
            apply(&mut visibility),
            "{what} stopped working after an undo"
        );
        assert!(visibility.can_undo(&snapshot));
    }
}

#[test]
fn taking_back_show_all_restores_a_mixed_arrangement_rather_than_all_of_it() {
    let snapshot = three_definitions_placed_twice();
    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.hide(
        Marked::Definition(snapshot.pick_of(0).expect("drawn")),
        &snapshot
    ));
    assert!(visibility.hide(
        Marked::Definition(snapshot.pick_of(2).expect("drawn")),
        &snapshot
    ));
    let mixed = visibility.hidden_in(&snapshot).to_vec();
    let extent = visibility.bounds(&snapshot);
    assert_eq!(
        mixed,
        [true, false, true],
        "the gate needs a mixed arrangement"
    );

    assert!(visibility.show_all());
    assert_eq!(visibility.bounds(&snapshot), snapshot.bounds());

    // Two of three hidden, not none and not all: an approximation would be a
    // different picture.
    assert!(visibility.undo(&snapshot));
    assert_eq!(visibility.hidden_in(&snapshot), mixed.as_slice());
    assert_eq!(
        visibility.bounds(&snapshot),
        extent,
        "the extent after taking it back is not the extent before"
    );
}

#[test]
fn geometry_that_is_nowhere_manufactures_no_history() {
    let mut builder = SnapshotBuilder::new();
    let drawn = builder.add_mesh(&divided(&[1, 1])).expect("packs");
    let empty = builder.add_mesh(&Mesh::default()).expect("packs");
    let unplaced = builder.add_mesh(&divided(&[1, 1])).expect("packs");
    for part in [drawn, empty] {
        builder
            .place(part, None, &moved(part as f64 * 40.0, 0.0, 0.0), [1.0; 3])
            .expect("places");
    }
    let snapshot = builder.build();
    let mut visibility = Visibility::new(&snapshot);

    // Neither can be hidden, shown or isolated to, so neither leaves a record
    // of anything to take back.
    for definition in [empty, unplaced] {
        let pick = snapshot.pick_of(definition).expect("has a row");
        assert!(!visibility.hide(Marked::Definition(pick), &snapshot));
        assert!(!visibility.show(Marked::Definition(pick), &snapshot));
        assert!(!visibility.isolate(Marked::Definition(pick), &snapshot));
    }
    assert!(
        !visibility.can_undo(&snapshot),
        "geometry that is already nowhere left something to take back"
    );
    assert!(!visibility.undo(&snapshot));
}

#[test]
fn history_belongs_to_the_picture_that_recorded_it() {
    let one = three_definitions_placed_twice();
    let other = {
        let mut builder = SnapshotBuilder::new();
        let part = builder.add_mesh(&divided(&[1, 1])).expect("packs");
        let second = builder.add_mesh(&divided(&[2, 2])).expect("packs");
        for definition in [part, second] {
            builder
                .place(
                    definition,
                    None,
                    &moved(definition as f64 * 30.0, 0.0, 0.0),
                    [1.0, 1.0, 1.0],
                )
                .expect("places");
        }
        builder.build()
    };

    let mut visibility = Visibility::new(&one);
    assert!(visibility.hide(Marked::Definition(one.pick_of(0).expect("drawn")), &one));
    let recorded = visibility.clone();

    // A record made for one picture says nothing about another, and cannot be
    // spent through it.
    assert!(!visibility.can_undo(&other));
    assert!(
        !visibility.undo(&other),
        "history of one picture was applied through another"
    );
    assert_eq!(
        visibility.hidden_in(&one),
        recorded.hidden_in(&one),
        "a refused undo changed the picture it does belong to"
    );

    // And it is still there for the picture it belongs to.
    assert!(visibility.can_undo(&one));
    assert!(visibility.undo(&one));
    assert!(visibility.shows(0, &one));
}

/// A camera looking at the origin from a fixed distance, framed on a box.
fn a_framed_camera(width: u32, height: u32) -> Camera {
    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(([-10.0, -10.0, -10.0], [10.0, 10.0, 10.0]))
        .expect("frames");
    camera
}

#[test]
fn a_camera_draws_as_an_eye_sees_until_it_is_asked_not_to() {
    let camera = a_framed_camera(200, 200);
    assert_eq!(
        camera.projection_mode(),
        Projection::Perspective,
        "a new camera does not draw the way it always has"
    );

    // Asking for the projection already in use is not a change.
    let mut same = camera;
    assert!(!same.set_projection(Projection::Perspective));
    assert_eq!(same, camera);
}

#[test]
fn equal_lengths_at_different_depths_project_equally_only_in_a_drawing() {
    let mut camera = Camera::new();
    camera.resize(200, 200);
    camera
        .frame(([-20.0, -60.0, -20.0], [20.0, 60.0, 20.0]))
        .expect("frames");

    // Two segments of the same world length, at different distances along the
    // direction the camera looks.
    let near = ([-5.0, -20.0, 0.0], [5.0, -20.0, 0.0]);
    let far = ([-5.0, 20.0, 0.0], [5.0, 20.0, 0.0]);
    let width_of = |camera: &Camera, segment: ([f32; 3], [f32; 3])| {
        (projected(&camera.view_projection(), segment.1)[0]
            - projected(&camera.view_projection(), segment.0)[0])
            .abs()
    };

    // As an eye sees: the further one is smaller.
    let (near_seen, far_seen) = (width_of(&camera, near), width_of(&camera, far));
    assert!(
        near_seen > far_seen * 1.2,
        "a perspective view drew equal lengths alike: {near_seen} against {far_seen}"
    );

    // As a drawing shows: equal is equal.
    assert!(camera.set_projection(Projection::Orthographic));
    let (near_drawn, far_drawn) = (width_of(&camera, near), width_of(&camera, far));
    assert!(
        (near_drawn - far_drawn).abs() < 1e-5,
        "an orthographic view drew equal lengths differently: {near_drawn} against {far_drawn}"
    );
}

#[test]
fn changing_projection_keeps_what_is_looked_at_and_how_big_it_is() {
    // Looking at something that is not at the origin, from a view with an up
    // axis of its own: a camera already pointing where a reset would send it
    // could not tell a reset from a change that kept everything.
    let mut camera = Camera::new();
    camera.resize(320, 200);
    camera
        .frame(([90.0, 40.0, 10.0], [110.0, 60.0, 30.0]))
        .expect("frames");
    camera.look_from(StandardView::Top);
    let camera = camera;
    assert_ne!(
        camera.target(),
        [0.0; 3],
        "the gate needs an off-centre target"
    );
    let mut flat = camera;
    assert!(flat.set_projection(Projection::Orthographic));

    assert_eq!(flat.target(), camera.target(), "the target moved");
    assert_eq!(flat.eye(), camera.eye(), "the eye moved");
    // The up axis is what decides which way round the picture is, so it is
    // checked by what it decides: two points keep their order on screen.
    let target = camera.target();
    for offset in [[7.0, 0.0, 0.0], [0.0, 7.0, 0.0], [0.0, 0.0, 7.0]] {
        let point = [
            target[0] + offset[0],
            target[1] + offset[1],
            target[2] + offset[2],
        ];
        let seen = projected(&camera.view_projection(), point);
        let drawn = projected(&flat.view_projection(), point);
        assert!(
            seen[0].signum() == drawn[0].signum() && seen[1].signum() == drawn[1].signum(),
            "a point at {offset:?} moved to another quadrant: {seen:?} became {drawn:?}"
        );
    }
    assert_eq!(flat.width(), camera.width());
    assert_eq!(flat.height(), camera.height());
    let (before, after) = (camera.world_per_pixel(), flat.world_per_pixel());
    assert!(
        (before - after).abs() <= before * 1e-5,
        "the model changed size on screen: {before} per pixel became {after}"
    );

    // And back again, with no camera operation in between, is the view that
    // was there before, to the last few bits of the matrix.
    let mut back = flat;
    assert!(back.set_projection(Projection::Perspective));
    for (index, (there, again)) in camera
        .view_projection()
        .iter()
        .zip(back.view_projection().iter())
        .enumerate()
    {
        assert!(
            (there - again).abs() <= there.abs().max(1.0) * 1e-5,
            "entry {index} of the matrix came back as {again} instead of {there}"
        );
    }
}

#[test]
fn zooming_a_drawing_changes_its_scale_and_not_where_the_eye_is() {
    let mut camera = a_framed_camera(200, 200);
    assert!(camera.set_projection(Projection::Orthographic));
    let (eye, target) = (camera.eye(), camera.target());
    let before = camera.world_per_pixel();

    camera.zoom(0.5);
    let after = camera.world_per_pixel();
    assert!(
        after < before * 0.95,
        "zooming an orthographic view showed the same amount of world: {before} then {after}"
    );
    assert_eq!(camera.eye(), eye, "zooming a drawing moved the eye");
    assert_eq!(camera.target(), target);
    assert!(
        camera
            .view_projection()
            .iter()
            .all(|value| value.is_finite())
    );
    assert_eq!(camera.projection_mode(), Projection::Orthographic);

    // Going back to perspective respects the zoom that was made here rather
    // than restoring the distance the eye happened to be at beforehand.
    let mut back = camera;
    assert!(back.set_projection(Projection::Perspective));
    let restored = back.world_per_pixel();
    assert!(
        (restored - after).abs() <= after * 1e-5,
        "the zoom made in the drawing was thrown away: {after} became {restored}"
    );
    assert!(
        back.distance() < a_framed_camera(200, 200).distance(),
        "the eye went back to where it was before the zoom"
    );
}

#[test]
fn panning_orbiting_and_resizing_work_in_both_projections() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = a_framed_camera(320, 200);
        assert!(camera.set_projection(projection) || projection == Projection::Perspective);

        // A pan moves the model by the pixels it was asked for, measured at
        // the target plane.
        let scale = camera.world_per_pixel();
        let before = camera.target();
        camera.pan(10.0, 0.0);
        let moved = sub_points(camera.target(), before);
        let travelled = moved[0].hypot(moved[1]).hypot(moved[2]);
        assert!(
            (travelled - 10.0 * scale).abs() <= scale,
            "{projection:?}: a ten pixel pan moved {travelled} of world, not {}",
            10.0 * scale
        );
        assert_eq!(camera.projection_mode(), projection);

        // An orbit keeps the scale and the projection.
        let scale = camera.world_per_pixel();
        camera.orbit(0.4, 0.2);
        assert!(
            (camera.world_per_pixel() - scale).abs() <= scale * 1e-4,
            "{projection:?}: orbiting changed how big the model is drawn"
        );
        assert_eq!(camera.projection_mode(), projection);

        // Both shapes of viewport, and the aspect that goes with them.
        for (width, height) in [(320u32, 200u32), (200, 320)] {
            camera.resize(width, height);
            let expected = width as f32 / height as f32;
            assert!(
                (camera.aspect() - expected).abs() < 1e-6,
                "{projection:?}: a {width}x{height} viewport has aspect {}",
                camera.aspect()
            );
            assert!(
                camera
                    .view_projection()
                    .iter()
                    .all(|value| value.is_finite())
            );
            assert_eq!(camera.projection_mode(), projection);
        }
    }
}

#[test]
fn every_standard_view_and_every_framing_keeps_the_projection() {
    let bounds = ([-30.0, -10.0, -5.0], [10.0, 20.0, 25.0]);
    for (width, height) in [(320u32, 200u32), (200, 320)] {
        let mut camera = Camera::new();
        camera.resize(width, height);
        camera.frame(bounds).expect("frames");
        assert!(camera.set_projection(Projection::Orthographic));

        for view in [
            StandardView::Front,
            StandardView::Back,
            StandardView::Left,
            StandardView::Right,
            StandardView::Top,
            StandardView::Bottom,
            StandardView::Isometric,
        ] {
            camera.look_from(view);
            assert_eq!(
                camera.projection_mode(),
                Projection::Orthographic,
                "{view:?} put the model back into perspective"
            );
        }

        // Framing keeps the projection and the direction, and every corner of
        // the box lands inside the clip volume.
        let direction_before = sub_points(camera.eye(), camera.target());
        camera.frame(bounds).expect("frames");
        assert_eq!(camera.projection_mode(), Projection::Orthographic);
        let direction_after = sub_points(camera.eye(), camera.target());
        for axis in 0..3 {
            let before = direction_before[axis]
                / direction_before[0]
                    .hypot(direction_before[1])
                    .hypot(direction_before[2]);
            let after = direction_after[axis]
                / direction_after[0]
                    .hypot(direction_after[1])
                    .hypot(direction_after[2]);
            assert!((before - after).abs() < 1e-4, "framing turned the model");
        }

        let (min, max) = bounds;
        for corner in 0..8 {
            let point = [
                if corner & 1 == 0 { min[0] } else { max[0] },
                if corner & 2 == 0 { min[1] } else { max[1] },
                if corner & 4 == 0 { min[2] } else { max[2] },
            ];
            assert!(
                inside_clip_volume(&camera.view_projection(), point),
                "corner {corner} fell outside a {width}x{height} view at {:?}",
                projected(&camera.view_projection(), point)
            );
        }
    }
}

#[test]
fn a_box_framed_as_a_drawing_stays_framed_when_seen_in_perspective() {
    let bounds = ([-10.0, -10.0, -10.0], [10.0, 10.0, 10.0]);
    let mut camera = Camera::new();
    camera.resize(200, 200);
    camera.look_from(StandardView::Isometric);
    assert!(camera.set_projection(Projection::Orthographic));
    camera.frame(bounds).expect("frames as a drawing");
    assert!(camera.set_projection(Projection::Perspective));

    let (min, max) = bounds;
    for corner in 0..8 {
        let point = [
            if corner & 1 == 0 { min[0] } else { max[0] },
            if corner & 2 == 0 { min[1] } else { max[1] },
            if corner & 4 == 0 { min[2] } else { max[2] },
        ];
        assert!(
            inside_clip_volume(&camera.view_projection(), point),
            "corner {corner} left the view when the framed drawing became perspective: {:?}",
            projected(&camera.view_projection(), point)
        );
    }
}

#[test]
fn a_viewport_of_no_size_or_an_extreme_one_still_has_a_finite_drawing() {
    for (width, height) in [(0u32, 0u32), (1, 4096), (4096, 1)] {
        let mut camera = Camera::new();
        camera.resize(width, height);
        camera
            .frame(([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]))
            .expect("frames");
        assert!(camera.set_projection(Projection::Orthographic));
        assert!(
            camera
                .view_projection()
                .iter()
                .all(|value| value.is_finite()),
            "a {width}x{height} orthographic view has a matrix that is not a number"
        );
        camera.zoom(1.0);
        camera.pan(3.0, -2.0);
        camera.orbit(0.3, 0.1);
        assert!(
            camera
                .view_projection()
                .iter()
                .all(|value| value.is_finite())
        );
    }
}

/// One point less another.
fn sub_points(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

/// Where a world point lands, in pixels from the centre of the viewport,
/// positive right and positive up. The same convention the camera takes.
fn pixel_offset(camera: &Camera, point: [f32; 3]) -> (f32, f32) {
    let ndc = projected(&camera.view_projection(), point);
    (
        ndc[0] * camera.width() as f32 * 0.5,
        ndc[1] * camera.height() as f32 * 0.5,
    )
}

/// A point on the plane through the target, square to the viewing direction.
///
/// Built from two vectors perpendicular to the direction rather than from the
/// camera's own screen axes, so a gate about anchoring cannot be satisfied by
/// the anchoring arithmetic agreeing with itself.
fn on_target_plane(camera: &Camera, first: f32, second: f32) -> [f32; 3] {
    let away = offset(camera);
    let scale = length(away);
    assert!(
        scale > 0.0,
        "a camera with no direction has no target plane"
    );
    let away = [away[0] / scale, away[1] / scale, away[2] / scale];
    let mut across = cross_product(away, [0.0, 0.0, 1.0]);
    if length(across) < 1e-3 {
        across = cross_product(away, [0.0, 1.0, 0.0]);
    }
    let scale = length(across);
    let across = [across[0] / scale, across[1] / scale, across[2] / scale];
    let other = cross_product(away, across);
    let target = camera.target();
    [
        target[0] + across[0] * first + other[0] * second,
        target[1] + across[1] * first + other[1] * second,
        target[2] + across[2] * first + other[2] * second,
    ]
}

fn cross_product(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

/// Zooms about the pixel a point is under, and says where it ended up.
fn zoom_holding(camera: &mut Camera, amount: f32, point: [f32; 3]) -> ((f32, f32), (f32, f32)) {
    let before = pixel_offset(camera, point);
    camera.zoom_at(amount, before.0, before.1);
    (before, pixel_offset(camera, point))
}

#[test]
fn a_wheel_keeps_the_place_it_was_pointed_at_under_the_pointer() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        for amount in [0.36f32, -0.36] {
            let mut camera = framed();
            assert!(
                projection == Projection::Perspective || camera.set_projection(projection),
                "the camera refused a projection to zoom in"
            );
            // Well away from the centre, and not on either axis of the screen.
            let anchor = on_target_plane(&camera, 3.1, -2.4);
            let scale = camera.world_per_pixel();

            let (before, after) = zoom_holding(&mut camera, amount, anchor);

            assert!(
                before.0.abs() > 50.0 && before.1.abs() > 50.0,
                "{projection:?}: the anchor was not off centre, it was at {before:?}"
            );
            assert!(
                (after.0 - before.0).abs() <= 0.1 && (after.1 - before.1).abs() <= 0.1,
                "{projection:?} zoom of {amount}: what was under the pointer at {before:?} \
                 moved to {after:?}"
            );
            // A camera that did nothing would hold every point exactly.
            let now = camera.world_per_pixel();
            assert!(
                (now / scale - (-amount).exp()).abs() < 1e-3,
                "{projection:?} zoom of {amount}: the scale went from {scale} to {now}"
            );
        }
    }
}

#[test]
fn a_wheel_at_the_middle_of_the_viewport_is_the_zoom_that_has_no_pointer() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut anchored = framed();
        assert!(
            projection == Projection::Perspective || anchored.set_projection(projection),
            "the camera refused a projection to zoom in"
        );
        // Somewhere with no symmetry to hide a mistake in.
        anchored.orbit(0.6, -0.35);
        anchored.pan(37.0, -19.0);
        let mut centred = anchored;

        anchored.zoom_at(0.4, 0.0, 0.0);
        centred.zoom(0.4);

        assert_eq!(
            anchored, centred,
            "{projection:?}: zooming about the middle differed from zooming about nothing"
        );
    }
}

#[test]
fn a_wheel_holds_its_place_after_turning_the_model_and_sliding_it() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = framed();
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to zoom in"
        );
        camera.orbit(-0.9, 0.4);
        camera.pan(-64.0, 45.0);

        let anchor = on_target_plane(&camera, -2.2, 1.7);
        let (before, after) = zoom_holding(&mut camera, 0.5, anchor);

        assert!(
            (after.0 - before.0).abs() <= 0.1 && (after.1 - before.1).abs() <= 0.1,
            "{projection:?}: after turning and sliding, {before:?} moved to {after:?}"
        );
    }
}

#[test]
fn a_wheel_holds_its_place_in_a_tall_viewport_as_well_as_a_wide_one() {
    for (width, height) in [(1600u32, 500u32), (500, 1600)] {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut camera = Camera::new();
            camera.resize(width, height);
            camera
                .frame(([-3.0, -3.0, -3.0], [4.0, 5.0, 6.0]))
                .expect("frames");
            assert!(
                projection == Projection::Perspective || camera.set_projection(projection),
                "the camera refused a projection to zoom in"
            );

            let anchor = on_target_plane(&camera, 1.9, -1.3);
            let (before, after) = zoom_holding(&mut camera, -0.45, anchor);

            assert!(
                (after.0 - before.0).abs() <= 0.1 && (after.1 - before.1).abs() <= 0.1,
                "{width}x{height} {projection:?}: {before:?} moved to {after:?}"
            );
        }
    }
}

#[test]
fn zooming_in_and_out_again_about_the_same_pixel_comes_back() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = framed();
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to zoom in"
        );
        camera.orbit(0.3, 0.2);
        let before = camera;
        let anchor = on_target_plane(&camera, 2.6, 2.1);
        let (pixel, _) = zoom_holding(&mut camera, 0.24, anchor);

        camera.zoom_at(-0.24, pixel.0, pixel.1);

        // Two exponentials and two slides of an f32 target, over a model ten
        // units across seen from about thirty.
        for axis in 0..3 {
            assert!(
                (camera.eye()[axis] - before.eye()[axis]).abs() <= 1e-3,
                "{projection:?}: the eye came back to {:?} rather than {:?}",
                camera.eye(),
                before.eye()
            );
            assert!(
                (camera.target()[axis] - before.target()[axis]).abs() <= 1e-3,
                "{projection:?}: what is looked at came back to {:?} rather than {:?}",
                camera.target(),
                before.target()
            );
        }
        assert!(
            (camera.world_per_pixel() - before.world_per_pixel()).abs()
                <= before.world_per_pixel() * 1e-4,
            "{projection:?}: the scale came back to {} rather than {}",
            camera.world_per_pixel(),
            before.world_per_pixel()
        );
    }
}

#[test]
fn an_anchored_wheel_in_a_drawing_does_not_move_the_eye_along_its_own_direction() {
    let mut camera = framed();
    assert!(camera.set_projection(Projection::Orthographic));
    camera.orbit(0.5, 0.25);
    let before = camera;

    let anchor = on_target_plane(&camera, 3.4, -1.8);
    let (pixel, after) = zoom_holding(&mut camera, 0.5, anchor);

    assert!(
        (pixel.0 - after.0).abs() <= 0.1 && (pixel.1 - after.1).abs() <= 0.1,
        "the drawing did not hold {pixel:?}, it moved to {after:?}"
    );
    assert!(
        (camera.distance() - before.distance()).abs() <= before.distance() * 1e-5,
        "an orthographic zoom changed the distance from {} to {}",
        before.distance(),
        camera.distance()
    );
    let (was, now) = (offset(&before), offset(&camera));
    let (was_length, now_length) = (length(was), length(now));
    for axis in 0..3 {
        assert!(
            (was[axis] / was_length - now[axis] / now_length).abs() <= 1e-5,
            "an orthographic zoom turned the view from {was:?} to {now:?}"
        );
    }
    assert!(
        camera.eye() != before.eye(),
        "the eye did not follow what is being looked at"
    );
}

#[test]
fn a_wheel_with_nothing_usable_in_it_leaves_the_camera_exactly_as_it_was() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = framed();
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to zoom in"
        );
        let before = camera;

        for (amount, right, up) in [
            (f32::NAN, 10.0, 10.0),
            (f32::INFINITY, 10.0, 10.0),
            (0.3, f32::NAN, 10.0),
            (0.3, 10.0, f32::INFINITY),
        ] {
            camera.zoom_at(amount, right, up);
            assert_eq!(
                camera, before,
                "{projection:?}: zooming by {amount} about ({right}, {up}) changed the camera"
            );
        }
    }
}

#[test]
fn a_pointer_far_outside_the_window_leaves_a_camera_that_is_still_a_camera() {
    // A finite position is a finite request, however far off the window it is:
    // a trackpad that reports a wild coordinate should not be able to produce
    // a camera made of infinities.
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = framed();
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to zoom in"
        );

        for (right, up) in [(f32::MAX, f32::MAX), (-f32::MAX, 1e30), (1e20, -1e20)] {
            camera.zoom_at(0.3, right, up);
            assert!(
                camera.eye().iter().all(|value| value.is_finite())
                    && camera.target().iter().all(|value| value.is_finite())
                    && camera
                        .view_projection()
                        .iter()
                        .all(|value| value.is_finite()),
                "{projection:?}: a pointer at ({right}, {up}) left {camera:?}"
            );
        }
    }
}

#[test]
fn a_wheel_over_a_window_of_no_size_still_leaves_a_camera_that_can_be_drawn() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = framed();
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to zoom in"
        );
        camera.resize(0, 0);

        camera.zoom_at(0.6, 220.0, -140.0);

        assert!(
            camera
                .view_projection()
                .iter()
                .all(|value| value.is_finite()),
            "{projection:?}: a wheel over a window of no size produced {:?}",
            camera.view_projection()
        );
        assert!(
            camera.eye().iter().all(|value| value.is_finite())
                && camera.target().iter().all(|value| value.is_finite()),
            "{projection:?}: the camera left the world"
        );
    }
}

#[test]
fn a_wheel_cannot_be_wound_past_what_the_numbers_can_hold() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = framed();
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to zoom in"
        );

        for _ in 0..400 {
            camera.zoom_at(1.0, 399.0, -299.0);
        }
        assert!(
            camera.world_per_pixel() > 0.0 && camera.world_per_pixel().is_finite(),
            "{projection:?}: winding in left a scale of {}",
            camera.world_per_pixel()
        );
        for _ in 0..800 {
            camera.zoom_at(-1.0, -399.0, 299.0);
        }
        assert!(
            camera.world_per_pixel().is_finite() && camera.world_per_pixel() > 0.0,
            "{projection:?}: winding out left a scale of {}",
            camera.world_per_pixel()
        );
        assert!(
            camera
                .view_projection()
                .iter()
                .all(|value| value.is_finite()),
            "{projection:?}: the matrix stopped being a number"
        );
    }
}

#[test]
fn a_wheel_step_too_small_to_change_scale_changes_no_pose() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = framed();
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to test"
        );
        // Leave the perspective eye at coordinates that cannot all be
        // reconstructed bit-for-bit from a normalised direction and distance.
        // Before the no-op rule, the second iteration exposed a one-ULP move.
        for step in 1..=2 {
            camera.orbit(step as f32 * 0.0137, step as f32 * -0.0089);
            camera.pan(step as f32 * 0.17, step as f32 * -0.11);
            camera.zoom_at((step % 7) as f32 * 0.031 - 0.08, 173.0, -91.0);
        }

        for amount in [0.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE] {
            let before = camera;
            camera.zoom_at(amount, 173.0, -91.0);
            assert_eq!(
                camera, before,
                "{projection:?}: a step of {amount} changed the camera"
            );
        }
    }
}

/// A camera rolled by a quarter turn each way, and what that must do.
#[test]
fn turning_two_fingers_turns_the_world_the_way_they_went() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        for (radians, name) in [
            (std::f32::consts::FRAC_PI_2, "counterclockwise"),
            (-std::f32::consts::FRAC_PI_2, "clockwise"),
        ] {
            let mut camera = framed();
            assert!(
                projection == Projection::Perspective || camera.set_projection(projection),
                "the camera refused a projection to turn in"
            );
            // A point the camera draws to the right of what it is looking at.
            // Which way round the plane's own axes lie is not the camera's
            // business, so the one that measurably renders on the right is
            // the one the gate uses.
            let right = [4.0f32, -4.0]
                .into_iter()
                .map(|along| on_target_plane(&camera, along, 0.0))
                .find(|point| pixel_offset(&camera, *point).0 > 20.0)
                .expect("one side of the target plane is drawn on the right");
            let before = pixel_offset(&camera, right);
            assert!(
                before.0 > 20.0 && before.1.abs() < 1.0,
                "{projection:?}: the gate did not start on the right, it started at {before:?}"
            );

            camera.roll(radians);

            let after = pixel_offset(&camera, right);
            // A quarter turn counterclockwise puts the right above.
            let expected = if radians > 0.0 { before.0 } else { -before.0 };
            assert!(
                after.0.abs() < 1.0 && (after.1 - expected).abs() < 0.5,
                "{projection:?}: a {name} quarter turn moved {before:?} to {after:?}"
            );
        }
    }
}

#[test]
fn a_turn_changes_where_up_is_and_nothing_else_whatever_the_view() {
    for (width, height) in [(800u32, 600u32), (480, 1024)] {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut camera = Camera::new();
            camera.resize(width, height);
            camera
                .frame(([-4.0, -3.0, -6.0], [5.0, 7.0, 2.0]))
                .expect("frames");
            assert!(
                projection == Projection::Perspective || camera.set_projection(projection),
                "the camera refused a projection to turn in"
            );
            // Somewhere with no symmetry left to hide a mistake in.
            camera.orbit(0.8, -0.35);
            camera.pan(41.0, -27.0);
            let before = camera;
            let depths: Vec<f32> = [[0.0, 0.0, 0.0], [3.0, -2.0, 1.0], [-1.5, 4.0, -2.5]]
                .iter()
                .map(|point| projected(&before.view_projection(), *point)[2])
                .collect();

            camera.roll(0.7);

            assert_eq!(camera.eye(), before.eye(), "{projection:?}: the eye moved");
            assert_eq!(
                camera.target(),
                before.target(),
                "{projection:?}: what is looked at moved"
            );
            assert_eq!(
                camera.distance(),
                before.distance(),
                "{projection:?}: the distance changed"
            );
            assert_eq!(
                camera.projection_mode(),
                before.projection_mode(),
                "{projection:?}: the projection changed"
            );
            assert_eq!(
                camera.world_per_pixel(),
                before.world_per_pixel(),
                "{projection:?}: the apparent scale changed"
            );
            // Turning about the direction of view cannot move anything nearer
            // or further, so every depth is exactly the depth it was, which
            // is only true while the clipping range is untouched.
            for (point, was) in [[0.0, 0.0, 0.0], [3.0, -2.0, 1.0], [-1.5, 4.0, -2.5]]
                .iter()
                .zip(&depths)
            {
                let now = projected(&camera.view_projection(), *point)[2];
                assert_eq!(now, *was, "{projection:?}: {point:?} changed depth");
            }
            assert_ne!(camera.up(), before.up(), "{projection:?}: nothing turned");
        }
    }
}

#[test]
fn turning_back_returns_and_turning_all_the_way_round_returns() {
    let mut camera = framed();
    camera.orbit(0.55, 0.2);
    let before = camera;
    let point = on_target_plane(&camera, 3.3, -1.9);
    let started = pixel_offset(&camera, point);

    camera.roll(std::f32::consts::FRAC_PI_2);
    camera.roll(-std::f32::consts::FRAC_PI_2);
    for axis in 0..3 {
        assert!(
            (camera.up()[axis] - before.up()[axis]).abs() <= 1e-6,
            "a quarter turn and back left up at {:?} rather than {:?}",
            camera.up(),
            before.up()
        );
    }

    // And all the way round is the same view again, which two sines and two
    // cosines of an f32 angle reach to within a fraction of a pixel.
    let mut whole = before;
    for _ in 0..4 {
        whole.roll(std::f32::consts::FRAC_PI_2);
    }
    let ended = pixel_offset(&whole, point);
    assert!(
        (ended.0 - started.0).abs() <= 0.05 && (ended.1 - started.1).abs() <= 0.05,
        "a whole turn moved {started:?} to {ended:?}"
    );
}

#[test]
fn many_small_turns_leave_a_basis_that_is_still_a_basis() {
    let mut camera = framed();
    camera.orbit(0.3, 0.4);
    let before = camera;

    for _ in 0..2000 {
        camera.roll(0.01);
    }

    let up = camera.up();
    let length = up[0].hypot(up[1]).hypot(up[2]);
    assert!(
        (length - 1.0).abs() <= 1e-4,
        "up drifted to a length of {length}"
    );
    let away = offset(&camera);
    let scale = length_of(away);
    let along = (up[0] * away[0] + up[1] * away[1] + up[2] * away[2]) / scale;
    assert!(
        along.abs() <= 1e-4,
        "up leaned {along} towards the direction of view"
    );
    assert!(
        camera
            .view_projection()
            .iter()
            .all(|value| value.is_finite()),
        "the matrix stopped being a number"
    );
    assert_eq!(camera.eye(), before.eye(), "the eye drifted while turning");
    assert_eq!(
        camera.distance(),
        before.distance(),
        "the distance drifted while turning"
    );
}

#[test]
fn a_turn_that_cannot_move_the_basis_leaves_the_camera_exactly_as_it_was() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = framed();
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to turn in"
        );
        // A pose whose stored up is not already the orthogonalised one, which
        // is where an accidental rewrite of the basis would show.
        camera.orbit(0.0137, -0.0089);
        camera.pan(0.17, -0.11);
        let before = camera;

        for radians in [0.0f32, -0.0, f32::NAN, f32::INFINITY, -f32::INFINITY, 1e-30] {
            camera.roll(radians);
            assert_eq!(
                camera, before,
                "{projection:?}: a turn of {radians} changed the camera"
            );
        }
    }
}

#[test]
fn a_turn_on_a_window_of_no_size_leaves_a_camera_that_can_be_drawn() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = framed();
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to turn in"
        );
        camera.resize(0, 0);

        camera.roll(0.9);

        assert!(
            camera
                .view_projection()
                .iter()
                .all(|value| value.is_finite()),
            "{projection:?}: a turn on a window of no size produced {:?}",
            camera.view_projection()
        );
    }
}

#[test]
fn framing_keeps_a_turned_horizon_and_a_named_view_levels_it() {
    let mut camera = framed();
    camera.roll(0.6);
    let turned = camera.up();

    camera
        .frame(([-2.0, -2.0, -2.0], [8.0, 6.0, 4.0]))
        .expect("frames");
    for axis in 0..3 {
        assert!(
            (camera.up()[axis] - turned[axis]).abs() <= 1e-6,
            "framing levelled a turned view: {:?} rather than {turned:?}",
            camera.up()
        );
    }

    // The documented way back: a named view carries its own up, and the first
    // ordinary orbit restores the world's.
    camera.look_from(StandardView::Front);
    assert!(
        (camera.up()[2] - 1.0).abs() <= 1e-6,
        "a named view did not level the horizon: {:?}",
        camera.up()
    );
}

fn length_of(vector: [f32; 3]) -> f32 {
    vector[0].hypot(vector[1]).hypot(vector[2])
}

/// A square face made of two triangles sharing a diagonal.
fn square_face(shape: u64, first_index: u32) -> (Vec<f32>, Vec<f32>, Vec<u32>, MeshFaceRange) {
    let positions = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    let normals = vec![
        0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];
    let range = MeshFaceRange {
        face: SubShapeHandle::new(
            ShapeHandle::new(SessionId::new(), shape),
            SubShapeKind::Face,
            0u64,
        ),
        first_index,
        index_count: 6,
    };
    (positions, normals, indices, range)
}

/// The undirected segments a packed mesh draws, as a set.
fn boundary_set(mesh: &ferritecad_viewport::PackedMesh) -> std::collections::BTreeSet<(u32, u32)> {
    mesh.line_indices()
        .chunks_exact(2)
        .map(|pair| (pair[0].min(pair[1]), pair[0].max(pair[1])))
        .collect()
}

fn packed(mesh: &Mesh) -> ferritecad_viewport::PackedMesh {
    let mut builder = SnapshotBuilder::new();
    let index = builder.add_mesh(mesh).expect("packs");
    builder
        .place(index, None, &Transform::IDENTITY, [0.5, 0.5, 0.5])
        .expect("places");
    let snapshot = builder.build();
    snapshot.meshes()[0].clone()
}

#[test]
fn a_square_face_is_bounded_by_its_four_sides_and_not_by_its_diagonal() {
    let (positions, normals, indices, range) = square_face(1, 0);
    let mesh = Mesh {
        positions,
        normals,
        indices,
        faces: vec![range],
        edges: None,
    };

    let packed = packed(&mesh);

    assert_eq!(packed.line_count(), 4, "a square has four sides");
    assert_eq!(
        boundary_set(&packed),
        [(0, 1), (1, 2), (2, 3), (0, 3)].into_iter().collect(),
        "the boundary is not the four sides of the square"
    );
    // The diagonal is shared by both triangles, so it is inside the face.
    assert!(
        !boundary_set(&packed).contains(&(0, 2)),
        "the triangulation's own diagonal was drawn"
    );
}

#[test]
fn two_faces_lying_in_one_plane_each_keep_their_own_boundary() {
    // Two squares side by side in the same plane, tessellated separately.
    // Cancelling edges across the whole mesh rather than within each face
    // would erase where they meet, which is exactly what makes a coplanar
    // join impossible to see.
    let shape = ShapeHandle::new(SessionId::new(), 7);
    let face = |ordinal: u32, first_index: u32| MeshFaceRange {
        face: SubShapeHandle::new(shape, SubShapeKind::Face, u64::from(ordinal)),
        first_index,
        index_count: 6,
    };
    let mesh = Mesh {
        positions: vec![
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 2.0, 0.0,
            0.0, 2.0, 0.0, 1.0, 1.0, 0.0, 1.0,
        ],
        normals: [0.0, -1.0, 0.0].repeat(8),
        indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
        faces: vec![face(0, 0), face(1, 6)],
        edges: None,
    };

    let packed = packed(&mesh);

    assert_eq!(
        packed.line_count(),
        8,
        "two squares have eight sides between them"
    );
    let lines = boundary_set(&packed);
    // The seam, drawn twice because each face stops there: vertices 1 and 2
    // belong to the left square, 4 and 7 to the right, at the same place.
    assert!(
        lines.contains(&(1, 2)) && lines.contains(&(4, 7)),
        "the join between two coplanar faces was cancelled away: {lines:?}"
    );
}

#[test]
fn several_faces_and_separate_pieces_pack_the_same_lines_every_time() {
    let shape = ShapeHandle::new(SessionId::new(), 11);
    let face = |ordinal: u32, first_index: u32| MeshFaceRange {
        face: SubShapeHandle::new(shape, SubShapeKind::Face, u64::from(ordinal)),
        first_index,
        index_count: 6,
    };
    // Three squares, the third nowhere near the other two.
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for (piece, offset) in [0.0f32, 1.0, 40.0].into_iter().enumerate() {
        positions.extend_from_slice(&[
            offset,
            0.0,
            0.0,
            offset + 1.0,
            0.0,
            0.0,
            offset + 1.0,
            0.0,
            1.0,
            offset,
            0.0,
            1.0,
        ]);
        let base = piece as u32 * 4;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mesh = Mesh {
        positions,
        normals: [0.0, -1.0, 0.0].repeat(12),
        indices,
        faces: vec![face(0, 0), face(1, 6), face(2, 12)],
        edges: None,
    };

    let once = packed(&mesh);
    let twice = packed(&mesh);

    assert_eq!(once.line_count(), 12, "three squares have twelve sides");
    assert_eq!(
        once.line_indices(),
        twice.line_indices(),
        "packing the same mesh twice produced different lines"
    );
}

#[test]
fn the_order_and_the_winding_of_triangles_do_not_change_the_boundary() {
    let (positions, normals, _, range) = square_face(3, 0);
    let straightforward = Mesh {
        positions: positions.clone(),
        normals: normals.clone(),
        indices: vec![0, 1, 2, 0, 2, 3],
        faces: vec![range],
        edges: None,
    };
    // The same square: the other triangle first, and both wound the other way.
    let rearranged = Mesh {
        positions,
        normals,
        indices: vec![3, 2, 0, 2, 1, 0],
        faces: vec![range],
        edges: None,
    };

    assert_eq!(
        boundary_set(&packed(&straightforward)),
        boundary_set(&packed(&rearranged)),
        "the boundary depends on how the triangles happen to be written"
    );
}

#[test]
fn a_tessellation_that_is_not_a_surface_is_refused_rather_than_guessed_at() {
    let shape = ShapeHandle::new(SessionId::new(), 5);
    // Three triangles sharing one edge: no surface has that, and there is no
    // boundary to choose.
    let mesh = Mesh {
        positions: vec![
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.5, 1.0, -1.0, 0.5, -1.0, -1.0,
        ],
        normals: [0.0, -1.0, 0.0].repeat(5),
        indices: vec![0, 1, 2, 0, 1, 3, 0, 1, 4],
        faces: vec![MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
            first_index: 0,
            index_count: 9,
        }],
        edges: None,
    };

    let mut builder = SnapshotBuilder::new();
    let error = builder
        .add_mesh(&mesh)
        .expect_err("an edge in three triangles of one face is not a surface");
    assert!(
        error.to_string().contains("more than"),
        "the refusal does not say what is wrong: {error}"
    );
    assert_eq!(error.kind(), ErrorKind::Input);

    // And a malformed partition or an index outside the mesh is refused
    // before any of this, by the kernel's own validation.
    let (positions, normals, indices, mut range) = square_face(6, 0);
    range.index_count = 4;
    let ragged = Mesh {
        positions: positions.clone(),
        normals: normals.clone(),
        indices: indices.clone(),
        faces: vec![range],
        edges: None,
    };
    assert!(SnapshotBuilder::new().add_mesh(&ragged).is_err());

    let (_, _, _, range) = square_face(6, 0);
    let out_of_range = Mesh {
        positions,
        normals,
        indices: vec![0, 1, 9, 0, 2, 3],
        faces: vec![range],
        edges: None,
    };
    assert!(SnapshotBuilder::new().add_mesh(&out_of_range).is_err());
}

#[test]
fn a_picture_with_no_triangles_invents_no_lines() {
    let mesh = Mesh {
        positions: Vec::new(),
        normals: Vec::new(),
        indices: Vec::new(),
        faces: Vec::new(),
        edges: None,
    };

    let packed = packed(&mesh);

    assert_eq!(packed.line_count(), 0, "an empty mesh drew a line");
    assert!(packed.line_indices().is_empty());
}

#[test]
fn drawing_where_faces_stop_does_not_change_what_a_picture_is() {
    // The lines are a function of the indices and the partition, both of which
    // the identity already covers, so nothing new is hashed and no algorithm
    // version is bumped. What must hold is that identity still tells the same
    // pictures apart and still calls the same picture the same.
    let (positions, normals, indices, range) = square_face(9, 0);
    let square = Mesh {
        positions: positions.clone(),
        normals: normals.clone(),
        indices,
        faces: vec![range],
        edges: None,
    };
    // The same four vertices, cut the other way: different triangles, and so
    // a different picture, even though the boundary is the same square.
    let other_cut = Mesh {
        positions,
        normals,
        indices: vec![1, 2, 3, 1, 3, 0],
        faces: vec![range],
        edges: None,
    };

    // A snapshot's identity is what its picks carry, which is how every other
    // gate here compares two pictures.
    let identity = |mesh: &Mesh| {
        let mut builder = SnapshotBuilder::new();
        let index = builder.add_mesh(mesh).expect("packs");
        builder
            .place(index, None, &Transform::IDENTITY, [0.5, 0.5, 0.5])
            .expect("places");
        let snapshot = builder.build();
        snapshot.pick_of(0).expect("the square is drawn")
    };

    assert_eq!(
        identity(&square),
        identity(&square),
        "the same picture stopped being the same picture"
    );
    assert_ne!(
        identity(&square),
        identity(&other_cut),
        "two different tessellations became one picture"
    );
    assert_eq!(
        boundary_set(&packed(&square)),
        boundary_set(&packed(&other_cut)),
        "the gate no longer proves the boundary is the same for both"
    );
}

// ---------------------------------------------------------------------------
// Topological edges: what the kernel named, as this picture numbers it.
// ---------------------------------------------------------------------------

/// A square of two triangles whose four sides are divided between topological
/// edges as `division` says, in segments per edge.
///
/// The triangles are identical whatever the division is. That is the point:
/// two pictures can draw the same lines in the same places and still disagree
/// about which of them are one edge.
fn square_with_edges(division: &[u32]) -> Mesh {
    let shape = ShapeHandle::new(SessionId::new(), 1);
    let total: u32 = division.iter().sum();
    let mut first_segment = 0;
    let ranges = division
        .iter()
        .enumerate()
        .map(|(ordinal, count)| {
            let range = ferritecad_kernel::MeshEdgeRange {
                edge: SubShapeHandle::new(shape, SubShapeKind::Edge, ordinal as u64),
                first_segment,
                segment_count: *count,
            };
            first_segment += count;
            range
        })
        .collect();
    // Four sides of the square, in order, and as many of them as the division
    // accounts for.
    let sides = [0u32, 1, 1, 2, 2, 3, 3, 0];
    Mesh {
        positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0],
        normals: [0.0, 0.0, 1.0].repeat(4),
        indices: vec![0, 1, 2, 0, 2, 3],
        faces: vec![MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
            first_index: 0,
            index_count: 6,
        }],
        edges: Some(ferritecad_kernel::MeshEdges {
            segments: sides[..total as usize * 2].to_vec(),
            ranges,
        }),
    }
}

fn snapshot_of(meshes: &[Mesh]) -> RenderSnapshot {
    let mut builder = SnapshotBuilder::new();
    for mesh in meshes {
        let index = builder.add_mesh(mesh).expect("packs");
        builder
            .place(index, None, &Transform::IDENTITY, [0.5, 0.5, 0.5])
            .expect("places");
    }
    builder.build()
}

#[test]
fn an_edge_resolves_to_the_definition_and_the_segments_it_draws() {
    // Four sides, one segment each.
    let snapshot = snapshot_of(&[square_with_edges(&[1, 1, 1, 1])]);

    assert_eq!(snapshot.edge_count(), 4);
    assert_eq!(snapshot.meshes()[0].edge_count(), Some(4));

    for ordinal in 0..4 {
        let edge = snapshot
            .edge_of(0, ordinal)
            .expect("this picture numbers this edge");
        assert_eq!(snapshot.definition_of_edge(edge), Some(0));
        assert_eq!(
            snapshot.segments_of_edge(edge),
            Some(&[ordinal as u32, (ordinal as u32 + 1) % 4][..]),
            "each side draws the segment between its own two corners"
        );
    }
    assert!(
        snapshot.edge_of(0, 4).is_none(),
        "a fifth edge of a square is not one of its edges"
    );
}

#[test]
fn nothing_and_identities_from_other_pictures_name_no_edge() {
    let snapshot = snapshot_of(&[square_with_edges(&[1, 1, 1, 1])]);
    // The same geometry built again: a different picture, however alike.
    let replaced = snapshot_of(&[square_with_edges(&[1, 1, 1, 1])]);
    let another = snapshot_of(&[square_with_edges(&[2, 2])]);

    assert_eq!(
        snapshot.definition_of_edge(ferritecad_viewport::EdgePickId::NOTHING),
        None
    );
    assert_eq!(
        snapshot.segments_of_edge(ferritecad_viewport::EdgePickId::NOTHING),
        None
    );

    let edge = snapshot.edge_of(0, 2).expect("numbers this edge");
    // A picture built again from the same geometry is the same picture, and
    // says so rather than pretending otherwise.
    assert_eq!(replaced.definition_of_edge(edge), Some(0));
    assert_eq!(
        another.definition_of_edge(edge),
        None,
        "an edge of a differently divided picture names nothing here"
    );
    assert_eq!(another.segments_of_edge(edge), None);

    // The case that a range check alone would let through: a picture with more
    // edges than this one, in which the same raw number is perfectly in range
    // and belongs to a different definition entirely.
    let larger = snapshot_of(&[
        square_with_edges(&[1, 1, 1, 1]),
        square_with_edges(&[1, 1, 1, 1]),
    ]);
    let far = larger.edge_of(1, 3).expect("numbers this edge");
    assert_eq!(larger.definition_of_edge(far), Some(1));
    assert!(
        far.to_raw() as usize > snapshot.edge_count(),
        "the raw value must be out of this picture's range for the next line \
         to be about identity rather than range"
    );
    let near = larger.edge_of(0, 2).expect("numbers this edge");
    assert_eq!(near.to_raw(), edge.to_raw(), "the same raw number");
    assert_eq!(
        snapshot.definition_of_edge(near),
        None,
        "an in-range number from another picture is still not this picture's edge"
    );
    assert_eq!(snapshot.segments_of_edge(near), None);
    assert_eq!(snapshot.definition_of_edge(far), None);

    // And a raw number outside this picture's range reads as nothing.
    assert_eq!(
        ferritecad_viewport::EdgePickId::from_raw(99, &snapshot),
        ferritecad_viewport::EdgePickId::NOTHING
    );
    assert_eq!(
        ferritecad_viewport::EdgePickId::from_raw(0, &snapshot),
        ferritecad_viewport::EdgePickId::NOTHING
    );
}

#[test]
fn how_the_segments_divide_between_edges_is_part_of_what_a_picture_is() {
    // The same four vertices, the same two triangles, the same eight segment
    // indices in the same order. Only the division into topological edges
    // differs: four sides, or two edges of two segments each.
    let four = snapshot_of(&[square_with_edges(&[1, 1, 1, 1])]);
    let two = snapshot_of(&[square_with_edges(&[2, 2])]);

    assert_eq!(
        four.meshes()[0].vertices(),
        two.meshes()[0].vertices(),
        "the two pictures are drawn from the same vertices"
    );
    assert_eq!(
        four.meshes()[0].indices(),
        two.meshes()[0].indices(),
        "and the same triangles"
    );
    assert_eq!(
        four.meshes()[0].line_indices(),
        two.meshes()[0].line_indices(),
        "and they draw the same boundary"
    );

    // But the second edge of one is not the second edge of the other, and
    // neither picture may accept the other's identity for it.
    let from_four = four.edge_of(0, 1).expect("numbers this edge");
    let from_two = two.edge_of(0, 1).expect("numbers this edge");
    assert_eq!(
        two.definition_of_edge(from_four),
        None,
        "an edge identity of a differently divided picture resolves to nothing"
    );
    assert_eq!(four.definition_of_edge(from_two), None);
    assert_ne!(
        four.segments_of_edge(from_four),
        two.segments_of_edge(from_two),
        "the two pictures disagree about what that edge draws"
    );
}

#[test]
fn an_unknown_association_is_not_a_shape_without_edges() {
    let unknown = snapshot_of(&[triangle()]);
    let none_to_draw = snapshot_of(&[Mesh {
        edges: Some(ferritecad_kernel::MeshEdges::default()),
        ..triangle()
    }]);

    // Neither picture can identify an edge, and the reasons differ.
    assert_eq!(unknown.edge_count(), 0);
    assert_eq!(none_to_draw.edge_count(), 0);
    assert_eq!(unknown.edge_of(0, 0), None);
    assert_eq!(none_to_draw.edge_of(0, 0), None);

    assert_eq!(
        unknown.meshes()[0].edge_count(),
        None,
        "nothing is known about this definition's edges"
    );
    assert_eq!(
        none_to_draw.meshes()[0].edge_count(),
        Some(0),
        "this definition is known to have no edge to draw"
    );

    // And the two are different pictures, so an identity of one is not an
    // identity of the other.
    assert_ne!(
        unknown.meshes()[0],
        none_to_draw.meshes()[0],
        "the two answers are not the same packed mesh"
    );
}

#[test]
fn edges_are_numbered_per_definition_and_not_per_placement() {
    let mesh = square_with_edges(&[1, 1, 1, 1]);
    let mut builder = SnapshotBuilder::new();
    let definition = builder.add_mesh(&mesh).expect("packs");
    for x in [0.0, 10.0, 20.0] {
        builder
            .place(definition, None, &moved(x, 0.0, 0.0), [0.5, 0.5, 0.5])
            .expect("places");
    }
    let snapshot = builder.build();

    assert_eq!(snapshot.draws().len(), 3, "three placements");
    assert_eq!(
        snapshot.edge_count(),
        4,
        "and four edges, not twelve: an edge belongs to the definition"
    );
    for ordinal in 0..4 {
        assert_eq!(
            snapshot.definition_of_edge(snapshot.edge_of(0, ordinal).expect("numbered")),
            Some(0)
        );
    }
}

#[test]
fn two_definitions_do_not_share_edge_identities() {
    let snapshot = snapshot_of(&[square_with_edges(&[1, 1, 1, 1]), square_with_edges(&[2, 2])]);
    assert_eq!(snapshot.edge_count(), 6);

    let mut seen = Vec::new();
    for (definition, count) in [(0usize, 4usize), (1, 2)] {
        for ordinal in 0..count {
            let edge = snapshot.edge_of(definition, ordinal).expect("numbered");
            assert_eq!(snapshot.definition_of_edge(edge), Some(definition));
            assert!(!seen.contains(&edge), "an identity was handed out twice");
            seen.push(edge);
        }
    }
    assert_eq!(seen.len(), 6);
}

#[test]
fn a_definition_with_no_association_does_not_shift_another_ones_edges() {
    // The unknown definition is packed first, so if it were numbered as though
    // it had edges, every identity after it would be off by that many.
    let snapshot = snapshot_of(&[triangle(), square_with_edges(&[1, 1, 1, 1])]);
    assert_eq!(snapshot.edge_count(), 4);
    for ordinal in 0..4 {
        let edge = snapshot.edge_of(1, ordinal).expect("numbered");
        assert_eq!(snapshot.definition_of_edge(edge), Some(1));
        assert_eq!(
            snapshot.segments_of_edge(edge),
            Some(&[ordinal as u32, (ordinal as u32 + 1) % 4][..])
        );
    }
    assert_eq!(snapshot.edge_of(0, 0), None);
}

#[test]
fn nothing_a_session_issued_survives_into_the_picture() {
    let shape = ShapeHandle::new(SessionId::new(), 1);
    let mesh = square_with_edges(&[1, 1, 1, 1]);
    let handle = mesh.edges.as_ref().expect("has edges").ranges[0].edge;
    let snapshot = snapshot_of(&[mesh]);

    // What the kernel called these things is not written down anywhere in the
    // picture, including in what a person debugging it would read.
    let written = format!("{snapshot:?}");
    for leaked in [
        handle.to_string(),
        handle.shape().to_string(),
        handle.shape().session().to_string(),
        shape.session().to_string(),
    ] {
        assert!(
            !written.contains(&leaked),
            "the picture still carries {leaked}"
        );
    }
    for word in ["session#", "shape#", "edge#", "face#"] {
        assert!(
            !written.contains(word),
            "the picture still carries a {word} name"
        );
    }
}

#[test]
fn a_refused_mesh_consumes_no_edge_identities() {
    // A mesh whose faces share a vertex is refused while packing, after the
    // point at which its edges would have been counted.
    let shape = ShapeHandle::new(SessionId::new(), 1);
    let mut shared = square_with_edges(&[1, 1, 1, 1]);
    shared.faces = vec![
        MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
            first_index: 0,
            index_count: 3,
        },
        MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, 1),
            first_index: 3,
            index_count: 3,
        },
    ];

    let mut builder = SnapshotBuilder::new();
    builder
        .add_mesh(&shared)
        .expect_err("two faces of these triangles share a vertex");
    let definition = builder
        .add_mesh(&square_with_edges(&[1, 1, 1, 1]))
        .expect("packs afterwards");
    builder
        .place(definition, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    let snapshot = builder.build();

    assert_eq!(snapshot.edge_count(), 4, "the refusal consumed edge ids");
    for ordinal in 0..4 {
        let edge = snapshot.edge_of(definition, ordinal).expect("numbered");
        assert_eq!(snapshot.definition_of_edge(edge), Some(definition));
        assert_eq!(
            snapshot.segments_of_edge(edge),
            Some(&[ordinal as u32, (ordinal as u32 + 1) % 4][..])
        );
    }
}
