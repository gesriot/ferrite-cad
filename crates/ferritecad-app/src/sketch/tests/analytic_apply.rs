// SPDX-License-Identifier: MIT
//! §30D: the saved Circle and annulus forms' Apply, through the window's own
//! availability predicate and the forms' real widgets, never a hand-set flag.
//! Kernel-free: the documents are written and read without Open CASCADE.

use super::*;
use crate::{Loads, can_apply_analytic, creates, edits, exports, sessions};
use ferritecad_document::ExtrudeEditSource;
use ferritecad_types::ObjectId;

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
        can_apply_analytic(
            &self.creates,
            &self.loads,
            &self.exports,
            &self.edits,
            &self.sessions,
        )
    }
    /// What the window says to the forms every frame.
    fn tell(&mut self, unsaved: bool) {
        let ready = self.ready();
        self.creates.sketch.set_analytic_apply(ready);
        self.creates.sketch.set_session(true, false, unsaved);
    }
    fn form(&mut self) -> &mut Editor {
        &mut self.creates.sketch
    }
}

/// One analytic circle (`radii` of one) or two concentric ones (the boundary
/// first, or the bore first when `swapped`), written through the document API: a
/// build with no kernel has no `create-*` command, and these tests need none.
pub(crate) fn write_round(
    path: &Path,
    center: [f64; 2],
    radii: &[f64],
    height: f64,
    swapped: bool,
) {
    use ferritecad_document::{
        Body, CapSide, DatumPlane, Dependency, DependencyRole, Document, EndCondition, EntityKind,
        Expression, Extrude, ObjectPayload, Point2, SelectionRule, SemanticRole, Sketch,
        SketchCurve, SketchGeometry, SolidOperation, TopologyRef,
    };
    use ferritecad_types::{ObjectId, StableEntityId, Transform};
    let mut d = Document::create(path).expect("document");
    let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
    let drawn: Vec<StableEntityId> = radii.iter().map(|_| StableEntityId::new()).collect();
    d.write(|w| {
        w.put_object(
            plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;
        let mut curves = Vec::new();
        for (id, radius) in drawn.iter().zip(radii) {
            curves.push(SketchCurve {
                id: *id,
                construction: false,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(center[0], center[1])?,
                    radius: *radius,
                },
            });
        }
        if swapped {
            curves.reverse();
        }
        w.put_object(
            sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves,
                constraints: Vec::new(),
            }),
        )?;
        w.put_object(
            extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(height)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                target_body: None,
                previous: None,
            }),
        )?;
        w.put_object(
            body,
            None,
            3,
            Some("Body"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(extrude),
            }),
        )?;
        for (dependent, dependency, role) in [
            (sketch, plane, DependencyRole::Plane),
            (extrude, sketch, DependencyRole::Profile),
            (body, extrude, DependencyRole::BodyTip),
        ] {
            w.add_dependency(Dependency {
                dependent,
                dependency,
                role,
            })?;
        }
        for side in [CapSide::Start, CapSide::End] {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: extrude,
                producer_feature: extrude,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::ExtrudeCap { side },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })?;
        }
        for curve in &drawn {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: extrude,
                producer_feature: extrude,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::ExtrudeSide {
                    profile_segment: *curve,
                },
                selection: SelectionRule::AllDerivedFrom { ancestor: *curve },
                fallback_signature: None,
            })?;
        }
        Ok(())
    })
    .expect("round document");
    d.close().expect("close");
}

fn circle_document(root: &Path) -> (PathBuf, ExtrudeEditSource) {
    let path = root.join("circle.fcad");
    write_round(&path, [12.5, -7.25], &[10.5], 15.25, false);
    let source = ferritecad_jobs::read_extrude_source(&path).expect("reading");
    (path, source)
}

