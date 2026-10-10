// SPDX-License-Identifier: MIT
//! §30O: several documents in tabs of one window, through the window's own
//! owners — `Tabs`, each tab's `Sessions`, the arrival statement `present` the
//! window's `show` runs, `close_shown`, the workers — without a graphics device.
//! Pictures are built from the documents by the scene builder on a mock kernel
//! (kernel-free) and, in the native gates, on Open CASCADE.

use super::*;
use crate::tabs::{After, CloseStep, MAX_TABS, Opening, QuitStep, TabId, Tabs};
use ferritecad_document::CheckpointEntry;
use ferritecad_jobs::{RecoveryRecorder, RecoveryStatus, RecoveryStore, SaveTarget};
use ferritecad_ui::ViewportInput;
use ferritecad_viewport::StandardView;
use std::time::{Duration, Instant};

/// §30P: each tab's unfinished forms, through the same owners.
mod drafts;
/// §30R: the last window's saved files, kept at Quit and reopened.
mod restore;

/// How a picture is built for a test: on the mock kernel (kernel-free gates) or
/// by the window's own scene worker on Open CASCADE (native gates).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Drawn {
    Mock,
    Native,
}

fn mock_scene(path: &Path) -> Result<LoadedScene> {
    ferritecad_scene::snapshot_of(
        path,
        &mut MockKernel::new(),
        |_, _| Err(CadError::unsupported("no STEP in these documents")),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
}

/// The picture of `path` on a worker, delivered through a channel as the window's
/// event would be.
fn draw(
    drawn: Drawn,
    path: PathBuf,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<LoadedScene>) + Send + 'static,
) -> std::thread::JoinHandle<()> {
    match drawn {
        Drawn::Native => spawn_scene(path, cancel, deliver),
        Drawn::Mock => std::thread::spawn(move || {
            let scene = cancel.check().and_then(|()| mock_scene(&path));
            deliver(scene);
        }),
    }
}

fn wait<T>(rx: &mpsc::Receiver<T>) -> T {
    rx.recv_timeout(Duration::from_secs(180))
        .expect("an answer")
}

/// The parts of the window an arrival touches, as `App` holds them.
struct Window {
    scene: crate::LiveScene<()>,
    input: ViewportInput,
    sessions: Sessions,
    tabs: Tabs,
    checkpoint_name: String,
    /// §30P: the shown tab's forms, as `App` holds them (`creates.sketch`).
    edits: crate::edits::Edits,
    creates: crate::creates::Creates,
    private: tempfile::TempDir,
    drawn: Drawn,
}

impl Window {
    fn decide_quit(&mut self) {
        let id = self
            .tabs
            .ask_quit_model(&self.sessions)
            .expect("model question");
        assert!(self.tabs.decide_quit_model(&self.sessions, id));
    }
    fn new(drawn: Drawn, store: Option<&RecoveryStore>) -> Self {
        let mut input = ViewportInput::new();
        input.resize(800, 600);
        Self {
            scene: crate::LiveScene::new(
                None,
                (),
                Vec::new(),
                crate::FaceNames::default(),
                crate::EdgeNames::default(),
                crate::VertexNames::default(),
                crate::Visibility::default(),
                Vec::new(),
            ),
            input,
            sessions: Sessions::default(),
            tabs: Tabs::new(store.map(|store| RecoveryRecorder::start(store.clone(), || {}))),
            checkpoint_name: String::new(),
            edits: crate::edits::Edits::default(),
            creates: crate::creates::Creates::default(),
            private: tempfile::tempdir().expect("private root"),
            drawn,
        }
    }

    /// `App::show` without the device: `upload` stands for the GPU upload.
    fn present(
        &mut self,
        document: &Path,
        loaded: Result<LoadedScene>,
        bind: Bind,
        upload: Result<()>,
    ) -> Result<()> {
        crate::present(
            &mut self.scene,
            &mut self.input,
            &mut self.sessions,
            &mut self.tabs,
            &mut self.checkpoint_name,
            crate::tabs::Forms {
                edits: &mut self.edits,
                editor: &mut self.creates.sketch,
            },
            document,
            loaded,
            bind,
            |_, _| upload,
        )
    }

    fn picture(&self, path: &Path) -> Result<LoadedScene> {
        let (tx, rx) = mpsc::channel();
        let worker = draw(
            self.drawn,
            path.to_path_buf(),
            CancelToken::new(),
            move |v| {
                tx.send(v).expect("scene");
            },
        );
        let scene = wait(&rx);
        worker.join().expect("joined");
        scene
    }

    /// `App::open`: the tab list decides; a candidate is read and accepted.
    fn open(&mut self, path: &Path) -> Opening {
        let opening = self.tabs.opening(&self.sessions, path);
        if opening == Opening::Load {
            let session =
                DocumentSession::open_in(self.private.path(), path, HistoryLimits::default())
                    .expect("session");
            let shown = session.current().path().to_path_buf();
            let scene = self.picture(&shown);
            self.present(&shown, scene, Bind::Open(Box::new(session)), Ok(()))
                .expect("a new tab");
        }
        opening
    }

    /// A new document (`App::create_new` after its room check): no file.
    fn create(&mut self, content: ferritecad_jobs::NewDocument) -> Result<()> {
        self.tabs.room().map_err(CadError::input)?;
        let session = DocumentSession::create_document_in(
            self.private.path(),
            HistoryLimits::default(),
            content,
            || Err::<MockKernel, _>(CadError::unsupported("not needed")),
            &OperationContext::default(),
        )?;
        let shown = session.current().path().to_path_buf();
        let scene = self.picture(&shown);
        self.present(&shown, scene, Bind::Open(Box::new(session)), Ok(()))
    }

    fn feature(&self) -> ObjectId {
        read_extrude_source(&self.sessions.export_path().expect("shown"))
            .expect("reading")
            .features[0]
            .feature
    }

    /// The shown tab's Apply of a height, through its edit worker, picture worker
    /// and the arrival statement.
    fn apply(&mut self, millimetres: f64) {
        let feature = self.feature();
        let (tx, rx) = mpsc::channel();
        let drawn = self.drawn;
        let generation = self
            .sessions
            .begin_apply(|ticket, _, cancel| match drawn {
                Drawn::Native => {
                    spawn_apply(ticket, feature, millimetres, cancel.clone(), move |v| {
                        tx.send(v).expect("edit");
                    })
                }
                Drawn::Mock => std::thread::spawn(move || {
                    let edit = ticket.edit_extrude_height(
                        feature,
                        millimetres,
                        &mut MockKernel::new(),
                        &OperationContext::default(),
                    );
                    tx.send(edit).expect("edit");
                }),
            })
            .expect("started");
        let Edited::Show(path) = self.sessions.finish_apply(generation, wait(&rx)) else {
            panic!("the edit was not shown: {}", self.sessions.status);
        };
        self.show_staged(generation, path);
    }

    fn show_staged(&mut self, generation: Address, path: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let token = self.sessions.scene_token(generation).expect("token");
        let worker = draw(self.drawn, path.clone(), token, move |v| {
            tx.send(v).expect("scene");
        });
        assert!(self.sessions.attach_scene(generation, worker));
        let scene = wait(&rx);
        let outcome = self.present(&path, scene, Bind::Staged, Ok(()));
        assert!(outcome.is_ok(), "{outcome:?}");
        assert!(self.sessions.finish_scene(generation, outcome));
    }

    fn step(&mut self, undo: bool) {
        let (generation, path) = self.sessions.begin_move(undo).expect("a step");
        if self.sessions.staged_keeps_picture(generation) {
            let (tx, rx) = mpsc::channel();
            let token = self.sessions.scene_token(generation).expect("token");
            let worker = spawn_kept_facts(path, token, move |v| tx.send(v).expect("facts"));
            assert!(self.sessions.attach_scene(generation, worker));
            let Edited::Keep(path) = self.sessions.finish_kept_facts(generation, wait(&rx)) else {
                panic!("kept");
            };
            self.keep(generation, path);
        } else {
            self.show_staged(generation, path);
        }
    }

    /// `App::keep_picture` without the device.
    fn keep(&mut self, generation: Address, path: PathBuf) {
        let facts = self.sessions.commit_kept().expect("kept");
        crate::retarget_scene(&mut self.scene, path, facts);
        assert!(self.sessions.finish_scene(generation, Ok(())));
    }

