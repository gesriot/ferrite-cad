// SPDX-License-Identifier: MIT
//! The FerriteCAD native document.
//!
//! A document is a single SQLite file with the extension `.fcad`. It holds
//! everything the model means and nothing that can be recomputed: the feature
//! graph, sketches, parameters, expressions and stable topology references.
//!
//! # Source of truth and cache are different files
//!
//! Anything derived — B-Rep, tessellation, entity mappings, previews — lives in
//! a separate `.fcad-cache` sidecar written by [`CacheStore`]. Deleting the
//! sidecar can only cost time, never meaning, and the separation is enforced by
//! the filesystem rather than by discipline. The two files also want different
//! durability settings: the document is journalled conservatively so that it
//! stays a single portable file, while the sidecar uses WAL for speed.
//!
//! # Forward compatibility
//!
//! Every object payload is a CBOR envelope carrying its type, schema version
//! and required capabilities. An object of an unknown type is preserved
//! byte-for-byte and written back unchanged, and a document requiring a
//! capability this build does not implement opens read-only.

mod cache;
mod document;
mod envelope;
mod graph;
mod model;
mod schema;
mod validate;

pub use cache::{CacheEntry, CacheStore};
pub use document::{
    Access, Document, DocumentMeta, DocumentWriter, ObjectRecord, StoredTopologyRef,
};
pub use envelope::{Envelope, UnknownObject};
pub use graph::{Dependency, DependencyRole, evaluation_order};
pub use model::{
    Body, CORE_CAPABILITY, CapSide, DatumPlane, EndCondition, EntityKind, Expression, Extrude,
    GeomSignature, ObjectKind, ObjectPayload, Parameter, Point2, SelectionRule, SemanticRole,
    Sketch, SketchCurve, SketchGeometry, SolidOperation, TopologyRef,
};
pub use schema::{
    CACHE_EXTENSION, DOCUMENT_EXTENSION, FORMAT_VERSION, MINIMUM_READER_VERSION,
    SUPPORTED_CAPABILITIES,
};
pub use validate::{Diagnostic, Severity, ValidationReport};
