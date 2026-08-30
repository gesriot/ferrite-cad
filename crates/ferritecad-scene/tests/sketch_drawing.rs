// SPDX-License-Identifier: MIT
//! Turning the drawings a load carries into something a viewport can draw.
//!
//! The conversion lives in `ferritecad-scene` because that is the one crate
//! that already knows both what an evaluation produced and what a viewport
//! consumes. These gates are about the arithmetic of it and nothing else:
//! there is no device here, no camera and no pixel, so what a plane means,
//! what a circle is and which way an arc runs are settled before anything
//! draws them.
//!
//! Everything goes through [`snapshot_of`] against a real file, so the drawing
//! measured here is the drawing a window would be handed.
//!
//! # What is deliberately not reachable from here
//!
//! A sketch whose profile does not build makes no scene at all – the rebuild
//! refuses before a drawing exists – so a *model* circle or point cannot be
//! loaded from a document in this slice. Both are drawn by the render input
//! and by the device all the same, and the gates for that live where a
//! drawing can be built directly. Here they appear as construction geometry,
//! which is the only way a document can hold them today.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::Path;

use ferritecad_document::{
    Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression, Extrude,
    ObjectPayload, Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
};
use ferritecad_exchange::Import;
use ferritecad_kernel::{OperationContext, TessellationParams, mock::MockKernel};
use ferritecad_scene::{
    CIRCLE_SEGMENTS, LoadedScene, sketch_drawing, sketch_drawings, snapshot_of,
};
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId, Transform, Vec3};
use ferritecad_viewport::{SketchDrawing, SketchStyle};

/// How close a converted coordinate must land to the arithmetic it describes.
///
/// Positions are packed as `f32`, so this is the width of that type near the
/// tens of millimetres these drawings live at, with a little room for the
/// double-precision trigonometry that produced them.
const PLACED: f32 = 1.0e-3;

fn no_step(_: &mut MockKernel, _: &[u8]) -> Result<Import> {
    Err(CadError::unsupported(
        "this gate's documents hold no imported geometry",
    ))
}

fn point2(x: f64, y: f64) -> Point2 {
    Point2::new(x, y).expect("finite")
}

fn line(start: (f64, f64), end: (f64, f64)) -> SketchCurve {
    SketchCurve {
        id: StableEntityId::new(),
        construction: false,
        geometry: SketchGeometry::Line {
            start: point2(start.0, start.1),
            end: point2(end.0, end.1),
        },
    }
}

fn guide(geometry: SketchGeometry) -> SketchCurve {
    SketchCurve {
        id: StableEntityId::new(),
        construction: true,
        geometry,
    }
}

/// A closed loop of three lines and one arc.
///
/// The arc runs from angle zero to `pi` about `(25, 30)` with a radius of
/// twenty-five, which puts its ends exactly on the two corners the lines
/// leave, and bulges away from the rectangle in between.
fn three_lines_and_an_arc(sweep: f64) -> Vec<SketchCurve> {
    vec![
        line((0.0, 0.0), (50.0, 0.0)),
        line((50.0, 0.0), (50.0, 30.0)),
        SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Arc {
                center: point2(25.0, 30.0),
                radius: 25.0,
                start_angle: 0.0,
                end_angle: sweep,
            },
        },
        line((0.0, 30.0), (0.0, 0.0)),
    ]
}

/// One sketch, written with the datum placement and curves a gate chose.
fn write(path: &Path, placement: Transform, curves: Vec<SketchCurve>, extruded: bool) {
    let mut document = Document::create(path).expect("creates");
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let extrude = ObjectId::new();
    let body = ObjectId::new();
    document
        .write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("Datum"),
                &ObjectPayload::DatumPlane(DatumPlane { placement }),
            )?;
            w.put_object(
                sketch,
                None,
                1,
                Some("Drawing"),
                &ObjectPayload::Sketch(Sketch {
                    plane,
                    curves,
                    constraints: Vec::new(),
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: sketch,
                dependency: plane,
                role: DependencyRole::Plane,
            })?;
            if extruded {
                w.put_object(
                    extrude,
                    None,
                    2,
                    Some("Extrude"),
                    &ObjectPayload::Extrude(Extrude {
                        profile: sketch,
                        end_condition: EndCondition::Blind {
                            distance: Expression::constant(10.0)?,
                        },
                        reversed: false,
                        operation: SolidOperation::NewBody,
                        target_body: None,
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: extrude,
                    dependency: sketch,
                    role: DependencyRole::Profile,
                })?;
                w.put_object(
                    body,
                    None,
                    3,
                    Some("Plate"),
                    &ObjectPayload::Body(Body {
                        tip_feature: Some(extrude),
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: body,
                    dependency: extrude,
                    role: DependencyRole::BodyTip,
                })?;
            }
            Ok(())
        })
        .expect("populates");
    document.close().expect("closes");
}

