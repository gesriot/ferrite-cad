// SPDX-License-Identifier: MIT
//! §30Q: Open, New and Recover beside the shown tab's unfinished forms. Through the
//! window's own owners — the reading's `Loads` (generations, a newer Open, Cancel),
//! `Creates` with its form and drawing, `Recoveries` with the store, `Tabs`, each
//! tab's `Sessions`, the window's `Edits` and `sketch::Editor`, the arrival
//! statement `present`, and the predicates the buttons and handlers share —
//! without a graphics device.

use super::*;
use crate::recoveries::Recoveries;
use crate::{LoadGeneration, ProgressRelay};
use ferritecad_jobs::RecordId;

impl Window {
    /// `App::read_document`: a reading of a file, numbered by the window's `Loads`.
    pub(super) fn begin_load(loads: &mut Loads, path: &Path) -> LoadGeneration {
        Self::begin_watched_load(loads, path).0
    }

    /// The same, with the token the reading's worker is told to stop by.
    fn begin_watched_load(loads: &mut Loads, path: &Path) -> (LoadGeneration, CancelToken) {
        let mut token = None;
        let generation = loads
            .open(
                Some(path),
                Arc::new(ProgressRelay::default()),
                |_, cancel| {
                    token = Some(cancel.clone());
                    std::thread::spawn(|| {})
                },
            )
            .expect("a reading starts");
        (generation, token.expect("the worker was given its token"))
    }

    /// What the reading worker answers for `path`: its session and its picture
    /// (on the mock kernel here; `open_for_view` in the kernel gates).
    pub(super) fn read(&self, path: &Path) -> Result<(LoadedScene, DocumentSession)> {
        let session =
            DocumentSession::open_in(self.private.path(), path, HistoryLimits::default())?;
        let scene = self.picture(session.current().path())?;
        Ok((scene, session))
    }

    /// The window's `Loaded` handler and `App::show` without the device: the
    /// worker's answer, accepted with its picture only when its generation is the
    /// one `loads` waits for. Whether a tab was added.
    pub(super) fn deliver_load(
        &mut self,
        loads: &mut Loads,
        generation: LoadGeneration,
        read: Result<(LoadedScene, DocumentSession)>,
        upload: Result<()>,
    ) -> bool {
        let awaited = loads.accepted_path(generation).is_some();
        let outcome = match (awaited, read) {
            (true, Ok((scene, session))) => {
                let shown = session.current().path().to_path_buf();
                self.arrive(&shown, Ok(scene), Bind::Open(Box::new(session)), upload)
            }
            (true, Err(error)) => Err(error),
            // Nobody waits for it: dropping the session removes its files.
            (false, read) => read.map(|_| ()),
        };
        let shown = awaited && outcome.is_ok();
        crate::finish_answer(loads, &mut self.input, generation, outcome);
        shown
    }

    /// `App::show`: the arrival statement, then what the forms are told.
    fn arrive(
        &mut self,
        document: &Path,
        loaded: Result<LoadedScene>,
        bind: Bind,
        upload: Result<()>,
    ) -> Result<()> {
        let outcome = self.present(document, loaded, bind, upload);
        self.creates
            .sketch
            .draft_load_finished(document, outcome.is_ok());
        self.edits.draft_load_finished(document, outcome.is_ok());
        outcome
    }

    /// `App::begin_new`: New's form (or, `drawing`, the drawing) over the shown
    /// tab, after the predicate its buttons ask. Whether New opened.
    pub(super) fn begin_new(&mut self, drawing: bool) -> bool {
        if !self.can_leave() {
            return false;
        }
        crate::open_new(
            &mut self.tabs,
            &mut self.sessions,
            &mut self.edits,
            &mut self.creates,
            &mut self.input,
            |creates, input| {
                if drawing {
                    creates.begin_drawing();
                    true
                } else {
                    crate::ask_new(creates, &Loads::default(), &Exports::default(), input)
                }
            },
        )
    }

    /// The end of a frame in which New may have closed (`App::end_new`).
    fn end_new(&mut self) {
        crate::end_new(
            &mut self.tabs,
            &mut self.sessions,
            &mut self.edits,
            &mut self.creates,
        );
    }

    /// The New form answered (`creates::answer_form` and what follows it).
    pub(super) fn answer_new(&mut self, choice: ferritecad_ui::NewChoice) -> Option<NewDocument> {
        let content = crate::creates::answer_form(&mut self.creates, &mut self.input, choice);
        self.end_new();
        content
    }

    /// `App::create_new` and the `Created` handler: the room check first, then
    /// the window's creation worker; the candidate is accepted with its picture.
    fn create_from_new(&mut self, content: NewDocument, upload: Result<()>) -> Result<()> {
        self.tabs.room().map_err(CadError::input)?;
        let (_, candidate) = crate::creates::tests::run_to_completion(
            &mut self.creates,
            &mut self.input,
            self.private.path(),
            content,
        );
        let Some(candidate) = candidate else {
            return Err(CadError::input("nothing was made"));
        };
        self.bind_candidate(candidate, upload)
    }

    /// The `Created` handler for a candidate made: shown, then New is told.
    pub(super) fn bind_candidate(
        &mut self,
        candidate: crate::creates::Candidate,
        upload: Result<()>,
    ) -> Result<()> {
        let crate::creates::Candidate { scene, session } = candidate;
        let shown = session.current().path().to_path_buf();
        let outcome = self.arrive(&shown, Ok(scene), Bind::Open(Box::new(session)), upload);
        crate::creates::finish_shown(
            &mut self.creates,
            &mut self.input,
            outcome.as_ref().copied().map_err(ToString::to_string),
        );
        self.end_new();
        outcome
    }

    /// `App::recover` up to its worker: the room first, then the claim's worker
    /// holds the shown tab's operation slot.
    fn begin_recovery(&mut self, recoveries: &mut Recoveries, record: RecordId) -> Option<u64> {
        self.tabs.room().ok()?;
        let sessions = &mut self.sessions;
        recoveries.begin(record, |generation, _| {
            sessions.hold_recovery(generation);
            std::thread::spawn(|| {})
        })
    }

    /// The window's `Recovered` handler without the device: an answer nobody waits
    /// for is dropped (with its claim); a cancelled one is refused; otherwise the
    /// copy becomes a tab with its picture. Whether a tab was added.
    fn deliver_recovery(
        &mut self,
        recoveries: &mut Recoveries,
        generation: u64,
        result: Result<(LoadedScene, DocumentSession)>,
        upload: Result<()>,
    ) -> bool {
        if !recoveries.accepts(generation) {
            return false;
        }
        let outcome = if !self.sessions.finish_recovery(generation) {
            Err(CadError::input(
                "Recovery was cancelled; its copy was kept.",
            ))
        } else {
            match result {
                Ok((scene, session)) => {
                    let shown = session.current().path().to_path_buf();
                    self.arrive(&shown, Ok(scene), Bind::Open(Box::new(session)), upload)
                }
                Err(error) => Err(error),
            }
        };
        let shown = outcome.is_ok();
        recoveries.finish(generation, outcome.map_err(|error| error.to_string()));
        shown
    }
}

