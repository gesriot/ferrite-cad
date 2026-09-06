// SPDX-License-Identifier: MIT
//! The scenes the FBX gates measure, and a reader for what the writer wrote.
//!
//! Shared between the integration gate and the example that produces bytes for
//! the independent `ufbx` and Unity gates, so there is one definition of the
//! measured scene rather than two that can drift. Each user needs a different
//! half of it.
#![allow(dead_code, reason = "each user of this module needs a different half")]
#![allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]

use std::collections::BTreeMap;

use ferritecad_exchange::{Diagnostic, Severity, Stage};
use ferritecad_export::{
    ExportColourOrigin, ExportDefinitionIdentity, ExportGeometry, ExportMaterial, ExportMesh,
    ExportOccurrence, ExportOmission, ExportProvenance, ExportScene, ExportSceneBuilder,
    ExportSource, ExportTransform,
};
use ferritecad_kernel::TessellationRefusal;
use ferritecad_types::{ImportedSourceId, OccurrenceId};

// ----------------------------------------------------------- fixed identities
//
// Spelled out rather than minted. §22B-1e3b puts the durable identities into
// the file, so a scene that made up a fresh `ImportedSourceId` or
// `OccurrenceId` would write different bytes on every run, and the committed
// digests would be a record of one process rather than of the writer. Every
// one of these is a well-formed UUIDv7, which is what the identifier types
// require, and none of them is derived from anything in the scene.

/// The one source the measured assembly was imported from.
const MEASURED_SOURCE: &str = "019ffc72-1e3b-7000-8000-000000000001";
/// The source of the escaping scene, which is a different file.
const ESCAPING_SOURCE: &str = "019ffc72-1e3b-7000-8000-000000000002";

/// One durable placement identity per node of the measured scene, in the order
/// the nodes are added.
const MEASURED_OCCURRENCES: [&str; 9] = [
    "019ffc72-1e3b-7000-8000-0000000000a0",
    "019ffc72-1e3b-7000-8000-0000000000a1",
    "019ffc72-1e3b-7000-8000-0000000000a2",
    "019ffc72-1e3b-7000-8000-0000000000a3",
    "019ffc72-1e3b-7000-8000-0000000000a4",
    "019ffc72-1e3b-7000-8000-0000000000a5",
    "019ffc72-1e3b-7000-8000-0000000000a6",
    "019ffc72-1e3b-7000-8000-0000000000a7",
    "019ffc72-1e3b-7000-8000-0000000000a8",
];

/// The source of the identity-escaping scene, which is a third file.
const KEYED_SOURCE: &str = "019ffc72-1e3b-7000-8000-000000000003";

/// The escaping scene's placement identities, one per named node. The
/// identity-escaping scene beside it uses the same five, because it is a
/// different file and its identities are qualified by its own source.
const ESCAPING_OCCURRENCES: [&str; 5] = [
    "019ffc72-1e3b-7000-8000-0000000000b0",
    "019ffc72-1e3b-7000-8000-0000000000b1",
    "019ffc72-1e3b-7000-8000-0000000000b2",
    "019ffc72-1e3b-7000-8000-0000000000b3",
    "019ffc72-1e3b-7000-8000-0000000000b4",
];

pub fn source_of(text: &str) -> ImportedSourceId {
    text.parse().expect("a fixed UUIDv7")
}

pub fn occurrence_of(text: &str) -> OccurrenceId {
    text.parse().expect("a fixed UUIDv7")
}

// ------------------------------------------------------------------ scenes

fn imported(source: ImportedSourceId, key: &str) -> ExportSource {
    ExportSource::Imported {
        source,
        definition_key: key.to_owned(),
    }
}

/// The durable identity of an imported definition, or the fact that a legacy
/// layout recorded none.
fn identified(source: ImportedSourceId, key: &str, identify: Identify) -> ExportDefinitionIdentity {
    match identify {
        Identify::Yes | Identify::Renamed => ExportDefinitionIdentity::Source {
            source,
            definition_key: key.to_owned(),
        },
        Identify::No => ExportDefinitionIdentity::Unrecorded,
    }
}

fn provenance() -> ExportProvenance {
    ExportProvenance::new(
        Some("measured.stp".to_owned()),
        Some("MILLIMETRE".to_owned()),
        Some("AP203".to_owned()),
        Some(1),
    )
}

