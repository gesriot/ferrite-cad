// SPDX-License-Identifier: MIT
use super::{Editor, Pending, Selection, geometry, rules};
use ferritecad_document::SketchGeometry;
use ferritecad_viewport::{Camera, PixelRect};
use ferritecad_viewport_gpu::{
    SKETCH_COLOUR, SKETCH_CONSTRUCTION_COLOUR, SKETCH_CONSTRUCTION_STROKE_PIXELS,
    SKETCH_HOVER_COLOUR, SKETCH_POINT_PIXELS, SKETCH_SELECTED_COLOUR, SKETCH_STROKE_PIXELS,
};

fn colour(rgb: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(
        (rgb[0] * 255.) as u8,
        (rgb[1] * 255.) as u8,
        (rgb[2] * 255.) as u8,
    )
}

impl Editor {
    pub(crate) fn paint(&mut self, ctx: &egui::Context, camera: &Camera, region: PixelRect) {
        self.labels.clear();
        let Some(draft) = &self.draft else {
            return;
        };
        let Some(plane) = draft.planes[draft.plane].frame else {
            return;
        };
        let scale = ctx.pixels_per_point();
        let at = |p: [f64; 2]| {
            let (x, y) = region.window((p[0] as f32, p[1] as f32));
            egui::pos2(x, y) / scale
        };
        let clip = egui::Rect::from_min_max(
            at([0., 0.]),
            at([f64::from(camera.width()), f64::from(camera.height())]),
        );
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("sketch drawing overlay"),
            ))
            .with_clip_rect(clip);
        let draw = |geometry: &SketchGeometry, colour, width| {
            let points: Vec<_> = geometry::projected(camera, plane, geometry)
                .into_iter()
                .map(at)
                .collect();
            if let [point] = points.as_slice() {
                painter.circle_filled(*point, SKETCH_POINT_PIXELS / scale / 2., colour);
            } else {
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(width / scale, colour),
                ));
            }
        };
        for curve in &draft.state.curves {
            let rgb = if draft
                .state
                .selected
                .iter()
                .any(|s| s.whole() == Some(curve.id))
            {
                SKETCH_SELECTED_COLOUR
            } else if self.hovered == Some(Selection::Curve(curve.id)) {
                SKETCH_HOVER_COLOUR
            } else if curve.construction {
                SKETCH_CONSTRUCTION_COLOUR
            } else {
                SKETCH_COLOUR
            };
            let width = if curve.construction {
                SKETCH_CONSTRUCTION_STROKE_PIXELS
            } else {
                SKETCH_STROKE_PIXELS
            };
            draw(&curve.geometry, colour(rgb), width);
        }
        for handle in draft.state.selected.iter().copied().chain(self.hovered) {
            if let Selection::Point(p) = handle
                && let Some(pos) = rules::position(&draft.state.curves, p)
                && let Some(screen) = geometry::screen(camera, plane, pos)
            {
                let rgb = if draft.state.selected.contains(&handle) {
                    SKETCH_SELECTED_COLOUR
                } else {
                    SKETCH_HOVER_COLOUR
                };
                painter.circle_filled(at(screen), 4., colour(rgb));
            }
        }
        let project = |p| geometry::screen(camera, plane, p).map(at);
        let mut occupied: Vec<egui::Rect> = Vec::new();
        for c in &draft.state.constraints {
            use crate::sketch::{dimension, ink};
            let Some((pos, words, marker)) = caption(
                c,
                &draft.state.curves,
                draft.state.diameters.contains(&c.id),
            ) else {
                continue;
            };
            let Some(pos) = project(pos) else {
                continue;
            };
            let selected = draft.state.selected.contains(&Selection::Constraint(c.id));
            let hot = self.hovered == Some(Selection::Constraint(c.id));
            let mut style = if marker {
                ink::marker_look(selected, hot, false, true)
            } else {
                ink::label_look(selected, hot, false, true)
            };
            if selected {
                style.0.color = colour(SKETCH_SELECTED_COLOUR);
            }
            let size = painter
                .layout_no_wrap(
                    words.clone(),
                    egui::FontId::proportional(ink::LABEL_FONT),
                    style.1,
                )
                .size();
            let mut rect =
                egui::Rect::from_min_size(pos + egui::vec2(8., 4.), size + dimension::PAD * 2.);
            while occupied.iter().any(|r| r.intersects(rect)) {
                rect = rect.translate(egui::vec2(0., rect.height() + 2.));
            }
            occupied.push(rect);
            if marker {
                ink::paint_marker_box(&painter, rect, &words, style);
            } else {
                ink::paint_label_box(&painter, rect, &words, style);
            }
            let pixel = |pos: egui::Pos2| {
                let delta = (pos - at([0., 0.])) * scale;
                [f64::from(delta.x), f64::from(delta.y)]
            };
            let visible = rect.intersect(clip);
            if visible.is_positive() {
                self.labels
                    .push((c.id, [pixel(visible.min), pixel(visible.max)]));
            }
        }
        if let (Some(pending), Some(cursor)) = (draft.pending, self.cursor) {
            let geometry = match pending {
                Pending::Line { last, .. } => SketchGeometry::Line {
                    start: last,
                    end: cursor,
                },
                Pending::Circle(center) => SketchGeometry::Circle {
                    center,
                    radius: (center.x - cursor.x).hypot(center.y - cursor.y),
                },
            };
            draw(
                &geometry,
                colour(SKETCH_SELECTED_COLOUR),
                SKETCH_CONSTRUCTION_STROKE_PIXELS,
            );
        }
        if let Some(point) = self
            .snapped
            .and_then(|p| geometry::screen(camera, plane, p))
        {
            painter.circle_stroke(
                at(point),
                6. / scale,
                egui::Stroke::new(1.5 / scale, colour(SKETCH_HOVER_COLOUR)),
            );
        }
    }
}

