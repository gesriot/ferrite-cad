// SPDX-License-Identifier: MIT
use ferritecad_kernel::{
    BrepBlob, ExtrudeExtent, ExtrudeRequest, ExtrudeResult, GeometryKernel, History, HistoryInput,
    KernelIdentity, Mesh, OperationContext, SegmentGeometry, SessionId, ShapeHandle, SketchPlane,
    SubShapeHandle, SubShapeKind, TessellationParams,
};
use ferritecad_types::{CadError, Result, Transform};

use crate::ffi;

/// Marks FerriteCAD's own framing around Open CASCADE's bytes.
const BLOB_MAGIC: &[u8; 4] = b"FCBR";

/// Length of FerriteCAD's framing: the magic and the format version.
const BLOB_HEADER_LEN: usize = BLOB_MAGIC.len() + 4;

/// Version of what FerriteCAD stores around the kernel's bytes.
///
/// Separate from [`KernelIdentity`] on purpose. The identity changes when Open
/// CASCADE or the bridge changes; this changes when FerriteCAD changes the
/// framing, and one moving must not be mistaken for the other.
const BLOB_FORMAT_VERSION: u32 = 1;

/// Open CASCADE behind the FerriteCAD geometry contract.
///
/// This slice implements extrusion, B-Rep encoding and release. Transform and
/// tessellation return [`CadError::Unsupported`] until their own slices;
/// refusing is the honest answer while the alternative would be a plausible
/// wrong one.
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
    /// Present because tessellation is not implemented yet and this is the only
    /// way to assert that a built solid is the one that was asked for.
    pub fn shape_stats(&mut self, shape: ShapeHandle) -> Result<(u64, f64)> {
        self.session.shape_stats(self.raw(shape)?)
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
        _shape: ShapeHandle,
        _params: &TessellationParams,
        _context: &OperationContext,
    ) -> Result<Mesh> {
        Err(CadError::unsupported(
            "the Open CASCADE adapter does not implement tessellation yet",
        ))
    }

    fn encode_shape(&mut self, shape: ShapeHandle) -> Result<BrepBlob> {
        let raw = self.raw(shape)?;
        let payload = self.session.encode_shape(raw)?;

        let mut bytes = Vec::with_capacity(BLOB_HEADER_LEN + payload.len());
        bytes.extend_from_slice(BLOB_MAGIC);
        bytes.extend_from_slice(&BLOB_FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&payload);

        Ok(BrepBlob::new(self.identity.clone(), bytes))
    }

    fn decode_shape(&mut self, blob: &BrepBlob) -> Result<ShapeHandle> {
        // The kernel identity is checked first: a blob written by a different
        // Open CASCADE, or by a different build of this bridge, describes
        // geometry this build may compute differently.
        blob.require_kernel(&self.identity)?;

        let bytes = blob.bytes();
        if bytes.len() <= BLOB_HEADER_LEN || &bytes[..BLOB_MAGIC.len()] != BLOB_MAGIC {
            return Err(CadError::kernel(
                "this cached shape is not in FerriteCAD's B-Rep blob format; discard the cache",
            ));
        }

        // Framed separately from the kernel identity because the two move
        // independently: this version changes when FerriteCAD changes what it
        // stores, not when Open CASCADE changes.
        let version = u32::from_le_bytes(
            bytes[BLOB_MAGIC.len()..BLOB_HEADER_LEN]
                .try_into()
                .map_err(|_| CadError::kernel("this cached shape has a truncated header"))?,
        );
        if version != BLOB_FORMAT_VERSION {
            return Err(CadError::unsupported(format!(
                "this cached shape is in blob format v{version}, and this build writes                  v{BLOB_FORMAT_VERSION}; discard the cache rather than decoding it"
            )));
        }

        let raw = self.session.decode_shape(&bytes[BLOB_HEADER_LEN..])?;
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
