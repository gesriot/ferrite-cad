// SPDX-License-Identifier: MIT
//! Solving a stored sketch, and putting the answer back where it belongs.
//!
//! This is the whole of what the document and the sketch solver have to say to
//! each other, and it lasts one solve.
//!
//! # Two vocabularies, and why neither may borrow the other's words
//!
//! A document names a point by the curve it belongs to and which point of that
//! curve it is — a [`SketchPointRef`] — and names a constraint by a
//! [`StableEntityId`] that was minted once and is never reissued. Those words
//! outlive the file, the session and whichever solver happens to be linked.
//!
//! The solver names a point by a [`solver::PointId`] and a constraint by a
//! [`solver::ConstraintId`], and those are the caller's to choose precisely so
//! that the solver has no vocabulary of its own to impose. Here the caller is
//! this file, and what it chooses is sequential and follows storage order,
//! because it is thrown away at the end of the call. Nothing that leaves this
//! module may depend on those numbers: a conflict is reported against the
//! document's own identifiers, and the same sketch stored in a different order
//! gives the same diagnosis with different numbers behind it.
//!
//! [`Translation`] is the only road between the two, in both directions, and
//! it is a lookup rather than arithmetic on either side. An identifier that
//! happened to equal its own storage position would hide every mistake that
//! matters here.
//!
//! # What a refusal carries
//!
//! Three of the four answers are a sentence and nothing more. The fourth is
//! not: a sketch whose constraints cannot all hold has something a program can
//! act on — which constraints, and what each of them says — and there is
//! nowhere for it to travel except with the refusal, because a conflict makes
//! no profile, no solid and no scene. So it goes out as the source of the
//! [`CadError::Constraint`] it produces, as a [`SketchConflict`], and
//! [`SketchConflict::of`] is the only road back. The message stays what it
//! was: a sentence for a person, not a record for a caller to take apart.
//!
//! # What a solve is allowed to change
//!
//! Nothing in the document. The answer is applied to a clone, which is handed
//! to the profile arithmetic and dropped. Constraints, identifiers,
//! construction flags, the plane and the order of the curves are carried
//! across untouched; only the three stored coordinate pairs a solver can
//! answer for — `Point.At`, `Line.Start` and `Line.End` — are written.

use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::{
    Point2, Sketch, SketchConstraint, SketchConstraintRule, SketchGeometry, SketchPointRef,
    SketchPointSelector, SketchSegmentRef,
};
use ferritecad_sketch_solver as solver;
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};

/// Solves `sketch` if it has anything to solve.
///
/// `Ok(None)` means there were no constraints, and is the one answer that asks
/// the solver nothing at all: a document written before constraints existed
/// must rebuild in a build that never linked a solver, exactly as it did.
///
/// `Ok(Some(_))` is a temporary sketch at the solved coordinates together with
/// a [`SketchSolveReport`] for the same solve. The sketch is not stored, not
/// returned to the caller of a rebuild and not compared against the original:
/// it exists to be turned into a profile. The report outlives it, because what
/// a solve found out about a drawing is a fact about the drawing.
///
/// One solve answers both. Asking again for the report would be asking a
/// second question of a solver that has already been asked, and two answers to
/// one question is one answer too many.
///
/// `Err` is every other outcome, and one of them carries more than a sentence:
/// a solve whose constraints cannot all hold refuses with a [`SketchConflict`]
/// naming `id` and each blamed constraint. That is built here because here is
/// the only place the stored sketch and the solver's answer are both in hand,
/// and nothing downstream gets a second chance to build it: a conflict makes
/// no profile, no solid and no scene for it to travel in.
pub(crate) fn solved(sketch: &Sketch, id: ObjectId) -> Result<Option<(Sketch, SketchSolveReport)>> {
    if sketch.constraints.is_empty() {
        return Ok(None);
    }

    let translation = Translation::read(sketch)?;
    let outcome =
        solver::solve(translation.stated()).map_err(|error| translation.refusal(&error))?;
    translation.interpret(sketch, id, outcome).map(Some)
}

/// What one successful solve found out about the sketch it solved.
///
/// Everything here is said in the document's words and outlives the solve that
/// produced it. There is no [`solver::PointId`], no [`solver::ConstraintId`],
/// no native tag, no equation index and no position: those number one call to
/// one library and mean nothing after it, and a report is read after it.
///
/// Not serialisable, and deliberately. This is what a rebuild found out, not
/// something a document holds: storing it would make a file's meaning depend
/// on which solver last opened it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SketchSolveReport {
    degrees_of_freedom: usize,
    redundant: Vec<StableEntityId>,
}

impl SketchSolveReport {
    /// How much freedom the drawing has left. Zero means it cannot move.
    pub fn degrees_of_freedom(&self) -> usize {
        self.degrees_of_freedom
    }

    /// Whether anything about this sketch is still undecided.
    ///
    /// Read from the degrees of freedom rather than stored beside them. A
    /// separate flag is a second account of one fact, free to disagree with
    /// the first the moment either is written by hand.
    pub fn is_under_constrained(&self) -> bool {
        self.degrees_of_freedom > 0
    }

    /// The constraints that repeat what the rest already said, named as the
    /// document names them and in the order the document stores them.
    ///
    /// Document order, because that is the order a person reading their own
    /// drawing expects; the solver's own order is an artefact of how it
    /// numbered a system it has since thrown away.
    pub fn redundant(&self) -> &[StableEntityId] {
        &self.redundant
    }
}

/// Why a sketch could not be solved: the constraints a solve said cannot all
/// hold, said in the document's words.
///
/// A conflicting solve produces no coordinates, no profile and no scene, so
/// there is nowhere for this to be filed the way [`SketchSolveReport`] is
/// filed against a successful rebuild. It travels with the refusal instead,
/// as the source of a [`CadError::Constraint`], and [`SketchConflict::of`] is
/// the only way to get it back.
///
/// Everything here outlives the solve that produced it: the document's own
/// identifier for the sketch, the document's own identifier for each blamed
/// constraint, and the rule the document stores under exactly that
/// identifier. There is no [`solver::ConstraintId`], no [`solver::PointId`],
/// no equation index, no session identity and no position, so the derived
/// `Debug` cannot publish one.
///
/// Not serialisable, for the same reason a report is not: this is what one
/// rebuild found out, not something a document holds.
///
/// The fields are private and there is no constructor outside this module.
/// A pair of an identifier and a rule that disagreed with each other would be
/// this type saying something no solve ever said, and the compiler is what
/// stops it being written rather than a test.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchConflict {
    sketch: ObjectId,
    constraints: Vec<ConflictingConstraint>,
}

impl SketchConflict {
    /// The sketch whose constraints disagree.
    ///
    /// The document's word for it, supplied by the caller that was rebuilding
    /// that sketch. A solver has no notion of a document object and could not
    /// have said this.
    pub fn sketch(&self) -> ObjectId {
        self.sketch
    }

    /// The blamed constraints, in the order the document stores them.
    pub fn constraints(&self) -> &[ConflictingConstraint] {
        &self.constraints
    }

    /// The conflict behind a refusal, if that refusal is one.
    ///
    /// The one road back. A caller asks the type whether an error is one of
    /// its own; it does not read the message, which is a sentence written for
    /// a person and free to be rewritten. Every other failure – a document
    /// that would not open, a sketch that would not converge, a build with no
    /// solver – answers `None`, because none of them put one here.
    pub fn of(error: &CadError) -> Option<&Self> {
        let source = std::error::Error::source(error)?;
        source.downcast_ref::<Self>()
    }
}

