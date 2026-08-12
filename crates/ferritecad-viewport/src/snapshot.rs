// SPDX-License-Identifier: MIT
//! A picture of a model, complete and unchanging.
//!
//! A snapshot is built once from meshes and placements and is then read-only:
//! no public field, no `&mut` method, nothing to invalidate. A renderer handed
//! one can upload it, draw it and pick against it without asking whether the
//! model moved underneath, because it cannot have. Replacing a snapshot is how
//! the picture changes, and that is one atomic swap rather than a set of edits
//! whose intermediate states are drawable.
//!
//! # A pick names a definition, not a placement
//!
//! Four bolts in one plate are one definition and four placements, and clicking
//! any of them yields the same [`PickId`]. That is not a limitation being worked
//! around; it is the whole of what this build can honestly say. A definition has
//! an identity its source file wrote down, so a reference to one survives being
//! saved and re-imported. An occurrence has only its position in a tree, and a
//! reference to *that* would look durable while resting on an index that the
//! next import is free to renumber.
//!
//! So the information needed to tell two placements apart never reaches a pick
//! result. Not filtered out at the end – never carried, so no later change can
//! start leaking it by accident.

use ferritecad_kernel::Mesh;
use ferritecad_types::{CadError, CanonicalHasher, ContentHash, Result, Transform};

/// Floats per packed vertex: three of position, three of normal.
pub const VERTEX_FLOATS: usize = 6;

/// What a pick can identify.
///
/// Transient by construction: it indexes into the snapshot that produced it and
/// means nothing against any other. Deliberately not serialisable – see the
/// module documentation for why a durable pick would have to name a definition
/// through the document rather than through a picture of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PickId {
    raw: u32,
    snapshot: ContentHash,
}

impl PickId {
    /// What the background reads as, so an empty pick is not definition zero.
    pub const NOTHING: Self = Self {
        raw: 0,
        snapshot: ContentHash::from_bytes([0; 32]),
    };

    /// The value a pick buffer stores.
    pub fn to_raw(self) -> u32 {
        self.raw
    }

    /// Reads a value back out of a pick buffer.
    ///
    /// A value naming no definition in `snapshot` reads as
    /// [`NOTHING`][Self::NOTHING]: a pick buffer is written by a GPU and read
    /// back over a bus, and a value outside this snapshot's definition range
    /// must land on the background rather than on whichever definition it
    /// happens to number. The caller must decode a readback against the exact
    /// snapshot that rendered it: an in-range integer carries no generation.
    pub fn from_raw(raw: u32, snapshot: &RenderSnapshot) -> Self {
        match (raw as usize).checked_sub(1) {
            Some(definition) if definition < snapshot.meshes.len() => Self {
                raw,
                snapshot: snapshot.identity,
            },
            _ => Self::NOTHING,
        }
    }

    fn unbound(raw: u32) -> Self {
        Self {
            raw,
            snapshot: ContentHash::from_bytes([0; 32]),
        }
    }
}

/// One definition's triangles, in the form a vertex buffer wants them.
///
/// Interleaved rather than parallel: one buffer, one stride, one upload. The
/// mesh this came from keeps its own parallel arrays, which are the right shape
/// for the kernel to produce and the wrong shape to draw from.
#[derive(Debug, Clone, PartialEq)]
pub struct PackedMesh {
    vertices: Vec<f32>,
    indices: Vec<u32>,
    min: [f32; 3],
    max: [f32; 3],
}

impl PackedMesh {
    /// Interleaved position and normal, [`VERTEX_FLOATS`] floats per vertex.
    pub fn vertices(&self) -> &[f32] {
        &self.vertices
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len() / VERTEX_FLOATS
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The corners of this mesh's own bounding box, before any placement.
    ///
    /// Both are zero for an empty mesh, which is the only answer that is not a
    /// lie about geometry that is not there.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        (self.min, self.max)
    }
}

/// One placement of one definition, ready to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawItem {
    /// Which packed mesh to draw.
    pub mesh: usize,
    /// The composed world placement, column-major, as a GPU expects it.
    pub transform: [f32; 16],
    /// Linear RGB and alpha. Linear because that is what the importer read out
    /// of the file; converting it here would guess at a transfer function.
    pub colour: [f32; 4],
    /// What clicking this draws identifies, which is its definition.
    pub pick: PickId,
}

/// Everything needed to draw one view of a model, and nothing that can change.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSnapshot {
    meshes: Vec<PackedMesh>,
    items: Vec<DrawItem>,
    min: [f32; 3],
    max: [f32; 3],
    has_geometry: bool,
    identity: ContentHash,
}

