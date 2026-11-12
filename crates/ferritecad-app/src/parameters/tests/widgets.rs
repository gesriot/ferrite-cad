// SPDX-License-Identifier: MIT
use super::*;

pub(super) struct Ui {
    ctx: egui::Context,
    confirmation: Option<commands::Confirmation>,
    size: egui::Vec2,
}

impl Ui {
    pub(super) fn new(window: &mut Window) -> Self {
        let mut result = Self {
            ctx: egui::Context::default(),
            confirmation: None,
            size: egui::vec2(1200., 1000.),
        };
        // An ordinary open form uses the viewport laid out before its opening click.
        let editor = std::mem::take(&mut window.creates.sketch.parameters);
        result.settle(window);
        window.creates.sketch.parameters = editor;
        result
    }

    fn frame(
        &mut self,
        w: &mut Window,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, commands::Chosen) {
        let available = availability(w, self.confirmation.is_some());
        let tab = w.sessions.tab().key();
        let (form, sketch) = w.creates.forms();
        let mut menu = None;
        let mut view = crate::workspace::View {
            activity: ferritecad_ui::Activity::default(),
            sections: crate::Sections {
                dialog_failure: None,
                sketch,
                creating: false,
                edits: &mut w.edits,
                stl_form: None,
                can_edit: false,
                can_new: true,
                height: ferritecad_ui::HeightState {
                    running: false,
                    can_cancel: false,
                    apply: false,
                    copy: false,
                    unsaved: false,
                },
                held_back: false,
                document_outcome: &w.sessions.status,
                export: None,
                replacing: None,
                form,
                open_over_new: None,
                closing_form: false,
                close_form: None,
                quit_new: None,
                created: None,
                reopen: Default::default(),
                recovery: Default::default(),
                checkpoints: None,
                tabs: ferritecad_ui::TabStrip {
                    tabs: &[],
                    available: Ok(()),
                },
                forms_scope: tab,
                model: Some((tab, w.tabs.model_navigation(w.sessions.tab()))),
                model_confirmation: self.confirmation.as_ref(),
                model_menu: &mut menu,
                model_available: available,
            },
            scene: crate::workspace::Scene {
                document: w.scene.document.as_deref(),
                edit_source: w.scene.edit_source.as_ref(),
                sketches: &w.scene.sketches,
                definitions: &[],
                chosen_row: None,
                offers: &[],
                described: None,
                solves: &[],
            },
        };
        let mut answers = crate::workspace::Answers::default();
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                events,
                ..Default::default()
            },
            |ui| answers = crate::workspace::compose(ui, &mut view),
        );
        output.textures_delta.clear();
        (output, answers.chosen)
    }

    pub(super) fn settle(&mut self, w: &mut Window) -> egui::FullOutput {
        for _ in 0..3 {
            self.frame(w, vec![]);
        }
        self.frame(w, vec![]).0
    }

    pub(super) fn press(&mut self, w: &mut Window, label: &str, last: bool) -> commands::Chosen {
        let output = self.settle(w);
        let at = rect(&output, label, last).center();
        self.frame(w, clicks(at)).1
    }

    pub(super) fn type_field(&mut self, w: &mut Window, label: &str, text: &str) {
        let output = self.settle(w);
        let label = rect(&output, label, true);
        self.frame(
            w,
            clicks(egui::pos2(label.left() + 30., label.bottom() + 12.)),
        );
        self.frame(
            w,
            vec![
                key(egui::Key::A, egui::Modifiers::COMMAND),
                egui::Event::Text(text.into()),
            ],
        );
    }
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn clicks(at: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    ]
}

fn rect(output: &egui::FullOutput, words: &str, last: bool) -> egui::Rect {
    let matches: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == words => {
                Some(text.visual_bounding_rect())
            }
            _ => None,
        })
        .collect();
    *if last {
        matches.last()
    } else {
        matches.first()
    }
    .unwrap_or_else(|| panic!("missing visible text {words}"))
}

