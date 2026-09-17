// SPDX-License-Identifier: MIT
//! Disposable Line constraint request over stored facts; no solver or file reads.
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, CircleConstraintKind, CircleRadiusMm,
    ConstraintSketchChoice, DocumentVersion, ExtrudeEditSource, LineConstraintKind, LineEndpoint,
    LineLengthMm, LineRelation, Sketch, SketchConstraintEdits, SketchConstraintRule,
    SketchCoordinateMm, SketchGeometry,
};
use ferritecad_jobs::{EditSketchConstraintsRequest, EditedSketchConstraints};
use ferritecad_types::{ObjectId, Result, StableEntityId};
use std::path::{Path, PathBuf};

const HISTORY_LIMIT: usize = 128;

/// Only pending requests belong to history, never selection or solver results.
#[derive(Debug, Clone, Default)]
struct History {
    undo: Vec<SketchConstraintEdits>,
    redo: Vec<SketchConstraintEdits>,
}
impl History {
    fn push(stack: &mut Vec<SketchConstraintEdits>, edits: SketchConstraintEdits) {
        if stack.len() == HISTORY_LIMIT {
            stack.remove(0);
        }
        stack.push(edits);
    }
    fn change(&mut self, edits: &mut SketchConstraintEdits, next: SketchConstraintEdits) {
        if *edits != next {
            Self::push(&mut self.undo, std::mem::replace(edits, next));
            self.redo.clear();
        }
    }
    fn undo(&mut self, edits: &mut SketchConstraintEdits) {
        if let Some(previous) = self.undo.pop() {
            Self::push(&mut self.redo, std::mem::replace(edits, previous));
        }
    }
    fn redo(&mut self, edits: &mut SketchConstraintEdits) {
        if let Some(next) = self.redo.pop() {
            Self::push(&mut self.undo, std::mem::replace(edits, next));
        }
    }
}

#[derive(Debug, Clone)]
struct Draft {
    source: PathBuf,
    version: DocumentVersion,
    choice: ConstraintSketchChoice,
    selected: Option<StableEntityId>,
    // Unapplied input, like selection, is not part of request Undo/Redo.
    length_mm: String,
    /// The circle radius being typed, for a profile that is one analytic
    /// circle. Its own field beside `length_mm`: a Line length and a circle
    /// radius are different dimensions of different geometry, and one box for
    /// both would carry a number from one family into the other.
    radius_mm: String,
    endpoint: LineEndpoint,
    pin_x_mm: String,
    pin_y_mm: String,
    // The pair both relationship families use is two explicit picks from the
    // same stored catalogue; there is no second list to keep in step with it.
    pair_a: Option<StableEntityId>,
    pair_b: Option<StableEntityId>,
    edits: SketchConstraintEdits,
    history: History,
    refusal: Option<String>,
}
#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<Draft>,
    pending: Option<EditSketchConstraintsRequest>,
}
impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some()
    }
    pub(crate) fn dismiss(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn take_request(&mut self) -> Option<EditSketchConstraintsRequest> {
        self.pending.take()
    }
    fn begin(&mut self, path: &Path, source: &ExtrudeEditSource, id: ObjectId) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .constraint_sketches
            .iter()
            .find(|c| c.sketch == id && c.refusal.is_none())
        else {
            return false;
        };
        self.draft = Some(Draft {
            source: path.to_path_buf(),
            version: source.version,
            choice: choice.clone(),
            selected: None,
            length_mm: String::new(),
            radius_mm: String::new(),
            endpoint: LineEndpoint::Start,
            pin_x_mm: String::new(),
            pin_y_mm: String::new(),
            pair_a: None,
            pair_b: None,
            edits: Default::default(),
            history: Default::default(),
            refusal: None,
        });
        true
    }
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
        if let (Some(path), Some(source)) = (path, source) {
            for choice in &source.constraint_sketches {
                let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
                let response = ui.add_enabled(
                    can_begin && refusal.is_none(),
                    egui::Button::new(format!(
                        "Edit constraints {} — {}…",
                        choice.name.as_deref().unwrap_or("Unnamed"),
                        choice.sketch
                    )),
                );
                if response.clicked() {
                    self.begin(path, source, choice.sketch);
                }
                if let Some(reason) = refusal {
                    response.on_hover_text(reason);
                }
            }
        }
    }
    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, running: bool) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        let mut cancel = false;
        egui::Window::new("Sketch constraints — new copy")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(
                    "Select a stored Line or Circle. Solver runs only when saving \
                     the new copy.",
                );
                ui.small(format!(
                    "Sketch {} · {}",
                    draft.choice.sketch,
                    draft.source.display()
                ));
                ui.label("Coordinates below are stored inputs, not the solved drawing.");
                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Cancel constraints draft").clicked() {
                            cancel = true;
                        }
                        if ui
                            .add_enabled(!draft.history.undo.is_empty(), egui::Button::new("Undo"))
                            .clicked()
                        {
                            draft.history.undo(&mut draft.edits);
                            draft.refusal = None;
                        }
                        if ui
                            .add_enabled(!draft.history.redo.is_empty(), egui::Button::new("Redo"))
                            .clicked()
                        {
                            draft.history.redo(&mut draft.edits);
                            draft.refusal = None;
                        }
                    });
                    let Some(stored) = &draft.choice.stored else {
                        return;
                    };
                    if stored.curves.iter().any(|c| matches!(c.geometry, SketchGeometry::Line { .. })) {
                        ui.label(
                            "Missing Coincident joints are added with constraints; closure remains after removal.",
                        );
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("constraint-lines")
                        .max_height(150.)
                        .show(ui, |ui| {
                            for (i, c) in stored.curves.iter().enumerate() {
                                if let SketchGeometry::Line { start, end } = c.geometry {
                                    ui.selectable_value(
                                        &mut draft.selected,
                                        Some(c.id),
                                        format!(
                                            "Segment {} · {} · ({}, {}) → ({}, {}) mm",
                                            i + 1,
                                            short(c.id),
                                            start.x,
                                            start.y,
                                            end.x,
                                            end.y
                                        ),
                                    );
                                }
                            }
                        });
                    // One analytic circle, offered as itself. No entry above
                    // describes it — it has no start and no end — so it gets
                    // its own row naming its UUID and the two numbers the
                    // document stores as the solver's starting guess.
                    let circles: Vec<_> = stored
                        .curves
                        .iter()
                        .filter_map(|c| match c.geometry {
                            SketchGeometry::Circle { center, radius } => {
                                Some((c.id, center, radius))
                            }
                            _ => None,
                        })
                        .collect();
                    // Which circle bounds the part and which is the bore, asked
                    // of the document rather than worked out here: the roles
                    // every request and every refusal is about have one source.
                    let roles = ferritecad_document::constraint_circle_roles(stored);
                    for (id, center, radius) in &circles {
                        ui.selectable_value(
                            &mut draft.selected,
                            Some(*id),
                            format!(
                                "{} · {} · centre ({}, {}) mm · radius {} mm",
                                role_name(roles, *id), short(*id), center.x, center.y, radius
                            ),
                        );
                    }
                    // Concentricity needs two circles to relate, so it is
                    // offered only where there are two. Both are picked
                    // explicitly and neither leads, exactly as the Line pair is.
                    if circles.len() == 2 {
                        ui.horizontal(|ui| {
                            ui.label("Circle pair:");
                            let picked = draft.selected.is_some();
                            if ui.add_enabled(picked, egui::Button::new("Pair Circle A")).clicked() {
                                draft.pair_a = draft.selected;
                            }
                            if ui.add_enabled(picked, egui::Button::new("Pair Circle B")).clicked() {
                                draft.pair_b = draft.selected;
                            }
                            ui.small(format!(
                                "Pair: A {} · B {}",
                                picked_line(draft.pair_a),
                                picked_line(draft.pair_b)
                            ));
                            let both = draft.pair_a.is_some() && draft.pair_b.is_some();
                            if ui.add_enabled(both, egui::Button::new("Add Concentric")).clicked() {
                                let mut proposed = draft.edits.clone();
                                proposed.add.push(AddSketchConstraint::Concentric {
                                    a: draft.pair_a.expect("Circle A"),
                                    b: draft.pair_b.expect("Circle B"),
                                });
                                match draft.choice.validate_edits(&proposed) {
                                    Ok(()) => {
                                        draft.history.change(&mut draft.edits, proposed);
                                        draft.refusal = None;
                                    }
                                    Err(e) => draft.refusal = Some(e.to_string()),
                                }
                            }
                        });
                    }
                    if !circles.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label("Radius mm");
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.radius_mm)
                                    .char_limit(32)
                                    .desired_width(90.),
                            );
                            if ui
                                .add_enabled(
                                    draft.selected.is_some(),
                                    egui::Button::new("Add radius"),
                                )
                                .clicked()
                            {
                                // Parsed here and judged nowhere else: what a
                                // number that parses may be is the document's
                                // rule, asked through `validate_edits` below.
                                match draft
                                    .radius_mm
                                    .trim()
                                    .parse::<f64>()
                                    .ok()
                                    .and_then(|v| CircleRadiusMm::new(v).ok())
                                {
                                    Some(radius) => {
                                        let mut proposed = draft.edits.clone();
                                        proposed.add.push(AddSketchConstraint::Circle {
                                            curve: draft.selected.expect("selected"),
                                            kind: CircleConstraintKind::Radius(radius),
                                        });
                                        match draft.choice.validate_edits(&proposed) {
                                            Ok(()) => {
                                                draft.history.change(&mut draft.edits, proposed);
                                                draft.refusal = None;
                                            }
                                            Err(e) => draft.refusal = Some(e.to_string()),
                                        }
                                    }
                                    None => {
                                        draft.refusal =
                                            Some("Enter a positive radius in mm.".to_owned());
                                    }
                                }
                            }
                            if ui
                                .add_enabled(
                                    draft.selected.is_some(),
                                    egui::Button::new("Add Fixed centre"),
                                )
                                .clicked()
                            {
                                match (
                                    draft.pin_x_mm.trim().parse::<f64>().ok(),
                                    draft.pin_y_mm.trim().parse::<f64>().ok(),
                                ) {
                                    (Some(x), Some(y)) => {
                                        match (
                                            SketchCoordinateMm::new(x),
                                            SketchCoordinateMm::new(y),
                                        ) {
                                            (Ok(x), Ok(y)) => {
                                                let mut proposed = draft.edits.clone();
                                                proposed.add.push(
                                                    AddSketchConstraint::Circle {
                                                        curve: draft.selected.expect("selected"),
                                                        kind:
                                                            CircleConstraintKind::FixedCenter {
                                                                x,
                                                                y,
                                                            },
                                                    },
                                                );
                                                match draft.choice.validate_edits(&proposed) {
                                                    Ok(()) => {
                                                        draft
                                                            .history
                                                            .change(&mut draft.edits, proposed);
                                                        draft.refusal = None;
                                                    }
                                                    Err(e) => {
                                                        draft.refusal = Some(e.to_string())
                                                    }
                                                }
                                            }
                                            _ => {
                                                draft.refusal = Some(
                                                    "Enter finite centre coordinates in mm."
                                                        .to_owned(),
                                                )
                                            }
                                        }
                                    }
                                    _ => {
                                        draft.refusal = Some(
                                            "Enter the centre X and Y in mm below.".to_owned(),
                                        )
                                    }
                                }
                            }
                        });
                    }
                    if circles.is_empty() {
                    ui.horizontal(|ui| {
                        for (label, kind) in [
                            ("Add Horizontal", LineConstraintKind::Horizontal),
                            ("Add Vertical", LineConstraintKind::Vertical),
                        ] {
                            if ui
                                .add_enabled(draft.selected.is_some(), egui::Button::new(label))
                                .clicked()
                            {
                                let mut proposed = draft.edits.clone();
                                proposed.add.push(AddSketchConstraint::Line(AddLineConstraint::Line {
                                    curve: draft.selected.expect("selected"),
                                    kind,
                                }));
                                match draft.choice.validate_edits(&proposed) {
                                    Ok(()) => {
                                        draft.history.change(&mut draft.edits, proposed);
                                        draft.refusal = None;
                                    }
                                    Err(e) => draft.refusal = Some(e.to_string()),
                                }
                            }
                        }
                    });
                    let saved_length = draft
                        .selected
                        .and_then(|curve| stored_line_length(stored, curve));
                    ui.horizontal(|ui| {
                        ui.label("Line length (mm):");
                        ui.add(egui::TextEdit::singleline(&mut draft.length_mm)
                            .id_salt("line-length-mm").desired_width(100.));
                        if ui.add_enabled(draft.selected.is_some(), egui::Button::new("Add length")).clicked() {
                            let length = draft.length_mm.trim().parse::<f64>()
                                .map_err(|_| ferritecad_types::CadError::input("enter a finite positive Line length in mm"))
                                .and_then(LineLengthMm::new);
                            match length.and_then(|length| {
                                let mut proposed = draft.edits.clone();
                                proposed.add.push(AddSketchConstraint::Line(AddLineConstraint::Line {
                                    curve: draft.selected.expect("selected"),
                                    kind: LineConstraintKind::Distance(length),
                                }));
                                draft.choice.validate_edits(&proposed)?;
                                Ok(proposed)
                            }) {
                                Ok(proposed) => {
                                    draft.history.change(&mut draft.edits, proposed);
                                    draft.refusal = None;
                                }
                                Err(e) => draft.refusal = Some(e.to_string()),
                            }
                        }
                        if ui
                            .add_enabled(saved_length.is_some(), egui::Button::new("Replace length"))
                            .clicked()
                        {
                            let (id, previous) = saved_length.expect("stored length");
                            match draft
                                .length_mm
                                .trim()
                                .parse::<f64>()
                                .map_err(|_| {
                                    ferritecad_types::CadError::input(
                                        "enter a finite positive Line length in mm",
                                    )
                                })
                                .and_then(LineLengthMm::new)
                                .and_then(|length| {
                                    let proposed = replace_stored_length(
                                        &draft.edits,
                                        draft.selected.expect("selected"),
                                        id,
                                        previous,
                                        length,
                                    );
                                    if proposed != draft.edits
                                        && !(proposed.remove.is_empty() && proposed.add.is_empty())
                                    {
                                        draft.choice.validate_edits(&proposed)?;
                                    }
                                    Ok(proposed)
                                })
                            {
                                Ok(proposed) => {
                                    draft.history.change(&mut draft.edits, proposed);
                                    draft.refusal = None;
                                }
                                Err(e) => draft.refusal = Some(e.to_string()),
                            }
                        }
                    });
                    if let Some((id, length)) = saved_length {
                        ui.small(format!("Stored length {} mm · {id}", length.get()));
                    }
                    ui.horizontal(|ui| {
                        ui.label("Pin endpoint:");
                        for endpoint in [LineEndpoint::Start, LineEndpoint::End] {
                            ui.selectable_value(
                                &mut draft.endpoint,
                                endpoint,
                                match endpoint {
                                    LineEndpoint::Start => "Pin Start",
                                    LineEndpoint::End => "Pin End",
                                },
                            );
                        }
                        // Stored inputs name the endpoint; the solved drawing is not read here.
                        if let Some((x, y)) = draft
                            .selected
                            .and_then(|curve| stored_endpoint(stored, curve, draft.endpoint))
                        {
                            ui.small(format!(
                                "Stored {} of the selected Line: ({x}, {y}) mm",
                                draft.endpoint.as_str()
                            ));
                        }
                    });
                    }
                    ui.horizontal(|ui| {
                        ui.label("Fixed X (mm):");
                        ui.add(egui::TextEdit::singleline(&mut draft.pin_x_mm)
                            .id_salt("fixed-x-mm").desired_width(80.));
                        ui.label("Fixed Y (mm):");
                        ui.add(egui::TextEdit::singleline(&mut draft.pin_y_mm)
                            .id_salt("fixed-y-mm").desired_width(80.));
                        if circles.is_empty() && ui
                            .add_enabled(draft.selected.is_some(), egui::Button::new("Add Fixed point"))
                            .clicked()
                        {
                            let x = coordinate(&draft.pin_x_mm);
                            let y = coordinate(&draft.pin_y_mm);
                            match x.and_then(|x| y.map(|y| (x, y))).and_then(|(x, y)| {
                                let mut proposed = draft.edits.clone();
                                proposed.add.push(AddSketchConstraint::Line(AddLineConstraint::Line {
                                    curve: draft.selected.expect("selected"),
                                    kind: LineConstraintKind::Fixed {
                                        at: draft.endpoint,
                                        x,
                                        y,
                                    },
                                }));
                                draft.choice.validate_edits(&proposed)?;
                                Ok(proposed)
                            }) {
                                Ok(proposed) => {
                                    draft.history.change(&mut draft.edits, proposed);
                                    draft.refusal = None;
                                }
                                Err(e) => draft.refusal = Some(e.to_string()),
                            }
                        }
                    });
                    if circles.is_empty() {
                    // Two explicit picks from the same stored list; neither leads.
                    ui.horizontal(|ui| {
                        ui.label("Line pair:");
                        let picked = draft.selected.is_some();
                        if ui
                            .add_enabled(picked, egui::Button::new("Pair Line A"))
                            .clicked()
                        {
                            draft.pair_a = draft.selected;
                        }
                        if ui
                            .add_enabled(picked, egui::Button::new("Pair Line B"))
                            .clicked()
                        {
                            draft.pair_b = draft.selected;
                        }
                        ui.small(format!(
                            "Pair: A {} · B {}",
                            picked_line(draft.pair_a),
                            picked_line(draft.pair_b)
                        ));
                    });
                    // One picked pair, three independent things it can be asked
                    // to keep: a length, and a relative orientation either way.
                    ui.horizontal(|ui| {
                        let both = draft.pair_a.is_some() && draft.pair_b.is_some();
                        for (label, make) in [
                            (
                                "Add Equal length",
                                None::<LineRelation>,
                            ),
                            ("Add Parallel", Some(LineRelation::Parallel)),
                            ("Add Perpendicular", Some(LineRelation::Perpendicular)),
                        ] {
                            if !ui.add_enabled(both, egui::Button::new(label)).clicked() {
                                continue;
                            }
                            let (a, b) = (
                                draft.pair_a.expect("Line A"),
                                draft.pair_b.expect("Line B"),
                            );
                            let mut proposed = draft.edits.clone();
                            proposed.add.push(AddSketchConstraint::Line(match make {
                                Some(relation) => AddLineConstraint::Relation { a, b, relation },
                                None => AddLineConstraint::EqualLength { a, b },
                            }));
                            match draft.choice.validate_edits(&proposed) {
                                Ok(()) => {
                                    draft.history.change(&mut draft.edits, proposed);
                                    draft.refusal = None;
                                }
                                Err(e) => draft.refusal = Some(e.to_string()),
                            }
                        }
                    });
                    }
                    ui.label("Persisted constraints:");
                    egui::ScrollArea::vertical()
                        .id_salt("stored-constraints")
                        .max_height(150.)
                        .show(ui, |ui| {
                            for c in &stored.constraints {
                                let (label, lines) = match c.rule {
                                    SketchConstraintRule::Horizontal { a, .. } => {
                                        ("Horizontal".to_owned(), Some(one_line(a.curve)))
                                    }
                                    SketchConstraintRule::Vertical { a, .. } => {
                                        ("Vertical".to_owned(), Some(one_line(a.curve)))
                                    }
                                    SketchConstraintRule::Distance { a, distance, .. } => {
                                        (format!("Line length {distance} mm"), Some(one_line(a.curve)))
                                    }
                                    SketchConstraintRule::Radius { curve, radius } => (
                                        format!("Radius {radius} mm"),
                                        Some(format!("Circle {curve}")),
                                    ),
                                    // A shared centre is the one Coincident a
                                    // reader may remove, and it says so; every
                                    // other Coincident is Line closure and
                                    // keeps its unremovable row.
                                    SketchConstraintRule::Coincident { a, b }
                                        if a.at == ferritecad_document::SketchPointSelector::Center
                                            && b.at
                                                == ferritecad_document::SketchPointSelector::Center
                                            && a.curve != b.curve => (
                                        "Concentric".to_owned(),
                                        Some(format!(
                                            "Circles {} and {}",
                                            short(a.curve),
                                            short(b.curve)
                                        )),
                                    ),
                                    SketchConstraintRule::Fixed { point, x, y }
                                        if point.at == ferritecad_document::SketchPointSelector::Center => (
                                        format!("Fixed centre ({x}, {y}) mm"),
                                        Some(format!("Circle {}", point.curve)),
                                    ),
                                    SketchConstraintRule::Fixed { point, x, y } => (
                                        format!("Fixed point {} ({x}, {y}) mm", point.at.as_str()),
                                        Some(one_line(point.curve)),
                                    ),
                                    SketchConstraintRule::EqualLength { a, b } => (
                                        "Equal length".to_owned(),
                                        Some(two_lines(a.from.curve, b.from.curve, "=")),
                                    ),
                                    SketchConstraintRule::Parallel { a, b } => (
                                        LineRelation::Parallel.as_str().to_owned(),
                                        Some(two_lines(a.from.curve, b.from.curve, "and")),
                                    ),
                                    SketchConstraintRule::Perpendicular { a, b } => (
                                        LineRelation::Perpendicular.as_str().to_owned(),
                                        Some(two_lines(a.from.curve, b.from.curve, "and")),
                                    ),
                                    _ => ("Coincident closure".to_owned(), None),
                                };
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("{label} · {}", c.id));
                                    if let Some(lines) = lines {
                                        ui.label(lines);
                                        let mut remove = draft.edits.remove.contains(&c.id);
                                        if ui.checkbox(&mut remove, "Remove").changed() {
                                            let mut proposed = draft.edits.clone();
                                            if remove {
                                                proposed.remove.push(c.id);
                                            } else {
                                                proposed.remove.retain(|id| *id != c.id);
                                            }
                                            draft.history.change(&mut draft.edits, proposed);
                                            draft.refusal = None;
                                        }
                                    }
                                });
                            }
                            if stored.constraints.is_empty() {
                                ui.label(if circles.is_empty() {
                                    "None. Closure links will be explicit in the saved copy."
                                } else {
                                    "None. The circle is already closed."
                                });
                            }
                        });
                    ui.label("Pending additions:");
                    egui::ScrollArea::vertical()
                        .id_salt("pending-constraints")
                        .max_height(100.)
                        .show(ui, |ui| {
                            for add in &draft.edits.add {
                                ui.label(addition_name(add));
                            }
                        });
                    if ui.button("Clear pending changes").clicked() {
                        draft.history.change(&mut draft.edits, Default::default());
                        draft.refusal = None;
                    }
                    if let Some(refusal) = &draft.refusal {
                        ui.colored_label(ui.visuals().error_fg_color, refusal);
                    }
                    match draft.choice.validate_edits(&draft.edits) {
                        Ok(()) => {
                            if ui.button("Save constraints copy…").clicked() {
                                self.pending = Some(EditSketchConstraintsRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,
                                    sketch: draft.choice.sketch,
                                    edits: draft.edits.clone(),
                                    destination: PathBuf::new(),
                                });
                            }
                        }
                        Err(e) => {
                            ui.colored_label(ui.visuals().error_fg_color, e.to_string());
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

fn stored_line_length(
    sketch: &Sketch,
    curve: StableEntityId,
) -> Option<(StableEntityId, LineLengthMm)> {
    sketch.constraints.iter().find_map(|c| match c.rule {
        SketchConstraintRule::Distance { a, b, distance }
            if a.curve == curve && b.curve == curve =>
        {
            LineLengthMm::new(distance)
                .ok()
                .map(|length| (c.id, length))
        }
        _ => None,
    })
}

fn line_distance_on(add: &AddSketchConstraint, curve: StableEntityId) -> bool {
    matches!(
        *add,
        AddSketchConstraint::Line(AddLineConstraint::Line {
            curve: on,
            kind: LineConstraintKind::Distance(_)
        }) if on == curve
    )
}

fn replace_stored_length(
    edits: &SketchConstraintEdits,
    curve: StableEntityId,
    stored: StableEntityId,
    previous: LineLengthMm,
    length: LineLengthMm,
) -> SketchConstraintEdits {
    let mut proposed = edits.clone();
    if length == previous {
        proposed.remove.retain(|id| *id != stored);
        proposed.add.retain(|add| !line_distance_on(add, curve));
        return proposed;
    }
    if !proposed.remove.contains(&stored) {
        proposed.remove.push(stored);
    }
    let addition = AddSketchConstraint::Line(AddLineConstraint::Line {
        curve,
        kind: LineConstraintKind::Distance(length),
    });
    match proposed
        .add
        .iter()
        .position(|add| line_distance_on(add, curve))
    {
        Some(i) => proposed.add[i] = addition,
        None => proposed.add.push(addition),
    }
    proposed
}

/// The pin's stored endpoint, so the choice needs no solved drawing.
fn stored_endpoint(sketch: &Sketch, curve: StableEntityId, at: LineEndpoint) -> Option<(f64, f64)> {
    sketch.curves.iter().find(|c| c.id == curve).and_then(|c| {
        let SketchGeometry::Line { start, end } = c.geometry else {
            return None;
        };
        let p = match at {
            LineEndpoint::Start => start,
            LineEndpoint::End => end,
        };
        Some((p.x, p.y))
    })
}

fn coordinate(text: &str) -> Result<SketchCoordinateMm> {
    text.trim()
        .parse::<f64>()
        .map_err(|_| {
            ferritecad_types::CadError::input("enter finite X and Y pin coordinates in mm")
        })
        .and_then(SketchCoordinateMm::new)
}

fn short(id: StableEntityId) -> String {
    id.to_string()[24..].to_owned()
}
fn one_line(curve: StableEntityId) -> String {
    format!("Line {}", short(curve))
}
fn two_lines(a: StableEntityId, b: StableEntityId, joins: &str) -> String {
    format!("Lines {} {joins} {}", short(a), short(b))
}
fn picked_line(id: Option<StableEntityId>) -> String {
    id.map_or_else(|| "none".to_owned(), short)
}
fn kind_name(kind: LineConstraintKind) -> String {
    match kind {
        LineConstraintKind::Horizontal => "Horizontal".into(),
        LineConstraintKind::Vertical => "Vertical".into(),
        LineConstraintKind::Distance(length) => format!("Line length {} mm", length.get()),
        LineConstraintKind::Fixed { at, x, y } => {
            format!("Fixed point {} ({}, {}) mm", at.as_str(), x.get(), y.get())
        }
    }
}
fn addition_name(add: &AddSketchConstraint) -> String {
    let line = match *add {
        AddSketchConstraint::Circle { curve, kind } => {
            return format!("{} · Circle {curve}", circle_kind_name(kind));
        }
        AddSketchConstraint::Concentric { a, b } => {
            return format!("Concentric · Circles {a} and {b}");
        }
        AddSketchConstraint::Line(line) => line,
    };
    match line {
        AddLineConstraint::Line { curve, kind } => format!("{} · Line {curve}", kind_name(kind)),
        AddLineConstraint::EqualLength { a, b } => format!("Equal length · Lines {a} = {b}"),
        AddLineConstraint::Relation { a, b, relation } => {
            format!("{} · Lines {a} and {b}", relation.as_str())
        }
    }
}

/// What this circle is called on the profile it belongs to.
///
/// A lone circle has no role to hold — there is no second circle for it to be
/// the boundary or the bore of — so it is called what it is.
fn role_name(roles: Option<(StableEntityId, StableEntityId)>, id: StableEntityId) -> &'static str {
    match roles {
        Some((outer, _)) if outer == id => "Boundary circle",
        Some((_, inner)) if inner == id => "Bore circle",
        _ => "Circle",
    }
}

