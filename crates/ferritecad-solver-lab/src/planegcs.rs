// SPDX-License-Identifier: MIT
//! FreeCAD's planegcs, as a second candidate.
//!
//! Behind a feature and behind a shared library. planegcs is
//! LGPL-2.0-or-later, so it is linked dynamically and can be replaced by
//! whoever runs this. Its terms are recorded separately from Open CASCADE in
//! THIRD_PARTY_LICENSES.md. Nothing of its API appears in Rust: the shim
//! beside it is FerriteCAD's own MIT code and the boundary is a flat C ABI.
//!
//! The workspace denies `unsafe_code`, so the exception is declared once, in
//! writing, and is greppable.
#![allow(
    unsafe_code,
    reason = "the FFI boundary to planegcs; confined to this module by design"
)]

#[cfg(planegcs_linked)]
use std::os::raw::c_char;
use std::time::Instant;

use crate::{COMPARISON_RESIDUAL_LIMIT, Constraint, Diagnosis, Outcome, Problem, Solver};

const STATUS_SUCCESS: i32 = 0;
const STATUS_CONVERGED: i32 = 2;

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
struct RawConstraint {
    kind: i32,
    points: [i32; 4],
    value: f64,
    value2: f64,
}

/// Whether this build actually links planegcs.
///
/// The feature can be on while the library is not there — `--all-features` on
/// a machine that has never built it — so the bench asks rather than assumes,
/// exactly as it does about Open CASCADE.
pub fn is_available() -> bool {
    cfg!(planegcs_linked)
}

#[cfg(planegcs_linked)]
unsafe extern "C" {
    fn fc_gcs_solve(
        state: *mut f64,
        point_count: usize,
        constraints: *const RawConstraint,
        constraint_count: usize,
        out_dofs: *mut i32,
        out_has_conflicting: *mut i32,
        out_has_redundant: *mut i32,
        out_iterations: *mut i32,
    ) -> i32;

    fn fc_gcs_provenance() -> *const c_char;

    fn fc_gcs_native_solves() -> u64;
    fn fc_gcs_native_sessions() -> u64;
}

/// What the loaded shared library says it is.
///
/// The answer travels the same dynamic link as everything else: the string
/// lives in the library, not in the shim, so a library swapped for another
/// after the tests were written says so instead of being described by them.
pub fn provenance() -> String {
    #[cfg(planegcs_linked)]
    {
        // SAFETY: the shim forwards a pointer to a string literal with static
        // lifetime and no interior nul beyond its terminator.
        let raw = unsafe { std::ffi::CStr::from_ptr(fc_gcs_provenance()) };
        raw.to_string_lossy().into_owned()
    }
    #[cfg(not(planegcs_linked))]
    "planegcs was not linked into this build".to_owned()
}

/// What the pin says the library should answer.
///
/// Compiled in from `tools/planegcs/pin.env`, which is also what the build
/// script bakes into the library, so the two cannot drift.
pub fn expected_provenance() -> &'static str {
    env!("FCAD_PLANEGCS_EXPECTED_PROVENANCE")
}

/// How many solves have crossed into planegcs on this thread.
///
/// What makes "planegcs solved it" a checkable claim rather than a label. A
/// candidate quietly delegating to the reference implementation returns the
/// same shape of answer and leaves this untouched.
pub fn native_solves() -> u64 {
    #[cfg(planegcs_linked)]
    // SAFETY: reads a thread-local counter in the shim; no arguments, no
    // pointers, and the value is a plain integer.
    unsafe {
        fc_gcs_native_solves()
    }
    #[cfg(not(planegcs_linked))]
    0
}

/// How many native systems have been built on this thread.
///
/// A drag is meant to build one and nudge it. Rebuilding it every step
/// produces the same coordinates, so nothing about the geometry would say so.
pub fn native_sessions() -> u64 {
    #[cfg(planegcs_linked)]
    // SAFETY: as above.
    unsafe {
        fc_gcs_native_sessions()
    }
    #[cfg(not(planegcs_linked))]
    0
}

/// Whether this run refuses to treat an absent planegcs as a skipped candidate.
///
/// Read at run time rather than compiled in: the build script already fails
/// the build under the same variable, so by the time a test asks, the only
/// thing left to enforce is that no gate quietly returns early.
pub fn is_required() -> bool {
    std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref() == Ok("1")
}

/// Calls the shim, or reports that there is nothing to call.
#[cfg(planegcs_linked)]
fn run(state: &mut [f64], encoded: &[RawConstraint]) -> (i32, i32, i32, i32, i32) {
    let (mut dofs, mut conflicting, mut redundant, mut iterations) = (-1, 0, 0, -1);
    // SAFETY: `state` holds two values per point and the count says so; the
    // constraint slice outlives the call; every out-pointer is valid.
    let status = unsafe {
        fc_gcs_solve(
            state.as_mut_ptr(),
            state.len() / 2,
            encoded.as_ptr(),
            encoded.len(),
            &mut dofs,
            &mut conflicting,
            &mut redundant,
            &mut iterations,
        )
    };
    (status, dofs, conflicting, redundant, iterations)
}