pub(super) fn text(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

pub(super) fn form_text(output: &egui::FullOutput) -> Vec<String> {
    text(output)
        .into_iter()
        .skip_while(|label| label != "Parameters")
        .skip(1)
        .collect()
}

pub(super) fn values_text(output: &egui::FullOutput) -> Vec<String> {
    form_text(output)
        .into_iter()
        .skip_while(|label| label != "Values")
        .skip(1)
        .take_while(|label| label != "Add parameter")
        .collect()
}

#[test]
fn empty_document_opens_parameters_adds_all_dimensions_and_reopens_the_selected_editor() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("empty.fcad");
    Document::create(&path)
        .expect("empty document")
        .close()
        .expect("close");
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&path);
    let mut ui = Ui::new(&mut w);
    assert!(!w.creates.sketch.parameters.active());
    ui.press(&mut w, "Parameters…", false);
    assert!(
        w.creates.sketch.parameters.active(),
        "the first parameter needs no existing row"
    );
    for (name, expression, dimension, label, value) in [
        ("stock", "10 mm", Dimension::Length, "Length", "10 mm"),
        ("turn", "30 deg", Dimension::Angle, "Angle", "30°"),
        ("ratio", "2", Dimension::Scalar, "Scalar", "2"),
    ] {
        ui.type_field(&mut w, "New name", name);
        ui.type_field(&mut w, "New expression", expression);
        ui.press(&mut w, label, true);
        let asked = ui
            .press(&mut w, "Add parameter", true)
            .parameter
            .expect("Add widget");
        let EditChange::AddParameter {
            object,
            dimension: asked_dimension,
            ..
        } = asked.change
        else {
            panic!("Add parameter request")
        };
        assert_eq!(asked_dimension, dimension);
        assert!(apply(&mut w, asked));
        assert_eq!(
            parameter(w.scene.edit_source.as_ref().expect("source"), object)
                .expect("added parameter")
                .expression
                .source,
            expression
        );
        let labels = form_text(&ui.settle(&mut w));
        assert_eq!(
            labels
                .iter()
                .find(|word| *word == value)
                .map(String::as_str),
            Some(value)
        );
    }
    ui.press(&mut w, "stock", false);
    let selected = w
        .tabs
        .model_navigation(w.sessions.tab())
        .selected
        .expect("tree selection");
    ui.press(&mut w, "Close", true);
    assert!(!w.creates.sketch.parameters.active());
    let asked = ui
        .press(&mut w, "Edit…", false)
        .model
        .expect("parameter Edit route");
    assert_eq!(asked.row, selected);
    assert_eq!(
        asked.edit,
        Command::Edit(ferritecad_ui::ModelEdit::Parameters)
    );
    assert!(matches!(
        launch(&mut w, asked.edit).expect("idle catalogue"),
        Launch::Opened(true)
    ));
    ui.settle(&mut w);
    let draft = w.creates.sketch.parameters.draft.as_ref().expect("form");
    assert_eq!(
        (draft.selected, draft.text.as_str()),
        (Some(selected.0), "10 mm")
    );
    assert!(
        ui.press(&mut w, "Apply parameter", true)
            .parameter
            .is_none(),
        "unchanged text does not submit another edit"
    );
    ui.type_field(&mut w, "Parameter expression", "20 mm");
    w.sessions.hold_stale_draft();
    assert!(
        ui.press(&mut w, "Apply parameter", true)
            .parameter
            .is_none(),
        "the existing held-draft guard also disables the form's Apply"
    );
}

