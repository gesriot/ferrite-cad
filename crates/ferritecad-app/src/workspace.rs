// SPDX-License-Identifier: MIT
//! §31B: where each thing is in the window.
//!
//! The model owns the middle of the window and the controls own panels around
//! it. The layout is one function, [`compose`], that the window draws every
//! frame and that the tests run with real widgets, so that "where the picture
//! is" is not a number two layouts could disagree about.
//!
//! * **Top**: the file and view actions, wrapping into two rows and scrolling
//!   beyond that, and the tabs, likewise. Bounded whatever the window's width.
//! * **Left**: the Model tree, scrolling.
//! * **Right**: Actions and Details, scrolling: the questions that need an
//!   answer and what the last operations said, recovery and checkpoints, the
//!   Create/Add/Edit routes with their forms, then the definitions, the
//!   topology inspector and the sketch solve status.
//! * **Bottom**: what the window is doing, in a strip of one fixed height that
//!   scrolls rather than growing, so a long sentence is read in it, never over
//!   the picture, and never moves the picture.
//!
//! What is left in the middle, when all of those are laid out, is the
//! viewport. [`compose`] returns that rectangle in logical points, and the one
//! conversion to physical pixels is `PixelRect::from_points`: the rectangle is
//! not a panel egui draws, because egui treats whatever no panel covers as not
//! its own, which is what lets a press there reach the camera.
//!
//! A question that has to be answered first (Close with an unfinished form,
//! Quit) takes the right panel, and everything else is drawn but disabled, so
//! the layout - and with it the picture's rectangle - does not jump while it
//! is open and nothing else can be pressed.

use std::path::Path;

use ferritecad_document::ExtrudeEditSource;
use ferritecad_ui::{
    Activity, Chosen, ModelRow, NewChoice, ReplaceChoice, RowVisibility, Selected, SolvedSketch,
};
use ferritecad_viewport::PixelRect;

use crate::{Sections, in_tab_scope, model_tree, sketch::accepted::Accepted};

/// The most room the action buttons and the tabs may take at the top, each, in
/// points: two rows. The buttons wrap into them at ordinary widths so that all
/// of them can be seen, and a window narrower than that scrolls them instead
/// of growing the strip over the picture.
const TOOLBAR_MAX_HEIGHT: f32 = 56.0;
const TABS_MAX_HEIGHT: f32 = 66.0;

/// The tab strip is at least one row tall even with no tab in it, so that the
/// first document opening - which gives the window its first tab - does not take
/// a row from the viewport after the model has been framed for it.
const TABS_ROW_HEIGHT: f32 = 32.0;

/// How tall the strip of status lines is, in points: three lines. Always this
/// tall, whether there is one short sentence or a page of them, because a strip
/// that grew with its message would move the viewport every time one came and
/// went, and the model would change size under the person reading it.
const STATUS_HEIGHT: f32 = 60.0;

/// What the Model panel's width may be - least, first and most, in points - before
/// the window's own width limits it.
const MODEL_WIDTH: (f32, f32, f32) = (160.0, 230.0, 340.0);

/// And the Actions and Details panel's.
const ACTIONS_WIDTH: (f32, f32, f32) = (220.0, 300.0, 480.0);

/// The share of the window's width the two side panels may take at most, each:
/// a window dragged narrow keeps a picture instead of two panels and no room.
const MODEL_SHARE: f32 = 0.28;
const ACTIONS_SHARE: f32 = 0.34;

/// Where the last central rectangle is remembered, for windows that open later.
fn central_id() -> egui::Id {
    egui::Id::new("ferritecad workspace central")
}

/// Where the viewport was in the last frame, in points, for the place a new
/// floating form first appears. `None` before the first frame has been laid out.
pub(crate) fn central_of(ctx: &egui::Context) -> Option<egui::Rect> {
    ctx.data(|data| data.get_temp::<egui::Rect>(central_id()))
}

/// What the picture and the document say this frame, in the words the panels
/// show. Borrowed from the scene; nothing here is worked out again per frame.
pub(crate) struct Scene<'a> {
    pub(crate) document: Option<&'a Path>,
    pub(crate) edit_source: Option<&'a ExtrudeEditSource>,
    /// §31K: the drawings of the same accepted scene, as the same reading saw them.
    pub(crate) sketches: &'a Accepted,
    pub(crate) definitions: &'a [Selected<'a>],
    pub(crate) chosen_row: Option<usize>,
    pub(crate) offers: &'a [RowVisibility],
    pub(crate) described: Option<Selected<'a>>,
    pub(crate) solves: &'a [SolvedSketch<'a>],
}

