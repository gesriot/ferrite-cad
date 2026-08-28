// SPDX-License-Identifier: MIT
//! What a solve found out, from the bytes on disk to the caller of a rebuild.
//!
//! The route is the product one, with nothing standing in for anything: a real
//! [`Document`] written to a real file, read back, translated into the product
//! sketch solver's terms, solved by planegcs, and asked afterwards what it
//! learned. The claim is not that a number came back but that the number is
//! this drawing's, said in the words this document stores.
//!
//! Two kinds of sketch carry the whole of it. One is under-constrained by a
//! known amount and says so; one repeats itself and names which constraint
//! repeated, by the identifier the document will still be using tomorrow.
//!
//! Nothing here asks for a second solve. A gate that wanted the facts and the
//! geometry would be a gate that could accept a build which solved twice, and
//! two answers to one question is one answer too many.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::sync::{Arc, Mutex};

use ferritecad_document::{
    Body, CacheStore, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression,
    Extrude, ImportedStep, ObjectPayload, Point2, Sketch, SketchConstraint, SketchConstraintRule,
    SketchCurve, SketchGeometry, SketchPointRef, SketchPointSelector, SolidOperation,
    StepImportRequest,
};
use ferritecad_eval::{RebuildResult, SketchSolveReport, rebuild_cached, rebuild_cold};
use ferritecad_exchange::{ColourSource, Definition, Import, Instance, Scene};
use ferritecad_kernel::{
    ArchiveSlot, BrepBlob, ExtrudeExtent, ExtrudeRequest, ExtrudeResult, GeometryKernel,
    KernelIdentity, Mesh, OperationContext, OperationResult, PlanarPoint, Profile, ProfileLoop,
    ProfileSegment, SegmentGeometry, ShapeHandle, SketchPlane, SubShapeHandle, TessellationParams,
    mock::MockKernel,
};
use ferritecad_sketch_solver as solver;
use ferritecad_types::{ErrorKind, ObjectId, Result, StableEntityId, Transform};
use tempfile::TempDir;

/// The plate the frame constraints square and pin, before either distance.
///
/// Stored closed and at fifty by thirty on purpose. A sketch that is left
/// under-constrained still has to build, so the coordinates it is left at have
/// to be a profile; and a sketch whose distances are added has to move, so
/// they have to be the wrong ones.
const CORNERS: [(f64, f64); 4] = [(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)];

/// What the two distance constraints say, when they are there.
const WIDTH: f64 = 60.0;
const HEIGHT: f64 = 40.0;

/// How close a solved corner must land to where the constraints put it.
const PLACED: f64 = 1.0e-4;

/// Leaves the caller when this build has no planegcs, and refuses to under
/// required mode.
fn ready() -> bool {
    if solver::is_available() {
        return true;
    }
    assert!(
        !solver::is_required(),
        "FERRITECAD_REQUIRE_PLANEGCS=1 was set, so no gate may skip: this build has no sketch \
         solver, and a rebuild that cannot solve has nothing to report about a solve"
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
/// Needed by one gate only: the one claiming that the coordinates the kernel
/// got and the facts the caller got are two halves of one solve. Reading both
/// out of this crate's own result would prove they agree with each other and
/// nothing about where either came from.
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
        self.inner.release(shape);
    }
}

// ---------------------------------------------------------------------------
// The document under test
// ---------------------------------------------------------------------------

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

fn at(curve: StableEntityId, selector: SketchPointSelector) -> SketchPointRef {
    SketchPointRef::new(curve, selector)
}

fn plate_curves() -> Vec<SketchCurve> {
    (0..4)
        .map(|index| {
            line(
                StableEntityId::new(),
                CORNERS[index],
                CORNERS[(index + 1) % CORNERS.len()],
            )
        })
        .collect()
}

/// The nine constraints that close and square the plate without sizing it.
///
/// Four coincidences shut the corners, one pin puts the first at the origin,
/// two horizontals and two verticals square it: fourteen equations over the
/// sixteen coordinates of eight stored points. Two degrees of freedom are left
/// over, and that number is what the under-constrained gates are about — it is
/// not the four curves, not the eight points, not the nine constraints and not
/// the sixteen coordinates.
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