    fn checkpoint(&mut self, action: CheckpointAction) {
        let (tx, rx) = mpsc::channel();
        let generation = self
            .sessions
            .begin_checkpoint(action, |ticket, action, _, cancel| {
                spawn_checkpoint(ticket, action, cancel.clone(), move |v| {
                    tx.send(v).expect("checkpoint");
                })
            })
            .expect("started");
        match self.sessions.finish_checkpoint(generation, wait(&rx)) {
            Edited::Keep(path) => self.keep(generation, path),
            Edited::Show(path) => self.show_staged(generation, path),
            other => panic!("{other:?}: {}", self.sessions.status),
        }
    }

    fn listed(&self) -> Vec<CheckpointEntry> {
        self.sessions
            .export_source()
            .expect("shown")
            .checkpoints()
            .expect("readable")
            .to_vec()
    }

    /// Starts showing `tab`; the answer is returned undelivered.
    fn begin_switch(
        &mut self,
        tab: TabId,
        after: Option<After>,
    ) -> (u64, mpsc::Receiver<Result<LoadedScene>>) {
        let (tx, rx) = mpsc::channel();
        let drawn = self.drawn;
        let generation = self
            .tabs
            .begin_switch(&mut self.sessions, tab, after, |path, _, cancel| {
                draw(drawn, path, cancel.clone(), move |v| {
                    let _ = tx.send(v);
                })
            })
            .expect("a switch starts");
        (generation, rx)
    }

    /// The window's `TabShown` handler without the device: `None` for an answer
    /// nobody waits for, otherwise whether it was shown and what follows.
    fn deliver_switch(
        &mut self,
        generation: u64,
        loaded: Result<LoadedScene>,
        upload: Result<()>,
    ) -> Option<(Result<()>, Option<After>)> {
        let path = self.tabs.switch_path(generation)?;
        let outcome = self.present(&path, loaded, Bind::Switch(generation), upload);
        let after = self
            .tabs
            .end_switch(&mut self.sessions, generation, &outcome);
        Some((outcome, after))
    }

    fn switch(&mut self, tab: TabId) {
        let (generation, rx) = self.begin_switch(tab, None);
        let loaded = wait(&rx);
        let (outcome, _) = self
            .deliver_switch(generation, loaded, Ok(()))
            .expect("awaited");
        outcome.expect("shown");
        assert_eq!(self.sessions.tab(), tab);
    }

    fn save(&mut self, target: SaveTarget) -> SaveReport {
        let (tx, rx) = mpsc::channel();
        let generation = self
            .sessions
            .begin_save(target, None, |plan, _, cancel| {
                spawn_save(plan, cancel.clone(), move |v| tx.send(v).expect("save"))
            })
            .expect("a save starts");
        self.sessions
            .finish_save(generation, wait(&rx))
            .expect("answered")
    }

    fn close(&mut self, tab: TabId) -> Result<Option<TabId>> {
        crate::close_shown(
            &mut self.scene,
            &mut self.input,
            &mut self.sessions,
            &mut self.tabs,
            &mut self.checkpoint_name,
            tab,
            |_| Ok(()),
        )
    }

    fn label(&self, tab: TabId) -> (String, bool, bool) {
        self.tabs
            .labels(&self.sessions)
            .into_iter()
            .find(|(id, ..)| *id == tab)
            .map(|(_, name, dirty, active)| (name, dirty, active))
            .expect("labelled")
    }

    fn hidden(&self, tab: TabId) -> &Sessions {
        self.tabs.hidden_sessions(tab).expect("hidden")
    }
}

fn plate(path: &Path, height: f64) {
    create_document(
        CreateDocumentRequest::new(
            path,
            NewDocument::SamplePlate(PlateSize {
                width: 80.0,
                depth: 40.0,
                height,
            }),
            "keep",
        ),
        &OperationContext::default(),
    )
    .expect("a plate");
}

fn document_id(path: &Path) -> ferritecad_types::DocumentId {
    let document = Document::open_read_only(path).expect("document");
    let id = document.meta().document_id;
    document.close().expect("close");
    id
}

/// Two physical copies of one document (one `DocumentId`) are two tabs: each has
/// its own accepted history, Undo/Redo, dirty state, checkpoints and Save.
#[test]
fn two_copies_of_one_document_are_two_tabs_with_their_own_history_saves_and_checkpoints() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("plate.fcad");
    plate(&a_file, 12.0);
    let copies = user.path().join("copies");
    std::fs::create_dir(&copies).expect("dir");
    let b_file = copies.join("plate.fcad");
    std::fs::copy(&a_file, &b_file).expect("a copy");
    assert_eq!(document_id(&a_file), document_id(&b_file));

    let mut w = Window::new(Drawn::Mock, None);
    assert_eq!(w.open(&a_file), Opening::Load);
    let a = w.sessions.tab();
    w.apply(20.0);
    w.checkpoint(CheckpointAction::Create("A at 20".to_owned()));
    let a_checkpoints = w.listed();
    assert_eq!(a_checkpoints.len(), 1);
    w.input
        .handle(ferritecad_ui::ViewportEvent::Look(StandardView::Top), false);
    let a_camera = (w.input.camera().eye(), w.input.camera().target());

    // Open B: a new tab; A stays as it was, unsaved, and is not asked about.
    assert_eq!(w.open(&b_file), Opening::Load);
    let b = w.sessions.tab();
    let kept_a = w.tabs.hidden_sessions(a);
    assert!(
        kept_a.is_some_and(|a| a.dirty() && a.can_undo()),
        "A lost its unsaved model and history when B was opened"
    );
    assert_ne!(a, b, "two copies of one document id are two tabs");
    assert_eq!(w.tabs.order(), [a, b]);
    assert_eq!(height_of(&w.hidden(a).export_path().expect("a")), 20.0);
    assert!(!w.sessions.dirty() && !w.sessions.can_undo());
    assert!(w.listed().is_empty(), "B has none of A's checkpoints");
    // Both are named plate.fcad; the row tells them apart by identity.
    assert_eq!(w.label(a), ("plate.fcad".to_owned(), true, false));
    assert_eq!(w.label(b), ("plate.fcad".to_owned(), false, true));

    w.apply(30.0);
    w.apply(35.0);
    w.step(true);
    assert_eq!(height_of(&w.sessions.export_path().expect("b")), 30.0);
    assert!(w.sessions.can_redo());

    // Back to A: its own model, history, checkpoints and camera.
    w.switch(a);
    assert_eq!(height_of(&w.sessions.export_path().expect("a")), 20.0);
    assert_eq!(w.listed(), a_checkpoints);
    assert_eq!(
        (w.input.camera().eye(), w.input.camera().target()),
        a_camera,
        "A is shown as it was being looked at"
    );
    assert!(matches!(w.scene.selection, crate::Selection::Nothing));
    assert_eq!(
        w.scene.document.as_deref(),
        w.sessions.export_path().as_deref()
    );
    assert!(w.hidden(b).can_redo(), "B's Redo waits in B");
    w.step(true);
    w.step(true);
    assert_eq!(height_of(&w.sessions.export_path().expect("a")), 12.0);
    assert!(w.listed().is_empty(), "Undo took A's checkpoint away");
    w.step(false);
    w.step(false);
    assert_eq!(w.listed(), a_checkpoints, "Redo brings the same one back");

    // Save A: only A's file changes; B is still unsaved and its file untouched.
    let b_bytes = std::fs::read(&b_file).expect("b");
    assert!(w.save(SaveTarget::InPlace).published);
    assert_eq!(height_of(&a_file), 20.0);
    assert_eq!(std::fs::read(&b_file).expect("b"), b_bytes);
    assert!(w.hidden(b).dirty());
    assert_eq!(w.label(a), ("plate.fcad".to_owned(), false, true));

    w.switch(b);
    w.step(false);
    assert_eq!(height_of(&w.sessions.export_path().expect("b")), 35.0);
    assert!(w.save(SaveTarget::InPlace).published);
    assert_eq!(height_of(&b_file), 35.0);
    assert_eq!(height_of(&a_file), 20.0);
    let reopened = Document::open_read_only(&a_file).expect("a");
    assert_eq!(reopened.checkpoints().expect("listed").len(), 1);
    reopened.close().expect("close");
    let reopened = Document::open_read_only(&b_file).expect("b");
    assert!(reopened.checkpoints().expect("listed").is_empty());
    reopened.close().expect("close");
}

