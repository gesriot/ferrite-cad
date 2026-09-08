// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]

use super::*;
use ferritecad_exchange::{ColourSource, Definition, Instance, Scene, Severity, Stage};
use ferritecad_kernel::{
    ArchiveSlot, BrepBlob, CancelToken, ExtrudeRequest, ExtrudeResult, KernelIdentity, Mesh,
    OperationResult, ProgressSink, SessionId, ShapeHandle, SubShapeHandle, TessellationParams,
};
use ferritecad_types::{ErrorKind, Transform};
use std::sync::{Arc, Mutex};

#[derive(Default, Debug)]
struct Counts {
    opens: usize,
    imports: usize,
    releases: usize,
    live: BTreeSet<ShapeHandle>,
    live_before_drop: Option<usize>,
}

struct CountingKernel {
    identity: KernelIdentity,
    session: SessionId,
    counts: Arc<Mutex<Counts>>,
    thread: std::thread::ThreadId,
}

impl CountingKernel {
    fn new(counts: &Arc<Mutex<Counts>>) -> Self {
        counts.lock().expect("ownership lock").opens += 1;
        Self {
            identity: KernelIdentity::new("counted-import", "1", "test")
                .expect("test fixture operation"),
            session: SessionId::new(),
            counts: counts.clone(),
            thread: std::thread::current().id(),
        }
    }

    fn read(&mut self, bytes: &[u8]) -> Result<Import> {
        assert_eq!(bytes, b"private source bytes");
        let mut counts = self.counts.lock().expect("ownership lock");
        counts.imports += 1;
        let a = ShapeHandle::new(self.session, 0);
        let b = ShapeHandle::new(self.session, 1);
        counts.live.extend([a, b]);
        Ok(Import::Imported {
            scene: Scene {
                schema: "test schema".into(),
                source_unit: "mm".into(),
                // Deliberately share one handle: release ownership only once.
                definitions: [a, b, a]
                    .into_iter()
                    .enumerate()
                    .map(|(i, shape)| Definition {
                        shape,
                        name: format!("part {i} 零\"\n"),
                        solids: u32::from(i != 1),
                        key: format!("definition/{i}"),
                    })
                    .collect(),
                instances: (0..4)
                    .map(|i| Instance {
                        definition: i % 3,
                        parent: (i > 0).then_some(0),
                        name: format!("place {i}"),
                        placement: [1., 0., 0., i as f64, 0., 1., 0., 2., 0., 0., 1., 3.],
                        colour_source: ColourSource::Definition,
                        colour: [0.1, 0.2, 0.3],
                    })
                    .collect(),
            },
            diagnostics: diagnostics(),
        })
    }
}

impl Drop for CountingKernel {
    fn drop(&mut self) {
        assert_eq!(std::thread::current().id(), self.thread);
        let mut counts = self.counts.lock().expect("ownership lock");
        // Observe ownership BEFORE any destructor could hide leaked handles.
        counts.live_before_drop = Some(counts.live.len());
    }
}

impl GeometryKernel for CountingKernel {
    fn identity(&self) -> &KernelIdentity {
        &self.identity
    }
    fn release(&mut self, shape: ShapeHandle) {
        assert_eq!(std::thread::current().id(), self.thread);
        assert_eq!(shape.session(), self.session);
        let mut counts = self.counts.lock().expect("ownership lock");
        assert!(counts.live.remove(&shape), "double or foreign release");
        counts.releases += 1;
    }
    fn extrude(&mut self, _: &ExtrudeRequest, _: &OperationContext) -> Result<ExtrudeResult> {
        unreachable!()
    }
    fn transform(
        &mut self,
        _: ShapeHandle,
        _: &Transform,
        _: &OperationContext,
    ) -> Result<OperationResult> {
        unreachable!()
    }
    fn tessellate(
        &mut self,
        _: ShapeHandle,
        _: &TessellationParams,
        _: &OperationContext,
    ) -> Result<Mesh> {
        unreachable!()
    }
    fn encode_shape_with(
        &mut self,
        _: ShapeHandle,
        _: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
        unreachable!()
    }
    fn decode_shape_with(
        &mut self,
        _: &BrepBlob,
        _: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        unreachable!()
    }
    fn encode_shape(&mut self, _: ShapeHandle) -> Result<BrepBlob> {
        unreachable!()
    }
    fn decode_shape(&mut self, _: &BrepBlob) -> Result<ShapeHandle> {
        unreachable!()
    }
}

