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
use ferritecad_viewport::{
    Camera, FacePickId, Marked, PickId, Projection, RenderSnapshot, SnapshotBuilder, StandardView,
    Visibility,
};
use ferritecad_viewport_gpu::{Frame, Renderer};

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
    one_coloured_quad(width, height, [0.0, 1.0, 0.0])
}

fn one_coloured_quad(width: u32, height: u32, colour: [f64; 3]) -> (Arc<RenderSnapshot>, Camera) {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&quad(10.0)).expect("packs");
    builder
        .place(mesh, None, &Transform::IDENTITY, colour)
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

/// A plate of two square faces, side by side in the XZ plane, facing -Y.
///
/// Two faces rather than one quad split along its diagonal, so "this face" and
/// "the one next to it" are regions a test can point at.
fn two_faced_plate(half: f32) -> Mesh {
    let shape = ShapeHandle::new(SessionId::new(), 3);
    let square = |centre: f32| {
        [
            centre - half,
            0.0,
            -half,
            centre + half,
            0.0,
            -half,
            centre + half,
            0.0,
            half,
            centre - half,
            0.0,
            half,
        ]
    };
    let mut positions = Vec::new();
    positions.extend_from_slice(&square(-half * 1.2));
    positions.extend_from_slice(&square(half * 1.2));
    Mesh {
        positions,
        normals: [[0.0f32, -1.0, 0.0]; 8].into_iter().flatten().collect(),
        indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
        faces: (0..2)
            .map(|face| MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Face, face),
                first_index: face as u32 * 6,
                index_count: 6,
            })
            .collect(),
    }
}

/// A two-faced plate placed twice, and a plate of another definition beside it.
fn two_faced_scene(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let mut builder = SnapshotBuilder::new();
    let plate = builder.add_mesh(&two_faced_plate(6.0)).expect("packs");
    let other = builder.add_mesh(&quad(6.0)).expect("packs");

    let at = |x: f64| {
        Transform::from_translation(ferritecad_types::Vec3::new(x, 0.0, 0.0).expect("finite"))
            .expect("finite")
    };
    for x in [-40.0, 0.0] {
        builder
            .place(plate, None, &at(x), [0.0, 1.0, 0.0])
            .expect("places");
    }
    builder
        .place(other, None, &at(40.0), [0.0, 0.4, 1.0])
        .expect("places");
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("something is drawn"))
        .expect("frames");
    (snapshot, camera)
}

/// Every pixel of one face, and every pixel of one definition.
fn pixels_of(frame: &Frame, of: impl Fn(&Frame, u32, u32) -> bool) -> Vec<(u32, u32)> {
    (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .filter(|(x, y)| of(frame, *x, *y))
        .collect()
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

/// How far apart the grid lines crossing one row are, in pixels.
///
/// The gap that occurs most often rather than the widest one: the model
/// covers whole stretches of the backdrop, and the gap across a part would
/// otherwise be mistaken for a spacing the grid chose.
fn modal_line_gap(frame: &ferritecad_viewport_gpu::Frame, row: u32) -> Option<f32> {
    let mut columns: Vec<u32> = (0..frame.width())
        .filter(|x| {
            frame
                .colour_at(*x, row)
                .is_some_and(|colour| colour != [0, 0, 0, 255])
                && frame.pick_at(*x, row) == PickId::NOTHING
        })
        .collect();
    columns.dedup_by(|a, b| *a == *b + 1);
    if columns.len() < 3 {
        return None;
    }
    let gaps: Vec<u32> = columns.windows(2).map(|pair| pair[1] - pair[0]).collect();
    gaps.iter()
        .max_by_key(|gap| gaps.iter().filter(|other| other == gap).count())
        .map(|gap| *gap as f32)
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    // Panning by part of a spacing moves the lines across the screen. A sheet
    // of graph paper drawn over the window would look identical.
    let mut panned = camera;
    panned.pan(17.0, 0.0);
    let after_pan = renderer
        .render(
            &prepared,
            &panned,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &orbited,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &far,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let second = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
    let lit = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(chosen),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

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
fn pointing_at_a_definition_marks_every_placement_of_it_and_no_other() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_definitions(96, 96);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");

    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let places = |definition: usize| -> Vec<(u32, u32)> {
        (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .filter(|(x, y)| snapshot.definition(plain.pick_at(*x, *y)) == Some(definition))
            .collect()
    };
    let first = places(0);
    let second = places(1);
    assert!(first.len() > 200 && !second.is_empty());

    let hovered = plain.pick_at(first[0].0, first[0].1);
    let pointed = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Definition(hovered),
            &Visibility::default(),
        )
        .expect("draws");

    // Every placement of what is under the pointer, because a pick names a
    // definition: pointing at one bolt tells you where all of them are.
    for (x, y) in &first {
        assert_ne!(
            pointed.colour_at(*x, *y),
            plain.colour_at(*x, *y),
            "a placement of the pointed-at definition was left alone at {x},{y}"
        );
    }
    for (x, y) in &second {
        assert_eq!(
            pointed.colour_at(*x, *y),
            plain.colour_at(*x, *y),
            "pointing at one definition changed another at {x},{y}"
        );
    }

    // What a click would say is unchanged: a highlight is a colour, and the
    // identities behind it are the same identities.
    for (x, y) in first.iter().chain(second.iter()) {
        assert_eq!(pointed.pick_at(*x, *y), plain.pick_at(*x, *y));
    }
}

#[test]
fn a_choice_and_a_question_are_told_apart() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_definitions(96, 96);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");

    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let pixel_of = |definition: usize| {
        (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .find(|(x, y)| snapshot.definition(plain.pick_at(*x, *y)) == Some(definition))
            .expect("that definition is drawn")
    };
    let (ax, ay) = pixel_of(0);
    let (bx, by) = pixel_of(1);
    let a = plain.pick_at(ax, ay);
    let b = plain.pick_at(bx, by);

    let chosen_only = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(a),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let pointed_only = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Definition(a),
            &Visibility::default(),
        )
        .expect("draws");
    let both = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(a),
            Marked::Definition(b),
            &Visibility::default(),
        )
        .expect("draws");
    let same = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(a),
            Marked::Definition(a),
            &Visibility::default(),
        )
        .expect("draws");

    // Choosing and pointing are different states, and a person has to be able
    // to see which is which.
    assert_ne!(
        chosen_only.colour_at(ax, ay),
        pointed_only.colour_at(ax, ay),
        "what is chosen looks the same as what is merely under the pointer"
    );

    // Chosen A while pointing at B: both visible, and neither mistaken for
    // the other.
    assert_eq!(both.colour_at(ax, ay), chosen_only.colour_at(ax, ay));
    assert_ne!(both.colour_at(bx, by), plain.colour_at(bx, by));
    assert_ne!(both.colour_at(bx, by), chosen_only.colour_at(ax, ay));

    // Pointing at what is already chosen changes nothing: a decision outranks
    // a question about the same thing.
    assert_eq!(same.colour(), chosen_only.colour());
}

#[test]
fn light_and_dark_parts_can_both_be_chosen_and_pointed_at() {
    let mut renderer = renderer_or_skip!();

    // White was the missing edge: mixing every mark towards white changes no
    // pixel of a white part. Black holds the opposite half of the rule so a
    // fix that merely reversed the same defect cannot pass.
    for colour in [[1.0; 3], [0.0; 3]] {
        let (snapshot, camera) = one_coloured_quad(64, 64, colour);
        let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
        let plain = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Marked::Nothing,
                &Visibility::default(),
            )
            .expect("draws");
        let (x, y, pick) = (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .map(|(x, y)| (x, y, plain.pick_at(x, y)))
            .find(|(_, _, pick)| snapshot.definition(*pick).is_some())
            .expect("the quad is drawn");

        let chosen = renderer
            .render(
                &prepared,
                &camera,
                Marked::Definition(pick),
                Marked::Nothing,
                &Visibility::default(),
            )
            .expect("draws selection");
        let pointed = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Marked::Definition(pick),
                &Visibility::default(),
            )
            .expect("draws hover");

        assert_ne!(
            chosen.colour_at(x, y),
            plain.colour_at(x, y),
            "a {colour:?} part could not be seen as selected"
        );
        assert_ne!(
            pointed.colour_at(x, y),
            plain.colour_at(x, y),
            "a {colour:?} part could not be seen as hovered"
        );
        assert_ne!(
            chosen.colour_at(x, y),
            pointed.colour_at(x, y),
            "selection and hover of a {colour:?} part were indistinguishable"
        );
    }
}

