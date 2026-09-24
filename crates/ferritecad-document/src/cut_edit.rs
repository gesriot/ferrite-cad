// SPDX-License-Identifier: MIT
//! Adding one circular cut to a saved body, in a new copy.
//!
//! This is the first operation that makes a body's history longer than one
//! feature, so it is also the first that has to say what a feature's *input*
//! is. It says it one way: a feature names the feature whose result it
//! modifies, and a body names its current tip. Ownership is not stored twice —
//! the body owns whatever it can reach from its tip through those names — and
//! nothing points from a feature at a body, which is what keeps the evaluation
//! graph acyclic by construction rather than by a check that could be removed.
//!
//! # The supported source
//!
//! Exactly the frame every copy edit of a saved profile already demands, and
//! nothing wider: one untransformed XY datum, one Sketch on it, one forward
//! literal Blind `Extrude`/`NewBody`, its `Body`, and those three dependencies.
//! On top of that the Sketch must be an unconstrained closed polygon of four
//! Lines forming an **axis-aligned rectangle**. That last narrowing is what
//! lets "the tool is inside the part and does not touch its outer wall" be a
//! measured fact rather than a hopeful one; it is not a claim about arbitrary
//! solids, and a wider class is refused with its reason.
//!
//! A single bounded reader validates the complete plate and 0–16 separate Cut
//! links. The same catalogue serves addition, editing and discovery.
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId, Tolerance, Transform};
use std::collections::BTreeSet;

use crate::{
    Body, CapSide, CircleExtrusion, DatumPlane, Dependency, DependencyRole, Document, EndCondition,
    EntityKind, Expression, Extrude, ObjectPayload, ObjectRecord, Point2, PolygonExtrusion,
    SelectionRule, SemanticRole, Sketch, SketchCurve, SketchGeometry, SolidOperation, TopologyRef,
};

fn unsupported(message: impl Into<String>) -> CadError {
    CadError::unsupported(message)
}

/// How close the tool may come to the part's outer wall before this build
/// refuses to publish the result.
///
/// The kernel's own linear tolerance, not a number chosen here. A tool that
/// grazes the boundary produces a sliver face or a self-touching solid
/// depending on rounding, and which of those it is is not something a first
/// managed route should discover at publication time. Refusing is the honest
/// answer; nothing is nudged.
pub const WALL_CLEARANCE_MM: f64 = Tolerance::DEFAULT_LINEAR;

/// Explicit editor boundary; general document reading is not limited by it.
pub const MAX_CIRCULAR_CUTS: usize = 16;

/// The saved body a circular cut can be added to, exactly as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedCutTarget {
    /// The body whose tip the new feature becomes. Its identity is kept.
    pub body: ObjectId,
    /// The datum the tool is drawn on: the same XY plane the part was drawn on.
    pub plane: ObjectId,
    /// The feature the cut modifies, which is the body's tip today.
    pub tip_feature: ObjectId,
    /// The extrusion that made the rectangular plate, even when the tip is a Cut.
    pub base_feature: ObjectId,
    /// All tools, ordered by predecessor links from first Cut to tip.
    pub tools: Vec<SavedCutTool>,
    /// The single existing cut for N=1; null for N=0 or N>1.
    pub existing_cut: Option<SavedCircularCut>,
    /// The original rectangular profile, reported so a form can name the part.
    pub profile: ObjectId,
    /// Every segment of that profile, in stored order, so the copy can name
    /// what the finished part still has.
    pub profile_segments: Vec<StableEntityId>,
    /// How tall the part is; the depth of a cut is measured against it.
    pub height_mm: f64,
    /// The rectangle the part is, as `[[min_x, min_y], [max_x, max_y]]`.
    pub extents_mm: [[f64; 2]; 2],
}

/// A row in objects() order. Refusal is local; copy_access still takes priority.
#[derive(Debug, Clone, PartialEq)]
pub struct CutChoice {
    pub body: ObjectId,
    pub name: Option<String>,
    /// None for an unsupported body, never an invented part.
    pub target: Option<SavedCutTarget>,
    pub refusal: Option<String>,
}

/// What one cut asks for.
///
/// The centre and radius are the tool's, on the part's own XY datum; the depth
/// runs along that datum's normal, which is +Z. Both facts are stated in the
/// contract and in the interface rather than derived from a face the user
/// clicked: this slice attaches to no face, and a request that could name one
/// would be promising something nothing here implements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CircularCut {
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub extent: CutExtent,
}

/// How far one circular Cut runs, as the person said it.
///
/// A stated Blind depth and a request to run through everything are different
/// intents, and only one of them is a number. The length a ThroughAll tool
/// actually has is computed by the evaluator from the current base at every
/// rebuild and is never stored; [`reach_mm`][Self::reach_mm] is that length in
/// this narrow class, for policy checks that must not confuse the two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CutExtent {
    /// A literal depth in mm along +Z from the base XY datum.
    Blind { depth_mm: f64 },
    /// Through everything the body holds, at whatever height it has.
    ThroughAll,
}

impl CutExtent {
    /// The stated depth, which a ThroughAll Cut does not have.
    pub fn blind_depth_mm(self) -> Option<f64> {
        match self {
            Self::Blind { depth_mm } => Some(depth_mm),
            Self::ThroughAll => None,
        }
    }

    /// How far the tool reaches into a plate `height_mm` tall.
    pub fn reach_mm(self, height_mm: f64) -> f64 {
        match self {
            Self::Blind { depth_mm } => depth_mm,
            Self::ThroughAll => height_mm,
        }
    }

    /// The one floor rule: only a Blind Cut shallower than the plate has one.
    /// ThroughAll never has a floor, at any height.
    pub fn leaves_a_floor(self, height_mm: f64) -> bool {
        match self {
            Self::Blind { depth_mm } => depth_mm < height_mm,
            Self::ThroughAll => false,
        }
    }

    /// The stored end condition. Never a depth standing in for ThroughAll.
    fn end_condition(self) -> Result<EndCondition> {
        Ok(match self {
            Self::Blind { depth_mm } => EndCondition::Blind {
                distance: Expression::constant(depth_mm)?,
            },
            Self::ThroughAll => EndCondition::ThroughAll,
        })
    }
}

/// What a request was able to say about the end of a Cut.
///
/// Request v1 has only a Blind depth. Taken to mean "make this Blind" it would
/// silently turn a saved ThroughAll Cut into a Blind one, destroying an intent
/// the client could not even see, so such a request is refused for a
/// ThroughAll Cut instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtentVocabulary {
    BlindOnly,
    BlindOrThroughAll,
}

/// One validated model per snapshot, shared by add and edit discovery.
pub(crate) struct CutHistory {
    pub(crate) target: SavedCutTarget,
    pub(crate) cuts: Vec<SavedCircularCut>,
}

impl CutHistory {
    fn add_target(&self, body: ObjectId) -> Result<SavedCutTarget> {
        if self.target.body != body {
            return Err(unsupported("selected Body is not in the supported history"));
        }
        if self.cuts.len() == MAX_CIRCULAR_CUTS {
            return Err(unsupported(
                "this editor supports at most 16 circular Cuts; all 16 remain editable",
            ));
        }
        Ok(self.target.clone())
    }
}

pub(crate) struct CutCatalog {
    pub bodies: Vec<CutChoice>,
    pub features: Vec<CutParameterChoice>,
    pub sketches: Vec<crate::SketchChoice>,
    pub height: std::result::Result<Option<crate::BaseHeightContext>, String>,
}

pub(crate) fn cut_catalog(document: &Document, objects: &[ObjectRecord]) -> CutCatalog {
    let history = saved_history(document, objects).map_err(|e| e.to_string());
    let bodies = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .map(|o| {
            let target = history
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|h| h.add_target(o.id).map_err(|e| e.to_string()));
            CutChoice {
                body: o.id,
                name: o.name.clone(),
                refusal: target.as_ref().err().cloned(),
                target: target.ok(),
            }
        })
        .collect();
    let features = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
        .map(|o| {
            let saved = require_cut_operation(o)
                .map_err(|e| e.to_string())
                .and_then(|()| history.as_ref().map_err(Clone::clone))
                .and_then(|h| {
                    h.cuts
                        .iter()
                        .find(|c| c.feature == o.id)
                        .cloned()
                        .ok_or_else(|| {
                            unsupported("selected Cut is not in the Body's supported history")
                                .to_string()
                        })
                });
            CutParameterChoice {
                feature: o.id,
                name: o.name.clone(),
                refusal: saved.as_ref().err().cloned(),
                saved: saved.ok(),
            }
        })
        .collect();
    let sketches = crate::sketch_edit::choices_with_history(document, objects, history.as_ref());
    let height = crate::height_edit::has_history(document, objects)
        .map_err(|e| e.to_string())
        .and_then(|present| {
            if present {
                history
                    .as_ref()
                    .map(|h| Some(h.height_context()))
                    .map_err(Clone::clone)
            } else {
                Ok(None)
            }
        });
    CutCatalog {
        bodies,
        features,
        sketches,
        height,
    }
}

pub fn cut_choices(document: &Document, objects: &[ObjectRecord]) -> Vec<CutChoice> {
    cut_catalog(document, objects).bodies
}

pub(crate) fn supported(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
) -> Result<SavedCutTarget> {
    saved_history(document, objects)?.add_target(object.id)
}

/// The axis-aligned rectangle a profile is, or why it is not one.
///
/// Read from the stored lines and from nothing else. Four segments, each
/// axis-parallel, meeting end to end and closing — which is what makes the
/// clearance check below a statement about the part rather than about its
/// bounding box.
pub(crate) fn rectangle(curves: &[SketchCurve], height_mm: f64) -> Result<[[f64; 2]; 2]> {
    if curves.len() != 4 {
        return Err(unsupported(
            "this slice cuts a rectangular plate, and this profile is not four Lines",
        ));
    }
    let mut corners = Vec::with_capacity(4);
    for curve in curves {
        if curve.construction {
            return Err(unsupported("construction geometry bounds no face"));
        }
        let SketchGeometry::Line { start, end } = curve.geometry else {
            return Err(unsupported(
                "this slice cuts a rectangular plate drawn with Lines",
            ));
        };
        let axis_aligned = (start.x - end.x).abs() <= WALL_CLEARANCE_MM
            || (start.y - end.y).abs() <= WALL_CLEARANCE_MM;
        if !axis_aligned {
            return Err(unsupported(
                "this slice cuts an axis-aligned rectangular plate, and one of these Lines runs \
                 at an angle",
            ));
        }
        corners.push([start.x, start.y]);
    }
    // Closed, end to end, in the order the sketch stores them.
    for (index, curve) in curves.iter().enumerate() {
        let SketchGeometry::Line { end, .. } = curve.geometry else {
            unreachable!("checked above")
        };
        let next = corners[(index + 1) % corners.len()];
        if (end.x - next[0]).abs() > WALL_CLEARANCE_MM
            || (end.y - next[1]).abs() > WALL_CLEARANCE_MM
        {
            return Err(unsupported(
                "this slice cuts a closed rectangular plate, and these Lines do not meet",
            ));
        }
    }
    // The one numeric policy the creation route applies, asked of the saved
    // numbers so a part outside it is refused rather than silently narrowed.
    PolygonExtrusion::new(corners.clone(), height_mm)
        .map_err(|e| unsupported(format!("the saved part is outside cut policy: {e}")))?;

    let min_x = corners.iter().map(|c| c[0]).fold(f64::INFINITY, f64::min);
    let max_x = corners
        .iter()
        .map(|c| c[0])
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = corners.iter().map(|c| c[1]).fold(f64::INFINITY, f64::min);
    let max_y = corners
        .iter()
        .map(|c| c[1])
        .fold(f64::NEG_INFINITY, f64::max);
    // Four corners of a rectangle are exactly two distinct x and two distinct
    // y values. Anything else is a four-sided figure that is not one.
    let distinct = |values: [f64; 4], low: f64, high: f64| {
        values
            .iter()
            .all(|v| (v - low).abs() <= WALL_CLEARANCE_MM || (v - high).abs() <= WALL_CLEARANCE_MM)
    };
    let xs = [corners[0][0], corners[1][0], corners[2][0], corners[3][0]];
    let ys = [corners[0][1], corners[1][1], corners[2][1], corners[3][1]];
    if !distinct(xs, min_x, max_x) || !distinct(ys, min_y, max_y) {
        return Err(unsupported(
            "this slice cuts a rectangular plate, and these four corners make some other shape",
        ));
    }
    Ok([[min_x, min_y], [max_x, max_y]])
}

/// Validate a changed plate against every unchanged, absolute tool.
/// Called by the coordinate editor's shared draft/prepare/writer check.
pub(crate) fn validate_base(
    curves: &[SketchCurve],
    height_mm: f64,
    tools: &[SavedCutTool],
) -> Result<()> {
    let extents = rectangle(curves, height_mm)?;
    for tool in tools {
        validate(
            height_mm,
            extents,
            &CircularCut {
                center_mm: tool.center_mm,
                radius_mm: tool.radius_mm,
                extent: tool.extent,
            },
        )
        .map_err(|e| CadError::input(format!("Cut {}: {e}", tool.feature)))?;
    }
    Ok(())
}

impl CutChoice {
    /// The same check the copy applies, with no SQLite or kernel work.
    pub fn validate_cut(&self, cut: &CircularCut) -> Result<()> {
        let target = self
            .target
            .as_ref()
            .ok_or_else(|| unsupported("unsupported cut target"))?;
        validate_target(target, cut).map(|_| ())
    }
}

fn validate_target(target: &SavedCutTarget, cut: &CircularCut) -> Result<CircleExtrusion> {
    let tool = validate(target.height_mm, target.extents_mm, cut)?;
    for saved in &target.tools {
        validate_disks(cut, saved)?;
    }
    Ok(tool)
}

/// Shared by add, discovery and parameter edits of either history link.
fn validate_disks(cut: &CircularCut, other: &SavedCutTool) -> Result<()> {
    let center = other.center_mm;
    let radius = other.radius_mm;
    let gap =
        (cut.center_mm[0] - center[0]).hypot(cut.center_mm[1] - center[1]) - cut.radius_mm - radius;
    if !gap.is_finite() || gap <= WALL_CLEARANCE_MM {
        return Err(CadError::input(format!(
            "the circular disk conflicts with Cut {}: disks must be separate by more than {WALL_CLEARANCE_MM} mm; gap is {gap} mm",
            other.feature
        )));
    }
    Ok(())
}