#[cfg(not(planegcs_linked))]
fn run(_state: &mut [f64], _encoded: &[RawConstraint]) -> (i32, i32, i32, i32, i32) {
    (-1, -1, 0, 0, -1)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Planegcs;

fn completed(status: i32) -> bool {
    matches!(status, STATUS_SUCCESS | STATUS_CONVERGED)
}

fn encode(constraints: &[Constraint]) -> Vec<RawConstraint> {
    let blank = RawConstraint {
        kind: 0,
        points: [-1; 4],
        value: 0.0,
        value2: 0.0,
    };
    constraints
        .iter()
        .map(|constraint| match *constraint {
            Constraint::Coincident { a, b } => RawConstraint {
                kind: COINCIDENT,
                points: [a.0 as i32, b.0 as i32, -1, -1],
                ..blank
            },
            Constraint::Fixed { point, x, y } => RawConstraint {
                kind: FIXED,
                points: [point.0 as i32, -1, -1, -1],
                value: x,
                value2: y,
            },
            Constraint::Distance { a, b, distance } => RawConstraint {
                kind: DISTANCE,
                points: [a.0 as i32, b.0 as i32, -1, -1],
                value: distance,
                ..blank
            },
            Constraint::Horizontal { a, b } => RawConstraint {
                kind: HORIZONTAL,
                points: [a.0 as i32, b.0 as i32, -1, -1],
                ..blank
            },
            Constraint::Vertical { a, b } => RawConstraint {
                kind: VERTICAL,
                points: [a.0 as i32, b.0 as i32, -1, -1],
                ..blank
            },
            Constraint::EqualLength { a, b } => RawConstraint {
                kind: EQUAL_LENGTH,
                points: [a.0.0 as i32, a.1.0 as i32, b.0.0 as i32, b.1.0 as i32],
                ..blank
            },
            Constraint::Perpendicular { a, b } => RawConstraint {
                kind: PERPENDICULAR,
                points: [a.0.0 as i32, a.1.0 as i32, b.0.0 as i32, b.1.0 as i32],
                ..blank
            },
            Constraint::Parallel { a, b } => RawConstraint {
                kind: PARALLEL,
                points: [a.0.0 as i32, a.1.0 as i32, b.0.0 as i32, b.1.0 as i32],
                ..blank
            },
        })
        .collect()
}

impl Planegcs {
    /// What planegcs makes of the system, asked before it is solved.
    ///
    /// Reported through the same solve call because planegcs diagnoses as part
    /// of setting up, and asking twice would mean building the system twice.
    pub fn diagnose(&self, problem: &Problem) -> (Diagnosis, bool, bool) {
        let mut state = problem.start.clone();
        let encoded = encode(&problem.constraints);
        let (_, dofs, conflicting, redundant, _) = run(&mut state, &encoded);

        let diagnosis = Diagnosis {
            unknowns: problem.unknowns(),
            equations: problem.equations(),
            rank: problem.unknowns().saturating_sub(dofs.max(0) as usize),
            degrees_of_freedom: dofs.max(0) as usize,
            redundant: usize::from(redundant != 0),
        };
        (diagnosis, conflicting != 0, redundant != 0)
    }
}

impl Solver for Planegcs {
    fn name(&self) -> &'static str {
        "planegcs-dogleg"
    }

    fn solve(&self, problem: &Problem, start: &[f64]) -> Outcome {
        let began = Instant::now();
        let mut state = start.to_vec();
        let encoded = encode(&problem.constraints);
        let (status, _, _, _, iterations) = run(&mut state, &encoded);

        // Measured the same way for every candidate: what the residuals
        // actually are, not what the solver says about itself.
        let (residuals, _) = problem.evaluate(&state);
        let worst = residuals
            .iter()
            .fold(0.0f64, |worst, value| worst.max(value.abs()));
        let elapsed = began.elapsed();

        Outcome {
            // Both conditions matter. A small residual at the starting state
            // must not turn an ABI error into success, and a native success
            // must still clear the same neutral threshold as every candidate.
            converged: completed(status) && worst <= COMPARISON_RESIDUAL_LIMIT,
            iterations: if iterations < 0 {
                None
            } else {
                Some(iterations as usize)
            },
            worst_residual: worst,
            elapsed,
            solution: state,
        }
    }
}

#[cfg(planegcs_linked)]
mod session {
    use std::ffi::c_void;
    use std::time::Instant;

