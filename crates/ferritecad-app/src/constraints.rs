// SPDX-License-Identifier: MIT
//! Disposable H/V request over accepted stored facts; no solver or file reads.
use ferritecad_document::{
    AddLineConstraint, ConstraintSketchChoice, DocumentVersion, ExtrudeEditSource,
    LineConstraintKind, SketchConstraintEdits, SketchConstraintRule, SketchGeometry,
};
use ferritecad_jobs::{EditSketchConstraintsRequest, EditedSketchConstraints};
use ferritecad_types::{ObjectId, Result, StableEntityId};
use std::path::{Path, PathBuf};

const HISTORY_LIMIT: usize = 128;

/// Only pending requests belong to history, never selection or solver results.
#[derive(Debug, Clone, Default)]
struct History {
    undo: Vec<SketchConstraintEdits>,
    redo: Vec<SketchConstraintEdits>,
}
impl History {
    fn push(stack: &mut Vec<SketchConstraintEdits>, edits: SketchConstraintEdits) {
        if stack.len() == HISTORY_LIMIT {
            stack.remove(0);
        }
        stack.push(edits);
    }
    fn change(&mut self, edits: &mut SketchConstraintEdits, next: SketchConstraintEdits) {
        if *edits != next {
            Self::push(&mut self.undo, std::mem::replace(edits, next));
            self.redo.clear();
        }
    }
    fn undo(&mut self, edits: &mut SketchConstraintEdits) {
        if let Some(previous) = self.undo.pop() {
            Self::push(&mut self.redo, std::mem::replace(edits, previous));
        }
    }
    fn redo(&mut self, edits: &mut SketchConstraintEdits) {
        if let Some(next) = self.redo.pop() {
            Self::push(&mut self.undo, std::mem::replace(edits, next));
        }
    }
}

