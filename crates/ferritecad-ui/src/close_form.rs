// SPDX-License-Identifier: MIT
//! §30U: the question before closing one tab with unapplied form values.

pub const BACK_TO_FORM: &str = "Back to form";
pub const DISCARD_FORM_AND_QUIT: &str = "Discard form and continue Quit";
pub const DISCARD_FORM_AND_CLOSE: &str = "Discard form and continue Close";
use crate::BACK_TO_NEW;
pub const DISCARD_NEW_AND_QUIT: &str = "Discard New and continue Quit";

/// §30W: New is window-owned; this decision creates/applies/saves nothing.
pub fn quit_new_panel(ui: &mut egui::Ui, losing: &str) -> CloseFormChoice {
    let mut choice = CloseFormChoice::Waiting;
    ui.push_id("quit-unfinished-new", |ui| {
        ui.strong("New is not finished");
        ui.label(format!("If the whole Quit succeeds, {losing}. Nothing from New will be created or saved. Back to New cancels the whole Quit. A later Cancel or refusal returns New and every tab form. Saves already published stay saved."));
        ui.horizontal_wrapped(|ui| {
            if ui.button(BACK_TO_NEW).clicked() { choice = CloseFormChoice::Back; }
            if ui.button(DISCARD_NEW_AND_QUIT).clicked() { choice = CloseFormChoice::Discard; }
        });
    });
    choice
}

#[derive(Debug, Clone, Copy)]
pub struct CloseFormPanel<'a> {
    pub document: &'a str,
    pub quitting: bool,
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
        if panel.quitting {
            ui.label(format!("Quitting will discard the entered, not yet applied values of {}'s form only if the whole Quit succeeds. Unsaved model changes will be asked about separately. Back to form cancels the whole Quit and returns every form. Saves already published stay saved.", panel.document));
        } else {
        ui.label(format!(
            "Closing {} will discard the entered, not yet applied values of its form. \
             Its accepted model is unchanged. Unsaved model changes will be asked about separately.",
            panel.document
        ));
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(BACK_TO_FORM).clicked() { choice = CloseFormChoice::Back; }
            if ui.button(if panel.quitting { DISCARD_FORM_AND_QUIT } else { DISCARD_FORM_AND_CLOSE }).clicked() { choice = CloseFormChoice::Discard; }
        });
    });
    choice
}
