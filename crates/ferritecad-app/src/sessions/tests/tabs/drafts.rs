// SPDX-License-Identifier: MIT
//! §30P: each tab keeps its own unfinished edit forms. Through the window's own
//! owners — `Tabs`, each tab's `Sessions`, the window's `Edits` and
//! `sketch::Editor` (with the constraints editor inside it), the arrival statement
//! `present` and the availability predicates the buttons and handlers share —
//! without a graphics device.

use super::*;
use crate::{Loads, exports::Exports};
use ferritecad_ui::{PointerButton, ViewportEvent};

/// §30U: Close with an unfinished form.
mod close_form;
/// §30Q: Open, New and Recover beside these forms, through the same owners.
mod open_new_recover;
/// §30T: Open while New is not finished.
mod open_over_new;
/// §30V: all forms survive an aborted window Quit.
mod quit_forms;
/// §30W: window New stays whole through Quit.
mod quit_new;

impl Window {
    /// `App::begin_edit`: the height form on the shown tab's accepted version.
    fn open_height_form(&mut self) {
        let current = self.sessions.export_path().expect("shown");
        let reading = read_extrude_source(&current).expect("reading");
        assert!(self.edits.begin(&current, &reading));
    }

    /// The window's `can_leave_tab`, for this window with nothing else running.
    fn can_leave(&self) -> bool {
        crate::can_leave_tab(
            &self.creates,
            &Loads::default(),
            &Exports::default(),
            &self.edits,
            &self.sessions,
            &self.input,
        )
    }

    /// `App::apply_height` and the answers it leads to: the shown tab's form,
    /// through the height form's availability, the edit worker on the mock
    /// kernel, the picture and the arrival; an accepted version ends the form, as
    /// the window's `SceneStaged` handler does. `false` when nothing started.
    fn apply_height_form(&mut self) -> bool {
        let offer = crate::height_state(
            &self.edits,
            &self.sessions,
            &self.creates,
            &Loads::default(),
            &Exports::default(),
        );
        if !offer.apply {
            return false;
        }
        let Some((feature, millimetres)) = self.edits.apply_request() else {
            return false;
        };
        let drawn = self.drawn;
        self.accept(move |ticket, cancel, tx| match drawn {
            Drawn::Native => spawn_apply(ticket, feature, millimetres, cancel.clone(), move |v| {
                tx.send(v).expect("edit");
            }),
            Drawn::Mock => std::thread::spawn(move || {
                let edit = ticket.edit_extrude_height(
                    feature,
                    millimetres,
                    &mut MockKernel::new(),
                    &OperationContext::default(),
                );
                tx.send(edit).expect("edit");
            }),
        });
        true
    }

    /// One session step started by a form, accepted with its picture; then, as
    /// the window's `SceneStaged` handler does, the height form ends (the other
    /// forms ended in `present`).
    fn accept(
        &mut self,
        start: impl FnOnce(
            StepTicket,
            &CancelToken,
            mpsc::Sender<Result<ProducedStep>>,
        ) -> std::thread::JoinHandle<()>,
    ) {
        let (tx, rx) = mpsc::channel();
        let generation = self
            .sessions
            .begin_apply(|ticket, _, cancel| start(ticket, cancel, tx))
            .expect("started");
        let Edited::Show(path) = self.sessions.finish_apply(generation, wait(&rx)) else {
            panic!("the edit was not shown: {}", self.sessions.status);
        };
        self.show_staged(generation, path);
        self.edits.cancel();
    }

    /// What the window's frame tells the open forms, from its own predicates.
    fn tell_forms(&mut self) {
        let (loads, exports) = (Loads::default(), Exports::default());
        let sketch =
            crate::can_apply_sketch(&self.creates, &loads, &exports, &self.edits, &self.sessions);
        let analytic =
            crate::can_apply_analytic(&self.creates, &loads, &exports, &self.edits, &self.sessions);
        let constraints = crate::can_apply_constraints(
            &self.creates,
            &loads,
            &exports,
            &self.edits,
            &self.sessions,
        );
        let unsaved = self.sessions.dirty();
        self.creates.sketch.set_session(true, sketch, unsaved);
        self.creates.sketch.set_analytic_apply(analytic);
        self.creates
            .sketch
            .constraints
            .set_session(true, constraints, unsaved);
    }

    /// **Apply vertices** pressed on the shown tab's form and run as `App::apply_sketch`.
    fn apply_vertices_form(&mut self) -> ferritecad_jobs::EditSketchRequest {
        self.tell_forms();
        crate::sketch::tests::press(&mut self.creates.sketch, "Apply vertices");
        let request = self.creates.sketch.take_apply_request().expect("asked");
        let asked = request.clone();
        self.accept(move |ticket, cancel, tx| {
            spawn_apply_sketch(
                ticket,
                asked.sketch,
                asked.vertices,
                asked.expected,
                cancel.clone(),
                move |v| tx.send(v).expect("edit"),
            )
        });
        request
    }

    /// **Apply circle** pressed on the shown tab's form and run as `App::apply_circle`.
    fn apply_circle_form(&mut self) -> ferritecad_jobs::EditCircleRequest {
        self.tell_forms();
        crate::sketch::tests::press(&mut self.creates.sketch, "Apply circle");
        let request = self
            .creates
            .sketch
            .take_apply_circle_request()
            .expect("asked");
        let asked = request.clone();
        self.accept(move |ticket, cancel, tx| {
            spawn_apply_circle(
                ticket,
                asked.sketch,
                asked.edit,
                asked.expected,
                cancel.clone(),
                move |v| tx.send(v).expect("edit"),
            )
        });
        request
    }

    /// **Apply constraints** pressed on the shown tab's form and run as
    /// `App::apply_constraints`.
    fn apply_constraints_form(&mut self) -> ferritecad_jobs::EditSketchConstraintsRequest {
        self.tell_forms();
        crate::constraints::tests::session_apply::press(
            &mut self.creates.sketch.constraints,
            "Apply constraints",
        );
        let request = self
            .creates
            .sketch
            .constraints
            .take_apply_request()
            .expect("asked");
        let asked = request.clone();
        self.accept(move |ticket, cancel, tx| {
            spawn_apply_constraints(
                ticket,
                asked.sketch,
                asked.edits,
                asked.expected,
                cancel.clone(),
                move |v| tx.send(v).expect("edit"),
            )
        });
        request
    }

    /// The shown tab's constraints form, opened and filled through its widgets:
    /// one Parallel pair of `a` and `b`, as `typed_parallel` presses it.
    fn constraints_draft(&mut self, a: usize, b: usize) {
        let current = self.sessions.export_path().expect("shown");
        let reading = read_extrude_source(&current).expect("reading");
        let sketch = reading.constraint_sketches[0].sketch;
        let (editor, _asked) = crate::constraints::tests::session_apply::typed_parallel(
            &current, &reading, sketch, a, b,
        );
        self.creates.sketch.constraints = editor;
    }