/// A sentence for the error chain, and nothing a window would show.
///
/// Names the sketch and the identifiers, because those are what a reader of a
/// log can look up. What each constraint *says* is deliberately absent: a
/// vocabulary for the eight families is the interface's to choose, and one
/// written here would be a second one to disagree with it.
impl std::fmt::Display for SketchConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sketch {} holds constraints that disagree:", self.sketch)?;
        for (index, constraint) in self.constraints.iter().enumerate() {
            let separator = if index == 0 { " " } else { ", " };
            write!(f, "{separator}{}", constraint.id)?;
        }
        Ok(())
    }
}

impl std::error::Error for SketchConflict {}

/// One constraint a solve said cannot hold with the rest, named durably and
/// carrying what the document stores under exactly that name.
///
/// The identifier is the solve's own answer, translated back; the rule is read
/// out of the sketch under that identifier and nothing else. Not by where a
/// constraint sits in a list, not by what a solver numbered it, not by two
/// rules happening to say the same thing. Two identical rules stored under two
/// identifiers are two of these, because they are two constraints.
///
/// The rule is carried whole rather than turned into a sentence: choosing
/// anybody's words is the interface's job, and the document's own vocabulary
/// is the smallest durable thing that can be explained later.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConflictingConstraint {
    id: StableEntityId,
    rule: SketchConstraintRule,
}

impl ConflictingConstraint {
    /// The identifier the document stores this constraint under.
    pub fn id(&self) -> StableEntityId {
        self.id
    }

    /// The exact rule stored under that identifier.
    pub fn rule(&self) -> &SketchConstraintRule {
        &self.rule
    }
}

/// The stored constraint behind each identifier a solve blamed for a conflict.
///
/// A pure function over two document values, so it can be held to its contract
/// with no solver in the room.
///
/// Joined by [`StableEntityId`] alone. Not by position, because the blamed
/// order and the storage order are both the document's and matching them up by
/// index would pass every test that compared two lists of the same length; not
/// by what a rule says, because two constraints are allowed to say the same
/// thing; not by a solver's numbering, which does not outlive the call that
/// issued it. An identifier this sketch does not store – one that belongs to
/// the sketch next to it, or to nothing at all – is a refusal rather than a
/// gap, because reporting the part that resolved would publish a partial
/// account of a drawing as a complete one.
fn conflicting_constraints(
    sketch: &Sketch,
    reported: &[StableEntityId],
) -> Result<Vec<ConflictingConstraint>> {
    reported
        .iter()
        .map(|id| {
            sketch
                .constraints
                .iter()
                .find(|constraint| constraint.id == *id)
                .map(|constraint| ConflictingConstraint {
                    id: *id,
                    rule: constraint.rule,
                })
                .ok_or_else(|| {
                    CadError::constraint(format!(
                        "the solve blamed {id} for a conflict, and this sketch stores no \
                         constraint under that identifier"
                    ))
                })
        })
        .collect()
}

/// The stored points of one piece of geometry, in the order the document
/// stores them.
///
/// Circles and arcs have none. A circle *is* its centre and its radius
/// together and the solver contract has no radius parameter; an arc's
/// endpoints are derived from a centre, a radius and two angles rather than
/// stored, so there is no durable pair to write an answer back into. This
/// agrees with [`SketchPointSelector`], which for the same reasons offers no
/// way to name either — a reference into one cannot be built, so none can be
/// resolved here.
///
/// A geometry this build does not know is treated as having no stored points.
/// A constraint that reached into one would then fail to resolve and be
/// refused, which is the honest answer: this build cannot say where that point
/// went.
fn stored_points(geometry: &SketchGeometry) -> &'static [SketchPointSelector] {
    use SketchPointSelector::{At, End, Start};
    match geometry {
        SketchGeometry::Point { .. } => &[At],
        SketchGeometry::Line { .. } => &[Start, End],
        SketchGeometry::Circle { .. } | SketchGeometry::Arc { .. } => &[],
        _ => &[],
    }
}

/// Writes one solved coordinate into the field it came from.
///
/// The pairing is spelled out rather than derived, because getting it wrong is
/// silent: a line whose start and end were exchanged is still a line, still
/// closes a loop with its neighbours, and extrudes into a solid nobody drew.
fn write_point(
    geometry: &mut SketchGeometry,
    selector: SketchPointSelector,
    value: Point2,
) -> Result<()> {
    match (selector, geometry) {
        (SketchPointSelector::At, SketchGeometry::Point { at }) => {
            *at = value;
            Ok(())
        }
        (SketchPointSelector::Start, SketchGeometry::Line { start, .. }) => {
            *start = value;
            Ok(())
        }
        (SketchPointSelector::End, SketchGeometry::Line { end, .. }) => {
            *end = value;
            Ok(())
        }
        (selector, geometry) => Err(CadError::constraint(format!(
            "the solver answered for the {} of a {}, which stores no such point",
            selector.as_str(),
            match geometry {
                SketchGeometry::Point { .. } => "point",
                SketchGeometry::Line { .. } => "line",
                SketchGeometry::Circle { .. } => "circle",
                SketchGeometry::Arc { .. } => "arc",
                _ => "shape this build does not know",
            }
        ))),
    }
}

/// One sketch stated in the solver's terms, and the way back.
///
/// Built for a single solve and dropped with it. Both directions are stored
/// rather than recomputed: the forward one builds the constraints, the reverse
/// one turns an answer — a solved position, a blamed constraint — back into
/// something a person who drew the sketch can act on.
struct Translation {
    stated: solver::Sketch,
    /// The document's word for a point to the solver's, for this solve only.
    point_of: BTreeMap<SketchPointRef, solver::PointId>,
    /// And back again.
    point_ref: BTreeMap<solver::PointId, SketchPointRef>,
    /// The solver's word for a constraint back to the document's.
    ///
    /// Only this direction is kept. The forward one is the act of allocating,
    /// which happens once, in order, in [`Translation::read`]; storing it as
    /// well would be a second copy of the same fact for nothing to read.
    constraint_ref: BTreeMap<solver::ConstraintId, StableEntityId>,
}

/// A `Debug` that cannot publish a number that lasts one call.
///
/// Written out rather than derived. The derived one would print every
/// transient identifier this type exists to keep transient, and something
/// printed is something somebody will read a meaning into.
impl std::fmt::Debug for Translation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Translation")
            .field("points", &self.point_of.len())
            .field("constraints", &self.constraint_ref.len())
            .finish()
    }
}

impl Translation {
    /// Reads a stored sketch into the solver's terms.
    ///
    /// Every stored point of every curve gets an identifier of its own, model
    /// geometry and construction geometry alike. Two points at the same
    /// coordinates stay two points: a document that welded them would be
    /// deciding that the user meant them to be coincident, which is a thing
    /// the user says with a constraint and which this file must never say on
    /// their behalf. It is also why `Coincident` exists.
    fn read(sketch: &Sketch) -> Result<Self> {
        let mut stated = solver::Sketch::new();
        let mut point_of = BTreeMap::new();
        let mut point_ref = BTreeMap::new();

        for curve in &sketch.curves {
            for &selector in stored_points(&curve.geometry) {
                let reference = SketchPointRef::new(curve.id, selector);
                let Some(at) = stored_coordinate(&curve.geometry, selector) else {
                    continue;
                };
                let id = solver::PointId(point_of.len() as u64);
                if point_of.insert(reference, id).is_some() {
                    // Persistence refuses a sketch with two curves of one
                    // identifier, so reaching this means the document was
                    // built in memory and never validated. Refusing is the
                    // only answer that does not silently solve one of them.
                    return Err(CadError::constraint(format!(
                        "this sketch names {reference} twice, so there is no one point to solve \
                         for"
                    )));
                }
                point_ref.insert(id, reference);
                stated.add_point(id, at.x, at.y);
            }
        }

        let mut translation = Self {
            stated,
            point_of,
            point_ref,
            constraint_ref: BTreeMap::new(),
        };

        for (index, constraint) in sketch.constraints.iter().enumerate() {
            let id = solver::ConstraintId(index as u64);
            let stated = translation.constraint(constraint)?;
            translation.constraint_ref.insert(id, constraint.id);
            translation.stated.add_constraint(id, stated);
        }

        Ok(translation)
    }

