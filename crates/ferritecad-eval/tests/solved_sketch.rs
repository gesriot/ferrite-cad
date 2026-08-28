// SPDX-License-Identifier: MIT
//! A constrained sketch, from the bytes on disk to the request the kernel gets.
//!
//! The whole run, with nothing standing in for anything: a real [`Document`]
//! written to a real file, read back cold, translated into the product sketch
//! solver's terms, solved by planegcs, and only then turned into a profile and
//! an [`ExtrudeRequest`]. The plate is stored at coordinates that are wrong on
//! purpose — its corners do not even meet — so a build that ignored the
//! constraints could not accidentally produce the right answer, and a build
//! that used the stored coordinates could not produce any answer at all.
//!
//! The gates that hold without a library are the ones about refusing: an
//! unconstrained sketch never asks a solver anything, and a build with no
//! solver refuses before the kernel is touched. Those run everywhere. The
//! gates about a solved answer need a real planegcs and say so.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::sync::{Arc, Mutex};

use ferritecad_document::{
    Body, CacheStore, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression,
    Extrude, ObjectPayload, Point2, Sketch, SketchConstraint, SketchConstraintRule, SketchCurve,
    SketchGeometry, SketchPointRef, SketchPointSelector, SketchSegmentRef, SolidOperation,
};
use ferritecad_eval::{rebuild_cached, rebuild_cold};
use ferritecad_kernel::{
    ArchiveSlot, BrepBlob, ExtrudeRequest, ExtrudeResult, GeometryKernel, KernelIdentity, Mesh,
    OperationContext, OperationResult, ShapeHandle, SubShapeHandle, TessellationParams,
    mock::MockKernel,
};
use ferritecad_sketch_solver as solver;
use ferritecad_types::{CadError, ErrorKind, ObjectId, Result, StableEntityId, Transform};
use tempfile::TempDir;

/// The plate the constraints describe: 60 by 40, cornered at the origin.
const WIDTH: f64 = 60.0;
const HEIGHT: f64 = 40.0;

/// How close a solved corner must land to where the constraints put it.
///
/// Two orders of magnitude above the solver's own residual limit and six below
/// anything a millimetre-scale part could mean, so this measures that the
/// constraints were solved rather than how tightly.
const PLACED: f64 = 1.0e-4;

