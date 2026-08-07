// SPDX-License-Identifier: MIT
//! Human-readable output for the inspection commands.

use std::collections::BTreeMap;
use std::path::Path;

use ferritecad_document::{
    Access, Document, EndCondition, ObjectPayload, ObjectRecord, SemanticRole, ValidationReport,
};
use ferritecad_types::{ObjectId, Result};

pub fn inspect(document: &Document) -> Result<()> {
    let meta = document.meta();

    println!("document {}", document.path().display());
    println!("  id                 {}", meta.document_id);
    println!(
        "  format             v{} (needs reader v{}+)",
        meta.format_version, meta.minimum_reader_version
    );
    println!("  generator          {}", meta.generator);
    println!("  created            {}", meta.created_at);
    println!("  modified           {}", meta.modified_at);
    println!(
        "  display units      {} / {}",
        meta.display_length_unit, meta.display_angle_unit
    );
    match document.access() {
        Access::ReadWrite => println!("  access             read-write"),
        Access::ReadOnly { reason } => println!("  access             read-only, {reason}"),
        other => println!("  access             {other:?}"),
    }

    let cache = document.cache_path();
    println!(
        "  cache sidecar      {} ({})",
        cache.display(),
        if cache.exists() { "present" } else { "absent" }
    );

    let objects = document.objects()?;
    println!("\nobjects ({})", objects.len());
    for object in &objects {
        println!(
            "  {} {:<16} {}",
            object.id,
            object.payload.type_name(),
            object.name.as_deref().unwrap_or("-")
        );
        if let Some(detail) = describe(&object.payload) {
            println!("      {detail}");
        }
    }

    let deps = document.dependencies()?;
    println!("\ndependencies ({})", deps.len());
    for dep in &deps {
        println!(
            "  {} -> {} [{}]",
            dep.dependent,
            dep.dependency,
            dep.role.as_str()
        );
    }

    let refs = document.topology_refs()?;
    println!("\ntopology references ({})", refs.len());
    for reference in &refs {
        println!(
            "  {} {} of {} :: {}",
            reference.id,
            reference.expected_kind.as_str(),
            reference.producer_feature,
            describe_role(&reference.output_role)
        );
    }

    println!("\nevaluation order");
    let names = name_index(&objects);
    for (position, id) in document.evaluation_order()?.iter().enumerate() {
        println!("  {:>3}. {} {}", position + 1, id, label(&names, *id));
    }

    Ok(())
}

pub fn graph_text(document: &Document) -> Result<()> {
    let objects = document.objects()?;
    let names = name_index(&objects);
    let deps = document.dependencies()?;

    for id in document.evaluation_order()? {
        println!("{} {}", id, label(&names, id));
        for dep in deps.iter().filter(|d| d.dependent == id) {
            println!(
                "    needs {} {} [{}]",
                dep.dependency,
                label(&names, dep.dependency),
                dep.role.as_str()
            );
        }
    }
    Ok(())
}

pub fn graph_dot(document: &Document) -> Result<()> {
    let objects = document.objects()?;
    let names = name_index(&objects);

    println!("digraph features {{");
    println!("  rankdir=LR;");
    println!("  node [shape=box, fontname=\"sans-serif\"];");

    for object in &objects {
        println!(
            "  \"{}\" [label=\"{}\\n{}\"];",
            object.id,
            escape(object.name.as_deref().unwrap_or("-")),
            escape(object.payload.type_name())
        );
    }

    // Edges point from dependency to dependent, so the drawing reads in the
    // direction the model is evaluated.
    for dep in document.dependencies()? {
        println!(
            "  \"{}\" -> \"{}\" [label=\"{}\"];",
            dep.dependency,
            dep.dependent,
            escape(dep.role.as_str())
        );
    }

    println!("}}");
    let _ = names;
    Ok(())
}

pub fn validation(path: &Path, report: &ValidationReport) {
    for diagnostic in &report.diagnostics {
        let location = diagnostic
            .object
            .map(|id| format!(" [{id}]"))
            .unwrap_or_default();
        println!(
            "{}: {}{}: {}",
            diagnostic.severity.as_str(),
            diagnostic.code,
            location,
            diagnostic.message
        );
    }

    let errors = report.errors().count();
    let warnings = report.warnings().count();

    if report.is_ok() {
        println!(
            "{} is valid ({} warning{})",
            path.display(),
            warnings,
            if warnings == 1 { "" } else { "s" }
        );
    } else {
        println!(
            "{} has {} error{} and {} warning{}",
            path.display(),
            errors,
            if errors == 1 { "" } else { "s" },
            warnings,
            if warnings == 1 { "" } else { "s" }
        );
    }
}

fn describe(payload: &ObjectPayload) -> Option<String> {
    Some(match payload {
        ObjectPayload::Parameter(p) => format!(
            "{} = {} ({}, {})",
            p.name,
            p.expression.source,
            p.expression.value(),
            p.dimension
        ),
        ObjectPayload::Sketch(s) => format!("on plane {}, {} curves", s.plane, s.curves.len()),
        ObjectPayload::Extrude(e) => format!(
            "profile {}, {}, {:?}{}",
            e.profile,
            match &e.end_condition {
                EndCondition::Blind { distance } => format!("blind {} mm", distance.value()),
                EndCondition::Symmetric { distance } =>
                    format!("symmetric {} mm", distance.value()),
                EndCondition::ThroughAll => "through all".to_owned(),
                other => format!("{other:?}"),
            },
            e.operation,
            if e.reversed { ", reversed" } else { "" }
        ),
        ObjectPayload::Body(b) => match b.tip_feature {
            Some(tip) => format!("tip feature {tip}"),
            None => "empty".to_owned(),
        },
        ObjectPayload::Unknown(u) => format!(
            "preserved verbatim: {} v{}, requires {}",
            u.type_name,
            u.schema_version,
            if u.required_capabilities.is_empty() {
                "nothing".to_owned()
            } else {
                u.required_capabilities.join(", ")
            }
        ),
        _ => return None,
    })
}

fn describe_role(role: &SemanticRole) -> String {
    match role {
        SemanticRole::SketchSegment { segment } => format!("sketch segment {segment}"),
        SemanticRole::ExtrudeCap { side } => format!("extrude cap {side:?}"),
        SemanticRole::ExtrudeSide { profile_segment } => {
            format!("extrude side from segment {profile_segment}")
        }
        SemanticRole::FilletFace { source_edge } => format!("fillet face from edge {source_edge}"),
        other => format!("{other:?}"),
    }
}

fn name_index(objects: &[ObjectRecord]) -> BTreeMap<ObjectId, String> {
    objects
        .iter()
        .map(|o| {
            (
                o.id,
                o.name
                    .clone()
                    .unwrap_or_else(|| o.payload.type_name().to_owned()),
            )
        })
        .collect()
}

fn label(names: &BTreeMap<ObjectId, String>, id: ObjectId) -> &str {
    names.get(&id).map(String::as_str).unwrap_or("?")
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
