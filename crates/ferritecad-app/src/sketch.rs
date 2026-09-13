// SPDX-License-Identifier: MIT
//! A disposable drawing, separate from the accepted scene and persisted model.
//! No kernel, filesystem, IDs, or document mutation occurs while editing it.
use ferritecad_jobs::{NewDocument, PolygonExtrusion};
use ferritecad_types::{CadError, Result};

#[derive(Debug, Clone, PartialEq)]
struct State {
    points: Vec<[String; 2]>,
    closed: bool,
    height: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            closed: false,
            height: "10".into(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<State>,
    undo: Vec<State>,
    redo: Vec<State>,
    next: [String; 2],
    pending: Option<NewDocument>,
    canvas: Canvas,
}
impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some()
    }
    pub(crate) fn dismiss(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn take_request(&mut self) -> Option<NewDocument> {
        self.pending.take()
    }
    fn begin(&mut self) {
        self.dismiss();
        self.draft = Some(State::default());
        self.next = ["0".into(), "0".into()];
    }
    fn record(&mut self, before: State) {
        if self.draft.as_ref() != Some(&before) {
            if self.undo.len() == 128 {
                self.undo.remove(0);
            }
            self.undo.push(before);
            self.redo.clear();
        }
    }
    fn undo(&mut self) {
        if let Some(previous) = self.undo.pop()
            && let Some(current) = self.draft.replace(previous)
        {
            self.redo.push(current);
        }
    }
    fn redo(&mut self) {
        if let Some(next) = self.redo.pop()
            && let Some(current) = self.draft.replace(next)
        {
            self.undo.push(current);
        }
    }
    fn content(&self) -> Result<NewDocument> {
        let draft = self
            .draft
            .as_ref()
            .ok_or_else(|| CadError::input("no sketch draft"))?;
        if !draft.closed {
            return Err(CadError::input(
                "Close the contour explicitly before creating.",
            ));
        }
        let points = draft
            .points
            .iter()
            .map(|p| Ok([number(&p[0])?, number(&p[1])?]))
            .collect::<Result<_>>()?;
        Ok(NewDocument::SketchExtrude(PolygonExtrusion::new(
            points,
            number(&draft.height)?,
        )?))
    }
    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, can_begin: bool, running: bool) {
        if !self.active() {
            if ui
                .add_enabled(can_begin, egui::Button::new("Create sketch + Extrude…"))
                .clicked()
            {
                self.begin();
            }
            return;
        }
        egui::Window::new("Sketch + Extrude — new document")
            .resizable(false)
            .default_width(540.)
            .show(ui.ctx(), |ui| self.draw_draft(ui, running));
    }

    fn draw_draft(&mut self, ui: &mut egui::Ui, running: bool) {
        ui.label("XY · mm · Line polygon · Blind · NewBody");
        ui.label(
            "Click to add vertices, or enter exact coordinates. Last edge closes to vertex 1.",
        );
        ui.add_enabled_ui(!running, |ui| self.edit(ui));
        if running {
            ui.label("Creating… Draft retained until publication. Cancel job in toolbar.");
        }
        ui.small("Draft undo ends at publication. Editing a saved sketch is not available yet.");
    }

    fn edit(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo draft"))
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo draft"))
                .clicked()
            {
                self.redo();
            }
            if ui.button("Cancel draft").clicked() {
                self.dismiss();
            }
        });
        let Some(before) = self.draft.clone() else {
            return;
        };
        let draft = self.draft.as_mut().expect("present");
        self.canvas.draw(ui, draft);
        ui.horizontal(|ui| {
            ui.label("Next X");
            ui.add(
                egui::TextEdit::singleline(&mut self.next[0])
                    .char_limit(64)
                    .desired_width(75.),
            );
            ui.label("Y");
            ui.add(
                egui::TextEdit::singleline(&mut self.next[1])
                    .char_limit(64)
                    .desired_width(75.),
            );
            if ui
                .add_enabled(
                    !draft.closed && draft.points.len() < PolygonExtrusion::MAX_POINTS,
                    egui::Button::new("Add point"),
                )
                .clicked()
            {
                draft.points.push(self.next.clone());
            }
            if ui
                .add_enabled(!draft.closed, egui::Button::new("Close contour"))
                .clicked()
            {
                draft.closed = true;
            }
        });
        egui::ScrollArea::vertical()
            .max_height(160.)
            .show(ui, |ui| {
                let mut remove = None;
                for (i, p) in draft.points.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}  X mm", i + 1));
                        ui.add(
                            egui::TextEdit::singleline(&mut p[0])
                                .char_limit(64)
                                .desired_width(125.),
                        );
                        ui.label("Y mm");
                        ui.add(
                            egui::TextEdit::singleline(&mut p[1])
                                .char_limit(64)
                                .desired_width(125.),
                        );
                        if ui.button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    draft.points.remove(i);
                }
            });
        ui.horizontal(|ui| {
            ui.label("Blind height mm");
            ui.add(
                egui::TextEdit::singleline(&mut draft.height)
                    .char_limit(64)
                    .desired_width(100.),
            );
        });
        self.record(before);
        match self.content() {
            Ok(content) => {
                if ui.button("Create in new file…").clicked() {
                    self.pending = Some(content);
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        }
    }
}

