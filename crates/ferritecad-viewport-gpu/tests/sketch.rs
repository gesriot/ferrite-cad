// SPDX-License-Identifier: MIT
//! A document's drawings, on a real device.
//!
//! What a drawing *is* – which plane it sits on, which way an arc runs, what a
//! circle closes to – is settled without a graphics stack in
//! `ferritecad-scene`. What is left for a device to answer is whether the
//! thing is actually on screen, whether it is on screen where the shared
//! camera says it should be, whether it stays the same width in pixels
//! wherever the camera goes, and whether putting it there changed anything
//! about the picture underneath: its depth, its colour away from the drawing,
//! and all four of the identities a click reads.
//!
//! Every gate skips itself when no adapter is available. A machine without a
//! GPU is an ordinary machine.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::sync::Arc;

use ferritecad_kernel::{
    Mesh, MeshFaceRange, SessionId, ShapeHandle, SubShapeHandle, SubShapeKind,
};
use ferritecad_types::{ErrorKind, Transform, Vec3};
use ferritecad_viewport::{
    Camera, Hovered, Marked, Projection, RenderSnapshot, SketchDrawing, SketchDrawingBuilder,
    SketchStyle, SnapshotBuilder, Visibility,
};
use ferritecad_viewport_gpu::{
    Frame, PreparedSnapshot, Renderer, SKETCH_COLOUR, SKETCH_CONSTRUCTION_COLOUR,
    SKETCH_CONSTRUCTION_STROKE_PIXELS, SKETCH_POINT_PIXELS, SKETCH_STROKE_PIXELS,
};

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

/// How near a colour has to be to count as the ink it claims to be.
///
/// Two levels out of two hundred and fifty-six: the rounding an eight-bit
/// target does to a float and nothing more. A drawing does not blend, so a
/// sketch pixel is exactly the constant the renderer declares.
const INK: i32 = 2;

fn is_ink(pixel: [u8; 4], colour: [f32; 3]) -> bool {
    (0..3).all(|channel| {
        let wanted = (colour[channel] * 255.0).round() as i32;
        (i32::from(pixel[channel]) - wanted).abs() <= INK
    })
}

/// Every pixel of `frame` drawn in `colour`.
fn ink_of(frame: &Frame, colour: [f32; 3]) -> Vec<(u32, u32)> {
    let mut found = Vec::new();
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            if let Some(pixel) = frame.colour_at(x, y)
                && is_ink(pixel, colour)
            {
                found.push((x, y));
            }
        }
    }
    found
}

/// Where a world point lands in a frame of this size.
///
/// Through the very matrix the frame was drawn with. A gate that recomputed a
/// camera would be measuring its own arithmetic rather than the renderer's,
/// and the whole point of one `view_projection` is that there is nothing else
/// to compare against.
fn on_screen(camera: &Camera, point: [f32; 3]) -> Option<(f32, f32)> {
    let m = camera.view_projection();
    let v = [point[0], point[1], point[2], 1.0];
    let mut clip = [0.0_f32; 4];
    for row in 0..4 {
        for column in 0..4 {
            clip[row] += m[column * 4 + row] * v[column];
        }
    }
    if clip[3] <= 0.0 {
        return None;
    }
    let (width, height) = (camera.width() as f32, camera.height() as f32);
    Some((
        (clip[0] / clip[3] * 0.5 + 0.5) * width,
        (0.5 - clip[1] / clip[3] * 0.5) * height,
    ))
}

/// How far a drawn sample may sit from where the geometry says it is.
///
/// Half the stroke plus one: a stroke is centred on the curve and reaches half
/// its width either side, and the extra pixel is the rasteriser's rounding of
/// a sample position onto a pixel centre. Stated rather than tuned, so a
/// drawing that drifted fails instead of being absorbed.
const PIXEL_TOLERANCE: f32 = SKETCH_STROKE_PIXELS / 2.0 + 1.0;

/// How far a pixel is from the nearest of the segments in `outline`.
fn distance_to(outline: &[([f32; 3], [f32; 3])], camera: &Camera, at: (u32, u32)) -> f32 {
    let point = (at.0 as f32 + 0.5, at.1 as f32 + 0.5);
    outline
        .iter()
        .filter_map(|(a, b)| Some((on_screen(camera, *a)?, on_screen(camera, *b)?)))
        .map(|(a, b)| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let length = dx * dx + dy * dy;
            let along = if length <= f32::EPSILON {
                0.0
            } else {
                (((point.0 - a.0) * dx + (point.1 - a.1) * dy) / length).clamp(0.0, 1.0)
            };
            let near = (a.0 + dx * along, a.1 + dy * along);
            ((point.0 - near.0).powi(2) + (point.1 - near.1).powi(2)).sqrt()
        })
        .fold(f32::INFINITY, f32::min)
}

