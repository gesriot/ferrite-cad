// SPDX-License-Identifier: MIT
//! §30R: the last window's saved files, kept at the end of a Quit and reopened at
//! the next start. Through the window's own owners — the Quit pass (`Tabs`,
//! `begin_window_quit`, `end_window_quit`), each tab's `Sessions`, the descriptor's
//! `last_tabs::Folder`, `Restores` and its `step`, the reading's `Loads`, the
//! arrival statement `present` and the predicates the buttons and handlers share —
//! without a graphics device.

use super::*;
use crate::exports::Exports;
use crate::last_tabs::{Folder, LastTabs};
use crate::recoveries::Recoveries;
use crate::restores::{Outcome, Restores, Step};
use crate::{LoadGeneration, Loads, ProgressRelay, QuitEnd};
use ferritecad_ui::{NewChoice, PointerButton, ViewportEvent};

impl Window {
    /// `App::start_reading`: a reading of `path`, numbered by the window's `Loads`.
    fn start(loads: &mut Loads, path: &Path) -> LoadGeneration {
        loads
            .open(Some(path), Arc::new(ProgressRelay::default()), |_, _| {
                std::thread::spawn(|| {})
            })
            .expect("a reading starts")
    }

    /// What the reading worker answers: on the mock kernel, or the production
    /// `open_for_view` (Open CASCADE, or its refusal in a build without it).
    fn reading(&self, path: &Path) -> Result<(LoadedScene, DocumentSession)> {
        match self.drawn {
            Drawn::Native => open_for_view(self.private.path(), path, &OperationContext::default()),
            Drawn::Mock => {
                let session =
                    DocumentSession::open_in(self.private.path(), path, HistoryLimits::default())?;
                let scene = self.picture(session.current().path())?;
                Ok((scene, session))
            }
        }
    }

    /// The window's `Loaded` handler without the device: the answer is shown only
    /// when `loads` waits for it, then the Reopen is told about its own reading.
    /// Whether the Reopen goes on.
    fn deliver_reopen(
        &mut self,
        loads: &mut Loads,
        restores: &mut Restores,
        generation: LoadGeneration,
        read: Result<(LoadedScene, DocumentSession)>,
        upload: Result<()>,
    ) -> bool {
        let awaited = loads.accepted_path(generation).is_some();
        let outcome = match (awaited, read) {
            (true, Ok((scene, session))) => {
                let shown = session.current().path().to_path_buf();
                let outcome =
                    self.present(&shown, Ok(scene), Bind::Open(Box::new(session)), upload);
                self.creates
                    .sketch
                    .draft_load_finished(&shown, outcome.is_ok());
                self.edits.draft_load_finished(&shown, outcome.is_ok());
                outcome
            }
            (true, Err(error)) => Err(error),
            (false, read) => read.map(|_| ()),
        };
        let shown = awaited.then(|| outcome.as_ref().map(|_| ()).map_err(ToString::to_string));
        crate::finish_answer(loads, &mut self.input, generation, outcome);
        restores.answered(generation, shown)
    }

    /// `can_restore` for this window, with `loads` the window's readings.
    fn can_reopen(&self, loads: &Loads, restores: &Restores) -> bool {
        crate::can_restore(
            &self.creates,
            loads,
            &Exports::default(),
            &self.edits,
            &self.sessions,
            &self.input,
            &Recoveries::default(),
            restores,
        )
    }

    /// `can_open` for this window: the toolbar's Open and its handler.
    fn can_open_now(&self, restores: &Restores) -> bool {
        crate::can_open(
            &self.creates,
            &self.edits,
            &self.sessions,
            &self.input,
            restores,
        )
    }

    /// `App::restore` and every answer to its end: each file read (or refused by
    /// `upload` for its picture), then the tab shown last time made shown. The tab
    /// shown at the end, if a switch was asked for.
    fn reopen(
        &mut self,
        restores: &mut Restores,
        upload: impl Fn(&Path) -> Result<()>,
    ) -> Option<TabId> {
        let mut loads = Loads::default();
        assert!(self.can_reopen(&loads, restores), "Reopen is offered");
        assert!(restores.begin());
        loop {
            match crate::restores::step(restores, &self.tabs, &self.sessions) {
                Step::Read(path) => {
                    let generation = Self::start(&mut loads, &path);
                    assert!(restores.reading(Some(generation)));
                    // While a file is read, nothing else may start.
                    assert!(!self.can_open_now(restores), "Open waits");
                    assert!(!self.can_reopen(&loads, restores), "Reopen waits");
                    assert!(
                        !crate::can_leave_tab(
                            &self.creates,
                            &loads,
                            &Exports::default(),
                            &self.edits,
                            &self.sessions,
                            &self.input,
                        ),
                        "New, Recover and the tab row wait"
                    );
                    let read = self.reading(&path);
                    self.deliver_reopen(&mut loads, restores, generation, read, upload(&path));
                }
                Step::Show(tab) => {
                    self.switch(tab);
                    return Some(tab);
                }
                Step::Wait => return None,
            }
        }
    }

    /// `end_window_quit` with this window's own form state.
    pub(super) fn end_quit(&mut self, restores: &mut Restores) -> QuitEnd {
        let form = crate::form_open(&self.edits, &self.creates.sketch);
        crate::end_window_quit(&mut self.tabs, &mut self.sessions, form, restores)
    }

    /// `begin_window_quit` with nothing else running.
    pub(super) fn begin_quit(&mut self) -> bool {
        crate::begin_window_quit(
            &mut self.tabs,
            &mut self.sessions,
            &self.creates,
            &Loads::default(),
            &Exports::default(),
            &self.edits,
            &self.input,
        )
    }

    /// Shows hidden `tab` for the Quit pass (`After::Quit`), as the window does.
    pub(super) fn show_for_quit(&mut self, tab: TabId) {
        let (generation, rx) = self.begin_switch(tab, Some(After::Quit));
        let (shown, after) = self
            .deliver_switch(generation, wait(&rx), Ok(()))
            .expect("awaited");
        shown.expect("shown");
        assert_eq!(after, Some(After::Quit));
    }

