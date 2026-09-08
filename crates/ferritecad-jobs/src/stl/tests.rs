// SPDX-License-Identifier: MIT
#![allow(clippy::panic, reason = "a forbidden operation must fail the gate")]
use super::*;
use crate::{CreateDocumentRequest, NewDocument, PlateSize, create_document};
use ferritecad_kernel::{mock::MockKernel, *};
use ferritecad_types::ErrorKind;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn source(root: &Path) -> PathBuf {
    let source = root.join("source.fcad");
    create_document(
        CreateDocumentRequest::new(
            &source,
            NewDocument::SamplePlate(PlateSize {
                width: 60.0,
                depth: 40.0,
                height: 10.0,
            }),
            "keep",
        ),
        &OperationContext::default(),
    )
    .expect("plate");
    source
}

fn request<'a>(source: &'a Path, output: &'a Path) -> StlExportRequest<'a> {
    StlExportRequest {
        document: source,
        destination: output,
        body: BodySelection::Only {
            advice: "choose one body",
        },
        params: TessellationParams::default(),
        existing: Existing::Keep { advice: "keep" },
    }
}

fn files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(root)
        .expect("dir")
        .map(|e| e.expect("entry").path())
        .collect();
    files.sort();
    files
}

#[test]
fn no_clobber_is_checked_again_at_publish_time() {
    let root = tempfile::tempdir().expect("root");
    let source = source(root.path());
    let output = root.path().join("out.stl");
    std::fs::write(&output, b"arrived during the rebuild").expect("racer");
    let error = write_and_publish(
        &request(&source, &output),
        b"new export",
        &OperationContext::default(),
    )
    .expect_err("must not overwrite");
    assert_eq!(error.kind(), ErrorKind::Input);
    assert_eq!(
        std::fs::read(&output).expect("racer"),
        b"arrived during the rebuild"
    );
    assert_eq!(files(root.path()).len(), 2, "scratch cleaned");
}

#[test]
fn publication_races_cancellation_and_aliases_preserve_files() {
    for event in ["occupied", "cancel", "late cancel", "alias"] {
        let root = tempfile::tempdir().expect("root");
        let source = source(root.path());
        let before = std::fs::read(&source).expect("source");
        let output = root.path().join("out.stl");
        let dest = output.clone();
        let from = source.clone();
        let cancel = CancelToken::new();
        let stop = cancel.clone();
        let context = OperationContext::default()
            .with_cancel(cancel)
            .with_progress(ProgressSink::new(move |fraction| {
                if fraction == 0.95 {
                    // Inspect actual scratch, not just a progress/call count.
                    let scratch: Vec<_> = files(dest.parent().expect("parent"))
                        .into_iter()
                        .filter(|p| p.is_dir())
                        .collect();
                    assert_eq!(scratch.len(), 1);
                    assert_eq!(
                        std::fs::read(scratch[0].join("payload"))
                            .expect("closed STL")
                            .len(),
                        684
                    );
                    match event {
                        "occupied" => std::fs::write(&dest, b"racer").expect("racer"),
                        "cancel" => stop.cancel(),
                        "alias" => std::fs::hard_link(&from, &dest).expect("alias"),
                        _ => {}
                    }
                }
                if fraction == 1.0 && event == "late cancel" {
                    stop.cancel();
                }
            }));
        let mut request = request(&source, &output);
        if event == "alias" {
            request.existing = Existing::Replace;
        }
        let result = export_document_as_stl(request, || Ok(MockKernel::new()), &context);
        assert_eq!(
            result.is_ok(),
            event == "late cancel",
            "{event}: {result:?}"
        );
        match event {
            "late cancel" => assert_eq!(std::fs::read(&output).expect("published").len(), 684),
            "occupied" => assert_eq!(std::fs::read(&output).expect("racer"), b"racer"),
            "alias" => assert_eq!(std::fs::read(&output).expect("alias"), before),
            "cancel" => {
                assert_eq!(result.expect_err("cancel").kind(), ErrorKind::Cancellation);
                assert!(!output.exists());
            }
            _ => unreachable!(),
        }
        assert_eq!(std::fs::read(&source).expect("source unchanged"), before);
        assert_eq!(
            files(root.path()).len(),
            if event == "cancel" { 1 } else { 2 }
        );
    }
}