/// The one numeric rule, applied to a request against the saved part.
pub(crate) fn validate(
    height_mm: f64,
    extents_mm: [[f64; 2]; 2],
    cut: &CircularCut,
) -> Result<CircleExtrusion> {
    // The tool is a cylinder, judged by the policy every cylinder in this
    // build is judged by: finite centre, positive radius, positive height and
    // the published bound on all of them.
    let tool = CircleExtrusion::new(cut.center_mm, cut.radius_mm, cut.extent.reach_mm(height_mm))?;
    if let CutExtent::Blind { depth_mm } = cut.extent
        && depth_mm > height_mm
    {
        return Err(CadError::input(format!(
            "a cut of {depth_mm} mm into a part {height_mm} mm tall would run past it; this slice \
             cuts to a depth the part has"
        )));
    }
    let [[min_x, min_y], [max_x, max_y]] = extents_mm;
    let clearances = [
        (cut.center_mm[0] - cut.radius_mm) - min_x,
        max_x - (cut.center_mm[0] + cut.radius_mm),
        (cut.center_mm[1] - cut.radius_mm) - min_y,
        max_y - (cut.center_mm[1] + cut.radius_mm),
    ];
    // Strictly inside, by more than the kernel's own idea of one point. A tool
    // that reaches the outer wall turns the cut into an open slot or a sliver,
    // and which of the two depends on rounding. Refused rather than nudged:
    // moving the number would publish a part the user did not ask for.
    if clearances.iter().any(|gap| *gap <= WALL_CLEARANCE_MM) {
        return Err(CadError::input(format!(
            "the tool must stay inside the part by more than {WALL_CLEARANCE_MM} mm, and the \
             closest approach here is {} mm",
            clearances.iter().copied().fold(f64::INFINITY, f64::min)
        )));
    }
    Ok(tool)
}

/// Everything one accepted cut adds to a copy, prepared before anything is
/// written.
///
/// Identifiers are minted once, here, after every check has passed. The body is
/// carried as a whole record because its payload changes — its tip becomes the
/// new feature — and nothing else about it does.
#[derive(Debug, Clone, PartialEq)]
pub struct NewObject {
    pub id: ObjectId,
    pub ordinal: i64,
    pub name: String,
    pub payload: ObjectPayload,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedCircularCut {
    pub(crate) body: ObjectRecord,
    pub(crate) sketch: NewObject,
    pub(crate) feature: NewObject,
    pub(crate) added_dependencies: Vec<Dependency>,
    pub(crate) removed_dependencies: Vec<Dependency>,
    pub(crate) references: Vec<TopologyRef>,
    /// The feature the new one modifies, kept so a completion can report it.
    pub(crate) previous: ObjectId,
    /// The circle the tool is drawn with, so a caller can name the bore.
    pub(crate) tool_curve: StableEntityId,
    pub(crate) height_mm: f64,
    pub(crate) cut: CircularCut,
}

impl PreparedCircularCut {
    pub fn body(&self) -> &ObjectRecord {
        &self.body
    }
    pub fn sketch(&self) -> &NewObject {
        &self.sketch
    }
    pub fn feature(&self) -> &NewObject {
        &self.feature
    }
    pub fn previous(&self) -> ObjectId {
        self.previous
    }
    pub fn tool_curve(&self) -> StableEntityId {
        self.tool_curve
    }
    pub fn references(&self) -> &[TopologyRef] {
        &self.references
    }
    /// Whether this cut leaves a floor, which a cut all the way through does
    /// not. Reported rather than recomputed by a caller from the two numbers.
    pub fn leaves_a_floor(&self) -> bool {
        self.cut.extent.leaves_a_floor(self.height_mm)
    }
    /// The intent the new Cut stores.
    pub fn extent(&self) -> CutExtent {
        self.cut.extent
    }
}

/// What the finished part is called after one circular cut.
///
/// The bore wall under the circle that drew it; the floor only when there is
/// one; and every face the part already had, as the cut leaves it.
///
/// One function, because the operation that adds a cut and the one that edits
/// its numbers have to agree about this down to the last rule. If they could
/// drift, the edit would be checking a saved document against a contract other
/// than the one that wrote it, and the one that had drifted would be the one
/// that accepted a document this build never made.
fn cut_references(
    feature: ObjectId,
    tool_curve: StableEntityId,
    profile_segments: &[StableEntityId],
    leaves_a_floor: bool,
) -> Vec<TopologyRef> {
    let mut references = vec![TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Face,
        output_role: SemanticRole::ExtrudeSide {
            profile_segment: tool_curve,
        },
        selection: SelectionRule::AllDerivedFrom {
            ancestor: tool_curve,
        },
        fallback_signature: None,
    }];
    if leaves_a_floor {
        references.push(TopologyRef {
            id: StableEntityId::new(),
            owner: feature,
            producer_feature: feature,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::ExtrudeCap { side: CapSide::End },
            selection: SelectionRule::Exact,
            fallback_signature: None,
        });
    }
    for side in [CapSide::Start, CapSide::End] {
        references.push(TopologyRef {
            id: StableEntityId::new(),
            owner: feature,
            producer_feature: feature,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::CarriedCap { side },
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
            output_role: SemanticRole::CarriedSide {
                profile_segment: *segment,
            },
            selection: SelectionRule::AllDerivedFrom { ancestor: *segment },
            fallback_signature: None,
        });
    }
    references
}

/// One naming contract for creation and discovery of either history link.
fn history_references(
    feature_id: ObjectId,
    tool_curve: StableEntityId,
    profile_segments: &[StableEntityId],
    leaves_a_floor: bool,
    base_feature: ObjectId,
    ancestors: &[SavedCutTool],
) -> Vec<TopologyRef> {
    let mut references = cut_references(feature_id, tool_curve, profile_segments, leaves_a_floor);

    if !ancestors.is_empty() {
        for reference in &mut references {
            reference.output_role = match reference.output_role {
                SemanticRole::CarriedCap { side } => SemanticRole::OriginCap {
                    origin_feature: base_feature,
                    side,
                },
                SemanticRole::CarriedSide { profile_segment } => SemanticRole::OriginSide {
                    origin_feature: base_feature,
                    profile_segment,
                },
                ref own => own.clone(),
            };
        }
    }
    for saved in ancestors {
        references.push(TopologyRef {
            id: StableEntityId::new(),
            owner: feature_id,
            producer_feature: feature_id,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::OriginSide {
                origin_feature: saved.feature,
                profile_segment: saved.tool_curve,
            },
            selection: SelectionRule::AllDerivedFrom {
                ancestor: saved.tool_curve,
            },
            fallback_signature: None,
        });
        if saved.leaves_a_floor {
            references.push(TopologyRef {
                id: StableEntityId::new(),
                owner: feature_id,
                producer_feature: feature_id,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::OriginCap {
                    origin_feature: saved.feature,
                    side: CapSide::End,
                },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            });
        }
    }
    references
}

/// Prepares one circular cut against the saved document.
///
/// Mints nothing until every check has passed, so a refused request leaves no
/// identifier behind that a later one might reuse.
pub fn prepare_circular_cut(
    document: &Document,
    body: ObjectId,
    cut: &CircularCut,
) -> Result<PreparedCircularCut> {
    let objects = document.objects()?;
    let record = objects
        .iter()
        .find(|o| o.id == body)
        .cloned()
        .ok_or_else(|| CadError::input("selected Body UUID does not exist"))?;
    let target = supported(document, &objects, &record)?;
    validate_target(&target, cut)?;

    let ordinal = objects.iter().map(|o| o.ordinal).max().unwrap_or(0);
    let feature_ordinal = ordinal.checked_add(2).ok_or_else(|| {
        CadError::input("two new object ordinals cannot fit after the saved objects")
    })?;

    // Identifiers come from the same clock the rest of the document's do, so a
    // copy's new objects sort after everything that was already in it.
    let sketch_id = ObjectId::new();
    let feature_id = ObjectId::new();
    let tool_curve = StableEntityId::new();

    let tool_sketch = NewObject {
        id: sketch_id,
        ordinal: feature_ordinal - 1,
        name: "Cut profile".to_owned(),
        payload: ObjectPayload::Sketch(Sketch {
            plane: target.plane,
            curves: vec![SketchCurve {
                id: tool_curve,
                construction: false,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(cut.center_mm[0], cut.center_mm[1])?,
                    radius: cut.radius_mm,
                },
            }],
            constraints: Vec::new(),
        }),
    };
    let feature = NewObject {
        id: feature_id,
        ordinal: feature_ordinal,
        name: "Cut".to_owned(),
        payload: ObjectPayload::Extrude(Extrude {
            profile: sketch_id,
            end_condition: cut.extent.end_condition()?,
            reversed: false,
            operation: SolidOperation::Cut,
            // Not the body. See the module note: a feature names the result it
            // modifies, and the body names its tip.
            target_body: None,
            previous: Some(target.tip_feature),
        }),
    };
    let mut moved = record.clone();
    moved.payload = ObjectPayload::Body(Body {
        tip_feature: Some(feature_id),
    });

    let added_dependencies = vec![
        Dependency {
            dependent: sketch_id,
            dependency: target.plane,
            role: DependencyRole::Plane,
        },
        Dependency {
            dependent: feature_id,
            dependency: sketch_id,
            role: DependencyRole::Profile,
        },
        Dependency {
            dependent: feature_id,
            dependency: target.tip_feature,
            role: DependencyRole::Predecessor,
        },
        Dependency {
            dependent: body,
            dependency: feature_id,
            role: DependencyRole::BodyTip,
        },
    ];
    // The body's old tip edge. Left behind it would be an ordering constraint
    // no payload asks for, and the frame check counts the edges a document has.
    let removed_dependencies = vec![Dependency {
        dependent: body,
        dependency: target.tip_feature,
        role: DependencyRole::BodyTip,
    }];

    let references = history_references(
        feature_id,
        tool_curve,
        &target.profile_segments,
        cut.extent.leaves_a_floor(target.height_mm),
        target.base_feature,
        &target.tools,
    );

    Ok(PreparedCircularCut {
        body: moved,
        sketch: tool_sketch,
        feature,
        added_dependencies,
        removed_dependencies,
        references,
        previous: target.tip_feature,
        tool_curve,
        height_mm: target.height_mm,
        cut: *cut,
    })
}

/// Re-derives a prepared cut from the document it claims to be against.
///
/// The writer's guard, and the reason it exists is the reason the analytic
/// editors have one: a prepared value is public and mutable, so its fields do
/// not prove what produced them. Everything except the identifiers is derived
/// again from the numbers the prepared payload itself carries, and the whole of
/// it is then compared.
pub(crate) fn rederive(document: &Document, prepared: &PreparedCircularCut) -> Result<()> {
    let ObjectPayload::Sketch(sketch) = &prepared.sketch.payload else {
        return Err(CadError::input("a prepared cut carries a Sketch"));
    };
    let [curve] = sketch.curves.as_slice() else {
        return Err(CadError::input("a prepared cut draws one circle"));
    };
    let SketchGeometry::Circle { center, radius } = curve.geometry else {
        return Err(CadError::input("a prepared cut draws a Circle"));
    };
    let ObjectPayload::Extrude(extrude) = &prepared.feature.payload else {
        return Err(CadError::input("a prepared cut carries an Extrude"));
    };
    let stated = CircularCut {
        center_mm: [center.x, center.y],
        radius_mm: radius,
        extent: saved_extent(extrude, "prepared cut")?,
    };

    // Everything the document would produce for exactly those numbers. The
    // identifiers are minted afresh each time and cannot be re-derived, so the
    // prepared ones are substituted in and the whole of the rest is compared:
    // one comparison rather than a list of fields somebody has to keep up to
    // date as this structure grows.
    let mut checked = prepare_circular_cut(document, prepared.body.id, &stated)?;
    let (fresh_sketch, fresh_feature, fresh_curve) =
        (checked.sketch.id, checked.feature.id, checked.tool_curve);
    let swap = |id: ObjectId| {
        if id == fresh_sketch {
            prepared.sketch.id
        } else if id == fresh_feature {
            prepared.feature.id
        } else {
            id
        }
    };

    checked.sketch.id = prepared.sketch.id;
    checked.feature.id = prepared.feature.id;
    checked.tool_curve = prepared.tool_curve;
    if let ObjectPayload::Sketch(s) = &mut checked.sketch.payload {
        for c in &mut s.curves {
            if c.id == fresh_curve {
                c.id = prepared.tool_curve;
            }
        }
    }
    if let ObjectPayload::Extrude(e) = &mut checked.feature.payload {
        e.profile = swap(e.profile);
    }
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
            "the prepared cut names a different number of faces than the document would",
        ));
    }
    for (mine, theirs) in checked.references.iter_mut().zip(&prepared.references) {
        // Only the identity is substituted. The owner, the producer, the role
        // and the rule are what is being checked, and a forged payload that
        // reordered them fails here rather than being matched up.
        mine.id = theirs.id;
        if let SemanticRole::ExtrudeSide { profile_segment } = &mut mine.output_role
            && *profile_segment == fresh_curve
        {
            *profile_segment = prepared.tool_curve;
        }
        if let SelectionRule::AllDerivedFrom { ancestor } = &mut mine.selection
            && *ancestor == fresh_curve
        {
            *ancestor = prepared.tool_curve;
        }
        mine.owner = swap(mine.owner);
        mine.producer_feature = swap(mine.producer_feature);
    }

    if checked != *prepared {
        return Err(CadError::input(
            "the prepared cut does not describe the document it is being written to",
        ));
    }
    Ok(())
}

/// The plane's own placement, for an interface that must name it.
///
/// Reported from the document rather than assumed, so a form that says "the
/// part's base XY plane, cutting along +Z" is repeating what is there.
pub fn cut_plane_placement(document: &Document, plane: ObjectId) -> Result<Transform> {
    let objects = document.objects()?;
    match objects.iter().find(|o| o.id == plane).map(|o| &o.payload) {
        Some(ObjectPayload::DatumPlane(DatumPlane { placement })) => Ok(*placement),
        _ => Err(unsupported("the cut's datum plane is missing")),
    }
}

