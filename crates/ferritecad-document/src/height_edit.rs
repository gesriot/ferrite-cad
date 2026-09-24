// SPDX-License-Identifier: MIT
//! Height edits retain the whole supported Cut history, including new floors.

use crate::cut_edit::{CutHistory, added_floor_references, floor_transition};
use crate::{
    DependencyRole, Document, EndCondition, Expression, ExtrudeChoice, ObjectPayload, ObjectRecord,
    PolygonExtrusion, SavedCutTool, SemanticRole, SolidOperation, TopologyRef,
};
use ferritecad_types::{CadError, ContentHash, ObjectId, Result, StableEntityId};

#[derive(Debug, Clone, PartialEq)]
pub struct ProtectedCutFloor {
    pub feature: ObjectId,
    pub references: Vec<StableEntityId>,
}

/// Constraints read from the same validated history as the other edit catalogues.
#[derive(Debug, Clone, PartialEq)]
pub struct BaseHeightContext {
    pub body: ObjectId,
    pub base_feature: ObjectId,
    pub profile: ObjectId,
    pub extents_mm: [[f64; 2]; 2],
    pub tools: Vec<SavedCutTool>,
    pub protected_floors: Vec<ProtectedCutFloor>,
}

impl CutHistory {
    pub(crate) fn height_context(&self) -> BaseHeightContext {
        BaseHeightContext {
            body: self.target.body,
            base_feature: self.target.base_feature,
            profile: self.target.profile,
            extents_mm: self.target.extents_mm,
            tools: self.target.tools.clone(),
            protected_floors: self
                .cuts
                .iter()
                .map(|c| ProtectedCutFloor {
                    feature: c.feature,
                    references: c.protected_floor_references.clone(),
                })
                .collect(),
        }
    }
}

/// A conservative dispatch check, not a second history traversal. Any boolean,
/// predecessor/target edge, carried name or non-base tip requires the full reader.
/// In particular a damaged history cannot become a standalone extrusion edit.
pub(crate) fn has_history(document: &Document, objects: &[ObjectRecord]) -> Result<bool> {
    if objects.iter().any(|o| match &o.payload {
        ObjectPayload::Extrude(e) => {
            e.operation != SolidOperation::NewBody
                || e.previous.is_some()
                || e.target_body.is_some()
        }
        ObjectPayload::Body(b) => b.tip_feature.is_some_and(|tip| {
            !objects.iter().any(|p| {
                p.id == tip
                    && matches!(&p.payload, ObjectPayload::Extrude(e)
                if e.operation == SolidOperation::NewBody && e.previous.is_none())
            })
        }),
        _ => false,
    }) {
        return Ok(true);
    }
    if document.dependencies()?.iter().any(|d| {
        matches!(
            d.role,
            DependencyRole::Predecessor | DependencyRole::TargetBody
        )
    }) {
        return Ok(true);
    }
    Ok(document.topology_refs()?.iter().any(|r| {
        matches!(
            r.output_role,
            SemanticRole::CarriedCap { .. }
                | SemanticRole::CarriedSide { .. }
                | SemanticRole::OriginCap { .. }
                | SemanticRole::OriginSide { .. }
        )
    }))
}

pub fn validate_extrude_distance(distance_mm: f64) -> Result<()> {
    Expression::constant(distance_mm)?;
    if distance_mm <= 0. {
        return Err(CadError::input("extrude distance must be positive"));
    }
    Ok(())
}

impl BaseHeightContext {
    /// No I/O, no geometry kernel, and no minted IDs during draft validation.
    pub fn validate_height(&self, height_mm: f64) -> Result<()> {
        validate_extrude_distance(height_mm)?;
        let [[x0, y0], [x1, y1]] = self.extents_mm;
        PolygonExtrusion::new(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], height_mm)?;
        for tool in &self.tools {
            let protected = self
                .protected_floors
                .iter()
                .find(|p| p.feature == tool.feature)
                .ok_or_else(|| CadError::input("height context has no floor facts for a Cut"))?;
            // Losing a saved floor is reported with its protected UUIDs even if
            // the new wall would also leave that tool deeper than the plate.
            floor_transition(
                tool.feature,
                &protected.references,
                tool.depth_mm,
                height_mm,
            )?;
            crate::cut_edit::validate(
                height_mm,
                self.extents_mm,
                &crate::CircularCut {
                    center_mm: tool.center_mm,
                    radius_mm: tool.radius_mm,
                    depth_mm: tool.depth_mm,
                },
            )
            .map_err(|e| match e {
                CadError::Input { message, source } => CadError::Input {
                    message: format!("Cut {}: {message}", tool.feature),
                    source,
                },
                other => other,
            })?;
        }
        Ok(())
    }

    fn added_references(&self, height_mm: f64) -> Vec<TopologyRef> {
        self.tools
            .iter()
            .filter(|t| !t.leaves_a_floor && t.depth_mm < height_mm)
            .flat_map(|t| added_floor_references(t.feature, &self.tools))
            .collect()
    }
}

