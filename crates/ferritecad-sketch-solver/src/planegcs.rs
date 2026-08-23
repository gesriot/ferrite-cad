// SPDX-License-Identifier: MIT
//! The solver behind the contract: planegcs, through a flat C boundary.
//!
//! planegcs is LGPL-2.0-or-later, so it is linked dynamically and can be
//! replaced by whoever receives a build. Its terms are recorded separately
//! from Open CASCADE in THIRD_PARTY_LICENSES.md. Nothing of its API appears in
//! Rust: the shim beside it, in `planegcs-bridge/`, is FerriteCAD's own MIT
//! code and holds no planegcs types.
//!
//! # What this module owes the contract
//!
//! - A native system is owned by a value with a destructor, so it is released
//!   exactly once, and a gesture holds one rather than building fifty.
//! - A refusal publishes no positions at all. planegcs writes its answer back
//!   whenever it converged *or* merely minimised, and the second is what an
//!   impossible sketch produces; a state read out of that and handed on would
//!   be a sketch half moved towards something it cannot be.
//! - Native tags leave as the caller's own identifiers, through `Prepared`,
//!   never as the positions they happen to be stored at.
//!
//! # Send and Sync
//!
//! Neither, and not by accident. The session holds a pointer to a C++
//! allocation whose parameter block the native system keeps its own pointers
//! into, and the shim's crossing counters are thread-local — a session made on
//! one thread and solved on another would have its work counted on neither
//! consistently, and the gates that check a gesture built one system would
//! stop meaning anything.
//!
//! It is withheld by a `PhantomData<*const ()>` on `Drag` rather than left to
//! the pointer field, because in a build with no library the session is
//! uninhabited and an uninhabited type is `Send` and `Sync`. Left to the
//! fields, a gesture would be thread-safe exactly when there is no solver, and
//! code that compiled without planegcs would stop compiling with it.
//! `neither_send_nor_sync` in the tests holds both configurations to the same
//! answer and keeps a later `unsafe impl` from being added quietly.
//!
//! The workspace denies `unsafe_code`, so the exception is declared once, in
//! writing, and is greppable.
#![allow(
    unsafe_code,
    reason = "the FFI boundary to planegcs; confined to this module by design"
)]

use std::marker::PhantomData;

use crate::prepared::Prepared;
use crate::{
    ConstraintId, Diagnosis, NotFinite, Outcome, PointId, Position, Sketch, SolverError,
    Unavailable,
};

