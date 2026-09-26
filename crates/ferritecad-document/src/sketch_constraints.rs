// SPDX-License-Identifier: MIT
//! Bounded persisted Line orientation/length/pin/equality/relation editing. No solver or wire format.
use crate::{
    CircleExtrusion, Document, ObjectPayload, ObjectRecord, Sketch, SketchConstraint,
    SketchConstraintRule, SketchGeometry, SketchPointRef, SketchPointSelector, SketchProfileUse,
    SketchSegmentRef,
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
/// Which relative orientation two Lines keep.
///
/// This says nothing about either Line's own direction, which is what makes it
/// different from `Horizontal`/`Vertical`: it holds on a profile at any angle,
/// and the profile is free to rotate while it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineRelation {
    Parallel,
    Perpendicular,
}
impl LineRelation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parallel => "Parallel",
            Self::Perpendicular => "Perpendicular",
        }
    }
}

/// One requested addition.
///
/// The first four families say something about one Line, so they name one. An
/// equal length and a relative orientation are relationships between two whole
/// Lines and have no leading side, so they name both: a pair cannot be spelled
/// by a single curve field without hiding one half of what the request means.
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
    /// These two Lines of the same profile keep one relative orientation.
    Relation {
        a: StableEntityId,
        b: StableEntityId,
        relation: LineRelation,
    },
}
/// A finite, strictly positive circle radius in millimetres.
///
/// Its own type beside [`LineLengthMm`] rather than that one reused: a length
/// between two points and the radius of a circle are different quantities of
/// the same unit, and a request that could spell one where the other belongs
/// would be one field away from giving a circle a line's dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircleRadiusMm(u64);
impl CircleRadiusMm {
    pub fn new(value: f64) -> Result<Self> {
        if !value.is_finite() || value <= 0. {
            return Err(CadError::input(
                "circle radius must be finite and positive in mm",
            ));
        }
        Ok(Self(value.to_bits()))
    }
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// What one addition says about one circle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircleConstraintKind {
    /// This circle's own radius in mm.
    Radius(CircleRadiusMm),
    /// This circle's centre pinned at explicit millimetres.
    FixedCenter {
        x: SketchCoordinateMm,
        y: SketchCoordinateMm,
    },
}

/// One requested addition, whichever geometry it is about.
///
/// Split by geometry rather than widened: [`AddLineConstraint`] keeps saying
/// exactly what it always said about lines, and a circle addition cannot be
/// spelled in its vocabulary by accident. The two families never mix in one
/// sketch — a profile this editor manages is all lines or one circle — so the
/// split is also what the request is really made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddSketchConstraint {
    Line(AddLineConstraint),
    /// This circle's radius, or its pinned centre.
    Circle {
        curve: StableEntityId,
        kind: CircleConstraintKind,
    },
    /// These two circles of the same profile keep one centre.
    ///
    /// Two named circles rather than one leading circle and an implied other:
    /// concentricity is a relationship, it has no side that owns it, and a
    /// request that named one circle would be deciding on the reader's behalf
    /// which second circle it meant.
    Concentric {
        a: StableEntityId,
        b: StableEntityId,
    },
}

impl From<AddLineConstraint> for AddSketchConstraint {
    fn from(line: AddLineConstraint) -> Self {
        Self::Line(line)
    }
}

/// Which geometry a managed profile is made of.
///
/// Read from the curves and from nothing else. The two families take different
/// constraints, different slots and different closure rules, and deciding
/// which is which once is what keeps every one of those from being guessed
/// again further down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Lines,
    Circle,
    /// Two analytic circles, one inside the other: the annular profile of
    /// [§25L][crate::AnnularExtrusion], now with dimensions of its own.
    Annulus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SketchConstraintEdits {
    pub remove: Vec<StableEntityId>,
    pub add: Vec<AddSketchConstraint>,
}

/// Same objects()/content-version snapshot as coordinate discovery; independent eligibility.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstraintSketchChoice {
    pub sketch: ObjectId,
    pub name: Option<String>,
    pub stored: Option<Sketch>,
    /// The extrusion height, for a profile an Extrude uses; `None` for a
    /// Revolve profile, which has no height, and for an unsupported Sketch.
    pub height_mm: Option<f64>,
    /// §27G: the feature that turns this profile into a solid, and so the
    /// policy its solved drawing must satisfy. Present exactly when `stored`
    /// is.
    pub profile_use: Option<SketchProfileUse>,
    pub refusal: Option<String>,
}

