// SPDX-License-Identifier: MIT
//! §31L: the labels of the saved whole-Line lengths and the single Circle's radius on the
//! accepted drawing of the constraints form, the selection that they, the persisted list and
//! the Lines list share, and the explicit replacement of a saved dimension, through the form's
//! real widgets and real egui events.
//!
//! The drawings here are the app's own `Curve`s moved and enlarged over the stored curves'
//! identities: **synthetic** (see `canvas.rs`). The saved targets (60 and 30 mm, a radius of
//! 10 mm) are deliberately not the lengths of the stored Lines or the drawing, so a label that
//! measured anything instead of stating the rule fails. The real presentations are read by the
//! native gates at the end of this file and in `sessions.rs`.
#![allow(clippy::panic)]
use super::canvas::{Rig, enlarged, middle, moved, moved_circle, near, wheel, whole};
use super::*;
use crate::sketch::accepted::{Drawing, SELECTED_COLOUR, Target};
use crate::sketch::dimension::PAD;

// ---- fixtures -------------------------------------------------------------------------

impl Rig {
    /// The form over a plate that holds a Horizontal and a Vertical and two lengths: 60 mm on
    /// Line 1 (stored End to Start) and 30 mm on Line 2; the drawing is the stored plate twice
    /// as large, which is neither of those lengths.
    pub(super) fn lengths(pixels_per_point: f32) -> Self {
        let (root, path, source) = lengths_document();
        Self::over(Some(root), &path, &source, pixels_per_point, enlarged)
    }

    /// The form over `source`, handed the drawing that `drawing` makes of the stored Sketch.
    pub(super) fn over(
        root: Option<tempfile::TempDir>,
        path: &Path,
        source: &ExtrudeEditSource,
        pixels_per_point: f32,
        drawing: impl FnOnce(&Sketch) -> Drawing,
    ) -> Self {
        let choice = &source.constraint_sketches[0];
        let stored = choice.stored.clone().expect("stored");
        let ids: Vec<_> = stored.curves.iter().map(|c| c.id).collect();
        let mut e = Editor::default();
        assert!(e.begin(path, source, choice.sketch));
        e.show_drawing(drawing(&stored));
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(pixels_per_point);
        let mut rig = Self {
            _root: root,
            stored,
            ids,
            e,
            ctx,
        };
        for _ in 0..3 {
            rig.frame(vec![]);
        }
        rig
    }
}

/// The document of `Rig::lengths`: forms opened on it share its Sketch, its curves and its rules.
pub(super) fn lengths_document() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let (root, path, _) = fixture();
    let source = persist_rectangle_lengths(&path);
    (root, path, source)
}

/// A cylinder that stores a circle of `stored` mm and persists a Radius of `rule` mm.
pub(super) fn persisted_circle(
    stored: f64,
    rule: f64,
) -> (
    tempfile::TempDir,
    PathBuf,
    ExtrudeEditSource,
    StableEntityId,
) {
    let (root, path, source, ids) = super::diameter::round_fixture(&[stored]);
    let mut document = Document::open(&path).expect("document");
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        source.constraint_sketches[0].sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![radius_add(ids[0], rule)],
        },
    )
    .expect("prepare");
    let id = ferritecad_fixtures::added_constraints(&prepared)[0].id;
    document.write_changes(&prepared).expect("rule");
    let source = crate::sketch_facts::Source::read(&document).expect("catalogue");
    document.close().expect("close");
    (root, path, source, id)
}

pub(super) fn length_add(curve: StableEntityId, mm: f64) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::Line {
        curve,
        kind: LineConstraintKind::Distance(LineLengthMm::new(mm).expect("a length")),
    })
}

fn radius_add(curve: StableEntityId, mm: f64) -> AddSketchConstraint {
    AddSketchConstraint::Circle {
        curve,
        kind: CircleConstraintKind::Radius(CircleRadiusMm::new(mm).expect("a radius")),
    }
}

/// The UUID of the stored length or radius of `mm`: read here from the stored rules, not
/// through the form's own reading.
pub(super) fn rule_of(stored: &Sketch, mm: f64) -> StableEntityId {
    stored
        .constraints
        .iter()
        .find_map(|c| match c.rule.clone() {
            SketchConstraintRule::Distance { distance, .. }
                if distance
                    == ferritecad_document::Expression::constant(mm).expect("finite constant") =>
            {
                Some(c.id)
            }
            SketchConstraintRule::Radius { radius, .. }
                if radius
                    == ferritecad_document::Expression::constant(mm).expect("finite constant") =>
            {
                Some(c.id)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no stored rule of {mm} mm"))
}

// ---- reading the painted frame --------------------------------------------------------

/// A label as painted: its words, the box it stands in and the border of that box.
#[derive(Debug, Clone)]
pub(super) struct Label {
    pub(super) text: String,
    pub(super) rect: egui::Rect,
    pub(super) stroke: egui::Stroke,
}

/// The labels of a frame: a box on a dark plate whose words stand one `PAD` inside it.
pub(super) fn labels(out: &egui::FullOutput) -> Vec<Label> {
    let boxes: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill.r() == 0 && r.fill.g() == 0 && r.fill.b() == 0 => {
                Some((r.rect, r.stroke))
            }
            _ => None,
        })
        .collect();
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if !t.galley.text().contains(" · ") => {
                let (rect, stroke) = boxes.iter().find(|(r, _)| near(r.min + PAD, t.pos))?;
                Some(Label {
                    text: t.galley.text().to_owned(),
                    rect: *rect,
                    stroke: *stroke,
                })
            }
            _ => None,
        })
        .collect()
}

