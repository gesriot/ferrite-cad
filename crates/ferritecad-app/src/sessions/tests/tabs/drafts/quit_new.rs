// SPDX-License-Identifier: MIT
//! §30W: the window's New and every tab form, on the real owners without a window.
use super::*;
use crate::creates::{CreateStatus, tests::hold_creation};
use crate::last_tabs::{Folder, LastTabs};
use crate::restores::Restores;
use crate::tabs::QuitNewId;
use ferritecad_ui::{CloseFormChoice, NewChoice, NewContent};

impl Window {
    fn answer_quit_new(&mut self, id: QuitNewId, choice: CloseFormChoice) -> bool {
        crate::answer_window_quit_new(
            &mut self.tabs,
            &mut self.sessions,
            &mut self.creates,
            &mut self.edits,
            &Loads::default(),
            &Exports::default(),
            &mut self.input,
            id,
            choice,
        )
    }
    fn hold_quit_new(&mut self) -> QuitNewId {
        assert!(self.begin_quit(), "idle New must allow Quit");
        let id = self.tabs.quit_new_question().expect("New question");
        assert_eq!(self.tabs.quit_step(&self.sessions, true), QuitStep::New);
        assert!(self.tabs.ask_quit_model(&self.sessions).is_none());
        assert!(self.answer_quit_new(id, CloseFormChoice::Discard));
        assert!(self.creates.has_held_new());
        id
    }
    fn literal_new(&mut self, content: NewContent) {
        let form = self.creates.form().expect("New form");
        form.content = content;
        form.width = " 50x ".into();
        form.depth = " 30.0 ".into();
        form.height = " 7e0 ".into();
    }
    fn assert_literal_new(&mut self, content: NewContent) {
        let form = self.creates.form().expect("held New lost on late cancel");
        assert_eq!(form.content, content);
        assert_eq!(
            (&*form.width, &*form.depth, &*form.height),
            (" 50x ", " 30.0 ", " 7e0 ")
        );
    }
    fn create_returned_form(&mut self, content: NewContent) {
        self.assert_literal_new(content);
        if content == NewContent::SamplePlate {
            assert!(
                self.answer_new(NewChoice::Create).is_none(),
                "invalid literal was silently fixed"
            );
            self.creates.form().expect("kept").width = " 50.0 ".into();
        }
        let request = self
            .answer_new(NewChoice::Create)
            .expect("returned form submits");
        self.create_from_new(request, Ok(()))
            .expect("original creation job");
        assert!(self.sessions.untitled() && self.sessions.dirty());
        assert!(!self.creates.making_new() && !self.creates.has_held_new());
        let source = self.sessions.export_path().expect("created");
        let reading = read_extrude_source(&source).expect("created model");
        if content == NewContent::SamplePlate {
            assert_eq!(reading.features[0].distance_mm, Some(7.));
            assert_eq!(
                reading.sketches[0]
                    .vertices
                    .as_ref()
                    .expect("vertices")
                    .iter()
                    .map(|v| v.start_mm)
                    .collect::<Vec<_>>(),
                vec![[0., 0.], [50., 0.], [50., 30.], [0., 30.]]
            );
        } else {
            assert!(reading.features.is_empty());
        }
    }
}

#[test]
fn idle_empty_and_sample_new_back_late_abort_and_original_create_are_executable() {
    for content in [NewContent::Empty, NewContent::SamplePlate] {
        let mut w = Window::new(Drawn::Mock, None);
        assert!(w.begin_new(false));
        w.literal_new(content);
        assert!(w.begin_quit());
        let first = w.tabs.quit_new_question().expect("question");
        assert!(!w.begin_quit(), "repeat Quit reset New question");
        assert_eq!(w.tabs.quit_new_question(), Some(first));
        assert!(w.answer_quit_new(first, CloseFormChoice::Back));
        w.assert_literal_new(content);
        assert_eq!(w.tabs.count(), 0, "New became a fictitious tab");
        w.hold_quit_new();
        w.stop_quit();
        w.assert_literal_new(content);
        w.create_returned_form(content);
        assert_eq!(w.tabs.count(), 1);
        assert!(w.begin_quit());
        assert!(
            w.tabs.quit_new_question().is_none(),
            "accepted creation asked about old New"
        );
        w.decide_quit();
        let mut restores = Restores::default();
        assert_eq!(w.complete_quit(&mut restores), crate::QuitEnd::Exit);
    }
    let mut empty = Window::new(Drawn::Mock, None);
    assert!(empty.begin_new(false));
    empty.hold_quit_new();
    assert_eq!(
        empty.complete_quit(&mut Restores::default()),
        crate::QuitEnd::Exit
    );
    assert!(!empty.creates.has_held_new() && empty.tabs.count() == 0);
    println!("\nFCAD_30W_IDLE_FORMS_EXECUTED");
}

fn two_forms(w: &mut Window, root: &Path) -> (TabId, TabId, ObjectId, ObjectId) {
    let a_file = root.join("a.fcad");
    let b_file = root.join("b.fcad");
    plate(&a_file, 12.);
    plate(&b_file, 15.);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(18.);
    w.open_height_form();
    let af = w.feature();
    w.edits.type_height(af, " 26x ");
    w.open(&b_file);
    let b = w.sessions.tab();
    w.apply(22.);
    w.open_height_form();
    let bf = w.feature();
    w.edits.type_height(bf, " Bx ");
    w.round(a);
    (a, b, af, bf)
}

