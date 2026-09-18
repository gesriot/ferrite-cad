// SPDX-License-Identifier: MIT
//! A disposable circular-cut request over stored facts; no kernel, no file reads.
//!
//! The form offers a cut only where the document says one can be made, and it
//! says what it is doing rather than implying it: the tool is drawn on the
//! part's own base XY datum and runs along that datum's normal. There is no
//! face to pick here and no attachment to choose, because this slice implements
//! neither, and a control that suggested otherwise would be promising it.
use ferritecad_document::{
    CircularCut, CircularCutEdit, CutChoice, CutParameterChoice, DocumentVersion, ExtrudeEditSource,
};
use ferritecad_jobs::{CircularCutRequest, EditCircularCutRequest};
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};
use std::path::{Path, PathBuf};

const HISTORY_LIMIT: usize = 128;

/// What the form holds, as text, before any of it is a number.
///
/// Text rather than parsed values for the reason every numeric form here keeps
/// text: a half-typed "-" is not a number and must not become one, and it must
/// not enter the history either.
#[derive(Debug, Clone, Default, PartialEq)]
struct Numbers {
    center_x: String,
    center_y: String,
    radius: String,
    depth: String,
}

/// Only applied requests belong to history, never half-typed input.
#[derive(Debug, Clone, Default)]
struct History {
    undo: Vec<Numbers>,
    redo: Vec<Numbers>,
}
impl History {
    fn push(stack: &mut Vec<Numbers>, state: Numbers) {
        if stack.len() == HISTORY_LIMIT {
            stack.remove(0);
        }
        stack.push(state);
    }
    fn change(&mut self, applied: &mut Numbers, next: Numbers) {
        if *applied != next {
            Self::push(&mut self.undo, std::mem::replace(applied, next));
            self.redo.clear();
        }
    }
    fn undo(&mut self, applied: &mut Numbers) {
        if let Some(previous) = self.undo.pop() {
            Self::push(&mut self.redo, std::mem::replace(applied, previous));
        }
    }
    fn redo(&mut self, applied: &mut Numbers) {
        if let Some(next) = self.redo.pop() {
            Self::push(&mut self.undo, std::mem::replace(applied, next));
        }
    }
}

/// Which of the two operations the open draft is for.
///
/// One draft, one window and one history for both: a cut's four numbers mean
/// the same thing whether they are being added to a body or changed on a cut
/// that is already in one, and two forms would be two places for the rule about
/// what an Apply is.
#[derive(Debug, Clone)]
enum Subject {
    Add(CutChoice),
    /// Boxed: a saved cut carries every identity it keeps, and an enum sized
    /// for it would make the ordinary add draft carry the difference.
    Edit(Box<CutParameterChoice>),
}

