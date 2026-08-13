// SPDX-License-Identifier: MIT
//! What the interface offers, drawn but not owned.
//!
//! Each panel is a function over a [`egui::Ui`] that returns what the user
//! asked for, and nothing else. No panel holds a camera, a document or a
//! renderer, and none of them applies what it returns: the caller does that
//! through the reducer, which is where the rules about what a request means
//! already live and are already tested.
//!
//! That split is what keeps a panel from becoming a second place where the
//! camera moves.

use ferritecad_viewport::StandardView;

/// What the toolbar was asked for while it was on screen.
///
/// A record of what was pressed, applied by nobody here. Two fields rather
/// than one enum because a frame in which the user pressed a view button is
/// not a frame in which they cannot also have pressed anything else, and a
/// panel that could only report one thing would decide which to drop.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Chosen {
    /// A direction to look from.
    pub view: Option<StandardView>,
    /// The user wants to open a different document.
    pub open: bool,
    /// The user has stopped waiting for the document being read.
    pub cancel: bool,
}

/// What is chosen, in the words a document would use for it.
///
/// Deliberately not the scene's own type: a panel has no business knowing what
/// a document or a kernel is, and the conversion happens where both are
/// already in hand. What it does know is which facts exist, so a caller cannot
/// hand it a number that means something only to this frame.
///
/// # Portable terms only
///
/// Everything the application assigns to these roles outlives the picture it
/// was read from. There is no role for a pick, mesh index, face index, shape
/// handle, session or occurrence position. The strings are borrowed display
/// text rather than proof of their own provenance, so the application boundary
/// remains responsible for supplying the source's leaf file name rather than
/// its path. See [`Selected::rows`], which is what actually reaches a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selected<'a> {
    /// A body of the open document.
    Body {
        name: Option<&'a str>,
        /// The identifier the document stores for it.
        object: &'a str,
    },
    /// A definition inside a file this document imported.
    Imported {
        name: Option<&'a str>,
        /// The file it came from, already reduced to a name by the caller.
        source_file: Option<&'a str>,
        /// The identifier the document gave those bytes.
        source: &'a str,
        /// The name the file itself gave the definition.
        definition_key: &'a str,
        solids: Option<u32>,
    },
}

impl Selected<'_> {
    /// The lines to put on screen, in order.
    ///
    /// Built as data rather than drawn directly so that what a person will
    /// read can be examined without a window. A fact the loader did not find
    /// is left out rather than shown as blank or invented.
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = Vec::new();
        match self {
            Self::Body { name, object } => {
                rows.push(("Kind", "Body".to_owned()));
                if let Some(name) = name {
                    rows.push(("Name", (*name).to_owned()));
                }
                rows.push(("Object", (*object).to_owned()));
            }
            Self::Imported {
                name,
                source_file,
                source,
                definition_key,
                solids,
            } => {
                rows.push(("Kind", "Imported definition".to_owned()));
                if let Some(name) = name {
                    rows.push(("Name", (*name).to_owned()));
                }
                if let Some(file) = source_file {
                    rows.push(("File", (*file).to_owned()));
                }
                rows.push(("Source", (*source).to_owned()));
                // The file's own name for it, which is what makes it findable
                // again in those bytes and nowhere else.
                rows.push(("Definition", (*definition_key).to_owned()));
                if let Some(solids) = solids {
                    rows.push(("Solids", solids.to_string()));
                }
            }
        }
        rows
    }
}

/// Shows what is chosen, and nothing when nothing is.
///
/// Read-only. Choosing happens in the viewport and clearing happens by
/// clicking away from the model; a panel that could change the selection would
/// be a second place where it is decided.
pub fn selection_inspector(ui: &mut egui::Ui, selected: Option<Selected<'_>>) {
    let Some(selected) = selected else {
        // Said rather than left blank: an empty strip is indistinguishable
        // from an interface that has stopped working.
        ui.label("Nothing selected");
        return;
    };

    egui::Grid::new("ferritecad selection")
        .num_columns(2)
        .show(ui, |ui| {
            for (label, value) in selected.rows() {
                ui.label(label);
                ui.add(egui::Label::new(value).truncate());
                ui.end_row();
            }
        });
}

