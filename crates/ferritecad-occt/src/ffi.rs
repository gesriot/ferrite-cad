// SPDX-License-Identifier: MIT
//! The only unsafe code in FerriteCAD.
//!
//! Everything below is a thin, checked wrapper over the flat C ABI in
//! `crates/ferritecad-occt-bridge`. Callers of this module see ordinary Rust
//! functions returning [`Result`]; the pointers, the fixed buffers and the
//! two-call length protocol stop here.
//!
//! The workspace denies `unsafe_code`, so the exception is declared once, in
//! writing, and is greppable.
#![allow(
    unsafe_code,
    reason = "the FFI boundary to Open CASCADE; confined to this module by design"
)]

use std::ffi::{CStr, c_char, c_void};

use ferritecad_kernel::CancelToken;
use ferritecad_types::{CadError, Result};

/// Must match `FC_OCCT_ERROR_CAPACITY` in `ferritecad_occt.h`.
const ERROR_CAPACITY: usize = 512;

const STATUS_OK: i32 = 0;
const STATUS_INVALID_INPUT: i32 = 1;
const STATUS_KERNEL: i32 = 2;
const STATUS_CANCELLED: i32 = 3;
const STATUS_UNSUPPORTED: i32 = 4;
const STATUS_UNKNOWN_HANDLE: i32 = 5;
const STATUS_INTERNAL: i32 = 6;

pub(crate) const SEGMENT_LINE: i32 = 0;
pub(crate) const SEGMENT_ARC: i32 = 1;

#[repr(C)]
struct RawSession {
    _opaque: [u8; 0],
}

#[repr(C)]
struct RawError {
    message: [c_char; ERROR_CAPACITY],
}

impl RawError {
    fn empty() -> Self {
        Self {
            message: [0; ERROR_CAPACITY],
        }
    }

