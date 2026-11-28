// SPDX-License-Identifier: MIT
use ferritecad_document::{DocumentVersion, EditChange};
use std::path::Path;

pub(crate) fn cli_edit(from: &Path, to: &Path, version: DocumentVersion, changes: &[EditChange]) {
    let request = to.with_extension("json");
    std::fs::write(
        &request,
        ferritecad_fixtures::edit_request_json(changes).to_string(),
    )
    .expect("write CLI request");
    let output = std::process::Command::new(crate::creates::tests::ferritecad())
        .arg("edit")
        .arg(from)
        .arg("--expect-version")
        .arg(version.content.to_string())
        .arg("--request")
        .arg(request)
        .arg("-o")
        .arg(to)
        .output()
        .expect("CLI edit");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn export(path: &Path, format: &str) -> Vec<u8> {
    let out = tempfile::tempdir().expect("export directory");
    let file = out.path().join(format!("model.{format}"));
    let result = std::process::Command::new(crate::creates::tests::ferritecad())
        .arg(format!("export-{format}"))
        .arg(path)
        .arg("-o")
        .arg(&file)
        .output()
        .expect("CLI export");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::read(file).expect("exported bytes")
}