/// One saved circular cut, exactly as stored, with everything an edit of its
/// numbers has to know about it.
///
/// Every identity here is reached through a link or a type — the body through
/// its tip, the predecessor through `previous`, the tool through the feature's
/// profile, the circle through being the one curve that sketch holds. None of
/// it is found by name, by row order, or by taking the first `Circle` in the
/// document: those would all be the same answer on this narrow class and a
/// different one on the first document that is not it.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedCircularCut {
    /// The Cut feature whose numbers may change. Its identity is kept.
    pub feature: ObjectId,
    /// The body whose history contains the cut.
    pub body: ObjectId,
    /// The datum both sketches are drawn on.
    pub plane: ObjectId,
    /// The original NewBody extrusion, used for plate dimensions.
    pub base_feature: ObjectId,
    /// The selected Cut's immediate predecessor, unchanged by this edit.
    pub previous_feature: ObjectId,
    /// The final feature exposed by the Body, unchanged by this edit.
    pub tip_feature: ObjectId,
    /// All tools in history order, including this selected Cut.
    pub tools: Vec<SavedCutTool>,
    /// The other tool for N=2; null for N=1 or N>2 (compatibility field).
    pub neighboring_tool: Option<SavedCutTool>,
    /// All saved floor names that would disappear, including the final origin.
    pub protected_floor_references: Vec<StableEntityId>,
    /// The part's own profile; unchanged by this edit.
    pub profile_sketch: ObjectId,
    /// The sketch holding the tool circle, whose payload this edit rewrites.
    pub tool_sketch: ObjectId,
    /// The circle inside it. It keeps this identity across the edit.
    pub tool_curve: StableEntityId,
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    /// The saved intent: a stated depth, or through everything.
    pub extent: CutExtent,
    /// How tall the part is, which is what a depth is measured against.
    pub height_mm: f64,
    /// The rectangle the part is, as `[[min_x, min_y], [max_x, max_y]]`.
    pub extents_mm: [[f64; 2]; 2],
    /// The saved reference naming this cut's floor, when it has one.
    ///
    /// `None` for a cut that already runs through the part. Its presence is
    /// what decides whether the depth may be raised to the part's height: the
    /// face that reference names would stop existing, and this slice refuses
    /// rather than dropping a saved name or pointing it somewhere else.
    pub floor_reference: Option<StableEntityId>,
}

/// A tool's saved identity and numbers, without recursive ownership.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedCutTool {
    pub feature: ObjectId,
    pub tool_sketch: ObjectId,
    pub tool_curve: StableEntityId,
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub extent: CutExtent,
    pub leaves_a_floor: bool,
}

impl SavedCircularCut {
    /// This Cut as one entry of its history's tool list.
    pub fn tool(&self) -> SavedCutTool {
        SavedCutTool {
            feature: self.feature,
            tool_sketch: self.tool_sketch,
            tool_curve: self.tool_curve,
            center_mm: self.center_mm,
            radius_mm: self.radius_mm,
            extent: self.extent,
            leaves_a_floor: self.extent.leaves_a_floor(self.height_mm),
        }
    }
    /// Whether a cut of this extent leaves a floor. One rule, asked of the
    /// saved height rather than recomputed by each caller from two numbers.
    fn floor_at(&self, extent: CutExtent) -> bool {
        extent.leaves_a_floor(self.height_mm)
    }
    /// Whether the saved Cut stops inside the part.
    pub fn leaves_a_floor(&self) -> bool {
        self.floor_at(self.extent)
    }
    /// Request versions that can express this Cut without losing its intent.
    pub fn request_versions(&self) -> &'static [u32] {
        match self.extent {
            CutExtent::Blind { .. } => &[1, 2],
            CutExtent::ThroughAll => &[2],
        }
    }
    /// Whether the depth may be raised to the part's height, cutting through.
    pub fn through_allowed(&self) -> bool {
        self.protected_floor_references.is_empty()
    }
}

/// A row in `objects()` order, one per feature. Refusal is local; the
/// document's own `copy_access` still takes priority over it.
#[derive(Debug, Clone, PartialEq)]
pub struct CutParameterChoice {
    pub feature: ObjectId,
    pub name: Option<String>,
    /// None for a feature whose numbers this slice cannot edit, never an
    /// invented cut.
    pub saved: Option<SavedCircularCut>,
    pub refusal: Option<String>,
}

/// New numbers for one saved circular cut.
///
/// The curve is named as well as the feature, for the reason the saved circle
/// editor names one: a form must send back the identity it read rather than
/// whatever the document happens to hold when the job runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CircularCutEdit {
    pub tool_curve: StableEntityId,
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub extent: CutExtent,
    /// What the request could have said; see [`ExtentVocabulary`].
    pub vocabulary: ExtentVocabulary,
}

/// What an accepted edit does to the saved names, stated before anything is
/// written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transition {
    /// The cut keeps the floor it had, or keeps having none.
    Kept,
    /// A cut that ran through the part now stops inside it, so it gains a
    /// floor, named in its historical and any dependent final output.
    FloorAppears,
}

pub fn cut_parameter_choices(
    document: &Document,
    objects: &[ObjectRecord],
) -> Vec<CutParameterChoice> {
    cut_catalog(document, objects).features
}

impl CutParameterChoice {
    /// The same check the copy applies, with no SQLite and no kernel work.
    pub fn validate_edit(&self, edit: &CircularCutEdit) -> Result<()> {
        let saved = self
            .saved
            .as_ref()
            .ok_or_else(|| unsupported("unsupported cut to edit"))?;
        transition(saved, edit).map(|_| ())
    }
}

/// The payload this edit is about to rewrite has to be one this reader can
/// write back whole.
fn require_rewritable(object: &ObjectRecord, what: &str) -> Result<()> {
    if object.payload.to_storage_bytes()?.as_slice() != object.storage_bytes() {
        return Err(unsupported(format!(
            "saved {what} storage cannot be rewritten losslessly by this reader"
        )));
    }
    Ok(())
}

/// The Blind literal one saved feature runs to.
///
/// Its own reader rather than `editable_extrude`, which refuses a Cut on
/// purpose so that the generic extrusion-distance edit cannot rewrite half of
/// a cut's history. What a distance may be — a finite literal, not a formula —
/// is the same rule here; that it is asked separately is what keeps the older
/// command's refusal intact.
fn blind_literal(extrude: &Extrude, what: &str) -> Result<f64> {
    let EndCondition::Blind { distance } = &extrude.end_condition else {
        return Err(unsupported(format!("only a Blind {what} has a depth")));
    };
    let literal = distance.source.trim().parse::<f64>().ok();
    if !literal.is_some_and(|v| v.is_finite() && v == distance.value()) {
        return Err(unsupported(format!(
            "the {what}'s distance is a formula or parameter; only a numeric literal can be \
             edited"
        )));
    }
    Ok(distance.value())
}

/// The saved intent of one Cut: a literal Blind depth or ThroughAll. Anything
/// else, including a formula, is refused rather than read as a number.
fn saved_extent(extrude: &Extrude, what: &str) -> Result<CutExtent> {
    match &extrude.end_condition {
        EndCondition::ThroughAll => Ok(CutExtent::ThroughAll),
        _ => blind_literal(extrude, what).map(|depth_mm| CutExtent::Blind { depth_mm }),
    }
}

fn require_cut_operation(object: &ObjectRecord) -> Result<()> {
    if !matches!(&object.payload, ObjectPayload::Extrude(e) if e.operation == SolidOperation::Cut) {
        return Err(unsupported(
            "this edit changes a Cut's tool and depth; use edit-extrude for NewBody",
        ));
    }
    Ok(())
}

/// Select by UUID only after validating the entire bounded history.
pub(crate) fn saved_cut(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
) -> Result<SavedCircularCut> {
    require_cut_operation(object)?;
    saved_history(document, objects)?
        .cuts
        .into_iter()
        .find(|c| c.feature == object.id)
        .ok_or_else(|| unsupported("selected Cut is not in the Body's supported history"))
}

/// Exactly a plate plus zero to sixteen cuts. All identities and the complete edge
/// set are derived from links, never names, ordinals or database iteration order.
pub(crate) fn saved_history(document: &Document, objects: &[ObjectRecord]) -> Result<CutHistory> {
    if objects.len() < 4
        || objects.len() > 4 + 2 * MAX_CIRCULAR_CUTS
        || objects.iter().any(|o| o.parent.is_some())
    {
        return Err(unsupported(
            "this editor requires one exact XY plate history with at most 16 circular Cuts",
        ));
    }
    let mut curve_ids = BTreeSet::new();
    for object in objects {
        require_rewritable(object, "history object")?;
        if let ObjectPayload::Sketch(sketch) = &object.payload {
            for curve in &sketch.curves {
                if !curve_ids.insert(curve.id) {
                    return Err(unsupported(format!(
                        "history reuses curve UUID {}",
                        curve.id
                    )));
                }
            }
        }
    }
    let get = |id| {
        objects
            .iter()
            .find(|o| o.id == id)
            .ok_or_else(|| unsupported("a linked history object is missing"))
    };
    let bodies: Vec<_> = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .collect();
    let [body] = bodies.as_slice() else {
        return Err(unsupported("editing a saved cut requires exactly one Body"));
    };
    let ObjectPayload::Body(b) = &body.payload else {
        unreachable!()
    };
    let tip = b
        .tip_feature
        .ok_or_else(|| unsupported("Body has no tip"))?;
    let mut cursor = tip;
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(cursor) {
            return Err(unsupported("cyclic Cut history"));
        }
        let record = get(cursor)?;
        let ObjectPayload::Extrude(cut) = &record.payload else {
            return Err(unsupported("a Cut history link is not an extrusion"));
        };
        if cut.operation == SolidOperation::NewBody {
            break;
        }
        if chain.len() == MAX_CIRCULAR_CUTS {
            return Err(unsupported("this editor supports at most 16 circular Cuts"));
        }
        if cut.operation != SolidOperation::Cut || cut.target_body.is_some() || cut.reversed {
            return Err(unsupported(
                "this slice edits forward Cut links with feature predecessors",
            ));
        }
        chain.push(record);
        cursor = cut
            .previous
            .ok_or_else(|| unsupported("a Cut with no predecessor modifies nothing"))?;
    }
    chain.reverse();
    let base = get(cursor)?;
    let ObjectPayload::Extrude(base_feature) = &base.payload else {
        return Err(unsupported("the history base is not an extrusion"));
    };
    if base_feature.operation != SolidOperation::NewBody
        || base_feature.previous.is_some()
        || base_feature.target_body.is_some()
        || base_feature.reversed
    {
        return Err(unsupported(
            "the history must start with a forward NewBody extrusion",
        ));
    }
    // The exact dependency set below also excludes every parameter edge.
    let height_mm = blind_literal(base_feature, "base extrusion")?;
    let profile = get(base_feature.profile)?;
    let ObjectPayload::Sketch(part) = &profile.payload else {
        return Err(unsupported("the part's profile is not a Sketch"));
    };
    if !part.constraints.is_empty() {
        return Err(unsupported("this slice edits an unconstrained part"));
    }
    let plane = get(part.plane)?;
    if !matches!(&plane.payload, ObjectPayload::DatumPlane(p) if p.placement == Transform::IDENTITY)
    {
        return Err(unsupported(
            "editing a saved cut requires the untransformed XY plane",
        ));
    }
    let extents_mm = rectangle(&part.curves, height_mm)?;
    let segments: Vec<_> = part.curves.iter().map(|c| c.id).collect();
    let refs = document.topology_refs()?;
    let mut expected = BTreeSet::from([
        Dependency {
            dependent: profile.id,
            dependency: plane.id,
            role: DependencyRole::Plane,
        },
        Dependency {
            dependent: base.id,
            dependency: profile.id,
            role: DependencyRole::Profile,
        },
        Dependency {
            dependent: body.id,
            dependency: tip,
            role: DependencyRole::BodyTip,
        },
    ]);
    let mut covered = BTreeSet::from([body.id, base.id, profile.id, plane.id]);
    let mut saved: Vec<SavedCircularCut> = Vec::new();
    for record in chain {
        let ObjectPayload::Extrude(cut) = &record.payload else {
            unreachable!()
        };
        let tool = get(cut.profile)?;
        let ObjectPayload::Sketch(sketch) = &tool.payload else {
            return Err(unsupported("the Cut's profile is not a Sketch"));
        };
        if sketch.plane != plane.id || !sketch.constraints.is_empty() {
            return Err(unsupported(
                "this slice edits an unconstrained tool on the part's base datum",
            ));
        }
        let [curve] = sketch.curves.as_slice() else {
            return Err(unsupported("the Cut's tool Sketch draws one circle"));
        };
        if curve.construction {
            return Err(unsupported("construction geometry cuts nothing"));
        }
        let SketchGeometry::Circle { center, radius } = curve.geometry else {
            return Err(unsupported("this slice edits a circular tool"));
        };
        let extent = saved_extent(cut, "cut")?;
        let numbers = CircularCut {
            center_mm: [center.x, center.y],
            radius_mm: radius,
            extent,
        };
        validate(height_mm, extents_mm, &numbers)?;
        for previous in &saved {
            validate_disks(&numbers, &previous.tool())?;
        }
        let previous = cut.previous.expect("checked history link");
        expected.extend([
            Dependency {
                dependent: tool.id,
                dependency: plane.id,
                role: DependencyRole::Plane,
            },
            Dependency {
                dependent: record.id,
                dependency: tool.id,
                role: DependencyRole::Profile,
            },
            Dependency {
                dependent: record.id,
                dependency: previous,
                role: DependencyRole::Predecessor,
            },
        ]);
        covered.extend([record.id, tool.id]);
        let stored: Vec<_> = refs.iter().filter(|r| r.owner == record.id).collect();
        let wanted = history_references(
            record.id,
            curve.id,
            &segments,
            extent.leaves_a_floor(height_mm),
            base.id,
            &saved.iter().map(SavedCircularCut::tool).collect::<Vec<_>>(),
        );
        let mut remaining = stored.clone();
        for want in wanted {
            let position = remaining.iter().position(|r| same_meaning(r, &want)).ok_or_else(||
                unsupported("the saved Cut does not name the faces this build gives a cut of its numbers"))?;
            remaining.remove(position);
        }
        if !remaining.is_empty() {
            return Err(unsupported(
                "the saved Cut names more faces than a cut of its numbers gives",
            ));
        }
        let floor_reference = stored
            .iter()
            .find(|r| r.output_role == SemanticRole::ExtrudeCap { side: CapSide::End })
            .map(|r| r.id);
        // Protect every saved reference to this floor, including references
        // owned elsewhere and the final output's explicit origin name.
        let protected_floor_references = refs
            .iter()
            .filter(|r| match r.output_role {
                SemanticRole::ExtrudeCap { side: CapSide::End } => r.producer_feature == record.id,
                SemanticRole::OriginCap {
                    origin_feature,
                    side: CapSide::End,
                } => origin_feature == record.id,
                SemanticRole::CarriedCap { side: CapSide::End } => {
                    objects.iter().any(|o| o.id == r.producer_feature && matches!(&o.payload, ObjectPayload::Extrude(e) if e.previous == Some(record.id)))
                }
                _ => false,
            })
            .map(|r| r.id)
            .collect();
        saved.push(SavedCircularCut {
            feature: record.id,
            body: body.id,
            plane: plane.id,
            base_feature: base.id,
            previous_feature: previous,
            tip_feature: tip,
            neighboring_tool: None,
            tools: Vec::new(),
            protected_floor_references,
            profile_sketch: profile.id,
            tool_sketch: tool.id,
            tool_curve: curve.id,
            center_mm: numbers.center_mm,
            radius_mm: radius,
            extent,
            height_mm,
            extents_mm,
            floor_reference,
        });
    }
    if covered != objects.iter().map(|o| o.id).collect() || covered.len() != objects.len() {
        return Err(unsupported(
            "history must use exactly the plate and distinct Cut/tool objects",
        ));
    }
    if document
        .dependencies()?
        .into_iter()
        .collect::<BTreeSet<_>>()
        != expected
    {
        return Err(unsupported(
            "editing a saved cut requires exactly the plane, profile, predecessor and body-tip edges",
        ));
    }
    let tools: Vec<_> = saved.iter().map(SavedCircularCut::tool).collect();
    for cut in &mut saved {
        cut.tools = tools.clone();
        if tools.len() == 2 {
            cut.neighboring_tool = tools.iter().find(|t| t.feature != cut.feature).cloned();
        }
    }
    let target = SavedCutTarget {
        body: body.id,
        plane: plane.id,
        tip_feature: tip,
        base_feature: base.id,
        profile: profile.id,
        profile_segments: segments,
        height_mm,
        extents_mm,
        existing_cut: if saved.len() == 1 {
            saved.first().cloned()
        } else {
            None
        },
        tools,
    };
    Ok(CutHistory {
        target,
        cuts: saved,
    })
}