fn load(path: &Path) -> LoadedScene {
    snapshot_of(
        path,
        &mut MockKernel::new(),
        no_step,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the document loads")
}

/// The one drawing a document of one sketch produces.
fn only(path: &Path) -> SketchDrawing {
    let loaded = load(path);
    let drawings = sketch_drawings(&loaded.sketch_presentations).expect("the drawing converts");
    assert_eq!(drawings.len(), 1, "one sketch, one drawing");
    drawings.into_iter().next().expect("just counted")
}

/// A directory that lives as long as the gate does.
fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary directory is available")
}

fn near(actual: [f32; 3], wanted: [f64; 3]) -> bool {
    (0..3).all(|axis| (actual[axis] - wanted[axis] as f32).abs() <= PLACED)
}

/// Whether any point of any stroke or point of `drawing` is at `wanted`.
fn touches(drawing: &SketchDrawing, wanted: [f64; 3]) -> bool {
    drawing
        .strokes()
        .iter()
        .flat_map(|stroke| stroke.points())
        .chain(
            drawing
                .points()
                .iter()
                .map(|point| point.at())
                .collect::<Vec<_>>()
                .iter(),
        )
        .any(|point| near(*point, wanted))
}

#[test]
fn every_kind_of_curve_a_document_can_hold_becomes_something_to_draw() {
    let directory = scratch();
    let path = directory.path().join("kinds.fcad");
    let mut curves = three_lines_and_an_arc(std::f64::consts::PI);
    curves.push(guide(SketchGeometry::Point {
        at: point2(5.0, 5.0),
    }));
    curves.push(guide(SketchGeometry::Circle {
        center: point2(25.0, 15.0),
        radius: 8.0,
    }));
    curves.push(guide(SketchGeometry::Arc {
        center: point2(40.0, 15.0),
        radius: 4.0,
        start_angle: 0.0,
        end_angle: std::f64::consts::FRAC_PI_2,
    }));
    write(&path, Transform::IDENTITY, curves, true);
    let drawing = only(&path);

    // Four kinds in, four kinds out. The point is the only one that is not a
    // run, because a point has no direction to run in.
    assert_eq!(drawing.points().len(), 1, "the point became something else");
    assert!(
        near(drawing.points()[0].at(), [5.0, 5.0, 0.0]),
        "the point is at {:?}",
        drawing.points()[0].at()
    );
    assert_eq!(
        drawing.strokes().len(),
        6,
        "three lines, a model arc, a construction circle and a construction arc"
    );

    // The lines, at their own ends.
    assert!(touches(&drawing, [0.0, 0.0, 0.0]));
    assert!(touches(&drawing, [50.0, 0.0, 0.0]));
    assert!(touches(&drawing, [50.0, 30.0, 0.0]));
    assert!(touches(&drawing, [0.0, 30.0, 0.0]));

    // The model arc, at the top of its bulge: centre (25, 30), radius 25, half
    // way round from angle zero to pi is straight up.
    assert!(
        touches(&drawing, [25.0, 55.0, 0.0]),
        "the arc does not pass through the top of its own sweep"
    );

    // The circle, at the four points its radius puts it.
    for wanted in [
        [33.0, 15.0, 0.0],
        [25.0, 23.0, 0.0],
        [17.0, 15.0, 0.0],
        [25.0, 7.0, 0.0],
    ] {
        assert!(touches(&drawing, wanted), "the circle misses {wanted:?}");
    }

    // The construction arc, at both of the angles the document stores.
    assert!(
        touches(&drawing, [44.0, 15.0, 0.0]),
        "the arc misses its start"
    );
    assert!(
        touches(&drawing, [40.0, 19.0, 0.0]),
        "the arc misses its end"
    );
}