/// Native tags, as the caller's identifiers.
///
/// Sorted and deduplicated by identifier rather than left in the order the
/// native diagnosis produced them, so three platforms diagnosing one sketch
/// report one list. Anything the caller did not issue — a gesture's own pin —
/// is dropped rather than given a number it never asked for.
fn caller_ids(prepared: &Prepared, stored: &[usize]) -> Result<Vec<ConstraintId>, SolverError> {
    let mut ids = Vec::with_capacity(stored.len());
    for &index in stored {
        if let Some(id) = prepared.caller_id(index)? {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// What a diagnosed system is, before anyone decides what to say about it.
struct Diagnosed {
    degrees_of_freedom: usize,
    conflicting: Vec<ConstraintId>,
    redundant: Vec<ConstraintId>,
}

#[cfg(planegcs_linked)]
mod linked {
    //! Everything that touches the C boundary.

    use std::ffi::{CStr, c_void};
    use std::os::raw::c_char;

    use super::Prepared;
    use crate::{Constraint, NativeFailure, PointId, SolverError};

    /// The shim's own return codes. Not planegcs's: those are an
    /// implementation detail of a library that may renumber them, and none of
    /// them reaches a caller of this crate.
    pub(super) const STATUS_SUCCESS: i32 = 0;
    pub(super) const STATUS_NOT_CONVERGED: i32 = 1;
    pub(super) const STATUS_CONVERGED: i32 = 2;

    /// Constraint kinds, stable by number because they cross an ABI.
    const COINCIDENT: i32 = 0;
    const FIXED: i32 = 1;
    const DISTANCE: i32 = 2;
    const HORIZONTAL: i32 = 3;
    const VERTICAL: i32 = 4;
    const EQUAL_LENGTH: i32 = 5;
    const PERPENDICULAR: i32 = 6;
    const PARALLEL: i32 = 7;

    #[repr(C)]
    #[derive(Debug, Clone, Copy)]
    pub(super) struct RawConstraint {
        kind: i32,
        points: [i32; 4],
        value: f64,
        value2: f64,
    }

    /// Lays the caller's constraints out the way the shim reads them.
    ///
    /// Point references become positions in the native parameter block. The
    /// translation is one-way and lives here; nothing that comes back out is
    /// read through it.
    pub(super) fn encode(prepared: &Prepared) -> Vec<RawConstraint> {
        let blank = RawConstraint {
            kind: 0,
            points: [-1; 4],
            value: 0.0,
            value2: 0.0,
        };
        // The shim indexes points; a slot is two doubles, so it halves back.
        let at = |point: PointId| (prepared.slot_of(point) / 2) as i32;
        prepared
            .constraints
            .iter()
            .map(|constraint| match *constraint {
                Constraint::Coincident { a, b } => RawConstraint {
                    kind: COINCIDENT,
                    points: [at(a), at(b), -1, -1],
                    ..blank
                },
                Constraint::Fixed { point, x, y } => RawConstraint {
                    kind: FIXED,
                    points: [at(point), -1, -1, -1],
                    value: x,
                    value2: y,
                },
                Constraint::Distance { a, b, distance } => RawConstraint {
                    kind: DISTANCE,
                    points: [at(a), at(b), -1, -1],
                    value: distance,
                    ..blank
                },
                Constraint::Horizontal { a, b } => RawConstraint {
                    kind: HORIZONTAL,
                    points: [at(a), at(b), -1, -1],
                    ..blank
                },
                Constraint::Vertical { a, b } => RawConstraint {
                    kind: VERTICAL,
                    points: [at(a), at(b), -1, -1],
                    ..blank
                },
                Constraint::EqualLength { a, b } => RawConstraint {
                    kind: EQUAL_LENGTH,
                    points: [at(a.0), at(a.1), at(b.0), at(b.1)],
                    ..blank
                },
                Constraint::Perpendicular { a, b } => RawConstraint {
                    kind: PERPENDICULAR,
                    points: [at(a.0), at(a.1), at(b.0), at(b.1)],
                    ..blank
                },
                Constraint::Parallel { a, b } => RawConstraint {
                    kind: PARALLEL,
                    points: [at(a.0), at(a.1), at(b.0), at(b.1)],
                    ..blank
                },
            })
            .collect()
    }

    unsafe extern "C" {
        pub(super) fn fc_gcs_session_create(
            start: *const f64,
            point_count: usize,
            constraints: *const RawConstraint,
            constraint_count: usize,
        ) -> *mut c_void;
        pub(super) fn fc_gcs_session_destroy(session: *mut c_void);
        pub(super) fn fc_gcs_session_diagnose(
            session: *mut c_void,
            out_dofs: *mut i32,
            out_conflicting: *mut i32,
            out_redundant: *mut i32,
            out_blamed: *mut i32,
            capacity: usize,
            out_blamed_count: *mut usize,
        ) -> i32;
        pub(super) fn fc_gcs_session_prepare(session: *mut c_void) -> i32;
        pub(super) fn fc_gcs_session_move(
            session: *mut c_void,
            constraint: usize,
            x: f64,
            y: f64,
        ) -> i32;
        pub(super) fn fc_gcs_session_solve(session: *mut c_void) -> i32;
        pub(super) fn fc_gcs_session_state(
            session: *const c_void,
            out: *mut f64,
            count: usize,
        ) -> i32;

        fn fc_gcs_provenance() -> *const c_char;
        fn fc_gcs_native_solves() -> u64;
        fn fc_gcs_native_sessions() -> u64;
        fn fc_gcs_native_live_sessions() -> u64;
    }

    pub(super) fn provenance() -> String {
        // SAFETY: the library forwards a pointer to a string literal with
        // static lifetime and no interior nul beyond its terminator.
        let raw = unsafe { CStr::from_ptr(fc_gcs_provenance()) };
        raw.to_string_lossy().into_owned()
    }

    // SAFETY for all three: each reads a thread-local counter in the shim. No
    // arguments, no pointers, and the value is a plain integer.
    pub(super) fn solves() -> u64 {
        unsafe { fc_gcs_native_solves() }
    }
    pub(super) fn sessions() -> u64 {
        unsafe { fc_gcs_native_sessions() }
    }
    pub(super) fn live_sessions() -> u64 {
        unsafe { fc_gcs_native_live_sessions() }
    }

    pub(super) fn ok(status: i32, failure: NativeFailure) -> Result<(), SolverError> {
        if status == STATUS_SUCCESS {
            Ok(())
        } else {
            Err(failure.into())
        }
    }

    /// Whether the native solver applied a solution.
    ///
    /// Non-convergence is a geometric answer. A negative ABI status or an
    /// unknown value is a broken native call and must remain an error rather
    /// than being reported as a sketch that merely did not converge.
    pub(super) fn solve_applied(status: i32) -> Result<bool, SolverError> {
        match status {
            STATUS_SUCCESS | STATUS_CONVERGED => Ok(true),
            STATUS_NOT_CONVERGED => Ok(false),
            _ => Err(NativeFailure::Refused.into()),
        }
    }
}

/// A native system, owned.
///
/// The pointer is private and never leaves this type, which is what makes the
/// destructor the only route to `fc_gcs_session_destroy`, and therefore makes
/// "released exactly once" a property of the type rather than of every place
/// that uses one.
#[cfg(planegcs_linked)]
struct Session {
    raw: *mut std::ffi::c_void,
    prepared: Prepared,
}

#[cfg(planegcs_linked)]
impl std::fmt::Debug for Session {
    /// Says how big the system is, and not where it is.
    ///
    /// A derived `Debug` would print the native pointer — the address of a C++
    /// allocation, nothing a caller can act on, and an invitation for somebody
    /// to log it and compare two runs by it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("prepared", &self.prepared)
            .finish_non_exhaustive()
    }
}

#[cfg(planegcs_linked)]
impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: `raw` came from a non-null fc_gcs_session_create, was never
        // handed to anybody else, and is destroyed exactly once, here.
        unsafe { linked::fc_gcs_session_destroy(self.raw) };
    }
}

