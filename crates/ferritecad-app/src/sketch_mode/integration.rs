// SPDX-License-Identifier: MIT
//! Routes the form through the existing window owners.
use crate::{App, commands, sketch::accepted::Accepted};
use crate::{claimed_by_interface, translate};
use ferritecad_types::Result;
use ferritecad_ui::{ViewportEvent, ViewportInput};
use ferritecad_viewport_gpu::{PreparedSnapshot, Renderer};
use winit::event::WindowEvent;

pub(crate) fn sync(
    renderer: &mut Renderer,
    prepared: &mut PreparedSnapshot,
    accepted: &mut Accepted,
    editor: &super::Editor,
) -> Result<()> {
    let filter = editor.filter();
    if accepted.mode_filter != filter {
        let drawings = accepted.drawings_except(filter.flatten())?;
        renderer.replace_sketches(prepared, &drawings)?;
        accepted.mode_filter = filter;
    }
    Ok(())
}

impl App {
    pub(crate) fn new_sketch(&mut self) {
        let idle = commands::availability(
            &self.creates,
            &self.edits,
            &self.sessions,
            &self.tabs,
            &self.loads,
            &self.exports,
            &self.input,
            self.model_confirmation.is_some(),
        );
        let Some(source) = self
            .live
            .as_ref()
            .and_then(|live| live.scene.edit_source.as_ref())
        else {
            return;
        };
        match commands::new_sketch_available(source, idle) {
            Err(reason) => self.sessions.status = reason,
            Ok(()) => {
                if let Some(snapshot) = self.sessions.export_source() {
                    self.creates
                        .sketch
                        .viewport
                        .begin_new(snapshot.path(), source, None);
                }
            }
        }
        self.input.request_redraw();
    }

    pub(crate) fn draw_or_pick(&mut self, x: f32, y: f32) {
        if !self.creates.sketch.viewport.take_click(
            &mut self.input,
            [x, y],
            self.modifiers.shift_key(),
            self.sessions.takes_form_apply(),
        ) {
            self.choose_at(x, y);
        }
    }

    pub(crate) fn sketch_history(&mut self, undo: bool) -> bool {
        self.creates.sketch.viewport.take_history(
            &mut self.input,
            undo,
            self.sessions.takes_form_apply(),
        )
    }

    pub(crate) fn sketch_hover(&mut self) -> bool {
        if !self.creates.sketch.viewport.take_hover(&mut self.input) {
            return false;
        }
        if let Some(live) = &mut self.live {
            live.scene.hovered = crate::Hovered::Nothing;
        }
        true
    }
}

impl super::Editor {
    /// Handle hover without asking the rendered body for an entity pixel.
    pub(crate) fn take_hover(&mut self, input: &mut ferritecad_ui::ViewportInput) -> bool {
        if !self.active() {
            return false;
        }
        self.resume_events(input);
        let hover = input.take_hover();
        if self.gesture.is_some() {
            let moved = self
                .gesture
                .as_ref()
                .is_some_and(super::drag::Gesture::pending_frame);
            self.drag_frame(input.camera());
            self.resume_events(input);
            if moved {
                input.request_redraw();
            }
            return true;
        }
        match hover {
            ferritecad_ui::Hover::At(x, y) => {
                self.pointer(input.camera(), [f64::from(x), f64::from(y)], false, false);
                input.request_redraw();
            }
            ferritecad_ui::Hover::Cleared => {
                self.clear_pointer();
                input.request_redraw();
            }
            ferritecad_ui::Hover::Unchanged => {}
        }
        true
    }

    /// Consume a drawing click even while its saved-object form is read-only.
    pub(crate) fn take_click(
        &mut self,
        input: &mut ferritecad_ui::ViewportInput,
        at: [f32; 2],
        shift: bool,
        editable: bool,
    ) -> bool {
        if !self.active() {
            return false;
        }
        if editable {
            self.pointer(input.camera(), at.map(f64::from), true, shift);
            input.request_redraw();
        }
        true
    }

    /// A draft owns history keys until its ordinary form lifecycle closes it.
    pub(crate) fn take_history(
        &mut self,
        input: &mut ferritecad_ui::ViewportInput,
        undo: bool,
        editable: bool,
    ) -> bool {
        if !self.active() {
            return false;
        }
        if editable {
            self.history(undo);
            input.request_redraw();
        }
        true
    }
}

/// Translates one window event and gives only viewport state to its answers.
///
/// Selection and visibility are deliberately absent: camera gestures cannot
/// change either through this semantic route. Keeping the small piece of
/// window wiring free also lets tests drive the same path without a window.
pub(crate) fn apply_viewport_input(
    input: &mut ViewportInput,
    event: &WindowEvent,
    consumed: bool,
    wants_pointer: bool,
) {
    route_viewport_events(event, consumed, wants_pointer, |event, claimed| {
        input.handle(event, claimed)
    });
}

fn route_viewport_events(
    event: &WindowEvent,
    consumed: bool,
    wants_pointer: bool,
    mut route: impl FnMut(ViewportEvent, bool),
) {
    for event in translate(event) {
        route(event, claimed_by_interface(&event, consumed, wants_pointer));
    }
}

impl super::Editor {
    pub(crate) fn window_event(
        &mut self,
        input: &mut ViewportInput,
        event: &WindowEvent,
        consumed: bool,
        wants_pointer: bool,
        editable: bool,
        shift: bool,
    ) {
        if self.active() {
            route_viewport_events(event, consumed, wants_pointer, |event, claimed| {
                self.route(input, event, claimed, editable, shift);
            });
        } else {
            apply_viewport_input(input, event, consumed, wants_pointer);
        }
    }
}
