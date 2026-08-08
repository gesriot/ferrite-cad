// SPDX-License-Identifier: MIT
use ferritecad_kernel::{
    BrepBlob, ExtrudeExtent, ExtrudeRequest, ExtrudeResult, GeometryKernel, History, HistoryInput,
    KernelIdentity, Mesh, OperationContext, SegmentGeometry, SessionId, ShapeHandle, SketchPlane,
    SubShapeHandle, SubShapeKind, TessellationParams,
};
use ferritecad_types::{CadError, Result, Transform};

use crate::ffi;

/// Open CASCADE behind the FerriteCAD geometry contract.
///
/// This slice implements extrusion and release, which is what a
/// `Sketch → Extrude` rebuild needs. Transform, tessellation and B-Rep
/// encoding return [`CadError::Unsupported`] until their own slices; refusing
/// is the honest answer while the alternative would be a plausible wrong one.
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
            identity: KernelIdentity::new(
                "occt",
                version,
                concat!("bridge ", env!("CARGO_PKG_VERSION")),
            )?,
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

        let mut history = History::new();
        for (index, segment) in outer.iter().enumerate() {
            for face in self.session.side_faces(raw, index)? {
                history
                    .record_generated(HistoryInput::Segment(segment.label), self.face(shape, face));
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

    fn encode_shape(&mut self, _shape: ShapeHandle) -> Result<BrepBlob> {
        Err(CadError::unsupported(
            "the Open CASCADE adapter does not implement B-Rep encoding yet",
        ))
    }

    fn decode_shape(&mut self, _blob: &BrepBlob) -> Result<ShapeHandle> {
        Err(CadError::unsupported(
            "the Open CASCADE adapter does not implement B-Rep decoding yet",
        ))
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
