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

/// Two quads side by side, each its own definition, and one of them placed
/// twice. Enough to ask both questions a selection has to answer: does it
/// reach every placement of what was chosen, and does it leave everything
/// else alone.
fn two_definitions(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let mut builder = SnapshotBuilder::new();
    let left = builder.add_mesh(&quad(10.0)).expect("packs");
    let right = builder.add_mesh(&quad(10.0)).expect("packs");

    let at = |x: f64| {
        Transform::from_translation(ferritecad_types::Vec3::new(x, 0.0, 0.0).expect("finite"))
            .expect("finite")
    };
    builder
        .place(left, None, &at(-14.0), [0.0, 1.0, 0.0])
        .expect("places");
    builder
        .place(left, None, &at(0.0), [0.0, 1.0, 0.0])
        .expect("places");
    builder
        .place(right, None, &at(14.0), [0.0, 1.0, 0.0])
        .expect("places");
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("something is drawn"))
        .expect("frames");
    (snapshot, camera)
}

/// A small model near the origin, framed, with room around it.
fn model_over_the_plane(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&quad(20.0)).expect("packs");
    builder
        .place(mesh, None, &Transform::IDENTITY, [0.1, 0.2, 0.9])
        .expect("places");
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("something is drawn"))
        .expect("frames");
    // Back off so the model occupies the middle and the plane fills the rest,
    // and look down at it: from the front view the plane is edge on, which is
    // what a floor looks like from the front and shows very little of it.
    camera.zoom(-1.0);
    camera.orbit(0.0, 0.7);
    (snapshot, camera)
}

/// Whether this pixel is one the grid drew: not the background, and one of
/// the greys or axis colours the grid shader can produce.
fn is_grid(pixel: [u8; 4]) -> bool {
    let [r, g, b, _] = pixel;
    let lit = u32::from(r) + u32::from(g) + u32::from(b) > 30;
    // The model in these tests is blue, and no grid colour is.
    lit && !(b > r + 40 && b > g + 40)
}

/// Where a row meets grid lines, as the x of each run's first pixel.
fn grid_lines_in_row(frame: &ferritecad_viewport_gpu::Frame, y: u32) -> Vec<u32> {
    let mut starts = Vec::new();
    let mut inside = false;
    for x in 0..frame.width() {
        let grid = frame
            .colour_at(x, y)
            .is_some_and(|pixel| is_grid(pixel) && frame.pick_at(x, y) == PickId::NOTHING);
        if grid && !inside {
            starts.push(x);
        }
        inside = grid;
    }
    starts
}

/// The widest gap between neighbouring grid lines anywhere in the frame.
///
/// Measured rather than counted, because a grid is a finite patch: how many
/// lines a row meets depends on how much of the patch is on screen, while how
/// far apart they are is what the ladder actually decides. Perspective
/// compresses the far ones, so the widest gap is the one nearest the eye and
/// the closest thing to the spacing that was chosen.
fn widest_gap(frame: &ferritecad_viewport_gpu::Frame) -> Option<u32> {
    (0..frame.height())
        .filter_map(|y| {
            let starts = grid_lines_in_row(frame, y);
            starts.windows(2).map(|pair| pair[1] - pair[0]).max()
        })
        .max()
}

/// How many pixels belonging to neither model changed between two backdrops.
///
/// Looking only where both frames say `NOTHING` removes model motion from the
/// answer. Comparing whole frames would let a screen-space grid pass merely
/// because pan or orbit moved the model drawn over it.
fn changed_common_background(
    before: &ferritecad_viewport_gpu::Frame,
    after: &ferritecad_viewport_gpu::Frame,
) -> (usize, usize) {
    assert_eq!(
        (before.width(), before.height()),
        (after.width(), after.height())
    );
    let mut comparable = 0;
    let mut changed = 0;
    for y in 0..before.height() {
        for x in 0..before.width() {
            if before.pick_at(x, y) == PickId::NOTHING && after.pick_at(x, y) == PickId::NOTHING {
                comparable += 1;
                if before.colour_at(x, y) != after.colour_at(x, y) {
                    changed += 1;
                }
            }
        }
    }
    (comparable, changed)
}