impl ExtrudeChoice {
    /// The document-domain numeric check used by the form and preparation.
    pub fn validate_distance(&self, distance_mm: f64) -> Result<()> {
        validate_extrude_distance(distance_mm)?;
        if let Some(history) = &self.cut_history {
            history.validate_height(distance_mm)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedExtrudeHeight {
    pub(crate) feature: ObjectRecord,
    pub(crate) history: Option<BaseHeightContext>,
    pub(crate) added_references: Vec<TopologyRef>,
    // Covers current tool data, refs and SQL facts even if the selected row did
    // not change. A backup keeps this version; any intervening edit does not.
    pub(crate) source_version: ContentHash,
}
impl PreparedExtrudeHeight {
    pub fn feature(&self) -> &ObjectRecord {
        &self.feature
    }
    pub fn history(&self) -> Option<&BaseHeightContext> {
        self.history.as_ref()
    }
    pub fn added_references(&self) -> &[TopologyRef] {
        &self.added_references
    }
}

pub fn prepare_extrude_height(
    document: &Document,
    feature: ObjectId,
    height_mm: f64,
) -> Result<PreparedExtrudeHeight> {
    validate_extrude_distance(height_mm)?;
    let objects = document.objects()?;
    let mut record = objects
        .iter()
        .find(|o| o.id == feature)
        .cloned()
        .ok_or_else(|| {
            CadError::input(format!("feature {feature} does not exist in this document"))
        })?;
    crate::editable_extrude(document, &record)?;
    let history = if has_history(document, &objects)? {
        let history = crate::cut_edit::saved_history(document, &objects)?;
        if history.target.base_feature != feature || history.target.tools.is_empty() {
            return Err(CadError::unsupported(
                "select the original base Extrude of the Cut history",
            ));
        }
        let context = history.height_context();
        context.validate_height(height_mm)?;
        Some(context)
    } else {
        None
    };
    let ObjectPayload::Extrude(extrude) = &mut record.payload else {
        unreachable!("checked extrusion")
    };
    extrude.end_condition = EndCondition::Blind {
        distance: Expression::constant(height_mm)?,
    };
    let added_references = history
        .as_ref()
        .map(|h| h.added_references(height_mm))
        .unwrap_or_default();
    Ok(PreparedExtrudeHeight {
        feature: record,
        history,
        added_references,
        source_version: document.content_version()?,
    })
}

pub(crate) fn rederive(document: &Document, prepared: &PreparedExtrudeHeight) -> Result<()> {
    if document.content_version()? != prepared.source_version {
        return Err(CadError::input(
            "document history changed after height preparation",
        ));
    }
    let ObjectPayload::Extrude(extrude) = &prepared.feature.payload else {
        return Err(CadError::input(
            "prepared height edit must carry an Extrude",
        ));
    };
    let EndCondition::Blind { distance } = &extrude.end_condition else {
        return Err(CadError::input(
            "prepared height edit must carry a Blind distance",
        ));
    };
    let mut checked = prepare_extrude_height(document, prepared.feature.id, distance.value())?;
    if checked.added_references.len() != prepared.added_references.len() {
        return Err(CadError::input(
            "prepared height edit has an incomplete set of new floor names",
        ));
    }
    for (mine, theirs) in checked
        .added_references
        .iter_mut()
        .zip(&prepared.added_references)
    {
        mine.id = theirs.id;
    }
    if checked != *prepared {
        return Err(CadError::input(
            "prepared height edit does not describe the current document",
        ));
    }
    Ok(())
}
