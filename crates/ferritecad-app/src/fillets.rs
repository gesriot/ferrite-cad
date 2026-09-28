// SPDX-License-Identifier: MIT
//! A disposable single-edge fillet request over stored facts; no kernel, no
//! file reads (§28A).
//!
//! The form offers only what the document says can be rounded: the four
//! vertical edges of one saved rectangular plate, each named by the base
//! Extrude and the two Lines that meet there. It lists them rather than
//! letting a pointer pick geometry, because this slice has no picking, and a
//! control that suggested otherwise would be promising it.
use ferritecad_document::{
    DocumentVersion, EdgeFillet, ExtrudeEditSource, FilletChoice, FilletCorner, FilletEdge,
    FilletRadiusChoice, SavedFillet,
};
use ferritecad_jobs::{EdgeFilletRequest, EditFilletRadiusRequest};
use ferritecad_types::{CadError, ObjectId, Result};
use std::path::{Path, PathBuf};

/// What the form holds before any of it is a request: the chosen corner, by
/// its position in the catalogue's list, and the radius as text.
///
/// The position never leaves the form. What is sent is the corner the
/// catalogue listed at that position, by its producer and its two Lines.
#[derive(Debug, Clone, Default, PartialEq)]
struct Typed {
    corner: Option<usize>,
    radius: String,
}

#[derive(Debug, Clone)]
struct Draft {
    source: PathBuf,
    version: DocumentVersion,
    choice: FilletChoice,
    typed: Typed,
    /// The last selection and radius a confirmed Apply accepted.
    applied: Option<Typed>,
    refusal: Option<String>,
}

impl Draft {
    fn corners(&self) -> &[FilletCorner] {
        self.choice
            .target
            .as_ref()
            .map(|t| t.corners.as_slice())
            .unwrap_or_default()
    }

    /// The typed state as one request, judged by the document's own rule.
    fn fillet(&self, typed: &Typed) -> Result<(FilletCorner, EdgeFillet)> {
        let corner = typed
            .corner
            .and_then(|i| self.corners().get(i))
            .ok_or_else(|| CadError::input("choose one vertical edge to round"))?;
        let radius_mm = typed
            .radius
            .trim()
            .parse::<f64>()
            .map_err(|_| CadError::input("enter a finite fillet radius in mm"))?;
        let fillet = EdgeFillet {
            edge: FilletEdge {
                feature: corner.feature,
                joint: corner.joint,
            },
            radius_mm,
        };
        let corner = self.choice.validate(&fillet)?;
        Ok((corner, fillet))
    }
}

/// §28B: a new radius for the saved Fillet, as typed. The edge is the saved
/// one and is shown, never chosen: this form cannot retarget the Fillet.
#[derive(Debug, Clone)]
struct RadiusDraft {
    source: PathBuf,
    version: DocumentVersion,
    choice: FilletRadiusChoice,
    typed: String,
    /// The radius a confirmed Apply accepted, as typed.
    applied: Option<String>,
    refusal: Option<String>,
}

impl RadiusDraft {
    fn saved(&self) -> Option<&SavedFillet> {
        self.choice.saved.as_ref()
    }

    /// The typed radius, judged by the document's own rule.
    fn radius(&self, typed: &str) -> Result<f64> {
        let radius_mm = typed
            .trim()
            .parse::<f64>()
            .map_err(|_| CadError::input("enter a finite fillet radius in mm"))?;
        self.choice.validate(radius_mm)?;
        Ok(radius_mm)
    }
}

#[derive(Debug, Default)]
pub(crate) struct Editor {
    draft: Option<Draft>,
    pending: Option<EdgeFilletRequest>,
    radius: Option<RadiusDraft>,
    pending_radius: Option<EditFilletRadiusRequest>,
}

impl Editor {
    pub(crate) fn active(&self) -> bool {
        self.draft.is_some() || self.radius.is_some()
    }
    pub(crate) fn dismiss(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn take_request(&mut self) -> Option<EdgeFilletRequest> {
        self.pending.take()
    }
    pub(crate) fn take_radius_request(&mut self) -> Option<EditFilletRadiusRequest> {
        self.pending_radius.take()
    }

    /// Begin changing the radius of one saved Fillet. The form opens on the
    /// stored radius.
    fn begin_radius(&mut self, path: &Path, source: &ExtrudeEditSource, feature: ObjectId) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .fillet_features
            .iter()
            .find(|c| c.feature == feature && c.refusal.is_none() && c.saved.is_some())
        else {
            return false;
        };
        self.radius = Some(RadiusDraft {
            source: path.to_path_buf(),
            version: source.version,
            typed: choice.stored.radius_mm.to_string(),
            choice: choice.clone(),
            applied: None,
            refusal: None,
        });
        true
    }

    fn begin(&mut self, path: &Path, source: &ExtrudeEditSource, body: ObjectId) -> bool {
        if self.active() || source.refusal.is_some() {
            return false;
        }
        let Some(choice) = source
            .fillet_bodies
            .iter()
            .find(|c| c.body == body && c.refusal.is_none() && c.target.is_some())
        else {
            return false;
        };
        self.draft = Some(Draft {
            source: path.to_path_buf(),
            version: source.version,
            choice: choice.clone(),
            typed: Typed::default(),
            applied: None,
            refusal: None,
        });
        true
    }