#[test]
fn a_model_is_drawn_over_a_grid_that_is_never_selectable() {
    let mut renderer = renderer_or_skip!();
    // Wide enough to contain more than ten minor intervals, otherwise the
    // axes can be visible while the next major line honestly lies outside the
    // frame and there are not two grey weights to compare.
    let (snapshot, camera) = model_over_the_plane(512, 512);

    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let frame = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

    let mut grid_pixels = 0;
    let mut model_pixels = 0;
    let mut x_axis_pixels = 0;
    let mut y_axis_pixels = 0;
    let mut grey_levels = std::collections::BTreeSet::new();
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let pixel = frame.colour_at(x, y).expect("inside the frame");
            let pick = frame.pick_at(x, y);
            if snapshot.definition(pick).is_some() {
                model_pixels += 1;
                // The model kept its own colour: the backdrop is behind it.
                assert!(
                    pixel[2] > pixel[0] && pixel[2] > pixel[1],
                    "a model pixel at {x},{y} is not the model's colour: {pixel:?}"
                );
            } else {
                assert_eq!(
                    pick,
                    PickId::NOTHING,
                    "a pixel at {x},{y} that is not the model names a definition"
                );
                if is_grid(pixel) {
                    grid_pixels += 1;
                    let [r, g, b, _] = pixel;
                    if r > g.saturating_add(30) && r > b.saturating_add(30) {
                        x_axis_pixels += 1;
                    } else if g > r.saturating_add(30) && g > b.saturating_add(30) {
                        y_axis_pixels += 1;
                    } else if r.abs_diff(g) <= 2 && g.abs_diff(b) <= 10 {
                        grey_levels.insert(r);
                    }
                }
            }
        }
    }

    assert!(model_pixels > 100, "the model drew {model_pixels} pixels");
    assert!(
        grid_pixels > 100,
        "the grid drew {grid_pixels} pixels, so there is no reference to see"
    );
    assert!(x_axis_pixels > 0, "the X axis is not distinguishable");
    assert!(y_axis_pixels > 0, "the Y axis is not distinguishable");
    assert!(
        grey_levels.len() >= 2,
        "minor and major lines are not distinguishable: {grey_levels:?}"
    );

    // Clicking a grid line is clicking the background, which is what clears a
    // selection: a backdrop is a thing to look at, not a thing to choose.
    let on_a_line = (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .find(|(x, y)| {
            frame.colour_at(*x, *y).is_some_and(is_grid)
                && snapshot.definition(frame.pick_at(*x, *y)).is_none()
        })
        .expect("some pixel is a grid line");
    assert_eq!(frame.pick_at(on_a_line.0, on_a_line.1), PickId::NOTHING);
}

#[test]
fn the_grid_belongs_to_the_world_and_not_to_the_screen() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = model_over_the_plane(128, 128);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");

    let still = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

    // Panning by part of a spacing moves the lines across the screen. A sheet
    // of graph paper drawn over the window would look identical.
    let mut panned = camera;
    panned.pan(17.0, 0.0);
    let after_pan = renderer
        .render(&prepared, &panned, PickId::NOTHING)
        .expect("draws");
    let (pan_background, pan_changed) = changed_common_background(&still, &after_pan);
    assert!(
        pan_background > 1_000,
        "only {pan_background} common background pixels could be compared"
    );
    assert!(
        pan_changed > 100,
        "panning changed only {pan_changed} background pixels; the grid stayed on the screen"
    );

    // Orbiting turns the plane away, so the lines converge instead of staying
    // parallel to the window's edges.
    let mut orbited = camera;
    orbited.orbit(0.6, 0.4);
    let after_orbit = renderer
        .render(&prepared, &orbited, PickId::NOTHING)
        .expect("draws");
    let (orbit_background, orbit_changed) = changed_common_background(&still, &after_orbit);
    assert!(
        orbit_background > 1_000,
        "only {orbit_background} common background pixels could be compared"
    );
    assert!(
        orbit_changed > 100,
        "orbiting changed only {orbit_changed} background pixels; the grid stayed on the screen"
    );

    // Zooming a long way changes which spacing is drawn, but not how dense the
    // lines look: that is the ladder doing its work. A fixed spacing would
    // multiply the count by the zoom.
    let near_gap = widest_gap(&still).expect("the grid is drawn at this zoom");
    let mut far = camera;
    far.zoom(-6.0);
    let after_zoom = renderer
        .render(&prepared, &far, PickId::NOTHING)
        .expect("draws");
    let far_gap = widest_gap(&after_zoom).expect("the grid is drawn at this zoom too");

    // A ladder step is at most two and a half times the one before it, so the
    // spacing on screen stays in the same range however far the camera moves.
    // A fixed world spacing would divide by the whole zoom factor instead.
    let ratio = f64::from(near_gap.max(far_gap)) / f64::from(near_gap.min(far_gap));
    assert!(
        ratio <= 3.0,
        "zooming changed the spacing on screen by {ratio} ({near_gap} then {far_gap} pixels)"
    );
}

