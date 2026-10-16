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
use super::canvas::{
    Rig, enlarged, escape, middle, moved, moved_circle, near, primary, shift_primary, wheel, whole,
};
use super::*;
use crate::sketch::accepted::{Drawing, SELECTED_COLOUR, Shape, Target};
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
fn lengths_document() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let (root, path, _) = fixture();
    let source = persist_rectangle_lengths(&path);
    (root, path, source)
}

/// A cylinder that stores a circle of `stored` mm and persists a Radius of `rule` mm.
fn persisted_circle(
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
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        source.constraint_sketches[0].sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![radius_add(ids[0], rule)],
        },
    )
    .expect("prepare");
    let id = prepared.added[0].id;
    document.write_sketch_constraints(&prepared).expect("rule");
    let source = ExtrudeEditSource::read(&document).expect("catalogue");
    document.close().expect("close");
    (root, path, source, id)
}

fn length_add(curve: StableEntityId, mm: f64) -> AddSketchConstraint {
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
fn rule_of(stored: &Sketch, mm: f64) -> StableEntityId {
    stored
        .constraints
        .iter()
        .find_map(|c| match c.rule {
            SketchConstraintRule::Distance { distance, .. } if distance == mm => Some(c.id),
            SketchConstraintRule::Radius { radius, .. } if radius == mm => Some(c.id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no stored rule of {mm} mm"))
}

// ---- reading the painted frame --------------------------------------------------------

/// A label as painted: its words, the box it stands in and the border of that box.
#[derive(Debug, Clone)]
struct Label {
    text: String,
    rect: egui::Rect,
    stroke: egui::Stroke,
}

/// The labels of a frame: a box on a dark plate whose words stand one `PAD` inside it.
fn labels(out: &egui::FullOutput) -> Vec<Label> {
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

fn label(out: &egui::FullOutput, text: &str) -> Label {
    let found: Vec<_> = labels(out).into_iter().filter(|l| l.text == text).collect();
    assert_eq!(
        found.len(),
        1,
        "{text:?} among {:?}",
        labels(out).iter().map(|l| &l.text).collect::<Vec<_>>()
    );
    found[0].clone()
}

fn has_label(out: &egui::FullOutput, text: &str) -> bool {
    labels(out).iter().any(|l| l.text == text)
}

/// Whether a painted colour is `colour` at the strength the window paints it with.
fn is_colour(shown: egui::Color32, colour: egui::Color32) -> bool {
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
fn press_exact(rig: &mut Rig, words: &str) {
    let at = text_center(&rig.frame(vec![]), words);
    click_at(&rig.ctx, &mut rig.e, at, false);
}

impl Rig {
    fn edits(&self) -> SketchConstraintEdits {
        self.e.draft_edits().expect("a draft")
    }
    /// The words the form typed into the dimension field holds.
    fn typed(&self) -> String {
        self.e.draft.as_ref().expect("draft").dimension_mm.clone()
    }
    fn refusal(&self) -> Option<String> {
        self.e.draft.as_ref().expect("draft").refusal.clone()
    }
    fn type_length(&mut self, text: &str) {
        enter_field(&self.ctx, &mut self.e, "New length", text, false);
    }
    /// The centre of the painted label `text`, to aim a click at.
    fn at_label(&mut self, text: &str) -> egui::Pos2 {
        label(&self.frame(vec![]), text).rect.center()
    }
    fn history(&self) -> (usize, usize) {
        self.kept().1
    }
    /// Frames in which the form lets a selection settle: the editor of a dimension that was
    /// just selected is brought into view in the frame after the one that selected it.
    fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }
    /// Press the persisted row that starts with `prefix`, scrolling the bounded list to it.
    fn press_row(&mut self, prefix: &str) {
        self.settle();
        scroll_to_row(&self.ctx, &mut self.e, prefix);
        self.press(prefix);
    }
}

/// The curves a frame draws as Lines, in the colour of the selection.
fn selected_strokes(out: &egui::FullOutput) -> Vec<[egui::Pos2; 2]> {
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
        // A label of the other rule picks the other Line.
        let mut again = Rig::lengths(pixels_per_point);
        again.target(Target::Dimension);
        let at = again.at_label("30 mm");
        again.click(at);
        assert_eq!(again.picks().0, Some(ids_of(&again)[1]));
    }
    println!("\nFCAD_31L_FORM_LABELS_EXECUTED");
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
    println!("\nFCAD_31L_FORM_SYNC_EXECUTED");
}

// ---- the replacement ------------------------------------------------------------------

#[test]
fn replace_dimension_prepares_one_step_updates_it_in_place_and_the_saved_number_takes_it_back() {
    for pixels_per_point in [1., 2.] {
        let mut rig = Rig::lengths(pixels_per_point);
        let ids = rig.ids.clone();
        let (d60, d30) = (rule_of(&rig.stored, 60.), rule_of(&rig.stored, 30.));
        let drawn = rig.solved();
        let view = rig.canvas().view();
        let stored = rig.e.draft.as_ref().expect("draft").choice.stored.clone();
        rig.target(Target::Dimension);
        let at = rig.at_label("60 mm");
        rig.click(at);
        rig.settle();
        assert_eq!(rig.picks().0, Some(ids[0]));

        // Typing is no step. One Replace is one remove and one add, one step of history.
        rig.type_length("70");
        assert_eq!((rig.typed().as_str(), rig.history()), ("70", (0, 0)));
        assert!(rig.edits() == SketchConstraintEdits::default());
        rig.press("Replace dimension");
        let first = rig.edits();
        assert_eq!(first.remove, vec![d60], "{pixels_per_point}x");
        assert_eq!(first.add, vec![length_add(ids[0], 70.)]);
        assert_eq!(rig.history(), (1, 0));
        assert_eq!(rig.refusal(), None);
        // The label states the target and, beside it, what is prepared; the curve, the view and
        // the stored Sketch are where they were, and the rule is still in the saved list.
        let out = rig.frame(vec![]);
        assert!(has_label(&out, "60 → 70 mm") && !has_label(&out, "60 mm"));
        assert!(painted(
            &out,
            &format!("Selected dimension {} · 60 → 70 mm", short(d60))
        ));
        assert!(painted(&out, &format!("Line length 60 mm · {d60}")));
        assert_eq!(rig.solved(), drawn, "a prepared change draws nothing");
        assert_eq!(rig.canvas().view(), view);
        assert_eq!(rig.e.draft.as_ref().expect("draft").choice.stored, stored);
        assert!(
            Rig::strokes(&out, crate::sketch::accepted::PENDING_COLOUR).is_empty(),
            "no preview stroke"
        );
        // Selecting another Line shows the prepared label by its own colour, not the selection's.
        rig.press("Segment 3");
        rig.settle();
        let out = rig.frame(vec![]);
        assert!(is_colour(
            label(&out, "60 → 70 mm").stroke.color,
            crate::sketch::accepted::PENDING_COLOUR
        ));
        rig.press("Segment 1");
        rig.settle();

        // Replacing it again updates that very addition: still one remove and one add.
        rig.type_length("55");
        rig.press("Replace dimension");
        let second = rig.edits();
        assert_eq!(second.remove, vec![d60]);
        assert_eq!(second.add, vec![length_add(ids[0], 55.)]);
        assert_eq!(rig.history(), (2, 0), "one step more, not a second pair");
        rig.press("Undo");
        assert_eq!(rig.edits(), first);
        rig.press("Redo");
        assert_eq!(rig.edits(), second);
        // The number that is already pending changes nothing, and spends no step.
        rig.press("Replace dimension");
        assert_eq!((rig.edits(), rig.history()), (second.clone(), (2, 0)));

        // Typing the saved number takes back only this replacement; the saved rule keeps its
        // UUID because it is no longer removed. With nothing left to change, nothing can be
        // applied or saved: an empty draft never reaches a job.
        rig.type_length("60");
        rig.press("Replace dimension");
        assert_eq!(rig.edits(), SketchConstraintEdits::default());
        assert_eq!(rig.history(), (3, 0));
        let out = rig.frame(vec![]);
        assert!(has_label(&out, "60 mm"));
        assert!(!painted(&out, "Apply constraints") && !painted(&out, "Save constraints copy…"));
        rig.press("Undo");
        assert_eq!(rig.edits(), second);
        rig.press("Redo");
        assert_eq!(rig.edits(), SketchConstraintEdits::default());

        // Other pending changes survive both: a Horizontal on Line 3, the removal of the other
        // length and its own replacement.
        rig.press("Segment 3");
        rig.press("Add Horizontal");
        let horizontal = AddSketchConstraint::Line(AddLineConstraint::Line {
            curve: ids[2],
            kind: LineConstraintKind::Horizontal,
        });
        rig.press("Segment 1");
        rig.settle();
        rig.type_length("70");
        rig.press("Replace dimension");
        rig.target(Target::Dimension);
        let at = rig.at_label("30 mm");
        rig.click(at);
        rig.settle();
        rig.type_length("33");
        rig.press("Replace dimension");
        let both = rig.edits();
        assert_eq!(both.remove, vec![d60, d30]);
        assert_eq!(
            both.add,
            vec![horizontal, length_add(ids[0], 70.), length_add(ids[1], 33.)]
        );
        let out = rig.frame(vec![]);
        assert!(has_label(&out, "60 → 70 mm") && has_label(&out, "30 → 33 mm"));
        // Returning 60 takes back that one pair and nothing else.
        let at = rig.at_label("60 → 70 mm");
        rig.click(at);
        rig.settle();
        rig.type_length("60");
        rig.press("Replace dimension");
        let one = rig.edits();
        assert_eq!(one.remove, vec![d30], "the other length is still replaced");
        assert_eq!(one.add, vec![horizontal, length_add(ids[1], 33.)]);
        let out = rig.frame(vec![]);
        assert!(has_label(&out, "60 mm") && has_label(&out, "30 → 33 mm"));
        // A request is never made by any of this.
        assert!(rig.e.take_request().is_none() && rig.e.take_apply_request().is_none());
    }
    println!("\nFCAD_31L_FORM_REPLACE_EXECUTED");
}

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
    println!("\nFCAD_31L_FORM_SEMANTICS_EXECUTED");
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
    println!("\nFCAD_31L_FORM_REVEAL_EXECUTED");
}

#[test]
fn a_cancelled_save_or_a_stale_reply_keeps_the_text_the_selection_and_the_prepared_replacement() {
    let (_root, path, source) = lengths_document();
    let mut rig = Rig::over(None, &path, &source, 1., enlarged);
    let ids = rig.ids.clone();
    let d60 = rule_of(&rig.stored, 60.);
    rig.target(Target::Dimension);
    let at = rig.at_label("60 mm");
    rig.click(at);
    rig.settle();
    rig.type_length("70");
    rig.press("Replace dimension");
    rig.type_length("58.25");
    rig.press("Save constraints copy…");
    let asked = rig.e.take_request().expect("the Save asked for");
    assert_eq!(asked.edits.remove, vec![d60]);
    let kept = (rig.kept(), rig.picks().0, rig.typed());
    let mut state = crate::edits::Edits::default();
    let (tx, rx) = std::sync::mpsc::channel();
    let generation = state
        .start_constraints(asked, move |_, g, _| {
            std::thread::spawn(move || tx.send(g).expect("reply"))
        })
        .expect("start");
    // A reply of another generation, and the worker's refusal or a cancelled Save: the draft,
    // the text, the selection and the label of the prepared replacement are what they were.
    assert!(
        finish_edit(
            &mut rig.e,
            &mut state,
            generation + 1,
            Err(ferritecad_types::CadError::input("stale"))
        )
        .is_none()
    );
    assert_eq!((rig.kept(), rig.picks().0, rig.typed()), kept);
    let g = rx.recv().expect("done");
    assert!(
        finish_edit(
            &mut rig.e,
            &mut state,
            g,
            Err(ferritecad_types::CadError::topology(
                "lost stored reference"
            ))
        )
        .is_none()
    );
    assert_eq!((rig.kept(), rig.picks().0, rig.typed()), kept);
    assert_eq!(rig.picks().0, Some(ids[0]));
    rig.settle();
    let out = rig.frame(vec![]);
    assert!(has_label(&out, "60 → 70 mm") && painted(&out, "Replace dimension"));
    // And the form takes the next action as if nothing had happened.
    rig.type_length("80");
    rig.press("Replace dimension");
    assert_eq!(rig.edits().add, vec![length_add(ids[0], 80.)]);
    println!("\nFCAD_31L_FORM_CANCELLED_EXECUTED");
}

#[test]
fn a_refused_replacement_keeps_the_typed_text_the_selection_and_the_history_exactly() {
    let mut rig = Rig::lengths(1.);
    let ids = rig.ids.clone();
    rig.target(Target::Dimension);
    let at = rig.at_label("60 mm");
    rig.click(at);
    rig.settle();
    let before = rig.kept();
    for text in [
        "", " ", "-", "abc", "NaN", "inf", "-inf", "0", "-0", "-1", "1e309", "7,5", "6 0",
    ] {
        rig.type_length(text);
        rig.press("Replace dimension");
        assert!(rig.refusal().is_some(), "{text:?} was taken");
        assert_eq!(rig.typed(), text, "the text is kept exactly");
        assert_eq!(rig.picks().0, Some(ids[0]), "{text:?}");
        let mut now = rig.kept();
        // The text itself is the one input that differs from before.
        now.0 = before.0.clone();
        assert_eq!(
            now, before,
            "{text:?} changed the history or made a request"
        );
        assert_eq!(rig.edits(), SketchConstraintEdits::default());
    }
    // A valid number takes the refusal away; the refusal never outlives the next valid action.
    rig.type_length("70");
    rig.press("Replace dimension");
    assert_eq!(rig.refusal(), None);
    // A draft the validator refuses stays refused whole, text and history untouched. The Remove
    // tick can leave a draft in which a Vertical is added to a Line that keeps its Horizontal;
    // a valid number must still pass the common validator before Replace changes anything.
    rig.press("Undo");
    assert_eq!(rig.edits(), SketchConstraintEdits::default());
    click_remove_on_row(&rig.ctx, &mut rig.e, "Horizontal");
    rig.press("Add Vertical");
    click_remove_on_row(&rig.ctx, &mut rig.e, "Horizontal");
    rig.press("Segment 2");
    rig.press("Segment 1");
    rig.settle();
    let held = rig.kept();
    rig.type_length("55");
    rig.press("Replace dimension");
    let refusal = rig.refusal().expect("the validator's refusal");
    assert!(refusal.contains("only one H/V"), "{refusal}");
    let mut now = rig.kept();
    now.0 = held.0.clone();
    assert_eq!(now, held, "a refused draft was changed");
    assert_eq!(rig.typed(), "55");
    // The Revolve-style guard is the same one: nothing is ever requested by a refusal.
    assert!(rig.e.take_request().is_none() && rig.e.take_apply_request().is_none());
    println!("\nFCAD_31L_FORM_REFUSED_EXECUTED");
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
    println!("\nFCAD_31L_FORM_CIRCLE_EXECUTED");
}

// ---- view, hit area, blocked input ----------------------------------------------------

/// The band of the canvas that a label stands in: the edge words keep their places.
fn band(canvas: egui::Rect) -> egui::Rect {
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
    println!("\nFCAD_31L_FORM_VIEW_EXECUTED");
}

#[test]
fn a_label_press_dies_with_its_surface_and_busy_queued_covered_or_unfocused_forms_take_nothing() {
    for pixels_per_point in [1., 2.] {
        // A press begun on an available label is no permission for a later release, however the
        // surface came back before it: focus lost, another layer over the canvas, and that
        // layer gone again.
        for interruption in ["focus", "covered", "uncovered"] {
            let mut held = Rig::lengths(pixels_per_point);
            let ids = held.ids.clone();
            held.target(Target::Dimension);
            held.e.pick_for_tests(ids[2]);
            let before = held.facts();
            let at = held.at_label("60 mm");
            let cover = (interruption != "focus").then_some(held.rect());
            held.frame(vec![moved(at), primary(at, true)]);
            assert!(held.canvas().pressed());
            for _ in 0..3 {
                held.frame_as(vec![], false, true, cover);
            }
            if interruption == "uncovered" {
                for _ in 0..3 {
                    held.frame(vec![]);
                }
            }
            held.frame_as(
                vec![primary(at, false)],
                false,
                interruption != "focus",
                (interruption == "covered").then_some(held.rect()),
            );
            assert_eq!(
                held.facts(),
                before,
                "{pixels_per_point}x: a release after {interruption} took a label"
            );
            assert!(!held.canvas().pressed());
        }

        // A release just outside the canvas, within a click's distance of the press, takes nothing.
        let mut edge = Rig::lengths(pixels_per_point);
        let ids = edge.ids.clone();
        edge.target(Target::Dimension);
        edge.e.pick_for_tests(ids[2]);
        let canvas = edge.rect();
        // Pan the Line so that its middle is twenty points from the canvas's left edge: the box
        // is held in the band, three points from that edge.
        let grab = canvas.center() + egui::vec2(0., 30.);
        let drag = egui::vec2(canvas.left() + 20. - edge.mid(0).x, 0.);
        edge.frame(vec![moved(grab), middle(grab, true)]);
        edge.frame(vec![moved(grab + drag)]);
        edge.frame(vec![middle(grab + drag, false)]);
        let hugging = label(&edge.frame(vec![]), "60 mm");
        assert!((hugging.rect.left() - (canvas.left() + 3.)).abs() < 0.01);
        let before = edge.facts();
        let at = hugging.rect.left_center() + egui::vec2(0.2, 0.);
        let outside = egui::pos2(canvas.left() - 0.8, at.y);
        assert!(!canvas.contains(outside) && at.distance(outside) <= 6.);
        edge.frame(vec![moved(at), primary(at, true)]);
        edge.frame(vec![moved(outside), primary(outside, false)]);
        assert_eq!(
            edge.facts(),
            before,
            "{pixels_per_point}x: release outside took a label"
        );
        assert!(!edge.canvas().pressed());

        // Escape, a lost focus and a vanished pointer end a press without a trace.
        for ending in ["escape", "focus", "gone"] {
            let mut rig = Rig::lengths(pixels_per_point);
            let ids = rig.ids.clone();
            rig.target(Target::Dimension);
            rig.e.pick_for_tests(ids[2]);
            let before = rig.facts();
            let at = rig.at_label("60 mm");
            rig.frame(vec![moved(at), primary(at, true)]);
            rig.frame(vec![match ending {
                "escape" => escape(),
                "focus" => egui::Event::WindowFocused(false),
                _ => egui::Event::PointerGone,
            }]);
            rig.frame(vec![
                egui::Event::WindowFocused(true),
                moved(at),
                primary(at, false),
            ]);
            assert_eq!(rig.facts(), before, "{pixels_per_point}x: {ending}");
        }

        // A shift press on a label is a pan's: it never takes the label, moving or not.
        let mut rig = Rig::lengths(pixels_per_point);
        let ids = rig.ids.clone();
        rig.target(Target::Dimension);
        rig.e.pick_for_tests(ids[2]);
        let before = rig.facts();
        let at = rig.at_label("60 mm");
        rig.frame(vec![moved(at), shift_primary(at, true)]);
        rig.frame(vec![shift_primary(at, false)]);
        assert_eq!(
            rig.facts(),
            before,
            "{pixels_per_point}x: a shift click took a label"
        );
    }

    // Busy, unfocused, covered, outside and queued forms: the label, the field and the action
    // take nothing, and take it again as soon as nothing blocks them.
    let mut rig = Rig::lengths(1.);
    let ids = rig.ids.clone();
    let (d60, d30) = (rule_of(&rig.stored, 60.), rule_of(&rig.stored, 30.));
    rig.target(Target::Dimension);
    let at = rig.at_label("60 mm");
    rig.click(at);
    rig.settle();
    rig.type_length("70");
    let base = rig.facts();
    let (aim, other) = (at, rig.at_label("30 mm"));
    let attempt = |rig: &mut Rig, running: bool, focused: bool, cover: Option<egui::Rect>| {
        for events in [
            vec![moved(other)],
            vec![primary(other, true)],
            vec![primary(other, false)],
            vec![moved(aim)],
            vec![primary(aim, true)],
            vec![primary(aim, false)],
        ] {
            rig.frame_as(events, running, focused, cover);
        }
    };
    attempt(&mut rig, true, true, None);
    assert_eq!(rig.facts(), base, "busy");
    attempt(&mut rig, false, false, None);
    assert_eq!(rig.facts(), base, "unfocused");
    let canvas = rig.rect();
    for _ in 0..3 {
        rig.frame_as(vec![], false, true, Some(canvas));
    }
    attempt(&mut rig, false, true, Some(canvas));
    assert_eq!(rig.facts(), base, "covered");
    for _ in 0..3 {
        rig.frame(vec![]);
    }
    // The field takes no text and the action no press while a job runs, though the field holds
    // the focus it had; the typed text is exactly what it was.
    enter_field(&rig.ctx, &mut rig.e, "New length", "99", true);
    click_running(&rig.ctx, &mut rig.e, "Replace dimension", true);
    assert_eq!(
        (rig.typed().as_str(), rig.edits()),
        ("70", SketchConstraintEdits::default())
    );
    // Prepared once, a request of the form waits to be taken: nothing on the canvas, no text in
    // the field, no action begins.
    rig.type_length("70");
    rig.press("Replace dimension");
    let prepared = rig.edits();
    assert_eq!(prepared.remove, vec![d60]);
    rig.press("Save constraints copy…");
    assert!(rig.e.pending.is_some());
    let queued = rig.facts();
    attempt(&mut rig, false, true, None);
    enter_field(&rig.ctx, &mut rig.e, "New length", "99", false);
    rig.press("Replace dimension");
    press_exact(&mut rig, "Dimension");
    rig.press("Fit");
    assert_eq!(rig.facts(), queued, "queued");
    assert_eq!(rig.typed(), "70");
    assert_eq!(
        rig.e.take_request().expect("the waiting request").edits,
        prepared
    );
    // Nothing blocks any more: the other label is taken and its dimension replaced.
    rig.click(other);
    assert_eq!(rig.picks().0, Some(ids[1]));
    rig.settle();
    rig.type_length("33");
    rig.press("Replace dimension");
    assert_eq!(rig.edits().remove, vec![d60, d30]);
    println!("\nFCAD_31L_FORM_BLOCKED_EXECUTED");
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
    println!("\nFCAD_31L_FORM_768_EXECUTED");
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
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        ring.constraint_sketches[0].sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![radius_add(ids[0], 10.5), radius_add(ids[1], 4.5)],
        },
    )
    .expect("prepare");
    document.write_sketch_constraints(&prepared).expect("rules");
    let ring = ExtrudeEditSource::read(&document).expect("catalogue");
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
    println!("\nFCAD_31L_FORM_UNAVAILABLE_EXECUTED");
}

fn dimension_stored(source: &ExtrudeEditSource) -> Sketch {
    source.constraint_sketches[0]
        .stored
        .clone()
        .expect("stored")
}

#[test]
fn two_forms_over_one_document_keep_their_own_selection_text_and_pending_replacement() {
    // Copies of a document share their curve and rule UUIDs; the forms must not.
    let (_root, path, source) = lengths_document();
    let (d60, d30) = (
        rule_of(&dimension_stored(&source), 60.),
        rule_of(&dimension_stored(&source), 30.),
    );
    let mut a = Rig::over(None, &path, &source, 1., enlarged);
    let mut b = Rig::over(None, &path, &source, 1., enlarged);
    a.target(Target::Dimension);
    b.target(Target::Dimension);
    let at = a.at_label("60 mm");
    a.click(at);
    a.settle();
    a.type_length("70");
    a.press("Replace dimension");
    let at = b.at_label("30 mm");
    b.click(at);
    b.settle();
    b.type_length("33");
    assert_eq!((a.picks().0, b.picks().0), (Some(a.ids[0]), Some(b.ids[1])));
    assert_eq!(a.ids, b.ids, "one document, one set of UUIDs");
    assert_eq!((a.typed().as_str(), b.typed().as_str()), ("70", "33"));
    let (wa, wb) = (a.frame(vec![]), b.frame(vec![]));
    assert!(has_label(&wa, "60 → 70 mm") && has_label(&wa, "30 mm"));
    assert!(has_label(&wb, "60 mm") && has_label(&wb, "30 mm"));
    assert_eq!(a.edits().remove, vec![d60]);
    assert_eq!(b.edits(), SketchConstraintEdits::default());
    b.press("Replace dimension");
    assert_eq!(b.edits().remove, vec![d30]);
    assert_eq!(a.edits().remove, vec![d60], "the other form is untouched");
    assert!(has_label(&a.frame(vec![]), "30 mm"));
    assert!(has_label(&b.frame(vec![]), "30 → 33 mm") && has_label(&b.frame(vec![]), "60 mm"));
    println!("\nFCAD_31L_FORM_FORMS_EXECUTED");
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
    println!("\nFCAD_31L_FORM_OVERLAP_EXECUTED");
}

// ---- native: the real scene of a real rebuild and solve -------------------------------

use super::canvas::{accepted_scene, by_lists, line_numbers, presented, read_stl_of};
use super::session_apply::replace_segment_length;

/// A rectangle whose Sketch STORES a rough guess (a 12 × 8 mm parallelogram about the origin)
/// and keeps the rules that solve it into a 60 × 30 mm rectangle around the same place:
/// Lines 1 and 3 horizontal, Lines 2 and 4 vertical, Line 1 60 mm and Line 2 30 mm. Every
/// stored vertex and Line middle then lies inside the solved drawing, far from all of it.
pub(crate) fn write_dimensioned_rectangle(path: &Path) {
    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            path,
            ferritecad_jobs::NewDocument::SketchExtrude(
                ferritecad_document::PolygonExtrusion::new(
                    vec![[-6., -3.], [6., -2.], [7., 5.], [-5., 4.]],
                    10.,
                )
                .expect("a guess"),
            ),
            "test",
        ),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
    .expect("the source");
    let mut document = Document::open(path).expect("document");
    let choice = ExtrudeEditSource::read(&document)
        .expect("catalogue")
        .constraint_sketches[0]
        .clone();
    let ids: Vec<_> = choice
        .stored
        .as_ref()
        .expect("stored")
        .curves
        .iter()
        .map(|c| c.id)
        .collect();
    let line = |i: usize, kind| {
        AddSketchConstraint::Line(AddLineConstraint::Line {
            curve: ids[i],
            kind,
        })
    };
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        choice.sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                line(0, LineConstraintKind::Horizontal),
                line(1, LineConstraintKind::Vertical),
                line(2, LineConstraintKind::Horizontal),
                line(3, LineConstraintKind::Vertical),
                length_add(ids[0], 60.),
                length_add(ids[1], 30.),
            ],
        },
    )
    .expect("prepare");
    document.write_sketch_constraints(&prepared).expect("rules");
    document.close().expect("close");
}