    /// The height form of the shown tab, opened and typed into, not applied.
    fn type_height(&mut self, text: &str) {
        let current = self.sessions.export_path().expect("shown");
        let reading = read_extrude_source(&current).expect("reading");
        assert!(self.edits.begin(&current, &reading));
        self.edits.type_height(reading.features[0].feature, text);
    }
}

fn height_in(sessions: &Sessions) -> f64 {
    height_of(&sessions.export_path().expect("accepted"))
}

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("bytes")
}

/// The window's Reopen offer for what `folder` holds, as read at start.
fn offered(folder: &Folder) -> Restores {
    let mut restores = Restores::new(Ok(folder.clone()));
    restores.listed(folder.read());
    restores
}

fn logical(w: &Window, tab: TabId) -> PathBuf {
    let sessions = if w.sessions.tab() == tab {
        &w.sessions
    } else {
        w.hidden(tab)
    };
    sessions.logical_path().expect("named").to_path_buf()
}

/// Quit keeps the list of saved files only when the window really ends: a
/// running worker, a form open on the shown tab or kept by a hidden one, Cancel,
/// a refused Save As and New's set-aside forms each stop it and leave the earlier
/// window's list byte for byte — also when the window's quit end is reached late.
/// At the end: the row's order, a named tab answered Discard as its saved file,
/// an Untitled saved during the pass under its new path, an Untitled discarded and
/// a tab closed before Quit left out, the shown tab marked. The next start offers
/// exactly that, and Reopen reads the files as saved (never the discarded model).
#[test]
fn quit_keeps_the_saved_files_only_when_the_window_really_ends() {
    let user = tempfile::tempdir().expect("user");
    let [a_file, b_file, c_file] = ["a.fcad", "b.fcad", "c.fcad"].map(|n| user.path().join(n));
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    plate(&c_file, 18.0);
    let originals = [&a_file, &b_file, &c_file].map(|f| bytes(f));
    let state = tempfile::tempdir().expect("state");
    let folder = Folder::at(&state.path().join("tabs")).expect("folder");
    // What an earlier window left.
    let earlier = LastTabs {
        paths: vec![user.path().join("earlier.fcad")],
        active: Some(0),
    };
    folder.publish(&earlier).expect("an earlier list");
    let kept = bytes(&folder.descriptor());
    let mut restores = Restores::new(Ok(folder.clone()));
    let unchanged = |why: &str| assert_eq!(bytes(&folder.descriptor()), kept, "{why}");

    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.open(&b_file);
    let b = w.sessions.tab();
    w.open(&c_file);
    let c = w.sessions.tab();

    // Every tab clean, B keeps a form: the pass shows B and stops there. Once the
    // form is closed nothing is left to ask — yet the Quit was given up, and a
    // late end of the window publishes nothing and decides nothing.
    w.switch(b);
    w.type_height("19");
    w.switch(c);
    assert!(w.begin_quit());
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Show(b));
    w.show_for_quit(b);
    assert_eq!(w.tabs.quit_step(&w.sessions, true), QuitStep::Form);
    w.tabs.abort_quit();
    w.edits.cancel();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Exit);
    assert_eq!(w.end_quit(&mut restores), QuitEnd::NotEnded);
    unchanged("a Quit given up at a form, the form since closed");
    assert_eq!(w.tabs.order(), [a, b, c]);

    w.switch(a);
    w.apply(14.0);
    w.switch(c);
    w.create(NewDocument::Empty).expect("Untitled");
    let u1 = w.sessions.tab();
    w.create(NewDocument::Empty).expect("Untitled");
    let u2 = w.sessions.tab();
    // C closed before Quit: a clean hidden tab closes at once.
    assert_eq!(w.close(c).expect("closed"), None);
    assert_eq!(w.tabs.order(), [a, b, u1, u2]);
    w.switch(a);

    // A running worker holds Quit: nothing is published, even by a late end.
    let running = w
        .sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("an Apply");
    assert!(!w.begin_quit());
    assert_eq!(w.end_quit(&mut restores), QuitEnd::NotEnded);
    unchanged("a running worker");
    assert_eq!(
        w.sessions.finish_apply(running, Err(CadError::Cancelled)),
        Edited::Failed
    );

    // A saved-object form now offers a Quit decision (§30V).
    w.type_height("2..6");
    assert!(w.begin_quit());
    w.tabs.abort_quit();
    assert_eq!(w.end_quit(&mut restores), QuitEnd::NotEnded);
    unchanged("an open form");
    w.edits.cancel();

    // A hidden tab's form: the pass shows it and stops there, closing nothing.
    w.switch(b);
    w.type_height("19");
    w.switch(a);
    assert!(w.tabs.has_draft(b));
    assert!(w.begin_quit());
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    w.decide_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Show(b));
    w.show_for_quit(b);
    assert_eq!(w.tabs.quit_step(&w.sessions, true), QuitStep::Form);
    w.tabs.abort_quit();
    assert_eq!(w.end_quit(&mut restores), QuitEnd::NotEnded);
    unchanged("a hidden tab's form");
    assert_eq!(w.tabs.order(), [a, b, u1, u2]);
    w.edits.cancel();

    // Cancel at a tab: the pass stops; a late end of the window publishes nothing.
    w.switch(a);
    assert!(w.begin_quit());
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Cancel)),
        Replace::Stay
    );
    w.tabs.abort_quit();
    assert_eq!(w.end_quit(&mut restores), QuitEnd::NotEnded);
    unchanged("Cancel");

    // A Save As refused (an occupied name): the pass stops.
    w.switch(u1);
    assert!(w.begin_quit());
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
    let occupied = user.path().join("occupied.fcad");
    std::fs::write(&occupied, b"theirs").expect("occupied");
    assert!(!w.save(SaveTarget::As(occupied.clone())).published);
    w.tabs.abort_quit();
    assert_eq!(w.end_quit(&mut restores), QuitEnd::NotEnded);
    unchanged("a refused Save As");
    assert_eq!(bytes(&occupied), b"theirs");

    // New over a form sets it aside: Quit waits for New.
    w.switch(a);
    w.type_height("3");
    assert!(crate::open_new(
        &mut w.tabs,
        &mut w.sessions,
        &mut w.edits,
        &mut w.creates,
        &mut w.input,
        |creates, input| crate::ask_new(creates, &Loads::default(), &Exports::default(), input),
    ));
    assert!(w.tabs.has_aside());
    assert!(!w.begin_quit());
    assert_eq!(w.end_quit(&mut restores), QuitEnd::NotEnded);
    unchanged("New's set-aside forms");
    crate::creates::answer_form(&mut w.creates, &mut w.input, NewChoice::Cancel);
    crate::end_new(&mut w.tabs, &mut w.sessions, &mut w.edits, &mut w.creates);
    assert!(!w.tabs.has_aside());
    w.edits.cancel();

    // The pass to its end, from U2: Discard U2 (Untitled), Discard A (named, its
    // file as saved), Save As U1 during the pass. B is clean.
    w.switch(u2);
    assert!(w.begin_quit());
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    w.decide_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Show(a));
    w.show_for_quit(a);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    w.decide_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Show(u1));
    w.show_for_quit(u1);
    let u1_file = user.path().join("u1.fcad");
    let quit_id = w.tabs.ask_quit_model(&w.sessions).expect("model question");
    let (tx, rx) = mpsc::channel();
    let generation = w
        .sessions
        .begin_save(
            SaveTarget::As(u1_file.clone()),
            Some(Continuation::Quit(quit_id)),
            |plan, _, cancel| spawn_save(plan, cancel.clone(), move |v| tx.send(v).expect("save")),
        )
        .expect("a save");
    let report = w
        .sessions
        .finish_save(generation, wait(&rx))
        .expect("answered");
    assert_eq!(report.continuation, Some(Continuation::Quit(quit_id)));
    assert!(w.tabs.decide_quit_model(&w.sessions, quit_id));
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Exit);
    unchanged("nothing is published before the end");
    assert_eq!(w.end_quit(&mut restores), QuitEnd::Exit);
    let published = folder.read().expect("read").expect("a list");
    assert_eq!(
        published,
        LastTabs {
            paths: vec![a_file.clone(), b_file.clone(), u1_file.clone()],
            active: Some(2),
        }
    );
    assert_eq!(logical(&w, a), a_file);
    // The end of the window publishes once: a repeated end changes nothing.
    w.tabs.abort_quit();
    let after = bytes(&folder.descriptor());
    assert_eq!(w.end_quit(&mut restores), QuitEnd::NotEnded);
    assert_eq!(bytes(&folder.descriptor()), after);
    assert_eq!(
        [&a_file, &b_file, &c_file].map(|f| bytes(f)),
        originals,
        "Discard wrote nothing"
    );
    w.sessions.stop_all();
    w.tabs.stop_all();

    // The next start: the offer is that list; Reopen reads each file as saved.
    let mut next = offered(&folder);
    let rows = next.rows();
    assert_eq!(
        rows.iter()
            .map(|(name, _, shown)| (name.as_str(), *shown))
            .collect::<Vec<_>>(),
        [("a.fcad", false), ("b.fcad", false), ("u1.fcad", true)]
    );
    let mut w2 = Window::new(Drawn::Mock, None);
    assert_eq!(
        w2.reopen(&mut next, |_| Ok(())),
        None,
        "the shown file was read last"
    );
    let order = w2.tabs.order().to_vec();
    assert_eq!(order.len(), 3);
    assert_eq!(
        order
            .iter()
            .map(|tab| logical(&w2, *tab))
            .collect::<Vec<_>>(),
        [a_file.clone(), b_file.clone(), u1_file.clone()]
    );
    assert_eq!(w2.sessions.tab(), order[2]);
    w2.switch(order[0]);
    assert_eq!(
        height_in(&w2.sessions),
        12.0,
        "the saved file, not the discarded 14"
    );
    assert!(!w2.sessions.dirty());
    assert_eq!(
        next.report()[0],
        "3 of 3 saved files are open, each as it is saved on disk now."
    );
    assert_eq!(
        bytes(&folder.descriptor()),
        after,
        "reopening wrote no list"
    );
    println!("\nFCAD_30R_QUIT_PUBLICATION_EXECUTED");
}

