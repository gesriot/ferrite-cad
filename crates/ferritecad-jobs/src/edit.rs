// SPDX-License-Identifier: MIT
//! Edit one existing feature in a preserved SQLite copy, then cold-check and publish.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ferritecad_document::{
    Access, Document, DocumentVersion, EndCondition, Expression, ObjectPayload, editable_extrude,
};
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
    let replacement = Expression::constant(request.distance_mm)?;
    if request.distance_mm <= 0.0 {
        return Err(CadError::input("extrude distance must be positive"));
    }
    refuse_source_as_destination(
        &request.source,
        &request.destination,
        "source and output must be different files",
    )?;
    if path_entry_exists(&request.destination)? {
        return Err(CadError::input(
            "output already exists; choose a different file name",
        ));
    }
    let source = Document::open_read_only(&request.source)?;
    require_version(&source, request.expected)?;
    if let Access::ReadOnly { reason } = source.copy_access()? {
        return Err(CadError::unsupported(format!(
            "document cannot be edited: {reason}"
        )));
    }
    let mut selected = source.object(request.feature)?.ok_or_else(|| {
        CadError::input(format!(
            "feature {} does not exist in this document",
            request.feature
        ))
    })?;
    editable_extrude(&source, &selected)?;
    let temporary = Temporary::beside(&request.destination)?;
    source.snapshot_to(temporary.path())?;
    source.close()?;
    context.progress().report(0.1);
    context.check_cancelled()?;

    let mut document = Document::open(temporary.path())?;
    // Baseline and edited refs are compared by their stored IDs. An already
    // unresolved ref may remain unresolved; a previously resolved one may not
    // be lost. Rebuild errors (including solver diagnostics) always refuse.
    let baseline = checked_rebuild(&document, kernel, &phase(context, 0.1, 0.4), None)?;
    let ObjectPayload::Extrude(extrude) = &mut selected.payload else {
        unreachable!("checked above")
    };
    extrude.end_condition = EndCondition::Blind {
        distance: replacement,
    };
    document.write(|writer| {
        writer.put_object(
            selected.id,
            selected.parent,
            selected.ordinal,
            selected.name.as_deref(),
            &selected.payload,
        )?;
        Ok(())
    })?;
    checked_rebuild(
        &document,
        kernel,
        &phase(context, 0.4, 0.9),
        Some(&baseline),
    )?;
    document.close()?;
    context.progress().report(0.95);
    context.check_cancelled()?;
    // Re-open the path, not the old SQLite handle: replacement by another
    // document while the worker was running is stale too.
    let current = Document::open_read_only(&request.source)?;
    require_version(&current, request.expected)?;
    refuse_source_as_destination(
        &request.source,
        &request.destination,
        "source and output must be different files",
    )?;
    context.check_cancelled()?;
    temporary.publish(
        &request.destination,
        Existing::Keep {
            advice: "choose a different file name",
        },
    )?;
    drop(current);
    context.progress().report(1.0);
    Ok(EditedDocument {
        destination: request.destination.clone(),
        document_id: request.expected.document_id,
        feature: request.feature,
    })
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

fn checked_rebuild<K: GeometryKernel + ?Sized>(
    document: &Document,
    kernel: &mut K,
    context: &OperationContext,
    baseline: Option<&BTreeSet<StableEntityId>>,
) -> Result<BTreeSet<StableEntityId>> {
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
        Ok(resolved)
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
    use ferritecad_document::{Dependency, DependencyRole, ExtrudeEditSource, ObjectRecord};
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
    }
    impl GeometryKernel for Refusing {
        fn identity(&self) -> &KernelIdentity {
            self.inner.identity()
        }
        fn extrude(&mut self, r: &ExtrudeRequest, c: &OperationContext) -> Result<ExtrudeResult> {
            if r.extent().total_length() == 27.0 && !self.lose_ref {
                return Err(CadError::kernel("deterministic edited geometry refusal"));
            }
            let mut result = self.inner.extrude(r, c)?;
            if r.extent().total_length() == 27.0 {
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
}
