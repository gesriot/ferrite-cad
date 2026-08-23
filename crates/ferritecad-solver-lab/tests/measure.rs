// SPDX-License-Identifier: MIT
//! The comparison itself: every candidate, the same questions.
//!
//! Run with `--nocapture` to read the tables. The assertions are the floor a
//! candidate has to clear to be worth considering; the tables are what the
//! choice between candidates is made on.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::time::Duration;

use ferritecad_solver_lab::{
    COMPARISON_RESIDUAL_LIMIT, Constraint, Corpus, DoesNothing, Drag, IMPOSSIBLE,
    LevenbergMarquardt, Point, Problem, Solver, blame, drag_with_lm, problem,
};

/// How close counts as solved, for every candidate alike.
///
/// This is deliberately a neutral numeric threshold. The corpus mixes length
/// residuals with dot/cross products in mm², so calling the scalar itself a
/// nanometre would be dimensionally false. The achieved residual is reported
/// either way.
fn iterations(outcome: &ferritecad_solver_lab::Outcome) -> String {
    outcome
        .iterations
        .map_or_else(|| "n/a".to_owned(), |count| count.to_string())
}

/// Every candidate the bench knows about.
///
/// planegcs joins the list only when the bench was built with it, because it
/// is an LGPL shared library that has to be built first. Without it there is
/// one candidate, and the numbers say what a solver this project could own
/// performs like rather than which of two is better.
fn candidates() -> Vec<Box<dyn Solver>> {
    let mut all: Vec<Box<dyn Solver>> = vec![Box::new(LevenbergMarquardt::default())];
    all.extend(optional());
    all
}

/// Whether the optional candidate is here.
///
/// One gate for every site that asks, because the answer is not simply "is it
/// linked". A run told `FERRITECAD_REQUIRE_PLANEGCS=1` exists to prove that
/// planegcs works, and the way such a run fails is by quietly becoming a run
/// of the reference implementation that passes. So the absence is a failure
/// there and a printed skip everywhere else, and the printed skip is what the
/// workflow greps for.
#[cfg(feature = "planegcs")]
fn planegcs_ready() -> bool {
    if ferritecad_solver_lab::planegcs_available() {
        return true;
    }
    assert!(
        !ferritecad_solver_lab::planegcs_required(),
        "FERRITECAD_REQUIRE_PLANEGCS=1 was set, so no gate may skip: this build linked no \
         planegcs, and a comparison with one candidate is not a comparison"
    );
    eprintln!("skipped: this build did not link planegcs");
    false
}

/// The candidates that are only there in some builds.
///
/// Written as two whole functions rather than a conditional push, so the list
/// above reads and compiles the same way whether or not the feature is on.
#[cfg(feature = "planegcs")]
fn optional() -> Vec<Box<dyn Solver>> {
    if planegcs_ready() {
        vec![Box::new(ferritecad_solver_lab::Planegcs)]
    } else {
        Vec::new()
    }
}

#[cfg(not(feature = "planegcs"))]
fn optional() -> Vec<Box<dyn Solver>> {
    Vec::new()
}

/// The corpus, in the sizes the comparison is made over.
///
/// The largest sits just above 200 equations, which is the size the decision
/// was framed around.
fn suite() -> Vec<Problem> {
    let mut problems = vec![
        problem(Corpus::Rectangle, 0),
        problem(Corpus::Underconstrained, 0),
        problem(Corpus::Overconstrained, 0),
    ];
    for count in [2, 5, 10, 21] {
        problems.push(problem(Corpus::RectangleChain, count));
    }
    for sides in [4, 8, 16, 32] {
        problems.push(problem(Corpus::Polygon, sides));
    }
    for arms in [4, 16, 48, 100] {
        problems.push(problem(Corpus::Bracket, arms));
    }
    problems
}

fn micros(duration: Duration) -> u128 {
    duration.as_micros()
}

