// SPDX-License-Identifier: MIT
//! §31B: the window's layout, run as the window runs it.
//!
//! Every frame here goes through `workspace::compose`, the function
//! `Live::draw` calls, with real egui widgets and input and the window's own
//! owners (`Tabs`, `Sessions`, the forms, `ViewportInput`). The conversion of the
//! layout into the viewport's pixels is `workspace::region_of`. Nothing here
//! draws a layout of its own. There is no graphics device, so this checks
//! *where* things are and *who* gets a press; that the geometry in that place is
//! the right geometry is the device-backed gate beside the renderer and the
//! native one in `main.rs`.

use super::*;
use crate::workspace::{Answers, Scene, View, compose, region_of};
use crate::{Loads, Sections, claimed_by_interface, exports::Exports};
use ferritecad_ui::{Activity, Hover, PointerButton, TabLabel, TabStrip, ViewportEvent};
use ferritecad_viewport::PixelRect;

/// The window in logical points, as an ordinary display lays it out.
const SCREEN: (f32, f32) = (1024.0, 768.0);

/// What a frame is drawn with besides the owners.
#[derive(Clone, Copy)]
pub(in crate::sessions::tests::tabs) struct Content {
    /// Long sentences in every place the window prints one, and long lists, and
    /// every operation that can be stopped is running.
    loud: bool,
    /// A question that has to be answered first is open.
    modal: bool,
    /// What the last New and the last dialog said: one more group above the
    /// editors, drawn from nothing the editors own.
    outcomes: bool,
}

pub(in crate::sessions::tests::tabs) const QUIET: Content = Content {
    loud: false,
    modal: false,
    outcomes: false,
};
const LOUD: Content = Content {
    loud: true,
    modal: false,
    outcomes: false,
};

pub(in crate::sessions::tests::tabs) struct Laid {
    pub(in crate::sessions::tests::tabs) out: egui::FullOutput,
    pub(in crate::sessions::tests::tabs) answers: Answers,
}

/// An activity with every button on, so the whole toolbar is as wide as it gets.
fn busy_activity<'a>(line: &'a str, document_line: &'a str, running: bool) -> Activity<'a> {
    Activity {
        line,
        document_line,
        can_open: true,
        can_create_document: true,
        can_save: true,
        can_save_as: true,
        can_undo_document: true,
        can_redo_document: true,
        can_export: true,
        can_export_stl: true,
        can_frame_selection: true,
        can_frame_scene: true,
        can_hide: true,
        can_show_all: true,
        can_isolate: true,
        can_undo_visibility: true,
        // Every button that appears only while something runs.
        can_cancel_create: running,
        can_cancel_document: running,
        can_cancel_export: running,
        progress: running.then_some(0.5),
        dirty: true,
        ..Activity::default()
    }
}

