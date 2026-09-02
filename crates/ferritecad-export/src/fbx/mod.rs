// SPDX-License-Identifier: MIT
//! Writing an [`ExportScene`] as FBX 7.4 ASCII.
//!
//! The whole writer is one function of one value: it is handed a scene and a
//! byte sink and has no other way to learn anything. It cannot open a
//! document, call a kernel, read a STEP file, look at a picture or ask what
//! time it is, so the bytes it produces are a function of the scene and
//! nothing else. That is the property the whole export rests on, and it is
//! why the sink is a [`Write`] rather than a path: publishing a file is
//! somebody else's decision, made somewhere this code cannot reach.
//!
//! # What the file says
//!
//! FBX 7.4.0 in the ASCII encoding, with the axis and unit metadata the
//! §22B-1a measurement settled and the coordinate conversion applied exactly
//! once. Every definition that has triangles becomes one `Geometry` object,
//! however many places it appears; every node becomes one `Model`, whether it
//! has geometry, is an assembly frame, or is a definition this build could not
//! give triangles to. Parents are connections, not multiplied-out matrices.
//!
//! # Identity is a number, and a name is not identity
//!
//! Two siblings a source called the same thing stay two `Model` objects with
//! the same name. FBX identity is the 64-bit object number, which is derived
//! from where a node sits in the scene and never from what it is called, so
//! equal names cannot merge two nodes and the writer never renames one to
//! keep them apart. The deterministic key travels beside the name as a
//! property, where an importer can read it.
//!
//! # A partial export says so in the file and in the report
//!
//! A definition with no triangles keeps its hierarchy node and carries
//! properties naming its source-local key, the finding the import persisted
//! and the typed refusal this build got. The report returned beside the bytes
//! lists the same omissions the scene's own completeness does, because it is
//! derived from it: there is no way to hand this writer a list of what is
//! missing, and so no way for it to call a partial export complete.

mod contract;
mod syntax;

use std::io::Write;

use ferritecad_types::{CadError, Result};

use crate::scene::{
    ExportDefinition, ExportGeometry, ExportMesh, ExportNodeId, ExportOmissionReport, ExportScene,
    ExportSource,
};
use syntax::{Ascii, Value};

/// The version this writer emits, and the only one it has been measured
/// against.
const FBX_VERSION: i64 = 7400;

/// Where each kind of object's numbers begin.
///
/// Spaced by more than a `u32` so a scene with the largest possible number of
/// definitions and nodes still cannot make two kinds collide, and derived
/// from position alone so a display name can never change an identity.
const GEOMETRY_BASE: i64 = 1 << 33;
const MODEL_BASE: i64 = 2 << 33;
const MATERIAL_BASE: i64 = 3 << 33;
const DOCUMENT_ID: i64 = 4 << 33;

/// Constants where other writers put a clock, a host name or a random
/// identifier. Two exports of one scene are one file, so none of those may
/// appear.
const CREATOR: &str = "FerriteCAD FBX 7.4 ASCII writer";
const CREATION_TIME: &str = "2000-01-01 00:00:00:000";
const FILE_ID: &str = "FCAD-FBX-7400-ASCII";

/// What one write produced, and what it could not.
///
/// Derived from the scene rather than supplied to the writer: a caller able to
/// hand in its own list of omissions would be a caller able to describe a
/// partial export as a complete one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FbxWriteReport {
    omissions: Vec<ExportOmissionReport>,
    bytes: u64,
    models: u32,
    geometries: u32,
    materials: u32,
}

impl FbxWriteReport {
    /// The exact completeness records from the scene that was written.
    ///
    /// Keeping the records whole matters: the source identity qualifies an
    /// imported definition's local key, and both the persisted finding and
    /// the current typed refusal are facts the publishing layer must report.
    pub fn omissions(&self) -> &[ExportOmissionReport] {
        &self.omissions
    }

    pub fn is_complete(&self) -> bool {
        self.omissions.is_empty()
    }

    /// How many bytes were written.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// One per node of the scene.
    pub fn models(&self) -> u32 {
        self.models
    }

    /// One per definition that has triangles, however often it is placed.
    pub fn geometries(&self) -> u32 {
        self.geometries
    }

    pub fn materials(&self) -> u32 {
        self.materials
    }
}

