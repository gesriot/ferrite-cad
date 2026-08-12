// SPDX-License-Identifier: MIT
//! Looking at a model, and nothing else.
//!
//! This crate turns meshes and placements into something a GPU can draw: a
//! packed, immutable [`RenderSnapshot`], a [`Camera`] that survives a resize,
//! and a draw list in a defined order. It views; it does not edit, rebuild,
//! open documents or call a geometry kernel, and it has no dependency that
//! would let it.
//!
//! # View only, on purpose
//!
//! A viewport is where a user first points at something, so it is also where a
//! reference to that something would be born. This one cannot deliver more than
//! it can honestly promise.
//!
//! Every placement of a definition is drawn – four bolts are four draws – but
//! a pick identifies the *definition*, never the placement. A definition has an
//! identity its source file wrote down, and a reference to one survives the
//! document being closed, reopened and re-imported in a new kernel session. An
//! occurrence has only its position in the assembly tree, which the next import
//! is free to renumber. A pick that returned one would look like a durable
//! reference and behave like an index.
//!
//! So a caller may turn a pick into a reference to a definition, and there is
//! no way for it to make one to an occurrence, because the information it would
//! need never leaves this crate. When occurrences gain identities of their own,
//! that will be a new thing this can return – not a promotion of an index that
//! was being handed out all along.
//!
//! # Picks are not a persistence format
//!
//! [`PickId`] does not implement a serialisation trait. A caller can still write
//! its raw integer deliberately – [`PickId::to_raw`] has to exist for a GPU pick
//! buffer – but that integer has meaning only together with the exact snapshot
//! that rendered it. Durable selection starts from a definition identity in the
//! document, never from this value.

mod camera;
mod snapshot;

pub use camera::Camera;
pub use snapshot::{DrawItem, PackedMesh, PickId, RenderSnapshot, SnapshotBuilder, VERTEX_FLOATS};
