// SPDX-License-Identifier: MIT
//! §31N: the markers of the saved Horizontal, Vertical, Fixed, EqualLength, Parallel, Perpendicular and
//! Collinear rules (and a single Circle's Fixed centre) on the accepted drawing of the constraints
//! form, the selection that they, the persisted list and the Lines list share, and the explicit
//! removal of a selected rule, through the form's real widgets and real egui events.
//!
//! The drawings here are the app's own `Curve`s moved and enlarged over the stored curves'
//! identities: **synthetic** (see `canvas.rs`). The saved rules are deliberately not what the
//! stored coordinates satisfy, so a marker that read coordinates instead of rules fails. The real
//! presentation is read by the native gates at the end of this file and in `sessions.rs`.
#![allow(clippy::panic)]
use super::canvas::{
    Rig, enlarged, escape, middle, moved, near, primary, shift_primary, wheel, whole,
};
use super::dimension::{band, is_colour, label, labels, press_exact, selected_strokes};
use super::*;
use crate::sketch::accepted::{
    Curve, Drawing, HOVER_COLOUR, MARKER_FILL, PENDING_COLOUR, SELECTED_COLOUR, Shape, Target,
};
use crate::sketch::dimension::PAD;
use crate::sketch::marker::{Kind, Mark, Refs};

// ---- fixtures -------------------------------------------------------------------------

/// The seven rules of the plate of `markers_document`, by their UUIDs.
#[derive(Debug, Clone, Copy)]
struct Rules {
    fixed: StableEntityId,
    horizontal: StableEntityId,
    vertical: StableEntityId,
    equal: StableEntityId,
    parallel: StableEntityId,
    perpendicular: StableEntityId,
    collinear: StableEntityId,
}

impl Rules {
    /// In the order the rules are stored in `markers_document`: rotated by one place, so that it is
    /// neither the order they were made in (and so neither ascending nor descending by UUID) nor
    /// the order of the kinds.
    fn stored(&self) -> [(Kind, StableEntityId); 7] {
        [
            (Kind::Horizontal, self.horizontal),
            (Kind::Vertical, self.vertical),
            (Kind::EqualLength, self.equal),
            (Kind::Parallel, self.parallel),
            (Kind::Perpendicular, self.perpendicular),
            (Kind::Collinear, self.collinear),
            (Kind::Fixed, self.fixed),
        ]
    }
}

/// The rules of the plate of `markers_document`, read from the stored Sketch and not through the
/// form: the Fixed endpoint, Horizontal, Vertical, EqualLength, Parallel, Perpendicular and
/// Collinear rule.
fn rules_of(stored: &Sketch) -> Rules {
    let find = |want: fn(&SketchConstraintRule) -> bool| {
        let all: Vec<_> = stored
            .constraints
            .iter()
            .filter(|c| want(&c.rule))
            .collect();
        assert_eq!(all.len(), 1, "one rule of the family");
        all[0].id
    };
    Rules {
        fixed: find(|r| matches!(r, SketchConstraintRule::Fixed { .. })),
        horizontal: find(|r| matches!(r, SketchConstraintRule::Horizontal { .. })),
        vertical: find(|r| matches!(r, SketchConstraintRule::Vertical { .. })),
        equal: find(|r| matches!(r, SketchConstraintRule::EqualLength { .. })),
        parallel: find(|r| matches!(r, SketchConstraintRule::Parallel { .. })),
        perpendicular: find(|r| matches!(r, SketchConstraintRule::Perpendicular { .. })),
        collinear: find(|r| matches!(r, SketchConstraintRule::Collinear { .. })),
    }
}

fn is_marker_kind(rule: &SketchConstraintRule) -> bool {
    matches!(
        rule,
        SketchConstraintRule::Fixed { .. }
            | SketchConstraintRule::Horizontal { .. }
            | SketchConstraintRule::Vertical { .. }
            | SketchConstraintRule::EqualLength { .. }
            | SketchConstraintRule::Parallel { .. }
            | SketchConstraintRule::Perpendicular { .. }
            | SketchConstraintRule::Collinear { .. }
    )
}

fn swapped(
    segment: ferritecad_document::SketchSegmentRef,
) -> ferritecad_document::SketchSegmentRef {
    ferritecad_document::SketchSegmentRef::new(segment.to, segment.from)
}

/// The plate of the other form gates (four Lines, kernel-free) that keeps one saved rule of each of
/// the seven families, and the two lengths of `persist_rectangle_lengths`:
///
/// * Fixed: Line 1's End at (1.25, -2.5);
/// * Horizontal: Line 1, its two points stored **End then Start**;
/// * Vertical: Line 2;
/// * EqualLength: Lines 1 and 3, the second segment stored **End to Start**;
/// * Parallel: Lines 2 and 3, the first segment stored **End to Start**;
/// * Perpendicular: Lines 3 and 4; Collinear: Lines 1 and 4 (the four pairs stand at four
///   different places of an enlarged plate; the same-place overlaps have their own drawing);
/// * Line length 60 on Line 1 and 30 on Line 2, and a directed angle of -70° from Line 1 to Line 2.
///
/// The seven are written in this order and then stored rotated by one place (Horizontal first,
/// Fixed last), so no order of theirs is the one of their UUIDs.
fn markers_document() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let (root, path, _) = fixture();
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
    let line = |i: usize, kind| {
        AddSketchConstraint::Line(AddLineConstraint::Line {
            curve: ids[i],
            kind,
        })
    };
    let relation = |a: usize, b: usize, relation| {
        AddSketchConstraint::Line(AddLineConstraint::Relation {
            a: ids[a],
            b: ids[b],
            relation,
        })
    };
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        choice.sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                line(
                    0,
                    LineConstraintKind::Fixed {
                        at: LineEndpoint::End,
                        x: SketchCoordinateMm::new(1.25).expect("x"),
                        y: SketchCoordinateMm::new(-2.5).expect("y"),
                    },
                ),
                line(0, LineConstraintKind::Horizontal),
                line(1, LineConstraintKind::Vertical),
                AddSketchConstraint::Line(AddLineConstraint::EqualLength {
                    a: ids[0],
                    b: ids[2],
                }),
                relation(1, 2, LineRelation::Parallel),
                relation(2, 3, LineRelation::Perpendicular),
                relation(0, 3, LineRelation::Collinear),
                line(
                    0,
                    LineConstraintKind::Distance(LineLengthMm::new(60.).expect("60")),
                ),
                line(
                    1,
                    LineConstraintKind::Distance(LineLengthMm::new(30.).expect("30")),
                ),
                AddSketchConstraint::Line(AddLineConstraint::DirectedAngle {
                    a: DirectedLine::new(ids[0], LineEndpoint::Start, LineEndpoint::End)
                        .expect("a vector"),
                    b: DirectedLine::new(ids[1], LineEndpoint::Start, LineEndpoint::End)
                        .expect("a vector"),
                    angle: DirectedAngle::from_degrees(-70.).expect("an angle"),
                }),
            ],
        },
    )
    .expect("prepare");
    document.write_sketch_constraints(&prepared).expect("rules");
    let mut object = document
        .object(choice.sketch)
        .expect("read")
        .expect("sketch");
    if let ferritecad_document::ObjectPayload::Sketch(s) = &mut object.payload {
        for c in &mut s.constraints {
            match &mut c.rule {
                SketchConstraintRule::Horizontal { a, b } => std::mem::swap(a, b),
                SketchConstraintRule::EqualLength { b, .. } => *b = swapped(*b),
                SketchConstraintRule::Parallel { a, .. } => *a = swapped(*a),
                _ => {}
            }
        }
        let at: Vec<_> = (0..s.constraints.len())
            .filter(|&i| is_marker_kind(&s.constraints[i].rule))
            .collect();
        assert_eq!(at.len(), 7);
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
        .expect("reversed references, stored rotated");
    let source = ExtrudeEditSource::read(&document).expect("catalogue");
    document.close().expect("close");
    (root, path, source)
}

impl Rig {
    /// The form over `markers_document`, handed the enlarged drawing.
    fn markers(pixels_per_point: f32) -> Self {
        let (root, path, source) = markers_document();
        Self::over(Some(root), &path, &source, pixels_per_point, enlarged)
    }
    /// The UUID of the selected rule, if one is selected.
    fn mark(&self) -> Option<StableEntityId> {
        self.e.draft_marks().expect("a draft").1
    }
    /// The selected angle, if any.
    fn turn_selected(&self) -> Option<StableEntityId> {
        self.e.draft_turns().expect("a draft").1
    }
    /// The pair slots, the way each vector runs and the angle text: unapplied input for a new rule.
    fn pair_slots(&self) -> super::AngleState {
        self.e.draft_angle().expect("a draft")
    }
    fn rules(&self) -> Rules {
        rules_of(&self.stored)
    }
    /// The bounded list of persisted rules scrolled back to its top. `scroll_to_row` only goes down,
    /// and where a row stands in the list depends on how many lines the rows above it wrapped into
    /// (their UUIDs differ from run to run): a test never starts from wherever the list happens to be.
    fn scroll_list_to_top(&mut self) {
        self.settle();
        for _ in 0..12 {
            let anchor = text_center(&self.frame(vec![]), "Persisted constraints:");
            self.frame(vec![moved(anchor + egui::vec2(0., 40.)), wheel(60.)]);
        }
    }
    /// Press the persisted row that starts with `prefix`.
    fn press_rule_row(&mut self, prefix: &str) {
        self.scroll_list_to_top();
        scroll_to_row(&self.ctx, &mut self.e, prefix);
        self.press(prefix);
    }
    /// Tick the Remove of the persisted row that starts with `prefix`.
    fn tick_remove(&mut self, prefix: &str) {
        self.scroll_list_to_top();
        click_remove_on_row(&self.ctx, &mut self.e, prefix);
    }
    /// Bring the editor of the selected rule (and its Remove constraint button) into view.
    fn reach_remove_constraint(&mut self) {
        self.scroll_list_to_top();
        scroll_to_row(&self.ctx, &mut self.e, "Remove constraint");
    }
    /// The centre of the painted marker `text`, to aim a click at.
    fn at_marker(&mut self, text: &str) -> egui::Pos2 {
        marker(&self.frame(vec![]), text).rect.center()
    }
    /// Select the rule whose marker says `text`, under the Rule target.
    fn choose_mark(&mut self, text: &str) {
        if self.canvas().target() != Target::Rule {
            self.target(Target::Rule);
        }
        let at = self.at_marker(text);
        self.click(at);
        self.settle();
    }
    /// Everything a selection must leave alone, and the three selections.
    fn facts3(
        &self,
    ) -> (
        super::canvas::Facts,
        (Option<StableEntityId>, Option<StableEntityId>),
        super::AngleState,
    ) {
        (
            self.facts(),
            (self.turn_selected(), self.mark()),
            self.pair_slots(),
        )
    }
}

// ---- reading the painted frame --------------------------------------------------------

/// A marker as painted: its words, the box it stands in, the border of that box and the colour of
/// its words.
#[derive(Debug, Clone)]
struct Marker {
    text: String,
    rect: egui::Rect,
    stroke: egui::Stroke,
    ink: egui::Color32,
}

/// Whether a painted fill is the marker tint, at the strength the window paints it with: the
/// numeric labels' boxes are black and are not.
fn is_marker_fill(fill: egui::Color32) -> bool {
    let a = f32::from(fill.a());
    let want = f32::from(MARKER_FILL.a());
    a > 0.
        && [
            (fill.r(), MARKER_FILL.r()),
            (fill.g(), MARKER_FILL.g()),
            (fill.b(), MARKER_FILL.b()),
        ]
        .iter()
        .all(|(x, y)| (f32::from(*x) / a - f32::from(*y) / want).abs() < 0.02)
}