    fn stated(&self) -> &solver::Sketch {
        &self.stated
    }

    /// One stored constraint in the solver's terms.
    ///
    /// Every reference the rule names is resolved first, and the enumeration
    /// of what it names comes from [`SketchConstraintRule::points`] and from
    /// nowhere else. The match below still has to place each reference in the
    /// field that means it — there is no way round that, and no way to derive
    /// which field is which — but it is not a second opinion about *which*
    /// references a family has: a family whose match arm forgot one would have
    /// had it resolved here anyway, and a family that grew one would be caught
    /// by the same loop.
    fn constraint(&self, constraint: &SketchConstraint) -> Result<solver::Constraint> {
        for reference in constraint.rule.points() {
            self.point(reference, constraint.id)?;
        }

        let point = |reference| self.point(reference, constraint.id);
        let segment = |segment: SketchSegmentRef| -> Result<(solver::PointId, solver::PointId)> {
            Ok((point(segment.from)?, point(segment.to)?))
        };

        // Directions and values cross unchanged. `from` stays `from`, `a`
        // stays `a`, and a distance in millimetres stays a distance in
        // millimetres, because the two contracts were written to say the same
        // eight things in the same units.
        Ok(match constraint.rule {
            SketchConstraintRule::Coincident { a, b } => solver::Constraint::Coincident {
                a: point(a)?,
                b: point(b)?,
            },
            SketchConstraintRule::Fixed { point: at, x, y } => solver::Constraint::Fixed {
                point: point(at)?,
                x,
                y,
            },
            SketchConstraintRule::Distance { a, b, distance } => solver::Constraint::Distance {
                a: point(a)?,
                b: point(b)?,
                distance,
            },
            SketchConstraintRule::Horizontal { a, b } => solver::Constraint::Horizontal {
                a: point(a)?,
                b: point(b)?,
            },
            SketchConstraintRule::Vertical { a, b } => solver::Constraint::Vertical {
                a: point(a)?,
                b: point(b)?,
            },
            SketchConstraintRule::EqualLength { a, b } => solver::Constraint::EqualLength {
                a: segment(a)?,
                b: segment(b)?,
            },
            SketchConstraintRule::Perpendicular { a, b } => solver::Constraint::Perpendicular {
                a: segment(a)?,
                b: segment(b)?,
            },
            SketchConstraintRule::Parallel { a, b } => solver::Constraint::Parallel {
                a: segment(a)?,
                b: segment(b)?,
            },
            ref other => {
                return Err(CadError::unsupported(format!(
                    "constraint {} is a {} constraint, which this build stores but cannot state to \
                     a solver",
                    constraint.id,
                    rule_name(other)
                )));
            }
        })
    }

    /// The solver's name for one of the document's points.
    fn point(
        &self,
        reference: SketchPointRef,
        constraint: StableEntityId,
    ) -> Result<solver::PointId> {
        self.point_of.get(&reference).copied().ok_or_else(|| {
            CadError::constraint(format!(
                "constraint {constraint} names {reference}, which is not a stored point of this \
                 sketch"
            ))
        })
    }

    /// What an outcome means for the sketch it came from.
    ///
    /// Split out from [`solved`] so that the three answers can be gated
    /// directly, in any build, without a library to produce them.
    fn interpret(
        &self,
        sketch: &Sketch,
        id: ObjectId,
        outcome: solver::Outcome,
    ) -> Result<(Sketch, SketchSolveReport)> {
        match outcome {
            // A solved sketch is built, whether or not it was fully
            // constrained. An under-constrained sketch has coordinates that
            // satisfy everything said about it and freedom left over, and a
            // redundant one says something twice; neither is a reason to
            // refuse to draw what the user drew. Nothing here calls such a
            // sketch fully constrained, because nothing here says so at all;
            // it says how much freedom is left, and lets a reader conclude it.
            solver::Outcome::Solved(solution) => {
                let report = SketchSolveReport {
                    degrees_of_freedom: solution.degrees_of_freedom(),
                    redundant: self.named_in_document_order(sketch, solution.redundant())?,
                };
                Ok((self.apply(sketch, &solution)?, report))
            }

            // The one answer that names constraints which cannot all hold.
            // Every number in it was minted by `read` from a constraint of
            // this sketch, so it can be said in the document's words; the
            // facts are built here, where the sketch and the answer are both
            // in hand, and travel with the refusal because there is no scene
            // for them to travel in. Nothing partial is built either way.
            solver::Outcome::Conflicting { constraints, .. } => {
                let named = self.named_in_document_order(sketch, &constraints)?;
                let conflict = SketchConflict {
                    sketch: id,
                    constraints: conflicting_constraints(sketch, &named)?,
                };
                Err(CadError::constraint_because(
                    format!(
                        "this sketch's constraints cannot all hold at once; the solver cannot \
                         satisfy {} together with the rest",
                        self.name_all(&constraints)
                    ),
                    conflict,
                ))
            }

            // No coordinates. The solver has some — it minimised an error
            // function and stopped somewhere — and publishing them is the one
            // failure that turns a solver problem into a wrong drawing,
            // because a half-moved sketch extrudes into a solid that looks
            // finished.
            solver::Outcome::DidNotConverge { .. } => Err(CadError::constraint(
                "the sketch solver could not satisfy this sketch's constraints; where it stopped \
                 is not a solution and is not built",
            )),

            // An answer this build has no reading of. Described rather than
            // printed: a future outcome could carry the solver's own
            // identifiers, and debug-printing one here would publish them by
            // accident in exactly the situation nobody is looking at.
            _ => Err(CadError::unsupported(
                "the sketch solver gave an answer this build has no reading of, so this sketch is \
                 not built",
            )),
        }
    }

    /// The solved sketch: a clone of the stored one with new coordinates.
    fn apply(&self, sketch: &Sketch, solution: &solver::Solution) -> Result<Sketch> {
        let mut solved = sketch.clone();
        for curve in &mut solved.curves {
            for &selector in stored_points(&curve.geometry) {
                let reference = SketchPointRef::new(curve.id, selector);
                let Some(&id) = self.point_of.get(&reference) else {
                    continue;
                };
                let position = solution.position(id).ok_or_else(|| {
                    CadError::constraint(format!(
                        "the solver returned no position for {reference}, which it was asked about"
                    ))
                })?;
                write_point(
                    &mut curve.geometry,
                    selector,
                    Point2::new(position.x, position.y)?,
                )?;
            }
        }
        Ok(solved)
    }

