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
use ferritecad_types::{Transform, Vec3};
use ferritecad_viewport::{Camera, PickId, RenderSnapshot, SnapshotBuilder, VERTEX_FLOATS};

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

fn projected(matrix: &[f32; 16], point: [f32; 3]) -> [f32; 3] {
    let clip = [
        matrix[0] * point[0] + matrix[4] * point[1] + matrix[8] * point[2] + matrix[12],
        matrix[1] * point[0] + matrix[5] * point[1] + matrix[9] * point[2] + matrix[13],
        matrix[2] * point[0] + matrix[6] * point[1] + matrix[10] * point[2] + matrix[14],
        matrix[3] * point[0] + matrix[7] * point[1] + matrix[11] * point[2] + matrix[15],
    ];
    assert!(clip[3] > 0.0, "a point in front has clip.w {}", clip[3]);
    [clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]]
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