fn annulus_document(root: &Path) -> (PathBuf, ExtrudeEditSource) {
    let path = root.join("annulus.fcad");
    write_round(&path, [12.5, -7.25], &[10.5, 4.75], 15.25, false);
    let source = ferritecad_jobs::read_extrude_source(&path).expect("reading");
    (path, source)
}

fn plate_document(root: &Path) -> (PathBuf, ExtrudeEditSource) {
    let path = root.join("plate.fcad");
    ferritecad_jobs::create_document(
        ferritecad_jobs::CreateDocumentRequest::new(
            &path,
            NewDocument::SamplePlate(Default::default()),
            "test",
        ),
        &ferritecad_kernel::OperationContext::default(),
    )
    .expect("plate");
    let source = ferritecad_jobs::read_extrude_source(&path).expect("reading");
    (path, source)
}

fn settle(ctx: &egui::Context, e: &mut Editor) {
    for _ in 0..3 {
        frame(ctx, e, vec![]);
    }
}

fn type_circle(ctx: &egui::Context, e: &mut Editor, x: &str, y: &str, radius: &str) {
    for (label, value) in [("Center X", x), ("Center Y", y), ("Radius", radius)] {
        let out = frame(ctx, e, vec![]);
        type_into_grid_row(ctx, e, &out, label, value);
    }
}

fn type_annulus(ctx: &egui::Context, e: &mut Editor, x: &str, y: &str, outer: &str, inner: &str) {
    for (label, value) in [
        ("Center X", x),
        ("Center Y", y),
        ("Outer radius", outer),
        ("Inner radius", inner),
    ] {
        let out = frame(ctx, e, vec![]);
        type_into_annulus_row(ctx, e, &out, label, value);
    }
}

fn press(ctx: &egui::Context, e: &mut Editor, label: &str) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, label));
}

fn shows(ctx: &egui::Context, e: &mut Editor, text: &str) -> bool {
    let out = frame(ctx, e, vec![]);
    painted(&out, text)
}

#[test]
fn the_open_forms_apply_through_the_windows_own_predicate_and_keep_their_drafts() {
    let root = tempfile::tempdir().expect("root");
    let (path, source) = circle_document(root.path());
    let sketch = source.circle_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(!w.ready(), "nothing to apply before a form is open");
    assert!(w.form().begin_circle_edit(&path, &source, sketch));
    assert!(w.creates.busy(), "New and Open still wait for this form");
    assert!(
        w.ready(),
        "the form's own Apply must not be blocked by the form"
    );
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    type_circle(&ctx, w.form(), "-3.5", "4.25", "6.75");

    // The window says "not ready": the button is there and does nothing.
    w.form().set_analytic_apply(false);
    press(&ctx, w.form(), "Apply circle");
    assert!(
        w.form().take_apply_circle_request().is_none(),
        "disabled asked"
    );

    // The window says what it computes: one press, one request, with the typed
    // numbers whether or not the draft was confirmed.
    w.tell(true);
    press(&ctx, w.form(), "Apply circle");
    let request = w.form().take_apply_circle_request().expect("Apply asked");
    assert_eq!(request.expected, source.version);
    assert_eq!(request.sketch, sketch);
    assert_eq!(request.edit.center_mm, [-3.5, 4.25]);
    assert_eq!(request.edit.radius_mm, 6.75);
    assert_eq!(
        request.edit.curve_id,
        source.circle_sketches[0]
            .circle
            .as_ref()
            .expect("saved")
            .curve_id
    );
    assert!(
        w.form().take_apply_circle_request().is_none(),
        "one press, one request"
    );
    assert!(
        w.ready(),
        "the command must allow the same request as its button"
    );
    assert!(
        w.form().editing_analytic(),
        "pressing Apply discarded the draft"
    );
    assert_eq!(w.form().circle.radius, "6.75");

    let (path, source) = annulus_document(root.path());
    let sketch = source.annulus_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin_annulus_edit(&path, &source, sketch));
    assert!(w.ready());
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    type_annulus(&ctx, w.form(), "-3.5", "4.25", "9.25", "3.125");
    w.tell(true);
    press(&ctx, w.form(), "Apply annulus");
    let request = w.form().take_apply_annulus_request().expect("Apply asked");
    let saved = source.annulus_sketches[0].annulus.as_ref().expect("saved");
    assert_eq!(request.expected, source.version);
    assert_eq!(request.edit.center_mm, [-3.5, 4.25]);
    assert_eq!(request.edit.outer_radius_mm, 9.25);
    assert_eq!(request.edit.inner_radius_mm, 3.125);
    assert_eq!(request.edit.outer_curve_id, saved.outer_curve_id);
    assert_eq!(request.edit.inner_curve_id, saved.inner_curve_id);
    assert!(w.form().take_apply_annulus_request().is_none());
    assert!(w.form().editing_analytic());
}

