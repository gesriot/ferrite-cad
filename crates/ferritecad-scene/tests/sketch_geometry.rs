// SPDX-License-Identifier: MIT
//! What a loaded scene carries of the drawings its solids were made from.
//!
//! A picture of a solid is not a picture of the sketch behind it. The profile
//! that raised the solid is less than the drawing on purpose — construction
//! geometry bounds no face, a chain is reordered head to tail, and a circle or
//! a point never reaches the kernel at all — so a renderer given only the
//! picture could not draw the sketch, and a renderer given only the file would
//! draw coordinates the constraints have already moved.
//!
//! These gates are about the fourth thing a load produces, beside the picture,
//! what a click means, and what each solve found out: every sketch of the
//! document, whole, at the coordinates its profile was actually built from.
//!
//! Everything here goes through [`snapshot_of`] against a real file. The gates
//! about solved coordinates need a real planegcs and say so; the rest hold in
//! every build, because a drawing nobody constrained is still a drawing and
//! must not start depending on whether a solver was linked.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::Path;

use ferritecad_document::{
    Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression, Extrude,
    ObjectPayload, Point2, Sketch, SketchConstraint, SketchConstraintRule, SketchCurve,
    SketchGeometry, SketchPointRef, SketchPointSelector, SolidOperation,
};
use ferritecad_exchange::Import;
use ferritecad_kernel::{OperationContext, TessellationParams, mock::MockKernel};
use ferritecad_scene::{LoadedScene, SketchPresentation, snapshot_of};
use ferritecad_sketch_solver as solver;
use ferritecad_types::{CadError, ObjectId, Point3, Result, StableEntityId, Transform, Vec3};

/// Where the four corners are stored. Fifty wide, and the constraints below
/// say sixty: a drawing carried at its stored coordinates cannot pass.
const STORED_CORNERS: [(f64, f64); 4] = [(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)];
/// What the width constraint says that width really is.
const WIDTH: f64 = 60.0;
/// How close a solved corner must land to where the constraints put it.
const PLACED: f64 = 1.0e-4;

fn ready() -> bool {
    if solver::is_available() {
        return true;
    }
    assert!(
        !solver::is_required(),
        "FERRITECAD_REQUIRE_PLANEGCS=1 was set, so no gate may skip: a scene that cannot be \
         loaded from a constrained document has not been shown to carry the drawing behind it"
    );
    eprintln!("skipped: this build has no sketch solver");
    false
}

macro_rules! solver_or_skip {
    () => {
        if !ready() {
            return;
        }
    };
}

fn params() -> TessellationParams {
    TessellationParams::default()
}

/// Refuses to read a STEP file, because these documents hold none.
fn no_step(_: &mut MockKernel, _: &[u8]) -> Result<Import> {
    Err(CadError::unsupported(
        "this gate's documents hold no imported geometry",
    ))
}

