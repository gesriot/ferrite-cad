// SPDX-License-Identifier: MIT
//! §31M: the labels of the saved directed angles on the accepted drawing of the constraints form,
//! the selection they share with the lists and the persisted rows, and the explicit replacement of a
//! saved angle, through the form's real widgets and real egui events.
//!
//! The drawings here are the app's own `Curve`s moved and enlarged over the stored curves'
//! identities: **synthetic** (see `canvas.rs`). The saved angles (−70°, 110°, 180°) are deliberately
//! not the turns of the stored plate or of the drawing (−90°, 90°, 180° measured by `atan2`), so a
//! label that measured anything instead of stating the rule fails. The real presentation is read by
//! the native gate at the end of this file and in `sessions.rs`.
#![allow(clippy::panic)]
use super::canvas::{
    Rig, enlarged, escape, middle, moved, near, primary, shift_primary, wheel, whole,
};
use super::dimension::{
    band, dimension_stored, has_label, is_colour, label, labels, rule_of, selected_strokes,
};
use super::*;
use crate::sketch::accepted::{
    Curve, Drawing, HOVER_COLOUR, PENDING_COLOUR, SELECTED_COLOUR, Shape, Target,
};

// ---- fixtures -------------------------------------------------------------------------

/// The plate of §31L (two saved lengths, 60 and 30 mm, a Horizontal and a Vertical) that also
/// keeps three saved directed angles between its Lines, in the order of the stored rules R0, R1, R2:
///
/// * R0: Line 1 start>end to Line 2 start>end, **−70°**;
/// * R1: Line 2 **end>start** (the reversed vector) to Line 3 start>end, **110°**;
/// * R2: Line 1 start>end to Line 3 start>end (opposite sides), **+180°**.
///
/// The rules were written in the order R2, R0, R1 and then stored in the order R0, R1, R2, so
/// their UUIDs (R2 < R0 < R1) run neither with nor against their stored order: an order decided by
/// UUID, ascending or descending, is not the saved one.
fn angles_document() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let (root, path, _) = fixture();
    persist_rectangle_lengths(&path);
    let mut document = Document::open(&path).expect("document");
    let source = ExtrudeEditSource::read(&document).expect("catalogue");
    let choice = &source.constraint_sketches[0];
    let ids: Vec<_> = choice
        .stored
        .as_ref()
        .expect("stored")
        .curves
        .iter()
        .map(|c| c.id)
        .collect();
    let [(a0, b0, d0), (a1, b1, d1), (a2, b2, d2)] = saved_angles(&ids);
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        choice.sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                angle_add(a2, b2, d2),
                angle_add(a0, b0, d0),
                angle_add(a1, b1, d1),
            ],
        },
    )
    .expect("prepare the angles");
    document
        .write_sketch_constraints(&prepared)
        .expect("angles");
    rotate_angle_rules(&mut document, choice.sketch);
    let source = ExtrudeEditSource::read(&document).expect("catalogue");
    document.close().expect("close");
    (root, path, source)
}

/// Stores the saved DirectedAngle rules of the Sketch one place on (the last first), in the same places.
fn rotate_angle_rules(document: &mut Document, sketch: ObjectId) {
    let mut object = document.object(sketch).expect("read").expect("sketch");
    if let ferritecad_document::ObjectPayload::Sketch(s) = &mut object.payload {
        let at: Vec<_> = (0..s.constraints.len())
            .filter(|&i| {
                matches!(
                    s.constraints[i].rule,
                    SketchConstraintRule::DirectedAngle { .. }
                )
            })
            .collect();
        let mut rules: Vec<_> = at.iter().map(|&i| s.constraints[i]).collect();
        rules.rotate_left(1);
        for (i, rule) in at.into_iter().zip(rules) {
            s.constraints[i] = rule;
        }
    }
    document
        .write(|w| {
            w.put_object(
                object.id,
                object.parent,
                object.ordinal,
                object.name.as_deref(),
                &object.payload,
            )
        })
        .expect("angles in stored order");
}

type Saved = (DirectedLine, DirectedLine, f64);

/// The three saved angles of `angles_document`, by the curves of the plate.
fn saved_angles(ids: &[StableEntityId]) -> [Saved; 3] {
    [
        (
            vector(ids, 0, LineEndpoint::Start),
            vector(ids, 1, LineEndpoint::Start),
            -70.,
        ),
        (
            vector(ids, 1, LineEndpoint::End),
            vector(ids, 2, LineEndpoint::Start),
            110.,
        ),
        (
            vector(ids, 0, LineEndpoint::Start),
            vector(ids, 2, LineEndpoint::Start),
            180.,
        ),
    ]
}

fn vector(ids: &[StableEntityId], i: usize, from: LineEndpoint) -> DirectedLine {
    DirectedLine::new(ids[i], from, from.other()).expect("two different ends")
}

fn angle_add(a: DirectedLine, b: DirectedLine, degrees: f64) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::DirectedAngle {
        a,
        b,
        angle: DirectedAngle::from_degrees(degrees).expect("an angle in range"),
    })
}

/// The UUIDs of the saved angle rules of a stored Sketch, in the order the rules are stored, read
/// here from the stored rules and not through the form.
fn angle_rules(stored: &Sketch) -> Vec<StableEntityId> {
    stored
        .constraints
        .iter()
        .filter(|c| matches!(c.rule, SketchConstraintRule::DirectedAngle { .. }))
        .map(|c| c.id)
        .collect()
}

impl Rig {
    /// The form over the plate with three saved angles, handed the enlarged drawing.
    fn angles(pixels_per_point: f32) -> Self {
        let (root, path, source) = angles_document();
        Self::over(Some(root), &path, &source, pixels_per_point, enlarged)
    }
    /// The rule of the selected saved angle, if one is selected.
    fn turn(&self) -> Option<StableEntityId> {
        self.e.draft_turns().expect("a draft").1
    }
    /// The text typed to replace a saved angle, in degrees.
    fn degrees_typed(&self) -> String {
        self.e.draft_turns().expect("a draft").2
    }
    fn type_angle(&mut self, text: &str) {
        enter_field(&self.ctx, &mut self.e, "New angle", text, false);
    }
    /// Everything a selection must leave alone, and the angle that is selected.
    fn angle_facts(&self) -> (super::canvas::Facts, Option<StableEntityId>) {
        (self.facts(), self.turn())
    }
    /// The pair slots, the way each vector runs and the angle text: unapplied input for a new rule.
    fn slots(&self) -> super::AngleState {
        self.e.draft_angle().expect("a draft")
    }
    /// Select the saved angle whose label says `words`, under the Dimension target.
    fn choose(&mut self, words: &str) {
        if self.canvas().target() != Target::Dimension {
            self.target(Target::Dimension);
        }
        let at = self.at_label(words);
        self.click(at);
        self.settle();
    }
}

// ---- reading the painted frame --------------------------------------------------------

/// The arrowheads of a colour in a frame: the filled triangles, tip first.
fn arrowheads(out: &egui::FullOutput, colour: egui::Color32) -> Vec<Vec<egui::Pos2>> {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Path(p)
                if p.closed && p.points.len() == 3 && is_colour(p.fill, colour) =>
            {
                Some(p.points.clone())
            }
            _ => None,
        })
        .collect()
}

/// The centres of the one-letter words painted in a colour (the A and B of a marked angle).
fn letters(out: &egui::FullOutput, colour: egui::Color32) -> Vec<(String, egui::Pos2)> {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t)
                if ["A", "B"].contains(&t.galley.text()) && is_colour(t.fallback_color, colour) =>
            {
                Some((
                    t.galley.text().to_owned(),
                    t.visual_bounding_rect().center(),
                ))
            }
            _ => None,
        })
        .collect()
}

/// The turn from `a` to `b` in the drawing, counter-clockwise in degrees, Y up: `atan2(cross, dot)`
/// of the two vectors as the rule holds them. The screen flips Y, so the drawing's own numbers are
/// read, not the screen's.
fn measured(drawing: &[Curve], a: DirectedLine, b: DirectedLine) -> f64 {
    let run = |v: DirectedLine| {
        let Shape::Line { start, end } = drawing
            .iter()
            .find(|c| c.id == v.curve())
            .expect("a drawn Line")
            .shape
        else {
            panic!("a Line")
        };
        let (from, to) = match v.from() {
            LineEndpoint::Start => (start, end),
            LineEndpoint::End => (end, start),
        };
        [to[0] - from[0], to[1] - from[1]]
    };
    let (u, v) = (run(a), run(b));
    (u[0] * v[1] - u[1] * v[0])
        .atan2(u[0] * v[0] + u[1] * v[1])
        .to_degrees()
}

fn point_near(a: egui::Pos2, b: egui::Pos2) -> bool {
    a.distance(b) < 0.05
}

// ---- the labels and their selection ---------------------------------------------------

