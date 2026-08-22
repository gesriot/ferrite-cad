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

/// Must match `FC_OCCT_SUB_SHAPE_EDGE` in `ferritecad_occt.h`.
const SUB_SHAPE_EDGE: i32 = 1;

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
    fn fc_occt_extrude_cap_edges(
        session: *mut RawSession,
        shape: u64,
        segment_index: usize,
        which: i32,
        out_ids: *mut u64,
        capacity: usize,
        out_count: *mut usize,
        out_error: *mut RawError,
    ) -> i32;
    fn fc_occt_extrude_sweep_edges(
        session: *mut RawSession,
        shape: u64,
        joint_index: usize,
        out_ids: *mut u64,
        capacity: usize,
        out_count: *mut usize,
        out_error: *mut RawError,
    ) -> i32;
    fn fc_occt_extrude_cap_vertices(
        session: *mut RawSession,
        shape: u64,
        joint_index: usize,
        which: i32,
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
    fn fc_occt_import_step(
        session: *mut RawSession,
        bytes: *const u8,
        length: usize,
        out_buffer: *mut u8,
        capacity: usize,
        out_length: *mut usize,
        out_error: *mut RawError,
    ) -> i32;

    fn fc_occt_fillet_all(
        session: *mut RawSession,
        shape: u64,
        radius: f64,
        cancel: Option<CancelFn>,
        cancel_context: *mut c_void,
        out_shape: *mut u64,
        out_error: *mut RawError,
    ) -> i32;

    #[allow(clippy::too_many_arguments)]
    fn fc_occt_shell(
        session: *mut RawSession,
        shape: u64,
        thickness: f64,
        open_faces: *const u64,
        open_face_count: usize,
        cancel: Option<CancelFn>,
        cancel_context: *mut c_void,
        out_shape: *mut u64,
        out_error: *mut RawError,
    ) -> i32;

    fn fc_occt_shape_is_valid(
        session: *mut RawSession,
        shape: u64,
        out_valid: *mut u8,
        out_error: *mut RawError,
    ) -> i32;

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
        out_edges: *mut RawEdgeBuffers,
        out_corners: *mut RawVertexBuffers,
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
        out_sub_kinds: *mut i32,
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
    /// Two vertex indices per segment, into the same vertices as `indices`.
    pub(crate) edge_segments: Vec<u32>,
    pub(crate) edge_shapes: Vec<u64>,
    pub(crate) edge_first_segment: Vec<u32>,
    pub(crate) edge_segment_count: Vec<u32>,
    /// Packed position indices, referenced by the vertex runs below.
    pub(crate) vertex_occurrences: Vec<u32>,
    pub(crate) vertex_shapes: Vec<u64>,
    pub(crate) vertex_first: Vec<u32>,
    pub(crate) vertex_occurrence_count: Vec<u32>,
}

/// The buffers `fc_occt_tessellate` writes the edge association into.
///
/// Mirrors `FcOcctEdgeBuffers`; the field order is the ABI and must not be
/// rearranged.
#[repr(C)]
struct RawEdgeBuffers {
    segments: *mut u32,
    segment_capacity: usize,
    edge_shapes: *mut u64,
    edge_first_segment: *mut u32,
    edge_segment_count: *mut u32,
    edge_capacity: usize,
    out_segment_count: usize,
    out_edge_count: usize,
}

/// Mirrors `FcOcctVertexBuffers`.
#[repr(C)]
struct RawVertexBuffers {
    occurrences: *mut u32,
    occurrence_capacity: usize,
    vertex_shapes: *mut u64,
    vertex_first: *mut u32,
    vertex_occurrence_count: *mut u32,
    vertex_capacity: usize,
    out_occurrence_count: usize,
    out_vertex_count: usize,
}

/// Every size the two-call tessellation protocol promises to reproduce.
///
/// Kept as one value so adding another caller-owned buffer cannot quietly add
/// a count to the first pass without adding it to the second-pass check too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TessellationCounts {
    positions: usize,
    indices: usize,
    faces: usize,
    edge_segments: usize,
    edges: usize,
    vertex_occurrences: usize,
    topological_vertices: usize,
}

