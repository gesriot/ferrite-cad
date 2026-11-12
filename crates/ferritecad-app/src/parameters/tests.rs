// SPDX-License-Identifier: MIT
#![allow(
    clippy::panic,
    reason = "acceptance witnesses must fail at the violated invariant"
)]
use super::*;
use crate::commands::{Access, Action, Command, Launch};
use crate::sessions::tests::tabs::{Drawn, Window};
use ferritecad_document::Document;
use std::{path::PathBuf, sync::mpsc, time::Duration};

mod native;
mod widgets;

struct Fixture {
    _root: tempfile::TempDir,
    path: PathBuf,
    stock: ObjectId,
    total: ObjectId,
    feature: ObjectId,
}

fn fixture() -> Fixture {
    let (root, path, source) = crate::cuts::tests::session_apply::fixture(0);
    let feature = source.expression_slots[0].object;
    let (stock, total) = (ObjectId::new(), ObjectId::new());
    let mut document = Document::open(&path).expect("document");
    document
        .write_changes(&[
            EditChange::AddParameter {
                object: stock,
                name: "stock".into(),
                dimension: Dimension::Length,
                expression: "10 mm".into(),
            },
            EditChange::AddParameter {
                object: total,
                name: "total".into(),
                dimension: Dimension::Length,
                expression: "stock + 3 mm".into(),
            },
            EditChange::SetExpressionText {
                object: feature,
                slot: ExpressionSlot::ExtrudeDistance,
                source: "total".into(),
            },
        ])
        .expect("parameter graph");
    document.close().expect("close");
    Fixture {
        _root: root,
        path,
        stock,
        total,
        feature,
    }
}

fn open(fixture: &Fixture, drawn: Drawn) -> Window {
    let mut window = Window::new(drawn, None);
    window.open(&fixture.path);
    let source = window.scene.edit_source.as_ref().expect("accepted facts");
    window.creates.sketch.parameters.begin(source);
    select(&mut window, fixture.stock);
    window
}

fn select(window: &mut Window, id: ObjectId) {
    let source = window.scene.edit_source.as_ref().expect("source");
    let row = source
        .model_tree
        .rows
        .iter()
        .find(|row| row.id.0 == id)
        .expect("row")
        .id;
    let navigation = window.tabs.model_navigation(window.sessions.tab());
    commands::select_row(source, navigation, row);
    window
        .creates
        .sketch
        .parameters
        .draft
        .as_mut()
        .expect("form")
        .select(navigation.selected, source);
}

fn availability(window: &Window, confirming: bool) -> commands::Availability {
    commands::availability(
        &window.creates,
        &window.edits,
        &window.sessions,
        &window.tabs,
        &crate::Loads::default(),
        &crate::exports::Exports::default(),
        &window.input,
        confirming,
    )
}

fn launch(window: &mut Window, command: Command) -> Result<Launch, String> {
    let navigation = window.tabs.model_navigation(window.sessions.tab());
    let action = Action {
        tab: window.sessions.tab().key(),
        epoch: navigation.epoch,
        row: navigation.selected.expect("selection"),
        edit: command,
    };
    commands::launch(
        action,
        window.scene.edit_source.as_ref().expect("source"),
        &window.scene.sketches,
        &mut window.creates,
        &mut window.edits,
        &window.sessions,
        &window.tabs,
        &crate::Loads::default(),
        &crate::exports::Exports::default(),
        &window.input,
        false,
    )
}

fn apply(window: &mut Window, request: Request) -> bool {
    apply_observing(window, request, |_| {})
}

fn apply_observing(
    window: &mut Window,
    request: Request,
    inspect: impl FnOnce(&ferritecad_scene::LoadedScene),
) -> bool {
    let (tx, rx) = mpsc::channel();
    let available = availability(window, false);
    let address = window
        .creates
        .sketch
        .parameters
        .apply(&mut window.sessions, available, request, move |_, reply| {
            tx.send(reply).expect("reply");
        })
        .expect("owner accepted request");
    let reply = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("owner reply");
    let kernel_error = reply.as_ref().err().map(ToString::to_string);
    let accepted = match window.sessions.finish_apply(address, reply) {
        sessions::Edited::Show(path) => {
            accept_observing(window, address, path, inspect);
            true
        }
        sessions::Edited::Failed => false,
        sessions::Edited::Keep(_) | sessions::Edited::NoChange | sessions::Edited::Ignore => {
            panic!("unexpected owner outcome: {}", window.sessions.status);
        }
    };
    window.creates.sketch.parameters.finished(
        address,
        accepted,
        window.scene.edit_source.as_ref(),
        &window.sessions.status,
    );
    if let Some(error) = kernel_error {
        assert_eq!(
            window.sessions.status,
            format!("Could not apply the change: {error}")
        );
        assert_eq!(
            window
                .creates
                .sketch
                .parameters
                .draft
                .as_ref()
                .expect("form")
                .refusal
                .as_ref()
                .expect("field refusal")
                .1,
            window.sessions.status
        );
    }
    accepted
}