fn markers(out: &egui::FullOutput) -> Vec<Marker> {
    let boxes: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if is_marker_fill(r.fill) => Some((r.rect, r.stroke)),
            _ => None,
        })
        .collect();
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => {
                let (rect, stroke) = boxes.iter().find(|(r, _)| near(r.min + PAD, t.pos))?;
                Some(Marker {
                    text: t.galley.text().to_owned(),
                    rect: *rect,
                    stroke: *stroke,
                    ink: t.fallback_color,
                })
            }
            _ => None,
        })
        .collect()
}

/// Every marker of a frame with its box, for the message of a failed assertion.
fn described(out: &egui::FullOutput) -> String {
    markers(out)
        .iter()
        .map(|m| format!("{} {:?}", m.text, m.rect))
        .collect::<Vec<_>>()
        .join("; ")
}

fn marker(out: &egui::FullOutput, text: &str) -> Marker {
    let found: Vec<_> = markers(out)
        .into_iter()
        .filter(|m| m.text == text)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "{text:?} among {:?}",
        markers(out).iter().map(|m| &m.text).collect::<Vec<_>>()
    );
    found[0].clone()
}

fn has_marker(out: &egui::FullOutput, text: &str) -> bool {
    markers(out).iter().any(|m| m.text == text)
}

fn painted(out: &egui::FullOutput, words: &str) -> bool {
    out.shapes
        .iter()
        .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == words))
}

fn painted_containing(out: &egui::FullOutput, words: &str) -> bool {
    out.shapes
        .iter()
        .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains(words)))
}

/// The dots and rings of a colour in a frame: a filled circle of radius `radius`, or an outline.
fn circles(
    out: &egui::FullOutput,
    colour: egui::Color32,
    radius: f32,
    filled: bool,
) -> Vec<egui::Pos2> {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Circle(c) if (c.radius - radius).abs() < 0.01 => {
                let hit = if filled {
                    c.fill.a() > 0 && is_colour(c.fill, colour)
                } else {
                    c.fill.a() == 0 && c.stroke.width > 0. && is_colour(c.stroke.color, colour)
                };
                hit.then_some(c.center)
            }
            _ => None,
        })
        .collect()
}

fn dots(out: &egui::FullOutput) -> Vec<egui::Pos2> {
    circles(out, SELECTED_COLOUR, 3., true)
}

fn rings(out: &egui::FullOutput) -> Vec<egui::Pos2> {
    circles(out, SELECTED_COLOUR, 4., false)
}

fn point_in(list: &[egui::Pos2], p: egui::Pos2) -> bool {
    list.iter().any(|q| near(*q, p))
}

/// Where a stored point reference is on the SOLVED drawing of the form, on the screen now.
fn on_screen(rig: &Rig, point: ferritecad_document::SketchPointRef) -> egui::Pos2 {
    let i = rig
        .ids
        .iter()
        .position(|id| *id == point.curve)
        .expect("a curve of the Sketch");
    let [start, end] = rig.ends(i);
    match point.at {
        ferritecad_document::SketchPointSelector::Start => start,
        ferritecad_document::SketchPointSelector::End => end,
        other => panic!("a Line has no {other:?}"),
    }
}

// ---- the markers and their selection --------------------------------------------------

#[test]
fn a_marker_states_its_kind_and_selects_exactly_its_rule_only_under_the_rule_target() {
    for pixels_per_point in [1., 2.] {
        let mut rig = Rig::markers(pixels_per_point);
        let rules = rig.rules();
        let out = rig.frame(vec![]);
        // Seven markers, each saying its kind in ASCII, and the lengths and the angle in their own black
        // boxes: no marker is taken for a label and no label for a marker.
        let mut words: Vec<_> = markers(&out).into_iter().map(|m| m.text).collect();
        words.sort();
        assert_eq!(
            words,
            ["Col", "Eq", "Fix", "H", "Par", "Perp", "V"],
            "{pixels_per_point}x"
        );
        let mut numbers: Vec<_> = labels(&out).into_iter().map(|l| l.text).collect();
        numbers.sort();
        assert_eq!(
            numbers,
            ["30 mm", "60 mm", "A>B -70°"],
            "{pixels_per_point}x"
        );
        // The form reads its rules by their own UUIDs, in the order of the stored rules.
        let (formed, none) = rig.e.draft_marks().expect("a draft");
        assert_eq!(none, None);
        assert_eq!(
            formed.iter().map(|m| (m.kind, m.id)).collect::<Vec<_>>(),
            rules.stored().to_vec()
        );

        // Under every other target a marker is a picture: a click on its place selects no rule
        // (a Line within nine points of it may still be picked, as ever).
        for target in [
            Target::Object,
            Target::Endpoint,
            Target::PairA,
            Target::PairB,
            Target::Dimension,
        ] {
            let mut rig = Rig::markers(pixels_per_point);
            rig.target(target);
            let before = rig.kept();
            for (kind, _) in rules.stored() {
                let at = rig.at_marker(kind.short());
                rig.click(at);
                assert_eq!(
                    rig.mark(),
                    None,
                    "{pixels_per_point}x: {target:?} took the marker {kind:?}"
                );
            }
            assert_eq!(
                rig.kept(),
                before,
                "{target:?}: a click is no input, no step"
            );
        }

        // Under the Rule target the marker is picked by its box, and only the rule of that marker
        // is selected: no Line, no angle, no slot, no pin, no text, no step, no request.
        let mut rig = Rig::markers(pixels_per_point);
        let rules = rig.rules();
        rig.target(Target::Rule);
        let before = rig.facts3();
        for (kind, rule) in rules.stored() {
            let at = rig.at_marker(kind.short());
            rig.click(at);
            rig.settle();
            let shown = described(&rig.frame(vec![]));
            assert_eq!(
                rig.mark(),
                Some(rule),
                "{pixels_per_point}x: {kind:?} at {at:?} among {shown}"
            );
            assert_eq!(
                (rig.picks().0, rig.turn_selected()),
                (None, None),
                "no Line and no angle is selected beside a rule"
            );
            let mut now = rig.facts3();
            // The target is the same and so is everything but the selection of the rule.
            now.1 = before.1;
            assert_eq!(now, before, "{pixels_per_point}x: {kind:?} moved something");
            let out = rig.frame(vec![]);
            for (other, _) in rules.stored() {
                let chosen = is_colour(marker(&out, other.short()).stroke.color, SELECTED_COLOUR);
                assert_eq!(chosen, other == kind, "{kind:?} chosen, {other:?} painted");
            }
        }
        // The empty canvas, a Line and a numeric label are no marker: the selection stays.
        let kept = rig.mark();
        rig.click(rig.mid(1));
        rig.click(rig.rect().center() + egui::vec2(100., 40.));
        assert_eq!(rig.mark(), kept, "{pixels_per_point}x");

        // Hovering a marker names the kind, the rule and what it names, as the rule stores it.
        let at = rig.at_marker("Perp");
        let out = rig.frame(vec![moved(at)]);
        let caption = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text().starts_with("Perpendicular · ") => {
                    Some(t.galley.text().to_owned())
                }
                _ => None,
            })
            .expect("a caption");
        assert!(
            caption.contains(&format!("saved rule {}", short(rules.perpendicular)))
                && caption.contains(&format!("Line {}", short(rig.ids[2])))
                && caption.contains(&format!("Line {}", short(rig.ids[3]))),
            "{caption}"
        );
        // The caption of a marker stays between the header and the footer of the canvas.
        let box_of = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == caption => {
                    Some(t.visual_bounding_rect())
                }
                _ => None,
            })
            .expect("caption box");
        let canvas = rig.rect();
        assert!(
            box_of.top() >= canvas.top() + 20. && box_of.bottom() <= canvas.bottom() - 24.,
            "{box_of:?} on {canvas:?}"
        );
    }
}

#[test]
fn the_selected_rule_marks_exactly_the_references_it_stores_and_a_reversed_segment_stays_as_stored()
{
    let mut rig = Rig::markers(1.);
    let rules = rig.rules();
    let stored = rig.stored.clone();
    // The Lines each rule names (by their index), and the points the rule names in its own order.
    let lines_of = |kind: Kind| -> Vec<usize> {
        match kind {
            Kind::Horizontal | Kind::Fixed => vec![0],
            Kind::Vertical => vec![1],
            Kind::EqualLength => vec![0, 2],
            Kind::Parallel => vec![1, 2],
            Kind::Perpendicular => vec![2, 3],
            Kind::Collinear => vec![0, 3],
        }
    };
    for (kind, rule) in rules.stored() {
        rig.choose_mark(kind.short());
        assert_eq!(rig.mark(), Some(rule));
        let out = rig.frame(vec![]);
        // Exactly the Lines of the rule are in the selection blue, on the SOLVED drawing.
        let marked = selected_strokes(&out);
        let ends: Vec<_> = (0..4).map(|i| rig.ends(i)).collect();
        for (i, end) in ends.iter().enumerate() {
            assert_eq!(
                marked
                    .iter()
                    .any(|s| near(s[0], end[0]) && near(s[1], end[1])),
                lines_of(kind).contains(&i),
                "{kind:?}: Line {}",
                i + 1
            );
        }
        // The references are the stored ones, not the marker's own idea of them.
        let held = rig.e.draft_marks().expect("draft").0;
        let mark = held.iter().find(|m| m.id == rule).copied().expect("held");
        let stored_rule = stored
            .constraints
            .iter()
            .find(|c| c.id == rule)
            .map(|c| c.rule.clone())
            .expect("stored");
        let points = mark.refs.points();
        match (stored_rule, mark.refs) {
            (SketchConstraintRule::Horizontal { a, b }, Refs::Line(x, y)) => {
                assert_eq!((x, y), (a, b));
            }
            (SketchConstraintRule::Fixed { point, .. }, Refs::Point(p)) => assert_eq!(p, point),
            (SketchConstraintRule::EqualLength { a, b }, Refs::Pair(x, y))
            | (SketchConstraintRule::Parallel { a, b }, Refs::Pair(x, y))
            | (SketchConstraintRule::Perpendicular { a, b }, Refs::Pair(x, y))
            | (SketchConstraintRule::Collinear { a, b }, Refs::Pair(x, y)) => {
                assert_eq!((x, y), (a, b), "{kind:?}: a segment was rewritten");
            }
            (SketchConstraintRule::Vertical { a, b }, Refs::Line(x, y)) => {
                assert_eq!((x, y), (a, b));
            }
            other => panic!("{kind:?}: {other:?}"),
        }
        let dot_at = dots(&out);
        let ring_at = rings(&out);
        if kind == Kind::Fixed {
            // The pinned END of Line 1: a filled disc and a ring around it, and the word.
            let at = on_screen(&rig, points[0]);
            assert_eq!(points[0].at, ferritecad_document::SketchPointSelector::End);
            assert!(near(at, ends[0][1]));
            assert!(point_in(&circles(&out, SELECTED_COLOUR, 5., true), at));
            assert!(painted(&out, "End") && !painted(&out, "Start"));
            assert!(dot_at.is_empty() && ring_at.is_empty());
        } else {
            // The first point of each reference a dot, the second a ring, where the rule says:
            // a reversed segment puts its dot on the End of the Line.
            assert_eq!(dot_at.len(), points.len() / 2, "{kind:?}: {dot_at:?}");
            assert_eq!(ring_at.len(), points.len() / 2, "{kind:?}: {ring_at:?}");
            for (i, p) in points.iter().enumerate() {
                let at = on_screen(&rig, *p);
                assert!(
                    point_in(if i % 2 == 0 { &dot_at } else { &ring_at }, at),
                    "{kind:?}: point {i} ({p}) is not marked where the rule puts it"
                );
            }
        }
    }
    // The three reversed pieces of the fixture, by name: Horizontal names End then Start, the
    // EqualLength's second segment runs End to Start, the Parallel's first as well.
    let at = |kind: Kind| {
        rig.e
            .draft_marks()
            .expect("draft")
            .0
            .into_iter()
            .find(|m| m.kind == kind)
            .expect("held")
            .refs
    };
    let ids = rig.ids.clone();
    let p = ferritecad_document::SketchPointRef::new;
    use ferritecad_document::SketchPointSelector::{End, Start};
    assert_eq!(
        at(Kind::Horizontal),
        Refs::Line(p(ids[0], End), p(ids[0], Start))
    );
    assert!(matches!(at(Kind::EqualLength),
        Refs::Pair(a, b) if a.from == p(ids[0], Start) && b.from == p(ids[2], End) && b.to == p(ids[2], Start)));
    assert!(matches!(at(Kind::Parallel),
        Refs::Pair(a, b) if a.from == p(ids[1], End) && a.to == p(ids[1], Start) && b.from == p(ids[2], Start)));
}

