// SPDX-License-Identifier: MIT
//! §31I: the canvas of the Circle and Circle-with-hole forms, New and saved.
//!
//! A view of the strings the form already holds, with three handles on it. It
//! owns no model and no history: a finished gesture hands back its exact press
//! and release strings, and the form records that as one step of its own bounded
//! history. The circles are drawn by egui for the screen only; the request and
//! the document keep their analytic circles.

use super::{
    DRAWABLE_MM, PICK_POINTS, fit_view,
    navigation::{self, Navigation, Surface, View},
    number, paint_grid, paint_pointer_label, pointer_over,
};

/// The canvas's size in logical points. Fixed, so it never widens the form.
pub(super) const SIZE: egui::Vec2 = egui::vec2(510., 230.);
/// The bounding circle and its handle, and the bore and its handle.
const OUTER_COLOUR: egui::Color32 = egui::Color32::LIGHT_BLUE;
const BORE_COLOUR: egui::Color32 = egui::Color32::from_rgb(255, 165, 60);
const HELD_COLOUR: egui::Color32 = egui::Color32::LIGHT_YELLOW;

/// What a press can take hold of. The order is the order an exact tie is
/// decided in: a centre move cannot change a size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Handle {
    Centre,
    Outer,
    Bore,
}

/// The fields of one form, lent to the canvas for one frame.
pub(super) struct Fields<'a> {
    pub(super) centre: &'a mut [String; 2],
    pub(super) outer: &'a mut String,
    /// The bore's radius; `None` for a plain circle.
    pub(super) bore: Option<&'a mut String>,
}

/// The exact text of those fields: what a cancelled gesture restores and what
/// one Undo after a gesture gives back.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Strings {
    pub(super) centre: [String; 2],
    pub(super) outer: String,
    pub(super) bore: Option<String>,
}

/// A released gesture's exact endpoints. A later, still-held gesture in the same
/// frame may already have changed the fields, but is not a confirmed edit.
pub(super) struct Finished {
    pub(super) before: Strings,
    pub(super) after: Strings,
}

impl Fields<'_> {
    fn strings(&self) -> Strings {
        Strings {
            centre: self.centre.clone(),
            outer: self.outer.clone(),
            bore: self.bore.as_deref().cloned(),
        }
    }
    fn restore(&mut self, strings: &Strings) {
        self.centre.clone_from(&strings.centre);
        self.outer.clone_from(&strings.outer);
        if let (Some(bore), Some(text)) = (self.bore.as_deref_mut(), &strings.bore) {
            bore.clone_from(text);
        }
    }
}

/// The numbers of the fields when every one of them can be drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Profile {
    pub(super) centre: [f64; 2],
    pub(super) outer: f64,
    pub(super) bore: Option<f64>,
}

/// One field as a number the canvas can draw, or why not.
fn drawable(text: &str) -> Result<f64, String> {
    if text.trim().is_empty() {
        return Err("empty".into());
    }
    let value = number(text).map_err(|_| format!("{text:?} is not a number"))?;
    if !value.is_finite() {
        return Err(format!("{text:?} is not a finite number"));
    }
    if value.abs() > DRAWABLE_MM {
        return Err(format!(
            "{} mm is farther than {DRAWABLE_MM} mm from the origin",
            text.trim()
        ));
    }
    Ok(value)
}

impl Profile {
    /// What the fields say, or every field that cannot be drawn and why. Nothing
    /// is clamped, swapped or replaced by an older value.
    pub(super) fn read(fields: &Fields<'_>) -> Result<Self, String> {
        let outer_name = if fields.bore.is_some() {
            "Outer radius"
        } else {
            "Radius"
        };
        let mut named = vec![
            ("Center X", fields.centre[0].as_str()),
            ("Center Y", fields.centre[1].as_str()),
            (outer_name, fields.outer.as_str()),
        ];
        if let Some(bore) = fields.bore.as_deref() {
            named.push(("Inner radius", bore.as_str()));
        }
        let mut values = Vec::new();
        let mut problems = Vec::new();
        for (name, text) in named {
            match drawable(text) {
                Ok(value) => values.push(value),
                Err(why) => problems.push(format!("{name}: {why}")),
            }
        }
        if !problems.is_empty() {
            return Err(problems.join("; "));
        }
        Ok(Self {
            centre: [values[0], values[1]],
            outer: values[2],
            bore: values.get(3).copied(),
        })
    }