#[test]
fn every_candidate_solves_what_it_should_and_says_so() {
    let mut rows = Vec::new();

    for solver in candidates() {
        for problem in suite() {
            let diagnosis = problem.diagnose(1e-9);
            let outcome = solver.solve(&problem, &problem.start);

            rows.push(format!(
                "  {:<18} {:<18} eq {:>4}  unk {:>4}  dof {:>3}  redundant {:>2}  \
                 {:<9} iters {:>4}  worst {:>11.3e}  {:>8} us",
                solver.name(),
                problem.name,
                problem.equations(),
                problem.unknowns(),
                diagnosis.degrees_of_freedom,
                diagnosis.redundant,
                if outcome.converged {
                    "converged"
                } else {
                    "gave up"
                },
                iterations(&outcome),
                outcome.worst_residual,
                micros(outcome.elapsed)
            ));

            // A sketch with freedom left still has to satisfy what it was
            // told; it simply has more than one way to. A sketch with
            // redundant constraints must also solve — saying a thing twice
            // does not make it unsatisfiable.
            assert!(
                outcome.converged && outcome.worst_residual <= COMPARISON_RESIDUAL_LIMIT,
                "{} could not solve {}: converged {}, worst residual {:.3e}, iterations {}",
                solver.name(),
                problem.name,
                outcome.converged,
                outcome.worst_residual,
                iterations(&outcome)
            );
        }
    }

    eprintln!(
        "solver comparison ({} build; candidate-path timings include each \n\
         implementation's own setup and are smoke measurements, not a speed \n\
         ranking):\n{}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        rows.join("\n")
    );
}

#[test]
fn the_default_candidate_and_the_gate_use_one_limit() {
    assert_eq!(
        LevenbergMarquardt::default().tolerance,
        COMPARISON_RESIDUAL_LIMIT
    );
}

#[test]
fn the_bench_can_tell_a_solver_from_something_that_does_nothing() {
    // Without this, every measurement above could be measuring nothing.
    let problem = problem(Corpus::Rectangle, 0);
    let idle = DoesNothing.solve(&problem, &problem.start);
    assert!(!idle.converged);
    assert!(
        idle.worst_residual > 0.1,
        "the starting sketch must be genuinely unsolved, not nearly right"
    );

    let solved = LevenbergMarquardt::default().solve(&problem, &problem.start);
    assert!(solved.converged);
    assert!(solved.worst_residual <= COMPARISON_RESIDUAL_LIMIT);
    assert!(solved.worst_residual < idle.worst_residual / 1e6);
}

#[test]
fn a_solved_rectangle_is_actually_a_rectangle() {
    // Residuals near zero is what the solver claims. This checks the geometry
    // it produced, which is what the claim is supposed to mean.
    let problem = problem(Corpus::Rectangle, 0);
    let outcome = LevenbergMarquardt::default().solve(&problem, &problem.start);
    assert!(outcome.converged);
    assert!(outcome.worst_residual <= COMPARISON_RESIDUAL_LIMIT);

    let at = |point: usize| (outcome.solution[point * 2], outcome.solution[point * 2 + 1]);
    let (x0, y0) = at(0);
    let (x1, y1) = at(1);
    let (x2, y2) = at(2);
    let (x3, y3) = at(3);

    assert!(
        (x0 - 0.0).abs() < 1e-6 && (y0 - 0.0).abs() < 1e-6,
        "corner moved"
    );
    assert!((y1 - y0).abs() < 1e-6, "the bottom is not horizontal");
    assert!((x3 - x0).abs() < 1e-6, "the left side is not vertical");
    assert!(((x1 - x0).abs() - 60.0).abs() < 1e-6, "the width is wrong");
    assert!(((y3 - y0).abs() - 40.0).abs() < 1e-6, "the height is wrong");
    assert!(
        (x2 - x1).abs() < 1e-6 && (y2 - y3).abs() < 1e-6,
        "the corner is loose"
    );
}

