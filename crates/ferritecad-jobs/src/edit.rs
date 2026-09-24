// SPDX-License-Identifier: MIT
//! Edit one existing feature in a preserved SQLite copy, then cold-check and publish.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ferritecad_document::{Access, Document, DocumentVersion, ObjectPayload};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{GeometryKernel, OperationContext, ProgressSink};
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};

use crate::{Existing, Temporary, path_entry_exists, refuse_source_as_destination};

#[derive(Debug, Clone)]
pub struct EditExtrudeRequest {
    pub source: PathBuf,
    pub expected: DocumentVersion,
    pub feature: ObjectId,
    pub distance_mm: f64,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedDocument {
    pub destination: PathBuf,
    pub document_id: ferritecad_types::DocumentId,
    pub feature: ObjectId,
}

/// A saved version of the same model, preserving every identity. The caller
/// owns the kernel thread. Cancellation before publication leaves no output;
/// after publication this returns success even if cancellation has arrived.
pub fn edit_extrude_copy<K: GeometryKernel + ?Sized>(
    request: &EditExtrudeRequest,
    kernel: &mut K,
    context: &OperationContext,
) -> Result<EditedDocument> {
    context.check_cancelled()?;
    ferritecad_document::validate_extrude_distance(request.distance_mm)?;
    edit_object_copy(
        &request.source,
        request.expected,
        &request.destination,
        kernel,
        context,
        |source| {
            Ok(CopyWrite::Height(Box::new(
                ferritecad_document::prepare_extrude_height(
                    source,
                    request.feature,
                    request.distance_mm,
                )?,
            )))
        },
        |_, _| Ok(()),
    )?;
    Ok(EditedDocument {
        destination: request.destination.clone(),
        document_id: request.expected.document_id,
        feature: request.feature,
    })
}

#[derive(Debug, Clone)]
pub struct EditSketchRequest {
    pub source: PathBuf,
    pub expected: DocumentVersion,
    pub sketch: ObjectId,
    pub vertices: Vec<ferritecad_document::SketchVertex>,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedSketch {
    pub destination: PathBuf,
    pub document_id: ferritecad_types::DocumentId,
    pub sketch: ObjectId,
}

pub fn edit_sketch_copy<K: GeometryKernel + ?Sized>(
    request: &EditSketchRequest,
    kernel: &mut K,
    context: &OperationContext,
) -> Result<EditedSketch> {
    context.check_cancelled()?;
    edit_object_copy(
        &request.source,
        request.expected,
        &request.destination,
        kernel,
        context,
        |source| {
            Ok(CopyWrite::Coordinates(
                ferritecad_document::replace_sketch_coordinates(
                    source,
                    request.sketch,
                    &request.vertices,
                )?,
            ))
        },
        |_, _| Ok(()),
    )?;
    Ok(EditedSketch {
        destination: request.destination.clone(),
        document_id: request.expected.document_id,
        sketch: request.sketch,
    })
}

#[derive(Debug, Clone)]
pub struct EditCircleRequest {
    pub source: PathBuf,
    pub expected: DocumentVersion,
    pub sketch: ObjectId,
    /// The circle to move or resize, named by its own UUID. No height: the
    /// saved extrusion decides that, and `edit_extrude_copy` changes it.
    pub edit: ferritecad_document::CircleEdit,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedCircle {
    pub destination: PathBuf,
    pub document_id: ferritecad_types::DocumentId,
    pub sketch: ObjectId,
    pub curve: StableEntityId,
}

/// Change the centre and radius of one saved analytic circle in a new copy.
///
/// The same snapshot, baseline rebuild, reference check, version recheck and
/// atomic publication every other copy edit uses; only the prepared payload
/// differs. Nothing here opens a second copier or relaxes the reference rule.
pub fn edit_circle_copy<K: GeometryKernel + ?Sized>(
    request: &EditCircleRequest,
    kernel: &mut K,
    context: &OperationContext,
) -> Result<EditedCircle> {
    context.check_cancelled()?;
    edit_object_copy(
        &request.source,
        request.expected,
        &request.destination,
        kernel,
        context,
        |source| {
            ferritecad_document::replace_circle_geometry(source, request.sketch, &request.edit)
                .map(CopyWrite::Circle)
        },
        |_, _| Ok(()),
    )?;
    Ok(EditedCircle {
        destination: request.destination.clone(),
        document_id: request.expected.document_id,
        sketch: request.sketch,
        curve: request.edit.curve_id,
    })
}

#[derive(Debug, Clone)]
pub struct EditAnnulusRequest {
    pub source: PathBuf,
    pub expected: DocumentVersion,
    pub sketch: ObjectId,
    /// The two circles to move and resize, each named by its own UUID in the
    /// role it already holds. No height: the saved extrusion decides that, and
    /// `edit_extrude_copy` changes it.
    pub edit: ferritecad_document::AnnulusEdit,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedAnnulus {
    pub destination: PathBuf,
    pub document_id: ferritecad_types::DocumentId,
    pub sketch: ObjectId,
    pub outer_curve: StableEntityId,
    pub inner_curve: StableEntityId,
}

/// Change the shared centre and both radii of one saved annular profile in a
/// new copy.
///
/// The same snapshot, baseline rebuild, reference check, version recheck and
/// atomic publication every other copy edit uses; only the prepared payload
/// differs. Nothing here opens a second copier or relaxes the reference rule.
pub fn edit_annulus_copy<K: GeometryKernel + ?Sized>(
    request: &EditAnnulusRequest,
    kernel: &mut K,
    context: &OperationContext,
) -> Result<EditedAnnulus> {
    context.check_cancelled()?;
    edit_object_copy(
        &request.source,
        request.expected,
        &request.destination,
        kernel,
        context,
        |source| {
            ferritecad_document::replace_annulus_geometry(source, request.sketch, &request.edit)
                .map(CopyWrite::Annulus)
        },
        |_, _| Ok(()),
    )?;
    Ok(EditedAnnulus {
        destination: request.destination.clone(),
        document_id: request.expected.document_id,
        sketch: request.sketch,
        outer_curve: request.edit.outer_curve_id,
        inner_curve: request.edit.inner_curve_id,
    })
}

#[derive(Debug, Clone)]
pub struct EditSketchConstraintsRequest {
    pub source: PathBuf,
    pub expected: DocumentVersion,
    pub sketch: ObjectId,
    pub edits: ferritecad_document::SketchConstraintEdits,
    pub destination: PathBuf,
}
#[derive(Debug, Clone, PartialEq)]
pub struct EditedSketchConstraints {
    pub destination: PathBuf,
    pub document_id: ferritecad_types::DocumentId,
    pub sketch: ObjectId,
    pub added: Vec<ferritecad_document::SketchConstraint>,
    pub removed: Vec<StableEntityId>,
    /// What the solve found out, and `None` when the edit left nothing to
    /// solve.
    ///
    /// Reachable only since a profile could be one analytic circle: a Line
    /// profile keeps its Coincident closure however many dimensions are taken
    /// off it, so there was always a system. A circle has no joints, so
    /// removing its last constraint leaves a drawing with no relationships —
    /// and a sketch with none asks the solver nothing, exactly as a document
    /// written before constraints existed does.
    pub solve: Option<ferritecad_eval::SketchSolveReport>,
}

pub fn edit_sketch_constraints_copy<K: GeometryKernel + ?Sized>(
    request: &EditSketchConstraintsRequest,
    kernel: &mut K,
    context: &OperationContext,
) -> Result<EditedSketchConstraints> {
    context.check_cancelled()?;
    edit_object_copy(
        &request.source,
        request.expected,
        &request.destination,
        kernel,
        context,
        |source| {
            ferritecad_document::prepare_sketch_constraints(source, request.sketch, &request.edits)
                .map(CopyWrite::Constraints)
        },
        |prepared, solve| {
            let CopyWrite::Constraints(prepared) = prepared else {
                return Err(CadError::input("missing prepared constraint edit"));
            };
            Ok(EditedSketchConstraints {
                destination: request.destination.clone(),
                document_id: request.expected.document_id,
                sketch: request.sketch,
                added: prepared.added.clone(),
                removed: prepared.removed.clone(),
                solve,
            })
        },
    )
}

/// What one published circular cut is, in identities.
#[derive(Debug, Clone, PartialEq)]
pub struct CircularCutRequest {
    pub source: PathBuf,
    pub expected: DocumentVersion,
    pub body: ObjectId,
    pub cut: ferritecad_document::CircularCut,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AddedCircularCut {
    pub destination: PathBuf,
    pub document_id: ferritecad_types::DocumentId,
    /// The body that gained the feature. Its identity is the source's.
    pub body: ObjectId,
    /// The feature the body's tip now is.
    pub feature: ObjectId,
    /// The sketch the tool was drawn on.
    pub sketch: ObjectId,
    /// The circle inside it, so a caller can name the bore it made.
    pub tool_curve: StableEntityId,
    /// The feature the new one modifies, which was the tip before.
    pub previous: ObjectId,
    /// The intent the new Cut stores: a stated depth or ThroughAll.
    pub extent: ferritecad_document::CutExtent,
}

/// Adds one circular cut to a saved body, publishing a new copy.
///
/// The same snapshot, version guard, read-only source, baseline rebuild,
/// reference check, SQLite close and atomic no-clobber publication every other
/// copy operation uses. What differs is only what is written.
pub fn circular_cut_copy<K: GeometryKernel + ?Sized>(
    request: &CircularCutRequest,
    kernel: &mut K,
    context: &OperationContext,
) -> Result<AddedCircularCut> {
    context.check_cancelled()?;
    edit_object_copy(
        &request.source,
        request.expected,
        &request.destination,
        kernel,
        context,
        |source| {
            ferritecad_document::prepare_circular_cut(source, request.body, &request.cut)
                .map(Box::new)
                .map(CopyWrite::Cut)
        },
        |prepared, _| {
            let CopyWrite::Cut(prepared) = prepared else {
                return Err(CadError::input("missing prepared cut"));
            };
            Ok(AddedCircularCut {
                destination: request.destination.clone(),
                document_id: request.expected.document_id,
                body: request.body,
                feature: prepared.feature().id,
                sketch: prepared.sketch().id,
                tool_curve: prepared.tool_curve(),
                previous: prepared.previous(),
                extent: prepared.extent(),
            })
        },
    )
}

/// What one published parameter edit of a saved cut is, in identities.
#[derive(Debug, Clone, PartialEq)]
pub struct EditCircularCutRequest {
    pub source: PathBuf,
    pub expected: DocumentVersion,
    /// The saved Cut feature whose tool and depth change. Its identity is kept.
    pub cut: ObjectId,
    pub edit: ferritecad_document::CircularCutEdit,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditedCircularCut {
    pub destination: PathBuf,
    pub document_id: ferritecad_types::DocumentId,
    /// The body whose history contains the cut; unchanged by this edit.
    pub body: ObjectId,
    /// The edited feature, under the identity it already had.
    pub feature: ObjectId,
    /// The sketch the tool is drawn on, under the identity it already had.
    pub sketch: ObjectId,
    /// The circle inside it, which keeps its identity across the edit.
    pub tool_curve: StableEntityId,
    /// The feature the cut modifies; unchanged by this edit.
    pub previous: ObjectId,
    /// Whether the published cut stops inside the part.
    pub leaves_a_floor: bool,
    /// The intent the edited Cut stores: a stated depth or ThroughAll.
    pub extent: ferritecad_document::CutExtent,
}

/// Changes the tool and depth of one saved circular cut, publishing a new copy.
///
/// The same snapshot, version guard, read-only source, baseline rebuild,
/// reference check, SQLite close and atomic no-clobber publication every other
/// copy operation uses. What differs is only what is written: two payloads that
/// already existed, and the floor names gained by stopping inside the part:
/// its own historical name, plus a final origin name when editing the first
/// of two cuts.
pub fn edit_circular_cut_copy<K: GeometryKernel + ?Sized>(
    request: &EditCircularCutRequest,
    kernel: &mut K,
    context: &OperationContext,
) -> Result<EditedCircularCut> {
    context.check_cancelled()?;
    edit_object_copy(
        &request.source,
        request.expected,
        &request.destination,
        kernel,
        context,
        |source| {
            ferritecad_document::prepare_cut_parameters(source, request.cut, &request.edit)
                .map(Box::new)
                .map(CopyWrite::CutParameters)
        },
        |prepared, _| {
            let CopyWrite::CutParameters(prepared) = prepared else {
                return Err(CadError::input("missing prepared cut edit"));
            };
            Ok(EditedCircularCut {
                destination: request.destination.clone(),
                document_id: request.expected.document_id,
                body: prepared.body(),
                feature: prepared.feature().id,
                sketch: prepared.tool_sketch().id,
                tool_curve: prepared.tool_curve(),
                previous: prepared.previous(),
                leaves_a_floor: prepared.leaves_a_floor(),
                extent: prepared.extent(),
            })
        },
    )
}

enum CopyWrite {
    Height(Box<ferritecad_document::PreparedExtrudeHeight>),
    /// Boxed: this variant is much larger than the others, and an enum sized
    /// for it would make every copy operation carry the difference.
    Cut(Box<ferritecad_document::PreparedCircularCut>),
    /// Boxed for the same reason as the one above it.
    CutParameters(Box<ferritecad_document::PreparedCutParameters>),
    Coordinates(ferritecad_document::ObjectRecord),
    Constraints(ferritecad_document::PreparedSketchConstraints),
    Circle(ferritecad_document::ObjectRecord),
    Annulus(ferritecad_document::ObjectRecord),
}
impl CopyWrite {
    fn object(&self) -> &ferritecad_document::ObjectRecord {
        match self {
            Self::Coordinates(o) | Self::Circle(o) | Self::Annulus(o) => o,
            Self::Height(p) => p.feature(),
            Self::Constraints(p) => p.object(),
            // The body is the one object a cut changes; the two it adds did
            // not exist to be read.
            Self::Cut(p) => p.body(),
            // Two objects change here, and this is the one a generic write
            // would name. Nothing but that generic write uses it, and this
            // variant does not take it.
            Self::CutParameters(p) => p.feature(),
        }
    }

