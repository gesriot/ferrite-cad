// SPDX-License-Identifier: MIT
//! §30T: the question asked when a file is chosen while New is not finished,
//! drawn beside New's own form as the window draws them — real egui widgets and
//! input, the window's `Creates` — headless, not a window. Both buttons answer,
//! the typed text survives the question and Back to New literally, the keyboard
//! stays with the field that had it, and Discard leaves no form to type into.

use super::*;
use crate::creates::Creates;
use ferritecad_ui::{
    BACK_TO_NEW, DISCARD_NEW_AND_OPEN, NewChoice, NewContent, OpenOverNewChoice, OpenOverNewPanel,
    ViewportInput,
};
use std::path::PathBuf;

/// One frame of the window's New section: the tab's forms, then New's form with the
/// question under it when `ask` is set (`file`, `can_discard`) — the same
/// `new_document_section` `Live::draw` calls.
fn window_frame(
    ctx: &egui::Context,
    creates: &mut Creates,
    ask: Option<(&str, bool)>,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, OpenOverNewChoice, NewChoice) {
    let (mut over, mut asked) = (OpenOverNewChoice::Waiting, NewChoice::Waiting);
    let losing = creates.asking().map(|(_, losing)| losing);
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
            crate::in_tab_scope(ui, "tab-forms", 1, |ui| sketch.draw(ui, true, false, ""));
            (asked, over) = ferritecad_ui::new_document_section(
                ui,
                form,
                ask.zip(losing)
                    .map(|((file, can_discard), losing)| OpenOverNewPanel {
                        file,
                        losing,
                        can_discard,
                    }),
            );
        },
    );
    output.textures_delta.clear();
    (output, over, asked)
}

/// Frames with no input until the layout, which egui settles over a few, is still.
fn settled(
    ctx: &egui::Context,
    creates: &mut Creates,
    ask: Option<(&str, bool)>,
) -> egui::FullOutput {
    for _ in 0..3 {
        window_frame(ctx, creates, ask, vec![]);
    }
    window_frame(ctx, creates, ask, vec![]).0
}

fn pointer(ctx: &egui::Context, creates: &mut Creates, ask: Option<(&str, bool)>, at: egui::Pos2) {
    window_frame(ctx, creates, ask, vec![egui::Event::PointerMoved(at)]);
}

/// A press and release at `at`; what the question answered on either frame.
fn press_at(
    ctx: &egui::Context,
    creates: &mut Creates,
    ask: Option<(&str, bool)>,
    at: egui::Pos2,
) -> OpenOverNewChoice {
    pointer(ctx, creates, ask, at);
    let mut answer = OpenOverNewChoice::Waiting;
    for pressed in [true, false] {
        let (_, over, _) = window_frame(
            ctx,
            creates,
            ask,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }],
        );
        if over != OpenOverNewChoice::Waiting {
            answer = over;
        }
    }
    answer
}

fn type_text(ctx: &egui::Context, creates: &mut Creates, ask: Option<(&str, bool)>, text: &str) {
    window_frame(ctx, creates, ask, vec![egui::Event::Text(text.into())]);
}

/// Replaces the content of the box showing `shown` (select all, then type).
fn replace_in(
    ctx: &egui::Context,
    creates: &mut Creates,
    ask: Option<(&str, bool)>,
    shown: &str,
    text: &str,
) {
    let out = settled(ctx, creates, ask);
    press_at(ctx, creates, ask, text_at(&out, shown));
    window_frame(
        ctx,
        creates,
        ask,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            },
            egui::Event::Text(text.into()),
        ],
    );
}

fn sizes(creates: &mut Creates) -> [String; 3] {
    let form = creates.form().expect("New's form");
    [form.width.clone(), form.depth.clone(), form.height.clone()]
}

fn painted(output: &egui::FullOutput, label: &str) -> bool {
    output
        .shapes
        .iter()
        .any(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text() == label))
}

