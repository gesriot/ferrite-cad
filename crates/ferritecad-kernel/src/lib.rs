// SPDX-License-Identifier: MIT
//! What FerriteCAD asks a geometry kernel to do, expressed without naming one.
//!
//! This crate is a contract and nothing else. It computes no geometry, links no
//! library and has no build script. Its job is to state precisely enough what a
//! kernel must provide that an adapter can be written against it, an evaluator
//! can be written against it, and neither has to know which kernel is present.
//!
//! # What may not appear here
//!
//! No Open CASCADE type, handle or header. No raw pointer. No face index, edge
//! index or traversal position in any persisted form. Those are the things that
//! make a parametric model quietly point at different geometry after an
//! upstream edit, and keeping them out of this file is what makes swapping a
//! kernel a matter of replacing an adapter rather than rewriting a document.
//!
//! # Handles are session-local and never persisted
//!
//! [`ShapeHandle`] and [`SubShapeHandle`] are tokens a kernel session hands out
//! and only that session understands. They implement no serialisation, they
//! carry the identity of the session that issued them, and a session rejects a
//! handle it did not issue. The durable half of naming — semantic roles and
//! selection rules — lives in the document; this crate deliberately cannot
//! express it.
//!
//! # These are not the document's types
//!
//! `ferritecad-document` has its own sketch, point and entity-kind types.
//! Nothing here reuses them, and that is on purpose: a persisted feature
//! payload and a kernel request answer to different pressures, and a shared
//! type would drag one set of constraints into the other. The evaluator
//! converts between them explicitly, where the conversion can be read and
//! tested.

mod context;
mod handle;
mod identity;
mod kernel;
mod profile;
mod request;
mod result;

pub mod mock;

pub use context::{CancelToken, OperationContext, ProgressSink};
pub use handle::{SessionId, ShapeHandle, SubShapeHandle, SubShapeKind};
pub use identity::KernelIdentity;
pub use kernel::{GeometryKernel, extrude_cache_key, tessellation_cache_key};
pub use profile::{
    PlanarPoint, Profile, ProfileLoop, ProfileSegment, SegmentGeometry, SketchPlane,
};
pub use request::{ExtrudeExtent, ExtrudeRequest, TessellationParams};
pub use result::{
    ArchiveSlot, BrepBlob, ExtrudeResult, History, HistoryInput, Mesh, MeshEdgeRange, MeshEdges,
    MeshFaceRange, MeshVertexRange, MeshVertices, OperationResult,
};