// ---- one selection --------------------------------------------------------------------

/// The words of the persisted row of a rule of the plate of `markers_document`.
fn row(kind: Kind, rule: StableEntityId) -> String {
    match kind {
        Kind::Fixed => format!("Fixed point end (1.25, -2.5) mm · {rule}"),
        other => format!("{} · {rule}", other.name()),
    }
}

#[test]
fn the_markers_the_lines_list_the_persisted_rows_and_the_other_labels_show_one_selection_and_the_pair_slots_never_move()
 {
    let mut rig = Rig::markers(1.);
    let ids = rig.ids.clone();
    let rules = rig.rules();
    let (len60, len30) = (rule_of(&rig.stored, 60.), rule_of(&rig.stored, 30.));
    let angle = rig.e.draft_turns().expect("draft").0[0].id;
    let before = rig.kept();
    assert!(
        !painted(&rig.frame(vec![]), "Remove constraint"),
        "nothing is selected"
    );

    // A persisted row selects its rule, as a marker does: the marker, the Lines and the editor.
    rig.press_rule_row(&row(Kind::Parallel, rules.parallel));
    assert_eq!(
        (rig.mark(), rig.picks().0, rig.turn_selected()),
        (Some(rules.parallel), None, None)
    );
    rig.settle();
    let out = rig.frame(vec![]);
    assert!(is_colour(marker(&out, "Par").stroke.color, SELECTED_COLOUR));
    assert!(painted(&out, "Remove constraint") && painted(&out, "No change pending."));
    assert_eq!(rig.kept(), before, "choosing a row is no step and no input");

    // The Lines list selects a Line and the rule is given up; no editor is left behind.
    rig.press("Segment 3");
    rig.settle();
    let out = rig.frame(vec![]);
    assert_eq!((rig.mark(), rig.picks().0), (None, Some(ids[2])));
    assert!(!painted(&out, "Remove constraint"));
    assert!(
        markers(&out)
            .iter()
            .all(|m| !is_colour(m.stroke.color, SELECTED_COLOUR)),
        "no marker is chosen"
    );
    // The row of a length takes the Line's selection; the row of a marker takes it back.
    rig.press_rule_row(&format!("Line length 30 mm · {len30}"));
    assert_eq!((rig.mark(), rig.picks().0), (None, Some(ids[1])));
    rig.press_rule_row(&row(Kind::Horizontal, rules.horizontal));
    assert_eq!((rig.mark(), rig.picks().0), (Some(rules.horizontal), None));
    // Pair A and Pair B on the canvas leave the rule selection and the whole of the other input
    // alone, and what they fill is the slots only.
    rig.target(Target::PairA);
    rig.click(rig.mid(3));
    rig.target(Target::PairB);
    rig.click(rig.mid(1));
    assert_eq!(rig.mark(), Some(rules.horizontal));
    assert_eq!(rig.pair_slots().0, (Some(ids[3]), Some(ids[1])));
    assert_eq!(rig.picks().0, None);
    // An Object click on a Line is the Line's selection and the rule is given up.
    rig.target(Target::Object);
    rig.click(rig.mid(2));
    assert_eq!((rig.mark(), rig.picks().0), (None, Some(ids[2])));
    // A marker takes the Line's selection from the canvas; a numeric label gives it up again.
    rig.choose_mark("V");
    assert_eq!((rig.mark(), rig.picks().0), (Some(rules.vertical), None));
    rig.target(Target::Dimension);
    let at = rig.at_label("60 mm");
    rig.click(at);
    assert_eq!((rig.mark(), rig.picks().0), (None, Some(ids[0])));
    let _ = len60;
    // An angle is the third kind of selection: the label takes it and gives the rule up, a marker
    // takes the rule back and gives the angle up. At no time are two of the three set.
    let at = rig.at_label("A>B -70°");
    rig.click(at);
    assert_eq!(
        (rig.mark(), rig.picks().0, rig.turn_selected()),
        (None, None, Some(angle))
    );
    rig.choose_mark("Col");
    assert_eq!(
        (rig.mark(), rig.picks().0, rig.turn_selected()),
        (Some(rules.collinear), None, None)
    );
    rig.press_rule_row(&format!(
        "Directed angle {}° · {angle}",
        rig.e.draft_turns().expect("draft").0[0].angle.degrees()
    ));
    assert_eq!(
        (rig.mark(), rig.picks().0, rig.turn_selected()),
        (None, None, Some(angle))
    );
    // The pair slots are what they were set to, through all of it.
    assert_eq!(rig.pair_slots().0, (Some(ids[3]), Some(ids[1])));

    // With a rule selected the buttons that need a Line take nothing until a Line is chosen again.
    rig.choose_mark("Eq");
    let held = rig.kept();
    rig.press("Add Horizontal");
    rig.press("Add length");
    rig.press("Add Fixed point");
    assert_eq!(rig.kept(), held, "a button that needs a Line took a press");
    rig.press("Segment 3");
    rig.press("Add Horizontal");
    assert_eq!(rig.edits().add.len(), 1);
}

// ---- the removal ----------------------------------------------------------------------

/// The typed texts of the form, which no selection and no removal may touch: the first eleven
/// of `draft_inputs` (the boxes, the unit and the vector directions), before the request, its
/// history and the refusal.
fn typed_texts(rig: &Rig) -> Vec<String> {
    rig.e.draft_inputs().expect("draft")[..11].to_vec()
}

#[test]
fn remove_constraint_prepares_one_step_once_and_undo_redo_and_clear_return_exactly_the_request() {
    for pixels_per_point in [1., 2.] {
        let mut rig = Rig::markers(pixels_per_point);
        let (rules, ids) = (rig.rules(), rig.ids.clone());
        let len30 = rule_of(&rig.stored, 30.);
        // Unrelated input and pending changes of the person's own, which the removal must keep:
        // typed texts, a pair with its directions, a pending Horizontal and the ticked removal of a
        // length.
        enter_field(&rig.ctx, &mut rig.e, "Line length (mm):", "12.50", false);
        enter_field(&rig.ctx, &mut rig.e, "Fixed X (mm):", "3", false);
        rig.press("Segment 4");
        rig.press("Pair Line A");
        rig.press("A: start>end");
        rig.press("Segment 2");
        rig.press("Pair Line B");
        rig.press("Segment 3");
        rig.press("Add Horizontal");
        rig.tick_remove("Line length 30 mm");
        let plain = rig.edits();
        assert_eq!(plain.remove, vec![len30]);
        assert_eq!(plain.add.len(), 1);
        // A pending addition is no saved rule: it has no marker, and the seven saved rules are the
        // only ones the canvas holds.
        let out = rig.frame(vec![]);
        assert_eq!(markers(&out).len(), 7, "{}", described(&out));
        assert_eq!(
            markers(&out).iter().filter(|m| m.text == "H").count(),
            1,
            "the pending Horizontal of Line 3 got a marker"
        );
        let (texts, slots, steps) = (typed_texts(&rig), rig.pair_slots(), rig.history());
        let drawing = rig.solved();

        // One gesture, one step: the rule joins the removals, the rest of the request is as it was.
        rig.choose_mark("Par");
        assert_eq!(rig.edits(), plain, "choosing a marker prepares nothing");
        assert_eq!(rig.history(), steps);
        rig.press("Remove constraint");
        let removed = rig.edits();
        assert_eq!(removed.remove, vec![len30, rules.parallel]);
        assert_eq!(removed.add, plain.add);
        assert_eq!(rig.history(), (steps.0 + 1, 0), "{pixels_per_point}x");
        assert_eq!(rig.refusal(), None);
        assert_eq!(typed_texts(&rig), texts, "a typed text moved");
        assert_eq!(
            rig.pair_slots(),
            slots,
            "the pair slots or directions moved"
        );
        assert_eq!(rig.mark(), Some(rules.parallel), "the selection is kept");
        // What the person sees: the marker says what is prepared (amber, beside its kind), the
        // saved rule and its row are there as they were, and the drawing has not moved.
        rig.settle();
        let out = rig.frame(vec![]);
        let told = marker(&out, "Par -> removed");
        assert!(is_colour(told.ink, PENDING_COLOUR), "{told:?}");
        assert!(
            !has_marker(&out, "Par") && has_marker(&out, "Eq") && has_marker(&out, "H"),
            "only the removed rule's marker changed"
        );
        assert!(painted(
            &out,
            "Pending: this constraint is removed; the drawing keeps it until Apply constraints."
        ));
        assert_eq!(rig.solved(), drawing, "a prepared removal draws nothing");
        // The saved rule is still the rule of its row: a different marker is not "removed" because
        // this one is.
        for other in ["H", "V", "Eq", "Perp", "Col", "Fix"] {
            assert!(has_marker(&out, other), "{other}");
        }

        // A repeat is the same request: no step, no duplicate, and nothing that was undone is lost.
        rig.press("Remove constraint");
        assert_eq!(rig.edits(), removed);
        assert_eq!(rig.history(), (steps.0 + 1, 0));
        assert_eq!(rig.refusal(), None, "a repeat is no refusal either");
        rig.tick_remove("Collinear");
        let both = rig.edits();
        assert_eq!(both.remove, vec![len30, rules.parallel, rules.collinear]);
        rig.press("Undo");
        assert_eq!(rig.edits(), removed);
        assert_eq!(rig.history(), (steps.0 + 1, 1));
        rig.press("Remove constraint");
        assert_eq!(rig.edits(), removed, "the repeat changed the request");
        assert_eq!(
            rig.history(),
            (steps.0 + 1, 1),
            "a repeat wiped the Redo of a request it did not change"
        );
        rig.press("Redo");
        assert_eq!(rig.edits(), both);

        // The form's own road to the same request: the Remove tick on the row of the rule.
        let mut ticked = Rig::markers(pixels_per_point);
        ticked.tick_remove("Parallel");
        let mut marked = Rig::markers(pixels_per_point);
        marked.choose_mark("Par");
        marked.press("Remove constraint");
        assert_eq!(ticked.edits().remove.len(), 1);
        assert_eq!(
            (marked.edits().remove.len(), marked.edits().add.len()),
            (1, 0)
        );
        // Not the same UUIDs (two documents), but the same shape: one removal of the Parallel.
        assert_eq!(
            marked.edits().remove,
            vec![marked.rules().parallel],
            "{pixels_per_point}x"
        );
        assert_eq!(ticked.edits().remove, vec![ticked.rules().parallel]);

        // Undo and Redo and Clear return exactly the request.
        while rig.history().0 > steps.0 {
            rig.press("Undo");
        }
        assert_eq!(rig.edits(), plain, "Undo did not return the request");
        assert!(has_marker(&rig.frame(vec![]), "Par"));
        rig.press("Redo");
        rig.press("Redo");
        assert_eq!(rig.edits(), both);
        rig.press("Clear pending changes");
        assert_eq!(rig.edits(), SketchConstraintEdits::default());
        rig.press("Undo");
        assert_eq!(
            rig.edits(),
            both,
            "Undo after Clear did not return the request"
        );
        assert_eq!(typed_texts(&rig), texts);
        assert_eq!(rig.pair_slots(), slots);

        // The request that leaves the form is the prepared one, nothing else: the Save of a copy.
        rig.press("Save constraints copy…");
        let asked = rig.e.take_request().expect("the Save asked for");
        assert_eq!(asked.edits, both);
        let draft = rig.e.draft.as_ref().expect("draft");
        assert_eq!(
            (asked.sketch, asked.expected, &asked.source),
            (draft.choice.sketch, draft.version, &draft.source)
        );
        let _ = (ids, len30);
    }
}