    /// Shows `tab` and checks that every tab's unsaved state stayed as it was.
    fn round(&mut self, tab: TabId) {
        assert!(self.can_leave(), "an idle form holds no window");
        self.switch(tab);
    }
}

fn private_count(sessions: &Sessions) -> usize {
    std::fs::read_dir(sessions.private_directory().expect("private"))
        .expect("listed")
        .count()
}

fn height_in(sessions: &Sessions) -> f64 {
    height_of(&sessions.export_path().expect("accepted"))
}

/// Two physical copies of one document (one `DocumentId`, the same object ids
/// and the same form controls) each keep their own height draft exactly as
/// typed — invalid text included — through every switch; Apply after returning
/// goes to its own tab only, and only an accepted Apply makes a tab dirty.
#[test]
fn height_drafts_of_two_copies_stay_literal_and_apply_only_to_their_own_tab() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("plate.fcad");
    plate(&a_file, 12.0);
    let b_file = user.path().join("copy.fcad");
    std::fs::copy(&a_file, &b_file).expect("copy");
    assert_eq!(document_id(&a_file), document_id(&b_file));
    let originals = (
        std::fs::read(&a_file).expect("a"),
        std::fs::read(&b_file).expect("b"),
    );

    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.open(&b_file);
    let b = w.sessions.tab();
    w.round(a);
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "-");
    let files = (private_count(&w.sessions), private_count(w.hidden(b)));
    w.round(b);
    assert_eq!(w.edits.typed(), None, "A's form came along to B");
    assert!(w.tabs.has_draft(a));
    assert_eq!(w.feature(), feature, "copies name the same extrusion");
    w.open_height_form();
    w.edits.type_height(feature, "33.0");
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(feature), "-")));
    assert!(w.tabs.has_draft(b) && !w.tabs.has_draft(a));
    for typed in ["", "1e999x", " 33.0 ", "-0", "-"] {
        w.edits.type_height(feature, typed);
        w.round(b);
        assert_eq!(w.edits.typed(), Some((Some(feature), "33.0")), "B kept");
        w.round(a);
        assert_eq!(w.edits.typed(), Some((Some(feature), typed)), "A literal");
    }
    // Nothing was accepted: clean, no version made, no file written.
    assert!(!w.sessions.dirty() && !w.hidden(b).dirty());
    assert_eq!(
        (private_count(&w.sessions), private_count(w.hidden(b))),
        files
    );
    assert!(!w.apply_height_form(), "an invalid draft is not a request");
    assert_eq!(height_in(&w.sessions), 12.0);

    // Apply after returning: A's own draft, into A only.
    w.edits.type_height(feature, "27.5");
    assert!(w.apply_height_form());
    assert_eq!(height_in(&w.sessions), 27.5);
    assert!(w.sessions.dirty());
    assert_eq!(w.edits.typed(), None, "an accepted Apply ends the form");
    assert!(!w.hidden(b).dirty(), "B is not touched by A's Apply");
    assert_eq!(height_in(w.hidden(b)), 12.0);
    assert!(w.tabs.has_draft(b), "B's form waits in B");
    w.round(b);
    assert_eq!(w.edits.typed(), Some((Some(feature), "33.0")));
    assert!(!w.sessions.dirty(), "a restored draft is not a change");
    assert!(w.apply_height_form());
    assert_eq!(height_in(&w.sessions), 33.0);

    // Save writes the accepted model, never typed text, and only its own file.
    assert!(w.save(SaveTarget::InPlace).published);
    assert_eq!(height_of(&b_file), 33.0);
    assert_eq!(std::fs::read(&a_file).expect("a"), originals.0);
    w.round(a);
    w.open_height_form();
    w.edits.type_height(feature, "40");
    assert!(
        w.save(SaveTarget::InPlace).published,
        "Save works with a form"
    );
    assert_eq!(height_of(&a_file), 27.5, "Save wrote the accepted model");
    assert_eq!(
        w.edits.typed(),
        Some((Some(feature), "40")),
        "and kept the form"
    );
    assert_ne!(std::fs::read(&b_file).expect("b"), originals.1);
    println!("\nFCAD_30P_HEIGHT_DRAFTS_EXECUTED");
}

/// The constraints form of two copies — the same Sketch, the same Line ids, the
/// same controls — keeps each tab's pending additions, picks and request Undo
/// and Redo; a hundred switches copy no history and make no version.
#[test]
fn constraint_drafts_with_one_set_of_ids_keep_picks_changes_and_history_per_tab() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("plate.fcad");
    plate(&a_file, 12.0);
    let b_file = user.path().join("copy.fcad");
    std::fs::copy(&a_file, &b_file).expect("copy");

    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.constraints_draft(1, 3);
    let a_edits = w.creates.sketch.constraints.draft_edits().expect("A");
    let a_state = w.creates.sketch.constraints.draft_state().expect("A");
    assert_eq!(a_edits.add.len(), 1);
    w.open(&b_file);
    let b = w.sessions.tab();
    assert!(
        !w.creates.sketch.constraints.active(),
        "a new tab opens with no form"
    );
    // Open waits while a form is open in the window; here A's was parked by the
    // statement that added B (`Tabs::open`) exactly as a switch parks it.
    assert!(w.tabs.has_draft(a));
    w.constraints_draft(2, 4);
    let b_edits = w.creates.sketch.constraints.draft_edits().expect("B");
    let b_state = w.creates.sketch.constraints.draft_state().expect("B");
    assert_ne!(a_edits, b_edits, "two drafts over one set of ids");

    w.round(a);
    assert_eq!(
        w.creates.sketch.constraints.draft_edits(),
        Some(a_edits.clone())
    );
    assert_eq!(w.creates.sketch.constraints.draft_state(), Some(a_state));
    crate::constraints::tests::session_apply::press(&mut w.creates.sketch.constraints, "Undo");
    let undone = w.creates.sketch.constraints.draft_state().expect("A");
    assert_eq!((undone.3, undone.4), (a_state.3 - 1, a_state.4 + 1));
    assert!(
        w.creates
            .sketch
            .constraints
            .draft_edits()
            .expect("A")
            .add
            .is_empty()
    );
    w.round(b);
    assert_eq!(
        w.creates.sketch.constraints.draft_edits(),
        Some(b_edits.clone())
    );
    assert_eq!(w.creates.sketch.constraints.draft_state(), Some(b_state));
    w.round(a);
    assert_eq!(w.creates.sketch.constraints.draft_state(), Some(undone));
    crate::constraints::tests::session_apply::press(&mut w.creates.sketch.constraints, "Redo");
    assert_eq!(
        w.creates.sketch.constraints.draft_edits(),
        Some(a_edits.clone())
    );

    // A hundred switches: the drafts move, nothing is copied or made.
    let files = (private_count(&w.sessions), private_count(w.hidden(b)));
    w.round(b);
    let leases = Arc::strong_count(&w.hidden(a).export_source().expect("A"));
    for _ in 0..50 {
        w.round(a);
        w.round(b);
    }
    assert_eq!(
        Arc::strong_count(&w.hidden(a).export_source().expect("A")),
        leases,
        "a hidden draft holds one lease of its version, however often it moved"
    );
    assert_eq!(w.creates.sketch.constraints.draft_edits(), Some(b_edits));
    w.round(a);
    assert_eq!(w.creates.sketch.constraints.draft_edits(), Some(a_edits));
    assert_eq!(
        (private_count(&w.sessions), private_count(w.hidden(b))),
        files
    );
    assert!(!w.sessions.dirty() && !w.hidden(b).dirty());
    assert_eq!(
        std::fs::read(&a_file).expect("a"),
        std::fs::read(&b_file).expect("b"),
        "nothing was written"
    );
    println!("\nFCAD_30P_CONSTRAINT_DRAFTS_EXECUTED");
}