#[cfg(planegcs_linked)]
impl Session {
    fn new(prepared: Prepared) -> Result<Self, SolverError> {
        let encoded = linked::encode(&prepared);
        // SAFETY: both slices live across the call and their lengths travel
        // with them; the shim range-checks every point reference again.
        let raw = unsafe {
            linked::fc_gcs_session_create(
                prepared.state.as_ptr(),
                prepared.points(),
                encoded.as_ptr(),
                encoded.len(),
            )
        };
        if raw.is_null() {
            return Err(crate::NativeFailure::CouldNotBuild.into());
        }
        Ok(Self { raw, prepared })
    }

    fn diagnose(&mut self) -> Result<Diagnosed, SolverError> {
        let (mut dofs, mut conflicting, mut redundant) = (-1, 0, 0);
        let mut total = 0usize;
        // Ask how much room is needed first. The session caches its native
        // diagnosis, so the second call retrieves rather than recomputes.
        // SAFETY: the session is live for the whole call, and a null blame
        // buffer is paired with a zero capacity, which the shim requires.
        let status = unsafe {
            linked::fc_gcs_session_diagnose(
                self.raw,
                &mut dofs,
                &mut conflicting,
                &mut redundant,
                std::ptr::null_mut(),
                0,
                &mut total,
            )
        };
        linked::ok(status, crate::NativeFailure::CouldNotDiagnose)?;

        let mut blamed = vec![0i32; total];
        let mut returned = 0usize;
        // SAFETY: the buffer is exactly the size the shim is told.
        let status = unsafe {
            linked::fc_gcs_session_diagnose(
                self.raw,
                &mut dofs,
                &mut conflicting,
                &mut redundant,
                blamed.as_mut_ptr(),
                blamed.len(),
                &mut returned,
            )
        };
        linked::ok(status, crate::NativeFailure::CouldNotDiagnose)?;
        if returned != blamed.len() {
            return Err(crate::NativeFailure::CouldNotDiagnose.into());
        }

        // The shim writes the conflicting group first and the redundant group
        // after it, and reports how many of each. Splitting on those counts is
        // what keeps "these cannot all hold" apart from "this repeats what
        // another already said": two different things to be told about a
        // drawing, and two different things to do about it.
        let conflicting = usize::try_from(conflicting.max(0)).unwrap_or(0);
        let redundant = usize::try_from(redundant.max(0)).unwrap_or(0);
        if conflicting + redundant != blamed.len() {
            return Err(crate::NativeFailure::CouldNotDiagnose.into());
        }
        let stored = |group: &[i32]| -> Result<Vec<usize>, SolverError> {
            group
                .iter()
                .map(|&index| {
                    usize::try_from(index)
                        .map_err(|_| SolverError::from(crate::NativeFailure::CouldNotDiagnose))
                })
                .collect()
        };
        Ok(Diagnosed {
            degrees_of_freedom: usize::try_from(dofs.max(0)).unwrap_or(0),
            conflicting: caller_ids(&self.prepared, &stored(&blamed[..conflicting])?)?,
            redundant: caller_ids(&self.prepared, &stored(&blamed[conflicting..])?)?,
        })
    }

