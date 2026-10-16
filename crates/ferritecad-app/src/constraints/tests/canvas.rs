// SPDX-License-Identifier: MIT
//! §31K: the canvas of the accepted solved drawing in the constraints form, through the
//! form's real widgets and real egui events.
//!
//! The drawings here are built from the app's own `Curve` type over the stored curves'
//! identities, deliberately moved and enlarged so that stored and solved differ: they are
//! **synthetic**, they test what the form does with a drawing, not what a rebuild draws.
//! The real presentations are read by the scene gates (`main.rs`) and the native gates
//! below, which need the kernel and the solver.
#![allow(clippy::panic)]
use super::*;
use crate::sketch::accepted::{
    Curve, Drawing, Part, SELECTED_COLOUR, SIDE_A_COLOUR, SIDE_B_COLOUR, Shape, Target, Unavailable,
};

/// The form on a four-Line plate with a synthetic solved drawing, in one egui context.
pub(super) struct Rig {
    pub(super) _root: Option<tempfile::TempDir>,
    pub(super) stored: Sketch,
    pub(super) ids: Vec<StableEntityId>,
    pub(super) e: Editor,
    pub(super) ctx: egui::Context,
}

/// The stored Sketch drawn twice as large about its centre, one curve for one curve.
pub(crate) fn enlarged(stored: &Sketch) -> Drawing {
    let points: Vec<_> = stored
        .curves
        .iter()
        .flat_map(|c| match c.geometry {
            SketchGeometry::Line { start, end } => vec![start, end],
            _ => Vec::new(),
        })
        .collect();
    let n = points.len() as f64;
    let (cx, cy) = (
        points.iter().map(|p| p.x).sum::<f64>() / n,
        points.iter().map(|p| p.y).sum::<f64>() / n,
    );
    let grow = |p: ferritecad_document::Point2| ferritecad_document::Point2 {
        x: cx + 2.0 * (p.x - cx),
        y: cy + 2.0 * (p.y - cy),
    };
    Drawing::of(
        stored,
        stored.curves.iter().map(|c| {
            let SketchGeometry::Line { start, end } = c.geometry else {
                panic!("a Line polygon")
            };
            (
                c.id,
                SketchGeometry::Line {
                    start: grow(start),
                    end: grow(end),
                },
            )
        }),
    )
}

impl Rig {
    pub(super) fn new(pixels_per_point: f32) -> Self {
        let (root, path, source) = fixture();
        let choice = &source.constraint_sketches[0];
        let stored = choice.stored.clone().expect("stored");
        let ids: Vec<_> = stored.curves.iter().map(|c| c.id).collect();
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, choice.sketch));
        e.show_drawing(enlarged(&stored));
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(pixels_per_point);
        let mut rig = Self {
            _root: Some(root),
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

    pub(super) fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        frame_running(&self.ctx, &mut self.e, events, false)
    }

    /// A frame as the window runs it, with a job's state, the focus and a cover given.
    pub(super) fn frame_as(
        &mut self,
        events: Vec<egui::Event>,
        running: bool,
        focused: bool,
        cover: Option<egui::Rect>,
    ) -> egui::FullOutput {
        let e = &mut self.e;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(988., 768.),
                )),
                events,
                focused,
                ..Default::default()
            },
            |ui| {
                e.draw(ui, running);
                if let Some(cover) = cover {
                    egui::Area::new(egui::Id::new("over the constraints canvas"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(cover.min)
                        .show(ui.ctx(), |ui| {
                            ui.allocate_exact_size(cover.size(), egui::Sense::click_and_drag())
                        });
                }
            },
        );
        output.textures_delta.clear();
        output
    }

    pub(super) fn solved(&self) -> Vec<Curve> {
        match self.e.draft_canvas().expect("a draft").0 {
            Drawing::Solved(curves) => curves.clone(),
            other => panic!("no solved drawing: {other:?}"),
        }
    }

    pub(super) fn canvas(&self) -> &crate::sketch::accepted::Canvas {
        self.e.draft_canvas().expect("a draft").1
    }

    pub(super) fn rect(&self) -> egui::Rect {
        self.canvas().rect().expect("the canvas was drawn")
    }

    /// The screen point of a document point in the view now.
    pub(super) fn screen(&self, p: [f64; 2]) -> egui::Pos2 {
        self.canvas().screen(p).expect("a view")
    }

    /// Where the solved Line `i` starts, ends, and its middle.
    pub(super) fn ends(&self, i: usize) -> [egui::Pos2; 2] {
        match self.solved()[i].shape {
            Shape::Line { start, end } => [self.screen(start), self.screen(end)],
            _ => panic!("a Line"),
        }
    }
    pub(super) fn mid(&self, i: usize) -> egui::Pos2 {
        let [a, b] = self.ends(i);
        a + (b - a) * 0.5
    }

    /// The same Line's middle where the Sketch STORES it, in the view now.
    pub(super) fn stored_mid(&self, i: usize) -> egui::Pos2 {
        let SketchGeometry::Line { start, end } = self.stored.curves[i].geometry else {
            panic!("a Line")
        };
        self.screen([(start.x + end.x) / 2., (start.y + end.y) / 2.])
    }

    pub(super) fn click(&mut self, at: egui::Pos2) {
        click_at(&self.ctx, &mut self.e, at, false);
    }
    pub(super) fn press(&mut self, label: &str) {
        click(&self.ctx, &mut self.e, label);
    }
    pub(super) fn target(&mut self, target: Target) {
        self.press(match target {
            Target::Object => "Object",
            Target::Endpoint => "Endpoint",
            Target::PairA => "Pair A",
            Target::PairB => "Pair B",
            Target::Dimension => "Dimension",
        });
        assert_eq!(self.canvas().target(), target);
    }

    pub(super) fn picks(
        &self,
    ) -> (
        Option<StableEntityId>,
        [Option<StableEntityId>; 2],
        LineEndpoint,
    ) {
        let (selected, a, b, _, _) = self.e.draft_state().expect("a draft");
        (selected, [a, b], self.e.draft_pin().expect("a draft"))
    }

    /// Everything a click on the canvas must leave alone.
    pub(super) fn kept(&self) -> (Vec<String>, (usize, usize), bool) {
        let (_, _, _, undo, redo) = self.e.draft_state().expect("a draft");
        (
            self.e.draft_inputs().expect("a draft"),
            (undo, redo),
            self.e.pending.is_some(),
        )
    }

    /// Stroke segments of one colour in a frame, as the screen ends they join. A window
    /// that is still fading in paints every colour at a fraction of its strength, so the
    /// colour is compared at the strength the stroke has.
    pub(super) fn strokes(out: &egui::FullOutput, colour: egui::Color32) -> Vec<[egui::Pos2; 2]> {
        let is = |shown: egui::Color32| {
            let want = colour.gamma_multiply(f32::from(shown.a()) / 255.);
            [
                (shown.r(), want.r()),
                (shown.g(), want.g()),
                (shown.b(), want.b()),
            ]
            .iter()
            .all(|(x, y)| x.abs_diff(*y) <= 1)
        };
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::LineSegment { points, stroke } if is(stroke.color) => Some(*points),
                _ => None,
            })
            .collect()
    }
}

pub(super) fn near(a: egui::Pos2, b: egui::Pos2) -> bool {
    a.distance(b) < 0.01
}

/// The moved-and-enlarged drawing really differs from the stored one where it is clicked:
/// the middle of a stored Line is far from every solved Line, so a click there is a click
/// on the stored approximation.
fn stored_is_far_from_solved(rig: &Rig) {
    for i in 0..4 {
        let at = rig.stored_mid(i);
        assert!(
            rig.rect().contains(at),
            "stored Line {i} must be on the canvas to be clicked: {at:?}"
        );
        for target in [Target::Object, Target::Endpoint] {
            assert_eq!(
                rig.canvas().would_pick(&rig.solved(), target, at),
                None,
                "Line {i} stored middle {at:?} is near a solved part"
            );
        }
    }
}

