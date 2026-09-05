// SPDX-License-Identifier: MIT
//! What a scene looks like to a writer, and nothing else.
//!
//! An [`ExportScene`] is the one thing an interchange writer is handed. It is
//! immutable, read-only and kernel-neutral: no document connection, no kernel
//! reference, no filesystem path, no GPU buffer, no camera, no picking
//! identity, and no session-bound handle. A writer that could reach any of
//! those could produce a different file from the same scene, and then the
//! bytes would no longer be a function of what was exported.
//!
//! # Why not the kernel's mesh
//!
//! [`ferritecad_kernel::Mesh`] partitions its triangles by face, its segments
//! by edge and its positions by topological vertex, and every one of those
//! partitions is expressed with a handle that means something only inside the
//! kernel session that issued it. Carrying that into an export would carry a
//! dangling meaning: the session ends, the handles stay, and the file would
//! record numbers that identify nothing. [`ExportMesh`] therefore holds the
//! exportable geometry alone.
//!
//! # Three geometry states, not two
//!
//! A definition either has triangles, is structure that never had its own
//! geometry — an assembly frame — or is a definition whose retained topology
//! this build could not turn into triangles. Collapsing the last two would
//! make a deliberate assembly node indistinguishable from a part that went
//! missing, which is exactly the confusion a partial export must not create.
//!
//! # Millimetres
//!
//! Geometry here is in FerriteCAD millimetres and FerriteCAD axes. The
//! axis and unit conversion a particular format wants belongs to that
//! format's writer, because two writers want different ones.

use ferritecad_exchange::Diagnostic;
use ferritecad_kernel::TessellationRefusal;
use ferritecad_types::{CadError, ImportedSourceId, ObjectId, OccurrenceId, Result};

/// How far from exact a placement may be and still be called representable.
///
/// The §22B-1a measurement classified all 170 placements of the STEP corpus
/// with this tolerance and found every one of them finite, orthogonal,
/// uniformly scaled and unreflected. It is defined here, once, so the gate
/// that measures the corpus and the gate that refuses a placement cannot
/// drift apart.
pub const TRANSFORM_TOLERANCE: f64 = 1.0e-10;

/// Where a definition's identity comes from.
///
/// Two kinds, because they are durable for different reasons. A native body is
/// the object that holds it. An imported definition is the key its own file
/// gave it, which means nothing without the identity of the bytes it was read
/// from: `#31` occurs in most STEP files and names something different in each.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Closed on purpose: a definition is a body of this document or a
/// definition of an imported file, and a writer that could fall through to a
/// wildcard would be a writer that silently ignored a third kind.
pub enum ExportSource {
    /// A body of the exported document.
    Body { object: ObjectId },
    /// A definition inside one imported file.
    Imported {
        source: ImportedSourceId,
        definition_key: String,
    },
}

/// The durable identity of one placement, in terms that outlive the export.
///
/// Read-only and neutral. A writer may carry one and may compare two of them,
/// and there is nothing reachable through it: no document to reopen, no kernel
/// session, no picture and no number a file format later assigns. It is what
/// tells two placements of one definition apart when everything a source
/// records about them — the name, the key, the shape and often the transform —
/// is identical.
///
/// # Three states, not two
///
/// For the same reason [`ExportGeometry`] has three. A placement the document
/// gave an identity and a placement whose document predates placement identity
/// are different facts, and collapsing them would let a value invented at
/// export time be presented as something the document recorded. There is
/// deliberately no way to spell "make one up".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Closed on purpose: a placement is a body of this document, a recorded
/// occurrence of an imported one, or a placement no identity was ever written
/// for. A writer that could fall through to a wildcard would be a writer that
/// silently treated the third as one of the first two.
pub enum ExportOccurrence {
    /// A native body, identified by the document object that holds it.
    ///
    /// Not an [`OccurrenceId`] beside it, and not a fresh one per export: the
    /// object identifier is already durable and already unique in the document,
    /// and minting a second identity over it would be two names for one thing
    /// that nothing could later be sure were the same.
    Object(ObjectId),
    /// One placement of an imported scene, as its document recorded it.
    Occurrence(OccurrenceId),
    /// A placement from a document layout written before placements carried
    /// identities.
    ///
    /// Not lost and not missing: never recorded. Such a document still exports
    /// exactly what it always did; what it cannot do is answer a question about
    /// which placement this is, and saying so is the whole of the difference.
    Unrecorded,
}

