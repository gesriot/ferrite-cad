// SPDX-License-Identifier: MIT
//! §30R: the window's side of reopening the last window's saved files — what the
//! start-up offer says, the queue of files reopened one at a time, and what became
//! of each. Which files a window had is `last_tabs`'s; reading a file is the one
//! Open route (`Loads`, `open_for_view`, `Bind::Open`): nothing here reads a
//! document, makes a session or draws a picture.
//!
//! The queue holds no foreground slot of its own. It starts one ordinary Open at a
//! time and waits for that Open's answer by its generation; while it waits, the
//! reading in flight holds the window as any Open does.

use std::path::{Path, PathBuf};

use crate::LoadGeneration;
use crate::last_tabs::{Folder, LastTabs, LastTabsError};
use crate::sessions::Sessions;
use crate::tabs::{Opening, TabId, Tabs};

/// What became of one listed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Opened,
    /// A tab already names that file (by any name): nothing was read again.
    AlreadyOpen,
    /// Read and refused: the file, its document or its picture.
    Failed(String),
    /// Not read: the window was full, or the person cancelled.
    NotOpened(String),
}

/// A Reopen in progress: the list as it was offered, how far it got, and the
/// reading it waits for.
struct Run {
    set: LastTabs,
    next: usize,
    waiting: Option<(LoadGeneration, usize)>,
    outcomes: Vec<Option<Outcome>>,
}

/// What the window does next for the Reopen in progress.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// Read this file into a new tab through Open, then tell [`Restores::reading`].
    Read(PathBuf),
    /// Every file was tried: show this tab (the one that was shown last time).
    Show(TabId),
    /// Nothing to do now: a reading is in flight, or the Reopen is over.
    Wait,
}

#[derive(Default)]
pub(crate) struct Restores {
    folder: Option<Folder>,
    /// Why nothing can be reopened or kept for the next start at all.
    unavailable: Option<String>,
    /// The list read at start; `None` when there was none, or it was empty.
    offer: Option<LastTabs>,
    /// Why the list read at start was not used.
    refused: Option<String>,
    /// Not now was pressed: the offer stays out of the way for this run.
    hidden: bool,
    run: Option<Run>,
    /// What the last Reopen did to each file, kept until Not now or the next one.
    last: Option<(LastTabs, Vec<Option<Outcome>>)>,
    /// A Quit could not keep its list once already: the next one ends anyway.
    publication_failed: bool,
}

impl Restores {
    pub(crate) fn new(folder: Result<Folder, LastTabsError>) -> Self {
        match folder {
            Ok(folder) => Self {
                folder: Some(folder),
                ..Self::default()
            },
            Err(error) => Self {
                unavailable: Some(format!(
                    "Reopening files on the next start is off: {error}. Documents still open \
                     and save normally."
                )),
                ..Self::default()
            },
        }
    }

    /// Where the list is kept; `None` when no folder is known.
    pub(crate) fn folder(&self) -> Option<&Folder> {
        self.folder.as_ref()
    }

    /// What the start-up reading of the list found.
    pub(crate) fn listed(&mut self, read: Result<Option<LastTabs>, LastTabsError>) {
        self.offer = None;
        self.refused = None;
        match read {
            Ok(Some(set)) if !set.paths.is_empty() => self.offer = Some(set),
            // None was kept, or the last window ended with no saved file open.
            Ok(_) => {}
            Err(error) => {
                self.refused = Some(format!(
                    "The list of files open when FerriteCAD was last quit was not used ({}): \
                     {error}. Nothing was opened or changed.",
                    error.kind()
                ));
            }
        }
    }

    /// Not now: nothing is opened, read or removed; the offer goes for this run.
    pub(crate) fn later(&mut self) {
        if !self.running() {
            self.hidden = true;
        }
    }

    pub(crate) fn running(&self) -> bool {
        self.run.is_some()
    }

    /// Whether Reopen may be pressed, as far as the offer is concerned (the
    /// window adds its own [`crate::can_restore`]).
    pub(crate) fn can_begin(&self) -> bool {
        self.offer.is_some() && !self.hidden && self.run.is_none()
    }

    /// Starts a Reopen of the offered list from its first file.
    pub(crate) fn begin(&mut self) -> bool {
        if !self.can_begin() {
            return false;
        }
        let Some(set) = self.offer.clone() else {
            return false;
        };
        self.last = None;
        self.run = Some(Run {
            outcomes: vec![None; set.paths.len()],
            set,
            next: 0,
            waiting: None,
        });
        true
    }

    /// The reading [`Step::Read`] asked for started as `generation` (`None`: it
    /// did not start). Whether the Reopen now waits for it.
    pub(crate) fn reading(&mut self, generation: Option<LoadGeneration>) -> bool {
        let Some(run) = &mut self.run else {
            return false;
        };
        let index = run.next - 1;
        match generation {
            Some(generation) => {
                run.waiting = Some((generation, index));
                true
            }
            None => {
                run.outcomes[index] = Some(Outcome::Failed("the reading did not start".to_owned()));
                false
            }
        }
    }

    /// A reading answered. Only the generation the Reopen waits for counts:
    /// `shown` is its outcome when `Loads` accepted it, `None` when it did not (it
    /// was cancelled or replaced), which ends the Reopen. Whether the window should
    /// take the next [`step`].
    pub(crate) fn answered(
        &mut self,
        generation: LoadGeneration,
        shown: Option<Result<(), String>>,
    ) -> bool {
        let Some(run) = &mut self.run else {
            return false;
        };
        let Some(index) = run
            .waiting
            .take_if(|(awaited, _)| *awaited == generation)
            .map(|(_, index)| index)
        else {
            return false;
        };
        match shown {
            Some(Ok(())) => run.outcomes[index] = Some(Outcome::Opened),
            Some(Err(message)) => run.outcomes[index] = Some(Outcome::Failed(message)),
            None => {
                self.stop("the reading was cancelled");
                return false;
            }
        }
        true
    }

