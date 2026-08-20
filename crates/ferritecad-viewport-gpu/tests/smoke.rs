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
    Camera, FacePickId, Hovered, Marked, PickId, Projection, RenderSnapshot, SnapshotBuilder,
    StandardView, Visibility,
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
        topological_vertices: None,
        positions: positions.into_iter().flatten().collect(),
        normals: [normal; 4].into_iter().flatten().collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
        faces: vec![MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
            first_index: 0,
            index_count: 6,
        }],
        edges: None,
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
        topological_vertices: None,
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
        edges: None,
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
        topological_vertices: None,
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
        edges: None,
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
    let (r, g, b) = (u32::from(r), u32::from(g), u32::from(b));
    let lit = r + g + b > 30;
    // Widened before comparing: a boundary drawn in white would overflow the
    // eight-bit addition this used to do.
    // The model in these tests is blue, and no grid colour is.
    lit && !(b > r + 40 && b > g + 40)
}

/// Whether the picture shows a surface here, rather than a boundary drawn
/// over one.
///
/// A gate about what a part looks like when it is chosen, pointed at or left
/// alone is a gate about its surface. Its boundary is drawn in ink taken to
/// the end of the range, so two fills on the same side of that range are
/// inked the same and the boundary says nothing about marking. That is a
/// property of linework, gated where linework is gated, and excluded here.
fn shows_surface(frame: &ferritecad_viewport_gpu::Frame, x: u32, y: u32) -> bool {
    frame
        .colour_at(x, y)
        .is_some_and(|pixel| !is_boundary_ink(pixel))
}

