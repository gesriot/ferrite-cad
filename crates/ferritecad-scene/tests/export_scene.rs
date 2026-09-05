// SPDX-License-Identifier: MIT
//! What an interchange writer is entitled to be handed.
//!
//! Every gate here drives the production route: a real document on disk, one
//! read-only open, one cold rebuild, one reading of each stored STEP source
//! and one kernel session. Nothing fabricates an `ExportScene` and asks
//! whether it looks right.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use ferritecad_document::{
    Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Envelope, Expression,
    Extrude, IMPORTED_STEP_CAPABILITY, ImporterIdentity, ObjectPayload, Point2, Sketch,
    SketchCurve, SketchGeometry, SolidOperation, StepImportRequest,
};
use ferritecad_exchange::{
    ColourSource, Definition, Diagnostic, Import, Instance, KeyedInstance, KeyedScene,
    PersistedScene, Scene, Severity, Stage, StoredScene,
};
use ferritecad_export::{
    ExportColourOrigin, ExportGeometry, ExportOccurrence, ExportProvenance, ExportScene,
    ExportSceneBuilder, ExportSource, ExportTransform, TRANSFORM_TOLERANCE,
};
use ferritecad_kernel::mock::MockKernel;
use ferritecad_kernel::{
    ArchiveSlot, BrepBlob, CancelToken, ExtrudeExtent, ExtrudeRequest, ExtrudeResult,
    GeometryKernel, KernelIdentity, Mesh, OperationContext, OperationResult, PlanarPoint, Profile,
    ProfileLoop, ProfileSegment, SegmentGeometry, ShapeHandle, SketchPlane, SubShapeHandle,
    TessellationParams, TessellationRefusal,
};
use ferritecad_scene::{export_scene, snapshot_of};
use ferritecad_types::{
    CadError, ErrorKind, ObjectId, OccurrenceId, Result, StableEntityId, Transform, Vec3,
};

const SOURCE: &[u8] = b"ISO-10303-21; this is what the document stores";
const OTHER_SOURCE: &[u8] = b"ISO-10303-21; a different file entirely";

fn params() -> TessellationParams {
    TessellationParams::new(
        TessellationParams::DEFAULT_LINEAR,
        TessellationParams::DEFAULT_ANGULAR,
        false,
    )
    .expect("the defaults are valid")
}

fn no_imports<K: ?Sized>(_: &mut K, _: &[u8]) -> Result<Import> {
    Err(CadError::unsupported(
        "this document was supposed to hold no imports",
    ))
}

/// One mock solid, so a fabricated scene refers to geometry that exists.
fn solid(kernel: &mut MockKernel) -> ShapeHandle {
    let corners = [
        PlanarPoint::new(0.0, 0.0),
        PlanarPoint::new(10.0, 0.0),
        PlanarPoint::new(10.0, 10.0),
        PlanarPoint::new(0.0, 10.0),
    ]
    .map(|corner| corner.expect("finite"));
    let segments = corners
        .iter()
        .enumerate()
        .map(|(index, start)| {
            ProfileSegment::new(
                StableEntityId::new(),
                SegmentGeometry::line(*start, corners[(index + 1) % corners.len()])
                    .expect("distinct"),
            )
        })
        .collect();
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(segments).expect("closes"),
        Vec::new(),
    )
    .expect("valid");
    let request = ExtrudeRequest::new(
        profile,
        ExtrudeExtent::blind(10.0).expect("positive"),
        false,
    );
    kernel
        .extrude(&request, &OperationContext::default())
        .expect("the mock builds a solid")
        .shape
}

fn definition(kernel: &mut MockKernel, name: &str, solids: u32, key: &str) -> Definition {
    Definition {
        shape: solid(kernel),
        name: name.to_owned(),
        solids,
        key: key.to_owned(),
    }
}

fn placed(
    definition: usize,
    parent: Option<usize>,
    name: &str,
    placement: [f64; 12],
    colour_source: ColourSource,
    colour: [f64; 3],
) -> Instance {
    Instance {
        definition,
        parent,
        name: name.to_owned(),
        placement,
        colour_source,
        colour,
    }
}

fn at(offset: [f64; 3]) -> [f64; 12] {
    [
        1.0, 0.0, 0.0, offset[0], 0.0, 1.0, 0.0, offset[1], 0.0, 0.0, 1.0, offset[2],
    ]
}

/// The shape of `fixtures/step/canonical/03-nested-assembly.step`: two groups
/// of two cubes inside an outer group, every placement relative to its parent.
fn nested_assembly(kernel: &mut MockKernel) -> Scene {
    Scene {
        source_unit: "MILLIMETRE".to_owned(),
        schema: "AP214".to_owned(),
        definitions: vec![
            definition(kernel, "OuterGroup", 4, "step.product_definition#5"),
            definition(kernel, "InnerGroup", 2, "step.product_definition#31"),
            definition(kernel, "Cube", 1, "step.product_definition#58"),
        ],
        instances: vec![
            placed(0, None, "Outer", at([0.0; 3]), ColourSource::None, [0.0; 3]),
            placed(
                1,
                Some(0),
                "Left group",
                at([1.0, 2.0, 3.0]),
                ColourSource::None,
                [0.0; 3],
            ),
            placed(
                2,
                Some(1),
                "Cube",
                at([0.0; 3]),
                ColourSource::Definition,
                [0.1, 0.2, 0.3],
            ),
            placed(
                2,
                Some(1),
                "Cube",
                at([30.0, 0.0, 0.0]),
                ColourSource::Definition,
                [0.1, 0.2, 0.3],
            ),
            placed(
                1,
                Some(0),
                "Right group",
                at([0.0, 40.0, 0.0]),
                ColourSource::None,
                [0.0; 3],
            ),
            placed(
                2,
                Some(4),
                "Cube",
                at([0.0; 3]),
                ColourSource::Definition,
                [0.1, 0.2, 0.3],
            ),
            placed(
                2,
                Some(4),
                "Repainted cube",
                at([30.0, 0.0, 0.0]),
                ColourSource::Instance,
                [0.9, 0.1, 0.1],
            ),
        ],
    }
}

/// One flat assembly: a frame holding two differently placed parts.
fn flat_assembly(kernel: &mut MockKernel) -> Scene {
    Scene {
        source_unit: "MILLIMETRE".to_owned(),
        schema: "AP203".to_owned(),
        definitions: vec![
            definition(kernel, "Frame", 2, "step.product_definition#1"),
            definition(kernel, "Part", 1, "step.product_definition#17"),
        ],
        instances: vec![
            placed(0, None, "Frame", at([0.0; 3]), ColourSource::None, [0.0; 3]),
            placed(
                1,
                Some(0),
                "Repeated Part",
                at([5.0, 0.0, 0.0]),
                ColourSource::None,
                [0.0; 3],
            ),
            placed(
                1,
                Some(0),
                "Repeated Part",
                at([0.0, 7.0, 0.0]),
                ColourSource::None,
                [0.0; 3],
            ),
        ],
    }
}

/// One definition, one placement, under a key every file numbers alike.
fn one_part(kernel: &mut MockKernel, name: &str) -> Scene {
    Scene {
        source_unit: "MILLIMETRE".to_owned(),
        schema: "AP214".to_owned(),
        definitions: vec![definition(kernel, name, 1, "step.product_definition#5")],
        instances: vec![placed(
            0,
            None,
            name,
            at([0.0; 3]),
            ColourSource::None,
            [0.0; 3],
        )],
    }
}

/// One import as a test wants to store it.
struct Stored<'a> {
    object: ObjectId,
    name: &'a str,
    source: &'a [u8],
    source_name: &'a str,
    scene: Scene,
    diagnostics: Vec<Diagnostic>,
}

fn store_import(document: &mut Document, kernel: &mut MockKernel, stored: Stored<'_>) {
    let import = Import::Imported {
        scene: stored.scene,
        diagnostics: stored.diagnostics,
    };
    document
        .store_step_import(StepImportRequest {
            object: stored.object,
            name: Some(stored.name),
            source: stored.source,
            source_name: Some(stored.source_name),
            import: &import,
            importer: kernel.identity(),
        })
        .expect("stores the import");
    for shape in import.scene().expect("a scene was stored").shapes() {
        kernel.release(shape);
    }
}