    /// Whether every reference the saved model already resolved must still
    /// resolve after the edit.
    ///
    /// True for every edit to a profile: moving a vertex, a constraint or a
    /// circle may not cost the document a name it had. An extrusion distance
    /// edit without a Cut history predates the rule and keeps its weaker promise,
    /// so it is named here rather than everything else being exempted by
    /// default.
    fn requires_resolved_references(&self) -> bool {
        !matches!(self, Self::Height(p) if p.history().is_none())
    }
}

/// Shared snapshot, transaction, cold references, cancellation and publication.
/// All owned completion facts are prepared fallibly BEFORE publication.
fn edit_object_copy<K: GeometryKernel + ?Sized, T>(
    source_path: &Path,
    expected: DocumentVersion,
    destination: &Path,
    kernel: &mut K,
    context: &OperationContext,
    prepare: impl FnOnce(&Document) -> Result<CopyWrite>,
    complete: impl FnOnce(&CopyWrite, Option<ferritecad_eval::SketchSolveReport>) -> Result<T>,
) -> Result<T> {
    refuse_source_as_destination(
        source_path,
        destination,
        "source and output must be different files",
    )?;
    if path_entry_exists(destination)? {
        return Err(CadError::input(
            "output already exists; choose a different file name",
        ));
    }
    let source = Document::open_read_only(source_path)?;
    require_version(&source, expected)?;
    if let Access::ReadOnly { reason } = source.copy_access()? {
        return Err(CadError::unsupported(format!(
            "document cannot be edited: {reason}"
        )));
    }
    let prepared = prepare(&source)?;
    let selected = prepared.object();
    let temporary = Temporary::beside(destination)?;
    source.snapshot_to(temporary.path())?;
    source.close()?;
    context.progress().report(0.1);
    context.check_cancelled()?;

    let mut document = Document::open(temporary.path())?;
    // Baseline and edited refs are compared by their stored IDs. An already
    // unresolved ref may remain unresolved; a previously resolved one may not
    // be lost. Rebuild errors (including solver diagnostics) always refuse.
    let baseline = checked_rebuild(&document, kernel, &phase(context, 0.1, 0.4), None, None)?.0;
    if prepared.requires_resolved_references() && baseline.len() != document.topology_refs()?.len()
    {
        return Err(CadError::topology(
            "saved sketch has unresolved topology references",
        ));
    }
    context.progress().report(0.4);
    context.check_cancelled()?;
    match &prepared {
        CopyWrite::Coordinates(prepared) => document.write_sketch_geometry(prepared)?,
        CopyWrite::Constraints(p) => document.write_sketch_constraints(p)?,
        CopyWrite::Circle(prepared) => document.write_circle_geometry(prepared)?,
        CopyWrite::Annulus(prepared) => document.write_annulus_geometry(prepared)?,
        CopyWrite::Cut(prepared) => document.write_circular_cut(prepared)?,
        CopyWrite::CutParameters(prepared) => document.write_cut_parameters(prepared)?,
        CopyWrite::Height(p) => document.write_extrude_height(p)?,
    }
    // A solve is asked for only when the edited sketch still has something to
    // solve. Taking the last constraint off a circle leaves a drawing with no
    // relationships, and demanding a report for it would make an unconstrained
    // sketch require a solver — the one thing a document written before
    // constraints existed must never start doing.
    let constraints = match &prepared {
        CopyWrite::Constraints(p) => match &p.object().payload {
            ObjectPayload::Sketch(sketch) if !sketch.constraints.is_empty() => Some(SolveCheck {
                sketch: selected.id,
                height_mm: p.height_mm,
                roles: p.circle_roles(),
            }),
            _ => None,
        },
        _ => None,
    };
    // Every name the edit *added* has to resolve as well. The baseline
    // comparison below can only speak about names that already existed, so a
    // feature that published geometry nothing could point at would pass it.
    let minted: BTreeSet<StableEntityId> = match &prepared {
        CopyWrite::Height(p) => p.added_references().iter().map(|r| r.id).collect(),
        CopyWrite::Cut(p) => p.references().iter().map(|r| r.id).collect(),
        CopyWrite::CutParameters(p) => p.added_references().iter().map(|r| r.id).collect(),
        _ => BTreeSet::new(),
    };
    let required: BTreeSet<StableEntityId> = baseline.union(&minted).copied().collect();
    let (_, solve) = checked_rebuild(
        &document,
        kernel,
        &phase(context, 0.4, 0.9),
        Some(&required),
        constraints,
    )?;
    let completed = complete(&prepared, solve)?;
    document.close()?;
    context.progress().report(0.95);
    context.check_cancelled()?;
    // Re-open the path, not the old SQLite handle: replacement by another
    // document while the worker was running is stale too.
    let current = Document::open_read_only(source_path)?;
    require_version(&current, expected)?;
    refuse_source_as_destination(
        source_path,
        destination,
        "source and output must be different files",
    )?;
    context.check_cancelled()?;
    temporary.publish(
        destination,
        Existing::Keep {
            advice: "choose a different file name",
        },
    )?;
    drop(current);
    context.progress().report(1.0);
    Ok(completed)
}

fn require_version(document: &Document, expected: DocumentVersion) -> Result<()> {
    if document.meta().document_id != expected.document_id
        || document.content_version()? != expected.content
    {
        return Err(CadError::input(
            "source has changed since it was read; reopen the document and confirm the edit again",
        ));
    }
    Ok(())
}

/// What the solved drawing of one changed sketch has to be.
///
/// The saved roles travel with it because they are a fact about the document
/// before the solve, and the one question the solved geometry cannot answer
/// about itself: a pair that swapped sizes is a perfectly good annulus, and
/// only the saved roles say it is not the one that was being edited.
#[derive(Debug, Clone, Copy)]
struct SolveCheck {
    sketch: ObjectId,
    height_mm: f64,
    roles: Option<(StableEntityId, StableEntityId)>,
}

fn checked_rebuild<K: GeometryKernel + ?Sized>(
    document: &Document,
    kernel: &mut K,
    context: &OperationContext,
    baseline: Option<&BTreeSet<StableEntityId>>,
    constraints: Option<SolveCheck>,
) -> Result<(
    BTreeSet<StableEntityId>,
    Option<ferritecad_eval::SketchSolveReport>,
)> {
    let built = rebuild_cold(document, kernel, context)?;
    let result = (|| {
        let mut resolved = BTreeSet::new();
        for reference in document.topology_refs()? {
            match built.resolve(&reference) {
                Ok(found) if !found.is_empty() => {
                    resolved.insert(reference.id);
                }
                Err(error) if baseline.is_some_and(|set| set.contains(&reference.id)) => {
                    return Err(error);
                }
                _ if baseline.is_some_and(|set| set.contains(&reference.id)) => {
                    return Err(CadError::topology(format!(
                        "edit lost reference {}",
                        reference.id
                    )));
                }
                _ => {}
            }
        }
        let solve = if let Some(SolveCheck {
            sketch: id,
            height_mm: height,
            roles,
        }) = constraints
        {
            let report = built.solve_report(id).ok_or_else(|| {
                CadError::constraint("changed constrained Sketch produced no solve report")
            })?;
            let picture = built.sketch_presentation(id).ok_or_else(|| {
                CadError::constraint("changed constrained Sketch produced no presentation")
            })?;
            // What the solved drawing has to be is a question about its
            // geometry, so it is asked of the geometry. A Line profile has to
            // still close and still be a polygon this build would publish; one
            // analytic circle has no joints to lose and is judged by the circle
            // policy instead. Neither check is a second copy of the numeric
            // rules — both call the same policy the creation route calls.
            let mut starts = Vec::new();
            let mut ends = Vec::new();
            let mut circles = Vec::new();
            for curve in picture.curves() {
                match *curve.geometry() {
                    ferritecad_document::SketchGeometry::Line { start, end } => {
                        starts.push([start.x, start.y]);
                        ends.push([end.x, end.y]);
                    }
                    ferritecad_document::SketchGeometry::Circle { center, radius } => {
                        circles.push((curve.id(), [center.x, center.y], radius));
                    }
                    _ => {
                        return Err(CadError::unsupported(
                            "constraint edit solved a curve that is neither a Line nor a Circle",
                        ));
                    }
                }
            }
            if starts.is_empty() == circles.is_empty() {
                return Err(CadError::unsupported(
                    "a constrained profile is Lines or one Circle, and this solved to neither",
                ));
            }
            if circles.is_empty() {
                for (i, end) in ends.iter().enumerate() {
                    let next = starts[(i + 1) % starts.len()];
                    if (end[0] - next[0]).abs()
                        > ferritecad_document::PolygonExtrusion::TOLERANCE_MM
                        || (end[1] - next[1]).abs()
                            > ferritecad_document::PolygonExtrusion::TOLERANCE_MM
                    {
                        return Err(CadError::constraint(
                            "solved constraint polygon lost an adjacent joint",
                        ));
                    }
                }
                ferritecad_document::PolygonExtrusion::new(starts, height)?;
            } else if let Some((outer_id, inner_id)) = roles {
                // Two circles, and which is which was decided from the saved
                // radii before the solve. Each is found by its own UUID: taking
                // them in list order, or re-deciding the roles from the solved
                // radii, would accept a pair that had swapped sizes as though
                // it had always been the other way round.
                let solved = |wanted| {
                    circles
                        .iter()
                        .find(|(id, ..)| *id == wanted)
                        .map(|(_, center, radius)| (*center, *radius))
                        .ok_or_else(|| {
                            CadError::constraint(format!(
                                "the solved drawing has no circle {wanted}, which this edit named"
                            ))
                        })
                };
                if circles.len() != 2 {
                    return Err(CadError::unsupported(
                        "an annular profile is exactly two analytic Circles",
                    ));
                }
                let (outer_center, outer_radius) = solved(outer_id)?;
                let (inner_center, inner_radius) = solved(inner_id)?;
                // Concentricity is checked on the solved centres, because it is
                // what the part needs and not every accepted request states it.
                if !ferritecad_document::AnnularExtrusion::concentric(
                    ferritecad_document::Point2::new(outer_center[0], outer_center[1])?,
                    ferritecad_document::Point2::new(inner_center[0], inner_center[1])?,
                ) {
                    return Err(CadError::constraint(
                        "the solved circles are about different centres, and an off-centre hole \
                         needs more than this slice builds",
                    ));
                }
                // The saved roles are passed in that order, so a solve that put
                // the bore outside its boundary is refused by the one numeric
                // policy rather than by a second opinion here.
                ferritecad_document::AnnularExtrusion::new(
                    outer_center,
                    outer_radius,
                    inner_radius,
                    height,
                )?;
            } else {
                let [(_, center, radius)] = circles.as_slice() else {
                    return Err(CadError::unsupported(
                        "constraint edit supports one analytic Circle per profile",
                    ));
                };
                // The solved numbers, not the saved guess: a solve that moved
                // the circle outside what this build will publish has to be
                // refused here rather than at the kernel.
                ferritecad_document::CircleExtrusion::new(*center, *radius, height)?;
            }
            Some(report.clone())
        } else {
            None
        };
        Ok((resolved, solve))
    })();
    built.release_all(kernel);
    result
}

fn phase(context: &OperationContext, start: f64, end: f64) -> OperationContext {
    let progress = context.progress().clone();
    context
        .clone()
        .with_progress(ProgressSink::new(move |fraction| {
            progress.report(start + (end - start) * fraction)
        }))
}

/// Public reading used by the CLI before submitting the same request as UI.
pub fn read_extrude_source(path: &Path) -> Result<ferritecad_document::ExtrudeEditSource> {
    let document = Document::open_read_only(path)?;
    let source = ferritecad_document::ExtrudeEditSource::read(&document)?;
    document.close()?;
    Ok(source)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::{CreateDocumentRequest, NewDocument, PlateSize, create_document};
    use ferritecad_document::{
        Dependency, DependencyRole, EndCondition, Expression, ExtrudeEditSource, ObjectRecord,
    };
    use ferritecad_kernel::{mock::MockKernel, *};

    fn fixture() -> (tempfile::TempDir, EditExtrudeRequest) {
        let root = tempfile::tempdir().expect("directory");
        let source = root.path().join("source.fcad");
        create_document(
            CreateDocumentRequest::new(
                &source,
                NewDocument::SamplePlate(PlateSize {
                    width: 83.0,
                    depth: 47.0,
                    height: 13.0,
                }),
                "keep",
            ),
            &OperationContext::default(),
        )
        .expect("source");
        let reading = read_extrude_source(&source).expect("reading");
        let request = EditExtrudeRequest {
            source,
            expected: reading.version,
            feature: reading.features[0].feature,
            distance_mm: 27.0,
            destination: root.path().join("output.fcad"),
        };
        (root, request)
    }
    fn entries(root: &Path) -> Vec<PathBuf> {
        let mut out: Vec<_> = std::fs::read_dir(root)
            .expect("directory")
            .map(|e| e.expect("entry").path())
            .collect();
        out.sort();
        out
    }
    fn replace(request: &mut EditExtrudeRequest, change: impl FnOnce(&mut ObjectRecord)) {
        let mut doc = Document::open(&request.source).expect("source");
        let mut object = doc
            .object(request.feature)
            .expect("object")
            .expect("selected");
        change(&mut object);
        doc.write(|w| {
            w.put_object(
                object.id,
                object.parent,
                object.ordinal,
                object.name.as_deref(),
                &object.payload,
            )
        })
        .expect("change fixture");
        doc.close().expect("close");
        request.expected = read_extrude_source(&request.source)
            .expect("reading")
            .version;
    }
    fn refused(
        request: &EditExtrudeRequest,
        context: &OperationContext,
        kind: ferritecad_types::ErrorKind,
    ) {
        let before = std::fs::read(&request.source).expect("bytes");
        let files = entries(request.source.parent().expect("parent"));
        let mut kernel = MockKernel::new();
        let error = edit_extrude_copy(request, &mut kernel, context).expect_err("refused");
        assert_eq!(error.kind(), kind, "{error}");
        assert_eq!(kernel.live_shape_count(), 0);
        assert_eq!(before, std::fs::read(&request.source).expect("bytes"));
        assert_eq!(files, entries(request.source.parent().expect("parent")));
    }

    #[test]
    fn selected_existing_feature_changes_without_replacing_any_identity_or_field() {
        let (_root, mut request) = fixture();
        replace(&mut request, |object| {
            object.name = Some("Chosen extrusion".into());
            object.ordinal = 19;
            if let ObjectPayload::Extrude(e) = &mut object.payload {
                e.reversed = true;
            }
        });
        let mut source = Document::open(&request.source).expect("source");
        let first = source
            .object(request.feature)
            .expect("object")
            .expect("first");
        let second = ObjectId::new();
        let mut other = first.payload.clone();
        if let ObjectPayload::Extrude(e) = &mut other {
            e.reversed = false;
        }
        source
            .write(|w| {
                w.put_object(second, first.parent, -1, Some("Earlier extrusion"), &other)?;
                let ObjectPayload::Extrude(e) = &other else {
                    unreachable!()
                };
                w.add_dependency(Dependency {
                    dependent: second,
                    dependency: e.profile,
                    role: DependencyRole::Profile,
                })
            })
            .expect("second");
        source.close().expect("close");
        request.expected = read_extrude_source(&request.source)
            .expect("reading")
            .version;
        let bytes = std::fs::read(&request.source).expect("before");
        let mut kernel = MockKernel::new();
        edit_extrude_copy(&request, &mut kernel, &OperationContext::default()).expect("edit");
        assert_eq!(kernel.live_shape_count(), 0);
        let source = Document::open_read_only(&request.source).expect("source");
        let output = Document::open_read_only(&request.destination).expect("output");
        let mut expected = source.objects().expect("objects");
        let actual = output.objects().expect("objects");
        assert_eq!(source.meta().document_id, output.meta().document_id);
        assert_eq!(
            source.dependencies().expect("deps"),
            output.dependencies().expect("deps")
        );
        assert_eq!(
            source.topology_refs().expect("refs"),
            output.topology_refs().expect("refs")
        );
        for (old, new) in expected.iter_mut().zip(&actual) {
            if old.id == request.feature {
                assert_eq!(old.id, new.id);
                assert_eq!(old.name, new.name);
                assert_eq!(old.parent, new.parent);
                assert_eq!(old.ordinal, new.ordinal);
                if let ObjectPayload::Extrude(e) = &mut old.payload {
                    e.end_condition = EndCondition::Blind {
                        distance: Expression::constant(27.0).expect("literal"),
                    };
                }
                assert_eq!(old.payload, new.payload);
            } else {
                assert_eq!(old, new, "another object changed");
            }
        }
        assert_eq!(bytes, std::fs::read(&request.source).expect("source bytes"));
    }

    #[test]
    fn invalid_inputs_and_unsupported_features_leave_all_files_unchanged() {
        use ferritecad_types::ErrorKind::{Input, Unsupported};
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -3.0] {
            let (_root, mut request) = fixture();
            request.distance_mm = value;
            refused(&request, &OperationContext::default(), Input);
        }
        let (_root, mut request) = fixture();
        let selected = request.feature;
        request.feature = ObjectId::new();
        refused(&request, &OperationContext::default(), Input);
        let doc = Document::open_read_only(&request.source).expect("document");
        request.feature = doc
            .objects()
            .expect("objects")
            .iter()
            .find(|o| !matches!(o.payload, ObjectPayload::Extrude(_)))
            .expect("other")
            .id;
        doc.close().expect("close");
        refused(&request, &OperationContext::default(), Unsupported);
        request.feature = selected;
        for end in [
            EndCondition::Symmetric {
                distance: Expression::constant(13.0).expect("literal"),
            },
            EndCondition::ThroughAll,
            EndCondition::Blind {
                distance: Expression::new("height * 2", 13.0).expect("expression"),
            },
            EndCondition::Blind {
                distance: Expression::new("12", 13.0).expect("inconsistent literal"),
            },
        ] {
            replace(&mut request, |o| {
                if let ObjectPayload::Extrude(e) = &mut o.payload {
                    e.end_condition = end;
                }
            });
            refused(&request, &OperationContext::default(), Unsupported);
        }
    }