/// Whether this pixel is the ink a face boundary is drawn in.
///
/// The shader takes it to whichever end of the range is further from what the
/// surface shows, so it is pure black or pure white and no shaded surface in
/// these fixtures is either. Gates about what a surface looks like exclude
/// these: a boundary is a different statement about the picture, drawn over
/// the surface rather than by it.
fn is_boundary_ink(pixel: [u8; 4]) -> bool {
    pixel[0..3] == [0, 0, 0] || pixel[0..3] == [255, 255, 255]
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
            Hovered::Nothing,
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
                // Where the model draws its own boundary it is ink rather than
                // material, which is a statement about the model and not about
                // the grid.
                assert!(
                    is_boundary_ink(pixel) || (pixel[2] > pixel[0] && pixel[2] > pixel[1]),
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let second = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
        .filter(|(x, y)| {
            snapshot.definition(plain.pick_at(*x, *y)) == Some(0) && shows_surface(&plain, *x, *y)
        })
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let places = |definition: usize| -> Vec<(u32, u32)> {
        (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .filter(|(x, y)| {
                snapshot.definition(plain.pick_at(*x, *y)) == Some(definition)
                    && shows_surface(&plain, *x, *y)
            })
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
            Hovered::Definition(hovered),
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let pixel_of = |definition: usize| {
        (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .find(|(x, y)| {
                snapshot.definition(plain.pick_at(*x, *y)) == Some(definition)
                    && shows_surface(&plain, *x, *y)
            })
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let pointed_only = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Definition(a),
            &Visibility::default(),
        )
        .expect("draws");
    let both = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(a),
            Hovered::Definition(b),
            &Visibility::default(),
        )
        .expect("draws");
    let same = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(a),
            Hovered::Definition(a),
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
                Hovered::Nothing,
                &Visibility::default(),
            )
            .expect("draws");
        // Well inside the quad. A white part's surface is the same colour as
        // the ink of a black one, so which pixels are boundary cannot be told
        // from colour here; where the boundary is can be, because it is the
        // rim.
        let (x, y, pick) = (0..plain.height())
            .flat_map(|y| (0..plain.width()).map(move |x| (x, y)))
            .map(|(x, y)| (x, y, plain.pick_at(x, y)))
            .find(|(x, y, pick)| {
                snapshot.definition(*pick).is_some()
                    && (x.saturating_sub(3)..=(x + 3).min(plain.width() - 1)).all(|nx| {
                        (y.saturating_sub(3)..=(y + 3).min(plain.height() - 1))
                            .all(|ny| plain.pick_at(nx, ny) == *pick)
                    })
            })
            .expect("the quad is drawn");

        let chosen = renderer
            .render(
                &prepared,
                &camera,
                Marked::Definition(pick),
                Hovered::Nothing,
                &Visibility::default(),
            )
            .expect("draws selection");
        let pointed = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Definition(pick),
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
                Hovered::Definition(hovered),
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
            Hovered::Nothing,
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
            Hovered::Definition(hovered),
            &Visibility::default(),
        )
        .expect("draws");
    let again = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Definition(hovered),
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
                Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let baked = renderer
        .render(
            &baked_prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
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
            topological_vertices: None,
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            faces: Vec::new(),
            edges: None,
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
                Hovered::Nothing,
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let second = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
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
                Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let drawn = pixels_of(&plain, |frame, x, y| {
        frame.pick_at(x, y) != PickId::NOTHING && shows_surface(frame, x, y)
    });
    let (fx, fy) = drawn
        .iter()
        .copied()
        .find(|(x, y)| snapshot.definition(plain.pick_at(*x, *y)) == Some(0))
        .expect("the plate is drawn");
    let face = plain.hit_at(fx, fy).face();
    let marked = pixels_of(&plain, |frame, x, y| {
        frame.hit_at(x, y).face() == face && shows_surface(frame, x, y)
    });
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
            Hovered::Face(face),
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let plate = pixels_of(&plain, |frame, x, y| {
        snapshot.definition(frame.pick_at(x, y)) == Some(0) && shows_surface(frame, x, y)
    });
    let pick = plain.pick_at(plate[0].0, plate[0].1);
    let pointed = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Definition(pick),
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let both = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(chosen),
            Hovered::Face(face),
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Face(foreign),
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
            Hovered::Nothing,
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
            Hovered::Face(face),
            &Visibility::default(),
        )
        .expect("draws");
    let second = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Face(face),
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let drawn = pixels_of(&plain, |frame, x, y| {
        frame.pick_at(x, y) != PickId::NOTHING && shows_surface(frame, x, y)
    });
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let (x, y) = pixels_of(&plain, |frame, x, y| {
        snapshot.definition(frame.pick_at(x, y)) == Some(0) && shows_surface(frame, x, y)
    })[0];
    let face = plain.hit_at(x, y).face();
    let definition = plain.pick_at(x, y);

    let as_face = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let as_definition = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(definition),
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let pointed = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Face(face),
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    for hovered in [
        Hovered::Face(face),
        Hovered::Face(other_face),
        Hovered::Definition(definition),
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");
    let second = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            topological_vertices: None,
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
            edges: None,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");

    // Choosing or pointing at something that is not drawn tints nothing: it
    // has no pixels to tint, which is the whole of why hiding is omission.
    for (selected, hovered) in [
        (Marked::Definition(front), Hovered::Nothing),
        (Marked::Face(face), Hovered::Nothing),
        (Marked::Nothing, Hovered::Definition(front)),
        (Marked::Nothing, Hovered::Face(face)),
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws");
        let hidden_twice = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
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
                Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");
    // What is chosen still looks chosen after everything else has gone.
    let as_definition = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(keep),
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");
    let as_face = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");
    assert_ne!(as_definition.colour(), plain.colour());
    assert_ne!(as_face.colour(), plain.colour());
    assert_ne!(as_face.colour(), as_definition.colour());

    // And nothing that was removed can be marked, chosen or pointed at: it has
    // no pixels to mark.
    for (selected, hovered) in [
        (Marked::Definition(gone), Hovered::Nothing),
        (Marked::Face(gone_face), Hovered::Nothing),
        (Marked::Nothing, Hovered::Definition(gone)),
        (Marked::Nothing, Hovered::Face(gone_face)),
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
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");
    let twice = renderer
        .render(
            &prepared,
            &three_camera,
            Marked::Nothing,
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");
    let face_before = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");

    assert!(visibility.show(Marked::Definition(returning), &snapshot));
    let chosen_after = renderer
        .render(
            &prepared,
            &camera,
            Marked::Definition(kept),
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");
    let face_after = renderer
        .render(
            &prepared,
            &camera,
            Marked::Face(face),
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");

    // What was chosen looks exactly as it did, wherever it is drawn.
    let plain = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
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
        (Marked::Definition(staying_hidden), Hovered::Nothing),
        (Marked::Nothing, Hovered::Definition(staying_hidden)),
        (
            Marked::Nothing,
            Hovered::Face(snapshot.face_of(2, 0).expect("numbered")),
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            topological_vertices: None,
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
            edges: None,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            Hovered::Nothing,
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
            topological_vertices: None,
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
            edges: None,
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
                    Hovered::Nothing,
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
                Hovered::Nothing,
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

/// Where the camera says a world point lands, in pixels from the top left.
///
/// A rotation gate that only compared the picture with itself would be blind
/// to a constant extra turn applied somewhere else, because turns in a plane
/// commute. Measuring against the one matrix is what closes that.
fn where_the_camera_puts(camera: &Camera, point: [f32; 3]) -> (f32, f32) {
    let m = camera.view_projection();
    let clip = [
        m[0] * point[0] + m[4] * point[1] + m[8] * point[2] + m[12],
        m[1] * point[0] + m[5] * point[1] + m[9] * point[2] + m[13],
        m[3] * point[0] + m[7] * point[1] + m[11] * point[2] + m[15],
    ];
    assert!(clip[2] > 0.0, "a point behind the eye has no pixel");
    (
        (clip[0] / clip[2] + 1.0) * 0.5 * camera.width() as f32,
        (1.0 - clip[1] / clip[2]) * 0.5 * camera.height() as f32,
    )
}

#[test]
fn turning_the_view_turns_the_model_around_what_it_is_looking_at() {
    let mut renderer = renderer_or_skip!();
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let (snapshot, mut camera) = plates_in_the_target_plane(256, 256);
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to turn in"
        );
        let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
        let uploaded = renderer.geometry_uploads();
        let everything = Visibility::default();
        let marker = snapshot.pick_of(1).expect("drawn");
        let marker_face = snapshot.face_of(1, 0).expect("numbered");
        let nearer = snapshot.pick_of(0).expect("drawn");

        let draw = |renderer: &mut Renderer, camera: &Camera| {
            renderer
                .render(
                    &prepared,
                    camera,
                    Marked::Nothing,
                    Hovered::Nothing,
                    &everything,
                )
                .expect("draws")
        };

        let before = draw(&mut renderer, &camera);
        let (was_x, was_y) = centre_of(&before, marker).expect("the marker is on screen");
        // The marker's own middle, as this fixture places it.
        let marker_centre = [26.0, 0.0, 17.0];
        let said = where_the_camera_puts(&camera, marker_centre);
        assert!(
            (was_x - said.0).abs() <= 1.5 && (was_y - said.1).abs() <= 1.5,
            "{projection:?}: the marker is drawn at ({was_x}, {was_y}) where the camera \
             says {said:?}"
        );
        let colour = before
            .colour_at(was_x as u32, was_y as u32)
            .expect("the marker has a colour");
        let covered = covered_by(&before, marker);
        // The marker is up and to the right of the middle in this fixture.
        assert!(
            was_x - 128.0 > 20.0 && 128.0 - was_y > 10.0,
            "{projection:?}: the marker did not start where the gate expects: ({was_x}, {was_y})"
        );

        // A quarter turn counterclockwise: what was to the right goes up, and
        // what was above goes to the left.
        camera.roll(std::f32::consts::FRAC_PI_2);
        let after = draw(&mut renderer, &camera);

        let (now_x, now_y) = centre_of(&after, marker).expect("the marker left the view");
        let (dx, dy) = (was_x - 128.0, 128.0 - was_y);
        let (expected_x, expected_y) = (128.0 - dy, 128.0 - dx);
        assert!(
            (now_x - expected_x).abs() <= 2.0 && (now_y - expected_y).abs() <= 2.0,
            "{projection:?}: a quarter turn moved the marker from ({was_x}, {was_y}) to \
             ({now_x}, {now_y}) where it belongs at ({expected_x}, {expected_y})"
        );

        let said = where_the_camera_puts(&camera, marker_centre);
        assert!(
            (now_x - said.0).abs() <= 1.5 && (now_y - said.1).abs() <= 1.5,
            "{projection:?}: after turning, the marker is drawn at ({now_x}, {now_y}) \
             where the camera says {said:?}"
        );

        // It is the same part, the same face and the same colour: turning the
        // view is not a change to the model.
        assert_eq!(
            after.pick_at(now_x as u32, now_y as u32),
            marker,
            "{projection:?}: the marker changed hands"
        );
        assert_eq!(
            after.hit_at(now_x as u32, now_y as u32).face(),
            marker_face,
            "{projection:?}: the marker changed which face it shows"
        );
        assert_eq!(
            after.colour_at(now_x as u32, now_y as u32),
            Some(colour),
            "{projection:?}: turning the view repainted the model"
        );
        let now_covered = covered_by(&after, marker);
        assert!(
            now_covered.abs_diff(covered) * 20 < covered,
            "{projection:?}: the marker changed size from {covered} to {now_covered}"
        );

        // What is in front is still in front.
        assert!(
            covered_by(&after, nearer) > 0,
            "{projection:?}: the nearer plate vanished behind the one it is in front of"
        );

        // The backdrop turned with the world rather than staying with the
        // screen, and is still nobody to click.
        let backdrop = pixels_of(&after, |frame, x, y| {
            frame.pick_at(x, y) == PickId::NOTHING
                && frame
                    .colour_at(x, y)
                    .is_some_and(|colour| colour != [0, 0, 0, 255])
        });
        assert!(
            !backdrop.is_empty(),
            "{projection:?}: the backdrop disappeared after a turn"
        );
        for (x, y) in &backdrop {
            assert_eq!(
                after.hit_at(*x, *y).definition(),
                PickId::NOTHING,
                "{projection:?}: the grid became something to click at ({x}, {y})"
            );
            assert_eq!(
                after.hit_at(*x, *y).face(),
                FacePickId::NOTHING,
                "{projection:?}: the grid became a face at ({x}, {y})"
            );
        }
        let (changed, common) = changed_common_background(&before, &after);
        assert!(
            changed * 5 > common,
            "{projection:?}: the backdrop stayed with the screen: {changed} of {common} moved"
        );

        // Turning the view is a camera change and nothing else.
        assert_eq!(
            renderer.geometry_uploads(),
            uploaded,
            "{projection:?}: turning uploaded geometry"
        );
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

/// Light plates, so that the ink a boundary is drawn in is black and nothing
/// else in the picture is.
///
/// One definition placed twice, and a smaller one tucked entirely behind the
/// left placement of it. That covers what linework has to answer for: it is
/// drawn for every placement, it stops where a face stops rather than along a
/// triangulation seam, and a boundary behind something is not drawn through
/// it.
fn light_plates(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    plates_coloured(width, height, [0.75, 0.75, 0.75])
}

/// The same plates in a chosen material.
///
/// A dark one inks in white, which is the only way to see linework drawn for a
/// part that is not: black ink on the cleared background is black on black.
fn plates_coloured(width: u32, height: u32, colour: [f64; 3]) -> (Arc<RenderSnapshot>, Camera) {
    let plate = |shape: u64, half: f32, y: f32| {
        let handle = ShapeHandle::new(SessionId::new(), shape);
        Mesh {
            topological_vertices: None,
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
            edges: None,
        }
    };

    let mut builder = SnapshotBuilder::new();
    let front = builder.add_mesh(&plate(1, 8.0, 0.0)).expect("packs");
    // Behind the left placement, and smaller than it in every direction.
    let behind = builder.add_mesh(&plate(2, 3.0, 12.0)).expect("packs");
    let at = |x: f32| {
        Transform::from_translation(
            ferritecad_types::Vec3::new(x as f64, 0.0, 0.0).expect("finite"),
        )
        .expect("finite")
    };
    for x in [-20.0, 20.0] {
        builder.place(front, None, &at(x), colour).expect("places");
    }
    builder
        .place(behind, None, &at(-20.0), colour)
        .expect("places");
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("the plates have an extent"))
        .expect("frames");
    (snapshot, camera)
}

/// Whether this pixel is drawn in the ink a light material's boundary uses.
///
/// The shader takes the ink to the far end of the range from what the face is
/// showing, so on an unmarked light plate it is black and no shaded surface of
/// one comes near it.
fn is_ink(pixel: [u8; 4]) -> bool {
    let luminance =
        0.2126 * f32::from(pixel[0]) + 0.7152 * f32::from(pixel[1]) + 0.0722 * f32::from(pixel[2]);
    luminance < 60.0 && pixel[3] > 0
}

/// Every pixel the model drew in boundary ink.
///
/// The cleared background is black as well, so being black is not enough, and
/// neither is being black beside the model: the background just outside a
/// plate's silhouette is both, and counting it would let a placement with no
/// linework at all look as though it had some. What counts is ink over a pixel
/// the model owns.
fn ink_pixels(frame: &ferritecad_viewport_gpu::Frame) -> Vec<(u32, u32)> {
    pixels_of(frame, |frame, x, y| {
        frame.pick_at(x, y) != PickId::NOTHING && frame.colour_at(x, y).is_some_and(is_ink)
    })
}

#[test]
fn a_face_is_drawn_with_the_edges_it_stops_at_and_not_its_own_seams() {
    let mut renderer = renderer_or_skip!();
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let (snapshot, mut camera) = light_plates(300, 300);
        assert!(
            projection == Projection::Perspective || camera.set_projection(projection),
            "the camera refused a projection to draw in"
        );
        let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
        let uploaded = renderer.geometry_uploads();
        let everything = Visibility::default();
        let front = snapshot.pick_of(0).expect("drawn");

        let frame = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &everything,
            )
            .expect("draws");

        let ink = ink_pixels(&frame);
        assert!(
            ink.len() > 60,
            "{projection:?}: the plates have no boundary drawn: {} pixels",
            ink.len()
        );

        // Each square plate is bounded on four sides and crossed by one
        // diagonal seam. Take the left placement alone and require its ink to
        // lie at its rim, which is where a boundary is and where a drawn
        // diagonal would not be.
        let mine = pixels_of(&frame, |frame, x, y| {
            frame.pick_at(x, y) == front && x < 150
        });
        assert!(
            !mine.is_empty(),
            "{projection:?}: the left plate is missing"
        );
        let (min_x, max_x) = (
            mine.iter().map(|(x, _)| *x).min().expect("drawn"),
            mine.iter().map(|(x, _)| *x).max().expect("drawn"),
        );
        let (min_y, max_y) = (
            mine.iter().map(|(_, y)| *y).min().expect("drawn"),
            mine.iter().map(|(_, y)| *y).max().expect("drawn"),
        );
        let inside: Vec<(u32, u32)> = ink
            .iter()
            .copied()
            .filter(|(x, y)| *x > min_x + 4 && *x < max_x - 4 && *y > min_y + 4 && *y < max_y - 4)
            .collect();
        assert!(
            inside.is_empty(),
            "{projection:?}: {} ink pixels are inside the face rather than at its edge, \
             for example {:?}; the plate covers x {min_x}..{max_x} and y {min_y}..{max_y}",
            inside.len(),
            inside.first()
        );

        // A line says nothing about what a pixel is: the face beneath it still
        // answers for itself.
        for (x, y) in ink.iter().filter(|(x, _)| *x < 150) {
            let hit = frame.hit_at(*x, *y);
            if hit.definition() == front {
                assert_eq!(
                    hit.face(),
                    snapshot.face_of(0, 0).expect("numbered"),
                    "{projection:?}: a line changed which face ({x}, {y}) belongs to"
                );
            }
        }

        // Drawing lines is not a change to the model, and the same camera
        // draws the same picture.
        assert_eq!(
            renderer.geometry_uploads(),
            uploaded,
            "{projection:?}: drawing boundaries uploaded geometry"
        );
        let again = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &everything,
            )
            .expect("draws");
        assert_eq!(
            frame.colour(),
            again.colour(),
            "{projection:?}: the same camera drew two different pictures"
        );

        // And the lines follow the one camera.
        camera.roll(std::f32::consts::FRAC_PI_2);
        let turned = renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &everything,
            )
            .expect("draws");
        let moved = ink_pixels(&turned);
        assert!(
            moved.len() > 60,
            "{projection:?}: the boundary vanished when the view turned"
        );
        assert_ne!(ink, moved, "{projection:?}: the boundary stayed on screen");
    }
}

#[test]
fn every_placement_of_a_definition_is_drawn_with_its_own_lines() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = light_plates(300, 300);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let frame = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    let ink = ink_pixels(&frame);
    let left = ink.iter().filter(|(x, _)| *x < 150).count();
    let right = ink.iter().filter(|(x, _)| *x >= 150).count();
    assert!(
        left > 30 && right > 30,
        "one placement of a definition was drawn without its boundary: {left} and {right}"
    );
    // The same definition, so the same boundary: within a few pixels of
    // rasterisation, the two placements are drawn with as much line as each
    // other.
    assert!(
        left.abs_diff(right) * 5 < left,
        "two placements of one definition were drawn with different linework: \
         {left} and {right}"
    );
}

#[test]
fn a_boundary_behind_another_part_is_not_drawn_through_it() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = light_plates(300, 300);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let everything = Visibility::new(&snapshot);
    let front = snapshot.pick_of(0).expect("drawn");
    let behind = snapshot.pick_of(1).expect("drawn");

    let frame = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &everything,
        )
        .expect("draws");

    // Nothing of the smaller plate is visible at all: it is entirely behind
    // the left placement of the larger one.
    assert!(
        pixels_of(&frame, |frame, x, y| frame.pick_at(x, y) == behind).is_empty(),
        "the fixture no longer hides the smaller plate"
    );
    // So no ink of it may appear either. Every ink pixel over the left plate
    // is at that plate's own rim.
    let over_the_front = pixels_of(&frame, |frame, x, y| {
        frame.pick_at(x, y) == front && frame.colour_at(x, y).is_some_and(is_ink)
    });
    let mine = pixels_of(&frame, |frame, x, y| {
        frame.pick_at(x, y) == front && x < 150
    });
    let (min_x, max_x) = (
        mine.iter().map(|(x, _)| *x).min().expect("drawn"),
        mine.iter().map(|(x, _)| *x).max().expect("drawn"),
    );
    let (min_y, max_y) = (
        mine.iter().map(|(_, y)| *y).min().expect("drawn"),
        mine.iter().map(|(_, y)| *y).max().expect("drawn"),
    );
    let through = over_the_front
        .iter()
        .filter(|(x, y)| {
            *x < 150 && *x > min_x + 4 && *x < max_x - 4 && *y > min_y + 4 && *y < max_y - 4
        })
        .count();
    assert_eq!(
        through, 0,
        "a boundary behind the left plate was drawn through it at {through} pixels"
    );

    // And it is there to be drawn once what covered it is out of the way.
    let mut hiding = everything.clone();
    assert!(hiding.hide(Marked::Definition(front), &snapshot));
    let revealed = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &hiding,
        )
        .expect("draws");
    assert!(
        !pixels_of(&revealed, |frame, x, y| {
            frame.pick_at(x, y) == behind && frame.colour_at(x, y).is_some_and(is_ink)
        })
        .is_empty(),
        "the plate behind has no boundary of its own"
    );
}

