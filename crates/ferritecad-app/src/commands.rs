// SPDX-License-Identifier: MIT
//! One command catalogue for existing model objects.

#[cfg(test)]
mod tests;

use crate::sketch_facts::Source as ExtrudeEditSource;
use ferritecad_document::{Document, DocumentVersion, EditChange, ObjectKind};
use ferritecad_types::{ObjectId, Result};
use ferritecad_ui::{ModelAction, ModelCommand, ModelEdit, ModelKey};

use crate::{
    Loads, creates::Creates, edits::Edits, exports::Exports, sessions::Sessions,
    sketch::accepted::Accepted, tabs::Tabs,
};

/// An open Parameters form defers the operation guard until the command is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Idle,
    /// An open form with the ordinary foreground-operation answer still to check.
    Parameters {
        operation: Result<(), &'static str>,
    },
}

/// One answer shared by the command catalogue and all of its surfaces.
pub(crate) type Availability = Result<Access, &'static str>;

impl Access {
    /// Applying the open form uses the same operation answer as its catalogue routes.
    pub(crate) fn operation(self) -> Result<(), &'static str> {
        match self {
            Self::Idle => Ok(()),
            Self::Parameters { operation } => operation,
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one availability answer over the existing window owners"
)]
pub(crate) fn availability(
    creates: &Creates,
    edits: &Edits,
    sessions: &Sessions,
    tabs: &Tabs,
    loads: &Loads,
    exports: &Exports,
    input: &ferritecad_ui::ViewportInput,
    confirming: bool,
) -> Availability {
    if confirming {
        return Err("Delete or Cancel the pending deletion before another command.");
    }
    let parameters = creates.sketch.parameters.active();
    if edits.form_open() || (creates.sketch.saved_object_open() && !parameters) {
        return Err("Apply or Cancel the open form before opening another editor.");
    }
    let operation = if !crate::can_leave_tab(creates, loads, exports, edits, sessions, input)
        || tabs.quitting()
        || tabs.closing_form()
        || !sessions.takes_form_apply()
    {
        Err("Finish the current operation, New, dialog or gesture before editing.")
    } else {
        Ok(())
    };
    if parameters {
        Ok(Access::Parameters { operation })
    } else {
        operation.map(|()| Access::Idle)
    }
}

/// Only genuinely supported routes of this accepted catalogue, by UUID.
pub(crate) fn actions(source: &ExtrudeEditSource, key: ModelKey) -> Vec<ModelEdit> {
    if source.refusal.is_some() || !source.model_tree.rows.iter().any(|r| r.id == key) {
        return Vec::new();
    }
    let id = key.0;
    let mut out = Vec::new();
    if source
        .objects
        .iter()
        .any(|o| o.id == id && o.payload.kind() == Some(ObjectKind::Parameter))
    {
        out.push(ModelEdit::Parameters);
    }
    if source.sketches.iter().any(|c| {
        c.sketch == id && c.refusal.is_none() && c.vertices.is_some() && c.profile_use.is_some()
    }) {
        out.push(ModelEdit::Vertices);
    }
    if source
        .circle_sketches
        .iter()
        .any(|c| c.sketch == id && c.refusal.is_none())
    {
        out.push(ModelEdit::Circle);
    }
    if source
        .annulus_sketches
        .iter()
        .any(|c| c.sketch == id && c.refusal.is_none())
    {
        out.push(ModelEdit::Annulus);
    }
    if source
        .constraint_sketches
        .iter()
        .any(|c| c.sketch == id && c.refusal.is_none())
    {
        out.push(ModelEdit::Constraints);
    }
    if source
        .cut_features
        .iter()
        .any(|c| c.feature == id && c.refusal.is_none() && c.saved.is_some())
    {
        out.push(ModelEdit::Cut);
    }
    for (slot, action) in [
        (
            ferritecad_document::ExpressionSlot::ExtrudeDistance,
            ModelEdit::Height,
        ),
        (
            ferritecad_document::ExpressionSlot::FilletRadius,
            ModelEdit::Fillet,
        ),
        (
            ferritecad_document::ExpressionSlot::ChamferDistance,
            ModelEdit::Chamfer,
        ),
        (
            ferritecad_document::ExpressionSlot::RevolveAngle,
            ModelEdit::RevolveAngle,
        ),
    ] {
        if source
            .expression_slot(id, slot)
            .is_some_and(|s| s.refusal.is_none())
        {
            out.push(action);
        }
    }
    out
}