#[test]
fn a_sketch_says_how_much_freedom_it_has_left() {
    // Fully constrained: nothing free, nothing repeated.
    let whole = problem(Corpus::Rectangle, 0).diagnose(1e-9);
    assert!(whole.is_fully_constrained(), "{whole:?}");
    assert_eq!(whole.degrees_of_freedom, 0);
    assert_eq!(whole.redundant, 0);

    // Unpinned: it can still slide in x and y.
    let loose = problem(Corpus::Underconstrained, 0).diagnose(1e-9);
    assert_eq!(loose.degrees_of_freedom, 2, "{loose:?}");
    assert_eq!(loose.redundant, 0);

    // Told the same thing twice: one equation says nothing new.
    let repeated = problem(Corpus::Overconstrained, 0).diagnose(1e-9);
    assert_eq!(repeated.redundant, 1, "{repeated:?}");
    assert_eq!(repeated.degrees_of_freedom, 0);
    assert!(!repeated.is_fully_constrained());
}

#[test]
fn dragging_a_corner_keeps_the_sketch_together() {
    // What a person actually does: hold one point and move it, one small step
    // at a time, re-solving after each. A solver that only works from a good
    // starting guess is no use for this.
    let base = problem(Corpus::Underconstrained, 0);
    let solver = LevenbergMarquardt::default();

    let mut state = solver.solve(&base, &base.start).solution;
    assert!(solver.solve(&base, &base.start).converged);

    let mut worst_seen: f64 = 0.0;
    let mut slowest = Duration::ZERO;
    let steps = 40;

    for step in 1..=steps {
        // The dragged corner is pinned a little further along each time.
        let target = (step as f64 * 1.5, step as f64 * 0.75);
        let mut dragged = base.clone();
        dragged.constraints.push(Constraint::Fixed {
            point: Point(0),
            x: target.0,
            y: target.1,
        });

        let outcome = solver.solve(&dragged, &state);
        assert!(
            outcome.worst_residual <= COMPARISON_RESIDUAL_LIMIT,
            "the sketch came apart at drag step {step}: worst residual {:.3e}",
            outcome.worst_residual
        );
        worst_seen = worst_seen.max(outcome.worst_residual);
        slowest = slowest.max(outcome.elapsed);
        state = outcome.solution;

        // The rectangle must still be 60 by 40 wherever it has been dragged to.
        let width = ((state[2] - state[0]).powi(2) + (state[3] - state[1]).powi(2)).sqrt();
        assert!(
            (width - 60.0).abs() < 1e-6,
            "drag step {step} stretched the rectangle to {width}"
        );
        assert!(
            (state[0] - target.0).abs() < 1e-6 && (state[1] - target.1).abs() < 1e-6,
            "drag step {step} did not follow the pointer"
        );
    }

    eprintln!(
        "drag over {steps} steps: worst residual {worst_seen:.3e}, slowest step {} us",
        micros(slowest)
    );
}

