// SPDX-License-Identifier: MIT
//! Bounded persisted Line orientation/length/pin/equality editing. No solver or wire format.
use crate::{
    Document, ObjectPayload, ObjectRecord, Sketch, SketchConstraint, SketchConstraintRule,
    SketchPointRef, SketchPointSelector, SketchSegmentRef,
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
/// One requested addition.
///
/// The first four families say something about one Line, so they name one. An
/// equal length is a relationship between two whole Lines and has no leading
/// side, so it names both: a pair cannot be spelled by a single curve field
/// without hiding one half of what the request means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddLineConstraint {
    /// This Line's own orientation, length, or the profile's single pinned endpoint.
    Line {
        curve: StableEntityId,
        kind: LineConstraintKind,
    },
    /// These two Lines of the same profile keep one Euclidean length between them.
    EqualLength {
        a: StableEntityId,
        b: StableEntityId,
    },
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
/// The Line a point pair spans, when it is exactly that Line's Start and End
/// in either stored orientation.
fn whole_line(a: SketchPointRef, b: SketchPointRef) -> Option<StableEntityId> {
    let (start, end) = endpoints(a.curve);
    (a.curve == b.curve && unordered(a, b) == unordered(start, end)).then_some(a.curve)
}
fn segment(curve: StableEntityId) -> SketchSegmentRef {
    let (start, end) = endpoints(curve);
    SketchSegmentRef::new(start, end)
}
/// The two distinct Lines a stored equal length relates, when both of its
/// segments are whole Lines. Stored orientation of either segment is kept as it
/// is; only what it names is read.
fn equal_of(rule: SketchConstraintRule) -> Option<(StableEntityId, StableEntityId)> {
    let SketchConstraintRule::EqualLength { a, b } = rule else {
        return None;
    };
    let (x, y) = (whole_line(a.from, a.to)?, whole_line(b.from, b.to)?);
    (x != y).then_some((x, y))
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
    whole_line(a, b).map(|curve| (curve, kind))
}
/// What a request occupies: one orientation and one length per Line, at most
/// one pinned endpoint in the whole profile, whichever Line carries it, and one
/// equal length per unordered pair of Lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Orientation(StableEntityId),
    Length(StableEntityId),
    Pin,
    /// Sorted, so (A,B) and (B,A) are the same occupied slot.
    Equal(StableEntityId, StableEntityId),
}
fn line_slot(curve: StableEntityId, kind: LineConstraintKind) -> Slot {
    match kind {
        LineConstraintKind::Horizontal | LineConstraintKind::Vertical => Slot::Orientation(curve),
        LineConstraintKind::Distance(_) => Slot::Length(curve),
        LineConstraintKind::Fixed { .. } => Slot::Pin,
    }
}
fn equal_slot(a: StableEntityId, b: StableEntityId) -> Slot {
    if a <= b {
        Slot::Equal(a, b)
    } else {
        Slot::Equal(b, a)
    }
}
/// The slot a stored rule occupies and every Line it names, or `None` when the
/// rule is not one this family manages.
fn slot_of(rule: SketchConstraintRule) -> Option<(Slot, [StableEntityId; 2])> {
    if let Some((a, b)) = equal_of(rule) {
        return Some((equal_slot(a, b), [a, b]));
    }
    let (curve, kind) = line_of(rule)?;
    Some((line_slot(curve, kind), [curve, curve]))
}
/// The slot a requested addition occupies, refusing a pair that is not one.
fn requested(add: &AddLineConstraint) -> Result<(Slot, [StableEntityId; 2])> {
    Ok(match *add {
        AddLineConstraint::Line { curve, kind } => (line_slot(curve, kind), [curve, curve]),
        AddLineConstraint::EqualLength { a, b } => {
            if a == b {
                return Err(CadError::input(
                    "equal length relates two different Lines; this addition names one twice",
                ));
            }
            (equal_slot(a, b), [a, b])
        }
    })
}
fn occupied(slot: Slot) -> &'static str {
    match slot {
        Slot::Length(_) => "a Line may hold only one length; remove its current constraint first",
        Slot::Pin => {
            "a profile may hold only one fixed endpoint; remove the stored one in the same request"
        }
        Slot::Equal(..) => {
            "these two Lines already hold an equal length; remove it in the same request"
        }
        Slot::Orientation(_) => "a Line may hold only one H/V; remove its current constraint first",
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
        if let Some((slot, curves)) = slot_of(c.rule) {
            if !curves.iter().all(|c| curve_ids.contains(c)) || !lines.insert(slot) {
                return Err(CadError::unsupported(match slot {
                    Slot::Pin => "constraint edit supports at most one Fixed endpoint per profile",
                    Slot::Equal(..) => {
                        "constraint edit refuses a duplicate equal length on one pair of Lines"
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
                "constraint edit supports only Line H/V, positive Start/End length, one finite Fixed Line endpoint, equal length between two whole Lines and adjacent-joint Coincident families",
            ));
        }
    }
    if !lines.is_empty() && joins != expected {
        return Err(CadError::unsupported(
            "existing H/V, length, equal length or Fixed endpoint require all persisted Coincident closure links",
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
        if slot_of(c.rule).is_none() {
            return Err(CadError::input(
                "only H/V, Line length, equal length or the Fixed endpoint may be removed; closure links are retained",
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
        .filter_map(|c| slot_of(c.rule).map(|(slot, _)| slot))
        .collect();
    for add in &edits.add {
        let (slot, curves) = requested(add)?;
        for curve in curves {
            if !sketch.curves.iter().any(|c| c.id == curve) {
                return Err(CadError::input(format!(
                    "curve {curve} does not belong to the selected Sketch"
                )));
            }
        }
        if !lines.insert(slot) {
            return Err(CadError::input(occupied(slot)));
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
        let rule = match *add {
            AddLineConstraint::Line { curve, kind } => {
                let (a, b) = endpoints(curve);
                match kind {
                    LineConstraintKind::Horizontal => SketchConstraintRule::Horizontal { a, b },
                    LineConstraintKind::Vertical => SketchConstraintRule::Vertical { a, b },
                    LineConstraintKind::Distance(length) => SketchConstraintRule::Distance {
                        a,
                        b,
                        distance: length.get(),
                    },
                    LineConstraintKind::Fixed { at, x, y } => SketchConstraintRule::Fixed {
                        point: SketchPointRef::new(curve, at.selector()),
                        x: x.get(),
                        y: y.get(),
                    },
                }
            }
            AddLineConstraint::EqualLength { a, b } => SketchConstraintRule::EqualLength {
                a: segment(a),
                b: segment(b),
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