/// A dropped session records the number of live shapes BEFORE its own cleanup.
/// Thus dropping an owning kernel cannot conceal missing release_all calls.
struct Observed {
    inner: MockKernel,
    fault: &'static str,
    at_drop: Arc<AtomicUsize>,
}
impl Drop for Observed {
    fn drop(&mut self) {
        self.at_drop
            .store(self.inner.live_shape_count(), Ordering::SeqCst);
    }
}
impl GeometryKernel for Observed {
    fn identity(&self) -> &KernelIdentity {
        self.inner.identity()
    }
    fn extrude(&mut self, r: &ExtrudeRequest, c: &OperationContext) -> Result<ExtrudeResult> {
        if self.fault == "rebuild" {
            return Err(CadError::kernel("rebuild refused"));
        }
        self.inner.extrude(r, c)
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
        if self.fault == "tessellation" {
            return Err(CadError::kernel("mesh refused"));
        }
        let mut mesh = self.inner.tessellate(s, p, c)?;
        if self.fault == "serialization" {
            mesh.positions[0] = f32::NAN;
        }
        if self.fault == "cancel" {
            c.cancel().cancel();
        }
        Ok(mesh)
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
fn all_geometry_exits_release_shapes_without_publishing_a_failed_export() {
    for fault in ["none", "rebuild", "tessellation", "serialization", "cancel"] {
        let root = tempfile::tempdir().expect("root");
        let source = source(root.path());
        let before = std::fs::read(&source).expect("source");
        let output = root.path().join("out.stl");
        std::fs::write(&output, b"older output").expect("sentinel");
        let mut request = request(&source, &output);
        request.existing = Existing::Replace;
        let live = Arc::new(AtomicUsize::new(usize::MAX));
        let captured = live.clone();
        let result = export_document_as_stl(
            request,
            || {
                Ok(Observed {
                    inner: MockKernel::new(),
                    fault,
                    at_drop: captured,
                })
            },
            &OperationContext::default(),
        );
        assert_eq!(result.is_ok(), fault == "none", "{fault}: {result:?}");
        assert_eq!(
            live.load(Ordering::SeqCst),
            0,
            "{fault}: live shapes at session drop"
        );
        if fault != "none" {
            assert_eq!(std::fs::read(&output).expect("sentinel"), b"older output");
        }
        assert_eq!(std::fs::read(&source).expect("source unchanged"), before);
        assert_eq!(files(root.path()).len(), 2);
    }
}

#[test]
fn preflight_and_cancel_refuse_before_opening_a_kernel() {
    let root = tempfile::tempdir().expect("root");
    let source = source(root.path());
    let before = std::fs::read(&source).expect("source");
    let output = root.path().join("out.stl");
    let cancelled = OperationContext::default();
    cancelled.cancel().cancel();
    let result = export_document_as_stl(
        request(&source, &output),
        || -> Result<MockKernel> { panic!("cancel must precede kernel") },
        &cancelled,
    );
    assert_eq!(
        result.expect_err("cancelled").kind(),
        ErrorKind::Cancellation
    );
    let mut request = request(&source, &output);
    request.body = BodySelection::Id(ObjectId::new());
    let result = export_document_as_stl(
        request,
        || -> Result<MockKernel> { panic!("choice must precede kernel") },
        &OperationContext::default(),
    );
    assert_eq!(result.expect_err("missing ID").kind(), ErrorKind::Input);
    assert_eq!(files(root.path()), vec![source.clone()]);
    assert_eq!(std::fs::read(source).expect("source unchanged"), before);
}
