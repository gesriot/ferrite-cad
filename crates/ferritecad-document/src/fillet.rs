// SPDX-License-Identifier: MIT
//! Rounding one vertical edge of a saved rectangular plate, in a new copy
//! (§28A).
//!
//! The same history model a Cut uses (ADR 0004): the new Fillet names the
//! feature whose result it rounds, the Body names its tip, and ownership is
//! derived. The supported source is the exact plate the Cut editors read
//! through [`crate::cut_edit::saved_history`], with no Cut, and a profile that
//! is literally an axis-aligned rectangle.
//!
//! The edge is chosen by what it means — the base Extrude and the corner of
//! its profile, named by the unordered pair of the two Line UUIDs that meet
//! there — never by an index or a position. The radius is judged by one
//! measured, conservative policy ([`MIN_RADIUS_MM`], [`MAX_RADIUS_FRACTION`]),
//! applied by discovery, preparation, the writer's re-derivation and the
//! evaluator alike. Nothing is clamped.
use ferritecad_types::{CadError, ObjectId, ProfileJoint, Result, StableEntityId};

use crate::cut_boundary::CutBoundary;
use crate::cut_edit::NewObject;
use crate::{
    Body, CapSide, Dependency, DependencyRole, Document, EntityKind, Fillet, FilletEdge,
    ObjectPayload, ObjectRecord, SelectionRule, SemanticRole, Sketch, SketchGeometry, TopologyRef,
};

/// The smallest radius this build rounds an edge to, in millimetres.
///
/// Measured on Open CASCADE 8.0.1: at 1e-7 mm the builder returns a "valid"
/// solid that removes no measurable material, and at 1e-5 mm the removed
/// volume is off by 1.4e-3 relative; from 0.01 mm it agrees with the
/// analytic value to 5e-10. 0.01 mm is 10⁵ times the kernel's linear
/// tolerance.
pub const MIN_RADIUS_MM: f64 = 0.01;

/// The largest radius, as a fraction of the shorter Line meeting at the
/// corner.
///
/// Open CASCADE stops at the shorter side itself (measured: done at 0.996 of
/// it, not done at 1.0). Half of it keeps at least half of each adjacent side
/// flat, well away from the degenerate limit, so neither neighbouring corner
/// and no other face can be reached. Chosen conservatively; never widened to
/// make an example pass.
pub const MAX_RADIUS_FRACTION: f64 = 0.5;

fn unsupported(message: impl Into<String>) -> CadError {
    CadError::unsupported(message)
}

/// One vertical edge of the plate a Fillet could round.
///
/// `joint` is the identity. The rest are labels a person can read — where the
/// corner is, how long its two sides are, how large a radius fits — and are
/// recomputed from the saved profile every time; none of them names the edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilletCorner {
    /// The feature that swept the edge: the plate's base Extrude.
    pub feature: ObjectId,
    pub joint: ProfileJoint,
    /// The corner of the profile the edge was swept from, in sketch mm.
    pub corner_mm: [f64; 2],
    /// The lengths of the two Lines meeting there, in the joint's canonical
    /// order.
    pub adjacent_lengths_mm: [f64; 2],
    /// The largest radius this build accepts here.
    pub max_radius_mm: f64,
}

impl FilletCorner {
    /// The radius policy, with the numbers in the refusal.
    pub fn check_radius(&self, radius_mm: f64) -> Result<()> {
        if !radius_mm.is_finite() {
            return Err(CadError::input(format!(
                "a fillet radius must be a finite number of millimetres, found {radius_mm}"
            )));
        }
        if radius_mm < MIN_RADIUS_MM {
            return Err(CadError::input(format!(
                "a fillet radius must be at least {MIN_RADIUS_MM} mm, found {radius_mm} mm"
            )));
        }
        if radius_mm > self.max_radius_mm {
            return Err(CadError::input(format!(
                "a fillet of {radius_mm} mm at corner {} is too large: this build rounds up to \
                 {MAX_RADIUS_FRACTION} × the shorter adjacent side ({} mm), which is {} mm; \
                 nothing is clamped",
                self.joint,
                self.adjacent_lengths_mm[0].min(self.adjacent_lengths_mm[1]),
                self.max_radius_mm
            )));
        }
        Ok(())
    }
}