impl ExportOccurrence {
    /// Whether this is an identity a document actually wrote down.
    ///
    /// The one thing a caller may ask without matching, because it is the one
    /// question with a stable meaning across all three states.
    pub const fn is_recorded(self) -> bool {
        !matches!(self, Self::Unrecorded)
    }
}

/// What one definition is called, and where it came from.
///
/// Display facts only. Nothing here is identity, and nothing here may be
/// matched on: two definitions may share every one of these values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExportProvenance {
    /// The file an imported definition came from, by name. Never a path to
    /// open.
    pub file_name: Option<String>,
    /// The unit the source file declared, as it declared it.
    pub source_unit: Option<String>,
    /// The schema the source file declared.
    pub schema: Option<String>,
    /// How many solids the importer counted in this definition.
    pub solids: Option<u32>,
}

impl ExportProvenance {
    pub fn new(
        file_name: Option<String>,
        source_unit: Option<String>,
        schema: Option<String>,
        solids: Option<u32>,
    ) -> Self {
        Self {
            file_name,
            source_unit,
            schema,
            solids,
        }
    }

    /// Whether anything at all was recorded.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Where a base colour came from.
///
/// A colour a file recorded and a colour nobody recorded are different facts,
/// and a writer is entitled to treat them differently. Collapsing them would
/// present a default as something the source said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Closed on purpose: a colour either came from the source or did not.
pub enum ExportColourOrigin {
    /// The source file said this.
    Source,
    /// Nothing said anything; this is the neutral colour a definition with no
    /// recorded appearance is given.
    Default,
}

/// One material slot of one definition.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ExportMaterial {
    /// A name for the slot. Not identity: two slots may share it.
    pub name: String,
    /// Linear RGB, the form imported colours are stored in. Converting to a
    /// display transfer function is the writer's business and happens once.
    pub base_colour_linear: [f64; 3],
    pub origin: ExportColourOrigin,
}

impl ExportMaterial {
    pub fn new(
        name: impl Into<String>,
        base_colour_linear: [f64; 3],
        origin: ExportColourOrigin,
    ) -> Result<Self> {
        let name = name.into();
        if let Some(component) = base_colour_linear
            .iter()
            .find(|value| !value.is_finite() || **value < 0.0)
        {
            return Err(CadError::input(format!(
                "material {name} has base colour component {component}, which is not a linear \
                 intensity"
            )));
        }
        Ok(Self {
            name,
            base_colour_linear,
            origin,
        })
    }
}

/// The exportable geometry of one definition, and nothing that only meant
/// something inside a kernel session.
///
/// Positions and normals are `f32` because that is what the kernel produced
/// and widening them here would invent precision the tessellation never had.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    triangle_materials: Vec<u32>,
    materials: Vec<ExportMaterial>,
}

impl ExportMesh {
    /// Builds a mesh, refusing one no writer could honestly serialise.
    ///
    /// The checks are the ones whose failure is invisible until much later: a
    /// normal array that does not match the positions makes a shaded surface
    /// wrong rather than absent, an index past the end is a reader crash in
    /// another program, and a material slot nothing defines is a file whose
    /// appearance depends on which reader opens it.
    pub fn new(
        positions: Vec<[f32; 3]>,
        normals: Vec<[f32; 3]>,
        triangles: Vec<[u32; 3]>,
        triangle_materials: Vec<u32>,
        materials: Vec<ExportMaterial>,
    ) -> Result<Self> {
        if positions.is_empty() || triangles.is_empty() {
            return Err(CadError::input(format!(
                "an exported mesh has {} vertices and {} triangles; a definition with no \
                 geometry is a separate state and must not be written as an empty mesh",
                positions.len(),
                triangles.len()
            )));
        }
        if normals.len() != positions.len() {
            return Err(CadError::input(format!(
                "an exported mesh has {} vertices and {} normals",
                positions.len(),
                normals.len()
            )));
        }
        if triangle_materials.len() != triangles.len() {
            return Err(CadError::input(format!(
                "an exported mesh has {} triangles and {} material assignments",
                triangles.len(),
                triangle_materials.len()
            )));
        }
        if materials.is_empty() {
            return Err(CadError::input(
                "an exported mesh with triangles has no material slot to assign them to",
            ));
        }

        for value in positions.iter().chain(normals.iter()).flatten() {
            if !value.is_finite() {
                return Err(CadError::input(format!(
                    "an exported mesh has the non-finite component {value}"
                )));
            }
        }

        let vertices = u32::try_from(positions.len()).map_err(|_| {
            CadError::input("an exported mesh has more vertices than uint32 can index")
        })?;
        for corner in triangles.iter().flatten() {
            if *corner >= vertices {
                return Err(CadError::input(format!(
                    "an exported triangle names vertex {corner} of {vertices}"
                )));
            }
        }

        let slots = u32::try_from(materials.len())
            .map_err(|_| CadError::input("an exported mesh has more material slots than uint32"))?;
        for slot in &triangle_materials {
            if *slot >= slots {
                return Err(CadError::input(format!(
                    "an exported triangle is assigned material slot {slot} of {slots}"
                )));
            }
        }

        Ok(Self {
            positions,
            normals,
            triangles,
            triangle_materials,
            materials,
        })
    }

    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// The normals the tessellation authored, one per position.
    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }

    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    /// Which material slot each triangle belongs to, parallel to
    /// [`triangles`][Self::triangles].
    pub fn triangle_materials(&self) -> &[u32] {
        &self.triangle_materials
    }

    pub fn materials(&self) -> &[ExportMaterial] {
        &self.materials
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }
}

