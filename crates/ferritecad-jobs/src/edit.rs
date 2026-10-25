// SPDX-License-Identifier: MIT
//! Edit one existing feature in a preserved SQLite copy, then cold-check and publish.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ferritecad_document::{Access, Document, DocumentVersion};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{GeometryKernel, OperationContext, ProgressSink};
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};

use crate::{Existing, Temporary, path_entry_exists, refuse_source_as_destination};

#[derive(Debug, Clone)]
pub struct EditRequest {
    pub source: PathBuf,
    pub expected: DocumentVersion,
    pub changes: Vec<ferritecad_document::EditChange>,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedDocument {
    pub rebuild: ferritecad_eval::RebuildReport,
    pub destination: PathBuf,
    pub version: DocumentVersion,
}

pub fn edit_copy<K: GeometryKernel + ?Sized>(
    request: &EditRequest,
    kernel: &mut K,
    context: &OperationContext,
) -> Result<EditedDocument> {
    edit_copy_with(request, kernel, context, &mut EditRun::standalone())
}

pub(crate) fn edit_copy_with<K: GeometryKernel + ?Sized>(
    request: &EditRequest,
    kernel: &mut K,
    context: &OperationContext,
    run: &mut EditRun<'_, K>,
) -> Result<EditedDocument> {
    context.check_cancelled()?;
    edit_object_copy(
        &request.source,
        request.expected,
        &request.destination,
        kernel,
        context,
        run,
        &request.changes,
        |checked| {
            Ok(EditedDocument {
                rebuild: checked.rebuild,
                destination: request.destination.clone(),
                version: checked
                    .version
                    .expect("candidate rebuild records its version"),
            })
        },
    )
}

pub(crate) type Observe<'a, K> = dyn FnMut(
        &Document,
        &ferritecad_eval::RebuildResult,
        &mut K,
        &OperationContext,
    ) -> Result<ferritecad_scene::LoadedScene>
    + 'a;

pub(crate) struct EditRun<'a, K: ?Sized> {
    pub(crate) baseline: Option<&'a ferritecad_eval::RebuildFacts>,
    pub(crate) observe: Option<&'a mut Observe<'a, K>>,
    pub(crate) scene: Option<ferritecad_scene::LoadedScene>,
}

impl<K: ?Sized> EditRun<'_, K> {
    fn standalone() -> Self {
        Self {
            baseline: None,
            observe: None,
            scene: None,
        }
    }
}

