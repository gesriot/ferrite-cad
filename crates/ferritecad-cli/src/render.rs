// SPDX-License-Identifier: MIT
//! Human-readable output for the inspection commands.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use ferritecad_document::{
    Access, Dependency, Document, ObjectPayload, ObjectRecord, ValidationReport,
};
use ferritecad_types::{ObjectId, Result};

pub fn inspect(document: &Document) -> Result<()> {
    let meta = document.meta();

    println!("document {}", document.path().display());
    println!("  id                 {}", meta.document_id);
    println!("  content version    {}", document.content_version()?);
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

    print!("\n{}", crate::topology::inspect_names(document));

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
    let order = document.evaluation_order()?;

    // Group rows already read. Push order is the `needs` order, so two roles
    // of one pair stay two lines and nothing is sorted again here.
    let mut needs_by_dependent: HashMap<_, Vec<&Dependency>> = HashMap::new();
    for dep in &deps {
        needs_by_dependent
            .entry(dep.dependent)
            .or_default()
            .push(dep);
    }

    for id in order {
        println!("{} {}", id, label(&names, id));
        if let Some(needs) = needs_by_dependent.get(&id) {
            for dep in needs {
                println!(
                    "    needs {} {} [{}]",
                    dep.dependency,
                    label(&names, dep.dependency),
                    dep.role.as_str()
                );
            }
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
            "profile {}, {}, {}{}",
            e.profile,
            e.end_condition,
            match e.operation {
                ferritecad_document::SolidOperation::NewBody => "NewBody",
                ferritecad_document::SolidOperation::Add => "Add",
                ferritecad_document::SolidOperation::Cut => "Cut",
                ferritecad_document::SolidOperation::Intersect => "Intersect",
                _ => "unsupported operation",
            },
            if e.reversed { ", reversed" } else { "" }
        ),
        ObjectPayload::Revolve(r) => format!(
            "profile {}, {}, {:?}",
            r.profile,
            match &r.extent {
                ferritecad_document::RevolveExtent::FullTurn => "full turn".to_owned(),
                ferritecad_document::RevolveExtent::Partial { angle } =>
                    format!("{}°", ferritecad_types::display_degrees(angle.value())),
                other => format!("{other:?}"),
            },
            r.operation
        ),
        ObjectPayload::KeepSolids(keep) => {
            format!("keep {}, previous {}", keep.selection, keep.previous)
        }
        ObjectPayload::Body(b) => match b.tip_feature {
            Some(tip) => format!("tip feature {tip}"),
            None => "empty".to_owned(),
        },
        ObjectPayload::ImportedStep(i) => format!(
            "scene: {} definitions, {} placements, {} byte(s) of source ({}), read by {}{}",
            i.scene.definition_count(),
            i.scene.instance_count(),
            i.source_byte_len,
            i.source_name.as_deref().unwrap_or("origin not recorded"),
            i.imported_by,
            // Counted, not repeated: `inspect` describes a document's shape,
            // and what a reading reported belongs with that reading.
            match i.diagnostics_at_import.len() {
                0 => String::new(),
                one => format!(", which reported {one} thing(s) at the time"),
            }
        ),
        _ => return None,
    })
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