#[test]
fn a_label_states_the_saved_signed_target_and_selects_exactly_its_rule_with_both_vectors_only_under_the_dimension_target()
 {
    for pixels_per_point in [1., 2.] {
        let mut rig = Rig::angles(pixels_per_point);
        let ids = rig.ids.clone();
        let rules = angle_rules(&rig.stored);
        let [(a0, b0, _), (a1, b1, _), (a2, b2, _)] = saved_angles(&ids);
        assert_eq!(rules.len(), 3);
        assert!(
            rules[2].to_string() < rules[0].to_string()
                && rules[0].to_string() < rules[1].to_string(),
            "the fixture stores the rules in an order that is neither ascending nor descending by UUID"
        );
        // The form reads each angle by the UUID of its rule, in stored order, with both exact
        // vectors and the validated angle: not a pair, not a number.
        let (turns, none, typed) = rig.e.draft_turns().expect("draft");
        assert_eq!(
            turns
                .iter()
                .map(|t| (t.id, t.a, t.b, t.angle.degrees()))
                .collect::<Vec<_>>(),
            vec![
                (rules[0], a0, b0, -70.),
                (rules[1], a1, b1, 110.),
                (rules[2], a2, b2, 180.)
            ],
            "{pixels_per_point}x"
        );
        assert_eq!((none, typed.as_str()), (None, ""));
        // The saved targets are not what the stored plate or the drawing measure.
        let drawn = rig.solved();
        for (a, b, saved) in [(a0, b0, -70.), (a1, b1, 110.)] {
            let turn = measured(&drawn, a, b);
            assert!(
                (turn.abs() - 90.).abs() < 1e-9 && (turn - saved).abs() > 15.,
                "{turn}"
            );
        }
        assert!(
            (measured(&drawn, a2, b2) - 180.).abs() < 1e-9
                || (measured(&drawn, a2, b2) + 180.).abs() < 1e-9
        );

        // Five labels, the saved targets of the two lengths and of the three angles, the sign and the
        // order A>B told.
        let out = rig.frame(vec![]);
        let mut words: Vec<_> = labels(&out).into_iter().map(|l| l.text).collect();
        words.sort();
        assert_eq!(
            words,
            ["30 mm", "60 mm", "A>B -70°", "A>B 110°", "A>B 180°"],
            "{pixels_per_point}x"
        );
        // A label of a pair stands at the middle of the middles of its two solved Lines.
        for (words, i, j) in [("A>B -70°", 0, 1), ("A>B 110°", 1, 2), ("A>B 180°", 0, 2)] {
            let want = rig.mid(i) + (rig.mid(j) - rig.mid(i)) * 0.5;
            let at = label(&out, words).rect.center();
            assert!(
                band(rig.rect()).contains_rect(label(&out, words).rect) && at.distance(want) < 0.01,
                "{words}: {at:?} {want:?}"
            );
        }
        // Pair slots and their directions are the unapplied input for a new rule: set, and kept.
        rig.press("Segment 4");
        rig.press("Pair Line A");
        rig.press("Segment 3");
        rig.press("Pair Line B");
        rig.press("B: start>end");
        let slots = rig.slots();
        assert_eq!(slots.0, (Some(ids[3]), Some(ids[2])));
        assert_eq!(slots.1, (LineEndpoint::Start, LineEndpoint::End));

        // Under Object a label is a picture: a click on the place of a label takes nothing.
        assert_eq!(rig.canvas().target(), Target::Object);
        for words in ["A>B -70°", "A>B 110°", "A>B 180°"] {
            let at = rig.at_label(words);
            assert_eq!(
                rig.canvas().would_pick(&rig.solved(), Target::Object, at),
                None
            );
            rig.e.pick_for_tests(ids[2]);
            rig.click(at);
            assert_eq!((rig.picks().0, rig.turn()), (Some(ids[2]), None), "{words}");
        }
        // Under Dimension the label of R0 is picked, by its box, and only it.
        rig.target(Target::Dimension);
        let at = rig.at_label("A>B -70°");
        let kept = rig.kept();
        rig.click(at);
        rig.settle();
        assert_eq!(rig.turn(), Some(rules[0]), "{pixels_per_point}x");
        assert_eq!(
            rig.picks().0,
            None,
            "an angle belongs to two Lines: neither is the selection"
        );
        assert_eq!(
            rig.kept(),
            kept,
            "a selection is no input, no step, no request"
        );
        assert_eq!(
            rig.slots(),
            slots,
            "the pair slots and their directions are not reassigned"
        );

        // Both solved Lines of the rule are marked in the selection's colour, with the way each
        // vector runs: the arrowhead stands four fifths along from the vector's `from` end toward its `to`
        // end, and the letters A and B stand by the Lines. The other two Lines are not marked.
        let out = rig.frame(vec![]);
        let marked = selected_strokes(&out);
        let ends: Vec<_> = (0..4).map(|i| rig.ends(i)).collect();
        for i in [0, 1] {
            assert!(
                marked
                    .iter()
                    .any(|s| near(s[0], ends[i][0]) && near(s[1], ends[i][1])),
                "Line {} is not marked",
                i + 1
            );
        }
        for i in [2, 3] {
            assert!(
                !marked
                    .iter()
                    .any(|s| near(s[0], ends[i][0]) && near(s[1], ends[i][1])),
                "Line {} is marked",
                i + 1
            );
        }
        let heads = arrowheads(&out, SELECTED_COLOUR);
        assert_eq!(heads.len(), 2, "{pixels_per_point}x");
        for (curve, from_start) in [(0, true), (1, true)] {
            let [s, e] = ends[curve];
            let (from, to) = if from_start { (s, e) } else { (e, s) };
            let tip = from + (to - from) * 0.8;
            assert!(
                heads.iter().any(|h| point_near(h[0], tip)),
                "the vector on Line {} does not run start>end",
                curve + 1
            );
        }
        let marks = letters(&out, SELECTED_COLOUR);
        assert_eq!(
            marks.iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>(),
            ["A", "B"]
        );
        for ((_, at), i) in marks.iter().zip([0, 1]) {
            assert!(at.distance(rig.mid(i)) < 20., "{at:?} by Line {}", i + 1);
        }
        // The label, the row and the words on the canvas say the rule by its UUID and number.
        let chosen = label(&out, "A>B -70°");
        assert!(is_colour(chosen.stroke.color, SELECTED_COLOUR));
        for words in ["A>B 110°", "A>B 180°", "60 mm", "30 mm"] {
            assert!(
                !is_colour(label(&out, words).stroke.color, SELECTED_COLOUR),
                "{words}"
            );
        }
        assert!(painted(
            &out,
            &format!("Selected dimension {} · A>B -70°", short(rules[0]))
        ));
        assert!(painted(
            &out,
            &format!("Directed angle -70° · {}", rules[0])
        ));
        assert!(painted(&out, "New angle") && painted(&out, "Replace dimension"));
        assert!(!painted(&out, "New length"));

        // The top edge says an angle is selected under the other targets too, never `Selected: none`.
        rig.target(Target::Object);
        assert!(painted(
            &rig.frame(vec![]),
            &format!("Selected: angle {}", short(rules[0]))
        ));
        rig.target(Target::Dimension);

        // R1: the first vector is Line 2 run from its End, and the arrow says so.
        let kept = rig.kept();
        let at = rig.at_label("A>B 110°");
        rig.click(at);
        rig.settle();
        assert_eq!(rig.turn(), Some(rules[1]));
        let out = rig.frame(vec![]);
        let heads = arrowheads(&out, SELECTED_COLOUR);
        let [s2, e2] = ends[1];
        let [s3, e3] = ends[2];
        assert!(
            heads.iter().any(|h| point_near(h[0], e2 + (s2 - e2) * 0.8)),
            "Line 2 must run end>start"
        );
        assert!(heads.iter().any(|h| point_near(h[0], s3 + (e3 - s3) * 0.8)));
        assert!(!heads.iter().any(|h| point_near(h[0], s2 + (e2 - s2) * 0.8)));
        assert!(
            selected_strokes(&out)
                .iter()
                .any(|s| near(s[0], s2) && near(s[1], e2))
                && !selected_strokes(&out)
                    .iter()
                    .any(|s| near(s[0], ends[0][0]) && near(s[1], ends[0][1]))
        );
        assert_eq!(rig.kept(), kept);

        // The hover caption names the rule, the order and the way each vector runs, and leaves the
        // canvas captions readable.
        let hovering = rig.at_label("A>B 110°");
        let out = rig.frame(vec![moved(hovering)]);
        let caption = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if t.galley.text()
                        == format!(
                            "Directed angle 110° · saved rule {} · A: Line {} end>start · B: Line {} start>end",
                            short(rules[1]),
                            short(ids[1]),
                            short(ids[2])
                        ) =>
                {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()).expand(2.))
                }
                _ => None,
            })
            .expect("the angle's hover caption");
        let canvas = rig.rect();
        let content = egui::Rect::from_min_max(
            canvas.min + egui::vec2(3., 20.),
            canvas.max - egui::vec2(3., 24.),
        );
        assert!(
            content.contains_rect(caption),
            "{pixels_per_point}x: {caption:?} in {content:?}"
        );
        let yellow = Rig::strokes(&out, HOVER_COLOUR);
        for i in [1, 2] {
            assert!(
                yellow
                    .iter()
                    .any(|s| near(s[0], ends[i][0]) && near(s[1], ends[i][1])),
                "hovering the label marks Line {}",
                i + 1
            );
        }

        // A label of a length takes the Line it dimensions and gives the angle up: exactly one editor.
        let at = rig.at_label("60 mm");
        rig.click(at);
        rig.settle();
        let out = rig.frame(vec![]);
        assert_eq!((rig.picks().0, rig.turn()), (Some(ids[0]), None));
        assert!(painted(&out, "New length") && !painted(&out, "New angle"));
        assert!(arrowheads(&out, SELECTED_COLOUR).is_empty());
        // And the label of an angle gives the Line up.
        let at = rig.at_label("A>B 180°");
        rig.click(at);
        rig.settle();
        let out = rig.frame(vec![]);
        assert_eq!((rig.picks().0, rig.turn()), (None, Some(rules[2])));
        assert!(painted(&out, "New angle") && !painted(&out, "New length"));
        // Line 1 is also in R0 and Line 3 in R1: the selection is the RULE, so only R2's two Lines are
        // marked (Line 1 and Line 3, both run start>end) and Line 2 is not.
        let marked = selected_strokes(&out);
        for (i, on) in [(0, true), (1, false), (2, true), (3, false)] {
            assert_eq!(
                marked
                    .iter()
                    .any(|s| near(s[0], ends[i][0]) && near(s[1], ends[i][1])),
                on,
                "Line {} with R2 selected",
                i + 1
            );
        }
        let heads = arrowheads(&out, SELECTED_COLOUR);
        assert_eq!(heads.len(), 2);
        for i in [0, 2] {
            let [s, e] = ends[i];
            assert!(heads.iter().any(|h| point_near(h[0], s + (e - s) * 0.8)));
        }
        // The Line itself, the empty canvas and the stored plate are not labels in this mode.
        rig.click(rig.mid(3));
        rig.click(rig.stored_mid(1));
        assert_eq!(rig.turn(), Some(rules[2]));
        assert!(rig.e.take_request().is_none() && rig.e.take_apply_request().is_none());
    }
}

