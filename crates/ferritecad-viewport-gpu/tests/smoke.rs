// SPDX-License-Identifier: MIT
//! Drawing a snapshot on a real device, off screen.
//!
//! Small on purpose. What a snapshot means, what a pick may say and how
//! placements compose are settled without a graphics stack in
//! `ferritecad-viewport`; what is left for a device to answer is whether the
//! thing actually draws, whether the pick target really comes back carrying the
//! identities that were put in it, and whether a frame can be separated from
//! the snapshot it belongs to. Only the last of those needs care, and it is a
//! type question that a GPU merely confirms.
//!
//! Every test skips itself when no adapter is available. A machine without a
//! GPU is an ordinary machine, and a suite that failed on one would be a suite
//! people learn to ignore.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::sync::Arc;

use ferritecad_kernel::{
    Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeHandle, SubShapeKind,
};
use ferritecad_types::{ErrorKind, Transform, Vec3};
use ferritecad_viewport::{Camera, PickId, RenderSnapshot, SnapshotBuilder};
use ferritecad_viewport_gpu::Renderer;

/// A renderer, or a reason to stop.
macro_rules! renderer_or_skip {
    () => {
        match Renderer::new() {
            Ok(renderer) => renderer,
            Err(reason) if reason.kind() == ErrorKind::Unsupported => {
                eprintln!("skipped: {reason}");
                return;
            }
            Err(reason) => panic!("a renderer failed after adapter discovery: {reason}"),
        }
    };
}

fn tilted_quad(baked_scale: bool) -> Mesh {
    let shape = ShapeHandle::new(SessionId::new(), 2);
    let scale = if baked_scale { 4.0 } else { 1.0 };
    let normal = if baked_scale {
        [0.242_535_62, -0.970_142_5, 0.0]
    } else {
        [
            std::f32::consts::FRAC_1_SQRT_2,
            -std::f32::consts::FRAC_1_SQRT_2,
            0.0,
        ]
    };
    let positions = [
        [-scale, -1.0, -1.0],
        [scale, 1.0, -1.0],
        [scale, 1.0, 1.0],
        [-scale, -1.0, 1.0],
    ];
    Mesh {
        positions: positions.into_iter().flatten().collect(),
        normals: [normal; 4].into_iter().flatten().collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
        faces: vec![MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
            first_index: 0,
            index_count: 6,
        }],
    }
}

/// A square in the XZ plane, facing -Y, two triangles.
///
/// Facing -Y because that is where [`Camera::frame`] puts the eye: a quad in
/// the XY plane would be edge-on and perfectly invisible, which looks exactly
/// like a renderer that draws nothing.
fn quad(half: f32) -> Mesh {
    let shape = ShapeHandle::new(SessionId::new(), 1);
    Mesh {
        positions: vec![
            -half, 0.0, -half, half, 0.0, -half, half, 0.0, half, -half, 0.0, half,
        ],
        normals: vec![
            0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
        ],
        indices: vec![0, 1, 2, 0, 2, 3],
        faces: vec![MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
            first_index: 0,
            index_count: 6,
        }],
    }
}

fn moved(x: f64, y: f64, z: f64) -> Transform {
    Transform::from_translation(Vec3::new(x, y, z).expect("finite")).expect("finite")
}

/// One quad at the origin, framed by a camera of the given size.
fn one_quad(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&quad(10.0)).expect("packs");
    builder
        .place(mesh, None, &Transform::IDENTITY, [0.0, 1.0, 0.0])
        .expect("places");
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("something is drawn"))
        .expect("frames");
    (snapshot, camera)
}

#[test]
fn a_snapshot_reaches_the_colour_target() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = one_quad(64, 64);

    let prepared = renderer.prepare(snapshot).expect("uploads");
    let frame = renderer.render(&prepared, &camera).expect("draws");
    assert_eq!(frame.width(), 64);
    assert_eq!(frame.height(), 64);
    assert_eq!(frame.colour().len(), 64 * 64 * 4);

    // The middle is the lit quad and a corner is the cleared background. Which
    // exact green it is depends on the shading, so what is asserted is that
    // something was drawn and that it is the channel the colour was given in.
    let centre = frame.colour_at(32, 32).expect("inside the frame");
    assert!(
        centre[1] > 0,
        "nothing was drawn in the middle of the frame: {centre:?}"
    );
    assert_eq!(centre[0], 0, "a green quad has no red in it: {centre:?}");

    // Opaque black: the clear colour, alpha and all.
    let corner = frame.colour_at(0, 0).expect("inside the frame");
    assert_eq!(corner, [0, 0, 0, 255], "the background was not cleared");

    assert_eq!(frame.colour_at(64, 0), None, "outside the frame is nothing");
}