fn width_of(edges: &[StableEntityId]) -> SketchConstraintRule {
    SketchConstraintRule::Distance {
        a: at(edges[0], SketchPointSelector::Start),
        b: at(edges[0], SketchPointSelector::End),
        distance: WIDTH,
    }
}

fn height_of(edges: &[StableEntityId]) -> SketchConstraintRule {
    SketchConstraintRule::Distance {
        a: at(edges[1], SketchPointSelector::Start),
        b: at(edges[1], SketchPointSelector::End),
        distance: HEIGHT,
    }
}

/// Gives every rule a fresh stored identifier, in the order it is written.
fn named(rules: Vec<SketchConstraintRule>) -> Vec<SketchConstraint> {
    rules
        .into_iter()
        .map(|rule| SketchConstraint {
            id: StableEntityId::new(),
            rule,
        })
        .collect()
}

/// Every identifier one written document handed out.
struct Written {
    sketch: ObjectId,
    plane: ObjectId,
    extrude: ObjectId,
    body: ObjectId,
    constraints: Vec<StableEntityId>,
}

/// Writes a plate with the given curves and constraints to a real file.
fn write(
    dir: &TempDir,
    curves: Vec<SketchCurve>,
    constraints: Vec<SketchConstraint>,
    extra_extrude: bool,
) -> (Document, Written) {
    let mut document = Document::create(dir.path().join("part.fcad")).expect("creates");

    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let extrude = ObjectId::new();
    let second = ObjectId::new();
    let body = ObjectId::new();

    let constraint_ids: Vec<StableEntityId> = constraints.iter().map(|one| one.id).collect();

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
        Written {
            sketch,
            plane,
            extrude,
            body,
            constraints: constraint_ids,
        },
    )
}

/// Reads the document back from its own bytes.
fn reopen(document: &Document) -> Document {
    Document::open(document.path()).expect("reopens what was written")
}

/// One rebuild of a written document, and the report it produced.
fn report_of(document: &Document, sketch: ObjectId) -> Option<SketchSolveReport> {
    let mut kernel = MockKernel::new();
    let built = rebuild_cold(document, &mut kernel, &OperationContext::default())
        .expect("the plate rebuilds");
    let report = built.solve_report(sketch).cloned();
    built.release_all(&mut kernel);
    report
}

fn sidecar(dir: &TempDir, document: &Document, kernel: &impl GeometryKernel) -> CacheStore {
    CacheStore::open(
        dir.path().join("part.fcad-cache"),
        document.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )
    .expect("the sidecar opens")
}

fn reports(built: &RebuildResult) -> Vec<(ObjectId, SketchSolveReport)> {
    built
        .solve_reports()
        .map(|(id, report)| (id, report.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// How much freedom is left
// ---------------------------------------------------------------------------

#[test]
fn an_under_constrained_sketch_reports_the_freedom_it_has_left() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let (document, written) = write(&dir, curves, named(frame(&edges)), false);
    let document = reopen(&document);

    let report = report_of(&document, written.sketch).expect("a constrained sketch was solved");

    assert_eq!(
        report.degrees_of_freedom(),
        2,
        "a closed square that nothing sizes is free in two directions"
    );
    assert!(report.is_under_constrained());
    assert!(
        report.redundant().is_empty(),
        "nothing here repeats anything: {:?}",
        report.redundant()
    );
}

#[test]
fn a_fully_constrained_sketch_reports_no_freedom_and_nothing_repeated() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    rules.push(height_of(&edges));
    let (document, written) = write(&dir, curves, named(rules), false);
    let document = reopen(&document);

    let report = report_of(&document, written.sketch).expect("a constrained sketch was solved");

    assert_eq!(report.degrees_of_freedom(), 0);
    assert!(!report.is_under_constrained());
    assert!(report.redundant().is_empty());
}

#[test]
fn the_freedom_left_is_the_solvers_and_not_a_count_of_anything_stored() {
    // Two documents whose sketches hold the same curves and differ by one
    // constraint. A report derived from how much was stored rather than from
    // what was solved would move by one; the freedom moves by one too, in the
    // other direction, and neither number is any tally of the document.
    solver_or_skip!();

    let loose = tempfile::tempdir().expect("temp dir");
    let looser = tempfile::tempdir().expect("temp dir");

    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut sized = frame(&edges);
    sized.push(width_of(&edges));

    let (one, one_written) = write(&loose, curves.clone(), named(sized), false);
    let (two, two_written) = write(&looser, curves, named(frame(&edges)), false);
    let (one, two) = (reopen(&one), reopen(&two));

    let with_width = report_of(&one, one_written.sketch).expect("solved");
    let without = report_of(&two, two_written.sketch).expect("solved");

    assert_eq!(with_width.degrees_of_freedom(), 1);
    assert_eq!(without.degrees_of_freedom(), 2);
}

// ---------------------------------------------------------------------------
// What repeated itself, and what it is called
// ---------------------------------------------------------------------------

#[test]
fn a_repeated_constraint_is_named_by_the_identifier_the_document_stores() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    rules.push(height_of(&edges));
    // A second statement of the width, agreeing with the first. The drawing is
    // buildable and says one thing twice, which is a different fact from a
    // drawing that cannot hold.
    rules.push(width_of(&edges));
    let constraints = named(rules);
    let repeated = constraints[11].id;
    let (document, written) = write(&dir, curves, constraints, false);
    let document = reopen(&document);

    let report = report_of(&document, written.sketch).expect("a repeated constraint still solves");

    assert_eq!(report.degrees_of_freedom(), 0);
    assert_eq!(
        report.redundant(),
        [repeated],
        "the repeated constraint must be named as the document names it"
    );
    assert_eq!(
        written.constraints.len(),
        12,
        "the fixture stores twelve constraints"
    );
}