/// Shared snapshot, transaction, cold references, cancellation and publication.
/// All owned completion facts are prepared fallibly BEFORE publication.
#[allow(clippy::too_many_arguments)]
fn edit_object_copy<K: GeometryKernel + ?Sized, T>(
    source_path: &Path,
    expected: DocumentVersion,
    destination: &Path,
    kernel: &mut K,
    context: &OperationContext,
    run: &mut EditRun<'_, K>,
    changes: &[ferritecad_document::EditChange],
    complete: impl FnOnce(Checked) -> Result<T>,
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
    let temporary = Temporary::beside(destination)?;
    source.snapshot_to(temporary.path())?;
    source.close()?;
    context.progress().report(0.1);
    context.check_cancelled()?;

    let mut document = Document::open(temporary.path())?;
    // Baseline and edited refs are compared by their stored IDs. An already
    // unresolved ref may remain unresolved; a previously resolved one may not
    // be lost. Names produced by already unbuilt features do not block repair.
    let computed;
    let facts = match run.baseline {
        Some(facts) => facts,
        None => {
            computed = checked_rebuild(
                &document,
                kernel,
                &phase(context, 0.1, 0.4),
                None,
                None,
                None,
            )?
            .0;
            &computed
        }
    };
    let baseline_unbuilt = &facts.unbuilt;
    let baseline = &facts.resolved;
    context.progress().report(0.4);
    context.check_cancelled()?;
    let minted: BTreeSet<_> = document.write_changes(changes)?.into_iter().collect();
    let required: BTreeSet<_> = baseline.union(&minted).copied().collect();
    let (_, mut checked) = checked_rebuild(
        &document,
        kernel,
        &phase(context, 0.4, 0.9),
        Some(&required),
        Some(baseline_unbuilt),
        run.observe.as_deref_mut(),
    )?;
    run.scene = checked.scene.take();
    let completed = complete(checked)?;
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

/// What the rebuild of a written copy found, for its completion.
#[derive(Debug, Default)]
struct Checked {
    version: Option<DocumentVersion>,
    scene: Option<ferritecad_scene::LoadedScene>,
    rebuild: ferritecad_eval::RebuildReport,
}

#[allow(clippy::too_many_arguments)]
fn checked_rebuild<K: GeometryKernel + ?Sized>(
    document: &Document,
    kernel: &mut K,
    context: &OperationContext,
    baseline: Option<&BTreeSet<StableEntityId>>,
    baseline_unbuilt: Option<&BTreeSet<ObjectId>>,
    observe: Option<&mut Observe<'_, K>>,
) -> Result<(ferritecad_eval::RebuildFacts, Checked)> {
    let rebuilding = if observe.is_some() {
        phase(context, 0.0, 0.7)
    } else {
        context.clone()
    };
    let built = rebuild_cold(document, kernel, &rebuilding)?;
    let rebuild = built.report();
    let unbuilt = rebuild.unbuilt();
    let references = match document.topology_refs() {
        Ok(references) => references,
        Err(error) => {
            built.release_all(kernel);
            return Err(error);
        }
    };
    let mut resolved = BTreeSet::new();
    let mut lost = std::collections::BTreeMap::new();
    for reference in references {
        match built.resolve(&reference) {
            Ok(found) if !found.is_empty() => {
                resolved.insert(reference.id);
            }
            outcome if baseline.is_some_and(|set| set.contains(&reference.id)) => {
                lost.insert(
                    reference.id,
                    outcome.err().unwrap_or_else(|| {
                        CadError::topology(format!("edit lost reference {}", reference.id))
                    }),
                );
            }
            _ => {}
        }
    }
    if let Some(required) = baseline {
        for id in required.difference(&resolved) {
            lost.entry(*id)
                .or_insert_with(|| CadError::topology(format!("edit lost reference {id}")));
        }
    }
    let built = match baseline_unbuilt {
        Some(baseline) => built
            .require_preserved(baseline, lost, kernel)
            .map_err(|error| {
                let names = document
                    .objects()
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter(|object| unbuilt.contains(&object.id) && !baseline.contains(&object.id))
                    .filter_map(|object| object.name.map(|name| format!("{name} ({})", object.id)))
                    .collect::<Vec<_>>();
                if names.is_empty() {
                    return error;
                }
                CadError::Context {
                    kind: error.kind(),
                    message: format!("{}: {error}", names.join(", ")),
                    source: Box::new(error),
                }
            })?,
        None => built,
    };
    let result = (|| {
        Ok((
            ferritecad_eval::RebuildFacts {
                model: document.model_without_checkpoints()?,
                resolved,
                unbuilt: unbuilt.clone(),
            },
            Checked {
                version: Some(DocumentVersion {
                    document_id: document.meta().document_id,
                    content: document.content_version()?,
                }),
                scene: observe
                    .map(|observe| observe(document, &built, kernel, &phase(context, 0.7, 1.0)))
                    .transpose()?,
                rebuild,
            },
        ))
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
        Dependency, DependencyRole, EndCondition, Expression, ExtrudeEditSource, ObjectPayload,
        ObjectRecord,
    };
    use ferritecad_kernel::{mock::MockKernel, *};

    fn fixture() -> (tempfile::TempDir, EditRequest) {
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
        let request = EditRequest {
            source,
            expected: reading.version,

            changes: vec![
                ferritecad_document::EditChange::constant(
                    reading.expression_slots[0].object,
                    ferritecad_document::ExpressionSlot::ExtrudeDistance,
                    27.0,
                )
                .expect("test value"),
            ],
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
    fn replace(request: &mut EditRequest, change: impl FnOnce(&mut ObjectRecord)) {
        let mut doc = Document::open(&request.source).expect("source");
        let mut object = doc
            .object(request.changes[0].object())
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
        request: &EditRequest,
        context: &OperationContext,
        kind: ferritecad_types::ErrorKind,
    ) {
        let before = std::fs::read(&request.source).expect("bytes");
        let files = entries(request.source.parent().expect("parent"));
        let mut kernel = MockKernel::new();
        let error = edit_copy(request, &mut kernel, context).expect_err("refused");
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
            .object(request.changes[0].object())
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
        edit_copy(&request, &mut kernel, &OperationContext::default()).expect("edit");
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
            if old.id == request.changes[0].object() {
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
    fn one_refusal_reports_every_new_failure_and_lost_reference() {
        let (_root, mut request) = fixture();
        let mut d = Document::open(&request.source).expect("source");
        let first = d
            .object(request.changes[0].object())
            .expect("object")
            .expect("extrusion");
        let second = ObjectId::new();
        let ObjectPayload::Extrude(extrude) = &first.payload else {
            panic!("extrusion");
        };
        d.write(|w| {
            w.put_object(
                second,
                first.parent,
                20,
                Some("Second extrusion"),
                &first.payload,
            )?;
            w.add_dependency(Dependency {
                dependent: second,
                dependency: extrude.profile,
                role: DependencyRole::Profile,
            })
        })
        .expect("independent feature");
        d.close().expect("close");
        request.expected = read_extrude_source(&request.source)
            .expect("reading")
            .version;
        request.changes.push(
            ferritecad_document::EditChange::constant(
                second,
                ferritecad_document::ExpressionSlot::ExtrudeDistance,
                27.0,
            )
            .expect("height"),
        );
        let before = std::fs::read(&request.source).expect("before");
        let mut kernel = Refusing {
            inner: MockKernel::new(),
            lose_ref: false,
            missing_cap: None,
            sketch_fault: false,
        };
        let error =
            edit_copy(&request, &mut kernel, &OperationContext::default()).expect_err("both fail");
        let refusal =
            ferritecad_eval::RebuildRefusal::from_error(&error).expect("aggregate refusal");
        for id in [first.id, second] {
            assert!(refusal.report.failed.iter().any(|f| f.id == id), "{error}");
        }
        assert!(
            !refusal.lost_references.is_empty(),
            "reference losses accompany feature failures"
        );
        assert_eq!(kernel.inner.live_shape_count(), 0);
        assert!(!request.destination.exists());
        assert_eq!(std::fs::read(&request.source).expect("unchanged"), before);
    }

    #[test]
    fn changes_are_ordered_and_payload_validation_checks_the_final_candidate() {
        let (_root, mut request) = fixture();
        let mut intermediate = request.changes[0].clone();
        if let ferritecad_document::EditChange::SetExpression { expression, .. } = &mut intermediate
        {
            *expression = Expression::constant(0.0).expect("finite intermediate value");
        }
        request.changes.insert(0, intermediate);
        edit_copy(
            &request,
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("final height is valid");
        let saved = Document::open_read_only(&request.destination)
            .expect("copy")
            .expression_slots()
            .expect("slots");
        assert_eq!(
            saved
                .iter()
                .find(|s| s.object == request.changes[0].object())
                .expect("height")
                .expression
                .value(),
            27.0
        );
        std::fs::remove_file(&request.destination).expect("remove owned output");
        request.changes.reverse();
        refused(
            &request,
            &OperationContext::default(),
            ferritecad_types::ErrorKind::Input,
        );
    }

    #[test]
    fn invalid_inputs_and_missing_slots_leave_all_files_unchanged() {
        for value in [0.0, -3.0] {
            let (_root, mut request) = fixture();
            if let ferritecad_document::EditChange::SetExpression { expression, .. } =
                &mut request.changes[0]
            {
                *expression = Expression::constant(value).expect("finite");
            }
            refused(
                &request,
                &OperationContext::default(),
                ferritecad_types::ErrorKind::Input,
            );
        }
        let (_root, mut request) = fixture();
        if let ferritecad_document::EditChange::SetExpression { object, .. } =
            &mut request.changes[0]
        {
            *object = ObjectId::new();
        }
        refused(
            &request,
            &OperationContext::default(),
            ferritecad_types::ErrorKind::Input,
        );
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
            let result = edit_copy(&request, &mut MockKernel::new(), &context);
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
        missing_cap: Option<ShapeHandle>,
        sketch_fault: bool,
    }
    impl GeometryKernel for Refusing {
        /// Delegated: this double is about something else, and a cut it
        /// answered differently would be a second kernel.
        fn cut(
            &mut self,
            request: &ferritecad_kernel::CutRequest,

            context: &OperationContext,
        ) -> Result<ferritecad_kernel::OperationResult> {
            self.inner.cut(request, context)
        }

        fn shape_topology(
            &mut self,
            shape: ShapeHandle,
        ) -> Result<ferritecad_kernel::ShapeTopology> {
            self.inner.shape_topology(shape)
        }
        fn naming_history(
            &mut self,
            shape: ShapeHandle,
        ) -> Result<ferritecad_kernel::NamingHistory> {
            let mut history = self.inner.naming_history(shape)?;
            if self.missing_cap == Some(shape) {
                history
                    .relations
                    .retain(|(i, _, _)| *i != ferritecad_kernel::NamingInput::Cap { end: true });
            }
            Ok(history)
        }
        fn identity(&self) -> &KernelIdentity {
            self.inner.identity()
        }
        fn extrude(&mut self, r: &ExtrudeRequest, c: &OperationContext) -> Result<OperationResult> {
            let refuse = r.extent().total_length() == 27.0
                || (self.sketch_fault && self.inner.extrude_count() == 1);
            if refuse && !self.lose_ref {
                return Err(CadError::kernel("deterministic edited geometry refusal"));
            }
            let result = self.inner.extrude(r, c)?;
            if refuse {
                self.missing_cap = Some(result.shape);
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
                missing_cap: None,
                sketch_fault: false,
            };
            let error = edit_copy(&request, &mut kernel, &OperationContext::default())
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
        assert!(reading.expression_slots.is_empty());
        assert!(reading.unavailable_reason().is_some());
    }

    #[test]
    fn parameter_dependencies_are_not_silently_rewritten() {
        let (_root, mut request) = fixture();
        let mut document = Document::open(&request.source).expect("source");
        document
            .write(|writer| {
                let parameter = ObjectId::new();
                writer.put_object(
                    parameter,
                    None,
                    50,
                    Some("Height"),
                    &ObjectPayload::Parameter(ferritecad_document::Parameter {
                        name: "height".into(),
                        dimension: ferritecad_types::Dimension::Length,
                        expression: Expression::constant(13.0)?,
                    }),
                )?;
                writer.add_dependency(Dependency {
                    dependent: request.changes[0].object(),
                    dependency: parameter,
                    role: DependencyRole::Parameter,
                })?;
                Ok(())
            })
            .expect("parameter fixture");
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

    #[test]
    fn unreadable_objects_refuse_copy_before_rebuild_or_publication() {
        for unknown_type in [false, true] {
            let (_root, request) = fixture();
            let document = Document::open_read_only(&request.source).expect("source");
            let record = document
                .object(request.changes[0].object())
                .expect("row")
                .expect("feature");
            let encoded = record.payload.to_storage_bytes().expect("payload");
            document.close().expect("close");
            let mut bytes = std::fs::read(&request.source).expect("bytes");
            if unknown_type {
                let old = b"feature.extrude";
                let offsets: Vec<_> = bytes
                    .windows(old.len())
                    .enumerate()
                    .filter_map(|(index, value)| (value == old).then_some(index))
                    .collect();
                assert!(!offsets.is_empty());
                for index in offsets {
                    bytes[index..index + old.len()].copy_from_slice(b"feature.unknown");
                }
            } else {
                let index = bytes
                    .windows(encoded.len())
                    .position(|value| value == encoded)
                    .expect("SQLite holds the encoded envelope");
                bytes[index] = 0xff;
            }
            std::fs::write(&request.source, bytes).expect("structural damage");
            refused(
                &request,
                &OperationContext::default(),
                ferritecad_types::ErrorKind::Input,
            );
            assert!(!request.destination.exists());
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
            edit_copy(&request, &mut MockKernel::new(), &context),
            Err(CadError::Cancelled)
        ));
        let mut expected = vec![request.source.clone(), foreign.clone()];
        expected.sort();
        assert_eq!(entries(root.path()), expected);
        assert_eq!(std::fs::read(&request.source).expect("source"), before);
        assert_eq!(std::fs::read(foreign).expect("foreign"), b"foreign cache");
    }
    fn sketch_fixture() -> (tempfile::TempDir, EditRequest) {
        let (root, mut request) = fixture();
        let d = Document::open_read_only(&request.source).expect("document");
        let (id, sketch) = ferritecad_fixtures::sketch(&d).expect("sketch");
        let vertices = sketch
            .curves
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let ferritecad_document::SketchGeometry::Line { start, .. } = c.geometry else {
                    unreachable!("fixture lines")
                };
                ferritecad_document::SketchVertex {
                    curve_id: c.id,
                    start_mm: [if i == 1 || i == 2 { 100. } else { start.x }, start.y],
                }
            })
            .collect::<Vec<_>>();
        request.changes =
            ferritecad_document::move_sketch_vertices(id, &sketch, &vertices).expect("changes");
        (root, request)
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
        let sketch_id = request.changes[0].object();
        let mut kernel = MockKernel::new();
        let saved =
            edit_copy(&request, &mut kernel, &OperationContext::default()).expect("published");
        assert_eq!(
            kernel.live_shape_count(),
            0,
            "released before kernel destructor"
        );
        assert_eq!(kernel.extrude_count(), 2, "two cold checks");
        assert_eq!(saved.version.document_id, source.meta().document_id);
        let copy = Document::open_read_only(&saved.destination).expect("copy");
        assert_eq!(copy.meta().document_id, source.meta().document_id);
        assert_eq!(
            copy.meta().display_length_unit,
            source.meta().display_length_unit
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
            if o.id == sketch_id {
                assert_eq!(actual.id, o.id);
                assert_eq!(actual.name, o.name);
                assert_eq!(actual.ordinal, o.ordinal);
                assert_eq!(actual.parent, o.parent);
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
        let probe = edit_copy(
            &request,
            &mut MockKernel::new(),
            &OperationContext::default(),
        );
        if probe
            .as_ref()
            .is_err_and(|e| e.kind() == ferritecad_types::ErrorKind::Unsupported)
        {
            ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
            return;
        }
        probe.expect("constraint solve with test kernel");
        copy_faults(true);
    }
    fn constraint_request(old: &EditRequest) -> EditRequest {
        let d = Document::open_read_only(&old.source).expect("document");
        let (id, sketch) = ferritecad_fixtures::sketch(&d).expect("sketch");
        let edits = ferritecad_document::SketchConstraintEdits {
            remove: vec![],
            add: vec![ferritecad_document::AddSketchConstraint::Line(
                ferritecad_document::AddLineConstraint::Line {
                    curve: sketch.curves[0].id,
                    kind: ferritecad_document::LineConstraintKind::Horizontal,
                },
            )],
        };
        EditRequest {
            changes: ferritecad_document::sketch_constraint_changes(id, &sketch, &edits)
                .expect("changes"),
            ..old.clone()
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
                missing_cap: None,
                sketch_fault: matches!(case, "geometry" | "references"),
            };
            // Reuse the fault seam after a successful baseline rebuild: the
            // second extrusion, with the changed profile, fails or loses a ref.
            let result = if constraints {
                edit_copy(&constraint_request(&request), &mut kernel, &context).map(|_| ())
            } else {
                edit_copy(&request, &mut kernel, &context).map(|_| ())
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
