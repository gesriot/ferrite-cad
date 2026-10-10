// SPDX-License-Identifier: MIT
//! §30T: Open while New is not finished. Through the window's own owners — `Creates`
//! (its form, its drawing, its creation worker held at a barrier), the reading's
//! `Loads`, `Tabs`, each tab's `Sessions`, the window's `Edits` and `sketch::Editor`,
//! the arrival statement `present`, the dialog's `Dialogs::receive` and the free
//! functions the window's handlers call (`choose_open`, `stop_new_for_open`,
//! `open_after_new`, the predicates) — without a graphics device.

use super::open_new_recover::{contents, orphan};
use super::*;
use crate::creates::tests::{Held, hold_creation};
use crate::creates::{Candidate, CreateGeneration, CreateStatus};
use crate::dialogs::{Action, Dialogs, Outcome};
use crate::last_tabs::LastTabs;
use crate::recoveries::Recoveries;
use crate::restores::Restores;
use crate::{LoadGeneration, Status};
use ferritecad_ui::{NewChoice, NewContent, NewDocumentForm};

impl Window {
    /// `App::open_chosen`: the dialog's choice. The file to read now, if any.
    fn choose_file(&mut self, restores: &Restores, path: &Path) -> Option<PathBuf> {
        crate::choose_open(
            &mut self.creates,
            &self.tabs,
            &mut self.sessions,
            &self.edits,
            &mut self.input,
            restores,
            path.to_path_buf(),
        )
    }

    /// `App::stop_new_and_open`: *Discard New and open*. The file to read now.
    fn discard_new(&mut self, restores: &Restores) -> Option<PathBuf> {
        crate::stop_new_for_open(
            &mut self.creates,
            &mut self.tabs,
            &mut self.sessions,
            &mut self.edits,
            &mut self.input,
            restores,
        )
    }

    /// `App::open_after_new`: New ended some other way. The file to read now.
    fn open_after_new(&mut self, restores: &Restores) -> Option<PathBuf> {
        crate::open_after_new(
            &mut self.creates,
            &self.tabs,
            &mut self.sessions,
            &self.edits,
            &mut self.input,
            restores,
        )
    }

    /// `App::read_document`: the tab list decides; a file to read gets a generation
    /// of the window's `Loads`; a file a hidden tab names shows that tab.
    fn read_file(&mut self, loads: &mut Loads, path: &Path) -> Option<LoadGeneration> {
        match self.tabs.opening(&self.sessions, path) {
            Opening::Load => Some(Self::begin_load(loads, path)),
            Opening::Shown => {
                self.sessions.status = "That document is already open here.".to_owned();
                None
            }
            Opening::Show(tab) => {
                assert!(self.can_leave(), "showing a tab waits for the window");
                self.switch(tab);
                None
            }
            Opening::Refused(reason) => {
                self.sessions.status = reason;
                None
            }
        }
    }

    /// The window's `Created` handler: the answer is accepted only if `Creates` still
    /// waits for it, shown with its picture, and then a file chosen over this New
    /// is opened. Whether a tab was added, and the file to read now.
    fn created_answer(
        &mut self,
        restores: &Restores,
        generation: CreateGeneration,
        result: Result<Candidate>,
        upload: Result<()>,
    ) -> (bool, Option<PathBuf>) {
        let added =
            crate::creates::finish_create(&mut self.creates, &mut self.input, generation, result)
                .is_some_and(|candidate| self.bind_candidate(candidate, upload).is_ok());
        (added, self.open_after_new(restores))
    }

    /// New's values exactly as typed, whatever they are.
    fn typed_new(&mut self) -> Option<NewDocumentForm> {
        self.creates.form().map(|form| form.clone())
    }

    /// New's form, filled with values some of which are not numbers.
    fn fill_new(&mut self, width: &str, depth: &str, height: &str) {
        let form = self.creates.form().expect("New's form");
        form.content = NewContent::SamplePlate;
        form.width = width.to_owned();
        form.depth = depth.to_owned();
        form.height = height.to_owned();
    }
}

/// `can_open` for this window: the toolbar's Open and its handler.
fn can_open_here(w: &Window, restores: &Restores) -> bool {
    crate::can_open(&w.creates, &w.edits, &w.sessions, &w.input, restores)
}

fn quit_allowed(w: &mut Window, loads: &Loads) -> bool {
    crate::begin_window_quit(
        &mut w.tabs,
        &mut w.sessions,
        &w.creates,
        loads,
        &Exports::default(),
        &w.edits,
        &w.input,
    )
}

fn scratch_entries(root: &Path) -> usize {
    std::fs::read_dir(root).expect("scratch").count()
}

