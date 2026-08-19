// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::CapSide;
use ferritecad_kernel::{
    ExtrudeResult, HistoryInput, Profile, ShapeHandle, SubShapeHandle, SubShapeKind,
};
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};

/// What one feature's output is called, in the session that produced it.
///
/// Faces are grouped by the role they play, never by position: the caps are
/// the caps, and a side face is filed under the profile segment it was raised
/// from. There is no index anywhere in this structure, which is what makes it
/// survive a profile gaining or losing a segment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeatureNames {
    shape: Option<ShapeHandle>,
    start_cap: BTreeSet<SubShapeHandle>,
    end_cap: BTreeSet<SubShapeHandle>,
    sides: BTreeMap<StableEntityId, BTreeSet<SubShapeHandle>>,
    /// The edge where each cap meets the face swept from one profile segment,
    /// keyed by the side and then by the segment.
    ///
    /// A set rather than one handle, exactly as `sides` is. The kernel reports
    /// at most one, and this is where a kernel that reported more would be
    /// recorded honestly so the resolver can refuse rather than pick.
    start_cap_edges: BTreeMap<StableEntityId, BTreeSet<SubShapeHandle>>,
    end_cap_edges: BTreeMap<StableEntityId, BTreeSet<SubShapeHandle>>,
}

impl FeatureNames {
    /// The shape these names belong to, if the feature produced one.
    pub fn shape(&self) -> Option<ShapeHandle> {
        self.shape
    }

    /// The faces closing one known end of the sweep, in identifier order.
    ///
    /// `None` means this build does not understand the requested side. It must
    /// not be confused with `Some(empty)`, which means the side is known but
    /// this rebuild produced no cap for it.
    pub fn cap(&self, side: CapSide) -> Option<impl ExactSizeIterator<Item = SubShapeHandle> + '_> {
        let set = match side {
            CapSide::Start => &self.start_cap,
            CapSide::End => &self.end_cap,
            // `CapSide` is non-exhaustive. Treating a future side as either
            // known end would silently retarget the reference.
            _ => return None,
        };
        Some(set.iter().copied())
    }

    /// The faces raised from one profile segment, in identifier order.
    pub fn side(
        &self,
        segment: StableEntityId,
    ) -> impl ExactSizeIterator<Item = SubShapeHandle> + '_ {
        self.sides
            .get(&segment)
            .map(|set| set.iter())
            .unwrap_or_default()
            .copied()
    }

    /// The edges where one known end of the sweep meets the face raised from
    /// one profile segment, in identifier order.
    ///
    /// `None` means this build does not understand the requested side, which
    /// must not be read as "there is no such edge": a future `CapSide` folded
    /// into one of the two known ends would silently retarget the reference.
    pub fn cap_edge(
        &self,
        side: CapSide,
        segment: StableEntityId,
    ) -> Option<impl ExactSizeIterator<Item = SubShapeHandle> + '_> {
        let edges = match side {
            CapSide::Start => &self.start_cap_edges,
            CapSide::End => &self.end_cap_edges,
            _ => return None,
        };
        Some(
            edges
                .get(&segment)
                .map(|set| set.iter())
                .unwrap_or_default()
                .copied(),
        )
    }

    /// Every profile segment this feature named a cap edge for, in identifier
    /// order, on either side.
    pub fn named_cap_edge_segments(&self) -> impl ExactSizeIterator<Item = StableEntityId> {
        let mut segments: BTreeSet<StableEntityId> = self.start_cap_edges.keys().copied().collect();
        segments.extend(self.end_cap_edges.keys().copied());
        segments.into_iter()
    }

    /// Every profile segment this feature raised a face from.
    pub fn named_segments(&self) -> impl ExactSizeIterator<Item = StableEntityId> + '_ {
        self.sides.keys().copied()
    }
}

