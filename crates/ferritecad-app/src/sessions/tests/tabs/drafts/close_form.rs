// SPDX-License-Identifier: MIT
//! §30U: the same tab/session/form owners as the window, without a window.
use super::*;
use crate::tabs::{FormCloseId, Forms};

impl Window {
    fn ask_form_close(&mut self) -> FormCloseId {
        assert!(self.can_leave());
        crate::ask_form_close(
            &mut self.tabs,
            &self.sessions,
            &mut self.creates,
            &mut self.edits,
            &Loads::default(),
            &Exports::default(),
            &self.input,
        )
        .expect("shown form")
    }
    fn back_from_form_close(&mut self, id: FormCloseId) -> bool {
        self.tabs.cancel_form_close(
            &mut self.sessions,
            id,
            &mut Forms {
                edits: &mut self.edits,
                editor: &mut self.creates.sketch,
            },
        )
    }
    fn confirm_form_close(&mut self, id: FormCloseId) {
        assert!(self.tabs.confirm_form_close(&self.sessions, id));
    }
    fn decide_form_close(&mut self, id: FormCloseId) {
        assert!(self.tabs.decide_form_close(&self.sessions, id));
    }
    fn save_for_form_close(&mut self, id: FormCloseId, target: SaveTarget) -> SaveReport {
        let (tx, rx) = mpsc::channel();
        let address = self
            .sessions
            .begin_save(
                target,
                Some(Continuation::CloseForm(id)),
                |plan, _, cancel| {
                    spawn_save(plan, cancel.clone(), move |v| tx.send(v).expect("save"))
                },
            )
            .expect("save started");
        assert!(
            self.close(id.tab()).is_err(),
            "Close ran before publication"
        );
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
            "duplicate Save answer continued"
        );
        report
    }
}

#[test]
fn same_document_tabs_keep_literal_forms_and_history_until_their_own_close_succeeds() {
    let root = tempfile::tempdir().expect("root");
    let a_file = root.path().join("a.fcad");
    let b_file = root.path().join("b.fcad");
    plate(&a_file, 12.0);
    std::fs::copy(&a_file, &b_file).expect("same UUID");
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    let a_base = w.sessions.export_source().expect("base");
    w.constraints_draft(1, 3);
    crate::constraints::tests::session_apply::press(&mut w.creates.sketch.constraints, "Undo");
    let a_form = format!("{:?}", w.creates.sketch);
    w.open(&b_file);
    let b = w.sessions.tab();
    assert_eq!(document_id(&a_file), document_id(&b_file));
    assert_ne!(a, b);
    w.open_height_form();
    let feature = w.feature();
    w.edits.type_height(feature, " 2..6 ");
    let b_base = w.sessions.export_source().expect("base");
    let b_private = w
        .sessions
        .private_directory()
        .expect("private")
        .to_path_buf();
    let id = w.ask_form_close();
    assert!(
        w.tabs
            .form_close_question(&w.sessions)
            .expect("question")
            .1
            .contains("b.fcad")
    );
    assert!(w.back_from_form_close(id));
    assert_eq!(w.edits.typed(), Some((Some(feature), " 2..6 ")));
    assert!(Arc::ptr_eq(
        &b_base,
        &w.sessions.export_source().expect("base")
    ));
    assert_eq!(private_count(&w.sessions), 1, "Back made a step");
    w.round(a);
    assert_eq!(format!("{:?}", w.creates.sketch), a_form);
    assert!(Arc::ptr_eq(
        &a_base,
        &w.sessions.export_source().expect("base")
    ));
    // Hidden B is shown by the old route; refused preparation leaves both forms.
    assert_eq!(
        w.tabs.close_step(&w.sessions, b, true),
        Some(CloseStep::Show)
    );
    let (g, rx) = w.begin_switch(b, Some(After::Close));
    let (outcome, after) = w
        .deliver_switch(g, wait(&rx), Err(CadError::input("upload refused")))
        .expect("awaited");
    assert!(outcome.is_err() && after.is_none());
    assert_eq!(format!("{:?}", w.creates.sketch), a_form);
    w.round(b);
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    assert_eq!(
        w.sessions.replacing(None),
        Replace::Go,
        "clean model needs no Save"
    );
    w.decide_form_close(id);
    // The actual empty picture can refuse too. Nothing was destroyed.
    assert!(
        crate::close_shown(
            &mut w.scene,
            &mut w.input,
            &mut w.sessions,
            &mut w.tabs,
            &mut w.checkpoint_name,
            b,
            |_| Err::<(), _>(CadError::input("empty upload refused"))
        )
        .is_err()
    );
    assert!(w.back_from_form_close(id));
    assert_eq!(w.edits.typed(), Some((Some(feature), " 2..6 ")));
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    w.decide_form_close(id);
    drop(b_base);
    assert_eq!(w.close(b).expect("closed"), Some(a));
    assert!(
        !b_private.exists(),
        "form's lease kept private files after Close"
    );
    assert_eq!(w.tabs.count(), 1);
    w.round(a);
    assert_eq!(format!("{:?}", w.creates.sketch), a_form);
    crate::constraints::tests::session_apply::press(&mut w.creates.sketch.constraints, "Redo");
    assert_ne!(
        format!("{:?}", w.creates.sketch),
        a_form,
        "draft Redo was lost"
    );
    println!("\nFCAD_30U_TAB_FORMS_EXECUTED");
}

