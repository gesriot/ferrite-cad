// SPDX-License-Identifier: MIT
use ferritecad_document::{CapSide, EntityKind, SelectionRule, SemanticRole, TopologyRef};
use ferritecad_kernel::SubShapeHandle;
use ferritecad_types::{CadError, Result};

use crate::TopologyMap;

/// Resolves a stored reference against what this rebuild produced.
///
/// The role, the selection rule and the expected kind are checked together, as
/// one contract. A reference that names a cap but asks for every entity
/// derived from an ancestor is not a reference with a recoverable typo in it;
/// it is a statement that does not describe anything, and it is refused.
///
/// # What the failures mean
///
/// - [`CadError::Input`] — the reference contradicts itself: a role and rule
///   that cannot go together, or an expected kind the role can never produce.
///   The stored reference is wrong, and no rebuild will make it right.
/// - [`CadError::Topology`] — the reference is well formed and the geometry it
///   names is not there: the feature produced nothing, the segment it points at
///   is gone, or an exact selection matched a number of faces other than one.
///   This is the lost reference the user must be shown.
/// - [`CadError::Unsupported`] — the role is a real one this slice does not
///   produce.
///
/// No failure returns a face. There is no nearest match, no first candidate and
/// no fallback to geometry: `fallback_signature` is deliberately ignored here.
/// A reference that resolves to the wrong face is worse than one that refuses
/// to resolve, because only the second is visible.
pub fn resolve(map: &TopologyMap, reference: &TopologyRef) -> Result<Vec<SubShapeHandle>> {
    match &reference.output_role {
        SemanticRole::ExtrudeCap { side } => {
            require_kind(reference, EntityKind::Face, "an extrusion cap")?;
            match side {
                CapSide::Start | CapSide::End => {}
                other => {
                    return Err(CadError::unsupported(format!(
                        "topology reference {} names extrusion cap side {other:?}, which this \
                         build does not understand",
                        reference.id
                    )));
                }
            }
            match reference.selection {
                SelectionRule::Exact => {}
                SelectionRule::AllDerivedFrom { .. } => {
                    return Err(CadError::input(format!(
                        "topology reference {} names an extrusion cap but selects everything \
                         derived from an ancestor; a cap descends from no input and is selected \
                         exactly",
                        reference.id
                    )));
                }
                ref other => return Err(unknown_rule(reference, other)),
            }

            let faces: Vec<SubShapeHandle> = map.legacy_candidates(reference).into_iter().collect();
            exactly_one(reference, faces, &format!("the {side:?} cap"))
        }

        SemanticRole::ExtrudeSide { profile_segment } => {
            require_kind(reference, EntityKind::Face, "an extrusion side")?;

            let faces: Vec<SubShapeHandle> = map.legacy_candidates(reference).into_iter().collect();

            match reference.selection {
                SelectionRule::Exact => exactly_one(
                    reference,
                    faces,
                    &format!("the side raised from segment {profile_segment}"),
                ),
                SelectionRule::AllDerivedFrom { ancestor } => {
                    // The rule names the ancestor a second time. Letting the
                    // two disagree would mean a reference that reads as one
                    // thing and selects another.
                    if ancestor != *profile_segment {
                        return Err(CadError::input(format!(
                            "topology reference {} names the side raised from segment \
                             {profile_segment} but selects everything derived from {ancestor}",
                            reference.id
                        )));
                    }
                    if faces.is_empty() {
                        return Err(CadError::topology(format!(
                            "topology reference {} selects every face raised from segment \
                             {ancestor}, and this rebuild raised none; the segment is gone or \
                             produced nothing",
                            reference.id
                        )));
                    }
                    Ok(faces)
                }
                ref other => Err(unknown_rule(reference, other)),
            }
        }

        SemanticRole::RevolveFace { profile_segment } => {
            require_kind(reference, EntityKind::Face, "a face of revolution")?;

            // Only a revolution's own faces. An extrusion's sides are filed
            // elsewhere, so a Revolve reference against an Extrude finds
            // nothing here and is refused, never answered with a swept face.
            let faces: Vec<SubShapeHandle> = map.legacy_candidates(reference).into_iter().collect();

            match reference.selection {
                SelectionRule::Exact => exactly_one(
                    reference,
                    faces,
                    &format!("the face turned from Line {profile_segment}"),
                ),
                SelectionRule::AllDerivedFrom { ancestor } => {
                    if ancestor != *profile_segment {
                        return Err(CadError::input(format!(
                            "topology reference {} names the face turned from Line \
                             {profile_segment} but selects everything derived from {ancestor}",
                            reference.id
                        )));
                    }
                    if faces.is_empty() {
                        return Err(CadError::topology(format!(
                            "topology reference {} selects every face turned from Line \
                             {ancestor}, and this rebuild turned none; the Line is gone or the \
                             producer is not a revolution",
                            reference.id
                        )));
                    }
                    Ok(faces)
                }
                ref other => Err(unknown_rule(reference, other)),
            }
        }

        SemanticRole::RevolveCap { side } => {
            require_kind(reference, EntityKind::Face, "a revolution's end face")?;
            match side {
                CapSide::Start | CapSide::End => {}
                other => {
                    return Err(CadError::unsupported(format!(
                        "topology reference {} names revolution end face {other:?}, which this \
                         build does not understand",
                        reference.id
                    )));
                }
            }
            match reference.selection {
                SelectionRule::Exact => {}
                SelectionRule::AllDerivedFrom { .. } => {
                    return Err(CadError::input(format!(
                        "topology reference {} names a revolution's end face but selects \
                         everything derived from an ancestor; an end face descends from no Line \
                         and is selected exactly",
                        reference.id
                    )));
                }
                ref other => return Err(unknown_rule(reference, other)),
            }

            // Only a partial revolution's own end faces: an extrusion's caps are
            // filed elsewhere, so this never answers with one, and a full turn
            // has none to answer with.
            let faces: Vec<SubShapeHandle> = map.legacy_candidates(reference).into_iter().collect();
            exactly_one(reference, faces, &format!("the revolution's {side:?} face"))
        }

        SemanticRole::ExtrudeCapEdge {
            side,
            profile_segment,
        } => {
            require_kind(reference, EntityKind::Edge, "an extrusion cap edge")?;
            match reference.selection {
                SelectionRule::Exact => {}
                SelectionRule::AllDerivedFrom { .. } => {
                    return Err(CadError::input(format!(
                        "topology reference {} names the edge where a cap meets the face \
                         raised from segment {profile_segment}, which is one edge, but selects \
                         everything derived from an ancestor",
                        reference.id
                    )));
                }
                ref other => return Err(unknown_rule(reference, other)),
            }

            let Some(_) = map.feature(reference.producer_feature) else {
                return Err(CadError::topology(format!(
                    "topology reference {} names geometry of a feature this rebuild produced \
                     nothing for",
                    reference.id
                )));
            };
            if !matches!(side, CapSide::Start | CapSide::End) {
                return Err(CadError::unsupported(format!(
                    "topology reference {} names extrusion cap side {side:?}, which this build \
                     does not understand",
                    reference.id
                )));
            };
            exactly_one(
                reference,
                map.legacy_candidates(reference).into_iter().collect(),
                &format!("the {side:?} cap edge of segment {profile_segment}"),
            )
        }

        SemanticRole::ExtrudeSweepEdge { joint } => {
            require_kind(reference, EntityKind::Edge, "an extrusion sweep edge")?;
            match reference.selection {
                SelectionRule::Exact => {}
                SelectionRule::AllDerivedFrom { .. } => {
                    return Err(CadError::input(format!(
                        "topology reference {} names the edge swept from {joint}, which is one \
                         edge, but selects everything derived from an ancestor",
                        reference.id
                    )));
                }
                ref other => return Err(unknown_rule(reference, other)),
            }

            let Some(_) = map.feature(reference.producer_feature) else {
                return Err(CadError::topology(format!(
                    "topology reference {} names geometry of a feature this rebuild produced \
                     nothing for",
                    reference.id
                )));
            };
            exactly_one(
                reference,
                map.legacy_candidates(reference).into_iter().collect(),
                &format!("the edge swept from {joint}"),
            )
        }

        SemanticRole::ExtrudeCapVertex { side, joint } => {
            require_kind(reference, EntityKind::Vertex, "an extrusion cap vertex")?;
            match reference.selection {
                SelectionRule::Exact => {}
                SelectionRule::AllDerivedFrom { .. } => {
                    return Err(CadError::input(format!(
                        "topology reference {} names the vertex where {joint} reaches a cap, \
                         which is one point, but selects everything derived from an ancestor",
                        reference.id
                    )));
                }
                ref other => return Err(unknown_rule(reference, other)),
            }

            let Some(_) = map.feature(reference.producer_feature) else {
                return Err(CadError::topology(format!(
                    "topology reference {} names geometry of a feature this rebuild produced \
                     nothing for",
                    reference.id
                )));
            };
            if !matches!(side, CapSide::Start | CapSide::End) {
                return Err(CadError::unsupported(format!(
                    "topology reference {} names extrusion cap side {side:?}, which this build \
                     does not understand",
                    reference.id
                )));
            };
            exactly_one(
                reference,
                map.legacy_candidates(reference).into_iter().collect(),
                &format!("the vertex where {joint} reaches the {side:?} cap"),
            )
        }

        SemanticRole::CarriedCap { side } => {
            require_kind(reference, EntityKind::Face, "a carried cap")?;
            match side {
                CapSide::Start | CapSide::End => {}
                other => {
                    return Err(CadError::unsupported(format!(
                        "topology reference {} names carried cap side {other:?}, which this \
                         build does not understand",
                        reference.id
                    )));
                }
            }
            match reference.selection {
                SelectionRule::Exact => {}
                SelectionRule::AllDerivedFrom { .. } => {
                    return Err(CadError::input(format!(
                        "topology reference {} names a carried cap but selects everything \
                         derived from an ancestor; a cap is selected exactly",
                        reference.id
                    )));
                }
                ref other => return Err(unknown_rule(reference, other)),
            }
            if map.legacy_removed(reference) {
                return Err(CadError::topology(format!(
                    "topology reference {} names the {side:?} cap carried into feature {}, and \
                     that feature removed it",
                    reference.id, reference.producer_feature
                )));
            }
            let faces: Vec<SubShapeHandle> = map.legacy_candidates(reference).into_iter().collect();
            exactly_one(reference, faces, &format!("the carried {side:?} cap"))
        }

        SemanticRole::CarriedSide { profile_segment } => {
            require_kind(reference, EntityKind::Face, "a carried side")?;
            if map.legacy_removed(reference) {
                return Err(CadError::topology(format!(
                    "topology reference {} names the face carried into feature {} from segment \
                     {profile_segment}, and that feature removed it",
                    reference.id, reference.producer_feature
                )));
            }
            let faces: Vec<SubShapeHandle> = map.legacy_candidates(reference).into_iter().collect();
            match reference.selection {
                SelectionRule::Exact => exactly_one(
                    reference,
                    faces,
                    &format!("the face carried from segment {profile_segment}"),
                ),
                SelectionRule::AllDerivedFrom { ancestor } => {
                    if ancestor != *profile_segment {
                        return Err(CadError::input(format!(
                            "topology reference {} names the face carried from segment \
                             {profile_segment} but selects everything derived from {ancestor}",
                            reference.id
                        )));
                    }
                    if faces.is_empty() {
                        return Err(CadError::topology(format!(
                            "topology reference {} selects every face carried from segment \
                             {ancestor}, and this rebuild carried none",
                            reference.id
                        )));
                    }
                    Ok(faces)
                }
                ref other => Err(unknown_rule(reference, other)),
            }
        }

        SemanticRole::OriginCap {
            origin_feature: _,
            side,
        } => {
            require_kind(reference, EntityKind::Face, "an origin cap")?;
            if !matches!(side, CapSide::Start | CapSide::End) {
                return Err(CadError::unsupported("unknown origin cap side"));
            }
            if reference.selection != SelectionRule::Exact {
                return Err(CadError::input("an origin cap is selected exactly"));
            }
            let faces = origin_faces(map, reference)?;
            exactly_one(reference, faces, "the cap of the named origin feature")
        }
        SemanticRole::OriginSide {
            origin_feature: _,
            profile_segment,
        } => {
            require_kind(reference, EntityKind::Face, "an origin side")?;
            let faces = origin_faces(map, reference)?;
            match reference.selection {
                SelectionRule::Exact => {
                    exactly_one(reference, faces, "the side of the named origin feature")
                }
                SelectionRule::AllDerivedFrom { ancestor } if ancestor == *profile_segment => {
                    if faces.is_empty() {
                        Err(CadError::topology("no faces carried from this origin"))
                    } else {
                        Ok(faces)
                    }
                }
                _ => Err(CadError::input(
                    "origin side selection must name its own segment",
                )),
            }
        }

        SemanticRole::OriginFilletFace {
            origin_feature: _,
            edge_feature: _,
            joint: _,
        } => {
            require_kind(reference, EntityKind::Face, "an earlier fillet's face")?;
            if reference.selection != SelectionRule::Exact {
                return Err(CadError::input(
                    "an earlier fillet's face is selected exactly",
                ));
            }
            let faces = origin_faces(map, reference)?;
            exactly_one(reference, faces, "the face the named fillet made")
        }

        SemanticRole::SketchSegment { segment } => Err(CadError::unsupported(format!(
            "topology reference {} names sketch segment {segment} directly, and the geometry \
             kernel does not yet produce a shape for a sketch on its own",
            reference.id
        ))),

        SemanticRole::EdgeFilletFace {
            edge_feature,
            joint,
        } => {
            require_kind(reference, EntityKind::Face, "a fillet face")?;
            if reference.selection != SelectionRule::Exact {
                return Err(CadError::input(format!(
                    "topology reference {} names the face rounded from {joint}, which is one \
                     face, and must select it exactly",
                    reference.id
                )));
            }
            let Some(_) = map.feature(reference.producer_feature) else {
                return Err(CadError::topology(format!(
                    "topology reference {} names geometry of a feature this rebuild produced \
                     nothing for",
                    reference.id
                )));
            };
            exactly_one(
                reference,
                map.legacy_candidates(reference).into_iter().collect(),
                &format!("the face rounded from the edge {edge_feature} swept at {joint}"),
            )
        }

        SemanticRole::EdgeChamferFace {
            edge_feature,
            joint,
        } => {
            require_kind(reference, EntityKind::Face, "a chamfer face")?;
            if reference.selection != SelectionRule::Exact {
                return Err(CadError::input(format!(
                    "topology reference {} names the face cut from {joint}, which is one \
                     face, and must select it exactly",
                    reference.id
                )));
            }
            let Some(_) = map.feature(reference.producer_feature) else {
                return Err(CadError::topology(format!(
                    "topology reference {} names geometry of a feature this rebuild produced \
                     nothing for",
                    reference.id
                )));
            };
            exactly_one(
                reference,
                map.legacy_candidates(reference).into_iter().collect(),
                &format!("the face cut from the edge {edge_feature} swept at {joint}"),
            )
        }

        SemanticRole::FilletFace { source_edge } => Err(CadError::unsupported(format!(
            "topology reference {} names a fillet face from edge {source_edge}, and fillets are \
             not implemented",
            reference.id
        ))),

        other => Err(CadError::unsupported(format!(
            "topology reference {} has role {other:?}, which this build cannot resolve",
            reference.id
        ))),
    }
}

