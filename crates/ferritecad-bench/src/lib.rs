// SPDX-License-Identifier: MIT
pub mod corpus;

use std::path::Path;
use std::time::Instant;

use ferritecad_document::{CacheStore, Document};
use ferritecad_eval::{rebuild_cached, rebuild_cold};
use ferritecad_jobs::{BodySelection, DocumentSession, Existing, HistoryLimits, StlExportRequest};
use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{CadError, Result};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Measurement {
    pub case: String,
    pub bodies: usize,
    pub operation: String,
    pub runs: usize,
    pub median_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

pub fn run(root: &Path, runs: usize, smoke: bool) -> Result<Vec<Measurement>> {
    if runs == 0 {
        return Err(CadError::input("runs must be positive"));
    }
    if !ferritecad_occt::is_available() || !ferritecad_sketch_solver::is_available() {
        return Err(CadError::unsupported(
            "benchmarks require OCCT and PlaneGCS",
        ));
    }
    let cases = corpus::generate(root, smoke)?;
    let context = OperationContext::default();
    let params = TessellationParams::default();
    let mut rows = Vec::new();
    for case in &cases {
        let document = Document::open_read_only(&case.path)?;
        let mut kernel = OcctKernel::new()?;
        let mut cache = CacheStore::open(
            root.join(format!("{}.cache", case.name)),
            document.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )?;
        let warm = rebuild_cached(&document, &mut kernel, &mut cache, &context)?.0;
        ensure_built(warm, &mut kernel)?;
        for operation in [
            "cold_rebuild",
            "warm_rebuild_cached",
            "open",
            "apply_height",
            "apply_fillet",
            "apply_angle",
            "undo",
            "export_stl",
        ] {
            if (operation == "apply_height" && case.extrude.is_none())
                || (operation == "apply_fillet" && case.fillet.is_none())
                || (operation == "apply_angle" && case.revolve.is_none())
            {
                continue;
            }
            let mut samples = Vec::with_capacity(runs);
            for _ in 0..runs {
                // Setup is outside the measured interval: Apply starts with a shown model,
                // Undo starts with one accepted edit, and the cache is already warm.
                let mut session = if operation.starts_with("apply_") || operation == "undo" {
                    let session =
                        DocumentSession::open_in(root, &case.path, HistoryLimits::default())?;
                    session.current().load_scene(
                        &mut kernel,
                        |k, bytes| k.import_step(bytes),
                        &params,
                        &context,
                    )?;
                    Some(session)
                } else {
                    None
                };
                if operation == "undo" {
                    let s = session.as_mut().expect("undo session");
                    let step = apply(
                        s,
                        case,
                        &mut kernel,
                        &context,
                        if case.extrude.is_some() {
                            "apply_height"
                        } else {
                            "apply_angle"
                        },
                    )?;
                    s.commit_step(step)?;
                }
                let stl = root.join("output.stl");
                let start = Instant::now();
                match operation {
                    "cold_rebuild" => {
                        let built = rebuild_cold(&document, &mut kernel, &context)?;
                        ensure_built(built, &mut kernel)?;
                    }
                    "warm_rebuild_cached" => {
                        let built = rebuild_cached(&document, &mut kernel, &mut cache, &context)?.0;
                        ensure_built(built, &mut kernel)?;
                    }
                    "open" => {
                        let scene = ferritecad_scene::snapshot_of(
                            &case.path,
                            &mut kernel,
                            |k, bytes| k.import_step(bytes),
                            &params,
                            &context,
                        )?;
                        std::hint::black_box(scene);
                    }
                    "undo" => {
                        let s = session.as_mut().expect("undo session");
                        let target = s.begin_undo().expect("one accepted step");
                        let scene = ferritecad_scene::snapshot_of(
                            target.path(),
                            &mut kernel,
                            |k, bytes| k.import_step(bytes),
                            &params,
                            &context,
                        )?;
                        s.commit_move(target)?;
                        std::hint::black_box(scene);
                    }
                    "export_stl" => {
                        let result = ferritecad_jobs::export_document_as_stl(
                            StlExportRequest {
                                document: &case.path,
                                destination: &stl,
                                body: BodySelection::Id(case.body),
                                params,
                                existing: Existing::Replace,
                            },
                            OcctKernel::new,
                            &context,
                        )?;
                        std::hint::black_box(result);
                    }
                    operation => {
                        let s = session.as_mut().expect("apply session");
                        let mut step = apply(s, case, &mut kernel, &context, operation)?;
                        let scene = step
                            .take_scene()
                            .ok_or_else(|| CadError::input("Apply returned no scene"))?;
                        s.commit_step(step)?;
                        std::hint::black_box(scene);
                    }
                }
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            samples.sort_by(f64::total_cmp);
            let median = if runs.is_multiple_of(2) {
                (samples[runs / 2 - 1] + samples[runs / 2]) / 2.0
            } else {
                samples[runs / 2]
            };
            rows.push(Measurement {
                case: case.name.clone(),
                bodies: case.bodies,
                operation: operation.to_owned(),
                runs,
                median_ms: median,
                min_ms: samples[0],
                max_ms: samples[runs - 1],
            });
        }
    }
    Ok(rows)
}

fn ensure_built(built: ferritecad_eval::RebuildResult, kernel: &mut OcctKernel) -> Result<()> {
    let healthy = built.all_built();
    let report = built.report();
    built.release_all(kernel);
    if healthy {
        Ok(())
    } else {
        Err(CadError::input(format!(
            "corpus does not build: {report:?}"
        )))
    }
}
fn apply(
    session: &mut DocumentSession,
    case: &corpus::Case,
    kernel: &mut OcctKernel,
    context: &OperationContext,
    operation: &str,
) -> Result<ferritecad_jobs::ProducedStep> {
    let expected = session.current().version();
    let ticket = session.begin_step();
    match operation {
        "apply_height" => ticket.edit_extrude_height(
            case.extrude.expect("extrude"),
            12.0,
            kernel,
            |k, bytes| k.import_step(bytes),
            context,
        ),
        "apply_fillet" => ticket.edit_fillet_radius(
            case.fillet.expect("fillet"),
            2.5,
            expected,
            kernel,
            |k, bytes| k.import_step(bytes),
            context,
        ),
        "apply_angle" => ticket.edit_revolve_angle(
            case.revolve.expect("revolve"),
            200.0,
            expected,
            kernel,
            |k, bytes| k.import_step(bytes),
            context,
        ),
        _ => Err(CadError::input("unknown benchmark operation")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_corpus_smoke() {
        if !ferritecad_fixtures::native_dependencies(&[
            (ferritecad_occt::is_available(), "OCCT"),
            (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
        ]) {
            return;
        }
        let root = tempfile::tempdir().expect("corpus directory");
        let rows = super::run(root.path(), 1, true).expect("all operations");
        assert!(rows.iter().any(|row| row.operation == "apply_height"));
        assert!(rows.iter().any(|row| row.operation == "undo"));
        assert!(rows.iter().any(|row| row.operation == "export_stl"));
        assert!(rows.iter().all(|row| row.runs == 1
            && row.min_ms <= row.median_ms
            && row.median_ms <= row.max_ms));
        serde_json::to_value(rows).expect("JSON output");
    }
}