#[test]
fn nothing_worth_pointing_at_is_marked() {
    let mut renderer = renderer_or_skip!();
    let (mine, camera) = two_definitions(96, 96);
    let (theirs, _) = one_quad(96, 96);
    let prepared = renderer.prepare(Arc::clone(&mine)).expect("uploads");

    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    // A pick from another picture names a definition of that picture. Its
    // number would fit here, which is exactly why the picture is asked.
    let elsewhere = renderer.prepare(Arc::clone(&theirs)).expect("uploads");
    let other = renderer
        .render(
            &elsewhere,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let foreign = (0..other.height())
        .flat_map(|y| (0..other.width()).map(move |x| (x, y)))
        .map(|(x, y)| other.pick_at(x, y))
        .find(|pick| theirs.definition(*pick).is_some())
        .expect("the other picture drew something");

    for hovered in [PickId::NOTHING, foreign] {
        let frame = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Marked::Definition(hovered),
                &Visibility::default(),
            )
            .expect("draws");
        assert_eq!(
            frame.colour(),
            plain.colour(),
            "something was marked for a pick this picture does not know"
        );
    }
}

#[test]
fn pointing_costs_no_geometry_and_leaves_the_backdrop_alone() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = model_over_the_plane(96, 96);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let uploaded = renderer.geometry_uploads();

    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let hovered = (0..plain.height())
        .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
        .map(|(x, y)| plain.pick_at(x, y))
        .find(|pick| snapshot.definition(*pick).is_some())
        .expect("the model is drawn");

    let pointed = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Definition(hovered),
            &Visibility::default(),
        )
        .expect("draws");
    let again = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Definition(hovered),
            &Visibility::default(),
        )
        .expect("draws");

    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "pointing at something uploaded geometry"
    );
    assert_eq!(
        pointed.colour(),
        again.colour(),
        "one hover drew two pictures"
    );

    // The backdrop is not a thing to point at: every grid pixel is exactly
    // what it was, and none of it names anything.
    for y in 0..plain.height() {
        for x in 0..plain.width() {
            if snapshot.definition(plain.pick_at(x, y)).is_none() {
                assert_eq!(
                    pointed.colour_at(x, y),
                    plain.colour_at(x, y),
                    "the backdrop at {x},{y} responded to the pointer"
                );
                assert_eq!(pointed.pick_at(x, y), PickId::NOTHING);
            }
        }
    }
}

#[test]
fn a_selection_from_another_snapshot_selects_nothing() {
    let mut renderer = renderer_or_skip!();
    let (mine, camera) = two_definitions(64, 64);
    let (theirs, _) = one_quad(64, 64);

    let prepared = renderer.prepare(Arc::clone(&mine)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &elsewhere,
            &other_camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let foreign = (0..other.height())
        .flat_map(|y| (0..other.width()).map(move |x| (x, y)))
        .map(|(x, y)| other.pick_at(x, y))
        .find(|pick| theirs.definition(*pick).is_some())
        .expect("the other picture drew something");

    let after = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(foreign),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Marked::Nothing,
                &Visibility::default()
            )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &transformed_prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let baked = renderer
        .render(
            &baked_prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Marked::Nothing,
                &Visibility::default(),
            )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let second = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Marked::Nothing,
                &Visibility::default(),
            )
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
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect_err("another renderer's buffers must not be drawn");
    assert_eq!(error.kind(), ErrorKind::Rendering, "{error}");
    assert!(
        error.to_string().contains("belong to the other device"),
        "{error}"
    );

    // The renderer that owns them still draws them.
    theirs
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &first_prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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
        .render(
            &second_prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
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

#[test]
fn two_faces_of_one_plate_are_one_definition_and_two_faces() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(160, 160);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let frame = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let mut faces = Vec::new();
    let mut definitions = Vec::new();
    for (x, y) in pixels_of(&frame, |frame, x, y| frame.pick_at(x, y) != PickId::NOTHING) {
        let hit = frame.hit_at(x, y);
        assert_eq!(
            hit.definition(),
            frame.pick_at(x, y),
            "the two answers about one pixel disagree at {x},{y}"
        );
        if snapshot.definition(hit.definition()) == Some(0) {
            assert_ne!(hit.face(), FacePickId::NOTHING, "a drawn pixel of no face");
            if !faces.contains(&hit.face()) {
                faces.push(hit.face());
            }
        }
        if !definitions.contains(&hit.definition()) {
            definitions.push(hit.definition());
        }
    }

    // One definition drawn twice, and two faces of it: what a placement is has
    // not become part of either answer.
    assert_eq!(faces.len(), 2, "a plate of two faces read as {faces:?}");
    for face in &faces {
        assert_eq!(snapshot.definition_of_face(*face), Some(0));
    }
    assert_eq!(definitions.len(), 2, "three placements of two definitions");
}

#[test]
fn pointing_at_a_face_marks_that_face_in_every_placement_and_nothing_else() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(160, 160);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let drawn = pixels_of(&plain, |frame, x, y| frame.pick_at(x, y) != PickId::NOTHING);
    let (fx, fy) = drawn
        .iter()
        .copied()
        .find(|(x, y)| snapshot.definition(plain.pick_at(*x, *y)) == Some(0))
        .expect("the plate is drawn");
    let face = plain.hit_at(fx, fy).face();
    let marked = pixels_of(&plain, |frame, x, y| frame.hit_at(x, y).face() == face);
    let others: Vec<_> = drawn
        .iter()
        .copied()
        .filter(|(x, y)| plain.hit_at(*x, *y).face() != face)
        .collect();
    assert!(marked.len() > 100 && others.len() > 100);

    let pointed = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Face(face),
            &Visibility::default(),
        )
        .expect("draws");

    // Both placements of the plate show it, because a face belongs to the
    // definition and not to where it was put.
    for (x, y) in &marked {
        assert_ne!(
            pointed.colour_at(*x, *y),
            plain.colour_at(*x, *y),
            "the pointed-at face was left alone at {x},{y}"
        );
    }
    // And the face beside it, and the definition beside that, are untouched.
    for (x, y) in &others {
        assert_eq!(
            pointed.colour_at(*x, *y),
            plain.colour_at(*x, *y),
            "pointing at one face changed something else at {x},{y}"
        );
    }

    // What a click would say is unchanged, byte for byte.
    for (x, y) in &drawn {
        assert_eq!(pointed.pick_at(*x, *y), plain.pick_at(*x, *y));
    }
}

#[test]
fn pointing_at_a_definition_still_marks_all_of_it() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(160, 160);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let plate = pixels_of(&plain, |frame, x, y| {
        snapshot.definition(frame.pick_at(x, y)) == Some(0)
    });
    let pick = plain.pick_at(plate[0].0, plate[0].1);
    let pointed = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Definition(pick),
            &Visibility::default(),
        )
        .expect("draws");

    // Every face and every placement: a row in a list names a definition, and
    // face hover has not quietly narrowed what that means.
    for (x, y) in &plate {
        assert_ne!(
            pointed.colour_at(*x, *y),
            plain.colour_at(*x, *y),
            "a definition hover missed part of the definition at {x},{y}"
        );
    }
}

#[test]
fn a_chosen_definition_outranks_a_pointed_at_face() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(160, 160);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let plate = pixels_of(&plain, |frame, x, y| {
        snapshot.definition(frame.pick_at(x, y)) == Some(0)
    });
    let chosen = plain.pick_at(plate[0].0, plate[0].1);
    let face = plain.hit_at(plate[0].0, plate[0].1).face();

    let only_chosen = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(chosen),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let both = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(chosen),
            Marked::Face(face),
            &Visibility::default(),
        )
        .expect("draws");

    // A decision already made is not repainted by a question about part of it.
    for (x, y) in &plate {
        assert_eq!(
            both.colour_at(*x, *y),
            only_chosen.colour_at(*x, *y),
            "a face hover repainted a chosen definition at {x},{y}"
        );
    }
}