#[test]
fn the_labels_the_lines_list_and_the_persisted_rows_show_one_selection_and_the_pair_slots_never_move()
 {
    let mut rig = Rig::angles(1.);
    let ids = rig.ids.clone();
    let rules = angle_rules(&rig.stored);
    let row = |i: usize, words: &str| format!("Directed angle {words}° · {}", rules[i]);
    let before = rig.kept();
    assert!(
        !painted(&rig.frame(vec![]), "New angle"),
        "nothing is selected"
    );

    // A persisted row selects its angle, as a label does: both Lines, the label and the editor.
    rig.press_row(&row(1, "110"));
    assert_eq!((rig.turn(), rig.picks().0), (Some(rules[1]), None));
    rig.settle();
    let out = rig.frame(vec![]);
    assert!(is_colour(
        label(&out, "A>B 110°").stroke.color,
        SELECTED_COLOUR
    ));
    assert!(painted(&out, "New angle") && painted(&out, "Replace dimension"));
    assert_eq!(rig.kept(), before, "choosing a row is no step and no input");

    // The Lines list selects a Line and the angle is given up; a Line with a length shows that
    // editor instead; a Line without one shows none.
    rig.press("Segment 3");
    rig.settle();
    let out = rig.frame(vec![]);
    assert_eq!((rig.picks().0, rig.turn()), (Some(ids[2]), None));
    assert!(!painted(&out, "New angle") && !painted(&out, "Replace dimension"));
    assert!(arrowheads(&out, SELECTED_COLOUR).is_empty());
    rig.press("Segment 1");
    rig.settle();
    let out = rig.frame(vec![]);
    assert!(painted(&out, "New length") && !painted(&out, "New angle"));

    // The row of the other family gives the angle up the same way, and the angle's row takes the
    // Line's selection back.
    rig.press_row(&row(0, "-70"));
    assert_eq!((rig.turn(), rig.picks().0), (Some(rules[0]), None));
    rig.press_row(&format!(
        "Line length 30 mm · {}",
        rule_of(&rig.stored, 30.)
    ));
    assert_eq!((rig.turn(), rig.picks().0), (None, Some(ids[1])));
    rig.press_row(&row(2, "180"));
    assert_eq!((rig.turn(), rig.picks().0), (Some(rules[2]), None));

    // Pair A and Pair B on the canvas leave the angle selection and the whole of the other input
    // alone, and what they fill is the slots only.
    rig.target(Target::PairA);
    rig.click(rig.mid(3));
    rig.target(Target::PairB);
    rig.click(rig.mid(1));
    assert_eq!(rig.turn(), Some(rules[2]));
    assert_eq!(rig.slots().0, (Some(ids[3]), Some(ids[1])));
    assert_eq!(rig.picks().0, None);
    // An Object click on a Line is the Line's selection and the angle is given up.
    rig.target(Target::Object);
    rig.click(rig.mid(2));
    assert_eq!((rig.turn(), rig.picks().0), (None, Some(ids[2])));
    // The label of an angle, from the Lines list, and back.
    rig.choose("A>B -70°");
    assert_eq!((rig.turn(), rig.picks().0), (Some(rules[0]), None));
    rig.press("Segment 2");
    assert_eq!((rig.turn(), rig.picks().0), (None, Some(ids[1])));
    assert_eq!(rig.slots().0, (Some(ids[3]), Some(ids[1])));
}

// ---- the replacement ------------------------------------------------------------------

#[test]
fn replace_dimension_for_a_saved_angle_prepares_one_step_keeps_both_vectors_updates_in_place_and_the_saved_angle_takes_it_back()
 {
    for pixels_per_point in [1., 2.] {
        let mut rig = Rig::angles(pixels_per_point);
        let ids = rig.ids.clone();
        let rules = angle_rules(&rig.stored);
        let [(a0, b0, _), (a1, b1, _), (a2, b2, _)] = saved_angles(&ids);
        let (drawn, view, stored) = (
            rig.solved(),
            rig.canvas().view(),
            rig.e.draft.as_ref().expect("draft").choice.stored.clone(),
        );
        rig.choose("A>B -70°");
        assert_eq!(rig.turn(), Some(rules[0]));

        // Typing is no step. One Replace is one remove and one add, of the SAME two exact vectors.
        rig.type_angle("110");
        assert_eq!(
            (rig.degrees_typed().as_str(), rig.history()),
            ("110", (0, 0))
        );
        assert!(rig.edits() == SketchConstraintEdits::default());
        rig.press("Replace dimension");
        let first = rig.edits();
        assert_eq!(first.remove, vec![rules[0]], "{pixels_per_point}x");
        assert_eq!(first.add, vec![angle_add(a0, b0, 110.)]);
        assert_eq!(rig.history(), (1, 0));
        assert_eq!(rig.refusal(), None);
        // The label states the target and, beside it, what is prepared; the drawing, the view and
        // the stored Sketch are where they were, and the saved rule is still in the list.
        let out = rig.frame(vec![]);
        assert!(has_label(&out, "A>B -70 -> 110°") && !has_label(&out, "A>B -70°"));
        assert!(painted(
            &out,
            &format!("Selected dimension {} · A>B -70 -> 110°", short(rules[0]))
        ));
        assert!(painted(
            &out,
            &format!("Directed angle -70° · {}", rules[0])
        ));
        assert!(painted(
            &out,
            "Pending: 110° in place of -70°; the drawing keeps the accepted angle until Apply constraints."
        ));
        assert_eq!(rig.solved(), drawn, "a prepared change draws nothing");
        assert_eq!(rig.canvas().view(), view);
        assert_eq!(rig.e.draft.as_ref().expect("draft").choice.stored, stored);
        assert!(
            Rig::strokes(&out, PENDING_COLOUR).is_empty(),
            "no preview stroke"
        );
        // Another selection shows the prepared label by its own colour, not the selection's.
        rig.press("Segment 3");
        rig.settle();
        let out = rig.frame(vec![]);
        assert!(is_colour(
            label(&out, "A>B -70 -> 110°").stroke.color,
            PENDING_COLOUR
        ));
        rig.choose("A>B -70 -> 110°");
        assert_eq!(rig.turn(), Some(rules[0]));

        // Replacing again updates that very addition: still one remove and one add.
        rig.type_angle("-150");
        rig.press("Replace dimension");
        let second = rig.edits();
        assert_eq!(second.remove, vec![rules[0]]);
        assert_eq!(second.add, vec![angle_add(a0, b0, -150.)]);
        assert_eq!(rig.history(), (2, 0), "one step more, not a second pair");
        rig.press("Undo");
        assert_eq!(rig.edits(), first);
        rig.press("Redo");
        assert_eq!(rig.edits(), second);
        // The angle that is already pending changes nothing, and spends no step.
        rig.press("Replace dimension");
        assert_eq!((rig.edits(), rig.history()), (second.clone(), (2, 0)));

        // The exactly saved angle takes back only this replacement; the saved rule keeps its UUID
        // because it is no longer removed. An empty draft offers nothing to Apply or Save.
        rig.type_angle("-70");
        rig.press("Replace dimension");
        assert_eq!(rig.edits(), SketchConstraintEdits::default());
        assert_eq!(rig.history(), (3, 0));
        let out = rig.frame(vec![]);
        assert!(has_label(&out, "A>B -70°"));
        assert!(!painted(&out, "Apply constraints") && !painted(&out, "Save constraints copy…"));
        rig.press("Undo");
        assert_eq!(rig.edits(), second);
        rig.press("Redo");
        assert_eq!(rig.edits(), SketchConstraintEdits::default());

        // Other pending changes survive a replacement and its return: a Horizontal on Line 3, the
        // replacement of R0 and of R1, whose first vector is the REVERSED Line 2.
        rig.press("Segment 3");
        rig.press("Add Horizontal");
        let horizontal = AddSketchConstraint::Line(AddLineConstraint::Line {
            curve: ids[2],
            kind: LineConstraintKind::Horizontal,
        });
        rig.choose("A>B -70°");
        rig.type_angle("110");
        rig.press("Replace dimension");
        rig.choose("A>B 110°");
        assert_eq!(rig.turn(), Some(rules[1]));
        rig.type_angle("70");
        rig.press("Replace dimension");
        let both = rig.edits();
        assert_eq!(both.remove, vec![rules[0], rules[1]]);
        assert_eq!(
            both.add,
            vec![horizontal, angle_add(a0, b0, 110.), angle_add(a1, b1, 70.)],
            "R1's replacement keeps Line 2 run from its End"
        );
        let out = rig.frame(vec![]);
        assert!(has_label(&out, "A>B -70 -> 110°") && has_label(&out, "A>B 110 -> 70°"));
        // Returning R0 takes back that one pair and nothing else.
        rig.choose("A>B -70 -> 110°");
        rig.type_angle("-70");
        rig.press("Replace dimension");
        let one = rig.edits();
        assert_eq!(one.remove, vec![rules[1]]);
        assert_eq!(one.add, vec![horizontal, angle_add(a1, b1, 70.)]);
        let out = rig.frame(vec![]);
        assert!(has_label(&out, "A>B -70°") && has_label(&out, "A>B 110 -> 70°"));

        // +180 is the stored opposite: -180 typed IS the saved number (nothing to take back and no
        // step), 0 replaces it, and 180 takes that back.
        rig.choose("A>B 180°");
        assert_eq!(rig.turn(), Some(rules[2]));
        let steps = rig.history();
        rig.type_angle("-180");
        rig.press("Replace dimension");
        assert_eq!(
            (rig.edits(), rig.history()),
            (one.clone(), steps),
            "-180 is +180"
        );
        rig.type_angle("0");
        rig.press("Replace dimension");
        let zero = rig.edits();
        assert_eq!(zero.remove, vec![rules[1], rules[2]]);
        assert_eq!(zero.add.last(), Some(&angle_add(a2, b2, 0.)));
        assert!(has_label(&rig.frame(vec![]), "A>B 180 -> 0°"));
        rig.type_angle("180");
        rig.press("Replace dimension");
        assert_eq!(rig.edits(), one);
        assert!(has_label(&rig.frame(vec![]), "A>B 180°"));
        // A request is never made by any of this.
        assert!(rig.e.take_request().is_none() && rig.e.take_apply_request().is_none());
    }
}