pub fn constraint_sketch_choices(
    document: &Document,
    objects: &[ObjectRecord],
) -> Vec<ConstraintSketchChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .map(|o| match supported_family(document, objects, o) {
            Ok((_, profile_use)) => {
                let ObjectPayload::Sketch(sketch) = &o.payload else {
                    unreachable!("checked")
                };
                ConstraintSketchChoice {
                    sketch: o.id,
                    name: o.name.clone(),
                    stored: Some(sketch.clone()),
                    height_mm: extrusion_height(&profile_use),
                    profile_use: Some(profile_use),
                    refusal: None,
                }
            }
            Err(e) => ConstraintSketchChoice {
                sketch: o.id,
                name: o.name.clone(),
                stored: None,
                height_mm: None,
                profile_use: None,
                refusal: Some(e.to_string()),
            },
        })
        .collect()
}

fn extrusion_height(profile_use: &SketchProfileUse) -> Option<f64> {
    match *profile_use {
        SketchProfileUse::BlindExtrude { height_mm, .. } => Some(height_mm),
        _ => None,
    }
}

/// The frame, the geometry family and the owning feature, all from one reading.
fn supported_family(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
) -> Result<(Family, SketchProfileUse)> {
    // The frame every copy edit of a saved profile requires is checked once,
    // in the one place that owns it, before either family is considered. It
    // also says which feature uses the profile (§27G): an Extrude and its
    // height, or a Revolve with a bore and its stated turn.
    let (sketch, profile_use) = crate::sketch_edit::constraint_frame(document, objects, object)?;
    let family = classify(sketch)?;
    let height = extrusion_height(&profile_use);
    match (family, height) {
        // The Line editor's own class, checked by its own code against the
        // policy of the feature that uses it — the same check an Extrude
        // profile always had, and the Revolve policy for a turned one.
        (Family::Lines, _) => {
            crate::sketch_edit::constrained_lines(sketch, &profile_use)?;
        }
        (Family::Circle | Family::Annulus, None) => {
            return Err(CadError::unsupported(
                "a Revolve profile is Lines; constraint editing of Circles is supported only on \
                 an extruded profile",
            ));
        }
        (Family::Circle, Some(height)) => {
            let (_, center, radius) = crate::circle_edit::analytic_circle(&sketch.curves[0])?;
            // The stored geometry has to be inside the policy new numbers are
            // judged by, and it is the *same* policy: one numeric rule, asked
            // here, in the request check and at creation alike.
            CircleExtrusion::new([center.x, center.y], radius, height).map_err(|e| {
                CadError::unsupported(format!("saved circle is outside edit policy: {e}"))
            })?;
        }
        (Family::Annulus, Some(height)) => {
            // The same reading, the same roles and the same numeric policy the
            // annulus editor applies to a saved pair — asked here through the
            // one function that owns them, so a document the two editors both
            // see cannot be two different drawings.
            crate::annulus_edit::saved_pair(sketch, height)?;
        }
    }
    managed(sketch, family)?;
    Ok((family, profile_use))
}