/// One summary per run, in facts that a floating-point unit cannot disagree
/// about.
///
/// Three platforms will not produce identical doubles and must not be asked
/// to: the same solve on the same source can land a few ulp apart and be
/// equally right. What has to match is the meaning – how many equations, how
/// much freedom is left, what was blamed, whether it converged, which library
/// answered – so that is what is printed, as integers and booleans, and the
/// pin workflow requires the three files to be the same file.
///
/// Timings are deliberately absent. They are reported by the tables above and
/// are not a gate anywhere: there is no recorded hardware profile, and a
/// runner that is busy is not a solver that is slow.
#[test]
fn a_semantic_summary_is_printed_for_cross_platform_comparison() {
    let mut lines = Vec::new();

    for solver in candidates() {
        for problem in suite() {
            let diagnosis = problem.diagnose(1e-9);
            let outcome = solver.solve(&problem, &problem.start);
            lines.push(format!(
                "semantic solve candidate={} problem={} eq={} unk={} dof={} redundant={} \
                 converged={}",
                solver.name(),
                problem.name,
                problem.equations(),
                problem.unknowns(),
                diagnosis.degrees_of_freedom,
                diagnosis.redundant,
                outcome.converged,
            ));
        }
    }

    for kind in IMPOSSIBLE {
        let sketch = problem(kind, 0);
        lines.push(format!(
            "semantic impossible problem={} lm_refused={}",
            sketch.name,
            !LevenbergMarquardt::default()
                .solve(&sketch, &sketch.start)
                .converged,
        ));
        #[cfg(feature = "planegcs")]
        if planegcs_ready() {
            lines.push(format!(
                "semantic impossible problem={} planegcs_refused={}",
                sketch.name,
                !ferritecad_solver_lab::Planegcs
                    .solve(&sketch, &sketch.start)
                    .converged,
            ));
        }
    }

    for kind in [Corpus::Rectangle, Corpus::Overconstrained] {
        let sketch = problem(kind, 0);
        lines.push(format!(
            "semantic blame problem={} lm={:?}",
            sketch.name,
            blame(&sketch).constraints
        ));
        #[cfg(feature = "planegcs")]
        if planegcs_ready() {
            let native = ferritecad_solver_lab::blame_with_planegcs(&sketch)
                .expect("a linked planegcs diagnoses");
            lines.push(format!(
                "semantic blame problem={} planegcs={:?}",
                sketch.name, native.constraints
            ));
        }
    }

    let sketch = problem(Corpus::Underconstrained, 0);
    let gesture = Drag::diagonal(Point(0), 50);
    let mine = drag_with_lm(&sketch, &gesture);
    lines.push(format!(
        "semantic drag candidate={} steps={} converged={} follows={}",
        mine.candidate,
        mine.steps.len(),
        mine.all_steps_converged,
        mine.worst_follow_error < 1e-6,
    ));
    #[cfg(feature = "planegcs")]
    if planegcs_ready() {
        let theirs = ferritecad_solver_lab::drag_with_planegcs(&sketch, &gesture)
            .expect("a linked planegcs drags");
        lines.push(format!(
            "semantic drag candidate={} steps={} converged={} follows={}",
            theirs.candidate,
            theirs.steps.len(),
            theirs.all_steps_converged,
            theirs.worst_follow_error < 1e-6,
        ));
    }

    #[cfg(feature = "planegcs")]
    if planegcs_ready() {
        lines.push(format!(
            "semantic provenance={}",
            ferritecad_solver_lab::planegcs_provenance()
        ));
    }

    // The leading newline is not decoration: the harness writes "test <name>
    // ... " without one, and the first summary line would otherwise arrive
    // with that prefix attached and be dropped by whatever reads them.
    eprintln!("\n{}", lines.join("\n"));
}

#[test]
fn the_largest_sketch_in_the_corpus_is_the_size_the_decision_was_framed_around() {
    let mut largest = 0;
    for problem in suite() {
        largest = largest.max(problem.equations());
    }
    assert!(
        largest >= 200,
        "the corpus tops out at {largest} equations, which does not answer the \
         question that was asked"
    );
}

#[cfg(feature = "planegcs")]
mod against_planegcs {
    use super::*;
    use ferritecad_solver_lab::{
        Planegcs, planegcs_expected_provenance, planegcs_native_solves, planegcs_provenance,
    };

    /// Leaves the caller when this build has the feature but not the library.
    macro_rules! planegcs_or_skip {
        () => {
            if !planegcs_ready() {
                return;
            }
        };
    }

    #[test]
    fn both_solvers_agree_about_the_geometry() {
        planegcs_or_skip!();
        // The point of a second candidate: not that each converges, but that
        // they land in the same place. A rectangle has one answer once a
        // corner is pinned, and two solvers that disagree about it are telling
        // us one of them is wrong.
        for problem in [
            problem(Corpus::Rectangle, 0),
            problem(Corpus::RectangleChain, 5),
            problem(Corpus::Polygon, 8),
            problem(Corpus::Bracket, 16),
        ] {
            let mine = LevenbergMarquardt::default().solve(&problem, &problem.start);
            let theirs = Planegcs.solve(&problem, &problem.start);

            // Both satisfy the constraints; where the sketch has freedom left
            // they may satisfy them differently, so what has to match is the
            // residual, not the coordinates.
            assert!(
                mine.converged && mine.worst_residual <= COMPARISON_RESIDUAL_LIMIT,
                "{}: mine left {:.3e}",
                problem.name,
                mine.worst_residual
            );
            assert!(
                theirs.converged && theirs.worst_residual <= COMPARISON_RESIDUAL_LIMIT,
                "{}: planegcs left {:.3e}",
                problem.name,
                theirs.worst_residual
            );
        }
    }