#[test]
fn nothing_else_running_or_open_lets_the_command_start() {
    let root = tempfile::tempdir().expect("root");
    let (path, source) = circle_document(root.path());
    let sketch = source.circle_sketches[0].sketch;

    // A load in flight, a session operation: each blocks.
    let mut w = Window::open(&path);
    assert!(w.form().begin_circle_edit(&path, &source, sketch));
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

    // The height form open beside it.
    let mut w = Window::open(&path);
    assert!(w.form().begin_circle_edit(&path, &source, sketch));
    assert!(w.edits.begin(&path, &source));
    assert!(!w.ready(), "the height form being open must block Apply");

    // No form at all, a form that was closed, and no session.
    let mut w = Window::open(&path);
    assert!(!w.ready());
    assert!(w.form().begin_circle_edit(&path, &source, sketch));
    w.form().dismiss();
    assert!(!w.ready());
    let mut w = Window::open(&path);
    assert!(w.form().begin_circle_edit(&path, &source, sketch));
    w.sessions = sessions::Sessions::default();
    assert!(!w.ready(), "no open document, nothing to apply into");

    // Another editor's form, and a Sketch of the wrong kind, never start it.
    let (plate, plate_source) = plate_document(root.path());
    let mut w = Window::open(&plate);
    assert!(
        w.form()
            .begin_edit(&plate, &plate_source, plate_source.sketches[0].sketch)
    );
    assert!(!w.ready(), "the vertex form is not an analytic form");
    w.form().dismiss();
    assert!(
        !w.form()
            .begin_circle_edit(&plate, &plate_source, plate_source.sketches[0].sketch),
        "a Line plate is not a Circle"
    );
    assert!(
        !w.form()
            .begin_annulus_edit(&plate, &plate_source, plate_source.sketches[0].sketch),
        "a Line plate is not an annulus"
    );
    assert!(!w.ready());
    let mut w = Window::open(&path);
    assert!(
        !w.form().begin_annulus_edit(&path, &source, sketch),
        "a Circle is not an annulus"
    );
}