/// A long run of words, as a failure message is.
fn sentence(words: usize) -> String {
    (0..words)
        .map(|n| format!("word{n}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// One frame of the whole interface, through the production layout.
pub(in crate::sessions::tests::tabs) fn frame(
    w: &mut Window,
    ctx: &egui::Context,
    screen: (f32, f32),
    pixels_per_point: f32,
    events: Vec<egui::Event>,
    content: Content,
) -> Laid {
    ctx.set_pixels_per_point(pixels_per_point);
    let line = if content.loud {
        sentence(400)
    } else {
        "Ready".to_owned()
    };
    let outcome = if content.loud {
        sentence(1500)
    } else {
        String::new()
    };
    // A path or an identifier is one word: it cannot wrap, so it must scroll.
    let token = "x".repeat(400);
    let definitions: Vec<_> = if content.loud {
        (0..400)
            .map(|_| ferritecad_ui::Selected::Body {
                name: Some("part"),
                object: "same",
            })
            .collect()
    } else {
        Vec::new()
    };
    let tab_rows = w.tabs.labels(&w.sessions);
    let labels: Vec<TabLabel<'_>> = tab_rows
        .iter()
        .map(|(tab, name, dirty, active)| TabLabel {
            key: tab.key(),
            name,
            dirty: *dirty,
            active: *active,
        })
        .collect();
    let height = crate::height_state(
        &w.edits,
        &w.sessions,
        &w.creates,
        &Loads::default(),
        &Exports::default(),
    );
    let model_available = crate::model_tree::availability(
        &w.creates,
        &w.edits,
        &w.sessions,
        &w.tabs,
        &Loads::default(),
        &Exports::default(),
        &w.input,
    );
    let model = w.sessions.has_session().then(|| {
        (
            w.sessions.tab().key(),
            w.tabs.model_navigation(w.sessions.tab()),
        )
    });
    let forms_scope = if w.sessions.has_session() {
        w.sessions.tab().key()
    } else {
        0
    };
    let (form, sketch) = w.creates.forms();
    let sections = Sections {
        dialog_failure: (content.loud || content.outcomes).then_some(token.as_str()),
        sketch,
        creating: false,
        edits: &mut w.edits,
        stl_form: None,
        can_edit: true,
        can_new: true,
        height,
        held_back: content.loud,
        document_outcome: &outcome,
        failure: None,
        export: None,
        replacing: None,
        form,
        open_over_new: None,
        closing_form: content.modal,
        close_form: content.modal.then_some(ferritecad_ui::CloseFormPanel {
            document: "plate.fcad",
            quitting: false,
        }),
        quit_new: None,
        created: (content.loud || content.outcomes).then_some(token.as_str()),
        reopen: Default::default(),
        recovery: Default::default(),
        checkpoints: None,
        tabs: TabStrip {
            tabs: &labels,
            available: Ok(()),
        },
        forms_scope,
        model,
        model_available,
    };
    let mut view = View {
        activity: busy_activity(&line, &outcome, content.loud),
        sections,
        scene: Scene {
            document: None,
            edit_source: w.scene.edit_source.as_ref(),
            definitions: &definitions,
            chosen_row: None,
            offers: &[],
            described: None,
            solves: &[],
        },
    };
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(screen.0, screen.1),
        )),
        events,
        ..Default::default()
    };
    let mut answers = Answers::default();
    let mut out = ctx.run_ui(raw, |ui| answers = compose(ui, &mut view));
    out.textures_delta.clear();
    Laid { out, answers }
}

/// Frames until the panels have found their sizes.
fn settle(
    w: &mut Window,
    ctx: &egui::Context,
    screen: (f32, f32),
    pixels_per_point: f32,
    content: Content,
) -> Laid {
    let mut laid = frame(w, ctx, screen, pixels_per_point, vec![], content);
    for _ in 0..3 {
        laid = frame(w, ctx, screen, pixels_per_point, vec![], content);
    }
    laid
}

/// The physical pixels of the window the viewport covers, for a surface that is
/// `screen` points at `pixels_per_point`.
fn region(laid: &Laid, screen: (f32, f32), pixels_per_point: f32) -> PixelRect {
    region_of(
        laid.answers.central,
        pixels_per_point,
        (
            (screen.0 * pixels_per_point) as u32,
            (screen.1 * pixels_per_point) as u32,
        ),
    )
}

fn texts(out: &egui::FullOutput, label: &str) -> Vec<egui::Rect> {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == label => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            _ => None,
        })
        .collect()
}

