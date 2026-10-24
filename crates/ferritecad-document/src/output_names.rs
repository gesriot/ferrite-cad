// SPDX-License-Identifier: MIT
use crate::{
    Body, CapSide, Document, EndCondition, EntityKind, ObjectPayload, ObjectRecord, SelectionRule,
    SemanticRole, SketchGeometry, SolidOperation, TopologyRef,
};
use ferritecad_types::{ObjectId, Result, StableEntityId};
use std::collections::BTreeSet;
struct CutOrigin {
    feature: ObjectId,
    tool_curve: StableEntityId,
    leaves_a_floor: bool,
}
/// The output names the written candidate's feature histories give and the saved
/// catalogue lacks, so the catalogue stays consistent with the numbers until
/// generative naming (ADR 0006 D3) derives names during rebuild and removes
/// this step. Bodies outside the Cut, Fillet and Chamfer naming structures need none here.
pub(crate) fn candidate_output_references(document: &Document) -> Result<Vec<TopologyRef>> {
    let objects = document.objects()?;
    let stored = document.topology_refs()?;
    let mut missing = Vec::new();
    for object in &objects {
        let ObjectPayload::Body(Body {
            tip_feature: Some(tip),
        }) = &object.payload
        else {
            continue;
        };
        if let Some(wanted) = candidate_history_references(&objects, *tip)
            .or_else(|| candidate_edge_references(&objects, *tip))
        {
            missing.extend(
                wanted
                    .into_iter()
                    .filter(|want| !stored.iter().chain(&missing).any(|r| same_meaning(r, want)))
                    .collect::<Vec<_>>(),
            );
        }
    }
    Ok(missing)
}

fn candidate_history_references(
    objects: &[ObjectRecord],
    tip: ObjectId,
) -> Option<Vec<TopologyRef>> {
    let get = |id| objects.iter().find(|o| o.id == id).map(|o| &o.payload);
    let mut cursor = tip;
    let mut seen = BTreeSet::new();
    let mut chain = Vec::new();
    let (base_id, base) = loop {
        if !seen.insert(cursor) {
            return None;
        }
        let ObjectPayload::Extrude(extrude) = get(cursor)? else {
            return None;
        };
        if extrude.reversed {
            return None;
        }
        match (extrude.operation, extrude.previous) {
            (SolidOperation::NewBody, None) => break (cursor, extrude),
            (SolidOperation::Cut, Some(previous)) => {
                chain.push((cursor, extrude));
                cursor = previous;
            }
            _ => return None,
        }
    };
    let EndCondition::Blind { distance } = &base.end_condition else {
        return None;
    };
    let ObjectPayload::Sketch(part) = get(base.profile)? else {
        return None;
    };
    let segments: Vec<_> = part
        .curves
        .iter()
        .filter(|c| !c.construction)
        .map(|c| matches!(c.geometry, SketchGeometry::Line { .. }).then_some(c.id))
        .collect::<Option<_>>()?;
    if segments.len() < 3 {
        return None;
    }
    let mut ancestors = Vec::new();
    let mut wanted = Vec::new();
    for (feature, cut) in chain.into_iter().rev() {
        let ObjectPayload::Sketch(sketch) = get(cut.profile)? else {
            return None;
        };
        if sketch.plane != part.plane {
            return None;
        }
        let curves: Vec<_> = sketch.curves.iter().filter(|c| !c.construction).collect();
        let [curve] = curves.as_slice() else {
            return None;
        };
        let SketchGeometry::Circle { .. } = curve.geometry else {
            return None;
        };
        let floor = match &cut.end_condition {
            EndCondition::Blind { distance: depth } => depth.value() < distance.value(),
            EndCondition::ThroughAll => false,
            _ => return None,
        };
        wanted.extend(history_references(
            feature, curve.id, &segments, floor, base_id, &ancestors,
        ));
        ancestors.push(CutOrigin {
            feature,
            tool_curve: curve.id,
            leaves_a_floor: floor,
        });
    }
    Some(wanted)
}

fn candidate_edge_references(objects: &[ObjectRecord], tip: ObjectId) -> Option<Vec<TopologyRef>> {
    let get = |id| objects.iter().find(|o| o.id == id).map(|o| &o.payload);
    let mut cursor = tip;
    let mut seen = BTreeSet::new();
    let mut chain = Vec::new();
    let base = loop {
        if !seen.insert(cursor) {
            return None;
        }
        match get(cursor)? {
            ObjectPayload::Fillet(f) => {
                chain.push((cursor, f.edge, false));
                cursor = f.previous;
            }
            ObjectPayload::Chamfer(c) if chain.is_empty() => {
                chain.push((cursor, c.edge, true));
                cursor = c.previous;
            }
            ObjectPayload::Extrude(e)
                if e.operation == SolidOperation::NewBody
                    && e.previous.is_none()
                    && !e.reversed
                    && matches!(e.end_condition, EndCondition::Blind { .. }) =>
            {
                break e;
            }
            _ => return None,
        }
    };
    let ObjectPayload::Sketch(sketch) = get(base.profile)? else {
        return None;
    };
    let segments = sketch
        .curves
        .iter()
        .filter(|c| !c.construction)
        .map(|c| matches!(c.geometry, SketchGeometry::Line { .. }).then_some(c.id))
        .collect::<Option<Vec<_>>>()?;
    if segments.len() != 4 || (chain.len() != 1 && chain.iter().any(|(_, _, c)| *c)) {
        return None;
    }
    let mut previous = Vec::new();
    let mut wanted = Vec::new();
    for (feature, edge, chamfer) in chain.into_iter().rev() {
        if edge.feature != cursor {
            return None;
        }
        wanted.extend(if chamfer {
            crate::chamfer::chamfer_references(feature, cursor, edge.joint, &segments)
        } else {
            crate::fillet::fillet_references(feature, cursor, edge.joint, &segments)
        });
        for (origin, edge) in &previous {
            wanted.push(crate::fillet::origin_fillet_reference(
                feature, *origin, *edge,
            ));
        }
        previous.push((feature, edge));
    }
    Some(wanted)
}
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
    ancestors: &[CutOrigin],
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

pub(crate) fn same_meaning(a: &TopologyRef, b: &TopologyRef) -> bool {
    a.owner == b.owner
        && a.producer_feature == b.producer_feature
        && a.expected_kind == b.expected_kind
        && a.output_role == b.output_role
        && a.selection == b.selection
        && a.fallback_signature == b.fallback_signature
}
