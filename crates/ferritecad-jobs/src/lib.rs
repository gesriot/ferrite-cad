// SPDX-License-Identifier: MIT
//! The work an interface asks for, done the same way whoever asked.
//!
//! A command line and a window are two ways of asking for one thing: take this
//! stored document, write it out as a file another program opens, and put that
//! file where it was asked for or nowhere at all. Everything about *how* that
//! is done — which scene is built, which writer is handed it, what makes the
//! last step atomic — is the same in both cases and lives here.
//!
//! # Why this crate exists at all
//!
//! It exists so that there is one of it. The command line had the whole route
//! inside its binary, which is a fine place for it right up until a second
//! interface needs the same route: a window cannot depend on a binary crate,
//! and the two ways out of that — copying the module, or starting the command
//! as a child process — are both ways of ending up with two exports that agree
//! until the day they do not.
//!
//! # What is deliberately not here
//!
//! No kernel implementation. The caller opens the session and hands it in, so
//! the thread that owns an Open CASCADE session is the thread that made it and
//! the one that ends it.
//!
//! No exit codes and no sentences for a person. A finished export is described
//! in terms of what it wrote and what it could not; whether that is a success,
//! a partial success or a number a script reads is a question about the
//! interface, and each of them answers it for itself.
//!
//! No window, no renderer, no picture. What is drawn on screen is never what
//! is written: an export is a cold read of the stored document, so the file is
//! a function of what was saved rather than of what a viewer happens to hold.

mod fbx;
mod publish;

pub use fbx::{FbxExport, FbxExportRequest, SOURCE_IS_DESTINATION, export_document_as_fbx};
pub use publish::{
    Existing, Temporary, is_same_entry, path_entry_exists, refuse_source_as_destination,
};