/// Everything one frame of the interface is drawn from.
pub(crate) struct View<'a> {
    pub(crate) activity: Activity<'a>,
    pub(crate) sections: Sections<'a>,
    pub(crate) scene: Scene<'a>,
}

/// What the person asked for in this frame, and where the picture is.
#[derive(Debug)]
pub(crate) struct Answers {
    pub(crate) chosen: Chosen,
    pub(crate) replace: ReplaceChoice,
    pub(crate) asked: NewChoice,
    pub(crate) pointed_row: Option<usize>,
    /// What no panel covers, in logical points: the viewport.
    pub(crate) central: egui::Rect,
}

impl Default for Answers {
    fn default() -> Self {
        Self {
            chosen: Chosen::default(),
            replace: ReplaceChoice::default(),
            asked: NewChoice::default(),
            pointed_row: None,
            // No viewport until something has been laid out.
            central: egui::Rect::NOTHING,
        }
    }
}

/// The pixels of the window that the viewport covers.
///
/// The one place a layout, in logical points, becomes the rectangle that the
/// camera is sized by, the renderer draws through and the pointer is
/// translated into. `surface` is the window texture's size in physical pixels.
pub(crate) fn region_of(
    central: egui::Rect,
    pixels_per_point: f32,
    surface: (u32, u32),
) -> PixelRect {
    PixelRect::from_points(
        [central.min.x, central.min.y],
        [central.max.x, central.max.y],
        pixels_per_point,
        surface,
    )
}

/// Complete initial framing after the first usable layout. A load may already
/// have framed its model while the camera still had zero size (square aspect).
/// Only the first layout does this: resizing, minimising and switching tabs must
/// keep the user's camera, not repeatedly invoke Frame all.
pub(crate) fn place_view(
    input: &mut ferritecad_ui::ViewportInput,
    region: PixelRect,
    bounds: Option<([f32; 3], [f32; 3])>,
    ready: &mut bool,
) -> ferritecad_types::Result<()> {
    input.set_region(region);
    if !*ready && !region.is_empty() {
        input.frame_extent(bounds)?;
        *ready = true;
    }
    Ok(())
}

/// A side panel's contents, scrolling both ways in the panel's own width.
///
/// Scrolling one way only is not a bound: a vertical scroll area grows to the
/// width of its widest content, and a panel grows with it, so one long name in
/// the Model tree would take the viewport's room. Here the contents are as wide
/// as the panel - labels wrap at that width - and whatever cannot wrap, a row of
/// a tree or a line of a path, scrolls sideways inside it.
fn scrolled(ui: &mut egui::Ui, id: &'static str, add: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    egui::ScrollArea::both()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(width);
            add(ui);
        });
}

/// A group of the right panel under an id of its own, so that a group above it
/// appearing or going away does not renumber the fields below it.
fn group<R>(ui: &mut egui::Ui, id: &'static str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.push_id(("ferritecad workspace group", id), add).inner
}

/// How wide a side panel may be in a window this wide.
fn width_range(
    window: f32,
    (least, _, most): (f32, f32, f32),
    share: f32,
) -> std::ops::RangeInclusive<f32> {
    let limit = (window * share).clamp(least, most);
    least..=limit
}

