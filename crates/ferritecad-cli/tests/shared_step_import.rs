// SPDX-License-Identifier: MIT
//! Direct shared job, real peer CLI, and ownership observed before kernel drop.
#![allow(clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use ferritecad_document::Document;
use ferritecad_exchange::{Import, StoredScene};
use ferritecad_jobs::{Existing, ImportStepRequest, StepImportOutcome, import_step_document};
use ferritecad_kernel::{
    ArchiveSlot, BrepBlob, CancelToken, ExtrudeRequest, ExtrudeResult, GeometryKernel,
    KernelIdentity, Mesh, OperationContext, OperationResult, ProgressSink, ShapeHandle,
    SubShapeHandle, TessellationParams,
};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{ContentHash, ErrorKind, Result, Transform};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn corpus(kind: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(kind)
        .join(name)
}
fn process(source: &Path, destination: &Path, extra: &[&str]) -> Output {
    cli()
        .arg("import-step")
        .arg(source)
        .arg("-o")
        .arg(destination)
        .args(extra)
        .output()
        .expect("test fixture operation")
}
fn inventory(directory: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    std::fs::read_dir(directory)
        .expect("test fixture operation")
        .map(|e| {
            let path = e.expect("test fixture operation").path();
            assert!(!path.is_dir(), "scratch left behind: {path:?}");
            let bytes = std::fs::read(&path).expect("test fixture operation");
            (path, bytes)
        })
        .collect()
}
fn keep() -> Existing<'static> {
    Existing::Keep {
        advice: "choose another file",
    }
}

#[derive(Default)]
struct Ownership {
    imported: BTreeSet<ShapeHandle>,
    released: BTreeSet<ShapeHandle>,
    before_destructor: Option<usize>,
}
struct ObservedKernel {
    inner: OcctKernel,
    owned: Arc<Mutex<Ownership>>,
    thread: std::thread::ThreadId,
}
impl ObservedKernel {
    fn new(owned: &Arc<Mutex<Ownership>>) -> Result<Self> {
        Ok(Self {
            inner: OcctKernel::new()?,
            owned: owned.clone(),
            thread: std::thread::current().id(),
        })
    }
    fn read(&mut self, bytes: &[u8]) -> Result<Import> {
        assert_eq!(self.thread, std::thread::current().id());
        let result = self.inner.import_step(bytes)?;
        if let Some(scene) = result.scene() {
            let mut owned = self.owned.lock().expect("ownership lock");
            owned.imported = scene.shapes().collect();
            assert_eq!(owned.imported.len(), self.inner.live_shape_count());
        } else {
            assert_eq!(self.inner.live_shape_count(), 0);
        }
        Ok(result)
    }
}
impl Drop for ObservedKernel {
    fn drop(&mut self) {
        assert_eq!(self.thread, std::thread::current().id());
        self.owned.lock().expect("ownership lock").before_destructor =
            Some(self.inner.live_shape_count());
    }
}
impl GeometryKernel for ObservedKernel {
    fn identity(&self) -> &KernelIdentity {
        self.inner.identity()
    }
    fn release(&mut self, shape: ShapeHandle) {
        assert_eq!(self.thread, std::thread::current().id());
        let mut owned = self.owned.lock().expect("ownership lock");
        assert!(owned.imported.contains(&shape), "foreign handle");
        assert!(owned.released.insert(shape), "duplicate release");
        self.inner.release(shape);
    }
    fn extrude(&mut self, r: &ExtrudeRequest, c: &OperationContext) -> Result<ExtrudeResult> {
        self.inner.extrude(r, c)
    }
    fn transform(
        &mut self,
        s: ShapeHandle,
        t: &Transform,
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
        refs: &[SubShapeHandle],
    ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
        self.inner.encode_shape_with(s, refs)
    }
    fn decode_shape_with(
        &mut self,
        b: &BrepBlob,
        slots: &[ArchiveSlot],
    ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
        self.inner.decode_shape_with(b, slots)
    }
    fn encode_shape(&mut self, s: ShapeHandle) -> Result<BrepBlob> {
        self.inner.encode_shape(s)
    }
    fn decode_shape(&mut self, b: &BrepBlob) -> Result<ShapeHandle> {
        self.inner.decode_shape(b)
    }
}
fn released(owned: &Arc<Mutex<Ownership>>) {
    let owned = owned.lock().expect("ownership lock");
    assert!(
        !owned.imported.is_empty(),
        "native geometry must really exist"
    );
    assert_eq!(owned.imported, owned.released);
    assert_eq!(
        owned.before_destructor,
        Some(0),
        "kernel destructor must not hide leaks"
    );
}