pub(super) fn label(out: &egui::FullOutput, text: &str) -> Label {
    let found: Vec<_> = labels(out).into_iter().filter(|l| l.text == text).collect();
    assert_eq!(
        found.len(),
        1,
        "{text:?} among {:?}",
        labels(out).iter().map(|l| &l.text).collect::<Vec<_>>()
    );
    found[0].clone()
}

pub(super) fn has_label(out: &egui::FullOutput, text: &str) -> bool {
    labels(out).iter().any(|l| l.text == text)
}

/// Whether a painted colour is `colour` at the strength the window paints it with.
pub(super) fn is_colour(shown: egui::Color32, colour: egui::Color32) -> bool {
    let want = colour.gamma_multiply(f32::from(shown.a()) / 255.);
    [
        (shown.r(), want.r()),
        (shown.g(), want.g()),
        (shown.b(), want.b()),
    ]
    .iter()
    .all(|(x, y)| x.abs_diff(*y) <= 1)
}

/// The colour the text `words` is painted in: a disabled button's differs from an enabled one's.
fn ink(out: &egui::FullOutput, words: &str) -> Option<egui::Color32> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == words => Some(t.fallback_color),
        _ => None,
    })
}

/// Press the button or toggle whose words are exactly `words`.
pub(super) fn press_exact(rig: &mut Rig, words: &str) {
    let at = text_center(&rig.frame(vec![]), words);
    click_at(&rig.ctx, &mut rig.e, at, false);
}

impl Rig {
    pub(super) fn edits(&self) -> SketchConstraintEdits {
        self.e.draft_edits().expect("a draft")
    }
    /// The words the form typed into the dimension field holds.
    pub(super) fn typed(&self) -> String {
        self.e.draft.as_ref().expect("draft").dimension_mm.clone()
    }
    pub(super) fn refusal(&self) -> Option<String> {
        self.e.draft.as_ref().expect("draft").refusal.clone()
    }
    pub(super) fn type_length(&mut self, text: &str) {
        enter_field(&self.ctx, &mut self.e, "New length", text, false);
    }
    /// The centre of the painted label `text`, to aim a click at.
    pub(super) fn at_label(&mut self, text: &str) -> egui::Pos2 {
        label(&self.frame(vec![]), text).rect.center()
    }
    pub(super) fn history(&self) -> (usize, usize) {
        self.kept().1
    }
    /// Frames in which the form lets a selection settle: the editor of a dimension that was
    /// just selected is brought into view in the frame after the one that selected it.
    pub(super) fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }
    /// Press the persisted row that starts with `prefix`, scrolling the bounded list to it.
    pub(super) fn press_row(&mut self, prefix: &str) {
        self.settle();
        scroll_to_row(&self.ctx, &mut self.e, prefix);
        self.press(prefix);
    }
}

/// The curves a frame draws as Lines, in the colour of the selection.
pub(super) fn selected_strokes(out: &egui::FullOutput) -> Vec<[egui::Pos2; 2]> {
    Rig::strokes(out, SELECTED_COLOUR)
}

// ---- the labels and their selection ---------------------------------------------------