/// Opening a file a tab already names, by any name (a link, a hard link, another
/// spelling), shows that tab; a different physical copy is a new tab.
#[test]
fn opening_a_file_open_by_any_name_shows_its_tab_and_copies_are_new_tabs() {
    let user = tempfile::tempdir().expect("user");
    let file = user.path().join("plate.fcad");
    plate(&file, 12.0);
    let mut w = Window::new(Drawn::Mock, None);
    assert_eq!(w.open(&file), Opening::Load);
    let a = w.sessions.tab();
    assert_eq!(w.open(&file), Opening::Shown);
    let spelled = user.path().join(".").join("plate.fcad");
    assert_eq!(w.open(&spelled), Opening::Shown);
    #[cfg(unix)]
    {
        let link = user.path().join("link.fcad");
        std::os::unix::fs::symlink(&file, &link).expect("symlink");
        assert_eq!(w.open(&link), Opening::Shown);
    }
    let hard = user.path().join("hard.fcad");
    std::fs::hard_link(&file, &hard).expect("hard link");
    assert_eq!(w.open(&hard), Opening::Shown);
    std::fs::remove_file(&hard).expect("one name again");

    let copy = user.path().join("copy.fcad");
    std::fs::copy(&file, &copy).expect("copy");
    assert_eq!(w.open(&copy), Opening::Load);
    let b = w.sessions.tab();
    assert_eq!(w.open(&spelled), Opening::Show(a), "A is hidden: show it");
    assert_eq!(w.tabs.count(), 2, "nothing was read twice");

    // A duplicate reaching the bind anyway (a defensive check) adds nothing.
    let twice = DocumentSession::open_in(w.private.path(), &file, HistoryLimits::default())
        .expect("session");
    let twice_dir = twice.private_directory().to_path_buf();
    let shown = twice.current().path().to_path_buf();
    let scene = w.picture(&shown);
    let before = w.scene.document.clone();
    let refused = w
        .present(&shown, scene, Bind::Open(Box::new(twice)), Ok(()))
        .expect_err("one file, one tab");
    assert!(refused.to_string().contains("already open"), "{refused}");
    assert_eq!((w.tabs.count(), w.sessions.tab()), (2, b));
    assert_eq!(w.scene.document, before);
    assert!(!twice_dir.exists(), "the refused candidate's files went");
}

/// Save As refuses another tab's file by any name, even when that file is gone
/// from disk; the session's own no-clobber and version guards are unchanged.
#[test]
fn save_as_onto_another_tabs_file_is_refused_and_the_existing_guards_still_hold() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    plate(&a_file, 12.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.create(NewDocument::Empty).expect("untitled");
    assert!(w.sessions.untitled() && w.sessions.dirty());
    let refusal = w
        .tabs
        .save_as_refusal(&w.sessions, &a_file)
        .expect("refused");
    assert!(refusal.contains("open in another tab"), "{refusal}");
    assert!(
        w.tabs
            .save_as_refusal(&w.sessions, &user.path().join(".").join("a.fcad"))
            .is_some()
    );
    // Gone from disk, still A's: a Save As there would make two writers.
    let a_bytes = std::fs::read(&a_file).expect("a");
    std::fs::remove_file(&a_file).expect("removed");
    assert!(w.tabs.save_as_refusal(&w.sessions, &a_file).is_some());
    std::fs::write(&a_file, &a_bytes).expect("back");
    // The shown tab's own file is not "another tab's"; other paths are free.
    let free = user.path().join("free.fcad");
    assert!(w.tabs.save_as_refusal(&w.sessions, &free).is_none());
    // An occupied path is still refused by the save itself.
    let occupied = user.path().join("occupied.fcad");
    std::fs::write(&occupied, b"theirs").expect("occupied");
    assert!(w.tabs.save_as_refusal(&w.sessions, &occupied).is_none());
    assert!(!w.save(SaveTarget::As(occupied.clone())).published);
    assert_eq!(std::fs::read(&occupied).expect("kept"), b"theirs");
    assert!(w.save(SaveTarget::As(free.clone())).published);
    assert!(!w.sessions.untitled());
    w.switch(a);
    assert!(
        w.tabs.save_as_refusal(&w.sessions, &free).is_some(),
        "the other way round as well"
    );
}

/// Closing asks only about an unsaved tab, shows a hidden one first, and Cancel or
/// a failed Save keeps it; closing the last tab leaves an empty window.
#[test]
fn closing_asks_only_about_unsaved_tabs_and_cancel_or_a_failed_save_keeps_the_tab() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(22.0);
    w.open(&b_file);
    let b = w.sessions.tab();
    w.create(NewDocument::Empty).expect("untitled");
    let c = w.sessions.tab();

    assert_eq!(
        w.tabs.close_step(&w.sessions, b, false),
        Some(CloseStep::Now)
    );
    assert_eq!(
        w.tabs.close_step(&w.sessions, a, false),
        Some(CloseStep::Show)
    );
    assert_eq!(
        w.tabs.close_step(&w.sessions, c, false),
        Some(CloseStep::Ask)
    );
    let b_private = w.hidden(b).private_directory().expect("b").to_path_buf();
    assert_eq!(
        w.close(b).expect("closed"),
        None,
        "a hidden tab: C stays shown"
    );
    assert!(!b_private.exists(), "B's private files went with it");
    assert_eq!(w.tabs.order(), [a, c]);

    // Untitled C: Cancel keeps it; a failed Save As keeps it, unsaved.
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Cancel)),
        Replace::Stay
    );
    assert_eq!(w.sessions.replacing(None), Replace::Stay);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Save)),
        Replace::AfterSave
    );
    let occupied = user.path().join("occupied.fcad");
    std::fs::write(&occupied, b"theirs").expect("occupied");
    let report = w.save(SaveTarget::As(occupied));
    assert!(!report.published && report.continuation.is_none());
    assert_eq!(w.tabs.order(), [a, c]);
    assert!(w.sessions.dirty() && w.sessions.untitled());
    // Discard: closed; the window shows nothing until A's picture is ready.
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    let c_private = w.sessions.private_directory().expect("c").to_path_buf();
    assert_eq!(
        w.close(c).expect("closed"),
        Some(a),
        "the neighbour is next"
    );
    assert!(!c_private.exists());
    assert!(!w.sessions.has_session());
    assert_eq!(w.scene.document, None, "nothing of C is drawn or exported");
    assert_eq!(w.sessions.title(), PRODUCT_NAME);
    // A is hidden and unsaved: shown first, then asked.
    w.switch(a);
    assert_eq!(
        w.tabs.close_step(&w.sessions, a, false),
        Some(CloseStep::Ask)
    );
    let report = w.save(SaveTarget::InPlace);
    assert!(report.published);
    assert_eq!(
        w.tabs.close_step(&w.sessions, a, false),
        Some(CloseStep::Now)
    );
    assert_eq!(w.close(a).expect("closed"), None, "the last tab");
    assert_eq!(w.tabs.count(), 0);
    assert!(w.tabs.labels(&w.sessions).is_empty());
    assert_eq!(height_of(&a_file), 22.0);

    // A device that refuses even the empty picture keeps the tab and its picture.
    w.open(&b_file);
    let b = w.sessions.tab();
    let shown = w.scene.document.clone();
    let refused = crate::close_shown(
        &mut w.scene,
        &mut w.input,
        &mut w.sessions,
        &mut w.tabs,
        &mut w.checkpoint_name,
        b,
        |_| Err(CadError::rendering("the device refused")),
    );
    assert!(refused.is_err());
    assert_eq!((w.sessions.tab(), w.tabs.count()), (b, 1));
    assert_eq!(w.scene.document, shown);
}

