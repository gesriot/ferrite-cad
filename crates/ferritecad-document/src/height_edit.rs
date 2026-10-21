// SPDX-License-Identifier: MIT
//! Height edits retain the whole supported Cut history, including new floors.

use crate::cut_edit::{CutHistory, added_floor_references, floor_transition};
use crate::{
    CutBoundary, DependencyRole, Document, EndCondition, Expression, ExtrudeChoice, ObjectPayload,
    ObjectRecord, SavedCutTool, SemanticRole, SolidOperation, TopologyRef,
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
    /// The part's real outer wall; a height edit never changes it.
    pub boundary: CutBoundary,
    pub tools: Vec<SavedCutTool>,
    pub protected_floors: Vec<ProtectedCutFloor>,
}

impl CutHistory {
    pub(crate) fn height_context(&self) -> BaseHeightContext {
        BaseHeightContext {
            body: self.target.body,
            base_feature: self.target.base_feature,
            profile: self.target.profile,
            boundary: self.target.boundary.clone(),
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
        ) && objects.iter().any(|o| o.id == d.dependent)
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
        ) && objects.iter().any(|o| o.id == r.producer_feature)
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
        self.boundary.polygon(height_mm)?;
        for tool in &self.tools {
            let protected = self
                .protected_floors
                .iter()
                .find(|p| p.feature == tool.feature)
                .ok_or_else(|| CadError::input("height context has no floor facts for a Cut"))?;
            // Losing a saved floor is reported with its protected UUIDs even if
            // the new wall would also leave that tool deeper than the plate.
            // ThroughAll has no floor at any height and is never "too deep";
            // Blind depths stay absolute and keep both checks.
            floor_transition(tool.feature, &protected.references, tool.extent, height_mm)?;
            crate::cut_edit::validate(
                height_mm,
                &self.boundary,
                &crate::CircularCut {
                    center_mm: tool.center_mm,
                    radius_mm: tool.radius_mm,
                    extent: tool.extent,
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
            .filter(|t| !t.leaves_a_floor && t.extent.leaves_a_floor(height_mm))
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
    /// §28C/§28L: the saved Fillets over this plate in history order, exactly
    /// as the radius edit reads them. They keep their rows; only the plate
    /// under them changes.
    pub(crate) fillets: Vec<crate::SavedFillet>,
    /// §29B: the one saved Chamfer over this plate, exactly as the distance
    /// edit reads it. It keeps its row; only the plate under it changes.
    pub(crate) chamfer: Option<crate::SavedChamfer>,
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
    /// The Fillets the height edit keeps, in history order.
    pub fn fillets(&self) -> &[crate::SavedFillet] {
        &self.fillets
    }
    /// The first Fillet, which rounds the plate.
    pub fn fillet(&self) -> Option<&crate::SavedFillet> {
        self.fillets.first()
    }
    /// §29B: the Chamfer the height edit keeps, if the plate has one.
    pub fn chamfer(&self) -> Option<&crate::SavedChamfer> {
        self.chamfer.as_ref()
    }
    pub fn added_references(&self) -> &[TopologyRef] {
        &self.added_references
    }
}

fn body_scopes(
    document: &Document,
    objects: &[ObjectRecord],
) -> Result<Vec<std::collections::BTreeSet<ObjectId>>> {
    let mut inputs = std::collections::BTreeMap::<ObjectId, Vec<ObjectId>>::new();
    for dependency in document.dependencies()? {
        inputs
            .entry(dependency.dependent)
            .or_default()
            .push(dependency.dependency);
    }
    let mut scopes = Vec::new();
    for body in objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
    {
        let mut ids = std::collections::BTreeSet::new();
        let mut pending = vec![body.id];
        while let Some(id) = pending.pop() {
            if ids.insert(id)
                && let Some(dependencies) = inputs.get(&id)
            {
                pending.extend(dependencies);
            }
        }
        scopes.push(ids);
    }
    Ok(scopes)
}

/// Select policy inputs through the owning body's dependencies, so independent
/// bodies do not change which history the selected height belongs to. Keep
/// unclaimed objects visible to policy checks, including malformed extra features.
pub(crate) fn objects_for_feature(
    document: &Document,
    objects: &[ObjectRecord],
    feature: ObjectId,
) -> Result<Vec<ObjectRecord>> {
    let all_scopes = body_scopes(document, objects)?;
    let claimed: std::collections::BTreeSet<_> = all_scopes.iter().flatten().copied().collect();
    let mut scopes = all_scopes.iter().filter(|ids| ids.contains(&feature));
    let selected = scopes.next();
    if scopes.next().is_some() {
        return Err(CadError::unsupported(
            "the selected feature belongs to multiple bodies",
        ));
    }
    Ok(match selected {
        Some(ids) => objects
            .iter()
            .filter(|o| ids.contains(&o.id) || !claimed.contains(&o.id))
            .cloned()
            .collect(),
        None => objects.to_vec(),
    })
}

pub(crate) fn scoped_cut_heights(
    document: &Document,
    objects: &[ObjectRecord],
) -> Result<
    std::collections::BTreeMap<ObjectId, std::result::Result<Option<BaseHeightContext>, String>>,
> {
    let mut contexts = std::collections::BTreeMap::new();
    let scopes = body_scopes(document, objects)?;
    let claimed: std::collections::BTreeSet<_> = scopes.iter().flatten().copied().collect();
    for scope in scopes {
        let local: Vec<_> = objects
            .iter()
            .filter(|o| scope.contains(&o.id) || !claimed.contains(&o.id))
            .cloned()
            .collect();
        let height = (|| {
            if has_history(document, &local)? {
                Ok(Some(
                    crate::cut_edit::saved_history(document, &local)?.height_context(),
                ))
            } else {
                Ok(None)
            }
        })()
        .map_err(|error: CadError| error.to_string());
        for object in local
            .iter()
            .filter(|o| scope.contains(&o.id) && matches!(o.payload, ObjectPayload::Extrude(_)))
        {
            let value = contexts.entry(object.id).or_insert_with(|| height.clone());
            if value != &height {
                *value = Err("the selected feature belongs to multiple bodies".to_owned());
            }
        }
    }
    Ok(contexts)
}

pub fn prepare_extrude_height(
    document: &Document,
    feature: ObjectId,
    height_mm: f64,
) -> Result<PreparedExtrudeHeight> {
    validate_extrude_distance(height_mm)?;
    let objects = objects_for_feature(document, &document.objects()?, feature)?;
    let mut record = objects
        .iter()
        .find(|o| o.id == feature)
        .cloned()
        .ok_or_else(|| {
            CadError::input(format!("feature {feature} does not exist in this document"))
        })?;
    crate::editable_extrude(document, &record)?;
    // §29B: the plate under the one saved Chamfer, through the reader the
    // distance edit uses. Asked first: a document with no Chamfer answers
    // `None` without reading anything else, and one with a Chamfer outside its
    // class (a second one, a Fillet or a Cut beside it, a dimension) is refused
    // here with the guilty UUID, never read as some other history. The Chamfer
    // adds no bound on the height: its distance's bound comes from the two
    // adjacent sides, and OCCT chamfers the edge at any height it can build
    // the plate at all but 1e-5 mm and below, refusing the rest itself.
    let chamfer = crate::chamfer::saved_chamfer(document, &objects)?;
    if let Some(saved) = &chamfer
        && saved.base_feature != feature
    {
        return Err(CadError::unsupported(format!(
            "select the base Extrude {} under Chamfer {}; only the chamfered plate's height \
             can be edited",
            saved.base_feature, saved.feature
        )));
    }
    let (history, fillets) = if chamfer.is_some() || !has_history(document, &objects)? {
        (None, Vec::new())
    } else if let Some(over) = crate::fillet_radius::fillets_over_plate(document, &objects)? {
        // §28C: the plate under the one saved Fillet, through the frame the
        // radius edit reads; §28I: or under both Fillets of a §28G history,
        // through the reader the radius edit uses for two. The Fillets add no
        // bound on the height: OCCT rounds a vertical edge at any height the
        // plate can have that it can round at all, and refuses the rest itself.
        let saved = over.first();
        if saved.previous != feature {
            return Err(CadError::unsupported(format!(
                "select the base Extrude {} under Fillet {}; only the rounded plate's height \
                 can be edited",
                saved.previous, saved.feature
            )));
        }
        (None, over.fillets)
    } else {
        let history = crate::cut_edit::saved_history(document, &objects)?;
        if history.target.base_feature != feature || history.target.tools.is_empty() {
            return Err(CadError::unsupported(
                "select the original base Extrude of the Cut history",
            ));
        }
        let context = history.height_context();
        context.validate_height(height_mm)?;
        (Some(context), Vec::new())
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
        fillets,
        chamfer,
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
