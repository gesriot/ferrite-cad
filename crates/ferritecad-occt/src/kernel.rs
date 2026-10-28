// SPDX-License-Identifier: MIT
use std::collections::BTreeMap;

use ferritecad_exchange::Import;
use ferritecad_kernel::{
    ArchiveSlot, BooleanRequest, BrepBlob, ChamferRequest, ExtrudeExtent, ExtrudeRequest,
    FaceSurface, FilletRequest, GeometryKernel, KernelIdentity, Mesh, MeshEdgeRange, MeshEdges,
    MeshFaceRange, MeshVertexRange, MeshVertices, OperationContext, OperationResult, ProfileLoop,
    ProfileSegment, RevolveAxis, RevolveRequest, RevolveTurn, SegmentGeometry, SessionId,
    ShapeHandle, SketchPlane, SubShapeHandle, SubShapeKind, TessellationParams,
};
use ferritecad_types::{CadError, ContentHash, Result, Transform};

use crate::ffi;

/// Marks FerriteCAD's own framing around Open CASCADE's bytes.
const BLOB_MAGIC: &[u8; 4] = b"FCBR";

/// Marks an archive: a shape together with sub-shapes to be found again.
///
/// A separate magic rather than a flag inside the header. An archive read as a
/// plain blob would hand back the compound instead of the solid, and a plain
/// blob read as an archive would find no sub-shapes; both are wrong quietly,
/// so the two are made unreadable as each other.
const NAMED_BLOB_MAGIC: &[u8; 4] = b"FCBN";

/// Byte offsets in FerriteCAD's framing.
const BLOB_VERSION_END: usize = BLOB_MAGIC.len() + 4;
const BLOB_LENGTH_END: usize = BLOB_VERSION_END + 8;
const BLOB_HASH_END: usize = BLOB_LENGTH_END + 32;
const BLOB_HEADER_LEN: usize = BLOB_HASH_END;

/// Version of what FerriteCAD stores around the kernel's bytes.
///
/// Separate from [`KernelIdentity`] on purpose. The identity changes when Open
/// CASCADE or the bridge changes; this changes when FerriteCAD changes the
/// framing, and one moving must not be mistaken for the other.
const BLOB_FORMAT_VERSION: u32 = 2;

/// Open CASCADE behind the FerriteCAD geometry contract.
///
/// This slice implements extrusion, face-associated tessellation, B-Rep
/// encoding and release. Transform still returns [`CadError::Unsupported`]
/// until its own slice; refusing is the honest answer while the alternative
/// would be a plausible wrong one.
///
/// # A decoded shape is geometry only
///
/// Open CASCADE's B-Rep format stores a shape, not the history of the
/// operations that produced it. A shape restored by [`Self::decode_shape`]
/// therefore has no side faces and no caps, and asking for them fails rather
/// than returning an empty list — a naming layer would read empty as "this
/// feature produced nothing". Warm-cache rebuilds need the mapping stored
/// beside the geometry and restored with it.
#[derive(Debug)]
pub struct OcctKernel {
    identity: KernelIdentity,
    session: ffi::Session,
    session_id: SessionId,
    naming_curves: BTreeMap<ShapeHandle, Vec<ferritecad_types::StableEntityId>>,
    naming_operations: BTreeMap<ShapeHandle, ferritecad_kernel::NamingOperation>,
}

impl OcctKernel {
    /// Opens a session against the Open CASCADE this binary was built with.
    pub fn new() -> Result<Self> {
        let version = ffi::version();
        Ok(Self {
            // The version is read from the library rather than assumed. It is
            // part of every cache key, and a build against a different OCCT
            // must not be able to reuse this one's results.
            // The build field carries a digest of the bridge sources and the
            // target triple, not the crate version. A crate version moves on
            // releases; the C++ that computes the geometry moves on edits, and
            // it is the latter that must invalidate a cached result.
            identity: KernelIdentity::new("occt", version, env!("FERRITECAD_BRIDGE_BUILD"))?,
            session: ffi::Session::new()?,
            session_id: SessionId::new(),
            naming_curves: BTreeMap::new(),
            naming_operations: BTreeMap::new(),
        })
    }

    /// How many shapes this session is still holding.
    ///
    /// The same affordance the mock offers, for the same reason: handles are
    /// opaque, so whether a caller released what it made cannot be answered
    /// from outside without asking the session.
    pub fn live_shape_count(&self) -> usize {
        self.session.live_shape_count()
    }