/// A file is chosen while New's form or drawing is open and nothing is running:
/// the window asks and throws nothing away. Cancelling or failing the file dialog
/// leaves no choice; *Back to New* keeps every string and the drawing's Undo and
/// Redo; a second choice replaces the first; only *Discard New and open* ends New —
/// and then A's forms, unsaved model, history and crash copy are what they were,
/// an unreadable file or a cancelled reading leaves A shown and New gone for good,
/// and an accepted file is a tab beside A, which comes back with its form.
#[test]
fn open_over_an_unfinished_new_asks_first_and_discards_only_when_told() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("plate.fcad");
    plate(&a_file, 12.0);
    let b_file = user.path().join("b.fcad");
    plate(&b_file, 15.0);
    let broken = user.path().join("broken.fcad");
    std::fs::write(&broken, b"not a document").expect("broken");
    let originals = contents(&[&a_file, &b_file, &broken]);
    let store_root = tempfile::tempdir().expect("store");
    let store = RecoveryStore::open(store_root.path()).expect("store");
    let mut w = Window::new(Drawn::Mock, Some(&store));
    let (mut loads, restores) = (Loads::default(), Restores::default());

    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(14.0);
    assert!(matches!(
        settled(&w.sessions),
        Some(RecoveryStatus::Written { .. })
    ));
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "2..6");
    let a_is_as_left = |w: &Window| {
        assert_eq!(w.sessions.tab(), a, "A stays shown");
        assert!(w.sessions.dirty() && w.sessions.can_undo());
        assert_eq!(height_in(&w.sessions), 14.0);
        assert_eq!(w.edits.typed(), Some((Some(feature), "2..6")), "A's form");
        assert!(!w.tabs.has_aside() && w.tabs.count() == 1);
    };
    a_is_as_left(&w);

    // New's form over A's form: A's form is set aside; New holds literal values.
    assert!(w.begin_new(false));
    w.fill_new("77x", " 33.0 ", "");
    let typed = w.typed_new();
    assert!(w.tabs.has_aside() && w.edits.typed().is_none());
    // Open is offered; every other way out of New stays held.
    assert!(can_open_here(&w, &restores) && !w.can_leave());
    assert!(!crate::may_leave_tab(
        &w.creates,
        &w.edits,
        &w.sessions,
        &w.input
    ));

    // The dialog's Cancel and Failed leave no choice: New and the model are as they were.
    let mut dialogs = Dialogs::default();
    for outcome in [Outcome::Cancelled, Outcome::Failed] {
        assert_eq!(
            dialogs.receive(Action::Open, outcome, &mut w.input, None),
            None
        );
    }
    assert!(w.creates.asking().is_none() && loads.current.is_none());
    assert_eq!(w.typed_new(), typed);

    // A choice asks. Nothing is read, nothing is thrown away, Quit still waits.
    assert_eq!(w.choose_file(&restores, &broken), None);
    assert_eq!(w.creates.asking().map(|(file, _)| file), Some(&*broken));
    assert!(loads.current.is_none() && w.tabs.count() == 1);
    assert_eq!(w.typed_new(), typed);
    assert!(!quit_allowed(&mut w, &loads), "New's form holds Quit");
    // A second choice replaces the first (the same New: one question).
    assert_eq!(w.choose_file(&restores, &b_file), None);
    let (asked, losing) = w.creates.asking().expect("the question");
    assert_eq!(asked, b_file);
    assert!(losing.contains("typed in the New form"), "{losing}");
    // Back to New: the question goes, every string is as typed, A's form still aside.
    w.creates.keep_new(&mut w.input);
    assert!(w.creates.asking().is_none() && loads.current.is_none());
    assert_eq!(w.typed_new(), typed, "values are kept literally");
    assert!(w.tabs.has_aside() && w.edits.typed().is_none());
    // And a stale Discard (no question) throws nothing away.
    assert_eq!(w.discard_new(&restores), None);
    assert_eq!(w.typed_new(), typed);

    // Round 1: Discard, then a file that cannot be read.
    assert_eq!(w.choose_file(&restores, &broken), None);
    assert_eq!(w.discard_new(&restores).as_deref(), Some(&*broken));
    assert!(w.creates.asking().is_none() && !w.creates.making_new());
    assert_eq!(w.creates.status(), &CreateStatus::Stopped);
    assert!(
        w.typed_new().is_none(),
        "New's form ended with the decision"
    );
    a_is_as_left(&w);
    let reading = w.read_file(&mut loads, &broken).expect("a reading");
    assert!(
        !quit_allowed(&mut w, &loads),
        "a reading holds Quit; so does A's form"
    );
    assert!(!w.deliver_load(&mut loads, reading, w.read(&broken), Ok(())));
    assert!(matches!(loads.status(), Status::Failed { .. }));
    a_is_as_left(&w);
    assert!(
        w.typed_new().is_none() && !w.creates.running() && !w.creates.making_new(),
        "an unreadable file brings nothing back and creates nothing"
    );

    // Round 2: New's drawing, with draft history both ways.
    assert!(w.begin_new(true));
    crate::sketch::tests::fill_drawing(&mut w.creates.sketch);
    // A gesture on the drawing holds Open, as before: nothing is asked.
    let (ctx, at) = crate::sketch::drag_tests::hold_vertex(&mut w.creates.sketch);
    assert!(w.creates.sketch.gesturing() && !can_open_here(&w, &restores));
    assert_eq!(w.choose_file(&restores, &b_file), None);
    assert!(w.creates.asking().is_none(), "no question during a gesture");
    crate::sketch::drag_tests::release_vertex(&ctx, &mut w.creates.sketch, at);
    assert!(can_open_here(&w, &restores));
    let (_, undo_before, _) =
        crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("drawing");
    let scribble = crate::sketch::tests::scribble_drawing(&mut w.creates.sketch);
    assert!(scribble.1 == undo_before + 1 && scribble.2 == 1 && w.creates.sketch.drawing_new());
    assert_eq!(w.choose_file(&restores, &b_file), None);
    let (_, losing) = w.creates.asking().expect("the question");
    assert!(losing.contains("sketch you drew"), "{losing}");
    w.creates.keep_new(&mut w.input);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(scribble.clone()),
        "Back to New keeps the drawing and its Undo/Redo"
    );
    // Discard, then Cancel the reading: A shown, nothing brought back.
    assert_eq!(w.choose_file(&restores, &b_file), None);
    assert_eq!(w.discard_new(&restores).as_deref(), Some(&*b_file));
    assert!(!w.creates.sketch.drawing_new() && w.typed_new().is_none());
    a_is_as_left(&w);
    let reading = w.read_file(&mut loads, &b_file).expect("a reading");
    assert!(crate::cancel_load(&mut loads, &mut w.input));
    assert!(!w.deliver_load(&mut loads, reading, w.read(&b_file), Ok(())));
    a_is_as_left(&w);
    assert!(!w.creates.sketch.drawing_new() && !w.creates.making_new());
    assert_ne!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(scribble),
        "the discarded drawing did not come back"
    );

    // Round 3: Discard, then a file that opens. A is hidden with its form.
    assert!(w.begin_new(false));
    w.fill_new("1e999x", "-", "0");
    assert_eq!(w.choose_file(&restores, &b_file), None);
    assert_eq!(w.discard_new(&restores).as_deref(), Some(&*b_file));
    a_is_as_left(&w);
    let reading = w.read_file(&mut loads, &b_file).expect("a reading");
    assert!(w.deliver_load(&mut loads, reading, w.read(&b_file), Ok(())));
    let b = w.sessions.tab();
    assert_ne!(b, a);
    assert!(w.tabs.has_draft(a) && w.hidden(a).dirty() && w.hidden(a).can_undo());
    assert_eq!(height_in(w.hidden(a)), 14.0);
    assert!(w.edits.typed().is_none() && !w.sessions.dirty());
    assert!(w.typed_new().is_none() && !w.creates.making_new());
    // Back in A: the literal text is there, and so is the crash copy.
    w.switch(a);
    a_is_as_left_after_return(&w, a, feature);
    assert!(matches!(
        settled(&w.sessions),
        Some(RecoveryStatus::Written { .. })
    ));
    assert_eq!(store.list().expect("list").active, 1, "A's record is whole");
    assert_eq!(contents(&[&a_file, &b_file, &broken]), originals);
    loads.stop_all();
    w.creates.stop_all();
    println!("\nFCAD_30T_OPEN_OVER_NEW_EXECUTED");
}

