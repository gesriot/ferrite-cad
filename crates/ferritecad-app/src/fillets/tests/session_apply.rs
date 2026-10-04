// SPDX-License-Identifier: MIT
//! §30G: real widgets using the handler's production predicate.
use super::*;
use crate::{Loads, can_apply_fillet_radius, creates, edits, exports, sessions};

pub(crate) fn fixture(count: usize) -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
    let spec = [
        ([33., 3.25], 2.375),
        ([-4.5, 15.5], 3.0625),
        ([33., 15.5], 1.5),
        ([-4.5, 3.25], 4.25),
    ];
    let (root, path, _) = rounded_n(&spec[..count]);
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
pub(crate) fn selected(reading: &ExtrudeEditSource, index: usize) -> SavedFillet {
    reading
        .fillet_features
        .iter()
        .find_map(|c| c.saved.as_ref().filter(|s| s.history_index == index + 1))
        .expect("history UUID")
        .clone()
}
fn ready(c: &creates::Creates, s: &sessions::Sessions) -> bool {
    can_apply_fillet_radius(
        c,
        &Loads::default(),
        &exports::Exports::default(),
        &edits::Edits::default(),
        s,
    )
}
fn tell(c: &mut creates::Creates, s: &sessions::Sessions) {
    c.sketch.fillets.set_session(true, ready(c, s), s.dirty());
}
fn opened(path: &Path) -> sessions::Sessions {
    let mut s = sessions::Sessions::default();
    s.adopt(ferritecad_jobs::DocumentSession::open(path).expect("session"));
    s
}
pub(crate) fn typed_radius(
    path: &Path,
    reading: &ExtrudeEditSource,
    s: &sessions::Sessions,
    feature: ObjectId,
    value: &str,
) -> (crate::sketch::Editor, EditFilletRadiusRequest) {
    let mut c = creates::Creates::default();
    assert!(c.sketch.fillets.begin_radius(path, reading, feature));
    assert!(ready(&c, s), "own form allows a changed idle request");
    tell(&mut c, s);
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut c.sketch.fillets, false);
    }
    enter(&ctx, &mut c.sketch.fillets, "New radius (mm):", value);
    click(&ctx, &mut c.sketch.fillets, "Apply radius");
    let r = c
        .sketch
        .fillets
        .take_apply_request()
        .expect("changed widget request");
    assert_eq!(r.feature, feature);
    assert_eq!(r.expected, reading.version);
    (c.sketch, r)
}
pub(crate) fn draft_state(e: &crate::sketch::Editor) -> String {
    format!("{:?}", e.fillets.radius)
}

pub(crate) fn visible_outcome(e: &mut crate::sketch::Editor, outcome: &str) {
    e.fillets.set_outcome(outcome);
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e.fillets, false);
    }
    assert!(painted(&frame(&ctx, &mut e.fillets, false), outcome));
}

#[test]
fn radius_current_text_confirmation_noop_clean_copy_and_add_exclusion() {
    let (_root, path, reading) = fixture(1);
    let s = opened(&path);
    let chosen = selected(&reading, 0);
    let (form, r) = typed_radius(&path, &reading, &s, chosen.feature, "3.12500");
    assert_eq!(r.radius_mm, 3.125);
    let mut c = creates::Creates::default();
    c.sketch = form;
    let e = &mut c.sketch.fillets;
    assert!(e.radius.as_ref().expect("draft").applied.is_none());
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, e, false);
    }
    click(&ctx, e, "Confirm draft number");
    assert!(e.take_apply_request().is_none());
    click(&ctx, e, "Save radius copy…");
    assert_eq!(
        e.take_radius_request().expect("clean copy").radius_mm,
        3.125
    );
    enter(&ctx, e, "New radius (mm):", "2.375000");
    click(&ctx, e, "Apply radius");
    assert!(e.take_apply_request().is_none());
    assert!(painted(&frame(&ctx, e, false), "nothing to apply"));
    // CLI copy confirmation is still meaningful for the same numeric radius.
    click(&ctx, e, "Confirm draft number");
    click(&ctx, e, "Save radius copy…");
    assert_eq!(
        e.take_radius_request().expect("no-op copy").radius_mm,
        2.375
    );
    e.dismiss();
    assert!(e.begin(&path, &reading, chosen.body));
    assert!(!ready(&c, &s), "Add has no session Apply");
    assert!(!painted(
        &frame(&ctx, &mut c.sketch.fillets, false),
        "Apply radius"
    ));
}

