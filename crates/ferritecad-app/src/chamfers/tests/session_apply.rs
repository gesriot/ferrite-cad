// SPDX-License-Identifier: MIT
//! §30H: real widgets using the handler's production predicate.
use super::*;
use crate::{Loads, can_apply_chamfer_distance, creates, edits, exports, sessions};

/// The corner this slice chamfers: the second and third Lines of the clockwise
/// plate, neither its first corner nor its first Line.
pub(crate) const CORNER: [f64; 2] = [-4.5, 3.25];

/// [`crate::fillets::tests::plate`] with one Chamfer of 2.375 mm at [`CORNER`],
/// written by the shipped preparation and writer, then made awkward: the base
/// Extrude owns one more name that resolves, every object is named `same`, SQL
/// rowids and ordinals run against the feature history, capability rowids move,
/// and an optional capability and an unrelated table ride along.
pub(crate) fn fixture() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let (root, path, source) = crate::fillets::tests::plate();
    let body = source.chamfer_bodies[0].body;
    let target = source.chamfer_bodies[0].target.clone().expect("a target");
    let corner = target
        .corners
        .iter()
        .find(|c| c.corner_mm == CORNER)
        .expect("that corner");
    let mut d = Document::open(&path).expect("writable");
    let prepared = ferritecad_document::prepare_edge_chamfer(
        &d,
        body,
        &EdgeChamfer {
            edge: SweptEdge {
                feature: corner.feature,
                joint: corner.joint,
            },
            distance_mm: 2.375,
        },
    )
    .expect("prepared");
    d.write_edge_chamfer(&prepared).expect("written");
    let mut extra = d
        .topology_refs()
        .expect("refs")
        .into_iter()
        .find(|r| r.owner == target.base_feature)
        .expect("a base-owned name");
    extra.id = StableEntityId::new();
    d.write(|w| w.put_topology_ref(&extra))
        .expect("one more base-owned name");
    d.close().expect("close");
    let db = rusqlite::Connection::open(&path).expect("SQL");
    db.execute_batch(
        "UPDATE objects SET rowid=-rowid, ordinal=100-ordinal, name='same';
        UPDATE capabilities SET rowid=rowid+100;
        INSERT INTO capabilities(rowid,name,required) VALUES(900,'optional.extension',0);
        CREATE TABLE extra_data(id INTEGER PRIMARY KEY,bytes BLOB);
        INSERT INTO extra_data VALUES(91,x'010203');",
    )
    .expect("reordered fixture");
    drop(db);
    let reading = ferritecad_jobs::read_extrude_source(&path).expect("reading");
    (root, path, reading)
}
pub(crate) fn saved_of(reading: &ExtrudeEditSource) -> SavedChamfer {
    let [choice] = reading.chamfer_features.as_slice() else {
        panic!("one saved Chamfer: {:?}", reading.chamfer_features);
    };
    assert_eq!(choice.refusal, None);
    choice.saved.clone().expect("editable")
}
fn ready(c: &creates::Creates, s: &sessions::Sessions) -> bool {
    can_apply_chamfer_distance(
        c,
        &Loads::default(),
        &exports::Exports::default(),
        &edits::Edits::default(),
        s,
    )
}
fn tell(c: &mut creates::Creates, s: &sessions::Sessions) {
    c.sketch.chamfers.set_session(true, ready(c, s), s.dirty());
}
fn opened(path: &Path) -> sessions::Sessions {
    let mut s = sessions::Sessions::default();
    s.adopt(ferritecad_jobs::DocumentSession::open(path).expect("session"));
    s
}
/// Opens the distance form on `reading`, proves the production predicate offers
/// Apply to it while idle, types `value` without confirming it and presses
/// **Apply distance**: the request the handler receives.
pub(crate) fn typed_distance(
    path: &Path,
    reading: &ExtrudeEditSource,
    s: &sessions::Sessions,
    feature: ObjectId,
    value: &str,
) -> (crate::sketch::Editor, EditChamferDistanceRequest) {
    let mut c = creates::Creates::default();
    assert!(c.sketch.chamfers.begin_distance(path, reading, feature));
    assert!(ready(&c, s), "own form allows a changed idle request");
    tell(&mut c, s);
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut c.sketch.chamfers, false);
    }
    new_distance(&ctx, &mut c.sketch.chamfers, value);
    click(&ctx, &mut c.sketch.chamfers, "Apply distance");
    let r = c
        .sketch
        .chamfers
        .take_apply_request()
        .expect("changed widget request");
    assert_eq!(r.feature, feature);
    assert_eq!(r.expected, reading.version);
    assert_eq!(
        c.sketch.chamfers.distance.as_ref().expect("draft").typed,
        value,
        "Apply keeps the exact text"
    );
    (c.sketch, r)
}
pub(crate) fn draft_state(e: &crate::sketch::Editor) -> String {
    format!("{:?}", e.chamfers.distance)
}
pub(crate) fn visible_outcome(e: &mut crate::sketch::Editor, outcome: &str) {
    e.chamfers.set_outcome(outcome);
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e.chamfers, false);
    }
    assert!(painted(&frame(&ctx, &mut e.chamfers, false), outcome));
}