/// Writes `scene` to `output` as FBX 7.4 ASCII, and says what it could not
/// write.
///
/// Deterministic: the same scene always produces the same bytes, on every
/// platform and however often it is written. Nothing here reads a clock, a
/// host name, a path, an environment or a random number, no hash map is
/// iterated into the output, and every value that comes from the platform's
/// maths library is rounded to a fixed number of decimals before it is
/// written.
///
/// Refuses rather than repairs. A placement that cannot be rebuilt from the
/// translation, three angles and one scale FBX stores, a name this format
/// cannot spell, and any value that is not a number all stop the write with a
/// typed refusal rather than producing a file that describes something else.
pub fn write_fbx_ascii_7400(
    scene: &ExportScene,
    output: &mut impl Write,
) -> Result<FbxWriteReport> {
    let plan = Plan::of(scene)?;
    let mut ascii = Ascii::new(output);

    ascii.comment("FBX 7.4.0 project file")?;
    ascii.comment(CREATOR)?;
    ascii.blank()?;

    plan.header(&mut ascii)?;
    plan.global_settings(&mut ascii)?;
    plan.documents(&mut ascii)?;
    plan.definitions(&mut ascii)?;
    plan.objects(&mut ascii)?;
    plan.connections(&mut ascii)?;
    plan.takes(&mut ascii)?;

    Ok(FbxWriteReport {
        omissions: plan.omissions,
        bytes: ascii.bytes(),
        models: count(scene.nodes().len(), "nodes")?,
        geometries: count(plan.geometries.len(), "geometries")?,
        materials: count(plan.materials.len(), "materials")?,
    })
}

fn count(value: usize, what: &str) -> Result<u32> {
    u32::try_from(value)
        .map_err(|_| CadError::input(format!("an FBX cannot record {value} {what}")))
}

/// One material object of the file.
#[derive(Debug)]
struct MaterialObject {
    name: String,
    /// Already converted to the display transfer FBX records.
    colour: [f64; 3],
}

/// Everything the file will say, worked out before a byte is written.
///
/// Built first so that a scene the writer cannot express is refused before the
/// sink has been given half a file.
#[derive(Debug)]
struct Plan<'a> {
    scene: &'a ExportScene,
    /// The definitions that have triangles, by their index in the scene.
    geometries: Vec<usize>,
    materials: Vec<MaterialObject>,
    /// Which materials each node binds, in slot order, as indices into
    /// [`Self::materials`].
    bindings: Vec<Vec<usize>>,
    omissions: Vec<ExportOmissionReport>,
}

impl<'a> Plan<'a> {
    fn of(scene: &'a ExportScene) -> Result<Self> {
        let mut geometries = Vec::new();
        let mut materials: Vec<MaterialObject> = Vec::new();
        // Which material objects each definition's own slots became.
        let mut definition_slots: Vec<Vec<usize>> = vec![Vec::new(); scene.definitions().len()];

        for (index, definition) in scene.definitions().iter().enumerate() {
            let Some(mesh) = definition.geometry.mesh() else {
                continue;
            };
            geometries.push(index);
            for slot in mesh.materials() {
                definition_slots[index].push(push_material(
                    &mut materials,
                    &slot.name,
                    slot.base_colour_linear,
                )?);
            }
        }

        // A colour set on one placement is that placement's own binding. It
        // gets its own material objects rather than changing the definition's,
        // because a definition placed twice with two colours is still one
        // definition and one geometry.
        let mut bindings: Vec<Vec<usize>> = Vec::with_capacity(scene.nodes().len());
        for node in scene.nodes() {
            let definition = scene.definition(node.definition).ok_or_else(|| {
                CadError::topology(format!(
                    "node {} places definition {}, which this scene does not have",
                    node.id.index(),
                    node.definition.index()
                ))
            })?;
            let bound = match (definition.geometry.mesh(), node.colour_override) {
                (None, _) => Vec::new(),
                (Some(_), None) => definition_slots[node.definition.index()].clone(),
                (Some(mesh), Some(colour)) => {
                    let mut bound = Vec::with_capacity(mesh.materials().len());
                    for slot in mesh.materials() {
                        bound.push(push_material(&mut materials, &slot.name, colour)?);
                    }
                    bound
                }
            };
            bindings.push(bound);
        }

        let omissions = scene.completeness().omissions().to_vec();

        Ok(Self {
            scene,
            geometries,
            materials,
            bindings,
            omissions,
        })
    }

