// SPDX-License-Identifier: MIT
use ferritecad_document::{CapSide, EntityKind, SelectionRule, SemanticRole, TopologyRef};
use ferritecad_kernel::SubShapeHandle;
use ferritecad_types::{CadError, Result};

use crate::map::TopologyMap;

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

            let faces: Vec<SubShapeHandle> = map
                .feature(reference.producer_feature)
                .and_then(|names| names.cap(*side))
                .map(Iterator::collect)
                .unwrap_or_default();
            exactly_one(reference, faces, &format!("the {side:?} cap"))
        }

        SemanticRole::ExtrudeSide { profile_segment } => {
            require_kind(reference, EntityKind::Face, "an extrusion side")?;

            let faces: Vec<SubShapeHandle> = map
                .feature(reference.producer_feature)
                .map(|names| names.side(*profile_segment).collect())
                .unwrap_or_default();

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

            let Some(edges) = map
                .feature(reference.producer_feature)
                .map(|names| names.cap_edge(*side, *profile_segment))
            else {
                return Err(CadError::topology(format!(
                    "topology reference {} names geometry of a feature this rebuild produced \
                     nothing for",
                    reference.id
                )));
            };
            let Some(edges) = edges else {
                return Err(CadError::unsupported(format!(
                    "topology reference {} names extrusion cap side {side:?}, which this build \
                     does not understand",
                    reference.id
                )));
            };
            exactly_one(
                reference,
                edges.collect(),
                &format!("the {side:?} cap edge of segment {profile_segment}"),
            )
        }

        // A real role, and one this slice cannot answer. The kernel emits no
        // shape for a sketch on its own, so there is no edge handle to hand
        // back; inventing one would be a name with nothing behind it.
        SemanticRole::SketchSegment { segment } => Err(CadError::unsupported(format!(
            "topology reference {} names sketch segment {segment} directly, and the geometry \
             kernel does not yet produce a shape for a sketch on its own",
            reference.id
        ))),

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

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_kernel::{
        ExtrudeExtent, ExtrudeRequest, ExtrudeResult, GeometryKernel, OperationContext,
        PlanarPoint, Profile, ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane,
        mock::MockKernel,
    };
    use ferritecad_types::{ErrorKind, ObjectId, StableEntityId};

    struct Fixture {
        map: TopologyMap,
        feature: ObjectId,
        labels: Vec<StableEntityId>,
        result: ExtrudeResult,
    }

    fn fixture() -> Fixture {
        let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let points: Vec<PlanarPoint> = corners
            .iter()
            .map(|(x, y)| PlanarPoint::new(*x, *y).expect("finite"))
            .collect();

        let mut segments = Vec::new();
        let mut labels = Vec::new();
        for (index, start) in points.iter().enumerate() {
            let label = StableEntityId::new();
            labels.push(label);
            segments.push(ProfileSegment::new(
                label,
                SegmentGeometry::line(*start, points[(index + 1) % points.len()])
                    .expect("distinct"),
            ));
        }

        let profile = Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(segments).expect("closes"),
            Vec::new(),
        )
        .expect("valid");
        let request =
            ExtrudeRequest::new(profile, ExtrudeExtent::blind(5.0).expect("positive"), false);

        let mut kernel = MockKernel::new();
        let result = kernel
            .extrude(&request, &OperationContext::default())
            .expect("builds");

        let feature = ObjectId::new();
        let mut map = TopologyMap::new();
        map.record_extrude(feature, request.profile(), &result)
            .expect("records");

        Fixture {
            map,
            feature,
            labels,
            result,
        }
    }

    fn reference(
        feature: ObjectId,
        role: SemanticRole,
        selection: SelectionRule,
        kind: EntityKind,
    ) -> TopologyRef {
        TopologyRef {
            id: StableEntityId::new(),
            owner: feature,
            producer_feature: feature,
            expected_kind: kind,
            output_role: role,
            selection,
            fallback_signature: None,
        }
    }

    #[test]
    fn both_caps_resolve_to_one_face_each() {
        let f = fixture();
        for (side, expected) in [
            (CapSide::Start, &f.result.start_cap),
            (CapSide::End, &f.result.end_cap),
        ] {
            let faces = resolve(
                &f.map,
                &reference(
                    f.feature,
                    SemanticRole::ExtrudeCap { side },
                    SelectionRule::Exact,
                    EntityKind::Face,
                ),
            )
            .expect("a cap resolves");
            assert_eq!(faces, *expected);
        }
    }

    #[test]
    fn a_side_resolves_under_either_rule() {
        let f = fixture();
        let segment = f.labels[1];

        let exact = resolve(
            &f.map,
            &reference(
                f.feature,
                SemanticRole::ExtrudeSide {
                    profile_segment: segment,
                },
                SelectionRule::Exact,
                EntityKind::Face,
            ),
        )
        .expect("one face was raised, so Exact is satisfied");

        let derived = resolve(
            &f.map,
            &reference(
                f.feature,
                SemanticRole::ExtrudeSide {
                    profile_segment: segment,
                },
                SelectionRule::AllDerivedFrom { ancestor: segment },
                EntityKind::Face,
            ),
        )
        .expect("the same face, selected as a family");

        assert_eq!(exact, derived);
        assert_eq!(exact.len(), 1);
    }

    #[test]
    fn a_cap_may_not_be_selected_as_a_family() {
        let f = fixture();
        let err = resolve(
            &f.map,
            &reference(
                f.feature,
                SemanticRole::ExtrudeCap {
                    side: CapSide::Start,
                },
                SelectionRule::AllDerivedFrom {
                    ancestor: f.labels[0],
                },
                EntityKind::Face,
            ),
        )
        .expect_err("a cap descends from no input");
        assert_eq!(err.kind(), ErrorKind::Input);
    }

    #[test]
    fn an_ancestor_that_contradicts_the_role_is_an_input_error() {
        let f = fixture();
        let err = resolve(
            &f.map,
            &reference(
                f.feature,
                SemanticRole::ExtrudeSide {
                    profile_segment: f.labels[0],
                },
                SelectionRule::AllDerivedFrom {
                    ancestor: f.labels[2],
                },
                EntityKind::Face,
            ),
        )
        .expect_err("the reference reads as one thing and selects another");
        assert_eq!(err.kind(), ErrorKind::Input);
    }

    #[test]
    fn a_mismatched_expected_kind_is_an_input_error() {
        let f = fixture();
        for role in [
            SemanticRole::ExtrudeCap { side: CapSide::End },
            SemanticRole::ExtrudeSide {
                profile_segment: f.labels[0],
            },
        ] {
            let err = resolve(
                &f.map,
                &reference(f.feature, role, SelectionRule::Exact, EntityKind::Edge),
            )
            .expect_err("these roles always name faces");
            assert_eq!(err.kind(), ErrorKind::Input);
        }
    }

    #[test]
    fn a_reference_to_a_segment_that_is_gone_is_a_lost_reference() {
        let f = fixture();
        let removed = StableEntityId::new();

        for selection in [
            SelectionRule::Exact,
            SelectionRule::AllDerivedFrom { ancestor: removed },
        ] {
            let err = resolve(
                &f.map,
                &reference(
                    f.feature,
                    SemanticRole::ExtrudeSide {
                        profile_segment: removed,
                    },
                    selection,
                    EntityKind::Face,
                ),
            )
            .expect_err("nothing was raised from a segment that is not there");

            // Topology, not Input: the reference is well formed, the geometry
            // is missing. And emphatically not a neighbouring face.
            assert_eq!(err.kind(), ErrorKind::Topology);
        }
    }

    #[test]
    fn a_reference_to_a_feature_that_produced_nothing_is_a_lost_reference() {
        let f = fixture();
        let err = resolve(
            &f.map,
            &reference(
                ObjectId::new(),
                SemanticRole::ExtrudeCap {
                    side: CapSide::Start,
                },
                SelectionRule::Exact,
                EntityKind::Face,
            ),
        )
        .expect_err("that feature is not in this rebuild");
        assert_eq!(err.kind(), ErrorKind::Topology);
    }

    #[test]
    fn exact_refuses_an_ambiguous_match_rather_than_choosing() {
        // Two faces under one name is the situation where picking either is
        // silently wrong half the time.
        let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let points: Vec<PlanarPoint> = corners
            .iter()
            .map(|(x, y)| PlanarPoint::new(*x, *y).expect("finite"))
            .collect();
        let shared = StableEntityId::new();
        let mut segments = Vec::new();
        let mut labels = Vec::new();
        for (index, start) in points.iter().enumerate() {
            let label = StableEntityId::new();
            labels.push(label);
            segments.push(ProfileSegment::new(
                label,
                SegmentGeometry::line(*start, points[(index + 1) % points.len()])
                    .expect("distinct"),
            ));
        }
        let profile = Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(segments).expect("closes"),
            Vec::new(),
        )
        .expect("valid");
        let request =
            ExtrudeRequest::new(profile, ExtrudeExtent::blind(5.0).expect("positive"), false);

        let mut kernel = MockKernel::new();
        let mut result = kernel
            .extrude(&request, &OperationContext::default())
            .expect("builds");

        // Two distinct faces recorded under one segment.
        let mut history = ferritecad_kernel::History::new();
        for face in [result.start_cap[0], result.end_cap[0]] {
            history.record_generated(ferritecad_kernel::HistoryInput::Segment(shared), face);
        }
        result.history = history;

        let ambiguous_profile_segments = vec![
            ProfileSegment::new(
                shared,
                SegmentGeometry::line(points[0], points[1]).expect("distinct"),
            ),
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(points[1], points[0]).expect("distinct"),
            ),
        ];
        let ambiguous = Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(ambiguous_profile_segments).expect("closes"),
            Vec::new(),
        )
        .expect("valid");

        let feature = ObjectId::new();
        let mut map = TopologyMap::new();
        map.record_extrude(feature, &ambiguous, &result)
            .expect("records");

        let err = resolve(
            &map,
            &reference(
                feature,
                SemanticRole::ExtrudeSide {
                    profile_segment: shared,
                },
                SelectionRule::Exact,
                EntityKind::Face,
            ),
        )
        .expect_err("two faces cannot satisfy an exact selection");
        assert_eq!(err.kind(), ErrorKind::Topology);
        assert!(err.to_string().contains("refusing rather than choosing"));

        // The family rule is the right way to say "however many there are".
        let both = resolve(
            &map,
            &reference(
                feature,
                SemanticRole::ExtrudeSide {
                    profile_segment: shared,
                },
                SelectionRule::AllDerivedFrom { ancestor: shared },
                EntityKind::Face,
            ),
        )
        .expect("a family selection accepts two");
        assert_eq!(both.len(), 2);
    }

    #[test]
    fn a_standalone_sketch_segment_is_an_honest_unsupported() {
        let f = fixture();
        let err = resolve(
            &f.map,
            &reference(
                f.feature,
                SemanticRole::SketchSegment {
                    segment: f.labels[0],
                },
                SelectionRule::Exact,
                EntityKind::Edge,
            ),
        )
        .expect_err("the kernel emits no shape for a sketch");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn a_fillet_face_is_an_honest_unsupported() {
        let f = fixture();
        let err = resolve(
            &f.map,
            &reference(
                f.feature,
                SemanticRole::FilletFace {
                    source_edge: f.labels[0],
                },
                SelectionRule::Exact,
                EntityKind::Face,
            ),
        )
        .expect_err("fillets are not implemented");
        assert_eq!(err.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn the_fallback_signature_is_not_consulted() {
        // A geometric fallback is the last resort in the design and is not
        // implemented here. Supplying one must not turn a lost reference into
        // a resolved one.
        let f = fixture();
        let mut lost = reference(
            f.feature,
            SemanticRole::ExtrudeSide {
                profile_segment: StableEntityId::new(),
            },
            SelectionRule::Exact,
            EntityKind::Face,
        );
        lost.fallback_signature = Some(ferritecad_document::GeomSignature {
            kind: EntityKind::Face,
            measure: 50.0,
            centroid: ferritecad_types::Point3::ORIGIN,
        });

        let err = resolve(&f.map, &lost).expect_err("a signature is not a resolution");
        assert_eq!(err.kind(), ErrorKind::Topology);
    }

    #[test]
    fn resolving_twice_gives_the_same_answer() {
        let f = fixture();
        let reference = reference(
            f.feature,
            SemanticRole::ExtrudeSide {
                profile_segment: f.labels[0],
            },
            SelectionRule::AllDerivedFrom {
                ancestor: f.labels[0],
            },
            EntityKind::Face,
        );

        assert_eq!(
            resolve(&f.map, &reference).expect("resolves"),
            resolve(&f.map, &reference).expect("resolves")
        );
    }
}
