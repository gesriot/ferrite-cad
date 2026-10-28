// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use ferritecad_bench::corpus::{LongCase, LongFamily, generate_long};
use ferritecad_document::{CacheStore, Document, ObjectPayload, ObjectRecord};
use ferritecad_eval::{
    CacheOutcome, boolean_archive_key, extrude_archive_key, extrude_request, plane_from_datum,
    profile_from_sketch, rebuild_cached, rebuild_cold,
};
use ferritecad_kernel::{BooleanKind, BooleanRequest, GeometryKernel, OperationContext};
use ferritecad_occt::OcctKernel;
use ferritecad_topology::generative::{FeatureNaming, ModelNaming, Name};
use ferritecad_topology::{ARCHIVE_CACHE_KIND, archive_feature};
use ferritecad_types::{CanonicalHasher, ContentHash, ObjectId};
use serde::{Deserialize, Serialize};

mod observed;
mod process;
pub(crate) use process::run;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Counts {
    derivations: usize,
    ancestry_members: usize,
    ancestry_rows: usize,
    empty_ancestry_rows: usize,
    entities: usize,
    unique_derivations: usize,
    distinct_derivation_allocations: usize,
    tip_entities: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Sample {
    cold_ms: f64,
    kernel_ms: f64,
    naming_replay_ms: f64,
    naming_windows_ms: [f64; 3],
    cache_ms: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Report {
    case: String,
    cuts: usize,
    features: usize,
    samples: Vec<Sample>,
    counts: Counts,
    cache_entry_bytes: u64,
    last_entry_bytes: u64,
    cache_file_bytes: u64,
    complete: bool,
}

fn save(path: &Path, report: &Report) -> Result<()> {
    let temporary = path.with_extension("partial");
    serde_json::to_writer_pretty(std::fs::File::create(&temporary)?, report)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

pub(crate) fn parse_case(value: &str) -> Result<(LongFamily, usize)> {
    let (family, count) = value
        .rsplit_once('_')
        .ok_or("expected pockets_N or rework_N")?;
    let family = match family {
        "pockets" => LongFamily::Pockets,
        "rework" => LongFamily::Rework,
        _ => return Err("expected pockets_N or rework_N".into()),
    };
    let count = count.parse()?;
    if ![100, 300, 1000].contains(&count) {
        return Err("long cases use N = 100, 300 or 1000".into());
    }
    Ok((family, count))
}

pub(crate) fn measure(root: &Path, case: &str, runs: usize, output: &Path) -> Result<()> {
    let (family, count) = parse_case(case)?;
    let report = measure_case(root, family, count, runs, output)?;
    save(output, &report)
}

fn measure_case(
    root: &Path,
    family: LongFamily,
    count: usize,
    runs: usize,
    output: &Path,
) -> Result<Report> {
    if runs == 0 {
        return Err("runs must be positive".into());
    }
    eprintln!("phase=generate feature=0/{count}");
    let case = generate_long(root, family, count)?;
    let document = Document::open_read_only(&case.path)?;
    let objects: BTreeMap<_, _> = document.objects()?.into_iter().map(|o| (o.id, o)).collect();
    let mut report = Report {
        case: format!("{}_{count}", family.name()),
        cuts: count,
        features: case.features.len(),
        ..Report::default()
    };
    let cache_path = root.join("history.cache");
    let context = OperationContext::default();
    let mut expected = None;
    for run in 0..runs {
        eprintln!("sample={}/{}", run + 1, runs);
        let mut sample = {
            let mut kernel = observed::Observed::new("cold", count)?;
            let start = Instant::now();
            let cold = rebuild_cold(&document, &mut kernel, &context)?;
            let sample = Sample {
                cold_ms: ms(start),
                kernel_ms: kernel.build_ms,
                ..Sample::default()
            };
            require_built(&cold)?;
            if matches!(family, LongFamily::Rework) {
                let named = &cold.topology().features;
                let first_count = named[&case.features[1]].entities.len();
                if case.features[1..]
                    .iter()
                    .any(|id| named[id].entities.len() != first_count)
                {
                    return Err(
                        "rework must retain constant live topology after its first Cut".into(),
                    );
                }
            }
            let signatures = signatures(cold.topology());
            if let Some(expected) = &expected {
                if &signatures != expected {
                    return Err("cold names changed between samples".into());
                }
            } else {
                report.counts =
                    counts(cold.topology(), *case.features.last().ok_or("empty chain")?);
                expected = Some(signatures);
            }
            cold.release_all(&mut kernel);
            sample
        };
        save(output, &report)?;

        // No second live model: profile the same public naming calls in a fresh session.
        {
            let mut kernel = OcctKernel::new()?;
            let mut cache = CacheStore::open(
                &cache_path,
                document.meta().document_id,
                kernel.identity().id(),
                kernel.identity().version(),
            )?;
            let profile = replay(&case, &objects, &mut kernel, &mut cache, run == 0)?;
            if Some(&profile.signatures) != expected.as_ref() {
                return Err("profile replay and evaluator names disagree".into());
            }
            sample.naming_replay_ms = profile.naming_ms.iter().sum::<f64>() + profile.prepare_ms;
            sample.naming_windows_ms = windows(&profile.naming_ms, count);
            if run == 0 {
                report.cache_entry_bytes = profile.cache_bytes;
                report.last_entry_bytes = profile.last_bytes;
            }
        }
        report.cache_file_bytes = std::fs::metadata(&cache_path)?.len();
        save(output, &report)?;

        let mut kernel = observed::Observed::new("cache", count)?;
        let mut cache = CacheStore::open(
            &cache_path,
            document.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )?;
        let start = Instant::now();
        let (warm, events) = rebuild_cached(&document, &mut kernel, &mut cache, &context)?;
        sample.cache_ms = ms(start);
        require_built(&warm)?;
        if events.len() != case.features.len()
            || events.iter().any(|e| e.outcome != CacheOutcome::Hit)
        {
            return Err("every feature must be an exact cache Hit".into());
        }
        if Some(&signatures_of(&warm)) != expected.as_ref() {
            return Err("cache and cold names or ancestry disagree".into());
        }
        warm.release_all(&mut kernel);
        report.samples.push(sample);
        save(output, &report)?;
    }
    report.complete = true;
    Ok(report)
}

fn require_built(built: &ferritecad_eval::RebuildResult) -> Result<()> {
    if built.all_built() {
        return Ok(());
    }
    for failure in built.report().failed {
        eprintln!("feature {}: {}", failure.id, failure.message);
    }
    Err("long history has an unbuilt feature".into())
}

fn signatures_of(built: &ferritecad_eval::RebuildResult) -> BTreeMap<ObjectId, ContentHash> {
    signatures(built.topology())
}

fn counts(model: &ModelNaming, tip: ObjectId) -> Counts {
    let mut out = Counts::default();
    let mut unique = BTreeSet::new();
    let mut allocations = BTreeSet::new();
    for named in model.features.values() {
        out.derivations += named.derivations.len();
        out.ancestry_rows += named.ancestry.len();
        out.ancestry_members += named.ancestry.values().map(BTreeSet::len).sum::<usize>();
        out.empty_ancestry_rows += named.ancestry.values().filter(|s| s.is_empty()).count();
        out.entities += named.entities.len();
        unique.extend(named.derivations.keys().copied());
        allocations.extend(named.derivations.values().map(std::sync::Arc::as_ptr));
    }
    out.unique_derivations = unique.len();
    out.distinct_derivation_allocations = allocations.len();
    out.tip_entities = model.features[&tip].entities.len();
    out
}

fn signatures(model: &ModelNaming) -> BTreeMap<ObjectId, ContentHash> {
    model
        .features
        .iter()
        .map(|(id, named)| (*id, signature(named)))
        .collect()
}

fn signature(named: &FeatureNaming) -> ContentHash {
    let mut h = CanonicalHasher::new("benchmark.long.names");
    let feed = |h: &mut CanonicalHasher, name: Name| {
        h.bytes(name.kind.as_str().as_bytes()).hash(&name.digest);
    };
    h.field("derivations").u64(named.derivations.len() as u64);
    for digest in named.derivations.keys() {
        h.hash(digest);
    }
    let mut entities: Vec<_> = named.entities.values().copied().collect();
    entities.sort();
    h.field("entities").u64(entities.len() as u64);
    for name in entities {
        feed(&mut h, name);
    }
    h.field("ancestry").u64(named.ancestry.len() as u64);
    for (ancestor, members) in &named.ancestry {
        feed(&mut h, *ancestor);
        let mut names: Vec<_> = members.iter().map(|s| named.entities[s]).collect();
        names.sort();
        h.u64(names.len() as u64);
        for name in names {
            feed(&mut h, name);
        }
    }
    h.finish()
}

struct Replay {
    prepare_ms: f64,
    naming_ms: Vec<f64>,
    signatures: BTreeMap<ObjectId, ContentHash>,
    cache_bytes: u64,
    last_bytes: u64,
}

fn replay(
    case: &LongCase,
    objects: &BTreeMap<ObjectId, ObjectRecord>,
    kernel: &mut OcctKernel,
    cache: &mut CacheStore,
    store: bool,
) -> Result<Replay> {
    let mut model = ModelNaming::new();
    let start = Instant::now();
    model.prepare(objects)?;
    let mut out = Replay {
        prepare_ms: ms(start),
        naming_ms: Vec::new(),
        signatures: BTreeMap::new(),
        cache_bytes: 0,
        last_bytes: 0,
    };
    let context = OperationContext::default();
    let mut shapes = BTreeMap::new();
    let mut keys = BTreeMap::new();
    for (index, id) in case.features.iter().enumerate() {
        eprintln!(
            "phase=naming feature={index}/{} object={id}",
            case.features.len() - 1
        );
        let ObjectPayload::Extrude(extrude) = &objects[id].payload else {
            return Err("long history contains a non-extrusion feature".into());
        };
        let ObjectPayload::Sketch(sketch) = &objects[&extrude.profile].payload else {
            return Err("long history has no sketch".into());
        };
        let ObjectPayload::DatumPlane(plane) = &objects[&sketch.plane].payload else {
            return Err("long history has no plane".into());
        };
        let profile = profile_from_sketch(sketch, extrude.profile, plane_from_datum(plane)?)?
            .profile
            .select(&extrude.regions)?;
        let start = Instant::now();
        model.select_profile(*id, &profile)?;
        let mut naming_ms = ms(start);
        let request = extrude_request(extrude, profile, None)?;
        let geometry_key = extrude_archive_key(kernel.identity(), &request, &context);
        let tool = kernel.extrude(&request, &context)?;
        let start = Instant::now();
        let tool_names = model.name_operation(*id, tool.shape, &[], kernel)?;
        naming_ms += ms(start);
        let (shape, names, key) = if let Some(previous) = extrude.previous {
            let request = BooleanRequest::new(BooleanKind::Cut, shapes[&previous], tool.shape)?;
            let shape = kernel.boolean(&request, &context)?.shape;
            let start = Instant::now();
            let names = model.name_operation(
                *id,
                shape,
                &[&model.features[&previous], &tool_names],
                kernel,
            )?;
            naming_ms += ms(start);
            let key = boolean_archive_key(
                kernel.identity(),
                BooleanKind::Cut,
                previous,
                &keys[&previous],
                &geometry_key,
                false,
                &context,
            );
            (shape, names, key)
        } else {
            (tool.shape, tool_names, geometry_key)
        };
        let start = Instant::now();
        model.insert_feature(*id, names)?;
        naming_ms += ms(start);
        out.naming_ms.push(naming_ms);
        shapes.insert(*id, shape);
        let key = model.cache_key(*id, key);
        keys.insert(*id, key);
        if store {
            eprintln!(
                "phase=archive feature={index}/{} object={id}",
                case.features.len() - 1
            );
            let archive = archive_feature(kernel, &model, *id)?;
            let bytes = archive.encode()?;
            cache.put(*id, key, ARCHIVE_CACHE_KIND, &bytes)?;
            let stored = cache
                .get(*id, key, ARCHIVE_CACHE_KIND)?
                .ok_or("missing stored archive")?;
            if stored.bytes != bytes {
                return Err("archive bytes changed in cache".into());
            }
            out.last_bytes = bytes.len() as u64;
            out.cache_bytes += out.last_bytes;
        }
    }
    out.signatures = signatures(&model);
    // The caller drops this fresh kernel session, including every retained tool and result.
    Ok(out)
}

fn windows(samples: &[f64], count: usize) -> [f64; 3] {
    [count / 10, count / 2, count].map(|k| {
        // Ten trailing Cuts, k-9 through k; position zero is the plate.
        let end = k.max(10);
        samples[end - 9..=end].iter().sum::<f64>() / 10.
    })
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_short_histories_build_and_restore_all_names() {
        if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
            return;
        }
        let mut tip_counts = Vec::new();
        for family in [LongFamily::Pockets, LongFamily::Rework] {
            let dir = tempfile::tempdir().expect("history directory");
            let report = measure_case(dir.path(), family, 10, 1, &dir.path().join("report.json"))
                .expect("cold, naming replay and exact cache hits agree");
            assert_eq!(report.features, 11);
            assert!(report.complete);
            assert!(report.counts.derivations > report.counts.unique_derivations);
            assert!(report.cache_entry_bytes > report.last_entry_bytes);
            tip_counts.push(report.counts.tip_entities);
        }
        assert!(
            tip_counts[0] > tip_counts[1],
            "only independent pockets grow the live boundary"
        );
    }
}
