// SPDX-License-Identifier: MIT
//! A kernel made of arithmetic, for testing everything above the kernel.
//!
//! It exists so the evaluator, the topology layer and their tests can be
//! written and run before any adapter does, and afterwards without Open
//! CASCADE installed. It is a test double, not a fallback: it approximates
//! every curve by its chord and knows nothing about booleans, fillets or
//! tolerance. Nothing that ships to a user may compute geometry with it.
//!
//! What it *is* faithful about is the contract — validation, cancellation,
//! history, handle scoping, blob identity and determinism — because those are
//! what the layers above depend on.

use std::collections::BTreeMap;

use ferritecad_types::{CadError, Point3, Result, StableEntityId, Transform};

use crate::context::OperationContext;
use crate::handle::{SessionId, ShapeHandle, SubShapeHandle, SubShapeKind};
use crate::identity::KernelIdentity;
use crate::kernel::GeometryKernel;
use crate::request::{ExtrudeExtent, ExtrudeRequest, TessellationParams};
use crate::result::{
    BrepBlob, ExtrudeResult, History, HistoryInput, Mesh, MeshFaceRange, OperationResult,
};

/// Marks the mock's own blob format, so a stray blob is refused early.
const BLOB_MAGIC: &[u8; 4] = b"FCMK";

/// A prism: a polygon swept between two parallel caps.
#[derive(Debug, Clone, PartialEq)]
struct Prism {
    /// Polygon vertices of the start cap, in model space.
    base: Vec<Point3>,
    /// The same vertices on the end cap.
    top: Vec<Point3>,
    /// The profile segment each side face was raised from, parallel to `base`.
    labels: Vec<StableEntityId>,
}

impl Prism {
    fn side_face_count(&self) -> usize {
        self.base.len()
    }

    /// Face slots: one per side, then the start cap, then the end cap.
    fn start_cap_index(&self) -> u64 {
        self.side_face_count() as u64
    }

    fn end_cap_index(&self) -> u64 {
        self.side_face_count() as u64 + 1
    }
}

/// A geometry kernel that computes prisms and nothing else.
#[derive(Debug)]
pub struct MockKernel {
    identity: KernelIdentity,
    session: SessionId,
    shapes: BTreeMap<u64, Prism>,
    next_index: u64,
}

impl MockKernel {
    pub fn new() -> Self {
        Self::with_version("1.0.0")
    }

    /// A mock claiming a particular version, for testing cache invalidation.
    pub fn with_version(version: &str) -> Self {
        Self {
            identity: KernelIdentity::new("mock", version, "")
                .expect("the mock's own identity is well formed"),
            session: SessionId::new(),
            shapes: BTreeMap::new(),
            next_index: 0,
        }
    }

    /// How many shapes this session is still holding.
    ///
    /// A test affordance, and the only way to check that a caller released
    /// what it created: handles are opaque, so "did anything leak" cannot be
    /// answered from outside without asking the session. A real adapter is
    /// free to offer the same, and it is worth having for the same reason.
    pub fn live_shape_count(&self) -> usize {
        self.shapes.len()
    }

    fn store(&mut self, prism: Prism) -> ShapeHandle {
        let index = self.next_index;
        self.next_index += 1;
        self.shapes.insert(index, prism);
        ShapeHandle::new(self.session, index)
    }

    /// Resolves a handle, refusing one this session did not issue.
    fn lookup(&self, shape: ShapeHandle) -> Result<&Prism> {
        if shape.session() != self.session {
            return Err(CadError::kernel(format!(
                "{shape} belongs to another kernel session; handles do not survive a rebuild"
            )));
        }
        self.shapes
            .get(&shape.index())
            .ok_or_else(|| CadError::kernel(format!("{shape} has been released or never existed")))
    }

    fn face(&self, shape: ShapeHandle, index: u64) -> SubShapeHandle {
        SubShapeHandle::new(shape, SubShapeKind::Face, index)
    }
}