/// Showing another tab waits for everything that is not an idle form; a switch
/// that is refused, cancelled, stale or answered for another tab — and a late
/// Apply answer addressed to another tab — leaves both tabs' drafts where they
/// were.
#[test]
fn running_work_holds_the_window_and_failed_or_foreign_answers_keep_both_drafts() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.open(&b_file);
    let b = w.sessions.tab();
    let feature_b = w.feature();
    w.open_height_form();
    w.edits.type_height(feature_b, "b-draft");
    w.round(a);
    let feature_a = w.feature();
    w.open_height_form();
    w.edits.type_height(feature_a, "a-draft");
    let kept = |w: &Window| {
        assert_eq!(w.sessions.tab(), a, "A stays shown");
        assert_eq!(w.edits.typed(), Some((Some(feature_a), "a-draft")));
        assert!(w.tabs.has_draft(b));
        assert!(!w.sessions.busy());
    };

    // Everything but an idle form holds the window, in the predicate the tab row
    // and the handlers share.
    let (loads, exports) = (Loads::default(), Exports::default());
    let leave = |creates: &crate::creates::Creates, edits: &crate::edits::Edits, w: &Window| {
        crate::can_leave_tab(creates, &loads, &exports, edits, &w.sessions, &w.input)
    };
    assert!(leave(&w.creates, &w.edits, &w));
    let mut new = crate::creates::Creates::default();
    assert!(crate::creates::open_form(&mut new, &mut w.input));
    assert!(!leave(&new, &w.edits, &w), "the New form holds the window");
    let mut drawing = crate::creates::Creates::default();
    crate::sketch::tests::begin_drawing(&mut drawing.sketch);
    assert!(!leave(&drawing, &w.edits, &w), "a drawing for New holds it");
    let mut copying = crate::edits::Edits::default();
    let request = {
        let current = w.sessions.export_path().expect("A");
        let reading = read_extrude_source(&current).expect("reading");
        assert!(copying.begin(&current, &reading));
        copying.type_height(feature_a, "20");
        copying
            .request(user.path().join("copy-out.fcad"))
            .expect("a copy request")
    };
    copying.start(request, |_, _, _| std::thread::spawn(|| {}));
    assert!(!leave(&w.creates, &copying, &w), "a copy worker holds it");
    copying.stop_all();
    w.input
        .handle(ViewportEvent::PointerMoved { x: 10.0, y: 10.0 }, false);
    w.input
        .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
    assert!(w.input.is_dragging());
    assert!(!w.can_leave(), "a camera gesture holds it");
    w.input.handle(ViewportEvent::GestureCancelled, false);
    let mut held = crate::creates::Creates::default();
    let (current, reading) = {
        let current = w.sessions.export_path().expect("A");
        let reading = read_extrude_source(&current).expect("reading");
        (current, reading)
    };
    assert!(
        held.sketch
            .begin_edit(&current, &reading, reading.sketches[0].sketch)
    );
    let (ctx, at) = crate::sketch::drag_tests::hold_vertex(&mut held.sketch);
    assert!(held.sketch.gesturing());
    assert!(!leave(&held, &w.edits, &w), "a vertex drag holds it");
    crate::sketch::drag_tests::release_vertex(&ctx, &mut held.sketch, at);
    assert!(!held.sketch.gesturing());
    assert!(leave(&held, &w.edits, &w), "its idle form does not");
    let running = w
        .sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("an Apply");
    assert!(!w.can_leave(), "an Apply holds it");
    assert_eq!(
        w.sessions.finish_apply(running, Err(CadError::Cancelled)),
        Edited::Failed
    );
    kept(&w);

    // A picture the device refuses, a kernel refusal, a Cancel, a stale answer.
    let (generation, rx) = w.begin_switch(b, None);
    let loaded = wait(&rx);
    let (outcome, _) = w
        .deliver_switch(generation, loaded, Err(CadError::input("no device")))
        .expect("awaited");
    assert!(outcome.is_err());
    kept(&w);
    let (generation, rx) = w.begin_switch(b, None);
    drop(wait(&rx));
    let (outcome, _) = w
        .deliver_switch(generation, Err(CadError::kernel("refused")), Ok(()))
        .expect("awaited");
    assert!(outcome.is_err());
    kept(&w);
    let (cancelled, rx) = w.begin_switch(b, None);
    let late = wait(&rx);
    assert!(w.sessions.cancel());
    w.tabs.cancel_switch();
    assert!(w.deliver_switch(cancelled, late, Ok(())).is_none());
    kept(&w);
    let (stale, rx) = w.begin_switch(b, None);
    let stale_scene = wait(&rx);
    assert!(w.sessions.cancel());
    w.tabs.cancel_switch();
    let (current, rx) = w.begin_switch(b, None);
    let current_scene = wait(&rx);
    assert!(w.deliver_switch(stale, stale_scene, Ok(())).is_none());
    assert_eq!(w.edits.typed(), Some((Some(feature_a), "a-draft")));
    w.deliver_switch(current, current_scene, Ok(()))
        .expect("awaited")
        .0
        .expect("shown");
    assert_eq!(w.edits.typed(), Some((Some(feature_b), "b-draft")));
    assert!(w.tabs.has_draft(a));

    // A late Apply answer addressed to A, with the generation B's own operation
    // has: B ignores it, keeps its slot, and neither draft moves.
    let (tx, rx) = mpsc::channel();
    let own = w
        .sessions
        .begin_apply(|ticket, _, _| {
            std::thread::spawn(move || {
                tx.send(ticket.edit_extrude_height(
                    feature_b,
                    16.0,
                    &mut MockKernel::new(),
                    &OperationContext::default(),
                ))
                .expect("edit");
            })
        })
        .expect("B's Apply");
    let answer = wait(&rx);
    let foreign = Address {
        tab: a,
        generation: own.generation,
    };
    assert_eq!(
        w.sessions.finish_apply(foreign, Err(CadError::Cancelled)),
        Edited::Ignore
    );
    assert!(w.sessions.busy(), "B's slot is its own Apply's");
    assert_eq!(w.edits.typed(), Some((Some(feature_b), "b-draft")));
    let Edited::Show(path) = w.sessions.finish_apply(own, answer) else {
        panic!("B's own answer: {}", w.sessions.status);
    };
    w.show_staged(own, path);
    assert_eq!(height_in(&w.sessions), 16.0);
    assert!(w.tabs.has_draft(a), "A's draft is A's");
    assert!(!w.hidden(a).dirty());
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(feature_a), "a-draft")));
    println!("\nFCAD_30P_REFUSED_SWITCHES_EXECUTED");
}