/// Two references that name the same thing. Identity is deliberately absent:
/// that is what is being looked up, and everything else is what is checked.
fn same_meaning(a: &TopologyRef, b: &TopologyRef) -> bool {
    a.owner == b.owner
        && a.producer_feature == b.producer_feature
        && a.expected_kind == b.expected_kind
        && a.output_role == b.output_role
        && a.selection == b.selection
        && a.fallback_signature == b.fallback_signature
}

/// What one accepted edit does to the saved names, or why it is refused.
///
/// The whole of the pocket/through policy, in one place:
///
/// * a cut that keeps its floor, or keeps having none, changes no name;
/// * a cut that ran through the part and now stops inside it **gains** a floor.
///   Nothing saved is lost; historical/final names are added, and the copy job
///   requires every name an operation adds to resolve — so this is supported
///   rather than refused for symmetry;
/// * a cut that had a floor and would now run through the part **loses** it.
///   The saved reference names the pocket's floor; the face at that depth after
///   the edit is the far side of the part, which is a different face with a
///   different meaning and already has its own name (`CarriedCap`). Deleting
///   the saved reference, or letting it resolve to the part's far side, are
///   both worse than refusing, so this refuses.
fn transition(saved: &SavedCircularCut, edit: &CircularCutEdit) -> Result<Transition> {
    if edit.tool_curve != saved.tool_curve {
        return Err(CadError::input(
            "the request names a curve this Cut does not draw",
        ));
    }
    if edit.vocabulary == ExtentVocabulary::BlindOnly && saved.extent == CutExtent::ThroughAll {
        return Err(unsupported(format!(
            "Cut {} is saved as ThroughAll; request v1 can state only a Blind depth and would \
             discard that intent; use request_version 2 with an explicit extent",
            saved.feature
        )));
    }
    let cut = CircularCut {
        center_mm: edit.center_mm,
        radius_mm: edit.radius_mm,
        extent: edit.extent,
    };
    validate(saved.height_mm, saved.extents_mm, &cut)?;
    for other in saved.tools.iter().filter(|t| t.feature != saved.feature) {
        validate_disks(&cut, other)?;
    }
    floor_transition(
        saved.feature,
        &saved.protected_floor_references,
        edit.extent,
        saved.height_mm,
    )
}

/// Shared by changes to the tool depth and changes to the plate height.
pub(crate) fn floor_transition(
    feature: ObjectId,
    protected: &[StableEntityId],
    extent: CutExtent,
    height_mm: f64,
) -> Result<Transition> {
    let floor = extent.leaves_a_floor(height_mm);
    match (protected.is_empty(), floor) {
        (false, true) | (true, false) => Ok(Transition::Kept),
        (true, true) => Ok(Transition::FloorAppears),
        (false, false) => {
            let protected = protected
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let reason = match extent {
                CutExtent::Blind { depth_mm } => {
                    format!("a depth of {depth_mm} mm in a part {height_mm} mm tall")
                }
                CutExtent::ThroughAll => "ThroughAll".to_owned(),
            };
            Err(unsupported(format!(
                "Cut {feature}: {reason} would remove the pocket floor; saved references [{protected}] cannot be deleted or moved to another face"
            )))
        }
    }
}

/// One own floor and one origin name at each later producer, in history order.
pub(crate) fn added_floor_references(
    feature: ObjectId,
    tools: &[SavedCutTool],
) -> Vec<TopologyRef> {
    tools
        .iter()
        .skip_while(|t| t.feature != feature)
        .map(|t| TopologyRef {
            id: StableEntityId::new(),
            owner: t.feature,
            producer_feature: t.feature,
            expected_kind: EntityKind::Face,
            output_role: if t.feature == feature {
                SemanticRole::ExtrudeCap { side: CapSide::End }
            } else {
                SemanticRole::OriginCap {
                    origin_feature: feature,
                    side: CapSide::End,
                }
            },
            selection: SelectionRule::Exact,
            fallback_signature: None,
        })
        .collect()
}

/// Everything one accepted parameter edit rewrites, prepared before anything
/// is written.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedCutParameters {
    pub(crate) tool_sketch: ObjectRecord,
    pub(crate) feature: ObjectRecord,
    /// The floor's historical/final names when a hole becomes a pocket. Empty
    /// otherwise; this operation never removes one.
    pub(crate) added_references: Vec<TopologyRef>,
    pub(crate) saved: SavedCircularCut,
    pub(crate) edit: CircularCutEdit,
}

impl PreparedCutParameters {
    pub fn tool_sketch(&self) -> &ObjectRecord {
        &self.tool_sketch
    }
    pub fn feature(&self) -> &ObjectRecord {
        &self.feature
    }
    pub fn added_references(&self) -> &[TopologyRef] {
        &self.added_references
    }
    pub fn body(&self) -> ObjectId {
        self.saved.body
    }
    pub fn previous(&self) -> ObjectId {
        self.saved.previous_feature
    }
    pub fn tool_curve(&self) -> StableEntityId {
        self.saved.tool_curve
    }
    /// Whether the edited cut leaves a floor.
    pub fn leaves_a_floor(&self) -> bool {
        self.saved.floor_at(self.edit.extent)
    }
    /// The intent the edited Cut stores.
    pub fn extent(&self) -> CutExtent {
        self.edit.extent
    }
}

/// Prepares one parameter edit of a saved circular cut.
///
/// Mints nothing until every check has passed, so a refused request leaves no
/// identifier behind that a later one might reuse.
pub fn prepare_cut_parameters(
    document: &Document,
    feature: ObjectId,
    edit: &CircularCutEdit,
) -> Result<PreparedCutParameters> {
    let objects = document.objects()?;
    let record = objects
        .iter()
        .find(|o| o.id == feature)
        .ok_or_else(|| CadError::input("selected Cut UUID does not exist"))?;
    let saved = saved_cut(document, &objects, record)?;
    let moved = transition(&saved, edit)?;

    let mut tool = objects
        .iter()
        .find(|o| o.id == saved.tool_sketch)
        .cloned()
        .ok_or_else(|| CadError::input("the Cut's tool Sketch disappeared"))?;
    let ObjectPayload::Sketch(sketch) = &mut tool.payload else {
        unreachable!("checked by saved_cut")
    };
    // Exactly the geometry of the one curve this cut draws with. Its identity
    // and its construction flag are left as they are.
    sketch.curves[0].geometry = SketchGeometry::Circle {
        center: Point2::new(edit.center_mm[0], edit.center_mm[1])?,
        radius: edit.radius_mm,
    };

    let mut cut = record.clone();
    let ObjectPayload::Extrude(extrude) = &mut cut.payload else {
        unreachable!("checked by saved_cut")
    };
    // Exactly the end. The profile, the operation, the predecessor and the
    // absence of a target body are the history, and this edit is not about it.
    // The payload layout follows from what it now holds (v2 Blind, v3
    // ThroughAll); the writer re-derives and records that, nothing else.
    extrude.end_condition = edit.extent.end_condition()?;

    let added_references = match moved {
        Transition::Kept => Vec::new(),
        Transition::FloorAppears => added_floor_references(saved.feature, &saved.tools),
    };

    Ok(PreparedCutParameters {
        tool_sketch: tool,
        feature: cut,
        added_references,
        saved,
        edit: *edit,
    })
}

