// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_types::{CadError, ContentHash, ProfileJoint, Result, StableEntityId};

use crate::handle::{ShapeHandle, SubShapeHandle, SubShapeKind};
use crate::identity::KernelIdentity;

/// What an operation consumed, as history refers back to it.
///
/// An extrusion's inputs are labelled profile segments; a transform's are the
/// sub-shapes of the shape it was given. One enum covers both so history has a
/// single shape regardless of which operation produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum HistoryInput {
    /// A profile segment, by the label the caller attached to it.
    Segment(StableEntityId),
    /// A sub-shape of an input shape, by its session-local handle.
    SubShape(SubShapeHandle),
}

/// What an operation did to each of its inputs.
///
/// This is the raw material the topology layer turns into durable names. It
/// says only what the kernel knows — that these outputs came from that input —
/// and nothing about what any of it means. Deciding that a face is "the cap of
/// this extrusion" happens a layer up, where the vocabulary for saying so
/// exists.
///
/// Ordered containers throughout, so two runs of the same rebuild produce
/// byte-identical history. Iteration order that depends on a hash seed would
/// make a naming bug reproducible only sometimes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    generated: BTreeMap<HistoryInput, BTreeSet<SubShapeHandle>>,
    modified: BTreeMap<HistoryInput, BTreeSet<SubShapeHandle>>,
    deleted: BTreeSet<HistoryInput>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `input` produced `output`, which did not exist before.
    pub fn record_generated(&mut self, input: HistoryInput, output: SubShapeHandle) {
        self.generated.entry(input).or_default().insert(output);
    }

    /// Records that `input` survives as `output` in some altered form.
    pub fn record_modified(&mut self, input: HistoryInput, output: SubShapeHandle) {
        self.modified.entry(input).or_default().insert(output);
    }

    /// Records that `input` has no counterpart in the result.
    ///
    /// This is the case a naming scheme must handle honestly: the reference to
    /// a deleted entity does not resolve, and the rebuild must say so rather
    /// than pick something nearby.
    pub fn record_deleted(&mut self, input: HistoryInput) {
        self.deleted.insert(input);
    }

    /// Taken by value rather than by reference: `HistoryInput` is `Copy`,
    /// and a reference would tie the returned iterator to the lifetime of a
    /// temporary at every call site that builds its key inline.
    pub fn generated(&self, input: HistoryInput) -> impl Iterator<Item = SubShapeHandle> + '_ {
        self.generated.get(&input).into_iter().flatten().copied()
    }

    pub fn modified(&self, input: HistoryInput) -> impl Iterator<Item = SubShapeHandle> + '_ {
        self.modified.get(&input).into_iter().flatten().copied()
    }

    pub fn is_deleted(&self, input: HistoryInput) -> bool {
        self.deleted.contains(&input)
    }

    /// Every input mentioned by the history, once and in a stable order.
    pub fn inputs(&self) -> impl Iterator<Item = HistoryInput> {
        self.generated
            .keys()
            .chain(self.modified.keys())
            .chain(self.deleted.iter())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
    }

    pub fn is_empty(&self) -> bool {
        self.generated.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
}

/// The result of an operation that produces one shape.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationResult {
    pub shape: ShapeHandle,
    pub history: History,
}

/// The result of an extrusion.
///
/// The caps are reported separately because they correspond to no input: they
/// are new geometry the sweep creates, and a kernel that only reported
/// "generated from" could not name them at all.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudeResult {
    pub shape: ShapeHandle,
    pub history: History,
    /// The face closing the start of the sweep.
    pub start_cap: Vec<SubShapeHandle>,
    /// The face closing the end of the sweep.
    pub end_cap: Vec<SubShapeHandle>,
    /// Where the start cap meets the face swept from each profile segment.
    ///
    /// One edge per segment, and the map says so: a segment is a key, so a
    /// kernel cannot report two edges for one segment on one side without
    /// overwriting itself, and the claimed cardinality is the representation
    /// rather than a rule applied afterwards. A segment that produced no such
    /// edge is simply absent.
    ///
    /// Kept apart from `history` deliberately. History says which *faces* a
    /// segment generated; folding an edge in beside them would make
    /// `generated()` answer with a mixture nobody asked for.
    pub start_cap_edges: BTreeMap<StableEntityId, SubShapeHandle>,
    /// The same, where the end cap meets each swept face.
    pub end_cap_edges: BTreeMap<StableEntityId, SubShapeHandle>,
    /// The edge running along the sweep at each corner of the profile.
    ///
    /// Keyed by the joint, which is the unordered pair of the two segments
    /// that meet there. Neither of the two owns the edge — it is where their
    /// swept faces meet — so filing it under one of them would be a choice the
    /// geometry does not make, and would break as soon as the profile were
    /// walked from the other direction.
    ///
    /// One edge per joint, and the map says so, for the same reason the cap
    /// edges do. A corner that produced none is absent.
    pub sweep_edges: BTreeMap<ProfileJoint, SubShapeHandle>,
    /// The vertex where one profile joint reaches the start cap.
    ///
    /// Keyed by the joint, on the same terms as the sweep edges: a corner is
    /// the pair of segments meeting there and nothing positional. Only a joint
    /// that occurs once in the profile appears here. A loop of two segments has
    /// one unordered pair at two corners, and the kernel distinguishes those
    /// two vertices perfectly well; the *name* cannot, so neither is filed.
    ///
    /// Measured before it was relied on. See `tools/occt-smoke` step 9, which
    /// the pin workflow runs on all three platforms: for each positional corner
    /// and each side the prism names exactly one vertex, in the solid by
    /// `IsSame`, on the cap it is claimed for, ending that corner's swept edge,
    /// and reached by the tessellation association.
    pub start_cap_vertices: BTreeMap<ProfileJoint, SubShapeHandle>,
    /// The same, where the joint reaches the end cap.
    pub end_cap_vertices: BTreeMap<ProfileJoint, SubShapeHandle>,
}