    #[test]
    fn stale_source_and_aliases_are_refused() {
        let (root, mut request) = fixture();
        let expected = request.expected;
        replace(&mut request, |o| o.name = Some("Renamed".into()));
        request.expected = expected;
        refused(
            &request,
            &OperationContext::default(),
            ferritecad_types::ErrorKind::Input,
        );
        request.expected = read_extrude_source(&request.source)
            .expect("version")
            .version;
        for path in [request.source.clone(), root.path().join("hard.fcad")] {
            if path != request.source {
                std::fs::hard_link(&request.source, &path).expect("hard link");
            }
            request.destination = path;
            assert!(
                crate::is_same_entry(&request.source, &request.destination).expect("same identity")
            );
            refused(
                &request,
                &OperationContext::default(),
                ferritecad_types::ErrorKind::Input,
            );
        }
        #[cfg(unix)]
        {
            request.destination = root.path().join("symbolic.fcad");
            std::os::unix::fs::symlink(&request.source, &request.destination).expect("symlink");
            refused(
                &request,
                &OperationContext::default(),
                ferritecad_types::ErrorKind::Input,
            );
        }
    }

    #[test]
    fn cancellation_racing_destination_and_late_source_change_have_atomic_outcomes() {
        for event in ["cancel", "late cancel", "occupied", "source changed"] {
            let (root, request) = fixture();
            let before = std::fs::read(&request.source).expect("bytes");
            let cancel = CancelToken::new();
            let stop = cancel.clone();
            let dest = request.destination.clone();
            let source = request.source.clone();
            let context = OperationContext::default()
                .with_cancel(cancel)
                .with_progress(ProgressSink::new(move |fraction| {
                    if fraction == 0.95 {
                        match event {
                            "cancel" => stop.cancel(),
                            "occupied" => std::fs::write(&dest, b"racing file").expect("racer"),
                            "source changed" => {
                                let mut doc = Document::open(&source).expect("open changed source");
                                let object = doc.objects().expect("objects").remove(0);
                                doc.write(|w| {
                                    w.put_object(
                                        object.id,
                                        object.parent,
                                        object.ordinal,
                                        Some("changed at barrier"),
                                        &object.payload,
                                    )
                                })
                                .expect("change version");
                                doc.close().expect("close");
                            }
                            _ => {}
                        }
                    }
                    if fraction == 1.0 && event == "late cancel" {
                        stop.cancel();
                    }
                }));
            let result = edit_extrude_copy(&request, &mut MockKernel::new(), &context);
            assert_eq!(
                result.is_ok(),
                event == "late cancel",
                "{event}: {result:?}"
            );
            if event != "source changed" {
                assert_eq!(
                    before,
                    std::fs::read(&request.source).expect("unchanged source")
                );
            }
            if event == "occupied" {
                assert_eq!(
                    std::fs::read(&request.destination).expect("racer"),
                    b"racing file"
                );
            }
            assert_eq!(
                request.destination.exists(),
                matches!(event, "late cancel" | "occupied")
            );
            assert!(
                entries(root.path())
                    .iter()
                    .all(|p| p == &request.source || p == &request.destination),
                "scratch leaked"
            );
        }
    }

