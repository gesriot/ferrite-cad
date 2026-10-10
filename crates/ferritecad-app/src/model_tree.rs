// SPDX-License-Identifier: MIT
//! §31A: one addressed navigation route to the pre-existing form owners.

use ferritecad_document::ExtrudeEditSource;
use ferritecad_ui::{ModelAction, ModelEdit, ModelKey};

use crate::{
    Loads, creates::Creates, edits::Edits, exports::Exports, sessions::Sessions, tabs::Tabs,
};

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
) -> Result<(), &'static str> {
    if edits.form_open() || creates.sketch.saved_object_open() {
        return Err("Apply or Cancel the open form before opening another editor.");
    }
    if !crate::can_leave_tab(creates, loads, exports, edits, sessions, input)
        || tabs.quitting()
        || tabs.closing_form()
        || !sessions.takes_form_apply()
    {
        return Err("Finish the current operation, New, dialog or gesture before editing.");
    }
    Ok(())
}

/// Only genuinely supported routes of this accepted catalogue, by UUID.
pub(crate) fn actions(source: &ExtrudeEditSource, key: ModelKey) -> Vec<ModelEdit> {
    if source.refusal.is_some() || !source.model_tree.rows.iter().any(|r| r.id == key) {
        return Vec::new();
    }
    let id = key.0;
    let mut out = Vec::new();
    if source.unavailable_reason().is_none()
        && source
            .features
            .iter()
            .any(|c| c.feature == id && c.refusal.is_none())
    {
        out.push(ModelEdit::Height);
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
    if source
        .fillet_features
        .iter()
        .any(|c| c.feature == id && c.refusal.is_none() && c.saved.is_some())
    {
        out.push(ModelEdit::Fillet);
    }
    if source
        .chamfer_features
        .iter()
        .any(|c| c.feature == id && c.refusal.is_none() && c.saved.is_some())
    {
        out.push(ModelEdit::Chamfer);
    }
    if source
        .revolve_angles
        .iter()
        .any(|c| c.feature == id && c.refusal.is_none() && c.degrees.is_some())
    {
        out.push(ModelEdit::RevolveAngle);
    }
    out
}

#[allow(
    clippy::too_many_arguments,
    reason = "the existing window owners share one availability predicate"
)]
pub(crate) fn open(
    asked: ModelAction,
    source: &ExtrudeEditSource,
    creates: &mut Creates,
    edits: &mut Edits,
    sessions: &Sessions,
    tabs: &Tabs,
    loads: &Loads,
    exports: &Exports,
    input: &ferritecad_ui::ViewportInput,
) -> bool {
    if availability(creates, edits, sessions, tabs, loads, exports, input).is_err()
        || asked.tab != sessions.tab().key()
        || !tabs.model_addressed(sessions.tab(), asked)
        || !actions(source, asked.row).contains(&asked.edit)
    {
        return false;
    }
    let Some(snapshot) = sessions.export_source() else {
        return false;
    };
    if snapshot.version() != source.version {
        return false;
    }
    let path = snapshot.path();
    let id = asked.row.0;
    match asked.edit {
        ModelEdit::Height => edits.begin_selected(path, source, id),
        ModelEdit::Vertices => creates.sketch.begin_edit(path, source, id),
        ModelEdit::Circle => creates.sketch.begin_circle_edit(path, source, id),
        ModelEdit::Annulus => creates.sketch.begin_annulus_edit(path, source, id),
        ModelEdit::Constraints => creates.sketch.constraints.begin(path, source, id),
        ModelEdit::Cut => creates.sketch.cuts.begin_edit(path, source, id),
        ModelEdit::Fillet => creates.sketch.fillets.begin_radius(path, source, id),
        ModelEdit::Chamfer => creates.sketch.chamfers.begin_distance(path, source, id),
        ModelEdit::RevolveAngle => creates.sketch.begin_angle_edit(path, source, id),
    }
}