fn drawing_with_history(w: &mut Window) -> (Vec<[String; 2]>, usize, usize) {
    assert!(w.begin_new(true));
    crate::sketch::tests::fill_drawing(&mut w.creates.sketch);
    crate::sketch::tests::scribble_drawing(&mut w.creates.sketch)
}

fn finish_returned_drawing(w: &mut Window, before: &(Vec<[String; 2]>, usize, usize)) {
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch).as_ref(),
        Some(before),
        "New contour/history lost"
    );
    crate::sketch::tests::press(&mut w.creates.sketch, "Redo draft");
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch)
            .expect("drawing")
            .0[0][0],
        " 33.0 "
    );
    crate::sketch::tests::press(&mut w.creates.sketch, "Undo draft");
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch).as_ref(),
        Some(before)
    );
    crate::sketch::tests::press(&mut w.creates.sketch, "Undo draft");
    w.creates.set_can_create(true);
    crate::sketch::tests::press(&mut w.creates.sketch, "Close contour");
    crate::sketch::tests::press(&mut w.creates.sketch, crate::sketch::CREATE_DRAWN);
    let request = w
        .creates
        .sketch
        .take_request()
        .expect("returned drawing submits");
    if w.drawn == Drawn::Native {
        w.create_from_new(request, Ok(()))
            .expect("original drawing job");
    } else {
        // Same DocumentSession creation job with an explicit mock factory. This
        // is owner/action evidence; the separate true-stub gate proves refusal.
        let (tx, rx) = mpsc::channel();
        let root = w.private.path().to_path_buf();
        let g = crate::start_new(
            &mut w.creates,
            &Loads::default(),
            &Exports::default(),
            &mut w.input,
            request,
            move |content, _, cancel| {
                let context = OperationContext::default().with_cancel(cancel.clone());
                std::thread::spawn(move || {
                    let result = DocumentSession::create_document_in(
                        &root,
                        HistoryLimits::default(),
                        content,
                        || Ok(MockKernel::new()),
                        &context,
                    )
                    .and_then(|session| {
                        Ok(crate::creates::Candidate {
                            scene: mock_scene(session.current().path())?,
                            session,
                        })
                    });
                    tx.send(result).expect("answer");
                })
            },
        )
        .expect("original job starts");
        let candidate = crate::creates::finish_create(&mut w.creates, &mut w.input, g, wait(&rx))
            .expect("mock candidate");
        w.bind_candidate(candidate, Ok(()))
            .expect("returned drawing accepted");
    }
    let reading = read_extrude_source(&w.sessions.export_path().expect("New model")).expect("read");
    assert_eq!(
        reading.sketches[0]
            .vertices
            .as_ref()
            .expect("vertices")
            .iter()
            .map(|v| v.start_mm)
            .collect::<Vec<_>>(),
        vec![[0., 0.], [20., 0.], [20., 10.]]
    );
    assert_eq!(reading.features[0].distance_mm, Some(10.));
}

#[test]
fn late_cancel_after_save_or_deferred_discard_returns_new_above_b_and_both_tab_forms() {
    for save in [true, false] {
        let root = tempfile::tempdir().expect("root");
        let mut w = Window::new(Drawn::Mock, None);
        let (a, b, af, bf) = two_forms(&mut w, root.path());
        let before = drawing_with_history(&mut w);
        assert!(w.tabs.has_aside());
        let tab_count = w.tabs.count();
        w.hold_quit_new();
        assert_eq!(
            w.edits.typed(),
            Some((Some(af), " 26x ")),
            "underlying A form lost before question"
        );
        w.confirm_quit();
        if save {
            assert!(w.save_quit(SaveTarget::InPlace, false).published);
        } else {
            assert_eq!(
                w.sessions.replacing(Some(UnsavedChoice::Discard)),
                Replace::Discarded
            );
            w.decide_quit();
        }
        w.show_for_quit(b);
        let b_question = w.confirm_quit();
        assert_eq!(
            w.sessions.replacing(Some(UnsavedChoice::Cancel)),
            Replace::Stay
        );
        w.stop_quit();
        assert_eq!(w.sessions.tab(), b);
        assert!(w.creates.making_new(), "held New lost on late cancel");
        assert!(w.tabs.has_aside(), "B form overwritten by returned New");
        assert!(w.tabs.has_draft(a), "early confirmed A form destroyed");
        assert_eq!(w.tabs.count(), tab_count);
        assert!(!w.tabs.back_quit_form(&w.sessions, b_question));
        assert_eq!(
            height_of(&root.path().join("a.fcad")),
            if save { 18. } else { 12. }
        );
        assert_eq!(
            w.hidden(a).dirty(),
            !save,
            "published Save or deferred Discard changed"
        );
        finish_returned_drawing(&mut w, &before);
        let n = w.sessions.tab();
        assert_eq!(w.tabs.count(), tab_count + 1);
        w.round(b);
        assert_eq!(w.edits.typed(), Some((Some(bf), " Bx ")));
        w.round(a);
        assert_eq!(w.edits.typed(), Some((Some(af), " 26x ")));
        w.round(n);
        assert!(w.begin_quit());
        assert!(w.tabs.quit_new_question().is_none());
        assert_eq!(
            w.complete_quit(&mut Restores::default()),
            crate::QuitEnd::Exit
        );
    }
    println!("\nFCAD_30W_LATE_CANCEL_EXECUTED");
}