#[test]
fn a_click_fills_the_fields_the_lists_fill_for_each_explicit_target_and_only_them() {
    let mut rig = Rig::new(1.);
    stored_is_far_from_solved(&rig);
    let before = rig.kept();
    assert_eq!(rig.picks(), (None, [None, None], LineEndpoint::Start));
    let ids = rig.ids.clone();

    // Object: a Line anywhere along it.
    rig.click(rig.mid(1));
    assert_eq!(rig.picks().0, Some(ids[1]));
    rig.click(rig.mid(2) + egui::vec2(4., 0.));
    assert_eq!(rig.picks().0, Some(ids[2]));
    assert_eq!(
        rig.kept(),
        before,
        "a selection is no input, no step, no request"
    );

    // The click on the stored approximation takes nothing.
    rig.click(rig.stored_mid(0));
    assert_eq!(
        rig.picks().0,
        Some(ids[2]),
        "stored geometry is not the drawing"
    );

    // Endpoint: the stored Line's named end; the pin buttons' own field follows. At a joint
    // where Lines meet the first Line stored takes the click, so the contour 0 → 1 → 2 → 3
    // gives Line 0's Start, and each later joint the End of the Line that comes first.
    rig.target(Target::Endpoint);
    rig.click(rig.ends(0)[0]);
    assert_eq!(
        (rig.picks().0, rig.picks().2),
        (Some(ids[0]), LineEndpoint::Start)
    );
    rig.click(rig.ends(2)[1]);
    assert_eq!(
        (rig.picks().0, rig.picks().2),
        (Some(ids[2]), LineEndpoint::End)
    );
    rig.click(rig.ends(1)[0]);
    assert_eq!(
        (rig.picks().0, rig.picks().2),
        (Some(ids[0]), LineEndpoint::End),
        "the joint of Lines 0 and 1 is Line 0's End, the earlier stored"
    );
    // Along the Line, away from both ends, an Endpoint click takes nothing.
    rig.click(rig.mid(3));
    assert_eq!(rig.picks().0, Some(ids[0]));
    rig.press("Segment 4");
    assert_eq!(rig.picks().0, Some(ids[3]));

    // Pair A and Pair B: the slots, not the selection.
    rig.target(Target::PairA);
    rig.click(rig.mid(0));
    rig.target(Target::PairB);
    rig.click(rig.mid(2));
    assert_eq!(rig.picks().1, [Some(ids[0]), Some(ids[2])]);
    assert_eq!(
        rig.picks().0,
        Some(ids[3]),
        "a pair click leaves the selection"
    );
    assert_eq!(rig.kept(), before);

    // The same four fields by the lists and buttons give the same state.
    let mut lists = Rig::new(1.);
    lists.press("Segment 4");
    lists.press("Segment 1");
    lists.press("Pair Line A");
    lists.press("Segment 3");
    lists.press("Pair Line B");
    lists.press("Segment 4");
    lists.press("Pin End");
    let index =
        |r: &Rig, id: Option<StableEntityId>| id.and_then(|id| r.ids.iter().position(|x| *x == id));
    let by_index = |r: &Rig| {
        let (selected, pair, pin) = r.picks();
        (
            index(r, selected),
            [index(r, pair[0]), index(r, pair[1])],
            pin,
        )
    };
    assert_eq!(by_index(&lists), by_index(&rig));
    assert_eq!(
        by_index(&rig),
        (Some(3), [Some(0), Some(2)], LineEndpoint::End)
    );
    println!("\nFCAD_31K_FORM_TARGETS_EXECUTED");
}

#[test]
fn the_lists_and_the_canvas_show_one_selection() {
    let mut rig = Rig::new(1.);
    let ids = rig.ids.clone();
    // List to canvas: the picked Line is drawn in the selection colour, and named.
    rig.press("Segment 3");
    let out = rig.frame(vec![]);
    let selected = Rig::strokes(&out, SELECTED_COLOUR);
    let ends = rig.ends(2);
    assert!(
        selected
            .iter()
            .any(|s| near(s[0], ends[0]) && near(s[1], ends[1])),
        "{selected:?} vs {ends:?}"
    );
    assert_eq!(selected.len(), 1);
    assert!(painted(&out, &format!("Selected: Line {}", short(ids[2]))));
    // Canvas to list: the clicked Line is the one the form acts on.
    rig.click(rig.mid(0));
    let out = rig.frame(vec![]);
    assert_eq!(rig.picks().0, Some(ids[0]));
    let selected = Rig::strokes(&out, SELECTED_COLOUR);
    let ends = rig.ends(0);
    assert!(
        selected
            .iter()
            .any(|s| near(s[0], ends[0]) && near(s[1], ends[1]))
    );
    assert_eq!(selected.len(), 1, "only the selection is drawn as selected");
    // Pair slots: A and B in their own colours on their own Lines, named on the top edge
    // while a slot is the target, with the direction each vector runs.
    rig.target(Target::PairA);
    rig.click(rig.mid(1));
    rig.target(Target::PairB);
    rig.click(rig.mid(3));
    let out = rig.frame(vec![]);
    for (colour, line) in [(SIDE_A_COLOUR, 1), (SIDE_B_COLOUR, 3)] {
        let ends = rig.ends(line);
        let drawn = Rig::strokes(&out, colour);
        assert!(
            drawn
                .iter()
                .any(|s| near(s[0], ends[0]) && near(s[1], ends[1])),
            "{line}: {drawn:?}"
        );
    }
    assert!(painted(
        &out,
        &format!(
            "A: Line {} start>end · B: Line {} start>end",
            short(ids[1]),
            short(ids[3])
        )
    ));
    // Turning a vector round with the form's own button changes the words, not the pick.
    rig.press("A: start>end");
    let out = rig.frame(vec![]);
    assert!(painted(
        &out,
        &format!(
            "A: Line {} end>start · B: Line {} start>end",
            short(ids[1]),
            short(ids[3])
        )
    ));
    let (undo, redo) = (rig.kept().1.0, rig.kept().1.1);
    assert_eq!((undo, redo), (0, 0));
    rig.target(Target::PairB);
    rig.click(rig.mid(2));
    assert_eq!(
        rig.e.draft_angle().expect("a draft").1,
        (LineEndpoint::End, LineEndpoint::Start),
        "picking again does not turn a vector round"
    );
    println!("\nFCAD_31K_FORM_SYNC_EXECUTED");
}

#[test]
fn typed_inputs_history_and_requests_wait_for_an_explicit_action() {
    let mut rig = Rig::new(1.);
    let ids = rig.ids.clone();
    // Text in every box, one rule added (a history step), the angle's vectors set.
    rig.press("Segment 1");
    enter_field(&rig.ctx, &mut rig.e, "Line length (mm):", "55.5", false);
    enter_field(&rig.ctx, &mut rig.e, "Fixed X (mm):", "3.25", false);
    enter_field(&rig.ctx, &mut rig.e, "Fixed Y (mm):", "-4.5", false);
    enter_field(&rig.ctx, &mut rig.e, "A>B (°):", "25", false);
    rig.press("Add Horizontal");
    rig.press("A: start>end");
    let before = rig.kept();
    assert_eq!(before.1, (1, 0), "one step of request history");
    let edits = rig.e.draft_edits();
    let steps: [fn(&mut Rig); 4] = [
        |r| {
            let m = r.mid(1);
            r.click(m)
        },
        |r| {
            r.target(Target::Endpoint);
            let e = r.ends(2)[1];
            r.click(e)
        },
        |r| {
            r.target(Target::PairA);
            let m = r.mid(0);
            r.click(m)
        },
        |r| {
            r.target(Target::PairB);
            let m = r.mid(3);
            r.click(m)
        },
    ];
    for step in steps {
        step(&mut rig);
        assert_eq!(
            rig.kept(),
            before,
            "a pick changed an input, the history or a request"
        );
        assert_eq!(rig.e.draft_edits(), edits);
    }
    assert!(rig.e.take_request().is_none() && rig.e.take_apply_request().is_none());
    // Only the explicit action changes the request, and Undo then spends a step of its
    // own, not one of the picks.
    rig.press("Add Equal length");
    assert_eq!(rig.e.draft_edits().expect("edits").add.len(), 2);
    assert_eq!(rig.kept().1, (2, 0));
    assert_eq!(rig.picks().1, [Some(ids[0]), Some(ids[3])]);
    println!("\nFCAD_31K_FORM_INPUTS_EXECUTED");
}

#[test]
fn a_reversed_line_is_picked_by_its_start_and_end_not_by_the_screen_side() {
    let mut rig = Rig::new(1.);
    rig.target(Target::Endpoint);
    // The contour runs 0 → 1 → 2 → 3, so Line 3 (index 2) runs along the top from right to
    // left: its Start is on the right of the screen, its End on the left. At the left corner,
    // shared with Line 4's Start, the earlier Line wins the tie, and it is Line 3's End.
    let corner = rig.ends(2)[1];
    assert!(rig.ends(2)[0].x > corner.x, "Line 3 runs right to left");
    rig.click(corner);
    assert_eq!(
        (rig.picks().0, rig.picks().2),
        (Some(rig.ids[2]), LineEndpoint::End)
    );
    // The same point of the drawing, with Line 3 stored the other way round: it is now that
    // Line's Start, although it is still the left end on the screen.
    let reversed = Drawing::of(
        &rig.stored,
        rig.solved().iter().enumerate().map(|(i, c)| {
            let Shape::Line { start, end } = c.shape else {
                panic!("a Line")
            };
            let (start, end) = if i == 2 { (end, start) } else { (start, end) };
            (
                c.id,
                SketchGeometry::Line {
                    start: ferritecad_document::Point2 {
                        x: start[0],
                        y: start[1],
                    },
                    end: ferritecad_document::Point2 {
                        x: end[0],
                        y: end[1],
                    },
                },
            )
        }),
    );
    assert!(matches!(reversed, Drawing::Solved(_)));
    rig.e.show_drawing(reversed);
    for _ in 0..3 {
        rig.frame(vec![]);
    }
    rig.target(Target::Endpoint);
    assert!(
        near(rig.ends(2)[0], corner) && rig.ends(2)[0].x < rig.ends(2)[1].x,
        "Line 3 now starts at that corner, on the left"
    );
    rig.click(corner);
    assert_eq!(
        (rig.picks().0, rig.picks().2),
        (Some(rig.ids[2]), LineEndpoint::Start)
    );
    // The Line's other end is shared with Line 2, which is stored earlier and takes the click
    // there: the End of Line 3 is chosen from the list, with its Pin button.
    rig.press("Segment 3");
    rig.press("Pin End");
    assert_eq!(rig.picks().2, LineEndpoint::End);
    println!("\nFCAD_31K_FORM_REVERSED_EXECUTED");
}

