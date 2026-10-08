// SPDX-License-Identifier: MIT
//! The window's side of crash recovery (§30M): what the start-up list offers,
//! which Recover is in flight, and the sentences a person reads. Which records
//! exist, whether one may be recovered and what recovering it makes are
//! `ferritecad_jobs::recovery`'s answers; nothing here reads a record.

use std::thread::JoinHandle;

use ferritecad_jobs::{
    RecordId, RecoveryEntry, RecoveryListing, RecoveryStore, RecoverySummary, format_utc,
};
use ferritecad_types::Result;

/// Which Recover an answer belongs to.
pub(crate) type RecoverGeneration = u64;

#[derive(Default)]
pub(crate) struct Recoveries {
    store: Option<RecoveryStore>,
    /// Why there are no crash copies at all, when there are none to be had.
    unavailable: Option<String>,
    offers: Vec<RecoverySummary>,
    written: Vec<String>,
    refused: Vec<String>,
    /// Later was pressed: the list stays out of the way for this run.
    hidden: bool,
    issued: RecoverGeneration,
    /// The Recover whose answer may still reach the screen.
    current: Option<(RecoverGeneration, RecordId)>,
    workers: Vec<JoinHandle<()>>,
    /// What the last Recover or Delete did.
    outcome: Option<String>,
}

impl Recoveries {
    pub(crate) fn new(store: Result<RecoveryStore>) -> Self {
        match store {
            Ok(store) => Self {
                store: Some(store),
                ..Self::default()
            },
            Err(error) => Self {
                unavailable: Some(format!(
                    "Recovery copies are off: {error}. Documents still open and save normally."
                )),
                ..Self::default()
            },
        }
    }

    pub(crate) fn store(&self) -> Option<&RecoveryStore> {
        self.store.as_ref()
    }

    /// What the folder holds now. Shown once at start and after a Delete, unless
    /// Later hid the list.
    pub(crate) fn listed(&mut self, listing: Result<RecoveryListing>) {
        self.offers.clear();
        self.written.clear();
        self.refused.clear();
        match listing {
            Ok(listing) => {
                for entry in listing.entries {
                    match entry {
                        RecoveryEntry::Recoverable(summary) => {
                            self.written.push(format_utc(summary.written_unix_ms));
                            self.offers.push(summary);
                        }
                        RecoveryEntry::Refused(refusal) => self.refused.push(format!(
                            "A recovery copy could not be used ({}): {}",
                            refusal.kind.as_str(),
                            refusal.error
                        )),
                    }
                }
            }
            Err(error) => {
                self.refused
                    .push(format!("The recovery folder could not be read: {error}"));
            }
        }
    }

    pub(crate) fn later(&mut self) {
        self.hidden = true;
    }

    pub(crate) fn record_at(&self, index: usize) -> Option<&RecoverySummary> {
        self.offers.get(index)
    }

    pub(crate) fn running(&self) -> bool {
        self.current.is_some()
    }

    /// The words this frame shows. The list is empty while hidden.
    pub(crate) fn offers(&self) -> Vec<ferritecad_ui::RecoveryOffer<'_>> {
        if self.hidden {
            return Vec::new();
        }
        self.offers
            .iter()
            .zip(&self.written)
            .map(|(summary, written)| ferritecad_ui::RecoveryOffer {
                name: &summary.name,
                written,
            })
            .collect()
    }

    pub(crate) fn refused(&self) -> &[String] {
        if self.hidden { &[] } else { &self.refused }
    }

    pub(crate) fn outcome(&self) -> Option<&str> {
        self.outcome.as_deref().or(self.unavailable.as_deref())
    }

    /// Starts a Recover. `spawn` is handed the generation to label its answer with.
    pub(crate) fn begin(
        &mut self,
        record: RecordId,
        spawn: impl FnOnce(RecoverGeneration) -> JoinHandle<()>,
    ) -> Option<RecoverGeneration> {
        if self.running() || self.store.is_none() {
            return None;
        }
        self.reap();
        self.issued += 1;
        let generation = self.issued;
        self.workers.push(spawn(generation));
        self.current = Some((generation, record));
        self.outcome = Some("Recovering…".to_owned());
        Some(generation)
    }

    /// Whether this answer is the one the window waits for.
    pub(crate) fn accepts(&self, generation: RecoverGeneration) -> bool {
        self.current
            .is_some_and(|(current, _)| current == generation)
    }

    /// The Recover is over. On success the copy is the open document now and
    /// leaves the list; on failure it stays there and so does the document.
    pub(crate) fn finish(
        &mut self,
        generation: RecoverGeneration,
        shown: std::result::Result<(), String>,
    ) {
        let Some((_, record)) = self.current.take_if(|(current, _)| *current == generation) else {
            return;
        };
        self.reap();
        match shown {
            Ok(()) => {
                if let Some(index) = self.offers.iter().position(|offer| offer.record == record) {
                    let summary = self.offers.remove(index);
                    self.written.remove(index);
                    self.outcome = Some(format!(
                        "Recovered {}. It is a new unsaved document: check it, then Save As.",
                        summary.name
                    ));
                }
            }
            Err(message) => {
                self.outcome = Some(format!(
                    "Could not recover the copy; it is still listed and nothing else changed: {message}"
                ));
            }
        }
    }

    /// A Delete or a listing is over.
    pub(crate) fn deleted(&mut self, outcome: String) {
        self.outcome = Some(outcome);
    }

    fn reap(&mut self) {
        let (done, running): (Vec<_>, Vec<_>) = std::mem::take(&mut self.workers)
            .into_iter()
            .partition(JoinHandle::is_finished);
        self.workers = running;
        for worker in done {
            let _ = worker.join();
        }
    }

    /// Joins every worker. Their answers change nothing any more.
    pub(crate) fn stop_all(&mut self) {
        self.current = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