    fn header<W: Write>(&self, ascii: &mut Ascii<W>) -> Result<()> {
        ascii.open("FBXHeaderExtension", &[])?;
        ascii.leaf("FBXHeaderVersion", &[Value::Int(1003)])?;
        ascii.leaf("FBXVersion", &[Value::Int(FBX_VERSION)])?;
        ascii.leaf("EncryptionType", &[Value::Int(0)])?;
        ascii.open("CreationTimeStamp", &[])?;
        ascii.leaf("Version", &[Value::Int(1000)])?;
        for (field, value) in [
            ("Year", 2000),
            ("Month", 1),
            ("Day", 1),
            ("Hour", 0),
            ("Minute", 0),
            ("Second", 0),
            ("Millisecond", 0),
        ] {
            ascii.leaf(field, &[Value::Int(value)])?;
        }
        ascii.close()?;
        ascii.leaf("Creator", &[Value::Text(CREATOR)])?;
        ascii.close()?;

        ascii.leaf("FileId", &[Value::Text(FILE_ID)])?;
        ascii.leaf("CreationTime", &[Value::Text(CREATION_TIME)])?;
        ascii.leaf("Creator", &[Value::Text(CREATOR)])
    }

    /// The single measured contract: right is `+X`, up is `+Y`, front-opposite
    /// forward is `+Z`, and one unit is one metre.
    fn global_settings<W: Write>(&self, ascii: &mut Ascii<W>) -> Result<()> {
        ascii.open("GlobalSettings", &[])?;
        ascii.leaf("Version", &[Value::Int(1000)])?;
        ascii.open("Properties70", &[])?;
        for (name, value) in [
            ("UpAxis", 1),
            ("UpAxisSign", 1),
            ("FrontAxis", 2),
            ("FrontAxisSign", 1),
            ("CoordAxis", 0),
            ("CoordAxisSign", 1),
            // What the file was authored in, which is the same thing: the
            // conversion happened before the file, not inside it.
            ("OriginalUpAxis", 1),
            ("OriginalUpAxisSign", 1),
        ] {
            ascii.property(name, "int", "Integer", "", &[Value::Int(value)])?;
        }
        for name in ["UnitScaleFactor", "OriginalUnitScaleFactor"] {
            ascii.property(
                name,
                "double",
                "Number",
                "",
                &[Value::Double(contract::UNIT_SCALE_FACTOR)],
            )?;
        }
        ascii.close()?;
        ascii.close()
    }

    fn documents<W: Write>(&self, ascii: &mut Ascii<W>) -> Result<()> {
        ascii.open("Documents", &[])?;
        ascii.leaf("Count", &[Value::Int(1)])?;
        ascii.open(
            "Document",
            &[
                Value::Int(DOCUMENT_ID),
                Value::Text("Scene"),
                Value::Text("Scene"),
            ],
        )?;
        ascii.open("Properties70", &[])?;
        ascii.close()?;
        ascii.leaf("RootNode", &[Value::Int(0)])?;
        ascii.close()?;
        ascii.close()?;
        ascii.open("References", &[])?;
        ascii.close()
    }

    fn definitions<W: Write>(&self, ascii: &mut Ascii<W>) -> Result<()> {
        let geometries = self.geometries.len();
        let models = self.scene.nodes().len();
        let materials = self.materials.len();
        ascii.open("Definitions", &[])?;
        ascii.leaf("Version", &[Value::Int(100)])?;
        ascii.leaf(
            "Count",
            &[Value::Int(as_int(1 + geometries + models + materials)?)],
        )?;
        for (kind, count) in [
            ("GlobalSettings", 1),
            ("Geometry", geometries),
            ("Model", models),
            ("Material", materials),
        ] {
            if count == 0 {
                continue;
            }
            ascii.open("ObjectType", &[Value::Text(kind)])?;
            ascii.leaf("Count", &[Value::Int(as_int(count)?)])?;
            ascii.close()?;
        }
        ascii.close()
    }

