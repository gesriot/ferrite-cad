// SPDX-License-Identifier: MIT
use std::path::PathBuf;

mod long;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut out = None;
    let mut corpus = None;
    let mut runs = 5;
    let mut generate_only = false;
    let mut long = false;
    let mut long_case = None;
    let mut worker = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--long" => long = true,
            "--worker" => worker = true,
            "--long-case" => long_case = Some(args.next().ok_or("--long-case needs a case")?),
            "--generate-only" => generate_only = true,
            "--out" => out = Some(PathBuf::from(args.next().ok_or("--out needs a path")?)),
            "--runs" => runs = args.next().ok_or("--runs needs a count")?.parse()?,
            "--corpus-dir" => {
                corpus = Some(PathBuf::from(
                    args.next().ok_or("--corpus-dir needs a path")?,
                ))
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let scratch = tempfile::tempdir()?;
    if generate_only && corpus.is_none() {
        return Err("--generate-only requires --corpus-dir so documents remain available".into());
    }
    if long && corpus.is_none() {
        return Err(
            "--long requires --corpus-dir to retain the measurements and guard logs".into(),
        );
    }
    let root = corpus.as_deref().unwrap_or(scratch.path());
    std::fs::create_dir_all(root)?;
    if runs == 0 {
        return Err("runs must be positive".into());
    }
    if !long && (worker || long_case.is_some()) {
        return Err("--long-case and --worker require --long".into());
    }
    if long {
        if let Some(case) = &long_case {
            long::parse_case(case)?;
        }
        if generate_only {
            for family in [
                ferritecad_bench::corpus::LongFamily::Pockets,
                ferritecad_bench::corpus::LongFamily::Rework,
            ] {
                for count in [100, 300, 1000] {
                    let case = format!("{}_{count}", family.name());
                    if long_case.as_ref().is_none_or(|selected| selected == &case) {
                        ferritecad_bench::corpus::generate_long(root, family, count)?;
                    }
                }
            }
            println!("Long corpus: {}", root.display());
            return Ok(());
        }
        let output = out.ok_or("--long requires --out <file.json>")?;
        if worker {
            long::measure(
                root,
                long_case.as_deref().ok_or("worker needs a case")?,
                runs,
                &output,
            )?;
        } else {
            long::run(root, runs, &output, long_case.as_deref())?;
        }
        return Ok(());
    }
    if generate_only {
        ferritecad_bench::corpus::generate(root, false)?;
        ferritecad_fixtures::write_constrained_fillet_chain(
            &root.join("fillet_pair.fcad"),
            3.0,
            60.0,
            40.0,
        )?;
        ferritecad_fixtures::write_constrained_fillet_chain(
            &root.join("broken_fillet_pair.fcad"),
            25.0,
            20.0,
            20.0,
        )?;
        println!("Corpus: {}", root.display());
        return Ok(());
    }
    let out =
        out.ok_or("usage: ferritecad-bench --out <file.json> [--runs N] [--corpus-dir DIR]")?;
    let rows = ferritecad_bench::run(root, runs, false)?;
    serde_json::to_writer_pretty(std::fs::File::create(out)?, &rows)?;
    println!(
        "{:<30} {:<22} {:>10} {:>10} {:>10} {:>5}",
        "document", "operation", "median ms", "min ms", "max ms", "runs"
    );
    for row in rows {
        println!(
            "{:<30} {:<22} {:>10.3} {:>10.3} {:>10.3} {:>5}",
            row.case, row.operation, row.median_ms, row.min_ms, row.max_ms, row.runs
        );
    }
    Ok(())
}