/// Leaves the caller when this build has no planegcs, and refuses to under
/// required mode.
///
/// The same rule the solver crate's own product gates hold themselves to: a
/// run whose job is to prove the rebuild path solves sketches cannot pass by
/// not having a solver.
fn ready() -> bool {
    if solver::is_available() {
        return true;
    }
    assert!(
        !solver::is_required(),
        "FERRITECAD_REQUIRE_PLANEGCS=1 was set, so no gate may skip: this build has no sketch \
         solver, and a rebuild that cannot solve has not been shown to solve"
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

// ---------------------------------------------------------------------------
// A kernel that keeps what it was asked to build
// ---------------------------------------------------------------------------

/// A [`MockKernel`] that also remembers every request it was handed.
///
/// The claim under test is not that a rebuild succeeded but that what reached
/// the kernel was built from the solved coordinates. A result read back from
/// the rebuild would be this crate's own account of that; the request the
/// kernel received is the kernel's.
#[derive(Debug)]
struct RecordingKernel {
    inner: MockKernel,
    seen: Arc<Mutex<Vec<ExtrudeRequest>>>,
}

impl RecordingKernel {
    fn new() -> Self {
        Self {
            inner: MockKernel::new(),
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn requests(&self) -> Vec<ExtrudeRequest> {
        self.seen
            .lock()
            .expect("no test thread panics here")
            .clone()
    }

    fn extrude_count(&self) -> u64 {
        self.inner.extrude_count()
    }

    fn live_shape_count(&self) -> usize {
        self.inner.live_shape_count()
    }
}

impl GeometryKernel for RecordingKernel {
    fn identity(&self) -> &KernelIdentity {
        self.inner.identity()
    }

    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<ExtrudeResult> {
        self.seen
            .lock()
            .expect("no test thread panics here")
            .push(request.clone());
        self.inner.extrude(request, context)
    }

    fn transform(
        &mut self,
        shape: ShapeHandle,
        transform: &Transform,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.inner.transform(shape, transform, context)
    }

    fn tessellate(
        &mut self,
        shape: ShapeHandle,
        params: &TessellationParams,
        context: &OperationContext,
    ) -> Result<Mesh> {
        self.inner.tessellate(shape, params, context)
    }

    fn encode_shape_with(
        &mut self,
        shape: ShapeHandle,
        sub_shapes: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
        self.inner.encode_shape_with(shape, sub_shapes)
    }

    fn decode_shape_with(
        &mut self,
        blob: &BrepBlob,
        slots: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        self.inner.decode_shape_with(blob, slots)
    }

    fn encode_shape(&mut self, shape: ShapeHandle) -> Result<BrepBlob> {
        self.inner.encode_shape(shape)
    }

    fn decode_shape(&mut self, blob: &BrepBlob) -> Result<ShapeHandle> {
        self.inner.decode_shape(blob)
    }

    fn release(&mut self, shape: ShapeHandle) {
        self.inner.release(shape)
    }
}

// ---------------------------------------------------------------------------
// The plate
// ---------------------------------------------------------------------------

/// What the document holds: four lines, none of which meets the next.
///
/// Every gap is a fifth of a millimetre or more, which is hundreds of
/// thousands of times the tolerance the profile chain joins at. A build that
/// extruded these coordinates would not produce a slightly wrong plate; it
/// would produce no plate at all, and say the profile does not close. That is
/// what makes "the kernel got the solved coordinates" a claim this file can
/// hold rather than assume.
const STORED: [((f64, f64), (f64, f64)); 4] = [
    ((0.5, -0.3), (59.2, 0.4)),
    ((59.4, 0.6), (60.6, 39.5)),
    ((60.3, 39.8), (-0.4, 40.3)),
    ((-0.2, 40.1), (0.3, 0.2)),
];

/// Where the constraints say those four lines belong.
const SOLVED: [((f64, f64), (f64, f64)); 4] = [
    ((0.0, 0.0), (WIDTH, 0.0)),
    ((WIDTH, 0.0), (WIDTH, HEIGHT)),
    ((WIDTH, HEIGHT), (0.0, HEIGHT)),
    ((0.0, HEIGHT), (0.0, 0.0)),
];

fn line(id: StableEntityId, start: (f64, f64), end: (f64, f64)) -> Result<SketchCurve> {
    Ok(SketchCurve {
        id,
        construction: false,
        geometry: SketchGeometry::Line {
            start: Point2::new(start.0, start.1)?,
            end: Point2::new(end.0, end.1)?,
        },
    })
}

fn at(curve: StableEntityId, selector: SketchPointSelector) -> SketchPointRef {
    SketchPointRef::new(curve, selector)
}

/// The eleven constraints that turn four loose lines into that plate.
///
/// Exactly sixteen equations over sixteen coordinates: four coincidences close
/// the corners, one pin puts the first corner at the origin, two horizontals
/// and two verticals square it, and two distances size it. Nothing here is
/// satisfied by the stored coordinates.
fn plate_constraints(edges: &[StableEntityId], ids: &[StableEntityId]) -> Vec<SketchConstraint> {
    use SketchPointSelector::{End, Start};
    let rules = [
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
        SketchConstraintRule::Distance {
            a: at(edges[0], Start),
            b: at(edges[0], End),
            distance: WIDTH,
        },
        SketchConstraintRule::Distance {
            a: at(edges[1], Start),
            b: at(edges[1], End),
            distance: HEIGHT,
        },
    ];

    ids.iter()
        .copied()
        .zip(rules)
        .map(|(id, rule)| SketchConstraint { id, rule })
        .collect()
}

/// Every identifier the fixture hands out, so a gate can name one afterwards.
struct Plate {
    extrude: ObjectId,
    edges: Vec<StableEntityId>,
    constraints: Vec<StableEntityId>,
}

/// Writes the plate to a real file.
///
/// `curves` and `constraints` are given rather than built here so that a gate
/// can reorder either one and get a document that differs in storage order and
/// in nothing else.
fn write_plate(
    dir: &TempDir,
    curves: Vec<SketchCurve>,
    constraints: Vec<SketchConstraint>,
    extra_extrude: bool,
) -> (Document, Plate) {
    let mut document = Document::create(dir.path().join("part.fcad")).expect("creates");

    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let extrude = ObjectId::new();
    let second = ObjectId::new();
    let body = ObjectId::new();

    let edges: Vec<StableEntityId> = curves.iter().map(|c| c.id).collect();
    let constraint_ids: Vec<StableEntityId> = constraints.iter().map(|c| c.id).collect();

    document
        .write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("XY"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            )?;
            w.put_object(
                sketch,
                None,
                1,
                Some("Profile"),
                &ObjectPayload::Sketch(Sketch {
                    plane,
                    curves: curves.clone(),
                    constraints: constraints.clone(),
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: sketch,
                dependency: plane,
                role: DependencyRole::Plane,
            })?;
            w.put_object(
                extrude,
                None,
                2,
                Some("Extrude1"),
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
            if extra_extrude {
                // A second feature reading the same sketch. It is the whole of
                // the "solved once" claim: two consumers, one sketch.
                w.put_object(
                    second,
                    None,
                    3,
                    Some("Extrude2"),
                    &ObjectPayload::Extrude(Extrude {
                        profile: sketch,
                        end_condition: EndCondition::Blind {
                            distance: Expression::constant(4.0)?,
                        },
                        reversed: true,
                        operation: SolidOperation::NewBody,
                        target_body: None,
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: second,
                    dependency: sketch,
                    role: DependencyRole::Profile,
                })?;
            }
            w.put_object(
                body,
                None,
                4,
                Some("Plate"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(extrude),
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: body,
                dependency: extrude,
                role: DependencyRole::BodyTip,
            })
        })
        .expect("populates");

    (
        document,
        Plate {
            extrude,
            edges,
            constraints: constraint_ids,
        },
    )
}

/// The sidecar this document's cached rebuilds use.
fn sidecar(dir: &TempDir, document: &Document, kernel: &impl GeometryKernel) -> CacheStore {
    CacheStore::open(
        dir.path().join("part.fcad-cache"),
        document.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("the sidecar opens")
}

fn plate_curves() -> Vec<SketchCurve> {
    STORED
        .iter()
        .map(|(start, end)| line(StableEntityId::new(), *start, *end).expect("finite"))
        .collect()
}

/// The plate as stored, ready to be read back cold.
fn plate(dir: &TempDir) -> (Document, Plate) {
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|c| c.id).collect();
    let ids: Vec<StableEntityId> = (0..11).map(|_| StableEntityId::new()).collect();
    let constraints = plate_constraints(&edges, &ids);
    write_plate(dir, curves, constraints, false)
}

/// Reads the document back from its own bytes.
///
/// The gate is about a cold rebuild of a stored document, so the value the
/// rebuild sees must come from the file rather than from the handle that just
/// wrote it.
fn reopen(document: &Document) -> Document {
    Document::open(document.path()).expect("reopens what was written")
}

/// Where a rebuild's profile put each of the four corners, in stored order.
fn corners(profile: &ferritecad_kernel::Profile, edges: &[StableEntityId]) -> Vec<(f64, f64)> {
    edges
        .iter()
        .map(|edge| {
            let segment = profile
                .outer()
                .segments()
                .iter()
                .find(|s| s.label == *edge)
                .unwrap_or_else(|| panic!("the profile has no segment for {edge}"));
            let start = segment.geometry.start().expect("a line has a start");
            (start.x, start.y)
        })
        .collect()
}

fn near(actual: (f64, f64), expected: (f64, f64)) -> bool {
    (actual.0 - expected.0).abs() <= PLACED && (actual.1 - expected.1).abs() <= PLACED
}

// ---------------------------------------------------------------------------
// The end-to-end gate
// ---------------------------------------------------------------------------

#[test]
fn a_stored_constrained_plate_reaches_the_kernel_at_its_solved_coordinates() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let (written, plate) = plate(&dir);
    let document = reopen(&written);
    drop(written);

    let mut kernel = RecordingKernel::new();
    let before = solver::native_solves();

    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a constrained plate whose constraints can be met must rebuild");

    assert_eq!(
        solver::native_solves() - before,
        1,
        "a rebuild of one constrained sketch must cross into planegcs exactly once"
    );

    let requests = kernel.requests();
    assert_eq!(requests.len(), 1, "one extrude, one request");
    let profile = requests[0].profile();
    assert_eq!(profile.outer().segments().len(), 4);

    // Read from the request the kernel was handed, not from anything this
    // crate kept: the claim is about what the kernel got.
    for (index, corner) in corners(profile, &plate.edges).into_iter().enumerate() {
        assert!(
            near(corner, SOLVED[index].0),
            "segment {index} starts at {corner:?}, and the constraints put it at {:?}",
            SOLVED[index].0
        );
        assert!(
            !near(corner, STORED[index].0),
            "segment {index} arrived at its stored coordinates, so nothing solved it"
        );
    }

    assert_eq!(kernel.extrude_count(), 1);
    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn two_features_reading_one_sketch_solve_it_once() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|c| c.id).collect();
    let ids: Vec<StableEntityId> = (0..11).map(|_| StableEntityId::new()).collect();
    let (written, _plate) = write_plate(&dir, curves, plate_constraints(&edges, &ids), true);
    let document = reopen(&written);
    drop(written);

    let mut kernel = MockKernel::new();
    let before = solver::native_solves();

    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("two features over one sketch rebuild");

    assert_eq!(kernel.extrude_count(), 2, "both features must be built");
    assert_eq!(
        solver::native_solves() - before,
        1,
        "the sketch is one sketch; a second consumer must not buy a second solve"
    );
    built.release_all(&mut kernel);
}

#[test]
fn an_unconstrained_sketch_never_asks_a_solver_anything() {
    // Holds in every build, with or without a library. A sketch that says
    // nothing about its constraints has nothing to solve, and reaching for a
    // solver at all would make an ordinary document's rebuild depend on
    // whether one was linked.
    let dir = tempfile::tempdir().expect("temp dir");
    let curves: Vec<SketchCurve> = SOLVED
        .iter()
        .map(|(start, end)| line(StableEntityId::new(), *start, *end).expect("finite"))
        .collect();
    let (written, _plate) = write_plate(&dir, curves, Vec::new(), false);
    let document = reopen(&written);
    drop(written);

    let mut kernel = MockKernel::new();
    let before = solver::native_solves();

    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("an unconstrained plate rebuilds as it always did");

    assert_eq!(
        solver::native_solves(),
        before,
        "an unconstrained sketch reached the solver"
    );
    assert_eq!(kernel.extrude_count(), 1);
    built.release_all(&mut kernel);
}

#[test]
fn a_build_with_no_solver_refuses_before_the_kernel() {
    if solver::is_available() {
        eprintln!("skipped: this build has a sketch solver, so it has nothing to refuse for");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let (written, _plate) = plate(&dir);
    let document = reopen(&written);
    drop(written);

    let mut kernel = MockKernel::new();
    let error = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("with no solver there is nothing that could have solved this sketch");

    assert_eq!(
        error.kind(),
        ErrorKind::Unsupported,
        "no solver is a missing component, not a wrong model: {error}"
    );
    assert_eq!(
        kernel.extrude_count(),
        0,
        "the kernel was asked to build something the solver never saw"
    );
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a refused rebuild left shapes behind"
    );
}

#[test]
fn a_conflict_is_named_in_the_documents_own_constraint_identifiers() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|c| c.id).collect();
    let ids: Vec<StableEntityId> = (0..12).map(|_| StableEntityId::new()).collect();
    let mut constraints = plate_constraints(&edges, &ids[..11]);
    // A second width, disagreeing with the first. Nothing else changes.
    let contradiction = ids[11];
    constraints.push(SketchConstraint {
        id: contradiction,
        rule: SketchConstraintRule::Distance {
            a: at(edges[0], SketchPointSelector::Start),
            b: at(edges[0], SketchPointSelector::End),
            distance: WIDTH + 15.0,
        },
    });

    let (written, _plate) = write_plate(&dir, curves, constraints, false);
    let document = reopen(&written);
    drop(written);

    let mut kernel = MockKernel::new();
    let live_before = solver::native_live_sessions();
    let error = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("a plate that is both 60 and 75 wide cannot be built");

    assert_eq!(
        error.kind(),
        ErrorKind::Constraint,
        "a sketch that cannot hold is a constraint failure, not a missing feature: {error}"
    );

    let message = error.to_string();
    // Whichever of the two widths the solver blames, it must blame it by the
    // identifier the document stores, and one of them must be the new one or
    // the one it contradicts.
    let named = [ids[9], contradiction]
        .iter()
        .any(|id| message.contains(&id.to_string()));
    assert!(
        named,
        "the conflict must name a stored constraint identifier: {message}"
    );

    for forbidden in ["PointId", "ConstraintId", "ordinal", "session"] {
        assert!(
            !message.contains(forbidden),
            "the diagnosis published {forbidden}, which means nothing outside one solve: {message}"
        );
    }

    assert_eq!(
        kernel.extrude_count(),
        0,
        "a conflict must not reach a kernel"
    );
    assert_eq!(kernel.live_shape_count(), 0, "a refusal left shapes behind");
    assert_eq!(
        solver::native_live_sessions(),
        live_before,
        "a refused solve left a native system alive"
    );
}

#[test]
fn reordering_curves_and_constraints_does_not_change_the_answer() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let mut curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|c| c.id).collect();
    let ids: Vec<StableEntityId> = (0..11).map(|_| StableEntityId::new()).collect();
    let mut constraints = plate_constraints(&edges, &ids);

    // Storage order is the user's, and the transient identifiers this
    // translation mints follow it. Nothing in the answer may.
    curves.reverse();
    constraints.rotate_left(5);

    let (written, plate) = write_plate(&dir, curves, constraints, false);
    let document = reopen(&written);
    drop(written);

    let mut kernel = RecordingKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("reordering storage does not change what a sketch means");

    let requests = kernel.requests();
    let profile = requests[0].profile();
    for (index, corner) in corners(profile, &edges).into_iter().enumerate() {
        assert!(
            near(corner, SOLVED[index].0),
            "segment {index} landed at {corner:?} once the storage order changed"
        );
    }
    assert_eq!(plate.constraints.len(), 11);
    built.release_all(&mut kernel);
}