    /// The document's names for the constraints a solve blamed, whichever
    /// way it blamed them.
    ///
    /// Every number the solver hands back is looked up, and a lookup that
    /// fails is a refusal rather than a gap. A constraint this sketch was
    /// never asked about, or one blamed twice, means the answer is about some
    /// other system than the one that was stated, and reporting the part that
    /// did resolve would publish a partial account of a sketch as a complete
    /// one.
    ///
    /// The order is read off the document. The numbers this module minted
    /// follow storage order today and the solver returns them sorted, so
    /// walking the answer would give the same list — and would give somebody
    /// else's list the moment either of those changed. What is promised is the
    /// order a person reading their own drawing sees, so that is what is
    /// computed.
    fn named_in_document_order(
        &self,
        sketch: &Sketch,
        blamed: &[solver::ConstraintId],
    ) -> Result<Vec<StableEntityId>> {
        let mut named = BTreeSet::new();
        for id in blamed {
            let Some(&constraint) = self.constraint_ref.get(id) else {
                // Said without the number. It is the one this module minted a
                // moment ago, and a message carrying it would invite a reader
                // to look for it in a sketch that has no such word.
                return Err(CadError::constraint(
                    "the solver blamed a constraint that this sketch never stated, so there is \
                     nothing this drawing could be told about it",
                ));
            };
            if !named.insert(constraint) {
                return Err(CadError::constraint(format!(
                    "the solver blamed constraint {constraint} more than once, and one \
                     constraint is one answer"
                )));
            }
        }

        let mut remaining = named;
        Ok(sketch
            .constraints
            .iter()
            .filter(|constraint| remaining.remove(&constraint.id))
            .map(|constraint| constraint.id)
            .collect())
    }

    /// Why the solver would not answer, said in the document's words.
    ///
    /// Deliberately not `error.to_string()`. Several of the solver's own
    /// messages name the identifiers it was given, which are the ones this
    /// module minted a moment ago and will not mint again.
    fn refusal(&self, error: &solver::SolverError) -> CadError {
        match error {
            // The one refusal that is about this build rather than this
            // sketch. Nothing may fall back to arithmetic of its own here: a
            // second solver would be a second answer, and the whole point of
            // storing constraints is that the answer does not depend on which
            // build opened the file.
            solver::SolverError::Unavailable(unavailable) => CadError::unsupported(format!(
                "this sketch carries {} constraint(s) and {unavailable}",
                self.constraint_ref.len()
            )),
            solver::SolverError::UnknownPoint { constraint, point } => {
                CadError::constraint(format!(
                    "constraint {} names {}, which the solver was not told about",
                    self.name(*constraint),
                    self.point_name(*point)
                ))
            }
            solver::SolverError::DuplicatePoint(point) => CadError::constraint(format!(
                "{} was stated to the solver more than once",
                self.point_name(*point)
            )),
            solver::SolverError::DuplicateConstraint(constraint) => CadError::constraint(format!(
                "constraint {} was stated to the solver more than once",
                self.name(*constraint)
            )),
            solver::SolverError::NotFinite(solver::NotFinite::PointCoordinate(point)) => {
                CadError::constraint(format!(
                    "{} has a coordinate that is not a number",
                    self.point_name(*point)
                ))
            }
            solver::SolverError::NotFinite(solver::NotFinite::ConstraintParameter(constraint)) => {
                CadError::constraint(format!(
                    "constraint {} carries a value that is not a number",
                    self.name(*constraint)
                ))
            }
            solver::SolverError::Native(failure) => {
                CadError::constraint(format!("this sketch was refused: {failure}"))
            }
            // A starting state is never supplied from here, so these two
            // cannot arise; and a variant added to the solver's error later
            // would arrive with words this file has not read. Neither is
            // restated by printing the error, which is the one thing that
            // could carry a transient identifier out.
            solver::SolverError::StateSize { .. } | solver::SolverError::UnknownPointInState(_) => {
                CadError::constraint(
                    "the sketch solver was given a starting state it could not use",
                )
            }
            _ => CadError::constraint(
                "the sketch solver refused this sketch, for a reason this build cannot restate",
            ),
        }
    }

    /// The document's name for a constraint the solver blamed.
    fn name(&self, constraint: solver::ConstraintId) -> String {
        match self.constraint_ref.get(&constraint) {
            Some(id) => id.to_string(),
            // Unreachable while every constraint in the system came from
            // `read`. Said in words rather than by falling back to the
            // solver's number, which would publish exactly what this module
            // exists to keep to itself.
            None => "a constraint this sketch does not hold".to_string(),
        }
    }

    fn name_all(&self, constraints: &[solver::ConstraintId]) -> String {
        if constraints.is_empty() {
            return "constraints it would not name".to_string();
        }
        constraints
            .iter()
            .map(|id| format!("constraint {}", self.name(*id)))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn point_name(&self, point: solver::PointId) -> String {
        match self.point_ref.get(&point) {
            Some(reference) => reference.to_string(),
            None => "a point this sketch does not hold".to_string(),
        }
    }
}

/// Where a stored point is now.
fn stored_coordinate(geometry: &SketchGeometry, selector: SketchPointSelector) -> Option<Point2> {
    match (selector, geometry) {
        (SketchPointSelector::At, SketchGeometry::Point { at }) => Some(*at),
        (SketchPointSelector::Start, SketchGeometry::Line { start, .. }) => Some(*start),
        (SketchPointSelector::End, SketchGeometry::Line { end, .. }) => Some(*end),
        _ => None,
    }
}

/// What to call a rule in a message about it.
fn rule_name(rule: &SketchConstraintRule) -> &'static str {
    match rule {
        SketchConstraintRule::Coincident { .. } => "coincident",
        SketchConstraintRule::Fixed { .. } => "fixed",
        SketchConstraintRule::Distance { .. } => "distance",
        SketchConstraintRule::Horizontal { .. } => "horizontal",
        SketchConstraintRule::Vertical { .. } => "vertical",
        SketchConstraintRule::EqualLength { .. } => "equal length",
        SketchConstraintRule::Perpendicular { .. } => "perpendicular",
        SketchConstraintRule::Parallel { .. } => "parallel",
        _ => "kind this build does not name",
    }
}

#[cfg(test)]
mod tests {
    // A test asserting the shape of a value has nowhere to return an error to.
    #![allow(clippy::panic)]

    use super::*;
    use ferritecad_document::{SketchCurve, SketchSegmentRef};
    use ferritecad_types::{ErrorKind, ObjectId};

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

    fn point(id: StableEntityId, at: (f64, f64)) -> SketchCurve {
        SketchCurve {
            id,
            construction: false,
            geometry: SketchGeometry::Point {
                at: Point2::new(at.0, at.1).expect("finite"),
            },
        }
    }

    fn sketch(curves: Vec<SketchCurve>, constraints: Vec<SketchConstraint>) -> Sketch {
        Sketch {
            plane: ObjectId::new(),
            curves,
            constraints,
        }
    }

    fn at(curve: StableEntityId, selector: SketchPointSelector) -> SketchPointRef {
        SketchPointRef::new(curve, selector)
    }

    fn rule(rule: SketchConstraintRule) -> SketchConstraint {
        SketchConstraint {
            id: StableEntityId::new(),
            rule,
        }
    }

    /// The solver's name for a point, looked up the way the answer will be.
    fn named(translation: &Translation, reference: SketchPointRef) -> solver::PointId {
        *translation
            .point_of
            .get(&reference)
            .expect("the translation states every stored point")
    }

    fn only_constraint(translation: &Translation) -> solver::Constraint {
        let stated = translation.stated().constraints();
        assert_eq!(stated.len(), 1, "this fixture states one constraint");
        stated[0].1
    }

    // -----------------------------------------------------------------
    // Points
    // -----------------------------------------------------------------