pub(super) fn moved(to: egui::Pos2) -> egui::Event {
    egui::Event::PointerMoved(to)
}

pub(super) fn button(
    pos: egui::Pos2,
    button: egui::PointerButton,
    pressed: bool,
    shift: bool,
) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: if shift {
            egui::Modifiers::SHIFT
        } else {
            Default::default()
        },
    }
}

pub(super) fn primary(pos: egui::Pos2, pressed: bool) -> egui::Event {
    button(pos, egui::PointerButton::Primary, pressed, false)
}

pub(super) fn shift_primary(pos: egui::Pos2, pressed: bool) -> egui::Event {
    button(pos, egui::PointerButton::Primary, pressed, true)
}

pub(super) fn middle(pos: egui::Pos2, pressed: bool) -> egui::Event {
    button(pos, egui::PointerButton::Middle, pressed, false)
}

pub(super) fn wheel(points: f32) -> egui::Event {
    egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0., points),
        phase: egui::TouchPhase::Move,
        modifiers: Default::default(),
    }
}

/// Everything the view and the form could change under a gesture that must change nothing.
pub(super) type Facts = (
    Option<(egui::Vec2, f32)>,
    (
        Option<StableEntityId>,
        [Option<StableEntityId>; 2],
        LineEndpoint,
    ),
    (Vec<String>, (usize, usize), bool),
    Target,
);

impl Rig {
    pub(super) fn facts(&self) -> Facts {
        (
            self.canvas().view(),
            self.picks(),
            self.kept(),
            self.canvas().target(),
        )
    }
    /// The middle of Line `i` as a document point (mm), from the drawing.
    pub(super) fn mid_mm(&self, i: usize) -> [f64; 2] {
        match self.solved()[i].shape {
            Shape::Line { start, end } => [(start[0] + end[0]) / 2., (start[1] + end[1]) / 2.],
            _ => panic!("a Line"),
        }
    }
}

#[test]
fn zoom_pan_and_fit_move_only_the_view_and_a_pick_follows_the_view_at_one_and_two_pixels_per_point()
{
    let mut at_both = Vec::new();
    for pixels_per_point in [1., 2.] {
        let mut rig = Rig::new(pixels_per_point);
        let ids = rig.ids.clone();
        let opening = rig.canvas().view().expect("a fitted view");
        let kept = rig.kept();

        // Zoom about the middle of Line 1: that point of the document stays under the
        // pointer, the scale grows, and nothing but the view changed.
        let (anchor, doc) = (rig.mid(1), rig.mid_mm(1));
        let old_mid_2 = rig.mid(2);
        rig.frame(vec![moved(anchor), wheel(80.)]);
        let zoomed = rig.canvas().view().expect("a view");
        assert!(zoomed.1 > opening.1, "the wheel zooms in");
        assert!(
            near(rig.screen(doc), anchor),
            "the point under the cursor moved"
        );
        assert_eq!(rig.kept(), kept);
        // A pick follows the view: Line 2 is where it is drawn now and not where it was.
        rig.click(rig.mid(2));
        assert_eq!(rig.picks().0, Some(ids[2]));
        assert_ne!(rig.mid(2), old_mid_2);
        let spare = rig
            .canvas()
            .would_pick(&rig.solved(), Target::Object, old_mid_2);
        assert_ne!(
            spare.map(|h| h.curve),
            Some(ids[2]),
            "the old place still picks"
        );
        // The pick radius is nine logical points whatever the zoom: exactly nine away from
        // Line 1's middle takes it, a hair more does not.
        let mid = rig.mid(1);
        let [a, b] = rig.ends(1);
        let across = (b - a).normalized().rot90() * 9.;
        let across = egui::vec2(across.x, across.y);
        rig.click(mid + across);
        assert_eq!(
            rig.picks().0,
            Some(ids[1]),
            "{pixels_per_point}x: nine points"
        );
        let on = rig.picks().0;
        rig.click(mid + across * (9.01 / 9.));
        assert_eq!(
            rig.picks().0,
            on,
            "{pixels_per_point}x: more than nine points"
        );

        // A pan with Shift and the primary button: the grabbed point stays under the
        // pointer, the press on a Line does not select it and the release is no click.
        let selection = rig.picks();
        let (grab, drag) = (rig.mid(2), egui::vec2(-17., 9.));
        let before = rig.mid(1);
        rig.frame(vec![moved(grab)]);
        rig.frame(vec![shift_primary(grab, true)]);
        rig.frame(vec![moved(grab + drag)]);
        let held = rig.canvas().panning();
        rig.frame(vec![shift_primary(grab + drag, false)]);
        assert!(held && !rig.canvas().panning());
        assert!(
            near(rig.mid(1), before + drag),
            "the pan moved the drawing by the drag"
        );
        assert_eq!(
            rig.canvas().view().expect("a view").1,
            zoomed.1,
            "a pan keeps the scale"
        );
        assert_eq!(rig.picks(), selection, "a pan selected something");
        // A held middle button pans the same way.
        let (start, end) = (
            rig.canvas().rect().expect("canvas").center(),
            egui::vec2(6., -4.),
        );
        rig.frame(vec![moved(start)]);
        rig.frame(vec![middle(start, true)]);
        rig.frame(vec![moved(start + end)]);
        rig.frame(vec![middle(start + end, false)]);
        assert!(near(rig.mid(1), before + drag + end));

        // Fit is the explicit way back, to the opening view bit for bit.
        rig.press("Fit");
        assert_eq!(
            rig.canvas().view().expect("a view"),
            opening,
            "Fit is not the opening view"
        );
        assert_eq!(rig.picks(), selection);
        at_both.push((
            zoomed,
            rig.picks().0.and_then(|i| ids.iter().position(|x| *x == i)),
        ));
    }
    assert_eq!(
        at_both[0], at_both[1],
        "1x and 2x differ in a view or a pick"
    );
    println!("\nFCAD_31K_FORM_VIEW_EXECUTED");
}

#[test]
fn busy_queued_covered_outside_and_unfocused_canvases_change_nothing() {
    // A press begun on an available drawing is not permission for a later release
    // after focus or ownership changed. The refused release also consumes that press.
    for pixels_per_point in [1., 2.] {
        for interruption in ["focus", "covered", "uncovered"] {
            let mut held = Rig::new(pixels_per_point);
            let before = held.facts();
            let at = held.mid(1);
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
                "{pixels_per_point}x: a release after {interruption} selected"
            );
            assert!(!held.canvas().pressed());
        }

        let mut edge = Rig::new(pixels_per_point);
        // Bring a visible Line just inside the canvas; a three-point motion ends
        // outside while staying within click slop, so this is not the long-drag case.
        let grab = edge.rect().center();
        let delta = egui::vec2(edge.rect().left() + 2. - edge.mid(3).x, 0.);
        edge.frame(vec![moved(grab), middle(grab, true)]);
        edge.frame(vec![moved(grab + delta)]);
        edge.frame(vec![middle(grab + delta, false)]);
        let before = edge.facts();
        let at = edge.mid(3);
        assert!(edge.rect().contains(at));
        let outside = at - egui::vec2(3., 0.);
        assert!(!edge.rect().contains(outside));
        edge.frame(vec![moved(at), primary(at, true)]);
        edge.frame(vec![moved(outside), primary(outside, false)]);
        assert_eq!(
            edge.facts(),
            before,
            "{pixels_per_point}x: release outside selected"
        );
        assert!(!edge.canvas().pressed());
    }
    let mut rig = Rig::new(1.);
    let ids = rig.ids.clone();
    let base = rig.facts();
    let (aim, grab) = (rig.mid(1), rig.mid(2));
    let rect = rig.rect();
    // Every gesture the canvas takes, in one list: a click, a wheel, a Shift pan and a
    // middle pan, then the target buttons and Fit.
    let attempt = |rig: &mut Rig, running: bool, focused: bool, cover: Option<egui::Rect>| {
        for events in [
            vec![moved(aim)],
            vec![primary(aim, true)],
            vec![primary(aim, false)],
            vec![wheel(120.)],
            vec![moved(grab)],
            vec![shift_primary(grab, true)],
            vec![moved(grab + egui::vec2(30., 10.))],
            vec![shift_primary(grab + egui::vec2(30., 10.), false)],
            vec![middle(grab, true)],
            vec![moved(grab + egui::vec2(-20., 5.))],
            vec![middle(grab + egui::vec2(-20., 5.), false)],
        ] {
            rig.frame_as(events, running, focused, cover);
            assert!(!rig.canvas().panning(), "a pan began");
        }
    };
    // A job runs: the form is disabled and says its view is locked.
    attempt(&mut rig, true, true, None);
    assert_eq!(rig.facts(), base, "busy");
    assert!(painted(
        &rig.frame_as(vec![], true, true, None),
        "View locked while the form is busy"
    ));
    // The window is not focused.
    attempt(&mut rig, false, false, None);
    assert_eq!(rig.facts(), base, "unfocused");
    // Another window lies over the canvas (it needs two frames to be there).
    for _ in 0..3 {
        rig.frame_as(vec![], false, true, Some(rect));
    }
    attempt(&mut rig, false, true, Some(rect));
    assert_eq!(rig.facts(), base, "covered");
    for _ in 0..3 {
        rig.frame(vec![]);
    }
    // Outside: a hair beyond each edge of the canvas.
    for outside in [
        rect.left_top() - egui::vec2(1., 1.),
        egui::pos2(rect.right() + 1., rect.center().y),
        egui::pos2(rect.center().x, rect.bottom() + 1.),
    ] {
        rig.frame(vec![moved(outside)]);
        rig.frame(vec![primary(outside, true)]);
        rig.frame(vec![primary(outside, false)]);
        rig.frame(vec![wheel(120.)]);
        assert_eq!(rig.facts(), base, "outside at {outside:?}");
    }
    // A request of the form waits to be taken: nothing on the canvas begins.
    rig.press("Segment 1");
    rig.press("Add Horizontal");
    rig.press("Save constraints copy…");
    assert!(rig.e.pending.is_some());
    let queued = rig.facts();
    attempt(&mut rig, false, true, None);
    rig.press("Pair B");
    rig.press("Fit");
    assert_eq!(rig.facts(), queued, "queued");
    assert!(rig.e.take_request().is_some());
    // The same gestures work as soon as nothing blocks them.
    rig.click(aim);
    assert_eq!(
        rig.picks().0,
        Some(ids[1]),
        "the canvas works when nothing blocks it"
    );
    println!("\nFCAD_31K_FORM_BLOCKED_EXECUTED");
}

