// SPDX-License-Identifier: MIT
use crate::commands;
use crate::sessions::tests::tabs::Window;

pub(super) struct Ui {
    ctx: egui::Context,
    confirmation: Option<commands::Confirmation>,
    size: egui::Vec2,
}

impl Ui {
    pub(super) fn new(window: &mut Window) -> Self {
        let mut result = Self {
            ctx: egui::Context::default(),
            confirmation: None,
            size: egui::vec2(988., 768.),
        };
        // An ordinary open form uses the viewport laid out before its opening click.
        let editor = std::mem::take(&mut window.creates.sketch.viewport);
        result.settle(window);
        window.creates.sketch.viewport = editor;
        result
    }

    pub(super) fn frame(
        &mut self,
        w: &mut Window,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, commands::Chosen) {
        let available = commands::availability(
            &w.creates,
            &w.edits,
            &w.sessions,
            &w.tabs,
            &crate::Loads::default(),
            &crate::exports::Exports::default(),
            &w.input,
            self.confirmation.is_some(),
        );
        w.creates
            .sketch
            .viewport
            .set_apply(w.sessions.takes_form_apply());
        let tab = w.sessions.tab().key();
        let (form, sketch) = w.creates.forms();
        let mut menu = None;
        let mut view = crate::workspace::View {
            activity: ferritecad_ui::Activity::default(),
            sections: crate::Sections {
                dialog_failure: None,
                sketch,
                creating: false,
                edits: &mut w.edits,
                stl_form: None,
                can_edit: false,
                can_new: true,
                height: ferritecad_ui::HeightState {
                    running: false,
                    can_cancel: false,
                    apply: false,
                    copy: false,
                    unsaved: false,
                },
                held_back: false,
                document_outcome: &w.sessions.status,
                export: None,
                replacing: None,
                form,
                open_over_new: None,
                closing_form: false,
                close_form: None,
                quit_new: None,
                created: None,
                reopen: Default::default(),
                recovery: Default::default(),
                checkpoints: None,
                tabs: ferritecad_ui::TabStrip {
                    tabs: &[],
                    available: Ok(()),
                },
                forms_scope: tab,
                model: Some((tab, w.tabs.model_navigation(w.sessions.tab()))),
                model_confirmation: self.confirmation.as_ref(),
                model_menu: &mut menu,
                model_available: available,
            },
            scene: crate::workspace::Scene {
                document: w.scene.document.as_deref(),
                edit_source: w.scene.edit_source.as_ref(),
                sketches: &w.scene.sketches,
                definitions: &[],
                chosen_row: None,
                offers: &[],
                described: None,
                solves: &[],
            },
        };
        let mut answers = crate::workspace::Answers::default();
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                events,
                ..Default::default()
            },
            |ui| answers = crate::workspace::compose(ui, &mut view),
        );
        output.textures_delta.clear();
        (output, answers.chosen)
    }

    pub(super) fn settle(&mut self, w: &mut Window) -> egui::FullOutput {
        for _ in 0..3 {
            self.frame(w, vec![]);
        }
        self.frame(w, vec![]).0
    }

    pub(super) fn press(&mut self, w: &mut Window, label: &str, last: bool) -> commands::Chosen {
        let output = self.settle(w);
        let at = rect(&output, label, last).center();
        self.frame(w, clicks(at)).1
    }

    pub(super) fn key(&mut self, w: &mut Window, key: egui::Key) {
        self.frame(w, vec![self::key(key, egui::Modifiers::NONE)]);
    }
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn clicks(at: egui::Pos2) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    ]
}

fn rect(output: &egui::FullOutput, words: &str, last: bool) -> egui::Rect {
    let matches: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == words => {
                Some(text.visual_bounding_rect())
            }
            _ => None,
        })
        .collect();
    *if last {
        matches.last()
    } else {
        matches.first()
    }
    .unwrap_or_else(|| panic!("missing visible text {words}"))
}

pub(super) fn text(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}