    /// Reads a STEP file that is already in memory.
    ///
    /// Bytes rather than a path: this adapter opens nothing, and where the
    /// data came from is the caller's business.
    ///
    /// Every shape in the returned scene belongs to this session and must be
    /// released like any other. Nothing about the result says the file was
    /// sound — see [`ferritecad_exchange::Import`] for why there is no flag
    /// that would.
    pub fn import_step(&mut self, step: &[u8]) -> Result<Import> {
        let encoded = self.session.import_step(step)?;
        ferritecad_exchange::decode(&encoded, self.session_id)
    }

    /// Require the selected edge to belong to this live target.
    fn check_edge_operation_names(
        &mut self,
        request_target: ShapeHandle,
        target: u64,
        edge: SubShapeHandle,
        what: &str,
        verb: &str,
    ) -> Result<()> {
        if edge.shape() != request_target {
            return Err(CadError::input(format!(
                "{edge} is not a sub-shape of the shape this {what} {verb}"
            )));
        }
        if edge.index() >= self.session.sub_shape_count(target)? as u64 {
            return Err(CadError::input(format!(
                "{edge} was never handed out by this session"
            )));
        }
        Ok(())
    }

    /// Rounds every edge of a shape to one radius.
    ///
    /// # Not part of the kernel contract, on purpose
    ///
    /// This and [`shell`][Self::shell] are here to answer a question, not to
    /// serve a feature: how far can Open CASCADE be pushed before filleting
    /// stops working, and does it say so when it does. Putting them in
    /// [`GeometryKernel`] would settle by declaration what this slice exists
    /// to measure. They move there when a fillet feature is designed, and the
    /// corpus is what that design will be based on.
    ///
    /// The returned shape carries no names. A fillet replaces the faces it
    /// touches, and nothing that named the original names this.
    pub fn fillet_all(
        &mut self,
        shape: ShapeHandle,
        radius: f64,
        context: &OperationContext,
    ) -> Result<ShapeHandle> {
        context.check_cancelled()?;
        let raw = self.raw(shape)?;
        let built = self.session.fillet_all(raw, radius, context.cancel())?;
        Ok(ShapeHandle::new(self.session_id, built))
    }

    /// Hollows a solid to a wall of `thickness`, opening the named faces.
    ///
    /// See [`fillet_all`][Self::fillet_all] for why this is not on the trait.
    pub fn shell(
        &mut self,
        shape: ShapeHandle,
        thickness: f64,
        open_faces: &[SubShapeHandle],
        context: &OperationContext,
    ) -> Result<ShapeHandle> {
        context.check_cancelled()?;
        let raw = self.raw(shape)?;

        let mut faces = Vec::with_capacity(open_faces.len());
        for face in open_faces {
            if face.shape() != shape {
                return Err(CadError::input(format!(
                    "{face} belongs to another shape and cannot be opened in this one"
                )));
            }
            faces.push(face.index());
        }

        let built = self
            .session
            .shell(raw, thickness, &faces, context.cancel())?;
        Ok(ShapeHandle::new(self.session_id, built))
    }

    /// Whether Open CASCADE considers this shape well formed.
    ///
    /// Every shape this adapter returns already passed the check; this is for
    /// asserting about inputs, so a corpus can say an operation was given
    /// something sound before blaming it for what came out.
    pub fn is_valid(&mut self, shape: ShapeHandle) -> Result<bool> {
        let raw = self.raw(shape)?;
        self.session.is_valid(raw)
    }

    /// Face count and volume, in millimetres cubed.
    ///
    /// A diagnostic independent of tessellation, used to assert that a built
    /// or restored solid is the one that was requested.
    pub fn shape_stats(&mut self, shape: ShapeHandle) -> Result<(u64, f64)> {
        self.session.shape_stats(self.raw(shape)?)
    }

    /// The analytic surface one named face lies on.
    ///
    /// The one way to tell a solid whose side really is a cylinder from one
    /// whose side is a fan of planes that a mesh would draw the same way.
    pub fn face_surface(&mut self, face: SubShapeHandle) -> Result<FaceSurface> {
        if face.kind() != SubShapeKind::Face {
            return Err(CadError::input("only a face lies on a surface"));
        }
        let raw = self.raw(face.shape())?;
        self.session.face_surface(raw, face.index())
    }