/// The §22B-1a reference mesh: four control vertices at 0, 1000, 2000 and
/// 3000 mm along three axes, four triangles with distinguishable winding,
/// authored per-corner normals and two material slots.
pub fn asymmetric_mesh() -> ExportMesh {
    let red = ExportMaterial::new(
        "Ferrite Red",
        [0.603_827, 0.033_105, 0.010_023],
        ExportColourOrigin::Source,
    )
    .expect("a linear colour");
    let blue = ExportMaterial::new(
        "Ferrite Blue",
        [0.010_023, 0.100_482, 0.787_412],
        ExportColourOrigin::Source,
    )
    .expect("a linear colour");
    ExportMesh::new(
        vec![
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [0.0, 2000.0, 0.0],
            [0.0, 0.0, 3000.0],
        ],
        vec![
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [-1.0, 0.0, 0.0],
        ],
        vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        vec![0, 0, 1, 1],
        vec![red, blue],
    )
    .expect("the measured reference is a valid mesh")
}

/// A placement from a translation in millimetres and an XYZ Euler rotation in
/// degrees, in FerriteCAD axes.
pub fn placement(translation: [f64; 3], degrees: [f64; 3]) -> ExportTransform {
    let linear = euler_xyz(degrees);
    ExportTransform::new([
        [linear[0][0], linear[0][1], linear[0][2], translation[0]],
        [linear[1][0], linear[1][1], linear[1][2], translation[1]],
        [linear[2][0], linear[2][1], linear[2][2], translation[2]],
    ])
    .expect("a rotation and a translation are representable")
}

/// `Rz * Ry * Rx`, the one rotation order this project decomposes into.
pub fn euler_xyz(degrees: [f64; 3]) -> [[f64; 3]; 3] {
    let [x, y, z] = degrees.map(f64::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
        [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
        [-sy, cy * sx, cy * cx],
    ]
}

/// The measured scene: an assembly root, a transformed frame, two differently
/// placed instances of one two-slot geometry, one of them recoloured, an
/// omitted definition and four named control points below the first instance.
///
/// Every definition and every placement carries the durable identity a
/// current-layout document records, spelled out as fixed identifiers.
pub fn measured_scene() -> ExportScene {
    build_measured(Identify::Yes)
}

/// The same scene as a document written before identities would export it.
///
/// Held beside [`measured_scene`] rather than derived from it because the pair
/// is the measurement: §22B-1e3b changes exactly two properties per node and
/// nothing else, and only two scenes that differ in exactly the identities can
/// show that a legacy document still gets the file it always got.
pub fn legacy_scene() -> ExportScene {
    build_measured(Identify::No)
}

/// The measured scene with every designation changed and every durable
/// identity kept.
///
/// A neutral test variant, and its provenance is exactly that: it is built
/// here by naming different strings, and it is **not** a STEP reimport. This
/// build has no reimport semantics and nothing here pretends otherwise. What
/// it is for is the one question the channel exists to answer — after a
/// rename, does the identity still name the same placement — and the one
/// question it cannot answer, which is what the target program's own
/// references do when the name they were derived from moves.
pub fn renamed_scene() -> ExportScene {
    build_measured(Identify::Renamed)
}

/// Whether the built scene carries the durable identities a document recorded,
/// and whether the designations beside them are the measured ones.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Identify {
    Yes,
    No,
    /// The identities of [`Identify::Yes`] under different designations.
    Renamed,
}

/// What a definition or a placement is called in the scene being built.
fn called(name: &str, identify: Identify) -> Option<String> {
    Some(match identify {
        Identify::Renamed => format!("{name} renamed"),
        Identify::Yes | Identify::No => name.to_owned(),
    })
}