/// A document holding one stored import of the nested assembly.
fn document_with_nested_assembly(path: &Path, kernel: &mut MockKernel) -> ObjectId {
    let object = ObjectId::new();
    let mut document = Document::create(path).expect("creates a document");
    let scene = nested_assembly(kernel);
    store_import(
        &mut document,
        kernel,
        Stored {
            object,
            name: "Assembly",
            source: SOURCE,
            source_name: "03-nested-assembly.step",
            scene,
            diagnostics: Vec::new(),
        },
    );
    object
}

/// A document of `count` separate square bodies, ten apart along x, every one
/// of them called the same thing.
fn several_bodies(path: &Path, count: usize) -> Vec<ObjectId> {
    let plane = ObjectId::new();
    let mut bodies = Vec::new();
    let mut document = Document::create(path).expect("creates a document");
    document
        .write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("XY"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            )?;
            for index in 0..count {
                let (sketch, extrude, body) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
                let left = index as f64 * 10.0;
                let corners = [
                    (left, 0.0),
                    (left + 5.0, 0.0),
                    (left + 5.0, 5.0),
                    (left, 5.0),
                ];
                let mut curves = Vec::new();
                for corner in 0..corners.len() {
                    let (sx, sy) = corners[corner];
                    let (ex, ey) = corners[(corner + 1) % corners.len()];
                    curves.push(SketchCurve {
                        id: StableEntityId::new(),
                        construction: false,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(sx, sy)?,
                            end: Point2::new(ex, ey)?,
                        },
                    });
                }
                let ordinal = index as i64 * 3;
                w.put_object(
                    sketch,
                    None,
                    ordinal + 1,
                    Some("Profile"),
                    &ObjectPayload::Sketch(Sketch {
                        plane,
                        curves,
                        constraints: Vec::new(),
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: sketch,
                    dependency: plane,
                    role: DependencyRole::Plane,
                })?;
                w.put_object(
                    extrude,
                    None,
                    ordinal + 2,
                    Some("Raise"),
                    &ObjectPayload::Extrude(Extrude {
                        profile: sketch,
                        end_condition: EndCondition::Blind {
                            distance: Expression::constant(2.0)?,
                        },
                        reversed: false,
                        operation: SolidOperation::NewBody,
                        target_body: None,
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: extrude,
                    dependency: sketch,
                    role: DependencyRole::Profile,
                })?;
                w.put_object(
                    body,
                    None,
                    ordinal + 3,
                    Some("Plate"),
                    &ObjectPayload::Body(Body {
                        tip_feature: Some(extrude),
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: body,
                    dependency: extrude,
                    role: DependencyRole::BodyTip,
                })?;
                bodies.push(body);
            }
            Ok(())
        })
        .expect("writes the document");
    bodies
}

fn validation_failure(entity: &str) -> Diagnostic {
    Diagnostic {
        stage: Stage::Validation,
        severity: Severity::Fail,
        entity: entity.to_owned(),
        message: "the imported definition contains an invalid solid".to_owned(),
    }
}

/// `Result::expect_err` without requiring the value to be `Debug`-printable
/// under a name that reads as a success.
trait UnwrapErrOrElse<T, E> {
    fn unwrap_err_or_else(self, missing: impl FnOnce() -> E) -> E;
}

impl<T, E> UnwrapErrOrElse<T, E> for std::result::Result<T, E> {
    fn unwrap_err_or_else(self, missing: impl FnOnce() -> E) -> E {
        match self {
            Ok(_) => missing(),
            Err(error) => error,
        }
    }
}

fn key_of(source: &ExportSource) -> &str {
    match source {
        ExportSource::Imported { definition_key, .. } => definition_key,
        ExportSource::Body { .. } => panic!("a native body was exported as an imported definition"),
    }
}

/// A compact, comparable description of everything an export claims.
fn described(scene: &ExportScene) -> String {
    let mut out = String::new();
    for definition in scene.definitions() {
        let geometry = match &definition.geometry {
            ExportGeometry::Mesh(mesh) => format!(
                "mesh v={} t={} m={}",
                mesh.vertex_count(),
                mesh.triangle_count(),
                mesh.materials().len()
            ),
            ExportGeometry::Structural => "structural".to_owned(),
            ExportGeometry::Omitted(omission) => {
                format!("omitted {} {:?}", omission.finding.entity, omission.refusal)
            }
        };
        out.push_str(&format!(
            "definition {} {:?} name={:?} provenance={:?} {geometry}\n",
            definition.id.index(),
            definition.source,
            definition.display_name,
            definition.provenance,
        ));
    }
    for node in scene.nodes() {
        out.push_str(&format!(
            "node {} parent={:?} definition={} name={:?} colour={:?} order={} rows={:?}\n",
            node.id.index(),
            node.parent.map(|parent| parent.index()),
            node.definition.index(),
            node.display_name,
            node.colour_override,
            node.order,
            node.local_transform.rows(),
        ));
    }
    for report in scene.completeness().omissions() {
        out.push_str(&format!(
            "omission {:?} {:?} nodes={:?}\n",
            report.source,
            report.omission.refusal,
            report
                .nodes
                .iter()
                .map(|node| node.index())
                .collect::<Vec<_>>()
        ));
    }
    out
}

// ---------------------------------------------------------------- native

#[test]
fn one_native_body_is_one_definition_one_node_and_one_mesh() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("one.fcad");
    let bodies = several_bodies(&path, 1);

    let mut kernel = MockKernel::new();
    let scene = export_scene(
        &path,
        &mut kernel,
        no_imports,
        &params(),
        &OperationContext::default(),
    )
    .expect("a native document exports");

    assert_eq!(scene.definitions().len(), 1, "one body is one definition");
    assert_eq!(scene.nodes().len(), 1, "one body is one node");
    assert_eq!(scene.roots().count(), 1);
    assert_eq!(
        scene.definitions()[0].source,
        ExportSource::Body { object: bodies[0] }
    );
    assert_eq!(
        scene.definitions()[0].display_name.as_deref(),
        Some("Plate")
    );
    let mesh = scene.definitions()[0]
        .geometry
        .mesh()
        .expect("a built body has triangles");
    assert!(mesh.triangle_count() > 0);
    assert_eq!(scene.nodes()[0].parent, None);
    assert_eq!(
        scene.nodes()[0].local_transform.rows(),
        Transform::IDENTITY.rows()
    );
    assert!(scene.completeness().is_complete());
    assert_eq!(kernel.live_shape_count(), 0, "the export leaked shapes");
}

#[test]
fn two_bodies_with_one_name_and_one_shape_are_two_definitions() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("two.fcad");
    let bodies = several_bodies(&path, 2);

    let mut kernel = MockKernel::new();
    let scene = export_scene(
        &path,
        &mut kernel,
        no_imports,
        &params(),
        &OperationContext::default(),
    )
    .expect("two bodies export");

    assert_eq!(scene.definitions().len(), 2);
    assert_eq!(scene.nodes().len(), 2);
    let sources: Vec<&ExportSource> = scene
        .definitions()
        .iter()
        .map(|definition| &definition.source)
        .collect();
    assert_eq!(
        sources,
        vec![
            &ExportSource::Body { object: bodies[0] },
            &ExportSource::Body { object: bodies[1] },
        ],
        "definitions follow document order"
    );
    for definition in scene.definitions() {
        assert_eq!(
            definition.display_name.as_deref(),
            Some("Plate"),
            "the document is entitled to call both the same thing"
        );
        assert!(definition.geometry.mesh().is_some());
    }
}

#[test]
fn sketches_datums_and_features_are_not_exported_as_nodes() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("features.fcad");
    let bodies = several_bodies(&path, 2);

    let document = Document::open_read_only(&path).expect("opens the document");
    let objects = document.objects().expect("reads the objects");
    assert!(
        objects.len() > bodies.len(),
        "the document must hold intermediate objects for this gate to mean anything"
    );
    drop(document);

    let mut kernel = MockKernel::new();
    let scene = export_scene(
        &path,
        &mut kernel,
        no_imports,
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");

    assert_eq!(scene.nodes().len(), 2, "only the bodies are model nodes");
    let exported: BTreeSet<ObjectId> = scene
        .definitions()
        .iter()
        .map(|definition| match definition.source {
            ExportSource::Body { object } => object,
            ExportSource::Imported { .. } => panic!("a native document exported an import"),
        })
        .collect();
    assert_eq!(exported, bodies.iter().copied().collect::<BTreeSet<_>>());
}

