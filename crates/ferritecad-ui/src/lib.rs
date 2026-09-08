// SPDX-License-Identifier: MIT
//! What the interface says, and what a gesture means.
//!
//! This crate owns neither a window nor a surface nor a renderer, and has no
//! event loop. It decides: what a drag does to a camera, whether an event
//! belonged to a panel, and when a frame is owed. All of that is arithmetic
//! and bookkeeping over values, so all of it is tested without a display.
//!
//! A panel returns what the user asked for and applies nothing: the caller
//! feeds it to the reducer, which is the one place a camera moves. A panel
//! that moved it directly would be a second such place, and the two would
//! disagree the first time either changed.
//!
//! The camera operations themselves live one layer down in
//! `ferritecad-viewport` and were settled before anything could deliver an
//! event to them. What is added here is only the translation: which gesture
//! calls which operation, and with what.

mod edit;
mod input;
mod panels;

pub use edit::{EditChoice, EditExtrudeForm, ExtrusionRow, edit_extrude_panel};
pub use input::{Hover, PointerButton, ViewportEvent, ViewportInput};
pub use panels::{
    Activity, CANCEL_EXPORT, CHOOSE_LOCATION, Chosen, ConflictingRule, EMPTY_DOCUMENT, EXPORT_FBX,
    EdgeName, ExportOutcome, FRAME_ALL_KEY, FRAME_KEY, FaceName, GeometryUnavailable, HIDE_KEY,
    ISOLATE_KEY, LENGTH_UNIT, NEW_DOCUMENT, NEW_DOCUMENT_TITLE, NewChoice, NewContent,
    NewDocumentForm, OmittedDefinition, OpenFailure, PROJECTION_KEY, PublishedFile,
    REPLACE_EXISTING, RedundantExplanation, ReplaceChoice, RowVisibility, Rows, SAMPLE_PLATE,
    SHOW_ALL_KEY, Selected, SolvedSketch, TopologyName, VIEWS, VertexName, create_panel,
    definitions_panel, export_panel, new_document_form, open_failure_panel, replace_confirmation,
    selection_inspector, sketch_solves_panel, toolbar,
};