    fn objects<W: Write>(&self, ascii: &mut Ascii<W>) -> Result<()> {
        ascii.open("Objects", &[])?;
        for index in &self.geometries {
            let definition = self
                .scene
                .definitions()
                .get(*index)
                .ok_or_else(|| CadError::topology("a planned geometry left the scene"))?;
            let mesh = definition
                .geometry
                .mesh()
                .ok_or_else(|| CadError::topology("a planned geometry has no mesh"))?;
            self.geometry(ascii, *index, definition, mesh)?;
        }
        for node in self.scene.nodes() {
            self.model(ascii, node)?;
        }
        for (index, material) in self.materials.iter().enumerate() {
            self.material(ascii, index, material)?;
        }
        ascii.close()
    }

    fn geometry<W: Write>(
        &self,
        ascii: &mut Ascii<W>,
        index: usize,
        definition: &ExportDefinition,
        mesh: &ExportMesh,
    ) -> Result<()> {
        let name = object_name(
            definition.display_name.as_deref(),
            &definition.source,
            index,
        );
        ascii.open(
            "Geometry",
            &[
                Value::Int(geometry_id(index)?),
                Value::Text(&format!("Geometry::{name}")),
                Value::Text("Mesh"),
            ],
        )?;
        ascii.leaf("GeometryVersion", &[Value::Int(124)])?;

        // One conversion, here, of the positions the kernel produced.
        ascii.array(
            "Vertices",
            mesh.positions().len() * 3,
            mesh.positions()
                .iter()
                .flat_map(|position| contract::point(*position))
                .map(syntax::double),
        )?;

        // The polygon order the source recorded, with the last corner of each
        // polygon written as its bitwise negation, which is how this format
        // says where a polygon ends. No winding is reversed: the coordinate
        // map's determinant is +1.
        ascii.array(
            "PolygonVertexIndex",
            mesh.triangles().len() * 3,
            mesh.triangles()
                .iter()
                .flat_map(|triangle| {
                    [
                        (triangle[0], false),
                        (triangle[1], false),
                        (triangle[2], true),
                    ]
                })
                .map(|(vertex, terminal)| {
                    let index = as_index(vertex)?;
                    syntax::integer(i64::from(if terminal { !index } else { index }))
                }),
        )?;

        // The authored normals, one per polygon vertex, rotated and neither
        // recalculated nor averaged. Converted once per vertex rather than
        // once per corner, because a vertex a dozen polygons share is one
        // authored normal and not a dozen.
        let normals: Vec<[f64; 3]> = mesh
            .normals()
            .iter()
            .map(|normal| contract::direction(*normal))
            .collect();
        ascii.open("LayerElementNormal", &[Value::Int(0)])?;
        ascii.leaf("Version", &[Value::Int(101)])?;
        ascii.leaf("Name", &[Value::Text("FCAD authored normals")])?;
        ascii.leaf("MappingInformationType", &[Value::Text("ByPolygonVertex")])?;
        ascii.leaf("ReferenceInformationType", &[Value::Text("Direct")])?;
        ascii.array(
            "Normals",
            mesh.triangles().len() * 9,
            mesh.triangles().iter().flatten().flat_map(|corner| {
                let normal = normals.get(*corner as usize).copied();
                [0usize, 1, 2].map(move |axis| match normal {
                    Some(normal) => syntax::double(normal[axis]),
                    None => Err(CadError::topology(
                        "a triangle names a vertex that has no authored normal",
                    )),
                })
            }),
        )?;
        ascii.close()?;

        ascii.open("LayerElementMaterial", &[Value::Int(0)])?;
        ascii.leaf("Version", &[Value::Int(101)])?;
        ascii.leaf("Name", &[Value::Text("")])?;
        ascii.leaf("MappingInformationType", &[Value::Text("ByPolygon")])?;
        ascii.leaf("ReferenceInformationType", &[Value::Text("IndexToDirect")])?;
        ascii.array(
            "Materials",
            mesh.triangle_materials().len(),
            mesh.triangle_materials()
                .iter()
                .map(|slot| syntax::integer(i64::from(as_index(*slot)?))),
        )?;
        ascii.close()?;

        ascii.open("Layer", &[Value::Int(0)])?;
        ascii.leaf("Version", &[Value::Int(100)])?;
        for kind in ["LayerElementNormal", "LayerElementMaterial"] {
            ascii.open("LayerElement", &[])?;
            ascii.leaf("Type", &[Value::Text(kind)])?;
            ascii.leaf("TypedIndex", &[Value::Int(0)])?;
            ascii.close()?;
        }
        ascii.close()?;
        ascii.close()
    }

