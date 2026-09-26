// SPDX-License-Identifier: MIT
//! Gesture tests feed actual egui pointer/key events without a window or GPU.
#![allow(clippy::panic)]
use super::{
    tests::{click, frame, replace_field, text_at},
    *,
};

fn setup() -> (egui::Context, Editor) {
    let ctx = egui::Context::default();
    let mut e = Editor::default();
    e.begin();
    e.draft.as_mut().expect("draft").points = [
        ["0", "0"],
        ["60", "0"],
        ["60", "20"],
        ["20", "20"],
        ["20", "40"],
        ["0", "40"],
    ]
    .map(|p| p.map(str::to_owned))
    .to_vec();
    e.draft.as_mut().expect("draft").closed = true;
    frame(&ctx, &mut e, vec![]);
    frame(&ctx, &mut e, vec![]);
    (ctx, e)
}
fn canvas(output: &egui::FullOutput) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Rect(r)
                if (r.rect.width() - 510.).abs() < 1. && (r.rect.height() - 250.).abs() < 1. =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .expect("painted canvas")
}
fn press(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
    frame(ctx, e, vec![egui::Event::PointerMoved(at)]);
    button(ctx, e, at, true);
}
fn button(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2, pressed: bool) {
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
fn move_to(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) -> egui::FullOutput {
    frame(ctx, e, vec![egui::Event::PointerMoved(at)])
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}
fn vertex(ctx: &egui::Context, e: &mut Editor, i: usize) -> egui::Pos2 {
    let rect = canvas(&frame(ctx, e, vec![]));
    e.canvas.screen(
        rect,
        Canvas::points(e.draft.as_ref().expect("draft")).expect("numbers")[i],
    )
}
fn choose(ctx: &egui::Context, e: &mut Editor, label: &str) {
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, label));
}

#[test]
fn pointer_drag_is_one_precise_undo_step_and_selects_only_its_vertex() {
    let (ctx, mut e) = setup();
    // Strings and coordinates on every untouched axis must survive pixel input.
    e.draft.as_mut().expect("draft").points[0][0] = "0.0000000000000001".into();
    e.draft.as_mut().expect("draft").points[1][1] = "0.0000".into();
    let before = e.draft.clone();
    let at = vertex(&ctx, &mut e, 1) + egui::vec2(3., -2.); // retain grab offset, no jump
    click(&ctx, &mut e, at);
    assert_eq!(e.canvas.selected, Some(1));
    assert_eq!(e.draft, before, "click does not round or edit");
    assert!(e.undo.is_empty());
    press(&ctx, &mut e, at);
    for dx in [8., 24., 56., 80.] {
        move_to(&ctx, &mut e, at + egui::vec2(dx, 0.));
        assert!(
            e.undo.is_empty(),
            "preview must not create per-frame history"
        );
    }
    let preview = e.draft.clone();
    let p = &preview.as_ref().expect("draft").points;
    assert_eq!(p[1], ["80".to_owned(), "0.0000".to_owned()]);
    for i in [0, 2, 3, 4, 5] {
        assert_eq!(p[i], before.as_ref().expect("draft").points[i]);
    }
    button(&ctx, &mut e, at + egui::vec2(80., 0.), false);
    assert_eq!(e.draft, preview);
    assert_eq!(e.undo.len(), 1);
    assert!(e.gesture_finished());
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Undo draft"));
    assert_eq!(e.draft, before);
    assert_eq!(e.redo.len(), 1);
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Redo draft"));
    assert_eq!(e.draft, preview);
    // A second gesture on another vertex is a second checkpoint.
    let at = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(80., 0.));
    button(&ctx, &mut e, at + egui::vec2(80., 0.), false);
    assert_eq!(e.undo.len(), 2);
    assert_eq!(e.draft.as_ref().expect("draft").points[2][0], "80");
    replace_field(&ctx, &mut e, "80", "81.125");
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "81.125");
    let history = e.undo.len();
    let exact = e.draft.clone();
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Fit drawing"));
    assert_eq!(e.draft, exact);
    assert_eq!(e.undo.len(), history);
    let at = vertex(&ctx, &mut e, 1);
    click(&ctx, &mut e, at + egui::vec2(7., 0.));
    assert_eq!(e.canvas.selected, Some(1));
    assert_eq!(e.draft, exact);
    assert_eq!(e.undo.len(), history);
    let out = frame(&ctx, &mut e, vec![]);
    click(&ctx, &mut e, text_at(&out, "Reset view"));
    assert_eq!(e.draft, exact);
    assert_eq!(e.undo.len(), history);
}

// No production test-only query API; inspect the draft and the two bounded states.
impl Editor {
    fn gesture_finished(&self) -> bool {
        self.canvas.gesture.is_none() && !self.canvas.claimed_press
    }
}

#[test]
fn pointer_escape_focus_and_outside_release_have_explicit_outcomes() {
    for event in [
        key(egui::Key::Escape),
        egui::Event::WindowFocused(false),
        egui::Event::PointerGone,
    ] {
        let (ctx, mut e) = setup();
        let before = e.draft.clone();
        let at = vertex(&ctx, &mut e, 1);
        press(&ctx, &mut e, at);
        move_to(&ctx, &mut e, at + egui::vec2(24., 5.));
        assert_ne!(e.draft, before);
        frame(&ctx, &mut e, vec![event]);
        assert_eq!(e.draft, before);
        assert!(e.undo.is_empty());
        button(&ctx, &mut e, at + egui::vec2(24., 5.), false);
        assert_eq!(e.draft, before);
        assert!(e.undo.is_empty());
        assert!(e.gesture_finished());
        // A fresh press after cancel really edits, with one new history entry.
        press(&ctx, &mut e, at);
        move_to(&ctx, &mut e, at + egui::vec2(40., 0.));
        button(&ctx, &mut e, at + egui::vec2(40., 0.), false);
        assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "70");
        assert_eq!(e.undo.len(), 1);
    }
    let (ctx, mut e) = setup();
    let before = e.draft.clone();
    let at = vertex(&ctx, &mut e, 1);
    let rect = canvas(&frame(&ctx, &mut e, vec![]));
    let outside = egui::pos2(rect.right() + 24., at.y);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, outside);
    button(&ctx, &mut e, outside, false);
    assert_eq!(
        e.draft.as_ref().expect("draft").points[1][0],
        (60. + f64::from(outside.x - at.x) / 4.).to_string()
    );
    assert_eq!(e.undo.len(), 1);
    assert!(e.gesture_finished());
    e.undo();
    assert_eq!(e.draft, before);
    // Moving away and back is a no-op, including exact strings and redo.
    let redo = e.redo.clone();
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(40., 0.));
    move_to(&ctx, &mut e, at);
    button(&ctx, &mut e, at, false);
    assert_eq!(e.draft, before);
    assert!(e.undo.is_empty());
    assert_eq!(e.redo, redo);
}