#[test]
fn a_backdrop_costs_no_geometry_and_repeats_exactly() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = model_over_the_plane(96, 96);
    let prepared = renderer.prepare(snapshot).expect("uploads");
    let uploaded = renderer.geometry_uploads();

    let first = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");
    let second = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

    // The grid has no vertex buffer to upload and the model's are already
    // resident, so drawing a backdrop must not look like uploading geometry.
    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "drawing the grid uploaded geometry"
    );

    // And nothing in it depends on when it was drawn.
    assert_eq!(first.colour(), second.colour());
    let picks = |frame: &ferritecad_viewport_gpu::Frame| {
        let mut all = Vec::new();
        for y in 0..frame.height() {
            for x in 0..frame.width() {
                all.push(frame.pick_at(x, y));
            }
        }
        all
    };
    assert_eq!(picks(&first), picks(&second));
}

#[test]
fn a_part_below_the_plane_is_still_drawn_over_the_grid() {
    let mut renderer = renderer_or_skip!();

    // The same model, put under the world's floor.
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&quad(20.0)).expect("packs");
    builder
        .place(
            mesh,
            None,
            &Transform::from_translation(Vec3::new(0.0, 0.0, -40.0).expect("finite"))
                .expect("finite"),
            [0.1, 0.2, 0.9],
        )
        .expect("places");
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(128, 128);
    // Framed to take in the plane as well as the part, so that looking down
    // from above really does put the grid between the eye and the model
    // rather than off to one side of it.
    camera
        .frame(([-25.0, -25.0, -45.0], [25.0, 25.0, 5.0]))
        .expect("frames");
    camera.orbit(0.0, 1.1);

    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let frame = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

    // The grid is a backdrop, not a floor that hides things: a part below the
    // plane is drawn over the lines and can still be clicked.
    let model_pixels = (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .filter(|(x, y)| snapshot.definition(frame.pick_at(*x, *y)).is_some())
        .count();
    assert!(
        model_pixels > 100,
        "a part below the plane covered {model_pixels} pixels"
    );

    // And no holes in it. A grid that wrote depth would not make the part
    // vanish – lines are thin – but every line crossing it would punch a row
    // of grid-coloured pixels through a solid it is meant to be behind.
    let mut holes = 0;
    for y in 0..frame.height() {
        let inside: Vec<u32> = (0..frame.width())
            .filter(|x| snapshot.definition(frame.pick_at(*x, y)).is_some())
            .collect();
        let (Some(first), Some(last)) = (inside.first(), inside.last()) else {
            continue;
        };
        holes += (*first..=*last)
            .filter(|x| snapshot.definition(frame.pick_at(*x, y)).is_none())
            .count();
    }
    assert_eq!(
        holes, 0,
        "the grid punched {holes} pixels through a part that is behind it"
    );
}

#[test]
fn a_picture_with_nothing_in_it_gets_no_backdrop() {
    let mut renderer = renderer_or_skip!();
    let empty = Arc::new(SnapshotBuilder::new().build());
    let mut camera = Camera::new();
    camera.resize(64, 64);

    let prepared = renderer.prepare(Arc::clone(&empty)).expect("uploads");
    let frame = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

    // An empty document is empty. Drawing a floor under nothing would invent
    // content for a picture that has none, and would give a camera something
    // to look at that the model does not contain.
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            assert_eq!(
                frame.colour_at(x, y),
                Some([0, 0, 0, 255]),
                "an empty picture drew something at {x},{y}"
            );
            assert_eq!(frame.pick_at(x, y), PickId::NOTHING);
        }
    }
}