#[test]
fn rebuilding_changes_nothing_about_the_stored_document() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let (written, _plate) = plate(&dir);
    let path = written.path().to_path_buf();
    drop(written);

    let before = std::fs::read(&path).expect("reads the document back");

    let document = Document::open(&path).expect("reopens");
    let mut kernel = MockKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("the plate rebuilds");
    built.release_all(&mut kernel);
    drop(document);

    let after = std::fs::read(&path).expect("reads the document back");
    assert_eq!(
        before, after,
        "solving a sketch wrote something into the document; a solve is a rebuild, not an edit"
    );
}

#[test]
fn a_cold_and_a_cached_rebuild_agree_on_the_solved_plate() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let (written, plate) = plate(&dir);
    let document = reopen(&written);
    drop(written);

    let mut kernel = RecordingKernel::new();
    let cold = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("cold rebuild of the plate");
    cold.release_all(&mut kernel);

    let mut cache = sidecar(&dir, &document, &kernel);
    let (first, _) = rebuild_cached(
        &document,
        &mut kernel,
        &mut cache,
        &OperationContext::default(),
    )
    .expect("a cached rebuild with an empty cache");
    first.release_all(&mut kernel);

    let (second, events) = rebuild_cached(
        &document,
        &mut kernel,
        &mut cache,
        &OperationContext::default(),
    )
    .expect("a cached rebuild with a warm cache");
    second.release_all(&mut kernel);

    assert!(
        events.iter().any(|event| event.feature == plate.extrude
            && event.outcome == ferritecad_eval::CacheOutcome::Hit),
        "the second cached rebuild did not find what the first stored: {events:?}"
    );

    // Every request the kernel saw is the same solved plate. The cache key is
    // derived from the request, so an entry found under it was stored by a
    // rebuild that had solved the sketch too.
    let requests = kernel.requests();
    assert!(
        requests.len() >= 2,
        "the cold and first cached rebuilds must both have built"
    );
    let solved = corners(requests[0].profile(), &plate.edges);
    for request in &requests {
        assert_eq!(
            corners(request.profile(), &plate.edges),
            solved,
            "two rebuilds of one document disagreed about where the plate is"
        );
    }
    for (index, corner) in solved.into_iter().enumerate() {
        assert!(near(corner, SOLVED[index].0));
    }
}