fn diagnostics() -> Vec<Diagnostic> {
    [Stage::Load, Stage::Transfer]
        .into_iter()
        .map(|stage| Diagnostic {
            stage,
            severity: Severity::Warning,
            entity: "#42".into(),
            message: "observed 零\"\n".into(),
        })
        .collect()
}

fn keep() -> Existing<'static> {
    Existing::Keep {
        advice: "choose another path",
    }
}

fn files(directory: &Path) -> BTreeSet<PathBuf> {
    std::fs::read_dir(directory)
        .expect("test fixture operation")
        .map(|e| e.expect("test fixture operation").path())
        .collect()
}

fn inputs(directory: &Path) -> (PathBuf, PathBuf) {
    let source = directory.join("source 零.step");
    std::fs::write(&source, b"private source bytes").expect("test fixture operation");
    (source, directory.join("out.fcad"))
}

fn request<'a>(
    source: &'a Path,
    destination: &'a Path,
    existing: Existing<'a>,
) -> ImportStepRequest<'a> {
    ImportStepRequest {
        source,
        destination,
        name: Some("name 零\"\n"),
        existing,
    }
}

fn assert_released(counts: &Arc<Mutex<Counts>>, imports: usize) {
    let counts = counts.lock().expect("ownership lock");
    assert_eq!(counts.imports, imports);
    assert_eq!(counts.releases, 2 * imports);
    assert!(counts.live.is_empty(), "leaked geometry: {counts:?}");
    if counts.opens > 0 {
        assert_eq!(counts.live_before_drop, Some(0));
    }
}

#[test]
fn published_facts_are_the_one_read_and_exact_stored_projection() {
    let dir = tempfile::tempdir().expect("private test directory");
    let (source, destination) = inputs(dir.path());
    let counts = Arc::new(Mutex::new(Counts::default()));
    let changed = source.clone();
    let context = OperationContext::default().with_progress(ProgressSink::new(move |f| {
        if f == 0.1 {
            std::fs::write(&changed, b"changed after reading").expect("test fixture operation");
        }
    }));
    let StepImportOutcome::Published(result) = import_step_document(
        &request(&source, &destination, keep()),
        || Ok(CountingKernel::new(&counts)),
        CountingKernel::read,
        &context,
    )
    .expect("test fixture operation") else {
        panic!("must publish")
    };
    assert_released(&counts, 1);
    assert_eq!(
        std::fs::read(&source).expect("test fixture operation"),
        b"changed after reading"
    );
    assert_eq!(result.destination, destination);
    assert_eq!(result.name, "name 零\"\n");
    assert_eq!(result.source_name.as_deref(), Some("source 零.step"));
    assert_eq!(
        result.read.source_hash,
        ContentHash::of_bytes(b"private source bytes")
    );
    assert_eq!(result.read.source_byte_len, 20);
    assert_eq!(result.read.diagnostics, diagnostics());
    let doc = Document::open_read_only(&destination).expect("test fixture operation");
    assert_eq!(doc.meta().document_id, result.document_id);
    let stored = doc
        .step_import(result.object_id)
        .expect("test fixture operation")
        .expect("test fixture operation");
    assert_eq!(stored.source, b"private source bytes");
    assert_eq!(stored.imported.source, result.source);
    assert_eq!(stored.imported.imported_by, result.read.imported_by);
    assert_eq!(
        stored.imported.diagnostics_at_import,
        result.read.diagnostics
    );
    assert_eq!(stored.imported.scene, StoredScene::V3(result.scene));
    assert_eq!(files(dir.path()), BTreeSet::from([source, destination]));
}

