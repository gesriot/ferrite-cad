// SPDX-License-Identifier: MIT
//! Turning a stored document into the scene a writer is handed.
//!
//! The same reading a picture is built from — [`prepare::load`] — with a
//! different thing kept from it. A picture flattens: it multiplies every
//! placement out, packs one mesh per drawn definition, and throws away the
//! assembly frames, the source-local keys and the parents, because none of
//! those put a pixel on screen. An interchange file needs exactly what the
//! picture threw away, so it is built from the reading rather than from the
//! picture.
//!
//! # This is not a writer
//!
//! Nothing here knows about a file format. Geometry stays in FerriteCAD
//! millimetres and FerriteCAD axes, colours stay linear, and the hierarchy
//! stays as the source recorded it. Whatever conversion a particular format
//! wants happens in that format's writer, once, where it can be measured.

use std::collections::BTreeMap;
use std::path::Path;

use ferritecad_exchange::{ColourSource, Import};
use ferritecad_export::{
    ExportColourOrigin, ExportDefinitionId, ExportGeometry, ExportMaterial, ExportMesh,
    ExportNodeId, ExportOccurrence, ExportOmission, ExportProvenance, ExportScene,
    ExportSceneBuilder, ExportSource, ExportTransform,
};
use ferritecad_kernel::{
    GeometryKernel, Mesh, OperationContext, TessellationParams, TessellationRefusal,
};
use ferritecad_types::{CadError, Result};

use crate::prepare::{self, LoadSink, NodeIdentity, PreparedDefinition, PreparedNode};
use crate::{BODY_COLOUR, SceneItem};

/// Reads a document and describes it for an interchange writer.
///
/// Native bodies and imported scenes, in document order, with the assembly
/// structure and the exact parent-local placements the sources recorded. One
/// read-only open, one cold rebuild, one reading of each stored STEP source,
/// one tessellation per definition however many nodes reference it, and every
/// shape released on success, on failure and on cancellation.
///
/// `read_step` is how this asks the kernel to read a stored STEP file again.
/// The bytes come from the document; no external file is opened, so a document
/// exports the same scene years after the file it was imported from is gone.
///
/// Refuses rather than repairs. A placement no static-mesh hierarchy can
/// express, a definition whose geometry would be written twice, and a
/// tessellation failure the document never recorded all stop the export before
/// anything is written.
pub fn export_scene<K>(
    path: &Path,
    kernel: &mut K,
    read_step: impl FnMut(&mut K, &[u8]) -> Result<Import>,
    params: &TessellationParams,
    context: &OperationContext,
) -> Result<ExportScene>
where
    K: GeometryKernel + ?Sized,
{
    prepare::load(path, kernel, read_step, params, context, Export::default())
}

/// The exportable half of one tessellation.
///
/// Held apart from [`ExportMesh`] because a mesh cannot exist without its
/// material slots, and which slots a definition has is settled by the
/// placements, which arrive after the geometry does.
#[derive(Debug)]
struct Triangles {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
}

impl Triangles {
    /// Everything a writer needs out of a kernel mesh, and nothing that only
    /// meant something inside the session that produced it.
    ///
    /// The face, edge and corner partitions are deliberately dropped: each is
    /// expressed with a handle that names something in one kernel session, and
    /// a file recording those numbers would record identities that no longer
    /// exist by the time anybody reads it.
    fn of(mesh: &Mesh) -> Result<Self> {
        mesh.validate()?;
        Ok(Self {
            positions: mesh
                .positions
                .chunks_exact(3)
                .map(|value| [value[0], value[1], value[2]])
                .collect(),
            normals: mesh
                .normals
                .chunks_exact(3)
                .map(|value| [value[0], value[1], value[2]])
                .collect(),
            triangles: mesh
                .indices
                .chunks_exact(3)
                .map(|value| [value[0], value[1], value[2]])
                .collect(),
        })
    }
}