#[test]
fn a_label_states_the_saved_target_and_selects_exactly_its_rule_only_under_the_dimension_target() {
    for pixels_per_point in [1., 2.] {
        let mut rig = Rig::lengths(pixels_per_point);
        let (d60, d30) = (rule_of(&rig.stored, 60.), rule_of(&rig.stored, 30.));
        let ids = rig.ids.clone();
        let out = rig.frame(vec![]);
        // Two labels, the saved targets: not the 80 and 40 mm of the stored plate and not the
        // 160 and 80 mm that are drawn.
        let mut words: Vec<_> = labels(&out).into_iter().map(|l| l.text).collect();
        words.sort();
        assert_eq!(words, ["30 mm", "60 mm"], "{pixels_per_point}x");
        let formed = rig.e.draft.as_ref().expect("draft").dimensions.clone();
        assert_eq!(
            formed.iter().map(|d| (d.id, d.curve)).collect::<Vec<_>>(),
            vec![(d60, ids[0]), (d30, ids[1])],
            "the form reads the stored rules by their own UUIDs, in the order of the curves"
        );
        let (at60, at30) = (rig.at_label("60 mm"), rig.at_label("30 mm"));
        let before = rig.kept();

        // Under the Object target a label is a picture: a click on it takes the Line within
        // nine points, and a label stands farther than that from its Line.
        assert_eq!(rig.canvas().target(), Target::Object);
        for at in [at60, at30] {
            assert_eq!(
                rig.canvas().would_pick(&rig.solved(), Target::Object, at),
                None
            );
            rig.click(at);
            assert_eq!(
                rig.picks().0,
                None,
                "{pixels_per_point}x: a label was hit by Object"
            );
        }
        // Labels swallow no curve: under Object, Endpoint and Pair a click on a Line that has a
        // label takes the Line, as ever.
        rig.click(rig.mid(0));
        assert_eq!(rig.picks().0, Some(ids[0]), "{pixels_per_point}x");
        rig.target(Target::Endpoint);
        rig.click(rig.ends(0)[0]);
        assert_eq!(
            (rig.picks().0, rig.picks().2),
            (Some(ids[0]), LineEndpoint::Start)
        );
        rig.target(Target::PairA);
        rig.click(rig.mid(1));
        assert_eq!(rig.picks().1[0], Some(ids[1]));
        rig.target(Target::Object);
        // Under the Dimension target the label is picked, by its box, and only by it.
        rig.target(Target::Dimension);
        rig.click(at30);
        assert_eq!(rig.picks().0, Some(ids[1]), "{pixels_per_point}x");
        assert_eq!(
            rig.kept(),
            before,
            "a selection is no input, no step, no request"
        );
        rig.click(at60);
        assert_eq!(rig.picks().0, Some(ids[0]));
        // The Line itself, the empty canvas and the stored approximation are not labels.
        rig.click(rig.mid(1));
        rig.click(rig.rect().center() + egui::vec2(60., 20.));
        rig.click(rig.stored_mid(2));
        assert_eq!(
            rig.picks().0,
            Some(ids[0]),
            "only a label is picked in this mode"
        );
        assert_eq!(rig.kept(), before);

        // The selection is drawn on the Line, on the label and on the row of the rule; the
        // words on the canvas name the rule by its UUID.
        let out = rig.frame(vec![]);
        let ends = rig.ends(0);
        assert!(
            selected_strokes(&out)
                .iter()
                .any(|s| near(s[0], ends[0]) && near(s[1], ends[1]))
        );
        let chosen = label(&out, "60 mm");
        assert!(is_colour(chosen.stroke.color, SELECTED_COLOUR));
        assert!(!is_colour(
            label(&out, "30 mm").stroke.color,
            SELECTED_COLOUR
        ));
        assert!(painted(
            &out,
            &format!("Selected dimension {} · 60 mm", short(d60))
        ));
        assert!(painted(&out, &format!("Line length 60 mm · {d60}")));
        // The same selection is the Lines list's, and the one the form's own length box acts
        // on: Replace length is enabled for this Line.
        assert!(painted(&out, "Replace dimension"));
        // A long hover caption near the bottom must leave the accepted-status and
        // navigation footer readable, including on a Retina display.
        let out = rig.frame(vec![moved(at60)]);
        let caption = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text().contains(" · saved Line length 60 mm") => {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()).expand(2.))
                }
                _ => None,
            })
            .expect("dimension hover caption");
        let canvas = rig.rect();
        let content = egui::Rect::from_min_max(
            canvas.min + egui::vec2(3., 20.),
            canvas.max - egui::vec2(3., 24.),
        );
        assert!(
            content.contains_rect(caption),
            "{pixels_per_point}x: hover caption {caption:?} covers the canvas captions {content:?}"
        );
        // A label of the other rule picks the other Line.
        let mut again = Rig::lengths(pixels_per_point);
        again.target(Target::Dimension);
        let at = again.at_label("30 mm");
        again.click(at);
        assert_eq!(again.picks().0, Some(ids_of(&again)[1]));
    }
}

fn ids_of(rig: &Rig) -> Vec<StableEntityId> {
    rig.ids.clone()
}

#[test]
fn the_labels_the_lines_list_and_the_persisted_rows_show_one_selection() {
    let mut rig = Rig::lengths(1.);
    let ids = rig.ids.clone();
    let (d60, d30) = (rule_of(&rig.stored, 60.), rule_of(&rig.stored, 30.));
    let before = rig.kept();
    assert!(
        !painted(&rig.frame(vec![]), "Replace dimension"),
        "nothing is selected"
    );

    // A persisted row selects its dimension's Line, as a label does: the Line, the label and
    // the editor of the dimension follow.
    rig.press_row(&format!("Line length 30 mm · {d30}"));
    assert_eq!(rig.picks().0, Some(ids[1]));
    rig.settle();
    let out = rig.frame(vec![]);
    assert!(is_colour(
        label(&out, "30 mm").stroke.color,
        SELECTED_COLOUR
    ));
    assert!(painted(&out, "Replace dimension"));
    assert_eq!(rig.kept(), before, "choosing a row is no step and no input");

    // The Lines list selects another Line, one with no saved dimension: no editor.
    rig.press("Segment 3");
    rig.settle();
    let out = rig.frame(vec![]);
    assert_eq!(rig.picks().0, Some(ids[2]));
    assert!(!painted(&out, "Replace dimension"));
    assert!(
        labels(&out)
            .iter()
            .all(|l| !is_colour(l.stroke.color, SELECTED_COLOUR)),
        "no label belongs to the selected Line"
    );
    rig.target(Target::Dimension);
    let out = rig.frame(vec![]);
    assert!(painted(
        &out,
        &format!("Selected: Line {} · no saved dimension", short(ids[2]))
    ));

    // The other rule: its row, then the Lines list, then the canvas, all say Line 1.
    rig.press_row(&format!("Line length 60 mm · {d60}"));
    assert_eq!(rig.picks().0, Some(ids[0]));
    rig.press("Segment 2");
    assert_eq!(rig.picks().0, Some(ids[1]));
    let at = rig.at_label("60 mm");
    rig.click(at);
    let out = rig.frame(vec![]);
    assert_eq!(rig.picks().0, Some(ids[0]));
    assert!(is_colour(
        label(&out, "60 mm").stroke.color,
        SELECTED_COLOUR
    ));
    assert!(!is_colour(
        label(&out, "30 mm").stroke.color,
        SELECTED_COLOUR
    ));
    assert_eq!(rig.kept(), before);
}

// ---- the replacement ------------------------------------------------------------------