#[test]
fn a_body_with_nothing_built_into_it_is_not_exported_and_is_not_a_failure() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("empty-body.fcad");
    several_bodies(&path, 1);
    let mut document = Document::open(&path).expect("opens the document for writing");
    let empty = ObjectId::new();
    document
        .write(|w| {
            w.put_object(
                empty,
                None,
                100,
                Some("Not built yet"),
                &ObjectPayload::Body(Body { tip_feature: None }),
            )
        })
        .expect("adds an empty body");
    drop(document);

    let mut kernel = MockKernel::new();
    let scene = export_scene(
        &path,
        &mut kernel,
        no_imports,
        &params(),
        &OperationContext::default(),
    )
    .expect("an unbuilt body is not a failure");
    assert_eq!(scene.definitions().len(), 1);
    assert_eq!(scene.nodes().len(), 1);
}

#[test]
fn authored_normals_and_a_material_colour_survive() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("normals.fcad");
    several_bodies(&path, 1);

    let mut kernel = MockKernel::new();
    let scene = export_scene(
        &path,
        &mut kernel,
        no_imports,
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");

    let mesh = scene.definitions()[0]
        .geometry
        .mesh()
        .expect("a built body has triangles");
    assert_eq!(mesh.normals().len(), mesh.positions().len());
    assert_eq!(mesh.triangle_materials().len(), mesh.triangle_count());
    assert_eq!(mesh.materials().len(), 1);
    assert_eq!(mesh.materials()[0].origin, ExportColourOrigin::Default);

    // The mock's prism has a distinguishable normal per face rather than one
    // recalculated from the triangles, so this is the authored pattern.
    let distinct: BTreeSet<[u32; 3]> = mesh
        .normals()
        .iter()
        .map(|normal| normal.map(f32::to_bits))
        .collect();
    assert!(
        distinct.len() >= 4,
        "the exported normals collapsed to {} distinct values",
        distinct.len()
    );
    for normal in mesh.normals() {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        assert!((length - 1.0).abs() < 1.0e-5, "a normal was not authored");
    }
}

#[test]
fn one_document_exports_the_same_scene_twice() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("repeat.fcad");
    several_bodies(&path, 3);

    let mut kernel = MockKernel::new();
    let first = export_scene(
        &path,
        &mut kernel,
        no_imports,
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");
    let second = export_scene(
        &path,
        &mut kernel,
        no_imports,
        &params(),
        &OperationContext::default(),
    )
    .expect("exports again");
    assert_eq!(described(&first), described(&second));
    assert_eq!(first, second);
}

// ------------------------------------------------------- imported scenes