fn line(id: StableEntityId, start: (f64, f64), end: (f64, f64)) -> SketchCurve {
    SketchCurve {
        id,
        construction: false,
        geometry: SketchGeometry::Line {
            start: Point2::new(start.0, start.1).expect("finite"),
            end: Point2::new(end.0, end.1).expect("finite"),
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

fn at(curve: StableEntityId, selector: SketchPointSelector) -> SketchPointRef {
    SketchPointRef::new(curve, selector)
}

/// Four lines closing a rectangle at `corners`, head to tail.
fn square(corners: [(f64, f64); 4]) -> Vec<SketchCurve> {
    (0..4)
        .map(|index| {
            line(
                StableEntityId::new(),
                corners[index],
                corners[(index + 1) % corners.len()],
            )
        })
        .collect()
}

/// Nine constraints that close and square the plate without sizing it.
fn frame(edges: &[StableEntityId]) -> Vec<SketchConstraintRule> {
    use SketchPointSelector::{End, Start};
    vec![
        SketchConstraintRule::Coincident {
            a: at(edges[0], End),
            b: at(edges[1], Start),
        },
        SketchConstraintRule::Coincident {
            a: at(edges[1], End),
            b: at(edges[2], Start),
        },
        SketchConstraintRule::Coincident {
            a: at(edges[2], End),
            b: at(edges[3], Start),
        },
        SketchConstraintRule::Coincident {
            a: at(edges[3], End),
            b: at(edges[0], Start),
        },
        SketchConstraintRule::Fixed {
            point: at(edges[0], Start),
            x: 0.0,
            y: 0.0,
        },
        SketchConstraintRule::Horizontal {
            a: at(edges[0], Start),
            b: at(edges[0], End),
        },
        SketchConstraintRule::Vertical {
            a: at(edges[1], Start),
            b: at(edges[1], End),
        },
        SketchConstraintRule::Horizontal {
            a: at(edges[2], Start),
            b: at(edges[2], End),
        },
        SketchConstraintRule::Vertical {
            a: at(edges[3], Start),
            b: at(edges[3], End),
        },
    ]
}

fn width_of(edges: &[StableEntityId], distance: f64) -> SketchConstraintRule {
    SketchConstraintRule::Distance {
        a: at(edges[0], SketchPointSelector::Start),
        b: at(edges[0], SketchPointSelector::End),
        distance,
    }
}

fn named(rules: Vec<SketchConstraintRule>) -> Vec<SketchConstraint> {
    rules
        .into_iter()
        .map(|rule| SketchConstraint {
            id: StableEntityId::new(),
            rule,
        })
        .collect()
}

/// One sketch of a document to be written, named by the caller.
///
/// The identifier is the caller's so that two documents can hold the same
/// objects and differ in nothing else, and so that a gate can mint the
/// identifiers in one order and store them in another.
#[derive(Clone)]
struct Spec<'a> {
    id: ObjectId,
    name: Option<&'a str>,
    curves: Vec<SketchCurve>,
    constraints: Vec<SketchConstraint>,
    /// The extrudes that read this sketch and the body each one tips, named
    /// up front so that two documents built from one spec hold the same
    /// objects rather than two sets that merely look alike.
    features: Vec<(ObjectId, ObjectId)>,
}

impl<'a> Spec<'a> {
    fn new(name: Option<&'a str>, curves: Vec<SketchCurve>) -> Self {
        Self {
            id: ObjectId::new(),
            name,
            curves,
            constraints: Vec::new(),
            features: vec![(ObjectId::new(), ObjectId::new())],
        }
    }

    fn with(mut self, constraints: Vec<SketchConstraint>) -> Self {
        self.constraints = constraints;
        self
    }

    fn extruded(mut self, extrudes: usize) -> Self {
        self.features = (0..extrudes)
            .map(|_| (ObjectId::new(), ObjectId::new()))
            .collect();
        self
    }
}

/// Writes a document holding one datum and the given sketches, in the order
/// they are given.
///
/// The ordinal a sketch is stored under follows that order and nothing else,
/// so a gate that hands the sketches over in one order and minted their
/// identifiers in another gets a file whose document order is not its
/// identifier order.
fn write(path: &Path, placement: Transform, sketches: &[Spec<'_>]) {
    let mut document = Document::create(path).expect("creates");
    let plane = ObjectId::new();

    document
        .write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("Datum"),
                &ObjectPayload::DatumPlane(DatumPlane { placement }),
            )?;
            let mut order = 1i64;
            for spec in sketches {
                w.put_object(
                    spec.id,
                    None,
                    order,
                    spec.name,
                    &ObjectPayload::Sketch(Sketch {
                        plane,
                        curves: spec.curves.clone(),
                        constraints: spec.constraints.clone(),
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: spec.id,
                    dependency: plane,
                    role: DependencyRole::Plane,
                })?;
                order += 1;
                for (index, (extrude, body)) in spec.features.iter().copied().enumerate() {
                    w.put_object(
                        extrude,
                        None,
                        order,
                        Some("Extrude"),
                        &ObjectPayload::Extrude(Extrude {
                            profile: spec.id,
                            end_condition: EndCondition::Blind {
                                // Different depths so two extrudes of one
                                // sketch are two solids rather than one shape
                                // drawn twice.
                                distance: Expression::constant(10.0 + index as f64)?,
                            },
                            reversed: false,
                            operation: SolidOperation::NewBody,
                            target_body: None,
                        }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: extrude,
                        dependency: spec.id,
                        role: DependencyRole::Profile,
                    })?;
                    w.put_object(
                        body,
                        None,
                        order + 1,
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
                    order += 2;
                }
            }
            Ok(())
        })
        .expect("populates");
    document.close().expect("closes");
}

fn load(path: &Path) -> LoadedScene {
    let mut kernel = MockKernel::new();
    let loaded = snapshot_of(
        path,
        &mut kernel,
        no_step,
        &params(),
        &OperationContext::default(),
    )
    .expect("the document loads");
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a load kept shapes the session will never hear about again"
    );
    loaded
}

/// The one drawing a scene carries, when a gate wrote one sketch.
fn only(loaded: &LoadedScene) -> &SketchPresentation {
    assert_eq!(
        loaded.sketch_presentations.len(),
        1,
        "one sketch, one drawing: {:?}",
        loaded.sketch_presentations
    );
    &loaded.sketch_presentations[0]
}

/// The ends of one named line of a drawing.
fn ends(drawing: &SketchPresentation, id: StableEntityId) -> ((f64, f64), (f64, f64)) {
    let curve = drawing
        .curves()
        .iter()
        .find(|curve| curve.id() == id)
        .unwrap_or_else(|| panic!("the drawing carries no curve called {id}"));
    match curve.geometry() {
        SketchGeometry::Line { start, end } => ((start.x, start.y), (end.x, end.y)),
        other => panic!("curve {id} was stored as a line and came back as {other:?}"),
    }
}

fn near(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= PLACED
}

// ---------------------------------------------------------------------------
// Solved coordinates, not stored ones
// ---------------------------------------------------------------------------

#[test]
fn a_constrained_sketch_reaches_the_scene_at_the_coordinates_it_was_solved_to() {
    solver_or_skip!();

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    let curves = square(STORED_CORNERS);
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges, WIDTH));
    let spec = Spec::new(Some("Profile"), curves).with(named(rules));
    let sketch = spec.id;
    write(&path, Transform::IDENTITY, &[spec]);

    let loaded = load(&path);
    let drawing = only(&loaded);
    assert_eq!(drawing.sketch(), sketch);

    let (start, end) = ends(drawing, edges[0]);
    assert!(
        near(end.0 - start.0, WIDTH) || near(start.0 - end.0, WIDTH),
        "the first edge runs {start:?} to {end:?}, and its constraint says it is {WIDTH} long"
    );
    assert!(
        !near((end.0 - start.0).abs(), 50.0),
        "the drawing arrived at the width the file stores, so nothing solved it"
    );
}