/// What an archive gave back, before it is checked and filed.
///
/// One value rather than five parameters. They are five parts of one answer —
/// what this feature's geometry is called — and a caller that transposed two
/// same-typed maps would file edges under faces' names with nothing to object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoredNames {
    pub start_cap: Vec<SubShapeHandle>,
    pub end_cap: Vec<SubShapeHandle>,
    pub sides: BTreeMap<StableEntityId, Vec<SubShapeHandle>>,
    pub start_cap_edges: BTreeMap<StableEntityId, Vec<SubShapeHandle>>,
    pub end_cap_edges: BTreeMap<StableEntityId, Vec<SubShapeHandle>>,
}

/// What a whole rebuild produced, addressed by feature and role.
///
/// Ordered containers throughout, and sets rather than lists, so two runs of
/// the same rebuild produce the same answers in the same order and a face
/// recorded twice is recorded once. Iteration order that depended on a hash
/// seed would make a naming bug reproducible only sometimes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TopologyMap {
    features: BTreeMap<ObjectId, FeatureNames>,
}

impl TopologyMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// What a feature is known to have produced.
    pub fn feature(&self, producer: ObjectId) -> Option<&FeatureNames> {
        self.features.get(&producer)
    }

    /// Every feature that produced named geometry, in identifier order.
    pub fn producers(&self) -> impl ExactSizeIterator<Item = ObjectId> + '_ {
        self.features.keys().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Records what an extrusion produced.
    ///
    /// Called with the result still whole, before the caller moves its history
    /// and caps into separate places: the correspondence between the two is
    /// exactly what is being recorded, and reassembling it afterwards from
    /// parts would be inventing it.
    ///
    /// Every handle is checked to be the right kind of sub-shape of this
    /// extrusion's own shape. A handle from somewhere else would be a name
    /// pointing at another feature's geometry, which is the failure this whole
    /// layer exists to prevent, so it is refused here rather than resolved
    /// later.
    pub fn record_extrude(
        &mut self,
        producer: ObjectId,
        profile: &Profile,
        result: &ExtrudeResult,
    ) -> Result<()> {
        // Do not rely on every GeometryKernel implementation remembering to
        // validate its DTO before returning it. The topology boundary is the
        // last place a contradictory durable meaning can be refused before it
        // is filed as an apparently valid name.
        result.validate()?;

        let mut names = FeatureNames {
            shape: Some(result.shape),
            ..FeatureNames::default()
        };
        let profile_segments: BTreeSet<StableEntityId> = profile
            .outer()
            .segments()
            .iter()
            .map(|segment| segment.label)
            .collect();

        for face in &result.start_cap {
            check(*face, result.shape, producer, "an extrusion start cap")?;
            names.start_cap.insert(*face);
        }
        for face in &result.end_cap {
            check(*face, result.shape, producer, "an extrusion end cap")?;
            names.end_cap.insert(*face);
        }

        // Only the outer loop: this slice builds no profile with holes, and a
        // segment of a loop that was never swept has no face to name.
        for segment in profile.outer().segments() {
            for face in result
                .history
                .generated(HistoryInput::Segment(segment.label))
            {
                check(face, result.shape, producer, "an extrusion side")?;
                names.sides.entry(segment.label).or_default().insert(face);
            }
        }

        // The edge each cap leaves against a swept face, filed under the same
        // segment label its face is. Checked to be an edge of this feature's
        // own shape for the same reason the faces are: a handle from anywhere
        // else is a name pointing at another feature's geometry.
        for (edges, into, what) in [
            (
                &result.start_cap_edges,
                &mut names.start_cap_edges,
                "an extrusion start cap edge",
            ),
            (
                &result.end_cap_edges,
                &mut names.end_cap_edges,
                "an extrusion end cap edge",
            ),
        ] {
            for (segment, edge) in edges {
                if !profile_segments.contains(segment) {
                    return Err(CadError::topology(format!(
                        "feature {producer} reported {what} for segment {segment}, which is not \
                         in the swept outer profile"
                    )));
                }
                check_kind(*edge, result.shape, producer, what, SubShapeKind::Edge)?;
                into.entry(*segment).or_default().insert(*edge);
            }
        }

        // Replacing rather than merging: a feature is rebuilt whole, and
        // merging would let a stale name from a previous attempt survive.
        self.features.insert(producer, names);
        Ok(())
    }

    /// Records names restored from an archive rather than from an operation.
    ///
    /// The same checks as a fresh record: every sub-shape must have the right
    /// kind and belong to the shape it was restored with. What is deliberately
    /// absent is history — an archive carries the sub-shapes that were named,
    /// not how they were made — so this cannot be used to fake a rebuild.
    pub fn record_restored(
        &mut self,
        producer: ObjectId,
        shape: ShapeHandle,
        restored: &RestoredNames,
    ) -> Result<()> {
        let mut names = FeatureNames {
            shape: Some(shape),
            ..FeatureNames::default()
        };

        for (faces, into) in [
            (&restored.start_cap, &mut names.start_cap),
            (&restored.end_cap, &mut names.end_cap),
        ] {
            for face in faces {
                check(*face, shape, producer, "a restored extrusion cap")?;
                into.insert(*face);
            }
        }
        for (segment, faces) in &restored.sides {
            for face in faces {
                check(*face, shape, producer, "a restored extrusion side")?;
                names.sides.entry(*segment).or_default().insert(*face);
            }
        }
        let mut claimed_edges: BTreeMap<SubShapeHandle, (&str, StableEntityId)> = BTreeMap::new();
        for (side, edges, into, what) in [
            (
                "start",
                &restored.start_cap_edges,
                &mut names.start_cap_edges,
                "a restored extrusion start cap edge",
            ),
            (
                "end",
                &restored.end_cap_edges,
                &mut names.end_cap_edges,
                "a restored extrusion end cap edge",
            ),
        ] {
            for (segment, handles) in edges {
                for edge in handles {
                    check_kind(*edge, shape, producer, what, SubShapeKind::Edge)?;
                    if let Some((other_side, other_segment)) =
                        claimed_edges.insert(*edge, (side, *segment))
                        && (other_side != side || other_segment != *segment)
                    {
                        return Err(CadError::topology(format!(
                            "feature {producer} restored the {other_side} cap edge of segment \
                             {other_segment} and the {side} cap edge of segment {segment} as \
                             the same edge"
                        )));
                    }
                    into.entry(*segment).or_default().insert(*edge);
                }
            }
        }

        self.features.insert(producer, names);
        Ok(())
    }
}