#[test]
fn replace_dimension_is_the_replace_length_of_the_list_and_never_a_tolerance() {
    let (_root, path, source) = lengths_document();
    let open = || Rig::over(None, &path, &source, 1., enlarged);
    let mut by_label = open();
    let ids = by_label.ids.clone();
    let d60 = rule_of(&by_label.stored, 60.);
    by_label.target(Target::Dimension);
    let at = by_label.at_label("60 mm");
    by_label.click(at);
    by_label.settle();
    by_label.type_length("70");
    by_label.press("Replace dimension");

    // The same number through the Lines list and the form's own length box and button.
    let mut by_list = open();
    by_list.press("Segment 1");
    enter_field(
        &by_list.ctx,
        &mut by_list.e,
        "Line length (mm):",
        "70",
        false,
    );
    by_list.press("Replace length");
    assert_eq!(by_label.edits(), by_list.edits());
    assert_eq!(by_label.history(), by_list.history());
    // And through the Remove tick and Add length.
    let mut by_hand = open();
    by_hand.press("Segment 1");
    by_hand.settle();
    click_remove_on_row(&by_hand.ctx, &mut by_hand.e, "Line length 60 mm");
    enter_field(
        &by_hand.ctx,
        &mut by_hand.e,
        "Line length (mm):",
        "70",
        false,
    );
    by_hand.press("Add length");
    assert_eq!(by_label.edits(), by_hand.edits());
    assert_eq!(by_label.edits().remove, vec![d60]);

    // The saved number is the exact number: one unit in the last place away is a replacement,
    // 60.0 spelt another way is the saved number.
    let one_more = f64::from_bits(60f64.to_bits() + 1);
    assert_ne!(one_more, 60.);
    by_label.type_length(&format!("{one_more}"));
    by_label.press("Replace dimension");
    assert_eq!(by_label.edits().add, vec![length_add(ids[0], one_more)]);
    assert_eq!(by_label.edits().remove, vec![d60]);
    for spelling in ["60.0", " 60 ", "6e1", "60.000"] {
        by_label.type_length(spelling);
        by_label.press("Replace dimension");
        assert_eq!(
            by_label.edits(),
            SketchConstraintEdits::default(),
            "{spelling:?} is 60"
        );
        by_label.type_length("70");
        by_label.press("Replace dimension");
        assert_eq!(by_label.edits().remove, vec![d60]);
    }
}

#[test]
fn a_dimension_chosen_on_the_canvas_brings_its_row_and_editor_into_view_in_a_long_list() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(988., 768.));
    let (_root, path, source) = super::canvas::polygon_fixture(64, true);
    let rule = rule_of(&dimension_stored(&source), 42.);
    let row = format!("Line length 42 mm · {rule}");
    let mut rig = Rig::over(None, &path, &source, 1., enlarged);
    let last = *rig.ids.last().expect("a Line");
    // The list opens at its top: the row of the length of the last Line is far below the fold.
    assert!(!whole(&rig.frame(vec![]), &row, screen));
    rig.target(Target::Dimension);
    let at = rig.at_label("42 mm");
    rig.click(at);
    rig.settle();
    assert_eq!(rig.picks().0, Some(last));
    // The row and, under it, the editor are in view without anyone scrolling the list.
    let out = rig.frame(vec![]);
    assert!(whole(&out, &row, screen), "the row was left out of sight");
    assert!(
        whole(&out, "Replace dimension", screen),
        "the editor was left out of sight"
    );
    rig.type_length("43");
    rig.press("Replace dimension");
    assert_eq!(rig.edits().remove, vec![rule]);
}

impl Rig {
    /// The number for the replacement of a Circle's radius, typed into the one box whichever
    /// unit is chosen: it stands to the right of the "Diameter" toggle.
    fn type_amount(&mut self, text: &str) {
        enter_field(&self.ctx, &mut self.e, "Diameter", text, false);
    }
}

