// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::CapSide;
use ferritecad_kernel::{
    CutResult, ExtrudeResult, HistoryInput, Profile, ShapeHandle, SubShapeHandle, SubShapeKind,
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
    /// The caps an earlier feature made, as this feature leaves them.
    ///
    /// Kept apart from `start_cap`/`end_cap` rather than merged into them.
    /// After a pocket both exist and they are different faces: this one is the
    /// plate's top, and that one is the floor the tool left. A single map would
    /// make a reference to either resolve to whichever was written last.
    carried_start_cap: BTreeSet<SubShapeHandle>,
    carried_end_cap: BTreeSet<SubShapeHandle>,
    /// The faces an earlier feature raised from each of its profile segments,
    /// as this feature leaves them. Apart from `sides` for the same reason.
    carried_sides: BTreeMap<StableEntityId, BTreeSet<SubShapeHandle>>,
    /// Every carried name the earlier feature had that this one does **not**
    /// leave behind, so a reference to it is refused rather than answered from
    /// an empty list that could equally mean "never named".
    carried_deleted: BTreeSet<CarriedName>,
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

    /// The faces an earlier feature's cap became, as this feature leaves it.
    ///
    /// `None` for a side this build does not understand, exactly as
    /// [`Self::cap`] is. An empty iterator with the name recorded as deleted is
    /// a different fact from an empty one with nothing recorded, and
    /// [`Self::carried_is_deleted`] is how a resolver tells them apart.
    pub fn carried_cap(
        &self,
        side: CapSide,
    ) -> Option<impl ExactSizeIterator<Item = SubShapeHandle> + '_> {
        let set = match side {
            CapSide::Start => &self.carried_start_cap,
            CapSide::End => &self.carried_end_cap,
            _ => return None,
        };
        Some(set.iter().copied())
    }

    /// The faces an earlier feature raised from one segment, as this feature
    /// leaves them.
    pub fn carried_side(
        &self,
        segment: StableEntityId,
    ) -> impl ExactSizeIterator<Item = SubShapeHandle> + '_ {
        self.carried_sides
            .get(&segment)
            .map(|set| set.iter())
            .unwrap_or_default()
            .copied()
    }

    /// Whether this feature removed a name the earlier feature had.
    ///
    /// The one answer that lets a resolver say "the face you mean is gone"
    /// instead of "this feature named nothing like that". They are different
    /// facts and a user acts differently on each.
    pub fn carried_is_deleted(&self, name: CarriedName) -> bool {
        self.carried_deleted.contains(&name)
    }

    /// How many faces this feature named, of every kind it names.
    ///
    /// One number, computed here, because "how much did this feature produce"
    /// is a question about the whole of what it named. A caller that added up
    /// the caps and the sides alone would report a boolean as having produced
    /// one face and lost the part it cut.
    pub fn named_face_count(&self) -> usize {
        let caps = self.start_cap.len() + self.end_cap.len();
        let sides: usize = self.sides.values().map(BTreeSet::len).sum();
        let carried_caps = self.carried_start_cap.len() + self.carried_end_cap.len();
        let carried_sides: usize = self.carried_sides.values().map(BTreeSet::len).sum();
        caps + sides + carried_caps + carried_sides
    }

    /// Every carried name this feature has an answer about, deleted or not.
    pub fn carried_names(&self) -> impl ExactSizeIterator<Item = CarriedName> {
        let mut names: BTreeSet<CarriedName> = self.carried_deleted.clone();
        if !self.carried_start_cap.is_empty() {
            names.insert(CarriedName::Cap(CapSide::Start));
        }
        if !self.carried_end_cap.is_empty() {
            names.insert(CarriedName::Cap(CapSide::End));
        }
        names.extend(self.carried_sides.keys().copied().map(CarriedName::Side));
        names.into_iter()
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
    /// What an earlier feature's caps became, as this one leaves them.
    pub carried_start_cap: Vec<SubShapeHandle>,
    pub carried_end_cap: Vec<SubShapeHandle>,
    /// The same for the faces it raised from each profile segment.
    pub carried_sides: BTreeMap<StableEntityId, Vec<SubShapeHandle>>,
    /// The carried names this feature removed.
    pub carried_deleted: BTreeSet<CarriedName>,
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
        for (faces, into) in [
            (&restored.carried_start_cap, &mut names.carried_start_cap),
            (&restored.carried_end_cap, &mut names.carried_end_cap),
        ] {
            for face in faces {
                check(*face, shape, producer, "a restored carried cap")?;
                into.insert(*face);
            }
        }
        for (segment, faces) in &restored.carried_sides {
            for face in faces {
                check(*face, shape, producer, "a restored carried side")?;
                names
                    .carried_sides
                    .entry(*segment)
                    .or_default()
                    .insert(*face);
            }
        }
        for name in &restored.carried_deleted {
            // A name cannot be both removed and restored: one of the two
            // answers would then depend on which map a resolver looked in.
            let present = match name {
                CarriedName::Cap(CapSide::Start) => !names.carried_start_cap.is_empty(),
                CarriedName::Cap(CapSide::End) => !names.carried_end_cap.is_empty(),
                CarriedName::Cap(_) => false,
                CarriedName::Side(segment) => names.carried_sides.contains_key(segment),
            };
            if present {
                return Err(CadError::topology(format!(
                    "the archive of feature {producer} restores {name:?} and also calls it removed"
                )));
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

        // Everything the earlier feature was called, carried forward or
        // recorded as gone.
        for side in [CapSide::Start, CapSide::End] {
            let Some(faces) = previous_names.cap(side) else {
                continue;
            };
            let mut survived = false;
            for face in faces {
                for out in outputs(face) {
                    check(out, result.shape, producer, "a carried cap")?;
                    survived = true;
                    match side {
                        CapSide::Start => names.carried_start_cap.insert(out),
                        CapSide::End => names.carried_end_cap.insert(out),
                        _ => unreachable!("the two sides are matched above"),
                    };
                }
            }
            if !survived {
                names.carried_deleted.insert(CarriedName::Cap(side));
            }
        }
        for segment in previous_names.named_segments() {
            let mut survived = false;
            for face in previous_names.side(segment) {
                for out in outputs(face) {
                    check(out, result.shape, producer, "a carried side")?;
                    survived = true;
                    names.carried_sides.entry(segment).or_default().insert(out);
                }
            }
            if !survived {
                names.carried_deleted.insert(CarriedName::Side(segment));
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