/// Quit asks about every unsaved tab in turn, the shown one first, each shown
/// before it is asked. Cancel part way closes nothing and undoes nothing.
#[test]
fn quit_asks_every_unsaved_tab_in_turn_and_cancel_part_way_closes_nothing() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let store_root = tempfile::tempdir().expect("store");
    let store = RecoveryStore::open(store_root.path()).expect("store");
    let mut w = Window::new(Drawn::Mock, Some(&store));
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(21.0);
    w.open(&b_file);
    let b = w.sessions.tab();
    w.create(NewDocument::Empty).expect("untitled");
    let c = w.sessions.tab();
    assert_eq!(w.tabs.order(), [a, b, c]);

    // First pass: C (shown) Save As → published; A is next and is shown first;
    // there Cancel stops the pass.
    w.tabs.begin_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
    let c_file = user.path().join("c.fcad");
    let quit_id = w.tabs.ask_quit_model(&w.sessions).expect("model question");
    let report = {
        let (tx, rx) = mpsc::channel();
        let generation = w
            .sessions
            .begin_save(
                SaveTarget::As(c_file.clone()),
                Some(Continuation::Quit(quit_id)),
                |plan, _, cancel| {
                    spawn_save(plan, cancel.clone(), move |v| tx.send(v).expect("save"))
                },
            )
            .expect("a save");
        w.sessions
            .finish_save(generation, wait(&rx))
            .expect("answered")
    };
    assert_eq!(report.continuation, Some(Continuation::Quit(quit_id)));
    assert!(w.tabs.decide_quit_model(&w.sessions, quit_id));
    assert_eq!(
        w.tabs.quit_step(&w.sessions, false),
        QuitStep::Show(a),
        "B is clean"
    );
    let (generation, rx) = w.begin_switch(a, Some(After::Quit));
    let (shown, after) = w
        .deliver_switch(generation, wait(&rx), Ok(()))
        .expect("awaited");
    shown.expect("A is shown");
    assert_eq!(after, Some(After::Quit));
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Cancel)),
        Replace::Stay
    );
    w.tabs.abort_quit();
    // Nothing was closed; C stays saved; A is unsaved and still has its copy.
    assert_eq!(w.tabs.order(), [a, b, c]);
    assert!(w.sessions.dirty() && w.sessions.tab() == a);
    assert!(!w.hidden(c).dirty());
    assert!(c_file.exists());

    // Second pass: Discard A. The tab stays open (and dirty) until the window
    // actually ends; then every tab's crash copy goes.
    w.tabs.begin_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    w.decide_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Exit);
    assert!(w.sessions.dirty(), "Discard acts only when the window ends");
    // A pass Cancelled after Discard forgets that answer.
    w.tabs.abort_quit();
    w.tabs.begin_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
    w.decide_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Exit);
    settled(&w.sessions);
    w.tabs.decide_exit(&mut w.sessions);
    w.sessions.stop_all();
    w.tabs.stop_all();
    assert_eq!(
        record_dirs(store_root.path()),
        0,
        "the person decided about every tab: no copy is kept"
    );
    assert_eq!(height_of(&a_file), 12.0, "Discard wrote nothing");
}

fn record_dirs(root: &Path) -> usize {
    std::fs::read_dir(root)
        .expect("store")
        .filter(|entry| {
            entry
                .as_ref()
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with("r-")
        })
        .count()
}

fn settled(s: &Sessions) -> Option<RecoveryStatus> {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !s.recovery_settled() {
        assert!(Instant::now() < deadline, "the recorder never settled");
        std::thread::sleep(Duration::from_millis(10));
    }
    s.recovery_status()
}

/// A late, cancelled or foreign answer changes no tab and releases no slot: a
/// switch given up on, a newer switch, a picture or kernel refusal, and an answer
/// addressed to another tab whose generation happens to match.
#[test]
fn late_cancelled_or_foreign_answers_change_no_tab_and_release_no_slot() {
    let user = tempfile::tempdir().expect("user");
    let files: Vec<PathBuf> = (0..3)
        .map(|i| {
            let file = user.path().join(format!("p{i}.fcad"));
            plate(&file, 10.0 + f64::from(i));
            file
        })
        .collect();
    let mut w = Window::new(Drawn::Mock, None);
    let tabs: Vec<TabId> = files
        .iter()
        .map(|file| {
            w.open(file);
            w.sessions.tab()
        })
        .collect();
    let (a, b, c) = (tabs[0], tabs[1], tabs[2]);
    let shown = w.scene.document.clone();

    // A switch to A, given up on: the slot is released at once; a switch to B
    // follows; A's late picture is nobody's and changes nothing.
    let (to_a, a_rx) = w.begin_switch(a, None);
    assert!(w.sessions.busy());
    assert!(
        w.sessions
            .begin_apply(|_, _, _| panic!("one operation"))
            .is_none()
    );
    assert!(w.sessions.cancel());
    w.tabs.cancel_switch();
    assert!(!w.sessions.busy());
    let (to_b, b_rx) = w.begin_switch(b, None);
    let late = wait(&a_rx);
    assert!(
        w.deliver_switch(to_a, late, Ok(())).is_none(),
        "a given-up switch's answer is dropped unseen"
    );
    assert_eq!(w.sessions.tab(), c);
    assert_eq!(w.scene.document, shown);
    assert!(w.sessions.busy(), "B's switch still holds the slot");
    let (outcome, _) = w
        .deliver_switch(to_b, wait(&b_rx), Ok(()))
        .expect("B's answer is awaited");
    outcome.expect("B shown");
    assert_eq!(w.sessions.tab(), b);
    assert!(!w.sessions.busy());

    // A picture the device refuses, a kernel that refuses, and Cancel after the
    // picture was ready: the shown tab stays, the target stays hidden.
    for refusal in ["upload", "kernel", "cancel"] {
        let shown = (w.sessions.tab(), w.scene.document.clone());
        let (generation, rx) = w.begin_switch(a, None);
        let mut loaded = wait(&rx);
        let mut upload = Ok(());
        match refusal {
            "upload" => upload = Err(CadError::rendering("the device refused")),
            "kernel" => loaded = Err(CadError::kernel("the kernel refused")),
            _ => {
                assert!(w.sessions.cancel());
            }
        }
        let (outcome, after) = w
            .deliver_switch(generation, loaded, upload)
            .expect("still awaited");
        assert!(outcome.is_err(), "{refusal}");
        assert_eq!(after, None);
        assert_eq!(
            (w.sessions.tab(), w.scene.document.clone()),
            shown,
            "{refusal}"
        );
        assert!(!w.sessions.busy(), "{refusal}: the slot is free again");
        assert!(w.tabs.hidden_sessions(a).is_some());
    }

    // B's Apply in flight; an answer addressed to A with the same generation
    // number (each tab counts its own) is not B's and releases nothing.
    w.switch(a);
    w.apply(13.0);
    let a_address = Address {
        tab: a,
        generation: 1,
    };
    w.switch(b);
    let feature = w.feature();
    let (tx, rx) = mpsc::channel();
    let b_address = w
        .sessions
        .begin_apply(|ticket, _, _| {
            std::thread::spawn(move || {
                let edit = ticket.edit_extrude_height(
                    feature,
                    17.0,
                    &mut MockKernel::new(),
                    &OperationContext::default(),
                );
                tx.send(edit).expect("edit");
            })
        })
        .expect("B's Apply");
    assert_eq!(b_address.generation, a_address.generation);
    let answer = wait(&rx);
    let foreign = Err(CadError::input("an answer for another tab"));
    assert_eq!(w.sessions.finish_apply(a_address, foreign), Edited::Ignore);
    assert!(w.sessions.busy(), "the foreign answer released nothing");
    assert!(
        !w.sessions.finish_scene(a_address, Ok(())),
        "nor ends the operation"
    );
    assert!(w.sessions.staged_path(a_address).is_none());
    let Edited::Show(path) = w.sessions.finish_apply(b_address, answer) else {
        panic!("B's own answer");
    };
    w.show_staged(b_address, path);
    assert_eq!(height_of(&w.sessions.export_path().expect("b")), 17.0);
    assert_eq!(height_of(&w.hidden(a).export_path().expect("a")), 13.0);
    println!("\nFCAD_30O_LATE_ANSWERS_EXECUTED");
}