fn pointer(at: egui::Pos2, down: Option<bool>) -> Vec<egui::Event> {
    let mut events = vec![egui::Event::PointerMoved(at)];
    if let Some(pressed) = down {
        events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    events
}

/// Whatever is painted, visibly, over the viewport.
fn painted_over(out: &egui::FullOutput, central: egui::Rect) -> Vec<String> {
    let inner = central.shrink(0.75);
    out.shapes
        .iter()
        .filter_map(|clipped| {
            let bounds = clipped
                .shape
                .visual_bounding_rect()
                .intersect(clipped.clip_rect);
            (bounds.is_positive() && bounds.intersects(inner))
                .then(|| format!("{:?} at {bounds:?}", clipped.shape))
        })
        .collect()
}

fn open_plates(w: &mut Window, heights: &[f64]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let root = tempfile::tempdir().expect("root");
    let mut paths = Vec::new();
    for (n, height) in heights.iter().enumerate() {
        let path = root.path().join(format!("plate{n}.fcad"));
        plate(&path, *height);
        assert_eq!(w.open(&path), Opening::Load);
        paths.push(path);
    }
    (root, paths)
}

#[test]
fn long_messages_and_long_lists_cannot_move_the_viewport_or_paint_over_it() {
    let (_root, path, _) = crate::cuts::tests::session_apply::fixture(16);
    let mut w = Window::new(Drawn::Mock, None);
    super::drafts::model_tree::open_facts(&mut w, &path);
    let ctx = egui::Context::default();

    let quiet = settle(&mut w, &ctx, SCREEN, 1.0, QUIET);
    let central = quiet.answers.central;
    // Beside two panels and between two strips, and a real size: a window
    // 1024x768 is not left with a thumbnail.
    assert!(
        central.width() > 380.0 && central.height() > 520.0,
        "{central:?}"
    );
    assert!(
        central.min.x >= 160.0 && central.min.y > 20.0,
        "{central:?}"
    );
    assert!(
        central.max.x < SCREEN.0 && central.max.y < SCREEN.1,
        "{central:?}"
    );
    assert!(
        painted_over(&quiet.out, central).is_empty(),
        "{:?}",
        painted_over(&quiet.out, central)
    );

    // At an ordinary width every button of the top strip is in view, the last
    // of the views as much as Open…: the strip wraps before it scrolls.
    let (view, name, key) = ferritecad_ui::VIEWS.last().expect("views");
    let _ = view;
    for label in ["Open…".to_owned(), format!("{name} ({key})")] {
        let shown: Vec<_> = quiet
            .out
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == label => {
                    let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
                    Some(clipped.clip_rect.contains_rect(rect))
                }
                _ => None,
            })
            .collect();
        assert_eq!(shown, [true], "{label} is not wholly in view");
    }
    eprintln!("viewport at 1024x768: {central:?}");

    // Four hundred words in the activity line, fifteen hundred in the document's
    // last outcome, four hundred definitions, sixteen Cuts in the Model tree:
    // the viewport is where it was to the pixel, and nothing is painted on it.
    // And one object whose name, kind and note are each a paragraph long.
    for row in &mut w
        .scene
        .edit_source
        .as_mut()
        .expect("accepted catalogue")
        .model_tree
        .rows
    {
        row.name = sentence(60);
        row.kind = sentence(20);
        row.note = sentence(80);
    }
    let loud = settle(&mut w, &ctx, SCREEN, 1.0, LOUD);
    assert_eq!(
        loud.answers.central, central,
        "long content changed the viewport"
    );
    let over = painted_over(&loud.out, central);
    assert!(over.is_empty(), "painted over the viewport: {over:?}");
    // And the content is reachable: the words are drawn, in the status strip.
    assert!(
        loud.out.shapes.iter().any(|clipped| matches!(
            &clipped.shape,
            egui::epaint::Shape::Text(t) if t.galley.text().starts_with("word0 word1")
        )),
        "the long message was not drawn at all"
    );
    // The status strip is one height whatever it says.
    assert_eq!(
        SCREEN.1 - loud.answers.central.max.y,
        SCREEN.1 - quiet.answers.central.max.y
    );
    println!("\nFCAD_31B_LAYOUT_BOUNDS_EXECUTED");
}