/// Apply reads the current unconfirmed text; Confirm draft number only feeds the
/// form's own request history and the clean copy; a numerically unchanged
/// distance offers no Apply but still a copy; the Add form has no session Apply.
#[test]
fn distance_current_text_draft_history_noop_clean_copy_and_add_exclusion() {
    let (_root, path, reading) = fixture();
    let s = opened(&path);
    let chosen = saved_of(&reading);
    assert_eq!(chosen.corner.corner_mm, CORNER);
    let (form, r) = typed_distance(&path, &reading, &s, chosen.feature, "3.06250");
    assert_eq!(r.distance_mm, 3.0625);
    let mut c = creates::Creates::default();
    c.sketch = form;
    let e = &mut c.sketch.chamfers;
    assert!(
        !e.distance.as_ref().expect("draft").history.can_undo(),
        "Apply is not a draft confirmation"
    );
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, e, false);
    }
    assert!(painted(
        &frame(&ctx, e, false),
        "Confirm the draft number before saving a copy."
    ));
    // The form's own history: confirmed requests, Undo/Redo request, no Apply.
    click(&ctx, e, "Confirm draft number");
    assert!(e.take_apply_request().is_none());
    new_distance(&ctx, e, "4.5");
    click(&ctx, e, "Confirm draft number");
    assert!(e.take_apply_request().is_none());
    let history = e.distance.as_ref().expect("draft").history.clone();
    assert_eq!(history.states, ["2.375", "3.06250", "4.5"]);
    click(&ctx, e, "Undo request");
    assert_eq!(e.distance.as_ref().expect("draft").typed, "3.06250");
    click(&ctx, e, "Redo request");
    assert_eq!(e.distance.as_ref().expect("draft").typed, "4.5");
    click(&ctx, e, "Undo request");
    assert!(
        e.take_apply_request().is_none(),
        "draft moves apply nothing"
    );
    // Apply after a draft Undo uses what the field now says.
    click(&ctx, e, "Apply distance");
    assert_eq!(
        e.take_apply_request().expect("current text").distance_mm,
        3.0625
    );
    assert!(e.distance.as_ref().expect("draft").history.can_redo());
    // The clean copy of the confirmed request.
    click(&ctx, e, "Save distance copy…");
    assert_eq!(
        e.take_distance_request().expect("clean copy").distance_mm,
        3.0625
    );
    // Another spelling of the stored number is no change.
    new_distance(&ctx, e, "2.3750");
    click(&ctx, e, "Apply distance");
    assert!(e.take_apply_request().is_none());
    assert!(painted(&frame(&ctx, e, false), "nothing to apply"));
    // CLI copy confirmation is still meaningful for the same numeric distance.
    click(&ctx, e, "Confirm draft number");
    click(&ctx, e, "Save distance copy…");
    assert_eq!(
        e.take_distance_request().expect("no-op copy").distance_mm,
        2.375
    );
    e.dismiss();
    // Add Chamfer: the plain plate's creation form has no session Apply.
    let (_plain_root, plain, plain_reading) = crate::fillets::tests::plate();
    assert!(
        c.sketch
            .chamfers
            .begin(&plain, &plain_reading, plain_reading.chamfer_bodies[0].body)
    );
    assert!(!ready(&c, &s), "Add has no session Apply");
    for _ in 0..3 {
        frame(&ctx, &mut c.sketch.chamfers, false);
    }
    let out = frame(&ctx, &mut c.sketch.chamfers, false);
    assert!(!painted(&out, "Apply distance"));
    assert!(painted(&out, "Confirm draft edge and distance"));
    assert!(
        painted(&out, "Add chamfer"),
        "§30K: the Add form has its own Add"
    );
}