fn a_is_as_left_after_return(w: &Window, a: TabId, feature: ObjectId) {
    assert_eq!(w.sessions.tab(), a);
    assert_eq!(w.edits.typed(), Some((Some(feature), "2..6")));
    assert!(w.sessions.takes_form_apply() && w.sessions.dirty() && w.sessions.can_undo());
    assert_eq!(height_in(&w.sessions), 14.0);
}

/// A's saved-object form lives in the very editor New's drawing uses. It is set aside
/// when New opens, and a Discard that ends the drawing gives it back whole — its
/// text and its draft Undo and Redo — before the file arrives; when the file is
/// accepted A is hidden with it, and it is there as left when A is shown again.
#[test]
fn a_saved_object_form_survives_a_discarded_drawing_exactly() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("plate.fcad");
    plate(&a_file, 12.0);
    let b_file = user.path().join("b.fcad");
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    let (mut loads, restores) = (Loads::default(), Restores::default());
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(14.0);
    let current = w.sessions.export_path().expect("A");
    let reading = read_extrude_source(&current).expect("A");
    let (editor, _) = crate::sketch::tests::typed_apply(
        &current,
        &reading,
        reading.sketches[0].sketch,
        "80",
        "90",
    );
    w.creates.sketch = editor;
    crate::sketch::tests::press(&mut w.creates.sketch, "Undo draft");
    let polygon = crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("A's draft");
    assert!(polygon.2 > 0, "a draft Redo to keep");

    // New's drawing over it: A's editor is set aside, the drawing starts clean.
    assert!(w.begin_new(true));
    assert!(w.tabs.has_aside() && w.creates.sketch.drawing_new());
    crate::sketch::tests::fill_drawing(&mut w.creates.sketch);
    let drawing = crate::sketch::tests::scribble_drawing(&mut w.creates.sketch);
    assert_ne!(drawing, polygon);
    assert_eq!(w.choose_file(&restores, &b_file), None);
    // Back to New: the drawing and its history are New's, as typed.
    w.creates.keep_new(&mut w.input);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(drawing)
    );
    // Discard: the drawing ends; A's editor is back exactly, before the file is read.
    assert_eq!(w.choose_file(&restores, &b_file), None);
    assert_eq!(w.discard_new(&restores).as_deref(), Some(&*b_file));
    assert!(!w.creates.sketch.drawing_new() && !w.tabs.has_aside());
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(polygon.clone()),
        "A's vertex form and its draft history"
    );
    assert!(w.sessions.dirty() && w.sessions.takes_form_apply());

    // The file is accepted: A is hidden with the form; B has none.
    let reading = w.read_file(&mut loads, &b_file).expect("a reading");
    assert!(w.deliver_load(&mut loads, reading, w.read(&b_file), Ok(())));
    assert!(w.tabs.has_draft(a) && w.hidden(a).dirty());
    assert!(!w.creates.sketch.saved_object_open() && !w.creates.sketch.drawing_new());
    w.switch(a);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(polygon.clone()),
        "as left, after the round trip"
    );
    crate::sketch::tests::press(&mut w.creates.sketch, "Redo draft");
    let redone = crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("A's draft");
    assert_eq!((redone.1, redone.2), (polygon.1 + 1, polygon.2 - 1));
    loads.stop_all();
    w.creates.stop_all();
    println!("\nFCAD_30T_SAVED_FORM_SURVIVES_EXECUTED");
}