#[test]
fn the_backdrop_and_the_grid_are_no_face_at_all() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = model_over_the_plane(120, 120);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let frame = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let empty = pixels_of(&frame, |frame, x, y| frame.pick_at(x, y) == PickId::NOTHING);
    assert!(empty.len() > 100, "the model fills the whole frame");
    for (x, y) in &empty {
        let hit = frame.hit_at(*x, *y);
        assert_eq!(hit.definition(), PickId::NOTHING);
        assert_eq!(
            hit.face(),
            FacePickId::NOTHING,
            "a grid line or the background named a face at {x},{y}"
        );
    }
}

#[test]
fn a_face_of_another_picture_marks_nothing() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(120, 120);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    // Another picture, whose faces are numbered from one exactly as this
    // one's are. The numbers name faces here too, and the identity a face
    // carries is the only thing that stops them being used.
    let mut builder = SnapshotBuilder::new();
    let plate = builder.add_mesh(&two_faced_plate(9.0)).expect("packs");
    builder
        .place(plate, None, &Transform::IDENTITY, [1.0, 0.0, 0.0])
        .expect("places");
    let other = Arc::new(builder.build());
    let mut other_camera = Camera::new();
    other_camera.resize(120, 120);
    other_camera
        .frame(other.bounds().expect("something is drawn"))
        .expect("frames");
    let other_prepared = renderer.prepare(Arc::clone(&other)).expect("uploads");
    let other_frame = renderer
        .render(
            &other_prepared,
            &other_camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let drawn = pixels_of(&other_frame, |frame, x, y| {
        frame.hit_at(x, y).face() != FacePickId::NOTHING
    });
    let foreign = other_frame.hit_at(drawn[0].0, drawn[0].1).face();

    let pointed = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Face(foreign),
            &Visibility::default(),
        )
        .expect("draws");
    assert_eq!(
        pointed.colour(),
        plain.colour(),
        "a face of another picture marked something in this one"
    );
}

#[test]
fn pointing_at_faces_uploads_no_geometry_and_draws_the_same_frame_twice() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(96, 96);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let drawn = pixels_of(&plain, |frame, x, y| {
        frame.hit_at(x, y).face() != FacePickId::NOTHING
    });
    let face = plain.hit_at(drawn[0].0, drawn[0].1).face();

    let uploaded = renderer.geometry_uploads();
    let first = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Face(face),
            &Visibility::default(),
        )
        .expect("draws");
    let second = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Face(face),
            &Visibility::default(),
        )
        .expect("draws");

    assert_eq!(first.colour(), second.colour());
    for (x, y) in &drawn {
        assert_eq!(first.hit_at(*x, *y), second.hit_at(*x, *y));
    }
    // The faces were prepared with the geometry. Moving a pointer is a
    // uniform, and a person moving one across a part must not be uploading a
    // table per frame.
    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "pointing at a face uploaded geometry"
    );
}

#[test]
fn choosing_a_face_marks_that_face_in_every_placement_and_nothing_else() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(160, 160);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let drawn = pixels_of(&plain, |frame, x, y| frame.pick_at(x, y) != PickId::NOTHING);
    let (fx, fy) = drawn
        .iter()
        .copied()
        .find(|(x, y)| snapshot.definition(plain.pick_at(*x, *y)) == Some(0))
        .expect("the plate is drawn");
    let face = plain.hit_at(fx, fy).face();
    let chosen: Vec<_> = drawn
        .iter()
        .copied()
        .filter(|(x, y)| plain.hit_at(*x, *y).face() == face)
        .collect();
    let others: Vec<_> = drawn
        .iter()
        .copied()
        .filter(|(x, y)| plain.hit_at(*x, *y).face() != face)
        .collect();
    assert!(chosen.len() > 100 && others.len() > 100);

    let selected = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    // Both placements of the plate, because a face belongs to the definition
    // and not to where it was put.
    for (x, y) in &chosen {
        assert_ne!(
            selected.colour_at(*x, *y),
            plain.colour_at(*x, *y),
            "the chosen face was left alone at {x},{y}"
        );
    }
    // The face beside it, and the definition beside that, are untouched: a
    // chosen face is not a chosen part.
    for (x, y) in &others {
        assert_eq!(
            selected.colour_at(*x, *y),
            plain.colour_at(*x, *y),
            "choosing one face changed something else at {x},{y}"
        );
    }

    // What a click would say next is unchanged, in both answers.
    for (x, y) in &drawn {
        assert_eq!(selected.hit_at(*x, *y), plain.hit_at(*x, *y));
    }
}

#[test]
fn a_chosen_face_a_chosen_definition_and_a_pointed_at_face_all_look_different() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(120, 120);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let (x, y) = pixels_of(&plain, |frame, x, y| {
        snapshot.definition(frame.pick_at(x, y)) == Some(0)
    })[0];
    let face = plain.hit_at(x, y).face();
    let definition = plain.pick_at(x, y);

    let as_face = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let as_definition = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(definition),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let pointed = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Face(face),
            &Visibility::default(),
        )
        .expect("draws");

    // Three states a person has to be able to tell apart at the same pixel.
    let colours = [
        plain.colour_at(x, y),
        as_face.colour_at(x, y),
        as_definition.colour_at(x, y),
        pointed.colour_at(x, y),
    ];
    for (first, one) in colours.iter().enumerate() {
        for (second, other) in colours.iter().enumerate().skip(first + 1) {
            assert_ne!(one, other, "states {first} and {second} look the same");
        }
    }
}

#[test]
fn choosing_a_face_beats_pointing_at_anything() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(120, 120);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let plate = pixels_of(&plain, |frame, x, y| {
        snapshot.definition(frame.pick_at(x, y)) == Some(0)
    });
    let (x, y) = plate[0];
    let face = plain.hit_at(x, y).face();
    let definition = plain.pick_at(x, y);
    let neighbour = plate
        .iter()
        .copied()
        .find(|(x, y)| plain.hit_at(*x, *y).face() != face)
        .expect("the plate has a second face");
    let other_face = plain.hit_at(neighbour.0, neighbour.1).face();

    let alone = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    for hovered in [
        Marked::Face(face),
        Marked::Face(other_face),
        Marked::Definition(definition),
    ] {
        let with_pointer = renderer
            .render(
                &prepared,
                &camera,
                Marked::Face(face),
                hovered,
                &Visibility::default(),
            )
            .expect("draws");
        // A decision already made is not repainted by a question about it.
        for (x, y) in &pixels_of(&plain, |frame, x, y| frame.hit_at(x, y).face() == face) {
            assert_eq!(
                with_pointer.colour_at(*x, *y),
                alone.colour_at(*x, *y),
                "pointing at {hovered:?} repainted the chosen face at {x},{y}"
            );
        }
    }
}

#[test]
fn a_face_of_another_picture_chooses_nothing() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(120, 120);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let mut builder = SnapshotBuilder::new();
    let plate = builder.add_mesh(&two_faced_plate(9.0)).expect("packs");
    builder
        .place(plate, None, &Transform::IDENTITY, [1.0, 0.0, 0.0])
        .expect("places");
    let other = Arc::new(builder.build());
    let other_prepared = renderer.prepare(Arc::clone(&other)).expect("uploads");
    let mut other_camera = Camera::new();
    other_camera.resize(120, 120);
    other_camera
        .frame(other.bounds().expect("geometry"))
        .expect("frames");
    let other_frame = renderer
        .render(
            &other_prepared,
            &other_camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let foreign = pixels_of(&other_frame, |frame, x, y| {
        frame.hit_at(x, y).face() != FacePickId::NOTHING
    })[0];
    let foreign = other_frame.hit_at(foreign.0, foreign.1).face();

    let selected = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(foreign),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    assert_eq!(
        selected.colour(),
        plain.colour(),
        "a face of another picture chose something in this one"
    );
}

