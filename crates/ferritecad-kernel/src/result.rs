// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_types::{CadError, ContentHash, Result, StableEntityId};

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

        let vertices = self.vertex_count() as u32;
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

        Ok(())
    }
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
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 9],
            faces: vec![MeshFaceRange {
                face: face(0),
                first_index: 0,
                index_count: 3,
            }],
        };
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_mesh_with_mismatched_normals_is_refused() {
        let mesh = Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: face(0),
                first_index: 0,
                index_count: 3,
            }],
        };
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_mesh_whose_face_ranges_leave_a_gap_is_refused() {
        let mesh = Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: face(0),
                first_index: 1,
                index_count: 2,
            }],
        };
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_mesh_face_range_cannot_split_a_triangle() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = Mesh {
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
        };
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_mesh_range_must_name_a_face() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        let mesh = Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Edge, 0),
                first_index: 0,
                index_count: 3,
            }],
        };
        assert!(mesh.validate().is_err());
    }

    #[test]
    fn a_mesh_with_a_non_finite_coordinate_is_refused() {
        let mesh = Mesh {
            positions: vec![0.0, 0.0, f32::NAN, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: face(0),
                first_index: 0,
                index_count: 3,
            }],
        };
        assert!(mesh.validate().is_err());
    }
}