/// A square in the XZ plane, facing -Y, two triangles.
///
/// Facing -Y because that is where a framed camera puts the eye, so a drawing
/// in the same plane is seen face on rather than edge on.
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

fn at(y: f64) -> Transform {
    Transform::from_translation(Vec3::new(0.0, y, 0.0).expect("finite")).expect("finite")
}

/// One green quad at `y`, and a camera framing it.
fn one_quad(width: u32, height: u32, y: f64) -> (Arc<RenderSnapshot>, Camera) {
    let mut builder = SnapshotBuilder::new();
    let mesh = builder.add_mesh(&quad(10.0)).expect("packs");
    builder
        .place(mesh, None, &at(y), [0.0, 0.4, 0.0])
        .expect("places");
    let snapshot = Arc::new(builder.build());
    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(([-10.0, -10.0, -10.0], [10.0, 10.0, 10.0]))
        .expect("frames");
    (snapshot, camera)
}

/// Nothing drawn at all, and a camera looking where a drawing will be.
fn no_picture(width: u32, height: u32) -> (Arc<RenderSnapshot>, Camera) {
    let snapshot = Arc::new(SnapshotBuilder::new().build());
    let mut camera = Camera::new();
    camera.resize(width, height);
    camera
        .frame(([-10.0, -10.0, -10.0], [10.0, 10.0, 10.0]))
        .expect("frames");
    (snapshot, camera)
}

/// The four sides of a square in the plane `y`, as world segments.
fn square(half: f32, y: f32) -> [([f32; 3], [f32; 3]); 4] {
    let corners = [
        [-half, y, -half],
        [half, y, -half],
        [half, y, half],
        [-half, y, half],
    ];
    [
        (corners[0], corners[1]),
        (corners[1], corners[2]),
        (corners[2], corners[3]),
        (corners[3], corners[0]),
    ]
}

/// A drawing of one closed square in the plane `y`.
fn square_drawing(half: f32, y: f32, style: SketchStyle) -> SketchDrawing {
    let mut builder = SketchDrawingBuilder::new();
    let mut run: Vec<[f64; 3]> = square(half, y)
        .iter()
        .map(|(a, _)| [a[0] as f64, a[1] as f64, a[2] as f64])
        .collect();
    run.push(run[0]);
    builder.stroke(style, &run).expect("a square is drawable");
    builder.build()
}

/// A prepared picture carrying the given drawings.
fn prepared_with(
    renderer: &mut Renderer,
    snapshot: &Arc<RenderSnapshot>,
    drawings: &[SketchDrawing],
) -> PreparedSnapshot {
    let prepared = renderer
        .prepare(Arc::clone(snapshot))
        .expect("the picture uploads");
    renderer
        .prepare_sketches(prepared, drawings)
        .expect("the drawings upload")
}

fn drawn(renderer: &mut Renderer, prepared: &PreparedSnapshot, camera: &Camera) -> Frame {
    renderer
        .render(
            prepared,
            camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::new(prepared.snapshot()),
        )
        .expect("draws")
}

#[test]
fn a_drawing_nothing_was_raised_from_is_still_on_screen() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = no_picture(240, 240);
    let prepared = prepared_with(
        &mut renderer,
        &snapshot,
        &[square_drawing(8.0, 0.0, SketchStyle::Model)],
    );
    let frame = drawn(&mut renderer, &prepared, &camera);

    let ink = ink_of(&frame, SKETCH_COLOUR);
    assert!(
        !ink.is_empty(),
        "a drawing no feature reads was not drawn, so a sketch is only visible when something \
         was extruded from it"
    );
    // On the square, and on nothing else. There is no model here at all, so
    // anything else drawn in this colour came from the drawing being in the
    // wrong place.
    let outline = square(8.0, 0.0);
    for sample in &ink {
        assert!(
            distance_to(&outline, &camera, *sample) <= PIXEL_TOLERANCE,
            "a sample at {sample:?} is not on the square that was drawn"
        );
    }
}