/// The recovery worker's answer: the production route where this build has Open
/// CASCADE, and otherwise the same claim and session with its picture read
/// through the mock kernel, so the window's handling is exercised on every build.
fn recovered(
    private: &Path,
    store: &RecoveryStore,
    record: RecordId,
) -> Result<(LoadedScene, DocumentSession)> {
    recovered_unless(private, store, record, CancelToken::new())
}

/// The same worker, stoppable while it waits for a reader of its record (§30S).
fn recovered_unless(
    private: &Path,
    store: &RecoveryStore,
    record: RecordId,
    cancel: CancelToken,
) -> Result<(LoadedScene, DocumentSession)> {
    if ferritecad_occt::is_available() {
        let context = OperationContext::default().with_cancel(cancel);
        return recover_for_view(private, store, record, &context);
    }
    let claim = store.claim_cancellable(record, &cancel)?;
    let session = DocumentSession::recover_in(private, HistoryLimits::default(), claim)?;
    let scene = mock_scene(session.current().path())?;
    Ok((scene, session))
}

/// A crash copy nobody holds: `file` changed to `millimetres` high and published,
/// then its process "ended" (the record's lease let go).
pub(super) fn orphan(
    store: &RecoveryStore,
    file: &Path,
    private: &Path,
    millimetres: f64,
) -> RecordId {
    let mut session =
        DocumentSession::open_in(private, file, HistoryLimits::default()).expect("session");
    let feature = read_extrude_source(session.current().path())
        .expect("reading")
        .features[0]
        .feature;
    let step = session
        .begin_step()
        .edit_extrude_height(
            feature,
            millimetres,
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("edit");
    session.commit_step(step).expect("accepted");
    let mut record = store.create_record().expect("record");
    record
        .publish(1, &session.current(), &session.recovery_name())
        .expect("copy");
    let id = record.id();
    drop(record);
    id
}

fn recoverable(store: &RecoveryStore) -> Vec<RecordId> {
    store
        .list()
        .expect("list")
        .recoverable()
        .map(|summary| summary.record)
        .collect()
}

/// Every input file's bytes, to show nothing but Save ever writes one.
pub(super) fn contents(files: &[&Path]) -> Vec<Vec<u8>> {
    files
        .iter()
        .map(|file| std::fs::read(file).expect("input"))
        .collect()
}

/// Open beside an unfinished form: a literal invalid height in A stays with A
/// through a newer Open, a Cancel, an unreadable file, a picture the device
/// refuses and an accepted Open of a second copy of A (one `DocumentId`); that
/// copy's vertex draft and its draft Undo/Redo stay with it; B's document Undo
/// moves B only; a file a tab names (by another name) shows its tab, also with
/// the window full, and a ninth file is refused before anything is read.
#[test]
fn open_over_unfinished_forms_keeps_them_with_their_tab_through_every_answer() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("plate.fcad");
    plate(&a_file, 12.0);
    let copy = user.path().join("copy.fcad");
    std::fs::copy(&a_file, &copy).expect("copy");
    assert_eq!(document_id(&a_file), document_id(&copy));
    let b_file = user.path().join("b.fcad");
    plate(&b_file, 15.0);
    let broken = user.path().join("broken.fcad");
    std::fs::write(&broken, b"not a document").expect("broken");
    let alias = user.path().join("alias.fcad");
    std::fs::hard_link(&a_file, &alias).expect("another name of A's file");
    let inputs = [a_file.as_path(), &copy, &b_file];
    let originals = contents(&inputs);

    let mut w = Window::new(Drawn::Mock, None);
    let mut loads = Loads::default();
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(14.0);
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "2..6");
    let a_shown = |w: &Window| {
        assert_eq!(w.sessions.tab(), a, "A stays shown");
        assert_eq!(w.edits.typed(), Some((Some(feature), "2..6")), "literal");
        assert!(w.sessions.dirty() && w.sessions.can_undo());
        assert_eq!(height_in(&w.sessions), 14.0);
    };

    // The toolbar's Open and its handler ask one predicate: an idle form does not
    // hold it, while an operation or a gesture still does.
    let open = |w: &Window| crate::may_leave_tab(&w.creates, &w.edits, &w.sessions, &w.input);
    assert!(open(&w) && w.can_leave());
    let running = w
        .sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("an Apply");
    assert!(!open(&w), "an Apply holds the window");
    assert_eq!(
        w.sessions.finish_apply(running, Err(CadError::Cancelled)),
        Edited::Failed
    );
    w.input
        .handle(ViewportEvent::PointerMoved { x: 10.0, y: 10.0 }, false);
    w.input
        .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
    assert!(!open(&w), "a camera gesture holds it");
    w.input.handle(ViewportEvent::GestureCancelled, false);
    assert!(open(&w));

    // A newer Open replaces one in flight: the older worker is told to stop at
    // once, and its answer, arriving late, adds nothing.
    let (first, stopped) = Window::begin_watched_load(&mut loads, &b_file);
    assert!(!stopped.is_cancelled());
    let second = Window::begin_load(&mut loads, &copy);
    assert!(stopped.is_cancelled(), "the replaced reading was retired");
    assert!(!w.deliver_load(&mut loads, first, w.read(&b_file), Ok(())));
    a_shown(&w);
    // Cancel before the answer: the same.
    assert!(crate::cancel_load(&mut loads, &mut w.input));
    assert!(!w.deliver_load(&mut loads, second, w.read(&copy), Ok(())));
    a_shown(&w);
    // An unreadable file; a picture the device refuses.
    let reading = Window::begin_load(&mut loads, &broken);
    assert!(!w.deliver_load(&mut loads, reading, w.read(&broken), Ok(())));
    a_shown(&w);
    let reading = Window::begin_load(&mut loads, &b_file);
    assert!(!w.deliver_load(
        &mut loads,
        reading,
        w.read(&b_file),
        Err(CadError::rendering("no device"))
    ));
    a_shown(&w);
    assert_eq!(w.tabs.count(), 1, "no empty tab was left");

    // Accepted: the copy (A's DocumentId) is a new tab with no form; A is hidden
    // with its form, its unsaved model and its history.
    let reading = Window::begin_load(&mut loads, &copy);
    assert!(w.deliver_load(&mut loads, reading, w.read(&copy), Ok(())));
    let c = w.sessions.tab();
    assert_ne!(c, a);
    assert_eq!(w.edits.typed(), None, "A's form came along");
    assert!(!w.creates.sketch.active() && !w.sessions.dirty());
    assert!(w.tabs.has_draft(a) && w.hidden(a).dirty() && w.hidden(a).can_undo());
    assert_eq!(height_in(w.hidden(a)), 14.0);
    // The copy's own vertex draft, with draft history both ways.
    let current = w.sessions.export_path().expect("C");
    let reading = read_extrude_source(&current).expect("C");
    let (editor, _) = crate::sketch::tests::typed_apply(
        &current,
        &reading,
        reading.sketches[0].sketch,
        "80",
        "90",
    );
    w.creates.sketch = editor;
    let typed = crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("C");
    crate::sketch::tests::press(&mut w.creates.sketch, "Undo draft");
    let c_draft = crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("C");
    assert_eq!((c_draft.1, c_draft.2), (typed.1 - 1, typed.2 + 1));

    // B over the copy's form; B's document Undo and Redo move B only.
    let reading = Window::begin_load(&mut loads, &b_file);
    assert!(w.deliver_load(&mut loads, reading, w.read(&b_file), Ok(())));
    let b = w.sessions.tab();
    assert!(w.tabs.has_draft(c) && w.tabs.has_draft(a));
    w.apply(16.0);
    w.step(true);
    assert_eq!(height_in(&w.sessions), 15.0);
    w.step(false);
    assert_eq!(height_in(&w.sessions), 16.0);
    assert!(!w.hidden(c).dirty() && w.hidden(a).dirty());
    assert_eq!(height_in(w.hidden(a)), 14.0);

    // A file a tab names, by another name: its tab is shown with its form as left.
    assert_eq!(w.tabs.opening(&w.sessions, &alias), Opening::Show(a));
    w.switch(a);
    a_shown(&w);
    assert!(w.sessions.takes_form_apply(), "the same version: not held");
    assert_eq!(w.tabs.opening(&w.sessions, &a_file), Opening::Shown);
    a_shown(&w);

    // A full window: a ninth file is refused before it is read, and a reading that
    // got through anyway is refused at its bind; a named file still shows its tab.
    for n in 0..(MAX_TABS - 3) {
        let more = user.path().join(format!("more-{n}.fcad"));
        std::fs::copy(&b_file, &more).expect("more");
        assert_eq!(w.open(&more), Opening::Load);
    }
    assert_eq!(w.tabs.count(), MAX_TABS);
    let last = w.sessions.tab();
    let last_feature = w.feature();
    w.open_height_form();
    w.edits.type_height(last_feature, " 33.0 ");
    let ninth = user.path().join("ninth.fcad");
    plate(&ninth, 9.0);
    assert!(matches!(
        w.tabs.opening(&w.sessions, &ninth),
        Opening::Refused(_)
    ));
    let reading = Window::begin_load(&mut loads, &ninth);
    assert!(!w.deliver_load(&mut loads, reading, w.read(&ninth), Ok(())));
    assert_eq!(w.tabs.count(), MAX_TABS);
    assert_eq!(w.edits.typed(), Some((Some(last_feature), " 33.0 ")));
    assert_eq!(w.tabs.opening(&w.sessions, &alias), Opening::Show(a));
    w.switch(a);
    a_shown(&w);
    assert!(w.tabs.has_draft(last));

    // The copy's draft and its history came back as they were left.
    w.switch(c);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(c_draft)
    );
    crate::sketch::tests::press(&mut w.creates.sketch, "Redo draft");
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(typed)
    );

    // Back in A: the literal text is no request; 26 is applied to A only.
    w.switch(a);
    assert!(!w.apply_height_form());
    w.edits.type_height(feature, "26");
    assert!(w.apply_height_form());
    assert_eq!(height_in(&w.sessions), 26.0);
    assert_eq!(height_in(w.hidden(c)), 12.0);
    assert_eq!(height_in(w.hidden(b)), 16.0);
    assert_eq!(contents(&inputs), originals, "only Save writes a file");
    loads.stop_all();
    println!("\nFCAD_30Q_OPEN_OVER_FORMS_EXECUTED");
}