#[test]
fn consent_names_quit_and_new_once_and_never_skips_the_dirty_model() {
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&file);
    w.apply(18.);
    assert!(w.begin_new(false));
    w.literal_new(NewContent::SamplePlate);
    assert!(w.begin_quit());
    let old = w.tabs.quit_new_question().expect("old question");
    assert!(w.answer_quit_new(old, CloseFormChoice::Back));
    assert!(w.begin_quit());
    let current = w.tabs.quit_new_question().expect("new attempt");
    assert_ne!(old, current);
    for choice in [CloseFormChoice::Discard, CloseFormChoice::Back] {
        assert!(
            !w.answer_quit_new(old, choice),
            "stale Quit answer accepted"
        );
        assert_eq!(
            w.tabs.quit_new_question(),
            Some(current),
            "stale answer reset attempt"
        );
        w.assert_literal_new(NewContent::SamplePlate);
    }
    assert!(w.answer_quit_new(current, CloseFormChoice::Discard));
    assert!(!w.answer_quit_new(current, CloseFormChoice::Discard));
    assert!(!w.answer_quit_new(current, CloseFormChoice::Back));
    assert_eq!(
        w.tabs.quit_step(&w.sessions, false),
        QuitStep::Ask,
        "New consent skipped dirty model"
    );
    let model = w.tabs.ask_quit_model(&w.sessions).expect("dirty question");
    w.stop_quit();
    w.answer_new(NewChoice::Cancel);
    assert!(w.begin_new(false));
    w.literal_new(NewContent::Empty);
    assert!(w.begin_quit());
    let newer = w.tabs.quit_new_question().expect("new New");
    assert_ne!(newer.over, current.over);
    assert!(
        !w.answer_quit_new(current, CloseFormChoice::Discard),
        "old New answer destroyed new New"
    );
    assert!(!w.tabs.decide_quit_model(&w.sessions, model));
    assert!(w.answer_quit_new(newer, CloseFormChoice::Back));
    w.assert_literal_new(NewContent::Empty);
    println!("\nFCAD_30W_ADDRESS_EXECUTED");
}

#[test]
fn save_refusals_dialog_cancel_save_as_and_external_conflict_return_whole_new() {
    for refusal in 0..4 {
        let root = tempfile::tempdir().expect("root");
        let file = root.path().join("a.fcad");
        plate(&file, 12.);
        let mut w = Window::new(Drawn::Mock, None);
        w.open(&file);
        w.apply(18.);
        w.open_height_form();
        let af = w.feature();
        w.edits.type_height(af, " a literal ");
        assert!(w.begin_new(false));
        w.literal_new(NewContent::SamplePlate);
        w.hold_quit_new();
        w.confirm_quit();
        match refusal {
            0 => {
                assert!(!w.save_quit(SaveTarget::InPlace, true).published);
            }
            1 => {
                let target = root.path().join("occupied.fcad");
                std::fs::write(&target, b"occupied").expect("occupied");
                assert!(!w.save_quit(SaveTarget::As(target.clone()), false).published);
                assert_eq!(std::fs::read(target).expect("kept"), b"occupied");
            }
            2 => {
                let theirs = root.path().join("theirs.fcad");
                plate(&theirs, 24.);
                std::fs::rename(theirs, &file).expect("external publication");
                assert!(!w.save_quit(SaveTarget::InPlace, false).published);
                assert_eq!(height_of(&file), 24., "external writer overwritten");
            }
            _ => {
                let id = w.tabs.ask_quit_model(&w.sessions).expect("question");
                let mut dialogs = crate::dialogs::Dialogs::default();
                assert!(
                    dialogs
                        .receive(
                            crate::dialogs::Action::SaveAs,
                            crate::dialogs::Outcome::Cancelled,
                            &mut w.input,
                            None
                        )
                        .is_none()
                );
                w.stop_quit();
                assert!(!w.tabs.decide_quit_model(&w.sessions, id));
            }
        }
        w.assert_literal_new(NewContent::SamplePlate);
        assert!(w.sessions.dirty() && w.tabs.has_aside());
        w.answer_new(NewChoice::Cancel);
        assert_eq!(w.edits.typed(), Some((Some(af), " a literal ")));
    }
    // Save As of an accepted Untitled is separate from the uncreated window New.
    let root = tempfile::tempdir().expect("root");
    let mut w = Window::new(Drawn::Mock, None);
    w.create(NewDocument::Empty).expect("accepted Untitled");
    assert!(w.begin_new(false));
    w.literal_new(NewContent::Empty);
    w.hold_quit_new();
    let occupied = root.path().join("occupied.fcad");
    std::fs::write(&occupied, b"occupied").expect("occupied");
    assert!(!w.save_quit(SaveTarget::As(occupied), false).published);
    w.assert_literal_new(NewContent::Empty);
    w.hold_quit_new();
    assert!(
        w.save_quit(SaveTarget::As(root.path().join("saved.fcad")), false)
            .published
    );
    w.stop_quit();
    w.assert_literal_new(NewContent::Empty);
    assert!(!w.sessions.dirty());
    assert!(
        read_extrude_source(&root.path().join("saved.fcad"))
            .expect("saved accepted Empty")
            .features
            .is_empty()
    );
    println!("\nFCAD_30W_SAVE_REFUSALS_EXECUTED");
}