/// What the window is doing, as far as the toolbar needs to know.
///
/// `line` is a finished sentence and `progress` is how far through a reading
/// it is. `None` there is not "nought per cent": it is nothing being read,
/// which is what decides whether there is anything to offer to stop.
///
/// Not `#[non_exhaustive]`, unlike [`Chosen`]: this one is an argument, and a
/// caller that cannot name every field cannot pass one at all.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Activity<'a> {
    pub line: &'a str,
    pub progress: Option<f32>,
}

/// The toolbar: a document to open, the directions a drawing would name, what
/// the window is doing, and a way to stop it.
///
/// Returns what was pressed. The order of the views is the one a drawing
/// office reads in, and the keyboard shortcuts beside each name are the same
/// ones the window binds, so the panel documents them rather than making the
/// user find out.
///
/// [`Activity::line`] arrives as a finished sentence. What the states are and
/// which one applies is decided where the loading happens; drawing it here
/// rather than composing it here is what keeps one account of what is going
/// on.
pub fn toolbar(ui: &mut egui::Ui, activity: Activity<'_>) -> Chosen {
    let mut chosen = Chosen::default();
    ui.horizontal(|ui| {
        // First, and separated: opening replaces everything else on screen,
        // where the buttons after it only change where it is seen from.
        chosen.open = ui.button("Open…").clicked();
        ui.separator();

        ui.label("View");
        for (view, name, key) in VIEWS {
            if ui.button(format!("{name} ({key})")).clicked() {
                chosen.view = Some(*view);
            }
        }

        // Last, and given whatever room is left: the line is the one thing
        // here that can be any length, and a long file name must push no
        // button off the toolbar.
        ui.separator();
        if let Some(fraction) = activity.progress {
            // Offered only while there is something to stop. A button that is
            // there all the time and does nothing most of the time teaches
            // people not to trust it.
            chosen.cancel = ui.button("Cancel").clicked();
            ui.add(egui::ProgressBar::new(fraction).desired_width(80.0));
        }
        ui.add(egui::Label::new(activity.line).truncate());
    });
    chosen
}