#[test]
fn several_repeated_constraints_arrive_in_the_order_the_document_stores_them() {
    // The same two repetitions, stored in the two possible orders, under the
    // same two identifiers. What is repeated does not change; which comes
    // first does, and follows the document.
    //
    // The two identifiers are minted once, before either document, and the one
    // written second in the second document is the one minted first. So an
    // order taken from when an identifier was created — identifiers are time
    // ordered, and sorting them is the easiest wrong answer here — is not the
    // order either document stores.
    solver_or_skip!();

    let first = tempfile::tempdir().expect("temp dir");
    let second = tempfile::tempdir().expect("temp dir");

    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let repeated_height = StableEntityId::new();
    let repeated_width = StableEntityId::new();
    assert!(
        repeated_height < repeated_width,
        "stored identifiers are time ordered, which this gate relies on"
    );

    let stated = |extra: Vec<SketchConstraint>| {
        let mut rules = frame(&edges);
        rules.push(width_of(&edges));
        rules.push(height_of(&edges));
        let mut constraints = named(rules);
        constraints.extend(extra);
        constraints
    };
    let height_then_width = stated(vec![
        SketchConstraint {
            id: repeated_height,
            rule: height_of(&edges),
        },
        SketchConstraint {
            id: repeated_width,
            rule: width_of(&edges),
        },
    ]);
    let width_then_height = stated(vec![
        SketchConstraint {
            id: repeated_width,
            rule: width_of(&edges),
        },
        SketchConstraint {
            id: repeated_height,
            rule: height_of(&edges),
        },
    ]);

    let (one, one_written) = write(&first, curves.clone(), height_then_width, false);
    let (two, two_written) = write(&second, curves, width_then_height, false);
    let (one, two) = (reopen(&one), reopen(&two));

    let one_report = report_of(&one, one_written.sketch).expect("solved");
    let two_report = report_of(&two, two_written.sketch).expect("solved");

    assert_eq!(
        one_report.redundant(),
        [repeated_height, repeated_width],
        "the document stores the repeated height first"
    );
    assert_eq!(
        two_report.redundant(),
        [repeated_width, repeated_height],
        "the document stores the repeated width first"
    );

    let mut one_set = one_report.redundant().to_vec();
    let mut two_set = two_report.redundant().to_vec();
    one_set.sort_unstable();
    two_set.sort_unstable();
    assert_eq!(
        one_set, two_set,
        "storage order changed which constraints repeat, and it may only change their order"
    );
}

