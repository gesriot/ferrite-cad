// SPDX-License-Identifier: MIT
//! FerriteCAD's sketch constraint solver.
//!
//! One crate owns the whole of it: the contract a caller states a sketch in,
//! the FFI to planegcs, the MIT bridge on the other side of that FFI, the
//! build-time detection that decides whether there is a library at all, and
//! the lifetime of the native session. `ferritecad-solver-lab` is a client of
//! this crate and holds none of those; nothing here points back at the lab.
//!
//! A document stores constraints in its own durable vocabulary. The evaluator
//! translates them into this crate's transient identifiers, solves them and
//! builds geometry from the answer without rewriting the document. No
//! interface creates or edits a constraint yet, and no published release is
//! offered.
//!
//! # Without planegcs
//!
//! The library is LGPL-2.0-or-later, is built separately and is off by
//! default, so an ordinary build of this workspace has no solver in it. That
//! build still compiles, and every entry point answers
//! [`Unavailable`] — a typed refusal, not a skipped test, not a panic, and
//! never a quiet substitution of some other arithmetic.

// In a build with no planegcs there is no native answer to interpret, so the
// half of this crate that maps one back into the caller's terms — the
// identifier table, the residual measurement, the constructor for a solved
// sketch — is compiled and never reached. It is not cfg'd away: it would then
// exist only in the configuration that cannot typecheck it on an ordinary
// machine, and its unit tests would go with it. The linked build carries no
// such allowance, and that is the build the pin workflow runs on every change
// to any of these files.
#![cfg_attr(
    not(planegcs_linked),
    allow(
        dead_code,
        reason = "without a library there is no native result to map back"
    )
)]

mod contract;
mod error;
mod planegcs;
mod prepared;
mod residual;

pub use contract::{
    Constraint, ConstraintId, Diagnosis, Outcome, PointId, Position, Sketch, Solution,
};
pub use error::{NativeFailure, NotFinite, SolverError, Unavailable};
pub use planegcs::Drag;

/// How closely a constraint must hold for a sketch to count as solved.
///
/// A numeric limit rather than one physical length: residuals carry the units
/// of the constraint that produced them, and a sketch mixes lengths with the
/// squared quantities that equal-length and perpendicularity produce.
pub const RESIDUAL_LIMIT: f64 = 1e-6;

/// Whether this build can solve a sketch at all.
///
/// Asked rather than assumed. The cargo feature can be on while no library was
/// found, which is what an ordinary `--all-features` build on a machine that
/// has never built planegcs looks like.
pub fn availability() -> Result<(), Unavailable> {
    planegcs::availability()
}

pub fn is_available() -> bool {
    availability().is_ok()
}

/// What the loaded shared library says it is.
///
/// Answered by the library itself rather than by anything compiled into this
/// crate, so that a library replaced after these words were written says so
/// instead of being described by them.
pub fn provenance() -> Result<String, Unavailable> {
    planegcs::provenance()
}

/// Whether this run refuses to treat an absent solver as an excuse.
///
/// The build script fails the build under the same variable. By the time
/// anything asks this, what is left to enforce is that no gate above quietly
/// returns early.
pub fn is_required() -> bool {
    std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref() == Ok("1")
}

/// How many solves have crossed into planegcs on this thread.
///
/// Instrumentation, and the only way to check rather than believe that an
/// answer attributed to planegcs came from planegcs. A path that quietly
/// solved the sketch some other way returns the same shape of answer and
/// leaves this untouched.
pub fn native_solves() -> u64 {
    planegcs::native_solves()
}

/// What the pin says the loaded library should answer.
///
/// Compiled in from `tools/planegcs/pin.env`, which is also what the build
/// script bakes into the library itself, so the two cannot drift.
#[cfg(feature = "planegcs")]
pub fn expected_provenance() -> &'static str {
    env!("FCAD_PLANEGCS_EXPECTED_PROVENANCE")
}

/// Native systems alive on this thread: created, less destroyed.
///
/// Instrumentation. A session that leaked and one that was released twice are
/// both invisible in a result — the coordinates are identical — so this is how
/// a gate states the lifetime was right rather than assuming it.
pub fn native_live_sessions() -> u64 {
    planegcs::native_live_sessions()
}

/// How many native systems have been built on this thread.
///
/// A gesture is meant to build one and nudge it. Rebuilding it every sample
/// produces the same coordinates, so nothing about the geometry would say so.
pub fn native_sessions() -> u64 {
    planegcs::native_sessions()
}

/// What the solver makes of this sketch's structure, without solving it.
pub fn diagnose(sketch: &Sketch) -> Result<Diagnosis, SolverError> {
    planegcs::diagnose(sketch)
}

/// Solves, starting from where the sketch says its points are.
pub fn solve(sketch: &Sketch) -> Result<Outcome, SolverError> {
    planegcs::solve_from(sketch, None)
}

/// Solves, starting from a state the caller supplies.
///
/// The state must be exactly one position per point of the sketch, naming
/// each of them once.
pub fn solve_from(sketch: &Sketch, start: &[Position]) -> Result<Outcome, SolverError> {
    planegcs::solve_from(sketch, Some(start))
}