#[test]
fn the_same_layout_at_two_pixels_per_point_is_the_same_rectangle_twice_as_large() {
    let mut w = Window::new(Drawn::Mock, None);
    let (_root, _paths) = open_plates(&mut w, &[12.0]);
    let one = settle(&mut w, &egui::Context::default(), SCREEN, 1.0, QUIET);
    let two = settle(&mut w, &egui::Context::default(), SCREEN, 2.0, QUIET);
    // The same points, so the same layout.
    assert_eq!(one.answers.central, two.answers.central);
    let a = region(&one, SCREEN, 1.0);
    let b = region(&two, SCREEN, 2.0);
    assert_eq!(
        (b.x(), b.y(), b.width(), b.height()),
        (2 * a.x(), 2 * a.y(), 2 * a.width(), 2 * a.height())
    );
    // A pointer at the same place in points is twice the pixels, and lands at
    // twice the place in the picture; the aspect is the same.
    let mut near = ViewportInput::new();
    near.set_region(a);
    let mut sharp = ViewportInput::new();
    sharp.set_region(b);
    let at = (a.x() as f32 + 30.0, a.y() as f32 + 20.0);
    near.handle(ViewportEvent::PointerMoved { x: at.0, y: at.1 }, false);
    sharp.handle(
        ViewportEvent::PointerMoved {
            x: at.0 * 2.0,
            y: at.1 * 2.0,
        },
        false,
    );
    assert_eq!(near.take_hover(), Hover::At(30.0, 20.0));
    assert_eq!(sharp.take_hover(), Hover::At(60.0, 40.0));
    assert_eq!(near.camera().aspect(), sharp.camera().aspect());
    println!("\nFCAD_31B_SCALE_EXECUTED");
}

#[test]
fn an_empty_window_and_an_empty_document_stay_controllable() {
    let ctx = egui::Context::default();
    let mut empty = Window::new(Drawn::Mock, None);
    let laid = settle(&mut empty, &ctx, SCREEN, 1.0, QUIET);
    assert!(laid.answers.central.width() > 380.0, "{:?}", laid.answers);
    assert!(
        !texts(&laid.out, "No document is open.").is_empty(),
        "the Model panel says nothing about having no document"
    );
    // Open… is where a person finds it, and is pressed through the layout.
    let open = texts(&laid.out, "Open…");
    assert_eq!(open.len(), 1, "one Open… button");
    let mut pressed = false;
    for step in [None, Some(true), Some(false)] {
        let at = open[0].center();
        let events = match step {
            None => pointer(at, None),
            other => pointer(at, other),
        };
        pressed |= frame(&mut empty, &ctx, SCREEN, 1.0, events, QUIET)
            .answers
            .chosen
            .open;
    }
    assert!(pressed, "Open… did not answer a press");
    // The Actions panel offers New… and the Create routes with no document.
    assert!(!texts(&laid.out, "Create sketch + Extrude…").is_empty());

    // Opening the first document does not move the viewport: a model framed for
    // the empty window's rectangle is still framed for the one with a tab in it.
    let mut first = Window::new(Drawn::Mock, None);
    let (_root, _paths) = open_plates(&mut first, &[12.0]);
    let with_a_tab = settle(&mut first, &egui::Context::default(), SCREEN, 1.0, QUIET);
    eprintln!(
        "empty {:?} with one tab {:?}",
        laid.answers.central, with_a_tab.answers.central
    );
    assert_eq!(
        laid.answers.central, with_a_tab.answers.central,
        "the first tab moved the viewport"
    );

    // An empty document: no Body, no history, no rows. The window still has its
    // picture and every panel.
    let mut w = Window::new(Drawn::Mock, None);
    w.create(ferritecad_jobs::NewDocument::Empty)
        .expect("an empty document");
    let laid = settle(&mut w, &egui::Context::default(), SCREEN, 1.0, QUIET);
    assert!(laid.answers.central.width() > 380.0);
    assert!(laid.answers.chosen.model.is_none());
    assert!(
        !texts(&laid.out, "No stored model objects").is_empty()
            || !texts(&laid.out, "Model").is_empty(),
        "the Model panel is gone"
    );
    // A fast load can finish before the first layout. Its initial square
    // camera must be framed again for the real (narrow) viewport exactly once.
    let bounds = Some(([-30.0, -20.0, 0.0], [30.0, 20.0, 10.0]));
    let mut input = ViewportInput::new();
    input.frame_extent(bounds).expect("early load framing");
    let mut ready = false;
    crate::workspace::place_view(&mut input, PixelRect::new(0, 0, 0, 0), bounds, &mut ready)
        .expect("not yet drawable");
    assert!(!ready);
    let region = PixelRect::new(230, 106, 494, 602);
    crate::workspace::place_view(&mut input, region, bounds, &mut ready)
        .expect("first usable layout");
    assert!(ready);
    let mut expected = ViewportInput::new();
    expected.set_region(region);
    expected
        .frame_extent(bounds)
        .expect("frame at the right aspect");
    assert_eq!(input.camera(), expected.camera());

    // Later layout changes, including an empty/minimized viewport, only resize
    // the camera: they do not replace the user's view with another Frame all.
    let eye = input.camera().eye();
    let target = input.camera().target();
    for region in [
        PixelRect::new(0, 0, 0, 0),
        PixelRect::new(340, 120, 900, 400),
    ] {
        crate::workspace::place_view(&mut input, region, bounds, &mut ready).expect("later layout");
        assert_eq!(input.camera().eye(), eye);
        assert_eq!(input.camera().target(), target);
    }
    println!("\nFCAD_31B_EMPTY_EXECUTED");
}