#[derive(Debug, Clone)]
struct Draft {
    source: PathBuf,
    version: DocumentVersion,
    choice: ConstraintSketchChoice,
    selected: Option<StableEntityId>,
    edits: SketchConstraintEdits,
    history: History,
    refusal: Option<String>,
}
#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<Draft>,
    pending: Option<EditSketchConstraintsRequest>,
}
impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some()
    }
    pub(crate) fn dismiss(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn take_request(&mut self) -> Option<EditSketchConstraintsRequest> {
        self.pending.take()
    }
    fn begin(&mut self, path: &Path, source: &ExtrudeEditSource, id: ObjectId) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .constraint_sketches
            .iter()
            .find(|c| c.sketch == id && c.refusal.is_none())
        else {
            return false;
        };
        self.draft = Some(Draft {
            source: path.to_path_buf(),
            version: source.version,
            choice: choice.clone(),
            selected: None,
            edits: Default::default(),
            history: Default::default(),
            refusal: None,
        });
        true
    }
    pub(crate) fn choices(
        &mut self,
        ui: &mut egui::Ui,
        can_begin: bool,
        path: Option<&Path>,
        source: Option<&ExtrudeEditSource>,
    ) {
        if self.active() {
            return;
        }
        if let (Some(path), Some(source)) = (path, source) {
            for choice in &source.constraint_sketches {
                let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
                let response = ui.add_enabled(
                    can_begin && refusal.is_none(),
                    egui::Button::new(format!(
                        "Edit H/V {} — {}…",
                        choice.name.as_deref().unwrap_or("Unnamed"),
                        choice.sketch
                    )),
                );
                if response.clicked() {
                    self.begin(path, source, choice.sketch);
                }
                if let Some(reason) = refusal {
                    response.on_hover_text(reason);
                }
            }
        }
    }
    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, running: bool) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        let mut cancel = false;
        egui::Window::new("Sketch H/V constraints — new copy")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label("Select a stored Line. Solver runs only when saving the new copy.");
                ui.small(format!(
                    "Sketch {} · {}",
                    draft.choice.sketch,
                    draft.source.display()
                ));
                ui.label("Coordinates below are stored inputs, not the solved drawing.");
                ui.label(
                    "Missing Coincident joints are added with H/V; closure remains after removal.",
                );
                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Cancel constraints draft").clicked() {
                            cancel = true;
                        }
                        if ui
                            .add_enabled(!draft.history.undo.is_empty(), egui::Button::new("Undo"))
                            .clicked()
                        {
                            draft.history.undo(&mut draft.edits);
                            draft.refusal = None;
                        }
                        if ui
                            .add_enabled(!draft.history.redo.is_empty(), egui::Button::new("Redo"))
                            .clicked()
                        {
                            draft.history.redo(&mut draft.edits);
                            draft.refusal = None;
                        }
                    });
                    let Some(stored) = &draft.choice.stored else {
                        return;
                    };
                    egui::ScrollArea::vertical()
                        .id_salt("constraint-lines")
                        .max_height(150.)
                        .show(ui, |ui| {
                            for (i, c) in stored.curves.iter().enumerate() {
                                if let SketchGeometry::Line { start, end } = c.geometry {
                                    ui.selectable_value(
                                        &mut draft.selected,
                                        Some(c.id),
                                        format!(
                                            "Segment {} · {} · ({}, {}) → ({}, {}) mm",
                                            i + 1,
                                            short(c.id),
                                            start.x,
                                            start.y,
                                            end.x,
                                            end.y
                                        ),
                                    );
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        for (label, kind) in [
                            ("Add Horizontal", LineConstraintKind::Horizontal),
                            ("Add Vertical", LineConstraintKind::Vertical),
                        ] {
                            if ui
                                .add_enabled(draft.selected.is_some(), egui::Button::new(label))
                                .clicked()
                            {
                                let mut proposed = draft.edits.clone();
                                proposed.add.push(AddLineConstraint {
                                    curve: draft.selected.expect("selected"),
                                    kind,
                                });
                                match draft.choice.validate_edits(&proposed) {
                                    Ok(()) => {
                                        draft.history.change(&mut draft.edits, proposed);
                                        draft.refusal = None;
                                    }
                                    Err(e) => draft.refusal = Some(e.to_string()),
                                }
                            }
                        }
                    });
                    ui.label("Persisted constraints:");
                    egui::ScrollArea::vertical()
                        .id_salt("stored-constraints")
                        .max_height(150.)
                        .show(ui, |ui| {
                            for c in &stored.constraints {
                                let (label, curve) = match c.rule {
                                    SketchConstraintRule::Horizontal { a, .. } => {
                                        ("Horizontal", Some(a.curve))
                                    }
                                    SketchConstraintRule::Vertical { a, .. } => {
                                        ("Vertical", Some(a.curve))
                                    }
                                    _ => ("Coincident closure", None),
                                };
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("{label} · {}", c.id));
                                    if let Some(curve) = curve {
                                        ui.label(format!("Line {}", short(curve)));
                                        let mut remove = draft.edits.remove.contains(&c.id);
                                        if ui.checkbox(&mut remove, "Remove").changed() {
                                            let mut proposed = draft.edits.clone();
                                            if remove {
                                                proposed.remove.push(c.id);
                                            } else {
                                                proposed.remove.retain(|id| *id != c.id);
                                            }
                                            draft.history.change(&mut draft.edits, proposed);
                                            draft.refusal = None;
                                        }
                                    }
                                });
                            }
                            if stored.constraints.is_empty() {
                                ui.label("None. Closure links will be explicit in the saved copy.");
                            }
                        });
                    ui.label("Pending additions:");
                    egui::ScrollArea::vertical()
                        .id_salt("pending-constraints")
                        .max_height(100.)
                        .show(ui, |ui| {
                            for add in &draft.edits.add {
                                ui.label(format!("{} · Line {}", kind_name(add.kind), add.curve));
                            }
                        });
                    if ui.button("Clear pending changes").clicked() {
                        draft.history.change(&mut draft.edits, Default::default());
                        draft.refusal = None;
                    }
                    if let Some(refusal) = &draft.refusal {
                        ui.colored_label(ui.visuals().error_fg_color, refusal);
                    }
                    match draft.choice.validate_edits(&draft.edits) {
                        Ok(()) => {
                            if ui.button("Save constraints copy…").clicked() {
                                self.pending = Some(EditSketchConstraintsRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,
                                    sketch: draft.choice.sketch,
                                    edits: draft.edits.clone(),
                                    destination: PathBuf::new(),
                                });
                            }
                        }
                        Err(e) => {
                            ui.colored_label(ui.visuals().error_fg_color, e.to_string());
                        }
                    }
                });
                if running {
                    ui.label("Saving… Draft retained until publication. Cancel job in toolbar.");
                }
            });
        if cancel {
            self.dismiss();
        }
    }
}
fn short(id: StableEntityId) -> String {
    id.to_string()[24..].to_owned()
}
fn kind_name(kind: LineConstraintKind) -> &'static str {
    match kind {
        LineConstraintKind::Horizontal => "Horizontal",
        LineConstraintKind::Vertical => "Vertical",
    }
}

