// SPDX-License-Identifier: MIT
use crate::Expression;
use ferritecad_types::{CadError, Result};

pub fn validate_extrude_distance(distance_mm: f64) -> Result<()> {
    Expression::constant(distance_mm)?;
    if distance_mm <= 0. {
        return Err(CadError::input("extrude distance must be positive"));
    }
    Ok(())
}