/// Moves to `at` (in points) and then presses or releases there, as the window
/// is told: each event is judged by what the interface wanted at the end of the
/// last completed frame (`EventResponse::consumed` is that, for a press or a
/// release, and false for an idle move), and then a frame is run with it.
fn deliver(
    w: &mut Window,
    ctx: &egui::Context,
    pixels_per_point: f32,
    at: egui::Pos2,
    down: Option<bool>,
) {
    let tell = |w: &mut Window, event: ViewportEvent, consumed: bool, wants: bool| {
        let claimed = claimed_by_interface(&event, consumed, wants);
        w.input.handle(event, claimed);
    };
    let x = at.x * pixels_per_point;
    let y = at.y * pixels_per_point;
    let wants = ctx.egui_wants_pointer_input();
    tell(w, ViewportEvent::PointerMoved { x, y }, false, wants);
    let _ = frame(w, ctx, SCREEN, pixels_per_point, pointer(at, None), QUIET);
    let Some(pressed) = down else {
        return;
    };
    let wants = ctx.egui_wants_pointer_input();
    let button = PointerButton::Primary;
    let event = if pressed {
        ViewportEvent::PointerPressed(button)
    } else {
        ViewportEvent::PointerReleased(button)
    };
    tell(w, event, wants, wants);
    let _ = frame(w, ctx, SCREEN, pixels_per_point, pointer(at, down), QUIET);
}