#[test]
fn the_stored_numbers_offer_no_apply_and_a_draft_round_trip_keeps_its_redo() {
    let root = tempfile::tempdir().expect("root");
    let (path, source) = circle_document(root.path());
    let sketch = source.circle_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin_circle_edit(&path, &source, sketch));
    w.tell(false);
    let ctx = egui::Context::default();
    settle(&ctx, w.form());

    // As opened: the stored numbers, so nothing to apply.
    assert!(shows(&ctx, w.form(), "nothing to apply"));
    press(&ctx, w.form(), "Apply circle");
    assert!(w.form().take_apply_circle_request().is_none());

    // Typed and confirmed, then Undo draft: the draft's Redo is there.
    type_circle(&ctx, w.form(), "1", "2", "3");
    press(&ctx, w.form(), "Confirm draft numbers");
    assert_eq!(w.form().circle_undo.len(), 1);
    press(&ctx, w.form(), "Undo draft");
    assert_eq!(w.form().circle_redo.len(), 1);
    assert_eq!(
        w.form().circle.radius,
        "10.5",
        "Undo draft restores the stored numbers"
    );

    // Back at the stored numbers: still nothing to apply, and the press that
    // asks for nothing leaves the draft's Redo where it was.
    assert!(shows(&ctx, w.form(), "nothing to apply"));
    press(&ctx, w.form(), "Apply circle");
    assert!(w.form().take_apply_circle_request().is_none());
    assert_eq!(
        w.form().circle_redo.len(),
        1,
        "the empty press lost the Redo"
    );

    // Retyping the same numbers differently written is the same numbers.
    type_circle(&ctx, w.form(), "12.50", "-7.250", "10.50");
    assert!(shows(&ctx, w.form(), "nothing to apply"));

    // The same for the annulus.
    let (path, source) = annulus_document(root.path());
    let sketch = source.annulus_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin_annulus_edit(&path, &source, sketch));
    w.tell(false);
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    assert!(shows(&ctx, w.form(), "nothing to apply"));
    type_annulus(&ctx, w.form(), "12.5", "-7.25", "10.5", "4.5");
    assert!(!shows(&ctx, w.form(), "nothing to apply"));
    type_annulus(&ctx, w.form(), "12.5", "-7.25", "10.5", "4.75");
    assert!(shows(&ctx, w.form(), "nothing to apply"));
    press(&ctx, w.form(), "Apply annulus");
    assert!(w.form().take_apply_annulus_request().is_none());
}

#[test]
fn the_draft_confirmation_is_not_the_apply_and_both_are_named_honestly() {
    let root = tempfile::tempdir().expect("root");
    let (path, source) = circle_document(root.path());
    let sketch = source.circle_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin_circle_edit(&path, &source, sketch));
    w.tell(false);
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    type_circle(&ctx, w.form(), "1", "2", "3");
    let out = frame(&ctx, w.form(), vec![]);
    assert!(painted(&out, "Apply circle"));
    assert!(painted(&out, "Confirm draft numbers"));
    assert!(
        !painted(&out, "Apply circle change"),
        "the draft confirmation still carries the name of the document action"
    );
    press(&ctx, w.form(), "Confirm draft numbers");
    assert!(
        w.form().take_apply_circle_request().is_none(),
        "confirming a draft asked for the document to change"
    );
    assert_eq!(
        w.form().circle_undo.len(),
        1,
        "one confirmation, one draft step"
    );
    // Save copy is offered once the draft is confirmed, on a clean document.
    press(&ctx, w.form(), "Save edited circle copy…");
    assert!(w.form().take_circle_edit_request().is_some());

    let (path, source) = annulus_document(root.path());
    let sketch = source.annulus_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin_annulus_edit(&path, &source, sketch));
    w.tell(false);
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    type_annulus(&ctx, w.form(), "1", "2", "9", "3");
    let out = frame(&ctx, w.form(), vec![]);
    assert!(painted(&out, "Apply annulus"));
    assert!(painted(&out, "Confirm draft numbers"));
    assert!(!painted(&out, "Apply annulus change"));
    press(&ctx, w.form(), "Confirm draft numbers");
    assert!(w.form().take_apply_annulus_request().is_none());
    press(&ctx, w.form(), "Save edited annulus copy…");
    assert!(w.form().take_annulus_edit_request().is_some());
}