    /// Analytic cylinder axis (origin in mm, unit direction) of this exact face.
    /// A measurement of an already resolved handle, never an identity search.
    pub fn cylinder_axis(&mut self, face: SubShapeHandle) -> Result<([f64; 3], [f64; 3])> {
        if face.kind() != SubShapeKind::Face {
            return Err(CadError::input("only a cylindrical face has this axis"));
        }
        let raw = self.raw(face.shape())?;
        self.session.cylinder_axis(raw, face.index())
    }

    /// Analytic axis (origin in mm, unit direction) of one named cylindrical
    /// or conical face. A measurement of an already resolved handle, never an
    /// identity search.
    pub fn surface_axis(&mut self, face: SubShapeHandle) -> Result<([f64; 3], [f64; 3])> {
        if face.kind() != SubShapeKind::Face {
            return Err(CadError::input("only a face of revolution has an axis"));
        }
        let raw = self.raw(face.shape())?;
        self.session.surface_axis(raw, face.index())
    }

    /// Analytic plane of one named planar face (§29A): a point on it (mm), its
    /// outward unit normal as the solid has the face, and its area (mm²). A
    /// measurement of an already resolved handle, never an identity search.
    pub fn face_plane(&mut self, face: SubShapeHandle) -> Result<([f64; 3], [f64; 3], f64)> {
        if face.kind() != SubShapeKind::Face {
            return Err(CadError::input("only a planar face has a plane"));
        }
        let raw = self.raw(face.shape())?;
        self.session.face_plane(raw, face.index())
    }

    /// Wraps a kernel payload in FerriteCAD's framing.
    fn frame(&self, magic: &[u8; 4], payload: Vec<u8>) -> Result<BrepBlob> {
        let payload_length = u64::try_from(payload.len())
            .map_err(|_| CadError::kernel("the encoded B-Rep is too large to frame"))?;
        let payload_hash = ContentHash::of_bytes(&payload);

        let mut bytes = Vec::with_capacity(BLOB_HEADER_LEN + payload.len());
        bytes.extend_from_slice(magic);
        bytes.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&payload_length.to_le_bytes());
        bytes.extend_from_slice(payload_hash.as_bytes());
        bytes.extend_from_slice(&payload);

        Ok(BrepBlob::new(self.identity.clone(), bytes))
    }

    /// Checks the framing and returns the kernel payload inside it.
    ///
    /// Identity first, then the magic, then the format version, then the
    /// declared length, then the checksum. Each answers a different way for a
    /// cache entry to be wrong, and every one of them ends in discarding the
    /// entry rather than decoding it.
    fn unframe<'b>(&self, magic: &[u8; 4], blob: &'b BrepBlob) -> Result<&'b [u8]> {
        blob.require_kernel(&self.identity)?;

        let bytes = blob.bytes();
        if bytes.len() < BLOB_HEADER_LEN || &bytes[..magic.len()] != magic {
            return Err(CadError::kernel(
                "this cached shape is not in the expected FerriteCAD blob format; discard the cache",
            ));
        }

        let version = u32::from_le_bytes(
            bytes[magic.len()..BLOB_VERSION_END]
                .try_into()
                .map_err(|_| CadError::kernel("this cached shape has a truncated header"))?,
        );
        if version != BLOB_FORMAT_VERSION {
            return Err(CadError::unsupported(format!(
                "this cached shape is in blob format v{version}, and this build writes \
                 v{BLOB_FORMAT_VERSION}; discard the cache rather than decoding it"
            )));
        }

        let declared_length = u64::from_le_bytes(
            bytes[BLOB_VERSION_END..BLOB_LENGTH_END]
                .try_into()
                .map_err(|_| CadError::kernel("this cached shape has a truncated length"))?,
        );
        let declared_length = usize::try_from(declared_length).map_err(|_| {
            CadError::kernel("this cached shape declares a payload too large for this platform")
        })?;

        let payload = &bytes[BLOB_HEADER_LEN..];
        if payload.len() != declared_length {
            return Err(CadError::kernel(format!(
                "this cached shape declares {declared_length} payload bytes but contains {}; \
                 discard the damaged cache",
                payload.len()
            )));
        }
        if payload.is_empty() {
            return Err(CadError::kernel(
                "this cached shape has an empty B-Rep payload; discard the damaged cache",
            ));
        }

        let expected_hash = ContentHash::from_slice(&bytes[BLOB_LENGTH_END..BLOB_HASH_END])
            .map_err(|_| CadError::kernel("this cached shape has a malformed payload checksum"))?;
        if ContentHash::of_bytes(payload) != expected_hash {
            return Err(CadError::kernel(
                "this cached shape's payload checksum does not match; discard the damaged cache",
            ));
        }

        Ok(payload)
    }

    /// Checks a handle belongs here and unwraps the bridge's identifier.
    fn raw(&self, shape: ShapeHandle) -> Result<u64> {
        if shape.session() != self.session_id {
            return Err(CadError::kernel(format!(
                "{shape} belongs to another kernel session; handles do not survive a rebuild"
            )));
        }
        Ok(shape.index())
    }

    fn face(&self, shape: ShapeHandle, id: u64) -> SubShapeHandle {
        SubShapeHandle::new(shape, SubShapeKind::Face, id)
    }
}