    /// The handles a press can take and where they are, in tie order. A radius that
    /// is not positive has no circle and so no handle.
    pub(super) fn handles(&self) -> Vec<(Handle, [f64; 2])> {
        let [x, y] = self.centre;
        let mut all = vec![(Handle::Centre, self.centre)];
        if self.outer > 0.0 {
            all.push((Handle::Outer, [x + self.outer, y]));
        }
        if let Some(bore) = self.bore.filter(|r| *r > 0.0) {
            all.push((Handle::Bore, [x, y + bore]));
        }
        all
    }

    /// The radius a fit has to show: the larger positive one, else 1 mm about the
    /// centre.
    fn extent(&self) -> f64 {
        let r = self.outer.max(self.bore.unwrap_or(0.0));
        if r > 0.0 { r } else { 1.0 }
    }
}

impl View {
    /// The Line canvas's fit (its scale and 30-point margins), centred on the profile.
    fn fit(profile: &Profile) -> Self {
        let r = profile.extent();
        let [x, y] = profile.centre;
        let (_, scale) = fit_view([x - r, y - r], [x + r, y + r], SIZE);
        let per_mm = f64::from(scale);
        Self {
            minimum: [
                x - f64::from(SIZE.x) / 2.0 / per_mm,
                y - f64::from(SIZE.y) / 2.0 / per_mm,
            ],
            scale,
        }
    }
}

