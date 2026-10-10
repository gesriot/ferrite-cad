// SPDX-License-Identifier: MIT
//! §30U: the question before closing one tab with unapplied form values.

pub const BACK_TO_FORM: &str = "Back to form";
pub const DISCARD_FORM_AND_CLOSE: &str = "Discard form and continue Close";

#[derive(Debug, Clone, Copy)]
pub struct CloseFormPanel<'a> {
    pub document: &'a str,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CloseFormChoice {
    #[default]
    Waiting,
    Back,
    Discard,
}

/// The caller holds the original form while this question is visible. Explicit
/// widget ids keep focus on the decision, never on an absent text field.
pub fn close_form_panel(ui: &mut egui::Ui, panel: Option<CloseFormPanel<'_>>) -> CloseFormChoice {
    let Some(panel) = panel else {
        return CloseFormChoice::Waiting;
    };
    let mut choice = CloseFormChoice::Waiting;
    ui.push_id("close-unfinished-form", |ui| {
        ui.strong("This tab has an unfinished form");
        ui.label(format!(
            "Closing {} will discard the entered, not yet applied values of its form. \
             Its accepted model is unchanged. Unsaved model changes will be asked about separately.",
            panel.document
        ));
        ui.horizontal_wrapped(|ui| {
            if ui.button(BACK_TO_FORM).clicked() { choice = CloseFormChoice::Back; }
            if ui.button(DISCARD_FORM_AND_CLOSE).clicked() { choice = CloseFormChoice::Discard; }
        });
    });
    choice
}