#[test]
fn each_missing_drawing_is_stated_in_the_form_and_the_lists_still_pick() {
    let mut rig = Rig::new(1.);
    let ids = rig.ids.clone();
    let out = rig.frame(vec![]);
    assert!(painted(
        &out,
        "Canvas: solved drawing; pending rules not drawn."
    ));
    assert!(painted(&out, "Accepted · solved"));
    assert!(painted(&out, "Object") && painted(&out, "Fit"));
    for why in [
        Unavailable::NotHanded,
        Unavailable::OtherVersion,
        Unavailable::NoDrawing,
        Unavailable::Differs,
        Unavailable::Profile,
        Unavailable::NotDrawable,
    ] {
        rig.e.show_drawing(Drawing::Unavailable(why));
        rig.frame(vec![]);
        let out = rig.frame(vec![]);
        assert!(
            painted(
                &out,
                &format!("Accepted drawing unavailable: {}.", why.short())
            ),
            "{why:?}"
        );
        assert!(painted(
            &out,
            "Coordinates below are stored inputs, not the solved drawing."
        ));
        assert!(
            !painted(&out, "Object") && !painted(&out, "Fit"),
            "no canvas without a drawing"
        );
        assert!(!painted(&out, "Accepted · solved"));
        // The selectors work as they always did.
        rig.press("Segment 2");
        assert_eq!(rig.picks().0, Some(ids[1]));
        rig.press("Segment 3");
        rig.press("Pair Line A");
        assert_eq!(rig.picks().1[0], Some(ids[2]));
        rig.press("Segment 1");
        assert_eq!(rig.picks().0, Some(ids[0]));
    }
    // A form opened with no scene's drawing says so; a draft held on a version that is no
    // longer the accepted one loses its drawing.
    let (_root, path, source) = fixture();
    let mut e = Editor::default();
    assert!(e.begin(&path, &source, source.constraint_sketches[0].sketch));
    assert_eq!(
        e.draft_canvas().expect("a draft").0,
        &Drawing::Unavailable(Unavailable::NotHanded)
    );
    println!("\nFCAD_31K_FORM_UNAVAILABLE_EXECUTED");
}

#[test]
fn rules_still_pending_are_not_in_the_drawing_until_they_are_applied() {
    let mut rig = Rig::new(1.);
    let drawn = rig.solved();
    let before = Rig::strokes(&rig.frame(vec![]), crate::sketch::accepted::DRAWING_COLOUR);
    rig.press("Segment 1");
    rig.press("Add Horizontal");
    rig.press("Segment 2");
    rig.press("Add Vertical");
    enter_field(&rig.ctx, &mut rig.e, "Line length (mm):", "33", false);
    rig.press("Add length");
    let out = rig.frame(vec![]);
    assert_eq!(
        rig.e.draft_edits().expect("edits").add.len(),
        3,
        "three pending rules"
    );
    assert_eq!(
        rig.solved(),
        drawn,
        "a pending rule changed the accepted drawing"
    );
    assert_eq!(
        Rig::strokes(&out, crate::sketch::accepted::DRAWING_COLOUR),
        before,
        "a pending rule moved a drawn curve"
    );
    assert!(painted(&out, "Pending additions:"));
    assert!(rig.e.take_apply_request().is_none() && rig.e.take_request().is_none());
    println!("\nFCAD_31K_FORM_PENDING_EXECUTED");
}

#[test]
fn a_pan_holds_the_window_and_ends_with_the_canvas_that_stops_being_drawn() {
    let mut rig = Rig::new(1.);
    let mut sketch = crate::sketch::Editor::default();
    std::mem::swap(&mut sketch.constraints, &mut rig.e);
    let run =
        |sketch: &mut crate::sketch::Editor, ctx: &egui::Context, events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(988., 768.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| sketch.draw(ui, true, false, ""),
            );
            out.textures_delta.clear();
        };
    for _ in 0..3 {
        run(&mut sketch, &rig.ctx, vec![]);
    }
    let from = sketch
        .constraints
        .draft_canvas()
        .and_then(|(_, c)| c.rect())
        .expect("canvas")
        .center();
    assert!(!sketch.gesturing());
    run(&mut sketch, &rig.ctx, vec![moved(from)]);
    run(&mut sketch, &rig.ctx, vec![middle(from, true)]);
    run(
        &mut sketch,
        &rig.ctx,
        vec![moved(from + egui::vec2(8., 3.))],
    );
    assert!(
        sketch.gesturing(),
        "a pan holds the window, as a dragged vertex does"
    );
    // Typing and the buttons are not disabled for it (it edits nothing).
    assert!(sketch.constraints.panning());
    // The release ends it.
    run(
        &mut sketch,
        &rig.ctx,
        vec![middle(from + egui::vec2(8., 3.), false)],
    );
    assert!(!sketch.gesturing());
    // A pan whose canvas then stops being drawn (the drawing went away under it) ends,
    // or the window could never be left or quit.
    run(&mut sketch, &rig.ctx, vec![moved(from)]);
    run(&mut sketch, &rig.ctx, vec![middle(from, true)]);
    run(
        &mut sketch,
        &rig.ctx,
        vec![moved(from + egui::vec2(8., 3.))],
    );
    assert!(sketch.gesturing());
    sketch.constraints.retire_drawing();
    run(&mut sketch, &rig.ctx, vec![]);
    run(&mut sketch, &rig.ctx, vec![]);
    run(&mut sketch, &rig.ctx, vec![]);
    assert!(
        !sketch.gesturing(),
        "a pan on a canvas that is gone holds the window"
    );
    println!("\nFCAD_31K_FORM_PAN_HOLD_EXECUTED");
}

#[test]
fn the_canvas_never_reaches_the_3d_camera_and_a_viewport_drag_still_crosses_the_form() {
    use winit::{
        dpi::PhysicalPosition,
        event::{DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent},
    };
    for pixels_per_point in [1., 2.] {
        let rig = Rig::new(pixels_per_point);
        let on_canvas = rig.mid(1);
        let outside = egui::pos2(900., 700.);
        let mut rig = rig;
        rig.frame(vec![moved(outside)]);
        assert!(!rig.ctx.egui_wants_pointer_input());
        let mut input = ferritecad_ui::ViewportInput::new();
        input.resize(
            (988. * pixels_per_point) as u32,
            (768. * pixels_per_point) as u32,
        );
        input.frame(([-10.; 3], [10.; 3])).expect("frame");
        let camera = *input.camera();
        let motion = |p: egui::Pos2| WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(
                f64::from(p.x * pixels_per_point),
                f64::from(p.y * pixels_per_point),
            ),
        };
        let mouse = |button, state| WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button,
        };
        let wheel_event = WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0., 3.),
            phase: TouchPhase::Moved,
        };
        let pinch = WindowEvent::PinchGesture {
            device_id: DeviceId::dummy(),
            delta: 0.4,
            phase: TouchPhase::Moved,
        };
        let ctx = rig.ctx.clone();
        let run = |input: &mut ferritecad_ui::ViewportInput, events: &[WindowEvent]| {
            for event in events {
                let claimed = crate::window_pointer_claimed(&ctx, input, event);
                crate::apply_viewport_input(input, event, false, claimed);
            }
        };
        run(
            &mut input,
            &[
                motion(on_canvas),
                wheel_event.clone(),
                pinch.clone(),
                mouse(MouseButton::Middle, ElementState::Pressed),
                motion(on_canvas + egui::vec2(24., 12.)),
                mouse(MouseButton::Middle, ElementState::Released),
                mouse(MouseButton::Left, ElementState::Pressed),
                motion(on_canvas + egui::vec2(48., 6.)),
                mouse(MouseButton::Left, ElementState::Released),
            ],
        );
        assert_eq!(
            *input.camera(),
            camera,
            "{pixels_per_point}x: the canvas moved the 3D view"
        );
        assert!(input.take_pick().is_none(), "the canvas picked in 3D");
        run(&mut input, &[motion(outside), wheel_event, pinch]);
        assert_ne!(*input.camera(), camera, "the viewport lost its wheel");
        let zoomed = *input.camera();
        // A drag begun in the viewport still belongs to it across the form.
        run(
            &mut input,
            &[
                motion(outside),
                mouse(MouseButton::Middle, ElementState::Pressed),
                motion(on_canvas),
                mouse(MouseButton::Middle, ElementState::Released),
            ],
        );
        assert_ne!(*input.camera(), zoomed, "the form swallowed a viewport pan");
    }
    println!("\nFCAD_31K_FORM_ROUTING_EXECUTED");
}

