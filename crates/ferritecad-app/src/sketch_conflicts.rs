// SPDX-License-Identifier: MIT
use ferritecad_scene::SketchConflict;
use ferritecad_types::ObjectId;

use crate::{ConstraintWords, sketch_facts::Source};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Conflict {
    sketch: ObjectId,
    identifier: String,
    name: String,
    constraints: Vec<ConstraintWords>,
}

pub(crate) fn attach(source: &mut Source, conflicts: &[SketchConflict]) {
    let conflicts = conflicts
        .iter()
        .map(|conflict| {
            let name = source
                .objects
                .iter()
                .find(|o| o.id == conflict.sketch())
                .and_then(|o| o.name.as_deref())
                .unwrap_or("Unnamed sketch")
                .to_owned();
            let constraints: Vec<_> = conflict
                .constraints()
                .iter()
                .map(|c| ConstraintWords::of(c.id(), c.rule()))
                .collect();
            Conflict {
                sketch: conflict.sketch(),
                identifier: conflict.sketch().to_string(),
                name,
                constraints,
            }
        })
        .collect::<Vec<_>>();
    notes(&mut source.model_tree, &conflicts);
    source.conflicts = conflicts;
}

/// Checkpoint-only changes retain the facts of the same evaluated picture.
pub(crate) fn follow(previous: Option<&mut Source>, next: &mut Source) {
    if let Some(previous) = previous {
        let conflicts = std::mem::take(&mut previous.conflicts);
        notes(&mut next.model_tree, &conflicts);
        next.conflicts = conflicts;
    }
}

fn notes(tree: &mut ferritecad_document::ModelTree, conflicts: &[Conflict]) {
    for conflict in conflicts {
        let words = conflict
            .constraints
            .iter()
            .map(|c| format!("{} ({})", c.says, c.identifier))
            .collect::<Vec<_>>()
            .join("; ");
        for row in &mut tree.rows {
            if row.id.0 == conflict.sketch {
                row.name = format!("⚠ {}", conflict.name);
                row.note = format!("Constraint conflict: {words}");
            }
        }
    }
}

pub(crate) fn panel(
    ui: &mut egui::Ui,
    solved: &[ferritecad_ui::SolvedSketch<'_>],
    source: Option<&Source>,
) {
    let conflicts = source.map_or(&[][..], |s| s.conflicts.as_slice());
    let rules: Vec<Vec<_>> = conflicts
        .iter()
        .map(|c| {
            c.constraints
                .iter()
                .map(|rule| ferritecad_ui::ConflictingRule {
                    identifier: &rule.identifier,
                    says: &rule.says,
                })
                .collect()
        })
        .collect();
    let rows: Vec<_> = conflicts
        .iter()
        .zip(&rules)
        .map(|(conflict, rules)| ferritecad_ui::ConflictedSketch {
            name: &conflict.name,
            object: &conflict.identifier,
            constraints: rules,
        })
        .collect();
    ferritecad_ui::sketch_solves_panel(ui, solved, &rows);
}