/// The UUID of the stored length of `mm` (the same helper the other gates use).
fn rule_of(stored: &Sketch, mm: f64) -> StableEntityId {
    super::dimension::rule_of(stored, mm)
}

#[test]
fn a_removal_the_validator_refuses_keeps_the_text_the_selection_and_the_history_exactly() {
    let mut rig = Rig::markers(1.);
    let rules = rig.rules();
    let ids = rig.ids.clone();
    // A draft the validator accepts only as a whole: the Parallel of Lines 1 and 3 is removed, a
    // Perpendicular takes its slot, and ticking the removal off again leaves the pair with an
    // answer too many. The form shows why and keeps the request as it is.
    rig.choose_mark("Par");
    rig.press("Remove constraint");
    rig.press("Segment 2");
    rig.press("Pair Line A");
    rig.press("Segment 3");
    rig.press("Pair Line B");
    rig.press("Add Perpendicular");
    assert_eq!(
        rig.refusal(),
        None,
        "the slot was freed in the same request"
    );
    assert_eq!(rig.edits().remove, vec![rules.parallel]);
    assert_eq!(rig.edits().add.len(), 1);
    rig.tick_remove("Parallel");
    assert!(rig.edits().remove.is_empty(), "the removal is ticked off");
    assert!(
        rig.e
            .draft
            .as_ref()
            .expect("draft")
            .choice
            .validate_edits(&rig.edits())
            .is_err(),
        "the pair holds two answers"
    );

    // A removal of another rule is judged with the whole request: refused, and nothing moves.
    rig.choose_mark("H");
    enter_field(&rig.ctx, &mut rig.e, "Fixed Y (mm):", "7.5", false);
    let held = rig.kept();
    let edits = rig.edits();
    rig.press("Remove constraint");
    let refusal = rig.refusal().expect("the validator's refusal");
    assert!(
        refusal.contains("Parallel") || refusal.contains("directed angle"),
        "{refusal}"
    );
    let mut now = rig.kept();
    now.0 = held.0.clone();
    assert_eq!(
        now, held,
        "a refused removal changed the history or made a request"
    );
    assert_eq!(rig.edits(), edits);
    assert_eq!(rig.mark(), Some(rules.horizontal), "the selection is kept");
    assert_eq!(
        rig.e.draft_inputs().expect("draft")[4],
        "7.5",
        "the typed text is kept exactly"
    );
    assert!(rig.e.take_request().is_none() && rig.e.take_apply_request().is_none());
    // The refusal does not outlive the next valid action: the Perpendicular is taken back, the
    // request is whole again, and the removal of the Horizontal is then accepted as one step.
    rig.press("Undo");
    rig.press("Undo");
    assert_eq!(rig.refusal(), None);
    let steps = rig.history();
    rig.press("Remove constraint");
    assert_eq!(rig.refusal(), None);
    assert!(rig.edits().remove.contains(&rules.horizontal));
    assert_eq!(rig.history().0, steps.0 + 1);
    let _ = ids;
}

#[test]
fn a_cancelled_save_or_a_stale_reply_keeps_the_selected_rule_and_the_prepared_removal() {
    let mut rig = Rig::markers(1.);
    let rules = rig.rules();
    rig.choose_mark("Perp");
    rig.press("Remove constraint");
    rig.press("Save constraints copy…");
    let asked = rig.e.take_request().expect("the Save asked for");
    assert_eq!(asked.edits.remove, vec![rules.perpendicular]);
    assert!(asked.edits.add.is_empty());
    let kept = (rig.kept(), rig.mark());
    let mut state = crate::edits::Edits::default();
    let (tx, rx) = std::sync::mpsc::channel();
    let generation = state
        .start_constraints(asked, move |_, g, _| {
            std::thread::spawn(move || tx.send(g).expect("reply"))
        })
        .expect("start");
    // A reply of another generation, and the worker's refusal or a cancelled Save: the draft, the
    // selection and the marker of the prepared removal are what they were.
    assert!(
        finish_edit(
            &mut rig.e,
            &mut state,
            generation + 1,
            Err(ferritecad_types::CadError::input("stale"))
        )
        .is_none()
    );
    assert_eq!((rig.kept(), rig.mark()), kept);
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
    assert_eq!((rig.kept(), rig.mark()), kept);
    rig.settle();
    let out = rig.frame(vec![]);
    assert!(has_marker(&out, "Perp -> removed") && painted(&out, "Remove constraint"));
    // And the form takes the next action as if nothing had happened.
    rig.choose_mark("V");
    rig.press("Remove constraint");
    assert_eq!(
        rig.edits().remove,
        vec![rules.perpendicular, rules.vertical]
    );
}

// ---- view, hit area, blocked input ----------------------------------------------------

/// A drawing of four short Lines at the corners of a wide box: every pair's marker stands far from
/// every other marker and from the labels of the lengths, so a hair outside one box is outside all.
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
fn markers_follow_zoom_pan_and_fit_and_are_hit_exactly_where_they_are_drawn() {
    for pixels_per_point in [1., 2.] {
        let (_root, path, source) = markers_document();
        let mut rig = Rig::over(None, &path, &source, pixels_per_point, spread);
        let rules = rig.rules();
        rig.target(Target::Rule);
        let canvas = rig.rect();
        let opening = rig.canvas().view();
        // The EqualLength marker of Lines 1 and 3 stands in the middle of the spread drawing and has no
        // neighbour.
        let first = marker(&rig.frame(vec![]), "Eq");
        let r = first.rect;
        assert!(band(canvas).contains_rect(r), "{first:?}");
        let middle_of_middles = |rig: &Rig| rig.mid(0) + (rig.mid(2) - rig.mid(0)) * 0.5;
        assert!(near(r.center(), middle_of_middles(&rig)));

        // The hit area is the drawn box: just inside every corner takes the marker, just outside
        // takes nothing.
        let nudge = |x: f32, y: f32| egui::vec2(x, y);
        for at in [
            r.min + nudge(0.5, 0.5),
            r.max - nudge(0.5, 0.5),
            r.left_bottom() + nudge(0.5, -0.5),
            r.right_top() + nudge(-0.5, 0.5),
            r.center(),
        ] {
            rig.e.draft.as_mut().expect("draft").mark = None;
            rig.click(at);
            assert_eq!(
                rig.mark(),
                Some(rules.equal),
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
            rig.e.draft.as_mut().expect("draft").mark = None;
            rig.click(at);
            assert_eq!(rig.mark(), None, "{pixels_per_point}x outside at {at:?}");
        }

        // Zoom out about a point above the box: the Lines move, and the marker with them, to the
        // middle of their new middles, still whole inside the band; the old place takes nothing.
        let before = middle_of_middles(&rig);
        let about = egui::pos2(before.x, canvas.top() + 30.);
        rig.frame(vec![moved(about)]);
        rig.frame(vec![moved(about), wheel(-40.)]);
        let after = middle_of_middles(&rig);
        assert_ne!(before, after);
        let zoomed = marker(&rig.frame(vec![]), "Eq");
        assert!(band(canvas).contains_rect(zoomed.rect), "{zoomed:?}");
        assert!(
            near(zoomed.rect.center(), after),
            "the marker did not move with its Lines"
        );
        rig.e.draft.as_mut().expect("draft").mark = None;
        rig.click(zoomed.rect.center());
        assert_eq!(
            rig.mark(),
            Some(rules.equal),
            "the box where it is drawn now"
        );

        // Fit gives the opening place back exactly; a pan moves the marker with the Lines until
        // the band holds it.
        rig.press("Fit");
        assert_eq!(marker(&rig.frame(vec![]), "Eq").rect, first.rect);
        let grab = canvas.center() + egui::vec2(0., 30.);
        rig.frame(vec![moved(grab), middle(grab, true)]);
        rig.frame(vec![moved(grab + egui::vec2(-20., 0.))]);
        rig.frame(vec![middle(grab + egui::vec2(-20., 0.), false)]);
        let panned = marker(&rig.frame(vec![]), "Eq");
        assert!(
            near(panned.rect.min, first.rect.min + egui::vec2(-20., 0.)),
            "{panned:?}"
        );
        assert_eq!(panned.rect.min.y, first.rect.min.y);

        // Far in, the middle of the middles is off the canvas: no marker, no hit, and the list
        // still reaches the rule. Fit returns the marker where it was.
        rig.press("Fit");
        for _ in 0..6 {
            rig.frame(vec![
                moved(canvas.left_top() + egui::vec2(4., 4.)),
                wheel(120.),
            ]);
        }
        assert!(!has_marker(&rig.frame(vec![]), "Eq"), "{pixels_per_point}x");
        rig.e.draft.as_mut().expect("draft").mark = None;
        rig.click(first.rect.center());
        assert_eq!(rig.mark(), None, "nothing is there");
        rig.press_rule_row(&row(Kind::EqualLength, rules.equal));
        assert_eq!(rig.mark(), Some(rules.equal), "the list reaches it");
        rig.press("Fit");
        assert_eq!(rig.canvas().view(), opening);
        assert_eq!(marker(&rig.frame(vec![]), "Eq").rect, first.rect);
        assert!(rig.e.take_request().is_none());
    }
}

#[test]
fn a_marker_press_dies_with_its_surface_and_busy_queued_covered_or_unfocused_forms_take_nothing() {
    for pixels_per_point in [1., 2.] {
        let ready = || {
            let (_root, path, source) = markers_document();
            let mut rig = Rig::over(None, &path, &source, pixels_per_point, spread);
            rig.target(Target::Rule);
            (rig, _root)
        };
        // A press begun on an available marker is no permission for a later release, however the
        // surface came back before it: focus lost, another layer over the canvas, and that layer
        // gone again.
        for interruption in ["focus", "covered", "uncovered"] {
            let (mut held, _keep) = ready();
            let before = held.facts3();
            let at = held.at_marker("Col");
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
                held.facts3(),
                before,
                "{pixels_per_point}x: a release after {interruption} took a marker"
            );
            assert!(!held.canvas().pressed());
        }

        // A release just outside the canvas, within a click's distance of the press, takes nothing:
        // pan the Lines until the marker hugs the canvas's left edge.
        let (mut edge, _keep) = ready();
        let canvas = edge.rect();
        let grab = canvas.center() + egui::vec2(0., 30.);
        let drag = egui::vec2(canvas.left() + 10. - edge.at_marker("V").x, 0.);
        edge.frame(vec![moved(grab), middle(grab, true)]);
        edge.frame(vec![moved(grab + drag)]);
        edge.frame(vec![middle(grab + drag, false)]);
        let hugging = marker(&edge.frame(vec![]), "V");
        assert!(
            hugging.rect.left() < canvas.left() + 3.5,
            "{hugging:?} on {canvas:?}"
        );
        let before = edge.facts3();
        let at = hugging.rect.left_center() + egui::vec2(0.2, 0.);
        let outside = egui::pos2(canvas.left() - 0.8, at.y);
        assert!(!canvas.contains(outside) && at.distance(outside) <= 6.);
        edge.frame(vec![moved(at), primary(at, true)]);
        edge.frame(vec![moved(outside), primary(outside, false)]);
        assert_eq!(
            edge.facts3(),
            before,
            "{pixels_per_point}x: release outside"
        );
        assert!(!edge.canvas().pressed());

        // Escape, a lost focus and a vanished pointer end a press without a trace.
        for ending in ["escape", "focus", "gone"] {
            let (mut rig, _keep) = ready();
            let before = rig.facts3();
            let at = rig.at_marker("Col");
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
            assert_eq!(rig.facts3(), before, "{pixels_per_point}x: {ending}");
        }

        // A shift press on a marker is a pan's: it never takes the marker, moving or not.
        let (mut rig, _keep) = ready();
        let before = rig.facts3();
        let at = rig.at_marker("Col");
        rig.frame(vec![moved(at), shift_primary(at, true)]);
        rig.frame(vec![shift_primary(at, false)]);
        assert_eq!(
            rig.facts3(),
            before,
            "{pixels_per_point}x: a shift click took a marker"
        );

        // What is picked is what is drawn: with another layer over the right half of the canvas, a
        // marker under it is not reached, one outside it is, and the same press is a press only
        // where nothing covers it.
        let (mut half, _keep) = ready();
        let canvas = half.rect();
        let cover = egui::Rect::from_min_max(
            egui::pos2(canvas.center().x, canvas.top()),
            canvas.right_bottom(),
        );
        let (left, right) = (half.at_marker("Col"), half.at_marker("Par"));
        assert!(
            (cover.contains(right) && !cover.contains(left))
                || (cover.contains(left) && !cover.contains(right)),
            "the cover splits the two markers: {cover:?} {left:?} {right:?}"
        );
        for _ in 0..3 {
            half.frame_as(vec![], false, true, Some(cover));
        }
        let (free, covered, free_rule) = if cover.contains(right) {
            (left, right, half.rules().collinear)
        } else {
            (right, left, half.rules().parallel)
        };
        half.frame_as(
            vec![moved(covered), primary(covered, true)],
            false,
            true,
            Some(cover),
        );
        half.frame_as(vec![primary(covered, false)], false, true, Some(cover));
        assert_eq!(
            half.mark(),
            None,
            "{pixels_per_point}x: a covered marker was reached"
        );
        half.frame_as(
            vec![moved(free), primary(free, true)],
            false,
            true,
            Some(cover),
        );
        half.frame_as(vec![primary(free, false)], false, true, Some(cover));
        assert_eq!(half.mark(), Some(free_rule), "{pixels_per_point}x");
    }

    // Busy, unfocused, covered, outside and queued forms: the marker, the button and the action take
    // nothing, and take it again as soon as nothing blocks them.
    let mut rig = Rig::markers(1.);
    let rules = rig.rules();
    rig.choose_mark("Par");
    let base = rig.facts3();
    let (aim, other) = (rig.at_marker("Par"), rig.at_marker("V"));
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
    assert_eq!(rig.facts3(), base, "busy");
    attempt(&mut rig, false, false, None);
    assert_eq!(rig.facts3(), base, "unfocused");
    let canvas = rig.rect();
    for _ in 0..3 {
        rig.frame_as(vec![], false, true, Some(canvas));
    }
    attempt(&mut rig, false, true, Some(canvas));
    assert_eq!(rig.facts3(), base, "covered");
    for _ in 0..3 {
        rig.frame(vec![]);
    }
    // The action takes no press while a job runs, though the form has the focus it had.
    click_running(&rig.ctx, &mut rig.e, "Remove constraint", true);
    assert_eq!(rig.edits(), SketchConstraintEdits::default());
    // Prepared once, a request of the form waits to be taken: nothing on the canvas and no action
    // begins.
    rig.press("Remove constraint");
    let prepared = rig.edits();
    assert_eq!(prepared.remove, vec![rules.parallel]);
    rig.press("Save constraints copy…");
    assert!(rig.e.pending.is_some());
    let queued = rig.facts3();
    attempt(&mut rig, false, true, None);
    rig.press("Remove constraint");
    press_exact(&mut rig, "Rule");
    rig.press("Fit");
    assert_eq!(rig.facts3(), queued, "queued");
    assert_eq!(
        rig.e.take_request().expect("the waiting request").edits,
        prepared
    );
    // Nothing blocks any more: the other marker is taken and its rule removed.
    rig.click(other);
    assert_eq!(rig.mark(), Some(rules.vertical));
    rig.settle();
    rig.press("Remove constraint");
    assert_eq!(rig.edits().remove, vec![rules.parallel, rules.vertical]);
}