#[test]
fn a_circle_radius_is_replaced_as_the_radius_whether_the_number_is_a_radius_or_a_diameter() {
    for pixels_per_point in [1., 2.] {
        let (_root, path, source, rule) = persisted_circle(6., 10.);
        let open = || Rig::over(None, &path, &source, pixels_per_point, moved_circle);
        let mut rig = open();
        let circle = rig.ids[0];
        // The label is the saved rule's radius: neither the stored 6 nor the drawn 15.
        let out = rig.frame(vec![]);
        assert_eq!(
            labels(&out)
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>(),
            ["R 10 mm"]
        );
        let out = rig.frame(vec![]);
        assert_ne!(
            ink(&out, "Pair A"),
            ink(&out, "Dimension"),
            "one circle has no pair, and the dimension target is offered"
        );
        assert_eq!(ink(&out, "Endpoint"), ink(&out, "Dimension"));
        rig.target(Target::Dimension);
        let at = rig.at_label("R 10 mm");
        rig.click(at);
        rig.settle();
        assert_eq!(rig.picks().0, Some(circle));
        assert!(painted(
            &rig.frame(vec![]),
            &format!("Selected dimension {} · R 10 mm", short(rule))
        ));

        // A radius typed as a radius.
        rig.type_amount("8.5");
        assert_eq!(rig.typed(), "8.5");
        rig.press("Replace dimension");
        let radius = rig.edits();
        assert_eq!(radius.remove, vec![rule]);
        assert_eq!(radius.add, vec![radius_add(circle, 8.5)]);
        assert_eq!(rig.history(), (1, 0));
        assert!(has_label(&rig.frame(vec![]), "R 10 → 8.5 mm"));
        // The unit is the person's choice and never converts the text: 8.5 as a diameter is a
        // radius of 4.25 mm.
        press_exact(&mut rig, "Diameter");
        assert_eq!(rig.typed(), "8.5", "the text is not converted");
        rig.press("Replace dimension");
        assert_eq!(rig.edits().add, vec![radius_add(circle, 4.25)]);
        assert_eq!(rig.history(), (2, 0), "an update in place, one step");
        assert!(has_label(&rig.frame(vec![]), "R 10 → 4.25 mm"));
        // 17 as a diameter is the first request again; that is a new step, as it is a change.
        rig.type_amount("17");
        rig.press("Replace dimension");
        assert_eq!(rig.edits(), radius);
        assert_eq!(rig.history(), (3, 0));
        // The same number again is nothing.
        rig.press("Replace dimension");
        assert_eq!((rig.edits(), rig.history()), (radius.clone(), (3, 0)));
        // A diameter of 20 mm is the saved radius of 10 mm: the replacement is taken back, and
        // 20 as a radius is a replacement by a circle twice as large.
        rig.type_amount("20");
        rig.press("Replace dimension");
        assert_eq!(rig.edits(), SketchConstraintEdits::default());
        assert!(has_label(&rig.frame(vec![]), "R 10 mm"));
        press_exact(&mut rig, "Radius");
        rig.press("Replace dimension");
        assert_eq!(rig.edits().add, vec![radius_add(circle, 20.)]);
        assert!(has_label(&rig.frame(vec![]), "R 10 → 20 mm"));
        // There is no tolerance: the next radius up is a replacement, 10 spelt otherwise is not.
        let next = f64::from_bits(10f64.to_bits() + 1);
        rig.type_amount(&format!("{next}"));
        rig.press("Replace dimension");
        assert_eq!(rig.edits().add, vec![radius_add(circle, next)]);
        for spelling in ["10", "1e1", " 10.0 "] {
            rig.type_amount(spelling);
            rig.press("Replace dimension");
            assert_eq!(
                rig.edits(),
                SketchConstraintEdits::default(),
                "{spelling:?}"
            );
            rig.type_amount("20");
            rig.press("Replace dimension");
        }
        // Whatever the unit, a number that is no radius keeps its exact text and the draft.
        let held = (rig.edits(), rig.history());
        for text in ["", "abc", "0", "-3", "NaN", "inf", "1e309"] {
            rig.type_amount(text);
            rig.press("Replace dimension");
            assert!(rig.refusal().is_some(), "radius {text:?}");
            assert_eq!(rig.typed(), text);
            assert_eq!((rig.edits(), rig.history()), held);
        }
        press_exact(&mut rig, "Diameter");
        for text in ["", "abc", "0", "-3", "5e-324", "1e309"] {
            rig.type_amount(text);
            rig.press("Replace dimension");
            assert!(rig.refusal().is_some(), "diameter {text:?}");
            assert_eq!(rig.typed(), text);
            assert_eq!((rig.edits(), rig.history()), held);
        }
        rig.type_amount("abc");
        rig.press("Replace dimension");
        assert_eq!(
            rig.refusal().as_deref(),
            Some("Enter a positive diameter in mm.")
        );
        press_exact(&mut rig, "Radius");
        rig.press("Replace dimension");
        assert_eq!(
            rig.refusal().as_deref(),
            Some("Enter a positive radius in mm.")
        );

        // The one request the form already made for this, by hand: Remove, then Add diameter.
        let mut by_hand = open();
        by_hand.press(&format!("Circle · {}", short(circle)));
        by_hand.settle();
        click_remove_on_row(&by_hand.ctx, &mut by_hand.e, "Radius 10 mm");
        enter_field(&by_hand.ctx, &mut by_hand.e, "Diameter mm", "17", false);
        by_hand.press("Add diameter");
        assert_eq!(by_hand.edits(), radius);
    }
}

// ---- view, hit area, blocked input ----------------------------------------------------

/// The band of the canvas that a label stands in: the edge words keep their places.
pub(super) fn band(canvas: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        canvas.min + egui::vec2(3., 20.),
        canvas.max - egui::vec2(3., 24.),
    )
}