fn require_reproducible_tessellation(
    measured: TessellationCounts,
    filled: TessellationCounts,
) -> Result<()> {
    if filled == measured {
        return Ok(());
    }
    Err(CadError::kernel(format!(
        "tessellating the same shape twice gave {measured:?} then {filled:?}; \
         the mesh is not reproducible"
    )))
}

impl RawVertexBuffers {
    /// Buffers that ask for the counts and receive no data.
    fn measuring() -> Self {
        Self {
            occurrences: std::ptr::null_mut(),
            occurrence_capacity: 0,
            vertex_shapes: std::ptr::null_mut(),
            vertex_first: std::ptr::null_mut(),
            vertex_occurrence_count: std::ptr::null_mut(),
            vertex_capacity: 0,
            out_occurrence_count: 0,
            out_vertex_count: 0,
        }
    }
}

impl RawEdgeBuffers {
    /// Buffers that ask for the counts and receive no data.
    fn measuring() -> Self {
        Self {
            segments: std::ptr::null_mut(),
            segment_capacity: 0,
            edge_shapes: std::ptr::null_mut(),
            edge_first_segment: std::ptr::null_mut(),
            edge_segment_count: std::ptr::null_mut(),
            edge_capacity: 0,
            out_segment_count: 0,
            out_edge_count: 0,
        }
    }
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

    /// The edge one profile segment left where a cap meets its swept face.
    ///
    /// Zero or one identifier: the bridge reports none for a segment that
    /// produced no such edge rather than offering a neighbour.
    pub(crate) fn cap_edges(
        &mut self,
        shape: u64,
        segment_index: usize,
        which: i32,
    ) -> Result<Vec<u64>> {
        self.collect_ids(
            "reading the edge where an extrusion cap meets a swept face",
            |s, ids, cap, count, err| {
                // SAFETY: pointers are valid for the call; see `collect_ids`.
                unsafe {
                    fc_occt_extrude_cap_edges(s, shape, segment_index, which, ids, cap, count, err)
                }
            },
        )
    }

    /// The edge swept from one corner of the profile.
    ///
    /// Everything the bridge recorded there, so a count other than one is the
    /// caller's to refuse rather than something silently trimmed here.
    pub(crate) fn sweep_edges(&mut self, shape: u64, joint_index: usize) -> Result<Vec<u64>> {
        self.collect_ids(
            "reading the edge swept from a corner of an extrusion profile",
            |s, ids, cap, count, err| {
                // SAFETY: pointers are valid for the call; see `collect_ids`.
                unsafe { fc_occt_extrude_sweep_edges(s, shape, joint_index, ids, cap, count, err) }
            },
        )
    }