/// A window that ends with no saved file publishes an empty list, which clears the
/// offer. A list that cannot be kept stops Quit once, in words, with nothing
/// closed or decided; the next Quit ends the window anyway.
#[test]
fn an_empty_end_clears_the_offer_and_a_list_that_cannot_be_kept_stops_quit_once() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    plate(&a_file, 12.0);
    let state = tempfile::tempdir().expect("state");
    let folder = Folder::at(&state.path().join("tabs")).expect("folder");
    folder
        .publish(&LastTabs {
            paths: vec![a_file.clone()],
            active: Some(0),
        })
        .expect("an earlier list");
    assert!(offered(&folder).can_begin(), "offered before");

    let mut restores = Restores::new(Ok(folder.clone()));
    let mut w = Window::new(Drawn::Mock, None);
    w.create(NewDocument::Empty).expect("Untitled");
    assert!(w.begin_quit());
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    w.decide_quit();
    assert_eq!(w.end_quit(&mut restores), QuitEnd::Exit);
    assert_eq!(folder.read().expect("read"), Some(LastTabs::default()));
    let next = offered(&folder);
    assert!(
        !next.can_begin() && next.rows().is_empty(),
        "nothing is offered"
    );
    w.sessions.stop_all();
    w.tabs.stop_all();

    // A folder that is a file: the list cannot be kept.
    let blocked = state.path().join("blocked");
    std::fs::write(&blocked, b"a file").expect("file");
    let mut restores = Restores::new(Folder::at(&blocked));
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(14.0);
    assert!(w.begin_quit());
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    w.decide_quit();
    let QuitEnd::Stay(reason) = w.end_quit(&mut restores) else {
        panic!("the first failure keeps the window");
    };
    assert!(
        reason.contains("could not be kept") && reason.contains("Quit again"),
        "{reason}"
    );
    assert!(!w.tabs.quitting(), "the pass stopped");
    assert!(
        w.sessions.dirty() && w.tabs.order() == [a],
        "nothing closed or decided"
    );
    assert_eq!(bytes(&blocked), b"a file");
    // Quit again: the same answers, and the window ends without the list.
    assert!(w.begin_quit());
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    w.decide_quit();
    assert_eq!(w.end_quit(&mut restores), QuitEnd::Exit);
    assert_eq!(bytes(&blocked), b"a file");
    assert_eq!(height_of(&a_file), 12.0);
    w.sessions.stop_all();
    w.tabs.stop_all();
    println!("\nFCAD_30R_EMPTY_AND_FAILED_LIST_EXECUTED");
}

