// SPDX-License-Identifier: MIT
//! §30V: real window owners and egui widgets, without an OS window.
use super::*;
use crate::last_tabs::{Folder, LastTabs};
use crate::restores::Restores;
use crate::tabs::{Forms, QuitId};

impl Window {
    fn park_quit_form(&mut self) -> QuitId {
        self.tabs
            .ask_quit_form(
                &self.sessions,
                &mut Forms {
                    edits: &mut self.edits,
                    editor: &mut self.creates.sketch,
                },
            )
            .expect("Quit form")
    }
    fn confirm_quit(&mut self) -> QuitId {
        let id = self.park_quit_form();
        assert!(self.tabs.confirm_quit_form(&self.sessions, id));
        id
    }
    fn restore_after_quit(&mut self) {
        self.tabs.bring_back(
            &mut self.sessions,
            &mut Forms {
                edits: &mut self.edits,
                editor: &mut self.creates.sketch,
            },
        );
    }
    fn stop_quit(&mut self) {
        self.tabs.abort_quit();
        self.restore_after_quit();
    }
    fn save_quit(&mut self, target: SaveTarget, cancelled: bool) -> SaveReport {
        let id = self
            .tabs
            .ask_quit_model(&self.sessions)
            .expect("model question");
        let (tx, rx) = mpsc::channel();
        let address = self
            .sessions
            .begin_save(target, Some(Continuation::Quit(id)), |plan, _, cancel| {
                if cancelled {
                    cancel.cancel();
                }
                spawn_save(plan, cancel.clone(), move |answer| {
                    tx.send(answer).expect("save")
                })
            })
            .expect("save");
        assert_eq!(self.tabs.quit_step(&self.sessions, false), QuitStep::Ask);
        let report = self
            .sessions
            .finish_save(address, wait(&rx))
            .expect("answer");
        assert!(
            self.sessions
                .finish_save(
                    address,
                    Err(ferritecad_jobs::SaveFailure {
                        kind: ferritecad_jobs::SaveFailureKind::Cancelled,
                        error: CadError::Cancelled,
                    })
                )
                .is_none(),
            "duplicate worker answer continued"
        );
        if let Some(Continuation::Quit(continued)) = report.continuation {
            assert_eq!(continued, id);
            assert!(!self.sessions.dirty());
            assert!(self.tabs.decide_quit_model(&self.sessions, continued));
        } else {
            self.stop_quit();
        }
        report
    }
    fn complete_quit(&mut self, restores: &mut Restores) -> crate::QuitEnd {
        loop {
            match self.tabs.quit_step(
                &self.sessions,
                crate::form_open(&self.edits, &self.creates.sketch),
            ) {
                QuitStep::Form => {
                    self.confirm_quit();
                }
                QuitStep::Ask => self.decide_quit(),
                QuitStep::Show(tab) => self.show_for_quit(tab),
                QuitStep::Exit => {
                    let end = self.end_quit(restores);
                    self.restore_after_quit();
                    return end;
                }
            }
        }
    }
}