#[test]
fn the_cache_key_is_the_one_the_solved_plate_would_have_had_anyway() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let (written, plate) = plate(&dir);
    let document = reopen(&written);
    drop(written);

    let mut kernel = RecordingKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("the plate rebuilds");
    built.release_all(&mut kernel);

    let context = OperationContext::default();
    let identity = kernel.identity().clone();
    let solved_key =
        ferritecad_eval::extrude_archive_key(&identity, &kernel.requests()[0], &context);

    // The same document with the constraints removed and the curves left where
    // they were stored: what a build that ignored the constraints would have
    // asked for. It cannot even close, which is the point — but the key is
    // still worth stating, because an identity derived from anything but the
    // request would not tell these two apart.
    let already_solved: Vec<SketchCurve> = SOLVED
        .iter()
        .zip(&plate.edges)
        .map(|((start, end), id)| line(*id, *start, *end).expect("finite"))
        .collect();
    let other = tempfile::tempdir().expect("temp dir");
    let (plain_written, _) = write_plate(&other, already_solved, Vec::new(), false);
    let plain = reopen(&plain_written);
    drop(plain_written);

    let mut plain_kernel = RecordingKernel::new();
    let plain_built = rebuild_cold(&plain, &mut plain_kernel, &OperationContext::default())
        .expect("an unconstrained plate at the solved coordinates builds");
    plain_built.release_all(&mut plain_kernel);

    let plain_key =
        ferritecad_eval::extrude_archive_key(&identity, &plain_kernel.requests()[0], &context);

    assert_eq!(
        solved_key, plain_key,
        "the cache identity of a solved plate must be the identity of the plate it solved to"
    );
}