    #[test]
    fn every_stored_point_of_every_curve_gets_an_identifier_of_its_own() {
        use SketchPointSelector::{At, End, Start};
        let (a, b, c) = (
            StableEntityId::new(),
            StableEntityId::new(),
            StableEntityId::new(),
        );
        let mut guide = line(c, (5.0, 5.0), (6.0, 6.0));
        guide.construction = true;

        let sketch = sketch(
            vec![line(a, (0.0, 0.0), (1.0, 2.0)), point(b, (3.0, 4.0)), guide],
            vec![rule(SketchConstraintRule::Coincident {
                a: at(a, Start),
                b: at(b, At),
            })],
        );
        let translation = Translation::read(&sketch).expect("translates");

        // Two for the line, one for the point, two for the construction line.
        assert_eq!(translation.stated().points().len(), 5);
        for reference in [
            at(a, Start),
            at(a, End),
            at(b, At),
            at(c, Start),
            at(c, End),
        ] {
            assert!(
                translation.point_of.contains_key(&reference),
                "{reference} was never stated to the solver"
            );
        }

        // Construction geometry is stated alongside model geometry. A sketch
        // is usually held together by its guides, and a solve that dropped
        // them would lose its skeleton while keeping its skin.
        assert!(translation.point_of.contains_key(&at(c, Start)));
    }

    #[test]
    fn the_stated_coordinate_is_the_one_the_document_stores() {
        use SketchPointSelector::{End, Start};
        let a = StableEntityId::new();
        let sketch = sketch(
            vec![line(a, (1.5, -2.5), (30.0, 40.0))],
            vec![rule(SketchConstraintRule::Horizontal {
                a: at(a, Start),
                b: at(a, End),
            })],
        );
        let translation = Translation::read(&sketch).expect("translates");

        let start = named(&translation, at(a, Start));
        let end = named(&translation, at(a, End));
        let position = |id| {
            *translation
                .stated()
                .points()
                .iter()
                .find(|p| p.point == id)
                .expect("stated")
        };
        assert_eq!((position(start).x, position(start).y), (1.5, -2.5));
        assert_eq!((position(end).x, position(end).y), (30.0, 40.0));
    }

    #[test]
    fn two_points_at_the_same_place_stay_two_points() {
        // The one thing a translation must never decide on the user's behalf.
        // Welding by coordinate would silently satisfy a coincidence nobody
        // stated, and would make a sketch whose corners happen to touch
        // unbreakable — the drag that pulls them apart could not be expressed.
        use SketchPointSelector::{End, Start};
        let (a, b) = (StableEntityId::new(), StableEntityId::new());
        let sketch = sketch(
            vec![
                line(a, (0.0, 0.0), (10.0, 0.0)),
                line(b, (10.0, 0.0), (10.0, 10.0)),
            ],
            vec![rule(SketchConstraintRule::Coincident {
                a: at(a, End),
                b: at(b, Start),
            })],
        );
        let translation = Translation::read(&sketch).expect("translates");

        assert_eq!(translation.stated().points().len(), 4);
        assert_ne!(
            named(&translation, at(a, End)),
            named(&translation, at(b, Start)),
            "two coincident corners were welded into one point"
        );
    }

