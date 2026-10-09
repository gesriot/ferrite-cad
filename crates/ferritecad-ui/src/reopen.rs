// SPDX-License-Identifier: MIT
//! The start-up offer to reopen the saved files of the last window (§30R). Words
//! and buttons only: which files there were, whether they may be reopened now and
//! what reopening does belong to the window.

/// The section's heading.
pub const REOPEN_HEADING: &str = "Files open when FerriteCAD was last quit";
/// Reopens the listed files, as saved on disk now.
pub const REOPEN: &str = "Reopen saved files";
/// Hides the offer for this run; nothing is opened or removed.
pub const NOT_NOW: &str = "Not now";

/// One listed file, in the words a person reads.
#[derive(Debug, Clone, Copy)]
pub struct ReopenRow<'a> {
    /// The file's name.
    pub name: &'a str,
    /// The folder it is in.
    pub folder: &'a str,
    /// It was the one shown.
    pub shown: bool,
}

/// What the section shows this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReopenPanel<'a> {
    /// The files offered; empty hides the offer.
    pub rows: &'a [ReopenRow<'a>],
    /// Why the list was not used, when it was found and refused.
    pub refused: Option<&'a str>,
    /// Whether Reopen may be pressed now (nothing else is reading or replacing a
    /// document).
    pub can_act: bool,
    /// A Reopen is in progress: Not now waits for it (Cancel stops it).
    pub running: bool,
    /// How the last Reopen went, then each file it did not open.
    pub report: &'a [String],
}

/// What was pressed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ReopenChoice {
    #[default]
    Waiting,
    Reopen,
    NotNow,
}

/// Draws the offer (when there is anything to say) and the last Reopen's account.
pub fn reopen_panel(ui: &mut egui::Ui, panel: ReopenPanel<'_>) -> ReopenChoice {
    let mut choice = ReopenChoice::Waiting;
    if panel.rows.is_empty() && panel.refused.is_none() && panel.report.is_empty() {
        return choice;
    }
    ui.separator();
    if !panel.rows.is_empty() {
        ui.strong(REOPEN_HEADING);
        ui.add(
            egui::Label::new(
                "Reopening opens each file as it is saved on disk now. Changes that were \
                 discarded or never saved are not part of it; work a crash interrupted is \
                 offered under Recover unsaved work.",
            )
            .wrap(),
        );
        for row in panel.rows {
            let shown = if row.shown { " (shown)" } else { "" };
            ui.add(egui::Label::new(format!("{}{shown} — {}", row.name, row.folder)).wrap());
        }
    }
    if let Some(refused) = panel.refused {
        ui.colored_label(ui.visuals().error_fg_color, refused);
    }
    for line in panel.report {
        ui.add(egui::Label::new(line.as_str()).wrap());
    }
    ui.horizontal_wrapped(|ui| {
        if !panel.rows.is_empty()
            && ui
                .add_enabled(panel.can_act, egui::Button::new(REOPEN))
                .clicked()
        {
            choice = ReopenChoice::Reopen;
        }
        if !panel.running && ui.button(NOT_NOW).clicked() {
            choice = ReopenChoice::NotNow;
        }
    });
    choice
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
        panel: ReopenPanel<'_>,
        at: Option<egui::Pos2>,
    ) -> (ReopenChoice, egui::FullOutput) {
        let events = at.map_or_else(Vec::new, |at| {
            vec![
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
            ]
        });
        let raw = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut choice = ReopenChoice::Waiting;
        let mut output = context.run_ui(raw, |ui| choice = reopen_panel(ui, panel));
        output.textures_delta.clear();
        (choice, output)
    }

    /// The offer says that saved files open as they are on disk, names each file
    /// and the shown one, and its buttons answer only when the window says they
    /// may; a refused list and a report are said even with nothing to offer.
    #[test]
    fn reopen_asks_nothing_while_the_window_says_it_cannot_and_says_what_opens() {
        let rows = [
            ReopenRow {
                name: "a.fcad",
                folder: "/work",
                shown: false,
            },
            ReopenRow {
                name: "b.fcad",
                folder: "/work",
                shown: true,
            },
        ];
        let report = vec!["b.fcad — not opened: missing".to_owned()];
        for (can_act, running) in [(false, false), (true, false), (false, true)] {
            let panel = ReopenPanel {
                rows: &rows,
                refused: None,
                can_act,
                running,
                report: &report,
            };
            let context = egui::Context::default();
            let (_, output) = frame(&context, panel, None);
            let said = texts(&output);
            assert!(said.iter().any(|t| t == REOPEN_HEADING));
            assert!(
                said.iter()
                    .any(|t| t.contains("as it is saved on disk now")),
                "{said:?}"
            );
            assert!(said.iter().any(|t| t == "b.fcad (shown) — /work"));
            assert!(said.iter().any(|t| t == "b.fcad — not opened: missing"));
            let reopen = centre(&output, REOPEN).expect("Reopen is drawn");
            let expected = if can_act {
                ReopenChoice::Reopen
            } else {
                ReopenChoice::Waiting
            };
            assert_eq!(frame(&context, panel, Some(reopen)).0, expected);
            // Not now waits while a Reopen runs (Cancel stops it); otherwise it is
            // always offered: it only hides.
            match centre(&output, NOT_NOW) {
                Some(later) => {
                    assert!(!running);
                    assert_eq!(frame(&context, panel, Some(later)).0, ReopenChoice::NotNow);
                }
                None => assert!(running),
            }
        }
        // A list that was refused is said, without an offer to reopen it.
        let context = egui::Context::default();
        let panel = ReopenPanel {
            refused: Some("The list was not used (damaged)."),
            ..ReopenPanel::default()
        };
        let (_, output) = frame(&context, panel, None);
        assert!(centre(&output, REOPEN).is_none() && centre(&output, NOT_NOW).is_some());
        assert!(texts(&output).iter().any(|t| t.contains("(damaged)")));
        // Nothing to say: nothing drawn.
        let (_, output) = frame(&context, ReopenPanel::default(), None);
        assert!(texts(&output).is_empty());
    }
}