/// How far a rectangle is from a segment, in points: zero when they meet.
fn rect_from_segment(rect: egui::Rect, segment: [egui::Pos2; 2]) -> f32 {
    let samples = 256;
    (0..=samples)
        .map(|i| {
            let p = segment[0] + (segment[1] - segment[0]) * (i as f32 / samples as f32);
            rect.distance_to_pos(p)
        })
        .fold(f32::MAX, f32::min)
}

/// The rule UUIDs that the saved rectangle's dimensions had in the form, to read back after Save:
/// the removed 60 mm rule and the kept 30 mm rule.
pub(crate) type Replaced = (StableEntityId, StableEntityId);

/// §31L, native: the form for the rectangle written by `write_dimensioned_rectangle`, driven by
/// the real widgets over the real accepted drawing. The labels state the saved 60 and 30 mm
/// (not the 12 × 8 mm parallelogram's lengths), stand beside the solved Lines, and the label of
/// 60 mm, chosen under the Dimension target, replaces that rule by 70 mm: the request equals the
/// one the Lines list makes. Returns the form with its Apply asked, as the session gate takes it.
pub(crate) fn native_dimension_rectangle(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    seen: &std::cell::Cell<Option<Replaced>>,
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut rig, loaded) = Rig::native(path, source, sketch);
    let real = presented(&loaded, sketch);
    let (ids, stored) = (rig.ids.clone(), rig.stored.clone());
    let (d60, d30) = (rule_of(&stored, 60.), rule_of(&stored, 30.));
    // The form's drawing IS the rebuild's presentation: a 60 × 30 mm rectangle, not the guess.
    let held = rig.solved();
    let solved: Vec<_> = real.iter().map(|(_, g)| line_numbers(g)).collect();
    for ((id, geometry), curve) in real.iter().zip(&held) {
        let (start, end) = line_numbers(geometry);
        assert_eq!((curve.id, curve.shape), (*id, Shape::Line { start, end }));
    }
    let length =
        |i: usize| (solved[i].0[0] - solved[i].1[0]).hypot(solved[i].0[1] - solved[i].1[1]);
    assert!(
        (length(0) - 60.).abs() < 1e-6 && (length(1) - 30.).abs() < 1e-6,
        "{solved:?}"
    );
    let guess: Vec<_> = stored
        .curves
        .iter()
        .map(|c| line_numbers(&c.geometry))
        .collect();
    let stored_length =
        |i: usize| (guess[i].0[0] - guess[i].1[0]).hypot(guess[i].0[1] - guess[i].1[1]);
    assert!(
        stored_length(0) < 13. && stored_length(1) < 9.,
        "the stored guess is small: {guess:?}"
    );

    // Two labels, the saved targets. No label tells the stored guess's lengths (12.04 and
    // 7.07 mm), and each stands beside the SOLVED Line of its rule.
    let out = rig.frame(vec![]);
    let mut words: Vec<_> = labels(&out).into_iter().map(|l| l.text).collect();
    words.sort();
    assert_eq!(words, ["30 mm", "60 mm"]);
    let (l60, l30) = (label(&out, "60 mm"), label(&out, "30 mm"));
    for (i, l) in [(0, &l60), (1, &l30)] {
        let ends = rig.ends(i);
        let near_solved = rect_from_segment(l.rect, ends);
        assert!(
            near_solved <= 5.,
            "Line {}'s label is {near_solved} points from the solved Line",
            i + 1
        );
        // The box stands at the middle of the SOLVED Line: its offset from that middle has no
        // part along the Line.
        let along = (ends[1] - ends[0]).normalized();
        let off = (l.rect.center() - (ends[0] + (ends[1] - ends[0]) * 0.5)).dot(along);
        assert!(
            off.abs() < 1.,
            "Line {}'s label is {off} points along from its middle",
            i + 1
        );
    }
    eprintln!(
        "NATIVE_DIMENSION_RECT stored_lengths=({:.3},{:.3}) solved_lengths=({:.3},{:.3}) l60={:?} l30={:?}",
        stored_length(0),
        stored_length(1),
        length(0),
        length(1),
        l60.rect,
        l30.rect
    );

    // Under the Object target a click on a label's place takes nothing; under the Dimension target
    // it takes the rule's Line.
    rig.click(l60.rect.center());
    assert_eq!(rig.picks().0, None);
    rig.target(Target::Dimension);
    rig.click(l60.rect.center());
    rig.settle();
    assert_eq!(rig.picks().0, Some(ids[0]));
    rig.type_length("70");
    rig.press("Replace dimension");
    let prepared = rig.edits();
    assert_eq!(prepared.remove, vec![d60]);
    assert_eq!(prepared.add, vec![length_add(ids[0], 70.)]);
    // Prepared only: the label states the replacement beside the target, and the drawing is
    // exactly the rebuild's.
    assert!(has_label(&rig.frame(vec![]), "60 → 70 mm"));
    assert_eq!(rig.solved(), held, "a prepared replacement draws nothing");
    assert_eq!(rig.refusal(), None);

    // The same replacement by the Lines list and the form's own box and button.
    let lists = by_lists(path, source, sketch, |ctx, e| {
        replace_segment_length(ctx, e, "Segment 1", "70");
    });
    assert_eq!(
        prepared, lists.edits,
        "the label and the list asked for different things"
    );
    rig.press("Apply constraints");
    let request = rig.e.take_apply_request().expect("Apply asked");
    assert_eq!(request.edits, lists.edits);
    assert_eq!(
        (request.expected, request.sketch, &request.source),
        (lists.expected, lists.sketch, &lists.source)
    );
    assert_eq!(
        (request.edits.remove.len(), request.edits.add.len()),
        (1, 1)
    );
    seen.set(Some((d60, d30)));
    println!(
        "\nFCAD_31L_NATIVE_RECTANGLE_REQUEST stored_mm=({:.2},{:.2}) solved_mm=(60,30) removes=1 adds=1",
        stored_length(0),
        stored_length(1)
    );
    (rig.e, request)
}