/// A single stored Circle drawn at another place and size: the synthetic solved drawing.
pub(super) fn moved_circle(stored: &Sketch) -> Drawing {
    let c = &stored.curves[0];
    let SketchGeometry::Circle { center, radius } = c.geometry else {
        panic!("a Circle")
    };
    Drawing::of(
        stored,
        [(
            c.id,
            SketchGeometry::Circle {
                center: ferritecad_document::Point2 {
                    x: center.x + 8.,
                    y: center.y + 10.,
                },
                radius: radius * 1.5,
            },
        )],
    )
}

/// A closed polygon of `n` Lines on a circle of 40 mm, written with a Sketch and an
/// Extrude by the document's own sample, so the form is the production one.
pub(super) fn polygon_fixture(
    n: usize,
    constrained: bool,
) -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let (root, path, _) = fixture();
    let mut document = Document::open(&path).expect("source");
    let mut object = document
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Sketch(_)))
        .expect("Sketch");
    let ferritecad_document::ObjectPayload::Sketch(sketch) = &mut object.payload else {
        unreachable!()
    };
    let old_ids: Vec<_> = sketch.curves.iter().map(|c| c.id).collect();
    let points: Vec<_> = (0..n)
        .map(|i| {
            let angle = i as f64 * std::f64::consts::TAU / n as f64;
            ferritecad_document::Point2::new(40. * angle.cos(), 40. * angle.sin()).expect("point")
        })
        .collect();
    sketch.curves = (0..n)
        .map(|i| ferritecad_document::SketchCurve {
            id: old_ids.get(i).copied().unwrap_or_else(StableEntityId::new),
            construction: false,
            geometry: SketchGeometry::Line {
                start: points[i],
                end: points[(i + 1) % n],
            },
        })
        .collect();
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
        .expect("polygon");
    if constrained {
        // As the old 768-point gate does: a horizontal and a length on the last Line, and
        // with them every closure link, so the list of persisted constraints is full.
        let ferritecad_document::ObjectPayload::Sketch(sketch) = &object.payload else {
            unreachable!()
        };
        let last = sketch.curves.last().expect("curve").id;
        let prepared = ferritecad_document::prepare_sketch_constraints(
            &document,
            object.id,
            &SketchConstraintEdits {
                remove: vec![],
                add: vec![
                    AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: last,
                        kind: LineConstraintKind::Horizontal,
                    }),
                    AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: last,
                        kind: LineConstraintKind::Distance(LineLengthMm::new(42.).expect("42")),
                    }),
                ],
            },
        )
        .expect("persisted H and all closure links");
        document
            .write_sketch_constraints(&prepared)
            .expect("constraints");
    }
    let source = ExtrudeEditSource::read(&document).expect("discovery");
    document.close().expect("close");
    (root, path, source)
}

/// Whether the painted text `label` is whole inside the screen and its clip.
pub(super) fn whole(out: &egui::FullOutput, label: &str, screen: egui::Rect) -> bool {
    out.shapes.iter().any(|s| {
        matches!(&s.shape, egui::Shape::Text(t)
            if t.galley.text() == label
                && s.clip_rect.contains_rect(t.visual_bounding_rect())
                && screen.contains_rect(t.visual_bounding_rect()))
    })
}

#[test]
fn the_form_keeps_its_canvas_its_modes_and_its_actions_reachable_at_768_points() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(988., 768.));
    let lines = [
        "Cancel constraints draft",
        "Undo",
        "Redo",
        "Clear pending changes",
        "Add length",
        "Replace length",
        "Pin Start",
        "Pin End",
        "Add Fixed point",
        "Pair Line A",
        "Pair Line B",
        "Add Equal length",
        "Add Parallel",
        "Add Perpendicular",
        "Add Collinear",
        "Apply constraints",
        "Save constraints copy…",
        "Object",
        "Endpoint",
        "Pair A",
        "Pair B",
        "Fit",
    ];
    let circle = [
        "Cancel constraints draft",
        "Undo",
        "Redo",
        "Clear pending changes",
        "Add radius",
        "Add diameter",
        "Add Fixed centre",
        "Apply constraints",
        "Save constraints copy…",
        "Object",
        "Endpoint",
        "Pair A",
        "Pair B",
        "Fit",
    ];
    let dir = tempfile::tempdir().expect("dir");
    let shaft = dir.path().join("shaft.fcad");
    revolve::write_sector(&shaft, &revolve::SHAFT_T, 137.5, Some(5));
    let sector = dir.path().join("sector.fcad");
    revolve::write_sector(&sector, &revolve::SECTOR_P, 137.5, None);
    let (_a, _b, plate) = fixture();
    let (_c, polygon_path, polygon) = polygon_fixture(64, false);
    let (_f, held_path, held) = polygon_fixture(64, true);
    let (_d, round_path, round, _) = diameter::round_fixture(&[10.]);
    let (_e, plate_path, _) = fixture();
    // A plate that already holds its lengths: the lists of persisted and pending rules, the
    // stored length of the picked Line and, below, a refusal.
    let (_g, loaded_path, _) = fixture();
    let loaded = persist_rectangle_lengths(&loaded_path);
    let cases: Vec<(&str, PathBuf, ExtrudeEditSource, &[&str])> = vec![
        ("plate", plate_path, plate, &lines),
        (
            "plate with H, V and both lengths persisted",
            loaded_path,
            loaded,
            &lines,
        ),
        ("64-gon", polygon_path, polygon, &lines),
        (
            "64-gon with every closure link and a stored length",
            held_path,
            held,
            &lines,
        ),
        (
            "six-segment shaft closed on the axis",
            shaft.clone(),
            revolve::reading(&shaft),
            &lines,
        ),
        (
            "five-segment sector",
            sector.clone(),
            revolve::reading(&sector),
            &lines,
        ),
        ("circle", round_path, round, &circle),
    ];
    let mut report = Vec::new();
    for (name, path, source, labels) in cases {
        let choice = &source.constraint_sketches[0];
        assert!(choice.refusal.is_none(), "{name}");
        let stored = choice.stored.clone().expect("stored");
        let is_circle = matches!(stored.curves[0].geometry, SketchGeometry::Circle { .. });
        let solved = if is_circle {
            moved_circle(&stored)
        } else {
            enlarged(&stored)
        };
        assert!(matches!(solved, Drawing::Solved(_)), "{name}");
        let mut bottoms = Vec::new();
        for (with_canvas, unsaved) in [(false, false), (true, false), (true, true)] {
            let mut e = Editor::default();
            assert!(e.begin(&path, &source, choice.sketch), "{name}");
            if with_canvas {
                e.show_drawing(solved.clone());
            }
            e.set_session(true, true, unsaved);
            let draft = e.draft.as_mut().expect("draft");
            if !stored.constraints.is_empty() {
                let pick = if stored.curves.len() >= 32 {
                    stored.curves.last()
                } else {
                    stored.curves.first()
                };
                draft.selected = pick.map(|c| c.id);
            }
            // The fully loaded polygon is as the old gate left it, which has no room for a
            // refusal line even without the canvas; every other form carries one.
            if stored.curves.len() < 32 {
                draft.refusal = Some("Enter a positive radius in mm.".to_owned());
            }
            let line = |i: usize, kind| {
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: stored.curves[i].id,
                    kind,
                })
            };
            draft.edits.add = if is_circle {
                Vec::new()
            } else if stored.curves.len() == 4 && !stored.constraints.is_empty() {
                vec![
                    line(2, LineConstraintKind::Horizontal),
                    line(3, LineConstraintKind::Vertical),
                ]
            } else {
                stored
                    .curves
                    .iter()
                    .step_by(2)
                    .map(|c| {
                        AddSketchConstraint::Line(AddLineConstraint::Line {
                            curve: c.id,
                            kind: LineConstraintKind::Horizontal,
                        })
                    })
                    .collect()
            };
            if is_circle {
                draft.edits.add.push(AddSketchConstraint::Circle {
                    curve: stored.curves[0].id,
                    kind: CircleConstraintKind::Radius(CircleRadiusMm::new(7.5).expect("radius")),
                });
            }
            draft
                .choice
                .validate_edits(&draft.edits)
                .unwrap_or_else(|why| panic!("{name}: {why}"));
            let ctx = egui::Context::default();
            for _ in 0..4 {
                frame(&ctx, &mut e, vec![]);
            }
            let out = frame(&ctx, &mut e, vec![]);
            for label in labels.iter().filter(|l| {
                with_canvas || !matches!(**l, "Object" | "Endpoint" | "Pair A" | "Pair B" | "Fit")
            }) {
                assert!(
                    whole(&out, label, screen),
                    "{name} (canvas {with_canvas}, unsaved {unsaved}): {label} is not reachable"
                );
            }
            if with_canvas {
                let rect = e
                    .draft_canvas()
                    .and_then(|(_, c)| c.rect())
                    .expect("canvas");
                assert!(
                    screen.contains_rect(rect),
                    "{name}: the canvas is off the screen {rect:?}"
                );
            }
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
            bottoms.push(bottom);
        }
        report.push(format!(
            "{name}: Save bottom {:.1} without the canvas, {:.1} with it, {:.1} with it and the unsaved note",
            bottoms[0], bottoms[1], bottoms[2]
        ));
    }
    for line in &report {
        eprintln!("FORM_BUDGET {line}");
    }
    println!("\nFCAD_31K_FORM_768_EXECUTED");
}