impl RenderSnapshot {
    pub fn meshes(&self) -> &[PackedMesh] {
        &self.meshes
    }

    /// The draw list, in the order the placements were added.
    ///
    /// That order is the caller's – document order, in practice – and is kept
    /// rather than sorted. Two builds of the same input produce the same list,
    /// which is what lets one frame be compared with another.
    pub fn draws(&self) -> &[DrawItem] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        !self.has_geometry
    }

    /// The world-space bounds of everything drawn, or `None` when nothing is.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        self.has_geometry.then_some((self.min, self.max))
    }

    /// The definition a pick identifies, if it identifies one.
    pub fn definition(&self, pick: PickId) -> Option<usize> {
        (pick.snapshot == self.identity)
            .then(|| (pick.raw as usize).checked_sub(1))
            .flatten()
            .filter(|index| *index < self.meshes.len())
    }
}

/// Collects meshes and placements into a snapshot.
///
/// Placements are added parent-first, each naming its parent by the value
/// [`place`][Self::place] returned for it, and world transforms are composed as
/// they arrive. A forward reference to a parent not yet added is refused rather
/// than deferred: a tree that has to be resolved in a second pass is a tree that
/// can contain a cycle.
#[derive(Debug, Default)]
pub struct SnapshotBuilder {
    meshes: Vec<PackedMesh>,
    items: Vec<DrawItem>,
    /// The composed world transform of each placement, kept as `Transform` so
    /// composition stays in `f64` until the last moment.
    world: Vec<Transform>,
}

impl SnapshotBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Packs one definition's mesh and returns its index.
    ///
    /// The mesh is validated first. A renderer cannot check an index against a
    /// vertex count on the GPU, and the failure looks like a driver fault
    /// rather than like the mesh it came from.
    pub fn add_mesh(&mut self, mesh: &Mesh) -> Result<usize> {
        mesh.validate()?;

        let vertex_count = mesh.vertex_count();
        let mut vertices = Vec::with_capacity(vertex_count * VERTEX_FLOATS);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];

        for vertex in 0..vertex_count {
            for axis in 0..3 {
                let value = mesh.positions[vertex * 3 + axis];
                if !value.is_finite() {
                    return Err(CadError::input(format!(
                        "vertex {vertex} has a position that is not finite, and a \
                         viewport cannot frame a model whose extent is unknown"
                    )));
                }
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
                vertices.push(value);
            }
            for axis in 0..3 {
                let value = mesh.normals[vertex * 3 + axis];
                if !value.is_finite() {
                    return Err(CadError::input(format!(
                        "vertex {vertex} has a normal that is not finite"
                    )));
                }
                vertices.push(value);
            }
        }

        if vertex_count == 0 {
            min = [0.0; 3];
            max = [0.0; 3];
        }

        self.meshes.push(PackedMesh {
            vertices,
            indices: mesh.indices.clone(),
            min,
            max,
        });
        Ok(self.meshes.len() - 1)
    }

    /// Places a definition, returning the index other placements name as parent.
    ///
    /// `local` is relative to `parent`, exactly as an imported scene records it.
    /// Composition happens here so a renderer never has to walk a tree, and so
    /// a placement's world transform is settled before anything can draw it.
    pub fn place(
        &mut self,
        mesh: usize,
        parent: Option<usize>,
        local: &Transform,
        colour: [f64; 3],
    ) -> Result<usize> {
        if mesh >= self.meshes.len() {
            return Err(CadError::input(format!(
                "placement names mesh {mesh}, and {} have been added",
                self.meshes.len()
            )));
        }

        let world = match parent {
            None => *local,
            Some(parent) => {
                let outer = self.world.get(parent).ok_or_else(|| {
                    CadError::input(format!(
                        "placement names parent {parent}, which has not been placed \
                         yet; parents are added before their children"
                    ))
                })?;
                local.then(outer)?
            }
        };

        let mut linear = [0.0f32; 4];
        for (slot, value) in linear.iter_mut().zip(colour) {
            if !value.is_finite() {
                return Err(CadError::input(
                    "a placement colour must be finite; a channel that is not \
                     would be uploaded as whatever the driver made of it",
                ));
            }
            let value = value as f32;
            if !value.is_finite() {
                return Err(CadError::input(
                    "a placement colour is outside the range a GPU can represent",
                ));
            }
            *slot = value;
        }
        linear[3] = 1.0;

        let transform = column_major(&world)?;
        let raw_pick = u32::try_from(mesh)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| {
                CadError::input("there are too many definitions for a u32 pick buffer")
            })?;

        if !self.meshes[mesh].indices.is_empty() {
            ensure_placeable(&transform, &self.meshes[mesh])?;
        }

        self.items.push(DrawItem {
            mesh,
            transform,
            colour: linear,
            // The pick identifies the definition and has no way to say which
            // placement of it this is. See the module documentation.
            pick: PickId::unbound(raw_pick),
        });
        self.world.push(world);
        Ok(self.items.len() - 1)
    }

    /// Freezes what has been collected.
    pub fn build(mut self) -> RenderSnapshot {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        let mut has_geometry = false;
        for item in &self.items {
            if self.meshes[item.mesh].indices.is_empty() {
                continue;
            }
            has_geometry = true;
            let (low, high) = self.meshes[item.mesh].bounds();
            // Every corner, not just the two: a rotated box's extent is not the
            // transform of its extent.
            for corner in 0..8 {
                let point = [
                    if corner & 1 == 0 { low[0] } else { high[0] },
                    if corner & 2 == 0 { low[1] } else { high[1] },
                    if corner & 4 == 0 { low[2] } else { high[2] },
                ];
                let placed = apply(&item.transform, point);
                for axis in 0..3 {
                    min[axis] = min[axis].min(placed[axis]);
                    max[axis] = max[axis].max(placed[axis]);
                }
            }
        }

        let identity = snapshot_identity(&self.meshes, &self.items);
        for item in &mut self.items {
            item.pick.snapshot = identity;
        }

        RenderSnapshot {
            meshes: self.meshes,
            items: self.items,
            min,
            max,
            has_geometry,
            identity,
        }
    }
}