#[test]
fn replace_dimension_is_the_remove_and_add_of_the_form_and_never_a_tolerance() {
    let (_root, path, source) = angles_document();
    let open = || Rig::over(None, &path, &source, 1., enlarged);
    let mut by_label = open();
    let ids = by_label.ids.clone();
    let rules = angle_rules(&by_label.stored);
    let [_, (a1, b1, _), _] = saved_angles(&ids);
    by_label.choose("A>B 110°");
    by_label.type_angle("70");
    by_label.press("Replace dimension");

    // The same request by the form's own road: the Remove tick of the row, the pair, the way each
    // vector runs (Line 2 turned round), the degrees box and Add directed angle.
    let mut by_hand = open();
    by_hand.press("Segment 2");
    by_hand.press("Pair Line A");
    by_hand.press("A: start>end");
    by_hand.press("Segment 3");
    by_hand.press("Pair Line B");
    click_remove_on_row(&by_hand.ctx, &mut by_hand.e, "Directed angle 110°");
    enter_field(&by_hand.ctx, &mut by_hand.e, "A>B (°):", "70", false);
    by_hand.press("Add directed angle");
    assert_eq!(by_label.edits(), by_hand.edits());
    assert_eq!(by_label.history(), (1, 0));
    assert_eq!(
        by_hand.history(),
        (2, 0),
        "two steps by hand, one by the label"
    );
    assert_eq!(by_label.edits().remove, vec![rules[1]]);
    assert_eq!(by_label.edits().add, vec![angle_add(a1, b1, 70.)]);

    // The saved angle is the exact angle, the validated `DirectedAngle`: never a tolerance on
    // degrees. Degrees map many to one onto radians, so the next representable number of degrees
    // may BE the saved angle (and then nothing is pending); the first number that is a different
    // angle is a replacement.
    let saved = DirectedAngle::from_degrees(110.).expect("110");
    let neighbour = f64::from_bits(110f64.to_bits() + 1);
    by_label.type_angle(&format!("{neighbour}"));
    by_label.press("Replace dimension");
    assert_eq!(
        by_label.edits() == SketchConstraintEdits::default(),
        DirectedAngle::from_degrees(neighbour).expect("in range") == saved,
        "a number of degrees is the saved angle exactly when it IS the saved angle"
    );
    let mut next = 110f64;
    let one_more = loop {
        next = f64::from_bits(next.to_bits() + 1);
        if DirectedAngle::from_degrees(next).expect("in range") != saved {
            break next;
        }
        assert!(next < 110.001, "no different angle near 110");
    };
    by_label.type_angle(&format!("{one_more}"));
    by_label.press("Replace dimension");
    assert_eq!(by_label.edits().add, vec![angle_add(a1, b1, one_more)]);
    assert_eq!(by_label.edits().remove, vec![rules[1]]);
    for spelling in ["110.0", " 110 ", "1.1e2", "110.000", "+110"] {
        by_label.type_angle(spelling);
        by_label.press("Replace dimension");
        assert_eq!(
            by_label.edits(),
            SketchConstraintEdits::default(),
            "{spelling:?} is 110"
        );
        by_label.type_angle("70");
        by_label.press("Replace dimension");
        assert_eq!(by_label.edits().remove, vec![rules[1]]);
    }
}

// ---- refusals and what survives them ---------------------------------------------------

#[test]
fn a_refused_angle_keeps_the_typed_text_the_selection_and_the_history_exactly() {
    let mut rig = Rig::angles(1.);
    let rules = angle_rules(&rig.stored);
    rig.choose("A>B -70°");
    let before = rig.kept();
    for text in [
        "",
        " ",
        "-",
        "abc",
        "NaN",
        "inf",
        "-inf",
        "1e999",
        "181",
        "-181",
        "180.000001",
        "-180.000001",
        "270",
        "360",
        "30,5",
        "−30",
        "3 0",
        "5e-324",
        "0x10",
        "--5",
    ] {
        rig.type_angle(text);
        rig.press("Replace dimension");
        assert!(rig.refusal().is_some(), "{text:?} was taken");
        assert_eq!(rig.degrees_typed(), text, "the text is kept exactly");
        assert_eq!(rig.turn(), Some(rules[0]), "{text:?}");
        let mut now = rig.kept();
        // The text itself and the line that says why are the inputs that differ from before.
        now.0 = before.0.clone();
        assert_eq!(
            now, before,
            "{text:?} changed the history or made a request"
        );
        assert_eq!(rig.edits(), SketchConstraintEdits::default());
    }
    // A valid number takes the refusal away; the refusal never outlives the next valid action.
    rig.type_angle("110");
    rig.press("Replace dimension");
    assert_eq!(rig.refusal(), None);
    rig.press("Undo");
    assert_eq!(rig.edits(), SketchConstraintEdits::default());

    // A draft the validator refuses stays refused whole: ticking Remove on R0 frees the pair's slot,
    // a pending Parallel on the pair takes it, and a replacement of R0 is then an answer too many.
    click_remove_on_row(&rig.ctx, &mut rig.e, "Directed angle -70°");
    rig.press("Segment 1");
    rig.press("Pair Line A");
    rig.press("Segment 2");
    rig.press("Pair Line B");
    rig.press("Add Parallel");
    assert_eq!(rig.refusal(), None);
    rig.choose("A>B -70° -> removed");
    assert_eq!(rig.turn(), Some(rules[0]));
    let held = rig.kept();
    let edits = rig.edits();
    assert_eq!(edits.remove, vec![rules[0]]);
    assert_eq!(edits.add.len(), 1);
    rig.type_angle("55");
    rig.press("Replace dimension");
    let refusal = rig.refusal().expect("the validator's refusal");
    assert!(
        refusal.contains("directed angle") || refusal.contains("Parallel"),
        "{refusal}"
    );
    let mut now = rig.kept();
    now.0 = held.0.clone();
    assert_eq!(now, held, "a refused draft was changed");
    assert_eq!((rig.degrees_typed().as_str(), rig.edits()), ("55", edits));
    assert!(rig.e.take_request().is_none() && rig.e.take_apply_request().is_none());
}

#[test]
fn a_cancelled_save_or_a_stale_reply_keeps_the_text_the_selection_and_the_prepared_angle() {
    let (_root, path, source) = angles_document();
    let mut rig = Rig::over(None, &path, &source, 1., enlarged);
    let ids = rig.ids.clone();
    let rules = angle_rules(&rig.stored);
    let [(a0, b0, _), ..] = saved_angles(&ids);
    rig.choose("A>B -70°");
    rig.type_angle("110");
    rig.press("Replace dimension");
    rig.type_angle("58.25");
    rig.press("Save constraints copy…");
    let asked = rig.e.take_request().expect("the Save asked for");
    assert_eq!(asked.edits.remove, vec![rules[0]]);
    assert_eq!(asked.edits.add, vec![angle_add(a0, b0, 110.)]);
    let kept = (rig.kept(), rig.turn(), rig.degrees_typed());
    let mut state = crate::edits::Edits::default();
    let (tx, rx) = std::sync::mpsc::channel();
    let generation = state
        .start_constraints(asked, move |_, g, _| {
            std::thread::spawn(move || tx.send(g).expect("reply"))
        })
        .expect("start");
    // A reply of another generation, and the worker's refusal or a cancelled Save: the draft, the
    // text, the selection and the label of the prepared replacement are what they were.
    assert!(
        finish_edit(
            &mut rig.e,
            &mut state,
            generation + 1,
            Err(ferritecad_types::CadError::input("stale"))
        )
        .is_none()
    );
    assert_eq!((rig.kept(), rig.turn(), rig.degrees_typed()), kept);
    let g = rx.recv().expect("done");
    assert!(
        finish_edit(
            &mut rig.e,
            &mut state,
            g,
            Err(ferritecad_types::CadError::constraint("solver refused"))
        )
        .is_none()
    );
    assert_eq!((rig.kept(), rig.turn(), rig.degrees_typed()), kept);
    rig.settle();
    let out = rig.frame(vec![]);
    assert!(has_label(&out, "A>B -70 -> 110°") && painted(&out, "Replace dimension"));
    // And the form takes the next action as if nothing had happened.
    rig.type_angle("80");
    rig.press("Replace dimension");
    assert_eq!(rig.edits().add, vec![angle_add(a0, b0, 80.)]);
}

// ---- view, hit area, blocked input ----------------------------------------------------

/// A drawing of four short Lines at the corners of a wide box: every pair's label stands far from
/// every other label and from the labels of the lengths, so a hair outside one box is outside all.
fn spread(stored: &Sketch) -> Drawing {
    let at = |x: f64, y: f64| ferritecad_document::Point2 { x, y };
    Drawing::of(
        stored,
        stored
            .curves
            .iter()
            .zip([
                (at(0., 0.), at(10., 0.)),
                (at(90., 0.), at(100., 0.)),
                (at(90., 30.), at(100., 30.)),
                (at(0., 30.), at(10., 30.)),
            ])
            .map(|(curve, (start, end))| (curve.id, SketchGeometry::Line { start, end })),
    )
}