    #[derive(Debug)]
    struct Refusing {
        inner: MockKernel,
        lose_ref: bool,
        sketch_fault: bool,
    }
    impl GeometryKernel for Refusing {
        /// Delegated: this double is about something else, and a cut it
        /// answered differently would be a second kernel.
        fn cut(
            &mut self,
            request: &ferritecad_kernel::CutRequest,
            track: &[ferritecad_kernel::SubShapeHandle],
            context: &OperationContext,
        ) -> Result<ferritecad_kernel::CutResult> {
            self.inner.cut(request, track, context)
        }

        fn identity(&self) -> &KernelIdentity {
            self.inner.identity()
        }
        fn extrude(&mut self, r: &ExtrudeRequest, c: &OperationContext) -> Result<ExtrudeResult> {
            let refuse = r.extent().total_length() == 27.0
                || (self.sketch_fault && self.inner.extrude_count() == 1);
            if refuse && !self.lose_ref {
                return Err(CadError::kernel("deterministic edited geometry refusal"));
            }
            let mut result = self.inner.extrude(r, c)?;
            if refuse {
                result.end_cap.clear();
            }
            Ok(result)
        }
        fn transform(
            &mut self,
            s: ShapeHandle,
            t: &ferritecad_types::Transform,
            c: &OperationContext,
        ) -> Result<OperationResult> {
            self.inner.transform(s, t, c)
        }
        fn tessellate(
            &mut self,
            s: ShapeHandle,
            p: &TessellationParams,
            c: &OperationContext,
        ) -> Result<Mesh> {
            self.inner.tessellate(s, p, c)
        }
        fn encode_shape_with(
            &mut self,
            s: ShapeHandle,
            e: &[SubShapeHandle],
        ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
            self.inner.encode_shape_with(s, e)
        }
        fn decode_shape_with(
            &mut self,
            b: &BrepBlob,
            a: &[ArchiveSlot],
        ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
            self.inner.decode_shape_with(b, a)
        }
        fn encode_shape(&mut self, s: ShapeHandle) -> Result<BrepBlob> {
            self.inner.encode_shape(s)
        }
        fn decode_shape(&mut self, b: &BrepBlob) -> Result<ShapeHandle> {
            self.inner.decode_shape(b)
        }
        fn release(&mut self, s: ShapeHandle) {
            self.inner.release(s);
        }
    }