#[test]
fn pointer_hit_ties_free_add_and_invalid_preview_use_existing_draft_policy() {
    let (ctx, mut e) = setup();
    e.draft.as_mut().expect("draft").closed = false;
    // Two nearby screen-space candidates: nearest, then stored order on a tie.
    e.draft.as_mut().expect("draft").points[2] = ["62".into(), "0".into()];
    let at = vertex(&ctx, &mut e, 1);
    let before = e.draft.clone();
    click(&ctx, &mut e, at + egui::vec2(4., 0.));
    assert_eq!(e.canvas.selected, Some(1));
    click(&ctx, &mut e, at + egui::vec2(5., 0.));
    assert_eq!(e.canvas.selected, Some(2));
    assert_eq!(e.draft, before);
    assert!(e.undo.is_empty());
    let rect = canvas(&frame(&ctx, &mut e, vec![]));
    let free = e.canvas.screen(rect, [90., 30.]);
    click(&ctx, &mut e, free);
    assert_eq!(e.draft.as_ref().expect("draft").points.len(), 7);
    assert_eq!(
        e.draft.as_ref().expect("draft").points[6],
        ["90.000".to_owned(), "30.000".to_owned()]
    );
    assert_eq!(e.undo.len(), 1);
    let (ctx, mut e) = setup();
    let at = vertex(&ctx, &mut e, 1);
    let other = vertex(&ctx, &mut e, 0);
    press(&ctx, &mut e, at);
    let output = move_to(&ctx, &mut e, other);
    assert!(e.content().is_err());
    assert!(output.shapes.iter().any(
        |c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text().contains("repeated vertices"))
    ));
    button(&ctx, &mut e, other, false);
    assert!(e.content().is_err());
    assert_eq!(e.undo.len(), 1);
    assert!(e.pending.is_none());
    let out = frame(&ctx, &mut e, vec![]);
    assert!(!out.shapes.iter().any(
        |c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text()=="Create in new file…")
    ));
    e.undo();
    assert!(e.content().is_ok());
}

#[test]
fn snap_change_after_a_coalesced_drag_does_not_rewrite_that_gesture() {
    let (ctx, mut e) = setup();
    choose(&ctx, &mut e, "1 mm");
    let at = vertex(&ctx, &mut e, 1);
    let target = at + egui::vec2(24., 0.); // 60 -> 66 mm
    let ten = text_at(&frame(&ctx, &mut e, vec![]), "10 mm");
    let mut events = Vec::new();
    for (press, release) in [(at, target), (ten, ten)] {
        events.extend([
            egui::Event::PointerMoved(press),
            egui::Event::PointerButton {
                pos: press,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerMoved(release),
            egui::Event::PointerButton {
                pos: release,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]);
    }
    frame(&ctx, &mut e, events);
    assert_eq!(
        e.canvas.snap,
        Snap::Ten,
        "the later control click was accepted"
    );
    assert_eq!(
        e.draft.as_ref().expect("draft").points[1][0],
        "66",
        "a later Snap change cannot rewrite an earlier gesture"
    );
    assert_eq!(e.undo.len(), 1);
}

#[test]
fn snap_rounds_values_below_half_a_step_toward_the_nearer_grid_point() {
    let below_half = f64::from_bits(0.05_f64.to_bits() - 1);
    assert_eq!(Snap::Tenth.apply(below_half), "0");
    assert_eq!(Snap::Tenth.apply(-below_half), "0");
    assert_eq!(Snap::Tenth.apply(0.05), "0.1");
    assert_eq!(Snap::Tenth.apply(-0.05), "-0.1");
}

#[test]
fn snap_large_finite_displacements_remain_refused_without_overflow() {
    for (snap, _) in Snap::CHOICES {
        for value in [
            f64::from(f32::MAX),
            -f64::from(f32::MAX),
            f64::MAX,
            -f64::MAX,
        ] {
            let coordinate = snap.apply(value).parse::<f64>().expect("number");
            assert!(
                PolygonExtrusion::new(
                    vec![[coordinate, 0.], [60., 0.], [60., 40.], [0., 40.]],
                    10.
                )
                .is_err(),
                "{snap:?} must retain the shared range refusal"
            );
        }
    }
}

#[test]
fn snap_preserves_invalid_coordinate_refusals_without_panicking() {
    for (snap, _) in Snap::CHOICES {
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            -f64::MAX,
        ] {
            let mut e = Editor::default();
            e.begin();
            let d = e.draft.as_mut().expect("draft");
            d.closed = true;
            // Replacing NaN with zero would make this rectangle valid.
            d.points = [["0", "0"], ["60", "0"], ["60", "40"], ["0", "40"]]
                .map(|p| p.map(str::to_owned))
                .to_vec();
            d.points[0][0] = snap.apply(value);
            assert!(
                e.content().is_err(),
                "{snap:?} must not make {value} publishable"
            );
        }
    }
}

#[test]
fn snap_step_rounds_pointer_input_without_changing_fields_or_off_precision() {
    assert_eq!(Snap::Off.apply(80.07456568667763), "80.07456568667763");
    assert_eq!(Snap::One.apply(80.07456568667763), "80");
    assert_eq!(Snap::Tenth.apply(0.1 + 0.2), "0.3");
    assert_eq!(Snap::Tenth.apply(0.15), "0.2");
    assert_eq!(Snap::Tenth.apply(-0.15), "-0.2");
    assert_eq!(Snap::Tenth.apply(0.05), "0.1");
    assert_eq!(Snap::Tenth.apply(-0.05), "-0.1");
    assert_eq!(Snap::Five.apply(-7.5), "-10");
    assert_eq!(Snap::Ten.apply(15.), "20");

    let (ctx, mut e) = setup();
    assert_eq!(e.canvas.snap, Snap::Off);
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        out.shapes
            .iter()
            .any(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text() == "Off"))
    );
    assert!(out.shapes.iter().any(
        |c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text().contains("snap: Off"))
    ));
    let at = vertex(&ctx, &mut e, 1);
    let slop = at + egui::vec2(81.4, 0.);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, slop);
    button(&ctx, &mut e, slop, false);
    let off = e.draft.as_ref().expect("draft").points[1][0].clone();
    assert_ne!(off, "80");
    assert!(off.starts_with("80."), "{off}");
    assert_eq!(e.undo.len(), 1);
    e.undo();
    let before = e.draft.clone();
    choose(&ctx, &mut e, "1 mm");
    assert_eq!(e.canvas.snap, Snap::One);
    assert_eq!(e.draft, before);
    assert!(e.undo.is_empty(), "toggling Snap is not an undo step");
    choose(&ctx, &mut e, "Fit drawing");
    assert_eq!(e.canvas.snap, Snap::One);
    assert_eq!(e.draft, before);

    e.draft.as_mut().expect("draft").points[1][1] = "0.37".into();
    let before = e.draft.clone();
    let at = vertex(&ctx, &mut e, 1) + egui::vec2(3., -2.);
    click(&ctx, &mut e, at);
    assert_eq!(e.canvas.selected, Some(1));
    assert_eq!(
        e.draft, before,
        "press without movement keeps off-grid strings"
    );
    let dx = 20. * e.canvas.scale + 1.25;
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(dx, 0.));
    assert_eq!(
        e.draft.as_ref().expect("draft").points[1],
        ["80".to_owned(), "0.37".to_owned()],
        "zero-delta axis keeps its off-grid string"
    );
    button(&ctx, &mut e, at + egui::vec2(dx, 0.), false);
    assert_eq!(e.undo.len(), 1);
    let at = vertex(&ctx, &mut e, 1);
    let six = 6. * e.canvas.scale;
    press(&ctx, &mut e, at);
    e.canvas.snap = Snap::Ten;
    move_to(&ctx, &mut e, at + egui::vec2(six, 0.));
    assert_eq!(
        e.draft.as_ref().expect("draft").points[1][0],
        "86",
        "the step is the one frozen at press, not the later control value"
    );
    frame(&ctx, &mut e, vec![key(egui::Key::Escape)]);
    button(&ctx, &mut e, at, false);
    e.canvas.snap = Snap::One;
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
    assert_eq!(e.undo.len(), 1);
    assert!(e.gesture_finished());
    for i in [0, 2, 3, 4, 5] {
        assert_eq!(
            e.draft.as_ref().expect("draft").points[i],
            before.as_ref().expect("draft").points[i]
        );
    }
    let kept = e.draft.clone();
    let at = vertex(&ctx, &mut e, 1);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(dx, 0.));
    move_to(&ctx, &mut e, at);
    button(&ctx, &mut e, at, false);
    assert_eq!(e.draft, kept);
    assert_eq!(e.undo.len(), 1, "return to press is a no-op");
    choose(&ctx, &mut e, "Undo draft");
    assert_eq!(e.draft, before);
    choose(&ctx, &mut e, "Redo draft");
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
    replace_field(&ctx, &mut e, "80", "80.25");
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80.25");

    let (ctx, mut e) = setup();
    e.draft.as_mut().expect("draft").closed = false;
    choose(&ctx, &mut e, "0.1 mm");
    let rect = canvas(&frame(&ctx, &mut e, vec![]));
    let added = e.canvas.screen(rect, [90.34, 30.15]);
    click(&ctx, &mut e, added);
    assert_eq!(
        e.draft.as_ref().expect("draft").points.last(),
        Some(&["90.3".to_owned(), "30.2".to_owned()])
    );
    choose(&ctx, &mut e, "5 mm");
    let at = vertex(&ctx, &mut e, 0);
    let left = -7.5 * e.canvas.scale;
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(left, 0.));
    button(&ctx, &mut e, at + egui::vec2(left, 0.), false);
    assert_eq!(e.draft.as_ref().expect("draft").points[0][0], "-10");

    let (ctx, mut e) = setup();
    choose(&ctx, &mut e, "1 mm");
    let at = vertex(&ctx, &mut e, 1);
    let before = e.draft.clone();
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(40., 0.));
    frame(&ctx, &mut e, vec![key(egui::Key::Escape)]);
    button(&ctx, &mut e, at + egui::vec2(40., 0.), false);
    assert_eq!(e.draft, before);
    let other = vertex(&ctx, &mut e, 0);
    press(&ctx, &mut e, at);
    let output = move_to(&ctx, &mut e, other);
    assert!(e.content().is_err());
    assert!(output.shapes.iter().any(
        |c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text().contains("repeated vertices"))
    ));
    button(&ctx, &mut e, other, false);
    assert!(e.content().is_err());
    assert!(e.take_request().is_none());
    e.undo();
    assert!(e.content().is_ok());
    e.dismiss();
    e.begin();
    assert_eq!(
        e.canvas.snap,
        Snap::Off,
        "a new editor starts with Snap Off"
    );
}

