// SPDX-License-Identifier: MIT
#![allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
use super::*;
mod curve_edits;
mod native;
mod rollback;
mod widgets;

#[test]
fn a_producer_requires_one_complete_meaning_not_one_distinct_producer() {
    let meaning = ferritecad_scene::EntityMeaning {
        feature: ObjectId::new(),
        producer: ObjectId::new(),
        kind: ferritecad_kernel::SubShapeKind::Face,
        digest: ferritecad_types::ContentHash::of_bytes(b"named face"),
        description: "face of the original feature".into(),
    };
    assert_eq!(
        producer(&[]),
        Err("The selected entity has no stored meaning; the tree selection is unchanged.".into())
    );
    assert_eq!(
        producer(std::slice::from_ref(&meaning)),
        Ok(meaning.producer)
    );
    let mut second = meaning.clone();
    second.digest = ferritecad_types::ContentHash::of_bytes(b"another meaning");
    assert_eq!(
        producer(&[meaning, second]),
        Err("The selected entity has several meanings; the tree selection is unchanged.".into())
    );
}

#[test]
fn every_command_shares_idle_refusals_and_states_its_object_requirement() {
    let (_root, _path, mut source) = crate::cuts::tests::session_apply::fixture(1);
    let saved = crate::cuts::tests::session_apply::selected(&source, 0);
    let feature = saved.feature;
    let body = saved.body;
    let sketch = saved.tool_sketch;
    let key = (feature, Some(body), None);
    let commands = [
        Command::Edit(ModelEdit::Cut),
        Command::Delete,
        Command::DeleteWithDependents,
        Command::Suppress,
        Command::Unsuppress,
        Command::Rollback,
        Command::RollToEnd,
    ];
    for command in commands {
        for reason in [
            "Apply or Cancel the open form before opening another editor.",
            "Finish the current operation, New, dialog or gesture before editing.",
            "Delete or Cancel the pending deletion before another command.",
        ] {
            assert_eq!(
                command.available(&source, key, Err(reason)),
                Err(reason.into())
            );
        }
    }
    for command in [Command::Delete, Command::DeleteWithDependents] {
        for object in [feature, body, sketch] {
            let row = source
                .model_tree
                .rows
                .iter()
                .find(|r| r.id.0 == object)
                .expect("row");
            assert_eq!(command.available(&source, row.id, Ok(Access::Idle)), Ok(()));
        }
    }
    assert_eq!(
        Command::Edit(ModelEdit::Cut).available(&source, key, Ok(Access::Idle)),
        Ok(())
    );
    assert_eq!(
        Command::Suppress.available(&source, key, Ok(Access::Idle)),
        Ok(())
    );
    let row_index = source
        .model_tree
        .rows
        .iter()
        .position(|r| r.id == key)
        .expect("feature row");
    source.model_tree.rows[row_index].id = (feature, None, None);
    assert_eq!(
        Command::Suppress.available(&source, (feature, None, None), Ok(Access::Idle)),
        Err("Only a feature in a Body history can be suppressed.".into())
    );
    source.model_tree.rows[row_index].id = key;
    assert_eq!(
        Command::Unsuppress.available(&source, key, Ok(Access::Idle)),
        Err("This feature is not suppressed.".into())
    );
    for object in [body, sketch] {
        let row = source
            .model_tree
            .rows
            .iter()
            .find(|r| r.id.0 == object)
            .expect("row");
        for command in [Command::Suppress, Command::Unsuppress] {
            assert_eq!(
                command.available(&source, row.id, Ok(Access::Idle)),
                Err("Only a feature in a Body history can be suppressed.".into())
            );
        }
        assert_eq!(
            Command::Edit(ModelEdit::Cut).available(&source, row.id, Ok(Access::Idle)),
            Err("This editor is unavailable for the selected object.".into())
        );
    }
    source
        .objects
        .iter_mut()
        .find(|o| o.id == feature)
        .expect("feature")
        .suppressed = true;
    assert_eq!(
        Command::Unsuppress.available(&source, key, Ok(Access::Idle)),
        Ok(())
    );
    assert_eq!(
        Command::Suppress.available(&source, key, Ok(Access::Idle)),
        Err("This feature is already suppressed.".into())
    );
    let missing = (ObjectId::new(), None, None);
    assert_eq!(
        Command::Delete.available(&source, missing, Ok(Access::Idle)),
        Err("This object is no longer in the displayed model.".into())
    );
    source.refusal = Some("The document is unavailable.".into());
    assert_eq!(
        Command::Delete.available(&source, key, Ok(Access::Idle)),
        Err("The document is unavailable.".into())
    );
    source.refusal = None;
    source.objects.retain(|o| o.id != feature);
    assert_eq!(
        Command::Delete.available(&source, key, Ok(Access::Idle)),
        Err("This object is no longer in the document.".into())
    );
    source.refusal = Some("The document is unavailable.".into());
    assert_eq!(
        Command::Delete.available(&source, key, Ok(Access::Idle)),
        Err("The document is unavailable.".into())
    );
}