impl GeometryKernel for OcctKernel {
    fn identity(&self) -> &KernelIdentity {
        &self.identity
    }

    fn shape_topology(&mut self, shape: ShapeHandle) -> Result<ferritecad_kernel::ShapeTopology> {
        let raw = self.raw(shape)?;
        let words = self.session.naming_data(raw, false)?;
        if !words.len().is_multiple_of(3) {
            return Err(CadError::kernel("invalid topology record length"));
        }
        let mut topology = ferritecad_kernel::ShapeTopology::default();
        for row in words.chunks_exact(3) {
            let entity = SubShapeHandle::new(shape, naming_kind(row[1])?, row[0]);
            let faces = topology.adjacent_faces.entry(entity).or_default();
            if row[2] != u64::MAX {
                faces.insert(self.face(shape, row[2]));
            }
        }
        Ok(topology)
    }

    fn naming_history(&mut self, shape: ShapeHandle) -> Result<ferritecad_kernel::NamingHistory> {
        use ferritecad_kernel::{NamingHistory, NamingInput, NamingRelation};
        let raw = self.raw(shape)?;
        let words = self.session.naming_data(raw, true)?;
        if !words.len().is_multiple_of(6) {
            return Err(CadError::kernel("invalid history record length"));
        }
        let operation =
            self.naming_operations.get(&shape).copied().ok_or_else(|| {
                CadError::unsupported("the shape has no live operation provenance")
            })?;
        let mut history = NamingHistory {
            operation: Some(operation),
            ..NamingHistory::default()
        };
        for row in words.chunks_exact(6) {
            if row[2] == 7 {
                let curve = self
                    .naming_curves
                    .get(&shape)
                    .and_then(|curves| usize::try_from(row[1]).ok().and_then(|i| curves.get(i)))
                    .copied()
                    .ok_or_else(|| CadError::kernel("unknown cap boundary curve"))?;
                if row[0] != 0 || row[3] != 2 || row[5] != 0 {
                    return Err(CadError::kernel("invalid cap boundary history"));
                }
                history
                    .cap_boundaries
                    .entry(self.face(shape, row[4]))
                    .or_default()
                    .insert(curve);
                continue;
            }
            let input = match row[2] {
                0..=2 => NamingInput::SubShape(SubShapeHandle::new(
                    ShapeHandle::new(self.session_id, row[0]),
                    naming_kind(row[2])?,
                    row[1],
                )),
                3..=5 => {
                    let curve = self
                        .naming_curves
                        .get(&shape)
                        .and_then(|curves| usize::try_from(row[1]).ok().and_then(|i| curves.get(i)))
                        .copied()
                        .ok_or_else(|| {
                            CadError::kernel("history names an unknown profile curve")
                        })?;
                    if row[2] == 3 {
                        NamingInput::Curve(curve)
                    } else {
                        NamingInput::CurveEnd {
                            curve,
                            end: row[2] == 5,
                        }
                    }
                }
                6 => NamingInput::Cap {
                    end: match row[1] {
                        0 => false,
                        1 => true,
                        _ => return Err(CadError::kernel("unknown operation cap")),
                    },
                },
                _ => return Err(CadError::kernel("unknown naming input kind")),
            };
            let relation = match row[3] {
                0 => NamingRelation::Kept,
                1 => NamingRelation::Modified,
                2 => NamingRelation::Generated,
                3 => NamingRelation::Deleted,
                _ => return Err(CadError::kernel("unknown naming relation")),
            };
            let output = if relation == NamingRelation::Deleted {
                None
            } else {
                Some(SubShapeHandle::new(shape, naming_kind(row[5])?, row[4]))
            };
            history.relations.insert((input, relation, output));
        }
        Ok(history)
    }

