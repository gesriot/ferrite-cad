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
    retained_form: Option<Form>,
    published_form: Option<(PathBuf, Form)>,
    running: Option<Running>,
    issued: u64,
    pub(crate) status: String,
}

impl Edits {
    pub(crate) fn accepts(&self, generation: u64) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.generation == generation)
    }
    pub(crate) fn running(&self) -> bool {
        self.running.is_some()
    }

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
                        context: f.cut_history.as_ref().map(|h| format!(
                            "Base of {} circular Cuts. Blind depths stay fixed; Through all follows the plate thickness. Saved pocket floors must stay inside the plate.", h.tools.len())),
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
        if let Some(form) = &mut self.form {
            Self::validate_form(form);
        }
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
        self.retained_form = None;
        self.published_form = None;
        if let Some(running) = &self.running {
            running.cancel.cancel();
        }
    }

    fn validate_form(form: &mut Form) -> Option<f64> {
        let feature = form.shown.selected?;
        let result = (|| {
            let distance_mm = form
                .shown
                .distance
                .trim()
                .parse::<f64>()
                .map_err(|_| CadError::input("Enter a distance in mm."))?;
            let selected = form
                .reading
                .features
                .iter()
                .find(|f| f.feature == feature)
                .ok_or_else(|| {
                    CadError::input("selected extrusion is not in the accepted catalogue")
                })?;
            if selected.refusal.is_some() {
                return Err(CadError::unsupported("selected extrusion is unavailable"));
            }
            selected.validate_distance(distance_mm)?;
            Ok(distance_mm)
        })();
        form.shown.refusal = result.as_ref().err().map(ToString::to_string);
        result.ok()
    }

    pub(crate) fn request(&mut self, destination: PathBuf) -> Option<EditExtrudeRequest> {
        let form = self.form.as_mut()?;
        let distance_mm = Self::validate_form(form)?;
        Some(EditExtrudeRequest {
            source: form.source.clone(),
            expected: form.reading.version,
            feature: form.shown.selected?,
            distance_mm,
            destination,
        })
    }

    /// Called by the same current-load commit path as the other edit drafts.
    pub(crate) fn draft_load_finished(&mut self, path: &Path, accepted: bool) {
        if accepted {
            self.published_form = None;
        } else if self.published_form.as_ref().is_some_and(|(p, _)| p == path) {
            self.form = self.published_form.take().map(|(_, f)| f);
        }
    }

    pub(crate) fn start(
        &mut self,
        request: EditExtrudeRequest,
        spawn: impl FnOnce(EditExtrudeRequest, u64, CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        let destination = request.destination.clone();
        self.start_at(&destination, |generation, cancel| {
            spawn(request, generation, cancel)
        })
    }

    pub(crate) fn start_sketch(
        &mut self,
        request: ferritecad_jobs::EditSketchRequest,
        spawn: impl FnOnce(ferritecad_jobs::EditSketchRequest, u64, CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        let destination = request.destination.clone();
        self.start_at(&destination, |generation, cancel| {
            spawn(request, generation, cancel)
        })
    }
    pub(crate) fn start_circle(
        &mut self,
        request: ferritecad_jobs::EditCircleRequest,
        spawn: impl FnOnce(ferritecad_jobs::EditCircleRequest, u64, CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        let destination = request.destination.clone();
        self.start_at(&destination, |generation, cancel| {
            spawn(request, generation, cancel)
        })
    }
    pub(crate) fn finish_circle(
        &mut self,
        generation: u64,
        result: Result<ferritecad_jobs::EditedCircle>,
    ) -> Option<PathBuf> {
        self.finish_path(generation, result.map(|r| r.destination))
    }
    pub(crate) fn start_annulus(
        &mut self,
        request: ferritecad_jobs::EditAnnulusRequest,
        spawn: impl FnOnce(ferritecad_jobs::EditAnnulusRequest, u64, CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        let destination = request.destination.clone();
        self.start_at(&destination, |generation, cancel| {
            spawn(request, generation, cancel)
        })
    }
    pub(crate) fn finish_annulus(
        &mut self,
        generation: u64,
        result: Result<ferritecad_jobs::EditedAnnulus>,
    ) -> Option<PathBuf> {
        self.finish_path(generation, result.map(|r| r.destination))
    }
    pub(crate) fn start_revolve_angle(
        &mut self,
        request: ferritecad_jobs::EditRevolveAngleRequest,
        spawn: impl FnOnce(ferritecad_jobs::EditRevolveAngleRequest, u64, CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        let destination = request.destination.clone();
        self.start_at(&destination, |generation, cancel| {
            spawn(request, generation, cancel)
        })
    }
    pub(crate) fn start_cut(
        &mut self,
        request: ferritecad_jobs::CircularCutRequest,
        spawn: impl FnOnce(ferritecad_jobs::CircularCutRequest, u64, CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        let destination = request.destination.clone();
        self.start_at(&destination, |generation, cancel| {
            spawn(request, generation, cancel)
        })
    }
    pub(crate) fn finish_cut(
        &mut self,
        generation: u64,
        result: Result<ferritecad_jobs::AddedCircularCut>,
    ) -> Option<PathBuf> {
        self.finish_path(generation, result.map(|r| r.destination))
    }
    pub(crate) fn start_cut_edit(
        &mut self,
        request: ferritecad_jobs::EditCircularCutRequest,
        spawn: impl FnOnce(ferritecad_jobs::EditCircularCutRequest, u64, CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        let destination = request.destination.clone();
        self.start_at(&destination, |generation, cancel| {
            spawn(request, generation, cancel)
        })
    }
    pub(crate) fn finish_cut_edit(
        &mut self,
        generation: u64,
        result: Result<ferritecad_jobs::EditedCircularCut>,
    ) -> Option<PathBuf> {
        self.finish_path(generation, result.map(|r| r.destination))
    }
    pub(crate) fn start_constraints(
        &mut self,
        request: ferritecad_jobs::EditSketchConstraintsRequest,
        spawn: impl FnOnce(
            ferritecad_jobs::EditSketchConstraintsRequest,
            u64,
            CancelToken,
        ) -> JoinHandle<()>,
    ) -> Option<u64> {
        let destination = request.destination.clone();
        self.start_at(&destination, |generation, cancel| {
            spawn(request, generation, cancel)
        })
    }
    pub(crate) fn finish_constraints(
        &mut self,
        generation: u64,
        result: Result<ferritecad_jobs::EditedSketchConstraints>,
    ) -> Option<PathBuf> {
        self.finish_path(generation, result.map(|r| r.destination))
    }
    fn start_at(
        &mut self,
        destination: &Path,
        spawn: impl FnOnce(u64, CancelToken) -> JoinHandle<()>,
    ) -> Option<u64> {
        if self.running.is_some() {
            return None;
        }
        self.issued += 1;
        let generation = self.issued;
        let cancel = CancelToken::new();
        self.status = format!("Saving edited model to {}", destination.display());
        let worker = spawn(generation, cancel.clone());
        self.running = Some(Running {
            generation,
            cancel,
            worker,
        });
        self.retained_form = self.form.take();
        Some(generation)
    }

    pub(crate) fn finish(
        &mut self,
        generation: u64,
        result: Result<EditedDocument>,
    ) -> Option<PathBuf> {
        self.finish_path(generation, result.map(|r| r.destination))
    }
    pub(crate) fn finish_sketch(
        &mut self,
        generation: u64,
        result: Result<ferritecad_jobs::EditedSketch>,
    ) -> Option<PathBuf> {
        self.finish_path(generation, result.map(|r| r.destination))
    }
    fn finish_path(&mut self, generation: u64, result: Result<PathBuf>) -> Option<PathBuf> {
        if self
            .running
            .as_ref()
            .is_none_or(|r| r.generation != generation)
        {
            return None;
        }
        let running = self.running.take().expect("matching running edit");
        let _ = running.worker.join();
        if result.is_err() {
            self.form = self.retained_form.take();
        }
        match result {
            Ok(saved) => {
                self.published_form = self.retained_form.take().map(|form| (saved.clone(), form));
                self.status = format!("Saved edited model: {}", saved.display());
                if !running.cancel.is_cancelled() {
                    return Some(saved);
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
    spawn_job(cancel, move |context| run_edit(&request, context), deliver)
}

pub(crate) fn spawn_sketch_edit(
    request: ferritecad_jobs::EditSketchRequest,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ferritecad_jobs::EditedSketch>) + Send + 'static,
) -> JoinHandle<()> {
    spawn_job(
        cancel,
        move |context| {
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ferritecad_jobs::edit_sketch_copy(&request, &mut kernel, context)
        },
        deliver,
    )
}
pub(crate) fn spawn_circle_edit(
    request: ferritecad_jobs::EditCircleRequest,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ferritecad_jobs::EditedCircle>) + Send + 'static,
) -> JoinHandle<()> {
    spawn_job(
        cancel,
        move |context| {
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ferritecad_jobs::edit_circle_copy(&request, &mut kernel, context)
        },
        deliver,
    )
}
pub(crate) fn spawn_annulus_edit(
    request: ferritecad_jobs::EditAnnulusRequest,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ferritecad_jobs::EditedAnnulus>) + Send + 'static,
) -> JoinHandle<()> {
    spawn_job(
        cancel,
        move |context| {
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ferritecad_jobs::edit_annulus_copy(&request, &mut kernel, context)
        },
        deliver,
    )
}
pub(crate) fn spawn_revolve_angle_edit(
    request: ferritecad_jobs::EditRevolveAngleRequest,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<EditedDocument>) + Send + 'static,
) -> JoinHandle<()> {
    spawn_job(
        cancel,
        move |context| {
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ferritecad_jobs::edit_revolve_angle_copy(&request, &mut kernel, context)
        },
        deliver,
    )
}
pub(crate) fn spawn_cut(
    request: ferritecad_jobs::CircularCutRequest,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ferritecad_jobs::AddedCircularCut>) + Send + 'static,
) -> JoinHandle<()> {
    spawn_job(
        cancel,
        move |context| {
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ferritecad_jobs::circular_cut_copy(&request, &mut kernel, context)
        },
        deliver,
    )
}
pub(crate) fn spawn_cut_edit(
    request: ferritecad_jobs::EditCircularCutRequest,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ferritecad_jobs::EditedCircularCut>) + Send + 'static,
) -> JoinHandle<()> {
    spawn_job(
        cancel,
        move |context| {
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ferritecad_jobs::edit_circular_cut_copy(&request, &mut kernel, context)
        },
        deliver,
    )
}
pub(crate) fn spawn_constraint_edit(
    request: ferritecad_jobs::EditSketchConstraintsRequest,
    cancel: CancelToken,
    deliver: impl FnOnce(Result<ferritecad_jobs::EditedSketchConstraints>) + Send + 'static,
) -> JoinHandle<()> {
    spawn_job(
        cancel,
        move |context| {
            let mut kernel = ferritecad_occt::OcctKernel::new()?;
            ferritecad_jobs::edit_sketch_constraints_copy(&request, &mut kernel, context)
        },
        deliver,
    )
}
fn spawn_job<T: Send + 'static>(
    cancel: CancelToken,
    job: impl FnOnce(&OperationContext) -> Result<T> + Send + 'static,
    deliver: impl FnOnce(Result<T>) + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            job(&OperationContext::default().with_cancel(cancel))
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
    fn hidden_row_identity_and_stored_triggers_refuse_publication() {
        if !native() {
            return;
        }
        for trigger in [false, true] {
            let root = tempfile::tempdir().expect("directory");
            let (source, feature) = make(root.path(), ["80", "50", "12"]);
            let sql = rusqlite::Connection::open(&source).expect("fixture connection");
            sql.execute_batch(if trigger {
                "CREATE TRIGGER extension_write AFTER UPDATE OF payload ON objects BEGIN \
                 UPDATE objects SET name = 'unexpected rename' WHERE kind = 'body'; END;"
            } else {
                "CREATE TABLE extension(rowid TEXT, value BLOB); \
                 INSERT INTO extension(_rowid_, rowid, value) VALUES (1, 'kept', X'0102');"
            })
            .expect("extension");
            let reading = opened(&source).edit_source.expect("accepted reading");
            let reason = if trigger {
                "extension_write"
            } else {
                "source has changed"
            };
            if trigger {
                let mut edits = Edits::default();
                assert!(
                    !edits.begin(&source, &reading),
                    "UI must refuse before Save"
                );
                assert!(
                    reading
                        .unavailable_reason()
                        .expect("named refusal")
                        .contains(reason)
                );
            } else {
                sql.execute("UPDATE extension SET _rowid_ = 42", [])
                    .expect("change only row identity");
            }
            let before = std::fs::read(&source).expect("source before rejected edit");
            let destination = root.path().join("refused.fcad");
            let result = cli(&[
                "edit-extrude".as_ref(),
                source.as_os_str(),
                "--feature".as_ref(),
                feature.to_string().as_ref(),
                "--distance-mm".as_ref(),
                "27".as_ref(),
                "--expect-version".as_ref(),
                reading.version.content.to_string().as_ref(),
                "-o".as_ref(),
                destination.as_os_str(),
            ]);
            assert_eq!(result.status.code(), Some(2));
            assert!(
                String::from_utf8_lossy(&result.stderr).contains(reason),
                "{result:?}"
            );
            assert!(!destination.exists());
            assert_eq!(std::fs::read(&source).expect("source"), before);
            if !trigger {
                // Also change only rowid after the worker has built the copy:
                // the final source recheck must see it before publication.
                let current = opened(&source).edit_source.expect("fresh reading");
                let request = EditExtrudeRequest {
                    source: source.clone(),
                    expected: current.version,
                    feature,
                    distance_mm: 27.0,
                    destination,
                };
                let changing_source = source.clone();
                let context =
                    OperationContext::default().with_progress(ProgressSink::new(move |fraction| {
                        if fraction == 0.95 {
                            rusqlite::Connection::open(&changing_source)
                                .expect("writer")
                                .execute("UPDATE extension SET _rowid_ = 77", [])
                                .expect("late rowid change");
                        }
                    }));
                let error = run_edit(&request, &context).expect_err("late change must refuse");
                assert!(error.to_string().contains(reason), "{error}");
                assert!(!request.destination.exists());
            }
            assert_eq!(
                std::fs::read_dir(root.path()).expect("only source").count(),
                1
            );
        }
    }

    #[test]
    fn foreign_key_actions_refuse_publication_without_changing_extension_data() {
        if !native() {
            return;
        }
        for extension in [
            "CREATE UNIQUE INDEX extension_key ON objects(payload_hash); \
             CREATE TABLE extension(value BLOB REFERENCES objects(payload_hash) ON UPDATE CASCADE); \
             INSERT INTO extension SELECT payload_hash FROM objects WHERE kind = 'feature.extrude';",
            "CREATE TABLE extension(value TEXT REFERENCES capabilities(name) ON DELETE CASCADE); \
             INSERT INTO extension SELECT name FROM capabilities;",
        ] {
            let root = tempfile::tempdir().expect("directory");
            let (source, feature) = make(root.path(), ["80", "50", "12"]);
            rusqlite::Connection::open(&source)
                .expect("SQL")
                .execute_batch(extension)
                .expect("extension");
            let before = std::fs::read(&source).expect("source bytes");
            let reading = opened(&source).edit_source.expect("readable model");
            assert!(!Edits::default().begin(&source, &reading));
            let reason = reading.unavailable_reason().expect("named refusal");
            assert!(
                reason.contains("extension") && reason.contains("foreign-key action"),
                "{reason}"
            );
            let output = root.path().join("refused.fcad");
            let result = cli(&[
                "edit-extrude".as_ref(),
                source.as_os_str(),
                "--feature".as_ref(),
                feature.to_string().as_ref(),
                "--distance-mm".as_ref(),
                "27".as_ref(),
                "-o".as_ref(),
                output.as_os_str(),
            ]);
            assert_eq!(result.status.code(), Some(2));
            assert!(
                String::from_utf8_lossy(&result.stderr).contains(reason),
                "{result:?}"
            );
            assert_eq!(std::fs::read(&source).expect("unchanged source"), before);
            assert_eq!(
                std::fs::read_dir(root.path())
                    .expect("no scratch or output")
                    .count(),
                1
            );
        }
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
    fn height_frame(
        ctx: &egui::Context,
        edits: &mut Edits,
        path: &Path,
        reading: &ExtrudeEditSource,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(988., 768.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ferritecad_ui::toolbar(
                    ui,
                    ferritecad_ui::Activity {
                        can_open: false,
                        can_export: false,
                        ..Default::default()
                    },
                );
                crate::sketch::Editor::default().draw_choices(ui, false, Some(path), Some(reading));
                edits.draw(ui, false, None);
            },
        );
        out.textures_delta.clear();
        out
    }
    fn height_click(
        ctx: &egui::Context,
        edits: &mut Edits,
        path: &Path,
        reading: &ExtrudeEditSource,
        label: &str,
    ) {
        let out = height_frame(ctx, edits, path, reading, vec![]);
        let at = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if t.galley.text() == label
                        && s.clip_rect.contains_rect(t.visual_bounding_rect()) =>
                {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("visible height control in full viewport");
        for pressed in [true, false] {
            height_frame(
                ctx,
                edits,
                path,
                reading,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }

    #[test]
    fn native_cut_base_height_form_worker_cli_preserve_draft_and_new_names() {
        if !native() {
            return;
        }
        use ferritecad_document::{CircularCut, prepare_circular_cut};
        for n in [4, 16] {
            let root = tempfile::tempdir().expect("root");
            let (source, feature) = make(root.path(), ["80", "50", "12"]);
            let mut d = Document::open(&source).expect("doc");
            let body = ExtrudeEditSource::read(&d).expect("catalogue").cut_bodies[0].body;
            for i in 0..n {
                let slot = i * 7 % 16;
                let p = prepare_circular_cut(
                    &d,
                    body,
                    &CircularCut {
                        center_mm: [10. + (slot % 4) as f64 * 19., 7. + (slot / 4) as f64 * 12.],
                        radius_mm: 1.5 + (i % 5) as f64 * 0.25,
                        extent: ferritecad_document::CutExtent::Blind {
                            depth_mm: if i % 2 == 0 { 12. } else { 3. + (i % 7) as f64 },
                        },
                    },
                )
                .expect("cut");
                d.write_circular_cut(&p).expect("write");
            }
            d.close().expect("close");
            let original = std::fs::read(&source).expect("bytes");
            let reading = opened(&source).edit_source.expect("accepted catalogue");
            let mut e = Edits::default();
            assert!(e.begin(&source, &reading));
            let ctx = egui::Context::default();
            for _ in 0..3 {
                height_frame(&ctx, &mut e, &source, &reading, vec![]);
            }
            let label = e
                .form
                .as_ref()
                .expect("form")
                .shown
                .features
                .iter()
                .find(|f| f.feature == feature)
                .expect("base")
                .label
                .clone();
            height_click(&ctx, &mut e, &source, &reading, &label);
            assert_eq!(e.form.as_ref().expect("selected").shown.distance, "12");
            height_click(&ctx, &mut e, &source, &reading, "12");
            height_frame(
                &ctx,
                &mut e,
                &source,
                &reading,
                vec![
                    egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            command: true,
                            ..Default::default()
                        },
                    },
                    egui::Event::Text("14.25".into()),
                ],
            );
            assert_eq!(
                e.form.as_ref().expect("edited text").shown.distance,
                "14.25"
            );
            let ui = root.path().join("ui.fcad");
            // Save Cancel does not submit a job or consume the form.
            height_click(&ctx, &mut e, &source, &reading, "Save new file…");
            assert!(!e.running());
            assert!(!ui.exists());
            let saved = e
                .form
                .as_ref()
                .expect("Save Cancel keeps form")
                .shown
                .distance
                .clone();
            e.form.as_mut().expect("form").shown.distance = "10".into();
            assert!(e.request(ui.clone()).is_none());
            let first = reading
                .features
                .iter()
                .find(|f| f.feature == feature)
                .expect("base")
                .cut_history
                .as_ref()
                .expect("context")
                .tools[0]
                .feature;
            assert!(
                e.form
                    .as_ref()
                    .expect("invalid draft")
                    .shown
                    .refusal
                    .as_ref()
                    .expect("refusal")
                    .contains(&first.to_string())
            );
            e.form.as_mut().expect("form").shown.distance = saved.clone();
            let occupied = root.path().join("occupied.fcad");
            std::fs::write(&occupied, b"keep").expect("occupied");
            let request = e.request(occupied.clone()).expect("valid request");
            let (tx, rx) = mpsc::channel();
            let g = e
                .start(request, move |r, g, c| {
                    spawn_edit(r, c, move |v| tx.send((g, v)).expect("reply"))
                })
                .expect("start");
            assert!(
                e.finish(g + 1, Err(CadError::input("stale reply")))
                    .is_none()
            );
            assert!(e.running());
            let (g, result) = rx.recv().expect("worker result");
            assert!(result.is_err());
            assert!(e.finish(g, result).is_none());
            assert_eq!(
                e.form
                    .as_ref()
                    .expect("worker refusal keeps draft")
                    .shown
                    .distance,
                saved
            );
            assert_eq!(std::fs::read(occupied).expect("kept"), b"keep");
            let request = e.request(ui.clone()).expect("request");
            let (tx, rx) = mpsc::channel();
            let g = e
                .start(request, move |r, g, c| {
                    spawn_edit(r, c, move |v| tx.send((g, v)).expect("reply"))
                })
                .expect("start");
            assert!(
                e.finish(
                    g + 1,
                    Ok(EditedDocument {
                        destination: ui.clone(),
                        document_id: reading.version.document_id,
                        feature
                    })
                )
                .is_none()
            );
            let (g, result) = rx.recv().expect("result");
            let path = e.finish(g, result).expect("published");
            assert_eq!(path, ui);
            assert!(!e.busy());
            e.draft_load_finished(&root.path().join("unrelated.fcad"), false);
            assert!(e.form.is_none());
            e.draft_load_finished(&ui, false);
            assert_eq!(
                e.form
                    .as_ref()
                    .expect("failed Open restores draft")
                    .shown
                    .distance,
                saved
            );
            // The actual accepted scene uses the new copy and its complete catalogue.
            let accepted = opened(&ui).edit_source.expect("new scene catalogue");
            let current = accepted
                .features
                .iter()
                .find(|f| f.feature == feature)
                .expect("base");
            assert_eq!(current.distance_mm, Some(14.25));
            assert_eq!(
                current.cut_history.as_ref().expect("context").tools.len(),
                n
            );
            assert_eq!(
                accepted
                    .cut_features
                    .iter()
                    .filter(|f| f.saved.is_some())
                    .count(),
                n
            );
            assert!(accepted.sketches.iter().any(|s| s.refusal.is_none()));
            e.cancel();
            e.draft_load_finished(&ui, true);
            let peer = root.path().join("cli.fcad");
            run(&[
                "edit-extrude".as_ref(),
                source.as_os_str(),
                "--feature".as_ref(),
                feature.to_string().as_ref(),
                "--expect-version".as_ref(),
                reading.version.content.to_string().as_ref(),
                "--distance-mm".as_ref(),
                "14.25".as_ref(),
                "-o".as_ref(),
                peer.as_os_str(),
            ]);
            let source_refs = Document::open_read_only(&source)
                .expect("doc")
                .topology_refs()
                .expect("refs");
            let a = Document::open_read_only(&ui)
                .expect("doc")
                .topology_refs()
                .expect("refs");
            let b = Document::open_read_only(&peer)
                .expect("doc")
                .topology_refs()
                .expect("refs");
            let mut mapping = std::collections::BTreeMap::new();
            for r in &a {
                if source_refs.iter().any(|old| old.id == r.id) {
                    assert!(b.contains(r));
                    continue;
                }
                let corresponding: Vec<_> = b
                    .iter()
                    .filter(|q| {
                        if source_refs.iter().any(|old| old.id == q.id) {
                            return false;
                        }
                        let mut q = (*q).clone();
                        q.id = r.id;
                        q == *r
                    })
                    .collect();
                assert_eq!(
                    corresponding.len(),
                    1,
                    "new UUID maps by full producer/origin meaning"
                );
                mapping.insert(
                    corresponding[0].id.to_bytes().to_vec(),
                    r.id.to_bytes().to_vec(),
                );
            }
            assert_eq!(a.len(), b.len());
            assert!(!mapping.is_empty());
            let left = tables(&ui);
            let mut right = tables(&peer);
            for (name, rows) in &mut right {
                if name == "topology_refs" {
                    for row in rows.iter_mut() {
                        if let rusqlite::types::Value::Blob(id) = &mut row[0]
                            && let Some(target) = mapping.get(id)
                        {
                            *id = target.clone();
                        }
                    }
                    rows.sort_by_key(|row| format!("{row:?}"));
                }
            }
            assert_eq!(
                left, right,
                "all SQL cells, only modified_at and genuinely new ref UUIDs mapped"
            );
            for format in ["stl", "fbx"] {
                let mut bytes = Vec::new();
                for path in [&ui, &peer] {
                    let out = path.with_extension(format);
                    run(&[
                        format!("export-{format}").as_ref(),
                        path.as_os_str(),
                        "-o".as_ref(),
                        out.as_os_str(),
                    ]);
                    bytes.push(std::fs::read(out).expect("export"));
                }
                assert_eq!(bytes[0], bytes[1], "worker/CLI {format}");
            }
            assert_eq!(std::fs::read(&source).expect("source"), original);
        }
    }
}