#[test]
fn every_kind_of_curve_a_drawing_can_hold_reaches_its_own_pixels() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = no_picture(400, 400);

    // A line, a closed circle, a quarter arc and a point, each somewhere of
    // its own, plus one construction line so both styles are in one frame.
    let mut builder = SketchDrawingBuilder::new();
    builder
        .stroke(SketchStyle::Model, &[[-9.0, 0.0, -9.0], [9.0, 0.0, -9.0]])
        .expect("a line");
    let circle: Vec<[f64; 3]> = (0..=64)
        .map(|step| {
            let angle = std::f64::consts::TAU * f64::from(step) / 64.0;
            [4.0 * angle.cos() - 4.0, 0.0, 4.0 * angle.sin()]
        })
        .collect();
    builder
        .stroke(SketchStyle::Model, &circle)
        .expect("a circle");
    let arc: Vec<[f64; 3]> = (0..=16)
        .map(|step| {
            let angle = std::f64::consts::FRAC_PI_2 * f64::from(step) / 16.0;
            [3.0 * angle.cos() + 5.0, 0.0, 3.0 * angle.sin() + 3.0]
        })
        .collect();
    builder.stroke(SketchStyle::Model, &arc).expect("an arc");
    builder
        .point(SketchStyle::Model, [0.0, 0.0, 9.0])
        .expect("a point");
    builder
        .stroke(
            SketchStyle::Construction,
            &[[-9.0, 0.0, 7.0], [9.0, 0.0, 7.0]],
        )
        .expect("a guide");
    let prepared = prepared_with(&mut renderer, &snapshot, &[builder.build()]);
    let frame = drawn(&mut renderer, &prepared, &camera);

    let near = |wanted: [f32; 3], colour: [f32; 3]| {
        let (x, y) = on_screen(&camera, wanted).expect("in front of the eye");
        ink_of(&frame, colour).iter().any(|(px, py)| {
            ((*px as f32 + 0.5) - x).abs() <= PIXEL_TOLERANCE
                && ((*py as f32 + 0.5) - y).abs() <= PIXEL_TOLERANCE
        })
    };

    assert!(near([0.0, 0.0, -9.0], SKETCH_COLOUR), "the line is missing");
    assert!(
        near([0.0, 0.0, 0.0], SKETCH_COLOUR),
        "the circle is missing at its rightmost sample"
    );
    assert!(
        near([-8.0, 0.0, 0.0], SKETCH_COLOUR),
        "the circle is missing at its leftmost sample"
    );
    assert!(
        near([8.0, 0.0, 3.0], SKETCH_COLOUR),
        "the arc is missing at the angle it starts on"
    );
    assert!(
        near([5.0, 0.0, 6.0], SKETCH_COLOUR),
        "the arc is missing at the angle it ends on"
    );
    assert!(near([0.0, 0.0, 9.0], SKETCH_COLOUR), "the point is missing");
    assert!(
        near([0.0, 0.0, 7.0], SKETCH_CONSTRUCTION_COLOUR),
        "the construction line is missing"
    );

    // And what guides the drawing is not drawn as what bounds a face.
    assert!(
        !near([0.0, 0.0, 7.0], SKETCH_COLOUR),
        "construction geometry was given the model's own ink"
    );
    assert!(
        !near([0.0, 0.0, -9.0], SKETCH_CONSTRUCTION_COLOUR),
        "model geometry was given the construction ink"
    );
}