fn open_editor(
    asked: ModelAction,
    source: &ExtrudeEditSource,
    accepted: &Accepted,
    creates: &mut Creates,
    edits: &mut Edits,
    path: &std::path::Path,
) -> bool {
    let id = asked.row.0;
    match asked.edit {
        ModelEdit::Parameters => creates.sketch.parameters.begin(source),
        ModelEdit::Height => edits.begin_selected(path, source, id),
        ModelEdit::Vertices => creates.sketch.begin_edit(path, source, id),
        ModelEdit::Circle => creates.sketch.begin_circle_edit(path, source, id),
        ModelEdit::Annulus => creates.sketch.begin_annulus_edit(path, source, id),
        ModelEdit::Constraints => creates
            .sketch
            .constraints
            .begin_with(path, source, accepted, id),
        ModelEdit::Cut => creates.sketch.cuts.begin_edit(path, source, id),
        ModelEdit::Fillet => creates.sketch.fillets.begin_radius(path, source, id),
        ModelEdit::Chamfer => creates.sketch.chamfers.begin_distance(path, source, id),
        ModelEdit::RevolveAngle => creates.sketch.begin_angle_edit(path, source, id),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Edit(ModelEdit),
    Delete,
    DeleteWithDependents,
    Suppress,
    Unsuppress,
    Rollback,
    RollToEnd,
}

pub(crate) type Action = ModelAction<Command>;

#[derive(Debug)]
pub(crate) struct Menu {
    tab: u64,
    epoch: u64,
    row: ModelKey,
    position: egui::Pos2,
}
impl Menu {
    pub(crate) fn new(
        sessions: &Sessions,
        tabs: &mut Tabs,
        row: ModelKey,
        position: egui::Pos2,
    ) -> Self {
        Self {
            tab: sessions.tab().key(),
            epoch: tabs.model_navigation(sessions.tab()).epoch,
            row,
            position,
        }
    }
    pub(crate) fn draw(
        &self,
        ctx: &egui::Context,
        source: &ExtrudeEditSource,
        idle: Availability,
    ) -> (Option<Action>, bool) {
        let mut asked = None;
        let mut close = false;
        let area = egui::Area::new(egui::Id::new("model entity commands"))
            .order(egui::Order::Foreground)
            .fixed_pos(self.position)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    if let Some(edit) =
                        ferritecad_ui::model_commands(ui, &offers(source, self.row, idle))
                    {
                        asked = Some(ModelAction {
                            tab: self.tab,
                            epoch: self.epoch,
                            row: self.row,
                            edit,
                        });
                    }
                    close = ui.button("Close menu").clicked();
                });
            });
        // Dismissed as any menu is: by a click outside it or by Escape.
        close |= area.response.clicked_elsewhere()
            || ctx.input(|input| input.key_pressed(egui::Key::Escape));
        (asked, close)
    }
}

