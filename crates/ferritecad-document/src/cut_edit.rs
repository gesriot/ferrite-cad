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
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId, Tolerance, Transform};
use std::collections::BTreeSet;

use crate::{
    Body, CapSide, CircleExtrusion, DatumPlane, Dependency, DependencyRole, Document, EndCondition,
    EntityKind, Expression, Extrude, ObjectPayload, ObjectRecord, Point2, PolygonExtrusion,
    SelectionRule, SemanticRole, Sketch, SketchCurve, SketchGeometry, SolidOperation, TopologyRef,
    sketch_edit::frame,
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

/// The saved body a circular cut can be added to, exactly as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedCutTarget {
    /// The body whose tip the new feature becomes. Its identity is kept.
    pub body: ObjectId,
    /// The datum the tool is drawn on: the same XY plane the part was drawn on.
    pub plane: ObjectId,
    /// The feature the cut modifies, which is the body's tip today.
    pub tip_feature: ObjectId,
    /// The sketch that feature reads, reported so a form can name the part.
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
    pub depth_mm: f64,
}

pub fn cut_choices(document: &Document, objects: &[ObjectRecord]) -> Vec<CutChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .map(|o| match supported(document, objects, o) {
            Ok(target) => CutChoice {
                body: o.id,
                name: o.name.clone(),
                target: Some(target),
                refusal: None,
            },
            Err(error) => CutChoice {
                body: o.id,
                name: o.name.clone(),
                target: None,
                refusal: Some(error.to_string()),
            },
        })
        .collect()
}

pub(crate) fn supported(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
) -> Result<SavedCutTarget> {
    let ObjectPayload::Body(body) = &object.payload else {
        return Err(unsupported("selected object is not a Body"));
    };
    if object.payload.to_storage_bytes()?.as_slice() != object.storage_bytes() {
        return Err(unsupported(
            "selected Body storage cannot be rewritten losslessly by this reader",
        ));
    }
    let tip_feature = body
        .tip_feature
        .ok_or_else(|| unsupported("a body with nothing built into it has nothing to cut"))?;
    let extrude = objects
        .iter()
        .find(|o| o.id == tip_feature)
        .ok_or_else(|| unsupported("the body's tip feature is missing"))?;
    let ObjectPayload::Extrude(feature) = &extrude.payload else {
        return Err(unsupported("the body's tip is not an extrusion"));
    };
    let sketch_object = objects
        .iter()
        .find(|o| o.id == feature.profile)
        .ok_or_else(|| unsupported("the tip feature's profile is missing"))?;

    // The frame is the one every copy edit of a saved profile requires, asked
    // through the function that owns it so this route cannot accept a document
    // the others refuse.
    let (sketch, height_mm) = frame(document, objects, sketch_object)?;
    if !sketch.constraints.is_empty() {
        return Err(unsupported(
            "this slice cuts an unconstrained part; a constrained profile has a solver between \
             its numbers and its geometry, and that is a later slice",
        ));
    }
    let extents = rectangle(sketch, height_mm)?;

    Ok(SavedCutTarget {
        body: object.id,
        plane: sketch.plane,
        tip_feature,
        profile: sketch_object.id,
        profile_segments: sketch.curves.iter().map(|c| c.id).collect(),
        height_mm,
        extents_mm: extents,
    })
}