    #[test]
    fn edited_geometry_failure_or_loss_of_a_resolved_reference_never_publishes() {
        for lose_ref in [false, true] {
            let (root, request) = fixture();
            let before = std::fs::read(&request.source).expect("source");
            let mut kernel = Refusing {
                inner: MockKernel::new(),
                lose_ref,
                sketch_fault: false,
            };
            let error = edit_extrude_copy(&request, &mut kernel, &OperationContext::default())
                .expect_err("refused edited geometry");
            assert_eq!(
                error.kind(),
                if lose_ref {
                    ferritecad_types::ErrorKind::Topology
                } else {
                    ferritecad_types::ErrorKind::Kernel
                }
            );
            assert_eq!(kernel.inner.live_shape_count(), 0);
            assert_eq!(entries(root.path()), vec![request.source.clone()]);
            assert_eq!(std::fs::read(&request.source).expect("unchanged"), before);
        }
    }

    #[test]
    fn no_native_feature_has_an_explanation() {
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("empty.fcad");
        let document = Document::create(path).expect("empty");
        let reading = ExtrudeEditSource::read(&document).expect("reading");
        assert!(reading.features.is_empty());
        assert!(reading.unavailable_reason().is_some());
    }
    #[test]
    fn parameter_dependencies_and_unknown_capabilities_are_not_silently_rewritten() {
        for future in [false, true] {
            let (_root, mut request) = fixture();
            let mut document = Document::open(&request.source).expect("source");
            document
                .write(|w| {
                    let id = ObjectId::new();
                    if future {
                        let bytes = ferritecad_document::Envelope::new(
                            "future.feature",
                            1,
                            vec!["future.required".into()],
                            vec![0x01],
                        )
                        .to_bytes()?;
                        w.put_object(
                            id,
                            None,
                            50,
                            Some("Future"),
                            &ObjectPayload::from_storage_bytes(&bytes)?,
                        )?;
                    } else {
                        w.put_object(
                            id,
                            None,
                            50,
                            Some("Height"),
                            &ObjectPayload::Parameter(ferritecad_document::Parameter {
                                name: "height".into(),
                                dimension: ferritecad_types::Dimension::Length,
                                expression: Expression::constant(13.0)?,
                            }),
                        )?;
                        w.add_dependency(Dependency {
                            dependent: request.feature,
                            dependency: id,
                            role: DependencyRole::Parameter,
                        })?;
                    }
                    Ok(())
                })
                .expect("fixture");
            document.close().expect("close");
            request.expected = read_extrude_source(&request.source)
                .expect("version")
                .version;
            refused(
                &request,
                &OperationContext::default(),
                ferritecad_types::ErrorKind::Unsupported,
            );
        }
    }

