// SPDX-License-Identifier: MIT
use ferritecad_document::{EndCondition, ObjectPayload, Sketch, SketchGeometry};
use ferritecad_types::{ObjectId, Result};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Source {
    facts: ferritecad_document::ExtrudeEditSource,
    /// Sketch identities that have a drawing in the accepted scene.
    pub built_sketches: Vec<ObjectId>,
    /// Resolved datum frames from that same rebuild; never recomputed by a form.
    pub datum_planes: Vec<(ObjectId, ferritecad_kernel::SketchPlane)>,
    /// Accepted rebuild explanations for built sketches that cannot supply a sweep profile.
    pub profile_failures: Vec<(ObjectId, String)>,
    /// Solver reports paired with the accepted drawing, including on a checkpoint rename.
    pub sketch_reports: Vec<(ObjectId, ferritecad_eval::SketchSolveReport)>,
    pub conflicts: Vec<crate::sketch_conflicts::Conflict>,
    pub cut_bodies: Vec<crate::feature_facts::CutChoice>,
    pub cut_features: Vec<crate::feature_facts::CutParameterChoice>,
    pub fillet_bodies: Vec<crate::feature_facts::EdgeChoice>,
    pub chamfer_bodies: Vec<crate::feature_facts::EdgeChoice>,
    pub constraint_sketches: Vec<ConstraintSketchChoice>,
}
impl std::ops::Deref for Source {
    type Target = ferritecad_document::ExtrudeEditSource;
    fn deref(&self) -> &Self::Target {
        &self.facts
    }
}
impl std::ops::DerefMut for Source {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.facts
    }
}
impl Source {
    #[cfg(test)]
    pub(crate) fn read(document: &ferritecad_document::Document) -> Result<Self> {
        ferritecad_document::ExtrudeEditSource::read(document).map(Self::from)
    }
}
pub(crate) fn read(path: &std::path::Path) -> Result<Source> {
    ferritecad_jobs::read_extrude_source(path).map(Source::from)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SketchProfileUse {
    BlindExtrude {
        feature: ObjectId,
        height_mm: f64,
    },
    FullTurnRevolve {
        feature: ObjectId,
        body: ObjectId,
        axis: ferritecad_document::RevolveAxis,
    },
    PartialRevolve {
        feature: ObjectId,
        body: ObjectId,
        axis: ferritecad_document::RevolveAxis,
        radians: ferritecad_document::RevolveAngle,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConstraintSketchChoice {
    pub sketch: ObjectId,
    pub name: Option<String>,
    pub stored: Option<Sketch>,
    pub height_mm: Option<f64>,
    pub profile_use: Option<SketchProfileUse>,
    pub refusal: Option<String>,
}
impl ConstraintSketchChoice {
    pub(crate) fn validate_edits(
        &self,
        edits: &ferritecad_document::SketchConstraintEdits,
    ) -> Result<()> {
        ferritecad_document::sketch_constraint_changes(
            self.sketch,
            self.stored
                .as_ref()
                .expect("constraint form stores its sketch"),
            edits,
        )
        .map(|_| ())
    }
}
impl From<ferritecad_document::ExtrudeEditSource> for Source {
    fn from(mut facts: ferritecad_document::ExtrudeEditSource) -> Self {
        // The document's numeric_form_refusal remains the only literal check.
        // A window offers its text editor; the document and CLI keep their wording.
        for slot in &mut facts.expression_slots {
            if slot.refusal.is_some() {
                slot.refusal = Some(crate::numeric::EXPRESSION_REFUSAL.into());
            }
        }
        let (cut_bodies, cut_features, fillet_bodies) = crate::feature_facts::read(&facts.objects);
        let mut result = Self {
            chamfer_bodies: fillet_bodies.clone(),
            cut_bodies,
            cut_features,
            fillet_bodies,
            facts,
            conflicts: Vec::new(),
            sketch_reports: Vec::new(),
            built_sketches: Vec::new(),
            datum_planes: Vec::new(),
            profile_failures: Vec::new(),
            constraint_sketches: vec![],
        };
        for object in &result.facts.objects {
            let ObjectPayload::Sketch(sketch) = &object.payload else {
                continue;
            };
            let profile_use =
                result
                    .facts
                    .objects
                    .iter()
                    .find_map(|feature| match &feature.payload {
                        ObjectPayload::Extrude(e) if e.profile == object.id => {
                            match &e.end_condition {
                                EndCondition::Blind { distance } => {
                                    Some(SketchProfileUse::BlindExtrude {
                                        feature: feature.id,
                                        height_mm: distance.value(),
                                    })
                                }
                                EndCondition::Symmetric { .. }
                                | EndCondition::UpToNext
                                | EndCondition::ThroughAll
                                | EndCondition::UpToFace { .. }
                                | EndCondition::UpToVertex { .. } => None,
                                _ => None,
                            }
                        }
                        ObjectPayload::Revolve(r) if r.profile == object.id => {
                            let body =
                                result.facts.objects.iter().find_map(|o| match &o.payload {
                                    ObjectPayload::Body(b) if b.tip_feature == Some(feature.id) => {
                                        Some(o.id)
                                    }
                                    _ => None,
                                })?;
                            Some(match &r.extent {
                                ferritecad_document::RevolveExtent::Partial { angle }
                                | ferritecad_document::RevolveExtent::ByAngle { angle }
                                    if !r.extent.is_full_turn() =>
                                {
                                    SketchProfileUse::PartialRevolve {
                                        feature: feature.id,
                                        body,
                                        axis: r.axis,
                                        radians: ferritecad_document::RevolveAngle::new(
                                            angle.value(),
                                        )
                                        .ok()?,
                                    }
                                }
                                extent if extent.is_full_turn() => {
                                    SketchProfileUse::FullTurnRevolve {
                                        feature: feature.id,
                                        body,
                                        axis: r.axis,
                                    }
                                }
                                _ => return None,
                            })
                        }
                        _ => None,
                    });
            let height = match profile_use {
                Some(SketchProfileUse::BlindExtrude { height_mm, .. }) => Some(height_mm),
                _ => None,
            };
            let drawable = sketch.curves.iter().all(|c| {
                matches!(
                    c.geometry,
                    SketchGeometry::Line { .. } | SketchGeometry::Circle { .. }
                )
            });
            result.constraint_sketches.push(ConstraintSketchChoice {
                sketch: object.id,
                name: object.name.clone(),
                stored: drawable.then(|| sketch.clone()),
                height_mm: height,
                profile_use,
                refusal: (!drawable).then(|| "This form draws Lines and Circles.".into()),
            });
        }
        result
    }
}