#[test]
fn with_unsaved_changes_save_copy_is_withheld_with_words_and_apply_is_not() {
    let root = tempfile::tempdir().expect("root");
    let (path, source) = circle_document(root.path());
    let sketch = source.circle_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin_circle_edit(&path, &source, sketch));
    w.tell(true);
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    type_circle(&ctx, w.form(), "1", "2", "3");
    press(&ctx, w.form(), "Confirm draft numbers");
    assert!(
        shows(&ctx, w.form(), "unsaved changes"),
        "the withheld copy workflow was not explained"
    );
    press(&ctx, w.form(), "Save edited circle copy…");
    assert!(
        w.form().take_circle_edit_request().is_none(),
        "a withheld copy asked"
    );
    press(&ctx, w.form(), "Apply circle");
    assert!(w.form().take_apply_circle_request().is_some());
    // The same draft on a clean document keeps the old copy workflow.
    w.tell(false);
    press(&ctx, w.form(), "Save edited circle copy…");
    assert!(w.form().take_circle_edit_request().is_some());

    let (path, source) = annulus_document(root.path());
    let sketch = source.annulus_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin_annulus_edit(&path, &source, sketch));
    w.tell(true);
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    type_annulus(&ctx, w.form(), "1", "2", "9", "3");
    press(&ctx, w.form(), "Confirm draft numbers");
    assert!(shows(&ctx, w.form(), "unsaved changes"));
    press(&ctx, w.form(), "Save edited annulus copy…");
    assert!(w.form().take_annulus_edit_request().is_none());
    press(&ctx, w.form(), "Apply annulus");
    assert!(w.form().take_apply_annulus_request().is_some());
    w.tell(false);
    press(&ctx, w.form(), "Save edited annulus copy…");
    assert!(w.form().take_annulus_edit_request().is_some());
}

#[test]
fn a_new_accepted_version_ends_the_drafts_and_keeps_what_the_window_said() {
    let root = tempfile::tempdir().expect("root");
    let (path, source) = circle_document(root.path());
    let sketch = source.circle_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin_circle_edit(&path, &source, sketch));
    w.tell(true);
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    type_circle(&ctx, w.form(), "1", "2", "3");
    w.form().finish_session_change();
    assert!(
        !w.form().active(),
        "the draft outlived the picture it described"
    );
    assert!(!w.form().editing_analytic());
    // The next form for the next version starts with what the window already said.
    assert!(w.form().begin_circle_edit(&path, &source, sketch));
    settle(&ctx, w.form());
    type_circle(&ctx, w.form(), "1", "2", "3");
    press(&ctx, w.form(), "Apply circle");
    assert!(
        w.form().take_apply_circle_request().is_some(),
        "the flags were lost when the draft was replaced"
    );

    let (path, source) = annulus_document(root.path());
    let sketch = source.annulus_sketches[0].sketch;
    let mut w = Window::open(&path);
    assert!(w.form().begin_annulus_edit(&path, &source, sketch));
    w.tell(true);
    w.form().finish_session_change();
    assert!(!w.form().active());
    assert!(w.form().begin_annulus_edit(&path, &source, sketch));
    settle(&ctx, w.form());
    type_annulus(&ctx, w.form(), "1", "2", "9", "3");
    press(&ctx, w.form(), "Apply annulus");
    assert!(w.form().take_apply_annulus_request().is_some());
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
        |ui| e.draw_choices(ui, false, Some(path), Some(source)),
    );
    out.textures_delta.clear();
    out
}

