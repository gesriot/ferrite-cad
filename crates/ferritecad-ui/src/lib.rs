// SPDX-License-Identifier: MIT
//! What the interface says, and what a gesture means.
//!
//! This crate owns neither a window nor a surface nor a renderer, and has no
//! event loop. It decides: what a drag does to a camera, whether an event
//! belonged to a panel, and when a frame is owed. All of that is arithmetic
//! and bookkeeping over values, so all of it is tested without a display.
//!
//! The camera operations themselves live one layer down in
//! `ferritecad-viewport` and were settled before anything could deliver an
//! event to them. What is added here is only the translation: which gesture
//! calls which operation, and with what.

mod input;

pub use input::{PointerButton, ViewportEvent, ViewportInput};
