// SPDX-License-Identifier: MIT
use std::path::PathBuf;
use std::time::Instant;

use ferritecad_document::{Document, EndCondition, Expression, ObjectPayload};
use ferritecad_eval::rebuild_cold;
use ferritecad_kernel::{OperationContext, SubShapeKind};
use ferritecad_occt::OcctKernel;
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("corpus directory required")?);
    let out = PathBuf::from(args.next().ok_or("output JSON required")?);
    let runs: usize = args.next().unwrap_or_else(|| "5".into()).parse()?;
    if runs == 0 {
        return Err("runs must be positive".into());
    }
    std::fs::create_dir_all(&root)?;
    if !root.join("plate.fcad").exists() {
        ferritecad_bench::corpus::generate(&root, false)?;
        let path = root.join("chamfer.fcad");
        ferritecad_fixtures::write_plate(&path)?;
        let mut d = Document::open(&path)?;
        let b = ferritecad_fixtures::feature_changes::body(&d)?;
        let c = ferritecad_fixtures::feature_changes::edge_addition(&d, b, 0, 2., true)?;
        d.write_changes(&[c])?;
    }
    for (case, center) in [
        ("open_slot", [1., 20.]),
        ("overlapping_tools", [20., 20.]),
        ("pocket", [20., 20.]),
    ] {
        let path = root.join(format!("{case}.fcad"));
        if path.exists() {
            continue;
        }
        ferritecad_fixtures::write_plate(&path)?;
        let mut d = Document::open(&path)?;
        let b = ferritecad_fixtures::feature_changes::body(&d)?;
        let end = if case == "pocket" {
            EndCondition::Blind {
                distance: Expression::constant(4.)?,
            }
        } else {
            EndCondition::ThroughAll
        };
        let mut changes =
            ferritecad_fixtures::feature_changes::circular_tool(&d, b, center, 2., end)?.changes;
        if case == "overlapping_tools" {
            changes.extend(
                ferritecad_fixtures::feature_changes::circular_tool(
                    &d,
                    b,
                    [22., 20.],
                    2.,
                    EndCondition::ThroughAll,
                )?
                .changes,
            );
        }
        d.write_changes(&changes)?;
    }
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
        let mut rebuild_samples = Vec::new();
        let mut report = json!(null);
        for _ in 0..runs {
            let mut kernel = OcctKernel::new()?;
            let start = Instant::now();
            let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())?;
            if !built.all_built() {
                return Err(format!("{case}: {:?}", built.report()).into());
            }
            let rebuild_ms = start.elapsed().as_secs_f64() * 1000.;
            let model = built.topology();
            let mut counts = [0; 3];
            let mut ambiguous = Vec::new();
            let mut features = Vec::new();
            for (id, result) in &model.features {
                let n = [SubShapeKind::Face, SubShapeKind::Edge, SubShapeKind::Vertex]
                    .map(|k| result.topology.entities(k).count());
                for i in 0..3 {
                    counts[i] += n[i];
                }
                features
                    .push(json!({"feature": id, "faces": n[0], "edges": n[1], "vertices": n[2]}));
                for (name, count, reason) in result.ambiguous() {
                    ambiguous.push(json!({
                        "feature": id,
                        "kind": name.kind.as_str(),
                        "name": name.digest,
                        "count": count,
                        "reason": reason,
                    }));
                }
            }
            let mut references = 0;
            for object in document.objects()? {
                let (previous, edges) = match object.payload {
                    ObjectPayload::Fillet(f) => (f.previous, f.edges),
                    ObjectPayload::Chamfer(c) => (c.previous, c.edges),
                    _ => continue,
                };
                model.resolve(
                    ferritecad_document::EntityKind::Edge,
                    object.id,
                    previous,
                    &edges,
                )?;
                references += edges.len();
            }
            report = json!({
                "faces": counts[0],
                "edges": counts[1],
                "vertices": counts[2],
                "features": features,
                "ambiguous": ambiguous,
                "resolved_references": references,
            });
            rebuild_samples.push(rebuild_ms);
            built.release_all(&mut kernel);
        }
        rows.push(json!({
            "case": case,
            "rebuild_ms": rebuild_samples,
            "report": report,
        }));
    }
    serde_json::to_writer_pretty(std::fs::File::create(out)?, &rows)?;
    Ok(())
}