    /// One button per Body the document says can be filleted, with the reason
    /// on the ones it says cannot.
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
        for choice in &source.fillet_bodies {
            let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
            let response = ui.add_enabled(
                can_begin && refusal.is_none(),
                egui::Button::new(format!(
                    "Fillet edge of {} — {}…",
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
        // §28B: one per saved Fillet, with the shared domain reason on the
        // ones this build cannot edit.
        for choice in &source.fillet_features {
            let refusal = source.refusal.as_ref().or(choice.refusal.as_ref());
            let response = ui.add_enabled(
                can_begin && refusal.is_none(),
                egui::Button::new(format!(
                    "Edit Fillet radius {} — {}…",
                    choice.name.as_deref().unwrap_or("Unnamed fillet"),
                    choice.feature
                )),
            );
            if response.clicked() {
                self.begin_radius(path, source, choice.feature);
            }
            if let Some(reason) = refusal {
                response.on_hover_text(reason);
            }
        }
    }

    fn draw_radius(&mut self, ui: &mut egui::Ui, running: bool) {
        let Some(draft) = &mut self.radius else {
            return;
        };
        let Some(saved) = draft.saved().cloned() else {
            return;
        };
        let mut cancel = false;
        egui::Window::new("Edit Fillet radius — new copy")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(
                    "Changes only the radius. The Fillet keeps its edge, its UUID and every \
                     name; the edge cannot be changed here.",
                );
                ui.small(format!(
                    "Fillet {} · Body {} · base Extrude {} · {}",
                    saved.feature,
                    saved.body,
                    saved.previous,
                    draft.source.display()
                ));
                ui.small(format!("Edge: {}", describe(&saved.corner)));
                ui.small(format!(
                    "Saved radius {} mm; from {} mm to {} mm here.",
                    saved.radius_mm,
                    ferritecad_document::MIN_RADIUS_MM,
                    saved.corner.max_radius_mm
                ));
                ui.add_enabled_ui(!running, |ui| {
                    if ui.button("Cancel radius draft").clicked() {
                        cancel = true;
                    }
                    ui.horizontal(|ui| {
                        ui.label("New radius (mm):");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.typed)
                                .id_salt("fillet-new-radius")
                                .char_limit(32)
                                .desired_width(110.),
                        );
                    });
                    if ui.button("Apply radius").clicked() {
                        match draft.radius(&draft.typed) {
                            Ok(_) => {
                                draft.applied = Some(draft.typed.clone());
                                draft.refusal = None;
                            }
                            Err(error) => draft.refusal = Some(error.to_string()),
                        }
                    }
                    if let Some(refusal) = &draft.refusal {
                        egui::ScrollArea::vertical()
                            .id_salt("fillet-radius-refusal")
                            .max_height(72.)
                            .show(ui, |ui| {
                                ui.colored_label(ui.visuals().error_fg_color, refusal);
                            });
                    }
                    let confirmed = draft
                        .applied
                        .as_ref()
                        .filter(|applied| **applied == draft.typed)
                        .and_then(|applied| draft.radius(applied).ok());
                    match confirmed {
                        Some(radius_mm) => {
                            ui.small(format!(
                                "Ready: radius {} mm -> {radius_mm} mm at ({}, {})",
                                saved.radius_mm,
                                saved.corner.corner_mm[0],
                                saved.corner.corner_mm[1]
                            ));
                            if ui.button("Save radius copy…").clicked() {
                                self.pending_radius = Some(EditFilletRadiusRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,
                                    feature: saved.feature,
                                    radius_mm,
                                    destination: PathBuf::new(),
                                });
                            }
                        }
                        None => {
                            ui.small("Apply a radius before saving.");
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

    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, running: bool) {
        if self.radius.is_some() {
            self.draw_radius(ui, running);
            return;
        }
        let Some(draft) = &mut self.draft else {
            return;
        };
        let Some(target) = draft.choice.target.clone() else {
            return;
        };
        let mut cancel = false;
        egui::Window::new("Fillet one vertical edge — new copy")
            .default_width(600.)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(
                    "Rounds one vertical edge of the plate with a constant radius. The edge is \
                     named by the base Extrude and the two Lines that meet at its corner.",
                );
                ui.small(format!(
                    "Body {} · base Extrude {} · profile {} · {} mm tall · {}",
                    target.body,
                    target.base_feature,
                    target.profile,
                    target.height_mm,
                    draft.source.display()
                ));
                ui.small(format!(
                    "Radius from {} mm to {} × the shorter adjacent side.",
                    ferritecad_document::MIN_RADIUS_MM,
                    ferritecad_document::MAX_RADIUS_FRACTION
                ));
                ui.add_enabled_ui(!running, |ui| {
                    if ui.button("Cancel fillet draft").clicked() {
                        cancel = true;
                    }
                    ui.label("Edge:");
                    for (i, corner) in target.corners.iter().enumerate() {
                        ui.radio_value(&mut draft.typed.corner, Some(i), describe(corner));
                    }
                    ui.horizontal(|ui| {
                        ui.label("Radius (mm):");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.typed.radius)
                                .id_salt("fillet-radius")
                                .char_limit(32)
                                .desired_width(110.),
                        );
                    });
                    if ui.button("Apply fillet").clicked() {
                        match draft.fillet(&draft.typed) {
                            Ok(_) => {
                                draft.applied = Some(draft.typed.clone());
                                draft.refusal = None;
                            }
                            Err(error) => draft.refusal = Some(error.to_string()),
                        }
                    }
                    if let Some(refusal) = &draft.refusal {
                        egui::ScrollArea::vertical()
                            .id_salt("fillet-refusal")
                            .max_height(72.)
                            .show(ui, |ui| {
                                ui.colored_label(ui.visuals().error_fg_color, refusal);
                            });
                    }
                    let confirmed = draft
                        .applied
                        .as_ref()
                        .filter(|applied| **applied == draft.typed)
                        .and_then(|applied| draft.fillet(applied).ok());
                    match confirmed {
                        Some((corner, fillet)) => {
                            ui.small(format!(
                                "Ready: round the edge at ({}, {}) with r{} mm",
                                corner.corner_mm[0], corner.corner_mm[1], fillet.radius_mm
                            ));
                            if ui.button("Save fillet copy…").clicked() {
                                self.pending = Some(EdgeFilletRequest {
                                    source: draft.source.clone(),
                                    expected: draft.version,
                                    body: target.body,
                                    fillet,
                                    destination: PathBuf::new(),
                                });
                            }
                        }
                        None => {
                            ui.small("Apply an edge and a radius before saving.");
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

/// One candidate as the form lists it: where, which Lines, and the limit.
fn describe(corner: &FilletCorner) -> String {
    let [a, b] = corner.joint.segments();
    format!(
        "Corner ({}, {}) — Lines {a} | {b}; sides {} × {} mm; r ≤ {} mm",
        corner.corner_mm[0],
        corner.corner_mm[1],
        corner.adjacent_lengths_mm[0],
        corner.adjacent_lengths_mm[1],
        corner.max_radius_mm
    )
}

/// The same two steps every published copy takes: the draft is handed to the
/// shared retention so an Open that is later refused can give it back.
pub(crate) fn finish_fillet(
    editor: &mut crate::sketch::Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::AddedEdgeFillet>,
) -> Option<PathBuf> {
    if edits.accepts(generation)
        && let Ok(saved) = &result
    {
        editor.draft_published(&saved.destination);
    }
    edits.finish_fillet(generation, result)
}

/// §28B's completion, through exactly the same two steps and retention as
/// the Fillet's own.
pub(crate) fn finish_fillet_radius(
    editor: &mut crate::sketch::Editor,
    edits: &mut crate::edits::Edits,
    generation: u64,
    result: Result<ferritecad_jobs::EditedFilletRadius>,
) -> Option<PathBuf> {
    if edits.accepts(generation)
        && let Ok(saved) = &result
    {
        editor.draft_published(&saved.destination);
    }
    edits.finish_fillet_radius(generation, result)
}

#[cfg(test)]
#[allow(clippy::panic)]
pub(crate) mod tests {
    use super::*;
    use ferritecad_document::{
        Body, CapSide, DatumPlane, Dependency, DependencyRole, Document, EndCondition, EntityKind,
        Expression, Extrude, ObjectPayload, Point2, SelectionRule, SemanticRole, Sketch,
        SketchCurve, SketchGeometry, SolidOperation, TopologyRef,
    };
    use ferritecad_types::{StableEntityId, Transform};

    /// The asymmetric, translated plate of the probe, drawn clockwise from a
    /// corner other than the first.
    const PLATE: [[f64; 2]; 4] = [[33., 15.5], [33., 3.25], [-4.5, 3.25], [-4.5, 15.5]];

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
    fn frame(ctx: &egui::Context, e: &mut Editor, running: bool) -> egui::FullOutput {
        run(ctx, e, Vec::new(), running)
    }
    fn press(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2, running: bool) {
        run(ctx, e, vec![egui::Event::PointerMoved(at)], running);
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
                running,
            );
        }
    }
    fn find(out: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text().starts_with(label) => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
    }
    fn click_while(ctx: &egui::Context, e: &mut Editor, label: &str, running: bool) {
        let out = frame(ctx, e, running);
        let at = find(&out, label).unwrap_or_else(|| panic!("not painted: {label}"));
        press(ctx, e, at, running);
    }
    fn click(ctx: &egui::Context, e: &mut Editor, label: &str) {
        click_while(ctx, e, label, false);
    }
    fn painted(out: &egui::FullOutput, label: &str) -> bool {
        out.shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains(label)))
    }
    fn radius(ctx: &egui::Context, e: &mut Editor, value: &str) {
        enter(ctx, e, "Radius (mm):", value);
    }
    fn enter(ctx: &egui::Context, e: &mut Editor, field: &str, value: &str) {
        let out = frame(ctx, e, false);
        let label = find(&out, field).unwrap_or_else(|| panic!("label {field}"));
        press(ctx, e, egui::pos2(label.x + 90., label.y), false);
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

    /// The plate written straight into a document: no kernel is needed to
    /// discover it, and the native gate below rebuilds exactly this.
    fn plate() -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("плита.fcad");
        let mut d = Document::create(&path).expect("document");
        let [plane, profile, extrude, body] = std::array::from_fn(|_| ObjectId::new());
        let segments: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();
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
            let curves = (0..4)
                .map(|i| {
                    Ok(SketchCurve {
                        id: segments[i],
                        construction: false,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(PLATE[i][0], PLATE[i][1])?,
                            end: Point2::new(PLATE[(i + 1) % 4][0], PLATE[(i + 1) % 4][1])?,
                        },
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            w.put_object(
                profile,
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
                    profile,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(6.75)?,
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
                Some("Plate"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(extrude),
                }),
            )?;
            for (dependent, dependency, role) in [
                (profile, plane, DependencyRole::Plane),
                (extrude, profile, DependencyRole::Profile),
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
            Ok(())
        })
        .expect("plate");
        let source = ExtrudeEditSource::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }

    /// Opens the form through its own button, as a person would.
    fn begin_by_button(e: &mut Editor, path: &Path, source: &ExtrudeEditSource) {
        begin_by(e, path, source, "Fillet edge of Plate");
    }
    fn begin_by(e: &mut Editor, path: &Path, source: &ExtrudeEditSource, button: &str) {
        let ctx = egui::Context::default();
        let layout = |e: &mut Editor, events| {
            let mut o = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(988., 768.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| e.choices(ui, true, Some(path), Some(source)),
            );
            o.textures_delta.clear();
            o
        };
        let out = layout(e, Vec::new());
        let at = find(&out, button).unwrap_or_else(|| panic!("button {button}"));
        layout(e, vec![egui::Event::PointerMoved(at)]);
        for pressed in [true, false] {
            layout(
                e,
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
            );
        }
        assert!(e.active(), "the button opened the form");
    }

    /// The form lists the four corners the document names, refuses what the
    /// document would, and keeps its draft through a cancelled Save, a worker
    /// refusal and a stale reply. No kernel is involved.
    #[test]
    fn fillet_widgets_list_corners_refuse_like_the_document_and_keep_the_draft() {
        let (_root, path, source) = plate();
        let choice = source.fillet_bodies[0].clone();
        let target = choice.target.clone().expect("a target");
        let mut e = Editor::default();
        begin_by_button(&mut e, &path, &source);
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        let out = frame(&ctx, &mut e, false);
        for corner in &target.corners {
            let [a, b] = corner.joint.segments();
            assert!(painted(
                &out,
                &format!(
                    "Corner ({}, {}) — Lines {a} | {b}",
                    corner.corner_mm[0], corner.corner_mm[1]
                )
            ));
        }
        assert!(painted(&out, &target.base_feature.to_string()));
        assert!(painted(
            &out,
            "Radius from 0.01 mm to 0.5 × the shorter adjacent side"
        ));
        assert!(!painted(&out, "Save fillet copy…"), "nothing applied yet");

        // Nothing chosen, then text that is no radius, then radii the
        // document refuses: each refused, none applied.
        click(&ctx, &mut e, "Apply fillet");
        assert!(
            e.draft
                .as_ref()
                .expect("draft")
                .refusal
                .as_deref()
                .expect("refused")
                .contains("choose one vertical edge")
        );
        click(&ctx, &mut e, "Corner (33, 3.25)");
        let chosen = target
            .corners
            .iter()
            .position(|c| c.corner_mm == [33., 3.25])
            .expect("that corner");
        assert_eq!(e.draft.as_ref().expect("draft").typed.corner, Some(chosen));
        for (text, why) in [
            ("banana", "finite"),
            ("0", "at least"),
            ("0.005", "at least"),
            ("6.2", "too large"),
            ("inf", "finite"),
        ] {
            radius(&ctx, &mut e, text);
            click(&ctx, &mut e, "Apply fillet");
            let draft = e.draft.as_ref().expect("draft");
            assert!(draft.applied.is_none(), "{text} was applied");
            assert!(
                draft.refusal.as_deref().expect("a refusal").contains(why),
                "{text}: {:?}",
                draft.refusal
            );
        }
        radius(&ctx, &mut e, "2.375");
        click(&ctx, &mut e, "Apply fillet");
        let out = frame(&ctx, &mut e, false);
        assert!(painted(
            &out,
            "Ready: round the edge at (33, 3.25) with r2.375 mm"
        ));
        assert!(painted(&out, "Save fillet copy…"));
        // An unapplied change un-readies the request.
        radius(&ctx, &mut e, "3");
        assert!(!painted(&frame(&ctx, &mut e, false), "Save fillet copy…"));
        radius(&ctx, &mut e, "2.375");
        assert!(painted(&frame(&ctx, &mut e, false), "Save fillet copy…"));

        click(&ctx, &mut e, "Save fillet copy…");
        let request = e.take_request().expect("the widgets' request");
        assert!(e.take_request().is_none(), "one press, one request");
        assert_eq!(request.source, path);
        assert_eq!(request.expected, source.version);
        assert_eq!(request.body, choice.body);
        assert_eq!(request.fillet.edge.feature, target.base_feature);
        assert_eq!(request.fillet.edge.joint, target.corners[chosen].joint);
        assert_eq!(request.fillet.radius_mm, 2.375);
        let typed = e.draft.as_ref().expect("draft").typed.clone();

        // The Save dialog was cancelled: nothing ran, and the draft is as it was.
        assert!(e.active());
        assert_eq!(e.draft.as_ref().expect("draft").typed, typed);

        // While a job runs the form is inert, and says the draft is kept.
        let out = frame(&ctx, &mut e, true);
        assert!(painted(&out, "Draft retained until publication"));
        click_while(&ctx, &mut e, "Cancel fillet draft", true);
        assert!(e.active(), "Cancel is disabled while saving");

        // A worker refusal and a stale reply both leave the draft in place.
        let mut editor = crate::sketch::Editor::default();
        editor.fillets = e;
        let mut edits = crate::edits::Edits::default();
        let mut refused = request.clone();
        refused.destination = PathBuf::from("refused.fcad");
        let generation = edits
            .start_fillet(refused, |_, _, _| std::thread::spawn(|| {}))
            .expect("started");
        assert_eq!(
            finish_fillet(
                &mut editor,
                &mut edits,
                generation + 1,
                Ok(published_stub())
            ),
            None,
            "a stale reply is ignored"
        );
        assert!(editor.fillets.active());
        assert_eq!(
            finish_fillet(
                &mut editor,
                &mut edits,
                generation,
                Err(ferritecad_types::CadError::kernel("refused by the worker"))
            ),
            None
        );
        assert!(editor.fillets.active(), "a refusal keeps the draft");
        assert_eq!(editor.fillets.draft.as_ref().expect("draft").typed, typed);

        // Cancel, when nothing runs, leaves nothing and starts nothing.
        let mut e = std::mem::take(&mut editor.fillets);
        click(&ctx, &mut e, "Cancel fillet draft");
        assert!(!e.active());
        assert!(e.take_request().is_none());
    }

    fn published_stub() -> ferritecad_jobs::AddedEdgeFillet {
        let corner = ferritecad_document::FilletCorner {
            feature: ObjectId::new(),
            joint: ferritecad_types::ProfileJoint::new(
                StableEntityId::new(),
                StableEntityId::new(),
            )
            .expect("joint"),
            corner_mm: [0., 0.],
            adjacent_lengths_mm: [1., 1.],
            max_radius_mm: 0.5,
        };
        ferritecad_jobs::AddedEdgeFillet {
            destination: PathBuf::from("stale.fcad"),
            document_id: ferritecad_types::DocumentId::new(),
            body: ObjectId::new(),
            feature: ObjectId::new(),
            previous: ObjectId::new(),
            corner,
            radius_mm: 0.25,
            references: Vec::new(),
        }
    }

    type Rows = Vec<Vec<rusqlite::types::Value>>;
    pub(crate) fn tables(path: &Path) -> std::collections::BTreeMap<String, (Vec<String>, Rows)> {
        let c =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .expect("SQL");
        let names: Vec<String> = c
            .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
            .expect("names")
            .query_map([], |r| r.get(0))
            .expect("names")
            .collect::<rusqlite::Result<_>>()
            .expect("names");
        names
            .into_iter()
            .map(|name| {
                let quoted = format!("\"{}\"", name.replace('"', "\"\""));
                let mut stmt = c
                    .prepare(&format!("SELECT rowid,* FROM {quoted} ORDER BY rowid"))
                    .or_else(|_| c.prepare(&format!("SELECT * FROM {quoted} ORDER BY 1,2")))
                    .expect("table");
                let n = stmt.column_count();
                let columns: Vec<String> = (0..n)
                    .map(|i| stmt.column_name(i).expect("column").to_owned())
                    .collect();
                let rows = stmt
                    .query_map([], |r| (0..n).map(|i| r.get(i)).collect())
                    .expect("rows")
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .expect("rows");
                (name, (columns, rows))
            })
            .collect()
    }

    /// The same widget request, run once through the app's worker and once
    /// through the shipped CLI, publishes one document: every SQL cell is the
    /// same once the identifiers this operation minted are matched, and the
    /// exports are the same bytes.
    #[test]
    fn native_fillet_widgets_worker_and_cli_publish_the_same_part() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: the fillet worker needs OCCT");
            return;
        }
        let (root, path, source) = plate();
        let before = std::fs::read(&path).expect("source");
        let mut e = Editor::default();
        begin_by_button(&mut e, &path, &source);
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        click(&ctx, &mut e, "Corner (-4.5, 15.5)");
        radius(&ctx, &mut e, "3.0625");
        click(&ctx, &mut e, "Apply fillet");
        click(&ctx, &mut e, "Save fillet copy…");
        let mut request = e.take_request().expect("widget request");
        let fillet = request.fillet;

        let ui = root.path().join("worker.fcad");
        request.destination = ui.clone();
        let mut edits = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        edits
            .start_fillet(request, move |r, g, c| {
                crate::edits::spawn_fillet(r, c, move |result| tx.send((g, result)).expect("reply"))
            })
            .expect("worker");
        let (generation, result) = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("worker response");
        let published = result.as_ref().expect("published").clone();
        assert_eq!(published.corner.corner_mm, [-4.5, 15.5]);
        let typed = e.draft.as_ref().expect("draft").typed.clone();
        let mut editor = crate::sketch::Editor::default();
        editor.fillets = e;
        assert_eq!(
            finish_fillet(&mut editor, &mut edits, generation, result),
            Some(ui.clone()),
            "publication goes to the ordinary async Open"
        );
        assert!(!editor.active());
        editor.draft_load_finished(&ui, false);
        assert!(editor.fillets.active(), "a refused Open restores the draft");
        assert_eq!(editor.fillets.draft.as_ref().expect("draft").typed, typed);
        editor.draft_published(&ui);
        editor.draft_load_finished(&ui, true);
        assert!(!editor.active());

        // The same request through the shipped command line.
        let input = root.path().join("request.json");
        let [a, b] = fillet.edge.joint.segments();
        std::fs::write(
            &input,
            format!(
                r#"{{"request_version":1,"edge":{{"feature_id":"{}","joint":["{b}","{a}"]}},"radius_mm":{}}}"#,
                fillet.edge.feature, fillet.radius_mm
            ),
        )
        .expect("input");
        let peer = root.path().join("peer.fcad");
        let out = std::process::Command::new(crate::creates::tests::ferritecad())
            .arg("fillet-edge-copy")
            .arg(&path)
            .arg("--body")
            .arg(source.fillet_bodies[0].body.to_string())
            .arg("--expect-version")
            .arg(source.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&peer)
            .arg("--json")
            .output()
            .expect("peer");
        assert!(out.status.success(), "{out:?}");

        // Match the minted identifiers: the Fillet, then each new name by
        // what it means. Nothing else may differ.
        let a_doc = Document::open_read_only(&ui).expect("worker copy");
        let b_doc = Document::open_read_only(&peer).expect("CLI copy");
        let minted = |d: &Document| {
            let refs = d.topology_refs().expect("refs");
            let fillet = d
                .objects()
                .expect("objects")
                .into_iter()
                .find(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
                .expect("a Fillet")
                .id;
            (
                fillet,
                refs.into_iter()
                    .filter(|r| r.owner == fillet)
                    .collect::<Vec<_>>(),
            )
        };
        let (mine, my_refs) = minted(&a_doc);
        let (theirs, their_refs) = minted(&b_doc);
        a_doc.close().expect("close");
        b_doc.close().expect("close");
        assert_eq!(my_refs.len(), 7);
        let mut pairs: Vec<(Vec<u8>, Vec<u8>)> =
            vec![(theirs.to_bytes().to_vec(), mine.to_bytes().to_vec())];
        for reference in &their_refs {
            let matched = my_refs
                .iter()
                .find(|m| {
                    m.output_role == reference.output_role && m.selection == reference.selection
                })
                .expect("the same meaning");
            pairs.push((
                reference.id.to_bytes().to_vec(),
                matched.id.to_bytes().to_vec(),
            ));
        }
        let map = |value: &rusqlite::types::Value| -> rusqlite::types::Value {
            use rusqlite::types::Value;
            match value {
                Value::Blob(bytes) => {
                    let mut out = bytes.clone();
                    for (from, to) in &pairs {
                        let mut i = 0;
                        while i + 16 <= out.len() {
                            if out[i..i + 16] == from[..] {
                                out[i..i + 16].copy_from_slice(to);
                                i += 16;
                            } else {
                                i += 1;
                            }
                        }
                    }
                    Value::Blob(out)
                }
                other => other.clone(),
            }
        };
        let (left, right) = (tables(&ui), tables(&peer));
        assert_eq!(
            left.keys().collect::<Vec<_>>(),
            right.keys().collect::<Vec<_>>()
        );
        for (table, (columns, rows)) in &left {
            let (their_columns, theirs) = &right[table];
            assert_eq!(columns, their_columns, "{table} columns");
            assert_eq!(rows.len(), theirs.len(), "{table} rows");
            for (l, r) in rows.iter().zip(theirs) {
                let r: Vec<_> = r.iter().map(map).collect();
                for (i, column) in columns.iter().enumerate() {
                    match (table.as_str(), column.as_str()) {
                        // The copy's own timestamp.
                        ("meta", "modified_at") => {}
                        // A payload hash covers the minted identifiers, so it
                        // is compared through the payload it hashes.
                        ("objects", "payload_hash") => {
                            let payload = columns
                                .iter()
                                .position(|c| c == "payload")
                                .expect("payload column");
                            assert_eq!(l[payload], r[payload], "objects payload");
                            let rusqlite::types::Value::Blob(bytes) = &l[payload] else {
                                panic!("a payload blob");
                            };
                            assert_eq!(
                                l[i],
                                rusqlite::types::Value::Blob(
                                    ferritecad_types::ContentHash::of_bytes(bytes)
                                        .as_bytes()
                                        .to_vec()
                                ),
                                "the stored hash is the payload's"
                            );
                        }
                        _ => assert_eq!(l[i], r[i], "{table}.{column} differs"),
                    }
                }
            }
        }

        // The exports are the same bytes, and the source is untouched.
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

    /// §28B: the plate of [`plate`], with a §28A Fillet on its (33, 3.25)
    /// corner written by the shipped preparation and writer, and no kernel.
    /// §28C's height form tests use it too.
    pub(crate) fn rounded(radius: f64) -> (tempfile::TempDir, PathBuf, ExtrudeEditSource) {
        let (root, path, source) = plate();
        let target = source.fillet_bodies[0].target.clone().expect("a target");
        let corner = target
            .corners
            .iter()
            .find(|c| c.corner_mm == [33., 3.25])
            .expect("that corner");
        let mut d = Document::open(&path).expect("writable");
        let prepared = ferritecad_document::prepare_edge_fillet(
            &d,
            source.fillet_bodies[0].body,
            &EdgeFillet {
                edge: FilletEdge {
                    feature: target.base_feature,
                    joint: corner.joint,
                },
                radius_mm: radius,
            },
        )
        .expect("prepared");
        d.write_edge_fillet(&prepared).expect("written");
        let source = ExtrudeEditSource::read(&d).expect("snapshot");
        d.close().expect("close");
        (root, path, source)
    }

    /// The radius form names the Fillet, its edge and the saved radius,
    /// refuses what the document would, hands over exactly the widgets'
    /// request, and keeps its draft through a cancelled Save, a running job, a
    /// worker refusal and a stale reply. No kernel is involved.
    #[test]
    fn fillet_radius_widgets_show_the_saved_edge_and_keep_the_draft() {
        let (_root, path, source) = rounded(2.375);
        let choice = source.fillet_features[0].clone();
        let saved = choice.saved.clone().expect("editable");
        let mut e = Editor::default();
        begin_by(&mut e, &path, &source, "Edit Fillet radius");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        let out = frame(&ctx, &mut e, false);
        let [a, b] = saved.edge.joint.segments();
        assert!(
            painted(&out, &saved.feature.to_string()),
            "the Fillet is named"
        );
        assert!(painted(
            &out,
            &format!("Corner (33, 3.25) — Lines {a} | {b}")
        ));
        assert!(painted(
            &out,
            "Saved radius 2.375 mm; from 0.01 mm to 6.125 mm here."
        ));
        assert!(painted(&out, "the edge cannot be changed here"));
        assert_eq!(e.radius.as_ref().expect("draft").typed, "2.375");
        assert!(!painted(&out, "Save radius copy…"), "nothing applied yet");

        for (text, why) in [
            ("banana", "finite"),
            ("0", "at least"),
            ("6.2", "too large"),
            ("inf", "finite"),
        ] {
            enter(&ctx, &mut e, "New radius (mm):", text);
            click(&ctx, &mut e, "Apply radius");
            let draft = e.radius.as_ref().expect("draft");
            assert!(draft.applied.is_none(), "{text} was applied");
            assert!(
                draft.refusal.as_deref().expect("a refusal").contains(why),
                "{text}: {:?}",
                draft.refusal
            );
        }
        enter(&ctx, &mut e, "New radius (mm):", "4.8125");
        click(&ctx, &mut e, "Apply radius");
        let out = frame(&ctx, &mut e, false);
        assert!(painted(
            &out,
            "Ready: radius 2.375 mm -> 4.8125 mm at (33, 3.25)"
        ));
        enter(&ctx, &mut e, "New radius (mm):", "5");
        assert!(!painted(&frame(&ctx, &mut e, false), "Save radius copy…"));
        enter(&ctx, &mut e, "New radius (mm):", "4.8125");
        click(&ctx, &mut e, "Save radius copy…");
        let request = e.take_radius_request().expect("the widgets' request");
        assert!(e.take_radius_request().is_none(), "one press, one request");
        assert!(e.take_request().is_none(), "no Fillet is added");
        assert_eq!(request.source, path);
        assert_eq!(request.expected, source.version);
        assert_eq!(request.feature, saved.feature);
        assert_eq!(request.radius_mm, 4.8125);

        // A cancelled Save: nothing ran, and the draft is as it was.
        assert!(e.active());
        assert_eq!(e.radius.as_ref().expect("draft").typed, "4.8125");
        // While a job runs the form is inert and says the draft is kept.
        assert!(painted(
            &frame(&ctx, &mut e, true),
            "Draft retained until publication"
        ));
        click_while(&ctx, &mut e, "Cancel radius draft", true);
        assert!(e.active(), "Cancel is disabled while saving");

        // A stale reply and a worker refusal both keep the draft.
        let mut editor = crate::sketch::Editor::default();
        editor.fillets = e;
        let mut edits = crate::edits::Edits::default();
        let mut refused = request.clone();
        refused.destination = PathBuf::from("refused.fcad");
        let generation = edits
            .start_fillet_radius(refused, |_, _, _| std::thread::spawn(|| {}))
            .expect("started");
        let stale = ferritecad_jobs::EditedFilletRadius {
            destination: PathBuf::from("stale.fcad"),
            document_id: ferritecad_types::DocumentId::new(),
            body: ObjectId::new(),
            feature: ObjectId::new(),
            edge: saved.edge,
            corner_mm: [0., 0.],
            previous_radius_mm: 1.,
            radius_mm: 2.,
        };
        assert_eq!(
            finish_fillet_radius(&mut editor, &mut edits, generation + 1, Ok(stale)),
            None
        );
        assert!(editor.fillets.active());
        assert_eq!(
            finish_fillet_radius(
                &mut editor,
                &mut edits,
                generation,
                Err(ferritecad_types::CadError::kernel("refused by the worker"))
            ),
            None
        );
        assert!(editor.fillets.active(), "a refusal keeps the draft");
        assert_eq!(
            editor.fillets.radius.as_ref().expect("draft").typed,
            "4.8125"
        );

        // Cancel, when nothing runs, leaves nothing and starts nothing.
        let mut e = std::mem::take(&mut editor.fillets);
        click(&ctx, &mut e, "Cancel radius draft");
        assert!(!e.active());
        assert!(e.take_radius_request().is_none());
    }

    /// The same widget request through the app's worker and through the
    /// shipped CLI publishes one document: every SQL cell equal with no
    /// identifier mapped (nothing is minted), except each copy's own stamp,
    /// and byte-identical STL and FBX.
    #[test]
    fn native_fillet_radius_widgets_worker_and_cli_publish_the_same_part() {
        if !ferritecad_occt::is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: the Fillet radius worker needs OCCT");
            return;
        }
        let (root, path, source) = rounded(2.375);
        let before = std::fs::read(&path).expect("source");
        let mut e = Editor::default();
        begin_by(&mut e, &path, &source, "Edit Fillet radius");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut e, false);
        }
        enter(&ctx, &mut e, "New radius (mm):", "1.1875");
        click(&ctx, &mut e, "Apply radius");
        click(&ctx, &mut e, "Save radius copy…");
        let mut request = e.take_radius_request().expect("widget request");
        let radius = request.radius_mm;
        let feature = request.feature;

        let ui = root.path().join("worker.fcad");
        request.destination = ui.clone();
        let mut edits = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        edits
            .start_fillet_radius(request, move |r, g, c| {
                crate::edits::spawn_fillet_radius(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (generation, result) = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("worker response");
        let published = result.as_ref().expect("published").clone();
        assert_eq!(published.feature, feature);
        assert_eq!(published.previous_radius_mm, 2.375);
        assert_eq!(published.radius_mm, 1.1875);
        let mut editor = crate::sketch::Editor::default();
        editor.fillets = e;
        assert_eq!(
            finish_fillet_radius(&mut editor, &mut edits, generation, result),
            Some(ui.clone()),
            "publication goes to the ordinary async Open"
        );
        assert!(!editor.active());
        editor.draft_load_finished(&ui, false);
        assert!(editor.fillets.active(), "a refused Open restores the draft");
        editor.draft_published(&ui);
        editor.draft_load_finished(&ui, true);
        assert!(!editor.active());

        let input = root.path().join("request.json");
        std::fs::write(
            &input,
            format!(r#"{{"request_version":1,"radius_mm":{radius}}}"#),
        )
        .expect("input");
        let peer = root.path().join("peer.fcad");
        let out = std::process::Command::new(crate::creates::tests::ferritecad())
            .arg("edit-fillet-radius")
            .arg(&path)
            .arg("--feature")
            .arg(feature.to_string())
            .arg("--expect-version")
            .arg(source.version.content.to_string())
            .arg("--request")
            .arg(&input)
            .arg("-o")
            .arg(&peer)
            .arg("--json")
            .output()
            .expect("peer");
        assert!(out.status.success(), "{out:?}");

        let (left, right) = (tables(&ui), tables(&peer));
        let original = tables(&path);
        assert_eq!(
            left.keys().collect::<Vec<_>>(),
            right.keys().collect::<Vec<_>>()
        );
        for (table, (columns, rows)) in &left {
            let (their_columns, theirs) = &right[table];
            assert_eq!(columns, their_columns);
            assert_eq!(rows.len(), theirs.len(), "{table} rows");
            assert_eq!(
                rows.len(),
                original[table].1.len(),
                "{table} rows vs source"
            );
            for (l, r) in rows.iter().zip(theirs) {
                for (i, column) in columns.iter().enumerate() {
                    if table == "meta" && column == "modified_at" {
                        continue;
                    }
                    assert_eq!(l[i], r[i], "{table}.{column} differs");
                }
            }
        }
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
}