/// Why a definition that was retained has no triangles.
///
/// Both halves are needed and neither is prose. The persisted finding is what
/// the document recorded when the file was imported; the refusal is the typed
/// answer this build's kernel gave. A historical warning cannot excuse an
/// unrelated failure now, and a failure now cannot rewrite what was recorded.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ExportOmission {
    /// The import-time observation the document stores.
    pub finding: Diagnostic,
    /// The typed refusal the current kernel gave. Not its message: the words
    /// shown to a person are free to change.
    pub refusal: TessellationRefusal,
}

impl ExportOmission {
    pub fn new(finding: Diagnostic, refusal: TessellationRefusal) -> Self {
        Self { finding, refusal }
    }
}

/// What a definition holds.
#[derive(Debug, Clone, PartialEq)]
/// Closed on purpose: the three states are the whole statement, and a
/// wildcard arm is exactly how structural emptiness and a missing part get
/// treated alike.
pub enum ExportGeometry {
    /// Real triangles.
    Mesh(ExportMesh),
    /// Structure with no geometry of its own — an assembly frame, whose parts
    /// are separate definitions placed inside it.
    Structural,
    /// Retained topology this build could not turn into triangles.
    Omitted(ExportOmission),
}

impl ExportGeometry {
    pub fn mesh(&self) -> Option<&ExportMesh> {
        match self {
            Self::Mesh(mesh) => Some(mesh),
            Self::Structural | Self::Omitted(_) => None,
        }
    }

    pub fn omission(&self) -> Option<&ExportOmission> {
        match self {
            Self::Omitted(omission) => Some(omission),
            Self::Mesh(_) | Self::Structural => None,
        }
    }

    pub fn is_structural(&self) -> bool {
        matches!(self, Self::Structural)
    }
}

/// Where a definition sits in this scene.
///
/// Durable for as long as this scene is, and no longer: it is an index into
/// [`ExportScene::definitions`]. What outlives the scene is
/// [`ExportDefinition::source`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExportDefinitionId(u32);

impl ExportDefinitionId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Where a node sits in this scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExportNodeId(u32);

impl ExportNodeId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// One thing that owns geometry, however many places it appears.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ExportDefinition {
    pub id: ExportDefinitionId,
    /// What this is, in terms that outlive the export.
    pub source: ExportSource,
    /// What the document or the file called it. Never identity.
    pub display_name: Option<String>,
    pub provenance: ExportProvenance,
    pub geometry: ExportGeometry,
}

/// A placement, in the exact local frame the source recorded.
///
/// Constructing one is where a placement no static-mesh format can express is
/// refused. Nothing is repaired, orthogonalised or decomposed on the way in: a
/// scene that silently straightened a shear would be a file that no longer
/// describes the model it was made from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportTransform {
    rows: [[f64; 4]; 3],
}

impl ExportTransform {
    pub const IDENTITY: Self = Self {
        rows: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    };