/// Each tab owns its private files and its crash copy: work in one never ends
/// the other's record, closing one frees only its own, Recover opens a separate
/// tab, and an exit nobody decided keeps every unsaved tab's copy.
#[test]
fn each_tab_keeps_its_own_files_and_crash_copy_and_closing_frees_only_its_own() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let store_root = tempfile::tempdir().expect("store");
    let store = RecoveryStore::open(store_root.path()).expect("store");
    let mut w = Window::new(Drawn::Mock, Some(&store));
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(21.0);
    assert!(matches!(
        settled(&w.sessions),
        Some(RecoveryStatus::Written { .. })
    ));
    let a_private = w.sessions.private_directory().expect("a").to_path_buf();
    w.open(&b_file);
    let b = w.sessions.tab();
    w.apply(26.0);
    w.apply(27.0);
    assert!(matches!(
        settled(&w.sessions),
        Some(RecoveryStatus::Written { .. })
    ));
    let listed = store.list().expect("list");
    assert_eq!(
        listed.active, 2,
        "A's crash copy was ended by work in B: one held record per unsaved tab"
    );
    assert!(a_private.exists(), "A's private history outlives B's work");
    assert_eq!(height_of(&w.hidden(a).export_path().expect("a")), 21.0);

    // B closed after Discard: only B's record and private files go.
    let b_private = w.sessions.private_directory().expect("b").to_path_buf();
    w.close(b).expect("closed");
    let deadline = Instant::now() + Duration::from_secs(60);
    // Retirement removes the lease before its directory. Seeing one active
    // record does not yet mean the worker finished the directory removal.
    while store.list().expect("list").active != 1 || record_dirs(store_root.path()) != 1 {
        assert!(
            Instant::now() < deadline,
            "B's record was never fully retired"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!b_private.exists() && a_private.exists());
    assert_eq!(record_dirs(store_root.path()), 1);

    // A crash copy left by another window: Recover opens it as its own tab; A,
    // unsaved, is not replaced.
    let orphan = {
        let other = tempfile::tempdir().expect("other window");
        let mut session = DocumentSession::open_in(other.path(), &b_file, HistoryLimits::default())
            .expect("open");
        let mut lane = RecoveryRecorder::start(store.clone(), || {});
        let feature = read_extrude_source(session.current().path())
            .expect("reading")
            .features[0]
            .feature;
        let step = session
            .begin_step()
            .edit_extrude_height(
                feature,
                33.0,
                &mut MockKernel::new(),
                &OperationContext::default(),
            )
            .expect("edit");
        session.commit_step(step).expect("accepted");
        lane.observe(&mut session);
        let deadline = Instant::now() + Duration::from_secs(60);
        while !lane.settled() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(lane);
        store
            .list()
            .expect("list")
            .recoverable()
            .next()
            .expect("orphan")
            .record
    };
    w.switch(a);
    let (scene, recovered) = {
        let claim = store.claim(orphan).expect("claim");
        let session =
            DocumentSession::recover_in(w.private.path(), HistoryLimits::default(), claim)
                .expect("restored");
        let shown = session.current().path().to_path_buf();
        (w.picture(&shown), session)
    };
    let shown = recovered.current().path().to_path_buf();
    w.present(&shown, scene, Bind::Open(Box::new(recovered)), Ok(()))
        .expect("a new tab");
    let r = w.sessions.tab();
    assert_ne!(r, a);
    assert!(w.sessions.recovered() && w.sessions.dirty());
    assert_eq!(height_of(&w.sessions.export_path().expect("r")), 33.0);
    assert!(w.hidden(a).dirty(), "A is not replaced");
    assert_eq!(w.tabs.order(), [a, r]);

    // The window ends without anybody deciding: both unsaved tabs keep a copy.
    settled(&w.sessions);
    w.sessions.stop_all();
    w.tabs.stop_all();
    let mut kept: Vec<String> = store
        .list()
        .expect("list")
        .recoverable()
        .map(|entry| entry.name.clone())
        .collect();
    kept.sort();
    assert_eq!(
        kept,
        ["a.fcad", "b.fcad"],
        "A's and the recovered B's copies"
    );
}

/// A ninth document is refused before anything is made or replaced.
#[test]
fn the_ninth_document_is_refused_before_anything_changes() {
    let mut w = Window::new(Drawn::Mock, None);
    for _ in 0..MAX_TABS {
        w.create(NewDocument::Empty).expect("room");
    }
    assert_eq!(w.tabs.count(), MAX_TABS);
    let refused = w.create(NewDocument::Empty).expect_err("full");
    assert!(
        refused.to_string().contains("8 documents are open"),
        "{refused}"
    );
    let user = tempfile::tempdir().expect("user");
    let file = user.path().join("plate.fcad");
    plate(&file, 12.0);
    assert!(matches!(w.open(&file), Opening::Refused(_)));
    // A candidate that reached the bind anyway is refused there too.
    let shown_tab = w.sessions.tab();
    let shown = w.scene.document.clone();
    let session = DocumentSession::open_in(w.private.path(), &file, HistoryLimits::default())
        .expect("session");
    let dir = session.private_directory().to_path_buf();
    let path = session.current().path().to_path_buf();
    let scene = w.picture(&path);
    assert!(
        w.present(&path, scene, Bind::Open(Box::new(session)), Ok(()))
            .is_err()
    );
    assert_eq!((w.tabs.count(), w.sessions.tab()), (MAX_TABS, shown_tab));
    assert_eq!(w.scene.document, shown);
    assert!(!dir.exists());
    let last = w.sessions.tab();
    w.close(last).expect("closed");
    assert!(w.tabs.room().is_ok(), "closing one makes room");
}

/// Without a kernel the window's own scene worker cannot draw a hidden tab: the
/// switch is refused at its picture and the shown tab stays (stub build).
#[test]
fn stub_switching_is_refused_at_the_picture_and_keeps_the_shown_tab() {
    if ferritecad_occt::is_available() {
        return;
    }
    let mut w = Window::new(Drawn::Mock, None);
    w.create(NewDocument::Empty).expect("first");
    let first = w.sessions.tab();
    w.create(NewDocument::Empty).expect("second");
    let second = w.sessions.tab();
    let shown = w.scene.document.clone();
    w.drawn = Drawn::Native;
    let (generation, rx) = w.begin_switch(first, None);
    let refused = wait(&rx).map(|_| ()).expect_err("no kernel");
    let (outcome, _) = w
        .deliver_switch(generation, Err(refused), Ok(()))
        .expect("awaited");
    assert!(outcome.is_err());
    assert_eq!(
        (w.sessions.tab(), w.scene.document.clone()),
        (second, shown)
    );
    assert!(!w.sessions.busy());
    assert!(
        w.sessions
            .status
            .starts_with("The other document could not be shown"),
        "{}",
        w.sessions.status
    );
    assert!(w.tabs.hidden_sessions(first).is_some());
    println!("\nFCAD_30O_STUB_TABS_EXECUTED");
}

// --- native: Open CASCADE and PlaneGCS, against the shipped command line ---------

impl Window {
    /// The shown tab's Circle Apply, through the window's circle worker.
    fn apply_circle(&mut self, center: [f64; 2], radius: f64) {
        let reading =
            read_extrude_source(&self.sessions.export_path().expect("shown")).expect("reading");
        let choice = &reading.circle_sketches[0];
        let circle = choice.circle.as_ref().expect("a supported circle");
        let edit = ferritecad_document::CircleEdit {
            curve_id: circle.curve_id,
            center_mm: center,
            radius_mm: radius,
        };
        let (sketch, expected) = (choice.sketch, reading.version);
        let (tx, rx) = mpsc::channel();
        let generation = self
            .sessions
            .begin_apply(|ticket, _, cancel| {
                spawn_apply_circle(ticket, sketch, edit, expected, cancel.clone(), move |v| {
                    tx.send(v).expect("edit");
                })
            })
            .expect("started");
        let Edited::Show(path) = self.sessions.finish_apply(generation, wait(&rx)) else {
            panic!("the circle was not applied: {}", self.sessions.status);
        };
        self.show_staged(generation, path);
    }
}

/// The circle's own numbers as the document stores them.
fn circle_of(path: &Path) -> ([f64; 2], f64, f64) {
    let reading = read_extrude_source(path).expect("reading");
    let circle = reading.circle_sketches[0]
        .circle
        .clone()
        .expect("a supported circle");
    (circle.center_mm, circle.radius_mm, circle.height_mm)
}