#[test]
fn cancelled_close_and_refused_saves_return_the_original_form_and_accepted_model() {
    let root = tempfile::tempdir().expect("root");
    let a_file = root.path().join("a.fcad");
    let b_file = root.path().join("b.fcad");
    plate(&a_file, 12.0);
    std::fs::copy(&a_file, &b_file).expect("copy");
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a_file);
    let a = w.sessions.tab();
    w.apply(18.0);
    w.open_height_form();
    let feature = w.feature();
    w.edits.type_height(feature, " 26x ");
    w.open(&b_file);
    let b = w.sessions.tab();
    w.open_height_form();
    w.edits.type_height(feature, " b ");
    w.round(a);
    let base = w.sessions.export_source().expect("base");
    let files = private_count(&w.sessions);
    for choice in [Some(UnsavedChoice::Cancel), None] {
        let id = w.ask_form_close();
        w.confirm_form_close(id);
        assert_eq!(w.sessions.replacing(choice), Replace::Stay);
        assert!(w.back_from_form_close(id));
        assert_eq!(w.edits.typed(), Some((Some(feature), " 26x ")));
    }
    // Save As Cancel means no SavePlan or session operation starts.
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    let mut dialogs = crate::dialogs::Dialogs::default();
    assert_eq!(
        dialogs.receive(
            crate::dialogs::Action::SaveAs,
            crate::dialogs::Outcome::Cancelled,
            &mut w.input,
            None
        ),
        None
    );
    assert!(!w.sessions.busy());
    assert!(w.back_from_form_close(id));
    assert!(w.tabs.save_as_refusal(&w.sessions, &b_file).is_some());
    for target in [
        SaveTarget::As(b_file.clone()),
        SaveTarget::As(a_file.clone()),
        SaveTarget::As(root.path().join("absent-parent/output.fcad")),
    ] {
        let id = w.ask_form_close();
        w.confirm_form_close(id);
        let report = w.save_for_form_close(id, target);
        assert!(!report.published && report.continuation.is_none());
        assert!(w.back_from_form_close(id));
        assert_eq!(w.edits.typed(), Some((Some(feature), " 26x ")));
    }
    // A Save cancelled before publication answers through the real save worker.
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    let (tx, rx) = mpsc::channel();
    let address = w
        .sessions
        .begin_save(
            SaveTarget::As(root.path().join("cancelled.fcad")),
            Some(Continuation::CloseForm(id)),
            |plan, _, _| {
                let cancel = CancelToken::new();
                cancel.cancel();
                spawn_save(plan, cancel, move |answer| tx.send(answer).expect("Save"))
            },
        )
        .expect("Save");
    let report = w.sessions.finish_save(address, wait(&rx)).expect("answer");
    assert!(!report.published && report.continuation.is_none());
    assert!(w.back_from_form_close(id));
    assert_eq!(w.edits.typed(), Some((Some(feature), " 26x ")));
    assert!(!root.path().join("cancelled.fcad").exists());
    // External file replacement is a real Save conflict, never a model overwrite.
    let external = root.path().join("external.fcad");
    plate(&external, 31.0);
    std::fs::copy(&external, &a_file).expect("external replacement");
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    let report = w.save_for_form_close(id, SaveTarget::InPlace);
    assert!(!report.published && report.continuation.is_none());
    assert!(w.back_from_form_close(id));
    assert_eq!(height_of(&a_file), 31.0);
    assert_eq!(height_in(&w.sessions), 18.0);
    assert!(w.sessions.dirty() && Arc::ptr_eq(&base, &w.sessions.export_source().expect("base")));
    assert_eq!(w.edits.typed(), Some((Some(feature), " 26x ")));
    assert_eq!(private_count(&w.sessions), files);
    w.round(b);
    assert_eq!(w.edits.typed(), Some((Some(feature), " b ")));
    w.round(a);
    // Save-and-Close of an untitled model uses no-clobber Save As, accepted model only.
    w.edits.cancel();
    w.create(ferritecad_jobs::NewDocument::SamplePlate(PlateSize {
        width: 80.,
        depth: 40.,
        height: 18.,
    }))
    .expect("Untitled");
    w.open_height_form();
    let feature = w.feature();
    w.edits.type_height(feature, " 99x ");
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Save)),
        Replace::AfterSave
    );
    let saved = root.path().join("saved.fcad");
    let report = w.save_for_form_close(id, SaveTarget::As(saved.clone()));
    assert!(report.published && report.continuation == Some(Continuation::CloseForm(id)));
    assert_eq!(height_of(&saved), 18.0, "Save read the unapplied form");
    assert!(!w.sessions.dirty());
    w.decide_form_close(id);
    w.close(id.tab()).expect("closed");
    assert_eq!(w.tabs.count(), 2);
    w.round(a);
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, "still unapplied");
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    w.decide_form_close(id);
    w.close(a).expect("explicit model Discard");
    assert_eq!(w.tabs.count(), 1);
    w.round(b);
    assert_eq!(w.edits.typed(), Some((Some(f), " b ")));
    println!("\nFCAD_30U_CANCEL_SAVE_EXECUTED");
}

