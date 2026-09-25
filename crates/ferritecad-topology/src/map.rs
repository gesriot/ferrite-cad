// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::CapSide;
use ferritecad_kernel::{
    CutResult, ExtrudeResult, HistoryInput, Profile, RevolveResult, ShapeHandle, SubShapeHandle,
    SubShapeKind,
};
use ferritecad_types::{CadError, ObjectId, ProfileJoint, Result, StableEntityId};

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
    /// The edge running along the sweep at each corner of the profile, keyed
    /// by the pair of segments meeting there. A set for the same reason as
    /// above: a kernel reporting more than one is recorded as it answered so
    /// the resolver can refuse instead of choosing.
    sweep_edges: BTreeMap<ProfileJoint, BTreeSet<SubShapeHandle>>,
    /// The vertex where each corner of the profile reaches each cap, keyed by
    /// the side and then by the pair of segments meeting there.
    ///
    /// Kept apart from the edges rather than folded in beside them. A vertex
    /// and an edge are different sorts of geometry, and one map holding both
    /// would let a name written as a corner be read back as the edge along it.
    /// Sets for the same reason as everywhere above: a kernel that answered
    /// twice is recorded as it answered, so the resolver refuses instead of
    /// choosing.
    start_cap_vertices: BTreeMap<ProfileJoint, BTreeSet<SubShapeHandle>>,
    end_cap_vertices: BTreeMap<ProfileJoint, BTreeSet<SubShapeHandle>>,
    /// The face of revolution each profile Line raised, for a Revolve.
    ///
    /// Apart from `sides` on purpose: a turned face and a swept face are
    /// different meanings, so an extrusion-side reference can never resolve to
    /// a face of revolution, nor the other way round.
    revolved: BTreeMap<StableEntityId, BTreeSet<SubShapeHandle>>,
    /// Immediate predecessor, for the unchanged legacy CarriedCap/Side roles.
    previous: Option<ObjectId>,
    /// Original producer and role, never reassigned by an intervening boolean.
    carried: BTreeMap<(ObjectId, CarriedName), BTreeSet<SubShapeHandle>>,
    carried_deleted: BTreeSet<(ObjectId, CarriedName)>,
}

