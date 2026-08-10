// SPDX-License-Identifier: MIT
//! Sketches to solve, stated without reference to any solver.
//!
//! Generated rather than drawn, so a problem can be grown to any size and a
//! failure names the parameters that produced it. The shapes are the ones a
//! sketcher meets: rectangles, chains of them, polygons held by distances,
//! and a bracket outline whose sides are related to each other rather than
//! fixed.

use crate::{Constraint, Point, Problem};

/// The families of sketch this bench uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Corpus {
    /// One rectangle, fully constrained: the smallest interesting case.
    Rectangle,
    /// A row of `n` rectangles, each sharing an edge with the last.
    RectangleChain,
    /// A closed polygon of `n` sides with every side length given.
    Polygon,
    /// A bracket: an outline whose opposite sides are equal and square to
    /// each other, held by two dimensions and one fixed corner.
    Bracket,
    /// A rectangle with nothing pinned, so it can slide and turn.
    Underconstrained,
    /// A rectangle told twice that one side is horizontal.
    Overconstrained,
    /// A triangle whose three sides cannot close: 10, 10 and 40.
    ///
    /// Not over-constrained but *unsatisfiable*, which is a different thing
    /// and the one a person is most likely to create by accident.
    ImpossibleTriangle,
    /// One side told it is both 60 and 70 long.
    ContradictoryDimensions,
    /// Two segments told to be both parallel and perpendicular.
    ParallelAndPerpendicular,
}

/// Builds one problem of a family at a given size.
///
/// `size` means different things per family and is part of the name, so a
/// result can always be traced back to the sketch that produced it.
pub fn problem(corpus: Corpus, size: usize) -> Problem {
    match corpus {
        Corpus::Rectangle => rectangle(),
        Corpus::RectangleChain => chain(size),
        Corpus::Polygon => polygon(size),
        Corpus::Bracket => bracket(size),
        Corpus::Underconstrained => underconstrained(),
        Corpus::Overconstrained => overconstrained(),
        Corpus::ImpossibleTriangle => impossible_triangle(),
        Corpus::ContradictoryDimensions => contradictory_dimensions(),
        Corpus::ParallelAndPerpendicular => parallel_and_perpendicular(),
    }
}

/// Sketches that have no solution at all.
///
/// A solver that reports success on one of these is worse than one that
/// fails: the drawing then says something the geometry does not.
pub const IMPOSSIBLE: [Corpus; 3] = [
    Corpus::ImpossibleTriangle,
    Corpus::ContradictoryDimensions,
    Corpus::ParallelAndPerpendicular,
];

/// Four corners, anticlockwise, starting slightly out of place so a solver
/// has something to do.
fn corners(x: f64, y: f64, width: f64, height: f64) -> Vec<f64> {
    vec![
        x + 0.3,
        y - 0.2,
        x + width - 0.4,
        y + 0.1,
        x + width + 0.2,
        y + height + 0.3,
        x - 0.1,
        y + height - 0.2,
    ]
}

fn rectangle_constraints(base: usize, width: f64, height: f64) -> Vec<Constraint> {
    let p = |offset: usize| Point(base + offset);
    vec![
        Constraint::Horizontal { a: p(0), b: p(1) },
        Constraint::Horizontal { a: p(3), b: p(2) },
        Constraint::Vertical { a: p(0), b: p(3) },
        Constraint::Vertical { a: p(1), b: p(2) },
        Constraint::Distance {
            a: p(0),
            b: p(1),
            distance: width,
        },
        Constraint::Distance {
            a: p(0),
            b: p(3),
            distance: height,
        },
    ]
}

fn rectangle() -> Problem {
    let mut constraints = vec![Constraint::Fixed {
        point: Point(0),
        x: 0.0,
        y: 0.0,
    }];
    constraints.extend(rectangle_constraints(0, 60.0, 40.0));
    Problem {
        name: "rectangle".to_owned(),
        start: corners(0.0, 0.0, 60.0, 40.0),
        constraints,
    }
}

fn chain(count: usize) -> Problem {
    let count = count.max(1);
    let mut start = Vec::new();
    let mut constraints = vec![Constraint::Fixed {
        point: Point(0),
        x: 0.0,
        y: 0.0,
    }];

    for index in 0..count {
        let base = index * 4;
        start.extend(corners(index as f64 * 30.0, 0.0, 30.0, 20.0));
        constraints.extend(rectangle_constraints(base, 30.0, 20.0));

        // Each rectangle stands on the previous one's right-hand edge.
        if index > 0 {
            let previous = (index - 1) * 4;
            constraints.push(Constraint::Coincident {
                a: Point(base),
                b: Point(previous + 1),
            });
            constraints.push(Constraint::Coincident {
                a: Point(base + 3),
                b: Point(previous + 2),
            });
        }
    }

    Problem {
        name: format!("chain-{count}"),
        start,
        constraints,
    }
}