#[test]
fn close_decisions_are_once_only_and_address_the_tab_attempt_and_snapshot() {
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&file);
    let a = w.sessions.tab();
    w.apply(18.);
    w.open_height_form();
    let feature = w.feature();
    w.edits.type_height(feature, "- ");
    let old = w.ask_form_close();
    assert!(w.back_from_form_close(old));
    w.edits.type_height(feature, " 2..6 ");
    let new = w.ask_form_close();
    assert_ne!(old, new);
    assert!(!w.tabs.confirm_form_close(&w.sessions, old));
    assert!(!w.back_from_form_close(old));
    w.confirm_form_close(new);
    assert!(
        !w.tabs.confirm_form_close(&w.sessions, new),
        "repeated first answer"
    );
    assert!(
        w.close(a).is_err(),
        "confirmation bypassed dirty-model decision"
    );
    assert!(
        w.tabs
            .begin_switch(&mut w.sessions, TabId::default(), None, |_, _, _| panic!(
                "worker"
            ))
            .is_err()
    );
    assert!(!crate::begin_window_quit(
        &mut w.tabs,
        &mut w.sessions,
        &w.creates,
        &Loads::default(),
        &Exports::default(),
        &w.edits
    ));
    let mut other = Sessions::default();
    other.adopt(
        DocumentSession::open_in(w.private.path(), &file, HistoryLimits::default()).expect("other"),
    );
    assert!(!w.tabs.decide_form_close(&other, new));
    assert!(w.tabs.close(&mut other, a).is_err());
    // Changing the accepted snapshot invalidates the answer, even with same TabId.
    w.sessions.adopt(
        DocumentSession::open_in(w.private.path(), &file, HistoryLimits::default())
            .expect("new base"),
    );
    assert_eq!(w.sessions.tab(), a);
    assert!(!w.tabs.decide_form_close(&w.sessions, new));
    assert!(w.back_from_form_close(new));
    assert_eq!(w.edits.typed(), Some((Some(feature), " 2..6 ")));
    assert!(
        !w.sessions.takes_form_apply(),
        "restored original form must be held on newer base"
    );
    println!("\nFCAD_30U_ADDRESS_EXECUTED");
}

