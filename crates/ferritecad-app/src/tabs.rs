// SPDX-License-Identifier: MIT
//! Several open documents in tabs of one window (§30O, ADR 0005).
//!
//! A tab is one [`Sessions`] — the window's controller of one `DocumentSession` —
//! and nothing else decides anything about its document: its name, `*`, Undo,
//! Save and checkpoints are its session's. This module owns only which tabs exist,
//! in what order, the hidden tabs' controllers with how they were being looked at,
//! and the two things that move between tabs: showing another tab, and the Quit
//! pass over every unsaved one.
//!
//! The shown tab's `Sessions` is the window's own field, so every existing route
//! (Apply, Add, exports, Save, checkpoints, Recover) keeps working on exactly the
//! accepted active session. A hidden tab has no picture: showing it builds one on
//! a worker, and the tab becomes active in the same statement that shows it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

use ferritecad_jobs::{DocumentSession, RecoveryRecorder, Snapshot};
use ferritecad_kernel::CancelToken;
use ferritecad_types::{CadError, Result};
use ferritecad_ui::ViewportInput;

use crate::sessions::{Bind, Sessions};

/// The most documents one window keeps open. Each tab may hold up to 64 private
/// versions and 512 MiB of them, so the window as a whole holds at most eight
/// times that; a ninth document is refused rather than squeezed in.
pub(crate) const MAX_TABS: usize = 8;

/// The words of that refusal.
pub(crate) fn full() -> String {
    format!(
        "{MAX_TABS} documents are open, the most one window keeps. Close one to open another; \
         nothing was opened."
    )
}

/// A tab's identity for the life of the window: made once, never reused, never a
/// position and never the document's id (two copies of one file share that).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TabId(u64);

static NEXT_TAB: AtomicU64 = AtomicU64::new(1);

impl Default for TabId {
    /// A new identity, different from every one made before in this process.
    fn default() -> Self {
        Self(NEXT_TAB.fetch_add(1, Ordering::Relaxed))
    }
}

impl TabId {
    /// The number the tab row hands back when this tab is pressed.
    pub(crate) fn key(self) -> u64 {
        self.0
    }
}

/// How a hidden tab was being looked at: kept with it, given back when it is
/// shown again. Selection and visibility are not kept: they name parts of a
/// picture, and a hidden tab has none.
#[derive(Clone)]
pub(crate) struct View {
    pub(crate) camera: ViewportInput,
    /// The checkpoint name typed in this tab.
    pub(crate) checkpoint_name: String,
}

struct Hidden {
    sessions: Sessions,
    view: View,
}

/// What follows once a tab is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum After {
    /// Ask about this tab, then go on with Quit.
    Quit,
    /// Ask about this tab, then close it.
    Close,
}

struct Switch {
    generation: u64,
    target: TabId,
    cancel: CancelToken,
    worker: Option<JoinHandle<()>>,
    after: Option<After>,
    /// Keeps the version being drawn alive while the worker reads it.
    _lease: Arc<Snapshot>,
}

/// What an Open of a path does (§30O).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Opening {
    /// Not open in any tab, and there is room: read it into a new tab.
    Load,
    /// Already the shown tab's file.
    Shown,
    /// Already this hidden tab's file: show that tab instead.
    Show(TabId),
    /// The window is full; nothing is read.
    Refused(String),
}

/// What closing a tab needs first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseStep {
    /// Nothing would be lost: close it.
    Now,
    /// Unsaved and hidden: show it first, then ask.
    Show,
    /// Unsaved and shown: ask Save / Discard / Cancel.
    Ask,
}

/// The next thing a Quit pass does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitStep {
    /// The shown tab is unsaved and has not been answered: ask about it.
    Ask,
    /// This hidden tab is unsaved and has not been answered: show it first.
    Show(TabId),
    /// Every unsaved tab was saved or answered Discard: the window may end.
    Exit,
}

