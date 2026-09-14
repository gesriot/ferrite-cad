// SPDX-License-Identifier: MIT
//! Bounded persisted Line orientation/length/pin editing. No solver or wire format.
use crate::{
    Document, ObjectPayload, ObjectRecord, Sketch, SketchConstraint, SketchConstraintRule,
    SketchPointRef, SketchPointSelector,
};
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};
use std::collections::BTreeSet;

/// A finite, strictly positive Euclidean Line length in millimetres.
/// Private validated bits make exact Eq safe: NaN and both zeros are excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineLengthMm(u64);
impl LineLengthMm {
    pub fn new(value: f64) -> Result<Self> {
        if !value.is_finite() || value <= 0. {
            return Err(CadError::input(
                "Line length must be finite and positive in mm",
            ));
        }
        Ok(Self(value.to_bits()))
    }
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// A finite signed sketch coordinate in millimetres. Zero is normalised to one
/// pattern so exact Eq agrees with f64 equality; NaN and infinity are excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SketchCoordinateMm(u64);
impl SketchCoordinateMm {
    pub fn new(value: f64) -> Result<Self> {
        if !value.is_finite() {
            return Err(CadError::input("sketch coordinate must be finite in mm"));
        }
        Ok(Self(if value == 0. {
            0f64.to_bits()
        } else {
            value.to_bits()
        }))
    }
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// Which end of a Line a pin names. The model's `At` selector belongs to point
/// geometry, so a Line endpoint request cannot spell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndpoint {
    Start,
    End,
}
impl LineEndpoint {
    pub fn selector(self) -> SketchPointSelector {
        match self {
            Self::Start => SketchPointSelector::Start,
            Self::End => SketchPointSelector::End,
        }
    }
    pub fn of(selector: SketchPointSelector) -> Option<Self> {
        match selector {
            SketchPointSelector::Start => Some(Self::Start),
            SketchPointSelector::End => Some(Self::End),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        self.selector().as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineConstraintKind {
    Horizontal,
    Vertical,
    Distance(LineLengthMm),
    /// One endpoint of this Line pinned at explicit millimetres.
    Fixed {
        at: LineEndpoint,
        x: SketchCoordinateMm,
        y: SketchCoordinateMm,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddLineConstraint {
    pub curve: StableEntityId,
    pub kind: LineConstraintKind,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SketchConstraintEdits {
    pub remove: Vec<StableEntityId>,
    pub add: Vec<AddLineConstraint>,
}

/// Same objects()/content-version snapshot as coordinate discovery; independent eligibility.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintSketchChoice {
    pub sketch: ObjectId,
    pub name: Option<String>,
    pub stored: Option<Sketch>,
    pub height_mm: Option<f64>,
    pub refusal: Option<String>,
}

pub fn constraint_sketch_choices(
    document: &Document,
    objects: &[ObjectRecord],
) -> Vec<ConstraintSketchChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .map(|o| match supported(document, objects, o) {
            Ok(height) => {
                let ObjectPayload::Sketch(sketch) = &o.payload else {
                    unreachable!("checked")
                };
                ConstraintSketchChoice {
                    sketch: o.id,
                    name: o.name.clone(),
                    stored: Some(sketch.clone()),
                    height_mm: Some(height),
                    refusal: None,
                }
            }
            Err(e) => ConstraintSketchChoice {
                sketch: o.id,
                name: o.name.clone(),
                stored: None,
                height_mm: None,
                refusal: Some(e.to_string()),
            },
        })
        .collect()
}

fn supported(document: &Document, objects: &[ObjectRecord], object: &ObjectRecord) -> Result<f64> {
    let (_, height) = crate::sketch_edit::supported(document, objects, object, true)?;
    let ObjectPayload::Sketch(sketch) = &object.payload else {
        unreachable!("checked")
    };
    managed(sketch)?;
    Ok(height)
}

fn endpoints(curve: StableEntityId) -> (SketchPointRef, SketchPointRef) {
    (
        SketchPointRef::new(curve, SketchPointSelector::Start),
        SketchPointRef::new(curve, SketchPointSelector::End),
    )
}
fn unordered(a: SketchPointRef, b: SketchPointRef) -> [SketchPointRef; 2] {
    if a < b { [a, b] } else { [b, a] }
}
fn closures(sketch: &Sketch) -> Vec<[SketchPointRef; 2]> {
    sketch
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let next = sketch.curves[(i + 1) % sketch.curves.len()].id;
            unordered(endpoints(c.id).1, endpoints(next).0)
        })
        .collect()
}
fn line_of(rule: SketchConstraintRule) -> Option<(StableEntityId, LineConstraintKind)> {
    if let SketchConstraintRule::Fixed { point, x, y } = rule {
        return Some((
            point.curve,
            LineConstraintKind::Fixed {
                at: LineEndpoint::of(point.at)?,
                x: SketchCoordinateMm::new(x).ok()?,
                y: SketchCoordinateMm::new(y).ok()?,
            },
        ));
    }
    let (a, b, kind) = match rule {
        SketchConstraintRule::Horizontal { a, b } => (a, b, LineConstraintKind::Horizontal),
        SketchConstraintRule::Vertical { a, b } => (a, b, LineConstraintKind::Vertical),
        SketchConstraintRule::Distance { a, b, distance } => (
            a,
            b,
            LineConstraintKind::Distance(LineLengthMm::new(distance).ok()?),
        ),
        _ => return None,
    };
    (a.curve == b.curve && unordered(a, b) == unordered(endpoints(a.curve).0, endpoints(a.curve).1))
        .then_some((a.curve, kind))
}
/// What a request occupies: one orientation and one length per Line, and at
/// most one pinned endpoint in the whole profile, whichever Line carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Orientation(StableEntityId),
    Length(StableEntityId),
    Pin,
}
fn line_slot(curve: StableEntityId, kind: LineConstraintKind) -> Slot {
    match kind {
        LineConstraintKind::Horizontal | LineConstraintKind::Vertical => Slot::Orientation(curve),
        LineConstraintKind::Distance(_) => Slot::Length(curve),
        LineConstraintKind::Fixed { .. } => Slot::Pin,
    }
}
fn occupied(kind: LineConstraintKind) -> &'static str {
    match kind {
        LineConstraintKind::Distance(_) => {
            "a Line may hold only one length; remove its current constraint first"
        }
        LineConstraintKind::Fixed { .. } => {
            "a profile may hold only one fixed endpoint; remove the stored one in the same request"
        }
        _ => "a Line may hold only one H/V; remove its current constraint first",
    }
}

/// Preserve only this declared family of stored relationships; never simplify others.
fn managed(sketch: &Sketch) -> Result<()> {
    let curve_ids: BTreeSet<_> = sketch.curves.iter().map(|c| c.id).collect();
    let expected: BTreeSet<_> = closures(sketch).into_iter().collect();
    let mut seen_ids = BTreeSet::new();
    let mut lines = BTreeSet::new();
    let mut joins = BTreeSet::new();
    for c in &sketch.constraints {
        if !seen_ids.insert(c.id) {
            return Err(CadError::unsupported("duplicate constraint UUID"));
        }
        if let Some((curve, kind)) = line_of(c.rule) {
            if !curve_ids.contains(&curve) || !lines.insert(line_slot(curve, kind)) {
                return Err(CadError::unsupported(match kind {
                    LineConstraintKind::Fixed { .. } => {
                        "constraint edit supports at most one Fixed endpoint per profile"
                    }
                    _ => "constraint edit refuses duplicate orientation or length on a Line",
                }));
            }
        } else if let SketchConstraintRule::Coincident { a, b } = c.rule {
            let pair = unordered(a, b);
            if !expected.contains(&pair) || !joins.insert(pair) {
                return Err(CadError::unsupported(
                    "constraint edit requires unique Coincident links at adjacent Line joints",
                ));
            }
        } else {
            return Err(CadError::unsupported(
                "constraint edit supports only Line H/V, positive Start/End length, one finite Fixed Line endpoint and adjacent-joint Coincident families",
            ));
        }
    }
    if !lines.is_empty() && joins != expected {
        return Err(CadError::unsupported(
            "existing H/V, length or Fixed endpoint require all persisted Coincident closure links",
        ));
    }
    Ok(())
}

/// Validate the atomic remove-then-add request without minting IDs or solving.
fn retained(sketch: &Sketch, edits: &SketchConstraintEdits) -> Result<Vec<SketchConstraint>> {
    managed(sketch)?;
    let count = edits.remove.len().saturating_add(edits.add.len());
    if count == 0 || count > 512 {
        return Err(CadError::input(
            "constraint request requires 1..512 changes",
        ));
    }
    let mut removals = BTreeSet::new();
    for id in &edits.remove {
        if !removals.insert(*id) {
            return Err(CadError::input("duplicate constraint removal UUID"));
        }
        let c = sketch
            .constraints
            .iter()
            .find(|c| c.id == *id)
            .ok_or_else(|| {
                CadError::input(format!(
                    "constraint {id} does not belong to the selected Sketch"
                ))
            })?;
        if line_of(c.rule).is_none() {
            return Err(CadError::input(
                "only H/V, Line length or the Fixed endpoint may be removed; closure links are retained",
            ));
        }
    }
    let kept: Vec<_> = sketch
        .constraints
        .iter()
        .filter(|c| !removals.contains(&c.id))
        .copied()
        .collect();
    let mut lines: BTreeSet<_> = kept
        .iter()
        .filter_map(|c| line_of(c.rule).map(|(id, kind)| line_slot(id, kind)))
        .collect();
    for add in &edits.add {
        if !sketch.curves.iter().any(|c| c.id == add.curve) {
            return Err(CadError::input(format!(
                "curve {} does not belong to the selected Sketch",
                add.curve
            )));
        }
        if !lines.insert(line_slot(add.curve, add.kind)) {
            return Err(CadError::input(occupied(add.kind)));
        }
    }
    Ok(kept)
}
impl ConstraintSketchChoice {
    pub fn validate_edits(&self, edits: &SketchConstraintEdits) -> Result<()> {
        let sketch = self
            .stored
            .as_ref()
            .ok_or_else(|| CadError::unsupported("unsupported constraint Sketch choice"))?;
        retained(sketch, edits).map(|_| ())
    }
}

/// Owned preparation: IDs are allocated once, only after all request checks pass.
#[derive(Debug, Clone)]
pub struct PreparedSketchConstraints {
    pub(crate) object: ObjectRecord,
    pub added: Vec<SketchConstraint>,
    pub removed: Vec<StableEntityId>,
    pub height_mm: f64,
}
impl PreparedSketchConstraints {
    pub fn object(&self) -> &ObjectRecord {
        &self.object
    }
}

pub fn prepare_sketch_constraints(
    document: &Document,
    id: ObjectId,
    edits: &SketchConstraintEdits,
) -> Result<PreparedSketchConstraints> {
    let objects = document.objects()?;
    let mut object = objects
        .iter()
        .find(|o| o.id == id)
        .cloned()
        .ok_or_else(|| CadError::input("selected Sketch UUID does not exist"))?;
    let height_mm = supported(document, &objects, &object)?;
    let ObjectPayload::Sketch(sketch) = &mut object.payload else {
        unreachable!("checked")
    };
    let mut kept = retained(sketch, edits)?;
    let mut added = Vec::new();
    if !edits.add.is_empty() {
        for (i, pair) in closures(sketch).into_iter().enumerate() {
            if kept.iter().any(
                |c| matches!(c.rule,SketchConstraintRule::Coincident{a,b} if unordered(a,b)==pair),
            ) {
                continue;
            }
            let c = SketchConstraint {
                id: StableEntityId::new(),
                rule: SketchConstraintRule::Coincident {
                    a: endpoints(sketch.curves[i].id).1,
                    b: endpoints(sketch.curves[(i + 1) % sketch.curves.len()].id).0,
                },
            };
            kept.push(c);
            added.push(c);
        }
    }
    for add in &edits.add {
        let (a, b) = endpoints(add.curve);
        let rule = match add.kind {
            LineConstraintKind::Horizontal => SketchConstraintRule::Horizontal { a, b },
            LineConstraintKind::Vertical => SketchConstraintRule::Vertical { a, b },
            LineConstraintKind::Distance(length) => SketchConstraintRule::Distance {
                a,
                b,
                distance: length.get(),
            },
            LineConstraintKind::Fixed { at, x, y } => SketchConstraintRule::Fixed {
                point: SketchPointRef::new(add.curve, at.selector()),
                x: x.get(),
                y: y.get(),
            },
        };
        let c = SketchConstraint {
            id: StableEntityId::new(),
            rule,
        };
        kept.push(c);
        added.push(c);
    }
    sketch.constraints = kept;
    Ok(PreparedSketchConstraints {
        object,
        added,
        removed: edits.remove.clone(),
        height_mm,
    })
}
