// SPDX-License-Identifier: MIT
use ferritecad_document::{EndCondition, ObjectPayload, Sketch, SketchGeometry, SketchVertex};
use ferritecad_types::{ObjectId, Result, StableEntityId};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Source {
    facts: ferritecad_document::ExtrudeEditSource,
    pub sketches: Vec<SketchChoice>,
    pub constraint_sketches: Vec<ConstraintSketchChoice>,
    pub circle_sketches: Vec<CircleChoice>,
    pub annulus_sketches: Vec<AnnulusChoice>,
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
        axis_segment: Option<StableEntityId>,
    },
    PartialRevolve {
        feature: ObjectId,
        body: ObjectId,
        axis_segment: Option<StableEntityId>,
        radians: ferritecad_document::RevolveAngle,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SketchChoice {
    pub details: Vec<String>,
    pub sketch: ObjectId,
    pub name: Option<String>,
    pub stored: Option<Sketch>,
    pub vertices: Option<Vec<SketchVertex>>,
    pub profile_use: Option<SketchProfileUse>,
    pub refusal: Option<String>,
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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CircleChoice {
    pub sketch: ObjectId,
    pub name: Option<String>,
    pub circle: Option<SavedCircle>,
    pub refusal: Option<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavedCircle {
    pub curve_id: StableEntityId,
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub height_mm: Option<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnnulusChoice {
    pub sketch: ObjectId,
    pub name: Option<String>,
    pub annulus: Option<SavedAnnulus>,
    pub refusal: Option<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SavedAnnulus {
    pub outer_curve_id: StableEntityId,
    pub inner_curve_id: StableEntityId,
    pub center_mm: [f64; 2],
    pub inner_center_mm: [f64; 2],
    pub outer_radius_mm: f64,
    pub inner_radius_mm: f64,
    pub height_mm: Option<f64>,
}
impl From<ferritecad_document::ExtrudeEditSource> for Source {
    fn from(facts: ferritecad_document::ExtrudeEditSource) -> Self {
        let mut result = Self {
            facts,
            sketches: vec![],
            constraint_sketches: vec![],
            circle_sketches: vec![],
            annulus_sketches: vec![],
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
                                ferritecad_document::RevolveExtent::Partial { angle } => {
                                    SketchProfileUse::PartialRevolve {
                                        feature: feature.id,
                                        body,
                                        axis_segment: r.axis_segment,
                                        radians: ferritecad_document::RevolveAngle::new(
                                            angle.value(),
                                        )
                                        .ok()?,
                                    }
                                }
                                ferritecad_document::RevolveExtent::FullTurn => {
                                    SketchProfileUse::FullTurnRevolve {
                                        feature: feature.id,
                                        body,
                                        axis_segment: r.axis_segment,
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
            let vertices: Option<Vec<_>> = sketch.curves.iter().enumerate().map(|(i,c)| match c.geometry {
                SketchGeometry::Line {start,end} if !c.construction && matches!(sketch.curves[(i+1)%sketch.curves.len()].geometry, SketchGeometry::Line {start:next,..} if next == end) => Some(SketchVertex {curve_id:c.id,start_mm:[start.x,start.y]}), _=>None,
            }).collect::<Option<Vec<_>>>().filter(|v| !v.is_empty());
            let base = result.facts.objects.iter().find_map(|o| match &o.payload {
                ObjectPayload::Extrude(e) if e.profile == object.id => Some(o.id),
                _ => None,
            });
            let mut details = vec![];
            for feature in &result.facts.objects {
                match &feature.payload {
                    ObjectPayload::Fillet(f) if Some(f.edge.feature) == base => {
                        details.push(format!(
                            "Rounded by Fillet {} · radius {} mm",
                            feature.id,
                            f.radius_mm.value()
                        ))
                    }
                    ObjectPayload::Chamfer(c) if Some(c.edge.feature) == base => {
                        details.push(format!(
                            "Chamfer {} · distance {} mm",
                            feature.id,
                            c.distance_mm.value()
                        ))
                    }
                    _ => {}
                }
            }
            let cuts = result
                .facts
                .cut_features
                .iter()
                .filter_map(|c| c.saved.as_ref())
                .filter(|c| c.profile_sketch == object.id)
                .count();
            if cuts != 0 {
                details.push(format!("Base of {cuts} circular Cuts"));
            }
            result.sketches.push(SketchChoice {
                details,
                sketch: object.id,
                name: object.name.clone(),
                stored: Some(sketch.clone()),
                refusal: vertices
                    .is_none()
                    .then(|| "This form draws closed Line profiles.".into()),
                vertices,
                profile_use,
            });
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
            let circles: Option<Vec<_>> = sketch
                .curves
                .iter()
                .map(|c| match c.geometry {
                    SketchGeometry::Circle { center, radius } => {
                        Some((c.id, [center.x, center.y], radius))
                    }
                    _ => None,
                })
                .collect();
            let circle = circles.as_deref().and_then(|cs| match cs {
                [(id, center, radius)] => Some(SavedCircle {
                    curve_id: *id,
                    center_mm: *center,
                    radius_mm: *radius,
                    height_mm: height,
                }),
                _ => None,
            });
            result.circle_sketches.push(CircleChoice {
                sketch: object.id,
                name: object.name.clone(),
                refusal: circle
                    .is_none()
                    .then(|| "This form draws one Circle.".into()),
                circle,
            });
            let annulus = circles.as_deref().and_then(|cs| match cs {
                [a, b] => {
                    let (outer, inner) = if a.2 >= b.2 { (a, b) } else { (b, a) };
                    Some(SavedAnnulus {
                        outer_curve_id: outer.0,
                        inner_curve_id: inner.0,
                        center_mm: outer.1,
                        inner_center_mm: inner.1,
                        outer_radius_mm: outer.2,
                        inner_radius_mm: inner.2,
                        height_mm: height,
                    })
                }
                _ => None,
            });
            result.annulus_sketches.push(AnnulusChoice {
                sketch: object.id,
                name: object.name.clone(),
                refusal: annulus
                    .is_none()
                    .then(|| "This form draws two Circles.".into()),
                annulus,
            });
        }
        result
    }
}
