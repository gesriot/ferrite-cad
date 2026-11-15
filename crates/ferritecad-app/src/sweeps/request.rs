// SPDX-License-Identifier: MIT
//! Saved sweep intent and its single transaction, independent of widgets.
use ferritecad_document::{
    EditChange, EndCondition, Expression, FeatureAddition, ObjectRecord, RevolveAngle, RevolveAxis,
    RevolveExtent, SketchGeometry, SolidOperation,
};
use ferritecad_scene::SketchPresentation;
use ferritecad_types::{CadError, ObjectId, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Extrude,
    Revolve,
}
impl Kind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Extrude => "Extrude",
            Self::Revolve => "Revolve",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum End {
    Blind,
    Symmetric,
    ThroughAll,
    UpToNext,
}
impl End {
    pub fn label(self) -> &'static str {
        match self {
            Self::Blind => "Blind",
            Self::Symmetric => "Symmetric",
            Self::ThroughAll => "Through all",
            Self::UpToNext => "Up to next",
        }
    }
}

#[derive(Debug)]
pub(super) struct Draft {
    pub kind: Kind,
    pub sketch: ObjectId,
    pub operation: SolidOperation,
    pub body: Option<ObjectId>,
    pub end: End,
    pub distance: String,
    pub reversed: bool,
    pub axis: RevolveAxis,
    pub full_turn: bool,
    pub angle: String,
}
impl Draft {
    pub fn new(kind: Kind, sketch: ObjectId, body: Option<ObjectId>) -> Self {
        Self {
            kind,
            sketch,
            operation: SolidOperation::NewBody,
            body,
            end: End::Blind,
            distance: "10".into(),
            reversed: false,
            axis: RevolveAxis::SketchY,
            full_turn: true,
            angle: "90".into(),
        }
    }

    pub fn changes(&self, objects: &[ObjectRecord]) -> Result<Vec<EditChange>> {
        let feature = match self.kind {
            Kind::Extrude => FeatureAddition::Extrude {
                profile: self.sketch,
                regions: Vec::new(),
                operation: self.operation,
                reversed: self.reversed,
                end_condition: match self.end {
                    End::Blind | End::Symmetric => {
                        let mm = self
                            .distance
                            .trim()
                            .parse::<f64>()
                            .map_err(|_| CadError::input("enter a finite distance in mm"))?;
                        let distance = Expression::constant(mm)?;
                        match self.end {
                            End::Blind => EndCondition::Blind { distance },
                            End::Symmetric => EndCondition::Symmetric { distance },
                            End::ThroughAll | End::UpToNext => unreachable!("distance end"),
                        }
                    }
                    End::ThroughAll => EndCondition::ThroughAll,
                    End::UpToNext => EndCondition::UpToNext,
                },
            },
            Kind::Revolve => FeatureAddition::Revolve {
                profile: self.sketch,
                regions: Vec::new(),
                operation: self.operation,
                axis: self.axis,
                extent: if self.full_turn {
                    RevolveExtent::FullTurn
                } else {
                    // The saved-angle editor parses degrees, creates a constant, then
                    // asks the same persisted partial-angle invariant.
                    let angle =
                        Expression::constant(crate::sketch::number(&self.angle)?.to_radians())?;
                    RevolveAngle::new(angle.value())?;
                    RevolveExtent::Partial { angle }
                },
            },
        };
        let name = Some(crate::object_names::next(objects, self.kind.label()));
        Ok(vec![match self.operation {
            SolidOperation::NewBody => EditChange::AddBody {
                object: ObjectId::new(),
                name: Some(crate::object_names::next(objects, "Body")),
                feature_object: ObjectId::new(),
                feature_name: name,
                feature,
            },
            SolidOperation::Add | SolidOperation::Cut | SolidOperation::Intersect => {
                EditChange::AddFeature {
                    object: ObjectId::new(),
                    body: self
                        .body
                        .ok_or_else(|| CadError::input("The document has no body to change."))?,
                    name,
                    feature,
                }
            }
            _ => {
                return Err(CadError::unsupported(
                    "This sweep operation is not supported.",
                ));
            }
        }])
    }
}

pub(super) fn object_label(object: &ObjectRecord) -> String {
    let kind = object.payload.kind().expect("catalogued object has a kind");
    format!(
        "{} '{}'",
        kind.label(),
        object.name.as_deref().unwrap_or("Unnamed")
    )
}

pub(super) fn axes(presentation: &SketchPresentation) -> Vec<(RevolveAxis, String)> {
    let mut axes = vec![
        (RevolveAxis::SketchX, "Sketch X".into()),
        (RevolveAxis::SketchY, "Sketch Y".into()),
    ];
    for curve in presentation.curves() {
        if let SketchGeometry::Line { start, end } = curve.geometry() {
            let k = axes.len() - 1;
            axes.push((
                RevolveAxis::Line(curve.id()),
                format!(
                    "Line {k}: ({}, {}) → ({}, {}) mm",
                    crate::numeric::mm(start.x),
                    crate::numeric::mm(start.y),
                    crate::numeric::mm(end.x),
                    crate::numeric::mm(end.y),
                ),
            ));
        }
    }
    axes
}