    /// Reads the message the bridge wrote, which is always NUL-terminated
    /// after a failure and all zeroes otherwise.
    fn text(&self) -> String {
        // SAFETY: the bridge writes a NUL within the buffer or leaves it
        // zeroed, so a terminator is present either way.
        let bytes = unsafe { CStr::from_ptr(self.message.as_ptr()) };
        bytes.to_string_lossy().into_owned()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Plane {
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub normal: [f64; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Segment {
    pub kind: i32,
    pub start_x: f64,
    pub start_y: f64,
    pub end_x: f64,
    pub end_y: f64,
    pub center_x: f64,
    pub center_y: f64,
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
}

impl Segment {
    pub(crate) fn zeroed() -> Self {
        Self {
            kind: SEGMENT_LINE,
            start_x: 0.0,
            start_y: 0.0,
            end_x: 0.0,
            end_y: 0.0,
            center_x: 0.0,
            center_y: 0.0,
            radius: 0.0,
            start_angle: 0.0,
            end_angle: 0.0,
        }
    }
}

type CancelFn = extern "C" fn(*mut c_void) -> i32;

unsafe extern "C" {
    fn fc_occt_version() -> *const c_char;
    fn fc_occt_session_create(out_session: *mut *mut RawSession, out_error: *mut RawError) -> i32;
    fn fc_occt_session_destroy(session: *mut RawSession);
    #[allow(clippy::too_many_arguments)]
    fn fc_occt_extrude(
        session: *mut RawSession,
        plane: *const Plane,
        segments: *const Segment,
        segment_count: usize,
        base_offset: f64,
        top_offset: f64,
        cancel: Option<CancelFn>,
        cancel_context: *mut c_void,
        out_shape: *mut u64,
        out_error: *mut RawError,
    ) -> i32;
    fn fc_occt_extrude_side_faces(
        session: *mut RawSession,
        shape: u64,
        segment_index: usize,
        out_ids: *mut u64,
        capacity: usize,
        out_count: *mut usize,
        out_error: *mut RawError,
    ) -> i32;
    fn fc_occt_extrude_cap_faces(
        session: *mut RawSession,
        shape: u64,
        which: i32,
        out_ids: *mut u64,
        capacity: usize,
        out_count: *mut usize,
        out_error: *mut RawError,
    ) -> i32;
    fn fc_occt_shape_stats(
        session: *mut RawSession,
        shape: u64,
        out_face_count: *mut u64,
        out_volume: *mut f64,
        out_error: *mut RawError,
    ) -> i32;
    fn fc_occt_encode_shape(
        session: *mut RawSession,
        shape: u64,
        out_bytes: *mut u8,
        capacity: usize,
        out_length: *mut usize,
        out_error: *mut RawError,
    ) -> i32;
    fn fc_occt_decode_shape(
        session: *mut RawSession,
        bytes: *const u8,
        length: usize,
        out_shape: *mut u64,
        out_error: *mut RawError,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn fc_occt_tessellate(
        session: *mut RawSession,
        shape: u64,
        linear_deflection: f64,
        angular_deflection: f64,
        relative: u8,
        cancel: Option<CancelFn>,
        cancel_context: *mut c_void,
        out_positions: *mut f32,
        out_normals: *mut f32,
        vertex_capacity: usize,
        out_indices: *mut u32,
        index_capacity: usize,
        out_face_shapes: *mut u64,
        out_face_first: *mut u32,
        out_face_index_count: *mut u32,
        face_capacity: usize,
        out_vertex_count: *mut usize,
        out_index_count: *mut usize,
        out_face_count: *mut usize,
        out_error: *mut RawError,
    ) -> i32;

    fn fc_occt_encode_shape_named(
        session: *mut RawSession,
        shape: u64,
        sub_shapes: *const u64,
        sub_shape_count: usize,
        out_slots: *mut u32,
        out_bytes: *mut u8,
        capacity: usize,
        out_length: *mut usize,
        out_error: *mut RawError,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn fc_occt_decode_shape_named(
        session: *mut RawSession,
        bytes: *const u8,
        length: usize,
        slots: *const u32,
        slot_count: usize,
        out_shape: *mut u64,
        out_sub_shapes: *mut u64,
        out_error: *mut RawError,
    ) -> i32;
    fn fc_occt_release_shape(session: *mut RawSession, shape: u64);
    fn fc_occt_live_shape_count(session: *const RawSession) -> usize;
}

/// Asks the token whether the caller wants to stop.
///
/// Called by C++, so it must not unwind: an atomic load cannot panic, and
/// nothing else happens here.
extern "C" fn cancel_trampoline(context: *mut c_void) -> i32 {
    if context.is_null() {
        return 0;
    }
    // SAFETY: `context` is the `&CancelToken` handed to the call below, which
    // outlives it — the bridge only invokes this during that call.
    let token = unsafe { &*(context as *const CancelToken) };
    i32::from(token.is_cancelled())
}

fn interpret(status: i32, error: &RawError, what: &str) -> Result<()> {
    match status {
        STATUS_OK => Ok(()),
        STATUS_CANCELLED => Err(CadError::Cancelled),
        STATUS_INVALID_INPUT => Err(CadError::input(format!("{what}: {}", error.text()))),
        STATUS_UNSUPPORTED => Err(CadError::unsupported(format!("{what}: {}", error.text()))),
        STATUS_KERNEL | STATUS_UNKNOWN_HANDLE => {
            Err(CadError::kernel(format!("{what}: {}", error.text())))
        }
        STATUS_INTERNAL => Err(CadError::kernel(format!(
            "{what}: the bridge caught an exception it could not identify: {}",
            error.text()
        ))),
        other => Err(CadError::kernel(format!(
            "{what}: the bridge returned an unknown status {other}"
        ))),
    }
}

/// The Open CASCADE version the bridge was compiled against.
pub(crate) fn version() -> String {
    // SAFETY: the bridge returns a pointer to static storage.
    let raw = unsafe { fc_occt_version() };
    if raw.is_null() {
        return "unknown".to_owned();
    }
    // SAFETY: static, NUL-terminated, never freed.
    unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned()
}

/// An owned Open CASCADE session.
#[derive(Debug)]
pub(crate) struct Session {
    raw: *mut RawSession,
}

/// A mesh exactly as the bridge produced it, before any naming is attached.
pub(crate) struct RawMesh {
    pub(crate) positions: Vec<f32>,
    pub(crate) normals: Vec<f32>,
    pub(crate) indices: Vec<u32>,
    pub(crate) face_shapes: Vec<u64>,
    pub(crate) face_first: Vec<u32>,
    pub(crate) face_index_count: Vec<u32>,
}

impl Session {
    pub(crate) fn new() -> Result<Self> {
        let mut raw: *mut RawSession = std::ptr::null_mut();
        let mut error = RawError::empty();
        // SAFETY: both out-parameters are valid for the call.
        let status = unsafe { fc_occt_session_create(&mut raw, &mut error) };
        interpret(status, &error, "creating an Open CASCADE session")?;

        if raw.is_null() {
            return Err(CadError::kernel(
                "the bridge reported success but produced no session",
            ));
        }
        Ok(Self { raw })
    }

    pub(crate) fn extrude(
        &mut self,
        plane: &Plane,
        segments: &[Segment],
        base_offset: f64,
        top_offset: f64,
        cancel: &CancelToken,
    ) -> Result<u64> {
        let mut shape = 0u64;
        let mut error = RawError::empty();

        // The token is borrowed for exactly the duration of the call, which is
        // the only time the bridge may invoke the trampoline.
        let context = cancel as *const CancelToken as *mut c_void;

        // SAFETY: the slice is non-empty and lives across the call; the
        // out-parameters are valid; the bridge is noexcept.
        let status = unsafe {
            fc_occt_extrude(
                self.raw,
                plane,
                segments.as_ptr(),
                segments.len(),
                base_offset,
                top_offset,
                Some(cancel_trampoline),
                context,
                &mut shape,
                &mut error,
            )
        };
        interpret(status, &error, "extruding a profile")?;
        Ok(shape)
    }

    pub(crate) fn side_faces(&mut self, shape: u64, segment_index: usize) -> Result<Vec<u64>> {
        self.collect_ids(
            "reading the faces raised from a profile segment",
            |s, ids, cap, count, err| {
                // SAFETY: pointers are valid for the call; see `collect_ids`.
                unsafe { fc_occt_extrude_side_faces(s, shape, segment_index, ids, cap, count, err) }
            },
        )
    }

    pub(crate) fn cap_faces(&mut self, shape: u64, which: i32) -> Result<Vec<u64>> {
        self.collect_ids("reading an extrusion cap", |s, ids, cap, count, err| {
            // SAFETY: pointers are valid for the call; see `collect_ids`.
            unsafe { fc_occt_extrude_cap_faces(s, shape, which, ids, cap, count, err) }
        })
    }

    pub(crate) fn shape_stats(&mut self, shape: u64) -> Result<(u64, f64)> {
        let mut faces = 0u64;
        let mut volume = 0.0f64;
        let mut error = RawError::empty();
        // SAFETY: all out-parameters are valid for the call.
        let status =
            unsafe { fc_occt_shape_stats(self.raw, shape, &mut faces, &mut volume, &mut error) };
        interpret(status, &error, "measuring a shape")?;
        Ok((faces, volume))
    }

    /// Serialises a shape, using the bridge's two-call length protocol.
    pub(crate) fn encode_shape(&mut self, shape: u64) -> Result<Vec<u8>> {
        const WHAT: &str = "encoding a shape for the cache";

        let mut length = 0usize;
        let mut error = RawError::empty();
        // SAFETY: a null buffer with zero capacity is the documented way to
        // ask for the length; the out-parameters are valid for the call.
        let status = unsafe {
            fc_occt_encode_shape(
                self.raw,
                shape,
                std::ptr::null_mut(),
                0,
                &mut length,
                &mut error,
            )
        };
        interpret(status, &error, WHAT)?;

        if length == 0 {
            return Err(CadError::kernel(format!(
                "{WHAT}: the bridge reported an empty encoding"
            )));
        }

        let mut bytes = vec![0u8; length];
        let mut written = 0usize;
        // SAFETY: the buffer is exactly `length` bytes and lives across the
        // call; the out-parameters are valid.
        let status = unsafe {
            fc_occt_encode_shape(
                self.raw,
                shape,
                bytes.as_mut_ptr(),
                length,
                &mut written,
                &mut error,
            )
        };
        interpret(status, &error, WHAT)?;

        if written != length {
            return Err(CadError::kernel(format!(
                "{WHAT}: the bridge first reported {length} bytes and then wrote {written}"
            )));
        }
        Ok(bytes)
    }

    /// Restores a shape from bytes the bridge wrote.
    pub(crate) fn decode_shape(&mut self, bytes: &[u8]) -> Result<u64> {
        let mut shape = 0u64;
        let mut error = RawError::empty();
        // SAFETY: the slice lives across the call and its length is passed
        // alongside it; the out-parameters are valid.
        let status = unsafe {
            fc_occt_decode_shape(
                self.raw,
                bytes.as_ptr(),
                bytes.len(),
                &mut shape,
                &mut error,
            )
        };
        interpret(status, &error, "decoding a cached shape")?;
        Ok(shape)
    }

    /// Archives a shape with the sub-shapes to be found again.
    /// Triangles, their normals, and which face each triangle belongs to.
    ///
    /// Two calls, as the bridge documents: the first learns the sizes and the
    /// second fills the buffers. Re-meshing between them is nearly free
    /// because Open CASCADE keeps the triangulation on the shape.
    pub(crate) fn tessellate(
        &mut self,
        shape: u64,
        linear_deflection: f64,
        angular_deflection: f64,
        relative: bool,
        cancel: &CancelToken,
    ) -> Result<RawMesh> {
        let context = cancel as *const CancelToken as *mut c_void;
        let mut error = RawError::empty();
        let (mut vertices, mut indices, mut faces) = (0usize, 0usize, 0usize);

        // SAFETY: every out-parameter is valid; passing no buffers with zero
        // capacity is the size query the bridge documents.
        let status = unsafe {
            fc_occt_tessellate(
                self.raw,
                shape,
                linear_deflection,
                angular_deflection,
                u8::from(relative),
                Some(cancel_trampoline),
                context,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                &mut vertices,
                &mut indices,
                &mut faces,
                &mut error,
            )
        };
        interpret(status, &error, "measuring a tessellation")?;

        let mut mesh = RawMesh {
            positions: vec![0.0; vertices * 3],
            normals: vec![0.0; vertices * 3],
            indices: vec![0; indices],
            face_shapes: vec![0; faces],
            face_first: vec![0; faces],
            face_index_count: vec![0; faces],
        };
        if vertices == 0 && indices == 0 && faces == 0 {
            return Ok(mesh);
        }

        let (mut got_vertices, mut got_indices, mut got_faces) = (0usize, 0usize, 0usize);
        // SAFETY: each buffer was allocated at the size the call above
        // reported, and the capacities passed match those allocations.
        let status = unsafe {
            fc_occt_tessellate(
                self.raw,
                shape,
                linear_deflection,
                angular_deflection,
                u8::from(relative),
                Some(cancel_trampoline),
                context,
                mesh.positions.as_mut_ptr(),
                mesh.normals.as_mut_ptr(),
                vertices,
                mesh.indices.as_mut_ptr(),
                indices,
                mesh.face_shapes.as_mut_ptr(),
                mesh.face_first.as_mut_ptr(),
                mesh.face_index_count.as_mut_ptr(),
                faces,
                &mut got_vertices,
                &mut got_indices,
                &mut got_faces,
                &mut error,
            )
        };
        interpret(status, &error, "reading a tessellation")?;

        // A second mesher pass that produced different counts would mean the
        // triangulation is not stable, and the buffers above would be part
        // filled with no way to tell which part.
        if (got_vertices, got_indices, got_faces) != (vertices, indices, faces) {
            return Err(CadError::kernel(format!(
                "tessellating the same shape twice gave {vertices}/{indices}/{faces} then                  {got_vertices}/{got_indices}/{got_faces}; the mesh is not reproducible"
            )));
        }
        Ok(mesh)
    }

    pub(crate) fn encode_shape_named(
        &mut self,
        shape: u64,
        sub_shapes: &[u64],
    ) -> Result<(Vec<u8>, Vec<u32>)> {
        const WHAT: &str = "archiving a shape with its named sub-shapes";

        let mut slots = vec![0u32; sub_shapes.len()];
        let mut length = 0usize;
        let mut error = RawError::empty();

        // Two calls, as elsewhere: the first reports the length, the second
        // fills a buffer the caller owns.
        //
        // SAFETY: the slices live across the call and their lengths travel
        // with them; the out-parameters are valid.
        let status = unsafe {
            fc_occt_encode_shape_named(
                self.raw,
                shape,
                sub_shapes.as_ptr(),
                sub_shapes.len(),
                slots.as_mut_ptr(),
                std::ptr::null_mut(),
                0,
                &mut length,
                &mut error,
            )
        };
        interpret(status, &error, WHAT)?;

        if length == 0 {
            return Err(CadError::kernel(format!(
                "{WHAT}: the bridge reported an empty archive"
            )));
        }

        let mut bytes = vec![0u8; length];
        let mut written = 0usize;
        // SAFETY: the buffer is exactly `length` bytes and lives across the
        // call; the out-parameters are valid.
        let status = unsafe {
            fc_occt_encode_shape_named(
                self.raw,
                shape,
                sub_shapes.as_ptr(),
                sub_shapes.len(),
                slots.as_mut_ptr(),
                bytes.as_mut_ptr(),
                length,
                &mut written,
                &mut error,
            )
        };
        interpret(status, &error, WHAT)?;

        if written != length {
            return Err(CadError::kernel(format!(
                "{WHAT}: the bridge first reported {length} bytes and then wrote {written}"
            )));
        }
        Ok((bytes, slots))
    }

    /// Restores a shape and the sub-shapes named by their slots.
    pub(crate) fn decode_shape_named(
        &mut self,
        bytes: &[u8],
        slots: &[u32],
    ) -> Result<(u64, Vec<u64>)> {
        let mut shape = 0u64;
        let mut resolved = vec![0u64; slots.len()];
        let mut error = RawError::empty();

        // SAFETY: both slices live across the call with their lengths; the
        // out-parameters are valid and sized by `slots.len()`.
        let status = unsafe {
            fc_occt_decode_shape_named(
                self.raw,
                bytes.as_ptr(),
                bytes.len(),
                slots.as_ptr(),
                slots.len(),
                &mut shape,
                resolved.as_mut_ptr(),
                &mut error,
            )
        };
        interpret(status, &error, "restoring an archived shape")?;
        Ok((shape, resolved))
    }

    pub(crate) fn release(&mut self, shape: u64) {
        // SAFETY: releasing an unknown identifier is defined to be harmless.
        unsafe { fc_occt_release_shape(self.raw, shape) }
    }

    pub(crate) fn live_shape_count(&self) -> usize {
        // SAFETY: the session pointer is valid for this object's lifetime.
        unsafe { fc_occt_live_shape_count(self.raw) }
    }

    /// Runs the bridge's two-call length protocol: ask for the count, then ask
    /// again with a buffer that size.
    fn collect_ids(
        &mut self,
        what: &str,
        mut call: impl FnMut(*mut RawSession, *mut u64, usize, *mut usize, *mut RawError) -> i32,
    ) -> Result<Vec<u64>> {
        let mut count = 0usize;
        let mut error = RawError::empty();

        let status = call(self.raw, std::ptr::null_mut(), 0, &mut count, &mut error);
        interpret(status, &error, what)?;

        if count == 0 {
            return Ok(Vec::new());
        }

        let mut ids = vec![0u64; count];
        let mut written = 0usize;
        let status = call(self.raw, ids.as_mut_ptr(), count, &mut written, &mut error);
        interpret(status, &error, what)?;

        if written != count {
            return Err(CadError::kernel(format!(
                "{what}: the bridge first reported {count} entries and then wrote {written}"
            )));
        }
        Ok(ids)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `fc_occt_session_create` and is
        // destroyed exactly once, here.
        unsafe { fc_occt_session_destroy(self.raw) }
    }
}

// A session owns C++ state that no other thread touches, so it may be moved to
// a worker. It is deliberately not `Sync`: Open CASCADE is not thread-safe, and
// the contract already says a session is used from one thread at a time.
// SAFETY: the raw pointer is uniquely owned by this value.
unsafe impl Send for Session {}

#[cfg(test)]
mod tests {
    use ferritecad_types::ErrorKind;

    use super::*;

    fn rectangle_segments() -> [Segment; 4] {
        let line = |start_x, start_y, end_x, end_y| Segment {
            kind: SEGMENT_LINE,
            start_x,
            start_y,
            end_x,
            end_y,
            center_x: 0.0,
            center_y: 0.0,
            radius: 0.0,
            start_angle: 0.0,
            end_angle: 0.0,
        };
        [
            line(0.0, 0.0, 10.0, 0.0),
            line(10.0, 0.0, 10.0, 5.0),
            line(10.0, 5.0, 0.0, 5.0),
            line(0.0, 5.0, 0.0, 0.0),
        ]
    }

    #[test]
    fn the_bridge_rejects_trailing_bytes_and_history_queries_on_a_decoded_shape() {
        let mut session = Session::new().expect("opens a real OCCT session");
        let plane = Plane {
            origin: [0.0, 0.0, 0.0],
            x_axis: [1.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        };
        let original = session
            .extrude(&plane, &rectangle_segments(), 0.0, 2.0, &CancelToken::new())
            .expect("builds a prism");
        let bytes = session.encode_shape(original).expect("encodes");

        let mut with_trailing_byte = bytes.clone();
        with_trailing_byte.push(0);
        let err = session
            .decode_shape(&with_trailing_byte)
            .expect_err("the C ABI must consume the whole input");
        assert_eq!(err.kind(), ErrorKind::Input);
        assert_eq!(session.live_shape_count(), 1);

        let decoded = session.decode_shape(&bytes).expect("decodes");
        for err in [
            session
                .side_faces(decoded, 0)
                .expect_err("a decoded shape has no operation history"),
            session
                .cap_faces(decoded, 0)
                .expect_err("a decoded shape has no operation caps"),
        ] {
            assert_eq!(err.kind(), ErrorKind::Unsupported);
        }

        session.release(original);
        session.release(decoded);
        assert_eq!(session.live_shape_count(), 0);
    }
}