/// What an export keeps from one reading of a document.
#[derive(Debug, Default)]
struct Export {
    /// The geometry of each prepared definition that has any, by its index.
    triangles: BTreeMap<usize, Triangles>,
    /// Why each prepared definition that has none has none.
    omissions: BTreeMap<usize, TessellationRefusal>,
    /// Every placement, in the order the load reported it.
    nodes: Vec<PreparedNode>,
}

impl LoadSink for Export {
    type Output = ExportScene;

    fn opened(
        &mut self,
        _document: &ferritecad_document::Document,
        _objects: &[ferritecad_document::ObjectRecord],
        _built: &ferritecad_eval::RebuildResult,
    ) -> Result<()> {
        // An export needs neither the durable names of faces nor what a solve
        // found out: a static mesh file can carry neither, and reading them
        // here would be work whose result is thrown away.
        Ok(())
    }

    fn definition(&mut self, definition: usize, geometry: prepare::Geometry<'_>) -> Result<()> {
        match geometry {
            prepare::Geometry::Mesh(mesh) => {
                self.triangles.insert(definition, Triangles::of(mesh)?);
            }
            prepare::Geometry::Omitted(refusal) => {
                self.omissions.insert(definition, refusal);
            }
            // Structure carries no geometry, which is a third state and not an
            // empty one. Recording nothing here is what keeps it distinct.
            prepare::Geometry::Structural => {}
        }
        Ok(())
    }

    fn node(&mut self, node: &PreparedNode) -> Result<()> {
        self.nodes.push(node.clone());
        Ok(())
    }

    fn finish(self, definitions: &[PreparedDefinition]) -> Result<ExportScene> {
        let mut builder = ExportSceneBuilder::new();
        let mut definition_ids: Vec<ExportDefinitionId> = Vec::with_capacity(definitions.len());
        for (index, prepared) in definitions.iter().enumerate() {
            let places: Vec<&PreparedNode> = self
                .nodes
                .iter()
                .filter(|node| node.definition == index)
                .collect();
            let geometry = self.geometry_of(index, prepared, &places)?;
            definition_ids.push(builder.definition(
                source_of(prepared),
                prepared.name.clone(),
                provenance_of(prepared),
                geometry,
            )?);
        }

        let mut ids: Vec<ExportNodeId> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let parent = match node.parent {
                None => None,
                Some(parent) => Some(*ids.get(parent).ok_or_else(|| {
                    CadError::topology(format!(
                        "a placement sits inside node {parent}, which the load reported after it"
                    ))
                })?),
            };
            let definition = *definition_ids.get(node.definition).ok_or_else(|| {
                CadError::topology(format!(
                    "a placement names definition {}, which this load never settled",
                    node.definition
                ))
            })?;
            // The exact local placement, checked here and repaired nowhere: a
            // scene that quietly straightened a shear would describe a model
            // nobody built.
            let local = ExportTransform::new(*node.local.rows())?;
            let colour = match node.colour_source {
                ColourSource::Instance => Some(node.colour),
                ColourSource::Definition | ColourSource::None => None,
                // A colour source this build has not been measured against is
                // not silently treated as an override.
                _ => {
                    return Err(CadError::unsupported(
                        "a placement carries a colour from somewhere this export does not know \
                         how to describe",
                    ));
                }
            };
            ids.push(builder.node(
                parent,
                definition,
                local,
                node.name.clone(),
                colour,
                occurrence_of(node.identity),
            )?);
        }

        builder.finish()
    }
}

