// SPDX-License-Identifier: MIT
//! Shared constraint marker and dimension ink for both sketch editors.
use super::dimension;
pub(crate) const SELECTED_COLOUR: egui::Color32 = egui::Color32::from_rgb(90, 175, 255);
pub(crate) const SIDE_A_COLOUR: egui::Color32 = egui::Color32::from_rgb(255, 165, 60);
pub(crate) const SIDE_B_COLOUR: egui::Color32 = egui::Color32::from_rgb(64, 224, 160);
pub(crate) const HOVER_COLOUR: egui::Color32 = egui::Color32::LIGHT_YELLOW;
/// §31L: a dimension label's text, and the border and text of a label whose saved rule has a
/// replacement prepared.
pub(crate) const LABEL_COLOUR: egui::Color32 = egui::Color32::from_rgb(130, 220, 230);
pub(crate) const PENDING_COLOUR: egui::Color32 = egui::Color32::from_rgb(255, 205, 80);
/// §31N: a rule marker's words and the tint of its box: a look of its own, apart from the numeric
/// labels' black boxes.
pub(crate) const MARKER_COLOUR: egui::Color32 = egui::Color32::from_rgb(210, 180, 255);
pub(crate) const MARKER_FILL: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(34, 24, 58, 225);
pub(crate) const LABEL_FONT: f32 = 11.;

/// How a marker looks now: its border and the colour of its words. As for the numeric labels, the
/// selected rule's marker stays blue under the pointer; a prepared removal is amber.
pub(crate) fn marker_look(
    chosen: bool,
    hot: bool,
    removed: bool,
    live: bool,
) -> (egui::Stroke, egui::Color32) {
    let border = if chosen {
        egui::Stroke::new(2., SELECTED_COLOUR)
    } else if hot {
        egui::Stroke::new(2., HOVER_COLOUR)
    } else if removed {
        egui::Stroke::new(1.5, PENDING_COLOUR)
    } else if live {
        egui::Stroke::new(1., egui::Color32::from_gray(150))
    } else {
        egui::Stroke::new(1., egui::Color32::from_gray(80))
    };
    let words = if removed {
        PENDING_COLOUR
    } else if live || chosen {
        MARKER_COLOUR
    } else {
        egui::Color32::from_rgb(150, 130, 185)
    };
    (border, words)
}

/// How a label looks now: its border and the colour of its words. The label of the selected curve
/// (or angle) stays blue under the pointer: hovering never hides what is selected.
pub(crate) fn label_look(
    chosen: bool,
    hot: bool,
    pending: bool,
    live: bool,
) -> (egui::Stroke, egui::Color32) {
    let border = if chosen {
        egui::Stroke::new(2., SELECTED_COLOUR)
    } else if hot {
        egui::Stroke::new(2., HOVER_COLOUR)
    } else if pending {
        egui::Stroke::new(1.5, PENDING_COLOUR)
    } else if live {
        egui::Stroke::new(1., egui::Color32::from_gray(150))
    } else {
        egui::Stroke::new(1., egui::Color32::from_gray(80))
    };
    let words = if pending {
        PENDING_COLOUR
    } else if live || chosen {
        LABEL_COLOUR
    } else {
        egui::Color32::from_gray(190)
    };
    (border, words)
}

/// One label: a dark box with a border and its words one `PAD` inside it.
pub(crate) fn paint_label_box(
    painter: &egui::Painter,
    rect: egui::Rect,
    words: &str,
    (border, text): (egui::Stroke, egui::Color32),
) {
    painter.rect(
        rect,
        3.,
        egui::Color32::from_black_alpha(215),
        border,
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.min + dimension::PAD,
        egui::Align2::LEFT_TOP,
        words,
        egui::FontId::proportional(LABEL_FONT),
        text,
    );
}

pub(crate) fn paint_marker_box(
    painter: &egui::Painter,
    rect: egui::Rect,
    words: &str,
    (border, text): (egui::Stroke, egui::Color32),
) {
    painter.rect(rect, 1., MARKER_FILL, border, egui::StrokeKind::Inside);
    painter.text(
        rect.min + dimension::PAD,
        egui::Align2::LEFT_TOP,
        words,
        egui::FontId::proportional(LABEL_FONT),
        text,
    );
}