#[test]
fn parameters_entry_and_field_widgets_preserve_existing_read_only_and_pending_guards() {
    let f = fixture();
    let mut w = open(&f, Drawn::Mock);
    let mut ui = Ui::new(&mut w);
    ui.press(&mut w, "Close", true);
    w.scene.edit_source.as_mut().expect("source").refusal = Some("read only".into());
    ui.press(&mut w, "Parameters…", false);
    assert!(
        !w.creates.sketch.parameters.active(),
        "read-only sources do not open an editor"
    );
    w.scene.edit_source.as_mut().expect("source").refusal = None;
    ui.press(&mut w, "Parameters…", false);
    ui.type_field(&mut w, "Parameter expression", "20 mm");
    ui.press(&mut w, "Parameters…", false);
    assert_eq!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("draft")
            .text,
        "20 mm",
        "an open form cannot be replaced through the entry button"
    );
    let address = sessions::Address {
        tab: w.sessions.tab(),
        generation: 99,
    };
    w.creates
        .sketch
        .parameters
        .draft
        .as_mut()
        .expect("draft")
        .pending = Some((address, Field::Text(f.stock)));
    assert!(
        ui.press(&mut w, "Apply parameter", true)
            .parameter
            .is_none()
    );
    ui.press(&mut w, "Close", true);
    assert!(
        w.creates.sketch.parameters.active(),
        "a pending owner address keeps the form"
    );
    w.creates
        .sketch
        .parameters
        .finished(address, false, w.scene.edit_source.as_ref(), "Cancelled");
    w.scene.edit_source.as_mut().expect("source").refusal = Some("read only".into());
    assert!(
        ui.press(&mut w, "Apply parameter", true)
            .parameter
            .is_none()
    );
    ui.press(&mut w, "Close", true);
    assert!(!w.creates.sketch.parameters.active());
}

#[test]
fn values_use_the_tree_sentence_for_skipped_and_geometry_failed_objects_without_cached_numbers() {
    let f = fixture();
    for skipped in [false, true] {
        let mut w = open(&f, Drawn::Mock);
        let mut report = ferritecad_eval::RebuildReport::default();
        let expected = if skipped {
            report.skipped.push(ferritecad_eval::SkippedFeature {
                id: f.feature,
                cause: f.stock,
            });
            "not built: depends on stock"
        } else {
            report.failed.push(ferritecad_eval::FailedFeature {
                id: f.feature,
                kind: "kernel".into(),
                message: "boolean failed".into(),
            });
            "Failed: boolean failed"
        };
        let source = w.scene.edit_source.as_mut().expect("source");
        let facts: &mut ferritecad_document::ExtrudeEditSource = source;
        facts.unbuilt_statuses = ferritecad_scene::project_rebuild_status(
            &mut facts.model_tree,
            &facts.objects,
            &report,
        );
        assert_eq!(facts.unbuilt_statuses, vec![(f.feature, expected.into())]);
        for row in facts
            .model_tree
            .rows
            .iter()
            .filter(|row| row.id.0 == f.feature)
        {
            assert_eq!(row.note, expected);
        }
        select(&mut w, f.feature);
        let mut ui = Ui::new(&mut w);
        let output = ui.settle(&mut w);
        let labels = values_text(&output);
        assert_eq!(labels.iter().filter(|label| *label == expected).count(), 1);
        assert_eq!(
            labels.iter().filter(|label| *label == "13 mm").count(),
            0,
            "Values must hide the cached number of an unbuilt object"
        );
        assert_eq!(labels.iter().filter(|label| *label == "total").count(), 1);
        ui.type_field(&mut w, "Extrude distance", "stock + 3 mm");
        // Status presentation does not prevent correcting the stored expression.
        let draft = w.creates.sketch.parameters.draft.as_ref().expect("form");
        assert_eq!(
            draft.slots[0],
            (ExpressionSlot::ExtrudeDistance, "stock + 3 mm".into())
        );
        assert_eq!(
            ui.press(&mut w, "Apply value", true)
                .parameter
                .expect("editable slot")
                .change,
            EditChange::SetExpressionText {
                object: f.feature,
                slot: ExpressionSlot::ExtrudeDistance,
                source: "stock + 3 mm".into()
            }
        );
    }
}

