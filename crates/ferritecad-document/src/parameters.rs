// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use ferritecad_types::{CadError, Dimension, ObjectId, Quantity, Result};

use crate::expression_edit::{expressions, read_expressions};
use crate::formula::{Formula, check_name};
use crate::{Dependency, DependencyRole, Expression, ExpressionSlot, ObjectPayload, ObjectRecord};

#[derive(Clone, Copy)]
enum Slot<'a> {
    Field(ExpressionSlot),
    Parameter(&'a str, Dimension),
}

impl Slot<'_> {
    fn dimension(self) -> Dimension {
        match self {
            Self::Field(f) => f.dimension(),
            Self::Parameter(_, d) => d,
        }
    }
}

impl fmt::Display for Slot<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Field(slot) => slot.fmt(f),
            Self::Parameter(name, _) => write!(f, "parameter '{name}' expression"),
        }
    }
}

fn visit(
    id: ObjectId,
    payload: &ObjectPayload,
    mut run: impl FnMut(Slot<'_>, &Expression, Formula<'_>) -> Result<()>,
) -> Result<()> {
    let mut check = |slot, e: &Expression| {
        Formula::parse(&e.source)
            .and_then(|formula| run(slot, e, formula))
            .map_err(|error| CadError::input_because(format!("object {id} {slot}: {error}"), error))
    };
    if let ObjectPayload::Parameter(p) = payload {
        return check(Slot::Parameter(&p.name, p.dimension), &p.expression);
    }
    let mut result = Ok(());
    read_expressions(payload, |slot, e| {
        if result.is_ok() {
            result = check(Slot::Field(slot), e);
        }
    });
    result
}

pub(crate) type ParameterEdges = BTreeSet<Dependency>;

/// Label saved dependencies using the same expression grammar as validation.
/// This reads stated inputs only; it neither evaluates nor changes the graph.
pub(crate) fn consumers(
    objects: &[ObjectRecord],
    saved: &[Dependency],
) -> Result<Vec<crate::edit::ParameterConsumer>> {
    let mut result = Vec::new();
    for edge in saved.iter().filter(|e| e.role == DependencyRole::Parameter) {
        let Some(ObjectPayload::Parameter(parameter)) = objects
            .iter()
            .find(|o| o.id == edge.dependency)
            .map(|o| &o.payload)
        else {
            continue;
        };
        let Some(object) = objects.iter().find(|o| o.id == edge.dependent) else {
            continue;
        };
        visit(object.id, &object.payload, |slot, _, formula| {
            if formula.names().any(|(name, _)| name == parameter.name) {
                result.push(crate::edit::ParameterConsumer {
                    parameter: edge.dependency,
                    object: edge.dependent,
                    slot: match slot {
                        Slot::Field(slot) => Some(slot),
                        Slot::Parameter(_, _) => None,
                    },
                });
            }
            Ok(())
        })?;
    }
    Ok(result)
}

pub(crate) fn references<'a>(
    objects: impl Iterator<Item = (ObjectId, &'a ObjectPayload)> + Clone,
) -> Result<ParameterEdges> {
    let mut names = BTreeMap::new();
    for (id, payload) in objects.clone() {
        if let ObjectPayload::Parameter(p) = payload {
            check_name(&p.name)
                .map_err(|e| CadError::input_because(format!("parameter {id}: {e}"), e))?;
            if names.insert(p.name.as_str(), id).is_some() {
                return Err(CadError::input(format!(
                    "duplicate parameter name '{}'",
                    p.name
                )));
            }
        }
    }
    let mut edges = BTreeSet::new();
    for (id, payload) in objects {
        visit(id, payload, |_, _, formula| {
            for (name, range) in formula.names() {
                let dependency = *names.get(name).ok_or_else(|| {
                    CadError::input(format!(
                        "unknown parameter '{name}' at byte {}",
                        range.start + 1
                    ))
                })?;
                edges.insert(Dependency {
                    dependent: id,
                    dependency,
                    role: DependencyRole::Parameter,
                });
            }
            Ok(())
        })?;
    }
    Ok(edges)
}