/// The open documents of one window, except the shown one's controller, which is
/// the window's own field and is passed in as `active`.
pub(crate) struct Tabs {
    /// Every tab, shown one included, in the order the row shows them.
    order: Vec<TabId>,
    hidden: Vec<Hidden>,
    /// Lends each new tab its own lane of the window's one recovery worker, and
    /// keeps the worker for the next tab after the last one closed.
    recorder: Option<RecoveryRecorder>,
    switch: Option<Switch>,
    /// Workers of switches given up on, and the files they may still read.
    /// Both are released only after the worker ends.
    retired: Vec<(JoinHandle<()>, Arc<Snapshot>)>,
    issued: u64,
    /// The Quit pass in progress: the tabs answered Discard in it so far.
    quitting: Option<Vec<TabId>>,
}

impl Drop for Tabs {
    fn drop(&mut self) {
        self.stop_all();
    }
}

impl Tabs {
    pub(crate) fn new(recorder: Option<RecoveryRecorder>) -> Self {
        Self {
            order: Vec::new(),
            hidden: Vec::new(),
            recorder,
            switch: None,
            retired: Vec::new(),
            issued: 0,
            quitting: None,
        }
    }

    /// A controller for a new tab, with its own recovery lane.
    fn fresh(&self) -> Sessions {
        let mut sessions = Sessions::default();
        if let Some(recorder) = &self.recorder {
            sessions.keep_recovery(recorder.lane());
        }
        sessions
    }

    fn hidden(&self, tab: TabId) -> Option<&Hidden> {
        self.hidden
            .iter()
            .find(|hidden| hidden.sessions.tab() == tab)
    }

    fn take_hidden(&mut self, tab: TabId) -> Option<Hidden> {
        let at = self
            .hidden
            .iter()
            .position(|hidden| hidden.sessions.tab() == tab)?;
        Some(self.hidden.remove(at))
    }

    /// Every open tab's controller, shown one first.
    fn all<'a>(&'a self, active: &'a Sessions) -> impl Iterator<Item = &'a Sessions> {
        active
            .has_session()
            .then_some(active)
            .into_iter()
            .chain(self.hidden.iter().map(|hidden| &hidden.sessions))
    }

    /// How many documents are open.
    pub(crate) fn count(&self) -> usize {
        self.order.len()
    }

    /// Whether another document may be opened, and if not, why.
    pub(crate) fn room(&self) -> std::result::Result<(), String> {
        if self.count() >= MAX_TABS {
            Err(full())
        } else {
            Ok(())
        }
    }

    /// The tab whose document is the file at `path` (that file through any name),
    /// if one is open. A tab other than `except` only.
    pub(crate) fn owner_of(
        &self,
        active: &Sessions,
        path: &Path,
        except: Option<TabId>,
    ) -> Option<TabId> {
        self.all(active)
            .filter(|sessions| Some(sessions.tab()) != except)
            .find(|sessions| {
                sessions
                    .logical_path()
                    .is_some_and(|own| ferritecad_jobs::names_same_file(own, path))
            })
            .map(Sessions::tab)
    }

    /// The open tab the row's `key` names.
    pub(crate) fn by_key(&self, key: u64) -> Option<TabId> {
        self.order.iter().copied().find(|tab| tab.key() == key)
    }

    /// What opening `path` does: a file that a tab names already (through any
    /// name) is shown in that tab rather than read a second time as a competing
    /// writer; a different physical copy, even of the same document, is a new tab.
    pub(crate) fn opening(&self, active: &Sessions, path: &Path) -> Opening {
        match self.owner_of(active, path, None) {
            Some(tab) if active.has_session() && tab == active.tab() => Opening::Shown,
            Some(tab) => Opening::Show(tab),
            None => match self.room() {
                Ok(()) => Opening::Load,
                Err(reason) => Opening::Refused(reason),
            },
        }
    }

    /// Why the shown tab may not be saved as `path`: another tab's file (by any
    /// name, on disk or not) would have two writers. `None` when nothing here
    /// objects; the save's own no-clobber and version guards still apply.
    pub(crate) fn save_as_refusal(&self, active: &Sessions, path: &Path) -> Option<String> {
        self.owner_of(active, path, Some(active.tab())).map(|_| {
            format!(
                "Not saved: {} is open in another tab. Save it there, or choose another name.",
                path.file_name().unwrap_or_default().to_string_lossy()
            )
        })
    }