#[test]
fn scene_switch_refusals_and_first_last_tabs_error_return_all_owners_second_error_exits() {
    for refusal in 0..3 {
        let root = tempfile::tempdir().expect("root");
        let mut w = Window::new(Drawn::Mock, None);
        let (a, b, af, bf) = two_forms(&mut w, root.path());
        assert!(w.begin_new(false));
        w.literal_new(NewContent::SamplePlate);
        w.hold_quit_new();
        w.confirm_quit();
        w.decide_quit();
        let (g, rx) = w.begin_switch(b, Some(After::Quit));
        if refusal == 2 {
            w.tabs.cancel_switch();
            w.sessions.cancel();
            assert!(w.deliver_switch(g, wait(&rx), Ok(())).is_none());
        } else {
            let scene = if refusal == 0 {
                wait(&rx)
            } else {
                let _ = wait(&rx);
                Err(CadError::kernel("scene preparation refused"))
            };
            let upload = if refusal == 0 {
                Err(CadError::rendering("device refused"))
            } else {
                Ok(())
            };
            assert!(
                w.deliver_switch(g, scene, upload)
                    .expect("answer")
                    .0
                    .is_err()
            );
        }
        w.restore_after_quit();
        assert_eq!(w.sessions.tab(), a);
        w.assert_literal_new(NewContent::SamplePlate);
        w.answer_new(NewChoice::Cancel);
        assert_eq!(w.edits.typed(), Some((Some(af), " 26x ")));
        w.round(b);
        assert_eq!(w.edits.typed(), Some((Some(bf), " Bx ")));
    }
    let root = tempfile::tempdir().expect("root");
    let folder = Folder::at(&root.path().join("tabs")).expect("folder");
    folder.publish(&LastTabs::default()).expect("prior");
    std::fs::remove_file(folder.descriptor()).expect("remove own descriptor");
    std::fs::create_dir(folder.descriptor()).expect("force publication refusal");
    let mut restores = Restores::new(Ok(folder));
    let mut w = Window::new(Drawn::Mock, None);
    let (a, _, af, bf) = two_forms(&mut w, root.path());
    assert!(w.begin_new(false));
    w.literal_new(NewContent::SamplePlate);
    w.hold_quit_new();
    assert!(matches!(
        w.complete_quit(&mut restores),
        crate::QuitEnd::Stay(_)
    ));
    w.assert_literal_new(NewContent::SamplePlate);
    assert!(w.tabs.has_aside() && w.tabs.has_draft(a));
    w.answer_new(NewChoice::Cancel);
    assert_eq!(w.edits.typed(), Some((Some(bf), " Bx ")));
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(af), " 26x ")));
    assert!(w.begin_new(false));
    w.literal_new(NewContent::Empty);
    w.hold_quit_new();
    assert_eq!(
        w.complete_quit(&mut restores),
        crate::QuitEnd::Exit,
        "changed §30R second-refusal boundary"
    );
    assert!(!w.creates.has_held_new());
    println!("\nFCAD_30W_SWITCH_LASTTABS_EXECUTED");
}

#[test]
fn running_queued_and_unaccepted_create_hold_quit_and_deliver_their_original_result() {
    for made_first in [false, true] {
        let mut w = Window::new(Drawn::Mock, None);
        assert!(w.begin_new(false));
        w.literal_new(NewContent::Empty);
        let held = hold_creation(
            &mut w.creates,
            &mut w.input,
            w.private.path(),
            NewDocument::Empty,
            made_first,
        );
        held.at_barrier();
        assert!(!w.begin_quit(), "running Create was cancelled for Quit");
        assert_eq!(w.creates.accounted(), 1);
        let (g, result) = held.finish();
        assert_eq!(g, held_generation(&w.creates));
        assert!(!w.begin_quit(), "queued result lost its hold");
        let candidate = crate::creates::finish_create(&mut w.creates, &mut w.input, g, result)
            .expect("result not lost");
        assert_eq!(w.creates.status(), &CreateStatus::Made);
        assert!(!w.begin_quit(), "unaccepted candidate lost its hold");
        w.bind_candidate(candidate, Ok(()))
            .expect("same creation accepted");
        assert!(w.begin_quit());
        assert!(w.tabs.quit_new_question().is_none());
        assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Ask);
        w.stop_quit();
    }
    let mut w = Window::new(Drawn::Mock, None);
    assert!(w.begin_new(false));
    w.literal_new(NewContent::SamplePlate);
    let held = hold_creation(
        &mut w.creates,
        &mut w.input,
        w.private.path(),
        NewDocument::Empty,
        true,
    );
    held.at_barrier();
    let (g, result) = held.finish();
    let candidate =
        crate::creates::finish_create(&mut w.creates, &mut w.input, g, result).expect("candidate");
    assert!(
        w.bind_candidate(candidate, Err(CadError::rendering("completed refusal")))
            .is_err()
    );
    assert!(matches!(w.creates.status(), CreateStatus::Failed { .. }));
    w.hold_quit_new();
    w.stop_quit();
    w.assert_literal_new(NewContent::SamplePlate);
    w.create_returned_form(NewContent::SamplePlate);
    println!("\nFCAD_30W_CREATE_BARRIERS_EXECUTED");
}