    fn revolve(
        &mut self,
        request: &RevolveRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        let profile = request.profile();
        let plane = profile.plane();
        // The axis is the request's, stated in the plane's own terms and
        // placed in model space here, where the plane is.
        let (axis_origin, axis_direction) = match request.axis() {
            RevolveAxis::PlaneX => {
                let o = plane.origin();
                let d = plane.x_axis();
                ([o.x, o.y, o.z], [d.x, d.y, d.z])
            }
            RevolveAxis::Line { start, end } => {
                let o = plane.origin();
                let x = plane.x_axis();
                let y = plane.y_axis();
                let dx = end.x - start.x;
                let dy = end.y - start.y;
                if dx.hypot(dy) == 0. {
                    return Err(CadError::input("revolution axis Line has no length"));
                }
                (
                    [
                        o.x + x.x * start.x + y.x * start.y,
                        o.y + x.y * start.x + y.y * start.y,
                        o.z + x.z * start.x + y.z * start.y,
                    ],
                    [
                        x.x * dx + y.x * dy,
                        x.y * dx + y.y * dy,
                        x.z * dx + y.z * dy,
                    ],
                )
            }
            RevolveAxis::PlaneY => {
                let origin = plane.origin();
                let y = plane.y_axis();
                ([origin.x, origin.y, origin.z], [y.x, y.y, y.z])
            }
            other => {
                return Err(CadError::unsupported(format!(
                    "revolution axis {other:?} is not implemented"
                )));
            }
        };
        // The angle is the request's: exactly one full turn, or a sector
        // through its own radians on the bridge's own entry point.
        let partial = match request.turn() {
            RevolveTurn::Full => None,
            RevolveTurn::Partial(turn) => Some(turn.radians()),
            other => {
                return Err(CadError::unsupported(format!(
                    "revolution angle {other:?} is not implemented"
                )));
            }
        };
        let segments: Vec<_> = profile
            .segments()
            .map(|s| segment_of(&s.geometry))
            .collect();
        let loops: Vec<_> = profile.loops().map(|l| l.segments().len()).collect();
        context.progress().report(0.0);
        let raw = self.session.revolve(
            &plane_of(plane),
            &segments,
            ffi::ProfilePartition {
                loops: &loops,
                regions: &profile
                    .regions()
                    .iter()
                    .map(|r| 1 + r.inner.len())
                    .collect::<Vec<_>>(),
            },
            (axis_origin, axis_direction),
            partial.unwrap_or(std::f64::consts::TAU),
            context.cancel(),
        )?;
        context.progress().report(1.0);
        let shape = ShapeHandle::new(self.session_id, raw);
        self.naming_operations
            .insert(shape, ferritecad_kernel::NamingOperation::Revolve);
        self.naming_curves
            .insert(shape, profile.segments().map(|s| s.label).collect());
        let assembled = (|| -> Result<OperationResult> {
            context.check_cancelled()?;
            Ok(OperationResult {
                shape,
                removed_volume: 0.0,
            })
        })();
        if assembled.is_err() {
            self.release(shape);
        }
        assembled
    }

    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;

        let profile = request.profile();
        let plane = plane_of(profile.plane());

        // Every loop, boundary first, in one list with the lengths that
        // partition it. The order is the contract: `segment_index` and
        // `joint_index` below, and in the bridge, both count through the loops
        // in exactly this order, and `Profile::segments` yields them the same
        // way. A profile with no holes produces one length and the single list
        // it always produced.
        let loops: Vec<&ProfileLoop> = profile.loops().collect();
        let loop_segment_counts: Vec<usize> =
            loops.iter().map(|entry| entry.segments().len()).collect();
        let drawn: Vec<&ProfileSegment> = loops.iter().flat_map(|entry| entry.segments()).collect();
        let segments: Vec<ffi::Segment> = drawn
            .iter()
            .map(|segment| segment_of(&segment.geometry))
            .collect();