#[test]
fn cancellation_at_every_boundary_releases_and_late_cancel_keeps_publication() {
    for phase in [0.0, 0.1, 0.6, 0.7, 0.8, 0.9, 1.0] {
        for existing in [keep(), Existing::Replace] {
            let dir = tempfile::tempdir().expect("private test directory");
            let (source, destination) = inputs(dir.path());
            if existing == Existing::Replace {
                std::fs::write(&destination, b"old destination").expect("test fixture operation");
            }
            let before = files(dir.path());
            let counts = Arc::new(Mutex::new(Counts::default()));
            let cancel = CancelToken::new();
            let token = cancel.clone();
            let directory = dir.path().to_owned();
            let context = OperationContext::default()
                .with_cancel(cancel)
                .with_progress(ProgressSink::new(move |f| {
                    if f == 0.9 {
                        let scratch = files(&directory)
                            .into_iter()
                            .find(|p| p.is_dir())
                            .expect("test fixture operation")
                            .join("payload");
                        // Windows refuses this if SQLite still owns an open handle.
                        let moved = scratch.with_extension("closed");
                        std::fs::rename(&scratch, &moved).expect("test fixture operation");
                        std::fs::rename(&moved, &scratch).expect("test fixture operation");
                        let doc =
                            Document::open_read_only(&scratch).expect("test fixture operation");
                        assert_eq!(doc.objects().expect("test fixture operation").len(), 1);
                        doc.close().expect("test fixture operation");
                    }
                    if f == phase {
                        token.cancel();
                    }
                }));
            let result = import_step_document(
                &request(&source, &destination, existing),
                || Ok(CountingKernel::new(&counts)),
                CountingKernel::read,
                &context,
            );
            assert_released(&counts, usize::from(phase >= 0.6));
            assert_eq!(
                counts.lock().expect("ownership lock").opens,
                usize::from(phase >= 0.6)
            );
            if phase == 1.0 {
                assert!(matches!(
                    result.expect("test fixture operation"),
                    StepImportOutcome::Published(_)
                ));
                Document::open_read_only(&destination)
                    .expect("test fixture operation")
                    .close()
                    .expect("test fixture operation");
                assert_eq!(
                    files(dir.path()),
                    BTreeSet::from([source.clone(), destination])
                );
            } else {
                assert!(
                    matches!(result, Err(CadError::Cancelled)),
                    "phase {phase}: {result:?}"
                );
                assert_eq!(files(dir.path()), before);
                if existing == Existing::Replace {
                    assert_eq!(
                        std::fs::read(&destination).expect("test fixture operation"),
                        b"old destination"
                    );
                }
            }
            assert_eq!(
                std::fs::read(source).expect("test fixture operation"),
                b"private source bytes"
            );
        }
    }
}

#[test]
fn storage_and_publication_failures_release_every_handle_and_clean_only_scratch() {
    for failure in ["storage", "create", "publish"] {
        let dir = tempfile::tempdir().expect("private test directory");
        let (source, mut destination) = inputs(dir.path());
        if failure == "create" {
            destination = dir.path().join("missing-parent/out.fcad");
        }
        let counts = Arc::new(Mutex::new(Counts::default()));
        let output = destination.clone();
        let context = OperationContext::default().with_progress(ProgressSink::new(move |f| {
            if f == 0.9 && failure == "publish" {
                std::fs::create_dir(&output).expect("test fixture operation");
                std::fs::write(output.join("foreign"), b"keep me").expect("test fixture operation");
            }
        }));
        let result = import_step_document(
            &request(&source, &destination, Existing::Replace),
            || Ok(CountingKernel::new(&counts)),
            |kernel, bytes| {
                let mut outcome = kernel.read(bytes)?;
                if failure == "storage"
                    && let Import::Imported { scene, .. } = &mut outcome
                {
                    scene.definitions[0].key.clear();
                }
                Ok(outcome)
            },
            &context,
        );
        let error = result.expect_err("operation must refuse");
        assert_eq!(
            error.kind(),
            if failure == "storage" {
                ErrorKind::Input
            } else {
                ErrorKind::Io
            }
        );
        assert_released(&counts, 1);
        if failure == "publish" {
            assert_eq!(
                std::fs::read(destination.join("foreign")).expect("test fixture operation"),
                b"keep me"
            );
            assert_eq!(
                files(dir.path()),
                BTreeSet::from([source.clone(), destination])
            );
        } else {
            assert_eq!(files(dir.path()), BTreeSet::from([source.clone()]));
        }
        assert_eq!(
            std::fs::read(source).expect("test fixture operation"),
            b"private source bytes"
        );
    }
}