#[test]
fn labels_follow_zoom_pan_and_fit_and_are_hit_exactly_where_they_are_drawn() {
    for pixels_per_point in [1., 2.] {
        let (_root, path, source) = angles_document();
        let mut rig = Rig::over(None, &path, &source, pixels_per_point, spread);
        let ids = rig.ids.clone();
        let rules = angle_rules(&rig.stored);
        rig.target(Target::Dimension);
        rig.e.pick_for_tests(ids[3]);
        let canvas = rig.rect();
        let opening = rig.canvas().view();
        let first = label(&rig.frame(vec![]), "A>B 180°");
        let r = first.rect;
        assert!(band(canvas).contains_rect(r), "{first:?}");
        let middle_of_middles = |rig: &Rig| rig.mid(0) + (rig.mid(2) - rig.mid(0)) * 0.5;
        assert!(near(r.center(), middle_of_middles(&rig)));

        // The hit area is the drawn box: just inside every corner takes the label, just outside
        // takes nothing.
        let nudge = |x: f32, y: f32| egui::vec2(x, y);
        for at in [
            r.min + nudge(0.5, 0.5),
            r.max - nudge(0.5, 0.5),
            r.left_bottom() + nudge(0.5, -0.5),
            r.right_top() + nudge(-0.5, 0.5),
            r.center(),
        ] {
            rig.e.pick_for_tests(ids[3]);
            rig.click(at);
            assert_eq!(
                rig.turn(),
                Some(rules[2]),
                "{pixels_per_point}x inside at {at:?} of {r:?} on {canvas:?}"
            );
            assert_eq!(rig.picks().0, None);
        }
        for at in [
            r.min - nudge(0.5, 0.),
            r.min - nudge(0., 0.5),
            r.max + nudge(0.5, 0.),
            r.max + nudge(0., 0.5),
            r.left_bottom() + nudge(-0.5, 0.5),
            r.right_top() + nudge(0.5, -0.5),
        ] {
            rig.e.pick_for_tests(ids[3]);
            rig.click(at);
            assert_eq!(
                (rig.turn(), rig.picks().0),
                (None, Some(ids[3])),
                "{pixels_per_point}x outside at {at:?}"
            );
        }

        // Zoom out about a point above the plate: both Lines move, and the label with them, to the
        // middle of their new middles, still whole inside the band; the old place takes nothing.
        let before = middle_of_middles(&rig);
        let about = egui::pos2(before.x, canvas.top() + 30.);
        rig.frame(vec![moved(about)]);
        rig.frame(vec![moved(about), wheel(-40.)]);
        let after = middle_of_middles(&rig);
        assert_ne!(before, after);
        let zoomed = label(&rig.frame(vec![]), "A>B 180°");
        assert!(band(canvas).contains_rect(zoomed.rect), "{zoomed:?}");
        assert!(
            near(zoomed.rect.center(), after),
            "the label did not move with its Lines"
        );
        rig.e.pick_for_tests(ids[3]);
        rig.click(zoomed.rect.center());
        assert_eq!(rig.turn(), Some(rules[2]), "the box where it is drawn now");

        // Fit gives the opening place back exactly; a pan moves the label with the Lines until the
        // band holds it.
        rig.press("Fit");
        assert_eq!(label(&rig.frame(vec![]), "A>B 180°").rect, first.rect);
        let grab = canvas.center() + egui::vec2(0., 30.);
        rig.frame(vec![moved(grab), middle(grab, true)]);
        rig.frame(vec![moved(grab + egui::vec2(-20., 0.))]);
        rig.frame(vec![middle(grab + egui::vec2(-20., 0.), false)]);
        let panned = label(&rig.frame(vec![]), "A>B 180°");
        assert!(
            near(panned.rect.min, first.rect.min + egui::vec2(-20., 0.)),
            "{panned:?}"
        );
        assert_eq!(panned.rect.min.y, first.rect.min.y);

        // Far in, the middle of the middles is off the canvas: no label, no hit, and the list
        // still reaches the angle. Fit returns the label where it was.
        rig.press("Fit");
        for _ in 0..6 {
            rig.frame(vec![
                moved(canvas.left_top() + egui::vec2(4., 4.)),
                wheel(120.),
            ]);
        }
        assert!(
            !has_label(&rig.frame(vec![]), "A>B 180°"),
            "{pixels_per_point}x"
        );
        rig.e.pick_for_tests(ids[3]);
        rig.click(first.rect.center());
        assert_eq!(
            (rig.turn(), rig.picks().0),
            (None, Some(ids[3])),
            "nothing is there"
        );
        rig.press_row(&format!("Directed angle 180° · {}", rules[2]));
        assert_eq!(rig.turn(), Some(rules[2]), "the list reaches it");
        rig.press("Fit");
        assert_eq!(rig.canvas().view(), opening);
        assert_eq!(label(&rig.frame(vec![]), "A>B 180°").rect, first.rect);
        assert!(rig.e.take_request().is_none());
    }
}

#[test]
fn an_angle_label_press_dies_with_its_surface_and_busy_queued_covered_or_unfocused_forms_take_nothing()
 {
    for pixels_per_point in [1., 2.] {
        let ready = || {
            let mut rig = Rig::angles(pixels_per_point);
            let ids = rig.ids.clone();
            rig.target(Target::Dimension);
            rig.e.pick_for_tests(ids[3]);
            rig
        };
        // A press begun on an available label is no permission for a later release, however the
        // surface came back before it: focus lost, another layer over the canvas, and that layer
        // gone again.
        for interruption in ["focus", "covered", "uncovered"] {
            let mut held = ready();
            let before = held.angle_facts();
            let at = held.at_label("A>B -70°");
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
                held.angle_facts(),
                before,
                "{pixels_per_point}x: a release after {interruption} took a label"
            );
            assert!(!held.canvas().pressed());
        }

        // A release just outside the canvas, within a click's distance of the press, takes nothing:
        // pan the Lines until the label hugs the canvas's left edge.
        let mut edge = ready();
        let canvas = edge.rect();
        let grab = canvas.center() + egui::vec2(0., 30.);
        // The middle of the middles goes to ten points from the canvas's left edge; the box is held
        // in the band, three points from that edge.
        let drag = egui::vec2(canvas.left() + 10. - edge.at_label("A>B 110°").x, 0.);
        edge.frame(vec![moved(grab), middle(grab, true)]);
        edge.frame(vec![moved(grab + drag)]);
        edge.frame(vec![middle(grab + drag, false)]);
        let hugging = label(&edge.frame(vec![]), "A>B 110°");
        assert!(
            (hugging.rect.left() - (canvas.left() + 3.)).abs() < 0.01,
            "{hugging:?} on {canvas:?}"
        );
        let before = edge.angle_facts();
        let at = hugging.rect.left_center() + egui::vec2(0.2, 0.);
        let outside = egui::pos2(canvas.left() - 0.8, at.y);
        assert!(!canvas.contains(outside) && at.distance(outside) <= 6.);
        edge.frame(vec![moved(at), primary(at, true)]);
        edge.frame(vec![moved(outside), primary(outside, false)]);
        assert_eq!(
            edge.angle_facts(),
            before,
            "{pixels_per_point}x: release outside"
        );
        assert!(!edge.canvas().pressed());

        // Escape, a lost focus and a vanished pointer end a press without a trace.
        for ending in ["escape", "focus", "gone"] {
            let mut rig = ready();
            let before = rig.angle_facts();
            let at = rig.at_label("A>B -70°");
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
            assert_eq!(rig.angle_facts(), before, "{pixels_per_point}x: {ending}");
        }

        // A shift press on a label is a pan's: it never takes the label, moving or not.
        let mut rig = ready();
        let before = rig.angle_facts();
        let at = rig.at_label("A>B -70°");
        rig.frame(vec![moved(at), shift_primary(at, true)]);
        rig.frame(vec![shift_primary(at, false)]);
        assert_eq!(
            rig.angle_facts(),
            before,
            "{pixels_per_point}x: a shift click took a label"
        );
    }

    // Busy, unfocused, covered, outside and queued forms: the label, the field and the action take
    // nothing, and take it again as soon as nothing blocks them.
    let mut rig = Rig::angles(1.);
    let ids = rig.ids.clone();
    let rules = angle_rules(&rig.stored);
    rig.choose("A>B -70°");
    rig.type_angle("70");
    let base = rig.angle_facts();
    let (aim, other) = (rig.at_label("A>B -70°"), rig.at_label("A>B 110°"));
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
    assert_eq!(rig.angle_facts(), base, "busy");
    attempt(&mut rig, false, false, None);
    assert_eq!(rig.angle_facts(), base, "unfocused");
    let canvas = rig.rect();
    for _ in 0..3 {
        rig.frame_as(vec![], false, true, Some(canvas));
    }
    attempt(&mut rig, false, true, Some(canvas));
    assert_eq!(rig.angle_facts(), base, "covered");
    for _ in 0..3 {
        rig.frame(vec![]);
    }
    // The field takes no text and the action no press while a job runs, though the field holds the
    // focus it had; the typed text is exactly what it was.
    enter_field(&rig.ctx, &mut rig.e, "New angle", "99", true);
    click_running(&rig.ctx, &mut rig.e, "Replace dimension", true);
    assert_eq!(
        (rig.degrees_typed().as_str(), rig.edits()),
        ("70", SketchConstraintEdits::default())
    );
    // Prepared once, a request of the form waits to be taken: nothing on the canvas, no text in
    // the field, no action begins.
    rig.type_angle("70");
    rig.press("Replace dimension");
    let prepared = rig.edits();
    assert_eq!(prepared.remove, vec![rules[0]]);
    rig.press("Save constraints copy…");
    assert!(rig.e.pending.is_some());
    let queued = rig.angle_facts();
    attempt(&mut rig, false, true, None);
    enter_field(&rig.ctx, &mut rig.e, "New angle", "99", false);
    rig.press("Replace dimension");
    press_exact_target(&mut rig, "Dimension");
    rig.press("Fit");
    assert_eq!(rig.angle_facts(), queued, "queued");
    assert_eq!(rig.degrees_typed(), "70");
    assert_eq!(
        rig.e.take_request().expect("the waiting request").edits,
        prepared
    );
    // Nothing blocks any more: the other label is taken and its dimension replaced.
    rig.click(other);
    assert_eq!(rig.turn(), Some(rules[1]));
    rig.settle();
    rig.type_angle("33");
    rig.press("Replace dimension");
    assert_eq!(rig.edits().remove, vec![rules[0], rules[1]]);
    let _ = ids;
}

/// Press the button whose words are exactly `words` (the target buttons, not the rows).
fn press_exact_target(rig: &mut Rig, words: &str) {
    super::dimension::press_exact(rig, words);
}

// ---- overlaps, the 768-point screen, no drawing, other forms ----------------------------

/// A drawing in which every Line runs through one point: the middles of all the Lines coincide, so
/// every pair's label stands on that point and the Lines' own labels stand beside it.
fn crossing(stored: &Sketch) -> Drawing {
    let centre = [20., 15.];
    Drawing::of(
        stored,
        stored.curves.iter().enumerate().map(|(i, curve)| {
            let turn = i as f64 * std::f64::consts::FRAC_PI_4;
            let (dx, dy) = (15. * turn.cos(), 15. * turn.sin());
            (
                curve.id,
                SketchGeometry::Line {
                    start: ferritecad_document::Point2 {
                        x: centre[0] - dx,
                        y: centre[1] - dy,
                    },
                    end: ferritecad_document::Point2 {
                        x: centre[0] + dx,
                        y: centre[1] + dy,
                    },
                },
            )
        }),
    )
}