/// Reopen beside an open tab and its unfinished form: the files in the list's
/// order, each through Open into a tab of its own; a missing file, a broken one
/// and a picture the device refuses are each said by name and do not stop the
/// rest; a file open already (by another name) is not read again; a second copy
/// of one document is a tab of its own; the shown file's tab is shown at the end.
/// Reopen again adds only what was missing and never a second tab of a file.
#[test]
fn reopen_restores_saved_files_beside_open_tabs_in_order_and_shows_the_last_shown() {
    let user = tempfile::tempdir().expect("user");
    let [x_file, a_file, d_file] = ["x.fcad", "a.fcad", "d.fcad"].map(|n| user.path().join(n));
    plate(&x_file, 10.0);
    plate(&a_file, 12.0);
    plate(&d_file, 16.0);
    let d_copy = user.path().join("d copy.fcad");
    std::fs::copy(&d_file, &d_copy).expect("copy");
    assert_eq!(document_id(&d_file), document_id(&d_copy));
    let alias = user.path().join("x alias.fcad");
    std::fs::hard_link(&x_file, &alias).expect("another name of X");
    let missing = user.path().join("missing.fcad");
    let broken = user.path().join("broken.fcad");
    std::fs::write(&broken, b"not a document").expect("broken");
    let inputs = [&x_file, &a_file, &d_file, &d_copy, &broken];
    let originals = inputs.map(|f| bytes(f));
    let state = tempfile::tempdir().expect("state");
    let folder = Folder::at(&state.path().join("tabs")).expect("folder");
    let list = LastTabs {
        paths: vec![
            a_file.clone(),
            missing.clone(),
            alias.clone(),
            broken.clone(),
            d_file.clone(),
            d_copy.clone(),
        ],
        active: Some(4),
    };
    folder.publish(&list).expect("published");
    let kept = bytes(&folder.descriptor());
    let mut restores = offered(&folder);

    // X is open with an unfinished, invalid height and an accepted change.
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&x_file);
    let x = w.sessions.tab();
    w.apply(11.0);
    w.type_height("2..6");
    let feature = w.feature();
    assert!(w.can_open_now(&restores), "an idle form holds nothing");

    // The device refuses D's second copy's picture this time.
    let refuse_copy = |path: &Path| {
        if path == d_copy {
            Err(CadError::kernel("the device refused the picture"))
        } else {
            Ok(())
        }
    };
    // D, the file shown last time, was the last one opened: it is shown already.
    assert_eq!(w.reopen(&mut restores, refuse_copy), None);
    let order = w.tabs.order().to_vec();
    assert_eq!(order.len(), 3, "X, A and D");
    assert_eq!(order[0], x);
    assert_eq!(logical(&w, order[1]), a_file);
    assert_eq!(logical(&w, order[2]), d_file);
    assert_eq!(w.sessions.tab(), order[2]);
    let outcomes = restores.outcomes();
    assert_eq!(outcomes[0], Some(Outcome::Opened));
    assert!(
        matches!(&outcomes[1], Some(Outcome::Failed(_))),
        "{outcomes:?}"
    );
    assert_eq!(outcomes[2], Some(Outcome::AlreadyOpen), "X by another name");
    assert!(
        matches!(&outcomes[3], Some(Outcome::Failed(_))),
        "{outcomes:?}"
    );
    assert_eq!(outcomes[4], Some(Outcome::Opened));
    assert!(
        matches!(&outcomes[5], Some(Outcome::Failed(why)) if why.contains("device refused")),
        "{outcomes:?}"
    );
    let report = restores.report();
    assert_eq!(
        report[0],
        "3 of 6 saved files are open, each as it is saved on disk now."
    );
    assert!(
        report[1].starts_with("missing.fcad — not opened: "),
        "{report:?}"
    );
    assert!(
        report[2].starts_with("broken.fcad — not opened: "),
        "{report:?}"
    );
    assert_eq!(
        report[3],
        "d copy.fcad — not opened: geometry kernel failure: the device refused the picture"
    );
    assert!(w.can_open_now(&restores) && restores.can_begin());

    // X kept its form and its unsaved change through all of it.
    assert!(w.tabs.has_draft(x));
    w.switch(x);
    assert_eq!(w.edits.typed(), Some((Some(feature), "2..6")));
    assert!(w.sessions.dirty());
    assert_eq!(height_in(&w.sessions), 11.0);

    // Reopen again (X shown): only D's copy is new — a tab of its own, after the
    // tab shown when it was read — and then D is shown.
    let shown = w.reopen(&mut restores, |_| Ok(())).expect("D is shown");
    let order = w.tabs.order().to_vec();
    assert_eq!(
        order
            .iter()
            .map(|tab| logical(&w, *tab))
            .collect::<Vec<_>>(),
        [
            x_file.clone(),
            d_copy.clone(),
            a_file.clone(),
            d_file.clone()
        ]
    );
    assert_eq!(shown, order[3]);
    assert_eq!(w.sessions.tab(), order[3]);
    assert_eq!(restores.outcomes()[5], Some(Outcome::Opened));
    assert_eq!(
        restores.report()[0],
        "4 of 6 saved files are open, each as it is saved on disk now."
    );
    // And again: nothing new at all.
    w.reopen(&mut restores, |_| Ok(()));
    assert_eq!(w.tabs.order(), order.as_slice());
    assert_eq!(restores.outcomes()[5], Some(Outcome::AlreadyOpen));

    // X's draft is still X's; nothing wrote a file or the list.
    w.switch(x);
    assert_eq!(w.edits.typed(), Some((Some(feature), "2..6")));
    assert_eq!(inputs.map(|f| bytes(f)), originals);
    assert_eq!(bytes(&folder.descriptor()), kept);
    // Not now hides the offer and its account; it opens and removes nothing.
    restores.later();
    assert!(restores.rows().is_empty() && restores.report().is_empty() && !restores.can_begin());
    assert_eq!(bytes(&folder.descriptor()), kept);
    println!("\nFCAD_30R_REOPEN_EXECUTED");
}