/// New is making its document when a file is chosen. In every timing — the worker
/// still working, its candidate made and held, its answer queued for the event
/// loop, or a refusal — Discard ends New's draft and abandons the creation without
/// joining it on the event loop; the worker's scratch stays until its owner drops
/// it; the late answer, success or refusal, changes nothing; the Open that
/// replaced New keeps its slot and status; and A's model, forms and history are as
/// they were.
#[test]
fn a_creation_under_way_is_abandoned_and_its_late_answer_changes_nothing() {
    for timing in ["working", "made", "queued", "refused"] {
        let user = tempfile::tempdir().expect("user");
        let a_file = user.path().join("plate.fcad");
        plate(&a_file, 12.0);
        let b_file = user.path().join("b.fcad");
        plate(&b_file, 15.0);
        let scratch = tempfile::tempdir().expect("scratch");
        let mut w = Window::new(Drawn::Mock, None);
        let (mut loads, restores) = (Loads::default(), Restores::default());
        w.open(&a_file);
        let a = w.sessions.tab();
        w.apply(14.0);
        let feature = w.feature();
        w.open_height_form();
        w.edits.type_height(feature, "2..6");

        assert!(w.begin_new(false));
        w.fill_new("50", " 30 ", "7");
        let content = w.answer_new(NewChoice::Create).expect("a plate");
        let typed = w.typed_new();
        let held = hold_creation(
            &mut w.creates,
            &mut w.input,
            scratch.path(),
            content,
            timing != "working",
        );
        held.at_barrier();
        assert!(w.creates.running() && !w.can_leave());
        assert!(
            can_open_here(&w, &restores),
            "Open is offered over a creation"
        );
        // The answer may already be queued when the person decides.
        let (held, queued): (Option<Held>, _) = if timing == "queued" || timing == "refused" {
            (None, Some(held.finish()))
        } else {
            (Some(held), None)
        };

        assert_eq!(w.choose_file(&restores, &b_file), None);
        let (_, losing) = w.creates.asking().expect("the question");
        assert!(losing.contains("being made"), "{losing}");
        assert_eq!(w.typed_new(), typed, "values stay while the question is up");
        let made_scratch = scratch_entries(scratch.path());
        assert_eq!(w.discard_new(&restores).as_deref(), Some(&*b_file));

        // New is over; its worker is accounted, not joined, and nothing under it was
        // removed by the decision.
        assert!(!w.creates.making_new() && !w.creates.running() && w.typed_new().is_none());
        assert_eq!(w.creates.status(), &CreateStatus::Stopped);
        if queued.is_none() {
            assert_eq!(
                w.creates.accounted(),
                1,
                "{timing}: the worker is accounted"
            );
        }
        assert_eq!(scratch_entries(scratch.path()), made_scratch, "{timing}");
        let reading = w.read_file(&mut loads, &b_file).expect("a reading");
        let waiting = loads.current;
        assert_eq!(waiting, Some(reading));

        // The late answer.
        let (generation, result) = match (held, queued) {
            (Some(held), None) => held.finish(),
            (None, Some((generation, real))) => {
                let result = if timing == "refused" {
                    drop(real);
                    Err(CadError::input("the kernel refused the plate"))
                } else {
                    real
                };
                (generation, result)
            }
            _ => unreachable!("one of the two"),
        };
        let (added, open) = w.created_answer(&restores, generation, result, Ok(()));
        assert!(
            !added && open.is_none(),
            "{timing}: an abandoned answer acted"
        );
        assert_eq!(w.creates.status(), &CreateStatus::Stopped, "{timing}");
        assert_eq!(
            loads.current, waiting,
            "{timing}: the Open slot was released"
        );
        assert!(matches!(
            loads.status(),
            Status::Loading { generation, .. } if *generation == reading
        ));
        assert_eq!(w.sessions.tab(), a);
        assert_eq!(w.tabs.count(), 1);
        assert_eq!(scratch_entries(scratch.path()), 0, "{timing}: scratch left");
        assert_eq!(w.edits.typed(), Some((Some(feature), "2..6")));
        assert!(w.sessions.dirty() && w.sessions.can_undo());
        assert_eq!(height_in(&w.sessions), 14.0);

        // The Open then lands as a tab beside A, which keeps its form.
        assert!(w.deliver_load(&mut loads, reading, w.read(&b_file), Ok(())));
        assert!(w.tabs.has_draft(a) && w.hidden(a).dirty());
        assert!(!w.creates.making_new() && w.typed_new().is_none());
        assert_eq!(w.tabs.count(), 2, "{timing}: no Untitled tab appeared");
        loads.stop_all();
        w.creates.stop_all();
        assert_eq!(w.creates.accounted(), 0);
    }
    println!("\nFCAD_30T_OPEN_OVER_RUNNING_NEW_EXECUTED");
}

/// A creation that finishes before the person answers is a document of its own:
/// the question settles by opening the chosen file beside that tab. Nothing is
/// discarded by a decision made about a New that no longer exists — a late Discard
/// finds no question — and an Untitled tab that New became has the ordinary
/// protection of an unsaved document. A picture the device refuses leaves New (and
/// the question) as they were.
#[test]
fn a_creation_that_becomes_a_tab_first_is_never_discarded() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("plate.fcad");
    plate(&a_file, 12.0);
    let b_file = user.path().join("b.fcad");
    plate(&b_file, 15.0);
    let scratch = tempfile::tempdir().expect("scratch");
    let mut w = Window::new(Drawn::Mock, None);
    let (mut loads, restores) = (Loads::default(), Restores::default());
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(14.0);
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "2..6");

    assert!(w.begin_new(false));
    w.fill_new("50", "30", "7");
    let content = w.answer_new(NewChoice::Create).expect("a plate");
    let held = hold_creation(&mut w.creates, &mut w.input, scratch.path(), content, true);
    held.at_barrier();
    assert_eq!(w.choose_file(&restores, &b_file), None);
    assert!(w.creates.asking().is_some());
    let (generation, result) = held.finish();

    // A picture the device refuses: not shown, New and its question stay.
    let tabs = w.tabs.count();
    let (added, open) = w.created_answer(
        &restores,
        generation,
        result,
        Err(CadError::rendering("no device")),
    );
    assert!(!added && open.is_none());
    assert_eq!(w.tabs.count(), tabs);
    assert!(w.creates.making_new() && w.creates.asking().is_some());
    assert_eq!(
        w.typed_new().map(|form| form.width),
        Some("50".to_owned()),
        "New's values stay after a refused picture"
    );

    // Again, and now accepted: New is a tab of its own, and the question settles.
    let content = w.answer_new(NewChoice::Create).expect("a plate");
    let held = hold_creation(&mut w.creates, &mut w.input, scratch.path(), content, true);
    held.at_barrier();
    assert!(w.creates.asking().is_some(), "still the same New");
    let (generation, result) = held.finish();
    let (added, open) = w.created_answer(&restores, generation, result, Ok(()));
    assert!(added, "the creation became a tab");
    let n = w.sessions.tab();
    assert_ne!(n, a);
    assert!(w.sessions.untitled() && !w.creates.making_new());
    assert_eq!(open.as_deref(), Some(&*b_file), "the chosen file opens");
    assert!(w.creates.asking().is_none());
    assert_eq!(
        w.discard_new(&restores),
        None,
        "a late Discard has no question"
    );
    assert!(w.tabs.has_draft(a), "A went with its form");
    assert_eq!(w.tabs.count(), 2);

    // The ordinary Open, beside the tab New became.
    let reading = w.read_file(&mut loads, &b_file).expect("a reading");
    assert!(w.deliver_load(&mut loads, reading, w.read(&b_file), Ok(())));
    let b = w.sessions.tab();
    assert_eq!(w.tabs.order(), &[a, n, b], "B opened beside N");
    assert!(w.hidden(n).untitled(), "N was not destroyed");
    // N has the ordinary protection of an unsaved document: closing asks.
    assert_eq!(
        w.tabs.close_step(&w.sessions, n, false),
        Some(CloseStep::Show),
        "an Untitled tab is shown, then asked about"
    );
    w.switch(n);
    assert_eq!(
        w.tabs.close_step(&w.sessions, n, false),
        Some(CloseStep::Ask)
    );

    // A creation that became a tab before the file was even chosen: a plain Open.
    assert!(w.begin_new(false));
    w.fill_new("40", "20", "5");
    let content = w.answer_new(NewChoice::Create).expect("a plate");
    let held = hold_creation(&mut w.creates, &mut w.input, scratch.path(), content, false);
    held.at_barrier();
    let (generation, result) = held.finish();
    let (added, open) = w.created_answer(&restores, generation, result, Ok(()));
    assert!(added && open.is_none());
    let copy = user.path().join("copy.fcad");
    std::fs::copy(&b_file, &copy).expect("copy");
    assert_eq!(
        w.choose_file(&restores, &copy).as_deref(),
        Some(&*copy),
        "no New is in the way: the ordinary Open"
    );
    assert!(w.creates.asking().is_none());
    loads.stop_all();
    w.creates.stop_all();
    println!("\nFCAD_30T_OPEN_AFTER_NEW_BECAME_A_TAB_EXECUTED");
}