    /// The row: every tab's name and mark, from its own session, in order.
    pub(crate) fn labels(&self, active: &Sessions) -> Vec<(TabId, String, bool, bool)> {
        self.order
            .iter()
            .filter_map(|tab| {
                let sessions = if active.has_session() && active.tab() == *tab {
                    active
                } else {
                    &self.hidden(*tab)?.sessions
                };
                Some((
                    *tab,
                    sessions.name().unwrap_or_default(),
                    sessions.dirty(),
                    sessions.tab() == active.tab(),
                ))
            })
            .collect()
    }

    /// Binds an arriving picture's document change. Called by the window between
    /// preparing the picture and replacing the shown one, with the camera and the
    /// typed checkpoint name of the tab being left (or of nothing, in an empty
    /// window). An error leaves every tab and the picture as they were.
    pub(crate) fn bind(
        &mut self,
        active: &mut Sessions,
        bind: Bind,
        camera: &ViewportInput,
        checkpoint_name: &mut String,
    ) -> Result<()> {
        match bind {
            Bind::Open(session) => self.open(active, *session, camera, checkpoint_name),
            Bind::Staged => active.commit_staged(),
            Bind::Switch(generation) => self.activate(active, generation, camera, checkpoint_name),
        }
    }

    /// A new tab for an accepted document (Open, New, Recover). The tab that was
    /// shown is kept as it is, hidden, with its camera.
    fn open(
        &mut self,
        active: &mut Sessions,
        session: DocumentSession,
        camera: &ViewportInput,
        checkpoint_name: &mut String,
    ) -> Result<()> {
        self.room().map_err(CadError::input)?;
        if let Some(path) = session.logical_path()
            && self.owner_of(active, path, None).is_some()
        {
            return Err(CadError::input(format!(
                "{} is already open in another tab; nothing was opened twice",
                session.display_name()
            )));
        }
        let mut fresh = self.fresh();
        fresh.adopt(session);
        let tab = fresh.tab();
        let previous = std::mem::replace(active, fresh);
        let at = match previous.has_session() {
            true => {
                let left = previous.tab();
                self.hidden.push(Hidden {
                    sessions: previous,
                    view: View {
                        camera: camera.clone(),
                        checkpoint_name: std::mem::take(checkpoint_name),
                    },
                });
                self.order
                    .iter()
                    .position(|other| *other == left)
                    .map_or(self.order.len(), |at| at + 1)
            }
            // An empty window's controller held nothing to keep.
            false => {
                checkpoint_name.clear();
                self.order.len()
            }
        };
        self.order.insert(at, tab);
        Ok(())
    }

    // --- showing another tab ---------------------------------------------------

    /// Starts showing hidden tab `target`: its current version is drawn on a worker
    /// (`spawn` is given the version's private file and the generation to answer
    /// with) while the shown tab holds the one operation slot. `after` is what
    /// follows once it is shown.
    pub(crate) fn begin_switch(
        &mut self,
        active: &mut Sessions,
        target: TabId,
        after: Option<After>,
        spawn: impl FnOnce(PathBuf, u64, &CancelToken) -> JoinHandle<()>,
    ) -> std::result::Result<u64, String> {
        let lease = self
            .hidden(target)
            .and_then(|hidden| hidden.sessions.export_source())
            .ok_or_else(|| "That document is not open in a hidden tab.".to_owned())?;
        self.issued += 1;
        let generation = self.issued;
        if !active.hold_switch(generation) {
            return Err("Wait for the current operation to finish.".to_owned());
        }
        // A switch given up on (Cancel) is still running: its answer is stale now.
        self.retire_switch();
        let cancel = CancelToken::new();
        let worker = spawn(lease.path().to_path_buf(), generation, &cancel);
        self.switch = Some(Switch {
            generation,
            target,
            cancel,
            worker: Some(worker),
            after,
            _lease: lease,
        });
        active.status = format!(
            "Showing {}…",
            self.hidden(target)
                .and_then(|hidden| hidden.sessions.name())
                .unwrap_or_default()
        );
        Ok(generation)
    }

