// SPDX-License-Identifier: MIT
//! §30Q: New's form opened over a tab's floating form share no
//! keyboard focus or text with it, either way — real egui widgets and input,
//! drawn where the window draws them, and the window's own `open_new`/`end_new`.

use super::*;
use crate::creates::Creates;
use crate::edits::Edits;
use crate::sessions::Sessions;
use crate::tabs::Tabs;
use ferritecad_jobs::HistoryLimits;
use ferritecad_ui::{NewChoice, NewContent, ViewportInput};

/// One frame of the window's form sections for the shown tab `key`: the tab's
/// forms under its own ids, then New's form, as `Live::draw` lays them out.
fn window_frame(
    ctx: &egui::Context,
    key: u64,
    creates: &mut Creates,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, NewChoice) {
    let mut choice = NewChoice::Waiting;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 900.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let (form, sketch) = creates.forms();
            crate::in_tab_scope(ui, "tab-forms", key, |ui| {
                sketch.draw(ui, false, "");
            });
            choice = ferritecad_ui::new_document_form(ui, form);
        },
    );
    output.textures_delta.clear();
    (output, choice)
}

fn press_at(ctx: &egui::Context, key: u64, creates: &mut Creates, at: egui::Pos2) -> NewChoice {
    window_frame(ctx, key, creates, vec![egui::Event::PointerMoved(at)]);
    let mut choice = NewChoice::Waiting;
    for pressed in [true, false] {
        let (_, answered) = window_frame(
            ctx,
            key,
            creates,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }],
        );
        if answered != NewChoice::Waiting {
            choice = answered;
        }
    }
    choice
}

fn typed(ctx: &egui::Context, key: u64, creates: &mut Creates, text: &str) {
    window_frame(ctx, key, creates, vec![egui::Event::Text(text.into())]);
}

#[test]
fn new_opened_over_a_floating_form_shares_no_focus_or_text_with_it() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("circle.fcad");
    analytic_apply::write_round(&path, [12.0, -7.0], &[10.0], 15.0, false);
    let mut sessions = Sessions::default();
    sessions.adopt(
        crate::sessions::test_open::open_in(root.path(), &path, HistoryLimits::default())
            .expect("session"),
    );
    let key = sessions.tab().key();
    let current = sessions.export_path().expect("shown");
    let reading = crate::sketch_facts::read(&current).expect("reading");
    let (mut tabs, mut edits, mut creates) =
        (Tabs::new(None), Edits::default(), Creates::default());
    let mut input = ViewportInput::new();
    assert!(creates.sketch.begin_circle_edit(
        &current,
        &reading,
        reading.circle_sketches[0].sketch
    ));
    let ctx = egui::Context::default();

    // A's floating Circle form takes the keyboard.
    window_frame(&ctx, key, &mut creates, vec![]);
    let (out, _) = window_frame(&ctx, key, &mut creates, vec![]);
    press_at(&ctx, key, &mut creates, text_at(&out, "12"));
    typed(&ctx, key, &mut creates, "5");
    let a_typed = creates.sketch.circle.clone();
    assert_ne!(a_typed.center[0], "12", "A's field took the keyboard");
    let a_history = (
        creates.sketch.circle_undo.clone(),
        creates.sketch.circle_redo.clone(),
    );
    let (out, _) = window_frame(&ctx, key, &mut creates, vec![]);
    assert!(!painted(&out, "Create sketch + Extrude…"));
    assert!(crate::open_new(
        &mut tabs,
        &mut sessions,
        &mut edits,
        &mut creates,
        &mut input,
        |creates, input| crate::ask_new(
            creates,
            &crate::Loads::default(),
            &crate::exports::Exports::default(),
            input
        ),
    ));
    assert!(tabs.has_aside() && !creates.sketch.active());
    creates.form().expect("New's form").content = NewContent::SamplePlate;
    let sizes = |creates: &mut Creates| {
        let form = creates.form().expect("New's form");
        [form.width.clone(), form.depth.clone(), form.height.clone()]
    };
    let before = sizes(&mut creates);
    for _ in 0..2 {
        window_frame(&ctx, key, &mut creates, vec![]);
    }
    typed(&ctx, key, &mut creates, "8");
    assert_eq!(sizes(&mut creates), before, "A's focus typed into New");
    let (out, _) = window_frame(&ctx, key, &mut creates, vec![]);
    press_at(&ctx, key, &mut creates, text_at(&out, &before[0]));
    typed(&ctx, key, &mut creates, "8");
    assert_ne!(sizes(&mut creates)[0], before[0], "New's own field");
    let (out, _) = window_frame(&ctx, key, &mut creates, vec![]);
    let choice = press_at(&ctx, key, &mut creates, text_at(&out, "Cancel"));
    assert_eq!(choice, NewChoice::Cancel);
    crate::creates::answer_form(&mut creates, &mut input, choice);
    crate::end_new(&mut tabs, &mut sessions, &mut edits, &mut creates);
    assert!(creates.form().is_none() && !tabs.has_aside());
    for _ in 0..2 {
        window_frame(&ctx, key, &mut creates, vec![]);
    }
    typed(&ctx, key, &mut creates, "4");
    assert_eq!(creates.sketch.circle, a_typed, "New's focus typed into A");
    assert_eq!(
        (
            creates.sketch.circle_undo.clone(),
            creates.sketch.circle_redo.clone()
        ),
        a_history
    );
}