/// The window's room is checked before anything is asked and before anything is
/// discarded: a full window refuses a new file in words and keeps New and its
/// values; the shown tab's own file is only said to be open; a file a hidden tab
/// names is asked about like any other and, once confirmed, shows that tab without
/// a ninth one — and with A's form where A's form was.
#[test]
fn a_full_window_is_checked_before_anything_is_asked_or_discarded() {
    let user = tempfile::tempdir().expect("user");
    let first = user.path().join("first.fcad");
    plate(&first, 12.0);
    let mut w = Window::new(Drawn::Mock, None);
    let (mut loads, restores) = (Loads::default(), Restores::default());
    w.open(&first);
    let first_tab = w.sessions.tab();
    for n in 1..MAX_TABS {
        let more = user.path().join(format!("more-{n}.fcad"));
        std::fs::copy(&first, &more).expect("copy");
        assert_eq!(w.open(&more), Opening::Load);
    }
    assert_eq!(w.tabs.count(), MAX_TABS);
    let last = w.sessions.tab();
    let last_file = user.path().join(format!("more-{}.fcad", MAX_TABS - 1));
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, " 33.0 ");

    assert!(w.begin_new(false));
    w.fill_new("77x", "", "-");
    let typed = w.typed_new();
    let ninth = user.path().join("ninth.fcad");
    plate(&ninth, 9.0);
    // Full: said, nothing asked, New as typed.
    assert_eq!(w.choose_file(&restores, &ninth), None);
    assert!(w.creates.asking().is_none());
    assert!(
        w.sessions.status.contains("documents are open"),
        "{}",
        w.sessions.status
    );
    assert_eq!(w.typed_new(), typed);
    // The shown tab's own file: said, nothing asked.
    assert_eq!(w.choose_file(&restores, &last_file), None);
    assert!(w.creates.asking().is_none());
    assert!(
        w.sessions.status.contains("already open"),
        "{}",
        w.sessions.status
    );
    assert_eq!(w.typed_new(), typed);
    assert!(loads.current.is_none());

    // A hidden tab's file: asked about like any other.
    assert_eq!(w.choose_file(&restores, &first), None);
    assert!(w.creates.asking().is_some());
    assert_eq!(w.discard_new(&restores).as_deref(), Some(&*first));
    assert!(w.typed_new().is_none() && w.edits.typed() == Some((Some(feature), " 33.0 ")));
    assert_eq!(w.read_file(&mut loads, &first), None, "it shows a tab");
    assert_eq!(w.sessions.tab(), first_tab);
    assert_eq!(w.tabs.count(), MAX_TABS, "no ninth tab");
    assert!(w.tabs.has_draft(last), "the left tab keeps its form");
    w.switch(last);
    assert_eq!(w.edits.typed(), Some((Some(feature), " 33.0 ")));
    loads.stop_all();
    println!("\nFCAD_30T_OPEN_OVER_NEW_FULL_WINDOW_EXECUTED");
}

