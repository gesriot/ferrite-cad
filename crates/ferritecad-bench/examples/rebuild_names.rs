// SPDX-License-Identifier: MIT
use ferritecad_document::{CacheStore, Document};
use ferritecad_eval::{rebuild_cached, rebuild_cold};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use ferritecad_occt::OcctKernel;
use serde_json::json;
use std::{path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("persisted corpus required")?);
    let out = PathBuf::from(args.next().ok_or("output JSON required")?);
    let runs: usize = args.next().unwrap_or_else(|| "5".into()).parse()?;
    if runs == 0 {
        return Err("positive sample count required".into());
    }
    let caches = out.with_extension("caches");
    std::fs::create_dir(&caches)?;
    let mut rows = Vec::new();
    for case in [
        "plate",
        "plate_16_cuts",
        "10_bodies",
        "50_bodies",
        "plate_4_fillets_constraints",
        "revolve",
        "chamfer",
        "open_slot",
        "overlapping_tools",
        "pocket",
    ] {
        let document = Document::open_read_only(root.join(format!("{case}.fcad")))?;
        let context = OperationContext::default();
        let mut cold = Vec::new();
        let mut warm = Vec::new();
        let mut outcomes = Vec::new();
        for sample in 0..=runs {
            let mut kernel = OcctKernel::new()?;
            let start = Instant::now();
            let built = rebuild_cold(&document, &mut kernel, &context)?;
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            if !built.all_built() {
                return Err(format!("{case}: {:?}", built.report()).into());
            }
            built.release_all(&mut kernel);
            if sample > 0 {
                cold.push(elapsed);
            }
        }
        let mut kernel = OcctKernel::new()?;
        let mut cache = CacheStore::open(
            caches.join(format!("{case}.cache")),
            document.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )?;
        let (prime, events) = rebuild_cached(&document, &mut kernel, &mut cache, &context)?;
        if !prime.all_built() {
            return Err(format!("{case}: cache priming failed: {events:?}").into());
        }
        prime.release_all(&mut kernel);
        for _ in 0..runs {
            let mut kernel = OcctKernel::new()?;
            let start = Instant::now();
            let (built, events) = rebuild_cached(&document, &mut kernel, &mut cache, &context)?;
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            if !built.all_built() {
                return Err(format!("{case}: warm rebuild failed: {events:?}").into());
            }
            built.release_all(&mut kernel);
            outcomes.push(
                events
                    .iter()
                    .map(|e| format!("{:?}", e.outcome))
                    .collect::<Vec<_>>(),
            );
            warm.push(elapsed);
        }
        rows.push(json!({"case":case,"cold_ms":cold,"cached_ms":warm,"cache_outcomes":outcomes}));
    }
    serde_json::to_writer_pretty(std::fs::File::create(out)?, &rows)?;
    Ok(())
}