#[test]
fn what_guides_a_drawing_is_thinner_than_what_bounds_a_face() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = no_picture(300, 300);
    let mut builder = SketchDrawingBuilder::new();
    // Two horizontal runs, so a column of the frame crosses each exactly once
    // and the count of ink in it is the width.
    builder
        .stroke(SketchStyle::Model, &[[-9.0, 0.0, -4.0], [9.0, 0.0, -4.0]])
        .expect("a line");
    builder
        .stroke(
            SketchStyle::Construction,
            &[[-9.0, 0.0, 4.0], [9.0, 0.0, 4.0]],
        )
        .expect("a guide");
    let prepared = prepared_with(&mut renderer, &snapshot, &[builder.build()]);
    let frame = drawn(&mut renderer, &prepared, &camera);

    let thickness = |colour: [f32; 3]| {
        let ink = ink_of(&frame, colour);
        assert!(!ink.is_empty(), "nothing was drawn in {colour:?}");
        let column = ink[ink.len() / 2].0;
        ink.iter().filter(|(x, _)| *x == column).count()
    };
    let model = thickness(SKETCH_COLOUR);
    let guide = thickness(SKETCH_CONSTRUCTION_COLOUR);
    assert!(
        guide < model,
        "a construction curve is {guide} pixels and a model curve {model}: the two are the same \
         thing to look at"
    );
    assert!(guide >= 1, "a construction curve vanished entirely");
    // And both are the widths the renderer declares, so the difference is the
    // decision recorded beside those two constants rather than a coincidence
    // of this frame.
    assert!(
        model.abs_diff(SKETCH_STROKE_PIXELS.round() as usize) <= 1,
        "a model curve declared {SKETCH_STROKE_PIXELS} pixels wide drew {model}"
    );
    assert!(
        guide.abs_diff(SKETCH_CONSTRUCTION_STROKE_PIXELS.round() as usize) <= 1,
        "a construction curve declared {SKETCH_CONSTRUCTION_STROKE_PIXELS} pixels wide drew \
         {guide}"
    );
}

#[test]
fn a_stroke_is_the_same_number_of_pixels_wide_however_near_the_camera_is() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, mut camera) = no_picture(300, 300);
    let mut builder = SketchDrawingBuilder::new();
    builder
        .stroke(SketchStyle::Model, &[[-9.0, 0.0, 0.0], [9.0, 0.0, 0.0]])
        .expect("a line");
    builder
        .point(SketchStyle::Model, [0.0, 0.0, 6.0])
        .expect("a point");
    let prepared = prepared_with(&mut renderer, &snapshot, &[builder.build()]);

    let measure = |renderer: &mut Renderer, camera: &Camera| {
        let frame = drawn(renderer, &prepared, camera);
        let ink = ink_of(&frame, SKETCH_COLOUR);
        assert!(!ink.is_empty(), "nothing was drawn to measure");
        // The run is the wide, flat band; the point is the small square above
        // it. Split them by which row they are on.
        let (line_row, point_row) = {
            let (x, _) = on_screen(camera, [0.0, 0.0, 0.0]).expect("in front");
            let (_, line_y) = on_screen(camera, [0.0, 0.0, 0.0]).expect("in front");
            let (_, point_y) = on_screen(camera, [0.0, 0.0, 6.0]).expect("in front");
            (
                (x.round() as u32, line_y.round() as u32),
                (x.round() as u32, point_y.round() as u32),
            )
        };
        let across = |column: u32, near_row: u32| {
            ink.iter()
                .filter(|(x, y)| *x == column && y.abs_diff(near_row) <= 8)
                .count()
        };
        (
            across(line_row.0, line_row.1),
            across(point_row.0, point_row.1),
        )
    };

    let (line_far, point_far) = measure(&mut renderer, &camera);
    // Much nearer. A width that was a length in the world would grow with the
    // magnification; a width in pixels does not.
    camera.zoom(0.75);
    let (line_near, point_near) = measure(&mut renderer, &camera);

    let close = |a: usize, b: usize| a.abs_diff(b) <= 1;
    assert!(
        close(line_far, line_near),
        "a stroke is {line_far} pixels wide far away and {line_near} near to: its width is a \
         length in the world rather than a number of pixels"
    );
    assert!(
        close(point_far, point_near),
        "a point is {point_far} pixels across far away and {point_near} near to"
    );
    // And the numbers really are the ones the renderer declares.
    let wanted = SKETCH_STROKE_PIXELS.round() as usize;
    assert!(
        line_far.abs_diff(wanted) <= 1,
        "a stroke declared {SKETCH_STROKE_PIXELS} pixels wide drew {line_far}"
    );
    let wanted_point = SKETCH_POINT_PIXELS.round() as usize;
    assert!(
        point_far.abs_diff(wanted_point) <= 1,
        "a point declared {SKETCH_POINT_PIXELS} pixels across drew {point_far}"
    );
}

