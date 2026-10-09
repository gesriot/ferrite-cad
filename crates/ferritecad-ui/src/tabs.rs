// SPDX-License-Identifier: MIT
//! The open documents of the window as a row of tabs (§30O): a name, a `*` while
//! it has unsaved changes, which one is shown, and a close button each. Words and
//! buttons only: what a tab holds, whether it may be shown or closed and what
//! closing asks belong to the window.

/// The close button of every tab.
pub const CLOSE_TAB: &str = "×";

/// One tab, in the words a person reads.
#[derive(Debug, Clone, Copy)]
pub struct TabLabel<'a> {
    /// The window's identity of the tab, handed back when it is pressed. Never a
    /// position: a tab closed in between would shift every position after it.
    pub key: u64,
    /// The document's name (never a path, never a private file).
    pub name: &'a str,
    pub dirty: bool,
    /// The tab whose document is on screen.
    pub active: bool,
}

/// What the row shows this frame.
#[derive(Debug, Clone, Copy)]
pub struct TabStrip<'a> {
    pub tabs: &'a [TabLabel<'a>],
    /// `Err` holds why another tab cannot be shown or a tab closed now.
    pub available: Result<(), &'a str>,
}

/// What was pressed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TabChoice {
    #[default]
    Waiting,
    /// Show this tab's document.
    Show(u64),
    /// Close this tab (the window asks first when it has unsaved changes).
    Close(u64),
}

/// The text of one tab: its name, marked while unsaved.
pub fn tab_title(label: &TabLabel<'_>) -> String {
    if label.dirty {
        format!("*{}", label.name)
    } else {
        label.name.to_owned()
    }
}

/// Draws the row. Nothing is drawn while no document is open.
pub fn tab_strip(ui: &mut egui::Ui, strip: TabStrip<'_>) -> TabChoice {
    let mut choice = TabChoice::Waiting;
    if strip.tabs.is_empty() {
        return choice;
    }
    let enabled = strip.available.is_ok();
    ui.horizontal_wrapped(|ui| {
        for label in strip.tabs {
            ui.group(|ui| {
                let title = tab_title(label);
                // The shown tab is selected and is not a button to press again.
                let tab = ui.add_enabled(
                    enabled || label.active,
                    egui::Button::selectable(label.active, title),
                );
                if tab.clicked() && !label.active {
                    choice = TabChoice::Show(label.key);
                }
                let close = ui
                    .add_enabled(enabled, egui::Button::new(CLOSE_TAB).small())
                    .on_hover_text(format!("Close {}", label.name));
                if close.clicked() {
                    choice = TabChoice::Close(label.key);
                }
            });
        }
    });
    if let (Err(reason), true) = (strip.available, strip.tabs.len() > 1) {
        ui.weak(reason);
    }
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        context: &egui::Context,
        strip: TabStrip<'_>,
        events: Vec<egui::Event>,
    ) -> (TabChoice, egui::FullOutput) {
        let raw = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut choice = TabChoice::Waiting;
        let mut output = context.run_ui(raw, |ui| choice = tab_strip(ui, strip));
        output.textures_delta.clear();
        (choice, output)
    }

    fn centers(output: &egui::FullOutput, label: &str) -> Vec<egui::Pos2> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .collect()
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

    fn click(at: egui::Pos2) -> Vec<egui::Event> {
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
    }

    #[test]
    fn tabs_show_names_marks_and_answer_with_their_key_only_when_available() {
        // Two copies of one document can have one name: keys tell them apart.
        let tabs = [
            TabLabel {
                key: 41,
                name: "plate.fcad",
                dirty: true,
                active: false,
            },
            TabLabel {
                key: 7,
                name: "plate.fcad",
                dirty: false,
                active: true,
            },
            TabLabel {
                key: 9,
                name: "Untitled",
                dirty: true,
                active: false,
            },
        ];
        for allowed in [false, true] {
            let strip = TabStrip {
                tabs: &tabs,
                available: if allowed {
                    Ok(())
                } else {
                    Err("Wait for the current operation to finish.")
                },
            };
            let context = egui::Context::default();
            let (_, output) = frame(&context, strip, Vec::new());
            let all = texts(&output);
            assert_eq!(centers(&output, "*plate.fcad").len(), 1, "{all:?}");
            assert_eq!(centers(&output, "plate.fcad").len(), 1, "{all:?}");
            assert_eq!(centers(&output, "*Untitled").len(), 1, "{all:?}");
            assert_eq!(centers(&output, CLOSE_TAB).len(), 3, "{all:?}");
            assert_eq!(
                all.iter()
                    .any(|t| t == "Wait for the current operation to finish."),
                !allowed,
                "an unavailable row says why"
            );
            let expected = |choice| {
                if allowed { choice } else { TabChoice::Waiting }
            };
            let dirty_copy = centers(&output, "*plate.fcad")[0];
            assert_eq!(
                frame(&context, strip, click(dirty_copy)).0,
                expected(TabChoice::Show(41))
            );
            let _ = frame(&context, strip, Vec::new());
            // Pressing the shown tab asks for nothing.
            let shown = centers(&output, "plate.fcad")[0];
            assert_eq!(frame(&context, strip, click(shown)).0, TabChoice::Waiting);
            let _ = frame(&context, strip, Vec::new());
            let third_close = centers(&output, CLOSE_TAB)[2];
            assert_eq!(
                frame(&context, strip, click(third_close)).0,
                expected(TabChoice::Close(9))
            );
        }
        // No document, no row.
        let context = egui::Context::default();
        let (_, output) = frame(
            &context,
            TabStrip {
                tabs: &[],
                available: Ok(()),
            },
            Vec::new(),
        );
        assert!(texts(&output).is_empty());
    }
}