/// Closing and Quit never lose an open form, even over a clean model: the tab
/// with the form is shown and left open with it; nothing is closed, saved or
/// applied. When the form is done, closing frees that tab's files only.
#[test]
fn close_and_quit_keep_an_open_form_even_over_a_clean_model() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    let a_private = w.sessions.private_directory().expect("A").to_path_buf();
    w.open_height_form();
    let feature = w.feature();
    w.edits.type_height(feature, "21");
    w.open(&b_file);
    let b = w.sessions.tab();
    let form = |w: &Window| crate::form_open(&w.edits, &w.creates.sketch);

    // Close of hidden, clean A with a form: shown first, then refused in words.
    assert!(!w.hidden(a).dirty());
    assert_eq!(
        w.tabs.close_step(&w.sessions, a, form(&w)),
        Some(CloseStep::Show)
    );
    let (generation, rx) = w.begin_switch(a, Some(After::Close));
    let loaded = wait(&rx);
    let (outcome, after) = w
        .deliver_switch(generation, loaded, Ok(()))
        .expect("awaited");
    outcome.expect("shown");
    assert_eq!(after, Some(After::Close));
    assert_eq!(
        w.tabs.close_step(&w.sessions, a, form(&w)),
        Some(CloseStep::Form)
    );
    assert_eq!(w.tabs.count(), 2, "nothing closed");
    assert_eq!(w.edits.typed(), Some((Some(feature), "21")));

    // Quit over clean tabs: the shown form refuses it; a hidden one stops the
    // pass once its tab is shown. Nothing is closed and nothing is applied.
    let quit = |w: &mut Window| {
        crate::begin_window_quit(
            &mut w.tabs,
            &mut w.sessions,
            &w.creates,
            &Loads::default(),
            &Exports::default(),
            &w.edits,
            &w.input,
        )
    };
    assert!(quit(&mut w), "Quit offers the shown saved-object form");
    w.tabs.abort_quit();
    w.round(b);
    assert!(quit(&mut w));
    assert_eq!(w.tabs.quit_step(&w.sessions, form(&w)), QuitStep::Show(a));
    let (generation, rx) = w.begin_switch(a, Some(After::Quit));
    let loaded = wait(&rx);
    let (outcome, after) = w
        .deliver_switch(generation, loaded, Ok(()))
        .expect("awaited");
    outcome.expect("shown");
    assert_eq!(after, Some(After::Quit));
    assert_eq!(w.tabs.quit_step(&w.sessions, form(&w)), QuitStep::Form);
    w.tabs.abort_quit();
    assert_eq!(w.tabs.count(), 2);
    assert_eq!(w.edits.typed(), Some((Some(feature), "21")));
    assert_eq!(height_in(&w.sessions), 12.0, "nothing was applied");

    // An unsaved tab answered Discard does not end the pass before a clean tab
    // whose form is hidden.
    w.round(b);
    w.apply(19.0);
    assert!(quit(&mut w));
    assert_eq!(w.tabs.quit_step(&w.sessions, form(&w)), QuitStep::Ask);
    w.decide_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, form(&w)), QuitStep::Show(a));
    w.tabs.abort_quit();

    // The form done (Cancel), A closes, and takes only its own files with it.
    w.round(a);
    w.edits.cancel();
    assert_eq!(
        w.tabs.close_step(&w.sessions, a, form(&w)),
        Some(CloseStep::Now)
    );
    assert_eq!(w.close(a).expect("closed"), Some(b));
    assert!(!a_private.exists(), "A's draft lease went with A");
    assert!(w.hidden(b).dirty() && w.hidden(b).export_path().is_some());
    println!("\nFCAD_30P_CLOSE_QUIT_FORMS_EXECUTED");
}

/// A hidden tab's version cannot change through the window. If it does, its
/// draft comes back held: its text is kept to be read and cancelled, and no
/// Apply of it is offered or started.
#[test]
fn a_draft_made_on_another_version_comes_back_held_and_is_never_applied() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "27.5");
    w.open(&b_file);

    // Behind the window's back: A's session accepts another version.
    let hidden = w.tabs.hidden_sessions_mut(a).expect("A");
    let (tx, rx) = mpsc::channel();
    let generation = hidden
        .begin_apply(|ticket, _, _| {
            std::thread::spawn(move || {
                tx.send(ticket.edit_extrude_height(
                    feature,
                    20.0,
                    &mut MockKernel::new(),
                    &OperationContext::default(),
                ))
                .expect("edit");
            })
        })
        .expect("started");
    assert!(matches!(
        hidden.finish_apply(generation, wait(&rx)),
        Edited::Show(_)
    ));
    hidden.commit_staged().expect("accepted");
    assert!(hidden.finish_scene(generation, Ok(())));

    w.round(a);
    assert!(w.sessions.stale_draft());
    assert!(
        w.sessions.status.contains("another version"),
        "{}",
        w.sessions.status
    );
    assert_eq!(w.edits.typed(), Some((Some(feature), "27.5")), "kept");
    assert!(!w.sessions.takes_form_apply());
    assert!(!w.apply_height_form(), "no Apply is offered or started");
    assert_eq!(height_in(&w.sessions), 20.0);
    // Cancel ends it; the window's frame then releases the hold.
    w.edits.cancel();
    assert!(!crate::form_open(&w.edits, &w.creates.sketch));
    w.sessions.release_stale_draft();
    w.open_height_form();
    w.edits.type_height(feature, "22");
    assert!(w.apply_height_form());
    assert_eq!(height_in(&w.sessions), 22.0);
    println!("\nFCAD_30P_STALE_DRAFT_EXECUTED");
}