/// A plate whose stored coordinates close, and are the wrong size.
///
/// The main fixture cannot close at all, which makes "the kernel got the
/// solved coordinates" easy to see and one thing hard to see: a build that
/// ignored the constraints would fail loudly rather than quietly. This one is
/// the quiet case. Stored it is fifty by thirty and perfectly buildable; the
/// constraints say sixty by forty. Both are solids; only one is the drawing.
fn misplaced_plate(dir: &TempDir, width: f64, height: f64) -> (Document, Plate) {
    let corners = [(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)];
    let curves: Vec<SketchCurve> = (0..4)
        .map(|index| {
            line(
                StableEntityId::new(),
                corners[index],
                corners[(index + 1) % corners.len()],
            )
            .expect("finite")
        })
        .collect();
    let edges: Vec<StableEntityId> = curves.iter().map(|c| c.id).collect();
    let ids: Vec<StableEntityId> = (0..11).map(|_| StableEntityId::new()).collect();

    let mut constraints = plate_constraints(&edges, &ids);
    // The two dimensions are the last two rules the fixture builds.
    constraints[9].rule = SketchConstraintRule::Distance {
        a: at(edges[0], SketchPointSelector::Start),
        b: at(edges[0], SketchPointSelector::End),
        distance: width,
    };
    constraints[10].rule = SketchConstraintRule::Distance {
        a: at(edges[1], SketchPointSelector::Start),
        b: at(edges[1], SketchPointSelector::End),
        distance: height,
    };

    write_plate(dir, curves, constraints, false)
}

