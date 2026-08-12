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

use std::path::Path;

use ferritecad_document::{Document, ObjectPayload};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
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
/// The bodies it describes, in document order, each tessellated once and
/// placed at the origin. A native document has no assembly structure yet, so
/// every body is its own definition with one placement; when instancing
/// arrives it belongs here, in the projection, and not in the renderer.
///
/// Cancellation is checked between bodies as well as inside the rebuild, so a
/// document whose geometry takes a while can be abandoned without waiting for
/// it to finish.
pub fn snapshot_of(
    path: &Path,
    kernel: &mut dyn GeometryKernel,
    params: &TessellationParams,
    context: &OperationContext,
) -> Result<RenderSnapshot> {
    let document = Document::open_read_only(path)?;

    // Cold on purpose, as everywhere else a result must be right rather than
    // quick: consulting a cache would make what is on screen depend on the
    // state of a sidecar that exists only to save time.
    let built = rebuild_cold(&document, kernel, context)?;

    // Everything that can fail happens in here, so that the shapes can be
    // handed back in one place whatever the outcome.
    let snapshot = (|| -> Result<RenderSnapshot> {
        let mut builder = SnapshotBuilder::new();
        for object in document.objects()? {
            let ObjectPayload::Body(body) = &object.payload else {
                continue;
            };
            // A body with no tip feature is empty by definition rather than
            // broken: nothing has been built into it yet.
            if body.tip_feature.is_none() {
                continue;
            }
            context.check_cancelled()?;

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
        Ok(builder.build())
    })();

    built.release_all(kernel);
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    use ferritecad_kernel::mock::MockKernel;
    use ferritecad_kernel::{
        ArchiveSlot, BrepBlob, CancelToken, ExtrudeRequest, ExtrudeResult, KernelIdentity, Mesh,
        OperationResult, ShapeHandle, SubShapeHandle,
    };

    /// The committed plate, copied somewhere the test owns.
    ///
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

    #[test]
    fn the_committed_plate_becomes_something_to_draw() {
        let (_directory, path) = plate();
        let mut kernel = MockKernel::new();

        let snapshot = snapshot_of(&path, &mut kernel, &params(), &OperationContext::default())
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
    fn every_definition_can_be_named_and_told_apart() {
        let (_directory, path) = plate();
        let mut kernel = MockKernel::new();
        let snapshot = snapshot_of(&path, &mut kernel, &params(), &OperationContext::default())
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
        snapshot_of(&path, &mut kernel, &params(), &OperationContext::default())
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
        let error = snapshot_of(&path, &mut kernel, &params(), &OperationContext::default())
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

        snapshot_of(&path, &mut kernel, &params(), &OperationContext::default())
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

        let error = snapshot_of(&path, &mut kernel, &params(), &context)
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

        let error = snapshot_of(&path, &mut kernel, &params(), &context)
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

        let error = snapshot_of(&path, &mut kernel, &params(), &OperationContext::default())
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
        let snapshot = snapshot_of(&path, &mut kernel, &params(), &OperationContext::default())
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
        let snapshot = snapshot_of(&path, &mut kernel, &params(), &OperationContext::default())
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

        let error = snapshot_of(&path, &mut kernel, &params(), &context)
            .expect_err("a cancelled load must not produce a picture");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert_eq!(
            kernel.inner.live_shape_count(),
            0,
            "cancelling between bodies left the session holding solids"
        );
    }
}