/// With unsaved changes the window's copy-workflow gate is closed (`can_begin`
/// false). The forms are still offered, because they apply into the document.
#[test]
fn the_forms_open_on_a_document_with_unsaved_changes_only_when_the_window_says_so() {
    let root = tempfile::tempdir().expect("root");
    for (circle, entry) in [(true, "Edit circle"), (false, "Edit annulus")] {
        let (path, source) = if circle {
            circle_document(root.path())
        } else {
            annulus_document(root.path())
        };
        for (session_idle, opens) in [(false, false), (true, true)] {
            let mut e = Editor::default();
            e.set_session(session_idle, false, true);
            let ctx = egui::Context::default();
            choice_frame(&ctx, &mut e, &path, &source, vec![]);
            let out = choice_frame(&ctx, &mut e, &path, &source, vec![]);
            let at = out
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text().starts_with(entry) => {
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
            assert_eq!(e.active(), opens, "{entry}, session_idle={session_idle}");
        }
    }
}

/// The session's own line is drawn inside the form, where a refusal can be read.
#[test]
fn the_documents_last_outcome_is_readable_inside_the_open_form() {
    let root = tempfile::tempdir().expect("root");
    let (path, source) = circle_document(root.path());
    let sketch = source.circle_sketches[0].sketch;
    let mut e = Editor::default();
    assert!(e.begin_circle_edit(&path, &source, sketch));
    let ctx = egui::Context::default();
    let line = "Could not apply the change: the radius is too large";
    let draw = |e: &mut Editor, outcome: &str| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                ..Default::default()
            },
            |ui| e.draw(ui, true, false, outcome),
        );
        out.textures_delta.clear();
        out
    };
    for _ in 0..3 {
        draw(&mut e, "");
    }
    let out = draw(&mut e, line);
    assert!(painted(&out, line), "the refusal is not on the screen");
    // It is the text of a shape the form's window clips, never one beneath it.
    let at = text_at(&out, &format!("Document: {line}"));
    let window = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Edit saved Circle" => Some(s.clip_rect),
            _ => None,
        })
        .expect("the form's title");
    assert!(
        window.contains(at),
        "the refusal is outside the form's window"
    );
    assert!(
        !painted(&draw(&mut e, ""), line),
        "an empty line paints nothing"
    );

    let (path, source) = annulus_document(root.path());
    let mut e = Editor::default();
    assert!(e.begin_annulus_edit(&path, &source, source.annulus_sketches[0].sketch));
    for _ in 0..3 {
        draw(&mut e, "");
    }
    assert!(painted(&draw(&mut e, line), line));
}

/// While a worker is busy the forms offer nothing: a second press starts no work.
#[test]
fn while_a_worker_is_busy_the_forms_start_no_second_request() {
    let root = tempfile::tempdir().expect("root");
    let (path, source) = circle_document(root.path());
    let sketch = source.circle_sketches[0].sketch;
    let mut e = Editor::default();
    assert!(e.begin_circle_edit(&path, &source, sketch));
    e.set_analytic_apply(true);
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame_running(&ctx, &mut e, vec![], true);
    }
    let out = frame_running(&ctx, &mut e, vec![], true);
    click_running(&ctx, &mut e, text_at(&out, "Apply circle"));
    assert!(e.take_apply_circle_request().is_none());
}

fn click_running(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
    frame_running(ctx, e, vec![egui::Event::PointerMoved(at)], true);
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
            true,
        );
    }
}

/// The Circle form's own widgets, as the window leaves them for an idle session
/// with unsaved changes: the numbers typed and **Apply circle** pressed.
pub(crate) fn typed_circle(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    center: [&str; 2],
    radius: &str,
) -> (Editor, EditCircleRequest) {
    let mut e = Editor::default();
    e.set_session(true, false, true);
    e.set_analytic_apply(true);
    assert!(e.begin_circle_edit(path, source, sketch));
    let ctx = egui::Context::default();
    settle(&ctx, &mut e);
    type_circle(&ctx, &mut e, center[0], center[1], radius);
    press(&ctx, &mut e, "Apply circle");
    let request = e.take_apply_circle_request().expect("Apply circle asked");
    (e, request)
}

/// The annulus form's own widgets, likewise.
pub(crate) fn typed_annulus(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    center: [&str; 2],
    outer: &str,
    inner: &str,
) -> (Editor, EditAnnulusRequest) {
    let mut e = Editor::default();
    e.set_session(true, false, true);
    e.set_analytic_apply(true);
    assert!(e.begin_annulus_edit(path, source, sketch));
    let ctx = egui::Context::default();
    settle(&ctx, &mut e);
    type_annulus(&ctx, &mut e, center[0], center[1], outer, inner);
    press(&ctx, &mut e, "Apply annulus");
    let request = e.take_apply_annulus_request().expect("Apply annulus asked");
    (e, request)
}