#[test]
fn late_cancel_returns_all_forms_and_published_save_then_final_quit_cleans_up() {
    let root = tempfile::tempdir().expect("root");
    let a_file = root.path().join("a.fcad");
    let b_file = root.path().join("b.fcad");
    plate(&a_file, 12.);
    std::fs::copy(&a_file, &b_file).expect("same DocumentId");
    let store = RecoveryStore::open(&root.path().join("recovery")).expect("recovery");
    let folder = Folder::at(&root.path().join("tabs")).expect("tabs");
    let old = LastTabs {
        paths: vec![b_file.clone()],
        active: Some(0),
    };
    folder.publish(&old).expect("previous list");
    let mut restores = Restores::new(Ok(folder.clone()));
    let mut w = Window::new(Drawn::Mock, Some(&store));
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(18.);
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " 26x ");
    let a_base = w.sessions.export_source().expect("A base");
    let a_files = private_count(&w.sessions);
    w.open(&b_file);
    let b = w.sessions.tab();
    w.constraints_draft(1, 3);
    crate::constraints::tests::session_apply::press(&mut w.creates.sketch.constraints, "Undo");
    let b_form = format!("{:?}", w.creates.sketch);
    let b_base = w.sessions.export_source().expect("B base");
    w.create(ferritecad_jobs::NewDocument::Empty)
        .expect("C Untitled");
    let c = w.sessions.tab();
    w.round(a);
    assert!(w.begin_quit());
    let first = w.confirm_quit();
    assert!(
        !w.tabs.confirm_quit_form(&w.sessions, first),
        "double confirmation"
    );
    assert!(!w.begin_quit(), "repeat Quit reset a pending attempt");
    assert!(w.save_quit(SaveTarget::InPlace, false).published);
    assert_eq!(height_of(&a_file), 18., "Save applied literal form");
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Show(b));
    w.show_for_quit(b);
    w.confirm_quit();
    w.decide_quit();
    assert_eq!(w.tabs.quit_step(&w.sessions, false), QuitStep::Show(c));
    w.show_for_quit(c);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Cancel)),
        Replace::Stay
    );
    w.stop_quit();
    assert_eq!(w.tabs.order(), [a, b, c]);
    assert!(w.sessions.untitled() && w.sessions.dirty());
    assert_eq!(folder.read().expect("list"), Some(old));
    w.round(a);
    assert_eq!(
        w.edits.typed(),
        Some((Some(f), " 26x ")),
        "early confirmed A form destroyed"
    );
    assert!(!w.sessions.dirty());
    assert!(Arc::ptr_eq(
        &a_base,
        &w.sessions.export_source().expect("A")
    ));
    assert_eq!(private_count(&w.sessions), a_files, "cancel made history");
    w.round(b);
    assert_eq!(
        format!("{:?}", w.creates.sketch),
        b_form,
        "B history/UUID picks lost"
    );
    assert!(Arc::ptr_eq(
        &b_base,
        &w.sessions.export_source().expect("B")
    ));
    crate::constraints::tests::session_apply::press(&mut w.creates.sketch.constraints, "Redo");
    assert!(w.creates.sketch.constraints.take_apply_request().is_none());
    crate::constraints::tests::session_apply::press(&mut w.creates.sketch.constraints, "Undo");
    assert_eq!(format!("{:?}", w.creates.sketch), b_form);
    w.round(a);
    let private = w
        .sessions
        .private_directory()
        .expect("private")
        .to_path_buf();
    drop(a_base);
    drop(b_base);
    assert!(w.begin_quit());
    assert_eq!(w.complete_quit(&mut restores), crate::QuitEnd::Exit);
    assert_eq!(
        folder.read().expect("list").expect("saved").paths,
        [a_file, b_file]
    );
    w.sessions.stop_all();
    w.tabs.stop_all();
    assert!(!private.exists());
    assert_eq!(store.list().expect("recovery").recoverable().count(), 0);
    println!("\nFCAD_30V_LATE_CANCEL_EXECUTED all_forms=true saved_a=18");
}

