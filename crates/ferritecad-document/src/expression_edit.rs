// SPDX-License-Identifier: MIT
use crate::candidate::Candidate;

use crate::formula::Formula;

use ferritecad_types::{CadError, Dimension, ObjectId, Result, StableEntityId};
use serde::{Deserialize, Serialize};

use crate::{
    Document, EndCondition, Expression, ObjectPayload, RevolveExtent, SketchConstraintRule,
};

/// A stored expression, addressed independently of geometry or evaluation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpressionSlot {
    ExtrudeDistance,
    ExtrudeEndOffset,
    DatumOffset,
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
            Self::DatumOffset => ("datum_offset", None),
            Self::ExtrudeEndOffset => ("extrude_end_offset", None),
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
    AddParameter {
        object: ObjectId,
        name: String,
        dimension: Dimension,
        expression: String,
    },
    SetParameter {
        object: ObjectId,
        expression: String,
    },
    RenameParameter {
        object: ObjectId,
        name: String,
    },
    RemoveObject {
        object: ObjectId,
    },
    /// Set the shown history position of a Body; absence shows its tip.
    SetRollback {
        /// Body whose displayed result and insertion position change.
        object: ObjectId,
        /// Earlier feature in that Body, or no rollback marker.
        feature: Option<ObjectId>,
    },
    SetSuppressed {
        object: ObjectId,
        suppressed: bool,
    },
    SetExpressionText {
        object: ObjectId,
        slot: ExpressionSlot,
        source: String,
    },
    SetKeepSelection {
        object: ObjectId,
        selection: crate::KeepSelection,
    },
    AddDatumPlane {
        object: ObjectId,
        name: Option<String>,
        plane: crate::DatumPlane,
    },
    SetDatumFace {
        object: ObjectId,
        face: crate::EntityReference,
    },
    AddBody {
        object: ObjectId,
        name: Option<String>,
        feature_object: ObjectId,
        feature_name: Option<String>,
        feature: FeatureAddition,
    },
    SetRegions {
        object: ObjectId,
        regions: Vec<StableEntityId>,
    },
    SetOperation {
        object: ObjectId,
        operation: crate::SolidOperation,
    },
    SetReversed {
        object: ObjectId,
        reversed: bool,
    },
    SetRevolveAxis {
        object: ObjectId,
        axis: crate::RevolveAxis,
    },
    SetRevolveExtent {
        object: ObjectId,
        extent: RevolveExtent,
    },
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
    SetEdges {
        object: ObjectId,
        edges: Vec<crate::EntityReference>,
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
    /// Append a caller-identified curve to the drawing.
    AddCurve {
        /// Sketch receiving the curve.
        sketch: ObjectId,
        /// Saved geometry and identity to append.
        curve: crate::SketchCurve,
    },
    /// Remove exactly one curve; final sketch validation protects its constraints.
    RemoveCurve {
        /// Sketch containing the curve.
        sketch: ObjectId,
        /// Identity to remove.
        curve: StableEntityId,
    },
    /// Set whether a curve guides the drawing instead of bounding a profile.
    SetConstruction {
        /// Sketch containing the curve.
        sketch: ObjectId,
        /// Identity whose flag changes.
        curve: StableEntityId,
        /// New construction flag.
        construction: bool,
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
    KeepSolids {
        selection: crate::KeepSelection,
    },
    Extrude {
        profile: ObjectId,
        regions: Vec<StableEntityId>,
        end_condition: EndCondition,
        reversed: bool,
        operation: crate::SolidOperation,
    },
    Revolve {
        profile: ObjectId,
        regions: Vec<StableEntityId>,
        axis: crate::RevolveAxis,
        extent: RevolveExtent,
        operation: crate::SolidOperation,
    },
    Fillet {
        edges: Vec<crate::EntityReference>,
        radius_mm: Expression,
    },
    Chamfer {
        edges: Vec<crate::EntityReference>,
        distance_mm: Expression,
    },
}

impl FeatureAddition {
    pub fn payload(&self, previous: Option<ObjectId>) -> Result<ObjectPayload> {
        Ok(match self {
            Self::KeepSolids { selection } => ObjectPayload::KeepSolids(crate::KeepSolids {
                previous: previous
                    .ok_or_else(|| CadError::input("KeepSolids needs a predecessor"))?,
                selection: selection.clone(),
            }),
            Self::Extrude {
                profile,
                regions,
                end_condition,
                reversed,
                operation,
            } => ObjectPayload::Extrude(crate::Extrude {
                profile: *profile,
                regions: regions.clone(),
                end_condition: end_condition.clone(),
                reversed: *reversed,
                operation: *operation,
                previous,
            }),
            Self::Revolve {
                profile,
                regions,
                axis,
                extent,
                operation,
            } => ObjectPayload::Revolve(crate::Revolve {
                profile: *profile,
                regions: regions.clone(),
                axis: *axis,
                extent: extent.clone(),
                operation: *operation,
                previous,
            }),
            Self::Fillet { edges, radius_mm } => ObjectPayload::Fillet(crate::Fillet {
                previous: previous
                    .ok_or_else(|| CadError::input("edge feature needs a predecessor"))?,
                edges: edges.clone(),
                radius_mm: radius_mm.clone(),
            }),
            Self::Chamfer { edges, distance_mm } => ObjectPayload::Chamfer(crate::Chamfer {
                previous: previous
                    .ok_or_else(|| CadError::input("edge feature needs a predecessor"))?,
                edges: edges.clone(),
                distance_mm: distance_mm.clone(),
            }),
        })
    }
}

impl EditChange {
    pub fn expression(&self) -> Option<&Expression> {
        match self {
            Self::SetExpression { expression, .. } => Some(expression),
            Self::AddParameter { .. }
            | Self::SetParameter { .. }
            | Self::RenameParameter { .. }
            | Self::RemoveObject { .. }
            | Self::SetSuppressed { .. }
            | Self::SetRollback { .. }
            | Self::SetExpressionText { .. }
            | Self::SetKeepSelection { .. }
            | Self::AddDatumPlane { .. }
            | Self::SetDatumFace { .. }
            | Self::AddBody { .. }
            | Self::SetRegions { .. }
            | Self::SetOperation { .. }
            | Self::SetReversed { .. }
            | Self::SetRevolveAxis { .. }
            | Self::SetRevolveExtent { .. }
            | Self::AddSketch { .. }
            | Self::AddFeature { .. }
            | Self::SetEdges { .. }
            | Self::SetEndCondition { .. }
            | Self::AddCurve { .. }
            | Self::RemoveCurve { .. }
            | Self::SetConstruction { .. }
            | Self::SetCurveGeometry { .. }
            | Self::AddConstraint { .. }
            | Self::RemoveConstraint { .. } => None,
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
            Self::AddParameter { object, .. }
            | Self::SetParameter { object, .. }
            | Self::RenameParameter { object, .. }
            | Self::RemoveObject { object }
            | Self::SetSuppressed { object, .. }
            | Self::SetRollback { object, .. }
            | Self::SetExpressionText { object, .. }
            | Self::SetKeepSelection { object, .. }
            | Self::AddDatumPlane { object, .. }
            | Self::SetDatumFace { object, .. }
            | Self::AddBody { object, .. }
            | Self::SetEdges { object, .. }
            | Self::SetRegions { object, .. }
            | Self::SetOperation { object, .. }
            | Self::SetReversed { object, .. }
            | Self::SetRevolveAxis { object, .. }
            | Self::SetRevolveExtent { object, .. }
            | Self::SetExpression { object, .. }
            | Self::SetEndCondition { object, .. }
            | Self::AddSketch { object, .. }
            | Self::AddFeature { object, .. } => *object,
            Self::AddCurve { sketch, .. }
            | Self::RemoveCurve { sketch, .. }
            | Self::SetConstruction { sketch, .. }
            | Self::SetCurveGeometry { sketch, .. }
            | Self::AddConstraint { sketch, .. }
            | Self::RemoveConstraint { sketch, .. } => *sketch,
        }
    }

    pub(crate) fn apply(&self, payload: &mut ObjectPayload) -> Result<()> {
        if let Self::SetEndCondition { end_condition, .. } = self {
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
            return set_expression(payload, *slot, expression.clone(), true);
        }
        if let Self::SetExpressionText { slot, source, .. } = self {
            if let (ExpressionSlot::RevolveAngle, ObjectPayload::Revolve(r)) = (slot, &mut *payload)
            {
                r.extent = RevolveExtent::ByAngle {
                    angle: Expression::new(source, 0.)?,
                };
                return Ok(());
            }
            return set_expression(payload, *slot, Expression::new(source, 0.)?, false);
        }
        match self {
            Self::SetDatumFace { face, .. } => {
                let ObjectPayload::DatumPlane(crate::DatumPlane::Face { face: saved, .. }) =
                    payload
                else {
                    return Err(CadError::input("face belongs to a face datum plane"));
                };
                crate::entity_reference::validate_kind(face, crate::EntityKind::Face)?;
                *saved = *face;
                return Ok(());
            }
            Self::SetKeepSelection { selection, .. } => {
                let ObjectPayload::KeepSolids(keep) = payload else {
                    return Err(CadError::input("selection edit requires KeepSolids"));
                };
                keep.selection = selection.clone();
                return Ok(());
            }
            Self::SetEdges { edges, .. } => {
                crate::entity_reference::validate_edges(edges)?;
                match payload {
                    ObjectPayload::Fillet(feature) => feature.edges = edges.clone(),
                    ObjectPayload::Chamfer(feature) => feature.edges = edges.clone(),
                    _ => return Err(CadError::input("edges belong to a Fillet or Chamfer")),
                }
                return Ok(());
            }
            Self::SetRegions { regions, .. } => {
                match payload {
                    ObjectPayload::Extrude(e) => e.regions = regions.clone(),
                    ObjectPayload::Revolve(r) => r.regions = regions.clone(),
                    _ => return Err(CadError::input("regions belong to an Extrude or Revolve")),
                }
                return Ok(());
            }
            Self::SetOperation { operation, .. } => {
                match payload {
                    ObjectPayload::Extrude(e) => e.operation = *operation,
                    ObjectPayload::Revolve(r) => r.operation = *operation,
                    _ => {
                        return Err(CadError::input(
                            "operation belongs to an Extrude or Revolve",
                        ));
                    }
                }
                return Ok(());
            }
            Self::SetReversed { reversed, .. } => {
                let ObjectPayload::Extrude(e) = payload else {
                    return Err(CadError::input("reversed belongs to an Extrude"));
                };
                e.reversed = *reversed;
                return Ok(());
            }
            Self::SetRevolveAxis { axis, .. } => {
                let ObjectPayload::Revolve(r) = payload else {
                    return Err(CadError::input("axis belongs to a Revolve"));
                };
                r.axis = *axis;
                return Ok(());
            }
            Self::SetRevolveExtent { extent, .. } => {
                let ObjectPayload::Revolve(r) = payload else {
                    return Err(CadError::input("revolution extent belongs to a Revolve"));
                };
                r.extent = extent.clone();
                return Ok(());
            }
            Self::AddParameter { .. }
            | Self::SetParameter { .. }
            | Self::RenameParameter { .. }
            | Self::RemoveObject { .. }
            | Self::SetSuppressed { .. }
            | Self::SetRollback { .. }
            | Self::SetExpressionText { .. }
            | Self::AddDatumPlane { .. }
            | Self::AddBody { .. }
            | Self::AddSketch { .. }
            | Self::AddFeature { .. }
            | Self::SetEndCondition { .. }
            | Self::SetExpression { .. }
            | Self::AddCurve { .. }
            | Self::RemoveCurve { .. }
            | Self::SetConstruction { .. }
            | Self::SetCurveGeometry { .. }
            | Self::AddConstraint { .. }
            | Self::RemoveConstraint { .. } => {}
        }
        let ObjectPayload::Sketch(sketch) = payload else {
            return Err(CadError::input(format!(
                "object {} is not a Sketch",
                self.object()
            )));
        };
        match self {
            Self::AddCurve { curve, .. } => {
                if sketch.curves.iter().any(|saved| saved.id == curve.id) {
                    return Err(CadError::input(format!(
                        "curve {} already exists",
                        curve.id
                    )));
                }
                sketch.curves.push(curve.clone());
            }
            Self::RemoveCurve { curve, .. } => {
                let index = sketch
                    .curves
                    .iter()
                    .position(|saved| saved.id == *curve)
                    .ok_or_else(|| CadError::input(format!("sketch has no curve {curve}")))?;
                sketch.curves.remove(index);
            }
            Self::SetConstruction {
                curve,
                construction,
                ..
            } => {
                let saved = sketch
                    .curves
                    .iter_mut()
                    .find(|saved| saved.id == *curve)
                    .ok_or_else(|| CadError::input(format!("sketch has no curve {curve}")))?;
                saved.construction = *construction;
            }
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
            Self::AddParameter { .. }
            | Self::SetParameter { .. }
            | Self::RenameParameter { .. }
            | Self::RemoveObject { .. }
            | Self::SetSuppressed { .. }
            | Self::SetRollback { .. }
            | Self::AddDatumPlane { .. }
            | Self::AddBody { .. }
            | Self::AddSketch { .. }
            | Self::AddFeature { .. } => {
                return Err(CadError::input(
                    "an object addition must be applied to the document",
                ));
            }
            Self::SetKeepSelection { .. }
            | Self::SetDatumFace { .. }
            | Self::SetEdges { .. }
            | Self::SetRegions { .. }
            | Self::SetOperation { .. }
            | Self::SetReversed { .. }
            | Self::SetRevolveAxis { .. }
            | Self::SetRevolveExtent { .. }
            | Self::SetExpression { .. }
            | Self::SetExpressionText { .. }
            | Self::SetEndCondition { .. } => {
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
        set_expression(&mut payload, self.slot, Expression::constant(value)?, true)?;
        payload.validate()
    }
}

// Keep the read and write traversals exhaustive over the same stored slots.
macro_rules! visit_slots {
    ($payload:ident, $visit:ident) => {{
        use ExpressionSlot as S;
        match $payload {
            ObjectPayload::DatumPlane(crate::DatumPlane::Offset { distance_mm, .. }) => {
                $visit(S::DatumOffset, distance_mm)
            }
            ObjectPayload::DatumPlane(crate::DatumPlane::Face { offset_mm, .. }) => {
                $visit(S::DatumOffset, offset_mm)
            }
            ObjectPayload::Extrude(crate::Extrude { end_condition, .. }) => match end_condition {
                EndCondition::Blind { distance } | EndCondition::Symmetric { distance } => {
                    $visit(S::ExtrudeDistance, distance)
                }
                EndCondition::UpToFace { offset_mm, .. } => $visit(S::ExtrudeEndOffset, offset_mm),
                EndCondition::ThroughAll
                | EndCondition::UpToNext
                | EndCondition::UpToVertex { .. } => {}
            },
            ObjectPayload::Fillet(crate::Fillet { radius_mm, .. }) => {
                $visit(S::FilletRadius, radius_mm)
            }
            ObjectPayload::Chamfer(crate::Chamfer { distance_mm, .. }) => {
                $visit(S::ChamferDistance, distance_mm)
            }
            ObjectPayload::Revolve(crate::Revolve { extent, .. }) => match extent {
                RevolveExtent::Partial { angle } | RevolveExtent::ByAngle { angle } => {
                    $visit(S::RevolveAngle, angle)
                }
                RevolveExtent::FullTurn => {}
            },
            ObjectPayload::Sketch(crate::Sketch { constraints, .. }) => {
                for crate::SketchConstraint { id, rule } in constraints {
                    let constraint = *id;
                    match rule {
                        SketchConstraintRule::Distance { distance, .. } => {
                            $visit(S::Distance { constraint }, distance)
                        }
                        SketchConstraintRule::Radius { radius, .. } => {
                            $visit(S::Radius { constraint }, radius)
                        }
                        SketchConstraintRule::DirectedAngle { angle, .. } => {
                            $visit(S::DirectedAngle { constraint }, angle)
                        }
                        SketchConstraintRule::Fixed { x, y, .. } => {
                            $visit(S::FixedX { constraint }, x);
                            $visit(S::FixedY { constraint }, y);
                        }
                        _ => {}
                    }
                }
            }
            ObjectPayload::KeepSolids(_)
            | ObjectPayload::DatumPlane(crate::DatumPlane::Fixed { .. })
            | ObjectPayload::Body(_)
            | ObjectPayload::Parameter(_)
            | ObjectPayload::ImportedStep(_) => {}
        }
    }};
}

pub(crate) fn expressions(
    payload: &mut ObjectPayload,
    mut visit: impl FnMut(ExpressionSlot, &mut Expression),
) {
    visit_slots!(payload, visit);
}

pub(crate) fn read_expressions(
    payload: &ObjectPayload,
    mut visit: impl FnMut(ExpressionSlot, &Expression),
) {
    visit_slots!(payload, visit);
}

fn set_expression(
    payload: &mut ObjectPayload,
    slot: ExpressionSlot,
    expression: Expression,
    literal_only: bool,
) -> Result<()> {
    let mut replace = Some(expression);
    let mut result = Err(CadError::input(format!(
        "object has no expression slot {slot}"
    )));
    expressions(payload, |candidate, value| {
        if candidate == slot {
            if literal_only && let Some(reason) = value.numeric_form_refusal() {
                result = Err(CadError::unsupported(reason));
            } else {
                *value = replace.take().expect("unique expression slot");
                result = Ok(());
            }
        }
    });
    result
}

impl Expression {
    pub fn numeric_form_refusal(&self) -> Option<&'static str> {
        (!Formula::parse(&self.source).is_ok_and(|f| f.is_literal()))
            .then_some("this value is an expression; send a text `expression` through `edit`")
    }
}

impl Document {
    pub fn expression_slots(&self) -> Result<Vec<ExpressionSlotValue>> {
        let mut result = Vec::new();
        for object in self.objects()? {
            read_expressions(&object.payload, |slot, expression| {
                result.push(ExpressionSlotValue {
                    object: object.id,
                    name: object.name.clone(),
                    slot,
                    expression: expression.clone(),
                    dimension: slot.dimension(),
                    refusal: expression.numeric_form_refusal().map(str::to_owned),
                    payload: object.payload.clone(),
                });
            });
        }
        Ok(result)
    }

    /// Apply in request order and write each changed object once in one SQLite transaction.
    pub fn write_changes(&mut self, changes: &[EditChange]) -> Result<()> {
        self.write_reading(|document, writer| {
            let mut candidate = Candidate::read(document)?;
            for change in changes {
                let id = change.object();
                match change {
                    EditChange::AddParameter {
                        name,
                        dimension,
                        expression,
                        ..
                    } => {
                        candidate.check_name(id, name)?;
                        candidate.add(
                            id,
                            &Some(name.clone()),
                            ObjectPayload::Parameter(crate::Parameter {
                                name: name.clone(),
                                dimension: *dimension,
                                expression: Expression::new(expression, 0.)?,
                            }),
                        )?;
                    }
                    EditChange::SetParameter { expression, .. } => {
                        candidate
                            .parameter_mut(id)?
                            .expression
                            .source
                            .clone_from(expression);
                    }
                    EditChange::RenameParameter { name, .. } => candidate.rename(id, name)?,
                    EditChange::RemoveObject { .. } => candidate.remove(id)?,
                    EditChange::SetRollback { feature, .. } => {
                        candidate.set_rollback(id, *feature)?;
                    }
                    EditChange::SetSuppressed { suppressed, .. } => {
                        candidate.set_suppressed(id, *suppressed)?;
                    }
                    EditChange::AddBody {
                        name,
                        feature_object,
                        feature_name,
                        feature,
                        ..
                    } => {
                        let payload = feature.payload(None)?;
                        candidate.add(*feature_object, feature_name, payload)?;
                        candidate.add(
                            id,
                            name,
                            ObjectPayload::Body(crate::Body {
                                tip_feature: Some(*feature_object),
                                rollback: None,
                            }),
                        )?;
                    }
                    EditChange::AddDatumPlane { name, plane, .. } => {
                        candidate.add(id, name, ObjectPayload::DatumPlane(plane.clone()))?;
                    }
                    EditChange::AddSketch { name, sketch, .. } => {
                        candidate.add(id, name, ObjectPayload::Sketch(sketch.clone()))?;
                    }
                    EditChange::AddFeature {
                        body,
                        name,
                        feature,
                        ..
                    } => {
                        candidate.add_feature(id, *body, name, feature)?;
                    }
                    EditChange::AddCurve { .. }
                    | EditChange::RemoveCurve { .. }
                    | EditChange::SetConstruction { .. } => {
                        let object = candidate.edit(id)?;
                        if !matches!(object.payload, ObjectPayload::Sketch(_)) {
                            return Err(CadError::input(format!(
                                "{} is not a Sketch; only a Sketch has editable curves.",
                                object.payload.label(object.name.as_deref(), id)
                            )));
                        }
                        change.apply(&mut object.payload)?;
                    }
                    EditChange::SetExpressionText { .. }
                    | EditChange::SetKeepSelection { .. }
                    | EditChange::SetDatumFace { .. }
                    | EditChange::SetRegions { .. }
                    | EditChange::SetOperation { .. }
                    | EditChange::SetReversed { .. }
                    | EditChange::SetRevolveAxis { .. }
                    | EditChange::SetRevolveExtent { .. }
                    | EditChange::SetEdges { .. }
                    | EditChange::SetEndCondition { .. }
                    | EditChange::SetExpression { .. }
                    | EditChange::SetCurveGeometry { .. }
                    | EditChange::AddConstraint { .. }
                    | EditChange::RemoveConstraint { .. } => {
                        let object = candidate.edit(id)?;
                        change
                            .apply(&mut object.payload)
                            .map_err(|source| CadError::Context {
                                kind: source.kind(),
                                message: format!("cannot edit object {id}: {source}"),
                                source: Box::new(source),
                            })?;
                    }
                }
            }
            candidate.write(document, writer)
        })
    }
}