#[test]
fn panels_keep_their_presses_and_the_viewport_keeps_its_own() {
    let mut w = Window::new(Drawn::Mock, None);
    let (_root, _paths) = open_plates(&mut w, &[12.0]);
    let ctx = egui::Context::default();
    let laid = settle(&mut w, &ctx, SCREEN, 1.0, QUIET);
    let central = laid.answers.central;
    w.input.set_region(region(&laid, SCREEN, 1.0));
    w.input.take_redraw();
    let before = *w.input.camera();

    // A press on each panel and strip, on the real widgets' own ground: the top
    // strip, the Model panel, the Actions panel, the status strip.
    for (what, at) in [
        (
            "top strip",
            egui::pos2(central.center().x, central.min.y - 8.0),
        ),
        (
            "Model panel",
            egui::pos2(central.min.x - 30.0, central.center().y),
        ),
        (
            "Actions panel",
            egui::pos2(central.max.x + 30.0, central.center().y),
        ),
        (
            "status strip",
            egui::pos2(central.center().x, central.max.y + 8.0),
        ),
    ] {
        deliver(&mut w, &ctx, 1.0, at, None);
        deliver(&mut w, &ctx, 1.0, at, Some(true));
        assert!(
            !w.input.is_dragging(),
            "a press on the {what} began a gesture"
        );
        deliver(&mut w, &ctx, 1.0, at, Some(false));
        assert_eq!(
            w.input.take_pick(),
            None,
            "a click on the {what} chose something"
        );
        assert_eq!(*w.input.camera(), before, "the {what} moved the camera");
    }

    // The real Open… button is a press that goes to the button.
    let open = texts(&laid.out, "Open…")[0].center();
    deliver(&mut w, &ctx, 1.0, open, None);
    let pressed = frame(&mut w, &ctx, SCREEN, 1.0, pointer(open, Some(true)), QUIET);
    let released = frame(&mut w, &ctx, SCREEN, 1.0, pointer(open, Some(false)), QUIET);
    assert!(
        pressed.answers.chosen.open || released.answers.chosen.open,
        "the button did not take the press"
    );

    // A click inside the viewport is the viewport's: nothing in a panel's way.
    let inside = central.center();
    deliver(&mut w, &ctx, 1.0, inside, None);
    assert!(!ctx.egui_wants_pointer_input(), "the viewport is egui's");
    deliver(&mut w, &ctx, 1.0, inside, Some(true));
    assert!(w.input.is_dragging());
    deliver(&mut w, &ctx, 1.0, inside, Some(false));
    let (x, y) = w.input.take_pick().expect("a click in the viewport asks");
    assert!((x - central.width() / 2.0).abs() < 1.0);
    assert!((y - central.height() / 2.0).abs() < 1.0);

    // A drag that began in the viewport goes on across the Actions panel, turns
    // the model, and does not pick where it ends.
    deliver(&mut w, &ctx, 1.0, inside, None);
    deliver(&mut w, &ctx, 1.0, inside, Some(true));
    let across = egui::pos2(central.max.x + 60.0, inside.y + 30.0);
    deliver(&mut w, &ctx, 1.0, across, None);
    assert_ne!(*w.input.camera(), before, "the drag stopped at the panel");
    assert!(w.input.is_dragging());
    deliver(&mut w, &ctx, 1.0, across, Some(false));
    assert!(!w.input.is_dragging());
    assert_eq!(w.input.take_pick(), None, "the end of a drag picked");
    println!("\nFCAD_31B_OWNERSHIP_EXECUTED");
}

#[test]
fn dragging_a_panel_moves_the_viewport_and_what_was_asked_in_the_old_one_is_dropped() {
    let mut w = Window::new(Drawn::Mock, None);
    let (_root, _paths) = open_plates(&mut w, &[12.0]);
    let ctx = egui::Context::default();
    let laid = settle(&mut w, &ctx, SCREEN, 1.0, QUIET);
    let first = region(&laid, SCREEN, 1.0);
    w.input.set_region(first);
    w.input.take_redraw();

    // A click and a hover are waiting, asked in the first region.
    let inside = laid.answers.central.center();
    deliver(&mut w, &ctx, 1.0, inside, None);
    deliver(&mut w, &ctx, 1.0, inside, Some(true));
    deliver(&mut w, &ctx, 1.0, inside, Some(false));
    deliver(&mut w, &ctx, 1.0, inside, None);

    // The Model panel's edge is dragged 20 points to the right, and the Actions
    // panel's 20 to the right as well: the same size of viewport, further along.
    let central = laid.answers.central;
    let left_edge = egui::pos2(central.min.x, central.center().y);
    let right_edge = egui::pos2(central.max.x, central.center().y);
    for (edge, to) in [(left_edge, 20.0), (right_edge, 20.0)] {
        let mut at = edge;
        let _ = frame(&mut w, &ctx, SCREEN, 1.0, pointer(at, None), QUIET);
        let _ = frame(&mut w, &ctx, SCREEN, 1.0, pointer(at, Some(true)), QUIET);
        for step in 1..=4 {
            at = edge + egui::vec2(to * step as f32 / 4.0, 0.0);
            let _ = frame(&mut w, &ctx, SCREEN, 1.0, pointer(at, None), QUIET);
        }
        let _ = frame(&mut w, &ctx, SCREEN, 1.0, pointer(at, Some(false)), QUIET);
    }
    let moved = settle(&mut w, &ctx, SCREEN, 1.0, QUIET);
    let second = region(&moved, SCREEN, 1.0);
    assert_eq!(second.x(), first.x() + 20, "the Model panel did not move");
    assert_eq!(
        second.width(),
        first.width(),
        "moving both edges changed the size"
    );

    // Same size, different origin: pending asks are forgotten, not answered in
    // the new place.
    assert!(w.input.set_region(second));
    assert_eq!(w.input.take_pick(), None, "an old click survived the move");
    assert_eq!(w.input.take_hover(), Hover::Cleared);
    assert_eq!(w.input.camera().width(), first.width());
    println!("\nFCAD_31B_REGION_MOVE_EXECUTED");
}