// ---- native: the real scene of a real rebuild and solve ------------------------------

use super::session_apply::replace_segment_length;
use crate::{EdgeNames, FaceNames, LiveScene, VertexNames, Visibility};

/// The scene of `path` read as the window reads a document (Open CASCADE and the solver),
/// and the live scene that arrival commits.
pub(super) fn accepted_scene(path: &Path) -> (ferritecad_scene::LoadedScene, LiveScene<()>) {
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("the kernel");
    let loaded = ferritecad_scene::snapshot_of(
        path,
        &mut kernel,
        |kernel: &mut ferritecad_occt::OcctKernel, bytes: &[u8]| kernel.import_step(bytes),
        &ferritecad_kernel::TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the accepted scene");
    let mut camera = ferritecad_ui::ViewportInput::new();
    camera.resize(800, 600);
    let mut scene = LiveScene::new(
        None,
        (),
        Vec::new(),
        FaceNames::default(),
        EdgeNames::default(),
        VertexNames::default(),
        Visibility::default(),
        Vec::new(),
    );
    let prepared = crate::prepare_load(&camera, path, Ok(loaded.clone()), |_, _| Ok(()));
    crate::commit_scene(&mut scene, &mut camera, prepared).expect("the scene commits");
    (loaded, scene)
}

/// The curves of the sketch `sketch` in the real presentation of `loaded`, as numbers.
pub(super) fn presented(
    loaded: &ferritecad_scene::LoadedScene,
    sketch: ObjectId,
) -> Vec<(StableEntityId, SketchGeometry)> {
    loaded
        .sketch_presentations
        .iter()
        .find(|p| p.sketch() == sketch)
        .expect("the rebuild drew the Sketch")
        .curves()
        .iter()
        .map(|c| (c.id(), c.geometry().clone()))
        .collect()
}

impl Rig {
    /// The form on a real accepted scene of `path`, as the window opens it.
    pub(super) fn native(
        path: &Path,
        source: &ExtrudeEditSource,
        sketch: ObjectId,
    ) -> (Self, ferritecad_scene::LoadedScene) {
        let (loaded, scene) = accepted_scene(path);
        let facts = scene.edit_source.clone().expect("the scene's facts");
        assert_eq!(
            facts.version, source.version,
            "the scene and the gate read one version"
        );
        let stored = facts.constraint_sketches[0].stored.clone().expect("stored");
        let ids = stored.curves.iter().map(|c| c.id).collect();
        let mut e = Editor::default();
        e.set_session(true, true, true);
        assert!(e.begin_with(path, &facts, &scene.sketches, sketch));
        let ctx = egui::Context::default();
        let mut rig = Self {
            _root: None,
            stored,
            ids,
            e,
            ctx,
        };
        for _ in 0..3 {
            rig.frame(vec![]);
        }
        (rig, loaded)
    }
}

/// The request the lists and buttons alone make, for the form opened with no drawing.
pub(super) fn by_lists(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    script: impl FnOnce(&egui::Context, &mut Editor),
) -> EditSketchConstraintsRequest {
    let mut e = Editor::default();
    e.set_session(true, true, true);
    assert!(e.begin(path, source, sketch));
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    script(&ctx, &mut e);
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    click(&ctx, &mut e, "Apply constraints");
    e.take_apply_request().expect("Apply asked")
}

/// §31K: a rectangle whose Sketch STORES a rough guess (a 12 × 8 mm parallelogram about the
/// origin) and keeps rules that solve it into a 60 × 30 mm trapezoid around the same place:
/// Line 1 horizontal and 60 mm, Line 2 vertical and 30 mm, Line 3 horizontal. Every stored
/// vertex and Line middle then lies inside the solved drawing, far from all of it.
pub(crate) fn write_guessed_rectangle(path: &Path) {
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
    let mm = |v| LineConstraintKind::Distance(LineLengthMm::new(v).expect("length"));
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
                line(0, mm(60.)),
                line(1, mm(30.)),
                line(2, LineConstraintKind::Horizontal),
            ],
        },
    )
    .expect("prepare");
    document.write_sketch_constraints(&prepared).expect("rules");
    document.close().expect("close");
}

pub(super) fn line_numbers(g: &SketchGeometry) -> ([f64; 2], [f64; 2]) {
    match g {
        SketchGeometry::Line { start, end } => ([start.x, start.y], [end.x, end.y]),
        _ => panic!("a Line"),
    }
}

/// §31K, native: the form for the rectangle written by `write_guessed_rectangle`, driven
/// only by clicks on the canvas of the real accepted drawing, and the same rules made once
/// more by the lists alone. The two requests must be one request. Returns the form with its
/// Apply asked, as the session gate takes it.
pub(crate) fn native_rectangle(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut rig, loaded) = Rig::native(path, source, sketch);
    let real = presented(&loaded, sketch);
    let (ids, stored) = (rig.ids.clone(), rig.stored.clone());
    // The drawing the form holds IS the rebuild's presentation, curve for curve.
    let held = rig.solved();
    assert_eq!(held.len(), 4);
    for ((id, geometry), curve) in real.iter().zip(&held) {
        let (start, end) = line_numbers(geometry);
        assert_eq!((curve.id, curve.shape), (*id, Shape::Line { start, end }));
    }
    // It is the solved rectangle's trapezoid, not the stored guess: Line 1 is horizontal and
    // 60 mm, Line 2 vertical and 30, Line 3 horizontal; and every stored Line is far from it.
    let solved: Vec<_> = real.iter().map(|(_, g)| line_numbers(g)).collect();
    assert!((solved[0].0[1] - solved[0].1[1]).abs() < 1e-6, "{solved:?}");
    assert!(
        (solved[0].0[0] - solved[0].1[0]).abs() - 60. < 1e-6,
        "{solved:?}"
    );
    assert!((solved[1].0[0] - solved[1].1[0]).abs() < 1e-6, "{solved:?}");
    assert!(
        ((solved[1].0[1] - solved[1].1[1]).abs() - 30.).abs() < 1e-6,
        "{solved:?}"
    );
    assert!((solved[2].0[1] - solved[2].1[1]).abs() < 1e-6, "{solved:?}");
    let guess: Vec<_> = stored
        .curves
        .iter()
        .map(|c| line_numbers(&c.geometry))
        .collect();
    let apart = guess
        .iter()
        .zip(&solved)
        .map(|(g, s)| (g.1[0] - s.1[0]).hypot(g.1[1] - s.1[1]))
        .fold(0., f64::max);
    assert!(
        apart > 15.,
        "the stored guess and the solved drawing differ by only {apart} mm"
    );
    eprintln!(
        "NATIVE_RECT stored={guess:?} solved={solved:?} scale={:?} rect={:?}",
        rig.canvas().view(),
        rig.rect()
    );
    // A click on the stored approximation is a click on nothing, in every mode: each stored
    // vertex and Line middle is on the canvas, inside the solved drawing, and not near any
    // part of it.
    let stored_points: Vec<_> = (0..4)
        .flat_map(|i| [rig.stored_mid(i), rig.screen(guess[i].0)])
        .collect();
    for at in stored_points {
        assert!(
            rig.rect().contains(at),
            "{at:?} is off the canvas {:?}",
            rig.rect()
        );
        for target in [Target::Object, Target::PairA, Target::Endpoint] {
            assert_eq!(
                rig.canvas().would_pick(&rig.solved(), target, at),
                None,
                "{target:?}: the stored point {at:?} is near the solved drawing"
            );
        }
    }
    rig.canvas_mut().set_target(Target::Object);
    let stored_hit = rig.stored_mid(0);
    rig.click(stored_hit);
    assert_eq!(
        rig.picks().0,
        None,
        "a click on the stored guess selected a Line"
    );

    // The rules, from the canvas alone. Perpendicular(Line 4, Line 1): Pair A is Line 4.
    rig.target(Target::PairA);
    rig.click(rig.mid(3));
    rig.target(Target::PairB);
    rig.click(rig.mid(0));
    assert_eq!(rig.picks().1, [Some(ids[3]), Some(ids[0])]);
    rig.press("Add Perpendicular");
    // Replace Line 1's 60 mm by 70.
    rig.target(Target::Object);
    rig.click(rig.mid(0));
    assert_eq!(rig.picks().0, Some(ids[0]));
    enter_field(&rig.ctx, &mut rig.e, "Line length (mm):", "70", false);
    rig.press("Replace length");
    // Pin the End of Line 2, the solved corner, at (80, 45).
    rig.target(Target::Endpoint);
    let corner = rig.ends(1)[1];
    rig.click(corner);
    assert_eq!(
        (rig.picks().0, rig.picks().2),
        (Some(ids[1]), LineEndpoint::End)
    );
    enter_field(&rig.ctx, &mut rig.e, "Fixed X (mm):", "80", false);
    enter_field(&rig.ctx, &mut rig.e, "Fixed Y (mm):", "45", false);
    rig.press("Add Fixed point");
    assert_eq!(
        rig.e
            .draft_inputs()
            .expect("draft")
            .last()
            .map(String::as_str),
        Some("None"),
        "a refusal"
    );

    // The same rules by the lists and buttons alone.
    let lists = by_lists(path, source, sketch, |ctx, e| {
        click(ctx, e, "Segment 4");
        click(ctx, e, "Pair Line A");
        click(ctx, e, "Segment 1");
        click(ctx, e, "Pair Line B");
        click(ctx, e, "Add Perpendicular");
        replace_segment_length(ctx, e, "Segment 1", "70");
        click(ctx, e, "Segment 2");
        click(ctx, e, "Pin End");
        enter_field(ctx, e, "Fixed X (mm):", "80", false);
        enter_field(ctx, e, "Fixed Y (mm):", "45", false);
        click(ctx, e, "Add Fixed point");
    });
    rig.press("Apply constraints");
    let request = rig.e.take_apply_request().expect("Apply asked");
    assert_eq!(
        request.edits, lists.edits,
        "the canvas and the lists asked for different things"
    );
    assert_eq!(
        (request.expected, request.sketch, &request.source),
        (lists.expected, lists.sketch, &lists.source)
    );
    assert_eq!(request.edits.remove.len(), 1, "the replaced length");
    assert_eq!(request.edits.add.len(), 3);
    println!(
        "\nFCAD_31K_NATIVE_RECTANGLE_REQUEST stored_apart_mm={apart:.3} solved_width=60 adds=3 removes=1"
    );
    (rig.e, request)
}