/// The predicate first offers a changed idle request, then refuses it while
/// any load, session step, export, edit worker or other form is in the way.
#[test]
fn distance_busy_guard_proves_changed_idle_request_and_excludes_other_work() {
    let (_root, path, reading) = fixture();
    let mut s = opened(&path);
    let chosen = saved_of(&reading);
    let (form, _) = typed_distance(&path, &reading, &s, chosen.feature, "3.125");
    let mut c = creates::Creates::default();
    c.sketch = form;
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut c.sketch.chamfers, false);
    }
    for mode in 0..6 {
        let mut loads = Loads::default();
        let mut ex = exports::Exports::default();
        let mut ed = edits::Edits::default();
        let mut generation = None;
        let mut other = None;
        match mode {
            0 => {
                loads
                    .open(
                        Some(&path),
                        std::sync::Arc::new(crate::ProgressRelay::default()),
                        |_, _| std::thread::spawn(|| {}),
                    )
                    .expect("load");
            }
            1 => {
                generation = s.begin_apply(|_, _, _| std::thread::spawn(|| {}));
            }
            2 => {
                exports::begin_export(
                    &mut ex,
                    &mut crate::ViewportInput::default(),
                    Some(&path),
                    Some(path.with_extension("fbx")),
                    |_, _, _, _| std::thread::spawn(|| {}),
                )
                .expect("export");
            }
            3 => {
                assert!(ed.begin(&path, &reading));
            }
            4 => {
                // An existing Fillet's form beside this one: not its own form.
                let (root, fillets, fillet_reading) =
                    crate::fillets::tests::session_apply::fixture(1);
                let radius = crate::fillets::tests::session_apply::selected(&fillet_reading, 0);
                let fs = opened(&fillets);
                let (editor, _) = crate::fillets::tests::session_apply::typed_radius(
                    &fillets,
                    &fillet_reading,
                    &fs,
                    radius.feature,
                    "2.5",
                );
                c.sketch.fillets = editor.fillets;
                other = Some((root, fs));
            }
            _ => {
                // New's form stays open: it is the last mode.
                let form = std::mem::take(&mut c.sketch);
                assert!(creates::open_form(
                    &mut c,
                    &mut crate::ViewportInput::default()
                ));
                c.sketch = form;
            }
        }
        let available = can_apply_chamfer_distance(&c, &loads, &ex, &ed, &s);
        assert!(!available, "busy mode {mode}");
        c.sketch.chamfers.set_session(true, available, s.dirty());
        click(&ctx, &mut c.sketch.chamfers, "Apply distance");
        assert!(c.sketch.chamfers.take_apply_request().is_none());
        if let Some(g) = generation {
            s.cancel();
            s.finish_apply(g, Err(CadError::Cancelled));
        }
        if other.is_some() {
            c.sketch.fillets = crate::fillets::Editor::default();
        }
        loads.stop_all();
        ex.stop_all();
        if mode < 5 {
            assert!(ready(&c, &s), "mode {mode} left the guard closed");
        }
    }
    c = creates::Creates::default();
    assert!(c.sketch.begin_edit(&path, &reading, chosen.profile));
    assert!(!ready(&c, &s), "another editor");
    c.sketch.dismiss();
    assert!(
        c.sketch
            .chamfers
            .begin_distance(&path, &reading, chosen.feature)
    );
    assert!(!ready(&c, &sessions::Sessions::default()), "no session");
}