impl Default for MockKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl GeometryKernel for MockKernel {
    fn identity(&self) -> &KernelIdentity {
        &self.identity
    }

    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<ExtrudeResult> {
        context.check_cancelled()?;

        let profile = request.profile();
        if !profile.inner().is_empty() {
            return Err(CadError::unsupported(
                "the mock kernel does not implement profile holes; refusing to return a solid without them",
            ));
        }
        let plane = profile.plane();
        let normal = plane.normal();

        // Every curve becomes its chord. Enough to exercise the contract, and
        // deliberately not enough to be mistaken for a real kernel.
        let mut planar = Vec::new();
        let mut labels = Vec::new();
        for segment in profile.outer().segments() {
            planar.push(segment.geometry.start()?);
            labels.push(segment.label);
        }

        if planar.len() < 3 {
            return Err(CadError::kernel(format!(
                "the mock kernel needs at least three profile corners, got {}",
                planar.len()
            )));
        }

        let (base_offset, top_offset) = match request.extent() {
            ExtrudeExtent::Blind { distance } => {
                if request.reversed() {
                    (0.0, -distance)
                } else {
                    (0.0, distance)
                }
            }
            ExtrudeExtent::Symmetric { half_distance } => (-half_distance, half_distance),
        };

        let mut base = Vec::with_capacity(planar.len());
        let mut top = Vec::with_capacity(planar.len());
        for point in &planar {
            let on_plane = plane.to_model(*point)?;
            base.push(Point3::new(
                on_plane.x + normal.x * base_offset,
                on_plane.y + normal.y * base_offset,
                on_plane.z + normal.z * base_offset,
            )?);
            top.push(Point3::new(
                on_plane.x + normal.x * top_offset,
                on_plane.y + normal.y * top_offset,
                on_plane.z + normal.z * top_offset,
            )?);
        }
        context.progress().report(0.5);
        context.check_cancelled()?;

        let prism = Prism { base, top, labels };
        let side_faces = prism.side_face_count();
        let start_cap_index = prism.start_cap_index();
        let end_cap_index = prism.end_cap_index();
        let labels = prism.labels.clone();
        let shape = self.store(prism);

        // Each side face is generated from the segment it was raised from.
        // The caps come from no input at all, which is why they are reported
        // separately rather than squeezed into the history.
        let mut history = History::new();
        for (index, label) in labels.iter().enumerate().take(side_faces) {
            history.record_generated(
                HistoryInput::Segment(*label),
                self.face(shape, index as u64),
            );
        }

        context.progress().report(1.0);
        Ok(ExtrudeResult {
            shape,
            history,
            start_cap: vec![self.face(shape, start_cap_index)],
            end_cap: vec![self.face(shape, end_cap_index)],
        })
    }

    fn transform(
        &mut self,
        shape: ShapeHandle,
        transform: &Transform,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        let source = self.lookup(shape)?.clone();

        let mut moved = Prism {
            base: Vec::with_capacity(source.base.len()),
            top: Vec::with_capacity(source.top.len()),
            labels: source.labels.clone(),
        };
        for point in &source.base {
            moved.base.push(transform.apply_to_point(*point)?);
        }
        for point in &source.top {
            moved.top.push(transform.apply_to_point(*point)?);
        }
        context.check_cancelled()?;

        let face_count = source.side_face_count() as u64 + 2;
        let result = self.store(moved);

        // A transform preserves every face; each one is modified, not replaced.
        let mut history = History::new();
        for index in 0..face_count {
            history.record_modified(
                HistoryInput::SubShape(self.face(shape, index)),
                self.face(result, index),
            );
        }

        context.progress().report(1.0);
        Ok(OperationResult {
            shape: result,
            history,
        })
    }

    fn tessellate(
        &mut self,
        shape: ShapeHandle,
        params: &TessellationParams,
        context: &OperationContext,
    ) -> Result<Mesh> {
        context.check_cancelled()?;
        let prism = self.lookup(shape)?.clone();

        // The mock's geometry is flat, so deflection changes nothing about the
        // result. Reading it keeps the parameter honest in the signature and
        // makes the unused-argument question explicit rather than accidental.
        let _ = params;

        let corners = prism.base.len();
        let mut mesh = Mesh::default();
        let push_vertex = |mesh: &mut Mesh, point: Point3, normal: [f64; 3]| -> u32 {
            let index = (mesh.positions.len() / 3) as u32;
            mesh.positions
                .extend_from_slice(&[point.x as f32, point.y as f32, point.z as f32]);
            mesh.normals
                .extend_from_slice(&[normal[0] as f32, normal[1] as f32, normal[2] as f32]);
            index
        };

        // Side faces, one quad each, in segment order.
        for index in 0..corners {
            let next = (index + 1) % corners;
            let a = prism.base[index];
            let b = prism.base[next];
            let up = [
                prism.top[index].x - a.x,
                prism.top[index].y - a.y,
                prism.top[index].z - a.z,
            ];
            let along = [b.x - a.x, b.y - a.y, b.z - a.z];
            let normal = normalize(cross(along, up));

            let first_index = mesh.indices.len() as u32;
            let v0 = push_vertex(&mut mesh, a, normal);
            let v1 = push_vertex(&mut mesh, b, normal);
            let v2 = push_vertex(&mut mesh, prism.top[next], normal);
            let v3 = push_vertex(&mut mesh, prism.top[index], normal);
            mesh.indices.extend_from_slice(&[v0, v1, v2, v0, v2, v3]);

            mesh.faces.push(MeshFaceRange {
                face: self.face(shape, index as u64),
                first_index,
                index_count: 6,
            });
        }
        context.check_cancelled()?;

        // Caps as triangle fans, the start one wound the other way so both
        // face outwards.
        for (cap, points, outward) in [
            (prism.start_cap_index(), &prism.base, false),
            (prism.end_cap_index(), &prism.top, true),
        ] {
            let normal = normalize(cross(
                [
                    points[1].x - points[0].x,
                    points[1].y - points[0].y,
                    points[1].z - points[0].z,
                ],
                [
                    points[2].x - points[1].x,
                    points[2].y - points[1].y,
                    points[2].z - points[1].z,
                ],
            ));
            let normal = if outward {
                normal
            } else {
                [-normal[0], -normal[1], -normal[2]]
            };

            let first_index = mesh.indices.len() as u32;
            let base_vertex = mesh.positions.len() / 3;
            for point in points.iter() {
                push_vertex(&mut mesh, *point, normal);
            }
            for corner in 1..corners - 1 {
                let a = base_vertex as u32;
                let b = (base_vertex + corner) as u32;
                let c = (base_vertex + corner + 1) as u32;
                if outward {
                    mesh.indices.extend_from_slice(&[a, b, c]);
                } else {
                    mesh.indices.extend_from_slice(&[a, c, b]);
                }
            }

            mesh.faces.push(MeshFaceRange {
                face: self.face(shape, cap),
                first_index,
                index_count: mesh.indices.len() as u32 - first_index,
            });
        }

        context.progress().report(1.0);
        mesh.validate()?;
        Ok(mesh)
    }