fn circle_kind_name(kind: CircleConstraintKind) -> String {
    match kind {
        CircleConstraintKind::Radius(radius) => format!("Radius {} mm", radius.get()),
        CircleConstraintKind::FixedCenter { x, y } => {
            format!("Fixed centre ({}, {}) mm", x.get(), y.get())
        }
    }
}

pub(crate) fn finish_edit(
    editor: &mut Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<EditedSketchConstraints>,
) -> Option<PathBuf> {
    if !edits.accepts(generation) {
        return None;
    }
    if result.is_ok() {
        editor.dismiss();
    }
    edits.finish_constraints(generation, result)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use ferritecad_document::Document;
    use ferritecad_kernel::OperationContext;
    /// The single-Line halves of a pending request, in the request's own words.
    fn kind_of(add: &AddSketchConstraint) -> LineConstraintKind {
        match *add {
            AddSketchConstraint::Line(AddLineConstraint::Line { kind, .. }) => kind,
            _ => panic!("a pair relationship names no single Line"),
        }
    }
    fn curve_of(add: &AddSketchConstraint) -> StableEntityId {
        match *add {
            AddSketchConstraint::Line(AddLineConstraint::Line { curve, .. }) => curve,
            _ => panic!("a pair relationship names no single Line"),
        }
    }
    /// One addition as the peer CLI spells it in request v1.
    fn peer_addition(add: &AddSketchConstraint) -> String {
        let add = match *add {
            AddSketchConstraint::Circle { curve, kind } => {
                return match kind {
                    CircleConstraintKind::Radius(radius) => format!(
                        r#"{{"rule":"radius","curve_id":"{curve}","radius_mm":{}}}"#,
                        radius.get()
                    ),
                    CircleConstraintKind::FixedCenter { x, y } => format!(
                        r#"{{"rule":"fixed","curve_id":"{curve}","at":"center","x_mm":{},"y_mm":{}}}"#,
                        x.get(),
                        y.get()
                    ),
                };
            }
            AddSketchConstraint::Concentric { a, b } => {
                return format!(r#"{{"rule":"concentric","a_curve_id":"{a}","b_curve_id":"{b}"}}"#);
            }
            AddSketchConstraint::Line(line) => line,
        };
        let (curve, kind) = match add {
            AddLineConstraint::EqualLength { a, b } => {
                return format!(
                    r#"{{"rule":"equal_length","a_curve_id":"{a}","b_curve_id":"{b}"}}"#
                );
            }
            AddLineConstraint::Relation { a, b, relation } => {
                let rule = match relation {
                    LineRelation::Parallel => "parallel",
                    LineRelation::Perpendicular => "perpendicular",
                };
                return format!(r#"{{"rule":"{rule}","a_curve_id":"{a}","b_curve_id":"{b}"}}"#);
            }
            AddLineConstraint::Line { curve, kind } => (curve, kind),
        };
        let rule = match kind {
            LineConstraintKind::Horizontal => r#""rule":"horizontal""#.to_owned(),
            LineConstraintKind::Vertical => r#""rule":"vertical""#.to_owned(),
            LineConstraintKind::Distance(length) => {
                format!(r#""rule":"distance","distance_mm":{}"#, length.get())
            }
            LineConstraintKind::Fixed { at, x, y } => format!(
                r#""rule":"fixed","at":"{}","x_mm":{},"y_mm":{}"#,
                at.as_str(),
                x.get(),
                y.get()
            ),
        };
        format!(r#"{{"curve_id":"{curve}",{rule}}}"#)
    }
    fn frame(ctx: &egui::Context, e: &mut Editor, events: Vec<egui::Event>) -> egui::FullOutput {
        frame_running(ctx, e, events, false)
    }
    fn frame_running(
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
    fn click(ctx: &egui::Context, e: &mut Editor, label: &str) {
        click_running(ctx, e, label, false);
    }
    fn click_running(ctx: &egui::Context, e: &mut Editor, label: &str, running: bool) {
        let out = frame_running(ctx, e, vec![], running);
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
        click_at(ctx, e, at, running);
    }
    fn click_at(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2, running: bool) {
        frame_running(ctx, e, vec![egui::Event::PointerMoved(at)], running);
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
                running,
            );
        }
    }
    /// Bring a row of the bounded persisted catalogue into view. A long list
    /// scrolls; the row is still reachable, which is the point.
    fn scroll_to_row(ctx: &egui::Context, e: &mut Editor, prefix: &str) {
        for _ in 0..40 {
            let out = frame(ctx, e, vec![]);
            let at = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text().starts_with(prefix) => {
                    Some(t.visual_bounding_rect())
                }
                _ => None,
            });
            match at {
                Some(rect)
                    if out.shapes.iter().any(|s| {
                        matches!(&s.shape, egui::Shape::Text(t)
                            if t.galley.text().starts_with(prefix)
                                && s.clip_rect.contains_rect(rect))
                    }) =>
                {
                    return;
                }
                _ => {}
            }
            let anchor = text_center(&frame(ctx, e, vec![]), "Persisted constraints:");
            frame(
                ctx,
                e,
                vec![
                    egui::Event::PointerMoved(egui::pos2(anchor.x, anchor.y + 40.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0., -30.),
                        phase: egui::TouchPhase::Move,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        panic!("row never became visible: {prefix}");
    }
    fn text_center(out: &egui::FullOutput, label: &str) -> egui::Pos2 {
        out.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("not painted: {label}"))
    }
    /// The catalogue paints one Remove per removable constraint; pick the one
    /// that sits on the named row rather than whichever comes first.
    fn click_remove_on_row(ctx: &egui::Context, e: &mut Editor, prefix: &str) {
        scroll_to_row(ctx, e, prefix);
        let out = frame(ctx, e, vec![]);
        // The catalogue paints each row in order: name, Line, then its Remove.
        let row = out
            .shapes
            .iter()
            .position(
                |s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().starts_with(prefix)),
            )
            .unwrap_or_else(|| panic!("no such row: {prefix}"));
        let at = out.shapes[row..]
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "Remove" => {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no Remove after row: {prefix}"));
        click_at(ctx, e, at, false);
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("source.fcad");
        ferritecad_jobs::create_document(
            ferritecad_jobs::CreateDocumentRequest::new(
                &path,
                ferritecad_jobs::NewDocument::SamplePlate(Default::default()),
                "test",
            ),
            &OperationContext::default(),
        )
        .expect("source");
        let d = Document::open_read_only(&path).expect("doc");
        let source = ExtrudeEditSource::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }
    fn persist_rectangle_lengths(path: &Path) -> ExtrudeEditSource {
        let mut document = Document::open(path).expect("doc");
        let source = ExtrudeEditSource::read(&document).expect("catalog");
        let choice = &source.constraint_sketches[0];
        let curves = &choice.stored.as_ref().expect("stored").curves;
        let prepared = ferritecad_document::prepare_sketch_constraints(
            &document,
            choice.sketch,
            &SketchConstraintEdits {
                remove: vec![],
                add: vec![
                    AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: curves[0].id,
                        kind: LineConstraintKind::Horizontal,
                    }),
                    AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: curves[1].id,
                        kind: LineConstraintKind::Vertical,
                    }),
                    AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: curves[0].id,
                        kind: LineConstraintKind::Distance(LineLengthMm::new(60.).expect("60")),
                    }),
                    AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: curves[1].id,
                        kind: LineConstraintKind::Distance(LineLengthMm::new(30.).expect("30")),
                    }),
                ],
            },
        )
        .expect("prepare");
        document
            .write_sketch_constraints(&prepared)
            .expect("write lengths");
        let mut object = document
            .object(choice.sketch)
            .expect("read")
            .expect("sketch");
        if let ferritecad_document::ObjectPayload::Sketch(sketch) = &mut object.payload {
            for c in &mut sketch.constraints {
                if let SketchConstraintRule::Distance { a, b, distance } = &mut c.rule
                    && *distance == 60.
                {
                    std::mem::swap(a, b);
                }
            }
        }
        document
            .write(|w| {
                w.put_object(
                    object.id,
                    object.parent,
                    object.ordinal,
                    object.name.as_deref(),
                    &object.payload,
                )
            })
            .expect("reversed Distance endpoints remain selectable");
        let source = ExtrudeEditSource::read(&document).expect("catalog");
        document.close().expect("close");
        source
    }
    fn painted(out: &egui::FullOutput, label: &str) -> bool {
        out.shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == label))
    }
    fn history_state(
        e: &Editor,
    ) -> (
        SketchConstraintEdits,
        Vec<SketchConstraintEdits>,
        Vec<SketchConstraintEdits>,
    ) {
        let d = e.draft.as_ref().expect("draft");
        (
            d.edits.clone(),
            d.history.undo.clone(),
            d.history.redo.clone(),
        )
    }
    fn enter_length(ctx: &egui::Context, e: &mut Editor, value: &str, running: bool) {
        enter_field(ctx, e, "Line length (mm):", value, running);
    }
    fn enter_field(ctx: &egui::Context, e: &mut Editor, label: &str, value: &str, running: bool) {
        let out = frame_running(ctx, e, vec![], running);
        let at = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    let r = t.visual_bounding_rect();
                    Some(egui::pos2(r.right() + 30., r.center().y))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("input label: {label}"));
        frame_running(ctx, e, vec![egui::Event::PointerMoved(at)], running);
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
                running,
            );
        }
        frame_running(
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
            running,
        );
    }

    #[test]
    fn line_length_widgets_validate_input_without_changing_history_until_apply() {
        let (_root, path, source) = fixture();
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, source.constraint_sketches[0].sketch));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Add Horizontal");
        let h = history_state(&e);
        enter_length(&ctx, &mut e, "60", false);
        assert_eq!(history_state(&e), h, "typing is not a checkpoint");
        click(&ctx, &mut e, "Add length");
        let accepted = history_state(&e).0;
        assert_eq!(
            kind_of(&accepted.add[1]),
            LineConstraintKind::Distance(LineLengthMm::new(60.).expect("60 mm"))
        );
        click(&ctx, &mut e, "Undo");
        let branch = history_state(&e);
        for value in [
            "", "-", "1e-", "NaN", "inf", "-inf", "0", "-0", "-1", "1e309", "1e-999",
        ] {
            enter_length(&ctx, &mut e, value, false);
            assert_eq!(e.draft.as_ref().expect("draft").length_mm, value);
            click(&ctx, &mut e, "Add length");
            assert!(
                e.draft.as_ref().expect("draft").refusal.is_some(),
                "{value}"
            );
            assert_eq!(history_state(&e), branch, "{value}");
        }
        let field = e.draft.as_ref().expect("draft").length_mm.clone();
        enter_length(&ctx, &mut e, "30", true);
        click_running(&ctx, &mut e, "Add length", true);
        assert_eq!(e.draft.as_ref().expect("draft").length_mm, field);
        assert_eq!(history_state(&e), branch);
        click(&ctx, &mut e, "Segment 2");
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, accepted);
        assert!(e.draft.as_ref().expect("draft").refusal.is_none());
        assert_eq!(
            e.draft.as_ref().expect("draft").length_mm,
            field,
            "unapplied field is not restored"
        );
        assert_eq!(
            e.draft.as_ref().expect("draft").selected,
            Some(
                source.constraint_sketches[0]
                    .stored
                    .as_ref()
                    .expect("stored")
                    .curves[1]
                    .id
            )
        );
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(
            e.take_request()
                .expect("save accepted edits, not unapplied field")
                .edits,
            accepted
        );
        click(&ctx, &mut e, "Segment 1");
        enter_length(&ctx, &mut e, "60", false);
        let duplicate = history_state(&e);
        click(&ctx, &mut e, "Add length");
        assert_eq!(history_state(&e), duplicate);
        click(&ctx, &mut e, "Undo");
        enter_length(&ctx, &mut e, "30", false);
        click(&ctx, &mut e, "Add length");
        assert!(history_state(&e).2.is_empty());
        assert_eq!(
            kind_of(&history_state(&e).0.add[1]),
            LineConstraintKind::Distance(LineLengthMm::new(30.).expect("30"))
        );
        click(&ctx, &mut e, "Clear pending changes");
        click(&ctx, &mut e, "Undo");
        assert_eq!(
            kind_of(&history_state(&e).0.add[1]),
            LineConstraintKind::Distance(LineLengthMm::new(30.).expect("30"))
        );
    }

    #[test]
    fn replace_line_length_widgets_build_one_remove_add_request() {
        let (_root, path, _) = fixture();
        let source = persist_rectangle_lengths(&path);
        let sketch = source.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored");
        let curves = &sketch.curves;
        let h0 = sketch
            .constraints
            .iter()
            .find(|c| {
                matches!(
                    c.rule,
                    SketchConstraintRule::Horizontal { a, .. } if a.curve == curves[0].id
                )
            })
            .expect("H")
            .id;
        let d60 = stored_line_length(sketch, curves[0].id).expect("stored 60 mm");
        let d30 = stored_line_length(sketch, curves[1].id).expect("stored 30 mm");
        assert_eq!(d60.1, LineLengthMm::new(60.).expect("60"));
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, source.constraint_sketches[0].sketch));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        assert!(!frame(&ctx, &mut e, vec![]).shapes.iter().any(|s| {
            matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().starts_with("Stored length"))
        }));
        let idle = history_state(&e);
        click(&ctx, &mut e, "Replace length");
        assert_eq!(
            history_state(&e),
            idle,
            "disabled without a selected stored length"
        );
        click(&ctx, &mut e, "Segment 3");
        enter_length(&ctx, &mut e, "55", false);
        assert_eq!(e.draft.as_ref().expect("draft").length_mm, "55");
        click(&ctx, &mut e, "Replace length");
        assert_eq!(history_state(&e), idle, "no stored length on this Line");
        click(&ctx, &mut e, "Segment 1");
        assert_eq!(
            e.draft.as_ref().expect("draft").length_mm,
            "55",
            "selection does not overwrite the unapplied field"
        );
        assert!(frame(&ctx, &mut e, vec![]).shapes.iter().any(|s| matches!(
            &s.shape,
            egui::Shape::Text(t)
                if t.galley.text() == format!("Stored length 60 mm · {}", d60.0)
        )));
        click(&ctx, &mut e, "Replace length");
        let first = history_state(&e);
        assert_eq!(first.0.remove, vec![d60.0]);
        assert_eq!(
            first.0.add,
            vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                curve: curves[0].id,
                kind: LineConstraintKind::Distance(LineLengthMm::new(55.).expect("55")),
            })]
        );
        assert_eq!(first.1.len(), 1);
        assert!(first.2.is_empty());
        click(&ctx, &mut e, "Undo");
        let empty_with_redo = history_state(&e);
        assert_eq!(empty_with_redo.0, SketchConstraintEdits::default());
        assert_eq!(empty_with_redo.2, vec![first.0.clone()]);
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, first.0);
        enter_length(&ctx, &mut e, "50", false);
        click(&ctx, &mut e, "Replace length");
        let second = history_state(&e);
        assert_eq!(second.0.remove, vec![d60.0]);
        assert_eq!(
            second.0.add,
            vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                curve: curves[0].id,
                kind: LineConstraintKind::Distance(LineLengthMm::new(50.).expect("50")),
            })]
        );
        assert_eq!(
            second.1.len(),
            2,
            "in-place pending update is one checkpoint"
        );
        click(&ctx, &mut e, "Undo");
        let at_55 = history_state(&e);
        assert_eq!(at_55.0, first.0);
        assert_eq!(at_55.2, vec![second.0.clone()]);
        enter_length(&ctx, &mut e, "55", false);
        click(&ctx, &mut e, "Replace length");
        assert_eq!(history_state(&e), at_55, "same pending value is a no-op");
        enter_length(&ctx, &mut e, "50", false);
        click(&ctx, &mut e, "Replace length");
        assert_eq!(history_state(&e).0, second.0);
        assert!(history_state(&e).2.is_empty());
        click(&ctx, &mut e, "Undo");
        let branch = history_state(&e);
        for value in ["", "-", "NaN", "inf", "0", "-1", "1e309"] {
            enter_length(&ctx, &mut e, value, false);
            click(&ctx, &mut e, "Replace length");
            assert!(
                e.draft.as_ref().expect("draft").refusal.is_some(),
                "{value}"
            );
            assert_eq!(history_state(&e), branch, "{value}");
        }
        enter_length(&ctx, &mut e, "70", true);
        click_running(&ctx, &mut e, "Replace length", true);
        assert_eq!(history_state(&e), branch);
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, second.0);
        click(&ctx, &mut e, "Clear pending changes");
        click(&ctx, &mut e, "Remove");
        click(&ctx, &mut e, "Segment 3");
        click(&ctx, &mut e, "Add Horizontal");
        click(&ctx, &mut e, "Segment 4");
        enter_length(&ctx, &mut e, "40", false);
        click(&ctx, &mut e, "Add length");
        click(&ctx, &mut e, "Segment 1");
        enter_length(&ctx, &mut e, "55", false);
        click(&ctx, &mut e, "Replace length");
        let mixed = history_state(&e).0;
        assert_eq!(mixed.remove, vec![h0, d60.0]);
        assert_eq!(
            mixed.add,
            vec![
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: curves[2].id,
                    kind: LineConstraintKind::Horizontal,
                }),
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: curves[3].id,
                    kind: LineConstraintKind::Distance(LineLengthMm::new(40.).expect("40")),
                }),
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: curves[0].id,
                    kind: LineConstraintKind::Distance(LineLengthMm::new(55.).expect("55")),
                }),
            ]
        );
        enter_length(&ctx, &mut e, "50", false);
        click(&ctx, &mut e, "Replace length");
        let updated = history_state(&e).0;
        assert_eq!(updated.remove, vec![h0, d60.0]);
        assert_eq!(updated.add.len(), 3);
        assert_eq!(
            kind_of(&updated.add[2]),
            LineConstraintKind::Distance(LineLengthMm::new(50.).expect("50"))
        );
        assert_eq!(&updated.add[..2], &mixed.add[..2]);
        let before_revert = history_state(&e);
        enter_length(&ctx, &mut e, "60", false);
        click(&ctx, &mut e, "Replace length");
        let reverted = history_state(&e).0;
        assert_eq!(reverted.remove, vec![h0]);
        assert!(!reverted.remove.contains(&d60.0));
        assert!(!reverted.remove.contains(&d30.0));
        assert_eq!(
            reverted.add,
            vec![
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: curves[2].id,
                    kind: LineConstraintKind::Horizontal,
                }),
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: curves[3].id,
                    kind: LineConstraintKind::Distance(LineLengthMm::new(40.).expect("40")),
                }),
            ]
        );
        click(&ctx, &mut e, "Undo");
        assert_eq!(history_state(&e).0, before_revert.0);
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, reverted);
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(e.take_request().expect("restored request").edits, reverted);
        let history = history_state(&e);
        let mut state = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        let generation = state
            .start_constraints(
                EditSketchConstraintsRequest {
                    source: path.clone(),
                    expected: source.version,
                    sketch: source.constraint_sketches[0].sketch,
                    edits: reverted.clone(),
                    destination: PathBuf::new(),
                },
                move |_, g, _| std::thread::spawn(move || tx.send(g).expect("reply")),
            )
            .expect("start");
        assert!(
            finish_edit(
                &mut e,
                &mut state,
                generation + 1,
                Err(ferritecad_types::CadError::input("stale"))
            )
            .is_none()
        );
        assert_eq!(history_state(&e), history);
        let g = rx.recv().expect("done");
        assert!(
            finish_edit(
                &mut e,
                &mut state,
                g,
                Err(ferritecad_types::CadError::topology(
                    "lost stored reference"
                ))
            )
            .is_none()
        );
        assert_eq!(history_state(&e), history);
        click(&ctx, &mut e, "Clear pending changes");
        click(&ctx, &mut e, "Segment 1");
        enter_length(&ctx, &mut e, "55", false);
        click(&ctx, &mut e, "Replace length");
        enter_length(&ctx, &mut e, "60", false);
        click(&ctx, &mut e, "Replace length");
        assert_eq!(
            history_state(&e).0,
            SketchConstraintEdits::default(),
            "return to stored length cancels only that replacement"
        );
        assert!(!painted(
            &frame(&ctx, &mut e, vec![]),
            "Save constraints copy…"
        ));
        // The Remove checkbox can leave a structurally invalid draft. A valid
        // number must still pass the common validator before Replace changes it.
        click(&ctx, &mut e, "Remove");
        click(&ctx, &mut e, "Add Vertical");
        click(&ctx, &mut e, "Remove");
        click(&ctx, &mut e, "Remove");
        click(&ctx, &mut e, "Undo");
        let conflicting = history_state(&e);
        assert!(conflicting.0.remove.is_empty());
        assert_eq!(kind_of(&conflicting.0.add[0]), LineConstraintKind::Vertical);
        assert!(!conflicting.2.is_empty());
        enter_length(&ctx, &mut e, "55", false);
        click(&ctx, &mut e, "Replace length");
        assert_eq!(history_state(&e), conflicting);
        assert!(
            e.draft
                .as_ref()
                .expect("draft")
                .refusal
                .as_deref()
                .expect("validator refusal")
                .contains("only one H/V")
        );
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Replace length");
        let combined = history_state(&e).0;
        assert_eq!(combined.remove, vec![h0, d60.0]);
        assert_eq!(
            combined.add[0], conflicting.0.add[0],
            "same-Line V stays first"
        );
        assert_eq!(combined.add.len(), 2);
        click(&ctx, &mut e, "Cancel constraints draft");
        assert!(e.begin(&path, &source, source.constraint_sketches[0].sketch));
        assert_eq!(history_state(&e), Default::default());
        assert_eq!(e.draft.as_ref().expect("draft").length_mm, "");
    }

    #[test]
    fn constraint_history_widgets_restore_requests_and_branch_only_on_changes() {
        let (_root, path, source) = fixture();
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, source.constraint_sketches[0].sketch));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Clear pending changes");
        assert_eq!(history_state(&e), Default::default());
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Add Horizontal");
        let h = e.draft.as_ref().expect("draft").edits.clone();
        assert_eq!(
            h.add,
            vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                curve: source.constraint_sketches[0]
                    .stored
                    .as_ref()
                    .expect("stored")
                    .curves[0]
                    .id,
                kind: LineConstraintKind::Horizontal
            })]
        );
        click(&ctx, &mut e, "Undo");
        let empty_with_redo = history_state(&e);
        assert_eq!(empty_with_redo.0, SketchConstraintEdits::default());
        // No-op Clear and disabled Undo cannot erase the redo branch.
        click(&ctx, &mut e, "Clear pending changes");
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Segment 2");
        assert_eq!(history_state(&e), empty_with_redo);
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, h);
        let after_redo = history_state(&e);
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e), after_redo);
        click(&ctx, &mut e, "Add Vertical");
        let hv = history_state(&e).0;
        assert_eq!(hv.add[0], h.add[0]);
        assert_eq!(kind_of(&hv.add[1]), LineConstraintKind::Vertical);
        click(&ctx, &mut e, "Undo");
        let branch = history_state(&e);
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Add Vertical");
        assert!(e.draft.as_ref().expect("draft").refusal.is_some());
        assert_eq!(history_state(&e), branch);
        // Running disables both nonempty stacks and all request mutations.
        for label in [
            "Undo",
            "Redo",
            "Clear pending changes",
            "Add Horizontal",
            "Cancel constraints draft",
            "Save constraints copy…",
        ] {
            click_running(&ctx, &mut e, label, true);
            assert_eq!(history_state(&e), branch);
            assert!(e.take_request().is_none());
        }
        click(&ctx, &mut e, "Redo");
        assert!(e.draft.as_ref().expect("draft").refusal.is_none());
        assert_eq!(history_state(&e).0, hv);
        click(&ctx, &mut e, "Clear pending changes");
        click(&ctx, &mut e, "Undo");
        assert_eq!(history_state(&e).0, hv);
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Segment 3");
        click(&ctx, &mut e, "Add Horizontal");
        let branched = history_state(&e);
        assert_eq!(branched.0.add[0], h.add[0]);
        assert_eq!(
            curve_of(&branched.0.add[1]),
            source.constraint_sketches[0]
                .stored
                .as_ref()
                .expect("stored")
                .curves[2]
                .id
        );
        assert!(branched.2.is_empty());
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(
            e.take_request().expect("save current branch").edits,
            branched.0
        );
    }

    #[test]
    fn constraint_history_remove_widgets_restore_exact_persisted_ids() {
        for kind in [
            LineConstraintKind::Horizontal,
            LineConstraintKind::Distance(LineLengthMm::new(60.).expect("length")),
        ] {
            let (_root, path, source) = fixture();
            let choice = &source.constraint_sketches[0];
            let mut document = Document::open(&path).expect("doc");
            let prepared = ferritecad_document::prepare_sketch_constraints(
                &document,
                choice.sketch,
                &SketchConstraintEdits {
                    remove: vec![],
                    add: vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: choice.stored.as_ref().expect("stored").curves[0].id,
                        kind,
                    })],
                },
            )
            .expect("prepare persisted H");
            document.write_sketch_constraints(&prepared).expect("write");
            let source = ExtrudeEditSource::read(&document).expect("catalog");
            document.close().expect("close");
            let h = source.constraint_sketches[0]
                .stored
                .as_ref()
                .expect("stored")
                .constraints
                .iter()
                .find(|c| {
                    matches!(
                        c.rule,
                        SketchConstraintRule::Horizontal { .. }
                            | SketchConstraintRule::Distance { .. }
                    )
                })
                .expect("H")
                .id;
            let mut e = Editor::default();
            assert!(e.begin(&path, &source, choice.sketch));
            let ctx = egui::Context::default();
            for _ in 0..3 {
                frame(&ctx, &mut e, vec![]);
            }
            click(&ctx, &mut e, "Remove");
            let removed = SketchConstraintEdits {
                remove: vec![h],
                add: vec![],
            };
            assert_eq!(history_state(&e).0, removed);
            click(&ctx, &mut e, "Remove");
            assert_eq!(history_state(&e).0, SketchConstraintEdits::default());
            click(&ctx, &mut e, "Undo");
            assert_eq!(history_state(&e).0, removed);
            click(&ctx, &mut e, "Redo");
            assert_eq!(history_state(&e).0, SketchConstraintEdits::default());
            click(&ctx, &mut e, "Undo");
            click(&ctx, &mut e, "Undo");
            assert_eq!(history_state(&e).0, SketchConstraintEdits::default());
            click(&ctx, &mut e, "Redo");
            click(&ctx, &mut e, "Save constraints copy…");
            assert_eq!(e.take_request().expect("exact UUID request").edits, removed);
        }
    }

    #[test]
    fn constraint_history_bounds_both_stacks_and_retains_order() {
        let mut history = History::default();
        let mut edits = SketchConstraintEdits::default();
        let states: Vec<_> = (0..=140)
            .map(|_| SketchConstraintEdits {
                remove: vec![StableEntityId::new(), StableEntityId::new()],
                add: vec![
                    AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: StableEntityId::new(),
                        kind: LineConstraintKind::Vertical,
                    }),
                    AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: StableEntityId::new(),
                        kind: LineConstraintKind::Horizontal,
                    }),
                ],
            })
            .collect();
        for next in &states {
            history.change(&mut edits, next.clone());
            assert!(history.undo.len() <= HISTORY_LIMIT);
            assert!(history.redo.len() <= HISTORY_LIMIT);
        }
        for expected in states[12..140].iter().rev() {
            history.undo(&mut edits);
            assert_eq!(&edits, expected);
        }
        assert!(history.undo.is_empty());
        assert_eq!(history.redo.len(), HISTORY_LIMIT);
        history.undo(&mut edits);
        history.change(&mut edits, states[12].clone());
        assert_eq!(history.redo.len(), HISTORY_LIMIT, "no-op preserves redo");
        for expected in &states[13..] {
            history.redo(&mut edits);
            assert_eq!(&edits, expected);
            assert!(history.undo.len() <= HISTORY_LIMIT);
        }
        assert_eq!(history.undo.len(), HISTORY_LIMIT);
        assert!(history.redo.is_empty());
    }
    #[test]
    fn constraint_editor_keeps_actions_reachable_with_many_pending_additions() {
        for constrained in [false, true] {
            let (_root, path, _) = fixture();
            let mut document = Document::open(&path).expect("source");
            let mut object = document
                .objects()
                .expect("objects")
                .into_iter()
                .find(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Sketch(_)))
                .expect("Sketch");
            let ferritecad_document::ObjectPayload::Sketch(sketch) = &mut object.payload else {
                unreachable!()
            };
            let old_ids: Vec<_> = sketch.curves.iter().map(|c| c.id).collect();
            let points: Vec<_> = (0..64)
                .map(|i| {
                    let angle = f64::from(i) * std::f64::consts::TAU / 64.;
                    ferritecad_document::Point2::new(40. * angle.cos(), 40. * angle.sin())
                        .expect("point")
                })
                .collect();
            sketch.curves = (0..64)
                .map(|i| ferritecad_document::SketchCurve {
                    id: old_ids.get(i).copied().unwrap_or_else(StableEntityId::new),
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: points[i],
                        end: points[(i + 1) % 64],
                    },
                })
                .collect();
            document
                .write(|w| {
                    w.put_object(
                        object.id,
                        object.parent,
                        object.ordinal,
                        object.name.as_deref(),
                        &object.payload,
                    )
                })
                .expect("polygon");
            if constrained {
                let ferritecad_document::ObjectPayload::Sketch(sketch) = &object.payload else {
                    unreachable!()
                };
                let prepared = ferritecad_document::prepare_sketch_constraints(
                    &document,
                    object.id,
                    &SketchConstraintEdits {
                        remove: vec![],
                        add: vec![
                            AddSketchConstraint::Line(AddLineConstraint::Line {
                                curve: sketch.curves.last().expect("curve").id,
                                kind: LineConstraintKind::Horizontal,
                            }),
                            AddSketchConstraint::Line(AddLineConstraint::Line {
                                curve: sketch.curves.last().expect("curve").id,
                                kind: LineConstraintKind::Distance(
                                    LineLengthMm::new(42.).expect("42"),
                                ),
                            }),
                        ],
                    },
                )
                .expect("persisted H and all closure links");
                document
                    .write_sketch_constraints(&prepared)
                    .expect("constraints");
            }
            let source = ExtrudeEditSource::read(&document).expect("discovery");
            document.close().expect("close");
            let mut e = Editor::default();
            assert!(e.begin(&path, &source, object.id));
            let draft = e.draft.as_mut().expect("draft");
            if constrained {
                draft.selected = draft
                    .choice
                    .stored
                    .as_ref()
                    .expect("stored")
                    .curves
                    .last()
                    .map(|c| c.id);
            }
            draft.edits.add = draft
                .choice
                .stored
                .as_ref()
                .expect("supported")
                .curves
                .iter()
                .step_by(2)
                .map(|c| {
                    AddSketchConstraint::Line(AddLineConstraint::Line {
                        curve: c.id,
                        kind: LineConstraintKind::Horizontal,
                    })
                })
                .collect();
            draft
                .choice
                .validate_edits(&draft.edits)
                .expect("valid request");
            let ctx = egui::Context::default();
            for _ in 0..3 {
                frame(&ctx, &mut e, vec![]);
            }
            let out = frame(&ctx, &mut e, vec![]);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(988., 768.));
            if constrained {
                assert!(out.shapes.iter().any(|s| matches!(&s.shape,
                    egui::Shape::Text(t) if t.galley.text().starts_with("Stored length 42 mm"))));
            }
            for label in [
                "Cancel constraints draft",
                "Undo",
                "Redo",
                "Clear pending changes",
                "Add length",
                "Replace length",
                "Pin Start",
                "Pin End",
                "Add Fixed point",
                "Pair Line A",
                "Pair Line B",
                "Add Equal length",
                "Add Parallel",
                "Add Perpendicular",
                "Save constraints copy…",
            ] {
                assert!(
                    out.shapes.iter().any(|s| matches!(&s.shape,
                        egui::Shape::Text(t) if t.galley.text() == label
                            && s.clip_rect.contains_rect(t.visual_bounding_rect())
                            && screen.contains_rect(t.visual_bounding_rect())
                    )),
                    "{label} must remain visible and reachable with many pending additions"
                );
            }
            let pending = e.draft.as_ref().expect("draft").edits.clone();
            click(&ctx, &mut e, "Save constraints copy…");
            assert_eq!(
                e.take_request().expect("reachable Save").edits.add.len(),
                32
            );
            click(&ctx, &mut e, "Clear pending changes");
            assert!(e.draft.as_ref().expect("draft").edits.add.is_empty());
            click(&ctx, &mut e, "Undo");
            assert_eq!(e.draft.as_ref().expect("draft").edits, pending);
            click(&ctx, &mut e, "Redo");
            assert_eq!(
                e.draft.as_ref().expect("draft").edits,
                SketchConstraintEdits::default()
            );
            click(&ctx, &mut e, "Undo");
            click(&ctx, &mut e, "Save constraints copy…");
            assert_eq!(e.take_request().expect("restored Save").edits, pending);
        }
    }

    #[test]
    fn constraint_widgets_keep_draft_on_save_cancel_refusal_and_stale_reply() {
        let (_root, path, source) = fixture();
        let mut e = Editor::default();
        let id = source.constraint_sketches[0].sketch;
        assert!(!e.begin(&path, &source, ObjectId::new()));
        assert!(e.begin(&path, &source, id));
        let ctx = egui::Context::default();
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Add Horizontal");
        enter_length(&ctx, &mut e, "60", false);
        click(&ctx, &mut e, "Add length");
        let kept = e.draft.as_ref().expect("draft").edits.clone();
        assert_eq!(kept.add.len(), 2);
        click(&ctx, &mut e, "Segment 2");
        click(&ctx, &mut e, "Add Vertical");
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Segment 1");
        let history = history_state(&e);
        assert!(!history.1.is_empty() && !history.2.is_empty());
        click(&ctx, &mut e, "Add Vertical");
        assert!(e.draft.as_ref().expect("draft").refusal.is_some());
        assert_eq!(e.draft.as_ref().expect("draft").edits, kept);
        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("submit");
        assert_eq!(request.edits, kept);
        assert_eq!(request.expected, source.version);
        assert!(e.take_request().is_none());
        // Save Cancel returns no path: no worker is started and this same draft is retryable.
        assert_eq!(e.draft.as_ref().expect("retained").edits, kept);
        assert_eq!(history_state(&e), history);
        let mut state = crate::edits::Edits::default();
        for cancelled in [false, true] {
            let (tx, rx) = std::sync::mpsc::channel();
            let generation = state
                .start_constraints(request.clone(), move |_, g, c| {
                    std::thread::spawn(move || {
                        if cancelled {
                            c.cancel();
                        }
                        tx.send(g).expect("reply")
                    })
                })
                .expect("start");
            assert!(
                state
                    .start_constraints(request.clone(), |_, _, _| panic!("duplicate"))
                    .is_none()
            );
            let error = if cancelled {
                ferritecad_types::CadError::Cancelled
            } else {
                ferritecad_types::CadError::topology("lost stored reference")
            };
            assert!(
                finish_edit(
                    &mut e,
                    &mut state,
                    generation + 1,
                    Err(ferritecad_types::CadError::input("stale"))
                )
                .is_none()
            );
            assert_eq!(e.draft.as_ref().expect("retained").edits, kept);
            assert_eq!(history_state(&e), history);
            let g = rx.recv().expect("done");
            assert!(finish_edit(&mut e, &mut state, g, Err(error)).is_none());
            assert_eq!(e.draft.as_ref().expect("retained").edits, kept);
            assert_eq!(history_state(&e), history);
        }
        click(&ctx, &mut e, "Clear pending changes");
        assert!(e.draft.as_ref().expect("draft").edits.add.is_empty());
        click(&ctx, &mut e, "Cancel constraints draft");
        assert!(!e.active());
        assert!(e.begin(&path, &source, id));
        assert_eq!(history_state(&e), Default::default());
        e.dismiss();
        let mut forbidden = source.clone();
        forbidden.refusal = Some("document copy forbidden".into());
        assert!(!e.begin(&path, &forbidden, id));
    }
    fn persist_pin(path: &Path) -> (ExtrudeEditSource, StableEntityId, StableEntityId) {
        let mut document = Document::open(path).expect("doc");
        let source = ExtrudeEditSource::read(&document).expect("catalog");
        let choice = &source.constraint_sketches[0];
        let curve = choice.stored.as_ref().expect("stored").curves[1].id;
        let prepared = ferritecad_document::prepare_sketch_constraints(
            &document,
            choice.sketch,
            &SketchConstraintEdits {
                remove: vec![],
                add: vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve,
                    kind: pin(LineEndpoint::End, 12.5, -0.5),
                })],
            },
        )
        .expect("prepare pin");
        let id = prepared.added.last().expect("pin").id;
        document
            .write_sketch_constraints(&prepared)
            .expect("write pin");
        let source = ExtrudeEditSource::read(&document).expect("catalog");
        document.close().expect("close");
        (source, id, curve)
    }
    fn pin(at: LineEndpoint, x: f64, y: f64) -> LineConstraintKind {
        LineConstraintKind::Fixed {
            at,
            x: SketchCoordinateMm::new(x).expect("x"),
            y: SketchCoordinateMm::new(y).expect("y"),
        }
    }

    /// A real §25J cylinder and the accepted reading a form may read from.
    fn circle_fixture() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("cylinder.fcad");
        ferritecad_jobs::create_document_with_kernel(
            ferritecad_jobs::CreateDocumentRequest::new(
                &path,
                ferritecad_jobs::NewDocument::CircleExtrude(
                    ferritecad_document::CircleExtrusion::new([12., -7.], 10., 15.)
                        .expect("a circle"),
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

    /// A real §25L ring and the accepted reading a form may read from.
    fn annulus_fixture() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("ring.fcad");
        ferritecad_jobs::create_document_with_kernel(
            ferritecad_jobs::CreateDocumentRequest::new(
                &path,
                ferritecad_jobs::NewDocument::AnnularExtrude(
                    ferritecad_document::AnnularExtrusion::new([12., -7.], 10., 4., 15.)
                        .expect("a ring"),
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

    /// The annular half of the same form, through real widgets and real clicks.
    ///
    /// Both halves matter and the second one is the one review found missing
    /// last time: a persisted request has to come back as its own removable
    /// rows, not only be addable into an empty draft.
    #[test]
    fn annulus_widgets_name_both_roles_and_build_one_parametric_request() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for the accepted scene this form reads");
            return;
        }
        let (_root, path, source) = annulus_fixture();
        let id = source.constraint_sketches[0].sketch;
        let stored = source.constraint_sketches[0]
            .stored
            .clone()
            .expect("a supported annular profile");
        let (boundary, bore) =
            ferritecad_document::constraint_circle_roles(&stored).expect("two circles in roles");
        assert_ne!(boundary, bore);

        let mut e = Editor::default();
        assert!(
            e.begin(&path, &source, id),
            "the annular profile is offered"
        );
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        let out = frame(&ctx, &mut e, vec![]);
        let drawn: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        // Each circle is named by its role and by its own UUID, and no Line row
        // or Line action is offered for either of them.
        let boundary_row = format!(
            "Boundary circle · {} · centre (12, -7) mm · radius 10 mm",
            short(boundary)
        );
        let bore_row = format!(
            "Bore circle · {} · centre (12, -7) mm · radius 4 mm",
            short(bore)
        );
        for row in [&boundary_row, &bore_row] {
            assert!(drawn.contains(row), "missing {row} in {drawn:?}");
        }
        assert!(!drawn.iter().any(|l| l.starts_with("Segment ")));
        for hidden in ["Add Horizontal", "Add length", "Pin Start", "Pair Line A"] {
            assert!(!painted(&out, hidden), "{hidden} offered on a ring");
        }

        // A pair has to be picked before it can be made concentric.
        click(&ctx, &mut e, "Add Concentric");
        assert!(e.draft.as_ref().expect("draft").edits.add.is_empty());

        click(&ctx, &mut e, &boundary_row);
        click(&ctx, &mut e, "Pair Circle A");
        click(&ctx, &mut e, &bore_row);
        click(&ctx, &mut e, "Pair Circle B");
        click(&ctx, &mut e, "Add Concentric");
        let draft = e.draft.as_ref().expect("draft");
        assert_eq!(draft.refusal, None);
        assert_eq!(
            draft.edits.add,
            vec![AddSketchConstraint::Concentric {
                a: boundary,
                b: bore
            }]
        );
        // One pair, one slot: the same pair the other way round is refused by
        // the document's own rule rather than by a copy of it here.
        click(&ctx, &mut e, "Pair Circle A");
        click(&ctx, &mut e, &boundary_row);
        click(&ctx, &mut e, "Pair Circle B");
        click(&ctx, &mut e, "Add Concentric");
        let draft = e.draft.as_ref().expect("draft");
        assert_eq!(draft.edits.add.len(), 1, "(B,A) took a second slot");
        assert!(draft.refusal.is_some());

        // A radius per circle, each against the circle that is selected.
        click(&ctx, &mut e, &boundary_row);
        enter_field(&ctx, &mut e, "Radius mm", "6.75", false);
        click(&ctx, &mut e, "Add radius");
        click(&ctx, &mut e, &bore_row);
        enter_field(&ctx, &mut e, "Radius mm", "2.125", false);
        click(&ctx, &mut e, "Add radius");
        // And one pin, on whichever circle is selected.
        click(&ctx, &mut e, &boundary_row);
        enter_field(&ctx, &mut e, "Fixed X (mm):", "-3.5", false);
        enter_field(&ctx, &mut e, "Fixed Y (mm):", "4.25", false);
        click(&ctx, &mut e, "Add Fixed centre");
        let draft = e.draft.as_ref().expect("draft");
        assert_eq!(draft.refusal, None);
        assert_eq!(
            draft.edits.add,
            vec![
                AddSketchConstraint::Concentric {
                    a: boundary,
                    b: bore
                },
                AddSketchConstraint::Circle {
                    curve: boundary,
                    kind: CircleConstraintKind::Radius(
                        CircleRadiusMm::new(6.75).expect("positive")
                    ),
                },
                AddSketchConstraint::Circle {
                    curve: bore,
                    kind: CircleConstraintKind::Radius(
                        CircleRadiusMm::new(2.125).expect("positive")
                    ),
                },
                AddSketchConstraint::Circle {
                    curve: boundary,
                    kind: CircleConstraintKind::FixedCenter {
                        x: SketchCoordinateMm::new(-3.5).expect("finite"),
                        y: SketchCoordinateMm::new(4.25).expect("finite"),
                    },
                },
            ]
        );
        assert_eq!(draft.history.undo.len(), 4, "one add, one step");
        click(&ctx, &mut e, "Undo");
        assert_eq!(e.draft.as_ref().expect("draft").edits.add.len(), 3);
        assert!(e.take_request().is_none(), "Undo submitted a job");
        click(&ctx, &mut e, "Redo");
        assert_eq!(e.draft.as_ref().expect("draft").edits.add.len(), 4);
        assert!(e.take_request().is_none(), "Redo submitted a job");

        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("real widget request");
        assert_eq!(request.sketch, id);
        assert_eq!(request.source, path);
        assert_eq!(request.expected, source.version);
        assert!(e.take_request().is_none(), "one press, one request");

        // Persist exactly that request, then reopen it: every stored row has to
        // come back as itself, with its own UUID and its own Remove.
        let mut document = Document::open(&path).expect("open fixture");
        let prepared =
            ferritecad_document::prepare_sketch_constraints(&document, id, &request.edits)
                .expect("prepare annular constraints");
        let [concentric, outer_radius, inner_radius, pin] = prepared.added.as_slice() else {
            panic!("four constraints, got {:?}", prepared.added)
        };
        let (concentric, outer_radius, inner_radius, pin) =
            (concentric.id, outer_radius.id, inner_radius.id, pin.id);
        document
            .write_sketch_constraints(&prepared)
            .expect("persist");
        let reading = ExtrudeEditSource::read(&document).expect("read persisted");
        document.close().expect("close");

        let mut e = Editor::default();
        let ctx = egui::Context::default();
        assert!(e.begin(&path, &reading, id));
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        let concentric_row = format!("Concentric · {concentric}");
        let outer_row = format!("Radius 6.75 mm · {outer_radius}");
        let out = frame(&ctx, &mut e, vec![]);
        for row in [
            &concentric_row,
            &outer_row,
            &format!("Radius 2.125 mm · {inner_radius}"),
            &format!("Fixed centre (-3.5, 4.25) mm · {pin}"),
        ] {
            assert!(painted(&out, row), "persisted row missing: {row}");
        }
        assert!(
            !painted(&out, "Coincident closure"),
            "a shared centre was labelled Line closure"
        );

        // Both a shared centre and a radius come off by their exact UUIDs, and
        // a replacement radius rides in the same request.
        click_remove_on_row(&ctx, &mut e, &concentric_row);
        assert_eq!(history_state(&e).0.remove, vec![concentric]);
        click_remove_on_row(&ctx, &mut e, &outer_row);
        assert_eq!(history_state(&e).0.remove, vec![concentric, outer_radius]);
        click(&ctx, &mut e, &boundary_row);
        enter_field(&ctx, &mut e, "Radius mm", "8.125", false);
        click(&ctx, &mut e, "Add radius");
        let replacement = history_state(&e).0;
        assert_eq!(replacement.remove, vec![concentric, outer_radius]);
        assert_eq!(
            replacement.add,
            vec![AddSketchConstraint::Circle {
                curve: boundary,
                kind: CircleConstraintKind::Radius(CircleRadiusMm::new(8.125).expect("positive")),
            }]
        );
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, replacement);
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(
            e.take_request().expect("replacement request").edits,
            replacement
        );
    }

    /// The circle half of the same form, through real widgets and real clicks.
    #[test]
    fn circle_widgets_add_a_radius_and_a_fixed_centre_as_one_request_each() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: no OCCT for the accepted scene this form reads");
            return;
        }
        let (_root, path, source) = circle_fixture();
        let id = source.constraint_sketches[0].sketch;
        let stored = source.constraint_sketches[0]
            .stored
            .clone()
            .expect("a supported circle profile");
        let curve = stored.curves[0].id;

        let mut e = Editor::default();
        assert!(e.begin(&path, &source, id), "the circle profile is offered");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }

        // The form names the circle by its own UUID and its stored numbers, and
        // offers no Line row for it.
        let out = frame(&ctx, &mut e, vec![]);
        let drawn: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        assert!(
            drawn
                .iter()
                .any(|l| l.starts_with("Circle ·") && l.contains("radius 10 mm")),
            "the form does not offer the stored circle: {drawn:?}"
        );
        assert!(
            !drawn.iter().any(|l| l.starts_with("Segment ")),
            "a circle profile offered a Line segment: {drawn:?}"
        );

        // Nothing is selected yet, so nothing may be added.
        click(&ctx, &mut e, "Add radius");
        assert!(
            e.draft.as_ref().expect("draft").edits.add.is_empty(),
            "an unselected circle accepted a radius"
        );

        click(&ctx, &mut e, &format!("Circle · {}", short(curve)));
        assert_eq!(e.draft.as_ref().expect("draft").selected, Some(curve));

        // A radius that is not a positive number is refused in the form and
        // changes no history.
        for bad in ["", "0", "-4", "banana"] {
            enter_field(&ctx, &mut e, "Radius mm", bad, false);
            click(&ctx, &mut e, "Add radius");
            let draft = e.draft.as_ref().expect("draft");
            assert!(draft.edits.add.is_empty(), "radius {bad:?} was accepted");
            assert!(draft.refusal.is_some(), "radius {bad:?} said nothing");
            assert!(draft.history.undo.is_empty());
        }

        // One accepted radius is one request and one history step.
        enter_field(&ctx, &mut e, "Radius mm", "6.75", false);
        click(&ctx, &mut e, "Add radius");
        let draft = e.draft.as_ref().expect("draft");
        assert_eq!(draft.refusal, None);
        assert_eq!(
            draft.edits.add,
            vec![AddSketchConstraint::Circle {
                curve,
                kind: CircleConstraintKind::Radius(CircleRadiusMm::new(6.75).expect("positive")),
            }]
        );
        assert_eq!(draft.history.undo.len(), 1, "one add, one step");

        // A second radius on the same circle is the occupied slot, refused by
        // the document's own rule rather than by a copy of it here.
        enter_field(&ctx, &mut e, "Radius mm", "8.125", false);
        click(&ctx, &mut e, "Add radius");
        let draft = e.draft.as_ref().expect("draft");
        assert_eq!(draft.edits.add.len(), 1, "a second radius was accepted");
        assert!(
            draft
                .refusal
                .as_deref()
                .is_some_and(|r| r.contains("only one radius")),
            "{:?}",
            draft.refusal
        );

        // The centre pin uses the same X/Y boxes the Line pin uses, and lands
        // as a circle addition with the centre selector.
        enter_field(&ctx, &mut e, "Fixed X (mm):", "-3.5", false);
        enter_field(&ctx, &mut e, "Fixed Y (mm):", "4.25", false);
        click(&ctx, &mut e, "Add Fixed centre");
        let draft = e.draft.as_ref().expect("draft");
        assert_eq!(draft.refusal, None);
        assert_eq!(draft.edits.add.len(), 2);
        assert_eq!(
            draft.edits.add[1],
            AddSketchConstraint::Circle {
                curve,
                kind: CircleConstraintKind::FixedCenter {
                    x: SketchCoordinateMm::new(-3.5).expect("finite"),
                    y: SketchCoordinateMm::new(4.25).expect("finite"),
                },
            }
        );
        assert_eq!(draft.history.undo.len(), 2);

        // Undo and Redo move the request and ask no job for anything.
        click(&ctx, &mut e, "Undo");
        assert_eq!(e.draft.as_ref().expect("draft").edits.add.len(), 1);
        assert!(e.take_request().is_none(), "Undo submitted a job");
        click(&ctx, &mut e, "Redo");
        assert_eq!(e.draft.as_ref().expect("draft").edits.add.len(), 2);
        assert!(e.take_request().is_none(), "Redo submitted a job");

        // Saving hands over exactly the request the widgets built.
        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("real widget request");
        assert_eq!(request.sketch, id);
        assert_eq!(request.source, path);
        assert_eq!(request.expected, source.version);
        assert_eq!(request.edits.remove, Vec::new());
        assert_eq!(request.edits.add.len(), 2);
        assert!(e.take_request().is_none(), "one press, one request");

        // Reopen a persisted request: the radius must remain editable by its
        // own UUID, rather than falling into the non-removable closure row.
        let mut document = Document::open(&path).expect("open fixture");
        let prepared =
            ferritecad_document::prepare_sketch_constraints(&document, id, &request.edits)
                .expect("prepare circle constraints");
        let radius_id = prepared.added[0].id;
        let pin_id = prepared.added[1].id;
        document
            .write_sketch_constraints(&prepared)
            .expect("persist");
        let reading = ExtrudeEditSource::read(&document).expect("read persisted");
        document.close().expect("close");
        let mut e = Editor::default();
        let ctx = egui::Context::default();
        assert!(e.begin(&path, &reading, id));
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        let radius_row = format!("Radius 6.75 mm · {radius_id}");
        let out = frame(&ctx, &mut e, vec![]);
        assert!(
            painted(&out, &radius_row),
            "persisted Radius must not be labelled closure"
        );
        assert!(painted(
            &out,
            &format!("Fixed centre (-3.5, 4.25) mm · {pin_id}")
        ));
        assert!(
            !painted(&out, "Add Horizontal"),
            "circle must not offer Line actions"
        );
        assert!(!painted(&out, "Pin Start"), "circle has no endpoints");
        click_remove_on_row(&ctx, &mut e, &radius_row);
        assert_eq!(history_state(&e).0.remove, vec![radius_id]);
        click(&ctx, &mut e, &format!("Circle · {}", short(curve)));
        enter_field(&ctx, &mut e, "Radius mm", "8.125", false);
        click(&ctx, &mut e, "Add radius");
        let replacement = history_state(&e).0;
        assert_eq!(replacement.remove, vec![radius_id]);
        assert_eq!(
            replacement.add,
            vec![AddSketchConstraint::Circle {
                curve,
                kind: CircleConstraintKind::Radius(CircleRadiusMm::new(8.125).expect("radius")),
            }]
        );
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, replacement);
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(
            e.take_request().expect("replacement request").edits,
            replacement
        );
        // Removing both is also possible; the centre remains a separate UUID.
        click(&ctx, &mut e, "Clear pending changes");
        click_remove_on_row(&ctx, &mut e, &radius_row);
        click_remove_on_row(
            &ctx,
            &mut e,
            &format!("Fixed centre (-3.5, 4.25) mm · {pin_id}"),
        );
        click(&ctx, &mut e, "Save constraints copy…");
        let removed = e.take_request().expect("remove both request").edits;
        assert_eq!(removed.remove, vec![radius_id, pin_id]);
        assert!(removed.add.is_empty());
    }

    #[test]
    fn native_circle_constraint_worker_and_cli_publish_the_same_solid() {
        if !ferritecad_occt::is_available() || !ferritecad_sketch_solver::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            assert_ne!(
                std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
                Ok("1")
            );
            eprintln!("skipped: circle constraint worker needs OCCT and planegcs");
            return;
        }
        let (root, path, source) = circle_fixture();
        let before = std::fs::read(&path).expect("source");
        let choice = &source.constraint_sketches[0];
        let curve = choice.stored.as_ref().expect("circle").curves[0].id;
        let mut e = Editor::default();
        let ctx = egui::Context::default();
        assert!(e.begin(&path, &source, choice.sketch));
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        click(&ctx, &mut e, &format!("Circle · {}", short(curve)));
        enter_field(&ctx, &mut e, "Radius mm", "6.75", false);
        click(&ctx, &mut e, "Add radius");
        enter_field(&ctx, &mut e, "Fixed X (mm):", "-3.5", false);
        enter_field(&ctx, &mut e, "Fixed Y (mm):", "4.25", false);
        click(&ctx, &mut e, "Add Fixed centre");
        click(&ctx, &mut e, "Save constraints copy…");
        let mut request = e.take_request().expect("widget request");
        let additions = request
            .edits
            .add
            .iter()
            .map(peer_addition)
            .collect::<Vec<_>>()
            .join(",");
        let ui = root.path().join("worker.fcad");
        request.destination = ui.clone();
        let mut state = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start_constraints(request, move |r, g, c| {
                crate::edits::spawn_constraint_edit(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (g, result) = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("worker response");
        assert_eq!(
            result
                .as_ref()
                .expect("published")
                .solve
                .as_ref()
                .expect("solve")
                .degrees_of_freedom(),
            0
        );
        assert_eq!(finish_edit(&mut e, &mut state, g, result), Some(ui.clone()));
        assert!(!e.active());
        let input = root.path().join("request.json");
        std::fs::write(
            &input,
            format!(r#"{{"request_version":1,"remove":[],"add":[{additions}]}}"#),
        )
        .expect("input");
        let peer = root.path().join("peer.fcad");
        let result = std::process::Command::new(crate::creates::tests::ferritecad())
            .arg("edit-sketch-constraints-copy")
            .arg(&path)
            .arg("--sketch")
            .arg(choice.sketch.to_string())
            .arg("--expect-version")
            .arg(source.version.content.to_string())
            .arg("--request")
            .arg(input)
            .arg("-o")
            .arg(&peer)
            .arg("--json")
            .output()
            .expect("peer");
        assert!(result.status.success(), "{result:?}");
        let a = Document::open_read_only(&ui).expect("worker copy");
        let b = Document::open_read_only(&peer).expect("CLI copy");
        assert_eq!(a.meta().document_id, b.meta().document_id);
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
        assert_eq!(
            a.dependencies().expect("dependencies"),
            b.dependencies().expect("dependencies")
        );
        for mut left in a.objects().expect("objects") {
            let right = b.object(left.id).expect("read").expect("same object");
            if let (
                ferritecad_document::ObjectPayload::Sketch(s),
                ferritecad_document::ObjectPayload::Sketch(t),
            ) = (&mut left.payload, &right.payload)
            {
                assert_eq!(s.curves, choice.stored.as_ref().expect("stored").curves);
                assert_eq!(s.constraints.len(), 2);
                for (x, y) in s.constraints.iter_mut().zip(&t.constraints) {
                    assert_ne!(x.id, y.id, "only newly minted UUIDs differ");
                    x.id = y.id;
                }
                assert_eq!(s, t);
            } else {
                assert_eq!(left, right);
            }
        }
        a.close().expect("close");
        b.close().expect("close");
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

    /// Two copies of the same document, published two ways, are one document.
    ///
    /// The newly minted constraint UUIDs are the only thing allowed to differ,
    /// and they are matched off one by one rather than ignored, so a copy with
    /// a different *number* of constraints still fails.
    fn same_publication(
        ui: &Path,
        peer: &Path,
        stored_curves: &[ferritecad_document::SketchCurve],
        constraints: usize,
    ) {
        let a = Document::open_read_only(ui).expect("worker copy");
        let b = Document::open_read_only(peer).expect("CLI copy");
        assert_eq!(a.meta().document_id, b.meta().document_id);
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
        assert_eq!(
            a.dependencies().expect("dependencies"),
            b.dependencies().expect("dependencies")
        );
        for mut left in a.objects().expect("objects") {
            let right = b.object(left.id).expect("read").expect("same object");
            if let (
                ferritecad_document::ObjectPayload::Sketch(s),
                ferritecad_document::ObjectPayload::Sketch(t),
            ) = (&mut left.payload, &right.payload)
            {
                assert_eq!(s.curves, stored_curves, "the saved guess moved");
                assert_eq!(s.constraints.len(), constraints);
                assert_eq!(t.constraints.len(), constraints);
                for (x, y) in s.constraints.iter_mut().zip(&t.constraints) {
                    if x.id != y.id {
                        x.id = y.id;
                    }
                }
                assert_eq!(s, t);
            } else {
                assert_eq!(left, right);
            }
        }
        a.close().expect("close");
        b.close().expect("close");
        for format in ["stl", "fbx"] {
            let mut exports = Vec::new();
            for model in [ui, peer] {
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
    }

    /// The same request, run once through the app's own worker and once through
    /// the shipped CLI, publishes the same ring — added into an empty draft and
    /// again as a removal of rows that were already persisted.
    #[test]
    fn native_annular_constraint_worker_and_cli_publish_the_same_ring() {
        if !ferritecad_occt::is_available() || !ferritecad_sketch_solver::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            assert_ne!(
                std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
                Ok("1")
            );
            eprintln!("skipped: annular constraint worker needs OCCT and planegcs");
            return;
        }
        let (root, path, source) = annulus_fixture();
        let before = std::fs::read(&path).expect("source");
        let choice = &source.constraint_sketches[0];
        let stored = choice.stored.clone().expect("a ring");
        let (boundary, bore) =
            ferritecad_document::constraint_circle_roles(&stored).expect("roles");
        let boundary_row = format!("Boundary circle · {}", short(boundary));
        let bore_row = format!("Bore circle · {}", short(bore));

        // One pass of the form, exactly as a person drives it.
        let mut e = Editor::default();
        let ctx = egui::Context::default();
        assert!(e.begin(&path, &source, choice.sketch));
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        click(&ctx, &mut e, &boundary_row);
        click(&ctx, &mut e, "Pair Circle A");
        click(&ctx, &mut e, &bore_row);
        click(&ctx, &mut e, "Pair Circle B");
        click(&ctx, &mut e, "Add Concentric");
        click(&ctx, &mut e, &boundary_row);
        enter_field(&ctx, &mut e, "Radius mm", "6.75", false);
        click(&ctx, &mut e, "Add radius");
        click(&ctx, &mut e, &bore_row);
        enter_field(&ctx, &mut e, "Radius mm", "2.125", false);
        click(&ctx, &mut e, "Add radius");
        click(&ctx, &mut e, &boundary_row);
        enter_field(&ctx, &mut e, "Fixed X (mm):", "-3.5", false);
        enter_field(&ctx, &mut e, "Fixed Y (mm):", "4.25", false);
        click(&ctx, &mut e, "Add Fixed centre");
        click(&ctx, &mut e, "Save constraints copy…");

        let run = |e: &mut Editor,
                   request: EditSketchConstraintsRequest,
                   ui: &Path,
                   peer: &Path,
                   source_path: &Path,
                   sketch: ObjectId,
                   version: &ferritecad_document::DocumentVersion,
                   dof: Option<usize>| {
            let additions = request
                .edits
                .add
                .iter()
                .map(peer_addition)
                .collect::<Vec<_>>()
                .join(",");
            let removals = request
                .edits
                .remove
                .iter()
                .map(|id| format!("\"{id}\""))
                .collect::<Vec<_>>()
                .join(",");
            let mut request = request;
            request.destination = ui.to_path_buf();
            let mut state = crate::edits::Edits::default();
            let (tx, rx) = std::sync::mpsc::channel();
            state
                .start_constraints(request, move |r, g, c| {
                    crate::edits::spawn_constraint_edit(r, c, move |result| {
                        tx.send((g, result)).expect("reply")
                    })
                })
                .expect("worker");
            let (g, result) = rx
                .recv_timeout(std::time::Duration::from_secs(60))
                .expect("worker response");
            assert_eq!(
                result
                    .as_ref()
                    .expect("published")
                    .solve
                    .as_ref()
                    .map(|s| s.degrees_of_freedom()),
                dof
            );
            assert_eq!(
                finish_edit(e, &mut state, g, result),
                Some(ui.to_path_buf())
            );
            let input = peer.with_extension("json");
            std::fs::write(
                &input,
                format!(r#"{{"request_version":1,"remove":[{removals}],"add":[{additions}]}}"#),
            )
            .expect("input");
            let out = std::process::Command::new(crate::creates::tests::ferritecad())
                .arg("edit-sketch-constraints-copy")
                .arg(source_path)
                .arg("--sketch")
                .arg(sketch.to_string())
                .arg("--expect-version")
                .arg(version.content.to_string())
                .arg("--request")
                .arg(input)
                .arg("-o")
                .arg(peer)
                .arg("--json")
                .output()
                .expect("peer");
            assert!(out.status.success(), "{out:?}");
        };

        let ui = root.path().join("worker.fcad");
        let peer = root.path().join("peer.fcad");
        let built = e.take_request().expect("widget request");
        run(
            &mut e,
            built,
            &ui,
            &peer,
            &path,
            choice.sketch,
            &source.version,
            Some(0),
        );
        assert!(!e.active());
        same_publication(&ui, &peer, &stored.curves, 4);
        assert_eq!(std::fs::read(&path).expect("source"), before);

        // Now the half review found missing last time: the rows are already
        // persisted, and what is driven is their removal and replacement.
        let published = Document::open_read_only(&ui).expect("published copy");
        let reading = ExtrudeEditSource::read(&published).expect("snapshot");
        published.close().expect("close");
        let saved = reading.constraint_sketches[0]
            .stored
            .clone()
            .expect("a constrained ring");
        let concentric = saved
            .constraints
            .iter()
            .find(|c| {
                matches!(c.rule, SketchConstraintRule::Coincident { a, b }
                    if a.at == ferritecad_document::SketchPointSelector::Center
                        && b.at == ferritecad_document::SketchPointSelector::Center)
            })
            .expect("the shared centre")
            .id;
        let pin = saved
            .constraints
            .iter()
            .find(|c| matches!(c.rule, SketchConstraintRule::Fixed { .. }))
            .expect("the pin")
            .id;
        let mut e = Editor::default();
        let ctx = egui::Context::default();
        assert!(e.begin(&ui, &reading, reading.constraint_sketches[0].sketch));
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        click_remove_on_row(&ctx, &mut e, &format!("Concentric · {concentric}"));
        click_remove_on_row(
            &ctx,
            &mut e,
            &format!("Fixed centre (-3.5, 4.25) mm · {pin}"),
        );
        click(&ctx, &mut e, "Save constraints copy…");
        let loosened = e.take_request().expect("removal request");
        assert_eq!(loosened.edits.remove, vec![concentric, pin]);
        assert!(loosened.edits.add.is_empty());
        let ui2 = root.path().join("worker-freed.fcad");
        let peer2 = root.path().join("peer-freed.fcad");
        run(
            &mut e,
            loosened,
            &ui2,
            &peer2,
            &ui,
            reading.constraint_sketches[0].sketch,
            &reading.version,
            Some(4),
        );
        same_publication(&ui2, &peer2, &stored.curves, 2);
    }

    #[test]
    fn fixed_point_widgets_pick_an_endpoint_and_replace_the_stored_pin_in_one_step() {
        let (_root, path, source) = fixture();
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, source.constraint_sketches[0].sketch));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        let curves = source.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored")
            .curves
            .clone();
        let SketchGeometry::Line { start, end } = curves[0].geometry else {
            panic!("line")
        };
        // The endpoint choice reads stored inputs, never a solved drawing.
        click(&ctx, &mut e, "Segment 1");
        assert!(painted(
            &frame(&ctx, &mut e, vec![]),
            &format!(
                "Stored start of the selected Line: ({}, {}) mm",
                start.x, start.y
            )
        ));
        click(&ctx, &mut e, "Pin End");
        assert!(painted(
            &frame(&ctx, &mut e, vec![]),
            &format!("Stored end of the selected Line: ({}, {}) mm", end.x, end.y)
        ));
        let empty = history_state(&e);
        enter_field(&ctx, &mut e, "Fixed X (mm):", "10", false);
        enter_field(&ctx, &mut e, "Fixed Y (mm):", "-5", false);
        assert_eq!(history_state(&e), empty, "typing is not a checkpoint");
        click(&ctx, &mut e, "Add Fixed point");
        let accepted = history_state(&e).0;
        assert_eq!(
            accepted.add,
            vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                curve: curves[0].id,
                kind: pin(LineEndpoint::End, 10., -5.),
            })]
        );
        assert!(
            painted(
                &frame(&ctx, &mut e, vec![]),
                &format!("Fixed point end (10, -5) mm · Line {}", curves[0].id)
            ),
            "the pending entry names the pin, not a closure fallback label"
        );

        // A second pin on the same profile is refused by the shared validator.
        let after_accept = history_state(&e);
        click(&ctx, &mut e, "Add Fixed point");
        assert!(
            e.draft
                .as_ref()
                .expect("draft")
                .refusal
                .as_deref()
                .is_some_and(|r| r.contains("one fixed endpoint")),
            "the validator refuses a second pin"
        );
        assert_eq!(history_state(&e), after_accept);

        // Invalid coordinates refuse without touching history or the Redo branch.
        click(&ctx, &mut e, "Undo");
        let branch = history_state(&e);
        for (x, y) in [
            ("", "0"),
            ("0", ""),
            ("-", "0"),
            ("0", "1e-"),
            ("NaN", "0"),
            ("0", "inf"),
            ("-inf", "0"),
            ("1e309", "0"),
            ("0", "-1e309"),
            ("10 mm", "0"),
        ] {
            enter_field(&ctx, &mut e, "Fixed X (mm):", x, false);
            enter_field(&ctx, &mut e, "Fixed Y (mm):", y, false);
            click(&ctx, &mut e, "Add Fixed point");
            assert!(
                e.draft.as_ref().expect("draft").refusal.is_some(),
                "{x}/{y}"
            );
            assert_eq!(history_state(&e), branch, "{x}/{y}");
        }
        let fields = {
            let d = e.draft.as_ref().expect("draft");
            (d.pin_x_mm.clone(), d.pin_y_mm.clone())
        };
        // A running job blocks the action and keeps the unapplied field.
        enter_field(&ctx, &mut e, "Fixed X (mm):", "3", true);
        click_running(&ctx, &mut e, "Add Fixed point", true);
        assert_eq!(
            (
                e.draft.as_ref().expect("draft").pin_x_mm.clone(),
                e.draft.as_ref().expect("draft").pin_y_mm.clone()
            ),
            fields
        );
        assert_eq!(history_state(&e), branch);
        click(&ctx, &mut e, "Redo");
        assert_eq!(
            history_state(&e).0,
            accepted,
            "Redo survives refusal and no-op"
        );
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(
            e.take_request().expect("Save accepted edits").edits,
            accepted
        );

        // Zero and negative millimetres are ordinary coordinates; a real new
        // edit after Undo is what clears the Redo branch.
        click(&ctx, &mut e, "Clear pending changes");
        enter_field(&ctx, &mut e, "Fixed X (mm):", "0", false);
        enter_field(&ctx, &mut e, "Fixed Y (mm):", "-0", false);
        click(&ctx, &mut e, "Add Fixed point");
        assert_eq!(
            kind_of(&history_state(&e).0.add[0]),
            pin(LineEndpoint::End, 0., 0.),
            "both zeros are one coordinate"
        );
        assert!(history_state(&e).2.is_empty(), "an actual edit clears Redo");

        // A persisted pin is named, shows its coordinates and its UUID, and is
        // removed by exact identity in the same request that adds the new one.
        let (stored_source, pin_id, pinned_curve) = persist_pin(&path);
        let mut e = Editor::default();
        assert!(e.begin(
            &path,
            &stored_source,
            stored_source.constraint_sketches[0].sketch
        ));
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        assert!(painted(
            &frame(&ctx, &mut e, vec![]),
            &format!("Fixed point end (12.5, -0.5) mm · {pin_id}")
        ));
        click_remove_on_row(&ctx, &mut e, "Fixed point end (12.5, -0.5) mm");
        assert_eq!(history_state(&e).0.remove, vec![pin_id]);
        click(&ctx, &mut e, "Segment 3");
        click(&ctx, &mut e, "Pin Start");
        enter_field(&ctx, &mut e, "Fixed X (mm):", "-7.5", false);
        enter_field(&ctx, &mut e, "Fixed Y (mm):", "4", false);
        click(&ctx, &mut e, "Add Fixed point");
        let replacement = history_state(&e).0;
        assert_eq!(replacement.remove, vec![pin_id]);
        assert_eq!(
            replacement.add,
            vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                curve: stored_source.constraint_sketches[0]
                    .stored
                    .as_ref()
                    .expect("stored")
                    .curves[2]
                    .id,
                kind: pin(LineEndpoint::Start, -7.5, 4.),
            })]
        );
        assert_ne!(curve_of(&replacement.add[0]), pinned_curve);
        stored_source.constraint_sketches[0]
            .validate_edits(&replacement)
            .expect("remove exact UUID and add in one request");
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, replacement);
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(e.take_request().expect("Save").edits, replacement);

        // Cancel keeps nothing behind and the editor closes.
        click(&ctx, &mut e, "Cancel constraints draft");
        assert!(!e.active());
    }
    fn persist_equality(path: &Path) -> (ExtrudeEditSource, StableEntityId, [StableEntityId; 2]) {
        let mut document = Document::open(path).expect("doc");
        let source = ExtrudeEditSource::read(&document).expect("catalog");
        let choice = &source.constraint_sketches[0];
        let curves = &choice.stored.as_ref().expect("stored").curves;
        let pair = [curves[1].id, curves[2].id];
        let prepared = ferritecad_document::prepare_sketch_constraints(
            &document,
            choice.sketch,
            &SketchConstraintEdits {
                remove: vec![],
                add: vec![AddSketchConstraint::Line(AddLineConstraint::EqualLength {
                    a: pair[0],
                    b: pair[1],
                })],
            },
        )
        .expect("prepare equality");
        let id = prepared.added.last().expect("equality").id;
        document
            .write_sketch_constraints(&prepared)
            .expect("write equality");
        let source = ExtrudeEditSource::read(&document).expect("catalog");
        document.close().expect("close");
        (source, id, pair)
    }

    #[test]
    fn equal_length_widgets_pick_two_lines_and_name_the_persisted_pair() {
        let (_root, path, source) = fixture();
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, source.constraint_sketches[0].sketch));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        let curves: Vec<_> = source.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored")
            .curves
            .iter()
            .map(|c| c.id)
            .collect();
        assert!(
            painted(&frame(&ctx, &mut e, vec![]), "Pair: A none · B none"),
            "the pair is shown before either side is picked"
        );

        // Picking either side is a selection, and selections are not history.
        let empty = history_state(&e);
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Pair Line A");
        click(&ctx, &mut e, "Segment 2");
        click(&ctx, &mut e, "Pair Line B");
        assert_eq!(
            history_state(&e),
            empty,
            "picking a Line is not a checkpoint"
        );
        assert!(painted(
            &frame(&ctx, &mut e, vec![]),
            &format!("Pair: A {} · B {}", short(curves[0]), short(curves[1]))
        ));

        click(&ctx, &mut e, "Add Equal length");
        let accepted = history_state(&e).0;
        assert_eq!(
            accepted.add,
            vec![AddSketchConstraint::Line(AddLineConstraint::EqualLength {
                a: curves[0],
                b: curves[1],
            })]
        );
        assert!(
            painted(
                &frame(&ctx, &mut e, vec![]),
                &format!("Equal length · Lines {} = {}", curves[0], curves[1])
            ),
            "the pending entry names both Lines, not a closure fallback label"
        );

        // The same pair either way round, and a Line paired with itself, are
        // refused by the shared validator without touching history or Redo.
        let after_accept = history_state(&e);
        for (a, b) in [(0, 1), (1, 0), (0, 0)] {
            click(&ctx, &mut e, &format!("Segment {}", a + 1));
            click(&ctx, &mut e, "Pair Line A");
            click(&ctx, &mut e, &format!("Segment {}", b + 1));
            click(&ctx, &mut e, "Pair Line B");
            click(&ctx, &mut e, "Add Equal length");
            assert!(
                e.draft.as_ref().expect("draft").refusal.is_some(),
                "({a}, {b}) must be refused"
            );
            assert_eq!(history_state(&e), after_accept, "({a}, {b})");
        }

        // A running job blocks the action and keeps both picks.
        click_running(&ctx, &mut e, "Add Equal length", true);
        assert_eq!(history_state(&e), after_accept);
        click(&ctx, &mut e, "Undo");
        let branch = history_state(&e);
        assert_eq!(branch.0, SketchConstraintEdits::default());
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, accepted, "Redo survives refusal");
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(
            e.take_request().expect("Save accepted edits").edits,
            accepted
        );

        // A persisted equality is named as one, shows both Lines and its UUID,
        // and is removed by exact identity in the same request as a new pair.
        let (stored_source, equal_id, pair) = persist_equality(&path);
        let mut e = Editor::default();
        assert!(e.begin(
            &path,
            &stored_source,
            stored_source.constraint_sketches[0].sketch
        ));
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        let row = format!("Equal length · {equal_id}");
        scroll_to_row(&ctx, &mut e, &row);
        let out = frame(&ctx, &mut e, vec![]);
        assert!(painted(&out, &row), "persisted equality names itself");
        assert!(
            painted(&out, &two_lines(pair[0], pair[1], "=")),
            "persisted equality names both of its Lines"
        );
        click_remove_on_row(&ctx, &mut e, &row);
        assert_eq!(history_state(&e).0.remove, vec![equal_id]);
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Pair Line A");
        click(&ctx, &mut e, "Segment 4");
        click(&ctx, &mut e, "Pair Line B");
        click(&ctx, &mut e, "Add Equal length");
        let replacement = history_state(&e).0;
        assert_eq!(replacement.remove, vec![equal_id]);
        assert_eq!(
            replacement.add,
            vec![AddSketchConstraint::Line(AddLineConstraint::EqualLength {
                a: curves[0],
                b: curves[3],
            })]
        );
        stored_source.constraint_sketches[0]
            .validate_edits(&replacement)
            .expect("remove exact UUID and add one pair in one request");
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, replacement);
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(e.take_request().expect("Save").edits, replacement);
        click(&ctx, &mut e, "Cancel constraints draft");
        assert!(!e.active());
    }

    fn persist_relation(
        path: &Path,
        relation: LineRelation,
    ) -> (ExtrudeEditSource, StableEntityId, [StableEntityId; 2]) {
        let mut document = Document::open(path).expect("doc");
        let source = ExtrudeEditSource::read(&document).expect("catalog");
        let choice = &source.constraint_sketches[0];
        let curves = &choice.stored.as_ref().expect("stored").curves;
        let pair = [curves[1].id, curves[2].id];
        let prepared = ferritecad_document::prepare_sketch_constraints(
            &document,
            choice.sketch,
            &SketchConstraintEdits {
                remove: vec![],
                add: vec![AddSketchConstraint::Line(AddLineConstraint::Relation {
                    a: pair[0],
                    b: pair[1],
                    relation,
                })],
            },
        )
        .expect("prepare relation");
        let id = prepared.added.last().expect("relation").id;
        document
            .write_sketch_constraints(&prepared)
            .expect("write relation");
        let source = ExtrudeEditSource::read(&document).expect("catalog");
        document.close().expect("close");
        (source, id, pair)
    }

    #[test]
    fn line_relation_widgets_pick_two_lines_and_name_the_persisted_relation() {
        let (_root, path, source) = fixture();
        let mut e = Editor::default();
        assert!(e.begin(&path, &source, source.constraint_sketches[0].sketch));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        let curves: Vec<_> = source.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored")
            .curves
            .iter()
            .map(|c| c.id)
            .collect();

        // The pair is picked once, from the one stored catalogue, and serves
        // every relationship; picking is selection, and selection is not history.
        let empty = history_state(&e);
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Pair Line A");
        click(&ctx, &mut e, "Segment 2");
        click(&ctx, &mut e, "Pair Line B");
        assert_eq!(
            history_state(&e),
            empty,
            "picking a Line is not a checkpoint"
        );
        assert!(painted(
            &frame(&ctx, &mut e, vec![]),
            &format!("Pair: A {} · B {}", short(curves[0]), short(curves[1]))
        ));

        click(&ctx, &mut e, "Add Parallel");
        let accepted = history_state(&e).0;
        assert_eq!(
            accepted.add,
            vec![AddSketchConstraint::Line(AddLineConstraint::Relation {
                a: curves[0],
                b: curves[1],
                relation: LineRelation::Parallel,
            })]
        );
        assert!(
            painted(
                &frame(&ctx, &mut e, vec![]),
                &format!("Parallel · Lines {} and {}", curves[0], curves[1])
            ),
            "the pending entry names the relation and both Lines"
        );

        // One question per pair: the other answer, the same answer again, either
        // way round, and a Line paired with itself are refused by the shared
        // validator without touching history or Redo.
        let after_accept = history_state(&e);
        for (a, b, label) in [
            (0, 1, "Add Perpendicular"),
            (1, 0, "Add Perpendicular"),
            (0, 1, "Add Parallel"),
            (1, 0, "Add Parallel"),
            (0, 0, "Add Parallel"),
            (0, 0, "Add Perpendicular"),
        ] {
            click(&ctx, &mut e, &format!("Segment {}", a + 1));
            click(&ctx, &mut e, "Pair Line A");
            click(&ctx, &mut e, &format!("Segment {}", b + 1));
            click(&ctx, &mut e, "Pair Line B");
            click(&ctx, &mut e, label);
            assert!(
                e.draft.as_ref().expect("draft").refusal.is_some(),
                "{label} ({a}, {b}) must be refused"
            );
            assert_eq!(history_state(&e), after_accept, "{label} ({a}, {b})");
        }

        // A running job blocks the action and keeps both picks.
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Pair Line A");
        click(&ctx, &mut e, "Segment 2");
        click(&ctx, &mut e, "Pair Line B");
        click_running(&ctx, &mut e, "Add Perpendicular", true);
        assert_eq!(history_state(&e), after_accept);
        click(&ctx, &mut e, "Undo");
        assert_eq!(history_state(&e).0, SketchConstraintEdits::default());
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, accepted, "Redo survives refusal");

        // An equal length on the very same pair is a different property, so the
        // same two picks may also be asked for it.
        click(&ctx, &mut e, "Add Equal length");
        let with_equality = history_state(&e).0;
        assert_eq!(
            with_equality.add,
            [
                accepted.add.clone(),
                vec![AddSketchConstraint::Line(AddLineConstraint::EqualLength {
                    a: curves[0],
                    b: curves[1],
                })]
            ]
            .concat()
        );
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(
            e.take_request().expect("Save accepted edits").edits,
            with_equality
        );

        // A persisted relation is named by its kind, shows both Lines and its
        // UUID, and is removed by exact identity in the same request as the
        // other answer for that pair.
        let (stored_source, relation_id, pair) =
            persist_relation(&path, LineRelation::Perpendicular);
        let mut e = Editor::default();
        assert!(e.begin(
            &path,
            &stored_source,
            stored_source.constraint_sketches[0].sketch
        ));
        for _ in 0..3 {
            frame(&ctx, &mut e, vec![]);
        }
        let row = format!("Perpendicular · {relation_id}");
        scroll_to_row(&ctx, &mut e, &row);
        let out = frame(&ctx, &mut e, vec![]);
        assert!(painted(&out, &row), "persisted relation names itself");
        assert!(
            painted(&out, &two_lines(pair[0], pair[1], "and")),
            "persisted relation names both of its Lines"
        );
        click_remove_on_row(&ctx, &mut e, &row);
        assert_eq!(history_state(&e).0.remove, vec![relation_id]);
        click(&ctx, &mut e, "Segment 2");
        click(&ctx, &mut e, "Pair Line A");
        click(&ctx, &mut e, "Segment 3");
        click(&ctx, &mut e, "Pair Line B");
        click(&ctx, &mut e, "Add Parallel");
        let replacement = history_state(&e).0;
        assert_eq!(replacement.remove, vec![relation_id]);
        assert_eq!(
            replacement.add,
            vec![AddSketchConstraint::Line(AddLineConstraint::Relation {
                a: pair[0],
                b: pair[1],
                relation: LineRelation::Parallel,
            })]
        );
        stored_source.constraint_sketches[0]
            .validate_edits(&replacement)
            .expect("remove exact UUID and add the other answer in one request");
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Redo");
        assert_eq!(history_state(&e).0, replacement);
        click(&ctx, &mut e, "Save constraints copy…");
        assert_eq!(e.take_request().expect("Save").edits, replacement);
        click(&ctx, &mut e, "Cancel constraints draft");
        assert!(!e.active());
    }

    #[test]
    fn native_fixed_point_worker_and_cli_pin_the_same_solved_body() {
        use crate::creates::tests::ferritecad;
        if !ferritecad_occt::is_available() || !ferritecad_sketch_solver::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: constraint worker requires OCCT and PlaneGCS");
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let source = root.path().join("source.fcad");
        let input = root.path().join("request.json");
        std::fs::write(
            &input,
            r#"{"request_version":1,"points_mm":[[-40,-20],[40,-20],[40,20],[-40,20]],"height_mm":10}"#,
        )
        .expect("polygon");
        let o = std::process::Command::new(ferritecad())
            .arg("create-sketch-extrude")
            .arg(&input)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(o.status.success(), "{o:?}");
        crate::sketch::drag_tests::attach_source_claim(&source);
        let sql_before = crate::sketch::drag_tests::sql_facts(&source);
        let loaded = {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                &source,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("scene")
        };
        let reading = loaded.edit_source.expect("catalog");
        let id = reading.constraint_sketches[0].sketch;
        let curves = reading.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored")
            .curves
            .clone();
        let original = std::fs::read(&source).expect("source");
        let mut e = Editor::default();
        assert!(e.begin(&source, &reading, id));
        let ctx = egui::Context::default();
        for _ in 0..2 {
            frame(&ctx, &mut e, vec![]);
        }
        for (segment, label) in [
            ("Segment 1", "Add Horizontal"),
            ("Segment 2", "Add Vertical"),
            ("Segment 3", "Add Horizontal"),
            ("Segment 4", "Add Vertical"),
        ] {
            click(&ctx, &mut e, segment);
            click(&ctx, &mut e, label);
        }
        for (segment, value) in [("Segment 1", "60"), ("Segment 2", "30")] {
            click(&ctx, &mut e, segment);
            enter_length(&ctx, &mut e, value, false);
            click(&ctx, &mut e, "Add length");
        }
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Pin Start");
        enter_field(&ctx, &mut e, "Fixed X (mm):", "10", false);
        enter_field(&ctx, &mut e, "Fixed Y (mm):", "-5", false);
        click(&ctx, &mut e, "Add Fixed point");
        let expected_edits = e.draft.as_ref().expect("draft").edits.clone();
        assert_eq!(
            expected_edits.add.last().expect("pin"),
            &AddSketchConstraint::Line(AddLineConstraint::Line {
                curve: curves[0].id,
                kind: pin(LineEndpoint::Start, 10., -5.),
            })
        );
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("real widget request");
        assert_eq!(request.edits, expected_edits);
        assert_eq!(request.expected, reading.version);
        let mut state = crate::edits::Edits::default();
        let mut r = request.clone();
        r.destination = root.path().join("ui.fcad");
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start_constraints(r, move |r, g, c| {
                crate::edits::spawn_constraint_edit(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (g, result) = rx.recv().expect("worker reply");
        let solved = result
            .as_ref()
            .expect("published")
            .solve
            .clone()
            .expect("a Line profile keeps its closure, so there is always a solve");
        assert_eq!(
            solved.degrees_of_freedom(),
            0,
            "the pin removed both translations"
        );
        let ui = root.path().join("ui.fcad");
        assert_eq!(finish_edit(&mut e, &mut state, g, result), Some(ui.clone()));
        assert!(!e.active());

        // The peer CLI process replays the same ordered request over one source.
        let additions = expected_edits
            .add
            .iter()
            .map(peer_addition)
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            &input,
            format!(r#"{{"request_version":1,"remove":[],"add":[{additions}]}}"#),
        )
        .expect("typed IDs to peer request");
        let cli = root.path().join("cli.fcad");
        let o = std::process::Command::new(ferritecad())
            .arg("edit-sketch-constraints-copy")
            .arg(&source)
            .arg("--sketch")
            .arg(id.to_string())
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(o.status.success(), "{o:?}");

        let a = Document::open_read_only(&ui).expect("UI");
        let b = Document::open_read_only(&cli).expect("CLI");
        assert_eq!(a.meta(), b.meta());
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
        let mut pin_id = None;
        for old in a.objects().expect("objects") {
            let new = b.object(old.id).expect("read").expect("same id");
            if let (
                ferritecad_document::ObjectPayload::Sketch(s),
                ferritecad_document::ObjectPayload::Sketch(t),
            ) = (&old.payload, &new.payload)
            {
                assert_eq!(s.curves, t.curves, "stored coordinates are inputs");
                assert_eq!(s.curves, curves, "the solve did not write itself back");
                assert_eq!(s.constraints.len(), 11);
                for (x, y) in s.constraints.iter().zip(&t.constraints) {
                    assert_ne!(x.id, y.id, "only genuinely new UUIDs differ");
                    assert_eq!(x.rule, y.rule);
                }
                let last = s.constraints.last().expect("pin");
                assert_eq!(
                    last.rule,
                    SketchConstraintRule::Fixed {
                        point: ferritecad_document::SketchPointRef::new(
                            curves[0].id,
                            ferritecad_document::SketchPointSelector::Start,
                        ),
                        x: 10.,
                        y: -5.,
                    }
                );
                pin_id = Some(last.id);
            } else {
                assert_eq!(old, new);
            }
        }
        a.close().expect("close");
        b.close().expect("close");
        for path in [&ui, &cli] {
            let after = crate::sketch::drag_tests::sql_facts(path);
            assert_eq!(
                sql_before.keys().collect::<Vec<_>>(),
                after.keys().collect::<Vec<_>>()
            );
            for (table, rows) in &sql_before {
                if table == "objects" {
                    for row in rows {
                        let actual = after[table]
                            .iter()
                            .find(|r| r[1] == row[1])
                            .expect("same row");
                        for col in 0..row.len() {
                            if row[1] == rusqlite::types::Value::Blob(id.to_bytes().to_vec())
                                && [3, 7, 8].contains(&col)
                            {
                                continue;
                            }
                            assert_eq!(row[col], actual[col], "object cell {col}");
                        }
                    }
                } else if table == "capabilities" {
                    assert_eq!(
                        after[table]
                            .iter()
                            .filter(|r| r[1]
                                != rusqlite::types::Value::Text(
                                    ferritecad_document::SKETCH_CONSTRAINTS_CAPABILITY.into()
                                ))
                            .cloned()
                            .collect::<Vec<_>>(),
                        *rows
                    );
                } else {
                    assert_eq!(
                        &after[table], rows,
                        "{table}: source claims and unrelated rows"
                    );
                }
            }
        }

        let mut outputs = vec![];
        for path in [&ui, &cli] {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                path,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("async Open route");
            let stl = path.with_extension("stl");
            let fbx = path.with_extension("fbx");
            for (op, dest) in [("export-stl", &stl), ("export-fbx", &fbx)] {
                let o = std::process::Command::new(ferritecad())
                    .arg(op)
                    .arg(path)
                    .arg("-o")
                    .arg(dest)
                    .output()
                    .expect("export");
                assert!(o.status.success(), "{o:?}");
            }
            outputs.push(std::fs::read(&stl).expect("STL"));
            if let Some(dir) = std::env::var_os("FERRITECAD_CONSTRAINT_ARTIFACTS") {
                std::fs::create_dir_all(&dir).expect("dir");
                for p in [path.as_path(), stl.as_path(), fbx.as_path()] {
                    std::fs::copy(p, Path::new(&dir).join(p.file_name().expect("name")))
                        .expect("artifact");
                }
            }
        }
        assert_eq!(
            outputs[0], outputs[1],
            "UI and CLI pin the same solved Body"
        );
        assert_eq!(
            std::fs::read(ui.with_extension("fbx")).expect("UI FBX"),
            std::fs::read(cli.with_extension("fbx")).expect("CLI FBX"),
            "UI and CLI publish the same FBX for the pinned Body"
        );

        // Independent integration of the published triangles: size, volume and
        // where in XY the pinned vertex actually put the body.
        let stl = &outputs[0];
        let n = u32::from_le_bytes(stl[80..84].try_into().expect("count")) as usize;
        assert_eq!(stl.len(), 84 + 50 * n);
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut volume6 = 0.;
        for i in 0..n {
            let v: [f64; 9] = std::array::from_fn(|j| {
                let at = 84 + 50 * i + 12 + 4 * j;
                f64::from(f32::from_le_bytes(stl[at..at + 4].try_into().expect("f32")))
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
        let extents: [f64; 3] = std::array::from_fn(|j| hi[j] - lo[j]);
        assert!(
            (extents[0] - 60.).abs() < 1e-4
                && (extents[1] - 30.).abs() < 1e-4
                && (extents[2] - 10.).abs() < 1e-4,
            "{extents:?}"
        );
        assert!((volume6.abs() / 6. - 18000.).abs() < 0.02);
        assert!(
            (lo[0] - 10.).abs() < 1e-4 || (hi[0] - 10.).abs() < 1e-4,
            "pinned X is not a corner of the body: {lo:?} {hi:?}"
        );
        assert!(
            (lo[1] + 5.).abs() < 1e-4 || (hi[1] + 5.).abs() < 1e-4,
            "pinned Y is not a corner of the body: {lo:?} {hi:?}"
        );
        assert_eq!(std::fs::read(&source).expect("source"), original);

        // The persisted pin is named as a pin and removable by exact identity.
        let d = Document::open_read_only(&ui).expect("published");
        let after = ExtrudeEditSource::read(&d).expect("discover");
        d.close().expect("close");
        assert!(after.constraint_sketches[0].refusal.is_none());
        assert!(e.begin(&ui, &after, id));
        for _ in 0..2 {
            frame(&ctx, &mut e, vec![]);
        }
        let pin_row = format!("Fixed point start (10, -5) mm · {}", pin_id.expect("pin"));
        scroll_to_row(&ctx, &mut e, &pin_row);
        assert!(painted(&frame(&ctx, &mut e, vec![]), &pin_row));
        click_remove_on_row(&ctx, &mut e, "Fixed point start (10, -5) mm");
        click(&ctx, &mut e, "Save constraints copy…");
        let removal = e.take_request().expect("removal");
        assert_eq!(removal.edits.remove, vec![pin_id.expect("pin")]);
        assert!(removal.edits.add.is_empty());
    }

    #[test]
    fn native_equal_length_worker_and_cli_tie_the_same_solved_body() {
        use crate::creates::tests::ferritecad;
        if !ferritecad_occt::is_available() || !ferritecad_sketch_solver::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: constraint worker requires OCCT and PlaneGCS");
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let source = root.path().join("source.fcad");
        let input = root.path().join("request.json");
        std::fs::write(
            &input,
            r#"{"request_version":1,"points_mm":[[-40,-20],[40,-20],[40,20],[-40,20]],"height_mm":10}"#,
        )
        .expect("polygon");
        let o = std::process::Command::new(ferritecad())
            .arg("create-sketch-extrude")
            .arg(&input)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(o.status.success(), "{o:?}");
        crate::sketch::drag_tests::attach_source_claim(&source);
        let sql_before = crate::sketch::drag_tests::sql_facts(&source);
        let original = std::fs::read(&source).expect("source");
        let d = Document::open_read_only(&source).expect("doc");
        let reading = ExtrudeEditSource::read(&d).expect("catalog");
        d.close().expect("close");
        let id = reading.constraint_sketches[0].sketch;
        let curves = reading.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored")
            .curves
            .clone();
        let mut e = Editor::default();
        assert!(e.begin(&source, &reading, id));
        let ctx = egui::Context::default();
        for _ in 0..2 {
            frame(&ctx, &mut e, vec![]);
        }
        for (segment, label) in [
            ("Segment 1", "Add Horizontal"),
            ("Segment 2", "Add Vertical"),
            ("Segment 3", "Add Horizontal"),
            ("Segment 4", "Add Vertical"),
        ] {
            click(&ctx, &mut e, segment);
            click(&ctx, &mut e, label);
        }
        click(&ctx, &mut e, "Segment 1");
        enter_length(&ctx, &mut e, "60", false);
        click(&ctx, &mut e, "Add length");
        click(&ctx, &mut e, "Pin Start");
        enter_field(&ctx, &mut e, "Fixed X (mm):", "10", false);
        enter_field(&ctx, &mut e, "Fixed Y (mm):", "-5", false);
        click(&ctx, &mut e, "Add Fixed point");
        click(&ctx, &mut e, "Pair Line A");
        click(&ctx, &mut e, "Segment 2");
        click(&ctx, &mut e, "Pair Line B");
        click(&ctx, &mut e, "Add Equal length");
        let expected_edits = e.draft.as_ref().expect("draft").edits.clone();
        assert_eq!(
            expected_edits.add.last().expect("equality"),
            &AddSketchConstraint::Line(AddLineConstraint::EqualLength {
                a: curves[0].id,
                b: curves[1].id,
            })
        );
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("real widget request");
        assert_eq!(request.edits, expected_edits);
        assert_eq!(request.expected, reading.version);
        let mut state = crate::edits::Edits::default();
        let mut r = request.clone();
        let ui = root.path().join("equal-ui.fcad");
        r.destination = ui.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start_constraints(r, move |r, g, c| {
                crate::edits::spawn_constraint_edit(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (g, result) = rx.recv().expect("worker reply");
        let solved = result
            .as_ref()
            .expect("published")
            .solve
            .clone()
            .expect("a Line profile keeps its closure, so there is always a solve");
        assert_eq!(
            solved.degrees_of_freedom(),
            0,
            "the equality removed the last free dimension"
        );
        assert!(solved.redundant().is_empty());
        assert_eq!(finish_edit(&mut e, &mut state, g, result), Some(ui.clone()));
        assert!(!e.active());

        // The peer CLI process replays the same ordered request over one source.
        let additions = expected_edits
            .add
            .iter()
            .map(peer_addition)
            .collect::<Vec<_>>()
            .join(",");
        assert!(
            additions.contains(r#""rule":"equal_length""#),
            "{additions}"
        );
        std::fs::write(
            &input,
            format!(r#"{{"request_version":1,"remove":[],"add":[{additions}]}}"#),
        )
        .expect("typed IDs to peer request");
        let cli = root.path().join("equal-cli.fcad");
        let o = std::process::Command::new(ferritecad())
            .arg("edit-sketch-constraints-copy")
            .arg(&source)
            .arg("--sketch")
            .arg(id.to_string())
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(o.status.success(), "{o:?}");

        // Only genuinely new constraint UUIDs differ between the two routes.
        let a = Document::open_read_only(&ui).expect("UI");
        let b = Document::open_read_only(&cli).expect("CLI");
        assert_eq!(a.meta(), b.meta());
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
        for old in a.objects().expect("objects") {
            let new = b.object(old.id).expect("read").expect("same id");
            if let (
                ferritecad_document::ObjectPayload::Sketch(s),
                ferritecad_document::ObjectPayload::Sketch(t),
            ) = (&old.payload, &new.payload)
            {
                assert_eq!(s.curves, t.curves, "stored coordinates are inputs");
                assert_eq!(s.curves, curves, "the solve did not write itself back");
                assert_eq!(s.constraints.len(), 11);
                for (x, y) in s.constraints.iter().zip(&t.constraints) {
                    assert_ne!(x.id, y.id, "only genuinely new UUIDs differ");
                    assert_eq!(x.rule, y.rule);
                }
                assert_eq!(
                    s.constraints.last().expect("equality").rule,
                    SketchConstraintRule::EqualLength {
                        a: ferritecad_document::SketchSegmentRef::new(
                            ferritecad_document::SketchPointRef::new(
                                curves[0].id,
                                ferritecad_document::SketchPointSelector::Start,
                            ),
                            ferritecad_document::SketchPointRef::new(
                                curves[0].id,
                                ferritecad_document::SketchPointSelector::End,
                            ),
                        ),
                        b: ferritecad_document::SketchSegmentRef::new(
                            ferritecad_document::SketchPointRef::new(
                                curves[1].id,
                                ferritecad_document::SketchPointSelector::Start,
                            ),
                            ferritecad_document::SketchPointRef::new(
                                curves[1].id,
                                ferritecad_document::SketchPointSelector::End,
                            ),
                        ),
                    }
                );
            } else {
                assert_eq!(old, new);
            }
        }
        a.close().expect("close");
        b.close().expect("close");
        for path in [&ui, &cli] {
            let after = crate::sketch::drag_tests::sql_facts(path);
            assert_eq!(
                sql_before.keys().collect::<Vec<_>>(),
                after.keys().collect::<Vec<_>>()
            );
            for (table, rows) in &sql_before {
                if table == "objects" {
                    for row in rows {
                        let actual = after[table]
                            .iter()
                            .find(|r| r[1] == row[1])
                            .expect("same row");
                        for col in 0..row.len() {
                            if row[1] == rusqlite::types::Value::Blob(id.to_bytes().to_vec())
                                && [3, 7, 8].contains(&col)
                            {
                                continue;
                            }
                            assert_eq!(row[col], actual[col], "object cell {col}");
                        }
                    }
                } else if table == "capabilities" {
                    assert_eq!(
                        after[table]
                            .iter()
                            .filter(|r| r[1]
                                != rusqlite::types::Value::Text(
                                    ferritecad_document::SKETCH_CONSTRAINTS_CAPABILITY.into()
                                ))
                            .cloned()
                            .collect::<Vec<_>>(),
                        *rows
                    );
                } else {
                    assert_eq!(
                        &after[table], rows,
                        "{table}: source claims and unrelated rows"
                    );
                }
            }
        }

        let mut outputs = vec![];
        for path in [&ui, &cli] {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                path,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("async Open route");
            let stl = path.with_extension("stl");
            let fbx = path.with_extension("fbx");
            for (op, dest) in [("export-stl", &stl), ("export-fbx", &fbx)] {
                let o = std::process::Command::new(ferritecad())
                    .arg(op)
                    .arg(path)
                    .arg("-o")
                    .arg(dest)
                    .output()
                    .expect("export");
                assert!(o.status.success(), "{o:?}");
            }
            outputs.push(std::fs::read(&stl).expect("STL"));
            if let Some(dir) = std::env::var_os("FERRITECAD_CONSTRAINT_ARTIFACTS") {
                std::fs::create_dir_all(&dir).expect("dir");
                for p in [path.as_path(), stl.as_path(), fbx.as_path()] {
                    std::fs::copy(p, Path::new(&dir).join(p.file_name().expect("name")))
                        .expect("artifact");
                }
            }
        }
        assert_eq!(
            outputs[0], outputs[1],
            "UI and CLI tie the same solved Body"
        );
        assert_eq!(
            std::fs::read(ui.with_extension("fbx")).expect("UI FBX"),
            std::fs::read(cli.with_extension("fbx")).expect("CLI FBX"),
            "UI and CLI publish the same FBX for the tied Body"
        );

        // Independent integration of the published triangles: one length was
        // asked for, and the equality made the other side match it.
        let stl = &outputs[0];
        let n = u32::from_le_bytes(stl[80..84].try_into().expect("count")) as usize;
        assert_eq!(stl.len(), 84 + 50 * n);
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut volume6 = 0.;
        for i in 0..n {
            let v: [f64; 9] = std::array::from_fn(|j| {
                let at = 84 + 50 * i + 12 + 4 * j;
                f64::from(f32::from_le_bytes(stl[at..at + 4].try_into().expect("f32")))
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
        let extents: [f64; 3] = std::array::from_fn(|j| hi[j] - lo[j]);
        assert!(
            (extents[0] - 60.).abs() < 1e-4
                && (extents[1] - 60.).abs() < 1e-4
                && (extents[2] - 10.).abs() < 1e-4,
            "{extents:?}"
        );
        assert!((volume6.abs() / 6. - 36000.).abs() < 0.04);
        assert_eq!(std::fs::read(&source).expect("source"), original);
    }

    #[test]
    fn native_line_relations_worker_and_cli_orient_the_same_solved_body() {
        use crate::creates::tests::ferritecad;
        if !ferritecad_occt::is_available() || !ferritecad_sketch_solver::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: constraint worker requires OCCT and PlaneGCS");
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let source = root.path().join("source.fcad");
        let input = root.path().join("request.json");
        // A slanted stored profile: nothing here may quietly become H/V.
        std::fs::write(
            &input,
            r#"{"request_version":1,"points_mm":[[-20,-10],[40,-8],[42,30],[-20,30]],"height_mm":10}"#,
        )
        .expect("polygon");
        let o = std::process::Command::new(ferritecad())
            .arg("create-sketch-extrude")
            .arg(&input)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(o.status.success(), "{o:?}");
        crate::sketch::drag_tests::attach_source_claim(&source);
        let sql_before = crate::sketch::drag_tests::sql_facts(&source);
        let original = std::fs::read(&source).expect("source");
        let d = Document::open_read_only(&source).expect("doc");
        let reading = ExtrudeEditSource::read(&d).expect("catalog");
        d.close().expect("close");
        let id = reading.constraint_sketches[0].sketch;
        let curves = reading.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored")
            .curves
            .clone();
        let mut e = Editor::default();
        assert!(e.begin(&source, &reading, id));
        let ctx = egui::Context::default();
        for _ in 0..2 {
            frame(&ctx, &mut e, vec![]);
        }
        // Opposite sides parallel, one corner square, two sizes: a rectangle
        // asked for as relationships, with no Line's own direction named.
        for (a, b, label) in [
            (1, 3, "Add Parallel"),
            (2, 4, "Add Parallel"),
            (1, 2, "Add Perpendicular"),
        ] {
            click(&ctx, &mut e, &format!("Segment {a}"));
            click(&ctx, &mut e, "Pair Line A");
            click(&ctx, &mut e, &format!("Segment {b}"));
            click(&ctx, &mut e, "Pair Line B");
            click(&ctx, &mut e, label);
        }
        for (segment, mm) in [("Segment 1", "60"), ("Segment 2", "30")] {
            click(&ctx, &mut e, segment);
            enter_length(&ctx, &mut e, mm, false);
            click(&ctx, &mut e, "Add length");
        }
        let expected_edits = e.draft.as_ref().expect("draft").edits.clone();
        assert_eq!(
            expected_edits.add[..3],
            [
                AddSketchConstraint::Line(AddLineConstraint::Relation {
                    a: curves[0].id,
                    b: curves[2].id,
                    relation: LineRelation::Parallel,
                }),
                AddSketchConstraint::Line(AddLineConstraint::Relation {
                    a: curves[1].id,
                    b: curves[3].id,
                    relation: LineRelation::Parallel,
                }),
                AddSketchConstraint::Line(AddLineConstraint::Relation {
                    a: curves[0].id,
                    b: curves[1].id,
                    relation: LineRelation::Perpendicular,
                }),
            ]
        );
        click(&ctx, &mut e, "Undo");
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("real widget request");
        assert_eq!(request.edits, expected_edits);
        assert_eq!(request.expected, reading.version);
        let mut state = crate::edits::Edits::default();
        let mut r = request.clone();
        let ui = root.path().join("relation-ui.fcad");
        r.destination = ui.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start_constraints(r, move |r, g, c| {
                crate::edits::spawn_constraint_edit(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (g, result) = rx.recv().expect("worker reply");
        let solved = result
            .as_ref()
            .expect("published")
            .solve
            .clone()
            .expect("a Line profile keeps its closure, so there is always a solve");
        assert_eq!(
            solved.degrees_of_freedom(),
            3,
            "a rectangle of a known size is still free to sit and to turn"
        );
        assert!(solved.redundant().is_empty());
        assert_eq!(finish_edit(&mut e, &mut state, g, result), Some(ui.clone()));
        assert!(!e.active());

        // The peer CLI process replays the same ordered request over one source.
        let additions = expected_edits
            .add
            .iter()
            .map(peer_addition)
            .collect::<Vec<_>>()
            .join(",");
        for rule in [r#""rule":"parallel""#, r#""rule":"perpendicular""#] {
            assert!(additions.contains(rule), "{additions}");
        }
        std::fs::write(
            &input,
            format!(r#"{{"request_version":1,"remove":[],"add":[{additions}]}}"#),
        )
        .expect("typed IDs to peer request");
        let cli = root.path().join("relation-cli.fcad");
        let o = std::process::Command::new(ferritecad())
            .arg("edit-sketch-constraints-copy")
            .arg(&source)
            .arg("--sketch")
            .arg(id.to_string())
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(o.status.success(), "{o:?}");

        // Only genuinely new constraint UUIDs differ between the two routes.
        let a = Document::open_read_only(&ui).expect("UI");
        let b = Document::open_read_only(&cli).expect("CLI");
        assert_eq!(a.meta(), b.meta());
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
        let whole = |curve: &ferritecad_document::SketchCurve| {
            ferritecad_document::SketchSegmentRef::new(
                ferritecad_document::SketchPointRef::new(
                    curve.id,
                    ferritecad_document::SketchPointSelector::Start,
                ),
                ferritecad_document::SketchPointRef::new(
                    curve.id,
                    ferritecad_document::SketchPointSelector::End,
                ),
            )
        };
        for old in a.objects().expect("objects") {
            let new = b.object(old.id).expect("read").expect("same id");
            if let (
                ferritecad_document::ObjectPayload::Sketch(s),
                ferritecad_document::ObjectPayload::Sketch(t),
            ) = (&old.payload, &new.payload)
            {
                assert_eq!(s.curves, t.curves, "stored coordinates are inputs");
                assert_eq!(s.curves, curves, "the solve did not write itself back");
                assert_eq!(s.constraints.len(), 9);
                for (x, y) in s.constraints.iter().zip(&t.constraints) {
                    assert_ne!(x.id, y.id, "only genuinely new UUIDs differ");
                    assert_eq!(x.rule, y.rule);
                }
                assert_eq!(
                    s.constraints[4..7]
                        .iter()
                        .map(|c| c.rule)
                        .collect::<Vec<_>>(),
                    vec![
                        SketchConstraintRule::Parallel {
                            a: whole(&curves[0]),
                            b: whole(&curves[2]),
                        },
                        SketchConstraintRule::Parallel {
                            a: whole(&curves[1]),
                            b: whole(&curves[3]),
                        },
                        SketchConstraintRule::Perpendicular {
                            a: whole(&curves[0]),
                            b: whole(&curves[1]),
                        },
                    ]
                );
            } else {
                assert_eq!(old, new);
            }
        }
        a.close().expect("close");
        b.close().expect("close");
        for path in [&ui, &cli] {
            let after = crate::sketch::drag_tests::sql_facts(path);
            assert_eq!(
                sql_before.keys().collect::<Vec<_>>(),
                after.keys().collect::<Vec<_>>()
            );
            for (table, rows) in &sql_before {
                if table == "objects" {
                    for row in rows {
                        let actual = after[table]
                            .iter()
                            .find(|r| r[1] == row[1])
                            .expect("same row");
                        for col in 0..row.len() {
                            if row[1] == rusqlite::types::Value::Blob(id.to_bytes().to_vec())
                                && [3, 7, 8].contains(&col)
                            {
                                continue;
                            }
                            assert_eq!(row[col], actual[col], "object cell {col}");
                        }
                    }
                } else if table == "capabilities" {
                    assert_eq!(
                        after[table]
                            .iter()
                            .filter(|r| r[1]
                                != rusqlite::types::Value::Text(
                                    ferritecad_document::SKETCH_CONSTRAINTS_CAPABILITY.into()
                                ))
                            .cloned()
                            .collect::<Vec<_>>(),
                        *rows
                    );
                } else {
                    assert_eq!(
                        &after[table], rows,
                        "{table}: source claims and unrelated rows"
                    );
                }
            }
        }

        let mut outputs = vec![];
        for path in [&ui, &cli] {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                path,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("async Open route");
            let stl = path.with_extension("stl");
            let fbx = path.with_extension("fbx");
            for (op, dest) in [("export-stl", &stl), ("export-fbx", &fbx)] {
                let o = std::process::Command::new(ferritecad())
                    .arg(op)
                    .arg(path)
                    .arg("-o")
                    .arg(dest)
                    .output()
                    .expect("export");
                assert!(o.status.success(), "{o:?}");
            }
            outputs.push(std::fs::read(&stl).expect("STL"));
            if let Some(dir) = std::env::var_os("FERRITECAD_CONSTRAINT_ARTIFACTS") {
                std::fs::create_dir_all(&dir).expect("dir");
                for p in [path.as_path(), stl.as_path(), fbx.as_path()] {
                    std::fs::copy(p, Path::new(&dir).join(p.file_name().expect("name")))
                        .expect("artifact");
                }
            }
        }
        assert_eq!(
            outputs[0], outputs[1],
            "UI and CLI orient the same solved Body"
        );
        assert_eq!(
            std::fs::read(ui.with_extension("fbx")).expect("UI FBX"),
            std::fs::read(cli.with_extension("fbx")).expect("CLI FBX"),
            "UI and CLI publish the same FBX for the oriented Body"
        );

        // Independent integration of the published triangles. The sides were
        // asked for, so the volume is; the footprint is free to turn, so its
        // axis-aligned extents are only asked to prove it did not turn onto the
        // axes, which is what a Parallel replaced by H/V would look like.
        let stl = &outputs[0];
        let n = u32::from_le_bytes(stl[80..84].try_into().expect("count")) as usize;
        assert_eq!(stl.len(), 84 + 50 * n);
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut volume6 = 0.;
        for i in 0..n {
            let v: [f64; 9] = std::array::from_fn(|j| {
                let at = 84 + 50 * i + 12 + 4 * j;
                f64::from(f32::from_le_bytes(stl[at..at + 4].try_into().expect("f32")))
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
        let extents: [f64; 3] = std::array::from_fn(|j| hi[j] - lo[j]);
        assert!((volume6.abs() / 6. - 18000.).abs() < 0.02, "{extents:?}");
        assert!((extents[2] - 10.).abs() < 1e-4, "{extents:?}");
        assert!(
            extents[0] > 60.05 && extents[1] > 30.05,
            "a turned 60 x 30 rectangle must overhang both axes: {extents:?}"
        );
        assert_eq!(std::fs::read(&source).expect("source"), original);
    }

    #[test]
    fn native_constraint_worker_and_cli_preserve_model_and_solved_body() {
        native_constraint_worker_and_cli(false);
    }
    #[test]
    fn native_line_length_worker_and_cli_preserve_model_and_solved_body() {
        native_constraint_worker_and_cli(true);
    }
    fn native_constraint_worker_and_cli(lengths: bool) {
        use crate::creates::tests::ferritecad;
        if !ferritecad_occt::is_available() || !ferritecad_sketch_solver::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: constraint worker requires OCCT and PlaneGCS");
            return;
        }
        let root = tempfile::tempdir().expect("dir");
        let source = root.path().join("source.fcad");
        let input = root.path().join("request.json");
        let points = if lengths {
            "[[-40,-20],[40,-20],[40,20],[-40,20]]"
        } else {
            "[[-20,-10],[40,-8],[42,30],[-20,30]]"
        };
        std::fs::write(
            &input,
            format!(r#"{{"request_version":1,"points_mm":{points},"height_mm":10}}"#),
        )
        .expect("polygon");
        let o = std::process::Command::new(ferritecad())
            .arg("create-sketch-extrude")
            .arg(&input)
            .arg("-o")
            .arg(&source)
            .arg("--json")
            .output()
            .expect("create");
        assert!(o.status.success(), "{o:?}");
        crate::sketch::drag_tests::attach_source_claim(&source);
        let sql_before = crate::sketch::drag_tests::sql_facts(&source);
        // Same accepted native snapshot used by the real UI; immutable while the draft exists.
        let loaded = {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                &source,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("scene")
        };
        let reading = loaded.edit_source.expect("catalog");
        let id = reading.constraint_sketches[0].sketch;
        let original = std::fs::read(&source).expect("source");
        let mut e = Editor::default();
        assert!(e.begin(&source, &reading, id));
        let ctx = egui::Context::default();
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, "Segment 1");
        click(&ctx, &mut e, "Add Horizontal");
        if lengths {
            for (segment, label) in [
                ("Segment 2", "Add Vertical"),
                ("Segment 3", "Add Horizontal"),
                ("Segment 4", "Add Vertical"),
            ] {
                click(&ctx, &mut e, segment);
                click(&ctx, &mut e, label);
            }
            for (segment, value) in [("Segment 1", "60"), ("Segment 2", "30")] {
                click(&ctx, &mut e, segment);
                enter_length(&ctx, &mut e, value, false);
                click(&ctx, &mut e, "Add length");
            }
        }
        let expected_edits = e.draft.as_ref().expect("draft").edits.clone();
        click(&ctx, &mut e, "Undo");
        let mut previous = expected_edits.clone();
        previous.add.pop();
        assert_eq!(e.draft.as_ref().expect("draft").edits, previous);
        click(&ctx, &mut e, "Redo");
        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("real widget request");
        assert_eq!(request.edits, expected_edits);
        assert_eq!(request.source, source);
        assert_eq!(request.sketch, id);
        assert_eq!(request.expected, reading.version);
        let kept = request.edits.clone();
        let history = history_state(&e);
        let mut state = crate::edits::Edits::default();
        for occupied in [true, false] {
            let mut r = request.clone();
            r.destination = root
                .path()
                .join(if occupied { "busy.fcad" } else { "ui.fcad" });
            if occupied {
                std::fs::write(&r.destination, b"keep").expect("busy");
            }
            let (tx, rx) = std::sync::mpsc::channel();
            state
                .start_constraints(r, move |r, g, c| {
                    crate::edits::spawn_constraint_edit(r, c, move |result| {
                        tx.send((g, result)).expect("reply")
                    })
                })
                .expect("worker");
            let (g, result) = rx.recv().expect("worker reply");
            let open = finish_edit(&mut e, &mut state, g, result);
            if occupied {
                assert!(open.is_none());
                assert_eq!(e.draft.as_ref().expect("retained").edits, kept);
                assert_eq!(history_state(&e), history);
                assert_eq!(
                    std::fs::read(root.path().join("busy.fcad")).expect("busy"),
                    b"keep"
                );
            } else {
                assert_eq!(open, Some(root.path().join("ui.fcad")));
                assert!(!e.active());
            }
        }
        let additions = kept
            .add
            .iter()
            .map(peer_addition)
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            &input,
            format!(r#"{{"request_version":1,"remove":[],"add":[{additions}]}}"#),
        )
        .expect("typed IDs to peer request");
        let cli = root.path().join("cli.fcad");
        let ui = root.path().join("ui.fcad");
        let o = std::process::Command::new(ferritecad())
            .arg("edit-sketch-constraints-copy")
            .arg(&source)
            .arg("--sketch")
            .arg(id.to_string())
            .arg("--expect-version")
            .arg(reading.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&cli)
            .arg("--json")
            .output()
            .expect("peer CLI");
        assert!(o.status.success(), "{o:?}");
        let a = Document::open_read_only(&ui).expect("UI");
        let b = Document::open_read_only(&cli).expect("CLI");
        assert_eq!(a.meta(), b.meta());
        assert_eq!(
            a.dependencies().expect("deps"),
            b.dependencies().expect("deps")
        );
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
        for old in a.objects().expect("objects") {
            let new = b.object(old.id).expect("read").expect("same id");
            if let (
                ferritecad_document::ObjectPayload::Sketch(s),
                ferritecad_document::ObjectPayload::Sketch(t),
            ) = (&old.payload, &new.payload)
            {
                assert_eq!(s.curves, t.curves);
                assert_eq!(s.plane, t.plane);
                assert_eq!(s.constraints.len(), if lengths { 10 } else { 5 });
                for (x, y) in s.constraints.iter().zip(&t.constraints) {
                    assert_ne!(x.id, y.id);
                    assert_eq!(x.rule, y.rule);
                }
            } else {
                assert_eq!(old, new);
            }
        }
        a.close().expect("close");
        b.close().expect("close");
        for path in [&ui, &cli] {
            let after = crate::sketch::drag_tests::sql_facts(path);
            assert_eq!(
                sql_before.keys().collect::<Vec<_>>(),
                after.keys().collect::<Vec<_>>()
            );
            for (table, rows) in &sql_before {
                if table == "objects" {
                    for row in rows {
                        let actual = after[table]
                            .iter()
                            .find(|r| r[1] == row[1])
                            .expect("same row");
                        for col in 0..row.len() {
                            if row[1] == rusqlite::types::Value::Blob(id.to_bytes().to_vec())
                                && [3, 7, 8].contains(&col)
                            {
                                continue;
                            }
                            assert_eq!(row[col], actual[col], "object cell {col}");
                        }
                    }
                } else if table == "capabilities" {
                    assert_eq!(
                        after[table]
                            .iter()
                            .filter(|r| r[1]
                                != rusqlite::types::Value::Text(
                                    ferritecad_document::SKETCH_CONSTRAINTS_CAPABILITY.into()
                                ))
                            .cloned()
                            .collect::<Vec<_>>(),
                        *rows
                    );
                } else {
                    assert_eq!(
                        &after[table], rows,
                        "{table}: source claims and all unrelated rows"
                    );
                }
            }
        }

        let mut outputs = vec![];
        for path in [&ui, &cli] {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            let snap = ferritecad_scene::snapshot_of(
                path,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("async Open route");
            assert!(
                snap.edit_source.expect("facts").constraint_sketches[0]
                    .refusal
                    .is_none()
            );
            let stl = path.with_extension("stl");
            let o = std::process::Command::new(ferritecad())
                .arg("export-stl")
                .arg(path)
                .arg("-o")
                .arg(&stl)
                .output()
                .expect("STL");
            assert!(o.status.success(), "{o:?}");
            outputs.push(std::fs::read(&stl).expect("STL"));
            if let Some(dir) = std::env::var_os("FERRITECAD_CONSTRAINT_ARTIFACTS") {
                std::fs::create_dir_all(&dir).expect("dir");
                std::fs::copy(path, Path::new(&dir).join(path.file_name().expect("name")))
                    .expect("artifact");
            }
        }
        assert_eq!(
            outputs[0], outputs[1],
            "independent UUIDs do not alter solved Body; CLI process gate integrates these triangles independently"
        );
        assert_eq!(std::fs::read(&source).expect("source"), original);
        // Exact persisted H is removable through the same widgets after publication.
        let d = Document::open_read_only(&ui).expect("published");
        let after = ExtrudeEditSource::read(&d).expect("discover");
        d.close().expect("close");
        assert!(e.begin(&ui, &after, id));
        assert_eq!(
            history_state(&e),
            Default::default(),
            "published draft history is discarded"
        );
        frame(&ctx, &mut e, vec![]);
        frame(&ctx, &mut e, vec![]);
        click(&ctx, &mut e, "Remove");
        click(&ctx, &mut e, "Save constraints copy…");
        let remove = e.take_request().expect("removal");
        assert_eq!(
            remove.edits.remove,
            vec![
                after.constraint_sketches[0]
                    .stored
                    .as_ref()
                    .expect("sketch")
                    .constraints[4]
                    .id
            ]
        );
        assert!(remove.edits.add.is_empty());
        if !lengths {
            return;
        }
        click(&ctx, &mut e, "Undo");
        let sketch = after.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("sketch");
        let (old_length, curve) = sketch
            .constraints
            .iter()
            .find_map(|c| match c.rule {
                SketchConstraintRule::Distance {
                    a, distance: 60., ..
                } => Some((c.id, a.curve)),
                _ => None,
            })
            .expect("stored 60 mm");
        click(&ctx, &mut e, "Segment 1");
        enter_length(&ctx, &mut e, "55", false);
        click(&ctx, &mut e, "Replace length");
        let replaced = e.draft.as_ref().expect("draft").edits.clone();
        assert_eq!(replaced.remove, vec![old_length]);
        assert_eq!(
            replaced.add,
            vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                curve,
                kind: LineConstraintKind::Distance(LineLengthMm::new(55.).expect("55")),
            })]
        );
        click(&ctx, &mut e, "Undo");
        assert_eq!(
            e.draft.as_ref().expect("draft").edits,
            SketchConstraintEdits::default()
        );
        click(&ctx, &mut e, "Redo");
        assert_eq!(e.draft.as_ref().expect("draft").edits, replaced);
        click(&ctx, &mut e, "Save constraints copy…");
        let request = e.take_request().expect("replace request");
        assert_eq!(request.edits, replaced);
        assert_eq!(request.source, ui);
        assert_eq!(request.expected, after.version);
        let kept = request.edits.clone();
        let history = history_state(&e);
        let original_constrained = std::fs::read(&ui).expect("constrained source");
        let mut state = crate::edits::Edits::default();
        for occupied in [true, false] {
            let mut r = request.clone();
            r.destination = root.path().join(if occupied {
                "busy-replaced.fcad"
            } else {
                "ui-replaced.fcad"
            });
            if occupied {
                std::fs::write(&r.destination, b"keep").expect("busy");
            }
            let (tx, rx) = std::sync::mpsc::channel();
            state
                .start_constraints(r, move |r, g, c| {
                    crate::edits::spawn_constraint_edit(r, c, move |result| {
                        tx.send((g, result)).expect("reply")
                    })
                })
                .expect("worker");
            let (g, result) = rx.recv().expect("worker reply");
            let open = finish_edit(&mut e, &mut state, g, result);
            if occupied {
                assert!(open.is_none());
                assert_eq!(e.draft.as_ref().expect("retained").edits, kept);
                assert_eq!(history_state(&e), history);
                assert_eq!(
                    std::fs::read(root.path().join("busy-replaced.fcad")).expect("busy"),
                    b"keep"
                );
            } else {
                assert_eq!(open, Some(root.path().join("ui-replaced.fcad")));
                assert!(!e.active());
            }
        }
        std::fs::write(
            &input,
            format!(
                r#"{{"request_version":1,"remove":["{old_length}"],"add":[{{"curve_id":"{curve}","rule":"distance","distance_mm":55}}]}}"#
            ),
        )
        .expect("peer replace request");
        let cli_replaced = root.path().join("cli-replaced.fcad");
        let ui_replaced = root.path().join("ui-replaced.fcad");
        let o = std::process::Command::new(ferritecad())
            .arg("edit-sketch-constraints-copy")
            .arg(&ui)
            .arg("--sketch")
            .arg(id.to_string())
            .arg("--expect-version")
            .arg(after.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&cli_replaced)
            .arg("--json")
            .output()
            .expect("peer CLI replace");
        assert!(o.status.success(), "{o:?}");
        let a = Document::open_read_only(&ui_replaced).expect("UI replace");
        let b = Document::open_read_only(&cli_replaced).expect("CLI replace");
        let source_doc = Document::open_read_only(&ui).expect("constrained source");
        assert_eq!(a.meta(), b.meta());
        assert_eq!(
            a.dependencies().expect("deps"),
            b.dependencies().expect("deps")
        );
        assert_eq!(
            a.topology_refs().expect("refs"),
            b.topology_refs().expect("refs")
        );
        let mut new_ui = None;
        let mut new_cli = None;
        for old in a.objects().expect("objects") {
            let new = b.object(old.id).expect("read").expect("same id");
            let src = source_doc.object(old.id).expect("read").expect("same id");
            if let (
                ferritecad_document::ObjectPayload::Sketch(s),
                ferritecad_document::ObjectPayload::Sketch(t),
                ferritecad_document::ObjectPayload::Sketch(u),
            ) = (&old.payload, &new.payload, &src.payload)
            {
                assert_eq!(s.curves, t.curves);
                assert_eq!(s.curves, u.curves);
                assert_eq!(s.plane, t.plane);
                assert_eq!(s.constraints.len(), u.constraints.len());
                for (x, y) in s.constraints.iter().zip(&t.constraints) {
                    match (x.rule, y.rule) {
                        (
                            SketchConstraintRule::Distance { distance: a_mm, .. },
                            SketchConstraintRule::Distance { distance: b_mm, .. },
                        ) if a_mm == 55. && b_mm == 55. => {
                            assert_ne!(x.id, y.id);
                            assert_ne!(x.id, old_length);
                            assert_ne!(y.id, old_length);
                            new_ui = Some(x.id);
                            new_cli = Some(y.id);
                            assert_eq!(x.rule, y.rule);
                        }
                        _ => assert_eq!(x, y, "every other constraint keeps its UUID"),
                    }
                }
                assert!(!s.constraints.iter().any(|c| c.id == old_length));
                assert_eq!(
                    s.constraints
                        .iter()
                        .filter(|c| c.id != new_ui.expect("UI length"))
                        .map(|c| c.id)
                        .collect::<Vec<_>>(),
                    u.constraints
                        .iter()
                        .filter(|c| c.id != old_length)
                        .map(|c| c.id)
                        .collect::<Vec<_>>()
                );
            } else {
                assert_eq!(old, new);
                assert_eq!(old, src);
            }
        }
        a.close().expect("close");
        b.close().expect("close");
        source_doc.close().expect("close");
        let sql_constrained = crate::sketch::drag_tests::sql_facts(&ui);
        for path in [&ui_replaced, &cli_replaced] {
            let after_sql = crate::sketch::drag_tests::sql_facts(path);
            assert_eq!(
                sql_constrained.keys().collect::<Vec<_>>(),
                after_sql.keys().collect::<Vec<_>>()
            );
            for (table, rows) in &sql_constrained {
                if table == "objects" {
                    for row in rows {
                        let actual = after_sql[table]
                            .iter()
                            .find(|r| r[1] == row[1])
                            .expect("same row");
                        for col in 0..row.len() {
                            if row[1] == rusqlite::types::Value::Blob(id.to_bytes().to_vec())
                                && [3, 7, 8].contains(&col)
                            {
                                continue;
                            }
                            assert_eq!(row[col], actual[col], "object cell {col}");
                        }
                    }
                } else {
                    assert_eq!(&after_sql[table], rows, "{table}: source claims retained");
                }
            }
        }
        let mut outputs = vec![];
        for path in [&ui_replaced, &cli_replaced] {
            let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
            ferritecad_scene::snapshot_of(
                path,
                &mut k,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("async Open route");
            let stl = path.with_extension("stl");
            let fbx = path.with_extension("fbx");
            for op in ["export-stl", "export-fbx"] {
                let dest = if op == "export-stl" { &stl } else { &fbx };
                let o = std::process::Command::new(ferritecad())
                    .arg(op)
                    .arg(path)
                    .arg("-o")
                    .arg(dest)
                    .output()
                    .expect("export");
                assert!(o.status.success(), "{o:?}");
            }
            outputs.push((
                std::fs::read(&stl).expect("STL"),
                std::fs::read(&fbx).expect("FBX"),
            ));
            if let Some(dir) = std::env::var_os("FERRITECAD_CONSTRAINT_ARTIFACTS") {
                std::fs::create_dir_all(&dir).expect("dir");
                for p in [path.as_path(), stl.as_path(), fbx.as_path()] {
                    std::fs::copy(p, Path::new(&dir).join(p.file_name().expect("name")))
                        .expect("artifact");
                }
            }
        }
        assert_eq!(
            outputs[0], outputs[1],
            "UI and CLI replacement exports match"
        );
        let stl = &outputs[0].0;
        let n = u32::from_le_bytes(stl[80..84].try_into().expect("count")) as usize;
        assert_eq!(stl.len(), 84 + 50 * n);
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut volume6 = 0.;
        for i in 0..n {
            let v: [f64; 9] = std::array::from_fn(|j| {
                let at = 84 + 50 * i + 12 + 4 * j;
                f64::from(f32::from_le_bytes(stl[at..at + 4].try_into().expect("f32")))
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
        let mut extents = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        extents.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
        assert!(
            (extents[0] - 10.).abs() < 1e-4
                && (extents[1] - 30.).abs() < 1e-4
                && (extents[2] - 55.).abs() < 1e-4,
            "{extents:?}"
        );
        assert!((volume6.abs() / 6. - 16500.).abs() < 0.02);
        assert_eq!(std::fs::read(&ui).expect("source"), original_constrained);
        assert_ne!(new_ui.expect("UI length"), new_cli.expect("CLI length"));
    }
}
