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
//! # An imported file is source of truth too
//!
//! A STEP file brought into a document is stored whole, byte for byte, in
//! `imported_sources`. It is not a cache and not a convenience: the scene
//! stored beside it is one kernel's reading of those bytes, and a reading can
//! be redone in a new session, but only while the bytes are still here. An
//! external path would make the document depend on a file it does not own; a
//! chunked or linked mode is deferred until measured sizes justify one.
//!
//! Nothing here calls a kernel. [`Document::reopen_step_import`] is handed the
//! importer and checks the bytes against their stored length and hash before
//! that importer sees them, then compares the whole scene it produced with the
//! one stored, and only then lets a caller near the new handles.
//!
//! A stored scene records what identifies each definition in its source file,
//! so re-attaching matches identities rather than positions: a file that comes
//! back describing the same parts in another order binds, while one that has
//! gained, lost or renamed a part does not. Scenes written before identities
//! existed keep binding by position, which is the guarantee they were written
//! under — see [`ferritecad_exchange::StoredScene`].
//!
//! [`ImportedDefinitionRef`] is what a lasting reference into an imported file
//! looks like: the source and the key together, because a key alone identifies
//! a part within one file and something else in the next. It resolves inside
//! the source it names and nowhere else, and never falls back to a name, a
//! position or a nearest match. A reference into a scene stored before
//! identities existed is refused outright rather than answered from a position.
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
    Access, Document, DocumentMeta, DocumentWriter, ObjectRecord, ReopenedStepImport,
    StepImportRequest, StepImporter, StoredStepImport, StoredTopologyRef,
};
pub use envelope::{Envelope, UnknownObject};
pub use graph::{Dependency, DependencyRole, evaluation_order};
pub use model::{
    Body, CORE_CAPABILITY, CapSide, DatumPlane, EXTRUDE_CAP_EDGE_CAPABILITY,
    EXTRUDE_CAP_VERTEX_CAPABILITY, EXTRUDE_SWEEP_EDGE_CAPABILITY, EndCondition, EntityKind,
    Expression, Extrude, GeomSignature, IMPORTED_STEP_CAPABILITY, ImportedDefinitionRef,
    ImportedStep, ImporterIdentity, ObjectKind, ObjectPayload, Parameter, Point2,
    SKETCH_CONSTRAINTS_CAPABILITY, STEP_SOURCE_FORMAT, SelectionRule, SemanticRole, Sketch,
    SketchConstraint, SketchConstraintRule, SketchCurve, SketchGeometry, SketchPointRef,
    SketchPointSelector, SketchSegmentRef, SolidOperation, TopologyRef,
};
pub use schema::{
    CACHE_EXTENSION, DOCUMENT_EXTENSION, FORMAT_VERSION, MINIMUM_READER_VERSION,
    SUPPORTED_CAPABILITIES,
};
pub use validate::{Diagnostic, Severity, ValidationReport};