#[test]
fn choosing_a_definition_changes_every_placement_of_it_and_nothing_else() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_definitions(96, 96);

    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

    // Find a pixel of each definition by asking the frame what is under it.
    let mut of = [None, None];
    for y in 0..plain.height() {
        for x in 0..plain.width() {
            if let Some(definition) = snapshot.definition(plain.pick_at(x, y))
                && definition < 2
                && of[definition].is_none()
            {
                of[definition] = Some((x, y));
            }
        }
    }
    let first = of[0].expect("the first definition was drawn");
    let second = of[1].expect("the second definition was drawn");

    // Both placements of the first definition, found the same way.
    let placements: Vec<(u32, u32)> = (0..plain.height())
        .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
        .filter(|(x, y)| snapshot.definition(plain.pick_at(*x, *y)) == Some(0))
        .collect();
    assert!(
        placements.len() > 200,
        "the first definition covers {} pixels",
        placements.len()
    );

    let chosen = plain.pick_at(first.0, first.1);
    let lit = renderer.render(&prepared, &camera, chosen).expect("draws");

    // Every pixel of the chosen definition changed, wherever it was drawn:
    // the two placements share one identity, so choosing reaches both.
    for (x, y) in &placements {
        assert_ne!(
            lit.colour_at(*x, *y),
            plain.colour_at(*x, *y),
            "a placement of the chosen definition was left as it was at {x},{y}"
        );
    }

    // And nothing else moved, which is what makes it a selection rather than
    // a change of lighting.
    assert_eq!(
        lit.colour_at(second.0, second.1),
        plain.colour_at(second.0, second.1),
        "choosing one definition changed another"
    );
}

#[test]
fn a_selection_from_another_snapshot_selects_nothing() {
    let mut renderer = renderer_or_skip!();
    let (mine, camera) = two_definitions(64, 64);
    let (theirs, _) = one_quad(64, 64);

    let prepared = renderer.prepare(Arc::clone(&mine)).expect("uploads");
    let plain = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

    // A pick taken from a different picture. Its number is in range here, so
    // nothing but the snapshot it was issued against can tell that it means
    // another definition – which is why the renderer asks.
    let elsewhere = renderer.prepare(Arc::clone(&theirs)).expect("uploads");
    let other_camera = {
        let mut camera = Camera::new();
        camera.resize(64, 64);
        camera
            .frame(theirs.bounds().expect("something is drawn"))
            .expect("frames");
        camera
    };
    let other = renderer
        .render(&elsewhere, &other_camera, PickId::NOTHING)
        .expect("draws");
    let foreign = (0..other.height())
        .flat_map(|y| (0..other.width()).map(move |x| (x, y)))
        .map(|(x, y)| other.pick_at(x, y))
        .find(|pick| theirs.definition(*pick).is_some())
        .expect("the other picture drew something");

    let after = renderer.render(&prepared, &camera, foreign).expect("draws");
    assert_eq!(
        after.colour(),
        plain.colour(),
        "a pick from another picture selected something in this one"
    );
}

#[test]
fn a_snapshot_reaches_the_colour_target() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = one_quad(64, 64);

    let prepared = renderer.prepare(snapshot).expect("uploads");
    let frame = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");
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
    let frame = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

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
    let frame = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

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
    let frame = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

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
    let frame = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws nothing");
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
            .render(&prepared, &camera, PickId::NOTHING)
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
        .render(&prepared, &camera, PickId::NOTHING)
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
        .render(&transformed_prepared, &camera, PickId::NOTHING)
        .expect("draws");
    let baked = renderer
        .render(&baked_prepared, &camera, PickId::NOTHING)
        .expect("draws");
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
        let frame = renderer
            .render(&prepared, &camera, PickId::NOTHING)
            .expect("draws nothing");
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
    let first = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");
    let second = renderer
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("draws");

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
        let frame = renderer
            .render(&prepared, &camera, PickId::NOTHING)
            .expect("draws");
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
        .render(&prepared, &camera, PickId::NOTHING)
        .expect_err("another renderer's buffers must not be drawn");
    assert_eq!(error.kind(), ErrorKind::Rendering, "{error}");
    assert!(
        error.to_string().contains("belong to the other device"),
        "{error}"
    );

    // The renderer that owns them still draws them.
    theirs
        .render(&prepared, &camera, PickId::NOTHING)
        .expect("its own buffers");
}

#[test]
fn an_older_frame_keeps_resolving_against_the_snapshot_it_was_drawn_from() {
    let mut renderer = renderer_or_skip!();

    // One definition, so the raw pick value is 1 in both snapshots and only
    // the snapshot each frame carries can tell them apart.
    let (first, camera) = one_quad(32, 32);
    let first_prepared = renderer.prepare(Arc::clone(&first)).expect("uploads");
    let old = renderer
        .render(&first_prepared, &camera, PickId::NOTHING)
        .expect("draws");
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
    let new = renderer
        .render(&second_prepared, &camera, PickId::NOTHING)
        .expect("draws");
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