/// Open gets one narrow exception, for New itself. Everything else that holds the
/// window still holds Open — an operation, a pointer gesture, a copy worker, a
/// Reopen — and nothing else gets the exception: the tab row, New, Recover, Reopen
/// and the predicate for leaving a tab are as they were. Create still follows its
/// own predicate, and a decision made after a worker answered is made against the
/// state then.
#[test]
fn open_alone_leaves_an_unfinished_new_and_every_other_hold_stays() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("plate.fcad");
    plate(&a_file, 12.0);
    let b_file = user.path().join("b.fcad");
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    let (mut loads, mut restores) = (Loads::default(), Restores::default());
    w.open(&a_file);
    w.open(&b_file);
    let (exports, recoveries) = (Exports::default(), Recoveries::default());
    assert!(w.begin_new(false));
    w.fill_new("50", "30", "7");

    let leave_tab = |w: &Window| crate::may_leave_tab(&w.creates, &w.edits, &w.sessions, &w.input);
    let others = |w: &Window, restores: &Restores| {
        (
            w.can_leave(),
            crate::can_recover(
                &w.creates,
                &Loads::default(),
                &exports,
                &w.edits,
                &w.sessions,
                &w.input,
                &recoveries,
            ),
            crate::can_restore(
                &w.creates,
                &Loads::default(),
                &exports,
                &w.edits,
                &w.sessions,
                &w.input,
                &recoveries,
                restores,
            ),
            leave_tab(w),
        )
    };
    // New unfinished: Open yes, the rest no; Create asks its own predicate.
    assert!(can_open_here(&w, &restores));
    assert_eq!(others(&w, &restores), (false, false, false, false));
    assert!(crate::can_create(
        &w.creates,
        &loads,
        &exports,
        &w.edits,
        &w.sessions
    ));
    assert!(!w.begin_new(false) && !w.begin_new(true), "no second New");

    // An operation, a gesture, a copy worker and a Reopen each hold Open as before.
    let running = w
        .sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("an Apply");
    assert!(!can_open_here(&w, &restores), "an Apply holds Open");
    assert_eq!(w.choose_file(&restores, &a_file), None);
    assert!(w.creates.asking().is_none(), "nothing is asked while held");
    assert_eq!(
        w.sessions.finish_apply(running, Err(CadError::Cancelled)),
        Edited::Failed
    );
    assert!(can_open_here(&w, &restores));
    w.input
        .handle(ViewportEvent::PointerMoved { x: 10.0, y: 10.0 }, false);
    w.input
        .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
    assert!(!can_open_here(&w, &restores), "a camera gesture holds Open");
    w.input.handle(ViewportEvent::GestureCancelled, false);
    assert!(can_open_here(&w, &restores));
    let mut copying = crate::edits::Edits::default();
    let request = {
        let current = w.sessions.export_path().expect("shown");
        let reading = read_extrude_source(&current).expect("reading");
        assert!(copying.begin(&current, &reading));
        let feature = reading.features[0].feature;
        copying.type_height(feature, "20");
        copying
            .request(user.path().join("copy-out.fcad"))
            .expect("a copy request")
    };
    copying.start(request, |_, _, _| std::thread::spawn(|| {}));
    assert!(
        !crate::can_open(&w.creates, &copying, &w.sessions, &w.input, &restores),
        "a copy worker holds Open"
    );
    copying.stop_all();
    restores.listed(Ok(Some(LastTabs {
        paths: vec![a_file.clone()],
        active: None,
    })));
    assert!(restores.begin());
    assert!(!can_open_here(&w, &restores), "a Reopen holds Open");
    restores.cancel();

    // A question asked, then a worker's answer arrives before the decision: the
    // decision is made against the state then. Here New became a tab, so Discard
    // finds nothing to decide and the shown tab is not touched.
    let scratch = tempfile::tempdir().expect("scratch");
    let content = w.answer_new(NewChoice::Create).expect("a plate");
    let held = hold_creation(&mut w.creates, &mut w.input, scratch.path(), content, true);
    held.at_barrier();
    let c_file = user.path().join("c.fcad");
    plate(&c_file, 18.0);
    assert_eq!(w.choose_file(&restores, &c_file), None);
    let (generation, result) = held.finish();
    let (added, open) = w.created_answer(&restores, generation, result, Ok(()));
    assert!(added && open.as_deref() == Some(&*c_file));
    let n = w.sessions.tab();
    assert_eq!(w.discard_new(&restores), None);
    assert_eq!(w.sessions.tab(), n, "the new tab was not touched");
    // Open's own destination is unchanged: still `read_document`.
    let reading = w.read_file(&mut loads, &c_file).expect("a reading");
    assert!(w.deliver_load(&mut loads, reading, w.read(&c_file), Ok(())));
    loads.stop_all();
    w.creates.stop_all();
    println!("\nFCAD_30T_OPEN_NARROW_EXCEPTION_EXECUTED");
}

/// The window cannot be asked for a creation that is still running, so the race is
/// proved here: the two tests above hold the production-shaped worker at a barrier
/// in each timing. Without a kernel, the production workers themselves refuse at the
/// picture: the abandoned creation's late refusal rewrites nothing, the Open that
/// replaced New is refused at its picture, and A is as it was (executes only in a
/// build with no kernel).
#[test]
fn stub_open_over_a_running_new_is_refused_at_the_picture_and_new_stays_gone() {
    if ferritecad_occt::is_available() {
        return;
    }
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    let (mut loads, restores) = (Loads::default(), Restores::default());
    let context = OperationContext::default();
    w.open(&a_file);
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "2..6");

    assert!(w.begin_new(false));
    w.fill_new("50", "30", "7");
    let content = w.answer_new(NewChoice::Create).expect("a plate");
    // The production creation (`run_create`), held until the decision is made.
    let (release, resume) = mpsc::channel::<()>();
    let (answers, answered) = mpsc::channel();
    let root = w.private.path().to_path_buf();
    crate::start_new(
        &mut w.creates,
        &Loads::default(),
        &Exports::default(),
        &mut w.input,
        content,
        move |content, generation, cancel| {
            let context = OperationContext::default().with_cancel(cancel.clone());
            crate::creates::spawn_create(
                move || {
                    let _ = resume.recv();
                    crate::creates::run_create(&root, content, &context)
                },
                move |result| {
                    let _ = answers.send((generation, result));
                },
            )
        },
    )
    .expect("a creation starts");
    assert_eq!(w.choose_file(&restores, &b_file), None);
    assert_eq!(w.discard_new(&restores).as_deref(), Some(&*b_file));
    release.send(()).expect("released");

    // The Open that replaced New, refused at its picture: A as left.
    let reading = w.read_file(&mut loads, &b_file).expect("a reading");
    let read = open_for_view(w.private.path(), &b_file, &context);
    assert!(read.is_err(), "no kernel to draw it with");
    // The abandoned creation's refusal arrives meanwhile: nothing is rewritten.
    let (generation, result) = wait(&answered);
    assert!(result.is_err(), "no kernel to draw it with");
    let (added, open) = w.created_answer(&restores, generation, result, Ok(()));
    assert!(!added && open.is_none());
    assert_eq!(w.creates.status(), &CreateStatus::Stopped, "not Failed");
    assert!(!w.deliver_load(&mut loads, reading, read, Ok(())));
    assert_eq!(w.tabs.count(), 1);
    assert_eq!(w.edits.typed(), Some((Some(feature), "2..6")));
    assert!(!w.creates.making_new() && w.typed_new().is_none());
    loads.stop_all();
    w.creates.stop_all();
    println!("\nFCAD_30T_STUB_OPEN_OVER_NEW_EXECUTED");
}