#[test]
fn parameter_widgets_follow_tree_selection_and_backspace_belongs_to_text_when_focused() {
    let f = fixture();
    let mut w = open(&f, Drawn::Mock);
    let mut ui = Ui::new(&mut w);
    let output = ui.settle(&mut w);
    assert!(
        !text(&output)
            .iter()
            .any(|s| s == "Sketch + Extrude — new document")
    );
    for label in ["stock", "total", "10 mm", "13 mm", "stock + 3 mm"] {
        assert!(text(&output).iter().any(|s| s == label), "missing {label}");
    }
    let at = rect(&output, "total", false).center();
    ui.frame(&mut w, clicks(at));
    assert_eq!(
        w.tabs
            .model_navigation(w.sessions.tab())
            .selected
            .expect("tree selection")
            .0,
        f.total
    );
    assert_eq!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("draft")
            .text,
        "stock + 3 mm"
    );
    ui.type_field(&mut w, "Parameter expression", "18 mm");
    assert_eq!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("typed")
            .text,
        "18 mm"
    );
    let answer = ui
        .frame(
            &mut w,
            vec![key(egui::Key::Backspace, egui::Modifiers::NONE)],
        )
        .1;
    assert!(answer.model.is_none());
    assert_eq!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("typed")
            .text,
        "18 m"
    );
    assert!(!w.sessions.dirty());
    // A table selection shares the ordinary tree action address, including its epoch.
    for key_code in [egui::Key::Delete, egui::Key::Backspace] {
        ui.ctx
            .memory_mut(|memory| memory.surrender_focus(memory.focused().expect("field focus")));
        let answer = ui
            .frame(&mut w, vec![key(key_code, egui::Modifiers::NONE)])
            .1;
        let action = answer.model.expect("catalogue shortcut");
        assert_eq!(action.edit, Command::Delete);
        assert_eq!(action.row.0, f.total);
        assert!(w.tabs.model_addressed(w.sessions.tab(), action));
        // Refocus for the next iteration, without submitting the draft.
        ui.type_field(&mut w, "Parameter expression", "18 m");
    }
    select(&mut w, f.feature);
    let output = ui.settle(&mut w);
    assert!(text(&output).iter().any(|s| s == "Extrude distance"));
    assert!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("draft")
            .text
            .is_empty()
    );
}

#[test]
fn table_and_tree_delete_offers_have_one_address_and_confirmation_preserves_the_draft() {
    let f = fixture();
    let mut w = open(&f, Drawn::Mock);
    let mut ui = Ui::new(&mut w);
    w.creates
        .sketch
        .parameters
        .draft
        .as_mut()
        .expect("draft")
        .add_text = "keep me".into();
    let tree = ui
        .press(&mut w, "Delete", false)
        .model
        .expect("tree command");
    let table = ui
        .press(&mut w, "Delete", true)
        .model
        .expect("table command");
    assert_eq!(tree, table);
    for delete in [false, true] {
        let asked = ui
            .press(&mut w, "Delete with dependents…", true)
            .model
            .expect("table action");
        let Launch::Confirm(confirmation) = launch(&mut w, asked.edit).expect("confirmation")
        else {
            panic!("expected confirmation");
        };
        ui.confirmation = Some(confirmation);
        let output = ui.settle(&mut w);
        assert!(!text(&output).iter().any(|s| s == "Parameter expression"));
        assert!(text(&output).iter().any(|s| s == "Delete with dependents"));
        let answer = ui.press(&mut w, if delete { "Delete" } else { "Cancel" }, true);
        assert_eq!(answer.confirm_delete, Some(delete));
        let available = availability(&w, false);
        let result = ui
            .confirmation
            .take()
            .expect("question")
            .finish(
                delete,
                w.scene.edit_source.as_ref(),
                &w.sessions,
                &w.tabs,
                available,
            )
            .expect("current question");
        if let Some((expected, changes)) = result {
            let (tx, rx) = mpsc::channel();
            let address = w
                .sessions
                .apply(expected, changes, move |_, reply| {
                    tx.send(reply).expect("reply");
                })
                .expect("one batch");
            let sessions::Edited::Show(path) = w.sessions.finish_apply(
                address,
                rx.recv_timeout(Duration::from_secs(60)).expect("reply"),
            ) else {
                panic!("deletion failed: {}", w.sessions.status)
            };
            w.show_staged(address, path);
            assert!(w.creates.sketch.parameters.active());
            assert!(w.sessions.dirty());
            ui.settle(&mut w);
            assert!(
                w.creates
                    .sketch
                    .parameters
                    .draft
                    .as_ref()
                    .expect("draft")
                    .selected
                    .is_none()
            );
            w.creates.sketch.parameters.draft = None;
            w.step(true);
            assert!(
                !w.sessions.dirty(),
                "one Undo returns to the original version"
            );
            assert!(parameter(w.scene.edit_source.as_ref().expect("source"), f.stock).is_some());
        } else {
            assert!(!w.sessions.dirty());
            ui.settle(&mut w);
            assert_eq!(
                w.creates
                    .sketch
                    .parameters
                    .draft
                    .as_ref()
                    .expect("draft")
                    .add_text,
                "keep me"
            );
        }
    }
}