fn require_kind(reference: &TopologyRef, expected: EntityKind, what: &str) -> Result<()> {
    if reference.expected_kind != expected {
        return Err(CadError::input(format!(
            "topology reference {} names {what}, which is always a {}, but expects a {}",
            reference.id,
            expected.as_str(),
            reference.expected_kind.as_str()
        )));
    }
    Ok(())
}

fn exactly_one(
    reference: &TopologyRef,
    faces: Vec<SubShapeHandle>,
    what: &str,
) -> Result<Vec<SubShapeHandle>> {
    match faces.len() {
        1 => Ok(faces),
        0 => Err(CadError::topology(format!(
            "topology reference {} names {what} of feature {}, and this rebuild produced none",
            reference.id, reference.producer_feature
        ))),
        found => Err(CadError::topology(format!(
            "topology reference {} names {what} of feature {} exactly, and this rebuild produced \
             {found}; refusing rather than choosing one",
            reference.id, reference.producer_feature
        ))),
    }
}

fn unknown_rule(reference: &TopologyRef, rule: &SelectionRule) -> CadError {
    CadError::unsupported(format!(
        "topology reference {} uses selection rule {rule:?}, which this build cannot apply",
        reference.id
    ))
}

fn origin_faces(map: &TopologyMap, reference: &TopologyRef) -> Result<Vec<SubShapeHandle>> {
    if map.legacy_removed(reference) {
        return Err(CadError::topology(format!(
            "topology reference {} names an origin face removed by feature {}",
            reference.id, reference.producer_feature
        )));
    }
    Ok(map.legacy_candidates(reference).into_iter().collect())
}