/// Re-derives a prepared parameter edit from the document it claims to be
/// against.
///
/// The writer's guard, for the reason every other prepared payload here has
/// one: the value is public and mutable, so its fields do not prove what
/// produced them. The numbers are read back out of the prepared payloads
/// themselves, the whole edit is built again from the *current* document, and
/// the two are compared entire — one comparison rather than a list of fields
/// somebody has to keep up to date as the structure grows.
pub(crate) fn rederive_parameters(
    document: &Document,
    prepared: &PreparedCutParameters,
) -> Result<()> {
    let ObjectPayload::Sketch(sketch) = &prepared.tool_sketch.payload else {
        return Err(CadError::input("a prepared cut edit carries a Sketch"));
    };
    let [curve] = sketch.curves.as_slice() else {
        return Err(CadError::input("a prepared cut edit draws one circle"));
    };
    let SketchGeometry::Circle { center, radius } = curve.geometry else {
        return Err(CadError::input("a prepared cut edit draws a Circle"));
    };
    let ObjectPayload::Extrude(extrude) = &prepared.feature.payload else {
        return Err(CadError::input("a prepared cut edit carries an Extrude"));
    };
    let stated = CircularCutEdit {
        tool_curve: curve.id,
        center_mm: [center.x, center.y],
        radius_mm: radius,
        extent: saved_extent(extrude, "prepared cut edit")?,
        // Not in any payload: what the request could say is re-checked against
        // the current saved Cut exactly as preparation checked it.
        vocabulary: prepared.edit.vocabulary,
    };

    let mut checked = prepare_cut_parameters(document, prepared.feature.id, &stated)?;
    // The one thing that cannot be derived again is the identity of a name
    // that did not exist before. Substituted in; everything around it — the
    // owner, the producer, the role and the rule — is what is being checked.
    if checked.added_references.len() != prepared.added_references.len() {
        return Err(CadError::input(
            "the prepared cut edit adds a different number of names than the document would",
        ));
    }
    for (mine, theirs) in checked
        .added_references
        .iter_mut()
        .zip(&prepared.added_references)
    {
        mine.id = theirs.id;
    }
    if checked != *prepared {
        return Err(CadError::input(
            "the prepared cut edit does not describe the document it is being written to",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::{Dependency, DependencyRole, EndCondition, Expression, ObjectKind, SolidOperation};
    use ferritecad_types::ErrorKind;

    /// One plate, written straight into a document: plane, profile, extrusion,
    /// body, and the three edges that record them.
    fn plate(corners: [[f64; 2]; 4], height: f64) -> (tempfile::TempDir, Document, ObjectId) {
        let root = tempfile::tempdir().expect("dir");
        let mut d = Document::create(root.path().join("plate.fcad")).expect("document");
        let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
        d.write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("XY"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            )?;
            let curves = (0..4)
                .map(|i| {
                    Ok(SketchCurve {
                        id: StableEntityId::new(),
                        construction: false,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(corners[i][0], corners[i][1])?,
                            end: Point2::new(corners[(i + 1) % 4][0], corners[(i + 1) % 4][1])?,
                        },
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            w.put_object(
                sketch,
                None,
                1,
                Some("Profile"),
                &ObjectPayload::Sketch(Sketch {
                    plane,
                    curves,
                    constraints: Vec::new(),
                }),
            )?;
            w.put_object(
                extrude,
                None,
                2,
                Some("Extrude1"),
                &ObjectPayload::Extrude(Extrude {
                    profile: sketch,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(height)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    target_body: None,
                    previous: None,
                }),
            )?;
            w.put_object(
                body,
                None,
                3,
                Some("Body"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(extrude),
                }),
            )?;
            for (dependent, dependency, role) in [
                (sketch, plane, DependencyRole::Plane),
                (extrude, sketch, DependencyRole::Profile),
                (body, extrude, DependencyRole::BodyTip),
            ] {
                w.add_dependency(Dependency {
                    dependent,
                    dependency,
                    role,
                })?;
            }
            Ok(())
        })
        .expect("fixture");
        (root, d, body)
    }

    fn rectangle() -> [[f64; 2]; 4] {
        [[0., 0.], [60., 0.], [60., 40.], [0., 40.]]
    }
    fn cut(depth: f64) -> CircularCut {
        CircularCut {
            center_mm: [20., 15.],
            radius_mm: 5.,
            extent: CutExtent::Blind { depth_mm: depth },
        }
    }

    #[test]
    fn a_feature_names_the_result_it_modifies_and_a_body_names_its_tip() {
        let (_root, mut d, body) = plate(rectangle(), 10.);
        let prepared = prepare_circular_cut(&d, body, &cut(4.)).expect("a cut");

        // The feature names a feature, never a body, and the edge that records
        // it says so. This is the whole of what keeps the graph orderable.
        let ObjectPayload::Extrude(feature) = &prepared.feature().payload else {
            panic!("a cut is an extrude-shaped feature")
        };
        assert_eq!(feature.previous, Some(prepared.previous()));
        assert_eq!(feature.target_body, None, "a feature does not name a body");
        assert_eq!(feature.operation, SolidOperation::Cut);
        assert!(prepared.added_dependencies.contains(&Dependency {
            dependent: prepared.feature().id,
            dependency: prepared.previous(),
            role: DependencyRole::Predecessor,
        }));
        // And the body names the new feature, replacing the edge to the old one.
        let ObjectPayload::Body(moved) = &prepared.body().payload else {
            panic!("the body is a body")
        };
        assert_eq!(moved.tip_feature, Some(prepared.feature().id));
        assert!(prepared.added_dependencies.contains(&Dependency {
            dependent: body,
            dependency: prepared.feature().id,
            role: DependencyRole::BodyTip,
        }));
        assert_eq!(
            prepared.removed_dependencies,
            vec![Dependency {
                dependent: body,
                dependency: prepared.previous(),
                role: DependencyRole::BodyTip,
            }]
        );

        d.write_circular_cut(&prepared).expect("write");
        assert!(
            d.validate().expect("validate").is_ok(),
            "the copy validates"
        );

        // Written down, the feature is a v2 payload declaring the capability a
        // reader needs; the feature that was there is untouched at v1.
        let objects = d.objects().expect("objects");
        let written = objects
            .iter()
            .find(|o| o.id == prepared.feature().id)
            .expect("the cut");
        assert_eq!(written.payload.schema_version(), 2);
        assert_eq!(
            written.payload.required_capabilities(),
            vec![
                crate::CORE_CAPABILITY.to_owned(),
                crate::FEATURE_PREDECESSOR_CAPABILITY.to_owned()
            ]
        );
        let older = objects
            .iter()
            .find(|o| o.id == prepared.previous())
            .expect("the plate's own feature");
        assert_eq!(older.payload.schema_version(), 1);
        assert_eq!(
            older.payload.required_capabilities(),
            vec![crate::CORE_CAPABILITY.to_owned()]
        );
        // A build that predates this one reads no v2 extrusion, which is the
        // refusal that protects the data; v3 is the ThroughAll Cut layout.
        assert_eq!(ObjectKind::Extrude.readable_schema_versions(), &[3, 2, 1]);
    }

    #[test]
    fn naming_a_body_instead_of_a_predecessor_cannot_be_ordered_at_all() {
        // The shape the old field would have forced. Written by hand because
        // nothing in this build produces it, and kept as a test because it is
        // the reason the new field exists.
        let (_root, mut d, body) = plate(rectangle(), 10.);
        let tip = match &d
            .objects()
            .expect("objects")
            .iter()
            .find(|o| o.id == body)
            .expect("the body")
            .payload
        {
            ObjectPayload::Body(b) => b.tip_feature.expect("a tip"),
            _ => panic!("a body"),
        };
        let cut = ObjectId::new();
        let sketch = d
            .objects()
            .expect("objects")
            .iter()
            .find_map(|o| match &o.payload {
                ObjectPayload::Sketch(_) => Some(o.id),
                _ => None,
            })
            .expect("a sketch");
        d.write(|w| {
            w.put_object(
                cut,
                None,
                4,
                Some("Cut"),
                &ObjectPayload::Extrude(Extrude {
                    profile: sketch,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(4.)?,
                    },
                    reversed: false,
                    operation: SolidOperation::Cut,
                    target_body: Some(body),
                    previous: None,
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: cut,
                dependency: sketch,
                role: DependencyRole::Profile,
            })?;
            w.add_dependency(Dependency {
                dependent: cut,
                dependency: body,
                role: DependencyRole::TargetBody,
            })?;
            w.put_object(
                body,
                None,
                3,
                Some("Body"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(cut),
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: body,
                dependency: cut,
                role: DependencyRole::BodyTip,
            })?;
            w.remove_dependency(Dependency {
                dependent: body,
                dependency: tip,
                role: DependencyRole::BodyTip,
            })
        })
        .expect("writes the older shape");

        let report = d.validate().expect("validate");
        assert!(
            !report.is_ok(),
            "a feature-to-body edge closes the tip loop"
        );
        let said = format!("{:?}", report);
        assert!(said.contains("cycle"), "{said}");
    }

    #[test]
    fn one_history_per_body_and_one_body_per_tip() {
        let (_root, mut d, body) = plate(rectangle(), 10.);
        let first = prepare_circular_cut(&d, body, &cut(4.)).expect("a cut");
        d.write_circular_cut(&first).expect("write");

        // A second feature modifying the same earlier result is a forked
        // history, and a body has one.
        let fork = ObjectId::new();
        let sketch = first.sketch().id;
        d.write(|w| {
            w.put_object(
                fork,
                None,
                9,
                Some("Cut2"),
                &ObjectPayload::Extrude(Extrude {
                    profile: sketch,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(2.)?,
                    },
                    reversed: false,
                    operation: SolidOperation::Cut,
                    target_body: None,
                    previous: Some(first.previous()),
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: fork,
                dependency: sketch,
                role: DependencyRole::Profile,
            })?;
            w.add_dependency(Dependency {
                dependent: fork,
                dependency: first.previous(),
                role: DependencyRole::Predecessor,
            })
        })
        .expect("writes a fork");
        let said = format!("{:?}", d.validate().expect("validate"));
        assert!(said.contains("forked-history"), "{said}");
    }

    #[test]
    fn the_writer_refuses_a_prepared_cut_the_document_would_not_have_made() {
        let (_root, mut d, body) = plate(rectangle(), 10.);
        let honest = prepare_circular_cut(&d, body, &cut(4.)).expect("a cut");

        // 1. A bigger tool than the one the numbers describe.
        let mut wider = honest.clone();
        if let ObjectPayload::Sketch(s) = &mut wider.sketch.payload {
            s.curves[0].geometry = SketchGeometry::Circle {
                center: Point2::new(20., 15.).expect("centre"),
                radius: 400.,
            };
        }
        assert_eq!(
            d.write_circular_cut(&wider).expect_err("refused").kind(),
            ErrorKind::Input
        );

        // 2. A body whose tip is moved somewhere the preparation did not put it.
        let mut elsewhere = honest.clone();
        if let ObjectPayload::Body(b) = &mut elsewhere.body.payload {
            b.tip_feature = Some(elsewhere.previous);
        }
        assert_eq!(
            d.write_circular_cut(&elsewhere)
                .expect_err("refused")
                .kind(),
            ErrorKind::Input
        );

        // 3. A reference claiming to be produced by somebody else's feature.
        let mut stolen = honest.clone();
        stolen.references[0].producer_feature = stolen.previous;
        assert_eq!(
            d.write_circular_cut(&stolen).expect_err("refused").kind(),
            ErrorKind::Input
        );

        // 4. A feature that keeps the payload but drops the predecessor.
        let mut orphan = honest.clone();
        if let ObjectPayload::Extrude(e) = &mut orphan.feature.payload {
            e.previous = None;
        }
        assert!(d.write_circular_cut(&orphan).is_err());

        // The honest one still writes, and nothing above changed the document.
        assert_eq!(d.objects().expect("objects").len(), 4);
        d.write_circular_cut(&honest).expect("the honest plan");
        assert_eq!(d.objects().expect("objects").len(), 6);
    }

    #[test]
    fn bodies_cannot_own_overlapping_feature_histories() {
        for same_tip in [false, true] {
            let (_root, mut d, body) = plate(rectangle(), 10.);
            let prepared = prepare_circular_cut(&d, body, &cut(4.)).expect("cut");
            d.write_circular_cut(&prepared).expect("write");
            assert!(d.validate().expect("valid history").is_ok());
            let other = ObjectId::new();
            let tip = if same_tip {
                prepared.feature().id
            } else {
                prepared.previous()
            };
            d.write(|w| {
                w.put_object(
                    other,
                    None,
                    20,
                    Some("Another body"),
                    &ObjectPayload::Body(Body {
                        tip_feature: Some(tip),
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: other,
                    dependency: tip,
                    role: DependencyRole::BodyTip,
                })
            })
            .expect("overlapping body");
            let report = d.validate().expect("validate");
            let code = if same_tip {
                "body.shared-tip"
            } else {
                "body.shared-history"
            };
            assert!(report.errors().any(|d| d.code == code), "{report:?}");
        }
    }

    #[test]
    fn unknown_body_fields_refuse_before_the_tip_is_rewritten() {
        let (root, d, body) = plate(rectangle(), 10.);
        let record = d.object(body).expect("row").expect("body");
        let mut envelope = crate::Envelope::from_bytes(record.storage_bytes()).expect("envelope");
        let mut value: ciborium::value::Value =
            ciborium::from_reader(envelope.payload.as_slice()).expect("CBOR");
        value
            .as_map_mut()
            .expect("map")
            .push(("future_body_detail".into(), "retain me".into()));
        envelope.payload.clear();
        ciborium::into_writer(&value, &mut envelope.payload).expect("CBOR");
        let bytes = envelope.to_bytes().expect("envelope");
        d.close().expect("close");
        let path = root.path().join("plate.fcad");
        let sql = rusqlite::Connection::open(&path).expect("SQL");
        sql.execute(
            "UPDATE objects SET payload=?1,payload_hash=?2 WHERE id=?3",
            rusqlite::params![
                bytes,
                ferritecad_types::ContentHash::of_bytes(&bytes)
                    .as_bytes()
                    .as_slice(),
                body.to_bytes().as_slice()
            ],
        )
        .expect("future field");
        drop(sql);
        let before = std::fs::read(&path).expect("source");
        let d = Document::open_read_only(&path).expect("readable");
        assert!(d.validate().expect("valid").is_ok());
        let reading = crate::ExtrudeEditSource::read(&d).expect("discovery");
        assert!(
            reading.cut_bodies[0]
                .refusal
                .as_ref()
                .is_some_and(|s| s.contains("losslessly"))
        );
        let error =
            prepare_circular_cut(&d, body, &cut(4.)).expect_err("do not discard unknown fields");
        assert!(error.to_string().contains("losslessly"), "{error}");
        d.close().expect("close");
        assert_eq!(std::fs::read(path).expect("source"), before);
    }

    #[test]
    fn exhausted_object_ordinals_refuse_without_wrapping() {
        for ordinal in [i64::MAX - 2, i64::MAX - 1, i64::MAX] {
            let (_root, mut d, body) = plate(rectangle(), 10.);
            let row = d.object(body).expect("row").expect("body");
            d.write(|w| w.put_object(body, row.parent, ordinal, row.name.as_deref(), &row.payload))
                .expect("large ordinal");
            let before = d.objects().expect("before");
            let result = prepare_circular_cut(&d, body, &cut(4.));
            if ordinal == i64::MAX - 2 {
                let prepared = result.expect("two slots remain");
                assert_eq!(prepared.sketch().ordinal, i64::MAX - 1);
                assert_eq!(prepared.feature().ordinal, i64::MAX);
            } else {
                let error = result.expect_err("two new object ordinals cannot fit");
                assert!(error.to_string().contains("ordinal"), "{error}");
            }
            assert_eq!(d.objects().expect("after"), before);
        }
    }

    #[test]
    fn a_cut_is_not_offered_as_an_editable_extrusion() {
        let (_root, mut d, body) = plate(rectangle(), 10.);
        let prepared = prepare_circular_cut(&d, body, &cut(4.)).expect("cut");
        d.write_circular_cut(&prepared).expect("write");
        let source = crate::ExtrudeEditSource::read(&d).expect("discovery");
        assert!(
            source
                .features
                .iter()
                .find(|f| f.feature == prepared.previous())
                .expect("base")
                .refusal
                .is_none()
        );
        assert!(
            source
                .features
                .iter()
                .find(|f| f.feature == prepared.feature().id)
                .expect("cut")
                .refusal
                .is_some()
        );
        let row = d.object(prepared.feature().id).expect("row").expect("cut");
        assert!(crate::editable_extrude(&d, &row).is_err());
    }

    #[test]
    fn the_supported_class_is_stated_and_anything_wider_says_why() {
        // A part that is not an axis-aligned rectangle.
        let (_root, d, body) = plate([[0., 0.], [60., 5.], [60., 40.], [0., 40.]], 10.);
        let error = prepare_circular_cut(&d, body, &cut(4.)).expect_err("not a rectangle");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert!(error.to_string().contains("at an angle"), "{error}");

        // A part with constraints on its profile.
        let (_root, mut d, body) = plate(rectangle(), 10.);
        let sketch = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("a sketch");
        let ObjectPayload::Sketch(mut drawing) = sketch.payload.clone() else {
            panic!("a sketch")
        };
        let curve = drawing.curves[0].id;
        drawing.constraints.push(crate::SketchConstraint {
            id: StableEntityId::new(),
            rule: crate::SketchConstraintRule::Horizontal {
                a: crate::SketchPointRef::new(curve, crate::SketchPointSelector::Start),
                b: crate::SketchPointRef::new(curve, crate::SketchPointSelector::End),
            },
        });
        d.write(|w| {
            w.put_object(
                sketch.id,
                None,
                1,
                Some("Profile"),
                &ObjectPayload::Sketch(drawing),
            )
        })
        .expect("constrains the profile");
        let error = prepare_circular_cut(&d, body, &cut(4.)).expect_err("constrained");
        assert!(error.to_string().contains("unconstrained"), "{error}");
    }

    #[test]
    fn the_tool_must_stay_inside_the_part_by_more_than_the_kernels_own_tolerance() {
        let (_root, d, body) = plate(rectangle(), 10.);
        let target = cut_choices(&d, &d.objects().expect("objects"))
            .into_iter()
            .find(|c| c.body == body)
            .expect("the body");
        // Exactly touching is refused, and so is anything closer.
        for center in [[5.0, 15.0], [5.0 - 1e-9, 15.0], [4.0, 15.0]] {
            let error = target
                .validate_cut(&CircularCut {
                    center_mm: center,
                    radius_mm: 5.,
                    extent: CutExtent::Blind { depth_mm: 4. },
                })
                .expect_err("a tool reaching the wall");
            assert_eq!(error.kind(), ErrorKind::Input, "{center:?}");
        }
        // Clear of it by more than the tolerance is accepted.
        target
            .validate_cut(&CircularCut {
                center_mm: [5.0 + 1e-3, 15.0],
                radius_mm: 5.,
                extent: CutExtent::Blind { depth_mm: 4. },
            })
            .expect("clear of the wall");
        // Through is the deepest a cut goes here, and it is allowed.
        target.validate_cut(&cut(10.)).expect("a through hole");
        assert_eq!(
            target
                .validate_cut(&cut(10.001))
                .expect_err("too deep")
                .kind(),
            ErrorKind::Input
        );
    }

    /// A plate with one cut written into it, and the cut's identity.
    fn cut_plate(depth: f64) -> (tempfile::TempDir, Document, ObjectId) {
        let (root, mut d, body) = plate(rectangle(), 10.);
        let prepared = prepare_circular_cut(&d, body, &cut(depth)).expect("a cut");
        let feature = prepared.feature().id;
        d.write_circular_cut(&prepared).expect("write");
        (root, d, feature)
    }
    fn moved(saved: &SavedCircularCut, depth: f64) -> CircularCutEdit {
        CircularCutEdit {
            tool_curve: saved.tool_curve,
            center_mm: [30., 20.],
            radius_mm: 7.5,
            extent: CutExtent::Blind { depth_mm: depth },
            vocabulary: ExtentVocabulary::BlindOrThroughAll,
        }
    }
    fn only(d: &Document) -> SavedCircularCut {
        let objects = d.objects().expect("objects");
        cut_parameter_choices(d, &objects)
            .into_iter()
            .find_map(|c| c.saved)
            .expect("one editable cut")
    }

    #[test]
    fn a_saved_cut_is_found_through_links_and_types_and_never_by_name_or_order() {
        let (_root, mut d, feature) = cut_plate(4.);
        let saved = only(&d);
        assert_eq!(saved.feature, feature);
        assert_eq!(saved.center_mm, [20., 15.]);
        assert_eq!(saved.radius_mm, 5.);
        assert_eq!(saved.extent.blind_depth_mm().expect("blind"), 4.);
        assert_eq!(saved.height_mm, 10.);
        assert_eq!(saved.extents_mm, [[0., 0.], [60., 40.]]);
        assert!(saved.floor_reference.is_some(), "a pocket has a floor");
        assert!(!saved.through_allowed());

        // The two sketches are told apart by what links to them, not by their
        // names or their row order: renaming both to the same thing and
        // rewriting the tool's ordinal to sit first changes no answer.
        let objects = d.objects().expect("objects");
        let tool = objects
            .iter()
            .find(|o| o.id == saved.tool_sketch)
            .expect("tool")
            .clone();
        let profile = objects
            .iter()
            .find(|o| o.id == saved.profile_sketch)
            .expect("profile")
            .clone();
        d.write(|w| {
            w.put_object(tool.id, None, -1, Some("Sketch"), &tool.payload)?;
            w.put_object(profile.id, None, 9, Some("Sketch"), &profile.payload)?;
            Ok(())
        })
        .expect("renamed and reordered");
        let again = only(&d);
        assert_eq!(again.tool_sketch, saved.tool_sketch);
        assert_eq!(again.profile_sketch, saved.profile_sketch);
        assert_eq!(again.tool_curve, saved.tool_curve);
        assert_eq!(again.center_mm, saved.center_mm);

        // And the extrusion that started the body is refused here, so the
        // generic distance edit keeps owning it.
        let objects = d.objects().expect("objects");
        let base = objects
            .iter()
            .find(|o| o.id == saved.base_feature)
            .expect("base");
        assert_eq!(
            saved_cut(&d, &objects, base).expect_err("refused").kind(),
            ErrorKind::Unsupported
        );
        assert!(crate::editable_extrude(&d, base).is_ok());
    }

    #[test]
    fn a_saved_pocket_floor_may_not_be_cut_away_and_a_hole_may_gain_one() {
        // A pocket: shallower, deeper and moved are all fine; through is not.
        let (_root, d, _) = cut_plate(4.);
        let saved = only(&d);
        let floor = saved.floor_reference.expect("a floor");
        for depth in [0.5, 4., 9.9] {
            prepare_cut_parameters(&d, saved.feature, &moved(&saved, depth)).expect("a pocket");
        }
        let refused = prepare_cut_parameters(&d, saved.feature, &moved(&saved, 10.))
            .expect_err("cutting the floor away");
        assert_eq!(refused.kind(), ErrorKind::Unsupported);
        assert!(
            refused.to_string().contains(&floor.to_string()),
            "the refusal names the reference it is protecting: {refused}"
        );

        // A hole: shortening it adds the floor's name and takes none away.
        let (_root, d, _) = cut_plate(10.);
        let saved = only(&d);
        assert!(saved.floor_reference.is_none() && saved.through_allowed());
        let deep = prepare_cut_parameters(&d, saved.feature, &moved(&saved, 10.)).expect("through");
        assert!(deep.added_references().is_empty());
        assert!(!deep.leaves_a_floor());
        let shallow =
            prepare_cut_parameters(&d, saved.feature, &moved(&saved, 3.)).expect("a pocket");
        assert!(shallow.leaves_a_floor());
        let [added] = shallow.added_references() else {
            panic!("a pocket gains exactly one name")
        };
        assert_eq!(added.owner, saved.feature);
        assert_eq!(added.producer_feature, saved.feature);
        assert_eq!(
            added.output_role,
            SemanticRole::ExtrudeCap { side: CapSide::End }
        );
        assert_eq!(added.selection, SelectionRule::Exact);
    }

    #[test]
    fn editing_a_cut_changes_its_numbers_and_nothing_it_is_made_of() {
        let (_root, mut d, feature) = cut_plate(4.);
        let saved = only(&d);
        let before = d.objects().expect("objects");
        let refs_before = d.topology_refs().expect("refs");
        let prepared =
            prepare_cut_parameters(&d, feature, &moved(&saved, 6.)).expect("a prepared edit");
        d.write_cut_parameters(&prepared).expect("write");

        let after = only(&d);
        assert_eq!(after.feature, saved.feature);
        assert_eq!(after.body, saved.body);
        assert_eq!(after.plane, saved.plane);
        assert_eq!(after.base_feature, saved.base_feature);
        assert_eq!(after.profile_sketch, saved.profile_sketch);
        assert_eq!(after.tool_sketch, saved.tool_sketch);
        assert_eq!(after.tool_curve, saved.tool_curve);
        assert_eq!(after.floor_reference, saved.floor_reference);
        assert_eq!(after.center_mm, [30., 20.]);
        assert_eq!(after.radius_mm, 7.5);
        assert_eq!(after.extent.blind_depth_mm().expect("blind"), 6.);
        assert!(d.validate().expect("validated").is_ok());

        // Only two payloads moved, and only their payload cells.
        let now = d.objects().expect("objects");
        assert_eq!(now.len(), before.len());
        for old in &before {
            let new = now.iter().find(|o| o.id == old.id).expect("same object");
            assert_eq!(old.parent, new.parent);
            assert_eq!(old.ordinal, new.ordinal);
            assert_eq!(old.name, new.name);
            if old.id == saved.tool_sketch || old.id == saved.feature {
                assert_ne!(old.payload, new.payload, "the edit changed nothing");
                assert_eq!(
                    old.payload.schema_version(),
                    new.payload.schema_version(),
                    "an edit of numbers does not move a contract"
                );
            } else {
                assert_eq!(old, new, "an untouched object moved");
            }
        }
        // The history itself is exactly what it was.
        let ObjectPayload::Extrude(edited) = &now
            .iter()
            .find(|o| o.id == saved.feature)
            .expect("the cut")
            .payload
        else {
            panic!("the cut is an extrusion")
        };
        assert_eq!(edited.operation, SolidOperation::Cut);
        assert_eq!(edited.previous, Some(saved.base_feature));
        assert!(edited.target_body.is_none());
        assert_eq!(edited.profile, saved.tool_sketch);
        assert_eq!(d.topology_refs().expect("refs"), refs_before);
        assert_eq!(d.dependencies().expect("deps").len(), 6);
    }

    #[test]
    fn the_writer_refuses_a_prepared_cut_edit_the_document_would_not_have_made() {
        let (_root, mut d, feature) = cut_plate(4.);
        let saved = only(&d);
        let honest = prepare_cut_parameters(&d, feature, &moved(&saved, 6.)).expect("an edit");
        let objects = d.objects().expect("objects");

        // 1. A tool that reaches past the part, which the rule would refuse.
        let mut wider = honest.clone();
        if let ObjectPayload::Sketch(s) = &mut wider.tool_sketch.payload {
            s.curves[0].geometry = SketchGeometry::Circle {
                center: Point2::new(30., 20.).expect("centre"),
                radius: 400.,
            };
        }
        assert_eq!(
            d.write_cut_parameters(&wider).expect_err("refused").kind(),
            ErrorKind::Input
        );

        // 2. A depth that would cut the saved floor away, smuggled in through
        //    the payload rather than asked for.
        let mut through = honest.clone();
        if let ObjectPayload::Extrude(e) = &mut through.feature.payload {
            e.end_condition = EndCondition::Blind {
                distance: Expression::constant(10.).expect("depth"),
            };
        }
        assert!(d.write_cut_parameters(&through).is_err());

        // 3. A payload that keeps the numbers but rewrites the history.
        let mut orphan = honest.clone();
        if let ObjectPayload::Extrude(e) = &mut orphan.feature.payload {
            e.previous = None;
        }
        assert!(d.write_cut_parameters(&orphan).is_err());

        // 4. A curve given a new identity, which would lose the name hung on it.
        let mut renamed = honest.clone();
        if let ObjectPayload::Sketch(s) = &mut renamed.tool_sketch.payload {
            s.curves[0].id = StableEntityId::new();
        }
        assert!(d.write_cut_parameters(&renamed).is_err());

        // 5. A feature swapped for one this edit was never prepared against.
        let mut elsewhere = honest.clone();
        elsewhere.feature.id = saved.base_feature;
        assert!(d.write_cut_parameters(&elsewhere).is_err());

        // Nothing above changed the document, and the honest plan still writes.
        assert_eq!(d.objects().expect("objects"), objects);
        d.write_cut_parameters(&honest).expect("the honest plan");
        assert_eq!(only(&d).radius_mm, 7.5);
    }

    #[test]
    fn the_class_this_edit_accepts_is_stated_and_anything_wider_says_why() {
        // A second cut makes the history longer than this slice edits.
        let (_root, mut d, feature) = cut_plate(4.);
        let saved = only(&d);
        let objects = d.objects().expect("objects");
        let stray = ObjectId::new();
        d.write(|w| {
            w.put_object(
                stray,
                None,
                9,
                Some("Spare"),
                &ObjectPayload::Sketch(Sketch {
                    plane: saved.plane,
                    curves: Vec::new(),
                    constraints: Vec::new(),
                }),
            )?;
            Ok(())
        })
        .expect("a seventh object");
        assert_eq!(
            prepare_cut_parameters(&d, feature, &moved(&saved, 6.))
                .expect_err("seven objects")
                .kind(),
            ErrorKind::Unsupported
        );

        // A request naming a curve this cut does not draw is refused with
        // everything else about it valid.
        let (_root, d, feature) = cut_plate(4.);
        let saved = only(&d);
        let foreign = CircularCutEdit {
            tool_curve: StableEntityId::new(),
            ..moved(&saved, 6.)
        };
        assert_eq!(
            prepare_cut_parameters(&d, feature, &foreign)
                .expect_err("a foreign curve")
                .kind(),
            ErrorKind::Input
        );
        // As is a feature UUID that is not in the document at all.
        assert_eq!(
            prepare_cut_parameters(&d, ObjectId::new(), &moved(&saved, 6.))
                .expect_err("a foreign feature")
                .kind(),
            ErrorKind::Input
        );

        // A tool the numbers put outside the part, or exactly on its wall.
        for (center, radius, why) in [
            ([5., 15.], 5., "touching the wall exactly"),
            ([2., 15.], 5., "hanging off the edge"),
            ([200., 200.], 5., "missing the part"),
            ([20., 15.], 0., "no radius"),
        ] {
            assert!(
                prepare_cut_parameters(
                    &d,
                    feature,
                    &CircularCutEdit {
                        tool_curve: saved.tool_curve,
                        center_mm: center,
                        radius_mm: radius,
                        extent: CutExtent::Blind { depth_mm: 4. },
                        vocabulary: ExtentVocabulary::BlindOrThroughAll,
                    }
                )
                .is_err(),
                "{why} was accepted"
            );
        }
        // And an unchanged document after every one of them.
        assert_eq!(only(&d), saved);
        let _ = objects;
    }

    #[test]
    fn a_cut_whose_names_are_not_the_ones_this_build_gives_is_refused() {
        // A saved cut carrying one name this build would not have minted is not
        // a cut this build made, and editing its numbers would be guessing.
        let (_root, mut d, feature) = cut_plate(4.);
        let saved = only(&d);
        let extra = TopologyRef {
            id: StableEntityId::new(),
            owner: saved.feature,
            producer_feature: saved.feature,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::ExtrudeCap {
                side: CapSide::Start,
            },
            selection: SelectionRule::Exact,
            fallback_signature: None,
        };
        d.write(|w| w.put_topology_ref(&extra)).expect("one more");
        let objects = d.objects().expect("objects");
        let record = objects.iter().find(|o| o.id == feature).expect("the cut");
        assert_eq!(
            saved_cut(&d, &objects, record).expect_err("refused").kind(),
            ErrorKind::Unsupported
        );
    }

    #[test]
    fn a_new_floor_reference_cannot_overwrite_an_existing_name() {
        let (_root, mut d, feature) = cut_plate(10.);
        let saved = only(&d);
        let prepared = prepare_cut_parameters(&d, feature, &moved(&saved, 3.)).expect("edit");
        // A legitimate document write between preparation and application can
        // occupy that identity without changing either edited object.
        let occupied = TopologyRef {
            id: prepared.added_references()[0].id,
            owner: saved.base_feature,
            producer_feature: saved.base_feature,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::ExtrudeCap {
                side: CapSide::Start,
            },
            selection: SelectionRule::Exact,
            fallback_signature: None,
        };
        d.write(|w| w.put_topology_ref(&occupied))
            .expect("occupied identity");
        let before = d.content_version().expect("version");
        let error = d
            .write_cut_parameters(&prepared)
            .expect_err("must not replace a saved name");
        assert_eq!(error.kind(), ErrorKind::Input);
        assert!(error.to_string().contains(&occupied.id.to_string()));
        assert_eq!(
            d.content_version().expect("version"),
            before,
            "entire write rolled back"
        );
        assert!(d.topology_refs().expect("refs").contains(&occupied));
    }

    #[test]
    fn cut_catalog_keeps_operation_refusals_independent_of_history_shape() {
        for corners in [rectangle(), [[0., 0.], [60., 2.], [60., 40.], [0., 40.]]] {
            let (_root, mut d, _) = plate(corners, 10.);
            for extra_body in [false, true] {
                if extra_body {
                    d.write(|w| {
                        w.put_object(
                            ObjectId::new(),
                            None,
                            4,
                            Some("Extra Body"),
                            &ObjectPayload::Body(Body { tip_feature: None }),
                        )
                    })
                    .expect("extra Body");
                }
                let objects = d.objects().expect("objects");
                let choices = cut_parameter_choices(&d, &objects);
                assert_eq!(choices.len(), 1);
                let choice = &choices[0];
                let feature = objects
                    .iter()
                    .find(|o| o.id == choice.feature)
                    .expect("feature");
                let refusal = saved_cut(&d, &objects, feature)
                    .expect_err("NewBody")
                    .to_string();
                assert!(refusal.contains("use edit-extrude for NewBody"));
                assert_eq!(choice.refusal.as_deref(), Some(refusal.as_str()));
                assert!(choice.saved.is_none());
            }
        }
    }

    #[test]
    fn saved_cut_discovery_checks_the_stored_tool_against_cut_policy() {
        for (center, radius, depth) in [
            ([2., 15.], 5., 4.),
            ([20., 15.], 30., 4.),
            ([20., 15.], 5., 10.001),
        ] {
            let (_root, mut d, feature) = cut_plate(if depth > 10. { 10. } else { 4. });
            let saved = only(&d);
            let mut tool = d.object(saved.tool_sketch).expect("read").expect("tool");
            if let ObjectPayload::Sketch(sketch) = &mut tool.payload {
                sketch.curves[0].geometry = SketchGeometry::Circle {
                    center: Point2::new(center[0], center[1]).expect("point"),
                    radius,
                };
            }
            let mut cut = d.object(feature).expect("read").expect("cut");
            if let ObjectPayload::Extrude(e) = &mut cut.payload {
                e.end_condition = EndCondition::Blind {
                    distance: Expression::constant(depth).expect("depth"),
                };
            }
            d.write(|w| {
                for row in [&tool, &cut] {
                    w.put_object(
                        row.id,
                        row.parent,
                        row.ordinal,
                        row.name.as_deref(),
                        &row.payload,
                    )?;
                }
                Ok(())
            })
            .expect("stored invalid tool");
            let before = d.content_version().expect("version");
            let objects = d.objects().expect("objects");
            let choice = cut_parameter_choices(&d, &objects)
                .into_iter()
                .find(|c| c.feature == feature)
                .expect("choice");
            assert!(
                choice.saved.is_none() && choice.refusal.is_some(),
                "out-of-policy stored tool was offered: {choice:?}"
            );
            assert!(prepare_cut_parameters(&d, feature, &moved(&saved, 6.)).is_err());
            assert_eq!(d.content_version().expect("version"), before);
        }
    }
    #[test]
    fn second_cut_writer_rechecks_policy_and_rejects_reference_collisions() {
        let (_root, mut d, first) = cut_plate(4.);
        let saved = only(&d);
        let second = CircularCut {
            center_mm: [45., 25.],
            radius_mm: 6.,
            extent: CutExtent::Blind { depth_mm: 7. },
        };
        let honest = prepare_circular_cut(&d, saved.body, &second).expect("second");
        let objects = d.objects().expect("objects");
        let refs = d.topology_refs().expect("refs");
        let mut forged = honest.clone();
        forged.references[0].id = refs[0].id;
        assert!(
            d.write_circular_cut(&forged)
                .expect_err("collision")
                .to_string()
                .contains("already exists")
        );
        let mut forged = honest.clone();
        forged.references[1].id = forged.references[0].id;
        assert!(d.write_circular_cut(&forged).is_err());
        let mut forged = honest.clone();
        if let ObjectPayload::Sketch(s) = &mut forged.sketch.payload {
            s.curves[0].geometry = SketchGeometry::Circle {
                center: Point2::new(20., 15.).expect("point"),
                radius: 6.,
            };
        }
        assert!(d.write_circular_cut(&forged).is_err());
        let mut forged = honest.clone();
        if let SemanticRole::OriginCap { origin_feature, .. } =
            &mut forged.references[2].output_role
        {
            *origin_feature = first;
        }
        assert!(d.write_circular_cut(&forged).is_err());
        assert_eq!(d.objects().expect("objects"), objects);
        assert_eq!(d.topology_refs().expect("refs"), refs);
        d.write_circular_cut(&honest).expect("honest second");
        assert!(d.validate().expect("validator").is_ok());
        assert!(
            cut_parameter_choices(&d, &d.objects().expect("objects"))
                .iter()
                .filter(|c| c.saved.is_some())
                .count()
                == 2
        );
        assert!(prepare_circular_cut(&d, saved.body, &second).is_err());
    }
    #[test]
    fn sequential_edit_floor_policy_and_writer_forgery_are_checked_before_writing() {
        for first_depth in [4., 10.] {
            for second_depth in [7., 10.] {
                let (_root, mut d, first) = cut_plate(first_depth);
                let body = only(&d).body;
                let p = prepare_circular_cut(
                    &d,
                    body,
                    &CircularCut {
                        center_mm: [45., 25.],
                        radius_mm: 6.,
                        extent: CutExtent::Blind {
                            depth_mm: second_depth,
                        },
                    },
                )
                .expect("second");
                let second = p.feature().id;
                d.write_circular_cut(&p).expect("second write");
                let history = saved_history(&d, &d.objects().expect("objects"))
                    .expect("history")
                    .cuts;
                assert_eq!(history.len(), 2);
                assert_eq!(history[0].feature, first);
                assert_eq!(history[1].feature, second);
                for saved in history {
                    let edit = CircularCutEdit {
                        tool_curve: saved.tool_curve,
                        center_mm: saved.center_mm,
                        radius_mm: saved.radius_mm,
                        extent: CutExtent::Blind { depth_mm: 3. },
                        vocabulary: ExtentVocabulary::BlindOrThroughAll,
                    };
                    assert_eq!(
                        saved.previous_feature,
                        if saved.feature == first {
                            saved.base_feature
                        } else {
                            first
                        }
                    );
                    assert_eq!(saved.tip_feature, second);
                    let before = d.content_version().expect("version");
                    let honest = prepare_cut_parameters(&d, saved.feature, &edit).expect("edit");
                    let count = if saved.extent.blind_depth_mm().expect("blind") == 10. {
                        if saved.feature == first { 2 } else { 1 }
                    } else {
                        0
                    };
                    assert_eq!(honest.added_references.len(), count);
                    if count == 2 {
                        let mut duplicate = honest.clone();
                        duplicate.added_references[1].id = duplicate.added_references[0].id;
                        assert!(
                            d.write_cut_parameters(&duplicate)
                                .expect_err("duplicate IDs")
                                .to_string()
                                .contains("duplicate")
                        );
                        let mut wrong_origin = honest.clone();
                        wrong_origin.added_references[1].output_role = SemanticRole::OriginCap {
                            origin_feature: saved.base_feature,
                            side: CapSide::End,
                        };
                        assert!(d.write_cut_parameters(&wrong_origin).is_err());
                        let mut missing = honest.clone();
                        missing.added_references.pop();
                        assert!(d.write_cut_parameters(&missing).is_err());
                    }
                    if count > 0 {
                        let mut collision = honest.clone();
                        collision.added_references[0].id = d.topology_refs().expect("refs")[0].id;
                        assert!(
                            d.write_cut_parameters(&collision)
                                .expect_err("occupied ID")
                                .to_string()
                                .contains("already exists")
                        );
                    }
                    let mut forged = honest.clone();
                    forged.saved.previous_feature = ObjectId::new();
                    assert!(d.write_cut_parameters(&forged).is_err());
                    let mut forged = honest.clone();
                    if let ObjectPayload::Sketch(s) = &mut forged.tool_sketch.payload {
                        let other = saved.neighboring_tool.as_ref().expect("other");
                        s.curves[0].geometry = SketchGeometry::Circle {
                            center: Point2::new(other.center_mm[0], other.center_mm[1])
                                .expect("point"),
                            radius: saved.radius_mm,
                        };
                    }
                    assert!(d.write_cut_parameters(&forged).is_err());
                    assert_eq!(d.content_version().expect("unchanged"), before);
                    d.write_cut_parameters(&honest).expect("honest write");
                    let saved = saved_cut(
                        &d,
                        &d.objects().expect("objects"),
                        &d.object(saved.feature).expect("object").expect("feature"),
                    )
                    .expect("rediscovery");
                    assert_eq!(
                        saved.protected_floor_references.len(),
                        if saved.feature == first { 2 } else { 1 }
                    );
                    let error = prepare_cut_parameters(
                        &d,
                        saved.feature,
                        &CircularCutEdit {
                            extent: CutExtent::Blind { depth_mm: 10. },
                            vocabulary: ExtentVocabulary::BlindOrThroughAll,
                            ..edit
                        },
                    )
                    .expect_err("protected floor");
                    for id in &saved.protected_floor_references {
                        assert!(error.to_string().contains(&id.to_string()), "{error}");
                    }
                    assert!(
                        prepare_cut_parameters(&d, saved.feature, &edit)
                            .expect("repeat")
                            .added_references
                            .is_empty()
                    );
                }
            }
        }
    }
    #[test]
    fn bounded_history_writer_checks_every_floor_and_longer_documents_still_open() {
        let (root, mut d, body) = plate([[0., 0.], [100., 0.], [100., 100.], [0., 100.]], 10.);
        let mut ids = Vec::new();
        let mut seventeenth = None;
        for i in 0..16 {
            let cut = CircularCut {
                center_mm: [10. + (i % 4) as f64 * 20., 10. + (i / 4) as f64 * 20.],
                radius_mm: 2.,
                extent: CutExtent::Blind { depth_mm: 10. },
            };
            let p = prepare_circular_cut(&d, body, &cut).expect("link");
            ids.push(p.feature.id);
            if i == 15 {
                seventeenth = Some(p.clone());
            }
            d.write_circular_cut(&p).expect("write");
        }
        let objects = d.objects().expect("objects");
        let h = saved_history(&d, &objects).expect("history");
        assert_eq!(h.cuts.len(), 16);
        assert!(
            h.add_target(body)
                .expect_err("limit")
                .to_string()
                .contains("16")
        );
        for index in [0, 8, 15] {
            let saved = &h.cuts[index];
            let edit = CircularCutEdit {
                tool_curve: saved.tool_curve,
                center_mm: saved.center_mm,
                radius_mm: saved.radius_mm,
                extent: CutExtent::Blind { depth_mm: 3. },
                vocabulary: ExtentVocabulary::BlindOrThroughAll,
            };
            let p = prepare_cut_parameters(&d, saved.feature, &edit).expect("floor");
            assert_eq!(p.added_references.len(), 16 - index);
            for (offset, r) in p.added_references.iter().enumerate() {
                assert_eq!(r.producer_feature, ids[index + offset]);
                assert_eq!(r.owner, r.producer_feature);
                assert_eq!(
                    r.output_role,
                    if offset == 0 {
                        SemanticRole::ExtrudeCap { side: CapSide::End }
                    } else {
                        SemanticRole::OriginCap {
                            origin_feature: saved.feature,
                            side: CapSide::End,
                        }
                    }
                );
            }
            let before = d.content_version().expect("version");
            let mut forged = p.clone();
            forged.added_references.pop();
            assert!(d.write_cut_parameters(&forged).is_err());
            let mut forged = p.clone();
            forged.added_references[0].id = saved.tool_curve;
            assert!(
                d.write_cut_parameters(&forged)
                    .expect_err("embedded collision")
                    .to_string()
                    .contains("already exists")
            );
            if p.added_references.len() > 1 {
                let mut forged = p.clone();
                forged.added_references[1].id = forged.added_references[0].id;
                assert!(
                    d.write_cut_parameters(&forged)
                        .expect_err("duplicate")
                        .to_string()
                        .contains("duplicate")
                );
                let mut forged = p.clone();
                forged
                    .added_references
                    .last_mut()
                    .expect("last")
                    .output_role = SemanticRole::OriginCap {
                    origin_feature: saved.base_feature,
                    side: CapSide::End,
                };
                assert!(d.write_cut_parameters(&forged).is_err());
            }
            assert_eq!(before, d.content_version().expect("unchanged"));
        }
        let last_tool = d
            .object(h.cuts[15].tool_sketch)
            .expect("read")
            .expect("tool");
        let mut aliased = last_tool.clone();
        if let ObjectPayload::Sketch(sketch) = &mut aliased.payload {
            sketch.curves[0].id = h.cuts[0].tool_curve;
        }
        d.write(|w| {
            w.put_object(
                aliased.id,
                aliased.parent,
                aliased.ordinal,
                aliased.name.as_deref(),
                &aliased.payload,
            )
        })
        .expect("shared identity fixture");
        let catalog = crate::ExtrudeEditSource::read(&d).expect("catalog");
        assert_eq!(catalog.cut_features.len(), 17);
        for c in &catalog.cut_features {
            assert!(c.saved.is_none());
            let reason = c.refusal.as_deref().expect("refused");
            if c.feature == h.target.base_feature {
                assert!(reason.contains("use edit-extrude for NewBody"));
            } else {
                assert!(reason.contains("reuses curve UUID"));
            }
        }
        d.write(|w| {
            w.put_object(
                last_tool.id,
                last_tool.parent,
                last_tool.ordinal,
                last_tool.name.as_deref(),
                &last_tool.payload,
            )
        })
        .expect("restore fixture");

        // General document writer can represent longer histories. Editor refuses
        // the complete 17-link document; it never reports only a prefix.
        let mut p = seventeenth.expect("template");
        let sketch = ObjectId::new();
        let feature = ObjectId::new();
        p.sketch.id = sketch;
        p.feature.id = feature;
        if let ObjectPayload::Sketch(s) = &mut p.sketch.payload {
            s.curves[0].id = StableEntityId::new();
            s.curves[0].geometry = SketchGeometry::Circle {
                center: Point2::new(90., 90.).expect("center"),
                radius: 2.,
            };
        }
        if let ObjectPayload::Extrude(e) = &mut p.feature.payload {
            e.profile = sketch;
            e.previous = Some(ids[15]);
        }
        p.body.payload = ObjectPayload::Body(Body {
            tip_feature: Some(feature),
        });
        d.write(|w| {
            w.put_object(sketch, None, 100, Some("same"), &p.sketch.payload)?;
            w.put_object(feature, None, 101, Some("same"), &p.feature.payload)?;
            w.put_object(
                body,
                None,
                p.body.ordinal,
                p.body.name.as_deref(),
                &p.body.payload,
            )?;
            w.remove_dependency(Dependency {
                dependent: body,
                dependency: ids[15],
                role: DependencyRole::BodyTip,
            })?;
            for edge in [
                Dependency {
                    dependent: body,
                    dependency: feature,
                    role: DependencyRole::BodyTip,
                },
                Dependency {
                    dependent: feature,
                    dependency: ids[15],
                    role: DependencyRole::Predecessor,
                },
                Dependency {
                    dependent: feature,
                    dependency: sketch,
                    role: DependencyRole::Profile,
                },
                Dependency {
                    dependent: sketch,
                    dependency: h.target.plane,
                    role: DependencyRole::Plane,
                },
            ] {
                w.add_dependency(edge)?;
            }
            Ok(())
        })
        .expect("general history write");
        assert!(d.validate().expect("general validation").is_ok());
        let path = d.path().to_path_buf();
        d.close().expect("close");
        let d = Document::open_read_only(path).expect("long history opens");
        let catalog = crate::ExtrudeEditSource::read(&d).expect("catalog");
        assert_eq!(catalog.cut_features.len(), 18);
        for c in &catalog.cut_features {
            assert!(c.saved.is_none());
            let reason = c.refusal.as_deref().expect("refused");
            if c.feature == h.target.base_feature {
                assert!(reason.contains("use edit-extrude for NewBody"));
            } else {
                assert!(reason.contains("16"));
            }
        }
        assert!(catalog.cut_bodies[0].target.is_none());
        drop(root);
    }
    fn height_fixture(n: usize) -> (tempfile::TempDir, Document, ObjectId) {
        let (root, mut d, body) = plate([[0., 0.], [80., 0.], [80., 50.], [0., 50.]], 12.);
        for i in 0..n {
            let slot = i * 7 % 16;
            let p = prepare_circular_cut(
                &d,
                body,
                &CircularCut {
                    center_mm: [10. + (slot % 4) as f64 * 19., 7. + (slot / 4) as f64 * 12.],
                    radius_mm: 1.5 + (i % 5) as f64 * 0.25,
                    extent: CutExtent::Blind {
                        depth_mm: if i % 2 == 0 { 12. } else { 3. + (i % 7) as f64 },
                    },
                },
            )
            .expect("cut");
            d.write_circular_cut(&p).expect("write cut");
        }
        let base = saved_history(&d, &d.objects().expect("objects"))
            .expect("history")
            .target
            .base_feature;
        (root, d, base)
    }

    #[test]
    fn base_height_writer_rederives_payload_and_all_floor_names() {
        let (_root, mut d, base) = height_fixture(4);
        let original = d.content_version().expect("version");
        let p = crate::prepare_extrude_height(&d, base, 14.).expect("height");
        assert_eq!(
            p.added_references.len(),
            6,
            "all new own and descendant floors"
        );
        let old = d.topology_refs().expect("old refs");
        for mode in 0..13 {
            let mut forged = p.clone();
            match mode {
                0 => {
                    forged.added_references.pop();
                }
                1 => forged.added_references[0].owner = ObjectId::new(),
                2 => forged.added_references[1].producer_feature = base,
                3 => {
                    forged.added_references[1].output_role = SemanticRole::ExtrudeCap {
                        side: CapSide::Start,
                    }
                }
                4 => forged.added_references[0].id = old[0].id,
                5 => {
                    forged.added_references[0].id =
                        StableEntityId::from_bytes(base.to_bytes()).expect("object bytes")
                }
                6 => {
                    forged.added_references[0].id =
                        p.history.as_ref().expect("history").tools[0].tool_curve
                }
                7 => forged.added_references[1].id = forged.added_references[0].id,
                8 => forged.history = None,
                9 => {
                    if let ObjectPayload::Extrude(e) = &mut forged.feature.payload {
                        e.reversed = true;
                    }
                }
                10 => forged.feature.name = Some("forged".into()),
                11 => {
                    if let ObjectPayload::Extrude(e) = &mut forged.feature.payload {
                        e.profile = ObjectId::new();
                    }
                }
                12 => {
                    if let ObjectPayload::Extrude(e) = &mut forged.feature.payload {
                        e.end_condition = EndCondition::Blind {
                            distance: Expression::new("14+0", 14.).expect("expression"),
                        };
                    }
                }
                _ => unreachable!(),
            }
            d.write_extrude_height(&forged)
                .expect_err("forged height must be rederived");
            assert_eq!(
                d.content_version().expect("version"),
                original,
                "atomic forgery {mode}"
            );
        }
        d.write_extrude_height(&p).expect("valid height");
        let history =
            saved_history(&d, &d.objects().expect("objects")).expect("catalogue remains supported");
        assert!(history.cuts.iter().all(|c| c.floor_reference.is_some()));
        for r in &old {
            assert!(d.topology_refs().expect("refs").contains(r));
        }
        let error = crate::prepare_extrude_height(&d, base, 12.)
            .expect_err("protected floor")
            .to_string();
        assert!(error.contains(&history.cuts[0].feature.to_string()));
        for id in &history.cuts[0].protected_floor_references {
            assert!(error.contains(&id.to_string()));
        }
        let same = crate::prepare_extrude_height(&d, base, 15.).expect("pocket to pocket");
        assert!(same.added_references.is_empty());
    }

    #[test]
    fn base_height_refuses_current_history_changes_and_preserves_typed_errors() {
        use std::error::Error;
        let (_root, mut d, base) = height_fixture(16);
        let mut p = crate::prepare_extrude_height(&d, base, 14.).expect("prepare");
        let tool = p.history.as_ref().expect("history").tools[8].clone();
        let changed = prepare_cut_parameters(
            &d,
            tool.feature,
            &CircularCutEdit {
                tool_curve: tool.tool_curve,
                center_mm: tool.center_mm,
                radius_mm: tool.radius_mm,
                extent: CutExtent::Blind { depth_mm: 5. },
                vocabulary: ExtentVocabulary::BlindOrThroughAll,
            },
        )
        .expect("edit far tool");
        d.write_cut_parameters(&changed).expect("write far tool");
        let current = d.content_version().expect("version");
        d.write_extrude_height(&p)
            .expect_err("stale tools despite unchanged base");
        // Even a caller replacing the version token cannot substitute old floor facts.
        p.source_version = current;
        d.write_extrude_height(&p)
            .expect_err("current full history rederived");
        assert_eq!(d.content_version().expect("unchanged"), current);
        let sql = rusqlite::Connection::open(d.path()).expect("SQL");
        sql.execute_batch("ALTER TABLE deps RENAME TO unreadable_deps")
            .expect("damage dependencies");
        let error = crate::prepare_extrude_height(&d, base, 14.).expect_err("typed read error");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Io);
        assert!(error.source().is_some(), "SQLite cause survives");
    }

    fn through(saved: &SavedCircularCut) -> CircularCutEdit {
        CircularCutEdit {
            tool_curve: saved.tool_curve,
            center_mm: saved.center_mm,
            radius_mm: saved.radius_mm,
            extent: CutExtent::ThroughAll,
            vocabulary: ExtentVocabulary::BlindOrThroughAll,
        }
    }

    fn capability_rows(d: &Document) -> Vec<(i64, String)> {
        let conn = rusqlite::Connection::open(d.path()).expect("sqlite");
        let mut statement = conn
            .prepare("SELECT rowid,name FROM capabilities ORDER BY rowid")
            .expect("rows");
        statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .collect::<std::result::Result<_, _>>()
            .expect("rows")
    }

    #[test]
    fn through_all_is_stored_as_intent_at_payload_v3_and_never_as_a_depth() {
        let (_root, mut d, body) = plate(rectangle(), 10.);
        let prepared = prepare_circular_cut(
            &d,
            body,
            &CircularCut {
                extent: CutExtent::ThroughAll,
                ..cut(4.)
            },
        )
        .expect("a ThroughAll cut");
        let ObjectPayload::Extrude(feature) = &prepared.feature().payload else {
            panic!("a cut is an extrusion")
        };
        assert_eq!(feature.end_condition, EndCondition::ThroughAll);
        assert!(!prepared.leaves_a_floor());
        assert!(
            !prepared
                .references()
                .iter()
                .any(|r| r.output_role == SemanticRole::ExtrudeCap { side: CapSide::End }),
            "ThroughAll names no floor"
        );
        assert_eq!(prepared.feature().payload.schema_version(), 3);
        assert_eq!(
            prepared.feature().payload.required_capabilities(),
            vec![
                crate::CORE_CAPABILITY.to_owned(),
                crate::FEATURE_PREDECESSOR_CAPABILITY.to_owned(),
                crate::FEATURE_THROUGH_ALL_CAPABILITY.to_owned(),
            ]
        );
        d.write_circular_cut(&prepared).expect("write");
        assert!(d.validate().expect("validate").is_ok());
        assert!(
            capability_rows(&d)
                .iter()
                .any(|(_, n)| n == crate::FEATURE_THROUGH_ALL_CAPABILITY),
            "the document declares what a reader needs"
        );
        let saved = only(&d);
        assert_eq!(saved.extent, CutExtent::ThroughAll);
        assert_eq!(saved.extent.blind_depth_mm(), None);
        assert!(saved.floor_reference.is_none() && saved.through_allowed());
        assert_eq!(saved.request_versions(), &[2]);

        // The envelope contract is checked both ways: a v2 header over a
        // ThroughAll payload tells an older build it may rewrite it.
        let objects = d.objects().expect("objects");
        let stored = objects
            .iter()
            .find(|o| o.id == prepared.feature().id)
            .expect("stored cut");
        let lying = crate::Envelope::encode(
            "feature.extrude".to_owned(),
            2,
            vec![
                crate::CORE_CAPABILITY.to_owned(),
                crate::FEATURE_PREDECESSOR_CAPABILITY.to_owned(),
            ],
            match &stored.payload {
                ObjectPayload::Extrude(e) => e,
                _ => panic!("extrusion"),
            },
        )
        .expect("encode")
        .to_bytes()
        .expect("bytes");
        assert!(ObjectPayload::from_storage_bytes(&lying).is_err());
    }

    #[test]
    fn mode_transitions_keep_add_or_refuse_names_before_anything_is_minted() {
        // Blind through -> ThroughAll -> Blind through: no name changes.
        let (_root, mut d, feature) = cut_plate(10.);
        let saved = only(&d);
        assert!(saved.through_allowed() && saved.floor_reference.is_none());
        let rows = capability_rows(&d);
        let refs = d.topology_refs().expect("refs");
        let p = prepare_cut_parameters(&d, feature, &through(&saved)).expect("intent");
        assert!(p.added_references().is_empty(), "same tool, same names");
        assert_eq!(p.feature().payload.schema_version(), 3);
        d.write_cut_parameters(&p).expect("write");
        assert_eq!(d.topology_refs().expect("refs"), refs);
        let after = capability_rows(&d);
        assert_eq!(&after[..rows.len()], rows.as_slice(), "rowids kept");
        assert_eq!(
            after[rows.len()..]
                .iter()
                .map(|(_, n)| n.as_str())
                .collect::<Vec<_>>(),
            vec![crate::FEATURE_THROUGH_ALL_CAPABILITY],
            "exactly one new index row"
        );
        let objects = d.objects().expect("objects");
        let row = objects.iter().find(|o| o.id == feature).expect("cut");
        assert_eq!(
            crate::Envelope::from_bytes(row.storage_bytes())
                .expect("header")
                .schema_version,
            3
        );
        assert!(d.validate().expect("validate").is_ok());

        // Request v1 cannot say ThroughAll and must not erase it.
        let through_saved = only(&d);
        let v1 = CircularCutEdit {
            vocabulary: ExtentVocabulary::BlindOnly,
            ..moved(&through_saved, 10.)
        };
        let refused = prepare_cut_parameters(&d, feature, &v1).expect_err("v1 on ThroughAll");
        assert!(
            refused.to_string().contains(&feature.to_string()),
            "{refused}"
        );
        assert!(
            refused.to_string().contains("request_version 2"),
            "{refused}"
        );

        // ThroughAll -> Blind through keeps every name and returns to v2.
        let back = prepare_cut_parameters(
            &d,
            feature,
            &CircularCutEdit {
                extent: CutExtent::Blind { depth_mm: 10. },
                ..through(&through_saved)
            },
        )
        .expect("back");
        assert!(back.added_references().is_empty());
        assert_eq!(back.feature().payload.schema_version(), 2);

        // ThroughAll -> pocket adds its own floor and nothing else.
        let pocket =
            prepare_cut_parameters(&d, feature, &moved(&through_saved, 4.)).expect("pocket");
        assert_eq!(
            pocket
                .added_references()
                .iter()
                .map(|r| r.output_role.clone())
                .collect::<Vec<_>>(),
            vec![SemanticRole::ExtrudeCap { side: CapSide::End }]
        );
        d.write_cut_parameters(&pocket).expect("pocket write");
        assert!(d.validate().expect("validate").is_ok());

        // A pocket with saved floor names refuses ThroughAll at preparation,
        // with the Cut and every protected UUID.
        let with_floor = only(&d);
        assert!(!with_floor.through_allowed());
        let refused = prepare_cut_parameters(&d, feature, &through(&with_floor))
            .expect_err("protected floor");
        let message = refused.to_string();
        assert!(message.contains(&feature.to_string()), "{message}");
        assert!(message.contains("ThroughAll"), "{message}");
        for id in &with_floor.protected_floor_references {
            assert!(message.contains(&id.to_string()), "{message}");
        }
    }

    #[test]
    fn the_writer_refuses_a_forged_end_or_a_forged_vocabulary() {
        let (_root, mut d, feature) = cut_plate(10.);
        let saved = only(&d);
        let honest = prepare_cut_parameters(&d, feature, &through(&saved)).expect("intent");

        // The payload says Blind, the edit says ThroughAll.
        let mut forged = honest.clone();
        if let ObjectPayload::Extrude(e) = &mut forged.feature.payload {
            e.end_condition = EndCondition::Blind {
                distance: Expression::constant(10.).expect("literal"),
            };
        }
        d.write_cut_parameters(&forged).expect_err("end forged");

        // A ThroughAll payload with a v1 vocabulary the saved Cut rejects.
        d.write_cut_parameters(&honest).expect("honest");
        let now = only(&d);
        let mut forged = prepare_cut_parameters(
            &d,
            feature,
            &CircularCutEdit {
                center_mm: [21., 15.],
                ..through(&now)
            },
        )
        .expect("still ThroughAll");
        forged.edit.vocabulary = ExtentVocabulary::BlindOnly;
        forged.edit.extent = CutExtent::Blind { depth_mm: 10. };
        d.write_cut_parameters(&forged)
            .expect_err("vocabulary forged");
    }

    #[test]
    fn height_keeps_through_all_through_and_blind_depths_absolute() {
        let (_root, mut d, body) = plate([[0., 0.], [80., 0.], [80., 50.], [0., 50.]], 12.);
        for (center, extent) in [
            ([10.125, 7.625], CutExtent::ThroughAll),
            ([29.125, 7.625], CutExtent::Blind { depth_mm: 12. }),
            ([48.125, 7.625], CutExtent::Blind { depth_mm: 4.5 }),
        ] {
            let p = prepare_circular_cut(
                &d,
                body,
                &CircularCut {
                    center_mm: center,
                    radius_mm: 1.75,
                    extent,
                },
            )
            .expect("cut");
            d.write_circular_cut(&p).expect("write");
        }
        let history = saved_history(&d, &d.objects().expect("objects")).expect("history");
        let [through_all, blind_through, pocket] = [0, 1, 2].map(|i| history.cuts[i].feature);
        let base = history.target.base_feature;

        // Growing: only the Blind through hole gains a floor, at its own
        // producer and at the one later producer.
        let grown = crate::prepare_extrude_height(&d, base, 14.25).expect("grow");
        let owners: Vec<_> = grown
            .added_references()
            .iter()
            .map(|r| (r.owner, r.output_role.clone()))
            .collect();
        assert_eq!(
            owners,
            vec![
                (
                    blind_through,
                    SemanticRole::ExtrudeCap { side: CapSide::End }
                ),
                (
                    pocket,
                    SemanticRole::OriginCap {
                        origin_feature: blind_through,
                        side: CapSide::End
                    }
                ),
            ]
        );
        assert!(owners.iter().all(|(o, _)| *o != through_all));
        d.write_extrude_height(&grown).expect("grown");
        let after = saved_history(&d, &d.objects().expect("objects")).expect("history");
        assert_eq!(after.cuts[0].extent, CutExtent::ThroughAll);
        assert!(!after.cuts[0].leaves_a_floor(), "ThroughAll stays through");
        assert!(after.cuts[1].leaves_a_floor(), "Blind 12 is now a pocket");

        // Shrinking below the pocket depth refuses the whole publication, with
        // the far Cut named; ThroughAll never refuses a height.
        let refused = crate::prepare_extrude_height(&d, base, 4.).expect_err("too shallow");
        assert!(
            refused.to_string().contains(&pocket.to_string())
                || refused.to_string().contains(&blind_through.to_string()),
            "{refused}"
        );
        crate::prepare_extrude_height(&d, base, 13.).expect("a smaller valid height");
    }
}