#[test]
fn the_picture_and_the_drawing_are_the_same_answer() {
    solver_or_skip!();

    // The solid is raised from the profile and the drawing is carried beside
    // it. If they came from two answers they could disagree, and the way to
    // see that is to measure the same edge in both: the picture is packed out
    // of the coordinates the kernel was handed.
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    let curves = square(STORED_CORNERS);
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges, WIDTH));
    let spec = Spec::new(Some("Profile"), curves).with(named(rules));
    write(&path, Transform::IDENTITY, &[spec]);

    let loaded = load(&path);
    let (start, end) = ends(only(&loaded), edges[0]);
    let drawn_width = (end.0 - start.0).abs();

    let (low, high) = loaded
        .snapshot
        .bounds()
        .expect("the document draws one solid");
    let pictured_width = f64::from(high[0] - low[0]);

    assert!(
        near(pictured_width, drawn_width),
        "the solid is {pictured_width} wide and the drawing behind it is {drawn_width} wide, so \
         they are two answers"
    );
    assert!(
        near(pictured_width, WIDTH),
        "the solid is {pictured_width} wide, and the constraints say {WIDTH}"
    );
}

#[test]
fn an_unconstrained_sketch_is_carried_without_a_solver() {
    // No skip: this is the gate that must hold in a build that never linked
    // planegcs. A drawing with nothing to solve is still a drawing.
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plain.fcad");
    let curves = square(STORED_CORNERS);
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let spec = Spec::new(Some("Plain"), curves);
    write(&path, Transform::IDENTITY, &[spec]);

    let before = solver::native_solves();
    let loaded = load(&path);
    assert_eq!(
        solver::native_solves(),
        before,
        "an unconstrained sketch reached the solver"
    );
    assert!(
        loaded.sketch_solves.is_empty(),
        "nothing solved this sketch, so nothing may report on it"
    );

    let drawing = only(&loaded);
    for (index, id) in edges.iter().enumerate() {
        assert_eq!(
            ends(drawing, *id),
            (
                STORED_CORNERS[index],
                STORED_CORNERS[(index + 1) % STORED_CORNERS.len()]
            ),
            "line {index} came back as something other than what the file stores"
        );
    }
}