pub(super) fn move_saved_l(ctx: &egui::Context, e: &mut Editor) {
    let original = e.edit_request().expect("original");
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, "1 mm"));
    assert_eq!(e.canvas.snap, Snap::One);
    let out = frame(ctx, e, vec![]);
    click(ctx, e, text_at(&out, "Fit drawing"));
    assert_eq!(
        e.canvas.snap,
        Snap::One,
        "Fit does not change the document step"
    );
    for i in [1, 2] {
        let before = e.draft.clone();
        let history = e.undo.len();
        let at = vertex(ctx, e, i);
        // Slightly inaccurate pixels; snap, not numeric fields, yields 80 mm.
        let dx = 20. * e.canvas.scale + 1.25;
        press(ctx, e, at);
        for fraction in [0.25, 0.5, 0.75, 1.] {
            move_to(ctx, e, at + egui::vec2(dx * fraction, 0.));
        }
        button(ctx, e, at + egui::vec2(dx, 0.), false);
        assert_eq!(e.draft.as_ref().expect("draft").points[i][0], "80");
        assert_eq!(e.undo.len(), history + 1);
        for j in 0..6 {
            if j != i {
                assert_eq!(
                    e.draft.as_ref().expect("draft").points[j],
                    before.as_ref().expect("draft").points[j]
                );
            }
        }
    }
    let modified = e.edit_request().expect("edited");
    assert_eq!(
        original
            .vertices
            .iter()
            .map(|v| v.curve_id)
            .collect::<Vec<_>>(),
        modified
            .vertices
            .iter()
            .map(|v| v.curve_id)
            .collect::<Vec<_>>()
    );
    assert_eq!(original.expected, modified.expected);
    let at = vertex(ctx, e, 1);
    let zero = vertex(ctx, e, 0);
    let before = e.draft.clone();
    let history = e.undo.len();
    press(ctx, e, at);
    move_to(ctx, e, zero);
    assert!(
        e.edit_request().is_err(),
        "saved preview uses common policy"
    );
    let out = frame(ctx, e, vec![]);
    assert!(
        !out.shapes.iter().any(
            |c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text()=="Save edited copy…")
        )
    );
    frame(ctx, e, vec![key(egui::Key::Escape)]);
    button(ctx, e, zero, false);
    assert_eq!(e.draft, before);
    assert_eq!(e.undo.len(), history);
    assert!(e.edit_request().is_ok());
}