/// Two tabs' height forms have the same controls. Drawn the way the window draws
/// them (`in_tab_scope` with the shown tab's key), keyboard focus in A's field
/// does not carry typing into B's field when B is shown.
#[test]
fn the_same_control_in_two_tabs_shares_no_focus_or_text() {
    let (_root, path, reading) = crate::fillets::tests::plate();
    let feature = reading.features[0].feature;
    let mut a = crate::edits::Edits::default();
    let mut b = crate::edits::Edits::default();
    for edits in [&mut a, &mut b] {
        assert!(edits.begin(&path, &reading));
        edits.type_height(feature, "12");
    }
    let (tab_a, tab_b) = (TabId::default(), TabId::default());
    let ctx = egui::Context::default();
    let frame = |edits: &mut crate::edits::Edits, tab: TabId, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(988., 768.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                crate::in_tab_scope(ui, "tab-height", tab.key(), |ui| {
                    edits.draw_with(ui, false, None, ferritecad_ui::HeightState::default())
                });
            },
        );
        out.textures_delta.clear();
        out
    };
    let out = frame(&mut a, tab_a, vec![]);
    let field = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "12" => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .expect("A's height field");
    for pressed in [true, false] {
        frame(
            &mut a,
            tab_a,
            vec![
                egui::Event::PointerMoved(field),
                egui::Event::PointerButton {
                    pos: field,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    frame(&mut a, tab_a, vec![egui::Event::Text("5".into())]);
    let typed_a = a.typed().expect("A").1.to_owned();
    assert_ne!(typed_a, "12", "A's field took the keyboard");
    // B is shown: its form, drawn where A's was, receives the next keys.
    frame(&mut b, tab_b, vec![]);
    frame(&mut b, tab_b, vec![egui::Event::Text("9".into())]);
    frame(&mut b, tab_b, vec![egui::Event::Text("9".into())]);
    assert_eq!(b.typed(), Some((Some(feature), "12")), "B's text changed");
    assert_eq!(a.typed().expect("A").1, typed_a);
    println!("\nFCAD_30P_FORM_FOCUS_EXECUTED");
}

/// The polygon form of two copies keeps each tab's typed vertices and its own
/// draft Undo and Redo; an Undo in one tab never moves the other's draft.
#[test]
fn vertex_drafts_of_two_copies_keep_their_own_text_and_draft_history() {
    let (root, path, _) = crate::fillets::tests::plate();
    let a_file = root.path().join("a.fcad");
    let b_file = root.path().join("b.fcad");
    std::fs::copy(&path, &a_file).expect("a");
    std::fs::copy(&path, &b_file).expect("b");
    let mut w = Window::new(Drawn::Mock, None);
    let draft = |w: &mut Window, to: &str| {
        let current = w.sessions.export_path().expect("shown");
        let reading = read_extrude_source(&current).expect("reading");
        let sketch = reading.sketches[0].sketch;
        let (editor, _asked) =
            crate::sketch::tests::typed_apply(&current, &reading, sketch, "33", to);
        w.creates.sketch = editor;
        crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("a draft")
    };
    w.open(&a_file);
    let a = w.sessions.tab();
    let a_draft = draft(&mut w, "41.25");
    assert!(a_draft.1 > 0, "typing made draft history");
    w.open(&b_file);
    let b = w.sessions.tab();
    assert_eq!(crate::sketch::tests::vertex_draft(&w.creates.sketch), None);
    let b_draft = draft(&mut w, "44.5");
    assert_ne!(a_draft.0, b_draft.0);
    w.round(a);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(a_draft.clone())
    );
    crate::sketch::tests::press(&mut w.creates.sketch, "Undo draft");
    let undone = crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("A");
    assert_eq!((undone.1, undone.2), (a_draft.1 - 1, a_draft.2 + 1));
    w.round(b);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(b_draft)
    );
    w.round(a);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(undone)
    );
    crate::sketch::tests::press(&mut w.creates.sketch, "Redo draft");
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(a_draft)
    );
    assert!(w.tabs.has_draft(b) && !w.sessions.dirty() && !w.hidden(b).dirty());
    println!("\nFCAD_30P_VERTEX_DRAFTS_EXECUTED");
}

/// Without a kernel, showing a tab is refused at its picture: both tabs' drafts
/// stay exactly where they were (executes only in a build with no kernel).
#[test]
fn stub_a_refused_switch_keeps_both_drafts() {
    if ferritecad_occt::is_available() {
        return;
    }
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "a");
    w.open(&b_file);
    w.open_height_form();
    w.edits.type_height(feature, "b");
    w.drawn = Drawn::Native;
    let (generation, rx) = w.begin_switch(a, None);
    let refused = wait(&rx).map(|_| ()).expect_err("no kernel");
    let (outcome, _) = w
        .deliver_switch(generation, Err(refused), Ok(()))
        .expect("awaited");
    assert!(outcome.is_err());
    assert_eq!(w.edits.typed(), Some((Some(feature), "b")));
    assert!(w.tabs.has_draft(a) && !w.sessions.busy());
    w.drawn = Drawn::Mock;
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(feature), "a")));
    println!("\nFCAD_30P_STUB_DRAFTS_EXECUTED");
}

/// Open CASCADE without the solver: height drafts of two plates are kept through
/// switches drawn by Open CASCADE and applied after returning.
#[test]
fn mixed_drafts_switch_and_apply_with_occt_and_no_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        return;
    }
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Native, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    let feature_a = w.feature();
    w.open_height_form();
    w.edits.type_height(feature_a, "18");
    w.open(&b_file);
    let b = w.sessions.tab();
    let feature_b = w.feature();
    w.open_height_form();
    w.edits.type_height(feature_b, "19");
    w.round(a);
    assert!(!w.scene.catalogue.is_empty(), "A is drawn by Open CASCADE");
    assert_eq!(w.edits.typed(), Some((Some(feature_a), "18")));
    assert!(w.apply_height_form());
    w.round(b);
    assert_eq!(w.edits.typed(), Some((Some(feature_b), "19")));
    assert!(w.apply_height_form());
    assert_eq!(height_in(&w.sessions), 19.0);
    assert_eq!(height_in(w.hidden(a)), 18.0);
    println!("\nFCAD_30P_MIXED_DRAFTS_EXECUTED");
}

// --- native: Open CASCADE and PlaneGCS, against the shipped command line ---------