#[test]
fn a_drawing_is_in_the_world_and_goes_wherever_the_camera_puts_the_world() {
    let mut renderer = renderer_or_skip!();
    let drawing = square_drawing(8.0, 0.0, SketchStyle::Model);
    let outline = square(8.0, 0.0);

    for (name, size) in [("landscape", (400_u32, 260_u32)), ("portrait", (260, 400))] {
        let (snapshot, base) = no_picture(size.0, size.1);
        let prepared = prepared_with(&mut renderer, &snapshot, std::slice::from_ref(&drawing));

        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut moved = [
                ("still", base),
                ("orbited", base),
                ("panned", base),
                ("zoomed", base),
                ("rolled", base),
            ];
            moved[1].1.orbit(0.6, -0.35);
            moved[2].1.pan(0.15, -0.2);
            moved[3].1.zoom(0.6);
            moved[4].1.roll(0.7);

            for (movement, mut camera) in moved {
                camera.set_projection(projection);
                let frame = drawn(&mut renderer, &prepared, &camera);
                let ink = ink_of(&frame, SKETCH_COLOUR);
                assert!(
                    !ink.is_empty(),
                    "{name}, {projection:?}, {movement}: the drawing left the screen"
                );
                let stray: Vec<(u32, u32)> = ink
                    .iter()
                    .copied()
                    .filter(|sample| distance_to(&outline, &camera, *sample) > PIXEL_TOLERANCE)
                    .collect();
                assert!(
                    stray.is_empty(),
                    "{name}, {projection:?}, {movement}: {} of {} samples are not where the \
                     shared camera puts the geometry: {:?}",
                    stray.len(),
                    ink.len(),
                    &stray[..stray.len().min(6)]
                );
            }
        }
    }
}

#[test]
fn the_model_hides_a_drawing_behind_it_and_not_one_in_front_of_it() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = one_quad(240, 240, 0.0);

    // Behind: further along +Y, which is away from a framed eye.
    let behind = square_drawing(5.0, 4.0, SketchStyle::Model);
    let prepared = prepared_with(&mut renderer, &snapshot, &[behind]);
    let frame = drawn(&mut renderer, &prepared, &camera);
    assert!(
        ink_of(&frame, SKETCH_COLOUR).is_empty(),
        "a drawing behind a solid surface was drawn straight through it"
    );

    // In front, and the same square.
    let front = square_drawing(5.0, -4.0, SketchStyle::Model);
    let prepared = prepared_with(&mut renderer, &snapshot, &[front]);
    let frame = drawn(&mut renderer, &prepared, &camera);
    assert!(
        !ink_of(&frame, SKETCH_COLOUR).is_empty(),
        "a drawing in front of the model is hidden by it"
    );
}

#[test]
fn a_drawing_on_the_very_surface_raised_from_it_is_drawn_whole() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = one_quad(240, 240, 0.0);
    // Exactly coplanar with the quad, which is where a sketch sits relative to
    // the cap of the solid raised from it. Two coplanar surfaces fight pixel
    // by pixel unless somebody decides the tie.
    let drawing = square_drawing(5.0, 0.0, SketchStyle::Model);
    let prepared = prepared_with(&mut renderer, &snapshot, &[drawing]);
    let frame = drawn(&mut renderer, &prepared, &camera);

    let ink = ink_of(&frame, SKETCH_COLOUR);
    assert!(!ink.is_empty(), "a coplanar drawing was hidden entirely");

    // No holes: every sample of the square is drawn, not a speckled half of
    // them. Measured along one side, where the run is straight.
    let outline = square(5.0, 0.0);
    let (from, to) = (
        on_screen(&camera, outline[0].0).expect("in front"),
        on_screen(&camera, outline[0].1).expect("in front"),
    );
    let steps: u16 = 24;
    let missing = (1..steps)
        .filter(|step| {
            let along = f32::from(*step) / f32::from(steps);
            let x = from.0 + (to.0 - from.0) * along;
            let y = from.1 + (to.1 - from.1) * along;
            !ink.iter().any(|(px, py)| {
                ((*px as f32 + 0.5) - x).abs() <= PIXEL_TOLERANCE
                    && ((*py as f32 + 0.5) - y).abs() <= PIXEL_TOLERANCE
            })
        })
        .count();
    assert_eq!(
        missing,
        0,
        "{missing} of {} samples along a coplanar edge are holes, which is what z-fighting looks \
         like",
        steps - 1
    );

    // And it is the same every time: a fight would come out differently from
    // one frame to the next only by luck, but a stable answer is the claim.
    let again = drawn(&mut renderer, &prepared, &camera);
    assert_eq!(
        frame.colour(),
        again.colour(),
        "two frames of one coplanar drawing came out differently"
    );
}

