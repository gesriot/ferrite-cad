// SPDX-License-Identifier: MIT
//! §30E: actual angle widgets using the window's production availability predicate.
use super::*;
use crate::{Loads, can_apply_angle, creates, edits, exports, sessions};
use ferritecad_document::ExtrudeEditSource;

pub(crate) const RADIAL: [[f64; 2]; 4] = [[2.75, -3.25], [7.5, -3.25], [7.5, 9.5], [2.75, 9.5]];

struct Window {
    _root: tempfile::TempDir,
    sessions: sessions::Sessions,
    creates: creates::Creates,
    loads: Loads,
    exports: exports::Exports,
    edits: edits::Edits,
}
impl Window {
    fn open(path: &Path) -> Self {
        let root = tempfile::tempdir().expect("root");
        let mut sessions = sessions::Sessions::default();
        sessions.adopt(
            ferritecad_jobs::DocumentSession::open_in(
                root.path(),
                path,
                ferritecad_jobs::HistoryLimits::default(),
            )
            .expect("session"),
        );
        Self {
            _root: root,
            sessions,
            creates: Default::default(),
            loads: Default::default(),
            exports: Default::default(),
            edits: Default::default(),
        }
    }
    fn ready(&self) -> bool {
        can_apply_angle(
            &self.creates,
            &self.loads,
            &self.exports,
            &self.edits,
            &self.sessions,
        )
    }
    fn tell(&mut self) {
        self.creates.sketch.set_angle_apply(self.ready());
        self.creates
            .sketch
            .set_session(true, false, self.sessions.dirty());
    }
    fn form(&mut self) -> &mut Editor {
        &mut self.creates.sketch
    }
}
fn fixture() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("sector.fcad");
    crate::constraints::tests::revolve::write_sector(&path, &RADIAL, 137.5, None);
    let reading = ferritecad_jobs::read_extrude_source(&path).expect("reading");
    (root, path, reading)
}
fn settle(ctx: &egui::Context, e: &mut Editor) {
    for _ in 0..3 {
        frame(ctx, e, vec![]);
    }
}
fn type_angle(ctx: &egui::Context, e: &mut Editor, value: &str) {
    let out = frame(ctx, e, vec![]);
    let column = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Angle" => {
                Some(t.visual_bounding_rect().right())
            }
            _ => None,
        })
        .expect("angle row");
    replace_box_on_row(ctx, e, &out, column, "Angle", value);
}
fn press(ctx: &egui::Context, e: &mut Editor, label: &str) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, label));
}
/// Used by native gates too: never substitute a manually enabled Apply flag.
pub(crate) fn typed_angle(
    path: &Path,
    reading: &ExtrudeEditSource,
    sessions: &sessions::Sessions,
    value: &str,
) -> (Editor, EditRevolveAngleRequest) {
    let mut creates = creates::Creates::default();
    assert!(
        creates
            .sketch
            .begin_angle_edit(path, reading, reading.revolve_angles[0].feature)
    );
    let ready = can_apply_angle(
        &creates,
        &Loads::default(),
        &exports::Exports::default(),
        &edits::Edits::default(),
        sessions,
    );
    assert!(ready, "the form's own Apply must be available in idle");
    creates.sketch.set_angle_apply(ready);
    creates.sketch.set_session(true, false, sessions.dirty());
    let ctx = egui::Context::default();
    settle(&ctx, &mut creates.sketch);
    type_angle(&ctx, &mut creates.sketch, value);
    press(&ctx, &mut creates.sketch, "Apply angle");
    let request = creates
        .sketch
        .take_apply_angle_request()
        .expect("changed angle Apply asked");
    (creates.sketch, request)
}
#[test]
fn angle_apply_uses_current_fields_and_confirmation_is_only_draft_history() {
    let (_root, path, reading) = fixture();
    let mut w = Window::open(&path);
    assert!(!w.ready());
    assert!(
        w.form()
            .begin_angle_edit(&path, &reading, reading.revolve_angles[0].feature)
    );
    assert!(w.ready(), "own form blocks Apply");
    w.tell();
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    type_angle(&ctx, w.form(), "212.25");
    press(&ctx, w.form(), "Apply angle");
    let r = w.form().take_apply_angle_request().expect("Apply");
    assert_eq!(r.degrees, 212.25);
    assert_eq!(r.expected, reading.version);
    assert_eq!(r.feature, reading.revolve_angles[0].feature);
    assert!(w.form().angle_undo.is_empty());
    assert!(w.form().editing_saved_angle());
    let out = frame(&ctx, w.form(), vec![]);
    assert!(!painted(&out, "Apply angle change"));
    press(&ctx, w.form(), "Confirm draft numbers");
    assert!(w.form().take_apply_angle_request().is_none());
    assert_eq!(w.form().angle_undo, ["137.5"]);
    press(&ctx, w.form(), "Undo draft");
    assert_eq!(w.form().angle_edit, "137.5");
    press(&ctx, w.form(), "Apply angle");
    assert!(w.form().take_apply_angle_request().is_none());
    assert_eq!(w.form().angle_redo, ["212.25"]);
    type_angle(&ctx, w.form(), "137.500");
    assert!(painted(&frame(&ctx, w.form(), vec![]), "nothing to apply"));
    press(&ctx, w.form(), "Apply angle");
    assert!(w.form().take_apply_angle_request().is_none());
    assert_eq!(w.form().angle_redo.len(), 1);
    press(&ctx, w.form(), "Redo draft");
    assert_eq!(w.form().angle_edit, "212.25");
    type_angle(&ctx, w.form(), "198.125");
    press(&ctx, w.form(), "Apply angle");
    assert_eq!(
        w.form()
            .take_apply_angle_request()
            .expect("unconfirmed fields")
            .degrees,
        198.125
    );
    assert!(
        crate::form_open(&w.edits, &w.creates.sketch),
        "document Undo waits for the form"
    );
}
#[test]
fn angle_apply_busy_guards_first_prove_a_changed_request_is_available() {
    let (_root, path, reading) = fixture();
    let mut w = Window::open(&path);
    assert!(
        w.form()
            .begin_angle_edit(&path, &reading, reading.revolve_angles[0].feature)
    );
    w.tell();
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    type_angle(&ctx, w.form(), "212.25");
    press(&ctx, w.form(), "Apply angle");
    assert!(
        w.form().take_apply_angle_request().is_some(),
        "idle changed request must work before testing guards"
    );
    w.loads
        .open(
            Some(&path),
            std::sync::Arc::new(crate::ProgressRelay::default()),
            |_, _| std::thread::spawn(|| {}),
        )
        .expect("load");
    assert!(!w.ready());
    w.tell();
    press(&ctx, w.form(), "Apply angle");
    assert!(w.form().take_apply_angle_request().is_none());
    w.loads.stop_all();
    w.loads = Loads::default();
    assert!(w.ready());
    let busy_generation = w
        .sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("busy");
    assert!(!w.ready());
    w.tell();
    press(&ctx, w.form(), "Apply angle");
    assert!(w.form().take_apply_angle_request().is_none());
    w.sessions.cancel();
    w.sessions
        .finish_apply(busy_generation, Err(CadError::Cancelled));
    assert!(w.ready());
    let mut input = crate::ViewportInput::default();
    exports::begin_export(
        &mut w.exports,
        &mut input,
        Some(&path),
        Some(path.with_extension("fbx")),
        |_, _, _, _| std::thread::spawn(|| {}),
    )
    .expect("export");
    assert!(!w.ready());
    w.tell();
    press(&ctx, w.form(), "Apply angle");
    assert!(w.form().take_apply_angle_request().is_none());
    w.exports.stop_all();
    w.exports = exports::Exports::default();
    assert!(w.ready());
    w.exports.ask_stl(
        &path,
        vec![ferritecad_jobs::StlBody {
            id: ferritecad_types::ObjectId::new(),
            name: Some("Body".into()),
        }],
        &mut input,
    );
    assert!(!w.ready());
    w.exports = exports::Exports::default();
    std::fs::write(path.with_extension("fbx"), b"occupied").expect("occupied");
    exports::begin_export(
        &mut w.exports,
        &mut input,
        Some(&path),
        Some(path.with_extension("fbx")),
        |_, _, _, _| panic!("must wait for replacement"),
    );
    assert!(!w.ready(), "export replacement pending");
    w.exports = exports::Exports::default();
    assert!(w.ready());
    let plate = path.with_file_name("height-plate.fcad");
    ferritecad_jobs::create_document(
        ferritecad_jobs::CreateDocumentRequest::new(
            &plate,
            ferritecad_jobs::NewDocument::SamplePlate(Default::default()),
            "test",
        ),
        &ferritecad_kernel::OperationContext::default(),
    )
    .expect("plate");
    let plate_reading = ferritecad_jobs::read_extrude_source(&plate).expect("plate");
    assert!(w.edits.begin(&plate, &plate_reading));
    assert!(!w.ready());
    w.edits = edits::Edits::default();
    w.form().dismiss();
    assert!(creates::open_form(&mut w.creates, &mut input));
    assert!(
        w.form()
            .begin_angle_edit(&path, &reading, reading.revolve_angles[0].feature)
    );
    assert!(!w.ready(), "another New form");
}
#[test]
fn angle_apply_requires_its_own_form_and_session_and_supported_kind() {
    let (_root, path, reading) = fixture();
    let mut w = Window::open(&path);
    assert!(!w.ready());
    let sketch = reading.sketches[0].sketch;
    assert!(w.form().begin_edit(&path, &reading, sketch));
    assert!(!w.ready());
    w.form().dismiss();
    assert!(
        w.form()
            .begin_angle_edit(&path, &reading, reading.revolve_angles[0].feature)
    );
    w.sessions = sessions::Sessions::default();
    assert!(!w.ready());
    w.form().dismiss();
    assert!(!w.ready());
    assert!(!w.form().begin_angle_edit(&path, &reading, sketch));
    let mut d = ferritecad_document::Document::open(&path).expect("open");
    let id = reading.revolve_angles[0].feature;
    let o = d.object(id).expect("object").expect("Revolve");
    let ferritecad_document::ObjectPayload::Revolve(mut r) = o.payload else {
        panic!("Revolve")
    };
    r.extent = ferritecad_document::RevolveExtent::FullTurn;
    d.write(|wr| {
        wr.put_object(
            id,
            o.parent,
            o.ordinal,
            o.name.as_deref(),
            &ferritecad_document::ObjectPayload::Revolve(r),
        )
    })
    .expect("full turn");
    d.close().expect("close");
    let full = ferritecad_jobs::read_extrude_source(&path).expect("full");
    assert!(
        !w.form().begin_angle_edit(&path, &full, id),
        "full turn has no editable partial angle"
    );
}
#[test]
fn angle_range_refusal_status_and_accepted_scene_dismissal_are_visible() {
    let (_root, path, reading) = fixture();
    let mut w = Window::open(&path);
    assert!(
        w.form()
            .begin_angle_edit(&path, &reading, reading.revolve_angles[0].feature)
    );
    w.tell();
    let ctx = egui::Context::default();
    settle(&ctx, w.form());
    for value in ["360", "0", "-90", "359.991", "0.009", "NaN", "abc"] {
        type_angle(&ctx, w.form(), value);
        assert!(w.form().angle_edit_request().is_err(), "{value}");
        assert_eq!(w.form().angle_edit, value, "refusal keeps typed draft");
        assert!(w.form().take_apply_angle_request().is_none());
    }
    let status = "Could not apply the change: the document changed after this form was opened";
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 900.),
            )),
            ..Default::default()
        },
        |ui| w.form().draw(ui, true, false, status),
    );
    out.textures_delta.clear();
    assert!(painted(&out, status));
    let at = text_at(&out, &format!("Document: {status}"));
    let clip = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Edit saved Revolve angle" => {
                Some(s.clip_rect)
            }
            _ => None,
        })
        .expect("window");
    assert!(clip.contains(at), "status outside the form");
    w.form().finish_session_change();
    assert!(!w.form().active());
    assert!(
        w.form()
            .begin_angle_edit(&path, &reading, reading.revolve_angles[0].feature)
    );
    settle(&ctx, w.form());
    type_angle(&ctx, w.form(), "212.25");
    press(&ctx, w.form(), "Apply angle");
    assert!(
        w.form().take_apply_angle_request().is_some(),
        "dismiss retains frame availability"
    );
}