fn polygon(sides: usize) -> Problem {
    let sides = sides.max(3);
    let radius = 50.0;
    let mut start = Vec::new();
    for index in 0..sides {
        let angle = std::f64::consts::TAU * index as f64 / sides as f64;
        // Nudged off the true polygon so the solver has work to do.
        start.push(radius * angle.cos() + 0.4);
        start.push(radius * angle.sin() - 0.3);
    }

    // The true side length of a regular polygon of this radius.
    let side = 2.0 * radius * (std::f64::consts::PI / sides as f64).sin();
    let mut constraints = vec![
        Constraint::Fixed {
            point: Point(0),
            x: radius,
            y: 0.0,
        },
        Constraint::Horizontal {
            a: Point(0),
            b: Point(sides - 1),
        },
    ];
    for index in 0..sides {
        constraints.push(Constraint::Distance {
            a: Point(index),
            b: Point((index + 1) % sides),
            distance: side,
        });
    }

    Problem {
        name: format!("polygon-{sides}"),
        start,
        constraints,
    }
}

fn bracket(arms: usize) -> Problem {
    let arms = arms.max(2);
    let mut start = Vec::new();
    let mut constraints = vec![Constraint::Fixed {
        point: Point(0),
        x: 0.0,
        y: 0.0,
    }];

    // A staircase of segments, each square to the last and the same length as
    // the one before it. Nothing is dimensioned except the first arm, so every
    // other arm depends on it — which is what makes this worth solving.
    // A staircase that goes right first, because the first arm is constrained
    // horizontal. An earlier version started it going up, which put the guess
    // ninety degrees from the answer: the Levenberg-Marquardt candidate
    // recovered and planegcs's DogLeg did not, which said more about the
    // corpus than about either solver.
    for index in 0..=arms {
        let along = index.div_ceil(2) as f64 * 20.0;
        let up = (index / 2) as f64 * 20.0;
        start.push(along + 0.2);
        start.push(up - 0.15);
    }
    constraints.push(Constraint::Distance {
        a: Point(0),
        b: Point(1),
        distance: 20.0,
    });
    constraints.push(Constraint::Horizontal {
        a: Point(0),
        b: Point(1),
    });

    for index in 1..arms {
        constraints.push(Constraint::Perpendicular {
            a: (Point(index - 1), Point(index)),
            b: (Point(index), Point(index + 1)),
        });
        constraints.push(Constraint::EqualLength {
            a: (Point(index - 1), Point(index)),
            b: (Point(index), Point(index + 1)),
        });
    }

    Problem {
        name: format!("bracket-{arms}"),
        start,
        constraints,
    }
}

fn underconstrained() -> Problem {
    Problem {
        name: "underconstrained".to_owned(),
        start: corners(0.0, 0.0, 60.0, 40.0),
        constraints: rectangle_constraints(0, 60.0, 40.0),
    }
}

fn overconstrained() -> Problem {
    let mut constraints = vec![Constraint::Fixed {
        point: Point(0),
        x: 0.0,
        y: 0.0,
    }];
    constraints.extend(rectangle_constraints(0, 60.0, 40.0));
    // Said twice, in as many words.
    constraints.push(Constraint::Horizontal {
        a: Point(0),
        b: Point(1),
    });
    Problem {
        name: "overconstrained".to_owned(),
        start: corners(0.0, 0.0, 60.0, 40.0),
        constraints,
    }
}

fn impossible_triangle() -> Problem {
    // 10 + 10 < 40. No arrangement of three points satisfies this.
    Problem {
        name: "impossible-triangle".to_owned(),
        start: vec![0.0, 0.0, 30.0, 1.0, 15.0, 9.0],
        constraints: vec![
            Constraint::Fixed {
                point: Point(0),
                x: 0.0,
                y: 0.0,
            },
            Constraint::Distance {
                a: Point(0),
                b: Point(1),
                distance: 40.0,
            },
            Constraint::Distance {
                a: Point(1),
                b: Point(2),
                distance: 10.0,
            },
            Constraint::Distance {
                a: Point(2),
                b: Point(0),
                distance: 10.0,
            },
        ],
    }
}

fn contradictory_dimensions() -> Problem {
    let mut constraints = vec![Constraint::Fixed {
        point: Point(0),
        x: 0.0,
        y: 0.0,
    }];
    constraints.extend(rectangle_constraints(0, 60.0, 40.0));
    // The same edge, told a second and different length.
    constraints.push(Constraint::Distance {
        a: Point(0),
        b: Point(1),
        distance: 70.0,
    });
    Problem {
        name: "contradictory-dimensions".to_owned(),
        start: corners(0.0, 0.0, 60.0, 40.0),
        constraints,
    }
}

fn parallel_and_perpendicular() -> Problem {
    Problem {
        name: "parallel-and-perpendicular".to_owned(),
        start: vec![0.0, 0.0, 30.0, 0.5, 0.0, 20.0, 30.0, 20.5],
        constraints: vec![
            Constraint::Fixed {
                point: Point(0),
                x: 0.0,
                y: 0.0,
            },
            Constraint::Fixed {
                point: Point(2),
                x: 0.0,
                y: 20.0,
            },
            Constraint::Distance {
                a: Point(0),
                b: Point(1),
                distance: 30.0,
            },
            Constraint::Distance {
                a: Point(2),
                b: Point(3),
                distance: 30.0,
            },
            // Both at once, which nothing but a degenerate segment satisfies.
            Constraint::Parallel {
                a: (Point(0), Point(1)),
                b: (Point(2), Point(3)),
            },
            Constraint::Perpendicular {
                a: (Point(0), Point(1)),
                b: (Point(2), Point(3)),
            },
        ],
    }
}