/// Two real models with their forms left open across switches: A's height (an
/// invalid string first) and then its polygon, B's circle. Each is applied after
/// returning to its tab; A's document Undo/Redo; unsaved exports; Save; every SQL
/// cell, every UUID and reference, the model hash, the exports before and after
/// Save and the geometry compared with the command line doing the same.
#[test]
fn native_drafts_of_two_models_apply_after_returning_and_save_like_the_command_line() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let user = root.path();
    let (plate_root, plate_path, _) = crate::fillets::tests::plate();
    let a_file = user.join("a.fcad");
    std::fs::copy(&plate_path, &a_file).expect("A");
    drop(plate_root);
    let b_file = super::super::analytic::create(user, "create-circle-extrude", CIRCLE_JSON);
    let originals = (
        std::fs::read(&a_file).expect("a"),
        std::fs::read(&b_file).expect("b"),
    );
    let (a_ids, b_ids) = (
        super::super::checkpoints::ids_and_refs(&a_file),
        super::super::checkpoints::ids_and_refs(&b_file),
    );
    let a_reading = read_extrude_source(&a_file).expect("a");
    let feature = a_reading.features[0].feature;
    let sketch = a_reading
        .sketches
        .iter()
        .find(|s| s.refusal.is_none())
        .expect("an editable Sketch")
        .sketch;

    let mut w = Window::new(Drawn::Native, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.open(&b_file);
    let b = w.sessions.tab();
    w.round(a);
    w.open_height_form();
    w.edits.type_height(feature, "9.5x");
    w.round(b);
    let current = w.sessions.export_path().expect("B");
    let reading = read_extrude_source(&current).expect("B");
    let (editor, first) = crate::sketch::tests::analytic_apply::typed_circle(
        &current,
        &reading,
        reading.circle_sketches[0].sketch,
        ["-3.5", "4.25"],
        "8",
    );
    w.creates.sketch = editor;
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(feature), "9.5x")), "literal");
    assert!(!w.apply_height_form(), "the invalid string is not applied");
    w.edits.type_height(feature, "9.5");
    assert!(w.apply_height_form());
    assert_eq!(height_in(&w.sessions), 9.5);
    let current = w.sessions.export_path().expect("A");
    let reading = read_extrude_source(&current).expect("A");
    let (editor, _) = crate::sketch::tests::typed_apply(&current, &reading, sketch, "33", "41.25");
    w.creates.sketch = editor;
    let polygon = crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("A");
    w.round(b);
    assert!(!w.sessions.dirty(), "B's draft is not a change");
    let circle = w.apply_circle_form();
    assert_eq!(circle.edit, first.edit, "B's own draft, as left");
    assert_eq!(circle_of(&w.sessions.export_path().expect("B")).1, 8.0);
    w.round(a);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(polygon)
    );
    let vertices = w.apply_vertices_form();
    assert!(
        !w.creates.sketch.active(),
        "an accepted Apply ends the form"
    );
    // Document Undo and Redo of A's last step, after its form is over.
    let applied = w.sessions.export_path().expect("A");
    w.step(true);
    assert_eq!(w.sessions.export_path().expect("A"), current);
    w.step(false);
    assert_eq!(w.sessions.export_path().expect("A"), applied);
    for (file, original) in [(&a_file, &originals.0), (&b_file, &originals.1)] {
        assert_eq!(&std::fs::read(file).expect("file"), original, "Save only");
    }

    // The command line does the same three things to the untouched files.
    let a_height = user.join("a-height.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        a_file.as_os_str(),
        "--feature".as_ref(),
        feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        a_reading.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "9.5".as_ref(),
        "-o".as_ref(),
        a_height.as_os_str(),
    ]);
    let a_peer = user.join("a-peer.fcad");
    sketch_peer(&a_height, &vertices, user, &a_peer);
    let b_reading = read_extrude_source(&b_file).expect("b");
    let request = user.join("b-request.json");
    std::fs::write(
        &request,
        format!(
            r#"{{"request_version":1,"curve_id":"{}","center_mm":[-3.5,4.25],"radius_mm":8}}"#,
            circle.edit.curve_id
        ),
    )
    .expect("request");
    let b_peer = user.join("b-peer.fcad");
    cli(&[
        "edit-circle".as_ref(),
        b_file.as_os_str(),
        "--sketch".as_ref(),
        b_reading.circle_sketches[0].sketch.to_string().as_ref(),
        "--expect-version".as_ref(),
        b_reading.version.content.to_string().as_ref(),
        "--request".as_ref(),
        request.as_os_str(),
        "-o".as_ref(),
        b_peer.as_os_str(),
    ]);

    // Unsaved exports of each tab's accepted model, by the window's workers.
    let work = tempfile::tempdir().expect("work");
    let alias = w.sessions.suggestion().expect("alias");
    let (a_stl, a_fbx) = export_bytes(&applied, &alias, work.path(), "a-unsaved");
    let (a_peer_stl, a_peer_fbx) = peer_bytes(&a_peer, work.path(), "a");
    assert_eq!((&a_stl, &a_fbx), (&a_peer_stl, &a_peer_fbx), "A unsaved");
    assert!(w.save(SaveTarget::InPlace).published);
    w.round(b);
    let alias = w.sessions.suggestion().expect("alias");
    let (b_stl, b_fbx) = export_bytes(
        &w.sessions.export_path().expect("B"),
        &alias,
        work.path(),
        "b-unsaved",
    );
    let (b_peer_stl, b_peer_fbx) = peer_bytes(&b_peer, work.path(), "b");
    assert_eq!((&b_stl, &b_fbx), (&b_peer_stl, &b_peer_fbx), "B unsaved");
    assert!(w.save(SaveTarget::InPlace).published);
    w.sessions.stop_all();
    w.tabs.stop_all();

    // Every SQL cell but the write stamp; the model hash; ids and references.
    use super::super::checkpoints::all_sql;
    assert_eq!(all_sql(&a_file, &[], false), all_sql(&a_peer, &[], false));
    assert_eq!(all_sql(&b_file, &[], false), all_sql(&b_peer, &[], false));
    let model = |path: &Path| {
        let document = Document::open_read_only(path).expect("document");
        let hash = document.model_version().expect("hash");
        document.close().expect("close");
        hash
    };
    assert_eq!(model(&a_file), model(&a_peer));
    assert_eq!(model(&b_file), model(&b_peer));
    assert_eq!(super::super::checkpoints::ids_and_refs(&a_file), a_ids);
    assert_eq!(super::super::checkpoints::ids_and_refs(&b_file), b_ids);

    // Exports after Save are the exports before it; geometry read independently.
    assert_eq!(
        peer_bytes(&a_file, work.path(), "a-saved"),
        (a_stl.clone(), a_fbx.clone())
    );
    assert_eq!(
        peer_bytes(&b_file, work.path(), "b-saved"),
        (b_stl.clone(), b_fbx.clone())
    );
    let area = {
        let p: Vec<[f64; 2]> = vertices.vertices.iter().map(|v| v.start_mm).collect();
        (0..p.len())
            .map(|i| {
                let (s, e) = (p[i], p[(i + 1) % p.len()]);
                s[0] * e[1] - e[0] * s[1]
            })
            .sum::<f64>()
            .abs()
            / 2.0
    };
    let (_, height_a, volume_a) = super::super::checkpoints::stl_facts(&a_stl);
    assert!((height_a - 9.5).abs() < 1e-4, "{height_a}");
    assert!(
        (volume_a - area * 9.5).abs() < 1e-2,
        "{volume_a} vs {}",
        area * 9.5
    );
    super::super::analytic::assert_round(&b_stl, [-3.5, 4.25], 8.0, None, 15.25);
    let (_, height_b, volume_b) = super::super::checkpoints::stl_facts(&b_stl);
    if let Ok(dir) = std::env::var("FCAD_TAB_DRAFTS_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        for (name, stl, fbx) in [("drafts-a", &a_stl, &a_fbx), ("drafts-b", &b_stl, &b_fbx)] {
            std::fs::write(dir.join(format!("{name}.stl")), stl).expect("artifact");
            std::fs::write(dir.join(format!("{name}.fbx")), fbx).expect("artifact");
        }
    }
    println!(
        "\nFCAD_30P_NATIVE_DRAFTS_EXECUTED volume_a={volume_a:.3} height_b={height_b:.3} \
         volume_b={volume_b:.3}"
    );
}