#[test]
fn native_shared_step_import_owns_geometry_and_matches_cli() {
    if let Err(error) = OcctKernel::new() {
        assert_ne!(
            std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(),
            Ok("1"),
            "{error}"
        );
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        eprintln!("skipped: this build has no Open CASCADE (shared STEP import)");
        return;
    }
    for (kind, fixture, expected_code) in [
        ("canonical", "03-nested-assembly.step", 0),
        ("canonical", "06-unicode-names.step", 0),
        ("canonical", "05-inch-units.step", 0),
        ("damaged", "02-broken-reference.step", 4),
    ] {
        let dir = tempfile::tempdir().expect("private test directory");
        let source = dir.path().join("private 零 source.step");
        let bytes = std::fs::read(corpus(kind, fixture)).expect("test fixture operation");
        std::fs::write(&source, &bytes).expect("test fixture operation");
        let source_mtime = std::fs::metadata(&source)
            .expect("test fixture operation")
            .modified()
            .expect("test fixture operation");
        let destination = dir.path().join("job.fcad");
        let owned = Arc::new(Mutex::new(Ownership::default()));
        let StepImportOutcome::Published(result) = import_step_document(
            &ImportStepRequest {
                source: &source,
                destination: &destination,
                name: Some("Тело 零\"\n"),
                existing: keep(),
            },
            || ObservedKernel::new(&owned),
            ObservedKernel::read,
            &OperationContext::default(),
        )
        .expect("test fixture operation") else {
            panic!("published")
        };
        released(&owned);
        assert_eq!(result.destination, destination);
        assert_eq!(result.read.source_hash, ContentHash::of_bytes(&bytes));
        assert_eq!(result.read.source_byte_len, bytes.len() as u64);
        assert_eq!(result.read.diagnostics.is_empty(), expected_code == 0);
        assert_eq!(
            result.source_name.as_deref(),
            Some("private 零 source.step")
        );
        let job = Document::open_read_only(&destination).expect("test fixture operation");
        assert_eq!(job.meta().document_id, result.document_id);
        let job_stored = job
            .step_import(result.object_id)
            .expect("test fixture operation")
            .expect("test fixture operation");
        assert_eq!(job_stored.source, bytes);
        assert_eq!(
            job_stored.imported.scene,
            StoredScene::V3(result.scene.clone())
        );
        assert_eq!(job_stored.imported.source, result.source);
        assert_eq!(
            job_stored.imported.diagnostics_at_import,
            result.read.diagnostics
        );
        job.close().expect("test fixture operation");

        let peer_path = dir.path().join("cli.fcad");
        let process = process(&source, &peer_path, &["--name", "Тело 零\"\n"]);
        assert_eq!(
            process.status.code(),
            Some(expected_code),
            "{}",
            String::from_utf8_lossy(&process.stderr)
        );
        assert!(process.stderr.is_empty());
        let peer = Document::open_read_only(&peer_path).expect("test fixture operation");
        assert_ne!(peer.meta().document_id, result.document_id);
        let objects = peer.objects().expect("test fixture operation");
        assert_eq!(objects.len(), 1);
        assert_ne!(objects[0].id, result.object_id);
        assert_eq!(objects[0].name.as_deref(), Some(result.name.as_str()));
        assert_eq!(objects[0].parent, None);
        assert_eq!(objects[0].ordinal, 0);
        let stored = peer
            .step_import(objects[0].id)
            .expect("test fixture operation")
            .expect("test fixture operation");
        assert_eq!(stored.source, bytes);
        assert_ne!(stored.imported.source, result.source);
        assert_eq!(stored.imported.source_hash, result.read.source_hash);
        assert_eq!(stored.imported.source_byte_len, result.read.source_byte_len);
        assert_eq!(stored.imported.source_name, result.source_name);
        assert_eq!(stored.imported.imported_by, result.read.imported_by);
        assert_eq!(
            stored.imported.diagnostics_at_import,
            result.read.diagnostics
        );
        let StoredScene::V3(mut scene) = stored.imported.scene else {
            panic!("V3")
        };
        let ids: BTreeSet<_> = scene.instances.iter().map(|i| i.occurrence).collect();
        let job_ids: BTreeSet<_> = result
            .scene
            .instances
            .iter()
            .map(|i| i.occurrence)
            .collect();
        assert_eq!(ids.len(), scene.instances.len());
        assert_eq!(job_ids.len(), result.scene.instances.len());
        assert!(ids.is_disjoint(&job_ids));
        // Explicit bijection by ordered placement; all other facts stay exact.
        for (a, b) in scene.instances.iter_mut().zip(&result.scene.instances) {
            a.occurrence = b.occurrence;
        }
        assert_eq!(scene, result.scene);
        peer.close().expect("test fixture operation");
        assert_eq!(
            std::fs::read(&source).expect("test fixture operation"),
            bytes
        );
        assert_eq!(
            std::fs::metadata(&source)
                .expect("test fixture operation")
                .modified()
                .expect("test fixture operation"),
            source_mtime
        );
        assert_eq!(inventory(dir.path()).len(), 3);
        // Only the private source is deleted. Both clients' files remain usable.
        std::fs::remove_file(&source).expect("test fixture operation");
        if expected_code == 0 {
            for (doc, name) in [(&destination, "job.fbx"), (&peer_path, "cli.fbx")] {
                let fbx = dir.path().join(name);
                let output = cli()
                    .arg("export-fbx")
                    .arg(doc)
                    .arg("-o")
                    .arg(&fbx)
                    .arg("--json")
                    .output()
                    .expect("test fixture operation");
                assert_eq!(
                    output.status.code(),
                    Some(0),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let json: serde_json::Value =
                    serde_json::from_slice(&output.stdout).expect("test fixture operation");
                assert_eq!(json["result"]["complete"], true);
                assert_eq!(
                    json["result"]["bytes"].as_u64(),
                    Some(
                        std::fs::metadata(fbx)
                            .expect("test fixture operation")
                            .len()
                    )
                );
            }
        }
    }

    for fixture in [
        "01-truncated.step",
        "03-missing-terminator.step",
        "06-duplicate-product-definition.step",
    ] {
        let dir = tempfile::tempdir().expect("private rejection directory");
        let source = dir.path().join("private.step");
        let destination = dir.path().join("refused.fcad");
        let bytes = std::fs::read(corpus("damaged", fixture)).expect("fixture bytes");
        std::fs::write(&source, &bytes).expect("private source");
        let before = inventory(dir.path());
        let owned = Arc::new(Mutex::new(Ownership::default()));
        let result = import_step_document(
            &ImportStepRequest {
                source: &source,
                destination: &destination,
                name: None,
                existing: keep(),
            },
            || ObservedKernel::new(&owned),
            ObservedKernel::read,
            &OperationContext::default(),
        )
        .expect("typed reader rejection");
        let StepImportOutcome::Rejected(read) = result else {
            panic!("must reject")
        };
        assert_eq!(read.source_hash, ContentHash::of_bytes(&bytes));
        assert_eq!(read.source_byte_len, bytes.len() as u64);
        assert!(!read.diagnostics.is_empty());
        let observed = owned.lock().expect("ownership lock");
        assert!(observed.imported.is_empty() && observed.released.is_empty());
        assert_eq!(observed.before_destructor, Some(0));
        let peer = process(&source, &destination, &[]);
        assert_eq!(peer.status.code(), Some(5));
        assert!(peer.stderr.is_empty());
        assert_eq!(inventory(dir.path()), before);
    }

    // Real geometry on error/cancellation, with the observer alive before OCCT
    // teardown. The compact source keeps this independent of the large campaign.
    for failure in ["storage", "publish", "cancel", "late-cancel"] {
        let dir = tempfile::tempdir().expect("private test directory");
        let source = dir.path().join("private.step");
        let bytes = std::fs::read(corpus("canonical", "01-single-part.step"))
            .expect("test fixture operation");
        std::fs::write(&source, &bytes).expect("test fixture operation");
        let destination = dir.path().join("out.fcad");
        let output = destination.clone();
        let folder = dir.path().to_owned();
        let token = CancelToken::new();
        let cancel = token.clone();
        let context = OperationContext::default().with_cancel(token).with_progress(ProgressSink::new(move |f| {
            if failure == "storage" && f == 0.7 {
                let scratch = std::fs::read_dir(&folder).expect("test fixture operation").map(|e| e.expect("test fixture operation").path()).find(|p| p.is_dir()).expect("test fixture operation").join("payload");
                let connection = rusqlite::Connection::open(scratch).expect("test fixture operation");
                connection.execute_batch("CREATE TRIGGER refuse_storage BEFORE INSERT ON objects BEGIN SELECT RAISE(ABORT, 'injected storage failure'); END;").expect("test fixture operation");
                connection.close().expect("test fixture operation");
            }
            if failure == "publish" && f == 0.9 { std::fs::write(&output, b"racing destination").expect("test fixture operation"); }
            if (failure == "cancel" && f == 0.6) || (failure == "late-cancel" && f == 1.0) { cancel.cancel(); }
        }));
        let owned = Arc::new(Mutex::new(Ownership::default()));
        let result = import_step_document(
            &ImportStepRequest {
                source: &source,
                destination: &destination,
                name: None,
                existing: keep(),
            },
            || ObservedKernel::new(&owned),
            ObservedKernel::read,
            &context,
        );
        released(&owned);
        if failure == "late-cancel" {
            assert!(matches!(
                result.expect("test fixture operation"),
                StepImportOutcome::Published(_)
            ));
            Document::open_read_only(&destination)
                .expect("test fixture operation")
                .close()
                .expect("test fixture operation");
        } else {
            let error = result.expect_err("operation must refuse");
            assert_eq!(
                error.kind(),
                match failure {
                    "storage" => ErrorKind::Io,
                    "publish" => ErrorKind::Input,
                    _ => ErrorKind::Cancellation,
                }
            );
            if failure == "storage" {
                assert!(
                    std::error::Error::source(&error)
                        .expect("test fixture operation")
                        .to_string()
                        .contains("injected storage failure"),
                    "{error:?}"
                );
            }
            if failure == "publish" {
                assert_eq!(
                    std::fs::read(&destination).expect("test fixture operation"),
                    b"racing destination"
                );
            } else {
                assert!(!destination.exists());
            }
        }
        assert_eq!(
            std::fs::read(source).expect("test fixture operation"),
            bytes
        );
        assert_eq!(
            inventory(dir.path()).len(),
            if destination.exists() { 2 } else { 1 }
        );
    }
    println!("FCAD_SHARED_STEP_IMPORT parity=4 rejected=3 native_cleanup=4");
}

#[test]
fn step_preflight_and_stub_refusals_preserve_sources_and_names() {
    let dir = tempfile::tempdir().expect("private test directory");
    let source = dir.path().join("private.step");
    std::fs::copy(corpus("canonical", "01-single-part.step"), &source)
        .expect("test fixture operation");
    let destination = dir.path().join("out.fcad");
    let missing = dir.path().join("missing.step");
    let before = inventory(dir.path());
    let output = process(&missing, &destination, &[]);
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("reading"),
        "I/O must precede kernel: {error}"
    );
    assert!(!error.contains("no Open CASCADE"));
    assert_eq!(inventory(dir.path()), before);
    std::fs::write(&destination, b"busy").expect("test fixture operation");
    let before = inventory(dir.path());
    let output = process(&source, &destination, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));
    assert_eq!(inventory(dir.path()), before);
    let alias = dir.path().join("alias.fcad");
    std::fs::hard_link(&source, &alias).expect("test fixture operation");
    let aliases = vec![source.clone(), alias];
    #[cfg(unix)]
    let aliases = {
        let mut aliases = aliases;
        let symlink = dir.path().join("symlink.fcad");
        std::os::unix::fs::symlink(&source, &symlink).expect("test fixture operation");
        aliases.push(symlink);
        aliases
    };
    let before = inventory(dir.path());
    for alias in aliases {
        for flags in [&[][..], &["--force"][..]] {
            let output = process(&source, &alias, flags);
            assert_eq!(output.status.code(), Some(2));
            assert!(String::from_utf8_lossy(&output.stderr).contains("STEP file cannot also"));
            assert_eq!(inventory(dir.path()), before);
        }
    }
    let output = process(&source, &destination, &["--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
    assert_eq!(inventory(dir.path()), before);
    let output = cli()
        .arg("import-step")
        .arg("--help")
        .output()
        .expect("test fixture operation");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--force"));
    // Literal flag-shaped source after -- must stay a filename.
    let shaped = dir.path().join("--json");
    std::fs::copy(&source, &shaped).expect("test fixture operation");
    let fresh = dir.path().join("flag.fcad");
    let output = cli()
        .current_dir(dir.path())
        .args(["import-step", "-o"])
        .arg(&fresh)
        .args(["--", "--json"])
        .output()
        .expect("test fixture operation");
    if let Err(error) = OcctKernel::new() {
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert_eq!(output.status.code(), Some(2));
        assert!(!fresh.exists());
        let before = inventory(dir.path());
        for flags in [&[][..], &["--force"][..]] {
            let out = if flags.is_empty() {
                dir.path().join("stub.fcad")
            } else {
                destination.clone()
            };
            let output = process(&source, &out, flags);
            assert_eq!(output.status.code(), Some(2));
            assert!(String::from_utf8_lossy(&output.stderr).contains("no Open CASCADE"));
            assert_eq!(inventory(dir.path()), before);
        }
    } else {
        assert_eq!(output.status.code(), Some(0));
        let doc = Document::open_read_only(&fresh).expect("test fixture operation");
        assert_eq!(
            doc.objects().expect("test fixture operation")[0]
                .name
                .as_deref(),
            Some("--json")
        );
        doc.close().expect("test fixture operation");
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn text_import_keeps_lossy_basename_and_explicit_name_rules() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().expect("private test directory");
    let source = dir
        .path()
        .join(std::ffi::OsString::from_vec(b"source-\xff.step".to_vec()));
    std::fs::copy(corpus("canonical", "01-single-part.step"), &source)
        .expect("test fixture operation");
    let destination = dir.path().join("out.fcad");
    let output = process(&source, &destination, &[]);
    if OcctKernel::new().is_err() {
        assert_eq!(output.status.code(), Some(2));
        assert!(!destination.exists());
        eprintln!("skipped: this build has no Open CASCADE (lossy stored import name)");
        return;
    }
    assert_eq!(output.status.code(), Some(0));
    let doc = Document::open_read_only(&destination).expect("test fixture operation");
    let object = doc.objects().expect("test fixture operation").remove(0);
    assert_eq!(object.name.as_deref(), Some("source-�"));
    assert_eq!(
        doc.step_import(object.id)
            .expect("test fixture operation")
            .expect("test fixture operation")
            .imported
            .source_name
            .as_deref(),
        Some("source-�.step")
    );
}