#[test]
fn every_existing_form_family_uses_the_same_move_and_keeps_its_entire_state() {
    let root = tempfile::tempdir().expect("fixtures");
    let polygon = crate::fillets::tests::plate();
    let cut = crate::cuts::tests::session_apply::fixture(1);
    let fillet = crate::fillets::tests::session_apply::fixture(1);
    let chamfer = crate::chamfers::tests::session_apply::fixture();
    let circle = root.path().join("circle.fcad");
    let annulus = root.path().join("annulus.fcad");
    let sector = root.path().join("sector.fcad");
    crate::sketch::tests::analytic_apply::write_round(&circle, [1., 2.], &[10.], 12., false);
    crate::sketch::tests::analytic_apply::write_round(&annulus, [1., 2.], &[10., 4.], 12., false);
    crate::constraints::tests::revolve::write_sector(
        &sector,
        &crate::sketch::tests::angle_apply::RADIAL,
        137.5,
        None,
    );
    // All saved-object branches of Editor::saved_object_open, including Add/Edit.
    for (kind, path) in [
        ("height", &polygon.1),
        ("vertices", &polygon.1),
        ("constraints", &polygon.1),
        ("circle", &circle),
        ("annulus", &annulus),
        ("angle", &sector),
        ("cut-add", &polygon.1),
        ("cut-edit", &cut.1),
        ("fillet-add", &polygon.1),
        ("fillet-edit", &fillet.1),
        ("chamfer-add", &polygon.1),
        ("chamfer-edit", &chamfer.1),
    ] {
        let reading = read_extrude_source(path).expect("reading");
        let mut w = Window::new(Drawn::Mock, None);
        w.sessions.adopt(
            DocumentSession::open_in(w.private.path(), path, HistoryLimits::default())
                .expect("session"),
        );
        match kind {
            "height" => {
                w.open_height_form();
                let f = w.feature();
                w.edits.type_height(f, " 1e999x ");
            }
            "vertices" => {
                w.creates.sketch = crate::sketch::tests::typed_apply(
                    path,
                    &reading,
                    reading.sketches[0].sketch,
                    "33",
                    " 41.25 ",
                )
                .0;
                crate::sketch::tests::press(&mut w.creates.sketch, "Undo draft");
            }
            "constraints" => {
                w.constraints_draft(1, 3);
            }
            "circle" => {
                w.creates.sketch = crate::sketch::tests::analytic_apply::typed_circle(
                    path,
                    &reading,
                    reading.circle_sketches[0].sketch,
                    [" 3.0 ", "-2"],
                    " 11.0 ",
                )
                .0;
            }
            "annulus" => {
                w.creates.sketch = crate::sketch::tests::analytic_apply::typed_annulus(
                    path,
                    &reading,
                    reading.annulus_sketches[0].sketch,
                    ["-2", " 3.0 "],
                    " 11.0 ",
                    " 3.0 ",
                )
                .0;
            }
            "angle" => {
                w.creates.sketch = crate::sketch::tests::angle_apply::typed_angle(
                    path,
                    &reading,
                    &w.sessions,
                    " 90.0 ",
                )
                .0;
            }
            "cut-add" => {
                assert!(
                    w.creates
                        .sketch
                        .cuts
                        .begin(path, &reading, reading.cut_bodies[0].body)
                );
            }
            "cut-edit" => {
                w.creates.sketch = crate::cuts::tests::session_apply::typed_cut(
                    path,
                    &reading,
                    &w.sessions,
                    0,
                    " 3.0 ",
                    " 2.0 ",
                    None,
                )
                .0;
            }
            "fillet-add" => {
                assert!(w.creates.sketch.fillets.begin(
                    path,
                    &reading,
                    reading.fillet_bodies[0].body
                ));
            }
            "fillet-edit" => {
                w.creates.sketch = crate::fillets::tests::session_apply::typed_radius(
                    path,
                    &reading,
                    &w.sessions,
                    reading.fillet_features[0].feature,
                    " 2.0 ",
                )
                .0;
            }
            "chamfer-add" => {
                assert!(w.creates.sketch.chamfers.begin(
                    path,
                    &reading,
                    reading.chamfer_bodies[0].body
                ));
            }
            "chamfer-edit" => {
                w.creates.sketch = crate::chamfers::tests::session_apply::typed_distance(
                    path,
                    &reading,
                    &w.sessions,
                    reading.chamfer_features[0].feature,
                    " 2.0 ",
                )
                .0;
            }
            _ => unreachable!(),
        }
        let editor = format!("{:?}", w.creates.sketch);
        let height = w.edits.typed().map(|(id, text)| (id, text.to_owned()));
        let id = w.ask_form_close();
        assert!(
            !w.creates.sketch.active() && !w.edits.form_open(),
            "{kind} not parked"
        );
        w.confirm_form_close(id);
        assert!(w.back_from_form_close(id));
        assert_eq!(format!("{:?}", w.creates.sketch), editor, "{kind}");
        assert_eq!(
            w.edits.typed().map(|(id, text)| (id, text.to_owned())),
            height,
            "{kind}"
        );
        assert!(!w.sessions.dirty());
    }
    println!("\nFCAD_30U_FORM_FAMILIES_EXECUTED families=12");
}

