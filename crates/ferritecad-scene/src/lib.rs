// SPDX-License-Identifier: MIT
//! Turning a stored document into a picture of what it describes.
//!
//! One direction only: a document is read, rebuilt, tessellated and packed
//! into a [`RenderSnapshot`]. Nothing here writes, and nothing here draws.
//!
//! # The document is not touched
//!
//! Opening is [`Document::open_read_only`], which neither migrates a schema
//! nor changes a persistent pragma. A viewer that quietly rewrote the file it
//! was asked to look at would be the worst kind of surprise: the change would
//! be invisible, and it would happen to the one copy the user has.
//!
//! # A kernel is handed in
//!
//! So this can be exercised against the mock, with no Open CASCADE anywhere,
//! and so the caller decides which session the shapes belong to. Every shape
//! this makes is released before it returns, on the path that succeeds and on
//! every path that does not: a viewer that leaked a session's worth of solids
//! per failed load would run out of memory while showing an error message.
//!
//! # Two kinds of geometry, one session
//!
//! A native body is rebuilt from its features. An imported STEP object is not
//! built at all: its geometry comes from bytes the document stores, and
//! reading them again needs an importer. Both must end up in the same kernel
//! session, because both are drawn in the same picture and released by the
//! same session at the end.
//!
//! That is why reading a STEP file arrives as a function rather than as a
//! second object: `GeometryKernel` is the kernel's contract and `StepImporter`
//! is the document's, and nothing in the shipped graph points from the kernel
//! adapter back at the document. Passing the kernel to the function instead of
//! capturing it is what lets one `&mut` satisfy both.

use std::collections::BTreeMap;
use std::path::Path;

use ferritecad_document::{Document, ObjectPayload, StepImporter};
use ferritecad_eval::rebuild_cold;
use ferritecad_exchange::{ColourSource, Import, Scene};
use ferritecad_kernel::{
    GeometryKernel, KernelIdentity, OperationContext, ShapeHandle, TessellationParams,
};
use ferritecad_types::{CadError, Result, Transform};
use ferritecad_viewport::{RenderSnapshot, SnapshotBuilder};

/// What every body is drawn in.
///
/// One colour for all of them, because a document records no appearance and
/// inventing one per body would be presenting a decision nobody made as
/// something the file said. Appearance is a document feature that does not
/// exist yet; when it does, it will arrive here as data rather than as a
/// palette.
const BODY_COLOUR: [f64; 3] = [0.62, 0.66, 0.70];