pub(crate) fn picked_row(
    source: &ExtrudeEditSource,
    selection: &ferritecad_scene::Selection,
    catalogue: &[ferritecad_scene::CatalogueEntry],
    snapshot: &ferritecad_viewport::RenderSnapshot,
) -> std::result::Result<Option<ModelKey>, String> {
    use ferritecad_scene::{SceneItem, Selection};
    let meanings = match selection {
        Selection::Nothing => return Ok(None),
        Selection::Definition(_) => &[][..],
        Selection::Face(face) => face.meanings(),
        Selection::Edge(edge) => edge.meanings(),
        Selection::Vertex(vertex) => vertex.meanings(),
    };
    let entry = selection
        .owning_definition(snapshot)
        .and_then(|i| catalogue.get(i))
        .ok_or("The selection belongs to an older picture.")?;
    let (object, body) = match &entry.item {
        SceneItem::Body(body) => (producer(meanings)?, Some(*body)),
        SceneItem::Imported(reference) => {
            let mut imports = source.objects.iter().filter(|o| {
                matches!(
                    &o.payload, ferritecad_document::ObjectPayload::ImportedStep(step)
                        if step.source == reference.source()
                )
            });
            let object = imports
                .next()
                .ok_or("The selected import is no longer in the document.")?;
            if imports.next().is_some() {
                return Err(
                    "Several import objects own this source; the tree selection is unchanged."
                        .into(),
                );
            }
            (object.id, None)
        }
    };
    source
        .model_tree
        .rows
        .iter()
        .find(|r| r.id == (object, body, None))
        .map(|r| Some(r.id))
        .ok_or_else(|| "The producer is not in the displayed model tree.".into())
}

fn producer(meanings: &[ferritecad_scene::EntityMeaning]) -> std::result::Result<ObjectId, String> {
    match meanings {
        [meaning] => Ok(meaning.producer),
        [] => Err(
            "The selected entity has no stored meaning; the tree selection is unchanged.".into(),
        ),
        _ => {
            Err("The selected entity has several meanings; the tree selection is unchanged.".into())
        }
    }
}

pub(crate) fn select_row(
    source: &ExtrudeEditSource,
    navigation: &mut ferritecad_ui::ModelNavigation,
    row: ModelKey,
) {
    navigation.selected = Some(row);
    navigation.collapsed.remove(&row);
    let mut parent = source
        .model_tree
        .rows
        .iter()
        .find(|r| r.id == row)
        .and_then(|r| r.parent);
    while let Some(key) = parent {
        navigation.collapsed.remove(&key);
        parent = source
            .model_tree
            .rows
            .iter()
            .find(|r| r.id == key)
            .and_then(|r| r.parent);
    }
}

#[derive(Debug, Default)]
pub(crate) struct Chosen {
    pub(crate) window: ferritecad_ui::Chosen,
    pub(crate) model: Option<Action>,
    /// One text field submitted by the Parameters form this frame.
    pub(crate) parameter: Option<crate::parameters::Request>,
    pub(crate) confirm_delete: Option<bool>,
}
impl std::ops::Deref for Chosen {
    type Target = ferritecad_ui::Chosen;
    fn deref(&self) -> &Self::Target {
        &self.window
    }
}
impl std::ops::DerefMut for Chosen {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.window
    }
}