/// The handle a press at `at` takes: the least screen distance within the pick
/// radius among the handles inside the visible part of the canvas; an exact tie
/// goes to the earlier of centre, outer, bore.
pub(super) fn pick(
    view: &View,
    rect: egui::Rect,
    visible: egui::Rect,
    profile: &Profile,
    at: egui::Pos2,
) -> Option<Handle> {
    profile
        .handles()
        .into_iter()
        .enumerate()
        .filter_map(|(order, (handle, p))| {
            let screen = view.screen(rect, p);
            let distance = screen.distance_sq(at);
            (visible.contains(screen) && distance <= PICK_POINTS * PICK_POINTS)
                .then_some((order, distance, handle))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(_, _, handle)| handle)
}

/// A press on a handle, frozen until its release or cancellation.
#[derive(Debug)]
struct Gesture {
    handle: Handle,
    press: egui::Pos2,
    /// The transform of the press.
    view: View,
    /// The numbers at the press: the centre a radius is measured from.
    profile: Profile,
    before: Strings,
}

#[derive(Debug, Default)]
pub(super) struct Canvas {
    /// None until the first frame that can draw the profile fits it.
    pub(super) view: Option<View>,
    gesture: Option<Gesture>,
    /// §31J: zoom and pan of this canvas's own view.
    nav: Navigation,
}

impl Canvas {
    /// A press holds a handle.
    pub(super) fn gesturing(&self) -> bool {
        self.gesture.is_some()
    }
    /// §31J: a pan of the view is held. It edits nothing, so it does not disable the
    /// form's boxes and buttons as a held handle does; the window is held with it.
    pub(super) fn panning(&self) -> bool {
        self.nav.panning()
    }
    /// §31J: see `Navigation::forget_undrawn`.
    pub(super) fn forget_undrawn(&mut self, frame: u64) {
        self.nav.forget_undrawn(frame);
    }
    /// The handle held, for tests and the words.
    pub(super) fn held(&self) -> Option<Handle> {
        self.gesture.as_ref().map(|g| g.handle)
    }

    fn cancel(&mut self, fields: &mut Fields<'_>) {
        if let Some(gesture) = self.gesture.take() {
            fields.restore(&gesture.before);
        }
    }

    /// The strings the held gesture makes with the pointer at `at`: the centre moves
    /// by the pointer's movement (no jump to the pointer, an unmoved axis keeps its
    /// text); a radius is the Euclidean distance of the moved handle from the centre
    /// of the press. Only the held handle's fields are written.
    fn preview(&self, fields: &mut Fields<'_>, at: egui::Pos2) {
        let Some(g) = &self.gesture else {
            return;
        };
        let delta = [
            f64::from(at.x) - f64::from(g.press.x),
            f64::from(g.press.y) - f64::from(at.y),
        ];
        let per_mm = f64::from(g.view.scale);
        let moved = [delta[0] / per_mm, delta[1] / per_mm];
        match g.handle {
            Handle::Centre => {
                for axis in 0..2 {
                    fields.centre[axis] = if delta[axis] == 0.0 {
                        g.before.centre[axis].clone()
                    } else {
                        (g.profile.centre[axis] + moved[axis]).to_string()
                    };
                }
            }
            Handle::Outer => {
                *fields.outer = if delta == [0.0, 0.0] {
                    g.before.outer.clone()
                } else {
                    (g.profile.outer + moved[0]).hypot(moved[1]).to_string()
                };
            }
            Handle::Bore => {
                let (Some(field), Some(text), Some(r)) =
                    (fields.bore.as_deref_mut(), &g.before.bore, g.profile.bore)
                else {
                    return;
                };
                *field = if delta == [0.0, 0.0] {
                    text.clone()
                } else {
                    moved[0].hypot(r + moved[1]).to_string()
                };
            }
        }
    }

    /// Shows the profile and takes the frame's pointer events in order. Returns the
    /// before/after strings of every gesture that ended in this frame with a change:
    /// each is one history step of the form. `queued`: a request of the form waits to
    /// be taken, so no gesture may start.
    pub(super) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        mut fields: Fields<'_>,
        queued: bool,
    ) -> Vec<Finished> {
        let read = Profile::read(&fields);
        if self.view.is_none()
            && let Ok(profile) = &read
        {
            self.view = Some(View::fit(profile));
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.gesture.is_none() && !self.nav.panning() && read.is_ok(),
                    egui::Button::new("Fit profile"),
                )
                .clicked()
                && let Ok(profile) = &read
            {
                self.view = Some(View::fit(profile));
            }
            ui.label("+X right · +Y up · mm · drag the centre or a radius handle");
        });
        let events = ui.input(|i| i.events.clone());
        let (response, painter) = ui.allocate_painter(SIZE, egui::Sense::click_and_drag());
        let rect = response.rect;
        let visible = rect.intersect(ui.clip_rect());
        // A form that cannot be used now (a job runs) holds no gesture.
        if !ui.is_enabled() {
            self.cancel(&mut fields);
        }
        // §31J: navigation reads the same events in order, against a working copy of the
        // view. Presses, moves and releases of the frame are read against the view the
        // person saw; the copy replaces it at the end of the frame, unless a handle is held.
        let open = ui.is_enabled() && !queued;
        self.nav.begin(ui, open);
        let mut working = self.view;
        let mut finished = Vec::new();
        for event in &events {
            if let Some(view) = working.as_mut()
                && self.nav.event(
                    &Surface {
                        ui,
                        rect,
                        visible,
                        open,
                        frozen: self.gesture.is_some(),
                    },
                    event,
                    view,
                )
            {
                continue;
            }
            match *event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } => {
                    self.cancel(&mut fields);
                    if !queued
                        && pointer_over(ui, visible, pos)
                        && let Some(view) = self.view
                        && let Ok(profile) = Profile::read(&fields)
                        && let Some(handle) = pick(&view, rect, visible, &profile, pos)
                    {
                        self.gesture = Some(Gesture {
                            handle,
                            press: pos,
                            view,
                            profile,
                            before: fields.strings(),
                        });
                    }
                }
                egui::Event::PointerMoved(pos) => self.preview(&mut fields, pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => {
                    self.preview(&mut fields, pos);
                    let after = fields.strings();
                    if let Some(gesture) = self.gesture.take()
                        && after != gesture.before
                    {
                        finished.push(Finished {
                            before: gesture.before,
                            after,
                        });
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } if self.gesture.is_some() => {
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                    self.cancel(&mut fields);
                }
                egui::Event::WindowFocused(false) | egui::Event::PointerGone => {
                    self.cancel(&mut fields);
                }
                _ => {}
            }
        }
        // A lost release must not leave a preview or a held handle.
        if !ui.input(|i| i.focused) || !ui.input(|i| i.pointer.primary_down()) {
            self.cancel(&mut fields);
        }
        self.nav.finish(ui);
        if self.gesture.is_none() {
            self.view = working;
        }
        self.paint(&painter, rect, visible, &fields, response.hover_pos());
        navigation::paint_hint(&painter, rect, self.nav.hint(open, self.gesture.is_some()));
        finished
    }

    fn paint(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        visible: egui::Rect,
        fields: &Fields<'_>,
        hover: Option<egui::Pos2>,
    ) {
        let Some(view) = self.view else {
            painter.rect_filled(rect, 0., egui::Color32::from_gray(25));
            paint_words(
                painter,
                rect,
                &Profile::read(fields).err().unwrap_or_default(),
            );
            return;
        };
        paint_grid(painter, rect, view.minimum, view.scale);
        let annular = fields.bore.is_some();
        painter.text(
            rect.left_top() + egui::vec2(5., 5.),
            egui::Align2::LEFT_TOP,
            if annular {
                "blue: bounding circle, square handle · orange: bore, diamond handle"
            } else {
                "blue: the circle, square handle · cross: the centre"
            },
            egui::FontId::proportional(12.),
            egui::Color32::WHITE,
        );
        let profile = match Profile::read(fields) {
            Ok(profile) => profile,
            Err(why) => {
                paint_words(painter, rect, &format!("Not drawn: {why}"));
                return;
            }
        };
        let per_mm = view.scale;
        let centre = view.screen(rect, profile.centre);
        let circles = [
            (profile.outer, OUTER_COLOUR),
            (profile.bore.unwrap_or(0.0), BORE_COLOUR),
        ];
        let mut shown = false;
        for (radius, colour) in circles.into_iter().filter(|(r, _)| *r > 0.0) {
            let r = (radius * f64::from(per_mm)) as f32;
            painter.circle_stroke(centre, r, egui::Stroke::new(2., colour));
            shown |= stroke_visible(visible, centre, r);
        }
        let held = self.held();
        let hovered = if held.is_none() {
            hover.and_then(|at| pick(&view, rect, visible, &profile, at))
        } else {
            None
        };
        for (handle, at) in profile.handles() {
            let at = view.screen(rect, at);
            shown |= visible.contains(at);
            let colour = if held == Some(handle) {
                HELD_COLOUR
            } else {
                match handle {
                    Handle::Centre => egui::Color32::WHITE,
                    Handle::Outer => OUTER_COLOUR,
                    Handle::Bore => BORE_COLOUR,
                }
            };
            match handle {
                Handle::Centre => {
                    let stroke = egui::Stroke::new(2., colour);
                    painter
                        .line_segment([at - egui::vec2(6., 0.), at + egui::vec2(6., 0.)], stroke);
                    painter
                        .line_segment([at - egui::vec2(0., 6.), at + egui::vec2(0., 6.)], stroke);
                }
                Handle::Outer => {
                    painter.rect_filled(
                        egui::Rect::from_center_size(at, egui::Vec2::splat(8.)),
                        0.,
                        colour,
                    );
                }
                Handle::Bore => {
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            at + egui::vec2(0., -5.),
                            at + egui::vec2(5., 0.),
                            at + egui::vec2(0., 5.),
                            at + egui::vec2(-5., 0.),
                        ],
                        colour,
                        egui::Stroke::NONE,
                    ));
                }
            }
            if hovered == Some(handle) || held == Some(handle) {
                painter.circle_stroke(at, PICK_POINTS, egui::Stroke::new(1.5, colour));
                let pointer = hover.unwrap_or(at);
                paint_pointer_label(painter, rect, pointer, &words(handle, fields), colour);
            }
        }
        if !shown {
            paint_words(
                painter,
                rect,
                "The profile is outside the view: press Fit profile.",
            );
        }
    }
}