    /// Row-major 3x4: the linear part in the first three columns, the
    /// translation in the fourth.
    pub fn new(rows: [[f64; 4]; 3]) -> Result<Self> {
        for value in rows.iter().flatten() {
            if !value.is_finite() {
                return Err(CadError::unsupported(format!(
                    "a placement holds {value}, which is not a position anything can be exported \
                     to"
                )));
            }
        }

        let columns = [
            [rows[0][0], rows[1][0], rows[2][0]],
            [rows[0][1], rows[1][1], rows[2][1]],
            [rows[0][2], rows[1][2], rows[2][2]],
        ];
        let scales = columns.map(|column| dot(column, column).sqrt());
        if let Some(scale) = scales.iter().find(|scale| **scale <= TRANSFORM_TOLERANCE) {
            return Err(CadError::unsupported(format!(
                "a placement has an axis of length {scale}, so it collapses the geometry it \
                 places and cannot be exported"
            )));
        }

        for (left, right) in [(0, 1), (0, 2), (1, 2)] {
            let skew = dot(columns[left], columns[right]).abs();
            if skew > TRANSFORM_TOLERANCE * scales[left] * scales[right] {
                return Err(CadError::unsupported(format!(
                    "a placement's axes {left} and {right} are not perpendicular; a sheared \
                     placement has no translation, rotation and scale to export it as"
                )));
            }
        }

        let largest = scales.into_iter().fold(0.0_f64, f64::max);
        let smallest = scales.into_iter().fold(f64::INFINITY, f64::min);
        if largest - smallest > TRANSFORM_TOLERANCE * largest.max(1.0) {
            return Err(CadError::unsupported(format!(
                "a placement scales by {smallest} along one axis and {largest} along another; \
                 non-uniform scale is not exported without a policy for baking it"
            )));
        }

        let determinant = rows[0][0] * (rows[1][1] * rows[2][2] - rows[1][2] * rows[2][1])
            - rows[0][1] * (rows[1][0] * rows[2][2] - rows[1][2] * rows[2][0])
            + rows[0][2] * (rows[1][0] * rows[2][1] - rows[1][1] * rows[2][0]);
        if determinant <= TRANSFORM_TOLERANCE {
            return Err(CadError::unsupported(format!(
                "a placement has determinant {determinant}; a reflected or degenerate placement \
                 would turn every surface it places inside out"
            )));
        }

        Ok(Self { rows })
    }

    pub fn rows(&self) -> &[[f64; 4]; 3] {
        &self.rows
    }

    pub fn translation(&self) -> [f64; 3] {
        [self.rows[0][3], self.rows[1][3], self.rows[2][3]]
    }
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

/// One place a definition appears.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ExportNode {
    pub id: ExportNodeId,
    /// The node this sits inside, or `None` at the top of the scene. Parents
    /// always come before their children.
    pub parent: Option<ExportNodeId>,
    pub definition: ExportDefinitionId,
    /// Local to the parent, never accumulated. Composing the chain is a
    /// writer's business, and doing it here would throw away the structure
    /// the source recorded.
    pub local_transform: ExportTransform,
    /// What the document or the file called this placement. Never identity:
    /// two siblings may be called the same thing and remain two nodes.
    pub display_name: Option<String>,
    /// Linear RGB set on this placement rather than on its definition.
    pub colour_override: Option<[f64; 3]>,
    /// What this placement durably is, as the document recorded it.
    ///
    /// Beside [`Self::order`] and never instead of it: the order is where this
    /// node came in one export and changes whenever anything before it does,
    /// which is exactly why it is not an identity.
    pub occurrence: ExportOccurrence,
    /// Where this placement came in the document and its source, counted from
    /// zero across the whole export.
    pub order: u32,
}

/// One definition this export could not give geometry to, and every placement
/// of it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ExportOmissionReport {
    pub definition: ExportDefinitionId,
    /// The source-local identity, so a report names something a person can
    /// find in the file they exported.
    pub source: ExportSource,
    pub omission: ExportOmission,
    /// Every node that places this definition, in scene order.
    pub nodes: Vec<ExportNodeId>,
}

/// Whether this scene describes everything the document holds.
///
/// A report rather than a flag: a partial export and the list of what is
/// missing are one result, and a caller that could see the first without the
/// second would be able to describe a partial assembly as a complete one.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ExportCompleteness {
    omissions: Vec<ExportOmissionReport>,
}

impl ExportCompleteness {
    pub fn omissions(&self) -> &[ExportOmissionReport] {
        &self.omissions
    }

    pub fn is_complete(&self) -> bool {
        self.omissions.is_empty()
    }
}