pub(crate) fn check_rows(objects: &[ObjectRecord], saved: &[Dependency]) -> Result<()> {
    let expected = references(objects.iter().map(|obj| (obj.id, &obj.payload)))?;
    let actual = saved
        .iter()
        .copied()
        .filter(|e| e.role == DependencyRole::Parameter)
        .collect();
    if let Some(edge) = expected.symmetric_difference(&actual).next() {
        let kind = if expected.contains(edge) {
            "missing"
        } else {
            "unexpected"
        };
        let mut locations = Vec::new();
        let parameter = objects
            .iter()
            .find(|o| o.id == edge.dependency)
            .and_then(|o| match &o.payload {
                ObjectPayload::Parameter(p) => Some(p.name.as_str()),
                _ => None,
            });
        if let Some(object) = objects.iter().find(|o| o.id == edge.dependent) {
            visit(object.id, &object.payload, |slot, _, formula| {
                if let Some((_, range)) = formula.names().find(|(n, _)| Some(*n) == parameter) {
                    locations.push(format!("{slot} at byte {}", range.start + 1));
                } else if kind == "unexpected" {
                    locations.push(slot.to_string());
                }
                Ok(())
            })?;
        }
        return Err(CadError::input(format!(
            "object {} has {kind} parameter dependency on {} ({}) in {}",
            edge.dependent,
            edge.dependency,
            parameter.unwrap_or("unknown parameter"),
            locations.join(", ")
        )));
    }
    Ok(())
}

/// Parameter values become available only after their owning object succeeds in graph order.
#[derive(Debug, Default)]
pub struct ParameterValues(BTreeMap<String, Quantity>);

impl ParameterValues {
    fn evaluate(
        &self,
        id: ObjectId,
        payload: &ObjectPayload,
        mut receive: impl FnMut(&Expression, f64) -> Result<()>,
    ) -> Result<()> {
        visit(id, payload, |slot, expression, formula| {
            let dimension = slot.dimension();
            let value = formula.value(dimension, |name| self.0.get(name).copied())?;
            receive(expression, value)
        })
    }

    pub fn bind(&mut self, parameter: &crate::Parameter) -> Result<()> {
        self.0.insert(
            parameter.name.clone(),
            Quantity::from_base(parameter.expression.value(), parameter.dimension)?,
        );
        Ok(())
    }

    pub fn verify(&self, id: ObjectId, payload: &ObjectPayload) -> Result<()> {
        self.evaluate(id, payload, |expr, value| {
            if value.to_bits() != expr.value().to_bits() {
                return Err(CadError::input(format!(
                    "stored value {} differs from the evaluated value {value}",
                    expr.value()
                )));
            }
            Ok(())
        })?;
        payload.validate_values()
    }

    pub(crate) fn update(&mut self, id: ObjectId, payload: &mut ObjectPayload) -> Result<()> {
        let mut values = Vec::new();
        self.evaluate(id, payload, |_, value| {
            values.push(value);
            Ok(())
        })?;
        if let ObjectPayload::Parameter(p) = payload {
            p.expression.store_value(values[0]);
            self.bind(p)?;
        } else {
            let mut values = values.into_iter();
            expressions(payload, |_, e| {
                e.store_value(values.next().expect("evaluated slot"))
            });
        }
        Ok(())
    }
}

pub(crate) fn rename_references(
    id: ObjectId,
    payload: &mut ObjectPayload,
    old: &str,
    new: &str,
) -> Result<()> {
    let mut sources = Vec::new();
    visit(id, payload, |_, _, formula| {
        sources.push(formula.renamed(old, new));
        Ok(())
    })?;
    let mut sources = sources.into_iter();
    let mut replace = |e: &mut Expression| {
        if let Some(text) = sources.next().expect("visited slot") {
            e.source = text;
        }
    };
    if let ObjectPayload::Parameter(p) = payload {
        replace(&mut p.expression);
    } else {
        expressions(payload, |_, e| replace(e));
    }
    Ok(())
}

pub(crate) fn names_in(id: ObjectId, payload: &ObjectPayload, name: &str) -> Result<bool> {
    let mut found = false;
    visit(id, payload, |_, _, formula| {
        found |= formula.names().any(|(n, _)| n == name);
        Ok(())
    })?;
    Ok(found)
}