/// Which family this sketch's curves make it, or why it is neither.
fn classify(sketch: &Sketch) -> Result<Family> {
    if sketch
        .curves
        .iter()
        .all(|c| matches!(c.geometry, SketchGeometry::Line { .. }))
        && !sketch.curves.is_empty()
    {
        return Ok(Family::Lines);
    }
    if let [only] = sketch.curves.as_slice()
        && matches!(only.geometry, SketchGeometry::Circle { .. })
    {
        return Ok(Family::Circle);
    }
    // Two of them are the annular profile. Which is the boundary and which is
    // the bore is not decided here: that is read from the radii, once, where
    // the annulus editor already reads it.
    if let [a, b] = sketch.curves.as_slice()
        && matches!(a.geometry, SketchGeometry::Circle { .. })
        && matches!(b.geometry, SketchGeometry::Circle { .. })
    {
        return Ok(Family::Annulus);
    }
    Err(CadError::unsupported(
        "constraint edit supports a profile of Lines, one analytic Circle, or two making an \
         annulus",
    ))
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
/// The two distinct Lines a stored pair relationship relates, when both of its
/// segments are whole Lines. Stored orientation of either segment is kept as it
/// is; only what it names is read.
fn related(a: SketchSegmentRef, b: SketchSegmentRef) -> Option<(StableEntityId, StableEntityId)> {
    let (x, y) = (whole_line(a.from, a.to)?, whole_line(b.from, b.to)?);
    (x != y).then_some((x, y))
}
fn equal_of(rule: SketchConstraintRule) -> Option<(StableEntityId, StableEntityId)> {
    let SketchConstraintRule::EqualLength { a, b } = rule else {
        return None;
    };
    related(a, b)
}
/// The two distinct Lines a stored Parallel or Perpendicular relates. Which of
/// the two it is does not change the slot: they are two answers to one question.
fn relation_of(rule: SketchConstraintRule) -> Option<(StableEntityId, StableEntityId)> {
    match rule {
        SketchConstraintRule::Parallel { a, b } | SketchConstraintRule::Perpendicular { a, b } => {
            related(a, b)
        }
        _ => None,
    }
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
/// one pinned endpoint in the whole profile, whichever Line carries it, one
/// equal length per unordered pair of Lines, and one relative orientation per
/// unordered pair of Lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Orientation(StableEntityId),
    Length(StableEntityId),
    Pin,
    /// Sorted, so (A,B) and (B,A) are the same occupied slot.
    Equal(StableEntityId, StableEntityId),
    /// Sorted for the same reason. Parallel and Perpendicular answer one
    /// question about the pair, so they share this slot; an equal length on the
    /// same pair is a different question and has its own.
    Relation(StableEntityId, StableEntityId),
    /// The one radius of one circle.
    ///
    /// Its own slot rather than [`Slot::Length`] reused: the two are different
    /// dimensions of different geometry, and sharing a slot would make a
    /// document's refusal message name a Line where a circle stands.
    Radius(StableEntityId),
    /// The one shared centre of an unordered pair of circles.
    ///
    /// Sorted, so naming the boundary first and naming the bore first occupy
    /// the same slot: they are one relationship asked twice, and a profile that
    /// stored both would be saying the same thing to the solver twice.
    Concentric(StableEntityId, StableEntityId),
}
fn line_slot(curve: StableEntityId, kind: LineConstraintKind) -> Slot {
    match kind {
        LineConstraintKind::Horizontal | LineConstraintKind::Vertical => Slot::Orientation(curve),
        LineConstraintKind::Distance(_) => Slot::Length(curve),
        LineConstraintKind::Fixed { .. } => Slot::Pin,
    }
}
fn sorted(a: StableEntityId, b: StableEntityId) -> (StableEntityId, StableEntityId) {
    if a <= b { (a, b) } else { (b, a) }
}
fn equal_slot(a: StableEntityId, b: StableEntityId) -> Slot {
    let (a, b) = sorted(a, b);
    Slot::Equal(a, b)
}
fn relation_slot(a: StableEntityId, b: StableEntityId) -> Slot {
    let (a, b) = sorted(a, b);
    Slot::Relation(a, b)
}
fn concentric_slot(a: StableEntityId, b: StableEntityId) -> Slot {
    let (a, b) = sorted(a, b);
    Slot::Concentric(a, b)
}
/// The centre of a circle, as a point of the sketch.
fn center(curve: StableEntityId) -> SketchPointRef {
    SketchPointRef::new(curve, SketchPointSelector::Center)
}
/// The two distinct circles a stored `Coincident` makes concentric, when that
/// is what it says.
///
/// A `Coincident` is concentricity only when **both** of its points are centres
/// and they belong to different curves. Every other `Coincident` — in
/// particular the closure link between two Line endpoints — is not this, is not
/// read as this, and stays exactly as protected as it was.
fn concentric_of(rule: SketchConstraintRule) -> Option<(StableEntityId, StableEntityId)> {
    let SketchConstraintRule::Coincident { a, b } = rule else {
        return None;
    };
    (a.at == SketchPointSelector::Center
        && b.at == SketchPointSelector::Center
        && a.curve != b.curve)
        .then_some((a.curve, b.curve))
}
/// The slot a stored circle rule occupies, and the circle it names.
///
/// A pinned centre takes the profile-wide [`Slot::Pin`], the same slot a pinned
/// Line endpoint takes: a profile holds one pin whichever geometry carries it,
/// and the two families never share a profile anyway.
fn circle_slot_of(rule: SketchConstraintRule) -> Option<(Slot, StableEntityId)> {
    match rule {
        SketchConstraintRule::Radius { curve, .. } => Some((Slot::Radius(curve), curve)),
        SketchConstraintRule::Fixed { point, .. } if point.at == SketchPointSelector::Center => {
            Some((Slot::Pin, point.curve))
        }
        _ => None,
    }
}

/// The slot a stored rule occupies on an annular profile, and every circle it
/// names.
///
/// The single-circle vocabulary plus concentricity, which only a pair can hold.
/// Written as its own reading rather than as [`circle_slot_of`] widened: that
/// one answers for a profile with no second circle to name, and a `Coincident`
/// there could only ever be a link this editor does not manage.
fn annulus_slot_of(rule: SketchConstraintRule) -> Option<(Slot, [StableEntityId; 2])> {
    if let Some((a, b)) = concentric_of(rule) {
        return Some((concentric_slot(a, b), [a, b]));
    }
    circle_slot_of(rule).map(|(slot, curve)| (slot, [curve, curve]))
}

/// The slot a stored rule occupies and every Line it names, or `None` when the
/// rule is not one this family manages.
fn slot_of(rule: SketchConstraintRule) -> Option<(Slot, [StableEntityId; 2])> {
    if let Some((a, b)) = equal_of(rule) {
        return Some((equal_slot(a, b), [a, b]));
    }
    if let Some((a, b)) = relation_of(rule) {
        return Some((relation_slot(a, b), [a, b]));
    }
    let (curve, kind) = line_of(rule)?;
    Some((line_slot(curve, kind), [curve, curve]))
}
/// The slot a requested addition occupies, refusing a pair that is not one.
fn requested(add: &AddSketchConstraint) -> Result<(Slot, [StableEntityId; 2])> {
    let add = match *add {
        AddSketchConstraint::Circle { curve, kind } => {
            return Ok((
                match kind {
                    CircleConstraintKind::Radius(_) => Slot::Radius(curve),
                    CircleConstraintKind::FixedCenter { .. } => Slot::Pin,
                },
                [curve, curve],
            ));
        }
        AddSketchConstraint::Concentric { a, b } => {
            if a == b {
                return Err(CadError::input(
                    "concentricity relates two different Circles; this addition names one twice",
                ));
            }
            return Ok((concentric_slot(a, b), [a, b]));
        }
        AddSketchConstraint::Line(line) => line,
    };
    Ok(match add {
        AddLineConstraint::Line { curve, kind } => (line_slot(curve, kind), [curve, curve]),
        AddLineConstraint::EqualLength { a, b } => {
            if a == b {
                return Err(CadError::input(
                    "equal length relates two different Lines; this addition names one twice",
                ));
            }
            (equal_slot(a, b), [a, b])
        }
        AddLineConstraint::Relation { a, b, .. } => {
            if a == b {
                return Err(CadError::input(
                    "a relative orientation relates two different Lines; this addition names one twice",
                ));
            }
            (relation_slot(a, b), [a, b])
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
        Slot::Relation(..) => {
            "these two Lines already hold a Parallel or Perpendicular; remove it in the same request"
        }
        Slot::Orientation(_) => "a Line may hold only one H/V; remove its current constraint first",
        Slot::Radius(_) => "a circle may hold only one radius; remove its current constraint first",
        Slot::Concentric(..) => {
            "these two circles already share a centre; remove that constraint in the same request"
        }
    }
}

/// Preserve only this declared family of stored relationships; never simplify others.
fn managed(sketch: &Sketch, family: Family) -> Result<()> {
    match family {
        Family::Circle | Family::Annulus => return managed_circles(sketch, family),
        Family::Lines => {}
    }
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
                    Slot::Relation(..) => {
                        "constraint edit refuses more than one Parallel or Perpendicular on one pair of Lines"
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
                "constraint edit supports only Line H/V, positive Start/End length, one finite Fixed Line endpoint, equal length or Parallel/Perpendicular between two whole Lines and adjacent-joint Coincident families",
            ));
        }
    }
    if !lines.is_empty() && joins != expected {
        return Err(CadError::unsupported(
            "existing H/V, length, equal length, Parallel/Perpendicular or Fixed endpoint require all persisted Coincident closure links",
        ));
    }
    Ok(())
}