/// Whether some of a circle's stroke lies in `visible`: the rectangle is neither
/// wholly outside the circle nor wholly inside it.
fn stroke_visible(visible: egui::Rect, centre: egui::Pos2, r: f32) -> bool {
    let nearest = centre.distance(visible.clamp(centre));
    let farthest = [
        visible.left_top(),
        visible.right_top(),
        visible.left_bottom(),
        visible.right_bottom(),
    ]
    .into_iter()
    .map(|corner| centre.distance(corner))
    .fold(0., f32::max);
    nearest <= r && r <= farthest
}

/// What a handle is and what its field says now, as typed.
fn words(handle: Handle, fields: &Fields<'_>) -> String {
    match handle {
        Handle::Centre => format!(
            "Centre ({}, {}) mm",
            fields.centre[0].trim(),
            fields.centre[1].trim()
        ),
        Handle::Outer if fields.bore.is_some() => {
            format!("Outer radius {} mm", fields.outer.trim())
        }
        Handle::Outer => format!("Radius {} mm", fields.outer.trim()),
        Handle::Bore => format!(
            "Bore radius {} mm",
            fields.bore.as_deref().map_or("", |b| b.trim())
        ),
    }
}

/// A standing sentence in the middle of the canvas, wrapped to its width.
fn paint_words(painter: &egui::Painter, rect: egui::Rect, words: &str) {
    let galley = painter.layout(
        words.to_owned(),
        egui::FontId::proportional(13.),
        egui::Color32::from_rgb(255, 110, 100),
        rect.width() - 20.,
    );
    let corner = rect.center() - galley.size() / 2.;
    painter.rect_filled(
        egui::Rect::from_min_size(corner, galley.size()).expand(4.),
        2.,
        egui::Color32::from_black_alpha(200),
    );
    painter.galley(corner, galley, egui::Color32::from_rgb(255, 110, 100));
}