#[test]
fn overlapping_labels_are_reached_in_saved_order_and_the_list_reaches_the_one_underneath() {
    let (_root, path, source) = angles_document();
    let mut rig = Rig::over(None, &path, &source, 1., crossing);
    let ids = rig.ids.clone();
    let rules = angle_rules(&rig.stored);
    rig.target(Target::Dimension);
    let out = rig.frame(vec![]);
    let (r0, r1, r2) = (
        label(&out, "A>B -70°").rect,
        label(&out, "A>B 110°").rect,
        label(&out, "A>B 180°").rect,
    );
    // The three pairs stand on one point whatever the Lines are, and the length of Line 1 stands
    // above it.
    let centre = rig.mid(0);
    for r in [r0, r1, r2] {
        assert!(near(r.center(), centre), "{r:?} {centre:?}");
    }
    let length = label(&out, "60 mm").rect;
    let apart = centre + egui::vec2(0., 4.);
    let shared = egui::pos2(centre.x, (length.bottom() + centre.y - 7.5) / 2.);
    assert!(
        r0.contains(apart) && r1.contains(apart) && r2.contains(apart) && !length.contains(apart),
        "{apart:?} must be on the three angles and not on the length"
    );
    assert!(length.contains(shared) && r0.contains(shared), "{shared:?}");
    // The first angle in the order the rules are stored is the one reached, whatever the order of
    // their UUIDs says (R2 < R0 < R1: ascending would take R2, descending R1).
    rig.e.pick_for_tests(ids[3]);
    rig.click(apart);
    assert_eq!((rig.turn(), rig.picks().0), (Some(rules[0]), None));
    // A label of a curve is drawn above the labels of pairs and is reached where they overlap.
    rig.e.pick_for_tests(ids[3]);
    rig.click(shared);
    assert_eq!((rig.turn(), rig.picks().0), (None, Some(ids[0])));
    // The persisted rows reach each one of the angles underneath.
    for (i, words) in [(1, "110"), (2, "180"), (0, "-70")] {
        rig.press_row(&format!("Directed angle {words}° · {}", rules[i]));
        rig.settle();
        assert_eq!(
            (rig.turn(), rig.picks().0),
            (Some(rules[i]), None),
            "row {i}"
        );
        assert!(painted(
            &rig.frame(vec![]),
            &format!("Selected dimension {} · A>B {words}°", short(rules[i]))
        ));
    }
    // What the pointer shows is the top label's: hovering the shared place marks R0's two Lines.
    let out = rig.frame(vec![moved(apart)]);
    let yellow = Rig::strokes(&out, HOVER_COLOUR);
    for i in [0, 1] {
        let ends = rig.ends(i);
        assert!(
            yellow
                .iter()
                .any(|s| near(s[0], ends[0]) && near(s[1], ends[1])),
            "Line {}",
            i + 1
        );
    }
    assert!(
        !yellow
            .iter()
            .any(|s| near(s[0], rig.ends(2)[0]) && near(s[1], rig.ends(2)[1])),
        "only the top label's Lines"
    );
}

/// The 64-gon of the §31K gate with two saved angles between neighbouring Lines.
fn polygon_with_angles() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let (root, path, source) = super::canvas::polygon_fixture(64, true);
    let choice = &source.constraint_sketches[0];
    let ids: Vec<_> = choice
        .stored
        .as_ref()
        .expect("stored")
        .curves
        .iter()
        .map(|c| c.id)
        .collect();
    let mut document = Document::open(&path).expect("document");
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        choice.sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                angle_add(
                    vector(&ids, 0, LineEndpoint::Start),
                    vector(&ids, 1, LineEndpoint::Start),
                    5.625,
                ),
                angle_add(
                    vector(&ids, 1, LineEndpoint::End),
                    vector(&ids, 2, LineEndpoint::Start),
                    -174.375,
                ),
            ],
        },
    )
    .expect("prepare");
    document
        .write_sketch_constraints(&prepared)
        .expect("angles");
    let source = ExtrudeEditSource::read(&document).expect("catalogue");
    document.close().expect("close");
    (root, path, source)
}

#[test]
fn the_angle_labels_the_editor_and_the_actions_stay_reachable_at_768_points_in_the_worst_forms() {
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
        "New angle",
        "Replace dimension",
    ];
    let long_refusal = "enter the angle in degrees, from -180 to 180 — and so on, a refusal worded \
                        at such length that it must wrap inside the form instead of leaving it";
    let (_a, plate_path, plate) = angles_document();
    let (_b, polygon_path, polygon) = polygon_with_angles();
    let mut report = Vec::new();
    for (name, path, source, words) in [
        (
            "plate with two lengths and three angles",
            plate_path,
            plate,
            "A>B 110°",
        ),
        (
            "64-gon with every closure link, a length and two angles",
            polygon_path,
            polygon,
            "A>B 5.625°",
        ),
    ] {
        let stored = source.constraint_sketches[0]
            .stored
            .clone()
            .expect("stored");
        let mut bottoms = Vec::new();
        for (unsaved, extras) in [(false, false), (true, true)] {
            let mut rig = Rig::over(None, &path, &source, 1., enlarged);
            rig.e.set_session(true, true, unsaved);
            // The worst of the old gate: thirty-two pending Horizontals on a 64-gon.
            if extras && stored.curves.len() >= 32 {
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
            rig.choose(words);
            // A replacement is prepared, so that Apply and Save are offered.
            rig.type_angle("42.5");
            rig.press("Replace dimension");
            assert!(rig.edits().remove.len() == 1, "{name}: no replacement");
            if extras {
                // A long text in the field and a long refusal under it. The fully loaded 64-gon is as
                // the 31K and 31L gates left it: no room for a refusal line even without the canvas.
                rig.type_angle("1234567890123456789012345678901");
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
            eprintln!("FORM_WIDTH_31M {name}: widest text ends at {right:.1}");
            bottoms.push(bottom);
        }
        report.push(format!(
            "{name}: Save bottom {:.1} with an angle selected, {:.1} with a long text, a prepared replacement, a refusal and the unsaved note",
            bottoms[0], bottoms[1]
        ));
    }
    for line in &report {
        eprintln!("FORM_BUDGET_31M {line}");
    }
}

#[test]
fn without_a_drawing_the_angle_rows_still_select_and_replace_and_other_profiles_have_no_angle() {
    let (_root, path, source) = angles_document();
    let rules = angle_rules(&dimension_stored(&source));
    let row = format!("Directed angle 110° · {}", rules[1]);
    // The form with no accepted drawing: no canvas, no labels, no Dimension target; the list of
    // persisted rules selects and replaces as it does with one.
    let mut with = Rig::over(None, &path, &source, 1., enlarged);
    with.choose("A>B 110°");
    with.type_angle("70");
    with.press("Replace dimension");
    let mut without = Rig::over(None, &path, &source, 1., |_| {
        Drawing::Unavailable(crate::sketch::accepted::Unavailable::NotHanded)
    });
    let out = without.frame(vec![]);
    assert!(labels(&out).is_empty() && !painted(&out, "Dimension") && !painted(&out, "Fit"));
    without.press_row(&row);
    without.settle();
    assert_eq!((without.turn(), without.picks().0), (Some(rules[1]), None));
    without.type_angle("70");
    without.press("Replace dimension");
    assert_eq!(without.edits(), with.edits());
    assert_eq!(without.edits().remove, vec![rules[1]]);
    // A form held on another version loses its drawing and keeps its selection, its rows and text.
    let mut held = Rig::over(None, &path, &source, 1., enlarged);
    held.choose("A>B 180°");
    assert!(!labels(&held.frame(vec![])).is_empty());
    held.type_angle("0");
    held.e.retire_drawing();
    held.settle();
    let out = held.frame(vec![]);
    assert!(labels(&out).is_empty() && !painted(&out, "Dimension"));
    assert_eq!(
        held.turn(),
        Some(rules[2]),
        "the selection of a held form is kept"
    );
    assert_eq!(held.degrees_typed(), "0");
    scroll_to_row(&held.ctx, &mut held.e, "Replace dimension");
    held.press("Replace dimension");
    assert_eq!(held.edits().remove, vec![rules[2]]);
    assert_eq!(held.refusal(), None);

    // A Circle, an annulus and a mixed profile have no angle: no row of theirs is selectable.
    let (_ring, ring_path, ring, _) = super::diameter::round_fixture(&[10., 4.]);
    let rig = Rig::over(None, &ring_path, &ring, 1., |_| {
        Drawing::Unavailable(crate::sketch::accepted::Unavailable::Profile)
    });
    assert!(rig.e.draft_turns().expect("draft").0.is_empty());
    let (_round, round_path, round, _, _) = persisted_circle_for_angles();
    let rig = Rig::over(None, &round_path, &round, 1., moved_circle_for_angles);
    assert!(rig.e.draft_turns().expect("draft").0.is_empty());
}

#[test]
fn two_forms_over_one_document_keep_their_own_angle_selection_text_and_pending_replacement() {
    // Copies of a document share their curve and rule UUIDs; the forms must not.
    let (_root, path, source) = angles_document();
    let rules = angle_rules(&dimension_stored(&source));
    let mut a = Rig::over(None, &path, &source, 1., enlarged);
    let mut b = Rig::over(None, &path, &source, 1., enlarged);
    a.choose("A>B -70°");
    a.type_angle("110");
    a.press("Replace dimension");
    let in_a = (a.turn(), a.degrees_typed(), a.edits());
    assert_eq!(in_a.0, Some(rules[0]));
    // B has not been touched by any of it.
    assert_eq!(
        (b.turn(), b.degrees_typed(), b.edits()),
        (None, String::new(), SketchConstraintEdits::default())
    );
    assert!(
        has_label(&b.frame(vec![]), "A>B -70°") && !has_label(&b.frame(vec![]), "A>B -70 -> 110°")
    );
    b.choose("A>B 180°");
    b.type_angle("0");
    b.press("Replace dimension");
    assert_eq!(b.edits().remove, vec![rules[2]]);
    // A is what it was; its text box holds A's text and B's holds B's.
    assert_eq!((a.turn(), a.degrees_typed(), a.edits()), in_a);
    assert_eq!(b.degrees_typed(), "0");
}

/// The cylinder of §31L with a Radius, only to show that a Circle has no angle.
fn persisted_circle_for_angles() -> (
    tempfile::TempDir,
    PathBuf,
    ExtrudeEditSource,
    StableEntityId,
    StableEntityId,
) {
    let (root, path, source, rule) = super::dimension::persisted_circle(6., 10.);
    let circle = source.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .curves[0]
        .id;
    (root, path, source, rule, circle)
}

fn moved_circle_for_angles(stored: &Sketch) -> Drawing {
    super::canvas::moved_circle(stored)
}

// ---- the number: near zero, near half a turn, and exact editable degree spellings ------

/// The plate with five saved angles whose numbers sit where a display could lie: a hair from zero on
/// both sides, a hair from half a turn on both sides, and 7.9e-9° — a value that has more than nine
/// places and whose inverse conversion used to miss the saved radians by one ULP. Pairs: (1,2), (2,3), (3,4), (4,1), (1,3).
fn odd_angles_document() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let (root, path, source) = fixture();
    let mut document = Document::open(&path).expect("document");
    let choice = &source.constraint_sketches[0];
    let ids: Vec<_> = choice
        .stored
        .as_ref()
        .expect("stored")
        .curves
        .iter()
        .map(|c| c.id)
        .collect();
    let s = |i: usize| vector(&ids, i, LineEndpoint::Start);
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        choice.sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                angle_add(s(0), s(1), 1e-10),
                angle_add(s(1), s(2), -1e-10),
                angle_add(s(2), s(3), 179.999_999_9),
                angle_add(s(3), s(0), -179.999_999_9),
                angle_add(s(0), s(2), 7.9e-9),
            ],
        },
    )
    .expect("prepare");
    document
        .write_sketch_constraints(&prepared)
        .expect("angles");
    let source = ExtrudeEditSource::read(&document).expect("catalogue");
    document.close().expect("close");
    (root, path, source)
}

