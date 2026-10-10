// SPDX-License-Identifier: MIT
//! The question asked when a file is chosen while New is not finished (§30T). Words
//! and two buttons only: whether New can be given up now, and what giving it up
//! does, belong to the window.

/// The question's heading.
pub const OPEN_OVER_NEW_HEADING: &str = "New is not finished";
/// Gives New up, throwing away what it holds, and opens the chosen file.
pub const DISCARD_NEW_AND_OPEN: &str = "Discard New and open";
/// Withdraws the question; New is exactly as it was.
pub const BACK_TO_NEW: &str = "Back to New";

/// What the question shows this frame.
#[derive(Debug, Clone, Copy)]
pub struct OpenOverNewPanel<'a> {
    /// The chosen file's name.
    pub file: &'a str,
    /// What giving New up throws away, as a clause: "the sizes you typed will be
    /// discarded".
    pub losing: &'a str,
    /// Whether the discard may be pressed now: the window's own answer to whether
    /// Open may start, the one its handler asks again.
    pub can_discard: bool,
}

/// What was pressed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenOverNewChoice {
    #[default]
    Waiting,
    /// Give New up and open the file.
    Discard,
    /// Keep working on New.
    Back,
}

/// Draws the question, when there is one. Nothing is drawn, and nothing is decided,
/// on a frame in which the person pressed neither button.
pub fn open_over_new_panel(
    ui: &mut egui::Ui,
    panel: Option<OpenOverNewPanel<'_>>,
) -> OpenOverNewChoice {
    let Some(panel) = panel else {
        return OpenOverNewChoice::Waiting;
    };
    let mut choice = OpenOverNewChoice::Waiting;
    ui.strong(OPEN_OVER_NEW_HEADING);
    ui.add(
        egui::Label::new(format!(
            "You chose {} while a new document is still being prepared. Opening it means \
             giving up the new document: {}. Documents that are already open, with their \
             unsaved changes, are not touched.",
            panel.file, panel.losing
        ))
        .wrap(),
    );
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(panel.can_discard, egui::Button::new(DISCARD_NEW_AND_OPEN))
            .clicked()
        {
            choice = OpenOverNewChoice::Discard;
        }
        if ui.button(BACK_TO_NEW).clicked() {
            choice = OpenOverNewChoice::Back;
        }
    });
    if !panel.can_discard {
        ui.small("New can be given up once the current operation has finished.");
    }
    ui.separator();
    choice
}

/// New's form with the question under it, in the order the window draws them.
///
/// Under, not above: egui numbers a widget by what is drawn before it in its
/// parent, so a panel that came and went above the form would take the keyboard
/// off the box a person is typing in the moment it appeared. A real-widget gate
/// in the application holds it.
pub fn new_document_section(
    ui: &mut egui::Ui,
    form: Option<&mut crate::NewDocumentForm>,
    question: Option<OpenOverNewPanel<'_>>,
) -> (crate::NewChoice, OpenOverNewChoice) {
    let asked = crate::new_document_form(ui, form);
    (asked, open_over_new_panel(ui, question))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
    }

    fn texts(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    fn frame(
        context: &egui::Context,
        panel: Option<OpenOverNewPanel<'_>>,
        at: Option<egui::Pos2>,
    ) -> (OpenOverNewChoice, egui::FullOutput) {
        let events = at.map_or_else(Vec::new, |at| {
            let mut events = vec![egui::Event::PointerMoved(at)];
            events.extend([true, false].map(|pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            }));
            events
        });
        let raw = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut choice = OpenOverNewChoice::Waiting;
        let mut output = context.run_ui(raw, |ui| choice = open_over_new_panel(ui, panel));
        output.textures_delta.clear();
        (choice, output)
    }

    /// The question names the file and what is lost, both buttons answer, and the
    /// discard answers only while the window says it may.
    #[test]
    fn the_question_says_what_is_lost_and_both_buttons_answer() {
        let panel = |can_discard| OpenOverNewPanel {
            file: "b.fcad",
            losing: "the sizes you typed will be discarded",
            can_discard,
        };
        let context = egui::Context::default();
        let (_, output) = frame(&context, Some(panel(true)), None);
        let said = texts(&output);
        assert!(said.iter().any(|t| t == OPEN_OVER_NEW_HEADING));
        assert!(
            said.iter().any(|t| t.contains("b.fcad")
                && t.contains("the sizes you typed will be discarded")
                && t.contains("not touched")),
            "{said:?}"
        );
        let discard = centre(&output, DISCARD_NEW_AND_OPEN).expect("Discard is drawn");
        let back = centre(&output, BACK_TO_NEW).expect("Back is drawn");
        assert_eq!(
            frame(&context, Some(panel(true)), Some(back)).0,
            OpenOverNewChoice::Back
        );
        assert_eq!(
            frame(&context, Some(panel(true)), Some(discard)).0,
            OpenOverNewChoice::Discard
        );

        // Held back: the discard is greyed and says so; Back still works.
        let context = egui::Context::default();
        let (_, output) = frame(&context, Some(panel(false)), None);
        assert!(
            texts(&output)
                .iter()
                .any(|t| t.contains("once the current operation has finished"))
        );
        let discard = centre(&output, DISCARD_NEW_AND_OPEN).expect("Discard is drawn");
        let back = centre(&output, BACK_TO_NEW).expect("Back is drawn");
        assert_eq!(
            frame(&context, Some(panel(false)), Some(discard)).0,
            OpenOverNewChoice::Waiting
        );
        assert_eq!(
            frame(&context, Some(panel(false)), Some(back)).0,
            OpenOverNewChoice::Back
        );

        // Nothing to ask: nothing drawn, nothing decided.
        let (choice, output) = frame(&context, None, None);
        assert_eq!(choice, OpenOverNewChoice::Waiting);
        assert!(texts(&output).is_empty());
    }
}