#[test]
fn a_drawing_writes_no_depth_of_its_own() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, mut camera) = one_quad(240, 240, 0.0);
    // Parallel, so two runs at different depths project onto exactly the same
    // band of pixels and the only question left is which of them the depth
    // lets through. Under perspective they would project to two bands that
    // merely look alike, and a drawing that wrote depth would still be visible
    // beside the one that hid it - which is a gate that measures nothing.
    assert!(
        camera.set_projection(Projection::Orthographic),
        "the camera refused a parallel projection"
    );

    let run = |y: f64, style| {
        let mut builder = SketchDrawingBuilder::new();
        builder
            .stroke(style, &[[-9.0, y, 0.0], [9.0, y, 0.0]])
            .expect("a run");
        builder.build()
    };

    // First, that the band is drawn at all, so the absence measured below is
    // an absence of something that would otherwise be there.
    let alone = prepared_with(
        &mut renderer,
        &snapshot,
        &[run(-3.0, SketchStyle::Construction)],
    );
    assert!(
        !ink_of(
            &drawn(&mut renderer, &alone, &camera),
            SKETCH_CONSTRUCTION_COLOUR
        )
        .is_empty(),
        "the farther run is not drawn even by itself"
    );

    // Then the nearer run in front of it, and drawn first. If a drawing wrote
    // depth, the farther one would be refused everywhere the nearer one had
    // been; with no depth written it simply covers it.
    let both = prepared_with(
        &mut renderer,
        &snapshot,
        &[
            run(-6.0, SketchStyle::Model),
            run(-3.0, SketchStyle::Construction),
        ],
    );
    let frame = drawn(&mut renderer, &both, &camera);
    assert!(
        !ink_of(&frame, SKETCH_CONSTRUCTION_COLOUR).is_empty(),
        "a drawing was refused by the depth another drawing wrote, so a sketch is writing depth"
    );
}

#[test]
fn a_drawing_changes_no_pixel_of_what_a_click_would_name() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = one_quad(240, 240, 0.0);

    let without = prepared_with(&mut renderer, &snapshot, &[]);
    let plain = drawn(&mut renderer, &without, &camera);

    // A drawing in front of the model: a square inside its outline, so some
    // of the ink is over the surface, and a run reaching well outside it, so
    // some of the ink is over the backdrop.
    let mut builder = SketchDrawingBuilder::new();
    let mut run: Vec<[f64; 3]> = square(6.0, -4.0)
        .iter()
        .map(|(a, _)| [a[0] as f64, a[1] as f64, a[2] as f64])
        .collect();
    run.push(run[0]);
    builder.stroke(SketchStyle::Model, &run).expect("a square");
    builder
        .stroke(SketchStyle::Model, &[[-18.0, -4.0, 2.0], [18.0, -4.0, 2.0]])
        .expect("a run across the picture");
    builder
        .point(SketchStyle::Model, [0.0, -4.0, 0.0])
        .expect("a point");
    let with = prepared_with(&mut renderer, &snapshot, &[builder.build()]);
    let marked = drawn(&mut renderer, &with, &camera);

    let ink = ink_of(&marked, SKETCH_COLOUR);
    assert!(
        !ink.is_empty(),
        "there is no drawing here to prove anything"
    );
    let over_the_model = ink
        .iter()
        .filter(|(x, y)| plain.pick_at(*x, *y) != ferritecad_viewport::PickId::NOTHING)
        .count();
    assert!(
        over_the_model > 0,
        "the drawing misses the model entirely, so this gate cannot see a change to its identity"
    );

    for y in 0..marked.height() {
        for x in 0..marked.width() {
            assert_eq!(
                plain.pick_at(x, y),
                marked.pick_at(x, y),
                "the drawing changed which definition ({x}, {y}) is"
            );
            assert_eq!(
                plain.hit_at(x, y),
                marked.hit_at(x, y),
                "the drawing changed what ({x}, {y}) is a question about"
            );
            assert_eq!(
                plain.edge_at(x, y),
                marked.edge_at(x, y),
                "the drawing changed which edge ({x}, {y}) is on"
            );
            assert_eq!(
                plain.vertex_at(x, y),
                marked.vertex_at(x, y),
                "the drawing changed which corner ({x}, {y}) is on"
            );
        }
    }
}

