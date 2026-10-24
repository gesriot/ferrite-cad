// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_types::{CadError, Dimension, ObjectId, Result, StableEntityId};
use serde::{Deserialize, Serialize};

use crate::{
    DependencyRole, Document, EndCondition, Expression, ObjectPayload, ObjectRecord, RevolveExtent,
    SketchConstraintRule,
};

/// A stored expression, addressed independently of geometry or evaluation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpressionSlot {
    ExtrudeDistance,
    FilletRadius,
    ChamferDistance,
    RevolveAngle,
    Distance { constraint: StableEntityId },
    Radius { constraint: StableEntityId },
    DirectedAngle { constraint: StableEntityId },
    FixedX { constraint: StableEntityId },
    FixedY { constraint: StableEntityId },
}

impl ExpressionSlot {
    pub fn dimension(self) -> Dimension {
        match self {
            Self::RevolveAngle | Self::DirectedAngle { .. } => Dimension::Angle,
            _ => Dimension::Length,
        }
    }
}

impl std::fmt::Display for ExpressionSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, constraint) = match self {
            Self::ExtrudeDistance => ("extrude_distance", None),
            Self::FilletRadius => ("fillet_radius", None),
            Self::ChamferDistance => ("chamfer_distance", None),
            Self::RevolveAngle => ("revolve_angle", None),
            Self::Distance { constraint } => ("distance", Some(constraint)),
            Self::Radius { constraint } => ("radius", Some(constraint)),
            Self::DirectedAngle { constraint } => ("directed_angle", Some(constraint)),
            Self::FixedX { constraint } => ("fixed_x", Some(constraint)),
            Self::FixedY { constraint } => ("fixed_y", Some(constraint)),
        };
        f.write_str(kind)?;
        if let Some(id) = constraint {
            write!(f, " ({id})")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EditChange {
    AddSketch {
        object: ObjectId,
        name: Option<String>,
        sketch: crate::Sketch,
    },
    AddFeature {
        object: ObjectId,
        body: ObjectId,
        name: Option<String>,
        feature: FeatureAddition,
    },
    SetEndCondition {
        object: ObjectId,
        end_condition: EndCondition,
    },
    SetExpression {
        object: ObjectId,
        slot: ExpressionSlot,
        expression: Expression,
    },
    SetCurveGeometry {
        sketch: ObjectId,
        curve: StableEntityId,
        geometry: crate::SketchGeometry,
    },
    AddConstraint {
        sketch: ObjectId,
        constraint: crate::SketchConstraint,
    },
    RemoveConstraint {
        sketch: ObjectId,
        constraint: StableEntityId,
    },
}

/// A feature's stated inputs; the candidate Body supplies its predecessor.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureAddition {
    Cut {
        profile: ObjectId,
        end_condition: EndCondition,
        reversed: bool,
    },
    Fillet {
        edge: crate::SweptEdge,
        radius_mm: Expression,
    },
    Chamfer {
        edge: crate::SweptEdge,
        distance_mm: Expression,
    },
}

impl FeatureAddition {
    pub fn payload(&self, previous: ObjectId) -> ObjectPayload {
        match self {
            Self::Cut {
                profile,
                end_condition,
                reversed,
            } => ObjectPayload::Extrude(crate::Extrude {
                profile: *profile,
                end_condition: end_condition.clone(),
                reversed: *reversed,
                operation: crate::SolidOperation::Cut,
                previous: Some(previous),
            }),
            Self::Fillet { edge, radius_mm } => ObjectPayload::Fillet(crate::Fillet {
                previous,
                edge: *edge,
                radius_mm: radius_mm.clone(),
            }),
            Self::Chamfer { edge, distance_mm } => ObjectPayload::Chamfer(crate::Chamfer {
                previous,
                edge: *edge,
                distance_mm: distance_mm.clone(),
            }),
        }
    }
}

impl EditChange {
    pub fn expression(&self) -> Option<&Expression> {
        match self {
            Self::SetExpression { expression, .. } => Some(expression),
            _ => None,
        }
    }
    pub fn constant(object: ObjectId, slot: ExpressionSlot, value: f64) -> Result<Self> {
        Ok(Self::SetExpression {
            object,
            slot,
            expression: Expression::constant(value)?,
        })
    }