    #[test]
    fn cancellation_cleans_only_operation_owned_sqlite_sidecars() {
        let (root, request) = fixture();
        let foreign = request.source.with_extension("fcad-cache");
        std::fs::write(&foreign, b"foreign cache").expect("sentinel");
        let before = std::fs::read(&request.source).expect("before");
        let directory = root.path().to_path_buf();
        let cancel = CancelToken::new();
        let stop = cancel.clone();
        let context = OperationContext::default()
            .with_cancel(cancel)
            .with_progress(ProgressSink::new(move |fraction| {
                if fraction == 0.95 {
                    let scratch = entries(&directory)
                        .into_iter()
                        .find(|p| {
                            p.file_name()
                                .expect("name")
                                .to_string_lossy()
                                .starts_with(".ferritecad-")
                        })
                        .expect("owned scratch");
                    for suffix in ["-wal", "-shm", "-journal"] {
                        std::fs::write(
                            scratch.join(format!("payload{suffix}")),
                            b"operation sidecar",
                        )
                        .expect("sidecar");
                    }
                    stop.cancel();
                }
            }));
        assert!(matches!(
            edit_extrude_copy(&request, &mut MockKernel::new(), &context),
            Err(CadError::Cancelled)
        ));
        let mut expected = vec![request.source.clone(), foreign.clone()];
        expected.sort();
        assert_eq!(entries(root.path()), expected);
        assert_eq!(std::fs::read(&request.source).expect("source"), before);
        assert_eq!(std::fs::read(foreign).expect("foreign"), b"foreign cache");
    }
    fn sketch_fixture() -> (tempfile::TempDir, EditSketchRequest) {
        let (root, old) = fixture();
        let reading = read_extrude_source(&old.source).expect("catalog");
        let choice = &reading.sketches[0];
        let mut vertices = choice.vertices.clone().expect("supported");
        vertices[1].start_mm[0] = 100.;
        vertices[2].start_mm[0] = 100.;
        (
            root,
            EditSketchRequest {
                source: old.source,
                expected: reading.version,
                sketch: choice.sketch,
                vertices,
                destination: old.destination,
            },
        )
    }