/// New beside unfinished forms: A's constraints draft (with its request history)
/// is set aside — not overwritten — while New's form or drawing is open; the
/// window is held, and Close and Quit stop at the forms set aside. A size that is
/// not a number, a document the kernel refuses, a picture the device refuses and
/// a full window keep New's typed values and A's forms aside; Cancel gives them
/// back exactly. An accepted New is a separate Untitled tab, and A is hidden with
/// its forms, its unsaved model and its history.
#[test]
fn new_over_unfinished_forms_sets_them_aside_and_gives_them_back_exactly() {
    use ferritecad_ui::{NewChoice, NewContent};
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("plate.fcad");
    plate(&a_file, 12.0);
    let originals = contents(&[&a_file]);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(14.0);
    w.constraints_draft(1, 3);
    crate::constraints::tests::session_apply::press(&mut w.creates.sketch.constraints, "Undo");
    let a_edits = w.creates.sketch.constraints.draft_edits().expect("A");
    let a_state = w.creates.sketch.constraints.draft_state().expect("A");
    assert!(a_state.4 > 0, "a request Redo to keep");
    let a_back = |w: &Window| {
        assert_eq!(w.sessions.tab(), a);
        assert!(!w.tabs.has_aside());
        assert_eq!(
            w.creates.sketch.constraints.draft_edits(),
            Some(a_edits.clone())
        );
        assert_eq!(w.creates.sketch.constraints.draft_state(), Some(a_state));
        assert!(w.sessions.dirty() && w.sessions.can_undo());
        assert!(w.sessions.takes_form_apply());
    };
    let set_aside = |w: &Window| {
        assert!(w.tabs.has_aside());
        assert!(!w.creates.sketch.saved_object_open() && !w.edits.form_open());
        assert_eq!(w.tabs.count(), 1);
    };

    // New waits while an operation, a gesture, an export or its question holds
    // the window, in the predicate its buttons and its handler share.
    let running = w
        .sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("an Apply");
    assert!(!w.begin_new(false) && !w.begin_new(true));
    assert_eq!(
        w.sessions.finish_apply(running, Err(CadError::Cancelled)),
        Edited::Failed
    );
    w.input
        .handle(ViewportEvent::PointerMoved { x: 10.0, y: 10.0 }, false);
    w.input
        .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
    assert!(
        !w.begin_new(false) && !w.begin_new(true),
        "a camera gesture"
    );
    w.input.handle(ViewportEvent::GestureCancelled, false);
    let new_allowed = |w: &Window, exports: &Exports| {
        crate::can_leave_tab(
            &w.creates,
            &Loads::default(),
            exports,
            &w.edits,
            &w.sessions,
            &w.input,
        )
    };
    let mut exporting = Exports::default();
    let source = w.sessions.export_path().expect("A");
    crate::exports::begin_export(
        &mut exporting,
        &mut w.input,
        Some(&source),
        Some(user.path().join("out.fbx")),
        |_, _, _, _| std::thread::spawn(|| {}),
    )
    .expect("an export");
    assert!(!new_allowed(&w, &exporting), "an export holds New");
    crate::exports::cancel_export(&mut exporting, &mut w.input);
    std::fs::write(user.path().join("taken.fbx"), b"existing").expect("taken");
    crate::exports::begin_export(
        &mut exporting,
        &mut w.input,
        Some(&source),
        Some(user.path().join("taken.fbx")),
        |_, _, _, _| panic!("replaced without asking"),
    );
    assert!(exporting.pending().is_some());
    assert!(!new_allowed(&w, &exporting), "so does its question");
    exporting.stop_all();
    assert!(new_allowed(&w, &Exports::default()));
    a_back(&w);

    // New's form over A's forms: set aside; New starts on empty forms.
    assert!(w.begin_new(false));
    set_aside(&w);
    assert!(w.creates.form().is_some());
    // While New is open the window is held: no tab, Recover or Quit (Open only
    // asks, §30T), and Close and Quit stop at A's forms although none is on screen.
    assert!(!w.can_leave());
    assert!(!crate::may_leave_tab(
        &w.creates,
        &w.edits,
        &w.sessions,
        &w.input
    ));
    assert!(!crate::begin_window_quit(
        &mut w.tabs,
        &mut w.sessions,
        &w.creates,
        &Loads::default(),
        &Exports::default(),
        &w.edits,
    ));
    assert_eq!(
        w.tabs.close_step(&w.sessions, a, false),
        Some(CloseStep::Form)
    );
    w.tabs.begin_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Form);
    w.tabs.abort_quit();

    // A size that is not a number: refused in the form, kept as typed.
    {
        let form = w.creates.form().expect("New's form");
        form.content = NewContent::SamplePlate;
        form.width = "77x".to_owned();
    }
    assert!(w.answer_new(NewChoice::Create).is_none());
    assert!(w.creates.form().expect("kept").refusal.is_some());
    set_aside(&w);
    // A plate the document refuses to make: nothing is added.
    w.creates.form().expect("kept").width = "0".to_owned();
    let refused = w.answer_new(NewChoice::Create).expect("parsed");
    assert!(w.create_from_new(refused, Ok(())).is_err());
    assert_eq!(w.creates.form().expect("kept").width, "0");
    set_aside(&w);
    // A candidate whose picture the device refuses: made, not shown.
    w.creates.form().expect("kept").width = "50".to_owned();
    let content = w.answer_new(NewChoice::Create).expect("a plate");
    assert!(
        w.create_from_new(content, Err(CadError::rendering("no device")))
            .is_err()
    );
    assert_eq!(w.creates.form().expect("kept").width, "50", "New's values");
    set_aside(&w);
    // A creation given up on while it runs: its late candidate is dropped.
    let (go, released) = mpsc::channel::<()>();
    let (tx, rx) = mpsc::channel();
    let root = w.private.path().to_path_buf();
    crate::start_new(
        &mut w.creates,
        &Loads::default(),
        &Exports::default(),
        &mut w.input,
        NewDocument::Empty,
        move |content, generation, _| {
            std::thread::spawn(move || {
                let _ = released.recv();
                let made = crate::creates::tests::test_candidate(
                    &root,
                    content,
                    &OperationContext::default(),
                );
                tx.send((generation, made)).expect("answer");
            })
        },
    )
    .expect("a creation starts");
    assert!(!w.can_leave(), "a running creation holds the window");
    w.creates.cancel(&mut w.input);
    go.send(()).expect("released");
    let (generation, late) = wait(&rx);
    assert!(late.is_ok(), "made after all, and still not shown");
    assert!(
        crate::creates::finish_create(&mut w.creates, &mut w.input, generation, late).is_none()
    );
    assert_eq!(w.creates.form().expect("kept").width, "50");
    set_aside(&w);
    // Cancel: A's forms come back exactly, request history included.
    assert!(w.answer_new(NewChoice::Cancel).is_none());
    assert!(w.creates.form().is_none());
    a_back(&w);

    // The drawing: the same set-aside, and its own Cancel gives them back.
    assert!(w.begin_new(true));
    set_aside(&w);
    assert!(w.creates.sketch.drawing_new());
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some((Vec::new(), 0, 0)),
        "New's drawing starts empty"
    );
    crate::sketch::tests::press(&mut w.creates.sketch, "Cancel draft");
    w.end_new();
    a_back(&w);

    // Accepted: a separate Untitled tab; A hidden with its forms.
    assert!(w.begin_new(false));
    w.creates.form().expect("New").content = NewContent::Empty;
    let content = w.answer_new(NewChoice::Create).expect("Empty");
    w.create_from_new(content, Ok(())).expect("a new tab");
    let n = w.sessions.tab();
    assert_ne!(n, a);
    assert!(w.sessions.untitled());
    assert!(w.creates.form().is_none() && !w.creates.sketch.active());
    assert!(!w.tabs.has_aside() && w.tabs.has_draft(a));
    assert!(w.hidden(a).dirty() && w.hidden(a).can_undo());
    w.round(a);
    a_back(&w);
    crate::constraints::tests::session_apply::press(&mut w.creates.sketch.constraints, "Redo");
    assert_eq!(a_edits.add.len() + 1, {
        let redone = w.creates.sketch.constraints.draft_edits().expect("A");
        redone.add.len()
    });
    // Its Apply is offered again, through the predicate its button asks.
    w.tell_forms();
    assert!(crate::can_apply_constraints(
        &w.creates,
        &Loads::default(),
        &Exports::default(),
        &w.edits,
        &w.sessions
    ));
    assert!(w.hidden(n).untitled() && !w.tabs.has_draft(n));

    // A full window: refused at Create before anything is made, and a candidate
    // made anyway is refused at its bind; Cancel gives the form back.
    for k in 0..(MAX_TABS - 2) {
        let more = user.path().join(format!("more-{k}.fcad"));
        std::fs::copy(&a_file, &more).expect("more");
        assert_eq!(w.open(&more), Opening::Load);
    }
    assert_eq!(w.tabs.count(), MAX_TABS);
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "x7");
    assert!(w.begin_new(false));
    w.creates.form().expect("New").content = NewContent::Empty;
    let content = w.answer_new(NewChoice::Create).expect("Empty");
    let refusal = w.create_from_new(content, Ok(())).expect_err("full");
    assert_eq!(
        refusal.to_string(),
        CadError::input(crate::tabs::full()).to_string()
    );
    let (_, candidate) = crate::creates::tests::run_to_completion(
        &mut w.creates,
        &mut w.input,
        w.private.path(),
        NewDocument::Empty,
    );
    assert!(w.bind_candidate(candidate.expect("made"), Ok(())).is_err());
    assert_eq!(w.tabs.count(), MAX_TABS);
    assert!(w.tabs.has_aside() && w.creates.form().is_some());
    assert!(w.answer_new(NewChoice::Cancel).is_none());
    assert_eq!(w.edits.typed(), Some((Some(feature), "x7")));
    assert!(!w.tabs.has_aside());
    assert_eq!(contents(&[&a_file]), originals, "only Save writes a file");
    println!("\nFCAD_30Q_NEW_OVER_FORMS_EXECUTED");
}

