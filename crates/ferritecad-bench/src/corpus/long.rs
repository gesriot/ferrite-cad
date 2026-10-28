// SPDX-License-Identifier: MIT
use std::path::{Path, PathBuf};

use ferritecad_document::{
    Document, EndCondition, Expression, ObjectPayload, Point2, SketchGeometry,
};
use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool};
use ferritecad_types::{CadError, ObjectId, Result};

#[derive(Debug, Clone, Copy)]
pub enum LongFamily {
    Pockets,
    Rework,
}

impl LongFamily {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pockets => "pockets",
            Self::Rework => "rework",
        }
    }
}

#[derive(Debug)]
pub struct LongCase {
    pub path: PathBuf,
    /// The plate, then the ordered Cuts; N counts Cuts, not the plate.
    pub features: Vec<ObjectId>,
}

pub fn generate_long(root: &Path, family: LongFamily, count: usize) -> Result<LongCase> {
    if !(10..=1000).contains(&count) {
        return Err(CadError::input(
            "long histories require 10 through 1000 Cuts",
        ));
    }
    let path = root.join(format!("{}_{count}.fcad", family.name()));
    ferritecad_fixtures::write_plate(&path)?;
    let mut document = Document::open(&path)?;
    let body = body(&document)?;
    let (plate, profile, mut sketch) = base_profile(&document, body)?;
    // Fixed dimensions and grid pitch across N keep geometry scale out of the comparison.
    let points = [(0., 0.), (140., 0.), (140., 140.), (0., 140.)];
    for (index, curve) in sketch.curves.iter_mut().enumerate() {
        let (x, y) = points[index];
        let (next_x, next_y) = points[(index + 1) % points.len()];
        curve.geometry = SketchGeometry::Line {
            start: Point2::new(x, y)?,
            end: Point2::new(next_x, next_y)?,
        };
    }
    document.write(|writer| {
        writer.put_object(
            profile,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(sketch),
        )
    })?;
    let mut features = vec![plate];
    for index in 0..count {
        let (center, radius) = match family {
            LongFamily::Pockets => (
                [4. + (index % 32) as f64 * 4., 4. + (index / 32) as f64 * 4.],
                1.,
            ),
            LongFamily::Rework => ([70., 70.], 1. + index as f64 * 0.01),
        };
        let tool = circular_tool(
            &document,
            body,
            center,
            radius,
            EndCondition::Blind {
                distance: Expression::constant(4.)?,
            },
        )?;
        document.write_changes(&tool.changes)?;
        features.push(tool.feature);
    }
    document.close()?;
    Ok(LongCase { path, features })
}