fn held_generation(creates: &crate::creates::Creates) -> crate::creates::CreateGeneration {
    let CreateStatus::Running { generation } = creates.status() else {
        panic!("not Running");
    };
    *generation
}

fn new_frame(
    w: &Window,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, CloseFormChoice) {
    let mut choice = CloseFormChoice::Waiting;
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 900.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            if let Some(loss) = w
                .tabs
                .quit_new_question()
                .and_then(|id| w.creates.quit_new_loss(id))
            {
                choice = ferritecad_ui::quit_new_panel(ui, loss);
            }
        },
    );
    out.textures_delta.clear();
    (out, choice)
}

#[test]
fn real_new_quit_widgets_explain_loss_and_back_and_discard_address_the_actual_owners() {
    for drawing in [false, true] {
        let mut w = Window::new(Drawn::Mock, None);
        assert!(w.begin_new(drawing));
        let before = if drawing {
            Some(drawing_already_open(&mut w))
        } else {
            w.literal_new(NewContent::SamplePlate);
            None
        };
        assert!(w.begin_quit());
        let ctx = egui::Context::default();
        let out = new_frame(&w, &ctx, vec![]).0;
        let words = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        for word in [
            "New is not finished",
            "whole Quit",
            "every tab form",
            "Saves already published",
            if drawing {
                "sketch you drew"
            } else {
                "choices and sizes"
            },
        ] {
            assert!(words.contains(word), "missing {word}: {words}");
        }
        for label in [
            ferritecad_ui::BACK_TO_NEW,
            ferritecad_ui::DISCARD_NEW_AND_QUIT,
        ] {
            if !w.tabs.quitting() {
                assert!(w.begin_quit());
            }
            let id = w.tabs.quit_new_question().expect("question");
            let out = new_frame(&w, &ctx, vec![egui::Event::Text("999".into())]).0;
            let at = close_form::centre(&out, label);
            new_frame(&w, &ctx, vec![egui::Event::PointerMoved(at)]);
            let mut answer = CloseFormChoice::Waiting;
            for pressed in [true, false] {
                let (_, choice) = new_frame(
                    &w,
                    &ctx,
                    vec![egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    }],
                );
                if choice != CloseFormChoice::Waiting {
                    answer = choice;
                }
            }
            assert_eq!(
                answer,
                if label == ferritecad_ui::BACK_TO_NEW {
                    CloseFormChoice::Back
                } else {
                    CloseFormChoice::Discard
                }
            );
            assert!(w.answer_quit_new(id, answer));
            if answer == CloseFormChoice::Discard {
                w.stop_quit();
            }
            if let Some(before) = &before {
                assert_eq!(
                    crate::sketch::tests::vertex_draft(&w.creates.sketch).as_ref(),
                    Some(before)
                );
            } else {
                w.assert_literal_new(NewContent::SamplePlate);
            }
        }
    }
    println!("\nFCAD_30W_WIDGETS_EXECUTED");
}

fn drawing_already_open(w: &mut Window) -> (Vec<[String; 2]>, usize, usize) {
    crate::sketch::tests::fill_drawing(&mut w.creates.sketch);
    crate::sketch::tests::scribble_drawing(&mut w.creates.sketch)
}

#[test]
fn stub_real_creation_refusal_returns_idle_new_and_underlying_form() {
    if ferritecad_occt::is_available() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&file);
    w.open_height_form();
    let af = w.feature();
    w.edits.type_height(af, " a literal ");
    assert!(w.begin_new(false));
    w.literal_new(NewContent::SamplePlate);
    let (tx, rx) = mpsc::channel();
    let root_path = w.private.path().to_path_buf();
    let g = crate::start_new(
        &mut w.creates,
        &Loads::default(),
        &Exports::default(),
        &mut w.input,
        NewDocument::Empty,
        move |content, _, cancel| {
            let context = OperationContext::default().with_cancel(cancel.clone());
            std::thread::spawn(move || {
                tx.send(crate::creates::run_create(&root_path, content, &context))
                    .expect("answer");
            })
        },
    )
    .expect("start");
    let result = wait(&rx);
    assert!(result.is_err(), "true stub unexpectedly drew a scene");
    assert!(crate::creates::finish_create(&mut w.creates, &mut w.input, g, result).is_none());
    assert!(matches!(w.creates.status(), CreateStatus::Failed { .. }));
    w.hold_quit_new();
    w.stop_quit();
    w.assert_literal_new(NewContent::SamplePlate);
    w.answer_new(NewChoice::Cancel);
    assert_eq!(w.edits.typed(), Some((Some(af), " a literal ")));
    println!("\nFCAD_30W_STUB_EXECUTED");
}

