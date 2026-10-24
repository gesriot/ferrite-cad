// SPDX-License-Identifier: MIT
use ferritecad_document::{CutBoundary, WALL_CLEARANCE_MM, circle_clearances};
use ferritecad_kernel::{Profile, SegmentGeometry};
use ferritecad_types::{CadError, ObjectId, Result};

#[derive(Clone)]
pub(crate) struct CutContacts {
    datum: ObjectId,
    boundary: CutBoundary,
    tools: Vec<(ObjectId, [f64; 2], f64)>,
}

impl CutContacts {
    pub(crate) fn new(profile: &Profile, datum: ObjectId) -> Option<Self> {
        Some(Self {
            datum,
            boundary: CutBoundary::from_profile(profile)?,
            tools: Vec::new(),
        })
    }

    pub(crate) fn add(&mut self, id: ObjectId, profile: &Profile, datum: ObjectId) -> Result<()> {
        if self.datum != datum || !profile.inner().is_empty() {
            return Ok(());
        }
        let [segment] = profile.outer().segments() else {
            return Ok(());
        };
        let SegmentGeometry::Circle { center, radius } = segment.geometry else {
            return Ok(());
        };
        let center = [center.x, center.y];
        if let Some((side, gap)) = self.boundary.disk_contact(center, radius) {
            return Err(CadError::input(format!(
                "Cut {id} touches side or vertex {side}: gap {gap} mm is within {WALL_CLEARANCE_MM} mm"
            )));
        }
        for &(other, c, r) in &self.tools {
            for gap in circle_clearances(center, radius, c, r) {
                if gap.abs() <= WALL_CLEARANCE_MM {
                    return Err(CadError::input(format!(
                        "Cut {id} touches Cut {other}: gap {gap} mm is within {WALL_CLEARANCE_MM} mm"
                    )));
                }
            }
        }
        self.tools.push((id, center, radius));
        Ok(())
    }
}
