// SPDX-License-Identifier: MIT
use std::path::PathBuf;

use crate::{App, AppEvent, Loads, creates::Creates, dialogs, edits, exports::Exports, sessions};
use ferritecad_document::{DOCUMENT_EXTENSION, DocumentVersion, EditChange};
use ferritecad_jobs::EditRequest;

pub(crate) const EXPRESSION_REFUSAL: &str =
    "This value is an expression; edit its text in Parameters → Values.";

#[derive(Clone, Copy)]
pub(crate) enum Form {
    Cut,
    Fillet,
    Chamfer,
    Height,
    Angle,
    Radius,
    Distance,
    Constraints,
}

/// Window state checked by a form button and rechecked when it submits its request.
pub(crate) type FormGuard =
    fn(&Creates, &Loads, &Exports, &edits::Edits, &sessions::Sessions) -> bool;

/// Shared by sketch Finish, sweep Apply and their request handlers.
pub(crate) fn can_apply(
    creates: &crate::creates::Creates,
    loads: &crate::Loads,
    exports: &crate::exports::Exports,
    edits: &edits::Edits,
    sessions: &sessions::Sessions,
) -> bool {
    !creates.making_new()
        && !edits.running()
        && crate::document_io_idle(loads, exports)
        && sessions.takes_form_apply()
}

/// Both the Height button and its handler allow the open Height editor to submit.
pub(crate) fn can_apply_height(
    creates: &Creates,
    loads: &Loads,
    exports: &Exports,
    edits: &edits::Edits,
    sessions: &sessions::Sessions,
) -> bool {
    can_apply(creates, loads, exports, edits, sessions) && !creates.sketch.active()
}

/// Both Keep solids actions and their handler wait for any open editor to close.
pub(crate) fn can_keep_solids(
    creates: &Creates,
    loads: &Loads,
    exports: &Exports,
    edits: &edits::Edits,
    sessions: &sessions::Sessions,
) -> bool {
    can_apply(creates, loads, exports, edits, sessions) && !creates.sketch.active() && !edits.busy()
}

/// Shared by the saved Revolve angle form's Apply button and its request handler.
pub(crate) fn can_apply_angle(
    creates: &Creates,
    loads: &Loads,
    exports: &Exports,
    edits: &edits::Edits,
    sessions: &sessions::Sessions,
) -> bool {
    can_apply(creates, loads, exports, edits, sessions)
        && creates.can_apply_angle()
        && !edits.busy()
}

/// Shared by the saved Cut form's Apply button and its request handler.
pub(crate) fn can_apply_cut(
    creates: &Creates,
    loads: &Loads,
    exports: &Exports,
    edits: &edits::Edits,
    sessions: &sessions::Sessions,
) -> bool {
    can_apply(creates, loads, exports, edits, sessions) && creates.can_apply_cut() && !edits.busy()
}

/// Shared by the Cut creation form's Add button and its request handler.
pub(crate) fn can_add_cut(
    creates: &Creates,
    loads: &Loads,
    exports: &Exports,
    edits: &edits::Edits,
    sessions: &sessions::Sessions,
) -> bool {
    can_apply(creates, loads, exports, edits, sessions) && creates.can_add_cut() && !edits.busy()
}

/// Shared by the saved Fillet radius form's Apply button and its request handler.
pub(crate) fn can_apply_fillet_radius(
    creates: &Creates,
    loads: &Loads,
    exports: &Exports,
    edits: &edits::Edits,
    sessions: &sessions::Sessions,
) -> bool {
    can_apply(creates, loads, exports, edits, sessions)
        && creates.can_apply_fillet_radius()
        && !edits.busy()
}

/// Shared by the Fillet creation form's Add button and its request handler.
pub(crate) fn can_add_fillet(
    creates: &Creates,
    loads: &Loads,
    exports: &Exports,
    edits: &edits::Edits,
    sessions: &sessions::Sessions,
) -> bool {
    can_apply(creates, loads, exports, edits, sessions) && creates.can_add_fillet() && !edits.busy()
}

/// Shared by the saved Chamfer distance form's Apply button and its request handler.
pub(crate) fn can_apply_chamfer_distance(
    creates: &Creates,
    loads: &Loads,
    exports: &Exports,
    edits: &edits::Edits,
    sessions: &sessions::Sessions,
) -> bool {
    can_apply(creates, loads, exports, edits, sessions)
        && creates.can_apply_chamfer_distance()
        && !edits.busy()
}

/// Shared by the Chamfer creation form's Add button and its request handler.
pub(crate) fn can_add_chamfer(
    creates: &Creates,
    loads: &Loads,
    exports: &Exports,
    edits: &edits::Edits,
    sessions: &sessions::Sessions,
) -> bool {
    can_apply(creates, loads, exports, edits, sessions)
        && creates.can_add_chamfer()
        && !edits.busy()
}

impl App {
    pub(crate) fn apply_expressions(
        &mut self,
        allowed: FormGuard,
        expected: DocumentVersion,
        changes: Vec<EditChange>,
    ) {
        if !allowed(
            &self.creates,
            &self.loads,
            &self.exports,
            &self.edits,
            &self.sessions,
        ) {
            self.input.request_redraw();
            return;
        }
        let proxy = self.proxy.clone();
        self.sessions
            .apply(expected, changes, move |generation, result| {
                let _ = proxy.send_event(AppEvent::Applied {
                    generation,
                    result: Box::new(result),
                });
            });
        self.input.request_redraw();
    }

    pub(crate) fn apply_edge_and_piece_requests(&mut self) {
        if let Some(request) = self.creates.sketch.fillets.take_apply_request() {
            self.apply_expressions(can_apply_fillet_radius, request.expected, request.changes);
        }
        if let Some(request) = self.creates.sketch.fillets.take_add_request() {
            self.apply_expressions(can_add_fillet, request.expected, request.changes);
        }
        if let Some(request) = self.creates.sketch.chamfers.take_apply_request() {
            self.apply_expressions(
                can_apply_chamfer_distance,
                request.expected,
                request.changes,
            );
        }
        if let Some(request) = self.creates.sketch.chamfers.take_add_request() {
            self.apply_expressions(can_add_chamfer, request.expected, request.changes);
        }
        if let Some(request) = self.creates.sketch.keep_solids.take_request() {
            self.apply_expressions(can_keep_solids, request.expected, request.changes);
        }
    }

    pub(crate) fn ask_where_to_edit(
        &mut self,
        form: Form,
        source: PathBuf,
        expected: DocumentVersion,
        changes: Vec<EditChange>,
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
            Form::Cut => "cut.fcad",
            Form::Fillet => "fillet.fcad",
            Form::Chamfer => "chamfer.fcad",
            Form::Constraints => "sketch-constraints.fcad",
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
                            Form::Cut => AppEvent::SketchEdited { generation, result },
                            Form::Fillet => AppEvent::SketchEdited { generation, result },
                            Form::Chamfer => AppEvent::SketchEdited { generation, result },
                            Form::Constraints => AppEvent::SketchConstraintsEdited {
                                generation,
                                result: Box::new(result),
                            },
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

/// A length the way a drawing states one.
///
/// Millimetres, which is the only unit this document has, printed as the
/// number that was stored rather than rounded to a house style: a size a
/// person typed is a size they should be able to recognise.
pub(crate) fn mm(value: f64) -> String {
    value.to_string()
}