/// The four corners of a profile that is an axis-aligned rectangle of four
/// Lines, in stored segment order: corner `j` is where Line `j - 1` ends and
/// Line `j` starts.
///
/// Refuses anything else with its reason. Shared by discovery, preparation and
/// the evaluator, so all three agree about which edges exist.
pub fn rectangle_corners(feature: ObjectId, sketch: &Sketch) -> Result<Vec<FilletCorner>> {
    if !sketch.constraints.is_empty() {
        return Err(unsupported(
            "this slice rounds an edge of an unconstrained profile; the profile carries \
             constraints",
        ));
    }
    let lines: Vec<(StableEntityId, [f64; 2], [f64; 2])> = sketch
        .curves
        .iter()
        .map(|c| match c.geometry {
            SketchGeometry::Line { start, end } if !c.construction => {
                Ok((c.id, [start.x, start.y], [end.x, end.y]))
            }
            _ => Err(unsupported(
                "this slice rounds an edge of a profile of four Lines, and this profile holds \
                 something else",
            )),
        })
        .collect::<Result<_>>()?;
    let boundary = CutBoundary::read(&sketch.curves, 1.0)?;
    if lines.len() != 4 || boundary.rectangle_mm().is_none() {
        return Err(unsupported(
            "this slice rounds a vertical edge of an axis-aligned rectangular plate, and this \
             profile is not an axis-aligned rectangle",
        ));
    }
    let length =
        |(_, a, b): &(StableEntityId, [f64; 2], [f64; 2])| (b[0] - a[0]).hypot(b[1] - a[1]);
    (0..lines.len())
        .map(|j| {
            let before = &lines[(j + lines.len() - 1) % lines.len()];
            let after = &lines[j];
            let joint = ProfileJoint::new(before.0, after.0)?;
            let [first, _] = joint.segments();
            let lengths = if first == before.0 {
                [length(before), length(after)]
            } else {
                [length(after), length(before)]
            };
            Ok(FilletCorner {
                feature,
                joint,
                corner_mm: after.1,
                adjacent_lengths_mm: lengths,
                max_radius_mm: MAX_RADIUS_FRACTION * lengths[0].min(lengths[1]),
            })
        })
        .collect()
}

/// The corner a stated edge means, or why it means none.
pub fn corner_for(corners: &[FilletCorner], edge: FilletEdge) -> Result<FilletCorner> {
    let Some(first) = corners.first() else {
        return Err(unsupported("this profile has no corner to round"));
    };
    if edge.feature != first.feature {
        return Err(CadError::input(format!(
            "the edge names feature {}, and this slice rounds an edge of the base Extrude {}",
            edge.feature, first.feature
        )));
    }
    corners
        .iter()
        .find(|c| c.joint == edge.joint)
        .copied()
        .ok_or_else(|| {
            CadError::input(format!(
                "{} is not a corner of this profile: the two Lines must be adjacent Lines of \
                 the base Extrude's profile",
                edge.joint
            ))
        })
}

/// A saved Body a Fillet can be added to, exactly as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedFilletTarget {
    pub body: ObjectId,
    /// The base Extrude: both the feature consumed and the one whose edge is
    /// rounded.
    pub base_feature: ObjectId,
    pub profile: ObjectId,
    pub profile_segments: Vec<StableEntityId>,
    pub height_mm: f64,
    /// The four candidates, in stored segment order.
    pub corners: Vec<FilletCorner>,
}

/// A Body row of discovery: a target, or the reason it is not one.
#[derive(Debug, Clone, PartialEq)]
pub struct FilletChoice {
    pub body: ObjectId,
    pub name: Option<String>,
    pub target: Option<SavedFilletTarget>,
    pub refusal: Option<String>,
}

impl FilletChoice {
    /// The same check preparation applies, with no SQLite or kernel work.
    pub fn validate(&self, fillet: &EdgeFillet) -> Result<FilletCorner> {
        let target = self
            .target
            .as_ref()
            .ok_or_else(|| unsupported(self.refusal.clone().unwrap_or_default()))?;
        let corner = corner_for(&target.corners, fillet.edge)?;
        corner.check_radius(fillet.radius_mm)?;
        Ok(corner)
    }
}

/// What one fillet asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeFillet {
    pub edge: FilletEdge,
    pub radius_mm: f64,
}

/// Why a document that already holds a Fillet is not a target for anything
/// this build edits. One sentence, used by every editor that reads the plate.
pub(crate) fn refuse_filleted(objects: &[ObjectRecord]) -> Result<()> {
    if let Some(fillet) = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
    {
        return Err(unsupported(format!(
            "this Body ends in Fillet {} (§28A); editing a filleted part, and adding a second \
             Fillet or a Cut after one, are not supported yet",
            fillet.id
        )));
    }
    Ok(())
}