/// §31L, native: the SAVED rectangle read three ways that agree. The form of its cold
/// rebuild holds a 70 mm and a 30 mm label, the 30 mm rule under the very UUID it had and the 70 mm
/// one under a new UUID; the drawing of that rebuild is a closed 70 × 30 mm rectangle; and the
/// exported STL, read by `read_stl` here and not by the writer, is a prism of that size.
pub(crate) fn saved_dimensioned_rectangle(saved: &Path, stl: &[u8], height: f64, was: Replaced) {
    let (loaded, scene) = accepted_scene(saved);
    let facts = scene.edit_source.clone().expect("facts");
    let sketch = facts.constraint_sketches[0].sketch;
    let lines: Vec<_> = presented(&loaded, sketch)
        .iter()
        .map(|(_, g)| line_numbers(g))
        .collect();
    let length = |i: usize| (lines[i].0[0] - lines[i].1[0]).hypot(lines[i].0[1] - lines[i].1[1]);
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
    assert!(
        (length(0) - 70.).abs() < 1e-6 && (length(1) - 30.).abs() < 1e-6,
        "{lines:?}"
    );
    assert!(
        (lines[0].0[1] - lines[0].1[1]).abs() < 1e-6,
        "Line 1 is horizontal"
    );
    assert!(
        (lines[1].0[0] - lines[1].1[0]).abs() < 1e-6,
        "Line 2 is vertical"
    );
    for i in 0..4 {
        assert!(
            near(lines[i].1, lines[(i + 1) % 4].0),
            "the contour is closed at {i}"
        );
    }
    // The cold reopen's form: its own labels, rules by UUID.
    let mut e = Editor::default();
    assert!(e.begin_with(saved, &facts, &scene.sketches, sketch));
    let ctx = egui::Context::default();
    let mut out = frame(&ctx, &mut e, vec![]);
    for _ in 0..3 {
        out = frame(&ctx, &mut e, vec![]);
    }
    let mut words: Vec<_> = labels(&out).into_iter().map(|l| l.text).collect();
    words.sort();
    assert_eq!(words, ["30 mm", "70 mm"]);
    let (dimensions, _) = e.draft_dimensions().expect("a draft");
    let ids: Vec<_> = dimensions.iter().map(|d| (d.id, d.target_mm)).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&(was.1, 30.)), "the kept rule keeps its UUID");
    assert!(
        ids.iter().all(|(id, _)| *id != was.0),
        "the replaced rule is gone for good"
    );
    let new = ids.iter().find(|(_, mm)| *mm == 70.).expect("the new rule");
    assert_ne!(new.0, was.0);
    // The STL: a prism 70 × 30 × height.
    let (faces, volume) = read_stl_of(stl);
    let expected = 70. * 30. * height;
    assert!(
        (volume - expected).abs() <= 1e-4 * expected,
        "the exported solid has volume {volume}, the saved drawing {expected}"
    );
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in faces.iter().flatten() {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    for (got, want) in [
        (hi[0] - lo[0], 70.),
        (hi[1] - lo[1], 30.),
        (hi[2] - lo[2], height),
    ] {
        assert!((got - want).abs() < 1e-3, "extent {got}, not {want}");
    }
}