// ---- overlaps, paint order, the 768-point screen, no drawing, other forms ------------

/// A drawing in which every Line runs through one point: the middles of all the Lines coincide, so
/// every pair's marker stands on that point and the Lines' own markers and labels stand beside it.
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
fn overlapping_markers_are_reached_in_saved_order_the_target_decides_what_is_on_top_and_the_list_reaches_the_one_underneath()
 {
    let (_root, path, source) = markers_document();
    let mut rig = Rig::over(None, &path, &source, 1., crossing);
    let rules = rig.rules();
    let ids = rig.ids.clone();
    // The EqualLength, the Parallel, the Perpendicular and the Collinear name four different pairs
    // of Lines, and on this drawing every pair's marker stands at the one point the Lines cross.
    rig.target(Target::Rule);
    let out = rig.frame(vec![]);
    let (eq, par, perp, col) = (
        marker(&out, "Eq").rect,
        marker(&out, "Par").rect,
        marker(&out, "Perp").rect,
        marker(&out, "Col").rect,
    );
    let centre = rig.mid(0);
    for r in [eq, par, perp, col] {
        assert!(near(r.center(), centre), "{r:?} {centre:?}");
    }
    // The first of them in the order the rules are stored is the one reached, whatever the order
    // of their UUIDs says. Stored order: ... EqualLength, Parallel, Perpendicular, Collinear.
    rig.click(centre);
    assert_eq!(rig.mark(), Some(rules.equal), "stored first, not by UUID");
    // The persisted rows reach each one of the others, whose Lines are then the marked ones.
    for (kind, rule, lines) in [
        (Kind::Parallel, rules.parallel, [1, 2]),
        (Kind::Collinear, rules.collinear, [0, 3]),
        (Kind::Perpendicular, rules.perpendicular, [2, 3]),
        (Kind::EqualLength, rules.equal, [0, 2]),
    ] {
        rig.press_rule_row(&row(kind, rule));
        rig.settle();
        assert_eq!(rig.mark(), Some(rule), "{kind:?}");
        let marked = selected_strokes(&rig.frame(vec![]));
        for i in 0..4 {
            let ends = rig.ends(i);
            assert_eq!(
                marked
                    .iter()
                    .any(|s| near(s[0], ends[0]) && near(s[1], ends[1])),
                lines.contains(&i),
                "{kind:?}: Line {}",
                i + 1
            );
        }
    }
    // What the pointer shows is the top marker's: hovering the shared place marks the top rule's
    // two Lines (EqualLength: Lines 1 and 3).
    let out = rig.frame(vec![moved(centre)]);
    let yellow = Rig::strokes(&out, HOVER_COLOUR);
    for i in [0, 2] {
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
            .any(|s| near(s[0], rig.ends(1)[0]) && near(s[1], rig.ends(1)[1])),
        "only the top marker's Lines"
    );

    // The target decides which family is on top, so what is picked is what is drawn. The length of
    // Line 1 stands above the crossing: where its label meets a marker, Dimension reaches the label
    // and Rule the marker, and each is painted last (on top) under the target that reaches it.
    let draw_order = |out: &egui::FullOutput, rect: egui::Rect, marker_fill: bool| {
        out.shapes
            .iter()
            .position(|s| match &s.shape {
                egui::Shape::Rect(r) if r.rect == rect => is_marker_fill(r.fill) == marker_fill,
                _ => false,
            })
            .unwrap_or_else(|| panic!("no box {rect:?}"))
    };
    let length = label(&out, "60 mm").rect;
    let mut shared = None;
    for m in markers(&out) {
        if m.rect.intersects(length) {
            shared = Some(m);
            break;
        }
    }
    let shared = shared.expect("a marker meets the label of the length of Line 1");
    let at = shared.rect.intersect(length).center();
    let rule_of_shared = rig
        .e
        .draft_marks()
        .expect("draft")
        .0
        .into_iter()
        .find(|m| m.kind.short() == shared.text)
        .expect("held")
        .id;
    let _ = rule_of_shared;
    rig.target(Target::Rule);
    let under_rule = rig.frame(vec![]);
    assert!(
        draw_order(&under_rule, shared.rect, true) > draw_order(&under_rule, length, false),
        "under Rule the marker is painted over the label"
    );
    rig.e.draft.as_mut().expect("draft").mark = None;
    rig.click(at);
    assert!(
        rig.mark().is_some(),
        "Rule reaches the marker where they meet"
    );
    assert_eq!(rig.picks().0, None);
    rig.target(Target::Dimension);
    let under_dimension = rig.frame(vec![]);
    assert!(
        draw_order(&under_dimension, shared.rect, true)
            < draw_order(&under_dimension, length, false),
        "under Dimension the label is painted over the marker"
    );
    rig.click(at);
    assert_eq!(
        (rig.mark(), rig.picks().0),
        (None, Some(ids[0])),
        "Dimension reaches the label where they meet"
    );
}

/// The 64-gon of the §31K gate with a Horizontal of the last Line, a stored length and every
/// closure link, and a Vertical on the one before the last (so a marker is selected on a full list).
fn polygon_with_markers() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
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
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: ids[ids.len() - 2],
                    kind: LineConstraintKind::Vertical,
                }),
                AddSketchConstraint::Line(AddLineConstraint::EqualLength {
                    a: ids[0],
                    b: ids[1],
                }),
            ],
        },
    )
    .expect("prepare");
    document.write_sketch_constraints(&prepared).expect("rules");
    let source = ExtrudeEditSource::read(&document).expect("catalogue");
    document.close().expect("close");
    (root, path, source)
}