fn build_measured(identify: Identify) -> ExportScene {
    let source = source_of(MEASURED_SOURCE);
    let mut builder = ExportSceneBuilder::new();
    // One fixed identity per node, handed out in the order the nodes are
    // added. Not minted, and not derived from anything about the node: a fresh
    // identifier would make the writer's bytes different on every run, and a
    // derived one would be the very thing §22B-1e3a forbids.
    let mut next = 0usize;
    let mut occurrence = move || match identify {
        Identify::Yes | Identify::Renamed => {
            let at = next;
            next += 1;
            ExportOccurrence::Occurrence(occurrence_of(
                MEASURED_OCCURRENCES
                    .get(at)
                    .unwrap_or_else(|| panic!("the measured scene has more nodes than identities")),
            ))
        }
        Identify::No => ExportOccurrence::Unrecorded,
    };

    let root = builder
        .definition(
            imported(source, "step.product_definition#1"),
            identified(source, "step.product_definition#1", identify),
            called("Assembly Root", identify),
            provenance(),
            ExportGeometry::Structural,
        )
        .expect("a root frame");
    let frame = builder
        .definition(
            imported(source, "step.product_definition#7"),
            identified(source, "step.product_definition#7", identify),
            called("Assembly Frame", identify),
            provenance(),
            ExportGeometry::Structural,
        )
        .expect("an assembly frame");
    let part = builder
        .definition(
            imported(source, "step.product_definition#2428"),
            identified(source, "step.product_definition#2428", identify),
            called("Repeated Part", identify),
            provenance(),
            ExportGeometry::Mesh(asymmetric_mesh()),
        )
        .expect("the measured part");
    let missing = builder
        .definition(
            imported(source, "step.product_definition#2583"),
            identified(source, "step.product_definition#2583", identify),
            called("Omitted #2583", identify),
            provenance(),
            ExportGeometry::Omitted(ExportOmission::new(
                Diagnostic {
                    stage: Stage::Validation,
                    severity: Severity::Fail,
                    entity: "step.product_definition#2583".to_owned(),
                    message: "the imported definition contains an invalid solid".to_owned(),
                },
                TessellationRefusal::IncompleteFace,
            )),
        )
        .expect("the measured omission");
    let point = builder
        .definition(
            imported(source, "step.product_definition#9"),
            identified(source, "step.product_definition#9", identify),
            called("Control Point", identify),
            provenance(),
            ExportGeometry::Structural,
        )
        .expect("a control point frame");

    let root_node = builder
        .node(
            None,
            root,
            ExportTransform::IDENTITY,
            called("Assembly Root", identify),
            None,
            occurrence(),
        )
        .expect("the root");
    let frame_node = builder
        .node(
            Some(root_node),
            frame,
            placement([100.0, 200.0, 300.0], [11.0, 23.0, -17.0]),
            called("Assembly Frame", identify),
            None,
            occurrence(),
        )
        .expect("the frame");
    let first = builder
        .node(
            Some(frame_node),
            part,
            placement([1200.0, -400.0, 800.0], [31.0, -19.0, 47.0]),
            called("Repeated Part", identify),
            None,
            occurrence(),
        )
        .expect("the first placement");
    builder
        .node(
            Some(frame_node),
            part,
            placement([-700.0, 900.0, 1300.0], [-13.0, 29.0, -37.0]),
            // Deliberately the same display name, and deliberately recoloured.
            called("Repeated Part", identify),
            Some([0.216, 0.523, 0.052]),
            occurrence(),
        )
        .expect("the second placement");
    builder
        .node(
            Some(frame_node),
            missing,
            placement([400.0, 500.0, 600.0], [7.0, 13.0, 29.0]),
            called("Omitted #2583", identify),
            None,
            occurrence(),
        )
        .expect("the omitted placement");

    for (name, translation) in [
        ("CP Origin", [0.0, 0.0, 0.0]),
        ("CP X1000", [1000.0, 0.0, 0.0]),
        ("CP Y2000", [0.0, 2000.0, 0.0]),
        ("CP Z3000", [0.0, 0.0, 3000.0]),
    ] {
        builder
            .node(
                Some(first),
                point,
                placement(translation, [0.0, 0.0, 0.0]),
                called(name, identify),
                None,
                occurrence(),
            )
            .expect("a control point");
    }

    builder.finish().expect("the measured scene is complete")
}

/// Names that exercise the one FBX ASCII escaping rule: quotes, a backslash, a
/// tab, a line break, a carriage return, an empty name and non-ASCII UTF-8.
///
/// The keys are the ordinary ones this fixture has always used, so the only
/// thing §22B-1e3b changes about this file is the two properties it adds.
pub fn escaping_scene() -> ExportScene {
    named_scene(
        source_of(ESCAPING_SOURCE),
        &[
            (Some("a \"quoted\" name"), "step.product_definition#0"),
            (Some("back\\slash and\ttab"), "step.product_definition#1"),
            (Some("Кириллица и юникод — ok"), "step.product_definition#2"),
            (None, "step.product_definition#3"),
            (Some("line\nbreak and\rreturn"), "step.product_definition#4"),
        ],
    )
}