#[test]
fn reordering_the_curves_changes_nothing_the_report_says() {
    solver_or_skip!();

    let upright = tempfile::tempdir().expect("temp dir");
    let reversed = tempfile::tempdir().expect("temp dir");

    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    rules.push(width_of(&edges));
    let constraints = named(rules);
    let repeated = constraints[10].id;

    let mut backwards = curves.clone();
    backwards.reverse();

    let (one, one_written) = write(&upright, curves, constraints.clone(), false);
    let (two, two_written) = write(&reversed, backwards, constraints, false);
    let (one, two) = (reopen(&one), reopen(&two));

    let one_report = report_of(&one, one_written.sketch).expect("solved");
    let two_report = report_of(&two, two_written.sketch).expect("solved");

    assert_eq!(one_report.redundant(), [repeated]);
    assert_eq!(
        one_report, two_report,
        "the order the curves are stored in is not part of what a sketch means"
    );
}

// ---------------------------------------------------------------------------
// One solve
// ---------------------------------------------------------------------------

#[test]
fn the_solved_coordinates_and_the_report_are_two_halves_of_one_solve() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    rules.push(height_of(&edges));
    rules.push(height_of(&edges));
    let constraints = named(rules);
    let repeated = constraints[11].id;
    let (document, written) = write(&dir, curves, constraints, false);
    let document = reopen(&document);

    let mut kernel = RecordingKernel::new();
    let before = solver::native_solves();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a plate that repeats one of its dimensions still builds");

    assert_eq!(
        solver::native_solves() - before,
        1,
        "the facts and the geometry must come from one crossing into planegcs"
    );

    // The geometry, read from what the kernel was handed.
    let requests = kernel.requests();
    assert_eq!(requests.len(), 1);
    let spans = |axis: fn(&ferritecad_kernel::PlanarPoint) -> f64| {
        let values: Vec<f64> = requests[0]
            .profile()
            .outer()
            .segments()
            .iter()
            .filter_map(|segment| segment.geometry.start().ok())
            .map(|point| axis(&point))
            .collect();
        let low = values.iter().copied().fold(f64::INFINITY, f64::min);
        let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        high - low
    };
    assert!(
        (spans(|point| point.x) - WIDTH).abs() <= PLACED,
        "the kernel was not given the solved width"
    );
    assert!(
        (spans(|point| point.y) - HEIGHT).abs() <= PLACED,
        "the kernel was not given the solved height"
    );

    // And the facts, out of the same rebuild.
    let report = built
        .solve_report(written.sketch)
        .expect("the sketch that was solved has a report");
    assert_eq!(report.degrees_of_freedom(), 0);
    assert_eq!(report.redundant(), [repeated]);

    built.release_all(&mut kernel);
}

#[test]
fn two_features_reading_one_sketch_get_one_solve_and_one_report() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let (document, written) = write(&dir, curves, named(frame(&edges)), true);
    let document = reopen(&document);

    let mut kernel = MockKernel::new();
    let before = solver::native_solves();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("two features over one sketch rebuild");

    assert_eq!(kernel.extrude_count(), 2, "both features must be built");
    assert_eq!(
        solver::native_solves() - before,
        1,
        "a second consumer of one sketch must not buy a second solve"
    );
    assert_eq!(
        reports(&built).len(),
        1,
        "one sketch is one sketch however many features read it"
    );
    assert_eq!(
        reports(&built)[0].0,
        written.sketch,
        "the report belongs to the sketch, not to a feature that read it"
    );

    built.release_all(&mut kernel);
}

#[test]
fn a_cached_rebuild_reports_what_the_cold_one_did_and_solves_no_more_often() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    rules.push(width_of(&edges));
    let (document, written) = write(&dir, curves, named(rules), false);
    let document = reopen(&document);

    let mut kernel = MockKernel::new();

    let before_cold = solver::native_solves();
    let cold = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("cold rebuild of the plate");
    let cold_report = cold
        .solve_report(written.sketch)
        .cloned()
        .expect("reported");
    assert_eq!(solver::native_solves() - before_cold, 1);
    cold.release_all(&mut kernel);

    let mut cache = sidecar(&dir, &document, &kernel);
    let before_first = solver::native_solves();
    let (first, _) = rebuild_cached(
        &document,
        &mut kernel,
        &mut cache,
        &OperationContext::default(),
    )
    .expect("a cached rebuild with an empty cache");
    let first_report = first
        .solve_report(written.sketch)
        .cloned()
        .expect("reported");
    assert_eq!(solver::native_solves() - before_first, 1);
    first.release_all(&mut kernel);

    let before_warm = solver::native_solves();
    let (warm, events) = rebuild_cached(
        &document,
        &mut kernel,
        &mut cache,
        &OperationContext::default(),
    )
    .expect("a cached rebuild with a warm cache");
    let warm_report = warm
        .solve_report(written.sketch)
        .cloned()
        .expect("reported");

    assert!(
        events.iter().any(|event| event.feature == written.extrude
            && event.outcome == ferritecad_eval::CacheOutcome::Hit),
        "the warm rebuild did not restore what the first stored: {events:?}"
    );
    assert_eq!(
        solver::native_solves() - before_warm,
        1,
        "a cache holds geometry, not facts; restoring the geometry must neither add a solve nor \
         let one be skipped"
    );
    assert_eq!(
        cold_report, first_report,
        "the cold and cached rebuilds disagree about the same sketch"
    );
    assert_eq!(
        cold_report, warm_report,
        "a cache hit lost what the solve found out"
    );

    warm.release_all(&mut kernel);
}