fn accept_observing(
    window: &mut Window,
    address: sessions::Address,
    path: PathBuf,
    inspect: impl FnOnce(&ferritecad_scene::LoadedScene),
) {
    let (tx, rx) = mpsc::channel();
    window.sessions.draw_staged(address, move |scene| {
        tx.send(scene).expect("scene");
    });
    let scene = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("picture")
        .expect("built");
    if window.drawn == Drawn::Native {
        let cold = ferritecad_scene::snapshot_of(
            &path,
            &mut ferritecad_occt::OcctKernel::new().expect("OCCT"),
            ferritecad_occt::OcctKernel::import_step,
            &ferritecad_kernel::TessellationParams::default(),
            &ferritecad_kernel::OperationContext::default(),
        )
        .expect("independent cold reading");
        assert_eq!(
            scene, cold,
            "candidate agrees with cold loading before publication"
        );
    }
    inspect(&scene);
    let outcome = window.present(&path, Ok(scene), sessions::Bind::Staged, Ok(()));
    assert!(outcome.is_ok(), "{outcome:?}");
    assert!(window.sessions.finish_scene(address, outcome));
}

fn request(window: &Window, field: Field, change: EditChange) -> Request {
    Request {
        expected: window.sessions.export_source().expect("source").version(),
        field,
        change,
    }
}

#[test]
fn parameters_catalogue_exception_checks_both_command_and_kind_then_operation() {
    let f = fixture();
    let mut w = open(&f, Drawn::Mock);
    w.creates
        .sketch
        .parameters
        .draft
        .as_mut()
        .expect("form")
        .add_text = "23.".into();
    let original = std::fs::read(&f.path).expect("original");
    let version = w.sessions.export_source().expect("source").version();
    let all = [
        Command::Edit(ferritecad_ui::ModelEdit::Parameters),
        Command::Delete,
        Command::DeleteWithDependents,
        Command::Suppress,
        Command::Unsuppress,
    ];
    let objects: Vec<_> = w
        .scene
        .edit_source
        .as_ref()
        .expect("source")
        .objects
        .iter()
        .map(|o| o.id)
        .collect();
    for id in objects {
        select(&mut w, id);
        let source = w.scene.edit_source.as_ref().expect("source");
        let row = w
            .tabs
            .model_navigation(w.sessions.tab())
            .selected
            .expect("selected");
        let allowed = availability(&w, false);
        assert_eq!(allowed, Ok(Access::Parameters { operation: Ok(()) }));
        let is_parameter = parameter(source, id).is_some();
        for command in all {
            let offered = commands::offers(source, row, allowed);
            if let Some(offer) = offered.iter().find(|offer| offer.command == command) {
                assert_eq!(
                    offer.available.is_ok(),
                    is_parameter
                        && matches!(command, Command::Delete | Command::DeleteWithDependents)
                );
            }
        }
        for command in all {
            let result = launch(&mut w, command);
            if is_parameter && matches!(command, Command::Delete | Command::DeleteWithDependents) {
                match (command, result.expect("parameter deletion offered")) {
                    (Command::Delete, Launch::Apply(expected, changes)) => {
                        assert_eq!(expected, version);
                        assert_eq!(changes, vec![EditChange::RemoveObject { object: id }]);
                    }
                    (Command::DeleteWithDependents, Launch::Confirm(_)) => {}
                    _ => panic!("wrong deletion route"),
                }
            } else {
                assert!(matches!(result, Err(ref reason)
                    if reason == "Close the Parameters form before another command."));
            }
            assert_eq!(
                w.sessions.export_source().expect("version").version(),
                version
            );
            assert!(!w.sessions.dirty());
            assert_eq!(
                w.creates
                    .sketch
                    .parameters
                    .draft
                    .as_ref()
                    .expect("form")
                    .add_text,
                "23."
            );
        }
    }
    select(&mut w, f.stock);
    assert_eq!(
        availability(&w, true),
        Err("Delete or Cancel the pending deletion before another command.")
    );
    w.sessions.hold_stale_draft();
    for command in [Command::Delete, Command::DeleteWithDependents] {
        assert!(matches!(launch(&mut w, command), Err(ref reason)
            if reason == "Finish the current operation, New, dialog or gesture before editing."));
    }
    assert_eq!(std::fs::read(&f.path).expect("unchanged"), original);
}