#[test]
fn angle_form_opens_dirty_and_withholds_only_the_copy_workflow() {
    let (_root, path, reading) = fixture();
    let mut w = Window::open(&path);
    // Kernel-free accepted model change, through the real session step. The
    // widget gate needs actual dirty state, not a manually set can_apply flag.
    let (tx, rx) = std::sync::mpsc::channel();
    let id = reading.revolve_angles[0].feature;
    let g = w
        .sessions
        .begin_apply(|ticket, _, _| {
            std::thread::spawn(move || {
                tx.send(ticket.run(|src, _, dst| {
                    std::fs::copy(src, dst).expect("private copy");
                    let mut d = ferritecad_document::Document::open(dst)?;
                    let o = d.object(id)?.expect("Revolve");
                    d.write(|writer| {
                        writer.put_object(id, o.parent, o.ordinal, Some("Changed name"), &o.payload)
                    })?;
                    d.close()
                }))
                .expect("deliver");
            })
        })
        .expect("step");
    assert!(matches!(
        w.sessions.finish_apply(g, rx.recv().expect("result")),
        sessions::Edited::Show(_)
    ));
    w.sessions.bind(sessions::Bind::Staged).expect("bind");
    w.sessions.finish_scene(g, Ok(()));
    assert!(w.sessions.dirty());
    let current = w.sessions.export_path().expect("current");
    let reading = ferritecad_jobs::read_extrude_source(&current).expect("reading");
    w.tell();
    let ctx = egui::Context::default();
    // The old copy-only entry gate is shut; the session entry must still open.
    click_saved_action(
        &ctx,
        w.form(),
        &current,
        &reading,
        &format!("Edit Revolve angle Changed name — {id}…"),
    );
    assert!(
        w.form().editing_saved_angle(),
        "dirty document must offer angle editing"
    );
    assert!(w.ready());
    w.tell();
    settle(&ctx, w.form());
    type_angle(&ctx, w.form(), "212.25");
    press(&ctx, w.form(), "Confirm draft numbers");
    let out = frame(&ctx, w.form(), vec![]);
    assert!(painted(&out, "unsaved changes"));
    press(&ctx, w.form(), "Save edited Revolve copy…");
    assert!(w.form().take_angle_edit_request().is_none());
    press(&ctx, w.form(), "Apply angle");
    assert!(w.form().take_apply_angle_request().is_some());
    // A new clean session with the same form keeps the previous copy workflow.
    let mut clean = Window::open(&path);
    let reading = ferritecad_jobs::read_extrude_source(&path).expect("reading");
    assert!(clean.form().begin_angle_edit(&path, &reading, id));
    clean.tell();
    let ctx = egui::Context::default();
    settle(&ctx, clean.form());
    type_angle(&ctx, clean.form(), "212.25");
    press(&ctx, clean.form(), "Confirm draft numbers");
    press(&ctx, clean.form(), "Save edited Revolve copy…");
    assert!(clean.form().take_angle_edit_request().is_some());
}

pub(crate) fn draft_state(e: &Editor) -> (bool, String, Vec<String>, Vec<String>) {
    (
        e.editing_saved_angle(),
        e.angle_edit.clone(),
        e.angle_undo.clone(),
        e.angle_redo.clone(),
    )
}