fn form_frame(
    w: &mut Window,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, ferritecad_ui::CloseFormChoice) {
    let mut choice = ferritecad_ui::CloseFormChoice::Waiting;
    let question = w.tabs.form_close_question(&w.sessions);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(988., 768.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            if w.tabs.closing_form() {
                choice = ferritecad_ui::close_form_panel(
                    ui,
                    question
                        .as_ref()
                        .map(|(_, name)| ferritecad_ui::CloseFormPanel { document: name }),
                );
                return;
            }
            crate::in_tab_scope(ui, "tab-height", w.sessions.tab().key(), |ui| {
                w.edits
                    .draw_with(ui, false, None, ferritecad_ui::HeightState::default());
            });
        },
    );
    output.textures_delta.clear();
    (output, choice)
}
fn centre(out: &egui::FullOutput, label: &str) -> egui::Pos2 {
    out.shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("not painted: {label}"))
}
fn click_form(
    w: &mut Window,
    ctx: &egui::Context,
    at: egui::Pos2,
) -> ferritecad_ui::CloseFormChoice {
    form_frame(w, ctx, vec![egui::Event::PointerMoved(at)]);
    let mut choice = ferritecad_ui::CloseFormChoice::Waiting;
    for pressed in [true, false] {
        let answer = form_frame(
            w,
            ctx,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }],
        )
        .1;
        if answer != ferritecad_ui::CloseFormChoice::Waiting {
            choice = answer;
        }
    }
    choice
}
#[test]
fn real_close_buttons_name_unapplied_values_and_parked_fields_cannot_take_typing() {
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&file);
    w.open_height_form();
    let ctx = egui::Context::default();
    let feature = w.feature();
    w.edits.type_height(feature, "12");
    for _ in 0..3 {
        form_frame(&mut w, &ctx, vec![]);
    }
    let out = form_frame(&mut w, &ctx, vec![]).0;
    click_form(&mut w, &ctx, centre(&out, "12"));
    form_frame(&mut w, &ctx, vec![egui::Event::Text("x ".into())]);
    let original = w.edits.typed().expect("typed").1.to_owned();
    assert_ne!(original, "12");
    let id = w.ask_form_close();
    for _ in 0..3 {
        form_frame(&mut w, &ctx, vec![]);
    }
    let out = form_frame(&mut w, &ctx, vec![egui::Event::Text("999".into())]).0;
    let words = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        words.contains("a.fcad")
            && words.contains("not yet applied")
            && words.contains("Unsaved model changes")
    );
    assert_eq!(
        click_form(&mut w, &ctx, centre(&out, ferritecad_ui::BACK_TO_FORM)),
        ferritecad_ui::CloseFormChoice::Back
    );
    assert!(w.back_from_form_close(id));
    form_frame(&mut w, &ctx, vec![]);
    form_frame(&mut w, &ctx, vec![egui::Event::Text("777".into())]);
    assert_eq!(
        w.edits.typed(),
        Some((Some(feature), original.as_str())),
        "focus from question edited the returned field"
    );
    let id = w.ask_form_close();
    for _ in 0..3 {
        form_frame(&mut w, &ctx, vec![]);
    }
    let out = form_frame(&mut w, &ctx, vec![]).0;
    assert_eq!(
        click_form(
            &mut w,
            &ctx,
            centre(&out, ferritecad_ui::DISCARD_FORM_AND_CLOSE)
        ),
        ferritecad_ui::CloseFormChoice::Discard
    );
    w.confirm_form_close(id);
    assert!(w.tabs.form_close_question(&w.sessions).is_none());
    assert!(w.back_from_form_close(id));
    assert_eq!(w.edits.typed(), Some((Some(feature), original.as_str())));
    println!("\nFCAD_30U_WIDGETS_EXECUTED");
}