pub(crate) fn finish_edit(
    editor: &mut Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<EditedSketchConstraints>,
) -> Option<PathBuf> {
    if !edits.accepts(generation) {
        return None;
    }
    if result.is_ok() {
        editor.dismiss();
    }
    edits.finish_constraints(generation, result)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use ferritecad_document::Document;
    use ferritecad_kernel::OperationContext;
    fn frame(ctx: &egui::Context, e: &mut Editor, events: Vec<egui::Event>) -> egui::FullOutput {
        frame_running(ctx, e, events, false)
    }
    fn frame_running(
        ctx: &egui::Context,
        e: &mut Editor,
        events: Vec<egui::Event>,
        running: bool,
    ) -> egui::FullOutput {
        let mut o = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(988., 768.),
                )),
                events,
                ..Default::default()
            },
            |ui| e.draw(ui, running),
        );
        o.textures_delta.clear();
        o
    }
    fn click(ctx: &egui::Context, e: &mut Editor, label: &str) {
        click_running(ctx, e, label, false);
    }
    fn click_running(ctx: &egui::Context, e: &mut Editor, label: &str, running: bool) {
        let out = frame_running(ctx, e, vec![], running);
        let at = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text().starts_with(label) => {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("not painted: {label}"));
        frame_running(ctx, e, vec![egui::Event::PointerMoved(at)], running);
        for pressed in [true, false] {
            frame_running(
                ctx,
                e,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
                running,
            );
        }
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("source.fcad");
        ferritecad_jobs::create_document(
            ferritecad_jobs::CreateDocumentRequest::new(
                &path,
                ferritecad_jobs::NewDocument::SamplePlate(Default::default()),
                "test",
            ),
            &OperationContext::default(),
        )
        .expect("source");
        let d = Document::open_read_only(&path).expect("doc");
        let source = ExtrudeEditSource::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }
    fn history_state(
        e: &Editor,
    ) -> (
        SketchConstraintEdits,
        Vec<SketchConstraintEdits>,
        Vec<SketchConstraintEdits>,
    ) {
        let d = e.draft.as_ref().expect("draft");
        (
            d.edits.clone(),
            d.history.undo.clone(),
            d.history.redo.clone(),
        )
    }
    #[test]
    fn constraint_history_widgets_restore_requests_and_branch_only_on_changes() {
        let (_root, path, source) = fixture();
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, source.constraint_sketches[0].sketch));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Clear pending changes");
        assert_eq!(history_state(&e), Default::default());
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Add Horizontal");
        let h = e.draft.as_ref().expect("draft").edits.clone();
        assert_eq!(
            h.add,
            vec![AddLineConstraint {
                curve: source.constraint_sketches[0]
                    .stored
                    .as_ref()
                    .expect("stored")
                    .curves[0]
                    .id,
                kind: LineConstraintKind::Horizontal
            }]
        );
        click(&ctx, &mut e, "Undo");
        let empty_with_redo = history_state(&e);
        assert_eq!(empty_with_redo.0, SketchConstraintEdits::default());
        // No-op Clear and disabled Undo cannot erase the redo branch.
        click(&ctx, &mut e, "Clear pending changes");
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Segment 2");
        assert_eq!(history_state(&e), empty_with_redo);
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, h);
        let after_redo = history_state(&e);
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e), after_redo);
        click(&ctx, &mut e, "Add Vertical");
        let hv = history_state(&e).0;
        assert_eq!(hv.add[0], h.add[0]);
        assert_eq!(hv.add[1].kind, LineConstraintKind::Vertical);
        click(&ctx, &mut e, "Undo");
        let branch = history_state(&e);
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Add Vertical");
        assert!(e.draft.as_ref().expect("draft").refusal.is_some());
        assert_eq!(history_state(&e), branch);
        // Running disables both nonempty stacks and all request mutations.
        for label in [
            "Undo",
            "Redo",
            "Clear pending changes",
            "Add Horizontal",
            "Cancel constraints draft",
            "Save constraints copy…",
        ] {
            click_running(&ctx, &mut e, label, true);
            assert_eq!(history_state(&e), branch);
            assert!(e.take_request().is_none());
        }
        click(&ctx, &mut e, "Redo");
        assert!(e.draft.as_ref().expect("draft").refusal.is_none());
        assert_eq!(history_state(&e).0, hv);
        click(&ctx, &mut e, "Clear pending changes");
        click(&ctx, &mut e, "Undo");
        assert_eq!(history_state(&e).0, hv);
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Segment 3");
        click(&ctx, &mut e, "Add Horizontal");
        let branched = history_state(&e);
        assert_eq!(branched.0.add[0], h.add[0]);
        assert_eq!(
            branched.0.add[1].curve,
            source.constraint_sketches[0]
                .stored
                .as_ref()
                .expect("stored")
                .curves[2]
                .id
        );
        assert!(branched.2.is_empty());
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(
            e.take_request().expect("save current branch").edits,
            branched.0
        );
    }

    #[test]
    fn constraint_history_remove_widgets_restore_exact_persisted_ids() {
        let (_root, path, source) = fixture();
        let choice = &source.constraint_sketches[0];
        let mut document = Document::open(&path).expect("doc");
        let prepared = ferritecad_document::prepare_sketch_constraints(
            &document,
            choice.sketch,
            &SketchConstraintEdits {
                remove: vec![],
                add: vec![AddLineConstraint {
                    curve: choice.stored.as_ref().expect("stored").curves[0].id,
                    kind: LineConstraintKind::Horizontal,
                }],
            },
        )
        .expect("prepare persisted H");
        document.write_sketch_constraints(&prepared).expect("write");
        let source = ExtrudeEditSource::read(&document).expect("catalog");
        document.close().expect("close");
        let h = source.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored")
            .constraints
            .iter()
            .find(|c| matches!(c.rule, SketchConstraintRule::Horizontal { .. }))
            .expect("H")
            .id;
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, choice.sketch));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        click(&ctx, &mut e, "Remove");
        let removed = SketchConstraintEdits {
            remove: vec![h],
            add: vec![],
        };
        assert_eq!(history_state(&e).0, removed);
        click(&ctx, &mut e, "Remove");
        assert_eq!(history_state(&e).0, SketchConstraintEdits::default());
        click(&ctx, &mut e, "Undo");
        assert_eq!(history_state(&e).0, removed);
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, SketchConstraintEdits::default());
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Undo");
        assert_eq!(history_state(&e).0, SketchConstraintEdits::default());
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(e.take_request().expect("exact UUID request").edits, removed);
    }

    #[test]
    fn constraint_history_bounds_both_stacks_and_retains_order() {
        let mut history = History::default();
        let mut edits = SketchConstraintEdits::default();
        let states: Vec<_> = (0..=140)
            .map(|_| SketchConstraintEdits {
                remove: vec![StableEntityId::new(), StableEntityId::new()],
                add: vec![
                    AddLineConstraint {
                        curve: StableEntityId::new(),
                        kind: LineConstraintKind::Vertical,
                    },
                    AddLineConstraint {
                        curve: StableEntityId::new(),
                        kind: LineConstraintKind::Horizontal,
                    },
                ],
            })
            .collect();
        for next in &states {
            history.change(&mut edits, next.clone());
            assert!(history.undo.len() <= HISTORY_LIMIT);
            assert!(history.redo.len() <= HISTORY_LIMIT);
        }
        for expected in states[12..140].iter().rev() {
            history.undo(&mut edits);
            assert_eq!(&edits, expected);
        }
        assert!(history.undo.is_empty());
        assert_eq!(history.redo.len(), HISTORY_LIMIT);
        history.undo(&mut edits);
        history.change(&mut edits, states[12].clone());
        assert_eq!(history.redo.len(), HISTORY_LIMIT, "no-op preserves redo");
        for expected in &states[13..] {
            history.redo(&mut edits);
            assert_eq!(&edits, expected);
            assert!(history.undo.len() <= HISTORY_LIMIT);
        }
        assert_eq!(history.undo.len(), HISTORY_LIMIT);
        assert!(history.redo.is_empty());
    }
    #[test]
    fn constraint_editor_keeps_actions_reachable_with_many_pending_additions() {
        for constrained in [false, true] {
            let (_root, path, _) = fixture();
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
            let points: Vec<_> = (0..64)
                .map(|i| {
                    let angle = f64::from(i) * std::f64::consts::TAU / 64.;
                    ferritecad_document::Point2::new(40. * angle.cos(), 40. * angle.sin())
                        .expect("point")
                })
                .collect();
            sketch.curves = (0..64)
                .map(|i| ferritecad_document::SketchCurve {
                    id: old_ids.get(i).copied().unwrap_or_else(StableEntityId::new),
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: points[i],
                        end: points[(i + 1) % 64],
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
                let ferritecad_document::ObjectPayload::Sketch(sketch) = &object.payload else {
                    unreachable!()
                };
                let prepared = ferritecad_document::prepare_sketch_constraints(
                    &document,
                    object.id,
                    &SketchConstraintEdits {
                        remove: vec![],
                        add: vec![AddLineConstraint {
                            curve: sketch.curves.last().expect("curve").id,
                            kind: LineConstraintKind::Horizontal,
                        }],
                    },
                )
                .expect("persisted H and all closure links");
                document
                    .write_sketch_constraints(&prepared)
                    .expect("constraints");
            }
            let source = ExtrudeEditSource::read(&document).expect("discovery");
            document.close().expect("close");
            let mut e = Editor::default();
            assert!(e.begin(&path, &source, object.id));
            let draft = e.draft.as_mut().expect("draft");
            draft.edits.add = draft
                .choice
                .stored
                .as_ref()
                .expect("supported")
                .curves
                .iter()
                .step_by(2)
                .map(|c| AddLineConstraint {
                    curve: c.id,
                    kind: LineConstraintKind::Horizontal,
                })
                .collect();
            draft
                .choice
                .validate_edits(&draft.edits)
                .expect("valid request");
            let ctx = egui::Context::default();
            for _ in 0..3 {
                frame(&ctx, &mut e, vec![]);
            }
            let out = frame(&ctx, &mut e, vec![]);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(988., 768.));
            for label in [
                "Cancel constraints draft",
                "Undo",
                "Redo",
                "Clear pending changes",
                "Save constraints copy…",
            ] {
                assert!(
                    out.shapes.iter().any(|s| matches!(&s.shape,
                        egui::Shape::Text(t) if t.galley.text() == label
                            && s.clip_rect.contains_rect(t.visual_bounding_rect())
                            && screen.contains_rect(t.visual_bounding_rect())
                    )),
                    "{label} must remain visible and reachable with many pending additions"
                );
            }
            let pending = e.draft.as_ref().expect("draft").edits.clone();
            click(&ctx, &mut e, "Save constraints copy…");
            assert_eq!(
                e.take_request().expect("reachable Save").edits.add.len(),
                32
            );
            click(&ctx, &mut e, "Clear pending changes");
            assert!(e.draft.as_ref().expect("draft").edits.add.is_empty());
            click(&ctx, &mut e, "Undo");
            assert_eq!(e.draft.as_ref().expect("draft").edits, pending);
            click(&ctx, &mut e, "Redo");
            assert_eq!(
                e.draft.as_ref().expect("draft").edits,
                SketchConstraintEdits::default()
            );
            click(&ctx, &mut e, "Undo");
            click(&ctx, &mut e, "Save constraints copy…");
            assert_eq!(e.take_request().expect("restored Save").edits, pending);
        }
    }

    #[test]
    fn constraint_widgets_keep_draft_on_save_cancel_refusal_and_stale_reply() {
        let (_root, path, source) = fixture();
        let mut e = Editor::default();
        let id = source.constraint_sketches[0].sketch;
        assert!(!e.begin(&path, &source, ObjectId::new()));
        assert!(e.begin(&path, &source, id));
        let ctx = egui::Context::default();
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Add Horizontal");
        let kept = e.draft.as_ref().expect("draft").edits.clone();
        assert_eq!(kept.add.len(), 1);
        click(&ctx, &mut e, "Segment 2");
        click(&ctx, &mut e, "Add Vertical");
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Segment 1");
        let history = history_state(&e);
        assert!(!history.1.is_empty() && !history.2.is_empty());
        click(&ctx, &mut e, "Add Vertical");
        assert!(e.draft.as_ref().expect("draft").refusal.is_some());
        assert_eq!(e.draft.as_ref().expect("draft").edits, kept);
        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("submit");
        assert_eq!(request.edits, kept);
        assert_eq!(request.expected, source.version);
        assert!(e.take_request().is_none());
        // Save Cancel returns no path: no worker is started and this same draft is retryable.
        assert_eq!(e.draft.as_ref().expect("retained").edits, kept);
        assert_eq!(history_state(&e), history);
        let mut state = crate::edits::Edits::default();
        for cancelled in [false, true] {
            let (tx, rx) = std::sync::mpsc::channel();
            let generation = state
                .start_constraints(request.clone(), move |_, g, c| {
                    std::thread::spawn(move || {
                        if cancelled {
                            c.cancel();
                        }
                        tx.send(g).expect("reply")
                    })
                })
                .expect("start");
            assert!(
                state
                    .start_constraints(request.clone(), |_, _, _| panic!("duplicate"))
                    .is_none()
            );
            let error = if cancelled {
                ferritecad_types::CadError::Cancelled
            } else {
                ferritecad_types::CadError::topology("lost stored reference")
            };
            assert!(
                finish_edit(
                    &mut e,
                    &mut state,
                    generation + 1,
                    Err(ferritecad_types::CadError::input("stale"))
                )
                .is_none()
            );
            assert_eq!(e.draft.as_ref().expect("retained").edits, kept);
            assert_eq!(history_state(&e), history);
            let g = rx.recv().expect("done");
            assert!(finish_edit(&mut e, &mut state, g, Err(error)).is_none());
            assert_eq!(e.draft.as_ref().expect("retained").edits, kept);
            assert_eq!(history_state(&e), history);
        }
        click(&ctx, &mut e, "Clear pending changes");
        assert!(e.draft.as_ref().expect("draft").edits.add.is_empty());
        click(&ctx, &mut e, "Cancel constraints draft");
        assert!(!e.active());
        assert!(e.begin(&path, &source, id));
        assert_eq!(history_state(&e), Default::default());
        e.dismiss();
        let mut forbidden = source.clone();
        forbidden.refusal = Some("document copy forbidden".into());
        assert!(!e.begin(&path, &forbidden, id));
    }
    #[test]
    fn native_constraint_worker_and_cli_preserve_model_and_solved_body() {
        use crate::creates::tests::ferritecad;
        if !ferritecad_occt::is_available() || !ferritecad_sketch_solver::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: constraint worker requires OCCT and PlaneGCS");
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let source = root.path().join("source.fcad");
        let input = root.path().join("request.json");
        std::fs::write(&input,r#"{"request_version":1,"points_mm":[[-20,-10],[40,-8],[42,30],[-20,30]],"height_mm":10}"#).expect("polygon");
        let o = std::process::Command::new(ferritecad())
            .arg("create-sketch-extrude")
            .arg(&input)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(o.status.success(), "{o:?}");
        crate::sketch::drag_tests::attach_source_claim(&source);
        let sql_before = crate::sketch::drag_tests::sql_facts(&source);
        // Same accepted native snapshot used by the real UI; immutable while the draft exists.
        let loaded = {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                &source,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("scene")
        };
        let reading = loaded.edit_source.expect("catalog");
        let id = reading.constraint_sketches[0].sketch;
        let original = std::fs::read(&source).expect("source");
        let mut e = Editor::default();
        assert!(e.begin(&source, &reading, id));
        let ctx = egui::Context::default();
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Add Horizontal");
        let expected_edits = e.draft.as_ref().expect("draft").edits.clone();
        click(&ctx, &mut e, "Undo");
        assert_eq!(
            e.draft.as_ref().expect("draft").edits,
            SketchConstraintEdits::default()
        );
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("real widget request");
        assert_eq!(request.edits, expected_edits);
        assert_eq!(request.source, source);
        assert_eq!(request.sketch, id);
        assert_eq!(request.expected, reading.version);
        let kept = request.edits.clone();
        let history = history_state(&e);
        let mut state = crate::edits::Edits::default();
        for occupied in [true, false] {
            let mut r = request.clone();
            r.destination = root
                .path()
                .join(if occupied { "busy.fcad" } else { "ui.fcad" });
            if occupied {
                std::fs::write(&r.destination, b"keep").expect("busy");
            }
            let (tx, rx) = std::sync::mpsc::channel();
            state
                .start_constraints(r, move |r, g, c| {
                    crate::edits::spawn_constraint_edit(r, c, move |result| {
                        tx.send((g, result)).expect("reply")
                    })
                })
                .expect("worker");
            let (g, result) = rx.recv().expect("worker reply");
            let open = finish_edit(&mut e, &mut state, g, result);
            if occupied {
                assert!(open.is_none());
                assert_eq!(e.draft.as_ref().expect("retained").edits, kept);
                assert_eq!(history_state(&e), history);
                assert_eq!(
                    std::fs::read(root.path().join("busy.fcad")).expect("busy"),
                    b"keep"
                );
            } else {
                assert_eq!(open, Some(root.path().join("ui.fcad")));
                assert!(!e.active());
            }
        }
        std::fs::write(&input,format!(r#"{{"request_version":1,"remove":[],"add":[{{"curve_id":"{}","rule":"horizontal"}}]}}"#,kept.add[0].curve)).expect("typed IDs to peer request");
        let cli = root.path().join("cli.fcad");
        let ui = root.path().join("ui.fcad");
        let o = std::process::Command::new(ferritecad())
            .arg("edit-sketch-constraints-copy")
            .arg(&source)
            .arg("--sketch")
            .arg(id.to_string())
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(o.status.success(), "{o:?}");
        let a = Document::open_read_only(&ui).expect("UI");
        let b = Document::open_read_only(&cli).expect("CLI");
        assert_eq!(a.meta(), b.meta());
        assert_eq!(
            a.dependencies().expect("deps"),
            b.dependencies().expect("deps")
        );
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
        for old in a.objects().expect("objects") {
            let new = b.object(old.id).expect("read").expect("same id");
            if let (
                ferritecad_document::ObjectPayload::Sketch(s),
                ferritecad_document::ObjectPayload::Sketch(t),
            ) = (&old.payload, &new.payload)
            {
                assert_eq!(s.curves, t.curves);
                assert_eq!(s.plane, t.plane);
                assert_eq!(s.constraints.len(), 5);
                for (x, y) in s.constraints.iter().zip(&t.constraints) {
                    assert_ne!(x.id, y.id);
                    assert_eq!(x.rule, y.rule);
                }
            } else {
                assert_eq!(old, new);
            }
        }
        a.close().expect("close");
        b.close().expect("close");
        for path in [&ui, &cli] {
            let after = crate::sketch::drag_tests::sql_facts(path);
            assert_eq!(
                sql_before.keys().collect::<Vec<_>>(),
                after.keys().collect::<Vec<_>>()
            );
            for (table, rows) in &sql_before {
                if table == "objects" {
                    for row in rows {
                        let actual = after[table]
                            .iter()
                            .find(|r| r[1] == row[1])
                            .expect("same row");
                        for col in 0..row.len() {
                            if row[1] == rusqlite::types::Value::Blob(id.to_bytes().to_vec())
                                && [3, 7, 8].contains(&col)
                            {
                                continue;
                            }
                            assert_eq!(row[col], actual[col], "object cell {col}");
                        }
                    }
                } else if table == "capabilities" {
                    assert_eq!(
                        after[table]
                            .iter()
                            .filter(|r| r[1]
                                != rusqlite::types::Value::Text(
                                    ferritecad_document::SKETCH_CONSTRAINTS_CAPABILITY.into()
                                ))
                            .cloned()
                            .collect::<Vec<_>>(),
                        *rows
                    );
                } else {
                    assert_eq!(
                        &after[table], rows,
                        "{table}: source claims and all unrelated rows"
                    );
                }
            }
        }

        let mut outputs = vec![];
        for path in [&ui, &cli] {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            let snap = ferritecad_scene::snapshot_of(
                path,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("async Open route");
            assert!(
                snap.edit_source.expect("facts").constraint_sketches[0]
                    .refusal
                    .is_none()
            );
            let stl = path.with_extension("stl");
            let o = std::process::Command::new(ferritecad())
                .arg("export-stl")
                .arg(path)
                .arg("-o")
                .arg(&stl)
                .output()
                .expect("STL");
            assert!(o.status.success(), "{o:?}");
            outputs.push(std::fs::read(&stl).expect("STL"));
            if let Some(dir) = std::env::var_os("FERRITECAD_CONSTRAINT_ARTIFACTS") {
                std::fs::create_dir_all(&dir).expect("dir");
                std::fs::copy(path, Path::new(&dir).join(path.file_name().expect("name")))
                    .expect("artifact");
            }
        }
        assert_eq!(
            outputs[0], outputs[1],
            "independent UUIDs do not alter solved Body; CLI process gate integrates these triangles independently"
        );
        assert_eq!(std::fs::read(&source).expect("source"), original);
        // Exact persisted H is removable through the same widgets after publication.
        let d = Document::open_read_only(&ui).expect("published");
        let after = ExtrudeEditSource::read(&d).expect("discover");
        d.close().expect("close");
        assert!(e.begin(&ui, &after, id));
        assert_eq!(
            history_state(&e),
            Default::default(),
            "published draft history is discarded"
        );
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, "Remove");
        click(&ctx, &mut e, "Save constraints copy…");
        let remove = e.take_request().expect("removal");
        assert_eq!(
            remove.edits.remove,
            vec![
                after.constraint_sketches[0]
                    .stored
                    .as_ref()
                    .expect("sketch")
                    .constraints[4]
                    .id
            ]
        );
        assert!(remove.edits.add.is_empty());
    }
}