/// Recover beside an unfinished form: a full window refuses before the record is
/// claimed; a Cancel, a picture the device refuses and an answer nobody waits for
/// leave the record whole and A's vertex draft (with its history) in A; a live
/// window's own record is never taken; an accepted Recover is a new tab that owns
/// the adopted record, and A's draft is there when A is shown again.
#[test]
fn recover_over_an_unfinished_form_keeps_the_record_on_every_refusal() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("plate.fcad");
    plate(&a_file, 12.0);
    let c_file = user.path().join("crashed.fcad");
    plate(&c_file, 12.0);
    let root = tempfile::tempdir().expect("recovery");
    let store = RecoveryStore::open(root.path()).expect("store");
    let scratch = tempfile::tempdir().expect("scratch");
    let c = orphan(&store, &c_file, scratch.path(), 21.5);
    let originals = contents(&[&a_file, &c_file]);

    let mut w = Window::new(Drawn::Mock, Some(&store));
    let mut recoveries = Recoveries::new(Ok(store.clone()));
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(14.0);
    // A's own crash copy is live: it is neither offered nor claimable.
    let deadline = Instant::now() + Duration::from_secs(60);
    while store.list().expect("list").active == 0 {
        assert!(Instant::now() < deadline, "A's copy was never written");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(recoverable(&store), [c]);
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
    let a_draft = crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("A");
    assert!(a_draft.2 > 0, "a draft Redo to keep");
    let kept = |w: &Window| {
        assert_eq!(w.sessions.tab(), a);
        assert_eq!(
            crate::sketch::tests::vertex_draft(&w.creates.sketch),
            Some(a_draft.clone())
        );
        assert!(w.sessions.dirty() && !w.sessions.busy());
        assert_eq!(w.tabs.count(), 1);
        assert_eq!(recoverable(&store), [c], "the record is whole");
    };
    // The buttons and the handler ask one predicate; an idle form does not hold it.
    let can_recover = |w: &Window, recoveries: &Recoveries| {
        crate::can_recover(
            &w.creates,
            &Loads::default(),
            &Exports::default(),
            &w.edits,
            &w.sessions,
            &w.input,
            recoveries,
        )
    };
    assert!(can_recover(&w, &recoveries));

    // Cancel: the late answer is refused and its claim goes with it.
    let generation = w.begin_recovery(&mut recoveries, c).expect("started");
    assert!(!can_recover(&w, &recoveries) && w.sessions.busy());
    let answer = recovered(w.private.path(), &store, c);
    assert!(w.sessions.cancel());
    assert!(!w.deliver_recovery(&mut recoveries, generation, answer, Ok(())));
    kept(&w);
    // A picture the device refuses.
    let generation = w.begin_recovery(&mut recoveries, c).expect("started");
    let answer = recovered(w.private.path(), &store, c);
    assert!(!w.deliver_recovery(
        &mut recoveries,
        generation,
        answer,
        Err(CadError::rendering("no device"))
    ));
    kept(&w);
    // An answer for an older generation is nobody's.
    let stale = generation;
    let generation = w.begin_recovery(&mut recoveries, c).expect("started");
    let answer = recovered(w.private.path(), &store, c);
    assert!(!w.deliver_recovery(&mut recoveries, stale, answer, Ok(())));
    assert!(w.sessions.busy(), "the stale answer released nothing");

    // Accepted: a new tab that owns the record; A hidden with its draft.
    let answer = recovered(w.private.path(), &store, c);
    assert!(w.deliver_recovery(&mut recoveries, generation, answer, Ok(())));
    let r = w.sessions.tab();
    assert_ne!(r, a);
    assert!(w.sessions.recovered() && w.sessions.dirty());
    assert_eq!(height_in(&w.sessions), 21.5);
    assert!(!w.creates.sketch.active() && w.tabs.has_draft(a));
    assert!(recoverable(&store).is_empty());
    assert_eq!(
        store.list().expect("list").active,
        2,
        "A's and the adopted one"
    );
    assert!(
        store.claim(c).is_err(),
        "the adopted record is the new tab's"
    );

    // A full window refuses before the claim: a second orphan stays whole.
    let d = orphan(&store, &c_file, scratch.path(), 30.0);
    for k in 0..(MAX_TABS - 2) {
        let more = user.path().join(format!("more-{k}.fcad"));
        std::fs::copy(&a_file, &more).expect("more");
        assert_eq!(w.open(&more), Opening::Load);
    }
    assert!(w.begin_recovery(&mut recoveries, d).is_none());
    assert_eq!(recoverable(&store), [d]);
    let made = recovered(w.private.path(), &store, d).expect("claimed anyway");
    let shown = made.1.current().path().to_path_buf();
    assert!(
        w.arrive(&shown, Ok(made.0), Bind::Open(Box::new(made.1)), Ok(()))
            .is_err(),
        "refused at the bind"
    );
    assert_eq!(recoverable(&store), [d], "and the claim let go");

    // Back to A: the draft as left, its history both ways; closing the recovered
    // tab retires its record only.
    w.switch(a);
    kept_after(&w, a, &a_draft);
    crate::sketch::tests::press(&mut w.creates.sketch, "Redo draft");
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch)
            .expect("A")
            .2,
        a_draft.2 - 1
    );
    assert_eq!(w.close(r).expect("closed"), None);
    // Retirement removes the lease before its directory: wait for both.
    let deadline = Instant::now() + Duration::from_secs(60);
    while store.list().expect("list").active != 1 || record_dirs(root.path()) != 2 {
        assert!(
            Instant::now() < deadline,
            "the recovered tab's record stayed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        recoverable(&store),
        [d],
        "A's record stays A's, D stays whole"
    );
    assert_eq!(contents(&[&a_file, &c_file]), originals);
    println!("\nFCAD_30Q_RECOVER_OVER_FORMS_EXECUTED");
}