#[test]
fn idle_new_transition_checks_foreground_and_gestures_before_taking_any_form() {
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&file);
    w.open_height_form();
    w.edits.type_height(w.feature(), " underneath New ");
    assert!(w.begin_new(false));
    w.literal_new(NewContent::SamplePlate);
    assert!(w.begin_quit());
    let id = w.tabs.quit_new_question().expect("question");
    w.restore_after_quit();
    w.assert_literal_new(NewContent::SamplePlate);
    assert!(
        w.tabs.has_aside() && !w.edits.form_open(),
        "late callback restored a tab form over live New"
    );
    let g = w
        .sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("operation");
    assert!(
        !w.answer_quit_new(id, CloseFormChoice::Discard),
        "form taken before operation guard"
    );
    assert_eq!(w.tabs.quit_new_question(), Some(id));
    w.assert_literal_new(NewContent::SamplePlate);
    w.sessions.finish_apply(g, Err(CadError::Cancelled));
    w.input
        .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
    assert!(
        !w.answer_quit_new(id, CloseFormChoice::Discard),
        "form taken during gesture"
    );
    w.input.forget_pending();
    assert!(w.answer_quit_new(id, CloseFormChoice::Back));
    w.assert_literal_new(NewContent::SamplePlate);
    let mut loads = Loads::default();
    Window::begin_load(&mut loads, &file);
    assert!(!crate::begin_window_quit(
        &mut w.tabs,
        &mut w.sessions,
        &w.creates,
        &loads,
        &Exports::default(),
        &w.edits,
        &w.input
    ));
    loads.stop_all();
    // §30T's pending Open decision keeps its original guard and values.
    assert!(w.creates.ask_open(root.path().join("b.fcad"), &mut w.input));
    assert!(!w.begin_quit());
    w.creates.keep_new(&mut w.input);
    w.answer_new(NewChoice::Cancel);
    assert!(w.begin_new(true));
    crate::sketch::tests::fill_drawing(&mut w.creates.sketch);
    let (ctx, at) = crate::sketch::drag_tests::hold_vertex(&mut w.creates.sketch);
    assert!(!w.begin_quit(), "new-contour gesture lost its hold");
    crate::sketch::drag_tests::release_vertex(&ctx, &mut w.creates.sketch, at);
    println!("\nFCAD_30W_GUARDS_EXECUTED");
}

#[test]
fn new_contour_over_saved_sketch_returns_both_editors_with_functional_history() {
    let root = tempfile::tempdir().expect("root");
    let a_file = root.path().join("a.fcad");
    let b_file = root.path().join("b.fcad");
    plate(&a_file, 12.);
    plate(&b_file, 15.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    let source = w.sessions.export_path().expect("A");
    let reading = read_extrude_source(&source).expect("A");
    w.creates.sketch = crate::sketch::tests::typed_apply(
        &source,
        &reading,
        reading.sketches[0].sketch,
        "80",
        "90",
    )
    .0;
    let saved = crate::sketch::tests::vertex_draft(&w.creates.sketch).expect("saved Sketch");
    w.open(&b_file);
    let b = w.sessions.tab();
    w.open_height_form();
    w.round(a);
    let new = drawing_with_history(&mut w);
    w.hold_quit_new();
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(saved.clone()),
        "shared Editor lost underlying saved Sketch"
    );
    w.confirm_quit();
    w.decide_quit();
    w.show_for_quit(b);
    let id = w.park_quit_form();
    assert!(w.tabs.back_quit_form(&w.sessions, id));
    w.restore_after_quit();
    finish_returned_drawing(&mut w, &new);
    w.round(a);
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(saved.clone())
    );
    crate::sketch::tests::press(&mut w.creates.sketch, "Undo draft");
    assert_ne!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(saved.clone())
    );
    crate::sketch::tests::press(&mut w.creates.sketch, "Redo draft");
    assert_eq!(
        crate::sketch::tests::vertex_draft(&w.creates.sketch),
        Some(saved)
    );
    println!("\nFCAD_30W_SHARED_EDITOR_EXECUTED");
}

const NEW_OUTPUTS: &[&str] = &[
    "a.fcad",
    "b.fcad",
    "a-unsaved.stl",
    "a-unsaved.fbx",
    "n-created.fcad",
    "n.fcad",
    "n-unsaved.stl",
    "n-unsaved.fbx",
    "tabs/last-window",
];

