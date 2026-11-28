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
use super::canvas::{Rig, enlarged, middle, moved, near, wheel, whole};
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
    let source = crate::sketch_facts::Source::read(&document).expect("catalogue");
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
    let prepared = ferritecad_fixtures::constraint_changes(
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
    document.write_changes(&prepared).expect("angles");
    rotate_angle_rules(&mut document, choice.sketch);
    let source = crate::sketch_facts::Source::read(&document).expect("catalogue");
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
        let mut rules: Vec<_> = at.iter().map(|&i| s.constraints[i].clone()).collect();
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
    let prepared = ferritecad_fixtures::constraint_changes(
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
    document.write_changes(&prepared).expect("angles");
    let source = crate::sketch_facts::Source::read(&document).expect("catalogue");
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

// ---- native: the real accepted drawing, the real worker -------------------------------