/// With unsaved changes the existing Chamfer's form opens on the accepted
/// snapshot, Add stays shut, the copy says why and the handler refuses it before
/// any dialog; refused text stays exactly as typed, the document outcome is read
/// inside the form, and an accepted scene closes it.
#[test]
fn distance_dirty_discovery_copy_reason_refusal_retains_text_and_acceptance_closes_form() {
    let (_root, path, reading) = fixture();
    let original = std::fs::read(&path).expect("source");
    let mut s = opened(&path);
    let chosen = saved_of(&reading);
    assert!(
        !crate::refuse_unsaved_distance_copy(&mut s),
        "a clean copy is offered"
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let g = s
        .begin_apply(|t, _, _| {
            std::thread::spawn(move || {
                tx.send(t.run(|src, _, dst| {
                    std::fs::copy(src, dst).expect("copy");
                    let mut d = Document::open(dst)?;
                    let o = d.object(chosen.base_feature)?.expect("base");
                    d.write(|w| {
                        w.put_object(o.id, o.parent, o.ordinal, Some("Dirty base"), &o.payload)
                    })?;
                    d.close()
                }))
                .expect("deliver");
            })
        })
        .expect("step");
    assert!(matches!(
        s.finish_apply(g, rx.recv().expect("answer")),
        sessions::Edited::Show(_)
    ));
    s.bind(sessions::Bind::Staged).expect("bind");
    s.finish_scene(g, Ok(()));
    assert!(s.dirty());
    let current = s.export_path().expect("current");
    assert_ne!(current, path, "the accepted snapshot, not the logical file");
    let reading = ferritecad_jobs::read_extrude_source(&current).expect("reading");
    let mut c = creates::Creates::default();
    tell(&mut c, &s);
    let ctx = egui::Context::default();
    let choice_frame = |c: &mut creates::Creates,
                        source: &ExtrudeEditSource,
                        file: &Path,
                        can_begin: bool,
                        events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(988., 768.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                c.sketch
                    .chamfers
                    .choices(ui, can_begin, Some(file), Some(source))
            },
        );
        output.textures_delta.clear();
        output
    };
    let press = |c: &mut creates::Creates,
                 source: &ExtrudeEditSource,
                 file: &Path,
                 can_begin: bool,
                 label: &str| {
        let mut out = choice_frame(c, source, file, can_begin, Vec::new());
        for _ in 0..3 {
            out = choice_frame(c, source, file, can_begin, Vec::new());
        }
        let at = find(&out, label).unwrap_or_else(|| panic!("not painted: {label}"));
        for pressed in [true, false] {
            choice_frame(
                c,
                source,
                file,
                can_begin,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    };
    // §30K: Add Chamfer adds into the open document, so it opens while dirty when
    // the session is idle (it was shut here before §30K).
    let (_plain_root, plain, plain_reading) = crate::fillets::tests::plate();
    press(&mut c, &plain_reading, &plain, true, "Chamfer edge of");
    assert!(c.sketch.chamfers.adding(), "Add opens while dirty and idle");
    c.sketch.chamfers.dismiss();
    // The existing Chamfer's form opens though the copy workflows' gate is shut.
    press(
        &mut c,
        &reading,
        &current,
        false,
        &format!("Edit Chamfer distance same — {}", chosen.feature),
    );
    assert!(c.sketch.chamfers.editing_distance());
    assert!(ready(&c, &s), "the dirty form may Apply");
    tell(&mut c, &s);
    let e = &mut c.sketch.chamfers;
    assert_eq!(
        e.distance.as_ref().expect("draft").source,
        current,
        "the form reads the accepted snapshot"
    );
    for _ in 0..3 {
        frame(&ctx, e, false);
    }
    let past = chosen.corner.max_distance_mm.next_up().to_string();
    for (text, why) in [
        ("banana", "finite"),
        ("0", "at least"),
        (past.as_str(), "too large"),
    ] {
        new_distance(&ctx, e, text);
        click(&ctx, e, "Apply distance");
        assert!(e.take_apply_request().is_none(), "{text}");
        assert_eq!(e.distance.as_ref().expect("draft").typed, text);
        assert!(painted(&frame(&ctx, e, false), why), "{text}: {why}");
    }
    new_distance(&ctx, e, "3.125");
    click(&ctx, e, "Confirm draft number");
    click(&ctx, e, "Save distance copy…");
    assert!(e.take_distance_request().is_none(), "dirty copy");
    assert!(painted(&frame(&ctx, e, false), "unsaved changes"));
    // The handler refuses it again before any dialog, in words.
    assert!(crate::refuse_unsaved_distance_copy(&mut s));
    assert!(s.status.contains("unsaved changes"), "{}", s.status);
    e.set_outcome("Could not apply the change: Chamfer UUID solved bound");
    assert!(painted(&frame(&ctx, e, false), "Chamfer UUID solved bound"));
    assert_eq!(e.distance.as_ref().expect("draft").typed, "3.125");
    assert_eq!(std::fs::read(&path).expect("source"), original);
    c.sketch.finish_session_change();
    assert!(!c.sketch.active());
    assert_eq!(c.sketch.chamfers.session(), (true, true, true));
}
