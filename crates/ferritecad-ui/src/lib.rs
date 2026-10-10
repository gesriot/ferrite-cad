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

pub use edit::{EditChoice, EditExtrudeForm, ExtrusionRow, HeightState, edit_extrude_panel};
pub use input::{Hover, PointerButton, ViewportEvent, ViewportInput};
pub use panels::{
    Activity, CANCEL_EXPORT, CREATE_DOCUMENT, Chosen, ConflictingRule, EMPTY_DOCUMENT, EXPORT_FBX,
    EdgeName, ExportOutcome, FRAME_ALL_KEY, FRAME_KEY, FaceName, GeometryUnavailable, HIDE_KEY,
    ISOLATE_KEY, LENGTH_UNIT, NEW_DOCUMENT, NEW_DOCUMENT_TITLE, NewChoice, NewContent,
    NewDocumentForm, OmittedDefinition, OpenFailure, PROJECTION_KEY, PublishedFile, REDO_DOCUMENT,
    REPLACE_EXISTING, RedundantExplanation, ReplaceChoice, RowVisibility, Rows, SAMPLE_PLATE, SAVE,
    SAVE_AS, SHORTCUT_PRIMARY, SHOW_ALL_KEY, Selected, SolvedSketch, TopologyName, UNDO_DOCUMENT,
    VIEWS, VertexName, create_panel, definitions_panel, export_panel, new_document_form,
    open_failure_panel, replace_confirmation, selection_inspector, sketch_solves_panel, toolbar,
};

mod checkpoint;
pub use checkpoint::{
    CHECKPOINTS_HEADING, CREATE_CHECKPOINT, CheckpointChoice, CheckpointPanel, CheckpointRow,
    DELETE_CHECKPOINT, RESTORE_CHECKPOINT, checkpoint_panel,
};

mod recovery;
pub use recovery::{
    DELETE_RECOVERY, LATER, RECOVER, RECOVER_HEADING, RecoveryChoice, RecoveryOffer, RecoveryPanel,
    recovery_panel,
};

mod open_over_new;
pub use open_over_new::{
    BACK_TO_NEW, DISCARD_NEW_AND_OPEN, OPEN_OVER_NEW_HEADING, OpenOverNewChoice, OpenOverNewPanel,
    new_document_section, open_over_new_panel,
};

mod reopen;
pub use reopen::{
    NOT_NOW, REOPEN, REOPEN_HEADING, ReopenChoice, ReopenPanel, ReopenRow, reopen_panel,
};

mod stl;
pub use stl::{PublishedStl, StlBodyRow, StlChoice, StlExportForm, stl_export_form};

mod tabs;
pub use tabs::{CLOSE_TAB, TabChoice, TabLabel, TabStrip, tab_strip, tab_title};

mod close_form;
pub use close_form::{
    BACK_TO_FORM, CloseFormChoice, CloseFormPanel, DISCARD_FORM_AND_CLOSE, DISCARD_FORM_AND_QUIT,
    DISCARD_NEW_AND_QUIT, close_form_panel, quit_new_panel,
};