#[test]
fn stub_hidden_close_refusal_keeps_both_forms_and_no_native_work_is_claimed() {
    if ferritecad_occt::is_available() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let a = root.path().join("a.fcad");
    let b = root.path().join("b.fcad");
    plate(&a, 12.);
    std::fs::copy(&a, &b).expect("copy");
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&a);
    let tab = w.sessions.tab();
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " a x ");
    w.open(&b);
    w.open_height_form();
    w.edits.type_height(f, " b x ");
    w.drawn = Drawn::Native;
    let (g, rx) = w.begin_switch(tab, Some(After::Close));
    let (outcome, after) = w.deliver_switch(g, wait(&rx), Ok(())).expect("answer");
    assert!(outcome.is_err() && after.is_none());
    assert_eq!(w.edits.typed(), Some((Some(f), " b x ")));
    w.drawn = Drawn::Mock;
    w.round(tab);
    assert_eq!(w.edits.typed(), Some((Some(f), " a x ")));
    println!("\nFCAD_30U_STUB_EXECUTED");
}

#[test]
fn mixed_close_with_occt_and_no_solver_keeps_the_form_on_cancel() {
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
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Cancel)),
        Replace::Stay
    );
    assert!(w.back_from_form_close(id));
    assert_eq!(w.edits.typed(), Some((Some(f), " 26x ")));
    assert_eq!(height_in(&w.sessions), 18.);
    println!("\nFCAD_30U_MIXED_EXECUTED");
}

fn close_recipe(root: &Path) {
    let store = RecoveryStore::open(&root.join("recovery")).expect("store");
    let mut w = Window::new(Drawn::Native, Some(&store));
    w.open(&root.join("a.fcad"));
    let a = w.sessions.tab();
    w.apply(18.);
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " 26x ");
    w.open(&root.join("b.fcad"));
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
    let b_base = w.sessions.export_source().expect("B");
    w.round(a);
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Cancel)),
        Replace::Stay
    );
    assert!(w.back_from_form_close(id));
    assert_eq!(w.edits.typed(), Some((Some(f), " 26x ")));
    w.edits.type_height(f, "26");
    assert!(w.apply_height_form());
    let alias = w.sessions.suggestion().expect("alias");
    export_bytes(
        &w.sessions.export_path().expect("A"),
        &alias,
        root,
        "a-unsaved",
    );
    // Another unfinished form proves Save reads only the accepted 26 mm model.
    w.open_height_form();
    w.edits.type_height(f, " 99x ");
    let private = w
        .sessions
        .private_directory()
        .expect("private")
        .to_path_buf();
    let id = w.ask_form_close();
    w.confirm_form_close(id);
    assert_eq!(
        w.sessions.replacing(Some(UnsavedChoice::Save)),
        Replace::AfterSave
    );
    let report = w.save_for_form_close(id, SaveTarget::InPlace);
    assert!(report.published && report.continuation == Some(Continuation::CloseForm(id)));
    w.decide_form_close(id);
    assert_eq!(w.close(a).expect("Close"), Some(b));
    // The one process recorder can still be finishing its last accepted-model
    // copy. Its snapshot lease ends asynchronously; it must release A's files.
    let deadline = Instant::now() + Duration::from_secs(10);
    while private.exists() {
        assert!(
            Instant::now() < deadline,
            "closed A's private files stayed leased"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!private.exists());
    assert_eq!(w.tabs.count(), 1);
    w.round(b);
    assert_eq!(format!("{:?}", w.creates.sketch), b_form);
    assert!(Arc::ptr_eq(
        &b_base,
        &w.sessions.export_source().expect("B")
    ));
    w.sessions.stop_all();
    w.tabs.stop_all();
}