/// The §22B-1e3b escaping rule, on keys chosen to break it.
///
/// The value separator, the escape character itself, an already-escaped-looking
/// key, whitespace and a multi-byte code point. Held apart from
/// [`escaping_scene`] because these keys are also written to the properties
/// §22B-1b2 added and to the object names of definitions with no display name,
/// so putting them in that fixture would change a committed file for a reason
/// that has nothing to do with this slice. The two escaping rules still meet
/// here: the names are ordinary, and the keys are not.
pub fn identity_escaping_scene() -> ExportScene {
    named_scene(
        source_of(KEYED_SOURCE),
        &[
            (Some("Plain"), "step.product_definition#1"),
            (Some("Separators"), "a:b:c"),
            (Some("Unicode"), "ключ — 100%"),
            (Some("Already escaped"), "already%3Aescaped"),
            (Some("Whitespace"), "space and\ttab"),
        ],
    )
}

/// A scene of structural nodes, one per name, all below the first.
pub fn named_scene(source: ImportedSourceId, nodes: &[(Option<&str>, &str)]) -> ExportScene {
    let mut builder = ExportSceneBuilder::new();
    let mut parent = None;
    for (index, (name, key)) in nodes.iter().enumerate() {
        let definition = builder
            .definition(
                imported(source, key),
                identified(source, key, Identify::Yes),
                name.map(str::to_owned),
                ExportProvenance::default(),
                ExportGeometry::Structural,
            )
            .expect("a definition");
        let node = builder
            .node(
                parent,
                definition,
                ExportTransform::IDENTITY,
                name.map(str::to_owned),
                None,
                ExportOccurrence::Occurrence(occurrence_of(
                    ESCAPING_OCCURRENCES
                        .get(index)
                        .unwrap_or_else(|| panic!("a named scene of at most five nodes")),
                )),
            )
            .expect("a node");
        if parent.is_none() {
            parent = Some(node);
        }
    }
    builder.finish().expect("a scene of names")
}

// ------------------------------------------------------------------ reader

/// One node of a written file, for gates that ask what the writer produced.
#[derive(Debug, Clone, Default)]
pub struct FbxNode {
    pub name: String,
    pub props: Vec<String>,
    pub children: Vec<FbxNode>,
}

/// A whole written file.
#[derive(Debug, Clone, Default)]
pub struct Fbx {
    root: FbxNode,
}

impl Fbx {
    pub fn parse(text: &str) -> Self {
        let mut root = FbxNode::default();
        let mut stack: Vec<FbxNode> = Vec::new();
        let mut current = std::mem::take(&mut root);
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') {
                continue;
            }
            if trimmed == "}" {
                let finished = current;
                current = stack.pop().unwrap_or_else(|| panic!("unbalanced braces"));
                current.children.push(finished);
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix(',') {
                let last = current
                    .children
                    .last_mut()
                    .unwrap_or_else(|| panic!("a continuation with nothing to continue"));
                last.props.extend(tokens(rest));
                continue;
            }
            let (name, rest) = trimmed
                .split_once(':')
                .unwrap_or_else(|| panic!("not an FBX line: {trimmed}"));
            let block = rest.trim_end().ends_with('{');
            let rest = if block {
                rest.trim_end().trim_end_matches('{')
            } else {
                rest
            };
            let node = FbxNode {
                name: name.trim().to_owned(),
                props: tokens(rest),
                children: Vec::new(),
            };
            if block {
                stack.push(current);
                current = node;
            } else {
                current.children.push(node);
            }
        }
        assert!(stack.is_empty(), "unbalanced braces");
        Self { root: current }
    }

    pub fn top(&self) -> &FbxNode {
        &self.root
    }

    /// The one node at a slash-separated path.
    pub fn at(&self, path: &str) -> &FbxNode {
        let found = self.all(path);
        assert_eq!(found.len(), 1, "{path} matched {} nodes", found.len());
        found[0]
    }

    /// Every node at a slash-separated path, in file order.
    pub fn all(&self, path: &str) -> Vec<&FbxNode> {
        let mut level = vec![&self.root];
        for step in path.split('/') {
            let mut next = Vec::new();
            for node in level {
                next.extend(node.children.iter().filter(|child| child.name == step));
            }
            level = next;
        }
        level
    }

    fn connections(&self) -> Vec<(i64, i64)> {
        self.all("Connections/C")
            .iter()
            .map(|c| {
                assert_eq!(unquote(&c.props[0]), "OO");
                (number(&c.props[1]) as i64, number(&c.props[2]) as i64)
            })
            .collect()
    }

    /// Which model each model sits inside, `0` for the file root.
    pub fn parents(&self) -> BTreeMap<i64, i64> {
        let models: Vec<i64> = self
            .all("Objects/Model")
            .iter()
            .map(|m| m.object_id())
            .collect();
        self.connections()
            .into_iter()
            .filter(|(from, _)| models.contains(from))
            .collect()
    }

    pub fn connections_from(&self, id: i64) -> Vec<i64> {
        self.connections()
            .into_iter()
            .filter(|(from, _)| *from == id)
            .map(|(_, to)| to)
            .collect()
    }

    pub fn connections_to(&self, id: i64) -> Vec<i64> {
        self.connections()
            .into_iter()
            .filter(|(_, to)| *to == id)
            .map(|(from, _)| from)
            .collect()
    }

    pub fn materials_of(&self, model: i64) -> Vec<i64> {
        let materials: Vec<i64> = self
            .all("Objects/Material")
            .iter()
            .map(|m| m.object_id())
            .collect();
        self.connections_to(model)
            .into_iter()
            .filter(|from| materials.contains(from))
            .collect()
    }

    pub fn material(&self, id: i64) -> &FbxNode {
        self.all("Objects/Material")
            .into_iter()
            .find(|node| node.object_id() == id)
            .unwrap_or_else(|| panic!("no material {id}"))
    }

    pub fn is_geometry(&self, id: i64) -> bool {
        self.all("Objects/Geometry")
            .iter()
            .any(|node| node.object_id() == id)
    }
}