#[test]
fn choosing_a_face_costs_no_geometry_and_draws_the_same_frame_twice() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(96, 96);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let drawn = pixels_of(&plain, |frame, x, y| {
        frame.hit_at(x, y).face() != FacePickId::NOTHING
    });
    let face = plain.hit_at(drawn[0].0, drawn[0].1).face();

    let uploaded = renderer.geometry_uploads();
    let first = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let second = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    assert_eq!(first.colour(), second.colour());
    for (x, y) in &drawn {
        assert_eq!(first.hit_at(*x, *y), second.hit_at(*x, *y));
    }
    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "choosing a face uploaded geometry"
    );
}

#[test]
fn the_backdrop_and_the_grid_cannot_be_chosen_as_a_face() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = model_over_the_plane(120, 120);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    // Every pixel that is not the model: the grid, and the background behind
    // it. Neither answers with a face, so neither can be chosen as one.
    let empty = pixels_of(&plain, |frame, x, y| frame.pick_at(x, y) == PickId::NOTHING);
    assert!(empty.len() > 100);
    for (x, y) in &empty {
        let hit = plain.hit_at(*x, *y);
        assert_eq!(hit.face(), FacePickId::NOTHING);
        assert_eq!(hit.definition(), PickId::NOTHING);
    }

    // And marking nothing leaves the backdrop exactly as it was.
    let marked = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(FacePickId::NOTHING),
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    assert_eq!(marked.colour(), plain.colour());
}

/// A big plate with a small one directly behind it.
///
/// From the camera framing puts on the far side of Y, the front one covers the
/// rear one completely.
fn occluding_pair(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let plate = |half: f32, y: f32, shape: u64| {
        let handle = ShapeHandle::new(SessionId::new(), shape);
        Mesh {
            positions: vec![
                -half, y, -half, half, y, -half, half, y, half, -half, y, half,
            ],
            normals: vec![
                0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(handle, SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 6,
            }],
        }
    };

    let mut builder = SnapshotBuilder::new();
    let front = builder.add_mesh(&plate(20.0, 0.0, 1)).expect("packs");
    let rear = builder.add_mesh(&plate(4.0, 9.0, 2)).expect("packs");
    builder
        .place(front, None, &Transform::IDENTITY, [0.8, 0.2, 0.2])
        .expect("places");
    builder
        .place(rear, None, &Transform::IDENTITY, [0.2, 0.4, 0.9])
        .expect("places");
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("the pair has an extent"))
        .expect("frames");
    (snapshot, camera)
}

#[test]
fn hiding_what_is_in_front_reveals_exactly_what_was_behind_it() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = occluding_pair(128, 128);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let everything = Visibility::new(&snapshot);

    let before = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");
    let front = snapshot.pick_of(0).expect("drawn");
    let rear = snapshot.pick_of(1).expect("drawn");
    let model = pixels_of(&before, |frame, x, y| {
        frame.pick_at(x, y) != PickId::NOTHING
    });
    assert!(model.len() > 400, "the pair is drawn");
    assert!(
        model.iter().all(|(x, y)| before.pick_at(*x, *y) == front),
        "the gate needs the front definition to cover the rear one"
    );

    let mut visibility = everything.clone();
    assert!(visibility.hide(Marked::Definition(front), &snapshot));
    let after = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    // What was behind is now drawn, and is exactly itself: its own colour, its
    // own definition and its own face.
    let revealed = pixels_of(&after, |frame, x, y| frame.pick_at(x, y) != PickId::NOTHING);
    assert!(!revealed.is_empty(), "hiding the front revealed nothing");
    let rear_face = snapshot.face_of(1, 0).expect("numbered");
    for (x, y) in &revealed {
        assert_eq!(after.pick_at(*x, *y), rear);
        assert_eq!(after.hit_at(*x, *y).face(), rear_face);
    }

    // And a hidden definition is omitted rather than dimmed: where only it was
    // drawn there is now the backdrop, and nothing at all in either identity
    // target. A renderer that suppressed the colour and kept the identities
    // would leave a click landing on something invisible.
    for (x, y) in &model {
        if after.pick_at(*x, *y) == PickId::NOTHING {
            assert_eq!(after.hit_at(*x, *y).definition(), PickId::NOTHING);
            assert_eq!(after.hit_at(*x, *y).face(), FacePickId::NOTHING);
        }
    }
    assert!(
        model
            .iter()
            .any(|(x, y)| after.pick_at(*x, *y) == PickId::NOTHING),
        "the front covered more than the rear, so some of it must now be backdrop"
    );
}

#[test]
fn a_hidden_definition_cannot_be_marked_by_a_selection_or_a_pointer() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = occluding_pair(96, 96);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let front = snapshot.pick_of(0).expect("drawn");
    let face = snapshot.face_of(0, 0).expect("numbered");

    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.hide(Marked::Definition(front), &snapshot));
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    // Choosing or pointing at something that is not drawn tints nothing: it
    // has no pixels to tint, which is the whole of why hiding is omission.
    for (selected, hovered) in [
        (Marked::Definition(front), Marked::Nothing),
        (Marked::Face(face), Marked::Nothing),
        (Marked::Nothing, Marked::Definition(front)),
        (Marked::Nothing, Marked::Face(face)),
    ] {
        let marked = renderer
            .render(&prepared, &camera, selected, hovered, &visibility)
            .expect("draws");
        assert_eq!(
            marked.colour(),
            plain.colour(),
            "a hidden definition was tinted by {selected:?}/{hovered:?}"
        );
    }
}

#[test]
fn a_selected_face_hides_the_whole_definition_it_is_on() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(128, 128);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let everything = Visibility::new(&snapshot);
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");

    let plate = pixels_of(&plain, |frame, x, y| {
        snapshot.definition(frame.pick_at(x, y)) == Some(0)
    });
    // Placed more than once, so "every pixel of it" below is a claim about
    // every placement and not about one of them.
    assert_eq!(
        snapshot
            .draws()
            .iter()
            .filter(|item| item.mesh == 0)
            .count(),
        2,
        "the gate needs the definition to be drawn in two places"
    );
    let face = plain.hit_at(plate[0].0, plate[0].1).face();
    let mut visibility = everything.clone();
    assert!(visibility.hide(Marked::Face(face), &snapshot));

    let after = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    // Every pixel of the whole definition, not only the face that was chosen.
    for (x, y) in &plate {
        assert_eq!(
            snapshot.definition(after.pick_at(*x, *y)),
            None,
            "part of the definition survived hiding one of its faces at {x},{y}"
        );
    }
    // And the definition beside it is untouched.
    assert!(
        pixels_of(&after, |frame, x, y| snapshot
            .definition(frame.pick_at(x, y))
            == Some(1))
        .len()
            > 20,
        "hiding one definition removed another"
    );
}

#[test]
fn the_grid_is_not_something_that_can_be_hidden() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = model_over_the_plane(120, 120);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let mut visibility = Visibility::new(&snapshot);
    let with_model = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    assert!(visibility.hide(
        Marked::Definition(snapshot.pick_of(0).expect("drawn")),
        &snapshot
    ));
    let without = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    // The backdrop is still there once the only definition is hidden, and it
    // is still not a thing that can be picked.
    let lit = pixels_of(&without, |frame, x, y| {
        frame
            .colour_at(x, y)
            .is_some_and(|colour| colour != [0, 0, 0, 255])
    });
    assert!(!lit.is_empty(), "hiding the model took the grid with it");
    for (x, y) in &lit {
        assert_eq!(without.pick_at(*x, *y), PickId::NOTHING);
        assert_eq!(without.hit_at(*x, *y).face(), FacePickId::NOTHING);
    }
    assert_ne!(with_model.colour(), without.colour());
}