/// The same four curves at the same stored coordinates, with nothing said
/// about them.
fn stored_plate_as_drawn(dir: &TempDir) -> (Document, Plate) {
    let corners = [(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)];
    let curves: Vec<SketchCurve> = (0..4)
        .map(|index| {
            line(
                StableEntityId::new(),
                corners[index],
                corners[(index + 1) % corners.len()],
            )
            .expect("finite")
        })
        .collect();
    write_plate(dir, curves, Vec::new(), false)
}

/// What one document's single extrude is keyed under, and how big it came out.
fn key_and_size(
    document: &Document,
    kernel: &mut RecordingKernel,
) -> (ferritecad_types::ContentHash, (f64, f64)) {
    let built =
        rebuild_cold(document, kernel, &OperationContext::default()).expect("the plate rebuilds");
    built.release_all(kernel);

    let requests = kernel.requests();
    let request = requests.last().expect("one request per rebuild");
    let context = OperationContext::default();
    let key = ferritecad_eval::extrude_archive_key(kernel.identity(), request, &context);

    let xs: Vec<f64> = request
        .profile()
        .outer()
        .segments()
        .iter()
        .filter_map(|s| s.geometry.start().ok())
        .map(|p| p.x)
        .collect();
    let ys: Vec<f64> = request
        .profile()
        .outer()
        .segments()
        .iter()
        .filter_map(|s| s.geometry.start().ok())
        .map(|p| p.y)
        .collect();
    let span = |v: &[f64]| {
        let lo = v.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        hi - lo
    };
    (key, (span(&xs), span(&ys)))
}

#[test]
fn a_buildable_but_wrong_sized_plate_is_still_resized_by_its_constraints() {
    // The quiet failure this whole slice exists to prevent: stored coordinates
    // that build a perfectly good solid of the wrong size.
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let (written, _plate) = misplaced_plate(&dir, WIDTH, HEIGHT);
    let document = reopen(&written);
    drop(written);

    let mut kernel = RecordingKernel::new();
    let (_, size) = key_and_size(&document, &mut kernel);
    assert!(
        (size.0 - WIDTH).abs() <= PLACED && (size.1 - HEIGHT).abs() <= PLACED,
        "the kernel was given a {size:?} plate; the constraints say {WIDTH} by {HEIGHT}"
    );
}

#[test]
fn the_cache_identity_follows_the_constraints_and_not_the_stored_coordinates() {
    // Three documents whose sketches store byte for byte the same four curves.
    // Two carry constraints that disagree about the size; one carries none. A
    // cache identity taken from what is stored would give all three one entry,
    // and the second rebuild of any of them would restore somebody else's
    // solid.
    solver_or_skip!();

    let one = tempfile::tempdir().expect("temp dir");
    let two = tempfile::tempdir().expect("temp dir");
    let three = tempfile::tempdir().expect("temp dir");

    let (a_written, _) = misplaced_plate(&one, WIDTH, HEIGHT);
    let (b_written, _) = misplaced_plate(&two, 80.0, 20.0);
    let (c_written, _) = stored_plate_as_drawn(&three);
    let (a, b, c) = (reopen(&a_written), reopen(&b_written), reopen(&c_written));
    drop((a_written, b_written, c_written));

    let mut kernel = RecordingKernel::new();
    let (a_key, a_size) = key_and_size(&a, &mut kernel);
    let (b_key, b_size) = key_and_size(&b, &mut kernel);
    let (c_key, c_size) = key_and_size(&c, &mut kernel);

    assert!((a_size.0 - WIDTH).abs() <= PLACED, "{a_size:?}");
    assert!((b_size.0 - 80.0).abs() <= PLACED, "{b_size:?}");
    assert!((c_size.0 - 50.0).abs() <= PLACED, "{c_size:?}");

    assert_ne!(
        a_key, b_key,
        "two sketches stored identically and constrained differently shared a cache entry"
    );
    assert_ne!(
        a_key, c_key,
        "a solved plate shares a cache entry with the unsolved coordinates it came from"
    );
    assert_ne!(b_key, c_key);
}