fn saved_target(
    document: &Document,
    objects: &[ObjectRecord],
    body: ObjectId,
) -> Result<SavedFilletTarget> {
    refuse_filleted(objects)?;
    let history = crate::cut_edit::saved_history(document, objects)?;
    let target = history.target_for_fillet(body)?;
    let profile = objects
        .iter()
        .find(|o| o.id == target.profile)
        .ok_or_else(|| unsupported("the part's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(unsupported("the part's profile is not a Sketch"));
    };
    let corners = rectangle_corners(target.base_feature, sketch)?;
    Ok(SavedFilletTarget {
        body,
        base_feature: target.base_feature,
        profile: target.profile,
        profile_segments: target.profile_segments,
        height_mm: target.height_mm,
        corners,
    })
}

/// One row per Body, each a target or its reason, from one snapshot.
pub fn fillet_choices(document: &Document, objects: &[ObjectRecord]) -> Vec<FilletChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .map(|o| {
            let target = saved_target(document, objects, o.id).map_err(|e| e.to_string());
            FilletChoice {
                body: o.id,
                name: o.name.clone(),
                refusal: target.as_ref().err().cloned(),
                target: target.ok(),
            }
        })
        .collect()
}

/// Everything one accepted fillet adds to a copy, prepared before anything is
/// written. Identifiers are minted once, after every check has passed.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEdgeFillet {
    pub(crate) body: ObjectRecord,
    pub(crate) feature: NewObject,
    pub(crate) added_dependencies: Vec<Dependency>,
    pub(crate) removed_dependencies: Vec<Dependency>,
    pub(crate) references: Vec<TopologyRef>,
    pub(crate) previous: ObjectId,
    pub(crate) corner: FilletCorner,
}

impl PreparedEdgeFillet {
    pub fn body(&self) -> &ObjectRecord {
        &self.body
    }
    pub fn feature(&self) -> &NewObject {
        &self.feature
    }
    pub fn previous(&self) -> ObjectId {
        self.previous
    }
    pub fn references(&self) -> &[TopologyRef] {
        &self.references
    }
    /// The corner being rounded, with its labels.
    pub fn corner(&self) -> FilletCorner {
        self.corner
    }
    pub fn radius_mm(&self) -> f64 {
        match &self.feature.payload {
            ObjectPayload::Fillet(f) => f.radius_mm,
            _ => f64::NAN,
        }
    }
}

/// What the finished part is called after one fillet: the new face under the
/// edge it replaced, and every face the plate had, as the fillet leaves it,
/// qualified by the plate's own Extrude.
fn fillet_references(
    feature: ObjectId,
    base: ObjectId,
    joint: ProfileJoint,
    profile_segments: &[StableEntityId],
) -> Vec<TopologyRef> {
    let mut references = vec![TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Face,
        output_role: SemanticRole::EdgeFilletFace {
            edge_feature: base,
            joint,
        },
        selection: SelectionRule::Exact,
        fallback_signature: None,
    }];
    for side in [CapSide::Start, CapSide::End] {
        references.push(TopologyRef {
            id: StableEntityId::new(),
            owner: feature,
            producer_feature: feature,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::OriginCap {
                origin_feature: base,
                side,
            },
            selection: SelectionRule::Exact,
            fallback_signature: None,
        });
    }
    for segment in profile_segments {
        references.push(TopologyRef {
            id: StableEntityId::new(),
            owner: feature,
            producer_feature: feature,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::OriginSide {
                origin_feature: base,
                profile_segment: *segment,
            },
            selection: SelectionRule::AllDerivedFrom { ancestor: *segment },
            fallback_signature: None,
        });
    }
    references
}