    fn encode_shape(&mut self, shape: ShapeHandle) -> Result<BrepBlob> {
        let prism = self.lookup(shape)?;

        let mut bytes = Vec::new();
        bytes.extend_from_slice(BLOB_MAGIC);
        bytes.extend_from_slice(&(prism.base.len() as u32).to_le_bytes());
        for point in prism.base.iter().chain(prism.top.iter()) {
            for value in [point.x, point.y, point.z] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for label in &prism.labels {
            bytes.extend_from_slice(&label.to_bytes());
        }

        Ok(BrepBlob::new(self.identity.clone(), bytes))
    }

    fn decode_shape(&mut self, blob: &BrepBlob) -> Result<ShapeHandle> {
        blob.require_kernel(&self.identity)?;

        let bytes = blob.bytes();
        let header = 4 + 4;
        if bytes.len() < header || &bytes[..4] != BLOB_MAGIC {
            return Err(CadError::kernel(
                "cached shape is not in the mock kernel's blob format",
            ));
        }

        let count = u32::from_le_bytes(
            bytes[4..8]
                .try_into()
                .map_err(|_| CadError::kernel("cached shape has a truncated header"))?,
        ) as usize;

        let expected = header + count * 6 * 8 + count * 16;
        if bytes.len() != expected {
            return Err(CadError::kernel(format!(
                "cached shape claims {count} corners, which needs {expected} bytes, but has {}",
                bytes.len()
            )));
        }

        let mut cursor = header;
        let read_point = |cursor: &mut usize| -> Result<Point3> {
            let mut coords = [0.0f64; 3];
            for coord in &mut coords {
                let slice = bytes
                    .get(*cursor..*cursor + 8)
                    .ok_or_else(|| CadError::kernel("cached shape ends mid-coordinate"))?;
                *coord = f64::from_le_bytes(
                    slice
                        .try_into()
                        .map_err(|_| CadError::kernel("cached shape ends mid-coordinate"))?,
                );
                *cursor += 8;
            }
            Point3::new(coords[0], coords[1], coords[2])
        };

        let mut base = Vec::with_capacity(count);
        for _ in 0..count {
            base.push(read_point(&mut cursor)?);
        }
        let mut top = Vec::with_capacity(count);
        for _ in 0..count {
            top.push(read_point(&mut cursor)?);
        }

        let mut labels = Vec::with_capacity(count);
        for _ in 0..count {
            let slice = bytes
                .get(cursor..cursor + 16)
                .ok_or_else(|| CadError::kernel("cached shape ends mid-label"))?;
            labels.push(StableEntityId::from_slice(slice)?);
            cursor += 16;
        }

        Ok(self.store(Prism { base, top, labels }))
    }

    fn release(&mut self, shape: ShapeHandle) {
        if shape.session() == self.session {
            self.shapes.remove(&shape.index());
        }
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length < f64::EPSILON {
        // A degenerate polygon has no meaningful normal. Zero is wrong in a way
        // that shows up as a black face rather than as a crash, which is the
        // right failure mode for a test double.
        return [0.0, 0.0, 0.0];
    }
    [v[0] / length, v[1] / length, v[2] / length]
}