        let (base_offset, top_offset) = match request.extent() {
            ExtrudeExtent::Blind { distance } => {
                if request.reversed() {
                    (0.0, -distance)
                } else {
                    (0.0, distance)
                }
            }
            ExtrudeExtent::Symmetric { half_distance } => (-half_distance, half_distance),
            other => {
                return Err(CadError::unsupported(format!(
                    "extrusion extent {other:?} is not implemented"
                )));
            }
        };

        // Open CASCADE reports no intermediate progress for a prism — see the
        // note on fc_occt_extrude — so the honest report is "started" and
        // "finished" rather than an invented curve between them.
        context.progress().report(0.0);
        let raw = self.session.extrude(
            &plane,
            &segments,
            ffi::ProfilePartition {
                loops: &loop_segment_counts,
                regions: &profile
                    .regions()
                    .iter()
                    .map(|r| 1 + r.inner.len())
                    .collect::<Vec<_>>(),
            },
            base_offset,
            top_offset,
            context.cancel(),
        )?;
        context.progress().report(1.0);

        let shape = ShapeHandle::new(self.session_id, raw);
        self.naming_operations
            .insert(shape, ferritecad_kernel::NamingOperation::Extrude);
        self.naming_curves
            .insert(shape, profile.segments().map(|s| s.label).collect());
        let assembled = (|| -> Result<OperationResult> {
            context.check_cancelled()?;
            Ok(OperationResult {
                shape,
                removed_volume: 0.0,
            })
        })();