#[test]
fn a_drawing_over_the_backdrop_names_nothing_at_all() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = one_quad(240, 240, 0.0);
    // Reaching well outside the quad, so most of it is over the grid.
    let drawing = square_drawing(18.0, -4.0, SketchStyle::Model);
    let prepared = prepared_with(&mut renderer, &snapshot, &[drawing]);
    let frame = drawn(&mut renderer, &prepared, &camera);

    let ink = ink_of(&frame, SKETCH_COLOUR);
    assert!(!ink.is_empty(), "nothing was drawn");
    let outside: Vec<(u32, u32)> = ink
        .iter()
        .copied()
        .filter(|(x, y)| frame.pick_at(*x, *y) == ferritecad_viewport::PickId::NOTHING)
        .collect();
    assert!(
        !outside.is_empty(),
        "no part of the drawing landed on the backdrop, so this gate proves nothing"
    );
    for (x, y) in outside {
        let hit = frame.hit_at(x, y);
        assert_eq!(hit.definition(), ferritecad_viewport::PickId::NOTHING);
        assert_eq!(hit.face(), ferritecad_viewport::FacePickId::NOTHING);
        assert_eq!(hit.edge(), ferritecad_viewport::EdgePickId::NOTHING);
        assert_eq!(hit.vertex(), ferritecad_viewport::VertexPickId::NOTHING);
    }
}

/// A quad whose -Z border is one topological edge and whose first corner is
/// one topological vertex.
///
/// Enough for the two questions a picture already answers about itself: which
/// edge is being pointed at, and which corner. Both are marked over the model,
/// and a drawing must not take those pixels away.
fn quad_with_a_named_border(half: f32) -> Mesh {
    let shape = ShapeHandle::new(SessionId::new(), 4);
    Mesh {
        topological_vertices: Some(ferritecad_kernel::MeshVertices {
            occurrences: vec![0],
            ranges: vec![ferritecad_kernel::MeshVertexRange {
                vertex: SubShapeHandle::new(shape, SubShapeKind::Vertex, 0),
                first_occurrence: 0,
                occurrence_count: 1,
            }],
        }),
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
        edges: Some(ferritecad_kernel::MeshEdges {
            segments: vec![0, 1],
            ranges: vec![ferritecad_kernel::MeshEdgeRange {
                edge: SubShapeHandle::new(shape, SubShapeKind::Edge, 0),
                first_segment: 0,
                segment_count: 1,
            }],
        }),
    }
}

#[test]
fn a_mark_on_the_model_wins_over_a_drawing_that_crosses_it() {
    let mut renderer = renderer_or_skip!();
    let mut builder = SnapshotBuilder::new();
    let mesh = builder
        .add_mesh(&quad_with_a_named_border(10.0))
        .expect("packs");
    builder
        .place(mesh, None, &at(0.0), [0.0, 0.4, 0.0])
        .expect("places");
    let snapshot = Arc::new(builder.build());
    let mut camera = Camera::new();
    camera.resize(240, 240);
    camera
        .frame(([-10.0, -10.0, -10.0], [10.0, 10.0, 10.0]))
        .expect("frames");

    let edge = snapshot
        .edge_of(0, 0)
        .expect("the quad has one named border");
    let vertex = snapshot
        .vertex_of(0, 0)
        .expect("the quad has one named corner");

    // A drawing lying right over that border and that corner, in front of the
    // model so nothing hides it.
    let mut drawing = SketchDrawingBuilder::new();
    drawing
        .stroke(
            SketchStyle::Model,
            &[[-12.0, -2.0, -10.0], [12.0, -2.0, -10.0]],
        )
        .expect("a run along the border");
    let drawing = drawing.build();

    let plain = prepared_with(&mut renderer, &snapshot, &[]);
    let with = prepared_with(&mut renderer, &snapshot, std::slice::from_ref(&drawing));

    let render = |renderer: &mut Renderer, prepared: &PreparedSnapshot, marked: Marked| {
        renderer
            .render(
                prepared,
                &camera,
                marked,
                Hovered::Nothing,
                &Visibility::new(&snapshot),
            )
            .expect("draws")
    };

    let bare = render(&mut renderer, &plain, Marked::Nothing);
    for (what, marked) in [
        ("edge", Marked::Edge(edge)),
        ("corner", Marked::Vertex(vertex)),
    ] {
        let only_marked = render(&mut renderer, &plain, marked);
        let both = render(&mut renderer, &with, marked);

        // The pixels the mark actually changed, found by asking the picture
        // rather than by guessing the mark's colour.
        let marks: Vec<(u32, u32)> = (0..bare.height())
            .flat_map(|y| (0..bare.width()).map(move |x| (x, y)))
            .filter(|(x, y)| bare.colour_at(*x, *y) != only_marked.colour_at(*x, *y))
            .collect();
        assert!(!marks.is_empty(), "the {what} mark changed no pixel");

        let lost: Vec<(u32, u32)> = marks
            .iter()
            .copied()
            .filter(|(x, y)| both.colour_at(*x, *y) != only_marked.colour_at(*x, *y))
            .collect();
        assert!(
            lost.is_empty(),
            "a drawing took {} of the {} pixels the {what} mark had: {:?}",
            lost.len(),
            marks.len(),
            &lost[..lost.len().min(6)]
        );
    }

    // And the drawing really is there, away from the mark.
    assert!(
        !ink_of(
            &render(&mut renderer, &with, Marked::Nothing),
            SKETCH_COLOUR
        )
        .is_empty(),
        "there is no drawing in this frame, so the gate proves nothing"
    );
}

