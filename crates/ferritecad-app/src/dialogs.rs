// SPDX-License-Identifier: MIT
//! The UI boundary of a native file panel; no document or worker lives here.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Open,
    Edit,
    SaveAs,
    ExportFbx,
    ExportStl,
}

impl Action {
    fn title(self) -> &'static str {
        match self {
            Self::Open => "Open a document",
            Self::Edit => "Save edited model as a new file",
            Self::SaveAs => "Save As",
            Self::ExportFbx => "Export FBX",
            Self::ExportStl => "Export STL",
        }
    }

    #[cfg(target_os = "macos")]
    fn null_constructor(self) -> &'static str {
        match self {
            Self::Open => "unexpected NULL returned from +[NSOpenPanel openPanel]",
            _ => "unexpected NULL returned from +[NSSavePanel savePanel]",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Selected(PathBuf),
    Cancelled,
    Failed,
}

fn invoke(
    action: Action,
    panel: impl FnOnce() -> Option<PathBuf> + std::panic::UnwindSafe,
) -> Outcome {
    // rfd 0.17.2 / objc2-app-kit 0.3.2: these constructors are the FIRST
    // AppKit call in Panel::build_{pick,save}_file. A NULL return panics before
    // Panel::new changes focus/policy, or any sheet/modal loop starts. rfd's
    // autorelease pool drains on Rust unwind. Calls are synchronous on winit's
    // main thread. Do not generalise this to later panel failures, Objective-C
    // exceptions, other panic payloads, or another backend without an audit.
    #[cfg(target_os = "macos")]
    let selected = match std::panic::catch_unwind(panel) {
        Ok(selected) => selected,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str));
            if message == Some(action.null_constructor()) {
                return Outcome::Failed;
            }
            std::panic::resume_unwind(payload);
        }
    };
    #[cfg(not(target_os = "macos"))]
    let selected = {
        let _ = action;
        panel()
    };
    match selected {
        Some(path) => Outcome::Selected(path),
        None => Outcome::Cancelled,
    }
}

#[derive(Default)]
pub(crate) struct Dialogs {
    failure: Option<String>,
}

impl Dialogs {
    pub(crate) fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// The caller may only start work with the selected path. A failure is
    /// retained separately from a cancellation, without rewriting job status.
    pub(crate) fn choose(
        &mut self,
        action: Action,
        builder: rfd::FileDialog,
        input: &mut ferritecad_ui::ViewportInput,
        private: Option<&std::path::Path>,
    ) -> Option<PathBuf> {
        let answer = invoke(action, || {
            let builder = builder.set_title(action.title());
            match action {
                Action::Open => builder.pick_file(),
                _ => builder.save_file(),
            }
        });
        self.receive(action, answer, input, private)
    }

    /// Asks what to do with unsaved changes before something replaces the document.
    ///
    /// A message dialog, modal like the file dialogs. `None` is "could not ask", which
    /// the caller treats as Cancel: a question that cannot be asked never loses work.
    pub(crate) fn ask_unsaved(
        &mut self,
        name: &str,
        parent: &winit::window::Window,
        request: &str,
    ) -> Option<crate::sessions::UnsavedChoice> {
        let buttons = if cfg!(target_os = "macos") {
            rfd::MessageButtons::YesNoCancelCustom(
                "Save".to_owned(),
                "Discard".to_owned(),
                "Cancel".to_owned(),
            )
        } else {
            rfd::MessageButtons::YesNoCancel
        };
        let explanation = if cfg!(target_os = "macos") {
            String::new()
        } else {
            " Yes saves them, No discards them, Cancel keeps the document open.".to_owned()
        };
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Unsaved changes")
            .set_description(format!(
                "{name} has changes that are not saved. {request}{explanation}"
            ))
            .set_buttons(buttons)
            .set_parent(parent)
            .show();
        Some(unsaved_choice(&answer))
    }

    pub(super) fn receive(
        &mut self,
        action: Action,
        answer: Outcome,
        input: &mut ferritecad_ui::ViewportInput,
        private: Option<&std::path::Path>,
    ) -> Option<PathBuf> {
        // Something written for the user to keep (everything but Open) must not be
        // placed in the working folder that goes away with the document.
        let answer = match answer {
            Outcome::Selected(path)
                if action != Action::Open
                    && private.is_some_and(|dir| ferritecad_jobs::is_inside(dir, &path)) =>
            {
                self.failure = Some(format!(
                    "{}: {} is inside FerriteCAD's temporary working folder, which is deleted when the document is closed. Nothing was written; choose a folder of your own.",
                    action.title(),
                    // The file's own name: the working folder is never shown (§30L).
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                ));
                input.request_redraw();
                return None;
            }
            other => other,
        };
        let failure = (answer == Outcome::Failed).then(|| {
            format!(
                "{}: the system file dialog could not be opened. No file was chosen. \
                 Your current model is unchanged; you can try again.",
                action.title()
            )
        });
        if self.failure != failure {
            self.failure = failure;
            input.request_redraw();
        }
        match answer {
            Outcome::Selected(path) => Some(path),
            Outcome::Cancelled | Outcome::Failed => None,
        }
    }
}

