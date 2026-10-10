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
//!
//! §30P: so are the shown tab's edit forms — the window's `Edits` height form and
//! its `sketch::Editor` with every other saved-object form. A hidden tab keeps
//! them as a [`Draft`]: the same values, moved, never copied or re-read, in the
//! statement that hides the tab, and given back in the one that shows it again.
//!
//! §30Q: New is opened over the shown tab without closing its forms: they are set
//! aside as that tab's draft (the same [`Draft`], the same move) while New uses the
//! window's forms, and come back when New ends without a new tab — or stay with
//! their tab, hidden, when the new document is accepted.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

use ferritecad_jobs::{DocumentSession, RecoveryRecorder, Snapshot};
use ferritecad_kernel::CancelToken;
use ferritecad_types::{CadError, Result};
use ferritecad_ui::ViewportInput;

use crate::edits::{Edits, Form};
use crate::last_tabs::LastTabs;
use crate::sessions::{Bind, Sessions};
use crate::sketch::Editor;

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
    /// §30P: the forms that were open when the tab was hidden; `None` when none was.
    draft: Option<Draft>,
}

/// §30P: the edit forms of a hidden tab, exactly as they were left: typed text,
/// picked ids, pending removals and additions, each form's own Undo and Redo.
/// Only idle forms: a worker never leaves the window, and nothing here runs.
pub(crate) struct Draft {
    height: Option<Form>,
    editor: Box<Editor>,
    /// The accepted version the forms describe: the tab's current one when it was
    /// hidden. Its identity, not its content: two copies of one file, or one
    /// file's two equal versions, are not the same version.
    base: Arc<Snapshot>,
}

/// §30P: the shown tab's forms as the window holds them, lent to the statements
/// that hide and show a tab.
pub(crate) struct Forms<'a> {
    pub(crate) edits: &'a mut Edits,
    pub(crate) editor: &'a mut Editor,
}

impl Forms<'_> {
    /// Takes the open forms of the tab `of` is the controller of, leaving none
    /// shown. `None` (and nothing taken) when no form about its document is open.
    fn park(&mut self, of: &Sessions) -> Option<Draft> {
        if !(self.edits.form_open() || self.editor.saved_object_open()) {
            return None;
        }
        let base = of.export_source()?;
        Some(Draft {
            height: self.edits.take_form(),
            editor: Box::new(std::mem::take(self.editor)),
            base,
        })
    }

    /// Gives `draft` back to the window, as it was left. A draft made on another
    /// version than `to`'s current one comes back held: readable, cancellable,
    /// never applied (`Sessions::hold_stale_draft`).
    fn restore(&mut self, draft: Draft, to: &mut Sessions) {
        let Draft {
            height,
            editor,
            base,
        } = draft;
        if !to
            .export_source()
            .is_some_and(|current| Arc::ptr_eq(&current, &base))
        {
            to.hold_stale_draft();
        }
        self.edits.restore_form(height);
        *self.editor = *editor;
    }
}

/// One Close attempt: runtime tab identity and a never-reused question number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FormCloseId {
    tab: TabId,
    generation: u64,
}

impl FormCloseId {
    pub(crate) fn tab(self) -> TabId {
        self.tab
    }
}

struct FormClose {
    id: FormCloseId,
    draft: Draft,
    confirmed: bool,
    model_decided: bool,
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
    /// Unsaved, or keeping an open form (§30P), and hidden: show it first.
    Show,
    /// Unsaved and shown: ask Save / Discard / Cancel.
    Ask,
    /// §30U: shown with a form open: ask before continuing Close.
    /// Nothing is closed, saved or applied by requesting it.
    Form,
}