// ---------------------------------------------------------------------------
// Nothing of the drawing is lost on the way
// ---------------------------------------------------------------------------

#[test]
fn construction_geometry_of_every_kind_survives() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("guided.fcad");
    let mut curves = square(STORED_CORNERS);
    let point = guide(SketchGeometry::Point {
        at: Point2::new(5.0, 6.0).expect("finite"),
    });
    let rail = guide(SketchGeometry::Line {
        start: Point2::new(1.0, 2.0).expect("finite"),
        end: Point2::new(3.0, 4.0).expect("finite"),
    });
    let circle = guide(SketchGeometry::Circle {
        center: Point2::new(25.0, 15.0).expect("finite"),
        radius: 7.5,
    });
    let arc = guide(SketchGeometry::Arc {
        center: Point2::new(10.0, 20.0).expect("finite"),
        radius: 4.25,
        start_angle: 0.25,
        end_angle: 1.75,
    });
    let guides = [point, rail, circle, arc];
    curves.extend(guides.iter().cloned());
    let spec = Spec::new(Some("Guided"), curves);
    write(&path, Transform::IDENTITY, &[spec]);

    let loaded = load(&path);
    let drawing = only(&loaded);
    assert_eq!(
        drawing.curves().len(),
        8,
        "four model lines and four guides went in"
    );

    // A profile keeps none of these: a point and a circle it refuses outright,
    // and construction geometry it drops before it starts. Carrying the
    // drawing out of the profile instead of out of the sketch would lose all
    // four, and lose the flag that says what they are.
    for expected in &guides {
        let carried = drawing
            .curves()
            .iter()
            .find(|curve| curve.id() == expected.id)
            .unwrap_or_else(|| panic!("the drawing lost construction curve {}", expected.id));
        assert!(
            carried.is_construction(),
            "construction curve {} arrived as model geometry",
            expected.id
        );
        assert_eq!(
            *carried.geometry(),
            expected.geometry,
            "construction curve {} arrived as something else",
            expected.id
        );
    }

    for curve in drawing.curves().iter().take(4) {
        assert!(
            !curve.is_construction(),
            "a model line arrived as construction geometry"
        );
    }
}

#[test]
fn curve_identities_and_document_order_survive() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("shuffled.fcad");

    // Stored in an order that is not the order they chain in, because a sketch
    // stores its curves in presentation order. A drawing read out of the
    // profile would come back head to tail, which is somebody else's order.
    let mut curves = square(STORED_CORNERS);
    curves.swap(1, 3);
    let stored: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let spec = Spec::new(Some("Shuffled"), curves);
    write(&path, Transform::IDENTITY, &[spec]);

    let loaded = load(&path);
    let carried: Vec<StableEntityId> = only(&loaded)
        .curves()
        .iter()
        .map(|curve| curve.id())
        .collect();
    assert_eq!(
        carried, stored,
        "the drawing came back in an order the document does not store"
    );
}

#[test]
fn two_curves_that_draw_the_same_thing_are_two_curves() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("twins.fcad");
    let mut curves = square(STORED_CORNERS);
    let twin = SketchGeometry::Line {
        start: Point2::new(2.0, 2.0).expect("finite"),
        end: Point2::new(8.0, 2.0).expect("finite"),
    };
    let first = guide(twin.clone());
    let second = guide(twin);
    let ids = [first.id, second.id];
    curves.extend([first, second]);
    let spec = Spec::new(Some("Twins"), curves);
    write(&path, Transform::IDENTITY, &[spec]);

    let loaded = load(&path);
    let drawing = only(&loaded);
    assert_eq!(
        drawing.curves().len(),
        6,
        "two guides at the same coordinates were welded into one: {:?}",
        drawing.curves()
    );
    assert_ne!(ids[0], ids[1], "the document minted one identifier twice");
    for id in ids {
        assert!(
            drawing.curves().iter().any(|curve| curve.id() == id),
            "the drawing lost the guide called {id}"
        );
    }
}