/// Two copies of a dimensioned plate, each with a *Replace length* draft — one
/// pending removal and one addition over the same stored ids — kept across
/// switches and applied after returning, each to its own tab, like the command
/// line (the newly added constraint ids are random: they are the only cells set
/// aside, and only by the existing explicit-new-id comparison).
#[test]
fn native_replace_length_drafts_of_two_copies_apply_to_their_own_tab_like_the_command_line() {
    if !native() {
        return;
    }
    if !ferritecad_sketch_solver::is_available() {
        assert_ne!(
            std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
            Ok("1")
        );
        eprintln!("skipped: constraints need PlaneGCS");
        return;
    }
    let (root, path, source) = crate::fillets::tests::dimensioned();
    let a_file = root.path().join("a.fcad");
    let b_file = root.path().join("b.fcad");
    std::fs::copy(&path, &a_file).expect("a");
    std::fs::copy(&path, &b_file).expect("b");
    let sketch = source.constraint_sketches[0].sketch;
    let old: Vec<_> = stored_constraints(&a_file).iter().map(|c| c.id).collect();
    let mut w = Window::new(Drawn::Native, None);
    let draft = |w: &mut Window, length: &str| {
        let current = w.sessions.export_path().expect("shown");
        let reading = read_extrude_source(&current).expect("reading");
        let (editor, _) = crate::constraints::tests::session_apply::typed_replace_length(
            &current, &reading, sketch, 2, length,
        );
        w.creates.sketch.constraints = editor;
        w.creates.sketch.constraints.draft_edits().expect("a draft")
    };
    w.open(&a_file);
    let a = w.sessions.tab();
    let a_edits = draft(&mut w, "38.5");
    assert_eq!((a_edits.remove.len(), a_edits.add.len()), (1, 1));
    w.open(&b_file);
    let b_edits = draft(&mut w, "44");
    assert_eq!(
        a_edits.remove, b_edits.remove,
        "one stored id in both copies"
    );
    assert_ne!(a_edits.add, b_edits.add);
    w.round(a);
    assert_eq!(w.creates.sketch.constraints.draft_edits(), Some(a_edits));
    let a_request = w.apply_constraints_form();
    assert!(
        w.sessions.dirty()
            && !w
                .tabs
                .hidden_sessions(w.tabs.order()[1])
                .expect("B")
                .dirty()
    );
    let b = w.tabs.order()[1];
    w.round(b);
    assert_eq!(w.creates.sketch.constraints.draft_edits(), Some(b_edits));
    let b_request = w.apply_constraints_form();
    assert!(w.save(SaveTarget::InPlace).published);
    w.round(a);
    assert!(w.save(SaveTarget::InPlace).published);
    w.sessions.stop_all();
    w.tabs.stop_all();
    for (file, request, tag) in [(&a_file, &a_request, "a"), (&b_file, &b_request, "b")] {
        let peer = root.path().join(format!("{tag}-peer.fcad"));
        constraint_peer(&path, request, root.path(), &peer);
        let added = same_model_with_explicit_new_ids(file, &peer, sketch, &old, tag);
        assert_eq!(added.len(), 1, "{tag}: one new constraint");
    }
    println!("\nFCAD_30P_NATIVE_CONSTRAINT_DRAFTS_EXECUTED");
}

// --- the real window's artifacts --------------------------------------------------

/// The command line's copies of what the §30P recipe makes of each input: A at
/// height 26 with its two `80` vertex coordinates made `90`; B's circle as §30O's.
fn draft_peers(inputs: &Path, work: &Path) -> (PathBuf, PathBuf) {
    let (a_height, b_peer) = super::peers(inputs, work);
    let reading = read_extrude_source(&a_height).expect("a");
    let choice = reading
        .sketches
        .iter()
        .find(|s| s.refusal.is_none())
        .expect("an editable Sketch");
    let mut vertices = choice.vertices.clone().expect("vertices");
    for vertex in &mut vertices {
        if vertex.start_mm[0] == 80.0 {
            vertex.start_mm[0] = 90.0;
        }
    }
    let request = ferritecad_jobs::EditSketchRequest {
        source: a_height.clone(),
        expected: reading.version,
        sketch: choice.sketch,
        vertices,
        destination: PathBuf::new(),
    };
    let a_peer = work.join("a-draft-peer.fcad");
    sketch_peer(&a_height, &request, work, &a_peer);
    (a_peer, b_peer)
}

/// Compares the outputs of the §30P window recipe in `root` with the shipped
/// command line. Reads only what the window wrote; refuses a missing output
/// before any peer job.
fn compare_drafts_gui(root: &Path) {
    use super::super::checkpoints::{all_sql, ids_and_refs, stl_facts};
    let missing: Vec<&str> = GUI_OUTPUTS
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
    let work = tempfile::tempdir().expect("work");
    let (a_peer, b_peer) = draft_peers(&root.join("inputs"), work.path());
    let (a, b) = (root.join("a.fcad"), root.join("b.fcad"));
    // Every SQL cell but the write stamp: only applied forms reached the files.
    assert!(
        all_sql(&a, &[], false) == all_sql(&a_peer, &[], false),
        "a.fcad is not the command line's model"
    );
    assert!(
        all_sql(&b, &[], false) == all_sql(&b_peer, &[], false),
        "b.fcad is not the command line's model"
    );
    assert!(
        ids_and_refs(&a) == ids_and_refs(&root.join("inputs/a.fcad")),
        "a.fcad identities"
    );
    assert!(
        ids_and_refs(&b) == ids_and_refs(&root.join("inputs/b.fcad")),
        "b.fcad identities"
    );
    let (a_stl, a_fbx) = peer_bytes(&a_peer, work.path(), "a");
    let (b_stl, b_fbx) = peer_bytes(&b_peer, work.path(), "b");
    assert!(
        read("a-unsaved.stl") == a_stl,
        "a-unsaved.stl against the CLI"
    );
    assert!(
        read("a-unsaved.fbx") == a_fbx,
        "a-unsaved.fbx against the CLI"
    );
    assert!(
        read("b-unsaved.stl") == b_stl,
        "b-unsaved.stl against the CLI"
    );
    assert!(
        read("b-unsaved.fbx") == b_fbx,
        "b-unsaved.fbx against the CLI"
    );
    // Exports of the saved files are the unsaved ones; geometry read independently.
    assert!(
        peer_bytes(&a, work.path(), "a-saved") == (a_stl.clone(), a_fbx),
        "a.fcad exports differ from the unsaved ones"
    );
    assert!(
        peer_bytes(&b, work.path(), "b-saved") == (b_stl.clone(), b_fbx),
        "b.fcad exports differ from the unsaved ones"
    );
    let (_, height, volume) = stl_facts(&a_stl);
    assert!((height - 26.0).abs() < 1e-4 && (volume - 90.0 * 40.0 * 26.0).abs() < 1e-2);
    super::super::analytic::assert_round(&b_stl, [-3.5, 4.25], 8.0, None, 15.25);
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let listing = store.list().expect("list");
    assert!(
        listing.recoverable().count() == 0 && listing.active == 0,
        "a recovery record was left behind"
    );
}

