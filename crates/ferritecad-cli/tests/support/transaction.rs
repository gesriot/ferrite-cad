// SPDX-License-Identifier: MIT
#![allow(dead_code)] // Different integration targets use different request helpers.
use std::{path::Path, process::Command};

pub fn point(p: [f64; 2]) -> serde_json::Value {
    serde_json::json!({"x":{"value":p[0],"unit":"mm"},"y":{"value":p[1],"unit":"mm"}})
}
pub fn circle(
    sketch: &serde_json::Value,
    curve: &serde_json::Value,
    center: [f64; 2],
    radius: f64,
) -> serde_json::Value {
    serde_json::json!({"kind":"set_curve_geometry", "sketch":sketch, "curve":curve,
        "geometry":{"kind":"circle", "center":point(center), "radius":{"value":radius,"unit":"mm"}}})
}
pub fn line_changes(sketch: &serde_json::Value, points: &[[f64; 2]]) -> serde_json::Value {
    let curves = sketch["curves"].as_array().expect("fixture curves");
    assert_eq!(curves.len(), points.len());
    let changes: Vec<_> = curves.iter().enumerate().map(|(i, curve)| serde_json::json!({
        "kind":"set_curve_geometry", "sketch":sketch["sketch_id"], "curve":curve["curve_id"],
        "geometry":{"kind":"line", "start":point(points[i]), "end":point(points[(i+1)%points.len()])}
    })).collect();
    serde_json::json!({"request_version":1,"changes":changes})
}

pub fn write(path: &Path, object: &str, kind: &str, value: f64, unit: &str) {
    std::fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({
            "request_version": 1,
            "changes": [{"kind":"set_expression","object":object,"slot":{"kind":kind},"value":value,"unit":unit}]
        }))
        .expect("JSON"),
    )
    .expect("request");
}
pub fn command(source: &Path, version: &str, request: &Path, output: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferritecad"));
    command
        .arg("edit")
        .arg(source)
        .arg("--expect-version")
        .arg(version)
        .arg("--request")
        .arg(request)
        .arg("--output")
        .arg(output)
        .arg("--json");
    command
}

pub fn address(request: &Path, object: &str) {
    let Ok(bytes) = std::fs::read(request) else {
        return;
    };
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    if let Some(changes) = value
        .get_mut("changes")
        .and_then(serde_json::Value::as_array_mut)
    {
        for change in changes {
            change["object"] = object.into();
        }
        std::fs::write(request, serde_json::to_vec(&value).expect("JSON"))
            .expect("address request");
    }
}

pub fn slot<'a>(
    catalog: &'a serde_json::Value,
    object: &serde_json::Value,
    kind: &str,
) -> &'a serde_json::Value {
    catalog["expression_slots"]
        .as_array()
        .expect("slots")
        .iter()
        .find(|s| &s["object"] == object && s["slot"]["kind"] == kind)
        .expect("slot")
}
pub fn first_slot<'a>(catalog: &'a serde_json::Value, kind: &str) -> &'a serde_json::Value {
    catalog["expression_slots"]
        .as_array()
        .expect("slots")
        .iter()
        .find(|s| s["slot"]["kind"] == kind)
        .expect("slot")
}

pub struct RequestCommand {
    command: Command,
    _request: tempfile::NamedTempFile,
}
impl std::ops::Deref for RequestCommand {
    type Target = Command;
    fn deref(&self) -> &Command {
        &self.command
    }
}
impl std::ops::DerefMut for RequestCommand {
    fn deref_mut(&mut self) -> &mut Command {
        &mut self.command
    }
}
pub fn constant(
    source: &Path,
    version: &str,
    object: &str,
    kind: &str,
    value: &str,
    unit: &str,
    output: &Path,
) -> RequestCommand {
    let request = tempfile::NamedTempFile::new().expect("request file");
    std::fs::write(request.path(), format!(r#"{{"request_version":1,"changes":[{{"kind":"set_expression","object":"{object}","slot":{{"kind":"{kind}"}},"value":{value},"unit":"{unit}"}}]}}"#)).expect("request");
    RequestCommand {
        command: command(source, version, request.path(), output),
        _request: request,
    }
}

pub fn number(value: f64, unit: &str) -> serde_json::Value {
    serde_json::json!({"value":value,"unit":unit})
}
pub fn point_ref(curve: &serde_json::Value, at: &str) -> serde_json::Value {
    serde_json::json!({"curve":curve,"at":at})
}
pub fn segment(curve: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({"from":point_ref(curve,"start"),"to":point_ref(curve,"end")})
}
pub fn constraints(
    sketch: &serde_json::Value,
    remove: &[serde_json::Value],
    rules: &[serde_json::Value],
) -> serde_json::Value {
    use serde_json::json;
    let id = &sketch["sketch_id"];
    let mut changes: Vec<_> = remove
        .iter()
        .map(|constraint| json!({"kind":"remove_constraint","sketch":id,"constraint":constraint}))
        .collect();
    let mut add = |rule: &serde_json::Value| {
        changes.push(json!({"kind":"add_constraint","sketch":id,"constraint":{"id":ferritecad_types::StableEntityId::new(),"rule":rule}}))
    };
    // Line fixtures make their existing closed joints explicit for the solver.
    if !rules.is_empty() && sketch["constraints"].as_array().is_some_and(Vec::is_empty) {
        let curves = sketch["curves"].as_array().expect("curves");
        for (i, c) in curves.iter().enumerate() {
            let next = &curves[(i + 1) % curves.len()];
            if c["geometry"]["kind"] == "line"
                && c["geometry"]["end_mm"] == next["geometry"]["start_mm"]
            {
                add(
                    &json!({"kind":"coincident","a":point_ref(&c["curve_id"],"end"),"b":point_ref(&next["curve_id"],"start")}),
                );
            }
        }
    }
    for rule in rules {
        add(rule);
    }
    json!({"request_version":1,"changes":changes})
}
pub fn sketch_address(request: &Path, id: &serde_json::Value) {
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(request).expect("request")).expect("JSON");
    for change in value["changes"].as_array_mut().expect("changes") {
        change["sketch"] = id.clone();
    }
    std::fs::write(request, serde_json::to_vec(&value).expect("JSON")).expect("request");
}
pub fn solve(path: &Path) -> serde_json::Value {
    let document = ferritecad_document::Document::open_read_only(path).expect("document");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = ferritecad_eval::rebuild_cold(&document, &mut kernel, &Default::default())
        .expect("cold rebuild");
    let value = built.solve_reports().next().map(|(_,r)| serde_json::json!({"degrees_of_freedom":r.degrees_of_freedom(),"redundant_constraint_ids":r.redundant()})).unwrap_or(serde_json::Value::Null);
    built.release_all(&mut kernel);
    value
}
pub fn line_rule(curve: &serde_json::Value, kind: &str) -> serde_json::Value {
    serde_json::json!({"kind":kind,"a":point_ref(curve,"start"),"b":point_ref(curve,"end")})
}
pub fn distance(curve: &serde_json::Value, mm: f64) -> serde_json::Value {
    let mut rule = line_rule(curve, "distance");
    rule["distance"] = number(mm, "mm");
    rule
}
pub fn fixed(curve: &serde_json::Value, at: &str, x: f64, y: f64) -> serde_json::Value {
    serde_json::json!({"kind":"fixed","point":point_ref(curve,at),"x":number(x,"mm"),"y":number(y,"mm")})
}
pub fn relation(a: &serde_json::Value, b: &serde_json::Value, kind: &str) -> serde_json::Value {
    serde_json::json!({"kind":kind,"a":segment(a),"b":segment(b)})
}
