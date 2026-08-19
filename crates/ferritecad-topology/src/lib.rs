// SPDX-License-Identifier: MIT
//! Joining a document's durable names to the geometry a rebuild just made.
//!
//! A document stores what a face *is* — the cap of this extrusion, the side
//! raised from that profile segment — and never which face it was. A kernel
//! session hands out opaque handles that mean nothing after it ends. This
//! crate holds the correspondence between the two, for exactly as long as the
//! session does.
//!
//! # The mapping is not storable, by construction
//!
//! [`TopologyMap`] contains `ShapeHandle` and `SubShapeHandle`, which implement
//! no serialisation at all. Nothing here can be written into a document even by
//! accident, and that is the point: a stored handle is a face index by another
//! name, and a face index is what silently retargets a reference after an
//! upstream edit.
//!
//! # There is only one durable reference type
//!
//! [`TopologyRef`][ferritecad_document::TopologyRef] already exists and is
//! already persisted by `ferritecad-document`. This crate defines no second
//! version of it; it reads the stored one and answers with handles.
//!
//! # What this slice resolves
//!
//! `ExtrudeCap` and `ExtrudeSide`, which is what the current vertical path —
//! datum, sketch of lines and arcs, extrusion producing a new body — actually
//! produces. `SketchSegment` and `FilletFace` are refused as
//! [`CadError::Unsupported`][ferritecad_types::CadError::Unsupported]: the
//! geometry kernel does not yet emit a shape for a sketch on its own, and
//! inventing a handle for one would be a name with nothing behind it.

mod archive;
mod codec;
mod map;
mod resolve;

pub use archive::{ArchivedFeature, BoundName, archive_feature, restore_feature};
pub use codec::ARCHIVE_CACHE_KIND;
pub use map::{FeatureNames, RestoredNames, TopologyMap};
pub use resolve::resolve;