#[test]
fn each_tab_keeps_its_camera_across_a_layout_change_and_is_fitted_to_the_present_viewport() {
    let mut w = Window::new(Drawn::Mock, None);
    let (_root, paths) = open_plates(&mut w, &[12.0, 20.0]);
    let ctx = egui::Context::default();
    let laid = settle(&mut w, &ctx, SCREEN, 1.0, QUIET);
    let first = region(&laid, SCREEN, 1.0);
    w.input.set_region(first);
    let second_tab = w.sessions.tab();

    // Each tab is turned differently while it is the shown one.
    let turn = |w: &mut Window, dx: f32| {
        let at = (first.x() as f32 + 100.0, first.y() as f32 + 100.0);
        w.input
            .handle(ViewportEvent::PointerMoved { x: at.0, y: at.1 }, false);
        w.input
            .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
        w.input.handle(
            ViewportEvent::PointerMoved {
                x: at.0 + dx,
                y: at.1 + 10.0,
            },
            false,
        );
        w.input.handle(
            ViewportEvent::PointerReleased(PointerButton::Primary),
            false,
        );
    };
    turn(&mut w, 60.0);
    let camera_b = *w.input.camera();
    let first_tab = w
        .tabs
        .labels(&w.sessions)
        .iter()
        .map(|(tab, ..)| *tab)
        .find(|tab| *tab != second_tab)
        .expect("the other tab");
    w.switch(first_tab);
    // Opening a tab frames it by its own picture, so it starts from its own view.
    turn(&mut w, -90.0);
    let camera_a = *w.input.camera();
    assert_ne!(camera_a.eye(), camera_b.eye());

    // The panels move while the other tab is shown: a different viewport.
    let narrow = egui::vec2(SCREEN.0 - 120.0, SCREEN.1);
    let relaid = settle(
        &mut w,
        &egui::Context::default(),
        (narrow.x, narrow.y),
        1.0,
        QUIET,
    );
    let present = region(&relaid, (narrow.x, narrow.y), 1.0);
    assert_ne!(present, first);
    w.input.set_region(present);

    // Back and forth: each tab's view is its own, and is fitted to where the
    // viewport is now, not where it was when the tab was left.
    w.switch(second_tab);
    assert_eq!(w.input.camera().eye(), camera_b.eye());
    assert_eq!(w.input.camera().target(), camera_b.target());
    assert_eq!(w.input.region(), present);
    assert_eq!(
        (w.input.camera().width(), w.input.camera().height()),
        (present.width(), present.height())
    );
    w.switch(first_tab);
    assert_eq!(w.input.camera().eye(), camera_a.eye());
    assert_eq!(w.input.region(), present);
    drop(paths);
    println!("\nFCAD_31B_TABS_EXECUTED");
}