        if assembled.is_err() {
            self.release(shape);
        }
        assembled
    }

    fn solid_count(&mut self, shape: ShapeHandle) -> Result<usize> {
        self.session.solid_count(self.raw(shape)?)
    }

    fn projected_bounds(
        &mut self,
        shape: ShapeHandle,
        direction: ferritecad_types::Vec3,
    ) -> Result<[f64; 2]> {
        self.session
            .projected_bounds(self.raw(shape)?, [direction.x, direction.y, direction.z])
    }

    fn boolean(
        &mut self,
        request: &BooleanRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        let target = self.raw(request.target())?;
        let tool = self.raw(request.tool())?;

        context.progress().report(0.0);
        let (raw, removed_volume) =
            self.session
                .boolean(request.kind(), target, tool, context.cancel())?;
        context.progress().report(1.0);

        let shape = ShapeHandle::new(self.session_id, raw);
        self.naming_operations
            .insert(shape, request.kind().naming_operation());
        let assembled = (|| -> Result<OperationResult> {
            context.check_cancelled()?;
            Ok(OperationResult {
                shape,
                removed_volume,
            })
        })();

        match assembled {
            Ok(result) => Ok(result),
            Err(error) => {
                self.release(shape);
                Err(error)
            }
        }
    }

    fn fillet_edge(
        &mut self,
        request: &FilletRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        let target = self.raw(request.target())?;
        let edge = request.edge();
        self.check_edge_operation_names(request.target(), target, edge, "fillet", "rounds")?;

        context.progress().report(0.0);
        let (raw, removed_volume) = self.session.fillet_edge(
            target,
            edge.index(),
            request.radius_mm(),
            context.cancel(),
        )?;
        context.progress().report(1.0);

        let shape = ShapeHandle::new(self.session_id, raw);
        self.naming_operations
            .insert(shape, ferritecad_kernel::NamingOperation::Fillet);
        let assembled = (|| -> Result<OperationResult> {
            context.check_cancelled()?;
            Ok(OperationResult {
                shape,
                removed_volume,
            })
        })();

        match assembled {
            Ok(result) => Ok(result),
            Err(error) => {
                self.release(shape);
                Err(error)
            }
        }
    }

    fn chamfer_edge(
        &mut self,
        request: &ChamferRequest,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        context.check_cancelled()?;
        let target = self.raw(request.target())?;
        let edge = request.edge();
        self.check_edge_operation_names(request.target(), target, edge, "chamfer", "cuts")?;

        context.progress().report(0.0);
        let (raw, removed_volume) = self.session.chamfer_edge(
            target,
            edge.index(),
            request.distance_mm(),
            context.cancel(),
        )?;
        context.progress().report(1.0);

        let shape = ShapeHandle::new(self.session_id, raw);
        self.naming_operations
            .insert(shape, ferritecad_kernel::NamingOperation::Chamfer);
        let assembled = (|| -> Result<OperationResult> {
            context.check_cancelled()?;
            Ok(OperationResult {
                shape,
                removed_volume,
            })
        })();

        match assembled {
            Ok(result) => Ok(result),
            Err(error) => {
                self.release(shape);
                Err(error)
            }
        }
    }

    fn transform(
        &mut self,
        _shape: ShapeHandle,
        _transform: &Transform,
        _context: &OperationContext,
    ) -> Result<ferritecad_kernel::OperationResult> {
        Err(CadError::unsupported(
            "the Open CASCADE adapter does not implement transform yet",
        ))
    }

    fn tessellate(
        &mut self,
        shape: ShapeHandle,
        params: &TessellationParams,
        context: &OperationContext,
    ) -> Result<Mesh> {
        context.check_cancelled()?;
        let raw = self.raw(shape)?;

        let mesh = self.session.tessellate(
            raw,
            params.linear_deflection(),
            params.angular_deflection(),
            params.relative(),
            context.cancel(),
        )?;

        let faces = mesh
            .face_shapes
            .iter()
            .zip(mesh.face_first.iter().zip(mesh.face_index_count.iter()))
            .map(|(id, (first, count))| MeshFaceRange {
                face: SubShapeHandle::new(shape, SubShapeKind::Face, *id),
                first_index: *first,
                index_count: *count,
            })
            .collect();

        // Read from the same meshing pass the triangles came from, so the
        // segments index the vertices that are actually here. Which segment
        // belongs to which edge is Open CASCADE's own answer, taken from the
        // polyline it records for each edge of each face; nothing here joins
        // vertices by where they happen to be.
        let ranges = mesh
            .edge_shapes
            .iter()
            .zip(
                mesh.edge_first_segment
                    .iter()
                    .zip(mesh.edge_segment_count.iter()),
            )
            .map(|(id, (first, count))| MeshEdgeRange {
                edge: SubShapeHandle::new(shape, SubShapeKind::Edge, *id),
                first_segment: *first,
                segment_count: *count,
            })
            .collect();

        // Which packed positions are which corner, on the same terms: Open
        // CASCADE's own answer, read from each edge's polyline and each edge's
        // topological ends. One corner shared by three faces is one identifier
        // owning three occurrences, not three corners.
        let corners = mesh
            .vertex_shapes
            .iter()
            .zip(
                mesh.vertex_first
                    .iter()
                    .zip(mesh.vertex_occurrence_count.iter()),
            )
            .map(|(id, (first, count))| MeshVertexRange {
                vertex: SubShapeHandle::new(shape, SubShapeKind::Vertex, *id),
                first_occurrence: *first,
                occurrence_count: *count,
            })
            .collect();

        let mesh = Mesh {
            topological_vertices: Some(MeshVertices {
                occurrences: mesh.vertex_occurrences,
                ranges: corners,
            }),
            positions: mesh.positions,
            normals: mesh.normals,
            indices: mesh.indices,
            faces,
            edges: Some(MeshEdges {
                segments: mesh.edge_segments,
                ranges,
            }),
        };

        // Checked here rather than trusted: a mesh that fails this renders as
        // garbage or takes a driver down, and the cause would be a long way
        // from the symptom.
        mesh.validate()?;
        context.check_cancelled()?;
        Ok(mesh)
    }

    fn encode_shape_with(
        &mut self,
        shape: ShapeHandle,
        sub_shapes: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
        let raw = self.raw(shape)?;

        let mut ids = Vec::with_capacity(sub_shapes.len());
        for sub in sub_shapes {
            if sub.shape() != shape {
                return Err(CadError::kernel(format!(
                    "{sub} does not belong to the shape being archived"
                )));
            }
            // Every kind the contract has a word for. `SubShapeKind` is
            // non-exhaustive, so a kind added later is refused here rather
            // than sent to a bridge whose archive has no way to report it
            // back.
            if !matches!(
                sub.kind(),
                SubShapeKind::Face | SubShapeKind::Edge | SubShapeKind::Vertex
            ) {
                return Err(CadError::kernel(format!(
                    "{sub} is a {}, and this slice archives faces, edges and vertices",
                    sub.kind()
                )));
            }
            ids.push(sub.index());
        }

        let (payload, slots) = self.session.encode_shape_named(raw, &ids)?;
        Ok((
            self.frame(NAMED_BLOB_MAGIC, payload)?,
            slots.into_iter().map(ArchiveSlot::new).collect(),
        ))
    }

    fn decode_shape_with(
        &mut self,
        blob: &BrepBlob,
        slots: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        let payload = self.unframe(NAMED_BLOB_MAGIC, blob)?;

        let raw_slots: Vec<u32> = slots.iter().map(|s| s.index()).collect();
        let (raw_shape, raw_subs) = self.session.decode_shape_named(payload, &raw_slots)?;

        let shape = ShapeHandle::new(self.session_id, raw_shape);
        // The kind each sub-shape actually came back as, read off the restored
        // shape by the bridge and carried here unchanged. Not inferred from
        // the slot's position, and not from what the entries beside it were:
        // an archive mixes faces, edges and vertices, and either inference
        // would hand a name geometry of the wrong sort.
        Ok((
            shape,
            raw_subs
                .into_iter()
                .map(|(id, kind)| {
                    let kind = match kind {
                        ffi::RawSubShapeKind::Face => SubShapeKind::Face,
                        ffi::RawSubShapeKind::Edge => SubShapeKind::Edge,
                        ffi::RawSubShapeKind::Vertex => SubShapeKind::Vertex,
                    };
                    SubShapeHandle::new(shape, kind, id)
                })
                .collect(),
        ))
    }

    fn encode_shape(&mut self, shape: ShapeHandle) -> Result<BrepBlob> {
        let raw = self.raw(shape)?;
        let payload = self.session.encode_shape(raw)?;
        self.frame(BLOB_MAGIC, payload)
    }

    fn decode_shape(&mut self, blob: &BrepBlob) -> Result<ShapeHandle> {
        let payload = self.unframe(BLOB_MAGIC, blob)?;
        let raw = self.session.decode_shape(payload)?;
        Ok(ShapeHandle::new(self.session_id, raw))
    }

    fn release(&mut self, shape: ShapeHandle) {
        self.naming_curves.remove(&shape);
        self.naming_operations.remove(&shape);
        // A handle from another session names nothing here, and releasing what
        // one does not hold is defined to be harmless.
        if shape.session() == self.session_id {
            self.session.release(shape.index());
        }
    }
}