/// Open CASCADE without the solver: the production creation and the production
/// reading, in the order of the race — a candidate made and held, Discard, the
/// reading accepted, the late candidate dropped — and A's form applied after
/// returning.
#[test]
fn mixed_open_over_a_running_new_with_occt_and_no_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        return;
    }
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let scratch = tempfile::tempdir().expect("scratch");
    let mut w = Window::new(Drawn::Native, None);
    let (mut loads, restores) = (Loads::default(), Restores::default());
    let context = OperationContext::default();
    w.open(&a_file);
    let a = w.sessions.tab();
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "18");
    assert!(w.begin_new(false));
    w.fill_new("50", "30", "7");
    let content = w.answer_new(NewChoice::Create).expect("a plate");
    let held = hold_creation(&mut w.creates, &mut w.input, scratch.path(), content, true);
    held.at_barrier();
    assert_eq!(w.choose_file(&restores, &b_file), None);
    assert_eq!(w.discard_new(&restores).as_deref(), Some(&*b_file));
    let reading = w.read_file(&mut loads, &b_file).expect("a reading");
    let (generation, result) = held.finish();
    assert!(result.is_ok(), "drawn by Open CASCADE, and still not shown");
    let (added, open) = w.created_answer(&restores, generation, result, Ok(()));
    assert!(!added && open.is_none());
    assert_eq!(scratch_entries(scratch.path()), 0);
    let read = open_for_view(w.private.path(), &b_file, &context);
    assert!(w.deliver_load(&mut loads, reading, read, Ok(())));
    assert!(!w.scene.catalogue.is_empty(), "drawn by Open CASCADE");
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(feature), "18")));
    assert!(w.apply_height_form());
    assert_eq!(height_in(&w.sessions), 18.0);
    loads.stop_all();
    w.creates.stop_all();
    println!("\nFCAD_30T_MIXED_OPEN_OVER_NEW_EXECUTED");
}

// --- native: the recipe on the owners, and its comparator -------------------------

/// What the §30T window recipe leaves in its root.
const OPEN_OVER_NEW_OUTPUTS: [&str; 3] = ["a.fcad", "a-unsaved.stl", "a-unsaved.fbx"];

/// The recipe (see the contract) on the window's owners, without a window: the
/// comparator's own check, never window evidence. A is changed and has a form; New
/// is making its document, its candidate made and held; B is chosen, New is given
/// up; the late candidate arrives while B is read; A comes back with its form.
fn open_over_new_recipe_on_owners(root: &Path) {
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let mut w = Window::new(Drawn::Native, Some(&store));
    let (mut loads, restores) = (Loads::default(), Restores::default());
    let context = OperationContext::default();
    let scratch = tempfile::tempdir().expect("scratch");
    // 1: A; both 80 made 90 and applied: A is unsaved. 2: its height form, `2..6`.
    w.open(&root.join("a.fcad"));
    let a = w.sessions.tab();
    let current = w.sessions.export_path().expect("A");
    let reading = read_extrude_source(&current).expect("A");
    let (editor, _) = crate::sketch::tests::typed_apply(
        &current,
        &reading,
        reading.sketches[0].sketch,
        "80",
        "90",
    );
    w.creates.sketch = editor;
    w.apply_vertices_form();
    assert!(w.sessions.dirty() && !w.creates.sketch.saved_object_open());
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "2..6");
    // 3: New, making a 50 x 30 x 7 plate; its candidate is made and held.
    assert!(w.begin_new(false));
    w.fill_new("50", "30", "7");
    let content = w.answer_new(NewChoice::Create).expect("a plate");
    let held = hold_creation(&mut w.creates, &mut w.input, scratch.path(), content, true);
    held.at_barrier();
    // 4: the file dialog's Cancel changes nothing.
    let mut dialogs = Dialogs::default();
    assert_eq!(
        dialogs.receive(Action::Open, Outcome::Cancelled, &mut w.input, None),
        None
    );
    assert!(w.creates.asking().is_none() && w.creates.running());
    // 5: b.fcad: the question; Back to New keeps the creation.
    let b_file = root.join("b.fcad");
    assert_eq!(w.choose_file(&restores, &b_file), None);
    w.creates.keep_new(&mut w.input);
    assert!(w.creates.running() && w.creates.asking().is_none());
    // 6: the question again; Discard New and open.
    assert_eq!(w.choose_file(&restores, &b_file), None);
    assert_eq!(w.discard_new(&restores).as_deref(), Some(&*b_file));
    let reading = w.read_file(&mut loads, &b_file).expect("a reading");
    let (generation, result) = held.finish();
    assert!(result.is_ok(), "the candidate was made after all");
    let (added, open) = w.created_answer(&restores, generation, result, Ok(()));
    assert!(!added && open.is_none(), "the late candidate was shown");
    assert_eq!(loads.current, Some(reading));
    let read = open_for_view(w.private.path(), &b_file, &context);
    assert!(w.deliver_load(&mut loads, reading, read, Ok(())));
    assert_eq!(w.tabs.count(), 2, "A and B; no Untitled document");
    assert!(w.edits.typed().is_none() && !w.creates.making_new());
    // 7: A: `2..6` as typed; 26; Apply. 8: unsaved exports; Save.
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(feature), "2..6")));
    w.edits.type_height(feature, "26");
    assert!(w.apply_height_form());
    let alias = w.sessions.suggestion().expect("alias");
    export_bytes(
        &w.sessions.export_path().expect("A"),
        &alias,
        root,
        "a-unsaved",
    );
    assert!(w.save(SaveTarget::InPlace).published);
    // 9: Quit: B is clean and nothing is open or running.
    assert!(crate::begin_window_quit(
        &mut w.tabs,
        &mut w.sessions,
        &w.creates,
        &Loads::default(),
        &Exports::default(),
        &w.edits,
        &w.input,
    ));
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Exit);
    w.tabs.decide_exit(&mut w.sessions);
    w.sessions.stop_all();
    w.tabs.stop_all();
    w.creates.stop_all();
    loads.stop_all();
}