fn kept_after(w: &Window, a: TabId, draft: &(Vec<[String; 2]>, usize, usize)) {
    assert_eq!(w.sessions.tab(), a);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch).as_ref(),
        Some(draft)
    );
    assert!(w.sessions.dirty() && w.sessions.takes_form_apply());
}

/// New's production creation worker (`creates::run_create`) through the window's
/// own state machine: the candidate to show, or nothing (refused or failed).
fn produce(w: &mut Window, content: NewDocument) -> Option<crate::creates::Candidate> {
    let (tx, rx) = mpsc::channel();
    let root = w.private.path().to_path_buf();
    let started = crate::start_new(
        &mut w.creates,
        &Loads::default(),
        &Exports::default(),
        &mut w.input,
        content,
        move |content, generation, cancel| {
            let context = OperationContext::default().with_cancel(cancel.clone());
            crate::creates::spawn_create(
                move || crate::creates::run_create(&root, content, &context),
                move |result| {
                    let _ = tx.send((generation, result));
                },
            )
        },
    )
    .expect("a creation starts");
    let (generation, result) = wait(&rx);
    assert_eq!(generation, started);
    crate::creates::finish_create(&mut w.creates, &mut w.input, generation, result)
}

/// Without a kernel, Open, New and Recover beside A's form are each refused at
/// their picture by the production workers: no tab, A's form as typed, New's
/// form kept until its Cancel gives A's back, the crash copy whole (executes
/// only in a build with no kernel).
#[test]
fn stub_open_new_and_recover_beside_a_form_are_refused_at_the_picture_and_keep_it() {
    if ferritecad_occt::is_available() {
        return;
    }
    use ferritecad_ui::NewChoice;
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let root = tempfile::tempdir().expect("recovery");
    let store = RecoveryStore::open(root.path()).expect("store");
    let scratch = tempfile::tempdir().expect("scratch");
    let c = orphan(&store, &b_file, scratch.path(), 21.5);
    let mut w = Window::new(Drawn::Mock, None);
    let mut loads = Loads::default();
    let mut recoveries = Recoveries::new(Ok(store.clone()));
    w.open(&a_file);
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "2..6");
    let kept = |w: &Window| {
        assert_eq!(w.edits.typed(), Some((Some(feature), "2..6")));
        assert_eq!(w.tabs.count(), 1);
        assert!(!w.sessions.busy() && !w.tabs.has_aside());
    };
    let context = OperationContext::default();

    let reading = Window::begin_load(&mut loads, &b_file);
    let read = open_for_view(w.private.path(), &b_file, &context);
    assert!(read.is_err(), "no kernel to draw it with");
    assert!(!w.deliver_load(&mut loads, reading, read, Ok(())));
    kept(&w);

    assert!(w.begin_new(false));
    let content = w.answer_new(NewChoice::Create).expect("Empty");
    assert!(produce(&mut w, content).is_none(), "refused at its picture");
    assert!(w.creates.form().is_some() && w.tabs.has_aside());
    w.answer_new(NewChoice::Cancel);
    kept(&w);

    let generation = w.begin_recovery(&mut recoveries, c).expect("started");
    let answer = recover_for_view(w.private.path(), &store, c, &context);
    assert!(answer.is_err(), "no kernel to draw it with");
    assert!(!w.deliver_recovery(&mut recoveries, generation, answer, Ok(())));
    kept(&w);
    assert_eq!(recoverable(&store), [c], "the record is whole");
    loads.stop_all();
    println!("\nFCAD_30Q_STUB_OVER_FORMS_EXECUTED");
}