#[test]
fn a_cancelled_rebuild_leaves_no_shapes_and_no_native_session() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let (written, _plate) = plate(&dir);
    let document = reopen(&written);
    drop(written);

    let mut kernel = MockKernel::new();
    let context = OperationContext::default();
    context.cancel().cancel();

    let live_before = solver::native_live_sessions();
    let error = rebuild_cold(&document, &mut kernel, &context)
        .expect_err("a cancelled rebuild produces nothing");
    assert_eq!(error.kind(), ErrorKind::Cancellation);
    assert_eq!(kernel.live_shape_count(), 0);
    assert_eq!(
        solver::native_live_sessions(),
        live_before,
        "a cancelled rebuild left a native system alive"
    );
}

// ---------------------------------------------------------------------------
// The families that are not in the plate
// ---------------------------------------------------------------------------

/// A square held together by equal length, perpendicularity and parallelism.
///
/// The plate is sized by two distances and squared by horizontals and
/// verticals, which leaves three of the eight families untested against a real
/// planegcs. This is those three, solved for real rather than only translated.
#[test]
fn equal_length_perpendicular_and_parallel_reach_the_solver() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|c| c.id).collect();
    let ids: Vec<StableEntityId> = (0..10).map(|_| StableEntityId::new()).collect();

    use SketchPointSelector::{End, Start};
    let side = |edge: StableEntityId| SketchSegmentRef::new(at(edge, Start), at(edge, End));

    let rules = [
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
        SketchConstraintRule::Perpendicular {
            a: side(edges[0]),
            b: side(edges[1]),
        },
        SketchConstraintRule::Parallel {
            a: side(edges[0]),
            b: side(edges[2]),
        },
        SketchConstraintRule::EqualLength {
            a: side(edges[1]),
            b: side(edges[3]),
        },
        SketchConstraintRule::Distance {
            a: at(edges[0], Start),
            b: at(edges[0], End),
            distance: WIDTH,
        },
    ];
    let constraints: Vec<SketchConstraint> = ids
        .iter()
        .copied()
        .zip(rules)
        .map(|(id, rule)| SketchConstraint { id, rule })
        .collect();

    let (written, plate) = write_plate(&dir, curves, constraints, false);
    let document = reopen(&written);
    drop(written);

    let mut kernel = RecordingKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a square held by these three families solves");

    let requests = kernel.requests();
    let placed = corners(requests[0].profile(), &plate.edges);

    // Two vertical sides that are equal and perpendicular to a horizontal
    // bottom of sixty: a rectangle, sixty wide, upright at the origin.
    assert!(near(placed[0], (0.0, 0.0)), "{placed:?}");
    assert!(near(placed[1], (WIDTH, 0.0)), "{placed:?}");
    assert!(
        (placed[2].0 - WIDTH).abs() <= PLACED,
        "the top starts above the bottom right corner: {placed:?}"
    );
    assert!(
        (placed[3].0).abs() <= PLACED,
        "the left side starts above the origin: {placed:?}"
    );
    assert!(
        (placed[2].1 - placed[3].1).abs() <= PLACED,
        "the top is not level: {placed:?}"
    );

    built.release_all(&mut kernel);
}

// ---------------------------------------------------------------------------
// Construction geometry
// ---------------------------------------------------------------------------

#[test]
fn a_construction_line_is_solved_and_kept() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let mut curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|c| c.id).collect();
    let guide = StableEntityId::new();
    let mut construction = line(guide, (1.0, 1.0), (30.0, 3.0)).expect("finite");
    construction.construction = true;
    curves.push(construction);

    let ids: Vec<StableEntityId> = (0..13).map(|_| StableEntityId::new()).collect();
    let mut constraints = plate_constraints(&edges, &ids[..11]);
    // The guide hangs off the plate: its start is pinned to the bottom left
    // corner and it runs horizontally. Nothing about it bounds a face, and a
    // translation that dropped it would leave the sketch under-constrained
    // without saying so.
    constraints.push(SketchConstraint {
        id: ids[11],
        rule: SketchConstraintRule::Coincident {
            a: at(guide, SketchPointSelector::Start),
            b: at(edges[0], SketchPointSelector::Start),
        },
    });
    constraints.push(SketchConstraint {
        id: ids[12],
        rule: SketchConstraintRule::Coincident {
            a: at(guide, SketchPointSelector::End),
            b: at(edges[1], SketchPointSelector::End),
        },
    });

    let (written, _plate) = write_plate(&dir, curves, constraints, false);
    let document = reopen(&written);
    drop(written);

    let mut kernel = RecordingKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a plate with a diagonal guide solves");

    let requests = kernel.requests();
    let profile = requests[0].profile();
    assert_eq!(
        profile.outer().segments().len(),
        4,
        "a construction line bounds nothing and must not become an edge"
    );
    assert!(
        !profile.outer().segments().iter().any(|s| s.label == guide),
        "the guide became an edge of the profile"
    );
    for (index, corner) in corners(profile, &edges).into_iter().enumerate() {
        assert!(near(corner, SOLVED[index].0), "{corner:?}");
    }
    built.release_all(&mut kernel);
}