/// Compares the outputs of the §30T window recipe in `root` with the shipped
/// command line. Reads only what the window wrote; refuses a missing output before
/// any peer job. A is the command line's `edit-extrude 26` + `edit-sketch-copy`
/// (80 made 90) in every SQL cell but the write stamp; B was never written; nothing
/// New was making reached a file.
fn compare_open_over_new(root: &Path) {
    use super::super::super::checkpoints::{all_sql, ids_and_refs, stl_facts};
    let missing: Vec<&str> = OPEN_OVER_NEW_OUTPUTS
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
        read("b.fcad") == read("inputs/b.fcad"),
        "b.fcad was written"
    );
    let work = tempfile::tempdir().expect("work");
    let (a_peer, _) = draft_peers(&root.join("inputs"), work.path());
    let a = root.join("a.fcad");
    assert!(
        all_sql(&a, &[], false) == all_sql(&a_peer, &[], false),
        "a.fcad is not the command line's model"
    );
    assert!(
        ids_and_refs(&a) == ids_and_refs(&root.join("inputs/a.fcad")),
        "a.fcad identities"
    );
    let (a_stl, a_fbx) = peer_bytes(&a_peer, work.path(), "a");
    assert!(
        read("a-unsaved.stl") == a_stl,
        "a-unsaved.stl against the CLI"
    );
    assert!(
        read("a-unsaved.fbx") == a_fbx,
        "a-unsaved.fbx against the CLI"
    );
    assert!(
        peer_bytes(&a, work.path(), "a-saved") == (a_stl.clone(), a_fbx),
        "a.fcad exports differ from the unsaved ones"
    );
    let (_, height, volume) = stl_facts(&a_stl);
    assert!(
        (height - 26.0).abs() < 1e-4 && (volume - 90.0 * 40.0 * 26.0).abs() < 1e-2,
        "a.fcad geometry: {height} {volume}"
    );
    let documents: Vec<String> = std::fs::read_dir(root)
        .expect("root")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".fcad"))
        .collect();
    assert!(
        documents.len() == 2,
        "a document New was making reached a file: {documents:?}"
    );
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let listing = store.list().expect("list");
    assert!(
        listing.recoverable().count() == 0 && listing.active == 0,
        "a recovery record was left behind"
    );
}

/// The comparator's positive run and its seven negative controls, each on a copy.
fn compare_open_over_new_with_controls(root: &Path) {
    compare_open_over_new(root);
    let control = |name: &str, why: &str, breaks: &dyn Fn(&Path)| {
        super::super::control_of(compare_open_over_new, root, name, why, breaks);
    };
    let file = |r: &Path, name: &str| std::fs::read(r.join(name)).expect(name);
    let before = cli_runs();
    control("missing", "missing real GUI output: a-unsaved.fbx", &|r| {
        std::fs::remove_file(r.join("a-unsaved.fbx")).expect("remove");
    });
    assert_eq!(cli_runs(), before, "a peer job ran before a missing output");
    control("unsaved a", "a.fcad was never saved", &|r| {
        std::fs::write(r.join("a.fcad"), file(r, "inputs/a.fcad")).expect("w");
    });
    // A's accepted vertices lost to the discard: only a height reached the file.
    control(
        "change lost",
        "a.fcad is not the command line's model",
        &|r| {
            let work = tempfile::tempdir().expect("work");
            let (height_only, _) = super::super::peers(&r.join("inputs"), work.path());
            std::fs::write(r.join("a.fcad"), std::fs::read(height_only).expect("26")).expect("w");
        },
    );
    control("b written", "b.fcad was written", &|r| {
        std::fs::write(r.join("b.fcad"), file(r, "a.fcad")).expect("w");
    });
    control("exports swapped", "a-unsaved.fbx against the CLI", &|r| {
        std::fs::write(r.join("a-unsaved.fbx"), file(r, "a-unsaved.stl")).expect("w");
    });
    // The abandoned New reached a file: a third document beside A and B.
    control(
        "new saved",
        "a document New was making reached a file",
        &|r| {
            std::fs::write(r.join("n.fcad"), file(r, "inputs/b.fcad")).expect("w");
        },
    );
    control("record left", "a recovery record was left behind", &|r| {
        let store = RecoveryStore::open(&r.join("recovery")).expect("store");
        orphan(&store, &r.join("inputs/a.fcad"), &r.join("inputs"), 30.0);
    });
}

/// The real window's outputs in `FCAD_30T_GUI_DIR` against the command line, then
/// seven negative controls on copies of those same outputs. Nothing here makes a
/// window output; without the variable the test does nothing.
#[test]
fn native_compare_real_open_during_new_gui_artifacts_with_negative_controls() {
    let Some(root) = std::env::var_os("FCAD_30T_GUI_DIR") else {
        return;
    };
    assert!(native(), "the comparison needs Open CASCADE");
    compare_open_over_new_with_controls(Path::new(&root));
    println!("\nFCAD_30T_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true");
}

/// Native, compact: A changed and holding a form; New making its document with the
/// candidate made and held; B chosen and the question confirmed; the late candidate
/// arrives while B is read and is dropped; back in A the form is as typed, applied,
/// exported and saved. The recipe's files then go through the comparator (every SQL
/// cell, ids and references, CLI exports before and after Save, geometry read
/// independently) and its seven controls. The owners without a window: not window
/// evidence.
#[test]
fn native_confirmed_open_over_a_running_new_then_late_create_and_back_to_a_match_the_command_line()
{
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let root = root.path();
    make_inputs(root);
    open_over_new_recipe_on_owners(root);
    compare_open_over_new_with_controls(root);
    let volume = super::super::super::checkpoints::stl_facts(
        &std::fs::read(root.join("a-unsaved.stl")).expect("a"),
    )
    .2;
    println!("\nFCAD_30T_NATIVE_OPEN_OVER_NEW_EXECUTED volume_a={volume:.3}");
    println!("FCAD_30T_SESSION_FILES_COMPARE_OK negative_controls=7 all_SQL_cells=true");
}