fn plane_of(plane: &SketchPlane) -> ffi::Plane {
    let origin = plane.origin();
    let x_axis = plane.x_axis();
    let normal = plane.normal();
    ffi::Plane {
        origin: [origin.x, origin.y, origin.z],
        x_axis: [x_axis.x, x_axis.y, x_axis.z],
        normal: [normal.x, normal.y, normal.z],
    }
}

fn segment_of(geometry: &SegmentGeometry) -> ffi::Segment {
    let mut segment = ffi::Segment::zeroed();
    match geometry {
        SegmentGeometry::Line { start, end } => {
            segment.kind = ffi::SEGMENT_LINE;
            segment.start_x = start.x;
            segment.start_y = start.y;
            segment.end_x = end.x;
            segment.end_y = end.y;
        }
        SegmentGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            segment.kind = ffi::SEGMENT_ARC;
            segment.center_x = center.x;
            segment.center_y = center.y;
            segment.radius = *radius;
            segment.start_angle = *start_angle;
            segment.end_angle = *end_angle;
        }
        SegmentGeometry::Circle { center, radius } => {
            segment.kind = ffi::SEGMENT_CIRCLE;
            segment.center_x = center.x;
            segment.center_y = center.y;
            segment.radius = *radius;
        }
        // `SegmentGeometry` is non-exhaustive. An unknown variant reaching the
        // bridge as a zeroed line would be a silently wrong profile, so it is
        // sent as an unknown kind, which the bridge refuses.
        _ => segment.kind = -1,
    }
    segment
}

fn naming_kind(raw: u64) -> Result<SubShapeKind> {
    match raw {
        0 => Ok(SubShapeKind::Face),
        1 => Ok(SubShapeKind::Edge),
        2 => Ok(SubShapeKind::Vertex),
        _ => Err(CadError::kernel("unknown elementary shape kind")),
    }
}