/// Every standard view, with what to call it and the key that reaches it.
///
/// One list, used by the panel here and available to whatever binds the keys,
/// so a shortcut cannot end up printed on a button that does something else.
pub const VIEWS: &[(StandardView, &str, &str)] = &[
    (StandardView::Front, "Front", "1"),
    (StandardView::Back, "Back", "2"),
    (StandardView::Left, "Left", "3"),
    (StandardView::Right, "Right", "4"),
    (StandardView::Top, "Top", "5"),
    (StandardView::Bottom, "Bottom", "6"),
    (StandardView::Isometric, "Iso", "7"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Lays out one string and reports how wide it came out.
    ///
    /// Zero for a build with no font: `egui` lays the text out against whatever
    /// typefaces are installed, and with none installed there is nothing to
    /// measure. That is exactly the failure this guards against, and it is a
    /// quiet one – no error, no panic, just an interface with no words on it.
    fn text_width(context: &egui::Context, text: &str) -> f32 {
        context.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(
                    text.to_owned(),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                )
                .size()
                .x
        })
    }

    #[test]
    fn this_build_can_draw_words() {
        let context = egui::Context::default();
        // A frame has to have run for the fonts to be loaded. Its texture
        // deltas are the font atlas, and epaint refuses to have them dropped
        // unapplied – a renderer that ignored them would show blank glyphs.
        let mut output = context.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();

        let width = text_width(&context, "Front");
        assert!(
            width > 0.0,
            "no font is installed, so every label in this build is blank"
        );

        // And it is really measuring the glyphs rather than returning a
        // constant: a longer word is wider than a shorter one.
        assert!(
            text_width(&context, "Isometric") > width,
            "text measurement does not depend on the text"
        );
    }

    #[test]
    fn a_label_reaches_the_geometry_that_would_be_drawn() {
        let context = egui::Context::default();

        let mut with_text = context.run_ui(egui::RawInput::default(), |ui| {
            ui.label("Front");
        });
        with_text.textures_delta.clear();
        let mut empty = context.run_ui(egui::RawInput::default(), |_| {});
        empty.textures_delta.clear();

        // End to end, past layout and into what a renderer would upload. A
        // font that laid out but produced no glyphs would pass the measurement
        // above and still draw nothing.
        let vertices = |output: egui::FullOutput| {
            context
                .tessellate(output.shapes, 1.0)
                .into_iter()
                .map(|primitive| match primitive.primitive {
                    egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                    egui::epaint::Primitive::Callback(_) => 0,
                })
                .sum::<usize>()
        };

        let drawn = vertices(with_text);
        assert!(
            drawn > vertices(empty),
            "a label produced no more geometry than an empty frame"
        );
    }

    #[test]
    fn the_toolbar_asks_for_nothing_until_it_is_pressed() {
        let context = egui::Context::default();
        let mut chosen = Chosen::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            chosen = toolbar(ui, Activity::default());
        });
        output.textures_delta.clear();

        // A frame in which the user did nothing must ask for nothing. A
        // toolbar that reported a press every frame would reopen the file
        // dialog for as long as the window was on screen.
        assert_eq!(chosen, Chosen::default());
        assert!(!chosen.open);
        assert!(chosen.view.is_none());
    }

    /// Runs the toolbar once, with a click delivered at `at`.
    ///
    /// A press and a release at one place, which is what a click is. Nothing
    /// simulates the toolbar itself: the widget under that point is whichever
    /// one really got laid out there.
    fn click_at(context: &egui::Context, at: egui::Pos2) -> Chosen {
        click_on(context, at, Activity::default())
    }

    /// The same, for a window that is in the middle of reading something.
    fn click_on(context: &egui::Context, at: egui::Pos2, activity: Activity<'_>) -> Chosen {
        let input = egui::RawInput {
            events: vec![
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
            ],
            ..Default::default()
        };

        let mut chosen = Chosen::default();
        let mut output = context.run_ui(input, |ui| {
            chosen = toolbar(ui, activity);
        });
        output.textures_delta.clear();
        chosen
    }

    #[test]
    fn the_way_to_open_a_document_is_where_it_can_be_pressed() {
        let context = egui::Context::default();

        // Laid out first in the row, so the first thing the pointer meets is
        // it. A toolbar whose button was never added would report "not
        // pressed" for ever and look, to every other test here, identical to
        // one that works.
        let mut first = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal(|ui| {
                first = Some(ui.button("Open…").rect);
            });
        });
        output.textures_delta.clear();
        let centre = first.expect("the reference row was laid out").center();

        assert!(
            click_at(&context, centre).open,
            "nothing at the front of the toolbar opens a document"
        );

        // And the rest of the toolbar is not that button: pressing where the
        // views are must not open a file dialog.
        let elsewhere = egui::Pos2::new(centre.x + 400.0, centre.y);
        let chosen = click_at(&context, elsewhere);
        assert!(!chosen.open, "a view button opened the file dialog");
        assert!(chosen.view.is_some(), "the point tested hit no view button");
    }

    #[test]
    fn a_reading_can_be_given_up_on_and_nothing_else_can() {
        let context = egui::Context::default();
        let reading = Activity {
            line: "Opening part.fcad… 40%",
            progress: Some(0.4),
        };

        // Where the Cancel button is: after the views, so the row up to it is
        // laid out the same either way and the difference is this button.
        let mut cancel = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal(|ui| {
                let _ = ui.button("Open…");
                ui.separator();
                ui.label("View");
                for (_, name, key) in VIEWS {
                    let _ = ui.button(format!("{name} ({key})"));
                }
                ui.separator();
                cancel = Some(ui.button("Cancel").rect);
            });
        });
        output.textures_delta.clear();
        let centre = cancel.expect("the reference row was laid out").center();

        assert!(
            click_on(&context, centre, reading).cancel,
            "nothing there gives up on the reading"
        );

        // And with nothing being read there is nothing to give up on, so that
        // same place must not be a button that quietly does something else.
        let idle = click_on(&context, centre, Activity::default());
        assert!(
            !idle.cancel,
            "a window with nothing to stop offered to stop it"
        );
        assert!(idle.view.is_none() && !idle.open);
    }

    /// The words a viewport uses about one frame, none of which survives it.
    ///
    /// A person reading any of these could write it down and find it means
    /// nothing an hour later, which is the whole reason the inspector deals
    /// in what a document could store.
    const TRANSIENT: &[&str] = &[
        "pick",
        "mesh",
        "face",
        "edge",
        "handle",
        "session",
        "occurrence",
        "instance",
        "index",
        "snapshot",
    ];

    fn body() -> Selected<'static> {
        Selected::Body {
            name: Some("Plate"),
            object: "018f2b7c-0000-7000-8000-000000000001",
        }
    }

    fn imported() -> Selected<'static> {
        Selected::Imported {
            name: Some("Cube"),
            source_file: Some("03-nested-assembly.step"),
            source: "018f2b7c-0000-7000-8000-0000000000ff",
            definition_key: "step.product_definition#58",
            solids: Some(1),
        }
    }

    #[test]
    fn the_inspector_says_what_a_body_is_in_the_document_s_own_terms() {
        let rows = body().rows();
        let value = |label: &str| {
            rows.iter()
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("Kind"), Some("Body"));
        assert_eq!(value("Name"), Some("Plate"));
        assert_eq!(
            value("Object"),
            Some("018f2b7c-0000-7000-8000-000000000001")
        );

        // A body has no file and no counted solids, and inventing rows for
        // them would be answering questions nobody asked of it.
        assert_eq!(value("File"), None);
        assert_eq!(value("Solids"), None);

        // A nameless body is still a body. Nothing is invented for it.
        let rows = Selected::Body {
            name: None,
            object: "018f2b7c-0000-7000-8000-000000000001",
        }
        .rows();
        assert!(rows.iter().all(|(label, _)| *label != "Name"));
    }

    #[test]
    fn the_inspector_says_what_an_imported_definition_is() {
        let rows = imported().rows();
        let value = |label: &str| {
            rows.iter()
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("Kind"), Some("Imported definition"));
        assert_eq!(value("Name"), Some("Cube"));
        assert_eq!(value("File"), Some("03-nested-assembly.step"));
        assert_eq!(
            value("Source"),
            Some("018f2b7c-0000-7000-8000-0000000000ff")
        );
        assert_eq!(value("Definition"), Some("step.product_definition#58"));
        assert_eq!(value("Solids"), Some("1"));

        // The key alone is not the identity, and the panel says so by always
        // showing the file it belongs to beside it: `#58` in another file is
        // another definition entirely.
        assert!(rows.iter().any(|(label, _)| *label == "Source"));
    }

    #[test]
    fn the_inspector_never_puts_a_transient_word_on_screen() {
        for selected in [body(), imported()] {
            for (label, value) in selected.rows() {
                let text = format!("{label} {value}").to_lowercase();
                for word in TRANSIENT {
                    assert!(
                        !text.contains(word),
                        "the inspector would show {word:?} in {label:?}: {value:?}"
                    );
                }
                assert!(
                    !value.contains('/') && !value.contains('\\'),
                    "the inspector would show a path in {label:?}: {value:?}"
                );
            }
        }
    }

    #[test]
    fn an_empty_selection_says_so_rather_than_showing_nothing() {
        let context = egui::Context::default();

        let mut empty = context.run_ui(egui::RawInput::default(), |ui| {
            selection_inspector(ui, None);
        });
        let mut chosen = context.run_ui(egui::RawInput::default(), |ui| {
            selection_inspector(ui, Some(imported()));
        });
        let mut nothing_at_all = context.run_ui(egui::RawInput::default(), |_| {});

        let vertices = |output: &egui::FullOutput| {
            context
                .clone()
                .tessellate(output.shapes.clone(), 1.0)
                .into_iter()
                .map(|primitive| match primitive.primitive {
                    egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                    egui::epaint::Primitive::Callback(_) => 0,
                })
                .sum::<usize>()
        };

        // Something is drawn either way, and more of it when there is more to
        // say. An interface that went blank on an empty selection would be
        // indistinguishable from one that had stopped working.
        assert!(vertices(&empty) > vertices(&nothing_at_all));
        assert!(vertices(&chosen) > vertices(&empty));

        empty.textures_delta.clear();
        chosen.textures_delta.clear();
        nothing_at_all.textures_delta.clear();
    }

    #[test]
    fn every_standard_view_is_offered_and_named_once() {
        // A panel that quietly dropped one would leave a direction reachable
        // only by a key nobody was told about.
        let mut seen: Vec<StandardView> = Vec::new();
        for (view, name, key) in VIEWS {
            assert!(!name.is_empty(), "{view:?} has no name");
            assert!(!key.is_empty(), "{view:?} has no shortcut");
            assert!(!seen.contains(view), "{view:?} is offered twice");
            seen.push(*view);
        }
        assert_eq!(seen.len(), 7);

        let mut keys: Vec<&str> = VIEWS.iter().map(|(_, _, key)| *key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), VIEWS.len(), "two views share a shortcut");
    }
}