    fn prepare(&mut self) -> Result<(), SolverError> {
        // SAFETY: the session is live for the whole call.
        linked::ok(
            unsafe { linked::fc_gcs_session_prepare(self.raw) },
            crate::NativeFailure::Refused,
        )
    }

    /// Solves, and answers whether an acceptable native solution was applied.
    fn solve(&mut self) -> Result<bool, SolverError> {
        // SAFETY: the session is live for the whole call.
        let status = unsafe { linked::fc_gcs_session_solve(self.raw) };
        linked::solve_applied(status)
    }

    /// Moves a stored pin, in the native system and in the copy the answer
    /// will be judged against.
    ///
    /// Both, together, because measuring a solve against where the pointer
    /// used to be makes a sample that followed perfectly look like one that
    /// came apart.
    fn move_pin(&mut self, stored: usize, x: f64, y: f64) -> Result<(), SolverError> {
        // SAFETY: the session is live; the shim range-checks the index and
        // refuses a constraint that is not a pin.
        linked::ok(
            unsafe { linked::fc_gcs_session_move(self.raw, stored, x, y) },
            crate::NativeFailure::Refused,
        )?;
        self.prepared.move_pin(stored, x, y);
        Ok(())
    }

    fn state(&self) -> Result<Vec<f64>, SolverError> {
        let mut out = vec![0.0; self.prepared.points() * 2];
        // SAFETY: the buffer is exactly the size the shim is told.
        let status = unsafe { linked::fc_gcs_session_state(self.raw, out.as_mut_ptr(), out.len()) };
        linked::ok(status, crate::NativeFailure::Refused)?;
        Ok(out)
    }

    /// Solves an already diagnosed and partitioned system, and judges it.
    ///
    /// No positions leave this function unless every constraint the caller
    /// wrote is satisfied.
    fn solve_and_judge(&mut self, diagnosed: &Diagnosed) -> Result<Outcome, SolverError> {
        if !self.solve()? {
            // Nothing is read out of the session. planegcs leaves the state
            // alone when it fails outright, but the point is that this path
            // cannot publish one either way.
            return Ok(Outcome::DidNotConverge {
                worst_residual: None,
            });
        }
        let state = self.state()?;
        // Measured against what the caller asked for, not against what the
        // solver says about itself. planegcs reports "minimised the error
        // function" for a sketch that has no solution, and that status alone
        // would call the 10-10-40 triangle solved.
        let worst = crate::residual::worst(&self.prepared, &state);
        // Finiteness is tested by name rather than left to a negated
        // comparison: a state carrying NaN produces a NaN residual, which
        // compares false against every limit, and the reader should see that
        // case decided rather than infer it.
        if !worst.is_finite() || worst > crate::RESIDUAL_LIMIT {
            return Ok(Outcome::DidNotConverge {
                worst_residual: Some(worst),
            });
        }
        Ok(Outcome::Solved(crate::Solution::new(
            self.prepared.positions(&state),
            diagnosed.degrees_of_freedom,
            diagnosed.redundant.clone(),
            worst,
        )))
    }
}

/// The same type, in a build with no library behind it.
///
/// Uninhabited rather than absent, so everything above — `Drag`, its methods,
/// the public signatures — is written once and means the same thing in both
/// builds. There is no way to make one, which is exactly the fact a build
/// without planegcs has to represent.
#[cfg(not(planegcs_linked))]
#[derive(Debug)]
struct Session(std::convert::Infallible);

#[cfg(not(planegcs_linked))]
impl Session {
    fn new(_prepared: Prepared) -> Result<Self, SolverError> {
        Err(Unavailable::NotLinked.into())
    }
    fn diagnose(&mut self) -> Result<Diagnosed, SolverError> {
        match self.0 {}
    }
    fn prepare(&mut self) -> Result<(), SolverError> {
        match self.0 {}
    }
    fn move_pin(&mut self, _stored: usize, _x: f64, _y: f64) -> Result<(), SolverError> {
        match self.0 {}
    }
    fn solve_and_judge(&mut self, _diagnosed: &Diagnosed) -> Result<Outcome, SolverError> {
        match self.0 {}
    }
}