#[test]
fn hiding_a_part_takes_its_lines_with_it_and_taking_it_back_restores_them() {
    let mut renderer = renderer_or_skip!();
    // Dark, so its linework is white and shows against the cleared background:
    // a light part's ink is black, and lines drawn for a part that is not on
    // screen would be black on black and impossible to see.
    let (snapshot, camera) = plates_coloured(300, 300, [0.08, 0.08, 0.08]);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let front = snapshot.pick_of(0).expect("drawn");
    let draw = |renderer: &mut Renderer, visibility: &Visibility| {
        renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                visibility,
            )
            .expect("draws")
    };

    let everything = Visibility::new(&snapshot);
    let shown = draw(&mut renderer, &everything);
    // Where the two placements of the front definition were drawn, and how
    // much of that was its own linework.
    let was_drawn = pixels_of(&shown, |frame, x, y| frame.pick_at(x, y) == front);
    assert!(
        !ink_pixels(&shown).is_empty(),
        "the picture had no lines to lose"
    );

    let mut hiding = everything.clone();
    assert!(hiding.hide(Marked::Definition(front), &snapshot));
    let hidden = draw(&mut renderer, &hiding);
    assert!(
        pixels_of(&hidden, |frame, x, y| frame.pick_at(x, y) == front).is_empty(),
        "a hidden part kept its fill"
    );
    // And nothing of it is drawn where it used to be. The backdrop shows
    // through there, and the grid draws greys, so what must not appear is the
    // one thing only this part could have drawn: the white its dark material
    // inks in.
    assert!(
        !ink_pixels(&shown).is_empty() && shown.colour_at(was_drawn[0].0, was_drawn[0].1).is_some(),
        "the gate lost track of where the part was"
    );
    // Away from the plate that has just been revealed there, whose own
    // boundary is drawn in the same white and belongs on screen.
    let left_behind = was_drawn
        .iter()
        .filter(|(x, y)| {
            hidden.colour_at(*x, *y) == Some([255, 255, 255, 255])
                && !(x.saturating_sub(3)..=(x + 3).min(hidden.width() - 1)).any(|nx| {
                    (y.saturating_sub(3)..=(y + 3).min(hidden.height() - 1))
                        .any(|ny| hidden.pick_at(nx, ny) != PickId::NOTHING)
                })
        })
        .count();
    assert_eq!(
        left_behind, 0,
        "a hidden part kept its lines at {left_behind} pixels it used to cover"
    );

    // Isolating is the same rule reached another way.
    let mut isolating = everything.clone();
    assert!(isolating.isolate(Marked::Definition(front), &snapshot));
    let isolated = draw(&mut renderer, &isolating);
    assert!(
        !ink_pixels(&isolated).is_empty(),
        "isolating a part lost its lines"
    );

    // And taking the change back draws the picture it left, to the byte.
    let mut undone = hiding.clone();
    assert!(undone.undo(&snapshot));
    let restored = draw(&mut renderer, &undone);
    assert_eq!(
        shown.colour(),
        restored.colour(),
        "taking a hide back did not draw the picture it left"
    );
}

#[test]
fn an_empty_picture_still_draws_nothing_at_all() {
    let mut renderer = renderer_or_skip!();
    let snapshot = Arc::new(SnapshotBuilder::new().build());
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let mut camera = Camera::new();
    camera.resize(64, 64);

    let frame = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::default(),
        )
        .expect("draws");

    for y in 0..frame.height() {
        for x in 0..frame.width() {
            assert_eq!(frame.pick_at(x, y), PickId::NOTHING);
            assert_eq!(frame.hit_at(x, y).face(), FacePickId::NOTHING);
        }
    }
}

#[test]
fn the_backdrop_draws_no_linework_of_its_own() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = model_over_the_plane(200, 200);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let draw = |renderer: &mut Renderer, prepared: &_, visibility: &Visibility| {
        renderer
            .render(
                prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                visibility,
            )
            .expect("draws")
    };

    // A dim grid line and a dark boundary are both dark, so which pixels are
    // ink cannot be told from colour alone here, and an empty picture draws no
    // grid at all to compare against. What is stated instead is the thing this
    // gate is for: with everything hidden, the picture is the picture of a
    // model that is not drawn, and drawing lines added nothing to it.
    let mut hiding = Visibility::new(&snapshot);
    for definition in 0..snapshot.meshes().len() {
        if let Some(pick) = snapshot.pick_of(definition) {
            hiding.hide(Marked::Definition(pick), &snapshot);
        }
    }
    let backdrop = draw(&mut renderer, &prepared, &hiding);
    assert!(
        pixels_of(&backdrop, |frame, x, y| frame.pick_at(x, y)
            != PickId::NOTHING)
        .is_empty(),
        "hiding everything left something drawn"
    );
    // And with the model there, the backdrop is still nobody to click.
    let shown = draw(&mut renderer, &prepared, &Visibility::default());
    assert_ne!(
        shown.colour(),
        backdrop.colour(),
        "the gate compared two identical pictures"
    );
    for (x, y) in pixels_of(&shown, |frame, x, y| frame.pick_at(x, y) == PickId::NOTHING) {
        assert_eq!(shown.hit_at(x, y).definition(), PickId::NOTHING);
        assert_eq!(shown.hit_at(x, y).face(), FacePickId::NOTHING);
    }
}