#[test]
fn a_constraint_that_only_construction_geometry_can_satisfy_still_holds() {
    // The plate on its own is fully constrained. Adding a guide adds four
    // coordinates and pinning both its ends removes exactly four, so this
    // sketch is solvable only if the construction line took part. A
    // translation that skipped construction geometry would be asked for a
    // point it never created and would have to refuse.
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let mut curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|c| c.id).collect();
    let guide = StableEntityId::new();
    let mut construction = line(guide, (5.0, 5.0), (20.0, 20.0)).expect("finite");
    construction.construction = true;
    curves.push(construction);

    let ids: Vec<StableEntityId> = (0..13).map(|_| StableEntityId::new()).collect();
    let mut constraints = plate_constraints(&edges, &ids[..11]);
    constraints.push(SketchConstraint {
        id: ids[11],
        rule: SketchConstraintRule::Fixed {
            point: at(guide, SketchPointSelector::Start),
            x: 7.0,
            y: 9.0,
        },
    });
    constraints.push(SketchConstraint {
        id: ids[12],
        rule: SketchConstraintRule::Coincident {
            a: at(guide, SketchPointSelector::End),
            b: at(edges[2], SketchPointSelector::Start),
        },
    });

    let (written, _plate) = write_plate(&dir, curves, constraints, false);
    let document = reopen(&written);
    drop(written);

    let mut kernel = MockKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a guide the constraints reach through must be part of the solve");
    assert_eq!(kernel.extrude_count(), 1);
    built.release_all(&mut kernel);
}

// ---------------------------------------------------------------------------
// Nothing durable is a transient number
// ---------------------------------------------------------------------------

#[test]
fn no_failure_publishes_a_transient_identifier() {
    // Every refusal this path can produce, checked for the vocabulary that
    // lasts one solve. A message naming one invites somebody to depend on it,
    // and the next solve of the same sketch would number it differently.
    let dir = tempfile::tempdir().expect("temp dir");
    let (written, _plate) = plate(&dir);
    let document = reopen(&written);
    drop(written);

    let mut kernel = MockKernel::new();
    let outcome = rebuild_cold(&document, &mut kernel, &OperationContext::default());

    let message = match outcome {
        Ok(built) => {
            built.release_all(&mut kernel);
            return;
        }
        Err(error) => error.to_string(),
    };
    for forbidden in ["PointId", "ConstraintId", "equation", "session"] {
        assert!(!message.contains(forbidden), "{message}");
    }
}

/// A refusal is a refusal however the caller asked for it.
#[test]
fn a_conflicting_sketch_is_refused_by_the_cached_path_too() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|c| c.id).collect();
    let ids: Vec<StableEntityId> = (0..12).map(|_| StableEntityId::new()).collect();
    let mut constraints = plate_constraints(&edges, &ids[..11]);
    constraints.push(SketchConstraint {
        id: ids[11],
        rule: SketchConstraintRule::Distance {
            a: at(edges[0], SketchPointSelector::Start),
            b: at(edges[0], SketchPointSelector::End),
            distance: WIDTH + 15.0,
        },
    });

    let (written, _plate) = write_plate(&dir, curves, constraints, false);
    let document = reopen(&written);
    drop(written);

    let mut kernel = MockKernel::new();
    let mut cache = sidecar(&dir, &document, &kernel);
    let error = rebuild_cached(
        &document,
        &mut kernel,
        &mut cache,
        &OperationContext::default(),
    )
    .expect_err("a cache cannot rescue a sketch that cannot hold");

    assert_eq!(error.kind(), ErrorKind::Constraint);
    assert_eq!(kernel.extrude_count(), 0);
    assert_eq!(kernel.live_shape_count(), 0);
}

/// Not a gate on behaviour: a reminder that `CadError` is what a caller sees.
#[test]
fn the_error_type_is_the_documents_own() {
    let error: CadError = CadError::constraint("a plate cannot be two widths at once");
    assert_eq!(error.kind(), ErrorKind::Constraint);
}
