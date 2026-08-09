// SPDX-License-Identifier: MIT
//! The small amount of dense linear algebra this bench needs.
//!
//! Written out rather than taken from a crate. A comparison between solvers
//! should not be shaped by what one library makes convenient, and every
//! numerical decision here — the damping, the tolerance a rank is judged
//! against — is a decision the comparison is about.

/// A dense matrix in row-major order.
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    pub rows: usize,
    pub columns: usize,
    pub values: Vec<f64>,
}

impl Matrix {
    pub fn zeros(rows: usize, columns: usize) -> Self {
        Self {
            rows,
            columns,
            values: vec![0.0; rows * columns],
        }
    }

    pub fn at(&self, row: usize, column: usize) -> f64 {
        self.values[row * self.columns + column]
    }

    pub fn set(&mut self, row: usize, column: usize, value: f64) {
        self.values[row * self.columns + column] = value;
    }

    pub fn add(&mut self, row: usize, column: usize, value: f64) {
        self.values[row * self.columns + column] += value;
    }

    /// `self^T * self`, which is symmetric and the left side of a normal
    /// equation.
    pub fn transpose_times_self(&self) -> Self {
        let mut out = Self::zeros(self.columns, self.columns);
        for row in 0..self.rows {
            for i in 0..self.columns {
                let left = self.at(row, i);
                if left == 0.0 {
                    continue;
                }
                for j in 0..self.columns {
                    out.add(i, j, left * self.at(row, j));
                }
            }
        }
        out
    }

    /// `self^T * vector`.
    pub fn transpose_times(&self, vector: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; self.columns];
        for (row, scale) in vector.iter().enumerate().take(self.rows) {
            if *scale == 0.0 {
                continue;
            }
            for (column, value) in out.iter_mut().enumerate() {
                *value += self.at(row, column) * scale;
            }
        }
        out
    }

    /// The number of independent rows, judged against `tolerance`.
    ///
    /// Gaussian elimination with partial pivoting on a copy. This is what says
    /// whether a sketch is under-constrained (rank below the number of
    /// unknowns) or carries redundant constraints (rank below the number of
    /// rows), which is the diagnosis a person actually needs from a solver.
    pub fn rank(&self, tolerance: f64) -> usize {
        let mut work = self.clone();
        let mut rank = 0;

        for column in 0..work.columns {
            if rank == work.rows {
                break;
            }
            // The largest remaining entry in this column, for stability.
            let (pivot_row, pivot) = (rank..work.rows).fold((rank, 0.0), |best, row| {
                let value = work.at(row, column).abs();
                if value > best.1 { (row, value) } else { best }
            });
            if pivot <= tolerance {
                continue;
            }

            for c in 0..work.columns {
                let swapped = work.at(rank, c);
                let value = work.at(pivot_row, c);
                work.set(rank, c, value);
                work.set(pivot_row, c, swapped);
            }
            for row in (rank + 1)..work.rows {
                let factor = work.at(row, column) / work.at(rank, column);
                if factor == 0.0 {
                    continue;
                }
                for c in column..work.columns {
                    let value = work.at(row, c) - factor * work.at(rank, c);
                    work.set(row, c, value);
                }
            }
            rank += 1;
        }
        rank
    }
}

/// Solves a symmetric positive definite system by Cholesky decomposition.
///
/// Returns `None` when the matrix is not positive definite, which for a
/// damped normal equation means the damping was not enough — the caller
/// raises it and tries again rather than treating this as an error.
pub fn solve_spd(matrix: &Matrix, rhs: &[f64]) -> Option<Vec<f64>> {
    let n = matrix.rows;
    let mut lower = Matrix::zeros(n, n);

    for i in 0..n {
        for j in 0..=i {
            let mut sum = matrix.at(i, j);
            for k in 0..j {
                sum -= lower.at(i, k) * lower.at(j, k);
            }
            if i == j {
                if sum <= 0.0 || !sum.is_finite() {
                    return None;
                }
                lower.set(i, j, sum.sqrt());
            } else {
                lower.set(i, j, sum / lower.at(j, j));
            }
        }
    }

    // Forward substitution, then back.
    let mut y = vec![0.0; n];
    for i in 0..n {
        let mut sum = rhs[i];
        for (k, solved) in y.iter().enumerate().take(i) {
            sum -= lower.at(i, k) * solved;
        }
        y[i] = sum / lower.at(i, i);
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut sum = y[i];
        for (k, solved) in x.iter().enumerate().skip(i + 1) {
            sum -= lower.at(k, i) * solved;
        }
        x[i] = sum / lower.at(i, i);
    }

    if x.iter().all(|value| value.is_finite()) {
        Some(x)
    } else {
        None
    }
}

pub fn norm(values: &[f64]) -> f64 {
    values.iter().map(|value| value * value).sum::<f64>().sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_system_is_solved() {
        // [[4,1],[1,3]] x = [1,2] has the solution [1/11, 7/11].
        let mut a = Matrix::zeros(2, 2);
        a.set(0, 0, 4.0);
        a.set(0, 1, 1.0);
        a.set(1, 0, 1.0);
        a.set(1, 1, 3.0);

        let x = solve_spd(&a, &[1.0, 2.0]).expect("positive definite");
        assert!((x[0] - 1.0 / 11.0).abs() < 1e-12);
        assert!((x[1] - 7.0 / 11.0).abs() < 1e-12);
    }

    #[test]
    fn a_matrix_that_is_not_positive_definite_is_reported_not_guessed() {
        let mut a = Matrix::zeros(2, 2);
        a.set(0, 0, 0.0);
        a.set(1, 1, 1.0);
        assert!(solve_spd(&a, &[1.0, 1.0]).is_none());
    }

    #[test]
    fn rank_counts_independent_rows() {
        let mut a = Matrix::zeros(3, 3);
        // Two independent rows and their sum.
        for (row, values) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]]
            .into_iter()
            .enumerate()
        {
            for (column, value) in values.into_iter().enumerate() {
                a.set(row, column, value);
            }
        }
        assert_eq!(a.rank(1e-9), 2);
    }
}