impl FbxNode {
    pub fn child(&self, name: &str) -> &FbxNode {
        let found: Vec<&FbxNode> = self.children.iter().filter(|c| c.name == name).collect();
        assert_eq!(found.len(), 1, "{name} matched {} children", found.len());
        found[0]
    }

    pub fn has_child(&self, name: &str) -> bool {
        self.children.iter().any(|c| c.name == name)
    }

    pub fn object_id(&self) -> i64 {
        number(&self.props[0]) as i64
    }

    /// The name half of an ASCII `Type::Name` property.
    pub fn object_name(&self) -> String {
        let full = unquote(&self.props[1]);
        match full.find("::") {
            Some(at) => full[at + 2..].to_owned(),
            None => full,
        }
    }

    pub fn class(&self) -> String {
        unquote(&self.props[2])
    }

    pub fn text(&self) -> String {
        unquote(&self.props[0])
    }

    pub fn number(&self) -> f64 {
        number(&self.props[0])
    }

    /// One `P` of a `Properties70` block, by the name it declares.
    pub fn property(&self, name: &str) -> &FbxNode {
        self.children
            .iter()
            .filter(|child| child.name == "P")
            .find(|child| unquote(&child.props[0]) == name)
            .unwrap_or_else(|| panic!("no property {name}"))
    }

    /// One property of an object that has a `Properties70` block.
    pub fn at_property(&self, name: &str) -> &FbxNode {
        self.child("Properties70").property(name)
    }

    /// Every property this object declares as user-defined, by name.
    pub fn user_properties(&self) -> BTreeMap<String, String> {
        let Some(properties) = self.children.iter().find(|c| c.name == "Properties70") else {
            return BTreeMap::new();
        };
        properties
            .children
            .iter()
            .filter(|child| child.name == "P")
            .filter(|child| unquote(&child.props[3]).contains('U'))
            .map(|child| {
                let value = child.props[4..]
                    .iter()
                    .map(|token| unquote(token))
                    .collect::<Vec<_>>()
                    .join(",");
                (unquote(&child.props[0]), value)
            })
            .collect()
    }

    /// The numbers this node carries: an array's payload, a property's values,
    /// or its own properties.
    pub fn numbers(&self) -> Vec<f64> {
        if let Some(array) = self.children.iter().find(|c| c.name == "a") {
            return array.props.iter().map(|token| number(token)).collect();
        }
        let values = if self.name == "P" {
            &self.props[4..]
        } else {
            &self.props[..]
        };
        values.iter().map(|token| number(token)).collect()
    }
}

/// Splits one line's property list, respecting quoted strings.
fn tokens(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in rest.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                current.push(ch);
            }
            ',' if !quoted => {
                push_token(&mut out, &mut current);
            }
            _ => current.push(ch),
        }
    }
    push_token(&mut out, &mut current);
    out
}

fn push_token(out: &mut Vec<String>, current: &mut String) {
    let token = current.trim().to_owned();
    current.clear();
    if !token.is_empty() {
        out.push(token);
    }
}

fn number(token: &str) -> f64 {
    token
        .trim_start_matches('*')
        .parse()
        .unwrap_or_else(|_| panic!("not a number: {token}"))
}

/// Undoes the one FBX ASCII escaping rule.
fn unquote(token: &str) -> String {
    let inner = token
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(token);
    inner
        .replace("&quot;", "\"")
        .replace("&cr;", "\r")
        .replace("&lf;", "\n")
}