#[test]
fn two_frames_of_one_drawing_are_the_same_bytes() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = one_quad(200, 200, 0.0);
    let prepared = prepared_with(
        &mut renderer,
        &snapshot,
        &[square_drawing(6.0, -3.0, SketchStyle::Model)],
    );
    let first = drawn(&mut renderer, &prepared, &camera);
    let second = drawn(&mut renderer, &prepared, &camera);
    assert_eq!(first.colour(), second.colour());
    assert!(!ink_of(&first, SKETCH_COLOUR).is_empty());
}

#[test]
fn a_picture_with_nothing_to_draw_beside_it_is_the_frame_it_always_was() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, camera) = one_quad(200, 200, 0.0);

    // Never asked about drawings at all.
    let untouched = renderer
        .prepare(Arc::clone(&snapshot))
        .expect("the picture uploads");
    let before = drawn(&mut renderer, &untouched, &camera);

    // Asked, and told there are none.
    let empty = prepared_with(&mut renderer, &snapshot, &[]);
    let after = drawn(&mut renderer, &empty, &camera);
    assert_eq!(
        before.colour(),
        after.colour(),
        "an empty list of drawings changed the picture"
    );

    // And a list of drawings that are themselves empty.
    let nothing = SketchDrawingBuilder::new().build();
    let vacant = prepared_with(&mut renderer, &snapshot, &[nothing]);
    let again = drawn(&mut renderer, &vacant, &camera);
    assert_eq!(before.colour(), again.colour());
}

#[test]
fn a_camera_that_moved_and_a_frame_that_was_drawn_again_upload_nothing() {
    let mut renderer = renderer_or_skip!();
    let (snapshot, mut camera) = one_quad(200, 200, 0.0);
    let prepared = prepared_with(
        &mut renderer,
        &snapshot,
        &[square_drawing(6.0, -3.0, SketchStyle::Model)],
    );
    let uploads = renderer.sketch_uploads();
    let geometry = renderer.geometry_uploads();
    assert_eq!(uploads, 1, "the drawings were uploaded once");

    for step in 0..20 {
        camera.orbit(0.05, 0.02);
        camera.zoom(0.01);
        let frame = drawn(&mut renderer, &prepared, &camera);
        assert!(
            !ink_of(&frame, SKETCH_COLOUR).is_empty(),
            "the drawing left the screen on step {step}"
        );
    }

    assert_eq!(
        renderer.sketch_uploads(),
        uploads,
        "drawing twenty frames uploaded the drawings again"
    );
    assert_eq!(
        renderer.geometry_uploads(),
        geometry,
        "drawing twenty frames uploaded the model again"
    );
}

#[test]
fn drawings_belong_to_the_device_that_prepared_them() {
    let mut one = renderer_or_skip!();
    let mut other = renderer_or_skip!();
    let (snapshot, _camera) = one_quad(64, 64, 0.0);
    let prepared = one.prepare(Arc::clone(&snapshot)).expect("uploads");
    let refusal = other
        .prepare_sketches(prepared, &[square_drawing(4.0, 0.0, SketchStyle::Model)])
        .expect_err("another device's buffers are not this device's");
    assert!(refusal.to_string().contains("cannot be drawn by"));
}