/// Cancel stops the queue without taking back the tabs already opened; the late
/// answer of the cancelled reading opens nothing and does not touch the next
/// Reopen; a full window opens what fits and says why not the rest; the shown file
/// that did not open leaves the first open file of the list shown.
#[test]
fn cancel_late_answers_and_a_full_window_never_revive_or_mix_a_reopen() {
    let user = tempfile::tempdir().expect("user");
    let files: Vec<PathBuf> = ["a.fcad", "b.fcad", "c.fcad"]
        .iter()
        .map(|n| user.path().join(n))
        .collect();
    for (file, height) in files.iter().zip([12.0, 15.0, 18.0]) {
        plate(file, height);
    }
    let state = tempfile::tempdir().expect("state");
    let folder = Folder::at(&state.path().join("tabs")).expect("folder");
    folder
        .publish(&LastTabs {
            paths: files.clone(),
            active: Some(1),
        })
        .expect("published");
    let mut restores = offered(&folder);
    let mut w = Window::new(Drawn::Mock, None);
    let mut loads = Loads::default();

    // A opens; B's reading is cancelled while it runs.
    assert!(restores.begin());
    let Step::Read(path) = crate::restores::step(&mut restores, &w.tabs, &w.sessions) else {
        panic!("A is read first");
    };
    assert_eq!(path, files[0]);
    let first = Window::start(&mut loads, &path);
    assert!(restores.reading(Some(first)));
    let read = w.reading(&path);
    assert!(w.deliver_reopen(&mut loads, &mut restores, first, read, Ok(())));
    let a = w.sessions.tab();
    let Step::Read(path) = crate::restores::step(&mut restores, &w.tabs, &w.sessions) else {
        panic!("B is read next");
    };
    assert_eq!(path, files[1]);
    let late = Window::start(&mut loads, &path);
    assert!(restores.reading(Some(late)));
    // The window's Cancel: the reading and the Reopen stop together.
    assert!(crate::cancel_load(&mut loads, &mut w.input));
    restores.cancel();
    assert!(!restores.running());
    assert!(w.can_open_now(&restores), "Open is free again");
    assert_eq!(
        restores.outcomes(),
        [
            Some(Outcome::Opened),
            Some(Outcome::NotOpened("cancelled".to_owned())),
            Some(Outcome::NotOpened("cancelled".to_owned())),
        ]
    );
    assert_eq!(w.tabs.order(), [a], "A stays open");
    let report = restores.report();
    assert_eq!(report[1], "b.fcad — not opened: cancelled");
    assert_eq!(report[2], "c.fcad — not opened: cancelled");
    let late_read = w.reading(&files[1]);

    // Reopen again: A is open already, B is read anew; now the cancelled reading's
    // answer arrives late — it shows nothing and the new Reopen still waits for its
    // own reading.
    assert!(restores.begin());
    let Step::Read(path) = crate::restores::step(&mut restores, &w.tabs, &w.sessions) else {
        panic!("B is read");
    };
    assert_eq!(path, files[1]);
    let again = Window::start(&mut loads, &path);
    assert!(restores.reading(Some(again)));
    assert!(!w.deliver_reopen(&mut loads, &mut restores, late, late_read, Ok(())));
    assert!(
        restores.running(),
        "the late answer did not end this Reopen"
    );
    assert_eq!(restores.outcomes()[1], None, "B is still being read");
    assert_eq!(w.tabs.order(), [a], "the late answer added no tab");
    assert!(!w.can_open_now(&restores));
    // A foreign answer (a generation nobody issued for it) changes nothing either.
    assert!(!restores.answered(LoadGeneration(9_999), Some(Ok(()))));
    assert!(restores.running());
    let read = w.reading(&path);
    assert!(w.deliver_reopen(&mut loads, &mut restores, again, read, Ok(())));
    let b = w.sessions.tab();
    let Step::Read(path) = crate::restores::step(&mut restores, &w.tabs, &w.sessions) else {
        panic!("C is read");
    };
    let generation = Window::start(&mut loads, &path);
    assert!(restores.reading(Some(generation)));
    let read = w.reading(&path);
    assert!(w.deliver_reopen(&mut loads, &mut restores, generation, read, Ok(())));
    // B was shown last time: it is shown again.
    assert_eq!(
        crate::restores::step(&mut restores, &w.tabs, &w.sessions),
        Step::Show(b)
    );
    w.switch(b);
    assert_eq!(w.tabs.order().len(), 3);

    // A full window: seven tabs open, the list's three files beside them — one
    // fits, the rest are refused in words; the shown file (B) did not open, so the
    // first open file of the list (A) is shown.
    let mut full = Window::new(Drawn::Mock, None);
    for _ in 0..MAX_TABS - 1 {
        full.create(NewDocument::Empty).expect("Untitled");
    }
    let mut restores = offered(&folder);
    let shown = full.reopen(&mut restores, |_| Ok(()));
    assert_eq!(full.tabs.count(), MAX_TABS);
    let outcomes = restores.outcomes();
    assert_eq!(outcomes[0], Some(Outcome::Opened));
    assert_eq!(outcomes[1], Some(Outcome::NotOpened(crate::tabs::full())));
    assert_eq!(outcomes[2], Some(Outcome::NotOpened(crate::tabs::full())));
    assert_eq!(shown, None, "A was read last: it is shown already");
    assert_eq!(logical(&full, full.sessions.tab()), files[0]);
    assert!(restores.report()[1].starts_with("b.fcad — not opened: 8 documents are open"));

    // Capacity is not cancellation: a later listed file may already be open,
    // and the previous active file must still be selected at the end.
    let existing_a = full.sessions.tab();
    full.switch(full.tabs.order()[0]);
    folder
        .publish(&LastTabs {
            paths: vec![files[1].clone(), files[0].clone()],
            active: Some(1),
        })
        .expect("B followed by already-open A");
    let mut restores = offered(&folder);
    assert_eq!(full.reopen(&mut restores, |_| Ok(())), Some(existing_a));
    assert_eq!(full.sessions.tab(), existing_a);
    assert_eq!(full.tabs.count(), MAX_TABS);
    assert_eq!(
        restores.outcomes()[0],
        Some(Outcome::NotOpened(crate::tabs::full()))
    );
    assert_eq!(restores.outcomes()[1], Some(Outcome::AlreadyOpen));
    assert!(restores.report()[0].starts_with("1 of 2 saved files are open"));
    println!("\nFCAD_30R_CANCEL_LATE_FULL_EXECUTED");
}