/// The comparator's positive run and its six negative controls, each on a copy.
fn compare_drafts_with_controls(root: &Path) {
    compare_drafts_gui(root);
    let control = |name: &str, why: &str, breaks: &dyn Fn(&Path)| {
        super::control_of(compare_drafts_gui, root, name, why, breaks);
    };
    let before = cli_runs();
    control("missing", "missing real GUI output: a-unsaved.stl", &|r| {
        std::fs::remove_file(r.join("a-unsaved.stl")).expect("remove");
    });
    assert_eq!(cli_runs(), before, "a peer job ran before a missing output");
    let file = |r: &Path, name: &str| std::fs::read(r.join(name)).expect(name);
    control("unsaved b", "b.fcad was never saved", &|r| {
        std::fs::write(r.join("b.fcad"), file(r, "inputs/b.fcad")).expect("w");
    });
    control(
        "a saved as b",
        "a.fcad is not the command line's model",
        &|r| {
            std::fs::write(r.join("a.fcad"), file(r, "b.fcad")).expect("w");
        },
    );
    // The height form left open at Quit (30) must never reach the file.
    control(
        "open form applied",
        "a.fcad is not the command line's model",
        &|r| {
            let work = tempfile::tempdir().expect("work");
            let (a_peer, _) = draft_peers(&r.join("inputs"), work.path());
            let reading = read_extrude_source(&a_peer).expect("peer");
            let applied = work.path().join("a-30.fcad");
            cli(&[
                "edit-extrude".as_ref(),
                a_peer.as_os_str(),
                "--feature".as_ref(),
                reading.features[0].feature.to_string().as_ref(),
                "--expect-version".as_ref(),
                reading.version.content.to_string().as_ref(),
                "--distance-mm".as_ref(),
                "30".as_ref(),
                "-o".as_ref(),
                applied.as_os_str(),
            ]);
            std::fs::write(r.join("a.fcad"), std::fs::read(applied).expect("30")).expect("w");
        },
    );
    control("exports swapped", "b-unsaved.fbx against the CLI", &|r| {
        std::fs::write(r.join("b-unsaved.fbx"), file(r, "a-unsaved.fbx")).expect("w");
    });
    control("record left", "a recovery record was left behind", &|r| {
        let store = RecoveryStore::open(&r.join("recovery")).expect("store");
        let private = tempfile::tempdir().expect("private");
        let mut session = DocumentSession::create_document_in(
            private.path(),
            HistoryLimits::default(),
            NewDocument::Empty,
            || Err::<MockKernel, _>(CadError::unsupported("not needed")),
            &OperationContext::default(),
        )
        .expect("untitled");
        let mut recorder = RecoveryRecorder::start(store, || {});
        recorder.observe(&mut session);
        let deadline = Instant::now() + Duration::from_secs(60);
        while !recorder.settled() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(recorder);
    });
}

/// The real window's outputs in `FCAD_30P_GUI_DIR` against the command line, then
/// six negative controls on copies of those same outputs. Nothing here makes a
/// window output; without the variable the test does nothing.
#[test]
fn native_compare_real_tab_drafts_gui_artifacts_with_negative_controls() {
    let Some(root) = std::env::var_os("FCAD_30P_GUI_DIR") else {
        return;
    };
    assert!(native(), "the comparison needs Open CASCADE");
    compare_drafts_with_controls(Path::new(&root));
    println!("\nFCAD_30P_GUI_COMPARE_OK negative_controls=6 all_SQL_cells=true");
}

/// The comparator's own check, not window evidence: the recipe's steps run on the
/// window's owners without a window (the same `Tabs`, `Sessions`, forms, workers
/// and arrival statement); their files go through the comparator and its controls.
#[test]
fn native_tab_drafts_scenario_on_session_files_passes_the_comparator_and_its_controls() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let root = root.path();
    make_inputs(root);
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let mut w = Window::new(Drawn::Native, Some(&store));
    let (a_file, b_file) = (root.join("a.fcad"), root.join("b.fcad"));
    // 1: A, then B in a second tab.
    w.open(&a_file);
    let a = w.sessions.tab();
    assert_eq!(w.open(&b_file), Opening::Load);
    let b = w.sessions.tab();
    // 2: A's height form: `2..6`, not applied.
    w.round(a);
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "2..6");
    // 3: B's circle form: centre −3.5, 4.25, radius 8, not applied.
    w.round(b);
    let current = w.sessions.export_path().expect("B");
    let reading = read_extrude_source(&current).expect("B");
    let (editor, _) = crate::sketch::tests::analytic_apply::typed_circle(
        &current,
        &reading,
        reading.circle_sketches[0].sketch,
        ["-3.5", "4.25"],
        "8",
    );
    w.creates.sketch = editor;
    // 4: A shows `2..6` as typed; 26; Apply.
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(feature), "2..6")));
    w.edits.type_height(feature, "26");
    assert!(w.apply_height_form());
    // 5: A's vertices: both `80` made `90`, not applied.
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
    // 6: B: Apply circle.
    w.round(b);
    w.apply_circle_form();
    // 7: A: Apply vertices; Undo; Redo; unsaved exports.
    w.round(a);
    w.apply_vertices_form();
    w.step(true);
    w.step(false);
    let alias = w.sessions.suggestion().expect("alias");
    export_bytes(
        &w.sessions.export_path().expect("A"),
        &alias,
        root,
        "a-unsaved",
    );
    // 8: A's height form: 30, left open.
    w.open_height_form();
    w.edits.type_height(feature, "30");
    // 9: B: unsaved exports.
    w.round(b);
    let alias = w.sessions.suggestion().expect("alias");
    export_bytes(
        &w.sessions.export_path().expect("B"),
        &alias,
        root,
        "b-unsaved",
    );
    // 10: Quit: B asked → Save; A shown; its open form stops the pass.
    let form = |w: &Window| crate::form_open(&w.edits, &w.creates.sketch);
    let quit = |w: &mut Window| {
        crate::begin_window_quit(
            &mut w.tabs,
            &mut w.sessions,
            &w.creates,
            &Loads::default(),
            &Exports::default(),
            &w.edits,
            &w.input,
        )
    };
    assert!(quit(&mut w));
    assert_eq!(w.tabs.quit_step(&w.sessions, form(&w)), QuitStep::Ask);
    assert!(w.save(SaveTarget::InPlace).published);
    assert_eq!(w.tabs.quit_step(&w.sessions, form(&w)), QuitStep::Show(a));
    w.show_for_quit(a);
    assert_eq!(w.tabs.quit_step(&w.sessions, form(&w)), QuitStep::Form);
    w.tabs.abort_quit();
    assert_eq!(w.tabs.order(), [a, b]);
    // 11: Cancel A's form; Quit: A asked → Save; the window ends.
    w.edits.cancel();
    assert!(quit(&mut w));
    assert_eq!(w.tabs.quit_step(&w.sessions, form(&w)), QuitStep::Ask);
    assert!(w.save(SaveTarget::InPlace).published);
    assert_eq!(w.tabs.quit_step(&w.sessions, form(&w)), QuitStep::Exit);
    w.tabs.decide_exit(&mut w.sessions);
    w.sessions.stop_all();
    w.tabs.stop_all();
    compare_drafts_with_controls(root);
    println!("\nFCAD_30P_SESSION_FILES_COMPARE_OK negative_controls=6 all_SQL_cells=true");
}