/// The circle families' own version of the same rule.
///
/// A circle has no joints, so there is no closure to require and no Coincident
/// to preserve for that reason: what a managed circle profile may hold is one
/// radius per circle and one pinned centre, and — when there are two circles —
/// one concentricity between them. A `Coincident` here is therefore either that
/// concentricity or nothing this editor manages; the Line closure it protects
/// elsewhere cannot occur on a profile with no Line in it.
fn managed_circles(sketch: &Sketch, family: Family) -> Result<()> {
    let curves: BTreeSet<_> = sketch.curves.iter().map(|c| c.id).collect();
    let mut seen_ids = BTreeSet::new();
    let mut slots = BTreeSet::new();
    for c in &sketch.constraints {
        if !seen_ids.insert(c.id) {
            return Err(CadError::unsupported("duplicate constraint UUID"));
        }
        let Some((slot, named)) = family_slot_of(family, c.rule) else {
            return Err(CadError::unsupported(match family {
                Family::Annulus => {
                    "constraint edit supports only a positive radius per Circle, one fixed centre \
                     and one concentricity on an annular profile"
                }
                _ => {
                    "constraint edit supports only one positive radius and one fixed centre on an \
                     analytic Circle"
                }
            }));
        };
        if !named.iter().all(|id| curves.contains(id)) || !slots.insert(slot) {
            return Err(CadError::unsupported(match slot {
                Slot::Pin => "constraint edit supports at most one fixed centre per circle",
                Slot::Concentric(..) => {
                    "constraint edit supports at most one concentricity per pair of Circles"
                }
                _ => "constraint edit refuses more than one radius on a circle",
            }));
        }
    }
    Ok(())
}