#[test]
fn linework_leaves_a_choice_and_a_question_telling_themselves_apart() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = two_faced_scene(200, 200);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("uploads");
    let everything = Visibility::default();
    let face = snapshot.face_of(0, 0).expect("numbered");

    let draw = |renderer: &mut Renderer, selected: Marked, hovered: Hovered| {
        renderer
            .render(&prepared, &camera, selected, hovered, &everything)
            .expect("draws")
    };

    let plain = draw(&mut renderer, Marked::Nothing, Hovered::Nothing);
    let chosen = draw(&mut renderer, Marked::Face(face), Hovered::Nothing);
    let asked = draw(&mut renderer, Marked::Nothing, Hovered::Face(face));

    // A pixel of that face, away from any line, in all three pictures.
    let plain_face = pixels_of(&plain, |frame, x, y| {
        frame.hit_at(x, y).face() == face && shows_surface(frame, x, y)
    });
    let (x, y) = *plain_face.first().expect("the face is on screen");
    let colours = [
        plain.colour_at(x, y).expect("drawn"),
        chosen.colour_at(x, y).expect("drawn"),
        asked.colour_at(x, y).expect("drawn"),
    ];
    assert_ne!(colours[0], colours[1], "a chosen face looks unchosen");
    assert_ne!(
        colours[0], colours[2],
        "a face nobody asked about is marked"
    );
    assert_ne!(
        colours[1], colours[2],
        "a choice and a question look the same"
    );

    // And the lines are still there when a face is chosen: linework and
    // marking are two different statements about the same face.
    assert!(
        !pixels_of(&chosen, |frame, x, y| {
            frame.hit_at(x, y).face() == face && frame.colour_at(x, y).is_some_and(is_ink)
        })
        .is_empty(),
        "choosing a face erased its boundary"
    );
}

// ---------------------------------------------------------------------------
// Topological edge identities, on a device.
// ---------------------------------------------------------------------------

/// Five points of one arc, as a face that meshed it finely would place them.
const FINE_ARC: [[f32; 3]; 5] = [
    [-8.66, 0.0, -1.0],
    [-5.0, 0.0, 2.66],
    [0.0, 0.0, 4.0],
    [5.0, 0.0, 2.66],
    [8.66, 0.0, -1.0],
];

/// Three points of the same arc, as a neighbouring face that meshed it coarsely
/// would place them: the first, the middle and the last of the five.
const COARSE_ARC: [[f32; 3]; 3] = [FINE_ARC[0], FINE_ARC[2], FINE_ARC[4]];

/// Two faces meeting along one curved topological edge, each with its own
/// vertices and its own approximation of that edge.
///
/// This is the shape of a real tessellation and not a convenience. Two faces
/// never share a vertex, so each keeps its own nodes; and two faces meshed to
/// different fineness approximate the curve they share with different chords,
/// which is what makes "one edge, two face-side representations" a statement
/// with observable content rather than two names for the same pixels.
///
/// Vertices 0..4 are the fine arc and 5 is the apex below it; 6..8 are the
/// coarse arc and 9 the apex above. The shared edge owns six segments, four
/// from the lower face and two from the upper one; the four remaining edges
/// own one each.
fn two_faces_sharing_a_curve() -> Mesh {
    let shape = ShapeHandle::new(SessionId::new(), 7);
    let mut positions: Vec<f32> = Vec::new();
    for point in FINE_ARC {
        positions.extend_from_slice(&point);
    }
    positions.extend_from_slice(&[0.0, 0.0, -16.0]);
    for point in COARSE_ARC {
        positions.extend_from_slice(&point);
    }
    positions.extend_from_slice(&[0.0, 0.0, 14.0]);

    let edge =
        |index: u64, first_segment: u32, segment_count: u32| ferritecad_kernel::MeshEdgeRange {
            edge: SubShapeHandle::new(shape, SubShapeKind::Edge, index),
            first_segment,
            segment_count,
        };

    Mesh {
        topological_vertices: None,
        positions,
        normals: [[0.0f32, -1.0, 0.0]; 10].into_iter().flatten().collect(),
        // A fan of four triangles below the curve, and one of two above it.
        indices: vec![5, 0, 1, 5, 1, 2, 5, 2, 3, 5, 3, 4, 9, 6, 7, 9, 7, 8],
        faces: vec![
            MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 12,
            },
            MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Face, 1),
                first_index: 12,
                index_count: 6,
            },
        ],
        edges: Some(ferritecad_kernel::MeshEdges {
            segments: vec![
                // The shared curve: the lower face's four chords, then the
                // upper face's two, under one identity.
                0, 1, 1, 2, 2, 3, 3, 4, 6, 7, 7, 8, //
                // The four edges that belong to one face each.
                5, 0, // lower left
                4, 5, // lower right
                9, 6, // upper left
                8, 9, // upper right
            ],
            ranges: vec![
                edge(0, 0, 6),
                edge(1, 6, 1),
                edge(2, 7, 1),
                edge(3, 8, 1),
                edge(4, 9, 1),
            ],
        }),
    }
}

/// That definition placed twice, side by side.
fn curved_pair(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let mut builder = SnapshotBuilder::new();
    let definition = builder
        .add_mesh(&two_faces_sharing_a_curve())
        .expect("packs");
    for x in [-24.0, 24.0] {
        builder
            .place(definition, None, &moved(x, 0.0, 0.0), [0.2, 0.6, 0.2])
            .expect("places");
    }
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("something is drawn"))
        .expect("frames");
    (snapshot, camera)
}

/// Where a world point lands, through the very matrix the frame was drawn with.
///
/// No second projection: the camera is asked for the matrix it gave the shader,
/// so a probe cannot disagree with the picture about where something is.
fn pixel_of(camera: &Camera, world: [f64; 3]) -> Option<(u32, u32)> {
    let m = camera.view_projection();
    let mut clip = [0.0f64; 4];
    for (row, value) in clip.iter_mut().enumerate() {
        *value = f64::from(m[row]) * world[0]
            + f64::from(m[4 + row]) * world[1]
            + f64::from(m[8 + row]) * world[2]
            + f64::from(m[12 + row]);
    }
    if clip[3].abs() < 1e-9 {
        return None;
    }
    let ndc = [clip[0] / clip[3], clip[1] / clip[3]];
    let x = (ndc[0] * 0.5 + 0.5) * f64::from(camera.width());
    // Clip space has +y up and a frame's rows begin at the top.
    let y = (0.5 - ndc[1] * 0.5) * f64::from(camera.height());
    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
        return None;
    }
    let (x, y) = (x as u32, y as u32);
    (x < camera.width() && y < camera.height()).then_some((x, y))
}

/// The edge identity at a probe, allowing for where a line lands on a grid.
///
/// A rasterised line passes within a pixel of the geometry it came from, not
/// exactly through the pixel arithmetic says. Anything further than this is a
/// different line: the probes below are placed several pixels clear of every
/// other edge, and the distances are stated where they are chosen.
fn edge_near(frame: &Frame, at: (u32, u32)) -> ferritecad_viewport::EdgePickId {
    const REACH: i64 = 2;
    let mut found = ferritecad_viewport::EdgePickId::NOTHING;
    for dy in -REACH..=REACH {
        for dx in -REACH..=REACH {
            let (x, y) = (at.0 as i64 + dx, at.1 as i64 + dy);
            if x < 0 || y < 0 {
                continue;
            }
            let edge = frame.edge_at(x as u32, y as u32);
            if edge != ferritecad_viewport::EdgePickId::NOTHING {
                found = edge;
            }
        }
    }
    found
}

/// The midpoint of two world points.
fn between(a: [f32; 3], b: [f32; 3]) -> [f64; 3] {
    [
        f64::from(a[0] + b[0]) / 2.0,
        f64::from(a[1] + b[1]) / 2.0,
        f64::from(a[2] + b[2]) / 2.0,
    ]
}

/// The same point, in the placement that is `shift` along x.
fn placed(point: [f64; 3], shift: f64) -> [f64; 3] {
    [point[0] + shift, point[1], point[2]]
}