    #[test]
    fn a_circle_offers_no_point_to_solve_for() {
        let circle = StableEntityId::new();
        let sketch = sketch(
            vec![SketchCurve {
                id: circle,
                construction: false,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(1.0, 2.0).expect("finite"),
                    radius: 3.0,
                },
            }],
            Vec::new(),
        );
        let translation = Translation::read(&sketch).expect("translates");
        assert!(
            translation.stated().points().is_empty(),
            "a circle's centre is not a point this slice can solve for"
        );
    }

    // -----------------------------------------------------------------
    // The eight families
    // -----------------------------------------------------------------

    #[test]
    fn every_family_becomes_the_constraint_that_means_it() {
        use SketchPointSelector::{At, End, Start};
        let (a, b) = (StableEntityId::new(), StableEntityId::new());
        let pin = StableEntityId::new();
        let curves = || {
            vec![
                line(a, (0.0, 0.0), (10.0, 0.0)),
                line(b, (10.0, 0.0), (10.0, 8.0)),
                point(pin, (2.0, 3.0)),
            ]
        };
        let seg = |edge| SketchSegmentRef::new(at(edge, Start), at(edge, End));

        // Each family, and what it must become. Written out rather than
        // generated: the whole risk here is that a family is translated into
        // the constraint next to it, and a table that built both sides from
        // one description could not tell.
        /// What one family is, and what it must have become.
        type Case = (
            SketchConstraintRule,
            Box<dyn Fn(&Translation) -> solver::Constraint>,
        );

        let cases: Vec<Case> = vec![
            (
                SketchConstraintRule::Coincident {
                    a: at(a, End),
                    b: at(b, Start),
                },
                Box::new(move |t: &Translation| solver::Constraint::Coincident {
                    a: named(t, at(a, End)),
                    b: named(t, at(b, Start)),
                }),
            ),
            (
                SketchConstraintRule::Fixed {
                    point: at(pin, At),
                    x: 4.5,
                    y: -6.25,
                },
                Box::new(move |t: &Translation| solver::Constraint::Fixed {
                    point: named(t, at(pin, At)),
                    x: 4.5,
                    y: -6.25,
                }),
            ),
            (
                SketchConstraintRule::Distance {
                    a: at(a, Start),
                    b: at(a, End),
                    distance: 62.5,
                },
                Box::new(move |t: &Translation| solver::Constraint::Distance {
                    a: named(t, at(a, Start)),
                    b: named(t, at(a, End)),
                    distance: 62.5,
                }),
            ),
            (
                SketchConstraintRule::Horizontal {
                    a: at(a, Start),
                    b: at(a, End),
                },
                Box::new(move |t: &Translation| solver::Constraint::Horizontal {
                    a: named(t, at(a, Start)),
                    b: named(t, at(a, End)),
                }),
            ),
            (
                SketchConstraintRule::Vertical {
                    a: at(b, Start),
                    b: at(b, End),
                },
                Box::new(move |t: &Translation| solver::Constraint::Vertical {
                    a: named(t, at(b, Start)),
                    b: named(t, at(b, End)),
                }),
            ),
            (
                SketchConstraintRule::EqualLength {
                    a: seg(a),
                    b: seg(b),
                },
                Box::new(move |t: &Translation| solver::Constraint::EqualLength {
                    a: (named(t, at(a, Start)), named(t, at(a, End))),
                    b: (named(t, at(b, Start)), named(t, at(b, End))),
                }),
            ),
            (
                SketchConstraintRule::Perpendicular {
                    a: seg(a),
                    b: seg(b),
                },
                Box::new(move |t: &Translation| solver::Constraint::Perpendicular {
                    a: (named(t, at(a, Start)), named(t, at(a, End))),
                    b: (named(t, at(b, Start)), named(t, at(b, End))),
                }),
            ),
            (
                SketchConstraintRule::Parallel {
                    a: seg(a),
                    b: seg(b),
                },
                Box::new(move |t: &Translation| solver::Constraint::Parallel {
                    a: (named(t, at(a, Start)), named(t, at(a, End))),
                    b: (named(t, at(b, Start)), named(t, at(b, End))),
                }),
            ),
        ];

        assert_eq!(cases.len(), 8, "there are eight families and no more");

        for (family, expected) in cases {
            let sketch = sketch(curves(), vec![rule(family)]);
            let translation = Translation::read(&sketch).expect("translates");
            assert_eq!(
                only_constraint(&translation),
                expected(&translation),
                "{family:?} was not stated as itself"
            );
        }
    }

    #[test]
    fn a_segments_ends_are_not_exchanged() {
        // Perpendicularity and parallelism do not care which way a segment
        // runs, so an exchange here is invisible in those two families and
        // wrong in the answer's provenance. Stated against the one family
        // where the direction is checkable at all: what `a` and `b` are.
        use SketchPointSelector::{End, Start};
        let (a, b) = (StableEntityId::new(), StableEntityId::new());
        let sketch = sketch(
            vec![
                line(a, (0.0, 0.0), (10.0, 0.0)),
                line(b, (10.0, 0.0), (10.0, 8.0)),
            ],
            vec![rule(SketchConstraintRule::EqualLength {
                a: SketchSegmentRef::new(at(a, Start), at(a, End)),
                b: SketchSegmentRef::new(at(b, End), at(b, Start)),
            })],
        );
        let translation = Translation::read(&sketch).expect("translates");

        let solver::Constraint::EqualLength {
            a: first,
            b: second,
        } = only_constraint(&translation)
        else {
            panic!("equal length must become equal length");
        };
        assert_eq!(
            first,
            (
                named(&translation, at(a, Start)),
                named(&translation, at(a, End))
            )
        );
        assert_eq!(
            second,
            (
                named(&translation, at(b, End)),
                named(&translation, at(b, Start))
            ),
            "the second segment was stated the way round it was not written"
        );
    }

    #[test]
    fn a_fixed_pin_keeps_x_as_x() {
        let pin = StableEntityId::new();
        let sketch = sketch(
            vec![point(pin, (0.0, 0.0))],
            vec![rule(SketchConstraintRule::Fixed {
                point: at(pin, SketchPointSelector::At),
                x: 3.0,
                y: 91.0,
            })],
        );
        let translation = Translation::read(&sketch).expect("translates");
        let solver::Constraint::Fixed { x, y, .. } = only_constraint(&translation) else {
            panic!("a pin must become a pin");
        };
        assert_eq!((x, y), (3.0, 91.0), "the pin's coordinates were exchanged");
    }

    #[test]
    fn a_distance_crosses_in_the_units_it_was_written_in() {
        let a = StableEntityId::new();
        let sketch = sketch(
            vec![line(a, (0.0, 0.0), (1.0, 0.0))],
            vec![rule(SketchConstraintRule::Distance {
                a: at(a, SketchPointSelector::Start),
                b: at(a, SketchPointSelector::End),
                distance: 61.75,
            })],
        );
        let translation = Translation::read(&sketch).expect("translates");
        let solver::Constraint::Distance { distance, .. } = only_constraint(&translation) else {
            panic!("a distance must become a distance");
        };
        assert_eq!(distance, 61.75);
    }

    // -----------------------------------------------------------------
    // Storage order means nothing
    // -----------------------------------------------------------------

    #[test]
    fn reordering_the_curves_moves_no_point() {
        use SketchPointSelector::{End, Start};
        let (a, b) = (StableEntityId::new(), StableEntityId::new());
        let curves = vec![
            line(a, (0.0, 0.0), (10.0, 0.0)),
            line(b, (10.0, 0.0), (10.0, 8.0)),
        ];
        let constraints = vec![rule(SketchConstraintRule::Coincident {
            a: at(a, End),
            b: at(b, Start),
        })];

        let forwards =
            Translation::read(&sketch(curves.clone(), constraints.clone())).expect("translates");
        let mut reversed_curves = curves;
        reversed_curves.reverse();
        let backwards =
            Translation::read(&sketch(reversed_curves, constraints)).expect("translates");

        // The numbers differ, because they follow storage order and are thrown
        // away. What each of them means does not.
        let coordinate = |t: &Translation, reference| {
            let id = named(t, reference);
            let position = t
                .stated()
                .points()
                .iter()
                .find(|p| p.point == id)
                .copied()
                .expect("stated");
            (position.x, position.y)
        };
        for reference in [at(a, Start), at(a, End), at(b, Start), at(b, End)] {
            assert_eq!(
                coordinate(&forwards, reference),
                coordinate(&backwards, reference),
                "{reference} moved when the storage order changed"
            );
        }
    }

    // -----------------------------------------------------------------
    // Writing an answer back
    // -----------------------------------------------------------------

    #[test]
    fn each_selector_writes_the_field_it_names() {
        let mut geometry = SketchGeometry::Line {
            start: Point2::ORIGIN,
            end: Point2::ORIGIN,
        };
        write_point(
            &mut geometry,
            SketchPointSelector::Start,
            Point2::new(1.0, 2.0).expect("finite"),
        )
        .expect("a line has a start");
        write_point(
            &mut geometry,
            SketchPointSelector::End,
            Point2::new(3.0, 4.0).expect("finite"),
        )
        .expect("a line has an end");

        let SketchGeometry::Line { start, end } = geometry else {
            panic!("a line stays a line");
        };
        assert_eq!(
            (start.x, start.y),
            (1.0, 2.0),
            "start took the end's answer"
        );
        assert_eq!((end.x, end.y), (3.0, 4.0), "end took the start's answer");

        let mut standalone = SketchGeometry::Point { at: Point2::ORIGIN };
        write_point(
            &mut standalone,
            SketchPointSelector::At,
            Point2::new(5.0, 6.0).expect("finite"),
        )
        .expect("a point has a position");
        let SketchGeometry::Point { at } = standalone else {
            panic!("a point stays a point");
        };
        assert_eq!((at.x, at.y), (5.0, 6.0));
    }

    #[test]
    fn a_selector_that_does_not_fit_its_geometry_is_refused() {
        let mut geometry = SketchGeometry::Point { at: Point2::ORIGIN };
        let error = write_point(
            &mut geometry,
            SketchPointSelector::Start,
            Point2::new(1.0, 1.0).expect("finite"),
        )
        .expect_err("a point has no start");
        assert_eq!(error.kind(), ErrorKind::Constraint);
    }

    // -----------------------------------------------------------------
    // The three answers
    // -----------------------------------------------------------------

    /// A plate with one constraint, and the translation of it.
    fn one_constraint() -> (Sketch, Translation, StableEntityId) {
        let a = StableEntityId::new();
        let sketch = sketch(
            vec![line(a, (0.0, 0.0), (10.0, 0.0))],
            vec![rule(SketchConstraintRule::Horizontal {
                a: at(a, SketchPointSelector::Start),
                b: at(a, SketchPointSelector::End),
            })],
        );
        let id = sketch.constraints[0].id;
        let translation = Translation::read(&sketch).expect("translates");
        (sketch, translation, id)
    }

    #[test]
    fn a_conflict_is_named_in_the_documents_own_words() {
        let (sketch, translation, id) = one_constraint();
        let blamed = translation
            .stated()
            .constraints()
            .first()
            .map(|(stated, _)| *stated)
            .expect("one constraint");

        let error = translation
            .interpret(
                &sketch,
                ObjectId::new(),
                solver::Outcome::Conflicting {
                    constraints: vec![blamed],
                    redundant: Vec::new(),
                },
            )
            .expect_err("a conflicting sketch is not built");

        assert_eq!(error.kind(), ErrorKind::Constraint);
        let message = error.to_string();
        assert!(
            message.contains(&id.to_string()),
            "the conflict must be reported against the stored identifier: {message}"
        );
        for forbidden in ["ConstraintId", "PointId"] {
            assert!(!message.contains(forbidden), "{message}");
        }
    }

    #[test]
    fn a_sketch_that_did_not_converge_publishes_no_coordinates() {
        let (sketch, translation, _) = one_constraint();
        let error = translation
            .interpret(
                &sketch,
                ObjectId::new(),
                solver::Outcome::DidNotConverge {
                    worst_residual: Some(4.2),
                },
            )
            .expect_err("where a solver stopped is not a solution");

        // The type is most of the claim: there is no sketch here to build
        // from, so nothing partial can be published however the caller reads
        // the answer.
        assert_eq!(error.kind(), ErrorKind::Constraint);
        let message = error.to_string();
        assert!(
            message.contains("not a solution"),
            "a refusal must say it is one: {message}"
        );
    }

    #[test]
    fn a_missing_solver_is_a_missing_component_and_not_a_wrong_sketch() {
        let (_, translation, _) = one_constraint();
        let error = translation.refusal(&solver::SolverError::Unavailable(
            solver::Unavailable::NotLinked,
        ));
        assert_eq!(
            error.kind(),
            ErrorKind::Unsupported,
            "no solver is this build's problem, not the drawing's"
        );
    }

    // -----------------------------------------------------------------
    // Nothing is asked when there is nothing to ask
    // -----------------------------------------------------------------

    #[test]
    fn a_sketch_with_no_constraints_is_not_translated_at_all() {
        let a = StableEntityId::new();
        let plain = sketch(vec![line(a, (0.0, 0.0), (10.0, 0.0))], Vec::new());
        assert!(
            solved(&plain, ObjectId::new())
                .expect("no constraints, no failure")
                .is_none(),
            "an unconstrained sketch must not be handed to a solver"
        );
    }

    #[test]
    fn a_reference_into_geometry_with_no_such_point_is_refused() {
        let circle = StableEntityId::new();
        let sketch = sketch(
            vec![SketchCurve {
                id: circle,
                construction: false,
                geometry: SketchGeometry::Circle {
                    center: Point2::ORIGIN,
                    radius: 3.0,
                },
            }],
            vec![rule(SketchConstraintRule::Horizontal {
                a: at(circle, SketchPointSelector::Start),
                b: at(circle, SketchPointSelector::End),
            })],
        );
        let error = Translation::read(&sketch)
            .expect_err("a circle has no start, so there is nothing to solve for");
        assert_eq!(error.kind(), ErrorKind::Constraint);
    }

    // -----------------------------------------------------------------
    // What a solve reports, in the document's own order and words
    // -----------------------------------------------------------------

    /// Four constraints over one line, so an answer about them can be given
    /// in an order that is not the document's.
    ///
    /// Two horizontals and two verticals: nothing here is solvable and nothing
    /// here is asked to be. What is under test is the road back from a blamed
    /// constraint to the word the document uses for it, which is travelled
    /// whatever the geometry was.
    fn four_constraints() -> (Sketch, Translation, Vec<StableEntityId>) {
        let a = StableEntityId::new();
        let ends = || SketchConstraintRule::Horizontal {
            a: at(a, SketchPointSelector::Start),
            b: at(a, SketchPointSelector::End),
        };
        let upright = || SketchConstraintRule::Vertical {
            a: at(a, SketchPointSelector::Start),
            b: at(a, SketchPointSelector::End),
        };
        let sketch = sketch(
            vec![line(a, (0.0, 0.0), (10.0, 0.0))],
            vec![rule(ends()), rule(upright()), rule(ends()), rule(upright())],
        );
        let ids = sketch.constraints.iter().map(|one| one.id).collect();
        let translation = Translation::read(&sketch).expect("translates");
        (sketch, translation, ids)
    }

    /// The solver's name for the constraint the document stores at `index`.
    fn stated_at(translation: &Translation, index: usize) -> solver::ConstraintId {
        translation
            .stated()
            .constraints()
            .get(index)
            .map(|(id, _)| *id)
            .expect("the sketch stated this constraint")
    }

    // -----------------------------------------------------------------
    // Joining a blamed identifier to the rule the document stores under it
    //
    // Nothing here asks a solver anything. What a real planegcs answers for a
    // real conflicting document is measured in `tests/solve_conflict.rs`; what
    // is measured there is a set of identifiers it *can* produce, and these
    // are the answers it cannot. Today's solver crate sorts and deduplicates
    // its answer and refuses a tag outside the system it was given, so an
    // unknown, a repeated and a foreign identifier cannot arrive from it. Each
    // of those is one decision away from arriving, and the decision is not
    // this file's, which is why they are gated here rather than assumed.
    // -----------------------------------------------------------------

    #[test]
    fn every_blamed_identifier_carries_the_rule_stored_under_exactly_it() {
        let (sketch, _, ids) = four_constraints();
        let joined = conflicting_constraints(&sketch, &ids).expect("all four are stored here");

        assert_eq!(joined.len(), 4);
        for (entry, stored) in joined.iter().zip(&sketch.constraints) {
            assert_eq!(entry.id(), stored.id);
            assert_eq!(entry.rule(), &stored.rule);
        }
    }

    #[test]
    fn the_rule_follows_the_identifier_and_not_the_position() {
        let (sketch, _, ids) = four_constraints();
        // The second and the fourth, in that order. Their rules differ from
        // the ones stored first and third, so a join by index would answer
        // with the wrong two and still hand back a list of the right length.
        let asked = [ids[1], ids[3]];
        let joined = conflicting_constraints(&sketch, &asked).expect("both are stored here");

        assert_eq!(joined.len(), 2);
        assert_eq!(joined[0].id(), ids[1]);
        assert_eq!(joined[0].rule(), &sketch.constraints[1].rule);
        assert_eq!(joined[1].id(), ids[3]);
        assert_eq!(joined[1].rule(), &sketch.constraints[3].rule);
        assert_ne!(
            joined[0].rule(),
            &sketch.constraints[0].rule,
            "the first stored rule answered for the second identifier"
        );
    }

    #[test]
    fn two_identical_rules_under_two_identifiers_stay_two_entries() {
        let (sketch, _, ids) = four_constraints();
        // The first and the third say exactly the same thing.
        assert_eq!(sketch.constraints[0].rule, sketch.constraints[2].rule);
        let joined =
            conflicting_constraints(&sketch, &[ids[0], ids[2]]).expect("both are stored here");

        assert_eq!(joined.len(), 2, "two constraints became one report");
        assert_eq!(joined[0].id(), ids[0]);
        assert_eq!(joined[1].id(), ids[2]);
        assert_ne!(joined[0].id(), joined[1].id());
        assert_eq!(joined[0].rule(), joined[1].rule());
    }

    #[test]
    fn an_identifier_this_sketch_does_not_store_is_refused() {
        let (sketch, _, ids) = four_constraints();
        let stranger = StableEntityId::new();
        let error = conflicting_constraints(&sketch, &[ids[0], stranger])
            .expect_err("a sketch cannot be told about a constraint it does not hold");

        assert_eq!(error.kind(), ErrorKind::Constraint);
        assert!(
            error.to_string().contains(&stranger.to_string()),
            "the refusal did not say which identifier it could not place: {error}"
        );
    }

    #[test]
    fn an_identifier_belonging_to_the_sketch_next_door_is_refused() {
        let (sketch, _, _) = four_constraints();
        // A second sketch drawn over the same curve, so its first constraint
        // says word for word what this one's first constraint says and sits at
        // the same position. Only the identifier differs, and the identifier
        // is the whole of what may decide the answer.
        let neighbour = self::tests::sketch(
            sketch.curves.clone(),
            vec![rule(sketch.constraints[0].rule)],
        );
        assert_eq!(
            sketch.constraints[0].rule, neighbour.constraints[0].rule,
            "this gate needs the neighbour to say the same thing"
        );
        assert_ne!(sketch.constraints[0].id, neighbour.constraints[0].id);

        let error = conflicting_constraints(&sketch, &[neighbour.constraints[0].id])
            .expect_err("a constraint of another sketch is not a constraint of this one");

        assert_eq!(error.kind(), ErrorKind::Constraint);
    }

    #[test]
    fn a_conflict_carries_the_rules_of_the_constraints_it_blamed() {
        let (sketch, translation, ids) = four_constraints();
        let sketch_id = ObjectId::new();
        let blamed = vec![stated_at(&translation, 2), stated_at(&translation, 0)];

        let error = translation
            .interpret(
                &sketch,
                sketch_id,
                solver::Outcome::Conflicting {
                    constraints: blamed,
                    redundant: Vec::new(),
                },
            )
            .expect_err("a conflicting sketch is not built");

        let conflict =
            SketchConflict::of(&error).expect("a conflict carries what a caller can act on");
        assert_eq!(conflict.sketch(), sketch_id);
        // Reported back to front, reported out in the document's order.
        assert_eq!(
            conflict
                .constraints()
                .iter()
                .map(ConflictingConstraint::id)
                .collect::<Vec<_>>(),
            vec![ids[0], ids[2]]
        );
        assert_eq!(
            conflict.constraints()[0].rule(),
            &sketch.constraints[0].rule
        );
        assert_eq!(
            conflict.constraints()[1].rule(),
            &sketch.constraints[2].rule
        );
    }

    #[test]
    fn a_blamed_identifier_this_translation_never_minted_is_refused() {
        let (sketch, translation, _) = four_constraints();
        let error = translation
            .interpret(
                &sketch,
                ObjectId::new(),
                solver::Outcome::Conflicting {
                    // One past the last this sketch stated.
                    constraints: vec![solver::ConstraintId(4)],
                    redundant: Vec::new(),
                },
            )
            .expect_err("a number this sketch never issued names nothing in it");

        assert_eq!(error.kind(), ErrorKind::Constraint);
        assert!(
            SketchConflict::of(&error).is_none(),
            "a refusal about an answer that could not be placed carried a report anyway: {error}"
        );
    }

    #[test]
    fn a_constraint_blamed_twice_for_a_conflict_is_refused() {
        let (sketch, translation, _) = four_constraints();
        let once = stated_at(&translation, 1);
        let error = translation
            .interpret(
                &sketch,
                ObjectId::new(),
                solver::Outcome::Conflicting {
                    constraints: vec![once, once],
                    redundant: Vec::new(),
                },
            )
            .expect_err("one constraint is one answer");

        assert_eq!(error.kind(), ErrorKind::Constraint);
        assert!(
            SketchConflict::of(&error).is_none(),
            "an answer that named one constraint twice was reported as a conflict: {error}"
        );
    }

    #[test]
    fn a_sketch_that_did_not_converge_carries_no_conflict() {
        let (sketch, translation, _) = four_constraints();
        let error = translation
            .interpret(
                &sketch,
                ObjectId::new(),
                solver::Outcome::DidNotConverge {
                    worst_residual: Some(0.5),
                },
            )
            .expect_err("where a solver stopped is not a solution");

        assert!(
            SketchConflict::of(&error).is_none(),
            "a sketch that would not converge was reported as one whose constraints disagree"
        );
    }

    #[test]
    fn a_solver_refusal_carries_no_conflict() {
        let (_, translation, _) = four_constraints();
        for refusal in [
            translation.refusal(&solver::SolverError::Unavailable(
                solver::Unavailable::NotLinked,
            )),
            translation.refusal(&solver::SolverError::DuplicateConstraint(
                solver::ConstraintId(0),
            )),
            translation.refusal(&solver::SolverError::UnknownPoint {
                constraint: solver::ConstraintId(0),
                point: solver::PointId(9),
            }),
        ] {
            assert!(
                SketchConflict::of(&refusal).is_none(),
                "a solver that would not answer was reported as a set of constraints that \
                 disagree: {refusal}"
            );
        }
    }

    #[test]
    fn what_repeated_is_reported_in_the_order_the_document_stores_it() {
        // The answer arrives back to front. What comes out follows the
        // document, which is the only order a person reading their own drawing
        // has ever seen.
        let (sketch, translation, ids) = four_constraints();
        let backwards: Vec<solver::ConstraintId> = (0..4)
            .rev()
            .map(|index| stated_at(&translation, index))
            .collect();

        let reported = translation
            .named_in_document_order(&sketch, &backwards)
            .expect("every one of these is a constraint of this sketch");

        assert_eq!(reported, ids, "the answer's own order reached the report");
    }

    #[test]
    fn a_report_names_a_subset_in_the_documents_order_and_not_a_position() {
        let (sketch, translation, ids) = four_constraints();
        let blamed = [stated_at(&translation, 3), stated_at(&translation, 1)];

        let reported = translation
            .named_in_document_order(&sketch, &blamed)
            .expect("both are constraints of this sketch");

        assert_eq!(reported, vec![ids[1], ids[3]]);
    }

    #[test]
    fn a_constraint_the_sketch_never_stated_is_refused_rather_than_dropped() {
        // The identifier of a fifth constraint, in a sketch that has four.
        // Silently reporting the three that did resolve would publish a
        // partial account of a sketch as the whole of it.
        let (sketch, translation, _) = four_constraints();
        let blamed = [
            stated_at(&translation, 0),
            solver::ConstraintId(4),
            stated_at(&translation, 2),
        ];

        let error = translation
            .named_in_document_order(&sketch, &blamed)
            .expect_err("an answer about a constraint this sketch does not hold is not an answer");

        assert_eq!(error.kind(), ErrorKind::Constraint);
        let message = error.to_string();
        assert!(
            message.contains("never stated"),
            "a refusal must say what it is: {message}"
        );
        for forbidden in ["ConstraintId", "PointId", "4"] {
            assert!(
                !message.contains(forbidden),
                "the refusal published {forbidden}, which lasts one solve: {message}"
            );
        }
    }

    #[test]
    fn a_constraint_blamed_twice_is_refused_rather_than_reported_twice() {
        let (sketch, translation, ids) = four_constraints();
        let once = stated_at(&translation, 2);
        let blamed = [once, stated_at(&translation, 0), once];

        let error = translation
            .named_in_document_order(&sketch, &blamed)
            .expect_err("one constraint cannot be two answers");

        assert_eq!(error.kind(), ErrorKind::Constraint);
        let message = error.to_string();
        assert!(
            message.contains(&ids[2].to_string()),
            "the refusal must name the constraint as the document does: {message}"
        );
        for forbidden in ["ConstraintId", "PointId"] {
            assert!(!message.contains(forbidden), "{message}");
        }
    }

    #[test]
    fn a_solve_that_blamed_nothing_reports_nothing() {
        let (sketch, translation, _) = four_constraints();
        assert!(
            translation
                .named_in_document_order(&sketch, &[])
                .expect("nothing to resolve")
                .is_empty()
        );
    }

    #[test]
    fn the_debug_of_a_report_publishes_no_transient_identifier() {
        let (sketch, translation, ids) = four_constraints();
        let report = SketchSolveReport {
            degrees_of_freedom: 3,
            redundant: translation
                .named_in_document_order(&sketch, &[stated_at(&translation, 1)])
                .expect("resolves"),
        };

        let printed = format!("{report:?}");
        for forbidden in ["PointId", "ConstraintId", "ordinal", "equation", "session"] {
            assert!(!printed.contains(forbidden), "{printed}");
        }
        assert!(printed.contains(&ids[1].to_string()), "{printed}");
    }

    #[test]
    fn under_constrained_is_read_from_the_freedom_and_not_stored_beside_it() {
        let none = SketchSolveReport {
            degrees_of_freedom: 0,
            redundant: Vec::new(),
        };
        let some = SketchSolveReport {
            degrees_of_freedom: 1,
            redundant: Vec::new(),
        };
        assert!(!none.is_under_constrained());
        assert!(some.is_under_constrained());
    }

    #[test]
    fn the_debug_of_a_translation_publishes_no_transient_identifier() {
        let (_, translation, _) = one_constraint();
        let printed = format!("{translation:?}");
        for forbidden in ["PointId", "ConstraintId"] {
            assert!(!printed.contains(forbidden), "{printed}");
        }
    }
}