    #[test]
    fn sketch_copy_identity_and_metadata_survive() {
        let (root, request) = sketch_fixture();
        let bytes = std::fs::read(&request.source).expect("bytes");
        let mtime = std::fs::metadata(&request.source)
            .expect("metadata")
            .modified()
            .expect("mtime");
        let source = Document::open_read_only(&request.source).expect("source");
        let expected = ferritecad_document::replace_sketch_coordinates(
            &source,
            request.sketch,
            &request.vertices,
        )
        .expect("prepared");
        let mut kernel = MockKernel::new();
        let saved = edit_sketch_copy(&request, &mut kernel, &OperationContext::default())
            .expect("published");
        assert_eq!(
            kernel.live_shape_count(),
            0,
            "released before kernel destructor"
        );
        assert_eq!(kernel.extrude_count(), 2, "two cold checks");
        assert_eq!(saved.document_id, source.meta().document_id);
        let copy = Document::open_read_only(&saved.destination).expect("copy");
        assert_eq!(
            copy.meta(),
            source.meta(),
            "all metadata including modified_at"
        );
        assert_eq!(
            copy.dependencies().expect("deps"),
            source.dependencies().expect("deps")
        );
        assert_eq!(
            copy.topology_refs().expect("refs"),
            source.topology_refs().expect("refs")
        );
        for o in source.objects().expect("objects") {
            let actual = copy.object(o.id).expect("object").expect("same ID");
            if o.id == request.sketch {
                assert_eq!(actual.payload, expected.payload);
                assert_eq!(actual.id, expected.id);
                assert_eq!(actual.name, expected.name);
                assert_eq!(actual.ordinal, expected.ordinal);
                assert_eq!(actual.parent, expected.parent);
                let (ObjectPayload::Sketch(old), ObjectPayload::Sketch(new)) =
                    (o.payload, actual.payload)
                else {
                    panic!("sketch")
                };
                assert_eq!(
                    old.curves.iter().map(|c| c.id).collect::<Vec<_>>(),
                    new.curves.iter().map(|c| c.id).collect::<Vec<_>>()
                );
            } else {
                assert_eq!(actual, o);
            }
        }
        copy.close().expect("close");
        source.close().expect("close");
        assert_eq!(std::fs::read(&request.source).expect("source"), bytes);
        assert_eq!(
            std::fs::metadata(&request.source)
                .expect("metadata")
                .modified()
                .expect("mtime"),
            mtime
        );
        assert_eq!(entries(root.path()).len(), 2);
    }