#[test]
fn one_topological_edge_answers_with_one_identity_from_both_of_its_faces() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(320, 320);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let frame = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::new(&snapshot),
        )
        .expect("draws");

    let shared = snapshot
        .edge_of(0, 0)
        .expect("the picture numbers this edge");

    // A point on the lower face's approximation of the shared curve, and one
    // on the upper face's. The fine vertex sits 1.34 world units from the
    // coarse chord and the coarse midpoint 1.29 from the fine polyline, which
    // at this framing is about nine pixels: far outside the two-pixel reach
    // above, so neither probe can be answered by the other side's line.
    let on_the_lower_side = [
        f64::from(FINE_ARC[1][0]),
        f64::from(FINE_ARC[1][1]),
        f64::from(FINE_ARC[1][2]),
    ];
    let on_the_upper_side = between(COARSE_ARC[0], COARSE_ARC[1]);
    // A third, on a chord of the lower side that the coarse approximation
    // does not go near either.
    let further_along_the_lower_side = between(FINE_ARC[2], FINE_ARC[3]);

    for (shift, placement) in [(-24.0, "left"), (24.0, "right")] {
        for (what, world) in [
            ("the lower face's chord", on_the_lower_side),
            ("the upper face's chord", on_the_upper_side),
            (
                "a later chord of the lower face",
                further_along_the_lower_side,
            ),
        ] {
            let at = pixel_of(&camera, placed(world, shift)).expect("the probe is on screen");
            assert_eq!(
                edge_near(&frame, at),
                shared,
                "{what} of the {placement} placement, at {at:?}, did not answer the shared edge"
            );
        }
    }

    // The four edges that belong to one face each are four other identities.
    let others = [
        (1usize, between(FINE_ARC[0], [0.0, 0.0, -16.0])),
        (2, between(FINE_ARC[4], [0.0, 0.0, -16.0])),
        (3, between(COARSE_ARC[0], [0.0, 0.0, 14.0])),
        (4, between(COARSE_ARC[2], [0.0, 0.0, 14.0])),
    ];
    for (ordinal, world) in others {
        let expected = snapshot.edge_of(0, ordinal).expect("numbered");
        assert_ne!(expected, shared, "the picture reused an identity");
        let at = pixel_of(&camera, placed(world, -24.0)).expect("on screen");
        assert_eq!(
            edge_near(&frame, at),
            expected,
            "edge {ordinal} did not answer with its own identity"
        );
    }

    // Inside a face, and far from the model, there is no edge. The probe is
    // the centroid of one of the upper face's triangles, which is 1.66 world
    // units from the nearest edge of it and 2.89 from the shared curve.
    let inside = pixel_of(&camera, placed([-2.89, 0.0, 5.67], -24.0)).expect("on screen");
    assert_eq!(
        frame.edge_at(inside.0, inside.1),
        ferritecad_viewport::EdgePickId::NOTHING,
        "the inside of a face answered with an edge"
    );
    assert_eq!(
        frame.edge_at(2, 2),
        ferritecad_viewport::EdgePickId::NOTHING,
        "the background answered with an edge"
    );

    // What was already true of this pixel stays true: an edge target is one
    // more answer about a pixel, not a replacement for the two it had.
    let on_edge = pixel_of(&camera, placed(on_the_upper_side, -24.0)).expect("on screen");
    let hit = frame.hit_at(on_edge.0, on_edge.1);
    assert_eq!(
        hit.definition(),
        frame.pick_at(on_edge.0, on_edge.1),
        "the definition under an edge changed"
    );
    assert_ne!(
        hit.definition(),
        PickId::NOTHING,
        "the probe is meant to be on the model"
    );
}

/// The curved pair, with a plate in front of the left placement.
///
/// The plate is a definition with no edge association of its own, so every
/// edge identity in the picture belongs to the curved definition behind it.
fn curved_pair_behind_a_plate(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let shape = ShapeHandle::new(SessionId::new(), 11);
    let cover = Mesh {
        topological_vertices: None,
        positions: vec![
            -20.0, -8.0, -20.0, 8.0, -8.0, -20.0, 8.0, -8.0, 20.0, -20.0, -8.0, 20.0,
        ],
        normals: [[0.0f32, -1.0, 0.0]; 4].into_iter().flatten().collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
        faces: vec![MeshFaceRange {
            face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
            first_index: 0,
            index_count: 6,
        }],
        edges: None,
    };

    let mut builder = SnapshotBuilder::new();
    let curved = builder
        .add_mesh(&two_faces_sharing_a_curve())
        .expect("packs");
    let plate = builder.add_mesh(&cover).expect("packs");
    for x in [-24.0, 24.0] {
        builder
            .place(curved, None, &moved(x, 0.0, 0.0), [0.2, 0.6, 0.2])
            .expect("places");
    }
    // Nearer the eye than the left placement, and covering all of it.
    builder
        .place(plate, None, &moved(-24.0, 0.0, 0.0), [0.9, 0.3, 0.2])
        .expect("places");
    let snapshot = Arc::new(builder.build());

    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("something is drawn"))
        .expect("frames");
    (snapshot, camera)
}

/// How many pixels carry an edge identity while lying clear of the model.
///
/// The one measurement that says the edge pass and the fill agree about where
/// the model is: an edge is drawn from the same vertices through the same
/// matrices, so every pixel of one must touch the other. The outer silhouette
/// is why the reach is a pixel rather than none — a line legitimately lands on
/// a pixel the fill did not quite reach — and anything beyond that is the edge
/// pass drawing somewhere the model is not.
fn strays_from_the_model(frame: &Frame) -> usize {
    edge_pixels(frame)
        .into_iter()
        .filter(|(x, y)| {
            !(-1i64..=1).any(|dy| {
                (-1i64..=1).any(|dx| {
                    let (nx, ny) = (*x as i64 + dx, *y as i64 + dy);
                    nx >= 0 && ny >= 0 && frame.pick_at(nx as u32, ny as u32) != PickId::NOTHING
                })
            })
        })
        .count()
}

/// Every pixel that answers with some topological edge.
fn edge_pixels(frame: &Frame) -> Vec<(u32, u32)> {
    pixels_of(frame, |frame, x, y| {
        frame.edge_at(x, y) != ferritecad_viewport::EdgePickId::NOTHING
    })
}

#[test]
fn an_edge_behind_a_nearer_part_does_not_answer_through_it() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair_behind_a_plate(320, 320);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let visibility = Visibility::new(&snapshot);
    let frame = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");

    let shared = snapshot.edge_of(0, 0).expect("numbered");
    let on_the_curve = between(COARSE_ARC[0], COARSE_ARC[1]);

    // Covered by the plate in the left placement, and not in the right one.
    let hidden_at = pixel_of(&camera, placed(on_the_curve, -24.0)).expect("on screen");
    let shown_at = pixel_of(&camera, placed(on_the_curve, 24.0)).expect("on screen");
    assert_eq!(
        edge_near(&frame, shown_at),
        shared,
        "the placement in the open should answer with its edge"
    );
    assert_eq!(
        edge_near(&frame, hidden_at),
        ferritecad_viewport::EdgePickId::NOTHING,
        "an edge answered through the part covering it"
    );

    // Taking the cover away brings that edge back, at the same identity.
    let mut isolated = Visibility::new(&snapshot);
    assert!(
        isolated.hide(
            Marked::Definition(snapshot.pick_of(1).expect("the plate")),
            &snapshot
        ),
        "the plate should be hideable"
    );
    let uncovered = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &isolated,
        )
        .expect("draws");
    assert_eq!(
        edge_near(&uncovered, hidden_at),
        shared,
        "the edge did not come back when what covered it was hidden"
    );
}

#[test]
fn what_is_not_drawn_leaves_no_edge_identity_behind() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(320, 320);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let curved = snapshot.pick_of(0).expect("the only definition");

    let shown = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::new(&snapshot),
        )
        .expect("draws");
    assert!(
        !edge_pixels(&shown).is_empty(),
        "the picture should have edges to begin with"
    );

    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.hide(Marked::Definition(curved), &snapshot));
    let hidden = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");
    assert!(
        edge_pixels(&hidden).is_empty(),
        "a hidden definition still answered with its edges"
    );

    // Isolating the only definition that draws anything changes nothing, and
    // showing it again returns exactly the identities it had.
    assert!(visibility.show(Marked::Definition(curved), &snapshot));
    let again = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");
    assert_eq!(
        edge_pixels(&again),
        edge_pixels(&shown),
        "showing a definition again did not restore its edges exactly"
    );
    for (x, y) in edge_pixels(&again) {
        assert_eq!(
            again.edge_at(x, y),
            shown.edge_at(x, y),
            "the identity at {x},{y} changed across hide and show"
        );
    }
}

#[test]
fn isolating_one_of_two_definitions_keeps_only_its_edges() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair_behind_a_plate(320, 320);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");

    let mut visibility = Visibility::new(&snapshot);
    let plate = snapshot.pick_of(1).expect("the plate");
    assert!(visibility.isolate(Marked::Definition(plate), &snapshot));
    let alone = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");
    assert!(
        edge_pixels(&alone).is_empty(),
        "isolating a definition with no edge association left edges on screen"
    );

    assert!(visibility.show_all());
    let all = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &visibility,
        )
        .expect("draws");
    let shared = snapshot.edge_of(0, 0).expect("numbered");
    let at =
        pixel_of(&camera, placed(between(COARSE_ARC[0], COARSE_ARC[1]), 24.0)).expect("on screen");
    assert_eq!(
        edge_near(&all, at),
        shared,
        "showing everything again did not restore the edge identities"
    );
}

#[test]
fn an_edge_target_changes_no_colour_and_no_other_answer() {
    let mut renderer = renderer_or_skip!();

    // The same triangles twice: once with the kernel's edge association and
    // once without it. Only the edge target may differ.
    let with_edges = two_faces_sharing_a_curve();
    let without_edges = Mesh {
        edges: None,
        ..two_faces_sharing_a_curve()
    };

    let build = |mesh: &Mesh| {
        let mut builder = SnapshotBuilder::new();
        let definition = builder.add_mesh(mesh).expect("packs");
        for x in [-24.0, 24.0] {
            builder
                .place(definition, None, &moved(x, 0.0, 0.0), [0.2, 0.6, 0.2])
                .expect("places");
        }
        Arc::new(builder.build())
    };
    let named = build(&with_edges);
    let plain = build(&without_edges);

    let mut camera = Camera::new();
    camera.resize(288, 288);
    camera
        .frame(named.bounds().expect("something is drawn"))
        .expect("frames");

    let draw = |renderer: &mut Renderer, snapshot: &Arc<RenderSnapshot>| {
        let prepared = renderer.prepare(Arc::clone(snapshot)).expect("prepares");
        renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &Visibility::new(snapshot),
            )
            .expect("draws")
    };
    let named_frame = draw(&mut renderer, &named);
    let plain_frame = draw(&mut renderer, &plain);

    assert_eq!(
        named_frame.colour(),
        plain_frame.colour(),
        "drawing the edge identities changed the picture"
    );

    // The other two answers are the same as well, pixel for pixel. They are
    // read through the pictures that issued them, so they are compared as
    // definition indices rather than as identities of two different pictures.
    for y in 0..named_frame.height() {
        for x in 0..named_frame.width() {
            assert_eq!(
                named.definition(named_frame.pick_at(x, y)),
                plain.definition(plain_frame.pick_at(x, y)),
                "the definition at {x},{y} changed"
            );
            assert_eq!(
                named.definition_of_face(named_frame.hit_at(x, y).face()),
                plain.definition_of_face(plain_frame.hit_at(x, y).face()),
                "the face at {x},{y} changed"
            );
        }
    }

    // A definition with no association draws no edge geometry at all, so its
    // whole edge target stays as it was cleared.
    assert!(
        edge_pixels(&plain_frame).is_empty(),
        "a picture with no edge association answered with edges"
    );
    assert!(
        !edge_pixels(&named_frame).is_empty(),
        "a picture with an edge association answered with none"
    );
}