    #[test]
    fn both_solvers_agree_about_what_is_wrong_with_a_sketch() {
        planegcs_or_skip!();
        let loose = problem(Corpus::Underconstrained, 0);
        let (theirs, conflicting, redundant) = Planegcs.diagnose(&loose);
        let mine = loose.diagnose(1e-9);
        assert_eq!(
            mine.degrees_of_freedom, theirs.degrees_of_freedom,
            "the two disagree about how free this sketch is: {mine:?} vs {theirs:?}"
        );
        assert!(!conflicting && !redundant);

        let repeated = problem(Corpus::Overconstrained, 0);
        let (_, conflicting, redundant) = Planegcs.diagnose(&repeated);
        assert!(
            redundant || conflicting,
            "planegcs did not notice a constraint stated twice"
        );
        assert_eq!(repeated.diagnose(1e-9).redundant, 1);
    }

    #[test]
    fn the_library_the_lab_loaded_is_the_pinned_one() {
        planegcs_or_skip!();
        // Asked of the shared library, answered by a string compiled into it
        // from tools/planegcs/pin.env, and compared against the same file read
        // by this crate's build script. A library swapped for another after
        // these gates were written fails here rather than being described by
        // them, which is what makes the rest of them statements about planegcs
        // at all.
        let provenance = planegcs_provenance();
        assert_eq!(
            provenance,
            planegcs_expected_provenance(),
            "the library that was loaded is not the pinned one"
        );
        assert!(
            provenance.contains("planegcs from FreeCAD 1.0.1"),
            "the pin no longer names the release the decision was made on: {provenance}"
        );

        // And the candidate that carries planegcs's name really goes there.
        // Every other gate compares numbers, and a candidate that quietly
        // handed the problem to the reference implementation would clear all
        // of them while the table said planegcs: same residuals, same
        // diagnosis, same refusals, and a decision made on nothing.
        let sketch = problem(Corpus::Rectangle, 0);
        let before = planegcs_native_solves();
        let outcome = Planegcs.solve(&sketch, &sketch.start);
        assert!(outcome.converged);
        assert_eq!(
            planegcs_native_solves(),
            before + 1,
            "the planegcs candidate returned an answer without asking planegcs"
        );

        eprintln!("second candidate: {provenance}");
    }
}

#[test]
fn a_drag_is_measured_the_same_way_for_every_candidate() {
    // Fifty nudges of one corner, with setup and diagnosis paid once and
    // reported apart from the steps. What a person feels is the distribution
    // of the steps, not their mean: a drag that is usually fast and
    // occasionally not is a drag that feels broken.
    let sketch = problem(Corpus::Underconstrained, 0);
    let gesture = Drag::diagonal(Point(0), 50);

    let mut lines = Vec::new();
    let mine = drag_with_lm(&sketch, &gesture);
    assert!(mine.all_steps_converged, "LM reported a failed drag step");
    assert!(
        mine.worst_residual <= COMPARISON_RESIDUAL_LIMIT,
        "the sketch came apart while dragging: {:.3e}",
        mine.worst_residual
    );
    assert!(
        mine.worst_follow_error < 1e-6,
        "the dragged point did not follow the pointer: off by {:.3e}",
        mine.worst_follow_error
    );
    lines.push(mine.line());

    #[cfg(feature = "planegcs")]
    if planegcs_ready() {
        // One native system for the whole gesture, which is the thing being
        // measured. Rebuilding it per step returns the same coordinates at a
        // different price, so the geometry cannot report it and the timings
        // are not a gate; the count is.
        let systems_before = ferritecad_solver_lab::planegcs_native_sessions();
        let theirs = ferritecad_solver_lab::drag_with_planegcs(&sketch, &gesture)
            .expect("a linked planegcs drag must not disappear as an unavailable candidate");
        assert_eq!(
            ferritecad_solver_lab::planegcs_native_sessions(),
            systems_before + 1,
            "a gesture of {} steps built more than one native system",
            gesture.targets.len()
        );
        assert!(theirs.all_steps_converged, "planegcs failed a drag step");
        assert!(
            theirs.worst_residual <= COMPARISON_RESIDUAL_LIMIT,
            "planegcs let the sketch come apart: {:.3e}",
            theirs.worst_residual
        );
        assert!(
            theirs.worst_follow_error < 1e-6,
            "planegcs did not follow the pointer: off by {:.3e}",
            theirs.worst_follow_error
        );
        lines.push(theirs.line());
    }

    eprintln!(
        "persistent drag, {} steps ({} build):\n{}",
        gesture.targets.len(),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        lines.join("\n")
    );
}