#[test]
fn an_angle_typed_as_it_is_displayed_is_the_saved_angle_exactly_and_the_one_that_is_not_says_so() {
    let (_root, path, source) = odd_angles_document();
    let mut rig = Rig::over(None, &path, &source, 1., enlarged);
    let rules = angle_rules(&rig.stored);
    let value = |i: usize| rig.e.draft_turns().expect("draft").0[i].angle;
    let angles: Vec<_> = (0..5).map(value).collect();
    // The label never reads as another number: never 0 for 1e-10°, never 180 for 179.9999999°.
    let odd = DirectedAngle::from_degrees(7.9e-9).expect("in range");
    assert_eq!(angles[4], odd);
    let words = [
        "A>B 1e-10°".to_owned(),
        "A>B -1e-10°".to_owned(),
        "A>B 179.9999999°".to_owned(),
        "A>B -179.9999999°".to_owned(),
        format!("A>B {:e}°", crate::sketch::dimension::editable_degrees(odd)),
    ];
    let out = rig.frame(vec![]);
    for w in &words {
        assert!(
            has_label(&out, w),
            "{w}: {:?}",
            labels(&out).iter().map(|l| &l.text).collect::<Vec<_>>()
        );
    }
    assert!(!has_label(&out, "A>B 0°") && !has_label(&out, "A>B 180°"));
    // Typing the number a row or a sentence displays: for the probes that is the saved angle, to the
    // bit, so the replacement is taken back (nothing is pending and no step is spent); one angle
    // further is a replacement; no tolerance anywhere.
    for (i, text) in words.iter().enumerate() {
        rig.choose(text);
        assert_eq!(rig.turn(), Some(rules[i]));
        let typed = crate::sketch::dimension::editable_degrees(angles[i]);
        assert!(
            !painted_containing(&rig.frame(vec![]), "Typing the saved"),
            "{text}"
        );
        let steps = rig.history();
        rig.type_angle(&format!("{typed}"));
        rig.press("Replace dimension");
        assert_eq!(
            rig.edits(),
            SketchConstraintEdits::default(),
            "{text}: {typed} is the saved angle"
        );
        assert_eq!(rig.history(), steps, "{text}: no step is spent");
        let mut next = typed;
        let different = loop {
            next = f64::from_bits(if next >= 0. {
                next.to_bits() + 1
            } else {
                next.to_bits() - 1
            });
            if DirectedAngle::from_degrees(next).expect("in range") != angles[i] {
                break next;
            }
        };
        rig.type_angle(&format!("{different}"));
        rig.press("Replace dimension");
        assert_eq!(
            rig.edits().remove,
            vec![rules[i]],
            "{text}: {different} is another angle"
        );
        rig.press("Undo");
        assert_eq!(rig.edits(), SketchConstraintEdits::default());
    }
    // The formerly problematic 7.9e-9° also reverts only its own pending replacement:
    // the exact editable spelling passes through the unchanged document converter.
    rig.choose(&words[4]);
    let shown = crate::sketch::dimension::editable_degrees(odd);
    assert!(angles_typeable(&rig, 4));
    rig.type_angle("8e-9");
    rig.press("Replace dimension");
    assert_eq!(rig.edits().remove, vec![rules[4]]);
    rig.type_angle(&shown.to_string());
    rig.press("Replace dimension");
    assert_eq!(rig.edits(), SketchConstraintEdits::default());
}

fn angles_typeable(rig: &Rig, i: usize) -> bool {
    rig.e.draft_turns().expect("draft").0[i].typeable()
}

fn painted_containing(out: &egui::FullOutput, words: &str) -> bool {
    out.shapes
        .iter()
        .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains(words)))
}

// ---- native: the real accepted drawing, the real worker -------------------------------

use super::canvas::{accepted_scene, by_lists, line_numbers, presented, read_stl_of};
use super::dimension::Replaced;

/// A quadrilateral whose Sketch STORES a rough guess (a 12 × 8 mm parallelogram about the origin)
/// and keeps the rules that solve it into a different shape: Line 1's start fixed at the origin,
/// Line 1 horizontal and 50 mm, Line 2 30 mm, Line 3 40 mm, and two saved directed angles — R_a
/// Line 1 start>end to Line 2 start>end, **+100°**, and R_b Line 2 **end>start** (the reversed
/// vector) to Line 3 start>end, **−100°**. Solved: (0,0) (50,0) (44.7906, 29.5442) (4.7906, 29.5442);
/// the stored guess turns ±77.1°, so no saved number is what the guess measures.
pub(crate) fn write_angled_quad(path: &Path) {
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
    let origin = SketchCoordinateMm::new(0.).expect("origin");
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        choice.sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                line(
                    0,
                    LineConstraintKind::Fixed {
                        at: LineEndpoint::Start,
                        x: origin,
                        y: origin,
                    },
                ),
                line(0, LineConstraintKind::Horizontal),
                length_add(ids[0], 50.),
                angle_add(
                    vector(&ids, 0, LineEndpoint::Start),
                    vector(&ids, 1, LineEndpoint::Start),
                    100.,
                ),
                length_add(ids[1], 30.),
                angle_add(
                    vector(&ids, 1, LineEndpoint::End),
                    vector(&ids, 2, LineEndpoint::Start),
                    -100.,
                ),
                length_add(ids[2], 40.),
            ],
        },
    )
    .expect("prepare");
    document.write_sketch_constraints(&prepared).expect("rules");
    document.close().expect("close");
}

fn length_add(curve: StableEntityId, mm: f64) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::Line {
        curve,
        kind: LineConstraintKind::Distance(LineLengthMm::new(mm).expect("a length")),
    })
}

/// The lines of the real presentation as the form's own curves.
fn curves_of(real: &[(StableEntityId, SketchGeometry)]) -> Vec<Curve> {
    real.iter()
        .map(|(id, g)| {
            let (start, end) = line_numbers(g);
            Curve {
                id: *id,
                shape: Shape::Line { start, end },
            }
        })
        .collect()
}