/// Without a kernel, the production reading refuses each file at its picture: the
/// open tab, its form and the list stay; each file is said by name. Keeping the
/// list needs no kernel.
#[test]
fn stub_reopen_is_refused_at_the_picture_and_keeps_the_open_tab() {
    if ferritecad_occt::is_available() {
        return;
    }
    let user = tempfile::tempdir().expect("user");
    let [x_file, a_file] = ["x.fcad", "a.fcad"].map(|n| user.path().join(n));
    plate(&x_file, 10.0);
    plate(&a_file, 12.0);
    let state = tempfile::tempdir().expect("state");
    let folder = Folder::at(&state.path().join("tabs")).expect("folder");
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&x_file);
    let mut restores = Restores::new(Ok(folder.clone()));
    assert!(w.begin_quit());
    assert_eq!(
        w.end_quit(&mut restores),
        QuitEnd::Exit,
        "kept without a kernel"
    );
    assert_eq!(
        folder.read().expect("read").expect("a list").paths,
        std::slice::from_ref(&x_file)
    );
    w.sessions.stop_all();
    w.tabs.stop_all();
    folder
        .publish(&LastTabs {
            paths: vec![a_file.clone(), x_file.clone()],
            active: Some(0),
        })
        .expect("published");
    // X is opened on the mock kernel; the Reopen reads through the production
    // `open_for_view`, which has no kernel here.
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&x_file);
    let x = w.sessions.tab();
    w.type_height("2..6");
    w.drawn = Drawn::Native;
    let mut restores = offered(&folder);
    assert_eq!(w.reopen(&mut restores, |_| Ok(())), None);
    assert_eq!(w.tabs.order(), [x]);
    assert_eq!(w.sessions.tab(), x);
    assert!(w.edits.typed().is_some(), "the form stayed");
    let outcomes = restores.outcomes();
    assert!(
        matches!(&outcomes[0], Some(Outcome::Failed(_))),
        "{outcomes:?}"
    );
    assert_eq!(outcomes[1], Some(Outcome::AlreadyOpen));
    assert!(restores.report()[1].starts_with("a.fcad — not opened: "));
    println!("\nFCAD_30R_STUB_REOPEN_EXECUTED");
}

/// Open CASCADE without the sketch solver: two files kept at Quit are reopened by
/// the production reading and the shown one's height is applied.
#[test]
fn mixed_reopen_with_occt_and_no_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        return;
    }
    let user = tempfile::tempdir().expect("user");
    let [a_file, b_file] = ["a.fcad", "b.fcad"].map(|n| user.path().join(n));
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let state = tempfile::tempdir().expect("state");
    let folder = Folder::at(&state.path().join("tabs")).expect("folder");
    let mut w = Window::new(Drawn::Native, None);
    w.open(&a_file);
    w.open(&b_file);
    let mut restores = Restores::new(Ok(folder.clone()));
    assert!(w.begin_quit());
    assert_eq!(w.end_quit(&mut restores), QuitEnd::Exit);
    w.sessions.stop_all();
    w.tabs.stop_all();
    let mut w = Window::new(Drawn::Native, None);
    let mut restores = offered(&folder);
    assert_eq!(w.reopen(&mut restores, |_| Ok(())), None);
    assert_eq!(w.tabs.count(), 2);
    assert_eq!(logical(&w, w.sessions.tab()), b_file);
    w.apply(19.0);
    assert_eq!(height_in(&w.sessions), 19.0);
    println!("\nFCAD_30R_MIXED_REOPEN_EXECUTED");
}

/// A pointer gesture holds Reopen like any document change.
#[test]
fn reopen_waits_for_a_gesture_and_is_offered_only_with_a_list() {
    let state = tempfile::tempdir().expect("state");
    let folder = Folder::at(&state.path().join("tabs")).expect("folder");
    let w = Window::new(Drawn::Mock, None);
    let none = offered(&folder);
    assert!(!w.can_reopen(&Loads::default(), &none), "no list, no offer");
    folder
        .publish(&LastTabs {
            paths: vec![state.path().join("a.fcad")],
            active: None,
        })
        .expect("published");
    let restores = offered(&folder);
    let mut w = w;
    assert!(w.can_reopen(&Loads::default(), &restores));
    w.input
        .handle(ViewportEvent::PointerMoved { x: 10.0, y: 10.0 }, false);
    w.input
        .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
    assert!(
        !w.can_reopen(&Loads::default(), &restores),
        "a gesture holds it"
    );
    w.input.handle(ViewportEvent::GestureCancelled, false);
    assert!(w.can_reopen(&Loads::default(), &restores));
    // A refused list is said and offers nothing.
    std::fs::write(folder.descriptor(), b"FERRITECAD-LAST-TABS 7\nplatform x\n").expect("w");
    let refused = offered(&folder);
    assert!(!refused.can_begin());
    assert!(
        refused.refused().is_some_and(
            |why| why.contains("(unknown-version)") && why.contains("Nothing was opened")
        ),
        "{:?}",
        refused.refused()
    );
}

// --- native: two lifecycles on the owners, and the comparator ---------------------

/// What the §30R window recipe leaves in its root: both files saved, the second
/// window's unsaved exports of B, the first window's list (copied by the
/// generator's `--between` step after the first viewer ended, never written by
/// it) and the list the second window kept.
const RECIPE_OUTPUTS: [&str; 6] = [
    "a.fcad",
    "b.fcad",
    "b-unsaved.stl",
    "b-unsaved.fbx",
    "first-window",
    "tabs/last-window",
];

