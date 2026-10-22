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

#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionChange {
    pub object: ObjectId,
    pub slot: ExpressionSlot,
    pub expression: Expression,
}

impl ExpressionChange {
    pub fn constant(object: ObjectId, slot: ExpressionSlot, value: f64) -> Result<Self> {
        Ok(Self {
            object,
            slot,
            expression: Expression::constant(value)?,
        })
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
            "object has no expression slot {slot:?}"
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
    pub fn write_expression_changes(&mut self, changes: &[ExpressionChange]) -> Result<()> {
        let parameters = parameter_dependents(self)?;
        let mut changed = BTreeMap::<ObjectId, ObjectRecord>::new();
        for change in changes {
            if parameters.contains(&change.object) {
                return Err(CadError::unsupported(parameter_reason(change.object)));
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = changed.entry(change.object)
            {
                entry.insert(
                    self.object(change.object)?.ok_or_else(|| {
                        CadError::input(format!("unknown object {}", change.object))
                    })?,
                );
            }
            let object = changed
                .get_mut(&change.object)
                .expect("changed object was inserted");
            set_expression(&mut object.payload, change.slot, change.expression.clone()).map_err(
                |source| CadError::Context {
                    kind: source.kind(),
                    message: format!("cannot edit object {} ({:?})", change.object, change.slot),
                    source: Box::new(source),
                },
            )?;
        }
        for object in changed.values() {
            object.payload.validate()?;
        }
        self.write(|writer| {
            for object in changed.values() {
                writer.put_object(
                    object.id,
                    object.parent,
                    object.ordinal,
                    object.name.as_deref(),
                    &object.payload,
                )?;
            }
            Ok(())
        })
    }
}