/// Two small real models in two tabs: Apply alternates between them, Undo, Redo
/// and Restore of a checkpoint happen in A only, both export while unsaved, both
/// are saved and reopened, and each saved file is the command line's copy in
/// every SQL cell (A's checkpoint row aside, checked by extraction), keeps every
/// UUID and draws the expected geometry.
#[test]
fn native_two_tabs_alternate_applies_undo_restore_exports_and_saves_like_the_command_line() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let user = root.path();
    let a_file = user.join("plate.fcad");
    plate(&a_file, 12.0);
    let b_file = super::analytic::create(
        user,
        "create-circle-extrude",
        r#"{"schema_version":1,"center_mm":[12.5,-7.25],"radius_mm":10.5,"height_mm":15.25}"#,
    );
    let a_original = user.join("a-original.fcad");
    std::fs::copy(&a_file, &a_original).expect("kept");
    let b_original = user.join("b-original.fcad");
    std::fs::copy(&b_file, &b_original).expect("kept");
    let (a_ids, b_ids) = (
        super::checkpoints::ids_and_refs(&a_file),
        super::checkpoints::ids_and_refs(&b_file),
    );

    // The command line's copies of the final models, from the untouched files.
    let a_feature = read_extrude_source(&a_file).expect("a").features[0].feature;
    let a_version = read_extrude_source(&a_file).expect("a").version;
    let a_peer = user.join("a-peer.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        a_file.as_os_str(),
        "--feature".as_ref(),
        a_feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        a_version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "26".as_ref(),
        "-o".as_ref(),
        a_peer.as_os_str(),
    ]);
    let b_reading = read_extrude_source(&b_file).expect("b");
    let b_curve = b_reading.circle_sketches[0]
        .circle
        .as_ref()
        .expect("circle")
        .curve_id;
    let request = user.join("b-request.json");
    std::fs::write(
        &request,
        format!(
            r#"{{"request_version":1,"curve_id":"{b_curve}","center_mm":[-3.5,4.25],"radius_mm":8}}"#
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

    let mut w = Window::new(Drawn::Native, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.checkpoint(CheckpointAction::Create("A at 12".to_owned()));
    let kept = w.listed()[0].clone();
    w.apply(20.0);
    w.open(&b_file);
    let b = w.sessions.tab();
    w.apply_circle([-3.5, 4.25], 6.75);
    w.switch(a);
    assert_eq!(height_of(&w.sessions.export_path().expect("a")), 20.0);
    w.apply(26.0);
    w.switch(b);
    assert_eq!(
        circle_of(&w.sessions.export_path().expect("b")).1,
        6.75,
        "B's own model"
    );
    w.apply_circle([-3.5, 4.25], 8.0);
    w.switch(a);
    w.step(true);
    assert_eq!(height_of(&w.sessions.export_path().expect("a")), 20.0);
    w.step(false);
    w.checkpoint(CheckpointAction::Restore(kept.id));
    assert_eq!(height_of(&w.sessions.export_path().expect("a")), 12.0);
    assert_eq!(w.listed(), vec![kept.clone()], "Restore keeps the list");
    w.step(true);
    assert_eq!(height_of(&w.sessions.export_path().expect("a")), 26.0);
    assert!(w.sessions.can_redo(), "A's Redo of the Restore waits");
    assert!(w.hidden(b).dirty() && !w.hidden(b).can_redo());
    assert_eq!(circle_of(&w.hidden(b).export_path().expect("b")).1, 8.0);
    for (file, original) in [(&a_file, &a_original), (&b_file, &b_original)] {
        assert_eq!(
            std::fs::read(file).expect("file"),
            std::fs::read(original).expect("original"),
            "nothing is written before Save"
        );
    }

    // Unsaved exports of each tab's own working model, by the window's workers.
    let work = tempfile::tempdir().expect("work");
    let a_alias = w.sessions.suggestion().expect("alias");
    let (a_stl, a_fbx) = export_bytes(
        &w.sessions.export_path().expect("a"),
        &a_alias,
        work.path(),
        "a-unsaved",
    );
    w.switch(b);
    let b_alias = w.sessions.suggestion().expect("alias");
    let (b_stl, b_fbx) = export_bytes(
        &w.sessions.export_path().expect("b"),
        &b_alias,
        work.path(),
        "b-unsaved",
    );
    let (a_peer_stl, a_peer_fbx) = peer_bytes(&a_peer, work.path(), "a");
    let (b_peer_stl, b_peer_fbx) = peer_bytes(&b_peer, work.path(), "b");
    assert_eq!(
        (&a_stl, &a_fbx),
        (&a_peer_stl, &a_peer_fbx),
        "A's unsaved exports"
    );
    assert_eq!(
        (&b_stl, &b_fbx),
        (&b_peer_stl, &b_peer_fbx),
        "B's unsaved exports"
    );

    // Save both (B shown, then A), and reopen them as two new tabs of a new window.
    assert!(w.save(SaveTarget::InPlace).published);
    w.switch(a);
    assert!(w.save(SaveTarget::InPlace).published);
    w.sessions.stop_all();
    w.tabs.stop_all();
    let mut again = Window::new(Drawn::Native, None);
    again.open(&a_file);
    assert_eq!(again.listed(), vec![kept.clone()]);
    assert_eq!(height_of(&a_file), 26.0);
    again.open(&b_file);
    assert!(again.listed().is_empty());
    assert_eq!(circle_of(&b_file), ([-3.5, 4.25], 8.0, 15.25));

    // Every SQL cell: B is the command line's copy with only the write stamp set
    // aside; A too, with its checkpoint row set aside — that row is checked by
    // extracting it: the image is the original A in every cell, stamp included.
    use super::checkpoints::all_sql;
    assert_eq!(all_sql(&b_file, &[], false), all_sql(&b_peer, &[], false));
    assert_eq!(
        all_sql(&a_file, &["checkpoints"], false),
        all_sql(&a_peer, &["checkpoints"], false)
    );
    assert_eq!(all_sql(&a_file, &[], false)["checkpoints"].len(), 1);
    let extracted = work.path().join("a-checkpoint.fcad");
    cli(&[
        "extract-checkpoint".as_ref(),
        a_file.as_os_str(),
        "--checkpoint".as_ref(),
        kept.id.to_string().as_ref(),
        "--output".as_ref(),
        extracted.as_os_str(),
    ]);
    assert_eq!(
        all_sql(&extracted, &[], true),
        all_sql(&a_original, &[], true)
    );
    // Identity: every object UUID and saved reference kept in both.
    assert_eq!(super::checkpoints::ids_and_refs(&a_file), a_ids);
    assert_eq!(super::checkpoints::ids_and_refs(&b_file), b_ids);

    // Geometry, read independently from the saved files' exports.
    let (a_saved_stl, a_saved_fbx) = peer_bytes(&a_file, work.path(), "a-saved");
    let (b_saved_stl, b_saved_fbx) = peer_bytes(&b_file, work.path(), "b-saved");
    assert_eq!((&a_saved_stl, &a_saved_fbx), (&a_stl, &a_fbx));
    assert_eq!((&b_saved_stl, &b_saved_fbx), (&b_stl, &b_fbx));
    let (_, height_a, volume_a) = super::checkpoints::stl_facts(&a_stl);
    assert!((height_a - 26.0).abs() < 1e-4, "{height_a}");
    assert!((volume_a - 80.0 * 40.0 * 26.0).abs() < 1e-2, "{volume_a}");
    super::analytic::assert_round(&b_stl, [-3.5, 4.25], 8.0, None, 15.25);
    let (_, height_b, volume_b) = super::checkpoints::stl_facts(&b_stl);
    if let Ok(dir) = std::env::var("FCAD_TABS_SESSION_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        for (name, stl, fbx) in [("tabs-a", &a_stl, &a_fbx), ("tabs-b", &b_stl, &b_fbx)] {
            std::fs::write(dir.join(format!("{name}.stl")), stl).expect("artifact");
            std::fs::write(dir.join(format!("{name}.fbx")), fbx).expect("artifact");
        }
    }
    println!(
        "\nFCAD_30O_NATIVE_TABS_EXECUTED volume_a={volume_a:.3} height_b={height_b:.3} \
         volume_b={volume_b:.3}"
    );
}

/// Open CASCADE without the solver: two unconstrained plates in two tabs are
/// edited and switched between with the window's own workers.
#[test]
fn mixed_tabs_switch_and_apply_with_occt_and_no_solver() {
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
    w.apply(18.0);
    w.open(&b_file);
    let b = w.sessions.tab();
    w.apply(19.0);
    w.switch(a);
    assert_eq!(height_of(&w.sessions.export_path().expect("a")), 18.0);
    assert!(!w.scene.catalogue.is_empty(), "A is drawn by Open CASCADE");
    w.switch(b);
    assert_eq!(height_of(&w.sessions.export_path().expect("b")), 19.0);
    println!("\nFCAD_30O_MIXED_TABS_EXECUTED");
}

// --- the real window's artifacts --------------------------------------------------

/// What the window recipe (see the verification record) leaves in its root.
const GUI_OUTPUTS: [&str; 6] = [
    "a.fcad",
    "b.fcad",
    "a-unsaved.stl",
    "a-unsaved.fbx",
    "b-unsaved.stl",
    "b-unsaved.fbx",
];

const CIRCLE_JSON: &str =
    r#"{"schema_version":1,"center_mm":[12.5,-7.25],"radius_mm":10.5,"height_mm":15.25}"#;

/// The inputs the generator makes (`tools/document-tabs-gui.py`): the plate and the
/// circle by the shipped command line, their pristine copies, an empty recovery
/// folder. No window output.
fn make_inputs(root: &Path) {
    std::fs::create_dir_all(root.join("inputs")).expect("inputs");
    std::fs::create_dir_all(root.join("recovery")).expect("recovery");
    let a = root.join("a.fcad");
    cli(&[
        "create".as_ref(),
        a.as_os_str(),
        "--sample".as_ref(),
        "--size".as_ref(),
        "80".as_ref(),
        "40".as_ref(),
        "12".as_ref(),
    ]);
    let made = super::analytic::create(root, "create-circle-extrude", CIRCLE_JSON);
    std::fs::rename(&made, root.join("b.fcad")).expect("b");
    std::fs::remove_file(root.join("create-circle-extrude.json")).expect("request");
    for name in ["a.fcad", "b.fcad"] {
        std::fs::copy(root.join(name), root.join("inputs").join(name)).expect("pristine");
    }
}

/// The command line's copies of what the recipe makes of each input.
fn peers(inputs: &Path, work: &Path) -> (PathBuf, PathBuf) {
    let a = inputs.join("a.fcad");
    let reading = read_extrude_source(&a).expect("a");
    let a_peer = work.join("a-peer.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        a.as_os_str(),
        "--feature".as_ref(),
        reading.features[0].feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        reading.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "26".as_ref(),
        "-o".as_ref(),
        a_peer.as_os_str(),
    ]);
    let b = inputs.join("b.fcad");
    let reading = read_extrude_source(&b).expect("b");
    let choice = &reading.circle_sketches[0];
    let curve = choice.circle.as_ref().expect("circle").curve_id;
    let request = work.join("b-request.json");
    std::fs::write(
        &request,
        format!(
            r#"{{"request_version":1,"curve_id":"{curve}","center_mm":[-3.5,4.25],"radius_mm":8}}"#
        ),
    )
    .expect("request");
    let b_peer = work.join("b-peer.fcad");
    cli(&[
        "edit-circle".as_ref(),
        b.as_os_str(),
        "--sketch".as_ref(),
        choice.sketch.to_string().as_ref(),
        "--expect-version".as_ref(),
        reading.version.content.to_string().as_ref(),
        "--request".as_ref(),
        request.as_os_str(),
        "-o".as_ref(),
        b_peer.as_os_str(),
    ]);
    (a_peer, b_peer)
}