#[test]
fn the_markers_the_editor_and_the_actions_stay_reachable_at_768_points_in_the_worst_forms() {
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
        "Rule",
        "Fit",
        "Remove constraint",
    ];
    let long_refusal = "enter finite centre coordinates in mm — and so on, a refusal worded at such \
                        length that it must wrap inside the form instead of leaving it";
    let (_a, plate_path, plate) = markers_document();
    let (_b, polygon_path, polygon) = polygon_with_markers();
    let mut report = Vec::new();
    for (name, path, source, words) in [
        (
            "plate with seven rules and two lengths",
            plate_path,
            plate,
            "Par",
        ),
        (
            "64-gon with every closure link, a length and two rules",
            polygon_path,
            polygon,
            "Eq",
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
                    .enumerate()
                    .filter(|(i, _)| i % 2 == 0 && (4..62).contains(i))
                    .map(|(_, c)| {
                        AddSketchConstraint::Line(AddLineConstraint::Line {
                            curve: c.id,
                            kind: LineConstraintKind::Horizontal,
                        })
                    })
                    .collect();
                draft.choice.validate_edits(&draft.edits).expect("valid");
            }
            rig.choose_mark(words);
            // A removal is prepared, so that Apply and Save are offered.
            rig.press("Remove constraint");
            assert!(rig.edits().remove.len() == 1, "{name}: no removal");
            if extras {
                // A long text in a field and a long refusal under it. The fully loaded 64-gon is as
                // the 31K, 31L and 31M gates left it: no room for a refusal line even without the
                // canvas.
                enter_field(
                    &rig.ctx,
                    &mut rig.e,
                    "Fixed X (mm):",
                    "1234567890123456789012345678901",
                    false,
                );
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
            let boxes = markers(&out);
            assert!(!boxes.is_empty(), "{name}: no marker");
            for m in &boxes {
                assert!(
                    screen.contains_rect(m.rect) && rig.rect().contains_rect(m.rect),
                    "{name}: marker {m:?} is outside the canvas or the screen"
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
            eprintln!("FORM_WIDTH_31N {name}: widest text ends at {right:.1}");
            bottoms.push(bottom);
        }
        report.push(format!(
            "{name}: Save bottom {:.1} with a rule selected and its removal prepared, {:.1} with a long text, a refusal and the unsaved note",
            bottoms[0], bottoms[1]
        ));
    }
    for line in &report {
        eprintln!("FORM_BUDGET_31N {line}");
    }
}

#[test]
fn without_a_drawing_the_rule_rows_still_select_and_remove_and_closure_and_other_profiles_have_no_marker()
 {
    let (_root, path, source) = markers_document();
    let stored = source.constraint_sketches[0]
        .stored
        .clone()
        .expect("stored");
    let rules = rules_of(&stored);
    // The form with no accepted drawing: no canvas, no markers, no Rule target; the list of
    // persisted rules selects and removes as it does with one.
    let mut with = Rig::over(None, &path, &source, 1., enlarged);
    with.choose_mark("Par");
    with.press("Remove constraint");
    let mut without = Rig::over(None, &path, &source, 1., |_| {
        Drawing::Unavailable(crate::sketch::accepted::Unavailable::NotHanded)
    });
    let out = without.frame(vec![]);
    assert!(markers(&out).is_empty() && !painted(&out, "Rule") && !painted(&out, "Fit"));
    without.press_rule_row(&row(Kind::Parallel, rules.parallel));
    without.settle();
    assert_eq!(
        (without.mark(), without.picks().0),
        (Some(rules.parallel), None)
    );
    without.reach_remove_constraint();
    without.press("Remove constraint");
    assert_eq!(without.edits(), with.edits());
    assert_eq!(without.edits().remove, vec![rules.parallel]);
    // A form held on another version loses its drawing and keeps its selection, its rows and text.
    let mut held = Rig::over(None, &path, &source, 1., enlarged);
    held.choose_mark("Col");
    assert!(!markers(&held.frame(vec![])).is_empty());
    enter_field(&held.ctx, &mut held.e, "Fixed X (mm):", "4", false);
    held.e.retire_drawing();
    held.settle();
    let out = held.frame(vec![]);
    assert!(markers(&out).is_empty() && !painted(&out, "Rule"));
    assert_eq!(
        held.mark(),
        Some(rules.collinear),
        "the selection of a held form is kept"
    );
    assert_eq!(held.e.draft_inputs().expect("draft")[3], "4");
    held.reach_remove_constraint();
    held.press("Remove constraint");
    assert_eq!(held.edits().remove, vec![rules.collinear]);
    assert_eq!(held.refusal(), None);

    // A Line closure is nothing a person edits: no row of it is selectable and it has no Remove.
    let closure = stored
        .constraints
        .iter()
        .filter(|c| matches!(c.rule, SketchConstraintRule::Coincident { .. }))
        .count();
    assert_eq!(closure, 4, "the plate holds its four closure links");
    let mut rig = Rig::over(None, &path, &source, 1., enlarged);
    let out = rig.frame(vec![]);
    assert_eq!(
        out.shapes
            .iter()
            .filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().starts_with("Coincident closure")))
            .count()
            .min(1),
        1
    );
    let held_marks = rig.e.draft_marks().expect("draft").0;
    assert!(
        stored
            .constraints
            .iter()
            .filter(|c| matches!(c.rule, SketchConstraintRule::Coincident { .. }))
            .all(|c| held_marks.iter().all(|m| m.id != c.id)),
        "a closure link has no marker"
    );
    // A request that removes one is refused by the document's own rule, not by a list of ours.
    let link = stored
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::Coincident { .. }))
        .expect("a closure link")
        .id;
    let draft = rig.e.draft.as_ref().expect("draft");
    assert!(
        draft
            .choice
            .validate_edits(&SketchConstraintEdits {
                remove: vec![link],
                add: vec![]
            })
            .is_err()
    );

    // An annulus and a mixed profile keep their refusal and their lists: no marker, no Rule.
    let (_ring, ring_path, ring, _) = super::diameter::round_fixture(&[10., 4.]);
    let mut rig = Rig::over(None, &ring_path, &ring, 1., |_| {
        Drawing::Unavailable(crate::sketch::accepted::Unavailable::Profile)
    });
    assert!(rig.e.draft_marks().expect("draft").0.is_empty());
    assert!(markers(&rig.frame(vec![])).is_empty());
}

#[test]
fn two_forms_over_one_document_keep_their_own_rule_selection_and_pending_removals() {
    // Copies of a document share their curve and rule UUIDs; the forms must not.
    let (_root, path, source) = markers_document();
    let rules = rules_of(
        &source.constraint_sketches[0]
            .stored
            .clone()
            .expect("stored"),
    );
    let mut a = Rig::over(None, &path, &source, 1., enlarged);
    let mut b = Rig::over(None, &path, &source, 1., enlarged);
    a.choose_mark("Par");
    a.press("Remove constraint");
    let in_a = (a.mark(), a.edits());
    assert_eq!(in_a.0, Some(rules.parallel));
    // B has not been touched by any of it.
    assert_eq!(
        (b.mark(), b.edits()),
        (None, SketchConstraintEdits::default())
    );
    let out = b.frame(vec![]);
    assert!(has_marker(&out, "Par") && !has_marker(&out, "Par -> removed"));
    b.choose_mark("H");
    b.press("Remove constraint");
    assert_eq!(b.edits().remove, vec![rules.horizontal]);
    // A is what it was, and B's marker says what B prepared.
    assert_eq!((a.mark(), a.edits()), in_a);
    assert!(has_marker(&b.frame(vec![]), "H -> removed") && has_marker(&b.frame(vec![]), "Par"));
    assert!(has_marker(&a.frame(vec![]), "Par -> removed") && has_marker(&a.frame(vec![]), "H"));
}

#[test]
fn the_target_row_keeps_five_targets_and_fit_beside_rule_in_one_row_and_dimension_keeps_its_meaning()
 {
    let mut rig = Rig::markers(1.);
    let out = rig.frame(vec![]);
    // One row holds the six targets and Fit, in this order, on one line.
    let words = [
        "Object",
        "Endpoint",
        "Pair A",
        "Pair B",
        "Dimension",
        "Rule",
        "Fit",
    ];
    let at: Vec<_> = words
        .iter()
        .map(|w| {
            out.shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == *w => Some(t.visual_bounding_rect()),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("not painted: {w}"))
        })
        .collect();
    for pair in at.windows(2) {
        assert!(
            pair[0].right() < pair[1].left(),
            "{pair:?}: in one row, in order"
        );
        assert!(
            (pair[0].center().y - pair[1].center().y).abs() < 1.5,
            "{pair:?}: on one line"
        );
    }
    let canvas = rig.rect();
    let row_width = at[6].right() - at[0].left();
    eprintln!(
        "TARGET_ROW_31N: {row_width:.1} points of words, canvas {:.1}",
        canvas.width()
    );
    assert!(
        at[6].right() <= canvas.left() + 296.,
        "the row took {} points: it must stay inside the width the five targets and Fit had",
        at[6].right() - canvas.left()
    );
    // The Rule target is reachable and the canvas knows it; the older targets are as they were.
    for target in [
        Target::Rule,
        Target::Dimension,
        Target::PairB,
        Target::PairA,
        Target::Endpoint,
        Target::Object,
    ] {
        rig.target(target);
    }
    // A form that holds no marker has the Rule button greyed: pressing it changes no target.
    let mut bare = Rig::new(1.);
    assert!(bare.e.draft_marks().expect("draft").0.is_empty());
    bare.press("Rule");
    assert_eq!(bare.canvas().target(), Target::Object);
    // The row costs the forms around it nothing: a form whose Lines list sits beside the canvas is
    // no taller for the sixth target. The two Revolve forms of the §31K gate are the tight ones (the
    // Lines list wraps its rows in the width the row leaves it); their bottoms there were 620.5 and
    // 635.5 before §31N.
    let dir = tempfile::tempdir().expect("dir");
    for (name, points, axis, limit) in [
        (
            "five-segment sector",
            super::revolve::SECTOR_P.to_vec(),
            None,
            620.5,
        ),
        (
            "six-segment shaft closed on the axis",
            super::revolve::SHAFT_T.to_vec(),
            Some(5),
            635.5,
        ),
    ] {
        let path = dir.path().join(format!("{axis:?}.fcad"));
        super::revolve::write_sector(&path, &points, 137.5, axis);
        let source = super::revolve::reading(&path);
        let choice = &source.constraint_sketches[0];
        let stored = choice.stored.clone().expect("stored");
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, choice.sketch));
        e.show_drawing(enlarged(&stored));
        e.set_session(true, true, false);
        let draft = e.draft.as_mut().expect("draft");
        draft.refusal = Some("Enter a positive radius in mm.".to_owned());
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
        let ctx = egui::Context::default();
        for _ in 0..4 {
            frame(&ctx, &mut e, vec![]);
        }
        let out = frame(&ctx, &mut e, vec![]);
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
        eprintln!("FORM_BUDGET_31N {name}: Save bottom {bottom:.1} with the Rule target");
        assert!(
            bottom <= limit,
            "{name}: Save ends at {bottom}, it ended at {limit} before the Rule target"
        );
    }
    // Dimension reaches numeric labels only: the label of a length selects its Line, the marker
    // beside it nothing; Rule reaches markers only, and the label nothing.
    let mut rig = Rig::markers(1.);
    let ids = rig.ids.clone();
    let rules = rig.rules();
    rig.target(Target::Dimension);
    let at = rig.at_label("60 mm");
    rig.click(at);
    assert_eq!((rig.picks().0, rig.mark()), (Some(ids[0]), None));
    rig.target(Target::Rule);
    let at = rig.at_label("30 mm");
    rig.click(at);
    assert_eq!(rig.mark(), None, "Rule does not reach a numeric label");
    assert_eq!(rig.picks().0, Some(ids[0]), "and leaves the Line as it was");
    let at = rig.at_marker("V");
    rig.click(at);
    assert_eq!((rig.picks().0, rig.mark()), (None, Some(rules.vertical)));
}

// ---- a single Circle -------------------------------------------------------------------

/// A cylinder that stores a circle of `stored` mm and persists a Radius and a Fixed centre.
fn circle_with_fixed_centre() -> (
    tempfile::TempDir,
    PathBuf,
    ExtrudeEditSource,
    StableEntityId,
    StableEntityId,
) {
    let (root, path, source, ids) = super::diameter::round_fixture(&[1.2]);
    let mut document = Document::open(&path).expect("document");
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        source.constraint_sketches[0].sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                AddSketchConstraint::Circle {
                    curve: ids[0],
                    kind: CircleConstraintKind::Radius(CircleRadiusMm::new(10.).expect("radius")),
                },
                AddSketchConstraint::Circle {
                    curve: ids[0],
                    kind: CircleConstraintKind::FixedCenter {
                        x: SketchCoordinateMm::new(2.5).expect("x"),
                        y: SketchCoordinateMm::new(-1.25).expect("y"),
                    },
                },
            ],
        },
    )
    .expect("prepare");
    let (radius, fixed) = (prepared.added[0].id, prepared.added[1].id);
    document.write_sketch_constraints(&prepared).expect("rules");
    let source = ExtrudeEditSource::read(&document).expect("catalogue");
    document.close().expect("close");
    (root, path, source, radius, fixed)
}