#[test]
fn labels_follow_zoom_pan_and_fit_and_are_hit_exactly_where_they_are_drawn() {
    for pixels_per_point in [1., 2.] {
        let mut rig = Rig::lengths(pixels_per_point);
        let ids = rig.ids.clone();
        rig.target(Target::Dimension);
        rig.e.pick_for_tests(ids[2]);
        let canvas = rig.rect();
        let opening = rig.canvas().view();
        let first = label(&rig.frame(vec![]), "60 mm");
        assert!(band(canvas).contains_rect(first.rect), "{first:?}");
        let r = first.rect;

        // The hit area is the drawn box: just inside every corner takes the label, just
        // outside takes nothing, and the nine points of the curves' pick reach nothing.
        let nudge = |x: f32, y: f32| egui::vec2(x, y);
        for at in [
            r.min + nudge(0.5, 0.5),
            r.max - nudge(0.5, 0.5),
            r.left_bottom() + nudge(0.5, -0.5),
            r.right_top() + nudge(-0.5, 0.5),
            r.center(),
        ] {
            rig.e.pick_for_tests(ids[2]);
            rig.click(at);
            assert_eq!(
                rig.picks().0,
                Some(ids[0]),
                "{pixels_per_point}x inside at {at:?}"
            );
        }
        for at in [
            r.min - nudge(0.5, 0.),
            r.min - nudge(0., 0.5),
            r.max + nudge(0.5, 0.),
            r.max + nudge(0., 0.5),
            r.left_bottom() + nudge(-0.5, 0.5),
            r.right_top() + nudge(0.5, -0.5),
        ] {
            rig.e.pick_for_tests(ids[2]);
            rig.click(at);
            assert_eq!(
                rig.picks().0,
                Some(ids[2]),
                "{pixels_per_point}x outside at {at:?}"
            );
        }

        // Zoom out about a point above the Line: the Line's middle moves up, and the label with
        // it, by exactly as much, still whole inside the band.
        let before = rig.mid(0);
        let about = egui::pos2(before.x, canvas.top() + 30.);
        rig.frame(vec![moved(about)]);
        rig.frame(vec![moved(about), wheel(-40.)]);
        let after = rig.mid(0);
        assert_ne!(before, after);
        let zoomed = label(&rig.frame(vec![]), "60 mm");
        assert!(band(canvas).contains_rect(zoomed.rect), "{zoomed:?}");
        assert!(
            near(zoomed.rect.min, r.min + (after - before)),
            "the label did not move with its Line: {:?} vs {:?}",
            zoomed.rect,
            r.translate(after - before)
        );
        rig.e.pick_for_tests(ids[2]);
        rig.click(r.center());
        let moved_away = rig.picks().0 == Some(ids[0]);
        assert!(
            !moved_away || zoomed.rect.contains(r.center()),
            "the old place of the box still takes the label"
        );
        rig.e.pick_for_tests(ids[2]);
        rig.click(zoomed.rect.center());
        assert_eq!(rig.picks().0, Some(ids[0]), "the box where it is drawn now");

        // Pan with the middle button: the label moves with the Line, until the band holds it.
        rig.press("Fit");
        let fitted = label(&rig.frame(vec![]), "60 mm");
        assert_eq!(fitted.rect, first.rect, "Fit gives the opening view back");
        let grab = canvas.center() + egui::vec2(0., 30.);
        rig.frame(vec![moved(grab), middle(grab, true)]);
        rig.frame(vec![moved(grab + egui::vec2(-20., 0.))]);
        rig.frame(vec![middle(grab + egui::vec2(-20., 0.), false)]);
        let panned = label(&rig.frame(vec![]), "60 mm");
        assert!(
            near(panned.rect.min, first.rect.min + egui::vec2(-20., 0.)),
            "{panned:?}"
        );
        assert!(band(canvas).contains_rect(panned.rect));
        assert!(
            panned.rect.left() >= band(canvas).left() - 0.01,
            "{panned:?}"
        );
        assert_eq!(
            panned.rect.min.y, first.rect.min.y,
            "a sideways pan keeps the label's height"
        );
        assert!(panned.rect.min.x < first.rect.min.x);

        // Far in, the Line's middle is off the canvas: no label, no hit, and the list still
        // reaches the dimension. Fit returns the label where it was.
        rig.press("Fit");
        for _ in 0..6 {
            rig.frame(vec![
                moved(canvas.left_top() + egui::vec2(4., 4.)),
                wheel(120.),
            ]);
        }
        let near_in = rig.frame(vec![]);
        assert!(!has_label(&near_in, "60 mm"), "{pixels_per_point}x");
        rig.e.pick_for_tests(ids[2]);
        rig.click(first.rect.center());
        assert_eq!(rig.picks().0, Some(ids[2]), "nothing is there");
        rig.press("Fit");
        assert_eq!(rig.canvas().view(), opening);
        assert_eq!(label(&rig.frame(vec![]), "60 mm").rect, first.rect);
        rig.click(first.rect.center());
        assert_eq!(rig.picks().0, Some(ids[0]));
        assert!(rig.e.take_request().is_none());
    }
}

// ---- the 768-point screen -------------------------------------------------------------