#[test]
fn hiding_and_showing_upload_nothing_and_repeat_exactly() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = occluding_pair(96, 96);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let uploaded = renderer.geometry_uploads();
    let everything = Visibility::new(&snapshot);
    let mut visibility = everything.clone();

    let shown = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");
    for _ in 0..3 {
        assert!(visibility.hide(
            Marked::Definition(snapshot.pick_of(0).expect("drawn")),
            &snapshot
        ));
        let hidden_once = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Marked::Nothing,
                &visibility,
            )
            .expect("draws");
        let hidden_twice = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Marked::Nothing,
                &visibility,
            )
            .expect("draws");
        // The same visibility draws the same frame, down to the byte.
        assert_eq!(hidden_once.colour(), hidden_twice.colour());
        assert!(visibility.show_all());
        let again = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Marked::Nothing,
                &visibility,
            )
            .expect("draws");
        assert_eq!(again.colour(), shown.colour());
    }

    // Nothing was uploaded or repacked for any of it: what is drawn changed,
    // and what is resident did not.
    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "hiding or showing uploaded geometry"
    );
}

/// Three plates side by side, each placed twice, none hiding another.
///
/// Separated so every definition contributes pixels of its own: what isolation
/// removes has to be visible before it can be shown to be gone.
fn three_plates(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let mut builder = SnapshotBuilder::new();
    let mut parts = Vec::new();
    for shape in 1..=3u64 {
        parts.push(
            builder
                .add_mesh(&two_faced_plate(4.0 + shape as f32))
                .expect("packs"),
        );
    }
    for (index, part) in parts.iter().enumerate() {
        for z in [-30.0, 30.0] {
            builder
                .place(
                    *part,
                    None,
                    &Transform::from_translation(
                        ferritecad_types::Vec3::new(index as f64 * 40.0 - 40.0, 0.0, z)
                            .expect("finite"),
                    )
                    .expect("finite"),
                    [0.2 + index as f64 * 0.3, 0.5, 0.8],
                )
                .expect("places");
        }
    }
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("the plates have an extent"))
        .expect("frames");
    (snapshot, camera)
}

#[test]
fn isolating_one_definition_leaves_only_its_pixels_picks_and_faces() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = three_plates(160, 160);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let everything = Visibility::new(&snapshot);

    let before = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");
    // All three are on screen to begin with, which is what makes their absence
    // afterwards mean something.
    for definition in 0..3 {
        assert!(
            pixels_of(&before, |frame, x, y| snapshot
                .definition(frame.pick_at(x, y))
                == Some(definition))
            .len()
                > 40,
            "definition {definition} is not drawn before isolating"
        );
    }

    let keep = snapshot.pick_of(1).expect("drawn");
    let mut visibility = everything.clone();
    assert!(visibility.isolate(Marked::Definition(keep), &snapshot));
    let after = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    // Every pixel of the model, every definition identity and every face
    // identity belongs to the one that was kept.
    let drawn = pixels_of(&after, |frame, x, y| frame.pick_at(x, y) != PickId::NOTHING);
    assert!(!drawn.is_empty(), "isolating left nothing on screen");
    let faces: Vec<_> = (0..snapshot.meshes()[1].face_count())
        .map(|ordinal| snapshot.face_of(1, ordinal).expect("numbered"))
        .collect();
    for (x, y) in &drawn {
        assert_eq!(after.pick_at(*x, *y), keep);
        assert!(faces.contains(&after.hit_at(*x, *y).face()));
    }

    // Both placements of it are still there: two clusters, on either side of
    // the middle of the frame.
    let middle = after.height() / 2;
    let near = drawn.iter().filter(|(_, y)| *y < middle).count();
    let far = drawn.len() - near;
    assert!(
        near > 20 && far > 20,
        "one placement of the isolated definition went missing"
    );

    // Where the neighbours were there is now backdrop, and no stale identity
    // of anything.
    let vacated = pixels_of(&before, |frame, x, y| {
        snapshot.definition(frame.pick_at(x, y)) == Some(0)
    });
    assert!(!vacated.is_empty());
    for (x, y) in &vacated {
        assert_eq!(after.pick_at(*x, *y), PickId::NOTHING);
        assert_eq!(after.hit_at(*x, *y).face(), FacePickId::NOTHING);
        assert_eq!(after.hit_at(*x, *y).definition(), PickId::NOTHING);
    }
}

#[test]
fn an_isolated_definition_still_looks_chosen_and_its_neighbours_cannot_be_marked() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = three_plates(128, 128);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let keep = snapshot.pick_of(1).expect("drawn");
    let face = snapshot.face_of(1, 0).expect("numbered");
    let gone = snapshot.pick_of(0).expect("drawn");
    let gone_face = snapshot.face_of(0, 0).expect("numbered");

    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.isolate(Marked::Definition(keep), &snapshot));

    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    // What is chosen still looks chosen after everything else has gone.
    let as_definition = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(keep),
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    let as_face = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    assert_ne!(as_definition.colour(), plain.colour());
    assert_ne!(as_face.colour(), plain.colour());
    assert_ne!(as_face.colour(), as_definition.colour());

    // And nothing that was removed can be marked, chosen or pointed at: it has
    // no pixels to mark.
    for (selected, hovered) in [
        (Marked::Definition(gone), Marked::Nothing),
        (Marked::Face(gone_face), Marked::Nothing),
        (Marked::Nothing, Marked::Definition(gone)),
        (Marked::Nothing, Marked::Face(gone_face)),
    ] {
        let marked = renderer
            .render(&prepared, &camera, selected, hovered, &visibility)
            .expect("draws");
        assert_eq!(
            marked.colour(),
            plain.colour(),
            "something isolated away was tinted by {selected:?}/{hovered:?}"
        );
    }
}

#[test]
fn isolating_keeps_the_backdrop_and_costs_no_geometry() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, _camera) = model_over_the_plane(120, 120);
    // Prepared so the count below starts from a picture that is resident.
    let _ = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let uploaded = renderer.geometry_uploads();
    let visibility = Visibility::new(&snapshot);

    // One definition over the grid: it is alone, so there is nothing to
    // isolate away and the grid is untouched either way.
    assert!(!visibility.can_isolate(
        Marked::Definition(snapshot.pick_of(0).expect("drawn")),
        &snapshot
    ));

    let (three, three_camera) = three_plates(96, 96);
    let prepared = renderer.prepare(Arc::clone(&three)).expect("uploads");
    let uploaded_after_preparing = renderer.geometry_uploads();
    assert!(
        uploaded_after_preparing > uploaded,
        "the gate uploaded nothing"
    );
    let mut visibility = Visibility::new(&three);
    assert!(visibility.isolate(Marked::Definition(three.pick_of(1).expect("drawn")), &three));

    let once = renderer
        .render(
            &prepared,
            &three_camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    let twice = renderer
        .render(
            &prepared,
            &three_camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    // The same visibility draws the same frame, down to the byte, and nothing
    // was uploaded or repacked to achieve any of it.
    assert_eq!(once.colour(), twice.colour());
    assert_eq!(
        renderer.geometry_uploads(),
        uploaded_after_preparing,
        "isolating uploaded geometry"
    );

    // The grid is still drawn where the model is not, and is still not
    // something a click can reach.
    let empty = pixels_of(&once, |frame, x, y| frame.pick_at(x, y) == PickId::NOTHING);
    for (x, y) in empty.iter().take(200) {
        assert_eq!(once.hit_at(*x, *y).face(), FacePickId::NOTHING);
    }
}

#[test]
fn one_definition_returns_with_its_own_pixels_picks_and_faces() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = three_plates(160, 160);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let uploaded = renderer.geometry_uploads();

    // Only the middle one on screen, which is where a person ends up after
    // isolating and then wanting one of the others back.
    let kept = snapshot.pick_of(1).expect("drawn");
    let returning = snapshot.pick_of(0).expect("drawn");
    let staying_hidden = snapshot.pick_of(2).expect("drawn");
    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.isolate(Marked::Definition(kept), &snapshot));

    let alone = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    let was_there = pixels_of(&alone, |frame, x, y| frame.pick_at(x, y) == returning);
    assert!(
        was_there.is_empty(),
        "the definition to be shown is already on screen"
    );

    assert!(visibility.show(Marked::Definition(returning), &snapshot));
    let back = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    // It is back, in both of its placements, with its own identities.
    let returned = pixels_of(&back, |frame, x, y| frame.pick_at(x, y) == returning);
    assert!(returned.len() > 40, "the definition did not come back");
    let faces: Vec<_> = (0..snapshot.meshes()[0].face_count())
        .map(|ordinal| snapshot.face_of(0, ordinal).expect("numbered"))
        .collect();
    for (x, y) in &returned {
        assert!(faces.contains(&back.hit_at(*x, *y).face()));
    }
    let middle = back.height() / 2;
    let near = returned.iter().filter(|(_, y)| *y < middle).count();
    assert!(
        near > 10 && returned.len() - near > 10,
        "one placement of the returned definition is missing"
    );

    // The other hidden one stayed away, and what was already drawn is
    // untouched where it was drawn.
    assert!(
        pixels_of(&back, |frame, x, y| frame.pick_at(x, y) == staying_hidden).is_empty(),
        "showing one definition brought back another"
    );
    for (x, y) in &pixels_of(&alone, |frame, x, y| frame.pick_at(x, y) == kept) {
        assert_eq!(back.pick_at(*x, *y), kept);
    }

    // Where nothing came back there is still no stale identity of anything.
    for (x, y) in &pixels_of(&back, |frame, x, y| frame.pick_at(x, y) == PickId::NOTHING) {
        assert_eq!(back.hit_at(*x, *y).definition(), PickId::NOTHING);
        assert_eq!(back.hit_at(*x, *y).face(), FacePickId::NOTHING);
    }

    // Nothing was uploaded for any of it, and the same mask draws the same
    // frame twice.
    let again = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    assert_eq!(again.colour(), back.colour());
    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "showing one definition uploaded geometry"
    );
}

