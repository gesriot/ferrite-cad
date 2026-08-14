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
    /// The user wants to see what is chosen.
    pub frame: bool,
    /// The user wants to see the whole model.
    pub frame_all: bool,
    /// A definition the user picked out of the list, by its place in it.
    ///
    /// A position in this frame's list and nothing more: the caller turns it
    /// into an identity by asking the picture, and it means nothing once that
    /// picture is replaced.
    pub definition: Option<usize>,
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

    /// One line naming this definition, for a list of them.
    ///
    /// Name and portable identity together. Two definitions may be called the
    /// same thing – a document may hold two bodies called `Plate`, and two
    /// files may each describe a `Bracket` – so a line that gave only the name
    /// would offer no way to tell which row is which.
    pub fn summary(&self) -> String {
        match self {
            Self::Body { name, object } => match name {
                Some(name) => format!("{name} · {object}"),
                None => format!("Body · {object}"),
            },
            Self::Imported {
                name,
                definition_key,
                source_file,
                source,
                ..
            } => {
                let named = match name {
                    Some(name) => format!("{name} · {definition_key}"),
                    None => (*definition_key).to_owned(),
                };
                let described = match source_file {
                    // A useful hint, but not identity: unrelated files in
                    // different directories commonly share one basename.
                    Some(file) => format!("{named} · {file}"),
                    None => named,
                };
                // The key is scoped to this source. Always include both so
                // two unrelated files with the same name and ordinary STEP
                // numbering cannot produce rows that read alike.
                format!("{described} · {source}")
            }
        }
    }
}