pub(crate) fn availability() -> Result<(), Unavailable> {
    #[cfg(planegcs_linked)]
    {
        Ok(())
    }
    #[cfg(not(planegcs_linked))]
    {
        Err(Unavailable::NotLinked)
    }
}

pub(crate) fn provenance() -> Result<String, Unavailable> {
    #[cfg(planegcs_linked)]
    {
        Ok(linked::provenance())
    }
    #[cfg(not(planegcs_linked))]
    {
        Err(Unavailable::NotLinked)
    }
}

pub(crate) fn native_solves() -> u64 {
    #[cfg(planegcs_linked)]
    {
        linked::solves()
    }
    #[cfg(not(planegcs_linked))]
    {
        0
    }
}

pub(crate) fn native_sessions() -> u64 {
    #[cfg(planegcs_linked)]
    {
        linked::sessions()
    }
    #[cfg(not(planegcs_linked))]
    {
        0
    }
}

pub(crate) fn native_live_sessions() -> u64 {
    #[cfg(planegcs_linked)]
    {
        linked::live_sessions()
    }
    #[cfg(not(planegcs_linked))]
    {
        0
    }
}

pub(crate) fn diagnose(sketch: &Sketch) -> Result<Diagnosis, SolverError> {
    // Checked before anything is asked of the library, in every build.
    let prepared = Prepared::new(sketch, None)?;
    let diagnosed = Session::new(prepared)?.diagnose()?;
    Ok(Diagnosis::new(
        diagnosed.degrees_of_freedom,
        diagnosed.conflicting,
        diagnosed.redundant,
    ))
}

pub(crate) fn solve_from(
    sketch: &Sketch,
    start: Option<&[Position]>,
) -> Result<Outcome, SolverError> {
    let prepared = Prepared::new(sketch, start)?;
    let mut session = Session::new(prepared)?;
    let diagnosed = session.diagnose()?;
    // A system whose constraints cannot all hold is refused before it is
    // solved, and refused without positions.
    if !diagnosed.conflicting.is_empty() {
        return Ok(Outcome::Conflicting {
            constraints: diagnosed.conflicting,
            redundant: diagnosed.redundant,
        });
    }
    session.prepare()?;
    session.solve_and_judge(&diagnosed)
}

/// One gesture, holding one native system.
///
/// The reason this is a type and not a loop over [`crate::solve`]: a drag is a
/// sketch set up once and then nudged. Rebuilding the system for every sample
/// returns the same coordinates at a different price, so nothing about the
/// geometry would ever say it was happening.
pub struct Drag {
    session: Session,
    /// Where this gesture's own pin is stored. Not the caller's constraint,
    /// and deliberately without an identifier.
    pin: usize,
    point: PointId,
    diagnosed: Diagnosed,
    /// Withholds `Send` and `Sync`, in every configuration.
    ///
    /// The linked `Session` holds a raw pointer and would withhold them by
    /// itself; the one in a build without a library is uninhabited, and an
    /// uninhabited type is `Send` and `Sync`. Left to the fields, this type
    /// would therefore be thread-safe exactly when there is no solver — so
    /// code that compiled on a machine without planegcs would stop compiling
    /// on one with it. Which traits a public type has must not depend on
    /// whether a build found a library.
    not_thread_safe: PhantomData<*const ()>,
}

/// Names the point being dragged and what the system was diagnosed as.
///
/// Not the pin's storage position: that is where this crate put a constraint
/// inside the native system, it changes with nothing the caller did, and a
/// caller who read it out of a log would be reading an internal handle.
impl std::fmt::Debug for Drag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drag")
            .field("point", &self.point)
            .field("diagnosed", &self.diagnosed)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Diagnosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Diagnosed")
            .field("degrees_of_freedom", &self.degrees_of_freedom)
            .field("conflicting", &self.conflicting)
            .field("redundant", &self.redundant)
            .finish()
    }
}

