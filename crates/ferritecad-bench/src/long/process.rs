// SPDX-License-Identifier: MIT
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use super::{Report, Result};

pub(crate) fn run(root: &Path, runs: usize, output: &Path, only: Option<&str>) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Err("guarded long measurements require the macOS memory watchdog".into());
    }
    let cases = [
        "pockets_100",
        "pockets_300",
        "pockets_1000",
        "rework_100",
        "rework_300",
        "rework_1000",
    ];
    if let Some(case) = only {
        super::parse_case(case)?;
    }
    let mut results = Vec::new();
    for case in cases
        .into_iter()
        .filter(|case| only.is_none_or(|s| s == *case))
    {
        let directory = root.join(case);
        std::fs::create_dir(&directory)?;
        let directory = directory.canonicalize()?;
        let log = directory.join("watch.jsonl");
        let data = directory.join("result.json");
        let watchdog =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/watch-viewer-memory.py");
        let libraries = std::env::var("DYLD_LIBRARY_PATH").unwrap_or_default();
        let status = Command::new("python3")
            .arg(watchdog)
            .args(["--log"])
            .arg(&log)
            .args(["--limit-mib", "6144", "--seconds", "1800", "--"])
            .args(["/usr/bin/time", "-l", "/bin/bash", "-c"])
            // Protected macOS executables clear DYLD; pass the value as data, not shell code.
            .arg("export DYLD_LIBRARY_PATH=\"$1\"; shift; exec \"$@\"")
            .arg("long-history")
            .arg(libraries)
            .arg(std::env::current_exe()?)
            .args(["--long", "--worker", "--long-case", case, "--runs"])
            .arg(runs.to_string())
            .arg("--corpus-dir")
            .arg(&directory)
            .arg("--out")
            .arg(&data)
            .status()?;
        let stderr = std::fs::read_to_string(log.with_extension("stderr"))?;
        let events: Vec<Value> = std::fs::read_to_string(&log)?
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        let peak_rss_sample = events
            .iter()
            .filter_map(|event| {
                Some(
                    event["processes"]
                        .as_array()?
                        .iter()
                        .filter_map(|p| p["rss"].as_u64())
                        .sum::<u64>(),
                )
            })
            .max();
        let peak_footprint = events
            .iter()
            .filter_map(|e| e["footprint_sum"].as_u64())
            .max();
        let max_rss = stderr
            .lines()
            .find(|s| s.contains("maximum resident set size"))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<u64>().ok());
        let aborted = events
            .iter()
            .find(|e| e["event"] == "aborted" || e["event"] == "refused");
        let report: Option<Report> = if data.exists() {
            Some(serde_json::from_reader(std::fs::File::open(&data)?)?)
        } else {
            None
        };
        let complete = status.success() && report.as_ref().is_some_and(|r| r.complete);
        let last_feature = stderr.lines().rev().find(|s| s.starts_with("phase="));
        let summary = report.as_ref().map(summarize);
        results.push(json!({
            "case": case, "complete": complete, "exit_code": status.code(),
            "report": report, "medians": summary, "max_rss_bytes": max_rss,
            "sampled_rss_bytes": peak_rss_sample, "sampled_footprint_bytes": peak_footprint,
            "last_feature": last_feature, "abort": aborted, "directory": directory,
        }));
        let growth = exponents(&results);
        serde_json::to_writer_pretty(
            std::fs::File::create(output)?,
            &json!({
                "cases": results, "growth": growth,
            }),
        )?;
        println!(
            "{case}: {}",
            if complete {
                "complete"
            } else {
                "incomplete; see guard log"
            }
        );
        if !complete {
            let resource_limit = aborted.is_some_and(|e| {
                matches!(
                    e["reason"].as_str(),
                    Some("physical footprint limit" | "time limit")
                )
            });
            let hundred_timeout =
                case.ends_with("_100") && aborted.is_some_and(|e| e["reason"] == "time limit");
            if !resource_limit || hundred_timeout {
                return Err(format!(
                    "{case} stopped at {}; inspect {}",
                    last_feature.unwrap_or("startup"),
                    directory.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn summarize(report: &Report) -> Value {
    json!({
        "cold_ms": median(report.samples.iter().map(|s| s.cold_ms).collect()),
        "kernel_ms": median(report.samples.iter().map(|s| s.kernel_ms).collect()),
        "naming_ms": median(report.samples.iter().map(|s| s.naming_replay_ms).collect()),
        "cache_ms": median(report.samples.iter().map(|s| s.cache_ms).collect()),
        "naming_k10_ms": median(report.samples.iter().map(|s| s.naming_windows_ms[0]).collect()),
        "naming_k50_ms": median(report.samples.iter().map(|s| s.naming_windows_ms[1]).collect()),
        "naming_k100_ms": median(report.samples.iter().map(|s| s.naming_windows_ms[2]).collect()),
    })
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.
    } else {
        values[mid]
    })
}

fn exponents(results: &[Value]) -> Vec<Value> {
    let mut growth = Vec::new();
    for family in ["pockets", "rework"] {
        let cases: Vec<_> = results
            .iter()
            .filter(|r| r["case"].as_str().is_some_and(|s| s.starts_with(family)))
            .collect();
        for pair in cases.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let mut metrics = serde_json::Map::new();
            if a["complete"] == true && b["complete"] == true {
                let ratio = (b["report"]["cuts"].as_f64().expect("cut count")
                    / a["report"]["cuts"].as_f64().expect("cut count"))
                .ln();
                for pointer in [
                    "/medians/cold_ms",
                    "/medians/kernel_ms",
                    "/medians/naming_ms",
                    "/medians/cache_ms",
                    "/medians/naming_k10_ms",
                    "/medians/naming_k50_ms",
                    "/medians/naming_k100_ms",
                    "/report/counts/derivations",
                    "/report/counts/ancestry_members",
                    "/report/counts/ancestry_rows",
                    "/report/counts/empty_ancestry_rows",
                    "/report/counts/entities",
                    "/report/counts/unique_derivations",
                    "/report/counts/tip_entities",
                    "/report/counts/distinct_derivation_allocations",
                    "/report/cache_entry_bytes",
                    "/report/last_entry_bytes",
                    "/report/cache_file_bytes",
                    "/max_rss_bytes",
                ] {
                    let exponent = a
                        .pointer(pointer)
                        .and_then(Value::as_f64)
                        .zip(b.pointer(pointer).and_then(Value::as_f64))
                        .filter(|(a, b)| *a > 0. && *b > 0.)
                        .map(|(a, b)| (b / a).ln() / ratio);
                    metrics.insert(pointer.to_owned(), json!(exponent));
                }
            }
            growth.push(json!({"from": a["case"], "to": b["case"], "exponents": metrics}));
        }
    }
    growth
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_preserves_missing_measurements_and_distinguishes_linear_from_quadratic() {
        let cases = [100., 300., 1000.].map(|n| {
            json!({
                "case": format!("rework_{}", n as usize), "complete": true,
                "report": { "cuts": n, "counts": { "derivations": n * n, "entities": n } },
            })
        });
        for growth in exponents(&cases) {
            let metrics = &growth["exponents"];
            assert!(
                (metrics["/report/counts/derivations"]
                    .as_f64()
                    .expect("quadratic")
                    - 2.)
                    .abs()
                    < 1e-10
            );
            assert!(
                (metrics["/report/counts/entities"].as_f64().expect("linear") - 1.).abs() < 1e-10
            );
            assert!(metrics["/max_rss_bytes"].is_null());
        }
    }
}