/// Open CASCADE without the solver: Open, New and Recover drawn by the production
/// workers beside each tab's height form; each form applied after returning.
#[test]
fn mixed_open_new_and_recover_beside_forms_with_occt_and_no_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        return;
    }
    use ferritecad_ui::NewChoice;
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let root = tempfile::tempdir().expect("recovery");
    let store = RecoveryStore::open(root.path()).expect("store");
    let scratch = tempfile::tempdir().expect("scratch");
    let c = orphan(&store, &b_file, scratch.path(), 21.5);
    let mut w = Window::new(Drawn::Native, None);
    let mut loads = Loads::default();
    let mut recoveries = Recoveries::new(Ok(store.clone()));
    let context = OperationContext::default();
    w.open(&a_file);
    let a = w.sessions.tab();
    let feature_a = w.feature();
    w.open_height_form();
    w.edits.type_height(feature_a, "18");
    let reading = Window::begin_load(&mut loads, &b_file);
    let read = open_for_view(w.private.path(), &b_file, &context);
    assert!(w.deliver_load(&mut loads, reading, read, Ok(())));
    let b = w.sessions.tab();
    let feature_b = w.feature();
    w.open_height_form();
    w.edits.type_height(feature_b, "19");
    assert!(w.begin_new(false));
    let content = w.answer_new(NewChoice::Create).expect("Empty");
    let candidate = produce(&mut w, content).expect("drawn by Open CASCADE");
    w.bind_candidate(candidate, Ok(())).expect("a new tab");
    assert!(w.sessions.untitled() && w.tabs.has_draft(b));
    let generation = w.begin_recovery(&mut recoveries, c).expect("started");
    let answer = recover_for_view(w.private.path(), &store, c, &context);
    assert!(w.deliver_recovery(&mut recoveries, generation, answer, Ok(())));
    assert!(!w.scene.catalogue.is_empty(), "drawn by Open CASCADE");
    assert_eq!(height_in(&w.sessions), 21.5);
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(feature_a), "18")));
    assert!(w.apply_height_form());
    w.round(b);
    assert_eq!(w.edits.typed(), Some((Some(feature_b), "19")));
    assert!(w.apply_height_form());
    assert_eq!(height_in(&w.sessions), 19.0);
    assert_eq!(height_in(w.hidden(a)), 18.0);
    loads.stop_all();
    println!("\nFCAD_30Q_MIXED_OVER_FORMS_EXECUTED");
}

// --- native: the window recipe on the owners, and its comparator -----------------

/// What the §30Q window recipe leaves in its root.
const RECIPE_OUTPUTS: [&str; 5] = [
    "a.fcad",
    "c.fcad",
    "n.fcad",
    "a-unsaved.stl",
    "a-unsaved.fbx",
];

/// The inputs `tools/open-new-recover-gui.py` makes: §30O's plate and circle, a
/// third plate the crashed process had open (`child/plate.fcad`, pristine in
/// `inputs/`), and that process's crash copy, 22 mm high, in the root's own
/// recovery folder — the generator's controlled child (`named-dirty`) does the
/// same open, 22 mm Apply and publication before it is killed. No window output.
fn make_recipe_inputs(root: &Path) {
    make_inputs(root);
    let child = root.join("child");
    std::fs::create_dir_all(child.join("sessions")).expect("child");
    let plate = child.join("plate.fcad");
    cli(&[
        "create".as_ref(),
        plate.as_os_str(),
        "--sample".as_ref(),
        "--size".as_ref(),
        "80".as_ref(),
        "40".as_ref(),
        "12".as_ref(),
    ]);
    std::fs::copy(&plate, root.join("inputs/plate.fcad")).expect("pristine");
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    orphan(&store, &plate, &child.join("sessions"), 22.0);
}