#[test]
fn deferred_discard_and_refused_saves_keep_forms_and_models() {
    let root = tempfile::tempdir().expect("root");
    let a_file = root.path().join("a.fcad");
    let b_file = root.path().join("b.fcad");
    plate(&a_file, 12.);
    plate(&b_file, 15.);
    let original = std::fs::read(&a_file).expect("A");
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(18.);
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " 26x ");
    w.open(&b_file);
    let b = w.sessions.tab();
    w.apply(22.);
    w.round(a);
    assert!(w.begin_quit());
    w.confirm_quit();
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    w.decide_quit();
    w.show_for_quit(b);
    w.stop_quit();
    w.round(a);
    assert!(w.sessions.dirty());
    assert_eq!(height_in(&w.sessions), 18.);
    assert_eq!(std::fs::read(&a_file).expect("A"), original);
    assert_eq!(w.edits.typed(), Some((Some(f), " 26x ")));
    for (target, cancelled) in [
        (SaveTarget::As(b_file.clone()), false),
        (
            SaveTarget::As(root.path().join("absent/output.fcad")),
            false,
        ),
        (SaveTarget::As(root.path().join("cancelled.fcad")), true),
    ] {
        assert!(w.begin_quit());
        w.confirm_quit();
        assert!(!w.save_quit(target, cancelled).published);
        assert_eq!(w.edits.typed(), Some((Some(f), " 26x ")));
        assert_eq!(height_in(&w.sessions), 18.);
        assert!(w.sessions.dirty());
    }
    assert!(w.tabs.save_as_refusal(&w.sessions, &b_file).is_some());
    assert!(w.begin_quit());
    w.confirm_quit();
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
    assert_eq!(w.edits.typed(), Some((Some(f), " 26x ")));
    let external = root.path().join("external.fcad");
    plate(&external, 31.);
    std::fs::copy(&external, &a_file).expect("external change");
    assert!(w.begin_quit());
    w.confirm_quit();
    assert!(!w.save_quit(SaveTarget::InPlace, false).published);
    assert_eq!(height_of(&a_file), 31.);
    assert_eq!(height_in(&w.sessions), 18.);
    assert_eq!(w.edits.typed(), Some((Some(f), " 26x ")));
    // Untitled accepted plate has a saved-object form too; first Save writes only 18.
    w.edits.cancel();
    w.create(ferritecad_jobs::NewDocument::SamplePlate(PlateSize {
        width: 80.,
        depth: 40.,
        height: 18.,
    }))
    .expect("Untitled");
    w.open_height_form();
    let u = w.feature();
    w.edits.type_height(u, " 99x ");
    assert!(w.begin_quit());
    w.confirm_quit();
    w.tabs
        .ask_quit_model(&w.sessions)
        .expect("Untitled Save As question");
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
    assert!(w.sessions.untitled() && w.sessions.dirty());
    assert_eq!(w.edits.typed(), Some((Some(u), " 99x ")));
    assert!(w.begin_quit());
    w.confirm_quit();
    assert!(!w.save_quit(SaveTarget::As(b_file), false).published);
    assert!(w.sessions.untitled());
    assert_eq!(w.edits.typed(), Some((Some(u), " 99x ")));
    assert!(w.begin_quit());
    w.confirm_quit();
    let file = root.path().join("untitled.fcad");
    assert!(w.save_quit(SaveTarget::As(file.clone()), false).published);
    w.stop_quit();
    assert_eq!(height_of(&file), 18.);
    assert_eq!(w.edits.typed(), Some((Some(u), " 99x ")));
    assert!(!w.sessions.dirty());
    println!("\nFCAD_30V_SAVE_REFUSALS_EXECUTED");
}

#[test]
fn form_and_model_answers_address_attempt_tab_and_snapshot_once() {
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&file);
    w.apply(18.);
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " - x ");
    assert!(w.begin_quit());
    let old = w.park_quit_form();
    assert!(
        !w.tabs.decide_quit_model(&w.sessions, old),
        "form answer bypassed model question"
    );
    assert!(
        w.tabs.ask_quit_model(&w.sessions).is_none(),
        "model bypassed form"
    );
    assert!(w.tabs.back_quit_form(&w.sessions, old));
    w.restore_after_quit();
    assert!(w.begin_quit());
    let new = w.park_quit_form();
    assert_ne!(new, old);
    assert!(!w.tabs.confirm_quit_form(&w.sessions, old));
    assert!(!w.tabs.back_quit_form(&w.sessions, old));
    let mut foreign = Sessions::default();
    foreign.adopt(
        DocumentSession::open_in(w.private.path(), &file, HistoryLimits::default())
            .expect("foreign same DocumentId"),
    );
    assert!(!w.tabs.confirm_quit_form(&foreign, new));
    assert!(w.tabs.confirm_quit_form(&w.sessions, new));
    assert!(!w.tabs.confirm_quit_form(&w.sessions, new));
    assert!(
        !w.tabs.decide_quit_model(&w.sessions, new),
        "unasked model decision advanced Quit"
    );
    let model = w.tabs.ask_quit_model(&w.sessions).expect("model");
    assert!(!w.tabs.decide_quit_model(&foreign, model));
    assert!(
        w.close(w.sessions.tab()).is_err(),
        "Close bypassed pending Quit"
    );
    assert!(
        w.tabs
            .begin_switch(&mut w.sessions, TabId::default(), None, |_, _, _| panic!(
                "worker"
            ))
            .is_err()
    );
    w.sessions.adopt(
        DocumentSession::open_in(w.private.path(), &file, HistoryLimits::default())
            .expect("new accepted snapshot"),
    );
    assert!(
        !w.tabs.decide_quit_model(&w.sessions, model),
        "new snapshot used old authority"
    );
    w.stop_quit();
    assert_eq!(w.edits.typed(), Some((Some(f), " - x ")));
    assert!(
        !w.sessions.takes_form_apply(),
        "stale original draft was rebuilt"
    );
    w.edits.cancel();
    w.sessions.release_stale_draft();
    assert!(w.begin_quit());
    let id = w.tabs.ask_quit_model(&w.sessions).expect("new model");
    assert!(
        !w.tabs.decide_quit_model(&w.sessions, model),
        "late save continued new pass"
    );
    assert!(w.tabs.decide_quit_model(&w.sessions, id));
    assert!(
        !w.tabs.decide_quit_model(&w.sessions, id),
        "double model answer"
    );
    w.stop_quit();
    println!("\nFCAD_30V_ADDRESS_EXECUTED");
}