/// The next thing a Quit pass does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitStep {
    /// §30P: the shown tab has a form open: the pass stops there, nothing closed.
    Form,
    /// The shown tab is unsaved and has not been answered: ask about it.
    Ask,
    /// This hidden tab is unsaved, or keeps an open form, and has not been
    /// answered: show it first.
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
    /// §30Q: the shown tab's forms, set aside while New uses the window's forms.
    aside: Option<(TabId, Draft)>,
    /// §30U: the original form, held until actual Close or returned on failure.
    form_close: Option<FormClose>,
    close_issued: u64,
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
            aside: None,
            form_close: None,
            close_issued: 0,
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

    /// §30P: whether hidden `tab` keeps an open form.
    #[cfg(test)]
    pub(crate) fn has_draft(&self, tab: TabId) -> bool {
        self.hidden(tab)
            .is_some_and(|hidden| hidden.draft.is_some())
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
    /// preparing the picture and replacing the shown one, with the camera, the
    /// typed checkpoint name and the forms of the tab being left (or of nothing,
    /// in an empty window). An error leaves every tab, form and the picture as
    /// they were.
    pub(crate) fn bind(
        &mut self,
        active: &mut Sessions,
        bind: Bind,
        camera: &ViewportInput,
        checkpoint_name: &mut String,
        forms: &mut Forms<'_>,
    ) -> Result<()> {
        if self.closing_form() {
            return Err(CadError::input(
                "Answer the unfinished-form Close question first.",
            ));
        }
        match bind {
            Bind::Open(session) => self.open(active, *session, camera, checkpoint_name, forms),
            Bind::Staged => active.commit_staged(),
            Bind::Switch(generation) => {
                self.activate(active, generation, camera, checkpoint_name, forms)
            }
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
        forms: &mut Forms<'_>,
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
                // Forms about the document left stay with it, never with the new one.
                let draft = self.leaving(&previous, forms);
                self.hidden.push(Hidden {
                    sessions: previous,
                    view: View {
                        camera: camera.clone(),
                        checkpoint_name: std::mem::take(checkpoint_name),
                    },
                    draft,
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

    /// The forms that go with tab `left` as it is hidden: the ones New set aside
    /// for it (§30Q), or else the ones the window shows.
    fn leaving(&mut self, left: &Sessions, forms: &mut Forms<'_>) -> Option<Draft> {
        match self.aside.take() {
            Some((tab, draft)) if tab == left.tab() => Some(draft),
            aside => {
                self.aside = aside;
                forms.park(left)
            }
        }
    }

    // --- New over a tab's forms (§30Q) -----------------------------------------

    /// New is opening over the shown tab: its open forms are set aside as its
    /// draft — moved, exactly as left, as a switch would — so New starts on empty
    /// forms and nothing typed for New reaches them. Nothing when none is open.
    pub(crate) fn set_aside(&mut self, active: &Sessions, forms: &mut Forms<'_>) {
        if self.closing_form() || self.aside.is_some() || !active.has_session() {
            return;
        }
        if let Some(draft) = forms.park(active) {
            self.aside = Some((active.tab(), draft));
        }
    }

    /// New ended without a new tab (Cancel, or nothing it made was shown): the
    /// forms set aside come back to the shown tab as they were left. Never over a
    /// form the window shows: then they stay aside.
    pub(crate) fn bring_back(&mut self, active: &mut Sessions, forms: &mut Forms<'_>) {
        if forms.edits.form_open() || forms.editor.active() {
            return;
        }
        if let Some((_, draft)) = self
            .aside
            .take_if(|(tab, _)| active.has_session() && *tab == active.tab())
        {
            forms.restore(draft, active);
        }
    }

    /// §30Q: whether the shown tab's forms are set aside for New.
    #[cfg(test)]
    pub(crate) fn has_aside(&self) -> bool {
        self.aside.is_some()
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
        if self.closing_form() {
            return Err("Answer the unfinished-form Close question first.".to_owned());
        }
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
    /// left is hidden with its camera, typed name and open forms; the shown one's
    /// forms come back as they were left (§30P).
    fn activate(
        &mut self,
        active: &mut Sessions,
        generation: u64,
        camera: &ViewportInput,
        checkpoint_name: &mut String,
        forms: &mut Forms<'_>,
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
        let Hidden {
            sessions,
            view,
            draft,
        } = self
            .take_hidden(target)
            .expect("the target was found hidden above");
        let left = self.leaving(active, forms);
        let previous = std::mem::replace(active, sessions);
        if previous.has_session() {
            self.hidden.push(Hidden {
                sessions: previous,
                view: View {
                    camera: camera.clone(),
                    checkpoint_name: std::mem::take(checkpoint_name),
                },
                draft: left,
            });
        }
        *checkpoint_name = view.checkpoint_name;
        active.status.clear();
        if let Some(draft) = draft {
            forms.restore(draft, active);
        }
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

    /// §30U: freeze one whole form using the existing move, never a copy. Call
    /// only after the window's foreground-work and gesture checks, on a shown tab.
    pub(crate) fn ask_form_close(
        &mut self,
        active: &Sessions,
        forms: &mut Forms<'_>,
    ) -> Option<FormCloseId> {
        if self.closing_form() || self.aside.is_some() || active.busy() || !active.has_session() {
            return None;
        }
        let draft = forms.park(active)?;
        self.close_issued += 1;
        let id = FormCloseId {
            tab: active.tab(),
            generation: self.close_issued,
        };
        self.form_close = Some(FormClose {
            id,
            draft,
            confirmed: false,
            model_decided: false,
        });
        Some(id)
    }

    pub(crate) fn closing_form(&self) -> bool {
        self.form_close.is_some()
    }

    pub(crate) fn form_close_id(&self) -> Option<FormCloseId> {
        self.form_close.as_ref().map(|close| close.id)
    }

    fn addressed_close(&self, active: &Sessions, id: FormCloseId) -> Option<&FormClose> {
        self.form_close.as_ref().filter(|close| {
            close.id == id
                && active.has_session()
                && active.tab() == id.tab
                && active
                    .export_source()
                    .is_some_and(|base| Arc::ptr_eq(&base, &close.draft.base))
        })
    }

    /// The question and its address are borrowed for this frame only. Once
    /// confirmed it is no longer answerable, including by a repeated old click.
    pub(crate) fn form_close_question(&self, active: &Sessions) -> Option<(FormCloseId, String)> {
        let id = self.form_close.as_ref()?.id;
        self.addressed_close(active, id)
            .filter(|close| !close.confirmed)?;
        Some((id, active.name()?))
    }

    pub(crate) fn confirm_form_close(&mut self, active: &Sessions, id: FormCloseId) -> bool {
        if active.busy()
            || self
                .addressed_close(active, id)
                .is_none_or(|close| close.confirmed)
        {
            return false;
        }
        self.form_close.as_mut().expect("address checked").confirmed = true;
        true
    }

    /// Called only after the existing model question says Go (clean or explicit
    /// Discard), or after a published Save continuation verifies the model is clean.
    pub(crate) fn decide_form_close(&mut self, active: &Sessions, id: FormCloseId) -> bool {
        if active.busy()
            || !self
                .addressed_close(active, id)
                .is_some_and(|close| close.confirmed && !close.model_decided)
        {
            return false;
        }
        self.form_close
            .as_mut()
            .expect("address checked")
            .model_decided = true;
        true
    }

    /// Back, a cancelled dialog, failed Save or refused actual Close: return the
    /// very same form. A foreign/late response never consumes another attempt.
    /// Never overwrite a newer form; the held one stays owned here on refusal.
    pub(crate) fn cancel_form_close(
        &mut self,
        active: &mut Sessions,
        id: FormCloseId,
        forms: &mut Forms<'_>,
    ) -> bool {
        if active.busy()
            || active.tab() != id.tab
            || forms.edits.form_open()
            || forms.editor.active()
            || !self.form_close.as_ref().is_some_and(|close| close.id == id)
        {
            return false;
        }
        let close = self.form_close.take().expect("address checked");
        forms.restore(close.draft, active);
        true
    }

    /// What closing `tab` needs first. `form_open`: the shown tab has a form open.
    /// A form is never closed behind the person's back, even over a clean model.
    pub(crate) fn close_step(
        &self,
        active: &Sessions,
        tab: TabId,
        form_open: bool,
    ) -> Option<CloseStep> {
        if active.has_session() && active.tab() == tab {
            // §30Q: forms set aside for New are the shown tab's open forms too.
            return Some(if form_open || self.aside.is_some() {
                CloseStep::Form
            } else if active.dirty() {
                CloseStep::Ask
            } else {
                CloseStep::Now
            });
        }
        let hidden = self.hidden(tab)?;
        Some(if hidden.sessions.dirty() || hidden.draft.is_some() {
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
        if let Some(close) = &self.form_close
            && !self
                .addressed_close(active, close.id)
                .is_some_and(|asked| asked.id.tab == tab && asked.confirmed && asked.model_decided)
        {
            return Err(CadError::input(
                "this Close attempt has not decided the form and model",
            ));
        }
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
        // Only now is closing irreversible. Release the draft lease before the
        // session removes its private snapshots. Every refusal above kept it.
        self.form_close = None;
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

    /// The next thing the Quit pass does: the shown tab first, then each hidden
    /// tab that is unsaved or keeps a form (§30P), in the row's order. Saved tabs
    /// without a form need nothing. `form_open`: the shown tab has a form open.
    pub(crate) fn quit_step(&self, active: &Sessions, form_open: bool) -> QuitStep {
        let answered = self.quitting.as_deref().unwrap_or_default();
        // §30Q: forms set aside for New are the shown tab's open forms too.
        if active.has_session() && (form_open || self.aside.is_some()) {
            return QuitStep::Form;
        }
        if active.has_session() && active.dirty() && !answered.contains(&active.tab()) {
            return QuitStep::Ask;
        }
        self.order
            .iter()
            .filter(|tab| !answered.contains(tab))
            .find(|tab| {
                self.hidden(**tab)
                    .is_some_and(|h| h.sessions.dirty() || h.draft.is_some())
            })
            .map_or(QuitStep::Exit, |tab| QuitStep::Show(*tab))
    }

    /// §30R: the saved files of the open tabs, in the row's order, each its
    /// session's logical path (a tab answered Discard is its saved file; an
    /// Untitled tab has none and is left out), and which of them is shown.
    pub(crate) fn saved_set(&self, active: &Sessions) -> LastTabs {
        let mut set = LastTabs::default();
        for tab in &self.order {
            let shown = active.has_session() && active.tab() == *tab;
            let sessions = match self.hidden(*tab) {
                _ if shown => active,
                Some(hidden) => &hidden.sessions,
                None => continue,
            };
            if let Some(path) = sessions.logical_path() {
                if shown {
                    set.active = Some(set.paths.len());
                }
                set.paths.push(path.to_path_buf());
            }
        }
        set
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

    /// A hidden tab's controller, to change it behind the window's back (§30P:
    /// what no window route can do while a tab is hidden).
    #[cfg(test)]
    pub(crate) fn hidden_sessions_mut(&mut self, tab: TabId) -> Option<&mut Sessions> {
        self.hidden
            .iter_mut()
            .find(|hidden| hidden.sessions.tab() == tab)
            .map(|hidden| &mut hidden.sessions)
    }

    #[cfg(test)]
    pub(crate) fn order(&self) -> &[TabId] {
        &self.order
    }
}