#[test]
fn a_proven_absence_of_edges_draws_no_edge_geometry_either() {
    let mut renderer = renderer_or_skip!();
    // Not "nothing is known about this definition's edges" but "this
    // definition has none": a different value, and the same picture.
    let mesh = Mesh {
        edges: Some(ferritecad_kernel::MeshEdges::default()),
        ..two_faces_sharing_a_curve()
    };
    let mut builder = SnapshotBuilder::new();
    let definition = builder.add_mesh(&mesh).expect("packs");
    builder
        .place(definition, None, &Transform::IDENTITY, [0.2, 0.6, 0.2])
        .expect("places");
    let snapshot = Arc::new(builder.build());
    assert_eq!(snapshot.edge_count(), 0);

    let mut camera = Camera::new();
    camera.resize(192, 192);
    camera
        .frame(snapshot.bounds().expect("drawn"))
        .expect("frames");
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let frame = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::new(&snapshot),
        )
        .expect("draws");

    assert!(
        edge_pixels(&frame).is_empty(),
        "a definition proven to have no edges answered with one"
    );
    // And an empty picture, which has neither meshes nor edges.
    let empty = Arc::new(SnapshotBuilder::new().build());
    let mut small = Camera::new();
    small.resize(32, 32);
    let prepared = renderer.prepare(Arc::clone(&empty)).expect("prepares");
    let frame = renderer
        .render(
            &prepared,
            &small,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::new(&empty),
        )
        .expect("draws");
    assert!(edge_pixels(&frame).is_empty());
}

#[test]
fn edges_cost_no_geometry_per_frame_and_repeat_exactly() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(256, 256);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let uploaded = renderer.geometry_uploads();
    let visibility = Visibility::new(&snapshot);

    let draw = |renderer: &mut Renderer| {
        renderer
            .render(
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            )
            .expect("draws")
    };
    let first = draw(&mut renderer);
    let second = draw(&mut renderer);

    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "asking again uploaded geometry"
    );
    assert_eq!(first.colour(), second.colour(), "the colour differed");
    for y in 0..first.height() {
        for x in 0..first.width() {
            assert_eq!(first.pick_at(x, y), second.pick_at(x, y));
            assert_eq!(first.hit_at(x, y).face(), second.hit_at(x, y).face());
            assert_eq!(
                first.edge_at(x, y),
                second.edge_at(x, y),
                "the edge at {x},{y} differed between two identical frames"
            );
        }
    }
}

#[test]
fn edges_follow_the_camera_through_both_projections() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(320, 320);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let visibility = Visibility::new(&snapshot);
    let shared = snapshot.edge_of(0, 0).expect("numbered");
    let probe = placed(between(COARSE_ARC[0], COARSE_ARC[1]), 24.0);

    let mut moved_camera = camera;
    moved_camera.orbit(0.35, -0.2);
    moved_camera.pan(12.0, -7.0);
    moved_camera.roll(0.4);

    for (what, mut camera) in [
        ("as drawn", camera),
        ("orbited, panned and rolled", moved_camera),
    ] {
        for projection in [Projection::Orthographic, Projection::Perspective] {
            camera.set_projection(projection);
            let frame = renderer
                .render(
                    &prepared,
                    &camera,
                    Marked::Nothing,
                    Hovered::Nothing,
                    &visibility,
                )
                .expect("draws");
            let at = pixel_of(&camera, probe).expect("the probe stays on screen");
            assert_eq!(
                edge_near(&frame, at),
                shared,
                "{what} in {projection:?}: the edge is not where the shared \
                 camera arithmetic says it is"
            );
            // And not merely near the right place: every edge pixel of the
            // whole picture touches the model. A pass that projected through
            // arithmetic of its own would drift off it, and the further from
            // the middle of the picture the more.
            assert_eq!(
                strays_from_the_model(&frame),
                0,
                "{what} in {projection:?}: edge pixels landed away from the model"
            );
        }
    }
}

#[test]
fn nothing_but_the_model_ever_carries_an_edge_identity() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(256, 256);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let frame = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::new(&snapshot),
        )
        .expect("draws");

    // Every pixel carrying an edge is on the model or within a pixel of it.
    // The outer silhouette is where a line legitimately lands on a pixel the
    // fill did not reach; the backdrop, the grid and the space around them
    // never do. `hit_at` is what refuses such a pixel's edge, and this is the
    // measurement of how few of them there are.
    let strays = strays_from_the_model(&frame);
    for (x, y) in edge_pixels(&frame) {
        if frame.pick_at(x, y) == PickId::NOTHING {
            // Whatever the target holds, a hit refuses it here.
            assert_eq!(
                frame.hit_at(x, y).edge(),
                ferritecad_viewport::EdgePickId::NOTHING,
                "an edge at {x},{y} survived over the background"
            );
        }
    }
    assert_eq!(
        strays, 0,
        "{strays} pixels away from the model carried an edge identity"
    );
}

#[test]
fn a_prepared_picture_of_another_renderer_is_still_refused() {
    let mut renderer = renderer_or_skip!();
    let mut other = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(64, 64);
    let prepared = other.prepare(Arc::clone(&snapshot)).expect("prepares");

    let refusal = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::new(&snapshot),
        )
        .expect_err("another renderer's buffers were drawn");
    assert_eq!(refusal.kind(), ErrorKind::Rendering, "{refusal}");
}

// ---------------------------------------------------------------------------
// Marking the one topological edge under the pointer.
// ---------------------------------------------------------------------------

/// The curved definition placed twice, in whatever material is asked for.
fn curved_pair_coloured(
    width: u32,
    height: u32,
    colour: [f64; 3],
) -> (Arc<RenderSnapshot>, Camera) {
    let mut builder = SnapshotBuilder::new();
    let definition = builder
        .add_mesh(&two_faces_sharing_a_curve())
        .expect("packs");
    for x in [-24.0, 24.0] {
        builder
            .place(definition, None, &moved(x, 0.0, 0.0), colour)
            .expect("places");
    }
    let snapshot = Arc::new(builder.build());
    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(snapshot.bounds().expect("something is drawn"))
        .expect("frames");
    (snapshot, camera)
}

/// Whether a pixel is a sample of `edge`, or touches one.
///
/// The reach of one pixel is not slack, it is the identity target's own
/// limitation stated honestly. That target holds one value per sample, so
/// where two edges meet or cross in the picture the sample reports whichever
/// was drawn last, while the mark, which draws one edge, legitimately covers
/// it. Everything further away than that belongs to another line.
fn is_a_sample_of(frame: &Frame, edge: ferritecad_viewport::EdgePickId, x: u32, y: u32) -> bool {
    (-1i64..=1).any(|dy| {
        (-1i64..=1).any(|dx| {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            nx >= 0 && ny >= 0 && frame.edge_at(nx as u32, ny as u32) == edge
        })
    })
}

/// Every pixel whose colour differs between two frames of one size.
fn changed(before: &Frame, after: &Frame) -> Vec<(u32, u32)> {
    pixels_of(before, |frame, x, y| {
        frame.colour_at(x, y) != after.colour_at(x, y)
    })
}

fn draw(
    renderer: &mut Renderer,
    prepared: &ferritecad_viewport_gpu::PreparedSnapshot,
    camera: &Camera,
    selected: Marked,
    hovered: Hovered,
    visibility: &Visibility,
) -> Frame {
    renderer
        .render(prepared, camera, selected, hovered, visibility)
        .expect("draws")
}

#[test]
fn marking_one_edge_marks_every_segment_of_it_and_nothing_else() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(320, 320);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let visibility = Visibility::new(&snapshot);
    let shared = snapshot.edge_of(0, 0).expect("numbered");

    let plain = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Nothing,
        &visibility,
    );
    let marked = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Edge(shared),
        &visibility,
    );

    let moved_pixels = changed(&plain, &marked);
    assert!(
        moved_pixels.len() > 40,
        "the mark covered {} pixels, which is not a line",
        moved_pixels.len()
    );

    // Everything that changed is a sample of that edge, and nothing that
    // changed belongs to another edge or to the inside of a face.
    for (x, y) in &moved_pixels {
        assert_ne!(
            plain.edge_at(*x, *y),
            ferritecad_viewport::EdgePickId::NOTHING,
            "the pixel at {x},{y} changed and is not on any edge at all"
        );
        assert!(
            is_a_sample_of(&plain, shared, *x, *y),
            "the pixel at {x},{y} changed and is not on the marked edge"
        );
    }
    // And every sample of that edge changed: not the first segment only, and
    // not one side of it only.
    for (x, y) in pixels_of(&plain, |frame, x, y| frame.edge_at(x, y) == shared) {
        assert_ne!(
            plain.colour_at(x, y),
            marked.colour_at(x, y),
            "a sample of the marked edge at {x},{y} was left alone"
        );
    }

    // Both faces' own approximations of the shared curve, and both placements.
    for shift in [-24.0, 24.0] {
        for world in [
            [
                f64::from(FINE_ARC[1][0]),
                f64::from(FINE_ARC[1][1]),
                f64::from(FINE_ARC[1][2]),
            ],
            between(COARSE_ARC[0], COARSE_ARC[1]),
            between(FINE_ARC[2], FINE_ARC[3]),
        ] {
            let at = pixel_of(&camera, placed(world, shift)).expect("on screen");
            assert!(
                moved_pixels
                    .iter()
                    .any(|(x, y)| x.abs_diff(at.0) <= 2 && y.abs_diff(at.1) <= 2),
                "nothing was marked near {at:?}"
            );
        }
    }

    // A neighbouring edge and the inside of a face are untouched.
    for world in [between(FINE_ARC[0], [0.0, 0.0, -16.0]), [-2.89, 0.0, 5.67]] {
        let at = pixel_of(&camera, placed(world, -24.0)).expect("on screen");
        assert_eq!(
            plain.colour_at(at.0, at.1),
            marked.colour_at(at.0, at.1),
            "marking one edge changed something at {at:?}"
        );
    }
}