#[test]
fn switch_and_first_last_tabs_refusal_return_previously_confirmed_forms() {
    let root = tempfile::tempdir().expect("root");
    let a_file = root.path().join("a.fcad");
    let b_file = root.path().join("b.fcad");
    plate(&a_file, 12.);
    plate(&b_file, 15.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " a x ");
    w.open(&b_file);
    let b = w.sessions.tab();
    w.open_height_form();
    let bf = w.feature();
    w.edits.type_height(bf, " b x ");
    w.round(a);
    assert!(w.begin_quit());
    w.confirm_quit();
    w.decide_quit();
    let (g, rx) = w.begin_switch(b, Some(After::Quit));
    let (outcome, after) = w
        .deliver_switch(g, wait(&rx), Err(CadError::rendering("fault seam")))
        .expect("answer");
    assert!(outcome.is_err() && after.is_none());
    w.restore_after_quit();
    assert!(!w.tabs.quitting());
    assert_eq!(w.edits.typed(), Some((Some(f), " a x ")));
    // A cancelled switch and its late answer also return the early form.
    assert!(w.begin_quit());
    w.confirm_quit();
    w.decide_quit();
    let (g, rx) = w.begin_switch(b, Some(After::Quit));
    w.tabs.cancel_switch();
    w.sessions.cancel();
    assert!(w.deliver_switch(g, wait(&rx), Ok(())).is_none());
    w.restore_after_quit();
    assert_eq!(w.edits.typed(), Some((Some(f), " a x ")));
    let folder = Folder::at(&root.path().join("tabs")).expect("folder");
    folder.publish(&LastTabs::default()).expect("prior");
    std::fs::remove_file(folder.descriptor()).expect("remove test descriptor");
    std::fs::create_dir(folder.descriptor()).expect("block list publication");
    let mut restores = Restores::new(Ok(folder));
    assert!(w.begin_quit());
    assert!(matches!(
        w.complete_quit(&mut restores),
        crate::QuitEnd::Stay(_)
    ));
    assert_eq!(w.tabs.count(), 2);
    assert_eq!(w.edits.typed(), Some((Some(bf), " b x ")));
    w.round(a);
    assert_eq!(
        w.edits.typed(),
        Some((Some(f), " a x ")),
        "first LastTabs refusal destroyed early form"
    );
    assert!(w.begin_quit());
    assert_eq!(w.complete_quit(&mut restores), crate::QuitEnd::Exit);
    println!("\nFCAD_30V_SWITCH_LASTTABS_EXECUTED");
}

#[test]
fn real_quit_widgets_name_loss_back_and_repeat_preserve_literal_form() {
    use close_form::{centre, click_form, form_frame};
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&file);
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " 2..6 ");
    let ctx = egui::Context::default();
    assert!(w.begin_quit());
    let id = w.park_quit_form();
    let out = form_frame(&mut w, &ctx, vec![]).0;
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
        "a.fcad",
        "not yet applied",
        "whole Quit",
        "every form",
        "Saves already published",
    ] {
        assert!(words.contains(word), "missing {word}: {words}");
    }
    let back = centre(&out, ferritecad_ui::BACK_TO_FORM);
    assert_eq!(
        click_form(&mut w, &ctx, back),
        ferritecad_ui::CloseFormChoice::Back
    );
    assert!(w.tabs.back_quit_form(&w.sessions, id));
    w.restore_after_quit();
    assert_eq!(w.edits.typed(), Some((Some(f), " 2..6 ")));
    assert!(w.begin_quit());
    let next = w.park_quit_form();
    assert!(!w.tabs.back_quit_form(&w.sessions, id));
    let out = form_frame(&mut w, &ctx, vec![]).0;
    let discard = centre(&out, ferritecad_ui::DISCARD_FORM_AND_QUIT);
    assert_eq!(
        click_form(&mut w, &ctx, discard),
        ferritecad_ui::CloseFormChoice::Discard
    );
    assert!(w.tabs.confirm_quit_form(&w.sessions, next));
    assert_eq!(
        click_form(&mut w, &ctx, discard),
        ferritecad_ui::CloseFormChoice::Waiting
    );
    form_frame(&mut w, &ctx, vec![egui::Event::Text("999".into())]);
    w.stop_quit();
    assert_eq!(w.edits.typed(), Some((Some(f), " 2..6 ")));
    println!("\nFCAD_30V_WIDGETS_EXECUTED");
}