    /// The version to draw when `generation` is the switch being waited for and
    /// its tab is still open; `None` for any other answer.
    pub(crate) fn switch_path(&self, generation: u64) -> Option<PathBuf> {
        let switch = self
            .switch
            .as_ref()
            .filter(|s| s.generation == generation)?;
        self.hidden(switch.target)?;
        Some(switch._lease.path().to_path_buf())
    }

    /// The camera the arriving picture of switch `generation` is shown with: the
    /// tab's own, fitted to the window's present size.
    pub(crate) fn view_for(
        &self,
        generation: u64,
        window: &ViewportInput,
    ) -> Option<ViewportInput> {
        let switch = self
            .switch
            .as_ref()
            .filter(|s| s.generation == generation)?;
        let mut camera = self.hidden(switch.target)?.view.camera.clone();
        camera.resize(window.camera().width(), window.camera().height());
        Some(camera)
    }

    /// The statement that makes the switched-to tab active: only for the switch
    /// still awaited, whose slot is still held (not cancelled). The tab being
    /// left is hidden with its camera and typed name.
    fn activate(
        &mut self,
        active: &mut Sessions,
        generation: u64,
        camera: &ViewportInput,
        checkpoint_name: &mut String,
    ) -> Result<()> {
        let target = self
            .switch
            .as_ref()
            .filter(|switch| switch.generation == generation)
            .map(|switch| switch.target)
            .ok_or_else(|| CadError::input("that tab is no longer being shown"))?;
        if self.hidden(target).is_none() {
            return Err(CadError::input("that tab was closed"));
        }
        if !active.finish_switch(generation) {
            return Err(CadError::Cancelled);
        }
        let Hidden { sessions, view } = self
            .take_hidden(target)
            .expect("the target was found hidden above");
        let previous = std::mem::replace(active, sessions);
        if previous.has_session() {
            self.hidden.push(Hidden {
                sessions: previous,
                view: View {
                    camera: camera.clone(),
                    checkpoint_name: std::mem::take(checkpoint_name),
                },
            });
        }
        *checkpoint_name = view.checkpoint_name;
        active.status.clear();
        Ok(())
    }

    /// The switch `generation` is over, shown or not; returns what was to follow
    /// it when it was shown. Not shown: the slot is released, the tab that was
    /// shown stays, and a Quit pass it belonged to stops.
    pub(crate) fn end_switch(
        &mut self,
        active: &mut Sessions,
        generation: u64,
        shown: &Result<()>,
    ) -> Option<After> {
        let mut switch = self
            .switch
            .take_if(|switch| switch.generation == generation)?;
        if let Some(worker) = switch.worker.take() {
            let _ = worker.join();
        }
        if let Err(error) = shown {
            active.finish_switch(generation);
            active.status = if error.kind() == ferritecad_types::ErrorKind::Cancellation {
                "Switching tabs cancelled; this document stays.".to_owned()
            } else {
                format!("The other document could not be shown; this one stays: {error}")
            };
            if switch.after == Some(After::Quit) {
                self.quitting = None;
            }
            return None;
        }
        switch.after
    }

    /// Cancel was pressed: the switch is given up on, its worker asked to stop.
    /// The slot itself is released by the shown tab's own Cancel.
    pub(crate) fn cancel_switch(&mut self) {
        if self
            .switch
            .as_ref()
            .is_some_and(|switch| switch.after == Some(After::Quit))
        {
            self.quitting = None;
        }
        // Cancel invalidates the address now. Otherwise a late answer could
        // prepare a discarded picture and overwrite a newer Apply's status.
        self.retire_switch();
    }

    fn retire_switch(&mut self) {
        if let Some(mut switch) = self.switch.take() {
            switch.cancel.cancel();
            if let Some(worker) = switch.worker.take() {
                self.retired.push((worker, switch._lease));
            }
        }
        let (done, running): (Vec<_>, Vec<_>) = std::mem::take(&mut self.retired)
            .into_iter()
            .partition(|(worker, _)| worker.is_finished());
        self.retired = running;
        for (worker, _lease) in done {
            let _ = worker.join();
        }
    }

    // --- closing ---------------------------------------------------------------