#[test]
fn a_sketch_that_cannot_be_satisfied_is_not_reported_as_solved() {
    // The worst thing a solver can do: say yes to a drawing the geometry
    // cannot produce. A refusal costs the person a correction; a false success
    // costs them a part.
    for kind in IMPOSSIBLE {
        let sketch = problem(kind, 0);
        let outcome = LevenbergMarquardt::default().solve(&sketch, &sketch.start);
        assert!(
            !outcome.converged,
            "{}: LM claimed convergence",
            sketch.name
        );
        assert!(
            outcome.worst_residual > COMPARISON_RESIDUAL_LIMIT,
            "{}: reported as solved with residual {:.3e}, which it cannot be",
            sketch.name,
            outcome.worst_residual
        );

        #[cfg(feature = "planegcs")]
        if planegcs_ready() {
            let theirs = ferritecad_solver_lab::Planegcs.solve(&sketch, &sketch.start);
            assert!(
                !theirs.converged,
                "{}: planegcs claimed convergence",
                sketch.name
            );
            assert!(
                theirs.worst_residual > COMPARISON_RESIDUAL_LIMIT,
                "{}: planegcs reported it solved with residual {:.3e}",
                sketch.name,
                theirs.worst_residual
            );
        }
    }
}

#[test]
fn a_conflict_names_the_constraints_a_person_should_look_at() {
    // Counting is not enough. Somebody told "this sketch is over-constrained"
    // has to find the offending line themselves, and on a real sketch they
    // will not.
    let repeated = problem(Corpus::Overconstrained, 0);
    let found = blame(&repeated);
    assert!(
        !found.constraints.is_empty(),
        "a redundant constraint was detected and not named"
    );
    for index in &found.constraints {
        assert!(
            *index < repeated.constraints.len(),
            "blamed constraint {index} is not in the sketch"
        );
    }

    let sentence = found.explain(&repeated);
    assert!(
        sentence.contains("horizontal"),
        "the message must say what the constraint is, not which row it is: {sentence}"
    );
    assert!(
        !sentence.contains("Constraint {") && !sentence.contains("Point("),
        "the message reads like a debug dump: {sentence}"
    );
    eprintln!("conflict message: {sentence}");

    #[cfg(feature = "planegcs")]
    if planegcs_ready() {
        let native = ferritecad_solver_lab::blame_with_planegcs(&repeated)
            .expect("linked planegcs must return its diagnosed tags");
        assert!(
            !native.constraints.is_empty(),
            "planegcs detected redundancy but named no constraint"
        );
        assert!(
            native
                .constraints
                .iter()
                .all(|index| *index < repeated.constraints.len()),
            "planegcs returned a tag outside the caller's constraint list: {native:?}"
        );
        assert!(
            native.explain(&repeated).contains("horizontal"),
            "planegcs did not map the native tag back to a useful constraint: {native:?}"
        );
    }

    // A sound sketch blames nobody.
    assert!(blame(&problem(Corpus::Rectangle, 0)).constraints.is_empty());

    #[cfg(feature = "planegcs")]
    if planegcs_ready() {
        assert!(
            ferritecad_solver_lab::blame_with_planegcs(&problem(Corpus::Rectangle, 0))
                .expect("linked diagnosis")
                .constraints
                .is_empty()
        );
    }
}
