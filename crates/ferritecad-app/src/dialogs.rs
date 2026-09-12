// SPDX-License-Identifier: MIT
//! The UI boundary of a native file panel; no document or worker lives here.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Open,
    New,
    Edit,
    ExportFbx,
    ExportStl,
}

impl Action {
    fn title(self) -> &'static str {
        match self {
            Self::Open => "Open a document",
            Self::New => "New document",
            Self::Edit => "Save edited model as a new file",
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
    ) -> Option<PathBuf> {
        let answer = invoke(action, || {
            let builder = builder.set_title(action.title());
            match action {
                Action::Open => builder.pick_file(),
                _ => builder.save_file(),
            }
        });
        self.receive(action, answer, input)
    }

    pub(super) fn receive(
        &mut self,
        action: Action,
        answer: Outcome,
        input: &mut ferritecad_ui::ViewportInput,
    ) -> Option<PathBuf> {
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

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn null_panel_constructor_is_a_failure_instead_of_unwinding_the_viewer() {
        for action in [
            Action::Open,
            Action::New,
            Action::Edit,
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
            Action::New,
            Action::Edit,
            Action::ExportFbx,
            Action::ExportStl,
        ] {
            assert_eq!(
                dialogs.receive(action, invoke(action, || None), &mut input),
                None
            );
            assert_eq!(dialogs.failure(), None);
            assert!(!input.take_redraw());
            assert_eq!(dialogs.receive(action, Outcome::Failed, &mut input), None);
            let failure = dialogs.failure().expect("a failure must be shown");
            assert!(failure.starts_with(action.title()));
            assert!(failure.contains("could not be opened"));
            assert!(input.take_redraw());
            assert_eq!(dialogs.receive(action, Outcome::Failed, &mut input), None);
            assert!(
                !input.take_redraw(),
                "the same failure must not request frames forever"
            );
            assert_eq!(
                dialogs.receive(action, invoke(action, || Some(path.clone())), &mut input),
                Some(path.clone())
            );
            assert_eq!(dialogs.failure(), None);
            assert!(input.take_redraw());
        }
    }
}
