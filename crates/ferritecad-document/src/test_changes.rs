// SPDX-License-Identifier: MIT
#![allow(dead_code, clippy::panic)]

pub fn constraint_changes(
    document: &crate::Document,
    id: ferritecad_types::ObjectId,
    edits: &crate::SketchConstraintEdits,
) -> ferritecad_types::Result<Vec<crate::EditChange>> {
    let object = document
        .object(id)?
        .ok_or_else(|| ferritecad_types::CadError::input("fixture sketch is missing"))?;
    let crate::ObjectPayload::Sketch(sketch) = object.payload else {
        return Err(ferritecad_types::CadError::input(
            "fixture object is not a sketch",
        ));
    };
    crate::sketch_constraint_changes(id, &sketch, edits)
}
pub fn added_constraints(changes: &[crate::EditChange]) -> Vec<&crate::SketchConstraint> {
    changes
        .iter()
        .filter_map(|c| match c {
            crate::EditChange::AddConstraint { constraint, .. } => Some(constraint),
            _ => None,
        })
        .collect()
}

pub fn vertex_changes(
    document: &crate::Document,
    id: ferritecad_types::ObjectId,
    vertices: &[crate::SketchVertex],
) -> ferritecad_types::Result<Vec<crate::EditChange>> {
    let object = document.object(id)?.expect("fixture sketch");
    let crate::ObjectPayload::Sketch(sketch) = object.payload else {
        panic!("fixture sketch")
    };
    crate::move_sketch_vertices(id, &sketch, vertices)
}