#[test]
fn a_circle_is_closed_and_is_not_an_arc_that_nearly_meets_itself() {
    let directory = scratch();
    let path = directory.path().join("circle.fcad");
    let mut curves = three_lines_and_an_arc(std::f64::consts::PI);
    curves.push(guide(SketchGeometry::Circle {
        center: point2(25.0, 15.0),
        radius: 8.0,
    }));
    write(&path, Transform::IDENTITY, curves, true);
    let drawing = only(&path);

    let circle = drawing
        .strokes()
        .iter()
        .find(|stroke| stroke.points().len() == CIRCLE_SEGMENTS as usize + 1)
        .expect("the circle is the only run sampled a whole turn's worth");
    let first = circle.points().first().copied().expect("sampled");
    let last = circle.points().last().copied().expect("sampled");
    assert_eq!(
        first, last,
        "a circle that ends a rounding away from where it began leaves a gap no tolerance explains"
    );

    // And every sample really is on the circle, so it is a circle and not a
    // polygon that happens to close.
    for point in circle.points() {
        let radius = ((point[0] - 25.0).powi(2) + (point[1] - 15.0).powi(2)).sqrt();
        assert!(
            (radius - 8.0).abs() <= PLACED,
            "a sample sits at radius {radius} rather than eight"
        );
        assert!(point[2].abs() <= PLACED, "the circle left its own plane");
    }
}

#[test]
fn an_arc_runs_the_way_its_angles_say_and_not_the_other_way() {
    let directory = scratch();
    let forwards = directory.path().join("forwards.fcad");
    let backwards = directory.path().join("backwards.fcad");
    write(
        &forwards,
        Transform::IDENTITY,
        three_lines_and_an_arc(std::f64::consts::PI),
        true,
    );
    write(
        &backwards,
        Transform::IDENTITY,
        three_lines_and_an_arc(-std::f64::consts::PI),
        true,
    );

    let one = only(&forwards);
    let other = only(&backwards);

    // Both arcs join the same two corners, so an arc drawn without regard for
    // its direction would pass both of these.
    assert!(touches(&one, [50.0, 30.0, 0.0]) && touches(&one, [0.0, 30.0, 0.0]));
    assert!(touches(&other, [50.0, 30.0, 0.0]) && touches(&other, [0.0, 30.0, 0.0]));

    // They differ entirely in between: one bulges away from the rectangle and
    // the other through it.
    assert!(
        touches(&one, [25.0, 55.0, 0.0]),
        "the arc the document says runs counterclockwise does not"
    );
    assert!(
        touches(&other, [25.0, 5.0, 0.0]),
        "the arc the document says runs clockwise does not"
    );
    assert!(
        !touches(&one, [25.0, 5.0, 0.0]),
        "an arc was drawn the way it was not told to run"
    );
}

#[test]
fn a_drawing_sits_on_the_datum_the_sketch_is_actually_on() {
    let directory = scratch();
    let flat = directory.path().join("flat.fcad");
    let tilted = directory.path().join("tilted.fcad");
    write(
        &flat,
        Transform::IDENTITY,
        three_lines_and_an_arc(std::f64::consts::PI),
        true,
    );

    // A quarter turn about X, and then seven millimetres along Y: the plane's
    // normal is no longer Z and its origin is no longer the world's, so a
    // drawing that assumed the world XY plane cannot pass.
    let rotated =
        Transform::from_rotation(Vec3::X, std::f64::consts::FRAC_PI_2).expect("a quarter turn");
    let moved =
        Transform::from_translation(Vec3::new(0.0, 7.0, 0.0).expect("finite")).expect("finite");
    let placement = rotated.then(&moved).expect("composes");
    write(
        &tilted,
        placement,
        three_lines_and_an_arc(std::f64::consts::PI),
        true,
    );

    let on_the_floor = only(&flat);
    let on_the_wall = only(&tilted);

    assert!(touches(&on_the_floor, [50.0, 30.0, 0.0]));
    // The same corner of the same drawing, put where the datum actually is:
    // the plane's local y has become the world's z, and everything moved seven
    // millimetres along y.
    assert!(
        touches(&on_the_wall, [50.0, 7.0, 30.0]),
        "the drawing is not on the datum the sketch names"
    );
    assert!(
        !touches(&on_the_wall, [50.0, 30.0, 0.0]),
        "the drawing was put on the world XY plane rather than on its own"
    );
}