pub(crate) fn attach_source_claim(source: &std::path::Path) {
    let db = rusqlite::Connection::open(source).expect("SQL");
    let bytes = b"attached Sketch source must remain owned after pointer editing";
    db.execute("INSERT INTO imported_sources(id,format,bytes,content_hash,byte_len,created_at) VALUES(zeroblob(16),'future.sketch-source',?1,?2,?3,'saved')",
        rusqlite::params![bytes.as_slice(),ferritecad_types::ContentHash::of_bytes(bytes).as_bytes().as_slice(),bytes.len()]).expect("source bytes");
    db.execute_batch("INSERT INTO imported_source_refs SELECT id,zeroblob(16) FROM objects WHERE kind='sketch'; CREATE TABLE extension(value BLOB); INSERT INTO extension(rowid,value) VALUES(73,X'00FF'); UPDATE capabilities SET rowid=rowid+100; INSERT INTO capabilities(rowid,name,required) VALUES(999,'future.optional',0);").expect("claims and other SQL");
}
pub(crate) fn sql_facts(
    path: &std::path::Path,
) -> std::collections::BTreeMap<String, Vec<Vec<rusqlite::types::Value>>> {
    use rusqlite::{Connection, OpenFlags};
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("SQL");
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .expect("tables")
        .query_map([], |r| r.get(0))
        .expect("rows")
        .collect::<rusqlite::Result<_>>()
        .expect("names");
    names
        .into_iter()
        .map(|name| {
            let quoted = name.replace('"', "\"\"");
            let mut query = db
                .prepare(&format!("SELECT rowid,* FROM \"{quoted}\" ORDER BY rowid"))
                .or_else(|_| db.prepare(&format!("SELECT * FROM \"{quoted}\"")))
                .expect("table");
            let n = query.column_count();
            let rows = query
                .query_map([], |r| {
                    (0..n)
                        .map(|i| r.get(i))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .expect("rows")
                .collect::<rusqlite::Result<_>>()
                .expect("cells");
            (name, rows)
        })
        .collect()
}
pub(super) fn verify_publications(
    source: &std::path::Path,
    ui: &std::path::Path,
    cli: &std::path::Path,
) {
    use crate::creates::tests::ferritecad;
    use ferritecad_document::Document;
    use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
    let a = sql_facts(ui);
    assert_eq!(a, sql_facts(cli), "all SQL cells, including rowids");
    for (table, rows) in sql_facts(source) {
        if table != "objects" {
            assert_eq!(a[&table], rows, "preserve {table}");
        }
    }
    let d = Document::open_read_only(ui).expect("UI copy");
    let original = Document::open_read_only(source).expect("source");
    let choices = ExtrudeEditSource::read(&d).expect("catalog");
    assert_eq!(
        choices.sketches[0]
            .vertices
            .as_ref()
            .expect("vertices")
            .iter()
            .map(|v| v.start_mm)
            .collect::<Vec<_>>(),
        [
            [0., 0.],
            [80., 0.],
            [80., 20.],
            [20., 20.],
            [20., 40.],
            [0., 40.]
        ]
    );
    let selected = choices.sketches[0].sketch;
    for old in original.objects().expect("objects") {
        let new = d.object(old.id).expect("object").expect("same ID");
        if old.id != selected {
            assert_eq!(old, new);
        } else {
            assert_eq!(
                (old.id, old.name, old.parent, old.ordinal),
                (new.id, new.name, new.parent, new.ordinal)
            );
        }
    }
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&d, &mut kernel, &context).expect("cold rebuild");
    let refs = d.topology_refs().expect("refs");
    assert_eq!(refs.len(), 3);
    for r in refs {
        let faces = built.resolve(&r).expect("preserved ref");
        assert_eq!(faces.len(), 1);
        let face = faces[0];
        let mesh = kernel
            .tessellate(face.shape(), &TessellationParams::default(), &context)
            .expect("face mesh");
        let range = mesh
            .faces
            .iter()
            .find(|m| m.face == face)
            .expect("actual face");
        let points: Vec<_> = mesh.indices
            [range.first_index as usize..(range.first_index + range.index_count) as usize]
            .iter()
            .map(|&i| &mesh.positions[3 * i as usize..3 * i as usize + 3])
            .collect();
        use ferritecad_document::{CapSide, SemanticRole};
        match r.output_role {
            SemanticRole::ExtrudeCap {
                side: CapSide::Start,
            } => assert!(points.iter().all(|p| p[2] == 0.)),
            SemanticRole::ExtrudeCap { side: CapSide::End } => {
                assert!(points.iter().all(|p| p[2] == 10.))
            }
            SemanticRole::ExtrudeSide { profile_segment } => {
                assert_eq!(
                    profile_segment,
                    choices.sketches[0].vertices.as_ref().expect("vertices")[0].curve_id
                );
                assert!(points.iter().all(|p| p[1] == 0.));
                assert!(points.iter().any(|p| p[0] == 80.));
            }
            _ => panic!("unexpected reference"),
        }
    }
    built.release_all(&mut kernel);
    d.close().expect("close");
    original.close().expect("close");
    let mut artifacts = Vec::new();
    for path in [ui, cli] {
        for command in ["validate", "rebuild"] {
            let mut cmd = std::process::Command::new(ferritecad());
            cmd.arg(command).arg(path);
            if command == "rebuild" {
                cmd.arg("--cold");
            }
            let output = cmd.output().expect("process");
            assert!(output.status.success(), "{output:?}");
        }
        let mut files = Vec::new();
        for (op, suffix) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
            let out = path.with_extension(suffix);
            let p = std::process::Command::new(ferritecad())
                .arg(op)
                .arg(path)
                .arg("-o")
                .arg(&out)
                .arg("--json")
                .output()
                .expect("export");
            assert!(p.status.success(), "{p:?}");
            files.push(std::fs::read(&out).expect("artifact"));
        }
        artifacts.push(files);
    }
    assert_eq!(artifacts[0], artifacts[1]);
    let bytes = &artifacts[0][0];
    let triangles = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + triangles * 50);
    assert_eq!(triangles, 20);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut volume6 = 0.;
    for i in 0..triangles {
        let v: [f64; 9] = std::array::from_fn(|j| {
            let at = 84 + 50 * i + 12 + 4 * j;
            f64::from(f32::from_le_bytes(
                bytes[at..at + 4].try_into().expect("f32"),
            ))
        });
        for p in v.chunks_exact(3) {
            for j in 0..3 {
                lo[j] = lo[j].min(p[j]);
                hi[j] = hi[j].max(p[j]);
            }
        }
        volume6 += v[0] * (v[4] * v[8] - v[5] * v[7])
            + v[1] * (v[5] * v[6] - v[3] * v[8])
            + v[2] * (v[3] * v[7] - v[4] * v[6]);
    }
    assert_eq!(lo, [0., 0., 0.]);
    assert_eq!(hi, [80., 40., 10.]);
    // Same 1 ppm tolerance as §25B; planar integer vertices are exact in f32.
    assert!((volume6.abs() / 6. - 20000.).abs() < 0.02);
    if let Some(dir) = std::env::var_os("FCAD_SKETCH_DRAG_ARTIFACT_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifact directory");
        for (from, name) in [
            (ui.to_path_buf(), "drag.fcad"),
            (ui.with_extension("stl"), "drag.stl"),
            (ui.with_extension("fbx"), "drag.fbx"),
        ] {
            std::fs::copy(from, dir.join(name)).expect("retain artifacts for independent reader");
        }
    }
}

#[test]
fn native_dragged_sketch_worker_and_cli_preserve_identity_and_geometry() {
    super::tests::saved_sketch_and_cli(true);
}

#[test]
fn coalesced_pointer_events_keep_press_target_and_history_stays_bounded() {
    let (ctx, mut e) = setup();
    let before = e.draft.clone().expect("draft");
    e.undo = vec![before.clone(); 128];
    let at = vertex(&ctx, &mut e, 1);
    let to = at + egui::vec2(20., -20.);
    frame(
        &ctx,
        &mut e,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerMoved(to),
        ],
    );
    button(&ctx, &mut e, to, false);
    assert_eq!(
        e.draft.as_ref().expect("draft").points[1],
        ["65".to_owned(), "5".to_owned()]
    );
    for i in [0, 2, 3, 4, 5] {
        assert_eq!(e.draft.as_ref().expect("draft").points[i], before.points[i]);
    }
    assert_eq!(e.undo.len(), 128);
    e.undo();
    assert_eq!(e.draft, Some(before));
    let mut idle = frame(&ctx, &mut e, vec![]);
    for _ in 0..30 {
        idle = frame(&ctx, &mut e, vec![]);
    }
    assert!(
        idle.viewport_output[&egui::ViewportId::ROOT].repaint_delay > std::time::Duration::ZERO,
        "settled canvas must not request a perpetual idle repaint"
    );
}

#[test]
fn saved_winding_and_crossing_previews_refuse_through_shared_policy() {
    let (ctx, mut e) = setup();
    e.draft.as_mut().expect("draft").points = [["0", "0"], ["60", "0"], ["0", "40"]]
        .map(|p| p.map(str::to_owned))
        .to_vec();
    let vertices: Vec<_> = Canvas::points(e.draft.as_ref().expect("draft"))
        .expect("points")
        .into_iter()
        .map(|start_mm| SketchVertex {
            curve_id: ferritecad_types::StableEntityId::new(),
            start_mm,
        })
        .collect();
    let id = ferritecad_types::ObjectId::new();
    let choice = SketchChoice {
        cut_history: None,
        sketch: id,
        name: None,
        vertices: Some(vertices.clone()),
        profile_use: Some(ferritecad_document::SketchProfileUse::BlindExtrude {
            feature: ferritecad_types::ObjectId::new(),
            height_mm: 10.,
        }),
        refusal: None,
    };
    e.editing = Some((
        EditSketchRequest {
            source: PathBuf::from("accepted.fcad"),
            destination: PathBuf::new(),
            sketch: id,
            vertices,
            expected: ferritecad_document::DocumentVersion {
                document_id: ferritecad_types::DocumentId::new(),
                content: ferritecad_types::ContentHash::of_bytes(b"accepted snapshot"),
            },
        },
        choice,
    ));
    frame(&ctx, &mut e, vec![]);
    frame(&ctx, &mut e, vec![]); // changed editor window identity: finish its first layout
    let at = vertex(&ctx, &mut e, 2);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, at + egui::vec2(0., 320.));
    let error = e
        .edit_request()
        .expect_err("winding must not change")
        .to_string();
    assert!(error.contains("winding"));
    let out = frame(&ctx, &mut e, vec![]);
    assert!(
        out.shapes
            .iter()
            .any(|c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text()==error))
    );
    button(&ctx, &mut e, at + egui::vec2(0., 320.), false);
    assert_eq!(e.undo.len(), 1);
    assert!(e.edit_request().is_err());
    e.undo();
    assert!(e.edit_request().is_ok());
    let (ctx, mut e) = setup();
    let at = vertex(&ctx, &mut e, 1);
    press(&ctx, &mut e, at);
    let out = move_to(&ctx, &mut e, at + egui::vec2(-200., -120.));
    let error = e.content().expect_err("crossing preview").to_string();
    assert!(error.contains("intersect"));
    assert!(
        out.shapes
            .iter()
            .any(|c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text()==error))
    );
}