impl Subject {
    /// The same numeric rule as the job, with no SQLite and no kernel work.
    fn validate(&self, typed: &Numbers) -> Result<()> {
        match self {
            Self::Add(choice) => choice.validate_cut(&numbers(typed)?),
            Self::Edit(choice) => {
                let saved = choice
                    .saved
                    .as_ref()
                    .ok_or_else(|| CadError::input("unsupported cut to edit"))?;
                let cut = numbers(typed)?;
                choice.validate_edit(&CircularCutEdit {
                    tool_curve: saved.tool_curve,
                    center_mm: cut.center_mm,
                    radius_mm: cut.radius_mm,
                    depth_mm: cut.depth_mm,
                })
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Draft {
    source: PathBuf,
    version: DocumentVersion,
    subject: Subject,
    /// The saved numbers for an edit, or empty fields for a new cut. Apply is
    /// still required, but its first Undo returns to this starting point.
    initial: Numbers,
    typed: Numbers,
    /// The last set of numbers a confirmed Apply accepted. One Apply is one
    /// history step over all four, exactly as the annulus editor treats its
    /// three, because they describe one tool and half of one is not a cut.
    applied: Option<Numbers>,
    history: History,
    refusal: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<Draft>,
    pending: Option<CircularCutRequest>,
    /// The edit's own pending request beside the addition's. Two fields rather
    /// than one enum, so the window keeps handing each operation to the worker
    /// that already knows what to do with it.
    pending_edit: Option<EditCircularCutRequest>,
}

impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some()
    }
    pub(crate) fn dismiss(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn take_request(&mut self) -> Option<CircularCutRequest> {
        self.pending.take()
    }
    pub(crate) fn take_edit_request(&mut self) -> Option<EditCircularCutRequest> {
        self.pending_edit.take()
    }

    fn begin(&mut self, path: &Path, source: &ExtrudeEditSource, body: ObjectId) -> bool {
        let Some(choice) = source
            .cut_bodies
            .iter()
            .find(|c| c.body == body && c.refusal.is_none())
        else {
            return false;
        };
        self.open(
            path,
            source,
            Subject::Add(choice.clone()),
            Numbers::default(),
        )
    }

    /// Begin changing the numbers of one saved cut.
    ///
    /// The form opens on what the document stores, so the first thing it shows
    /// is the cut as it is rather than an empty box beside the word "Depth".
    fn begin_edit(&mut self, path: &Path, source: &ExtrudeEditSource, feature: ObjectId) -> bool {
        let Some(choice) = source
            .cut_features
            .iter()
            .find(|c| c.feature == feature && c.refusal.is_none())
        else {
            return false;
        };
        let Some(saved) = &choice.saved else {
            return false;
        };
        let typed = Numbers {
            center_x: saved.center_mm[0].to_string(),
            center_y: saved.center_mm[1].to_string(),
            radius: saved.radius_mm.to_string(),
            depth: saved.depth_mm.to_string(),
        };
        self.open(path, source, Subject::Edit(Box::new(choice.clone())), typed)
    }

    fn open(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        subject: Subject,
        typed: Numbers,
    ) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        self.draft = Some(Draft {
            source: path.to_path_buf(),
            version: source.version,
            subject,
            initial: typed.clone(),
            typed,
            applied: None,
            history: History::default(),
            refusal: None,
        });
        true
    }

    /// One button per body the document says can be cut, with the reason on
    /// the ones it says cannot.
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
        let (Some(path), Some(source)) = (path, source) else {
            return;
        };
        for choice in &source.cut_bodies {
            let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
            let response = ui.add_enabled(
                can_begin && refusal.is_none(),
                egui::Button::new(format!(
                    "Cut circle into {} — {}…",
                    choice.name.as_deref().unwrap_or("Unnamed body"),
                    choice.body
                )),
            );
            if response.clicked() {
                self.begin(path, source, choice.body);
            }
            if let Some(reason) = refusal {
                response.on_hover_text(reason);
            }
        }
        // And one per cut already in a history, with the reason on the features
        // whose numbers this slice cannot change — which includes the
        // extrusion that started the body, whose distance `edit-extrude` owns.
        for choice in &source.cut_features {
            let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
            let response = ui.add_enabled(
                can_begin && refusal.is_none(),
                egui::Button::new(format!(
                    "Edit cut {} — {}…",
                    choice.name.as_deref().unwrap_or("Unnamed cut"),
                    choice.feature
                )),
            );
            if response.clicked() {
                self.begin_edit(path, source, choice.feature);
            }
            if let Some(reason) = refusal {
                response.on_hover_text(reason);
            }
        }
    }

    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, running: bool) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        let Some(shown) = draft.subject.shown() else {
            return;
        };
        let mut cancel = false;
        let title = match &draft.subject {
            Subject::Add(_) => "Circular cut — new copy",
            Subject::Edit(_) => "Edit circular cut — new copy",
        };
        egui::Window::new(title)
            .default_width(560.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(
                    "The tool is drawn on the part's own base XY plane and cuts along that \
                     plane's normal (+Z).",
                );
                ui.small(format!(
                    "Body {} · plane {} · {}",
                    shown.body,
                    shown.plane,
                    draft.source.display()
                ));
                ui.small(format!(
                    "Part {} × {} mm from ({}, {}), {} mm tall; the cut modifies feature {}",
                    shown.extents_mm[1][0] - shown.extents_mm[0][0],
                    shown.extents_mm[1][1] - shown.extents_mm[0][1],
                    shown.extents_mm[0][0],
                    shown.extents_mm[0][1],
                    shown.height_mm,
                    shown.modifies,
                ));
                match &shown.editing {
                    None => {
                        ui.label(
                            "A depth equal to the part's height cuts through it; less leaves a \
                             pocket opening on the base side.",
                        );
                    }
                    Some(editing) => {
                        ui.small(format!(
                            "Editing cut {} · tool sketch {} · circle {}",
                            editing.feature, editing.tool_sketch, editing.tool_curve
                        ));
                        ui.small(format!(
                            "Saved: circle ({}, {}) r{} cut {} mm deep — {}",
                            editing.center_mm[0],
                            editing.center_mm[1],
                            editing.radius_mm,
                            editing.depth_mm,
                            if editing.leaves_a_floor {
                                "a pocket with a floor"
                            } else {
                                "a hole through the part"
                            },
                        ));
                        ui.label(if editing.through_allowed {
                            "The depth may run to the part's height, cutting through it."
                        } else {
                            "This pocket's saved floor face is named by the document, so the \
                             depth must stay below the part's height; cutting through would \
                             destroy that name and is refused."
                        });
                    }
                }

                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Cancel cut draft").clicked() {
                            cancel = true;
                        }
                        if ui
                            .add_enabled(!draft.history.undo.is_empty(), egui::Button::new("Undo"))
                            .clicked()
                        {
                            let mut applied = draft.applied.clone().unwrap_or_default();
                            draft.history.undo(&mut applied);
                            draft.typed = applied.clone();
                            draft.applied = Some(applied);
                            draft.refusal = None;
                        }
                        if ui
                            .add_enabled(!draft.history.redo.is_empty(), egui::Button::new("Redo"))
                            .clicked()
                        {
                            let mut applied = draft.applied.clone().unwrap_or_default();
                            draft.history.redo(&mut applied);
                            draft.typed = applied.clone();
                            draft.applied = Some(applied);
                            draft.refusal = None;
                        }
                    });
                    for (label, field) in [
                        ("Centre X (mm):", &mut draft.typed.center_x),
                        ("Centre Y (mm):", &mut draft.typed.center_y),
                        ("Radius (mm):", &mut draft.typed.radius),
                        ("Depth (mm):", &mut draft.typed.depth),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            ui.add(
                                egui::TextEdit::singleline(field)
                                    .id_salt(label)
                                    .char_limit(32)
                                    .desired_width(110.),
                            );
                        });
                    }
                    if ui.button("Apply cut").clicked() {
                        match draft.subject.validate(&draft.typed) {
                            Ok(()) => {
                                let mut applied = draft
                                    .applied
                                    .clone()
                                    .unwrap_or_else(|| draft.initial.clone());
                                draft.history.change(&mut applied, draft.typed.clone());
                                draft.applied = Some(applied);
                                draft.refusal = None;
                            }
                            Err(error) => draft.refusal = Some(error.to_string()),
                        }
                    }
                    if let Some(refusal) = &draft.refusal {
                        ui.colored_label(ui.visuals().error_fg_color, refusal);
                    }
                    let confirmed = draft
                        .applied
                        .as_ref()
                        .filter(|applied| **applied == draft.typed)
                        .and_then(|applied| numbers(applied).ok());
                    match confirmed {
                        Some(cut) => {
                            ui.small(format!(
                                "Ready: circle ({}, {}) r{} cut {} mm deep",
                                cut.center_mm[0], cut.center_mm[1], cut.radius_mm, cut.depth_mm
                            ));
                            if ui.button("Save cut copy…").clicked() {
                                match &shown.editing {
                                    None => {
                                        self.pending = Some(CircularCutRequest {
                                            source: draft.source.clone(),
                                            expected: draft.version,
                                            body: shown.body,
                                            cut,
                                            destination: PathBuf::new(),
                                        })
                                    }
                                    // The saved identity, never the text of a
                                    // field: the form sends back the curve the
                                    // document told it about.
                                    Some(editing) => {
                                        self.pending_edit = Some(EditCircularCutRequest {
                                            source: draft.source.clone(),
                                            expected: draft.version,
                                            cut: editing.feature,
                                            edit: CircularCutEdit {
                                                tool_curve: editing.tool_curve,
                                                center_mm: cut.center_mm,
                                                radius_mm: cut.radius_mm,
                                                depth_mm: cut.depth_mm,
                                            },
                                            destination: PathBuf::new(),
                                        })
                                    }
                                }
                            }
                        }
                        None => {
                            ui.small("Apply the four numbers before saving.");
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

/// What the open form says about the part, whichever operation it is for.
///
/// Read out of the catalogue the accepted reading produced, never assumed and
/// never recomputed here.
#[derive(Debug, Clone)]
struct Shown {
    body: ObjectId,
    plane: ObjectId,
    extents_mm: [[f64; 2]; 2],
    height_mm: f64,
    /// The feature whose result the operation changes.
    modifies: ObjectId,
    editing: Option<Editing>,
}

/// The saved cut a form is changing, and what it is today.
#[derive(Debug, Clone)]
struct Editing {
    feature: ObjectId,
    tool_sketch: ObjectId,
    tool_curve: StableEntityId,
    center_mm: [f64; 2],
    radius_mm: f64,
    depth_mm: f64,
    leaves_a_floor: bool,
    through_allowed: bool,
}

impl Subject {
    fn shown(&self) -> Option<Shown> {
        match self {
            Self::Add(choice) => {
                let target = choice.target.as_ref()?;
                Some(Shown {
                    body: target.body,
                    plane: target.plane,
                    extents_mm: target.extents_mm,
                    height_mm: target.height_mm,
                    modifies: target.tip_feature,
                    editing: None,
                })
            }
            Self::Edit(choice) => {
                let saved = choice.saved.as_ref()?;
                Some(Shown {
                    body: saved.body,
                    plane: saved.plane,
                    extents_mm: saved.extents_mm,
                    height_mm: saved.height_mm,
                    modifies: saved.base_feature,
                    editing: Some(Editing {
                        feature: saved.feature,
                        tool_sketch: saved.tool_sketch,
                        tool_curve: saved.tool_curve,
                        center_mm: saved.center_mm,
                        radius_mm: saved.radius_mm,
                        depth_mm: saved.depth_mm,
                        leaves_a_floor: saved.depth_mm < saved.height_mm,
                        through_allowed: saved.through_allowed(),
                    }),
                })
            }
        }
    }
}

/// The four boxes as one request, or the first reason they are not one.
///
/// Parsed here and judged nowhere else: what a number that parses may be is the
/// document's rule, asked through `validate_cut`.
fn numbers(typed: &Numbers) -> Result<CircularCut> {
    let read = |text: &str, what: &str| -> Result<f64> {
        text.trim()
            .parse::<f64>()
            .map_err(|_| CadError::input(format!("enter a finite {what} in mm")))
    };
    Ok(CircularCut {
        center_mm: [
            read(&typed.center_x, "centre X")?,
            read(&typed.center_y, "centre Y")?,
        ],
        radius_mm: read(&typed.radius, "tool radius")?,
        depth_mm: read(&typed.depth, "cut depth")?,
    })
}

/// The same two steps every published copy takes: the draft is handed to the
/// shared retention so an Open that is later refused can give it back.
pub(crate) fn finish_cut(
    editor: &mut crate::sketch::Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::AddedCircularCut>,
) -> Option<PathBuf> {
    if edits.accepts(generation)
        && let Ok(saved) = &result
    {
        editor.draft_published(&saved.destination);
    }
    edits.finish_cut(generation, result)
}

/// The edit's completion, through exactly the same two steps and the same
/// retention. Its own function only because the two jobs answer with different
/// facts; nothing about what happens to the draft differs.
pub(crate) fn finish_cut_edit(
    editor: &mut crate::sketch::Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::EditedCircularCut>,
) -> Option<PathBuf> {
    if edits.accepts(generation)
        && let Ok(saved) = &result
    {
        editor.draft_published(&saved.destination);
    }
    edits.finish_cut_edit(generation, result)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use ferritecad_document::Document;
    use ferritecad_kernel::OperationContext;

    /// One frame, with its texture delta cleared.
    ///
    /// Always through here: `epaint` refuses a second delta that is never
    /// consumed, so a helper that ran the context directly would fail inside
    /// the library rather than in the test it belongs to.
    fn run(
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
    fn frame(ctx: &egui::Context, e: &mut Editor, running: bool) -> egui::FullOutput {
        run(ctx, e, Vec::new(), running)
    }
    fn click_at(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
        run(ctx, e, vec![egui::Event::PointerMoved(at)], false);
        for pressed in [true, false] {
            run(
                ctx,
                e,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
                false,
            );
        }
    }
    fn click(ctx: &egui::Context, e: &mut Editor, label: &str) {
        let out = frame(ctx, e, false);
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
        click_at(ctx, e, at);
    }
    fn painted(out: &egui::FullOutput, label: &str) -> bool {
        out.shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains(label)))
    }
    fn enter_field(ctx: &egui::Context, e: &mut Editor, label: &str, value: &str) {
        let out = frame(ctx, e, false);
        let at = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    let r = t.visual_bounding_rect();
                    Some(egui::pos2(r.right() + 40., r.center().y))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("input label: {label}"));
        click_at(ctx, e, at);
        run(
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
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                },
                egui::Event::Text(value.into()),
            ],
            false,
        );
    }

    /// A real plate, made by the shipped creation route.
    fn plate() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("plate.fcad");
        ferritecad_jobs::create_document_with_kernel(
            ferritecad_jobs::CreateDocumentRequest::new(
                &path,
                ferritecad_jobs::NewDocument::SketchExtrude(
                    ferritecad_document::PolygonExtrusion::new(
                        vec![[0., 0.], [60., 0.], [60., 40.], [0., 40.]],
                        10.,
                    )
                    .expect("a plate"),
                ),
                "test",
            ),
            ferritecad_occt::OcctKernel::new,
            &OperationContext::default(),
        )
        .expect("source");
        let d = Document::open_read_only(&path).expect("doc");
        let source = ExtrudeEditSource::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }

    fn fill(ctx: &egui::Context, e: &mut Editor, x: &str, y: &str, r: &str, depth: &str) {
        enter_field(ctx, e, "Centre X (mm):", x);
        enter_field(ctx, e, "Centre Y (mm):", y);
        enter_field(ctx, e, "Radius (mm):", r);
        enter_field(ctx, e, "Depth (mm):", depth);
    }

    #[test]
    fn the_form_names_the_plane_and_refuses_a_tool_the_document_would() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for the accepted scene this form reads");
            return;
        }
        let (_root, path, source) = plate();
        let body = source.cut_bodies[0].body;
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, body), "the plate can be cut");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }

        // The plane and the direction are named rather than implied, and the
        // part's own extents are offered so a centre need not be guessed.
        let out = frame(&ctx, &mut e, false);
        assert!(
            painted(&out, "base XY plane"),
            "the form must name the plane"
        );
        assert!(painted(&out, "+Z"), "and the direction it cuts");
        assert!(painted(&out, "60 × 40 mm"), "and the part it is cutting");
        assert!(
            painted(&out, "cuts through it"),
            "and what a full depth does"
        );

        // Nothing is ready before Apply, however complete the boxes look.
        fill(&ctx, &mut e, "20", "15", "5", "4");
        assert!(
            !painted(&frame(&ctx, &mut e, false), "Save cut copy…"),
            "typing is not applying"
        );
        assert!(e.take_request().is_none());

        // A tool the document refuses is refused in the form, by the document's
        // own rule rather than a copy of it here, and changes no history.
        for (x, y, r, depth, why) in [
            ("20", "15", "0", "4", "no radius"),
            ("20", "15", "5", "0", "no depth"),
            ("20", "15", "5", "11", "deeper than the part"),
            ("2", "15", "5", "4", "hanging off the edge"),
            ("banana", "15", "5", "4", "not a number"),
        ] {
            fill(&ctx, &mut e, x, y, r, depth);
            click(&ctx, &mut e, "Apply cut");
            let draft = e.draft.as_ref().expect("draft");
            assert!(draft.applied.is_none(), "{why} was accepted");
            assert!(draft.refusal.is_some(), "{why} said nothing");
            assert!(draft.history.undo.is_empty());
        }

        // One Apply is one history step over all four numbers.
        fill(&ctx, &mut e, "20", "15", "5", "4");
        click(&ctx, &mut e, "Apply cut");
        let draft = e.draft.as_ref().expect("draft");
        assert_eq!(draft.refusal, None);
        assert_eq!(draft.history.undo.len(), 1, "one Apply, one step");
        assert!(painted(&frame(&ctx, &mut e, false), "Save cut copy…"));

        // Changing a number un-readies the request until it is applied again.
        enter_field(&ctx, &mut e, "Depth (mm):", "6");
        assert!(
            !painted(&frame(&ctx, &mut e, false), "Save cut copy…"),
            "an unapplied change must not be saveable"
        );
        click(&ctx, &mut e, "Apply cut");
        assert_eq!(e.draft.as_ref().expect("draft").history.undo.len(), 2);

        // Undo and Redo move the request and ask no job for anything.
        click(&ctx, &mut e, "Undo");
        assert_eq!(e.draft.as_ref().expect("draft").typed.depth, "4");
        assert!(e.take_request().is_none(), "Undo submitted a job");
        click(&ctx, &mut e, "Redo");
        assert_eq!(e.draft.as_ref().expect("draft").typed.depth, "6");
        assert!(e.take_request().is_none(), "Redo submitted a job");

        // Saving hands over exactly the request the widgets built.
        click(&ctx, &mut e, "Save cut copy…");
        let request = e.take_request().expect("real widget request");
        assert_eq!(request.body, body);
        assert_eq!(request.source, path);
        assert_eq!(request.expected, source.version);
        assert_eq!(request.cut.center_mm, [20., 15.]);
        assert_eq!(request.cut.radius_mm, 5.);
        assert_eq!(request.cut.depth_mm, 6.);
        assert!(e.take_request().is_none(), "one press, one request");

        // Cancelling leaves nothing behind and starts nothing.
        click(&ctx, &mut e, "Cancel cut draft");
        assert!(!e.active());
        assert!(e.take_request().is_none());
    }

    /// The same request, run once through the app's own worker and once through
    /// the shipped CLI, publishes the same part.
    #[test]
    fn native_cut_worker_and_cli_publish_the_same_part() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: the cut worker needs OCCT");
            return;
        }
        let (root, path, source) = plate();
        let before = std::fs::read(&path).expect("source");
        let body = source.cut_bodies[0].body;

        let mut e = Editor::default();
        let ctx = egui::Context::default();
        assert!(e.begin(&path, &source, body));
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        fill(&ctx, &mut e, "20", "15", "5", "4");
        click(&ctx, &mut e, "Apply cut");
        click(&ctx, &mut e, "Save cut copy…");
        let mut request = e.take_request().expect("widget request");

        let ui = root.path().join("worker.fcad");
        request.destination = ui.clone();
        let cut = request.cut;
        let mut state = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start_cut(request, move |r, g, c| {
                crate::edits::spawn_cut(r, c, move |result| tx.send((g, result)).expect("reply"))
            })
            .expect("worker");
        let (generation, result) = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("worker response");
        let published = result.as_ref().expect("published").clone();
        assert_eq!(published.body, body, "the body kept its identity");
        let typed = e.draft.as_ref().expect("draft").typed.clone();
        let applied = e.draft.as_ref().expect("draft").applied.clone();
        let undo = e.draft.as_ref().expect("draft").history.undo.clone();
        let mut editor = crate::sketch::Editor::default();
        editor.cuts = e;
        assert_eq!(
            finish_cut(&mut editor, &mut state, generation, result),
            Some(ui.clone())
        );
        assert!(!editor.active());
        editor.draft_load_finished(&root.path().join("unrelated.fcad"), false);
        assert!(!editor.active());
        editor.draft_load_finished(&ui, false);
        assert!(
            editor.cuts.active(),
            "a refused Open must restore the Cut draft"
        );
        let restored = editor.cuts.draft.as_ref().expect("restored draft");
        assert_eq!(restored.typed, typed);
        assert_eq!(restored.applied, applied);
        assert_eq!(restored.history.undo, undo);
        editor.draft_published(&ui);
        editor.draft_load_finished(&ui, true);
        assert!(!editor.active());
        editor.draft_load_finished(&ui, false);
        assert!(!editor.active(), "accepted drafts cannot be resurrected");

        // The same numbers, through the shipped command line.
        let input = root.path().join("request.json");
        std::fs::write(
            &input,
            format!(
                r#"{{"request_version":1,"center_mm":[{},{}],"radius_mm":{},"depth_mm":{}}}"#,
                cut.center_mm[0], cut.center_mm[1], cut.radius_mm, cut.depth_mm
            ),
        )
        .expect("input");
        let peer = root.path().join("peer.fcad");
        let out = std::process::Command::new(crate::creates::tests::ferritecad())
            .arg("cut-circular-copy")
            .arg(&path)
            .arg("--body")
            .arg(body.to_string())
            .arg("--expect-version")
            .arg(source.version.content.to_string())
            .arg("--request")
            .arg(input)
            .arg("-o")
            .arg(&peer)
            .arg("--json")
            .output()
            .expect("peer");
        assert!(out.status.success(), "{out:?}");

        // One document, two ways. Only the identifiers this operation minted
        // may differ, and they are matched off one by one rather than ignored.
        let a = Document::open_read_only(&ui).expect("worker copy");
        let b = Document::open_read_only(&peer).expect("CLI copy");
        assert_eq!(a.meta().document_id, b.meta().document_id);
        let (mine, theirs) = (a.objects().expect("objects"), b.objects().expect("objects"));
        assert_eq!(mine.len(), theirs.len());
        assert_eq!(mine.len(), 6);
        let minted = |objects: &[ferritecad_document::ObjectRecord]| {
            objects
                .iter()
                .filter(|o| {
                    matches!(&o.payload, ferritecad_document::ObjectPayload::Extrude(e)
                        if e.previous.is_some())
                        || o.name.as_deref() == Some("Cut profile")
                })
                .map(|o| o.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(minted(&mine).len(), 2, "a cut mints two objects");
        assert_eq!(minted(&theirs).len(), 2);
        for left in &mine {
            if minted(&mine).contains(&left.id) {
                continue;
            }
            let right = b.object(left.id).expect("read").expect("same object");
            match (&left.payload, &right.payload) {
                // The body's tip is the feature each run minted, so the payload
                // differs by exactly that one identifier and nothing else.
                (
                    ferritecad_document::ObjectPayload::Body(one),
                    ferritecad_document::ObjectPayload::Body(other),
                ) => {
                    assert!(minted(&mine).contains(&one.tip_feature.expect("a tip")));
                    assert!(minted(&theirs).contains(&other.tip_feature.expect("a tip")));
                    assert_eq!(left.name, right.name);
                    assert_eq!(left.ordinal, right.ordinal);
                }
                _ => assert_eq!(
                    left, &right,
                    "an object this operation does not touch moved"
                ),
            }
        }
        a.close().expect("close");
        b.close().expect("close");

        // And the geometry both published is the same geometry, byte for byte.
        for format in ["stl", "fbx"] {
            let mut exports = Vec::new();
            for model in [&ui, &peer] {
                let output = model.with_extension(format);
                let result = std::process::Command::new(crate::creates::tests::ferritecad())
                    .arg(format!("export-{format}"))
                    .arg(model)
                    .arg("-o")
                    .arg(&output)
                    .arg("--json")
                    .output()
                    .expect("export");
                assert!(result.status.success(), "{result:?}");
                exports.push(std::fs::read(output).expect("export bytes"));
            }
            assert_eq!(exports[0], exports[1], "worker/CLI {format} bytes");
        }
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }

    /// A real plate with one real pocket cut into it, made by the shipped
    /// routes, and the catalogue a form would read from it.
    fn pocket(depth: f64) -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let (root, plate, source) = plate();
        let cut = root.path().join("cut.fcad");
        ferritecad_jobs::circular_cut_copy(
            &CircularCutRequest {
                source: plate,
                expected: source.version,
                body: source.cut_bodies[0].body,
                cut: CircularCut {
                    center_mm: [20., 15.],
                    radius_mm: 5.,
                    depth_mm: depth,
                },
                destination: cut.clone(),
            },
            &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
            &OperationContext::default(),
        )
        .expect("a part with a cut in it");
        let d = Document::open_read_only(&cut).expect("doc");
        let reading = ExtrudeEditSource::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, cut, reading)
    }

    #[test]
    fn the_edit_form_shows_the_saved_cut_and_refuses_what_the_document_would() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for the saved cut this form reads");
            return;
        }
        let (_root, path, source) = pocket(4.);
        let saved = source
            .cut_features
            .iter()
            .find(|c| c.refusal.is_none())
            .expect("one editable cut")
            .saved
            .clone()
            .expect("the saved cut");
        // The extrusion that started the body is not offered here; its own
        // distance edit still owns it.
        assert_eq!(
            source
                .cut_features
                .iter()
                .filter(|c| c.refusal.is_none())
                .count(),
            1,
            "only the Cut is offered"
        );
        let mut e = Editor::default();
        assert!(e.begin_edit(&path, &source, saved.feature));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }

        // It opens on what the document stores, not on an empty box.
        let draft = e.draft.as_ref().expect("draft");
        assert_eq!(draft.typed.center_x, "20");
        assert_eq!(draft.typed.center_y, "15");
        assert_eq!(draft.typed.radius, "5");
        assert_eq!(draft.typed.depth, "4");
        let out = frame(&ctx, &mut e, false);
        assert!(painted(&out, "base XY plane"), "the plane is named");
        assert!(painted(&out, "+Z"), "and the direction");
        assert!(painted(&out, "60 × 40 mm"), "and the part");
        assert!(
            painted(&out, &saved.feature.to_string()),
            "and the cut being edited"
        );
        assert!(
            painted(&out, &saved.tool_curve.to_string()),
            "and the circle whose numbers these are"
        );
        assert!(
            painted(&out, "a pocket with a floor"),
            "and what the cut is today"
        );
        assert!(
            painted(&out, "must stay below the part's height"),
            "and that this pocket may not be cut through"
        );
        assert!(e.take_edit_request().is_none());

        // What the document refuses, the form refuses, by the document's own
        // rule and without touching the history.
        for (x, y, r, depth, why) in [
            ("20", "15", "5", "10", "cutting the saved floor away"),
            ("20", "15", "0", "4", "no radius"),
            ("20", "15", "5", "0", "no depth"),
            ("2", "15", "5", "4", "hanging off the edge"),
            ("banana", "15", "5", "4", "not a number"),
        ] {
            fill(&ctx, &mut e, x, y, r, depth);
            click(&ctx, &mut e, "Apply cut");
            let draft = e.draft.as_ref().expect("draft");
            assert!(draft.applied.is_none(), "{why} was accepted");
            assert!(draft.refusal.is_some(), "{why} said nothing");
            assert!(draft.history.undo.is_empty());
        }

        // One Apply is one step over all four, and a later change closes Save.
        fill(&ctx, &mut e, "30", "20", "7.5", "6");
        click(&ctx, &mut e, "Apply cut");
        assert_eq!(e.draft.as_ref().expect("draft").history.undo.len(), 1);
        let confirmed = e.draft.as_ref().expect("draft").typed.clone();
        click(&ctx, &mut e, "Undo");
        assert_eq!(
            e.draft.as_ref().expect("draft").typed,
            Numbers {
                center_x: "20".into(),
                center_y: "15".into(),
                radius: "5".into(),
                depth: "4".into(),
            },
            "the first Undo restores the saved cut, not an empty creation form"
        );
        assert!(e.take_edit_request().is_none());
        click(&ctx, &mut e, "Redo");
        assert_eq!(e.draft.as_ref().expect("draft").typed, confirmed);
        assert!(painted(&frame(&ctx, &mut e, false), "Save cut copy…"));
        enter_field(&ctx, &mut e, "Radius (mm):", "8");
        assert!(
            !painted(&frame(&ctx, &mut e, false), "Save cut copy…"),
            "an unapplied change must not be saveable"
        );
        click(&ctx, &mut e, "Apply cut");
        assert_eq!(e.draft.as_ref().expect("draft").history.undo.len(), 2);
        click(&ctx, &mut e, "Undo");
        assert_eq!(e.draft.as_ref().expect("draft").typed.radius, "7.5");
        assert!(e.take_edit_request().is_none(), "Undo submitted a job");
        click(&ctx, &mut e, "Redo");
        assert_eq!(e.draft.as_ref().expect("draft").typed.radius, "8");
        assert!(e.take_edit_request().is_none(), "Redo submitted a job");

        // Saving hands over the identities the document gave, never text.
        click(&ctx, &mut e, "Save cut copy…");
        let request = e.take_edit_request().expect("real widget request");
        assert_eq!(request.source, path);
        assert_eq!(request.expected, source.version);
        assert_eq!(request.cut, saved.feature);
        assert_eq!(request.edit.tool_curve, saved.tool_curve);
        assert_eq!(request.edit.center_mm, [30., 20.]);
        assert_eq!(request.edit.radius_mm, 8.);
        assert_eq!(request.edit.depth_mm, 6.);
        assert!(e.take_edit_request().is_none(), "one press, one request");
        click(&ctx, &mut e, "Cancel cut draft");
        assert!(!e.active());
    }

    /// A cut that already runs through the part may be shortened: that adds a
    /// name and loses none, which is the whole of the policy in one direction.
    #[test]
    fn a_hole_may_be_shortened_into_a_pocket_and_the_form_says_so() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for the saved cut this form reads");
            return;
        }
        let (_root, path, source) = pocket(10.);
        let saved = source
            .cut_features
            .iter()
            .find(|c| c.refusal.is_none())
            .expect("one editable cut")
            .saved
            .clone()
            .expect("the saved cut");
        assert!(saved.through_allowed(), "a hole has no floor to lose");
        let mut e = Editor::default();
        assert!(e.begin_edit(&path, &source, saved.feature));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        let out = frame(&ctx, &mut e, false);
        assert!(painted(&out, "a hole through the part"));
        assert!(painted(&out, "may run to the part's height"));
        fill(&ctx, &mut e, "20", "15", "5", "3");
        click(&ctx, &mut e, "Apply cut");
        assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
        // And the depth it already has is still accepted.
        fill(&ctx, &mut e, "20", "15", "5", "10");
        click(&ctx, &mut e, "Apply cut");
        assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    }

    /// The same request, run once through the app's own worker and once through
    /// the shipped CLI, publishes the same edited part.
    #[test]
    fn native_cut_edit_worker_and_cli_publish_the_same_part() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: the cut edit worker needs OCCT");
            return;
        }
        let (root, path, source) = pocket(4.);
        let before = std::fs::read(&path).expect("source");
        let saved = source
            .cut_features
            .iter()
            .find(|c| c.refusal.is_none())
            .expect("one editable cut")
            .saved
            .clone()
            .expect("the saved cut");

        let mut e = Editor::default();
        let ctx = egui::Context::default();
        assert!(e.begin_edit(&path, &source, saved.feature));
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        fill(&ctx, &mut e, "30", "20", "7.5", "6");
        click(&ctx, &mut e, "Apply cut");
        click(&ctx, &mut e, "Save cut copy…");
        let mut request = e.take_edit_request().expect("widget request");

        let ui = root.path().join("worker.fcad");
        request.destination = ui.clone();
        let edit = request.edit;
        let mut state = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start_cut_edit(request, move |r, g, c| {
                crate::edits::spawn_cut_edit(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (generation, result) = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("worker response");
        let published = result.as_ref().expect("published").clone();
        assert_eq!(
            published.feature, saved.feature,
            "the cut kept its identity"
        );
        assert_eq!(published.tool_curve, saved.tool_curve);
        assert_eq!(published.body, saved.body);
        assert_eq!(published.previous, saved.base_feature);
        assert!(published.leaves_a_floor);

        // The draft survives a publication whose Open is then refused, through
        // exactly the retention every other copy edit uses.
        let typed = e.draft.as_ref().expect("draft").typed.clone();
        let applied = e.draft.as_ref().expect("draft").applied.clone();
        let undo = e.draft.as_ref().expect("draft").history.undo.clone();
        let mut editor = crate::sketch::Editor::default();
        editor.cuts = e;
        assert_eq!(
            finish_cut_edit(&mut editor, &mut state, generation, result),
            Some(ui.clone())
        );
        assert!(!editor.active());
        editor.draft_load_finished(&root.path().join("unrelated.fcad"), false);
        assert!(!editor.active());
        editor.draft_load_finished(&ui, false);
        assert!(
            editor.cuts.active(),
            "a refused Open must restore the cut edit draft"
        );
        let restored = editor.cuts.draft.as_ref().expect("restored draft");
        assert_eq!(restored.typed, typed);
        assert_eq!(restored.applied, applied);
        assert_eq!(restored.history.undo, undo);
        editor.draft_published(&ui);
        editor.draft_load_finished(&ui, true);
        assert!(!editor.active(), "an accepted draft is finished");

        // The same numbers, through the shipped command line.
        let input = root.path().join("request.json");
        std::fs::write(
            &input,
            format!(
                r#"{{"request_version":1,"tool_curve_id":"{}","center_mm":[{},{}],"radius_mm":{},"depth_mm":{}}}"#,
                edit.tool_curve,
                edit.center_mm[0],
                edit.center_mm[1],
                edit.radius_mm,
                edit.depth_mm
            ),
        )
        .expect("input");
        let peer = root.path().join("peer.fcad");
        let out = std::process::Command::new(crate::creates::tests::ferritecad())
            .arg("edit-circular-cut")
            .arg(&path)
            .arg("--feature")
            .arg(saved.feature.to_string())
            .arg("--expect-version")
            .arg(source.version.content.to_string())
            .arg("--request")
            .arg(input)
            .arg("-o")
            .arg(&peer)
            .arg("--json")
            .output()
            .expect("peer");
        assert!(out.status.success(), "{out:?}");

        // One document, two ways. This operation mints no object at all, so
        // every object must match whole — not "all but the new ones".
        let a = Document::open_read_only(&ui).expect("worker copy");
        let b = Document::open_read_only(&peer).expect("CLI copy");
        assert_eq!(a.meta().document_id, b.meta().document_id);
        let (mine, theirs) = (a.objects().expect("objects"), b.objects().expect("objects"));
        assert_eq!(mine.len(), 6);
        assert_eq!(mine, theirs, "an edited object differs between the two");
        // And the names: an edit that kept a floor adds none, so both copies
        // hold exactly the same references under exactly the same identities.
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
        a.close().expect("close");
        b.close().expect("close");

        for format in ["stl", "fbx"] {
            let mut exports = Vec::new();
            for model in [&ui, &peer] {
                let output = model.with_extension(format);
                let result = std::process::Command::new(crate::creates::tests::ferritecad())
                    .arg(format!("export-{format}"))
                    .arg(model)
                    .arg("-o")
                    .arg(&output)
                    .arg("--json")
                    .output()
                    .expect("export");
                assert!(result.status.success(), "{result:?}");
                exports.push(std::fs::read(output).expect("export bytes"));
            }
            assert_eq!(exports[0], exports[1], "worker/CLI {format} bytes");
        }
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }
}
