// SPDX-License-Identifier: MIT
//! The window's side of named checkpoints (§30N): whether Create, Restore and
//! Delete may be pressed, and the words the panel shows. What a checkpoint is and
//! what each action makes belong to `ferritecad_jobs` (the steps) and
//! `ferritecad_document` (the catalog); the operation runs through `Sessions`.

use ferritecad_document::{
    CheckpointEntry, MAX_CHECKPOINT_BYTES, MAX_CHECKPOINT_NAME_CHARS, MAX_CHECKPOINTS,
};
use ferritecad_jobs::Snapshot;

/// Whether each kind of button may be pressed now, and if not why, in a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Availability {
    pub(crate) create: Result<(), String>,
    /// Restore and Delete.
    pub(crate) act: Result<(), String>,
}

/// The one predicate the panel's buttons and the handlers both ask. `settled`:
/// nothing reads or replaces the document (no Open, New, export, Apply, Undo,
/// Save or Recover in flight); `form_open`: a form describes the picture.
pub(crate) fn availability(
    settled: bool,
    form_open: bool,
    current: Option<&Snapshot>,
    name: &str,
) -> Availability {
    let shared = if current.is_none() {
        Err("Open or create a document first.".to_owned())
    } else if !settled {
        Err("Wait for the current operation to finish.".to_owned())
    } else if form_open {
        Err(
            "Close the open form first: a checkpoint keeps the accepted model, not a draft."
                .to_owned(),
        )
    } else {
        Ok(())
    };
    let listed = current.map(|snapshot| {
        snapshot
            .checkpoints()
            .map_err(|error| format!("The checkpoint list cannot be read: {error}"))
    });
    let act = match (&shared, &listed) {
        (Err(reason), _) => Err(reason.clone()),
        (Ok(()), Some(Err(reason))) => Err(reason.clone()),
        _ => Ok(()),
    };
    let create = act.clone().and_then(|()| {
        ferritecad_document::checkpoint_name(name).map_err(|_| {
            format!(
                "Type a name of 1 to {MAX_CHECKPOINT_NAME_CHARS} characters, without tabs or \
                 line breaks."
            )
        })?;
        match listed {
            Some(Ok(entries)) if entries.len() >= MAX_CHECKPOINTS => Err(format!(
                "This document keeps at most {MAX_CHECKPOINTS} checkpoints; delete one first."
            )),
            _ => Ok(()),
        }
    });
    Availability { create, act }
}

/// One row's words: its name, when it was made, and whether it holds the model
/// on screen.
pub(crate) fn rows(snapshot: &Snapshot) -> Vec<(String, String, bool)> {
    let drawn = snapshot.drawn_model();
    snapshot
        .checkpoints()
        .unwrap_or_default()
        .iter()
        .map(|entry| (entry.name.clone(), shown_time(entry), entry.model == drawn))
        .collect()
}

/// How much of the limits this version uses, in one line.
pub(crate) fn usage(snapshot: &Snapshot) -> String {
    let entries = snapshot.checkpoints().unwrap_or_default();
    let bytes: u64 = entries.iter().map(|entry| entry.bytes).sum();
    let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    format!(
        "{} of {MAX_CHECKPOINTS} checkpoints, {:.1} of {:.0} MiB. Saved with the document.",
        entries.len(),
        mib(bytes),
        mib(MAX_CHECKPOINT_BYTES)
    )
}

/// `2026-10-08T07:12:33.123Z` as a person reads it; anything else as stored.
pub(crate) fn shown_time(entry: &CheckpointEntry) -> String {
    let stamp = &entry.created_at;
    match (stamp.get(..10), stamp.get(11..19), stamp.as_bytes().get(10)) {
        (Some(date), Some(time), Some(b'T')) => format!("{date} {time} UTC"),
        _ => stamp.clone(),
    }
}