/// What a message dialog's answer means. Anything that is not a clear Save or
/// Discard is Cancel: the document stays and nothing is lost.
pub(crate) fn unsaved_choice(answer: &rfd::MessageDialogResult) -> crate::sessions::UnsavedChoice {
    use crate::sessions::UnsavedChoice;
    use rfd::MessageDialogResult as Answer;
    match answer {
        Answer::Yes => UnsavedChoice::Save,
        Answer::No => UnsavedChoice::Discard,
        Answer::Custom(label) if label == "Save" => UnsavedChoice::Save,
        Answer::Custom(label) if label == "Discard" => UnsavedChoice::Discard,
        _ => UnsavedChoice::Cancel,
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn null_panel_constructor_is_a_failure_instead_of_unwinding_the_viewer() {
        for action in [
            Action::Open,
            Action::Edit,
            Action::SaveAs,
            Action::ExportFbx,
            Action::ExportStl,
        ] {
            assert_eq!(
                invoke(action, || std::panic::panic_any(action.null_constructor())),
                Outcome::Failed,
            );
            assert_eq!(
                invoke(action, || std::panic::panic_any(
                    action.null_constructor().to_owned()
                )),
                Outcome::Failed,
            );
        }
    }

    #[test]
    fn unrelated_panics_are_not_disguised_as_dialog_failures() {
        for message in [
            "unrelated panic",
            "unexpected NULL returned from -[NSOpenPanel URL]",
        ] {
            let answer = std::panic::catch_unwind(|| {
                invoke(Action::Open, || std::panic::panic_any(message))
            });
            assert_eq!(
                answer
                    .expect_err("unrelated panic must propagate")
                    .downcast_ref::<&str>(),
                Some(&message)
            );
        }
        // A save constructor panic on the Open path is not the audited call.
        let answer = std::panic::catch_unwind(|| {
            invoke(Action::Open, || {
                std::panic::panic_any("unexpected NULL returned from +[NSSavePanel savePanel]")
            })
        });
        assert!(answer.is_err());
    }

    #[test]
    fn failure_is_visible_cancel_is_silent_and_only_selection_reaches_work() {
        let mut dialogs = Dialogs::default();
        let mut input = ferritecad_ui::ViewportInput::new();
        input.take_redraw();
        let path = PathBuf::from("a chosen document.fcad");
        for action in [
            Action::Open,
            Action::Edit,
            Action::SaveAs,
            Action::ExportFbx,
            Action::ExportStl,
        ] {
            assert_eq!(
                dialogs.receive(action, invoke(action, || None), &mut input, None),
                None
            );
            assert_eq!(dialogs.failure(), None);
            assert!(!input.take_redraw());
            assert_eq!(
                dialogs.receive(action, Outcome::Failed, &mut input, None),
                None
            );
            let failure = dialogs.failure().expect("a failure must be shown");
            assert!(failure.starts_with(action.title()));
            assert!(failure.contains("could not be opened"));
            assert!(input.take_redraw());
            assert_eq!(
                dialogs.receive(action, Outcome::Failed, &mut input, None),
                None
            );
            assert!(
                !input.take_redraw(),
                "the same failure must not request frames forever"
            );
            assert_eq!(
                dialogs.receive(
                    action,
                    invoke(action, || Some(path.clone())),
                    &mut input,
                    None
                ),
                Some(path.clone())
            );
            assert_eq!(dialogs.failure(), None);
            assert!(input.take_redraw());
        }
    }

    #[test]
    fn only_a_clear_save_or_discard_is_not_a_cancel() {
        use crate::sessions::UnsavedChoice;
        use rfd::MessageDialogResult as Answer;
        for (answer, expected) in [
            (Answer::Yes, UnsavedChoice::Save),
            (Answer::Custom("Save".to_owned()), UnsavedChoice::Save),
            (Answer::No, UnsavedChoice::Discard),
            (Answer::Custom("Discard".to_owned()), UnsavedChoice::Discard),
            (Answer::Cancel, UnsavedChoice::Cancel),
            (Answer::Custom("Cancel".to_owned()), UnsavedChoice::Cancel),
            (Answer::Ok, UnsavedChoice::Cancel),
            (
                Answer::Custom("anything else".to_owned()),
                UnsavedChoice::Cancel,
            ),
        ] {
            assert_eq!(unsaved_choice(&answer), expected, "{answer:?}");
        }
        // The dialog's own default, which a dialog that could not be shown returns.
        assert_eq!(unsaved_choice(&Answer::default()), UnsavedChoice::Cancel);
    }
}