/// Window comparator: preflight every actual output before the first peer job.
fn compare_new_outputs(root: &Path) {
    use super::super::super::checkpoints::{all_sql, ids_and_refs, stl_facts};
    use crate::creates::tests::read_semantics;
    for name in NEW_OUTPUTS
        .iter()
        .copied()
        .chain(["inputs/a.fcad", "inputs/b.fcad"])
    {
        assert!(root.join(name).is_file(), "missing real GUI output: {name}");
    }
    let origin = root.join("n-created.fcad");
    let saved = root.join("n.fcad");
    assert_ne!(
        document_id(&origin),
        document_id(&root.join("a.fcad")),
        "New copied A"
    );
    assert_ne!(
        document_id(&origin),
        document_id(&root.join("b.fcad")),
        "New copied B"
    );
    close_form::compare_close(root);
    let peer_root = tempfile::tempdir().expect("peer");
    let request = peer_root.path().join("request.json");
    std::fs::write(
        &request,
        r#"{"request_version":1,"points_mm":[[0,0],[20,0],[20,10]],"height_mm":10}"#,
    )
    .expect("request");
    let independently_created = peer_root.path().join("created.fcad");
    cli(&[
        "create-sketch-extrude".as_ref(),
        request.as_os_str(),
        "-o".as_ref(),
        independently_created.as_os_str(),
    ]);
    assert_eq!(
        read_semantics(&origin).0,
        read_semantics(&independently_created).0,
        "New creation differs from CLI route"
    );
    assert_ne!(
        read_semantics(&origin).1,
        read_semantics(&independently_created).1,
        "independent creations reused UUIDs"
    );
    let reading = read_extrude_source(&origin).expect("origin");
    let peer = peer_root.path().join("edited.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        origin.as_os_str(),
        "--feature".as_ref(),
        reading.features[0].feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        reading.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "7".as_ref(),
        "-o".as_ref(),
        peer.as_os_str(),
    ]);
    assert_eq!(
        all_sql(&saved, &[], false),
        all_sql(&peer, &[], false),
        "New all SQL/source except write stamp"
    );
    assert_eq!(
        ids_and_refs(&saved),
        ids_and_refs(&origin),
        "New UUID/reference changed after creation"
    );
    assert_eq!(document_id(&saved), document_id(&origin));
    let expected = peer_bytes(&peer, peer_root.path(), "new-peer");
    assert_eq!(
        std::fs::read(root.join("n-unsaved.stl")).expect("STL"),
        expected.0,
        "New unsaved STL differs from CLI"
    );
    assert_eq!(
        std::fs::read(root.join("n-unsaved.fbx")).expect("FBX"),
        expected.1,
        "New unsaved FBX differs from CLI"
    );
    assert_eq!(
        peer_bytes(&saved, peer_root.path(), "new-saved"),
        expected,
        "Save changed New exports"
    );
    let (_, height, volume) = stl_facts(&expected.0);
    assert!(
        (height - 7.).abs() < 1e-4 && (volume - 700.).abs() < 1e-2,
        "New cold geometry {height} {volume}"
    );
    let listed = Folder::at(&root.join("tabs"))
        .expect("folder")
        .read()
        .expect("list")
        .expect("published");
    assert_eq!(
        listed.paths,
        [root.join("a.fcad"), root.join("b.fcad"), saved],
        "final LastTabs"
    );
    let records = RecoveryStore::open(&root.join("recovery"))
        .expect("store")
        .list()
        .expect("records");
    assert_eq!(records.active, 0);
    assert_eq!(records.recoverable().count(), 0);
}

/// The same comparator refuses broken copies of the actual owner/window files.
fn compare_new_with_controls(root: &Path) {
    compare_new_outputs(root);
    let control = |name: &str, why: &str, breaks: &dyn Fn(&Path)| {
        super::super::control_of(compare_new_outputs, root, name, why, breaks);
    };
    let before = cli_runs();
    control(
        "missing New export",
        "missing real GUI output: n-unsaved.fbx",
        &|r| {
            std::fs::remove_file(r.join("n-unsaved.fbx")).expect("remove");
        },
    );
    assert_eq!(cli_runs(), before, "peer ran before output preflight");
    control(
        "wrong New SQL",
        "New all SQL/source except write stamp",
        &|r| {
            rusqlite::Connection::open(r.join("n.fcad"))
                .expect("SQL")
                .execute(
                    "UPDATE objects SET name='altered' WHERE id=(SELECT id FROM objects LIMIT 1)",
                    [],
                )
                .expect("alter SQL");
        },
    );
    control(
        "New export replaced",
        "New unsaved FBX differs from CLI",
        &|r| {
            std::fs::copy(r.join("a-unsaved.fbx"), r.join("n-unsaved.fbx")).expect("replace");
        },
    );
    control("changed B", "B was written", &|r| {
        std::fs::copy(r.join("a.fcad"), r.join("b.fcad")).expect("replace");
    });
    control("wrong LastTabs", "final LastTabs", &|r| {
        Folder::at(&r.join("tabs"))
            .expect("folder")
            .publish(&LastTabs::default())
            .expect("publish");
    });
}