/// §31L, native: the form for the cylinder of `write_guessed_circle` (the circle (4.5, 0) r 1.2
/// stored, a Radius of 10 and a Fixed centre at the origin kept). The label states the saved
/// R 10, not the stored 1.2; chosen under the Dimension target, a diameter of 17 mm replaces the
/// Radius by 8.5 and leaves the Fixed centre alone; the request equals Remove plus Add diameter.
pub(crate) fn native_dimension_circle(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    seen: &std::cell::Cell<Option<Replaced>>,
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut rig, loaded) = Rig::native(path, source, sketch);
    let real = presented(&loaded, sketch);
    let (circle, stored) = (rig.ids[0], rig.stored.clone());
    let rule = rule_of(&stored, 10.);
    let SketchGeometry::Circle { center, radius } = real[0].1 else {
        panic!("a Circle")
    };
    assert!(
        center.x.abs() < 1e-6 && center.y.abs() < 1e-6 && (radius - 10.).abs() < 1e-6,
        "the solved circle is ({}, {}) r {radius}",
        center.x,
        center.y
    );
    let held = rig.solved();
    assert_eq!(
        held[0].shape,
        Shape::Circle {
            center: [center.x, center.y],
            radius
        }
    );
    let out = rig.frame(vec![]);
    assert_eq!(
        labels(&out)
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["R 10 mm"],
        "the saved target, not the stored 1.2"
    );
    let l = label(&out, "R 10 mm");
    // It stands just outside the SOLVED ring: the nearest point of the ring to the box is
    // within a few points of the box, and the box is not inside the ring.
    let centre = rig.screen([center.x, center.y]);
    let ring = rig.screen([center.x + radius, center.y]).x - centre.x;
    let towards = (l.rect.center() - centre).normalized();
    let gap = l.rect.distance_to_pos(centre + towards * ring);
    assert!(
        gap <= 5. && (l.rect.center() - centre).length() > ring,
        "the label is {gap} points from the solved ring"
    );
    rig.target(Target::Dimension);
    rig.click(l.rect.center());
    rig.settle();
    assert_eq!(rig.picks().0, Some(circle));
    press_exact(&mut rig, "Diameter");
    rig.type_amount("17");
    rig.press("Replace dimension");
    let prepared = rig.edits();
    assert_eq!(prepared.remove, vec![rule]);
    assert_eq!(prepared.add, vec![radius_add(circle, 8.5)]);
    assert!(has_label(&rig.frame(vec![]), "R 10 → 8.5 mm"));
    assert_eq!(rig.solved(), held, "a prepared replacement draws nothing");
    let lists = by_lists(path, source, sketch, |ctx, e| {
        click(ctx, e, &format!("Circle · {}", short(circle)));
        click_remove_on_row(ctx, e, "Radius 10 mm");
        enter_field(ctx, e, "Diameter mm", "17", false);
        click(ctx, e, "Add diameter");
    });
    assert_eq!(
        prepared, lists.edits,
        "the label and the lists asked for different things"
    );
    rig.press("Apply constraints");
    let request = rig.e.take_apply_request().expect("Apply asked");
    assert_eq!(request.edits, lists.edits);
    assert_eq!(
        (request.edits.remove.len(), request.edits.add.len()),
        (1, 1)
    );
    let fixed = stored
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::Fixed { .. }))
        .expect("the Fixed centre");
    seen.set(Some((rule, fixed.id)));
    println!(
        "\nFCAD_31L_NATIVE_CIRCLE_REQUEST stored=(4.5,0,1.2) solved=(0,0,10) diameter=17 removes=1 adds=1"
    );
    (rig.e, request)
}