#[test]
fn a_single_circles_fixed_centre_is_a_marker_beside_its_radius_label_and_marks_the_centre() {
    for pixels_per_point in [1., 2.] {
        let (_root, path, source, radius, fixed) = circle_with_fixed_centre();
        let mut rig = Rig::over(
            None,
            &path,
            &source,
            pixels_per_point,
            super::canvas::moved_circle,
        );
        let out = rig.frame(vec![]);
        // The Radius is a label, the Fixed centre a marker: neither is the other.
        assert_eq!(
            labels(&out)
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>(),
            ["R 10 mm"]
        );
        assert_eq!(
            markers(&out)
                .iter()
                .map(|m| m.text.as_str())
                .collect::<Vec<_>>(),
            ["Fix"]
        );
        assert_eq!(
            rig.e.draft_marks().expect("draft").0,
            vec![Mark {
                id: fixed,
                kind: Kind::Fixed,
                refs: Refs::Point(ferritecad_document::SketchPointRef::new(
                    rig.ids[0],
                    ferritecad_document::SketchPointSelector::Center
                )),
            }]
        );
        // It stands beside the centre of the SOLVED circle and never on it.
        let circle = match rig.solved()[0].shape {
            Shape::Circle { center, .. } => center,
            other => panic!("{other:?}"),
        };
        let centre = rig.screen(circle);
        let fix = marker(&out, "Fix");
        assert!(
            !fix.rect.contains(centre)
                && fix.rect.center().x > centre.x
                && fix.rect.center().y > centre.y,
            "{fix:?} for the centre {centre:?}"
        );
        // Under Rule it selects the rule, marks the centre and names it, and the Circle is the
        // only curve in the selection blue.
        rig.target(Target::Rule);
        rig.click(fix.rect.center());
        rig.settle();
        assert_eq!((rig.mark(), rig.picks().0), (Some(fixed), None));
        let out = rig.frame(vec![]);
        assert!(point_in(&circles(&out, SELECTED_COLOUR, 5., true), centre));
        assert!(painted(&out, "centre"));
        assert!(painted_containing(
            &out,
            &format!("Selected rule {} · Fixed", short(fixed))
        ));
        // The Radius is the other dimension: the Dimension target reaches its label, which gives
        // the rule up and selects the Circle.
        rig.target(Target::Dimension);
        let at = rig.at_label("R 10 mm");
        rig.click(at);
        assert_eq!((rig.mark(), rig.picks().0), (None, Some(rig.ids[0])));
        rig.press_rule_row(&format!("Fixed centre (2.5, -1.25) mm · {fixed}"));
        assert_eq!(rig.mark(), Some(fixed));
        rig.settle();
        rig.press("Remove constraint");
        assert_eq!(rig.edits().remove, vec![fixed]);
        assert!(rig.edits().add.is_empty());
        let _ = radius;
        assert!(has_marker(&rig.frame(vec![]), "Fix -> removed"));
    }
}

// ---- native: the real accepted drawing, the real worker -------------------------------

use super::canvas::{accepted_scene, by_lists, line_numbers, presented, read_stl_of};

/// What the form removed and what it kept, for the reader of the saved result: the UUID of the
/// removed rule, the UUID of a rule that must still be there under the very same UUID, and the
/// degrees of freedom the solver reported for the pristine source.
pub(crate) type Removal = (StableEntityId, StableEntityId, usize);

/// The degrees of freedom the solver reports for the Sketch of a document, from a cold rebuild:
/// the solver's own answer, not a number read back from anything the document stores.
fn solver_dof(path: &Path, sketch: ObjectId) -> usize {
    let document = Document::open_read_only(path).expect("document");
    let built = ferritecad_eval::rebuild_cold(
        &document,
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &OperationContext::default(),
    )
    .expect("cold rebuild");
    built
        .solve_report(sketch)
        .expect("the Sketch has a solve report")
        .degrees_of_freedom()
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

/// A rectangle whose Sketch STORES a rough guess (a 12 × 8 mm parallelogram about the origin) and
/// keeps the rules that solve it into a different shape: Line 1's start fixed at the origin, Line 1
/// horizontal and 60 mm, Line 2 vertical and 30 mm, and Lines 1 and 3 of **equal length** and
/// **parallel** (the EqualLength stored before the Parallel, so the two markers stand on one place
/// and a click reaches the EqualLength). Solved: (0,0) (60,0) (60,30) (0,30), no degree of freedom
/// left. Remove the EqualLength and the solver has one: the free corner stays where the Sketch
/// stored it, (-5, 4) slid onto the line y = 30 of the Parallel, a 65 mm Line 3.
pub(crate) fn write_marked_rectangle(path: &Path) {
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
                line(
                    0,
                    LineConstraintKind::Distance(LineLengthMm::new(60.).expect("60")),
                ),
                line(1, LineConstraintKind::Vertical),
                line(
                    1,
                    LineConstraintKind::Distance(LineLengthMm::new(30.).expect("30")),
                ),
                AddSketchConstraint::Line(AddLineConstraint::EqualLength {
                    a: ids[0],
                    b: ids[2],
                }),
                AddSketchConstraint::Line(AddLineConstraint::Relation {
                    a: ids[0],
                    b: ids[2],
                    relation: LineRelation::Parallel,
                }),
            ],
        },
    )
    .expect("prepare");
    document.write_sketch_constraints(&prepared).expect("rules");
    document.close().expect("close");
}

fn find_rule(stored: &Sketch, want: fn(&SketchConstraintRule) -> bool) -> StableEntityId {
    let all: Vec<_> = stored
        .constraints
        .iter()
        .filter(|c| want(&c.rule))
        .collect();
    assert_eq!(all.len(), 1, "one rule of the family");
    all[0].id
}

/// §31N, native: the form for the rectangle written by `write_marked_rectangle`, driven by the real
/// widgets over the real accepted drawing. The markers stand by the SOLVED rectangle (not the stored
/// guess); the EqualLength and the Parallel of Lines 1 and 3 stand on one place, where a click under
/// the Rule target reaches the EqualLength (stored first) and the Parallel is reached by its row;
/// the EqualLength marks Lines 1 and 3 and its removal is prepared by the one explicit action; the
/// request equals the one the form's own road makes (the Remove tick of the row). Returns the form
/// with its Apply asked, as the session gate takes it.
pub(crate) fn native_marked_rectangle(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    seen: &std::cell::Cell<Option<Removal>>,
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut rig, loaded) = Rig::native(path, source, sketch);
    let real = presented(&loaded, sketch);
    let stored = rig.stored.clone();
    let equal = find_rule(&stored, |r| {
        matches!(r, SketchConstraintRule::EqualLength { .. })
    });
    let parallel = find_rule(&stored, |r| {
        matches!(r, SketchConstraintRule::Parallel { .. })
    });
    // The form's drawing IS the rebuild's presentation: the rectangle the rules ask for. The
    // stored guess is another shape altogether.
    let held = rig.solved();
    assert_eq!(held, curves_of(&real), "the drawing is the rebuild's");
    let corners = [[0., 0.], [60., 0.], [60., 30.], [0., 30.]];
    for (i, c) in held.iter().enumerate() {
        let Shape::Line { start, end } = c.shape else {
            panic!("a Line")
        };
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        assert!(
            (start[0] - a[0]).abs() < 1e-6
                && (start[1] - a[1]).abs() < 1e-6
                && (end[0] - b[0]).abs() < 1e-6
                && (end[1] - b[1]).abs() < 1e-6,
            "Line {} runs {start:?} to {end:?}",
            i + 1
        );
    }
    let guess = Drawing::of(
        &stored,
        stored.curves.iter().map(|c| (c.id, c.geometry.clone())),
    );
    let Drawing::Solved(guess) = guess else {
        panic!("the stored curves draw")
    };
    let apart = |a: &Curve, b: &Curve| match (a.shape, b.shape) {
        (Shape::Line { start: p, .. }, Shape::Line { start: q, .. }) => {
            (p[0] - q[0]).hypot(p[1] - q[1])
        }
        _ => 0.,
    };
    assert!(
        guess.iter().zip(&held).all(|(g, h)| apart(g, h) > 5.),
        "the stored guess must be no part of the solved rectangle"
    );
    // The solver's own count of what is left free: nothing.
    let dof_before = solver_dof(path, sketch);
    assert_eq!(dof_before, 0, "the rectangle is fully constrained");

    // Five markers (the rules that are not lengths) and two labels, standing by the SOLVED Lines.
    let out = rig.frame(vec![]);
    let mut words: Vec<_> = markers(&out).into_iter().map(|m| m.text).collect();
    words.sort();
    assert_eq!(
        words,
        ["Eq", "Fix", "H", "Par", "V"],
        "no marker tells the stored guess"
    );
    let mut numbers: Vec<_> = labels(&out).into_iter().map(|l| l.text).collect();
    numbers.sort();
    assert_eq!(numbers, ["30 mm", "60 mm"]);
    let centre = rig.screen([30., 15.]);
    let (eq, par) = (marker(&out, "Eq").rect, marker(&out, "Par").rect);
    assert!(
        near(eq.center(), par.center()),
        "one pair, one place: {eq:?} {par:?}"
    );
    assert!(
        near(eq.center(), centre),
        "{eq:?} is not at the middle of the solved middles"
    );
    eprintln!(
        "NATIVE_MARKED_RECTANGLE dof_before={dof_before} solved_corners={corners:?} markers={words:?}"
    );

    // Under Object a click on the shared place takes nothing; under Rule it takes the EqualLength,
    // stored first, and nothing else moves; the Parallel beneath is reached by its row.
    rig.click(centre);
    assert_eq!((rig.mark(), rig.picks().0), (None, None));
    rig.target(Target::Rule);
    let before = rig.facts3();
    rig.click(centre);
    rig.settle();
    assert_eq!((rig.mark(), rig.picks().0), (Some(equal), None));
    let mut now = rig.facts3();
    now.1 = before.1;
    assert_eq!(now, before, "a selection is no input, no step, no request");
    rig.press_rule_row(&row(Kind::Parallel, parallel));
    assert_eq!(rig.mark(), Some(parallel));
    rig.click(centre);
    rig.settle();
    assert_eq!(rig.mark(), Some(equal));
    // Lines 1 and 3 of the SOLVED drawing are marked, and no other.
    let out = rig.frame(vec![]);
    let marked = selected_strokes(&out);
    let ends: Vec<_> = (0..4).map(|i| rig.ends(i)).collect();
    for (i, on) in [(0, true), (1, false), (2, true), (3, false)] {
        assert_eq!(
            marked
                .iter()
                .any(|s| near(s[0], ends[i][0]) && near(s[1], ends[i][1])),
            on,
            "Line {}",
            i + 1
        );
    }
    assert_eq!(dots(&out).len(), 2);
    assert_eq!(rings(&out).len(), 2);

    // The removal: one explicit action, one removal and no addition; the marker says so, the
    // drawing is exactly the rebuild's.
    rig.press("Remove constraint");
    let prepared = rig.edits();
    assert_eq!(prepared.remove, vec![equal]);
    assert!(prepared.add.is_empty());
    let out = rig.frame(vec![]);
    assert!(has_marker(&out, "Eq -> removed") && has_marker(&out, "Par"));
    assert_eq!(rig.solved(), held, "a prepared removal draws nothing");
    assert_eq!(rig.refusal(), None);

    // The same removal by the form's own road: the Remove tick on the row of the rule.
    let lists = by_lists(path, source, sketch, |ctx, e| {
        click_remove_on_row(ctx, e, "Equal length");
    });
    assert_eq!(
        prepared, lists.edits,
        "the marker and the form's own road asked for different things"
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
        (1, 0)
    );
    seen.set(Some((equal, parallel, dof_before)));
    println!(
        "\nFCAD_31N_NATIVE_RULE_REQUEST markers=5 stored_vs_solved=1 shared_place=1 removes=1 adds=0 dof_before=0"
    );
    (rig.e, request)
}