fn caption(
    c: &ferritecad_document::SketchConstraint,
    curves: &[ferritecad_document::SketchCurve],
    diameter: bool,
) -> Option<(ferritecad_document::Point2, String, bool)> {
    use crate::sketch::{dimension, marker::Kind};
    use ferritecad_document::{Point2, SketchConstraintRule as R};
    let points = c.rule.points();
    let mut locations = points
        .into_iter()
        .filter_map(|p| rules::position(curves, p))
        .peekable();
    let anchor = if let R::Radius { curve, .. } = c.rule {
        let SketchGeometry::Circle { center, radius } =
            curves.iter().find(|k| k.id == curve)?.geometry
        else {
            return None;
        };
        Point2 {
            x: center.x + radius,
            y: center.y,
        }
    } else {
        locations.peek()?;
        let (sum, n) = locations.fold(([0., 0.], 0.), |([x, y], n), p| {
            ([x + p.x, y + p.y], n + 1.)
        });
        Point2 {
            x: sum[0] / n,
            y: sum[1] / n,
        }
    };
    let mark = match &c.rule {
        R::Fixed { .. } => Kind::Fixed,
        R::Horizontal { .. } => Kind::Horizontal,
        R::Vertical { .. } => Kind::Vertical,
        R::Parallel { .. } => Kind::Parallel,
        R::Perpendicular { .. } => Kind::Perpendicular,
        R::Collinear { .. } => Kind::Collinear,
        R::EqualLength { .. } => Kind::EqualLength,
        R::Coincident { .. } => return Some((anchor, "Coincident".into(), true)),
        R::Distance { distance, .. } => {
            return Some((
                anchor,
                format!("{} mm", dimension::compact(distance.value())),
                false,
            ));
        }
        R::Radius { radius, .. } => {
            let (prefix, value) = if diameter {
                ("Ø", radius.value() * 2.)
            } else {
                ("R", radius.value())
            };
            return Some((
                anchor,
                format!("{prefix} {} mm", dimension::compact(value)),
                false,
            ));
        }
        R::DirectedAngle { angle, .. } => {
            let angle = ferritecad_document::DirectedAngle::from_radians(angle.value()).ok()?;
            return Some((
                anchor,
                format!("A>B {}°", dimension::compact_degrees(angle)),
                false,
            ));
        }
        _ => return None,
    };
    Some((anchor, mark.short().into(), true))
}