/// Compares the outputs of the window recipe in `root` with the shipped command
/// line. Reads only what the window wrote; refuses a missing output before any
/// peer job.
fn compare_gui(root: &Path) {
    use super::checkpoints::{all_sql, ids_and_refs, stl_facts};
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
    let (a_peer, b_peer) = peers(&root.join("inputs"), work.path());
    let (a, b) = (root.join("a.fcad"), root.join("b.fcad"));

    // A: the command line's model in every SQL cell but the stamp and the list;
    // the list is exactly the checkpoint made from the file as it was opened.
    let listing = ferritecad_jobs::list_checkpoints(&a).expect("lists");
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names == ["A 12"],
        "the saved checkpoints of a.fcad: {names:?}"
    );
    assert!(
        all_sql(&a, &["checkpoints"], false) == all_sql(&a_peer, &["checkpoints"], false),
        "a.fcad is not the command line's model"
    );
    let extracted = work.path().join("a-12.fcad");
    cli(&[
        "extract-checkpoint".as_ref(),
        a.as_os_str(),
        "--checkpoint".as_ref(),
        listing.entries[0].id.to_string().as_ref(),
        "--output".as_ref(),
        extracted.as_os_str(),
    ]);
    assert!(
        all_sql(&extracted, &[], true) == all_sql(&root.join("inputs/a.fcad"), &[], true),
        "checkpoint A 12 is not the opened a.fcad"
    );
    assert!(
        ids_and_refs(&a) == ids_and_refs(&root.join("inputs/a.fcad")),
        "a.fcad identities"
    );
    // B: the command line's model in every SQL cell but the stamp; no list.
    assert!(
        all_sql(&b, &[], false) == all_sql(&b_peer, &[], false),
        "b.fcad is not the command line's model"
    );
    assert!(
        ferritecad_jobs::list_checkpoints(&b)
            .expect("lists")
            .entries
            .is_empty(),
        "b.fcad has a checkpoint"
    );
    assert!(
        ids_and_refs(&b) == ids_and_refs(&root.join("inputs/b.fcad")),
        "b.fcad identities"
    );

    // The window's unsaved exports, byte for byte the command line's, and the
    // geometry read independently.
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
    let (_, height, volume) = stl_facts(&a_stl);
    assert!((height - 26.0).abs() < 1e-4 && (volume - 80.0 * 40.0 * 26.0).abs() < 1e-2);
    super::analytic::assert_round(&b_stl, [-3.5, 4.25], 8.0, None, 15.25);

    // Every unsaved tab was decided at Quit: nothing is left to recover.
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let listing = store.list().expect("list");
    assert!(
        listing.recoverable().count() == 0 && listing.active == 0,
        "a recovery record was left behind"
    );
}

fn gui_control(root: &Path, name: &str, why: &str, breaks: &dyn Fn(&Path)) {
    control_of(compare_gui, root, name, why, breaks);
}

/// One negative control: a copy of `root` broken by `breaks` must be refused by
/// `compare` for the reason `why`.
fn control_of(compare: fn(&Path), root: &Path, name: &str, why: &str, breaks: &dyn Fn(&Path)) {
    let copy = tempfile::tempdir().expect("control");
    for entry in super::add_fillet::walk(root) {
        let to = copy.path().join(entry.strip_prefix(root).expect("inside"));
        std::fs::create_dir_all(to.parent().expect("parent")).expect("dir");
        std::fs::copy(&entry, &to).expect("copy");
    }
    breaks(copy.path());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compare(copy.path())));
    std::panic::set_hook(previous);
    let payload = outcome.expect_err(&format!("negative control {name} accepted"));
    let refused = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default();
    assert!(
        refused.contains(why),
        "control {name} refused for another reason: {refused}"
    );
}

/// The comparator's positive run and its six negative controls, each on a copy.
fn compare_with_controls(root: &Path) {
    compare_gui(root);
    let before = cli_runs();
    gui_control(
        root,
        "missing",
        "missing real GUI output: b-unsaved.fbx",
        &|r| std::fs::remove_file(r.join("b-unsaved.fbx")).expect("remove"),
    );
    assert_eq!(
        cli_runs(),
        before,
        "a peer job ran before a missing output was refused"
    );
    let file = |r: &Path, name: &str| std::fs::read(r.join(name)).expect(name);
    gui_control(root, "unsaved a", "a.fcad was never saved", &|r| {
        std::fs::write(r.join("a.fcad"), file(r, "inputs/a.fcad")).expect("w");
    });
    gui_control(
        root,
        "b saved as a",
        "b.fcad is not the command line's model",
        &|r| {
            std::fs::write(r.join("b.fcad"), file(r, "a.fcad")).expect("w");
        },
    );
    gui_control(
        root,
        "checkpoint lost",
        "the saved checkpoints of a.fcad",
        &|r| {
            let work = tempfile::tempdir().expect("work");
            let (a_peer, _) = peers(&r.join("inputs"), work.path());
            std::fs::write(r.join("a.fcad"), std::fs::read(a_peer).expect("peer")).expect("w");
        },
    );
    gui_control(
        root,
        "exports swapped",
        "a-unsaved.stl against the CLI",
        &|r| {
            std::fs::write(r.join("a-unsaved.stl"), file(r, "b-unsaved.stl")).expect("w");
        },
    );
    gui_control(
        root,
        "record left",
        "a recovery record was left behind",
        &|r| {
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
        },
    );
}

/// The real window's outputs in `FCAD_30O_GUI_DIR` against the command line, then
/// six negative controls on copies of those same outputs. Nothing here makes a
/// window output; without the variable the test does nothing.
#[test]
fn native_compare_real_tabs_gui_artifacts_with_negative_controls() {
    let Some(root) = std::env::var_os("FCAD_30O_GUI_DIR") else {
        return;
    };
    assert!(native(), "the comparison needs Open CASCADE");
    compare_with_controls(Path::new(&root));
    println!("\nFCAD_30O_GUI_COMPARE_OK negative_controls=6 all_SQL_cells=true");
}

