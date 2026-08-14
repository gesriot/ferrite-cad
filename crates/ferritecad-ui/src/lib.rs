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

mod input;
mod panels;

pub use input::{Hover, PointerButton, ViewportEvent, ViewportInput};
pub use panels::{
    Activity, Chosen, FRAME_ALL_KEY, FRAME_KEY, FaceName, HIDE_KEY, ISOLATE_KEY, Rows,
    SHOW_ALL_KEY, Selected, VIEWS, definitions_panel, selection_inspector, toolbar,
};