#[test]
fn a_question_that_must_be_answered_first_leaves_the_layout_and_the_viewport_alone() {
    let mut w = Window::new(Drawn::Mock, None);
    let (_root, _paths) = open_plates(&mut w, &[12.0]);
    let ctx = egui::Context::default();
    let calm = settle(&mut w, &ctx, SCREEN, 1.0, QUIET);
    let asking = settle(
        &mut w,
        &ctx,
        SCREEN,
        1.0,
        Content {
            loud: false,
            modal: true,
            outcomes: false,
        },
    );
    assert_eq!(
        asking.answers.central, calm.answers.central,
        "an open question moved the viewport"
    );
    assert!(!texts(&asking.out, "Question").is_empty());
    // The rest of the window is drawn and cannot be pressed.
    let open = texts(&asking.out, "Open…")[0].center();
    let mut pressed = false;
    for step in [None, Some(true), Some(false)] {
        let events = pointer(open, step);
        pressed |= frame(
            &mut w,
            &ctx,
            SCREEN,
            1.0,
            events,
            Content {
                loud: false,
                modal: true,
                outcomes: false,
            },
        )
        .answers
        .chosen
        .open;
    }
    assert!(
        !pressed,
        "a disabled button took a press during the question"
    );
    println!("\nFCAD_31B_MODAL_EXECUTED");
}

#[test]
fn a_form_keeps_its_focus_and_text_when_the_groups_above_it_come_and_go() {
    let mut w = Window::new(Drawn::Mock, None);
    let (_root, _paths) = open_plates(&mut w, &[12.0]);
    w.open_height_form();
    let ctx = egui::Context::default();
    let laid = settle(&mut w, &ctx, SCREEN, 1.0, QUIET);

    // The extrusion, chosen by a real press on its row (the first thing drawn
    // after the form's instruction).
    let after = {
        let mut seen = false;
        laid.out
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(t) if seen => {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()).center())
                }
                egui::epaint::Shape::Text(t)
                    if t.galley.text() == "Choose the existing extrusion to change:" =>
                {
                    seen = true;
                    None
                }
                _ => None,
            })
    }
    .expect("the form lists its extrusion");
    for step in [None, Some(true), Some(false)] {
        let _ = frame(&mut w, &ctx, SCREEN, 1.0, pointer(after, step), QUIET);
    }
    let (feature, before) = {
        let (feature, text) = w.edits.typed().expect("the height form is open");
        (feature, text.to_owned())
    };
    assert!(feature.is_some(), "the extrusion was chosen");
    let laid = settle(&mut w, &ctx, SCREEN, 1.0, QUIET);

    // Into the "New distance" field, by a real press, and a real keystroke.
    let label = texts(&laid.out, "New distance");
    assert_eq!(label.len(), 1, "one distance field");
    let field = label[0].right_center() + egui::vec2(40.0, 0.0);
    for step in [None, Some(true), Some(false)] {
        let _ = frame(&mut w, &ctx, SCREEN, 1.0, pointer(field, step), QUIET);
    }
    let _ = frame(
        &mut w,
        &ctx,
        SCREEN,
        1.0,
        vec![egui::Event::Text("7".to_owned())],
        QUIET,
    );
    let focused = ctx
        .memory(|memory| memory.focused())
        .expect("the field has focus");
    let typed = w.edits.typed().expect("open").1.to_owned();
    assert_ne!(typed, before, "the keystroke reached the field");

    // Groups above the editors appear (what the last New and the last dialog
    // said) and go away again: the field is the same field, with the same text.
    // Everything loud is not asked of this: it pushes the form out of the panel's
    // view, and a field nobody can see is not drawn and so does not keep focus.
    for content in [
        Content {
            outcomes: true,
            ..QUIET
        },
        QUIET,
        Content {
            outcomes: true,
            ..QUIET
        },
    ] {
        let _ = settle(&mut w, &ctx, SCREEN, 1.0, content);
        assert_eq!(
            ctx.memory(|memory| memory.focused()),
            Some(focused),
            "the field lost its identity when a group above it changed"
        );
        assert_eq!(w.edits.typed().expect("open").1, typed);
    }
    println!("\nFCAD_31B_FORM_IDS_EXECUTED");
}