/// Which stored rules a family manages, decided once for every caller.
fn family_slot_of(
    family: Family,
    rule: SketchConstraintRule,
) -> Option<(Slot, [StableEntityId; 2])> {
    match family {
        Family::Lines => slot_of(rule),
        Family::Circle => circle_slot_of(rule).map(|(slot, curve)| (slot, [curve, curve])),
        Family::Annulus => annulus_slot_of(rule),
    }
}

/// Validate the atomic remove-then-add request without minting IDs or solving.
fn retained(
    sketch: &Sketch,
    family: Family,
    edits: &SketchConstraintEdits,
) -> Result<Vec<SketchConstraint>> {
    managed(sketch, family)?;
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
        if family_slot_of(family, c.rule).is_none() {
            return Err(CadError::input(
                "only H/V, Line length, equal length, Parallel/Perpendicular, the Fixed endpoint, a circle radius, a fixed centre or a concentricity may be removed; Line closure links are retained",
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
        .filter_map(|c| family_slot_of(family, c.rule).map(|(slot, _)| slot))
        .collect();
    for add in &edits.add {
        let (slot, curves) = requested(add)?;
        // An addition has to speak the family's own vocabulary. A radius asked
        // of a Line profile and an H/V asked of a circle are both requests for
        // geometry that is not there, and saying so beats storing a constraint
        // the solver would then be asked to make sense of.
        let fits = matches!(
            (family, add),
            (Family::Lines, AddSketchConstraint::Line(_))
                | (
                    Family::Circle | Family::Annulus,
                    AddSketchConstraint::Circle { .. }
                )
                | (Family::Annulus, AddSketchConstraint::Concentric { .. })
        );
        if !fits {
            return Err(CadError::input(match family {
                Family::Lines => "this Sketch is a Line profile and takes no circle constraint",
                Family::Circle => {
                    "this Sketch is one analytic Circle: it takes no Line constraint, and one \
                     circle has nothing to be concentric with"
                }
                Family::Annulus => {
                    "this Sketch is two analytic Circles and takes no Line constraint"
                }
            }));
        }
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
        let family = classify(sketch)?;
        retained(sketch, family, edits).map(|_| ())
    }
}

/// Owned preparation: IDs are allocated once, only after all request checks pass.
#[derive(Debug, Clone)]
pub struct PreparedSketchConstraints {
    pub(crate) object: ObjectRecord,
    pub added: Vec<SketchConstraint>,
    pub removed: Vec<StableEntityId>,
    /// The feature that uses the profile, read with the frame: what the
    /// solved drawing must satisfy before publication. Private, like the
    /// payload, so no caller can swap the policy a copy is judged by.
    profile_use: SketchProfileUse,
    roles: Option<(StableEntityId, StableEntityId)>,
}
impl PreparedSketchConstraints {
    pub fn object(&self) -> &ObjectRecord {
        &self.object
    }

    /// The feature that turns the profile into a solid (§27G).
    pub fn profile_use(&self) -> SketchProfileUse {
        self.profile_use
    }

    /// The boundary and the bore of the **saved** profile, in that order, or
    /// `None` for a profile that is not an annulus.
    ///
    /// Read from the saved radii before anything is solved, and carried here so
    /// the rebuild can check the solved drawing still holds the same two roles
    /// under the same two UUIDs. Re-reading it from the solved answer instead
    /// would legalise a swap: two circles that exchanged sizes would look like
    /// a perfectly good annulus whose roles had simply always been the other
    /// way round.
    pub fn circle_roles(&self) -> Option<(StableEntityId, StableEntityId)> {
        self.roles
    }
}

/// Which of a managed profile's circles bounds the part and which is the bore,
/// or `None` when the profile is not two circles.
///
/// The catalogue's answer for a reader — `inspect`, and the form in the app —
/// so neither has to decide roles for itself. It is the same reading the editor
/// uses, asked of the same stored sketch.
pub fn constraint_circle_roles(sketch: &Sketch) -> Option<(StableEntityId, StableEntityId)> {
    (classify(sketch).ok()? == Family::Annulus)
        .then(|| crate::annulus_edit::pair_in_roles(sketch).ok())
        .flatten()
        .map(|(outer, inner)| (outer.0, inner.0))
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
    let (family, profile_use) = supported_family(document, &objects, &object)?;
    let ObjectPayload::Sketch(sketch) = &mut object.payload else {
        unreachable!("checked")
    };
    let mut kept = retained(sketch, family, edits)?;
    let mut added = Vec::new();
    // A circle has no joints to close, so nothing is appended on its behalf.
    if family == Family::Lines && !edits.add.is_empty() {
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
        let line = match *add {
            AddSketchConstraint::Concentric { a, b } => {
                // The existing Coincident of two points, which is what "one
                // shared centre" already means: a circle's centre is a point of
                // the sketch, so there is nothing else to say and no second way
                // to compute a centre.
                let c = SketchConstraint {
                    id: StableEntityId::new(),
                    rule: SketchConstraintRule::Coincident {
                        a: center(a),
                        b: center(b),
                    },
                };
                kept.push(c);
                added.push(c);
                continue;
            }
            AddSketchConstraint::Circle { curve, kind } => {
                let rule = match kind {
                    CircleConstraintKind::Radius(radius) => SketchConstraintRule::Radius {
                        curve,
                        radius: radius.get(),
                    },
                    CircleConstraintKind::FixedCenter { x, y } => SketchConstraintRule::Fixed {
                        point: SketchPointRef::new(curve, SketchPointSelector::Center),
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
                continue;
            }
            AddSketchConstraint::Line(line) => line,
        };
        let rule = match line {
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
            AddLineConstraint::Relation { a, b, relation } => {
                let (a, b) = (segment(a), segment(b));
                match relation {
                    LineRelation::Parallel => SketchConstraintRule::Parallel { a, b },
                    LineRelation::Perpendicular => SketchConstraintRule::Perpendicular { a, b },
                }
            }
        };
        let c = SketchConstraint {
            id: StableEntityId::new(),
            rule,
        };
        kept.push(c);
        added.push(c);
    }
    sketch.constraints = kept;
    // The saved roles, read before this edit is written and from the saved
    // radii alone.
    let roles = (family == Family::Annulus)
        .then(|| crate::annulus_edit::pair_in_roles(sketch))
        .transpose()?
        .map(|(outer, inner)| (outer.0, inner.0));
    Ok(PreparedSketchConstraints {
        object,
        added,
        removed: edits.remove.clone(),
        profile_use,
        roles,
    })
}
