// SPDX-License-Identifier: MIT
//! §30C: the constraints form's Apply, through the window's own availability
//! predicate and the form's real widgets, never a hand-set `can_apply`.

use super::*;
use crate::{Loads, can_apply_constraints, creates, edits, exports, sessions};

struct Window {
    _private: tempfile::TempDir,
    sessions: sessions::Sessions,
    creates: creates::Creates,
    loads: Loads,
    exports: exports::Exports,
    edits: edits::Edits,
}

impl Window {
    fn open(path: &Path) -> Self {
        let private = tempfile::tempdir().expect("private root");
        let mut sessions = sessions::Sessions::default();
        sessions.adopt(
            ferritecad_jobs::DocumentSession::open_in(
                private.path(),
                path,
                ferritecad_jobs::HistoryLimits::default(),
            )
            .expect("session"),
        );
        Self {
            _private: private,
            sessions,
            creates: creates::Creates::default(),
            loads: Loads::default(),
            exports: exports::Exports::default(),
            edits: edits::Edits::default(),
        }
    }
    fn ready(&self) -> bool {
        can_apply_constraints(
            &self.creates,
            &self.loads,
            &self.exports,
            &self.edits,
            &self.sessions,
        )
    }
    /// What the window says to the form every frame.
    fn tell(&mut self, unsaved: bool) {
        let ready = self.ready();
        self.creates
            .sketch
            .constraints
            .set_session(true, ready, unsaved);
    }
    fn form(&mut self) -> &mut Editor {
        &mut self.creates.sketch.constraints
    }
}

fn replace_segment_length(ctx: &egui::Context, e: &mut Editor, segment: &str, length: &str) {
    for _ in 0..3 {
        frame(ctx, e, vec![]);
    }
    click(ctx, e, segment);
    enter_length(ctx, e, length, false);
    click(ctx, e, "Replace length");
}

#[test]
fn the_open_form_applies_through_the_windows_own_predicate_and_keeps_its_draft() {
    let (_root, path, _) = fixture();
    let source = persist_rectangle_lengths(&path);
    let sketch = source.constraint_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(
        !w.ready(),
        "nothing to apply before a form is open for the Sketch"
    );
    assert!(w.form().begin(&path, &source, sketch));
    assert!(w.creates.busy(), "New and Open still wait for this form");
    assert!(
        w.ready(),
        "the form's own Apply must not be blocked by the form"
    );
    let ctx = egui::Context::default();

    // The window says "not ready": the button is there and does nothing.
    w.form().set_session(true, false, true);
    replace_segment_length(&ctx, w.form(), "Segment 1", "55");
    click(&ctx, w.form(), "Apply constraints");
    assert!(w.form().take_apply_request().is_none(), "disabled asked");

    // The window says what it computes: one press, one request.
    w.tell(true);
    click(&ctx, w.form(), "Apply constraints");
    let request = w.form().take_apply_request().expect("Apply asked");
    assert_eq!(request.expected, source.version);
    assert_eq!(request.sketch, sketch);
    assert_eq!(request.edits.remove.len(), 1, "the replaced length");
    assert_eq!(request.edits.add.len(), 1);
    assert!(
        w.form().take_apply_request().is_none(),
        "one press, one request"
    );
    assert!(
        w.ready(),
        "the command must allow the same request as its button"
    );
    assert_eq!(
        history_state(w.form()).0,
        request.edits,
        "pressing Apply discarded the draft"
    );
}

#[test]
fn nothing_else_running_or_open_lets_the_command_start() {
    let (_root, path, _) = fixture();
    let source = persist_rectangle_lengths(&path);
    let sketch = source.constraint_sketches[0].sketch;

    // A load in flight, a session operation, another editor's form: each blocks.
    let mut w = Window::open(&path);
    assert!(w.form().begin(&path, &source, sketch));
    assert!(w.ready());
    w.loads
        .open(
            Some(&path),
            std::sync::Arc::new(crate::ProgressRelay::default()),
            |_, _| std::thread::spawn(|| {}),
        )
        .expect("load");
    assert!(!w.ready(), "an Open in flight must block Apply");
    w.loads.stop_all();
    w.loads = Loads::default();
    assert!(w.ready());
    w.sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("operation");
    assert!(!w.ready(), "a running session operation must block Apply");
    w.sessions.stop_all();

    let mut w = Window::open(&path);
    assert!(w.form().begin(&path, &source, sketch));
    assert!(w.edits.begin(&path, &source));
    assert!(!w.ready(), "the height form being open must block Apply");

    // No form for the Sketch at all (the form was closed), and no session.
    let mut w = Window::open(&path);
    assert!(!w.ready());
    assert!(w.form().begin(&path, &source, sketch));
    w.form().dismiss();
    assert!(!w.ready());
    let mut w = Window::open(&path);
    assert!(w.form().begin(&path, &source, sketch));
    w.sessions = sessions::Sessions::default();
    assert!(!w.ready(), "no open document, nothing to apply into");
}