impl Command {
    fn label(self) -> &'static str {
        match self {
            Self::Edit(edit) => edit.label(),
            Self::Delete => "Delete",
            Self::DeleteWithDependents => "Delete with dependents…",
            Self::Suppress => "Suppress",
            Self::Unsuppress => "Unsuppress",
            Self::Rollback => "Roll back to here",
            Self::RollToEnd => "Roll to end",
        }
    }
    fn shortcuts(self) -> &'static [egui::KeyboardShortcut] {
        const DELETE: [egui::KeyboardShortcut; 2] = [
            egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Delete),
            egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Backspace),
        ];
        match self {
            Self::Delete => &DELETE,
            Self::Edit(_)
            | Self::DeleteWithDependents
            | Self::Suppress
            | Self::Unsuppress
            | Self::Rollback
            | Self::RollToEnd => &[],
        }
    }
    fn available(
        self,
        source: &ExtrudeEditSource,
        key: ModelKey,
        idle: Availability,
    ) -> std::result::Result<(), String> {
        if let Access::Parameters { operation } = idle.map_err(str::to_owned)? {
            if !matches!(self, Self::Delete | Self::DeleteWithDependents)
                || !source
                    .objects
                    .iter()
                    .any(|o| o.id == key.0 && o.payload.kind() == Some(ObjectKind::Parameter))
            {
                return Err("Close the Parameters form before another command.".into());
            }
            operation.map_err(str::to_owned)?;
        }
        if let Some(reason) = &source.refusal {
            return Err(reason.clone());
        }
        let row = source
            .model_tree
            .rows
            .iter()
            .find(|r| r.id == key)
            .ok_or("This object is no longer in the displayed model.")?;
        let object = source
            .objects
            .iter()
            .find(|o| o.id == row.id.0)
            .ok_or("This object is no longer in the document.")?;
        match self {
            Self::Edit(edit) => {
                if !actions(source, key).contains(&edit) {
                    return Err("This editor is unavailable for the selected object.".into());
                }
            }
            Self::Delete | Self::DeleteWithDependents => {}
            Self::Suppress | Self::Unsuppress => {
                if !object.payload.kind().is_some_and(|k| k.is_feature()) || key.1.is_none() {
                    return Err("Only a feature in a Body history can be suppressed.".into());
                }
                if (self == Self::Unsuppress) != object.suppressed {
                    return Err(if object.suppressed {
                        "This feature is already suppressed."
                    } else {
                        "This feature is not suppressed."
                    }
                    .into());
                }
            }
            Self::Rollback => {
                if !object.payload.kind().is_some_and(ObjectKind::is_feature) || key.1.is_none() {
                    return Err("Only a feature in a Body history can be rolled back to.".into());
                }
                let (_, body) = history_body(source, key)
                    .ok_or("Only a feature in a Body history can be rolled back to.")?;
                if body.shown_feature() == Some(object.id) {
                    return Err("The Body already shows this feature.".into());
                }
                if body.tip_feature == Some(object.id) {
                    return Err("This feature ends the history; use Roll to end.".into());
                }
            }
            Self::RollToEnd => {
                let (_, body) = history_body(source, key)
                    .ok_or("Only a Body or a feature in its history can roll to the end.")?;
                if body.rollback.is_none() {
                    return Err("This Body is not rolled back.".into());
                }
            }
        }
        Ok(())
    }
}

fn history_body(
    source: &ExtrudeEditSource,
    row: ModelKey,
) -> Option<(ObjectId, &ferritecad_document::Body)> {
    let id = match row {
        (_, Some(body), None) => body,
        (object, None, None) => object,
        (_, _, Some(_)) => return None,
    };
    source
        .objects
        .iter()
        .find_map(|object| match &object.payload {
            ferritecad_document::ObjectPayload::Body(body) if object.id == id => Some((id, body)),
            _ => None,
        })
}

pub(crate) fn offers(
    source: &ExtrudeEditSource,
    row: ModelKey,
    idle: Availability,
) -> Vec<ModelCommand<Command>> {
    actions(source, row)
        .into_iter()
        .map(Command::Edit)
        .chain([
            Command::Delete,
            Command::DeleteWithDependents,
            Command::Suppress,
            Command::Unsuppress,
            Command::Rollback,
            Command::RollToEnd,
        ])
        .map(|command| ModelCommand {
            command,
            label: command.label(),
            shortcuts: command.shortcuts(),
            available: command.available(source, row, idle),
        })
        .collect()
}

#[derive(Debug)]
pub(crate) struct Confirmation {
    action: Action,
    version: DocumentVersion,
    objects: Vec<(ObjectId, String, String)>,
}