/// A deterministic generation for CPU pick values.
///
/// It is deliberately absent from the raw GPU value: a pick target there must
/// stay a u32. A readback therefore retains the snapshot used for the draw,
/// while a `PickId` already decoded on the CPU refuses to resolve against a
/// different picture instead of silently keeping the same integer meaning.
fn snapshot_identity(meshes: &[PackedMesh], items: &[DrawItem]) -> ContentHash {
    let mut hasher = CanonicalHasher::new("ferritecad.render-snapshot");
    hasher.algorithm_version(1);
    hasher.field("meshes").u64(meshes.len() as u64);
    for mesh in meshes {
        hasher.field("vertices").u64(mesh.vertices.len() as u64);
        for value in &mesh.vertices {
            hasher.u64(u64::from(canonical_f32_bits(*value)));
        }
        hasher.field("indices").u64(mesh.indices.len() as u64);
        for index in &mesh.indices {
            hasher.u64(u64::from(*index));
        }
    }
    hasher.field("items").u64(items.len() as u64);
    for item in items {
        hasher.u64(item.mesh as u64);
        for value in item.transform.iter().chain(item.colour.iter()) {
            hasher.u64(u64::from(canonical_f32_bits(*value)));
        }
        hasher.u64(u64::from(item.pick.raw));
    }
    hasher.finish()
}

fn canonical_f32_bits(value: f32) -> u32 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

/// A 3x4 row-major transform as the 4x4 column-major matrix a GPU wants.
fn column_major(transform: &Transform) -> Result<[f32; 16]> {
    let rows = transform.rows();
    let mut out = [0.0f32; 16];
    for column in 0..4 {
        for row in 0..3 {
            let value = rows[row][column] as f32;
            if !value.is_finite() {
                return Err(CadError::input(
                    "a placement transform is outside the range a GPU can represent",
                ));
            }
            out[column * 4 + row] = value;
        }
    }
    out[15] = 1.0;
    Ok(out)
}

/// Checks the same corner arithmetic a vertex shader will perform.
fn ensure_placeable(matrix: &[f32; 16], mesh: &PackedMesh) -> Result<()> {
    let (low, high) = mesh.bounds();
    for corner in 0..8 {
        let point = [
            if corner & 1 == 0 { low[0] } else { high[0] },
            if corner & 2 == 0 { low[1] } else { high[1] },
            if corner & 4 == 0 { low[2] } else { high[2] },
        ];
        if apply(matrix, point).iter().any(|value| !value.is_finite()) {
            return Err(CadError::input(
                "a placement would overflow while a GPU transforms its vertices",
            ));
        }
    }
    Ok(())
}

/// Applies a column-major matrix to a point.
fn apply(matrix: &[f32; 16], point: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for (row, value) in out.iter_mut().enumerate() {
        *value = matrix[row] * point[0]
            + matrix[4 + row] * point[1]
            + matrix[8 + row] * point[2]
            + matrix[12 + row];
    }
    out
}
