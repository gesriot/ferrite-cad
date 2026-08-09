// SPDX-License-Identifier: MIT
use ferritecad_kernel::{
    ArchiveSlot, BrepBlob, ExtrudeExtent, ExtrudeRequest, ExtrudeResult, GeometryKernel, History,
    HistoryInput, KernelIdentity, Mesh, MeshFaceRange, OperationContext, SegmentGeometry,
    SessionId, ShapeHandle, SketchPlane, SubShapeHandle, SubShapeKind, TessellationParams,
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

    /// Face count and volume, in millimetres cubed.
    ///
    /// A diagnostic independent of tessellation, used to assert that a built
    /// or restored solid is the one that was requested.
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

    pub fn shape_stats(&mut self, shape: ShapeHandle) -> Result<(u64, f64)> {
        self.session.shape_stats(self.raw(shape)?)
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

    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<ExtrudeResult> {
        context.check_cancelled()?;

        let profile = request.profile();
        if !profile.inner().is_empty() {
            return Err(CadError::unsupported(
                "a profile with holes needs more than one wire, which this slice does not build",
            ));
        }

        let plane = plane_of(profile.plane());
        let outer = profile.outer().segments();
        let mut segments = Vec::with_capacity(outer.len());
        for segment in outer {
            segments.push(segment_of(&segment.geometry));
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
        let raw =
            self.session
                .extrude(&plane, &segments, base_offset, top_offset, context.cancel())?;
        context.progress().report(1.0);

        let shape = ShapeHandle::new(self.session_id, raw);
        let assembled = (|| -> Result<ExtrudeResult> {
            // A progress callback may cancel at the completion report. The
            // bridge has already handed us a shape at that point, so the error
            // path below must release it rather than returning a cancelled
            // operation with a live, unreachable solid.
            context.check_cancelled()?;

            let mut history = History::new();
            for (index, segment) in outer.iter().enumerate() {
                for face in self.session.side_faces(raw, index)? {
                    history.record_generated(
                        HistoryInput::Segment(segment.label),
                        self.face(shape, face),
                    );
                }
            }

            let start_cap = self
                .session
                .cap_faces(raw, 0)?
                .into_iter()
                .map(|id| self.face(shape, id))
                .collect();
            let end_cap = self
                .session
                .cap_faces(raw, 1)?
                .into_iter()
                .map(|id| self.face(shape, id))
                .collect();

            Ok(ExtrudeResult {
                shape,
                history,
                start_cap,
                end_cap,
            })
        })();

        if assembled.is_err() {
            self.session.release(raw);
        }
        assembled
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

        let mesh = Mesh {
            positions: mesh.positions,
            normals: mesh.normals,
            indices: mesh.indices,
            faces,
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
            if sub.kind() != SubShapeKind::Face {
                return Err(CadError::kernel(format!(
                    "{sub} is a {}, and this slice archives faces",
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
        Ok((
            shape,
            raw_subs
                .into_iter()
                .map(|id| self.face(shape, id))
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
        // `SegmentGeometry` is non-exhaustive. An unknown variant reaching the
        // bridge as a zeroed line would be a silently wrong profile, so it is
        // sent as an unknown kind, which the bridge refuses.
        _ => segment.kind = -1,
    }
    segment
}
