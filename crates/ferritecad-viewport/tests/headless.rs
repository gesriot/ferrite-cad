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
use ferritecad_types::{ErrorKind, Transform, Vec3};
use ferritecad_viewport::{
    Camera, FacePickId, PickId, RenderSnapshot, SnapshotBuilder, StandardView, VERTEX_FLOATS,
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