fn check(face: SubShapeHandle, shape: ShapeHandle, producer: ObjectId, what: &str) -> Result<()> {
    check_kind(face, shape, producer, what, SubShapeKind::Face)
}

/// Refuses a name that is the wrong sort of thing or belongs to another shape.
///
/// One statement for faces and edges alike. A handle of the wrong kind would
/// let a reference expecting an edge resolve to a face, and one of another
/// shape would point at a different feature's geometry: both are exactly the
/// silent retargeting this layer exists to prevent.
fn check_kind(
    sub: SubShapeHandle,
    shape: ShapeHandle,
    producer: ObjectId,
    what: &str,
    expected: SubShapeKind,
) -> Result<()> {
    if sub.kind() != expected {
        return Err(CadError::topology(format!(
            "feature {producer} reported {what} as a {}, which is not a {expected}",
            sub.kind()
        )));
    }
    if sub.shape() != shape {
        return Err(CadError::topology(format!(
            "feature {producer} reported {what} belonging to {}, not to the shape it built",
            sub.shape()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_kernel::{
        ExtrudeExtent, ExtrudeRequest, GeometryKernel, History, OperationContext, PlanarPoint,
        ProfileLoop, ProfileSegment, SegmentGeometry, SessionId, SketchPlane, mock::MockKernel,
    };

    struct Square {
        request: ExtrudeRequest,
        labels: Vec<StableEntityId>,
    }

    fn square() -> Square {
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

        Square {
            request: ExtrudeRequest::new(
                profile,
                ExtrudeExtent::blind(5.0).expect("positive"),
                false,
            ),
            labels,
        }
    }

    fn built(kernel: &mut MockKernel, square: &Square) -> ExtrudeResult {
        kernel
            .extrude(&square.request, &OperationContext::default())
            .expect("the mock builds")
    }

    #[test]
    fn an_extrusion_is_recorded_by_role() {
        let square = square();
        let mut kernel = MockKernel::new();
        let result = built(&mut kernel, &square);
        let feature = ObjectId::new();

        let mut map = TopologyMap::new();
        map.record_extrude(feature, square.request.profile(), &result)
            .expect("the mock reports faces of its own shape");

        let names = map.feature(feature).expect("the feature is recorded");
        assert_eq!(names.shape(), Some(result.shape));
        assert_eq!(
            names
                .cap(CapSide::Start)
                .expect("the start side is known")
                .count(),
            1
        );
        assert_eq!(
            names
                .cap(CapSide::End)
                .expect("the end side is known")
                .count(),
            1
        );
        for label in &square.labels {
            assert_eq!(names.side(*label).count(), 1, "segment {label}");
        }
    }

    #[test]
    fn an_unnamed_segment_has_no_faces_rather_than_someone_elses() {
        let square = square();
        let mut kernel = MockKernel::new();
        let result = built(&mut kernel, &square);

        let mut map = TopologyMap::new();
        let feature = ObjectId::new();
        map.record_extrude(feature, square.request.profile(), &result)
            .expect("records");

        let stranger = StableEntityId::new();
        assert_eq!(
            map.feature(feature)
                .expect("recorded")
                .side(stranger)
                .count(),
            0
        );
    }

    #[test]
    fn a_face_belonging_to_another_shape_is_refused() {
        let square = square();
        let mut kernel = MockKernel::new();
        let mut result = built(&mut kernel, &square);

        // A handle from a shape this feature did not build is exactly the
        // mistake that ends in a reference naming the wrong solid.
        let elsewhere = ShapeHandle::new(SessionId::new(), 99);
        result.start_cap = vec![SubShapeHandle::new(elsewhere, SubShapeKind::Face, 0)];

        let err = TopologyMap::new()
            .record_extrude(ObjectId::new(), square.request.profile(), &result)
            .expect_err("a foreign face must not be recorded");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Topology);
    }

    #[test]
    fn a_handle_that_is_not_a_face_is_refused() {
        let square = square();
        let mut kernel = MockKernel::new();
        let mut result = built(&mut kernel, &square);
        result.end_cap = vec![SubShapeHandle::new(result.shape, SubShapeKind::Edge, 0)];

        let err = TopologyMap::new()
            .record_extrude(ObjectId::new(), square.request.profile(), &result)
            .expect_err("a cap is a face");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Topology);
    }

    #[test]
    fn a_kernel_result_cannot_file_one_edge_under_both_caps() {
        let square = square();
        let mut kernel = MockKernel::new();
        let mut result = built(&mut kernel, &square);
        let edge = SubShapeHandle::new(result.shape, SubShapeKind::Edge, 100);
        result.start_cap_edges.insert(square.labels[0], edge);
        result.end_cap_edges.insert(square.labels[1], edge);

        let err = TopologyMap::new()
            .record_extrude(ObjectId::new(), square.request.profile(), &result)
            .expect_err("one edge cannot acquire two durable meanings");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Kernel);
    }

    #[test]
    fn a_cap_edge_cannot_be_filed_under_a_segment_outside_the_profile() {
        let square = square();
        let mut kernel = MockKernel::new();
        let mut result = built(&mut kernel, &square);
        let invented = StableEntityId::new();
        result.start_cap_edges.insert(
            invented,
            SubShapeHandle::new(result.shape, SubShapeKind::Edge, 100),
        );

        let err = TopologyMap::new()
            .record_extrude(ObjectId::new(), square.request.profile(), &result)
            .expect_err("an unknown segment cannot become a durable edge name");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Topology);
        assert!(err.to_string().contains(&invented.to_string()));
    }

    #[test]
    fn an_archive_result_cannot_file_one_edge_under_both_caps() {
        let shape = ShapeHandle::new(SessionId::new(), 1);
        let edge = SubShapeHandle::new(shape, SubShapeKind::Edge, 100);
        let one = StableEntityId::new();
        let other = StableEntityId::new();
        let restored = RestoredNames {
            start_cap_edges: BTreeMap::from([(one, vec![edge])]),
            end_cap_edges: BTreeMap::from([(other, vec![edge])]),
            ..RestoredNames::default()
        };

        let err = TopologyMap::new()
            .record_restored(ObjectId::new(), shape, &restored)
            .expect_err("an archived edge cannot acquire two durable meanings");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Topology);
    }

    #[test]
    fn recording_the_same_face_twice_records_it_once() {
        let square = square();
        let mut kernel = MockKernel::new();
        let mut result = built(&mut kernel, &square);

        let cap = result.start_cap[0];
        result.start_cap = vec![cap, cap, cap];

        let feature = ObjectId::new();
        let mut map = TopologyMap::new();
        map.record_extrude(feature, square.request.profile(), &result)
            .expect("records");

        assert_eq!(
            map.feature(feature)
                .expect("recorded")
                .cap(CapSide::Start)
                .expect("the start side is known")
                .count(),
            1
        );
    }

    #[test]
    fn the_order_entries_arrive_in_does_not_change_the_map() {
        let square = square();
        let mut kernel = MockKernel::new();
        let result = built(&mut kernel, &square);
        let first = ObjectId::new();
        let second = ObjectId::new();

        let mut forwards = TopologyMap::new();
        forwards
            .record_extrude(first, square.request.profile(), &result)
            .expect("records");
        forwards
            .record_extrude(second, square.request.profile(), &result)
            .expect("records");

        let mut backwards = TopologyMap::new();
        backwards
            .record_extrude(second, square.request.profile(), &result)
            .expect("records");
        backwards
            .record_extrude(first, square.request.profile(), &result)
            .expect("records");

        assert_eq!(forwards, backwards);
        assert_eq!(
            forwards.producers().collect::<Vec<_>>(),
            backwards.producers().collect::<Vec<_>>()
        );
    }

    #[test]
    fn rebuilding_a_feature_replaces_its_names_rather_than_adding_to_them() {
        let square = square();
        let mut kernel = MockKernel::new();
        let feature = ObjectId::new();

        let first = built(&mut kernel, &square);
        let mut map = TopologyMap::new();
        map.record_extrude(feature, square.request.profile(), &first)
            .expect("records");

        let second = built(&mut kernel, &square);
        map.record_extrude(feature, square.request.profile(), &second)
            .expect("records");

        let names = map.feature(feature).expect("recorded");
        assert_eq!(names.shape(), Some(second.shape));
        assert_eq!(
            names
                .cap(CapSide::Start)
                .expect("the start side is known")
                .count(),
            1,
            "no stale cap survives"
        );
        assert_eq!(
            names
                .cap(CapSide::Start)
                .expect("the start side is known")
                .next()
                .map(|f| f.shape()),
            Some(second.shape)
        );
    }

    #[test]
    fn a_feature_with_no_history_records_only_its_caps() {
        let square = square();
        let mut kernel = MockKernel::new();
        let mut result = built(&mut kernel, &square);
        result.history = History::new();

        let feature = ObjectId::new();
        let mut map = TopologyMap::new();
        map.record_extrude(feature, square.request.profile(), &result)
            .expect("records");

        let names = map.feature(feature).expect("recorded");
        assert_eq!(names.named_segments().count(), 0);
        assert_eq!(
            names
                .cap(CapSide::End)
                .expect("the end side is known")
                .count(),
            1
        );
    }
}