// ---------------------------------------------------------------------------
// Who has a report, and who does not
// ---------------------------------------------------------------------------

#[test]
fn an_unconstrained_sketch_has_no_report_and_asks_no_solver() {
    // Holds in every build, with or without a library.
    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let (document, written) = write(&dir, curves, Vec::new(), false);
    let document = reopen(&document);

    let mut kernel = MockKernel::new();
    let before = solver::native_solves();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("an unconstrained plate rebuilds as it always did");

    assert_eq!(
        solver::native_solves(),
        before,
        "an unconstrained sketch reached the solver"
    );
    assert!(
        built.solve_report(written.sketch).is_none(),
        "nothing solved this sketch, so nothing may claim to have found anything out about it"
    );
    assert!(reports(&built).is_empty());

    built.release_all(&mut kernel);
}

#[test]
fn nothing_but_the_solved_sketch_has_a_report() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let (document, written) = write(&dir, curves, named(frame(&edges)), true);
    let document = reopen(&document);

    let mut kernel = MockKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("the plate rebuilds");

    for other in [
        written.plane,
        written.extrude,
        written.body,
        ObjectId::new(),
    ] {
        assert!(
            built.solve_report(other).is_none(),
            "{other} is not a sketch that was solved, and has a report"
        );
    }
    assert_eq!(
        reports(&built),
        vec![(
            written.sketch,
            built
                .solve_report(written.sketch)
                .cloned()
                .expect("reported")
        )],
        "the enumeration must hold exactly the one sketch that was solved"
    );

    built.release_all(&mut kernel);
}

#[test]
fn a_document_of_stored_geometry_reports_no_solves() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("imported.fcad");
    let mut kernel = MockKernel::new();
    store_import(&path, &mut kernel).expect("stores an import");

    let document = Document::open_read_only(&path).expect("reopens");
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("an imported object is not a reason to refuse the document");

    assert!(
        reports(&built).is_empty(),
        "nothing in this document was solved, and something claims otherwise"
    );

    built.release_all(&mut kernel);
}

#[test]
fn an_empty_document_reports_no_solves() {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = Document::create(dir.path().join("empty.fcad")).expect("creates");
    let document = reopen(&document);

    let mut kernel = MockKernel::new();
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("an empty document rebuilds into nothing");

    assert!(reports(&built).is_empty());
    built.release_all(&mut kernel);
}

// ---------------------------------------------------------------------------
// A refusal publishes nothing
// ---------------------------------------------------------------------------

#[test]
fn a_conflicting_sketch_publishes_no_facts() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    // A second width, disagreeing with the first.
    rules.push(SketchConstraintRule::Distance {
        a: at(edges[0], SketchPointSelector::Start),
        b: at(edges[0], SketchPointSelector::End),
        distance: WIDTH + 15.0,
    });
    let (document, _written) = write(&dir, curves, named(rules), false);
    let document = reopen(&document);

    let mut kernel = MockKernel::new();
    let error = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("a plate that is both sixty and seventy five wide cannot be built");

    assert_eq!(error.kind(), ErrorKind::Constraint);
    assert_eq!(kernel.live_shape_count(), 0, "a refusal left shapes behind");
}

#[test]
fn a_cancelled_rebuild_publishes_no_facts() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let (document, _written) = write(&dir, curves, named(frame(&edges)), false);
    let document = reopen(&document);

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

