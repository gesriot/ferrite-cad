// SPDX-License-Identifier: MIT
use std::path::PathBuf;

use crate::{App, AppEvent, dialogs, edits, sessions};
use ferritecad_document::{DOCUMENT_EXTENSION, DocumentVersion, ExpressionChange};
use ferritecad_jobs::EditRequest;

#[derive(Clone, Copy)]
pub(crate) enum Form {
    Height,
    Angle,
    Radius,
    Distance,
}

impl App {
    pub(crate) fn apply_expressions(
        &mut self,
        expected: DocumentVersion,
        changes: Vec<ExpressionChange>,
    ) {
        if self.creates.making_new()
            || self.edits.running()
            || !crate::document_io_idle(&self.loads, &self.exports)
            || !self.sessions.takes_form_apply()
        {
            self.input.request_redraw();
            return;
        }
        let proxy = self.proxy.clone();
        self.sessions.begin_apply(|ticket, generation, cancel| {
            sessions::spawn_apply(expected, ticket, changes, cancel.clone(), move |result| {
                let _ = proxy.send_event(AppEvent::Applied {
                    generation,
                    result: Box::new(result),
                });
            })
        });
        self.input.request_redraw();
    }

    pub(crate) fn ask_where_to_edit(
        &mut self,
        form: Form,
        source: PathBuf,
        expected: DocumentVersion,
        changes: Vec<ExpressionChange>,
    ) {
        if refuse_unsaved_copy(&mut self.sessions)
            || self.sessions.busy()
            || self.edits.running()
            || !crate::document_io_idle(&self.loads, &self.exports)
        {
            self.input.request_redraw();
            return;
        }
        let Some(live) = &self.live else {
            return;
        };
        let name = match form {
            Form::Height => "edited.fcad",
            Form::Angle => "edited-revolve.fcad",
            Form::Radius => "edited-fillet.fcad",
            Form::Distance => "edited-chamfer.fcad",
        };
        let Some(destination) = self.dialogs.choose(
            dialogs::Action::Edit,
            rfd::FileDialog::new()
                .add_filter("FerriteCAD document", &[DOCUMENT_EXTENSION])
                .set_directory(self.sessions.suggested_directory())
                .set_file_name(name)
                .set_parent(live.window.as_ref()),
            &mut self.input,
            self.sessions.private_directory(),
        ) else {
            return;
        };
        let request = EditRequest {
            source,
            expected,
            changes,
            destination,
        };
        let proxy = self.proxy.clone();
        self.edits
            .start(request, move |request, generation, cancel| {
                edits::spawn_edit(
                    request.source,
                    request.expected,
                    request.changes,
                    request.destination,
                    cancel,
                    move |result| {
                        let event = match form {
                            Form::Height => AppEvent::Edited { generation, result },
                            Form::Angle => AppEvent::RevolveAngleEdited { generation, result },
                            Form::Radius => AppEvent::FilletRadiusEdited { generation, result },
                            Form::Distance => {
                                AppEvent::ChamferDistanceEdited { generation, result }
                            }
                        };
                        let _ = proxy.send_event(event);
                    },
                )
            });
        self.input.request_redraw();
    }
}

pub(crate) fn refuse_unsaved_copy(sessions: &mut sessions::Sessions) -> bool {
    if sessions.dirty() {
        sessions.status = "Saving an edited copy is unavailable while the document has unsaved changes: Save or Undo them first.".to_owned();
    }
    sessions.dirty()
}