/// The recipe's two lifecycles (see the contract) on the window's owners, with
/// the production reading, Apply, export and Save workers: not window evidence.
fn recipe_on_owners(root: &Path) {
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let folder = Folder::at(&root.join("tabs")).expect("folder");
    let (a_file, b_file) = (root.join("a.fcad"), root.join("b.fcad"));

    // First window: a.fcad named at start, Open b.fcad; A 26 mm, Save; Quit on A.
    let mut restores = offered(&folder);
    assert!(!restores.can_begin(), "nothing is offered the first time");
    let mut w = Window::new(Drawn::Native, Some(&store));
    w.open(&a_file);
    let a = w.sessions.tab();
    w.open(&b_file);
    w.switch(a);
    w.apply(26.0);
    assert!(w.save(SaveTarget::InPlace).published);
    assert!(w.begin_quit());
    assert_eq!(w.end_quit(&mut restores), QuitEnd::Exit);
    w.sessions.stop_all();
    w.tabs.stop_all();
    // The generator's `--between`: a copy of the list as the first window left it.
    std::fs::copy(folder.descriptor(), root.join("first-window")).expect("copied");

    // Second window, started with no document: Reopen; B's circle; exports; Save;
    // Quit on B.
    let mut restores = offered(&folder);
    let mut w = Window::new(Drawn::Native, Some(&store));
    let shown = w.reopen(&mut restores, |_| Ok(())).expect("A was shown");
    assert_eq!(logical(&w, shown), a_file);
    assert!(!w.sessions.dirty(), "A is its saved file");
    let b = *w.tabs.order().last().expect("B");
    w.switch(b);
    w.apply_circle([-3.5, 4.25], 8.0);
    let alias = w.sessions.suggestion().expect("alias");
    export_bytes(
        &w.sessions.export_path().expect("B"),
        &alias,
        root,
        "b-unsaved",
    );
    assert!(w.save(SaveTarget::InPlace).published);
    assert!(w.begin_quit());
    assert_eq!(w.end_quit(&mut restores), QuitEnd::Exit);
    w.sessions.stop_all();
    w.tabs.stop_all();
}

