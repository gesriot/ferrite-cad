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
use ferritecad_viewport::{
    Camera, PickId, RenderSnapshot, SnapshotBuilder, StandardView, VERTEX_FLOATS,
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
    (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
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
    let mut camera = framed();
    camera.resize(0, 0);
    let before = camera;

    // There is no pixel to measure against, so a drag has no length. Moving by
    // some default instead would send the model off to nowhere in particular.
    assert_eq!(camera.world_per_pixel(), 0.0);
    camera.pan(50.0, -50.0);
    assert_eq!(camera, before);
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