/// §31N, native: the SAVED rectangle read four ways that agree. The drawing of its cold rebuild is the
/// rectangle the remaining rules ask for with Line 3 now 65 mm (the free corner where the Sketch
/// stored it); the solver reports one degree of freedom where it reported none; the form of that
/// rebuild holds no EqualLength, keeps the Parallel under the very UUID it had, and the exported STL,
/// read by `read_stl_of` here and not by the writer, is the prism over that quadrilateral.
pub(crate) fn saved_unequal_rectangle(saved: &Path, stl: &[u8], height: f64, was: Removal) {
    let (removed, kept, dof_before) = was;
    let (loaded, scene) = accepted_scene(saved);
    let facts = scene.edit_source.clone().expect("facts");
    let sketch = facts.constraint_sketches[0].sketch;
    let stored = facts.constraint_sketches[0].stored.clone().expect("stored");
    let curves = curves_of(&presented(&loaded, sketch));
    let line = |i: usize| match curves[i].shape {
        Shape::Line { start, end } => (start, end),
        Shape::Circle { .. } => panic!("a Line"),
    };
    let at = |p: [f64; 2], x: f64, y: f64| (p[0] - x).abs() < 1e-3 && (p[1] - y).abs() < 1e-3;
    eprintln!(
        "NATIVE_SAVED_RECTANGLE lines={:?}",
        (0..4).map(line).collect::<Vec<_>>()
    );
    // What is kept: the pin, the horizontal 60, the vertical 30, the Parallel (Line 3 on y = 30).
    assert!(at(line(0).0, 0., 0.) && at(line(0).1, 60., 0.), "Line 1");
    assert!(at(line(1).0, 60., 0.) && at(line(1).1, 60., 30.), "Line 2");
    assert!(
        at(line(2).0, 60., 30.) && (line(2).1[1] - 30.).abs() < 1e-6,
        "Line 3 is parallel to Line 1"
    );
    // What is freed: the corner stays where the Sketch stored it, so Line 3 is no longer 60.
    let third = line(2).0[0] - line(2).1[0];
    assert!(
        (third - 65.).abs() < 1e-3,
        "Line 3 is {third} mm long, not the 65 the free corner leaves"
    );
    for i in 0..4 {
        let (p, q) = (line(i).1, line((i + 1) % 4).0);
        assert!(
            (p[0] - q[0]).abs() < 1e-6 && (p[1] - q[1]).abs() < 1e-6,
            "closed at {i}"
        );
    }
    // The solver says the same: one degree of freedom where there was none.
    assert_eq!(
        solver_dof(saved, sketch),
        dof_before + 1,
        "the solver did not report the freed degree of freedom"
    );
    // The cold form: no EqualLength, the Parallel under the UUID it had, the lengths and the pin.
    assert!(stored.constraints.iter().all(|c| c.id != removed));
    let mut e = Editor::default();
    assert!(e.begin_with(saved, &facts, &scene.sketches, sketch));
    let ctx = egui::Context::default();
    let mut out = frame(&ctx, &mut e, vec![]);
    for _ in 0..3 {
        out = frame(&ctx, &mut e, vec![]);
    }
    let mut words: Vec<_> = markers(&out).into_iter().map(|m| m.text).collect();
    words.sort();
    assert_eq!(words, ["Fix", "H", "Par", "V"]);
    let (marks, _) = e.draft_marks().expect("a draft");
    assert!(
        marks
            .iter()
            .any(|m| m.id == kept && m.kind == Kind::Parallel)
    );
    assert!(
        marks
            .iter()
            .all(|m| m.id != removed && m.kind != Kind::EqualLength)
    );
    // The STL, by a reader that is not the writer: the prism over the quadrilateral that remains.
    let (faces, volume) = read_stl_of(stl);
    let expected = area_of(&curves) * height;
    assert!(
        (volume - expected).abs() <= 1e-4 * expected,
        "the exported solid has volume {volume}, the saved drawing {expected}"
    );
    assert!(
        (expected - 0.5 * (60. + 65.) * 30. * height).abs() <= 1e-3,
        "and the drawing is the trapezoid the rules leave"
    );
    let mut base: Vec<(i64, i64)> = faces
        .iter()
        .flatten()
        .filter(|p| p[2].abs() < 1e-6)
        .map(|p| ((p[0] * 1e3).round() as i64, (p[1] * 1e3).round() as i64))
        .collect();
    base.sort_unstable();
    base.dedup();
    let mut want = vec![(0, 0), (60_000, 0), (60_000, 30_000), (-5_000, 30_000)];
    want.sort_unstable();
    assert_eq!(
        base, want,
        "the solid's base is the quadrilateral the rules leave"
    );
    let top = faces
        .iter()
        .flatten()
        .map(|p| p[2])
        .fold(f64::MIN, f64::max);
    assert!((top - height).abs() < 1e-6);
}

/// §31N, native: the form for the cylinder written by `write_guessed_circle` (the circle (4.5, 0) r 1.2
/// is stored; a Radius of 10 and a Fixed centre at the origin solve it into (0, 0) r 10). The Fixed
/// centre is a marker beside the solved centre, the Radius a label; the marker chosen under the Rule
/// target marks the centre, and its removal equals the form's own road.
pub(crate) fn native_marked_circle(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    seen: &std::cell::Cell<Option<Removal>>,
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut rig, loaded) = Rig::native(path, source, sketch);
    let real = presented(&loaded, sketch);
    let (circle, stored) = (rig.ids[0], rig.stored.clone());
    let fixed = find_rule(&stored, |r| matches!(r, SketchConstraintRule::Fixed { .. }));
    let radius = find_rule(&stored, |r| {
        matches!(r, SketchConstraintRule::Radius { .. })
    });
    let SketchGeometry::Circle { center, radius: r } = real[0].1 else {
        panic!("a Circle")
    };
    assert!(
        center.x.abs() < 1e-6 && center.y.abs() < 1e-6 && (r - 10.).abs() < 1e-6,
        "the solved circle is ({}, {}) r {r}",
        center.x,
        center.y
    );
    let SketchGeometry::Circle {
        center: guess,
        radius: guess_r,
    } = stored.curves[0].geometry
    else {
        panic!("a Circle")
    };
    assert_eq!(
        (guess.x, guess.y, guess_r),
        (4.5, 0., 1.2),
        "the stored guess"
    );
    assert_eq!(
        rig.solved()[0].shape,
        Shape::Circle {
            center: [center.x, center.y],
            radius: r
        },
        "the form holds the rebuild's circle"
    );
    let dof_before = solver_dof(path, sketch);
    assert_eq!(
        dof_before, 0,
        "a radius and a pinned centre leave nothing free"
    );

    let out = rig.frame(vec![]);
    assert_eq!(
        markers(&out)
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>(),
        ["Fix"]
    );
    assert_eq!(
        labels(&out)
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["R 10 mm"]
    );
    let solved_centre = rig.screen([center.x, center.y]);
    let stored_centre = rig.screen([guess.x, guess.y]);
    let fix = marker(&out, "Fix").rect;
    assert!(
        !fix.contains(solved_centre)
            && fix.center().x > solved_centre.x
            && fix.center().y > solved_centre.y,
        "{fix:?} for the solved centre {solved_centre:?}"
    );
    assert!(
        !fix.contains(stored_centre),
        "the marker stands by the solved centre, not the stored one"
    );
    // Under Rule the marker takes the rule; the centre of the SOLVED circle is told.
    rig.target(Target::Rule);
    rig.click(fix.center());
    rig.settle();
    assert_eq!((rig.mark(), rig.picks().0), (Some(fixed), None));
    let out = rig.frame(vec![]);
    assert!(point_in(
        &circles(&out, SELECTED_COLOUR, 5., true),
        solved_centre
    ));
    assert!(painted(&out, "centre"));
    // Removal, by the one explicit action and by the form's own road.
    rig.press("Remove constraint");
    let prepared = rig.edits();
    assert_eq!(prepared.remove, vec![fixed]);
    assert!(prepared.add.is_empty());
    assert!(has_marker(&rig.frame(vec![]), "Fix -> removed"));
    assert_eq!(
        rig.solved()[0].shape,
        Shape::Circle {
            center: [center.x, center.y],
            radius: r
        }
    );
    let lists = by_lists(path, source, sketch, |ctx, e| {
        click_remove_on_row(ctx, e, "Fixed centre (0, 0) mm");
    });
    assert_eq!(
        prepared, lists.edits,
        "the marker and the form's own road asked for different things"
    );
    rig.press("Apply constraints");
    let request = rig.e.take_apply_request().expect("Apply asked");
    assert_eq!(request.edits, lists.edits);
    assert_eq!(
        (request.expected, request.sketch, &request.source),
        (lists.expected, lists.sketch, &lists.source)
    );
    let _ = circle;
    seen.set(Some((fixed, radius, dof_before)));
    println!("\nFCAD_31N_NATIVE_CENTRE_REQUEST stored_vs_solved=1 removes=1 adds=0 dof_before=0");
    (rig.e, request)
}

/// §31N, native: the SAVED cylinder read four ways that agree. The drawing of its cold rebuild is
/// the circle (4.5, 0) r 10 — the centre the Sketch stored, now that nothing pins it — and the Radius
/// still holds; the solver reports two degrees of freedom where it reported none; the cold form has
/// no marker and keeps the Radius under the UUID it had; every rim vertex of the exported STL, read
/// here and not by the writer, is on that circle.
pub(crate) fn saved_unpinned_circle(saved: &Path, stl: &[u8], height: f64, was: Removal) {
    let (removed, kept, dof_before) = was;
    let (loaded, scene) = accepted_scene(saved);
    let facts = scene.edit_source.clone().expect("facts");
    let sketch = facts.constraint_sketches[0].sketch;
    let stored = facts.constraint_sketches[0].stored.clone().expect("stored");
    let SketchGeometry::Circle { center, radius } = presented(&loaded, sketch)[0].1 else {
        panic!("a Circle")
    };
    eprintln!(
        "NATIVE_SAVED_CIRCLE centre=({}, {}) r={radius}",
        center.x, center.y
    );
    assert!(
        (center.x - 4.5).abs() < 1e-6 && center.y.abs() < 1e-6 && (radius - 10.).abs() < 1e-6,
        "({}, {}) r {radius}",
        center.x,
        center.y
    );
    assert_eq!(
        solver_dof(saved, sketch),
        dof_before + 2,
        "the solver did not report the freed centre"
    );
    assert!(stored.constraints.iter().all(|c| c.id != removed));
    assert!(stored.constraints.iter().any(|c| c.id == kept));
    let mut e = Editor::default();
    assert!(e.begin_with(saved, &facts, &scene.sketches, sketch));
    let ctx = egui::Context::default();
    let mut out = frame(&ctx, &mut e, vec![]);
    for _ in 0..3 {
        out = frame(&ctx, &mut e, vec![]);
    }
    assert!(markers(&out).is_empty(), "nothing is pinned any more");
    assert_eq!(
        labels(&out)
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["R 10 mm"]
    );
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
        let d = (p[0] - 4.5).hypot(p[1]);
        assert!(
            (d - radius).abs() < 1e-3,
            "a rim vertex {p:?} is {d} from the centre (4.5, 0)"
        );
    }
}
