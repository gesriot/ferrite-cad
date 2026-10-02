// SPDX-License-Identifier: MIT
//! Explicit CLI projections, never serialization of evolving document structs.
use ferritecad_document::{
    ConstraintSketchChoice, SketchConstraint, SketchConstraintRule, SketchGeometry, SketchPointRef,
};
use ferritecad_types::{ObjectId, StableEntityId};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct Point {
    curve_id: StableEntityId,
    at: &'static str,
}
impl From<SketchPointRef> for Point {
    fn from(p: SketchPointRef) -> Self {
        Self {
            curve_id: p.curve,
            at: p.at.as_str(),
        }
    }
}
#[derive(Serialize)]
struct Segment {
    from: Point,
    to: Point,
}
impl From<ferritecad_document::SketchSegmentRef> for Segment {
    fn from(s: ferritecad_document::SketchSegmentRef) -> Self {
        Self {
            from: s.from.into(),
            to: s.to.into(),
        }
    }
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Rule {
    Coincident {
        a: Point,
        b: Point,
    },
    Horizontal {
        a: Point,
        b: Point,
    },
    Vertical {
        a: Point,
        b: Point,
    },
    Fixed {
        point: Point,
        x: f64,
        y: f64,
    },
    Distance {
        a: Point,
        b: Point,
        distance: f64,
    },
    EqualLength {
        a: Segment,
        b: Segment,
    },
    Perpendicular {
        a: Segment,
        b: Segment,
    },
    Parallel {
        a: Segment,
        b: Segment,
    },
    /// A circle's own radius. Names the curve, not a point of it.
    Radius {
        curve_id: StableEntityId,
        radius: f64,
    },
    Unknown,
}
impl From<SketchConstraintRule> for Rule {
    fn from(r: SketchConstraintRule) -> Self {
        match r {
            SketchConstraintRule::Coincident { a, b } => Self::Coincident {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Horizontal { a, b } => Self::Horizontal {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Vertical { a, b } => Self::Vertical {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Fixed { point, x, y } => Self::Fixed {
                point: point.into(),
                x,
                y,
            },
            SketchConstraintRule::Distance { a, b, distance } => Self::Distance {
                a: a.into(),
                b: b.into(),
                distance,
            },
            SketchConstraintRule::EqualLength { a, b } => Self::EqualLength {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Perpendicular { a, b } => Self::Perpendicular {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Parallel { a, b } => Self::Parallel {
                a: a.into(),
                b: b.into(),
            },
            SketchConstraintRule::Radius { curve, radius } => Self::Radius {
                curve_id: curve,
                radius,
            },
            _ => Self::Unknown,
        }
    }
}
#[derive(Serialize)]
pub(crate) struct Constraint {
    constraint_id: StableEntityId,
    rule: Rule,
}
impl From<&SketchConstraint> for Constraint {
    fn from(c: &SketchConstraint) -> Self {
        Self {
            constraint_id: c.id,
            rule: c.rule.into(),
        }
    }
}
#[derive(Serialize)]
struct Curve {
    curve_id: StableEntityId,
    start_mm: [f64; 2],
    end_mm: [f64; 2],
}
/// One addressable analytic circle of a managed profile, as stored.
///
/// Its own list rather than an entry in `curves`: a circle has no start and no
/// end, and describing it with two of them — or leaving it out of the only list
/// there is — would be a projection of a shape this build does not have.
#[derive(Serialize)]
struct CircleCurve {
    curve_id: StableEntityId,
    center_mm: [f64; 2],
    radius_mm: f64,
    /// `"boundary"` or `"bore"` on a profile of two circles, and `null` on one
    /// of a single circle, which has no second circle to hold a role against.
    ///
    /// Reported rather than left to be worked out from the two radii, because
    /// the roles are what every request, refusal and stored constraint is about
    /// and a reader that derived them would be a second opinion on them.
    role: Option<&'static str>,
}
#[derive(Serialize)]
pub(crate) struct Discovery {
    available: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    /// Stored Lines; [] for a managed circle, null for an unsupported profile.
    curves: Option<Vec<Curve>>,
    /// Stored circles; [] for a managed Line profile, null when unsupported.
    circles: Option<Vec<CircleCurve>>,
    constraints: Option<Vec<Constraint>>,
    /// §27G, additive: the feature that turns this profile into a solid, in
    /// the kinds `profile_feature` of the Sketch row already uses, and so the
    /// policy its solved drawing must satisfy. Null exactly when `curves` is.
    /// No height is reported for a Revolve, which has none.
    profile_feature: Option<super::ProfileFeature>,
    /// §28E, additive: the saved Fillet over the plate when this is its base
    /// Sketch; `null` on every other row. `stored_corner_mm` is the corner in
    /// the stored coordinates — the solver's starting guess — and not the
    /// part's: whether the radius fits the solved plate is checked when a copy
    /// is rebuilt.
    fillet_base: Option<FilletContext>,
    /// §28L, additive: every saved Fillet over the plate in history order,
    /// one to four, when this is its base Sketch; `null` on every other row.
    /// `fillet_base` is `null` for three or four.
    fillet_history: Option<super::FilletHistoryDiscovery>,
    /// §29D, additive: the saved Chamfer over the plate when this is its base
    /// Sketch; the same object `features[].chamfer_base` carries. Its
    /// `corner_mm` is in the stored coordinates — the solver's starting guess —
    /// and whether the distance fits the solved plate is checked when a copy is
    /// rebuilt. `null` on every other row.
    chamfer_base: Option<super::chamfer::ChamferBaseDiscovery>,
}
/// The Fillet a constraint edit of its base Sketch keeps (§28E).
#[derive(Serialize)]
struct FilletContext {
    fillet_feature_id: ObjectId,
    body_id: ObjectId,
    edge: super::FilletEdgeDto,
    radius_mm: f64,
    stored_corner_mm: [f64; 2],
    /// §28K, additive: Fillet 2 of a §28G history, which rounds Fillet 1's
    /// result (not the base Extrude); `null` with one Fillet. Its corner is
    /// in the stored coordinates too.
    second_fillet: Option<SecondFilletContext>,
}
/// §28K: Fillet 2 of Extrude -> Fillet 1 -> Fillet 2, as a constraint edit keeps it.
impl FilletContext {
    /// The older projection of a history of one or two Fillets (§28E, §28K).
    fn of(
        f: &ferritecad_document::SavedFillet,
        second: Option<&ferritecad_document::SavedFillet>,
    ) -> Self {
        Self {
            fillet_feature_id: f.feature,
            body_id: f.body,
            edge: super::FilletEdgeDto {
                feature_id: f.edge.feature,
                joint: f.edge.joint.segments(),
            },
            radius_mm: f.radius_mm,
            stored_corner_mm: f.corner.corner_mm,
            second_fillet: second.map(|s| SecondFilletContext {
                fillet_feature_id: s.feature,
                previous_feature_id: s.previous,
                history_index: s.history_index,
                edge: super::FilletEdgeDto {
                    feature_id: s.edge.feature,
                    joint: s.edge.joint.segments(),
                },
                radius_mm: s.radius_mm,
                stored_corner_mm: s.corner.corner_mm,
            }),
        }
    }
}
#[derive(Serialize)]
struct SecondFilletContext {
    fillet_feature_id: ObjectId,
    /// Fillet 1: the feature whose result this Fillet rounds.
    previous_feature_id: ObjectId,
    history_index: usize,
    edge: super::FilletEdgeDto,
    radius_mm: f64,
    stored_corner_mm: [f64; 2],
}
impl Discovery {
    pub(crate) fn new(choice: ConstraintSketchChoice, document_refusal: Option<String>) -> Self {
        // Split by what each curve actually is. A supported profile is all
        // Lines, or one or two Circles, so exactly one of the two lists is
        // non-empty; reading the geometry rather than asserting which family it
        // is keeps this honest if either class ever widens.
        let curves = choice.stored.as_ref().map(|s| {
            s.curves
                .iter()
                .filter_map(|c| match c.geometry {
                    SketchGeometry::Line { start, end } => Some(Curve {
                        curve_id: c.id,
                        start_mm: [start.x, start.y],
                        end_mm: [end.x, end.y],
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        let circles = choice.stored.as_ref().map(|s| {
            // The roles, asked once of the document rather than decided here.
            let roles = ferritecad_document::constraint_circle_roles(s);
            s.curves
                .iter()
                .filter_map(|c| match c.geometry {
                    SketchGeometry::Circle { center, radius } => Some(CircleCurve {
                        curve_id: c.id,
                        center_mm: [center.x, center.y],
                        radius_mm: radius,
                        role: roles.and_then(|(outer, inner)| match c.id {
                            id if id == outer => Some("boundary"),
                            id if id == inner => Some("bore"),
                            _ => None,
                        }),
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        Self {
            available: choice.refusal.is_none() && document_refusal.is_none(),
            refusal: choice.refusal,
            document_refusal,
            curves,
            circles,
            constraints: choice
                .stored
                .as_ref()
                .map(|s| s.constraints.iter().map(Constraint::from).collect()),
            profile_feature: choice.profile_use.map(super::ProfileFeature::of),
            fillet_base: match choice.fillets.as_slice() {
                [one] => Some(FilletContext::of(one, None)),
                [one, two] => Some(FilletContext::of(one, Some(two))),
                _ => None,
            },
            fillet_history: super::FilletHistoryDiscovery::of(&choice.fillets),
            chamfer_base: super::chamfer::ChamferBaseDiscovery::of(choice.chamfer.as_ref()),
        }
    }
}
#[derive(Serialize)]
pub(super) struct Conflict {
    sketch_id: ObjectId,
    constraints: Vec<Constraint>,
}
impl From<&ferritecad_eval::SketchConflict> for Conflict {
    fn from(c: &ferritecad_eval::SketchConflict) -> Self {
        Self {
            sketch_id: c.sketch(),
            constraints: c
                .constraints()
                .iter()
                .map(|c| Constraint {
                    constraint_id: c.id(),
                    rule: (*c.rule()).into(),
                })
                .collect(),
        }
    }
}
#[derive(Serialize)]
struct Solve {
    degrees_of_freedom: usize,
    redundant_constraint_ids: Vec<StableEntityId>,
}
#[derive(Serialize)]
pub(crate) struct Published {
    destination: std::path::PathBuf,
    document_id: ferritecad_types::DocumentId,
    sketch_id: ObjectId,
    added_constraints: Vec<Constraint>,
    removed_constraint_ids: Vec<StableEntityId>,
    /// What the solve found out, or null when the edit left no constraint to
    /// solve. A Line profile always keeps its closure and so always reports;
    /// a circle with its last dimension removed has nothing to report about.
    solve: Option<Solve>,
}
impl From<ferritecad_jobs::EditedSketchConstraints> for Published {
    fn from(p: ferritecad_jobs::EditedSketchConstraints) -> Self {
        Self {
            destination: p.destination,
            document_id: p.document_id,
            sketch_id: p.sketch,
            added_constraints: p.added.iter().map(Constraint::from).collect(),
            removed_constraint_ids: p.removed,
            solve: p.solve.as_ref().map(|s| Solve {
                degrees_of_freedom: s.degrees_of_freedom(),
                redundant_constraint_ids: s.redundant().to_vec(),
            }),
        }
    }
}