#[test]
fn the_pick_target_comes_back_carrying_the_identities_that_went_in() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = one_quad(64, 64);
    let expected = snapshot.draws()[0].pick;

    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let frame = renderer.render(&prepared, &camera).expect("draws");

    let hit = frame.pick_at(32, 32);
    assert_eq!(
        hit, expected,
        "the middle of the quad picked something else"
    );
    assert_eq!(frame.snapshot().definition(hit), Some(0));

    // Nothing was drawn in the corner, and nothing is what it must read as
    // rather than definition zero.
    assert_eq!(frame.pick_at(0, 0), PickId::NOTHING);
    assert_eq!(frame.pick_at(999, 999), PickId::NOTHING);
}

#[test]
fn every_placement_is_drawn_and_they_all_pick_their_definition() {
    let mut renderer = renderer_or_skip!();

    // One definition placed twice, side by side, so both are on screen at once.
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&quad(4.0)).expect("packs");
    builder
        .place(mesh, None, &moved(-10.0, 0.0, 0.0), [1.0, 0.0, 0.0])
        .expect("places");
    builder
        .place(mesh, None, &moved(10.0, 0.0, 0.0), [1.0, 0.0, 0.0])
        .expect("places");
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(128, 64);
    camera
        .frame(snapshot.bounds().expect("something is drawn"))
        .expect("frames");

    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let frame = renderer.render(&prepared, &camera).expect("draws");

    // Scanned rather than sampled at guessed coordinates: where the framing
    // puts each quad is the camera's business, and a test that hardcoded it
    // would fail for a reason that has nothing to do with what it checks.
    let mut painted: Vec<(u32, PickId)> = Vec::new();
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let pick = frame.pick_at(x, y);
            if pick != PickId::NOTHING {
                painted.push((x, pick));
            }
        }
    }
    assert!(!painted.is_empty(), "nothing was drawn at all");

    let middle = frame.width() / 2;
    assert!(
        painted.iter().any(|(x, _)| *x < middle),
        "the left placement was not drawn"
    );
    assert!(
        painted.iter().any(|(x, _)| *x >= middle),
        "the right placement was not drawn"
    );

    // And every painted pixel says the same thing, because a pick names the
    // definition. Two placements of one part are indistinguishable here by
    // construction, which is what stops a click becoming a reference to an
    // occurrence.
    let first = painted[0].1;
    assert!(
        painted.iter().all(|(_, pick)| *pick == first),
        "two placements of one definition picked differently"
    );
    assert_eq!(frame.snapshot().definition(first), Some(0));
}

#[test]
fn a_frame_cannot_be_read_against_a_different_snapshot() {
    let mut renderer = renderer_or_skip!();
    let (first, camera) = one_quad(32, 32);
    let prepared = renderer.prepare(Arc::clone(&first)).expect("uploads");
    let frame = renderer.render(&prepared, &camera).expect("draws");

    // A second snapshot describing something else. Its definition zero is a
    // different part, and the raw number in the frame's pick buffer would name
    // it just as happily.
    let mut builder = SnapshotBuilder::new();
    let other_mesh = builder.add_mesh(&quad(1.0)).expect("packs");
    builder
        .place(other_mesh, None, &Transform::IDENTITY, [1.0, 1.0, 1.0])
        .expect("places");
    let second = builder.build();

    let hit = frame.pick_at(16, 16);
    assert_ne!(hit, PickId::NOTHING);

    // The frame answers against the snapshot it was drawn from, and there is no
    // way to ask it about another one: `snapshot()` returns that snapshot, and
    // the pick resolved from it belongs to it.
    assert!(std::ptr::eq(frame.snapshot(), Arc::as_ptr(&first)));
    assert_eq!(frame.snapshot().definition(hit), Some(0));
    assert_eq!(
        second.definition(hit),
        None,
        "a pick from one snapshot resolved inside another"
    );
}

#[test]
fn a_viewport_of_no_size_draws_nothing_and_says_so() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, _) = one_quad(64, 64);

    // Minimised windows and the moment before a first layout. There is no
    // target to draw into, and that is an answer rather than an error.
    let mut camera = Camera::new();
    camera.resize(0, 0);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let frame = renderer.render(&prepared, &camera).expect("draws nothing");
    assert_eq!(frame.width(), 0);
    assert!(frame.colour().is_empty());
    assert_eq!(frame.pick_at(0, 0), PickId::NOTHING);
    assert_eq!(
        frame.snapshot().draws().len(),
        1,
        "the snapshot is still there"
    );

    camera.resize(16, 0);
    assert!(
        renderer
            .render(&prepared, &camera)
            .expect("draws nothing")
            .colour()
            .is_empty()
    );
}