impl Export {
    /// What one definition holds, and why.
    fn geometry_of(
        &self,
        index: usize,
        prepared: &PreparedDefinition,
        places: &[&PreparedNode],
    ) -> Result<ExportGeometry> {
        if places.is_empty() {
            return Err(CadError::topology(format!(
                "{:?} was read but nothing places it",
                prepared.item
            )));
        }
        if prepared.structural {
            // Every placement of it holds other placements, so its own shape is
            // the compound of what is inside it and belongs to the parts rather
            // than to the frame.
            return Ok(ExportGeometry::Structural);
        }
        // A definition placed both as a part and as a frame would have its
        // geometry written once for itself and again for every child. There is
        // no measured source that does this, and guessing which of the two
        // readings was meant would be inventing an answer.
        if let Some(frame) = places.iter().find(|node| node.structural) {
            return Err(CadError::unsupported(format!(
                "{:?} carries geometry and is also the frame that node {} places other things \
                 inside; exporting it would write the same solids twice",
                prepared.item, frame.definition
            )));
        }

        if let Some(omission) = &prepared.omission {
            let refusal = self.omissions.get(&index).copied().ok_or_else(|| {
                CadError::topology(format!(
                    "{:?} is recorded as an omission with no typed refusal behind it",
                    prepared.item
                ))
            })?;
            return Ok(ExportGeometry::Omitted(ExportOmission::new(
                omission.diagnostic.clone(),
                refusal,
            )));
        }

        let triangles = self.triangles.get(&index).ok_or_else(|| {
            CadError::topology(format!(
                "{:?} has neither geometry, an omission nor a structural role",
                prepared.item
            ))
        })?;
        let material = material_of(prepared, places)?;
        let slots = vec![0u32; triangles.triangles.len()];
        Ok(ExportGeometry::Mesh(ExportMesh::new(
            triangles.positions.clone(),
            triangles.normals.clone(),
            triangles.triangles.clone(),
            slots,
            vec![material],
        )?))
    }
}

/// The one material slot a definition read from a document has.
///
/// A source colour is a property of the definition and is carried as such; a
/// colour set on one placement is that placement's override and never rewrites
/// what the definition is. Where nothing said anything, the neutral colour a
/// body with no recorded appearance is drawn in, marked as the default it is.
fn material_of(prepared: &PreparedDefinition, places: &[&PreparedNode]) -> Result<ExportMaterial> {
    let name = prepared
        .name
        .clone()
        .unwrap_or_else(|| "material".to_owned());
    let mut colour: Option<[f64; 3]> = None;
    for node in places {
        if node.colour_source != ColourSource::Definition {
            continue;
        }
        match colour {
            None => colour = Some(node.colour),
            Some(known) if known == node.colour => {}
            Some(known) => {
                // Two placements both said the colour came from the definition
                // and disagreed about what it is. One definition has one
                // appearance, so this is a contradiction rather than a choice.
                return Err(CadError::topology(format!(
                    "{:?} is said to be coloured {known:?} by its definition and {:?} by it as \
                     well",
                    prepared.item, node.colour
                )));
            }
        }
    }
    match colour {
        Some(colour) => ExportMaterial::new(name, colour, ExportColourOrigin::Source),
        None => ExportMaterial::new(name, BODY_COLOUR, ExportColourOrigin::Default),
    }
}

/// The load's identity for one placement, in the neutral terms a writer sees.
///
/// A translation and nothing more. There is no arm here that invents a value,
/// because the three states of the load's identity and the three states a
/// writer is offered are the same three facts, deliberately.
fn occurrence_of(identity: NodeIdentity) -> ExportOccurrence {
    match identity {
        NodeIdentity::Object(object) => ExportOccurrence::Object(object),
        NodeIdentity::Occurrence(occurrence) => ExportOccurrence::Occurrence(occurrence),
        NodeIdentity::Unrecorded => ExportOccurrence::Unrecorded,
    }
}

fn source_of(prepared: &PreparedDefinition) -> ExportSource {
    match &prepared.item {
        SceneItem::Body(object) => ExportSource::Body { object: *object },
        SceneItem::Imported(reference) => ExportSource::Imported {
            source: reference.source(),
            definition_key: reference.definition_key().to_owned(),
        },
    }
}

fn provenance_of(prepared: &PreparedDefinition) -> ExportProvenance {
    ExportProvenance::new(
        prepared.source_file.clone(),
        prepared.source_unit.clone(),
        prepared.schema.clone(),
        prepared.solids,
    )
}