#[test]
fn accepted_parameter_steps_keep_the_form_and_only_reset_the_submitted_field() {
    let f = fixture();
    let mut w = open(&f, Drawn::Mock);
    let draft = w.creates.sketch.parameters.draft.as_mut().expect("form");
    draft.name = "unsent_name".into();
    draft.text = "18 mm".into();
    draft.add_name = "later".into();
    draft.add_text = "stock * 2".into();
    let asked = request(
        &w,
        Field::Text(f.stock),
        EditChange::SetParameter {
            object: f.stock,
            expression: "18 mm".into(),
        },
    );
    assert!(apply(&mut w, asked));
    assert!(
        w.creates.sketch.parameters.active(),
        "Parameters survived accepted version"
    );
    let draft = w.creates.sketch.parameters.draft.as_ref().expect("form");
    assert_eq!(
        (
            &*draft.name,
            &*draft.text,
            &*draft.add_name,
            &*draft.add_text
        ),
        ("unsent_name", "18 mm", "later", "stock * 2")
    );
    let source = w.scene.edit_source.as_ref().expect("source");
    assert_eq!(
        parameter(source, f.total)
            .expect("total")
            .expression
            .value(),
        21.
    );
    assert!(w.sessions.dirty());
    let asked = request(
        &w,
        Field::Name(f.stock),
        EditChange::RenameParameter {
            object: f.stock,
            name: "thickness".into(),
        },
    );
    assert!(apply(&mut w, asked));
    let source = w.scene.edit_source.as_ref().expect("source");
    assert_eq!(
        parameter(source, f.total).expect("total").expression.source,
        "thickness + 3 mm"
    );
    assert_eq!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("form")
            .name,
        "thickness"
    );
    assert_eq!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("form")
            .add_text,
        "stock * 2"
    );
    for (dimension, name, text) in [
        (Dimension::Length, "gap", "2 mm"),
        (Dimension::Angle, "turn", "30 deg"),
        (Dimension::Scalar, "ratio", "2"),
    ] {
        let asked = request(
            &w,
            Field::Add,
            EditChange::AddParameter {
                object: ObjectId::new(),
                name: name.into(),
                dimension,
                expression: text.into(),
            },
        );
        assert!(apply(&mut w, asked));
        let draft = w.creates.sketch.parameters.draft.as_ref().expect("form");
        assert!(draft.add_name.is_empty() && draft.add_text.is_empty());
    }
    assert_eq!(words::value(30_f64.to_radians(), Dimension::Angle), "30°");
}

#[test]
fn selection_changes_discard_only_the_old_selected_fields_and_follow_the_tree() {
    let f = fixture();
    let mut w = open(&f, Drawn::Mock);
    let draft = w.creates.sketch.parameters.draft.as_mut().expect("form");
    draft.text = "unsent".into();
    draft.add_text = "keep this".into();
    select(&mut w, f.total);
    let draft = w.creates.sketch.parameters.draft.as_ref().expect("form");
    assert_eq!(draft.text, "stock + 3 mm");
    assert_eq!(draft.add_text, "keep this");
    select(&mut w, f.feature);
    let draft = w.creates.sketch.parameters.draft.as_ref().expect("form");
    assert!(draft.name.is_empty() && draft.text.is_empty());
    assert_eq!(
        draft.slots,
        vec![(ExpressionSlot::ExtrudeDistance, "total".into())]
    );
    select(&mut w, f.stock);
    let draft = w.creates.sketch.parameters.draft.as_ref().expect("form");
    assert_eq!(draft.text, "10 mm");
    assert!(draft.slots.is_empty());
}

#[test]
fn accepted_source_reports_dependency_slots_without_guessing_names_in_the_window() {
    let f = fixture();
    let w = open(&f, Drawn::Mock);
    let source = w.scene.edit_source.as_ref().expect("source");
    let consumers = source.parameter_consumers.as_ref().expect("saved graph");
    assert_eq!(consumers.len(), 2);
    assert!(
        consumers
            .iter()
            .any(|c| c.parameter == f.stock && c.object == f.total && c.slot.is_none())
    );
    assert!(consumers.iter().any(|c| c.parameter == f.total
        && c.object == f.feature
        && c.slot == Some(ExpressionSlot::ExtrudeDistance)));
}