#[test]
fn a_definition_that_came_back_does_not_disturb_what_was_chosen() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = three_plates(128, 128);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let kept = snapshot.pick_of(1).expect("drawn");
    let face = snapshot.face_of(1, 0).expect("numbered");
    let returning = snapshot.pick_of(0).expect("drawn");
    let staying_hidden = snapshot.pick_of(2).expect("drawn");

    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.isolate(Marked::Definition(kept), &snapshot));
    let chosen_before = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(kept),
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    let face_before = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    assert!(visibility.show(Marked::Definition(returning), &snapshot));
    let chosen_after = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(kept),
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    let face_after = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    // What was chosen looks exactly as it did, wherever it is drawn.
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    for (x, y) in &pixels_of(&plain, |frame, x, y| frame.pick_at(x, y) == kept) {
        assert_eq!(
            chosen_after.colour_at(*x, *y),
            chosen_before.colour_at(*x, *y)
        );
        assert_eq!(face_after.colour_at(*x, *y), face_before.colour_at(*x, *y));
    }
    assert_ne!(chosen_after.colour(), face_after.colour());

    // And what is still hidden cannot be marked, chosen or pointed at.
    for (selected, hovered) in [
        (Marked::Definition(staying_hidden), Marked::Nothing),
        (Marked::Nothing, Marked::Definition(staying_hidden)),
        (
            Marked::Nothing,
            Marked::Face(snapshot.face_of(2, 0).expect("numbered")),
        ),
    ] {
        let marked = renderer
            .render(&prepared, &camera, selected, hovered, &visibility)
            .expect("draws");
        assert_eq!(
            marked.colour(),
            plain.colour(),
            "something still hidden was tinted by {selected:?}/{hovered:?}"
        );
    }
}

#[test]
fn one_definition_taken_off_screen_leaves_no_trace_of_itself() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = three_plates(160, 160);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let uploaded = renderer.geometry_uploads();
    let everything = Visibility::new(&snapshot);

    let before = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");
    let going = snapshot.pick_of(0).expect("drawn");
    let staying = [
        snapshot.pick_of(1).expect("drawn"),
        snapshot.pick_of(2).expect("drawn"),
    ];
    let was_there = pixels_of(&before, |frame, x, y| frame.pick_at(x, y) == going);
    assert!(
        was_there.len() > 40,
        "the definition to be hidden is not drawn"
    );

    // The same mask the row action produces: hiding one definition by its own
    // identity.
    let mut visibility = everything.clone();
    assert!(visibility.hide(Marked::Definition(going), &snapshot));
    let after = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    // Nothing of it remains: no pixels, no definition identity, no face.
    for (x, y) in &was_there {
        assert_ne!(after.pick_at(*x, *y), going);
        let hit = after.hit_at(*x, *y);
        if hit.definition() == PickId::NOTHING {
            assert_eq!(hit.face(), FacePickId::NOTHING);
        }
    }
    assert!(
        pixels_of(&after, |frame, x, y| frame.pick_at(x, y) == going).is_empty(),
        "the hidden definition still answers somewhere"
    );

    // Everything else is exactly as it was, in every placement.
    for pick in staying {
        let mine = pixels_of(&before, |frame, x, y| frame.pick_at(x, y) == pick);
        assert!(!mine.is_empty());
        for (x, y) in &mine {
            assert_eq!(after.pick_at(*x, *y), pick);
            assert_eq!(after.hit_at(*x, *y).face(), before.hit_at(*x, *y).face());
            assert_eq!(after.colour_at(*x, *y), before.colour_at(*x, *y));
        }
    }

    // And nothing was uploaded to achieve it.
    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "hiding one definition uploaded geometry"
    );
}

#[test]
fn taking_a_change_back_draws_the_frame_that_was_there_before_it() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = three_plates(160, 160);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let uploaded = renderer.geometry_uploads();

    // A mixed arrangement, deliberately built: one of three already away.
    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.hide(
        Marked::Definition(snapshot.pick_of(2).expect("drawn")),
        &snapshot
    ));
    let before = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    // The accident, and then taking it back.
    assert!(visibility.hide(
        Marked::Definition(snapshot.pick_of(0).expect("drawn")),
        &snapshot
    ));
    let accident = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    assert_ne!(
        accident.colour(),
        before.colour(),
        "the accident changed nothing"
    );

    assert!(visibility.undo(&snapshot));
    let after = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");

    // The same frame, byte for byte, in colour and in both identity targets:
    // every placement and every face is back exactly as it was, and the one
    // that was already away is still away.
    assert_eq!(after.colour(), before.colour());
    for y in 0..after.height() {
        for x in 0..after.width() {
            assert_eq!(after.pick_at(x, y), before.pick_at(x, y));
            assert_eq!(after.hit_at(x, y), before.hit_at(x, y));
        }
    }
    assert!(
        pixels_of(&after, |frame, x, y| frame.pick_at(x, y)
            == snapshot.pick_of(2).expect("drawn"))
        .is_empty(),
        "taking one change back brought something else back with it"
    );

    // And nothing was uploaded for any of it.
    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "taking a change back uploaded geometry"
    );
}

/// Two equal plates, one behind the other and offset sideways so neither hides
/// the other, seen down the Y axis.
fn two_equal_plates_at_two_depths(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let plate = |shape: u64| {
        let handle = ShapeHandle::new(SessionId::new(), shape);
        Mesh {
            positions: vec![
                -5.0, 0.0, -5.0, 5.0, 0.0, -5.0, 5.0, 0.0, 5.0, -5.0, 0.0, 5.0,
            ],
            normals: vec![
                0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(handle, SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 6,
            }],
        }
    };

    let mut builder = SnapshotBuilder::new();
    let near = builder.add_mesh(&plate(1)).expect("packs");
    let far = builder.add_mesh(&plate(2)).expect("packs");
    // Same size, different distance from the eye, side by side so both are on
    // screen at once and neither occludes the other.
    for (definition, x, y) in [(near, -12.0, 0.0), (far, 12.0, 60.0)] {
        builder
            .place(
                definition,
                None,
                &Transform::from_translation(
                    ferritecad_types::Vec3::new(x, y, 0.0).expect("finite"),
                )
                .expect("finite"),
                [0.7, 0.7, 0.7],
            )
            .expect("places");
    }
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("the plates have an extent"))
        .expect("frames");
    (snapshot, camera)
}