    pub fn object(&self) -> ObjectId {
        match self {
            Self::SetExpression { object, .. }
            | Self::SetEndCondition { object, .. }
            | Self::AddSketch { object, .. }
            | Self::AddFeature { object, .. } => *object,
            Self::SetCurveGeometry { sketch, .. }
            | Self::AddConstraint { sketch, .. }
            | Self::RemoveConstraint { sketch, .. } => *sketch,
        }
    }

    pub(crate) fn apply(&self, payload: &mut ObjectPayload) -> Result<()> {
        if let Self::SetEndCondition { end_condition, .. } = self {
            if matches!(end_condition, EndCondition::Symmetric { .. }) {
                return Err(CadError::input(
                    "set_end_condition accepts Blind or ThroughAll",
                ));
            }
            let ObjectPayload::Extrude(extrude) = payload else {
                return Err(CadError::input(format!(
                    "object {} is not an Extrude or Cut",
                    self.object()
                )));
            };
            extrude.end_condition = end_condition.clone();
            return Ok(());
        }
        if let Self::SetExpression {
            slot, expression, ..
        } = self
        {
            return set_expression(payload, *slot, expression.clone());
        }
        let ObjectPayload::Sketch(sketch) = payload else {
            return Err(CadError::input(format!(
                "object {} is not a Sketch",
                self.object()
            )));
        };
        match self {
            Self::SetCurveGeometry {
                curve, geometry, ..
            } => {
                let saved = sketch
                    .curves
                    .iter_mut()
                    .find(|c| c.id == *curve)
                    .ok_or_else(|| CadError::input(format!("sketch has no curve {curve}")))?;
                if std::mem::discriminant(&saved.geometry) != std::mem::discriminant(geometry) {
                    return Err(CadError::input(format!(
                        "curve {curve} must keep its geometry kind"
                    )));
                }
                saved.geometry = geometry.clone();
            }
            Self::AddConstraint { constraint, .. } => {
                if sketch.constraints.iter().any(|c| c.id == constraint.id) {
                    return Err(CadError::input(format!(
                        "constraint {} already exists",
                        constraint.id
                    )));
                }
                sketch.constraints.push(constraint.clone());
            }
            Self::RemoveConstraint { constraint, .. } => {
                let index = sketch
                    .constraints
                    .iter()
                    .position(|c| c.id == *constraint)
                    .ok_or_else(|| {
                        CadError::input(format!("sketch has no constraint {constraint}"))
                    })?;
                sketch.constraints.remove(index);
            }
            Self::AddSketch { .. } | Self::AddFeature { .. } => {
                return Err(CadError::input(
                    "an object addition must be applied to the document",
                ));
            }
            Self::SetExpression { .. } | Self::SetEndCondition { .. } => {
                unreachable!("expressions were handled before reading the sketch")
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionSlotValue {
    pub object: ObjectId,
    pub name: Option<String>,
    pub slot: ExpressionSlot,
    pub expression: Expression,
    pub dimension: Dimension,
    pub refusal: Option<String>,
    payload: ObjectPayload,
}

impl ExpressionSlotValue {
    /// Forms use precisely the persistence invariants, without predicting a rebuild.
    pub fn validate(&self, value: f64) -> Result<()> {
        if let Some(reason) = &self.refusal {
            return Err(CadError::unsupported(reason.clone()));
        }
        let mut payload = self.payload.clone();
        set_expression(&mut payload, self.slot, Expression::constant(value)?)?;
        payload.validate()
    }
}

fn expressions(
    payload: &mut ObjectPayload,
    mut visit: impl FnMut(ExpressionSlot, &mut Expression),
) {
    use ExpressionSlot as S;
    match payload {
        ObjectPayload::Extrude(extrude) => match &mut extrude.end_condition {
            EndCondition::Blind { distance } | EndCondition::Symmetric { distance } => {
                visit(S::ExtrudeDistance, distance)
            }
            EndCondition::ThroughAll => {}
        },
        ObjectPayload::Fillet(fillet) => visit(S::FilletRadius, &mut fillet.radius_mm),
        ObjectPayload::Chamfer(chamfer) => visit(S::ChamferDistance, &mut chamfer.distance_mm),
        ObjectPayload::Revolve(revolve) => {
            if let RevolveExtent::Partial { angle } = &mut revolve.extent {
                visit(S::RevolveAngle, angle);
            }
        }
        ObjectPayload::Sketch(sketch) => {
            for constraint in &mut sketch.constraints {
                let id = constraint.id;
                match &mut constraint.rule {
                    SketchConstraintRule::Distance { distance, .. } => {
                        visit(S::Distance { constraint: id }, distance)
                    }
                    SketchConstraintRule::Radius { radius, .. } => {
                        visit(S::Radius { constraint: id }, radius)
                    }
                    SketchConstraintRule::DirectedAngle { angle, .. } => {
                        visit(S::DirectedAngle { constraint: id }, angle)
                    }
                    SketchConstraintRule::Fixed { x, y, .. } => {
                        visit(S::FixedX { constraint: id }, x);
                        visit(S::FixedY { constraint: id }, y);
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn set_expression(
    payload: &mut ObjectPayload,
    slot: ExpressionSlot,
    expression: Expression,
) -> Result<()> {
    let mut found = false;
    expressions(payload, |candidate, value| {
        if candidate == slot {
            *value = expression.clone();
            found = true;
        }
    });
    if !found {
        return Err(CadError::input(format!(
            "object has no expression slot {slot}"
        )));
    }
    Ok(())
}

fn parameter_dependents(document: &Document) -> Result<BTreeSet<ObjectId>> {
    Ok(document
        .dependencies()?
        .into_iter()
        .filter(|d| d.role == DependencyRole::Parameter)
        .map(|d| d.dependent)
        .collect())
}

fn parameter_reason(object: ObjectId) -> String {
    format!("object {object} depends on a Parameter; parameter expressions cannot be edited yet")
}

impl Document {
    pub fn expression_slots(&self) -> Result<Vec<ExpressionSlotValue>> {
        let parameters = parameter_dependents(self)?;
        let mut result = Vec::new();
        for object in self.objects()? {
            expressions(&mut object.payload.clone(), |slot, expression| {
                result.push(ExpressionSlotValue {
                    object: object.id,
                    name: object.name.clone(),
                    slot,
                    expression: expression.clone(),
                    dimension: slot.dimension(),
                    refusal: parameters
                        .contains(&object.id)
                        .then(|| parameter_reason(object.id)),
                    payload: object.payload.clone(),
                });
            });
        }
        Ok(result)
    }

    /// Apply in request order and write each changed object once in one SQLite transaction.
    pub fn write_changes(&mut self, changes: &[EditChange]) -> Result<Vec<StableEntityId>> {
        self.write_reading(|document, writer| {
            let parameters = parameter_dependents(document)?;
            let mut changed = BTreeMap::<ObjectId, CandidateObject>::new();
            let mut ordinal = document
                .objects()?
                .iter()
                .map(|o| o.ordinal)
                .max()
                .unwrap_or(0);
            for change in changes {
                let id = change.object();
                match change {
                    EditChange::AddSketch { name, sketch, .. } => {
                        insert_candidate(
                            document,
                            &mut changed,
                            &mut ordinal,
                            id,
                            name,
                            ObjectPayload::Sketch(sketch.clone()),
                        )?;
                    }
                    EditChange::AddFeature {
                        body,
                        name,
                        feature,
                        ..
                    } => {
                        let saved = candidate_object(document, &mut changed, &parameters, *body)?;
                        let ObjectPayload::Body(body_payload) = &saved.payload else {
                            return Err(CadError::input(format!("object {body} is not a Body")));
                        };
                        let previous = body_payload.tip_feature.ok_or_else(|| {
                            CadError::input(format!("Body {body} has no current feature"))
                        })?;
                        if matches!(
                            feature,
                            FeatureAddition::Cut {
                                end_condition: EndCondition::Symmetric { .. },
                                ..
                            }
                        ) {
                            return Err(CadError::input("a new Cut accepts Blind or ThroughAll"));
                        }
                        insert_candidate(
                            document,
                            &mut changed,
                            &mut ordinal,
                            id,
                            name,
                            feature.payload(previous),
                        )?;
                        candidate_object(document, &mut changed, &parameters, *body)?.payload =
                            ObjectPayload::Body(crate::Body {
                                tip_feature: Some(id),
                            });
                    }
                    _ => {
                        let object = candidate_object(document, &mut changed, &parameters, id)?;
                        change
                            .apply(&mut object.payload)
                            .map_err(|source| CadError::Context {
                                kind: source.kind(),
                                message: format!("cannot edit object {id}"),
                                source: Box::new(source),
                            })?;
                    }
                }
            }
            for object in changed.values() {
                object.payload.validate()?;
            }
            // New rows follow request order, independently of their caller-supplied UUIDs.
            let mut ordered: Vec<_> = changed.iter().collect();
            ordered.sort_by_key(|(_, object)| object.ordinal);
            for (id, object) in ordered {
                if object.original.is_some() {
                    writer.update_payload(*id, &object.payload)?;
                } else {
                    writer.put_object(
                        *id,
                        None,
                        object.ordinal,
                        object.name.as_deref(),
                        &object.payload,
                    )?;
                }
            }
            for (id, object) in &changed {
                let old = object
                    .original
                    .as_ref()
                    .map(|o| o.payload.dependencies(*id))
                    .unwrap_or_default();
                let new = object.payload.dependencies(*id);
                for dependency in old.iter().filter(|d| !new.contains(d)) {
                    writer.remove_dependency(*dependency)?;
                }
                for dependency in new.iter().filter(|d| !old.contains(d)) {
                    writer.add_dependency(*dependency)?;
                }
            }
            let mut minted = Vec::new();
            for reference in crate::output_names::candidate_output_references(document)? {
                writer.put_topology_ref(&reference)?;
                minted.push(reference.id);
            }
            Ok(minted)
        })
    }
}

struct CandidateObject {
    original: Option<ObjectRecord>,
    payload: ObjectPayload,
    name: Option<String>,
    ordinal: i64,
}

fn candidate_object<'a>(
    document: &Document,
    changed: &'a mut BTreeMap<ObjectId, CandidateObject>,
    parameters: &BTreeSet<ObjectId>,
    id: ObjectId,
) -> Result<&'a mut CandidateObject> {
    if parameters.contains(&id) {
        return Err(CadError::unsupported(parameter_reason(id)));
    }
    if let std::collections::btree_map::Entry::Vacant(entry) = changed.entry(id) {
        let original = document
            .object(id)?
            .ok_or_else(|| CadError::input(format!("unknown object {id}")))?;
        // This check also protects the Body whose tip an addition moves.
        if original.payload.to_storage_bytes()?.as_slice() != original.storage_bytes() {
            return Err(CadError::unsupported(format!(
                "object {id}'s saved payload cannot be rewritten losslessly by this build"
            )));
        }
        entry.insert(CandidateObject {
            payload: original.payload.clone(),
            name: original.name.clone(),
            ordinal: original.ordinal,
            original: Some(original),
        });
    }
    Ok(changed.get_mut(&id).expect("candidate object was inserted"))
}

fn insert_candidate(
    document: &Document,
    changed: &mut BTreeMap<ObjectId, CandidateObject>,
    ordinal: &mut i64,
    id: ObjectId,
    name: &Option<String>,
    payload: ObjectPayload,
) -> Result<()> {
    if changed.contains_key(&id) || document.object(id)?.is_some() {
        return Err(CadError::input(format!("object {id} already exists")));
    }
    let get = |id| -> Result<ObjectPayload> {
        if let Some(object) = changed.get(&id) {
            return Ok(object.payload.clone());
        }
        document
            .object(id)?
            .map(|o| o.payload)
            .ok_or_else(|| CadError::input(format!("unknown object {id}")))
    };
    for dependency in payload.dependencies(id) {
        let target = get(dependency.dependency)?;
        let valid = match dependency.role {
            DependencyRole::Plane => matches!(target, ObjectPayload::DatumPlane(_)),
            DependencyRole::Profile => matches!(target, ObjectPayload::Sketch(_)),
            DependencyRole::Predecessor | DependencyRole::BodyTip => {
                target.kind().is_some_and(crate::ObjectKind::is_feature)
            }
            _ => true,
        };
        if !valid {
            return Err(CadError::input(format!(
                "object {} has the wrong kind for {}",
                dependency.dependency,
                dependency.role.as_str()
            )));
        }
    }
    let edge = match &payload {
        ObjectPayload::Fillet(f) => Some(f.edge),
        ObjectPayload::Chamfer(c) => Some(c.edge),
        _ => None,
    };
    if let Some(edge) = edge {
        get(edge.feature)?;
    }
    *ordinal = ordinal
        .checked_add(1)
        .ok_or_else(|| CadError::input("new object ordinal overflows"))?;
    changed.insert(
        id,
        CandidateObject {
            original: None,
            payload,
            name: name.clone(),
            ordinal: *ordinal,
        },
    );
    Ok(())
}