#[test]
fn table_scroll_and_numeric_escape_do_not_start_canvas_gestures() {
    let (ctx, mut e) = setup();
    e.draft.as_mut().expect("draft").points = (0..20)
        .map(|i| [(i * 10).to_string(), (i * 2).to_string()])
        .collect();
    let before = e.draft.clone();
    let out = frame(&ctx, &mut e, vec![]);
    let at = text_at(&out, "2  X mm");
    let visible = |out: &egui::FullOutput, label: &str| {
        out.shapes.iter().any(|c| matches!(&c.shape,egui::Shape::Text(t) if t.galley.text()==label && c.clip_rect.contains_rect(t.visual_bounding_rect())))
    };
    assert!(visible(&out, "1  X mm"));
    frame(
        &ctx,
        &mut e,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0., -140.),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            },
        ],
    );
    let mut out = frame(&ctx, &mut e, vec![]);
    for _ in 0..12 {
        out = frame(&ctx, &mut e, vec![]);
    }
    assert!(!visible(&out, "1  X mm"), "table scrolled");
    assert_eq!(e.draft, before);
    assert!(e.undo.is_empty());
    assert!(e.gesture_finished());
    let (ctx, mut e) = setup();
    replace_field(&ctx, &mut e, "60", "61.125");
    let before = e.draft.clone();
    let history = e.undo.len();
    frame(&ctx, &mut e, vec![key(egui::Key::Escape)]);
    assert_eq!(e.draft, before);
    assert_eq!(e.undo.len(), history);
    assert!(e.gesture_finished());
    assert!(e.canvas.selected.is_none());
}