#[test]
fn parameter_tree_context_and_table_delete_the_same_unused_object_and_keep_the_form() {
    for table in [false, true] {
        let f = fixture();
        let spare = ObjectId::new();
        let mut document = Document::open(&f.path).expect("document");
        document
            .write_changes(&[EditChange::AddParameter {
                object: spare,
                name: "spare".into(),
                dimension: Dimension::Scalar,
                expression: "2".into(),
            }])
            .expect("unused parameter");
        document.close().expect("close");
        let mut w = open(&f, Drawn::Mock);
        select(&mut w, spare);
        let mut ui = Ui::new(&mut w);
        assert_eq!(
            text(&ui.settle(&mut w))
                .iter()
                .filter(|s| *s == "Delete")
                .count(),
            2,
            "both the table and tree buttons are visible"
        );
        let tree = ui
            .press(&mut w, "Delete", false)
            .model
            .expect("tree button");
        let from_table = ui
            .press(&mut w, "Delete", true)
            .model
            .expect("table button");
        assert_eq!(tree, from_table);
        let output = ui.settle(&mut w);
        let at = rect(&output, "spare · parameter", false).center();
        let events = clicks(at)
            .into_iter()
            .map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    modifiers,
                    ..
                } => egui::Event::PointerButton {
                    pos,
                    pressed,
                    modifiers,
                    button: egui::PointerButton::Secondary,
                },
                event => event,
            })
            .collect();
        ui.frame(&mut w, events);
        let from_menu = ui
            .press(&mut w, "Delete", true)
            .model
            .expect("context command");
        assert_eq!(from_menu, tree);
        let asked = if table { from_table } else { tree };
        assert!(w.tabs.model_addressed(w.sessions.tab(), asked));
        let Launch::Apply(expected, changes) = launch(&mut w, asked.edit).expect("launch") else {
            panic!("ordinary deletion")
        };
        assert_eq!(changes, vec![EditChange::RemoveObject { object: spare }]);
        let (tx, rx) = mpsc::channel();
        let address = w
            .sessions
            .apply(expected, changes, move |_, reply| {
                tx.send(reply).expect("reply");
            })
            .expect("apply");
        let sessions::Edited::Show(path) = w.sessions.finish_apply(
            address,
            rx.recv_timeout(Duration::from_secs(60)).expect("reply"),
        ) else {
            panic!("deletion: {}", w.sessions.status)
        };
        w.show_staged(address, path);
        ui.settle(&mut w);
        assert!(
            w.creates.sketch.parameters.active(),
            "deletion keeps Parameters open"
        );
        assert!(parameter(w.scene.edit_source.as_ref().expect("source"), spare).is_none());
        ui.press(&mut w, "Close", true);
        w.step(true);
        assert!(!w.sessions.dirty(), "one deletion is one Undo step");
        assert!(parameter(w.scene.edit_source.as_ref().expect("source"), spare).is_some());
    }
}