const CLOSE_OUTPUTS: [&str; 3] = ["a.fcad", "a-unsaved.stl", "a-unsaved.fbx"];
fn compare_close(root: &Path) {
    use super::super::super::checkpoints::{all_sql, ids_and_refs, stl_facts};
    for name in CLOSE_OUTPUTS {
        assert!(root.join(name).is_file(), "missing real GUI output: {name}");
    }
    let read = |name: &str| std::fs::read(root.join(name)).expect(name);
    assert!(
        read("a.fcad") != read("inputs/a.fcad"),
        "a.fcad was never saved"
    );
    assert_eq!(read("b.fcad"), read("inputs/b.fcad"), "B was written");
    let work = tempfile::tempdir().expect("peer");
    let source = root.join("inputs/a.fcad");
    let reading = read_extrude_source(&source).expect("source");
    let peer = work.path().join("peer.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        reading.features[0].feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        reading.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "26".as_ref(),
        "-o".as_ref(),
        peer.as_os_str(),
    ]);
    let saved = root.join("a.fcad");
    assert_eq!(
        all_sql(&saved, &[], false),
        all_sql(&peer, &[], false),
        "all SQL except write stamp"
    );
    assert_eq!(
        ids_and_refs(&saved),
        ids_and_refs(&source),
        "UUID/reference changed"
    );
    let (stl, fbx) = peer_bytes(&peer, work.path(), "expected");
    assert_eq!(read("a-unsaved.stl"), stl, "STL differs from CLI");
    assert_eq!(read("a-unsaved.fbx"), fbx, "FBX differs from CLI");
    assert_eq!(
        peer_bytes(&saved, work.path(), "saved"),
        (stl.clone(), fbx),
        "Save changed exported model"
    );
    let (_, height, volume) = stl_facts(&stl);
    assert!(
        (height - 26.).abs() < 1e-4 && (volume - 83200.).abs() < 1e-2,
        "geometry {height} {volume}"
    );
    let records = RecoveryStore::open(&root.join("recovery"))
        .expect("store")
        .list()
        .expect("list");
    assert_eq!(
        records.recoverable().count(),
        0,
        "closed A left a recovery record"
    );
}

#[test]
fn native_close_cancel_apply_export_save_and_close_matches_cli_and_keeps_b() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    make_inputs(root.path());
    close_recipe(root.path());
    compare_close(root.path());
    if let Ok(dir) = std::env::var("FCAD_CLOSE_DRAFT_ARTIFACTS") {
        for ext in ["stl", "fbx"] {
            std::fs::copy(
                root.path().join(format!("a-unsaved.{ext}")),
                Path::new(&dir).join(format!("close-draft-a.{ext}")),
            )
            .expect("artifact");
        }
    }
    println!("\nFCAD_30U_NATIVE_EXECUTED volume_a=83200.000 all_SQL_cells=true");
}

#[test]
fn native_compare_real_close_draft_gui_outputs() {
    let Ok(root) = std::env::var("FCAD_30U_GUI_DIR") else {
        return;
    };
    assert!(native(), "GUI comparator requires native geometry");
    compare_close(Path::new(&root));
    println!("\nFCAD_30U_GUI_COMPARE_OK all_SQL_cells=true");
}

