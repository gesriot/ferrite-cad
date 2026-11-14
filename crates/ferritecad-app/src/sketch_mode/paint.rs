// SPDX-License-Identifier: MIT
use super::{Editor, Pending, geometry};
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
    pub(crate) fn paint(&self, ctx: &egui::Context, camera: &Camera, region: PixelRect) {
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
            let rgb = if draft.state.selected.contains(&curve.id) {
                SKETCH_SELECTED_COLOUR
            } else if self.hovered == Some(curve.id) {
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