/// The recipe's steps (see the contract) on the window's owners, without a window:
/// the comparator's own check, never window evidence.
fn recipe_on_owners(root: &Path) {
    use ferritecad_ui::{NewChoice, NewContent};
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let mut w = Window::new(Drawn::Native, Some(&store));
    let mut loads = Loads::default();
    let mut recoveries = Recoveries::new(Ok(store.clone()));
    recoveries.listed(store.list());
    let context = OperationContext::default();
    // 1: A; 2: its height form, `2..6`, not applied.
    w.open(&root.join("a.fcad"));
    let a = w.sessions.tab();
    let feature = w.feature();
    w.open_height_form();
    w.edits.type_height(feature, "2..6");
    // 3: Open B beside it: a new tab with no form.
    assert!(crate::may_leave_tab(
        &w.creates,
        &w.edits,
        &w.sessions,
        &w.input
    ));
    let b_file = root.join("b.fcad");
    let reading = Window::begin_load(&mut loads, &b_file);
    let read = open_for_view(w.private.path(), &b_file, &context);
    assert!(w.deliver_load(&mut loads, reading, read, Ok(())));
    assert_eq!(w.edits.typed(), None);
    // 4: A: `2..6` as typed; 26; Apply.
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
    let polygon = crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("A");
    // 6: the drawing beside it, then its Cancel: A's form is back.
    assert!(w.begin_new(true));
    assert!(w.tabs.has_aside());
    crate::sketch::tests::press(&mut w.creates.sketch, "Cancel draft");
    w.end_new();
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(polygon.clone())
    );
    // 7: New: a 50 × 30 × 7 plate in an Untitled tab.
    assert!(w.begin_new(false));
    {
        let form = w.creates.form().expect("New");
        form.content = NewContent::SamplePlate;
        form.width = "50".to_owned();
        form.depth = "30".to_owned();
        form.height = "7".to_owned();
    }
    let content = w.answer_new(NewChoice::Create).expect("a plate");
    let candidate = produce(&mut w, content).expect("made");
    w.bind_candidate(candidate, Ok(())).expect("a new tab");
    let n = w.sessions.tab();
    // 8: Recover immediately, including while New's lane creates its record.
    // Cleanup must not borrow a published orphan's lease and falsely report it
    // as a live window. No waiting for unrelated lanes to settle.
    let record = recoveries.record_at(0).expect("offered").record;
    let generation = w.begin_recovery(&mut recoveries, record).expect("started");
    let answer = recover_for_view(w.private.path(), &store, record, &context);
    assert!(w.deliver_recovery(&mut recoveries, generation, answer, Ok(())));
    let c = w.sessions.tab();
    // 9: A: the vertices as left; Apply; unsaved exports; Save.
    w.round(a);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(polygon)
    );
    w.apply_vertices_form();
    let alias = w.sessions.suggestion().expect("alias");
    export_bytes(
        &w.sessions.export_path().expect("A"),
        &alias,
        root,
        "a-unsaved",
    );
    assert!(w.save(SaveTarget::InPlace).published);
    // 10, 11: the recovered copy and the new plate, each saved as a file.
    w.round(c);
    assert!(w.save(SaveTarget::As(root.join("c.fcad"))).published);
    w.round(n);
    assert!(w.save(SaveTarget::As(root.join("n.fcad"))).published);
    // 12: Quit: nothing unsaved and no form: the window ends.
    assert!(crate::begin_window_quit(
        &mut w.tabs,
        &mut w.sessions,
        &w.creates,
        &Loads::default(),
        &Exports::default(),
        &w.edits,
    ));
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Exit);
    w.tabs.decide_exit(&mut w.sessions);
    w.sessions.stop_all();
    w.tabs.stop_all();
    loads.stop_all();
}

/// The command line's copy of the crashed process's work: the plate it had open,
/// 22 mm high.
fn recovered_peer(inputs: &Path, work: &Path) -> PathBuf {
    let plate = inputs.join("plate.fcad");
    let reading = read_extrude_source(&plate).expect("plate");
    let peer = work.join("c-peer.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        plate.as_os_str(),
        "--feature".as_ref(),
        reading.features[0].feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        reading.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "22".as_ref(),
        "-o".as_ref(),
        peer.as_os_str(),
    ]);
    peer
}

/// Compares the outputs of the §30Q window recipe in `root` with the shipped
/// command line. Reads only what the window wrote; refuses a missing output
/// before any peer job.
fn compare_recipe(root: &Path) {
    use super::super::super::checkpoints::{all_sql, ids_and_refs, stl_facts};
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
        read("b.fcad") == read("inputs/b.fcad"),
        "b.fcad was written"
    );
    let work = tempfile::tempdir().expect("work");
    // A: height 26 and both 80 made 90, applied after Open, New and Recover.
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
    // C: the crash copy, recovered and saved: the crashed process's model.
    let c = root.join("c.fcad");
    let c_peer = recovered_peer(&root.join("inputs"), work.path());
    assert!(
        all_sql(&c, &[], false) == all_sql(&c_peer, &[], false),
        "c.fcad is not the recovered model"
    );
    assert!(
        ids_and_refs(&c) == ids_and_refs(&root.join("inputs/plate.fcad")),
        "c.fcad identities"
    );
    let (c_stl, _) = peer_bytes(&c, work.path(), "c");
    assert!(
        c_stl == peer_bytes(&c_peer, work.path(), "c-cli").0,
        "c.fcad exports against the CLI"
    );
    let (_, height, volume) = stl_facts(&c_stl);
    assert!(
        (height - 22.0).abs() < 1e-4 && (volume - 80.0 * 40.0 * 22.0).abs() < 1e-2,
        "c.fcad geometry: {height} {volume}"
    );
    // N: New's plate, a document of its own.
    let n = root.join("n.fcad");
    let others = [
        document_id(&a),
        document_id(&root.join("b.fcad")),
        document_id(&c),
    ];
    assert!(
        !others.contains(&document_id(&n)),
        "n.fcad is not a new document"
    );
    let (n_stl, _) = peer_bytes(&n, work.path(), "n");
    let (_, height, volume) = stl_facts(&n_stl);
    assert!(
        (height - 7.0).abs() < 1e-4 && (volume - 50.0 * 30.0 * 7.0).abs() < 1e-2,
        "n.fcad is not the new plate: {height} {volume}"
    );
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let listing = store.list().expect("list");
    assert!(
        listing.recoverable().count() == 0 && listing.active == 0,
        "a recovery record was left behind"
    );
}

/// The comparator's positive run and its eight negative controls, each on a copy.
fn compare_recipe_with_controls(root: &Path) {
    compare_recipe(root);
    let control = |name: &str, why: &str, breaks: &dyn Fn(&Path)| {
        super::super::control_of(compare_recipe, root, name, why, breaks);
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
    // A's vertex form lost to New: only the height reached the file.
    control(
        "draft lost at New",
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
    // The plate as it was before the crash: the copy was not what was saved.
    control("not recovered", "c.fcad is not the recovered model", &|r| {
        std::fs::write(r.join("c.fcad"), file(r, "inputs/plate.fcad")).expect("w");
    });
    control("not new", "n.fcad is not a new document", &|r| {
        std::fs::write(r.join("n.fcad"), file(r, "a.fcad")).expect("w");
    });
    control("record left", "a recovery record was left behind", &|r| {
        let store = RecoveryStore::open(&r.join("recovery")).expect("store");
        orphan(&store, &r.join("inputs/plate.fcad"), &r.join("child"), 30.0);
    });
}

/// The real window's outputs in `FCAD_30Q_GUI_DIR` against the command line, then
/// eight negative controls on copies of those same outputs. Nothing here makes a
/// window output; without the variable the test does nothing.
#[test]
fn native_compare_real_open_new_recover_gui_artifacts_with_negative_controls() {
    let Some(root) = std::env::var_os("FCAD_30Q_GUI_DIR") else {
        return;
    };
    assert!(native(), "the comparison needs Open CASCADE");
    compare_recipe_with_controls(Path::new(&root));
    println!("\nFCAD_30Q_GUI_COMPARE_OK negative_controls=8 all_SQL_cells=true");
}

/// Native, compact: A's accepted change and two forms left open in turn; Open B,
/// the drawing's Cancel, an accepted New and an accepted Recover beside them; A's
/// form applied after returning; Save of all three. The recipe's files then go
/// through the comparator (every SQL cell, ids and references, CLI exports before
/// and after Save, geometry read independently) and its eight controls. The
/// owners without a window: not window evidence.
#[test]
fn native_open_new_recover_beside_forms_then_apply_save_and_compare_with_the_command_line() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let root = root.path();
    make_recipe_inputs(root);
    recipe_on_owners(root);
    compare_recipe_with_controls(root);
    let work = tempfile::tempdir().expect("work");
    let a_stl = std::fs::read(root.join("a-unsaved.stl")).expect("a");
    let a_fbx = std::fs::read(root.join("a-unsaved.fbx")).expect("a");
    let (n_stl, n_fbx) = peer_bytes(&root.join("n.fcad"), work.path(), "n");
    let (c_stl, c_fbx) = peer_bytes(&root.join("c.fcad"), work.path(), "c");
    let volume = |stl: &[u8]| super::super::super::checkpoints::stl_facts(stl).2;
    if let Ok(dir) = std::env::var("FCAD_OPEN_NEW_RECOVER_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        for (name, stl, fbx) in [
            ("over-a", &a_stl, &a_fbx),
            ("over-n", &n_stl, &n_fbx),
            ("over-c", &c_stl, &c_fbx),
        ] {
            std::fs::write(dir.join(format!("{name}.stl")), stl).expect("artifact");
            std::fs::write(dir.join(format!("{name}.fbx")), fbx).expect("artifact");
        }
    }
    println!(
        "\nFCAD_30Q_NATIVE_OVER_FORMS_EXECUTED volume_a={:.3} volume_n={:.3} volume_c={:.3}",
        volume(&a_stl),
        volume(&n_stl),
        volume(&c_stl)
    );
    println!("FCAD_30Q_SESSION_FILES_COMPARE_OK negative_controls=8 all_SQL_cells=true");
}

