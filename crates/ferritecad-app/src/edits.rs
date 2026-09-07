// SPDX-License-Identifier: MIT
//! The window's one edit, including the form and its owned worker.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use ferritecad_document::ExtrudeEditSource;
use ferritecad_jobs::{EditExtrudeRequest, EditedDocument, edit_extrude_copy};
use ferritecad_kernel::{CancelToken, OperationContext};
use ferritecad_types::{CadError, ErrorKind, Result};
use ferritecad_ui::{EditChoice, EditExtrudeForm, ExtrusionRow};

#[derive(Debug)]
struct Form {
    source: PathBuf,
    reading: ExtrudeEditSource,
    shown: EditExtrudeForm,
}

#[derive(Debug)]
struct Running {
    generation: u64,
    cancel: CancelToken,
    worker: JoinHandle<()>,
}

#[derive(Debug, Default)]
pub(crate) struct Edits {
    form: Option<Form>,
    running: Option<Running>,
    issued: u64,
    pub(crate) status: String,
}

impl Edits {
    pub(crate) fn busy(&self) -> bool {
        self.form.is_some() || self.running.is_some()
    }

    pub(crate) fn begin(&mut self, source: &Path, reading: &ExtrudeEditSource) -> bool {
        if self.busy() || reading.unavailable_reason().is_some() {
            return false;
        }
        self.form = Some(Form {
            source: source.to_path_buf(),
            reading: reading.clone(),
            shown: EditExtrudeForm {
                features: reading
                    .features
                    .iter()
                    .map(|f| ExtrusionRow {
                        feature: f.feature,
                        label: format!(
                            "{} — {}{}",
                            f.name.as_deref().unwrap_or("Extrusion"),
                            f.feature,
                            f.distance_mm
                                .map(|v| format!(" — {v} mm"))
                                .unwrap_or_default()
                        ),
                        distance_mm: f.distance_mm,
                        refusal: f.refusal.clone(),
                    })
                    .collect(),
                selected: None,
                distance: String::new(),
                refusal: None,
            },
        });
        true
    }

    pub(crate) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        can_begin: bool,
        unavailable: Option<&str>,
    ) -> EditChoice {
        ferritecad_ui::edit_extrude_panel(
            ui,
            can_begin,
            unavailable,
            self.form.as_mut().map(|f| &mut f.shown),
            self.running.is_some(),
            self.running
                .as_ref()
                .is_some_and(|r| !r.cancel.is_cancelled()),
            &self.status,
        )
    }

    pub(crate) fn cancel(&mut self) {
        self.form = None;
        if let Some(running) = &self.running {
            running.cancel.cancel();
        }
    }

    /// Parsing only. Domain validation and stale-source checks are the job's.
    pub(crate) fn request(&mut self, destination: PathBuf) -> Option<EditExtrudeRequest> {
        let form = self.form.as_mut()?;
        let feature = form.shown.selected?;
        let distance_mm = match form.shown.distance.trim().parse() {
            Ok(value) => value,
            Err(_) => {
                form.shown.refusal = Some("Enter a distance in mm.".to_owned());
                return None;
            }
        };
        Some(EditExtrudeRequest {
            source: form.source.clone(),
            expected: form.reading.version,
            feature,
            distance_mm,
            destination,
        })
    }

    pub(crate) fn start(
        &mut self,
        request: EditExtrudeRequest,
        spawn: impl FnOnce(EditExtrudeRequest, u64, CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        if self.running.is_some() {
            return None;
        }
        self.issued += 1;
        let generation = self.issued;
        let cancel = CancelToken::new();
        self.status = format!("Saving edited model to {}", request.destination.display());
        let worker = spawn(request, generation, cancel.clone());
        self.running = Some(Running {
            generation,
            cancel,
            worker,
        });
        self.form = None;
        Some(generation)
    }

    pub(crate) fn finish(
        &mut self,
        generation: u64,
        result: Result<EditedDocument>,
    ) -> Option<PathBuf> {
        if self
            .running
            .as_ref()
            .is_none_or(|r| r.generation != generation)
        {
            return None;
        }
        let running = self.running.take().expect("matching running edit");
        let _ = running.worker.join();
        match result {
            Ok(saved) => {
                self.status = format!("Saved edited model: {}", saved.destination.display());
                if !running.cancel.is_cancelled() {
                    return Some(saved.destination);
                }
            }
            Err(error) if error.kind() == ErrorKind::Cancellation => {
                self.status = "Edit cancelled; no file saved.".to_owned()
            }
            Err(error) => self.status = format!("Could not save edited model: {error}"),
        }
        None
    }

    pub(crate) fn stop_all(&mut self) {
        self.cancel();
        if let Some(running) = self.running.take() {
            let _ = running.worker.join();
        }
    }
}