#[test]
fn parameter_widget_refusal_keeps_text_and_used_parameter_deletion_uses_only_status() {
    let f = fixture();
    let mut w = open(&f, Drawn::Mock);
    let mut ui = Ui::new(&mut w);
    let before = (
        w.scene.catalogue.clone(),
        w.scene.faces.clone(),
        w.scene.edit_source.clone(),
    );
    let version = w.sessions.export_source().expect("source").version();
    ui.type_field(&mut w, "Parameter expression", "unknown + 3 mm");
    let asked = ui
        .press(&mut w, "Apply parameter", true)
        .parameter
        .expect("Apply");
    assert!(!apply(&mut w, asked));
    assert!(
        w.sessions
            .status
            .contains("unknown parameter 'unknown' at byte 1")
    );
    let output = ui.settle(&mut w);
    assert!(text(&output).iter().any(|s| s == &w.sessions.status));
    let draft = w.creates.sketch.parameters.draft.as_ref().expect("form");
    assert_eq!(draft.text, "unknown + 3 mm");
    let field_refusal = draft.refusal.clone();
    let asked = ui
        .press(&mut w, "Delete", true)
        .model
        .expect("table Delete");
    let Launch::Apply(expected, changes) = launch(&mut w, asked.edit).expect("catalogue") else {
        panic!("ordinary removal")
    };
    let (tx, rx) = mpsc::channel();
    let address = w
        .sessions
        .apply(expected, changes, move |_, reply| {
            tx.send(reply).expect("reply");
        })
        .expect("owner request");
    let reply = rx.recv_timeout(Duration::from_secs(60)).expect("reply");
    let error = reply
        .as_ref()
        .expect_err("used parameter must refuse")
        .to_string();
    assert!(matches!(
        w.sessions.finish_apply(address, reply),
        sessions::Edited::Failed
    ));
    assert_eq!(
        w.sessions.status,
        format!("Could not apply the change: {error}")
    );
    assert!(w.sessions.status.contains("total"));
    assert_eq!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("form")
            .refusal,
        field_refusal,
        "deletion does not create another field-error route"
    );
    assert_eq!(
        (&w.scene.catalogue, &w.scene.faces, &w.scene.edit_source),
        (&before.0, &before.1, &before.2)
    );
    assert_eq!(
        w.sessions.export_source().expect("source").version(),
        version
    );
    assert!(!w.sessions.dirty());
}

#[test]
fn forty_parameter_rows_scroll_in_name_order_within_the_form_height_budget() {
    let f = fixture();
    let mut document = Document::open(&f.path).expect("document");
    let changes: Vec<_> = (0..38)
        .rev()
        .map(|i| EditChange::AddParameter {
            object: ObjectId::new(),
            name: format!("A{i:02}"),
            dimension: Dimension::Scalar,
            expression: i.to_string(),
        })
        .collect();
    document
        .write_changes(&changes)
        .expect("forty small parameter rows");
    document.close().expect("close");
    let mut w = open(&f, Drawn::Mock);
    let mut ui = Ui::new(&mut w);
    ui.size = egui::vec2(1024., 768.);
    let output = ui.settle(&mut w);
    let a = rect(&output, "A00", true);
    assert!(a.top() < rect(&output, "A01", true).top());
    assert!(!text(&output).iter().any(|s| s == "A37"));
    assert!(rect(&output, "Close", true).bottom() < 768.);
    for _ in 0..12 {
        ui.frame(
            &mut w,
            vec![
                egui::Event::PointerMoved(a.center()),
                egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0., -160.),
                    modifiers: Default::default(),
                },
            ],
        );
        let output = ui.settle(&mut w);
        if text(&output).iter().any(|s| s == "total") {
            break;
        }
    }
    let output = ui.settle(&mut w);
    assert!(
        text(&output).iter().any(|s| s == "A37"),
        "last rows are reachable"
    );
    let row = |name: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                let egui::Shape::Text(text) = &shape.shape else {
                    return None;
                };
                let rect = text.visual_bounding_rect();
                (text.galley.text() == name
                    // Glyph ink bearings differ within the same name column.
                    && (rect.left() - a.left()).abs() < 5.
                    && rect.top() >= a.top() - 30.
                    && rect.bottom() <= a.top() + 144.)
                    .then_some(rect)
            })
            .unwrap_or_else(|| {
                let matches: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        let egui::Shape::Text(text) = &shape.shape else {
                            return None;
                        };
                        (text.galley.text() == name)
                            .then_some((text.visual_bounding_rect(), shape.clip_rect))
                    })
                    .collect();
                panic!("missing visible table row {name}, first row {a:?}, matches {matches:?}")
            })
    };
    assert!(row("A37").top() < row("stock").top());
    assert!(row("stock").top() < row("total").top());
    assert!(
        rect(&output, "Close", true).bottom() < 768.,
        "form remains bounded"
    );
}