    /// The vertex one corner of the profile reaches on one cap.
    ///
    /// Positional, as the bridge reports it. Everything recorded at that
    /// corner comes back, so a count other than one is the caller's to refuse
    /// rather than something quietly trimmed here.
    pub(crate) fn cap_vertices(
        &mut self,
        shape: u64,
        joint_index: usize,
        which: i32,
    ) -> Result<Vec<u64>> {
        self.collect_ids(
            "reading the vertex where a profile corner reaches a cap",
            |s, ids, cap, count, err| {
                // SAFETY: pointers are valid for the call; see `collect_ids`.
                unsafe {
                    fc_occt_extrude_cap_vertices(s, shape, joint_index, which, ids, cap, count, err)
                }
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

    /// Reads a STEP file that is already in memory.
    ///
    /// Two calls, as the bridge documents: the first learns the length, the
    /// second fills the buffer. The import runs twice, which is the price of
    /// not inventing a second protocol for a tree with names in it.
    pub(crate) fn import_step(&mut self, step: &[u8]) -> Result<Vec<u8>> {
        let mut error = RawError::empty();
        let mut length = 0usize;

        // SAFETY: the input slice lives across the call with its length; the
        // out-parameters are valid; passing no buffer with zero capacity is
        // the size query the bridge documents.
        let status = unsafe {
            fc_occt_import_step(
                self.raw,
                step.as_ptr(),
                step.len(),
                std::ptr::null_mut(),
                0,
                &mut length,
                &mut error,
            )
        };
        interpret(status, &error, "measuring a STEP import")?;

        let mut buffer = vec![0u8; length];
        if length == 0 {
            return Ok(buffer);
        }
        let mut written = 0usize;
        // SAFETY: the buffer was allocated at the length just reported and
        // the capacity passed matches it.
        let status = unsafe {
            fc_occt_import_step(
                self.raw,
                step.as_ptr(),
                step.len(),
                buffer.as_mut_ptr(),
                length,
                &mut written,
                &mut error,
            )
        };
        interpret(status, &error, "reading a STEP file")?;

        if written != length {
            return Err(CadError::kernel(format!(
                "importing the same STEP twice gave {length} bytes then \
                 {written}; the result is not reproducible"
            )));
        }
        Ok(buffer)
    }

    /// Rounds every edge of a shape to one radius.
    pub(crate) fn fillet_all(
        &mut self,
        shape: u64,
        radius: f64,
        cancel: &CancelToken,
    ) -> Result<u64> {
        let context = cancel as *const CancelToken as *mut c_void;
        let mut out = 0u64;
        let mut error = RawError::empty();
        // SAFETY: the out-parameters are valid for the call and the token
        // outlives it; the bridge is noexcept.
        let status = unsafe {
            fc_occt_fillet_all(
                self.raw,
                shape,
                radius,
                Some(cancel_trampoline),
                context,
                &mut out,
                &mut error,
            )
        };
        interpret(status, &error, "rounding every edge")?;
        Ok(out)
    }

    /// Hollows a solid, leaving the named faces open.
    pub(crate) fn shell(
        &mut self,
        shape: u64,
        thickness: f64,
        open_faces: &[u64],
        cancel: &CancelToken,
    ) -> Result<u64> {
        let context = cancel as *const CancelToken as *mut c_void;
        let mut out = 0u64;
        let mut error = RawError::empty();
        // SAFETY: the slice lives across the call and its length is passed
        // with it; the out-parameters are valid.
        let status = unsafe {
            fc_occt_shell(
                self.raw,
                shape,
                thickness,
                open_faces.as_ptr(),
                open_faces.len(),
                Some(cancel_trampoline),
                context,
                &mut out,
                &mut error,
            )
        };
        interpret(status, &error, "hollowing a solid")?;
        Ok(out)
    }

    /// Whether Open CASCADE considers this shape well formed.
    pub(crate) fn is_valid(&mut self, shape: u64) -> Result<bool> {
        let mut valid = 0u8;
        let mut error = RawError::empty();
        // SAFETY: the out-parameter is valid for the call.
        let status = unsafe { fc_occt_shape_is_valid(self.raw, shape, &mut valid, &mut error) };
        interpret(status, &error, "checking a shape")?;
        Ok(valid != 0)
    }

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
        let mut measured = RawEdgeBuffers::measuring();
        let mut measured_corners = RawVertexBuffers::measuring();

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
                &mut measured,
                &mut measured_corners,
                &mut vertices,
                &mut indices,
                &mut faces,
                &mut error,
            )
        };
        interpret(status, &error, "measuring a tessellation")?;
        let segments = measured.out_segment_count;
        let edges = measured.out_edge_count;
        let occurrences = measured_corners.out_occurrence_count;
        let corners = measured_corners.out_vertex_count;
        let measured_counts = TessellationCounts {
            positions: vertices,
            indices,
            faces,
            edge_segments: segments,
            edges,
            vertex_occurrences: occurrences,
            topological_vertices: corners,
        };

        let coordinates = vertices
            .checked_mul(3)
            .ok_or_else(|| CadError::kernel("the tessellation vertex count overflows usize"))?;
        let segment_indices = segments
            .checked_mul(2)
            .ok_or_else(|| CadError::kernel("the tessellation segment count overflows usize"))?;
        let mut mesh = RawMesh {
            positions: vec![0.0; coordinates],
            normals: vec![0.0; coordinates],
            indices: vec![0; indices],
            face_shapes: vec![0; faces],
            face_first: vec![0; faces],
            face_index_count: vec![0; faces],
            edge_segments: vec![0; segment_indices],
            edge_shapes: vec![0; edges],
            edge_first_segment: vec![0; edges],
            edge_segment_count: vec![0; edges],
            vertex_occurrences: vec![0; occurrences],
            vertex_shapes: vec![0; corners],
            vertex_first: vec![0; corners],
            vertex_occurrence_count: vec![0; corners],
        };
        if vertices == 0
            && indices == 0
            && faces == 0
            && segments == 0
            && edges == 0
            && occurrences == 0
            && corners == 0
        {
            return Ok(mesh);
        }

        let (mut got_vertices, mut got_indices, mut got_faces) = (0usize, 0usize, 0usize);
        let mut filled = RawEdgeBuffers {
            segments: mesh.edge_segments.as_mut_ptr(),
            segment_capacity: segments,
            edge_shapes: mesh.edge_shapes.as_mut_ptr(),
            edge_first_segment: mesh.edge_first_segment.as_mut_ptr(),
            edge_segment_count: mesh.edge_segment_count.as_mut_ptr(),
            edge_capacity: edges,
            out_segment_count: 0,
            out_edge_count: 0,
        };
        let mut filled_corners = RawVertexBuffers {
            occurrences: mesh.vertex_occurrences.as_mut_ptr(),
            occurrence_capacity: occurrences,
            vertex_shapes: mesh.vertex_shapes.as_mut_ptr(),
            vertex_first: mesh.vertex_first.as_mut_ptr(),
            vertex_occurrence_count: mesh.vertex_occurrence_count.as_mut_ptr(),
            vertex_capacity: corners,
            out_occurrence_count: 0,
            out_vertex_count: 0,
        };
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
                &mut filled,
                &mut filled_corners,
                &mut got_vertices,
                &mut got_indices,
                &mut got_faces,
                &mut error,
            )
        };
        interpret(status, &error, "reading a tessellation")?;

        // A second mesher pass that produced different counts would mean the
        // triangulation is not stable, and the buffers above would be part
        // filled with no way to tell which part. Every caller-owned array is
        // represented here, including the vertex association introduced after
        // the original five-count check.
        require_reproducible_tessellation(
            measured_counts,
            TessellationCounts {
                positions: got_vertices,
                indices: got_indices,
                faces: got_faces,
                edge_segments: filled.out_segment_count,
                edges: filled.out_edge_count,
                vertex_occurrences: filled_corners.out_occurrence_count,
                topological_vertices: filled_corners.out_vertex_count,
            },
        )?;
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
    ) -> Result<(u64, Vec<(u64, bool)>)> {
        let mut shape = 0u64;
        let mut resolved = vec![0u64; slots.len()];
        let mut kinds = vec![0i32; slots.len()];
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
                kinds.as_mut_ptr(),
                &mut error,
            )
        };
        interpret(status, &error, "restoring an archived shape")?;
        // `true` for an edge, so the caller names each restored sub-shape what
        // it is rather than what the archive's other entries happened to be.
        Ok((
            shape,
            resolved
                .into_iter()
                .zip(kinds)
                .map(|(id, kind)| (id, kind == SUB_SHAPE_EDGE))
                .collect(),
        ))
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ferritecad_types::ErrorKind;

    use super::*;

    #[test]
    fn both_vertex_counts_belong_to_the_tessellations_two_call_promise() {
        let measured = TessellationCounts {
            positions: 24,
            indices: 36,
            faces: 6,
            edge_segments: 24,
            edges: 12,
            vertex_occurrences: 24,
            topological_vertices: 8,
        };

        require_reproducible_tessellation(measured, measured)
            .expect("identical counts are reproducible");
        for filled in [
            TessellationCounts {
                vertex_occurrences: 23,
                ..measured
            },
            TessellationCounts {
                topological_vertices: 7,
                ..measured
            },
        ] {
            let error = require_reproducible_tessellation(measured, filled)
                .expect_err("neither vertex count may change between calls");
            assert_eq!(error.kind(), ErrorKind::Kernel);
            assert!(error.to_string().contains("not reproducible"), "{error}");
        }
    }

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

    extern "C" fn cancel_on_third_poll(context: *mut c_void) -> i32 {
        // SAFETY: the test passes a live `AtomicUsize` for the duration of the
        // synchronous bridge call.
        let polls = unsafe { &*(context as *const AtomicUsize) };
        i32::from(polls.fetch_add(1, Ordering::SeqCst) + 1 >= 3)
    }

    #[test]
    fn the_mesher_polls_cancellation_inside_the_operation() {
        let mut session = Session::new().expect("opens a real OCCT session");
        let plane = Plane {
            origin: [0.0, 0.0, 0.0],
            x_axis: [1.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        };
        let shape = session
            .extrude(&plane, &rectangle_segments(), 0.0, 2.0, &CancelToken::new())
            .expect("builds a prism");
        let polls = AtomicUsize::new(0);
        let mut vertices = 0usize;
        let mut indices = 0usize;
        let mut faces = 0usize;
        let mut edges = RawEdgeBuffers::measuring();
        let mut corners = RawVertexBuffers::measuring();
        let mut error = RawError::empty();

        // The bridge checks once before and once after OCCT. Cancelling only
        // on the third callback means this cannot pass unless the progress
        // indicator itself was polled during `Perform`.
        // SAFETY: every pointer is valid for this synchronous size query.
        let status = unsafe {
            fc_occt_tessellate(
                session.raw,
                shape,
                0.01,
                0.5,
                0,
                Some(cancel_on_third_poll),
                &polls as *const AtomicUsize as *mut c_void,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                &mut edges,
                &mut corners,
                &mut vertices,
                &mut indices,
                &mut faces,
                &mut error,
            )
        };
        assert_eq!(status, STATUS_CANCELLED);
        assert!(polls.load(Ordering::SeqCst) >= 3);

        session.release(shape);
        assert_eq!(session.live_shape_count(), 0);
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

    #[test]
    fn a_failed_import_buffer_write_rolls_back_every_shape() {
        let session = Session::new().expect("opens a real OCCT session");
        let step: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/step/canonical/01-single-part.step"
        ));

        let mut length = 0usize;
        let mut error = RawError::empty();
        // SAFETY: this is the documented size query; the input and
        // out-parameters live for the duration of the call.
        let status = unsafe {
            fc_occt_import_step(
                session.raw,
                step.as_ptr(),
                step.len(),
                std::ptr::null_mut(),
                0,
                &mut length,
                &mut error,
            )
        };
        assert_eq!(status, STATUS_OK);
        assert!(length > 1);
        assert_eq!(session.live_shape_count(), 0, "the size query kept a shape");

        let mut too_small = vec![0u8; length - 1];
        let mut needed = 0usize;
        // SAFETY: every pointer is valid and the deliberately short capacity
        // matches the allocated output slice.
        let status = unsafe {
            fc_occt_import_step(
                session.raw,
                step.as_ptr(),
                step.len(),
                too_small.as_mut_ptr(),
                too_small.len(),
                &mut needed,
                &mut error,
            )
        };
        assert_eq!(status, STATUS_INVALID_INPUT);
        assert_eq!(needed, length);
        assert_eq!(
            session.live_shape_count(),
            0,
            "an import the caller could not receive leaked its definitions"
        );
    }
}