/// A reader that is not this window: any process holding the record's lease shared,
/// as a listing does while it verifies.
fn foreign_reader(root: &Path, record: RecordId) -> std::fs::File {
    let lease = root.join(format!("r-{record}")).join("lease");
    let file = std::fs::File::open(lease).expect("lease");
    file.try_lock_shared().expect("nobody holds it exclusively");
    file
}

/// §30S: a Recover whose record a reader is holding, through the window's owners and
/// a real worker thread on two tabs that each keep a form. The worker waits, off the
/// event loop; Cancel and a late, stale or refused answer open nothing, free no newer
/// slot and delete nothing; the forms are as typed; once the reader is gone Recover
/// succeeds; and the window's end does not wait for the reader's bound.
#[test]
fn recover_waiting_for_a_reader_obeys_cancel_late_answers_and_quit_beside_forms_in_two_tabs() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    let c_file = user.path().join("crashed.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    plate(&c_file, 12.0);
    let root = tempfile::tempdir().expect("recovery");
    let store = RecoveryStore::open(root.path()).expect("store");
    let scratch = tempfile::tempdir().expect("scratch");
    let c = orphan(&store, &c_file, scratch.path(), 21.5);
    let originals = contents(&[&a_file, &b_file, &c_file]);
    let record_files = || {
        let mut files: Vec<_> = std::fs::read_dir(root.path().join(format!("r-{c}")))
            .expect("record")
            .map(|e| e.expect("entry").path())
            .map(|path| (path.clone(), std::fs::read(&path).expect("bytes")))
            .collect();
        files.sort();
        files
    };
    let whole = record_files();

    let mut w = Window::new(Drawn::Mock, None);
    let mut recoveries = Recoveries::new(Ok(store.clone()));
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
    let kept = |w: &Window| {
        assert_eq!(w.sessions.tab(), b);
        assert_eq!(w.edits.typed(), Some((Some(feature_b), "19")));
        assert_eq!(w.tabs.count(), 2);
        assert!(!w.sessions.busy(), "no foreground slot is held");
        assert_eq!(recoverable(&store), [c], "listed beside a reader, whole");
        assert_eq!(record_files(), whole, "the copy was not touched");
    };

    // One worker as `App::recover` starts it: its token comes from `Recoveries`.
    let spawn = |w: &mut Window, recoveries: &mut Recoveries, record: RecordId| {
        let (answers, receiver) = std::sync::mpsc::channel();
        let (private, store) = (w.private.path().to_path_buf(), store.clone());
        let sessions = &mut w.sessions;
        let generation = recoveries
            .begin(record, |generation, cancel| {
                sessions.hold_recovery(generation);
                std::thread::spawn(move || {
                    let _ = answers.send(recovered_unless(&private, &store, record, cancel));
                })
            })
            .expect("started");
        (generation, receiver)
    };

    // Cancel while it waits: the worker answers at once, not at the end of its bound.
    let reader = foreign_reader(root.path(), c);
    let (cancelled, answers) = spawn(&mut w, &mut recoveries, c);
    assert!(w.sessions.busy());
    assert!(w.sessions.cancel());
    recoveries.cancel();
    let late = answers
        .recv_timeout(Duration::from_secs(2))
        .expect("cancel ended the wait long before the 5 s bound");
    assert!(matches!(late, Err(CadError::Cancelled)), "{:?}", late.err());
    assert!(
        !w.deliver_recovery(&mut recoveries, cancelled, late, Ok(())),
        "a cancelled Recover opens nothing"
    );
    kept(&w);

    // A newer Recover is not released by the older one's late answer.
    let (newer, answers) = spawn(&mut w, &mut recoveries, c);
    assert!(w.sessions.busy());
    assert!(!w.deliver_recovery(&mut recoveries, cancelled, Err(CadError::Cancelled), Ok(())));
    assert!(w.sessions.busy(), "the stale answer released nothing");

    // The reader leaves: this Recover waited for it, and now succeeds.
    drop(reader);
    let answer = answers
        .recv_timeout(Duration::from_secs(60))
        .expect("the reader left");
    assert!(w.deliver_recovery(&mut recoveries, newer, answer, Ok(())));
    assert_ne!(w.sessions.tab(), b);
    assert!(w.sessions.recovered() && w.sessions.dirty());
    assert_eq!(height_in(&w.sessions), 21.5);
    assert!(recoverable(&store).is_empty(), "the copy is the new tab's");
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(feature_a), "18")));
    w.round(b);
    assert_eq!(w.edits.typed(), Some((Some(feature_b), "19")));

    // A refused one (the record is held by an owner now): nothing opens, the form
    // stays, the holder is untouched.
    let d = orphan(&store, &c_file, scratch.path(), 30.0);
    let holder = store.claim(d).expect("held by this test");
    let (refused, answers) = spawn(&mut w, &mut recoveries, d);
    let answer = answers
        .recv_timeout(Duration::from_secs(2))
        .expect("held, not waited for");
    assert!(answer.is_err());
    assert!(!w.deliver_recovery(&mut recoveries, refused, answer, Ok(())));
    assert_eq!(w.edits.typed(), Some((Some(feature_b), "19")));
    assert!(!w.sessions.busy());
    drop(holder);
    assert_eq!(recoverable(&store), [d]);

    // Quit: the window's end cancels the waiting worker instead of joining its bound.
    let reader = foreign_reader(root.path(), d);
    let (quit, _answers) = spawn(&mut w, &mut recoveries, d);
    assert!(recoveries.accepts(quit));
    let started = Instant::now();
    recoveries.stop_all();
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "Quit waited for the reader"
    );
    drop(reader);
    assert_eq!(recoverable(&store), [d], "the copy is whole");
    assert_eq!(contents(&[&a_file, &b_file, &c_file]), originals);
    println!("\nFCAD_30S_RECOVER_BESIDE_READER_EXECUTED");
}