    fn model<W: Write>(&self, ascii: &mut Ascii<W>, node: &crate::scene::ExportNode) -> Result<()> {
        let definition = self.scene.definition(node.definition).ok_or_else(|| {
            CadError::topology(format!(
                "node {} places a definition this scene does not have",
                node.id.index()
            ))
        })?;
        let has_mesh = definition.geometry.mesh().is_some();
        let kind = if has_mesh { "Mesh" } else { "Null" };
        // Written exactly as the source recorded it. Two siblings a source
        // called the same thing stay two models with one name; what tells them
        // apart is the identity below and the key property, never a suffix
        // this writer invented.
        let name = node.display_name.as_deref().unwrap_or_default();

        ascii.open(
            "Model",
            &[
                Value::Int(model_id(node.id.index())?),
                Value::Text(&format!("Model::{name}")),
                Value::Text(kind),
            ],
        )?;
        ascii.leaf("Version", &[Value::Int(232)])?;

        let trs = contract::local_transform(&node.local_transform)?;
        ascii.open("Properties70", &[])?;
        ascii.property(
            "Lcl Translation",
            "Lcl Translation",
            "",
            "A",
            &trs.translation.map(Value::Double),
        )?;
        ascii.property(
            "Lcl Rotation",
            "Lcl Rotation",
            "",
            "A",
            &trs.rotation_degrees.map(Value::Double),
        )?;
        ascii.property(
            "Lcl Scaling",
            "Lcl Scaling",
            "",
            "A",
            &[
                Value::Double(trs.scale),
                Value::Double(trs.scale),
                Value::Double(trs.scale),
            ],
        )?;

        let key = definition_key(&definition.source);
        ascii.property(
            "FerriteCADNodeKey",
            "KString",
            "",
            "U",
            &[Value::Text(&node_key(node.id))],
        )?;
        ascii.property(
            "FerriteCADDefinitionKey",
            "KString",
            "",
            "U",
            &[Value::Text(&key)],
        )?;
        // Structure carries no marker: an assembly frame that never had its
        // own geometry and a part that went missing are different facts, and
        // marking both would make the second invisible.
        if let ExportGeometry::Omitted(omission) = &definition.geometry {
            ascii.property(
                "FerriteCADGeometryOmission",
                "KString",
                "",
                "U",
                &[Value::Text(&key)],
            )?;
            ascii.property(
                "FerriteCADOmissionFinding",
                "KString",
                "",
                "U",
                &[Value::Text(&omission.finding.entity)],
            )?;
            ascii.property(
                "FerriteCADOmissionRefusal",
                "KString",
                "",
                "U",
                &[Value::Text(omission.refusal.stable_name())],
            )?;
            ascii.property("FerriteCADComplete", "bool", "", "U", &[Value::bool(false)])?;
        }
        ascii.close()?;

        ascii.leaf("Shading", &[Value::bool(true)])?;
        ascii.leaf("Culling", &[Value::Text("CullingOff")])?;
        if !has_mesh {
            ascii.leaf("TypeFlags", &[Value::Text("Null")])?;
        }
        ascii.close()
    }

    fn material<W: Write>(
        &self,
        ascii: &mut Ascii<W>,
        index: usize,
        material: &MaterialObject,
    ) -> Result<()> {
        ascii.open(
            "Material",
            &[
                Value::Int(material_id(index)?),
                Value::Text(&format!("Material::{}", material.name)),
                Value::Text(""),
            ],
        )?;
        ascii.leaf("Version", &[Value::Int(102)])?;
        ascii.leaf("ShadingModel", &[Value::Text("phong")])?;
        ascii.leaf("MultiLayer", &[Value::bool(false)])?;
        ascii.open("Properties70", &[])?;
        ascii.property(
            "DiffuseColor",
            "ColorRGB",
            "Color",
            "",
            &material.colour.map(Value::Double),
        )?;
        ascii.property("DiffuseFactor", "Number", "", "A", &[Value::Double(1.0)])?;
        ascii.property(
            "TransparencyFactor",
            "Number",
            "",
            "A",
            &[Value::Double(0.0)],
        )?;
        ascii.close()?;
        ascii.close()
    }