#[test]
fn publication_rechecks_aliases_and_keep_races_while_replace_publishes_whole() {
    for mode in ["keep", "replace", "alias", "source-swapped"] {
        let dir = tempfile::tempdir().expect("private test directory");
        let (source, destination) = inputs(dir.path());
        let counts = Arc::new(Mutex::new(Counts::default()));
        let output = destination.clone();
        let input = source.clone();
        let context = OperationContext::default().with_progress(ProgressSink::new(move |f| {
            if f == 0.9 {
                if mode == "alias" {
                    std::fs::hard_link(&input, &output).expect("test fixture operation");
                } else {
                    std::fs::write(&output, b"arrived during import")
                        .expect("test fixture operation");
                    if mode == "source-swapped" {
                        std::fs::remove_file(&input).expect("test fixture operation");
                        std::fs::hard_link(&output, &input).expect("test fixture operation");
                    }
                }
            }
        }));
        let result = import_step_document(
            &request(
                &source,
                &destination,
                if mode == "keep" {
                    keep()
                } else {
                    Existing::Replace
                },
            ),
            || Ok(CountingKernel::new(&counts)),
            CountingKernel::read,
            &context,
        );
        assert_released(&counts, 1);
        if mode == "replace" {
            assert!(matches!(
                result.expect("test fixture operation"),
                StepImportOutcome::Published(_)
            ));
            Document::open_read_only(&destination)
                .expect("test fixture operation")
                .close()
                .expect("test fixture operation");
        } else {
            assert_eq!(
                result.expect_err("operation must refuse").kind(),
                ErrorKind::Input
            );
            assert_eq!(
                std::fs::read(&destination).expect("test fixture operation"),
                if mode == "alias" {
                    &b"private source bytes"[..]
                } else {
                    &b"arrived during import"[..]
                }
            );
        }
        assert_eq!(
            std::fs::read(&source).expect("test fixture operation"),
            if mode == "source-swapped" {
                &b"arrived during import"[..]
            } else {
                &b"private source bytes"[..]
            }
        );
        assert_eq!(files(dir.path()), BTreeSet::from([source, destination]));
    }
}

#[test]
fn rejected_has_read_facts_without_publication_and_preflight_precedes_kernel() {
    let dir = tempfile::tempdir().expect("private test directory");
    let (source, destination) = inputs(dir.path());
    let counts = Arc::new(Mutex::new(Counts::default()));
    let result = import_step_document(
        &request(&source, &destination, keep()),
        || Ok(CountingKernel::new(&counts)),
        |_, bytes| {
            assert_eq!(bytes, b"private source bytes");
            Ok(Import::Rejected {
                diagnostics: diagnostics(),
            })
        },
        &OperationContext::default(),
    )
    .expect("test fixture operation");
    let StepImportOutcome::Rejected(read) = result else {
        panic!("rejected")
    };
    assert_eq!(read.diagnostics, diagnostics());
    assert_eq!(
        read.source_hash,
        ContentHash::of_bytes(b"private source bytes")
    );
    assert_eq!(read.source_byte_len, 20);
    assert_released(&counts, 0);
    assert_eq!(files(dir.path()), BTreeSet::from([source.clone()]));
    for (input, output, existing, kind) in [
        (&source, &source, Existing::Replace, ErrorKind::Input),
        (&source, &source, keep(), ErrorKind::Input),
        (&destination, &destination, keep(), ErrorKind::Io),
    ] {
        let result = import_step_document::<CountingKernel>(
            &request(input, output, existing),
            || panic!("preflight must precede factory"),
            |_, _| unreachable!(),
            &OperationContext::default(),
        );
        assert_eq!(result.expect_err("operation must refuse").kind(), kind);
    }
    std::fs::write(&destination, b"busy").expect("test fixture operation");
    let error = import_step_document::<CountingKernel>(
        &request(&source, &destination, keep()),
        || panic!("no-clobber precedes factory"),
        |_, _| unreachable!(),
        &OperationContext::default(),
    )
    .expect_err("operation must refuse");
    assert_eq!(error.kind(), ErrorKind::Input);
    assert_eq!(
        std::fs::read(&destination).expect("test fixture operation"),
        b"busy"
    );
}