/// One name an earlier feature had, as a later feature refers back to it.
///
/// Flat rather than a nested role: a role that contained a role would have to
/// be boxed, and the two cases a boolean can carry forward in this slice are
/// exactly these. Which of them a handle is filed under is decided from the
/// earlier feature's own names, never from the geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CarriedName {
    Cap(CapSide),
    Side(StableEntityId),
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

    /// The edges running along the sweep at one corner of the profile, in
    /// identifier order.
    ///
    /// An empty iterator means this rebuild named no edge for that joint,
    /// which is what a corner whose pair is not unique, or whose sweep
    /// produced nothing, honestly is.
    pub fn sweep_edge(
        &self,
        joint: ProfileJoint,
    ) -> impl ExactSizeIterator<Item = SubShapeHandle> + '_ {
        self.sweep_edges
            .get(&joint)
            .map(|set| set.iter())
            .unwrap_or_default()
            .copied()
    }

    /// Every joint this feature named an edge along the sweep for.
    pub fn named_joints(&self) -> impl ExactSizeIterator<Item = ProfileJoint> + '_ {
        self.sweep_edges.keys().copied()
    }

    /// The vertices where one corner of the profile reaches one known end of
    /// the sweep, in identifier order.
    ///
    /// `None` means this build does not understand the requested side, which
    /// again must not be read as "there is no such vertex": a future `CapSide`
    /// folded into one of the two known ends would silently retarget the
    /// reference to the other end of the same corner.
    ///
    /// An empty iterator means this rebuild named no vertex for that corner,
    /// which is what a corner whose pair occurs twice honestly is.
    pub fn cap_vertex(
        &self,
        side: CapSide,
        joint: ProfileJoint,
    ) -> Option<impl ExactSizeIterator<Item = SubShapeHandle> + '_> {
        let vertices = match side {
            CapSide::Start => &self.start_cap_vertices,
            CapSide::End => &self.end_cap_vertices,
            _ => return None,
        };
        Some(
            vertices
                .get(&joint)
                .map(|set| set.iter())
                .unwrap_or_default()
                .copied(),
        )
    }

    /// Every joint this feature named a cap vertex for, in identifier order,
    /// on either side.
    ///
    /// The archive walks this so the order it writes names in is the same on
    /// every machine and every run.
    pub fn named_cap_vertex_joints(&self) -> impl ExactSizeIterator<Item = ProfileJoint> {
        let mut joints: BTreeSet<ProfileJoint> = self.start_cap_vertices.keys().copied().collect();
        joints.extend(self.end_cap_vertices.keys().copied());
        joints.into_iter()
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

    /// The faces of revolution one profile Line raised; empty for a Line this
    /// feature did not turn, including every extrusion's Lines.
    pub fn revolved_face(
        &self,
        profile_segment: StableEntityId,
    ) -> impl Iterator<Item = SubShapeHandle> + '_ {
        self.revolved
            .get(&profile_segment)
            .into_iter()
            .flat_map(|faces| faces.iter().copied())
    }

    /// Every profile Line this feature turned into a face.
    pub fn named_revolved_segments(&self) -> impl ExactSizeIterator<Item = StableEntityId> + '_ {
        self.revolved.keys().copied()
    }

    /// The faces an earlier feature's cap became, as this feature leaves it.
    ///
    /// `None` for a side this build does not understand, exactly as
    /// [`Self::cap`] is. An empty iterator with the name recorded as deleted is
    /// a different fact from an empty one with nothing recorded, and
    /// [`Self::carried_is_deleted`] is how a resolver tells them apart.
    pub fn carried_cap(&self, side: CapSide) -> Option<impl Iterator<Item = SubShapeHandle> + '_> {
        match side {
            CapSide::Start | CapSide::End => Some(
                self.previous
                    .into_iter()
                    .flat_map(move |origin| self.origin_faces(origin, CarriedName::Cap(side))),
            ),
            _ => None,
        }
    }

    pub fn carried_side(
        &self,
        segment: StableEntityId,
    ) -> impl Iterator<Item = SubShapeHandle> + '_ {
        self.previous
            .into_iter()
            .flat_map(move |origin| self.origin_faces(origin, CarriedName::Side(segment)))
    }

    pub fn previous(&self) -> Option<ObjectId> {
        self.previous
    }

    pub fn origin_faces(
        &self,
        origin: ObjectId,
        name: CarriedName,
    ) -> impl ExactSizeIterator<Item = SubShapeHandle> + '_ {
        self.carried
            .get(&(origin, name))
            .map(|s| s.iter())
            .unwrap_or_default()
            .copied()
    }

    pub fn origin_is_deleted(&self, origin: ObjectId, name: CarriedName) -> bool {
        self.carried_deleted.contains(&(origin, name))
    }

    pub fn carried_is_deleted(&self, name: CarriedName) -> bool {
        self.previous
            .is_some_and(|origin| self.origin_is_deleted(origin, name))
    }

    pub fn named_face_count(&self) -> usize {
        self.start_cap.len()
            + self.end_cap.len()
            + self.sides.values().map(BTreeSet::len).sum::<usize>()
            + self.carried.values().map(BTreeSet::len).sum::<usize>()
    }

    /// Every qualified name, including deleted ancestors, in deterministic order.
    pub fn origins(&self) -> impl ExactSizeIterator<Item = (ObjectId, CarriedName)> {
        let mut names = self.carried_deleted.clone();
        names.extend(self.carried.keys().copied());
        names.into_iter()
    }

    pub fn carried_names(&self) -> impl Iterator<Item = CarriedName> {
        let previous = self.previous;
        self.origins()
            .filter_map(move |(origin, name)| (Some(origin) == previous).then_some(name))
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
    pub sweep_edges: BTreeMap<ProfileJoint, Vec<SubShapeHandle>>,
    pub start_cap_vertices: BTreeMap<ProfileJoint, Vec<SubShapeHandle>>,
    pub end_cap_vertices: BTreeMap<ProfileJoint, Vec<SubShapeHandle>>,
    pub previous: Option<ObjectId>,
    pub carried: BTreeMap<(ObjectId, CarriedName), Vec<SubShapeHandle>>,
    /// Faces of revolution by the Line that raised them.
    pub revolved: BTreeMap<StableEntityId, Vec<SubShapeHandle>>,
    pub carried_deleted: BTreeSet<(ObjectId, CarriedName)>,
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
        // Every loop of the profile, not only the one that bounds it. A
        // segment of a hole was swept exactly as a segment of the boundary was,
        // and it raised a face of the same solid; a check that only knew the
        // boundary would refuse the bore's own name as pointing at nothing.
        let loops = || std::iter::once(profile.outer()).chain(profile.inner());
        let profile_segments: BTreeSet<StableEntityId> = loops()
            .flat_map(|entry| entry.segments())
            .map(|segment| segment.label)
            .collect();
        // Counted across every loop together, because a pair that arises at a
        // corner of the boundary *and* at a corner of a hole names neither of
        // them, exactly as a pair arising twice within one loop does.
        let mut profile_joints: BTreeMap<ProfileJoint, usize> = BTreeMap::new();
        for joint in loops().flat_map(|entry| entry.joints()) {
            *profile_joints.entry(joint).or_insert(0) += 1;
        }

        for face in &result.start_cap {
            check(*face, result.shape, producer, "an extrusion start cap")?;
            names.start_cap.insert(*face);
        }
        for face in &result.end_cap {
            check(*face, result.shape, producer, "an extrusion end cap")?;
            names.end_cap.insert(*face);
        }

        // One entry per segment of every loop: each raised its own faces, and
        // the bore's wall belongs to the circle that drew it just as the outer
        // wall belongs to the circle that drew that.
        for segment in loops().flat_map(|entry| entry.segments()) {
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
                         in the swept profile"
                    )));
                }
                check_kind(*edge, result.shape, producer, what, SubShapeKind::Edge)?;
                into.entry(*segment).or_default().insert(*edge);
            }
        }

        // The edge along the sweep at each corner, filed under the pair that
        // names it. Both of the pair must be segments of the profile that was
        // actually swept, they must meet, and they must meet at exactly one
        // corner. Membership alone is insufficient: non-adjacent segments do
        // not name a corner, while the one unordered pair in a two-segment
        // loop occurs at both corners and cannot choose between them.
        for (joint, edge) in &result.sweep_edges {
            for segment in joint.segments() {
                if !profile_segments.contains(&segment) {
                    return Err(CadError::topology(format!(
                        "feature {producer} reported an extrusion sweep edge for {joint}, and \
                         segment {segment} is not in the swept profile"
                    )));
                }
            }
            match profile_joints.get(joint).copied() {
                None => {
                    return Err(CadError::topology(format!(
                        "feature {producer} reported an extrusion sweep edge for {joint}, but its \
                         segments do not meet in the swept profile"
                    )));
                }
                Some(1) => {}
                Some(2) => {
                    return Err(CadError::topology(format!(
                        "feature {producer} reported an extrusion sweep edge for {joint}, but the \
                         pair meets at two corners and names neither"
                    )));
                }
                Some(corners) => {
                    return Err(CadError::topology(format!(
                        "feature {producer} reported an extrusion sweep edge for {joint}, but the \
                         pair meets at {corners} corners and names none of them"
                    )));
                }
            }
            check_kind(
                *edge,
                result.shape,
                producer,
                "an extrusion sweep edge",
                SubShapeKind::Edge,
            )?;
            names.sweep_edges.entry(*joint).or_default().insert(*edge);
        }

        // The vertex each corner reaches on each cap, filed under the same
        // pair that names the edge along it. The same three questions as the
        // sweep edges above, asked again rather than assumed from them: a
        // kernel may report a vertex for a corner it reported no edge for, and
        // a pair that names no corner must not acquire a durable meaning
        // through the other map.
        let mut cornered: BTreeMap<SubShapeHandle, (CapSide, ProfileJoint)> = BTreeMap::new();
        for (side, vertices, what) in [
            (
                CapSide::Start,
                &result.start_cap_vertices,
                "an extrusion start cap vertex",
            ),
            (
                CapSide::End,
                &result.end_cap_vertices,
                "an extrusion end cap vertex",
            ),
        ] {
            for (joint, vertex) in vertices {
                for segment in joint.segments() {
                    if !profile_segments.contains(&segment) {
                        return Err(CadError::topology(format!(
                            "feature {producer} reported {what} for {joint}, and segment \
                             {segment} is not in the swept profile"
                        )));
                    }
                }
                match profile_joints.get(joint).copied() {
                    None => {
                        return Err(CadError::topology(format!(
                            "feature {producer} reported {what} for {joint}, but its segments do \
                             not meet in the swept profile"
                        )));
                    }
                    Some(1) => {}
                    Some(corners) => {
                        // The two corners of a two-segment loop are different
                        // points, and the pair naming both of them names
                        // neither. Taking the first would be a durable name
                        // for whichever the kernel happened to answer with.
                        return Err(CadError::topology(format!(
                            "feature {producer} reported {what} for {joint}, but the pair meets \
                             at {corners} corners and names none of them"
                        )));
                    }
                }
                check_kind(*vertex, result.shape, producer, what, SubShapeKind::Vertex)?;
                // One physical vertex carries one durable meaning. Two corners
                // claiming it, or one corner claiming it on both caps, would
                // be two references resolving to the same point.
                if let Some((other_side, other_joint)) = cornered.insert(*vertex, (side, *joint))
                    && (other_side != side || other_joint != *joint)
                {
                    return Err(CadError::topology(format!(
                        "feature {producer} reported the {other_side:?} cap vertex of \
                         {other_joint} and the {side:?} cap vertex of {joint} as the same vertex"
                    )));
                }
                let into = match side {
                    CapSide::Start => &mut names.start_cap_vertices,
                    CapSide::End => &mut names.end_cap_vertices,
                    // `CapSide` is non-exhaustive, and the loop above names
                    // only the two ends this build knows. Unreachable, and
                    // refused rather than filed under a guess.
                    _ => {
                        return Err(CadError::topology(format!(
                            "feature {producer} reported {what} for cap side {side:?}, which this \
                             build does not understand"
                        )));
                    }
                };
                into.entry(*joint).or_default().insert(*vertex);
            }
        }

        // Replacing rather than merging: a feature is rebuilt whole, and
        // merging would let a stale name from a previous attempt survive.
        self.features.insert(producer, names);
        Ok(())
    }

    /// Records what a revolution produced: one face of revolution per Line.
    ///
    /// Every face must be a face of this revolution's own shape, filed under a
    /// Line of the turned profile, and every Line must have raised one. A
    /// revolution has no caps, cap edges, sweep edges or corner vertices, and
    /// nothing is filed under those names.
    pub fn record_revolve(
        &mut self,
        producer: ObjectId,
        profile: &Profile,
        result: &RevolveResult,
    ) -> Result<()> {
        result.validate()?;
        if !profile.inner().is_empty() {
            return Err(CadError::topology(format!(
                "feature {producer} turned a profile with holes, which this build does not name"
            )));
        }
        let lines: BTreeSet<StableEntityId> = profile
            .outer()
            .segments()
            .iter()
            .map(|segment| segment.label)
            .collect();
        for input in result.history.inputs() {
            let HistoryInput::Segment(label) = input else {
                return Err(CadError::topology(format!(
                    "feature {producer} reported revolution history for {input:?}, which is not a \
                     profile Line"
                )));
            };
            if !lines.contains(&label) {
                return Err(CadError::topology(format!(
                    "feature {producer} reported a face for segment {label}, which is not in the \
                     turned profile"
                )));
            }
        }
        let mut names = FeatureNames {
            shape: Some(result.shape),
            ..FeatureNames::default()
        };
        let mut claimed: BTreeMap<SubShapeHandle, StableEntityId> = BTreeMap::new();
        for label in &lines {
            let faces: Vec<_> = result
                .history
                .generated(HistoryInput::Segment(*label))
                .collect();
            if faces.is_empty() {
                return Err(CadError::topology(format!(
                    "feature {producer} raised no face from Line {label}"
                )));
            }
            for face in faces {
                check(face, result.shape, producer, "a face of revolution")?;
                if let Some(other) = claimed.insert(face, *label)
                    && other != *label
                {
                    return Err(CadError::topology(format!(
                        "feature {producer} reported one face for Lines {other} and {label}"
                    )));
                }
                names.revolved.entry(*label).or_default().insert(face);
            }
        }
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
        if !restored.revolved.is_empty()
            && (!restored.sides.is_empty()
                || !restored.start_cap.is_empty()
                || !restored.end_cap.is_empty()
                || restored.previous.is_some())
        {
            return Err(CadError::topology(format!(
                "feature {producer} restored faces of revolution beside extrusion names; one \
                 feature is one or the other"
            )));
        }
        let mut turned: BTreeMap<SubShapeHandle, StableEntityId> = BTreeMap::new();
        for (segment, faces) in &restored.revolved {
            for face in faces {
                check(*face, shape, producer, "a restored face of revolution")?;
                if let Some(other) = turned.insert(*face, *segment)
                    && other != *segment
                {
                    return Err(CadError::topology(format!(
                        "feature {producer} restored one face for Lines {other} and {segment}"
                    )));
                }
                names.revolved.entry(*segment).or_default().insert(*face);
            }
        }
        names.previous = restored.previous;
        for (name, faces) in &restored.carried {
            if name.0 == producer || restored.previous.is_none() {
                return Err(CadError::topology(
                    "a carried face requires an earlier producer",
                ));
            }
            for face in faces {
                check(*face, shape, producer, "a restored origin face")?;
                names.carried.entry(*name).or_default().insert(*face);
            }
        }
        for name in &restored.carried_deleted {
            if names
                .carried
                .get(name)
                .is_some_and(|faces| !faces.is_empty())
            {
                return Err(CadError::topology(
                    "an origin face is both restored and removed",
                ));
            }
            names.carried_deleted.insert(*name);
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

        let mut claimed_joints: BTreeMap<SubShapeHandle, ProfileJoint> = BTreeMap::new();
        for (joint, handles) in &restored.sweep_edges {
            for edge in handles {
                check_kind(
                    *edge,
                    shape,
                    producer,
                    "a restored extrusion sweep edge",
                    SubShapeKind::Edge,
                )?;
                if let Some((side, segment)) = claimed_edges.get(edge) {
                    return Err(CadError::topology(format!(
                        "feature {producer} restored {edge} both as the {side} cap edge of \
                         segment {segment} and as the sweep edge of {joint}"
                    )));
                }
                if let Some(other) = claimed_joints.insert(*edge, *joint)
                    && other != *joint
                {
                    return Err(CadError::topology(format!(
                        "feature {producer} restored the sweep edges of {other} and of {joint} \
                         as the same edge"
                    )));
                }
                names.sweep_edges.entry(*joint).or_default().insert(*edge);
            }
        }

        // The cap vertices, on their own terms. A vertex cannot collide with a
        // restored edge — `check_kind` has already separated them — so what is
        // asked here is the question the kinds cannot answer: whether two
        // corners, or the two ends of one corner, came back as one point.
        let mut claimed_vertices: BTreeMap<SubShapeHandle, (CapSide, ProfileJoint)> =
            BTreeMap::new();
        for (side, vertices, into, what) in [
            (
                CapSide::Start,
                &restored.start_cap_vertices,
                &mut names.start_cap_vertices,
                "a restored extrusion start cap vertex",
            ),
            (
                CapSide::End,
                &restored.end_cap_vertices,
                &mut names.end_cap_vertices,
                "a restored extrusion end cap vertex",
            ),
        ] {
            for (joint, handles) in vertices {
                for vertex in handles {
                    check_kind(*vertex, shape, producer, what, SubShapeKind::Vertex)?;
                    if let Some((other_side, other_joint)) =
                        claimed_vertices.insert(*vertex, (side, *joint))
                        && (other_side != side || other_joint != *joint)
                    {
                        return Err(CadError::topology(format!(
                            "feature {producer} restored the {other_side:?} cap vertex of \
                             {other_joint} and the {side:?} cap vertex of {joint} as the same \
                             vertex"
                        )));
                    }
                    into.entry(*joint).or_default().insert(*vertex);
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

/// What one boolean produced, in the vocabulary a document can store.
///
/// Three groups of names come out of one cut, and they are three different
/// things:
///
/// * the **tool's** own faces, which are what bounds the new cavity. They are
///   filed under the roles the tool's extrusion gave them — a side per profile
///   segment, a cap per end — because that is what they are: the wall of a hole
///   is the wall the tool swept, and the floor of a pocket is the tool's end
///   cap that the boolean kept.
/// * the **earlier feature's** faces, filed as carried names, because a
///   reference to the part's top face and a reference to the floor of a hole in
///   it must not be the same reference.
/// * everything the boolean **removed**, recorded as removed, so a reference to
///   it is refused rather than quietly answered with nothing.
///
/// Every answer comes from `result`, which comes from the kernel's own history.
/// Nothing here looks at geometry, counts faces or picks a nearest match.
impl TopologyMap {
    #[allow(clippy::too_many_arguments)]
    pub fn record_cut(
        &mut self,
        producer: ObjectId,
        previous: ObjectId,
        tool_profile: &Profile,
        tool_shape: ShapeHandle,
        tool_names: &FeatureNames,
        previous_names: &FeatureNames,
        result: &CutResult,
    ) -> Result<()> {
        // The kernel's own boundary check, asked again here for the reason the
        // extrusion's is: this is the last place a contradictory durable
        // meaning can be refused before it is filed as an apparently valid one.
        let Some(previous_shape) = previous_names.shape() else {
            return Err(CadError::topology(format!(
                "feature {producer} modifies {previous}, which produced no shape"
            )));
        };
        result.validate(previous_shape, tool_shape)?;

        let mut names = FeatureNames {
            shape: Some(result.shape),
            ..FeatureNames::default()
        };

        // What each input name became, asked of the history and of nothing
        // else. `outputs` is empty exactly when the boolean removed the face.
        let outputs = |input: SubShapeHandle| -> Vec<SubShapeHandle> {
            result
                .history
                .modified(HistoryInput::SubShape(input))
                .chain(result.history.generated(HistoryInput::SubShape(input)))
                .collect()
        };

        // The tool's contributions, under the roles the tool already had.
        let tool_loops = || std::iter::once(tool_profile.outer()).chain(tool_profile.inner());
        for segment in tool_loops().flat_map(|entry| entry.segments()) {
            for face in tool_names.side(segment.label) {
                for out in outputs(face) {
                    check(out, result.shape, producer, "a cut wall")?;
                    names.sides.entry(segment.label).or_default().insert(out);
                }
            }
        }
        for side in [CapSide::Start, CapSide::End] {
            let Some(faces) = tool_names.cap(side) else {
                continue;
            };
            for face in faces {
                for out in outputs(face) {
                    check(out, result.shape, producer, "a cut cap")?;
                    match side {
                        CapSide::Start => names.start_cap.insert(out),
                        CapSide::End => names.end_cap.insert(out),
                        _ => unreachable!("the two sides are matched above"),
                    };
                }
            }
        }

        names.previous = Some(previous);
        let mut inputs: BTreeMap<(ObjectId, CarriedName), Vec<SubShapeHandle>> = BTreeMap::new();
        for side in [CapSide::Start, CapSide::End] {
            inputs.insert(
                (previous, CarriedName::Cap(side)),
                previous_names.cap(side).into_iter().flatten().collect(),
            );
        }
        for segment in previous_names.named_segments() {
            inputs.insert(
                (previous, CarriedName::Side(segment)),
                previous_names.side(segment).collect(),
            );
        }
        for (origin, name) in previous_names.origins() {
            inputs.insert(
                (origin, name),
                previous_names.origin_faces(origin, name).collect(),
            );
        }
        for (name, faces) in inputs {
            let mut carried = BTreeSet::new();
            for face in faces {
                if !result.carried.contains_key(&face) {
                    return Err(CadError::topology(
                        "boolean history omitted a named input face",
                    ));
                }
                for out in outputs(face) {
                    check(out, result.shape, producer, "a carried origin face")?;
                    carried.insert(out);
                }
            }
            if carried.is_empty() {
                names.carried_deleted.insert(name);
            } else {
                names.carried.insert(name, carried);
            }
        }

        if self.features.insert(producer, names).is_some() {
            return Err(CadError::topology(format!(
                "feature {producer} was recorded twice in one rebuild"
            )));
        }
        Ok(())
    }
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