#[test]
fn single_frame_drag_uses_press_and_release_even_after_later_hover() {
    for outside in [false, true] {
        let (ctx, mut e) = setup();
        let before = e.draft.clone().expect("draft");
        let at = vertex(&ctx, &mut e, 1);
        let dx = if outside { 400. } else { 80. };
        let to = at + egui::vec2(dx, 0.);
        frame(&ctx, &mut e, vec![egui::Event::PointerMoved(at)]);
        frame(
            &ctx,
            &mut e,
            vec![
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerMoved(to),
                egui::Event::PointerButton {
                    pos: to,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
                egui::Event::PointerMoved(at + egui::vec2(0., 30.)),
            ],
        );
        assert_eq!(
            e.draft.as_ref().expect("draft").points[1],
            [(60. + f64::from(dx) / 4.).to_string(), "0".into()]
        );
        for i in [0, 2, 3, 4, 5] {
            assert_eq!(e.draft.as_ref().expect("draft").points[i], before.points[i]);
        }
        assert_eq!(e.canvas.selected, Some(1));
        assert_eq!(e.undo.len(), 1);
        assert!(e.gesture_finished());
        e.undo();
        assert_eq!(e.draft, Some(before));
    }
}

#[test]
fn release_then_next_press_in_one_frame_keeps_the_next_drag_active() {
    let (ctx, mut e) = setup();
    let first = vertex(&ctx, &mut e, 1);
    let second = vertex(&ctx, &mut e, 2);
    let event = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    press(&ctx, &mut e, first);
    frame(
        &ctx,
        &mut e,
        vec![
            egui::Event::PointerMoved(first + egui::vec2(80., 0.)),
            event(first + egui::vec2(80., 0.), false),
            egui::Event::PointerMoved(second),
            event(second, true),
        ],
    );
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
    assert!(e.canvas.gesture.is_some(), "next press starts its own drag");
    move_to(&ctx, &mut e, second + egui::vec2(80., 0.));
    button(&ctx, &mut e, second + egui::vec2(80., 0.), false);
    assert_eq!(e.draft.as_ref().expect("draft").points[2][0], "80");
    assert_eq!(e.undo.len(), 2);
    e.undo();
    assert_eq!(e.draft.as_ref().expect("draft").points[2][0], "60");
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
}

#[test]
fn several_gestures_in_one_frame_keep_changed_checkpoints_in_order() {
    let (ctx, mut e) = setup();
    let original = e.draft.clone();
    let at = vertex(&ctx, &mut e, 1);
    let moved = at + egui::vec2(80., 0.);
    let mut events = Vec::new();
    // A no-op, then A -> B -> A. Comparing only the frame's final draft
    // would discard both changed gestures (or record the no-op incorrectly).
    for (press, release) in [(at, at), (at, moved), (moved, at)] {
        events.extend([
            egui::Event::PointerMoved(press),
            egui::Event::PointerButton {
                pos: press,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerMoved(release),
            egui::Event::PointerButton {
                pos: release,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]);
    }
    frame(&ctx, &mut e, events);
    assert_eq!(e.draft, original);
    assert_eq!(e.undo.len(), 2);
    assert!(e.gesture_finished());
    e.undo();
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
    e.undo();
    assert_eq!(e.draft, original);
    assert!(e.undo.is_empty());
    e.redo();
    assert_eq!(e.draft.as_ref().expect("draft").points[1][0], "80");
    e.redo();
    assert_eq!(e.draft, original);
}

#[test]
fn cancellation_before_release_in_one_frame_does_not_publish_a_gesture() {
    for cancel in [
        key(egui::Key::Escape),
        egui::Event::PointerGone,
        egui::Event::WindowFocused(false),
    ] {
        let (ctx, mut e) = setup();
        let original = e.draft.clone();
        let at = vertex(&ctx, &mut e, 1);
        press(&ctx, &mut e, at);
        let moved = at + egui::vec2(80., 0.);
        frame(
            &ctx,
            &mut e,
            vec![
                egui::Event::PointerMoved(moved),
                cancel,
                egui::Event::PointerButton {
                    pos: moved,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
        );
        assert_eq!(e.draft, original);
        assert!(e.undo.is_empty());
        assert!(e.gesture_finished());
    }
}

#[test]
fn completed_pointer_gesture_does_not_edit_through_an_overlapping_window() {
    let (ctx, mut e) = setup();
    let before = e.draft.clone();
    let at = vertex(&ctx, &mut e, 1);
    let to = at + egui::vec2(80., 0.);
    let mut draw = |events| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                e.draw(ui, true, false);
                egui::Area::new(egui::Id::new("cover-canvas"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(at - egui::vec2(20., 20.))
                    .show(ui.ctx(), |ui| {
                        ui.allocate_exact_size(
                            egui::vec2(150., 60.),
                            egui::Sense::click_and_drag(),
                        );
                    });
            },
        )
    };
    let _ = draw(vec![]);
    let _ = draw(vec![]);
    let _ = draw(vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerMoved(to),
        egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        },
    ]);
    assert_eq!(e.draft, before);
    assert_eq!(e.canvas.selected, None);
    assert!(e.undo.is_empty());
}

/// §27C: a solid cylinder drawn on the real canvas and published by the
/// window's worker and the peer CLI alike, then reopened and reshaped into a
/// frustum by one drag, with the saved axis Line held on the axis.
#[test]
fn native_solid_revolve_widgets_drag_worker_and_cli_create_and_edit() {
    use crate::creates::{
        self,
        tests::{ferritecad, read_semantics},
    };
    use ferritecad_document::Document;
    use ferritecad_kernel::OperationContext;
    if !ferritecad_occt::is_available() {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: no OCCT for the solid Revolve worker");
        return;
    }
    let pi = std::f64::consts::PI;
    let root = tempfile::tempdir().expect("directory");
    let ui = root.path().join("cylinder-ui.fcad");
    let cli = root.path().join("cylinder-cli.fcad");
    let mut creates = creates::Creates::default();
    let ctx = egui::Context::default();
    {
        let e = &mut creates.sketch;
        let out = frame(&ctx, e, vec![]);
        click(&ctx, e, text_at(&out, "Create sketch + Extrude…"));
        frame(&ctx, e, vec![]);
        for [x, y] in [[0., 0.], [10., 0.], [10., 15.], [0., 15.]] {
            let rect = canvas(&frame(&ctx, e, vec![]));
            click(
                &ctx,
                e,
                rect.left_bottom() + egui::vec2(35. + 4. * x, -30. - 4. * y),
            );
        }
        choose(&ctx, e, "Close contour");
        choose(&ctx, e, "Revolve 360°");
        let Ok(NewDocument::SketchRevolve(cylinder)) = e.content() else {
            panic!("a solid cylinder is a Revolve")
        };
        assert_eq!(
            cylinder
                .points()
                .iter()
                .map(|p| [p.x, p.y])
                .collect::<Vec<_>>(),
            [[0., 0.], [10., 0.], [10., 15.], [0., 15.]]
        );
        assert!((cylinder.volume_mm3() - 1500. * pi).abs() < 1e-9);
        let out = frame(&ctx, e, vec![]);
        let texts: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|t| t == "axis (Y) at X = 0 · radius X →"));
        assert!(
            texts
                .iter()
                .any(|t| t.contains("put exactly one whole edge on X = 0 for a solid part")),
            "{texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("Keep every point X > 0")),
            "no unconditional X > 0 rule"
        );
        // An isolated touch and a crossing are refused by the document's own
        // policy, with nothing to publish, and Undo gives the cylinder back.
        let valid = e.draft.clone();
        for (to, refusal) in [("4", "touches the axis alone"), ("-1", "cross")] {
            replace_field(&ctx, e, "0.000", to);
            let refused = e.content().expect_err("refused").to_string();
            assert!(refused.contains(refusal), "{to}: {refused}");
            let out = frame(&ctx, e, vec![]);
            assert!(!out.shapes.iter().any(|c| matches!(&c.shape,
                egui::Shape::Text(t) if t.galley.text() == "Create in new file…")));
            click(&ctx, e, text_at(&out, "Undo draft"));
            assert_eq!(e.draft, valid);
        }
    }
    let content = creates.sketch.content().expect("a Revolve");
    let before = creates.sketch.draft.clone();
    let mut view = ferritecad_ui::ViewportInput::new();
    let loads = crate::Loads::default();
    let exports = crate::exports::Exports::default();
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
        .is_none(),
        "a cancelled Save starts nothing"
    );
    assert_eq!(creates.sketch.draft, before);
    let (tx, rx) = std::sync::mpsc::channel();
    let spawn = move |path: &std::path::Path,
                      content,
                      generation,
                      cancel: &ferritecad_kernel::CancelToken| {
        let path = path.to_path_buf();
        let ctx = OperationContext::default().with_cancel(cancel.clone());
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
        content,
        Some(ui.clone()),
        spawn,
    )
    .expect("worker");
    let (generation, result) = rx.recv().expect("worker result");
    assert_eq!(
        creates::finish_create(&mut creates, &mut view, generation, result),
        Some(ui.clone())
    );
    creates.sketch.draft_load_finished(&ui, false);
    assert_eq!(
        creates.sketch.draft, before,
        "a failed Open keeps the draft"
    );
    creates.sketch.draft_published(&ui);
    creates.sketch.draft_load_finished(&ui, true);
    assert!(!creates.sketch.active());
    creates.stop_all();

    let input = root.path().join("cylinder.json");
    std::fs::write(
        &input,
        r#"{"request_version":1,"points_mm":[[0,0],[10,0],[10,15],[0,15]],"axis":"sketch_y","angle":"full_turn"}"#,
    )
    .expect("request");
    let p = std::process::Command::new(ferritecad())
        .arg("create-sketch-revolve")
        .arg(&input)
        .arg("-o")
        .arg(&cli)
        .arg("--json")
        .output()
        .expect("peer CLI");
    assert!(p.status.success(), "{p:?}");
    assert_eq!(read_semantics(&ui).0, read_semantics(&cli).0);
    let export = |path: &std::path::Path| {
        let mut bytes = Vec::new();
        for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
            let out = path.with_extension(extension);
            let p = std::process::Command::new(ferritecad())
                .arg(op)
                .arg(path)
                .arg("-o")
                .arg(&out)
                .output()
                .expect("export");
            assert!(p.status.success(), "{p:?}");
            bytes.push(std::fs::read(&out).expect("bytes"));
        }
        bytes
    };
    let (a, b) = (export(&ui), export(&cli));
    assert_eq!(a[0], b[0], "worker/CLI STL bytes");
    let mapped_fbx = |path: &std::path::Path, bytes: &[u8]| {
        let document = Document::open_read_only(path).expect("document");
        let body = document
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Body(_)))
            .expect("Body");
        let text = std::str::from_utf8(bytes).expect("ASCII FBX");
        assert_eq!(text.matches(&body.id.to_string()).count(), 3);
        let mapped = text.replace(&body.id.to_string(), "same-body");
        document.close().expect("close");
        mapped
    };
    assert_eq!(
        mapped_fbx(&ui, &a[1]),
        mapped_fbx(&cli, &b[1]),
        "all FBX bytes agree after mapping the two created Body identities"
    );

    // Reopen the worker's file and edit it in the same Line editor.
    let source_bytes = std::fs::read(&ui).expect("source");
    let reading = {
        let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
        ferritecad_scene::snapshot_of(
            &ui,
            &mut k,
            |k, b| k.import_step(b),
            &Default::default(),
            &OperationContext::default(),
        )
        .expect("accepted scene")
        .edit_source
        .expect("accepted edit facts")
    };
    let choice = &reading.sketches[0];
    let saved = choice.vertices.clone().expect("editable");
    let Some(SketchProfileUse::FullTurnRevolve {
        axis_segment: Some(axis),
        ..
    }) = choice.profile_use
    else {
        panic!(
            "a solid Revolve names its axis Line: {:?}",
            choice.profile_use
        )
    };
    assert_eq!(axis, saved[3].curve_id, "(0,15)→(0,0) lies on the axis");
    let mut e = Editor::default();
    assert!(e.begin_edit(&ui, &reading, choice.sketch));
    frame(&ctx, &mut e, vec![]);
    let out = frame(&ctx, &mut e, vec![]);
    let wanted = format!(
        "Solid part: edge 4 (Line {axis}) stays on the axis at X = 0; every other point stays at X > 0."
    );
    assert!(
        out.shapes.iter().any(|c| matches!(&c.shape,
            egui::Shape::Text(t) if t.galley.text() == wanted)),
        "the axis Line is named"
    );
    for absent in ["Revolve 360°", "Blind height mm", "Feature:"] {
        assert!(!out.shapes.iter().any(|c| matches!(&c.shape,
            egui::Shape::Text(t) if t.galley.text() == absent)));
    }
    choose(&ctx, &mut e, "1 mm");
    assert_eq!(e.canvas.snap, Snap::One);

    // One drag, one Undo step: the top outer corner in by 5 mm.
    let original = e.draft.clone();
    let at = vertex(&ctx, &mut e, 2);
    let dx = -(5. * e.canvas.scale) - 1.25;
    press(&ctx, &mut e, at);
    for fraction in [0.25, 0.5, 0.75, 1.] {
        move_to(&ctx, &mut e, at + egui::vec2(dx * fraction, 0.));
        assert!(e.undo.is_empty(), "no per-frame history");
    }
    button(&ctx, &mut e, at + egui::vec2(dx, 0.), false);
    assert_eq!(
        e.draft.as_ref().expect("draft").points[2],
        ["5".to_owned(), "15".to_owned()]
    );
    assert_eq!(e.undo.len(), 1);
    let frustum = e.draft.clone();
    choose(&ctx, &mut e, "Undo draft");
    assert_eq!(e.draft, original);
    choose(&ctx, &mut e, "Redo draft");
    assert_eq!(e.draft, frustum);

    // An axis end dragged off the axis is refused in preview; Escape cancels
    // the whole gesture.
    let at = vertex(&ctx, &mut e, 0);
    let off = at + egui::vec2(3. * e.canvas.scale, 0.);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, off);
    let refused = e.edit_request().expect_err("off the axis").to_string();
    assert!(refused.contains("touches the axis alone"), "{refused}");
    let out = frame(&ctx, &mut e, vec![]);
    assert!(!out.shapes.iter().any(|c| matches!(&c.shape,
        egui::Shape::Text(t) if t.galley.text() == "Save edited copy…")));
    frame(&ctx, &mut e, vec![key(egui::Key::Escape)]);
    button(&ctx, &mut e, off, false);
    assert_eq!(e.draft, frustum);
    assert_eq!(e.undo.len(), 1);
    // Both axis ends off the axis would make a bored part: refused as a
    // class change, and undone.
    let before = e.draft.clone().expect("draft");
    for i in [0, 3] {
        e.draft.as_mut().expect("draft").points[i][0] = "1".into();
    }
    e.record(before);
    let refused = e.edit_request().expect_err("hollow").to_string();
    assert!(refused.contains("cannot change between"), "{refused}");
    choose(&ctx, &mut e, "Undo draft");
    assert_eq!(e.draft, frustum);

    choose(&ctx, &mut e, "Save edited copy…");
    let mut request = e.take_edit_request().expect("submit");
    assert_eq!(
        request
            .vertices
            .iter()
            .map(|v| v.curve_id)
            .collect::<Vec<_>>(),
        saved.iter().map(|v| v.curve_id).collect::<Vec<_>>()
    );
    let kept = e.draft.clone();
    let edited = root.path().join("frustum-ui.fcad");
    request.destination = edited.clone();
    let mut edits = crate::edits::Edits::default();
    let (tx, rx) = std::sync::mpsc::channel();
    let generation = edits
        .start_sketch(request.clone(), move |r, g, c| {
            crate::edits::spawn_sketch_edit(r, c, move |result| {
                tx.send((g, result)).expect("reply")
            })
        })
        .expect("worker");
    let (g, result) = rx.recv().expect("completed");
    assert_eq!(g, generation);
    let path = finish_edit(&mut e, &mut edits, g, result).expect("published");
    e.draft_load_finished(&path, false);
    assert_eq!(e.draft, kept, "a failed Open restores the edited draft");
    e.draft_published(&path);
    e.draft_load_finished(&path, true);
    assert!(!e.active());

    let edit = root.path().join("edit.json");
    let vertices: Vec<String> = request
        .vertices
        .iter()
        .map(|v| {
            format!(
                r#"{{"curve_id":"{}","start_mm":[{},{}]}}"#,
                v.curve_id, v.start_mm[0], v.start_mm[1]
            )
        })
        .collect();
    std::fs::write(
        &edit,
        format!(
            r#"{{"request_version":1,"vertices":[{}]}}"#,
            vertices.join(",")
        ),
    )
    .expect("edit request");
    let peer = root.path().join("frustum-cli.fcad");
    let p = std::process::Command::new(ferritecad())
        .arg("edit-sketch-copy")
        .arg(&ui)
        .arg("--sketch")
        .arg(choice.sketch.to_string())
        .arg("--expect-version")
        .arg(reading.version.content.to_string())
        .arg("--request")
        .arg(&edit)
        .arg("-o")
        .arg(&peer)
        .arg("--json")
        .output()
        .expect("peer CLI");
    assert!(p.status.success(), "{p:?}");
    let a = Document::open_read_only(&edited).expect("UI");
    let b = Document::open_read_only(&peer).expect("CLI");
    assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
    let original = Document::open_read_only(&ui).expect("source");
    assert_eq!(
        a.topology_refs().expect("refs"),
        original.topology_refs().expect("refs"),
        "no new names, and still none for the axis Line"
    );
    assert_eq!(
        a.topology_refs().expect("refs"),
        b.topology_refs().expect("refs")
    );
    original.close().expect("close");
    // π·h/3·(R² + R·r + r²) for R = 10, r = 5, h = 15.
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&a, &mut kernel, &context).expect("cold");
    let body = a
        .objects()
        .expect("objects")
        .iter()
        .find(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Body(_)))
        .map(|o| o.id)
        .expect("Body");
    let (faces, volume) = kernel
        .shape_stats(built.shape(body).expect("one solid"))
        .expect("stats");
    assert_eq!(faces, 3, "two discs and one cone, no face for the axis");
    assert!((volume - 875. * pi).abs() < 1e-6 * 875. * pi, "{volume}");
    built.release_all(&mut kernel);
    a.close().expect("close");
    b.close().expect("close");
    assert_eq!(export(&edited), export(&peer), "STL and FBX bytes");
    assert_eq!(std::fs::read(&ui).expect("source"), source_bytes);
}