#[test]
fn a_marked_edge_looks_like_none_of_the_other_four_states() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(288, 288);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let visibility = Visibility::new(&snapshot);
    let definition = snapshot.pick_of(0).expect("drawn");
    let face = snapshot.face_of(0, 0).expect("numbered");
    let edge = snapshot.edge_of(0, 0).expect("numbered");

    let at =
        pixel_of(&camera, placed(between(COARSE_ARC[0], COARSE_ARC[1]), 24.0)).expect("on screen");
    let colour = |renderer: &mut Renderer, selected, hovered| {
        draw(renderer, &prepared, &camera, selected, hovered, &visibility)
            .colour_at(at.0, at.1)
            .expect("on screen")
    };

    let marked_edge = colour(&mut renderer, Marked::Nothing, Hovered::Edge(edge));
    let others = [
        (
            "plain",
            colour(&mut renderer, Marked::Nothing, Hovered::Nothing),
        ),
        (
            "definition hover",
            colour(
                &mut renderer,
                Marked::Nothing,
                Hovered::Definition(definition),
            ),
        ),
        (
            "face hover",
            colour(&mut renderer, Marked::Nothing, Hovered::Face(face)),
        ),
        (
            "selected definition",
            colour(
                &mut renderer,
                Marked::Definition(definition),
                Hovered::Nothing,
            ),
        ),
        (
            "selected face",
            colour(&mut renderer, Marked::Face(face), Hovered::Nothing),
        ),
    ];
    for (what, other) in others {
        assert_ne!(
            marked_edge, other,
            "a marked edge is indistinguishable from {what} at {at:?}"
        );
    }
}

#[test]
fn a_marked_edge_is_visible_on_a_nearly_white_and_a_nearly_black_part() {
    let mut renderer = renderer_or_skip!();
    for (what, material) in [
        ("nearly white", [0.97, 0.97, 0.97]),
        ("nearly black", [0.02, 0.02, 0.02]),
    ] {
        let (snapshot, camera) = curved_pair_coloured(256, 256, material);
        let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
        let visibility = Visibility::new(&snapshot);
        let edge = snapshot.edge_of(0, 0).expect("numbered");
        let plain = draw(
            &mut renderer,
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Nothing,
            &visibility,
        );
        let marked = draw(
            &mut renderer,
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Edge(edge),
            &visibility,
        );

        let samples = pixels_of(&plain, |frame, x, y| frame.edge_at(x, y) == edge);
        assert!(!samples.is_empty(), "{what}: the edge is drawn");
        // Not merely different from the material, but far from it: a mark a
        // person cannot see is not a mark.
        for (x, y) in &samples {
            let ink = marked.colour_at(*x, *y).expect("on screen");
            let fill = [
                (material[0] * 255.0) as i32,
                (material[1] * 255.0) as i32,
                (material[2] * 255.0) as i32,
            ];
            let distance: i32 = (0..3)
                .map(|c| (i32::from(ink[c]) - fill[c]).abs())
                .max()
                .expect("three channels");
            assert!(
                distance > 60,
                "{what}: the mark at {x},{y} is {ink:?} against a fill of {fill:?}"
            );
        }
    }
}

#[test]
fn a_choice_already_made_is_not_repainted_by_a_question_about_its_edge() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(288, 288);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let visibility = Visibility::new(&snapshot);
    let definition = snapshot.pick_of(0).expect("drawn");
    let lower = snapshot.face_of(0, 0).expect("numbered");
    let upper = snapshot.face_of(0, 1).expect("numbered");
    // The shared curve bounds both faces; the lower face's own side edge
    // bounds only the lower one.
    let shared = snapshot.edge_of(0, 0).expect("numbered");
    let lower_side = snapshot.edge_of(0, 1).expect("numbered");
    assert!(snapshot.edge_bounds_face(shared, lower));
    assert!(snapshot.edge_bounds_face(shared, upper));
    assert!(snapshot.edge_bounds_face(lower_side, lower));
    assert!(!snapshot.edge_bounds_face(lower_side, upper));

    let with = |renderer: &mut Renderer, selected, hovered| {
        draw(renderer, &prepared, &camera, selected, hovered, &visibility)
    };

    // The whole part is chosen: an edge of it is not marked over the choice.
    let chosen_part = with(
        &mut renderer,
        Marked::Definition(definition),
        Hovered::Nothing,
    );
    let asked_too = with(
        &mut renderer,
        Marked::Definition(definition),
        Hovered::Edge(shared),
    );
    assert_eq!(
        chosen_part.colour(),
        asked_too.colour(),
        "a question about an edge repainted a chosen part"
    );

    // One face is chosen, and the edge asked about bounds it: same rule.
    let chosen_face = with(&mut renderer, Marked::Face(lower), Hovered::Nothing);
    let asked_adjacent = with(&mut renderer, Marked::Face(lower), Hovered::Edge(shared));
    assert_eq!(
        chosen_face.colour(),
        asked_adjacent.colour(),
        "a question about an edge of a chosen face repainted it"
    );

    // A face is chosen and the edge asked about does not bound it: the
    // question is still worth answering, and it is answered.
    let asked_elsewhere = with(
        &mut renderer,
        Marked::Face(upper),
        Hovered::Edge(lower_side),
    );
    let plain_upper = with(&mut renderer, Marked::Face(upper), Hovered::Nothing);
    assert_ne!(
        plain_upper.colour(),
        asked_elsewhere.colour(),
        "an edge that bounds no chosen face was suppressed anyway"
    );
    for (x, y) in changed(&plain_upper, &asked_elsewhere) {
        assert!(
            is_a_sample_of(&plain_upper, lower_side, x, y),
            "something other than the asked edge changed at {x},{y}"
        );
    }
}

#[test]
fn a_choice_in_another_part_does_not_suppress_a_question_here() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair_behind_a_plate(288, 288);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let mut visibility = Visibility::new(&snapshot);
    // Out of the way, so the edge is on screen at all.
    assert!(visibility.hide(
        Marked::Definition(snapshot.pick_of(1).expect("the plate")),
        &snapshot
    ));
    let plate = snapshot.pick_of(1).expect("the plate");
    let edge = snapshot.edge_of(0, 0).expect("numbered");

    let plain = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Definition(plate),
        Hovered::Nothing,
        &visibility,
    );
    let marked = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Definition(plate),
        Hovered::Edge(edge),
        &visibility,
    );
    assert_ne!(
        plain.colour(),
        marked.colour(),
        "a choice made in another part suppressed this one's edge"
    );
}

#[test]
fn a_marked_edge_changes_no_answer_about_any_pixel() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(256, 256);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let visibility = Visibility::new(&snapshot);
    let uploaded = renderer.geometry_uploads();
    let edge = snapshot.edge_of(0, 0).expect("numbered");

    let plain = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Nothing,
        &visibility,
    );
    let marked = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Edge(edge),
        &visibility,
    );
    let again = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Edge(edge),
        &visibility,
    );

    assert_eq!(
        renderer.geometry_uploads(),
        uploaded,
        "pointing at an edge uploaded geometry"
    );
    assert_eq!(
        marked.colour(),
        again.colour(),
        "two identical frames differ"
    );
    for y in 0..plain.height() {
        for x in 0..plain.width() {
            assert_eq!(plain.pick_at(x, y), marked.pick_at(x, y), "at {x},{y}");
            assert_eq!(
                plain.hit_at(x, y).face(),
                marked.hit_at(x, y).face(),
                "at {x},{y}"
            );
            assert_eq!(plain.edge_at(x, y), marked.edge_at(x, y), "at {x},{y}");
            assert_eq!(marked.edge_at(x, y), again.edge_at(x, y), "at {x},{y}");
        }
    }
}

#[test]
fn a_question_about_a_hidden_part_marks_nothing() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(256, 256);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let definition = snapshot.pick_of(0).expect("drawn");
    let edge = snapshot.edge_of(0, 0).expect("numbered");

    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.hide(Marked::Definition(definition), &snapshot));
    let hidden_plain = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Nothing,
        &visibility,
    );
    let hidden_marked = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Edge(edge),
        &visibility,
    );
    assert_eq!(
        hidden_plain.colour(),
        hidden_marked.colour(),
        "a hidden part was marked"
    );

    // Bringing it back gives exactly the picture it had, byte for byte.
    let shown_before = {
        let all = Visibility::new(&snapshot);
        draw(
            &mut renderer,
            &prepared,
            &camera,
            Marked::Nothing,
            Hovered::Edge(edge),
            &all,
        )
    };
    assert!(visibility.show(Marked::Definition(definition), &snapshot));
    let shown_after = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Edge(edge),
        &visibility,
    );
    assert_eq!(
        shown_before.colour(),
        shown_after.colour(),
        "showing a part again did not restore its marked edge exactly"
    );
}