    /// The hierarchy, the geometry sharing and the material bindings, in that
    /// order. Every one of them is a connection between two object numbers:
    /// nothing here is a name, and nothing is a matrix with its parents
    /// multiplied in.
    fn connections<W: Write>(&self, ascii: &mut Ascii<W>) -> Result<()> {
        ascii.open("Connections", &[])?;
        for node in self.scene.nodes() {
            let parent = match node.parent {
                None => 0,
                Some(parent) => model_id(parent.index())?,
            };
            ascii.leaf(
                "C",
                &[
                    Value::Text("OO"),
                    Value::Int(model_id(node.id.index())?),
                    Value::Int(parent),
                ],
            )?;
        }
        for index in &self.geometries {
            let geometry = geometry_id(*index)?;
            for node in self.scene.nodes() {
                if node.definition.index() != *index {
                    continue;
                }
                ascii.leaf(
                    "C",
                    &[
                        Value::Text("OO"),
                        Value::Int(geometry),
                        Value::Int(model_id(node.id.index())?),
                    ],
                )?;
            }
        }
        for (node, bound) in self.scene.nodes().iter().zip(&self.bindings) {
            for material in bound {
                ascii.leaf(
                    "C",
                    &[
                        Value::Text("OO"),
                        Value::Int(material_id(*material)?),
                        Value::Int(model_id(node.id.index())?),
                    ],
                )?;
            }
        }
        ascii.close()
    }

    fn takes<W: Write>(&self, ascii: &mut Ascii<W>) -> Result<()> {
        ascii.open("Takes", &[])?;
        ascii.leaf("Current", &[Value::Text("")])?;
        ascii.close()
    }
}

/// Adds one material object and says where it went.
///
/// Every material gets a name that is unique in the file and derived from its
/// position, because two objects a reader tells apart only by name would be
/// one object to a reader that merges by name. The slot's own name is kept in
/// front of it, so the file is still readable by a person.
fn push_material(
    materials: &mut Vec<MaterialObject>,
    name: &str,
    linear: [f64; 3],
) -> Result<usize> {
    let index = materials.len();
    let mut colour = [0.0; 3];
    for (component, value) in colour.iter_mut().zip(linear) {
        *component = contract::srgb(value)?;
    }
    materials.push(MaterialObject {
        name: format!("{name} #{index}"),
        colour,
    });
    Ok(index)
}

/// What a definition is called in the file.
///
/// A display name when there is one, and its source-local key when there is
/// not, with the position appended so two definitions a source called the same
/// thing are two objects rather than one.
fn object_name(display_name: Option<&str>, source: &ExportSource, index: usize) -> String {
    match display_name {
        Some(name) => format!("{name} #{index}"),
        None => format!("{} #{index}", definition_key(source)),
    }
}

/// The source-local identity of a definition, as a file records it.
///
/// Never a filesystem path, a session identity or a number that means
/// something only while this process runs: an importer reading this property
/// years later must be able to find the same definition in the same source.
fn definition_key(source: &ExportSource) -> String {
    match source {
        ExportSource::Body { object } => format!("body/{object}"),
        ExportSource::Imported { definition_key, .. } => definition_key.clone(),
    }
}

/// A node's deterministic key, which is where it sits in the scene and not
/// what it is called.
fn node_key(node: ExportNodeId) -> String {
    format!("node/{}", node.index())
}

fn geometry_id(index: usize) -> Result<i64> {
    Ok(GEOMETRY_BASE + as_int(index)?)
}

fn model_id(index: usize) -> Result<i64> {
    Ok(MODEL_BASE + as_int(index)?)
}

fn material_id(index: usize) -> Result<i64> {
    Ok(MATERIAL_BASE + as_int(index)?)
}

fn as_int(value: usize) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| CadError::input(format!("an FBX cannot number {value} objects")))
}

fn as_index(value: u32) -> Result<i32> {
    i32::try_from(value).map_err(|_| {
        CadError::input(format!(
            "an FBX polygon index counts in 32 signed bits and this mesh names vertex {value}"
        ))
    })
}