#[test]
fn the_labels_the_editor_and_the_actions_stay_reachable_at_768_points_in_the_worst_forms() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(988., 768.));
    let always = [
        "Cancel constraints draft",
        "Undo",
        "Redo",
        "Clear pending changes",
        "Apply constraints",
        "Save constraints copy…",
        "Object",
        "Endpoint",
        "Pair A",
        "Pair B",
        "Dimension",
        "Fit",
        "Replace dimension",
    ];
    let long_refusal = "enter a finite positive Line length in mm — and so on, a refusal worded \
                        at such length that it must wrap inside the form instead of leaving it";
    let (_a, plate_path, plate) = lengths_document();
    let (_b, polygon_path, polygon) = super::canvas::polygon_fixture(64, true);
    let (_c, round_path, round, _) = persisted_circle(6., 10.);
    let cases: Vec<(&str, PathBuf, ExtrudeEditSource, &str, &str)> = vec![
        (
            "plate with two lengths",
            plate_path,
            plate,
            "60 mm",
            "New length",
        ),
        (
            "64-gon with every closure link and a stored length",
            polygon_path,
            polygon,
            "42 mm",
            "New length",
        ),
        (
            "circle with a radius",
            round_path,
            round,
            "R 10 mm",
            "Diameter",
        ),
    ];
    let mut report = Vec::new();
    for (name, path, source, label_text, field) in cases {
        let stored = source.constraint_sketches[0]
            .stored
            .clone()
            .expect("stored");
        let is_circle = matches!(stored.curves[0].geometry, SketchGeometry::Circle { .. });
        let mut bottoms = Vec::new();
        for (unsaved, extras) in [(false, false), (true, true)] {
            let mut rig = Rig::over(
                None,
                &path,
                &source,
                1.,
                if is_circle { moved_circle } else { enlarged },
            );
            rig.e.set_session(true, true, unsaved);
            // The worst of the old gate: thirty-two pending Horizontals on a 64-gon.
            if extras && !is_circle && stored.curves.len() >= 32 {
                let draft = rig.e.draft.as_mut().expect("draft");
                draft.edits.add = stored
                    .curves
                    .iter()
                    .step_by(2)
                    .map(|c| {
                        AddSketchConstraint::Line(AddLineConstraint::Line {
                            curve: c.id,
                            kind: LineConstraintKind::Horizontal,
                        })
                    })
                    .collect();
                draft.choice.validate_edits(&draft.edits).expect("valid");
            }
            rig.target(Target::Dimension);
            let at = rig.at_label(label_text);
            rig.click(at);
            rig.settle();
            // A replacement is prepared, so that Apply and Save are offered.
            if is_circle {
                press_exact(&mut rig, "Diameter");
            }
            rig.type_amount_or_length(is_circle, if is_circle { "17" } else { "55.5" });
            rig.press("Replace dimension");
            assert!(rig.edits().remove.len() == 1, "{name}: no replacement");
            if extras {
                // A long text in the field and a long refusal under it. The fully loaded 64-gon
                // is as the 31K gate left it: it has no room for a refusal line even without
                // the canvas or the dimension.
                rig.type_amount_or_length(is_circle, "1234567890123456789012345678901");
                if stored.curves.len() < 32 {
                    rig.e.draft.as_mut().expect("draft").refusal = Some(long_refusal.to_owned());
                }
            }
            rig.settle();
            let out = rig.frame(vec![]);
            for words in always {
                assert!(
                    whole(&out, words, screen),
                    "{name} (unsaved {unsaved}, extras {extras}): {words} is not reachable"
                );
            }
            assert!(whole(&out, field, screen), "{name}: the field's label");
            let boxes = labels(&out);
            assert!(!boxes.is_empty(), "{name}: no label");
            for l in &boxes {
                assert!(
                    screen.contains_rect(l.rect) && rig.rect().contains_rect(l.rect),
                    "{name}: label {l:?} is outside the canvas or the screen"
                );
            }
            assert!(screen.contains_rect(rig.rect()), "{name}: the canvas");
            let bottom = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == "Save constraints copy…" => {
                        Some(t.visual_bounding_rect().bottom())
                    }
                    _ => None,
                })
                .fold(f32::MIN, f32::max);
            assert!(bottom <= 768., "{name}: Save ends at {bottom}");
            // The Dimension target is one more button in the row under the canvas: the form
            // stays about as wide as the §31G forms were (their widest text ended at 671).
            let right = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.visual_bounding_rect().right()),
                    _ => None,
                })
                .fold(f32::MIN, f32::max);
            assert!(
                right <= 700.,
                "{name}: the form's widest text ends at {right}"
            );
            eprintln!("FORM_WIDTH_31L {name}: widest text ends at {right:.1}");
            bottoms.push(bottom);
        }
        report.push(format!(
            "{name}: Save bottom {:.1} with a dimension selected, {:.1} with a long text, a prepared replacement, a refusal and the unsaved note",
            bottoms[0], bottoms[1]
        ));
    }
    for line in &report {
        eprintln!("FORM_BUDGET_31L {line}");
    }
}

impl Rig {
    fn type_amount_or_length(&mut self, circle: bool, text: &str) {
        if circle {
            self.type_amount(text);
        } else {
            self.type_length(text);
        }
    }
}

// ---- no drawing, other profiles, other forms, overlaps -------------------------------

#[test]
fn without_a_drawing_the_saved_rows_still_select_and_replace_and_other_profiles_have_no_dimension()
{
    let (_root, path, source) = lengths_document();
    let (d60, d30) = (
        rule_of(&dimension_stored(&source), 60.),
        rule_of(&dimension_stored(&source), 30.),
    );
    // The form with no accepted drawing: no canvas, no labels, no dimension target; the list of
    // persisted rules selects and replaces as it does with one.
    let mut with = Rig::over(None, &path, &source, 1., enlarged);
    with.target(Target::Dimension);
    let at = with.at_label("60 mm");
    with.click(at);
    with.settle();
    with.type_length("70");
    with.press("Replace dimension");
    let mut without = Rig::over(None, &path, &source, 1., |_| {
        Drawing::Unavailable(crate::sketch::accepted::Unavailable::NotHanded)
    });
    let out = without.frame(vec![]);
    assert!(labels(&out).is_empty() && !painted(&out, "Dimension") && !painted(&out, "Fit"));
    without.press_row(&format!("Line length 60 mm · {d60}"));
    without.settle();
    assert_eq!(without.picks().0, Some(without.ids[0]));
    without.type_length("70");
    without.press("Replace dimension");
    assert_eq!(without.edits(), with.edits());
    assert_eq!(without.edits().remove, vec![d60]);
    // A form held on another version loses its drawing and keeps its picks and rows.
    let mut held = Rig::over(None, &path, &source, 1., enlarged);
    held.press_row(&format!("Line length 30 mm · {d30}"));
    held.settle();
    assert!(!labels(&held.frame(vec![])).is_empty());
    held.e.retire_drawing();
    held.settle();
    let out = held.frame(vec![]);
    assert!(labels(&out).is_empty() && !painted(&out, "Dimension"));
    assert_eq!(
        held.picks().0,
        Some(held.ids[1]),
        "the picks of a held form are kept"
    );
    // The form got shorter; the person scrolls the list to the editor as ever.
    scroll_to_row(&held.ctx, &mut held.e, "Replace dimension");
    held.type_length("33");
    held.press("Replace dimension");
    assert_eq!(held.edits().remove, vec![d30]);

    // An annulus has no dimension on this canvas and its rows stay rows; so has a ring in which
    // both circles keep a radius.
    let (_ring, ring_path, ring, ids) = super::diameter::round_fixture(&[10., 4.]);
    let mut document = Document::open(&ring_path).expect("document");
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        ring.constraint_sketches[0].sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![radius_add(ids[0], 10.5), radius_add(ids[1], 4.5)],
        },
    )
    .expect("prepare");
    document.write_changes(&prepared).expect("rules");
    let ring = crate::sketch_facts::Source::read(&document).expect("catalogue");
    document.close().expect("close");
    let mut rig = Rig::over(None, &ring_path, &ring, 1., |_| {
        Drawing::Unavailable(crate::sketch::accepted::Unavailable::Profile)
    });
    assert!(rig.e.draft.as_ref().expect("draft").dimensions.is_empty());
    let rule = rule_of(&rig.stored, 10.5);
    rig.press_row(&format!("Radius 10.5 mm · {rule}"));
    rig.settle();
    let out = rig.frame(vec![]);
    assert_eq!(rig.picks().0, None, "an annulus row is no selection");
    assert!(!painted(&out, "Replace dimension") && labels(&out).is_empty());
    rig.press(&format!("Boundary circle · {}", short(ids[0])));
    rig.settle();
    assert!(!painted(&rig.frame(vec![]), "Replace dimension"));
}

