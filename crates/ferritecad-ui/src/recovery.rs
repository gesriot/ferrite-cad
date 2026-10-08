// SPDX-License-Identifier: MIT
//! The start-up list of crash copies (§30M) and the line about the open
//! document's own copy. Words and buttons only: what a record is, whether it can
//! be recovered and what recovering it does belong to the window and the
//! library behind it.

/// The section's heading.
pub const RECOVER_HEADING: &str = "Recover unsaved work";
/// Opens the copy as a new unsaved document.
pub const RECOVER: &str = "Recover";
/// Removes the copy, after a question.
pub const DELETE_RECOVERY: &str = "Delete…";
/// Hides the list for this run; nothing is removed.
pub const LATER: &str = "Later";

/// One copy a person may recover, in the words they read.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryOffer<'a> {
    /// What the document was called. Never a path.
    pub name: &'a str,
    /// When the copy was confirmed written, as a person reads a time.
    pub written: &'a str,
}

/// What the section shows this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct RecoveryPanel<'a> {
    /// Copies that can be recovered; empty hides the list.
    pub offers: &'a [RecoveryOffer<'a>],
    /// Records that were found and refused, one sentence each.
    pub refused: &'a [String],
    /// Whether Recover and Delete may be pressed now (nothing else is replacing
    /// or reading the document).
    pub can_act: bool,
    /// What the last recovery did, or what is happening.
    pub outcome: Option<&'a str>,
    /// The open document's own crash copy: written, being written, or failed.
    pub line: Option<&'a str>,
}

/// What was pressed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryChoice {
    #[default]
    Waiting,
    Recover(usize),
    Delete(usize),
    Later,
}

/// Draws the list (when there is anything in it) and the copy line.
pub fn recovery_panel(ui: &mut egui::Ui, panel: RecoveryPanel<'_>) -> RecoveryChoice {
    let mut choice = RecoveryChoice::Waiting;
    if !panel.offers.is_empty() || !panel.refused.is_empty() {
        ui.separator();
        ui.strong(RECOVER_HEADING);
        if !panel.offers.is_empty() {
            ui.add(
                egui::Label::new(
                    "FerriteCAD ended without these documents being saved. Recovering one \
                     opens its last recovery copy as a new unsaved document; your files are \
                     not changed.",
                )
                .wrap(),
            );
        }
        for (index, offer) in panel.offers.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{} — copy written {}", offer.name, offer.written));
                if ui
                    .add_enabled(panel.can_act, egui::Button::new(RECOVER))
                    .clicked()
                {
                    choice = RecoveryChoice::Recover(index);
                }
                if ui
                    .add_enabled(panel.can_act, egui::Button::new(DELETE_RECOVERY))
                    .clicked()
                {
                    choice = RecoveryChoice::Delete(index);
                }
            });
        }
        for refused in panel.refused {
            ui.colored_label(ui.visuals().error_fg_color, refused);
        }
        if ui.button(LATER).clicked() {
            choice = RecoveryChoice::Later;
        }
    }
    if let Some(outcome) = panel.outcome {
        ui.add(egui::Label::new(outcome).wrap());
    }
    if let Some(line) = panel.line {
        ui.add(egui::Label::new(line).wrap());
    }
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("the label was drawn")
    }

    fn click(context: &egui::Context, panel: RecoveryPanel<'_>, at: egui::Pos2) -> RecoveryChoice {
        let raw = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };
        let mut choice = RecoveryChoice::Waiting;
        let mut output = context.run_ui(raw, |ui| choice = recovery_panel(ui, panel));
        output.textures_delta.clear();
        choice
    }

    #[test]
    fn recover_and_delete_ask_nothing_while_the_window_says_they_cannot() {
        let offers = [
            RecoveryOffer {
                name: "plate.fcad",
                written: "2026-10-08 07:12:33 UTC",
            },
            RecoveryOffer {
                name: "Untitled",
                written: "2026-10-08 07:13:00 UTC",
            },
        ];
        let refused = vec!["A recovery copy could not be read.".to_owned()];
        for can_act in [false, true] {
            let panel = RecoveryPanel {
                offers: &offers,
                refused: &refused,
                can_act,
                outcome: None,
                line: Some("Recovery copy written 2026-10-08 07:14:00 UTC"),
            };
            let context = egui::Context::default();
            let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                recovery_panel(ui, panel);
            });
            output.textures_delta.clear();
            assert!(
                output.shapes.iter().any(|clipped| matches!(&clipped.shape,
                    egui::Shape::Text(text) if text.galley.text().contains("plate.fcad — copy written"))),
                "the name and confirmed time are shown"
            );
            let recover = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == RECOVER => {
                        Some(text.visual_bounding_rect().center())
                    }
                    _ => None,
                })
                .nth(1)
                .expect("the second row's Recover");
            let delete = text_center(&output, DELETE_RECOVERY);
            let later = text_center(&output, LATER);
            let expected = |choice| {
                if can_act {
                    choice
                } else {
                    RecoveryChoice::Waiting
                }
            };
            assert_eq!(
                click(&context, panel, recover),
                expected(RecoveryChoice::Recover(1))
            );
            assert_eq!(
                click(&context, panel, delete),
                expected(RecoveryChoice::Delete(0))
            );
            // Later only hides the list, so it is always offered.
            assert_eq!(click(&context, panel, later), RecoveryChoice::Later);
        }
        // Nothing to offer: no heading, no buttons, only the line.
        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            recovery_panel(ui, RecoveryPanel::default());
        });
        output.textures_delta.clear();
        assert!(!output.shapes.iter().any(|clipped| matches!(&clipped.shape,
            egui::Shape::Text(text) if text.galley.text() == RECOVER_HEADING)));
    }
}