#[test]
fn a_flat_assembly_keeps_its_frame_and_both_local_placements() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("flat.fcad");
    let mut kernel = MockKernel::new();
    let object = ObjectId::new();
    {
        let mut document = Document::create(&path).expect("creates a document");
        let scene = flat_assembly(&mut kernel);
        store_import(
            &mut document,
            &mut kernel,
            Stored {
                object,
                name: "Flat",
                source: SOURCE,
                source_name: "02-flat-assembly.step",
                scene,
                diagnostics: Vec::new(),
            },
        );
    }

    let scene = export_scene(
        &path,
        &mut kernel,
        |kernel, bytes| {
            assert_eq!(bytes, SOURCE);
            Ok(Import::Imported {
                scene: flat_assembly(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("a flat assembly exports");

    assert_eq!(scene.definitions().len(), 2);
    assert_eq!(scene.nodes().len(), 3);
    assert_eq!(scene.roots().count(), 1);
    assert!(
        scene.definitions()[0].geometry.is_structural(),
        "the frame owns no geometry of its own"
    );
    assert!(scene.definitions()[1].geometry.mesh().is_some());

    let frame = scene.nodes()[0].id;
    assert_eq!(scene.nodes()[1].parent, Some(frame));
    assert_eq!(scene.nodes()[2].parent, Some(frame));
    assert_eq!(
        scene.nodes()[1].local_transform.translation(),
        [5.0, 0.0, 0.0]
    );
    assert_eq!(
        scene.nodes()[2].local_transform.translation(),
        [0.0, 7.0, 0.0]
    );
    assert_eq!(
        scene.nodes()[1].definition,
        scene.nodes()[2].definition,
        "two placements of one part share one definition"
    );
    assert_eq!(
        scene.nodes()[1].display_name,
        scene.nodes()[2].display_name,
        "equal display names are kept and merge nothing"
    );
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_nested_assembly_keeps_every_parent_and_exact_local_transform() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("nested.fcad");
    let mut kernel = MockKernel::new();
    document_with_nested_assembly(&path, &mut kernel);

    let scene = export_scene(
        &path,
        &mut kernel,
        |kernel, bytes| {
            assert_eq!(bytes, SOURCE);
            Ok(Import::Imported {
                scene: nested_assembly(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("a nested assembly exports");

    assert_eq!(scene.definitions().len(), 3);
    assert_eq!(scene.nodes().len(), 7);
    assert_eq!(scene.roots().count(), 1);

    let parents: Vec<Option<usize>> = scene
        .nodes()
        .iter()
        .map(|node| node.parent.map(|parent| parent.index()))
        .collect();
    assert_eq!(
        parents,
        vec![None, Some(0), Some(1), Some(1), Some(0), Some(4), Some(4)],
        "the assembly tree was flattened or reordered"
    );

    // Local, never accumulated: the inner group sits at [1,2,3] and the cube
    // below it at [30,0,0], so a world transform would read [31,2,3].
    assert_eq!(
        scene.nodes()[1].local_transform.translation(),
        [1.0, 2.0, 3.0]
    );
    assert_eq!(
        scene.nodes()[3].local_transform.translation(),
        [30.0, 0.0, 0.0]
    );

    let structural: Vec<bool> = scene
        .definitions()
        .iter()
        .map(|definition| definition.geometry.is_structural())
        .collect();
    assert_eq!(
        structural,
        vec![true, true, false],
        "an assembly frame must not carry the compound of what is inside it"
    );
    assert!(scene.completeness().is_complete());

    // The cube is one definition however many places it appears.
    let cube = scene.definitions()[2].id;
    assert_eq!(
        scene
            .nodes()
            .iter()
            .filter(|node| node.definition == cube)
            .count(),
        4
    );

    // Source-recorded colour lives on the definition; a repaint lives on the
    // node that carries it.
    let materials = scene.definitions()[2]
        .geometry
        .mesh()
        .expect("the cube has triangles")
        .materials();
    assert_eq!(materials.len(), 1);
    assert_eq!(materials[0].base_colour_linear, [0.1, 0.2, 0.3]);
    assert_eq!(materials[0].origin, ExportColourOrigin::Source);
    assert_eq!(scene.nodes()[5].colour_override, None);
    assert_eq!(scene.nodes()[6].colour_override, Some([0.9, 0.1, 0.1]));

    assert_eq!(
        scene.definitions()[2].provenance.source_unit.as_deref(),
        Some("MILLIMETRE")
    );
    assert_eq!(
        scene.definitions()[2].provenance.schema.as_deref(),
        Some("AP214")
    );
    assert_eq!(
        scene.definitions()[2].provenance.file_name.as_deref(),
        Some("03-nested-assembly.step")
    );
    assert_eq!(scene.definitions()[2].provenance.solids, Some(1));
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn the_render_snapshot_of_the_same_document_is_unchanged() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("both.fcad");
    let mut kernel = MockKernel::new();
    document_with_nested_assembly(&path, &mut kernel);

    let loaded = snapshot_of(
        &path,
        &mut kernel,
        |kernel, _| {
            Ok(Import::Imported {
                scene: nested_assembly(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("the picture still loads");

    // The picture flattens: one mesh, four world placements, and a catalogue
    // holding only what is drawn. The export keeps three definitions and seven
    // nodes. Both are correct answers to different questions.
    assert_eq!(loaded.snapshot.meshes().len(), 1);
    assert_eq!(loaded.snapshot.draws().len(), 4);
    assert_eq!(loaded.catalogue.len(), 1);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn two_objects_storing_the_same_bytes_share_definitions_and_keep_every_node() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("twice.fcad");
    let mut kernel = MockKernel::new();
    {
        let mut document = Document::create(&path).expect("creates a document");
        for name in ["left", "right"] {
            let scene = flat_assembly(&mut kernel);
            store_import(
                &mut document,
                &mut kernel,
                Stored {
                    object: ObjectId::new(),
                    name,
                    source: SOURCE,
                    source_name: &format!("{name}.step"),
                    scene,
                    diagnostics: Vec::new(),
                },
            );
        }
    }

    let reads = AtomicUsize::new(0);
    let scene = export_scene(
        &path,
        &mut kernel,
        |kernel, bytes| {
            reads.fetch_add(1, Ordering::Relaxed);
            assert_eq!(bytes, SOURCE);
            Ok(Import::Imported {
                scene: flat_assembly(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");

    assert_eq!(
        reads.load(Ordering::Relaxed),
        2,
        "each stored import is read exactly once"
    );
    assert_eq!(
        scene.definitions().len(),
        2,
        "identical bytes are one source, so they name the same definitions"
    );
    assert_eq!(
        scene.nodes().len(),
        6,
        "canonicalising definitions must not lose a placement"
    );
    assert_eq!(scene.roots().count(), 2);
}

#[test]
fn one_key_in_two_sources_stays_two_definitions() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("two-sources.fcad");
    let mut kernel = MockKernel::new();
    {
        let mut document = Document::create(&path).expect("creates a document");
        for (name, bytes) in [("left", SOURCE), ("right", OTHER_SOURCE)] {
            // The same definition name and the same source-local key in both.
            let scene = one_part(&mut kernel, "Bracket");
            store_import(
                &mut document,
                &mut kernel,
                Stored {
                    object: ObjectId::new(),
                    name,
                    source: bytes,
                    source_name: &format!("{name}.step"),
                    scene,
                    diagnostics: Vec::new(),
                },
            );
        }
    }

    let scene = export_scene(
        &path,
        &mut kernel,
        |kernel, _| {
            Ok(Import::Imported {
                scene: one_part(kernel, "Bracket"),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");

    assert_eq!(
        scene.definitions().len(),
        2,
        "`#5` in one file is not `#5` in another"
    );
    assert_eq!(
        key_of(&scene.definitions()[0].source),
        "step.product_definition#5"
    );
    assert_eq!(
        key_of(&scene.definitions()[1].source),
        "step.product_definition#5"
    );
    assert_ne!(scene.definitions()[0].source, scene.definitions()[1].source);
    assert_eq!(
        scene.definitions()[0].display_name,
        scene.definitions()[1].display_name,
        "one display name never merges two definitions"
    );
    assert_eq!(scene.nodes().len(), 2);
}

#[test]
fn deleting_the_source_file_before_exporting_changes_nothing() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("deleted.fcad");
    let external = directory.path().join("03-nested-assembly.step");
    std::fs::write(&external, SOURCE).expect("writes an external file");

    let mut kernel = MockKernel::new();
    document_with_nested_assembly(&path, &mut kernel);

    let read = |kernel: &mut MockKernel, bytes: &[u8]| {
        assert_eq!(
            bytes, SOURCE,
            "the export read something other than the stored bytes"
        );
        Ok(Import::Imported {
            scene: nested_assembly(kernel),
            diagnostics: Vec::new(),
        })
    };
    let before = export_scene(
        &path,
        &mut kernel,
        read,
        &params(),
        &OperationContext::default(),
    )
    .expect("exports while the file is there");

    std::fs::remove_file(&external).expect("removes the external file");
    assert!(!external.exists());
    let after = export_scene(
        &path,
        &mut kernel,
        read,
        &params(),
        &OperationContext::default(),
    )
    .expect("the stored bytes are the source");

    assert_eq!(described(&before), described(&after));
}

/// A second read-only open of the document, once the export has started, is
/// what this proves impossible: the file is unlinked while the export holds
/// it, so anything reopening it by path fails.
#[cfg(unix)]
#[test]
fn the_export_never_opens_the_document_a_second_time() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("unlinked.fcad");
    let mut kernel = MockKernel::new();
    document_with_nested_assembly(&path, &mut kernel);

    let unlinked = std::cell::Cell::new(false);
    let scene = export_scene(
        &path,
        &mut kernel,
        |kernel, _| {
            if !unlinked.get() {
                std::fs::remove_file(&path).expect("unlinks the open document");
                unlinked.set(true);
            }
            Ok(Import::Imported {
                scene: nested_assembly(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("one open is enough to finish the export");

    assert!(unlinked.get(), "the gate never removed the document");
    assert!(!path.exists());
    assert_eq!(scene.nodes().len(), 7);
}

// ------------------------------------------------------------ structure

#[test]
fn a_structural_definition_is_not_reported_as_an_omission() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("structure.fcad");
    let mut kernel = MockKernel::new();
    document_with_nested_assembly(&path, &mut kernel);

    let scene = export_scene(
        &path,
        &mut kernel,
        |kernel, _| {
            Ok(Import::Imported {
                scene: nested_assembly(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");

    assert_eq!(
        scene
            .definitions()
            .iter()
            .filter(|definition| definition.geometry.is_structural())
            .count(),
        2
    );
    assert!(
        scene.completeness().is_complete(),
        "an assembly frame is structure, not a missing part"
    );
    for definition in scene.definitions() {
        assert!(
            definition.geometry.omission().is_none(),
            "structural emptiness became an omission"
        );
    }
}

// ------------------------------------------------------- omission policy

#[derive(Clone, Copy)]
enum MeshAnswer {
    Typed,
    Unrelated,
    Empty,
}

/// A conforming kernel whose only special behaviour is refusing one mesh.
struct RefusesMesh {
    inner: MockKernel,
    answer: MeshAnswer,
    refuse_after: usize,
    tessellations: usize,
}

impl RefusesMesh {
    fn new(answer: MeshAnswer, refuse_after: usize) -> Self {
        Self {
            inner: MockKernel::new(),
            answer,
            refuse_after,
            tessellations: 0,
        }
    }
}

impl GeometryKernel for RefusesMesh {
    fn identity(&self) -> &KernelIdentity {
        self.inner.identity()
    }

    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<ExtrudeResult> {
        self.inner.extrude(request, context)
    }

    fn transform(
        &mut self,
        shape: ShapeHandle,
        transform: &Transform,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.inner.transform(shape, transform, context)
    }

    fn tessellate(
        &mut self,
        shape: ShapeHandle,
        params: &TessellationParams,
        context: &OperationContext,
    ) -> Result<Mesh> {
        self.tessellations += 1;
        if self.tessellations <= self.refuse_after {
            return self.inner.tessellate(shape, params, context);
        }
        match self.answer {
            MeshAnswer::Typed => Err(CadError::kernel_because(
                "the current kernel found an incomplete face",
                TessellationRefusal::IncompleteFace,
            )),
            MeshAnswer::Unrelated => Err(CadError::kernel("the kernel ran out of memory")),
            MeshAnswer::Empty => Ok(Mesh::default()),
        }
    }

    fn encode_shape_with(
        &mut self,
        shape: ShapeHandle,
        sub_shapes: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
        self.inner.encode_shape_with(shape, sub_shapes)
    }

    fn decode_shape_with(
        &mut self,
        blob: &BrepBlob,
        slots: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        self.inner.decode_shape_with(blob, slots)
    }

    fn encode_shape(&mut self, shape: ShapeHandle) -> Result<BrepBlob> {
        self.inner.encode_shape(shape)
    }

    fn decode_shape(&mut self, blob: &BrepBlob) -> Result<ShapeHandle> {
        self.inner.decode_shape(blob)
    }

    fn release(&mut self, shape: ShapeHandle) {
        self.inner.release(shape);
    }
}

const REFUSED_KEY: &str = "step.product_definition#5";

fn export_refused_part(
    answer: MeshAnswer,
    persisted: bool,
    fresh: bool,
) -> (Result<ExportScene>, usize) {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("retained.fcad");

    let mut storing = MockKernel::new();
    {
        let mut document = Document::create(&path).expect("creates the document");
        let scene = one_part(&mut storing, "Retained part");
        store_import(
            &mut document,
            &mut storing,
            Stored {
                object: ObjectId::new(),
                name: "Retained part",
                source: SOURCE,
                source_name: "retained.step",
                scene,
                diagnostics: persisted
                    .then(|| validation_failure(REFUSED_KEY))
                    .into_iter()
                    .collect(),
            },
        );
    }

    let mut kernel = RefusesMesh::new(answer, 0);
    let result = export_scene(
        &path,
        &mut kernel,
        |kernel: &mut RefusesMesh, bytes: &[u8]| {
            assert_eq!(bytes, SOURCE);
            Ok(Import::Imported {
                scene: one_part(&mut kernel.inner, "Retained part"),
                diagnostics: fresh
                    .then(|| validation_failure(REFUSED_KEY))
                    .into_iter()
                    .collect(),
            })
        },
        &params(),
        &OperationContext::default(),
    );
    let live = kernel.inner.live_shape_count();
    (result, live)
}

#[test]
fn matching_persisted_and_fresh_evidence_permits_the_known_omission() {
    let (result, live) = export_refused_part(MeshAnswer::Typed, true, true);
    let scene = result.expect("both observations confirm the typed current refusal");

    assert_eq!(live, 0);
    assert_eq!(scene.definitions().len(), 1);
    assert_eq!(
        scene.nodes().len(),
        1,
        "an omitted part keeps its placement"
    );
    let omission = scene.definitions()[0]
        .geometry
        .omission()
        .expect("the retained definition is visibly an omission");
    assert_eq!(omission.finding.entity, REFUSED_KEY);
    assert_eq!(omission.finding.stage, Stage::Validation);
    assert_eq!(omission.finding.severity, Severity::Fail);
    assert_eq!(omission.refusal, TessellationRefusal::IncompleteFace);
    assert!(scene.definitions()[0].geometry.mesh().is_none());

    let reports = scene.completeness().omissions();
    assert_eq!(reports.len(), 1);
    assert!(!scene.completeness().is_complete());
    assert_eq!(key_of(&reports[0].source), REFUSED_KEY);
    assert_eq!(reports[0].nodes, vec![scene.nodes()[0].id]);
}

#[test]
fn a_placement_of_a_definition_with_no_triangles_still_has_its_stored_identity() {
    // The §22B-1c boundary from the other side. A definition this build cannot
    // mesh is still placed, and its placements are held to exactly the rule
    // every other placement is: dropping them would make a partial export look
    // like a smaller complete one, and would slide every identity after them
    // onto the wrong place.
    let (result, live) = export_refused_part(MeshAnswer::Typed, true, true);
    let scene = result.expect("both observations confirm the typed current refusal");

    assert_eq!(live, 0);
    assert!(
        scene.definitions()[0].geometry.omission().is_some(),
        "this gate is about a definition with no triangles, and this one has some"
    );
    assert_eq!(
        scene.nodes().len(),
        1,
        "the placement of an omitted definition was dropped"
    );
    assert!(
        scene.nodes()[0].occurrence.is_recorded(),
        "the placement of a definition this build cannot mesh reached the export boundary \
         without an identity"
    );
}

#[test]
fn a_current_refusal_without_matching_persisted_evidence_stops_the_build() {
    for (persisted, fresh, missing) in [
        (true, false, "fresh validation"),
        (false, true, "persisted validation"),
        (false, false, "both validation observations"),
    ] {
        let (result, live) = export_refused_part(MeshAnswer::Typed, persisted, fresh);
        let error = result.expect_err(missing);
        assert_eq!(
            TessellationRefusal::of(&error),
            Some(&TessellationRefusal::IncompleteFace),
            "missing {missing} changed the underlying refusal"
        );
        assert_eq!(live, 0, "a refused export leaked shapes");
    }
}

#[test]
fn an_unrelated_kernel_failure_stops_the_build_even_with_both_observations() {
    let (result, live) = export_refused_part(MeshAnswer::Unrelated, true, true);
    let error = result.expect_err("an unrelated failure is not a known omission");
    assert_eq!(error.kind(), ErrorKind::Kernel);
    assert_eq!(TessellationRefusal::of(&error), None);
    assert_eq!(live, 0);
}

#[test]
fn a_triangle_free_mesh_is_not_itself_an_omission() {
    let (result, _) = export_refused_part(MeshAnswer::Empty, true, true);
    let error = result.expect_err(
        "a kernel that succeeded with no triangles produced no geometry to export and no \
         omission to report",
    );
    assert_eq!(error.kind(), ErrorKind::Input);
}

// ------------------------------------------------------------ transforms

fn export_with_placement(placement: [f64; 12]) -> Result<ExportScene> {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("placement.fcad");
    let mut kernel = MockKernel::new();
    let build = |kernel: &mut MockKernel| {
        let mut scene = one_part(kernel, "Part");
        scene.instances[0].placement = placement;
        scene
    };
    {
        let mut document = Document::create(&path).expect("creates a document");
        let scene = build(&mut kernel);
        store_import(
            &mut document,
            &mut kernel,
            Stored {
                object: ObjectId::new(),
                name: "Part",
                source: SOURCE,
                source_name: "part.step",
                scene,
                diagnostics: Vec::new(),
            },
        );
    }
    let result = export_scene(
        &path,
        &mut kernel,
        |kernel, _| {
            Ok(Import::Imported {
                scene: build(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    );
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a refused export leaked shapes"
    );
    result
}

#[test]
fn a_representable_placement_is_kept_exactly() {
    let rotation = Transform::from_rotation(Vec3::Z, std::f64::consts::FRAC_PI_3)
        .expect("a quarter turn is a rotation");
    let rows = rotation.rows();
    let placement = [
        rows[0][0], rows[0][1], rows[0][2], 11.0, rows[1][0], rows[1][1], rows[1][2], 12.0,
        rows[2][0], rows[2][1], rows[2][2], 13.0,
    ];
    let scene = export_with_placement(placement).expect("a rotation is representable");
    let kept = scene.nodes()[0].local_transform.rows();
    for (row, expected) in kept.iter().zip(placement.chunks_exact(4)) {
        assert_eq!(
            row.as_slice(),
            expected,
            "the placement was not kept exactly"
        );
    }
}

#[test]
fn a_placement_no_static_mesh_format_can_express_is_a_typed_refusal() {
    // Every one of these is finite, so a document stores it without complaint
    // and the export is the first thing that has to have an opinion.
    let cases: [(&str, [f64; 12]); 4] = [
        (
            "singular",
            [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        ),
        (
            "sheared",
            [1.0, 0.5, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        ),
        (
            "reflected",
            [-1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        ),
        (
            "non-uniform",
            [2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        ),
    ];

    for (what, placement) in cases {
        let error = export_with_placement(placement)
            .unwrap_err_or_else(|| panic!("a {what} placement must not be silently repaired"));
        assert_eq!(
            error.kind(),
            ErrorKind::Unsupported,
            "a {what} placement was refused as something other than unsupported"
        );
    }
}

/// The other half of the same rule: a non-finite placement never reaches an
/// export, because the document refuses to store one.
#[test]
fn a_non_finite_placement_is_refused_before_it_can_be_stored() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("non-finite.fcad");
    let mut kernel = MockKernel::new();
    let mut document = Document::create(&path).expect("creates a document");
    let mut scene = one_part(&mut kernel, "Part");
    scene.instances[0].placement[3] = f64::INFINITY;
    let import = Import::Imported {
        scene,
        diagnostics: Vec::new(),
    };
    let refused = document.store_step_import(StepImportRequest {
        object: ObjectId::new(),
        name: Some("Part"),
        source: SOURCE,
        source_name: Some("part.step"),
        import: &import,
        importer: kernel.identity(),
    });
    for shape in import.scene().expect("a scene was built").shapes() {
        kernel.release(shape);
    }
    assert!(
        refused.is_err(),
        "a placement that is not a position must not become a stored scene"
    );
}

#[test]
fn the_transform_tolerance_is_the_one_the_corpus_was_measured_with() {
    assert_eq!(TRANSFORM_TOLERANCE, 1.0e-10);
}

// ---------------------------------------------------------- cancellation

#[test]
fn cancelling_produces_no_partial_scene_and_leaks_no_shapes() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("cancelled.fcad");
    let mut kernel = MockKernel::new();
    document_with_nested_assembly(&path, &mut kernel);

    let cancel = CancelToken::new();
    let context = OperationContext::default().with_cancel(cancel.clone());
    let error = export_scene(
        &path,
        &mut kernel,
        |kernel, _| {
            let scene = nested_assembly(kernel);
            cancel.cancel();
            Ok(Import::Imported {
                scene,
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &context,
    )
    .expect_err("a cancelled export produces nothing");

    assert_eq!(error.kind(), ErrorKind::Cancellation);
    assert_eq!(kernel.live_shape_count(), 0, "cancellation leaked shapes");
}

// --------------------------------------------------- counting the work

/// Counts the calls a second solve, a second read or a second tessellation
/// would make.
struct Counting {
    inner: MockKernel,
    extrusions: usize,
    tessellations: BTreeMap<u64, usize>,
}

impl Counting {
    fn new() -> Self {
        Self {
            inner: MockKernel::new(),
            extrusions: 0,
            tessellations: BTreeMap::new(),
        }
    }
}

impl GeometryKernel for Counting {
    fn identity(&self) -> &KernelIdentity {
        self.inner.identity()
    }

    fn extrude(
        &mut self,
        request: &ExtrudeRequest,
        context: &OperationContext,
    ) -> Result<ExtrudeResult> {
        self.extrusions += 1;
        self.inner.extrude(request, context)
    }

    fn transform(
        &mut self,
        shape: ShapeHandle,
        transform: &Transform,
        context: &OperationContext,
    ) -> Result<OperationResult> {
        self.inner.transform(shape, transform, context)
    }

    fn tessellate(
        &mut self,
        shape: ShapeHandle,
        params: &TessellationParams,
        context: &OperationContext,
    ) -> Result<Mesh> {
        *self.tessellations.entry(shape.index()).or_default() += 1;
        self.inner.tessellate(shape, params, context)
    }

    fn encode_shape_with(
        &mut self,
        shape: ShapeHandle,
        sub_shapes: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
        self.inner.encode_shape_with(shape, sub_shapes)
    }

    fn decode_shape_with(
        &mut self,
        blob: &BrepBlob,
        slots: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        self.inner.decode_shape_with(blob, slots)
    }

    fn encode_shape(&mut self, shape: ShapeHandle) -> Result<BrepBlob> {
        self.inner.encode_shape(shape)
    }

    fn decode_shape(&mut self, blob: &BrepBlob) -> Result<ShapeHandle> {
        self.inner.decode_shape(blob)
    }

    fn release(&mut self, shape: ShapeHandle) {
        self.inner.release(shape);
    }
}

#[test]
fn one_export_solves_once_reads_each_source_once_and_meshes_each_definition_once() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("counted.fcad");
    let mut setup = MockKernel::new();
    document_with_nested_assembly(&path, &mut setup);
    // And a native body beside it, so the rebuild has something to do.
    {
        let mut document = Document::open(&path).expect("opens for writing");
        let plane = ObjectId::new();
        let (sketch, extrude, body) = (ObjectId::new(), ObjectId::new(), ObjectId::new());
        document
            .write(|w| {
                w.put_object(
                    plane,
                    None,
                    10,
                    Some("XY"),
                    &ObjectPayload::DatumPlane(DatumPlane {
                        placement: Transform::IDENTITY,
                    }),
                )?;
                let corners = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
                let mut curves = Vec::new();
                for corner in 0..corners.len() {
                    let (sx, sy) = corners[corner];
                    let (ex, ey) = corners[(corner + 1) % corners.len()];
                    curves.push(SketchCurve {
                        id: StableEntityId::new(),
                        construction: false,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(sx, sy)?,
                            end: Point2::new(ex, ey)?,
                        },
                    });
                }
                w.put_object(
                    sketch,
                    None,
                    11,
                    None,
                    &ObjectPayload::Sketch(Sketch {
                        plane,
                        curves,
                        constraints: Vec::new(),
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: sketch,
                    dependency: plane,
                    role: DependencyRole::Plane,
                })?;
                w.put_object(
                    extrude,
                    None,
                    12,
                    None,
                    &ObjectPayload::Extrude(Extrude {
                        profile: sketch,
                        end_condition: EndCondition::Blind {
                            distance: Expression::constant(2.0)?,
                        },
                        reversed: false,
                        operation: SolidOperation::NewBody,
                        target_body: None,
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: extrude,
                    dependency: sketch,
                    role: DependencyRole::Profile,
                })?;
                w.put_object(
                    body,
                    None,
                    13,
                    Some("Native"),
                    &ObjectPayload::Body(Body {
                        tip_feature: Some(extrude),
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: body,
                    dependency: extrude,
                    role: DependencyRole::BodyTip,
                })?;
                Ok(())
            })
            .expect("adds a native body");
    }

    let mut kernel = Counting::new();
    let reads = AtomicUsize::new(0);
    let scene = export_scene(
        &path,
        &mut kernel,
        |kernel: &mut Counting, _: &[u8]| {
            reads.fetch_add(1, Ordering::Relaxed);
            Ok(Import::Imported {
                scene: nested_assembly(&mut kernel.inner),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");

    assert_eq!(
        reads.load(Ordering::Relaxed),
        1,
        "the one stored STEP source was read more than once"
    );
    assert_eq!(
        kernel.extrusions, 1,
        "one cold rebuild builds the one native feature once"
    );
    for (shape, count) in &kernel.tessellations {
        assert_eq!(
            *count, 1,
            "shape {shape} was tessellated {count} times; a definition is meshed once however \
             many nodes reference it"
        );
    }
    assert_eq!(
        kernel.tessellations.len(),
        2,
        "one native body and one leaf definition carry geometry"
    );
    assert_eq!(scene.nodes().len(), 8);
    assert_eq!(scene.definitions().len(), 4);
    assert_eq!(kernel.inner.live_shape_count(), 0);
}

// --------------------------------------------- occurrence identity

/// Every node's durable identity, in scene order.
fn occurrences(scene: &ExportScene) -> Vec<ExportOccurrence> {
    scene.nodes().iter().map(|node| node.occurrence).collect()
}

/// The recorded identities of a scene, refusing one that has none.
fn recorded(scene: &ExportScene) -> Vec<ExportOccurrence> {
    let all = occurrences(scene);
    assert!(
        all.iter().copied().all(ExportOccurrence::is_recorded),
        "a placement reached the export boundary without an identity: {all:?}"
    );
    all
}

/// One cold export of a document holding the nested assembly, in a session of
/// its own.
fn export_nested(path: &Path) -> ExportScene {
    let mut kernel = MockKernel::new();
    let scene = export_scene(
        path,
        &mut kernel,
        |kernel, _| {
            Ok(Import::Imported {
                scene: nested_assembly(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");
    assert_eq!(kernel.live_shape_count(), 0);
    scene
}

#[test]
fn two_placements_of_one_definition_are_two_identities_on_one_shared_mesh() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("shared.fcad");
    let mut kernel = MockKernel::new();
    let object = ObjectId::new();
    let mut document = Document::create(&path).expect("creates a document");
    let scene = flat_assembly(&mut kernel);
    store_import(
        &mut document,
        &mut kernel,
        Stored {
            object,
            name: "Flat",
            source: SOURCE,
            source_name: "02-flat-assembly.step",
            scene,
            diagnostics: Vec::new(),
        },
    );
    document.close().expect("closes");

    let exported = export_scene(
        &path,
        &mut kernel,
        |kernel, _| {
            Ok(Import::Imported {
                scene: flat_assembly(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");

    // The two placements agree about everything the file records: the same
    // definition, the same parent, the same display name. Only the transform
    // and the identity differ, and the identity is the half that would still
    // tell them apart if the transforms were equal too.
    let places: Vec<&_> = exported
        .nodes()
        .iter()
        .filter(|node| node.display_name.as_deref() == Some("Repeated Part"))
        .collect();
    assert_eq!(places.len(), 2, "the shared part is not placed twice");
    assert_eq!(
        places[0].definition, places[1].definition,
        "the two placements stopped sharing their definition"
    );
    assert_ne!(
        places[0].occurrence, places[1].occurrence,
        "two placements of one definition answer to one identity"
    );

    // And they still share one mesh: identity per placement must not have
    // turned one definition into two.
    assert_eq!(
        exported.definitions().len(),
        2,
        "a definition was duplicated"
    );
    let shared = exported
        .definition(places[0].definition)
        .expect("the placed definition");
    assert!(matches!(shared.geometry, ExportGeometry::Mesh(_)));

    let all = recorded(&exported);
    assert_eq!(all.len(), 3);
    assert_eq!(
        all.iter().collect::<BTreeSet<_>>().len(),
        3,
        "two of three placements share an identity"
    );
}

#[test]
fn identical_names_transforms_and_keys_do_not_collapse_two_placements() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("twins.fcad");
    let mut kernel = MockKernel::new();

    // Two placements a source file cannot tell apart at all: same definition,
    // same parent, same name, same transform, same colour.
    let twins = |kernel: &mut MockKernel| {
        let mut scene = flat_assembly(kernel);
        scene.instances[2].placement = scene.instances[1].placement;
        scene
    };

    let object = ObjectId::new();
    let mut document = Document::create(&path).expect("creates a document");
    let scene = twins(&mut kernel);
    store_import(
        &mut document,
        &mut kernel,
        Stored {
            object,
            name: "Twins",
            source: SOURCE,
            source_name: "twins.step",
            scene,
            diagnostics: Vec::new(),
        },
    );
    document.close().expect("closes");

    let exported = export_scene(
        &path,
        &mut kernel,
        |kernel, _| {
            Ok(Import::Imported {
                scene: twins(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");

    assert_eq!(exported.nodes().len(), 3, "a placement was collapsed away");
    let twins: Vec<&_> = exported
        .nodes()
        .iter()
        .filter(|node| node.display_name.as_deref() == Some("Repeated Part"))
        .collect();
    assert_eq!(twins.len(), 2);
    assert_eq!(twins[0].display_name, twins[1].display_name);
    assert_eq!(
        twins[0].local_transform.rows(),
        twins[1].local_transform.rows()
    );
    assert_eq!(twins[0].definition, twins[1].definition);
    assert_ne!(
        twins[0].occurrence, twins[1].occurrence,
        "two indistinguishable placements were given one identity"
    );
}

#[test]
fn the_same_identities_come_back_from_every_cold_rebuild_and_every_session() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("stable.fcad");
    let mut setup = MockKernel::new();
    document_with_nested_assembly(&path, &mut setup);

    let first = recorded(&export_nested(&path));
    assert_eq!(first.len(), 7);
    assert_eq!(
        first.iter().collect::<BTreeSet<_>>().len(),
        7,
        "two placements of one document share an identity"
    );

    // Twice more in the same session, and twice in sessions of their own. An
    // identity minted at open, at rebuild or at export would move in at least
    // one of these.
    let mut kernel = MockKernel::new();
    for _ in 0..2 {
        let again = export_scene(
            &path,
            &mut kernel,
            |kernel, _| {
                Ok(Import::Imported {
                    scene: nested_assembly(kernel),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect("exports");
        assert_eq!(
            recorded(&again),
            first,
            "a repeated cold rebuild moved an identity"
        );
    }
    assert_eq!(
        recorded(&export_nested(&path)),
        first,
        "another kernel session read different identities"
    );
    assert_eq!(recorded(&export_nested(&path)), first);
}

#[test]
fn the_identity_of_each_node_is_the_one_stored_for_that_place() {
    // The strongest form of the claim, and the one that catches a shift or a
    // swap: not merely that the identities are stable and distinct, but that
    // node `n` carries the identity the document stored for placement `n`. A
    // reader that rotated them would be stable, distinct and wrong.
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("aligned.fcad");
    let mut setup = MockKernel::new();
    let object = ObjectId::new();
    let mut document = Document::create(&path).expect("creates a document");
    let scene = nested_assembly(&mut setup);
    store_import(
        &mut document,
        &mut setup,
        Stored {
            object,
            name: "Assembly",
            source: SOURCE,
            source_name: "03-nested-assembly.step",
            scene,
            diagnostics: Vec::new(),
        },
    );
    document.close().expect("closes");

    let expected = {
        let document = Document::open_read_only(&path).expect("opens read-only");
        let record = document.object(object).expect("reads").expect("is there");
        let ObjectPayload::ImportedStep(stored) = &record.payload else {
            panic!("the object under test is an imported STEP file");
        };
        let StoredScene::V3(stored) = &stored.scene else {
            panic!("a fresh import stores a current-layout scene");
        };
        stored
            .instances
            .iter()
            .map(|instance| ExportOccurrence::Occurrence(instance.occurrence))
            .collect::<Vec<_>>()
    };
    assert_eq!(expected.len(), 7);

    assert_eq!(
        occurrences(&export_nested(&path)),
        expected,
        "the identity a node carries is not the one the document stored for that place"
    );
}

#[test]
fn a_fresh_reading_of_the_same_bytes_does_not_replace_the_stored_identities() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("fresh.fcad");
    let mut setup = MockKernel::new();
    document_with_nested_assembly(&path, &mut setup);
    let stored = recorded(&export_nested(&path));

    // A second document holding the very same bytes, imported separately. Its
    // placements are different places, so they are different identities — and
    // the first document's identities are unmoved by the second existing.
    let other_path = directory.path().join("fresh-again.fcad");
    let mut other_setup = MockKernel::new();
    document_with_nested_assembly(&other_path, &mut other_setup);
    let other = recorded(&export_nested(&other_path));

    assert_eq!(other.len(), stored.len());
    assert!(
        other.iter().all(|occurrence| !stored.contains(occurrence)),
        "two documents holding the same bytes were given the same placement identities, so \\
         the identity is derived from the bytes rather than owned by the document"
    );
    assert_eq!(
        recorded(&export_nested(&path)),
        stored,
        "reading the second document moved the first document's identities"
    );
}

#[test]
fn two_objects_storing_the_same_bytes_keep_their_own_placement_identities() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("twice.fcad");
    let mut kernel = MockKernel::new();
    let mut document = Document::create(&path).expect("creates a document");
    for (object, name) in [(ObjectId::new(), "First"), (ObjectId::new(), "Second")] {
        let scene = nested_assembly(&mut kernel);
        store_import(
            &mut document,
            &mut kernel,
            Stored {
                object,
                name,
                source: SOURCE,
                source_name: "03-nested-assembly.step",
                scene,
                diagnostics: Vec::new(),
            },
        );
    }
    document.close().expect("closes");

    let exported = export_nested(&path);
    // One copy of the bytes, one set of definitions, and two sets of places.
    assert_eq!(
        exported.definitions().len(),
        3,
        "two objects storing the same bytes stopped sharing their definitions"
    );
    assert_eq!(exported.nodes().len(), 14);
    let all = recorded(&exported);
    assert_eq!(
        all.iter().collect::<BTreeSet<_>>().len(),
        14,
        "two objects storing the same bytes shared a placement identity"
    );
}

#[test]
fn a_native_body_is_identified_by_its_object_and_not_by_a_fresh_uuid() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("bodies.fcad");
    let bodies = several_bodies(&path, 2);

    let export = || {
        let mut kernel = MockKernel::new();
        export_scene(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("exports")
    };

    let scene = export();
    assert_eq!(scene.nodes().len(), 2);
    assert_eq!(
        occurrences(&scene),
        bodies
            .iter()
            .map(|body| ExportOccurrence::Object(*body))
            .collect::<Vec<_>>(),
        "a native body is identified by something other than the object that holds it"
    );

    // And it is the same next time. A body given an OccurrenceId of its own
    // would need one minted somewhere, and the only place that could happen is
    // here, once per export.
    assert_eq!(occurrences(&export()), occurrences(&scene));
}

#[test]
fn a_document_written_before_placements_had_identities_says_so_and_still_exports() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("legacy.fcad");
    let mut kernel = MockKernel::new();
    let object = ObjectId::new();
    let mut document = Document::create(&path).expect("creates a document");
    let scene = nested_assembly(&mut kernel);
    store_import(
        &mut document,
        &mut kernel,
        Stored {
            object,
            name: "Assembly",
            source: SOURCE,
            source_name: "03-nested-assembly.step",
            scene,
            diagnostics: Vec::new(),
        },
    );
    document.close().expect("closes");

    let current = export_nested(&path);
    downgrade_to_version_2(&path, object);
    let before = ferritecad_types::ContentHash::of_bytes(
        &std::fs::read(&path).expect("snapshots the document"),
    );
    let after = export_nested(&path);

    // Everything a user sees is unchanged.
    assert_eq!(
        described(&after),
        described(&current),
        "a version 2 document exports something other than what it always did"
    );
    // And the one thing it cannot answer is said rather than filled in.
    assert_eq!(
        occurrences(&after),
        vec![ExportOccurrence::Unrecorded; 7],
        "a document that never recorded placement identities was given some"
    );

    // Reading it did not rewrite it.
    assert_eq!(
        ferritecad_types::ContentHash::of_bytes(
            &std::fs::read(&path).expect("re-reads the document")
        ),
        before,
        "exporting a version 2 document rewrote it"
    );
}

/// Rewrites an imported object's stored scene as a version 2 build left it.
///
/// There is no supported way to write one: a build that has placement
/// identities must not produce a scene without them. This reaches past the
/// writer for the one thing only a test needs — a document that predates the
/// layout it is being read by.
fn downgrade_to_version_2(path: &Path, object: ObjectId) {
    rewrite_stored_scene(path, object, 2, |scene| KeyedScene {
        source_unit: scene.source_unit.clone(),
        schema: scene.schema.clone(),
        definitions: scene.definitions.clone(),
        instances: scene
            .instances
            .iter()
            .map(|instance| KeyedInstance {
                definition: instance.definition.clone(),
                parent: instance.parent,
                name: instance.name.clone(),
                placement: instance.placement,
                colour_source: instance.colour_source,
                colour: instance.colour,
            })
            .collect(),
    });
}

#[test]
fn placement_identities_that_were_swapped_or_stolen_are_refused_before_any_export() {
    // Two placements answering to one identity, however it got that way. A
    // swap between neighbours is not detectable — two identities the document
    // wrote are two identities whichever place each sits at, and this slice
    // has no reimport semantics that could say otherwise — so what is gated is
    // the state that is detectable and is the one that does damage: one
    // identity naming two places.
    for (what, damage) in [
        (
            "an identity copied onto its neighbour",
            Box::new(|scene: &mut PersistedScene| {
                scene.instances[1].occurrence = scene.instances[0].occurrence;
            }) as Box<dyn Fn(&mut PersistedScene)>,
        ),
        (
            "an identity stolen from further down the scene",
            Box::new(|scene: &mut PersistedScene| {
                scene.instances[0].occurrence = scene.instances[6].occurrence;
            }),
        ),
    ] {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("damaged.fcad");
        let mut kernel = MockKernel::new();
        let object = ObjectId::new();
        let mut document = Document::create(&path).expect("creates a document");
        let scene = nested_assembly(&mut kernel);
        store_import(
            &mut document,
            &mut kernel,
            Stored {
                object,
                name: "Assembly",
                source: SOURCE,
                source_name: "03-nested-assembly.step",
                scene,
                diagnostics: Vec::new(),
            },
        );
        document.close().expect("closes");

        rewrite_stored_scene(&path, object, 3, |scene| {
            let mut damaged = scene.clone();
            damage(&mut damaged);
            damaged
        });

        let mut kernel = MockKernel::new();
        let error = export_scene(
            &path,
            &mut kernel,
            |kernel, _| {
                Ok(Import::Imported {
                    scene: nested_assembly(kernel),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect_err(&format!("{what} should not have exported"));
        assert_eq!(error.kind(), ErrorKind::Input, "{what}: {error}");
        assert!(error.to_string().contains("occurrence"), "{what}: {error}");
        assert_eq!(kernel.live_shape_count(), 0, "{what} leaked shapes");
    }
}

#[test]
fn one_identity_naming_two_placements_is_refused_at_the_export_boundary_too() {
    // The persistence boundary refuses this first, so a document can never
    // reach the builder with it. The builder refuses it as well, because it is
    // the boundary that sees the whole document at once: two imported objects
    // could each be internally sound and still claim one identity between them.
    let mut builder = ExportSceneBuilder::new();
    let definition = builder
        .definition(
            ExportSource::Body {
                object: ObjectId::new(),
            },
            Some("Part".to_owned()),
            ExportProvenance::default(),
            ExportGeometry::Structural,
        )
        .expect("a definition");
    let occurrence = ExportOccurrence::Occurrence(OccurrenceId::new());
    builder
        .node(
            None,
            definition,
            ExportTransform::IDENTITY,
            Some("First".to_owned()),
            None,
            occurrence,
        )
        .expect("the first placement");
    let error = builder
        .node(
            None,
            definition,
            ExportTransform::IDENTITY,
            Some("Second".to_owned()),
            None,
            occurrence,
        )
        .expect_err("one identity cannot name two placements");
    assert_eq!(error.kind(), ErrorKind::Topology, "{error}");

    // A placement with no identity recorded may of course repeat: that is not
    // an identity, it is the absence of one, and a legacy document is full of
    // them.
    builder
        .node(
            None,
            definition,
            ExportTransform::IDENTITY,
            Some("Third".to_owned()),
            None,
            ExportOccurrence::Unrecorded,
        )
        .expect("an unrecorded placement");
    builder
        .node(
            None,
            definition,
            ExportTransform::IDENTITY,
            Some("Fourth".to_owned()),
            None,
            ExportOccurrence::Unrecorded,
        )
        .expect("a second unrecorded placement");
}

/// Rewrites the stored scene of an imported object at a chosen layout.
fn rewrite_stored_scene<S: serde::Serialize>(
    path: &Path,
    object: ObjectId,
    version: u32,
    rewrite: impl FnOnce(&PersistedScene) -> S,
) {
    #[derive(serde::Serialize)]
    struct Payload<'a, S> {
        source: ferritecad_types::ImportedSourceId,
        source_hash: ferritecad_types::ContentHash,
        source_byte_len: u64,
        source_name: Option<String>,
        scene: &'a S,
        imported_by: ImporterIdentity,
        diagnostics_at_import: Vec<Diagnostic>,
    }

    let document = Document::open_read_only(path).expect("opens");
    let record = document.object(object).expect("reads").expect("is there");
    let ObjectPayload::ImportedStep(stored) = &record.payload else {
        panic!("the object under test is an imported STEP file");
    };
    let StoredScene::V3(current) = &stored.scene else {
        panic!("a fresh import stores a current-layout scene");
    };
    let envelope = Envelope::encode(
        "exchange.step.imported",
        version,
        vec![IMPORTED_STEP_CAPABILITY.to_owned()],
        &Payload {
            source: stored.source,
            source_hash: stored.source_hash,
            source_byte_len: stored.source_byte_len,
            source_name: stored.source_name.clone(),
            scene: &rewrite(current),
            imported_by: stored.imported_by.clone(),
            diagnostics_at_import: stored.diagnostics_at_import.clone(),
        },
    )
    .expect("encodes")
    .to_bytes()
    .expect("serialises");
    drop(document);

    let connection = rusqlite::Connection::open(path).expect("opens the document as SQL");
    connection
        .execute(
            "UPDATE objects SET schema_version = ?1, payload = ?2, payload_hash = ?3 \
             WHERE id = ?4",
            rusqlite::params![
                version,
                envelope.as_slice(),
                ferritecad_types::ContentHash::of_bytes(&envelope)
                    .as_bytes()
                    .as_slice(),
                object.to_bytes().as_slice()
            ],
        )
        .expect("rewrites the stored payload");
    connection.close().expect("closes the SQL connection");
}

// -------------------------------------------------------- architecture

struct Probe<T>(std::marker::PhantomData<T>);

trait NotSerialisable {
    const SERIALISABLE: bool = false;
}

impl<T> NotSerialisable for Probe<T> {}

impl<T: serde::Serialize> Probe<T> {
    const SERIALISABLE: bool = true;
}

#[test]
fn an_export_scene_cannot_be_written_down() {
    // Compared as a pair, because the probe means nothing unless it can also
    // see a type that really is serialisable.
    let observed = [
        Probe::<ExportScene>::SERIALISABLE,
        Probe::<Transform>::SERIALISABLE,
    ];
    assert_eq!(
        observed,
        [false, true],
        "an ExportScene describes one build's tessellation and must not be storable"
    );
}

#[test]
fn the_debug_output_names_no_transient_identity() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("debug.fcad");
    let mut kernel = MockKernel::new();
    document_with_nested_assembly(&path, &mut kernel);
    let scene = export_scene(
        &path,
        &mut kernel,
        |kernel, _| {
            Ok(Import::Imported {
                scene: nested_assembly(kernel),
                diagnostics: Vec::new(),
            })
        },
        &params(),
        &OperationContext::default(),
    )
    .expect("exports");

    let debug = format!("{scene:?}");
    for forbidden in [
        "ShapeHandle",
        "SubShapeHandle",
        "SessionId",
        "PickId",
        "FacePickId",
        "EdgePickId",
        "VertexPickId",
        "RenderSnapshot",
        "Camera",
    ] {
        assert!(
            !debug.contains(forbidden),
            "an ExportScene's Debug output mentions {forbidden}"
        );
    }
    assert!(debug.contains("ExportScene"));
}
