// SPDX-License-Identifier: MIT
#![allow(dead_code)] // Different integration targets use different request helpers.
use std::{path::Path, process::Command};

pub fn write(path: &Path, object: &str, kind: &str, value: f64, unit: &str) {
    std::fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({
            "request_version": 1,
            "changes": [{"object":object,"slot":{"kind":kind},"value":value,"unit":unit}]
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
    std::fs::write(request.path(), format!(r#"{{"request_version":1,"changes":[{{"object":"{object}","slot":{{"kind":"{kind}"}},"value":{value},"unit":"{unit}"}}]}}"#)).expect("request");
    RequestCommand {
        command: command(source, version, request.path(), output),
        _request: request,
    }
}