/// Lays the whole interface out and draws it, and returns what was asked for.
pub(crate) fn compose(ui: &mut egui::Ui, view: &mut View<'_>) -> Answers {
    let mut answers = Answers::default();
    let modal = view.sections.closing_form;
    let activity = view.activity;
    let window = ui.available_width();

    // The order panels are added in is the order they are nested in: the first
    // is outermost. The strips span the window; the sides share what is left.
    egui::Panel::top("ferritecad top panel")
        .resizable(false)
        .show(ui, |ui| {
            ui.add_enabled_ui(!modal, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("ferritecad toolbar rows")
                    .max_height(TOOLBAR_MAX_HEIGHT)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        answers.chosen = ferritecad_ui::toolbar_actions(ui, activity);
                    });
                let tabs = view.sections.tabs;
                egui::ScrollArea::vertical()
                    .id_salt("ferritecad tab rows")
                    .max_height(TABS_MAX_HEIGHT)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_min_height(TABS_ROW_HEIGHT);
                        answers.chosen.tab = ferritecad_ui::tab_strip(ui, tabs);
                    });
            });
        });

    egui::Panel::bottom("ferritecad status panel")
        .resizable(false)
        .show(ui, |ui| {
            ui.add_enabled_ui(!modal, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("ferritecad status lines")
                    .min_scrolled_height(STATUS_HEIGHT)
                    .max_height(STATUS_HEIGHT)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let lines = ferritecad_ui::activity_lines(ui, activity);
                        answers.chosen.cancel = lines.cancel;
                        answers.chosen.cancel_create = lines.cancel_create;
                        answers.chosen.cancel_document = lines.cancel_document;
                        answers.chosen.cancel_export = lines.cancel_export;
                    });
            });
        });

    egui::Panel::left("model-panel")
        .default_size(MODEL_WIDTH.1)
        .size_range(width_range(window, MODEL_WIDTH, MODEL_SHARE))
        .show(ui, |ui| {
            ui.add_enabled_ui(!modal, |ui| {
                scrolled(ui, "ferritecad model scroll", |ui| {
                    model(ui, view, &mut answers);
                });
            });
        });

    egui::Panel::right("actions-panel")
        .default_size(ACTIONS_WIDTH.1)
        .size_range(width_range(window, ACTIONS_WIDTH, ACTIONS_SHARE))
        .show(ui, |ui| {
            scrolled(ui, "ferritecad actions scroll", |ui| {
                actions(ui, view, &mut answers);
            });
        });

    // What no panel covers. Deliberately not a panel of its own: see the module
    // documentation.
    answers.central = ui.available_rect_before_wrap();
    ui.ctx()
        .data_mut(|data| data.insert_temp(central_id(), answers.central));
    answers
}

/// The Model panel's contents.
fn model(ui: &mut egui::Ui, view: &mut View<'_>, answers: &mut Answers) {
    let available = view.sections.model_available;
    let Some((tab, navigation)) = &mut view.sections.model else {
        ui.heading("Model");
        ui.label("No document is open.");
        return;
    };
    let source = view.scene.edit_source;
    let rows: Vec<_> = source
        .map(|source| {
            source
                .model_tree
                .rows
                .iter()
                .map(|row| ModelRow {
                    key: row.id,
                    parent: row.parent,
                    depth: row.depth,
                    name: &row.name,
                    kind: &row.kind,
                    note: &row.note,
                })
                .collect()
        })
        .unwrap_or_default();
    answers.chosen.model = ferritecad_ui::model_panel(
        ui,
        *tab,
        &rows,
        navigation,
        |key| {
            source
                .map(|source| model_tree::actions(source, key))
                .unwrap_or_default()
        },
        available,
    );
}