/// How many pixels wide one definition is, at its widest row.
fn widest_row(frame: &ferritecad_viewport_gpu::Frame, pick: PickId) -> u32 {
    (0..frame.height())
        .map(|y| {
            (0..frame.width())
                .filter(|x| frame.pick_at(*x, y) == pick)
                .count() as u32
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn equal_plates_at_two_depths_are_equal_only_in_an_orthographic_view() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_equal_plates_at_two_depths(200, 200);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let everything = Visibility::default();
    let near = snapshot.pick_of(0).expect("drawn");
    let far = snapshot.pick_of(1).expect("drawn");

    // The defect, in pixels: the same plate further away is drawn smaller, so
    // a plan or elevation cannot be measured off the screen.
    let perspective = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");
    let (near_wide, far_wide) = (
        widest_row(&perspective, near),
        widest_row(&perspective, far),
    );
    assert!(
        near_wide > 20 && far_wide > 10,
        "the gate needs both plates on screen: {near_wide} and {far_wide}"
    );
    assert!(
        near_wide > far_wide + 4,
        "the perspective view already draws them the same size: {near_wide} against {far_wide}"
    );

    // Orthographic: equal things are equal wherever they are.
    let mut square = camera;
    square.set_projection(Projection::Orthographic);
    let orthographic = renderer
        .render(
            &prepared,
            &square,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");
    let (near_flat, far_flat) = (
        widest_row(&orthographic, near),
        widest_row(&orthographic, far),
    );
    assert!(near_flat > 20, "the near plate left the orthographic view");
    assert_eq!(
        near_flat, far_flat,
        "equal plates are drawn at different widths in an orthographic view"
    );

    // And the width is the width the camera says it is. Comparing the two
    // plates with each other cannot see a renderer that squeezed the whole
    // picture equally; this can, because it ties pixels to the one place that
    // decides how much world a pixel covers.
    let expected = 10.0 / square.world_per_pixel();
    assert!(
        (near_flat as f32 - expected).abs() <= 2.0,
        "a ten millimetre plate covers {near_flat} pixels where the camera says {expected}"
    );

    // And both are still there to be clicked, with their own identities.
    for (definition, pick) in [(0usize, near), (1, far)] {
        let mine = pixels_of(&orthographic, |frame, x, y| frame.pick_at(x, y) == pick);
        assert!(!mine.is_empty(), "definition {definition} cannot be picked");
        let face = snapshot.face_of(definition, 0).expect("numbered");
        for (x, y) in &mine {
            assert_eq!(orthographic.hit_at(*x, *y).face(), face);
        }
    }
}

#[test]
fn a_drawing_still_hides_what_is_behind_something_and_repeats_exactly() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = occluding_pair(128, 128);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let everything = Visibility::default();
    let uploaded = renderer.geometry_uploads();

    let mut flat = camera;
    assert!(flat.set_projection(Projection::Orthographic));
    let front = snapshot.pick_of(0).expect("drawn");
    let rear = snapshot.pick_of(1).expect("drawn");

    let drawing = renderer
        .render(
            &prepared,
            &flat,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");

    // Depth still decides: the plate in front covers the one behind it, which
    // is a property of the depth test and not of the projection.
    let model = pixels_of(&drawing, |frame, x, y| {
        frame.pick_at(x, y) != PickId::NOTHING
    });
    assert!(model.len() > 400, "the pair is not drawn");
    assert!(
        model.iter().all(|(x, y)| drawing.pick_at(*x, *y) == front),
        "the rear plate showed through the front one"
    );
    assert!(pixels_of(&drawing, |frame, x, y| frame.pick_at(x, y) == rear).is_empty());

    // Hiding the front one reveals the rear one, in the same projection.
    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.hide(Marked::Definition(front), &snapshot));
    let revealed = renderer
        .render(
            &prepared,
            &flat,
            Marked::Nothing,
            Marked::Nothing,
            &visibility,
        )
        .expect("draws");
    assert!(!pixels_of(&revealed, |frame, x, y| frame.pick_at(x, y) == rear).is_empty());

    // The same camera draws the same frame twice, and none of it uploaded
    // anything: a projection is a matrix, not a different model.
    let again = renderer
        .render(
            &prepared,
            &flat,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");
    assert_eq!(again.colour(), drawing.colour());
    for (x, y) in &model {
        assert_eq!(again.hit_at(*x, *y), drawing.hit_at(*x, *y));
    }
    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "changing projection uploaded geometry"
    );
}

#[test]
fn the_backdrop_belongs_to_the_world_in_a_drawing_too() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = model_over_the_plane(120, 120);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let everything = Visibility::default();

    let mut flat = camera;
    assert!(flat.set_projection(Projection::Orthographic));
    let drawing = renderer
        .render(
            &prepared,
            &flat,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");

    // The grid is still drawn, is still not something a click can reach, and
    // still lies under the model rather than over it.
    let lit = pixels_of(&drawing, |frame, x, y| {
        frame
            .colour_at(x, y)
            .is_some_and(|colour| colour != [0, 0, 0, 255])
    });
    assert!(!lit.is_empty(), "the backdrop disappeared in a drawing");

    // The backdrop is measured in the same world as the model: looking
    // straight down at the plane, the gap between grid lines is the gap the
    // camera says it is. This is what makes the grid one more thing drawn
    // through the one projection rather than a picture of its own.
    let mut overhead = flat;
    overhead.look_from(ferritecad_viewport::StandardView::Top);
    let plan = ferritecad_viewport::grid_plan(&overhead).expect("the grid has a spacing");
    let straight_down = renderer
        .render(
            &prepared,
            &overhead,
            Marked::Nothing,
            Marked::Nothing,
            &everything,
        )
        .expect("draws");
    let expected = plan.minor / overhead.world_per_pixel();
    let measured = modal_line_gap(&straight_down, straight_down.height() / 2)
        .expect("no grid lines were found to measure");
    assert!(
        (measured - expected).abs() <= 2.0,
        "grid lines sit {measured} pixels apart where the camera says {expected}"
    );
    let model = pixels_of(&drawing, |frame, x, y| {
        frame.pick_at(x, y) != PickId::NOTHING
    });
    assert!(!model.is_empty(), "the model disappeared in a drawing");
    for (x, y) in &pixels_of(&drawing, |frame, x, y| {
        frame.pick_at(x, y) == PickId::NOTHING
    }) {
        assert_eq!(drawing.hit_at(*x, *y).face(), FacePickId::NOTHING);
        assert_eq!(drawing.hit_at(*x, *y).definition(), PickId::NOTHING);
    }
}

/// Three plates lying in one plane: a small marker away from the middle, a
/// larger neighbour to show the scale really changed, and one under the
/// centre. All at `y = 0`, which is the plane a front view targets, so the
/// pixels of the marker are pixels of the plane a wheel anchors on.
fn plates_in_the_target_plane(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let plate = |shape: u64, half: f32| {
        let handle = ShapeHandle::new(SessionId::new(), shape);
        Mesh {
            positions: vec![
                -half, 0.0, -half, half, 0.0, -half, half, 0.0, half, -half, 0.0, half,
            ],
            normals: vec![
                0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(handle, SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 6,
            }],
        }
    };

    let mut builder = SnapshotBuilder::new();
    let nearer = builder.add_mesh(&plate(0, 3.0)).expect("packs");
    let marker = builder.add_mesh(&plate(1, 3.0)).expect("packs");
    let neighbour = builder.add_mesh(&plate(2, 8.0)).expect("packs");
    let middle = builder.add_mesh(&plate(3, 5.0)).expect("packs");
    // The nearer plate is drawn first and lies in front of the larger one it
    // sits inside, so a renderer that stopped sorting by depth would paint it
    // over and leave nothing of it to find. The furthest plate balances it, so
    // that the middle of the whole extent is `y = 0` and the marker really
    // does lie on the plane a front view targets.
    for (definition, x, y, z, colour) in [
        (nearer, 0.0, -20.0, 0.0, [0.9, 0.9, 0.2]),
        (marker, 26.0, 0.0, 17.0, [0.9, 0.2, 0.2]),
        (neighbour, -24.0, 20.0, -14.0, [0.2, 0.9, 0.2]),
        (middle, 0.0, 0.0, 0.0, [0.2, 0.2, 0.9]),
    ] {
        builder
            .place(
                definition,
                None,
                &Transform::from_translation(ferritecad_types::Vec3::new(x, y, z).expect("finite"))
                    .expect("finite"),
                colour,
            )
            .expect("places");
    }
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("the plates have an extent"))
        .expect("frames");
    (snapshot, camera)
}

/// The middle of everything one definition drew, in pixels.
fn centre_of(frame: &ferritecad_viewport_gpu::Frame, pick: PickId) -> Option<(f32, f32)> {
    let mine = pixels_of(frame, |frame, x, y| frame.pick_at(x, y) == pick);
    if mine.is_empty() {
        return None;
    }
    let count = mine.len() as f32;
    let sum = mine.iter().fold((0.0f32, 0.0f32), |(x, y), (px, py)| {
        // A pixel covers a unit square, so its middle is half a pixel on.
        (x + *px as f32 + 0.5, y + *py as f32 + 0.5)
    });
    Some((sum.0 / count, sum.1 / count))
}

fn covered_by(frame: &ferritecad_viewport_gpu::Frame, pick: PickId) -> usize {
    pixels_of(frame, |frame, x, y| frame.pick_at(x, y) == pick).len()
}

#[test]
fn a_wheel_keeps_the_part_it_was_pointed_at_under_the_same_pixel() {
    let mut renderer = renderer_or_skip!();
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let (snapshot, mut camera) = plates_in_the_target_plane(320, 240);
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to zoom in"
        );
        let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
        let uploaded = renderer.geometry_uploads();
        let everything = Visibility::default();
        let nearer = snapshot.pick_of(0).expect("drawn");
        let marker = snapshot.pick_of(1).expect("drawn");
        let neighbour = snapshot.pick_of(2).expect("drawn");
        let marker_face = snapshot.face_of(1, 0).expect("numbered");

        let draw = |renderer: &mut Renderer, camera: &Camera| {
            renderer
                .render(
                    &prepared,
                    camera,
                    Marked::Nothing,
                    Marked::Nothing,
                    &everything,
                )
                .expect("draws")
        };

        let before = draw(&mut renderer, &camera);
        let (x, y) = centre_of(&before, marker).expect("the marker is on screen");
        assert!(
            (x - 160.0).abs() > 30.0 && (y - 120.0).abs() > 20.0,
            "{projection:?}: the marker is not off centre, it is at ({x}, {y})"
        );
        let was = covered_by(&before, neighbour);

        // Point at the middle of the marker and wind the wheel in.
        camera.zoom_at(0.5, x - 160.0, 120.0 - y);
        let after = draw(&mut renderer, &camera);

        let (moved_x, moved_y) = centre_of(&after, marker).expect("the marker is still on screen");
        assert!(
            (moved_x - x).abs() <= 1.5 && (moved_y - y).abs() <= 1.5,
            "{projection:?}: the marker was at ({x}, {y}) and is now at ({moved_x}, {moved_y})"
        );

        // The pixel that was pointed at still belongs to the same part, and to
        // the same face of it.
        let (px, py) = (x as u32, y as u32);
        assert_eq!(
            after.pick_at(px, py),
            marker,
            "{projection:?}: the pointed-at pixel changed hands"
        );
        assert_eq!(
            after.hit_at(px, py).face(),
            marker_face,
            "{projection:?}: the pointed-at pixel changed which face it shows"
        );
        assert_eq!(
            before.pick_at(px, py),
            marker,
            "{projection:?}: the gate measured a pixel the marker never covered"
        );

        // The picture really changed scale, by the amount a notch means. The
        // anchor is measured rather than a corner of the view: what a wheel
        // holds still is a point, not a size, and the marker grows about it
        // without leaving the screen for the count to be clipped.
        let (was_wide, now_wide) = (widest_row(&before, marker), widest_row(&after, marker));
        let grew = now_wide as f32 / was_wide as f32;
        assert!(
            (grew - 0.5f32.exp()).abs() < 0.15,
            "{projection:?}: a plate {was_wide} pixels wide became {now_wide}, \
             a factor of {grew} where a notch of 0.5 means {}",
            0.5f32.exp()
        );
        // And it is the width the camera says it is. Comparing the picture
        // with itself cannot see a renderer that squeezed the whole frame
        // equally; measuring a known plate against the one place that decides
        // how much world a pixel covers can.
        let expected = 6.0 / camera.world_per_pixel();
        assert!(
            (now_wide as f32 - expected).abs() <= 2.0,
            "{projection:?}: a six millimetre plate covers {now_wide} pixels \
             where the camera says {expected}"
        );

        // And something that is not the anchor changed too, so a picture that
        // merely grew one part could not pass.
        let now = covered_by(&after, neighbour);
        assert!(
            now > was,
            "{projection:?}: nothing but the anchor changed: {was} pixels became {now}"
        );

        // What is in front is still in front. The nearer plate is drawn first
        // and lies inside the larger one, so it survives only while depth
        // decides which pixel wins.
        assert!(
            covered_by(&after, nearer) > 0,
            "{projection:?}: the nearer plate vanished behind the one it is in front of"
        );

        // Nothing about a wheel is a change to the model.
        assert_eq!(
            renderer.geometry_uploads(),
            uploaded,
            "{projection:?}: zooming uploaded geometry"
        );

        // The nearer plate still covers the further one where they overlap,
        // and the backdrop is still the world's rather than the screen's.
        let repeat = draw(&mut renderer, &camera);
        assert_eq!(
            after.colour(),
            repeat.colour(),
            "{projection:?}: the same camera drew two different pictures"
        );
        for (x, y) in pixels_of(&after, |frame, x, y| frame.pick_at(x, y) != PickId::NOTHING) {
            assert_eq!(
                after.pick_at(x, y),
                repeat.pick_at(x, y),
                "{projection:?}: what a pixel belongs to depends on when it was drawn"
            );
        }
    }
}