impl Drag {
    /// Begins a gesture that moves `point`, holding it where it is now.
    ///
    /// The system is built, diagnosed and partitioned here, once. Every sample
    /// afterwards is a solve of this same system.
    pub fn begin(sketch: &Sketch, point: PointId) -> Result<Self, SolverError> {
        let mut prepared = Prepared::new(sketch, None)?;
        let pin = prepared.pin(point)?;
        let mut session = Session::new(prepared)?;
        let diagnosed = session.diagnose()?;
        session.prepare()?;
        Ok(Self {
            session,
            pin,
            point,
            diagnosed,
            not_thread_safe: PhantomData,
        })
    }

    /// Which point this gesture is moving.
    pub fn point(&self) -> PointId {
        self.point
    }

    /// Puts the dragged point at `(x, y)` and re-solves.
    ///
    /// The same native system as every other sample of this gesture: what
    /// changes is where its pin points.
    pub fn move_to(&mut self, x: f64, y: f64) -> Result<Outcome, SolverError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(NotFinite::PointCoordinate(self.point).into());
        }
        if !self.diagnosed.conflicting.is_empty() {
            return Ok(Outcome::Conflicting {
                constraints: self.diagnosed.conflicting.clone(),
                redundant: self.diagnosed.redundant.clone(),
            });
        }
        self.session.move_pin(self.pin, x, y)?;
        // The diagnosis is the one taken when the gesture began: the
        // constraints have not changed, only where the pin points.
        self.session.solve_and_judge(&self.diagnosed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Neither trait, and a compile error if somebody adds one.
    ///
    /// The thread-local crossing counters and the native system's pointers
    /// into its own parameter block are why. This is checked rather than
    /// commented because an `unsafe impl Send` is one line and reads as a fix.
    #[test]
    fn neither_send_nor_sync() {
        // A negative bound cannot be written on stable, so this is the usual
        // ambiguity trick: two impls whose type parameter is left to
        // inference. While `Drag` is neither `Send` nor `Sync` only the
        // blanket impl applies and `_` resolves; the moment somebody adds
        // either, both apply, inference has two answers and this stops
        // compiling. Verified by adding `unsafe impl Send for Drag` and
        // watching it fail, which is the only way to know a check like this
        // is not vacuous.
        trait AmbiguousIfSend<A> {
            fn check() {}
        }
        struct Yes;
        impl<T: ?Sized> AmbiguousIfSend<()> for T {}
        impl<T: ?Sized + Send> AmbiguousIfSend<Yes> for T {}
        <Drag as AmbiguousIfSend<_>>::check();

        trait AmbiguousIfSync<A> {
            fn check() {}
        }
        impl<T: ?Sized> AmbiguousIfSync<()> for T {}
        impl<T: ?Sized + Sync> AmbiguousIfSync<Yes> for T {}
        <Drag as AmbiguousIfSync<_>>::check();
    }

    #[cfg(planegcs_linked)]
    #[test]
    fn a_native_failure_is_not_reported_as_geometric_non_convergence() {
        assert_eq!(linked::solve_applied(linked::STATUS_SUCCESS), Ok(true));
        assert_eq!(linked::solve_applied(linked::STATUS_CONVERGED), Ok(true));
        assert_eq!(
            linked::solve_applied(linked::STATUS_NOT_CONVERGED),
            Ok(false)
        );
        for status in [-4, -3, -2, -1, 3, i32::MAX] {
            assert_eq!(
                linked::solve_applied(status),
                Err(SolverError::Native(crate::NativeFailure::Refused)),
                "native status {status} was mistaken for a sketch that did not converge"
            );
        }
    }

    #[test]
    fn an_out_of_range_native_tag_is_not_silently_lost() {
        let mut sketch = Sketch::new();
        sketch.add_point(PointId(7), 0.0, 0.0).add_constraint(
            ConstraintId(91),
            crate::Constraint::Fixed {
                point: PointId(7),
                x: 0.0,
                y: 0.0,
            },
        );
        let mut prepared = Prepared::new(&sketch, None).expect("the sketch is valid");

        assert_eq!(caller_ids(&prepared, &[0]), Ok(vec![ConstraintId(91)]));
        let pin = prepared.pin(PointId(7)).expect("the point can be dragged");
        assert_eq!(caller_ids(&prepared, &[pin]), Ok(Vec::new()));
        assert_eq!(
            caller_ids(&prepared, &[pin + 1]),
            Err(SolverError::Native(crate::NativeFailure::CouldNotDiagnose))
        );
    }
}