#[test]
fn the_backdrop_never_answers_a_question_about_an_edge() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = model_over_the_plane(256, 256);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let visibility = Visibility::new(&snapshot);
    // This picture's meshes carry no edge association at all, so there is no
    // edge to ask about and every value is one the picture does not know.
    assert_eq!(snapshot.edge_count(), 0);

    let other = snapshot_of_curved();
    let foreign = other.edge_of(0, 0).expect("numbered");
    let plain = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Nothing,
        &visibility,
    );
    let asked = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Edge(foreign),
        &visibility,
    );
    assert_eq!(
        plain.colour(),
        asked.colour(),
        "an edge of another picture marked this one"
    );
}

/// A curved picture built on its own, for identities to borrow from.
fn snapshot_of_curved() -> Arc<RenderSnapshot> {
    curved_pair(64, 64).0
}

#[test]
fn a_marked_edge_follows_the_camera_through_both_projections() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(288, 288);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let visibility = Visibility::new(&snapshot);
    let edge = snapshot.edge_of(0, 0).expect("numbered");

    let mut moved_camera = camera;
    moved_camera.orbit(0.3, -0.18);
    moved_camera.pan(9.0, -5.0);
    moved_camera.roll(0.35);

    for (what, mut camera) in [
        ("as drawn", camera),
        ("orbited, panned and rolled", moved_camera),
    ] {
        for projection in [Projection::Orthographic, Projection::Perspective] {
            camera.set_projection(projection);
            let plain = draw(
                &mut renderer,
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            );
            let marked = draw(
                &mut renderer,
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Edge(edge),
                &visibility,
            );
            let moved_pixels = changed(&plain, &marked);
            assert!(
                !moved_pixels.is_empty(),
                "{what} in {projection:?}: nothing was marked"
            );
            for (x, y) in moved_pixels {
                assert!(
                    is_a_sample_of(&plain, edge, x, y),
                    "{what} in {projection:?}: the mark at {x},{y} is not on the edge"
                );
            }
        }
    }
}

#[test]
fn two_edges_that_meet_give_one_answer_and_not_two() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(320, 320);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let frame = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Nothing,
        &Visibility::new(&snapshot),
    );

    // Where the shared curve meets the lower face's side edge, three edges
    // are within a pixel or two of each other. The target holds one value per
    // sample, so the answer there is one edge: which one is settled by the
    // draw order the picture packs, and nothing here claims to resolve a
    // choice between candidates.
    let corner = pixel_of(
        &camera,
        placed(
            [
                f64::from(FINE_ARC[0][0]),
                f64::from(FINE_ARC[0][1]),
                f64::from(FINE_ARC[0][2]),
            ],
            -24.0,
        ),
    )
    .expect("on screen");
    let answer = frame.edge_at(corner.0, corner.1);
    assert_ne!(
        answer,
        ferritecad_viewport::EdgePickId::NOTHING,
        "the corner is on some edge"
    );
    // Asked again, the same sample gives the same one edge.
    let again = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Nothing,
        &Visibility::new(&snapshot),
    );
    assert_eq!(answer, again.edge_at(corner.0, corner.1));
}

#[test]
fn a_chosen_edge_is_marked_as_chosen_and_nothing_else_is() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(320, 320);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let visibility = Visibility::new(&snapshot);
    let uploaded = renderer.geometry_uploads();
    let chosen = snapshot.edge_of(0, 0).expect("numbered");
    let neighbour = snapshot.edge_of(0, 1).expect("numbered");
    let definition = snapshot.pick_of(0).expect("drawn");
    let face = snapshot.face_of(0, 0).expect("numbered");

    let plain = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Nothing,
        &visibility,
    );
    let selected = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Edge(chosen),
        Hovered::Nothing,
        &visibility,
    );

    // Every sample of that edge changed, in both faces' representations and
    // in every placement, and nothing off it did.
    let moved_pixels = changed(&plain, &selected);
    assert!(moved_pixels.len() > 40, "{} pixels", moved_pixels.len());
    for (x, y) in &moved_pixels {
        assert!(
            is_a_sample_of(&plain, chosen, *x, *y),
            "the pixel at {x},{y} changed and is not on the chosen edge"
        );
    }
    for (x, y) in pixels_of(&plain, |frame, x, y| frame.edge_at(x, y) == chosen) {
        assert_ne!(
            plain.colour_at(x, y),
            selected.colour_at(x, y),
            "a sample of the chosen edge at {x},{y} was left alone"
        );
    }
    for shift in [-24.0, 24.0] {
        for world in [
            [
                f64::from(FINE_ARC[1][0]),
                f64::from(FINE_ARC[1][1]),
                f64::from(FINE_ARC[1][2]),
            ],
            between(COARSE_ARC[0], COARSE_ARC[1]),
        ] {
            let at = pixel_of(&camera, placed(world, shift)).expect("on screen");
            assert!(
                moved_pixels
                    .iter()
                    .any(|(x, y)| x.abs_diff(at.0) <= 2 && y.abs_diff(at.1) <= 2),
                "nothing was marked near {at:?}"
            );
        }
    }

    // A decision does not look like a question, nor like the other states.
    // Every frame is drawn first and compared afterwards, at a pixel the mark
    // is known to have covered, so nothing here depends on the order they were
    // asked for.
    let asked = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Edge(chosen),
        &visibility,
    );
    let chosen_face = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Face(face),
        Hovered::Nothing,
        &visibility,
    );
    let chosen_part = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Definition(definition),
        Hovered::Nothing,
        &visibility,
    );
    let at = *moved_pixels.first().expect("the mark covered something");
    let decided = selected.colour_at(at.0, at.1).expect("on screen");
    for (what, other) in [
        ("plain", &plain),
        ("the same edge asked about", &asked),
        ("a chosen face", &chosen_face),
        ("a chosen part", &chosen_part),
    ] {
        assert_ne!(
            Some(decided),
            other.colour_at(at.0, at.1),
            "a chosen edge looks like {what} at {at:?}"
        );
    }

    // A choice wins over a question about the same edge, and a question about
    // another edge is still answered.
    let asked_about_itself = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Edge(chosen),
        Hovered::Edge(chosen),
        &visibility,
    );
    assert_eq!(
        asked_about_itself.colour(),
        selected.colour(),
        "pointing at what is already chosen repainted it"
    );
    let both = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Edge(chosen),
        Hovered::Edge(neighbour),
        &visibility,
    );
    let elsewhere = pixel_of(
        &camera,
        placed(between(FINE_ARC[0], [0.0, 0.0, -16.0]), -24.0),
    )
    .expect("on screen");
    assert_ne!(
        both.colour_at(elsewhere.0, elsewhere.1),
        selected.colour_at(elsewhere.0, elsewhere.1),
        "another edge could not be asked about beside the chosen one"
    );

    // Nothing about any pixel changed, and nothing was uploaded.
    assert_eq!(renderer.geometry_uploads(), uploaded);
    for y in 0..plain.height() {
        for x in 0..plain.width() {
            assert_eq!(plain.pick_at(x, y), selected.pick_at(x, y), "at {x},{y}");
            assert_eq!(
                plain.hit_at(x, y).face(),
                selected.hit_at(x, y).face(),
                "at {x},{y}"
            );
            assert_eq!(plain.edge_at(x, y), selected.edge_at(x, y), "at {x},{y}");
        }
    }
    // And two identical frames are identical.
    let again = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Edge(chosen),
        Hovered::Nothing,
        &visibility,
    );
    assert_eq!(selected.colour(), again.colour());
}

#[test]
fn a_chosen_edge_of_a_hidden_part_marks_nothing() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(256, 256);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let chosen = snapshot.edge_of(0, 0).expect("numbered");
    let mut visibility = Visibility::new(&snapshot);
    assert!(visibility.hide(Marked::Edge(chosen), &snapshot));

    let plain = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Nothing,
        Hovered::Nothing,
        &visibility,
    );
    let selected = draw(
        &mut renderer,
        &prepared,
        &camera,
        Marked::Edge(chosen),
        Hovered::Nothing,
        &visibility,
    );
    assert_eq!(
        plain.colour(),
        selected.colour(),
        "a hidden part's edge was marked"
    );
}

#[test]
fn a_chosen_edge_follows_the_camera_through_both_projections() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = curved_pair(288, 288);
    let prepared = renderer.prepare(Arc::clone(&snapshot)).expect("prepares");
    let visibility = Visibility::new(&snapshot);
    let chosen = snapshot.edge_of(0, 0).expect("numbered");

    let mut moved_camera = camera;
    moved_camera.orbit(0.3, -0.18);
    moved_camera.pan(9.0, -5.0);
    moved_camera.roll(0.35);

    for (what, mut camera) in [
        ("as drawn", camera),
        ("orbited, panned and rolled", moved_camera),
    ] {
        for projection in [Projection::Orthographic, Projection::Perspective] {
            camera.set_projection(projection);
            let plain = draw(
                &mut renderer,
                &prepared,
                &camera,
                Marked::Nothing,
                Hovered::Nothing,
                &visibility,
            );
            let selected = draw(
                &mut renderer,
                &prepared,
                &camera,
                Marked::Edge(chosen),
                Hovered::Nothing,
                &visibility,
            );
            let moved_pixels = changed(&plain, &selected);
            assert!(!moved_pixels.is_empty(), "{what} in {projection:?}");
            for (x, y) in moved_pixels {
                assert!(
                    is_a_sample_of(&plain, chosen, x, y),
                    "{what} in {projection:?}: the mark at {x},{y} is off the edge"
                );
            }
        }
    }
}
