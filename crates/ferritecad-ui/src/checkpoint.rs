// SPDX-License-Identifier: MIT
//! The checkpoints of the open document (§30N): a short list, a name field and
//! three buttons. Words and buttons only: what a checkpoint is, whether it may be
//! made, restored or deleted, and what doing so changes belong to the window and
//! the library behind it.

/// The section's heading.
pub const CHECKPOINTS_HEADING: &str = "Checkpoints";
/// Keeps the current model under the typed name.
pub const CREATE_CHECKPOINT: &str = "Create checkpoint";
/// Makes a checkpoint's model the working model, as one Undo step.
pub const RESTORE_CHECKPOINT: &str = "Restore";
/// Removes a checkpoint from the working document, after a question.
pub const DELETE_CHECKPOINT: &str = "Delete…";

/// One checkpoint, in the words a person reads.
#[derive(Debug, Clone, Copy)]
pub struct CheckpointRow<'a> {
    pub name: &'a str,
    /// When it was made, as a person reads a time.
    pub created: &'a str,
    /// Whether it holds the model on screen.
    pub current: bool,
}

/// What the section shows this frame. Absent while no document is open.
#[derive(Debug, Clone, Copy)]
pub struct CheckpointPanel<'a> {
    pub rows: &'a [CheckpointRow<'a>],
    /// `Err` holds why Create cannot be pressed now.
    pub create: Result<(), &'a str>,
    /// `Err` holds why Restore and Delete cannot be pressed now.
    pub act: Result<(), &'a str>,
    /// How much of the limits the document uses, in one line.
    pub usage: &'a str,
}

/// What was pressed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointChoice {
    #[default]
    Waiting,
    Create,
    Restore(usize),
    Delete(usize),
}

/// Draws the section, collapsed until a person opens it. `name` is the typed
/// name; the window keeps it between frames.
pub fn checkpoint_panel(
    ui: &mut egui::Ui,
    panel: CheckpointPanel<'_>,
    name: &mut String,
) -> CheckpointChoice {
    let mut choice = CheckpointChoice::Waiting;
    egui::CollapsingHeader::new(format!("{CHECKPOINTS_HEADING} ({})", panel.rows.len()))
        .id_salt(CHECKPOINTS_HEADING)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(name).hint_text("Checkpoint name"));
                if ui
                    .add_enabled(panel.create.is_ok(), egui::Button::new(CREATE_CHECKPOINT))
                    .clicked()
                {
                    choice = CheckpointChoice::Create;
                }
            });
            if let Err(reason) = panel.create {
                ui.weak(reason);
            }
            for (index, row) in panel.rows.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    let marker = if row.current { " · on screen" } else { "" };
                    ui.label(format!("{} — {}{marker}", row.name, row.created));
                    if ui
                        .add_enabled(panel.act.is_ok(), egui::Button::new(RESTORE_CHECKPOINT))
                        .clicked()
                    {
                        choice = CheckpointChoice::Restore(index);
                    }
                    if ui
                        .add_enabled(panel.act.is_ok(), egui::Button::new(DELETE_CHECKPOINT))
                        .clicked()
                    {
                        choice = CheckpointChoice::Delete(index);
                    }
                });
            }
            if let (Err(reason), false) = (panel.act, panel.rows.is_empty()) {
                ui.weak(reason);
            }
            ui.weak(panel.usage);
        });
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        context: &egui::Context,
        panel: CheckpointPanel<'_>,
        name: &mut String,
        events: Vec<egui::Event>,
    ) -> (CheckpointChoice, egui::FullOutput) {
        let raw = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut choice = CheckpointChoice::Waiting;
        let mut output = context.run_ui(raw, |ui| choice = checkpoint_panel(ui, panel, name));
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
    fn the_buttons_ask_nothing_while_the_window_says_why_they_cannot() {
        let rows = [
            CheckpointRow {
                name: "Before cut",
                created: "2026-10-08 07:12:33 UTC",
                current: false,
            },
            CheckpointRow {
                name: "Before cut",
                created: "2026-10-08 07:13:00 UTC",
                current: true,
            },
        ];
        for allowed in [false, true] {
            let panel = CheckpointPanel {
                rows: &rows,
                create: if allowed {
                    Ok(())
                } else {
                    Err("Close the open form first.")
                },
                act: if allowed {
                    Ok(())
                } else {
                    Err("Wait for the current operation.")
                },
                usage: "2 of 32 checkpoints, 0.1 of 16 MiB",
            };
            let context = egui::Context::default();
            // Opening the section is animated; the test reads the opened section.
            context.all_styles_mut(|style| style.animation_time = 0.0);
            let mut name = "Kept".to_owned();
            // Collapsed at first: only the heading with the count.
            let (_, output) = frame(&context, panel, &mut name, Vec::new());
            let heading = centers(&output, "Checkpoints (2)");
            assert_eq!(heading.len(), 1, "{:?}", texts(&output));
            assert!(centers(&output, CREATE_CHECKPOINT).is_empty());
            let (_, _) = frame(&context, panel, &mut name, click(heading[0]));
            let (_, output) = frame(&context, panel, &mut name, Vec::new());
            let all = texts(&output);
            assert!(
                all.iter()
                    .any(|t| t == "Before cut — 2026-10-08 07:13:00 UTC · on screen"),
                "{all:?}"
            );
            assert!(
                all.iter()
                    .any(|t| t == "2 of 32 checkpoints, 0.1 of 16 MiB")
            );
            assert_eq!(
                all.iter().any(|t| t == "Close the open form first."),
                !allowed,
                "a disabled Create says why"
            );
            let expected = |choice| {
                if allowed {
                    choice
                } else {
                    CheckpointChoice::Waiting
                }
            };
            let create = centers(&output, CREATE_CHECKPOINT)[0];
            let restore = centers(&output, RESTORE_CHECKPOINT)[1];
            let delete = centers(&output, DELETE_CHECKPOINT)[0];
            assert_eq!(
                frame(&context, panel, &mut name, click(create)).0,
                expected(CheckpointChoice::Create)
            );
            let _ = frame(&context, panel, &mut name, Vec::new());
            assert_eq!(
                frame(&context, panel, &mut name, click(restore)).0,
                expected(CheckpointChoice::Restore(1))
            );
            let _ = frame(&context, panel, &mut name, Vec::new());
            assert_eq!(
                frame(&context, panel, &mut name, click(delete)).0,
                expected(CheckpointChoice::Delete(0))
            );
            assert_eq!(
                name, "Kept",
                "pressing a button does not change the typed name"
            );
        }
    }
}