/// The polygon area of the solved Lines, by the shoelace formula on their starts.
fn area_of(curves: &[Curve]) -> f64 {
    let starts: Vec<[f64; 2]> = curves
        .iter()
        .map(|c| match c.shape {
            Shape::Line { start, .. } => start,
            Shape::Circle { .. } => panic!("a Line"),
        })
        .collect();
    (0..starts.len())
        .map(|i| {
            let (p, q) = (starts[i], starts[(i + 1) % starts.len()]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f64>()
        .abs()
        / 2.
}

/// §31M, native: the form for the quadrilateral written by `write_angled_quad`, driven by the real
/// widgets over the real accepted drawing. The labels state the saved +100° and −100° (not the turns
/// of the stored parallelogram), stand between the SOLVED Lines, and the label of −100°, chosen under
/// the Dimension target, marks Lines 2 and 3 with the way each vector runs and replaces that rule by
/// −130° keeping both exact vectors; the request equals the one the form's own Remove, pair,
/// direction and degrees route makes. Returns the form with its Apply asked, as the session gate
/// takes it.
pub(crate) fn native_angle_quad(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    seen: &std::cell::Cell<Option<Replaced>>,
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut rig, loaded) = Rig::native(path, source, sketch);
    let real = presented(&loaded, sketch);
    let (ids, stored) = (rig.ids.clone(), rig.stored.clone());
    let rules = angle_rules(&stored);
    assert_eq!(rules.len(), 2);
    let (r_a, r_b) = (rules[0], rules[1]);
    let (a_l1, b_l2) = (
        vector(&ids, 0, LineEndpoint::Start),
        vector(&ids, 1, LineEndpoint::Start),
    );
    let (a_l2, b_l3) = (
        vector(&ids, 1, LineEndpoint::End),
        vector(&ids, 2, LineEndpoint::Start),
    );
    // The form's drawing IS the rebuild's presentation, and the rules hold there, measured by
    // atan2(cross, dot) of the very vectors the rules name; the stored guess does not.
    let held = rig.solved();
    assert_eq!(held, curves_of(&real), "the drawing is the rebuild's");
    let solved_lengths: Vec<f64> = held
        .iter()
        .map(|c| match c.shape {
            Shape::Line { start, end } => (end[0] - start[0]).hypot(end[1] - start[1]),
            Shape::Circle { .. } => panic!("a Line"),
        })
        .collect();
    assert!(
        (solved_lengths[0] - 50.).abs() < 1e-6
            && (solved_lengths[1] - 30.).abs() < 1e-6
            && (solved_lengths[2] - 40.).abs() < 1e-6,
        "{solved_lengths:?}"
    );
    let (turn_a, turn_b) = (measured(&held, a_l1, b_l2), measured(&held, a_l2, b_l3));
    assert!(
        (turn_a - 100.).abs() < 1e-6 && (turn_b + 100.).abs() < 1e-6,
        "the accepted drawing turns {turn_a} and {turn_b}"
    );
    let guess = Drawing::of(
        &stored,
        stored.curves.iter().map(|c| (c.id, c.geometry.clone())),
    );
    let Drawing::Solved(guess) = guess else {
        panic!("the stored curves draw")
    };
    let (guess_a, guess_b) = (measured(&guess, a_l1, b_l2), measured(&guess, a_l2, b_l3));
    assert!(
        (guess_a - 100.).abs() > 5. && (guess_b + 100.).abs() > 5.,
        "the stored guess turns {guess_a} and {guess_b}: it must not be the solved shape"
    );

    // Two angle labels (and three lengths): the saved targets, standing at the middle of the middles
    // of the SOLVED Lines.
    let out = rig.frame(vec![]);
    let mut words: Vec<_> = labels(&out).into_iter().map(|l| l.text).collect();
    words.sort();
    assert_eq!(
        words,
        ["30 mm", "40 mm", "50 mm", "A>B -100°", "A>B 100°"],
        "no label tells the stored guess"
    );
    for (text, i, j) in [("A>B 100°", 0, 1), ("A>B -100°", 1, 2)] {
        let want = rig.mid(i) + (rig.mid(j) - rig.mid(i)) * 0.5;
        let got = label(&out, text).rect.center();
        assert!(
            got.distance(want) < 1.0
                || !band(rig.rect()).contains_rect(egui::Rect::from_center_size(
                    want,
                    label(&out, text).rect.size()
                )),
            "{text}: {got:?} is not at the middle of the solved middles {want:?}"
        );
    }
    eprintln!(
        "NATIVE_ANGLE_QUAD stored_turns=({guess_a:.3},{guess_b:.3}) solved_turns=({turn_a:.6},{turn_b:.6}) solved_lengths={solved_lengths:?}"
    );

    // Under Object a click on a label's place takes nothing; under Dimension it takes the rule.
    let at = label(&out, "A>B -100°").rect.center();
    rig.click(at);
    assert_eq!((rig.turn(), rig.picks().0), (None, None));
    rig.target(Target::Dimension);
    rig.click(at);
    rig.settle();
    assert_eq!((rig.turn(), rig.picks().0), (Some(r_b), None));
    // Lines 2 and 3 of the SOLVED drawing are marked, the vector on Line 2 runs from its End.
    let out = rig.frame(vec![]);
    let marked = selected_strokes(&out);
    let ends: Vec<_> = (0..4).map(|i| rig.ends(i)).collect();
    for (i, on) in [(0, false), (1, true), (2, true), (3, false)] {
        assert_eq!(
            marked
                .iter()
                .any(|s| near(s[0], ends[i][0]) && near(s[1], ends[i][1])),
            on,
            "Line {}",
            i + 1
        );
    }
    let heads = arrowheads(&out, SELECTED_COLOUR);
    let [s2, e2] = ends[1];
    let [s3, e3] = ends[2];
    assert!(
        heads.iter().any(|h| point_near(h[0], e2 + (s2 - e2) * 0.8)),
        "Line 2 runs end>start"
    );
    assert!(
        heads.iter().any(|h| point_near(h[0], s3 + (e3 - s3) * 0.8)),
        "Line 3 runs start>end"
    );
    assert_eq!(heads.len(), 2);

    // Replace by -130°: one removal of the rule and one addition keeping both exact vectors.
    rig.type_angle("-130");
    rig.press("Replace dimension");
    let prepared = rig.edits();
    assert_eq!(prepared.remove, vec![r_b]);
    assert_eq!(prepared.add, vec![angle_add(a_l2, b_l3, -130.)]);
    // Prepared only: the label says so beside the target, and the drawing is exactly the rebuild's.
    assert!(has_label(&rig.frame(vec![]), "A>B -100 -> -130°"));
    assert_eq!(rig.solved(), held, "a prepared replacement draws nothing");
    assert_eq!(rig.refusal(), None);

    // The same replacement by the form's own road: the Remove tick of the row, the pair, the way
    // each vector runs, the degrees box and Add directed angle.
    let lists = by_lists(path, source, sketch, |ctx, e| {
        click(ctx, e, "Segment 2");
        click(ctx, e, "Pair Line A");
        click(ctx, e, "A: start>end");
        click(ctx, e, "Segment 3");
        click(ctx, e, "Pair Line B");
        click_remove_on_row(ctx, e, "Directed angle -100°");
        enter_field(ctx, e, "A>B (°):", "-130", false);
        click(ctx, e, "Add directed angle");
    });
    assert_eq!(
        prepared, lists.edits,
        "the label and the form's own road asked for different things"
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
    seen.set(Some((r_b, r_a)));
    println!(
        "\nFCAD_31M_NATIVE_ANGLE_REQUEST stored_turns_differ=1 solved_turns=(100,-100) removes=1 adds=1 vectors=exact"
    );
    (rig.e, request)
}

/// §31M, native: the SAVED quadrilateral read three ways that agree. The drawing of its cold rebuild
/// turns -130° from Line 2 run from its End to Line 3 (atan2 of the very vectors the rule names),
/// keeps +100° and the three lengths; the form of that rebuild holds `A>B -130°` under a new UUID and
/// `A>B 100°` under the UUID it had; and the exported STL, read by `read_stl_of` here and not by
/// the writer, is a prism over the polygon of that drawing.
pub(crate) fn saved_angled_quad(saved: &Path, stl: &[u8], height: f64, was: Replaced) {
    let (loaded, scene) = accepted_scene(saved);
    let facts = scene.edit_source.clone().expect("facts");
    let sketch = facts.constraint_sketches[0].sketch;
    let stored = facts.constraint_sketches[0].stored.clone().expect("stored");
    let curves = curves_of(&presented(&loaded, sketch));
    let ids: Vec<_> = stored.curves.iter().map(|c| c.id).collect();
    let (a_l1, b_l2) = (
        vector(&ids, 0, LineEndpoint::Start),
        vector(&ids, 1, LineEndpoint::Start),
    );
    let (a_l2, b_l3) = (
        vector(&ids, 1, LineEndpoint::End),
        vector(&ids, 2, LineEndpoint::Start),
    );
    let (turn_a, turn_b) = (measured(&curves, a_l1, b_l2), measured(&curves, a_l2, b_l3));
    assert!(
        (turn_a - 100.).abs() < 1e-6 && (turn_b + 130.).abs() < 1e-6,
        "the saved drawing turns {turn_a} and {turn_b}"
    );
    let line = |i: usize| match curves[i].shape {
        Shape::Line { start, end } => (start, end),
        Shape::Circle { .. } => panic!("a Line"),
    };
    let length = |i: usize| (line(i).0[0] - line(i).1[0]).hypot(line(i).0[1] - line(i).1[1]);
    assert!(
        (length(0) - 50.).abs() < 1e-6
            && (length(1) - 30.).abs() < 1e-6
            && (length(2) - 40.).abs() < 1e-6,
        "the lengths the rules kept"
    );
    assert!(
        line(0).0[0].abs() < 1e-6 && line(0).0[1].abs() < 1e-6,
        "the anchor"
    );
    assert!(
        (line(0).0[1] - line(0).1[1]).abs() < 1e-6,
        "Line 1 is horizontal"
    );
    for i in 0..4 {
        let (p, q) = (line(i).1, line((i + 1) % 4).0);
        assert!(
            (p[0] - q[0]).abs() < 1e-6 && (p[1] - q[1]).abs() < 1e-6,
            "closed at {i}"
        );
    }
    // The cold reopen's form: its own labels, its rules by UUID.
    let mut e = Editor::default();
    assert!(e.begin_with(saved, &facts, &scene.sketches, sketch));
    let ctx = egui::Context::default();
    let mut out = frame(&ctx, &mut e, vec![]);
    for _ in 0..3 {
        out = frame(&ctx, &mut e, vec![]);
    }
    let mut words: Vec<_> = labels(&out).into_iter().map(|l| l.text).collect();
    words.sort();
    assert_eq!(words, ["30 mm", "40 mm", "50 mm", "A>B -130°", "A>B 100°"]);
    let (turns, _, _) = e.draft_turns().expect("a draft");
    assert_eq!(turns.len(), 2);
    let kept = turns
        .iter()
        .find(|t| t.angle.degrees() == 100.)
        .expect("R_a");
    let new = turns
        .iter()
        .find(|t| t.angle.degrees() == -130.)
        .expect("the new rule");
    assert_eq!(kept.id, was.1, "the kept rule keeps its UUID");
    assert_ne!(new.id, was.0, "the replaced rule is gone for good");
    assert!(turns.iter().all(|t| t.id != was.0));
    assert_eq!(
        (new.a, new.b),
        (a_l2, b_l3),
        "the new rule has the vectors the old one had"
    );
    // The STL: a prism over the polygon of the drawing, read by the test's own reader.
    let (faces, volume) = read_stl_of(stl);
    let expected = area_of(&curves) * height;
    assert!(
        (volume - expected).abs() <= 1e-4 * expected,
        "the exported solid has volume {volume}, the saved drawing {expected}"
    );
    let mut base: Vec<(i64, i64)> = faces
        .iter()
        .flatten()
        .filter(|p| p[2].abs() < 1e-6)
        .map(|p| ((p[0] * 1e3).round() as i64, (p[1] * 1e3).round() as i64))
        .collect();
    base.sort_unstable();
    base.dedup();
    let mut want: Vec<(i64, i64)> = (0..4)
        .map(|i| {
            (
                (line(i).0[0] * 1e3).round() as i64,
                (line(i).0[1] * 1e3).round() as i64,
            )
        })
        .collect();
    want.sort_unstable();
    assert_eq!(base, want, "the solid's base is the drawing's polygon");
    let top = faces
        .iter()
        .flatten()
        .map(|p| p[2])
        .fold(f64::MIN, f64::max);
    assert!((top - height).abs() < 1e-6);
}