    /// Cancel of the reading in flight: the queue stops. Tabs already opened stay;
    /// the file being read and the rest are not opened; nothing more is shown.
    pub(crate) fn cancel(&mut self) {
        if self.run.is_some() {
            self.stop("cancelled");
        }
    }

    fn stop(&mut self, why: &str) {
        if let Some(run) = &mut self.run {
            run.waiting = None;
            for outcome in run.outcomes.iter_mut().filter(|outcome| outcome.is_none()) {
                *outcome = Some(Outcome::NotOpened(why.to_owned()));
            }
        }
        self.end();
    }

    /// The Reopen is over: its account is kept until Not now or the next Reopen.
    fn end(&mut self) -> Option<LastTabs> {
        let run = self.run.take()?;
        self.last = Some((run.set.clone(), run.outcomes));
        Some(run.set)
    }

    /// What each file of the Reopen in progress, or else of the last one, came to.
    #[cfg(test)]
    pub(crate) fn outcomes(&self) -> Vec<Option<Outcome>> {
        match (&self.run, &self.last) {
            (Some(run), _) => run.outcomes.clone(),
            (None, Some((_, outcomes))) => outcomes.clone(),
            (None, None) => Vec::new(),
        }
    }

    /// The offer's rows: each file's name and folder, and whether it was shown.
    pub(crate) fn rows(&self) -> Vec<(String, String, bool)> {
        if self.hidden {
            return Vec::new();
        }
        self.offer
            .iter()
            .flat_map(|set| {
                set.paths.iter().enumerate().map(|(index, path)| {
                    (
                        display_name(path),
                        path.parent()
                            .map(|folder| folder.display().to_string())
                            .unwrap_or_default(),
                        set.active == Some(index),
                    )
                })
            })
            .collect()
    }

    pub(crate) fn refused(&self) -> Option<&str> {
        if self.hidden {
            None
        } else {
            self.refused.as_deref()
        }
    }

    /// The account of the Reopen in progress or of the last one — how many files
    /// are open, then each file not opened with why — and why reopening is off.
    /// A file that opens never hides one that did not.
    pub(crate) fn report(&self) -> Vec<String> {
        if self.hidden {
            return Vec::new();
        }
        let mut lines = Vec::new();
        let account = match (&self.run, &self.last) {
            (Some(run), _) => Some((&run.set, &run.outcomes, true)),
            (None, Some((set, outcomes))) => Some((set, outcomes, false)),
            (None, None) => None,
        };
        if let Some((set, outcomes, running)) = account {
            let open = outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Some(Outcome::Opened | Outcome::AlreadyOpen)))
                .count();
            let total = set.paths.len();
            lines.push(if running {
                format!("Reopening the saved files: {open} of {total} open so far…")
            } else {
                format!("{open} of {total} saved files are open, each as it is saved on disk now.")
            });
            for (path, outcome) in set.paths.iter().zip(outcomes) {
                if let Some(Outcome::Failed(why) | Outcome::NotOpened(why)) = outcome {
                    lines.push(format!("{} — not opened: {why}", display_name(path)));
                }
            }
        }
        lines.extend(self.unavailable.clone());
        lines
    }

    /// A Quit could not publish its list. The first time the window stays (`true`:
    /// the person is told and may Quit again); after that it ends anyway.
    pub(crate) fn publication_failed(&mut self) -> bool {
        !std::mem::replace(&mut self.publication_failed, true)
    }
}

/// A file's name for a person: its last part, lossily for display only. The path
/// opened is the exact one.
fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// The next thing a Reopen in progress does: each file that is not open yet is
/// read through Open (one at a time); a file a tab already names is not read
/// again; a full window refuses only files that need a new tab. Once every file was tried, the
/// tab of the file shown last time is shown — or, when that file did not open, of
/// the first listed file that is open — unless it is shown already.
pub(crate) fn step(restores: &mut Restores, tabs: &Tabs, active: &Sessions) -> Step {
    loop {
        let Some(run) = &mut restores.run else {
            return Step::Wait;
        };
        if run.waiting.is_some() {
            return Step::Wait;
        }
        let Some(path) = run.set.paths.get(run.next).cloned() else {
            break;
        };
        let index = run.next;
        run.next += 1;
        match tabs.opening(active, &path) {
            Opening::Load => return Step::Read(path),
            Opening::Shown | Opening::Show(_) => {
                run.outcomes[index] = Some(Outcome::AlreadyOpen);
            }
            Opening::Refused(reason) => {
                // Later entries may already be open. Account for them and still
                // choose the previous active tab after the bounded list ends.
                run.outcomes[index] = Some(Outcome::NotOpened(reason));
            }
        }
    }
    let Some(set) = restores.end() else {
        return Step::Wait;
    };
    let open = |path: &PathBuf| tabs.owner_of(active, path, None);
    let target = set
        .active
        .and_then(|index| set.paths.get(index))
        .and_then(open)
        .or_else(|| set.paths.iter().find_map(open));
    match target {
        Some(tab) if !(active.has_session() && active.tab() == tab) => Step::Show(tab),
        _ => Step::Wait,
    }
}