#[test]
fn quit_keeps_foreground_new_export_and_gesture_holds() {
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&file);
    w.open_height_form();
    assert!(w.begin_quit());
    w.stop_quit();
    w.input
        .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
    assert!(!w.begin_quit());
    w.input.forget_pending();
    crate::sketch::tests::begin_drawing(&mut w.creates.sketch);
    assert!(!w.begin_quit());
    w.creates.sketch.dismiss();
    let g = w
        .sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("Apply");
    assert!(!w.begin_quit());
    w.sessions.finish_apply(g, Err(CadError::Cancelled));
    let g = w
        .sessions
        .begin_save(
            SaveTarget::As(root.path().join("unused.fcad")),
            None,
            |_, _, _| std::thread::spawn(|| {}),
        )
        .expect("Save");
    assert!(!w.begin_quit());
    w.sessions.finish_save(
        g,
        Err(ferritecad_jobs::SaveFailure {
            kind: ferritecad_jobs::SaveFailureKind::Cancelled,
            error: CadError::Cancelled,
        }),
    );
    let mut loads = Loads::default();
    loads.open(
        Some(&file),
        Arc::new(crate::ProgressRelay::default()),
        |_, _| std::thread::spawn(|| {}),
    );
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
    let mut exports = Exports::default();
    let destination = root.path().join("out.fbx");
    std::fs::write(&destination, b"occupied").expect("file");
    crate::exports::begin_export(
        &mut exports,
        &mut w.input,
        Some(&file),
        Some(destination),
        |_, _, _, _| panic!("unconfirmed export"),
    );
    assert!(!crate::begin_window_quit(
        &mut w.tabs,
        &mut w.sessions,
        &w.creates,
        &Loads::default(),
        &exports,
        &w.edits,
        &w.input
    ));
    assert!(crate::creates::open_form(&mut w.creates, &mut w.input));
    assert!(!w.begin_quit());
    w.creates = crate::creates::Creates::default();
    let source = w.sessions.export_path().expect("source");
    let reading = read_extrude_source(&source).expect("reading");
    w.edits.type_height(w.feature(), "20");
    let copy = w
        .edits
        .request(root.path().join("copy.fcad"))
        .expect("copy");
    w.edits.start(copy, |_, _, _| std::thread::spawn(|| {}));
    assert!(!w.begin_quit());
    w.edits.stop_all();
    assert!(
        w.creates
            .sketch
            .begin_edit(&source, &reading, reading.sketches[0].sketch)
    );
    let (ctx, at) = crate::sketch::drag_tests::hold_vertex(&mut w.creates.sketch);
    assert!(!w.begin_quit());
    crate::sketch::drag_tests::release_vertex(&ctx, &mut w.creates.sketch, at);
    println!("\nFCAD_30V_HOLDS_EXECUTED");
}

#[test]
fn stub_quit_switch_refusal_keeps_both_original_forms() {
    if ferritecad_occt::is_available() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let a_file = root.path().join("a.fcad");
    let b_file = root.path().join("b.fcad");
    plate(&a_file, 12.);
    plate(&b_file, 15.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " a ");
    w.open(&b_file);
    let b = w.sessions.tab();
    w.open_height_form();
    let bf = w.feature();
    w.edits.type_height(bf, " b ");
    w.round(a);
    assert!(w.begin_quit());
    w.confirm_quit();
    w.decide_quit();
    w.drawn = Drawn::Native;
    let (g, rx) = w.begin_switch(b, Some(After::Quit));
    let (outcome, after) = w.deliver_switch(g, wait(&rx), Ok(())).expect("answer");
    assert!(outcome.is_err() && after.is_none());
    w.restore_after_quit();
    assert_eq!(w.edits.typed(), Some((Some(f), " a ")));
    w.drawn = Drawn::Mock;
    w.round(b);
    assert_eq!(w.edits.typed(), Some((Some(bf), " b ")));
    println!("\nFCAD_30V_STUB_EXECUTED");
}

