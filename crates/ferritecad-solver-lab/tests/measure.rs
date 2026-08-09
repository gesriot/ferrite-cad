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
    Constraint, Corpus, DoesNothing, LevenbergMarquardt, Point, Problem, Solver, problem,
};

/// Every candidate the bench knows about.
fn candidates() -> Vec<Box<dyn Solver>> {
    vec![Box::new(LevenbergMarquardt::default())]
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
                outcome.iterations,
                outcome.worst_residual,
                micros(outcome.elapsed)
            ));

            // A sketch with freedom left still has to satisfy what it was
            // told; it simply has more than one way to. A sketch with
            // redundant constraints must also solve — saying a thing twice
            // does not make it unsatisfiable.
            assert!(
                outcome.converged,
                "{} could not solve {}: worst residual {:.3e} after {} iterations",
                solver.name(),
                problem.name,
                outcome.worst_residual,
                outcome.iterations
            );
        }
    }

    eprintln!(
        "solver comparison ({} build; timings from a debug build are not a \n\
         prediction about a release one, only a comparison between candidates \n\
         measured the same way):\n{}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        rows.join("\n")
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
    assert!(solved.worst_residual < idle.worst_residual / 1e6);
}

#[test]
fn a_solved_rectangle_is_actually_a_rectangle() {
    // Residuals near zero is what the solver claims. This checks the geometry
    // it produced, which is what the claim is supposed to mean.
    let problem = problem(Corpus::Rectangle, 0);
    let outcome = LevenbergMarquardt::default().solve(&problem, &problem.start);
    assert!(outcome.converged);

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
            outcome.converged,
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