#[test]
fn rename_updates_the_visible_dependent_texts_and_one_undo_restores_them() {
    let f = fixture();
    let mut document = Document::open(&f.path).expect("document");
    document
        .write_changes(&[EditChange::SetExpressionText {
            object: f.feature,
            slot: ExpressionSlot::ExtrudeDistance,
            source: "stock + 3 mm".into(),
        }])
        .expect("direct reference");
    document.close().expect("close");
    let mut w = open(&f, Drawn::Mock);
    let mut ui = widgets::Ui::new(&mut w);
    ui.type_field(&mut w, "Parameter name", "thickness");
    let asked = ui
        .press(&mut w, "Rename", true)
        .parameter
        .expect("Rename widget");
    assert!(apply(&mut w, asked));
    let labels = widgets::form_text(&ui.settle(&mut w));
    assert_eq!(
        labels
            .iter()
            .find(|label| *label == "thickness + 3 mm")
            .map(String::as_str),
        Some("thickness + 3 mm")
    );
    select(&mut w, f.feature);
    let values = widgets::values_text(&ui.settle(&mut w));
    assert_eq!(
        values
            .iter()
            .find(|label| *label == "thickness + 3 mm")
            .map(String::as_str),
        Some("thickness + 3 mm")
    );
    assert_eq!(
        values
            .iter()
            .find(|label| *label == "13 mm")
            .map(String::as_str),
        Some("13 mm")
    );
    ui.press(&mut w, "Close", true);
    w.step(true);
    assert!(!w.sessions.dirty());
    let source = w.scene.edit_source.as_ref().expect("source");
    assert_eq!(
        parameter(source, f.total).expect("total").expression.source,
        "stock + 3 mm"
    );
    assert_eq!(
        source
            .expression_slot(f.feature, ExpressionSlot::ExtrudeDistance)
            .expect("height")
            .expression
            .source,
        "stock + 3 mm"
    );
}

#[test]
fn accepted_versions_keep_unsent_slots_but_other_documents_and_missing_slots_end_their_drafts() {
    let f = fixture();
    let other = fixture();
    let mut w = open(&f, Drawn::Mock);
    select(&mut w, f.feature);
    w.creates
        .sketch
        .parameters
        .draft
        .as_mut()
        .expect("draft")
        .slots[0]
        .1 = "unsent height".into();
    let asked = request(
        &w,
        Field::Add,
        EditChange::AddParameter {
            object: ObjectId::new(),
            name: "later".into(),
            dimension: Dimension::Scalar,
            expression: "2".into(),
        },
    );
    assert!(apply(&mut w, asked));
    let source = w.scene.edit_source.as_ref().expect("source");
    let row = source
        .model_tree
        .rows
        .iter()
        .find(|row| row.id.0 == f.feature)
        .expect("feature row")
        .id;
    let draft = w
        .creates
        .sketch
        .parameters
        .draft
        .as_mut()
        .expect("form stayed open");
    draft.select(Some(row), source);
    assert_eq!(
        draft.slots,
        vec![(ExpressionSlot::ExtrudeDistance, "unsent height".into())]
    );
    let mut missing = source.clone();
    missing.expression_slots.clear();
    draft.select(Some(row), &missing);
    assert_eq!(draft.slots, vec![]);
    draft.select(Some(row), source);
    assert_eq!(
        draft.slots,
        vec![(ExpressionSlot::ExtrudeDistance, "total".into())]
    );
    w.creates.sketch.parameters.document_arrived(Some(source));
    assert!(w.creates.sketch.parameters.active());
    let other_source = crate::sketch_facts::read(&other.path).expect("other document");
    w.creates
        .sketch
        .parameters
        .document_arrived(Some(&other_source));
    assert!(!w.creates.sketch.parameters.active());
    w.creates.sketch.parameters.begin(source);
    w.creates.sketch.parameters.document_arrived(None);
    assert!(!w.creates.sketch.parameters.active());
}