#[test]
fn the_plane_the_sketch_was_actually_placed_on_is_carried() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("tilted.fcad");

    // A quarter turn about X and then a move, so neither the origin nor the
    // normal is the world XY plane's. A drawing that assumed the sketch's own
    // local frame would put this plate at the origin, flat.
    let placement = Transform::from_rotation(Vec3::X, std::f64::consts::FRAC_PI_2)
        .expect("a quarter turn about X")
        .then(
            &Transform::from_translation(Vec3::new(7.0, -3.0, 11.0).expect("finite"))
                .expect("a translation"),
        )
        .expect("a rigid placement");
    let spec = Spec::new(Some("Tilted"), square(STORED_CORNERS));
    write(&path, placement, &[spec]);

    let loaded = load(&path);
    let plane = only(&loaded).plane();

    assert_eq!(
        plane.origin(),
        Point3::new(7.0, -3.0, 11.0).expect("finite")
    );
    assert!(
        plane.normal().y < -0.999 && plane.normal().z.abs() < 1.0e-9,
        "a quarter turn about X points the normal along -Y, and this one is {:?}",
        plane.normal()
    );
    assert!(
        plane.x_axis().x > 0.999,
        "the local X axis was turned as well as the normal: {:?}",
        plane.x_axis()
    );
}

// ---------------------------------------------------------------------------
// One sketch is one drawing, and two sketches are two
// ---------------------------------------------------------------------------

#[test]
fn two_extrudes_of_one_sketch_give_one_drawing() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("twice.fcad");
    let spec = Spec::new(Some("Once"), square(STORED_CORNERS)).extruded(2);
    let sketch = spec.id;
    write(&path, Transform::IDENTITY, &[spec]);

    let loaded = load(&path);
    assert_eq!(
        loaded.catalogue.len(),
        2,
        "the document was written to draw two solids"
    );
    assert_eq!(
        loaded
            .sketch_presentations
            .iter()
            .map(|drawing| drawing.sketch())
            .collect::<Vec<_>>(),
        vec![sketch],
        "a sketch two features read was carried once per feature"
    );
}

#[test]
fn two_sketches_that_draw_the_same_thing_stay_two_drawings() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("pair.fcad");
    let first = Spec::new(Some("Left"), square(STORED_CORNERS));
    let second = Spec::new(Some("Right"), square(STORED_CORNERS));
    let ids = [first.id, second.id];
    write(&path, Transform::IDENTITY, &[first, second]);

    let loaded = load(&path);
    assert_eq!(
        loaded
            .sketch_presentations
            .iter()
            .map(|drawing| drawing.sketch())
            .collect::<Vec<_>>(),
        ids.to_vec(),
        "two sketches at the same coordinates became one drawing"
    );
}

#[test]
fn drawings_arrive_in_document_order() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("ordered.fcad");

    // Minted first, second, third and stored third, first, second. A drawing
    // list built out of a map would come back in the order the identifiers
    // were minted, which is close enough to document order to look right
    // whenever a gate happens to write them in the same order.
    let first = Spec::new(Some("First"), square(STORED_CORNERS));
    let second = Spec::new(Some("Second"), square(STORED_CORNERS));
    let third = Spec::new(Some("Third"), square(STORED_CORNERS));
    let minted = [first.id, second.id, third.id];
    let stored = [minted[2], minted[0], minted[1]];
    write(&path, Transform::IDENTITY, &[third, first, second]);

    let loaded = load(&path);
    assert_ne!(
        minted.to_vec(),
        stored.to_vec(),
        "the gate wrote the sketches in the order it minted them, so it proves nothing"
    );
    assert_eq!(
        loaded
            .sketch_presentations
            .iter()
            .map(|drawing| drawing.sketch())
            .collect::<Vec<_>>(),
        stored.to_vec(),
        "the drawings came back in an order the document does not store"
    );
}

