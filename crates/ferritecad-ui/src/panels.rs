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
    /// The user wants to stop drawing what is chosen.
    pub hide: bool,
    /// The user wants to stop drawing everything except what is chosen.
    pub isolate: bool,
    /// The user wants everything drawn again.
    pub show_all: bool,
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
    /// One face of a body, named by what the document stores about it.
    ///
    /// Only durable terms reach here. A face the document does not name is not
    /// selectable as a face at all, so there is no case for one.
    Face {
        /// What the body is called, if the document called it anything.
        name: Option<&'a str>,
        /// The identifier the document stores for the body.
        object: &'a str,
        /// Every stored reference that names exactly this face, in the order
        /// the document stores them. More than one is normal and is shown as
        /// more than one: which of them is "the" name is not this window's
        /// decision to make.
        names: &'a [FaceName<'a>],
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

/// One durable name for a face, already turned into words by the caller.
///
/// Text rather than document types, for the same reason the rest of this
/// module deals in text: a panel that could name a `SemanticRole` would be a
/// panel that knows what a document is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceName<'a> {
    /// The identifier the document stores for the reference itself.
    pub reference: &'a str,
    /// The object holding the reference.
    pub owner: &'a str,
    /// The feature whose output it names.
    pub producer_feature: &'a str,
    /// The kind of entity it expects.
    pub expected_kind: &'a str,
    /// What the named entity is, semantically.
    pub role: &'a str,
    /// How many entities it selects, and which.
    pub rule: &'a str,
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
            Self::Face {
                name,
                object,
                names,
            } => {
                rows.push(("Kind", "Face".to_owned()));
                if let Some(name) = name {
                    rows.push(("Body", (*name).to_owned()));
                }
                rows.push(("Object", (*object).to_owned()));
                for face in *names {
                    rows.push(("Reference", face.reference.to_owned()));
                    rows.push(("Owner", face.owner.to_owned()));
                    rows.push(("Feature", face.producer_feature.to_owned()));
                    rows.push(("Entity", face.expected_kind.to_owned()));
                    rows.push(("Role", face.role.to_owned()));
                    rows.push(("Rule", face.rule.to_owned()));
                }
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
            // A list of definitions holds no faces, so this is what a face
            // would be called if one ever reached a row: the body it is part
            // of, said the same way.
            Self::Face { name, object, .. } => match name {
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
    hidden: &[bool],
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
                // Every definition, whether or not it is being drawn: a list
                // that dropped what was hidden would be a list with no way
                // back to it, and the hidden ones are exactly what a person
                // is looking for when they wonder where something went.
                let is_hidden = hidden.get(row).copied().unwrap_or(false);
                let summary = if is_hidden {
                    format!("{} · hidden", definition.summary())
                } else {
                    definition.summary()
                };
                // `selectable_label` draws the chosen one differently, which
                // is how a click in the viewport shows up here.
                // The same widget a visible row draws, disabled: greyed rather
                // than missing, so a person can see that the row is there and
                // that pressing it is not the way back.
                let response = ui.add_enabled(
                    !is_hidden,
                    egui::Button::selectable(chosen == Some(row), summary),
                );
                // A hidden row reports neither, and the rule is written here
                // rather than left to whether a disabled widget happens to
                // report a click: pressing it would choose geometry nobody can
                // see, and pointing at it would mark nothing on screen.
                if is_hidden {
                    continue;
                }
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
    /// Whether there is a chosen definition still being drawn. Choosing
    /// something already hidden is not a thing that can happen, so this is
    /// exactly "something is chosen".
    pub can_hide: bool,
    /// Whether anything is hidden and could be brought back.
    pub can_show_all: bool,
    /// Whether there is a chosen definition still being drawn with something
    /// else still drawn beside it. On its own it is isolated already.
    pub can_isolate: bool,
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
        // Beside the framing buttons, because they are the same kind of
        // thing: ways of seeing what is already there. Hiding removes a part
        // from the picture and from what a click can reach, and putting it
        // back is one press away and always says so.
        chosen.hide = ui
            .add_enabled(
                activity.can_hide,
                egui::Button::new(format!("Hide selected ({HIDE_KEY})")),
            )
            .clicked();
        // Between them, because it is the other way of saying "this one":
        // Hide removes what is chosen, Isolate removes everything else, and
        // Show all is the way back from either.
        chosen.isolate = ui
            .add_enabled(
                activity.can_isolate,
                egui::Button::new(format!("Isolate selected ({ISOLATE_KEY})")),
            )
            .clicked();
        chosen.show_all = ui
            .add_enabled(
                activity.can_show_all,
                egui::Button::new(format!("Show all ({SHOW_ALL_KEY})")),
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

/// The key that stops drawing what is chosen, printed on its button.
///
/// One place, read by the panel and by whatever binds the keys, for the same
/// reason [`FRAME_KEY`] is one place.
pub const HIDE_KEY: &str = "H";

/// The key that leaves only what is chosen on screen, printed on its button.
///
/// One place, read by the panel and by whatever binds the keys, for the same
/// reason [`FRAME_KEY`] is one place.
pub const ISOLATE_KEY: &str = "I";

/// The key that draws everything again, printed on its button.
///
/// `U` rather than `S`: `S` is one keystroke from the view keys a drawing
/// office reaches for, and this one undoes something.
pub const SHOW_ALL_KEY: &str = "U";

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
        let open = first.expect("the reference row was laid out");
        let centre = open.center();

        assert!(
            click_at(&context, centre).open,
            "nothing at the front of the toolbar opens a document"
        );

        // And the rest of the toolbar is not that button: pressing where the
        // views are must not open a file dialog. Found by walking the row
        // rather than by assuming how far along it they sit, so adding a
        // button to the toolbar cannot quietly turn this into a test that
        // presses empty space.
        let mut found_a_view = false;
        for step in 1..200 {
            let along = egui::Pos2::new(open.right() + step as f32 * 8.0, centre.y);
            let chosen = click_at(&context, along);
            assert!(!chosen.open, "something other than Open opened a document");
            if chosen.view.is_some() {
                found_a_view = true;
            }
        }
        assert!(found_a_view, "no point along the toolbar reached a view");
    }

    #[test]
    fn a_reading_can_be_given_up_on_and_nothing_else_can() {
        let context = egui::Context::default();
        let reading = Activity {
            line: "Opening part.fcad… 40%",
            progress: Some(0.4),
            can_frame_selection: false,
            can_frame_scene: false,
            can_hide: false,
            can_show_all: false,
            can_isolate: false,
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
                let _ = ui.add_enabled(
                    reading.can_hide,
                    egui::Button::new(format!("Hide selected ({HIDE_KEY})")),
                );
                let _ = ui.add_enabled(
                    reading.can_isolate,
                    egui::Button::new(format!("Isolate selected ({ISOLATE_KEY})")),
                );
                let _ = ui.add_enabled(
                    reading.can_show_all,
                    egui::Button::new(format!("Show all ({SHOW_ALL_KEY})")),
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

    fn a_face() -> Selected<'static> {
        Selected::Face {
            name: Some("Plate"),
            object: "018f2b7c-0000-7000-8000-000000000001",
            names: &[FaceName {
                reference: "018f2b7c-0000-7000-8000-0000000000a1",
                owner: "018f2b7c-0000-7000-8000-000000000001",
                producer_feature: "018f2b7c-0000-7000-8000-000000000002",
                expected_kind: "face",
                role: "Extrusion cap, end",
                rule: "Exactly this one",
            }],
        }
    }

    #[test]
    fn the_inspector_says_what_a_face_is_in_the_document_s_own_terms() {
        let rows = a_face().rows();
        let value = |label: &str| {
            rows.iter()
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("Kind"), Some("Face"));
        assert_eq!(value("Body"), Some("Plate"));
        assert_eq!(value("Role"), Some("Extrusion cap, end"));
        assert_eq!(value("Rule"), Some("Exactly this one"));
        assert_eq!(
            value("Reference"),
            Some("018f2b7c-0000-7000-8000-0000000000a1")
        );
        // Everything shown is something the document stores. Nothing here
        // could be written down and found to mean nothing an hour later.
        let shown = rows
            .iter()
            .map(|(label, value)| format!("{label} {value}"))
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        for word in TRANSIENT {
            if *word == "face" {
                // The word names the kind of thing chosen, which is the one
                // durable use of it: what is refused is a face *number*.
                continue;
            }
            assert!(
                !shown.contains(word),
                "a face inspector said {word}: {shown}"
            );
        }
    }

    #[test]
    fn a_face_with_several_names_shows_all_of_them() {
        let names = [
            FaceName {
                reference: "018f2b7c-0000-7000-8000-0000000000a1",
                owner: "018f2b7c-0000-7000-8000-000000000001",
                producer_feature: "018f2b7c-0000-7000-8000-000000000002",
                expected_kind: "face",
                role: "Extrusion cap, end",
                rule: "Exactly this one",
            },
            FaceName {
                reference: "018f2b7c-0000-7000-8000-0000000000a2",
                owner: "018f2b7c-0000-7000-8000-000000000001",
                producer_feature: "018f2b7c-0000-7000-8000-000000000002",
                expected_kind: "face",
                role: "Extrusion cap, end",
                rule: "Everything derived from 018f2b7c-0000-7000-8000-000000000003",
            },
        ];
        let rows = Selected::Face {
            name: None,
            object: "018f2b7c-0000-7000-8000-000000000001",
            names: &names,
        }
        .rows();

        // Both, in the order they were given. Showing one of two names would
        // be presenting storage order as a decision about which is right.
        let references: Vec<&str> = rows
            .iter()
            .filter(|(label, _)| *label == "Reference")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(references, vec![names[0].reference, names[1].reference]);
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
    fn hiding_and_showing_are_offered_exactly_when_they_would_do_something() {
        let context = egui::Context::default();
        let state = |can_hide, can_show_all| Activity {
            line: "part.fcad",
            progress: None,
            can_frame_selection: false,
            can_frame_scene: false,
            can_hide,
            can_show_all,
            can_isolate: false,
        };

        // Found by pressing along the real toolbar rather than by rebuilding
        // its layout here: a gate that assumed where a button sits would pass
        // while pressing empty space.
        let row = 12.0;
        let mut hide = None;
        let mut show = None;
        for step in 0..200 {
            let at = egui::Pos2::new(step as f32 * 8.0, row);
            let chosen = click_on(&context, at, state(true, true));
            if chosen.hide && hide.is_none() {
                hide = Some(at);
            }
            if chosen.show_all && show.is_none() {
                show = Some(at);
            }
        }
        let hide = hide.expect("the toolbar offers no way to hide what is chosen");
        let show = show.expect("the toolbar offers no way to show everything");
        assert_ne!(hide, show, "one button asked for both things");

        // A frame with the pointer away from the toolbar before each press.
        // egui decides a click from the frame before as well as this one, so
        // consecutive probes at different places would otherwise report what
        // the previous probe left behind – and a context that has never laid
        // the row out lays it out differently the first time.
        let press = |at, activity| {
            let _ = click_on(&context, egui::Pos2::new(2000.0, 2000.0), activity);
            click_on(&context, at, activity)
        };

        // Available exactly when there is something to do: a button that
        // reports a press with nothing to act on is a button that lies about
        // what the window can do.
        assert!(press(hide, state(true, false)).hide);
        assert!(!press(hide, state(false, false)).hide);
        assert!(press(show, state(false, true)).show_all);
        assert!(!press(show, state(false, false)).show_all);

        // And neither is the other.
        let pressed_hide = press(hide, state(true, true));
        assert!(pressed_hide.hide && !pressed_hide.show_all);
        let pressed_show = press(show, state(true, true));
        assert!(pressed_show.show_all && !pressed_show.hide);
        // Nor is either of them a view or a way to open a document.
        assert!(pressed_hide.view.is_none() && !pressed_hide.open);
        assert!(pressed_show.view.is_none() && !pressed_show.open);
    }

    #[test]
    fn a_hidden_row_is_shown_as_hidden_and_answers_nothing() {
        let context = egui::Context::default();
        let definitions = [body(), imported()];
        let rows = rows_of_with(&context, &definitions, &[false, false]);
        assert_eq!(rows.len(), 2);

        // Just inside the second row, in a place that is inside it whether or
        // not it is hidden: the mark a hidden row carries makes it wider, and
        // a gate that pressed the far end would be pressing empty space in one
        // of the two cases and proving nothing in the other.
        let at = egui::Pos2::new(rows[1].left() + 4.0, rows[1].center().y);
        let press = |hidden: &[bool]| {
            let _ = press_list(
                &context,
                egui::Pos2::new(2000.0, 2000.0),
                &definitions,
                None,
                hidden,
            );
            press_list(&context, at, &definitions, None, hidden)
        };

        // The same press, on the same row, in both states. The first half is
        // what makes the second half mean anything.
        let visible = press(&[false, false]);
        assert_eq!(
            visible.pressed,
            Some(1),
            "the gate pressed something that is not the row"
        );
        assert_eq!(visible.hovered, Some(1));

        let hidden = press(&[false, true]);
        assert_eq!(
            hidden.pressed, None,
            "a hidden row chose invisible geometry"
        );
        assert_eq!(
            hidden.hovered, None,
            "a hidden row asked about invisible geometry"
        );

        // Still in the list: a row that vanished when it was hidden would be a
        // row with no way back to it.
        assert_eq!(
            rows_of_with(&context, &definitions, &[false, true]).len(),
            2,
            "a hidden definition left the list"
        );

        // And it says so: the same row reads differently when it is hidden.
        let plain = list_text(&context, &definitions, &[false, false]);
        let marked = list_text(&context, &definitions, &[false, true]);
        assert_ne!(plain, marked, "nothing on screen says the row is hidden");
        assert!(marked.contains("hidden"));
    }

    /// Every glyph the list draws, as one string.
    fn list_text(context: &egui::Context, definitions: &[Selected<'_>], hidden: &[bool]) -> String {
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let _ = definitions_panel(ui, definitions, None, hidden);
        });
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        output.textures_delta.clear();
        text
    }

    #[test]
    fn isolating_is_offered_exactly_when_something_else_is_still_drawn() {
        let context = egui::Context::default();
        let state = |can_isolate| Activity {
            line: "part.fcad",
            progress: None,
            can_frame_selection: false,
            can_frame_scene: false,
            can_hide: true,
            can_show_all: true,
            can_isolate,
        };

        // Found by pressing along the real toolbar rather than by rebuilding
        // its layout here.
        let mut isolate = None;
        for step in 0..200 {
            let at = egui::Pos2::new(step as f32 * 8.0, 12.0);
            if click_on(&context, at, state(true)).isolate {
                isolate = Some(at);
                break;
            }
        }
        let isolate = isolate.expect("the toolbar offers no way to isolate what is chosen");

        // A frame with the pointer away before each press: egui decides a
        // click from the frame before as well as this one.
        let press = |activity| {
            let _ = click_on(&context, egui::Pos2::new(2000.0, 2000.0), activity);
            click_on(&context, isolate, activity)
        };

        let pressed = press(state(true));
        assert!(pressed.isolate);
        // One press, one request: the button beside it must not fire too.
        assert!(!pressed.hide && !pressed.show_all);
        assert!(pressed.view.is_none() && !pressed.open && !pressed.frame);

        assert!(
            !press(state(false)).isolate,
            "a button with nothing to isolate reported a press"
        );
    }

    #[test]
    fn the_isolate_button_prints_the_key_that_reaches_it() {
        // The panel prints `ISOLATE_KEY`, and whatever binds the keyboard
        // reads the same constant. A shortcut that drifts from its label is a
        // shortcut nobody can trust.
        assert!(!ISOLATE_KEY.is_empty());
        assert_ne!(ISOLATE_KEY, HIDE_KEY);
        assert_ne!(ISOLATE_KEY, SHOW_ALL_KEY);
        assert_ne!(ISOLATE_KEY, FRAME_KEY);
        assert_ne!(ISOLATE_KEY, FRAME_ALL_KEY);
        assert!(
            VIEWS.iter().all(|(_, _, key)| *key != ISOLATE_KEY),
            "the isolate key is also a view key"
        );

        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let _ = toolbar(
                ui,
                Activity {
                    line: "part.fcad",
                    progress: None,
                    can_frame_selection: false,
                    can_frame_scene: false,
                    can_hide: false,
                    can_show_all: false,
                    can_isolate: true,
                },
            );
        });
        let printed = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        output.textures_delta.clear();
        assert!(
            printed.contains(&format!("Isolate selected ({ISOLATE_KEY})")),
            "the button does not print the key that reaches it: {printed}"
        );
    }

    /// Runs the list once, with a click delivered at `at`.
    fn press_list(
        context: &egui::Context,
        at: egui::Pos2,
        definitions: &[Selected<'_>],
        chosen: Option<usize>,
        hidden: &[bool],
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
            rows = definitions_panel(ui, definitions, chosen, hidden);
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
        rows_of_with(context, definitions, &[])
    }

    /// The same, for a list in which some definitions are hidden.
    fn rows_of_with(
        context: &egui::Context,
        definitions: &[Selected<'_>],
        hidden: &[bool],
    ) -> Vec<egui::Rect> {
        let mut rects = Vec::new();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    for (row, definition) in definitions.iter().enumerate() {
                        let is_hidden = hidden.get(row).copied().unwrap_or(false);
                        let summary = if is_hidden {
                            format!("{} · hidden", definition.summary())
                        } else {
                            definition.summary()
                        };
                        rects.push(
                            ui.add_enabled(!is_hidden, egui::Button::selectable(false, summary))
                                .rect,
                        );
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

        let pressed = press_list(&context, rows[1].center(), &definitions, None, &[]);
        assert_eq!(
            pressed.pressed,
            Some(1),
            "the second definition could not be chosen from the list"
        );

        // And pressing nothing chooses nothing: a list that reported a press
        // every frame would reselect for as long as the window was open.
        let quiet = context.run_ui(egui::RawInput::default(), |ui| {
            assert_eq!(
                definitions_panel(ui, &definitions, Some(1), &[]).pressed,
                None
            );
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
                    asked = definitions_panel(ui, &definitions, None, &[]);
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
            press_list(&context, rows[0].center(), &definitions, None, &[]).pressed,
            Some(0)
        );
        assert_eq!(
            press_list(&context, rows[1].center(), &definitions, None, &[]).pressed,
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
            rows = definitions_panel(ui, &[], None, &[]);
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
            let _ = definitions_panel(ui, &[], None, &[]);
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
            can_hide: false,
            can_show_all: false,
            can_isolate: false,
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
            can_hide: false,
            can_show_all: false,
            can_isolate: false,
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