#[test]
fn owner_completions_do_not_replace_unsent_fields_after_the_tree_selection_changes() {
    use ferritecad_document::{EndCondition, Expression, FeatureAddition, SolidOperation};
    let f = fixture();
    let mut document = Document::open(&f.path).expect("document");
    let ObjectPayload::Extrude(extrude) = document
        .object(f.feature)
        .expect("object")
        .expect("feature")
        .payload
    else {
        panic!("extrusion")
    };
    let other = ObjectId::new();
    document
        .write_changes(&[EditChange::AddBody {
            object: ObjectId::new(),
            name: Some("B".into()),
            feature_object: other,
            feature_name: Some("Other height".into()),
            feature: FeatureAddition::Extrude {
                profile: extrude.profile,
                regions: extrude.regions,
                reversed: false,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(5.).expect("literal"),
                },
                operation: SolidOperation::NewBody,
            },
        }])
        .expect("second feature with the same slot kind");
    document.close().expect("close");
    let mut w = open(&f, Drawn::Mock);
    let address = sessions::Address {
        tab: w.sessions.tab(),
        generation: 99,
    };
    for (field, selected) in [
        (Field::Name(f.stock), f.total),
        (Field::Text(f.stock), f.total),
        (
            Field::Slot(f.feature, ExpressionSlot::ExtrudeDistance),
            other,
        ),
    ] {
        select(&mut w, selected);
        let draft = w.creates.sketch.parameters.draft.as_mut().expect("form");
        draft.name = "unsent name".into();
        draft.text = "unsent expression".into();
        for (_, text) in &mut draft.slots {
            *text = "unsent slot".into();
        }
        let before = (draft.name.clone(), draft.text.clone(), draft.slots.clone());
        draft.pending = Some((address, field));
        w.creates.sketch.parameters.finished(
            address,
            true,
            w.scene.edit_source.as_ref(),
            "Applied",
        );
        let draft = w.creates.sketch.parameters.draft.as_ref().expect("form");
        assert_eq!(
            (&draft.name, &draft.text, &draft.slots),
            (&before.0, &before.1, &before.2),
            "a completed field belongs to its submitted object, not the new selection"
        );
        assert!(draft.pending.is_none());
    }
    select(&mut w, f.feature);
    let mut source = w.scene.edit_source.as_ref().expect("source").clone();
    source
        .expression_slots
        .retain(|saved| saved.object != f.feature);
    assert!(
        source
            .expression_slot(other, ExpressionSlot::ExtrudeDistance)
            .is_some()
    );
    let selected = w.tabs.model_navigation(w.sessions.tab()).selected;
    let draft = w.creates.sketch.parameters.draft.as_mut().expect("form");
    draft.select(selected, &source);
    assert!(
        draft.slots.is_empty(),
        "another object's slot must not preserve the selected object's missing slot"
    );
}

#[test]
fn parameter_refusals_keep_the_full_kernel_message_text_version_and_picture() {
    let f = fixture();
    let mut w = open(&f, Drawn::Mock);
    let original = std::fs::read(&f.path).expect("source");
    let version = w.sessions.export_source().expect("version").version();
    let scene = format!(
        "{:?}{:?}{:?}",
        w.scene.catalogue, w.scene.faces, w.scene.edit_source
    );
    for (rename, typed, words) in [
        (true, "2bad", vec!["name"]),
        (true, "mm", vec!["reserved", "unit"]),
        (true, "total", vec!["duplicate", "total"]),
        (false, "stock + )", vec!["byte"]),
        (
            false,
            "missing + 3 mm",
            vec!["unknown parameter", "missing"],
        ),
        (
            false,
            "30 deg",
            vec!["expression must have dimension length"],
        ),
        (false, "total", vec!["cycle", "stock", "total"]),
        (false, "1 mm / 0", vec!["division by zero"]),
    ] {
        let draft = w.creates.sketch.parameters.draft.as_mut().expect("form");
        let (field, change) = if rename {
            draft.name = typed.into();
            (
                Field::Name(f.stock),
                EditChange::RenameParameter {
                    object: f.stock,
                    name: typed.into(),
                },
            )
        } else {
            draft.text = typed.into();
            (
                Field::Text(f.stock),
                EditChange::SetParameter {
                    object: f.stock,
                    expression: typed.into(),
                },
            )
        };
        let asked = request(&w, field, change);
        assert!(!apply(&mut w, asked), "kernel should refuse {typed}");
        for word in words {
            assert!(w.sessions.status.contains(word), "{}", w.sessions.status);
        }
        let draft = w.creates.sketch.parameters.draft.as_ref().expect("form");
        assert_eq!(if rename { &draft.name } else { &draft.text }, typed);
        assert_eq!(
            w.sessions.export_source().expect("unchanged").version(),
            version
        );
        assert_eq!(
            format!(
                "{:?}{:?}{:?}",
                w.scene.catalogue, w.scene.faces, w.scene.edit_source
            ),
            scene
        );
        assert!(!w.sessions.dirty());
        eprintln!("PARAMETER_REFUSAL {typed}: {}", w.sessions.status);
    }
    assert_eq!(std::fs::read(&f.path).expect("unchanged source"), original);
}