/// §27F through the real widgets: a saved 220° sector of a solid cylinder is
/// opened from its own Edit Sketch row, shows its angle read-only, is edited
/// by one drag and exact fields, refuses an axis end moved off the axis, and
/// the shared worker and the peer CLI publish the same copy — the same SQL,
/// the same stored angle and names, and the same STL and FBX bytes.
#[test]
fn native_partial_revolve_profile_widgets_drag_worker_and_cli_edit_one_copy() {
    use crate::creates::tests::{ferritecad, read_semantics};
    use ferritecad_document::{Document, ObjectPayload, RevolveExtent};
    use ferritecad_kernel::OperationContext;
    if !ferritecad_occt::is_available() {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: no OCCT for the partial Revolve profile worker");
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("sector.fcad");
    let input = root.path().join("create.json");
    std::fs::write(
        &input,
        r#"{"request_version":2,"points_mm":[[0,0],[10,0],[10,15],[0,15]],"axis":"sketch_y","extent":{"kind":"angle","degrees":220}}"#,
    )
    .expect("request");
    let p = std::process::Command::new(ferritecad())
        .arg("create-sketch-revolve")
        .arg(&input)
        .arg("-o")
        .arg(&source)
        .arg("--json")
        .output()
        .expect("create");
    assert!(p.status.success(), "{p:?}");
    let source_bytes = std::fs::read(&source).expect("source");
    let reading = {
        let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
        ferritecad_scene::snapshot_of(
            &source,
            &mut k,
            |k, b| k.import_step(b),
            &Default::default(),
            &OperationContext::default(),
        )
        .expect("accepted scene")
        .edit_source
        .expect("accepted edit facts")
    };
    let choice = &reading.sketches[0];
    assert_eq!(choice.refusal, None);
    let saved = choice.vertices.clone().expect("editable");
    let Some(SketchProfileUse::PartialRevolve {
        axis_segment: Some(axis),
        degrees,
        ..
    }) = choice.profile_use
    else {
        panic!("a solid sector: {:?}", choice.profile_use)
    };
    assert_eq!(degrees.degrees(), 220.);
    assert_eq!(axis, saved[3].curve_id, "(0,15)→(0,0) lies on the axis");

    // Opened from its own row among the saved-object actions.
    let ctx = egui::Context::default();
    let mut e = Editor::default();
    super::tests::click_saved_action(
        &ctx,
        &mut e,
        &source,
        &reading,
        &format!("Edit Sketch Profile — {}…", choice.sketch),
    );
    assert!(e.editing.is_some(), "the row opened the Sketch editor");
    assert_eq!(
        e.draft.as_ref().expect("draft").feature,
        Feature::RevolveAngle
    );
    assert_eq!(e.draft.as_ref().expect("draft").angle, "220");
    frame(&ctx, &mut e, vec![]);
    let out = frame(&ctx, &mut e, vec![]);
    let texts: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
            _ => None,
        })
        .collect();
    for wanted in [
        "XY · mm · Line polygon · Revolve through an angle about the sketch Y axis · NewBody",
        "Edit exact coordinates. Curve IDs, order, closure and the saved 220° sector about Y \
         are retained.",
        "Revolve: the saved sector, right-handed about the sketch Y axis through X = 0. Its \
         angle is kept; change it with Edit Revolve angle.",
        "Angle °",
        "220",
        "axis (Y) at X = 0 · radius X →",
    ] {
        assert!(texts.iter().any(|t| t == wanted), "{wanted} in {texts:?}");
    }
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("Solid part: edge 4 (Line "))
    );
    for absent in ["Feature:", "Revolve 360°", "Blind height mm"] {
        assert!(!texts.iter().any(|t| t == absent), "{absent} shown");
    }
    choose(&ctx, &mut e, "1 mm");

    // One drag, one Undo step: the top outer corner in by 5 mm.
    let original = e.draft.clone();
    let at = vertex(&ctx, &mut e, 2);
    let dx = -(5. * e.canvas.scale) - 1.25;
    press(&ctx, &mut e, at);
    for fraction in [0.25, 0.5, 0.75, 1.] {
        move_to(&ctx, &mut e, at + egui::vec2(dx * fraction, 0.));
    }
    button(&ctx, &mut e, at + egui::vec2(dx, 0.), false);
    assert_eq!(
        e.draft.as_ref().expect("draft").points[2],
        ["5".to_owned(), "15".to_owned()]
    );
    assert_eq!(e.undo.len(), 1);
    let dragged = e.draft.clone();
    choose(&ctx, &mut e, "Undo draft");
    assert_eq!(e.draft, original);
    choose(&ctx, &mut e, "Redo draft");
    assert_eq!(e.draft, dragged);
    // Exact fields: the top of the part down to 12.5 on both vertices.
    replace_field(&ctx, &mut e, "15", "12.5");
    replace_field(&ctx, &mut e, "15", "12.5");
    let exact = e.draft.clone().expect("draft");
    assert_eq!(
        exact.points,
        [["0", "0"], ["10", "0"], ["5", "12.5"], ["0", "12.5"]]
            .map(|p| p.map(str::to_owned))
            .to_vec()
    );
    assert_eq!(exact.angle, "220", "the angle is not part of the edit");
    // An axis end dragged off the axis is refused in preview and cancelled.
    let at = vertex(&ctx, &mut e, 0);
    let off = at + egui::vec2(3. * e.canvas.scale, 0.);
    press(&ctx, &mut e, at);
    move_to(&ctx, &mut e, off);
    let refused = e.edit_request().expect_err("off the axis").to_string();
    assert!(refused.contains("touches the axis alone"), "{refused}");
    let out = frame(&ctx, &mut e, vec![]);
    assert!(!out.shapes.iter().any(|c| matches!(&c.shape,
        egui::Shape::Text(t) if t.galley.text() == "Save edited copy…")));
    frame(&ctx, &mut e, vec![key(egui::Key::Escape)]);
    button(&ctx, &mut e, off, false);
    assert_eq!(e.draft.as_ref(), Some(&exact));

    choose(&ctx, &mut e, "Save edited copy…");
    let request = e.take_edit_request().expect("submit");
    assert_eq!(request.expected, reading.version);
    assert_eq!(
        request
            .vertices
            .iter()
            .map(|v| v.curve_id)
            .collect::<Vec<_>>(),
        saved.iter().map(|v| v.curve_id).collect::<Vec<_>>()
    );
    let kept = e.draft.clone();
    let mut edits = crate::edits::Edits::default();
    for occupied in [true, false] {
        let mut request = request.clone();
        request.destination = root
            .path()
            .join(if occupied { "occupied.fcad" } else { "ui.fcad" });
        if occupied {
            std::fs::write(&request.destination, b"keep").expect("sentinel");
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let generation = edits
            .start_sketch(request.clone(), move |r, g, c| {
                crate::edits::spawn_sketch_edit(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (g, result) = rx.recv().expect("completed");
        assert_eq!(g, generation);
        let path = finish_edit(&mut e, &mut edits, g, result);
        if occupied {
            assert!(path.is_none(), "a taken destination publishes nothing");
            assert_eq!(e.draft, kept, "and keeps the draft");
            assert_eq!(
                std::fs::read(root.path().join("occupied.fcad")).expect("sentinel"),
                b"keep"
            );
        } else {
            let path = path.expect("published");
            assert!(!e.active());
            e.draft_load_finished(&path, false);
            assert_eq!(e.draft, kept, "a failed Open restores the edited draft");
            e.draft_published(&path);
            e.draft_load_finished(&path, true);
            assert!(!e.active());
        }
    }

    // The peer CLI, the same request, the same source.
    let edit = root.path().join("edit.json");
    let vertices: Vec<String> = request
        .vertices
        .iter()
        .map(|v| {
            format!(
                r#"{{"curve_id":"{}","start_mm":[{},{}]}}"#,
                v.curve_id, v.start_mm[0], v.start_mm[1]
            )
        })
        .collect();
    std::fs::write(
        &edit,
        format!(
            r#"{{"request_version":1,"vertices":[{}]}}"#,
            vertices.join(",")
        ),
    )
    .expect("edit request");
    let ui = root.path().join("ui.fcad");
    let peer = root.path().join("cli.fcad");
    let p = std::process::Command::new(ferritecad())
        .arg("edit-sketch-copy")
        .arg(&source)
        .arg("--sketch")
        .arg(choice.sketch.to_string())
        .arg("--expect-version")
        .arg(reading.version.content.to_string())
        .arg("--request")
        .arg(&edit)
        .arg("-o")
        .arg(&peer)
        .arg("--json")
        .output()
        .expect("peer CLI");
    assert!(p.status.success(), "{p:?}");
    assert_eq!(sql_facts(&ui), sql_facts(&peer), "every SQL cell");
    assert_eq!(read_semantics(&ui), read_semantics(&peer));
    let a = Document::open_read_only(&ui).expect("UI");
    let original = Document::open_read_only(&source).expect("source");
    assert_eq!(
        a.topology_refs().expect("refs"),
        original.topology_refs().expect("refs"),
        "no new names, both caps kept"
    );
    let revolve = |d: &Document| {
        d.objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Revolve(_)))
            .expect("Revolve")
    };
    assert_eq!(
        revolve(&a),
        revolve(&original),
        "the Revolve row, angle included"
    );
    let ObjectPayload::Revolve(r) = revolve(&a).payload else {
        panic!("a Revolve")
    };
    assert!(matches!(r.extent, RevolveExtent::Partial { degrees } if degrees.degrees() == 220.));
    original.close().expect("close");
    // A frustum R = 10, r = 5, h = 12.5, turned through 220°.
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&a, &mut kernel, &context).expect("cold");
    let body = a
        .objects()
        .expect("objects")
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .map(|o| o.id)
        .expect("Body");
    let (faces, volume) = kernel
        .shape_stats(built.shape(body).expect("one solid"))
        .expect("stats");
    assert_eq!(
        faces, 5,
        "two discs, one cone, two caps, no face for the axis"
    );
    let expected = std::f64::consts::PI * 12.5 / 3. * (100. + 50. + 25.) * 220. / 360.;
    assert!(
        (volume - expected).abs() < 1e-9 * expected,
        "{volume} != {expected}"
    );
    built.release_all(&mut kernel);
    a.close().expect("close");
    let export = |path: &std::path::Path| {
        let mut bytes = Vec::new();
        for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
            let out = path.with_extension(extension);
            let p = std::process::Command::new(ferritecad())
                .arg(op)
                .arg(path)
                .arg("-o")
                .arg(&out)
                .output()
                .expect("export");
            assert!(p.status.success(), "{p:?}");
            bytes.push(std::fs::read(&out).expect("bytes"));
        }
        bytes
    };
    assert_eq!(export(&ui), export(&peer), "STL and FBX bytes, no mapping");
    assert_eq!(std::fs::read(&source).expect("source"), source_bytes);
}