/// The comparator's own check, not window evidence: the recipe's steps run on the
/// window's owners without a window (the same `Tabs`, `Sessions`, workers and
/// arrival statement), their files go through the comparator and its controls.
#[test]
fn native_tabs_scenario_on_session_files_passes_the_comparator_and_its_controls() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let root = root.path();
    make_inputs(root);
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let mut w = Window::new(Drawn::Native, Some(&store));
    let (a_file, b_file) = (root.join("a.fcad"), root.join("b.fcad"));
    // 1–3: A opens; checkpoint A 12; height 20.
    w.open(&a_file);
    let a = w.sessions.tab();
    w.checkpoint(CheckpointAction::Create("A 12".to_owned()));
    let kept = w.listed()[0].clone();
    w.apply(20.0);
    // 4–5: B opens in a new tab with no question; its circle is changed.
    assert_eq!(w.open(&b_file), Opening::Load);
    let b = w.sessions.tab();
    w.apply_circle([-3.5, 4.25], 8.0);
    // 6: back to A: 26, Undo, Redo, Restore A 12, Undo; unsaved exports.
    w.switch(a);
    w.apply(26.0);
    w.step(true);
    w.step(false);
    w.checkpoint(CheckpointAction::Restore(kept.id));
    w.step(true);
    let alias = w.sessions.suggestion().expect("alias");
    export_bytes(
        &w.sessions.export_path().expect("a"),
        &alias,
        root,
        "a-unsaved",
    );
    // 7: Open of A again shows A; Open of B shows B's tab; B's unsaved exports.
    assert_eq!(w.open(&a_file), Opening::Shown);
    let Opening::Show(shown) = w.open(&b_file) else {
        panic!("B is shown in its tab");
    };
    w.switch(shown);
    let alias = w.sessions.suggestion().expect("alias");
    export_bytes(
        &w.sessions.export_path().expect("b"),
        &alias,
        root,
        "b-unsaved",
    );
    // 8–9: New Empty; Save As onto a.fcad refused; Cancel keeps, Discard closes it.
    w.create(NewDocument::Empty).expect("untitled");
    let untitled = w.sessions.tab();
    assert!(w.tabs.save_as_refusal(&w.sessions, &a_file).is_some());
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Cancel)),
        Replace::Stay
    );
    let next = w.close(untitled).expect("closed").expect("a neighbour");
    assert_eq!(next, b);
    w.switch(b);
    // 10: Quit: B (shown) Save; A shown and asked; Cancel. Nothing closes.
    w.tabs.begin_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
    assert!(w.save(SaveTarget::InPlace).published);
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Show(a));
    w.show_for_quit(a);
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
    w.tabs.abort_quit();
    assert_eq!(w.tabs.order(), [a, b]);
    // 11: Quit again: A Save; the window ends by the person's decision.
    w.tabs.begin_quit();
    assert!(w.save(SaveTarget::InPlace).published);
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Exit);
    w.tabs.decide_exit(&mut w.sessions);
    w.sessions.stop_all();
    w.tabs.stop_all();
    compare_with_controls(root);
    println!("\nFCAD_30O_SESSION_FILES_COMPARE_OK negative_controls=6 all_SQL_cells=true");
}

/// Native Quit and window-close enter through this same boundary, before the
/// clean-document fast path. Neither an unaccepted form nor ongoing work may be
/// discarded merely because the accepted model still matches the file on disk.
#[test]
fn quit_waits_for_open_forms_and_foreground_work_even_when_tabs_are_clean() {
    let user = tempfile::tempdir().expect("user");
    let file = user.path().join("clean.fcad");
    plate(&file, 12.0);
    let original = std::fs::read(&file).expect("source");
    for held in ["new", "load", "session", "switch"] {
        let mut w = Window::new(Drawn::Mock, None);
        w.open(&file);
        let tab = w.sessions.tab();
        let current = w.sessions.export_path().expect("accepted");
        let mut creates = crate::creates::Creates::default();
        let mut edits = crate::edits::Edits::default();
        let mut loads = crate::Loads::default();
        let exports = crate::exports::Exports::default();
        match held {
            "new" => assert!(crate::creates::open_form(&mut creates, &mut w.input)),
            "edit" => {
                let reading = read_extrude_source(&current).expect("facts");
                assert!(edits.begin(&current, &reading));
            }
            "load" => {
                loads.open(
                    Some(&file),
                    Arc::new(crate::ProgressRelay::default()),
                    |_, _| std::thread::spawn(|| {}),
                );
            }
            "session" => {
                assert!(
                    w.sessions
                        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
                        .is_some()
                );
            }
            _ => assert!(w.sessions.hold_switch(41)),
        }
        assert!(!w.sessions.dirty(), "the accepted model is clean");
        let begun = crate::begin_window_quit(
            &mut w.tabs,
            &mut w.sessions,
            &creates,
            &loads,
            &exports,
            &edits,
            &w.input,
        );
        if held == "new" {
            assert!(begun, "§30W asks before discarding idle New");
            assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::New);
            let id = w.tabs.quit_new_question().expect("New question");
            assert!(crate::answer_window_quit_new(
                &mut w.tabs,
                &mut w.sessions,
                &mut creates,
                &mut edits,
                &loads,
                &exports,
                &mut w.input,
                id,
                ferritecad_ui::CloseFormChoice::Back
            ));
            assert!(creates.form().is_some(), "Back lost idle New");
        } else {
            assert!(!begun, "Quit must keep an open {held}");
            assert!(
                !w.tabs.quitting(),
                "a blocked Quit entered its clean fast path"
            );
            assert!(w.sessions.status.contains("before quitting"));
        }
        assert_eq!(w.sessions.tab(), tab);
        assert_eq!(w.sessions.export_path().as_ref(), Some(&current));
        assert_eq!(std::fs::read(&file).expect("unchanged"), original);
        loads.stop_all();
    }
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&file);
    assert!(crate::begin_window_quit(
        &mut w.tabs,
        &mut w.sessions,
        &crate::creates::Creates::default(),
        &crate::Loads::default(),
        &crate::exports::Exports::default(),
        &crate::edits::Edits::default(),
        &w.input,
    ));
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Exit);
}

/// Cancelling a switch must invalidate it immediately, not only when another
/// switch happens to replace it. A later Apply keeps its status and slot.
#[test]
fn a_cancelled_switch_reply_is_ignored_while_an_apply_runs() {
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
    let (switch, scene_rx) = w.begin_switch(a, None);
    let late = wait(&scene_rx);
    assert!(w.sessions.cancel());
    w.tabs.cancel_switch();
    let feature = w.feature();
    let (tx, rx) = mpsc::channel();
    let edit = w
        .sessions
        .begin_apply(|ticket, _, _| {
            std::thread::spawn(move || {
                let result = ticket.edit_extrude_height(
                    feature,
                    19.0,
                    &mut MockKernel::new(),
                    &OperationContext::default(),
                );
                let _ = tx.send(result);
            })
        })
        .expect("Apply follows Cancel");
    let status = w.sessions.status.clone();
    assert!(
        w.deliver_switch(switch, late, Ok(())).is_none(),
        "a cancelled switch must be dropped before upload or status changes"
    );
    assert_eq!(w.sessions.status, status);
    assert_eq!(w.sessions.tab(), b);
    assert!(w.sessions.busy(), "Apply still holds the slot");
    let Edited::Show(path) = w.sessions.finish_apply(edit, wait(&rx)) else {
        panic!("the real Apply answer is still awaited");
    };
    w.show_staged(edit, path);
    assert_eq!(height_of(&w.sessions.export_path().expect("B")), 19.0);
    assert_eq!(height_of(&w.hidden(a).export_path().expect("A")), 12.0);
}

#[test]
fn a_cancelled_switch_keeps_its_input_alive_until_its_worker_finishes() {
    let user = tempfile::tempdir().expect("user");
    let a_file = user.path().join("a.fcad");
    let b_file = user.path().join("b.fcad");
    plate(&a_file, 12.0);
    plate(&b_file, 15.0);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    let private = w
        .sessions
        .private_directory()
        .expect("A directory")
        .to_path_buf();
    w.open(&b_file);
    let (release, blocked) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    w.tabs
        .begin_switch(&mut w.sessions, a, None, |path, _, _| {
            std::thread::spawn(move || {
                // A kernel or filesystem call cannot necessarily stop immediately.
                // Keep the actual file readable until the cancelled worker returns.
                if blocked.recv_timeout(Duration::from_secs(10)).is_ok() {
                    let _ = tx.send(mock_scene(&path));
                }
            })
        })
        .expect("started");
    assert!(w.sessions.cancel());
    w.tabs.cancel_switch();
    assert_eq!(w.close(a).expect("close hidden A"), None);
    assert!(
        private.exists(),
        "a retired reader still owns its snapshot lease"
    );
    release.send(()).expect("release read");
    wait(&rx).expect("the closed tab's input is still readable");
    w.tabs.stop_all();
    assert!(!private.exists(), "joined worker releases the final lease");
}