/// Real widgets: Width is typed into (even a text that is not a number), the
/// question appears while Width has the keyboard and takes none of it, Back to New
/// withdraws it with every box literally as typed, and Width takes typing again.
/// Discard answers only while the window says it may, ends New's form, and leaves
/// nothing for the keyboard to reach.
#[test]
fn the_question_beside_new_keeps_its_text_and_focus_and_both_buttons_answer() {
    let file = "b.fcad";
    let mut creates = Creates::default();
    let mut input = ViewportInput::new();
    assert!(crate::creates::open_form(&mut creates, &mut input));
    {
        let form = creates.form().expect("New's form");
        form.content = NewContent::SamplePlate;
        form.width = "12".to_owned();
        form.depth = " 33.0 ".to_owned();
        form.height = "".to_owned();
    }
    let ctx = egui::Context::default();

    // Width takes the keyboard, with a text that is not a number.
    window_frame(&ctx, &mut creates, None, vec![]);
    replace_in(&ctx, &mut creates, None, "12", "1e9");
    assert_eq!(sizes(&mut creates), ["1e9", " 33.0 ", ""]);

    // The question appears while Width has the keyboard: it takes none of it.
    assert!(creates.ask_open(PathBuf::from(file), &mut input));
    let ask = Some((file, true));
    let (out, over, _) = window_frame(&ctx, &mut creates, ask, vec![]);
    assert_eq!(over, OpenOverNewChoice::Waiting);
    assert!(painted(&out, DISCARD_NEW_AND_OPEN) && painted(&out, BACK_TO_NEW));
    type_text(&ctx, &mut creates, ask, "y");
    assert_eq!(sizes(&mut creates), ["1e9y", " 33.0 ", ""], "focus stayed");

    // Back to New: the boxes are literally as typed; the question is gone.
    let out = settled(&ctx, &mut creates, ask);
    let back = press_at(&ctx, &mut creates, ask, text_at(&out, BACK_TO_NEW));
    assert_eq!(back, OpenOverNewChoice::Back);
    creates.keep_new(&mut input);
    assert_eq!(sizes(&mut creates), ["1e9y", " 33.0 ", ""]);
    let (out, over, _) = window_frame(&ctx, &mut creates, None, vec![]);
    assert!(!painted(&out, DISCARD_NEW_AND_OPEN) && over == OpenOverNewChoice::Waiting);
    // A click elsewhere took the keyboard off Width, as any click does: typing
    // without a click changes nothing; a click into Width makes it editable again.
    type_text(&ctx, &mut creates, None, "q");
    assert_eq!(sizes(&mut creates), ["1e9y", " 33.0 ", ""]);
    let out = settled(&ctx, &mut creates, None);
    press_at(&ctx, &mut creates, None, text_at(&out, "1e9y"));
    type_text(&ctx, &mut creates, None, "z");
    let [width, depth, height] = sizes(&mut creates);
    assert_eq!(
        (width.len(), depth.as_str(), height.as_str()),
        (5, " 33.0 ", "")
    );
    assert!(width.contains('z'));

    // Discard, held back: greyed, answers nothing, New is as it was.
    assert!(creates.ask_open(PathBuf::from(file), &mut input));
    let held = Some((file, false));
    let out = settled(&ctx, &mut creates, held);
    let answered = press_at(
        &ctx,
        &mut creates,
        held,
        text_at(&out, DISCARD_NEW_AND_OPEN),
    );
    assert_eq!(answered, OpenOverNewChoice::Waiting);
    assert_eq!(
        sizes(&mut creates),
        [width.clone(), depth.clone(), height.clone()]
    );
    assert!(creates.asking().is_some());

    // Discard, offered: it answers, and the window ends New's form with it.
    let ask = Some((file, true));
    let out = settled(&ctx, &mut creates, ask);
    let answered = press_at(&ctx, &mut creates, ask, text_at(&out, DISCARD_NEW_AND_OPEN));
    assert_eq!(answered, OpenOverNewChoice::Discard);
    assert_eq!(creates.stop_new(&mut input), Some(PathBuf::from(file)));
    let (out, over, asked) = window_frame(&ctx, &mut creates, None, vec![]);
    assert!(!painted(&out, &width) && !painted(&out, DISCARD_NEW_AND_OPEN));
    assert_eq!(
        (over, asked),
        (OpenOverNewChoice::Waiting, NewChoice::Waiting)
    );
    type_text(&ctx, &mut creates, None, "typed into nothing");
    assert!(creates.form().is_none() && !creates.making_new());
    println!("\nFCAD_30T_QUESTION_WIDGETS_EXECUTED");
}