/// A descriptor file's list, read by the window's own reader from a copy.
fn list_in(file: &Path) -> std::result::Result<LastTabs, String> {
    let copy = tempfile::tempdir().expect("copy");
    std::fs::copy(file, copy.path().join(crate::last_tabs::DESCRIPTOR)).expect("copied");
    match Folder::at(copy.path()).expect("folder").read() {
        Ok(Some(list)) => Ok(list),
        Ok(None) => Err("no list".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// Whether `list` names exactly the files `names`, in that order, all in one
/// folder, with `shown` the shown one. Which folder is checked once, on the root
/// the windows ran in ([`lists_name_the_root`]): a control works on a copy.
fn names(list: &LastTabs, names: &[&str], shown: usize) -> bool {
    let folder = list.paths.first().and_then(|path| path.parent());
    list.active == Some(shown)
        && list.paths.len() == names.len()
        && list.paths.iter().zip(names).all(|(listed, name)| {
            listed.file_name() == Some(std::ffi::OsStr::new(name)) && listed.parent() == folder
        })
}

/// Both lists name files in `root` itself (by any name of it).
fn lists_name_the_root(root: &Path) {
    for name in ["first-window", "tabs/last-window"] {
        let list = list_in(&root.join(name)).expect("a list");
        for path in &list.paths {
            assert!(
                path.parent()
                    .is_some_and(|folder| ferritecad_jobs::names_same_file(folder, root)),
                "{name} names a file outside the recipe's root: {}",
                path.display()
            );
        }
    }
}

/// Compares the outputs of the §30R window recipe in `root` with the shipped
/// command line. Reads only what the windows wrote; refuses a missing output
/// before any peer job.
fn compare_reopen(root: &Path) {
    use super::super::checkpoints::{all_sql, ids_and_refs, stl_facts};
    let missing: Vec<&str> = RECIPE_OUTPUTS
        .iter()
        .copied()
        .filter(|name| !root.join(name).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "missing real GUI output: {}",
        missing.join(", ")
    );
    let read = |name: &str| std::fs::read(root.join(name)).expect(name);
    assert!(
        read("a.fcad") != read("inputs/a.fcad"),
        "a.fcad was never saved"
    );
    assert!(
        read("b.fcad") != read("inputs/b.fcad"),
        "b.fcad was never saved"
    );
    let files = [root.join("a.fcad"), root.join("b.fcad")];
    let both = ["a.fcad", "b.fcad"];
    // The first window's Quit kept A and B, A shown; the second window's Quit kept
    // the same two files, B shown.
    let first = list_in(&root.join("first-window"));
    assert!(
        first.as_ref().is_ok_and(|list| names(list, &both, 0)),
        "first-window is not the first window's list of a.fcad (shown) and b.fcad: {first:?}"
    );
    let last = list_in(&root.join("tabs/last-window"));
    assert!(
        last.as_ref().is_ok_and(|list| names(list, &both, 1)),
        "tabs/last-window is not the second window's list of a.fcad and b.fcad (shown): {last:?}"
    );

    let work = tempfile::tempdir().expect("work");
    let (a_peer, b_peer) = super::peers(&root.join("inputs"), work.path());
    let (a, b) = (&files[0], &files[1]);
    // A: saved by the first window; the command line's 26 mm in every SQL cell
    // but the write stamp, with the input's identities and references.
    assert!(
        all_sql(a, &[], false) == all_sql(&a_peer, &[], false),
        "a.fcad is not the command line's model"
    );
    assert!(
        ids_and_refs(a) == ids_and_refs(&root.join("inputs/a.fcad")),
        "a.fcad identities"
    );
    // B: reopened and edited by the second window, the same way.
    assert!(
        all_sql(b, &[], false) == all_sql(&b_peer, &[], false),
        "b.fcad is not the command line's model"
    );
    assert!(
        ids_and_refs(b) == ids_and_refs(&root.join("inputs/b.fcad")),
        "b.fcad identities"
    );
    // The second window's unsaved exports, byte for byte the command line's, and
    // the saved file exports the same bytes; geometry read independently.
    let (b_stl, b_fbx) = peer_bytes(&b_peer, work.path(), "b");
    assert!(
        read("b-unsaved.stl") == b_stl,
        "b-unsaved.stl against the CLI"
    );
    assert!(
        read("b-unsaved.fbx") == b_fbx,
        "b-unsaved.fbx against the CLI"
    );
    assert!(
        peer_bytes(b, work.path(), "b-saved") == (b_stl.clone(), b_fbx),
        "b.fcad exports differ from the unsaved ones"
    );
    super::super::analytic::assert_round(&b_stl, [-3.5, 4.25], 8.0, None, 15.25);
    let (a_stl, _) = peer_bytes(a, work.path(), "a-saved");
    assert!(
        a_stl == peer_bytes(&a_peer, work.path(), "a").0,
        "a.fcad exports against the CLI"
    );
    let (_, height, volume) = stl_facts(&a_stl);
    assert!(
        (height - 26.0).abs() < 1e-4 && (volume - 80.0 * 40.0 * 26.0).abs() < 1e-2,
        "a.fcad geometry: {height} {volume}"
    );
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let listing = store.list().expect("list");
    assert!(
        listing.recoverable().count() == 0 && listing.active == 0,
        "a recovery record was left behind"
    );
}

/// The comparator's positive run and its eight negative controls, each on a copy.
fn compare_reopen_with_controls(root: &Path) {
    compare_reopen(root);
    lists_name_the_root(root);
    let control = |name: &str, why: &str, breaks: &dyn Fn(&Path)| {
        super::control_of(compare_reopen, root, name, why, breaks);
    };
    let file = |r: &Path, name: &str| std::fs::read(r.join(name)).expect(name);
    let before = cli_runs();
    control("missing", "missing real GUI output: b-unsaved.fbx", &|r| {
        std::fs::remove_file(r.join("b-unsaved.fbx")).expect("remove");
    });
    assert_eq!(cli_runs(), before, "a peer job ran before a missing output");
    control("a old", "a.fcad was never saved", &|r| {
        std::fs::write(r.join("a.fcad"), file(r, "inputs/a.fcad")).expect("w");
    });
    control("b old", "b.fcad was never saved", &|r| {
        std::fs::write(r.join("b.fcad"), file(r, "inputs/b.fcad")).expect("w");
    });
    control(
        "documents swapped",
        "a.fcad is not the command line's model",
        &|r| {
            let (a, b) = (file(r, "a.fcad"), file(r, "b.fcad"));
            std::fs::write(r.join("a.fcad"), b).expect("w");
            std::fs::write(r.join("b.fcad"), a).expect("w");
        },
    );
    // A list made by hand naming only A: not what the first window's Quit kept.
    control(
        "first list replaced",
        "first-window is not the first window's list",
        &|r| {
            let made = tempfile::tempdir().expect("made");
            let folder = Folder::at(made.path()).expect("folder");
            folder
                .publish(&LastTabs {
                    paths: vec![r.join("a.fcad")],
                    active: Some(0),
                })
                .expect("published");
            std::fs::copy(folder.descriptor(), r.join("first-window")).expect("w");
        },
    );
    // The second window never got to its own Quit: its list is still the first's.
    control(
        "last list stale",
        "tabs/last-window is not the second window's list",
        &|r| {
            std::fs::copy(r.join("first-window"), r.join("tabs/last-window")).expect("w");
        },
    );
    control(
        "last list damaged",
        "tabs/last-window is not the second window's list",
        &|r| {
            let bytes = file(r, "tabs/last-window");
            std::fs::write(r.join("tabs/last-window"), &bytes[..bytes.len() - 3]).expect("w");
        },
    );
    control("exports swapped", "b-unsaved.fbx against the CLI", &|r| {
        std::fs::write(r.join("b-unsaved.fbx"), file(r, "b-unsaved.stl")).expect("w");
    });
}

/// The real windows' outputs in `FCAD_30R_GUI_DIR` against the command line, then
/// eight negative controls on copies of those outputs. Nothing here makes a
/// window output; without the variable the test does nothing.
#[test]
fn native_compare_real_reopen_gui_artifacts_with_negative_controls() {
    let Some(root) = std::env::var_os("FCAD_30R_GUI_DIR") else {
        return;
    };
    assert!(native(), "the comparison needs Open CASCADE");
    compare_reopen_with_controls(Path::new(&root));
    println!("\nFCAD_30R_GUI_COMPARE_OK negative_controls=8 all_SQL_cells=true");
}

/// Native, compact: a first window saves A and quits; a second one reopens A and
/// B from the list it kept, edits B, exports and saves it, and quits on B. The
/// recipe's files then go through the comparator (every SQL cell, ids and
/// references, CLI exports before and after Save, geometry read independently,
/// both lists) and its eight controls. The owners without a window: not window
/// evidence.
#[test]
fn native_reopen_saved_tabs_then_apply_save_and_compare_with_the_command_line() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let root = root.path();
    super::make_inputs(root);
    recipe_on_owners(root);
    compare_reopen_with_controls(root);
    let work = tempfile::tempdir().expect("work");
    let b_stl = std::fs::read(root.join("b-unsaved.stl")).expect("b");
    let b_fbx = std::fs::read(root.join("b-unsaved.fbx")).expect("b");
    let (a_stl, a_fbx) = peer_bytes(&root.join("a.fcad"), work.path(), "a");
    let volume = |stl: &[u8]| super::super::checkpoints::stl_facts(stl).2;
    if let Ok(dir) = std::env::var("FCAD_RESTORE_SAVED_TABS_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        for (name, stl, fbx) in [("reopen-b", &b_stl, &b_fbx), ("reopen-a", &a_stl, &a_fbx)] {
            std::fs::write(dir.join(format!("{name}.stl")), stl).expect("artifact");
            std::fs::write(dir.join(format!("{name}.fbx")), fbx).expect("artifact");
        }
    }
    println!(
        "\nFCAD_30R_NATIVE_REOPEN_EXECUTED volume_a={:.3} volume_b={:.3}",
        volume(&a_stl),
        volume(&b_stl)
    );
    println!("FCAD_30R_SESSION_FILES_COMPARE_OK negative_controls=8 all_SQL_cells=true");
}