/// The axis-aligned rectangle a profile is, or why it is not one.
///
/// Read from the stored lines and from nothing else. Four segments, each
/// axis-parallel, meeting end to end and closing — which is what makes the
/// clearance check below a statement about the part rather than about its
/// bounding box.
fn rectangle(sketch: &Sketch, height_mm: f64) -> Result<[[f64; 2]; 2]> {
    if sketch.curves.len() != 4 {
        return Err(unsupported(
            "this slice cuts a rectangular plate, and this profile is not four Lines",
        ));
    }
    let mut corners = Vec::with_capacity(4);
    for curve in &sketch.curves {
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
    for (index, curve) in sketch.curves.iter().enumerate() {
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

impl CutChoice {
    /// The same check the copy applies, with no SQLite or kernel work.
    pub fn validate_cut(&self, cut: &CircularCut) -> Result<()> {
        let target = self
            .target
            .as_ref()
            .ok_or_else(|| unsupported("unsupported cut target"))?;
        validate(target.height_mm, target.extents_mm, cut).map(|_| ())
    }
}

/// The one numeric rule, applied to a request against the saved part.
fn validate(
    height_mm: f64,
    extents_mm: [[f64; 2]; 2],
    cut: &CircularCut,
) -> Result<CircleExtrusion> {
    // The tool is a cylinder, judged by the policy every cylinder in this
    // build is judged by: finite centre, positive radius, positive height and
    // the published bound on all of them.
    let tool = CircleExtrusion::new(cut.center_mm, cut.radius_mm, cut.depth_mm)?;
    if cut.depth_mm > height_mm {
        return Err(CadError::input(format!(
            "a cut of {} mm into a part {} mm tall would run past it; this slice cuts to a depth \
             the part has",
            cut.depth_mm, height_mm
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
        self.cut.depth_mm < self.height_mm
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
    validate(target.height_mm, target.extents_mm, cut)?;

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
            end_condition: EndCondition::Blind {
                distance: Expression::constant(cut.depth_mm)?,
            },
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

    let references = cut_references(
        feature_id,
        tool_curve,
        &target.profile_segments,
        cut.depth_mm < target.height_mm,
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
    let EndCondition::Blind { distance } = &extrude.end_condition else {
        return Err(CadError::input("a prepared cut runs to a blind depth"));
    };
    let stated = CircularCut {
        center_mm: [center.x, center.y],
        radius_mm: radius,
        depth_mm: distance.value(),
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
    /// The body the cut is the tip of.
    pub body: ObjectId,
    /// The datum both sketches are drawn on.
    pub plane: ObjectId,
    /// The feature the cut modifies; unchanged by this edit.
    pub base_feature: ObjectId,
    /// The part's own profile; unchanged by this edit.
    pub profile_sketch: ObjectId,
    /// The sketch holding the tool circle, whose payload this edit rewrites.
    pub tool_sketch: ObjectId,
    /// The circle inside it. It keeps this identity across the edit.
    pub tool_curve: StableEntityId,
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub depth_mm: f64,
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

impl SavedCircularCut {
    /// Whether a cut of this depth leaves a floor. One rule, asked of the
    /// saved height rather than recomputed by each caller from two numbers.
    fn floor_at(&self, depth_mm: f64) -> bool {
        depth_mm < self.height_mm
    }
    /// Whether the depth may be raised to the part's height, cutting through.
    pub fn through_allowed(&self) -> bool {
        self.floor_reference.is_none()
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
    pub depth_mm: f64,
}

/// What an accepted edit does to the saved names, stated before anything is
/// written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transition {
    /// The cut keeps the floor it had, or keeps having none.
    Kept,
    /// A cut that ran through the part now stops inside it, so it gains a
    /// floor and one new name for it. Nothing saved is lost.
    FloorAppears,
}

pub fn cut_parameter_choices(
    document: &Document,
    objects: &[ObjectRecord],
) -> Vec<CutParameterChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
        .map(|o| match saved_cut(document, objects, o) {
            Ok(saved) => CutParameterChoice {
                feature: o.id,
                name: o.name.clone(),
                saved: Some(saved),
                refusal: None,
            },
            Err(error) => CutParameterChoice {
                feature: o.id,
                name: o.name.clone(),
                saved: None,
                refusal: Some(error.to_string()),
            },
        })
        .collect()
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

/// The saved cut one feature is, or why this slice cannot edit its numbers.
///
/// The class is exactly what [`prepare_circular_cut`] produces from the class
/// it accepts, and nothing wider: six objects, six dependency edges with those
/// exact roles, and a set of names on the cut that matches the one this module
/// would mint for the numbers the document stores. That last check is what
/// makes "this really is a cut this build made" a fact rather than a hope, and
/// it is what finds the floor reference the transition policy below turns on.
pub(crate) fn saved_cut(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
) -> Result<SavedCircularCut> {
    let ObjectPayload::Extrude(cut) = &object.payload else {
        return Err(unsupported("selected object is not a feature"));
    };
    if cut.operation != SolidOperation::Cut {
        return Err(unsupported(
            "this edit changes a Cut's tool and depth; the distance of the extrusion that starts \
             a body is edited by edit-extrude",
        ));
    }
    if cut.target_body.is_some() || cut.reversed {
        return Err(unsupported(
            "this slice edits a forward Cut that names the feature it modifies",
        ));
    }
    let base_id = cut
        .previous
        .ok_or_else(|| unsupported("a Cut with no predecessor modifies nothing"))?;
    require_rewritable(object, "Cut")?;
    let depth_mm = blind_literal(cut, "cut")?;

    // The shape of the whole document, before any single object is trusted:
    // a plate with one cut in it is six objects, and a document with more in
    // it is not this class however its rows happen to be ordered.
    if objects.len() != 6 || objects.iter().any(|o| o.parent.is_some()) {
        return Err(unsupported(
            "editing a saved cut requires exactly one XY plane, the part's Sketch and Extrude, \
             its Body, the tool Sketch and the Cut",
        ));
    }
    let base = objects
        .iter()
        .find(|o| o.id == base_id)
        .ok_or_else(|| unsupported("the feature this Cut modifies is missing"))?;
    if !matches!(&base.payload, ObjectPayload::Extrude(e)
        if e.operation == SolidOperation::NewBody && e.previous.is_none()
            && e.target_body.is_none() && !e.reversed)
    {
        return Err(unsupported(
            "this slice edits a Cut over the forward extrusion that started the body",
        ));
    }
    let bodies: Vec<&ObjectRecord> = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .collect();
    let [body] = bodies.as_slice() else {
        return Err(unsupported("editing a saved cut requires exactly one Body"));
    };
    if !matches!(&body.payload, ObjectPayload::Body(b) if b.tip_feature == Some(object.id)) {
        return Err(unsupported(
            "this slice edits the Cut a body tips at, not one buried in its history",
        ));
    }

    // The part, read through the same rules the cut that made it was accepted
    // under: a literal Blind NewBody with no parameter behind it, drawn as an
    // unconstrained axis-aligned rectangle on the untransformed XY datum.
    let ObjectPayload::Extrude(base_feature) = &base.payload else {
        unreachable!("checked above")
    };
    let height_mm = crate::editable_extrude(document, base)?;
    let profile = objects
        .iter()
        .find(|o| o.id == base_feature.profile)
        .ok_or_else(|| unsupported("the part's profile is missing"))?;
    let ObjectPayload::Sketch(part_sketch) = &profile.payload else {
        return Err(unsupported("the part's profile is not a Sketch"));
    };
    if !part_sketch.constraints.is_empty() {
        return Err(unsupported(
            "this slice edits a cut in an unconstrained part; a constrained profile has a solver \
             between its numbers and its geometry, and that is a later slice",
        ));
    }
    let plane = objects
        .iter()
        .find(|o| o.id == part_sketch.plane)
        .ok_or_else(|| unsupported("the part's datum plane is missing"))?;
    if !matches!(&plane.payload, ObjectPayload::DatumPlane(p) if p.placement == Transform::IDENTITY)
    {
        return Err(unsupported(
            "editing a saved cut requires the untransformed XY plane",
        ));
    }
    let extents_mm = rectangle(part_sketch, height_mm)?;

    // The tool: one unconstrained circle on that same datum, and no second
    // curve to have to choose between.
    let tool = objects
        .iter()
        .find(|o| o.id == cut.profile)
        .ok_or_else(|| unsupported("the Cut's tool Sketch is missing"))?;
    let ObjectPayload::Sketch(tool_sketch) = &tool.payload else {
        return Err(unsupported("the Cut's profile is not a Sketch"));
    };
    require_rewritable(tool, "tool Sketch")?;
    if tool_sketch.plane != plane.id {
        return Err(unsupported(
            "this slice edits a tool drawn on the part's own base plane",
        ));
    }
    if !tool_sketch.constraints.is_empty() {
        return Err(unsupported("this slice edits an unconstrained tool"));
    }
    let [tool_curve] = tool_sketch.curves.as_slice() else {
        return Err(unsupported("the Cut's tool Sketch draws one circle"));
    };
    if tool_curve.construction {
        return Err(unsupported("construction geometry cuts nothing"));
    }
    let SketchGeometry::Circle { center, radius } = tool_curve.geometry else {
        return Err(unsupported("this slice edits a circular tool"));
    };
    // Discovery states the supported source class as well as the next edit.
    // A valid replacement does not make an out-of-policy stored tool valid.
    validate(
        height_mm,
        extents_mm,
        &CircularCut {
            center_mm: [center.x, center.y],
            radius_mm: radius,
            depth_mm,
        },
    )?;

    // Six edges, with exactly these roles. A document carrying one more — a
    // parameter behind a distance, an ordering nobody asked for — is not this
    // class, and counting them is how that is noticed.
    let expected = BTreeSet::from([
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
            dependent: tool.id,
            dependency: plane.id,
            role: DependencyRole::Plane,
        },
        Dependency {
            dependent: object.id,
            dependency: tool.id,
            role: DependencyRole::Profile,
        },
        Dependency {
            dependent: object.id,
            dependency: base.id,
            role: DependencyRole::Predecessor,
        },
        Dependency {
            dependent: body.id,
            dependency: object.id,
            role: DependencyRole::BodyTip,
        },
    ]);
    if document
        .dependencies()?
        .into_iter()
        .collect::<BTreeSet<_>>()
        != expected
    {
        return Err(unsupported(
            "editing a saved cut requires exactly the plane, profile, predecessor and body-tip \
             edges a cut leaves behind",
        ));
    }

    // Every name this cut gave, against the ones this module mints for the
    // numbers the document stores. Matched by meaning, because the identities
    // are the document's and the order rows come back in is the database's.
    let stored: Vec<TopologyRef> = document
        .topology_refs()?
        .into_iter()
        .filter(|r| r.owner == object.id)
        .collect();
    let segments: Vec<StableEntityId> = part_sketch.curves.iter().map(|c| c.id).collect();
    let wanted = cut_references(object.id, tool_curve.id, &segments, depth_mm < height_mm);
    let mut remaining: Vec<&TopologyRef> = stored.iter().collect();
    for want in &wanted {
        let position = remaining
            .iter()
            .position(|r| same_meaning(r, want))
            .ok_or_else(|| {
                unsupported(
                    "the saved Cut does not name the faces this build gives a cut of its numbers",
                )
            })?;
        remaining.remove(position);
    }
    if !remaining.is_empty() {
        return Err(unsupported(
            "the saved Cut names more faces than a cut of its numbers gives",
        ));
    }
    let floor_reference = stored
        .iter()
        .find(|r| {
            matches!(
                r.output_role,
                SemanticRole::ExtrudeCap { side: CapSide::End }
            )
        })
        .map(|r| r.id);

    Ok(SavedCircularCut {
        feature: object.id,
        body: body.id,
        plane: plane.id,
        base_feature: base.id,
        profile_sketch: profile.id,
        tool_sketch: tool.id,
        tool_curve: tool_curve.id,
        center_mm: [center.x, center.y],
        radius_mm: radius,
        depth_mm,
        height_mm,
        extents_mm,
        floor_reference,
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
///   Nothing saved is lost, one name is added, and the shared copy job already
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
    validate(
        saved.height_mm,
        saved.extents_mm,
        &CircularCut {
            center_mm: edit.center_mm,
            radius_mm: edit.radius_mm,
            depth_mm: edit.depth_mm,
        },
    )?;
    match (saved.floor_reference, saved.floor_at(edit.depth_mm)) {
        (Some(_), true) | (None, false) => Ok(Transition::Kept),
        (None, true) => Ok(Transition::FloorAppears),
        (Some(floor), false) => Err(unsupported(format!(
            "a depth of {} mm would cut this pocket through a part {} mm tall, and the saved \
             reference {floor} names the pocket floor that would stop existing; this slice does \
             not delete saved references or move them to another face",
            edit.depth_mm, saved.height_mm
        ))),
    }
}

/// Everything one accepted parameter edit rewrites, prepared before anything
/// is written.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedCutParameters {
    pub(crate) tool_sketch: ObjectRecord,
    pub(crate) feature: ObjectRecord,
    /// The floor's name, when the edit turns a hole back into a pocket. Empty
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
        self.saved.base_feature
    }
    pub fn tool_curve(&self) -> StableEntityId {
        self.saved.tool_curve
    }
    /// Whether the edited cut leaves a floor.
    pub fn leaves_a_floor(&self) -> bool {
        self.saved.floor_at(self.edit.depth_mm)
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
    // Exactly the depth. The profile, the operation, the predecessor and the
    // absence of a target body are the history, and this edit is not about it.
    extrude.end_condition = EndCondition::Blind {
        distance: Expression::constant(edit.depth_mm)?,
    };

    let added_references = match moved {
        Transition::Kept => Vec::new(),
        Transition::FloorAppears => vec![TopologyRef {
            id: StableEntityId::new(),
            owner: saved.feature,
            producer_feature: saved.feature,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::ExtrudeCap { side: CapSide::End },
            selection: SelectionRule::Exact,
            fallback_signature: None,
        }],
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
    let EndCondition::Blind { distance } = &extrude.end_condition else {
        return Err(CadError::input("a prepared cut edit runs to a blind depth"));
    };
    let stated = CircularCutEdit {
        tool_curve: curve.id,
        center_mm: [center.x, center.y],
        radius_mm: radius,
        depth_mm: distance.value(),
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
            depth_mm: depth,
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
        // refusal that protects the data.
        assert_eq!(ObjectKind::Extrude.readable_schema_versions(), &[2, 1]);
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
                    depth_mm: 4.,
                })
                .expect_err("a tool reaching the wall");
            assert_eq!(error.kind(), ErrorKind::Input, "{center:?}");
        }
        // Clear of it by more than the tolerance is accepted.
        target
            .validate_cut(&CircularCut {
                center_mm: [5.0 + 1e-3, 15.0],
                radius_mm: 5.,
                depth_mm: 4.,
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
            depth_mm: depth,
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
        assert_eq!(saved.depth_mm, 4.);
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
        assert_eq!(after.depth_mm, 6.);
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
                        depth_mm: 4.,
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
}