impl Confirmation {
    fn read(action: Action, path: &std::path::Path, version: DocumentVersion) -> Result<Self> {
        let doc = Document::open_read_only(path)?;
        let objects = doc
            .dependent_closure(action.row.0)?
            .into_iter()
            .map(|id| {
                let row = doc.object(id)?.expect("closure contains stored objects");
                Ok((
                    id,
                    row.name.unwrap_or_else(|| "Unnamed".into()),
                    row.payload.kind().expect("stored kind").label().to_owned(),
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            action,
            version,
            objects,
        })
    }
    pub(crate) fn draw(&self, ui: &mut egui::Ui) -> Option<bool> {
        ui.heading("Delete with dependents");
        let count = self.objects.len();
        let noun = if count == 1 { "object" } else { "objects" };
        ui.label(format!("Delete {count} {noun}?"));
        for (_, name, kind) in &self.objects {
            ui.label(format!("{name} · {kind}"));
        }
        let mut answer = None;
        ui.horizontal(|ui| {
            if ui.button("Delete").clicked() {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
        answer
    }
    pub(crate) fn finish(
        self,
        delete: bool,
        source: Option<&ExtrudeEditSource>,
        sessions: &Sessions,
        tabs: &Tabs,
        idle: Availability,
    ) -> std::result::Result<Option<(DocumentVersion, Vec<EditChange>)>, String> {
        if !delete {
            return Ok(None);
        }
        let source = source.ok_or("The deletion belongs to an older document view.")?;
        self.action.edit.available(source, self.action.row, idle)?;
        current_action(self.action, self.version, sessions, tabs)?;
        Ok(Some((
            self.version,
            self.objects
                .into_iter()
                .map(|(object, _, _)| EditChange::RemoveObject { object })
                .collect(),
        )))
    }
}

fn current_action(
    asked: Action,
    version: DocumentVersion,
    sessions: &Sessions,
    tabs: &Tabs,
) -> std::result::Result<(), String> {
    if asked.tab != sessions.tab().key()
        || !tabs.model_addressed(sessions.tab(), asked)
        || sessions
            .export_source()
            .is_none_or(|s| s.version() != version)
    {
        return Err("The command belongs to an older document view.".into());
    }
    Ok(())
}

pub(crate) enum Launch {
    Opened(bool),
    Confirm(Confirmation),
    Apply(DocumentVersion, Vec<EditChange>),
}

#[allow(
    clippy::too_many_arguments,
    reason = "route through the existing window owners"
)]
pub(crate) fn launch(
    asked: Action,
    source: &ExtrudeEditSource,
    accepted: &Accepted,
    creates: &mut Creates,
    edits: &mut Edits,
    sessions: &Sessions,
    tabs: &Tabs,
    loads: &Loads,
    exports: &Exports,
    input: &ferritecad_ui::ViewportInput,
    confirming: bool,
) -> std::result::Result<Launch, String> {
    let idle = availability(
        creates, edits, sessions, tabs, loads, exports, input, confirming,
    );
    asked.edit.available(source, asked.row, idle)?;
    current_action(asked, source.version, sessions, tabs)?;
    match asked.edit {
        Command::Edit(edit) => Ok(Launch::Opened(open_editor(
            ModelAction {
                tab: asked.tab,
                epoch: asked.epoch,
                row: asked.row,
                edit,
            },
            source,
            accepted,
            creates,
            edits,
            sessions.export_source().expect("checked document").path(),
        ))),
        Command::DeleteWithDependents => {
            let snapshot = sessions.export_source().expect("checked document");
            Confirmation::read(asked, snapshot.path(), source.version)
                .map(Launch::Confirm)
                .map_err(|e| e.to_string())
        }
        Command::Delete => Ok(Launch::Apply(
            source.version,
            vec![EditChange::RemoveObject {
                object: asked.row.0,
            }],
        )),
        Command::Suppress | Command::Unsuppress => Ok(Launch::Apply(
            source.version,
            vec![EditChange::SetSuppressed {
                object: asked.row.0,
                suppressed: asked.edit == Command::Suppress,
            }],
        )),
        Command::Rollback | Command::RollToEnd => Ok(Launch::Apply(
            source.version,
            vec![EditChange::SetRollback {
                object: history_body(source, asked.row).expect("available Body").0,
                feature: (asked.edit == Command::Rollback).then_some(asked.row.0),
            }],
        )),
    }
}

impl crate::App {
    /// Answers "what is under this point", chooses it and selects the Model row
    /// of its producer, which is the row returned.
    ///
    /// One offscreen frame at the camera the window is already using, because
    /// identities are not written on the path a window takes: paying for them
    /// on every frame would be paying continuously for something wanted when
    /// somebody clicks.
    pub(crate) fn choose_at(&mut self, x: f32, y: f32) -> Option<ModelKey> {
        let live = self.live.as_mut()?;
        // Rounded rather than truncated, and refused outside the picture: a
        // pointer position is a float in window coordinates and the frame is a
        // grid of pixels.
        let (Ok(x), Ok(y)) = (
            u32::try_from(x.round() as i64),
            u32::try_from(y.round() as i64),
        ) else {
            return None;
        };
        let hit = match Self::hit_at(live, self.input.camera(), x as f32, y as f32) {
            Ok(hit) => hit,
            // A failed pick chooses nothing and changes nothing. The model is
            // still on screen and still correct; the click simply went
            // unanswered.
            Err(error) => {
                crate::journal::report(format_args!("ferritecad: {error}"));
                return None;
            }
        };
        let snapshot = live.scene.prepared.snapshot();
        live.scene.selection = crate::selection_at(
            hit,
            snapshot,
            &live.scene.faces,
            &live.scene.edges,
            &live.scene.vertices,
        );
        self.input.request_redraw();
        let source = live.scene.edit_source.as_ref()?;
        match picked_row(
            source,
            &live.scene.selection,
            &live.scene.catalogue,
            snapshot,
        ) {
            Ok(Some(row)) => {
                select_row(source, self.tabs.model_navigation(self.sessions.tab()), row);
                Some(row)
            }
            Ok(None) => None,
            Err(reason) => {
                self.sessions.status = reason;
                None
            }
        }
    }
    pub(crate) fn run_model_command(&mut self, asked: Action) {
        let Some(live) = &self.live else {
            return;
        };
        let Some(source) = &live.scene.edit_source else {
            return;
        };
        match launch(
            asked,
            source,
            &live.scene.sketches,
            &mut self.creates,
            &mut self.edits,
            &self.sessions,
            &self.tabs,
            &self.loads,
            &self.exports,
            &self.input,
            self.model_confirmation.is_some(),
        ) {
            Ok(Launch::Opened(true)) => {}
            Ok(Launch::Opened(false)) => {
                self.sessions.status = "The selected editor could not be opened.".into();
            }
            Ok(Launch::Confirm(confirmation)) => self.model_confirmation = Some(confirmation),
            Ok(Launch::Apply(expected, changes)) => self.apply_model_changes(expected, changes),
            Err(reason) => self.sessions.status = reason,
        }
        self.input.request_redraw();
    }
    fn apply_model_changes(&mut self, expected: DocumentVersion, changes: Vec<EditChange>) {
        let proxy = self.proxy.clone();
        self.sessions
            .apply(expected, changes, move |generation, result| {
                let _ = proxy.send_event(crate::AppEvent::Applied {
                    generation,
                    result: Box::new(result),
                });
            });
    }
    pub(crate) fn confirm_model_deletion(&mut self, delete: bool) {
        let Some(confirmation) = self.model_confirmation.take() else {
            return;
        };
        let source = self
            .live
            .as_ref()
            .and_then(|live| live.scene.edit_source.as_ref());
        let idle = availability(
            &self.creates,
            &self.edits,
            &self.sessions,
            &self.tabs,
            &self.loads,
            &self.exports,
            &self.input,
            false,
        );
        match confirmation.finish(delete, source, &self.sessions, &self.tabs, idle) {
            Ok(Some((expected, changes))) => self.apply_model_changes(expected, changes),
            Ok(None) => {}
            Err(reason) => self.sessions.status = reason,
        }
        self.input.request_redraw();
    }
}
