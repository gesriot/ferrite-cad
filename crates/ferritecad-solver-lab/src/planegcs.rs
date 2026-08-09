// SPDX-License-Identifier: MIT
//! FreeCAD's planegcs, as a second candidate.
//!
//! Behind a feature and behind a shared library. planegcs is
//! LGPL-2.0-or-later, so it is linked dynamically and can be replaced by
//! whoever runs this — the same terms Open CASCADE is on, and recorded in
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

use crate::{Constraint, Diagnosis, Outcome, Problem, Solver};

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
}

/// Which planegcs this was built against, for the record.
pub fn provenance() -> String {
    #[cfg(planegcs_linked)]
    {
        // SAFETY: the shim returns a pointer to a string literal with static
        // lifetime and no interior nul beyond its terminator.
        let raw = unsafe { std::ffi::CStr::from_ptr(fc_gcs_provenance()) };
        raw.to_string_lossy().into_owned()
    }
    #[cfg(not(planegcs_linked))]
    "planegcs was not linked into this build".to_owned()
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
        let elapsed = began.elapsed();

        // Measured the same way for every candidate: what the residuals
        // actually are, not what the solver says about itself.
        let (residuals, _) = problem.evaluate(&state);
        let worst = residuals
            .iter()
            .fold(0.0f64, |worst, value| worst.max(value.abs()));

        Outcome {
            // Judged on the residuals, not on which of planegcs's two
            // success codes came back: a system that cannot be satisfied
            // exactly reports Converged, and whether that is good enough is
            // the same question asked of every candidate.
            converged: status <= 2 && worst <= 1e-9,
            iterations: if iterations < 0 {
                0
            } else {
                iterations as usize
            },
            worst_residual: worst,
            elapsed,
            solution: state,
        }
    }
}