#[test]
fn a_pending_confirmation_uses_the_same_idle_gate_as_an_open_editor() {
    assert_eq!(
        availability(
            &Creates::default(),
            &Edits::default(),
            &Sessions::default(),
            &Tabs::new(None),
            &Loads::default(),
            &Exports::default(),
            &ferritecad_ui::ViewportInput::new(),
            true
        ),
        Err("Delete or Cancel the pending deletion before another command.")
    );
}

#[test]
fn confirmation_cancellation_sends_nothing_and_stale_confirmation_refuses() {
    let (_root, path, source) = crate::cuts::tests::session_apply::fixture(1);
    let saved = crate::cuts::tests::session_apply::selected(&source, 0);
    let row = (saved.feature, Some(saved.body), None);
    let sessions = Sessions::default();
    let mut tabs = Tabs::new(None);
    let navigation = tabs.model_navigation(sessions.tab());
    navigation.selected = Some(row);
    let action = Action {
        tab: sessions.tab().key(),
        epoch: navigation.epoch,
        row,
        edit: Command::DeleteWithDependents,
    };
    let confirmation = || Confirmation::read(action, &path, source.version).expect("closure");
    assert!(
        confirmation()
            .finish(false, None, &sessions, &tabs, Err("Busy"))
            .expect("Cancel needs no live source")
            .is_none()
    );
    assert_eq!(
        confirmation()
            .finish(true, None, &sessions, &tabs, Ok(Access::Idle))
            .expect_err("lost document"),
        "The deletion belongs to an older document view."
    );
    assert_eq!(
        confirmation()
            .finish(true, Some(&source), &sessions, &tabs, Err("Busy"))
            .expect_err("one availability gate"),
        "Busy"
    );
    assert_eq!(
        confirmation()
            .finish(true, Some(&source), &sessions, &tabs, Ok(Access::Idle))
            .expect_err("no accepted version"),
        "The command belongs to an older document view."
    );
}

#[test]
fn catalogue_carries_the_same_labels_shortcuts_and_reasons_for_each_surface() {
    let (_root, _path, source) = crate::cuts::tests::session_apply::fixture(1);
    let saved = crate::cuts::tests::session_apply::selected(&source, 0);
    let key = (saved.feature, Some(saved.body), None);
    let offered = offers(&source, key, Ok(Access::Idle));
    let expected: Vec<_> = actions(&source, key)
        .into_iter()
        .map(Command::Edit)
        .chain([
            Command::Delete,
            Command::DeleteWithDependents,
            Command::Suppress,
            Command::Unsuppress,
            Command::Rollback,
            Command::RollToEnd,
        ])
        .collect();
    assert_eq!(
        offered.iter().map(|o| o.command).collect::<Vec<_>>(),
        expected
    );
    for offer in &offered {
        assert_eq!(offer.label, offer.command.label());
        assert_eq!(offer.shortcuts, offer.command.shortcuts());
        assert_eq!(
            offer.available,
            offer.command.available(&source, key, Ok(Access::Idle))
        );
    }
    assert_eq!(
        Command::Delete.shortcuts(),
        &[
            egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete),
            egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Backspace),
        ]
    );
}