/// Drawing coordinates are a view of numbers, never a source of modelling rules.
#[derive(Debug)]
struct Canvas {
    minimum: [f64; 2],
    scale: f32,
}
impl Default for Canvas {
    fn default() -> Self {
        Self {
            minimum: [-8.75, -7.5],
            scale: 4.0,
        }
    }
}
impl Canvas {
    fn fit(&mut self, points: &[[f64; 2]]) {
        if points.is_empty() {
            return;
        }
        let mut lo = points[0];
        let mut hi = points[0];
        for p in points {
            for j in 0..2 {
                lo[j] = lo[j].min(p[j]);
                hi[j] = hi[j].max(p[j]);
            }
        }
        let width = (hi[0] - lo[0]).max(1e-5);
        let height = (hi[1] - lo[1]).max(1e-5);
        self.scale = (450.0 / width).min(190.0 / height) as f32;
        self.minimum = [
            lo[0] - 30.0 / f64::from(self.scale),
            lo[1] - 30.0 / f64::from(self.scale),
        ];
    }
    fn draw(&mut self, ui: &mut egui::Ui, draft: &mut State) {
        let points: Option<Vec<_>> = draft
            .points
            .iter()
            .map(|p| {
                Some([
                    p[0].trim().parse::<f64>().ok()?,
                    p[1].trim().parse::<f64>().ok()?,
                ])
            })
            .collect();
        let points = points.filter(|p| p.iter().flatten().all(|x| x.is_finite() && x.abs() <= 1e6));
        ui.horizontal(|ui| {
            if ui.button("Fit drawing").clicked()
                && let Some(p) = &points
            {
                self.fit(p);
            }
            if ui.button("Reset view").clicked() {
                *self = Self::default();
            }
            ui.label("+X right · +Y up · coordinates in mm");
        });
        let (response, painter) = ui.allocate_painter(egui::vec2(510., 250.), egui::Sense::click());
        let rect = response.rect;
        let origin = rect.left_bottom();
        let screen = |p: [f64; 2]| {
            origin
                + egui::vec2(
                    ((p[0] - self.minimum[0]) * f64::from(self.scale)) as f32,
                    -((p[1] - self.minimum[1]) * f64::from(self.scale)) as f32,
                )
        };
        painter.rect_filled(rect, 0., egui::Color32::from_gray(25));
        // At most ~50 lines, even for very small or far-translated drawings.
        let grid = 10_f64.powf((40.0 / f64::from(self.scale)).log10().floor());
        for axis in 0..2 {
            for i in 0..55 {
                let value = (self.minimum[axis] / grid).ceil() * grid + f64::from(i) * grid;
                let mut p = self.minimum;
                p[axis] = value;
                let start = screen(p);
                let end = if axis == 0 {
                    egui::pos2(start.x, rect.top())
                } else {
                    egui::pos2(rect.right(), start.y)
                };
                painter.line_segment([start, end], (1., egui::Color32::from_gray(48)));
            }
        }
        painter.text(
            rect.left_top() + egui::vec2(5., 5.),
            egui::Align2::LEFT_TOP,
            format!("grid: {grid} mm"),
            egui::FontId::proportional(12.),
            egui::Color32::WHITE,
        );
        if response.clicked()
            && !draft.closed
            && draft.points.len() < PolygonExtrusion::MAX_POINTS
            && let Some(pos) = response.interact_pointer_pos()
        {
            let offset = to_document(pos, origin, self.scale);
            draft.points.push([
                format!("{:.3}", self.minimum[0] + offset[0]),
                format!("{:.3}", self.minimum[1] + offset[1]),
            ]);
        }
        if let Some(points) = points {
            for pair in points.windows(2) {
                painter.line_segment(
                    [screen(pair[0]), screen(pair[1])],
                    (2., egui::Color32::LIGHT_BLUE),
                );
            }
            if draft.closed && points.len() > 1 {
                painter.line_segment(
                    [screen(points[points.len() - 1]), screen(points[0])],
                    (2., egui::Color32::LIGHT_BLUE),
                );
            }
            for (i, p) in points.iter().enumerate() {
                let p = screen(*p);
                painter.circle_filled(p, 3., egui::Color32::WHITE);
                painter.text(
                    p + egui::vec2(4., -4.),
                    egui::Align2::LEFT_BOTTOM,
                    (i + 1).to_string(),
                    egui::FontId::proportional(12.),
                    egui::Color32::WHITE,
                );
            }
        }
    }
}
fn number(s: &str) -> Result<f64> {
    s.trim()
        .parse()
        .map_err(|_| CadError::input(format!("Not a number: {s:?}")))
}
fn to_document(pos: egui::Pos2, origin: egui::Pos2, scale: f32) -> [f64; 2] {
    [
        f64::from((pos.x - origin.x) / scale),
        f64::from((origin.y - pos.y) / scale),
    ]
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    #[test]
    fn draft_undo_redo_closure_and_shared_validity() {
        let mut e = Editor::default();
        e.begin();
        let initial = e.draft.clone().expect("draft");
        e.draft.as_mut().expect("draft").points = vec![
            ["0".into(), "0".into()],
            ["60".into(), "0".into()],
            ["0".into(), "40".into()],
        ];
        e.record(initial.clone());
        assert!(e.content().is_err());
        let open = e.draft.clone().expect("draft");
        e.draft.as_mut().expect("draft").closed = true;
        e.record(open.clone());
        assert!(e.content().is_ok());
        e.undo();
        assert_eq!(e.draft, Some(open));
        e.redo();
        assert!(e.content().is_ok());
        e.undo();
        e.undo();
        assert_eq!(e.draft, Some(initial));
        e.redo();
        e.redo();
        e.draft.as_mut().expect("draft").points[2] = ["60".into(), "0".into()];
        assert!(e.content().is_err());
        e.dismiss();
        assert!(!e.active());
        assert_eq!(
            to_document(egui::pos2(90., 60.), egui::pos2(10., 100.), 4.),
            [20., 10.]
        );
    }

    fn frame(ctx: &egui::Context, e: &mut Editor, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                events,
                ..Default::default()
            },
            |ui| e.draw(ui, true, false),
        );
        output.textures_delta.clear();
        output
    }
    fn text_at(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    assert!(
                        c.clip_rect
                            .expand(1.)
                            .contains_rect(t.visual_bounding_rect()),
                        "clipped {label}"
                    );
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("button {label} was not painted"))
    }
    fn click(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
        frame(ctx, e, vec![egui::Event::PointerMoved(at)]);
        for pressed in [true, false] {
            frame(
                ctx,
                e,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
            );
        }
    }
    fn replace_field(ctx: &egui::Context, e: &mut Editor, label: &str, value: &str) {
        let out = frame(ctx, e, vec![]);
        click(ctx, e, text_at(&out, label));
        frame(
            ctx,
            e,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Text(value.into()),
            ],
        );
    }
    /// Real egui widgets and pointer input, without winit/GPU or modal panels.
    fn draw_l_through_widgets(e: &mut Editor) -> NewDocument {
        let ctx = egui::Context::default();
        let out = frame(&ctx, e, vec![]);
        let at = text_at(&out, "Create sketch + Extrude…");
        click(&ctx, e, at);
        frame(&ctx, e, vec![]); // let the new window choose its initial position
        for [x, y] in [
            [0., 0.],
            [60., 0.],
            [60., 20.],
            [20., 20.],
            [20., 40.],
            [0., 40.],
        ] {
            let out = frame(&ctx, e, vec![]);
            let rect = out
                .shapes
                .iter()
                .find_map(|c| match &c.shape {
                    egui::Shape::Rect(r)
                        if (r.rect.width() - 510.).abs() < 1.
                            && (r.rect.height() - 250.).abs() < 1. =>
                    {
                        Some(r.rect)
                    }
                    _ => None,
                })
                .expect("visible drawing canvas");
            click(
                &ctx,
                e,
                rect.left_bottom() + egui::vec2(35. + 4. * x, -30. - 4. * y),
            );
        }
        assert!(!e.draft.as_ref().expect("draft").closed);
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Close contour"));
        assert!(e.draft.as_ref().expect("draft").closed);
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Undo draft"));
        assert!(!e.draft.as_ref().expect("draft").closed);
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Redo draft"));
        assert!(e.draft.as_ref().expect("draft").closed);
        replace_field(&ctx, e, "10", "12");
        assert_eq!(e.draft.as_ref().expect("draft").height, "12");
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Undo draft"));
        assert_eq!(e.draft.as_ref().expect("draft").height, "10");
        replace_field(&ctx, e, "60.000", "NaN");
        assert!(e.content().is_err());
        assert!(e.take_request().is_none());
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Undo draft"));
        assert!(e.content().is_ok());
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Create in new file…"));
        e.take_request().expect("real button submitted one request")
    }
    #[test]
    fn draft_widgets_submit_exact_mouse_coordinates_with_explicit_closure() {
        let mut e = Editor::default();
        let NewDocument::SketchExtrude(p) = draw_l_through_widgets(&mut e) else {
            panic!("wrong request")
        };
        assert_eq!(
            p.points().iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
            [
                [0., 0.],
                [60., 0.],
                [60., 20.],
                [20., 20.],
                [20., 40.],
                [0., 40.]
            ]
        );
        assert_eq!(p.height_mm(), 10.);
        assert!(e.take_request().is_none());
    }
    #[test]
    fn native_sketch_draft_and_cli_publish_equivalent_models() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for sketch UI worker");
            return;
        }
        use crate::creates::{
            self,
            tests::{ferritecad, read_semantics},
        };
        use std::sync::mpsc;
        let d = tempfile::tempdir().expect("dir");
        let ui = d.path().join("ui.fcad");
        let cli = d.path().join("cli.fcad");
        let input = d.path().join("request.json");
        let mut creates = creates::Creates::default();
        let content = draw_l_through_widgets(&mut creates.sketch);
        let before = creates.sketch.draft.clone();
        let mut view = ferritecad_ui::ViewportInput::new();
        let loads = crate::Loads::default();
        let exports = crate::exports::Exports::default();
        // Cancelled/failed save dialogs both provide no destination; the exact
        // post-dialog UI route must leave the draft and accepted scene alone.
        assert!(
            crate::start_new(
                &mut creates,
                &loads,
                &exports,
                &mut view,
                content.clone(),
                None,
                |_, _, _, _| panic!("no worker on cancel")
            )
            .is_none()
        );
        assert_eq!(creates.sketch.draft, before);
        let busy = d.path().join("occupied.fcad");
        std::fs::write(&busy, b"keep").expect("busy");
        let (_, open) = creates::tests::run_to_completion(
            &mut creates,
            &mut view,
            content.clone(),
            Some(busy.clone()),
        );
        assert!(open.is_none());
        assert_eq!(creates.sketch.draft, before);
        assert_eq!(std::fs::read(busy).expect("busy"), b"keep");
        let (tx, rx) = mpsc::channel();
        let spawn = move |path: &std::path::Path,
                          content,
                          generation,
                          cancel: &ferritecad_kernel::CancelToken| {
            let path = path.to_path_buf();
            let ctx = ferritecad_kernel::OperationContext::default().with_cancel(cancel.clone());
            creates::spawn_create(
                move || creates::run_create(&path, content, &ctx),
                move |result| tx.send((generation, result)).expect("reply"),
            )
        };
        crate::start_new(
            &mut creates,
            &loads,
            &exports,
            &mut view,
            content.clone(),
            Some(ui.clone()),
            spawn,
        )
        .expect("worker");
        assert!(
            crate::start_new(
                &mut creates,
                &loads,
                &exports,
                &mut view,
                content,
                Some(ui.clone()),
                |_, _, _, _| panic!("duplicate worker")
            )
            .is_none()
        );
        let (generation, result) = rx.recv().expect("worker result");
        assert_eq!(
            creates::finish_create(&mut creates, &mut view, generation, result),
            Some(ui.clone())
        );
        assert!(!creates.sketch.active());
        creates.stop_all();
        std::fs::write(&input,r#"{"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10}"#).expect("request");
        let run = std::process::Command::new(ferritecad())
            .arg("create-sketch-extrude")
            .arg(input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(run.status.success(), "{run:?}");
        assert_eq!(read_semantics(&ui).0, read_semantics(&cli).0);
        assert_ne!(read_semantics(&ui).1, read_semantics(&cli).1);
        // Independent UUIDs must not leak into the geometric comparison.
        let mut bytes = Vec::new();
        for path in [&ui, &cli] {
            let out = path.with_extension("stl");
            let result = std::process::Command::new(ferritecad())
                .arg("export-stl")
                .arg(path)
                .arg("-o")
                .arg(&out)
                .output()
                .expect("export");
            assert!(result.status.success(), "{result:?}");
            bytes.push(std::fs::read(out).expect("STL"));
        }
        assert_eq!(bytes[0], bytes[1]);
    }
}