/// Reads a document and builds a picture of it.
///
/// Native bodies and imported scenes, in document order. A body is tessellated
/// once and placed at the origin; an imported scene is re-read from the bytes
/// the document stores, and every definition that is actually drawn is
/// tessellated once however many places it appears in.
///
/// `read_step` is how this asks the kernel to read a stored STEP file again.
/// It takes the kernel rather than holding it, so the same session builds
/// both kinds of geometry; a document with no imports never calls it, which is
/// why a caller with no importer can pass one that refuses.
///
/// Cancellation is checked between objects and between definitions as well as
/// inside the rebuild, so a document whose geometry takes a while can be
/// abandoned without waiting for it to finish.
pub fn snapshot_of<K>(
    path: &Path,
    kernel: &mut K,
    mut read_step: impl FnMut(&mut K, &[u8]) -> Result<Import>,
    params: &TessellationParams,
    context: &OperationContext,
) -> Result<RenderSnapshot>
where
    K: GeometryKernel + ?Sized,
{
    let document = Document::open_read_only(path)?;

    // Cold on purpose, as everywhere else a result must be right rather than
    // quick: consulting a cache would make what is on screen depend on the
    // state of a sidecar that exists only to save time.
    let built = rebuild_cold(&document, kernel, context)?;

    // Handles this function obtained itself, as opposed to the ones the
    // rebuild owns. Filled as it goes so that a failure halfway through an
    // assembly still gives back what had already been read.
    let mut imported: Vec<ShapeHandle> = Vec::new();

    // Everything that can fail happens in here, so that the shapes can be
    // handed back in one place whatever the outcome.
    let snapshot = (|| -> Result<RenderSnapshot> {
        let mut builder = SnapshotBuilder::new();
        for object in document.objects()? {
            context.check_cancelled()?;
            match &object.payload {
                ObjectPayload::Body(body) => {
                    // A body with no tip feature is empty by definition rather
                    // than broken: nothing has been built into it yet.
                    if body.tip_feature.is_none() {
                        continue;
                    }
                    let shape = built.shape(object.id).ok_or_else(|| {
                        CadError::topology(format!(
                            "body {} names a feature but the rebuild produced no geometry for it",
                            object.id
                        ))
                    })?;
                    let mesh = kernel.tessellate(shape, params, context)?;
                    let definition = builder.add_mesh(&mesh)?;
                    builder.place(definition, None, &Transform::IDENTITY, BODY_COLOUR)?;
                }

                ObjectPayload::ImportedStep(_) => {
                    // Scoped so the borrow ends before the kernel is needed
                    // for meshing. A refusal here releases what it built; what
                    // it returns is this function's to give back.
                    let reopened = {
                        let mut reader = Reader {
                            kernel: &mut *kernel,
                            read: &mut read_step,
                        };
                        document.reopen_step_import(object.id, &mut reader)?
                    };
                    imported.extend(reopened.scene.shapes());
                    draw_scene(&mut builder, kernel, &reopened.scene, params, context)?;
                }

                _ => continue,
            }
        }
        Ok(builder.build())
    })();

    for shape in imported.into_iter().rev() {
        kernel.release(shape);
    }
    built.release_all(kernel);
    snapshot
}

/// Adds an imported scene to the picture being built.
///
/// # Only the leaves carry geometry
///
/// An assembly arrives as both: a definition whose shape is the whole
/// assembly, and separate instances of the parts inside it. Drawing every
/// instance would draw the same solids twice – once through the assembly's own
/// compound and once through each component – so an instance that has children
/// is structure and is not drawn. Its placement still counts: it is what its
/// children sit in.
///
/// # Composed here
///
/// A file records each placement relative to its parent, which is the file's
/// own structure and worth keeping in the document. A picture needs world
/// positions, so the chain is multiplied out once, here, where the tree is
/// still in hand.
fn draw_scene<K: GeometryKernel + ?Sized>(
    builder: &mut SnapshotBuilder,
    kernel: &mut K,
    scene: &Scene,
    params: &TessellationParams,
    context: &OperationContext,
) -> Result<()> {
    let mut structural = vec![false; scene.instances.len()];
    for (index, instance) in scene.instances.iter().enumerate() {
        let Some(parent) = instance.parent else {
            continue;
        };
        let holds = structural.get_mut(parent).ok_or_else(|| {
            CadError::input(format!(
                "instance {index} sits inside {parent}, which this scene does not have"
            ))
        })?;
        *holds = true;
    }

    // Parents come before children, so one pass composes the whole tree.
    let mut world: Vec<Transform> = Vec::with_capacity(scene.instances.len());
    for (index, instance) in scene.instances.iter().enumerate() {
        let local = placement_of(&instance.placement)?;
        let placed = match instance.parent {
            None => local,
            Some(parent) => {
                let outer = world.get(parent).ok_or_else(|| {
                    CadError::input(format!(
                        "instance {index} sits inside {parent}, which the scene lists after it"
                    ))
                })?;
                local.then(outer)?
            }
        };
        world.push(placed);
    }

    let mut meshes: BTreeMap<usize, usize> = BTreeMap::new();
    for (index, instance) in scene.instances.iter().enumerate() {
        if structural[index] {
            continue;
        }
        context.check_cancelled()?;

        let mesh = match meshes.get(&instance.definition) {
            Some(mesh) => *mesh,
            None => {
                let definition = scene.definitions.get(instance.definition).ok_or_else(|| {
                    CadError::input(format!(
                        "instance {index} draws definition {}, which this scene does not have",
                        instance.definition
                    ))
                })?;
                let mesh = kernel.tessellate(definition.shape, params, context)?;
                let packed = builder.add_mesh(&mesh)?;
                meshes.insert(instance.definition, packed);
                packed
            }
        };

        // Linear RGB as the importer read it out of the file. Where the file
        // said nothing, the same neutral colour a native body gets: inventing
        // one per part would present a decision nobody made as something the
        // file recorded.
        // Any source at all means the number beside it came from the file.
        // Written this way rather than by naming the two known sources: a
        // third would be another place a colour can come from, not a reason to
        // stop using it.
        let colour = match instance.colour_source {
            ColourSource::None => BODY_COLOUR,
            _ => instance.colour,
        };
        builder.place(mesh, None, &world[index], colour)?;
    }
    Ok(())
}