#[test]
fn native_cancelled_quit_returns_form_for_apply_export_save_and_final_quit() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    make_inputs(root.path());
    let store = RecoveryStore::open(&root.path().join("recovery")).expect("store");
    let folder = Folder::at(&root.path().join("tabs")).expect("folder");
    let mut restores = Restores::new(Ok(folder.clone()));
    let mut w = Window::new(Drawn::Native, Some(&store));
    w.open(&root.path().join("a.fcad"));
    let a = w.sessions.tab();
    w.apply(18.);
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " 26x ");
    w.open(&root.path().join("b.fcad"));
    let b = w.sessions.tab();
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
    w.create(ferritecad_jobs::NewDocument::Empty).expect("C");
    let c = w.sessions.tab();
    w.round(a);
    assert!(w.begin_quit());
    w.confirm_quit();
    assert!(w.save_quit(SaveTarget::InPlace, false).published);
    w.show_for_quit(b);
    w.confirm_quit();
    w.decide_quit();
    w.show_for_quit(c);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Cancel)),
        Replace::Stay
    );
    w.stop_quit();
    assert_eq!(w.tabs.count(), 3);
    w.round(b);
    assert_eq!(format!("{:?}", w.creates.sketch), b_form);
    w.round(a);
    assert_eq!(w.edits.typed(), Some((Some(f), " 26x ")));
    assert!(!w.sessions.dirty());
    w.edits.type_height(f, "26");
    assert!(w.apply_height_form());
    let alias = w.sessions.suggestion().expect("alias");
    export_bytes(
        &w.sessions.export_path().expect("A"),
        &alias,
        root.path(),
        "a-unsaved",
    );
    w.open_height_form();
    w.edits.type_height(f, " 99x ");
    let private = w
        .sessions
        .private_directory()
        .expect("private")
        .to_path_buf();
    assert!(w.begin_quit());
    w.confirm_quit();
    assert!(w.save_quit(SaveTarget::InPlace, false).published);
    assert_eq!(w.complete_quit(&mut restores), crate::QuitEnd::Exit);
    w.sessions.stop_all();
    w.tabs.stop_all();
    assert!(!private.exists());
    close_form::compare_close(root.path());
    let listed = folder.read().expect("list").expect("published");
    assert_eq!(
        listed.paths,
        [root.path().join("a.fcad"), root.path().join("b.fcad")]
    );
    assert_eq!(listed.active, None, "Untitled C is omitted");
    assert_eq!(record_dirs(&root.path().join("recovery")), 0);
    if let Ok(dir) = std::env::var("FCAD_QUIT_DRAFT_ARTIFACTS") {
        for ext in ["stl", "fbx"] {
            std::fs::copy(
                root.path().join(format!("a-unsaved.{ext}")),
                Path::new(&dir).join(format!("quit-draft-a.{ext}")),
            )
            .expect("artifact");
        }
    }
    println!("\nFCAD_30V_NATIVE_EXECUTED volume_a=83200.000 all_SQL_cells=true");
}

#[test]
fn mixed_quit_with_occt_without_solver_returns_form_on_cancel() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Native, None);
    w.open(&file);
    w.apply(18.);
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " 26x ");
    assert!(w.begin_quit());
    w.confirm_quit();
    w.tabs.ask_quit_model(&w.sessions).expect("model");
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Cancel)),
        Replace::Stay
    );
    w.stop_quit();
    assert_eq!(w.edits.typed(), Some((Some(f), " 26x ")));
    assert_eq!(height_in(&w.sessions), 18.);
    println!("\nFCAD_30V_MIXED_EXECUTED");
}

#[test]
fn native_compare_real_quit_draft_gui_outputs() {
    let Ok(dir) = std::env::var("FCAD_30V_GUI_DIR") else {
        return;
    };
    assert!(native(), "GUI comparator requires native geometry");
    let root = Path::new(&dir);
    close_form::compare_close(root);
    let folder = Folder::at(&root.join("tabs")).expect("tabs");
    assert_eq!(
        folder
            .read()
            .expect("list")
            .expect("successful Quit list")
            .paths,
        [root.join("a.fcad"), root.join("b.fcad")]
    );
    assert_eq!(
        record_dirs(&root.join("recovery")),
        0,
        "successful Quit left recovery leases"
    );
    println!("\nFCAD_30V_GUI_COMPARE_OK all_SQL_cells=true");
}

#[test]
fn every_saved_form_family_returns_whole_after_confirmed_quit_is_aborted() {
    close_form::form_families(true);
    println!("\nFCAD_30V_FORM_FAMILIES_EXECUTED families=12");
}
