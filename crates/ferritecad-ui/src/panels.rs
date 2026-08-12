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
}

/// The toolbar: a document to open, and the directions a drawing would name.
///
/// Returns what was pressed. The order of the views is the one a drawing
/// office reads in, and the keyboard shortcuts beside each name are the same
/// ones the window binds, so the panel documents them rather than making the
/// user find out.
/// The toolbar: a document to open, the directions a drawing would name, and
/// what the window has to say about the document it is showing.
///
/// `status` is a finished sentence. What the states are and which one applies
/// is decided where the loading happens; drawing it here rather than composing
/// it here is what keeps one account of what is going on.
pub fn toolbar(ui: &mut egui::Ui, status: &str) -> Chosen {
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

        // Last, and given whatever room is left: it is the one thing here
        // that can be any length, and a long file name must push no button
        // off the toolbar.
        ui.separator();
        ui.add(egui::Label::new(status).truncate());
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
            chosen = toolbar(ui, "");
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
            chosen = toolbar(ui, "");
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
