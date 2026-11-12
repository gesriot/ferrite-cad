// SPDX-License-Identifier: MIT
//! Unsubmitted text over the accepted document, with the Model tree's selection.

mod draw;
#[cfg(test)]
mod tests;
mod words;

use ferritecad_document::{DocumentVersion, EditChange, ExpressionSlot, ObjectPayload};
use ferritecad_jobs::ProducedStep;
use ferritecad_types::{Dimension, DocumentId, ObjectId, Result};
use ferritecad_ui::ModelKey;

use crate::{App, AppEvent, commands, sessions, sketch_facts::Source};

#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<Draft>,
}

#[derive(Debug)]
struct Draft {
    document: DocumentId,
    selected: Option<ObjectId>,
    name: String,
    text: String,
    slots: Vec<(ExpressionSlot, String)>,
    add_name: String,
    add_text: String,
    dimension: Dimension,
    pending: Option<(sessions::Address, Field)>,
    refusal: Option<(Field, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Add,
    Name(ObjectId),
    Text(ObjectId),
    Slot(ObjectId, ExpressionSlot),
}

/// A field submitted against the accepted version at the moment of the click.
#[derive(Debug)]
pub(crate) struct Request {
    expected: DocumentVersion,
    field: Field,
    change: EditChange,
}

impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some()
    }

    pub(crate) fn begin(&mut self, source: &Source) -> bool {
        self.draft = Some(Draft {
            document: source.version.document_id,
            selected: None,
            name: String::new(),
            text: String::new(),
            slots: Vec::new(),
            add_name: String::new(),
            add_text: String::new(),
            dimension: Dimension::Length,
            pending: None,
            refusal: None,
        });
        true
    }

    /// A different document ends this form; another version of its document does not.
    pub(crate) fn document_arrived(&mut self, source: Option<&Source>) {
        if self.draft.as_ref().is_some_and(|draft| {
            source.is_none_or(|source| source.version.document_id != draft.document)
        }) {
            self.draft = None;
        }
    }

    fn apply(
        &mut self,
        sessions: &mut sessions::Sessions,
        available: commands::Availability,
        request: Request,
        deliver: impl FnOnce(sessions::Address, Result<ProducedStep>) + Send + 'static,
    ) -> Option<sessions::Address> {
        if let Err(reason) = available.and_then(commands::Access::operation) {
            sessions.status = reason.into();
            return None;
        }
        let draft = self.draft.as_mut()?;
        let address = sessions.apply(request.expected, vec![request.change], deliver)?;
        // The field tag stays beside the owner's address; only the owner accepts the step.
        draft.pending = Some((address, request.field));
        draft.refusal = None;
        Some(address)
    }

    /// Match the owner's completed step before replacing only the submitted text.
    pub(crate) fn finished(
        &mut self,
        address: sessions::Address,
        accepted: bool,
        source: Option<&Source>,
        outcome: &str,
    ) {
        let Some(draft) = &mut self.draft else { return };
        let Some((_, field)) = draft.pending.take_if(|(asked, _)| *asked == address) else {
            return;
        };
        if accepted {
            if let Some(source) = source {
                draft.accepted(field, source);
            }
        } else {
            draft.refusal = Some((field, outcome.to_owned()));
        }
    }
}

impl Draft {
    fn select(&mut self, selected: Option<ModelKey>, source: &Source) {
        let selected = selected
            .map(|key| key.0)
            .filter(|id| source.objects.iter().any(|object| object.id == *id));
        if self.selected != selected {
            self.selected = selected;
            self.name.clear();
            self.text.clear();
            self.slots.clear();
            if !matches!(self.refusal, Some((Field::Add, _))) {
                self.refusal = None;
            }
            if let Some(parameter) = selected.and_then(|id| parameter(source, id)) {
                self.name = parameter.name.clone();
                self.text = parameter.expression.source.clone();
            }
        }
        // Keep drafts for surviving slots, and seed only newly present slots.
        self.slots.retain(|(slot, _)| {
            source
                .expression_slots
                .iter()
                .any(|saved| Some(saved.object) == selected && saved.slot == *slot)
        });
        for saved in source
            .expression_slots
            .iter()
            .filter(|s| Some(s.object) == selected)
        {
            if !self.slots.iter().any(|(slot, _)| *slot == saved.slot) {
                self.slots
                    .push((saved.slot, saved.expression.source.clone()));
            }
        }
    }

    fn accepted(&mut self, field: Field, source: &Source) {
        match field {
            Field::Add => {
                self.add_name.clear();
                self.add_text.clear();
            }
            Field::Name(id) => {
                if self.selected == Some(id)
                    && let Some(saved) = parameter(source, id)
                {
                    self.name = saved.name.clone();
                }
            }
            Field::Text(id) => {
                if self.selected == Some(id)
                    && let Some(saved) = parameter(source, id)
                {
                    self.text = saved.expression.source.clone();
                }
            }
            Field::Slot(id, slot) => {
                if self.selected == Some(id)
                    && let Some(saved) = source.expression_slot(id, slot)
                    && let Some((_, text)) = self.slots.iter_mut().find(|(s, _)| *s == slot)
                {
                    *text = saved.expression.source.clone();
                }
            }
        }
    }
}

fn parameter(source: &Source, id: ObjectId) -> Option<&ferritecad_document::Parameter> {
    source
        .objects
        .iter()
        .find(|object| object.id == id)
        .and_then(|object| match &object.payload {
            ObjectPayload::Parameter(parameter) => Some(parameter),
            _ => None,
        })
}

impl App {
    pub(crate) fn apply_parameter(&mut self, request: Request) {
        let available = commands::availability(
            &self.creates,
            &self.edits,
            &self.sessions,
            &self.tabs,
            &self.loads,
            &self.exports,
            &self.input,
            self.model_confirmation.is_some(),
        );
        let proxy = self.proxy.clone();
        self.creates.sketch.parameters.apply(
            &mut self.sessions,
            available,
            request,
            move |generation, result| {
                let _ = proxy.send_event(AppEvent::Applied {
                    generation,
                    result: Box::new(result),
                });
            },
        );
        self.input.request_redraw();
    }

    pub(crate) fn finish_parameters(&mut self, address: sessions::Address, accepted: bool) {
        self.creates.sketch.parameters.finished(
            address,
            accepted,
            self.live
                .as_ref()
                .and_then(|live| live.scene.edit_source.as_ref()),
            &self.sessions.status,
        );
    }
}