    use super::{RawConstraint, STATUS_SUCCESS, completed, encode};
    use crate::{Blame, Constraint, Drag, DragTimings, Problem};

    unsafe extern "C" {
        fn fc_gcs_session_create(
            start: *const f64,
            point_count: usize,
            constraints: *const RawConstraint,
            constraint_count: usize,
        ) -> *mut c_void;
        fn fc_gcs_session_destroy(session: *mut c_void);
        fn fc_gcs_session_diagnose(
            session: *mut c_void,
            out_dofs: *mut i32,
            out_conflicting: *mut i32,
            out_redundant: *mut i32,
            out_blamed: *mut i32,
            capacity: usize,
            out_blamed_count: *mut usize,
        ) -> i32;
        fn fc_gcs_session_prepare(session: *mut c_void) -> i32;
        fn fc_gcs_session_move(session: *mut c_void, constraint: usize, x: f64, y: f64) -> i32;
        fn fc_gcs_session_solve(session: *mut c_void) -> i32;
        fn fc_gcs_session_state(session: *const c_void, out: *mut f64, count: usize) -> i32;
    }

    /// A planegcs system that outlives one solve.
    ///
    /// Owns a C++ allocation, so it is a type with a destructor rather than a
    /// raw pointer passed around.
    #[derive(Debug)]
    pub struct Session {
        raw: *mut c_void,
        points: usize,
        constraints: usize,
    }

    impl Drop for Session {
        fn drop(&mut self) {
            // SAFETY: `raw` came from fc_gcs_session_create and is destroyed
            // exactly once, here.
            unsafe { fc_gcs_session_destroy(self.raw) };
        }
    }

    impl Session {
        pub fn new(problem: &Problem) -> Option<Self> {
            let encoded = encode(&problem.constraints);
            // SAFETY: both slices live across the call and their lengths are
            // passed with them.
            let raw = unsafe {
                fc_gcs_session_create(
                    problem.start.as_ptr(),
                    problem.start.len() / 2,
                    encoded.as_ptr(),
                    encoded.len(),
                )
            };
            if raw.is_null() {
                return None;
            }
            Some(Self {
                raw,
                points: problem.start.len() / 2,
                constraints: problem.constraints.len(),
            })
        }

        /// Degrees of freedom, and which constraints planegcs blames.
        pub fn diagnose(&mut self) -> Option<(i32, Blame)> {
            let (mut dofs, mut conflicting, mut redundant) = (-1, 0, 0);
            let mut count = 0usize;

            // First ask how much room is needed. The session caches the native
            // diagnosis, so the second call retrieves rather than recomputes.
            let status = unsafe {
                fc_gcs_session_diagnose(
                    self.raw,
                    &mut dofs,
                    &mut conflicting,
                    &mut redundant,
                    std::ptr::null_mut(),
                    0,
                    &mut count,
                )
            };
            if status != STATUS_SUCCESS {
                return None;
            }

            let mut blamed = vec![0i32; count];
            let mut returned = 0usize;
            let status = unsafe {
                fc_gcs_session_diagnose(
                    self.raw,
                    &mut dofs,
                    &mut conflicting,
                    &mut redundant,
                    blamed.as_mut_ptr(),
                    blamed.len(),
                    &mut returned,
                )
            };
            if status != STATUS_SUCCESS || returned != blamed.len() {
                return None;
            }

            let mut constraints = Vec::with_capacity(blamed.len());
            for index in blamed {
                let index = usize::try_from(index).ok()?;
                if index >= self.constraints {
                    return None;
                }
                constraints.push(index);
            }
            constraints.sort_unstable();
            constraints.dedup();
            Some((dofs, Blame { constraints }))
        }

        pub fn prepare(&mut self) -> bool {
            // SAFETY: the session is live for the whole call.
            unsafe { fc_gcs_session_prepare(self.raw) == STATUS_SUCCESS }
        }

        pub fn solve(&mut self) -> bool {
            // SAFETY: the session is live for the whole call.
            completed(unsafe { fc_gcs_session_solve(self.raw) })
        }

        pub fn move_to(&mut self, constraint: usize, x: f64, y: f64) -> bool {
            // SAFETY: as above; the shim range-checks the index.
            unsafe { fc_gcs_session_move(self.raw, constraint, x, y) == STATUS_SUCCESS }
        }

        pub fn state(&self) -> Option<Vec<f64>> {
            let mut out = vec![0.0; self.points * 2];
            // SAFETY: the buffer is exactly the size the shim is told.
            let status = unsafe { fc_gcs_session_state(self.raw, out.as_mut_ptr(), out.len()) };
            (status == STATUS_SUCCESS).then_some(out)
        }
    }

