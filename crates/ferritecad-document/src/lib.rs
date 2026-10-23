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
//! Imported definitions bind by their source keys and placements retain their
//! stored occurrence identities. Every object has one CBOR payload layout.
//! Unknown types and corrupt payloads fail opening the document.

mod cache;
mod chamfer;
mod cut_boundary;
mod cut_edit;
mod document;
mod edit;
mod expression_edit;
pub use expression_edit::{EditChange, ExpressionSlot, ExpressionSlotValue};
mod fillet;
mod fillet_radius;
mod height_edit;
mod polygon;
mod revolve;
mod sketch_changes;
pub use chamfer::{
    ChamferChoice, ChamferCorner, EdgeChamfer, MIN_DISTANCE_MM, MIN_FLAT_MM, PreparedEdgeChamfer,
    SavedChamfer, SavedChamferTarget, chamfer_choices, check_distance_value, evaluable_chamfer,
    max_distance_of, prepare_edge_chamfer, saved_chamfer,
};
pub use cut_boundary::{BoundaryOrientation, BoundarySegment, CutBoundary, DiskClearance};
pub use cut_edit::{
    CircularCut, CircularCutEdit, CutChoice, CutExtent, CutParameterChoice, ExtentVocabulary,
    MAX_CIRCULAR_CUTS, NewObject, PreparedCircularCut, PreparedCutParameters, SavedCircularCut,
    SavedCutTarget, SavedCutTool, WALL_CLEARANCE_MM, cut_choices, cut_parameter_choices,
    cut_plane_placement, prepare_circular_cut, prepare_cut_parameters,
};
pub use fillet::{
    EdgeFillet, ExistingFillet, FilletChoice, FilletCorner, MAX_PLATE_FILLETS, MAX_RADIUS_FRACTION,
    MIN_RADIUS_MM, PreparedEdgeFillet, SavedFilletTarget, check_pair, check_radius_value,
    corner_for, evaluable_fillet, fillet_choices, pair_bound, prepare_edge_fillet,
    rectangle_corners, shared_side,
};
pub use fillet_radius::{NeighbourFillet, SavedFillet, saved_fillets};
pub use polygon::{
    AnnularExtrusion, CircleExtrusion, FullTurnRevolution, PolygonExtrusion, RevolutionClosure,
};
pub use revolve::{RevolveChoice, RevolveSegment, revolve_choices, stated_revolution};
pub use sketch_changes::{SketchVertex, move_sketch_vertices, validate_sketch_changes};

mod envelope;
mod graph;
mod model;
mod model_tree;
mod schema;
mod validate;

pub use cache::{CacheEntry, CacheStore};
pub use document::{
    Access, CheckpointEntry, Document, DocumentMeta, DocumentWriter, MAX_CHECKPOINT_BYTES,
    MAX_CHECKPOINT_NAME_CHARS, MAX_CHECKPOINTS, ObjectRecord, ReopenedStepImport,
    StepImportRequest, StepImporter, StoredStepImport, StoredTopologyRef, checkpoint_name,
};
pub use edit::{DocumentVersion, ExtrudeEditSource, editable_extrude};
pub use envelope::Envelope;
pub use graph::{Dependency, DependencyRole, evaluation_order};
pub use model::{
    Body, CapSide, Chamfer, DatumPlane, EndCondition, EntityKind, Expression, Extrude, Fillet,
    FilletEdge, GeomSignature, ImportedDefinitionRef, ImportedStep, ImporterIdentity, ObjectKind,
    ObjectPayload, Parameter, Point2, Revolve, RevolveAngle, RevolveAxis, RevolveExtent,
    STEP_SOURCE_FORMAT, SelectionRule, SemanticRole, Sketch, SketchConstraint,
    SketchConstraintRule, SketchCurve, SketchGeometry, SketchPointRef, SketchPointSelector,
    SketchSegmentRef, SolidOperation, SweptEdge, TopologyRef,
};
pub use schema::{CACHE_EXTENSION, DOCUMENT_EXTENSION};
pub use validate::{Diagnostic, Severity, ValidationReport};

mod sketch_constraints;
pub use sketch_constraints::{
    AddLineConstraint, AddSketchConstraint, CircleConstraintKind, CircleRadiusMm, DirectedAngle,
    DirectedLine, LineConstraintKind, LineEndpoint, LineLengthMm, LineRelation,
    SketchConstraintEdits, SketchCoordinateMm, constraint_circle_roles, sketch_constraint_changes,
};

pub use height_edit::validate_extrude_distance;

pub use model_tree::{ModelRowId, ModelTree, ModelTreeRow};

#[cfg(test)]
mod test_changes;