fn native_new_recipe(root: &Path) {
    make_inputs(root);
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let folder = Folder::at(&root.join("tabs")).expect("folder");
    let mut restores = Restores::new(Ok(folder));
    let mut w = Window::new(Drawn::Native, Some(&store));
    w.open(&root.join("a.fcad"));
    let a = w.sessions.tab();
    w.apply(18.);
    w.open_height_form();
    let af = w.feature();
    w.edits.type_height(af, " 26x ");
    w.open(&root.join("b.fcad"));
    let b = w.sessions.tab();
    w.apply(22.);
    let source = w.sessions.export_path().expect("B");
    let reading = read_extrude_source(&source).expect("B");
    w.creates.sketch = crate::sketch::tests::analytic_apply::typed_circle(
        &source,
        &reading,
        reading.circle_sketches[0].sketch,
        ["-3.5", "4.25"],
        "8",
    )
    .0;
    let b_form = format!("{:?}", w.creates.sketch);
    w.round(a);
    let before = drawing_with_history(&mut w);
    w.hold_quit_new();
    w.confirm_quit();
    assert!(w.save_quit(SaveTarget::InPlace, false).published);
    assert_eq!(height_of(&root.join("a.fcad")), 18.);
    w.show_for_quit(b);
    w.confirm_quit();
    w.tabs
        .ask_quit_model(&w.sessions)
        .expect("late B model question");
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Cancel)),
        Replace::Stay
    );
    w.stop_quit();
    assert!(w.tabs.has_aside() && w.tabs.has_draft(a));
    assert!(w.begin_quit());
    let again = w.tabs.quit_new_question().expect("repeat New question");
    assert!(w.answer_quit_new(again, CloseFormChoice::Back));
    finish_returned_drawing(&mut w, &before);
    let n = w.sessions.tab();
    assert!(
        w.save(SaveTarget::As(root.join("n-created.fcad")))
            .published
    );
    let new_ids = super::super::super::checkpoints::ids_and_refs(&root.join("n-created.fcad"));
    w.open_height_form();
    let nf = w.feature();
    w.edits.type_height(nf, "7");
    assert!(w.apply_height_form());
    export_bytes(
        &w.sessions.export_path().expect("New"),
        &w.sessions.suggestion().expect("alias"),
        root,
        "n-unsaved",
    );
    assert!(w.save(SaveTarget::As(root.join("n.fcad"))).published);
    assert_eq!(
        super::super::super::checkpoints::ids_and_refs(&root.join("n.fcad")),
        new_ids
    );
    w.round(b);
    assert_eq!(
        format!("{:?}", w.creates.sketch),
        b_form,
        "B form overwritten by New"
    );
    // Undo/Redo restored B's own draft, independently of the New history.
    crate::sketch::tests::press(&mut w.creates.sketch, "Undo draft");
    crate::sketch::tests::press(&mut w.creates.sketch, "Redo draft");
    assert_eq!(format!("{:?}", w.creates.sketch), b_form);
    w.round(a);
    assert_eq!(
        w.edits.typed(),
        Some((Some(af), " 26x ")),
        "underlying A form lost"
    );
    assert!(!w.sessions.dirty());
    w.edits.type_height(af, "26");
    assert!(w.apply_height_form());
    export_bytes(
        &w.sessions.export_path().expect("A"),
        &w.sessions.suggestion().expect("alias"),
        root,
        "a-unsaved",
    );
    w.open_height_form();
    w.edits.type_height(af, " 99x ");
    let private = w.private.path().to_path_buf();
    assert!(w.begin_quit());
    assert!(w.tabs.quit_new_question().is_none());
    w.confirm_quit();
    assert!(w.save_quit(SaveTarget::InPlace, false).published);
    assert_eq!(w.complete_quit(&mut restores), crate::QuitEnd::Exit);
    assert_eq!(w.tabs.count(), 3);
    assert!(!w.creates.has_held_new());
    assert!(w.tabs.order().contains(&n));
    w.creates.stop_all();
    w.sessions.stop_all();
    w.tabs.stop_all();
    assert_eq!(
        std::fs::read_dir(private).expect("private").count(),
        0,
        "final private cleanup"
    );
}

#[test]
fn native_returned_new_creates_then_publishes_and_matches_cli_sql_uuid_cold_geometry_exports() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    native_new_recipe(root.path());
    compare_new_with_controls(root.path());
    if let Some(artifacts) = std::env::var_os("FCAD_QUIT_NEW_ARTIFACTS") {
        let dest = PathBuf::from(artifacts);
        std::fs::create_dir_all(&dest).expect("artifacts");
        for name in ["n-unsaved.stl", "n-unsaved.fbx"] {
            std::fs::copy(root.path().join(name), dest.join(name)).expect("artifact");
        }
    }
    println!("\nFCAD_30W_NATIVE_EXECUTED volume_n=700.000 all_SQL_cells=true");
}

#[test]
fn mixed_returned_new_creates_with_occt_without_solver_and_keeps_both_forms() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Native, None);
    w.open(&file);
    let a = w.sessions.tab();
    w.open_height_form();
    let af = w.feature();
    w.edits.type_height(af, " a literal ");
    assert!(w.begin_new(false));
    w.literal_new(NewContent::SamplePlate);
    w.hold_quit_new();
    w.confirm_quit();
    w.stop_quit();
    w.create_returned_form(NewContent::SamplePlate);
    assert!(!w.scene.catalogue.is_empty(), "OCCT picture never executed");
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(af), " a literal ")));
    println!("\nFCAD_30W_MIXED_EXECUTED");
}

#[test]
fn native_compare_real_quit_new_gui_outputs() {
    let Some(root) = std::env::var_os("FCAD_30W_GUI_DIR") else {
        return;
    };
    assert!(native(), "GUI comparator requires native kernel");
    compare_new_with_controls(Path::new(&root));
    println!("\nFCAD_30W_GUI_COMPARE_OK all_SQL_cells=true");
}