#[test]
fn an_empty_draft_offers_no_apply_and_keeps_the_redo_of_the_draft() {
    let (_root, path, _) = fixture();
    let source = persist_rectangle_lengths(&path);
    let sketch = source.constraint_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin(&path, &source, sketch));
    w.tell(false);
    let ctx = egui::Context::default();
    replace_segment_length(&ctx, w.form(), "Segment 1", "55");
    click(&ctx, w.form(), "Undo");
    let (edits, undo, redo) = history_state(w.form());
    assert_eq!(edits, SketchConstraintEdits::default());
    assert_eq!(redo.len(), 1);
    let out = frame(&ctx, w.form(), vec![]);
    assert!(
        !painted(&out, "Apply constraints"),
        "Apply was offered for a draft that changes nothing"
    );
    assert!(
        w.form().take_apply_request().is_none(),
        "an empty draft asked"
    );
    assert_eq!(
        history_state(w.form()),
        (edits, undo, redo),
        "the empty press lost the draft's Redo"
    );
}

#[test]
fn with_unsaved_changes_save_copy_is_withheld_with_words_and_apply_is_not() {
    let (_root, path, _) = fixture();
    let source = persist_rectangle_lengths(&path);
    let sketch = source.constraint_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin(&path, &source, sketch));
    w.tell(true);
    let ctx = egui::Context::default();
    replace_segment_length(&ctx, w.form(), "Segment 1", "55");
    let out = frame(&ctx, w.form(), vec![]);
    assert!(
        out.shapes.iter().any(|s| matches!(
            &s.shape,
            egui::Shape::Text(t) if t.galley.text().contains("unsaved changes")
        )),
        "the withheld copy workflow was not explained"
    );
    click(&ctx, w.form(), "Save constraints copy…");
    assert!(w.form().take_request().is_none(), "a withheld copy asked");
    click(&ctx, w.form(), "Apply constraints");
    assert!(w.form().take_apply_request().is_some());

    // The same draft on a clean document keeps the old copy workflow.
    w.tell(false);
    click(&ctx, w.form(), "Save constraints copy…");
    assert!(w.form().take_request().is_some());
}

#[test]
fn a_new_accepted_version_ends_the_draft_and_keeps_what_the_window_said() {
    let (_root, path, _) = fixture();
    let source = persist_rectangle_lengths(&path);
    let sketch = source.constraint_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin(&path, &source, sketch));
    w.tell(true);
    let ctx = egui::Context::default();
    replace_segment_length(&ctx, w.form(), "Segment 1", "55");
    w.creates.sketch.finish_session_change();
    assert!(
        !w.form().active(),
        "the draft outlived the picture it described"
    );
    assert!(!w.creates.sketch.active());
    // The next form for the next version starts with what the window already said.
    assert!(w.form().begin(&path, &source, sketch));
    click(&ctx, w.form(), "Segment 1");
    enter_length(&ctx, w.form(), "56", false);
    click(&ctx, w.form(), "Replace length");
    click(&ctx, w.form(), "Apply constraints");
    assert!(
        w.form().take_apply_request().is_some(),
        "the flags were lost when the draft was replaced"
    );
}

fn choice_frame(
    ctx: &egui::Context,
    e: &mut Editor,
    path: &Path,
    source: &ExtrudeEditSource,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(988., 768.),
            )),
            events,
            ..Default::default()
        },
        |ui| e.choices(ui, false, Some(path), Some(source)),
    );
    out.textures_delta.clear();
    out
}

