// SPDX-License-Identifier: MIT
//! A disposable circular-cut request over stored facts; no kernel, no file reads.
//!
//! The form offers a cut only where the document says one can be made, and it
//! says what it is doing rather than implying it: the tool is drawn on the
//! part's own base XY datum and runs along that datum's normal. There is no
//! face to pick here and no attachment to choose, because this slice implements
//! neither, and a control that suggested otherwise would be promising it.
use ferritecad_document::{CircularCut, CutChoice, DocumentVersion, ExtrudeEditSource};
use ferritecad_jobs::CircularCutRequest;
use ferritecad_types::{CadError, ObjectId, Result};
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

#[derive(Debug, Clone)]
struct Draft {
    source: PathBuf,
    version: DocumentVersion,
    choice: CutChoice,
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

    fn begin(&mut self, path: &Path, source: &ExtrudeEditSource, body: ObjectId) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .cut_bodies
            .iter()
            .find(|c| c.body == body && c.refusal.is_none())
        else {
            return false;
        };
        self.draft = Some(Draft {
            source: path.to_path_buf(),
            version: source.version,
            choice: choice.clone(),
            typed: Numbers::default(),
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
    }

    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, running: bool) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        let Some(target) = draft.choice.target.clone() else {
            return;
        };
        let mut cancel = false;
        egui::Window::new("Circular cut — new copy")
            .default_width(560.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(
                    "The tool is drawn on the part's own base XY plane and cuts along that \
                     plane's normal (+Z).",
                );
                ui.small(format!(
                    "Body {} · plane {} · {}",
                    target.body,
                    target.plane,
                    draft.source.display()
                ));
                ui.small(format!(
                    "Part {} × {} mm from ({}, {}), {} mm tall; the cut modifies feature {}",
                    target.extents_mm[1][0] - target.extents_mm[0][0],
                    target.extents_mm[1][1] - target.extents_mm[0][1],
                    target.extents_mm[0][0],
                    target.extents_mm[0][1],
                    target.height_mm,
                    target.tip_feature,
                ));
                ui.label(
                    "A depth equal to the part's height cuts through it; less leaves a pocket \
                     opening on the base side.",
                );

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
                        match numbers(&draft.typed).and_then(|cut| {
                            draft.choice.validate_cut(&cut)?;
                            Ok(cut)
                        }) {
                            Ok(_) => {
                                let mut applied = draft.applied.clone().unwrap_or_default();
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
                                self.pending = Some(CircularCutRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,
                                    body: target.body,
                                    cut,
                                    destination: PathBuf::new(),
                                });
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
}