    /// Asks planegcs which stored constraints caused the diagnosis.
    pub fn blame(problem: &Problem) -> Option<Blame> {
        let mut session = Session::new(problem)?;
        session.diagnose().map(|(_, blame)| blame)
    }

    /// Drags with planegcs, keeping one system for the whole gesture.
    pub fn drag(problem: &Problem, drag: &Drag) -> Option<DragTimings> {
        let began = Instant::now();
        let mut dragged = problem.clone();
        dragged.constraints.push(Constraint::Fixed {
            point: drag.point,
            x: problem.start[drag.point.x()],
            y: problem.start[drag.point.y()],
        });
        let pin = dragged.constraints.len() - 1;
        let mut session = Session::new(&dragged)?;
        let mut setup = began.elapsed();

        let began = Instant::now();
        let _ = session.diagnose()?;
        let diagnose = began.elapsed();

        let began = Instant::now();
        if !session.prepare() || !session.solve() {
            return None;
        }
        setup += began.elapsed();
        let mut steps = Vec::with_capacity(drag.targets.len());
        let mut worst_residual: f64 = 0.0;
        let mut worst_follow: f64 = 0.0;

        for (x, y) in &drag.targets {
            if !session.move_to(pin, *x, *y) {
                return None;
            }
            // The same move, in the problem the residuals are judged against.
            // Without this the sketch is measured against where the pointer
            // used to be, and a solve that followed perfectly looks like one
            // that came apart — which is exactly how this was first written.
            dragged.constraints[pin] = Constraint::Fixed {
                point: drag.point,
                x: *x,
                y: *y,
            };

            let began = Instant::now();
            if !session.solve() {
                return None;
            }
            let state = session.state()?;
            steps.push(began.elapsed());

            let (residuals, _) = dragged.evaluate(&state);
            worst_residual = worst_residual.max(
                residuals
                    .iter()
                    .fold(0.0f64, |worst, value| worst.max(value.abs())),
            );
            worst_follow = worst_follow
                .max((state[drag.point.x()] - x).abs())
                .max((state[drag.point.y()] - y).abs());
        }

        Some(DragTimings {
            candidate: "planegcs-dogleg",
            setup,
            diagnose,
            steps,
            all_steps_converged: true,
            worst_residual,
            worst_follow_error: worst_follow,
        })
    }
}

#[cfg(planegcs_linked)]
pub use session::drag as drag_with_planegcs;

#[cfg(planegcs_linked)]
pub use session::blame as blame_with_planegcs;

/// Which constraints planegcs blames, or nothing when it is not linked.
#[cfg(not(planegcs_linked))]
pub fn drag_with_planegcs(
    _problem: &crate::Problem,
    _drag: &crate::Drag,
) -> Option<crate::DragTimings> {
    None
}

#[cfg(not(planegcs_linked))]
pub fn blame_with_planegcs(_problem: &crate::Problem) -> Option<crate::Blame> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_documented_completion_statuses_are_success() {
        assert!(completed(STATUS_SUCCESS));
        assert!(completed(STATUS_CONVERGED));
        for status in [-4, -3, -2, -1, 1, 3] {
            assert!(!completed(status), "status {status} is not success");
        }
    }

    #[cfg(planegcs_linked)]
    #[test]
    fn an_invalid_point_index_is_refused_at_the_abi_boundary() {
        let mut state = vec![0.0, 0.0];
        let invalid = RawConstraint {
            kind: COINCIDENT,
            points: [0, 1, -1, -1],
            value: 0.0,
            value2: 0.0,
        };
        let (status, ..) = run(&mut state, &[invalid]);
        assert_eq!(status, -1);
    }

    #[cfg(planegcs_linked)]
    #[test]
    fn a_persistent_session_enforces_its_lifecycle_and_moves_only_a_fixed_target() {
        let problem = crate::problem(crate::Corpus::Rectangle, 0);
        let distance = problem
            .constraints
            .iter()
            .position(|constraint| matches!(constraint, Constraint::Distance { .. }))
            .expect("the rectangle has a dimension");
        let fixed = problem
            .constraints
            .iter()
            .position(|constraint| matches!(constraint, Constraint::Fixed { .. }))
            .expect("the rectangle has a fixed point");

        let mut session = session::Session::new(&problem).expect("valid session");
        assert!(!session.solve(), "solve before diagnosis/prepare must fail");
        assert!(session.diagnose().is_some());
        assert!(session.prepare());
        assert!(session.solve());
        assert!(
            !session.move_to(distance, 1.0, 2.0),
            "a distance value is not a two-coordinate drag target"
        );
        assert!(
            !session.move_to(fixed, f64::NAN, 2.0),
            "non-finite pointer positions must stop at the ABI"
        );
        assert!(session.move_to(fixed, 1.0, 2.0));
    }
}