/// Turns a scene's row-major 3x4 placement into a transform.
fn placement_of(placement: &[f64; 12]) -> Result<Transform> {
    Transform::from_rows([
        [placement[0], placement[1], placement[2], placement[3]],
        [placement[4], placement[5], placement[6], placement[7]],
        [placement[8], placement[9], placement[10], placement[11]],
    ])
}

/// A kernel, behind the contract the document uses to re-read a STEP file.
///
/// Holds the kernel for exactly as long as one reopening takes. Identity and
/// release come from the kernel itself, so the only thing a caller has to
/// supply is how this particular kernel reads STEP bytes.
struct Reader<'a, K: ?Sized, F> {
    kernel: &'a mut K,
    read: F,
}

impl<K, F> StepImporter for Reader<'_, K, F>
where
    K: GeometryKernel + ?Sized,
    F: FnMut(&mut K, &[u8]) -> Result<Import>,
{
    fn identity(&self) -> &KernelIdentity {
        self.kernel.identity()
    }

    fn import(&mut self, source: &[u8]) -> Result<Import> {
        (self.read)(self.kernel, source)
    }

    fn release(&mut self, shape: ShapeHandle) {
        self.kernel.release(shape);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ferritecad_document::StepImportRequest;
    use ferritecad_exchange::{Definition, Instance};
    use ferritecad_kernel::mock::MockKernel;
    use ferritecad_kernel::{
        ArchiveSlot, BrepBlob, CancelToken, ExtrudeRequest, ExtrudeResult, KernelIdentity, Mesh,
        OperationResult, ShapeHandle, SubShapeHandle,
    };
    use ferritecad_types::ObjectId;

    /// The committed plate, copied somewhere the test owns.
    ///
    /// What a caller with no importer passes.
    ///
    /// A document with no imports never asks, so this refusing before it can
    /// do anything is also the check that it never asked.
    fn no_imports<K: ?Sized>(_: &mut K, _: &[u8]) -> Result<Import> {
        Err(CadError::unsupported(
            "this test opened a document that was supposed to hold no imports",
        ))
    }

    /// Copied rather than opened in place because a test that touched the
    /// checkout would be the very thing this crate promises not to do.
    fn plate() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");
        (directory, path)
    }

    /// A document of `count` separate square bodies, ten apart along x.
    ///
    /// The committed plate is one body, which cannot show that a second one is
    /// drawn, ordered or released. This is the smallest document that can.
    fn several_bodies(path: &Path, count: usize) -> Vec<ferritecad_types::ObjectId> {
        use ferritecad_document::{
            Body, DatumPlane, Dependency, DependencyRole, EndCondition, Expression, Extrude,
            Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
        };
        use ferritecad_types::{ObjectId, StableEntityId};

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
                    let (sketch, extrude, body) =
                        (ObjectId::new(), ObjectId::new(), ObjectId::new());
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
                        None,
                        &ObjectPayload::Sketch(Sketch { plane, curves }),
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
                        ordinal + 3,
                        None,
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

    fn params() -> TessellationParams {
        TessellationParams::new(
            TessellationParams::DEFAULT_LINEAR,
            TessellationParams::DEFAULT_ANGULAR,
            false,
        )
        .expect("the defaults are valid")
    }

    /// One mock solid, so a fabricated scene refers to geometry that exists.
    fn solid(kernel: &mut MockKernel) -> ShapeHandle {
        use ferritecad_kernel::{
            ExtrudeExtent, PlanarPoint, Profile, ProfileLoop, ProfileSegment, SegmentGeometry,
            SketchPlane,
        };
        use ferritecad_types::StableEntityId;

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

    fn instance(
        definition: usize,
        parent: Option<usize>,
        at: [f64; 3],
        colour_source: ColourSource,
        colour: [f64; 3],
    ) -> Instance {
        Instance {
            definition,
            parent,
            name: String::new(),
            placement: [
                1.0, 0.0, 0.0, at[0], 0.0, 1.0, 0.0, at[1], 0.0, 0.0, 1.0, at[2],
            ],
            colour_source,
            colour,
        }
    }

    /// The shape of `fixtures/step/canonical/03-nested-assembly.step`.
    ///
    /// Measured from the real import rather than imagined: two groups of two
    /// cubes inside an outer group, where every placement is relative to its
    /// parent and the two group definitions carry the whole compound of what
    /// is inside them. That last part is what makes drawing every instance
    /// wrong, so a made-up scene that left it out would agree with a wrong
    /// implementation.
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
                instance(0, None, [0.0, 0.0, 0.0], ColourSource::None, [0.0; 3]),
                instance(1, Some(0), [0.0, 0.0, 0.0], ColourSource::None, [0.0; 3]),
                instance(
                    2,
                    Some(1),
                    [0.0, 0.0, 0.0],
                    ColourSource::Definition,
                    [0.1, 0.2, 0.3],
                ),
                instance(
                    2,
                    Some(1),
                    [30.0, 0.0, 0.0],
                    ColourSource::Definition,
                    [0.1, 0.2, 0.3],
                ),
                instance(1, Some(0), [0.0, 40.0, 0.0], ColourSource::None, [0.0; 3]),
                instance(
                    2,
                    Some(4),
                    [0.0, 0.0, 0.0],
                    ColourSource::Definition,
                    [0.1, 0.2, 0.3],
                ),
                instance(
                    2,
                    Some(4),
                    [30.0, 0.0, 0.0],
                    ColourSource::Instance,
                    [0.9, 0.1, 0.1],
                ),
            ],
        }
    }

    /// A document holding one stored import of `scene`.
    ///
    /// The bytes are not a STEP file and never need to be: the document stores
    /// whatever it was handed and hands the same back, and what reads them is
    /// the importer this test supplies.
    fn document_with_import(path: &Path, kernel: &mut MockKernel) -> ObjectId {
        let scene = nested_assembly(kernel);
        let import = Import::Imported {
            scene,
            diagnostics: Vec::new(),
        };
        let object = ObjectId::new();
        let mut document = Document::create(path).expect("creates a document");
        document
            .store_step_import(StepImportRequest {
                object,
                name: Some("Assembly"),
                source: SOURCE,
                source_name: Some("03-nested-assembly.step"),
                import: &import,
                importer: kernel.identity(),
            })
            .expect("stores the import");
        for shape in import
            .scene()
            .expect("this import produced a scene")
            .shapes()
        {
            kernel.release(shape);
        }
        object
    }

    const SOURCE: &[u8] = b"ISO-10303-21; this is what the document stores";

    #[test]
    fn a_stored_assembly_is_drawn_once_per_place_it_appears() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("assembly.fcad");
        let mut kernel = MockKernel::new();
        document_with_import(&path, &mut kernel);
        assert_eq!(kernel.live_shape_count(), 0, "the setup kept shapes");

        let snapshot = snapshot_of(
            &path,
            &mut kernel,
            // Reading the file again produces the same scene with new handles,
            // which is what a second kernel session does.
            |kernel, source| {
                assert_eq!(source, SOURCE, "the document handed over other bytes");
                Ok(Import::Imported {
                    scene: nested_assembly(kernel),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect("the stored assembly reopens");

        // One mesh: four cubes in four places are one definition, and the two
        // groups are structure. A loader that tessellated every definition
        // would report three, and one that drew every instance would put the
        // whole assembly on screen twice.
        assert_eq!(snapshot.meshes().len(), 1, "definitions were meshed twice");
        assert_eq!(snapshot.draws().len(), 4, "one draw per cube, and no more");
        for item in snapshot.draws() {
            assert_eq!(item.mesh, 0);
        }

        // Where each cube ended up: the inner placement composed with the
        // group it sits in. A loader that ignored the tree would put all four
        // at two positions.
        let mut corners: Vec<[i64; 3]> = snapshot
            .draws()
            .iter()
            .map(|item| {
                // Column-major, so the translation is the last column.
                [
                    item.transform[12].round() as i64,
                    item.transform[13].round() as i64,
                    item.transform[14].round() as i64,
                ]
            })
            .collect();
        corners.sort_unstable();
        assert_eq!(
            corners,
            vec![[0, 0, 0], [0, 40, 0], [30, 0, 0], [30, 40, 0]]
        );

        assert_eq!(
            kernel.live_shape_count(),
            0,
            "the imported shapes were never given back"
        );
    }

    #[test]
    fn what_the_file_said_about_colour_is_what_is_drawn() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("assembly.fcad");
        let mut kernel = MockKernel::new();
        document_with_import(&path, &mut kernel);

        let snapshot = snapshot_of(
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
        .expect("the stored assembly reopens");

        // Three cubes take their definition's colour and one is painted over
        // it. Linear RGB, straight from the file: converting here would guess
        // at a transfer function the importer deliberately did not apply.
        let mut colours: Vec<[u32; 3]> = snapshot
            .draws()
            .iter()
            .map(|item| {
                [
                    (item.colour[0] * 1000.0).round() as u32,
                    (item.colour[1] * 1000.0).round() as u32,
                    (item.colour[2] * 1000.0).round() as u32,
                ]
            })
            .collect();
        colours.sort_unstable();
        assert_eq!(
            colours,
            vec![
                [100, 200, 300],
                [100, 200, 300],
                [100, 200, 300],
                [900, 100, 100]
            ]
        );
        for item in snapshot.draws() {
            assert_eq!(item.colour[3], 1.0, "an imported part is not transparent");
        }
    }

    #[test]
    fn an_import_that_cannot_be_reopened_keeps_nothing() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("assembly.fcad");
        let mut kernel = MockKernel::new();
        document_with_import(&path, &mut kernel);

        // The file reads, and describes something else. Binding refuses, and
        // the shapes it built are the importer's to take back – which is the
        // document's contract, checked here because this is the caller that
        // would otherwise be holding them.
        let error = snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                let mut scene = nested_assembly(kernel);
                scene.definitions[2].name = "Cuboid".to_owned();
                Ok(Import::Imported {
                    scene,
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect_err("a scene that is not what was stored must be refused");
        assert!(error.to_string().contains("Cuboid"), "{error}");
        assert_eq!(kernel.live_shape_count(), 0);

        // And a reading that fails outright.
        let error = snapshot_of(
            &path,
            &mut kernel,
            |_, _| Err(CadError::kernel("the file could not be read again")),
            &params(),
            &OperationContext::default(),
        )
        .expect_err("a reading that failed is not a picture");
        assert!(error.to_string().contains("could not be read again"));
        assert_eq!(kernel.live_shape_count(), 0);
    }

    #[test]
    fn cancelling_between_the_parts_of_an_assembly_gives_them_all_back() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("assembly.fcad");
        let mut kernel = MockKernel::new();
        document_with_import(&path, &mut kernel);

        // Cancelled the moment the scene has been read and bound, with every
        // imported solid live and none of them drawn yet.
        let token = CancelToken::new();
        let context = OperationContext::default().with_cancel(token.clone());
        let error = snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                let scene = nested_assembly(kernel);
                token.cancel();
                Ok(Import::Imported {
                    scene,
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &context,
        )
        .expect_err("a cancelled load must not produce a picture");

        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert_eq!(
            kernel.live_shape_count(),
            0,
            "cancelling left the imported assembly in the session"
        );
    }

    #[test]
    fn the_committed_plate_becomes_something_to_draw() {
        let (_directory, path) = plate();
        let mut kernel = MockKernel::new();

        let snapshot = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the plate loads");

        assert_eq!(snapshot.meshes().len(), 1, "the plate is one body");
        assert_eq!(snapshot.draws().len(), 1);
        assert!(snapshot.meshes()[0].triangle_count() > 0, "it has no faces");

        // 60 x 40 x 10, which is what the fixture is and what a viewer must
        // frame. Checked here so a loader that dropped the placement or the
        // extrusion height would not merely produce fewer triangles.
        let (min, max) = snapshot.bounds().expect("something is drawn");
        let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        assert!((size[0] - 60.0).abs() < 1e-3, "{size:?}");
        assert!((size[1] - 40.0).abs() < 1e-3, "{size:?}");
        assert!((size[2] - 10.0).abs() < 1e-3, "{size:?}");
    }

    #[test]
    fn a_loader_accepts_the_kernel_contract_without_knowing_its_implementation() {
        let (_directory, path) = plate();
        let mut implementation = MockKernel::new();
        let kernel: &mut dyn GeometryKernel = &mut implementation;

        let snapshot = snapshot_of(
            &path,
            kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the contract is enough to load a native document");

        assert_eq!(snapshot.meshes().len(), 1);
        assert_eq!(implementation.live_shape_count(), 0);
    }

    #[test]
    fn every_definition_can_be_named_and_told_apart() {
        let (_directory, path) = plate();
        let mut kernel = MockKernel::new();
        let snapshot = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the plate loads");

        // The viewport's own rule, met here rather than discovered on the GPU.
        for (index, item) in snapshot.draws().iter().enumerate() {
            assert_eq!(
                snapshot.definition(item.pick),
                Some(item.mesh),
                "draw {index} picks something other than what it draws"
            );
        }
    }

    #[test]
    fn looking_at_a_document_does_not_change_it() {
        let (_directory, path) = plate();
        let before = std::fs::read(&path).expect("reads");

        let mut kernel = MockKernel::new();
        snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the plate loads");

        // Byte for byte. `Document::open` would have migrated the schema and
        // set persistent pragmas, and either would be an edit to a file the
        // user only asked to look at.
        assert_eq!(std::fs::read(&path).expect("reads"), before);
        assert!(
            !path.with_extension("fcad-cache").exists(),
            "looking at a document left a cache sidecar beside it"
        );
    }

    #[test]
    fn a_document_another_program_left_in_wal_mode_is_refused_untouched() {
        let (_directory, path) = plate();
        {
            let connection = rusqlite::Connection::open(&path).expect("opens the copy");
            let mode: String = connection
                .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
                .expect("switches journal mode");
            assert_eq!(mode, "wal");
        }
        let before = std::fs::read(&path).expect("reads");

        let mut kernel = MockKernel::new();
        let error = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect_err("a WAL document must be refused rather than converted");
        assert!(
            error.to_string().contains("WAL"),
            "the refusal does not say what is wrong: {error}"
        );

        // The point of refusing. Reading a document must never rewrite its
        // journal mode or leave `-wal` and `-shm` beside it: that is an edit to
        // a file the user only asked to look at, and it happens behind a
        // program that may still have the document open.
        assert_eq!(std::fs::read(&path).expect("reads"), before);
        for sidecar in ["fcad-wal", "fcad-shm", "fcad-cache"] {
            assert!(
                !path.with_extension(sidecar).exists(),
                "looking at a document left a .{sidecar} beside it"
            );
        }
    }

    #[test]
    fn a_load_that_succeeds_keeps_no_shapes() {
        let (_directory, path) = plate();
        let mut kernel = MockKernel::new();

        snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the plate loads");
        assert_eq!(
            kernel.live_shape_count(),
            0,
            "the snapshot is packed and the shapes were still kept"
        );
    }

    #[test]
    fn a_load_that_fails_keeps_no_shapes_either() {
        let (directory, _) = plate();
        let mut kernel = MockKernel::new();

        // A file that is not a document at all: nothing is built, so nothing
        // can be leaked, but the path has to be exercised to say so.
        let rubbish = directory.path().join("not-a-document.fcad");
        std::fs::write(&rubbish, b"this is not a SQLite file").expect("writes");
        assert!(
            snapshot_of(
                &rubbish,
                &mut kernel,
                no_imports,
                &params(),
                &OperationContext::default()
            )
            .is_err()
        );
        assert_eq!(kernel.live_shape_count(), 0);

        // And one that does not exist.
        assert!(
            snapshot_of(
                &directory.path().join("absent.fcad"),
                &mut kernel,
                no_imports,
                &params(),
                &OperationContext::default()
            )
            .is_err()
        );
        assert_eq!(kernel.live_shape_count(), 0);
    }

    #[test]
    fn cancelling_before_anything_is_built_produces_no_picture() {
        let (_directory, path) = plate();
        let mut kernel = MockKernel::new();

        let token = CancelToken::new();
        token.cancel();
        let context = OperationContext::default().with_cancel(token);

        let error = snapshot_of(&path, &mut kernel, no_imports, &params(), &context)
            .expect_err("a cancelled load must not produce a picture");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert_eq!(kernel.live_shape_count(), 0);
    }

    /// What makes a kernel stop once the geometry already exists.
    enum Stop {
        /// The user changed their mind while the model was being meshed.
        Cancelled(CancelToken),
        /// Meshing itself failed.
        Failed,
        /// The user changed their mind between one body and the next, and the
        /// kernel noticed nothing: it was asked for one mesh and gave one.
        BetweenBodies(CancelToken),
    }

    /// A kernel that lets the rebuild finish and then refuses.
    ///
    /// This is the only arrangement in which a leak is possible at all. A
    /// loader that released shapes only on its way out of a successful load
    /// would pass every other test in this file: before the rebuild there is
    /// nothing to leak, and after the snapshot is packed there is nothing left
    /// to go wrong.
    struct StopsAfterBuilding {
        inner: MockKernel,
        stop: Stop,
    }

    impl StopsAfterBuilding {
        fn new(stop: Stop) -> Self {
            Self {
                inner: MockKernel::new(),
                stop,
            }
        }
    }

    impl GeometryKernel for StopsAfterBuilding {
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
            match &self.stop {
                Stop::Cancelled(token) => {
                    // Cancelled at the moment the picture was about to be
                    // built, with every solid of the model live.
                    token.cancel();
                    Err(ferritecad_types::CadError::Cancelled)
                }
                Stop::Failed => Err(CadError::topology("this shape cannot be meshed")),
                Stop::BetweenBodies(token) => {
                    // Deliberately meshed under a context that is not
                    // cancelled: the kernel contract says cancelling is a
                    // request, and that some algorithms finish the unit of work
                    // they are in. Such a kernel is conforming, and answering
                    // one more question correctly must not turn into meshing
                    // the rest of a model nobody is waiting for.
                    let uninterrupted = OperationContext::new(context.tolerance());
                    let mesh = self.inner.tessellate(shape, params, &uninterrupted)?;
                    token.cancel();
                    Ok(mesh)
                }
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

    #[test]
    fn cancelling_after_the_model_is_built_gives_every_shape_back() {
        let (_directory, path) = plate();
        let token = CancelToken::new();
        let mut kernel = StopsAfterBuilding::new(Stop::Cancelled(token.clone()));
        let context = OperationContext::default().with_cancel(token);

        let error = snapshot_of(&path, &mut kernel, no_imports, &params(), &context)
            .expect_err("a cancelled load must not produce a picture");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);

        // The model really was built first, so there was something to leak.
        assert!(kernel.inner.extrude_count() > 0, "nothing was ever built");
        assert_eq!(
            kernel.inner.live_shape_count(),
            0,
            "cancelling left the session holding solids"
        );
    }

    #[test]
    fn failing_after_the_model_is_built_gives_every_shape_back() {
        let (_directory, path) = plate();
        let mut kernel = StopsAfterBuilding::new(Stop::Failed);

        let error = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect_err("meshing failed, so there is no picture");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Topology);

        assert!(kernel.inner.extrude_count() > 0, "nothing was ever built");
        assert_eq!(
            kernel.inner.live_shape_count(),
            0,
            "a failed load left the session holding solids"
        );
    }

    #[test]
    fn every_body_becomes_its_own_drawing_in_document_order() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("three.fcad");
        let bodies = several_bodies(&path, 3);
        assert_eq!(bodies.len(), 3);

        let mut kernel = MockKernel::new();
        let snapshot = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the document loads");

        assert_eq!(snapshot.meshes().len(), 3, "bodies were merged or dropped");
        assert_eq!(snapshot.draws().len(), 3);

        // Each draw names its own mesh, and the order follows the document
        // rather than whatever order the rebuild happened to finish in.
        let named: Vec<usize> = snapshot.draws().iter().map(|item| item.mesh).collect();
        assert_eq!(named, vec![0, 1, 2]);

        // And they really are three separate squares ten apart, not one square
        // drawn three times: the whole thing is 25 wide.
        let (min, max) = snapshot.bounds().expect("something is drawn");
        assert!((max[0] - min[0] - 25.0).abs() < 1e-3, "{min:?} {max:?}");
    }

    #[test]
    fn a_body_with_nothing_in_it_yet_is_not_a_failure() {
        use ferritecad_document::{Body, DatumPlane};
        use ferritecad_types::ObjectId;

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("empty-body.fcad");
        let mut document = Document::create(&path).expect("creates a document");
        document
            .write(|w| {
                w.put_object(
                    ObjectId::new(),
                    None,
                    0,
                    Some("XY"),
                    &ObjectPayload::DatumPlane(DatumPlane {
                        placement: Transform::IDENTITY,
                    }),
                )?;
                w.put_object(
                    ObjectId::new(),
                    None,
                    1,
                    Some("Body1"),
                    &ObjectPayload::Body(Body { tip_feature: None }),
                )
            })
            .expect("writes the document");
        drop(document);

        // A body nothing has been built into is empty, not broken, and a
        // viewer that refused to open such a document would refuse the first
        // document anyone makes.
        let mut kernel = MockKernel::new();
        let snapshot = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("a document with an empty body still opens");
        assert!(snapshot.draws().is_empty());
        assert!(snapshot.bounds().is_none(), "empty geometry has no extent");
    }

    #[test]
    fn cancelling_between_two_bodies_gives_every_shape_back() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("two.fcad");
        several_bodies(&path, 2);

        // The kernel answers every question it is asked, correctly: the first
        // mesh comes back whole. Only the loader is in a position to notice
        // that the user has since asked it to stop, and it must, rather than
        // meshing the rest of a model nobody is waiting for.
        let token = CancelToken::new();
        let mut kernel = StopsAfterBuilding::new(Stop::BetweenBodies(token.clone()));
        let context = OperationContext::default().with_cancel(token);

        let error = snapshot_of(&path, &mut kernel, no_imports, &params(), &context)
            .expect_err("a cancelled load must not produce a picture");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert_eq!(
            kernel.inner.live_shape_count(),
            0,
            "cancelling between bodies left the session holding solids"
        );
    }
}