#[test]
fn rollback_catalogue_uses_the_body_marker_and_preserves_every_idle_guard() {
    let (_root, path, _) = crate::cuts::tests::session_apply::fixture(2);
    let mut doc = Document::open(&path).expect("document");
    let source: ExtrudeEditSource = ferritecad_document::ExtrudeEditSource::read(&doc)
        .expect("source")
        .into();
    let cut = crate::cuts::tests::session_apply::selected(&source, 0);
    let body_id = cut.body;
    let rows: Vec<_> = source
        .model_tree
        .rows
        .iter()
        .filter(|r| r.id.1 == Some(body_id) && r.id.2.is_none())
        .map(|r| r.id)
        .collect();
    assert_eq!(rows.len(), 3);
    let base = rows[0];
    let middle = rows[1];
    let tip = rows[2];
    let body = (body_id, None, None);
    for row in [base, middle, tip, body] {
        assert_eq!(
            Command::RollToEnd.available(&source, row, Ok(Access::Idle)),
            Err("This Body is not rolled back.".into())
        );
        for command in [Command::Rollback, Command::RollToEnd] {
            assert!(command.shortcuts().is_empty());
            for reason in [
                "Apply or Cancel the open form before opening another editor.",
                "Finish the current operation, New, dialog or gesture before editing.",
                "Delete or Cancel the pending deletion before another command.",
            ] {
                assert_eq!(
                    command.available(&source, row, Err(reason)),
                    Err(reason.into())
                );
            }
            assert_eq!(
                command.available(&source, row, Ok(Access::Parameters { operation: Ok(()) })),
                Err("Close the Parameters form before another command.".into())
            );
        }
    }
    assert_eq!(
        Command::Rollback.available(&source, tip, Ok(Access::Idle)),
        Err("The Body already shows this feature.".into())
    );
    for row in source
        .model_tree
        .rows
        .iter()
        .filter(|r| r.id.1.is_none() || r.id.2.is_some())
    {
        assert_eq!(
            Command::Rollback.available(&source, row.id, Ok(Access::Idle)),
            Err("Only a feature in a Body history can be rolled back to.".into())
        );
        if row.id != body {
            assert_eq!(
                Command::RollToEnd.available(&source, row.id, Ok(Access::Idle)),
                Err("Only a Body or a feature in its history can roll to the end.".into())
            );
        }
    }
    assert_eq!(
        Command::Rollback.available(&source, middle, Ok(Access::Idle)),
        Ok(())
    );
    doc.write_changes(&[EditChange::SetRollback {
        object: body_id,
        feature: Some(middle.0),
    }])
    .expect("marker");
    let source: ExtrudeEditSource = ferritecad_document::ExtrudeEditSource::read(&doc)
        .expect("source")
        .into();
    assert_eq!(
        Command::Rollback.available(&source, middle, Ok(Access::Idle)),
        Err("The Body already shows this feature.".into())
    );
    assert_eq!(
        Command::Rollback.available(&source, tip, Ok(Access::Idle)),
        Err("This feature ends the history; use Roll to end.".into())
    );
    assert_eq!(
        Command::Rollback.available(&source, base, Ok(Access::Idle)),
        Ok(())
    );
    for row in [base, middle, tip, body] {
        assert_eq!(
            Command::RollToEnd.available(&source, row, Ok(Access::Idle)),
            Ok(())
        );
        for offer in offers(&source, row, Ok(Access::Idle)) {
            assert_eq!(
                offer.available,
                offer.command.available(&source, row, Ok(Access::Idle))
            );
        }
    }
}