#[test]
fn radius_busy_guard_proves_changed_idle_request_and_excludes_other_work() {
    let (_root, path, reading) = fixture(1);
    let mut s = opened(&path);
    let (form, _) = typed_radius(&path, &reading, &s, selected(&reading, 0).feature, "3.125");
    let mut c = creates::Creates::default();
    c.sketch = form;
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut c.sketch.fillets, false);
    }
    for mode in 0..5 {
        let mut loads = Loads::default();
        let mut ex = exports::Exports::default();
        let mut ed = edits::Edits::default();
        let mut generation = None;
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
            _ => {
                let form = std::mem::take(&mut c.sketch);
                assert!(creates::open_form(
                    &mut c,
                    &mut crate::ViewportInput::default()
                ));
                c.sketch = form;
            }
        }
        let available = can_apply_fillet_radius(&c, &loads, &ex, &ed, &s);
        assert!(!available, "busy mode {mode}");
        c.sketch.fillets.set_session(true, available, s.dirty());
        click(&ctx, &mut c.sketch.fillets, "Apply radius");
        assert!(c.sketch.fillets.take_apply_request().is_none());
        if let Some(g) = generation {
            s.cancel();
            s.finish_apply(g, Err(CadError::Cancelled));
        }
        loads.stop_all();
        ex.stop_all();
    }
    c = creates::Creates::default();
    assert!(
        c.sketch
            .begin_edit(&path, &reading, selected(&reading, 0).profile)
    );
    assert!(!ready(&c, &s), "another editor");
    c.sketch.dismiss();
    assert!(
        c.sketch
            .fillets
            .begin_radius(&path, &reading, selected(&reading, 0).feature)
    );
    assert!(!ready(&c, &sessions::Sessions::default()), "no session");
}

#[test]
fn radius_dirty_discovery_refusal_retains_text_and_acceptance_closes_form() {
    let (_root, path, reading) = fixture(1);
    let original = std::fs::read(&path).expect("source");
    let mut s = opened(&path);
    let chosen = selected(&reading, 0);
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
    let reading = ferritecad_jobs::read_extrude_source(&current).expect("reading");
    let mut c = creates::Creates::default();
    tell(&mut c, &s);
    let ctx = egui::Context::default();
    let choice_frame = |c: &mut creates::Creates, events| {
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
                    .fillets
                    .choices(ui, false, Some(&current), Some(&reading))
            },
        );
        output.textures_delta.clear();
        output
    };
    let mut out = choice_frame(&mut c, Vec::new());
    for _ in 0..3 {
        out = choice_frame(&mut c, Vec::new());
    }
    let at = find(
        &out,
        &format!("Edit Fillet radius same — {}", chosen.feature),
    )
    .expect("dirty Edit");
    for pressed in [true, false] {
        choice_frame(
            &mut c,
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
    assert!(c.sketch.fillets.editing_radius());
    tell(&mut c, &s);
    let e = &mut c.sketch.fillets;
    for _ in 0..3 {
        frame(&ctx, e, false);
    }
    for text in ["banana", "0", "6.2"] {
        enter(&ctx, e, "New radius (mm):", text);
        click(&ctx, e, "Apply radius");
        assert!(e.take_apply_request().is_none());
        assert_eq!(e.radius.as_ref().expect("draft").typed, text);
    }
    enter(&ctx, e, "New radius (mm):", "3.125");
    click(&ctx, e, "Confirm draft number");
    click(&ctx, e, "Save radius copy…");
    assert!(e.take_radius_request().is_none());
    assert!(painted(&frame(&ctx, e, false), "unsaved changes"));
    e.set_outcome("Could not apply the change: shared Line UUID bound");
    assert!(painted(&frame(&ctx, e, false), "shared Line UUID bound"));
    assert_eq!(e.radius.as_ref().expect("draft").typed, "3.125");
    assert_eq!(std::fs::read(&path).expect("source"), original);
    c.sketch.finish_session_change();
    assert!(!c.sketch.active());
    assert_eq!(c.sketch.fillets.session(), (true, true, true));
}