// ---------------------------------------------------------------------------
// A load that fails publishes nothing, and one that succeeds changes no picture
// ---------------------------------------------------------------------------

#[test]
fn a_document_whose_constraints_disagree_publishes_no_drawing_at_all() {
    solver_or_skip!();

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("impossible.fcad");

    // The first sketch is ordinary and would have a drawing of its own; the
    // second cannot be solved. A load that published what it had got as far as
    // would hand out half a document.
    let good = Spec::new(Some("Fine"), square(STORED_CORNERS));
    let curves = square(STORED_CORNERS);
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges, WIDTH));
    rules.push(width_of(&edges, WIDTH + 15.0));
    let bad = Spec::new(Some("Impossible"), curves).with(named(rules));
    write(&path, Transform::IDENTITY, &[good, bad]);

    let mut kernel = MockKernel::new();
    let error = snapshot_of(
        &path,
        &mut kernel,
        no_step,
        &params(),
        &OperationContext::default(),
    )
    .expect_err("a plate that is two widths at once has no picture");

    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Constraint);
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a refused load left shapes behind"
    );
}

#[test]
fn carrying_the_drawings_changes_nothing_about_the_picture() {
    let plain = tempfile::tempdir().expect("a temporary directory is available");
    let guided = tempfile::tempdir().expect("a temporary directory is available");

    // The same objects under the same identifiers, twice, differing only in
    // four construction curves. Construction geometry bounds no face, so the
    // solid, the catalogue and every number the renderer uploads must be what
    // they were; the drawings must not be.
    let first = Spec::new(Some("Profile"), square(STORED_CORNERS));
    let mut second = first.clone();
    second.curves.extend([
        guide(SketchGeometry::Point {
            at: Point2::new(5.0, 6.0).expect("finite"),
        }),
        guide(SketchGeometry::Circle {
            center: Point2::new(25.0, 15.0).expect("finite"),
            radius: 7.5,
        }),
    ]);

    let bare = plain.path().join("bare.fcad");
    let extra = guided.path().join("extra.fcad");
    write(&bare, Transform::IDENTITY, &[first]);
    write(&extra, Transform::IDENTITY, &[second]);

    let without = load(&bare);
    let with = load(&extra);

    assert_eq!(
        without.snapshot.bounds(),
        with.snapshot.bounds(),
        "a construction curve moved the solid"
    );
    assert_eq!(
        without.snapshot.meshes().len(),
        with.snapshot.meshes().len(),
        "a construction curve packed another mesh"
    );
    assert_eq!(
        without.snapshot.draws().len(),
        with.snapshot.draws().len(),
        "a construction curve drew something"
    );
    assert_eq!(
        without.snapshot, with.snapshot,
        "the picture changed because the drawing beside it did"
    );
    assert_eq!(
        without.catalogue, with.catalogue,
        "the catalogue changed because the drawing beside it did"
    );
    assert_ne!(
        without.sketch_presentations, with.sketch_presentations,
        "the two documents draw different sketches and were carried as the same one"
    );
}

// ---------------------------------------------------------------------------
// What a drawing may not say
// ---------------------------------------------------------------------------

#[test]
fn a_drawing_names_nothing_that_lasts_one_solve() {
    solver_or_skip!();

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    let curves = square(STORED_CORNERS);
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges, WIDTH));
    let spec = Spec::new(Some("Profile"), curves).with(named(rules));
    let sketch = spec.id;
    write(&path, Transform::IDENTITY, &[spec]);

    let loaded = load(&path);
    let written = format!("{:?}", only(&loaded));

    // What it must say: the document's own words for the sketch and for every
    // curve of it, which is what a reader of a log can look up afterwards.
    assert!(written.contains(&sketch.to_string()));
    for id in &edges {
        assert!(
            written.contains(&id.to_string()),
            "the drawing does not name curve {id}"
        );
    }

    // And what it must not: anything a solver minted for the length of one
    // call, or anything a native library named itself.
    for forbidden in ["PointId", "ConstraintId", "planegcs", "GCS", "native"] {
        assert!(
            !written.contains(forbidden),
            "a drawing published {forbidden}, which means nothing after the solve that minted it:\
             \n{written}"
        );
    }
}