#[cfg(test)]
mod tests {
    use super::{super::canvas_point, *};

    fn read(centre: [&str; 2], outer: &str, bore: Option<&str>) -> Result<Profile, String> {
        let mut centre = centre.map(str::to_owned);
        let mut outer = outer.to_owned();
        let mut bore = bore.map(str::to_owned);
        Profile::read(&Fields {
            centre: &mut centre,
            outer: &mut outer,
            bore: bore.as_mut(),
        })
    }

    /// A canvas at the origin whose centre handle is at (255, 115) points.
    fn view_at(centre: [f64; 2], scale: f32) -> (View, egui::Rect) {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE);
        let per_mm = f64::from(scale);
        let view = View {
            minimum: [centre[0] - 255. / per_mm, centre[1] - 115. / per_mm],
            scale,
        };
        (view, rect)
    }

    #[test]
    fn fields_that_cannot_be_drawn_are_named_and_numbers_that_can_are_kept_as_they_are() {
        for (text, why) in [
            ("", "Radius: empty"),
            ("  ", "Radius: empty"),
            ("-", "Radius: \"-\" is not a number"),
            ("1,5", "Radius: \"1,5\" is not a number"),
            ("NaN", "Radius: \"NaN\" is not a finite number"),
            ("inf", "Radius: \"inf\" is not a finite number"),
            ("1e999", "Radius: \"1e999\" is not a finite number"),
            (
                "1000000.5",
                "Radius: 1000000.5 mm is farther than 1000000 mm from the origin",
            ),
        ] {
            assert_eq!(
                read(["1", "2"], text, None),
                Err(why.to_owned()),
                "{text:?}"
            );
        }
        // Every field that cannot be drawn is named, in the form's order and words.
        assert_eq!(
            read(["", "x"], "3", Some("-")),
            Err(
                "Center X: empty; Center Y: \"x\" is not a number; Inner radius: \"-\" is \
                 not a number"
                    .to_owned()
            )
        );
        assert_eq!(
            read(["0", "0"], "nan", Some("2")),
            Err("Outer radius: \"nan\" is not a finite number".to_owned())
        );
        // A refused size is still drawn as it is: nothing is clamped or swapped, and a
        // radius that is not positive has no handle.
        let negative = read(["1.5", "-2"], "-3", None).expect("finite");
        assert_eq!(negative.outer, -3.0);
        assert_eq!(
            negative.handles().iter().map(|h| h.0).collect::<Vec<_>>(),
            [Handle::Centre]
        );
        let inverted = read(["0", "0"], "4", Some("6.5")).expect("finite");
        assert_eq!((inverted.outer, inverted.bore), (4.0, Some(6.5)));
        assert_eq!(
            inverted.handles(),
            [
                (Handle::Centre, [0., 0.]),
                (Handle::Outer, [4., 0.]),
                (Handle::Bore, [0., 6.5]),
            ]
        );
        let zero = read([" 2 ", "1e1"], "0", Some("0")).expect("finite");
        assert_eq!(zero.centre, [2., 10.]);
        assert_eq!(zero.handles().len(), 1);
        // The edge of the drawable range is drawable.
        assert!(read(["-1000000", "1000000"], "1000000", None).is_ok());
        println!("\nFCAD_31I_UNDRAWABLE_EXECUTED");
    }

    #[test]
    fn crowded_handles_go_to_the_nearest_and_an_exact_tie_to_centre_outer_bore() {
        // 1 mm is 4 points: the outer handle is 4 points east of the centre and the
        // bore's 2 points north, all three inside one pick radius.
        let profile = read(["10", "-5"], "1", Some("0.5")).expect("numbers");
        let (view, rect) = view_at(profile.centre, 4.0);
        let pick_at = |x: f32, y: f32| pick(&view, rect, rect, &profile, egui::pos2(x, y));
        let (centre, outer, bore) = (
            egui::pos2(255., 115.),
            egui::pos2(259., 115.),
            egui::pos2(255., 113.),
        );
        for (handle, at) in [
            (Handle::Centre, centre),
            (Handle::Outer, outer),
            (Handle::Bore, bore),
        ] {
            assert_eq!(view.screen(rect, profile.handles()[handle as usize].1), at);
            assert_eq!(pick_at(at.x, at.y), Some(handle), "{handle:?} on itself");
        }
        // Exactly between the centre and the outer handle: the centre.
        assert_eq!(pick_at(257., 115.), Some(Handle::Centre));
        // Half a point nearer either side decides it.
        assert_eq!(pick_at(257.5, 115.), Some(Handle::Outer));
        assert_eq!(pick_at(256.5, 115.), Some(Handle::Centre));
        // Equally far from the outer and the bore handle, farther from the centre
        // (10, 10 and 18 squared points): the outer one.
        assert_eq!(pick_at(258., 112.), Some(Handle::Outer));
        assert_eq!(pick_at(257.5, 112.25), Some(Handle::Bore));
        // Equally far from the centre and the bore handle: the centre.
        assert_eq!(pick_at(255., 114.), Some(Handle::Centre));
        // The pick radius is nine points, inclusive, from the nearest handle.
        assert_eq!(pick_at(268., 115.), Some(Handle::Outer));
        assert_eq!(pick_at(268.5, 115.), None);
        // So small a scale that all three are one point: the centre, every time.
        let (tiny, rect) = view_at(profile.centre, 1e-7);
        for at in [egui::pos2(255., 115.), egui::pos2(260., 118.)] {
            assert_eq!(pick(&tiny, rect, rect, &profile, at), Some(Handle::Centre));
        }
        // What the canvas does not show cannot be taken, even when it is nearest.
        let visible = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(257., 230.));
        assert_eq!(
            pick(&view, rect, visible, &profile, outer),
            Some(Handle::Centre)
        );
        let none = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(250., 230.));
        assert_eq!(pick(&view, rect, none, &profile, centre), None);
        println!("\nFCAD_31I_PICK_RULE_EXECUTED");
    }

    #[test]
    fn a_fit_shows_the_larger_positive_radius_centred_with_the_line_canvas_margins() {
        for (profile, extent) in [
            (read(["12.5", "-7.25"], "10.5", Some("4.75")), 10.5),
            (read(["12.5", "-7.25"], "4", Some("6.5")), 6.5),
            (read(["-3", "1e3"], "-1", None), 1.0),
        ] {
            let profile = profile.expect("numbers");
            let view = View::fit(&profile);
            let rect = egui::Rect::from_min_size(egui::pos2(40., 60.), SIZE);
            let centre = view.screen(rect, profile.centre);
            assert!((centre - rect.center()).length() < 1e-3, "{centre:?}");
            // The tighter side keeps 30 points of margin: 230 - 2 x 30 = 170 points
            // across the diameter.
            assert!((f64::from(view.scale) * 2. * extent - 170.).abs() < 1e-3);
            // The transform is the Line canvas's.
            assert_eq!(
                centre,
                canvas_point(rect, view.minimum, view.scale, profile.centre)
            );
        }
        println!("\nFCAD_31I_FIT_EXECUTED");
    }
}