/// A whole scene, ready for a writer and useful to nothing else.
///
/// Deliberately not serialisable and deliberately without an accessor for
/// anything transient: this is a value produced for one export and thrown
/// away. Storing it would store a picture of one build's tessellation under a
/// name that suggests it is the model.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportScene {
    definitions: Vec<ExportDefinition>,
    nodes: Vec<ExportNode>,
    completeness: ExportCompleteness,
}

impl ExportScene {
    /// Definitions in document and source order, geometry stored once each.
    pub fn definitions(&self) -> &[ExportDefinition] {
        &self.definitions
    }

    /// Nodes in document and source order, parents before children.
    pub fn nodes(&self) -> &[ExportNode] {
        &self.nodes
    }

    pub fn completeness(&self) -> &ExportCompleteness {
        &self.completeness
    }

    pub fn definition(&self, id: ExportDefinitionId) -> Option<&ExportDefinition> {
        self.definitions.get(id.index())
    }

    pub fn node(&self, id: ExportNodeId) -> Option<&ExportNode> {
        self.nodes.get(id.index())
    }

    /// The nodes with no parent.
    pub fn roots(&self) -> impl Iterator<Item = &ExportNode> {
        self.nodes.iter().filter(|node| node.parent.is_none())
    }
}

/// Assembles an [`ExportScene`], checking what a writer is entitled to assume.
///
/// Separate from the scene so the scene has no partially built state and no
/// way to be edited after it exists. The builder lives here, with the value it
/// makes, because what is valid is a property of the format-neutral model
/// rather than of whatever read a document.
#[derive(Debug, Default)]
pub struct ExportSceneBuilder {
    definitions: Vec<ExportDefinition>,
    nodes: Vec<ExportNode>,
}