/// Both construction and destruction of the native session occur in this
/// worker call, including in tests of the real UI command without clicks.
pub(crate) fn run_edit(
    request: &EditExtrudeRequest,
    context: &OperationContext,
) -> Result<EditedDocument> {
    let mut kernel = ferritecad_occt::OcctKernel::new()?;
    edit_extrude_copy(request, &mut kernel, context)
}

pub(crate) fn spawn_edit(
    request: EditExtrudeRequest,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<EditedDocument>) + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_edit(&request, &OperationContext::default().with_cancel(cancel))
        }))
        .unwrap_or_else(|_| Err(CadError::kernel("edit worker stopped unexpectedly")));
        deliver(result);
    })
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use ferritecad_document::{Document, EndCondition, ObjectPayload};
    use ferritecad_kernel::{GeometryKernel, ProgressSink};
    use std::ffi::OsStr;
    use std::sync::{Mutex, mpsc};

    fn native() -> bool {
        if ferritecad_occt::is_available() {
            return true;
        }
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: this build has no Open CASCADE (edit geometry gate)");
        false
    }
    fn cli(args: &[&OsStr]) -> std::process::Output {
        let mut binary = std::env::current_exe().expect("test binary");
        binary.pop();
        binary.pop();
        binary.push(format!("ferritecad{}", std::env::consts::EXE_SUFFIX));
        std::process::Command::new(binary)
            .args(args)
            .output()
            .expect("real CLI process")
    }
    fn run(args: &[&OsStr]) -> String {
        let result = cli(args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).expect("text")
    }
    fn make(root: &Path, sizes: [&str; 3]) -> (PathBuf, ferritecad_types::ObjectId) {
        let source = root.join("Исходная плита.fcad");
        run(&[
            "create".as_ref(),
            source.as_os_str(),
            "--sample".as_ref(),
            "--size".as_ref(),
            sizes[0].as_ref(),
            sizes[1].as_ref(),
            sizes[2].as_ref(),
            "--length-unit".as_ref(),
            "in".as_ref(),
        ]);
        // The documented public discovery route. No source or SQLite read to find UUID.
        let inspection = run(&["inspect".as_ref(), source.as_os_str()]);
        let ids: Vec<_> = inspection
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let id = fields.next()?;
                (fields.next()? == "feature.extrude").then(|| id.parse().expect("public UUID"))
            })
            .collect();
        assert_eq!(ids.len(), 1);
        (source, ids[0])
    }
    fn opened(path: &Path) -> ferritecad_scene::LoadedScene {
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("native kernel");
        ferritecad_scene::snapshot_of(
            path,
            &mut kernel,
            |k, bytes| k.import_step(bytes),
            &ferritecad_kernel::TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("ordinary native load")
    }
    fn ui_edit(
        source: &Path,
        reading: &ExtrudeEditSource,
        feature: ferritecad_types::ObjectId,
        destination: &Path,
    ) -> (Edits, PathBuf) {
        let mut edits = Edits::default();
        assert!(edits.begin(source, reading));
        let form = &mut edits.form.as_mut().expect("form").shown;
        assert_eq!(form.selected, None, "selection must be explicit");
        form.selected = Some(feature);
        form.distance = "27".into();
        let request = edits
            .request(destination.to_path_buf())
            .expect("form parsed");
        let (send, receive) = mpsc::channel();
        edits
            .start(request, move |request, generation, cancel| {
                spawn_edit(request, cancel, move |result| {
                    send.send((generation, result)).expect("deliver");
                })
            })
            .expect("start UI command");
        let (generation, result) = receive.recv().expect("worker reply");
        let path = edits
            .finish(generation, result)
            .expect("published output to Open");
        (edits, path)
    }

    /// Every SQL cell, including payload bytes and identity/link columns.
    /// Only meta.modified_at is ignored: two commits occur at different times.
    fn tables(path: &Path) -> Vec<(String, Vec<Vec<rusqlite::types::Value>>)> {
        let connection =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .expect("read raw saved content");
        let names: Vec<String> = connection
            .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
            .expect("tables")
            .query_map([], |r| r.get(0))
            .expect("rows")
            .collect::<rusqlite::Result<_>>()
            .expect("names");
        names
            .into_iter()
            .map(|name| {
                let mut query = connection
                    .prepare(&format!("SELECT * FROM \"{}\"", name.replace('"', "\"\"")))
                    .expect("query");
                let count = query.column_count();
                let modified = query
                    .column_names()
                    .iter()
                    .position(|c| *c == "modified_at");
                let mut rows: Vec<Vec<_>> = query
                    .query_map([], |row| {
                        (0..count)
                            .map(|i| {
                                if name == "meta" && Some(i) == modified {
                                    Ok(rusqlite::types::Value::Null)
                                } else {
                                    row.get(i)
                                }
                            })
                            .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                    })
                    .expect("rows")
                    .collect::<rusqlite::Result<_>>()
                    .expect("values");
                rows.sort_by_key(|row| format!("{row:?}"));
                (name, rows)
            })
            .collect()
    }
    fn stl_facts(bytes: &[u8]) -> ([f64; 3], [f64; 3], f64) {
        let count = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
        assert_eq!(bytes.len(), 84 + count * 50);
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let mut volume = 0.0;
        for triangle in bytes[84..].chunks_exact(50) {
            let mut points = [[0.0; 3]; 3];
            for (v, point) in points.iter_mut().enumerate() {
                for (axis, coordinate) in point.iter_mut().enumerate() {
                    let offset = 12 + v * 12 + axis * 4;
                    *coordinate =
                        f32::from_le_bytes(triangle[offset..offset + 4].try_into().expect("float"))
                            as f64;
                    min[axis] = min[axis].min(*coordinate);
                    max[axis] = max[axis].max(*coordinate);
                }
            }
            let [a, b, c] = points;
            volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0;
        }
        (min, max, volume.abs())
    }

    #[test]
    fn one_source_ui_and_cli_preserve_all_ids_rebuild_and_export_the_changed_model() {
        if !native() {
            return;
        }
        for size in [["80", "50", "12"], ["91", "53", "17"]] {
            let root = tempfile::tempdir().expect("outside checkout");
            let (source, feature) = make(root.path(), size);
            let original = std::fs::read(&source).expect("source bytes");
            let loaded = opened(&source);
            let reading = loaded.edit_source.as_ref().expect("same reading as scene");
            let ui = root.path().join("UI edited.fcad");
            let command = root.path().join("CLI edited.fcad");
            let (_edits, open) = ui_edit(&source, reading, feature, &ui);
            assert_eq!(open, ui);
            run(&[
                "edit-extrude".as_ref(),
                source.as_os_str(),
                "--feature".as_ref(),
                feature.to_string().as_ref(),
                "--distance-mm".as_ref(),
                "27".as_ref(),
                "--expect-version".as_ref(),
                reading.version.content.to_string().as_ref(),
                "-o".as_ref(),
                command.as_os_str(),
            ]);
            assert_eq!(
                tables(&ui),
                tables(&command),
                "all SQL cells; no UUID normalization"
            );
            let original_document = Document::open_read_only(&source).expect("original");
            let mut exports = Vec::new();
            for path in [&ui, &command] {
                let doc = Document::open_read_only(path).expect("reopen");
                assert!(doc.validate().expect("validate").is_ok());
                assert_eq!(doc.meta().document_id, original_document.meta().document_id);
                assert_eq!(doc.objects().expect("objects").len(), 4);
                assert_eq!(
                    doc.dependencies().expect("links"),
                    original_document.dependencies().expect("original links")
                );
                assert_eq!(
                    doc.topology_refs().expect("refs"),
                    original_document.topology_refs().expect("original refs")
                );
                for old in original_document.objects().expect("objects") {
                    let new = doc
                        .object(old.id)
                        .expect("object by original UUID")
                        .expect("same UUID");
                    if old.id == feature {
                        let ObjectPayload::Extrude(e) = &new.payload else {
                            panic!("not extrusion");
                        };
                        assert_eq!(
                            e.end_condition,
                            EndCondition::Blind {
                                distance: ferritecad_document::Expression::constant(27.0)
                                    .expect("literal")
                            }
                        );
                        assert_eq!(old.parent, new.parent);
                        assert_eq!(old.ordinal, new.ordinal);
                        assert_eq!(old.name, new.name);
                    } else {
                        assert_eq!(old, new, "unselected object changed");
                    }
                }
                doc.close().expect("close");
                let rebuilt = run(&["rebuild".as_ref(), path.as_os_str(), "--cold".as_ref()]);
                assert!(
                    rebuilt.contains("4 objects evaluated, 1 shape built"),
                    "{rebuilt}"
                );
                assert!(
                    rebuilt.contains("3 of 3 stored references resolved"),
                    "{rebuilt}"
                );
                opened(path);
                let stl = path.with_extension("stl");
                let fbx = path.with_extension("fbx");
                let window_fbx = path.with_extension("window.fbx");
                run(&[
                    "export-stl".as_ref(),
                    path.as_os_str(),
                    "-o".as_ref(),
                    stl.as_os_str(),
                ]);
                run(&[
                    "export-fbx".as_ref(),
                    path.as_os_str(),
                    "-o".as_ref(),
                    fbx.as_os_str(),
                ]);
                let document = path.to_path_buf();
                let destination = window_fbx.clone();
                std::thread::spawn(move || {
                    crate::exports::run_export(
                        &document,
                        &destination,
                        false,
                        &OperationContext::default(),
                    )
                })
                .join()
                .expect("export thread")
                .expect("UI export");
                let stl_bytes = std::fs::read(stl).expect("STL");
                let fbx_bytes = std::fs::read(fbx).expect("FBX");
                assert_eq!(fbx_bytes, std::fs::read(window_fbx).expect("window FBX"));
                let (min, max, volume) = stl_facts(&stl_bytes);
                let x: f64 = size[0].parse().expect("X");
                let y: f64 = size[1].parse().expect("Y");
                assert_eq!(min, [0.0, 0.0, 0.0]);
                assert_eq!(max, [x, y, 27.0]);
                assert!((volume - x * y * 27.0).abs() < 1e-6, "volume {volume}");
                exports.push((stl_bytes, fbx_bytes));
            }
            assert_eq!(
                exports[0], exports[1],
                "both copies export byte-identically, identities included"
            );
            let stl = root.path().join("original.stl");
            run(&[
                "export-stl".as_ref(),
                source.as_os_str(),
                "-o".as_ref(),
                stl.as_os_str(),
            ]);
            let (_, max, volume) = stl_facts(&std::fs::read(stl).expect("original STL"));
            let dimensions: Vec<f64> = size.iter().map(|v| v.parse().expect("size")).collect();
            assert_eq!(max, [dimensions[0], dimensions[1], dimensions[2]]);
            assert!((volume - dimensions.iter().product::<f64>()).abs() < 1e-6);
            assert_eq!(original, std::fs::read(&source).expect("source unchanged"));
        }
    }

    #[test]
    fn imported_blobs_unknown_tables_and_unselected_payloads_survive_edit() {
        if !native() {
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let (source, feature) = make(root.path(), ["83", "47", "13"]);
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/step/canonical/01-single-part.step"),
        )
        .expect("STEP fixture");
        let imported = kernel.import_step(&bytes).expect("read STEP");
        let mut document = Document::open(&source).expect("source");
        document
            .store_step_import(ferritecad_document::StepImportRequest {
                object: ferritecad_types::ObjectId::new(),
                name: Some("Stored import"),
                source: &bytes,
                source_name: Some("removed.step"),
                import: &imported,
                importer: kernel.identity(),
            })
            .expect("store import");
        document.close().expect("close");
        if let Some(scene) = imported.scene() {
            for shape in scene.shapes() {
                kernel.release(shape);
            }
        }
        let raw = rusqlite::Connection::open(&source).expect("fixture data");
        raw.execute_batch("CREATE TABLE extra_data (id INTEGER PRIMARY KEY, payload BLOB); INSERT INTO extra_data VALUES (1, X'001122FF');").expect("unknown extension table");
        drop(raw);
        let before = tables(&source);
        let bytes = std::fs::read(&source).expect("before");
        let output = root.path().join("mixed edited.fcad");
        let reading = opened(&source).edit_source.expect("accepted reading");
        ui_edit(&source, &reading, feature, &output);
        let after = tables(&output);
        for table in before.iter().filter(|(name, _)| name != "objects") {
            assert!(after.contains(table), "table {} changed", table.0);
        }
        let old = Document::open_read_only(&source).expect("old");
        let new = Document::open_read_only(&output).expect("new");
        for object in old
            .objects()
            .expect("objects")
            .into_iter()
            .filter(|o| o.id != feature)
        {
            assert_eq!(new.object(object.id).expect("object"), Some(object));
        }
        opened(&output);
        assert_eq!(bytes, std::fs::read(&source).expect("same bytes"));
    }

    #[test]
    fn cancellation_before_and_after_publish_and_stale_answers_keep_honest_outcomes() {
        if !native() {
            return;
        }
        for late in [false, true] {
            let root = tempfile::tempdir().expect("directory");
            let (source, feature) = make(root.path(), ["80", "50", "12"]);
            let reading = opened(&source).edit_source.expect("reading");
            let mut edits = Edits::default();
            edits.begin(&source, &reading);
            let form = &mut edits.form.as_mut().expect("form").shown;
            form.selected = Some(feature);
            form.distance = "27".into();
            let destination = root.path().join("cancel.fcad");
            let request = edits.request(destination.clone()).expect("request");
            let (send, recv) = mpsc::channel();
            let (ready, reached) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            let generation = edits
                .start(request, move |request, generation, cancel| {
                    std::thread::spawn(move || {
                        let resume = Mutex::new(resume);
                        let context = OperationContext::default()
                            .with_cancel(cancel)
                            .with_progress(ProgressSink::new(move |fraction| {
                                if fraction == if late { 1.0 } else { 0.95 } {
                                    ready.send(()).expect("barrier");
                                    resume.lock().expect("lock").recv().expect("resume");
                                }
                            }));
                        send.send((generation, run_edit(&request, &context)))
                            .expect("reply");
                    })
                })
                .expect("start");
            reached.recv().expect("barrier reached");
            let status = edits.status.clone();
            assert!(
                edits
                    .finish(generation + 1, Err(CadError::input("stale reply")))
                    .is_none()
            );
            assert_eq!(edits.status, status);
            assert!(edits.busy());
            edits.cancel();
            release.send(()).expect("release");
            let (generation, result) = recv.recv().expect("outcome");
            assert!(edits.finish(generation, result).is_none());
            assert_eq!(destination.exists(), late);
            assert_eq!(edits.status.starts_with("Saved edited model:"), late);
            edits.stop_all();
            assert!(!edits.busy());
            assert!(std::fs::read_dir(root.path()).expect("files").all(|e| {
                !e.expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .contains(".partial")
            }));
        }
    }
    #[test]
    fn published_output_and_open_failure_are_separate_from_the_accepted_scene() {
        if !native() {
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let (source, feature) = make(root.path(), ["80", "50", "12"]);
        let mut input = ferritecad_ui::ViewportInput::new();
        input.resize(800, 600);
        let mut scene = crate::LiveScene::new(
            None,
            (),
            Vec::new(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Vec::new(),
        );
        let prepared = crate::prepare_load(&input, &source, Ok(opened(&source)), |_, _| Ok(()));
        crate::commit_scene(&mut scene, &mut input, prepared).expect("accept original");
        let accepted = scene.edit_source.clone();
        let camera = *input.camera();
        let output = root.path().join("saved but not opened.fcad");
        let (edits, path) = ui_edit(
            scene.document.as_ref().expect("accepted path"),
            scene.edit_source.as_ref().expect("accepted version"),
            feature,
            &output,
        );
        assert_eq!(scene.document.as_ref(), Some(&source));
        let refused = crate::prepare_load(
            &input,
            &path,
            Err(CadError::rendering("deterministic Open upload refusal")),
            |_, _| Ok(()),
        );
        assert!(crate::commit_scene(&mut scene, &mut input, refused).is_err());
        assert!(output.is_file());
        assert!(edits.status.contains("Saved edited model:"));
        assert_eq!(scene.document.as_ref(), Some(&source));
        assert_eq!(scene.edit_source, accepted);
        assert_eq!(*input.camera(), camera);
        assert!(crate::can_export(&scene));
        let next = crate::prepare_load(&input, &path, Ok(opened(&path)), |_, _| Ok(()));
        crate::commit_scene(&mut scene, &mut input, next).expect("ordinary successful Open");
        assert_eq!(scene.document.as_ref(), Some(&output));
        assert_ne!(
            scene.edit_source.as_ref().expect("edited version").version,
            accepted.expect("old version").version
        );
        assert_eq!(
            scene.edit_source.as_ref().expect("edited reading").features[0].distance_mm,
            Some(27.0)
        );
    }

    #[test]
    fn cancelling_the_form_and_a_closed_save_dialog_do_no_work() {
        if !native() {
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let (source, feature) = make(root.path(), ["80", "50", "12"]);
        let before = std::fs::read(&source).expect("bytes");
        let reading = opened(&source).edit_source.expect("reading");
        let mut edits = Edits::default();
        assert!(edits.begin(&source, &reading));
        assert!(!edits.begin(&source, &reading));
        let form = &mut edits.form.as_mut().expect("form").shown;
        form.selected = Some(feature);
        form.distance = "not a number".into();
        assert!(edits.request(root.path().join("bad.fcad")).is_none());
        edits.form.as_mut().expect("form").shown.distance = "27".into();
        // The system dialog may return None after parsing. No start occurs.
        assert!(edits.request(PathBuf::new()).is_some());
        assert!(edits.running.is_none());
        assert!(edits.form.is_some());
        edits.cancel();
        assert!(!edits.busy());
        assert_eq!(before, std::fs::read(&source).expect("same source"));
        assert_eq!(std::fs::read_dir(root.path()).expect("files").count(), 1);
    }
    #[test]
    fn shutdown_cancels_and_joins_the_edit_worker_before_it_can_publish() {
        if !native() {
            return;
        }
        let root = tempfile::tempdir().expect("directory");
        let (source, feature) = make(root.path(), ["80", "50", "12"]);
        let reading = opened(&source).edit_source.expect("reading");
        let request = EditExtrudeRequest {
            source: source.clone(),
            expected: reading.version,
            feature,
            distance_mm: 27.0,
            destination: root.path().join("shutdown.fcad"),
        };
        let (ready, reached) = mpsc::channel();
        let (send, recv) = mpsc::channel();
        let mut edits = Edits::default();
        edits
            .start(request, move |request, _, cancel| {
                std::thread::spawn(move || {
                    let waiting = cancel.clone();
                    let context = OperationContext::default()
                        .with_cancel(cancel)
                        .with_progress(ProgressSink::new(move |fraction| {
                            if fraction == 0.95 {
                                ready.send(()).expect("ready");
                                let limit =
                                    std::time::Instant::now() + std::time::Duration::from_secs(5);
                                while !waiting.is_cancelled() {
                                    assert!(
                                        std::time::Instant::now() < limit,
                                        "shutdown did not cancel"
                                    );
                                    std::thread::yield_now();
                                }
                            }
                        }));
                    send.send(run_edit(&request, &context)).expect("result");
                })
            })
            .expect("start");
        reached.recv().expect("worker reached prepublish barrier");
        edits.stop_all();
        assert!(matches!(
            recv.recv().expect("finished worker"),
            Err(CadError::Cancelled)
        ));
        assert!(edits.running.is_none());
        assert_eq!(std::fs::read_dir(root.path()).expect("files").count(), 1);
    }
}