impl Rig {
    pub(super) fn canvas_mut(&mut self) -> &mut crate::sketch::accepted::Canvas {
        self.e.draft_canvas_mut().expect("a draft")
    }
}

/// §31K: a cylinder whose Sketch STORES a small circle (4.5, 0) of radius 1.2 and keeps a
/// Radius of 10 and a Fixed centre at (0, 0): the solved circle is the big one around the
/// origin, and the stored one lies inside it, far from its ring and its centre.
pub(crate) fn write_guessed_circle(path: &Path) {
    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            path,
            ferritecad_jobs::NewDocument::CircleExtrude(
                ferritecad_document::CircleExtrusion::new([4.5, 0.], 1.2, 15.).expect("a circle"),
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
    let circle = choice.stored.as_ref().expect("stored").curves[0].id;
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        choice.sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                AddSketchConstraint::Circle {
                    curve: circle,
                    kind: CircleConstraintKind::Radius(CircleRadiusMm::new(10.).expect("radius")),
                },
                AddSketchConstraint::Circle {
                    curve: circle,
                    kind: CircleConstraintKind::FixedCenter {
                        x: SketchCoordinateMm::new(0.).expect("x"),
                        y: SketchCoordinateMm::new(0.).expect("y"),
                    },
                },
            ],
        },
    )
    .expect("prepare");
    document.write_sketch_constraints(&prepared).expect("rules");
    document.close().expect("close");
}

/// §31K, native: the form for the cylinder written by `write_guessed_circle`. The canvas
/// holds the solved circle (0, 0) r 10, not the stored (4.5, 0) r 1.2. The ring takes
/// the Radius, the centre the Fixed centre, each replacing the persisted rule.
pub(crate) fn native_circle(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut rig, loaded) = Rig::native(path, source, sketch);
    let real = presented(&loaded, sketch);
    let (circle, stored) = (rig.ids[0], rig.stored.clone());
    let SketchGeometry::Circle { center, radius } = real[0].1 else {
        panic!("a Circle")
    };
    assert_eq!(real[0].0, circle);
    assert!(
        center.x.abs() < 1e-6 && center.y.abs() < 1e-6 && (radius - 10.).abs() < 1e-6,
        "the solved circle is ({}, {}) r {radius}",
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
            radius
        },
        "the form holds the rebuild's circle"
    );
    // The stored circle's ring and centre are nowhere near the drawing.
    let stored_centre = rig.screen([guess.x, guess.y]);
    let stored_ring = rig.screen([guess.x + guess_r, guess.y]);
    for target in [Target::Object, Target::Endpoint] {
        for at in [stored_centre, stored_ring] {
            assert!(rig.rect().contains(at), "{at:?} is off the canvas");
            assert_eq!(
                rig.canvas().would_pick(&rig.solved(), target, at),
                None,
                "{target:?}: {at:?} is near the solved circle"
            );
        }
    }
    let before = rig.picks();
    rig.click(stored_ring);
    rig.click(stored_centre);
    assert_eq!(rig.picks(), before);

    // The ring selects the circle (and names the part); the persisted Radius is removed
    // and a Radius of 8.5 added.
    rig.click(rig.screen([center.x + radius, center.y]));
    assert_eq!(rig.picks().0, Some(circle));
    assert_eq!(rig.canvas().circle_part(circle), Some(Part::Ring));
    rig.press_remove("Radius 10 mm");
    enter_field(&rig.ctx, &mut rig.e, "Radius mm", "8.5", false);
    rig.press("Add radius");
    // The centre (Endpoint target): the same curve, named as its centre; the persisted Fixed
    // centre is removed and a new one added at (2.5, -1.25).
    rig.target(Target::Endpoint);
    rig.click(rig.screen([center.x, center.y]));
    assert_eq!(rig.picks().0, Some(circle));
    assert_eq!(rig.canvas().circle_part(circle), Some(Part::Centre));
    rig.press_remove("Fixed centre (0, 0) mm");
    enter_field(&rig.ctx, &mut rig.e, "Fixed X (mm):", "2.5", false);
    enter_field(&rig.ctx, &mut rig.e, "Fixed Y (mm):", "-1.25", false);
    rig.press("Add Fixed centre");

    let lists = by_lists(path, source, sketch, |ctx, e| {
        click(ctx, e, &format!("Circle · {}", short(circle)));
        click_remove_on_row(ctx, e, "Radius 10 mm");
        enter_field(ctx, e, "Radius mm", "8.5", false);
        click(ctx, e, "Add radius");
        click_remove_on_row(ctx, e, "Fixed centre (0, 0) mm");
        enter_field(ctx, e, "Fixed X (mm):", "2.5", false);
        enter_field(ctx, e, "Fixed Y (mm):", "-1.25", false);
        click(ctx, e, "Add Fixed centre");
    });
    rig.press("Apply constraints");
    let request = rig.e.take_apply_request().expect("Apply asked");
    assert_eq!(
        request.edits, lists.edits,
        "the canvas and the lists asked for different things"
    );
    assert_eq!(request.edits.remove.len(), 2);
    assert_eq!(request.edits.add.len(), 2);
    println!(
        "\nFCAD_31K_NATIVE_CIRCLE_REQUEST stored=(4.5,0,1.2) solved=(0,0,10) adds=2 removes=2"
    );
    (rig.e, request)
}

impl Rig {
    pub(super) fn press_remove(&mut self, row: &str) {
        click_remove_on_row(&self.ctx, &mut self.e, row);
    }
}

/// §31K, native: the SAVED rectangle read three ways that agree: the presentation of its
/// cold rebuild (the rectangle the rules ask for, at the corner they pin), the volume and
/// bounds of its exported STL read by `read_stl` here and not by the writer, and the pinned
/// reader the workflow runs on the FBX of the same model.
pub(crate) fn saved_rectangle_is_what_the_rules_ask(saved: &Path, stl: &[u8], height: f64) {
    let (loaded, _) = accepted_scene(saved);
    let sketch = loaded
        .edit_source
        .as_ref()
        .expect("facts")
        .constraint_sketches[0]
        .sketch;
    let lines: Vec<_> = presented(&loaded, sketch)
        .iter()
        .map(|(_, g)| line_numbers(g))
        .collect();
    // Line 1 horizontal and 70, Line 2 vertical and 30 with its End at (80, 45), Line 3
    // horizontal, Line 4 vertical (the Perpendicular): the rectangle (10,15)..(80,45).
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
    let corners = [[10., 15.], [80., 15.], [80., 45.], [10., 45.]];
    for (i, line) in lines.iter().enumerate() {
        assert!(
            near(line.0, corners[i]) && near(line.1, corners[(i + 1) % 4]),
            "Line {} runs {line:?}, not {:?} to {:?}",
            i + 1,
            corners[i],
            corners[(i + 1) % 4]
        );
    }
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
    for (got, want) in lo.iter().chain(&hi).zip([10., 15., 0., 80., 45., height]) {
        assert!((got - want).abs() < 1e-3, "bounds {lo:?} {hi:?}");
    }
}

/// §31K, native: the SAVED cylinder, its drawing and its STL: the circle (2.5, -1.25) r 8.5
/// that the rules ask for, every rim vertex of the mesh on it, and the mesh inscribed in it.
pub(crate) fn saved_circle_is_what_the_rules_ask(saved: &Path, stl: &[u8], height: f64) {
    let (loaded, _) = accepted_scene(saved);
    let sketch = loaded
        .edit_source
        .as_ref()
        .expect("facts")
        .constraint_sketches[0]
        .sketch;
    let SketchGeometry::Circle { center, radius } = presented(&loaded, sketch)[0].1 else {
        panic!("a Circle")
    };
    assert!(
        (center.x - 2.5).abs() < 1e-6
            && (center.y + 1.25).abs() < 1e-6
            && (radius - 8.5).abs() < 1e-6,
        "({}, {}) r {radius}",
        center.x,
        center.y
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
        let d = (p[0] - center.x).hypot(p[1] - center.y);
        assert!(
            (d - radius).abs() < 1e-3,
            "a rim vertex {p:?} is {d} from the centre"
        );
    }
}

