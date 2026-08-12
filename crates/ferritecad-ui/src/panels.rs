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

/// The directions a drawing would name, as buttons.
///
/// Returns the one that was pressed, or `None`. The order is the one a
/// drawing office reads in, and the keyboard shortcuts beside each name are
/// the same ones the window binds, so the panel documents them rather than
/// making the user find out.
pub fn views_panel(ui: &mut egui::Ui) -> Option<StandardView> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.label("View");
        for (view, name, key) in VIEWS {
            if ui.button(format!("{name} ({key})")).clicked() {
                chosen = Some(*view);
            }
        }
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
