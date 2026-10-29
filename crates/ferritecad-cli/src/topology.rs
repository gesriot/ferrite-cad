// SPDX-License-Identifier: MIT
//! Deterministic descriptions of complete feature output names.
use std::fmt::Write;
use std::process::ExitCode;

use ferritecad_document::Document;
use ferritecad_eval::{RebuildResult, rebuild_cold};
use ferritecad_kernel::OperationContext;
use ferritecad_occt::OcctKernel;
use ferritecad_types::Result;

use crate::{DocumentArgs, EXIT_FAILED};

pub fn print_topology(args: DocumentArgs) -> Result<ExitCode> {
    let document = Document::open_read_only(&args.path)?;
    let mut kernel = OcctKernel::new()?;
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())?;
    let status = if built.all_built() {
        ExitCode::SUCCESS
    } else {
        EXIT_FAILED.into()
    };
    let text = report(&built, true);
    built.release_all(&mut kernel);
    print!("{}", text?);
    Ok(status)
}

pub(crate) fn inspect_names(document: &Document) -> String {
    let result = (|| {
        let mut kernel = OcctKernel::new()?;
        let built = rebuild_cold(document, &mut kernel, &OperationContext::default())?;
        let text = report(&built, true);
        built.release_all(&mut kernel);
        text
    })();
    result.unwrap_or_else(|error| format!("topology names unavailable: {error}\n"))
}

/// Every feature's names in order, each with its derivation when `described`.
///
/// `rebuild --cold` leaves the derivations out: they list every node of each
/// name's graph, and a model of fifty bodies would print megabytes of them.
pub(crate) fn report(built: &RebuildResult, described: bool) -> Result<String> {
    let model = built.topology();
    let mut text = String::from("topology names\n");
    for (feature, names) in &model.features {
        writeln!(text, "  feature {feature}").expect("writing to a String cannot fail");
        for (name, entities) in &names.bindings {
            writeln!(
                text,
                "    producer={feature} kind={} digest={} entities={}",
                name.kind,
                name.digest,
                entities.len()
            )
            .expect("writing to a String cannot fail");
            if described {
                writeln!(text, "      {}", model.describe(*feature, *name)?)
                    .expect("writing to a String cannot fail");
            }
        }
    }
    let report = built.report();
    for failed in &report.failed {
        writeln!(text, "  feature {} failed: {}", failed.id, failed.message)
            .expect("writing to a String cannot fail");
    }
    for skipped in &report.skipped {
        writeln!(
            text,
            "  feature {} skipped: depends on {}",
            skipped.id, skipped.cause
        )
        .expect("writing to a String cannot fail");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_document::CacheStore;
    use ferritecad_eval::{CacheOutcome, rebuild_cached};
    use ferritecad_kernel::{GeometryKernel, mock::MockKernel};

    fn cold_and_cache_report(kernel: &mut impl GeometryKernel) {
        let directory = tempfile::tempdir().expect("directory");
        let document = ferritecad_fixtures::open_plate(directory.path()).expect("plate");
        let mut cache = CacheStore::open(
            directory.path().join("names.cache"),
            document.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("cache");
        let context = OperationContext::default();
        let (cold, misses) =
            rebuild_cached(&document, kernel, &mut cache, &context).expect("cold rebuild");
        assert!(cold.all_built());
        assert!(
            misses
                .iter()
                .all(|event| event.outcome == CacheOutcome::Miss)
        );
        let expected = report(&cold, true).expect("cold report");
        assert_eq!(expected.matches(" entities=1").count(), 26);
        cold.release_all(kernel);
        let (warm, hits) =
            rebuild_cached(&document, kernel, &mut cache, &context).expect("cached rebuild");
        assert!(warm.all_built());
        assert!(hits.iter().all(|event| event.outcome == CacheOutcome::Hit));
        assert_eq!(report(&warm, true).expect("warm report"), expected);
        warm.release_all(kernel);
    }

    #[test]
    fn output_names_have_the_same_order_and_description_after_a_cache_hit() {
        cold_and_cache_report(&mut MockKernel::new());
    }

    #[test]
    fn native_output_names_have_the_same_order_and_description_after_a_cache_hit() {
        if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
            return;
        }
        cold_and_cache_report(&mut OcctKernel::new().expect("native kernel"));
    }
}