    #[test]
    fn sketch_copy_refusals_cancellation_races_and_cleanup() {
        copy_faults(false);
    }

    #[test]
    fn constraint_copy_refusals_cancellation_races_and_cleanup() {
        let (_root, old) = sketch_fixture();
        let request = constraint_request(&old);
        let probe = edit_sketch_constraints_copy(
            &request,
            &mut MockKernel::new(),
            &OperationContext::default(),
        );
        if probe
            .as_ref()
            .is_err_and(|e| e.kind() == ferritecad_types::ErrorKind::Unsupported)
        {
            assert_ne!(
                std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
                Ok("1")
            );
            eprintln!("skipped: no PlaneGCS for constraint phase faults");
            return;
        }
        probe.expect("constraint solve with test kernel");
        copy_faults(true);
    }
    fn constraint_request(old: &EditSketchRequest) -> EditSketchConstraintsRequest {
        EditSketchConstraintsRequest {
            source: old.source.clone(),
            expected: old.expected,
            sketch: old.sketch,
            destination: old.destination.clone(),
            edits: ferritecad_document::SketchConstraintEdits {
                remove: vec![],
                add: vec![
                    ferritecad_document::AddSketchConstraint::Line(
                        ferritecad_document::AddLineConstraint::Line {
                            curve: old.vertices[0].curve_id,
                            kind: ferritecad_document::LineConstraintKind::Horizontal,
                        },
                    ),
                    ferritecad_document::AddSketchConstraint::Line(
                        ferritecad_document::AddLineConstraint::Line {
                            curve: old.vertices[0].curve_id,
                            kind: ferritecad_document::LineConstraintKind::Distance(
                                ferritecad_document::LineLengthMm::new(60.).expect("length"),
                            ),
                        },
                    ),
                ],
            },
        }
    }
    fn copy_faults(constraints: bool) {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        for case in [
            "before",
            "snapshot",
            "rebuilt",
            "closed",
            "late",
            "occupied",
            "changed",
            "alias",
            "publish",
            "geometry",
            "references",
        ] {
            let (root, request) = sketch_fixture();
            let bytes = std::fs::read(&request.source).expect("source");
            let mtime = std::fs::metadata(&request.source)
                .expect("metadata")
                .modified()
                .expect("mtime");
            let directory = root.path().to_path_buf();
            let cancel = CancelToken::new();
            if case == "before" {
                cancel.cancel();
            }
            let token = cancel.clone();
            let source = request.source.clone();
            let dest = request.destination.clone();
            let once = Arc::new(AtomicBool::new(false));
            let context = OperationContext::default()
                .with_cancel(cancel)
                .with_progress(ProgressSink::new(move |f| {
                    let threshold = match case {
                        "snapshot" => 0.1,
                        "rebuilt" => 0.4,
                        "late" => 1.,
                        _ => 0.95,
                    };
                    if f == threshold && !once.swap(true, Ordering::SeqCst) {
                        match case {
                            "snapshot" | "rebuilt" | "closed" | "late" => token.cancel(),
                            "occupied" => {
                                std::fs::write(&dest, b"raced destination").expect("race")
                            }
                            "changed" => {
                                let mut d = Document::open(&source).expect("source writer");
                                let mut o = d.objects().expect("objects").remove(0);
                                o.name = Some("changed during edit".into());
                                d.write(|w| {
                                    w.put_object(
                                        o.id,
                                        o.parent,
                                        o.ordinal,
                                        o.name.as_deref(),
                                        &o.payload,
                                    )
                                })
                                .expect("change");
                                d.close().expect("close");
                            }
                            "alias" => std::fs::hard_link(&source, &dest).expect("late alias"),
                            "publish" => {
                                let scratch = entries(&directory)
                                    .into_iter()
                                    .find(|p| {
                                        p.file_name()
                                            .expect("name")
                                            .to_string_lossy()
                                            .starts_with(".ferritecad-")
                                    })
                                    .expect("owned scratch")
                                    .join("payload");
                                std::fs::remove_file(scratch)
                                    .expect("inject missing scratch payload");
                            }
                            _ => {}
                        }
                    }
                }));
            let mut kernel = Refusing {
                inner: MockKernel::new(),
                lose_ref: case == "references",
                sketch_fault: matches!(case, "geometry" | "references"),
            };
            // Reuse the fault seam after a successful baseline rebuild: the
            // second extrusion, with the changed profile, fails or loses a ref.
            let result = if constraints {
                edit_sketch_constraints_copy(&constraint_request(&request), &mut kernel, &context)
                    .map(|_| ())
            } else {
                edit_sketch_copy(&request, &mut kernel, &context).map(|_| ())
            };
            if case == "late" {
                assert!(result.is_ok(), "{case}: {result:?}");
            } else {
                let error = result.expect_err(case);
                let expected = match case {
                    "before" | "snapshot" | "rebuilt" | "closed" => {
                        ferritecad_types::ErrorKind::Cancellation
                    }
                    "geometry" => ferritecad_types::ErrorKind::Kernel,
                    "references" => ferritecad_types::ErrorKind::Topology,
                    "publish" => ferritecad_types::ErrorKind::Io,
                    _ => ferritecad_types::ErrorKind::Input,
                };
                assert_eq!(error.kind(), expected, "{case}: {error}");
            }
            assert_eq!(kernel.inner.live_shape_count(), 0, "{case} leaked handles");
            if case != "changed" {
                assert_eq!(
                    std::fs::read(&request.source).expect("source"),
                    bytes,
                    "{case}"
                );
                assert_eq!(
                    std::fs::metadata(&request.source)
                        .expect("metadata")
                        .modified()
                        .expect("mtime"),
                    mtime,
                    "{case}"
                );
            }
            if case == "occupied" {
                assert_eq!(
                    std::fs::read(&request.destination).expect("destination"),
                    b"raced destination"
                );
            }
            let mut expected_entries = vec![request.source.clone()];
            if matches!(case, "late" | "occupied" | "alias") {
                expected_entries.push(request.destination.clone());
            }
            expected_entries.sort();
            assert_eq!(
                entries(root.path()),
                expected_entries,
                "{case}: scratch or sidecars remained"
            );
            if case == "alias" {
                assert_eq!(std::fs::read(&request.destination).expect("alias"), bytes);
            }
        }
    }
}