#[test]
fn the_backdrop_follows_an_anchored_wheel_and_is_still_nobody_to_click() {
    let mut renderer = renderer_or_skip!();
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let (snapshot, mut overhead) = plates_in_the_target_plane(200, 200);
        overhead.look_from(StandardView::Top);
        assert!(
            projection == Projection::Perspective || overhead.set_projection(projection),
            "the camera refused a projection to zoom in"
        );
        let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
        let everything = Visibility::default();

        overhead.zoom_at(0.4, 70.0, -55.0);
        let frame = renderer
            .render(
                &prepared,
                &overhead,
                Marked::Nothing,
                Marked::Nothing,
                &everything,
            )
            .expect("draws");

        // The grid is drawn through the same camera as the model, so its
        // spacing on screen is what that camera says a millimetre is.
        let plan = ferritecad_viewport::grid_plan(&overhead).expect("the grid has a spacing");
        let expected = plan.minor / overhead.world_per_pixel();
        let gap = modal_line_gap(&frame, frame.height() / 2).expect("the grid drew lines");
        assert!(
            (gap - expected).abs() <= 2.0,
            "{projection:?}: grid lines are {gap} pixels apart where the camera says {expected}"
        );

        // And it is still scenery. Which lit pixels are the grid's cannot be
        // told from colour in a picture whose parts are coloured freely, so
        // the rule is stated the other way round: everything that is not a
        // part is nobody, with no definition and no face to reach.
        let backdrop = pixels_of(&frame, |frame, x, y| {
            frame.pick_at(x, y) == PickId::NOTHING
                && frame
                    .colour_at(x, y)
                    .is_some_and(|colour| colour != [0, 0, 0, 255])
        });
        assert!(
            !backdrop.is_empty(),
            "{projection:?}: the backdrop disappeared after a wheel"
        );
        for (x, y) in &backdrop {
            assert_eq!(
                frame.hit_at(*x, *y).definition(),
                PickId::NOTHING,
                "{projection:?}: the grid became something to click at ({x}, {y})"
            );
            assert_eq!(
                frame.hit_at(*x, *y).face(),
                FacePickId::NOTHING,
                "{projection:?}: the grid became a face at ({x}, {y})"
            );
        }
    }
}