    /// What closing `tab` needs first.
    pub(crate) fn close_step(&self, active: &Sessions, tab: TabId) -> Option<CloseStep> {
        if active.has_session() && active.tab() == tab {
            return Some(if active.dirty() {
                CloseStep::Ask
            } else {
                CloseStep::Now
            });
        }
        let hidden = self.hidden(tab)?;
        Some(if hidden.sessions.dirty() {
            CloseStep::Show
        } else {
            CloseStep::Now
        })
    }

    /// Closes `tab`: its controller and session go, with their private files, and
    /// its crash copy is retired (the person decided: it was clean, saved, or
    /// answered Discard). Closing the shown tab leaves `active` empty; the window
    /// replaces the picture in the same statement. Returns the tab to show next,
    /// when the shown one was closed and others remain.
    pub(crate) fn close(&mut self, active: &mut Sessions, tab: TabId) -> Result<Option<TabId>> {
        let position = self
            .order
            .iter()
            .position(|other| *other == tab)
            .ok_or_else(|| CadError::input("that tab is not open"))?;
        let mut closed = if active.has_session() && active.tab() == tab {
            if active.busy() {
                return Err(CadError::input("wait for the current operation to finish"));
            }
            std::mem::take(active)
        } else {
            self.take_hidden(tab)
                .ok_or_else(|| CadError::input("that tab is not open"))?
                .sessions
        };
        self.order.remove(position);
        closed.decide_exit();
        drop(closed);
        if active.has_session() {
            return Ok(None);
        }
        // The neighbour that took the closed tab's place, or the one before it.
        Ok(self
            .order
            .get(position)
            .or_else(|| self.order.last())
            .copied())
    }

    // --- Quit ------------------------------------------------------------------

    pub(crate) fn begin_quit(&mut self) {
        self.quitting = Some(Vec::new());
    }

    pub(crate) fn quitting(&self) -> bool {
        self.quitting.is_some()
    }

    /// Cancel, a failed or cancelled Save, or a tab that could not be shown: the
    /// pass stops. Nothing it did is undone (saved files stay saved), and nothing
    /// it was told is acted on (tabs answered Discard stay open and unsaved).
    pub(crate) fn abort_quit(&mut self) {
        self.quitting = None;
    }

    /// The shown tab was answered Discard in this pass.
    pub(crate) fn discarded(&mut self, tab: TabId) {
        if let Some(answered) = &mut self.quitting {
            answered.push(tab);
        }
    }

    /// The next thing the Quit pass does: the shown tab first, then each unsaved
    /// hidden tab in the row's order. Saved tabs are clean and need nothing.
    pub(crate) fn quit_step(&self, active: &Sessions) -> QuitStep {
        let answered = self.quitting.as_deref().unwrap_or_default();
        if active.has_session() && active.dirty() && !answered.contains(&active.tab()) {
            return QuitStep::Ask;
        }
        self.order
            .iter()
            .filter(|tab| !answered.contains(tab))
            .find(|tab| self.hidden(**tab).is_some_and(|h| h.sessions.dirty()))
            .map_or(QuitStep::Exit, |tab| QuitStep::Show(*tab))
    }

    /// The window ends by the person's decision: every tab's crash copy goes.
    pub(crate) fn decide_exit(&mut self, active: &mut Sessions) {
        active.decide_exit();
        for hidden in &mut self.hidden {
            hidden.sessions.decide_exit();
        }
    }

    /// Stops and joins everything: the switch, every hidden tab (each ends its own
    /// crash copy as decided), and last the recovery worker once no lane is left.
    pub(crate) fn stop_all(&mut self) {
        self.retire_switch();
        for (worker, _lease) in self.retired.drain(..) {
            let _ = worker.join();
        }
        for mut hidden in self.hidden.drain(..) {
            hidden.sessions.stop_all();
        }
        self.order.clear();
        if let Some(recorder) = self.recorder.take() {
            recorder.finish(ferritecad_jobs::Ending::Keep);
        }
    }

    #[cfg(test)]
    pub(crate) fn hidden_sessions(&self, tab: TabId) -> Option<&Sessions> {
        self.hidden(tab).map(|hidden| &hidden.sessions)
    }

    #[cfg(test)]
    pub(crate) fn order(&self) -> &[TabId] {
        &self.order
    }
}