#[test]
fn a_viewport_larger_than_the_device_can_hold_is_refused_before_allocation() {
    let mut renderer = renderer_or_skip!();
    let snapshot = Arc::new(SnapshotBuilder::new().build());
    let mut camera = Camera::new();
    camera.resize(u32::MAX, 1);

    let prepared = renderer.prepare(snapshot).expect("uploads");
    let error = renderer
        .render(&prepared, &camera)
        .expect_err("an impossible target must be refused");
    assert_eq!(error.kind(), ErrorKind::Input);
}

#[test]
fn a_normal_and_its_baked_equivalent_receive_the_same_light() {
    let mut renderer = renderer_or_skip!();
    let build = |baked| {
        let mut builder = SnapshotBuilder::new();
        let mesh = builder.add_mesh(&tilted_quad(baked)).expect("packs");
        let transform = if baked {
            Transform::IDENTITY
        } else {
            Transform::from_rows([
                [4.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("finite")
        };
        builder
            .place(mesh, None, &transform, [0.8, 0.6, 0.2])
            .expect("places");
        Arc::new(builder.build())
    };
    let transformed = build(false);
    let baked = build(true);
    assert_eq!(transformed.bounds(), baked.bounds());

    let mut camera = Camera::new();
    camera.resize(64, 64);
    camera
        .frame(transformed.bounds().expect("geometry"))
        .expect("frames");
    let transformed_prepared = renderer.prepare(transformed).expect("uploads");
    let baked_prepared = renderer.prepare(baked).expect("uploads");
    let transformed = renderer
        .render(&transformed_prepared, &camera)
        .expect("draws");
    let baked = renderer.render(&baked_prepared, &camera).expect("draws");
    assert_eq!(
        transformed.colour(),
        baked.colour(),
        "non-uniform scaling changed the lighting of the same world-space surface"
    );
}

#[test]
fn an_empty_snapshot_draws_a_cleared_frame() {
    let mut renderer = renderer_or_skip!();
    let empty = Arc::new(SnapshotBuilder::new().build());
    let placed_empty = {
        let mesh = Mesh {
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            faces: Vec::new(),
        };
        let mut builder = SnapshotBuilder::new();
        let mesh = builder.add_mesh(&mesh).expect("packs an empty mesh");
        builder
            .place(mesh, None, &Transform::IDENTITY, [1.0, 0.0, 0.0])
            .expect("places an empty mesh");
        Arc::new(builder.build())
    };

    let mut camera = Camera::new();
    camera.resize(16, 16);

    // Neither no draws nor a placed definition with no triangles may make an
    // empty document (or an XDE assembly node) a rendering error.
    for snapshot in [empty, placed_empty] {
        let prepared = renderer.prepare(snapshot).expect("uploads");
        let frame = renderer.render(&prepared, &camera).expect("draws nothing");
        assert_eq!(frame.colour().len(), 16 * 16 * 4);
        assert!(
            frame
                .colour()
                .chunks_exact(4)
                .all(|pixel| pixel == [0, 0, 0, 255]),
            "an empty snapshot left something other than the clear colour"
        );
        assert_eq!(frame.pick_at(8, 8), PickId::NOTHING);
    }
}

#[test]
fn two_frames_of_one_snapshot_are_the_same_picture() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = one_quad(48, 48);

    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let first = renderer.render(&prepared, &camera).expect("draws");
    let second = renderer.render(&prepared, &camera).expect("draws");

    // Not a claim about GPUs in general: it is a claim that nothing in this
    // renderer varies between frames – no time, no frame counter, no iteration
    // over anything unordered.
    assert_eq!(first.colour(), second.colour());
    assert_eq!(first.pick_at(24, 24), second.pick_at(24, 24));
}

#[test]
fn geometry_is_uploaded_once_and_repeat_frames_only_move_the_camera() {
    let mut renderer = renderer_or_skip!();

    // Two definitions, so the count is not one by accident.
    let mut builder = SnapshotBuilder::new();
    let first = builder.add_mesh(&quad(4.0)).expect("packs");
    let second = builder.add_mesh(&quad(6.0)).expect("packs");
    builder
        .place(first, None, &moved(-8.0, 0.0, 0.0), [1.0, 0.0, 0.0])
        .expect("places");
    builder
        .place(second, None, &moved(8.0, 0.0, 0.0), [0.0, 0.0, 1.0])
        .expect("places");
    let snapshot = Arc::new(builder.build());

    let before = renderer.geometry_uploads();
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    assert_eq!(
        renderer.geometry_uploads() - before,
        2,
        "preparing should upload one buffer set per definition"
    );

    let mut camera = Camera::new();
    camera.resize(48, 48);
    let (minimum, maximum) = snapshot.bounds().expect("geometry");

    // Ten frames from ten camera positions. Nothing is uploaded again: what
    // changes between frames is a matrix, and that is the whole point of
    // preparing a snapshot rather than handing one over per frame.
    let after_upload = renderer.geometry_uploads();
    let mut matrices = Vec::new();
    let mut pictures = Vec::new();
    for step in 0..10 {
        let shift = step as f32 * 2.0;
        camera
            .frame((
                [minimum[0] + shift, minimum[1], minimum[2]],
                [maximum[0] + shift, maximum[1], maximum[2]],
            ))
            .expect("moves the camera without resizing the target");
        matrices.push(camera.view_projection());
        let frame = renderer.render(&prepared, &camera).expect("draws");
        assert_eq!((frame.width(), frame.height()), (48, 48));
        pictures.push(frame.colour().to_vec());
    }
    assert_eq!(
        renderer.geometry_uploads(),
        after_upload,
        "a repeat frame uploaded geometry again"
    );

    assert!(
        matrices.windows(2).all(|pair| pair[0] != pair[1]),
        "two requested camera positions produced one matrix"
    );

    // And the renderer really used those matrices. Every target has the same
    // dimensions, so a difference cannot come merely from a longer readback.
    // A renderer that ignored the camera would give ten identical pictures.
    assert!(
        pictures.windows(2).any(|pair| pair[0] != pair[1]),
        "ten different cameras produced one picture"
    );
}

#[test]
fn a_snapshot_prepared_by_another_renderer_is_refused() {
    let mut mine = renderer_or_skip!();
    // The first renderer proved that an adapter exists. Failure to open the
    // second is therefore a gate failure, not the no-adapter skip condition.
    let mut theirs = Renderer::new().expect("opens a second renderer on the available adapter");
    assert_ne!(mine.id(), theirs.id(), "two renderers share an identity");

    let (snapshot, camera) = one_quad(32, 32);
    let prepared = theirs.prepare(snapshot).expect("uploads");
    assert_eq!(prepared.renderer(), theirs.id());

    // Those buffers live on the other device. Drawing them here would be a
    // lifetime mistake surfacing as a driver error far from its cause, so it
    // is refused by name instead.
    let error = mine
        .render(&prepared, &camera)
        .expect_err("another renderer's buffers must not be drawn");
    assert_eq!(error.kind(), ErrorKind::Rendering, "{error}");
    assert!(
        error.to_string().contains("belong to the other device"),
        "{error}"
    );

    // The renderer that owns them still draws them.
    theirs.render(&prepared, &camera).expect("its own buffers");
}

#[test]
fn an_older_frame_keeps_resolving_against_the_snapshot_it_was_drawn_from() {
    let mut renderer = renderer_or_skip!();

    // One definition, so the raw pick value is 1 in both snapshots and only
    // the snapshot each frame carries can tell them apart.
    let (first, camera) = one_quad(32, 32);
    let first_prepared = renderer.prepare(Arc::clone(&first)).expect("uploads");
    let old = renderer.render(&first_prepared, &camera).expect("draws");
    let old_hit = old.pick_at(16, 16);
    assert_ne!(old_hit, PickId::NOTHING);
    assert_eq!(old_hit.to_raw(), 1);

    // A different model, prepared and drawn afterwards. The renderer has moved
    // on; the old frame has not.
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&quad(3.0)).expect("packs");
    builder
        .place(mesh, None, &Transform::IDENTITY, [0.0, 0.0, 1.0])
        .expect("places");
    let second = Arc::new(builder.build());
    let second_prepared = renderer.prepare(Arc::clone(&second)).expect("uploads");
    let new = renderer.render(&second_prepared, &camera).expect("draws");
    let new_hit = new.pick_at(16, 16);
    assert_ne!(new_hit, PickId::NOTHING);
    assert_eq!(new_hit.to_raw(), 1);

    // Both name definition zero of their own snapshot, and neither resolves
    // inside the other. A frame that answered against whatever the renderer
    // last drew would give the same number a different meaning.
    assert_eq!(old.snapshot().definition(old_hit), Some(0));
    assert_eq!(new.snapshot().definition(new_hit), Some(0));
    assert_eq!(first.definition(new_hit), None);
    assert_eq!(second.definition(old_hit), None);
    assert_ne!(old_hit, new_hit);

    // And the old frame's picture is still its own.
    assert_ne!(old.colour(), new.colour());
}