impl ExtrudeResult {
    /// Checks the naming a consumer is entitled to assume.
    ///
    /// A name that points at another shape, at a face rather than an edge, or
    /// at geometry two segments both claim is worse than no name at all: it
    /// resolves, and it resolves to the wrong thing. Refusing at the boundary
    /// that produced it keeps the failure next to its cause.
    pub fn validate(&self) -> Result<()> {
        let mut claimed: BTreeMap<SubShapeHandle, (&str, StableEntityId)> = BTreeMap::new();
        for (side, edges) in [
            ("start", &self.start_cap_edges),
            ("end", &self.end_cap_edges),
        ] {
            for (segment, edge) in edges {
                if edge.shape() != self.shape {
                    return Err(CadError::kernel(format!(
                        "the {side} cap edge named for segment {segment} is {edge}, which \
                         belongs to another shape than {}",
                        self.shape
                    )));
                }
                if edge.kind() != SubShapeKind::Edge {
                    return Err(CadError::kernel(format!(
                        "the {side} cap edge named for segment {segment} is {edge}, which is \
                         a {}",
                        edge.kind()
                    )));
                }
                // One physical edge may carry exactly one cap-edge meaning.
                // A start and an end are as distinct as two segments: letting
                // either pair share a handle would make two durable references
                // resolve to the same geometry.
                if let Some((other_side, other_segment)) = claimed.insert(*edge, (side, *segment))
                    && (other_side != side || other_segment != *segment)
                {
                    return Err(CadError::kernel(format!(
                        "the {other_side} cap edge of segment {other_segment} and the {side} cap \
                         edge of segment {segment} both name {edge}"
                    )));
                }
            }
        }

        let mut swept: BTreeMap<SubShapeHandle, ProfileJoint> = BTreeMap::new();
        for (joint, edge) in &self.sweep_edges {
            if edge.shape() != self.shape {
                return Err(CadError::kernel(format!(
                    "the sweep edge named for {joint} is {edge}, which belongs to another shape \
                     than {}",
                    self.shape
                )));
            }
            if edge.kind() != SubShapeKind::Edge {
                return Err(CadError::kernel(format!(
                    "the sweep edge named for {joint} is {edge}, which is a {}",
                    edge.kind()
                )));
            }
            // A cap edge and a sweep edge are different edges of the solid.
            // One handle answering to both would be two durable references
            // resolving to the same geometry, which is the failure these
            // names exist to prevent.
            if let Some((side, segment)) = claimed.get(edge) {
                return Err(CadError::kernel(format!(
                    "{edge} is named both as the {side} cap edge of segment {segment} and as the \
                     sweep edge of {joint}"
                )));
            }
            if let Some(other) = swept.insert(*edge, *joint)
                && other != *joint
            {
                return Err(CadError::kernel(format!(
                    "the sweep edges of {other} and of {joint} both name {edge}"
                )));
            }
        }

        // The cap vertices, on the same terms. One physical vertex carries one
        // meaning: two joints naming it, or one joint naming it on both caps,
        // would be two durable references resolving to the same point.
        let mut cornered: BTreeMap<SubShapeHandle, (&str, ProfileJoint)> = BTreeMap::new();
        for (side, vertices) in [
            ("start", &self.start_cap_vertices),
            ("end", &self.end_cap_vertices),
        ] {
            for (joint, vertex) in vertices {
                if vertex.shape() != self.shape {
                    return Err(CadError::kernel(format!(
                        "the {side} cap vertex named for {joint} is {vertex}, which belongs to \
                         another shape than {}",
                        self.shape
                    )));
                }
                if vertex.kind() != SubShapeKind::Vertex {
                    return Err(CadError::kernel(format!(
                        "the {side} cap vertex named for {joint} is {vertex}, which is a {}",
                        vertex.kind()
                    )));
                }
                if let Some((other_side, other_joint)) = cornered.insert(*vertex, (side, *joint))
                    && (other_side != side || other_joint != *joint)
                {
                    return Err(CadError::kernel(format!(
                        "the {other_side} cap vertex of {other_joint} and the {side} cap vertex \
                         of {joint} both name {vertex}"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// A kernel's own serialisation of a shape, for the cache and nothing else.
///
/// Opaque bytes plus the identity that produced them. It is cache, not source
/// of truth: a document that lost every blob rebuilds to the same model, only
/// more slowly. The identity travels with the bytes so a blob written by one
/// kernel build cannot be handed to another — an error nobody would otherwise
/// notice until the geometry came out subtly different.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrepBlob {
    kernel: KernelIdentity,
    bytes: Vec<u8>,
}

impl BrepBlob {
    pub fn new(kernel: KernelIdentity, bytes: Vec<u8>) -> Self {
        Self { kernel, bytes }
    }

    pub fn kernel(&self) -> &KernelIdentity {
        &self.kernel
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The content hash of the encoded bytes, for content-addressed storage.
    pub fn content_hash(&self) -> ContentHash {
        ContentHash::of_bytes(&self.bytes)
    }

    /// Refuses a blob that some other kernel build wrote.
    pub fn require_kernel(&self, expected: &KernelIdentity) -> Result<()> {
        if &self.kernel != expected {
            return Err(CadError::kernel(format!(
                "this cached shape was written by {}, but {expected} is loaded; \
                 discard the cache rather than decoding it",
                self.kernel
            )));
        }
        Ok(())
    }
}

/// A position inside one archive, and nothing more.
///
/// An archive holds a shape together with the sub-shapes its author asked to
/// keep; a slot says which of those a caller wants back. It is meaningful only
/// against the blob it was produced with, which is why the two must be stored
/// together and why this type says nothing about geometry.
///
/// It is deliberately *not* a name. A name says what a face is and survives a
/// rebuild; a slot says where a face sits in one particular archive and
/// survives nothing else.
///
/// A bare integer on purpose. When a later slice stores slots beside their
/// blobs, it can write this out with whatever it already uses; nothing here
/// needs a serialisation dependency to make that possible. Contrast
/// [`SubShapeHandle`], which may never be written down at all and implements
/// no serialisation for that reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchiveSlot(u32);

impl ArchiveSlot {
    /// Slot zero is the archived shape itself, never a sub-shape.
    pub const ROOT: Self = Self(0);

    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    pub const fn index(self) -> u32 {
        self.0
    }

    pub const fn is_root(self) -> bool {
        self.0 == 0
    }
}

/// Which triangles belong to which face.
///
/// Without this the viewport can draw a solid but cannot tell what was clicked,
/// and face selection is the point at which a viewer becomes a modeller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshFaceRange {
    pub face: SubShapeHandle,
    pub first_index: u32,
    pub index_count: u32,
}

/// One topological edge of a shape, and the rendered segments that draw it.
///
/// The edge is a sub-shape of the session that tessellated it, exactly as a
/// face range's face is. What makes this an edge rather than a boundary is
/// that the kernel says so: several segments of one curved edge carry one
/// handle, and the two face-side representations of an edge shared by two
/// faces carry the same handle, because they are the same edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshEdgeRange {
    pub edge: SubShapeHandle,
    /// First segment, counted in segments rather than indices.
    pub first_segment: u32,
    pub segment_count: u32,
}

/// One topological vertex of a shape, and the packed positions that draw it.
///
/// A B-Rep vertex is one point of the model and many points of the mesh: every
/// face meeting there carries its own copy of it. Measured on OCCT 7.9.3 over a
/// plate, a cylinder, a half cylinder, a sphere, a torus, a filleted and a
/// shelled plate and five STEP files: between two and five copies each, never
/// zero. So this is one-to-many by construction rather than by convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshVertexRange {
    pub vertex: SubShapeHandle,
    /// First occurrence, counted in occurrences rather than in floats.
    pub first_occurrence: u32,
    pub occurrence_count: u32,
}

/// Which packed positions belong to which topological vertex.
///
/// One value rather than two parallel fields on [`Mesh`], for the reason
/// [`MeshEdges`] is one: a mesh either carries this whole association or does
/// not. The fields stay public because this is a kernel-result DTO, so
/// malformed combinations can still be built by an adapter and are deliberately
/// refused by [`Mesh::validate`].
///
/// Deliberately *not* a partition of the packed positions. Most nodes of a
/// tessellation are interior and are no B-Rep vertex at all; requiring them to
/// be covered would be requiring the kernel to invent identities.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MeshVertices {
    /// Indices into the packed positions, referenced by the ranges.
    pub occurrences: Vec<u32>,
    /// Which topological vertex owns which occurrences: ordered, contiguous,
    /// and covering `occurrences` exactly.
    pub ranges: Vec<MeshVertexRange>,
}

/// Which rendered segments belong to which topological edge.
///
/// One value rather than two parallel fields on [`Mesh`], so availability and
/// the data it qualifies cannot drift apart: a mesh either carries this whole
/// association or does not; see [`Mesh::edges`]. The fields stay public because
/// this is a kernel-result DTO, so malformed combinations can still be built by
/// an adapter and are deliberately refused by [`Mesh::validate`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MeshEdges {
    /// Pairs of vertex indices, two per segment, into the same vertices the
    /// triangles use.
    pub segments: Vec<u32>,
    /// Which topological edge owns which segments: ordered, contiguous, and
    /// covering `segments` exactly.
    pub ranges: Vec<MeshEdgeRange>,
}

/// Triangles ready for upload, in millimetres.
///
/// `f32` on purpose: this is the form the GPU consumes, and carrying `f64` to
/// the vertex buffer only to narrow it there would double the transfer for no
/// visible gain. Everything the model *means* stays `f64`; this is a picture of
/// it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// Vertex positions, three floats each.
    pub positions: Vec<f32>,
    /// Vertex normals, three floats each, parallel to `positions`.
    pub normals: Vec<f32>,
    /// Triangle indices into the vertex arrays, three per triangle.
    pub indices: Vec<u32>,
    /// Index ranges per face, ordered and non-overlapping.
    pub faces: Vec<MeshFaceRange>,
    /// The topological edges of this shape, when the producer can name them.
    ///
    /// `None` and `Some` of an empty [`MeshEdges`] are different answers and
    /// are kept apart. `None` says the producer did not associate segments
    /// with edges at all; an empty `MeshEdges` says it looked and this shape
    /// has no topological edge to draw. Collapsing the two would give a mesh
    /// whose association is merely unknown the same standing as one proven to
    /// have no edges, which is the invention this contract exists to refuse.
    pub edges: Option<MeshEdges>,
    /// The topological vertices of this shape, when the producer can name them.
    ///
    /// `None` and `Some` of an empty [`MeshVertices`] are different answers and
    /// are kept apart, exactly as they are for the edges: `None` says the
    /// producer did not associate positions with vertices at all, and an empty
    /// association says it looked and this shape has no topological vertex.
    ///
    /// Named at length so it cannot be read as [`Mesh::vertex_count`], which
    /// counts packed positions and is a different thing entirely.
    pub topological_vertices: Option<MeshVertices>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }

    /// Checks the internal consistency a consumer is entitled to assume.
    ///
    /// An adapter that gets this wrong produces a mesh that renders as garbage
    /// or crashes a driver, and the cause is far from the symptom. Better to
    /// refuse the mesh at the boundary that produced it.
    pub fn validate(&self) -> Result<()> {
        if !self.positions.len().is_multiple_of(3) {
            return Err(CadError::kernel(format!(
                "mesh has {} position floats, which is not a whole number of vertices",
                self.positions.len()
            )));
        }
        if self.normals.len() != self.positions.len() {
            return Err(CadError::kernel(format!(
                "mesh has {} position floats but {} normal floats",
                self.positions.len(),
                self.normals.len()
            )));
        }
        if !self.indices.len().is_multiple_of(3) {
            return Err(CadError::kernel(format!(
                "mesh has {} indices, which is not a whole number of triangles",
                self.indices.len()
            )));
        }

        let vertices = u32::try_from(self.vertex_count())
            .map_err(|_| CadError::kernel("mesh has more vertices than uint32 can index"))?;
        if let Some(out_of_range) = self.indices.iter().find(|i| **i >= vertices) {
            return Err(CadError::kernel(format!(
                "mesh index {out_of_range} addresses vertex {out_of_range} of {vertices}"
            )));
        }
        if let Some(bad) = self
            .positions
            .iter()
            .chain(self.normals.iter())
            .find(|v| !v.is_finite())
        {
            return Err(CadError::kernel(format!(
                "mesh contains a non-finite coordinate: {bad}"
            )));
        }

        let mut covered = 0u32;
        let mut shape = None;
        let mut seen_faces = BTreeSet::new();
        for range in &self.faces {
            if range.face.kind() != SubShapeKind::Face {
                return Err(CadError::kernel(format!(
                    "mesh range names {}, which is not a face",
                    range.face
                )));
            }
            if range.index_count == 0 || !range.index_count.is_multiple_of(3) {
                return Err(CadError::kernel(format!(
                    "mesh face {} owns {} indices, which is not a non-empty whole number of triangles",
                    range.face, range.index_count
                )));
            }
            if !range.first_index.is_multiple_of(3) {
                return Err(CadError::kernel(format!(
                    "mesh face {} starts at index {}, in the middle of a triangle",
                    range.face, range.first_index
                )));
            }
            if !seen_faces.insert(range.face) {
                return Err(CadError::kernel(format!(
                    "mesh contains more than one range for face {}",
                    range.face
                )));
            }
            if let Some(expected) = shape {
                if range.face.shape() != expected {
                    return Err(CadError::kernel(format!(
                        "mesh mixes faces from {} and {}",
                        expected,
                        range.face.shape()
                    )));
                }
            } else {
                shape = Some(range.face.shape());
            }
            if range.first_index != covered {
                return Err(CadError::kernel(format!(
                    "mesh face ranges are not contiguous: expected to continue at {covered}, \
                     found a range starting at {}",
                    range.first_index
                )));
            }
            covered = covered
                .checked_add(range.index_count)
                .ok_or_else(|| CadError::kernel("mesh face ranges overflow the index space"))?;
        }
        if covered as usize != self.indices.len() {
            return Err(CadError::kernel(format!(
                "mesh face ranges cover {covered} indices but the mesh has {}",
                self.indices.len()
            )));
        }

        if let Some(edges) = &self.edges {
            validate_edges(edges, vertices, shape)?;
        }
        if let Some(corners) = &self.topological_vertices {
            validate_topological_vertices(corners, vertices, shape)?;
        }

        Ok(())
    }
}

/// Checks a vertex association against the mesh that carries it.
///
/// The rules are the edge association's, plus one the geometry forces: two
/// different topological vertices may not claim the same packed position. Each
/// face carries its own copy of every corner it touches, so a position belongs
/// to exactly one B-Rep vertex, and a mesh saying otherwise cannot answer which
/// vertex a point is.
///
/// Not required: that the occurrences cover the packed positions. Most nodes of
/// a tessellation lie inside a face and are no B-Rep vertex at all.
fn validate_topological_vertices(
    corners: &MeshVertices,
    vertices: u32,
    face_shape: Option<ShapeHandle>,
) -> Result<()> {
    if let Some(out_of_range) = corners.occurrences.iter().find(|i| **i >= vertices) {
        return Err(CadError::kernel(format!(
            "mesh vertex association addresses position {out_of_range} of {vertices}"
        )));
    }

    let total = u32::try_from(corners.occurrences.len())
        .map_err(|_| CadError::kernel("mesh has more vertex occurrences than uint32 can count"))?;
    let mut covered = 0u32;
    let mut seen = BTreeSet::new();
    let mut claimed: BTreeMap<u32, SubShapeHandle> = BTreeMap::new();
    // The shape every vertex must belong to, carried through the whole loop so
    // a stranger cannot enter behind a correct first one.
    let mut expected = face_shape;
    for range in &corners.ranges {
        match expected {
            Some(shape) if range.vertex.shape() != shape => {
                return Err(CadError::kernel(format!(
                    "mesh mixes vertices from {} and {shape}",
                    range.vertex.shape()
                )));
            }
            Some(_) => {}
            None => expected = Some(range.vertex.shape()),
        }
        if range.vertex.kind() != SubShapeKind::Vertex {
            return Err(CadError::kernel(format!(
                "mesh vertex range names {}, which is not a vertex",
                range.vertex
            )));
        }
        if range.occurrence_count == 0 {
            return Err(CadError::kernel(format!(
                "mesh vertex {} is drawn nowhere",
                range.vertex
            )));
        }
        if !seen.insert(range.vertex) {
            return Err(CadError::kernel(format!(
                "mesh contains more than one range for vertex {}",
                range.vertex
            )));
        }
        if range.first_occurrence != covered {
            return Err(CadError::kernel(format!(
                "mesh vertex ranges are not contiguous: expected to continue at {covered}, \
                 found a range starting at {}",
                range.first_occurrence
            )));
        }
        covered = covered
            .checked_add(range.occurrence_count)
            .ok_or_else(|| CadError::kernel("mesh vertex ranges overflow the occurrence space"))?;
        if covered > total {
            return Err(CadError::kernel(format!(
                "mesh vertex {} claims occurrences beyond the {total} the mesh has",
                range.vertex
            )));
        }

        let first = range.first_occurrence as usize;
        let last = first + range.occurrence_count as usize;
        let mut own = BTreeSet::new();
        for index in &corners.occurrences[first..last] {
            if !own.insert(*index) {
                return Err(CadError::kernel(format!(
                    "mesh vertex {} is drawn at position {index} twice",
                    range.vertex
                )));
            }
            if let Some(other) = claimed.insert(*index, range.vertex) {
                return Err(CadError::kernel(format!(
                    "vertices {other} and {} both claim position {index}",
                    range.vertex
                )));
            }
        }
    }
    if covered != total {
        return Err(CadError::kernel(format!(
            "mesh vertex ranges cover {covered} of {total} occurrences"
        )));
    }

    Ok(())
}

/// Checks an edge association against the mesh that carries it.
///
/// `vertices` is the mesh's vertex count, and `face_shape` the shape its faces
/// name when it has faces. A mesh whose ranges do not cover its segments
/// exactly, whose segments address vertices that are not there, or which names
/// one edge twice cannot say which edge a rendered line belongs to; refusing
/// beats choosing.
fn validate_edges(edges: &MeshEdges, vertices: u32, face_shape: Option<ShapeHandle>) -> Result<()> {
    if !edges.segments.len().is_multiple_of(2) {
        return Err(CadError::kernel(format!(
            "mesh has {} edge segment indices, which is not a whole number of segments",
            edges.segments.len()
        )));
    }
    if let Some(out_of_range) = edges.segments.iter().find(|i| **i >= vertices) {
        return Err(CadError::kernel(format!(
            "mesh edge segment addresses vertex {out_of_range} of {vertices}"
        )));
    }

    let segments = u32::try_from(edges.segments.len() / 2)
        .map_err(|_| CadError::kernel("mesh has more edge segments than uint32 can count"))?;
    let mut covered = 0u32;
    let mut seen = BTreeSet::new();
    // The shape every edge must belong to: the faces', when there are faces,
    // and otherwise whichever shape the first edge names. Carried through the
    // whole loop rather than checked once, so a stranger cannot enter behind a
    // correct first edge.
    let mut expected = face_shape;
    for range in &edges.ranges {
        match expected {
            Some(shape) if range.edge.shape() != shape => {
                return Err(CadError::kernel(format!(
                    "mesh mixes edges from {} and {shape}",
                    range.edge.shape()
                )));
            }
            Some(_) => {}
            None => expected = Some(range.edge.shape()),
        }
        if range.edge.kind() != SubShapeKind::Edge {
            return Err(CadError::kernel(format!(
                "mesh edge range names {}, which is not an edge",
                range.edge
            )));
        }
        if range.segment_count == 0 {
            return Err(CadError::kernel(format!(
                "mesh edge {} owns no segments",
                range.edge
            )));
        }
        if !seen.insert(range.edge) {
            return Err(CadError::kernel(format!(
                "mesh contains more than one range for edge {}",
                range.edge
            )));
        }
        if range.first_segment != covered {
            return Err(CadError::kernel(format!(
                "mesh edge ranges are not contiguous: expected to continue at {covered}, \
                 found a range starting at {}",
                range.first_segment
            )));
        }
        covered = covered
            .checked_add(range.segment_count)
            .ok_or_else(|| CadError::kernel("mesh edge ranges overflow the segment space"))?;
        if covered > segments {
            return Err(CadError::kernel(format!(
                "mesh edge {} claims segments beyond the {segments} the mesh has",
                range.edge
            )));
        }
    }
    if covered != segments {
        return Err(CadError::kernel(format!(
            "mesh edge ranges cover {covered} of {segments} segments"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::{SessionId, SubShapeKind};

    fn face(index: u64) -> SubShapeHandle {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        SubShapeHandle::new(shape, SubShapeKind::Face, index)
    }

    #[test]
    fn history_reports_what_it_was_told() {
        let segment = HistoryInput::Segment(StableEntityId::new());
        let side = face(1);

        let mut history = History::new();
        history.record_generated(segment, side);

        assert_eq!(history.generated(segment).collect::<Vec<_>>(), vec![side]);
        assert!(history.modified(segment).next().is_none());
        assert!(!history.is_deleted(segment));
        assert!(!history.is_empty());
    }

    #[test]
    fn a_deleted_input_is_reported_as_deleted_not_as_missing() {
        // The distinction matters: "no entry" means the kernel said nothing,
        // "deleted" means it said the entity is gone.
        let segment = HistoryInput::Segment(StableEntityId::new());
        let silent = HistoryInput::Segment(StableEntityId::new());

        let mut history = History::new();
        history.record_deleted(segment);

        assert!(history.is_deleted(segment));
        assert!(!history.is_deleted(silent));
    }

    #[test]
    fn history_is_ordered_regardless_of_insertion_order() {
        let a = HistoryInput::Segment(StableEntityId::new());
        let b = HistoryInput::Segment(StableEntityId::new());
        let one = face(1);
        let other = face(2);

        let mut forwards = History::new();
        forwards.record_generated(a, one);
        forwards.record_generated(b, other);

        let mut backwards = History::new();
        backwards.record_generated(b, other);
        backwards.record_generated(a, one);

        assert_eq!(forwards, backwards);
        assert_eq!(
            forwards.inputs().collect::<Vec<_>>(),
            backwards.inputs().collect::<Vec<_>>()
        );
    }

    #[test]
    fn history_lists_an_input_only_once_across_all_outcomes() {
        let input = HistoryInput::Segment(StableEntityId::new());
        let mut history = History::new();
        history.record_generated(input, face(1));
        history.record_modified(input, face(2));
        history.record_deleted(input);

        assert_eq!(history.inputs().collect::<Vec<_>>(), vec![input]);
    }

    #[test]
    fn recording_the_same_output_twice_does_not_duplicate_it() {
        let segment = HistoryInput::Segment(StableEntityId::new());
        let side = face(1);

        let mut history = History::new();
        history.record_generated(segment, side);
        history.record_generated(segment, side);

        assert_eq!(history.generated(segment).count(), 1);
    }

    #[test]
    fn a_blob_from_another_kernel_build_is_refused() {
        let written_by = KernelIdentity::new("occt", "8.0.0", "").expect("valid");
        let loaded = KernelIdentity::new("occt", "8.0.1", "").expect("valid");
        let blob = BrepBlob::new(written_by, vec![1, 2, 3]);

        let err = blob
            .require_kernel(&loaded)
            .expect_err("a blob from another build must not be decoded");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Kernel);
        assert!(blob.require_kernel(blob.kernel()).is_ok());
    }

    #[test]
    fn a_blob_is_addressed_by_its_content() {
        let kernel = KernelIdentity::new("mock", "1", "").expect("valid");
        let one = BrepBlob::new(kernel.clone(), vec![1, 2, 3]);
        let same = BrepBlob::new(kernel.clone(), vec![1, 2, 3]);
        let other = BrepBlob::new(kernel, vec![1, 2, 4]);

        assert_eq!(one.content_hash(), same.content_hash());
        assert_ne!(one.content_hash(), other.content_hash());
    }

    #[test]
    fn an_empty_mesh_is_valid() {
        assert!(Mesh::default().validate().is_ok());
        assert_eq!(Mesh::default().triangle_count(), 0);
    }

    #[test]
    fn a_mesh_with_a_dangling_index_is_refused() {
        let mesh = Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 9],
            faces: vec![MeshFaceRange {
                face: face(0),
                first_index: 0,
                index_count: 3,
            }],
            edges: None,
        };
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_mesh_with_mismatched_normals_is_refused() {
        let mesh = Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: face(0),
                first_index: 0,
                index_count: 3,
            }],
            edges: None,
        };
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_mesh_whose_face_ranges_leave_a_gap_is_refused() {
        let mesh = Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: face(0),
                first_index: 1,
                index_count: 2,
            }],
            edges: None,
        };
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_mesh_face_range_cannot_split_a_triangle() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![
                MeshFaceRange {
                    face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
                    first_index: 0,
                    index_count: 1,
                },
                MeshFaceRange {
                    face: SubShapeHandle::new(shape, SubShapeKind::Face, 1),
                    first_index: 1,
                    index_count: 2,
                },
            ],
            edges: None,
        };
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_mesh_range_must_name_a_face() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Edge, 0),
                first_index: 0,
                index_count: 3,
            }],
            edges: None,
        };
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_mesh_with_a_non_finite_coordinate_is_refused() {
        let mesh = Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, f32::NAN, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: face(0),
                first_index: 0,
                index_count: 3,
            }],
            edges: None,
        };
        assert!(mesh.validate().is_err());
    }

    /// A triangle of one face, with whatever edge association is being tried.
    fn triangle_with(shape: ShapeHandle, edges: Option<MeshEdges>) -> Mesh {
        Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 3,
            }],
            edges,
        }
    }

    /// A triangle of one face, with whatever vertex association is being tried.
    fn triangle_cornered(shape: ShapeHandle, corners: Option<MeshVertices>) -> Mesh {
        let mut mesh = triangle_with(shape, None);
        mesh.topological_vertices = corners;
        mesh
    }

    /// One corner of that triangle, owning `count` occurrences from `first`.
    fn corner(shape: ShapeHandle, index: u64, first: u32, count: u32) -> MeshVertexRange {
        MeshVertexRange {
            vertex: SubShapeHandle::new(shape, SubShapeKind::Vertex, index),
            first_occurrence: first,
            occurrence_count: count,
        }
    }

    #[test]
    fn a_vertex_association_the_mesh_can_carry_is_accepted() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        // Three corners, one of them drawn twice, and one position of the
        // triangle left unnamed: an interior node is no B-Rep vertex, so full
        // coverage is deliberately not required.
        let mesh = triangle_cornered(
            shape,
            Some(MeshVertices {
                occurrences: vec![0, 1, 2],
                ranges: vec![corner(shape, 0, 0, 2), corner(shape, 1, 2, 1)],
            }),
        );
        mesh.validate()
            .expect("a partial, consistent association is fine");
    }

    #[test]
    fn a_vertex_association_of_the_wrong_kind_or_shape_is_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let stranger = ShapeHandle::new(SessionId::new(), 0);

        let wrong_kind = triangle_cornered(
            shape,
            Some(MeshVertices {
                occurrences: vec![0],
                ranges: vec![MeshVertexRange {
                    vertex: SubShapeHandle::new(shape, SubShapeKind::Edge, 0),
                    first_occurrence: 0,
                    occurrence_count: 1,
                }],
            }),
        )
        .validate()
        .expect_err("an edge is not a vertex");
        assert!(
            wrong_kind.to_string().contains("which is not a vertex"),
            "{wrong_kind}"
        );

        let foreign = triangle_cornered(
            shape,
            Some(MeshVertices {
                occurrences: vec![0],
                ranges: vec![corner(stranger, 0, 0, 1)],
            }),
        )
        .validate()
        .expect_err("a vertex of another shape is not this mesh's");
        assert!(foreign.to_string().contains("mixes vertices"), "{foreign}");
    }

    #[test]
    fn a_vertex_drawn_nowhere_or_out_of_range_is_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);

        let nowhere = triangle_cornered(
            shape,
            Some(MeshVertices {
                occurrences: Vec::new(),
                ranges: vec![corner(shape, 0, 0, 0)],
            }),
        )
        .validate()
        .expect_err("a corner drawn nowhere names nothing");
        assert!(nowhere.to_string().contains("drawn nowhere"), "{nowhere}");

        let past_the_end = triangle_cornered(
            shape,
            Some(MeshVertices {
                occurrences: vec![9],
                ranges: vec![corner(shape, 0, 0, 1)],
            }),
        )
        .validate()
        .expect_err("a corner cannot be drawn at a position that is not there");
        assert!(
            past_the_end.to_string().contains("addresses position 9"),
            "{past_the_end}"
        );
    }

    #[test]
    fn two_corners_cannot_claim_one_position_and_one_cannot_claim_it_twice() {
        let shape = ShapeHandle::new(SessionId::new(), 0);

        let shared = triangle_cornered(
            shape,
            Some(MeshVertices {
                occurrences: vec![0, 0],
                ranges: vec![corner(shape, 0, 0, 1), corner(shape, 1, 1, 1)],
            }),
        )
        .validate()
        .expect_err("one position belongs to one corner");
        assert!(
            shared.to_string().contains("both claim position"),
            "{shared}"
        );

        let twice = triangle_cornered(
            shape,
            Some(MeshVertices {
                occurrences: vec![1, 1],
                ranges: vec![corner(shape, 0, 0, 2)],
            }),
        )
        .validate()
        .expect_err("a corner drawn at one position twice says nothing more");
        assert!(twice.to_string().contains("twice"), "{twice}");

        let repeated = triangle_cornered(
            shape,
            Some(MeshVertices {
                occurrences: vec![0, 1],
                ranges: vec![corner(shape, 0, 0, 1), corner(shape, 0, 1, 1)],
            }),
        )
        .validate()
        .expect_err("one corner cannot have two ranges");
        assert!(
            repeated
                .to_string()
                .contains("more than one range for vertex"),
            "{repeated}"
        );
    }

    #[test]
    fn vertex_ranges_that_do_not_cover_the_occurrences_are_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);

        let gap = triangle_cornered(
            shape,
            Some(MeshVertices {
                occurrences: vec![0, 1, 2],
                ranges: vec![corner(shape, 0, 0, 1), corner(shape, 1, 2, 1)],
            }),
        )
        .validate()
        .expect_err("a gap leaves an occurrence belonging to no corner");
        assert!(gap.to_string().contains("not contiguous"), "{gap}");

        let short = triangle_cornered(
            shape,
            Some(MeshVertices {
                occurrences: vec![0, 1, 2],
                ranges: vec![corner(shape, 0, 0, 1)],
            }),
        )
        .validate()
        .expect_err("occurrences belonging to no corner are not a mesh");
        assert!(short.to_string().contains("cover 1 of 3"), "{short}");
    }

    /// One side of that triangle, owning `segment_count` segments from
    /// `first_segment`.
    fn side(
        shape: ShapeHandle,
        index: u64,
        first_segment: u32,
        segment_count: u32,
    ) -> MeshEdgeRange {
        MeshEdgeRange {
            edge: SubShapeHandle::new(shape, SubShapeKind::Edge, index),
            first_segment,
            segment_count,
        }
    }

    /// An extrusion result naming one cap edge for one segment.
    fn swept(shape: ShapeHandle, named: &[(StableEntityId, SubShapeHandle)]) -> ExtrudeResult {
        ExtrudeResult {
            start_cap_vertices: BTreeMap::new(),
            end_cap_vertices: BTreeMap::new(),
            shape,
            history: History::new(),
            start_cap: Vec::new(),
            end_cap: Vec::new(),
            start_cap_edges: named.iter().copied().collect(),
            end_cap_edges: BTreeMap::new(),
            sweep_edges: BTreeMap::new(),
        }
    }

    /// An extrusion result naming one edge along the sweep for one joint.
    fn along(shape: ShapeHandle, named: &[(ProfileJoint, SubShapeHandle)]) -> ExtrudeResult {
        ExtrudeResult {
            start_cap_vertices: BTreeMap::new(),
            end_cap_vertices: BTreeMap::new(),
            shape,
            history: History::new(),
            start_cap: Vec::new(),
            end_cap: Vec::new(),
            start_cap_edges: BTreeMap::new(),
            end_cap_edges: BTreeMap::new(),
            sweep_edges: named.iter().copied().collect(),
        }
    }

    fn joint() -> ProfileJoint {
        ProfileJoint::new(StableEntityId::new(), StableEntityId::new())
            .expect("two different segments")
    }

    #[test]
    fn a_sweep_edge_of_another_shape_or_of_the_wrong_kind_is_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let stranger = ShapeHandle::new(SessionId::new(), 0);
        let corner = joint();

        assert!(
            along(
                shape,
                &[(corner, SubShapeHandle::new(shape, SubShapeKind::Edge, 0))]
            )
            .validate()
            .is_ok()
        );

        let elsewhere = along(
            shape,
            &[(corner, SubShapeHandle::new(stranger, SubShapeKind::Edge, 0))],
        )
        .validate()
        .expect_err("an edge of another shape is not this feature's");
        assert!(
            elsewhere.to_string().contains("belongs to another shape"),
            "{elsewhere}"
        );

        let face = along(
            shape,
            &[(corner, SubShapeHandle::new(shape, SubShapeKind::Face, 0))],
        )
        .validate()
        .expect_err("a sweep edge is never a face");
        assert!(face.to_string().contains("which is a face"), "{face}");
    }

    #[test]
    fn two_joints_naming_one_edge_are_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let edge = SubShapeHandle::new(shape, SubShapeKind::Edge, 7);
        let refusal = along(shape, &[(joint(), edge), (joint(), edge)])
            .validate()
            .expect_err("one edge cannot be two corners");
        assert!(refusal.to_string().contains("both name"), "{refusal}");
    }

    #[test]
    fn a_cap_edge_cannot_also_be_a_sweep_edge() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let edge = SubShapeHandle::new(shape, SubShapeKind::Edge, 3);
        let mut result = swept(shape, &[(StableEntityId::new(), edge)]);
        result.sweep_edges.insert(joint(), edge);
        let refusal = result
            .validate()
            .expect_err("one edge cannot be a cap edge and a corner at once");
        assert!(
            refusal
                .to_string()
                .contains("named both as the start cap edge"),
            "{refusal}"
        );
    }

    /// An extrusion result naming cap vertices for joints.
    fn capped(
        shape: ShapeHandle,
        start: &[(ProfileJoint, SubShapeHandle)],
        end: &[(ProfileJoint, SubShapeHandle)],
    ) -> ExtrudeResult {
        ExtrudeResult {
            shape,
            history: History::new(),
            start_cap: Vec::new(),
            end_cap: Vec::new(),
            start_cap_edges: BTreeMap::new(),
            end_cap_edges: BTreeMap::new(),
            sweep_edges: BTreeMap::new(),
            start_cap_vertices: start.iter().copied().collect(),
            end_cap_vertices: end.iter().copied().collect(),
        }
    }

    #[test]
    fn a_cap_vertex_of_another_shape_or_of_the_wrong_kind_is_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let stranger = ShapeHandle::new(SessionId::new(), 0);
        let corner = joint();

        assert!(
            capped(
                shape,
                &[(corner, SubShapeHandle::new(shape, SubShapeKind::Vertex, 0))],
                &[]
            )
            .validate()
            .is_ok()
        );

        let elsewhere = capped(
            shape,
            &[(
                corner,
                SubShapeHandle::new(stranger, SubShapeKind::Vertex, 0),
            )],
            &[],
        )
        .validate()
        .expect_err("a vertex of another shape is not this feature's");
        assert!(
            elsewhere.to_string().contains("belongs to another shape"),
            "{elsewhere}"
        );

        let edge = capped(
            shape,
            &[(corner, SubShapeHandle::new(shape, SubShapeKind::Edge, 0))],
            &[],
        )
        .validate()
        .expect_err("a cap vertex is never an edge");
        assert!(edge.to_string().contains("which is a edge"), "{edge}");
    }

    #[test]
    fn one_vertex_cannot_answer_to_two_joints_or_to_both_caps() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let vertex = SubShapeHandle::new(shape, SubShapeKind::Vertex, 5);

        let two_joints = capped(shape, &[(joint(), vertex), (joint(), vertex)], &[])
            .validate()
            .expect_err("one vertex cannot be two corners");
        assert!(two_joints.to_string().contains("both name"), "{two_joints}");

        let corner = joint();
        let both_caps = capped(shape, &[(corner, vertex)], &[(corner, vertex)])
            .validate()
            .expect_err("one vertex cannot be on both caps");
        assert!(both_caps.to_string().contains("both name"), "{both_caps}");

        // And a cap vertex that is also named as a cap edge is refused, since
        // the two would be one handle under two durable meanings.
        let mut mixed = capped(shape, &[(corner, vertex)], &[]);
        mixed.start_cap_vertices.insert(joint(), vertex);
        assert!(mixed.validate().is_err());
    }

    #[test]
    fn a_cap_edge_of_another_shape_is_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let stranger = ShapeHandle::new(SessionId::new(), 0);
        let segment = StableEntityId::new();

        assert!(
            swept(
                shape,
                &[(segment, SubShapeHandle::new(shape, SubShapeKind::Edge, 0))]
            )
            .validate()
            .is_ok()
        );
        assert!(
            swept(
                shape,
                &[(
                    segment,
                    SubShapeHandle::new(stranger, SubShapeKind::Edge, 0)
                )]
            )
            .validate()
            .is_err(),
            "an edge of another shape was accepted as this one's name"
        );
        assert!(
            swept(
                shape,
                &[(segment, SubShapeHandle::new(shape, SubShapeKind::Face, 0))]
            )
            .validate()
            .is_err(),
            "a face was accepted under an edge's name"
        );
    }

    #[test]
    fn two_segments_naming_one_cap_edge_are_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let edge = SubShapeHandle::new(shape, SubShapeKind::Edge, 0);
        let one = StableEntityId::new();
        let other = StableEntityId::new();

        // One edge cannot answer to two segments: a reference to either would
        // be satisfied by geometry the other named.
        assert!(
            swept(shape, &[(one, edge), (other, edge)])
                .validate()
                .is_err(),
            "one edge was quietly given two names"
        );
    }

    #[test]
    fn one_edge_cannot_name_both_caps() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let edge = SubShapeHandle::new(shape, SubShapeKind::Edge, 0);
        let one = StableEntityId::new();
        let other = StableEntityId::new();

        // The two caps are distinct meanings too. A non-degenerate sweep
        // cannot use one physical edge for both, and accepting it would make
        // two durable references resolve to the same geometry.
        let mut result = swept(shape, &[(one, edge)]);
        result.end_cap_edges.insert(other, edge);
        assert!(
            result.validate().is_err(),
            "one edge was quietly named on both ends of the sweep"
        );
    }

    #[test]
    fn a_foreign_edge_cannot_hide_behind_a_correct_first_one() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let stranger = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1, 1, 2, 2, 0],
                // The first edge is this shape's, so a check that looks only
                // at the first one is satisfied. The third belongs to another
                // session entirely.
                ranges: vec![
                    side(shape, 0, 0, 1),
                    side(shape, 1, 1, 1),
                    side(stranger, 2, 2, 1),
                ],
            }),
        );

        let refusal = mesh
            .validate()
            .expect_err("an edge of another shape is not part of this mesh");
        assert!(
            refusal.to_string().contains("mixes edges"),
            "the refusal should name the mixing, got: {refusal}"
        );
    }

    #[test]
    fn the_three_sides_of_a_triangle_are_a_valid_association() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1, 1, 2, 2, 0],
                ranges: vec![
                    side(shape, 0, 0, 1),
                    side(shape, 1, 1, 1),
                    side(shape, 2, 2, 1),
                ],
            }),
        );
        assert!(mesh.validate().is_ok());
    }

    #[test]
    fn a_shape_proven_to_have_no_edges_is_not_a_missing_association() {
        // Both are valid meshes, and they are different meshes: one producer
        // looked and found nothing to draw, the other never said.
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let known_empty = triangle_with(shape, Some(MeshEdges::default()));
        let unavailable = triangle_with(shape, None);
        assert!(known_empty.validate().is_ok());
        assert!(unavailable.validate().is_ok());
        assert_ne!(known_empty, unavailable);
    }

    #[test]
    fn an_odd_number_of_segment_indices_is_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1, 2],
                ranges: vec![side(shape, 0, 0, 1)],
            }),
        );
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_segment_addressing_a_vertex_that_is_not_there_is_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 9],
                ranges: vec![side(shape, 0, 0, 1)],
            }),
        );
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn an_edge_that_owns_no_segments_is_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1, 1, 2],
                ranges: vec![
                    side(shape, 0, 0, 1),
                    side(shape, 1, 1, 0),
                    side(shape, 2, 1, 1),
                ],
            }),
        );
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn edge_ranges_that_leave_a_gap_are_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1, 1, 2, 2, 0],
                ranges: vec![side(shape, 0, 0, 1), side(shape, 1, 2, 1)],
            }),
        );
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn edge_ranges_that_overlap_are_refused() {
        // Three segments, and three claimed: the totals agree exactly, so
        // nothing about coverage is wrong here. What is wrong is that the
        // second edge starts inside the first, which means segment 1 is drawn
        // as part of two different edges. Only the contiguity rule can say so,
        // which is what makes this gate about that rule and not another.
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1, 1, 2, 2, 0],
                ranges: vec![side(shape, 0, 0, 2), side(shape, 1, 1, 1)],
            }),
        );
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn edge_ranges_that_leave_a_segment_unclaimed_are_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1, 1, 2, 2, 0],
                ranges: vec![side(shape, 0, 0, 1), side(shape, 1, 1, 1)],
            }),
        );
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn edge_ranges_that_claim_more_than_is_drawn_are_refused() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1],
                ranges: vec![side(shape, 0, 0, 4)],
            }),
        );
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn an_edge_range_must_name_an_edge() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1],
                ranges: vec![MeshEdgeRange {
                    edge: SubShapeHandle::new(shape, SubShapeKind::Face, 0),
                    first_segment: 0,
                    segment_count: 1,
                }],
            }),
        );
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn one_edge_cannot_own_two_ranges() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1, 1, 2],
                ranges: vec![side(shape, 0, 0, 1), side(shape, 0, 1, 1)],
            }),
        );
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn edges_and_faces_of_one_mesh_belong_to_one_shape() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let stranger = ShapeHandle::new(SessionId::new(), 0);
        // Every edge is the stranger's, so a rule comparing edges only with
        // each other would accept this. It is the faces they must match.
        let mesh = triangle_with(
            shape,
            Some(MeshEdges {
                segments: vec![0, 1, 1, 2],
                ranges: vec![side(stranger, 0, 0, 1), side(stranger, 1, 1, 1)],
            }),
        );
        assert!(mesh.validate().is_err());
    }
}