#[test]
fn close_request_preserves_foreground_work_and_gesture_holds() {
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("a.fcad");
    plate(&file, 12.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&file);
    w.open_height_form();
    let f = w.feature();
    w.edits.type_height(f, " a x ");
    let mut loads = Loads::default();
    let mut exports = Exports::default();
    let request = |w: &mut Window, loads: &Loads, exports: &Exports| {
        crate::ask_form_close(
            &mut w.tabs,
            &w.sessions,
            &mut w.creates,
            &mut w.edits,
            loads,
            exports,
            &w.input,
        )
    };
    let kept = |w: &Window| {
        assert!(!w.tabs.closing_form());
        assert_eq!(w.edits.typed(), Some((Some(f), " a x ")));
    };
    let g = w
        .sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("Apply");
    assert!(request(&mut w, &loads, &exports).is_none());
    kept(&w);
    assert_eq!(
        w.sessions.finish_apply(g, Err(CadError::Cancelled)),
        Edited::Failed
    );
    let g = w
        .sessions
        .begin_save(SaveTarget::InPlace, None, |_, _, _| {
            std::thread::spawn(|| {})
        })
        .expect("Save");
    assert!(request(&mut w, &loads, &exports).is_none());
    kept(&w);
    w.sessions
        .finish_save(
            g,
            Err(ferritecad_jobs::SaveFailure {
                kind: ferritecad_jobs::SaveFailureKind::Cancelled,
                error: CadError::Cancelled,
            }),
        )
        .expect("answer");
    let g = loads
        .open(
            Some(&file),
            Arc::new(crate::ProgressRelay::default()),
            |_, _| std::thread::spawn(|| {}),
        )
        .expect("load");
    assert!(request(&mut w, &loads, &exports).is_none());
    kept(&w);
    loads.answered(g, Err(CadError::Cancelled));
    let destination = root.path().join("out.fbx");
    std::fs::write(&destination, b"occupied").expect("file");
    assert!(
        crate::exports::begin_export(
            &mut exports,
            &mut w.input,
            Some(&file),
            Some(destination),
            |_, _, _, _| panic!("unconfirmed export")
        )
        .is_none()
    );
    assert!(request(&mut w, &loads, &exports).is_none());
    kept(&w);
    exports = Exports::default();
    assert!(crate::creates::open_form(&mut w.creates, &mut w.input));
    assert!(request(&mut w, &loads, &exports).is_none());
    kept(&w);
    w.creates = crate::creates::Creates::default();
    crate::sketch::tests::begin_drawing(&mut w.creates.sketch);
    assert!(request(&mut w, &loads, &exports).is_none());
    kept(&w);
    w.creates.sketch.dismiss();
    let source = w.sessions.export_path().expect("source");
    let reading = read_extrude_source(&source).expect("reading");
    w.edits.type_height(f, "20");
    let copy = w
        .edits
        .request(root.path().join("copy.fcad"))
        .expect("copy");
    w.edits.start(copy, |_, _, _| std::thread::spawn(|| {}));
    assert!(request(&mut w, &loads, &exports).is_none());
    assert!(!w.tabs.closing_form());
    w.edits.stop_all();
    w.open_height_form();
    w.edits.type_height(f, " a x ");
    w.input
        .handle(ViewportEvent::PointerMoved { x: 10., y: 10. }, false);
    w.input
        .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
    assert!(request(&mut w, &loads, &exports).is_none());
    kept(&w);
    w.input.handle(ViewportEvent::GestureCancelled, false);
    assert!(
        w.creates
            .sketch
            .begin_edit(&source, &reading, reading.sketches[0].sketch)
    );
    let (ctx, at) = crate::sketch::drag_tests::hold_vertex(&mut w.creates.sketch);
    assert!(request(&mut w, &loads, &exports).is_none());
    kept(&w);
    crate::sketch::drag_tests::release_vertex(&ctx, &mut w.creates.sketch, at);
    assert!(
        request(&mut w, &loads, &exports).is_some(),
        "idle forms should be offered Close"
    );
    println!("\nFCAD_30U_HOLDS_EXECUTED");
}