#[test]
fn parameters_park_with_their_tab_and_keep_the_existing_close_undo_checkpoint_guards() {
    let f = fixture();
    let other = fixture();
    let mut w = open(&f, Drawn::Mock);
    let first = w.sessions.tab();
    w.creates
        .sketch
        .parameters
        .draft
        .as_mut()
        .expect("form")
        .add_text = "unfinished".into();
    let state = format!("{:?}", w.creates.sketch.parameters);
    let form_open = crate::form_open(&w.edits, &w.creates.sketch);
    assert!(
        form_open,
        "Parameters holds document Undo/Redo through the existing guard"
    );
    let current = w.sessions.export_source().expect("snapshot");
    assert_eq!(
        crate::checkpoints::availability(true, form_open, Some(&current), "test").create,
        Err(
            "Close the open form first: a checkpoint keeps the accepted model, not a draft.".into()
        )
    );
    w.open(&other.path);
    assert!(!w.creates.sketch.parameters.active());
    w.switch(first);
    assert_eq!(format!("{:?}", w.creates.sketch.parameters), state);
    let close = crate::ask_form_close(
        &mut w.tabs,
        &w.sessions,
        &mut w.creates,
        &mut w.edits,
        &crate::Loads::default(),
        &crate::exports::Exports::default(),
        &w.input,
    )
    .expect("existing Close question");
    assert!(w.tabs.closing_form());
    assert!(w.tabs.cancel_form_close(
        &mut w.sessions,
        close,
        &mut crate::tabs::Forms {
            edits: &mut w.edits,
            editor: &mut w.creates.sketch,
        }
    ));
    assert_eq!(format!("{:?}", w.creates.sketch.parameters), state);
    let close = crate::ask_form_close(
        &mut w.tabs,
        &w.sessions,
        &mut w.creates,
        &mut w.edits,
        &crate::Loads::default(),
        &crate::exports::Exports::default(),
        &w.input,
    )
    .expect("question again");
    assert!(w.tabs.confirm_form_close(&w.sessions, close));
    assert!(w.tabs.decide_form_close(&w.sessions, close));
    w.close(first).expect("close tab");
    assert!(!w.creates.sketch.parameters.active());
}

#[test]
fn parameter_apply_uses_the_catalogue_operation_guard_and_ignores_other_addresses() {
    let f = fixture();
    let mut w = open(&f, Drawn::Mock);
    w.sessions.hold_stale_draft();
    let asked = request(
        &w,
        Field::Text(f.stock),
        EditChange::SetParameter {
            object: f.stock,
            expression: "42 mm".into(),
        },
    );
    let available = availability(&w, false);
    assert!(
        w.creates
            .sketch
            .parameters
            .apply(&mut w.sessions, available, asked, |_, _| panic!(
                "held draft dispatched a worker"
            ))
            .is_none()
    );
    assert!(!w.sessions.busy() && !w.sessions.dirty());
    assert_eq!(
        w.sessions.status,
        "Finish the current operation, New, dialog or gesture before editing."
    );
    let address = sessions::Address {
        tab: w.sessions.tab(),
        generation: 99,
    };
    let draft = w.creates.sketch.parameters.draft.as_mut().expect("form");
    draft.pending = Some((address, Field::Add));
    draft.add_text = "unsent".into();
    w.creates.sketch.parameters.finished(
        address + 1,
        true,
        w.scene.edit_source.as_ref(),
        "wrong address",
    );
    assert_eq!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("form")
            .add_text,
        "unsent"
    );
    assert!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("form")
            .pending
            .is_some()
    );
    w.creates.sketch.parameters.finished(
        address,
        false,
        w.scene.edit_source.as_ref(),
        "Cancelled; nothing was changed.",
    );
    assert!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("form")
            .pending
            .is_none()
    );
    assert_eq!(
        w.creates
            .sketch
            .parameters
            .draft
            .as_ref()
            .expect("form")
            .add_text,
        "unsent"
    );
}