#[test]
fn a_build_with_no_solver_publishes_no_facts() {
    if solver::is_available() {
        eprintln!("skipped: this build has a sketch solver, so it has nothing to refuse for");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let (document, _written) = write(&dir, curves, named(frame(&edges)), false);
    let document = reopen(&document);

    let mut kernel = MockKernel::new();
    let error = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect_err("with no solver there is nothing that could have solved this sketch");

    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert_eq!(kernel.live_shape_count(), 0);
}

// ---------------------------------------------------------------------------
// Nothing durable is a transient number, and nothing is written back
// ---------------------------------------------------------------------------

#[test]
fn a_report_publishes_no_vocabulary_that_lasts_one_solve() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    rules.push(width_of(&edges));
    let constraints = named(rules);
    let repeated = constraints[10].id;
    let (document, written) = write(&dir, curves, constraints, false);
    let document = reopen(&document);

    let report = report_of(&document, written.sketch).expect("solved");
    let printed = format!("{report:?}");

    for forbidden in ["PointId", "ConstraintId", "ordinal", "equation", "session"] {
        assert!(
            !printed.contains(forbidden),
            "the report published {forbidden}, which means nothing outside one solve: {printed}"
        );
    }
    assert!(
        printed.contains(&repeated.to_string()),
        "the report must say what it is about in the document's own words: {printed}"
    );
}

#[test]
fn reporting_on_a_solve_changes_nothing_about_the_stored_document() {
    solver_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let curves = plate_curves();
    let edges: Vec<StableEntityId> = curves.iter().map(|curve| curve.id).collect();
    let mut rules = frame(&edges);
    rules.push(width_of(&edges));
    rules.push(width_of(&edges));
    let (written_document, written) = write(&dir, curves, named(rules), false);
    let path = written_document.path().to_path_buf();
    drop(written_document);

    let before = std::fs::read(&path).expect("reads the document back");

    let document = Document::open(&path).expect("reopens");
    let report = report_of(&document, written.sketch).expect("solved");
    assert_eq!(report.redundant().len(), 1);
    drop(document);

    let after = std::fs::read(&path).expect("reads the document back");
    assert_eq!(
        before, after,
        "asking what a solve found out wrote something into the document"
    );
}

// ---------------------------------------------------------------------------
// A document that holds only stored geometry
// ---------------------------------------------------------------------------

fn store_import(
    path: &std::path::Path,
    kernel: &mut MockKernel,
) -> Result<(ObjectId, ImportedStep)> {
    let corners = [
        PlanarPoint::new(0.0, 0.0),
        PlanarPoint::new(10.0, 0.0),
        PlanarPoint::new(10.0, 10.0),
        PlanarPoint::new(0.0, 10.0),
    ]
    .map(|corner| corner.expect("finite"));
    let segments = corners
        .iter()
        .enumerate()
        .map(|(index, start)| {
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(*start, corners[(index + 1) % corners.len()])
                    .expect("distinct"),
            )
        })
        .collect();
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(segments).expect("closes"),
        Vec::new(),
    )
    .expect("valid");
    let shape = kernel
        .extrude(
            &ExtrudeRequest::new(profile, ExtrudeExtent::blind(4.0).expect("positive"), false),
            &OperationContext::default(),
        )
        .expect("the mock builds a solid")
        .shape;

    let import = Import::Imported {
        scene: Scene {
            source_unit: "MILLIMETRE".to_owned(),
            schema: "AP214".to_owned(),
            definitions: vec![Definition {
                shape,
                name: "Plate".to_owned(),
                solids: 1,
                key: "step.product_definition#5".to_owned(),
            }],
            instances: vec![Instance {
                definition: 0,
                parent: None,
                name: "Plate".to_owned(),
                placement: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                colour_source: ColourSource::None,
                colour: [0.0; 3],
            }],
        },
        diagnostics: Vec::new(),
    };
    let object = ObjectId::new();
    let mut document = Document::create(path)?;
    let stored = document.store_step_import(StepImportRequest {
        object,
        name: Some("Imported"),
        source: b"ISO-10303-21; whatever the document was handed",
        source_name: None,
        import: &import,
        importer: kernel.identity(),
    })?;
    for shape in import.scene().expect("a scene was stored").shapes() {
        kernel.release(shape);
    }
    Ok((object, stored))
}