pub(super) fn read_stl_of(bytes: &[u8]) -> (Vec<[[f64; 3]; 3]>, f64) {
    super::diameter::read_stl(bytes)
}

pub(super) fn escape() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}

#[test]
fn a_press_is_a_click_only_while_it_stays_put_and_nothing_takes_it_away() {
    let mut rig = Rig::new(1.);
    let ids = rig.ids.clone();
    let base = rig.facts();
    let aim = rig.mid(1);
    let far = aim + egui::vec2(20., 0.);
    // A press that moves further than a click does (even if it comes back), and a release
    // away from where the press was, pick nothing.
    rig.frame(vec![moved(aim)]);
    rig.frame(vec![primary(aim, true)]);
    assert!(rig.canvas().pressed());
    rig.frame(vec![moved(far)]);
    rig.frame(vec![moved(aim)]);
    rig.frame(vec![primary(aim, false)]);
    assert_eq!(rig.facts(), base, "a drag that came back");
    rig.frame(vec![primary(aim, true)]);
    rig.frame(vec![moved(far), primary(far, false)]);
    assert_eq!(rig.facts(), base, "a release elsewhere");
    // Escape, a lost focus and a vanished pointer take the press away; the release then
    // finds nothing to finish.
    for event in [
        escape(),
        egui::Event::WindowFocused(false),
        egui::Event::PointerGone,
    ] {
        rig.frame(vec![moved(aim)]);
        rig.frame(vec![primary(aim, true)]);
        assert!(rig.canvas().pressed());
        rig.frame(vec![event.clone()]);
        assert!(!rig.canvas().pressed(), "{event:?}");
        rig.frame(vec![moved(aim), primary(aim, false)]);
        assert_eq!(rig.facts(), base, "{event:?}");
    }
    // A press with Shift or the middle button is a pan's, not a click's, even when it does
    // not move: it selects nothing, on a Line or off it.
    for (press, release) in [
        (shift_primary(aim, true), shift_primary(aim, false)),
        (middle(aim, true), middle(aim, false)),
        (
            shift_primary(rig.rect().center(), true),
            shift_primary(rig.rect().center(), false),
        ),
    ] {
        rig.frame(vec![moved(aim)]);
        rig.frame(vec![press]);
        rig.frame(vec![release]);
        assert_eq!(rig.facts(), base, "a press that is a pan's");
    }
    // The events of one frame keep their order: a wheel before a press that is still held at
    // the end of the frame is dropped, and the press is read against the view the person
    // saw, so the release picks the Line that was under the pointer.
    let view = rig.canvas().view();
    rig.frame(vec![moved(aim), wheel(120.), primary(aim, true)]);
    assert_eq!(rig.canvas().view(), view, "a held press holds the view");
    rig.frame(vec![primary(aim, false)]);
    assert_eq!(rig.picks().0, Some(ids[1]));
    // The control: the same press and release without any of that is a click.
    rig.click(rig.mid(0));
    assert_eq!(rig.picks().0, Some(ids[0]));
    println!("\nFCAD_31K_FORM_PRESS_EXECUTED");
}

impl Rig {
    /// The form on a single stored Circle with a synthetic solved drawing: the same circle
    /// moved by (8, 10) mm and grown by half.
    fn circle() -> Self {
        let (root, path, source, ids) = diameter::round_fixture(&[10.]);
        let choice = &source.constraint_sketches[0];
        let stored = choice.stored.clone().expect("stored");
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, choice.sketch));
        e.show_drawing(moved_circle(&stored));
        let mut rig = Self {
            _root: Some(root),
            stored,
            ids,
            e,
            ctx: egui::Context::default(),
        };
        for _ in 0..3 {
            rig.frame(vec![]);
        }
        rig
    }
}

#[test]
fn a_circle_is_one_curve_picked_by_its_ring_or_its_centre_and_a_single_circle_has_no_pair() {
    let mut rig = Rig::circle();
    let circle = rig.ids[0];
    let Shape::Circle { center, radius } = rig.solved()[0].shape else {
        panic!("a Circle")
    };
    let (middle, on_ring) = (
        rig.screen(center),
        rig.screen([center[0] + radius, center[1]]),
    );
    let before = rig.kept();
    // Object: the ring takes the circle and says so; the centre takes the same circle and
    // says that.
    rig.click(on_ring);
    assert_eq!(rig.picks().0, Some(circle));
    assert_eq!(rig.canvas().circle_part(circle), Some(Part::Ring));
    let out = rig.frame(vec![]);
    assert!(painted(
        &out,
        &format!("Selected: Circle {} · circle", short(circle))
    ));
    rig.click(middle);
    assert_eq!(rig.picks().0, Some(circle), "the centre is the same curve");
    assert_eq!(rig.canvas().circle_part(circle), Some(Part::Centre));
    let out = rig.frame(vec![]);
    assert!(painted(
        &out,
        &format!("Selected: Circle {} · centre", short(circle))
    ));
    // The centre is marked apart: a ring around it in the selection colour.
    let marked = out.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Circle(c) => {
            near(c.center, middle) && c.radius == 9.0 && c.stroke.width == 2.0
        }
        _ => false,
    });
    assert!(marked, "the picked centre is not marked");
    // Choosing the circle in the list says nothing of a part: the circle, not its centre.
    rig.press(&format!("Circle · {}", short(circle)));
    rig.frame(vec![]);
    assert_eq!(rig.picks().0, Some(circle));
    // Endpoint takes the centre only, never the ring.
    rig.target(Target::Endpoint);
    rig.click(on_ring);
    assert_eq!(
        rig.canvas().circle_part(circle),
        Some(Part::Centre),
        "the ring is no end"
    );
    rig.click(middle);
    assert_eq!(rig.canvas().circle_part(circle), Some(Part::Centre));
    // A single circle has nothing to pair: the two slots are not offered.
    let out = rig.frame(vec![]);
    assert!(painted(&out, "Object") && painted(&out, "Endpoint"));
    // A disabled button is painted in the weak text colour, an enabled one in its own.
    let ink = |label: &str| {
        out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => Some(t.fallback_color),
            _ => None,
        })
    };
    assert_ne!(
        ink("Pair A").expect("painted"),
        ink("Endpoint").expect("painted"),
        "Pair A is not shown as unavailable"
    );
    rig.press("Pair A");
    assert_ne!(rig.canvas().target(), Target::PairA);
    assert_eq!(rig.picks().1, [None, None]);
    // The numbers of the form are the stored ones' and nothing was added.
    assert_eq!(rig.kept().1, before.1);
    assert_eq!(rig.kept().2, before.2);
    // The stored circle is not the drawing: its centre is not the solved centre.
    let SketchGeometry::Circle { center: stored, .. } = rig.stored.curves[0].geometry else {
        panic!("a Circle")
    };
    assert_ne!([stored.x, stored.y], center);
    println!("\nFCAD_31K_FORM_CIRCLE_EXECUTED");
}

#[test]
fn a_pick_on_the_canvas_brings_its_row_of_a_long_list_into_view() {
    let (_root, path, source) = polygon_fixture(64, false);
    let choice = &source.constraint_sketches[0];
    let stored = choice.stored.clone().expect("stored");
    let ids: Vec<_> = stored.curves.iter().map(|c| c.id).collect();
    let mut e = Editor::default();
    assert!(e.begin(&path, &source, choice.sketch));
    e.show_drawing(enlarged(&stored));
    let ctx = egui::Context::default();
    for _ in 0..4 {
        frame(&ctx, &mut e, vec![]);
    }
    let shown = |out: &egui::FullOutput, prefix: &str| {
        out.shapes.iter().any(|s| {
            matches!(&s.shape, egui::Shape::Text(t)
                if t.galley.text().starts_with(prefix)
                    && s.clip_rect.contains_rect(t.visual_bounding_rect()))
        })
    };
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        shown(&out, "Segment 1 ·") && !shown(&out, "Segment 41 ·"),
        "the list opens at its top"
    );
    // Segment 41's middle, from the drawing the canvas holds.
    let (view, rect) = {
        let (_, canvas) = e.draft_canvas().expect("canvas");
        (canvas, canvas.rect().expect("drawn"))
    };
    let Drawing::Solved(curves) = e.draft_canvas().expect("draft").0 else {
        panic!("a drawing")
    };
    let Shape::Line { start, end } = curves[40].shape else {
        panic!("a Line")
    };
    let at = view
        .screen([(start[0] + end[0]) / 2., (start[1] + end[1]) / 2.])
        .expect("a view");
    assert!(rect.contains(at));
    click_at(&ctx, &mut e, at, false);
    assert_eq!(e.draft_state().expect("draft").0, Some(ids[40]));
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        shown(&out, "Segment 41 ·"),
        "the picked Line's row was left out of sight"
    );
    // A pick that does not move the selection leaves the list where it is.
    e.draft_canvas_mut()
        .expect("canvas")
        .set_target(Target::PairA);
    click_at(&ctx, &mut e, at, false);
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    let again = frame(&ctx, &mut e, vec![]);
    assert!(shown(&again, "Segment 41 ·"));
    println!("\nFCAD_31K_FORM_REVEAL_EXECUTED");
}