impl ExportSceneBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a definition, refusing a second one with the same durable source.
    pub fn definition(
        &mut self,
        source: ExportSource,
        display_name: Option<String>,
        provenance: ExportProvenance,
        geometry: ExportGeometry,
    ) -> Result<ExportDefinitionId> {
        if let Some(earlier) = self
            .definitions
            .iter()
            .find(|definition| definition.source == source)
        {
            return Err(CadError::topology(format!(
                "{source:?} was added as definition {} and again as {}; one durable identity has \
                 one geometry",
                earlier.id.index(),
                self.definitions.len()
            )));
        }
        let id = ExportDefinitionId(u32::try_from(self.definitions.len()).map_err(|_| {
            CadError::input("an export cannot hold more definitions than uint32 can count")
        })?);
        self.definitions.push(ExportDefinition {
            id,
            source,
            display_name: display_name.filter(|name| !name.trim().is_empty()),
            provenance,
            geometry,
        });
        Ok(id)
    }

    /// Adds a node below an already added parent.
    ///
    /// A recorded [`ExportOccurrence`] must be one no earlier node of this
    /// scene claimed. Checked across the whole export rather than within one
    /// source, because the ways an identity gets reused are exactly the ways
    /// that cross a source boundary: two objects storing the same bytes, an
    /// identity copied from one imported object into another, a stored payload
    /// edited by hand. A scene in which two placements answer to one identity
    /// is one where a reference to it resolves to whichever was looked at
    /// first, and that is the failure this whole slice exists to prevent.
    pub fn node(
        &mut self,
        parent: Option<ExportNodeId>,
        definition: ExportDefinitionId,
        local_transform: ExportTransform,
        display_name: Option<String>,
        colour_override: Option<[f64; 3]>,
        occurrence: ExportOccurrence,
    ) -> Result<ExportNodeId> {
        if definition.index() >= self.definitions.len() {
            return Err(CadError::input(format!(
                "a node names definition {}, and {} have been added",
                definition.index(),
                self.definitions.len()
            )));
        }
        if let Some(parent) = parent
            && parent.index() >= self.nodes.len()
        {
            return Err(CadError::input(format!(
                "a node sits inside {}, which has not been added yet; parents come before their \
                 children",
                parent.index()
            )));
        }
        if let Some(colour) = colour_override
            && let Some(component) = colour
                .iter()
                .find(|value| !value.is_finite() || **value < 0.0)
        {
            return Err(CadError::input(format!(
                "a node overrides its colour with the component {component}, which is not a \
                 linear intensity"
            )));
        }
        if occurrence.is_recorded()
            && let Some(earlier) = self.nodes.iter().find(|node| node.occurrence == occurrence)
        {
            return Err(CadError::topology(format!(
                "{occurrence:?} identifies node {} and is claimed again by node {}; one \
                 placement identity names one placement",
                earlier.id.index(),
                self.nodes.len()
            )));
        }
        let order = u32::try_from(self.nodes.len()).map_err(|_| {
            CadError::input("an export cannot hold more nodes than uint32 can count")
        })?;
        let id = ExportNodeId(order);
        self.nodes.push(ExportNode {
            id,
            parent,
            definition,
            local_transform,
            display_name: display_name.filter(|name| !name.trim().is_empty()),
            colour_override,
            occurrence,
            order,
        });
        Ok(id)
    }

    /// Finishes the scene and derives its completeness report.
    ///
    /// The report is computed rather than supplied: a caller able to hand in
    /// its own list could describe a partial export as a complete one, which
    /// is the single thing this boundary exists to prevent.
    pub fn finish(self) -> Result<ExportScene> {
        for definition in &self.definitions {
            let placed = self
                .nodes
                .iter()
                .any(|node| node.definition == definition.id);
            if !placed {
                return Err(CadError::topology(format!(
                    "{:?} owns geometry that no node places, so exporting it would write \
                     something the scene does not contain",
                    definition.source
                )));
            }
        }

        let mut omissions = Vec::new();
        for definition in &self.definitions {
            let Some(omission) = definition.geometry.omission() else {
                continue;
            };
            let nodes: Vec<ExportNodeId> = self
                .nodes
                .iter()
                .filter(|node| node.definition == definition.id)
                .map(|node| node.id)
                .collect();
            omissions.push(ExportOmissionReport {
                definition: definition.id,
                source: definition.source.clone(),
                omission: omission.clone(),
                nodes,
            });
        }

        Ok(ExportScene {
            definitions: self.definitions,
            nodes: self.nodes,
            completeness: ExportCompleteness { omissions },
        })
    }
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use super::*;
    use ferritecad_exchange::{Severity, Stage};

    fn red() -> ExportMaterial {
        ExportMaterial::new(
            "red",
            [0.603_827, 0.033_105, 0.010_023],
            ExportColourOrigin::Source,
        )
        .expect("a linear colour")
    }

    fn blue() -> ExportMaterial {
        ExportMaterial::new(
            "blue",
            [0.010_023, 0.100_482, 0.787_412],
            ExportColourOrigin::Source,
        )
        .expect("a linear colour")
    }

    /// The §22B-1a reference shape: four control vertices, four triangles,
    /// twelve authored per-corner normals and two material slots.
    fn asymmetric() -> ExportMesh {
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [0.0, 2000.0, 0.0],
            [0.0, 0.0, 3000.0],
        ];
        let normals = vec![
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [-1.0, 0.0, 0.0],
        ];
        let triangles = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        ExportMesh::new(
            positions,
            normals,
            triangles,
            vec![0, 0, 1, 1],
            vec![red(), blue()],
        )
        .expect("the measured reference is a valid mesh")
    }

    #[test]
    fn the_measured_two_slot_reference_is_expressible() {
        let mesh = asymmetric();
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 4);
        assert_eq!(mesh.materials().len(), 2);
        assert_eq!(mesh.triangle_materials(), [0, 0, 1, 1]);
        assert_eq!(mesh.normals().len(), mesh.positions().len());
    }

    #[test]
    fn a_mesh_that_does_not_hold_together_is_refused() {
        let ok = asymmetric();
        let positions = ok.positions().to_vec();
        let normals = ok.normals().to_vec();
        let triangles = ok.triangles().to_vec();

        assert!(
            ExportMesh::new(
                positions.clone(),
                normals[..3].to_vec(),
                triangles.clone(),
                vec![0; 4],
                vec![red()],
            )
            .is_err(),
            "one normal per vertex is the whole point of authored normals"
        );
        assert!(
            ExportMesh::new(
                positions.clone(),
                normals.clone(),
                vec![[0, 1, 9]],
                vec![0],
                vec![red()],
            )
            .is_err(),
            "an index past the end is another program's crash"
        );
        assert!(
            ExportMesh::new(
                positions.clone(),
                normals.clone(),
                triangles.clone(),
                vec![0, 0, 1, 1],
                vec![red()],
            )
            .is_err(),
            "a triangle assigned to a slot nothing defines has no appearance"
        );
        assert!(
            ExportMesh::new(
                positions.clone(),
                normals.clone(),
                triangles.clone(),
                vec![0; 3],
                vec![red(), blue()],
            )
            .is_err(),
            "every triangle belongs to exactly one slot"
        );
        assert!(
            ExportMesh::new(
                positions.clone(),
                normals.clone(),
                triangles.clone(),
                vec![0; 4],
                Vec::new(),
            )
            .is_err(),
            "triangles with no slot at all"
        );

        let mut broken = positions.clone();
        broken[1][2] = f32::NAN;
        assert!(
            ExportMesh::new(
                broken,
                normals.clone(),
                triangles.clone(),
                vec![0; 4],
                vec![red()]
            )
            .is_err()
        );
        let mut broken = normals;
        broken[0][0] = f32::INFINITY;
        assert!(ExportMesh::new(positions, broken, triangles, vec![0; 4], vec![red()]).is_err());
    }

    #[test]
    fn an_empty_mesh_is_not_a_mesh() {
        // A definition with no geometry is a separate state, and writing it as
        // a mesh with nothing in it is how an omission stops being visible.
        assert!(
            ExportMesh::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()).is_err()
        );
        assert!(
            ExportMesh::new(
                vec![[0.0, 0.0, 0.0]],
                vec![[0.0, 0.0, 1.0]],
                Vec::new(),
                Vec::new(),
                vec![red()],
            )
            .is_err()
        );
    }

    #[test]
    fn a_representable_placement_is_kept_exactly() {
        let rows = [
            [0.5, -0.866_025_403_784_438_6, 0.0, 11.0],
            [0.866_025_403_784_438_6, 0.5, 0.0, 12.0],
            [0.0, 0.0, 1.0, 13.0],
        ];
        let transform = ExportTransform::new(rows).expect("a rotation with a translation");
        assert_eq!(*transform.rows(), rows);
        assert_eq!(transform.translation(), [11.0, 12.0, 13.0]);
        assert_eq!(
            *ExportTransform::IDENTITY.rows(),
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ]
        );
    }

    #[test]
    fn a_placement_no_static_mesh_hierarchy_can_express_is_refused() {
        let cases: [(&str, [[f64; 4]; 3]); 6] = [
            (
                "not a number",
                [
                    [1.0, 0.0, 0.0, f64::NAN],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
            ),
            (
                "infinite",
                [
                    [f64::INFINITY, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
            ),
            (
                "singular",
                [
                    [0.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
            ),
            (
                "sheared",
                [
                    [1.0, 0.5, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
            ),
            (
                "reflected",
                [
                    [-1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
            ),
            (
                "non-uniform",
                [
                    [2.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ],
            ),
        ];
        for (what, rows) in cases {
            let Err(error) = ExportTransform::new(rows) else {
                panic!("a {what} placement was accepted");
            };
            assert_eq!(
                error.kind(),
                ferritecad_types::ErrorKind::Unsupported,
                "a {what} placement was refused as something other than unsupported"
            );
        }
    }

    #[test]
    fn a_uniform_scale_is_representable_and_a_hair_of_shear_is_not() {
        let scaled = [
            [3.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 3.0, 0.0],
        ];
        assert!(ExportTransform::new(scaled).is_ok());

        let mut sheared = scaled;
        sheared[0][1] = 3.0 * TRANSFORM_TOLERANCE * 100.0;
        assert!(
            ExportTransform::new(sheared).is_err(),
            "the measured tolerance is what separates rounding from a shear"
        );
    }

    fn finding() -> Diagnostic {
        Diagnostic {
            stage: Stage::Validation,
            severity: Severity::Fail,
            entity: "step.product_definition#2583".to_owned(),
            message: "the imported definition contains an invalid solid".to_owned(),
        }
    }

    fn imported(key: &str) -> ExportSource {
        ExportSource::Imported {
            source: ImportedSourceId::new(),
            definition_key: key.to_owned(),
        }
    }

    #[test]
    fn a_scene_reports_every_omission_and_every_placement_of_it() {
        let mut builder = ExportSceneBuilder::new();
        let frame = builder
            .definition(
                imported("step.product_definition#1"),
                Some("Assembly".to_owned()),
                ExportProvenance::default(),
                ExportGeometry::Structural,
            )
            .expect("a frame");
        let part = builder
            .definition(
                imported("step.product_definition#2428"),
                Some("Part".to_owned()),
                ExportProvenance::new(None, Some("MILLIMETRE".to_owned()), None, Some(1)),
                ExportGeometry::Mesh(asymmetric()),
            )
            .expect("a part");
        let missing = builder
            .definition(
                imported("step.product_definition#2583"),
                Some("Missing".to_owned()),
                ExportProvenance::default(),
                ExportGeometry::Omitted(ExportOmission::new(
                    finding(),
                    TessellationRefusal::IncompleteFace,
                )),
            )
            .expect("an omission");

        let root = builder
            .node(
                None,
                frame,
                ExportTransform::IDENTITY,
                None,
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("a root");
        builder
            .node(
                Some(root),
                part,
                ExportTransform::IDENTITY,
                None,
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("a placement");
        let second = builder
            .node(
                Some(root),
                missing,
                ExportTransform::IDENTITY,
                Some("first".to_owned()),
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("a placement of the omitted part");
        let third = builder
            .node(
                Some(root),
                missing,
                ExportTransform::IDENTITY,
                Some("second".to_owned()),
                Some([0.1, 0.2, 0.3]),
                ExportOccurrence::Unrecorded,
            )
            .expect("another placement of it");

        let scene = builder.finish().expect("the scene is complete enough");
        assert_eq!(scene.definitions().len(), 3);
        assert_eq!(scene.nodes().len(), 4);
        assert_eq!(scene.roots().count(), 1);
        assert!(!scene.completeness().is_complete());

        let reports = scene.completeness().omissions();
        assert_eq!(reports.len(), 1, "a frame is not an omission");
        assert_eq!(reports[0].definition, missing);
        assert_eq!(reports[0].nodes, vec![second, third]);
        assert_eq!(reports[0].omission.finding.entity, finding().entity);
        assert_eq!(
            reports[0].omission.refusal,
            TessellationRefusal::IncompleteFace
        );
        assert_eq!(
            scene.node(third).expect("a node").colour_override,
            Some([0.1, 0.2, 0.3])
        );
        assert_eq!(
            scene
                .definition(part)
                .expect("a definition")
                .provenance
                .solids,
            Some(1)
        );
    }

    #[test]
    fn one_durable_identity_is_one_definition() {
        let source = ImportedSourceId::new();
        let same = || ExportSource::Imported {
            source,
            definition_key: "step.product_definition#5".to_owned(),
        };
        let mut builder = ExportSceneBuilder::new();
        builder
            .definition(
                same(),
                Some("Bracket".to_owned()),
                ExportProvenance::default(),
                ExportGeometry::Structural,
            )
            .expect("the first");
        assert!(
            builder
                .definition(
                    same(),
                    Some("Support".to_owned()),
                    ExportProvenance::default(),
                    ExportGeometry::Mesh(asymmetric()),
                )
                .is_err(),
            "two geometries under one identity"
        );
    }

    #[test]
    fn a_node_cannot_name_what_is_not_there_yet() {
        let mut builder = ExportSceneBuilder::new();
        let definition = builder
            .definition(
                imported("step.product_definition#5"),
                None,
                ExportProvenance::default(),
                ExportGeometry::Mesh(asymmetric()),
            )
            .expect("a definition");
        let unplaced = ExportDefinitionId(7);
        assert!(
            builder
                .node(
                    None,
                    unplaced,
                    ExportTransform::IDENTITY,
                    None,
                    None,
                    ExportOccurrence::Unrecorded
                )
                .is_err()
        );
        let root = builder
            .node(
                None,
                definition,
                ExportTransform::IDENTITY,
                None,
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("a root");
        assert!(
            builder
                .node(
                    Some(ExportNodeId(9)),
                    definition,
                    ExportTransform::IDENTITY,
                    None,
                    None,
                    ExportOccurrence::Unrecorded,
                )
                .is_err(),
            "parents come before their children"
        );
        assert_eq!(root.index(), 0);
    }

    #[test]
    fn a_definition_nothing_places_is_not_a_scene() {
        let mut builder = ExportSceneBuilder::new();
        builder
            .definition(
                imported("step.product_definition#5"),
                None,
                ExportProvenance::default(),
                ExportGeometry::Mesh(asymmetric()),
            )
            .expect("a definition");
        assert!(
            builder.finish().is_err(),
            "geometry no node places would be written into a file that does not contain it"
        );
    }
}