#[test]
fn what_guides_a_drawing_is_drawn_differently_from_what_bounds_a_face() {
    let directory = scratch();
    let path = directory.path().join("styles.fcad");
    let mut curves = three_lines_and_an_arc(std::f64::consts::PI);
    curves.push(guide(SketchGeometry::Circle {
        center: point2(25.0, 15.0),
        radius: 8.0,
    }));
    curves.push(guide(SketchGeometry::Point {
        at: point2(5.0, 5.0),
    }));
    write(&path, Transform::IDENTITY, curves, true);
    let drawing = only(&path);

    let styles: Vec<SketchStyle> = drawing.strokes().iter().map(|s| s.style()).collect();
    assert_eq!(
        styles.iter().filter(|s| **s == SketchStyle::Model).count(),
        4,
        "three lines and an arc bound the face"
    );
    assert_eq!(
        styles
            .iter()
            .filter(|s| **s == SketchStyle::Construction)
            .count(),
        1,
        "the circle only guides the drawing"
    );
    assert_eq!(drawing.points()[0].style(), SketchStyle::Construction);
}

#[test]
fn construction_geometry_reaches_the_drawing_rather_than_being_dropped_with_the_profile() {
    // The profile drops it, because it bounds no face. The drawing must not:
    // a person drew it, and it is on screen in every other CAD program.
    let directory = scratch();
    let path = directory.path().join("guides.fcad");
    let mut curves = three_lines_and_an_arc(std::f64::consts::PI);
    curves.push(guide(SketchGeometry::Line {
        start: point2(-20.0, -20.0),
        end: point2(-10.0, -20.0),
    }));
    write(&path, Transform::IDENTITY, curves, true);
    let drawing = only(&path);
    assert!(
        touches(&drawing, [-20.0, -20.0, 0.0]) && touches(&drawing, [-10.0, -20.0, 0.0]),
        "the drawing lost the geometry the profile is entitled to drop"
    );
}

#[test]
fn a_sketch_no_feature_reads_is_still_a_drawing() {
    let directory = scratch();
    let path = directory.path().join("unused.fcad");
    write(
        &path,
        Transform::IDENTITY,
        three_lines_and_an_arc(std::f64::consts::PI),
        false,
    );
    let loaded = load(&path);
    assert!(
        loaded.snapshot.is_empty(),
        "nothing was raised from this sketch, so there is no picture"
    );
    let drawing = only(&path);
    assert!(
        touches(&drawing, [50.0, 30.0, 0.0]),
        "a sketch nobody extruded lost its drawing"
    );
}

#[test]
fn a_drawing_carries_no_name_for_anything_and_no_order_that_could_become_one() {
    // Two documents holding the same shapes, one storing its curves in the
    // reverse order and both minting fresh identifiers. What is drawn is the
    // same geometry; nothing that comes out here is a name, so a renderer
    // cannot start answering with one.
    let directory = scratch();
    let one = directory.path().join("one.fcad");
    let other = directory.path().join("other.fcad");
    let mut curves = three_lines_and_an_arc(std::f64::consts::PI);
    curves.push(guide(SketchGeometry::Circle {
        center: point2(25.0, 15.0),
        radius: 8.0,
    }));
    let mut reversed = curves.clone();
    reversed.reverse();
    // Fresh identifiers, so nothing about either drawing can be the same
    // number twice.
    for curve in &mut reversed {
        curve.id = StableEntityId::new();
    }
    write(&one, Transform::IDENTITY, curves, true);
    write(&other, Transform::IDENTITY, reversed, true);

    let first = only(&one);
    let second = only(&other);

    // The same geometry is drawn, whatever it is called and whatever order it
    // is stored in.
    let mut here: Vec<usize> = first.strokes().iter().map(|s| s.points().len()).collect();
    let mut there: Vec<usize> = second.strokes().iter().map(|s| s.points().len()).collect();
    here.sort_unstable();
    there.sort_unstable();
    assert_eq!(here, there);
    for wanted in [[0.0, 0.0, 0.0], [50.0, 30.0, 0.0], [25.0, 55.0, 0.0]] {
        assert!(touches(&first, wanted) && touches(&second, wanted));
    }

    // And a drawing is drawn in the order the document stores it, which is
    // what a person sees, rather than in a chain order chosen for a kernel.
    assert_eq!(
        first.strokes()[0].points(),
        [[0.0, 0.0, 0.0], [50.0, 0.0, 0.0]],
        "the first run is not the first curve the document stores"
    );
}