/// The Actions and Details panel's contents.
fn actions(ui: &mut egui::Ui, view: &mut View<'_>, answers: &mut Answers) {
    let activity = view.activity;
    let forms_scope = view.sections.forms_scope;
    let sections = &mut view.sections;
    let scene = &view.scene;

    if sections.closing_form {
        // Only the question. The rest of the window is drawn disabled, and its
        // forms are not drawn at all: they are not what is being asked.
        ui.heading("Question");
        answers.chosen.close_form = if let Some(losing) = sections.quit_new {
            ferritecad_ui::quit_new_panel(ui, losing)
        } else {
            ferritecad_ui::close_form_panel(ui, sections.close_form)
        };
        ui.label(sections.document_outcome);
        if activity.can_cancel_document && ui.button("Cancel operation").clicked() {
            answers.chosen.cancel_document = true;
        }
        return;
    }

    ui.heading("Actions");

    // What needs an answer, then what the last operations said.
    group(ui, "questions", |ui| {
        let stl = ferritecad_ui::stl_export_form(ui, sections.stl_form.as_deref_mut());
        if stl != ferritecad_ui::StlChoice::Waiting {
            answers.chosen.stl = stl;
        }
        answers.replace = ferritecad_ui::replace_confirmation(ui, sections.replacing);
        (answers.asked, answers.chosen.open_over_new) = ferritecad_ui::new_document_section(
            ui,
            sections.form.as_deref_mut(),
            sections.open_over_new,
        );
    });
    group(ui, "outcomes", |ui| {
        if let Some(message) = sections.dialog_failure {
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
        ferritecad_ui::create_panel(ui, sections.created);
        ferritecad_ui::export_panel(ui, sections.export);
        ferritecad_ui::open_failure_panel(ui, sections.failure);
    });

    // Getting work back: the last window's files, crash copies, checkpoints.
    group(ui, "recovery", |ui| {
        answers.chosen.reopen = ferritecad_ui::reopen_panel(ui, sections.reopen);
        answers.chosen.recovery = ferritecad_ui::recovery_panel(ui, sections.recovery);
        if let Some((panel, name)) = &mut sections.checkpoints {
            answers.chosen.checkpoint = ferritecad_ui::checkpoint_panel(ui, *panel, name);
        }
    });

    // Every route that makes or changes a document. The forms themselves float
    // in front of the viewport; their buttons are here.
    group(ui, "editors", |ui| {
        if sections.held_back {
            ui.label(
                "Copy workflows require a saved document with no unsaved changes. \
                 Height, vertices, constraints, Circle, annulus, a saved partial Revolve \
                 angle, existing Cut parameters, Fillet radii and Chamfer distances can \
                 still be changed with Apply, and a new Cut, Fillet or Chamfer added with \
                 Add cut, Add fillet or Add chamfer.",
            );
        }
        in_tab_scope(ui, "tab-forms", forms_scope, |ui| {
            sections.sketch.draw_choices(
                ui,
                sections.can_edit,
                scene.document,
                scene.edit_source,
                scene.sketches,
            );
            sections.sketch.draw(
                ui,
                sections.can_new,
                sections.creating || sections.edits.running(),
                sections.document_outcome,
            );
        });
        ui.separator();
        let unavailable = scene
            .edit_source
            .map(ExtrudeEditSource::unavailable_reason)
            .unwrap_or(Some("Open a document to edit an extrusion."));
        answers.chosen.edit = in_tab_scope(ui, "tab-height", forms_scope, |ui| {
            sections
                .edits
                .draw_with(ui, sections.can_edit, unavailable, sections.height)
        });
    });

    // What is in the picture and what is chosen in it.
    ui.separator();
    ui.heading("Details");
    group(ui, "details", |ui| {
        // Every definition in the picture, whether or not any of it is on
        // screen: a part hidden behind another, too small to hit or out of
        // shot is reachable here and nowhere else.
        let rows =
            ferritecad_ui::definitions_panel(ui, scene.definitions, scene.chosen_row, scene.offers);
        answers.chosen.definition = rows.pressed;
        answers.chosen.row_visibility = rows.visibility;
        answers.pointed_row = rows.hovered;
        ui.separator();
        // Read-only, and the only place the choice is described.
        ferritecad_ui::selection_inspector(ui, scene.described);
        ui.separator();
        // What the rebuild behind this picture found out about the drawings it
        // was built from. Not an inspector and not part of the choice.
        ferritecad_ui::sketch_solves_panel(ui, scene.solves);
    });
}

/// The production layout with nothing in it to say: a window with no document,
/// no tabs and no messages. For the gates elsewhere that need the viewport's
/// rectangle exactly as the window finds it, without owners to build one from.
#[cfg(test)]
pub(crate) fn quiet_layout(
    ctx: &egui::Context,
    screen: (f32, f32),
    pixels_per_point: f32,
) -> Answers {
    let mut creates = crate::creates::Creates::default();
    let mut edits = crate::edits::Edits::default();
    let (form, sketch) = creates.forms();
    let sections = Sections {
        dialog_failure: None,
        sketch,
        creating: false,
        edits: &mut edits,
        stl_form: None,
        can_edit: false,
        can_new: true,
        height: ferritecad_ui::HeightState {
            running: false,
            can_cancel: false,
            apply: false,
            copy: false,
            unsaved: false,
        },
        held_back: false,
        document_outcome: "",
        failure: None,
        export: None,
        replacing: None,
        form,
        open_over_new: None,
        closing_form: false,
        close_form: None,
        quit_new: None,
        created: None,
        reopen: Default::default(),
        recovery: Default::default(),
        checkpoints: None,
        tabs: ferritecad_ui::TabStrip {
            tabs: &[],
            available: Ok(()),
        },
        forms_scope: 0,
        model: None,
        model_available: Ok(()),
    };
    let mut view = View {
        activity: Activity {
            line: "Ready",
            can_open: true,
            ..Activity::default()
        },
        sections,
        scene: Scene {
            document: None,
            edit_source: None,
            sketches: &Accepted::default(),
            definitions: &[],
            chosen_row: None,
            offers: &[],
            described: None,
            solves: &[],
        },
    };
    ctx.set_pixels_per_point(pixels_per_point);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(screen.0, screen.1),
        )),
        ..Default::default()
    };
    let mut answers = Answers::default();
    // A few frames, for the panels to find their sizes as they do in a window.
    for _ in 0..4 {
        let mut out = ctx.run_ui(raw.clone(), |ui| answers = compose(ui, &mut view));
        out.textures_delta.clear();
    }
    answers
}