/// §31L, native: the SAVED cylinder. The form of its cold rebuild holds R 8.5 with a new UUID and
/// the Fixed centre under its own; the drawing of that rebuild is the circle (0, 0) r 8.5; every
/// rim vertex of the exported STL, read here and not by the writer, is on it.
pub(crate) fn saved_dimensioned_circle(saved: &Path, stl: &[u8], height: f64, was: Replaced) {
    let (loaded, scene) = accepted_scene(saved);
    let facts = scene.edit_source.clone().expect("facts");
    let sketch = facts.constraint_sketches[0].sketch;
    let SketchGeometry::Circle { center, radius } = presented(&loaded, sketch)[0].1 else {
        panic!("a Circle")
    };
    assert!(
        center.x.abs() < 1e-6 && center.y.abs() < 1e-6 && (radius - 8.5).abs() < 1e-6,
        "({}, {}) r {radius}",
        center.x,
        center.y
    );
    let mut e = Editor::default();
    assert!(e.begin_with(saved, &facts, &scene.sketches, sketch));
    let ctx = egui::Context::default();
    let mut out = frame(&ctx, &mut e, vec![]);
    for _ in 0..3 {
        out = frame(&ctx, &mut e, vec![]);
    }
    assert_eq!(
        labels(&out)
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["R 8.5 mm"]
    );
    let (dimensions, _) = e.draft_dimensions().expect("a draft");
    assert_eq!(dimensions.len(), 1);
    assert_ne!(dimensions[0].id, was.0, "the Radius is a new rule");
    let kept = facts.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .constraints
        .iter()
        .any(|c| c.id == was.1);
    assert!(kept, "the Fixed centre keeps its UUID");
    let (faces, volume) = read_stl_of(stl);
    let exact = std::f64::consts::PI * radius * radius * height;
    assert!(
        volume <= exact * (1. + 1e-9) && volume >= 0.97 * exact,
        "{volume} vs {exact}"
    );
    let rim: Vec<_> = faces
        .iter()
        .flatten()
        .filter(|p| p[2].abs() < 1e-4)
        .collect();
    assert!(rim.len() >= 24, "{} rim vertices", rim.len());
    for p in rim {
        let d = (p[0] - center.x).hypot(p[1] - center.y);
        assert!(
            (d - radius).abs() < 1e-3,
            "a rim vertex {p:?} is {d} from the centre"
        );
    }
}