/// Prepares one fillet against the saved document.
pub fn prepare_edge_fillet(
    document: &Document,
    body: ObjectId,
    fillet: &EdgeFillet,
) -> Result<PreparedEdgeFillet> {
    let objects = document.objects()?;
    let record = objects
        .iter()
        .find(|o| o.id == body)
        .cloned()
        .ok_or_else(|| CadError::input("selected Body UUID does not exist"))?;
    if !matches!(record.payload, ObjectPayload::Body(_)) {
        return Err(CadError::input("the selected object is not a Body"));
    }
    let target = saved_target(document, &objects, body)?;
    let corner = corner_for(&target.corners, fillet.edge)?;
    corner.check_radius(fillet.radius_mm)?;

    let ordinal = objects
        .iter()
        .map(|o| o.ordinal)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| {
            CadError::input("a new object ordinal cannot fit after the saved objects")
        })?;
    let feature_id = ObjectId::new();
    let feature = NewObject {
        id: feature_id,
        ordinal,
        name: "Fillet".to_owned(),
        payload: ObjectPayload::Fillet(Fillet {
            previous: target.base_feature,
            edge: FilletEdge {
                feature: target.base_feature,
                joint: corner.joint,
            },
            radius_mm: fillet.radius_mm,
        }),
    };
    let mut moved = record.clone();
    moved.payload = ObjectPayload::Body(Body {
        tip_feature: Some(feature_id),
    });
    let added_dependencies = vec![
        Dependency {
            dependent: feature_id,
            dependency: target.base_feature,
            role: DependencyRole::Predecessor,
        },
        Dependency {
            dependent: body,
            dependency: feature_id,
            role: DependencyRole::BodyTip,
        },
    ];
    let removed_dependencies = vec![Dependency {
        dependent: body,
        dependency: target.base_feature,
        role: DependencyRole::BodyTip,
    }];
    let references = fillet_references(
        feature_id,
        target.base_feature,
        corner.joint,
        &target.profile_segments,
    );
    Ok(PreparedEdgeFillet {
        body: moved,
        feature,
        added_dependencies,
        removed_dependencies,
        references,
        previous: target.base_feature,
        corner,
    })
}

/// Re-derives a prepared fillet from the document it claims to be against:
/// the writer's guard. Everything but the minted identities is derived again
/// from the numbers the prepared payload carries, and the whole is compared.
pub(crate) fn rederive(document: &Document, prepared: &PreparedEdgeFillet) -> Result<()> {
    let ObjectPayload::Fillet(fillet) = &prepared.feature.payload else {
        return Err(CadError::input("a prepared fillet carries a Fillet"));
    };
    let stated = EdgeFillet {
        edge: fillet.edge,
        radius_mm: fillet.radius_mm,
    };
    let mut checked = prepare_edge_fillet(document, prepared.body.id, &stated)?;
    let fresh = checked.feature.id;
    let swap = |id: ObjectId| {
        if id == fresh { prepared.feature.id } else { id }
    };
    checked.feature.id = prepared.feature.id;
    if let ObjectPayload::Body(b) = &mut checked.body.payload {
        b.tip_feature = b.tip_feature.map(swap);
    }
    for dependency in checked
        .added_dependencies
        .iter_mut()
        .chain(&mut checked.removed_dependencies)
    {
        dependency.dependent = swap(dependency.dependent);
        dependency.dependency = swap(dependency.dependency);
    }
    if checked.references.len() != prepared.references.len() {
        return Err(CadError::input(
            "the prepared fillet names a different number of faces than the document would",
        ));
    }
    for (mine, theirs) in checked.references.iter_mut().zip(&prepared.references) {
        mine.id = theirs.id;
        mine.owner = swap(mine.owner);
        mine.producer_feature = swap(mine.producer_feature);
    }
    if checked != *prepared {
        return Err(CadError::input(
            "the prepared fillet does not describe the document it is being written to",
        ));
    }
    Ok(())
}

/// The evaluator's statement of the class: the Fillet rounds a vertical edge
/// of the rectangular NewBody Extrude it consumes, at a radius the policy
/// accepts. Asked of the saved objects at every rebuild, so a document this
/// build did not write is refused by the same rule that wrote this one.
pub fn evaluable_fillet(objects: &[ObjectRecord], fillet: &Fillet) -> Result<FilletCorner> {
    if fillet.edge.feature != fillet.previous {
        return Err(unsupported(
            "this build rounds an edge of the feature a Fillet consumes, and this Fillet names \
             an edge of another feature",
        ));
    }
    let base = objects
        .iter()
        .find(|o| o.id == fillet.previous)
        .ok_or_else(|| CadError::input("the Fillet's predecessor is missing"))?;
    let ObjectPayload::Extrude(extrude) = &base.payload else {
        return Err(unsupported(
            "this build rounds an edge of an extruded plate, and the Fillet's predecessor is not \
             an Extrude",
        ));
    };
    if extrude.operation != crate::SolidOperation::NewBody
        || extrude.previous.is_some()
        || extrude.reversed
        || !matches!(extrude.end_condition, crate::EndCondition::Blind { .. })
    {
        return Err(unsupported(
            "this build rounds an edge of a forward Blind NewBody extrusion",
        ));
    }
    let profile = objects
        .iter()
        .find(|o| o.id == extrude.profile)
        .ok_or_else(|| CadError::input("the base Extrude's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(unsupported("the base Extrude's profile is not a Sketch"));
    };
    let corners = rectangle_corners(base.id, sketch)?;
    let corner = corner_for(&corners, fillet.edge)?;
    corner.check_radius(fillet.radius_mm)?;
    Ok(corner)
}