#[test]
fn a_drawing_is_no_part_of_the_picture_it_travels_beside() {
    // Construction geometry a long way outside the profile. The picture is
    // what the solid is; the drawing is larger. If the two were one thing,
    // framing, hiding and the catalogue would all have quietly changed.
    let directory = scratch();
    let path = directory.path().join("outside.fcad");
    let mut curves = three_lines_and_an_arc(std::f64::consts::PI);
    curves.push(guide(SketchGeometry::Circle {
        center: point2(25.0, 15.0),
        radius: 500.0,
    }));
    write(&path, Transform::IDENTITY, curves, true);

    let loaded = load(&path);
    let (low, high) = loaded.snapshot.bounds().expect("the plate is somewhere");
    assert!(
        low[0] > -100.0 && high[0] < 100.0 && low[1] > -100.0 && high[1] < 100.0,
        "the picture's own extent grew to hold something nobody raised: {low:?} {high:?}"
    );

    let drawing = sketch_drawing(&loaded.sketch_presentations[0]).expect("converts");
    let widest = drawing
        .strokes()
        .iter()
        .flat_map(|stroke| stroke.points())
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        widest > 400.0,
        "the drawing lost the construction circle instead of the picture keeping it out"
    );
}

#[test]
fn a_construction_curve_changes_the_drawing_and_leaves_the_picture_byte_for_byte() {
    let directory = scratch();
    let plain = directory.path().join("plain.fcad");
    let guided = directory.path().join("guided.fcad");
    let curves = three_lines_and_an_arc(std::f64::consts::PI);
    let mut with_a_guide = curves.clone();
    with_a_guide.push(guide(SketchGeometry::Circle {
        center: point2(25.0, 15.0),
        radius: 8.0,
    }));
    write(&plain, Transform::IDENTITY, curves, true);
    write(&guided, Transform::IDENTITY, with_a_guide, true);

    let without = load(&plain);
    let with = load(&guided);

    assert_eq!(
        without.snapshot.bounds(),
        with.snapshot.bounds(),
        "a curve that bounds no face changed how large the picture is"
    );
    assert_eq!(
        without.snapshot.meshes().len(),
        with.snapshot.meshes().len()
    );
    for (a, b) in without.snapshot.meshes().iter().zip(with.snapshot.meshes()) {
        assert_eq!(
            bytemuck_bytes(a.vertices()),
            bytemuck_bytes(b.vertices()),
            "a curve that bounds no face changed the vertices the picture is drawn from"
        );
        assert_eq!(a.indices(), b.indices());
        assert_eq!(a.line_indices(), b.line_indices());
        assert_eq!(a.faces_of_vertices(), b.faces_of_vertices());
    }

    let one = sketch_drawing(&without.sketch_presentations[0]).expect("converts");
    let two = sketch_drawing(&with.sketch_presentations[0]).expect("converts");
    assert_ne!(
        one.strokes().len(),
        two.strokes().len(),
        "the drawing did not change, so this gate proves nothing about the picture"
    );
}

/// The bytes of a float slice, for comparing two pictures exactly.
fn bytemuck_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn a_coordinate_no_picture_can_hold_is_refused_rather_than_drawn_somewhere_else() {
    // Finite in the file and with no `f32` at all. A buffer full of infinities
    // draws nothing anywhere, which looks exactly like a renderer that was
    // never asked to draw.
    let directory = scratch();
    let path = directory.path().join("enormous.fcad");
    let mut curves = three_lines_and_an_arc(std::f64::consts::PI);
    curves.push(guide(SketchGeometry::Point {
        at: point2(1.0e300, 0.0),
    }));
    write(&path, Transform::IDENTITY, curves, true);

    let loaded = load(&path);
    let refusal = sketch_drawings(&loaded.sketch_presentations)
        .expect_err("a point with no f32 is not somewhere a picture can put it");
    assert_eq!(refusal.kind(), ferritecad_types::ErrorKind::Input);
}

#[test]
fn the_origin_is_an_ordinary_place_for_a_drawing_to_be() {
    let directory = scratch();
    let path = directory.path().join("zero.fcad");
    let mut curves = three_lines_and_an_arc(std::f64::consts::PI);
    curves.push(guide(SketchGeometry::Point {
        at: point2(0.0, 0.0),
    }));
    write(&path, Transform::IDENTITY, curves, true);
    let drawing = only(&path);
    assert!(
        drawing
            .points()
            .iter()
            .any(|point| near(point.at(), [0.0, 0.0, 0.0])),
        "a point at the origin was refused or moved"
    );
}