pub(super) fn dimension_stored(source: &ExtrudeEditSource) -> Sketch {
    source.constraint_sketches[0]
        .stored
        .clone()
        .expect("stored")
}

#[test]
fn overlapping_labels_are_reached_in_stored_order_and_the_list_reaches_the_one_underneath() {
    let (_root, path, source) = lengths_document();
    let (d60, d30) = (
        rule_of(&dimension_stored(&source), 60.),
        rule_of(&dimension_stored(&source), 30.),
    );
    // Four parallel Lines two millimetres apart: the labels of Lines 1 and 2 overlap.
    let stack = |stored: &Sketch| {
        Drawing::of(
            stored,
            stored.curves.iter().enumerate().map(|(i, c)| {
                let y = 10. + 2. * i as f64;
                (
                    c.id,
                    SketchGeometry::Line {
                        start: ferritecad_document::Point2 { x: 0., y },
                        end: ferritecad_document::Point2 { x: 60., y },
                    },
                )
            }),
        )
    };
    let mut rig = Rig::over(None, &path, &source, 1., stack);
    let ids = rig.ids.clone();
    rig.target(Target::Dimension);
    let out = rig.frame(vec![]);
    let (one, two) = (label(&out, "60 mm"), label(&out, "30 mm"));
    assert!(one.rect.intersects(two.rect), "{one:?} {two:?}");
    // Line 2 is stored after Line 1: its label is the one underneath where they overlap, and
    // the order of the UUIDs plays no part.
    let both = one.rect.intersect(two.rect).center();
    rig.e.pick_for_tests(ids[3]);
    rig.click(both);
    assert_eq!(
        rig.picks().0,
        Some(ids[0]),
        "the first stored curve's label wins"
    );
    // The part of the second label that the first does not cover is its own.
    let uncovered = egui::pos2(
        two.rect.center().x,
        two.rect.top().min(one.rect.top()) + 0.5,
    );
    assert!(two.rect.contains(uncovered) && !one.rect.contains(uncovered));
    rig.click(uncovered);
    assert_eq!(rig.picks().0, Some(ids[1]));
    // The list reaches either dimension whatever lies over its label.
    rig.press_row(&format!("Line length 60 mm · {d60}"));
    assert_eq!(rig.picks().0, Some(ids[0]));
    rig.press_row(&format!("Line length 30 mm · {d30}"));
    assert_eq!(rig.picks().0, Some(ids[1]));
    rig.settle();
    rig.type_length("33");
    rig.press("Replace dimension");
    assert_eq!(rig.edits().remove, vec![d30]);
    // The label on top is the earlier stored one in the picture too: the second label is drawn
    // first and the first over it.
    let out = rig.frame(vec![]);
    let order: Vec<_> = labels(&out).into_iter().map(|l| l.text).collect();
    assert_eq!(order, ["30 → 33 mm", "60 mm"]);
}

#[test]
fn returning_to_a_dimension_after_picking_a_line_keeps_the_legacy_field() {
    let mut rig = Rig::lengths(1.);
    let width = rule_of(&rig.stored, 60.);
    rig.target(Target::Dimension);
    let at = rig.at_label("60 mm");
    rig.click(at);
    rig.settle();
    rig.type_length("55.5");
    assert_eq!(rig.typed(), "55.5");
    rig.target(Target::Object);
    rig.click(rig.mid(2));
    rig.settle();
    assert_eq!(rig.picks().0, Some(rig.ids[2]));
    rig.target(Target::Dimension);
    let at = rig.at_label("60 mm");
    rig.click(at);
    rig.settle();
    assert_eq!(rig.picks().0, Some(rig.ids[0]));
    assert_eq!(rule_of(&rig.stored, 60.), width);
    assert_eq!(rig.edits(), SketchConstraintEdits::default());
    assert_eq!(rig.history(), (0, 0));
    assert!(rig.e.take_request().is_none() && rig.e.take_apply_request().is_none());
    assert_eq!(rig.typed(), "55.5");
}