/// With unsaved changes the window's copy-workflow gate is closed (`can_begin`
/// false). The form is still offered, because it applies into the document.
#[test]
fn the_form_opens_on_a_document_with_unsaved_changes_only_when_the_window_says_so() {
    let (_root, path, _) = fixture();
    let source = persist_rectangle_lengths(&path);
    for (session_idle, opens) in [(false, false), (true, true)] {
        let mut e = Editor::default();
        e.set_session(session_idle, true, true);
        let ctx = egui::Context::default();
        choice_frame(&ctx, &mut e, &path, &source, vec![]);
        let out = choice_frame(&ctx, &mut e, &path, &source, vec![]);
        let at = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text().starts_with("Edit constraints") => {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("the entry is painted");
        choice_frame(
            &ctx,
            &mut e,
            &path,
            &source,
            vec![egui::Event::PointerMoved(at)],
        );
        for pressed in [true, false] {
            choice_frame(
                &ctx,
                &mut e,
                &path,
                &source,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
            );
        }
        assert_eq!(e.active(), opens, "session_idle={session_idle}");
    }
}

/// The request the peer command line takes for the same edits, in its own words.
pub(crate) fn peer_request(edits: &SketchConstraintEdits) -> String {
    let removals = edits
        .remove
        .iter()
        .map(|id| format!(r#""{id}""#))
        .collect::<Vec<_>>()
        .join(",");
    let additions = edits
        .add
        .iter()
        .map(peer_addition)
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"request_version":1,"remove":[{removals}],"add":[{additions}]}}"#)
}

fn pressed(
    ctx: &egui::Context,
    mut e: Editor,
    unsaved: bool,
) -> (Editor, EditSketchConstraintsRequest) {
    e.set_session(true, true, unsaved);
    click(ctx, &mut e, "Apply constraints");
    let request = e.take_apply_request().expect("Apply constraints asked");
    (e, request)
}

fn opened(path: &Path, source: &ExtrudeEditSource, sketch: ObjectId) -> (Editor, egui::Context) {
    let mut e = Editor::default();
    e.set_session(true, true, true);
    assert!(e.begin(path, source, sketch));
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    (e, ctx)
}

/// The stored length of Segment `segment` replaced by `length`, through the widgets.
pub(crate) fn typed_replace_length(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    segment: usize,
    length: &str,
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut e, ctx) = opened(path, source, sketch);
    replace_segment_length(&ctx, &mut e, &format!("Segment {segment}"), length);
    pressed(&ctx, e, true)
}

/// A Radius and a Fixed centre on the profile's one circle, through the widgets.
pub(crate) fn typed_circle(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    radius: &str,
    centre: (&str, &str),
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut e, ctx) = opened(path, source, sketch);
    let curve = source.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .curves[0]
        .id;
    click(&ctx, &mut e, &format!("Circle · {}", short(curve)));
    enter_field(&ctx, &mut e, "Radius mm", radius, false);
    click(&ctx, &mut e, "Add radius");
    enter_field(&ctx, &mut e, "Fixed X (mm):", centre.0, false);
    enter_field(&ctx, &mut e, "Fixed Y (mm):", centre.1, false);
    click(&ctx, &mut e, "Add Fixed centre");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    pressed(&ctx, e, true)
}

/// Two Lines forced parallel, through the widgets: the form cannot know whether
/// the solver will agree.
pub(crate) fn typed_parallel(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    a: usize,
    b: usize,
) -> (Editor, EditSketchConstraintsRequest) {
    let (mut e, ctx) = opened(path, source, sketch);
    click(&ctx, &mut e, &format!("Segment {a}"));
    click(&ctx, &mut e, "Pair Line A");
    click(&ctx, &mut e, &format!("Segment {b}"));
    click(&ctx, &mut e, "Pair Line B");
    click(&ctx, &mut e, "Add Parallel");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    pressed(&ctx, e, true)
}

/// The draft of `typed_replace_length` without pressing Apply: what the form says.
pub(crate) fn replace_length_offers_apply(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    segment: usize,
    length: &str,
) -> (Editor, bool) {
    let (mut e, ctx) = opened(path, source, sketch);
    replace_segment_length(&ctx, &mut e, &format!("Segment {segment}"), length);
    let out = frame(&ctx, &mut e, vec![]);
    let offered = painted(&out, "Apply constraints");
    (e, offered)
}
