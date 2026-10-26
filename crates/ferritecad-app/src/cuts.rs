// SPDX-License-Identifier: MIT
//! Draft fields over stored geometry; the shared transaction judges Apply.
use crate::feature_facts::{CircularCut, CutChoice, CutExtent, CutParameterChoice};
use crate::sketch_facts::Source as ExtrudeEditSource;
use ferritecad_document::DocumentVersion;
use ferritecad_jobs::EditRequest;
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
    /// The explicit end. Part of the same applied state as the numbers, so one
    /// Apply and one Undo cover it; the depth text is kept while it is unused.
    through_all: bool,
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
    Add(Box<CutChoice>),
    /// Boxed: a saved cut carries every identity it keeps, and an enum sized
    /// for it would make the ordinary add draft carry the difference.
    Edit(Box<CutParameterChoice>),
}

impl Subject {
    fn changes(&self, cut: CircularCut) -> Result<Vec<ferritecad_document::EditChange>> {
        match self {
            Self::Add(c) => cut.additions(c.target.as_ref().expect("form has target")),
            Self::Edit(c) => cut.changes(c.saved.as_ref().expect("form has saved cut")),
        }
    }
    fn validate(&self, typed: &Numbers) -> Result<()> {
        self.changes(numbers(typed)?).map(|_| ())
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

impl Draft {
    fn request(&self, cut: CircularCut) -> EditRequest {
        EditRequest {
            source: self.source.clone(),
            expected: self.version,
            changes: self.subject.changes(cut).expect("validated draft"),
            destination: PathBuf::new(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<Draft>,
    pending: Option<EditRequest>,
    /// The edit's own pending request beside the addition's. Two fields rather
    /// than one enum, so the window keeps handing each operation to the worker
    /// that already knows what to do with it.
    pending_edit: Option<EditRequest>,
    pending_apply: Option<EditRequest>,
    /// §30I: the new Cut the Add form asked to add to the open document.
    pending_add: Option<EditRequest>,
    can_begin_edit: bool,
    can_apply: bool,
    /// Whether the window's `can_add_cut` allows the Add form's own Add now.
    can_add: bool,
    unsaved: bool,
    outcome: String,
}

impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some()
    }
    pub(crate) fn dismiss(&mut self) {
        let availability = (self.can_begin_edit, self.can_apply, self.unsaved);
        let add = self.can_add;
        *self = Self::default();
        self.set_session(availability.0, availability.1, availability.2);
        self.can_add = add;
    }
    pub(crate) fn take_request(&mut self) -> Option<EditRequest> {
        self.pending.take()
    }
    pub(crate) fn take_edit_request(&mut self) -> Option<EditRequest> {
        self.pending_edit.take()
    }

    pub(crate) fn editing_saved(&self) -> bool {
        self.draft
            .as_ref()
            .is_some_and(|d| matches!(d.subject, Subject::Edit(_)))
    }
    pub(crate) fn set_session(&mut self, can_begin: bool, can_apply: bool, unsaved: bool) {
        self.can_begin_edit = can_begin;
        self.can_apply = can_apply;
        self.unsaved = unsaved;
    }
    pub(crate) fn session(&self) -> (bool, bool, bool) {
        (self.can_begin_edit, self.can_apply, self.unsaved)
    }
    pub(crate) fn set_outcome(&mut self, outcome: &str) {
        self.outcome = outcome.to_owned();
    }
    pub(crate) fn take_apply_request(&mut self) -> Option<EditRequest> {
        self.pending_apply.take()
    }
    /// §30I: the Add form is open (not the edit of a saved Cut).
    pub(crate) fn adding(&self) -> bool {
        self.draft
            .as_ref()
            .is_some_and(|d| matches!(d.subject, Subject::Add(_)))
    }
    /// What the window's `can_add_cut` says, each frame.
    pub(crate) fn set_add(&mut self, can_add: bool) {
        self.can_add = can_add;
    }
    pub(crate) fn add_session(&self) -> bool {
        self.can_add
    }
    /// §30I: the new Cut the user asked to add to the open document.
    pub(crate) fn take_add_request(&mut self) -> Option<EditRequest> {
        self.pending_add.take()
    }

    pub(crate) fn begin(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        body: ObjectId,
    ) -> bool {
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
            Subject::Add(Box::new(choice.clone())),
            Numbers::default(),
        )
    }

    /// Begin changing the numbers of one saved cut.
    ///
    /// The form opens on what the document stores, so the first thing it shows
    /// is the cut as it is rather than an empty box beside the word "Depth".
    pub(crate) fn begin_edit(
        &mut self,
        path: &Path,
        source: &ExtrudeEditSource,
        feature: ObjectId,
    ) -> bool {
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
            depth: saved
                .extent
                .blind_depth_mm()
                .map(|d| d.to_string())
                .unwrap_or_default(),
            through_all: saved.extent == CutExtent::ThroughAll,
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
        egui::ScrollArea::vertical()
            .id_salt("cut-choices")
            .max_height(190.)
            .show(ui, |ui| {
                for choice in &source.cut_bodies {
                    let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
                    // §30I: the Add form adds into the open document, so it may be
                    // opened with unsaved changes when the session is idle.
                    let response = ui.add_enabled(
                        (can_begin || self.can_begin_edit) && refusal.is_none(),
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
                // extrusion that started the body, whose distance `edit with extrude_distance` owns.
                for choice in &source.cut_features {
                    let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
                    let response = ui.add_enabled(
                        (can_begin || self.can_begin_edit) && refusal.is_none(),
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
            });
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
            Subject::Add(_) => "Add circular cut",
            Subject::Edit(_) => "Edit circular cut",
        };
        crate::form_window(ui, title)
            .default_width(560.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                if !self.outcome.is_empty() {
                    ui.separator();
                    ui.label(format!("Document: {}", self.outcome));
                }
                ui.label(
                    "The tool is drawn on the part's own base XY plane and cuts along that \
                     plane's normal (+Z).",
                );
                ui.small(format!(
                    "Body {} · plane {} · {}",
                    shown.body,
                    shown.plane,
                    crate::sessions::shown_as(&draft.source)
                ));
                ui.small(format!(
                    "Part {}, {} mm tall; the cut modifies feature {}",
                    part_outline(&shown.boundary),
                    shown.height_mm,
                    shown.modifies,
                ));
                ui.small("Apply rebuilds the Cut and reports any failed features or lost names.");
                match &shown.editing {
                    None => {
                        ui.label(
                            "A Blind depth equal to the part's height cuts through it; less \
                             leaves a pocket opening on the base side. Through all stays \
                             through whatever height the part is later given.",
                        );
                    }
                    Some(editing) => {
                        ui.small(format!(
                            "Editing cut {} · tool sketch {} · circle {}",
                            editing.feature, editing.tool_sketch, editing.tool_curve
                        ));
                        ui.small(format!(
                            "Saved: circle ({}, {}) r{} {} — {}",
                            editing.center_mm[0],
                            editing.center_mm[1],
                            editing.radius_mm,
                            describe(editing.extent),
                            if editing.leaves_a_floor {
                                "a pocket with a floor"
                            } else {
                                "a hole through the part"
                            },
                        ));

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
                    ui.horizontal(|ui| {
                        ui.label("End:");
                        ui.radio_value(&mut draft.typed.through_all, false, "Blind depth");
                        ui.radio_value(&mut draft.typed.through_all, true, "Through all");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Depth (mm):");
                        ui.add_enabled(
                            !draft.typed.through_all,
                            egui::TextEdit::singleline(&mut draft.typed.depth)
                                .id_salt("Depth (mm):")
                                .char_limit(32)
                                .desired_width(110.),
                        );
                    });
                    if draft.typed.through_all {
                        ui.small(
                            "Through all has no depth: its length follows the target along the \
                             sketch normal at every rebuild.",
                        );
                    }
                    if let Some(editing) = &shown.editing {
                        let parsed = numbers(&draft.typed);
                        let differs = parsed.as_ref().is_ok_and(|c| {
                            c.center_mm != editing.center_mm || c.radius_mm != editing.radius_mm
                                || c.extent != editing.extent
                        });
                        let valid = draft.subject.validate(&draft.typed);
                        if ui.add_enabled(self.can_apply && differs && valid.is_ok(),
                            egui::Button::new("Apply cut")).clicked() {
                            let cut = parsed.expect("validated fields");
                            self.pending_apply = Some(draft.request(cut));
                        }
                        if let Err(error) = valid {
                            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                        } else if !differs {
                            ui.small("These are the stored numbers and end: there is nothing to apply.");
                        }
                    }
                    if shown.editing.is_none() {
                        // §30I: into the open document, no file name; the current valid
                        // fields, whether or not the draft was confirmed.
                        let valid = draft.subject.validate(&draft.typed);
                        if ui
                            .add_enabled(self.can_add && valid.is_ok(), egui::Button::new("Add cut"))
                            .clicked()
                        {
                            let cut = numbers(&draft.typed).expect("validated fields");
                            self.pending_add = Some(draft.request(cut));
                        }
                        if let Err(error) = valid {
                            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                        }
                    }
                    if ui.button("Confirm draft numbers").clicked() {
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
                        egui::ScrollArea::vertical()
                            .id_salt("cut-refusal")
                            .max_height(72.)
                            .show(ui, |ui| {
                                ui.colored_label(ui.visuals().error_fg_color, refusal);
                            });
                    }
                    let confirmed = draft
                        .applied
                        .as_ref()
                        .filter(|applied| **applied == draft.typed)
                        .and_then(|applied| numbers(applied).ok());
                    match confirmed {
                        Some(cut) => {
                            ui.small(format!(
                                "Ready: circle ({}, {}) r{} {}",
                                cut.center_mm[0],
                                cut.center_mm[1],
                                cut.radius_mm,
                                describe(cut.extent)
                            ));
                            if ui.add_enabled(!self.unsaved, egui::Button::new("Save cut copy…")).clicked() {
                                match &shown.editing {
                                    None => {
                                        self.pending = Some(draft.request(cut))
                                    }
                                    // The saved identity, never the text of a
                                    // field: the form sends back the curve the
                                    // document told it about.
                                    Some(_) => {
                                        self.pending_edit = Some(draft.request(cut))
                                    }
                                }
                            }
                        }
                        None => {
                            ui.small("Confirm the draft numbers and end before saving a copy.");
                        }
                    }
                });
                if self.unsaved {
                    ui.small("Saving a copy is unavailable while the document has unsaved changes: Save or Undo them first. Add cut and Apply cut change this document; the file on disk changes only on Save.");
                }
                if running {
                    ui.label("Working… Draft retained until acceptance. Cancel job in toolbar.");
                }
            });
        if cancel {
            self.dismiss();
        }
    }
}

/// The part as the catalogue read it. A rectangle is named by its size; any
/// other outline by its Lines and area, with the box around it named as
/// bounds and never as the part.
pub(crate) fn part_outline(boundary: &[[f64; 2]]) -> String {
    format!("stored outline of {} Lines", boundary.len())
}

/// What the open form says about the part, whichever operation it is for.
///
/// Read out of the catalogue the accepted reading produced, never assumed and
/// never recomputed here.
#[derive(Debug, Clone)]
struct Shown {
    body: ObjectId,
    plane: ObjectId,
    boundary: Vec<[f64; 2]>,
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
    extent: CutExtent,
    leaves_a_floor: bool,
}

impl Subject {
    fn shown(&self) -> Option<Shown> {
        match self {
            Self::Add(choice) => {
                let target = choice.target.as_ref()?;
                Some(Shown {
                    body: target.body,
                    plane: target.plane,
                    boundary: target.boundary.clone(),
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
                    boundary: saved.boundary.clone(),
                    height_mm: saved.height_mm,
                    modifies: saved.previous_feature,
                    editing: Some(Editing {
                        feature: saved.feature,
                        tool_sketch: saved.tool_sketch,
                        tool_curve: saved.tool_curve,
                        center_mm: saved.center_mm,
                        radius_mm: saved.radius_mm,
                        extent: saved.extent,
                        leaves_a_floor: saved
                            .extent
                            .blind_depth_mm()
                            .is_some_and(|d| d < saved.height_mm),
                    }),
                })
            }
        }
    }
}

/// How a form names one Cut's end: a depth, or through all.
fn describe(extent: CutExtent) -> String {
    match extent {
        CutExtent::Blind { depth_mm } => format!("cut {depth_mm} mm deep"),
        CutExtent::ThroughAll => "cut through all".to_owned(),
    }
}

/// The boxes and the end as one request, or the first reason they are not one.
///
/// Parsed here and judged nowhere else: what a number that parses may be is the
/// document's rule, asked through `validate_cut`. The depth box is not read at
/// all for Through all, so text left in it can neither refuse nor leak in.
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
        extent: if typed.through_all {
            CutExtent::ThroughAll
        } else {
            CutExtent::Blind {
                depth_mm: read(&typed.depth, "cut depth")?,
            }
        },
    })
}

#[cfg(test)]
#[allow(clippy::panic)]
pub(crate) mod tests {
    pub(crate) mod add_session;
    pub(crate) mod session_apply;
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
    pub(crate) fn frame(ctx: &egui::Context, e: &mut Editor, running: bool) -> egui::FullOutput {
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
    pub(crate) fn click(ctx: &egui::Context, e: &mut Editor, label: &str) {
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
    pub(crate) fn enter_field(ctx: &egui::Context, e: &mut Editor, label: &str, value: &str) {
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

    fn fill(ctx: &egui::Context, e: &mut Editor, x: &str, y: &str, r: &str, depth: &str) {
        enter_field(ctx, e, "Centre X (mm):", x);
        enter_field(ctx, e, "Centre Y (mm):", y);
        enter_field(ctx, e, "Radius (mm):", r);
        enter_field(ctx, e, "Depth (mm):", depth);
    }
}