/// Every definition in the picture, and the one that is chosen.
///
/// Read-only, like the inspector: pressing a row asks for a definition to be
/// chosen and changes nothing itself. Returns the row that was pressed, which
/// the caller turns into an identity by asking the picture – a row's position
/// is how a list reports a press and is not a name for anything.
///
/// One row per definition, in the order the picture packs them. Two imported
/// objects that draw the same definition contribute one row between them,
/// because they contribute one definition between them.
pub fn definitions_panel(
    ui: &mut egui::Ui,
    definitions: &[Selected<'_>],
    chosen: Option<usize>,
) -> Rows {
    let mut rows = Rows::default();
    if definitions.is_empty() {
        // A document with nothing in it says so. Offering an empty list with
        // no explanation would look like a list that failed to load.
        ui.label("No definitions");
        return rows;
    }

    egui::ScrollArea::vertical()
        .max_height(140.0)
        .show(ui, |ui| {
            for (row, definition) in definitions.iter().enumerate() {
                // `selectable_label` draws the chosen one differently, which
                // is how a click in the viewport shows up here.
                let response = ui.selectable_label(chosen == Some(row), definition.summary());
                if response.clicked() {
                    rows.pressed = Some(row);
                }
                // Separate from pressing, and deliberately so: pointing at a
                // row says which geometry it is, and choosing it is a decision
                // the user has not made yet.
                if response.hovered() {
                    rows.hovered = Some(row);
                }
            }
        });
    rows
}

/// What the list of definitions was asked while it was on screen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Rows {
    /// A row that was pressed, which is a choice.
    pub pressed: Option<usize>,
    /// A row the pointer is over, which is a question and not a choice.
    pub hovered: Option<usize>,
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
    /// Whether the picture can say where what is chosen actually is.
    pub can_frame_selection: bool,
    /// Whether the picture has any extent at all. A document with nothing in
    /// it has nowhere to point a camera, and says so by offering nothing.
    pub can_frame_scene: bool,
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

        // Offered only when there is something to show, and disabled rather
        // than hidden: a control that comes and goes is harder to find than
        // one that is plainly not available yet.
        chosen.frame = ui
            .add_enabled(
                activity.can_frame_selection,
                egui::Button::new(format!("Frame selected ({FRAME_KEY})")),
            )
            .clicked();
        // Beside it, because the two answer the same question about different
        // things: show me this, and show me everything. Available whenever the
        // picture has an extent, which is what makes it the way back from a
        // camera that has wandered off the model.
        chosen.frame_all = ui
            .add_enabled(
                activity.can_frame_scene,
                egui::Button::new(format!("Frame all ({FRAME_ALL_KEY})")),
            )
            .clicked();
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
/// The key that shows what is chosen, printed on its button.
///
/// One place, read by the panel and by whatever binds the keys, for the same
/// reason [`VIEWS`] is one place: a shortcut printed on a button that does
/// something else is worse than no shortcut at all.
pub const FRAME_KEY: &str = "F";

/// The key that shows the whole model, printed on its button.
///
/// Distinct from [`FRAME_KEY`] and from every view key, and checked to be: two
/// actions on one key is a shortcut whose meaning depends on state nobody can
/// see.
pub const FRAME_ALL_KEY: &str = "A";

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
            can_frame_selection: false,
            can_frame_scene: false,
        };

        // Where the Cancel button is: after the views, so the row up to it is
        // laid out the same either way and the difference is this button.
        let mut cancel = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal(|ui| {
                let _ = ui.button("Open…");
                ui.separator();
                let _ = ui.add_enabled(
                    reading.can_frame_selection,
                    egui::Button::new(format!("Frame selected ({FRAME_KEY})")),
                );
                let _ = ui.add_enabled(
                    reading.can_frame_scene,
                    egui::Button::new(format!("Frame all ({FRAME_ALL_KEY})")),
                );
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

    /// Runs the list once, with a click delivered at `at`.
    fn press_list(
        context: &egui::Context,
        at: egui::Pos2,
        definitions: &[Selected<'_>],
        chosen: Option<usize>,
    ) -> Rows {
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

        let mut rows = Rows::default();
        let mut output = context.run_ui(input, |ui| {
            rows = definitions_panel(ui, definitions, chosen);
        });
        output.textures_delta.clear();
        rows
    }

    /// Where the panel lays each of its rows out.
    ///
    /// The same structure the panel builds, in a `Ui` that has had nothing
    /// else put in it – which is how the panel is drawn when it is pressed
    /// below. Measuring a second copy underneath would give the coordinates of
    /// the copy.
    fn rows_of(context: &egui::Context, definitions: &[Selected<'_>]) -> Vec<egui::Rect> {
        let mut rects = Vec::new();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    for definition in definitions {
                        rects.push(ui.selectable_label(false, definition.summary()).rect);
                    }
                });
        });
        output.textures_delta.clear();
        rects
    }

    #[test]
    fn a_definition_nothing_is_pointing_at_can_still_be_chosen() {
        let context = egui::Context::default();
        // Two definitions, and the second one is the one a click could not
        // reach: hidden behind the first, too small to hit, or off screen.
        // A list does not care where anything is drawn.
        let definitions = [body(), imported()];

        let rows = rows_of(&context, &definitions);
        assert_eq!(rows.len(), 2, "the list did not lay out one row each");

        let pressed = press_list(&context, rows[1].center(), &definitions, None);
        assert_eq!(
            pressed.pressed,
            Some(1),
            "the second definition could not be chosen from the list"
        );

        // And pressing nothing chooses nothing: a list that reported a press
        // every frame would reselect for as long as the window was open.
        let quiet = context.run_ui(egui::RawInput::default(), |ui| {
            assert_eq!(definitions_panel(ui, &definitions, Some(1)).pressed, None);
        });
        let mut quiet = quiet;
        quiet.textures_delta.clear();
    }

    #[test]
    fn pointing_at_a_row_asks_about_it_without_choosing_it() {
        let context = egui::Context::default();
        let definitions = [body(), imported()];
        let rows = rows_of(&context, &definitions);

        // Moving over a row, with no button involved.
        let over = |at: egui::Pos2| {
            let mut asked = Rows::default();
            let mut output = context.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::PointerMoved(at)],
                    ..Default::default()
                },
                |ui| {
                    asked = definitions_panel(ui, &definitions, None);
                },
            );
            output.textures_delta.clear();
            asked
        };

        let asked = over(rows[1].center());
        assert_eq!(asked.hovered, Some(1), "pointing at a row asked nothing");
        assert_eq!(
            asked.pressed, None,
            "pointing at a row chose it, so nothing could be looked at without \
             being selected"
        );

        // Somewhere else in the list is a different question, and off the list
        // is no question at all.
        assert_eq!(over(rows[0].center()).hovered, Some(0));
        assert_eq!(
            over(egui::Pos2::new(
                rows[0].center().x,
                rows[0].center().y + 400.0
            ))
            .hovered,
            None
        );
    }

    #[test]
    fn two_definitions_with_one_name_are_two_rows() {
        let context = egui::Context::default();
        // Legal, and the case a name-keyed list would collapse: same name,
        // different portable identity.
        let definitions = [
            Selected::Body {
                name: Some("Plate"),
                object: "018f2b7c-0000-7000-8000-000000000001",
            },
            Selected::Body {
                name: Some("Plate"),
                object: "018f2b7c-0000-7000-8000-000000000002",
            },
        ];

        let rows = rows_of(&context, &definitions);
        assert_eq!(rows.len(), 2, "two definitions were shown as one row");
        assert_ne!(
            definitions[0].summary(),
            definitions[1].summary(),
            "two rows read alike, so nothing on screen tells them apart"
        );

        // Each is chosen on its own.
        assert_eq!(
            press_list(&context, rows[0].center(), &definitions, None).pressed,
            Some(0)
        );
        assert_eq!(
            press_list(&context, rows[1].center(), &definitions, None).pressed,
            Some(1)
        );
    }

    #[test]
    fn the_same_imported_name_key_and_file_still_show_which_source_is_which() {
        // Two unrelated files may share a basename and use the same ordinary
        // PRODUCT_DEFINITION number. Their source identities are the only
        // portable fact that distinguishes them, so leaving the source out of
        // the summary makes two different rows read exactly alike.
        let first = Selected::Imported {
            name: Some("Bracket"),
            source_file: Some("part.step"),
            source: "018f2b7c-0000-7000-8000-000000000001",
            definition_key: "step.product_definition#5",
            solids: Some(1),
        };
        let second = Selected::Imported {
            name: Some("Bracket"),
            source_file: Some("part.step"),
            source: "018f2b7c-0000-7000-8000-000000000002",
            definition_key: "step.product_definition#5",
            solids: Some(1),
        };

        assert_ne!(
            first.summary(),
            second.summary(),
            "two imported definitions with different identities read alike"
        );
    }

    #[test]
    fn an_empty_picture_says_so_and_offers_nothing_to_choose() {
        let context = egui::Context::default();
        let mut rows = Rows {
            pressed: Some(7),
            hovered: Some(7),
        };
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            rows = definitions_panel(ui, &[], None);
        });
        output.textures_delta.clear();
        assert_eq!(rows, Rows::default(), "an empty list invented a choice");

        // And it says something rather than drawing an empty strip.
        let mut empty = context.run_ui(egui::RawInput::default(), |_| {});
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
        let mut said = context.run_ui(egui::RawInput::default(), |ui| {
            let _ = definitions_panel(ui, &[], None);
        });
        assert!(vertices(&said) > vertices(&empty));
        said.textures_delta.clear();
        empty.textures_delta.clear();
    }

    #[test]
    fn a_row_never_names_anything_that_dies_with_the_picture() {
        for definition in [body(), imported()] {
            let text = definition.summary().to_lowercase();
            for word in TRANSIENT {
                assert!(
                    !text.contains(word),
                    "a row would show {word:?}: {}",
                    definition.summary()
                );
            }
            assert!(
                !definition.summary().contains('/') && !definition.summary().contains('\\'),
                "a row would show a path: {}",
                definition.summary()
            );
        }
    }

    #[test]
    fn showing_what_is_chosen_is_offered_only_when_there_is_somewhere_to_go() {
        let context = egui::Context::default();
        let can = |can_frame_selection| Activity {
            line: "part.fcad",
            progress: None,
            can_frame_selection,
            can_frame_scene: false,
        };

        // Where the button is, laid out exactly as the toolbar lays it out.
        let mut frame = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal(|ui| {
                let _ = ui.button("Open…");
                ui.separator();
                frame = Some(
                    ui.add_enabled(
                        true,
                        egui::Button::new(format!("Frame selected ({FRAME_KEY})")),
                    )
                    .rect,
                );
            });
        });
        output.textures_delta.clear();
        let centre = frame.expect("the reference row was laid out").center();

        assert!(
            click_on(&context, centre, can(true)).frame,
            "nothing there asks to see what is chosen"
        );

        // With nothing to show, the same press asks for nothing: an action
        // that cannot be carried out must not report that it was.
        assert!(
            !click_on(&context, centre, can(false)).frame,
            "an unavailable action reported that it happened"
        );

        // And an untouched toolbar asks for nothing either way.
        let mut quiet = Chosen::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            quiet = toolbar(ui, can(true));
        });
        output.textures_delta.clear();
        assert!(!quiet.frame);
    }

    #[test]
    fn showing_everything_is_offered_only_when_there_is_anything_to_show() {
        let context = egui::Context::default();
        let with = |can_frame_scene| Activity {
            line: "part.fcad",
            progress: None,
            can_frame_selection: false,
            can_frame_scene,
        };

        // Where the button is, laid out exactly as the toolbar lays it out.
        let mut all = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            ui.horizontal(|ui| {
                let _ = ui.button("Open…");
                ui.separator();
                let _ = ui.add_enabled(
                    false,
                    egui::Button::new(format!("Frame selected ({FRAME_KEY})")),
                );
                all = Some(
                    ui.add_enabled(
                        true,
                        egui::Button::new(format!("Frame all ({FRAME_ALL_KEY})")),
                    )
                    .rect,
                );
            });
        });
        output.textures_delta.clear();
        let centre = all.expect("the reference row was laid out").center();

        assert!(
            click_on(&context, centre, with(true)).frame_all,
            "nothing there asks to see the whole model"
        );

        // A picture with nothing in it has nowhere to point a camera, and
        // pressing where the button is must not pretend otherwise.
        assert!(
            !click_on(&context, centre, with(false)).frame_all,
            "an empty picture offered somewhere to go"
        );

        // Showing everything is not showing what is chosen: one press is one
        // request, and not both.
        assert!(!click_on(&context, centre, with(true)).frame);

        let mut quiet = Chosen::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            quiet = toolbar(ui, with(true));
        });
        output.textures_delta.clear();
        assert!(!quiet.frame_all, "an untouched toolbar asked to reframe");
    }

    #[test]
    fn the_key_printed_on_the_button_is_the_one_this_crate_names() {
        // The panel prints `FRAME_KEY` and whatever binds the keyboard reads
        // the same constant, so a shortcut cannot end up printed on a button
        // that does something else.
        assert!(!FRAME_KEY.is_empty());
        assert!(!FRAME_ALL_KEY.is_empty());
        assert!(
            VIEWS.iter().all(|(_, _, key)| *key != FRAME_KEY),
            "the framing key is also printed on a view button"
        );
        assert!(
            VIEWS.iter().all(|(_, _, key)| *key != FRAME_ALL_KEY),
            "the whole-model key is also printed on a view button"
        );
        assert_ne!(
            FRAME_KEY, FRAME_ALL_KEY,
            "one key would mean two things depending on what is chosen"
        );
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
